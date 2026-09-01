//! The IPC surface. Every function here is registered in `main`.
//!
//! The device is opened per call rather than held, so the RØDECaster App can
//! stay running alongside.

use std::sync::atomic::Ordering;

use rcp_model::command::Set;
use rcp_model::model::{CellState, Model};
use rcp_transport::Device;

use crate::dto::{MatrixDto, Patch, StateArg};
use crate::listener::{frame_dto, started, LogFrames, Shared};
use crate::project::from_cache;

/// A fader slide emits an event per cell per step, so this is off by default.
#[tauri::command]
pub(crate) fn set_frame_logging(flag: tauri::State<'_, LogFrames>, enabled: bool) {
    flag.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
pub(crate) fn read_matrix(shared: tauri::State<'_, Shared>) -> Result<MatrixDto, String> {
    from_cache(&shared)
}

#[tauri::command]
pub(crate) fn set_cell(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    row: usize,
    col: usize,
    state: StateArg,
) -> Result<MatrixDto, String> {
    use tauri::Emitter;

    let target: CellState = state.into();

    // Leaving mute needs two commands, and which ones depends on where the cell
    // is now, so read that from the cache rather than asking the console again.
    let (id, current, master_mute) = {
        let guard = shared.lock().map_err(|e| e.to_string())?;
        let model = guard.as_ref().ok_or("device state not loaded yet")?;
        let cell = model
            .cell(row, col)
            .ok_or_else(|| format!("no cell at row {row}, column {col}"))?;
        let master_mute = model
            .strips()
            .iter()
            .any(|c| c.source == row as i32 && c.mute);
        (cell.id, cell.state(), master_mute)
    };

    let dev = Device::open()?;
    for step in Set::steps_to(target, Some(current)) {
        // While the strip is master muted every cell already reads muted on the
        // device. Sending an un-mute here would punch a hole in that mute, so
        // only the link half of the change is applied.
        if master_mute && matches!(step, Set::Mute | Set::Unmute) {
            continue;
        }
        dev.send_cell(id, step)?;
        let _ = app.emit(
            "protocol-frame",
            frame_dto("out", &rcp_model::command::cell_frame(id, step), true, started()),
        );

        // The console echoes each change; back-to-back writes need a beat.
        std::thread::sleep(std::time::Duration::from_millis(30));
    }

    // The listener will apply the console's echo and emit matrix-changed; this
    // return is just so the caller has something immediately.
    from_cache(&shared)
}

#[tauri::command]
pub(crate) fn set_output_mode(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    col: usize,
    mode: i32,
) -> Result<MatrixDto, String> {
    use tauri::Emitter;

    let id = {
        let guard = shared.lock().map_err(|e| e.to_string())?;
        let model = guard.as_ref().ok_or("device state not loaded yet")?;
        if col >= model.outputs {
            return Err(format!("column {col} is out of range"));
        }
        model.mixminus_base + col as u32
    };

    Device::open()?.send_output_mode(id, mode)?;
    let _ = app.emit(
        "protocol-frame",
        frame_dto("out", &rcp_model::command::output_mode_frame(id, mode), true, started()),
    );

    from_cache(&shared)
}

/// Record a change the console will not report back.
///
/// Cell writes come back as `mixMute`, so the listener keeps those current on
/// its own. `OUTPUT` broadcasts only changes made on the console itself, so a
/// monitor write from here is invisible to the listener and the UI would
/// revert to the stale value unless it is recorded at the source.
pub(crate) fn record_unreported(
    app: &tauri::AppHandle,
    shared: &Shared,
    apply: impl FnOnce(&mut Model),
    patch: Patch,
) -> Result<MatrixDto, String> {
    use tauri::Emitter;

    {
        let mut guard = shared.lock().map_err(|e| e.to_string())?;
        if let Some(model) = guard.as_mut() {
            apply(model);
        }
    }

    let _ = app.emit("matrix-patch", patch);
    from_cache(shared)
}

/// Mute or unmute the studio monitor output.
///
/// Worked out rather than captured, since the RODECaster App has no monitor
/// control, then confirmed against the event the console pushes.
#[tauri::command]
pub(crate) fn set_monitor_mute(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    mute: bool,
) -> Result<MatrixDto, String> {
    use tauri::Emitter;

    let id = {
        let guard = shared.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or("device state not loaded yet")?.output_id
    };

    Device::open()?.send_monitor_mute(id, mute)?;
    let _ = app.emit(
        "protocol-frame",
        frame_dto("out", &rcp_model::command::monitor_mute_frame(id, mute), true, started()),
    );

    record_unreported(
        &app,
        &shared,
        |m| m.info.monitor_mute = Some(mute),
        Patch::MonitorMute { mute },
    )
}

/// Set the studio monitor volume, 0.0-1.0.
///
/// Worked out like `set_monitor_mute`, then checked against the pushed event.
#[tauri::command]
pub(crate) fn set_monitor_level(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    level: f64,
) -> Result<MatrixDto, String> {
    use tauri::Emitter;

    if !level.is_finite() {
        return Err(format!("{level} is not a level"));
    }

    // The console stores an f32, so record what it will actually hold rather
    // than the double that was asked for.
    let level = (level.clamp(0.0, 1.0) as f32) as f64;

    let id = {
        let guard = shared.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or("device state not loaded yet")?.output_id
    };

    Device::open()?.send_monitor_level(id, level)?;
    let _ = app.emit(
        "protocol-frame",
        frame_dto("out", &rcp_model::command::monitor_level_frame(id, level), true, started()),
    );

    record_unreported(
        &app,
        &shared,
        |m| m.info.monitor_level = Some(level),
        Patch::MonitorLevel { level },
    )
}

/// Set one input source's colour, as `#rrggbb` or `aarrggbb`.
///
/// Writes the console's own show state, so the colour persists on the hardware.
#[tauri::command]
pub(crate) fn set_input_colour(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Shared>,
    row: usize,
    colour: String,
) -> Result<MatrixDto, String> {
    use tauri::Emitter;

    let hex = rcp_model::command::normalise_argb(&colour)?;

    let id = {
        let guard = shared.lock().map_err(|e| e.to_string())?;
        let model = guard.as_ref().ok_or("device state not loaded yet")?;
        if row >= model.input_colours.len() {
            return Err(format!("input source {row} is out of range"));
        }
        model.inputsource_base + row as u32
    };

    Device::open()?.send_input_colour(id, &hex)?;
    let _ = app.emit(
        "protocol-frame",
        frame_dto(
            "out",
            &rcp_model::command::input_colour_frame(id, &hex)?,
            true,
            started(),
        ),
    );

    // Like the OUTPUT properties, the console does not report this back; the
    // colour capture recorded three writes and zero events in reply.
    let reported = hex.clone();
    record_unreported(
        &app,
        &shared,
        move |m| {
            if let Some(slot) = m.input_colours.get_mut(row) {
                *slot = Some(hex);
            }
        },
        Patch::InputColour { row, colour: reported },
    )
}
