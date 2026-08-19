//! The plate — the app's entire colour, spacing and affordance vocabulary.
//!
//! Governed by research/PSD-editor-repaint.md (gate passed 2026-08-17; root:
//! the repaint never changes what the document contains or how the pen
//! reaches it). Design law: research/DESIGN-SPEC-editor-ui.md §2–§3.
//!
//! The direction in one line: everything the APPLICATION says is PLATE —
//! engraved, quiet, permanent (Legend ink on Graphite); everything the
//! ANIMATOR made or armed is PENCIL — brighter, in the trade's own pencil
//! colours (Struck, Ao, Aka, Tally). No colour is permitted to mean
//! "important", only one specific thing. This module is the only place a
//! UI colour may be defined; a `Color32::from_rgb` literal anywhere else
//! in the app's chrome is a bug with a name.

// Phase 0 declares the full vocabulary; Phases 1–2 consume it. Remove this
// allow when Phase 2 (the Rule) lands — anything still dead then IS dead.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use egui::{
    Color32, CornerRadius, FontFamily, FontId, Pos2, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2,
    text::{LayoutJob, TextFormat},
};

// ---------------------------------------------------------------------------
// Tokens (§2.1). Eight, no more. Names from the printed X-sheet: the plate
// carries engraved legend; the animator strikes it in pencil.
// ---------------------------------------------------------------------------

/// The plate. Every panel, bar, rail, chrome surface. Warm-neutral (olive,
/// not blue-black) so it reads as anodized surround and does not fight Paper.
pub const GRAPHITE: Color32 = Color32::from_rgb(26, 27, 24);
/// The one darker step. Recessed DATA fields only (sheet grid, layer list,
/// library, frame field). There is no third grey step — raised/hover is made
/// with a Legend rule or Tally, never with more grey.
pub const WELL: Color32 = Color32::from_rgb(14, 15, 13);
/// The engraved printing ink. Every word and rule the APP authors.
pub const LEGEND: Color32 = Color32::from_rgb(139, 141, 131);
/// The bright fill of the engraving. Facts the ANIMATOR authored or reads:
/// drawing names, values, the filename.
pub const STRUCK: Color32 = Color32::from_rgb(228, 225, 214);
/// The blue pencil (ao-enpitsu — does not photograph). ONE meaning:
/// continuity and scaffold. Hold rules, the ghost BEHIND, construction.
/// NEVER a selection.
pub const AO: Color32 = Color32::from_rgb(83, 137, 196);
/// The red pencil (the sakkan's correction). ONE meaning: this overrules
/// your hand — refusal, destruction, contradiction. Always text or a 1px
/// edge, never a fill. Never a configuration (the Warning Law, §3).
pub const AKA: Color32 = Color32::from_rgb(228, 82, 47);
/// The lamp. The ONLY colour meaning CURRENT / ARMED. Always a fill, notch
/// or dot — never text.
pub const TALLY: Color32 = Color32::from_rgb(239, 194, 74);
/// The canvas material. Not a UI token; listed so the number lives here.
pub const PAPER: Color32 = Color32::from_rgb(242, 238, 230);

/// Exact UI descriptions (Settings → UI Features): when set, every plate
/// control's hover names its kind + label + source, so the owner can ask
/// for edits by an element's true name. Set once per frame from config.
static EXACT: AtomicBool = AtomicBool::new(false);

pub fn set_exact(on: bool) {
    EXACT.store(on, Ordering::Relaxed);
}

pub fn exact() -> bool {
    EXACT.load(Ordering::Relaxed)
}

/// Append the exact identity line to a response's hover, when enabled.
fn describe(resp: Response, kind: &str, label: &str) -> Response {
    if exact() {
        resp.on_hover_text(format!("[{kind} '{label}' · plate.rs]"))
    } else {
        resp
    }
}

// Derived alphas. No new hues are permitted — these are the eight tokens at
// specified transparencies (spec §2.1), used for disabled states, the time
// rules, and the current row's ground.

