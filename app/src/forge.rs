//! THE BRUSH FORGE (research/PSD-brush-forge.md, gate 2026-08-19):
//! making brushes, not just importing them. Lives in the Presets pane
//! under the existing quick list (nothing moves — muscle memory).
//!
//! LAWS (the room's NEVER-DO, enforced here):
//! - One schema, one math: the forge edits the same BrushPreset/
//!   EngineDef every import produces, and the preview calls the SAME
//!   functions the stroke path calls (canvas::rasterize_auto_tip,
//!   canvas::curve_eval, canvas::hash01 — shared, never copied).
//! - The forge edits a DRAFT; the armed brush changes only on "arm",
//!   through the same apply_preset door the rail uses.
//! - Saving over an existing name is a held DANGER naming its victim.
//!   Forge brushes carry bank "my brushes" and engine tag "forge".

use eframe::egui;
use egui::{Color32, Pos2, Sense, pos2, vec2};

use crate::config::{AutoTip, BrushPreset, CurveDef, EngineDef};
use crate::plate;

/// The draft under the hammer, plus pane transients.
pub struct ForgeState {
    pub draft: BrushPreset,
    /// Rebuilt preview texture when the draft changes.
    dirty: bool,
    preview: Option<egui::TextureHandle>,
    /// The tip mask backing the preview (kept so the specimen doesn't
    /// re-rasterize per frame).
    tip_mask: Option<(u32, u32, Vec<u8>)>,
    open: bool,
}

impl Default for ForgeState {
    fn default() -> Self {
        Self {
            draft: blank_draft(),
            dirty: true,
            preview: None,
            tip_mask: None,
            open: false,
        }
    }
}

