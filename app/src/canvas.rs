//! The drawing canvas: a fixed-size logical "paper" (resolution independent)
//! rendered into the panel with pan/zoom. Pen strokes land in paper
//! coordinates so artwork never depends on window size or zoom level.
//!
//! Input reuses the M0-validated path: Windows Ink pen arrives as egui Touch
//! events with real pressure; mouse is the no-pressure fallback.

use anim_core::ids::ColumnId;
use anim_core::model::{Stroke, StrokePoint};
use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, pos2, vec2};

use crate::config::{PenConfig, PressureCurve};
use crate::doc::AppState;
use crate::paint::{Dab, PaintLayer};

/// Default new-project resolution (the New Project dialog's starting values).
pub const DEFAULT_PAPER_W: u32 = 1920;
pub const DEFAULT_PAPER_H: u32 = 1080;

const SWATCHES: [[u8; 4]; 4] = [
    [25, 25, 30, 255],   // ink
    [200, 40, 40, 255],  // red (shadow lines, corrections)
    [40, 80, 200, 255],  // blue (roughs/layout, like blue pencil)
    [245, 245, 245, 255],// white
];

// Pen-input tuning derived from Krita's tablet pipeline (see
// research/krita-pen-tuning.md). The load-bearing insight: egui/winit on
// Windows Ink never delivers the decreasing-pressure samples Krita relies on
// at lift-off, so we must SYNTHESIZE the taper ourselves (END_TAPER_POINTS)
// rather than trust the release event or the EMA to reach zero.

/// Provisional pressure for the Start point (Windows Ink Start force is usually
/// None/0). Overwritten by the first real Move sample; a pure tap keeps it, so
/// a dot is modest, not full width.
const START_SEED: f32 = 0.3;
/// Minimum screen-space distance between committed samples — banks a
/// decelerating pen onto one point instead of stacking overlapping AA
/// segments. Screen space = constant physical dead-zone (Krita Scalable Distance).
const MIN_SAMPLE_DIST: f32 = 1.5;
/// EMA factor toward each new pressure sample (weak stabilizer). NOT trusted to
/// reach 0 at the tail — the end taper overwrites the tail regardless.
const PRESSURE_SMOOTH: f32 = 0.5;
/// Number of trailing points whose pressure is ramped to 0 at pen-up, so the
/// stroke narrows to a clean tip instead of ending on a full-pressure disc.
const END_TAPER_POINTS: usize = 5;
/// Interior-only width floor: avoids sub-pixel invisibility mid-stroke without
/// re-creating a minimum-radius dot at the (now zero-pressure) tip.
const WIDTH_FLOOR: f32 = 0.1;

pub struct CanvasView {
    zoom: f32,
    pan: egui::Vec2,
    pub brush_width: f32,
    pub brush_color: [u8; 4],
    current: Vec<StrokePoint>,
    touch_active: bool,
    /// Start point still holds the provisional seed (no real Move yet).
    seed_pending: bool,
    /// Last REAL (>0) pressure the tablet reported; reused for force:None
    /// packets instead of a fabricated default.
    last_pressure: f32,
    smoothed_pressure: f32,
    /// Last 3 raw pressures for a median pre-filter (kills single-packet noise
    /// without EMA lag).
    raw_history: [f32; 3],
    /// Frames remaining in the post-lift mouse lockout: egui synthesizes a
    /// PointerButton(Released)/drag_stopped from the pen after Touch::End; this
    /// stops a phantom flat-pressure mouse point from being appended.
    mouse_lockout: u8,

    // --- Live pressure diagnostics (proves capture is real, not flat) ---
    dbg_pressure: f32,
    dbg_min: f32,
    dbg_max: f32,
    dbg_some: u32,
    dbg_none: u32,
    cur_some: u32,
    cur_none: u32,

