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

use crate::config::{BrushPreset, LayersConfig, PenConfig, PressureCurve};
use crate::doc::AppState;
use crate::icons::Icon;
use crate::paint::{Dab, PaintLayer, PaintMode};
use crate::plate;

/// Default new-project resolution (the New Project dialog's starting values).
pub const DEFAULT_PAPER_W: u32 = 1920;
pub const DEFAULT_PAPER_H: u32 = 1080;

/// The pencil case (spec §6 defect 5): ink · ao · aka · white. Ao and Aka are
/// the trade's blue/red pencils at the plate's own values, so the swatch board
/// reads by colour alone, peripherally, without stopping the stroke.
const SWATCHES: [[u8; 4]; 4] = [
    [25, 25, 30, 255],    // ink
    [83, 137, 196, 255],  // ao — construction / blue pencil
    [228, 82, 47, 255],   // aka — the sakkan's correction
    [245, 245, 245, 255], // white
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CanvasTool {
    Paint,
    Select,
    Fill,
    /// LASSO FILL (PSD-lasso-fill): loop a region freehand; it fills
    /// with the brush colour, feathered INWARD by `lasso_soft`.
    LassoFill,
}

/// Selection drawing shape (a rect is a 4-point polygon through the same
/// lift path).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    Move {
        start_ptr: Pos2,
        start_translate: egui::Vec2,
    },
    Scale {
        start_dist: f32,
        start_scale: f32,
    },
    Rotate {
        start_angle: f32,
        start_rotate: f32,
    },
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

// ---------------------------------------------------------------------------
// STAGE 1 (PSD-multiplayer-rescope): a stroke ON THE WIRE is its resolved
// dab list plus the few facts replay needs — mode, opacity, target, and
// the tip/grain images by content hash. Pixels never ride the stroke
// path (NEVER-DO 1); every machine grows the same pixels by replaying
// the same dabs through the same pipelines.
// ---------------------------------------------------------------------------

/// FNV-1a 64 — the content address for wire resources. Fixed constants,
/// no per-process keying: the SAME bytes hash the SAME on every machine.
pub fn wire_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A tip atlas on the wire: content hash + dims + GIH frame count.
#[derive(Clone)]
pub struct WireRes {
    pub hash: u64,
    pub w: u32,
    pub h: u32,
    pub frames: u32,
}

/// A grain image on the wire: content hash + dims + the armed scale and
/// strength (params ride the stroke; the image rides the cache).
#[derive(Clone)]
pub struct WireGrain {
    pub hash: u64,
    pub w: u32,
    pub h: u32,
    pub scale: f32,
    pub strength: f32,
}

/// One outgoing stroke event (canvas → App → wire).
pub enum StrokeMsg {
    Begin(StrokeBeginInfo),
    Dabs { stroke_id: u64, dabs: Vec<Dab> },
    End { stroke_id: u64 },
    /// v0.2.1 (audit): a streamed stroke that died locally (playback
    /// interrupted it, the room ended mid-stroke) — peers must drop
    /// their gathers and overlay, or they leak for the whole session.
    Abort { stroke_id: u64 },
}

pub struct StrokeBeginInfo {
    pub stroke_id: u64,
    pub drawing: u64,
    pub layer_name: String,
    /// 0 = ink, 1 = erase, 2 = alpha-lock (the latched sub-tool).
    pub mode: u8,
    pub opacity: f32,
    /// Header + bytes: the App announces unseen images once per session.
    pub tip: Option<(WireRes, std::sync::Arc<Vec<u8>>)>,
    pub grain: Option<(WireGrain, std::sync::Arc<Vec<u8>>)>,
}

/// v0.2.3 (the X-sheet law): ONE ordered queue for everything the host
/// sequenced — strokes, command batches, undos. Guests used to apply
/// commands inline while strokes deferred through the canvas, so a
/// delete-frame could leapfrog the stroke it followed and the X-sheet
/// forked. Now every machine applies the one order, whole.
pub enum SeqTask {
    Stroke(ReplayStroke),
    Cmds {
        origin: String,
        cmds: Vec<anim_core::command::Command>,
    },
    Undo {
        author: String,
        redo: bool,
    },
}

/// A remote stroke ready to replay: dabs + resolved resource bytes.
pub struct ReplayStroke {
    pub stroke_id: u64,
    pub author: String,
    pub drawing: u64,
    pub layer_name: String,
    pub mode: u8,
    pub opacity: f32,
    pub dabs: Vec<Dab>,
    pub tip: Option<(u32, u32, std::sync::Arc<Vec<u8>>, u32)>,
    pub grain: Option<(u32, u32, std::sync::Arc<Vec<u8>>, f32, f32)>,
}

/// What a replay (or a local streamed commit) actually changed — the
/// audit's raw material (host broadcasts these tiles' hashes).
pub struct ReplayDone {
    pub stroke_id: u64,
    pub author: String,
    pub drawing: u64,
    pub layer_name: String,
    pub changed: Vec<(i32, i32)>,
    pub ok: bool,
    pub why: String,
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
    pub(crate) raster: bool,
    raster_brush_px: f32,
    /// Eraser tool active: strokes subtract coverage (destination-out) instead of
    /// laying down ink. Same dab geometry, size and pressure response as the brush.
    /// Brush/eraser sub-tool. Readable so the UI hook can report the tool that
    /// is ACTUALLY armed rather than inferring one (see uidump.rs).
    pub(crate) erasing: bool,
    /// Alpha lock: ink can only land where the active layer ALREADY has
    /// coverage (Krita's lock-alpha) — recolor a shape without ever
    /// painting outside its silhouette. Session-transient (not a per-layer
    /// persisted property; v1.5's cheap MVP — a toolbar toggle, like
    /// eraser). Bypasses the wet buffer (like eraser) so the mask reads the
    /// ACTIVE layer's real alpha, not the wet buffer's own (which starts
    /// every stroke transparent and would mask out everything).
    alpha_lock: bool,
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
    /// Alpha-lock latched at stroke START — same reasoning as stroke_erasing.
    /// Ignored when stroke_erasing is also set (erase always wins).
    stroke_alpha_locked: bool,
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
    /// The pencil-box preset the brush was last armed FROM (by name). The
    /// slot's Tally ring draws broken once the live numbers drift off it.
    armed_preset: Option<String>,
    /// The brush rail (canvas right edge) is showing. Opens on hover at the
    /// edge, stays while the pointer is on it or a drag is live.
    rail_open: bool,
    /// Last frame's measured rail content height — centres the floating
    /// controls on the grip's line (settles in one frame).
    rail_content_h: f32,
    /// SESSION v2: this app is a GUEST in someone's room — finished
    /// strokes are SENT, never committed locally (one writer, NEVER-DO 1).
    pub(crate) is_guest: bool,
    /// STAGE 1 (PSD-multiplayer-rescope): a session is live (either
    /// role) — strokes stream as dab batches while they happen.
    pub(crate) session_live: bool,
    /// Outgoing stroke wire (Begin/Dabs/End), drained by the App.
    pub(crate) stroke_outbox: Vec<StrokeMsg>,
    /// v0.2.3: the one ordered queue of sequenced work (strokes, cmds,
    /// undos — host order preserved) + what each stroke committed and
    /// any cmd/undo failures (the App resyncs on those).
    pub(crate) replay_inbox: std::collections::VecDeque<SeqTask>,
    pub(crate) replay_done: Vec<ReplayDone>,
    pub(crate) seq_task_fail: Vec<String>,
    /// This stroke is streaming (latched at stroke start).
    stroke_streaming: bool,
    stroke_wire_id: u64,
    /// Wire identity only — never touches pixels, so the no-RNG stroke
    /// law is intact (uniqueness across peers, that's all).
    stroke_salt: u64,
    stroke_next_id: u64,
    /// The armed preset's content-addressed wire resources (header +
    /// bytes), refreshed whenever the brush resources re-arm.
    pub(crate) armed_tip_wire: Option<(WireRes, std::sync::Arc<Vec<u8>>)>,
    pub(crate) armed_grain_wire: Option<(WireGrain, std::sync::Arc<Vec<u8>>)>,
    /// STAGE 2: other artists' live ink — incoming dab batches
    /// (stroke id, stroke opacity, dabs), the strokes currently wet in
    /// the remote overlay, and the strokes whose commits landed (their
    /// overlay clears once all are done).
    pub(crate) remote_wet_inbox: Vec<(u64, f32, Vec<Dab>)>,
    pub(crate) remote_wet_end: Vec<u64>,
    remote_live: std::collections::HashSet<u64>,
    /// v0.2.1 (audit): the INCREMENTAL dab walk — a long stroke used to
    /// rebuild its whole dab list every frame (O(n²) per stroke). The
    /// cache holds every dab emitted so far; carry/walked/skipped and
    /// the smudge held-colour persist so only the new tail computes.
    dab_cache: Vec<Dab>,
    dab_carry: f32,
    dab_walked: f32,
    dab_skipped: u32,
    dab_pts_done: usize,
    dab_held: [f32; 4],
    /// A guest's pending undo(false)/redo(true) request for the host.
    pub(crate) history_request: Option<bool>,
    /// SESSION presence (PSD-session-room): peers as the canvas draws
    /// them, refreshed by the App each frame; and our own pointer in
    /// paper space, read back by the App to broadcast.
    pub(crate) peers: Vec<crate::net::PeerView>,
    pub(crate) presence_pos: Option<[f32; 2]>,
    /// MISS CHECK (shiage): the hole-hunt — the ground the cel
    /// composites over flips Paper -> Graphite so unpainted pixels read
    /// as dark pits. Pure UI paint; health is a meter, never a lamp.
    pub(crate) miss_check: bool,
    /// R-FLIP (momentary): while held, the canvas shows the PREVIOUS
    /// drawing at full strength — the animator's flip. Pure display,
    /// recomputed every frame; never engages mid-stroke or in playback.
    flip_held: bool,
    /// Refusal edge-flash bookkeeping (seq last seen + when).
    flash_seq: u32,
    flash_since: f64,
    /// Lightbox rail (Phase 4): ghost strength scales the onion alphas.
    onion_strength: f32,
    /// Paper furniture: field / safe-area / peg-bar guides, in Ao.
    show_field: bool,
    show_safe: bool,
    show_peg: bool,
    /// The INPUT plate field was clicked — open Settings at the pen page.
    pub(crate) request_pen_settings: bool,
    /// THE BRUSH LIBRARY (PSD-brush-library): the rail arms and imports;
    /// the import itself runs in the Editor, outside any stroke.
    pub(crate) request_brush_import: bool,
    /// PSD-brush-engine: the ARMED preset's machinery (None = procedural
    /// dab, byte-identical). Resources load on arm, at the next ui() that
    /// holds the paint layer.
    brush_engine: Option<crate::config::EngineDef>,
    brush_res_dirty: bool,
    /// LAN-JOIN GUARD (2026-08-19): a guest whose mirror has not yet
    /// arrived sees a blank canvas — a stroke committed against that
    /// blank REPLACES the host's inked tiles at their lift. Refuse
    /// strokes until the first snapshot/commands land.
    pub(crate) guest_ready: bool,
    /// The paint layer holds a real tip mask right now (dabs carry the
    /// tip flag only while true).
    tip_active: bool,
    /// Frames in the armed tip's atlas (GIH pipe brushes cycle; 1 = still).
    tip_frames: u32,
    /// LASSO FILL: the loop being drawn (paper space), and its edge
    /// parameters — softness feathers INWARD (NEVER-DO 3), grow shifts
    /// the edge ±px.
    lasso_pts: Vec<Pos2>,
    lasso_soft: f32,
    lasso_grow: f32,
    /// THE SMUDGE GATE: the active layer's committed tiles, snapshotted
    /// at stroke start — the PRE-STROKE truth the held colour samples
    /// (Arc clones; cannot change while the wet stroke is open).
    smudge_src: Option<std::collections::BTreeMap<anim_core::raster::TileCoord, std::sync::Arc<anim_core::raster::TileData>>>,
    /// One click imports the INSTALLED Krita's own brushes (bundles +
    /// presets + the user's %APPDATA%/krita).
    pub(crate) request_krita_scan: bool,
    brush_search: String,
    /// Lazy thumbnail textures by preset name; None = no cached thumb
    /// (the painted-dab fallback draws instead — never tofu).
    thumb_cache: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    /// The lightbox rail (left edge) is showing — same fold law as the
    /// brush rail (owner 2026-08-17: everything in the canvas folds).
    light_open: bool,
    light_content_h: f32,
    /// THE PAINT DISH (bottom edge, its own room 2026-08-17).
    dish_open: bool,
    dish_content_w: f32,
    /// Session mixing splotches: (position 0..1 in the tray, radius, colour).
    /// Never saved — the model is the only colour truth (NEVER-DO 2).
    splotches: Vec<([f32; 2], f32, [u8; 4])>,
    /// The Eye is editing this palette role (char idx, role idx); "set"
    /// writes it back, DANGER-guarded (palettes have no undo).
    eye_target: Option<(usize, usize)>,
    /// The Eye's sticky HSV (hue survives greys) + the rgb it derives from.
    eye_hsv: [f32; 3],
    eye_sync_rgb: [u8; 3],
    /// Session colour memory per layer NAME: picking a colour while a layer is
    /// active remembers it here (overrides the Settings default until close).
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
            alpha_lock: false,
            brush_flow: 1.0,
            brush_opacity: 1.0,
            wet_dirty: false,
            stroke_erasing: false,
            stroke_alpha_locked: false,
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
            armed_preset: None,
            rail_open: false,
            rail_content_h: 420.0,
            is_guest: false,
            session_live: false,
            stroke_outbox: Vec::new(),
            replay_inbox: std::collections::VecDeque::new(),
            replay_done: Vec::new(),
            seq_task_fail: Vec::new(),
            stroke_streaming: false,
            stroke_wire_id: 0,
            stroke_salt: rand::random(),
            stroke_next_id: 0,
            armed_tip_wire: None,
            armed_grain_wire: None,
            remote_wet_inbox: Vec::new(),
            remote_wet_end: Vec::new(),
            remote_live: std::collections::HashSet::new(),
            dab_cache: Vec::new(),
            dab_carry: 0.0,
            dab_walked: 0.0,
            dab_skipped: 0,
            dab_pts_done: 0,
            dab_held: [0.0; 4],
            history_request: None,
            peers: Vec::new(),
            presence_pos: None,
            miss_check: false,
            flip_held: false,
            flash_seq: 0,
            flash_since: -10.0,
            onion_strength: 0.45,
            show_field: false,
            show_safe: false,
            show_peg: false,
            request_pen_settings: false,
            request_brush_import: false,
            brush_engine: None,
            brush_res_dirty: false,
            guest_ready: false,
            tip_active: false,
            tip_frames: 1,
            smudge_src: None,
            lasso_pts: Vec::new(),
            lasso_soft: 0.0,
            lasso_grow: 0.0,
            request_krita_scan: false,
            brush_search: String::new(),
            thumb_cache: std::collections::HashMap::new(),
            light_open: false,
            light_content_h: 330.0,
            dish_open: false,
            dish_content_w: 620.0,
            splotches: Vec::new(),
            eye_target: None,
            eye_hsv: [0.0, 0.0, 0.0],
            eye_sync_rgb: [0, 0, 0],
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

    /// Flip alpha-lock (bound to a rebindable shortcut).
    pub fn toggle_alpha_lock(&mut self) {
        self.alpha_lock = !self.alpha_lock;
    }

    /// Apply a brush preset wholesale (keybind 1–8, Presets pane, or a
    /// workspace switch bound to it). Also drops the eraser — a preset is ink.
    pub fn apply_preset(&mut self, p: &crate::config::BrushPreset) {
        // PSD-brush-engine: arm the preset's machinery (None clears it —
        // the pencil box always lands here with None).
        if self.brush_engine != p.engine {
            self.brush_engine = p.engine.clone();
            self.brush_res_dirty = true;
        }
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
        self.armed_preset = Some(p.name.clone());
    }

    /// Snapshot the current brush as a preset (the Presets pane's "save").
    pub fn snapshot_preset(&self, name: String) -> crate::config::BrushPreset {
        crate::config::BrushPreset {
            name,
            engine: None,
            bank: String::new(),
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
            // AUDIT [20]: this returned in silence — the key looked
            // broken rather than refused.
            state.refuse("refused — finish the stroke first");
            return;
        }
        // Leaving Select for ANY tool lands the floating transform.
        if self.tool == CanvasTool::Select && self.floating.is_some() {
            self.commit_floating(state);
        }
        self.sel_draft = None;
        self.tool = tool;
        state.status = match tool {
            CanvasTool::Select => "select: drag = select, drag inside = move, corners = scale, \
        just outside corners = rotate; Enter applies, Esc cancels"
                .into(),
            CanvasTool::LassoFill => "lasso fill: draw a loop — it fills with the brush             colour; softness feathers inward, grow shifts the edge"
                .into(),
            CanvasTool::Fill => "fill: click a region — line art bounds it; gap closes line \
            breaks, under tucks the flat beneath the lines"
                .into(),
            CanvasTool::Paint => "paint".into(),
        };
    }

    /// Capture the tool/view state a workspace restores (LENS-DOCK: a
    /// workspace = layout + TOOL/MODE + view). The cursor (frame/column) is
    /// deliberately NOT captured — switching rooms re-lenses the same spot,
    /// it never navigates.
    pub fn snapshot_view(&self, state: &AppState) -> crate::workspace::WorkspaceView {
        crate::workspace::WorkspaceView {
            tool: self.tool,
            composite_view: self.composite_view,
            onion: state.view.onion,
            onion_layer_only: state.view.onion_layer_only,
            sel_shape: self.sel_shape,
            fill_ref_cel: self.fill_ref_cel,
        }
    }

