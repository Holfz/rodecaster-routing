//! Human labels for cells the console addresses only by number.
//!
//! The console stores no input or output names, so they have to come from here.
//! A wrong label sends someone's audio somewhere they did not intend, so a
//! source with no supportable label keeps `None` and renders as "source N".
//!
//! Keyed by index rather than console position: a fader's `channelInputSource`
//! decides which row it drives and the operator can reassign it.

/// The per-output mode carried by `outputMixMinus`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputMode {
    MainMix,
    MixMinus,
    Custom,
    Unknown(i32),
}

impl OutputMode {
    /// All three read off the console while each held a different value:
    /// Headphones 1 was 2/Custom, Headphones 2 was 0/Main Mix, Bluetooth
    /// was 1/Mix Minus.
    pub fn from_wire(v: i32) -> Self {
        match v {
            0 => Self::MainMix,
            1 => Self::MixMinus,
            2 => Self::Custom,
            other => Self::Unknown(other),
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::MainMix => "Main Mix".into(),
            Self::MixMinus => "Mix Minus".into(),
            Self::Custom => "Custom".into(),
            Self::Unknown(v) => format!("mode {v}"),
        }
    }

    /// Main Mix and Mix Minus carry every channel whatever the cell says, so
    /// only a Custom output honours its per-cell routing. An unseen value is
    /// not Custom, or the matrix would invite writes with no audible effect.
    pub fn is_custom(self) -> bool {
        matches!(self, Self::Custom)
    }
}

pub struct OutputLabel {
    pub col: usize,
    pub label: &'static str,
}

pub struct SourceLabel {
    pub src: usize,
    pub label: Option<&'static str>,
}

/// The object counts gathered from a dump, used to pick a profile.
///
/// The Pro II and the Duo run a byte-identical `rc_audio_mixer`, so counts are
/// not fixed per model; the tree adapts to the board it finds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shape {
    pub outputs: usize,
    pub input_sources: usize,
    pub channels: usize,
}

pub struct Profile {
    #[allow(dead_code)]
    pub name: &'static str,
    pub shape: Shape,
    pub outputs: &'static [OutputLabel],
    pub sources: &'static [SourceLabel],
}

/// 0 and 1 are pinned by captured cell ids 232 and 233. The rest follow the
/// Output grid order on the console; 11 and 12 are not visible in the
/// RODECaster App and rest on that ordering alone.
static PRO_II_OUTPUTS: &[OutputLabel] = &[
    OutputLabel { col: 0,  label: "Headphones 1" },
    OutputLabel { col: 1,  label: "Headphones 2" },
    OutputLabel { col: 2,  label: "Headphones 3" },
    OutputLabel { col: 3,  label: "Headphones 4" },
    OutputLabel { col: 4,  label: "Monitor" },
    OutputLabel { col: 5,  label: "Recording" },
    OutputLabel { col: 6,  label: "Bluetooth" },
    OutputLabel { col: 7,  label: "USB 1 Main" },
    OutputLabel { col: 8,  label: "USB 1 Comms" },
    OutputLabel { col: 9,  label: "USB 2 Main" },
    OutputLabel { col: 10, label: "Call Me 1" },
    OutputLabel { col: 11, label: "Call Me 2" },
    OutputLabel { col: 12, label: "Call Me 3" },
];

/// Candidate names come from string literals in `RODECaster App.exe`, but MSVC
/// pools literals in arbitrary order, so the binary gives the set and never the
/// index order. Each index below is pinned by device evidence or left `None`.
static PRO_II_SOURCES: &[SourceLabel] = &[
    SourceLabel { src: 0,  label: Some("Combo 1") },
    // 1-5 follow 0 by palette position, not by device evidence.
    SourceLabel { src: 1,  label: Some("Combo 2") },
    SourceLabel { src: 2,  label: Some("Combo 3") },
    SourceLabel { src: 3,  label: Some("Combo 4") },
    SourceLabel { src: 4,  label: Some("Wireless 1") },
    SourceLabel { src: 5,  label: Some("Wireless 2") },
    // A guess: the only physical-group label left over.
    SourceLabel { src: 6,  label: Some("Bluetooth") },
    // 7-9 pinned by inputColour matching the app's strip underline.
    SourceLabel { src: 7,  label: Some("USB 1 Main") },
    SourceLabel { src: 8,  label: Some("USB 1 Comms") },
    SourceLabel { src: 9,  label: Some("USB 2 Main") },
    // Carries a palette colour, so it is populated, but nothing names it.
    SourceLabel { src: 10, label: None },
    SourceLabel { src: 11, label: Some("Smart Pads") },
    // 12 and 13 pinned by cell ids 232 and 245 landing on these rows.
    SourceLabel { src: 12, label: Some("RC Game") },
    SourceLabel { src: 13, label: Some("RC Music") },
    // RODE documents four virtual devices - Game, Music, A, B - and 12-15 are
    // adjacent. Only carry audio while USB 1 is in Expanded mode.
    SourceLabel { src: 14, label: Some("RC Virtual A") },
    SourceLabel { src: 15, label: Some("RC Virtual B") },
    // 16-18 pinned by inputSipCallSlot 0/1/2.
    SourceLabel { src: 16, label: Some("Call Me 1") },
    SourceLabel { src: 17, label: Some("Call Me 2") },
    SourceLabel { src: 18, label: Some("Call Me 3") },
    // A guess: inputType 3, pad-like.
    SourceLabel { src: 19, label: Some("SMART Pads") },
    SourceLabel { src: 20, label: None },
    // 21-29 drive no fader and carry ff000000, so they fall through.
];

