//! BRUSH BANKS (PSD-brush-library amendment 2026-08-19): every imported
//! preset belongs to a BANK (its source file); banks list and remove as
//! one. The multi-format door: Krita .kpp/.bundle (via kpp.rs),
//! Photoshop .abr (sampled brushes, v1/v2 and the 8BIMsamp v6+ blocks,
//! PackBits RLE), Procreate .brush/.brushset (Shape.png → tip,
//! Grain.png → grain — the NSKeyedArchiver params are NOT parsed; sizes
//! default and the hover says so), and bare .gbr/.gih/.png as single
//! stamps. Malformed files are counted and said, never guessed at.

use std::io::Read;

use crate::config::{BrushPreset, EngineDef};

/// One import's outcome, for the status line and the Plugins page.
#[derive(Default)]
pub struct ImportReport {
    pub ok: usize,
    pub dup: usize,
    pub failed: usize,
}

/// Import any supported brush file, tagging everything it yields with a
/// BANK named after the source file. Krita formats route through kpp.rs
/// (same parse, same caches).
pub fn import_any(paths: &[std::path::PathBuf], presets: &mut Vec<BrushPreset>) -> ImportReport {
    let mut r = ImportReport::default();
    for path in paths {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "imported".into());
        let ext = path
            .extension()
            .map(|e| e.to_ascii_lowercase().to_string_lossy().to_string())
            .unwrap_or_default();
        match ext.as_str() {
            "kpp" | "bundle" => {
                let before: std::collections::HashSet<String> =
                    presets.iter().map(|p| p.name.clone()).collect();
                let (ok, dup, failed) =
                    crate::kpp::import_files(std::slice::from_ref(path), presets);
                r.ok += ok;
                r.dup += dup;
                r.failed += failed;
                for p in presets.iter_mut() {
                    if !before.contains(&p.name) && p.bank.is_empty() {
                        p.bank = stem.clone();
                    }
                }
            }
            "abr" => match std::fs::read(path) {
                Ok(bytes) => {
                    let tips = parse_abr(&bytes);
                    if tips.is_empty() {
                        r.failed += 1;
                    } else {
                        for (i, img) in tips.into_iter().enumerate() {
                            let name = format!("{stem} {}", i + 1);
                            add_stamp_preset(presets, &mut r, &name, &stem, img, None);
                        }
                    }
                }
                Err(_) => r.failed += 1,
            },
            "brush" | "brushset" => match std::fs::read(path) {
                Ok(bytes) => {
                    let found = parse_procreate(&bytes, &stem);
                    if found.is_empty() {
                        r.failed += 1;
                    } else {
                        for (name, shape, grain) in found {
                            add_stamp_preset(presets, &mut r, &name, &stem, shape, grain);
                        }
                    }
                }
                Err(_) => r.failed += 1,
            },
            "gbr" | "gih" | "png" => match std::fs::read(path) {
                Ok(bytes) => {
                    let img = match ext.as_str() {
                        "png" => decode_png_luma_alpha(&bytes),
                        e => crate::kritares::decode_by_ext(e, &bytes),
                    };
                    match img {
                        Some(img) => {
                            add_stamp_preset(presets, &mut r, &stem, &stem, img, None)
                        }
                        None => r.failed += 1,
                    }
                }
                Err(_) => r.failed += 1,
            },
            _ => r.failed += 1,
        }
    }
    r
}

/// Cache a tip image and add a stamp preset over it (the import path all
/// non-Krita formats share). Size defaults to the tip's own width,
/// clamped to our honest range.
fn add_stamp_preset(
    presets: &mut Vec<BrushPreset>,
    r: &mut ImportReport,
    name: &str,
    bank: &str,
    tip: crate::kritares::ResImage,
    grain: Option<crate::kritares::ResImage>,
) {
    if presets.iter().any(|p| p.name == name) {
        r.dup += 1;
        return;
    }
    let key = crate::kpp::thumb_key(name);
    if !crate::kpp::cache_rgba_as_tip(&key, tip.w, tip.h, &tip.rgba) {
        r.failed += 1;
        return;
    }
    let grain_key = grain.and_then(|g| {
        let gk = format!("{key}_grain");
        crate::kpp::cache_rgba_as_grain(&gk, g.w, g.h, &g.rgba).then_some(gk)
    });
    let has_grain = grain_key.is_some();
    presets.push(BrushPreset {
        name: name.to_string(),
        bank: bank.to_string(),
        size_px: (tip.w as f32 * 0.5).clamp(8.0, 300.0),
        flow: 1.0,
        opacity: 1.0,
        engine: Some(EngineDef {
            engine: "stamp-import".into(),
            spacing: 0.15,
            tip_key: Some(key),
            grain_key,
            grain_scale: 1.0,
            grain_strength: if has_grain { 0.7 } else { 0.0 },
            ..Default::default()
        }),
        ..Default::default()
    });
    r.ok += 1;
}

