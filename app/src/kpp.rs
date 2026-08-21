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
        engine: parse_engine(&xml),
        ..Default::default()
    })
}

/// PSD-brush-engine stage A: everything else the preset XML carries that
/// the dab pipeline can honour. None only when the XML has no brush
/// definition at all (then the plain dab is the honest rendering).
fn parse_engine(xml: &str) -> Option<crate::config::EngineDef> {
    use crate::config::{AutoTip, EngineDef};
    let engine = attr_str(xml, "paintopid").unwrap_or_default();
    let bd = param_str(xml, "brush_definition")?;
    let spacing = attr_f32(&bd, "spacing").unwrap_or(0.1).clamp(0.02, 5.0);
    let angle_deg = attr_f32(&bd, "angle").unwrap_or(0.0).to_degrees();
    let randomness = attr_f32(&bd, "randomness").unwrap_or(0.0).clamp(0.0, 1.0);
    let btype = attr_str(&bd, "type").unwrap_or_default();
    let (auto, tip_key) = if btype == "auto_brush" {
        let mg_ratio = attr_f32(&bd, "ratio").unwrap_or(1.0).clamp(0.05, 1.0);
        (
            Some(AutoTip {
                shape: attr_str(&bd, "type")
                    .filter(|t| t == "rect")
                    .unwrap_or_else(|| mask_shape(&bd)),
                ratio: mg_ratio,
                hfade: attr_f32(&bd, "hfade").unwrap_or(1.0).clamp(0.0, 1.0),
                vfade: attr_f32(&bd, "vfade").unwrap_or(1.0).clamp(0.0, 1.0),
                spikes: attr_f32(&bd, "spikes").unwrap_or(2.0).max(2.0) as u32,
                soft: attr_str(&bd, "id").as_deref() == Some("soft"),
            }),
            None,
        )
    } else {
        // Stamp: the tip file this preset references, as a cache key.
        let key = attr_str(&bd, "filename").map(|f| {
            let base = f.rsplit(['/', '\\']).next().unwrap_or(&f);
            let stem = base
                .trim_end_matches(".png")
                .trim_end_matches(".gbr")
                .trim_end_matches(".gih")
                .trim_end_matches(".svg");
            thumb_key(stem)
        });
        (None, key)
    };
    // Sensor curves, each behind its Krita gate.
    let mut curves = Vec::new();
    for (gate, sensor_param, target) in [
        ("PressureSize", "SizeSensor", "size"),
        ("PressureOpacity", "OpacitySensor", "opacity"),
        ("PressureFlow", "FlowSensor", "flow"),
        ("PressureRotation", "RotationSensor", "rotation"),
    ] {
        if param_bool(xml, gate) != Some(true) {
            continue;
        }
        if let Some(sx) = param_str(xml, sensor_param) {
            parse_sensors(&sx, target, &mut curves);
        }
    }
    // THE SMUDGE GATE: dulling rates, colorsmudge only. Their pressure
    // curves ride the existing machinery under their own targets.
    let (smudge_rate, color_rate) = if engine == "colorsmudge" {
        if param_bool(xml, "PressureSmudgeRate") == Some(true)
            && let Some(sx) = param_str(xml, "SmudgeRateSensor")
        {
            parse_sensors(&sx, "smudge", &mut curves);
        }
        if param_bool(xml, "PressureColorRate") == Some(true)
            && let Some(sx) = param_str(xml, "ColorRateSensor")
        {
            parse_sensors(&sx, "color_rate", &mut curves);
        }
        (
            param_f32(xml, "SmudgeRateValue").unwrap_or(0.5).clamp(0.0, 1.0),
            param_f32(xml, "ColorRateValue").unwrap_or(0.3).clamp(0.0, 1.0),
        )
    } else {
        (0.0, 0.0)
    };
    // Scatter/density: only where the engine's core IS scatter — the
    // paintbrush's own enable flag is not reliably recoverable from the
    // XML (room log), so an inky fineliner never starts spraying.
    let (scatter, density) = if engine == "spraybrush" {
        (
            param_f32(xml, "ScatterValue").unwrap_or(1.0).clamp(0.0, 5.0),
            param_f32(xml, "SprayShape/density")
                .or_else(|| param_f32(xml, "Spray/density"))
                .map(|d| (d / 100.0).clamp(0.05, 1.0))
                .unwrap_or(1.0),
        )
    } else {
        (0.0, 0.0)
    };
    // Paper grain.
    let (grain_key, grain_scale, grain_strength) =
        if param_bool(xml, "Texture/Pattern/Enabled") == Some(true) {
            let file = param_str(xml, "Texture/Pattern/PatternFileName")
                .or_else(|| param_str(xml, "Texture/Pattern/Pattern"))
                .and_then(|f| {
                    let base = f.rsplit(['/', '\\']).next().unwrap_or(&f).to_string();
                    let stem = base
                        .trim_end_matches(".pat")
                        .trim_end_matches(".png")
                        .to_string();
                    (!stem.is_empty()).then(|| thumb_key(&stem))
                });
            (
                file,
                param_f32(xml, "Texture/Pattern/Scale").unwrap_or(1.0).clamp(0.05, 8.0),
                param_f32(xml, "Texture/Strength/StrengthValue")
                    .or_else(|| param_f32(xml, "Texture/Pattern/Strength"))
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0),
            )
        } else {
            (None, 1.0, 0.0)
        };
    Some(EngineDef {
        engine,
        spacing,
        auto,
        tip_key,
        curves,
        scatter,
        density,
        randomness,
        angle_deg,
        smudge_rate,
        color_rate,
        eraser: param_bool(xml, "EraserMode") == Some(true),
        grain_key,
        grain_scale,
        grain_strength,
    })
}

