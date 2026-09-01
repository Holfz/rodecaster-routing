//! Commands the console accepts, as bytes.
//!
//! Every frame here reproduces bytes captured from the RODECaster App, except
//! the two monitor ones, which are noted where they are defined. Building a
//! frame needs no device, so all of this is tested without hardware.

use crate::model;
use rcp_proto::{id_bytes, Frame, RID_COMMAND};

/// The three states a routing cell can hold, plus the two commands that reach
/// them. Mute and link are independent booleans on the device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Set {
    Mute,
    Unmute,
    Link,
    Unlink,
}

impl Set {
    /// Commands needed to move a cell to a target state.
    ///
    /// Muted to linked takes two: mute and link are separate booleans.
    pub fn steps_to(target: model::CellState, from: Option<model::CellState>) -> Vec<Set> {
        use model::CellState::*;
        match target {
            Muted => vec![Set::Mute],
            Linked if from == Some(Muted) => vec![Set::Unmute, Set::Link],
            Linked => vec![Set::Link],
            Unlinked if from == Some(Muted) => vec![Set::Unmute, Set::Unlink],
            Unlinked => vec![Set::Unlink],
        }
    }
}

/// Build the frame for one cell command.
pub fn cell_frame(id: u32, set: Set) -> Vec<u8> {
    const TRUE: &[u8] = &[0x01, 0x01, 0x02];
    const FALSE: &[u8] = &[0x01, 0x01, 0x03];

    // Momentary trigger: a group of two booleans, both asserted. The console
    // echoes (false, false) once it has acted.
    const TRIGGER: &[u8] = &[0x01, 0x07, 0x08, 0x01, 0x01, 0x02, 0x01, 0x01, 0x02];

    let (name, args): (&str, &[u8]) = match set {
        Set::Mute => ("mixDisabled", TRUE),
        Set::Unmute => ("mixDisabled", FALSE),
        Set::Link => ("mixLinkRequest", TRIGGER),
        Set::Unlink => ("mixUnlinkRequest", TRIGGER),
    };

    Frame::encode(RID_COMMAND, &id_bytes(id), name, args)
}

/// Build the frame that sets an output's Main Mix / Custom / Mix Minus mode.
pub fn output_mode_frame(id: u32, mode: i32) -> Vec<u8> {
    let mut args = vec![0x01, 0x05, 0x01];
    args.extend_from_slice(&mode.to_le_bytes());

    Frame::encode(RID_COMMAND, &id_bytes(id), "outputMixMinus", &args)
}

/// Build the frame that mutes or unmutes the studio monitor output.
///
/// The RODECaster App has no monitor control, so this one was worked out rather
/// than captured, then confirmed against the event the console pushes when the
/// monitor is muted on its touchscreen, which is byte-identical apart from
/// the report id.
pub fn monitor_mute_frame(id: u32, mute: bool) -> Vec<u8> {
    const TRUE: &[u8] = &[0x01, 0x01, 0x02];
    const FALSE: &[u8] = &[0x01, 0x01, 0x03];

    let args = if mute { TRUE } else { FALSE };

    Frame::encode(RID_COMMAND, &id_bytes(id), "outputMonMute", args)
}

/// Build the frame that sets the studio monitor volume, 0.0-1.0.
///
/// The console stores this as float32 and widens it on the wire, so narrowing
/// here sends what it will keep rather than a double it would round.
pub fn monitor_level_frame(id: u32, level: f64) -> Vec<u8> {
    let level = (level.clamp(0.0, 1.0) as f32) as f64;

    let mut args = vec![0x01, 0x09, 0x04];
    args.extend_from_slice(&level.to_le_bytes());

    Frame::encode(RID_COMMAND, &id_bytes(id), "outputMonLevel", &args)
}

/// Normalise a colour to the eight lowercase hex digits the console stores.
///
/// A six-digit colour gains the opaque `ff` alpha every dump value carries.
pub fn normalise_argb(input: &str) -> Result<String, String> {
    let hex = input.trim().trim_start_matches('#').to_ascii_lowercase();
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("{input:?} is not a hex colour"));
    }

    let hex = match hex.len() {
        6 => format!("ff{hex}"),
        8 => hex,
        n => return Err(format!("{input:?} has {n} hex digits, expected 6 or 8")),
    };

    // The console silently keeps the old colour for anything off its palette,
    // so refuse rather than send a write that looks accepted and does nothing.
    if !model::INPUT_PALETTE.contains(&hex.as_str()) {
        return Err(format!(
            "the console only accepts its own sixteen colours, and {hex} is not one of them"
        ));
    }

    Ok(hex)
}

