#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri backend for the RØDECaster routing matrix.
//!
//! All protocol work lives in the rcp-* crates; this only shapes it for the UI.
//! The device is opened per operation rather than held, so the RØDECaster App
//! can stay running alongside.

mod commands;
mod dto;
mod listener;
mod project;
mod rta;

use commands::{
    read_matrix, set_cell, set_frame_logging, set_input_colour, set_monitor_level,
    set_monitor_mute, set_output_mode,
};
use listener::{spawn_listener, LogFrames, Shared};
use rta::{list_capture_devices, start_rta, stop_rta, Rta};
use tauri::Manager;
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// Register the app to start with the desktop, but only the first time it runs.
///
/// A marker file records that the default has been applied, so turning the
/// toggle off sticks instead of being re-enabled on the next launch.
fn apply_default_autostart(app: &tauri::AppHandle) {
    let Ok(dir) = app.path().app_config_dir() else {
        return;
    };
    let marker = dir.join("autostart-default-applied");
    if marker.exists() {
        return;
    }

    if let Err(e) = app.autolaunch().enable() {
        eprintln!("could not register autostart: {e}");
        return;
    }

    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(&marker, b"");
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        .manage(Shared::new(None))
        .manage(LogFrames::new(false))
        .manage(Rta::new(None))
        .setup(|app| {
            spawn_listener(app.handle().clone());
            apply_default_autostart(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            read_matrix,
            set_cell,
            set_output_mode,
            set_monitor_mute,
            set_monitor_level,
            set_input_colour,
            set_frame_logging,
            list_capture_devices,
            start_rta,
            stop_rta
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