    // --- Raster brush: GPU layer mirrors the current cel; strokes read back
    // into the engine as PaintTiles (undoable + saved) ---
    raster: bool,
    raster_brush_px: f32,
    /// How many of the current stroke's dabs are already on the GPU layer.
    dabs_flushed: usize,
    /// The stroke finished this frame; flush its last dabs, then reset.
    raster_stroke_done: bool,
    /// (drawing id, raster hash) currently uploaded to the GPU layer — when it
    /// no longer matches the current cel, re-sync from the engine.
    synced: (u64, u64),
    /// This stroke will create a NEW cel (the frame had no own key) — clear the
    /// GPU layer at the first dab so the new cel is blank, not a copy of the
    /// held drawing that was on display.
    raster_new_cel: bool,
    /// Pressure response curve (from Pen/Tablet settings); remaps pressure to
    /// width at render time. Stored pressure stays raw.
    pen_curve: PressureCurve,
}

impl CanvasView {
    pub fn new() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            brush_width: 3.0,
            brush_color: SWATCHES[0],
            current: Vec::new(),
            touch_active: false,
            seed_pending: false,
            last_pressure: START_SEED,
            smoothed_pressure: START_SEED,
            raw_history: [START_SEED; 3],
            mouse_lockout: 0,
            dbg_pressure: 0.0,
            dbg_min: 0.0,
            dbg_max: 0.0,
            dbg_some: 0,
            dbg_none: 0,
            cur_some: 0,
            cur_none: 0,
            raster: true,
            raster_brush_px: 14.0,
            dabs_flushed: 0,
            raster_stroke_done: false,
            synced: (u64::MAX, u64::MAX), // force an initial sync
            raster_new_cel: false,
            pen_curve: PressureCurve::linear(),
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        paint: Option<&mut PaintLayer>,
        pen: &PenConfig,
    ) {
        self.pen_curve = pen.pressure_curve.clone();
        let mut paint = paint;
        // ---- Toolbar ------------------------------------------------------
        ui.horizontal(|ui| {
            let raster_available = paint.is_some();
            if raster_available {
                ui.checkbox(&mut self.raster, "raster")
                    .on_hover_text("GPU raster brush (Phase 1 preview: one scratch layer, not yet per-frame/undo/saved)");
                ui.separator();
            } else {
                self.raster = false;
            }

            if self.raster {
                ui.add(
                    egui::Slider::new(&mut self.raster_brush_px, 1.0..=300.0)
                        .text("px")
                        .fixed_decimals(0),
                );
                if ui
                    .button("clear cel")
                    .on_hover_text("clear this cel's raster (undoable)")
                    .clicked()
                {
                    state.clear_current_raster();
                }
                if ui.button("test").on_hover_text("stamp test dabs (checks the GPU display path)").clicked()
                    && let Some(p) = paint.as_deref_mut() {
                        let (w, h) = p.size();
                        let col = linear_rgba(self.brush_color);
                        let test = vec![
                            Dab { center: [w as f32 * 0.35, h as f32 * 0.5], radius: h as f32 * 0.18, hardness: 0.5, color: col },
                            Dab { center: [w as f32 * 0.65, h as f32 * 0.5], radius: h as f32 * 0.10, hardness: 0.95, color: col },
                        ];
                        p.paint(&test);
                    }
            } else {
                ui.add(
                    egui::Slider::new(&mut self.brush_width, 0.5..=16.0)
                        .text("brush")
                        .fixed_decimals(1),
                );
            }
            ui.separator();
            for c in SWATCHES {
                let selected = self.brush_color == c;
                let swatch = egui::RichText::new("⬤")
                    .size(16.0)
                    .color(Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]));
                if ui.selectable_label(selected, swatch).clicked() {
                    self.brush_color = c;
                }
            }
            ui.separator();
            if ui.button("fit view").clicked() {
                self.zoom = 1.0;
                self.pan = egui::Vec2::ZERO;
            }
            ui.separator();
            // Live pressure diagnostic: current value + last stroke's range and
            // how many samples carried real force vs None. If the range is wide,
            // the device delivers varying pressure and the ribbon renders it; if
            // min≈max or none≫some, the problem is capture, not rendering.
            let total = (self.dbg_some + self.dbg_none).max(1);
            let real_pct = 100 * self.dbg_some / total;
            ui.label(
                egui::RichText::new(format!(
                    "P {:.2}   last {:.2}–{:.2}   force {}%",
                    self.dbg_pressure, self.dbg_min, self.dbg_max, real_pct
                ))
                .monospace()
                .color(if self.dbg_max - self.dbg_min > 0.15 {
                    Color32::from_rgb(120, 200, 140)
                } else {
                    Color32::from_rgb(210, 180, 90)
                }),
            );
            if state.playing {
                ui.separator();
                ui.label(
                    egui::RichText::new("PLAYING (space to stop)")
                        .color(Color32::from_rgb(120, 220, 140)),
                );
            }
        });

        // ---- Canvas area --------------------------------------------------
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        // Paper size = the project's chosen resolution (set at creation).
        let paper_w = state.engine.project.width as f32;
        let paper_h = state.engine.project.height as f32;

        // View transform: fit paper into rect, then user zoom/pan on top.
        let fit = ((rect.width() / paper_w).min(rect.height() / paper_h) * 0.94).max(0.01);
        let scale = fit * self.zoom;
        let origin = rect.center() - vec2(paper_w, paper_h) * scale * 0.5 + self.pan;
        let to_screen = |p: Pos2| -> Pos2 { origin + p.to_vec2() * scale };
        let to_paper = |s: Pos2| -> Pos2 { ((s - origin) / scale).to_pos2() };

        // Zoom at cursor.
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0
                && let Some(mouse) = response.hover_pos() {
                    let before = to_paper(mouse);
                    self.zoom = (self.zoom * (scroll * 0.0015).exp()).clamp(0.2, 10.0);
                    let scale2 = fit * self.zoom;
                    let origin2 = rect.center() - vec2(paper_w, paper_h) * scale2 * 0.5 + self.pan;
                    let after = origin2 + before.to_vec2() * scale2;
                    self.pan += mouse - after;
                }
        }
        // Middle-drag pans.
        if response.dragged_by(egui::PointerButton::Middle) {
            self.pan += response.drag_delta();
        }

        // ---- Paper --------------------------------------------------------
        let paper_rect =
            Rect::from_min_max(to_screen(pos2(0.0, 0.0)), to_screen(pos2(paper_w, paper_h)));
        painter.rect_filled(paper_rect, 2, Color32::from_rgb(242, 239, 233));
        painter.rect_stroke(
            paper_rect,
            2,
            egui::Stroke::new(1.0, Color32::from_gray(60)),
            egui::StrokeKind::Outside,
        );

        // ---- Pen input (edit mode only) -----------------------------------
        if !state.playing {
            self.handle_pen(ui, &response, rect, &to_paper, scale, state);
        } else {
            self.current.clear();
            self.touch_active = false;
        }

        // ---- Raster: keep the GPU layer synced to the current cel, stamp
        //      live dabs, and read back into the engine at pen-up ----------
        if self.raster
            && let Some(p) = paint.as_deref_mut()
        {
            p.ensure_size(state.engine.project.width, state.engine.project.height);

            if self.current.is_empty() && !self.raster_stroke_done {
                // Between strokes: re-upload if the current cel changed
                // (frame switch, undo, redo, selection).
                let key = state.current_raster_key();
                if key != self.synced {
                    match state.current_raster_tiles() {
                        Some(tiles) => p.sync_from(tiles),
                        None => p.clear(),
                    }
                    self.synced = key;
                }
            } else {
                // Starting a new cel: wipe the held drawing off the GPU layer
                // so the new cel is blank, not a copy of what was displayed.
                if self.raster_new_cel && self.dabs_flushed == 0 {
                    p.clear();
                }
                // Drawing (or finishing this frame): stamp the new dabs.
                let dabs = self.build_stroke_dabs();
                if dabs.len() > self.dabs_flushed {
                    p.paint(&dabs[self.dabs_flushed..]);
                    self.dabs_flushed = dabs.len();
                }
                if self.raster_stroke_done {
                    // Read the painted layer back and commit it as a PaintTiles
                    // edit (undoable, saved).
                    let tiles = p.read_tiles();
                    if let Some(id) = state.commit_raster(tiles) {
                        let h = state
                            .cut()
                            .drawing(id)
                            .and_then(|d| d.raster.as_ref())
                            .map(|r| r.content_hash())
                            .unwrap_or(0);
                        self.synced = (id.0, h);
                    }
                    self.current.clear();
                    self.dabs_flushed = 0;
                    self.raster_stroke_done = false;
                }
            }
        }

        // ---- Render layers ------------------------------------------------
        let cut = state.cut();
        let active_col = state.active_column;
        let frame = state.frame;

        // 1. Non-active columns (in sheet order = layer order).
        for col in &cut.xsheet.columns {
            if col.id == active_col {
                continue;
            }
            if let Some(id) = col.resolve(frame)
                && let Some(d) = cut.drawing(id) {
                    draw_strokes(&painter, &d.strokes, &to_screen, scale, None, &self.pen_curve);
                }
        }

        // 2. Onion ghosts of the active column, under its current drawing.
        if state.onion {
            let ghosts = onion_ghosts(state, active_col, frame);
            for (id, tint) in ghosts {
                if let Some(d) = cut.drawing(id) {
                    draw_strokes(
                        &painter,
                        &d.strokes,
                        &to_screen,
                        scale,
                        Some(tint),
                        &self.pen_curve,
                    );
                }
            }
        }

        // 3. Active column's current drawing on top.
        if let Some(id) = state.resolve_at(active_col, frame)
            && let Some(d) = state.cut().drawing(id) {
                draw_strokes(&painter, &d.strokes, &to_screen, scale, None, &self.pen_curve);
            }

        // 3b. The GPU raster layer, drawn over the paper at the paper rect.
        if self.raster
            && let Some(p) = paint {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(p.texture_id(), paper_rect, uv, Color32::WHITE);
            }

        // 4. In-progress stroke preview (vector mode only — the raster layer
        //    already shows the live stroke when raster is on).
        if !self.raster && self.current.len() >= 2 {
            let c = self.brush_color;
            let color = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            fill_stroke(
                &painter,
                &self.current,
                self.brush_width,
                scale,
                color,
                &to_screen,
                &self.pen_curve,
            );
        }

        // Empty-cell hint (vector mode only).
        if !self.raster
            && !state.playing
            && state.current_drawing().is_none()
            && self.current.is_empty()
        {
            painter.text(
                paper_rect.center(),
                egui::Align2::CENTER_CENTER,
                "empty cell — draw to create a new drawing here",
                egui::FontId::proportional(15.0),
                Color32::from_gray(150),
            );
        }
    }

    /// Walk the current stroke's points into evenly spaced dabs (paper space).
    /// Deterministic prefix: appending points only extends the tail, so
    /// `dabs[dabs_flushed..]` are always genuinely new.
    fn build_stroke_dabs(&self) -> Vec<Dab> {
        let pts = &self.current;
        let mut dabs = Vec::new();
        if pts.is_empty() {
            return dabs;
        }
        let color = linear_rgba(self.brush_color);
        let hardness = 0.85;
        let radius_of =
            |pr: f32| (self.raster_brush_px * self.pen_curve.apply(pr) * 0.5).max(0.5);

        if pts.len() == 1 {
            dabs.push(Dab {
                center: [pts[0].x, pts[0].y],
                radius: radius_of(pts[0].pressure),
                hardness,
                color,
            });
            return dabs;
        }

        let mut carry = 0.0f32; // distance into the next segment for the next dab
        for w in pts.windows(2) {
            let (ax, ay, apr) = (w[0].x, w[0].y, w[0].pressure);
            let (bx, by, bpr) = (w[1].x, w[1].y, w[1].pressure);
            let (dx, dy) = (bx - ax, by - ay);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-4 {
                continue;
            }
            let mut d = carry;
            while d < len {
                let t = d / len;
                let pr = apr + (bpr - apr) * t;
                let r = radius_of(pr);
                dabs.push(Dab {
                    center: [ax + dx * t, ay + dy * t],
                    radius: r,
                    hardness,
                    color,
                });
                let step = (0.1 * (2.0 * r)).max(0.75); // spacing 0.1 * diameter
                d += step;
            }
            carry = d - len;
        }
        dabs
    }

    fn handle_pen(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        to_paper: &impl Fn(Pos2) -> Pos2,
        scale: f32,
        state: &mut AppState,
    ) {
        if self.mouse_lockout > 0 {
            self.mouse_lockout -= 1;
        }

        let events = ui.input(|i| i.events.clone());
        let mut touch_seen = false;

        for event in &events {
            if let egui::Event::Touch {
                pos, force, phase, ..
            } = event
            {
                touch_seen = true;
                match phase {
                    // Step 2: distrust the Start force (WM_POINTERDOWN pressure
                    // is usually 0 -> None). Seed provisionally; the first real
                    // Move overwrites it so strokes don't begin with a fat dot.
                    egui::TouchPhase::Start => {
                        if !rect.contains(*pos) {
                            continue;
                        }
                        let p0 = force.filter(|f| *f > 0.0).unwrap_or(START_SEED);
                        self.touch_active = true;
                        self.seed_pending = force.filter(|f| *f > 0.0).is_none();
                        self.last_pressure = p0;
                        self.smoothed_pressure = p0;
                        self.raw_history = [p0; 3];
                        self.cur_some = 0;
                        self.cur_none = 0;
                        self.dabs_flushed = 0;
                        self.raster_stroke_done = false;
                        self.raster_new_cel = self.raster && state.own_key_drawing().is_none();
                        self.current.clear();
                        let p = to_paper(*pos);
                        self.current.push(StrokePoint {
                            x: p.x,
                            y: p.y,
                            pressure: p0,
                        });
                    }
                    egui::TouchPhase::Move => {
                        if !self.touch_active {
                            continue;
                        }
                        self.process_move(*pos, *force, to_paper, scale);
                    }
                    // Step 6: the release event's own pos/force are unreliable
                    // (End force is None on Windows). Discard them; synthesize
                    // the taper from the committed points instead.
                    egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                        if self.touch_active {
                            self.finish_stroke(state);
                            self.mouse_lockout = 1;
                        }
                        self.touch_active = false;
                        self.seed_pending = false;
                    }
                }
            }
        }

        // Mouse fallback (flat pressure) — only when NO pen stream is present,
        // and not during the post-lift lockout that suppresses egui's
        // synthesized primary-drag for the pen that just finished.
        if !self.touch_active && !touch_seen && self.mouse_lockout == 0 {
            if response.drag_started_by(egui::PointerButton::Primary) {
                self.current.clear();
                self.dabs_flushed = 0;
                self.raster_stroke_done = false;
                self.raster_new_cel = self.raster && state.own_key_drawing().is_none();
                if let Some(p) = response.interact_pointer_pos() {
                    let p = to_paper(p);
                    self.current.push(StrokePoint {
                        x: p.x,
                        y: p.y,
                        pressure: 0.5,
                    });
                }
            } else if response.dragged_by(egui::PointerButton::Primary) {
                if let Some(p) = response.interact_pointer_pos() {
                    let p = to_paper(p);
                    self.current.push(StrokePoint {
                        x: p.x,
                        y: p.y,
                        pressure: 0.5,
                    });
                }
            } else if response.drag_stopped_by(egui::PointerButton::Primary)
                && !self.current.is_empty()
            {
                // Mouse has no pressure, so no taper is synthesized here.
                self.finish_stroke(state);
            }
        }
    }

    /// One pen Move sample: pressure derivation (Steps 3-5) + distance banking.
    fn process_move(
        &mut self,
        pos: Pos2,
        force: Option<f32>,
        to_paper: &impl Fn(Pos2) -> Pos2,
        scale: f32,
    ) {
        let p = to_paper(pos);
        let moved = self
            .current
            .last()
            .map(|last| ((p.x - last.x).powi(2) + (p.y - last.y).powi(2)).sqrt() * scale)
            .unwrap_or(f32::INFINITY);

        // Pressure comes ONLY from the tablet. Some(>0) is real (a stroke start
        // is a rapid pressure rise while nearly stationary — that is normal, NOT
        // a spike to reject); Some(0)/None reuse the last real value.
        match force {
            Some(f) if f > 0.0 => self.cur_some += 1,
            _ => self.cur_none += 1,
        }
        let raw = match force {
            Some(f) if f > 0.0 => {
                self.last_pressure = f;
                f
            }
            Some(_) => 0.0,
            None => self.last_pressure,
        };

        // First real Move adopts the pressure immediately (no seed lag), so the
        // stroke has correct width from the moment the pen presses down.
        if self.seed_pending && matches!(force, Some(f) if f > 0.0) {
            self.smoothed_pressure = raw;
            self.raw_history = [raw; 3];
            if let Some(first) = self.current.first_mut() {
                first.pressure = raw;
            }
            self.seed_pending = false;
        }

        // Median-of-3 kills single-packet noise; a gentle EMA smooths. No spike
        // rejection — that was starving the stroke start of pressure.
        self.raw_history = [self.raw_history[1], self.raw_history[2], raw];
        let filtered = median3(self.raw_history);
        self.smoothed_pressure += (filtered - self.smoothed_pressure) * PRESSURE_SMOOTH;
        self.dbg_pressure = self.smoothed_pressure;

        // Distance gate: bank a near-stationary sample onto the last point,
        // tracking the LATEST pressure (min biased pressure downward → jumpy).
        if moved < MIN_SAMPLE_DIST {
            if let Some(lp) = self.current.last_mut() {
                lp.pressure = self.smoothed_pressure;
            }
            return;
        }
        self.current.push(StrokePoint {
            x: p.x,
            y: p.y,
            pressure: self.smoothed_pressure,
        });
    }

    fn finish_stroke(&mut self, state: &mut AppState) {
        // Snapshot the captured pressure range (pre-taper) for the diagnostic.
        if !self.current.is_empty() {
            self.dbg_min = self
                .current
                .iter()
                .map(|p| p.pressure)
                .fold(f32::INFINITY, f32::min);
            self.dbg_max = self
                .current
                .iter()
                .map(|p| p.pressure)
                .fold(0.0, f32::max);
            self.dbg_some = self.cur_some;
            self.dbg_none = self.cur_none;
        }
        // Raster mode: dabs are stamped incrementally in ui(); just flag the
        // final flush + reset. No vector taper/commit needed — dab radius
        // already follows pressure, and opaque dabs don't get the tip blob.
        if self.raster {
            self.raster_stroke_done = true;
            return;
        }
        if self.current.len() < 2 {
            self.current.clear();
            return;
        }
        // Step 6: ramp the trailing points' pressure to 0 (smoothstep) so the
        // stroke narrows to a clean tip. Our platform never sends the decaying
        // Move samples Krita gets from the driver, so we produce the taper here.
        let n = END_TAPER_POINTS.min(self.current.len());
        let len = self.current.len();
        for k in 0..n {
            let idx = len - 1 - k;
            // k=0 (endpoint) -> 0; k=n-1 -> ~1 (unchanged). smoothstep for a
            // soft narrowing rather than a linear wedge.
            let t = if n > 1 { k as f32 / (n - 1) as f32 } else { 0.0 };
            let factor = t * t * (3.0 - 2.0 * t);
            self.current[idx].pressure *= factor;
        }
        let stroke = Stroke {
            points: std::mem::take(&mut self.current),
            base_width: self.brush_width,
            color: self.brush_color,
        };
        state.commit_stroke(stroke);
    }
}