fn blank_draft() -> BrushPreset {
    BrushPreset {
        name: "new brush".into(),
        bank: "my brushes".into(),
        size_px: 14.0,
        flow: 1.0,
        opacity: 1.0,
        engine: Some(EngineDef {
            engine: "forge".into(),
            spacing: 0.1,
            auto: Some(AutoTip {
                shape: "circle".into(),
                ratio: 1.0,
                hfade: 1.0,
                vfade: 1.0,
                spikes: 2,
                soft: false,
            }),
            curves: vec![CurveDef {
                target: "size".into(),
                sensor: "pressure".into(),
                points: vec![[0.0, 0.0], [1.0, 1.0]],
            }],
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The forge section of the Presets pane.
#[allow(clippy::too_many_arguments)]
pub fn ui(
    ui: &mut egui::Ui,
    forge: &mut ForgeState,
    presets: &mut Vec<BrushPreset>,
    presets_dirty: &mut bool,
    canvas: &mut crate::canvas::CanvasView,
    status: &mut String,
) {
    ui.separator();
    let mut open = forge.open;
    plate::latch(ui, &mut open, "brush forge")
        .on_hover_text("design a brush of your own — same machinery the imports use");
    forge.open = open;
    if !forge.open {
        return;
    }
    ui.add_space(4.0);

    // ---- THE SPECIMEN: a real stroke, drawn by the same math the pen
    // uses, at preview scale (approximation said in the hover).
    if forge.dirty {
        forge.tip_mask = build_tip_mask(&forge.draft);
        let img = specimen(&forge.draft, forge.tip_mask.as_ref(), 300, 64);
        forge.preview = Some(ui.ctx().load_texture(
            "forge_specimen",
            img,
            egui::TextureOptions::LINEAR,
        ));
        forge.dirty = false;
    }
    if let Some(tex) = &forge.preview {
        let (rect, resp) = ui.allocate_exact_size(vec2(300.0, 64.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 0.0, plate::PAPER);
        p.image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        p.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, plate::legend_dim()),
            egui::StrokeKind::Inside,
        );
        resp.on_hover_text(
            "the draft, drawn by the stroke path's own math at preview scale \
             (pressure ramps left to right)",
        );
    }
    ui.add_space(4.0);

    // ---- FOUNDATION row: name, start-from, arm, save.
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut forge.draft.name)
                    .desired_width(120.0)
                    .hint_text("brush name"),
            )
            .changed();
        if ui
            .button("from armed")
            .on_hover_text("load the armed brush into the forge as a draft")
            .clicked()
        {
            let mut base = canvas.snapshot_preset(forge.draft.name.clone());
            base.engine = canvas.armed_engine().cloned().or_else(|| blank_draft().engine);
            base.bank = "my brushes".into();
            if let Some(e) = &mut base.engine {
                e.engine = "forge".into();
            }
            forge.draft = base;
            changed = true;
        }
        if ui.button("blank").on_hover_text("start over").clicked() {
            let name = forge.draft.name.clone();
            forge.draft = blank_draft();
            forge.draft.name = name;
            changed = true;
        }
    });

    // ---- TIP.
    plate::legend(ui, "tip");
    let Some(engine) = forge.draft.engine.as_mut() else {
        forge.draft.engine = blank_draft().engine;
        return;
    };
    ui.horizontal(|ui| {
        let is_auto = engine.auto.is_some();
        if plate::detent(ui, is_auto, "shaped").clicked() && !is_auto {
            engine.tip_key = None;
            engine.auto = blank_draft().engine.unwrap().auto;
            changed = true;
        }
        if plate::detent(ui, !is_auto && engine.tip_key.is_some(), "stamp").clicked() {
            // A stamp needs an image: any png/gbr/gih becomes the tip.
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("tip image", &["png", "gbr", "gih"])
                .pick_file()
            {
                let key = crate::kpp::thumb_key(
                    &path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                );
                let ok = std::fs::read(&path)
                    .ok()
                    .and_then(|bytes| {
                        let ext = path
                            .extension()
                            .map(|e| e.to_ascii_lowercase().to_string_lossy().to_string())
                            .unwrap_or_default();
                        if ext == "png" {
                            crate::canvas::load_rgba_png_bytes(&bytes)
                        } else {
                            crate::kritares::decode_by_ext(&ext, &bytes)
                                .map(|i| (i.w, i.h, i.rgba))
                        }
                    })
                    .map(|(w, h, rgba)| crate::kpp::cache_rgba_as_tip(&key, w, h, &rgba))
                    .unwrap_or(false);
                if ok {
                    engine.auto = None;
                    engine.tip_key = Some(key);
                    changed = true;
                }
            }
        }
    });
    if let Some(a) = engine.auto.as_mut() {
        ui.horizontal(|ui| {
            let circle = a.shape != "rect";
            if plate::detent(ui, circle, "circle").clicked() {
                a.shape = "circle".into();
                changed = true;
            }
            if plate::detent(ui, !circle, "rect").clicked() {
                a.shape = "rect".into();
                changed = true;
            }
            let mut soft = a.soft;
            changed |= plate::latch(ui, &mut soft, "soft").changed();
            a.soft = soft;
        });
        changed |= rail_row(ui, "ratio", &mut a.ratio, 0.05, 1.0);
        changed |= rail_row(ui, "fade", &mut a.hfade, 0.0, 1.0);
        let mut spikes = a.spikes as f32;
        if rail_row(ui, "spikes", &mut spikes, 2.0, 12.0) {
            a.spikes = spikes.round() as u32;
            a.vfade = a.hfade;
            changed = true;
        }
    }

    // ---- MOTION.
    plate::legend(ui, "motion");
    changed |= rail_row(ui, "size px", &mut forge.draft.size_px, 1.0, 300.0);
    changed |= rail_row(ui, "spacing", &mut engine.spacing, 0.02, 2.0);
    changed |= rail_row(ui, "rotation jitter", &mut engine.randomness, 0.0, 1.0);
    changed |= rail_row(ui, "scatter", &mut engine.scatter, 0.0, 3.0);

    // ---- DYNAMICS: pressure curves for size and opacity.
    plate::legend(ui, "pressure");
    changed |= curve_row(ui, "→ size", &mut engine.curves, "size");
    changed |= curve_row(ui, "→ opacity", &mut engine.curves, "opacity");
    changed |= rail_row(ui, "flow", &mut forge.draft.flow, 0.05, 1.0);
    changed |= rail_row(ui, "opacity", &mut forge.draft.opacity, 0.05, 1.0);

    // ---- GRAIN: any cached grain, or none.
    plate::legend(ui, "grain");
    ui.horizontal(|ui| {
        let none = engine.grain_key.is_none();
        if plate::detent(ui, none, "none").clicked() {
            engine.grain_key = None;
            engine.grain_strength = 0.0;
            changed = true;
        }
        if ui
            .button("pick…")
            .on_hover_text("a paper grain from the cache (imported patterns live there)")
            .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .set_directory(crate::kpp::grains_dir().unwrap_or_default())
                .add_filter("grain", &["png"])
                .pick_file()
        {
            engine.grain_key = path
                .file_stem()
                .map(|s| s.to_string_lossy().trim_end_matches(".png").to_string());
            if engine.grain_strength == 0.0 {
                engine.grain_strength = 0.5;
            }
            changed = true;
        }
    });
    if engine.grain_key.is_some() {
        changed |= rail_row(ui, "strength", &mut engine.grain_strength, 0.0, 1.0);
        changed |= rail_row(ui, "scale", &mut engine.grain_scale, 0.1, 4.0);
    }

    // ---- THE DOORS: arm (the rail's own path) and save.
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui
            .button("arm draft")
            .on_hover_text("paint with the draft now — nothing is saved yet")
            .clicked()
        {
            canvas.apply_preset(&forge.draft);
            *status = format!("armed the draft '{}'", forge.draft.name);
        }
        let name = forge.draft.name.trim().to_string();
        let exists = presets.iter().any(|p| p.name == name);
        if !exists {
            if ui.button("save to library").clicked() && !name.is_empty() {
                let mut p = forge.draft.clone();
                p.name = name.clone();
                presets.push(p);
                *presets_dirty = true;
                *status = format!("forged '{}'", forge.draft.name);
            }
        } else if plate::danger(ui, "OVERWRITE") {
            // Saving over a name destroys a tuned brush — held, and the
            // hover names the victim (NEVER-DO 3).
            if let Some(existing) = presets.iter_mut().find(|p| p.name == name) {
                *existing = forge.draft.clone();
                *presets_dirty = true;
                *status = format!("re-forged '{name}'");
            }
        }
        if exists {
            ui.label(
                egui::RichText::new(format!("'{name}' exists"))
                    .size(10.0)
                    .color(plate::legend_dim()),
            );
        }
    });

    if changed {
        forge.dirty = true;
    }
}

