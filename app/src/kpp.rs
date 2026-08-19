//! Krita brush-preset import: single `.kpp` files and community `.bundle`
//! packs (the downloads from krita.org / krita-artists.org).
//!
//! HONEST SCOPE: a .kpp is a PNG whose "preset" text chunk holds the full
//! Krita paintop XML — brush tip images, texture patterns, sensor curves and
//! all. Our engine is a procedural soft-round brush, so this import maps the
//! parts we can honour today — NAME, SIZE (diameter), OPACITY, FLOW — and
//! leaves the rest for the fuller brush engine later. Imported presets paint
//! with our dab, at the Krita preset's size and strength.

use std::io::Read;

use crate::config::BrushPreset;

/// Parse one .kpp (PNG) into a preset. `fallback_name` = file stem.
pub fn parse_kpp(bytes: &[u8], fallback_name: &str) -> Option<BrushPreset> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let reader = decoder.read_info().ok()?;
    let info = reader.info();
    let mut xml: Option<String> = None;
    for t in &info.uncompressed_latin1_text {
        if t.keyword == "preset" {
            xml = Some(t.text.clone());
        }
    }
    if xml.is_none() {
        for t in &info.compressed_latin1_text {
            if t.keyword == "preset"
                && let Ok(s) = t.get_text()
            {
                xml = Some(s);
            }
        }
    }
    if xml.is_none() {
        for t in &info.utf8_text {
            if t.keyword == "preset"
                && let Ok(s) = t.get_text()
            {
                xml = Some(s);
            }
        }
    }
    let xml = xml?;
    let name = attr_str(&xml, "name").unwrap_or_else(|| fallback_name.to_string());
    // Size: the paintop's brush definition carries a diameter attribute; some
    // paintops use a "Size" param instead.
    let size = attr_f32(&xml, "diameter")
        .or_else(|| param_f32(&xml, "Size"))
        .unwrap_or(14.0)
        .clamp(1.0, 300.0);
    let opacity = param_f32(&xml, "OpacityValue")
        .unwrap_or(1.0)
        .clamp(0.05, 1.0);
    let flow = param_f32(&xml, "FlowValue").unwrap_or(1.0).clamp(0.05, 1.0);
    Some(BrushPreset {
        name,
        size_px: size,
        flow,
        opacity,
        ..Default::default()
    })
}

/// Bundle extraction that also fills the thumbnail cache — each .kpp
/// entry is its own icon.
pub fn parse_bundle_with_thumbs(bytes: &[u8]) -> Vec<BrushPreset> {
    let mut out = Vec::new();
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return out;
    };
    for i in 0..zip.len() {
        let Ok(mut entry) = zip.by_index(i) else {
            continue;
        };
        let name = entry.name().to_string();
        if !name.to_ascii_lowercase().ends_with(".kpp") {
            continue;
        }
        let mut buf = Vec::new();
        if entry.read_to_end(&mut buf).is_err() {
            continue;
        }
        let stem = name
            .rsplit('/')
            .next()
            .unwrap_or(&name)
            .trim_end_matches(".kpp");
        if let Some(p) = parse_kpp(&buf, stem) {
            save_thumb(&p.name, &buf);
            out.push(p);
        }
    }
    out
}

/// Import .kpp/.bundle files into `presets` (skips duplicates by name).
/// Returns (imported, skipped-duplicates, failed-files).
pub fn import_files(
    paths: &[std::path::PathBuf],
    presets: &mut Vec<BrushPreset>,
) -> (usize, usize, usize) {
    let (mut ok, mut dup, mut failed) = (0usize, 0usize, 0usize);
    for path in paths {
        let Ok(bytes) = std::fs::read(path) else {
            failed += 1;
            continue;
        };
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "imported".into());
        let ext = path
            .extension()
            .map(|e| e.to_ascii_lowercase().to_string_lossy().to_string())
            .unwrap_or_default();
        let found: Vec<BrushPreset> = match ext.as_str() {
            "kpp" => {
                let found: Vec<BrushPreset> = parse_kpp(&bytes, &stem).into_iter().collect();
                if let Some(p) = found.first() {
                    save_thumb(&p.name, &bytes);
                }
                found
            }
            "bundle" => parse_bundle_with_thumbs(&bytes),
            _ => Vec::new(),
        };
        if found.is_empty() {
            failed += 1;
            continue;
        }
        for p in found {
            if presets.iter().any(|e| e.name == p.name) {
                dup += 1;
            } else {
                presets.push(p);
                ok += 1;
            }
        }
    }
    (ok, dup, failed)
}

