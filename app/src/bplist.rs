//! Apple binary property lists ("bplist00"), read-only, plus the
//! NSKeyedArchiver unwrapping Procreate's Brush.archive needs. No crate:
//! the format is small (a trailer, an offset table, typed objects).

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Str(String),
    Data(Vec<u8>),
    Uid(u64),
    Array(Vec<Value>),
    Dict(Vec<(String, Value)>),
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Real(r) => Some(*r),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(d) => d.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    offsets: Vec<usize>,
    ref_size: usize,
}

fn be_uint(b: &[u8], at: usize, n: usize) -> Option<u64> {
    let s = b.get(at..at.checked_add(n)?)?;
    if n > 8 {
        return None;
    }
    Some(s.iter().fold(0u64, |a, &x| (a << 8) | x as u64))
}

/// Parse a binary plist into its top object. None on anything malformed.
pub fn parse(b: &[u8]) -> Option<Value> {
    if b.len() < 40 || &b[..8] != b"bplist00" {
        return None;
    }
    let t = b.len() - 32;
    let off_size = b[t + 6] as usize;
    let ref_size = b[t + 7] as usize;
    let n = be_uint(b, t + 8, 8)? as usize;
    let top = be_uint(b, t + 16, 8)? as usize;
    let table = be_uint(b, t + 24, 8)? as usize;
    if off_size == 0 || ref_size == 0 || n == 0 || n > 1_000_000 || top >= n {
        return None;
    }
    let mut offsets = Vec::with_capacity(n);
    for i in 0..n {
        offsets.push(be_uint(b, table + i * off_size, off_size)? as usize);
    }
    Reader { b, offsets, ref_size }.object(top, 0)
}

impl Reader<'_> {
    /// Length from a marker's low nibble, or the int object after it.
    fn len(&self, low: u8, at: usize) -> Option<(usize, usize)> {
        if low != 0xF {
            return Some((low as usize, at + 1));
        }
        let m = *self.b.get(at + 1)?;
        if m >> 4 != 0x1 {
            return None;
        }
        let w = 1usize << (m & 0xF);
        Some((be_uint(self.b, at + 2, w)? as usize, at + 2 + w))
    }

    fn object(&self, idx: usize, depth: u32) -> Option<Value> {
        if depth > 64 {
            return None;
        }
        let at = *self.offsets.get(idx)?;
        let m = *self.b.get(at)?;
        let (hi, low) = (m >> 4, m & 0xF);
        Some(match hi {
            0x0 => match low {
                0x8 => Value::Bool(false),
                0x9 => Value::Bool(true),
                _ => Value::Null,
            },
            0x1 => {
                let w = 1usize << low;
                Value::Int(be_uint(self.b, at + 1, w)? as i64)
            }
            0x2 => match low {
                2 => Value::Real(f32::from_bits(be_uint(self.b, at + 1, 4)? as u32) as f64),
                3 => Value::Real(f64::from_bits(be_uint(self.b, at + 1, 8)?)),
                _ => return None,
            },
            0x3 => Value::Real(f64::from_bits(be_uint(self.b, at + 1, 8)?)),
            0x4 => {
                let (n, s) = self.len(low, at)?;
                Value::Data(self.b.get(s..s.checked_add(n)?)?.to_vec())
            }
            0x5 => {
                let (n, s) = self.len(low, at)?;
                Value::Str(String::from_utf8_lossy(self.b.get(s..s.checked_add(n)?)?).into())
            }
            0x6 => {
                let (n, s) = self.len(low, at)?;
                let raw = self.b.get(s..s.checked_add(n.checked_mul(2)?)?)?;
                let u: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                Value::Str(String::from_utf16_lossy(&u))
            }
            0x8 => Value::Uid(be_uint(self.b, at + 1, low as usize + 1)?),
            0xA => {
                let (n, s) = self.len(low, at)?;
                let mut v = Vec::with_capacity(n.min(4096));
                for i in 0..n {
                    let r = be_uint(self.b, s + i * self.ref_size, self.ref_size)? as usize;
                    v.push(self.object(r, depth + 1)?);
                }
                Value::Array(v)
            }
            0xD => {
                let (n, s) = self.len(low, at)?;
                let mut d = Vec::with_capacity(n.min(4096));
                for i in 0..n {
                    let kr = be_uint(self.b, s + i * self.ref_size, self.ref_size)? as usize;
                    let vr = be_uint(self.b, s + (n + i) * self.ref_size, self.ref_size)? as usize;
                    let Value::Str(k) = self.object(kr, depth + 1)? else {
                        return None;
                    };
                    d.push((k, self.object(vr, depth + 1)?));
                }
                Value::Dict(d)
            }
            _ => return None,
        })
    }
}

