//! Read the console's state dump into a routing matrix.
//!
//! Ids come from `rcp_proto::dump`, which walks the tree and reads them off it
//! as positions. Values come from a byte scan of the same blob, which ignores
//! properties it does not know instead of failing the whole 136 KB message.
//! If the tree does not parse, the bases fall back to run-length arithmetic.

/// Base id of the flat run of 390 MIX objects.
pub const MIX_BASE: u32 = 76;
/// Base id of the 13 MIXMINUSES objects, one per output.
pub const MIXMINUS_BASE: u32 = 49;
/// Id of the single OUTPUT object, which carries the monitor and Bluetooth
/// output controls. Captured: the console pushes `outputMonMute` with id 0x0d.
pub const OUTPUT_ID: u32 = 13;
/// The sixteen colours the console will accept for a source.
///
/// Read out of `RODECaster App.exe` as packed little-endian `0xAARRGGBB`. The
/// console silently keeps the old colour for anything else, even though the
/// wire carries a hex string rather than a palette index.
pub const INPUT_PALETTE: [&str; 16] = [
    "ffd43580", "ff59dd20", "ff00c0e0", "fffdba0e", "ffdd1919", "ffff7a00", "ffffb800", "ffffe600",
    "ff7ecc00", "ff00b800", "ff00daa6", "ff00cbf8", "ff0047ff", "ff7000ff", "ffad00ff", "ffff00e5",
];

/// Base id of the INPUTSOURCE run. Pinned by the app recolouring source 11 at
/// id 633.
pub const INPUTSOURCE_BASE: u32 = 622;
/// Id of the single ENCODER object. Captured: `encoderColour` and
/// `encoderPressed` both arrive with id 0x06.
pub const ENCODER_ID: u32 = 6;
pub const OUTPUTS: usize = 13;

/// Physical channel strips on the console.
///
/// The model carries ten `CHANNEL` objects but the console has nine strips.
/// Index 9 also reports a source, so showing it produces a phantom duplicate.
pub const FADERS: usize = 9;

#[derive(Debug, Clone, PartialEq)]
pub enum CellState {
    /// Follows the main fader on this output.
    Linked,
    /// Present at a level independent of the main fader.
    Unlinked,
    /// Absent from this output.
    Muted,
}

/// Levels are sent as a fraction but quantised to this many steps, matching
/// `faderMax`: 0.653543 is exactly 83/127, 0.700787 exactly 89/127.
pub const LEVEL_STEPS: f64 = 127.0;

#[derive(Debug, Clone)]
pub struct Cell {
    pub id: u32,
    pub row: usize,
    pub col: usize,
    pub link: bool,
    pub mute: bool,
    /// This cell's level, 0.0-1.0. The slider on the app's routing page.
    pub level: Option<f64>,
    /// The second half of `mixLevelWithAnchor`. Equal to `level` in every
    /// sample so far, so what it anchors to is not yet established.
    pub anchor: Option<f64>,
}

impl Cell {
    /// Level on the console's own 0-127 scale.
    pub fn level_steps(&self) -> Option<i32> {
        self.level.map(|l| (l * LEVEL_STEPS).round() as i32)
    }
}