/// Median of three values (branch-light).
fn median3(v: [f32; 3]) -> f32 {
    v[0].max(v[1]).min(v[0].min(v[1]).max(v[2]))
}

/// sRGB u8 swatch -> straight linear f32 RGBA (the dab shader premultiplies).
fn linear_rgba(c: [u8; 4]) -> [f32; 4] {
    let lin = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    [lin(c[0]), lin(c[1]), lin(c[2]), c[3] as f32 / 255.0]
}

/// Previous/next distinct drawings on a column, with their ghost tints.
/// Classic onion convention: red = before, green = after, fading with depth.
fn onion_ghosts(
    state: &AppState,
    column: ColumnId,
    frame: u32,
) -> Vec<(anim_core::ids::DrawingId, Color32)> {
    let current = state.resolve_at(column, frame);
    let mut out = Vec::new();

    let mut collect = |range: Box<dyn Iterator<Item = u32>>, base: (u8, u8, u8)| {
        let mut found: Vec<anim_core::ids::DrawingId> = Vec::new();
        for f in range {
            if let Some(id) = state.resolve_at(column, f)
                && Some(id) != current && !found.contains(&id) {
                    found.push(id);
                    if found.len() == 2 {
                        break;
                    }
                }
        }
        for (depth, id) in found.into_iter().enumerate() {
            let alpha = if depth == 0 { 100 } else { 55 };
            out.push((
                id,
                Color32::from_rgba_unmultiplied(base.0, base.1, base.2, alpha),
            ));
        }
    };

    collect(Box::new((0..frame).rev()), (215, 70, 70));
    collect(
        Box::new(frame + 1..state.frame_count()),
        (70, 175, 90),
    );
    out
}

