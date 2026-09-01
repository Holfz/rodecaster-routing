//! Following the console's pushed events.
//!
//! The only concurrent code in the crate: the reader thread, the mutex holding
//! the model, and the logging flag all live here.

use std::sync::atomic::Ordering;

use rcp_model::labels;
use rcp_model::model::Model;
use rcp_transport::Device;

use crate::dto::{state_name, FrameDto, Patch};

/// The model the listener keeps current, so the UI never has to re-read.
pub(crate) type Shared = std::sync::Mutex<Option<Model>>;

/// Whether to forward every wire frame to the protocol page. Off unless that
/// page is open: a fader slide produces a frame per cell per step.
pub(crate) type LogFrames = std::sync::atomic::AtomicBool;

pub(crate) fn patch_for(model: &Model, changed: rcp_model::model::Changed) -> Option<Patch> {
    use rcp_model::model::Changed;
    Some(match changed {
        Changed::Cell(index) => {
            let cell = model.cells.get(index)?;
            Patch::Cell {
                row: cell.row,
                col: cell.col,
                state: state_name(cell.state()),
                level: cell.level,
                level_steps: cell.level_steps(),
                strip_level: model.strip_level(cell.row as i32),
                strip_level_steps: model.strip_level_steps(cell.row as i32),
            }
        }
        Changed::OutputMode(col) => {
            let mode = model.output_modes.get(col).copied()?;
            Patch::OutputMode {
                col,
                mode,
                mode_label: labels::OutputMode::from_wire(mode).label(),
                custom: labels::OutputMode::from_wire(mode).is_custom(),
            }
        }
        Changed::ChannelMute(index) => {
            let ch = model.channels.iter().find(|c| c.index == index)?;
            Patch::ChannelMute { index, source: ch.source, mute: ch.mute }
        }
        Changed::MonitorMute => Patch::MonitorMute { mute: model.info.monitor_mute? },
        Changed::MonitorLevel => Patch::MonitorLevel { level: model.info.monitor_level? },
        Changed::EncoderColour => Patch::EncoderColour { colour: model.info.encoder_colour? },
        Changed::InputColour(row) => {
            Patch::InputColour { row, colour: model.input_colours.get(row).cloned().flatten()? }
        }
    })
}

pub(crate) fn frame_dto(dir: &'static str, bytes: &[u8], applied: bool, since: std::time::Instant) -> FrameDto {
    let parsed = rcp_proto::Frame::parse(bytes);
    let hex: String = bytes
        .iter()
        // Reports are zero-padded to 256; the tail carries nothing.
        .take(parsed.as_ref().map_or(bytes.len(), |f| 5 + f.payload_len as usize))
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");

    match parsed {
        Some(f) => FrameDto {
            at: since.elapsed().as_millis(),
            dir,
            name: f.name.clone(),
            id: f.id_hex(),
            id_num: f.id.iter().rev().fold(0u32, |a, b| (a << 8) | *b as u32),
            values: f.values_str(),
            hex,
            applied,
        },
        None => FrameDto {
            at: since.elapsed().as_millis(),
            dir,
            name: "<unparsed>".into(),
            id: String::new(),
            id_num: 0,
            values: String::new(),
            hex,
            applied: false,
        },
    }
}

/// Wall clock for the protocol log, shared by the listener and the commands.
static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
pub(crate) fn started() -> std::time::Instant {
    *START.get_or_init(std::time::Instant::now)
}

/// Follow the console's pushed events and tell the UI when something changes.
///
/// One 136 KB read establishes the baseline; after that the event stream keeps
/// the UI live with no polling, including changes made on the touchscreen.
pub(crate) fn spawn_listener(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};

    std::thread::spawn(move || {
        let mut backoff = std::time::Duration::from_secs(1);
        loop {
            let Ok(dev) = Device::open() else {
                // The console may be unplugged or held exclusively; retry
                // slowly rather than spinning on it.
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(std::time::Duration::from_secs(15));
                continue;
            };
            backoff = std::time::Duration::from_secs(1);

            match dev.read_model() {
                Ok(model) => {
                    *app.state::<Shared>().lock().unwrap() = Some(model);
                    let _ = app.emit("matrix-changed", ());
                }
                Err(_) => {
                    std::thread::sleep(backoff);
                    continue;
                }
            }

            let shared = app.state::<Shared>();
            loop {
                // Short timeout so a burst's trailing update is emitted
                // promptly rather than waiting for the next event.
                match dev.next_event(100) {
                    Ok(Some(frame)) => {
                        let patch = {
                            let mut guard = shared.lock().unwrap();
                            match guard.as_mut() {
                                Some(model) => rcp_model::model::apply_event(model, &frame)
                                    .and_then(|c| patch_for(model, c)),
                                None => None,
                            }
                        };

                        // Log every frame, applied or not: the unapplied ones
                        // are how new events get discovered.
                        if app.state::<LogFrames>().load(Ordering::Relaxed) {
                            let _ = app.emit(
                                "protocol-frame",
                                frame_dto("in", &frame.raw_report(), patch.is_some(), started()),
                            );
                        }

                        // Patches are tiny and touch one cell, so they go out
                        // immediately - no coalescing needed for these.
                        if let Some(patch) = patch {
                            let _ = app.emit("matrix-patch", patch);
                        }
                    }
                    Ok(None) => {}
                    // A read error means the handle is gone; reopen from the top.
                    Err(_) => break,
                }
            }
        }
    });
}