impl Cell {
    pub fn state(&self) -> CellState {
        // Mute wins: the two booleans are independent, and a muted cell is
        // silent whatever its link flag says.
        if self.mute {
            CellState::Muted
        } else if self.link {
            CellState::Linked
        } else {
            CellState::Unlinked
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub cells: Vec<Cell>,
    /// Per fader: which input row it drives, or -1 when empty.
    pub faders: Vec<i32>,
    /// Per output column: the `outputMixMinus` enum. 2 is Custom.
    pub output_modes: Vec<i32>,
    /// Base id of the MIX run, gathered from this dump rather than assumed.
    pub mix_base: u32,
    /// Base id of the MIXMINUSES run, one object per output.
    pub mixminus_base: u32,
    /// Base id of the CHANNEL run, likewise gathered. `channelOutputMute`
    /// arrives with `channel_base + strip index`, not the bare index.
    pub channel_base: u32,
    /// Base id of the INPUTSOURCE run, gathered the same way.
    pub inputsource_base: u32,
    /// Id of the single OUTPUT object, which carries the monitor controls.
    /// Not a constant: it sits right after the HEADPHONE run, and a console
    /// with two headphone outputs rather than four puts it at 11, not 13.
    pub output_id: u32,
    /// Outputs this console has, which is the row stride of `cells`.
    pub outputs: usize,
    /// Per input source: `inputColour`, the ARGB string the console holds.
    pub input_colours: Vec<Option<String>>,
    /// Object counts the base is gathered from, kept for diagnostics.
    pub counts: Counts,
    pub channels: Vec<Channel>,
    pub info: Info,
}

#[derive(Debug, Clone, Default)]
pub struct Counts {
    pub mixminus: usize,
    pub rcsyncminus: usize,
    pub mix: usize,
    pub channel: usize,
    pub inputsource: usize,
}

impl Model {
    /// Anything that would make the addressing wrong, stated plainly.
    ///
    /// Ids are positional, so adding or removing an object ahead of the MIX
    /// run shifts every cell id silently.
    pub fn warnings(&self) -> Vec<String> {
        let mut w = Vec::new();

        if self.counts.mixminus == 0 {
            w.push("no MIXMINUSES objects, so the console reported no outputs".into());
            return w;
        }

        let expected = self.counts.inputsource * self.counts.mixminus;
        if self.counts.mix != expected {
            w.push(format!(
                "{} MIX objects, but {} inputs x {} outputs is {expected}; the \
                 grid is not rectangular, so cell ids cannot be trusted",
                self.counts.mix, self.counts.inputsource, self.counts.mixminus
            ));
        }

        // MIXMINUSES then RCSYNCMIXMINUES then MIX, so the bases chain. If they
        // do not, something was miscounted and every cell id is off.
        let chained =
            self.mixminus_base + self.counts.mixminus as u32 + self.counts.rcsyncminus as u32;
        if self.mix_base != chained {
            w.push(format!(
                "MIX base is {} but the runs in front of it end at {chained}; \
                 cell ids may be off by {}",
                self.mix_base,
                self.mix_base as i64 - chained as i64
            ));
        }

        w
    }
}

impl Model {
    /// Row stride of `cells`. Never zero, so indexing cannot divide by it.
    pub fn stride(&self) -> usize {
        self.outputs.max(1)
    }

    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        self.cells.get(row * self.stride() + col)
    }
    /// Faders with a source assigned, as (fader index, input row).
    ///
    /// Physical strips only; the tenth `CHANNEL` would duplicate a row.
    pub fn active_faders(&self) -> Vec<(usize, usize)> {
        self.faders
            .iter()
            .take(FADERS)
            .enumerate()
            .filter_map(|(i, s)| (*s >= 0).then_some((i, *s as usize)))
            .collect()
    }

    /// Channel strips the console actually shows, in strip order.
    pub fn strips(&self) -> Vec<&Channel> {
        self.channels
            .iter()
            .filter(|c| c.index < FADERS && c.source >= 0)
            .collect()
    }

    /// A strip's fader level, 0.0-1.0.
    ///
    /// The console stores no such property; every linked cell in the row
    /// carries it. Worked out on demand so a fader move cannot leave it stale.
    pub fn strip_level(&self, source: i32) -> Option<f64> {
        let row = usize::try_from(source).ok()?;
        row_fader_level(&self.cells, row, self.stride())
    }

    pub fn strip_level_steps(&self, source: i32) -> Option<i32> {
        self.strip_level(source).map(|l| (l * LEVEL_STEPS).round() as i32)
    }

    /// Whether the strip driving this input row is master muted.
    pub fn row_master_muted(&self, row: usize) -> bool {
        self.strips().iter().any(|c| c.source == row as i32 && c.mute)
    }
}

/// Device facts worth showing, all from the same dump the matrix uses.
#[derive(Debug, Clone, Default)]
pub struct Info {
    pub firmware: Option<String>,
    pub serial: Option<String>,
    pub mixer_build: Option<String>,
    pub sample_rate: Option<f64>,
    pub buffer_size: Option<i32>,
    pub record_state: Option<i32>,
    pub record_ms: Option<i32>,
    pub storage_inserted: Option<bool>,
    pub storage_capacity: Option<i32>,
    pub storage_free: Option<i32>,
    pub ip: Option<String>,
    pub wifi_ip: Option<String>,
    pub ssid: Option<String>,
    pub show: Option<String>,
    pub usb1_connected: Option<bool>,
    /// `outputMonMute` on the single OUTPUT object: the studio monitor mute.
    pub monitor_mute: Option<bool>,
    /// `encoderColour` on the single ENCODER object.
    ///
    /// An index, not an ARGB string like `inputColour`, and nothing maps index
    /// to colour, so it is shown as a number.
    pub encoder_colour: Option<i32>,
    /// `outputMonLevel` on the same object: monitor volume, 0.0-1.0.
    ///
    /// Held as float32 and widened on the wire: the captured
    /// 0.29000037908554077 round-trips through f32 unchanged.
    ///
    /// UNVERIFIED: the taper is neither the faders' 127 steps (x127 = 36.83)
    /// nor whole percent (x100 is 13 f32 ULPs off), so it is shown as position.
    pub monitor_level: Option<f64>,
}

/// One channel strip. `source` is the input row it drives, or -1 when empty.
#[derive(Debug, Clone)]
pub struct Channel {
    pub index: usize,
    pub source: i32,
    /// `channelOutputMute`, the strip's master mute. Silences the channel
    /// without altering any cell, so the console still reports them linked.
    pub mute: bool,
    pub cue: bool,
    pub talkback: bool,
    pub bypass_processing: bool,
    pub pan: Option<f64>,
    pub fx_preset: i32,
}

/// The level shared by a row's linked cells, i.e. the channel's fader.
///
/// Muted and unlinked cells hold independent levels and are excluded. Takes
/// the most common value, not a unanimous one: a fader move arrives as one
/// `mixLevelWithAnchor` per cell, so mid-slide the row legitimately disagrees
/// and demanding agreement made the level blink out.
fn row_fader_level(cells: &[Cell], row: usize, outputs: usize) -> Option<f64> {
    // If a master mute was already on at connect, every cell in the row reads
    // muted, so fall back to linked-regardless: muting flags a cell without
    // changing its level. Individually muted cells have link = false and stay
    // out either way.
    let usable = |c: &&Cell| c.link && !c.mute;
    let any_usable = cells.iter().skip(row * outputs).take(outputs).any(|c| usable(&c));

    let mut tally: Vec<(i32, usize, f64)> = Vec::new();
    for cell in cells.iter().skip(row * outputs).take(outputs) {
        let qualifies = if any_usable { cell.link && !cell.mute } else { cell.link };
        if !qualifies {
            continue;
        }
        let Some(level) = cell.level else { continue };
        // Tally on the console's integer scale so float noise cannot split a
        // value into two near-identical buckets.
        let steps = (level * LEVEL_STEPS).round() as i32;
        match tally.iter_mut().find(|(s, _, _)| *s == steps) {
            Some((_, count, _)) => *count += 1,
            None => tally.push((steps, 1, level)),
        }
    }

    tally.iter().max_by_key(|(_, count, _)| *count).map(|(_, _, level)| *level)
}

fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return Vec::new();
    }
    (0..=hay.len() - needle.len())
        .filter(|i| &hay[*i..*i + needle.len()] == needle)
        .collect()
}

fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|i| &hay[*i..*i + needle.len()] == needle)
}

/// A property is `<name>\0` followed by an encoded value.
fn prop<'a>(rec: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let mut key = name.as_bytes().to_vec();
    key.push(0);
    let at = find_from(rec, &key, 0)?;
    rec.get(at + key.len()..)
}

fn prop_bool(rec: &[u8], name: &str) -> Option<bool> {
    let v = prop(rec, name)?;
    // 01 01 02 = true, 01 01 03 = false
    match (v.first(), v.get(1), v.get(2)) {
        (Some(1), Some(1), Some(2)) => Some(true),
        (Some(1), Some(1), Some(3)) => Some(false),
        _ => None,
    }
}

