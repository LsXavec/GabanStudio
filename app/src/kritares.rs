//! Krita resource decoders (PSD-brush-engine stage A): GIMP brush (.gbr),
//! animated brush (.gih, first frame), GIMP pattern (.pat) — the formats
//! Krita bundles carry tips and grains in — decoded to plain RGBA8 so the
//! tip/grain caches hold nothing but PNG.
//!
//! All three are simple big-endian headers + raw pixels; every read is
//! bounds-checked and a malformed file returns None (counted by the
//! importer, never guessed at — room NEVER-DO 4).

/// A decoded image: RGBA8, row-major.
pub struct ResImage {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Grayscale-or-RGBA raw pixels to RGBA8. GIMP brush grayscale is an
/// ALPHA mask (255 = full paint), so gray becomes white-with-alpha —
/// exactly what a tip mask wants.
fn raw_to_rgba(data: &[u8], w: u32, h: u32, bytes: u32) -> Option<Vec<u8>> {
    let n = (w as usize).checked_mul(h as usize)?;
    match bytes {
        1 => {
            let px = data.get(..n)?;
            Some(px.iter().flat_map(|&g| [255, 255, 255, g]).collect())
        }
        3 => {
            let px = data.get(..n * 3)?;
            Some(px.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect())
        }
        4 => Some(data.get(..n * 4)?.to_vec()),
        _ => None,
    }
}

/// GIMP .gbr: header { size, version, width, height, bytes, magic "GIMP",
/// spacing, name[...] } then raw pixels.
pub fn decode_gbr(b: &[u8]) -> Option<ResImage> {
    let hdr = be_u32(b, 0)? as usize;
    let version = be_u32(b, 4)?;
    let w = be_u32(b, 8)?;
    let h = be_u32(b, 12)?;
    let bytes = be_u32(b, 16)?;
    if !(1..=4).contains(&bytes) || w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    if version >= 2 && b.get(20..24)? != b"GIMP" {
        return None;
    }
    let rgba = raw_to_rgba(b.get(hdr..)?, w, h, bytes)?;
    Some(ResImage { w, h, rgba })
}

/// GIMP .gih: line 1 = name, line 2 = "<count> <params...>", then `count`
/// concatenated .gbr images. First frame only (an animated pipe brush is
/// a stamp sequence; one honest frame beats a faked cycle — logged in the
/// room).
pub fn decode_gih_first(b: &[u8]) -> Option<ResImage> {
    // Two text lines, each \n-terminated, before the first GBR header.
    let first_nl = b.iter().position(|&c| c == b'\n')?;
    let rest = &b[first_nl + 1..];
    let second_nl = rest.iter().position(|&c| c == b'\n')?;
    decode_gbr(&rest[second_nl + 1..])
}

/// GIMP .pat: header { size, version, width, height, bytes, magic "GPAT",
/// name[...] } then raw pixels.
pub fn decode_pat(b: &[u8]) -> Option<ResImage> {
    let hdr = be_u32(b, 0)? as usize;
    let w = be_u32(b, 8)?;
    let h = be_u32(b, 12)?;
    let bytes = be_u32(b, 16)?;
    if b.get(20..24)? != b"GPAT" || w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    // Patterns are IMAGES (paper grain), not alpha masks: gray stays gray.
    let n = (w as usize).checked_mul(h as usize)?;
    let data = b.get(hdr..)?;
    let rgba = match bytes {
        1 => {
            let px = data.get(..n)?;
            px.iter().flat_map(|&g| [g, g, g, 255]).collect()
        }
        _ => raw_to_rgba(data, w, h, bytes)?,
    };
    Some(ResImage { w, h, rgba })
}

/// Any supported resource by file extension (png handled by the caller's
/// png crate path).
pub fn decode_by_ext(ext: &str, bytes: &[u8]) -> Option<ResImage> {
    match ext {
        "gbr" => decode_gbr(bytes),
        "gih" => decode_gih_first(bytes),
        "pat" => decode_pat(bytes),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gbr_bytes(w: u32, h: u32, bytes: u32, px: &[u8]) -> Vec<u8> {
        let name = b"t\0";
        let hdr = 28 + name.len() as u32;
        let mut v = Vec::new();
        v.extend_from_slice(&hdr.to_be_bytes());
        v.extend_from_slice(&2u32.to_be_bytes());
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&bytes.to_be_bytes());
        v.extend_from_slice(b"GIMP");
        v.extend_from_slice(&10u32.to_be_bytes());
        v.extend_from_slice(name);
        v.extend_from_slice(px);
        v
    }

    #[test]
    fn gbr_gray_becomes_alpha_mask() {
        let b = gbr_bytes(2, 1, 1, &[0, 200]);
        let img = decode_gbr(&b).unwrap();
        assert_eq!((img.w, img.h), (2, 1));
        assert_eq!(img.rgba, vec![255, 255, 255, 0, 255, 255, 255, 200]);
    }

    #[test]
    fn gih_first_frame() {
        let mut v = b"name\n2 ncells:2\n".to_vec();
        v.extend(gbr_bytes(1, 1, 1, &[128]));
        v.extend(gbr_bytes(1, 1, 1, &[7]));
        let img = decode_gih_first(&v).unwrap();
        assert_eq!(img.rgba[3], 128, "first frame, not the second");
    }

    #[test]
    fn pat_gray_stays_image() {
        let name = b"p\0";
        let hdr = 24 + name.len() as u32;
        let mut v = Vec::new();
        v.extend_from_slice(&hdr.to_be_bytes());
        v.extend_from_slice(&1u32.to_be_bytes());
        v.extend_from_slice(&1u32.to_be_bytes());
        v.extend_from_slice(&1u32.to_be_bytes());
        v.extend_from_slice(&1u32.to_be_bytes());
        v.extend_from_slice(b"GPAT");
        v.extend_from_slice(name);
        v.push(90);
        let img = decode_pat(&v).unwrap();
        assert_eq!(img.rgba, vec![90, 90, 90, 255]);
    }

    #[test]
    fn malformed_returns_none() {
        assert!(decode_gbr(&[0, 1, 2]).is_none());
        assert!(decode_pat(b"not a pattern at all").is_none());
        assert!(decode_gih_first(b"no newlines").is_none());
    }
}