/// Build the frame that sets one input source's colour.
///
/// Reproduces bytes captured from the RODECaster App recolouring a channel.
/// The colour goes as an ARGB string rather than a palette index, which is why
/// the wire accepts any value even though the console only keeps sixteen.
pub fn input_colour_frame(id: u32, colour: &str) -> Result<Vec<u8>, String> {
    let hex = normalise_argb(colour)?;

    // 01 <len> 05 <ascii>\0, where len counts the type byte and the terminator.
    let mut args = vec![0x01, hex.len() as u8 + 2, 0x05];
    args.extend_from_slice(hex.as_bytes());
    args.push(0);

    Ok(Frame::encode(RID_COMMAND, &id_bytes(id), "inputColour", &args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CellState;

    #[test]
    fn cell_frame_matches_the_captured_bytes() {
        // Headphones 1 <- Game, mute. Captured from the RØDECaster App.
        let expect: Vec<u8> = "03 14 00 00 00 01 01 01 01 e8 6d 69 78 44 69 73 61 62 6c 65 64 \
                               00 01 01 02"
            .split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();
        assert_eq!(cell_frame(232, Set::Mute), expect);
    }

    #[test]
    fn output_mode_frame_matches_the_captured_bytes() {
        // outputMixMinus was captured with id 0x32 (50) and length 0x1b (27).
        let f = output_mode_frame(50, 2);
        assert_eq!(f[0], RID_COMMAND);
        assert_eq!(f[1], 0x1b);
        assert_eq!(f[9], 0x32);
    }

    #[test]
    fn monitor_mute_frame_matches_the_pushed_event() {
        // Captured from the console's own touchscreen, muting the monitor:
        //   04 16 00 00 00 01 01 01 01 0d "outputMonMute\0" 01 01 02
        // The command is the same payload under the host->device report id.
        let event: Vec<u8> = "04 16 00 00 00 01 01 01 01 0d 6f 75 74 70 75 74 4d 6f 6e 4d 75 \
                               74 65 00 01 01 02"
            .split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();

        let mut expect = event.clone();
        expect[0] = RID_COMMAND;
        assert_eq!(monitor_mute_frame(model::OUTPUT_ID, true), expect);

        // ...and unmute differs only in the value byte.
        let mut unmute = expect.clone();
        *unmute.last_mut().unwrap() = 0x03;
        assert_eq!(monitor_mute_frame(model::OUTPUT_ID, false), unmute);
    }

    #[test]
    fn monitor_level_frame_matches_the_pushed_event() {
        // Captured from the console, monitor volume at 29%:
        //   04 1f 00 00 00 01 01 01 01 0d "outputMonLevel\0" 01 09 04 <f64>
        let event: Vec<u8> = "04 1f 00 00 00 01 01 01 01 0d 6f 75 74 70 75 74 4d 6f 6e 4c 65 76 \
                               65 6c 00 01 09 04 00 00 00 c0 5d 8f d2 3f"
            .split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();

        let mut expect = event.clone();
        expect[0] = RID_COMMAND;
        assert_eq!(monitor_level_frame(model::OUTPUT_ID, 0.29000037908554077), expect);

        // Whatever is asked for is narrowed to what the console can hold, so
        // the low 29 mantissa bits of the value we send are always clear.
        let f = monitor_level_frame(model::OUTPUT_ID, 0.123456789012345);
        let mantissa = u64::from_le_bytes(f[f.len() - 8..].try_into().unwrap());
        assert_eq!(mantissa & ((1 << 29) - 1), 0);
    }

    #[test]
    fn monitor_level_is_clamped_to_the_unit_range() {
        let id = model::OUTPUT_ID;
        assert_eq!(monitor_level_frame(id, 2.0), monitor_level_frame(id, 1.0));
        assert_eq!(monitor_level_frame(id, -1.0), monitor_level_frame(id, 0.0));
    }

    #[test]
    fn input_colour_frame_matches_the_captured_bytes() {
        // Captured from the RODECaster App recolouring input source 11:
        //   03 1e 00 00 00 01 01 01 02 79 02 "inputColour\0" 01 0a 05 "ff00b800\0"
        // Note the two-byte id: 0x0279 = 633 = INPUTSOURCE_BASE + 11.
        let expect: Vec<u8> = "03 1e 00 00 00 01 01 01 02 79 02 69 6e 70 75 74 43 6f 6c 6f 75 72 \
                               00 01 0a 05 66 66 30 30 62 38 30 30 00"
            .split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();
        let id = model::INPUTSOURCE_BASE + 11;
        assert_eq!(id, 633);
        assert_eq!(input_colour_frame(id, "ff00b800").unwrap(), expect);
        // The same colour written the two ways a picker would hand it over.
        assert_eq!(input_colour_frame(id, "#00b800").unwrap(), expect);
        assert_eq!(input_colour_frame(id, "00B800").unwrap(), expect);
    }

    #[test]
    fn input_colour_refuses_anything_that_is_not_a_colour() {
        let id = model::INPUTSOURCE_BASE;
        assert!(input_colour_frame(id, "").is_err());
        assert!(input_colour_frame(id, "nope").is_err());
        assert!(input_colour_frame(id, "ff00b8000").is_err());
    }

    /// The wire carries a hex string, but the console still only takes the
    /// sixteen colours its own app offers; an arbitrary one is dropped.
    #[test]
    fn input_colour_refuses_colours_outside_the_palette() {
        let id = model::INPUTSOURCE_BASE;
        for hex in model::INPUT_PALETTE {
            assert!(input_colour_frame(id, hex).is_ok(), "{hex} should be accepted");
        }
        for hex in ["ff123456", "#ffffff", "000000", "ff00b801"] {
            assert!(input_colour_frame(id, hex).is_err(), "{hex} should be refused");
        }
    }

    #[test]
    fn leaving_mute_takes_two_commands() {
        assert_eq!(
            Set::steps_to(CellState::Linked, Some(CellState::Muted)),
            vec![Set::Unmute, Set::Link]
        );
        assert_eq!(
            Set::steps_to(CellState::Linked, Some(CellState::Unlinked)),
            vec![Set::Link]
        );
        assert_eq!(Set::steps_to(CellState::Muted, Some(CellState::Linked)), vec![Set::Mute]);
    }
}
