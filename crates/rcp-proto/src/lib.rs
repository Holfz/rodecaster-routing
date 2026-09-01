//! RØDECaster Pro II vendor-HID protocol, interface 9.
//!
//! Endpoints, from the device's own configuration descriptor:
//!
//! ```text
//! 07 05 85 03 3f 00 01   EP 0x85 IN,  interrupt - events from the console
//! 07 05 05 03 3f 00 01   EP 0x05 OUT, interrupt - commands to the console
//! ```
//!
//! Report ID selects the direction: 0x04 event, 0x03 command. Both share one
//! frame layout, which is what makes the protocol replayable.

pub mod dump;
pub mod usb;

pub use usb::VID;

/// Report ID of an event frame, device to host.
pub const RID_EVENT: u8 = 0x04;
/// Report ID of a command frame, host to device.
pub const RID_COMMAND: u8 = 0x03;

/// A typed value in the argument list.
///
/// Every value is `01 <len> <type> <payload>` where `len` counts the type byte
/// plus its payload, so a value is always `2 + len` bytes on the wire.
///
/// | type | meaning | len |
/// |---|---|---|
/// | 0x01 | signed 32-bit int, little-endian | 5 |
/// | 0x02 | boolean **true** | 1 |
/// | 0x03 | boolean **false** | 1 |
/// | 0x04 | 64-bit float, little-endian | 9 |
/// | 0x05 | null-terminated string | 1 + strlen + 1 |
/// | 0x08 | group of nested values | varies |
///
/// Booleans carry no payload: the type byte *is* the value. `[01 01 02]` and
/// `[01 01 03]` are `true` and `false`, not two copies of one message.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i32),
    /// 64-bit float, e.g. levels and pan, which the console sends as doubles.
    Float(f64),
    Bool(bool),
    Str(String),
    Group(Vec<Value>),
    Unknown { ty: u8, data: Vec<u8> },
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::Group(v) => {
                let inner: Vec<String> = v.iter().map(|x| x.to_string()).collect();
                write!(f, "({})", inner.join(", "))
            }
            Value::Unknown { ty, data } => {
                let hex: Vec<String> = data.iter().map(|b| format!("{b:02x}")).collect();
                write!(f, "<type {ty:#04x}: {}>", hex.join(" "))
            }
        }
    }
}

/// Decode one value, returning it and how many bytes it consumed.
pub fn parse_value(data: &[u8]) -> Option<(Value, usize)> {
    if data.len() < 3 || data[0] != 0x01 {
        return None;
    }
    let len = data[1] as usize;
    if len == 0 {
        return None;
    }
    let end = 2 + len;
    if end > data.len() {
        return None;
    }
    let ty = data[2];
    let body = &data[3..end];

    let value = match ty {
        0x01 => {
            let mut b = [0u8; 4];
            let n = body.len().min(4);
            b[..n].copy_from_slice(&body[..n]);
            Value::Int(i32::from_le_bytes(b))
        }
        0x02 => Value::Bool(true),
        0x03 => Value::Bool(false),
        0x04 => {
            let mut b = [0u8; 8];
            let n = body.len().min(8);
            b[..n].copy_from_slice(&body[..n]);
            Value::Float(f64::from_le_bytes(b))
        }
        0x05 => {
            let s = body.split(|b| *b == 0).next().unwrap_or(body);
            Value::Str(String::from_utf8_lossy(s).into_owned())
        }
        0x08 => Value::Group(parse_values(body)),
        _ => Value::Unknown { ty, data: body.to_vec() },
    };
    Some((value, end))
}

/// Decode an argument list, stopping at the first thing that is not a value.
pub fn parse_values(mut data: &[u8]) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some((v, used)) = parse_value(data) {
        out.push(v);
        data = &data[used..];
    }
    out
}

