//! Painted icons — every mark in the chrome is a `Painter` primitive, never a
//! font glyph. Resolution-independent, immune to font coverage forever (the
//! tofu squares in the old toolbar were emoji the installed fonts lacked).
//!
//! Governed by research/PSD-editor-repaint.md; design law
//! DESIGN-SPEC-editor-ui.md §6 defect 1. Icons are drawn in ONE ink passed by
//! the caller (colour meaning stays the caller's law); geometry is normalised
//! to the given rect so the same icon serves 12pt and 22pt targets.

use eframe::egui;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2};

// ---------------------------------------------------------------------------
// The detailed masks — OUR OWN SVG drawings (app/assets/icons/svg), rendered
// by `cargo run -p iconforge` into white-alpha PNGs and embedded here. Drawn
// tinted, so one mask serves every ink; secondary detail is authored at 55%
// alpha and dims automatically. The painted primitives below remain as the
// structural fallback — a missing mask degrades to a drawn shape, never tofu.
// ---------------------------------------------------------------------------

macro_rules! mask_bytes {
    ($name:literal, $size:literal) => {
        include_bytes!(concat!("../assets/icons/png/", $name, "_", $size, ".png")) as &[u8]
    };
}

/// (icon, stable id, 24px mask, 48px mask)
const MASKS: &[(Icon, &str, &[u8], &[u8])] = &[
    (
        Icon::Brush,
        "brush",
        mask_bytes!("brush", 24),
        mask_bytes!("brush", 48),
    ),
    (
        Icon::Eraser,
        "eraser",
        mask_bytes!("eraser", 24),
        mask_bytes!("eraser", 48),
    ),
    (
        Icon::Lock,
        "lock",
        mask_bytes!("lock", 24),
        mask_bytes!("lock", 48),
    ),
    (
        Icon::Select,
        "select",
        mask_bytes!("select", 24),
        mask_bytes!("select", 48),
    ),
    (
        Icon::Lasso,
        "lasso",
        mask_bytes!("lasso", 24),
        mask_bytes!("lasso", 48),
    ),
    (
        Icon::Marquee,
        "marquee",
        mask_bytes!("marquee", 24),
        mask_bytes!("marquee", 48),
    ),
    (
        Icon::Fill,
        "fill",
        mask_bytes!("fill", 24),
        mask_bytes!("fill", 48),
    ),
    (
        Icon::Comp,
        "comp",
        mask_bytes!("comp", 24),
        mask_bytes!("comp", 48),
    ),
    (
        Icon::Eye,
        "eye",
        mask_bytes!("eye", 24),
        mask_bytes!("eye", 48),
    ),
    (
        Icon::EyeOff,
        "eye_off",
        mask_bytes!("eye_off", 24),
        mask_bytes!("eye_off", 48),
    ),
    (Icon::Up, "up", mask_bytes!("up", 24), mask_bytes!("up", 48)),
    (
        Icon::Down,
        "down",
        mask_bytes!("down", 24),
        mask_bytes!("down", 48),
    ),
    (
        Icon::Fit,
        "fit",
        mask_bytes!("fit", 24),
        mask_bytes!("fit", 48),
    ),
    (
        Icon::Plus,
        "plus",
        mask_bytes!("plus", 24),
        mask_bytes!("plus", 48),
    ),
];

/// Decode a white-alpha PNG mask into a texture, cached per-Context. The
/// 24px render serves targets up to 28pt; larger targets get the 48px cut.
fn mask_texture(ctx: &egui::Context, icon: Icon, target: f32) -> Option<egui::TextureHandle> {
    let (_, name, png24, png48) = MASKS.iter().find(|(i, ..)| *i == icon)?;
    let big = target * ctx.pixels_per_point() > 28.0;
    let (bytes, size) = if big {
        (*png48, 48u32)
    } else {
        (*png24, 24u32)
    };
    let key = egui::Id::new(("icon_mask", *name, size));
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return Some(tex);
    }
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba {
        return None;
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [info.width as usize, info.height as usize],
        &buf[..info.buffer_size()],
    );
    let tex = ctx.load_texture(
        format!("icon:{name}:{size}"),
        image,
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(key, tex.clone()));
    Some(tex)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Pen nib on a diagonal — draw ink.
    Brush,
    /// Slanted eraser block.
    Eraser,
    /// Padlock — alpha lock.
    Lock,
    /// Marquee corner brackets — select/transform.
    Select,
    /// Open loop with a tail — lasso option.
    Lasso,
    /// Plain rectangle — rectangle option.
    Marquee,
    /// Paint drop — flood fill.
    Fill,
    /// Two overlapping frames — composite view.
    Comp,
    /// Open eye — layer visible.
    Eye,
    /// Struck eye — layer hidden.
    EyeOff,
    /// Chevron up / down — reorder.
    Up,
    Down,
    /// Corner brackets + centre dot — fit view.
    Fit,
    /// Plus — add.
    Plus,
    /// Cel-queue chevrons (shiage): open apex left/right.
    ChevL,
    ChevR,
    /// The hole-hunt's own defect, drawn as itself (miss check).
    Hole,
    /// The line guard's shackle (on the guarded layer's row).
    Shackle,
    /// Transport: |◀ ◀ ▶/❚❚ ▶ ▶| — the Foot's motor controls.
    SkipBack,
    StepBack,
    Play,
    Pause,
    StepFwd,
    SkipFwd,
}