/// Disabled controls.
#[inline]
pub fn legend_dim() -> Color32 {
    Color32::from_rgba_unmultiplied(139, 141, 131, 115)
}
/// Beat rule (every 6 frames), 1 device px.
#[inline]
pub fn rule_beat() -> Color32 {
    Color32::from_rgba_unmultiplied(139, 141, 131, 56)
}
/// Half-second rule (every 12 frames), 1 device px.
#[inline]
pub fn rule_half() -> Color32 {
    Color32::from_rgba_unmultiplied(139, 141, 131, 102)
}
/// Second rule (every 24 frames), 2 device px.
#[inline]
pub fn rule_sec() -> Color32 {
    LEGEND
}
/// The current row's ground inside the Well. Replaces the old saturated
/// navy slab entirely: there is one "current" and it is amber.
#[inline]
pub fn tally_well() -> Color32 {
    Color32::from_rgba_unmultiplied(239, 194, 74, 36)
}

// ---------------------------------------------------------------------------
// Structure (§2.3). The spacing scale is 2·4·6·8·12·16·24 — nothing else.
// Fixed dimensions, in logical points. Later phases consume these; they are
// declared here so every panel snaps to the same skeleton.
// ---------------------------------------------------------------------------

pub const ROW_H: f32 = 18.0;
pub const GUTTER_W: f32 = 44.0;
pub const SLATE_H: f32 = 42.0;
pub const FOOT_H: f32 = 34.0;
pub const OPTIONS_H: f32 = 26.0;
pub const RULE_STRIP_W: f32 = 14.0;
pub const LIGHTBOX_W: f32 = 96.0;
/// Every button, toggle, detent.
pub const CTRL_H: f32 = 22.0;

/// The pixel-snapping law. Every rule, notch, dot and 1px edge is snapped;
/// unsnapped hairlines shimmer under scroll at ppp 1.25 and this direction
/// has no ornament to hide behind. Rule thickness is specified in DEVICE
/// pixels and converted with `device_px`.
#[inline]
pub fn snap(v: f32, ppp: f32) -> f32 {
    (v * ppp).round() / ppp
}

/// N device pixels expressed in logical points at the given pixels-per-point.
#[inline]
pub fn device_px(n: f32, ppp: f32) -> f32 {
    n / ppp
}

// ---------------------------------------------------------------------------
// The type plate (§2.2). IBM Plex, four static cuts + a JP subset, installed
// once at boot. Static instances, not variable — determinism at 9.5pt beats
// an axis nobody drags. Tabular figures are structurally unavailable (epaint
// shapes with an empty feature list), so every digit column is Mono by law.
// ---------------------------------------------------------------------------

/// The SemiBold engraving cut (panel titles, legends, the current sheet row).
pub fn semibold() -> FontFamily {
    FontFamily::Name("plex-sans-semibold".into())
}

/// The SemiBold mono cut (the frame counter — the app's only large type).
pub fn mono_semibold() -> FontFamily {
    FontFamily::Name("plex-mono-semibold".into())
}