/// A labelled plate rail on one row. Returns true while dragging.
fn rail_row(ui: &mut egui::Ui, label: &str, v: &mut f32, lo: f32, hi: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(10.0)
                .color(plate::LEGEND),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{v:.2}"))
                    .monospace()
                    .size(10.5)
                    .color(plate::STRUCK),
            );
            changed = plate::rail(ui, v, lo..=hi, 120.0).dragged();
        });
    });
    changed
}

/// A pressure-curve editor row: a Well the artist drags a control point
/// in (three-point curve: fixed ends' Y + one movable middle).
fn curve_row(
    ui: &mut egui::Ui,
    label: &str,
    curves: &mut Vec<CurveDef>,
    target: &str,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let has = curves.iter().any(|c| c.target == target && c.sensor == "pressure");
        let mut on = has;
        plate::latch(ui, &mut on, label);
        if on != has {
            changed = true;
            if on {
                curves.push(CurveDef {
                    target: target.into(),
                    sensor: "pressure".into(),
                    points: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
                });
            } else {
                curves.retain(|c| !(c.target == target && c.sensor == "pressure"));
            }
        }
        if let Some(c) = curves
            .iter_mut()
            .find(|c| c.target == target && c.sensor == "pressure")
        {
            // The editor: 96×36 Well; drag moves the nearest point.
            let (rect, resp) = ui.allocate_exact_size(vec2(96.0, 36.0), Sense::drag());
            let p = ui.painter();
            p.rect_filled(rect, 0.0, plate::WELL);
            p.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, plate::legend_dim()),
                egui::StrokeKind::Inside,
            );
            if c.points.len() < 2 {
                c.points = vec![[0.0, 0.0], [1.0, 1.0]];
            }
            let to_screen = |pt: [f32; 2]| -> Pos2 {
                pos2(
                    rect.left() + pt[0] * rect.width(),
                    rect.bottom() - pt[1] * rect.height(),
                )
            };
            // The polyline through the SAME evaluator the stroke uses.
            let mut path = Vec::new();
            for i in 0..=24 {
                let x = i as f32 / 24.0;
                path.push(to_screen([x, crate::canvas::curve_eval(&c.points, x)]));
            }
            p.add(egui::Shape::line(path, egui::Stroke::new(1.5, plate::AO)));
            for pt in c.points.iter() {
                p.circle_filled(to_screen(*pt), 2.5, plate::STRUCK);
            }
            if resp.dragged()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let y = ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0);
                // Nearest point by x owns the drag; ends keep their x.
                let idx = c
                    .points
                    .iter()
                    .enumerate()
                    .min_by(|a, b| {
                        (a.1[0] - x).abs().total_cmp(&(b.1[0] - x).abs())
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let last = c.points.len() - 1;
                if idx != 0 && idx != last {
                    c.points[idx] = [x, y];
                } else {
                    c.points[idx][1] = y;
                }
                changed = true;
            }
        }
    });
    changed
}