/// Paint `icon` centred in `rect` in one ink. Prefers the detailed forged
/// mask (tinted); falls back to painted primitives so a missing asset can
/// never render as nothing. `rect` should be square-ish.
pub fn paint(p: &Painter, rect: Rect, ink: Color32, icon: Icon) {
    if let Some(tex) = mask_texture(p.ctx(), icon, rect.width()) {
        let side = rect.width().min(rect.height());
        let square = Rect::from_center_size(rect.center(), egui::Vec2::splat(side));
        p.image(
            tex.id(),
            square,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            ink,
        );
        return;
    }
    let m = rect.width().min(rect.height()) * 0.12;
    let r = rect.shrink(m);
    // Normalised helper: (0,0) = top-left of r, (1,1) = bottom-right.
    let at = |x: f32, y: f32| -> Pos2 { pos2(r.left() + x * r.width(), r.top() + y * r.height()) };
    let w = (r.width() * 0.11).clamp(1.0, 2.0);
    let s = Stroke::new(w, ink);
    match icon {
        Icon::Brush => {
            // Shaft.
            p.line_segment([at(0.15, 0.85), at(0.62, 0.38)], s);
            // Nib: a small filled triangle at the tip.
            p.add(Shape::convex_polygon(
                vec![at(0.55, 0.22), at(0.78, 0.45), at(0.92, 0.08)],
                ink,
                Stroke::NONE,
            ));
        }
        Icon::Eraser => {
            // Slanted block, split into the sleeve and the rubber.
            let body = vec![
                at(0.08, 0.72),
                at(0.52, 0.72),
                at(0.92, 0.32),
                at(0.48, 0.32),
            ];
            p.add(Shape::closed_line(body, s));
            p.line_segment([at(0.34, 0.72), at(0.74, 0.32)], s);
        }
        Icon::Lock => {
            // Body.
            let body = Rect::from_min_max(at(0.2, 0.48), at(0.8, 0.92));
            p.rect_stroke(body, 0.0, s, StrokeKind::Middle);
            // Shackle: half circle from the body's shoulders.
            let c = at(0.5, 0.48);
            let rad = (at(0.78, 0.0).x - at(0.22, 0.0).x) * 0.5;
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI + std::f32::consts::PI * (i as f32 / 12.0);
                    pos2(c.x + rad * a.cos(), c.y + rad * 0.9 * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, s));
        }
        Icon::Select => {
            // Four corner brackets (a marquee without the dashes).
            let l = 0.28;
            for (cx, cy, dx, dy) in [
                (0.0, 0.0, 1.0, 1.0),
                (1.0, 0.0, -1.0, 1.0),
                (0.0, 1.0, 1.0, -1.0),
                (1.0, 1.0, -1.0, -1.0),
            ] {
                p.line_segment([at(cx, cy), at(cx + dx * l, cy)], s);
                p.line_segment([at(cx, cy), at(cx, cy + dy * l)], s);
            }
        }
        Icon::Lasso => {
            // Open loop: an ellipse with a gap at the lower-left, plus a tail.
            let c = at(0.5, 0.42);
            let (rx, ry) = (r.width() * 0.38, r.height() * 0.30);
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    // Start past the gap (135°) and sweep ~300°.
                    let a = 2.4 + 5.3 * (i as f32 / 20.0);
                    pos2(c.x + rx * a.cos(), c.y + ry * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, s));
            p.line_segment([at(0.26, 0.66), at(0.14, 0.94)], s);
        }
        Icon::Marquee => {
            p.rect_stroke(
                Rect::from_min_max(at(0.08, 0.2), at(0.92, 0.8)),
                0.0,
                s,
                StrokeKind::Middle,
            );
        }
        Icon::Fill => {
            // A drop: triangle roof into a round belly.
            let c = at(0.5, 0.62);
            let rad = r.width() * 0.3;
            let pts: Vec<Pos2> = (0..=14)
                .map(|i| {
                    let a = std::f32::consts::PI * -0.15
                        + 1.3 * std::f32::consts::PI * (i as f32 / 14.0);
                    pos2(c.x + rad * a.cos(), c.y + rad * a.sin())
                })
                .collect();
            let first = pts[0];
            let last = pts[pts.len() - 1];
            p.add(Shape::line(pts, s));
            p.line_segment([last, at(0.5, 0.06)], s);
            p.line_segment([at(0.5, 0.06), first], s);
        }
        Icon::Comp => {
            // Back frame + front frame (the composite of two).
            p.rect_stroke(
                Rect::from_min_max(at(0.22, 0.08), at(0.92, 0.62)),
                0.0,
                s,
                StrokeKind::Middle,
            );
            let front = Rect::from_min_max(at(0.08, 0.38), at(0.78, 0.92));
            p.rect_filled(front, 0.0, ink.linear_multiply(0.25));
            p.rect_stroke(front, 0.0, s, StrokeKind::Middle);
        }
        Icon::Eye | Icon::EyeOff => {
            // Almond: two mirrored arcs.
            let c = at(0.5, 0.5);
            let rx = r.width() * 0.44;
            let ry = r.height() * 0.55;
            for sign in [-1.0f32, 1.0] {
                let pts: Vec<Pos2> = (0..=12)
                    .map(|i| {
                        let t = i as f32 / 12.0;
                        let x = c.x - rx + 2.0 * rx * t;
                        // Parabolic lid, mirrored for the lower lid.
                        let y = c.y + sign * ry * (0.5 - 2.0 * (t - 0.5) * (t - 0.5)) * 0.9;
                        pos2(x, y)
                    })
                    .collect();
                p.add(Shape::line(pts, s));
            }
            if icon == Icon::Eye {
                p.circle_filled(c, r.width() * 0.13, ink);
            } else {
                p.line_segment([at(0.12, 0.88), at(0.88, 0.12)], s);
            }
        }
        Icon::Up | Icon::Down => {
            let (y0, y1) = if icon == Icon::Up {
                (0.7, 0.3)
            } else {
                (0.3, 0.7)
            };
            p.add(Shape::line(
                vec![at(0.15, y0), at(0.5, y1), at(0.85, y0)],
                s,
            ));
        }
        Icon::Fit => {
            let l = 0.24;
            for (cx, cy, dx, dy) in [
                (0.0, 0.0, 1.0, 1.0),
                (1.0, 0.0, -1.0, 1.0),
                (0.0, 1.0, 1.0, -1.0),
                (1.0, 1.0, -1.0, -1.0),
            ] {
                p.line_segment([at(cx, cy), at(cx + dx * l, cy)], s);
                p.line_segment([at(cx, cy), at(cx, cy + dy * l)], s);
            }
            p.circle_filled(at(0.5, 0.5), r.width() * 0.09, ink);
        }
        Icon::Plus => {
            p.line_segment([at(0.5, 0.12), at(0.5, 0.88)], s);
            p.line_segment([at(0.12, 0.5), at(0.88, 0.5)], s);
        }
        Icon::Play => {
            p.add(Shape::convex_polygon(
                vec![at(0.22, 0.1), at(0.9, 0.5), at(0.22, 0.9)],
                ink,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for x in [0.22f32, 0.58] {
                p.rect_filled(Rect::from_min_max(at(x, 0.12), at(x + 0.2, 0.88)), 0.0, ink);
            }
        }
        Icon::StepBack => {
            p.add(Shape::convex_polygon(
                vec![at(0.8, 0.12), at(0.18, 0.5), at(0.8, 0.88)],
                ink,
                Stroke::NONE,
            ));
        }
        Icon::StepFwd => {
            p.add(Shape::convex_polygon(
                vec![at(0.2, 0.12), at(0.82, 0.5), at(0.2, 0.88)],
                ink,
                Stroke::NONE,
            ));
        }
        Icon::SkipBack => {
            p.rect_filled(Rect::from_min_max(at(0.1, 0.12), at(0.24, 0.88)), 0.0, ink);
            p.add(Shape::convex_polygon(
                vec![at(0.92, 0.12), at(0.32, 0.5), at(0.92, 0.88)],
                ink,
                Stroke::NONE,
            ));
        }
        Icon::ChevL => {
            p.add(Shape::line(
                vec![at(0.68, 0.12), at(0.28, 0.5), at(0.68, 0.88)],
                Stroke::new(w * 1.6, ink),
            ));
        }
        Icon::ChevR => {
            p.add(Shape::line(
                vec![at(0.32, 0.12), at(0.72, 0.5), at(0.32, 0.88)],
                Stroke::new(w * 1.6, ink),
            ));
        }
        Icon::Hole => {
            // A flat of paint with a pit in it — the defect, as itself.
            p.rect_stroke(
                Rect::from_min_max(at(0.08, 0.25), at(0.92, 0.75)),
                0.0,
                s,
                StrokeKind::Middle,
            );
            p.circle_filled(at(0.62, 0.5), r.width() * 0.11, ink);
        }
        Icon::Shackle => {
            p.rect_filled(Rect::from_min_max(at(0.25, 0.55), at(0.75, 0.9)), 0.0, ink);
            let c = at(0.5, 0.55);
            let rad = r.width() * 0.22;
            let pts: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let a = std::f32::consts::PI + std::f32::consts::PI * (i as f32 / 10.0);
                    pos2(c.x + rad * a.cos(), c.y + rad * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, s));
        }
        Icon::SkipFwd => {
            p.rect_filled(Rect::from_min_max(at(0.76, 0.12), at(0.9, 0.88)), 0.0, ink);
            p.add(Shape::convex_polygon(
                vec![at(0.08, 0.12), at(0.68, 0.5), at(0.08, 0.88)],
                ink,
                Stroke::NONE,
            ));
        }
    }
}