/// The on-disk thumbnail cache (PSD-brush-library NEVER-DO 2: cache,
/// never config). One 64px PNG per preset, keyed by sanitized name.
pub fn thumb_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    let dir = std::path::PathBuf::from(base)
        .join("AnimStudio")
        .join("brush_thumbs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// A preset name as a cache filename: anything shady becomes '_'.
pub fn thumb_key(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Decode the .kpp's own PNG (the preset IS its icon in Krita), downscale
/// to a 64px max side, and write it into the cache. Unsupported pixel
/// formats simply skip — the rail paints a dab fallback, never tofu.
fn save_thumb(name: &str, png_bytes: &[u8]) {
    let Some(dir) = thumb_dir() else { return };
    let path = dir.join(format!("{}.png", thumb_key(name)));
    if path.exists() {
        return;
    }
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let Ok(mut reader) = decoder.read_info() else { return };
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let Ok(info) = reader.next_frame(&mut buf) else { return };
    let (w, h) = (info.width as usize, info.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    // Expand what we understand to RGBA8; skip exotic formats.
    let rgba: Vec<u8> = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => buf[..w * h * 4].to_vec(),
        (png::ColorType::Rgb, png::BitDepth::Eight) => buf[..w * h * 3]
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => buf[..w * h * 2]
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        (png::ColorType::Grayscale, png::BitDepth::Eight) => buf[..w * h]
            .iter()
            .flat_map(|&g| [g, g, g, 255])
            .collect(),
        _ => return,
    };
    // Nearest-neighbour to 64 max side (a brush tip, not a photograph).
    let scale = 64.0 / w.max(h) as f32;
    let (tw, th) = if scale < 1.0 {
        (
            ((w as f32 * scale) as usize).max(1),
            ((h as f32 * scale) as usize).max(1),
        )
    } else {
        (w, h)
    };
    let mut small = vec![0u8; tw * th * 4];
    for y in 0..th {
        for x in 0..tw {
            let sx = x * w / tw;
            let sy = y * h / th;
            let s = (sy * w + sx) * 4;
            let d = (y * tw + x) * 4;
            small[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, tw as u32, th as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let Ok(mut writer) = enc.write_header() else { return };
        if writer.write_image_data(&small).is_err() {
            return;
        }
    }
    let _ = std::fs::write(&path, out);
}

fn attr_str(xml: &str, key: &str) -> Option<String> {
    let pat = format!("{key}=\"");
    let i = xml.find(&pat)?;
    let rest = &xml[i + pat.len()..];
    let end = rest.find('"')?;
    let v = rest[..end].trim();
    (!v.is_empty()).then(|| v.to_string())
}

fn attr_f32(xml: &str, key: &str) -> Option<f32> {
    attr_str(xml, key)?.parse().ok()
}

/// `<param name="KEY" ...>VALUE</param>` — value is the element text.
fn param_f32(xml: &str, key: &str) -> Option<f32> {
    let pat = format!("name=\"{key}\"");
    let i = xml.find(&pat)?;
    let rest = &xml[i..];
    let gt = rest.find('>')?;
    let after = &rest[gt + 1..];
    let end = after.find('<')?;
    // Some params wrap the value in CDATA.
    let text = after[..end].trim().trim_start_matches("<![CDATA[").trim();
    text.parse().ok()
}
