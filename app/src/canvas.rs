//! The drawing canvas: a fixed-size logical "paper" (resolution independent)
//! rendered into the panel with pan/zoom. Pen strokes land in paper
//! coordinates so artwork never depends on window size or zoom level.
//!
//! Input reuses the M0-validated path: Windows Ink pen arrives as egui Touch
//! events with real pressure; mouse is the no-pressure fallback.

use std::collections::BTreeMap;
use std::sync::Arc;

use anim_core::edit::{self, FloatingPatch};
use anim_core::ids::{ColumnId, DrawingId, LayerId};
use anim_core::model::{Stroke, StrokePoint};
use anim_core::raster::{TileCoord, TileData};
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

/// One native tablet sample (octotablet / Windows Ink), already mapped to
/// egui window points.
#[derive(Clone, Copy, Debug)]
pub struct PenSample {
    pub pos: Pos2,
    pub pressure: Option<f32>,
    /// Pen tilt from vertical in radians, [x, y] (octotablet convention:
    /// [+,+] = right + toward the user). None = the packet carried no tilt.
    pub tilt: Option<[f32; 2]>,
    pub phase: PenPhase,
}

/// Tilt magnitude that counts as "fully flat" for the dynamics mapping —
/// the XP-Pen Artist line (and most pens) report up to ±60° from vertical.
const TILT_MAX_RAD: f32 = std::f32::consts::PI / 3.0;

/// EMA factor for tilt smoothing. Tilt packets are far steadier than
/// pressure, so a light touch is enough — just kills single-packet steps.
const TILT_SMOOTH: f32 = 0.35;

/// How far tilt→shape flattens the dab: major/minor ratio grows to
/// 1 + GAIN·strength·tilt_norm (2.5× at full strength + a flat pen) — the
/// stamp visibly turns with the pen, like a Krita tip mask following tilt.
const TILT_ASPECT_GAIN: f32 = 1.5;

/// Tilt magnitude (normalized) below which the cursor needle hides — a
/// near-vertical pen has no meaningful direction to point.
const TILT_NEEDLE_MIN: f32 = 0.05;

/// The canvas's active TOOL — the abstraction workspaces will restore per
/// stage (LENS-DOCK: workspace = layout + tool/mode). `Paint` keeps the
/// existing brush/eraser pair (which sub-tool via `erasing`); `Select` is
/// the select/move/scale/rotate tool; `Fill` is the ink-&-paint bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasTool {
    Paint,
    Select,
    Fill,
}

/// Selection drawing shape (a rect is a 4-point polygon through the same
/// lift path).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelShape {
    Rect,
    Lasso,
}

/// A lifted, in-flight transform: the selected pixels float above the layer
/// under a live affine until Enter/click-outside commits them as ONE
/// PaintTiles command (or Esc restores). The target (drawing, layer) is
/// LATCHED at lift — a frame switch mid-gesture auto-commits to the latched
/// target, never to whatever is now displayed.
struct Floating {
    patch: FloatingPatch,
    cleared: Vec<(TileCoord, Option<Arc<TileData>>)>,
    /// Engine-truth tiles at lift (merge base; also rebuilt the punched
    /// display).
    base_tiles: BTreeMap<TileCoord, Arc<TileData>>,
    tex: egui::TextureHandle,
    drawing: DrawingId,
    layer: LayerId,
    // Live gesture affine (paper space).
    pivot: Pos2,
    translate: egui::Vec2,
    rotate: f32,
    scale: f32,
    drag: Option<FloatDrag>,
}

impl Floating {
    fn affine(&self) -> edit::Affine {
        edit::Affine {
            pivot: (self.pivot.x, self.pivot.y),
            translate: (self.translate.x, self.translate.y),
            rotate_rad: self.rotate,
            scale: self.scale,
        }
    }

    /// The four transformed corners of the patch bbox, paper space,
    /// in (TL, TR, BL, BR) order.
    fn corners(&self) -> [Pos2; 4] {
        let a = self.affine();
        let (x0, y0) = (self.patch.x0 as f32, self.patch.y0 as f32);
        let (x1, y1) = (
            (self.patch.x0 + self.patch.w as i32) as f32,
            (self.patch.y0 + self.patch.h as i32) as f32,
        );
        let m = |x, y| {
            let (px, py) = a.apply((x, y));
            pos2(px, py)
        };
        [m(x0, y0), m(x1, y0), m(x0, y1), m(x1, y1)]
    }
}