/// MaskGenerator shape attr lives on the INNER element; attr_str finds the
/// first "type=" which is the Brush's own — dig for the generator's.
fn mask_shape(bd: &str) -> String {
    bd.find("<MaskGenerator")
        .and_then(|i| attr_str(&bd[i..], "type"))
        .unwrap_or_else(|| "circle".into())
}

/// Sensor XML: either one `<params id="pressure"><curve>…</curve></params>`
/// or `<params id="sensorslist">` of `<ChildSensor id="…">` entries.
/// Unsupported sensors (speed, time, perspective…) are dropped — absent
/// curve = the parameter simply stays at its base (honest, logged).
fn parse_sensors(sx: &str, target: &str, out: &mut Vec<crate::config::CurveDef>) {
    const SUPPORTED: [&str; 8] = [
        "pressure", "fuzzy", "fade", "distance", "xtilt", "ytilt", "ascension", "declination",
    ];
    let mut push = |id: &str, seg: &str| {
        if !SUPPORTED.contains(&id) {
            return;
        }
        out.push(crate::config::CurveDef {
            target: target.to_string(),
            sensor: id.to_string(),
            points: parse_curve_points(seg),
        });
    };
    if sx.contains("sensorslist") {
        let mut rest = sx;
        while let Some(i) = rest.find("<ChildSensor") {
            let seg_end = rest[i..].find("</ChildSensor>").map(|e| i + e).unwrap_or(rest.len());
            let seg = &rest[i..seg_end];
            if let Some(id) = attr_str(seg, "id") {
                push(&id, seg);
            }
            rest = &rest[seg_end..];
            if rest.len() < 14 {
                break;
            }
        }
    } else if let Some(id) = attr_str(sx, "id") {
        push(&id, sx);
    }
}

/// "0,0;0.5,0.7;1,1;" → points. Missing/short curve = identity (empty).
fn parse_curve_points(seg: &str) -> Vec<[f32; 2]> {
    let Some(i) = seg.find("<curve>") else {
        return Vec::new();
    };
    let Some(j) = seg[i..].find("</curve>") else {
        return Vec::new();
    };
    let body = &seg[i + 7..i + j];
    let mut pts: Vec<[f32; 2]> = body
        .split(';')
        .filter_map(|p| {
            let (x, y) = p.trim().split_once(',')?;
            Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
        })
        .collect();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    if pts.len() < 2 { Vec::new() } else { pts }
}

fn param_bool(xml: &str, key: &str) -> Option<bool> {
    param_str(xml, key).map(|v| v.trim() == "true")
}