/// Install the Plex stack and the type scale. Proportional = Plex Sans
/// Regular; Monospace = Plex Mono Medium; two named families carry the
/// SemiBold cuts. The JP subset (kana + the pipeline's kanji, cut from
/// IBMPlexSansJP-Bold) rides second in every family so 原画 falls through
/// per-glyph automatically; egui's default fonts stay at the tail so an
/// unexpected glyph degrades to a drawn shape, never tofu.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (
            "plex-sans",
            &include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf")[..],
        ),
        (
            "plex-sans-sb",
            &include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf")[..],
        ),
        (
            "plex-mono",
            &include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf")[..],
        ),
        (
            "plex-mono-sb",
            &include_bytes!("../assets/fonts/IBMPlexMono-SemiBold.ttf")[..],
        ),
        (
            "plex-jp",
            &include_bytes!("../assets/fonts/IBMPlexSansJP-Bold.subset.otf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), egui::FontData::from_static(bytes).into());
    }

    // Capture egui's default stacks BEFORE prepending, so the named families
    // inherit the same symbol/emoji tail the built-ins keep.
    let default_prop = fonts.families[&FontFamily::Proportional].clone();
    let default_mono = fonts.families[&FontFamily::Monospace].clone();
    let prop = fonts.families.get_mut(&FontFamily::Proportional).unwrap();
    prop.insert(0, "plex-sans".to_owned());
    prop.insert(1, "plex-jp".to_owned());
    let mono = fonts.families.get_mut(&FontFamily::Monospace).unwrap();
    mono.insert(0, "plex-mono".to_owned());
    mono.insert(1, "plex-jp".to_owned());
    let mut sb = vec!["plex-sans-sb".to_owned(), "plex-jp".to_owned()];
    sb.extend(default_prop);
    fonts.families.insert(semibold(), sb);
    let mut msb = vec!["plex-mono-sb".to_owned(), "plex-jp".to_owned()];
    msb.extend(default_mono);
    fonts.families.insert(mono_semibold(), msb);
    ctx.set_fonts(fonts);

    // The type scale (§2.2): body/buttons 11.5, engraved small 9.5, digit
    // columns Mono 11. Heading maps to the SemiBold cut so any stray
    // heading-styled text lands on the engraving face, not a bigger Regular.
    ctx.all_styles_mut(|style| {
        use egui::TextStyle;
        // Debug-build red boxes on unaligned text read as Aka refusals —
        // red that is not Aka is a colour-law violation. Off.
        // egui's debug field exists only in debug builds.
        #[cfg(debug_assertions)]
        {
            style.debug.show_unaligned = false;
        }
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(11.5, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(11.5, FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Small, FontId::new(9.5, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(11.0, FontFamily::Monospace),
        );
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(13.0, semibold()));
        // AUDIT [9]: editable numbers drew in Plex Sans (the Button
        // style). Digits get read in columns and compared — Mono by law.
        style.drag_value_text_style = TextStyle::Monospace;
    });
}

// ---------------------------------------------------------------------------
// Type roles (§2.2) — the two inks as text helpers.
// ---------------------------------------------------------------------------

/// Engraved caps — panel titles, column heads, group legends. UPPERCASE,
/// 9.5pt, +0.9 letterspacing, Legend ink. The plate speaking.
pub fn legend(ui: &mut Ui, text: &str) -> Response {
    let mut job = LayoutJob::default();
    job.append(
        &text.to_uppercase(),
        0.0,
        TextFormat {
            font_id: FontId::new(9.5, semibold()),
            color: LEGEND,
            extra_letter_spacing: 0.9,
            ..Default::default()
        },
    );
    ui.label(job)
}

/// A value the animator authored or is reading — mono, 11pt, Struck ink.
/// The pencil speaking.
pub fn value(ui: &mut Ui, text: &str) -> Response {
    let mut job = LayoutJob::default();
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: FontId::new(11.0, FontFamily::Monospace),
            color: STRUCK,
            ..Default::default()
        },
    );
    ui.label(job)
}

// ---------------------------------------------------------------------------
// Affordances (§6 defect 3). Six controls, three behaviours — so exactly
// three presentations, and nothing else in the app may present a control.
// ---------------------------------------------------------------------------

/// DETENT — a radio position. A Legend ring; a filled Tally disc when this
/// position is armed. Mutually exclusive with its neighbours; clicking an
/// armed detent does nothing (the caller simply sets the same value again).
pub fn detent(ui: &mut Ui, armed: bool, label: &str) -> Response {
    let text_w = text_width(ui, label, 11.5);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(CTRL_H + 4.0 + text_w + 8.0, CTRL_H),
        Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let ppp = ui.ctx().pixels_per_point();
        let c = Pos2::new(
            snap(rect.left() + CTRL_H * 0.5, ppp),
            snap(rect.center().y, ppp),
        );
        p.circle_stroke(c, 5.0, Stroke::new(1.0, LEGEND));
        if armed {
            p.circle_filled(c, 3.0, TALLY);
        }
        let ink = if armed || resp.hovered() {
            STRUCK
        } else {
            LEGEND
        };
        label_at(
            ui,
            rect.left() + CTRL_H + 4.0,
            rect.center().y,
            label,
            11.5,
            ink,
        );
    }
    describe(resp, "DETENT detent", label)
}

/// LATCH — an independent on/off. A Tally bar seats into the control's left
/// edge while latched. Clicking toggles.
pub fn latch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let text_w = text_width(ui, label, 11.5);
    let (rect, mut resp) =
        ui.allocate_exact_size(Vec2::new(6.0 + 6.0 + text_w + 8.0, CTRL_H), Sense::click());
    if resp.clicked() {
        *on = !*on;
        // Callers gate saves on .changed(), as they do for checkboxes.
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let ppp = ui.ctx().pixels_per_point();
        let bar = egui::Rect::from_min_max(
            Pos2::new(snap(rect.left(), ppp), snap(rect.top() + 3.0, ppp)),
            Pos2::new(snap(rect.left() + 3.0, ppp), snap(rect.bottom() - 3.0, ppp)),
        );
        if *on {
            p.rect_filled(bar, CornerRadius::ZERO, TALLY);
        } else {
            p.rect_stroke(
                bar,
                CornerRadius::ZERO,
                Stroke::new(1.0, LEGEND),
                StrokeKind::Inside,
            );
        }
        let ink = if *on || resp.hovered() {
            STRUCK
        } else {
            LEGEND
        };
        label_at(ui, rect.left() + 12.0, rect.center().y, label, 11.5, ink);
    }
    describe(resp, "LATCH latch", label)
}

