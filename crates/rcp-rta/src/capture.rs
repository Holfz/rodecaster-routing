//! The cpal input stream, on a thread of its own.
//!
//! `cpal::Stream` is not `Send`, so it cannot be parked in shared state the way
//! the HID handle is. A thread owns it instead and lives exactly as long as the
//! stream: `Capture` is a handle that stops that thread when it is dropped.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};

use crate::analyzer::{Analyzer, Frame};

/// How often the owning thread checks whether it has been asked to stop. It is
/// also unparked on the way out, so this is only a backstop.
const POLL: Duration = Duration::from_millis(200);

/// A capture endpoint as Windows lists it, the same set a chat client offers.
pub struct DeviceInfo {
    pub name: String,
    /// The host's default input, which is what a chat client starts on.
    pub default: bool,
    pub channels: u16,
    pub sample_rate: u32,
}

/// What the stream actually opened as, which is not always what was asked for.
pub struct StreamInfo {
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// Band centre frequencies, so the UI can label its axis.
    pub centres: Vec<f32>,
}

/// What the capture thread reports back.
pub enum Event {
    Frame(Frame),
    /// The stream failed after it started, typically the device going away.
    Error(String),
}

pub fn input_devices() -> Result<Vec<DeviceInfo>, String> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| d.name().ok());

    let devices = host.input_devices().map_err(|e| e.to_string())?;

    Ok(devices
        .filter_map(|device| {
            let name = device.name().ok()?;
            let config = device.default_input_config().ok()?;
            Some(DeviceInfo {
                default: Some(&name) == default.as_ref(),
                name,
                channels: config.channels(),
                sample_rate: config.sample_rate().0,
            })
        })
        .collect())
}

/// A running capture. Dropping it closes the stream.
pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Capture {
    /// Open `wanted` by name, or the best guess when it is `None`.
    ///
    /// `on_event` runs on the audio thread, so it must not block: send the
    /// frame somewhere and return.
    pub fn start(
        wanted: Option<&str>,
        on_event: impl FnMut(Event) + Send + 'static,
    ) -> Result<(Self, StreamInfo), String> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let wanted = wanted.map(str::to_owned);
        let (tx, rx) = mpsc::channel();

        let thread = std::thread::Builder::new()
            .name("rta-capture".into())
            .spawn(move || match open(wanted.as_deref(), on_event) {
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
                Ok((stream, info)) => {
                    if let Err(e) = stream.play() {
                        let _ = tx.send(Err(e.to_string()));
                        return;
                    }
                    let _ = tx.send(Ok(info));

                    while !flag.load(Ordering::Relaxed) {
                        std::thread::park_timeout(POLL);
                    }
                }
            })
            .map_err(|e| e.to_string())?;

        match rx.recv() {
            Ok(Ok(info)) => Ok((Self { stop, thread: Some(thread) }, info)),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the capture thread stopped before it opened a stream".into()),
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

fn open(
    wanted: Option<&str>,
    on_event: impl FnMut(Event) + Send + 'static,
) -> Result<(cpal::Stream, StreamInfo), String> {
    let host = cpal::default_host();
    let device = pick(&host, wanted)?;
    let name = device.name().map_err(|e| e.to_string())?;

    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = config.channels as usize;

    let analyzer = Analyzer::new(config.sample_rate.0 as f32);
    let centres = analyzer.centres().to_vec();

    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, channels, analyzer, on_event),
        SampleFormat::I16 => build::<i16>(&device, &config, channels, analyzer, on_event),
        SampleFormat::I32 => build::<i32>(&device, &config, channels, analyzer, on_event),
        SampleFormat::U16 => build::<u16>(&device, &config, channels, analyzer, on_event),
        other => return Err(format!("{name} captures as {other}, which is not supported")),
    }
    .map_err(|e| format!("could not open {name}: {e}"))?;

    let info = StreamInfo {
        device: name,
        sample_rate: config.sample_rate.0,
        channels: config.channels,
        centres,
    };

    Ok((stream, info))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    mut analyzer: Analyzer,
    on_event: impl FnMut(Event) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    // One closure holds both callbacks, so the shared handle is what makes a
    // stream error reach the same place a frame does.
    let sink = Arc::new(std::sync::Mutex::new(on_event));
    let failed = sink.clone();

    let mut mono: Vec<f32> = Vec::new();

    device.build_input_stream(
        config,
        move |data: &[T], _| {
            mono.clear();
            mono.extend(data.chunks(channels).map(|frame| {
                // A source panned centre sits on both channels, so the average
                // keeps it at its own level rather than 6 dB under.
                frame.iter().map(|&v| f32::from_sample(v)).sum::<f32>() / channels as f32
            }));

            let Ok(mut emit) = sink.lock() else { return };
            analyzer.push(&mono, |frame| emit(Event::Frame(frame)));
        },
        move |e| {
            if let Ok(mut emit) = failed.lock() {
                emit(Event::Error(e.to_string()));
            }
        },
        None,
    )
}

fn pick(host: &cpal::Host, wanted: Option<&str>) -> Result<cpal::Device, String> {
    let devices: Vec<cpal::Device> = host.input_devices().map_err(|e| e.to_string())?.collect();
    let names: Vec<String> = devices.iter().filter_map(|d| d.name().ok()).collect();

    if devices.len() != names.len() {
        return Err("a capture device would not report its name".into());
    }

    let chosen = match wanted {
        Some(want) => names
            .iter()
            .position(|n| n == want)
            .ok_or_else(|| format!("capture device \"{want}\" is not connected"))?,
        None => preferred(&names).ok_or("this machine has no capture device")?,
    };

    Ok(devices.into_iter().nth(chosen).expect("index came from the same list"))
}

/// Which endpoint to start on when none has been chosen.
///
/// Comms first: it is the bus that can carry the microphone alone without
/// disturbing the main mix. Failing that any RØDECaster endpoint, then whatever
/// is first, which on WASAPI is the host's default input.
fn preferred(names: &[String]) -> Option<usize> {
    let matching = |needles: &[&str]| {
        names.iter().position(|name| {
            let lower = name.to_lowercase();
            needles.iter().all(|needle| lower.contains(needle))
        })
    };

    matching(&["rodecaster", "chat"])
        .or_else(|| matching(&["rødecaster", "chat"]))
        .or_else(|| matching(&["rodecaster"]))
        .or_else(|| matching(&["rødecaster"]))
        .or(if names.is_empty() { None } else { Some(0) })
}

#[cfg(test)]
mod tests {
    use super::preferred;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn comms_wins_over_the_main_bus() {
        let list = names(&[
            "Microphone (Steam Streaming Microphone)",
            "Microphone (RODECaster Pro II Main Multitrack)",
            "Microphone (RODECaster Pro II Chat)",
        ]);

        assert_eq!(preferred(&list), Some(2));
    }

    #[test]
    fn any_console_endpoint_beats_an_unrelated_one() {
        let list = names(&[
            "Microphone (Webcam)",
            "Microphone (RODECaster Pro II Main Multitrack)",
        ]);

        assert_eq!(preferred(&list), Some(1));
    }

    /// RØDE spells the name both ways across its endpoints.
    #[test]
    fn the_slashed_o_matches_too() {
        let list = names(&["Microphone (Webcam)", "Chat (RØDECaster)"]);

        assert_eq!(preferred(&list), Some(1));
    }

    #[test]
    fn with_no_console_the_first_device_is_taken() {
        assert_eq!(preferred(&names(&["Microphone (Webcam)"])), Some(0));
        assert_eq!(preferred(&[]), None);
    }
}