enum FloatDrag {
    Move { start_ptr: Pos2, start_translate: egui::Vec2 },
    Scale { start_dist: f32, start_scale: f32 },
    Rotate { start_angle: f32, start_rotate: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenPhase {
    Down,
    Move,
    Up,
    /// Pen in proximity but not touching: carries live tilt (and position)
    /// so the cursor needle and T° readout move BEFORE the stroke starts.
    /// Never paints, and never latches the native-owns-the-pen dedupe.
    Hover,
}

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
    // --- Brush dynamics (what pen pressure/tilt drives). Tilt flows only from
    // the octotablet backend — egui pen events carry no tilt.
    /// Pressure drives dab size (through the pen curve).
    dyn_size: bool,
    /// Pressure drives dab opacity (through the pen curve) — shading-pen feel.
    dyn_opacity: bool,
    /// Size floor as a fraction of the brush size: pressure maps size between
    /// `min_size×brush` and `brush`, so light touches on a big brush draw a
    /// thin-but-visible line instead of vanishing (Krita's minimum-size).
    min_size: f32,
    /// Tilting the pen broadens the stroke (side-of-the-lead pencil feel).
    tilt_size: bool,
    /// Tilting the pen lightens the stroke.
    tilt_opacity: bool,
    /// Tilting the pen flattens + rotates the dab along the lean direction —
    /// the stamp itself shows the tilt, like Krita's tip masks.
    tilt_shape: bool,
    /// How strongly tilt drives the enabled dynamics (0..1): at 1.0 a flat
    /// pen doubles the dab size and drops its alpha to a quarter.
    tilt_strength: f32,
    /// EMA-smoothed live tilt (radians, [x, y]); sampled into each stroke
    /// point so already-flushed dabs never recompute differently.
    smoothed_tilt: [f32; 2],
    /// Last tilt the tablet reported — carried across strokes (a pen's tilt
    /// barely changes between lift and touch-down; Down packets carry none).
    last_tilt: [f32; 2],
    /// Live tilt magnitude in degrees, for the status-bar diagnostic.
    dbg_tilt: f32,
    /// A real tilt sample arrived this session (hover or stroke) — gates the
    /// T° readout and the cursor needle. One-way latch: fixed-width law.
    tilt_seen: bool,
    /// COMPOSITE VIEW: the canvas shows the node graph's rendered output (the
    /// playback/export truth) instead of the editing sandwich. Review mode —
    /// painting is refused while on (a stroke must never land somewhere the
    /// view doesn't show). Toggled by the C keybind / 🎬 toolbar button.
    pub composite_view: bool,
    /// Active canvas tool (Paint = brush/eraser pipeline, Select = the
    /// select/move/scale/rotate tool). Keybinds: B = paint, V = select.
    pub tool: CanvasTool,
    /// Selection drawing shape for the Select tool (toolbar toggle).
    sel_shape: SelShape,
    /// Fill: line-art gaps up to this many px act closed.
    fill_gap: u32,
    /// Fill: how far flats tuck UNDER the lines (kills the pale halo).
    fill_grow: u32,
    /// Fill reference: true = the whole cel's flatten bounds the fill (line
    /// art on any layer — the shiage default); false = active layer only.
    fill_ref_cel: bool,
    /// Committed selection outline, paper-space polygon (None = no selection).
    /// Survives frame nav — it's a region of PAPER, not of one cel.
    selection: Option<Vec<Pos2>>,
    /// Selection being drawn right now (rect = [anchor, current]).
    sel_draft: Option<Vec<Pos2>>,
    /// Lifted pixels mid-transform. See [`Floating`].
    floating: Option<Floating>,
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
    /// Latched when octotablet delivers native tablet samples: the native path
    /// owns the pen; egui Touch/mouse (duplicates of the same physical strokes)
    /// go quiet.
    native_active: bool,
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
            tilt_size: false,
            tilt_opacity: false,
            tilt_shape: false,
            tilt_strength: crate::config::default_tilt_strength(),
            smoothed_tilt: [0.0; 2],
            last_tilt: [0.0; 2],
            dbg_tilt: 0.0,
            tilt_seen: false,
            composite_view: false,
            tool: CanvasTool::Paint,
            sel_shape: SelShape::Lasso,
            fill_gap: 2,
            fill_grow: 2,
            fill_ref_cel: true,
            selection: None,
            sel_draft: None,
            floating: None,
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
            native_active: false,
            stroke_from_mouse: false,
            dbg_mouse_mode: false,
        }
    }

    /// Flip between brush and eraser (bound to a rebindable shortcut).
    pub fn toggle_eraser(&mut self) {
        self.erasing = !self.erasing;
    }

    /// Apply a brush preset wholesale (keybind 1–8, Presets pane, or a
    /// workspace switch bound to it). Also drops the eraser — a preset is ink.
    pub fn apply_preset(&mut self, p: &crate::config::BrushPreset) {
        self.raster_brush_px = p.size_px;
        self.brush_flow = p.flow;
        self.brush_opacity = p.opacity;
        self.dyn_size = p.dyn_size;
        self.dyn_opacity = p.dyn_opacity;
        self.min_size = p.min_size;
        self.tilt_size = p.tilt_size;
        self.tilt_opacity = p.tilt_opacity;
        self.tilt_shape = p.tilt_shape;
        self.tilt_strength = p.tilt_strength;
        if let Some(c) = p.color {
            self.brush_color = c;
        }
        self.erasing = false;
    }

    /// Snapshot the current brush as a preset (the Presets pane's "save").
    pub fn snapshot_preset(&self, name: String) -> crate::config::BrushPreset {
        crate::config::BrushPreset {
            name,
            size_px: self.raster_brush_px,
            flow: self.brush_flow,
            opacity: self.brush_opacity,
            dyn_size: self.dyn_size,
            dyn_opacity: self.dyn_opacity,
            min_size: self.min_size,
            color: Some(self.brush_color),
            tilt_size: self.tilt_size,
            tilt_opacity: self.tilt_opacity,
            tilt_shape: self.tilt_shape,
            tilt_strength: self.tilt_strength,
        }
    }

    /// An edit gesture is live: a stroke (pen down or its pen-up commit
    /// pending), a selection being drawn, or a lifted transform in flight.
    /// Gesture-unsafe actions (frame nav, undo, layer cycle, clears) must not
    /// dispatch while this holds — they would retarget or orphan the commit.
    pub fn stroke_active(&self) -> bool {
        self.touch_active
            || !self.current.is_empty()
            || self.raster_stroke_done
            || self.sel_draft.is_some()
            || self.floating.is_some()
    }

    /// Switch to the Select tool (V) / back to Paint (B). A live stroke
    /// blocks the switch (the stroke pipeline would be orphaned mid-flight);
    /// leaving Select commits any floating transform rather than losing it.
    pub fn set_tool(&mut self, tool: CanvasTool, state: &mut AppState) {
        if self.tool == tool {
            return;
        }
        if self.touch_active || !self.current.is_empty() || self.raster_stroke_done {
            return; // mid-stroke: refuse silently, same as other guards
        }
        // Leaving Select for ANY tool lands the floating transform.
        if self.tool == CanvasTool::Select && self.floating.is_some() {
            self.commit_floating(state);
        }
        self.sel_draft = None;
        self.tool = tool;
        state.status = match tool {
            CanvasTool::Select => {
                "select: drag = select, drag inside = move, corners = scale, \
                 just outside corners = rotate; Enter applies, Esc cancels"
                    .into()
            }
            CanvasTool::Fill => {
                "fill: click a region — line art bounds it; gap closes line \
                 breaks, under tucks the flat beneath the lines"
                    .into()
            }
            CanvasTool::Paint => "paint".into(),
        };
    }

    /// Ctrl+A: select the whole paper (switches to the Select tool).
    pub fn select_all(&mut self, state: &mut AppState, paper_w: f32, paper_h: f32) {
        self.set_tool(CanvasTool::Select, state);
        if self.tool != CanvasTool::Select {
            return; // switch refused mid-stroke
        }
        self.selection = Some(vec![
            pos2(0.0, 0.0),
            pos2(paper_w, 0.0),
            pos2(paper_w, paper_h),
            pos2(0.0, paper_h),
        ]);
        state.status = "all selected — drag inside to move".into();
    }

    /// Lift the current selection (or the whole layer if `poly` is None) off
    /// the ACTIVE layer into a floating transform. Guards mirror the stroke
    /// pipeline's: no composite view, no hidden layer, own-key frames only.
    fn try_lift(
        &mut self,
        state: &mut AppState,
        paint: &mut PaintLayer,
        ctx: &egui::Context,
        poly: Option<&[Pos2]>,
    ) -> bool {
        if self.composite_view {
            state.status = "composite view — press C to edit".into();
            return false;
        }
        if !self.raster {
            state.status = "the select tool needs the raster engine (🖌)".into();
            return false;
        }
        if state.active_layer_props().is_some_and(|p| !p.visible) {
            state.status = format!(
                "layer '{}' is hidden — press A to switch or click its eye",
                state.active_layer_name()
            );
            return false;
        }
        let Some(did) = state.own_key_drawing() else {
            state.status =
                "held/blank frame — a transform edits a frame's OWN cel (draw here first)"
                    .into();
            return false;
        };
        let (kd, kl, _) = state.active_layer_key();
        if kd != did.0 || kl == u64::MAX {
            state.status = "no raster layer here to transform".into();
            return false;
        }
        let Some(tiles) = state.active_layer_tiles().cloned() else {
            state.status = "no raster layer here to transform".into();
            return false;
        };
        let lift = match poly {
            Some(p) => {
                let pts: Vec<(f32, f32)> = p.iter().map(|q| (q.x, q.y)).collect();
                edit::lift_region(&tiles, &pts)
            }
            None => edit::lift_all(&tiles),
        };
        let Some(lift) = lift else {
            state.status = format!(
                "selection has no ink on layer '{}'",
                state.active_layer_name()
            );
            return false;
        };

        // Preview texture (premultiplied bytes — the raw-texel display law).
        let mut bytes = Vec::with_capacity(lift.patch.rgba.len());
        for v in &lift.patch.rgba {
            bytes.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        let img = egui::ColorImage::from_rgba_premultiplied(
            [lift.patch.w as usize, lift.patch.h as usize],
            &bytes,
        );
        let tex = ctx.load_texture("floating_selection", img, egui::TextureOptions::LINEAR);

        // Punch the lifted pixels out of the DISPLAYED active texture (engine
        // truth is untouched until commit; the sync key still matches, so the
        // between-strokes sync won't fight this).
        let mut punched = tiles.clone();
        for (c, after) in &lift.cleared {
            match after {
                Some(t) => punched.insert(*c, t.clone()),
                None => punched.remove(c),
            };
        }
        paint.sync_active(&punched);

        let pivot = pos2(
            lift.patch.x0 as f32 + lift.patch.w as f32 / 2.0,
            lift.patch.y0 as f32 + lift.patch.h as f32 / 2.0,
        );
        self.floating = Some(Floating {
            patch: lift.patch,
            cleared: lift.cleared,
            base_tiles: tiles,
            tex,
            drawing: did,
            layer: LayerId(kl),
            pivot,
            translate: vec2(0.0, 0.0),
            rotate: 0.0,
            scale: 1.0,
            drag: None,
        });
        self.selection = None; // absorbed into the floating transform
        true
    }

    /// Commit the floating transform: resample + merge → ONE PaintTiles
    /// command against the LATCHED target, then re-sync the display from
    /// engine truth. An identity gesture commits nothing (empty diff).
    fn commit_floating(&mut self, state: &mut AppState) {
        let Some(f) = self.floating.take() else {
            return;
        };
        let affine = f.affine();
        if !affine.is_identity() {
            let moved = edit::transform_patch(&f.patch, &affine);
            let diff = edit::merge_patch(&f.base_tiles, &f.cleared, &moved);
            if !diff.is_empty() {
                state.commit_region_edit("transform", f.drawing, f.layer, diff);
            }
        }
        // Either way the displayed texture was punched — restore from truth.
        self.synced_active = (u64::MAX, u64::MAX, u64::MAX);
    }

    /// Drop the floating transform and restore the display (engine truth was
    /// never touched).
    fn cancel_floating(&mut self) {
        if self.floating.take().is_some() {
            self.synced_active = (u64::MAX, u64::MAX, u64::MAX);
        }
    }

    /// Land any in-flight gesture NOW (commit the floating transform, drop a
    /// half-drawn selection outline). Call before anything that would pull
    /// the document out from under the gesture — Save (the file must contain
    /// what the screen shows), Open (the commit belongs to the CURRENT
    /// project), New.
    pub fn finish_gesture(&mut self, state: &mut AppState) {
        self.sel_draft = None;
        if self.floating.is_some() {
            self.commit_floating(state);
        }
    }

    /// Select-tool input: draw selections, lift, and drive the transform
    /// handles. Pointer comes from egui (the pen arrives as synthesized
    /// mouse events for UI purposes — the stroke pipeline is not involved).
    #[allow(clippy::too_many_arguments)] // one call site; mirrors handle_pen
    fn select_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        to_paper: &impl Fn(Pos2) -> Pos2,
        to_screen: &impl Fn(Pos2) -> Pos2,
        scale: f32,
        state: &mut AppState,
        paint: Option<&mut PaintLayer>,
    ) {
        // Composite view is a review mode for gestures too: a selection drawn
        // over the graph render would be invisible (the overlay is edit-view
        // gated) and misleading. Hint once on an attempted drag.
        if self.composite_view {
            if response.drag_started_by(egui::PointerButton::Primary) {
                state.status = "composite view — press C to edit".into();
            }
            return;
        }
        // Enter/Esc belong to whoever has keyboard focus — finishing a rename
        // in another pane must not commit/cancel a live transform here.
        let kb_free = !ui.ctx().egui_wants_keyboard_input();
        let (esc, enter) = ui.input(|i| {
            (
                kb_free && i.key_pressed(egui::Key::Escape),
                kb_free && i.key_pressed(egui::Key::Enter),
            )
        });
        if esc {
            if self.floating.is_some() {
                self.cancel_floating();
                state.status = "transform cancelled".into();
            } else if self.sel_draft.take().is_some() {
                // dropped the in-progress outline
            } else if self.selection.take().is_some() {
                state.status = "selection cleared".into();
            }
        }
        if enter && self.floating.is_some() {
            self.commit_floating(state);
        }
        // A frame OR LAYER change slipped past the guards (an X-sheet click
        // scrub, a strip/chip layer click): commit to the LATCHED target
        // rather than displaying a floating patch over the wrong cel — or
        // doubling the lifted pixels over a projection rebuilt from
        // (unpunched) engine truth.
        if let Some(f) = &self.floating {
            let (kd, kl, _) = state.active_layer_key();
            if state.own_key_drawing() != Some(f.drawing)
                || kd != f.drawing.0
                || kl != f.layer.0
            {
                self.commit_floating(state);
            }
        }
        let Some(paint) = paint else {
            return; // no GPU: the select tool has nothing to lift/preview
        };

        let ptr = response.interact_pointer_pos();
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(pos) = ptr
            && rect.contains(pos)
        {
            let pp = to_paper(pos);
            if self.floating.is_some() {
                self.float_drag_start(pos, pp, to_screen, state);
            } else if let Some(sel) = self.selection.clone()
                && point_in_poly(&sel, pp)
            {
                // Drag inside the selection = lift it and start moving.
                if self.try_lift(state, paint, ui.ctx(), Some(&sel))
                    && let Some(f) = &mut self.floating
                {
                    f.drag = Some(FloatDrag::Move {
                        start_ptr: pos,
                        start_translate: f.translate,
                    });
                }
            } else {
                self.sel_draft = Some(vec![pp, pp]);
            }
        } else if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = ptr
        {
            if let Some(f) = &mut self.floating {
                match &f.drag {
                    Some(FloatDrag::Move { start_ptr, start_translate }) => {
                        f.translate = *start_translate + (pos - *start_ptr) / scale;
                    }
                    Some(FloatDrag::Scale { start_dist, start_scale }) => {
                        let pivot_s = to_screen(f.pivot + f.translate);
                        let d = (pos - pivot_s).length().max(1.0);
                        f.scale = (start_scale * d / start_dist.max(1.0)).clamp(0.05, 20.0);
                    }
                    Some(FloatDrag::Rotate { start_angle, start_rotate }) => {
                        let pivot_s = to_screen(f.pivot + f.translate);
                        let a = (pos - pivot_s).angle();
                        f.rotate = start_rotate + (a - start_angle);
                    }
                    None => {}
                }
            } else if let Some(draft) = &mut self.sel_draft {
                let pp = to_paper(pos);
                match self.sel_shape {
                    SelShape::Rect => {
                        draft.truncate(1);
                        draft.push(pp);
                    }
                    SelShape::Lasso => {
                        // Decimate: only keep points ≥2 screen px apart.
                        if draft
                            .last()
                            .is_none_or(|l| (pp - *l).length() * scale >= 2.0)
                        {
                            draft.push(pp);
                        }
                    }
                }
            }
        } else if response.drag_stopped_by(egui::PointerButton::Primary) {
            if let Some(f) = &mut self.floating {
                f.drag = None;
            } else if let Some(draft) = self.sel_draft.take() {
                self.selection = finalize_selection(draft, self.sel_shape, scale);
                if self.selection.is_some() {
                    state.status =
                        "selection set — drag inside to move; Esc clears".into();
                }
            }
        }
    }

    /// Hit-test a drag start against the floating transform's handles.
    fn float_drag_start(
        &mut self,
        pos: Pos2,
        pp: Pos2,
        to_screen: &impl Fn(Pos2) -> Pos2,
        state: &mut AppState,
    ) {
        let Some(f) = &mut self.floating else { return };
        let pivot_s = to_screen(f.pivot + f.translate);
        let corners_s: Vec<Pos2> = f.corners().iter().map(|c| to_screen(*c)).collect();
        let nearest = corners_s
            .iter()
            .map(|c| (pos - *c).length())
            .fold(f32::INFINITY, f32::min);
        if nearest <= 12.0 {
            f.drag = Some(FloatDrag::Scale {
                start_dist: (pos - pivot_s).length(),
                start_scale: f.scale,
            });
            return;
        }
        if nearest <= 28.0 {
            f.drag = Some(FloatDrag::Rotate {
                start_angle: (pos - pivot_s).angle(),
                start_rotate: f.rotate,
            });
            return;
        }
        // Inside the transformed patch = move; outside = commit (click-away).
        let a = f.affine();
        if a.scale.abs() >= edit::Affine::IDENTITY_SCALE_EPS {
            let inv = {
                let (sin, cos) = a.rotate_rad.sin_cos();
                let ox = pp.x - a.pivot.0 - a.translate.0;
                let oy = pp.y - a.pivot.1 - a.translate.1;
                let inv_s = 1.0 / a.scale;
                pos2(
                    a.pivot.0 + (cos * ox + sin * oy) * inv_s,
                    a.pivot.1 + (-sin * ox + cos * oy) * inv_s,
                )
            };
            let inside = inv.x >= f.patch.x0 as f32
                && inv.y >= f.patch.y0 as f32
                && inv.x < (f.patch.x0 + f.patch.w as i32) as f32
                && inv.y < (f.patch.y0 + f.patch.h as i32) as f32;
            if inside {
                f.drag = Some(FloatDrag::Move {
                    start_ptr: pos,
                    start_translate: f.translate,
                });
                return;
            }
        }
        self.commit_floating(state);
    }

    /// Select-tool overlay: marching-ants selection outline, the floating
    /// patch preview (textured mesh under the live affine), and its handles.
    /// Painted AFTER the layer images so it sits on top.
    fn select_overlay(&self, painter: &egui::Painter, to_screen: &impl Fn(Pos2) -> Pos2) {
        let ants = |pts: &[Pos2], closed: bool, painter: &egui::Painter| {
            if pts.len() < 2 {
                return;
            }
            let mut s: Vec<Pos2> = pts.iter().map(|p| to_screen(*p)).collect();
            if closed {
                s.push(s[0]);
            }
            painter.extend(egui::Shape::dashed_line(
                &s,
                egui::Stroke::new(2.5, Color32::from_white_alpha(180)),
                6.0,
                6.0,
            ));
            painter.extend(egui::Shape::dashed_line(
                &s,
                egui::Stroke::new(1.0, Color32::from_black_alpha(230)),
                6.0,
                6.0,
            ));
        };
        if let Some(sel) = &self.selection {
            ants(sel, true, painter);
        }
        if let Some(draft) = &self.sel_draft {
            match self.sel_shape {
                SelShape::Rect if draft.len() == 2 => {
                    let (a, b) = (draft[0], draft[1]);
                    ants(
                        &[a, pos2(b.x, a.y), b, pos2(a.x, b.y)],
                        true,
                        painter,
                    );
                }
                _ => ants(draft, false, painter),
            }
        }
        if let Some(f) = &self.floating {
            let c = f.corners(); // TL TR BL BR, paper
            let s: Vec<Pos2> = c.iter().map(|p| to_screen(*p)).collect();
            let mut mesh = egui::Mesh::with_texture(f.tex.id());
            let uv = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(0.0, 1.0), pos2(1.0, 1.0)];
            for i in 0..4 {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: s[i],
                    uv: uv[i],
                    color: Color32::WHITE,
                });
            }
            mesh.indices.extend_from_slice(&[0, 1, 2, 2, 1, 3]);
            painter.add(egui::Shape::mesh(mesh));
            // Outline (TL→TR→BR→BL) + corner scale handles.
            ants(&[c[0], c[1], c[3], c[2]], true, painter);
            for p in &s {
                let r = Rect::from_center_size(*p, vec2(9.0, 9.0));
                painter.rect_filled(r, 1.5, Color32::from_black_alpha(200));
                painter.rect_stroke(
                    r,
                    1.5,
                    egui::Stroke::new(1.5, Color32::from_white_alpha(230)),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    /// Fold one live tilt sample into the smoothed state (EMA) + diagnostics.
    /// Used by hover samples and by mid-stroke Moves past the seed.
    fn note_tilt(&mut self, t: [f32; 2]) {
        self.smoothed_tilt[0] += (t[0] - self.smoothed_tilt[0]) * TILT_SMOOTH;
        self.smoothed_tilt[1] += (t[1] - self.smoothed_tilt[1]) * TILT_SMOOTH;
        self.last_tilt = t;
        self.tilt_seen = true;
        self.dbg_tilt = (self.smoothed_tilt[0].powi(2) + self.smoothed_tilt[1].powi(2))
            .sqrt()
            .to_degrees();
    }

    /// Fixed-width pressure diagnostic for the status bar:
    /// (text, pressure-range-is-healthy, pen-arriving-as-mouse).
    pub fn pressure_diag(&self) -> (String, bool, bool) {
        let total = (self.dbg_some + self.dbg_none).max(1);
        let pct = 100 * self.dbg_some / total;
        // The tilt readout appears once a real tilt sample has arrived (hover
        // counts — the needle and readout work before the first stroke);
        // tilt_seen latches once, so the bar's width never reflows.
        let tilt = if self.tilt_seen {
            format!("  T{:2.0}°", self.dbg_tilt.clamp(0.0, 90.0))
        } else {
            String::new()
        };
        (
            format!(
                "{}P{:5.2}  {:4.2}–{:4.2}  {:3}%{}",
                if self.native_active { "ink " } else { "" },
                self.dbg_pressure, self.dbg_min, self.dbg_max, pct, tilt
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
        // Responsive: below the threshold the pane drops to COMPACT mode —
        // icon buttons and drag-value numbers instead of labelled sliders —
        // so it collapses to a slim icon rail beside the canvas.
        let compact = ui.available_width() < 190.0;
        let sw = (ui.available_width() * 0.45).clamp(48.0, 110.0);
        ui.spacing_mut().slider_width = sw;
        ui.horizontal_wrapped(|ui| {
            if raster_available {
                if compact {
                    if ui
                        .selectable_label(self.raster, "🖌")
                        .on_hover_text("raster brush engine")
                        .clicked()
                    {
                        self.raster = !self.raster;
                    }
                } else {
                    ui.checkbox(&mut self.raster, "raster")
                        .on_hover_text("GPU raster brush");
                    ui.separator();
                }
            } else {
                self.raster = false;
            }
            if self.raster {
                let (brush_lbl, eraser_lbl) = if compact {
                    ("✏", "▱")
                } else {
                    ("✏ brush", "▱ eraser")
                };
                if ui
                    .selectable_label(!self.erasing, brush_lbl)
                    .on_hover_text("paint ink")
                    .clicked()
                {
                    self.erasing = false;
                }
                if ui
                    .selectable_label(self.erasing, eraser_lbl)
                    .on_hover_text("erase to transparency")
                    .clicked()
                {
                    self.erasing = true;
                }
                // Select / transform tool (V; B returns to paint). Leaving
                // Select commits any floating transform.
                let sel_on = self.tool == CanvasTool::Select;
                if ui
                    .selectable_label(sel_on, if compact { "⬚" } else { "⬚ select" })
                    .on_hover_text(
                        "select / move / scale / rotate (V) — drag = select, drag \
                         inside = move, corners = scale, just outside = rotate; \
                         Enter applies, Esc cancels; Ctrl+A selects all",
                    )
                    .clicked()
                {
                    let next = if sel_on { CanvasTool::Paint } else { CanvasTool::Select };
                    self.set_tool(next, state);
                }
                if self.tool == CanvasTool::Select {
                    if ui
                        .selectable_label(self.sel_shape == SelShape::Lasso, "◌")
                        .on_hover_text("lasso selection")
                        .clicked()
                    {
                        self.sel_shape = SelShape::Lasso;
                    }
                    if ui
                        .selectable_label(self.sel_shape == SelShape::Rect, "▭")
                        .on_hover_text("rectangle selection")
                        .clicked()
                    {
                        self.sel_shape = SelShape::Rect;
                    }
                }
                // Fill / bucket tool (G): the shiage verb.
                let fill_on = self.tool == CanvasTool::Fill;
                if ui
                    .selectable_label(fill_on, if compact { "🪣" } else { "🪣 fill" })
                    .on_hover_text(
                        "flood fill (G) — click a region; line art bounds it. \
                         gap closes line breaks; under tucks the flat beneath \
                         the lines; cel/layer picks the boundary reference",
                    )
                    .clicked()
                {
                    let next = if fill_on { CanvasTool::Paint } else { CanvasTool::Fill };
                    self.set_tool(next, state);
                }
                if self.tool == CanvasTool::Fill {
                    let mut gap = self.fill_gap;
                    ui.add(egui::DragValue::new(&mut gap).range(0..=8).prefix("gap "));
                    self.fill_gap = gap;
                    let mut grow = self.fill_grow;
                    ui.add(egui::DragValue::new(&mut grow).range(0..=4).prefix("under "));
                    self.fill_grow = grow;
                    if ui
                        .selectable_label(self.fill_ref_cel, "cel")
                        .on_hover_text("boundaries come from the whole cel (all visible layers)")
                        .clicked()
                    {
                        self.fill_ref_cel = true;
                    }
                    if ui
                        .selectable_label(!self.fill_ref_cel, "layer")
                        .on_hover_text("boundaries come from the active layer only")
                        .clicked()
                    {
                        self.fill_ref_cel = false;
                    }
                }
                // Composite view: the node graph's rendered output (review
                // mode — painting pauses). Blocked mid-stroke: swapping the
                // view under a live stroke would orphan its display.
                if ui
                    .selectable_label(self.composite_view, if compact { "🎬" } else { "🎬 comp" })
                    .on_hover_text(
                        "composite view — what the node graph renders \
                         (playback/export truth). C toggles; painting pauses here.",
                    )
                    .clicked()
                    && !self.stroke_active()
                {
                    self.composite_view = !self.composite_view;
                }
                // Active cel-layer chip (RETAS trace-line colours). Click or A
                // cycles; strokes land on this layer. Red = hidden. Wide mode
                // pads to a FIXED width (no reflow on layer switch); compact
                // mode is the bare glyph with the name in the tooltip.
                let lname = state.active_layer_name();
                let hidden = state.active_layer_props().is_some_and(|p| !p.visible);
                let color = if hidden {
                    Color32::from_rgb(235, 90, 80)
                } else {
                    layer_chip_color(&lname)
                };
                let chip = if compact {
                    "▣".to_string()
                } else {
                    let shown: String = lname.chars().take(10).collect();
                    format!("▣ {shown:<10}")
                };
                if ui
                    .button(egui::RichText::new(chip).monospace().color(color))
                    .on_hover_text(if hidden {
                        format!("layer '{lname}' is HIDDEN — painting refused (A cycles)")
                    } else {
                        format!("active layer: {lname} — strokes land here (A cycles)")
                    })
                    .clicked()
                {
                    state.cycle_layer(false);
                }
                if compact {
                    ui.add(
                        egui::DragValue::new(&mut self.raster_brush_px)
                            .range(1.0..=300.0)
                            .suffix("px"),
                    )
                    .on_hover_text("brush size (drag)");
                    ui.add(
                        egui::DragValue::new(&mut self.brush_flow)
                            .range(0.05..=1.0)
                            .speed(0.01)
                            .fixed_decimals(2),
                    )
                    .on_hover_text("flow (drag)");
                    ui.add(
                        egui::DragValue::new(&mut self.brush_opacity)
                            .range(0.05..=1.0)
                            .speed(0.01)
                            .fixed_decimals(2),
                    )
                    .on_hover_text("opacity (drag)");
                } else {
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
                }
                ui.menu_button(if compact { "⚙" } else { "dynamics" }, |ui| {
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
                    ui.separator();
                    ui.checkbox(&mut self.tilt_size, "tilt → size")
                        .on_hover_text("tilting the pen broadens the stroke, like a pencil on its side");
                    ui.checkbox(&mut self.tilt_opacity, "tilt → opacity")
                        .on_hover_text("tilting the pen lightens the stroke");
                    ui.checkbox(&mut self.tilt_shape, "tilt → shape")
                        .on_hover_text(
                            "the stamp flattens and turns with the pen's lean — \
                             the stroke itself shows the tilt",
                        );
                    ui.add(
                        egui::Slider::new(&mut self.tilt_strength, 0.0..=1.0)
                            .text("tilt strength")
                            .fixed_decimals(2),
                    )
                    .on_hover_text(
                        "at 1.00 a fully flat pen (60°) doubles the size \
                         and quarters the opacity",
                    );
                    if !self.native_active {
                        ui.label(
                            egui::RichText::new("tilt flows from the native ink pen — draw once to latch it")
                                .weak()
                                .small(),
                        );
                    }
                });
                if ui
                    .button(if compact { "🗑" } else { "clear cel" })
                    .on_hover_text("clear this cel's raster (undoable)")
                    .clicked()
                    && !self.stroke_active()
                {
                    // Guarded like the keybind: clearing mid-gesture would
                    // mutate the layer a floating transform lifted from —
                    // the later commit would resurrect the cleared content.
                    state.clear_current_raster();
                }
            } else {
                ui.add(
                    egui::Slider::new(&mut self.brush_width, 0.5..=16.0)
                        .text("brush")
                        .fixed_decimals(1),
                );
            }
            if !compact {
                ui.separator();
            }
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
            if !compact {
                ui.separator();
            }
            if ui
                .button(if compact { "⌖" } else { "fit view" })
                .on_hover_text("reset zoom & pan")
                .clicked()
            {
                self.zoom = 1.0;
                self.pan = egui::Vec2::ZERO;
            }
            // NOTHING VARIABLE-WIDTH GOES IN THIS ROW (canvas-stability law):
            // diagnostics live in the status bar; PLAYING is a canvas overlay.
        });
    }

    #[allow(clippy::too_many_arguments)] // one call site; a struct would be noise
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        paint: Option<&mut PaintLayer>,
        graph: crate::viewer::GraphView,
        pen: &PenConfig,
        layers_cfg: &LayersConfig,
        native_pen: &[PenSample],
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
            match self.tool {
                CanvasTool::Paint => {
                    self.handle_pen(ui, &response, rect, &to_paper, scale, state, native_pen);
                }
                CanvasTool::Select => {
                    self.select_input(
                        ui,
                        &response,
                        rect,
                        &to_paper,
                        &to_screen,
                        scale,
                        state,
                        paint.as_deref_mut(),
                    );
                }
                CanvasTool::Fill => {
                    self.fill_input(ui, &response, rect, &to_paper, state);
                }
            }
        } else {
            self.current.clear();
            self.touch_active = false;
            // Playback started under a live gesture (e.g. the top-bar play
            // button): commit the floating transform to its LATCHED target
            // rather than leaving a lifted patch over a moving frame.
            if self.floating.is_some() {
                self.commit_floating(state);
            }
            self.sel_draft = None;
        }

        // ---- COMPOSITE VIEW ------------------------------------------------
        // The graph is executed ONCE per frame by Editor::ui (shared with the
        // viewer panes); the canvas just displays the result. The sandwich
        // section below is skipped entirely while this shows (its textures
        // keep their sync keys; anything that changed re-syncs by key
        // mismatch on return to edit view).
        let mut composite_tex: Option<egui::TextureId> = None;
        let mut composite_note: Option<&'static str> = None;
        if self.composite_view {
            use crate::viewer::GraphView as GV;
            match graph {
                GV::Ready(id) => composite_tex = Some(id),
                GV::NoOutput => {
                    composite_note =
                        Some("composite view — no graph output wired (C to edit)");
                }
                GV::EvalFailed => {
                    composite_note =
                        Some("composite view — the graph failed to evaluate (C to edit)");
                }
                GV::NoGpu => {
                    composite_note =
                        Some("composite view needs the GPU — showing edit view");
                }
                // Off = this canvas wasn't a visible consumer; if it IS being
                // rendered anyway, fall back honestly.
                GV::Off => {
                    composite_note =
                        Some("composite view — nothing rendered this frame (C to edit)");
                }
            }
        }

        // ---- Raster: keep the GPU layer synced to the current cel, stamp
        //      live dabs, and read back into the engine at pen-up ----------
        if composite_tex.is_none()
            && self.raster
            && let Some(p) = paint.as_deref_mut()
        {
            p.ensure_size(state.engine.project.width, state.engine.project.height);

            // RACE GUARD: a gesture commit/cancel set the invalidation
            // sentinel the SAME frame a stroke started (dispatch runs before
            // this ui). The between-strokes resync below only runs when no
            // stroke is live — restore engine truth HERE, before any dab
            // stamps, or the pen-up readback would bake the punched texture
            // back into the engine as a phantom erase. (Full-tuple sentinel
            // compare: a blank frame's key is (MAX, frame, 0), not this.)
            if self.synced_active == (u64::MAX, u64::MAX, u64::MAX)
                && (!self.current.is_empty() || self.raster_stroke_done)
            {
                match state.active_layer_tiles() {
                    Some(tiles) => p.sync_active(tiles),
                    None => p.clear_active(),
                }
                self.synced_active = state.active_layer_key();
            }

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

        // COMPOSITE VIEW: the graph's output IS the frame — nothing else
        // renders under or over it (no sandwich, no onion, no vector
        // overlays; it is the playback/export truth). `edit_view` gates
        // every editing-display step below.
        let edit_view = composite_tex.is_none();
        if let Some(id) = composite_tex {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(id, paper_rect, uv, Color32::WHITE);
            // Painted overlay tag (zero layout impact) so the mode is never
            // ambiguous — this view looks like art, not like a mode.
            painter.text(
                pos2(paper_rect.left() + 8.0, paper_rect.top() + 6.0),
                egui::Align2::LEFT_TOP,
                "🎬 composite",
                egui::FontId::proportional(12.0),
                Color32::from_rgba_unmultiplied(120, 190, 235, 200),
            );
        }
        if let Some(note) = composite_note {
            painter.text(
                pos2(rect.center().x, rect.top() + 34.0),
                egui::Align2::CENTER_CENTER,
                note,
                egui::FontId::proportional(13.0),
                Color32::from_rgb(235, 180, 90),
            );
        }

        // 1. Non-active columns (in sheet order = layer order).
        if edit_view {
            for col in &cut.xsheet.columns {
                if col.id == active_col {
                    continue;
                }
                if let Some(id) = col.resolve(frame)
                    && let Some(d) = cut.drawing(id) {
                        draw_strokes(&painter, &d.strokes, &to_screen, scale, None, &self.pen_curve);
                    }
            }
        }

        // 2. Onion ghosts of the active column, under its current drawing.
        if edit_view && state.onion {
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
        if edit_view
            && let Some(id) = state.resolve_at(active_col, frame)
            && let Some(d) = state.cut().drawing(id) {
                draw_strokes(&painter, &d.strokes, &to_screen, scale, None, &self.pen_curve);
            }

        // 3b. The raster sandwich, bottom→top: onion ghosts, BELOW projection,
        //     the ACTIVE layer at its own opacity, the live wet stroke, the
        //     ABOVE projection. Painting "color" previews under the line art
        //     live — the solo shiage workflow.
        if edit_view
            && self.raster
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
        if edit_view && !self.raster && self.current.len() >= 2 {
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

        // Select-tool overlay (ants, floating preview, handles) — after the
        // layer images so it reads on top of the art. Fill mode keeps the
        // selection ants visible: an active selection CONFINES fills, and an
        // invisible constraint would read as a broken bucket.
        if edit_view && matches!(self.tool, CanvasTool::Select | CanvasTool::Fill) {
            self.select_overlay(&painter, &to_screen);
        }

        // Empty-cell hint (vector mode only).
        if edit_view
            && !self.raster
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
            && !self.composite_view // review mode: painting refused, OS cursor back
            && self.tool == CanvasTool::Paint // select tool keeps the OS cursor
            && let Some(pos) = ui.input(|i| i.pointer.latest_pos())
            && rect.contains(pos)
            && ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            // Exact size: what a FULL-pressure dab paints (the falloff reaches
            // zero exactly at its radius), including the dynamics mapping —
            // min-size floor, the pressure→size toggle, and the tilt broaden
            // factor (without it, tilt→size would paint OUTSIDE the ring — the
            // one direction the preview must never err). Tilt is LIVE both
            // hovering and mid-stroke: proximity poses arrive as PenPhase::Hover
            // and feed note_tilt (pens that report no tilt in proximity keep
            // the previous stroke's carried value).
            // Floor only for visibility once a brush is sub-3px on screen.
            let t_max = if self.dyn_size { self.pen_curve.apply(1.0) } else { 1.0 };
            let min_s = self.min_size.clamp(0.0, 1.0);
            let strength = self.tilt_strength.clamp(0.0, 1.0);
            let tmag = (self.smoothed_tilt[0].powi(2) + self.smoothed_tilt[1].powi(2)).sqrt();
            let tn = (tmag / TILT_MAX_RAD).clamp(0.0, 1.0);
            let broaden = if self.tilt_size { 1.0 + strength * tn } else { 1.0 };
            let r = (self.raster_brush_px * (min_s + (1.0 - min_s) * t_max) * broaden * scale
                * 0.5)
                .max(1.5);
            // Tilt direction in screen space (paper axes = screen axes).
            let dir = if tmag > 1e-4 {
                egui::vec2(self.smoothed_tilt[0] / tmag, self.smoothed_tilt[1] / tmag)
            } else {
                egui::vec2(1.0, 0.0)
            };
            let aspect = if self.tilt_shape && tmag > 1e-4 {
                1.0 + TILT_ASPECT_GAIN * strength * tn
            } else {
                1.0
            };
            if aspect > 1.01 {
                // The footprint is a rotated ellipse (major axis along the
                // lean) — draw the exact outline, same double-ring contrast.
                let perp = egui::vec2(-dir.y, dir.x);
                let ellipse = |major: f32, minor: f32, color: Color32| {
                    let pts: Vec<Pos2> = (0..=32)
                        .map(|i| {
                            let th = i as f32 / 32.0 * std::f32::consts::TAU;
                            pos + dir * (th.cos() * major) + perp * (th.sin() * minor)
                        })
                        .collect();
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, color)));
                };
                ellipse(r * aspect, r, Color32::from_black_alpha(170));
                ellipse((r * aspect - 1.0).max(0.5), (r - 1.0).max(0.5),
                    Color32::from_white_alpha(170));
            } else {
                painter.circle_stroke(
                    pos, r, egui::Stroke::new(1.0, Color32::from_black_alpha(170)));
                painter.circle_stroke(
                    pos,
                    (r - 1.0).max(0.5),
                    egui::Stroke::new(1.0, Color32::from_white_alpha(170)),
                );
            }
            // Tilt needle (Krita-calligraphy style): a line from the centre
            // pointing the way the pen leans, growing with the lean — the
            // visual proof tilt is flowing even for a plain round brush.
            if self.tilt_seen && tn > TILT_NEEDLE_MIN {
                // Order-safe length: .max().min(), NOT clamp(4.0, footprint) —
                // f32::clamp PANICS when min > max, and small brushes routinely
                // have a sub-4px footprint. There the needle just spans it.
                let len = (r * aspect * tn).max(4.0).min(r * aspect);
                let tip = pos + dir * len;
                painter.line_segment(
                    [pos, tip],
                    egui::Stroke::new(3.0, Color32::from_white_alpha(150)),
                );
                painter.line_segment(
                    [pos, tip],
                    egui::Stroke::new(1.2, Color32::from_black_alpha(200)),
                );
            }
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
        // Tilt (native backend only; stored per point like pressure so
        // already-flushed dabs never recompute differently): the flatter the
        // pen, the broader and/or lighter the dab — side-of-the-lead feel.
        let min_s = self.min_size.clamp(0.0, 1.0);
        let strength = self.tilt_strength.clamp(0.0, 1.0);
        // Normalized 0 (vertical) .. 1 (≥60° flat) tilt magnitude.
        let tilt_norm =
            |t: [f32; 2]| ((t[0] * t[0] + t[1] * t[1]).sqrt() / TILT_MAX_RAD).clamp(0.0, 1.0);
        let radius_of = |pr: f32, tn: f32| {
            let t = if self.dyn_size { self.pen_curve.apply(pr) } else { 1.0 };
            let broaden = if self.tilt_size { 1.0 + strength * tn } else { 1.0 };
            (self.raster_brush_px * (min_s + (1.0 - min_s) * t) * broaden * 0.5)
                .max(0.5)
                .min(cap)
        };
        let dab_at = |x: f32, y: f32, pr: f32, tv: [f32; 2]| {
            let tn = tilt_norm(tv);
            let a = if self.dyn_opacity { self.pen_curve.apply(pr) } else { 1.0 };
            let lighten = if self.tilt_opacity { 1.0 - 0.75 * strength * tn } else { 1.0 };
            let mut color = base;
            color[3] *= flow * a * lighten;
            // tilt→shape: flatten + rotate the stamp along the lean direction
            // (paper axes = screen axes — the canvas never rotates).
            let radius = radius_of(pr, tn);
            let mag = (tv[0] * tv[0] + tv[1] * tv[1]).sqrt();
            let (dir, aspect) = if self.tilt_shape && mag > 1e-4 {
                let a = 1.0 + TILT_ASPECT_GAIN * strength * tn;
                // The no-force cap bounds the whole FOOTPRINT: the ellipse
                // paints out to radius×aspect along the lean, so a capped
                // (pressureless) dab must cap the major extent too — else a
                // leaned tap escapes the anti-blob cap by up to 2.5×.
                let a = if cap.is_finite() {
                    a.min((cap / radius).max(1.0))
                } else {
                    a
                };
                ([tv[0] / mag, tv[1] / mag], a)
            } else {
                ([1.0, 0.0], 1.0)
            };
            Dab {
                center: [x, y],
                radius,
                hardness,
                color,
                dir,
                aspect,
            }
        };

        if pts.len() == 1 {
            dabs.push(dab_at(pts[0].x, pts[0].y, pts[0].pressure, pts[0].tilt));
            return dabs;
        }

        let mut carry = 0.0f32; // distance into the next segment for the next dab
        for w in pts.windows(2) {
            let (ax, ay, apr) = (w[0].x, w[0].y, w[0].pressure);
            let (bx, by, bpr) = (w[1].x, w[1].y, w[1].pressure);
            // Lerp the tilt VECTOR (not the magnitude): adjacent samples point
            // the same general way, so the interpolated magnitude stays honest.
            let (at2, bt2) = (w[0].tilt, w[1].tilt);
            let (dx, dy) = (bx - ax, by - ay);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-4 {
                continue;
            }
            let mut d = carry;
            while d < len {
                let t = d / len;
                let pr = apr + (bpr - apr) * t;
                let tv = [
                    at2[0] + (bt2[0] - at2[0]) * t,
                    at2[1] + (bt2[1] - at2[1]) * t,
                ];
                let d2 = dab_at(ax + dx * t, ay + dy * t, pr, tv);
                let r = d2.radius;
                dabs.push(d2);
                let step = (0.1 * (2.0 * r)).max(0.75); // spacing 0.1 * diameter
                d += step;
            }
            carry = d - len;
        }
        dabs
    }

    /// Fill-tool input: a press flood-fills the clicked region on the ACTIVE
    /// layer, bounded by the reference (cel flatten or active layer), and
    /// commits immediately — one click = one PaintTiles = one undo step.
    fn fill_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        to_paper: &impl Fn(Pos2) -> Pos2,
        state: &mut AppState,
    ) {
        // Esc clears the selection HERE too — fills respect it, so dropping
        // it must not require a round-trip through the Select tool. Same
        // keyboard-focus gate as select_input.
        if !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Escape))
            && self.selection.take().is_some()
        {
            state.status = "selection cleared".into();
        }
        if !response.drag_started_by(egui::PointerButton::Primary) {
            return;
        }
        let Some(pos) = response.interact_pointer_pos() else {
            return;
        };
        if !rect.contains(pos) {
            return;
        }
        if self.composite_view {
            state.status = "composite view — press C to edit".into();
            return;
        }
        if !self.raster {
            state.status = "the fill tool needs the raster engine (🖌)".into();
            return;
        }
        if state.active_layer_props().is_some_and(|p| !p.visible) {
            state.status = format!(
                "layer '{}' is hidden — press A to switch or click its eye",
                state.active_layer_name()
            );
            return;
        }
        let Some(did) = state.own_key_drawing() else {
            state.status =
                "held/blank frame — a fill edits a frame's OWN cel (draw here first)".into();
            return;
        };
        let (kd, kl, _) = state.active_layer_key();
        if kd != did.0 || kl == u64::MAX {
            state.status = "no raster layer here to fill".into();
            return;
        }
        let (pw, ph) = (state.engine.project.width, state.engine.project.height);
        let p = to_paper(pos);
        let seed = (p.x.floor() as i32, p.y.floor() as i32);
        let reference = if self.fill_ref_cel {
            state.display_cel_flatten().unwrap_or_default()
        } else {
            state.active_layer_tiles().cloned().unwrap_or_default()
        };
        let opts = anim_core::fill::FillOpts {
            threshold: 0.1,
            gap_px: self.fill_gap,
            grow_px: self.fill_grow,
            // Layer mode: the reference IS the target — never let grow paint
            // over the very lines bounding the fill.
            protect_ink: !self.fill_ref_cel,
        };
        // Fills respect the select tool's active selection (industry rule);
        // its ants stay visible in fill mode so the constraint is never a
        // mystery.
        let clip: Option<Vec<(f32, f32)>> = self
            .selection
            .as_ref()
            .map(|sel| sel.iter().map(|q| (q.x, q.y)).collect());
        let mask = match anim_core::fill::flood_fill_mask(
            &reference,
            seed,
            pw,
            ph,
            &opts,
            clip.as_deref(),
        ) {
            Ok(m) => m,
            Err(r) => {
                use anim_core::fill::FillRefusal as FR;
                state.status = match r {
                    FR::OffPaper => "clicked outside the paper".into(),
                    FR::OnInk => "clicked on inked pixels — fills flow into EMPTY \
                         regions (a smaller gap shrinks the closing band; recolor \
                         is a future tool)"
                        .to_string(),
                    FR::OutsideSelection => {
                        "clicked outside the selection (Esc clears it)".into()
                    }
                    FR::ClippedOut => {
                        "the selection excluded the whole region (Esc clears it)".into()
                    }
                };
                return;
            }
        };
        // Fill colour = the brush colour through the same conversion the dab
        // path uses, premultiplied.
        let c = linear_rgba(self.brush_color);
        let premult = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
        let target = state.active_layer_tiles().cloned().unwrap_or_default();
        let diff = anim_core::fill::fill_diff(&target, &mask, premult);
        if diff.is_empty() {
            state.status = "fill made no change".into();
            return;
        }
        state.commit_region_edit("flood fill", did, LayerId(kl), diff);
        // The engine changed under the displayed texture — resync from truth.
        self.synced_active = (u64::MAX, u64::MAX, u64::MAX);
    }

    /// Begin a stroke at `pos` (shared by the native tablet path and the egui
    /// Touch path). Returns false if refused (outside canvas / hidden layer).
    fn stroke_start(
        &mut self,
        pos: Pos2,
        force: Option<f32>,
        tilt: Option<[f32; 2]>,
        rect: Rect,
        to_paper: &impl Fn(Pos2) -> Pos2,
        state: &mut AppState,
    ) -> bool {
        if !rect.contains(pos) {
            return false;
        }
        // GUARD: composite view is a review mode — a stroke would land on the
        // active layer while the canvas shows the graph's output (possibly
        // transformed or not even including that layer). Refuse + hint, same
        // pattern as the hidden-layer guard.
        if self.composite_view {
            state.status = "composite view — press C to edit".into();
            return false;
        }
        // GUARD (CSP behavior): never paint into a layer you can't see.
        if self.raster && state.active_layer_props().is_some_and(|p| !p.visible) {
            state.status = format!(
                "layer '{}' is hidden — press A to switch or click its eye",
                state.active_layer_name()
            );
            return false;
        }
        let p0 = force.filter(|f| *f > 0.0).unwrap_or(START_SEED);
        self.touch_active = true;
        self.seed_pending = force.filter(|f| *f > 0.0).is_none();
        self.last_pressure = p0;
        self.smoothed_pressure = p0;
        self.raw_history = [p0; 3];
        // Down packets carry no pose: seed from the last reported tilt (a
        // pen's tilt barely changes across a lift) and let the EMA converge.
        if let Some(t) = tilt {
            self.last_tilt = t;
        }
        self.smoothed_tilt = self.last_tilt;
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
        let p = to_paper(pos);
        self.current.push(StrokePoint {
            x: p.x,
            y: p.y,
            pressure: p0,
            tilt: self.smoothed_tilt,
        });
        true
    }

    /// End the live stroke (release pos/force are unreliable — the taper is
    /// synthesized from the committed points instead).
    fn stroke_end(&mut self, state: &mut AppState) {
        if self.touch_active {
            self.finish_stroke(state);
            self.mouse_lockout = 1;
        }
        self.touch_active = false;
        self.seed_pending = false;
    }

    #[allow(clippy::too_many_arguments)] // one call site; a struct would be noise
    fn handle_pen(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        to_paper: &impl Fn(Pos2) -> Pos2,
        scale: f32,
        state: &mut AppState,
        native_pen: &[PenSample],
    ) {
        if self.mouse_lockout > 0 {
            self.mouse_lockout -= 1;
        }

        let events = ui.input(|i| i.events.clone());
        let mut touch_seen = false;

        // HOVER TILT (any backend state): proximity samples carry live tilt so
        // the cursor needle + T° readout move before the stroke starts. Never
        // touches stroke state (the thread only emits Hover while the pen is
        // up, and the guard makes that a hard rule).
        for s in native_pen {
            if s.phase == PenPhase::Hover
                && !self.touch_active
                && let Some(t) = s.tilt
            {
                self.note_tilt(t);
            }
        }

        // NATIVE TABLET PATH (octotablet / Windows Ink RealTimeStylus): once
        // real tablet samples arrive, they own the pen forever this session —
        // Windows ALSO surfaces the same physical strokes as egui Touch and
        // mouse events, so both fallbacks must go quiet or every stroke would
        // paint twice. Hover samples deliberately do NOT latch: they never
        // paint, so they must not silence the fallbacks by themselves.
        if native_pen.iter().any(|s| s.phase != PenPhase::Hover) {
            self.native_active = true;
            self.seen_pen = true;
        }
        if self.native_active {
            for s in native_pen {
                match s.phase {
                    PenPhase::Down => {
                        self.stroke_start(s.pos, s.pressure, s.tilt, rect, to_paper, state);
                    }
                    PenPhase::Move => {
                        if self.touch_active {
                            self.process_move(s.pos, s.pressure, s.tilt, to_paper, scale);
                        }
                    }
                    PenPhase::Up => self.stroke_end(state),
                    PenPhase::Hover => {} // handled above
                }
            }
            return;
        }

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
                        // egui Touch carries no tilt data → treat it as a
                        // VERTICAL pen, explicitly (like the mouse path). None
                        // would seed from last_tilt, which hover samples now
                        // update WITHOUT latching native — a hovered pen's
                        // angle must not leak into a finger/fallback stroke.
                        self.stroke_start(*pos, *force, Some([0.0, 0.0]), rect, to_paper, state);
                    }
                    egui::TouchPhase::Move => {
                        if !self.touch_active {
                            continue;
                        }
                        self.process_move(*pos, *force, Some([0.0, 0.0]), to_paper, scale);
                    }
                    // Step 6: the release event's own pos/force are unreliable
                    // (End force is None on Windows). Discard them; synthesize
                    // the taper from the committed points instead.
                    egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                        self.stroke_end(state);
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
                if self.composite_view {
                    state.status = "composite view — press C to edit".into();
                    return;
                }
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
                        tilt: [0.0; 2], // mouse: vertical pen
                    });
                }
            } else if response.dragged_by(egui::PointerButton::Primary)
                && !self.current.is_empty()
            {
                // Continue ONLY a stroke whose start was ACCEPTED (current
                // non-empty). A refused start (composite view, hidden layer)
                // leaves current empty — without this gate the continuation
                // frames re-built the refused stroke and its release frame
                // committed it blind (and in raster mode latched
                // raster_stroke_done with the flush section skipped =
                // composite-view softlock).
                if let Some(p) = response.interact_pointer_pos() {
                    let p = to_paper(p);
                    self.current.push(StrokePoint {
                        x: p.x,
                        y: p.y,
                        pressure: MOUSE_PRESSURE,
                        tilt: [0.0; 2],
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
        tilt: Option<[f32; 2]>,
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

        // Tilt: EMA-smoothed (packets are steady — this just rounds off
        // single-packet steps); None reuses the last reported value. The first
        // real sample of a stroke SNAPS (the carried-over seed may be stale if
        // the pen re-approached at a new angle) — but the stored first point
        // is only corrected while NOTHING is stamped (dabs_flushed == 0):
        // rewriting a point whose dab is already on the GPU can't fix the
        // painted dab, only misalign every dab after it (radius feeds the
        // spacing chain). When Down and the first Move drain in the same
        // frame — the common case — the rewrite lands before any flush.
        if let Some(t) = tilt {
            if self.seed_pending {
                self.smoothed_tilt = t;
                if self.dabs_flushed == 0
                    && let Some(first) = self.current.first_mut()
                {
                    first.tilt = t;
                }
                self.last_tilt = t;
                self.tilt_seen = true;
                self.dbg_tilt = (t[0].powi(2) + t[1].powi(2)).sqrt().to_degrees();
            } else {
                self.note_tilt(t);
            }
        }

        // First real Move adopts the pressure immediately (no seed lag), so the
        // stroke has correct width from the moment the pen presses down. Same
        // flushed-prefix gate as the tilt snap above: a stamped first dab is
        // already painted at the seed width — rewriting the point can only
        // misalign the dabs that follow.
        if self.seed_pending && matches!(force, Some(f) if f > 0.0) {
            self.smoothed_pressure = raw;
            self.raw_history = [raw; 3];
            if self.dabs_flushed == 0
                && let Some(first) = self.current.first_mut()
            {
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
            // Bank only on the VECTOR path, which re-renders the whole stroke
            // every frame. The raster path's dabs for this point's segment are
            // already stamped — mutating the point recomputes them differently
            // (pressing or tilting while stationary would darken or gap the
            // stroke tip); the live values reach the stroke through the next
            // APPENDED point instead, which only extends the tail.
            if !self.raster
                && let Some(lp) = self.current.last_mut()
            {
                lp.pressure = self.smoothed_pressure;
                lp.tilt = self.smoothed_tilt;
            }
            return;
        }
        self.current.push(StrokePoint {
            x: p.x,
            y: p.y,
            pressure: self.smoothed_pressure,
            tilt: self.smoothed_tilt,
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
/// Even-odd point-in-polygon (paper space) — the same crossing rule the
/// engine's lift mask uses, so "drag inside the selection" and "pixels the
/// lift takes" agree.
fn point_in_poly(poly: &[Pos2], p: Pos2) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let t = (p.y - a.y) / (b.y - a.y);
            if p.x < a.x + t * (b.x - a.x) {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Turn a finished selection draft into a polygon; a near-click (tiny drag)
/// means deselect, mirroring the mouse stroke's stationary-click discard.
fn finalize_selection(draft: Vec<Pos2>, shape: SelShape, scale: f32) -> Option<Vec<Pos2>> {
    match shape {
        SelShape::Rect => {
            let (a, b) = (*draft.first()?, *draft.last()?);
            if (b.x - a.x).abs() * scale < 3.0 || (b.y - a.y).abs() * scale < 3.0 {
                return None;
            }
            Some(vec![a, pos2(b.x, a.y), b, pos2(a.x, b.y)])
        }
        SelShape::Lasso => {
            if draft.len() < 3 {
                return None;
            }
            let (mut lo, mut hi) = (draft[0], draft[0]);
            for p in &draft {
                lo = pos2(lo.x.min(p.x), lo.y.min(p.y));
                hi = pos2(hi.x.max(p.x), hi.y.max(p.y));
            }
            if (hi - lo).length() * scale < 4.0 {
                return None;
            }
            Some(draft)
        }
    }
}

fn linear_rgba(c: [u8; 4]) -> [f32; 4] {
    // ONE EOTF for the whole app: the engine's srgb_to_linear is the same
    // function Solid nodes render with (and the CPU/GPU graph compositors
    // pin) — sharing it makes "a Solid matches painting that colour" a
    // structural law instead of two copies that could drift.
    let lin = anim_core::export::srgb_to_linear;
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