/// DETENT with a painted icon — a tool position. The armed position carries
/// a Tally bar seated under the control (the lamp lit under a pressed key);
/// icon and label step from Legend to Struck. Larger targets than bare text
/// (owner's amendment: sparse space is spent on bigger controls).
pub fn tool_button(ui: &mut Ui, armed: bool, icon: crate::icons::Icon, label: &str) -> Response {
    let text_w = if label.is_empty() {
        0.0
    } else {
        text_width(ui, label, 11.5) + 5.0
    };
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(8.0 + 15.0 + text_w + 8.0, CTRL_H + 4.0),
        Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let ppp = ui.ctx().pixels_per_point();
        let ink = if armed || resp.hovered() {
            STRUCK
        } else {
            LEGEND
        };
        // Seated on the Well (owner's audit 2026-08-17): every control sits
        // IN the plate like the pencil slots do, never floats on chrome.
        p.rect_filled(rect, CornerRadius::ZERO, WELL);
        p.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(
                1.0,
                if resp.hovered() && !armed {
                    LEGEND
                } else {
                    legend_dim()
                },
            ),
            StrokeKind::Inside,
        );
        let icon_rect = egui::Rect::from_center_size(
            Pos2::new(rect.left() + 8.0 + 7.5, rect.center().y - 1.0),
            Vec2::splat(15.0),
        );
        crate::icons::paint(p, icon_rect, ink, icon);
        if !label.is_empty() {
            label_at(
                ui,
                rect.left() + 8.0 + 15.0 + 5.0,
                rect.center().y - 1.0,
                label,
                11.5,
                ink,
            );
        }
        if armed {
            let bar = egui::Rect::from_min_max(
                Pos2::new(snap(rect.left() + 2.0, ppp), snap(rect.bottom() - 3.0, ppp)),
                Pos2::new(snap(rect.right() - 2.0, ppp), snap(rect.bottom(), ppp)),
            );
            p.rect_filled(bar, CornerRadius::ZERO, TALLY);
        }
    }
    describe(resp, "DETENT tool_button", label)
}

/// LATCH with a painted icon — independent on/off; Tally bar on the LEFT
/// edge while latched (never confusable with the detent's bottom lamp).
pub fn tool_latch(ui: &mut Ui, on: &mut bool, icon: crate::icons::Icon, label: &str) -> Response {
    let text_w = if label.is_empty() {
        0.0
    } else {
        text_width(ui, label, 11.5) + 5.0
    };
    let (rect, mut resp) = ui.allocate_exact_size(
        Vec2::new(6.0 + 15.0 + text_w + 8.0, CTRL_H + 4.0),
        Sense::click(),
    );
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let ppp = ui.ctx().pixels_per_point();
        let ink = if *on || resp.hovered() {
            STRUCK
        } else {
            LEGEND
        };
        // Seated on the Well, same law as tool_button.
        p.rect_filled(rect, CornerRadius::ZERO, WELL);
        p.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(
                1.0,
                if resp.hovered() && !*on {
                    LEGEND
                } else {
                    legend_dim()
                },
            ),
            StrokeKind::Inside,
        );
        if *on {
            let bar = egui::Rect::from_min_max(
                Pos2::new(snap(rect.left(), ppp), snap(rect.top() + 2.0, ppp)),
                Pos2::new(snap(rect.left() + 3.0, ppp), snap(rect.bottom() - 2.0, ppp)),
            );
            p.rect_filled(bar, CornerRadius::ZERO, TALLY);
        }
        let icon_rect = egui::Rect::from_center_size(
            Pos2::new(rect.left() + 6.0 + 7.5, rect.center().y),
            Vec2::splat(15.0),
        );
        crate::icons::paint(p, icon_rect, ink, icon);
        if !label.is_empty() {
            label_at(
                ui,
                rect.left() + 6.0 + 15.0 + 5.0,
                rect.center().y,
                label,
                11.5,
                ink,
            );
        }
    }
    describe(resp, "LATCH tool_latch", label)
}

