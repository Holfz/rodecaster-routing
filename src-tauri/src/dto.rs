//! The shapes the frontend sees.
//!
//! These mirror `ui/app/types.ts` field for field. Keeping them in one file is
//! what makes drift between the two visible.

use rcp_model::model::CellState;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CellDto {
    pub(crate) col: usize,
    pub(crate) id: u32,
    pub(crate) state: &'static str,
    /// 0.0-1.0, the slider on the app's routing page.
    pub(crate) level: Option<f64>,
    /// The same level on the console's own 0-127 scale.
    pub(crate) level_steps: Option<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RowDto {
    pub(crate) row: usize,
    pub(crate) label: String,
    pub(crate) colour: String,
    /// Fader driving this input row, or null when no fader is assigned to it.
    pub(crate) fader: Option<usize>,
    /// The driving strip's master mute. It silences the input on every
    /// output without changing any cell, so the cells below stay as the
    /// console reports them and only this flag marks it.
    pub(crate) master_mute: bool,
    pub(crate) cells: Vec<CellDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OutputDto {
    pub(crate) col: usize,
    pub(crate) label: String,
    pub(crate) mode: i32,
    pub(crate) mode_label: String,
    /// Only Custom outputs honour their routing column.
    pub(crate) custom: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChannelDto {
    pub(crate) index: usize,
    pub(crate) label: String,
    pub(crate) colour: String,
    pub(crate) source: i32,
    pub(crate) mute: bool,
    pub(crate) cue: bool,
    pub(crate) talkback: bool,
    pub(crate) bypass_processing: bool,
    pub(crate) pan: Option<f64>,
    pub(crate) fx_preset: i32,
    /// Fader level 0.0-1.0, gathered from the row's linked cells.
    pub(crate) level: Option<f64>,
    pub(crate) level_steps: Option<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InfoDto {
    pub(crate) firmware: Option<String>,
    pub(crate) serial: Option<String>,
    pub(crate) mixer_build: Option<String>,
    pub(crate) sample_rate: Option<f64>,
    pub(crate) buffer_size: Option<i32>,
    pub(crate) record_label: String,
    pub(crate) storage: String,
    pub(crate) network: Option<String>,
    pub(crate) ssid: Option<String>,
    pub(crate) show: Option<String>,
    pub(crate) usb1_connected: Option<bool>,
    /// The studio monitor mute. `None` until the console has reported it.
    pub(crate) monitor_mute: Option<bool>,
    /// Monitor volume, 0.0-1.0. Position only; the scale is not established.
    pub(crate) monitor_level: Option<f64>,
    /// Encoder ring colour. An index into a palette we do not have, so it is
    /// shown as the number the console reports.
    pub(crate) encoder_colour: Option<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MatrixDto {
    /// The sixteen colours the console will accept, so the UI offers exactly
    /// those rather than a free picker the device would reject.
    pub(crate) palette: Vec<String>,
    pub(crate) outputs: Vec<OutputDto>,
    pub(crate) rows: Vec<RowDto>,
    pub(crate) channels: Vec<ChannelDto>,
    pub(crate) info: InfoDto,
    pub(crate) warnings: Vec<String>,
    pub(crate) mix_base: u32,
    pub(crate) read_ms: u128,
}

/// `recordState` enum. Only 3 has been observed (idle), so the rest are shown
/// as raw numbers rather than guessed at.
pub(crate) fn record_label(state: Option<i32>, ms: Option<i32>) -> String {
    match state {
        Some(3) => "Idle".into(),
        Some(n) => {
            let secs = ms.unwrap_or(0) / 1000;
            format!("state {n} · {}:{:02}", secs / 60, secs % 60)
        }
        None => "unknown".into(),
    }
}

pub(crate) fn storage_label(inserted: Option<bool>, free: Option<i32>, capacity: Option<i32>) -> String {
    match inserted {
        Some(false) | None => "No card".into(),
        Some(true) => match (free, capacity) {
            (Some(f), Some(c)) if c > 0 => {
                format!("{:.1} / {:.1} GB free", f as f64 / 1e9, c as f64 / 1e9)
            }
            _ => "Card inserted".into(),
        },
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum StateArg {
    Linked,
    Unlinked,
    Muted,
}

impl From<StateArg> for CellState {
    fn from(s: StateArg) -> CellState {
        match s {
            StateArg::Linked => CellState::Linked,
            StateArg::Unlinked => CellState::Unlinked,
            StateArg::Muted => CellState::Muted,
        }
    }
}

pub(crate) fn state_name(s: CellState) -> &'static str {
    match s {
        CellState::Linked => "linked",
        CellState::Unlinked => "unlinked",
        CellState::Muted => "muted",
    }
}

/// A single change, so the UI can patch one cell instead of rebuilding the
/// whole matrix.
///
/// A fader slide sends one event per cell, and re-reading ~400 cells for each
/// made the level bars stutter.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum Patch {
    #[serde(rename_all = "camelCase")]
    Cell {
        row: usize,
        col: usize,
        state: &'static str,
        level: Option<f64>,
        level_steps: Option<i32>,
        /// The strip's level is gathered from its row's cells, so it travels with
        /// the cell that changed rather than needing a separate round trip.
        strip_level: Option<f64>,
        strip_level_steps: Option<i32>,
    },
    #[serde(rename_all = "camelCase")]
    OutputMode {
        col: usize,
        mode: i32,
        mode_label: String,
        custom: bool,
    },
    #[serde(rename_all = "camelCase")]
    ChannelMute {
        index: usize,
        /// The input row this strip drives, so the matrix column can be marked.
        source: i32,
        mute: bool,
    },
    #[serde(rename_all = "camelCase")]
    MonitorMute { mute: bool },
    #[serde(rename_all = "camelCase")]
    MonitorLevel { level: f64 },
    #[serde(rename_all = "camelCase")]
    EncoderColour { colour: i32 },
    #[serde(rename_all = "camelCase")]
    InputColour { row: usize, colour: String },
}

/// One frame on the wire, for the protocol log. Mirrors what `probe`'s
/// `listen` and `pcapdec` print, so the app and the CLI tools agree.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrameDto {
    /// Milliseconds since the app started, so entries order sensibly.
    pub(crate) at: u128,
    /// "in" from the console, "out" to it.
    pub(crate) dir: &'static str,
    pub(crate) name: String,
    pub(crate) id: String,
    pub(crate) id_num: u32,
    pub(crate) values: String,
    pub(crate) hex: String,
    /// True when this frame changed the model, so noise is easy to filter.
    pub(crate) applied: bool,
}

/// A capture endpoint as Windows lists it. The analyser reads audio off one of
/// these rather than off the console's HID interface.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CaptureDeviceDto {
    pub(crate) name: String,
    /// The host's default input, marked in the list the way a chat client does.
    pub(crate) default: bool,
}

/// What the analyser opened, sent once when capture starts.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RtaInfoDto {
    pub(crate) device: String,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
    /// Band centre frequencies, so the UI can place its frequency axis without
    /// repeating the band plan.
    pub(crate) centres: Vec<f32>,
}

/// One analysis frame, about 23 a second.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RtaFrameDto {
    /// dBFS per band, in the order `RtaInfoDto::centres` gives.
    pub(crate) db: Vec<f32>,
    pub(crate) peak_db: f32,
    pub(crate) clipped: bool,
}