static PRO_II: Profile = Profile {
    name: "RODECaster Pro II",
    shape: Shape { outputs: 13, input_sources: 30, channels: 10 },
    outputs: PRO_II_OUTPUTS,
    sources: PRO_II_SOURCES,
};

/// No Duo entry: its shape is predictable from the spec sheet but its labels
/// are not, and matching on a predicted shape would dress a Duo in Pro II
/// names. It falls through to "source N" and still routes correctly.
static PROFILES: &[&Profile] = &[&PRO_II];

pub fn profile_for(shape: Shape) -> Option<&'static Profile> {
    PROFILES.iter().copied().find(|p| p.shape == shape)
}

pub struct Labels {
    profile: Option<&'static Profile>,
}

impl Labels {
    /// An unrecognised console gets no labels rather than another one's.
    pub fn for_shape(shape: Shape) -> Self {
        Self { profile: profile_for(shape) }
    }

    #[allow(dead_code)]
    pub fn profile_name(&self) -> Option<&'static str> {
        self.profile.map(|p| p.name)
    }

    pub fn output(&self, col: usize) -> String {
        self.profile
            .and_then(|p| p.outputs.iter().find(|o| o.col == col))
            .map(|o| o.label.to_string())
            .unwrap_or_else(|| format!("Output {col}"))
    }

    pub fn input(&self, src: usize) -> String {
        self.profile
            .and_then(|p| p.sources.iter().find(|s| s.src == src))
            .and_then(|s| s.label)
            .map(str::to_string)
            .unwrap_or_else(|| format!("source {src}"))
    }
}

/// Only reached before the first dump, since the console reports `inputColour`
/// per source. Deliberately not per-source: baking one unit's show colours in
/// would dress another console's rows in colours it never reported.
pub const FALLBACK_COLOUR: &str = "ff888888";

#[cfg(test)]
mod tests {
    use super::*;

    const PRO_II_SHAPE: Shape = Shape { outputs: 13, input_sources: 30, channels: 10 };

    #[test]
    fn the_confirmed_anchors_are_carried() {
        let l = Labels::for_shape(PRO_II_SHAPE);
        assert_eq!(l.profile_name(), Some("RODECaster Pro II"));
        assert_eq!(l.output(0), "Headphones 1");
        assert_eq!(l.input(12), "RC Game");
        assert_eq!(l.input(13), "RC Music");
        assert_eq!(l.input(11), "Smart Pads");
    }

    #[test]
    fn unidentified_rows_are_not_given_invented_names() {
        let l = Labels::for_shape(PRO_II_SHAPE);
        assert_eq!(l.input(10), "source 10");
        assert_eq!(l.input(20), "source 20");
    }

    #[test]
    fn unused_rows_fall_through() {
        assert_eq!(Labels::for_shape(PRO_II_SHAPE).input(25), "source 25");
    }

    #[test]
    fn an_unrecognised_shape_yields_no_labels() {
        let duo_ish = Shape { outputs: 11, input_sources: 30, channels: 8 };
        let l = Labels::for_shape(duo_ish);
        assert_eq!(l.profile_name(), None);
        assert_eq!(l.output(0), "Output 0");
        assert_eq!(l.input(12), "source 12");
    }

    #[test]
    fn modes_decode_and_only_custom_is_custom() {
        assert_eq!(OutputMode::from_wire(2), OutputMode::Custom);
        assert!(OutputMode::from_wire(2).is_custom());
        assert!(!OutputMode::from_wire(0).is_custom());
        assert!(!OutputMode::from_wire(1).is_custom());
        assert!(!OutputMode::from_wire(7).is_custom());
        assert_eq!(OutputMode::from_wire(7).label(), "mode 7");
    }

    /// An entry outside its profile's shape would be unreachable dead data.
    #[test]
    fn every_entry_is_within_the_declared_shape() {
        for p in PROFILES {
            assert_eq!(p.outputs.len(), p.shape.outputs, "{}", p.name);
            for o in p.outputs {
                assert!(o.col < p.shape.outputs, "{} output {}", p.name, o.col);
            }
            for s in p.sources {
                assert!(s.src < p.shape.input_sources, "{} source {}", p.name, s.src);
            }
        }
    }
}