fn draw_strokes(
    painter: &egui::Painter,
    strokes: &[Stroke],
    to_screen: &impl Fn(Pos2) -> Pos2,
    scale: f32,
    tint: Option<Color32>,
    curve: &PressureCurve,
) {
    for stroke in strokes {
        let c = stroke.color;
        let color =
            tint.unwrap_or_else(|| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]));
        fill_stroke(
            painter,
            &stroke.points,
            stroke.base_width,
            scale,
            color,
            to_screen,
            curve,
        );
    }
}

/// Render a stroke as a variable-width filled ribbon — Krita's pressure→size
/// model expressed as vector geometry. Each vertex has half-width
/// `base * pressure * scale / 2`; between consecutive vertices we fill a
/// trapezoid that interpolates the two half-widths, and a round dab at each
/// vertex fills the joints. Because width follows per-vertex pressure, the full
/// pressure range shows as continuous thick↔thin — not a uniform "solid" line —
/// and the pressure-0 taper tip narrows to nothing.
fn fill_stroke(
    painter: &egui::Painter,
    points: &[StrokePoint],
    base_width: f32,
    scale: f32,
    color: Color32,
    to_screen: &impl Fn(Pos2) -> Pos2,
    curve: &PressureCurve,
) {
    if points.is_empty() {
        return;
    }
    let half = |pr: f32| (base_width * curve.apply(pr) * scale * 0.5).max(WIDTH_FLOOR);

    // Single point (or a tap): one dab.
    if points.len() == 1 {
        let c = to_screen(pos2(points[0].x, points[0].y));
        painter.circle_filled(c, half(points[0].pressure), color);
        return;
    }

    // Round the starting cap.
    let start = to_screen(pos2(points[0].x, points[0].y));
    painter.circle_filled(start, half(points[0].pressure), color);

    for pair in points.windows(2) {
        let a = to_screen(pos2(pair[0].x, pair[0].y));
        let b = to_screen(pos2(pair[1].x, pair[1].y));
        let ha = half(pair[0].pressure);
        let hb = half(pair[1].pressure);
        let d = b - a;
        let len = d.length();
        if len > 0.001 {
            // Perpendicular offsets → a trapezoid whose width tracks pressure.
            let n = vec2(-d.y, d.x) / len;
            let quad = vec![a + n * ha, b + n * hb, b - n * hb, a - n * ha];
            painter.add(egui::Shape::convex_polygon(
                quad,
                color,
                egui::Stroke::NONE,
            ));
        }
        // Round dab at the far vertex smooths the joint to the next segment.
        painter.circle_filled(b, hb, color);
    }
}