fn prop_i32(rec: &[u8], name: &str) -> Option<i32> {
    let v = prop(rec, name)?;
    if v.first() != Some(&1) || v.get(1) != Some(&5) || v.get(2) != Some(&1) {
        return None;
    }
    Some(i32::from_le_bytes([*v.get(3)?, *v.get(4)?, *v.get(5)?, *v.get(6)?]))
}

/// `01 09 04` then eight bytes: levels and pan are doubles.
fn prop_f64(rec: &[u8], name: &str) -> Option<f64> {
    let v = prop(rec, name)?;
    if v.first() != Some(&1) || v.get(2) != Some(&4) {
        return None;
    }
    let b: [u8; 8] = v.get(3..11)?.try_into().ok()?;
    Some(f64::from_le_bytes(b))
}

fn prop_str(rec: &[u8], name: &str) -> Option<String> {
    let v = prop(rec, name)?;
    if v.first() != Some(&1) || v.get(2) != Some(&5) {
        return None;
    }
    let len = *v.get(1)? as usize;
    let body = v.get(3..2 + len)?;
    let end = body.iter().position(|b| *b == 0).unwrap_or(body.len());
    Some(String::from_utf8_lossy(&body[..end]).into_owned())
}

/// Byte offsets of every record of `ty`, in document order.
fn offsets(blob: &[u8], ty: &str) -> Vec<usize> {
    let mut marker = vec![0u8];
    marker.extend_from_slice(ty.as_bytes());
    marker.push(0);
    find_all(blob, &marker)
}

/// Slice each record of `ty` up to the start of the next one.
fn records<'a>(blob: &'a [u8], ty: &str) -> Vec<&'a [u8]> {
    let mut marker = vec![0u8];
    marker.extend_from_slice(ty.as_bytes());
    marker.push(0);
    let starts = find_all(blob, &marker);
    starts
        .iter()
        .enumerate()
        .map(|(i, s)| {
            // Records are short; cap the tail one so a missing terminator
            // cannot drag in the rest of the dump.
            let end = starts.get(i + 1).copied().unwrap_or((s + 4096).min(blob.len()));
            &blob[*s..end]
        })
        .collect()
}

/// `mixLevelWithAnchor` is `"<level>|<anchor>"`, both fractions of full scale.
fn parse_level(s: Option<&str>) -> (Option<f64>, Option<f64>) {
    let Some(s) = s else { return (None, None) };
    let mut parts = s.split('|');
    (
        parts.next().and_then(|p| p.parse().ok()),
        parts.next().and_then(|p| p.parse().ok()),
    )
}

/// What a pushed event actually altered, so callers can update just that much.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Changed {
    /// Index into `cells`.
    Cell(usize),
    /// Output column.
    OutputMode(usize),
    /// Strip index.
    ChannelMute(usize),
    /// The studio monitor mute, which belongs to the device rather than a cell.
    MonitorMute,
    /// The studio monitor volume, likewise device-wide.
    MonitorLevel,
    /// The encoder ring's colour index, changed on the console itself.
    EncoderColour,
    /// One input source's colour. Carries the source index.
    InputColour(usize),
}

/// Apply one pushed event to the model, reporting what it changed.
///
/// The console pushes state changes whoever caused them, so following events
/// is cheaper than re-reading 136 KB and catches touchscreen changes too.
pub fn apply_event(model: &mut Model, frame: &rcp_proto::Frame) -> Option<Changed> {
    use rcp_proto::Value;

    let id = frame.id.iter().rev().fold(0u32, |a, b| (a << 8) | *b as u32);
    let first = frame.values.first();

    // Cells are addressed relative to the base gathered from this dump.
    let cell_index = id.checked_sub(model.mix_base).map(|i| i as usize);

    match (frame.name.as_str(), first) {
        ("mixLink", Some(Value::Bool(b))) => {
            if let Some(i) = cell_index {
                if let Some(c) = model.cells.get_mut(i) {
                    let changed = c.link != *b;
                    c.link = *b;
                    return changed.then_some(Changed::Cell(i));
                }
            }
        }

        ("mixMute", Some(Value::Bool(b))) => {
            // A strip's mute sets mixMute on every cell in the row, which would
            // overwrite the real per-cell states with a blanket mute. On unmute
            // the console re-sends mixMute only for cells it set, so keeping
            // the row untouched loses nothing.
            if let Some(row) = cell_index.map(|i| i / model.stride()) {
                if model.row_master_muted(row) {
                    return None;
                }
            }

            if let Some(i) = cell_index {
                if let Some(c) = model.cells.get_mut(i) {
                    let changed = c.mute != *b;
                    c.mute = *b;
                    return changed.then_some(Changed::Cell(i));
                }
            }
        }

        ("mixLevelWithAnchor", Some(Value::Str(s))) => {
            if let Some(i) = cell_index {
                if let Some(c) = model.cells.get_mut(i) {
                    let (level, anchor) = parse_level(Some(s));
                    let changed = c.level != level || c.anchor != anchor;
                    c.level = level;
                    c.anchor = anchor;
                    return changed.then_some(Changed::Cell(i));
                }
            }
        }

        ("outputMixMinus", Some(Value::Int(v))) => {
            if let Some(col) = id.checked_sub(model.mixminus_base).map(|i| i as usize) {
                if let Some(m) = model.output_modes.get_mut(col) {
                    let changed = *m != *v;
                    *m = *v;
                    return changed.then_some(Changed::OutputMode(col));
                }
            }
        }

        ("channelOutputMute", Some(Value::Bool(b))) => {
            if let Some(index) = id.checked_sub(model.channel_base).map(|i| i as usize) {
                if let Some(c) = model.channels.iter_mut().find(|c| c.index == index) {
                    let changed = c.mute != *b;
                    c.mute = *b;
                    return changed.then_some(Changed::ChannelMute(index));
                }
            }
        }

        ("inputColour", Some(Value::Str(hex))) => {
            if let Some(src) = id.checked_sub(model.inputsource_base).map(|i| i as usize) {
                if let Some(slot) = model.input_colours.get_mut(src) {
                    let next = Some(hex.clone());
                    let changed = *slot != next;
                    *slot = next;
                    return changed.then_some(Changed::InputColour(src));
                }
            }
        }

        ("encoderColour", Some(Value::Int(v))) => {
            if id == ENCODER_ID {
                let changed = model.info.encoder_colour != Some(*v);
                model.info.encoder_colour = Some(*v);
                return changed.then_some(Changed::EncoderColour);
            }
        }

        ("outputMonLevel", Some(Value::Float(v))) => {
            if id == model.output_id {
                let changed = model.info.monitor_level != Some(*v);
                model.info.monitor_level = Some(*v);
                return changed.then_some(Changed::MonitorLevel);
            }
        }

        ("outputMonMute", Some(Value::Bool(b))) => {
            // One object, one property, so the id is matched outright rather
            // than offset from a base.
            if id == model.output_id {
                let changed = model.info.monitor_mute != Some(*b);
                model.info.monitor_mute = Some(*b);
                return changed.then_some(Changed::MonitorMute);
            }
        }
        _ => {}
    }
    None
}