/// A momentary action with a painted icon (no armed state — fires on click).
pub fn icon_button(ui: &mut Ui, icon: crate::icons::Icon, label: &str, hover: &str) -> Response {
    let text_w = if label.is_empty() {
        0.0
    } else {
        text_width(ui, label, 11.5) + 5.0
    };
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(6.0 + 15.0 + text_w + 6.0, CTRL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let ink = if resp.hovered() { STRUCK } else { LEGEND };
        // Seated on the Well, same law as every control (owner's audit).
        p.rect_filled(rect, CornerRadius::ZERO, WELL);
        p.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, if resp.hovered() { LEGEND } else { legend_dim() }),
            StrokeKind::Inside,
        );
        let icon_rect = egui::Rect::from_center_size(
            Pos2::new(rect.left() + 6.0 + 7.5, rect.center().y),
            Vec2::splat(15.0),
        );
        crate::icons::paint(p, icon_rect, ink, icon);
        if !label.is_empty() {
            label_at(
                ui,
                rect.left() + 6.0 + 15.0 + 5.0,
                rect.center().y,
                label,
                11.5,
                ink,
            );
        }
    }
    let resp = if hover.is_empty() {
        resp
    } else {
        resp.on_hover_text(hover)
    };
    describe(resp, "ACTION icon_button", label)
}

/// A colour swatch seated in a Well recess (§6 defect 5): an 18×18 painted
/// fill with a 1px Legend edge; the armed swatch is ringed in Tally. The hit
/// target is larger than the well (the owner's amendment).
pub fn swatch(ui: &mut Ui, color: Color32, armed: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(CTRL_H + 4.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let well = egui::Rect::from_center_size(rect.center(), Vec2::splat(18.0));
        p.rect_filled(well.expand(2.0), CornerRadius::ZERO, WELL);
        p.rect_filled(well, CornerRadius::ZERO, color);
        p.rect_stroke(
            well,
            CornerRadius::ZERO,
            Stroke::new(1.0, LEGEND),
            StrokeKind::Outside,
        );
        if armed {
            p.rect_stroke(
                well.expand(2.0),
                CornerRadius::ZERO,
                Stroke::new(2.0, TALLY),
                StrokeKind::Outside,
            );
        } else if resp.hovered() {
            p.rect_stroke(
                well.expand(2.0),
                CornerRadius::ZERO,
                Stroke::new(1.0, STRUCK),
                StrokeKind::Outside,
            );
        }
    }
    describe(resp, "SWATCH swatch", "colour well")
}

/// RAIL — a bounded value the hand drags (the plate's fourth and last
/// control kind, added 2026-08-18 under the sweetening room). A Well
/// track with a 1px Legend edge, filled Tally from the left to the
/// handle, so 0 and full are always visible. egui's own slider rail is
/// invisible under our Visuals (its track paints in widgets.inactive
/// .bg_fill = GRAPHITE, with bg_stroke NONE) — this replaces it.
/// Returns true while the value is being changed.
pub fn rail(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    w: f32,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, CTRL_H), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let span = (hi - lo).max(f32::EPSILON);
    if let Some(p) = resp.interact_pointer_pos() {
        let t = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        *value = lo + t * span;
    }
    if ui.is_rect_visible(rect) {
        let ppp = ui.ctx().pixels_per_point();
        let p = ui.painter();
        let track = egui::Rect::from_min_max(
            Pos2::new(snap(rect.left(), ppp), snap(rect.center().y - 4.0, ppp)),
            Pos2::new(snap(rect.right(), ppp), snap(rect.center().y + 4.0, ppp)),
        );
        p.rect_filled(track, CornerRadius::ZERO, WELL);
        let t = ((*value - lo) / span).clamp(0.0, 1.0);
        if t > 0.0 {
            let mut fill = track;
            fill.max.x = snap(track.left() + track.width() * t, ppp);
            p.rect_filled(fill.shrink(1.0), CornerRadius::ZERO, TALLY);
        }
        p.rect_stroke(
            track,
            CornerRadius::ZERO,
            Stroke::new(1.0, if resp.hovered() { LEGEND } else { legend_dim() }),
            StrokeKind::Inside,
        );
        // The handle: a struck bar the eye can find at any fill level.
        let hx = snap(track.left() + track.width() * t, ppp);
        p.rect_filled(
            egui::Rect::from_min_max(
                Pos2::new(hx - 1.5, rect.center().y - 7.0),
                Pos2::new(hx + 1.5, rect.center().y + 7.0),
            ),
            CornerRadius::ZERO,
            STRUCK,
        );
    }
    resp
}