// ---------------------------------------------------------------------------
// Photoshop .abr
// ---------------------------------------------------------------------------

fn be_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Every sampled brush mask in an .abr, as white-alpha images.
pub fn parse_abr(b: &[u8]) -> Vec<crate::kritares::ResImage> {
    let Some(version) = be_u16(b, 0) else {
        return Vec::new();
    };
    match version {
        1 | 2 => parse_abr_v12(b, version).unwrap_or_default(),
        6 | 7 | 9 | 10 => parse_abr_v6(b).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// v1/v2: count at 2; per brush: type i16, size i32, sampled payload.
fn parse_abr_v12(b: &[u8], version: u16) -> Option<Vec<crate::kritares::ResImage>> {
    let count = be_u16(b, 2)? as usize;
    let mut out = Vec::new();
    let mut at = 4usize;
    for _ in 0..count.min(512) {
        let btype = be_u16(b, at)?;
        let size = be_u32(b, at + 2)? as usize;
        let body = at + 6;
        let next = body.checked_add(size)?;
        if btype == 2 {
            // sampled: misc u32, spacing u16, [v2: pascal wide name],
            // antialias u8, 4×u16 bounds, 4×u32 bounds, depth u16, raw.
            let mut p = body + 4 + 2;
            if version == 2 {
                let n = be_u16(b, p)? as usize;
                p += 2 + n * 2;
            }
            p += 1; // antialias
            p += 8; // short bounds
            let top = be_u32(b, p)?;
            let left = be_u32(b, p + 4)?;
            let bottom = be_u32(b, p + 8)?;
            let right = be_u32(b, p + 12)?;
            let depth = be_u16(b, p + 16)?;
            p += 18;
            if depth == 8 && right > left && bottom > top {
                let (w, h) = (right - left, bottom - top);
                if w <= 4096 && h <= 4096 {
                    let n = (w * h) as usize;
                    if let Some(gray) = b.get(p..p + n) {
                        out.push(gray_to_tip(w, h, gray));
                    }
                }
            }
        }
        at = next;
        if at >= b.len() {
            break;
        }
    }
    Some(out)
}

/// v6+: "8BIM""samp" section of brush blocks: i32 length (pad 4), pascal
/// id, [subversion 2: +264 bytes], 4×i32 bounds, i16 depth, u8
/// compression, then raw or per-row PackBits.
fn parse_abr_v6(b: &[u8]) -> Option<Vec<crate::kritares::ResImage>> {
    let subversion = be_u16(b, 2)?;
    // Find the samp section.
    let mut samp_at = None;
    let mut i = 4;
    while i + 12 <= b.len() {
        if &b[i..i + 4] == b"8BIM" && &b[i + 4..i + 8] == b"samp" {
            samp_at = Some(i + 8);
            break;
        }
        i += 1;
    }
    let samp_at = samp_at?;
    let samp_len = be_u32(b, samp_at)? as usize;
    let mut at = samp_at + 4;
    let samp_end = at.checked_add(samp_len)?.min(b.len());
    let mut out = Vec::new();
    while at + 4 < samp_end && out.len() < 512 {
        let block_len = be_u32(b, at)? as usize;
        let body = at + 4;
        let next = body + block_len.div_ceil(4) * 4;
        (|| -> Option<()> {
            let idn = *b.get(body)? as usize;
            let mut p = body + 1 + idn;
            if subversion == 1 {
                // v6.1: brush data starts right after the id.
            } else {
                p += 264; // v6.2+: unicode name block
            }
            let top = be_u32(b, p)?;
            let left = be_u32(b, p + 4)?;
            let bottom = be_u32(b, p + 8)?;
            let right = be_u32(b, p + 12)?;
            let depth = be_u16(b, p + 16)?;
            let compress = *b.get(p + 18)?;
            p += 19;
            if depth != 8 || right <= left || bottom <= top {
                return None;
            }
            let (w, h) = (right - left, bottom - top);
            if w > 4096 || h > 4096 {
                return None;
            }
            let n = (w * h) as usize;
            let gray: Vec<u8> = if compress == 0 {
                b.get(p..p + n)?.to_vec()
            } else {
                // Per-row compressed sizes (i16 each), then PackBits rows.
                let rows = h as usize;
                let mut sizes = Vec::with_capacity(rows);
                for r in 0..rows {
                    sizes.push(be_u16(b, p + r * 2)? as usize);
                }
                let mut q = p + rows * 2;
                let mut gray = Vec::with_capacity(n);
                for sz in sizes {
                    let row = b.get(q..q + sz)?;
                    unpackbits(row, &mut gray, w as usize)?;
                    q += sz;
                }
                gray
            };
            if gray.len() >= n {
                out.push(gray_to_tip(w, h, &gray[..n]));
            }
            Some(())
        })();
        if next <= at {
            break;
        }
        at = next;
    }
    Some(out)
}

/// PackBits: n >= 0 → copy n+1 literal bytes; n <= -1 (not -128) → repeat
/// the next byte 1-n times.
fn unpackbits(src: &[u8], dst: &mut Vec<u8>, expect: usize) -> Option<()> {
    let start = dst.len();
    let mut i = 0;
    while i < src.len() && dst.len() - start < expect {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let cnt = n as usize + 1;
            dst.extend_from_slice(src.get(i..i + cnt)?);
            i += cnt;
        } else if n != -128 {
            let v = *src.get(i)?;
            i += 1;
            dst.extend(std::iter::repeat_n(v, (1 - n as isize) as usize));
        }
    }
    Some(())
}

/// Photoshop masks are gray coverage: value = ink.
fn gray_to_tip(w: u32, h: u32, gray: &[u8]) -> crate::kritares::ResImage {
    crate::kritares::ResImage {
        w,
        h,
        rgba: gray.iter().flat_map(|&g| [255, 255, 255, g]).collect(),
    }
}

// ---------------------------------------------------------------------------
// Procreate .brush / .brushset (zip)
// ---------------------------------------------------------------------------

/// Each brush folder's Shape.png (tip) + Grain.png. Names come from the
/// folder (the binary-plist params are not parsed — honest defaults).
fn parse_procreate(
    bytes: &[u8],
    stem: &str,
) -> Vec<(String, crate::kritares::ResImage, Option<crate::kritares::ResImage>)> {
    let mut out = Vec::new();
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return out;
    };
    let mut shapes: std::collections::BTreeMap<String, crate::kritares::ResImage> =
        Default::default();
    let mut grains: std::collections::BTreeMap<String, crate::kritares::ResImage> =
        Default::default();
    for i in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(i) else {
            continue;
        };
        let name = entry.name().to_string();
        let lower = name.to_ascii_lowercase();
        let is_shape = lower.ends_with("shape.png");
        let is_grain = lower.ends_with("grain.png");
        if !is_shape && !is_grain {
            continue;
        }
        let mut buf = Vec::new();
        if entry.read_to_end(&mut buf).is_err() {
            continue;
        }
        let Some(img) = decode_png_luma_alpha(&buf) else {
            continue;
        };
        let folder = name
            .rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default();
        if is_shape {
            shapes.insert(folder, img);
        } else {
            grains.insert(folder, img);
        }
    }
    for (i, (folder, shape)) in shapes.into_iter().enumerate() {
        let name = if folder.is_empty() {
            stem.to_string()
        } else {
            format!("{stem} {}", i + 1)
        };
        let grain = grains.remove(&folder);
        out.push((name, shape, grain));
    }
    out
}

