//! Projecting the console's model into what the UI renders.

use rcp_model::labels;
use rcp_model::model::Model;

use crate::dto::{
    CellDto, ChannelDto, InfoDto, MatrixDto, OutputDto, RowDto, record_label, state_name,
    storage_label,
};
use crate::listener::Shared;

/// Counts come from the dump, never a constant - see `labels::Shape`.
pub(crate) fn shape_of(model: &Model) -> labels::Shape {
    labels::Shape {
        outputs: model.counts.mixminus,
        input_sources: model.counts.inputsource,
        channels: model.counts.channel,
    }
}

pub(crate) fn build(model: &Model, read_ms: u128) -> MatrixDto {
    let names = labels::Labels::for_shape(shape_of(model));

    let outputs = (0..model.outputs)
        .map(|col| {
            let mode = model.output_modes.get(col).copied().unwrap_or(-1);
            OutputDto {
                col,
                label: names.output(col),
                mode,
                mode_label: labels::OutputMode::from_wire(mode).label(),
                custom: labels::OutputMode::from_wire(mode).is_custom(),
            }
        })
        .collect();

    // A fader drives an input row; several faders could in principle point at
    // the same row, so map row -> fader rather than the other way round.
    let mut fader_of = std::collections::HashMap::new();
    for (fader, src) in model.active_faders() {
        fader_of.entry(src).or_insert(fader);
    }
    let muted_sources: std::collections::HashSet<i32> =
        model.strips().iter().filter(|c| c.mute).map(|c| c.source).collect();

    let row_count = model.cells.len() / model.stride();
    let rows = (0..row_count)
        .map(|row| {
            let master_mute = muted_sources.contains(&(row as i32));
            RowDto {
            row,
            label: names.input(row),
            // The console is the authority on colours and reports one per
            // source, so this fill is only reached before the first dump.
            colour: model
                .input_colours
                .get(row)
                .cloned()
                .flatten()
                .unwrap_or_else(|| labels::FALLBACK_COLOUR.to_string()),
            fader: fader_of.get(&row).copied(),
            master_mute,
            cells: (0..model.outputs)
                .map(|col| {
                    let c = model.cell(row, col);
                    CellDto {
                        col,
                        id: c.map(|c| c.id).unwrap_or(0),
                        state: c.map(|c| state_name(c.state())).unwrap_or("muted"),
                        level: c.and_then(|c| c.level),
                        level_steps: c.and_then(|c| c.level_steps()),
                    }
                })
                .collect(),
            }
        })
        .collect();

    let channels = model
        .strips()
        .into_iter()
        .map(|c| ChannelDto {
            index: c.index,
            label: names.input(c.source as usize),
            // Same rule as the matrix rows: the console owns the colour.
            colour: model
                .input_colours
                .get(c.source as usize)
                .cloned()
                .flatten()
                .unwrap_or_else(|| labels::FALLBACK_COLOUR.to_string()),
            source: c.source,
            mute: c.mute,
            cue: c.cue,
            talkback: c.talkback,
            bypass_processing: c.bypass_processing,
            pan: c.pan,
            fx_preset: c.fx_preset,
            level: model.strip_level(c.source),
            level_steps: model.strip_level_steps(c.source),
        })
        .collect();

    let i = &model.info;
    let info = InfoDto {
        firmware: i.firmware.clone(),
        serial: i.serial.clone(),
        mixer_build: i.mixer_build.clone(),
        sample_rate: i.sample_rate,
        buffer_size: i.buffer_size,
        record_label: record_label(i.record_state, i.record_ms),
        storage: storage_label(i.storage_inserted, i.storage_free, i.storage_capacity),
        // The console reports both a wired and a WiFi address; show whichever
        // is actually set.
        network: i
            .wifi_ip
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| i.ip.clone().filter(|s| !s.is_empty())),
        ssid: i.ssid.clone().filter(|s| !s.is_empty()),
        show: i.show.clone().filter(|s| !s.is_empty()),
        usb1_connected: i.usb1_connected,
        monitor_mute: i.monitor_mute,
        monitor_level: i.monitor_level,
        encoder_colour: i.encoder_colour,
    };

    MatrixDto {
        palette: rcp_model::model::INPUT_PALETTE.iter().map(|s| s.to_string()).collect(),
        outputs,
        rows,
        channels,
        info,
        warnings: model.warnings(),
        mix_base: model.mix_base,
        read_ms,
    }
}

/// Build from the listener's model. The console is not read here: the listener
/// keeps this current from pushed events, so the UI costs the device nothing.
pub(crate) fn from_cache(shared: &Shared) -> Result<MatrixDto, String> {
    let guard = shared.lock().map_err(|e| e.to_string())?;
    let model = guard
        .as_ref()
        .ok_or("still reading the console's state - one moment")?;
    Ok(build(model, 0))
}