/// FIELD — an editable number. DragValue keeps its drag-and-type
/// behaviour; this only dresses it, because a number the animator sets
/// and reads is a fact they authored (Struck), not the plate's voice.
/// The FACE comes from `drag_value_text_style` set in `install_fonts`.
pub fn field(ui: &mut Ui, dv: egui::DragValue<'_>) -> Response {
    let saved_inactive = ui.visuals().widgets.inactive.fg_stroke.color;
    let saved_hovered = ui.visuals().widgets.hovered.fg_stroke.color;
    ui.visuals_mut().widgets.inactive.fg_stroke.color = STRUCK;
    ui.visuals_mut().widgets.hovered.fg_stroke.color = STRUCK;
    let r = ui.add(dv);
    ui.visuals_mut().widgets.inactive.fg_stroke.color = saved_inactive;
    ui.visuals_mut().widgets.hovered.fg_stroke.color = saved_hovered;
    r
}

/// METER — a read-only fill (export progress, and anything else that
/// reports how far along a machine is). Same material as RAIL: a Well
/// track with a 1px Legend edge and a Tally fill; the count rides
/// BESIDE it in Struck mono, never inside the bar where the fill
/// swallows it. Indeterminate work passes `None`, which sweeps a short
/// Tally band instead of faking a percentage.
pub fn meter(ui: &mut Ui, frac: Option<f32>, w: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 14.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let ppp = ui.ctx().pixels_per_point();
    let p = ui.painter();
    let track = egui::Rect::from_min_max(
        Pos2::new(snap(rect.left(), ppp), snap(rect.center().y - 5.0, ppp)),
        Pos2::new(snap(rect.right(), ppp), snap(rect.center().y + 5.0, ppp)),
    );
    p.rect_filled(track, CornerRadius::ZERO, WELL);
    match frac {
        Some(f) => {
            let t = f.clamp(0.0, 1.0);
            if t > 0.0 {
                let mut fill = track;
                fill.max.x = snap(track.left() + track.width() * t, ppp);
                p.rect_filled(fill.shrink(1.0), CornerRadius::ZERO, TALLY);
            }
        }
        None => {
            // Indeterminate: a band that sweeps, so the window reads as
            // working rather than stalled — without claiming a number.
            let phase = (ui.input(|i| i.time) * 0.6).fract() as f32;
            let bw = track.width() * 0.25;
            let x = track.left() + (track.width() + bw) * phase - bw;
            let band = egui::Rect::from_min_max(
                Pos2::new(x.max(track.left()), track.top() + 1.0),
                Pos2::new((x + bw).min(track.right()), track.bottom() - 1.0),
            );
            if band.width() > 0.0 {
                p.rect_filled(band, CornerRadius::ZERO, TALLY);
            }
            ui.ctx().request_repaint();
        }
    }
    p.rect_stroke(
        track,
        CornerRadius::ZERO,
        Stroke::new(1.0, legend_dim()),
        StrokeKind::Inside,
    );
}

/// How long DANGER must be held before it fires.
const DANGER_HOLD_S: f64 = 0.35;