/// A param's CDATA/text body, entity-unescaped enough for our fields.
fn param_str(xml: &str, key: &str) -> Option<String> {
    let pat = format!("name=\"{key}\"");
    let i = xml.find(&pat)?;
    let rest = &xml[i..];
    let gt = rest.find('>')?;
    let after = &rest[gt + 1..];
    let end = after.find("</param>")?;
    let body = after[..end]
        .trim()
        .trim_start_matches("<![CDATA[")
        .trim_end_matches("]]>")
        .trim();
    Some(body.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&"))
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
        let lower = name.to_ascii_lowercase();
        let is_kpp = lower.ends_with(".kpp");
        let is_tip = lower.starts_with("brushes/");
        let is_grain = lower.starts_with("patterns/");
        if !is_kpp && !is_tip && !is_grain {
            continue;
        }
        let mut buf = Vec::new();
        if entry.read_to_end(&mut buf).is_err() {
            continue;
        }
        if is_kpp {
            let stem = name
                .rsplit('/')
                .next()
                .unwrap_or(&name)
                .trim_end_matches(".kpp");
            if let Some(p) = parse_kpp(&buf, stem) {
                save_thumb(&p.name, &buf);
                out.push(p);
            }
        } else if is_tip {
            if let Some(dir) = tips_dir() {
                cache_resource(&dir, &name, &buf);
            }
        } else if let Some(dir) = grains_dir() {
            cache_resource(&dir, &name, &buf);
        }
    }
    out
}

/// Engines that are TOOLS wearing a brush costume (clone, move, blur,
/// grid…) — not paint. Imported as painters they are compatibility
/// problem makers (owner's order, 2026-08-19): skipped at import,
/// purged from existing configs by brushbank::purge_unreal.
pub const UTILITY_ENGINES: [&str; 8] = [
    "duplicate",
    "deformbrush",
    "filter",
    "experimentbrush",
    "gridbrush",
    "dynabrush",
    "particlebrush",
    "tangentnormal",
];

/// A preset that is not a real paint brush here: a Krita ERASER preset
/// (EraserMode — our eraser is a tool, not a preset) or a utility
/// engine.
pub fn is_unreal(p: &BrushPreset) -> bool {
    let Some(e) = &p.engine else { return false };
    if e.eraser {
        return true;
    }
    if UTILITY_ENGINES.contains(&e.engine.as_str()) {
        return true;
    }
    // Erasers imported before the eraser flag existed are named for it.
    e.engine == "paintbrush" && p.name.to_lowercase().contains("eraser")
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
            // Owner's order: erasers and tool-costume engines never
            // enter the library.
            if is_unreal(&p) {
                dup += 1;
                continue;
            }
            match presets.iter_mut().find(|e| e.name == p.name) {
                // A preset imported before the engine parser existed
                // carries None — re-importing UPGRADES it in place, so
                // "import installed Krita's brushes" again is the whole
                // migration.
                Some(existing)
                    if p.engine.is_some()
                        && (existing.engine.is_none() || existing.engine != p.engine) =>
                {
                    // The parser learned something new (smudge rates, a
                    // fixed curve): re-import refreshes the machinery.
                    // Forge brushes live in bank "my brushes" and never
                    // collide with import names.
                    existing.engine = p.engine;
                    ok += 1;
                }
                Some(_) => dup += 1,
                None => {
                    presets.push(p);
                    ok += 1;
                }
            }
        }
    }
    (ok, dup, failed)
}

