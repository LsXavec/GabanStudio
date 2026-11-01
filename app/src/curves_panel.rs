//! GM14: the Curves pane — the param columns' values over frames, drawn as
//! curves. Keys select (click, Shift+click, a box) and drag (time snaps to
//! whole frames; one drag is one undo step); a selected Bezier key's
//! handles drag too; the context menu sets interpolation. F frames the
//! selection, or everything. Frame numbers ride the time rules, values the
//! left edge, and the header types the selected key's value. Edits go
//! through SetParamKeys only; the verbs (framing, interpolation, handles)
//! are registry actions, run through the one dispatch.

use std::borrow::Cow;
use std::collections::BTreeSet;

use anim_core::ids::ParamId;
use anim_core::xsheet::{HandleMode, Handles, ParamColumn, ParamInterp, ParamKey};
use eframe::egui;
use egui::{Pos2, Rect, Sense, pos2, vec2};

use crate::config::Action;
use crate::doc::AppState;
use crate::plate;

/// How close (points) the pointer must be to take a key or a handle.
const HIT: f32 = 7.0;
/// The column list's width.
const LIST_W: f32 = 112.0;
/// The header row over the plot (the selected key's frame and value).
const HEAD_H: f32 = 24.0;

/// The key verbs: the context menu's label, the registry action, and what
/// it sets (a handle mode makes the key Bezier with that mode).
const KEY_VERBS: [(&str, Action, ParamInterp, Option<HandleMode>); 10] = [
    ("hold", Action::InterpHold, ParamInterp::Hold, None),
    ("linear", Action::InterpLinear, ParamInterp::Linear, None),
    ("ease", Action::InterpEase, ParamInterp::Ease, None),
    ("ease in", Action::InterpEaseIn, ParamInterp::EaseIn, None),
    ("ease out", Action::InterpEaseOut, ParamInterp::EaseOut, None),
    ("bezier", Action::InterpBezier, ParamInterp::Bezier, None),
    ("auto handles", Action::HandlesAuto, ParamInterp::Bezier, Some(HandleMode::Auto)),
    ("smooth handles", Action::HandlesSmooth, ParamInterp::Bezier, Some(HandleMode::Smooth)),
    ("flat handles", Action::HandlesFlat, ParamInterp::Bezier, Some(HandleMode::Flat)),
    ("broken handles", Action::HandlesBroken, ParamInterp::Bezier, Some(HandleMode::Broken)),
];

/// Frame/value ↔ screen. Frame `f0` sits on the plot's left edge and value
/// `v0` on its bottom edge; values grow upward.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveView {
    pub f0: f64,
    pub v0: f64,
    pub px_per_frame: f64,
    pub px_per_value: f64,
}

impl Default for CurveView {
    fn default() -> Self {
        Self {
            f0: -2.0,
            v0: -1.0,
            px_per_frame: 8.0,
            px_per_value: 20.0,
        }
    }
}

impl CurveView {
    pub fn to_screen(&self, plot: Rect, frame: f64, value: f64) -> Pos2 {
        pos2(
            (plot.left() as f64 + (frame - self.f0) * self.px_per_frame) as f32,
            (plot.bottom() as f64 - (value - self.v0) * self.px_per_value) as f32,
        )
    }

    /// The (frame, value) under a screen point.
    pub fn from_screen(&self, plot: Rect, p: Pos2) -> (f64, f64) {
        (
            self.f0 + (p.x as f64 - plot.left() as f64) / self.px_per_frame,
            self.v0 + (plot.bottom() as f64 - p.y as f64) / self.px_per_value,
        )
    }

    /// The view showing frames `f` and values `v` (min, max) with a margin
    /// all round; a single frame or a flat curve still gets an extent.
    pub fn framing(plot: Rect, f: (f64, f64), v: (f64, f64)) -> Self {
        let (fa, fb) = if f.1 - f.0 < 1.0 { (f.0 - 4.0, f.1 + 4.0) } else { f };
        let (va, vb) = if v.1 - v.0 < 1e-3 { (v.0 - 1.0, v.1 + 1.0) } else { v };
        let (mx, my) = ((fb - fa) * 0.08, (vb - va) * 0.12);
        Self {
            f0: fa - mx,
            v0: va - my,
            px_per_frame: (plot.width() as f64).max(1.0) / (fb - fa + 2.0 * mx),
            px_per_value: (plot.height() as f64).max(1.0) / (vb - va + 2.0 * my),
        }
    }

    /// Zoom by `fx` × `fy` (> 1 = in) keeping the point under `at` fixed.
    pub fn zoom_about(&mut self, plot: Rect, at: Pos2, fx: f64, fy: f64) {
        let (f, v) = self.from_screen(plot, at);
        self.px_per_frame = (self.px_per_frame * fx).clamp(0.05, 400.0);
        self.px_per_value = (self.px_per_value * fy).clamp(1e-4, 1e6);
        let (f2, v2) = self.from_screen(plot, at);
        self.f0 += f - f2;
        self.v0 += v - v2;
    }

    /// Move the view with the pointer (a screen-space drag delta).
    pub fn pan(&mut self, delta: egui::Vec2) {
        self.f0 -= delta.x as f64 / self.px_per_frame;
        self.v0 += delta.y as f64 / self.px_per_value;
    }
}

/// What a drag in the plot is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// Moving the selected keys; the press point.
    Keys { from: Pos2 },
    /// One key's handle (`out` = the handle after the key).
    Handle { column: ParamId, frame: u32, out: bool },
    /// A selection box from this corner.
    Box { from: Pos2 },
    /// Moving the view.
    Pan,
}

