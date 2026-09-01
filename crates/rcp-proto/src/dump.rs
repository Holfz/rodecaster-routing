//! The console's state dump, parsed as the tree it actually is.
//!
//! ```text
//! varint := <nbytes:u8> <bytes little-endian>   00 -> 0, "01 69" -> 105, "02 9f 02" -> 671
//! value  := varint(len) <type:u8> <payload: len-1 bytes>
//! prop   := cstr value
//! object := cstr varint(nprops) prop* varint(nchildren) object*
//! ```
//!
//! Byte 0 of the blob is a message tag; the tree starts at offset 1.
//!
//! **Ids are positions, not arithmetic.** An object's id is its index among the
//! root's direct children. Nested descendants consume none, which is why
//! `PHYSICALINTERFACE` owning 105 children does not shift `OUTPUT` off 13.
//! Reading ids off the tree means they stay right on hardware whose object
//! counts differ, where a base worked out from run lengths would not.
//!
//! `parse` keeps whatever it decoded before hitting damage rather than
//! discarding the lot, because a dump reassembled from 534 USB reports can
//! arrive with its tail clipped.

use crate::Value;

/// One object in the dump. `id` is set only on the root's direct children.
#[derive(Debug, Clone)]
pub struct Object {
    pub name: String,
    pub id: Option<u32>,
    pub props: Vec<(String, Value)>,
    pub children: Vec<Object>,
}

