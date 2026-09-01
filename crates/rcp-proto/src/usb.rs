//! Which console is on the other end of the cable.
//!
//! The device writes `idProduct` at boot from its USB mode, so a model reports
//! six different product ids depending on how multitrack is configured. The
//! values below are read out of the gadget script in the 1.7.3 rootfs.
//!
//! Streamer X runs the same mixer binary and answers on the same vendor
//! interface, so it would otherwise look like a console this app can drive. It
//! is listed here to be refused by name rather than mistaken for one.

/// RØDE.
pub const VID: u16 = 0x19f7;

/// Ordered `multi, extendmulti, extendstereo, stereo, update, syncmode`.
pub const PRO_II_PIDS: [u16; 6] = [0x0092, 0x0094, 0x0078, 0x0037, 0x0030, 0x008d];
pub const DUO_PIDS: [u16; 6] = [0x0093, 0x0095, 0x0079, 0x0050, 0x004f, 0x008e];

/// Same firmware family, no routing matrix. Refused, not driven.
pub const STREAMER_X_PIDS: [u16; 2] = [0x0055, 0x0038];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Console {
    ProII,
    Duo,
}

impl Console {
    pub fn name(self) -> &'static str {
        match self {
            Self::ProII => "RØDECaster Pro II",
            Self::Duo => "RØDECaster Duo",
        }
    }
}

/// What a product id identifies, if anything this app can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Supported(Console),
    /// Recognised, but it has no routing matrix to drive.
    Unsupported(&'static str),
    /// Not in any list. Could be a console in a mode added by a later firmware.
    Unknown,
}

pub fn identify(pid: u16) -> Support {
    if PRO_II_PIDS.contains(&pid) {
        Support::Supported(Console::ProII)
    } else if DUO_PIDS.contains(&pid) {
        Support::Supported(Console::Duo)
    } else if STREAMER_X_PIDS.contains(&pid) {
        Support::Unsupported("Streamer X")
    } else {
        Support::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_of_a_console_is_recognised() {
        for pid in PRO_II_PIDS {
            assert_eq!(identify(pid), Support::Supported(Console::ProII), "{pid:#06x}");
        }
        for pid in DUO_PIDS {
            assert_eq!(identify(pid), Support::Supported(Console::Duo), "{pid:#06x}");
        }
    }

    /// The unit this was reverse-engineered on, in its usual configuration.
    #[test]
    fn the_captured_product_id_is_a_pro_ii() {
        assert_eq!(identify(0x0078), Support::Supported(Console::ProII));
    }

    /// 0x0030 was once recorded as a different hardware revision. It is a
    /// Pro II in update mode.
    #[test]
    fn the_update_mode_id_is_not_a_separate_device() {
        assert_eq!(identify(0x0030), Support::Supported(Console::ProII));
    }

    #[test]
    fn streamer_x_is_refused_by_name() {
        for pid in STREAMER_X_PIDS {
            assert_eq!(identify(pid), Support::Unsupported("Streamer X"), "{pid:#06x}");
        }
    }

    #[test]
    fn an_unlisted_id_is_unknown_rather_than_supported() {
        assert_eq!(identify(0x0001), Support::Unknown);
    }

    /// A model's ids must not overlap another's, or identify would depend on
    /// the order the lists are checked.
    #[test]
    fn no_product_id_belongs_to_two_models() {
        let mut all: Vec<u16> =
            PRO_II_PIDS.iter().chain(&DUO_PIDS).chain(&STREAMER_X_PIDS).copied().collect();
        let total = all.len();
        all.sort_unstable();
        all.dedup();

        assert_eq!(all.len(), total, "a product id appears in more than one list");
    }
}