/// The pane's state — view furniture, never in the file.
#[derive(Default)]
pub struct CurvesState {
    /// Columns whose curves show; empty = every column.
    pub shown: BTreeSet<ParamId>,
    /// Selected keys, as (column, frame).
    pub selected: BTreeSet<(ParamId, u32)>,
    /// The view; None until the pane first frames everything.
    pub view: Option<CurveView>,
    /// The plot's rect last frame (tests aim with it).
    pub plot: Option<Rect>,
    drag: Option<Drag>,
    /// A key drag's live offset (whole frames, value), shown until release.
    moved: Option<(i64, f32)>,
    /// A handle drag's live key, shown until release.
    bent: Option<ParamKey>,
    /// The verbs the pane asked for (its F, its key menu), run by the
    /// editor through the one dispatch after the dock draws.
    pub asked: Vec<Action>,
    /// The header's value field last frame (tests aim with it).
    pub value_field: Option<Rect>,
    /// The header's value buffer for one key, and whether the field is
    /// mid-edit (typed or dragged): a resting field follows the key.
    value: Option<((ParamId, u32), f32)>,
    value_editing: bool,
}

/// One ink per shown column, from the plate's inks that carry no other
/// meaning here — Tally is the current frame, Aka a refusal, and a selected
/// key wears a ring, not a colour.
fn ink(i: usize) -> egui::Color32 {
    [
        plate::STRUCK,
        plate::PENCIL_AO,
        plate::INK3,
        plate::PENCIL_AO.gamma_multiply(0.6),
        plate::STRUCK.gamma_multiply(0.6),
    ][i % 5]
}

/// The columns drawn: the chosen ones, else all of them.
fn shown_columns(state: &AppState) -> Vec<ParamColumn> {
    let s = &state.curves.shown;
    state
        .cut()
        .xsheet
        .params
        .iter()
        .filter(|p| s.is_empty() || s.contains(&p.id))
        .cloned()
        .collect()
}

/// A key's effective handle points in (frame, value): (in, out).
fn handle_points(col: &ParamColumn, frame: u32) -> Option<((f64, f64), (f64, f64))> {
    let k = col.key_at(frame)?;
    let h = col.handles_at(frame)?;
    let (f, v) = (frame as f64, k.value as f64);
    Some((
        (f - h.in_dt as f64, v - h.in_dv as f64),
        (f + h.out_dt as f64, v + h.out_dv as f64),
    ))
}

/// The frames and values the view should hold: the selected keys, or
/// (with none) every shown curve — its keys and its values on every frame
/// between them, so a Bezier's bulge stays in view.
fn extent(cols: &[ParamColumn], selected: &BTreeSet<(ParamId, u32)>) -> Option<((f64, f64), (f64, f64))> {
    let mut f = (f64::INFINITY, f64::NEG_INFINITY);
    let mut v = (f64::INFINITY, f64::NEG_INFINITY);
    let mut take = |frame: f64, value: f64| {
        f = (f.0.min(frame), f.1.max(frame));
        v = (v.0.min(value), v.1.max(value));
    };
    for col in cols {
        let keys: Vec<(u32, ParamKey)> = col.keys().collect();
        for (frame, k) in &keys {
            if selected.is_empty() || selected.contains(&(col.id, *frame)) {
                take(*frame as f64, k.value as f64);
            }
        }
        if selected.is_empty()
            && let (Some(first), Some(last)) = (keys.first(), keys.last())
        {
            for frame in first.0..=last.0 {
                if let Some(value) = col.sample(frame as f64) {
                    take(frame as f64, value);
                }
            }
        }
    }
    f.0.is_finite().then_some((f, v))
}

/// Frame the selection, or everything when nothing is selected.
pub fn frame_view(state: &mut AppState, plot: Rect) {
    let cols = shown_columns(state);
    if let Some((f, v)) = extent(&cols, &state.curves.selected) {
        state.curves.view = Some(CurveView::framing(plot, f, v));
    }
}

/// The Frame curves verb: frame in the plot as last drawn.
pub fn frame_stored(state: &mut AppState) {
    if let Some(plot) = state.curves.plot {
        frame_view(state, plot);
    }
}

/// A key drag's frame offset as it will land: the whole selection stops
/// together at frame 0 (`sheet_ops::move_param_keys` clamps the same way).
fn clamp_frames(sel: &BTreeSet<(ParamId, u32)>, frames: i64) -> i64 {
    let lowest = sel.iter().map(|&(_, f)| f as i64).min().unwrap_or(0);
    frames.max(-lowest)
}

/// `col` with the selected keys moved by (frames, dv) — the drag's
/// preview, by the same law as `sheet_ops::move_param_keys` (`frames`
/// already clamped over the whole selection).
fn moved_column(col: &ParamColumn, sel: &BTreeSet<(ParamId, u32)>, frames: i64, dv: f32) -> ParamColumn {
    let mut out = col.clone();
    let moving: Vec<(u32, ParamKey)> = col
        .keys()
        .filter(|(f, _)| sel.contains(&(col.id, *f)))
        .collect();
    for (f, _) in &moving {
        out.clear_key(*f);
    }
    for (f, k) in moving {
        let to = (f as i64 + frames).clamp(0, u32::MAX as i64) as u32;
        out.set_key(
            to,
            ParamKey {
                value: k.value + dv,
                ..k
            },
        );
    }
    out
}

/// The key under `p`, nearest first.
fn key_at(cols: &[ParamColumn], view: &CurveView, plot: Rect, p: Pos2) -> Option<(ParamId, u32)> {
    let mut best: Option<((ParamId, u32), f32)> = None;
    for col in cols {
        for (f, k) in col.keys() {
            let d = view.to_screen(plot, f as f64, k.value as f64).distance(p);
            if d <= HIT && best.is_none_or(|(_, bd)| d < bd) {
                best = Some(((col.id, f), d));
            }
        }
    }
    best.map(|(k, _)| k)
}