/// DANGER — a destructive command. Aka 1px edge, and it does not fire on a
/// click: it fires after a ~350ms press-and-hold, during which a Tally ring
/// fills beside the label. Releasing early cancels. Returns true exactly
/// once, at the moment the hold completes.
pub fn danger(ui: &mut Ui, label: &str) -> bool {
    let text_w = text_width(ui, label, 11.5);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(8.0 + text_w + 6.0 + CTRL_H, CTRL_H),
        Sense::click_and_drag(),
    );
    let id = resp.id;
    let fired_id = id.with("fired");
    let now = ui.input(|i| i.time);
    let held = resp.is_pointer_button_down_on();
    let start: Option<f64> = ui.data_mut(|d| d.get_temp(id));
    let already_fired: bool = ui.data_mut(|d| d.get_temp(fired_id).unwrap_or(false));
    let mut fired = false;
    let mut progress = 0.0_f32;
    if held && !already_fired {
        let t0 = match start {
            Some(t) => t,
            None => {
                ui.data_mut(|d| d.insert_temp(id, now));
                now
            }
        };
        progress = (((now - t0) / DANGER_HOLD_S).min(1.0)) as f32;
        if progress >= 1.0 {
            fired = true;
            ui.data_mut(|d| {
                d.remove_temp::<f64>(id);
                d.insert_temp(fired_id, true);
            });
        } else {
            ui.ctx().request_repaint();
        }
    } else if !held {
        ui.data_mut(|d| {
            d.remove_temp::<f64>(id);
            d.remove_temp::<bool>(fired_id);
        });
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, AKA),
            StrokeKind::Inside,
        );
        let ink = if resp.hovered() || held {
            STRUCK
        } else {
            LEGEND
        };
        label_at(ui, rect.left() + 8.0, rect.center().y, label, 11.5, ink);
        // The filling ring, seated in the square right slot.
        let c = Pos2::new(rect.right() - CTRL_H * 0.5, rect.center().y);
        if progress > 0.0 {
            let n = 24usize;
            let pts: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2
                        + std::f32::consts::TAU * progress * (i as f32 / n as f32);
                    Pos2::new(c.x + 6.0 * a.cos(), c.y + 6.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new(2.0, TALLY)));
        } else {
            p.circle_stroke(c, 6.0, Stroke::new(1.0, legend_dim()));
        }
    }
    fired
}

// ---------------------------------------------------------------------------
// The base Visuals — the single site (main.rs) applies this once.
// ---------------------------------------------------------------------------

/// The app's egui Visuals, derived entirely from the tokens above. Square
/// corners everywhere (the direction has no friendly radius); Graphite
/// chrome; Well for recessed data; selection is Tally, never a blue.
pub fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = GRAPHITE;
    v.window_fill = GRAPHITE;
    v.extreme_bg_color = WELL;
    v.faint_bg_color = WELL;
    v.code_bg_color = WELL;

    // AUDIT [4]: selection.stroke.color is what egui paints a selected
    // widget's TEXT with — pointing it at TALLY turned six panels' worth
    // of selected labels amber, which is exactly what the colour law
    // forbids (Tally is a lamp, never text). The selection now reads as
    // a Tally GROUND under a Struck label; the lamp stays a lamp.
    v.selection.bg_fill = tally_well();
    v.selection.stroke = Stroke::new(1.0, STRUCK);
    v.hyperlink_color = STRUCK;
    v.warn_fg_color = AKA;
    v.error_fg_color = AKA;
    v.window_stroke = Stroke::new(1.0, legend_dim());

    // The two-ink test, applied to widget text: at rest the app speaks in
    // Legend; a control under the hand (hovered/active/open) steps to Struck.
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, LEGEND);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, LEGEND);
    v.widgets.hovered.fg_stroke = Stroke::new(1.2, STRUCK);
    v.widgets.active.fg_stroke = Stroke::new(1.2, STRUCK);
    v.widgets.open.fg_stroke = Stroke::new(1.2, STRUCK);

    // No third grey: rest state sits flush on the plate; hover is announced
    // by a Legend rule (edge), press by the Well step — never by more grey.
    v.widgets.noninteractive.bg_fill = GRAPHITE;
    v.widgets.noninteractive.weak_bg_fill = GRAPHITE;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, legend_dim());
    v.widgets.inactive.bg_fill = GRAPHITE;
    v.widgets.inactive.weak_bg_fill = GRAPHITE;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.bg_fill = GRAPHITE;
    v.widgets.hovered.weak_bg_fill = GRAPHITE;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, LEGEND);
    v.widgets.active.bg_fill = WELL;
    v.widgets.active.weak_bg_fill = WELL;
    v.widgets.active.bg_stroke = Stroke::new(1.0, LEGEND);
    v.widgets.open.bg_fill = WELL;
    v.widgets.open.weak_bg_fill = WELL;
    v.widgets.open.bg_stroke = Stroke::new(1.0, LEGEND);

    // Square. Everywhere. The engraved feel comes from type, rules and the
    // Well step — not radius, not bevels, not shadows.
    let square = CornerRadius::ZERO;
    v.widgets.noninteractive.corner_radius = square;
    v.widgets.inactive.corner_radius = square;
    v.widgets.hovered.corner_radius = square;
    v.widgets.active.corner_radius = square;
    v.widgets.open.corner_radius = square;
    v.window_corner_radius = square;
    v.menu_corner_radius = square;
    v.popup_shadow = egui::epaint::Shadow::NONE;
    v.window_shadow = egui::epaint::Shadow::NONE;
    v
}