/// A PNG whose ink is its LUMA on black (Procreate shapes/grains): alpha
/// = luma × its own alpha, colour = white.
fn decode_png_luma_alpha(bytes: &[u8]) -> Option<crate::kritares::ResImage> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let to_a = |r: u16, g: u16, bl: u16, a: u16| -> u8 {
        let luma = (r * 299 + g * 587 + bl * 114) / 1000;
        ((luma * a) / 255).min(255) as u8
    };
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()]
            .chunks_exact(4)
            .flat_map(|p| {
                [255, 255, 255, to_a(p[0] as u16, p[1] as u16, p[2] as u16, p[3] as u16)]
            })
            .collect(),
        png::ColorType::Rgb => buf[..info.buffer_size()]
            .chunks_exact(3)
            .flat_map(|p| [255, 255, 255, to_a(p[0] as u16, p[1] as u16, p[2] as u16, 255)])
            .collect(),
        png::ColorType::Grayscale => buf[..info.buffer_size()]
            .iter()
            .flat_map(|&g| [255, 255, 255, g])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf[..info.buffer_size()]
            .chunks_exact(2)
            .flat_map(|p| [255, 255, 255, ((p[0] as u16 * p[1] as u16) / 255) as u8])
            .collect(),
        _ => return None,
    };
    Some(crate::kritares::ResImage { w, h, rgba })
}