/// Which of a Bezier key's handles shape a curve, (in, out): the out
/// handle when a key follows; the in handle when the key before is Bezier
/// too (its segment is the one that reads it).
fn live_handles(col: &ParamColumn, frame: u32) -> (bool, bool) {
    let before = col.keys().filter(|(f, _)| *f < frame).last();
    let after = col.keys().any(|(f, _)| f > frame);
    (before.is_some_and(|(_, k)| k.interp == ParamInterp::Bezier), after)
}

/// The handle of a selected Bezier key under `p`.
fn handle_at(
    cols: &[ParamColumn],
    sel: &BTreeSet<(ParamId, u32)>,
    view: &CurveView,
    plot: Rect,
    p: Pos2,
) -> Option<(ParamId, u32, bool)> {
    for col in cols {
        for (f, k) in col.keys() {
            if k.interp != ParamInterp::Bezier || !sel.contains(&(col.id, f)) {
                continue;
            }
            let Some((hin, hout)) = handle_points(col, f) else { continue };
            let (live_in, live_out) = live_handles(col, f);
            let at = view.to_screen(plot, f as f64, k.value as f64);
            for (pt, out) in [(hout, true), (hin, false)] {
                if !(if out { live_out } else { live_in }) {
                    continue;
                }
                let hs = view.to_screen(plot, pt.0, pt.1);
                // A handle sitting on its key (an end key's empty side)
                // never steals the key's drag.
                if hs.distance(p) <= HIT && hs.distance(at) > HIT {
                    return Some((col.id, f, out));
                }
            }
        }
    }
    None
}

/// The key a handle drag to (frame, value) makes: an auto or flat key
/// turns smooth; a smooth key's other handle keeps its length and turns
/// to stay in line; a broken key's other handle stays put.
fn bend(col: &ParamColumn, frame: u32, out: bool, at: (f64, f64)) -> Option<ParamKey> {
    let k = col.key_at(frame)?;
    let mut h = col.handles_at(frame)?;
    let (dt, dv) = if out {
        ((at.0 - frame as f64).max(0.0), at.1 - k.value as f64)
    } else {
        ((frame as f64 - at.0).max(0.0), k.value as f64 - at.1)
    };
    let (dt, dv) = (dt as f32, dv as f32);
    h.mode = match h.mode {
        HandleMode::Auto | HandleMode::Flat => HandleMode::Smooth,
        m => m,
    };
    if out {
        (h.out_dt, h.out_dv) = (dt, dv);
    } else {
        (h.in_dt, h.in_dv) = (dt, dv);
    }
    if h.mode == HandleMode::Smooth && dt > 0.0 {
        let slope = dv / dt;
        if out {
            h.in_dv = slope * h.in_dt;
        } else {
            h.out_dv = slope * h.out_dt;
        }
    }
    h.is_finite().then_some(ParamKey {
        handles: Some(h),
        interp: ParamInterp::Bezier,
        ..k
    })
}

/// The selected keys with `interp` (and, for Bezier, the handle `mode`;
/// None keeps each key's handles) — one undo step from the context menu.
fn set_interp(state: &mut AppState, interp: ParamInterp, mode: Option<HandleMode>, label: &str) {
    let mut edits = Vec::new();
    for (pid, frame) in state.curves.selected.clone() {
        let Some(col) = state.cut().xsheet.param(pid) else { continue };
        let Some(k) = col.key_at(frame) else { continue };
        let handles = match (interp, mode) {
            (ParamInterp::Bezier, Some(HandleMode::Auto)) => None,
            (ParamInterp::Bezier, Some(m)) => col.handles_at(frame).map(|h| Handles { mode: m, ..h }),
            (ParamInterp::Bezier, None) => k.handles,
            _ => None,
        };
        edits.push((pid, frame, ParamKey { interp, handles, ..k }));
    }
    state.set_param_keys(label, edits);
}

/// A key verb (the context menu, Sheet ▸ Key interpolation / Key handles,
/// the palette) on the selected keys — one undo step.
pub fn key_verb(state: &mut AppState, action: Action) {
    let Some(&(label, _, interp, mode)) = KEY_VERBS.iter().find(|v| v.1 == action) else {
        return;
    };
    let label = match mode {
        Some(_) => label.to_string(),
        None => format!("interpolation: {label}"),
    };
    set_interp(state, interp, mode, &label);
}

/// The step between time rules, in frames: the first rung with room for
/// its label; from a second up the rules fall on whole seconds.
fn time_step(px_per_frame: f64, fps: u32) -> u32 {
    let fps = fps.max(1);
    let rungs = [1, 2, 4, 6, 12]
        .into_iter()
        .filter(|s| *s < fps)
        .chain([1, 2, 5, 10, 30, 60].into_iter().map(|s| s * fps));
    let mut last = fps;
    for s in rungs {
        last = s;
        if s as f64 * px_per_frame >= 44.0 {
            break;
        }
    }
    last
}

/// A rule's frame number as the sheet says it: 1-based, or seconds+frames
/// once the rules fall on seconds.
fn time_label(frame: u32, step: u32, fps: u32) -> String {
    if step >= fps.max(1) {
        crate::xsheet_panel::seconds_plus_frames(frame, fps)
    } else {
        (frame + 1).to_string()
    }
}

