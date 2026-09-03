//! Real-time analyser.
//!
//! Nothing here touches the console. The RØDECaster's USB returns arrive as
//! ordinary Windows capture endpoints, the same ones Discord and TeamSpeak
//! list as microphones, so the spectrum is read off one of those rather than
//! out of the HID protocol. Which sources are audible on a given endpoint is
//! decided by the routing matrix: `USB 1 Main` is the "Main Multitrack"
//! endpoint and `USB 1 Comms` is "Chat".
//!
//! `analyzer` is the maths and has no I/O; `capture` owns the cpal stream.

pub mod analyzer;
pub mod capture;

pub use analyzer::{Analyzer, Frame};
pub use capture::{input_devices, Capture, DeviceInfo, Event, StreamInfo};