/// A decoded vendor-HID frame, either direction.
///
/// ```text
/// 0x00       report id: 0x04 event (from device) / 0x03 command (to device)
/// 0x01..0x05 payload length, u32 little-endian, counted from 0x05
/// ---------- payload starts here ----------
/// 01 01 01   constant in every frame seen so far
/// <n> <id>   n-byte subsystem id (0xe8 mix/routing, 0x10 network, 0x07 UI)
/// <name> 00  null-terminated ASCII event name
/// <values>   zero or more encoded values
/// ---------- zero padding to the report size ----------
/// ```
#[derive(Debug, Clone)]
pub struct Frame {
    pub rid: u8,
    pub payload_len: u32,
    /// Subsystem id. `0xe8` is mix/routing, `0x10` network, `0x07` UI.
    pub id: Vec<u8>,
    pub name: String,
    pub values: Vec<Value>,
    /// Raw bytes after the name's terminator, before decoding.
    pub args: Vec<u8>,
    pub raw: Vec<u8>,
}

impl Frame {
    /// Parse a single-report frame: report id, u32 length, payload.
    ///
    /// Longer payloads span several reports; see `Frame::from_payload`.
    pub fn parse(data: &[u8]) -> Option<Frame> {
        if data.len() < 12 {
            return None;
        }
        let rid = data[0];
        if rid != RID_EVENT && rid != RID_COMMAND {
            return None;
        }
        let payload_len = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
        let end = (5 + payload_len).min(data.len());
        Frame::from_payload(rid, data.get(5..end)?)
    }

    /// Parse an already-reassembled payload.
    pub fn from_payload(rid: u8, payload: &[u8]) -> Option<Frame> {
        let payload_len = payload.len() as u32;

        // 3-byte constant, then a length-prefixed id.
        if payload.len() < 5 || payload[0..3] != [0x01, 0x01, 0x01] {
            return None;
        }
        let idlen = payload[3] as usize;
        let id_end = 4 + idlen;
        let id = payload.get(4..id_end)?.to_vec();

        let rest = payload.get(id_end..)?;
        let name_end = rest.iter().position(|b| *b == 0)?;
        let name = std::str::from_utf8(&rest[..name_end]).ok()?;
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_graphic()) {
            return None;
        }

        let args = rest[name_end + 1..].to_vec();
        Some(Frame {
            rid,
            payload_len,
            id,
            name: name.to_string(),
            values: parse_values(&args),
            args,
            raw: payload.to_vec(),
        })
    }

    pub fn is_command(&self) -> bool {
        self.rid == RID_COMMAND
    }

    /// Subsystem id as hex, for display.
    pub fn id_hex(&self) -> String {
        self.id.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join("")
    }

    /// Decoded values rendered for a log line.
    pub fn values_str(&self) -> String {
        self.values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ")
    }

    /// Raw argument bytes as 3-byte groups, for eyeballing undecoded structure.
    pub fn tuples(&self) -> Vec<[u8; 3]> {
        self.args
            .chunks(3)
            .filter(|c| c.len() == 3)
            .map(|c| [c[0], c[1], c[2]])
            .collect()
    }

    /// This frame's bytes as they appear on the wire, without the zero padding.
    ///
    /// `raw` holds only the payload, so the id and length are rebuilt rather
    /// than stored twice.
    pub fn raw_report(&self) -> Vec<u8> {
        Frame::encode(self.rid, &self.id, &self.name, &self.args)
    }

    /// Build a frame for replay to EP 0x05. Caller pads to the report size.
    pub fn encode(rid: u8, id: &[u8], name: &str, args: &[u8]) -> Vec<u8> {
        let mut payload = vec![0x01, 0x01, 0x01, id.len() as u8];
        payload.extend_from_slice(id);
        payload.extend_from_slice(name.as_bytes());
        payload.push(0);
        payload.extend_from_slice(args);

        let mut out = vec![rid];
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }
}