    /// Restore a workspace's tool/view state — ALL OR NOTHING: a room must
    /// never come back half-restored (new onion/shape but old tool). Any
    /// floating transform lands first (explicitly — set_tool's same-tool
    /// early-out would skip its commit); a live PEN stroke refuses the whole
    /// view restore with a status note (the layout still switches). Returns
    /// whether the view was restored (callers gate the preset apply on it).
    pub fn apply_view(
        &mut self,
        v: &crate::workspace::WorkspaceView,
        state: &mut AppState,
    ) -> bool {
        if self.floating.is_some() {
            self.commit_floating(state);
        }
        self.sel_draft = None;
        if self.touch_active || !self.current.is_empty() || self.raster_stroke_done {
            state.status = "workspace switched — tool/view kept (finish the stroke first)".into();
            return false;
        }
        self.lasso_pts.clear();
        self.set_tool(v.tool, state);
        self.composite_view = v.composite_view;
        self.sel_shape = v.sel_shape;
        self.fill_ref_cel = v.fill_ref_cel;
        state.view.onion = v.onion;
        state.view.onion_layer_only = v.onion_layer_only;
        true
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
            state.refuse("refused — composite view is read-only (C to edit)");
            return false;
        }
        if !self.raster {
            // AUDIT [34]: an emoji, in the grey lane, naming an
            // internal instead of the switch in Settings.
            state.refuse(
                "refused — the select tool needs the GPU brush engine \
                 (Settings › Performance)",
            );
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
                "held/blank frame — a transform edits a frame's OWN cel (draw here first)".into();
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
            if state.own_key_drawing() != Some(f.drawing) || kd != f.drawing.0 || kl != f.layer.0 {
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
                    Some(FloatDrag::Move {
                        start_ptr,
                        start_translate,
                    }) => {
                        f.translate = *start_translate + (pos - *start_ptr) / scale;
                    }
                    Some(FloatDrag::Scale {
                        start_dist,
                        start_scale,
                    }) => {
                        let pivot_s = to_screen(f.pivot + f.translate);
                        let d = (pos - pivot_s).length().max(1.0);
                        f.scale = (start_scale * d / start_dist.max(1.0)).clamp(0.05, 20.0);
                    }
                    Some(FloatDrag::Rotate {
                        start_angle,
                        start_rotate,
                    }) => {
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
                    state.status = "selection set — drag inside to move; Esc clears".into();
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
                    ants(&[a, pos2(b.x, a.y), b, pos2(a.x, b.y)], true, painter);
                }
                _ => ants(draft, false, painter),
            }
        }
        if let Some(f) = &self.floating {
            let c = f.corners(); // TL TR BL BR, paper
            let s: Vec<Pos2> = c.iter().map(|p| to_screen(*p)).collect();
            let mut mesh = egui::Mesh::with_texture(f.tex.id());
            let uv = [
                pos2(0.0, 0.0),
                pos2(1.0, 0.0),
                pos2(0.0, 1.0),
                pos2(1.0, 1.0),
            ];
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
                self.dbg_pressure,
                self.dbg_min,
                self.dbg_max,
                pct,
                tilt
            ),
            self.dbg_max - self.dbg_min > 0.15,
            self.dbg_mouse_mode,
        )
    }

    /// The engine changed under the displayed texture (session snapshot
    /// applied) — resync the raster display from truth.
    pub(crate) fn engine_changed(&mut self) {
        self.synced_active = (u64::MAX, u64::MAX, u64::MAX);
    }

    /// THE FORGE reads the armed engine to seed a draft.
    pub(crate) fn armed_engine(&self) -> Option<&crate::config::EngineDef> {
        self.brush_engine.as_ref()
    }

    // (presence_wet is RETIRED — STAGE 2's dab stream IS the live view;
    // peers with old builds still send wet points and they still draw.)

    /// The slate's arming line: what the pen does right now, in a word.
    pub fn arming_pencil(&self) -> String {
        match self.tool {
            CanvasTool::Select => "select".into(),
            CanvasTool::Fill => "fill".into(),
            CanvasTool::LassoFill => "lasso fill".into(),
            _ if self.erasing => "eraser".into(),
            _ => self.armed_preset.clone().unwrap_or_else(|| "brush".into()),
        }
    }

    /// True while the live brush still matches `p`'s numbers exactly.
    fn matches_preset(&self, p: &BrushPreset) -> bool {
        let e = |a: f32, b: f32| (a - b).abs() < 0.001;
        e(self.raster_brush_px, p.size_px)
            && e(self.brush_flow, p.flow)
            && e(self.brush_opacity, p.opacity)
            && self.dyn_size == p.dyn_size
            && self.dyn_opacity == p.dyn_opacity
            && e(self.min_size, p.min_size)
            && p.color.is_none_or(|c| self.brush_color == c)
            && self.tilt_size == p.tilt_size
            && self.tilt_opacity == p.tilt_opacity
            && self.tilt_shape == p.tilt_shape
            && e(self.tilt_strength, p.tilt_strength)
    }

    /// One pencil-box slot (genga room charter): an XL detent carrying the
    /// preset's specimen stroke in its own ink, engraved name, and hotkey
    /// digit. The specimen is painted from the preset's REAL numbers — the
    /// pressure story you see is the one you'll get.
    fn pencil_slot(
        &mut self,
        ui: &mut egui::Ui,
        p: &BrushPreset,
        digit: usize,
        state: &mut AppState,
    ) {
        let (rect, resp) = ui.allocate_exact_size(vec2(88.0, 44.0), Sense::click());
        let armed = self.tool == CanvasTool::Paint
            && !self.erasing
            && self.armed_preset.as_deref() == Some(p.name.as_str());
        if resp.clicked() && !self.stroke_active() {
            if self.tool != CanvasTool::Paint {
                self.set_tool(CanvasTool::Paint, state);
            }
            self.apply_preset(p);
        }
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter();
        // The specimen sits in a Well recess: paint is MATERIAL, seated,
        // never a signal (the charters' material rule).
        painter.rect_filled(rect, 0.0, plate::WELL);
        let ink = p.color.unwrap_or([228, 225, 214, 255]);
        let n = 12;
        let x0 = rect.left() + 8.0;
        let x1 = rect.right() - 8.0;
        let cy = rect.top() + 15.0;
        let mut prev = pos2(x0, cy + 2.5);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let x = x0 + (x1 - x0) * t;
            let y = cy + (t * std::f32::consts::TAU).sin() * 2.5;
            // Pressure story: ramp in, peak mid-stroke, release.
            let press = (t * std::f32::consts::PI).sin();
            let w = if p.dyn_size {
                p.min_size + (1.0 - p.min_size) * press
            } else {
                1.0
            } * (p.size_px * 0.5).clamp(2.5, 9.0);
            let a = p.opacity
                * if p.dyn_opacity {
                    (0.25 + 0.75 * press) * p.flow.max(0.4)
                } else {
                    1.0
                };
            let col = Color32::from_rgba_unmultiplied(ink[0], ink[1], ink[2], (a * 255.0) as u8);
            let pt = pos2(x, y);
            painter.line_segment([prev, pt], egui::Stroke::new(w, col));
            prev = pt;
        }
        let name_ink = if armed || resp.hovered() {
            plate::STRUCK
        } else {
            plate::LEGEND
        };
        painter.text(
            pos2(rect.left() + 8.0, rect.bottom() - 9.0),
            egui::Align2::LEFT_CENTER,
            p.name.to_uppercase(),
            egui::FontId::new(9.5, plate::semibold()),
            name_ink,
        );
        painter.text(
            pos2(rect.right() - 7.0, rect.bottom() - 9.0),
            egui::Align2::RIGHT_CENTER,
            format!("{digit}"),
            egui::FontId::new(11.0, egui::FontFamily::Monospace),
            plate::legend_dim(),
        );
        // The lamp: solid Tally ring armed; BROKEN ring = armed but the
        // live numbers have been nudged off the preset.
        if armed {
            if self.matches_preset(p) {
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(2.0, plate::TALLY),
                    egui::StrokeKind::Inside,
                );
            } else {
                let c = [
                    rect.left_top(),
                    rect.right_top(),
                    rect.right_bottom(),
                    rect.left_bottom(),
                    rect.left_top(),
                ];
                painter.extend(egui::Shape::dashed_line(
                    &c,
                    egui::Stroke::new(2.0, plate::TALLY),
                    7.0,
                    5.0,
                ));
            }
        } else if resp.hovered() {
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, plate::LEGEND),
                egui::StrokeKind::Inside,
            );
        }
        let mut hover = format!(
            "{} — {:.0}px · flow {:.2} · opacity {:.2} (hotkey {digit})",
            p.name, p.size_px, p.flow, p.opacity
        );
        if plate::exact() {
            hover.push_str(&format!(
                "\n[PENCIL SLOT '{}' · canvas.rs pencil_slot · preset in config.presets]",
                p.name
            ));
        }
        resp.on_hover_text(hover);
    }

    /// The eraser as the pencil box's fourth detent.
    fn eraser_slot(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        let (rect, resp) = ui.allocate_exact_size(vec2(56.0, 44.0), Sense::click());
        let armed = self.tool == CanvasTool::Paint && self.erasing;
        if resp.clicked() {
            if self.tool != CanvasTool::Paint {
                self.set_tool(CanvasTool::Paint, state);
            }
            self.erasing = true;
        }
        if !ui.is_rect_visible(rect) {
            return;
        }
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, plate::WELL);
        let ink = if armed || resp.hovered() {
            plate::STRUCK
        } else {
            plate::LEGEND
        };
        crate::icons::paint(
            painter,
            Rect::from_center_size(
                pos2(rect.center().x, rect.top() + 15.0),
                egui::Vec2::splat(18.0),
            ),
            ink,
            Icon::Eraser,
        );
        painter.text(
            pos2(rect.center().x, rect.bottom() - 9.0),
            egui::Align2::CENTER_CENTER,
            "ERASER",
            egui::FontId::new(9.5, plate::semibold()),
            ink,
        );
        if armed {
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(2.0, plate::TALLY),
                egui::StrokeKind::Inside,
            );
        } else if resp.hovered() {
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, plate::LEGEND),
                egui::StrokeKind::Inside,
            );
        }
        if plate::exact() {
            resp.on_hover_text("[PENCIL SLOT 'eraser' · canvas.rs eraser_slot]");
        }
    }

    /// THE PAINT DISH's content (its room, 2026-08-17): ARMED WELL with
    /// row context · THE MODEL (characters × roles — the colour-model
    /// table) · THE DISH (mixing splotches) · THE EYE (compact SV+hue).
    /// Picking never writes the model; only the Eye's held "set" and the
    /// row's held DERIVE write (DANGER — palettes have no undo).
    fn dish_ui(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        // ---- ARMED WELL + row context ----
        let mut armed_role: Option<String> = None;
        let mut row_chips: Vec<[u8; 4]> = Vec::new();
        for ch in &state.palettes.characters {
            for r in &ch.roles {
                if r.color[..3] == self.brush_color[..3] {
                    armed_role = Some(r.name.clone());
                    row_chips = ch.roles.iter().map(|q| q.color).collect();
                }
            }
        }
        ui.vertical(|ui| {
            ui.add_space(10.0);
            let (wrect, _) = ui.allocate_exact_size(vec2(46.0, 46.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(wrect.expand(2.0), 0.0, plate::WELL);
            p.rect_filled(
                wrect,
                0.0,
                Color32::from_rgba_unmultiplied(
                    self.brush_color[0],
                    self.brush_color[1],
                    self.brush_color[2],
                    255,
                ),
            );
            p.rect_stroke(
                wrect,
                0.0,
                egui::Stroke::new(1.0, plate::LEGEND),
                egui::StrokeKind::Outside,
            );
            ui.label(
                egui::RichText::new(format!(
                    "#{:02X}{:02X}{:02X}",
                    self.brush_color[0], self.brush_color[1], self.brush_color[2]
                ))
                .monospace()
                .size(10.0)
                .color(plate::STRUCK),
            );
            if let Some(n) = &armed_role {
                plate::legend(ui, n);
            }
            ui.horizontal(|ui| {
                for c in &row_chips {
                    let (cr, cresp) = ui.allocate_exact_size(vec2(13.0, 13.0), Sense::click());
                    let pnt = ui.painter();
                    pnt.rect_filled(
                        cr,
                        0.0,
                        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 255),
                    );
                    pnt.rect_stroke(
                        cr,
                        0.0,
                        egui::Stroke::new(1.0, plate::legend_dim()),
                        egui::StrokeKind::Inside,
                    );
                    if cresp.clicked() {
                        self.brush_color = *c;
                    }
                }
            });
        });
        ui.add_space(12.0);

        // ---- THE MODEL: the colour-model table ----
        ui.vertical(|ui| {
            ui.add_space(6.0);
            plate::legend(ui, "model");
            let headers: Vec<String> = state
                .palettes
                .characters
                .first()
                .map(|c| c.roles.iter().map(|r| r.name.clone()).collect())
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.allocate_exact_size(vec2(56.0, 11.0), Sense::hover());
                for h in &headers {
                    let (hr, _) = ui.allocate_exact_size(vec2(32.0, 11.0), Sense::hover());
                    ui.painter().text(
                        hr.center(),
                        egui::Align2::CENTER_CENTER,
                        h.chars().take(6).collect::<String>().to_uppercase(),
                        egui::FontId::new(8.0, plate::semibold()),
                        plate::LEGEND,
                    );
                }
            });
            let n_chars = state.palettes.characters.len();
            for ci in 0..n_chars {
                ui.horizontal(|ui| {
                    let cname = state.palettes.characters[ci].name.clone();
                    let (nr, _) = ui.allocate_exact_size(vec2(56.0, 22.0), Sense::hover());
                    ui.painter().text(
                        pos2(nr.left() + 2.0, nr.center().y),
                        egui::Align2::LEFT_CENTER,
                        cname.chars().take(7).collect::<String>(),
                        egui::FontId::new(10.0, egui::FontFamily::Proportional),
                        plate::LEGEND,
                    );
                    let n_roles = state.palettes.characters[ci].roles.len();
                    for ri in 0..n_roles {
                        let color = state.palettes.characters[ci].roles[ri].color;
                        let rname = state.palettes.characters[ci].roles[ri].name.clone();
                        let (pr, presp) = ui.allocate_exact_size(vec2(32.0, 22.0), Sense::click());
                        let well = pr.shrink2(vec2(2.0, 1.0));
                        let pnt = ui.painter();
                        pnt.rect_filled(well.expand(1.0), 0.0, plate::WELL);
                        pnt.rect_filled(
                            well,
                            0.0,
                            Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 255),
                        );
                        if self.brush_color[..3] == color[..3] {
                            pnt.rect_stroke(
                                well.expand(1.0),
                                0.0,
                                egui::Stroke::new(2.0, plate::TALLY),
                                egui::StrokeKind::Outside,
                            );
                        } else {
                            pnt.rect_stroke(
                                well,
                                0.0,
                                egui::Stroke::new(1.0, plate::legend_dim()),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if presp.double_clicked() {
                            self.brush_color = color;
                            self.eye_target = Some((ci, ri));
                        } else if presp.clicked() {
                            self.brush_color = color;
                        }
                        presp.on_hover_text(format!(
                            "{cname} · {rname} — click: arm · double-click: edit in the eye"
                        ));
                    }
                    // DERIVE (held DANGER — writes the model, no undo there):
                    // this row's shadow from its normal, the systematic way.
                    if plate::danger(ui, "derive") {
                        let ch = &mut state.palettes.characters[ci];
                        if let Some(norm) = ch
                            .roles
                            .iter()
                            .find(|r| r.name == "normal")
                            .map(|r| r.color)
                        {
                            let sh = crate::palette::derive_shadow(norm);
                            match ch.roles.iter_mut().find(|r| r.name == "shadow") {
                                Some(r) => r.color = sh,
                                None => ch.roles.push(crate::palette::ColorRole {
                                    name: "shadow".into(),
                                    color: sh,
                                }),
                            }
                            state.status =
                                format!("{cname}: shadow derived from normal (edit in the eye)");
                        } else {
                            state.refuse("refused — no 'normal' role to derive from");
                        }
                    }
                });
            }
        });
        ui.add_space(12.0);

        // ---- THE DISH: the mixing tray ----
        ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                plate::legend(ui, "dish");
                if ui
                    .button("rinse")
                    .on_hover_text("clear the tray (splotches are session-only)")
                    .clicked()
                {
                    self.splotches.clear();
                }
            });
            let (dr, dresp) = ui.allocate_exact_size(vec2(180.0, 100.0), Sense::click());
            let pnt = ui.painter();
            pnt.rect_filled(dr, 0.0, plate::WELL);
            pnt.rect_stroke(
                dr,
                0.0,
                egui::Stroke::new(1.0, plate::legend_dim()),
                egui::StrokeKind::Inside,
            );
            for (posn, r, c) in &self.splotches {
                pnt.circle_filled(
                    pos2(
                        dr.left() + posn[0] * dr.width(),
                        dr.top() + posn[1] * dr.height(),
                    ),
                    *r,
                    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 255),
                );
            }
            if let Some(hp) = dresp.interact_pointer_pos() {
                let hit = self.splotches.iter().rposition(|(q, r, _)| {
                    let cpos = pos2(dr.left() + q[0] * dr.width(), dr.top() + q[1] * dr.height());
                    cpos.distance(hp) <= *r
                });
                if dresp.secondary_clicked() {
                    if let Some(i) = hit {
                        self.splotches.remove(i);
                    }
                } else if dresp.clicked() {
                    match hit {
                        // Dead-centre on a splotch: PICK it up.
                        Some(i) => {
                            self.brush_color = self.splotches[i].2;
                        }
                        // Empty dish: LAY the current colour. Overlapping a
                        // neighbour MIXES 50/50 with it — a physical dish.
                        None => {
                            let rel = [
                                ((hp.x - dr.left()) / dr.width()).clamp(0.02, 0.98),
                                ((hp.y - dr.top()) / dr.height()).clamp(0.02, 0.98),
                            ];
                            let r_new = 8.0 + (self.splotches.len() % 4) as f32;
                            let mut c = self.brush_color;
                            if let Some(j) = self.splotches.iter().rposition(|(q, r, _)| {
                                let cpos = pos2(
                                    dr.left() + q[0] * dr.width(),
                                    dr.top() + q[1] * dr.height(),
                                );
                                cpos.distance(hp) <= *r + r_new
                            }) {
                                let o = self.splotches[j].2;
                                for k in 0..3 {
                                    c[k] = ((c[k] as u16 + o[k] as u16) / 2) as u8;
                                }
                                self.brush_color = c;
                            }
                            self.splotches.push((rel, r_new, c));
                        }
                    }
                }
            }
        });
        ui.add_space(12.0);

        // ---- THE EYE: compact SV square + hue strip, always open ----
        ui.vertical(|ui| {
            ui.add_space(6.0);
            plate::legend(ui, "eye");
            if self.brush_color[..3] != self.eye_sync_rgb[..] {
                let (h, sat, v) = crate::palette::rgb_to_hsv(
                    self.brush_color[0],
                    self.brush_color[1],
                    self.brush_color[2],
                );
                // Hue is meaningless at zero saturation — keep it sticky.
                if sat > 0.001 {
                    self.eye_hsv[0] = h;
                }
                self.eye_hsv[1] = sat;
                self.eye_hsv[2] = v;
                self.eye_sync_rgb = [
                    self.brush_color[0],
                    self.brush_color[1],
                    self.brush_color[2],
                ];
            }
            let mut changed = false;
            let (svr, svresp) = ui.allocate_exact_size(vec2(120.0, 78.0), Sense::click_and_drag());
            let pnt = ui.painter();
            let (hr_, hg, hb) = crate::palette::hsv_to_rgb(self.eye_hsv[0], 1.0, 1.0);
            let hue_col = Color32::from_rgb(hr_, hg, hb);
            let mut mesh = egui::Mesh::default();
            let idx = mesh.vertices.len() as u32;
            for (pt, col) in [
                (svr.left_top(), Color32::WHITE),
                (svr.right_top(), hue_col),
                (svr.right_bottom(), Color32::BLACK),
                (svr.left_bottom(), Color32::BLACK),
            ] {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: pt,
                    uv: egui::epaint::WHITE_UV,
                    color: col,
                });
            }
            mesh.indices
                .extend_from_slice(&[idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]);
            pnt.add(egui::Shape::mesh(mesh));
            pnt.rect_stroke(
                svr,
                0.0,
                egui::Stroke::new(1.0, plate::legend_dim()),
                egui::StrokeKind::Inside,
            );
            let marker = pos2(
                svr.left() + self.eye_hsv[1] * svr.width(),
                svr.top() + (1.0 - self.eye_hsv[2]) * svr.height(),
            );
            pnt.circle_stroke(marker, 4.0, egui::Stroke::new(1.5, plate::STRUCK));
            if svresp.dragged() || svresp.clicked() {
                if let Some(hp) = svresp.interact_pointer_pos() {
                    self.eye_hsv[1] = ((hp.x - svr.left()) / svr.width()).clamp(0.0, 1.0);
                    self.eye_hsv[2] = (1.0 - (hp.y - svr.top()) / svr.height()).clamp(0.0, 1.0);
                    changed = true;
                }
            }
            let (hue_r, hue_resp) =
                ui.allocate_exact_size(vec2(120.0, 12.0), Sense::click_and_drag());
            let pnt = ui.painter();
            let n = 24;
            for i in 0..n {
                let t0 = i as f32 / n as f32;
                let t1 = (i + 1) as f32 / n as f32;
                let (r0, g0, b0) = crate::palette::hsv_to_rgb(t0 * 360.0, 1.0, 1.0);
                pnt.rect_filled(
                    Rect::from_min_max(
                        pos2(hue_r.left() + t0 * hue_r.width(), hue_r.top()),
                        pos2(hue_r.left() + t1 * hue_r.width(), hue_r.bottom()),
                    ),
                    0.0,
                    Color32::from_rgb(r0, g0, b0),
                );
            }
            let hx = hue_r.left() + (self.eye_hsv[0] / 360.0) * hue_r.width();
            pnt.line_segment(
                [pos2(hx, hue_r.top() - 1.0), pos2(hx, hue_r.bottom() + 1.0)],
                egui::Stroke::new(2.0, plate::STRUCK),
            );
            if hue_resp.dragged() || hue_resp.clicked() {
                if let Some(hp) = hue_resp.interact_pointer_pos() {
                    self.eye_hsv[0] =
                        (((hp.x - hue_r.left()) / hue_r.width()) * 360.0).clamp(0.0, 359.9);
                    changed = true;
                }
            }
            if changed {
                let (r, g, b) =
                    crate::palette::hsv_to_rgb(self.eye_hsv[0], self.eye_hsv[1], self.eye_hsv[2]);
                self.brush_color = [r, g, b, self.brush_color[3]];
                self.eye_sync_rgb = [r, g, b];
            }
            if let Some((ci, ri)) = self.eye_target {
                let label = state
                    .palettes
                    .characters
                    .get(ci)
                    .and_then(|c| c.roles.get(ri).map(|r| format!("{} · {}", c.name, r.name)));
                match label {
                    Some(l) => {
                        ui.label(
                            egui::RichText::new(format!("→ {l}"))
                                .size(9.5)
                                .color(plate::LEGEND),
                        );
                        // Writes the model — held DANGER (no undo there).
                        if plate::danger(ui, "set") {
                            state.palettes.characters[ci].roles[ri].color = self.brush_color;
                            state.status = format!("{l} updated");
                            self.eye_target = None;
                        }
                    }
                    None => self.eye_target = None,
                }
            }
        });
        ui.add_space(8.0);
    }

    /// The lightbox fold-out's content: onion + strength (with a value
    /// line — a silent dial reads as a bug), paper furniture, view, INPUT.
    fn lightbox_rail_ui(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        fn vline(ui: &mut egui::Ui, label: &str, value: String) {
            let mut job = egui::text::LayoutJob::default();
            job.append(
                &label.to_uppercase(),
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::new(9.5, plate::semibold()),
                    color: plate::LEGEND,
                    extra_letter_spacing: 0.9,
                    ..Default::default()
                },
            );
            job.append(
                &format!("  {value}"),
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::new(11.0, egui::FontFamily::Monospace),
                    color: plate::STRUCK,
                    ..Default::default()
                },
            );
            ui.label(job);
        }

        // THE QUEUE (shiage charter): prev/next CEL at the paper's own
        // edge, 44x40 — the room's second verb; Q/W are their keys.
        plate::legend(ui, "queue");
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            for (icon, fwd, hover) in [
                (Icon::ChevL, false, "previous cel (Q)"),
                (Icon::ChevR, true, "next cel (W)"),
            ] {
                let (cr, cresp) = ui.allocate_exact_size(vec2(44.0, 40.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(cr, 0.0, plate::WELL);
                p.rect_stroke(
                    cr,
                    0.0,
                    egui::Stroke::new(
                        1.0,
                        if cresp.hovered() {
                            plate::LEGEND
                        } else {
                            plate::legend_dim()
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                let ink = if cresp.hovered() {
                    plate::STRUCK
                } else {
                    plate::LEGEND
                };
                crate::icons::paint(
                    p,
                    Rect::from_center_size(cr.center(), egui::Vec2::splat(22.0)),
                    ink,
                    icon,
                );
                if cresp.clicked() && !self.stroke_active() {
                    state.goto_adjacent_key(fwd);
                }
                cresp.on_hover_text(hover);
            }
        });
        let mut mc = self.miss_check;
        plate::tool_latch(ui, &mut mc, Icon::Hole, "miss check").on_hover_text(
            "hole-hunt (M): the ground goes dark; every unpainted pixel reads as a pit",
        );
        self.miss_check = mc;
        ui.add_space(10.0);
        plate::legend(ui, "lightbox");
        ui.add_space(2.0);
        let mut on = state.view.onion;
        plate::latch(ui, &mut on, "onion")
            .on_hover_text("ghost the neighbouring DRAWINGS (O) — a hold of the same drawing has no neighbour to ghost");
        state.view.onion = on;
        let mut lo = state.view.onion_layer_only;
        plate::latch(ui, &mut lo, "line only")
            .on_hover_text("ghost only the layer matching the active layer's name");
        state.view.onion_layer_only = lo;
        ui.add_space(2.0);
        vline(
            ui,
            "strength",
            format!("{:.0}%", self.onion_strength * 100.0),
        );
        ui.spacing_mut().slider_width = 96.0;
        ui.add(egui::Slider::new(&mut self.onion_strength, 0.1..=1.0).show_value(false));
        if let Some(pid) = state.view.ghost_pin {
            ui.add_space(6.0);
            let name = state
                .cut()
                .drawing(pid)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            vline(ui, "ghost", name);
            if ui
                .button("unpin")
                .on_hover_text("stop ghosting the pinned drawing")
                .clicked()
            {
                state.view.ghost_pin = None;
            }
        }
        ui.add_space(10.0);
        plate::legend(ui, "paper");
        ui.add_space(2.0);
        let mut f = self.show_field;
        plate::latch(ui, &mut f, "field").on_hover_text("the 90% action field");
        self.show_field = f;
        let mut sa = self.show_safe;
        plate::latch(ui, &mut sa, "safe").on_hover_text("the 80% safe area + centre cross");
        self.show_safe = sa;
        let mut pg = self.show_peg;
        plate::latch(ui, &mut pg, "peg").on_hover_text("the registration peg bar");
        self.show_peg = pg;
        ui.add_space(10.0);
        plate::legend(ui, "view");
        ui.add_space(2.0);
        if plate::icon_button(ui, Icon::Fit, "fit", "reset zoom & pan").clicked() {
            self.zoom = 1.0;
            self.pan = egui::Vec2::ZERO;
        }
        vline(ui, "zoom", format!("{:3.0}%", self.zoom * 100.0));
        ui.add_space(10.0);
        plate::legend(ui, "input");
        ui.add_space(2.0);
        let (_d, _h, mouse) = self.pressure_diag();
        let val = if mouse { "mouse" } else { "pen · ink" };
        if ui
            .add(
                egui::Label::new(
                    egui::RichText::new(val)
                        .monospace()
                        .size(11.0)
                        .color(plate::STRUCK),
                )
                .sense(Sense::click()),
            )
            .on_hover_text("input device — click for pen settings")
            .clicked()
        {
            self.request_pen_settings = true;
        }
    }

    /// The brush rail's content, centred on ONE axis: every slider is a
    /// bare track (no side text), so the track's centre IS the rail's
    /// centre — the point the hand reflexes to (owner 2026-08-17). Labels
    /// ride ABOVE the tracks as engraved value lines.
    fn brush_rail_ui(&mut self, ui: &mut egui::Ui, presets: &[BrushPreset]) {
        fn value_line(ui: &mut egui::Ui, label: &str, value: String) {
            let mut job = egui::text::LayoutJob::default();
            job.append(
                &label.to_uppercase(),
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::new(9.5, plate::semibold()),
                    color: plate::LEGEND,
                    extra_letter_spacing: 0.9,
                    ..Default::default()
                },
            );
            job.append(
                &format!("  {value}"),
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::new(11.0, egui::FontFamily::Monospace),
                    color: plate::STRUCK,
                    ..Default::default()
                },
            );
            ui.label(job);
        }
        let title = if self.erasing {
            "eraser".to_string()
        } else {
            self.armed_preset.clone().unwrap_or_else(|| "brush".into())
        };
        plate::legend(ui, &title);
        ui.add_space(8.0);
        ui.spacing_mut().slider_width = 150.0;
        value_line(ui, "size", format!("{:.0} px", self.raster_brush_px));
        ui.add(egui::Slider::new(&mut self.raster_brush_px, 1.0..=300.0).show_value(false));
        ui.add_space(4.0);
        value_line(ui, "flow", format!("{:.2}", self.brush_flow));
        ui.add(egui::Slider::new(&mut self.brush_flow, 0.05..=1.0).show_value(false));
        ui.add_space(4.0);
        value_line(ui, "opacity", format!("{:.2}", self.brush_opacity));
        ui.add(egui::Slider::new(&mut self.brush_opacity, 0.05..=1.0).show_value(false));
        ui.add_space(10.0);
        plate::legend(ui, "pressure");
        ui.add_space(2.0);
        let mut ps = self.dyn_size;
        plate::latch(ui, &mut ps, "p·size").on_hover_text("pressure drives size");
        self.dyn_size = ps;
        let mut po = self.dyn_opacity;
        plate::latch(ui, &mut po, "p·opac").on_hover_text("pressure drives opacity");
        self.dyn_opacity = po;
        ui.add_space(4.0);
        value_line(ui, "min size", format!("{:.2}", self.min_size));
        ui.add(egui::Slider::new(&mut self.min_size, 0.0..=1.0).show_value(false))
            .on_hover_text("size at zero pressure, as a fraction of the brush");
        ui.add_space(10.0);
        plate::legend(ui, "tilt");
        ui.add_space(2.0);
        let mut ts = self.tilt_size;
        plate::latch(ui, &mut ts, "t·size").on_hover_text("tilting broadens the stroke");
        self.tilt_size = ts;
        let mut to = self.tilt_opacity;
        plate::latch(ui, &mut to, "t·opac").on_hover_text("tilting lightens the stroke");
        self.tilt_opacity = to;
        let mut tsh = self.tilt_shape;
        plate::latch(ui, &mut tsh, "t·shape")
            .on_hover_text("the stamp flattens and turns with the pen's lean");
        self.tilt_shape = tsh;
        ui.add_space(4.0);
        value_line(ui, "strength", format!("{:.2}", self.tilt_strength));
        ui.add(egui::Slider::new(&mut self.tilt_strength, 0.0..=1.0).show_value(false));
        if !self.native_active {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("tilt flows from the native ink pen — draw once to latch it")
                    .color(plate::legend_dim())
                    .small(),
            );
        }

        // ---- THE LIBRARY (PSD-brush-library): imported Krita presets,
        // browsed and ARMED here; edited only in Settings → Brushes.
        ui.add_space(10.0);
        value_line(ui, "library", format!("{}", presets.len()));
        ui.add_space(2.0);
        if presets.len() > 6 {
            ui.add(
                egui::TextEdit::singleline(&mut self.brush_search)
                    .desired_width(168.0)
                    .hint_text("search brushes"),
            );
            ui.add_space(4.0);
        }
        let needle = self.brush_search.trim().to_lowercase();
        let shown: Vec<&BrushPreset> = presets
            .iter()
            .filter(|p| needle.is_empty() || p.name.to_lowercase().contains(&needle))
            .collect();
        if presets.is_empty() {
            ui.label(
                egui::RichText::new("no brushes yet — import Krita .kpp / .bundle below")
                    .color(plate::legend_dim())
                    .small(),
            );
        } else if shown.is_empty() {
            ui.label(
                egui::RichText::new("nothing matches that search")
                    .color(plate::legend_dim())
                    .small(),
            );
        }
        let mut arm: Option<BrushPreset> = None;
        egui::ScrollArea::vertical()
            .id_salt("brush_library")
            .max_height(240.0)
            .show(ui, |ui| {
                const CELL: f32 = 48.0;
                const PER_ROW: usize = 3;
                for row in shown.chunks(PER_ROW) {
                    ui.horizontal(|ui| {
                        for p in row {
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(CELL, CELL),
                                egui::Sense::click(),
                            );
                            let armed =
                                self.armed_preset.as_deref() == Some(p.name.as_str());
                            let painter = ui.painter();
                            painter.rect_filled(rect, 0.0, plate::WELL);
                            // The preset's own Krita icon from the cache;
                            // a painted dab when there is none (never tofu).
                            let tex = self
                                .thumb_cache
                                .entry(p.name.clone())
                                .or_insert_with(|| {
                                    let dir = crate::kpp::thumb_dir()?;
                                    let path = dir.join(format!(
                                        "{}.png",
                                        crate::kpp::thumb_key(&p.name)
                                    ));
                                    let bytes = std::fs::read(path).ok()?;
                                    let decoder = png::Decoder::new(
                                        std::io::Cursor::new(bytes),
                                    );
                                    let mut reader = decoder.read_info().ok()?;
                                    let mut buf =
                                        vec![0u8; reader.output_buffer_size()];
                                    let info = reader.next_frame(&mut buf).ok()?;
                                    if info.color_type != png::ColorType::Rgba {
                                        return None;
                                    }
                                    let img =
                                        egui::ColorImage::from_rgba_unmultiplied(
                                            [
                                                info.width as usize,
                                                info.height as usize,
                                            ],
                                            &buf[..info.buffer_size()],
                                        );
                                    Some(ui.ctx().load_texture(
                                        format!("brush_thumb:{}", p.name),
                                        img,
                                        egui::TextureOptions::LINEAR,
                                    ))
                                })
                                .clone();
                            match tex {
                                Some(t) => {
                                    painter.image(
                                        t.id(),
                                        rect.shrink(2.0),
                                        egui::Rect::from_min_max(
                                            pos2(0.0, 0.0),
                                            pos2(1.0, 1.0),
                                        ),
                                        Color32::WHITE,
                                    );
                                }
                                None => {
                                    let r = (p.size_px * 0.5)
                                        .clamp(3.0, CELL * 0.38);
                                    painter.circle_filled(
                                        rect.center(),
                                        r,
                                        plate::STRUCK,
                                    );
                                }
                            }
                            if armed {
                                painter.rect_stroke(
                                    rect.shrink(1.0),
                                    0.0,
                                    egui::Stroke::new(2.0, plate::TALLY),
                                    egui::StrokeKind::Inside,
                                );
                            } else if resp.hovered() {
                                painter.rect_stroke(
                                    rect,
                                    0.0,
                                    egui::Stroke::new(1.0, plate::LEGEND),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            // Hover says what the brush honestly is
                            // (room NEVER-DO 4): its engine, and when
                            // that engine is beyond our set, that it
                            // paints as the plain dab.
                            let hover = match &p.engine {
                                Some(e) => {
                                    let honest = match e.engine.as_str() {
                                        "paintbrush" | "spraybrush" | "roundmarker"
                                        | "forge" | "stamp-import" | "" => String::new(),
                                        // NEVER-DO 2: dulling, said plainly.
                                        "colorsmudge" => " — picks up committed colour \
                                             (no self-smear within a stroke)"
                                            .into(),
                                        other => {
                                            format!(" — {other} engine paints as our dab")
                                        }
                                    };
                                    format!(
                                        "arm '{}' · {:.0}px{honest}",
                                        p.name, p.size_px
                                    )
                                }
                                None => format!("arm '{}' · {:.0}px", p.name, p.size_px),
                            };
                            if resp.on_hover_text(hover).clicked() {
                                arm = Some((*p).clone());
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
            });
        if let Some(p) = arm {
            self.apply_preset(&p);
        }
        ui.add_space(4.0);
        if ui
            .button("import brushes…")
            .on_hover_text(
                "Krita .kpp presets and community .bundle packs — they \
                 paint with OUR brush at the preset's size and strength",
            )
            .clicked()
        {
            self.request_brush_import = true;
        }
        if !crate::kpp::installed_krita_paths().is_empty()
            && ui
                .button("import installed Krita's brushes")
                .on_hover_text(
                    "everything the Krita on this machine ships — its \
                     bundles, its presets, and your %APPDATA%/krita \
                     brushes — mapped onto OUR brush honestly",
                )
                .clicked()
        {
            self.request_krita_scan = true;
        }
    }

    /// Brush & tool controls — a dockable pane of its own (the old canvas
    /// toolbar). Wrapped layout: in a narrow dock it flows to more rows
    /// without pushing any other pane around.
    pub fn brush_ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        raster_available: bool,
        presets: &[BrushPreset],
    ) {
        // Sliders are the widest fixed-size widgets — scale them to the pane
        // so the toolbox keeps collapsing in narrow docks instead of hitting
        // a ~180px floor at the dock divider.
        // Responsive: below the threshold the pane drops to COMPACT mode —
        // icon buttons and drag-value numbers instead of labelled sliders —
        // so it collapses to a slim icon rail beside the canvas.
        let compact = ui.available_width() < 190.0;
        let sw = (ui.available_width() * 0.45).clamp(48.0, 110.0);
        ui.spacing_mut().slider_width = sw;
        // The raster/vector engine switch is a GPU backend choice, not a tool
        // (spec §6 defect 2) — it lives in Settings → Performance now.
        if !raster_available {
            self.raster = false;
        }
        ui.horizontal_wrapped(|ui| {
            if self.raster {
                // ---- THE PENCIL BOX (genga room charter): the three named
                // pencils + the eraser, one XL detent group with specimen
                // strokes. Arming a pencil arms the brush; the Tally ring
                // breaks when the live brush drifts off the preset.
                let lbl = |s: &'static str| if compact { "" } else { s };
                for (i, p) in presets.iter().enumerate() {
                    if matches!(p.name.as_str(), "atari" | "genga" | "shusei") {
                        self.pencil_slot(ui, p, i + 1, state);
                    }
                }
                self.eraser_slot(ui, state);
                ui.separator();
                // Select / transform tool (V; B returns to paint). Leaving
                // Select commits any floating transform. A detent: clicking
                // the armed position does nothing.
                let sel_on = self.tool == CanvasTool::Select;
                if plate::tool_button(ui, sel_on, Icon::Select, lbl("select"))
                    .on_hover_text(
                        "select / move / scale / rotate (V) — drag = select, drag \
                    inside = move, corners = scale, just outside = rotate; \
                    Enter applies, Esc cancels; Ctrl+A selects all",
                    )
                    .clicked()
                    && !sel_on
                {
                    self.set_tool(CanvasTool::Select, state);
                }
                // (Select's shape options live in the OPTIONS row below —
                // arming a tool must never move the strip. Spec defect 4.)
                // Fill / bucket tool (G): the shiage verb.
                let fill_on = self.tool == CanvasTool::Fill;
                if plate::tool_button(ui, fill_on, Icon::Fill, lbl("fill"))
                    .on_hover_text(
                        "flood fill (G) — click a region; line art bounds it. \
                    gap closes line breaks; under tucks the flat beneath \
                    the lines; cel/layer picks the boundary reference",
                    )
                    .clicked()
                    && !fill_on
                {
                    self.set_tool(CanvasTool::Fill, state);
                }
                // Lasso fill: loop a region, it fills feathered.
                let lf_on = self.tool == CanvasTool::LassoFill;
                if plate::tool_button(ui, lf_on, Icon::Lasso, lbl("lasso fill"))
                    .on_hover_text(
                        "lasso fill — draw a loop and it fills with the brush \
                    colour; softness feathers the edge inward, grow shifts \
                    the edge; one loop = one undo step",
                    )
                    .clicked()
                    && !lf_on
                {
                    self.set_tool(CanvasTool::LassoFill, state);
                }
                // (Fill's gap/under/boundary options live in the OPTIONS
                // row below, same law.)
                // ---- The latches (independent states, Tally left-edge). ----
                // Alpha lock (L): ink can only recolor pixels the active
                // layer already has coverage on — never paint outside its
                // silhouette. Combines with brush or eraser.
                let mut lock = self.alpha_lock;
                plate::tool_latch(ui, &mut lock, Icon::Lock, lbl("lock")).on_hover_text(
                    "alpha lock (L) — ink only recolors pixels this layer \
                already has coverage on; the silhouette can't grow",
                );
                self.alpha_lock = lock;
                // Composite view: the node graph's rendered output (review
                // mode — painting pauses). Blocked mid-stroke: swapping the
                // view under a live stroke would orphan its display.
                let mut comp = self.composite_view;
                plate::tool_latch(ui, &mut comp, Icon::Comp, lbl("comp")).on_hover_text(
                    "composite view — what the node graph renders \
                (playback/export truth). C toggles; painting pauses here.",
                );
                if comp != self.composite_view && !self.stroke_active() {
                    self.composite_view = comp;
                }
                // Active cel-layer chip (RETAS trace-line colours). Click or A
                // cycles; strokes land on this layer. Red = hidden. Wide mode
                // pads to a FIXED width (no reflow on layer switch); compact
                // mode is the bare glyph with the name in the tooltip.
                let lname = state.active_layer_name();
                let hidden = state.active_layer_props().is_some_and(|p| !p.visible);
                // Hidden is a CONFIGURATION the animator chose, not a
                // contradiction — so it dims, it does not go red (the
                // Warning Law, spec §3). The refusal at stroke time is
                // where the contradiction lives.
                let color = if hidden {
                    plate::legend_dim()
                } else {
                    layer_chip_color(&lname)
                };
                // Seated chip (owner's audit): identity dot + name in a
                // Well slot, same material as every other control.
                let (crect, cresp) =
                    ui.allocate_exact_size(vec2(96.0, plate::CTRL_H + 4.0), Sense::click());
                if cresp.clicked() {
                    state.cycle_layer(false);
                }
                if ui.is_rect_visible(crect) {
                    let pnt = ui.painter();
                    pnt.rect_filled(crect, 0.0, plate::WELL);
                    pnt.rect_stroke(
                        crect,
                        0.0,
                        egui::Stroke::new(
                            1.0,
                            if cresp.hovered() {
                                plate::LEGEND
                            } else {
                                plate::legend_dim()
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    pnt.circle_filled(pos2(crect.left() + 10.0, crect.center().y), 4.0, color);
                    let shown: String = lname.chars().take(10).collect();
                    pnt.text(
                        pos2(crect.left() + 20.0, crect.center().y),
                        egui::Align2::LEFT_CENTER,
                        shown,
                        egui::FontId::new(11.0, egui::FontFamily::Monospace),
                        if hidden {
                            plate::legend_dim()
                        } else {
                            plate::STRUCK
                        },
                    );
                }
                cresp.on_hover_text(if hidden {
                    format!("layer '{lname}' is HIDDEN — painting refused (A cycles)")
                } else {
                    format!("active layer: {lname} — strokes land here (A cycles)")
                });
            // (Size, flow, opacity and the dynamics latches live in
            // the OPTIONS row below — the strip's membership is law.)
            // (ERASE CEL moved to the Cel Layers pane footer as a
            // guarded DANGER control — spec §6 defects 6/7. A
            // destructive command does not sit in the tool strip
            // dressed like a mode switch.)
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
            let mut rgb = [
                self.brush_color[0],
                self.brush_color[1],
                self.brush_color[2],
            ];
            if ui
                .color_edit_button_srgb(&mut rgb)
                .on_hover_text("brush colour")
                .changed()
            {
                self.brush_color = [rgb[0], rgb[1], rgb[2], self.brush_color[3]];
            }
            for (i, c) in SWATCHES.iter().enumerate() {
                let selected = self.brush_color == *c;
                let hover = ["ink", "ao — construction", "aka — correction", "white"][i];
                if plate::swatch(
                    ui,
                    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                    selected,
                )
                .on_hover_text(hover)
                .clicked()
                {
                    self.brush_color = *c;
                }
            }
            if !compact {
                ui.separator();
            }
            // (fit view + zoom live on the lightbox rail now — Phase 4.)
            // NOTHING VARIABLE-WIDTH GOES IN THIS ROW (canvas-stability law):
            // diagnostics live in the status bar; PLAYING is a canvas overlay.
        });

        // ---- OPTIONS row (spec defect 4): ALWAYS allocated at OPTIONS_H,
        // even when empty, so arming select or fill NEVER moves the strip
        // above — the armed tool's sub-elements land here and nowhere else.
        if self.raster {
            let full = vec2(ui.available_width(), plate::OPTIONS_H);
            ui.allocate_ui_with_layout(
                full,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_size(full);
                    match self.tool {
                        CanvasTool::Select => {
                            plate::legend(ui, "select");
                            if plate::tool_button(
                                ui,
                                self.sel_shape == SelShape::Lasso,
                                Icon::Lasso,
                                "lasso",
                            )
                            .on_hover_text("lasso selection")
                            .clicked()
                            {
                                self.sel_shape = SelShape::Lasso;
                            }
                            if plate::tool_button(
                                ui,
                                self.sel_shape == SelShape::Rect,
                                Icon::Marquee,
                                "rect",
                            )
                            .on_hover_text("rectangle selection")
                            .clicked()
                            {
                                self.sel_shape = SelShape::Rect;
                            }
                        }
                        CanvasTool::Fill => {
                            plate::legend(ui, "fill");
                            // Engraved label BEFORE, 64pt value (shiage:
                            // leak-chasing nudges these several times a cel).
                            plate::legend(ui, "gap");
                            let mut gap = self.fill_gap;
                            plate::field(ui, egui::DragValue::new(&mut gap).range(0..=8));
                            self.fill_gap = gap;
                            plate::legend(ui, "under");
                            let mut grow = self.fill_grow;
                            plate::field(ui, egui::DragValue::new(&mut grow).range(0..=4));
                            self.fill_grow = grow;
                            if plate::detent(ui, self.fill_ref_cel, "cel")
                                .on_hover_text(
                                    "boundaries come from the whole cel (all visible layers)",
                                )
                                .clicked()
                            {
                                self.fill_ref_cel = true;
                            }
                            if plate::detent(ui, !self.fill_ref_cel, "layer")
                                .on_hover_text("boundaries come from the active layer only")
                                .clicked()
                            {
                                self.fill_ref_cel = false;
                            }
                        }
                        CanvasTool::LassoFill => {
                            plate::legend(ui, "lasso fill");
                            plate::legend(ui, "softness");
                            let mut soft = self.lasso_soft;
                            plate::field(ui, egui::DragValue::new(&mut soft).range(0.0..=64.0));
                            self.lasso_soft = soft;
                            plate::legend(ui, "grow");
                            let mut grow = self.lasso_grow;
                            plate::field(
                                ui,
                                egui::DragValue::new(&mut grow).range(-16.0..=16.0),
                            );
                            self.lasso_grow = grow;
                        }
                        _ => {
                            // Brush / eraser edits live on the RIGHT RAIL
                            // (owner 2026-08-17): hover the canvas's right
                            // edge. The deck stays clear; the row stays
                            // claimed so nothing moves.
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("brush edits → hover the canvas's right edge")
                                    .size(10.5)
                                    .color(plate::legend_dim()),
                            );
                        }
                    }
                },
            );
        }
    }

    #[allow(clippy::too_many_arguments)] // one call site; a struct would be noise
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        mut paint: Option<&mut PaintLayer>,
        graph: crate::viewer::GraphView,
        pen: &PenConfig,
        layers_cfg: &LayersConfig,
        native_pen: &[PenSample],
        presets: &[BrushPreset],
    ) {
        self.pen_curve = pen.pressure_curve.clone();
        // PSD-brush-engine: install the armed preset's tip/grain on the
        // paint layer — once per arm, never mid-stroke (stroke_active
        // can't be true on the frame an arm click happened).
        if self.brush_res_dirty {
            if let Some(p) = paint.as_deref_mut() {
                self.brush_res_dirty = false;
                let (tip, frames, grain) = self.build_brush_resources();
                self.tip_active = tip.is_some();
                self.tip_frames = frames;
                // STAGE 1: the armed resources' wire form — content
                // hashed so a session announces each image once.
                self.armed_tip_wire = tip.as_ref().map(|(w, h, b)| {
                    (
                        WireRes {
                            hash: wire_hash(b),
                            w: *w,
                            h: *h,
                            frames,
                        },
                        std::sync::Arc::new(b.clone()),
                    )
                });
                self.armed_grain_wire = grain.as_ref().map(|(w, h, b, sc, st)| {
                    (
                        WireGrain {
                            hash: wire_hash(b),
                            w: *w,
                            h: *h,
                            scale: *sc,
                            strength: *st,
                        },
                        std::sync::Arc::new(b.clone()),
                    )
                });
                p.set_brush_resources(tip, frames, grain);
            }
        }
        // (The silent per-layer colour swap is DEAD — spec defect 16: the
        // brush colour never changes without a gesture. The paint dish and
        // the pencil box are the colour-arming gestures now.)
        let _ = layers_cfg;

        // The tool deck rides ON the drawing surface (owner's directive
        // 2026-08-17): brush controls are part of the instrument, not a
        // floating sub-window above it. The paper gets everything below.
        let deck_raster = paint.is_some();
        egui::Panel::top("canvas_tool_deck")
            .show_separator_line(false)
            .show(ui, |ui| {
                crate::plate::surface(ui);
                ui.add_space(2.0);
                self.brush_ui(ui, state, deck_raster, presets);
                ui.add_space(2.0);
            });

        // (The lightbox rail folds out from the LEFT edge now — owner's
        // law: nothing lives fixed in the canvas but the top deck. See the
        // fold-out beside the brush rail below.)
        let mut paint = paint;
        if paint.is_none() {
            self.raster = false;
        }

        // R-FLIP: momentary, polled (not an Action — actions fire on
        // press; the flip lives exactly as long as the key is down). Never
        // while typing, playing, mid-stroke, or in composite review.
        self.flip_held = ui.input(|i| i.key_down(egui::Key::R))
            && !ui.ctx().egui_wants_keyboard_input()
            && !self.stroke_active()
            && !state.view.playing
            && !self.composite_view;

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

        // Session presence: our pointer, in paper space, when over the paper.
        self.presence_pos = response
            .hover_pos()
            .map(|hp| to_paper(hp))
            .filter(|pp| pp.x >= 0.0 && pp.y >= 0.0 && pp.x <= paper_w && pp.y <= paper_h)
            .map(|pp| [pp.x, pp.y]);

        // Zoom at cursor / middle-drag pan — DISABLED during playback: the pen
        // resting or hovering on the drawing display (hover scroll deltas, a
        // barrel button mapped to middle-click) must not move the view while
        // the animation plays.
        if !state.view.playing {
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll.abs() > 0.0
                    && let Some(mouse) = response.hover_pos()
                {
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
        }

        // ---- Paper --------------------------------------------------------
        let paper_rect =
            Rect::from_min_max(to_screen(pos2(0.0, 0.0)), to_screen(pos2(paper_w, paper_h)));
        painter.rect_filled(
            paper_rect,
            2,
            if self.miss_check {
                crate::plate::GRAPHITE
            } else {
                crate::plate::PAPER
            },
        );
        painter.rect_stroke(
            paper_rect,
            2,
            egui::Stroke::new(1.0, crate::plate::legend_dim()),
            egui::StrokeKind::Outside,
        );
        // Paper furniture (Phase 4): field / safe / peg guides in Ao — the
        // scaffold ink, printed under the drawing like a real layout sheet.
        {
            let ao = Color32::from_rgba_unmultiplied(83, 137, 196, 110);
            let g = egui::Stroke::new(1.0, ao);
            if self.show_field {
                painter.rect_stroke(
                    egui::Rect::from_center_size(paper_rect.center(), paper_rect.size() * 0.9),
                    0.0,
                    g,
                    egui::StrokeKind::Inside,
                );
            }
            if self.show_safe {
                painter.rect_stroke(
                    egui::Rect::from_center_size(paper_rect.center(), paper_rect.size() * 0.8),
                    0.0,
                    g,
                    egui::StrokeKind::Inside,
                );
                let c = paper_rect.center();
                painter.line_segment([pos2(c.x - 10.0, c.y), pos2(c.x + 10.0, c.y)], g);
                painter.line_segment([pos2(c.x, c.y - 10.0), pos2(c.x, c.y + 10.0)], g);
            }
            if self.show_peg {
                // The peg bar: round hole centre, slot holes either side —
                // the registration a real sheet hangs on.
                let cy = paper_rect.bottom() - paper_rect.height() * 0.05;
                let cx = paper_rect.center().x;
                painter.circle_stroke(pos2(cx, cy), 5.0, g);
                for dx in [-70.0f32, 70.0] {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(pos2(cx + dx, cy), vec2(20.0, 9.0)),
                        4.0,
                        g,
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }

        // ---- Pen input (edit mode only) -----------------------------------
        if !state.view.playing {
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
                CanvasTool::LassoFill => {
                    self.lasso_fill_input(ui, &response, rect, &to_paper, state);
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
                    composite_note = Some("composite view — no graph output wired (C to edit)");
                }
                GV::EvalFailed => {
                    composite_note =
                        Some("composite view — the graph failed to evaluate (C to edit)");
                }
                GV::NoGpu => {
                    composite_note = Some("composite view needs the GPU — showing edit view");
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
        // live dabs, and read back into the engine at pen-up ----------
        // v0.2.1 (audit): remote replays + the live overlay must not
        // starve behind the composite lens — they run whenever the GPU
        // paint layer exists, whatever the view mode. The between-
        // strokes sync below restores the viewer's textures afterward.
        if self.raster
            && let Some(p) = paint.as_deref_mut()
        {
            p.ensure_size(state.engine.project.width, state.engine.project.height);
            self.run_remote_wet(p);
            self.run_replays(p, state);
        }
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
                // v0.2.1 (audit): an abandoned STREAMED stroke must tell
                // the peers, or their gathers + overlay leak all session.
                if self.stroke_streaming {
                    self.stroke_streaming = false;
                    self.stroke_outbox.push(StrokeMsg::Abort {
                        stroke_id: self.stroke_wire_id,
                    });
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
                // v0.2.1: incremental — only the stroke's new tail
                // computes (O(n), was O(n²) over long strokes).
                self.extend_stroke_dabs();
                if self.dab_cache.len() > self.dabs_flushed {
                    let new: Vec<Dab> = self.dab_cache[self.dabs_flushed..].to_vec();
                    // STAGE 1: stream exactly the NEW dabs — the wire's
                    // stroke unit (never pixels; NEVER-DO 1).
                    if self.stroke_streaming {
                        self.stroke_outbox.push(StrokeMsg::Dabs {
                            stroke_id: self.stroke_wire_id,
                            dabs: new.clone(),
                        });
                    }
                    if self.stroke_erasing {
                        p.paint(&new, PaintMode::Erase);
                        self.cel_touched = true;
                    } else if self.stroke_alpha_locked {
                        p.paint(&new, PaintMode::AlphaLock);
                        self.cel_touched = true;
                    } else {
                        p.paint_wet(&new);
                        self.wet_dirty = true;
                    }
                    self.dabs_flushed = self.dab_cache.len();
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
                    // STAGE 1/3 (PSD-multiplayer-rescope): a STREAMED
                    // guest stroke never commits here — the dab stream
                    // already told the host everything; End asks for the
                    // sequenced commit, and synced_active stays
                    // untouched so the GPU keeps showing this stroke
                    // until the echo replays it into engine truth. An
                    // UNSTREAMED guest stroke (a fresh cel) falls
                    // through to the local commit below as a PREDICTION:
                    // the command mirror carries it whole, and its new
                    // ids live in this guest's own 2^48 partition.
                    if self.is_guest && self.stroke_streaming {
                        let _ = &tiles; // pixels stay home (NEVER-DO 1)
                        self.stroke_outbox.push(StrokeMsg::End {
                            stroke_id: self.stroke_wire_id,
                        });
                        self.stroke_streaming = false;
                        state.status = "stroke sent — the host is inking it".into();
                        self.cel_touched = false;
                        self.current.clear();
                        self.dabs_flushed = 0;
                        self.raster_stroke_done = false;
                        return;
                    }
                    // Commit against the slot LATCHED at stroke start — a
                    // mid-stroke A-cycle must not retarget the readback.
                    // STAGE 1: a STREAMED stroke's PaintTiles must not
                    // ride the command mirror (that would ship pixels —
                    // NEVER-DO 1): the dab broadcast IS the stroke; the
                    // App turns replay_done into the sequenced End +
                    // hash audit for every peer.
                    let streamed = std::mem::take(&mut self.stroke_streaming);
                    // v0.2.3 (the X-SHEET LAW): the cel can vanish MID-
                    // STROKE (a sequenced delete or undo). Then this
                    // commit CREATES structure (AddDrawing + SetCell —
                    // an X-sheet entry) which MUST ride the command
                    // mirror whole, or the room forks silently; the
                    // streamed stroke is aborted for the peers instead
                    // (their gathers target a dead drawing).
                    let target_alive = state.own_key_drawing().is_some();
                    let stream_commit = streamed && target_alive;
                    if streamed && !target_alive {
                        self.stroke_outbox.push(StrokeMsg::Abort {
                            stroke_id: self.stroke_wire_id,
                        });
                    }
                    let changed: Vec<(i32, i32)> = if stream_commit {
                        let base = state.active_layer_tiles().cloned().unwrap_or_default();
                        diff_guest_tiles(&base, &tiles)
                            .into_iter()
                            .map(|(x, y, _)| (x, y))
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let was_mirror = state.engine.mirror_log;
                    if stream_commit {
                        state.engine.mirror_log = false;
                    }
                    let committed = state.commit_raster(tiles, self.stroke_layer_slot);
                    state.engine.mirror_log = was_mirror;
                    if let Some((id, layer)) = committed {
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
                        if stream_commit {
                            let layer_name = state
                                .cut()
                                .drawing(id)
                                .and_then(|d| d.layer(layer))
                                .map(|l| l.props.name.clone())
                                .unwrap_or_default();
                            self.replay_done.push(ReplayDone {
                                stroke_id: self.stroke_wire_id,
                                author: String::new(),
                                drawing: id.0,
                                layer_name,
                                changed,
                                ok: true,
                                why: String::new(),
                            });
                        }
                    } else {
                        // Commit refused: the GPU layer holds a stroke the
                        // engine never accepted — invalidate so the next frame
                        // restores truth, not a phantom.
                        self.synced_active = (u64::MAX, u64::MAX, u64::MAX);
                        if stream_commit {
                            self.replay_done.push(ReplayDone {
                                stroke_id: self.stroke_wire_id,
                                author: String::new(),
                                drawing: 0,
                                layer_name: String::new(),
                                changed: Vec::new(),
                                ok: false,
                                why: "the commit was refused".into(),
                            });
                        }
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
            let pin = state.view.ghost_pin;
            if state.view.onion || pin.is_some() || self.flip_held {
                let neighbors = state.onion_neighbors();
                // Computed once per frame, not per neighbour: the active
                // layer's role name is what "line-only ghost" locks onto.
                // The FLIP shows the whole previous cel — no filter.
                let filter = (!self.flip_held && state.view.onion_layer_only)
                    .then(|| state.active_layer_name());
                // Slot 0 = the behind-neighbour (onion; the FLIP borrows it
                // at full identity). Slot 1 = THE GHOST PIN when set (the
                // pinned scaffold outranks the forward neighbour); else
                // onion's next drawing.
                let s0 = if self.flip_held || state.view.onion {
                    neighbors[0]
                } else {
                    None
                };
                let s1 = pin.or(if state.view.onion { neighbors[1] } else { None });
                for (slot, nid) in [s0, s1].into_iter().enumerate() {
                    match nid.and_then(|id| state.drawing_composite(id, filter.as_deref())) {
                        Some((slices, hash)) => p.set_onion(slot, Some(&slices), hash),
                        None => p.set_onion(slot, None, 0),
                    }
                }
            } else {
                p.set_onion(0, None, 0);
                p.set_onion(1, None, 0);
            }

            // ---- Multi-column raster display (B4): every OTHER column's
            // own resolved cel, refreshed every frame like onion (cheap —
            // sync_other_column is a no-op when the content hash matches).
            // Prune first so a removed column's texture doesn't leak.
            let live_cols: Vec<anim_core::ids::ColumnId> =
                state.cut().xsheet.columns.iter().map(|c| c.id).collect();
            p.prune_other_columns(&live_cols);
            for &col in &live_cols {
                if col == state.view.active_column {
                    continue;
                }
                let composite = state.column_composite(col);
                p.sync_other_column(col, composite.as_ref().map(|(s, h)| (s.as_slice(), *h)));
            }
        }

        // ---- Render layers ------------------------------------------------
        let cut = state.cut();
        let active_col = state.view.active_column;
        let frame = state.view.frame;

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
                "composite view",
                egui::FontId::proportional(12.0),
                Color32::from_rgb(139, 141, 131), // Legend: the app speaking
            );
        }
        if let Some(note) = composite_note {
            painter.text(
                pos2(rect.center().x, rect.top() + 34.0),
                egui::Align2::CENTER_CENTER,
                note,
                egui::FontId::proportional(13.0),
                Color32::from_rgb(228, 82, 47),
                // Aka: overrules the hand
            );
        }

        // 1. Non-active columns (in sheet order = layer order): each
        // column's own resolved RASTER cel (B4 — was invisible in edit view
        // before; only composite view showed it) plus any legacy vector
        // strokes. Reborrow, not move — `paint` is consumed by value at 3b
        // below, which must still see it.
        if edit_view {
            let other_tex = paint.as_deref(); // Option<&PaintLayer>, Copy
            for col in &cut.xsheet.columns {
                if col.id == active_col {
                    continue;
                }
                if self.raster
                    && let Some(id) = other_tex.and_then(|p| p.other_column_id(col.id))
                {
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    painter.image(id, paper_rect, uv, Color32::WHITE);
                }
                if let Some(id) = col.resolve(frame)
                    && let Some(d) = cut.drawing(id)
                {
                    draw_strokes(
                        &painter,
                        &d.strokes,
                        &to_screen,
                        scale,
                        None,
                        &self.pen_curve,
                    );
                }
            }
        }

        // 2a. THE GHOST PIN (vector path): the pinned drawing haunts every
        // frame in Ao at the onion strength — display only, per its room.
        if edit_view
            && !self.flip_held
            && let Some(pid) = state.view.ghost_pin
            && state.resolve_at(active_col, frame) != Some(pid)
            && let Some(d) = cut.drawing(pid)
        {
            let a = (255.0 * self.onion_strength.clamp(0.0, 1.0)) as u8;
            draw_strokes(
                &painter,
                &d.strokes,
                &to_screen,
                scale,
                Some(Color32::from_rgba_unmultiplied(83, 137, 196, a)),
                &self.pen_curve,
            );
        }

        // 2. Onion ghosts of the active column, under its current drawing.
        if edit_view && state.view.onion && !self.flip_held {
            let ghosts = onion_ghosts(state, active_col, frame, self.onion_strength);
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

        // 3. Active column's shown drawing on top — the CURRENT one, or
        // the PREVIOUS while the flip is held (full strength: the flip is
        // the drawing itself, never a ghost).
        if edit_view {
            let shown = if self.flip_held {
                state.onion_neighbors()[0]
            } else {
                state.resolve_at(active_col, frame)
            };
            if let Some(id) = shown
                && let Some(d) = state.cut().drawing(id)
            {
                draw_strokes(
                    &painter,
                    &d.strokes,
                    &to_screen,
                    scale,
                    None,
                    &self.pen_curve,
                );
            }
        }

        // 3b. The raster sandwich, bottom→top: onion ghosts, BELOW projection,
        //     the ACTIVE layer at its own opacity, the live wet stroke, the
        //     ABOVE projection. Painting "color" previews under the line art
        //     live — the solo shiage workflow.
        if edit_view
            && self.raster
            && let Some(p) = paint
        {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            // Ghost BEHIND leans Ao (continuity), ghost AHEAD leans
            // Legend (spec defect 14) — as multiply tints for now; the
            // true re-ink lands on the VECTOR path in Phase 4, and the
            // raster blit shader stays untouched (Do-NOT-build).
            let ga = (255.0 * self.onion_strength.clamp(0.0, 1.0)) as u8;
            if self.flip_held {
                // THE FLIP: the previous drawing full-strength, alone —
                // the current cel steps aside until R lifts.
                if let Some(id) = p.onion_id(0) {
                    painter.image(id, paper_rect, uv, Color32::WHITE);
                }
            } else {
                if let Some(id) = p.onion_id(0) {
                    painter.image(
                        id,
                        paper_rect,
                        uv,
                        Color32::from_rgba_unmultiplied(160, 195, 232, ga),
                    );
                }
                if let Some(id) = p.onion_id(1) {
                    // The pin is SCAFFOLD — it ghosts in Ao; the forward
                    // neighbour keeps its quiet Legend lean.
                    let t = if state.view.ghost_pin.is_some() {
                        (160u8, 195u8, 232u8)
                    } else {
                        (205u8, 206u8, 199u8)
                    };
                    painter.image(
                        id,
                        paper_rect,
                        uv,
                        Color32::from_rgba_unmultiplied(t.0, t.1, t.2, ga),
                    );
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
                // STAGE 2: other artists' LIVE ink, straight from their
                // dab streams (alpha pre-scaled; cleared at commit).
                if !self.remote_live.is_empty() {
                    painter.image(p.remote_id(), paper_rect, uv, Color32::WHITE);
                }
            }
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
            && !state.view.playing
            && state.current_drawing().is_none()
            && self.current.is_empty()
        {
            painter.text(
                paper_rect.center(),
                egui::Align2::CENTER_CENTER,
                "empty cell — draw to create a new drawing here",
                egui::FontId::proportional(15.0),
                Color32::from_rgb(139, 141, 131),
                // Legend
            );
        }

        // Playback indicator as a canvas OVERLAY (painted, zero layout impact
        // — a toolbar label here used to reflow the row and shift the view).
        if self.flip_held {
            painter.text(
                pos2(rect.center().x, rect.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "FLIP — previous drawing (release R)",
                egui::FontId::proportional(13.0),
                Color32::from_rgb(139, 141, 131), // Legend: the app speaking
            );
        }
        if state.view.playing {
            painter.text(
                pos2(rect.center().x, rect.top() + 16.0),
                egui::Align2::CENTER_CENTER,
                "PLAYING — space to stop",
                egui::FontId::proportional(13.0),
                Color32::from_rgb(139, 141, 131), // Legend: the app speaking
            );
        }

        // The refusal reaches the paper: a fresh refusal flashes the
        // canvas edge in Aka, so NO is seen from the pen tip (Foot law).
        if state.refusal_seq != self.flash_seq {
            self.flash_seq = state.refusal_seq;
            self.flash_since = ui.input(|i| i.time);
        }
        let fage = ui.input(|i| i.time) - self.flash_since;
        if fage < 0.9 && state.refusal_seq != 0 {
            let fppp = ui.ctx().pixels_per_point();
            painter.rect_stroke(
                paper_rect,
                2.0,
                egui::Stroke::new(crate::plate::device_px(2.0, fppp), crate::plate::AKA),
                egui::StrokeKind::Outside,
            );
            ui.ctx().request_repaint();
        }

        // ---- LASSO FILL preview: the loop so far, Ao dashed, closed.
        if self.tool == CanvasTool::LassoFill && self.lasso_pts.len() >= 2 {
            let mut pts: Vec<Pos2> =
                self.lasso_pts.iter().map(|p| to_screen(*p)).collect();
            pts.push(pts[0]);
            painter.extend(egui::Shape::dashed_line(
                &pts,
                egui::Stroke::new(1.0, plate::AO),
                4.0,
                3.0,
            ));
        }

        // ---- SESSION PRESENCE (PSD-session-room): the other artists.
        // Same-frame peers draw at full presence — Ao cursor ring, name
        // tag, live wet ghost; peers on other frames are named at the
        // paper's top edge in Legend (you know where they are, they never
        // paint over your frame).
        if !self.peers.is_empty() {
            let mut away = 0;
            for peer in &self.peers.clone() {
                if peer.frame == state.view.frame {
                    if let Some(c) = peer.cursor {
                        let sp = to_screen(pos2(c[0], c[1]));
                        painter.circle_stroke(sp, 5.0, egui::Stroke::new(1.5, crate::plate::AO));
                        if peer.pen_down {
                            painter.circle_filled(sp, 2.0, crate::plate::AO);
                        }
                        painter.text(
                            pos2(sp.x + 8.0, sp.y - 8.0),
                            egui::Align2::LEFT_BOTTOM,
                            &peer.name,
                            egui::FontId::new(10.0, crate::plate::semibold()),
                            crate::plate::AO,
                        );
                    }
                    if peer.wet.len() >= 2 {
                        let pts: Vec<Pos2> = peer
                            .wet
                            .iter()
                            .map(|p| to_screen(pos2(p[0], p[1])))
                            .collect();
                        painter.add(egui::Shape::line(
                            pts,
                            egui::Stroke::new(
                                2.0,
                                Color32::from_rgba_unmultiplied(83, 137, 196, 200),
                            ),
                        ));
                    }
                } else {
                    painter.text(
                        pos2(
                            paper_rect.left() + 8.0,
                            paper_rect.top() + 10.0 + away as f32 * 14.0,
                        ),
                        egui::Align2::LEFT_CENTER,
                        format!("{} · fr {}", peer.name, peer.frame + 1),
                        egui::FontId::new(10.0, crate::plate::semibold()),
                        crate::plate::legend_dim(),
                    );
                    away += 1;
                }
            }
        }

        // ---- THE RULE's mirror (spec §5.4): the active column's run
        // marks at the canvas's left edge, at the sheet's own 18pt pitch —
        // "how long is this hold, am I on ones, which frame" answered in
        // peripheral vision at the pen tip, like a VU meter at a desk.
        if !self.composite_view {
            let count = state.frame_count();
            let strip = Rect::from_min_max(
                pos2(rect.left(), rect.top() + 6.0),
                pos2(rect.left() + 14.0, rect.bottom() - 6.0),
            );
            let mp = painter.with_clip_rect(strip);
            mp.rect_filled(strip, 0.0, crate::plate::WELL);
            let ppp = ui.ctx().pixels_per_point();
            let fps = state.fps();
            let total = count as f32 * 18.0;
            // The whole cut fits, or the strip follows the playhead with
            // the current frame held at 40% height.
            let origin = if total <= strip.height() {
                strip.top()
            } else {
                (strip.top() + strip.height() * 0.4 - state.view.frame as f32 * 18.0)
                    .clamp(strip.bottom() - total, strip.top())
            };
            for f in 0..=count {
                if let Some(tier) = crate::runs::rule_tier(f, fps) {
                    let y = crate::plate::snap(origin + f as f32 * 18.0, ppp);
                    let (w, ink, x0) = match tier {
                        2 => (
                            crate::plate::device_px(2.0, ppp),
                            crate::plate::rule_sec(),
                            strip.left(),
                        ),
                        1 => (
                            crate::plate::device_px(1.0, ppp),
                            crate::plate::rule_half(),
                            strip.left() + 3.0,
                        ),
                        _ => (
                            crate::plate::device_px(1.0, ppp),
                            crate::plate::rule_beat(),
                            strip.left() + 6.0,
                        ),
                    };
                    mp.line_segment(
                        [pos2(x0, y), pos2(strip.right(), y)],
                        egui::Stroke::new(w, ink),
                    );
                }
            }
            let active = state.view.active_column;
            let cut = state.cut();
            if let Some(col) = cut.xsheet.columns.iter().find(|c| c.id == active) {
                let marks = crate::runs::column_marks(count, |f| col.key_at(f));
                let cx = crate::plate::snap(strip.center().x, ppp);
                for r in &marks.runs {
                    let y0 = crate::plate::snap(origin + r.start as f32 * 18.0 + 9.0, ppp);
                    let y1 = crate::plate::snap(origin + (r.end + 1) as f32 * 18.0, ppp) - 1.0;
                    mp.circle_filled(pos2(cx, y0), 2.5, crate::plate::TALLY);
                    if y1 > y0 + 2.5 {
                        mp.line_segment(
                            [pos2(cx, y0 + 2.5), pos2(cx, y1)],
                            egui::Stroke::new(crate::plate::device_px(2.0, ppp), crate::plate::AO),
                        );
                    }
                }
                for f in &marks.empties {
                    let y = crate::plate::snap(origin + *f as f32 * 18.0 + 9.0, ppp);
                    mp.circle_stroke(
                        pos2(cx, y),
                        2.5,
                        egui::Stroke::new(1.0, crate::plate::LEGEND),
                    );
                }
            }
            // The current frame: the strip's one Tally notch.
            let ncy = origin + (state.view.frame as f32 + 0.5) * 18.0;
            mp.add(egui::Shape::convex_polygon(
                vec![
                    pos2(strip.left(), ncy - 4.0),
                    pos2(strip.left() + 5.0, ncy),
                    pos2(strip.left(), ncy + 4.0),
                ],
                crate::plate::TALLY,
                egui::Stroke::NONE,
            ));
        }

        // ---- The brush rail (owner 2026-08-17): brush editing lives at
        // the canvas's RIGHT edge, hidden until reached for. It floats as
        // an overlay so the paper NEVER rescales when it opens. It cannot
        // open mid-stroke, and it stays open through a slider drag.
        const RAIL_W: f32 = 210.0;
        if self.raster {
            let hover = ui.ctx().pointer_hover_pos();
            let zone_w = if self.rail_open { RAIL_W + 12.0 } else { 16.0 };
            let in_zone = hover.is_some_and(|hp| {
                hp.x >= rect.right() - zone_w
                    && hp.x <= rect.right() + 1.0
                    && hp.y >= rect.top()
                    && hp.y <= rect.bottom()
            });
            let dragging = ui.input(|i| i.pointer.any_down());
            self.rail_open = if self.rail_open {
                in_zone || dragging
            } else {
                in_zone && !dragging && !self.stroke_active()
            };
            // The slide (owner 2026-08-17): in and out like a toolbar
            // should — 160ms, overlay only, so the paper never rescales.
            let t = ui.ctx().animate_bool_with_time(
                egui::Id::new("brush_rail_slide"),
                self.rail_open,
                0.16,
            );
            if t > 0.001 {
                let x = rect.right() - RAIL_W * t;
                let panel = Rect::from_min_size(pos2(x, rect.top()), vec2(RAIL_W, rect.height()));
                egui::Area::new(egui::Id::new("canvas_brush_rail"))
                    .fixed_pos(panel.min)
                    .constrain(false)
                    .show(ui.ctx(), |aui| {
                        // Clipped to the paper area: the slide emerges from
                        // the edge and nothing spills over neighbours.
                        aui.set_clip_rect(rect);
                        let pad = ((panel.height() - self.rail_content_h) * 0.5).max(0.0);
                        // The backing is sized to the CONTENT, not the
                        // column (owner 2026-08-17): readable text, without
                        // walling off the paper above and below the stack.
                        let back = Rect::from_min_size(
                            pos2(panel.left() + 2.0, panel.top() + pad - 12.0),
                            vec2(RAIL_W - 8.0, self.rail_content_h + 24.0),
                        );
                        aui.painter().rect_filled(
                            back,
                            0.0,
                            Color32::from_rgba_unmultiplied(14, 15, 13, 242),
                        );
                        aui.painter().rect_stroke(
                            back,
                            0.0,
                            egui::Stroke::new(1.0, crate::plate::legend_dim()),
                            egui::StrokeKind::Inside,
                        );
                        aui.allocate_ui_with_layout(
                            panel.size(),
                            egui::Layout::top_down(egui::Align::Center),
                            |aui| {
                                aui.set_min_size(panel.size());
                                aui.add_space(pad);
                                // Measure the STACK itself by allocation
                                // cursor, immune to min_rect inflation (the
                                // previous measurement fed back on itself
                                // and decayed the pad to zero — the bug the
                                // owner's screenshot caught).
                                let y0 = aui.next_widget_position().y;
                                self.brush_rail_ui(aui, presets);
                                let y1 = aui.next_widget_position().y;
                                self.rail_content_h = (y1 - y0).max(50.0);
                            },
                        );
                    });
            } else {
                // The grip: a quiet affordance while the rail is hidden.
                let gx = rect.right() - 6.0;
                let gcy = rect.center().y;
                for dy in [-9.0f32, 0.0, 9.0] {
                    painter.line_segment(
                        [pos2(gx, gcy + dy - 3.0), pos2(gx, gcy + dy + 3.0)],
                        egui::Stroke::new(2.0, crate::plate::legend_dim()),
                    );
                }
            }
        }

        // ---- THE LIGHTBOX FOLD-OUT (owner's law, 2026-08-17): the left
        // edge mirrors the right — hover to slide out, clipped to the
        // paper, vertically centred, content-sized backing. Onion, paper
        // furniture, view, and the INPUT plate field live here.
        const LIGHT_W: f32 = 136.0;
        {
            let hover = ui.ctx().pointer_hover_pos();
            let zone_w = if self.light_open {
                LIGHT_W + 12.0
            } else {
                16.0
            };
            let in_zone = hover.is_some_and(|hp| {
                hp.x >= rect.left() - 1.0
                    && hp.x <= rect.left() + zone_w
                    && hp.y >= rect.top()
                    && hp.y <= rect.bottom()
            });
            let dragging = ui.input(|i| i.pointer.any_down());
            self.light_open = if self.light_open {
                in_zone || dragging
            } else {
                in_zone && !dragging && !self.stroke_active()
            };
            let t = ui.ctx().animate_bool_with_time(
                egui::Id::new("lightbox_slide"),
                self.light_open,
                0.16,
            );
            if t > 0.001 {
                let x = rect.left() - LIGHT_W * (1.0 - t);
                let panel = Rect::from_min_size(pos2(x, rect.top()), vec2(LIGHT_W, rect.height()));
                egui::Area::new(egui::Id::new("canvas_lightbox_rail"))
                    .fixed_pos(panel.min)
                    .constrain(false)
                    .show(ui.ctx(), |aui| {
                        aui.set_clip_rect(rect);
                        let pad = ((panel.height() - self.light_content_h) * 0.5).max(0.0);
                        let back = Rect::from_min_size(
                            pos2(panel.left() + 2.0, panel.top() + pad - 12.0),
                            vec2(LIGHT_W - 8.0, self.light_content_h + 24.0),
                        );
                        aui.painter().rect_filled(
                            back,
                            0.0,
                            Color32::from_rgba_unmultiplied(14, 15, 13, 242),
                        );
                        aui.painter().rect_stroke(
                            back,
                            0.0,
                            egui::Stroke::new(1.0, crate::plate::legend_dim()),
                            egui::StrokeKind::Inside,
                        );
                        aui.allocate_ui_with_layout(
                            panel.size(),
                            egui::Layout::top_down(egui::Align::Center),
                            |aui| {
                                aui.set_min_size(panel.size());
                                aui.add_space(pad);
                                let y0 = aui.next_widget_position().y;
                                self.lightbox_rail_ui(aui, state);
                                let y1 = aui.next_widget_position().y;
                                self.light_content_h = (y1 - y0).max(50.0);
                            },
                        );
                    });
            } else {
                // The left grip, clear of the mirror strip.
                let gx = rect.left() + 20.0;
                let gcy = rect.center().y;
                for dy in [-9.0f32, 0.0, 9.0] {
                    painter.line_segment(
                        [pos2(gx, gcy + dy - 3.0), pos2(gx, gcy + dy + 3.0)],
                        egui::Stroke::new(2.0, crate::plate::legend_dim()),
                    );
                }
            }
        }

        // ---- THE PAINT DISH (its room, 2026-08-17): colour folds UP from
        // the bottom edge — the rendering stage's instrument. Fold law
        // inherited whole; contents centred on the grip's vertical line.
        const DISH_H: f32 = 150.0;
        if self.raster {
            let hover = ui.ctx().pointer_hover_pos();
            let zone_h = if self.dish_open { DISH_H + 12.0 } else { 16.0 };
            let in_zone = hover.is_some_and(|hp| {
                hp.y >= rect.bottom() - zone_h
                    && hp.y <= rect.bottom() + 1.0
                    && hp.x >= rect.left()
                    && hp.x <= rect.right()
            });
            let dragging = ui.input(|i| i.pointer.any_down());
            self.dish_open = if self.dish_open {
                in_zone || dragging
            } else {
                in_zone && !dragging && !self.stroke_active()
            };
            let t = ui.ctx().animate_bool_with_time(
                egui::Id::new("paint_dish_slide"),
                self.dish_open,
                0.16,
            );
            if t > 0.001 {
                let y = rect.bottom() - DISH_H * t;
                let panel = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), DISH_H));
                egui::Area::new(egui::Id::new("canvas_paint_dish"))
                    .fixed_pos(panel.min)
                    .constrain(false)
                    .show(ui.ctx(), |aui| {
                        aui.set_clip_rect(rect);
                        let pad = ((panel.width() - self.dish_content_w) * 0.5).max(0.0);
                        let back = Rect::from_min_size(
                            pos2(panel.left() + pad - 12.0, panel.top() + 2.0),
                            vec2(self.dish_content_w + 24.0, DISH_H - 6.0),
                        );
                        aui.painter().rect_filled(
                            back,
                            0.0,
                            Color32::from_rgba_unmultiplied(14, 15, 13, 242),
                        );
                        aui.painter().rect_stroke(
                            back,
                            0.0,
                            egui::Stroke::new(1.0, crate::plate::legend_dim()),
                            egui::StrokeKind::Inside,
                        );
                        aui.allocate_ui_with_layout(
                            panel.size(),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |aui| {
                                aui.set_min_size(panel.size());
                                aui.add_space(pad);
                                let x0 = aui.next_widget_position().x;
                                self.dish_ui(aui, state);
                                let x1 = aui.next_widget_position().x;
                                self.dish_content_w = (x1 - x0).max(240.0);
                            },
                        );
                    });
            } else {
                // The dish grip: bottom centre.
                let gy = rect.bottom() - 6.0;
                let gcx = rect.center().x;
                for dx in [-9.0f32, 0.0, 9.0] {
                    painter.line_segment(
                        [pos2(gcx + dx - 3.0, gy), pos2(gcx + dx + 3.0, gy)],
                        egui::Stroke::new(2.0, crate::plate::legend_dim()),
                    );
                }
            }
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
        && !state.view.playing
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
            let t_max = if self.dyn_size {
                self.pen_curve.apply(1.0)
            } else {
                1.0
            };
            let min_s = self.min_size.clamp(0.0, 1.0);
            let strength = self.tilt_strength.clamp(0.0, 1.0);
            let tmag = (self.smoothed_tilt[0].powi(2) + self.smoothed_tilt[1].powi(2)).sqrt();
            let tn = (tmag / TILT_MAX_RAD).clamp(0.0, 1.0);
            let broaden = if self.tilt_size {
                1.0 + strength * tn
            } else {
                1.0
            };
            let r =
                (self.raster_brush_px * (min_s + (1.0 - min_s) * t_max) * broaden * scale * 0.5)
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
                ellipse(
                    (r * aspect - 1.0).max(0.5),
                    (r - 1.0).max(0.5),
                    Color32::from_white_alpha(170),
                );
            } else {
                painter.circle_stroke(
                    pos,
                    r,
                    egui::Stroke::new(1.0, Color32::from_black_alpha(170)),
                );
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

    /// Load the armed preset's tip + grain images (PSD-brush-engine).
    /// Stamp tips come from the brush_tips cache; auto tips rasterize
    /// deterministically from their MaskGenerator recipe; grain from the
    /// brush_grains cache.
    fn build_brush_resources(
        &self,
    ) -> (
        Option<(u32, u32, Vec<u8>)>,
        u32,
        Option<(u32, u32, Vec<u8>, f32, f32)>,
    ) {
        let Some(e) = &self.brush_engine else {
            return (None, 1, None);
        };
        let mut frames = 1u32;
        let tip = if let Some(key) = &e.tip_key {
            crate::kpp::tips_dir().and_then(|d| {
                let img = load_cache_png(&d.join(format!("{key}.png")))?;
                // A GIH atlas carries its frame count in a sidecar.
                frames = std::fs::read_to_string(d.join(format!("{key}.frames")))
                    .ok()
                    .and_then(|t| t.trim().parse().ok())
                    .unwrap_or(1);
                Some(img)
            })
        } else {
            e.auto.as_ref().map(|a| rasterize_auto_tip(a, 256))
        };
        let grain = e.grain_key.as_ref().and_then(|key| {
            let img = crate::kpp::grains_dir()
                .and_then(|d| load_cache_png(&d.join(format!("{key}.png"))))?;
            Some((img.0, img.1, img.2, e.grain_scale, e.grain_strength))
        });
        (tip, frames.max(1), grain)
    }

    /// THE SMUDGE GATE: snapshot the active layer's committed tiles at
    /// stroke start (Arc clones — the pre-stroke truth, NEVER-DO 1).
    /// None when the armed engine doesn't smudge.
    fn capture_smudge_src(
        &self,
        state: &AppState,
    ) -> Option<std::collections::BTreeMap<anim_core::raster::TileCoord, std::sync::Arc<anim_core::raster::TileData>>>
    {
        let e = self.brush_engine.as_ref()?;
        if e.smudge_rate <= 0.0 {
            return None;
        }
        let id = state.own_key_drawing()?;
        let d = state.cut().drawing(id)?;
        let slot = self.stroke_layer_slot.min(d.layers.len().saturating_sub(1));
        let layer = &d.layers[d.layers.len() - 1 - slot];
        Some(layer.raster.tiles.clone())
    }

    /// STAGE 1 (PSD-multiplayer-rescope): latch + announce a streaming
    /// stroke (both roles). A stroke on a not-yet-existing cel does NOT
    /// stream — the command mirror carries that boundary case whole
    /// (host: its own ids; guest: a local PREDICTION whose fresh ids
    /// live in the guest's own partition — see the pen-up path).
    fn begin_stream(&mut self, state: &mut AppState) {
        self.stroke_streaming = false;
        if !(self.session_live && self.raster) {
            return;
        }
        let Some(did) = state.own_key_drawing() else {
            return;
        };
        // The LATCHED slot's layer name — a mid-stroke A-cycle must not
        // retarget the wire (same law as the local commit).
        let layer_name = state
            .cut()
            .drawing(did)
            .and_then(|d| {
                let slot = self.stroke_layer_slot.min(d.layers.len().saturating_sub(1));
                d.layers.get(d.layers.len() - 1 - slot)
            })
            .map(|l| l.props.name.clone());
        let Some(layer_name) = layer_name else {
            return;
        };
        self.stroke_next_id = self.stroke_next_id.wrapping_add(1);
        self.stroke_wire_id = self.stroke_salt.wrapping_add(self.stroke_next_id);
        self.stroke_streaming = true;
        self.stroke_outbox.push(StrokeMsg::Begin(StrokeBeginInfo {
            stroke_id: self.stroke_wire_id,
            drawing: did.0,
            layer_name,
            mode: if self.stroke_erasing {
                1
            } else if self.stroke_alpha_locked {
                2
            } else {
                0
            },
            opacity: self.brush_opacity,
            tip: if self.tip_active {
                self.armed_tip_wire.clone()
            } else {
                None
            },
            grain: self.armed_grain_wire.clone(),
        }));
    }

    /// STAGE 2: drain incoming remote dab batches into the overlay and
    /// clear it once every live remote stroke's commit has replayed.
    /// Preview approximations (room-documented): alpha carries the
    /// stroke opacity per dab; tip masks preview as the procedural
    /// falloff. The COMMIT replays them exactly.
    fn run_remote_wet(&mut self, p: &mut PaintLayer) {
        // v0.2.1 (audit): a room that ended mid-stroke must not leave
        // ghost ink parked in the overlay forever.
        if !self.session_live && !self.remote_live.is_empty() {
            self.remote_live.clear();
            self.remote_wet_inbox.clear();
            self.remote_wet_end.clear();
            p.clear_remote();
            return;
        }
        for (stroke_id, opacity, mut dabs) in std::mem::take(&mut self.remote_wet_inbox) {
            self.remote_live.insert(stroke_id);
            for d in &mut dabs {
                d.color[3] *= opacity.clamp(0.0, 1.0);
                d.tip = 0.0;
            }
            p.paint_remote(&dabs);
        }
        let mut any_ended = false;
        for id in std::mem::take(&mut self.remote_wet_end) {
            self.remote_live.remove(&id);
            any_ended = true;
        }
        if any_ended && self.remote_live.is_empty() {
            p.clear_remote();
        }
    }

    /// STAGE 1: replay queued remote strokes through the SAME machinery
    /// local strokes use — sync the target layer's engine tiles into the
    /// ACTIVE target, stamp the dabs, composite, read back, commit with
    /// the author's name. Never under a live pen. Afterward the viewer's
    /// layer and armed brush are restored (keys invalidated; the sync
    /// block runs in this same frame).
    fn run_replays(&mut self, p: &mut PaintLayer, state: &mut AppState) {
        if self.replay_inbox.is_empty() {
            return;
        }
        let mut did_stroke = false;
        let mut did_any = false;
        loop {
            // v0.2.3: strokes hijack the ACTIVE target — never under a
            // live pen; and NOTHING may overtake a waiting stroke (the
            // one order is the whole point).
            if matches!(self.replay_inbox.front(), Some(SeqTask::Stroke(_)))
                && self.touch_active
            {
                break;
            }
            let Some(task) = self.replay_inbox.pop_front() else {
                break;
            };
            did_any = true;
            match task {
                SeqTask::Stroke(rs) => {
                    let done = self.replay_one(p, state, &rs);
                    // STAGE 2: the commit landed (or died) — the live
                    // overlay for this stroke drops next drain.
                    self.remote_wet_end.push(rs.stroke_id);
                    crate::net::slog(format!(
                        "REPLAY stroke={} author={} dabs={} ok={} changed={} {}",
                        rs.stroke_id,
                        rs.author,
                        rs.dabs.len(),
                        done.ok,
                        done.changed.len(),
                        done.why
                    ));
                    self.replay_done.push(done);
                    did_stroke = true;
                }
                SeqTask::Cmds { origin, mut cmds } => {
                    // Befores rebuild HERE — against the document as of
                    // APPLICATION, after every queued stroke before this
                    // batch has landed.
                    state.rebuild_paint_befores(&mut cmds);
                    let n = cmds.len();
                    let was = state.engine.mirror_log;
                    state.engine.mirror_log = false;
                    let prev = state.engine.author();
                    state.engine.set_author(Some(origin.clone()));
                    let r = state.engine.apply("remote edit", cmds);
                    state.engine.set_author(prev);
                    state.engine.mirror_log = was;
                    match r {
                        Ok(()) => {
                            state.doc_gen = state.doc_gen.wrapping_add(1);
                            crate::net::slog(format!("APPLY cmds from {origin} n={n}"));
                        }
                        Err(e) => self
                            .seq_task_fail
                            .push(format!("cmds from {origin}: {e:?}")),
                    }
                }
                SeqTask::Undo { author, redo } => {
                    let was = state.engine.mirror_log;
                    state.engine.mirror_log = false;
                    let r = state.remote_history(&author, redo);
                    state.engine.mirror_log = was;
                    match r {
                        Ok(()) => {
                            crate::net::slog(format!("APPLY undone {author} redo={redo}"));
                        }
                        Err(why) => self
                            .seq_task_fail
                            .push(format!("undone {author}: {why}")),
                    }
                }
            }
        }
        if did_stroke {
            // The hijack armed the STROKE's tip/grain: restore the
            // viewer's brush.
            self.brush_res_dirty = true;
        }
        if did_any {
            // Engine truth moved (strokes, timeline edits, undos alike)
            // — restore every viewer texture from it.
            self.synced_active = (u64::MAX, u64::MAX, u64::MAX);
            self.synced_below = None;
            self.synced_above = None;
        }
    }

    fn replay_one(&mut self, p: &mut PaintLayer, state: &mut AppState, rs: &ReplayStroke) -> ReplayDone {
        let mk = |ok: bool, changed: Vec<(i32, i32)>, why: &str| ReplayDone {
            stroke_id: rs.stroke_id,
            author: rs.author.clone(),
            drawing: rs.drawing,
            layer_name: rs.layer_name.clone(),
            changed,
            ok,
            why: why.into(),
        };
        let did = anim_core::ids::DrawingId(rs.drawing);
        // v0.2.1 (audit): located PROJECT-WIDE — a stroke must land no
        // matter which cut this viewer is browsing.
        let base = match state.locate_layer(did, &rs.layer_name) {
            Some(l) => l.raster.tiles.clone(),
            None => return mk(false, Vec::new(), "the drawing/layer is gone"),
        };
        p.sync_active(&base);
        p.set_brush_resources(
            rs.tip.as_ref().map(|(w, h, b, _)| (*w, *h, b.as_ref().clone())),
            rs.tip.as_ref().map(|(_, _, _, f)| *f).unwrap_or(1),
            rs.grain
                .as_ref()
                .map(|(w, h, b, sc, st)| (*w, *h, b.as_ref().clone(), *sc, *st)),
        );
        match rs.mode {
            1 => p.paint(&rs.dabs, PaintMode::Erase),
            2 => p.paint(&rs.dabs, PaintMode::AlphaLock),
            _ => p.replay_ink(&rs.dabs, rs.opacity),
        }
        let tiles = p.read_tiles();
        let wire = diff_guest_tiles(&base, &tiles);
        let changed: Vec<(i32, i32)> = wire.iter().map(|(x, y, _)| (*x, *y)).collect();
        if wire.is_empty() {
            return mk(true, changed, "");
        }
        let apply: Vec<(
            anim_core::raster::TileCoord,
            Option<std::sync::Arc<anim_core::raster::TileData>>,
        )> = wire
            .into_iter()
            .map(|(x, y, t)| {
                let after = if t.is_empty() {
                    None
                } else {
                    Some(std::sync::Arc::new(anim_core::raster::TileData::from_vec(t)))
                };
                ((x, y), after)
            })
            .collect();
        // NEVER-DO 1: this PaintTiles must not ride the command mirror
        // (pixels) — the dab stream carried the stroke to every peer.
        let was = state.engine.mirror_log;
        state.engine.mirror_log = false;
        let r = state.apply_remote_tiles(&rs.author, did, &rs.layer_name, apply);
        state.engine.mirror_log = was;
        match r {
            Ok(()) => mk(true, changed, ""),
            Err(why) => mk(false, Vec::new(), &why),
        }
    }

    /// v0.2.1 (audit): drive the incremental walk — only the NEW tail
    /// of the stroke computes each frame, emitting BIT-identical dabs
    /// to a from-scratch rebuild (pinned by
    /// incremental_dabs_match_full_rebuild). O(n) per stroke where the
    /// old every-frame rebuild was O(n²) — the host's own long fast
    /// strokes were paying that.
    fn extend_stroke_dabs(&mut self) {
        let n = self.current.len();
        if n < self.dab_pts_done || (self.dab_pts_done <= 1 && n >= 2) {
            // A new stroke began, or the single-tap special hands over
            // to the pair walk (which re-folds from the start, exactly
            // like the old rebuild did).
            self.dab_cache.clear();
            self.dab_carry = 0.0;
            self.dab_walked = 0.0;
            self.dab_skipped = 0;
            self.dab_pts_done = if n >= 2 { 1 } else { 0 };
            self.dab_held = [0.0; 4];
        }
        if n == 0 || n == self.dab_pts_done {
            return;
        }
        let mut carry = self.dab_carry;
        let mut walked = self.dab_walked;
        let mut skipped = self.dab_skipped;
        let start = self.dab_pts_done.saturating_sub(1);
        let idx_base = self.dab_cache.len() as u32;
        let (new, held) = self.build_stroke_dabs_from(
            start,
            &mut carry,
            &mut walked,
            &mut skipped,
            self.dab_held,
            idx_base,
        );
        self.dab_cache.extend(new);
        self.dab_carry = carry;
        self.dab_walked = walked;
        self.dab_skipped = skipped;
        self.dab_held = held;
        self.dab_pts_done = n;
    }

    /// Walk stroke points into evenly spaced dabs (paper space), from
    /// the pair starting at `start`. Deterministic prefix: appending
    /// points only extends the tail, so `dab_cache[dabs_flushed..]` are
    /// always genuinely new. The walk state rides in the caller's
    /// fields; `held0` seeds the smudge fold; `idx_base` keeps the
    /// deterministic dab index continuous across frames.
    fn build_stroke_dabs_from(
        &self,
        start: usize,
        carry_io: &mut f32,
        walked_io: &mut f32,
        skipped_io: &mut u32,
        held0: [f32; 4],
        idx_base: u32,
    ) -> (Vec<Dab>, [f32; 4]) {
        let pts = &self.current;
        let mut dabs = Vec::new();
        if pts.is_empty() {
            return (dabs, held0);
        }
        let base = linear_rgba(self.brush_color);
        let flow = self.brush_flow.clamp(0.0, 1.0);
        let hardness = 0.85;
        // PSD-brush-engine: the armed preset's machinery. None = the old
        // path, byte-identical (tip stays 0, spacing stays 0.1).
        let eng = self.brush_engine.as_ref();
        let tip_flag = if self.tip_active && eng.is_some_and(|e| e.tip_key.is_some() || e.auto.is_some()) {
            1.0
        } else {
            0.0
        };
        let spacing_frac = eng.map(|e| e.spacing).unwrap_or(0.1);
        let curve_fac = |target: &str, pr: f32, tv: [f32; 2], idx: u32, dist: f32| -> f32 {
            let Some(e) = eng else { return 1.0 };
            let mut f = 1.0f32;
            let mut any = false;
            for c in e.curves.iter().filter(|c| c.target == target) {
                any = true;
                let x = sensor_input(&c.sensor, pr, tv, idx, dist);
                f *= curve_eval(&c.points, x);
            }
            if any { f } else { 1.0 }
        };
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
            let t = if self.dyn_size {
                self.pen_curve.apply(pr)
            } else {
                1.0
            };
            let broaden = if self.tilt_size {
                1.0 + strength * tn
            } else {
                1.0
            };
            (self.raster_brush_px * (min_s + (1.0 - min_s) * t) * broaden * 0.5)
                .max(0.5)
                .min(cap)
        };
        // THE SMUDGE GATE: the held colour, folded over dab order —
        // deterministic because the source tiles are the pre-stroke truth
        // and the fold recomputes identically on every prefix rebuild.
        let smudging = eng.is_some_and(|e| e.smudge_rate > 0.0) && self.smudge_src.is_some();
        let held = std::cell::Cell::new(held0);
        let dab_at = |x: f32, y: f32, pr: f32, tv: [f32; 2], idx: u32, dist: f32| {
            let tn = tilt_norm(tv);
            let a = if self.dyn_opacity {
                self.pen_curve.apply(pr)
            } else {
                1.0
            };
            let lighten = if self.tilt_opacity {
                1.0 - 0.75 * strength * tn
            } else {
                1.0
            };
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
            // PSD-brush-engine: sensor curves scale size/opacity/flow;
            // rotation + randomness aim the stamp; scatter offsets it.
            // All deterministic in (idx, geometry) — NEVER-DO 2.
            let mut center = [x, y];
            let mut radius = radius;
            let mut color = color;
            let mut dir = dir;
            let mut aspect = aspect;
            if let Some(e) = eng
                && smudging
            {
                let s_rate = (e.smudge_rate
                    * curve_fac("smudge", pr, tv, idx, dist))
                .clamp(0.0, 1.0);
                let c_rate = (e.color_rate
                    * curve_fac("color_rate", pr, tv, idx, dist))
                .clamp(0.0, 1.0);
                let sample = sample_tiles(self.smudge_src.as_ref().unwrap(), x, y);
                let mut h = held.get();
                for c in 0..4 {
                    h[c] += (sample[c] - h[c]) * s_rate;
                }
                held.set(h);
                // Deposit: brush colour re-inks by c_rate over the held
                // pickup; alpha rides the held coverage so blenders fade
                // to nothing over empty paper instead of painting paste.
                let mut mixed = color;
                for c in 0..3 {
                    mixed[c] = h[c] * (1.0 - c_rate) + color[c] * c_rate;
                }
                mixed[3] = color[3] * (h[3] * (1.0 - c_rate) + c_rate).clamp(0.0, 1.0);
                color = mixed;
            }
            if let Some(e) = eng {
                let sf = curve_fac("size", pr, tv, idx, dist);
                radius = (radius * sf.max(0.01)).max(0.35);
                let of = curve_fac("opacity", pr, tv, idx, dist);
                let ff = curve_fac("flow", pr, tv, idx, dist);
                color[3] *= (of * ff).clamp(0.0, 1.0);
                let mut rot_deg = e.angle_deg;
                let rf = curve_fac("rotation", pr, tv, idx, dist);
                if rf != 1.0 {
                    rot_deg += rf * 360.0;
                }
                if e.randomness > 0.0 {
                    rot_deg += (hash01(idx, 7) - 0.5) * 360.0 * e.randomness;
                }
                if tip_flag > 0.5 {
                    let r = rot_deg.to_radians();
                    dir = [r.cos(), r.sin()];
                    aspect = 1.0; // tip ratio is baked into the mask
                }
                if e.scatter > 0.0 {
                    let amp = e.scatter * radius * 2.0;
                    center[0] += (hash01(idx, 11) - 0.5) * amp;
                    center[1] += (hash01(idx, 13) - 0.5) * amp;
                }
            }
            // GIH cycling: the frame rides in the tip value (1 + f);
            // deterministic per dab (NEVER-DO 2).
            let tip = if tip_flag > 0.5 && self.tip_frames > 1 {
                1.0 + (hash01(idx, 29) * self.tip_frames as f32)
                    .floor()
                    .min(self.tip_frames as f32 - 1.0)
            } else {
                tip_flag
            };
            Dab {
                center,
                radius,
                hardness,
                color,
                dir,
                aspect,
                tip,
            }
        };
        if pts.len() == 1 {
            dabs.push(dab_at(pts[0].x, pts[0].y, pts[0].pressure, pts[0].tilt, 0, 0.0));
            return (dabs, held.get());
        }
        let mut carry = *carry_io; // distance into the next segment for the next dab
        let mut walked = *walked_io; // paper distance already covered (distance sensor)
        let mut skipped = *skipped_io; // density-dropped dabs (keeps idx stable)
        for w in pts[start..].windows(2) {
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
                let idx = idx_base + dabs.len() as u32 + skipped;
                let d2 = dab_at(ax + dx * t, ay + dy * t, pr, tv, idx, walked + d);
                let r = d2.radius;
                // Spray density: keep a deterministic fraction of dabs.
                let keep = match eng {
                    Some(e) if e.density > 0.0 && e.density < 1.0 => {
                        hash01(idx, 17) <= e.density
                    }
                    _ => true,
                };
                if keep {
                    dabs.push(d2);
                } else {
                    skipped += 1;
                }
                // Spacing from the preset (fraction of diameter).
                let step = (spacing_frac * (2.0 * r)).max(0.75);
                d += step;
            }
            carry = d - len;
            walked += len;
        }
        *carry_io = carry;
        *walked_io = walked;
        *skipped_io = skipped;
        (dabs, held.get())
    }

    /// LASSO FILL input (PSD-lasso-fill): drag draws the loop; release
    /// commits ONE region edit. The fill tool's guard set, verbatim
    /// (NEVER-DO 2), minus the GPU requirement — this path is pure CPU.
    fn lasso_fill_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        to_paper: &impl Fn(Pos2) -> Pos2,
        state: &mut AppState,
    ) {
        if !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Escape))
            && !self.lasso_pts.is_empty()
        {
            self.lasso_pts.clear();
            state.status = "lasso cleared".into();
            return;
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            let Some(pos) = response.interact_pointer_pos() else {
                return;
            };
            if !rect.contains(pos) {
                return;
            }
            // The fill tool's guards, in the fill tool's order.
            if self.composite_view {
                state.refuse("refused — composite view is read-only (C to edit)");
                return;
            }
            if state.active_layer_props().is_some_and(|p| !p.visible) {
                state.status = format!(
                    "layer '{}' is hidden — press A to switch or click its eye",
                    state.active_layer_name()
                );
                return;
            }
            if state.own_key_drawing().is_none() {
                state.status =
                    "held/blank frame — a fill edits a frame's OWN cel (draw here first)"
                        .into();
                return;
            }
            self.lasso_pts.clear();
            self.lasso_pts.push(to_paper(pos));
            return;
        }
        if response.dragged_by(egui::PointerButton::Primary) && !self.lasso_pts.is_empty() {
            if let Some(pos) = response.interact_pointer_pos() {
                let p = to_paper(pos);
                // Decimate: a point every ~1.5 paper px keeps loops light.
                if self
                    .lasso_pts
                    .last()
                    .is_none_or(|l| (l.x - p.x).hypot(l.y - p.y) > 1.5)
                {
                    self.lasso_pts.push(p);
                }
            }
            return;
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) && self.lasso_pts.len() >= 3
        {
            let pts = std::mem::take(&mut self.lasso_pts);
            self.commit_lasso_fill(&pts, state);
        }
    }

    /// Rasterize the loop (scanline), feather inward (chamfer signed
    /// distance — bounded O(area), NEVER-DO 4), composite the brush
    /// colour over the active layer, and commit ONE region edit.
    fn commit_lasso_fill(&mut self, pts: &[Pos2], state: &mut AppState) {
        use anim_core::raster::{TILE, TileData, f16_bits_to_f32, f32_to_f16_bits};
        let Some(did) = state.own_key_drawing() else { return };
        let (kd, kl, _) = state.active_layer_key();
        if kd != did.0 || kl == u64::MAX {
            state.status = "no raster layer here to fill".into();
            return;
        }
        let (pw, ph) = (
            state.engine.project.width as i32,
            state.engine.project.height as i32,
        );
        // The working grid: the loop's bbox, clipped to paper, padded by
        // the feather band.
        let pad = (self.lasso_soft + self.lasso_grow.abs()).ceil() as i32 + 2;
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for p in pts {
            x0 = x0.min(p.x.floor() as i32);
            y0 = y0.min(p.y.floor() as i32);
            x1 = x1.max(p.x.ceil() as i32);
            y1 = y1.max(p.y.ceil() as i32);
        }
        x0 = (x0 - pad).max(0);
        y0 = (y0 - pad).max(0);
        x1 = (x1 + pad).min(pw - 1);
        y1 = (y1 + pad).min(ph - 1);
        if x0 > x1 || y0 > y1 {
            state.status = "the loop was outside the paper".into();
            return;
        }
        let (gw, gh) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
        // 1) Scanline even-odd rasterization into a hard mask.
        let mut inside = vec![false; gw * gh];
        for gy in 0..gh {
            let yc = (y0 + gy as i32) as f32 + 0.5;
            let mut xs: Vec<f32> = Vec::new();
            for i in 0..pts.len() {
                let a = pts[i];
                let b = pts[(i + 1) % pts.len()];
                if (a.y <= yc) != (b.y <= yc) {
                    xs.push(a.x + (yc - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
            xs.sort_by(f32::total_cmp);
            for span in xs.chunks_exact(2) {
                let sx = (span[0].ceil() as i32).max(x0);
                let ex = (span[1].floor() as i32).min(x1);
                for x in sx..=ex {
                    inside[gy * gw + (x - x0) as usize] = true;
                }
            }
        }
        // 2) Chamfer signed distance (3-4 weights, two passes each way):
        //    dist > 0 inside, < 0 outside, in ~pixels (/3).
        let big = 1_000_000i32;
        let mut din = vec![big; gw * gh];
        let mut dout = vec![big; gw * gh];
        for i in 0..gw * gh {
            if inside[i] {
                dout[i] = 0;
            } else {
                din[i] = 0;
            }
        }
        let chamfer = |d: &mut [i32]| {
            for y in 0..gh {
                for x in 0..gw {
                    let i = y * gw + x;
                    let mut v = d[i];
                    if x > 0 {
                        v = v.min(d[i - 1] + 3);
                    }
                    if y > 0 {
                        v = v.min(d[i - gw] + 3);
                        if x > 0 {
                            v = v.min(d[i - gw - 1] + 4);
                        }
                        if x + 1 < gw {
                            v = v.min(d[i - gw + 1] + 4);
                        }
                    }
                    d[i] = v;
                }
            }
            for y in (0..gh).rev() {
                for x in (0..gw).rev() {
                    let i = y * gw + x;
                    let mut v = d[i];
                    if x + 1 < gw {
                        v = v.min(d[i + 1] + 3);
                    }
                    if y + 1 < gh {
                        v = v.min(d[i + gw] + 3);
                        if x + 1 < gw {
                            v = v.min(d[i + gw + 1] + 4);
                        }
                        if x > 0 {
                            v = v.min(d[i + gw - 1] + 4);
                        }
                    }
                    d[i] = v;
                }
            }
        };
        chamfer(&mut din);
        chamfer(&mut dout);
        // 3) Alpha: signed distance shifted by grow, ramped over softness.
        //    softness 0 = crisp edge exactly at the loop (+grow).
        let soft = self.lasso_soft.max(0.0);
        let c = linear_rgba(self.brush_color);
        let premult = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
        let target = state.active_layer_tiles().cloned().unwrap_or_default();
        let mut diff: anim_core::raster::TileDiff = Vec::new();
        let tx0 = x0.div_euclid(TILE as i32);
        let tx1 = x1.div_euclid(TILE as i32);
        let ty0 = y0.div_euclid(TILE as i32);
        let ty1 = y1.div_euclid(TILE as i32);
        for ty in ty0..=ty1 {
            for tx in tx0..=tx1 {
                let before = target.get(&(tx, ty)).cloned();
                let mut texels: Vec<u16> = before
                    .as_ref()
                    .map(|t| t.rgba.to_vec())
                    .unwrap_or_else(|| vec![0u16; TILE * TILE * 4]);
                let mut touched = false;
                for cy in 0..TILE {
                    let py = ty * TILE as i32 + cy as i32;
                    if py < y0 || py > y1 {
                        continue;
                    }
                    for cx in 0..TILE {
                        let px = tx * TILE as i32 + cx as i32;
                        if px < x0 || px > x1 {
                            continue;
                        }
                        let gi =
                            (py - y0) as usize * gw + (px - x0) as usize;
                        let signed = if inside[gi] {
                            din[gi] as f32 / 3.0
                        } else {
                            -(dout[gi] as f32 / 3.0)
                        };
                        let edge = signed + self.lasso_grow;
                        let a = if soft < 0.5 {
                            if edge > 0.0 { 1.0 } else { 0.0 }
                        } else {
                            (edge / soft).clamp(0.0, 1.0)
                        };
                        if a <= 0.0 {
                            continue;
                        }
                        touched = true;
                        let i = (cy * TILE + cx) * 4;
                        // src-over, premultiplied.
                        let src = [
                            premult[0] * a,
                            premult[1] * a,
                            premult[2] * a,
                            premult[3] * a,
                        ];
                        for ch in 0..4 {
                            let dst = f16_bits_to_f32(texels[i + ch]);
                            texels[i + ch] =
                                f32_to_f16_bits(src[ch] + dst * (1.0 - src[3]));
                        }
                    }
                }
                if touched {
                    diff.push((
                        (tx, ty),
                        before,
                        Some(std::sync::Arc::new(TileData::from_vec(texels))),
                    ));
                }
            }
        }
        if diff.is_empty() {
            state.status = "the loop enclosed nothing".into();
            return;
        }
        state.commit_region_edit("lasso fill", did, LayerId(kl), diff);
        self.synced_active = (u64::MAX, u64::MAX, u64::MAX);
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
            state.refuse(
                "refused — the fill tool needs the GPU brush engine \
                 (Settings › Performance)",
            );
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
                    FR::OutsideSelection => "clicked outside the selection (Esc clears it)".into(),
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
            // AUDIT [7]: the pen is his primary instrument — its refusal
            // must reach the Aka lane and flash the canvas edge, exactly
            // as the mouse path and the hidden-layer guard already do.
            state.refuse("refused — composite view is read-only (C to edit)");
            return false;
        }
        if self.is_guest && !self.guest_ready {
            state.refuse("refused — the host's file is still arriving");
            return false;
        }
        // GUARD (CSP behavior): never paint into a layer you can't see.
        if self.raster && state.active_layer_props().is_some_and(|p| !p.visible) {
            state.refuse(format!(
                "refused — layer '{}' is hidden (A to switch, or click its eye)",
                state.active_layer_name()
            ));
            return false;
        }
        // UNARMED (defect 16): the tracked layer name is absent on this
        // cel — no layer receives ink; refuse instead of misrouting.
        let Some(resolved_slot) = state.active_slot_resolved() else {
            state.refuse(format!(
                "refused — no '{}' layer on this cel (pick one in Cel Layers)",
                state.view.active_layer
            ));
            return false;
        };
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
        self.stroke_alpha_locked = self.alpha_lock;
        self.stroke_layer_slot = resolved_slot;
        self.cel_touched = false;
        self.dabs_flushed = 0;
        self.raster_stroke_done = false;
        self.raster_new_cel = self.raster && state.own_key_drawing().is_none();
        self.smudge_src = self.capture_smudge_src(state);
        self.begin_stream(state);
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

        // native_pen is collected ONCE per frame from the tablet thread,
        // which is bound to the MAIN window's HWND (see main.rs's
        // spawn_tablet_thread) — octotablet can never report samples for
        // any OTHER viewport, and the same slice is (necessarily) handed to
        // every window's canvas.ui() call this frame. Treating it as
        // meaningful outside the root viewport would either silently do
        // nothing (if the pen is elsewhere, native_pen is empty here too —
        // harmless) or double-process the SAME physical samples against a
        // second window's geometry (if the pen IS on the main window this
        // frame — not harmless). So: native input only exists for ROOT.
        let native_available = ui.ctx().viewport_id() == egui::ViewportId::ROOT;

        // HOVER TILT (any backend state): proximity samples carry live tilt so
        // the cursor needle + T° readout move before the stroke starts. Never
        // touches stroke state (the thread only emits Hover while the pen is
        // up, and the guard makes that a hard rule).
        if native_available {
            for s in native_pen {
                if s.phase == PenPhase::Hover
                    && !self.touch_active
                    && let Some(t) = s.tilt
                {
                    self.note_tilt(t);
                }
            }
        }

        // NATIVE TABLET PATH (octotablet / Windows Ink RealTimeStylus): once
        // real tablet samples arrive, they own the pen forever this session —
        // Windows ALSO surfaces the same physical strokes as egui Touch and
        // mouse events, so both fallbacks must go quiet or every stroke would
        // paint twice. Hover samples deliberately do NOT latch: they never
        // paint, so they must not silence the fallbacks by themselves.
        //
        // `native_active` is a SESSION-WIDE latch (once true, stays true) —
        // correct on the single window this app used to have, but wrong on
        // a second (non-root) window that native input can never reach: the
        // latch being true from earlier main-window strokes must NOT
        // silence ITS Touch/mouse fallback too, or nothing would ever paint
        // there. Gate the whole native branch on native_available so a
        // floating canvas always falls through to its own Touch/mouse
        // input, regardless of what the main window's pen has done before.
        if native_available {
            if native_pen.iter().any(|s| s.phase != PenPhase::Hover) {
                self.native_active = true;
                self.seen_pen = true;
            }
            if self.native_active {
                for s in native_pen {
                    match s.phase {
                        PenPhase::Down => {
                            // CLICKTHROUGH FIX (owner 2026-08-19): raw pen
                            // samples bypass egui's layering, so a pen-down
                            // on a fold-out / window / panel floating over
                            // the paper used to stroke THROUGH it. Same
                            // layer test the brush cursor already uses.
                            if ui.ctx().layer_id_at(s.pos) == Some(ui.layer_id()) {
                                self.stroke_start(
                                    s.pos, s.pressure, s.tilt, rect, to_paper, state,
                                );
                            }
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
                        // (Same clickthrough gate as the native path: raw
                        // Touch events also bypass egui's layering.)
                        if ui.ctx().layer_id_at(*pos) == Some(ui.layer_id()) {
                            self.stroke_start(
                                *pos, *force, Some([0.0, 0.0]), rect, to_paper, state,
                            );
                        }
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
                    state.refuse("refused — composite view is read-only (C to edit)");
                    return;
                }
                if self.raster && state.active_layer_props().is_some_and(|p| !p.visible) {
                    state.refuse(format!(
                        "refused — layer '{}' is hidden (A to switch, or click its eye)",
                        state.active_layer_name()
                    ));
                    return;
                }
                // UNARMED (defect 16), mouse path: same law as the pen.
                let Some(resolved_slot) = state.active_slot_resolved() else {
                    state.refuse(format!(
                        "refused — no '{}' layer on this cel (pick one in Cel Layers)",
                        state.view.active_layer
                    ));
                    return;
                };
                self.current.clear();
                self.cur_some = 0; // no tablet force will arrive; keep the
                self.cur_none = 0; // force% diagnostic honest for this stroke
                self.stroke_from_mouse = true;
                self.stroke_erasing = self.erasing;
                self.stroke_alpha_locked = self.alpha_lock;
                self.stroke_layer_slot = resolved_slot;
                self.cel_touched = false;
                self.dabs_flushed = 0;
                self.raster_stroke_done = false;
                self.raster_new_cel = self.raster && state.own_key_drawing().is_none();
                self.smudge_src = self.capture_smudge_src(state);
                self.begin_stream(state);
                if let Some(p) = response.interact_pointer_pos() {
                    let p = to_paper(p);
                    self.current.push(StrokePoint {
                        x: p.x,
                        y: p.y,
                        pressure: MOUSE_PRESSURE,
                        tilt: [0.0; 2], // mouse: vertical pen
                    });
                }
            } else if response.dragged_by(egui::PointerButton::Primary) && !self.current.is_empty()
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
            self.dbg_max = self.current.iter().map(|p| p.pressure).fold(0.0, f32::max);
            self.dbg_some = self.cur_some;
            self.dbg_none = self.cur_none;
        }
        // Mouse-mode = this stroke carried no real tablet force: either the mouse
        // fallback drew it, or a Touch stream that DID move (cur_none Move samples)
        // never once reported force. The `cur_none > 0` guard avoids falsely
        // flagging a legitimate quick pen tap (Start+End, no Move sample at all).
        // Drives the red MOUSE badge that tells the user to fix the driver.
        self.dbg_mouse_mode = self.stroke_from_mouse || (self.cur_none > 0 && self.cur_some == 0);
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
            let t = if n > 1 {
                k as f32 / (n - 1) as f32
            } else {
                0.0
            };
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

/// Cel-layer identity ink for a role name (orientation aid), under the
/// IDENTITY AMENDMENT (spec §3.5, owner-ratified 2026-08-18): the
/// pipeline's own pencil code in the plate's own tokens — rough is the
/// ao-enpitsu, correction is the sakkan's aka. Identity is always a
/// small fill beside the role's NAME, never text or an edge, so it can
/// never be read as a refusal or a ghost.
pub fn layer_chip_color(name: &str) -> Color32 {
    match name {
        // Layer identity in the plate's own inks (no hue outside the eight
        // tokens; no green anywhere — spec §3). rough is the ao-enpitsu,
        // correction is the sakkan's aka: the pipeline's own colour code.
        "line" => plate::STRUCK,
        "color" => plate::LEGEND,
        "shadow" => Color32::from_rgba_unmultiplied(83, 137, 196, 150), // ao, dimmed
        "highlight" => plate::PAPER,
        "correction" => plate::AKA,
        "rough" => plate::AO,
        _ => plate::LEGEND,
    }
}

/// THE ERASE-RACE FIX: what a guest's pen actually changed — readback
/// tiles whose hash differs from the STROKE-START base, plus an empty
/// marker for base tiles the pen erased entirely. Tiles the pen never
/// touched are absent even when the live mirror has moved on (a host
/// stroke arriving mid-stroke must never echo back as an erase).
fn diff_guest_tiles(
    base: &std::collections::BTreeMap<anim_core::raster::TileCoord, std::sync::Arc<anim_core::raster::TileData>>,
    readback: &[(anim_core::raster::TileCoord, std::sync::Arc<anim_core::raster::TileData>)],
) -> Vec<(i32, i32, Vec<u16>)> {
    let mut wire: Vec<(i32, i32, Vec<u16>)> = Vec::new();
    let mut seen = std::collections::HashSet::with_capacity(readback.len());
    for ((x, y), t) in readback {
        seen.insert((*x, *y));
        match base.get(&(*x, *y)) {
            Some(b) if b.hash == t.hash => {}
            _ => wire.push((*x, *y, t.rgba.to_vec())),
        }
    }
    for (c, _) in base.iter() {
        if !seen.contains(c) {
            wire.push((c.0, c.1, Vec::new()));
        }
    }
    wire
}

/// THE SMUDGE GATE: sample a committed tile map at paper coords —
/// straight (un-premultiplied) linear RGBA, transparent where no tile.
fn sample_tiles(
    tiles: &std::collections::BTreeMap<anim_core::raster::TileCoord, std::sync::Arc<anim_core::raster::TileData>>,
    x: f32,
    y: f32,
) -> [f32; 4] {
    use anim_core::raster::{TILE, f16_bits_to_f32};
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let coord = (xi.div_euclid(TILE as i32), yi.div_euclid(TILE as i32));
    let Some(tile) = tiles.get(&coord) else {
        return [0.0; 4];
    };
    let cx = xi.rem_euclid(TILE as i32) as usize;
    let cy = yi.rem_euclid(TILE as i32) as usize;
    let i = (cy * TILE + cx) * 4;
    let p = [
        f16_bits_to_f32(tile.rgba[i]),
        f16_bits_to_f32(tile.rgba[i + 1]),
        f16_bits_to_f32(tile.rgba[i + 2]),
        f16_bits_to_f32(tile.rgba[i + 3]),
    ];
    // Tiles are premultiplied; the dab wants straight colour.
    if p[3] > 1e-4 {
        [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]]
    } else {
        [0.0; 4]
    }
}

/// PSD-brush-engine: deterministic per-dab hash (Wang) — NEVER a clock,
/// NEVER thread RNG (NEVER-DO 2). Same stroke = same pixels, always.
pub(crate) fn hash01(idx: u32, salt: u32) -> f32 {
    let mut x = idx.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x as f32) / (u32::MAX as f32)
}

/// Piecewise-linear through the preset's own curve points; empty = the
/// sensor's raw value (identity — no invented defaults, room NEVER-DO 5).
pub(crate) fn curve_eval(points: &[[f32; 2]], x: f32) -> f32 {
    if points.len() < 2 {
        return x.clamp(0.0, 1.0);
    }
    let x = x.clamp(0.0, 1.0);
    if x <= points[0][0] {
        return points[0][1];
    }
    for w in points.windows(2) {
        if x <= w[1][0] {
            let span = (w[1][0] - w[0][0]).max(1e-6);
            let t = (x - w[0][0]) / span;
            return w[0][1] + (w[1][1] - w[0][1]) * t;
        }
    }
    points[points.len() - 1][1]
}

/// A sensor's 0..1 input for one dab. pressure = the pen; fuzzy = the
/// per-dab hash; fade/distance = how far the stroke has run (in dabs /
/// paper px, Krita's own defaults for unparameterized lengths); tilt
/// sensors from the native pen's tilt vector.
fn sensor_input(sensor: &str, pr: f32, tv: [f32; 2], idx: u32, dist: f32) -> f32 {
    match sensor {
        "pressure" => pr.clamp(0.0, 1.0),
        "fuzzy" => hash01(idx, 23),
        "fade" => (idx as f32 / 300.0).clamp(0.0, 1.0),
        "distance" => (dist / 500.0).clamp(0.0, 1.0),
        "xtilt" => (tv[0] / TILT_MAX_RAD * 0.5 + 0.5).clamp(0.0, 1.0),
        "ytilt" => (tv[1] / TILT_MAX_RAD * 0.5 + 0.5).clamp(0.0, 1.0),
        "ascension" => (tv[1].atan2(tv[0]) / std::f32::consts::TAU + 0.5).clamp(0.0, 1.0),
        "declination" => ((tv[0] * tv[0] + tv[1] * tv[1]).sqrt() / TILT_MAX_RAD).clamp(0.0, 1.0),
        _ => 1.0,
    }
}

/// Decode PNG bytes to RGBA8 (the forge's stamp-from-file door).
pub(crate) fn load_rgba_png_bytes(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
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
        _ => return None,
    };
    Some((w, h, rgba))
}

/// Load a cache PNG (written by kpp.rs — always RGBA8) back as raw pixels.
pub(crate) fn load_cache_png(path: &std::path::Path) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = std::fs::read(path).ok()?;
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba {
        return None;
    }
    Some((info.width, info.height, buf[..info.buffer_size()].to_vec()))
}

/// Rasterize a Krita MaskGenerator recipe to a tip mask (white + alpha).
/// Deterministic pure function of the recipe. The model: unit disc (or
/// box) squashed by ratio, spiked by the regular-polygon envelope when
/// spikes > 2, alpha 1 inside the fade start and falling to the rim —
/// fade 1.0 = crisp edge, 0.0 = cone from the centre; "soft" uses a
/// squared falloff. An approximation of Krita's generator, said plainly
/// in the room log.
pub(crate) fn rasterize_auto_tip(a: &crate::config::AutoTip, n: u32) -> (u32, u32, Vec<u8>) {
    let ratio = a.ratio.clamp(0.05, 1.0);
    let is_rect = a.shape == "rect";
    let fade = |d: f32, f: f32| -> f32 {
        let f = f.clamp(0.0, 0.995);
        if a.soft {
            let t = (1.0 - d).clamp(0.0, 1.0);
            (t * t).min(1.0)
        } else {
            ((1.0 - d) / (1.0 - f)).clamp(0.0, 1.0)
        }
    };
    let mut rgba = vec![0u8; (n * n * 4) as usize];
    for y in 0..n {
        for x in 0..n {
            let u = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let (ex, ey) = (u, v / ratio);
            let mut d = if is_rect {
                ex.abs().max(ey.abs())
            } else {
                (ex * ex + ey * ey).sqrt()
            };
            if a.spikes > 2 && !is_rect {
                let m = a.spikes as f32;
                let a0 = std::f32::consts::PI / m;
                let th = ey.atan2(ex);
                let env = (a0.cos() / ((th.rem_euclid(2.0 * a0)) - a0).cos()).abs();
                d /= env.max(1e-3);
            }
            // Direction-blended fade start (h across, v along).
            let th = ey.atan2(ex);
            let f = a.hfade * th.cos().abs() + a.vfade * th.sin().abs();
            let alpha = if d >= 1.0 { 0.0 } else { fade(d, f.clamp(0.0, 1.0)) };
            let i = ((y * n + x) * 4) as usize;
            rgba[i] = 255;
            rgba[i + 1] = 255;
            rgba[i + 2] = 255;
            rgba[i + 3] = (alpha * 255.0 + 0.5) as u8;
        }
    }
    (n, n, rgba)
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
    strength: f32,
) -> Vec<(anim_core::ids::DrawingId, Color32)> {
    let current = state.resolve_at(column, frame);
    let mut out = Vec::new();
    let mut collect = |range: Box<dyn Iterator<Item = u32>>, base: (u8, u8, u8)| {
        let mut found: Vec<anim_core::ids::DrawingId> = Vec::new();
        for f in range {
            if let Some(id) = state.resolve_at(column, f)
                && Some(id) != current
                && !found.contains(&id)
            {
                found.push(id);
                if found.len() == 2 {
                    break;
                }
            }
        }
        for (depth, id) in found.into_iter().enumerate() {
            let alpha = ((if depth == 0 { 255.0 } else { 140.0 }) * strength.clamp(0.0, 1.0)) as u8;
            out.push((
                id,
                Color32::from_rgba_unmultiplied(base.0, base.1, base.2, alpha),
            ));
        }
    };

    // Ghost BEHIND = Ao (continuity — where the motion came from); ghost
    // AHEAD = Legend (the plate stating what is next). Spec defect 14: the
    // old red/green pair is dead — Aka never sits on the paper for hours,
    // and there is no green in this application.
    collect(Box::new((0..frame).rev()), (83, 137, 196));
    collect(Box::new(frame + 1..state.frame_count()), (139, 141, 131));
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
        let color = tint.unwrap_or_else(|| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]));
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
            painter.add(egui::Shape::convex_polygon(quad, color, egui::Stroke::NONE));
        }
        // Round dab at the far vertex smooths the joint to the next segment.
        painter.circle_filled(b, hb, color);
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;

    #[test]
    fn hash01_is_deterministic_and_bounded() {
        for i in 0..2000u32 {
            let a = hash01(i, 7);
            assert_eq!(a, hash01(i, 7), "same inputs, same value — always");
            assert!((0.0..=1.0).contains(&a));
        }
        assert_ne!(hash01(1, 7), hash01(2, 7));
    }

    #[test]
    fn curve_eval_matches_its_points() {
        let pts = [[0.0, 0.0], [0.5, 0.8], [1.0, 1.0]];
        assert_eq!(curve_eval(&pts, 0.0), 0.0);
        assert!((curve_eval(&pts, 0.25) - 0.4).abs() < 1e-5);
        assert!((curve_eval(&pts, 0.5) - 0.8).abs() < 1e-5);
        assert_eq!(curve_eval(&pts, 1.0), 1.0);
        // Empty curve = the sensor's raw value.
        assert_eq!(curve_eval(&[], 0.37), 0.37);
    }

    #[test]
    fn auto_tip_rasterizes_deterministically() {
        let a = crate::config::AutoTip {
            shape: "circle".into(),
            ratio: 0.6,
            hfade: 0.9,
            vfade: 0.4,
            spikes: 5,
            soft: false,
        };
        let (w, h, m1) = rasterize_auto_tip(&a, 64);
        let (_, _, m2) = rasterize_auto_tip(&a, 64);
        assert_eq!((w, h), (64, 64));
        assert_eq!(m1, m2, "pure function of the recipe");
        let centre = m1[(32 * 64 + 32) * 4 + 3];
        let corner = m1[3];
        assert!(centre > 200, "solid centre (got {centre})");
        assert_eq!(corner, 0, "empty corner");
    }
}

#[cfg(test)]
mod dab_walk_tests {
    use super::*;

    /// v0.2.1: the incremental walk IS the old full rebuild — feeding
    /// points one at a time must emit bit-identical dabs to feeding
    /// them all at once (the determinism law survives the O(n) cache).
    #[test]
    fn incremental_dabs_match_full_rebuild() {
        let mk_points = || -> Vec<StrokePoint> {
            (0..120)
                .map(|i| {
                    let t = i as f32 * 0.37;
                    StrokePoint {
                        x: 100.0 + t * 13.0 + (t * 0.7).sin() * 9.0,
                        y: 80.0 + (t * 0.9).cos() * 21.0,
                        pressure: (0.2 + 0.8 * ((t * 0.31).sin() * 0.5 + 0.5)).min(1.0),
                        tilt: [(t * 0.11).sin() * 0.4, (t * 0.13).cos() * 0.3],
                    }
                })
                .collect()
        };
        // Incremental: points arrive a few per "frame".
        let mut inc = CanvasView::new();
        let pts = mk_points();
        let mut fed = 0usize;
        while fed < pts.len() {
            let step = 1 + (fed % 7); // uneven arrival, like a real pen
            fed = (fed + step).min(pts.len());
            inc.current = pts[..fed].to_vec();
            inc.extend_stroke_dabs();
        }
        // Full: everything at once, one walk.
        let mut full = CanvasView::new();
        full.current = pts;
        full.extend_stroke_dabs();
        assert_eq!(
            inc.dab_cache.len(),
            full.dab_cache.len(),
            "same dab count either way"
        );
        for (i, (a, b)) in inc.dab_cache.iter().zip(full.dab_cache.iter()).enumerate() {
            assert_eq!(a.center, b.center, "dab {i} center");
            assert_eq!(a.radius, b.radius, "dab {i} radius");
            assert_eq!(a.color, b.color, "dab {i} color");
            assert_eq!(a.dir, b.dir, "dab {i} dir");
            assert_eq!(a.aspect, b.aspect, "dab {i} aspect");
            assert_eq!(a.tip, b.tip, "dab {i} tip");
        }
        // And the single-tap special hands over cleanly.
        let mut tap = CanvasView::new();
        tap.current = mk_points()[..1].to_vec();
        tap.extend_stroke_dabs();
        assert_eq!(tap.dab_cache.len(), 1, "a tap is one dab");
        tap.current = mk_points()[..3].to_vec();
        tap.extend_stroke_dabs();
        let mut tap_full = CanvasView::new();
        tap_full.current = mk_points()[..3].to_vec();
        tap_full.extend_stroke_dabs();
        assert_eq!(
            tap.dab_cache.len(),
            tap_full.dab_cache.len(),
            "handover discards the tap dab exactly like the old rebuild"
        );
    }
}

#[cfg(test)]
mod smudge_tests {
    use super::*;
    use anim_core::raster::{TILE, TILE_LEN, TileData, f32_to_f16_bits};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn one_tile_map(rgba: [f32; 4]) -> BTreeMap<(i32, i32), Arc<TileData>> {
        // Premultiplied, as committed tiles are.
        let pm = [rgba[0] * rgba[3], rgba[1] * rgba[3], rgba[2] * rgba[3], rgba[3]];
        let mut t = vec![0u16; TILE_LEN];
        for i in 0..(TILE * TILE) {
            for c in 0..4 {
                t[i * 4 + c] = f32_to_f16_bits(pm[c]);
            }
        }
        let mut m = BTreeMap::new();
        m.insert((0, 0), Arc::new(TileData::from_vec(t)));
        m
    }

    #[test]
    fn sampling_unpremultiplies_and_misses_are_transparent() {
        let m = one_tile_map([0.8, 0.4, 0.2, 0.5]);
        let s = sample_tiles(&m, 3.0, 3.0);
        assert!((s[0] - 0.8).abs() < 0.01, "straight red, got {}", s[0]);
        assert!((s[3] - 0.5).abs() < 0.01);
        // Outside the only tile: transparent, never an error.
        assert_eq!(sample_tiles(&m, -200.0, -200.0), [0.0; 4]);
    }

    #[test]
    fn the_held_chain_is_deterministic_and_dulls_toward_the_canvas() {
        // Fold the held colour twice over the same sequence — identical;
        // and it converges toward the sampled colour.
        let m = one_tile_map([0.0, 0.0, 1.0, 1.0]); // blue canvas
        let fold = || {
            let mut h = [0.0f32; 4];
            for i in 0..24 {
                let s = sample_tiles(&m, 4.0 + i as f32, 4.0);
                for c in 0..4 {
                    h[c] += (s[c] - h[c]) * 0.5;
                }
            }
            h
        };
        let (a, b) = (fold(), fold());
        assert_eq!(a, b, "same stroke, same pixels");
        assert!(a[2] > 0.99 && a[3] > 0.99, "held colour became the canvas");
    }
}

#[cfg(test)]
mod lasso_tests {
    /// The chamfer + scanline math on a synthetic square loop: crisp
    /// edge at softness 0, ramp inside at softness N, never outward.
    #[test]
    fn feather_stays_inside_the_loop() {
        // 20×20 grid, square loop 4..16. Mirror the commit's math.
        let (gw, gh) = (20usize, 20usize);
        let mut inside = vec![false; gw * gh];
        for y in 4..16 {
            for x in 4..16 {
                inside[y * gw + x] = true;
            }
        }
        let big = 1_000_000i32;
        let mut din = vec![big; gw * gh];
        let mut dout = vec![big; gw * gh];
        for i in 0..gw * gh {
            if inside[i] {
                dout[i] = 0;
            } else {
                din[i] = 0;
            }
        }
        let chamfer = |d: &mut Vec<i32>| {
            for y in 0..gh {
                for x in 0..gw {
                    let i = y * gw + x;
                    let mut v = d[i];
                    if x > 0 {
                        v = v.min(d[i - 1] + 3);
                    }
                    if y > 0 {
                        v = v.min(d[i - gw] + 3);
                    }
                    d[i] = v;
                }
            }
            for y in (0..gh).rev() {
                for x in (0..gw).rev() {
                    let i = y * gw + x;
                    let mut v = d[i];
                    if x + 1 < gw {
                        v = v.min(d[i + 1] + 3);
                    }
                    if y + 1 < gh {
                        v = v.min(d[i + gw] + 3);
                    }
                    d[i] = v;
                }
            }
        };
        chamfer(&mut din);
        chamfer(&mut dout);
        let alpha = |x: usize, y: usize, soft: f32| -> f32 {
            let i = y * gw + x;
            let signed = if inside[i] {
                din[i] as f32 / 3.0
            } else {
                -(dout[i] as f32 / 3.0)
            };
            if soft < 0.5 {
                if signed > 0.0 { 1.0 } else { 0.0 }
            } else {
                (signed / soft).clamp(0.0, 1.0)
            }
        };
        // Crisp: solid centre, nothing outside.
        assert_eq!(alpha(10, 10, 0.0), 1.0);
        assert_eq!(alpha(2, 2, 0.0), 0.0);
        // Feathered: the rim ramps INSIDE, the outside stays clean, the
        // deep centre stays solid (NEVER-DO 3).
        assert_eq!(alpha(2, 2, 4.0), 0.0, "feather never bleeds outward");
        let rim = alpha(4, 10, 4.0);
        assert!(rim > 0.0 && rim < 0.9, "rim ramps ({rim})");
        assert!(alpha(10, 10, 4.0) > 0.99, "centre solid");
    }
}

#[cfg(test)]
mod erase_race_tests {
    use super::*;
    use anim_core::raster::{TILE_LEN, TileData, f32_to_f16_bits};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn tile(v: f32) -> Arc<TileData> {
        let bits = f32_to_f16_bits(v);
        Arc::new(TileData::from_vec(vec![bits; TILE_LEN]))
    }

    /// The race that erased the host's ink: a stroke lands in the mirror
    /// WHILE the guest's pen is down. The diff against the stroke-start
    /// base must not mention that tile at all.
    #[test]
    fn a_mid_stroke_host_tile_is_never_echoed_as_an_erase() {
        // Base at pen-down: one tile of shared history at (0,0).
        let mut base = BTreeMap::new();
        base.insert((0, 0), tile(0.3));
        // Pen-up readback: the untouched (0,0) plus the guest's new (5,5).
        let readback = vec![((0, 0), tile(0.3)), ((5, 5), tile(0.9))];
        // (The LIVE mirror meanwhile also gained the host's (2,2) — which
        // the readback lacks. Diffing against the live mirror would emit
        // (2,2, empty) — the erase. Against the base it cannot.)
        let wire = diff_guest_tiles(&base, &readback);
        assert_eq!(wire.len(), 1, "only the guest's own tile travels: {wire:?}");
        assert_eq!((wire[0].0, wire[0].1), (5, 5));
        assert!(!wire[0].2.is_empty());
    }

    #[test]
    fn a_genuinely_erased_base_tile_sends_the_empty_marker() {
        let mut base = BTreeMap::new();
        base.insert((1, 1), tile(0.5));
        base.insert((2, 2), tile(0.6));
        // The pen erased (2,2) entirely; (1,1) survives unchanged.
        let readback = vec![((1, 1), tile(0.5))];
        let wire = diff_guest_tiles(&base, &readback);
        assert_eq!(wire.len(), 1);
        assert_eq!((wire[0].0, wire[0].1), (2, 2));
        assert!(wire[0].2.is_empty(), "empty payload = erased tile");
    }

    #[test]
    fn a_repainted_tile_travels_with_its_new_content() {
        let mut base = BTreeMap::new();
        base.insert((3, 3), tile(0.2));
        let readback = vec![((3, 3), tile(0.8))];
        let wire = diff_guest_tiles(&base, &readback);
        assert_eq!(wire.len(), 1);
        assert_eq!(wire[0].2.len(), TILE_LEN);
    }
}
