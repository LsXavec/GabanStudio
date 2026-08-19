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

/// Extract every .kpp preset from a Krita .bundle (a zip archive).
pub fn parse_bundle(bytes: &[u8]) -> Vec<BrushPreset> {
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
            "kpp" => parse_kpp(&bytes, &stem).into_iter().collect(),
            "bundle" => parse_bundle(&bytes),
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
