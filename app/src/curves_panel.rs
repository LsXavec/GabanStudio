//! GM14: the Curves pane — the param columns' values over frames, drawn as
//! curves. Keys select (click, Shift+click, a box) and drag (time snaps to
//! whole frames; one drag is one undo step); a selected Bezier key's
//! handles drag too; the context menu sets interpolation. F frames the
//! selection, or everything. Edits go through SetParamKeys only.

use std::collections::BTreeSet;

use anim_core::ids::ParamId;
use anim_core::xsheet::{HandleMode, Handles, ParamColumn, ParamInterp, ParamKey};
use eframe::egui;
use egui::{Pos2, Rect, Sense, pos2, vec2};

use crate::doc::AppState;
use crate::plate;

/// How close (points) the pointer must be to take a key or a handle.
const HIT: f32 = 7.0;
/// The column list's width.
const LIST_W: f32 = 112.0;

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
}

/// One colour per shown column, from the plate's tokens.
fn ink(i: usize) -> egui::Color32 {
    [plate::AO, plate::TALLY, plate::AKA, plate::PAPER, plate::STRUCK][i % 5]
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

/// `col` with the selected keys moved by (frames, dv) — the drag's
/// preview, by the same law as `sheet_ops::move_param_keys`.
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
            let at = view.to_screen(plot, f as f64, k.value as f64);
            for (pt, out) in [(hout, true), (hin, false)] {
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

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool) {
    // Selections of keys that no longer exist (undo, the sheet) drop out.
    let live: BTreeSet<(ParamId, u32)> = state
        .curves
        .selected
        .iter()
        .copied()
        .filter(|(p, f)| state.cut().xsheet.param(*p).is_some_and(|c| c.key_at(*f).is_some()))
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
        ui.label(
            egui::RichText::new("no param columns yet — add one in the sheet (camera moves etc.)")
                .color(plate::legend_dim()),
        );
        return;
    }
    let full = ui.available_rect_before_wrap();
    let list = Rect::from_min_size(full.min, vec2(LIST_W, full.height()));
    let plot = Rect::from_min_max(pos2(full.left() + LIST_W + 4.0, full.top()), full.max);

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

    state.curves.plot = Some(plot);
    let cols = shown_columns(state);
    if state.curves.view.is_none() {
        frame_view(state, plot);
    }
    // F frames the selection (or everything) while the pointer is over
    // the pane — taken here, so the sheet's F (next frame) stays put.
    let pointer_here = ui.rect_contains_pointer(full);
    if pointer_here
        && !ui.ctx().text_edit_focused()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F))
    {
        frame_view(state, plot);
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
                state.curves.moved = Some((frames, dv));
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
                if let Some((frames, dv)) = state.curves.moved.take()
                    && gesture_safe
                {
                    let keys: Vec<(ParamId, u32)> = state.curves.selected.iter().copied().collect();
                    let lowest = keys.iter().map(|&(_, f)| f as i64).min().unwrap_or(0);
                    let frames = frames.max(-lowest);
                    if state.move_param_keys(&keys, frames, dv) {
                        // The selection follows the keys it moved.
                        state.curves.selected = keys
                            .iter()
                            .map(|&(p, f)| (p, (f as i64 + frames) as u32))
                            .collect();
                    }
                }
            }
            Some(Drag::Handle { column, frame, .. }) => {
                if let Some(key) = state.curves.bent.take()
                    && gesture_safe
                {
                    state.set_param_keys("bend handle", vec![(column, frame, key)]);
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
    // Frame rules every second, value zero line.
    let fps = state.fps().max(1) as f64;
    let (fa, _) = view.from_screen(plot, plot.left_bottom());
    let (fb, _) = view.from_screen(plot, plot.right_top());
    let step = if view.px_per_frame * fps < 24.0 { fps * 4.0 } else { fps };
    let mut s = (fa / step).floor() * step;
    while s <= fb {
        let x = view.to_screen(plot, s, 0.0).x;
        painter.line_segment(
            [pos2(x, plot.top()), pos2(x, plot.bottom())],
            egui::Stroke::new(1.0, plate::rule_beat()),
        );
        s += step;
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
        // The live drag shows on the curve itself.
        let shown = match (state.curves.moved, state.curves.drag, state.curves.bent) {
            (Some((frames, dv)), _, _) => moved_column(col, sel, frames, dv),
            (_, Some(Drag::Handle { column, frame, .. }), Some(k)) if column == col.id => {
                let mut c = col.clone();
                c.set_key(frame, k);
                c
            }
            _ => col.clone(),
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
                for hp in [hin, hout] {
                    let hs = view.to_screen(plot, hp.0, hp.1);
                    painter.line_segment([at, hs], egui::Stroke::new(1.0, plate::LEGEND));
                    painter.rect_filled(Rect::from_center_size(hs, vec2(5.0, 5.0)), 0.0, plate::LEGEND);
                }
            }
            painter.circle_filled(at, if picked { 4.5 } else { 3.5 }, if picked { plate::STRUCK } else { ink(i) });
        }
    }
    if let (Some(Drag::Box { from }), Some(p)) = (state.curves.drag, pointer) {
        let r = Rect::from_two_pos(from, p);
        painter.rect_filled(r, 0.0, plate::AO.gamma_multiply(0.15));
        painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, plate::AO), egui::StrokeKind::Inside);
    }
    painter.text(
        plot.left_top() + vec2(6.0, 4.0),
        egui::Align2::LEFT_TOP,
        "drag keys · box select · F frames · right-click: interpolation · Alt+drag pans · scroll zooms",
        egui::FontId::proportional(10.5),
        plate::legend_dim(),
    );
    state.curves.view = Some(view);

    // Interpolation and handle modes for the selected keys.
    resp.context_menu(|ui| {
        if state.curves.selected.is_empty() {
            ui.label("select keys first");
            return;
        }
        for (label, interp) in [
            ("hold", ParamInterp::Hold),
            ("linear", ParamInterp::Linear),
            ("ease", ParamInterp::Ease),
            ("ease in", ParamInterp::EaseIn),
            ("ease out", ParamInterp::EaseOut),
            ("bezier", ParamInterp::Bezier),
        ] {
            if ui.button(label).clicked() {
                if gesture_safe {
                    set_interp(state, interp, None, &format!("interpolation: {label}"));
                }
                ui.close();
            }
        }
        ui.separator();
        for mode in HandleMode::ALL {
            let label = format!("{} handles", mode.key());
            if ui.button(&label).clicked() {
                if gesture_safe {
                    set_interp(state, ParamInterp::Bezier, Some(mode), &label);
                }
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
}