// ---------------------------------------------------------------------------
// Dependencies (stage F2): a preset either has its resources cached or it
// is NAMED. Imports are self-contained afterwards — nothing depends on
// the Krita install once the caches are filled.
// ---------------------------------------------------------------------------

/// (missing tip, missing grain) preset names.
pub fn audit_deps(presets: &[BrushPreset]) -> (Vec<String>, Vec<String>) {
    let tips = crate::kpp::tips_dir();
    let grains = crate::kpp::grains_dir();
    let mut no_tip = Vec::new();
    let mut no_grain = Vec::new();
    for p in presets {
        let Some(e) = &p.engine else { continue };
        if let Some(k) = &e.tip_key {
            let hit = tips
                .as_ref()
                .map(|d| d.join(format!("{k}.png")).exists())
                .unwrap_or(false);
            if !hit {
                no_tip.push(p.name.clone());
            }
        }
        if let Some(k) = &e.grain_key {
            let hit = grains
                .as_ref()
                .map(|d| d.join(format!("{k}.png")).exists())
                .unwrap_or(false);
            if !hit {
                no_grain.push(p.name.clone());
            }
        }
    }
    (no_tip, no_grain)
}

/// Every bank present, with its preset count. Bankless presets group
/// under "" (shown as "unsorted").
pub fn banks(presets: &[BrushPreset]) -> Vec<(String, usize)> {
    let mut map: std::collections::BTreeMap<String, usize> = Default::default();
    for p in presets {
        *map.entry(p.bank.clone()).or_default() += 1;
    }
    map.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpackbits_literal_and_run() {
        let mut out = Vec::new();
        // 2 literals [7,8] then a run of 3×9: n=1 → copy 2; n=-2 → 3 reps.
        unpackbits(&[1, 7, 8, 0xFE, 9], &mut out, 5).unwrap();
        assert_eq!(out, vec![7, 8, 9, 9, 9]);
    }

    #[test]
    fn abr_v1_sampled_roundtrip() {
        // One sampled v1 brush, 2×1, depth 8, raw [0, 200].
        let mut v = Vec::new();
        v.extend_from_slice(&1u16.to_be_bytes()); // version
        v.extend_from_slice(&1u16.to_be_bytes()); // count
        v.extend_from_slice(&2u16.to_be_bytes()); // type sampled
        let body_len = 4 + 2 + 1 + 8 + 16 + 2 + 2;
        v.extend_from_slice(&(body_len as u32).to_be_bytes());
        v.extend_from_slice(&0u32.to_be_bytes()); // misc
        v.extend_from_slice(&25u16.to_be_bytes()); // spacing
        v.push(0); // antialias
        v.extend_from_slice(&[0u8; 8]); // short bounds
        v.extend_from_slice(&0u32.to_be_bytes()); // top
        v.extend_from_slice(&0u32.to_be_bytes()); // left
        v.extend_from_slice(&1u32.to_be_bytes()); // bottom
        v.extend_from_slice(&2u32.to_be_bytes()); // right
        v.extend_from_slice(&8u16.to_be_bytes()); // depth
        v.extend_from_slice(&[0, 200]); // pixels
        let tips = parse_abr(&v);
        assert_eq!(tips.len(), 1);
        assert_eq!((tips[0].w, tips[0].h), (2, 1));
        assert_eq!(tips[0].rgba[3], 0);
        assert_eq!(tips[0].rgba[7], 200);
    }

    #[test]
    fn malformed_abr_yields_nothing() {
        assert!(parse_abr(&[0, 42, 9, 9]).is_empty());
        assert!(parse_abr(b"junk").is_empty());
    }
}