/// The draft's tip mask, by the stroke path's own rasterizer / cache.
fn build_tip_mask(draft: &BrushPreset) -> Option<(u32, u32, Vec<u8>)> {
    let e = draft.engine.as_ref()?;
    if let Some(key) = &e.tip_key {
        let dir = crate::kpp::tips_dir()?;
        return crate::canvas::load_cache_png(&dir.join(format!("{key}.png")));
    }
    e.auto
        .as_ref()
        .map(|a| crate::canvas::rasterize_auto_tip(a, 128))
}

/// The specimen stroke: an S-curve with pressure ramping 0→1, walked by
/// the draft's spacing, each dab shaded by the draft's curves and tip —
/// the stroke path's math (curve_eval, hash01), CPU-composited.
fn specimen(
    draft: &BrushPreset,
    tip: Option<&(u32, u32, Vec<u8>)>,
    w: usize,
    h: usize,
) -> egui::ColorImage {
    let mut alpha = vec![0.0f32; w * h];
    let e = draft.engine.as_ref();
    let spacing = e.map(|e| e.spacing).unwrap_or(0.1).max(0.02);
    // Preview scale: the brush fits the strip.
    let base_r = (draft.size_px * 0.5).clamp(1.0, h as f32 * 0.38);
    let path_len = w as f32 - 24.0;
    let mut d = 0.0f32;
    let mut idx = 0u32;
    while d < path_len {
        let t = d / path_len;
        let pr = t; // pressure ramps left→right
        let cx = 12.0 + d;
        let cy = h as f32 * 0.5 + (t * std::f32::consts::TAU).sin() * h as f32 * 0.18;
        let mut r = base_r;
        let mut a_mul = draft.opacity * draft.flow;
        let mut rot = 0.0f32;
        if let Some(e) = e {
            for c in &e.curves {
                let x = match c.sensor.as_str() {
                    "pressure" => pr,
                    "fuzzy" => crate::canvas::hash01(idx, 23),
                    _ => 1.0,
                };
                let f = crate::canvas::curve_eval(&c.points, x);
                match c.target.as_str() {
                    "size" => r *= f.max(0.01),
                    "opacity" | "flow" => a_mul *= f,
                    "rotation" => rot += f * std::f32::consts::TAU,
                    _ => {}
                }
            }
            if e.randomness > 0.0 {
                rot += (crate::canvas::hash01(idx, 7) - 0.5)
                    * std::f32::consts::TAU
                    * e.randomness;
            }
            rot += e.angle_deg.to_radians();
        }
        let (cxs, cys) = if let Some(e) = e.filter(|e| e.scatter > 0.0) {
            (
                cx + (crate::canvas::hash01(idx, 11) - 0.5) * e.scatter * r * 2.0,
                cy + (crate::canvas::hash01(idx, 13) - 0.5) * e.scatter * r * 2.0,
            )
        } else {
            (cx, cy)
        };
        stamp(&mut alpha, w, h, cxs, cys, r.max(0.4), rot, a_mul, tip);
        d += (spacing * 2.0 * r).max(0.75);
        idx += 1;
    }
    // Ink on transparent, over nothing — the pane paints Paper beneath.
    let mut px = Vec::with_capacity(w * h * 4);
    for a in alpha {
        let a8 = (a.clamp(0.0, 1.0) * 255.0) as u8;
        px.extend_from_slice(&[26, 27, 24, a8]);
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &px)
}