/// NSKeyedArchiver: the root object's fields with every UID reference
/// resolved one level ("$null" → Null). None when it is not an archive.
pub fn keyed_root(top: &Value) -> Option<Vec<(String, Value)>> {
    let Some(Value::Array(objects)) = top.get("$objects") else {
        return None;
    };
    let Some(Value::Uid(root)) = top.get("$top").and_then(|t| t.get("root")) else {
        return None;
    };
    let Some(Value::Dict(fields)) = objects.get(*root as usize) else {
        return None;
    };
    Some(
        fields
            .iter()
            .filter(|(k, _)| k != "$class")
            .map(|(k, v)| {
                let v = match v {
                    Value::Uid(u) => match objects.get(*u as usize) {
                        Some(Value::Str(s)) if s == "$null" => Value::Null,
                        Some(o) => o.clone(),
                        None => Value::Null,
                    },
                    o => o.clone(),
                };
                (k.clone(), v)
            })
            .collect(),
    )
}

/// Writer for tests: one object per tree node, 2-byte refs, 4-byte offsets.
#[cfg(test)]
pub fn write(top: &Value) -> Vec<u8> {
    fn flatten(v: &Value, objs: &mut Vec<Vec<u8>>) -> usize {
        let idx = objs.len();
        objs.push(Vec::new());
        fn head(out: &mut Vec<u8>, hi: u8, n: usize) {
            if n < 15 {
                out.push(hi << 4 | n as u8);
            } else {
                out.push(hi << 4 | 0xF);
                out.push(0x13);
                out.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        let mut out = Vec::new();
        match v {
            Value::Null => out.push(0x00),
            Value::Bool(b) => out.push(if *b { 0x09 } else { 0x08 }),
            Value::Int(i) => {
                out.push(0x13);
                out.extend_from_slice(&i.to_be_bytes());
            }
            Value::Real(r) => {
                out.push(0x23);
                out.extend_from_slice(&r.to_bits().to_be_bytes());
            }
            Value::Str(s) => {
                head(&mut out, 0x5, s.len());
                out.extend_from_slice(s.as_bytes());
            }
            Value::Data(d) => {
                head(&mut out, 0x4, d.len());
                out.extend_from_slice(d);
            }
            Value::Uid(u) => {
                out.push(0x87);
                out.extend_from_slice(&u.to_be_bytes());
            }
            Value::Array(a) => {
                let refs: Vec<usize> = a.iter().map(|x| flatten(x, objs)).collect();
                head(&mut out, 0xA, refs.len());
                for r in refs {
                    out.extend_from_slice(&(r as u16).to_be_bytes());
                }
            }
            Value::Dict(d) => {
                let ks: Vec<usize> = d.iter().map(|(k, _)| flatten(&Value::Str(k.clone()), objs)).collect();
                let vs: Vec<usize> = d.iter().map(|(_, x)| flatten(x, objs)).collect();
                head(&mut out, 0xD, d.len());
                for r in ks.into_iter().chain(vs) {
                    out.extend_from_slice(&(r as u16).to_be_bytes());
                }
            }
        }
        objs[idx] = out;
        idx
    }
    let mut objs = Vec::new();
    flatten(top, &mut objs);
    let mut b = b"bplist00".to_vec();
    let mut offs = Vec::new();
    for o in &objs {
        offs.push(b.len() as u32);
        b.extend_from_slice(o);
    }
    let table = b.len() as u64;
    for o in offs {
        b.extend_from_slice(&o.to_be_bytes());
    }
    b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 4, 2]);
    b.extend_from_slice(&(objs.len() as u64).to_be_bytes());
    b.extend_from_slice(&0u64.to_be_bytes());
    b.extend_from_slice(&table.to_be_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_types() {
        let v = Value::Dict(vec![
            ("a".into(), Value::Int(-3)),
            ("b".into(), Value::Real(0.25)),
            ("c".into(), Value::Bool(true)),
            ("d".into(), Value::Str("a string longer than fifteen".into())),
            ("e".into(), Value::Array(vec![Value::Uid(7), Value::Null])),
        ]);
        assert_eq!(parse(&write(&v)), Some(v));
    }

    #[test]
    fn utf16_string_and_f32_real() {
        // Hand-built: top = array [ "é" (utf16), 1.5f32 ].
        let mut b = b"bplist00".to_vec();
        let o0 = b.len();
        b.extend_from_slice(&[0xA2, 1, 2]);
        let o1 = b.len();
        b.extend_from_slice(&[0x61, 0x00, 0xE9]);
        let o2 = b.len();
        b.push(0x22);
        b.extend_from_slice(&1.5f32.to_bits().to_be_bytes());
        let table = b.len();
        b.extend_from_slice(&[o0 as u8, o1 as u8, o2 as u8]);
        b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 1]);
        b.extend_from_slice(&3u64.to_be_bytes());
        b.extend_from_slice(&0u64.to_be_bytes());
        b.extend_from_slice(&(table as u64).to_be_bytes());
        assert_eq!(
            parse(&b),
            Some(Value::Array(vec![Value::Str("é".into()), Value::Real(1.5)]))
        );
    }

    #[test]
    fn malformed_is_none() {
        assert_eq!(parse(b"bplist00"), None);
        let mut b = write(&Value::Int(1));
        let n = b.len();
        b[n - 9] = 0xFF; // offset table points past the end
        assert_eq!(parse(&b), None);
        // A self-referencing array stops at the depth guard.
        let mut c = b"bplist00".to_vec();
        c.extend_from_slice(&[0xA1, 0]);
        c.push(8);
        c.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1, 1]);
        c.extend_from_slice(&1u64.to_be_bytes());
        c.extend_from_slice(&0u64.to_be_bytes());
        c.extend_from_slice(&10u64.to_be_bytes());
        assert_eq!(parse(&c), None);
    }

    #[test]
    fn keyed_root_resolves_uids() {
        let top = Value::Dict(vec![
            ("$archiver".into(), Value::Str("NSKeyedArchiver".into())),
            (
                "$objects".into(),
                Value::Array(vec![
                    Value::Str("$null".into()),
                    Value::Dict(vec![
                        ("$class".into(), Value::Uid(3)),
                        ("name".into(), Value::Uid(2)),
                        ("plotSpacing".into(), Value::Real(0.3)),
                        ("authorName".into(), Value::Uid(0)),
                    ]),
                    Value::Str("Ink".into()),
                    Value::Dict(vec![("$classname".into(), Value::Str("SilicaBrush".into()))]),
                ]),
            ),
            ("$top".into(), Value::Dict(vec![("root".into(), Value::Uid(1))])),
        ]);
        let f = keyed_root(&parse(&write(&top)).unwrap()).unwrap();
        assert_eq!(
            f,
            vec![
                ("name".into(), Value::Str("Ink".into())),
                ("plotSpacing".into(), Value::Real(0.3)),
                ("authorName".into(), Value::Null),
            ]
        );
    }
}