// ---------------------------------------------------------------------------
// The material (owner's amendment, 2026-08-17): panels sit on a procedurally
// generated Graphite grain — our own seeded texture from `iconforge`, opaque,
// ±4 levels of relief, hue preserved. Material, never a new colour.
// ---------------------------------------------------------------------------

const PLATE_GRAIN_PNG: &[u8] = include_bytes!("../assets/tex/plate_grain.png");
/// The grain tile's edge length in texels (see iconforge).
const GRAIN_TILE: f32 = 256.0;

fn grain_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let key = egui::Id::new("plate_grain");
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return Some(tex);
    }
    let decoder = png::Decoder::new(std::io::Cursor::new(PLATE_GRAIN_PNG));
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
        "plate_grain",
        image,
        egui::TextureOptions {
            wrap_mode: egui::TextureWrapMode::Repeat,
            ..egui::TextureOptions::LINEAR
        },
    );
    ctx.data_mut(|d| d.insert_temp(key, tex.clone()));
    Some(tex)
}

/// Lay the plate's grain down as this pane's ground. Call FIRST in a pane's
/// ui, before any content; everything else paints over it. Tiles in device
/// space (uv from rect size) so the grain never stretches.
pub fn surface(ui: &mut Ui) {
    surface_alpha(ui, 255);
}

/// The grain at partial opacity — the brush rail's ground (owner
/// 2026-08-17): the toolbar is present without covering the paper.
pub fn surface_alpha(ui: &mut Ui, alpha: u8) {
    let rect = ui.max_rect();
    if let Some(tex) = grain_texture(ui.ctx()) {
        let uv = egui::Rect::from_min_max(
            Pos2::new(rect.left() / GRAIN_TILE, rect.top() / GRAIN_TILE),
            Pos2::new(rect.right() / GRAIN_TILE, rect.bottom() / GRAIN_TILE),
        );
        ui.painter()
            .image(tex.id(), rect, uv, Color32::from_white_alpha(alpha));
    }
}

// ---------------------------------------------------------------------------
// Small internals
// ---------------------------------------------------------------------------

fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
    let font = FontId::new(size, FontFamily::Proportional);
    ui.fonts_mut(|f| {
        f.layout_no_wrap(text.to_string(), font, Color32::WHITE)
            .size()
            .x
    })
}

fn label_at(ui: &Ui, x: f32, cy: f32, text: &str, size: f32, ink: Color32) {
    ui.painter().text(
        Pos2::new(x, cy),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::new(size, FontFamily::Proportional),
        ink,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_lands_on_device_pixels() {
        // At ppp 1.25, logical 10.1 is 12.625 device px; the nearest device
        // pixel is 13, which is logical 10.4.
        let s = snap(10.1, 1.25);
        assert!((s - 10.4).abs() < 1e-6, "got {s}");
        // Snapped values are fixed points of snap.
        assert_eq!(snap(s, 1.25), s);
        // At ppp 1.0 snapping is plain rounding.
        assert_eq!(snap(3.4, 1.0), 3.0);
    }

    #[test]
    fn device_px_converts_rule_weights() {
        // A 1-device-px rule at ppp 1.25 is 0.8 logical points, and snapping
        // preserves it: 0.8 * 1.25 = 1.0 exactly.
        let w = device_px(1.0, 1.25);
        assert!((w - 0.8).abs() < 1e-6);
        assert_eq!(w * 1.25, 1.0);
    }

    #[test]
    fn the_palette_is_exactly_the_ratified_one() {
        // The spec's hex values, transcribed independently of the constants.
        assert_eq!(GRAPHITE, Color32::from_rgb(0x1A, 0x1B, 0x18));
        assert_eq!(WELL, Color32::from_rgb(0x0E, 0x0F, 0x0D));
        assert_eq!(LEGEND, Color32::from_rgb(0x8B, 0x8D, 0x83));
        assert_eq!(STRUCK, Color32::from_rgb(0xE4, 0xE1, 0xD6));
        assert_eq!(AO, Color32::from_rgb(0x53, 0x89, 0xC4));
        assert_eq!(AKA, Color32::from_rgb(0xE4, 0x52, 0x2F));
        assert_eq!(TALLY, Color32::from_rgb(0xEF, 0xC2, 0x4A));
        assert_eq!(PAPER, Color32::from_rgb(0xF2, 0xEE, 0xE6));
    }
}
