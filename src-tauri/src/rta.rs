//! The real-time analyser's IPC surface.
//!
//! Read-only, and nothing here reaches the console: the spectrum comes off a
//! Windows capture endpoint, the same list a chat client offers as microphones.
//! What is audible on a given endpoint is whatever the matrix routes to that
//! USB output, so the analyser follows the routing rather than the protocol.
//!
//! Capture only runs while the RTA page is open, the way frame logging does.

use std::sync::Mutex;

use rcp_rta::{Capture, Event};
use tauri::Emitter;

use crate::dto::{CaptureDeviceDto, RtaFrameDto, RtaInfoDto};

/// The running capture, or `None` while the page is closed. Dropping it closes
/// the stream and releases the endpoint for anything else that wants it.
pub(crate) type Rta = Mutex<Option<Capture>>;

#[tauri::command]
pub(crate) fn list_capture_devices() -> Result<Vec<CaptureDeviceDto>, String> {
    Ok(rcp_rta::input_devices()?
        .into_iter()
        .map(|d| CaptureDeviceDto { name: d.name, default: d.default })
        .collect())
}

/// Open a capture endpoint and start pushing `rta-frame`.
///
/// `device` is a name from `list_capture_devices`; `None` picks the console's
/// comms endpoint when it is there.
#[tauri::command]
pub(crate) fn start_rta(
    app: tauri::AppHandle,
    state: tauri::State<'_, Rta>,
    device: Option<String>,
) -> Result<RtaInfoDto, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;

    // Close the old stream before opening the next, or switching endpoints
    // would hold both.
    *guard = None;

    let (tx, rx) = std::sync::mpsc::channel();
    let (capture, info) = Capture::start(device.as_deref(), move |event| {
        let _ = tx.send(event);
    })?;

    // The callback above runs on the audio thread, so serialising a frame and
    // handing it to the webview happens here instead. The channel closes when
    // the capture is dropped, which is what ends this thread.
    let handle = app.clone();
    std::thread::spawn(move || {
        for event in rx {
            let _ = match event {
                Event::Frame(f) => handle.emit(
                    "rta-frame",
                    RtaFrameDto {
                        db: f.db.into_iter().map(tenth).collect(),
                        peak_db: tenth(f.peak_db),
                        clipped: f.clipped,
                    },
                ),
                Event::Error(e) => handle.emit("rta-error", e),
            };
        }
    });

    *guard = Some(capture);

    Ok(RtaInfoDto {
        device: info.device,
        sample_rate: info.sample_rate,
        channels: info.channels,
        centres: info.centres.into_iter().map(tenth).collect(),
    })
}

#[tauri::command]
pub(crate) fn stop_rta(state: tauri::State<'_, Rta>) -> Result<(), String> {
    *state.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

/// A tenth of a decibel is finer than the curve can be read, and it keeps the
/// 256 numbers in a frame short on the way through the IPC.
fn tenth(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}