impl Object {
    pub fn prop(&self, name: &str) -> Option<&Value> {
        self.props.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

#[derive(Debug, Clone)]
pub struct Dump {
    pub root: Object,
    /// Set when the blob ran out mid-object. Everything before that is kept.
    pub truncated: bool,
}

impl Dump {
    /// Id of the first object of this type, i.e. its base.
    pub fn first_id(&self, ty: &str) -> Option<u32> {
        self.root.children.iter().find(|o| o.name == ty).and_then(|o| o.id)
    }

    /// How many top-level objects of this type the console reported.
    pub fn count(&self, ty: &str) -> usize {
        self.root.children.iter().filter(|o| o.name == ty).count()
    }

    pub fn object(&self, id: u32) -> Option<&Object> {
        self.root.children.get(id as usize)
    }
}

#[derive(Debug, PartialEq)]
pub enum ParseError {
    /// The blob is too short to hold even a root object.
    Empty,
    /// A name held bytes that cannot be part of one.
    BadName(usize),
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

/// Raised inside the walk when the blob runs out. Callers keep what they have.
struct Eof;

impl<'a> Reader<'a> {
    fn varint(&mut self) -> Result<usize, Eof> {
        let n = *self.d.get(self.p).ok_or(Eof)? as usize;
        self.p += 1;
        let bytes = self.d.get(self.p..self.p + n).ok_or(Eof)?;
        self.p += n;

        Ok(bytes.iter().rev().fold(0usize, |a, b| (a << 8) | *b as usize))
    }

    fn cstr(&mut self) -> Result<String, Eof> {
        let end = self.d[self.p..].iter().position(|b| *b == 0).ok_or(Eof)?;
        let s = String::from_utf8_lossy(&self.d[self.p..self.p + end]).into_owned();
        self.p += end + 1;

        Ok(s)
    }

    fn value(&mut self) -> Result<Value, Eof> {
        let len = self.varint()?;
        if len == 0 {
            return Err(Eof);
        }

        let ty = *self.d.get(self.p).ok_or(Eof)?;
        let body = self.d.get(self.p + 1..self.p + len).ok_or(Eof)?.to_vec();
        self.p += len;

        Ok(decode(ty, &body))
    }

    fn object(&mut self) -> Result<Object, Eof> {
        let name = self.cstr()?;

        let n = self.varint()?;
        let mut props = Vec::with_capacity(n);
        for _ in 0..n {
            let k = self.cstr()?;
            props.push((k, self.value()?));
        }

        let n = self.varint()?;
        let mut children = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            children.push(self.object()?);
        }

        Ok(Object { name, id: None, props, children })
    }
}

/// Booleans carry no payload: the type byte is the value.
fn decode(ty: u8, body: &[u8]) -> Value {
    match ty {
        0x01 => body
            .get(..4)
            .and_then(|b| b.try_into().ok())
            .map(|b| Value::Int(i32::from_le_bytes(b)))
            .unwrap_or(Value::Unknown { ty, data: body.to_vec() }),
        0x02 => Value::Bool(true),
        0x03 => Value::Bool(false),
        0x04 => body
            .get(..8)
            .and_then(|b| b.try_into().ok())
            .map(|b| Value::Float(f64::from_le_bytes(b)))
            .unwrap_or(Value::Unknown { ty, data: body.to_vec() }),
        0x05 => {
            let end = body.iter().position(|b| *b == 0).unwrap_or(body.len());
            Value::Str(String::from_utf8_lossy(&body[..end]).into_owned())
        }
        0x08 => Value::Group(crate::parse_values(body)),
        _ => Value::Unknown { ty, data: body.to_vec() },
    }
}

pub fn parse(blob: &[u8]) -> Result<Dump, ParseError> {
    if blob.len() < 2 {
        return Err(ParseError::Empty);
    }

    let mut r = Reader { d: blob, p: 1 };
    let mut truncated = false;

    let name = r.cstr().map_err(|_| ParseError::Empty)?;
    if name.is_empty() || !name.bytes().all(|b| (32..127).contains(&b)) {
        return Err(ParseError::BadName(1));
    }

    let mut props = Vec::new();
    match r.varint() {
        Ok(n) => {
            for _ in 0..n {
                match (r.cstr(), r.value()) {
                    (Ok(k), Ok(v)) => props.push((k, v)),
                    _ => {
                        truncated = true;
                        break;
                    }
                }
            }
        }
        Err(_) => truncated = true,
    }

    // Keep every child that parsed. A clipped tail costs the last object, not
    // the 670 before it.
    let mut children = Vec::new();
    if !truncated {
        match r.varint() {
            Ok(n) => {
                for _ in 0..n {
                    match r.object() {
                        Ok(mut o) => {
                            o.id = Some(children.len() as u32);
                            children.push(o);
                        }
                        Err(Eof) => {
                            truncated = true;
                            break;
                        }
                    }
                }
            }
            Err(_) => truncated = true,
        }
    }

    Ok(Dump { root: Object { name, id: None, props, children }, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a blob the way the console lays one out, so the tests exercise
    /// the grammar rather than one captured sample.
    #[derive(Default)]
    struct Build(Vec<u8>);

    impl Build {
        fn varint(&mut self, v: usize) {
            let mut b = v.to_le_bytes().to_vec();
            while b.len() > 1 && *b.last().unwrap() == 0 {
                b.pop();
            }
            if v == 0 {
                self.0.push(0);
                return;
            }
            self.0.push(b.len() as u8);
            self.0.extend_from_slice(&b);
        }

        fn cstr(&mut self, s: &str) {
            self.0.extend_from_slice(s.as_bytes());
            self.0.push(0);
        }

        fn value(&mut self, ty: u8, body: &[u8]) {
            self.varint(body.len() + 1);
            self.0.push(ty);
            self.0.extend_from_slice(body);
        }

        fn object(&mut self, name: &str, props: &[(&str, u8, Vec<u8>)], nchildren: usize) {
            self.cstr(name);
            self.varint(props.len());
            for (k, ty, body) in props {
                self.cstr(k);
                self.value(*ty, body);
            }
            self.varint(nchildren);
        }
    }

    /// Root with three children, the second of which nests two of its own.
    fn sample() -> Vec<u8> {
        let mut b = Build(vec![0x02]);
        b.object("Rodecaster", &[], 3);
        b.object("ALPHA", &[("flag", 0x02, vec![])], 0);
        b.object("BETA", &[("n", 0x01, 42i32.to_le_bytes().to_vec())], 2);
        b.object("NESTED", &[], 0);
        b.object("NESTED", &[], 0);
        b.object("GAMMA", &[("s", 0x05, b"hi\0".to_vec())], 0);
        b.0
    }

    #[test]
    fn ids_number_the_roots_children_and_skip_nested_ones() {
        let d = parse(&sample()).unwrap();
        assert!(!d.truncated);
        assert_eq!(d.root.children.len(), 3);

        // GAMMA is id 2 even though two NESTED objects sit between it and BETA.
        assert_eq!(d.first_id("ALPHA"), Some(0));
        assert_eq!(d.first_id("BETA"), Some(1));
        assert_eq!(d.first_id("GAMMA"), Some(2));
        assert_eq!(d.count("NESTED"), 0);
    }

    #[test]
    fn values_decode_by_type() {
        let d = parse(&sample()).unwrap();
        assert_eq!(d.object(0).unwrap().prop("flag"), Some(&Value::Bool(true)));
        assert_eq!(d.object(1).unwrap().prop("n"), Some(&Value::Int(42)));
        assert_eq!(d.object(2).unwrap().prop("s"), Some(&Value::Str("hi".into())));
    }

    #[test]
    fn a_two_byte_length_is_read_as_one_number() {
        // fxPresetContents runs past 255 bytes, so the length needs two.
        let long = "x".repeat(300);
        let mut body = long.clone().into_bytes();
        body.push(0);

        let mut b = Build(vec![0x02]);
        b.object("Rodecaster", &[], 1);
        b.object("BIG", &[("s", 0x05, body)], 0);

        let d = parse(&b.0).unwrap();
        assert_eq!(d.object(0).unwrap().prop("s"), Some(&Value::Str(long)));
    }

    #[test]
    fn a_clipped_tail_keeps_every_object_before_it() {
        let full = sample();
        let d = parse(&full[..full.len() - 4]).unwrap();

        assert!(d.truncated);
        assert_eq!(d.root.children.len(), 2);
        assert_eq!(d.first_id("ALPHA"), Some(0));
    }

    #[test]
    fn an_unknown_value_type_is_carried_rather_than_fatal() {
        let mut b = Build(vec![0x02]);
        b.object("Rodecaster", &[], 1);
        b.object("ODD", &[("x", 0x7e, vec![1, 2, 3])], 0);

        let d = parse(&b.0).unwrap();
        assert_eq!(
            d.object(0).unwrap().prop("x"),
            Some(&Value::Unknown { ty: 0x7e, data: vec![1, 2, 3] })
        );
    }

    /// Guards the ids against a real console. Ignored by default because the
    /// captured dump lives in dev/, which is not published: without `#[ignore]`
    /// this would report `ok` on a clone that has no blob to read, which is a
    /// worse signal than not running at all. The grammar tests above need no
    /// fixture and do run everywhere.
    ///
    /// Run it with `cargo test -- --ignored`.
    #[test]
    #[ignore = "needs dev/docs/route-04.pcap.blob-5038ms.bin, which is not published"]
    fn the_captured_dump_reproduces_every_confirmed_id() {
        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../dev/docs/route-04.pcap.blob-5038ms.bin");
        let blob = std::fs::read(path).expect("dev/ present but the captured dump is missing");

        let d = parse(&blob).unwrap();

        assert_eq!(d.first_id("ENCODER"), Some(6));
        assert_eq!(d.first_id("OUTPUT"), Some(13));
        assert_eq!(d.first_id("MIXMINUSES"), Some(49));
        assert_eq!(d.first_id("MIX"), Some(76));
        assert_eq!(d.first_id("INPUTSOURCE"), Some(622));

        assert_eq!(d.count("MIX"), d.count("INPUTSOURCE") * d.count("MIXMINUSES"));
    }

    #[test]
    fn an_empty_blob_is_an_error_not_a_panic() {
        assert!(matches!(parse(&[]), Err(ParseError::Empty)));
        assert!(matches!(parse(&[0x02]), Err(ParseError::Empty)));
    }
}
