//! Talking to the console: open interface 9, read state, send commands.
//!
//! This crate is transport only. Frame contents live in `rcp-proto`, and what
//! they mean lives in `rcp-model`.

use hidapi::{HidApi, HidDevice};
use rcp_model::command::{
    cell_frame, input_colour_frame, monitor_level_frame, monitor_mute_frame, output_mode_frame, Set,
};
use rcp_model::model::{self, Model};
use rcp_proto::{state_request_frame, usb, Frame, RID_EVENT, VID};

/// Output report size seen on the wire, report id included.
const REPORT_SIZE: usize = 256;
/// Below this, a reply is an ordinary event rather than the state dump.
const DUMP_MIN: usize = 4096;

pub struct Device {
    dev: HidDevice,
}

impl Device {
    /// Opens the first console this app can drive.
    ///
    /// Matched on vendor plus the interface rather than the product id, because
    /// the id moves with the USB mode. The id is then used to reject hardware
    /// that answers here without having a routing matrix: Streamer X runs the
    /// same mixer binary on the same interface, so nothing downstream would
    /// tell it apart.
    pub fn open() -> Result<Device, String> {
        let api = HidApi::new().map_err(|e| e.to_string())?;

        let candidates: Vec<_> = api
            .device_list()
            .filter(|d| d.vendor_id() == VID && d.interface_number() == 9)
            .collect();
        if candidates.is_empty() {
            return Err("no RØDE device exposing vendor interface 9 is attached".into());
        }

        let info = candidates
            .iter()
            .find(|d| matches!(usb::identify(d.product_id()), usb::Support::Supported(_)))
            .ok_or_else(|| refusal(&candidates))?;

        let dev = info
            .open_device(&api)
            .map_err(|e| format!("found the console but could not open interface 9: {e}"))?;

        Ok(Device { dev })
    }

    fn write_report(&self, frame: Vec<u8>) -> Result<(), String> {
        let mut report = frame;
        report.resize(REPORT_SIZE, 0);
        self.dev.write(&report).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Ask for the full state and reassemble the ~136 KB reply.
    pub fn read_state(&self) -> Result<Vec<u8>, String> {
        self.write_report(state_request_frame())?;

        let mut buf = [0u8; 1024];
        let mut need = 0usize;
        let mut out: Vec<u8> = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);

        while std::time::Instant::now() < deadline {
            let n = self.dev.read_timeout(&mut buf, 1000).map_err(|e| e.to_string())?;
            if n == 0 {
                continue;
            }
            let data = &buf[..n];

            // The RØDECaster App's keepalive uses other report ids and is
            // broadcast to every open handle on this interface.
            if data[0] != RID_EVENT {
                continue;
            }

            if out.is_empty() {
                if n < 5 {
                    continue;
                }
                need = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
                if need < DUMP_MIN {
                    continue;
                }
                out.extend_from_slice(&data[5..]);
            } else {
                let take = (need - out.len()).min(n - 1);
                out.extend_from_slice(&data[1..1 + take]);
            }

            if out.len() >= need {
                out.truncate(need);
                return Ok(out);
            }
        }
        Err(format!("timed out after {} of {need} bytes", out.len()))
    }

    pub fn read_model(&self) -> Result<Model, String> {
        Ok(model::scan(&self.read_state()?))
    }

    pub fn send_cell(&self, id: u32, set: Set) -> Result<(), String> {
        self.write_report(cell_frame(id, set))
    }

    pub fn send_output_mode(&self, id: u32, mode: i32) -> Result<(), String> {
        self.write_report(output_mode_frame(id, mode))
    }

    pub fn send_monitor_mute(&self, id: u32, mute: bool) -> Result<(), String> {
        self.write_report(monitor_mute_frame(id, mute))
    }

    pub fn send_monitor_level(&self, id: u32, level: f64) -> Result<(), String> {
        self.write_report(monitor_level_frame(id, level))
    }

    pub fn send_input_colour(&self, id: u32, colour: &str) -> Result<(), String> {
        self.write_report(input_colour_frame(id, colour)?)
    }

    /// Write an already-built frame.
    ///
    /// For frames we have not captured, so there is no named builder above.
    /// Callers own the evidence for what they send.
    pub fn send_raw(&self, frame: Vec<u8>) -> Result<(), String> {
        self.write_report(frame)
    }

    /// Read one pushed event, or `None` if nothing arrived within `timeout_ms`.
    ///
    /// Single-report events only. The state dump is skipped, since
    /// reassembling it here would block the caller.
    pub fn next_event(&self, timeout_ms: i32) -> Result<Option<Frame>, String> {
        let mut buf = [0u8; 1024];
        let n = self.dev.read_timeout(&mut buf, timeout_ms).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(None);
        }

        let data = &buf[..n];
        if data[0] != RID_EVENT || n < 5 {
            return Ok(None);
        }

        let len = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
        if len == 0 || len > n - 5 {
            return Ok(None);
        }

        Ok(Frame::parse(data))
    }
}

/// Names what was attached instead, so an unlisted id can be reported.
fn refusal(found: &[&hidapi::DeviceInfo]) -> String {
    let mut named: Vec<String> = Vec::new();

    for d in found {
        named.push(match usb::identify(d.product_id()) {
            usb::Support::Unsupported(name) => {
                format!("{name} (0x{:04x}), which has no routing matrix", d.product_id())
            }
            _ => format!("an unrecognised RØDE device (0x{:04x})", d.product_id()),
        });
    }

    format!(
        "no supported console attached. Found {}. If that is a RODECaster Pro II          or Duo, it is reporting a product id this build does not know.",
        named.join(", ")
    )
}