/// The step between value ticks: 1, 2 or 5 × a power of ten, about 40
/// points apart.
fn value_step(px_per_value: f64) -> f64 {
    let raw = 40.0 / px_per_value.max(1e-9);
    let mag = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * mag)
}

/// A value tick's label, with the decimals its step needs.
fn value_label(v: f64, step: f64) -> String {
    let decimals = (-step.log10().floor()).max(0.0) as usize;
    let v = if v.abs() < step * 1e-6 { 0.0 } else { v };
    format!("{v:.decimals$}")
}

/// GM14: the header over the plot — the selected key's column and frame
/// (1-based, as the sheet counts), its value typed or dragged (one undo
/// step when the field lets go) and its interpolation.
fn key_header(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool) {
    let one = match state.curves.selected.len() {
        1 => state.curves.selected.iter().next().copied(),
        _ => None,
    };
    let picked = one.and_then(|(pid, frame)| {
        let col = state.cut().xsheet.param(pid)?;
        Some((pid, frame, col.name.clone(), col.key_at(frame)?))
    });
    let Some((pid, frame, name, key)) = picked else {
        state.curves.value_field = None;
        state.curves.value = None;
        let n = state.curves.selected.len();
        let text = if n == 0 {
            "click a key to read and type its value".to_string()
        } else {
            format!("{n} keys selected — the right-click menu sets their interpolation")
        };
        ui.label(egui::RichText::new(text).color(plate::legend_dim()));
        return;
    };
    ui.label(egui::RichText::new(format!("{name} @ {}", frame + 1)).color(plate::LEGEND));
    // A resting field follows the key (an undo, a drag in the plot).
    if !state.curves.value_editing || state.curves.value.map(|(k, _)| k) != Some((pid, frame)) {
        state.curves.value = Some(((pid, frame), key.value));
    }
    let mut v = state.curves.value.map_or(key.value, |(_, v)| v);
    let resp = plate::field_if(
        ui,
        gesture_safe,
        egui::DragValue::new(&mut v).speed(0.05).update_while_editing(false),
    )
    .on_hover_text("this key's value: drag it, or click and type — one undo step when you let go");
    state.curves.value_field = Some(resp.rect);
    state.curves.value = Some(((pid, frame), v));
    state.curves.value_editing = resp.has_focus() || resp.dragged();
    if (resp.drag_stopped() || (resp.changed() && !resp.dragged())) && v != key.value {
        state.set_param_keys("key value", vec![(pid, frame, ParamKey { value: v, ..key })]);
    }
    let word = KEY_VERBS
        .iter()
        .find(|(_, _, i, m)| *i == key.interp && m.is_none())
        .map_or("", |v| v.0);
    ui.label(egui::RichText::new(word).color(plate::legend_dim()));
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool) {
    // Selections of keys that no longer exist (undo, the sheet) or whose
    // curve is no longer shown drop out: a verb acts on what is seen.
    let shown = &state.curves.shown;
    let live: BTreeSet<(ParamId, u32)> = state
        .curves
        .selected
        .iter()
        .copied()
        .filter(|(p, f)| {
            (shown.is_empty() || shown.contains(p))
                && state.cut().xsheet.param(*p).is_some_and(|c| c.key_at(*f).is_some())
        })
        .collect();
    state.curves.selected = live;
    let params: Vec<(ParamId, String)> = state
        .cut()
        .xsheet
        .params
        .iter()
        .map(|p| (p.id, p.name.clone()))
        .collect();
    if params.is_empty() {
        plate::legend(ui, "curves");
        ui.label(egui::RichText::new("Animate a value to see its curve here").color(plate::legend_dim()));
        return;
    }
    let full = ui.available_rect_before_wrap();
    let list = Rect::from_min_size(full.min, vec2(LIST_W, full.height()));
    let head = Rect::from_min_max(
        pos2(full.left() + LIST_W + 4.0, full.top()),
        pos2(full.right(), full.top() + HEAD_H),
    );
    let plot = Rect::from_min_max(pos2(head.left(), head.bottom()), full.max);

    // The column list: pick which curves show (none picked = all).
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        plate::legend(ui, "columns");
        for (i, (id, name)) in params.iter().enumerate() {
            let on = state.curves.shown.contains(id);
            let text = egui::RichText::new(name).color(ink(i));
            if ui
                .selectable_label(on, text)
                .on_hover_text("show this column's curve (none picked = every column)")
                .clicked()
            {
                if on {
                    state.curves.shown.remove(id);
                } else {
                    state.curves.shown.insert(*id);
                }
            }
        }
    });

    // The header: the selected key's frame and value.
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(head)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| key_header(ui, state, gesture_safe),
    );

    state.curves.plot = Some(plot);
    let cols = shown_columns(state);
    if state.curves.view.is_none() {
        frame_view(state, plot);
    }
    // F frames the selection (or everything) while the pointer is over
    // the pane, as in every graph editor — taken here, so the sheet's F
    // (next frame) stays put, and run as the Frame curves verb.
    let pointer_here = ui.rect_contains_pointer(full);
    if pointer_here
        && !ui.ctx().text_edit_focused()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F))
    {
        state.curves.asked.push(Action::FrameCurves);
    }
    let mut view = state.curves.view.unwrap_or_default();

    let resp = ui.interact(plot, ui.id().with("curves_plot"), Sense::click_and_drag());
    // Scroll zooms about the pointer.
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0
            && let Some(p) = resp.hover_pos()
        {
            let z = (scroll as f64 * 0.005).exp();
            view.zoom_about(plot, p, z, z);
        }
    }
    let shift = ui.input(|i| i.modifiers.shift);
    let pointer = ui.input(|i| i.pointer.interact_pos());

    if resp.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        let pan = ui.input(|i| i.pointer.middle_down() || i.modifiers.alt);
        state.curves.drag = Some(if pan {
            Drag::Pan
        } else if let Some((column, frame, out)) =
            handle_at(&cols, &state.curves.selected, &view, plot, p)
        {
            Drag::Handle { column, frame, out }
        } else if let Some(k) = key_at(&cols, &view, plot, p) {
            if !state.curves.selected.contains(&k) {
                if !shift {
                    state.curves.selected.clear();
                }
                state.curves.selected.insert(k);
            }
            Drag::Keys { from: p }
        } else {
            Drag::Box { from: p }
        });
    }
    if resp.dragged()
        && let (Some(drag), Some(p)) = (state.curves.drag, pointer)
    {
        match drag {
            Drag::Pan => view.pan(resp.drag_delta()),
            Drag::Keys { from } => {
                let frames = ((p.x - from.x) as f64 / view.px_per_frame).round() as i64;
                let dv = ((from.y - p.y) as f64 / view.px_per_value) as f32;
                state.curves.moved = Some((clamp_frames(&state.curves.selected, frames), dv));
            }
            Drag::Handle { column, frame, out } => {
                state.curves.bent = cols
                    .iter()
                    .find(|c| c.id == column)
                    .and_then(|c| bend(c, frame, out, view.from_screen(plot, p)));
            }
            Drag::Box { .. } => {}
        }
    }
    if resp.drag_stopped() {
        match state.curves.drag.take() {
            Some(Drag::Keys { .. }) => {
                if let Some((frames, dv)) = state.curves.moved.take() {
                    if !gesture_safe {
                        state.refuse("refused — finish the stroke first");
                    } else {
                        let keys: Vec<(ParamId, u32)> =
                            state.curves.selected.iter().copied().collect();
                        let frames = clamp_frames(&state.curves.selected, frames);
                        if state.move_param_keys(&keys, frames, dv) {
                            // The selection follows the keys it moved.
                            state.curves.selected = keys
                                .iter()
                                .map(|&(p, f)| (p, (f as i64 + frames) as u32))
                                .collect();
                        }
                    }
                }
            }
            Some(Drag::Handle { column, frame, .. }) => {
                if let Some(key) = state.curves.bent.take() {
                    if gesture_safe {
                        state.set_param_keys("bend handle", vec![(column, frame, key)]);
                    } else {
                        state.refuse("refused — finish the stroke first");
                    }
                }
            }
            Some(Drag::Box { from }) => {
                if let Some(p) = pointer {
                    let r = Rect::from_two_pos(from, p);
                    if !shift {
                        state.curves.selected.clear();
                    }
                    for col in &cols {
                        for (f, k) in col.keys() {
                            if r.contains(view.to_screen(plot, f as f64, k.value as f64)) {
                                state.curves.selected.insert((col.id, f));
                            }
                        }
                    }
                }
            }
            Some(Drag::Pan) | None => {}
        }
        state.curves.moved = None;
        state.curves.bent = None;
    }
    if (resp.clicked() || resp.secondary_clicked())
        && let Some(p) = pointer
    {
        match key_at(&cols, &view, plot, p) {
            Some(k) if shift && resp.clicked() => {
                if !state.curves.selected.remove(&k) {
                    state.curves.selected.insert(k);
                }
            }
            Some(k) => {
                if !(resp.secondary_clicked() && state.curves.selected.contains(&k)) {
                    state.curves.selected.clear();
                    state.curves.selected.insert(k);
                }
            }
            None if !shift && resp.clicked() => state.curves.selected.clear(),
            None => {}
        }
    }

    // ---- paint ----
    let painter = ui.painter_at(plot);
    painter.rect_filled(plot, 0.0, plate::WELL);
    // Time rules with their frame numbers along the top (1-based, as the
    // sheet counts; seconds+frames once they fall on seconds), value ticks
    // with their numbers down the left edge, the value zero line.
    let fps = state.fps().max(1);
    let (fa, va) = view.from_screen(plot, plot.left_bottom());
    let (fb, vb) = view.from_screen(plot, plot.right_top());
    let numbers = egui::FontId::monospace(10.0);
    let tstep = time_step(view.px_per_frame, fps);
    let mut s = ((fa.max(0.0) / tstep as f64).ceil() as u32).saturating_mul(tstep);
    while (s as f64) <= fb {
        let x = view.to_screen(plot, s as f64, 0.0).x;
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            egui::Stroke::new(1.0, plate::rule_beat()),
        );
        painter.text(
            pos2(x + 3.0, plot.top() + 2.0),
            egui::Align2::LEFT_TOP,
            time_label(s, tstep, fps),
            numbers.clone(),
            plate::LEGEND,
        );
        let Some(next) = s.checked_add(tstep) else { break };
        s = next;
    }
    let vstep = value_step(view.px_per_value);
    let mut t = (va / vstep).ceil();
    while t * vstep <= vb {
        let v = t * vstep;
        let y = view.to_screen(plot, 0.0, v).y;
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.left() + 6.0, y)],
            egui::Stroke::new(1.0, plate::LEGEND),
        );
        painter.text(
            pos2(plot.left() + 8.0, y),
            egui::Align2::LEFT_CENTER,
            value_label(v, vstep),
            numbers.clone(),
            plate::LEGEND,
        );
        t += 1.0;
    }
    let y0 = view.to_screen(plot, 0.0, 0.0).y;
    painter.line_segment(
        [pos2(plot.left(), y0), pos2(plot.right(), y0)],
        egui::Stroke::new(1.0, plate::rule_half()),
    );
    // The current frame.
    let xf = view.to_screen(plot, state.view.frame as f64, 0.0).x;
    painter.line_segment(
        [pos2(xf, plot.top()), pos2(xf, plot.bottom())],
        egui::Stroke::new(1.0, plate::TALLY),
    );
    let sel = &state.curves.selected;
    for col in &cols {
        let i = params.iter().position(|(id, _)| *id == col.id).unwrap_or(0);
        // The live drag shows on the curve itself; only a curve the drag
        // changes is copied.
        let shown: Cow<ParamColumn> = match (state.curves.moved, state.curves.drag, state.curves.bent) {
            (Some((frames, dv)), _, _) if sel.iter().any(|(p, _)| *p == col.id) => {
                Cow::Owned(moved_column(col, sel, frames, dv))
            }
            (_, Some(Drag::Handle { column, frame, .. }), Some(k)) if column == col.id => {
                let mut c = col.clone();
                c.set_key(frame, k);
                Cow::Owned(c)
            }
            _ => Cow::Borrowed(col),
        };
        let step = (2.0 / view.px_per_frame).max(1e-3);
        let mut pts = Vec::new();
        let mut f = fa;
        while f <= fb + step {
            if let Some(v) = shown.sample(f) {
                pts.push(view.to_screen(plot, f, v));
            }
            f += step;
        }
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, ink(i))));
        for (f, k) in shown.keys() {
            let at = view.to_screen(plot, f as f64, k.value as f64);
            let picked = sel.contains(&(col.id, f))
                || (state.curves.moved.is_some()
                    && col.key_at(f).is_none_or(|old| old.value != k.value));
            if picked && k.interp == ParamInterp::Bezier
                && let Some((hin, hout)) = handle_points(&shown, f)
            {
                // Only the handles a segment reads (see `live_handles`).
                let (live_in, live_out) = live_handles(&shown, f);
                for (hp, live) in [(hin, live_in), (hout, live_out)] {
                    if !live {
                        continue;
                    }
                    let hs = view.to_screen(plot, hp.0, hp.1);
                    painter.line_segment([at, hs], egui::Stroke::new(1.0, plate::LEGEND));
                    painter.rect_filled(Rect::from_center_size(hs, vec2(5.0, 5.0)), 0.0, plate::LEGEND);
                }
            }
            painter.circle_filled(at, 3.5, ink(i));
            if picked {
                painter.circle_stroke(at, 6.5, egui::Stroke::new(1.5, plate::STRUCK));
            }
        }
    }
    if let (Some(Drag::Box { from }), Some(p)) = (state.curves.drag, pointer) {
        let r = Rect::from_two_pos(from, p);
        painter.rect_filled(r, 0.0, plate::LEGEND.gamma_multiply(0.12));
        painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, plate::LEGEND), egui::StrokeKind::Inside);
    }
    painter.text(
        plot.right_bottom() + vec2(-6.0, -4.0),
        egui::Align2::RIGHT_BOTTOM,
        "drag keys · box select · F frames · right-click: interpolation · Alt+drag pans · scroll zooms",
        egui::FontId::proportional(10.5),
        plate::legend_dim(),
    );
    state.curves.view = Some(view);

    // Interpolation and handle modes for the selected keys: the registry's
    // key verbs, run through the one dispatch once the dock has drawn.
    resp.context_menu(|ui| {
        if state.curves.selected.is_empty() {
            ui.label("select keys first");
            return;
        }
        for (n, (label, action, ..)) in KEY_VERBS.iter().enumerate() {
            if n == 6 {
                ui.separator();
            }
            if ui.button(*label).on_hover_text(action.help()).clicked() {
                state.curves.asked.push(*action);
                ui.close();
            }
        }
    });
    ui.advance_cursor_after_rect(full);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::EditorHarness;
    use crate::workspace::Pane;
    use anim_core::command::Command;

    /// A harness showing only the Curves pane, with one param column keyed
    /// 0 → 0, 10 → 5, 20 → 0 (linear), the view framed on it.
    fn curves() -> (EditorHarness, ParamId) {
        let mut h = EditorHarness::new();
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Curves]);
        let s = h.state_mut();
        s.add_param_column("cam.x");
        let p = s.cut().xsheet.params[0].id;
        let at = s.at();
        let key = |value| {
            Some(ParamKey {
                value,
                interp: ParamInterp::Linear,
                handles: None,
            })
        };
        let cmds = [(0, 0.0), (10, 5.0), (20, 0.0)]
            .into_iter()
            .map(|(frame, v)| Command::SetParamKey {
                at,
                column: p,
                frame,
                key: key(v),
            })
            .collect();
        s.engine.apply("keys", cmds).unwrap();
        h.run();
        h.run();
        (h, p)
    }

    fn screen_of(h: &EditorHarness, p: ParamId, frame: u32) -> Pos2 {
        let s = h.state();
        let v = s.cut().xsheet.param(p).unwrap().key_at(frame).unwrap().value;
        let (view, plot) = (s.curves.view.unwrap(), s.curves.plot.unwrap());
        view.to_screen(plot, frame as f64, v as f64)
    }

    fn keys(h: &EditorHarness, p: ParamId) -> Vec<(u32, f32)> {
        let col = h.state().cut().xsheet.param(p).unwrap().clone();
        col.keys().map(|(f, k)| (f, k.value)).collect()
    }

    #[test]
    fn curves_view_round_trips_screen_and_value() {
        let plot = Rect::from_min_size(pos2(37.0, 61.0), vec2(611.0, 287.0));
        for view in [
            CurveView::default(),
            CurveView::framing(plot, (0.0, 600.0), (-1e3, 2.5e3)),
            CurveView::framing(plot, (12.0, 12.0), (3.0, 3.0)),
            CurveView {
                f0: -40.5,
                v0: 0.125,
                px_per_frame: 13.7,
                px_per_value: 0.37,
            },
        ] {
            for (f, v) in [(0.0, 0.0), (12.5, -3.25), (599.0, 2400.0), (-30.0, 1e-3)] {
                let p = view.to_screen(plot, f, v);
                let (f2, v2) = view.from_screen(plot, p);
                let tol = |s: f64| 1e-4f64.max(2e-4 / s);
                assert!((f2 - f).abs() < tol(view.px_per_frame), "{view:?} f {f} -> {f2}");
                assert!((v2 - v).abs() < tol(view.px_per_value) + 1e-6 * v.abs(), "{view:?} v {v} -> {v2}");
            }
            // Zoom keeps the point under the pointer.
            let mut z = view;
            let at = pos2(200.0, 150.0);
            let before = z.from_screen(plot, at);
            z.zoom_about(plot, at, 1.7, 0.6);
            let after = z.from_screen(plot, at);
            assert!((before.0 - after.0).abs() < 1e-6 && (before.1 - after.1).abs() < 1e-6);
        }
    }

    #[test]
    fn curves_key_drag_moves_by_frames_and_value_in_one_undo_step() {
        let (mut h, p) = curves();
        let start = keys(&h, p);
        let steps = h.state().engine.position();
        let view = h.state().curves.view.unwrap();
        let from = screen_of(&h, p, 10);
        // +3 frames (a little off the grid: time snaps), +2 in value.
        let to = from
            + vec2(
                (3.2 * view.px_per_frame) as f32,
                -(2.0 * view.px_per_value) as f32,
            );
        h.drag(from, to, 6);
        let got = keys(&h, p);
        assert_eq!(got.len(), 3);
        assert_eq!(got[1].0, 13, "snapped to whole frames: {got:?}");
        assert!((got[1].1 - 7.0).abs() < 0.05, "value moved by 2: {got:?}");
        assert_eq!(h.state().engine.position(), steps + 1, "one drag, one undo step");
        assert!(h.state().curves.selected.contains(&(p, 13)), "the selection follows");
        h.perform(crate::config::Action::Undo);
        assert_eq!(keys(&h, p), start);
    }

    #[test]
    fn curves_dragging_onto_a_keyed_frame_replaces_it() {
        let (mut h, p) = curves();
        let view = h.state().curves.view.unwrap();
        let from = screen_of(&h, p, 10);
        let to = from + vec2((10.0 * view.px_per_frame) as f32, 0.0);
        h.drag(from, to, 8);
        let got = keys(&h, p);
        assert_eq!(got.iter().map(|k| k.0).collect::<Vec<_>>(), vec![0, 20], "never duplicated");
        assert!((got[1].1 - 5.0).abs() < 0.05, "the dragged key won: {got:?}");
    }

    #[test]
    fn curves_box_select_takes_the_keys_inside() {
        let (mut h, p) = curves();
        let (k0, k10, k20) = (screen_of(&h, p, 0), screen_of(&h, p, 10), screen_of(&h, p, 20));
        // A box from left of key 0 to between keys 10 and 20, spanning
        // every value: keys 0 and 10, not 20.
        let plot = h.state().curves.plot.unwrap();
        let from = pos2(k0.x - 10.0, plot.top() + 30.0);
        let to = pos2((k10.x + k20.x) * 0.5, plot.bottom() - 4.0);
        h.drag(from, to, 6);
        let sel: Vec<(ParamId, u32)> = h.state().curves.selected.iter().copied().collect();
        assert_eq!(sel, vec![(p, 0), (p, 10)]);
        assert_eq!(keys(&h, p).len(), 3, "a box moves nothing");
    }

    #[test]
    fn curves_f_frames_everything_without_stepping_the_frame() {
        let (mut h, p) = curves();
        // Somewhere far away.
        h.state_mut().curves.view = Some(CurveView {
            f0: 900.0,
            v0: -400.0,
            px_per_frame: 30.0,
            px_per_value: 0.5,
        });
        h.run();
        let plot = h.state().curves.plot.unwrap();
        let frame = h.state().view.frame;
        h.point_at(plot.center());
        h.key(egui::Key::F);
        for f in [0, 10, 20] {
            let at = screen_of(&h, p, f);
            assert!(plot.contains(at), "key {f} at {at:?} outside {plot:?}");
        }
        assert_eq!(h.state().view.frame, frame, "F over the curves never steps the frame");
        assert!(h.performed().iter().all(|(a, _)| *a != crate::config::Action::NextFrame));
    }

    #[test]
    fn curves_context_menu_sets_interpolation_in_one_step() {
        let (mut h, p) = curves();
        let steps = h.state().engine.position();
        h.right_click_pos(screen_of(&h, p, 10));
        assert!(h.has("ease in"), "{}", h.describe());
        h.click("ease in");
        let col = h.state().cut().xsheet.param(p).unwrap().clone();
        assert_eq!(col.key_at(10).unwrap().interp, ParamInterp::EaseIn);
        assert_eq!(col.key_at(0).unwrap().interp, ParamInterp::Linear, "only the selected key");
        assert_eq!(h.state().engine.position(), steps + 1);
    }

    #[test]
    fn curves_handle_drag_bends_a_bezier_key_in_one_step() {
        let (mut h, p) = curves();
        // Key 0 becomes Bezier (auto handles) from the menu.
        h.right_click_pos(screen_of(&h, p, 0));
        h.click("bezier");
        let steps = h.state().engine.position();
        let col = h.state().cut().xsheet.param(p).unwrap().clone();
        let (_, out) = handle_points(&col, 0).unwrap();
        let (view, plot) = (h.state().curves.view.unwrap(), h.state().curves.plot.unwrap());
        let from = view.to_screen(plot, out.0, out.1);
        let to = from - vec2(0.0, (2.0 * view.px_per_value) as f32);
        h.drag(from, to, 6);
        let k = h.state().cut().xsheet.param(p).unwrap().key_at(0).unwrap();
        let hd = k.handles.expect("the drag stores handles");
        assert_eq!(hd.mode, HandleMode::Smooth, "an auto key turns smooth");
        assert!((hd.out_dv - (out.1 as f32 + 2.0)).abs() < 0.05, "{hd:?} vs {out:?}");
        assert_eq!(h.state().engine.position(), steps + 1);
    }

    #[test]
    fn curves_header_types_a_keys_value_in_one_step() {
        let (mut h, p) = curves();
        assert!(h.has_text_containing("click a key"), "{}", h.describe());
        h.click_pos(screen_of(&h, p, 10));
        assert!(h.has("cam.x @ 11"), "the header names the key, 1-based\n{}", h.describe());
        let field = h.state().curves.value_field.expect("the value field drew");
        let steps = h.state().engine.position();
        h.click_pos(field.center());
        assert!(h.typing(), "a click types into the value\n{}", h.describe());
        h.type_text("7.5");
        h.key(egui::Key::Enter);
        let k = h.state().cut().xsheet.param(p).unwrap().key_at(10).unwrap();
        assert_eq!(k.value, 7.5);
        assert_eq!(k.interp, ParamInterp::Linear, "only the value changed");
        assert_eq!(h.state().engine.position(), steps + 1, "one typed value, one undo step");
        assert!(!h.typing());
        assert_eq!(keys(&h, p).len(), 3);
    }

    #[test]
    fn curves_rules_and_ticks_carry_numbers() {
        // Time: the first rung with room for its label; from a second up
        // the rules fall on whole seconds, labelled seconds+frames.
        assert_eq!(time_step(10.0, 24), 6);
        assert_eq!(time_step(1.0, 24), 48);
        assert_eq!(time_step(100.0, 24), 1);
        assert_eq!(time_label(0, 6, 24), "1", "frames count from 1, as the sheet does");
        assert_eq!(time_label(11, 6, 24), "12");
        assert_eq!(time_label(48, 48, 24), "2+0");
        // Value: 1, 2 or 5 × a power of ten, about 40 points apart, with
        // the decimals the step needs.
        assert_eq!(value_step(10.0), 5.0);
        assert!((value_step(100.0) - 0.5).abs() < 1e-12);
        assert!((value_step(0.4) - 100.0).abs() < 1e-9);
        assert_eq!(value_label(2.5, 0.5), "2.5");
        assert_eq!(value_label(10.0, 5.0), "10");
        assert_eq!(value_label(-1e-12, 0.5), "0.0", "no minus zero");
    }

    #[test]
    fn curves_handles_show_only_where_they_shape_the_curve() {
        let mut col = ParamColumn::new(ParamId(1), "cam.x");
        let k = |interp| ParamKey {
            value: 0.0,
            interp,
            handles: None,
        };
        col.set_key(0, k(ParamInterp::Bezier));
        col.set_key(10, k(ParamInterp::Linear));
        col.set_key(20, k(ParamInterp::Bezier));
        // (in, out): out when a key follows; in when the key before is
        // Bezier (its segment reads it).
        assert_eq!(live_handles(&col, 0), (false, true));
        assert_eq!(live_handles(&col, 10), (true, true));
        assert_eq!(live_handles(&col, 20), (false, false));
        // A drag left stops the whole selection together at frame 0.
        let sel: BTreeSet<(ParamId, u32)> = [(ParamId(1), 4), (ParamId(2), 8)].into_iter().collect();
        assert_eq!(clamp_frames(&sel, -9), -4);
        assert_eq!(clamp_frames(&sel, 3), 3);
        let moved = moved_column(&col, &[(ParamId(1), 10)].into_iter().collect(), -12, 1.0);
        assert_eq!(moved.keys().map(|(f, _)| f).collect::<Vec<_>>(), vec![0, 20], "10 - 12 lands on 0");
    }

    #[test]
    fn curves_verbs_live_in_the_registry_and_open_the_pane() {
        use crate::config::{Ctx, Keymap, default_binds};
        // The palette finds the pane by the words animators use for it.
        for q in ["graph editor", "f-curve", "curves"] {
            assert_eq!(crate::cmdpal::ranked(q, &[]).first(), Some(&Action::ShowCurves), "{q}");
        }
        assert_eq!(Action::ShowCurves.menu(), Some("More/Panels/Motion curves"));
        // Frame curves has a chord in both keymaps, over the curves pane;
        // the key verbs are filed in the Sheet menu.
        for km in Keymap::ALL {
            let binds = default_binds(*km);
            assert!(crate::cmdpal::chord_of(&binds, Action::FrameCurves).is_some(), "{km:?}");
        }
        assert_eq!(Action::FrameCurves.context(), Ctx::Curves);
        for (_, action, _, _) in KEY_VERBS {
            assert_eq!(action.context(), Ctx::Curves, "{action:?}");
            assert!(action.menu().is_some_and(|m| m.starts_with("Animation/")), "{action:?}");
        }
        // Window › Curves docks the pane where it is not.
        let mut h = EditorHarness::new();
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::XSheet]);
        h.run();
        h.perform(Action::ShowCurves);
        assert!(h.editor().dock.find_tab(&Pane::Curves).is_some());
    }
}