/// Printable ASCII runs of >= `min` chars, for frames we cannot parse.
pub fn strings(data: &[u8], min: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for b in data {
        if b.is_ascii_graphic() {
            cur.push(*b as char);
        } else if cur.len() >= min {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.clear();
        }
    }
    if cur.len() >= min {
        out.push(cur);
    }
    out
}

/// Ids are little-endian integers of the smallest length that fits.
pub fn id_bytes(id: u32) -> Vec<u8> {
    let mut b = id.to_le_bytes().to_vec();
    while b.len() > 1 && *b.last().unwrap() == 0 {
        b.pop();
    }
    b
}

/// The state request the app sends on connect. Not the usual frame layout.
pub fn state_request_frame() -> Vec<u8> {
    vec![0x03, 0x04, 0x00, 0x00, 0x00, 0xad, 0x10, 0xa7, 0xb0]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every case below is a real frame captured off the wire. Addresses are
    /// swapped for documentation-range ones of the same length, so the encoded
    /// lengths still match the capture.
    fn frame(hex: &str) -> Frame {
        let bytes: Vec<u8> = hex
            .split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();
        Frame::parse(&bytes).expect("should parse")
    }

    #[test]
    fn decodes_a_boolean_event() {
        let f = frame("04 10 00 00 00 01 01 01 01 e8 6d 69 78 4c 69 6e 6b 00 01 01 02");
        assert_eq!(f.name, "mixLink");
        assert_eq!(f.id_hex(), "e8");
        assert_eq!(f.values, vec![Value::Bool(true)]);
        assert!(!f.is_command());
    }

    #[test]
    fn boolean_false_is_type_03() {
        let f = frame("04 10 00 00 00 01 01 01 01 e8 6d 69 78 4d 75 74 65 00 01 01 03");
        assert_eq!(f.name, "mixMute");
        assert_eq!(f.values, vec![Value::Bool(false)]);
    }

    #[test]
    fn decodes_a_string_event() {
        let f = frame(
            "04 21 00 00 00 01 01 01 01 10 77 69 66 69 49 70 41 64 64 72 65 73 73 00 \
             01 0c 05 31 39 32 2e 30 2e 32 2e 31 31 00",
        );
        assert_eq!(f.name, "wifiIpAddress");
        assert_eq!(f.values, vec![Value::Str("192.0.2.11".into())]);
    }

    #[test]
    fn decodes_a_command_with_a_group() {
        let f = frame(
            "03 1f 00 00 00 01 01 01 01 e8 6d 69 78 55 6e 6c 69 6e 6b 52 65 71 75 65 \
             73 74 00 01 07 08 01 01 02 01 01 02",
        );
        assert_eq!(f.name, "mixUnlinkRequest");
        assert!(f.is_command());
        assert_eq!(
            f.values,
            vec![Value::Group(vec![Value::Bool(true), Value::Bool(true)])]
        );
    }

    #[test]
    fn encode_round_trips_a_captured_command() {
        let args = [0x01, 0x07, 0x08, 0x01, 0x01, 0x02, 0x01, 0x01, 0x02];
        let out = Frame::encode(RID_COMMAND, &[0xe8], "mixUnlinkRequest", &args);
        let expect: Vec<u8> = "03 1f 00 00 00 01 01 01 01 e8 6d 69 78 55 6e 6c 69 6e 6b 52 65 \
                               71 75 65 73 74 00 01 07 08 01 01 02 01 01 02"
            .split_whitespace()
            .map(|h| u8::from_str_radix(h, 16).unwrap())
            .collect();
        assert_eq!(out, expect);
    }

    #[test]
    fn ids_past_255_take_two_bytes() {
        assert_eq!(id_bytes(232), vec![0xe8]);
        assert_eq!(id_bytes(300), vec![0x2c, 0x01]);
        // The last cell of the grid is 465, so two-byte ids are routine.
        assert_eq!(id_bytes(465), vec![0xd1, 0x01]);
    }
}