/// One CPU dab: tip-mask sampled in the rotated frame, or the soft disc.
#[allow(clippy::too_many_arguments)]
fn stamp(
    alpha: &mut [f32],
    w: usize,
    h: usize,
    cx: f32,
    cy: f32,
    r: f32,
    rot: f32,
    a_mul: f32,
    tip: Option<&(u32, u32, Vec<u8>)>,
) {
    let (sin, cos) = rot.sin_cos();
    let x0 = (cx - r).floor().max(0.0) as usize;
    let x1 = (cx + r).ceil().min(w as f32 - 1.0) as usize;
    let y0 = (cy - r).floor().max(0.0) as usize;
    let y1 = (cy + r).ceil().min(h as f32 - 1.0) as usize;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let a = match tip {
                Some((tw, th, rgba)) => {
                    // Rotate into the dab frame, sample the mask.
                    let u = (dx * cos + dy * sin) / r * 0.5 + 0.5;
                    let v = (-dx * sin + dy * cos) / r * 0.5 + 0.5;
                    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                        0.0
                    } else {
                        let tx = (u * (*tw as f32 - 1.0)) as usize;
                        let ty = (v * (*th as f32 - 1.0)) as usize;
                        rgba[(ty * *tw as usize + tx) * 4 + 3] as f32 / 255.0
                    }
                }
                None => {
                    let rr = (dx * dx + dy * dy) / (r * r);
                    if rr >= 1.0 { 0.0 } else { (1.0 - rr).min(1.0) }
                }
            } * a_mul;
            if a > 0.0 {
                let px = &mut alpha[y * w + x];
                *px = *px + a * (1.0 - *px);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forged_brush_is_a_real_preset_and_never_purged() {
        let d = blank_draft();
        assert_eq!(d.bank, "my brushes");
        assert!(d.engine.is_some(), "one schema: a forge brush IS an EngineDef preset");
        // The unreal purge can never classify the forge's work.
        assert!(!crate::kpp::is_unreal(&d));
        assert!(!crate::kpp::UTILITY_ENGINES.contains(&"forge"));
    }

    #[test]
    fn the_specimen_is_deterministic() {
        let d = blank_draft();
        let tip = build_tip_mask(&d);
        let a = specimen(&d, tip.as_ref(), 120, 40);
        let b = specimen(&d, tip.as_ref(), 120, 40);
        assert_eq!(a.pixels, b.pixels, "same draft, same specimen — always");
        // And it actually drew something.
        assert!(a.pixels.iter().any(|p| p.a() > 0));
    }
}
