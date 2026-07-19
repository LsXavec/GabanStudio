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

use crate::config::{LayersConfig, PenConfig, PressureCurve};
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
/// None/0). Overwritten by the first real Move sample. Kept at ~0 so a feather
/// tap with NO real pressure sample makes essentially nothing (not a fat blob);
/// a deliberate dot comes from the pressure you actually apply.
const START_SEED: f32 = 0.0;
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
/// Pressure fabricated for the mouse fallback (no tablet force available). Kept
/// low so a mouse stroke is a thin line, not a fat flat-pressure ribbon, and a
/// stray synthesized pen→mouse click can't stamp a big disc.
const MOUSE_PRESSURE: f32 = 0.15;
/// Absolute radius cap (paper px) for a dab in a stroke that never saw real
/// pressure (mouse-mode / a tap with no force). Keeps a big-brush mouse tap from
/// blobbing a huge disc even when pressure is unavailable.
const NO_FORCE_DAB_MAX_PX: f32 = 6.0;
/// A mouse "stroke" whose committed path is shorter than this (screen px) is a
/// stationary click, not a drag — discard it so a bare click paints nothing.
const MOUSE_MIN_TRAVEL_PX: f32 = 3.0;

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
    /// Eraser tool active: strokes subtract coverage (destination-out) instead of
    /// laying down ink. Same dab geometry, size and pressure response as the brush.
    erasing: bool,
    /// Per-dab strength (0–1): scales each dab's alpha, so overlapping dabs build
    /// up within a stroke (airbrush-like). NOT whole-stroke opacity — that needs
    /// the wet-buffer (paints the stroke separately, then composites once).
    brush_flow: f32,
    /// Whole-stroke opacity (0–1): the live stroke paints into the wet buffer and
    /// is merged onto the cel ONCE at this level at pen-up, so a stroke can never
    /// build past it no matter how much it overlaps itself (Krita's opacity).
    brush_opacity: f32,
    /// The wet buffer holds un-committed dabs (brush stroke in progress, or an
    /// abandoned stroke that still needs clearing).
    wet_dirty: bool,
    /// Tool latched at stroke START — a mid-stroke eraser toggle must not split
    /// one stroke across the wet buffer and the cel (wrong ordering at composite).
    stroke_erasing: bool,
    /// Layer slot latched at stroke START — a mid-stroke A-cycle must not
    /// retarget the pen-up commit (it would bake the OLD layer's readback into
    /// the NEW layer: silent cross-layer corruption).
    stroke_layer_slot: usize,
    // --- Brush dynamics (what pen pressure drives). Tilt dynamics are gated on
    // the octotablet backend — egui pen events carry no tilt.
    /// Pressure drives dab size (through the pen curve).
    dyn_size: bool,
    /// Pressure drives dab opacity (through the pen curve) — shading-pen feel.
    dyn_opacity: bool,
    /// Size floor as a fraction of the brush size: pressure maps size between
    /// `min_size×brush` and `brush`, so light touches on a big brush draw a
    /// thin-but-visible line instead of vanishing (Krita's minimum-size).
    min_size: f32,
    /// This stroke wrote the CEL texture directly (eraser dabs, or the new-cel
    /// clear). If it is abandoned, the cel must re-sync from engine truth —
    /// otherwise phantom damage gets baked into the NEXT commit.
    cel_touched: bool,
    /// How many of the current stroke's dabs are already on the GPU layer.
    dabs_flushed: usize,
    /// The stroke finished this frame; flush its last dabs, then reset.
    raster_stroke_done: bool,
    /// (drawing, layer, content hash) currently uploaded to the GPU ACTIVE
    /// texture — when it no longer matches, re-sync from the engine.
    synced_active: (u64, u64, u64),
    /// Content keys of the below/above sandwich projections (None = must
    /// rebuild). Pen-up commits touch only `synced_active`; these change on
    /// frame/layer switches, visibility/opacity edits, reorders, undo.
    synced_below: Option<u64>,
    synced_above: Option<u64>,
    /// This stroke will create a NEW cel (the frame had no own key) — clear the
    /// GPU layer at the first dab so the new cel is blank, not a copy of the
    /// held drawing that was on display.
    raster_new_cel: bool,
    /// Pressure response curve (from Pen/Tablet settings); remaps pressure to
    /// width at render time. Stored pressure stays raw.
    pen_curve: PressureCurve,
    /// Active layer name last frame — detects layer switches so the brush
    /// colour follows the layer.
    last_layer_name: String,
    /// Session colour memory per layer NAME: picking a colour while a layer is
    /// active remembers it here (overrides the Settings default until close).
    layer_colors: std::collections::HashMap<String, [u8; 4]>,
    /// The canvas area rect of the last frame — read by the headless layout
    /// probe (ANIMSTUDIO_PROBE) to detect ANY movement of the drawing area.
    pub dbg_rect: Rect,
    /// Set once a real pressure pen (Touch with force > 0) is seen. After that,
    /// the mouse-drawing fallback is disabled so Windows' synthesized pen→mouse
    /// clicks (e.g. from a fast light double-tap) can't paint flat-pressure blobs.
    seen_pen: bool,
    /// The current stroke was started by the mouse fallback (no pen stream), so
    /// its pressure is fabricated, not measured.
    stroke_from_mouse: bool,
    /// The last committed stroke had no real pressure (mouse-mode: the pen is
    /// arriving as plain mouse events, or Windows Ink is off). Drives the red
    /// "MOUSE — no pen pressure" badge so the failure is visible, not silent.
    dbg_mouse_mode: bool,
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
            erasing: false,
            brush_flow: 1.0,
            brush_opacity: 1.0,
            wet_dirty: false,
            stroke_erasing: false,
            stroke_layer_slot: 0,
            dyn_size: true,
            dyn_opacity: false,
            min_size: 0.0,
            cel_touched: false,
            dabs_flushed: 0,
            raster_stroke_done: false,
            synced_active: (u64::MAX, u64::MAX, u64::MAX), // force initial sync
            synced_below: None,
            synced_above: None,
            raster_new_cel: false,
            pen_curve: PressureCurve::linear(),
            last_layer_name: String::new(),
            layer_colors: std::collections::HashMap::new(),
            dbg_rect: Rect::NOTHING,
            seen_pen: false,
            stroke_from_mouse: false,
            dbg_mouse_mode: false,
        }
    }

    /// Flip between brush and eraser (bound to a rebindable shortcut).
    pub fn toggle_eraser(&mut self) {
        self.erasing = !self.erasing;
    }

    /// A stroke is live (pen down, or its pen-up commit hasn't run yet).
    /// Stroke-unsafe actions (frame nav, undo, layer cycle, clears) must not
    /// dispatch while this holds — they would retarget or orphan the commit.
    pub fn stroke_active(&self) -> bool {
        self.touch_active || !self.current.is_empty() || self.raster_stroke_done
    }

    /// Fixed-width pressure diagnostic for the status bar:
    /// (text, pressure-range-is-healthy, pen-arriving-as-mouse).
    pub fn pressure_diag(&self) -> (String, bool, bool) {
        let total = (self.dbg_some + self.dbg_none).max(1);
        let pct = 100 * self.dbg_some / total;
        (
            format!(
                "P{:5.2}  {:4.2}–{:4.2}  {:3}%",
                self.dbg_pressure, self.dbg_min, self.dbg_max, pct
            ),
            self.dbg_max - self.dbg_min > 0.15,
            self.dbg_mouse_mode,
        )
    }

    /// Brush & tool controls — a dockable pane of its own (the old canvas
    /// toolbar). Wrapped layout: in a narrow dock it flows to more rows
    /// without pushing any other pane around.
    pub fn brush_ui(&mut self, ui: &mut egui::Ui, state: &mut AppState, raster_available: bool) {
        // Sliders are the widest fixed-size widgets — scale them to the pane
        // so the toolbox keeps collapsing in narrow docks instead of hitting
        // a ~180px floor at the dock divider.
        let sw = (ui.available_width() * 0.45).clamp(48.0, 110.0);
        ui.spacing_mut().slider_width = sw;
        ui.horizontal_wrapped(|ui| {
            if raster_available {
                ui.checkbox(&mut self.raster, "raster")
                    .on_hover_text("GPU raster brush");
                ui.separator();
            } else {
                self.raster = false;
            }
            if self.raster {
                if ui
                    .selectable_label(!self.erasing, "✏ brush")
                    .on_hover_text("paint ink")
                    .clicked()
                {
                    self.erasing = false;
                }
                if ui
                    .selectable_label(self.erasing, "▱ eraser")
                    .on_hover_text("erase to transparency")
                    .clicked()
                {
                    self.erasing = true;
                }
                // Active cel-layer chip (RETAS trace-line colours). Click or A
                // cycles; strokes land on this layer. Red = hidden (painting
                // is refused until it's shown or switched). FIXED WIDTH:
                // monospace + padded name, so switching layers never reflows
                // the toolbar items after it (canvas-stability law).
                let lname = state.active_layer_name();
                let hidden = state.active_layer_props().is_some_and(|p| !p.visible);
                let shown: String = lname.chars().take(10).collect();
                let color = if hidden {
                    Color32::from_rgb(235, 90, 80)
                } else {
                    layer_chip_color(&lname)
                };
                if ui
                    .button(
                        egui::RichText::new(format!("▣ {shown:<10}"))
                            .monospace()
                            .color(color),
                    )
                    .on_hover_text(if hidden {
                        "active layer is HIDDEN — painting refused (A cycles, or show its eye)"
                    } else {
                        "active layer — strokes land here (A cycles)"
                    })
                    .clicked()
                {
                    state.cycle_layer(false);
                }
                ui.add(
                    egui::Slider::new(&mut self.raster_brush_px, 1.0..=300.0)
                        .text("px")
                        .fixed_decimals(0),
                );
                ui.add(
                    egui::Slider::new(&mut self.brush_flow, 0.05..=1.0)
                        .text("flow")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut self.brush_opacity, 0.05..=1.0)
                        .text("opacity")
                        .fixed_decimals(2),
                );
                ui.menu_button("dynamics", |ui| {
                    ui.checkbox(&mut self.dyn_size, "pressure → size");
                    ui.checkbox(&mut self.dyn_opacity, "pressure → opacity");
                    ui.add(
                        egui::Slider::new(&mut self.min_size, 0.0..=1.0)
                            .text("min size")
                            .fixed_decimals(2),
                    )
                    .on_hover_text(
                        "size at zero pressure, as a fraction of the brush — \
                         keeps light touches visible on big brushes",
                    );
                    ui.label(
                        egui::RichText::new("tilt: needs the tablet backend (planned)")
                            .weak()
                            .small(),
                    );
                });
                if ui
                    .button("clear cel")
                    .on_hover_text("clear this cel's raster (undoable)")
                    .clicked()
                {
                    state.clear_current_raster();
                }
            } else {
                ui.add(
                    egui::Slider::new(&mut self.brush_width, 0.5..=16.0)
                        .text("brush")
                        .fixed_decimals(1),
                );
            }
            ui.separator();
            // Custom colour picker (any RGB); the swatches beside it are presets.
            let mut rgb = [self.brush_color[0], self.brush_color[1], self.brush_color[2]];
            if ui
                .color_edit_button_srgb(&mut rgb)
                .on_hover_text("brush colour")
                .changed()
            {
                self.brush_color = [rgb[0], rgb[1], rgb[2], self.brush_color[3]];
            }
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
            // NOTHING VARIABLE-WIDTH GOES IN THIS ROW. The toolbar used to
            // carry live diagnostics (pressure readout, onion/cel probes, the
            // PLAYING label) whose changing text reflowed everything after it
            // and read as "the workspace shifting". Diagnostics now live in
            // the status bar (fixed-width) and PLAYING is a canvas overlay.
        });
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        paint: Option<&mut PaintLayer>,
        pen: &PenConfig,
        layers_cfg: &LayersConfig,
    ) {
        self.pen_curve = pen.pressure_curve.clone();
        // Brush colour follows the active layer: on a switch, remember the
        // colour picked while the previous layer was active, then load the new
        // layer's colour (session pick, else the Settings default).
        let layer_name = state.active_layer_name();
        if layer_name != self.last_layer_name {
            if !self.last_layer_name.is_empty() {
                self.layer_colors
                    .insert(self.last_layer_name.clone(), self.brush_color);
            }
            if let Some(c) = self
                .layer_colors
                .get(&layer_name)
                .or_else(|| layers_cfg.colors.get(&layer_name))
            {
                self.brush_color = *c;
            }
            self.last_layer_name = layer_name;
        }
        let mut paint = paint;
        if paint.is_none() {
            self.raster = false;
        }

        // ---- Canvas area --------------------------------------------------
        let rect = ui.available_rect_before_wrap();
        self.dbg_rect = rect;
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

        // Zoom at cursor / middle-drag pan — DISABLED during playback: the pen
        // resting or hovering on the drawing display (hover scroll deltas, a
        // barrel button mapped to middle-click) must not move the view while
        // the animation plays.
        if !state.playing {
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll.abs() > 0.0
                    && let Some(mouse) = response.hover_pos() {
                        let before = to_paper(mouse);
                        self.zoom = (self.zoom * (scroll * 0.0015).exp()).clamp(0.2, 10.0);
                        let scale2 = fit * self.zoom;
                        let origin2 =
                            rect.center() - vec2(paper_w, paper_h) * scale2 * 0.5 + self.pan;
                        let after = origin2 + before.to_vec2() * scale2;
                        self.pan += mouse - after;
                    }
            }
            // Middle-drag pans.
            if response.dragged_by(egui::PointerButton::Middle) {
                self.pan += response.drag_delta();
            }
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
                // An abandoned stroke (e.g. playback started mid-stroke) may have
                // left dabs in the wet buffer — drop them, they were never
                // committed.
                if self.wet_dirty {
                    p.clear_wet();
                    self.wet_dirty = false;
                }
                // If the abandoned stroke wrote textures directly (eraser dabs,
                // new-cel clear), they no longer match engine truth —
                // invalidate every sync key so this block restores them.
                if self.cel_touched {
                    self.synced_active = (u64::MAX, u64::MAX, u64::MAX);
                    self.synced_below = None;
                    self.synced_above = None;
                    self.cel_touched = false;
                }
                // Between strokes: re-sync whatever changed (frame switch,
                // layer switch, undo/redo, visibility/opacity edits, reorder).
                // Three keys, one mechanism; the blank-frame sentinel is folded
                // into ALL of them (stale-pixel bug class).
                let key = state.active_layer_key();
                if key != self.synced_active {
                    match state.active_layer_tiles() {
                        Some(tiles) => p.sync_active(tiles),
                        None => p.clear_active(),
                    }
                    self.synced_active = key;
                }
                let bk = state.below_stack_key();
                if Some(bk) != self.synced_below {
                    p.build_projection(false, &state.below_layers());
                    self.synced_below = Some(bk);
                }
                let ak = state.above_stack_key();
                if Some(ak) != self.synced_above {
                    p.build_projection(true, &state.above_layers());
                    self.synced_above = Some(ak);
                }
            } else {
                // Starting a new cel: wipe the held drawing's textures so the
                // new cel starts blank — active AND both projections (nothing
                // is below or above a fresh cel yet), all keys invalidated.
                if self.raster_new_cel && self.dabs_flushed == 0 {
                    p.clear_active();
                    p.clear_projections();
                    self.synced_below = None;
                    self.synced_above = None;
                    self.cel_touched = true;
                }
                // Drawing (or finishing this frame): stamp the new dabs. Brush
                // dabs go to the WET buffer (composited at opacity at pen-up);
                // the eraser works on the cel directly. Tool is LATCHED at
                // stroke start so a mid-stroke toggle can't split the stroke.
                let dabs = self.build_stroke_dabs();
                if dabs.len() > self.dabs_flushed {
                    if self.stroke_erasing {
                        p.paint(&dabs[self.dabs_flushed..], true);
                        self.cel_touched = true;
                    } else {
                        p.paint_wet(&dabs[self.dabs_flushed..]);
                        self.wet_dirty = true;
                    }
                    self.dabs_flushed = dabs.len();
                }
                if self.raster_stroke_done {
                    // Merge the whole wet stroke onto the cel at the chosen
                    // opacity (single blend — can't build past the ceiling),
                    // then read back and commit as a PaintTiles edit.
                    if self.wet_dirty {
                        p.composite_wet(self.brush_opacity);
                        self.wet_dirty = false;
                    }
                    let tiles = p.read_tiles();
                    // Commit against the slot LATCHED at stroke start — a
                    // mid-stroke A-cycle must not retarget the readback.
                    if let Some((id, layer)) = state.commit_raster(tiles, self.stroke_layer_slot) {
                        let h = state
                            .cut()
                            .drawing(id)
                            .and_then(|d| d.layer(layer))
                            .map(|l| l.content_hash())
                            .unwrap_or(0);
                        // Pen-up touches ONLY the active key: the projections
                        // exclude the active layer, so their stacks are
                        // unchanged by this commit.
                        self.synced_active = (id.0, layer.0, h);
                    } else {
                        // Commit refused: the GPU layer holds a stroke the
                        // engine never accepted — invalidate so the next frame
                        // restores truth, not a phantom.
                        self.synced_active = (u64::MAX, u64::MAX, u64::MAX);
                    }
                    self.cel_touched = false;
                    self.current.clear();
                    self.dabs_flushed = 0;
                    self.raster_stroke_done = false;
                }
            }

            // ---- Raster onion: upload the neighbour cels into ghost slots.
            // Refresh EVERY frame (set_onion is a no-op when the content hash is
            // unchanged, so this is cheap) — decoupled from stroke state entirely,
            // so no current-cel/stroke predicate can ever tear a slot down. Only
            // onion-off clears the slots.
            if state.onion {
                let neighbors = state.onion_neighbors();
                for (slot, nid) in neighbors.iter().enumerate() {
                    match nid.and_then(|id| state.drawing_composite(id)) {
                        Some((slices, hash)) => p.set_onion(slot, Some(&slices), hash),
                        None => p.set_onion(slot, None, 0),
                    }
                }
            } else {
                p.set_onion(0, None, 0);
                p.set_onion(1, None, 0);
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

        // 3b. The raster sandwich, bottom→top: onion ghosts, BELOW projection,
        //     the ACTIVE layer at its own opacity, the live wet stroke, the
        //     ABOVE projection. Painting "color" previews under the line art
        //     live — the solo shiage workflow.
        if self.raster
            && let Some(p) = paint {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                // Previous cel tinted warm, next cel tinted cool, both faded.
                if let Some(id) = p.onion_id(0) {
                    painter.image(id, paper_rect, uv, Color32::from_rgba_unmultiplied(255, 180, 180, 110));
                }
                if let Some(id) = p.onion_id(1) {
                    painter.image(id, paper_rect, uv, Color32::from_rgba_unmultiplied(180, 235, 190, 110));
                }
                // Layers under the active one (per-layer opacities baked in).
                painter.image(p.below_id(), paper_rect, uv, Color32::WHITE);
                // Active layer: the texture holds FULL-strength pixels (so the
                // pen-up readback stays bit-exact); its layer opacity is applied
                // here as a display tint. from_white_alpha is an exact o×texel
                // multiply under egui-wgpu 0.35's raw native-texture sampling —
                // re-check if an egui upgrade changes egui.wgsl.
                let (a_visible, a_opacity) = state
                    .active_layer_props()
                    .map(|pr| (pr.visible, pr.opacity))
                    .unwrap_or((true, 1.0));
                if a_visible {
                    let t = (a_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
                    painter.image(p.texture_id(), paper_rect, uv, Color32::from_white_alpha(t));
                    // Live brush stroke over it, at stroke × layer opacity
                    // (both factors, or the stroke pops at pen-up).
                    // KNOWN latent deviation: where the stroke overlaps existing
                    // active-layer ink AND the layer opacity < 1, this two-quad
                    // preview attenuates the old ink slightly differently than
                    // the merged commit (exact at opacity 1 — always true until
                    // the Phase 3 opacity UI ships; revisit then with a merged
                    // preview pass through scratch).
                    if self.wet_dirty {
                        let wa = (self.brush_opacity.clamp(0.0, 1.0)
                            * a_opacity.clamp(0.0, 1.0)
                            * 255.0)
                            .round() as u8;
                        painter.image(p.wet_id(), paper_rect, uv, Color32::from_white_alpha(wa));
                    }
                }
                // Layers over the active one.
                painter.image(p.above_id(), paper_rect, uv, Color32::WHITE);
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

        // Playback indicator as a canvas OVERLAY (painted, zero layout impact
        // — a toolbar label here used to reflow the row and shift the view).
        if state.playing {
            painter.text(
                pos2(rect.center().x, rect.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "PLAYING — space to stop",
                egui::FontId::proportional(13.0),
                Color32::from_rgb(120, 220, 140),
            );
        }

        // ---- Brush-outline cursor (Krita-style) --------------------------
        // Over the canvas in raster mode, hide the OS cursor and draw the brush's
        // exact painted footprint, following the pen even mid-stroke. Gated on the
        // pointer's TOPMOST egui layer being the canvas itself — a floating menu,
        // popup or window over the canvas gets the normal OS cursor back (same
        // occlusion test egui's own hover uses). Black+white concentric rings stay
        // visible on any background; the eraser adds a centre dot.
        // (Future textured/mask brushes: derive this preview from the brush's
        // footprint/stamp instead of a plain circle.)
        if self.raster
            && !state.playing
            && let Some(pos) = ui.input(|i| i.pointer.latest_pos())
            && rect.contains(pos)
            && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            // Exact size: what a FULL-pressure dab paints (the falloff reaches
            // zero exactly at its radius), including the dynamics mapping —
            // min-size floor and the pressure→size toggle — in screen space.
            // Floor only for visibility once a brush is sub-3px on screen.
            let t_max = if self.dyn_size { self.pen_curve.apply(1.0) } else { 1.0 };
            let min_s = self.min_size.clamp(0.0, 1.0);
            let r = (self.raster_brush_px * (min_s + (1.0 - min_s) * t_max) * scale * 0.5)
                .max(1.5);
            painter.circle_stroke(pos, r, egui::Stroke::new(1.0, Color32::from_black_alpha(170)));
            painter.circle_stroke(
                pos,
                (r - 1.0).max(0.5),
                egui::Stroke::new(1.0, Color32::from_white_alpha(170)),
            );
            if self.erasing {
                painter.circle_filled(pos, 1.3, Color32::from_black_alpha(180));
            }
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
        let base = linear_rgba(self.brush_color);
        let flow = self.brush_flow.clamp(0.0, 1.0);
        let hardness = 0.85;
        // When no real pressure reached this stroke (mouse-mode, or a tap with no
        // force), cap the dab so a big brush can't stamp a huge flat-pressure disc.
        let cap = if self.stroke_from_mouse || self.cur_some == 0 {
            NO_FORCE_DAB_MAX_PX
        } else {
            f32::INFINITY
        };
        // Dynamics: pressure (through the pen curve) drives size and/or opacity.
        // Size maps between min_size×brush and brush (the floor keeps light
        // touches visible); opacity scales the dab's alpha on top of flow.
        let min_s = self.min_size.clamp(0.0, 1.0);
        let radius_of = |pr: f32| {
            let t = if self.dyn_size { self.pen_curve.apply(pr) } else { 1.0 };
            (self.raster_brush_px * (min_s + (1.0 - min_s) * t) * 0.5)
                .max(0.5)
                .min(cap)
        };
        let dab_at = |x: f32, y: f32, pr: f32| {
            let a = if self.dyn_opacity { self.pen_curve.apply(pr) } else { 1.0 };
            let mut color = base;
            color[3] *= flow * a;
            Dab { center: [x, y], radius: radius_of(pr), hardness, color }
        };

        if pts.len() == 1 {
            dabs.push(dab_at(pts[0].x, pts[0].y, pts[0].pressure));
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
                let d2 = dab_at(ax + dx * t, ay + dy * t, pr);
                let r = d2.radius;
                dabs.push(d2);
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
                self.seen_pen = true; // a pen/stylus is driving input — not a mouse
                match phase {
                    // Step 2: distrust the Start force (WM_POINTERDOWN pressure
                    // is usually 0 -> None). Seed provisionally; the first real
                    // Move overwrites it so strokes don't begin with a fat dot.
                    egui::TouchPhase::Start => {
                        if !rect.contains(*pos) {
                            continue;
                        }
                        // GUARD (CSP behavior): never paint into a layer you
                        // can't see — refuse with a hint instead.
                        if self.raster
                            && state.active_layer_props().is_some_and(|p| !p.visible)
                        {
                            state.status = format!(
                                "layer '{}' is hidden — press A to switch or click its eye",
                                state.active_layer_name()
                            );
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
                        self.stroke_from_mouse = false;
                        self.stroke_erasing = self.erasing;
                        self.stroke_layer_slot = state.active_layer_slot;
                        self.cel_touched = false;
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
        // not during the post-lift lockout that suppresses egui's synthesized
        // primary-drag for the pen that just finished, and never once a real
        // pressure pen has been seen this session (Windows synthesizes pen→mouse
        // clicks that would otherwise paint flat-pressure blobs on light taps).
        if !self.touch_active && !touch_seen && self.mouse_lockout == 0 && !self.seen_pen {
            if response.drag_started_by(egui::PointerButton::Primary) {
                if self.raster && state.active_layer_props().is_some_and(|p| !p.visible) {
                    state.status = format!(
                        "layer '{}' is hidden — press A to switch or click its eye",
                        state.active_layer_name()
                    );
                    return;
                }
                self.current.clear();
                self.cur_some = 0; // no tablet force will arrive; keep the
                self.cur_none = 0; // force% diagnostic honest for this stroke
                self.stroke_from_mouse = true;
                self.stroke_erasing = self.erasing;
                self.stroke_layer_slot = state.active_layer_slot;
                self.cel_touched = false;
                self.dabs_flushed = 0;
                self.raster_stroke_done = false;
                self.raster_new_cel = self.raster && state.own_key_drawing().is_none();
                if let Some(p) = response.interact_pointer_pos() {
                    let p = to_paper(p);
                    self.current.push(StrokePoint {
                        x: p.x,
                        y: p.y,
                        pressure: MOUSE_PRESSURE,
                    });
                }
            } else if response.dragged_by(egui::PointerButton::Primary) {
                if let Some(p) = response.interact_pointer_pos() {
                    let p = to_paper(p);
                    self.current.push(StrokePoint {
                        x: p.x,
                        y: p.y,
                        pressure: MOUSE_PRESSURE,
                    });
                }
            } else if response.drag_stopped_by(egui::PointerButton::Primary)
                && !self.current.is_empty()
            {
                // Discard a stationary click (no real drag) so a bare mouse click
                // paints nothing; only commit an actual dragged stroke. Mouse has
                // no pressure, so no taper is synthesized here.
                if self.mouse_path_len_px(scale) >= MOUSE_MIN_TRAVEL_PX {
                    self.finish_stroke(state);
                } else {
                    self.current.clear();
                }
            }
        }
    }

    /// Total committed path length of the current stroke, in screen pixels.
    fn mouse_path_len_px(&self, scale: f32) -> f32 {
        let mut len = 0.0;
        for w in self.current.windows(2) {
            len += ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt() * scale;
        }
        len
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
        // Mouse-mode = this stroke carried no real tablet force: either the mouse
        // fallback drew it, or a Touch stream that DID move (cur_none Move samples)
        // never once reported force. The `cur_none > 0` guard avoids falsely
        // flagging a legitimate quick pen tap (Start+End, no Move sample at all).
        // Drives the red MOUSE badge that tells the user to fix the driver.
        self.dbg_mouse_mode =
            self.stroke_from_mouse || (self.cur_none > 0 && self.cur_some == 0);
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

/// RETAS-convention trace-line colour for a layer name (orientation aid):
/// line = near-white ink, color = green, shadow = blue, highlight = red,
/// correction = orange, rough = the blue-pencil convention.
pub fn layer_chip_color(name: &str) -> Color32 {
    match name {
        "line" => Color32::from_gray(230),
        "color" => Color32::from_rgb(120, 200, 120),
        "shadow" => Color32::from_rgb(110, 160, 240),
        "highlight" => Color32::from_rgb(240, 120, 120),
        "correction" => Color32::from_rgb(240, 170, 90),
        "rough" => Color32::from_rgb(130, 180, 255),
        _ => Color32::from_gray(180),
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