pub fn scan(blob: &[u8]) -> Model {
    // Ids are positions in the tree, so read them off it when it parses. The
    // byte scanner below works the same numbers out from run lengths, which is
    // correct on this console but assumes its object counts.
    //
    // The root name gates it: the synthetic fixtures in the tests are record
    // fragments rather than whole dumps, and must not be trusted as trees.
    let tree = rcp_proto::dump::parse(blob)
        .ok()
        .filter(|d| d.root.name == "Rodecaster" && !d.root.children.is_empty());

    let mixminus = records(blob, "MIXMINUSES");
    let rcsyncminus = records(blob, "RCSYNCMIXMINUES");

    // These two runs sit immediately before MIX, so the base follows from their
    // sizes rather than being asserted: 49 + 13 + 14 = 76, the captured anchor.
    let mix_base = tree
        .as_ref()
        .and_then(|d| d.first_id("MIX"))
        .unwrap_or(MIXMINUS_BASE + mixminus.len() as u32 + rcsyncminus.len() as u32);

    let mixminus_base = tree
        .as_ref()
        .and_then(|d| d.first_id("MIXMINUSES"))
        .unwrap_or(MIXMINUS_BASE);

    let outputs = tree
        .as_ref()
        .map(|d| d.count("MIXMINUSES"))
        .filter(|n| *n > 0)
        .unwrap_or(OUTPUTS);

    let output_id = tree
        .as_ref()
        .and_then(|d| d.first_id("OUTPUT"))
        .unwrap_or(OUTPUT_ID);

    // Counting back from the MIXMINUSES base gives the CHANNEL base:
    // 49 - 11 - 10 = 28, matching the captured `channelOutputMute id=28`. Only
    // the EFFECTS_PARAMETERS before MIXMINUSES count; another run sits later
    // under PADEFFECTS.
    let first_mixminus = offsets(blob, "MIXMINUSES").first().copied().unwrap_or(usize::MAX);
    let effects_before = offsets(blob, "EFFECTS_PARAMETERS")
        .iter()
        .filter(|o| **o < first_mixminus)
        .count() as u32;
    let channel_count = offsets(blob, "CHANNEL").len() as u32;
    let channel_base = tree
        .as_ref()
        .and_then(|d| d.first_id("CHANNEL"))
        .unwrap_or_else(|| MIXMINUS_BASE.saturating_sub(effects_before + channel_count));

    // `\0MIX\0` excludes RCSYNCMIX and STREAMERXSTREAMMIX, whose names end in
    // MIX but are preceded by a letter rather than a terminator.
    let cells = records(blob, "MIX")
        .iter()
        .enumerate()
        .map(|(i, rec)| Cell {
            id: mix_base + i as u32,
            row: i / outputs,
            col: i % outputs,
            link: prop_bool(rec, "mixLink").unwrap_or(false),
            mute: prop_bool(rec, "mixMute").unwrap_or(false),
            level: parse_level(prop_str(rec, "mixLevelWithAnchor").as_deref()).0,
            anchor: parse_level(prop_str(rec, "mixLevelWithAnchor").as_deref()).1,
        })
        .collect();

    // Same rule as the MIX base, from the runs in front of INPUTSOURCE:
    // 76 + 390 + 126 + 30 = 622.
    let inputsource_base = tree.as_ref().and_then(|d| d.first_id("INPUTSOURCE")).unwrap_or(
        mix_base
            + records(blob, "MIX").len() as u32
            + records(blob, "RCSYNCMIX").len() as u32
            + records(blob, "STREAMERXSTREAMMIX").len() as u32,
    );

    let input_colours =
        records(blob, "INPUTSOURCE").iter().map(|rec| prop_str(rec, "inputColour")).collect();

    let channel_recs = records(blob, "CHANNEL");
    let faders = channel_recs
        .iter()
        .map(|rec| prop_i32(rec, "channelInputSource").unwrap_or(-1))
        .collect();

    let channels = channel_recs
        .iter()
        .enumerate()
        .map(|(index, rec)| {
            let source = prop_i32(rec, "channelInputSource").unwrap_or(-1);
            Channel {
                index,
                source,
                mute: prop_bool(rec, "channelOutputMute").unwrap_or(false),
                cue: prop_bool(rec, "channelCueEnable").unwrap_or(false),
                talkback: prop_bool(rec, "channelTalkbackEnable").unwrap_or(false),
                bypass_processing: prop_bool(rec, "channelBypassProcessing").unwrap_or(false),
                pan: prop_f64(rec, "channelPan"),
                fx_preset: prop_i32(rec, "channelCurrentFxPreset").unwrap_or(-1),
            }
        })
        .collect();

    // Single-instance objects: property names are unique across the dump, so
    // reading them from the whole blob avoids guessing where a record ends.
    let info = Info {
        firmware: prop_str(blob, "systemFirmwareVersion"),
        serial: prop_str(blob, "systemSerialNumber"),
        mixer_build: prop_str(blob, "buildMixerVersion"),
        sample_rate: prop_f64(blob, "audioSampleRate"),
        buffer_size: prop_i32(blob, "audioBufferSize"),
        record_state: prop_i32(blob, "recordState"),
        record_ms: prop_i32(blob, "recordTimeMs"),
        storage_inserted: prop_bool(blob, "storageVolumeInserted"),
        storage_capacity: prop_i32(blob, "storageVolumeCapacity"),
        storage_free: prop_i32(blob, "storageVolumeFree"),
        ip: prop_str(blob, "ipAddress"),
        wifi_ip: prop_str(blob, "wifiIpAddress"),
        ssid: prop_str(blob, "wifiScanResultSSID"),
        show: prop_str(blob, "currentShowName"),
        usb1_connected: prop_bool(blob, "usb1Connected"),
        monitor_mute: prop_bool(blob, "outputMonMute"),
        monitor_level: prop_f64(blob, "outputMonLevel"),
        encoder_colour: prop_i32(blob, "encoderColour"),
    };

    let output_modes = mixminus
        .iter()
        .map(|rec| prop_i32(rec, "outputMixMinus").unwrap_or(-1))
        .collect();

    let counts = Counts {
        mixminus: mixminus.len(),
        rcsyncminus: rcsyncminus.len(),
        mix: records(blob, "MIX").len(),
        channel: records(blob, "CHANNEL").len(),
        inputsource: records(blob, "INPUTSOURCE").len(),
    };

    Model {
        cells,
        faders,
        output_modes,
        mix_base,
        mixminus_base,
        channel_base,
        inputsource_base,
        output_id,
        outputs,
        input_colours,
        counts,
        channels,
        info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A MIX record exactly as it appears in the dump.
    fn mix_record(link: u8, mute: u8) -> Vec<u8> {
        let mut v = b"\0MIX\0\x01\x06".to_vec();
        v.extend_from_slice(b"mixLevelWithAnchor\0\x01\x13\x050.653543|0.653543\0");
        v.extend_from_slice(b"mixLink\0\x01\x01");
        v.push(link);
        v.extend_from_slice(b"mixLinkRequest\0\x01\x07\x08\x01\x01\x03\x01\x01\x03");
        v.extend_from_slice(b"mixUnlinkRequest\0\x01\x07\x08\x01\x01\x03\x01\x01\x03");
        v.extend_from_slice(b"mixMute\0\x01\x01");
        v.push(mute);
        v.extend_from_slice(b"mixDisabled\0\x01\x01\x03\0");
        v
    }

    #[test]
    fn reads_cells_and_gathers_tri_state() {
        let mut blob = Vec::new();
        blob.extend(mix_record(2, 3)); // linked, not muted
        blob.extend(mix_record(3, 3)); // unlinked
        blob.extend(mix_record(2, 2)); // muted wins over linked
        let m = scan(&blob);

        assert_eq!(m.cells.len(), 3);
        // No MIXMINUSES in this fixture, so the base gathers as MIXMINUS_BASE.
        // Ids are relative to whatever the dump implies, never assumed.
        assert_eq!(m.cells[0].id, m.mix_base);
        assert_eq!(m.cells[0].state(), CellState::Linked);
        assert_eq!(m.cells[1].state(), CellState::Unlinked);
        assert_eq!(m.cells[2].state(), CellState::Muted);
        // 0.653543 is exactly 83/127 on the console's 0-127 scale.
        assert_eq!(m.cells[0].level_steps(), Some(83));
    }

    #[test]
    fn ignores_rcsyncmix_whose_name_also_ends_in_mix() {
        let mut blob = b"\0RCSYNCMIX\0\x01\x06mixLink\0\x01\x01\x02".to_vec();
        blob.extend(mix_record(2, 3));
        let m = scan(&blob);
        assert_eq!(m.cells.len(), 1, "only the real MIX record should count");
    }

    #[test]
    fn reads_fader_sources_and_output_modes() {
        let mut blob = b"\0CHANNEL\0\x01\x02channelInputSource\0\x01\x05\x01\x0c\x00\x00\x00".to_vec();
        blob.extend_from_slice(
            b"\0CHANNEL\0\x01\x02channelInputSource\0\x01\x05\x01\xff\xff\xff\xff",
        );
        blob.extend_from_slice(b"\0MIXMINUSES\0\x01\x01outputMixMinus\0\x01\x05\x01\x02\x00\x00\x00");
        let m = scan(&blob);
        assert_eq!(m.faders, vec![12, -1]);
        assert_eq!(m.output_modes, vec![2]);
        assert_eq!(m.active_faders(), vec![(0, 12)]);
    }

    #[test]
    fn cell_id_arithmetic_matches_the_captured_anchors() {
        // Headphones 1 <- Game was captured as id 232, Music as 245.
        assert_eq!(MIX_BASE + (12 * OUTPUTS as u32) + 0, 232);
        assert_eq!(MIX_BASE + (12 * OUTPUTS as u32) + 1, 233);
        assert_eq!(MIX_BASE + (13 * OUTPUTS as u32) + 0, 245);
    }
}

#[cfg(test)]
/// Events arriving on a console whose bases are not this one's.
///
/// Both handlers below keyed off the constants until they were changed to read
/// the gathered values, and every other test in this file still passed, because
/// they all use a Pro II shape where the two happen to agree.
#[cfg(test)]
mod other_hardware_tests {
    use super::*;
    use rcp_proto::Frame;

    fn event(id: u32, name: &str, args: &[u8]) -> Frame {
        let bytes = Frame::encode(rcp_proto::RID_EVENT, &rcp_proto::id_bytes(id), name, args);
        Frame::parse(&bytes).expect("should parse")
    }

    /// Two headphone outputs rather than four, so OUTPUT lands at 11 and every
    /// base after it shifts down.
    fn other_console() -> Model {
        Model {
            mixminus_base: 47,
            mix_base: 47 + 11 + 12,
            output_id: 11,
            outputs: 11,
            output_modes: vec![-1; 11],
            counts: Counts {
                mixminus: 11,
                rcsyncminus: 12,
                mix: 330,
                inputsource: 30,
                channel: 8,
            },
            ..Default::default()
        }
    }

    #[test]
    fn an_output_mode_event_is_placed_by_the_gathered_base() {
        let mut m = other_console();
        let f = event(m.mixminus_base + 3, "outputMixMinus", &[0x01, 0x05, 0x01, 2, 0, 0, 0]);

        assert_eq!(apply_event(&mut m, &f), Some(Changed::OutputMode(3)));
        assert_eq!(m.output_modes[3], 2);
    }

    #[test]
    fn a_monitor_mute_event_is_matched_by_the_gathered_id() {
        let mut m = other_console();
        let f = event(m.output_id, "outputMonMute", &[0x01, 0x01, 0x02]);

        assert_eq!(apply_event(&mut m, &f), Some(Changed::MonitorMute));
        assert_eq!(m.info.monitor_mute, Some(true));
    }

    /// 13 is this console's OUTPUT id, not that one's. Applying it there would
    /// mute a monitor the event never mentioned.
    #[test]
    fn an_event_at_this_consoles_output_id_is_ignored_on_another() {
        let mut m = other_console();
        let f = event(OUTPUT_ID, "outputMonMute", &[0x01, 0x01, 0x02]);

        assert_eq!(apply_event(&mut m, &f), None);
        assert_eq!(m.info.monitor_mute, None);
    }

    #[test]
    fn a_monitor_frame_is_addressed_to_the_gathered_id() {
        let m = other_console();
        let f = crate::command::monitor_mute_frame(m.output_id, true);

        // Byte 9 is the id, right after `01 01 01 <len>`.
        assert_eq!(f[9], 11);
        assert_ne!(f[9], OUTPUT_ID as u8);
    }
}

#[cfg(test)]
mod base_tests {
    use super::*;

    #[test]
    fn mix_base_is_gathered_from_the_preceding_runs() {
        // 13 MIXMINUSES then 14 RCSYNCMIXMINUES, as the console sends them.
        let mut blob = Vec::new();
        for _ in 0..13 {
            blob.extend_from_slice(b"\0MIXMINUSES\0\x01\x01outputMixMinus\0\x01\x05\x01\x02\0\0\0");
        }
        for _ in 0..14 {
            blob.extend_from_slice(b"\0RCSYNCMIXMINUES\0\x01\x01outputMixMinus\0\x01\x05\x01\0\0\0\0");
        }
        blob.extend_from_slice(b"\0MIX\0\x01\x06mixLink\0\x01\x01\x02mixMute\0\x01\x01\x03");

        let m = scan(&blob);
        assert_eq!(m.mix_base, 76, "49 + 13 + 14");
        assert_eq!(m.cells[0].id, 232 - 12 * 13, "first cell is Headphones 1 of input row 0");
        assert_eq!(m.output_modes.len(), 13);
    }

    /// End to end on a real dump: the tree path engages and the addressing it
    /// reads off matches every id captured from the console.
    ///
    /// Ignored by default, like its counterpart in `rcp_proto::dump`, because
    /// the blob is not published and a test that passes on a missing fixture
    /// reports coverage it does not have.
    ///
    /// Run it with `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs dev/docs/route-04.pcap.blob-5038ms.bin, which is not published"]
    fn the_captured_dump_gathers_the_confirmed_addressing() {
        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../dev/docs/route-04.pcap.blob-5038ms.bin");
        let blob = std::fs::read(path).expect("dev/ present but the captured dump is missing");

        let m = scan(&blob);

        assert_eq!(m.output_id, OUTPUT_ID);
        assert_eq!(m.mixminus_base, MIXMINUS_BASE);
        assert_eq!(m.mix_base, MIX_BASE);
        assert_eq!(m.inputsource_base, INPUTSOURCE_BASE);
        assert_eq!(m.outputs, OUTPUTS);
        assert!(m.warnings().is_empty(), "got {:?}", m.warnings());
    }

    #[test]
    fn a_grid_that_is_not_rectangular_is_reported() {
        // 12 outputs but one MIX object: the run cannot be inputs x outputs,
        // so the cells are not a grid and their ids mean nothing.
        let mut blob = Vec::new();
        for _ in 0..12 {
            blob.extend_from_slice(b"\0MIXMINUSES\0\x01\x01outputMixMinus\0\x01\x05\x01\x02\0\0\0");
        }
        blob.extend_from_slice(b"\0MIX\0\x01\x06mixLink\0\x01\x01\x02mixMute\0\x01\x01\x03");

        let w = scan(&blob).warnings();
        assert!(w.iter().any(|s| s.contains("not rectangular")), "got {w:?}");
    }

    /// The check that matters once ids come off the tree: if the tree's base
    /// disagrees with the runs in front of it, one of them miscounted and every
    /// cell id is off. A console with different counts is not itself an error.
    #[test]
    fn a_base_that_does_not_follow_the_runs_is_reported() {
        let m = Model {
            mixminus_base: 49,
            mix_base: 80,
            counts: Counts {
                mixminus: 13,
                rcsyncminus: 14,
                mix: 390,
                inputsource: 30,
                channel: 10,
            },
            ..Default::default()
        };

        let w = m.warnings();
        assert!(w.iter().any(|s| s.contains("MIX base")), "got {w:?}");
        assert!(w.iter().any(|s| s.contains("off by 4")), "got {w:?}");
    }

    #[test]
    fn a_console_with_different_counts_is_not_an_error() {
        // A Duo shape: fewer outputs, and OUTPUT sitting at 11 rather than 13.
        let m = Model {
            mixminus_base: 47,
            mix_base: 47 + 11 + 12,
            output_id: 11,
            outputs: 11,
            counts: Counts {
                mixminus: 11,
                rcsyncminus: 12,
                mix: 330,
                inputsource: 30,
                channel: 8,
            },
            ..Default::default()
        };

        assert!(m.warnings().is_empty(), "got {:?}", m.warnings());
    }
}

#[cfg(test)]
mod strip_tests {
    use super::*;

    fn channel(source: i32) -> Vec<u8> {
        let mut v = b"\0CHANNEL\0\x01\x02channelInputSource\0\x01\x05\x01".to_vec();
        v.extend_from_slice(&source.to_le_bytes());
        v.extend_from_slice(b"channelOutputMute\0\x01\x01\x03");
        v
    }

    #[test]
    fn the_tenth_channel_is_not_a_strip() {
        // Real layout from the console: nine strips, then an extra CHANNEL
        // that also reports source 9 - the same source strip 4 already uses.
        let sources = [0, 8, 12, 9, 13, 7, -1, -1, -1, 9];
        let mut blob = Vec::new();
        for s in sources {
            blob.extend(channel(s));
        }
        let m = scan(&blob);

        assert_eq!(m.channels.len(), 10, "all channels are still parsed");
        assert_eq!(m.strips().len(), 6, "only assigned physical strips are shown");
        assert!(
            m.strips().iter().all(|c| c.index < FADERS),
            "the tenth channel must not appear as a strip"
        );
        // Source 9 must appear once, not twice.
        assert_eq!(m.strips().iter().filter(|c| c.source == 9).count(), 1);
        assert_eq!(m.active_faders(), vec![(0, 0), (1, 8), (2, 12), (3, 9), (4, 13), (5, 7)]);
    }
}

#[cfg(test)]
mod event_tests {
    use super::*;
    use rcp_proto::Frame;

    fn model_with_one_row() -> Model {
        let mut blob = Vec::new();
        for _ in 0..OUTPUTS {
            blob.extend_from_slice(b"\0MIXMINUSES\0\x01\x01outputMixMinus\0\x01\x05\x01\x02\0\0\0");
        }
        for _ in 0..14 {
            blob.extend_from_slice(b"\0RCSYNCMIXMINUES\0\x01\x01outputMixMinus\0\x01\x05\x01\0\0\0\0");
        }
        for _ in 0..OUTPUTS {
            blob.extend_from_slice(
                b"\0MIX\0\x01\x06mixLevelWithAnchor\0\x01\x13\x050.653543|0.653543\0\
                  mixLink\0\x01\x01\x02mixMute\0\x01\x01\x03",
            );
        }
        scan(&blob)
    }

    /// Frames as the console actually sends them.
    fn event(id: u8, name: &str, args: &[u8]) -> Frame {
        let bytes = Frame::encode(rcp_proto::RID_EVENT, &[id], name, args);
        Frame::parse(&bytes).expect("should parse")
    }

    #[test]
    fn a_pushed_mute_updates_the_right_cell() {
        let mut m = model_with_one_row();
        assert_eq!(m.cells[0].state(), CellState::Linked);

        // Headphones 1 of row 0 is the base id itself.
        let f = event(m.mix_base as u8, "mixMute", &[0x01, 0x01, 0x02]);
        assert_eq!(apply_event(&mut m, &f), Some(Changed::Cell(0)), "should report which cell");
        assert_eq!(m.cells[0].state(), CellState::Muted);
        assert_eq!(m.cells[1].state(), CellState::Linked, "neighbours untouched");

        // Re-applying the same value is not a change.
        assert_eq!(apply_event(&mut m, &f), None, "re-applying the same value is not a change");
    }

    #[test]
    fn a_pushed_level_reparses_both_halves() {
        let mut m = model_with_one_row();
        let mut args = vec![0x01, 0x13, 0x05];
        args.extend_from_slice(b"0.700787|0.354331\0");
        let f = event(m.mix_base as u8 + 2, "mixLevelWithAnchor", &args);

        assert!(apply_event(&mut m, &f).is_some());
        assert_eq!(m.cells[2].level_steps(), Some(89), "0.700787 is 89/127");
        assert_eq!(m.cells[2].anchor.map(|a| (a * LEVEL_STEPS).round() as i32), Some(45));
    }

    #[test]
    fn an_event_for_an_unknown_id_is_ignored() {
        let mut m = model_with_one_row();
        let f = event(250, "mixMute", &[0x01, 0x01, 0x02]);
        assert_eq!(apply_event(&mut m, &f), None, "out of range must not panic or alter state");
    }
}

#[cfg(test)]
mod level_tests {
    use super::*;

    /// A row of 13 cells: `linked` says which are linked, `levels` their values.
    fn row(linked: [bool; OUTPUTS], levels: [i32; OUTPUTS]) -> Vec<u8> {
        let mut blob = Vec::new();
        for col in 0..OUTPUTS {
            let lvl = levels[col] as f64 / LEVEL_STEPS;
            let s = format!("{lvl:.6}|{lvl:.6}");
            blob.extend_from_slice(b"\0MIX\0\x01\x06mixLevelWithAnchor\0\x01");
            blob.push((s.len() + 2) as u8);
            blob.push(0x05);
            blob.extend_from_slice(s.as_bytes());
            blob.push(0);
            blob.extend_from_slice(b"mixLink\0\x01\x01");
            blob.push(if linked[col] { 2 } else { 3 });
            blob.extend_from_slice(b"mixMute\0\x01\x01");
            blob.push(if linked[col] { 3 } else { 2 });
        }
        blob
    }

    #[test]
    fn the_fader_level_is_the_level_the_linked_cells_agree_on() {
        // Real shape from the console: Game is 89 everywhere it is linked, and
        // the two muted outputs hold their own independent values.
        let mut linked = [true; OUTPUTS];
        linked[4] = false;
        linked[8] = false;
        let mut levels = [89; OUTPUTS];
        levels[4] = 0;
        levels[8] = 62;

        let m = scan(&row(linked, levels));
        assert_eq!(m.strip_level_steps(0), Some(89), "muted cells must not skew it");
    }

    #[test]
    fn a_partly_updated_row_reports_a_level_not_a_blank() {
        // Mid-slide: the console has sent 8 of 13 cells at the new value.
        // Demanding unanimity here made the reading blink out.
        let mut levels = [40; OUTPUTS];
        for l in levels.iter_mut().take(8) {
            *l = 89;
        }
        let m = scan(&row([true; OUTPUTS], levels));
        assert_eq!(m.strip_level_steps(0), Some(89), "the majority wins");
    }

    #[test]
    fn one_stray_cell_does_not_move_the_level() {
        let mut levels = [89; OUTPUTS];
        levels[3] = 40;
        let m = scan(&row([true; OUTPUTS], levels));
        assert_eq!(m.strip_level_steps(0), Some(89));
    }

    #[test]
    fn a_row_with_nothing_linked_yields_no_level() {
        let m = scan(&row([false; OUTPUTS], [42; OUTPUTS]));
        assert_eq!(m.strip_level(0), None);
    }
}

#[cfg(test)]
mod channel_base_tests {
    use super::*;

    #[test]
    fn channel_base_counts_back_from_the_mixminus_run() {
        // Document order here: CHANNEL(10), EFFECTS_PARAMETERS(11),
        // MIXMINUSES(13). The later PADEFFECTS run must not be counted.
        let mut blob = Vec::new();
        for _ in 0..10 {
            blob.extend_from_slice(b"\0CHANNEL\0\x01\x01channelInputSource\0\x01\x05\x01\0\0\0\0");
        }
        for _ in 0..11 {
            blob.extend_from_slice(b"\0EFFECTS_PARAMETERS\0\x01\x01effectsIdx\0\x01\x05\x01\0\0\0\0");
        }
        for _ in 0..13 {
            blob.extend_from_slice(b"\0MIXMINUSES\0\x01\x01outputMixMinus\0\x01\x05\x01\x02\0\0\0");
        }
        for _ in 0..14 {
            blob.extend_from_slice(b"\0EFFECTS_PARAMETERS\0\x01\x01effectsIdx\0\x01\x05\x01\0\0\0\0");
        }

        // 49 - 11 - 10 = 28, matching the captured `channelOutputMute id=28`.
        assert_eq!(scan(&blob).channel_base, 28);
    }

    #[test]
    fn a_pushed_channel_mute_lands_on_the_right_strip() {
        let mut blob = Vec::new();
        for i in 0..10u8 {
            blob.extend_from_slice(b"\0CHANNEL\0\x01\x02channelInputSource\0\x01\x05\x01");
            blob.extend_from_slice(&(i as i32).to_le_bytes());
            blob.extend_from_slice(b"channelOutputMute\0\x01\x01\x03");
        }
        for _ in 0..11 {
            blob.extend_from_slice(b"\0EFFECTS_PARAMETERS\0\x01\x01effectsIdx\0\x01\x05\x01\0\0\0\0");
        }
        for _ in 0..13 {
            blob.extend_from_slice(b"\0MIXMINUSES\0\x01\x01outputMixMinus\0\x01\x05\x01\x02\0\0\0");
        }
        let mut m = scan(&blob);
        assert_eq!(m.channel_base, 28);

        // id 30 is channel_base + 2, i.e. the third strip.
        let bytes = rcp_proto::Frame::encode(rcp_proto::RID_EVENT, &[30], "channelOutputMute", &[1, 1, 2]);
        let f = rcp_proto::Frame::parse(&bytes).unwrap();
        assert!(apply_event(&mut m, &f).is_some());
        assert!(m.channels[2].mute, "strip 2 should be muted");
        assert!(!m.channels[0].mute, "and no other strip touched");
    }
}

#[cfg(test)]
mod master_mute_tests {
    use super::*;

    /// Six strips as the console reports them, plus a row of cells per source.
    fn model_for(source: i32, cell_mutes: [bool; OUTPUTS], links: [bool; OUTPUTS]) -> Model {
        let mut blob = Vec::new();
        blob.extend_from_slice(b"\0CHANNEL\0\x01\x02channelInputSource\0\x01\x05\x01");
        blob.extend_from_slice(&source.to_le_bytes());
        blob.extend_from_slice(b"channelOutputMute\0\x01\x01\x03");
        for _ in 0..11 {
            blob.extend_from_slice(b"\0EFFECTS_PARAMETERS\0\x01\x01effectsIdx\0\x01\x05\x01\0\0\0\0");
        }
        for _ in 0..13 {
            blob.extend_from_slice(b"\0MIXMINUSES\0\x01\x01outputMixMinus\0\x01\x05\x01\x02\0\0\0");
        }
        for _ in 0..14 {
            blob.extend_from_slice(b"\0RCSYNCMIXMINUES\0\x01\x01outputMixMinus\0\x01\x05\x01\0\0\0\0");
        }
        for col in 0..OUTPUTS {
            blob.extend_from_slice(b"\0MIX\0\x01\x06mixLevelWithAnchor\0\x01\x13\x050.700787|0.700787\0mixLink\0\x01\x01");
            blob.push(if links[col] { 2 } else { 3 });
            blob.extend_from_slice(b"mixMute\0\x01\x01");
            blob.push(if cell_mutes[col] { 2 } else { 3 });
        }
        scan(&blob)
    }

    fn event(id: u32, name: &str, on: bool) -> rcp_proto::Frame {
        let args = [0x01, 0x01, if on { 2 } else { 3 }];
        let id_bytes = rcp_proto::id_bytes(id);
        let bytes = rcp_proto::Frame::encode(rcp_proto::RID_EVENT, &id_bytes, name, &args);
        rcp_proto::Frame::parse(&bytes).unwrap()
    }

    #[test]
    fn a_master_mute_leaves_the_routing_row_untouched() {
        // Cell 3 is individually muted and unlinked; the rest are linked.
        let mut mutes = [false; OUTPUTS];
        let mut links = [true; OUTPUTS];
        mutes[3] = true;
        links[3] = false;
        let mut m = model_for(0, mutes, links);
        let base = m.mix_base;

        // The console sends channelOutputMute first, then mixMute for the row.
        let ch_base = m.channel_base;
        assert!(apply_event(&mut m, &event(ch_base, "channelOutputMute", true)).is_some());
        for col in 0..OUTPUTS as u32 {
            apply_event(&mut m, &event(base + col, "mixMute", true));
        }

        assert_eq!(m.cell(0, 0).unwrap().state(), CellState::Linked, "still linked");
        assert_eq!(m.cell(0, 3).unwrap().state(), CellState::Muted, "still muted");
        assert!(m.row_master_muted(0));
    }

    #[test]
    fn the_fader_level_survives_a_master_mute() {
        // Every cell muted, as a dump taken while already master muted looks.
        let m = model_for(0, [true; OUTPUTS], [true; OUTPUTS]);
        assert_eq!(
            m.strip_level_steps(0),
            Some(89),
            "muting does not move the fader, so the level must not blank out"
        );
    }
}