/// Every brush file the INSTALLED Krita carries: its shipped bundles,
/// its loose presets, and the user's own %APPDATA%/krita resources
/// (which is also where community bundles land when installed there).
pub fn installed_krita_paths() -> Vec<std::path::PathBuf> {
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for pf in ["ProgramFiles", "ProgramW6432"] {
        if let Some(base) = std::env::var_os(pf) {
            roots.push(
                std::path::PathBuf::from(&base)
                    .join("Krita (x64)")
                    .join("share")
                    .join("krita"),
            );
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(std::path::PathBuf::from(appdata).join("krita"));
    }
    let mut out = Vec::new();
    for root in roots {
        for sub in ["bundles", "paintoppresets"] {
            let dir = root.join(sub);
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                let ext = p
                    .extension()
                    .map(|x| x.to_ascii_lowercase())
                    .unwrap_or_default();
                if ext == "bundle" || ext == "kpp" {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The tip-mask cache (stage A): stamp images decoded to RGBA PNG.
/// v0.2.5 (the shared pencil box): write a raw RGBA image into a brush
/// cache EXACTLY — no premultiply roundtrip; the bytes that arrive are
/// the bytes that land.
pub fn write_cache_png(
    path: &std::path::Path,
    w: u32,
    h: u32,
    rgba: &[u8],
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn tips_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    let dir = std::path::PathBuf::from(base)
        .join("AnimStudio")
        .join("brush_tips");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The paper-grain cache: patterns decoded to RGBA PNG.
pub fn grains_dir() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    let dir = std::path::PathBuf::from(base)
        .join("AnimStudio")
        .join("brush_grains");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Encode RGBA8 to PNG bytes.
fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}

/// Decode one resource file's bytes (by extension) and write it into the
/// given cache as `<thumb_key(stem)>.png`. PNG passes through decoded so
/// the cache is uniformly RGBA8. Returns true when cached.
fn cache_resource(dir: &std::path::Path, file_name: &str, bytes: &[u8]) -> bool {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) => (s, e.to_ascii_lowercase()),
        None => return false,
    };
    let path = dir.join(format!("{}.png", thumb_key(stem)));
    if path.exists() {
        // A GIH cached before frame cycling existed is single-frame with
        // no sidecar — fall through and rebuild it as an atlas.
        if !(ext == "gih" && !path.with_extension("frames").exists()) {
            return true;
        }
    }
    // GIH: every frame into a vertical atlas + a .frames sidecar, so a
    // pipe brush stamps ALL its shapes (stage F). Other formats: single.
    if ext == "gih" {
        if let Some(frames) = crate::kritares::decode_gih_frames(bytes, 16) {
            let n = frames.len() as u32;
            let (w, h) = (frames[0].w, frames[0].h);
            let mut rgba = Vec::with_capacity((w * h * 4 * n) as usize);
            for f in &frames {
                rgba.extend_from_slice(&f.rgba);
            }
            if let Some(pngb) = encode_png(w, h * n, &rgba) {
                let _ = std::fs::write(&path, pngb);
                let _ = std::fs::write(
                    path.with_extension("frames"),
                    n.to_string(),
                );
                return true;
            }
        }
        return false;
    }
    let img = if ext == "png" {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let Ok(mut reader) = decoder.read_info() else {
            return false;
        };
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let Ok(info) = reader.next_frame(&mut buf) else {
            return false;
        };
        let (w, h) = (info.width, info.height);
        let rgba: Vec<u8> = match info.color_type {
            png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
            png::ColorType::Rgb => buf[..info.buffer_size()]
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::Grayscale => buf[..info.buffer_size()]
                .iter()
                .flat_map(|&g| [255, 255, 255, g])
                .collect(),
            png::ColorType::GrayscaleAlpha => buf[..info.buffer_size()]
                .chunks_exact(2)
                .flat_map(|p| [255, 255, 255, ((p[0] as u16 * p[1] as u16) / 255) as u8])
                .collect(),
            _ => return false,
        };
        crate::kritares::ResImage { w, h, rgba }
    } else {
        match crate::kritares::decode_by_ext(&ext, bytes) {
            Some(i) => i,
            None => return false,
        }
    };
    let Some(pngb) = encode_png(img.w, img.h, &img.rgba) else {
        return false;
    };
    std::fs::write(&path, pngb).is_ok()
}

/// Cache raw RGBA as a tip (brushbank's non-Krita importers).
pub fn cache_rgba_as_tip(key: &str, w: u32, h: u32, rgba: &[u8]) -> bool {
    let Some(dir) = tips_dir() else { return false };
    let Some(pngb) = encode_png(w, h, rgba) else { return false };
    std::fs::write(dir.join(format!("{key}.png")), pngb).is_ok()
}

/// Cache raw RGBA as a grain.
pub fn cache_rgba_as_grain(key: &str, w: u32, h: u32, rgba: &[u8]) -> bool {
    let Some(dir) = grains_dir() else { return false };
    let Some(pngb) = encode_png(w, h, rgba) else { return false };
    std::fs::write(dir.join(format!("{key}.png")), pngb).is_ok()
}

/// Pull tips and grains out of the Krita install's own resource folders
/// (share/krita/brushes, patterns + the user's %APPDATA%/krita) — the
/// files loose .kpp presets reference by name.
pub fn cache_krita_resource_dirs() -> usize {
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    for pf in ["ProgramFiles", "ProgramW6432"] {
        if let Some(base) = std::env::var_os(pf) {
            roots.push(
                std::path::PathBuf::from(&base)
                    .join("Krita (x64)")
                    .join("share")
                    .join("krita"),
            );
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(std::path::PathBuf::from(appdata).join("krita"));
    }
    let (Some(tips), Some(grains)) = (tips_dir(), grains_dir()) else {
        return 0;
    };
    let mut n = 0;
    for root in roots {
        for (sub, dir) in [("brushes", &tips), ("patterns", &grains)] {
            let Ok(entries) = std::fs::read_dir(root.join(sub)) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                let Some(name) = p.file_name().map(|s| s.to_string_lossy().to_string()) else {
                    continue;
                };
                let Ok(bytes) = std::fs::read(&p) else {
                    continue;
                };
                if cache_resource(dir, &name, &bytes) {
                    n += 1;
                }
            }
        }
    }
    n
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Machine-dependent smoke: when a Krita install is present, its
    /// bundles must actually yield presets through the real import path
    /// (parse + thumb cache). Skips silently where Krita is absent.
    #[test]
    fn installed_krita_import_smoke() {
        let paths = installed_krita_paths();
        if paths.is_empty() {
            return;
        }
        let mut presets = Vec::new();
        let (ok, _dup, _failed) = import_files(&paths, &mut presets);
        assert!(ok > 0, "an installed Krita must yield at least one preset");
        // Every imported preset is within our engine's honest ranges.
        for p in &presets {
            assert!((1.0..=300.0).contains(&p.size_px), "{}: size {}", p.name, p.size_px);
            assert!((0.05..=1.0).contains(&p.opacity));
            assert!((0.05..=1.0).contains(&p.flow));
        }
        println!("imported {ok} presets from the installed Krita");
        // Stage A proof: the engines parsed, and the interesting parts
        // actually populated.
        let with_engine = presets.iter().filter(|p| p.engine.is_some()).count();
        let stamps = presets
            .iter()
            .filter(|p| p.engine.as_ref().is_some_and(|e| e.tip_key.is_some()))
            .count();
        let autos = presets
            .iter()
            .filter(|p| p.engine.as_ref().is_some_and(|e| e.auto.is_some()))
            .count();
        let curved = presets
            .iter()
            .filter(|p| p.engine.as_ref().is_some_and(|e| !e.curves.is_empty()))
            .count();
        let grains = presets
            .iter()
            .filter(|p| p.engine.as_ref().is_some_and(|e| e.grain_key.is_some()))
            .count();
        println!(
            "engines {with_engine} · stamps {stamps} · auto {autos} · curved {curved} · grain {grains}"
        );
        assert!(with_engine > 200, "most presets carry a brush definition");
        assert!(stamps > 30 && autos > 100 && curved > 150);
        let cached = cache_krita_resource_dirs();
        println!("resources cached: {cached}");
        for p in presets.iter().filter(|p| {
            p.engine.as_ref().is_some_and(|e| e.tip_key.is_some())
        }) {
            let e = p.engine.as_ref().unwrap();
            let key = e.tip_key.as_ref().unwrap();
            let hit = tips_dir().map(|d| d.join(format!("{key}.png")).exists());
            if hit != Some(true) {
                println!("  no tip file for '{}' (key {key})", p.name);
            }
        }
    }
}

#[cfg(test)]
mod unreal_tests {
    use super::*;
    use crate::config::EngineDef;

    fn preset(name: &str, engine: &str, eraser: bool) -> BrushPreset {
        BrushPreset {
            name: name.into(),
            engine: Some(EngineDef {
                engine: engine.into(),
                eraser,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn erasers_and_utilities_are_unreal_paint_brushes_are_not() {
        assert!(is_unreal(&preset("Airbrush_eraser", "paintbrush", true)));
        assert!(is_unreal(&preset("Clone_tool", "duplicate", false)));
        assert!(is_unreal(&preset("Move_tool", "deformbrush", false)));
        assert!(is_unreal(&preset("FX_blur", "filter", false)));
        // Pre-flag erasers are caught by name.
        assert!(is_unreal(&preset("j)_Eraser_Soft", "paintbrush", false)));
        // Real paint stays.
        assert!(!is_unreal(&preset("Ink-2_Fineliner", "paintbrush", false)));
        assert!(!is_unreal(&preset("Basic_mix", "colorsmudge", false)));
        // Hand-made presets (no engine) are untouchable.
        assert!(!is_unreal(&BrushPreset { name: "eraser-ish".into(), ..Default::default() }));
    }

    #[test]
    fn krita_import_yields_no_unreal_presets() {
        let paths = installed_krita_paths();
        if paths.is_empty() {
            return;
        }
        let mut presets = Vec::new();
        import_files(&paths, &mut presets);
        let bad: Vec<&str> = presets
            .iter()
            .filter(|p| is_unreal(p))
            .map(|p| p.name.as_str())
            .collect();
        assert!(bad.is_empty(), "unreal presets imported: {bad:?}");
        assert!(
            !presets.iter().any(|p| p.name.to_lowercase().contains("eraser")),
            "an eraser preset slipped through"
        );
    }
}
