//! The drawing canvas: a fixed-size logical "paper" (resolution independent)
//! rendered into the panel with pan/zoom. Pen strokes land in paper
//! coordinates so artwork never depends on window size or zoom level.
//!
//! Input reuses the M0-validated path: Windows Ink pen arrives as egui Touch
//! events with real pressure; mouse is the no-pressure fallback.

use std::collections::BTreeMap;
use std::sync::Arc;

use anim_core::camera::IRect;
use anim_core::color::ColorLaw;
use anim_core::edit::{self, FloatingPatch};
use anim_core::ids::{ColumnId, CutId, DrawingId, LayerId, SceneId};
use anim_core::model::{DrawingKind, Stroke, StrokePoint};
use anim_core::raster::{TileCoord, TileData};
use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, pos2, vec2};

use crate::config::{Action, BrushPreset, LayersConfig, PenConfig, PressureCurve};
use crate::doc::AppState;
use crate::icons::Icon;
use crate::paint::{self, Dab, PaintLayer, PaintMode};
use crate::plate::{self, Card};

/// Default new-project resolution (the New Project dialog's starting values).
pub const DEFAULT_PAPER_W: u32 = 1920;
pub const DEFAULT_PAPER_H: u32 = 1080;

/// The pencil case (spec §6 defect 5): ink · ao · aka · white. Ao and Aka are
/// the trade's blue/red pencils at the plate's own values, so the swatch board
/// reads by colour alone, peripherally, without stopping the stroke.
pub(crate) const SWATCHES: [[u8; 4]; 4] = [
    [25, 25, 30, 255],             // ink
    plate::PENCIL_AO.to_array(),   // ao — construction / blue pencil
    plate::PENCIL_AKA.to_array(),  // aka — the sakkan's correction
    [245, 245, 245, 255],          // white
];

/// The private fields AU4's deck walk reads back.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeckReads {
    pub sel_shape: SelShape,
    pub fill_ref_cel: bool,
    pub alpha_lock: bool,
    pub dyn_size: bool,
    pub flip_held: bool,
}

/// An engraved label over a track, with its value beside it.
fn value_line(ui: &mut egui::Ui, label: &str, value: String) {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &plate::sentence(label),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(9.5, plate::semibold()),
            color: plate::LEGEND,
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

/// GM4: the keyed zoom stops, in screen points per paper px (1.0 = 100%).
pub const ZOOM_LADDER: [f32; 14] = [
    0.125,
    0.25,
    1.0 / 3.0,
    0.5,
    2.0 / 3.0,
    1.0,
    1.5,
    2.0,
    3.0,
    4.0,
    6.0,
    8.0,
    12.0,
    16.0,
];

/// GM4: the `[` / `]` brush-size stops, in paper px.
pub const SIZE_LADDER: [f32; 24] = [
    1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 20.0, 25.0, 30.0, 40.0, 50.0,
    60.0, 80.0, 100.0, 120.0, 150.0, 200.0, 250.0, 300.0,
];

/// GM4: the keyed rotate step and the Ctrl snap while turning.
pub const ROTATE_STEP_DEG: f32 = 15.0;

/// GM4: the next stop on `ladder` strictly above (`up`) or below `v`,
/// clamped at the ends.
pub fn ladder_step(ladder: &[f32], v: f32, up: bool) -> f32 {
    let eps = 1e-4 * v.abs().max(1.0);
    if up {
        ladder.iter().copied().find(|s| *s > v + eps).unwrap_or(*ladder.last().unwrap())
    } else {
        ladder.iter().rev().copied().find(|s| *s < v - eps).unwrap_or(ladder[0])
    }
}

/// GM4: the canvas view — paper to screen and back. The paper mirrors
/// about its own vertical centre line (display only), then turns about
/// its centre by `rotation_deg` (clockwise on screen, y down), scales by
/// `zoom` (screen points per paper px) and sits at the pane `center`
/// plus `pan`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewXform {
    pub zoom: f32,
    pub pan: egui::Vec2,
    pub rotation_deg: f32,
    pub mirror: bool,
    /// The pane's centre on screen (where pan 0 puts the paper's centre).
    pub center: Pos2,
    /// The paper's size in paper px.
    pub paper: egui::Vec2,
}

impl Default for ViewXform {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            rotation_deg: 0.0,
            mirror: false,
            center: Pos2::ZERO,
            paper: vec2(DEFAULT_PAPER_W as f32, DEFAULT_PAPER_H as f32),
        }
    }
}

/// Turn `v` by `deg` (clockwise on a y-down screen).
pub fn rotate_vec(v: egui::Vec2, deg: f32) -> egui::Vec2 {
    let (s, c) = deg.to_radians().sin_cos();
    vec2(v.x * c - v.y * s, v.x * s + v.y * c)
}

impl ViewXform {
    pub fn paper_to_screen(&self, p: Pos2) -> Pos2 {
        let mut v = (p - self.paper * 0.5).to_vec2() * self.zoom;
        if self.mirror {
            v.x = -v.x;
        }
        self.center + self.pan + rotate_vec(v, self.rotation_deg)
    }

    pub fn screen_to_paper(&self, s: Pos2) -> Pos2 {
        let d = s - self.center - self.pan;
        let mut v = rotate_vec(d, -self.rotation_deg) / self.zoom.max(1e-6);
        if self.mirror {
            v.x = -v.x;
        }
        (self.paper * 0.5 + v).to_pos2()
    }

    /// A screen direction (pen tilt) in paper axes: turned back by the
    /// view angle and unmirrored, length kept.
    pub fn dir_to_paper(&self, d: [f32; 2]) -> [f32; 2] {
        let mut v = rotate_vec(vec2(d[0], d[1]), -self.rotation_deg);
        if self.mirror {
            v.x = -v.x;
        }
        [v.x, v.y]
    }

    /// The paper's corners on screen: (0,0), (w,0), (w,h), (0,h).
    pub fn paper_corners(&self) -> [Pos2; 4] {
        let (w, h) = (self.paper.x, self.paper.y);
        [pos2(0.0, 0.0), pos2(w, 0.0), pos2(w, h), pos2(0.0, h)].map(|p| self.paper_to_screen(p))
    }

    /// The screen box around the (possibly turned) paper.
    pub fn paper_bounds(&self) -> Rect {
        let c = self.paper_corners();
        Rect::from_points(&c)
    }
}

/// GM4: the textured paper quad — every paper image (below / active / wet
/// / above / remote / onion / other columns / composite) goes through
/// here, so it turns, mirrors and zooms with the view.
pub fn paper_image_mesh(xf: &ViewXform, tex: egui::TextureId, tint: Color32) -> egui::Mesh {
    let mut mesh = egui::Mesh::with_texture(tex);
    let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    for (pos, uv) in xf.paper_corners().into_iter().zip(uvs) {
        mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    mesh
}

fn paint_paper_image(painter: &egui::Painter, xf: &ViewXform, tex: egui::TextureId, tint: Color32) {
    painter.add(egui::Shape::mesh(paper_image_mesh(xf, tex, tint)));
}

/// GM20: a paint-layer image (below / active / wet / above / remote) is
/// layer-sized: its quad covers the layer rect, beyond the frame when the
/// paper is wider; a frame rect gives the paper quad exactly.
pub fn layer_image_mesh(
    xf: &ViewXform,
    rect: &paint::LayerRect,
    tex: egui::TextureId,
    tint: Color32,
) -> egui::Mesh {
    let mut mesh = egui::Mesh::with_texture(tex);
    let (x0, y0) = (rect.origin.0 as f32, rect.origin.1 as f32);
    let (x1, y1) = (x0 + rect.size.0 as f32, y0 + rect.size.1 as f32);
    let corners = [pos2(x0, y0), pos2(x1, y0), pos2(x1, y1), pos2(x0, y1)];
    let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    for (p, uv) in corners.into_iter().zip(uvs) {
        let pos = xf.paper_to_screen(p);
        mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    mesh
}

fn paint_layer_image(
    painter: &egui::Painter,
    xf: &ViewXform,
    rect: &paint::LayerRect,
    tex: egui::TextureId,
    tint: Color32,
) {
    painter.add(egui::Shape::mesh(layer_image_mesh(xf, rect, tex, tint)));
}

/// GM4: one sample of the stroke as the paper receives it — after the
/// view mapping and the stabilizer. The test tap: recorded even without
/// a GPU paint layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeSample {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub tilt: [f32; 2],
}

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
    /// GM18: the panel (koma) tool — a drag on a page draws a panel, splits
    /// one, or moves a vanishing point or the horizon.
    Panel,
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

/// A handle drag, measured in PAPER space (GM4): under a turned or
/// mirrored view a screen delta is not a paper delta, so the grab point,
/// its distance and its angle about the pivot are all taken on the paper.
enum FloatDrag {
    Move {
        start_paper: Pos2,
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

/// GM20: the strips of the paper sheet `area` the paint layer `layer` does
/// not cover — past a capped paper's window, or around a rect held while a
/// stroke lands — which the canvas veils. Frame px; none when the layer
/// covers the sheet.
pub(crate) fn unpainted_strips(area: IRect, layer: &paint::LayerRect) -> Vec<IRect> {
    let (ax0, ay0) = (area.x as i64, area.y as i64);
    let (ax1, ay1) = (ax0 + area.w as i64, ay0 + area.h as i64);
    let (ox, oy) = (layer.origin.0 as i64, layer.origin.1 as i64);
    let (lx0, ly0) = (ox.clamp(ax0, ax1), oy.clamp(ay0, ay1));
    let (lx1, ly1) = (
        (ox + layer.size.0 as i64).clamp(ax0, ax1),
        (oy + layer.size.1 as i64).clamp(ay0, ay1),
    );
    if lx1 <= lx0 || ly1 <= ly0 {
        return vec![area]; // the layer is off the sheet
    }
    let strip = |x0: i64, y0: i64, x1: i64, y1: i64| {
        (x1 > x0 && y1 > y0).then(|| IRect {
            x: x0 as i32,
            y: y0 as i32,
            w: (x1 - x0) as u32,
            h: (y1 - y0) as u32,
        })
    };
    // Left and right the sheet's full height; above and below between them.
    [
        strip(ax0, ay0, lx0, ay1),
        strip(lx1, ay0, ax1, ay1),
        strip(lx0, ay0, lx1, ly0),
        strip(lx0, ly1, lx1, ay1),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// GM22: the 3D underlay as the canvas drew it this frame: the node and
/// frame its picture shows, the frame rect it covers, its opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UnderlayDrawn {
    pub node: anim_core::ids::NodeId,
    pub frame: u32,
    pub rect: anim_core::camera::IRect,
    pub opacity: f32,
}

/// GM10: the camera furniture the canvas puts on the sheet, in paper
/// (frame) pixels: the guides come from `anim_core::camera` (one law with
/// the viewer's safe frames), the paper and the camera's frame from the
/// cut. Recorded each frame as drawn.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Furniture {
    /// The cut's paper when it is not the frame (`camera.paper`).
    pub paper: Option<anim_core::camera::IRect>,
    /// The output camera's frame on the paper, corners clockwise from its
    /// top left.
    pub camera: Option<[(f32, f32); 4]>,
    /// Fields 1 to 12, while the field toggle is on (rects of the camera's
    /// frame; `quad` puts them on the paper).
    pub fields: Vec<anim_core::camera::Rect>,
    /// Action and title safe, while the safe toggle is on (likewise).
    pub safe: Option<anim_core::camera::SafeFrames>,
    /// The output camera's frame map (its z 0 plane) the guides ride on:
    /// None without a camera, when the camera's frame is the frame itself.
    pub view: Option<anim_core::camera::PlaneMap>,
}

impl Furniture {
    /// A point of the camera's frame (a guide's) on the paper.
    pub(crate) fn on_paper(&self, (u, v): (f32, f32)) -> (f32, f32) {
        self.view.map_or((u, v), |m| m.map(u, v))
    }

    /// A guide rect's corners on the paper, clockwise from its top left —
    /// where the canvas draws it: a panned, zoomed or turned camera carries
    /// its field chart and safe frames along.
    pub(crate) fn quad(&self, r: &anim_core::camera::Rect) -> [(f32, f32); 4] {
        r.corners().map(|p| self.on_paper(p))
    }

    /// What goes on a `width`×`height` sheet for `cut` at `frame`.
    pub(crate) fn of(
        cut: &anim_core::model::Cut,
        frame: u32,
        (width, height): (u32, u32),
        field: bool,
        safe: bool,
    ) -> Self {
        use anim_core::camera::{self, CameraState, IRect};
        let paper = camera::paper(cut, width, height);
        let st = camera::output_camera(cut).and_then(|n| CameraState::at(cut, n, frame));
        Self {
            paper: (paper != IRect::frame(width, height)).then_some(paper),
            camera: st.as_ref().map(|st| camera::frame_rect_on_paper(st, width, height)),
            fields: if field { camera::field_guide(width, height) } else { Vec::new() },
            safe: safe.then(|| camera::safe_frames(width, height)),
            view: st.map(|st| st.plane_map(0.0, width, height)),
        }
    }
}

pub struct CanvasView {
    zoom: f32,
    pan: egui::Vec2,
    pub brush_width: f32,
    pub brush_color: [u8; 4],
    /// GM7 (D8): the open project's colour law, synced each frame — every
    /// chip → stored value on the canvas goes through it ([`Self::ink`]).
    pub(crate) color_law: ColorLaw,
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
    /// GM20: the paint layer's rect (`sync_layer_rect`), kept with or
    /// without a GPU; and the GPU's texture side limit it is capped by.
    pub layer_rect: paint::LayerRect,
    pub max_layer_dim: u32,
    /// The paint layer generation the synced keys belong to.
    synced_generation: u64,
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
    /// AU4: the rail's Flip key is down: R by another hand. The rail writes
    /// it after this view's frame, so it reads a frame late.
    pub(crate) rail_flip: bool,
    /// AU4: the rail's Hand: a held state like Space, no tool of its own.
    /// Any tool clears it.
    pub(crate) hand_held: bool,
    /// AU4: the strip is closed by its latch here, and may open. The host
    /// sets it before `ui`: the rail's reference height and Fit read it.
    pub(crate) strip_closed: bool,
    /// AU4: the sheet is on screen: the rule's mirror stands beside it.
    pub(crate) sheet_shown: bool,
    /// Refusal edge-flash bookkeeping (seq last seen + when).
    flash_seq: u32,
    flash_since: f64,
    /// GM6: the onion's depth, mode and opacity (the old lightbox strength
    /// is its `opacity`) and the light table's settings — the shell feeds
    /// them from the config every frame; a light box edit lands in the
    /// requests below for the shell to save.
    pub onion: OnionCfg,
    pub light_table: LightTableCfg,
    pub(crate) onion_request: Option<OnionCfg>,
    pub(crate) light_table_request: Option<LightTableCfg>,
    /// GM6: a shift & trace drag in flight (its last paper point).
    shift_drag: Option<Pos2>,
    /// GM6: the chart editor's mark count, and a chart tick being dragged
    /// (the drawing, the mark, the marks so far — committed on release as
    /// one undo step).
    chart_count: usize,
    chart_drag: Option<(DrawingId, usize, Vec<f32>)>,
    /// GM6: the verbs the canvas asked for (Ctrl+C / Ctrl+V arrive as the
    /// system's Copy / Paste events, not keys); the Editor runs them
    /// through the one dispatch after the panes (D6).
    pub(crate) asked: Vec<crate::config::Action>,
    /// Paper furniture: field / safe-area / peg-bar guides, in Ao.
    show_field: bool,
    show_safe: bool,
    show_peg: bool,
    /// GM10: the camera furniture as drawn this frame.
    pub(crate) furniture: Furniture,
    /// GM22: View ▸ 3D underlay (a view, kept for the session) and what it
    /// drew this frame.
    pub underlay: crate::view3d::Underlay,
    pub(crate) underlay_drawn: Option<UnderlayDrawn>,
    /// GM22: this frame showed the composite (the underlay is edit view's).
    pub(crate) composite_shown: bool,
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
    /// AU4: `Make a brush`, in the Brushes float. The rooms are the Editor's.
    pub(crate) request_brush_maker: bool,
    brush_search: String,
    /// Lazy thumbnail textures by preset name; None = no cached thumb
    /// (the painted-dab fallback draws instead — never tofu).
    thumb_cache: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    /// Session mixing splotches: (position 0..1 in the tray, radius, colour).
    /// Never saved — the model is the only colour truth (NEVER-DO 2).
    splotches: Vec<([f32; 2], f32, [u8; 4])>,
    /// The Eye is editing this palette role (char idx, role idx); "set"
    /// writes it back, DANGER-guarded (palettes have no undo).
    pub(crate) eye_target: Option<(usize, usize)>,
    /// The Eye's sticky HSV (hue survives greys) + the rgb it derives from.
    eye_hsv: [f32; 3],
    eye_sync_rgb: [u8; 3],
    /// Session colour memory per layer NAME: picking a colour while a layer is
    /// active remembers it here (overrides the Settings default until close).
    /// The canvas area rect of the last frame — read by the headless layout
    /// probe (ANIMSTUDIO_PROBE) to detect ANY movement of the drawing area.
    pub dbg_rect: Rect,
    /// AU1: the layer `dbg_rect` lives on — the paper-share audit's "is this
    /// the canvas" (`layer_id_at` names a layer, never a widget).
    pub dbg_layer: egui::LayerId,
    /// AU1: `dbg_rect` holds Layout's preview, not this view: no paper in it,
    /// and `view_xform` is whatever room drew last.
    pub dbg_preview: bool,
    /// Set once a pen comes down on the canvas's own layer. After that, the
    /// mouse-drawing fallback is disabled so Windows' synthesized pen→mouse
    /// clicks (e.g. from a fast light double-tap) can't paint flat-pressure
    /// blobs. A Touch on the rail or a float says nothing of the sort: the
    /// mouse beside it must still draw.
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
    /// GM4: the view's turn (degrees, clockwise on screen) and its
    /// display-only left-right mirror.
    rotation_deg: f32,
    mirror: bool,
    /// GM4: the last frame's fit scale (screen points per paper px at
    /// zoom 1) and view — keyed zoom and the tests read them.
    last_fit: f32,
    xf: ViewXform,
    /// GM4: the Space hand. `space_was_down` finds the hold's edges; the
    /// hand is engaged for a hold that began off-stroke; `hand_turn` is a
    /// Shift-turn's grab (start angle, start pointer angle, start pan);
    /// `hand_view` the view (zoom, pan, turn, mirror) the hold began on —
    /// a hold that moved it is a drag, never a PlayPause tap.
    space_was_down: bool,
    hand_engaged: bool,
    hand_turn: Option<(f32, f32, egui::Vec2)>,
    hand_view: (f32, egui::Vec2, f32, bool),
    /// GM4: the stroke tap — every sample the paper receives, after the
    /// view mapping and the stabilizer; cleared at stroke start, recorded
    /// with or without a paint layer.
    pub stroke_tap: Vec<StrokeSample>,
    /// GM5: the last frame's column stacks around the active column
    /// (`Cut::render_split`: below, above) — what the edit view drew; the
    /// headless tests read it.
    pub stacks: (Vec<ColumnId>, Vec<ColumnId>),
    /// GM4: the stabilizer level (pen config), the live string, and the
    /// last raw paper point fed to it (the pen-up point).
    stabilizer: u8,
    stab: Option<crate::stabilizer::Stabilizer>,
    stab_raw: Pos2,
    /// GM18: the page's perspective rulers while their snap is on (read
    /// each frame), and the live stroke's hold on them — the tap feeds it
    /// before the stabilizer.
    pub(crate) snap_rulers: Option<anim_core::manga::Rulers>,
    ruler_snap: Option<anim_core::manga::Snap>,
    /// GM18: the panel tool's live drag (paper px), committed at its end.
    pub(crate) koma: Option<crate::manga_tools::KomaDrag>,
    /// GM4: an OPTIONS-row stabilizer change for the app to save.
    pub(crate) stabilizer_request: Option<u8>,
    /// GM4: the dab edge (MyPaint hardness) and the aliased toggle.
    pub brush_hardness: f32,
    pub brush_aliased: bool,
    /// GM4: where the last stroke ended — Shift+click rules on from here —
    /// with where it landed (scene, cut, column, frame, layer slot) and the
    /// document revision its commit left (None until the commit lands). A
    /// move off that spot or any edit after it retires the anchor.
    line_anchor: Option<Pos2>,
    anchor_at: AnchorAt,
    anchor_rev: Option<u64>,
}

/// GM4: the spot a Shift+click line continues on — scene, cut, column,
/// frame and the armed layer slot.
type AnchorAt = (SceneId, CutId, ColumnId, u32, Option<usize>);

fn anchor_at(state: &AppState) -> AnchorAt {
    let v = &state.view;
    (v.scene, v.cut, v.active_column, v.frame, state.active_slot_resolved())
}

impl CanvasView {
    pub fn new() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            brush_width: 3.0,
            brush_color: SWATCHES[0],
            color_law: ColorLaw::Legacy,
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
            layer_rect: paint::LayerRect::frame(DEFAULT_PAPER_W, DEFAULT_PAPER_H),
            max_layer_dim: paint::DEFAULT_MAX_DIM,
            synced_generation: 0,
            synced_below: None,
            synced_above: None,
            raster_new_cel: false,
            pen_curve: PressureCurve::linear(),
            armed_preset: None,
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
            rail_flip: false,
            hand_held: false,
            strip_closed: false,
            sheet_shown: false,
            flash_seq: 0,
            flash_since: -10.0,
            onion: OnionCfg::default(),
            light_table: LightTableCfg::default(),
            onion_request: None,
            light_table_request: None,
            shift_drag: None,
            chart_count: 3,
            chart_drag: None,
            asked: Vec::new(),
            show_field: false,
            show_safe: false,
            show_peg: false,
            furniture: Furniture::default(),
            underlay: crate::view3d::Underlay::default(),
            underlay_drawn: None,
            composite_shown: false,
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
            request_brush_maker: false,
            brush_search: String::new(),
            thumb_cache: std::collections::HashMap::new(),
            splotches: Vec::new(),
            eye_target: None,
            eye_hsv: [0.0, 0.0, 0.0],
            eye_sync_rgb: [0, 0, 0],
            dbg_rect: Rect::NOTHING,
            dbg_layer: egui::LayerId::background(),
            dbg_preview: false,
            seen_pen: false,
            native_active: false,
            stroke_from_mouse: false,
            dbg_mouse_mode: false,
            rotation_deg: 0.0,
            mirror: false,
            last_fit: 1.0,
            xf: ViewXform::default(),
            space_was_down: false,
            hand_engaged: false,
            hand_turn: None,
            hand_view: (1.0, egui::Vec2::ZERO, 0.0, false),
            stroke_tap: Vec::new(),
            stacks: Default::default(),
            stabilizer: 0,
            stab: None,
            stab_raw: Pos2::ZERO,
            snap_rulers: None,
            ruler_snap: None,
            koma: None,
            stabilizer_request: None,
            brush_hardness: crate::config::default_hardness(),
            brush_aliased: false,
            line_anchor: None,
            anchor_at: (SceneId(u64::MAX), CutId(u64::MAX), ColumnId(u64::MAX), u32::MAX, None),
            anchor_rev: None,
        }
    }

    /// Flip between brush and eraser (bound to a rebindable shortcut).
    pub fn toggle_eraser(&mut self) {
        self.hand_held = false;
        self.erasing = !self.erasing;
    }

    /// AU4: the rail's Brush and Eraser. `set_tool` refuses a live stroke by
    /// itself only when the tool changes; ink turning to eraser under the pen
    /// is the same refusal.
    pub(crate) fn arm_paint(&mut self, erasing: bool, state: &mut AppState) {
        self.hand_held = false;
        if self.touch_active || !self.current.is_empty() || self.raster_stroke_done {
            state.refuse("refused — finish the stroke first");
            return;
        }
        self.set_tool(CanvasTool::Paint, state);
        self.erasing = erasing;
    }

    /// Flip alpha-lock (bound to a rebindable shortcut).
    pub fn toggle_alpha_lock(&mut self) {
        self.alpha_lock = !self.alpha_lock;
    }

    /// Apply a brush preset wholesale (keybind 1–8, Presets pane, or a
    /// workspace switch bound to it). Also drops the eraser — a preset is ink.
    pub fn apply_preset(&mut self, p: &crate::config::BrushPreset) {
        self.hand_held = false;
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
        self.brush_hardness = p.hardness;
        self.brush_aliased = p.aliased;
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
            hardness: self.brush_hardness,
            aliased: self.brush_aliased,
        }
    }

    /// GM3: fit the canvas back in its pane — no pan, the whole paper in
    /// view (the rail's fit button and View › Fit view). GM4: a turned view
    /// keeps its turn and fits the turned paper's box.
    pub fn fit_view(&mut self) {
        self.zoom = self.turned_fit();
        self.pan = egui::Vec2::ZERO;
        self.sync_xf();
    }

    /// GM7: frame a paper box `[x0, y0, x1, y1)` — the paint check's jump.
    /// The box fills about half the pane (within the zoom ladder) and its
    /// centre comes to the pane's centre; turn and mirror are kept.
    pub fn look_at_paper(&mut self, bbox: [u32; 4]) {
        let pane = self.dbg_rect.size();
        let bw = bbox[2].saturating_sub(bbox[0]).max(1) as f32;
        let bh = bbox[3].saturating_sub(bbox[1]).max(1) as f32;
        if pane.x > 0.0 && pane.y > 0.0 {
            let z = (pane.x / bw).min(pane.y / bh) * 0.5;
            self.set_abs_zoom(z.clamp(ZOOM_LADDER[0], ZOOM_LADDER[ZOOM_LADDER.len() - 1]));
        }
        let q = pos2((bbox[0] + bbox[2]) as f32 * 0.5, (bbox[1] + bbox[3]) as f32 * 0.5);
        let mut v = (q - self.xf.paper * 0.5).to_vec2() * self.xf.zoom;
        if self.mirror {
            v.x = -v.x;
        }
        self.pan = -rotate_vec(v, self.rotation_deg);
        self.sync_xf();
    }

    /// GM4: the relative zoom that fits the turned paper — its screen box,
    /// (w|cos|+h|sin|) × (w|sin|+h|cos|), against the pane, over the
    /// upright fit. Exactly 1 upright (and before the first frame).
    fn turned_fit(&self) -> f32 {
        let (w, h) = (self.xf.paper.x, self.xf.paper.y);
        let pane = self.dbg_rect.size();
        if self.rotation_deg == 0.0 || w <= 0.0 || h <= 0.0 || pane.x <= 0.0 || pane.y <= 0.0 {
            return 1.0;
        }
        let (s, c) = self.rotation_deg.to_radians().sin_cos();
        let (s, c) = (s.abs(), c.abs());
        let (bw, bh) = (w * c + h * s, w * s + h * c);
        (pane.x / bw).min(pane.y / bh) / (pane.x / w).min(pane.y / h)
    }

    /// GM4: the view as last drawn (screen ↔ paper).
    pub fn view_xform(&self) -> ViewXform {
        self.xf
    }

    /// GM4: the view's turn in degrees and its mirror.
    pub fn view_rotation(&self) -> f32 {
        self.rotation_deg
    }

    pub fn view_mirrored(&self) -> bool {
        self.mirror
    }

    /// GM4: screen points per paper px (1.0 = 100%). `zoom` is held relative
    /// to the fit, and that round trip can miss a stop by its last bit: a stop
    /// reads back as itself, or the key that landed on it would land on it
    /// again.
    pub fn abs_zoom(&self) -> f32 {
        let z = self.last_fit * self.zoom;
        let reached = |stop: &f32| (stop - z).abs() <= stop * 2.0 * f32::EPSILON;
        ZOOM_LADDER.iter().copied().find(reached).unwrap_or(z)
    }

    /// GM4: zoom to `z` screen points per paper px about the pane centre.
    fn set_abs_zoom(&mut self, z: f32) {
        let old = self.abs_zoom().max(1e-6);
        self.zoom = z / self.last_fit.max(1e-6);
        self.pan *= z / old;
        self.sync_xf();
    }

    /// GM4: Zoom in / out one ladder stop, and 100%.
    pub fn zoom_in(&mut self) {
        self.set_abs_zoom(ladder_step(&ZOOM_LADDER, self.abs_zoom(), true));
    }

    pub fn zoom_out(&mut self) {
        self.set_abs_zoom(ladder_step(&ZOOM_LADDER, self.abs_zoom(), false));
    }

    pub fn zoom_100(&mut self) {
        self.set_abs_zoom(1.0);
    }

    /// GM4: turn the view to `deg` (kept in (-180, 180]) about the pane
    /// centre, so the paper point there stays put.
    pub fn set_view_rotation(&mut self, deg: f32) {
        let mut d = deg % 360.0;
        if d > 180.0 {
            d -= 360.0;
        } else if d <= -180.0 {
            d += 360.0;
        }
        self.pan = rotate_vec(self.pan, d - self.rotation_deg);
        self.rotation_deg = d;
        self.sync_xf();
    }

    pub fn rotate_view(&mut self, deg: f32) {
        self.set_view_rotation(self.rotation_deg + deg);
    }

    /// GM4: mirror the display left to right about the pane centre.
    pub fn toggle_mirror(&mut self) {
        let v = rotate_vec(self.pan, -self.rotation_deg);
        self.pan = rotate_vec(vec2(-v.x, v.y), self.rotation_deg);
        self.mirror = !self.mirror;
        self.sync_xf();
    }

    /// GM4: `[` / `]` — the brush size one stop along the size ladder.
    pub fn brush_size_step(&mut self, up: bool) {
        self.raster_brush_px = ladder_step(&SIZE_LADDER, self.raster_brush_px, up);
    }

    pub fn brush_size(&self) -> f32 {
        self.raster_brush_px
    }

    /// AU4: the rail's Size: what the active route draws at, as it stands.
    pub(crate) fn active_px(&self) -> f32 {
        if self.raster { self.raster_brush_px } else { self.brush_width }
    }

    /// AU4: the range of [`Self::active_px`], as the old sliders had it.
    pub(crate) fn px_range(&self) -> (f32, f32) {
        if self.raster { (1.0, 300.0) } else { (0.5, 16.0) }
    }

    /// AU4: the rail's Size sets the active route's size, never mid-stroke
    /// (the live stroke's dabs would change under the pen). Answers whether
    /// it took.
    pub(crate) fn set_brush_px(&mut self, px: f32, state: &mut AppState) -> bool {
        if self.stroke_active() {
            state.refuse("refused — finish the stroke first");
            return false;
        }
        let (lo, hi) = self.px_range();
        let px = px.clamp(lo, hi);
        if self.raster {
            self.raster_brush_px = px;
        } else {
            self.brush_width = px;
        }
        true
    }

    /// The zoom a dab reads at: the rail's bubble shows the brush true size.
    pub(crate) fn view_scale(&self) -> f32 {
        self.xf.zoom
    }

    fn sync_xf(&mut self) {
        self.xf.zoom = self.last_fit * self.zoom;
        self.xf.pan = self.pan;
        self.xf.rotation_deg = self.rotation_deg;
        self.xf.mirror = self.mirror;
    }

    /// GM3: the live selection as a paper-space polygon (Delete clears
    /// only inside it).
    pub fn selection_poly(&self) -> Option<Vec<(f32, f32)>> {
        self.selection
            .as_ref()
            .map(|p| p.iter().map(|q| (q.x, q.y)).collect())
    }

    /// GM3 tests: a selection without the GPU the select tool draws with.
    #[cfg(test)]
    pub fn set_selection(&mut self, poly: Vec<Pos2>) {
        self.selection = Some(poly);
    }

    /// GM4: the view as the Space hold compares it — zoom, pan, turn, mirror.
    fn view_now(&self) -> (f32, egui::Vec2, f32, bool) {
        (self.zoom, self.pan, self.rotation_deg, self.mirror)
    }

    /// GM4: the pen is mid-gesture — a stroke (down, or its pen-up commit
    /// pending), a selection outline being drawn, or a transform handle
    /// being dragged (GM18: or a panel-tool drag). A Space hold that starts
    /// here never takes the hand.
    pub(crate) fn pen_busy(&self) -> bool {
        self.touch_active
            || !self.current.is_empty()
            || self.raster_stroke_done
            || self.sel_draft.is_some()
            || self.floating.as_ref().is_some_and(|f| f.drag.is_some())
            || self.koma.is_some()
    }

    /// GM20: the side the paint layer paints at most — the room's limit
    /// (`paint::room_limit` of the device's `max_texture_dimension_2d`).
    pub fn with_layer_limit(mut self, max_dim: u32) -> Self {
        self.max_layer_dim = max_dim;
        self
    }

    /// GM20: THE layer rect of a paint area at this canvas's side limit —
    /// the one helper behind every rect the paint layer is sized to (the
    /// canvas's own and a replay's).
    fn rect_of(&self, state: &AppState, area: IRect) -> paint::LayerRect {
        let frame = (state.engine.project.width, state.engine.project.height);
        paint::layer_rect(area, frame, self.max_layer_dim)
    }

    /// GM20: the canvas's layer rect: the cut's paper (when it holds the
    /// frame) capped by the side limit. Held while a stroke is in progress —
    /// a paper set mid-stroke takes effect after the stroke lands.
    pub fn sync_layer_rect(&mut self, state: &AppState) -> paint::LayerRect {
        if !self.stroke_active() {
            self.layer_rect = self.rect_of(state, state.paper_area());
        }
        self.layer_rect
    }

    /// GM20: the rect a replayed stroke paints in: its OWN cut's paper
    /// (found project-wide) at the room's side limit — never the viewer's
    /// cut nor a rect held under the local pen, so every peer replays a
    /// stroke into the same rect and reads back the same tiles.
    pub(crate) fn replay_rect(&self, state: &AppState, drawing: DrawingId) -> paint::LayerRect {
        self.rect_of(state, state.paper_area_of(drawing))
    }

    /// Bring the paint layer onto the canvas's layer rect (the viewer asks
    /// here too).
    pub(crate) fn ensure_layer(&mut self, state: &AppState, p: &mut PaintLayer) {
        let rect = self.sync_layer_rect(state);
        self.layer_onto(p, rect);
    }

    /// The paint layer's one resize: a rebuild cleared the targets, so every
    /// synced key goes stale.
    fn layer_onto(&mut self, p: &mut PaintLayer, rect: paint::LayerRect) {
        p.ensure_size(rect);
        if p.generation() != self.synced_generation {
            self.synced_generation = p.generation();
            self.synced_active = (u64::MAX, u64::MAX, u64::MAX);
            self.synced_below = None;
            self.synced_above = None;
        }
    }

    /// An edit gesture is live: a stroke (pen down or its pen-up commit
    /// pending), a selection being drawn, a lifted transform in flight, or
    /// (GM18) a panel-tool drag that lands on the page at its release.
    /// Gesture-unsafe actions (frame nav, undo, layer cycle, clears) must not
    /// dispatch while this holds — they would retarget or orphan the commit.
    pub fn stroke_active(&self) -> bool {
        self.touch_active
            || !self.current.is_empty()
            || self.raster_stroke_done
            || self.sel_draft.is_some()
            || self.floating.is_some()
            || self.koma.is_some()
    }

    /// GM22: why View ▸ 3D underlay, ticked, shows nothing (its menu note):
    /// no Model3D node on this cut, the composite on the canvas, or a BG
    /// column drawn over it (the underlay sits under every column).
    pub(crate) fn underlay_note(&self, state: &AppState) -> Option<&'static str> {
        if !self.underlay.on {
            return None;
        }
        let cut = state.cut();
        if crate::view3d::model_node(cut, state.selected_node).is_none() {
            return Some("not shown: this cut has no Model3D node");
        }
        if self.composite_shown {
            return Some("not shown: composite view");
        }
        let frame = state.view.frame;
        let bg = cut.xsheet.columns.iter().any(|c| {
            cut.column_def(c.id).kind == anim_core::xsheet::ColumnKind::Bg
                && cut.column_renders(c.id)
                && c.resolve(frame).is_some()
        });
        bg.then_some("under the BG column — hide it in the sheet to see the model")
    }

    /// Switch to the Select tool (V) / back to Paint (B). A live stroke
    /// blocks the switch (the stroke pipeline would be orphaned mid-flight);
    /// leaving Select commits any floating transform rather than losing it.
    pub fn set_tool(&mut self, tool: CanvasTool, state: &mut AppState) {
        self.hand_held = false;
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
        self.line_anchor = None; // a ruled line follows a stroke, not a tool switch
        self.tool = tool;
        state.status = match tool {
            CanvasTool::Select => "select: drag = select, drag inside = move, corners = scale, \
        just outside corners = rotate; Enter applies, Esc cancels"
                .into(),
            CanvasTool::LassoFill => "lasso fill: draw a loop — it fills with the brush             colour; softness feathers inward, grow shifts the edge"
                .into(),
            CanvasTool::Fill => "fill: click a region — line art bounds it; gap closes line \
            breaks, under tucks the flat beneath the lines; Alt+click picks a colour; \
            Shift+click refills a painted flat with the brush colour, Ctrl+Shift+click \
            replaces that colour on the whole layer"
                .into(),
            CanvasTool::Paint => "paint: Alt+click picks a colour, Shift+click rules on from \
            the last stroke, Space+drag pans (Shift+drag turns)"
                .into(),
            CanvasTool::Panel if anim_core::manga::Page::of(state.cut()).is_none() => {
                "panel (koma): this cut is no page — New page makes one".into()
            }
            CanvasTool::Panel => "panel (koma): drag on bare paper draws a panel, across a panel \
            splits it at the gutter, from a VP or the horizon moves it — one undo step each"
                .into(),
        };
        self.koma = None;
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

    /// Ctrl+A: select the whole paper (switches to the Select tool) — GM20:
    /// the paper beyond the frame too, as far as the GPU paints it (the
    /// layer rect: a capped paper's window).
    pub fn select_all(&mut self, state: &mut AppState) {
        self.set_tool(CanvasTool::Select, state);
        if self.tool != CanvasTool::Select {
            return; // switch refused mid-stroke
        }
        let a = self.sync_layer_rect(state).irect();
        let (x0, y0) = (a.x as f32, a.y as f32);
        let (x1, y1) = (x0 + a.w as f32, y0 + a.h as f32);
        self.selection = Some(vec![pos2(x0, y0), pos2(x1, y0), pos2(x1, y1), pos2(x0, y1)]);
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
        let Some(punched) = self.lift_floating(state, ctx, poly) else {
            return false;
        };
        paint.sync_active(&punched);
        true
    }

    /// The CPU half of a lift: check the target, cut the pixels into a
    /// floating patch with its preview texture, and return the layer's
    /// tiles with the hole punched for the display. None = refused (the
    /// status line says why).
    fn lift_floating(
        &mut self,
        state: &mut AppState,
        ctx: &egui::Context,
        poly: Option<&[Pos2]>,
    ) -> Option<BTreeMap<TileCoord, Arc<TileData>>> {
        if state.active_layer_props().is_some_and(|p| !p.visible) {
            state.status = format!(
                "layer '{}' is hidden — press A to switch or click its eye",
                state.active_layer_name()
            );
            return None;
        }
        if state.refuse_if_locked() {
            return None;
        }
        let Some(did) = state.own_key_drawing() else {
            state.status =
                "held/blank frame — a transform edits a frame's OWN cel (draw here first)".into();
            return None;
        };
        let (kd, kl, _) = state.active_layer_key();
        if kd != did.0 || kl == u64::MAX {
            state.status = "no raster layer here to transform".into();
            return None;
        }
        let Some(tiles) = state.active_layer_tiles().cloned() else {
            state.status = "no raster layer here to transform".into();
            return None;
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
            return None;
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
        Some(punched)
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
        // No GPU: nothing to lift or preview — but a transform already
        // floating still takes its handle drags.
        if paint.is_none() && self.floating.is_none() {
            return;
        }
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
                && let Some(paint) = paint
            {
                // Drag inside the selection = lift it and start moving.
                if self.try_lift(state, paint, ui.ctx(), Some(&sel))
                    && let Some(f) = &mut self.floating
                {
                    f.drag = Some(FloatDrag::Move {
                        start_paper: pp,
                        start_translate: f.translate,
                    });
                }
            } else {
                self.sel_draft = Some(vec![pp, pp]);
            }
        } else if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = ptr
        {
            if self.floating.is_some() {
                self.float_drag_to(to_paper(pos), 1.0 / scale.max(1e-6));
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

    /// Hit-test a drag start against the floating transform's handles (in
    /// screen px — the handles are screen-sized); the drag itself is taken
    /// on the paper (`pp`).
    fn float_drag_start(
        &mut self,
        pos: Pos2,
        pp: Pos2,
        to_screen: &impl Fn(Pos2) -> Pos2,
        state: &mut AppState,
    ) {
        let Some(f) = &mut self.floating else { return };
        let centre = f.pivot + f.translate;
        let corners_s: Vec<Pos2> = f.corners().iter().map(|c| to_screen(*c)).collect();
        let nearest = corners_s
            .iter()
            .map(|c| (pos - *c).length())
            .fold(f32::INFINITY, f32::min);
        if nearest <= 12.0 {
            f.drag = Some(FloatDrag::Scale {
                start_dist: (pp - centre).length(),
                start_scale: f.scale,
            });
            return;
        }
        if nearest <= 28.0 {
            f.drag = Some(FloatDrag::Rotate {
                start_angle: (pp - centre).angle(),
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
                    start_paper: pp,
                    start_translate: f.translate,
                });
                return;
            }
        }
        self.commit_floating(state);
    }

    /// Carry the floating transform's handle drag to the paper point `pp`.
    /// Moves follow the paper delta and turns the paper angle about the
    /// pivot, so a turned or mirrored view drags the pixels under the pen;
    /// `min_d` (one screen px, in paper px) keeps a scale grab off zero.
    fn float_drag_to(&mut self, pp: Pos2, min_d: f32) {
        let Some(f) = &mut self.floating else { return };
        let centre = f.pivot + f.translate;
        match &f.drag {
            Some(FloatDrag::Move {
                start_paper,
                start_translate,
            }) => {
                f.translate = *start_translate + (pp - *start_paper);
            }
            Some(FloatDrag::Scale {
                start_dist,
                start_scale,
            }) => {
                let d = (pp - centre).length().max(min_d);
                f.scale = (start_scale * d / start_dist.max(min_d)).clamp(0.05, 20.0);
            }
            Some(FloatDrag::Rotate {
                start_angle,
                start_rotate,
            }) => {
                f.rotate = start_rotate + ((pp - centre).angle() - start_angle);
            }
            None => {}
        }
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
                egui::Stroke::new(2.5, plate::halo(180)),
                6.0,
                6.0,
            ));
            painter.extend(egui::Shape::dashed_line(
                &s,
                egui::Stroke::new(1.0, plate::shade(230)),
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
                    color: plate::UNTINTED,
                });
            }
            mesh.indices.extend_from_slice(&[0, 1, 2, 2, 1, 3]);
            painter.add(egui::Shape::mesh(mesh));
            // Outline (TL→TR→BR→BL) + corner scale handles.
            ants(&[c[0], c[1], c[3], c[2]], true, painter);
            for p in &s {
                let r = Rect::from_center_size(*p, vec2(9.0, 9.0));
                painter.rect_filled(r, 1.5, plate::shade(200));
                painter.rect_stroke(
                    r,
                    1.5,
                    egui::Stroke::new(1.5, plate::halo(230)),
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
        self.line_anchor = None; // the anchor's stroke may be gone
    }

    /// GM4: the Shift+click anchor holds only while its stroke is the last
    /// thing that happened where the pen is. A frame, cut, column or layer
    /// change retires it, and so does any edit after the stroke's commit
    /// landed (undo, redo, another stroke's commit replaces it anyway).
    fn retire_line_anchor(&mut self, state: &AppState) {
        if self.line_anchor.is_none() {
            return;
        }
        if anchor_at(state) != self.anchor_at {
            self.line_anchor = None;
            return;
        }
        if self.stroke_active() {
            return; // a raster commit lands on the pen-up flush
        }
        let rev = state.engine.revision();
        match self.anchor_rev {
            None => self.anchor_rev = Some(rev),
            Some(r) if r != rev => self.line_anchor = None,
            Some(_) => {}
        }
    }

    /// THE FORGE reads the armed engine to seed a draft.
    pub(crate) fn armed_engine(&self) -> Option<&crate::config::EngineDef> {
        self.brush_engine.as_ref()
    }

    // (presence_wet is RETIRED — STAGE 2's dab stream IS the live view;
    // peers with old builds still send wet points and they still draw.)

    /// What AU4's deck walk reads back of the fields no float can show it.
    #[cfg(test)]
    pub fn deck_reads(&self) -> DeckReads {
        DeckReads {
            sel_shape: self.sel_shape,
            fill_ref_cel: self.fill_ref_cel,
            alpha_lock: self.alpha_lock,
            dyn_size: self.dyn_size,
            flip_held: self.flip_held,
        }
    }

    /// The slate's arming line: what the pen does right now, in a word.
    pub fn arming_pencil(&self) -> String {
        match self.tool {
            CanvasTool::Select => "select".into(),
            CanvasTool::Fill => "fill".into(),
            CanvasTool::LassoFill => "lasso fill".into(),
            CanvasTool::Panel => "panel (koma)".into(),
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
            && e(self.brush_hardness, p.hardness)
            && self.brush_aliased == p.aliased
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
        wide: f32,
        state: &mut AppState,
    ) {
        let (rect, resp) = ui.allocate_exact_size(vec2(wide, 44.0), Sense::click());
        let armed = self.tool == CanvasTool::Paint
            && !self.erasing
            && self.armed_preset.as_deref() == Some(p.name.as_str());
        plate::speak(
            &resp,
            egui::WidgetType::SelectableLabel,
            Some(armed),
            &format!("{} pencil ({digit})", p.name),
        );
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
        painter.rect_filled(rect, plate::R_CONTROL, plate::WELL);
        let ink = p.color.unwrap_or(plate::STRUCK.to_array());
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
            let col = plate::chip([ink[0], ink[1], ink[2], (a * 255.0) as u8]);
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
            plate::sentence(&p.name),
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
        // The lamp: solid blue ring armed; BROKEN ring = armed but the
        // live numbers have been nudged off the preset.
        if armed {
            if self.matches_preset(p) {
                painter.rect_stroke(
                    rect,
                    plate::R_CONTROL,
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
                plate::R_CONTROL,
                egui::Stroke::new(1.0, plate::LEGEND),
                egui::StrokeKind::Inside,
            );
        }
        // GM3: the registry's line first (the chord as bound now), then
        // the pencil's own numbers — built only while hovered.
        let detail = || {
            let mut more = format!(
                "{} — {:.0}px · flow {:.2} · opacity {:.2}",
                p.name, p.size_px, p.flow, p.opacity
            );
            if plate::exact() {
                more.push_str(&format!(
                    "\n[PENCIL SLOT '{}' · canvas.rs pencil_slot · preset in config.presets]",
                    p.name
                ));
            }
            more
        };
        match Action::preset_slot(digit) {
            Some(a) => {
                crate::menus::hover_tip_with(resp, a, detail);
            }
            None => {
                plate::hover_card(resp, detail);
            }
        }
    }

    /// THE PAINT DISH's content (its room, 2026-08-17): ARMED WELL with
    /// row context · THE MODEL (characters × roles — the colour-model
    /// table) · THE DISH (mixing splotches) · THE EYE (compact SV+hue).
    /// Picking never writes the model; only the Eye's held "set" and the
    /// row's held DERIVE write (DANGER — palettes have no undo).
    pub(crate) fn dish_ui(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
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
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let (wrect, _) = ui.allocate_exact_size(vec2(46.0, 46.0), Sense::hover());
                let p = ui.painter();
                p.rect_filled(wrect.expand(2.0), 0.0, plate::WELL);
                p.rect_filled(
                    wrect,
                    0.0,
                    plate::chip([
                        self.brush_color[0],
                        self.brush_color[1],
                        self.brush_color[2],
                        255,
                    ]),
                );
                p.rect_stroke(
                    wrect,
                    0.0,
                    egui::Stroke::new(1.0, plate::LEGEND),
                    egui::StrokeKind::Outside,
                );
                ui.vertical(|ui| {
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
                    if !row_chips.is_empty() {
                        ui.horizontal(|ui| {
                            for c in &row_chips {
                                let (cr, cresp) = ui.allocate_exact_size(vec2(13.0, 13.0), Sense::click());
                                let pnt = ui.painter();
                                pnt.rect_filled(
                                    cr,
                                    0.0,
                                    plate::chip([c[0], c[1], c[2], 255]),
                                );
                                pnt.rect_stroke(
                                    cr,
                                    0.0,
                                    egui::Stroke::new(1.0, plate::INK3),
                                    egui::StrokeKind::Inside,
                                );
                                if cresp.clicked() {
                                    self.brush_color = *c;
                                }
                            }
                        });
                    }
                });
            });
        });
        ui.add_space(8.0);

        // ---- THE MODEL: the colour-model table ----
        ui.vertical(|ui| {
            // The Color float's width, even where a host leaves more.
            ui.set_max_width(ui.available_width().min(256.0));
            plate::legend(ui, "model");
            // AU4: a block to a character: its name and `derive` over its own
            // roles, each chip over its whole word, and as many to a line as
            // the widest word leaves room for.
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            let font = plate::Role::Small.font();
            let word_w = |word: &str| {
                let galley = ui.painter().layout_no_wrap(plate::sentence(word), font.clone(), plate::LEGEND);
                galley.size().x
            };
            let words = state.palettes.characters.iter().flat_map(|c| &c.roles).map(|r| word_w(&r.name));
            let widest = words.fold(0.0_f32, f32::max);
            let room = ui.available_width() + 2.0;
            let cell_w = room / (room / (widest + 2.0)).floor().max(1.0) - 2.0;
            let n_chars = state.palettes.characters.len();
            for ci in 0..n_chars {
                let cname = state.palettes.characters[ci].name.clone();
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    plate::value(ui, &cname);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // DERIVE (held DANGER — writes the model, no undo there):
                        // this row's shadow from its normal, the systematic way.
                        // Held, it overwrites a chosen colour for good: it stays
                        // red, off the chips.
                        if plate::danger(ui, "derive") {
                            let normal = state.palettes.characters[ci]
                                .roles
                                .iter()
                                .find(|r| r.name == "normal")
                                .map(|r| r.color);
                            if let Some(norm) = normal {
                                let sh = crate::palette::derive_shadow(norm);
                                // GM2: through palettes_mut — unsaved work.
                                let ch = &mut state.palettes_mut().characters[ci];
                                let prev = ch
                                    .roles
                                    .iter()
                                    .position(|r| r.name == "shadow")
                                    .map(|ri| (ri, ch.roles[ri].color));
                                match ch.roles.iter_mut().find(|r| r.name == "shadow") {
                                    Some(r) => r.color = sh,
                                    None => ch.roles.push(crate::palette::ColorRole {
                                        name: "shadow".into(),
                                        color: sh,
                                    }),
                                }
                                // GM7: a changed chip offers to recolour its paint.
                                if let Some((ri, old)) = prev {
                                    state.offer_recolour(ci, ri, old, sh);
                                }
                                state.status =
                                    format!("{cname}: shadow derived from normal (edit in the eye)");
                            } else {
                                state.refuse("refused — no 'normal' role to derive from");
                            }
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    let n_roles = state.palettes.characters[ci].roles.len();
                    for ri in 0..n_roles {
                        let color = state.palettes.characters[ci].roles[ri].color;
                        let rname = state.palettes.characters[ci].roles[ri].name.clone();
                        let size = vec2(cell_w, 22.0 + plate::Role::Small.size() + 4.0);
                        let (cell, presp) = ui.allocate_exact_size(size, Sense::click());
                        plate::speak(&presp, egui::WidgetType::Button, None, &format!("{cname} {rname}"));
                        let pr = Rect::from_min_size(cell.min, vec2(cell_w, 22.0));
                        let well = pr.shrink2(vec2(2.0, 1.0));
                        let pnt = ui.painter();
                        pnt.rect_filled(well.expand(1.0), 0.0, plate::WELL);
                        pnt.rect_filled(
                            well,
                            0.0,
                            plate::chip([color[0], color[1], color[2], 255]),
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
                                egui::Stroke::new(1.0, plate::INK3),
                                egui::StrokeKind::Inside,
                            );
                        }
                        pnt.text(
                            pos2(cell.center().x, cell.bottom()),
                            egui::Align2::CENTER_BOTTOM,
                            plate::sentence(&rname),
                            font.clone(),
                            plate::LEGEND,
                        );
                        if presp.double_clicked() {
                            self.brush_color = color;
                            self.eye_target = Some((ci, ri));
                        } else if presp.clicked() {
                            self.brush_color = color;
                        }
                        presp.card(format!(
                            "{cname} · {rname} — click: arm · double-click: edit in the eye"
                        ));
                    }
                });
            }
        });
        ui.add_space(8.0);

        // ---- THE DISH: the mixing tray ----
        ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                plate::legend(ui, "dish");
                if ui
                    .button("rinse")
                    .card("clear the tray (splotches are session-only)")
                    .clicked()
                {
                    self.splotches.clear();
                }
            });
            let wide = ui.available_width().min(180.0);
            let (dr, dresp) = ui.allocate_exact_size(vec2(wide, 100.0), Sense::click());
            let pnt = ui.painter();
            pnt.rect_filled(dr, plate::R_CONTROL, plate::WELL);
            pnt.rect_stroke(
                dr,
                plate::R_CONTROL,
                egui::Stroke::new(1.0, plate::hairline()),
                egui::StrokeKind::Inside,
            );
            for (posn, r, c) in &self.splotches {
                pnt.circle_filled(
                    pos2(
                        dr.left() + posn[0] * dr.width(),
                        dr.top() + posn[1] * dr.height(),
                    ),
                    *r,
                    plate::chip([c[0], c[1], c[2], 255]),
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
            // The hue at full value is a picked colour (a chip); the
            // square's white and black ends are the plate's.
            let hue_col = plate::chip([hr_, hg, hb, 255]);
            let mut mesh = egui::Mesh::default();
            let idx = mesh.vertices.len() as u32;
            for (pt, col) in [
                (svr.left_top(), plate::PICK_WHITE),
                (svr.right_top(), hue_col),
                (svr.right_bottom(), plate::PICK_BLACK),
                (svr.left_bottom(), plate::PICK_BLACK),
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
                egui::Stroke::new(1.0, plate::hairline()),
                egui::StrokeKind::Inside,
            );
            let marker = pos2(
                svr.left() + self.eye_hsv[1] * svr.width(),
                svr.top() + (1.0 - self.eye_hsv[2]) * svr.height(),
            );
            // Dark and light rings: the square runs from white to black.
            pnt.circle_stroke(marker, 4.0, egui::Stroke::new(1.5, plate::STRUCK));
            pnt.circle_stroke(marker, 5.5, egui::Stroke::new(1.0, plate::halo(255)));
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
                    plate::chip([r0, g0, b0, 255]),
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
                            // GM2: through palettes_mut — unsaved work.
                            let old = state.palettes.characters[ci].roles[ri].color;
                            state.palettes_mut().characters[ci].roles[ri].color = self.brush_color;
                            // GM7: a changed chip offers to recolour its paint.
                            state.offer_recolour(ci, ri, old, self.brush_color);
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

    /// GM6: the chart editor's chart — this key at the top rule, the next
    /// key at the bottom; drag a mark up or down to move it between its
    /// neighbours. The drag shows live and lands as ONE undo step on
    /// release.
    fn chart_editor(
        &mut self,
        ui: &mut egui::Ui,
        state: &mut AppState,
        here: DrawingId,
        marks: &[f32],
        busy: bool,
    ) {
        let (r, resp) = ui.allocate_exact_size(vec2(40.0, 112.0), Sense::drag());
        let g = r.shrink2(vec2(8.0, 8.0));
        let t_at = |y: f32| (y - g.top()) / g.height();
        if resp.drag_started()
            && !busy
            && let Some(p) = resp.interact_pointer_pos()
        {
            let t = t_at(p.y);
            let nearest = marks
                .iter()
                .enumerate()
                .min_by(|a, b| (a.1 - t).abs().total_cmp(&(b.1 - t).abs()))
                .map(|(i, _)| i);
            self.chart_drag = nearest.map(|i| (here, i, marks.to_vec()));
        }
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
            && let Some((d, i, live)) = &mut self.chart_drag
            && *d == here
        {
            *live = crate::doc::move_chart_mark(live, *i, t_at(p.y));
        }
        let shown = match &self.chart_drag {
            Some((d, _, live)) if *d == here => live.clone(),
            _ => marks.to_vec(),
        };
        if resp.drag_stopped()
            && let Some((d, _, live)) = self.chart_drag.take()
            && d == here
            && !busy
        {
            state.set_chart(here, &live);
        }
        let lit = resp.hovered() || resp.dragged();
        ui.painter().rect_filled(r, 0.0, plate::WELL);
        chart_glyph(ui.painter(), g, &shown, if lit { plate::STRUCK } else { plate::AO });
        if lit {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        plate::speak(
            &resp,
            egui::WidgetType::Slider,
            None,
            &format!("spacing chart: {} marks (drag a mark to move it)", shown.len()),
        );
        resp.card("drag a mark up or down to move that inbetween's spacing (one undo step)");
    }

    /// The Canvas options float: onion + strength (with a value line — a
    /// silent dial reads as a bug), paper furniture, view, INPUT.
    pub(crate) fn lightbox_rail_ui(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
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
                plate::speak(&cresp, egui::WidgetType::Button, None, hover);
                let p = ui.painter();
                let (fill, ink) = plate::filled(&cresp);
                p.rect_filled(cr, plate::R_CONTROL, fill);
                crate::icons::paint(
                    p,
                    Rect::from_center_size(cr.center(), egui::Vec2::splat(22.0)),
                    ink,
                    icon,
                );
                if cresp.clicked() && !self.stroke_active() {
                    state.goto_adjacent_key(fwd);
                }
                cresp.card(hover);
            }
        });
        // GM3: the rail's tooltips lead with the registry's line too, so a
        // rebound key is named as bound.
        let mut mc = self.miss_check;
        crate::menus::hover_tip(
            plate::tool_latch(ui, &mut mc, Icon::Hole, "miss check", ""),
            Action::MissCheck,
            "hole-hunt: the ground goes dark; every unpainted pixel reads as a pit",
        );
        self.miss_check = mc;
        ui.add_space(10.0);
        plate::legend(ui, "lightbox");
        ui.add_space(2.0);
        let mut on = state.view.onion;
        crate::menus::hover_tip(
            plate::latch(ui, &mut on, "onion"),
            Action::ToggleOnion,
            "ghosts the neighbouring DRAWINGS — a hold of the same drawing has no neighbour to ghost",
        );
        state.view.onion = on;
        let mut lo = state.view.onion_layer_only;
        plate::latch(ui, &mut lo, "line only")
            .card("ghost only the layer matching the active layer's name");
        state.view.onion_layer_only = lo;
        ui.add_space(2.0);
        // GM6: depth, mode, other columns, opacity with falloff — kept in
        // the config (an edit lands in onion_request for the shell).
        let mut cfg = self.onion;
        value_line(ui, "strength", format!("{:.0}%", cfg.opacity * 100.0));
        ui.spacing_mut().slider_width = 96.0;
        ui.add(egui::Slider::new(&mut cfg.opacity, 0.1..=1.0).show_value(false))
            .card("the nearest ghost's opacity");
        // Folded by default: the fold-out keeps its old height (the view
        // block stays on screen).
        egui::CollapsingHeader::new("onion depth").show(ui, |ui| {
            value_line(ui, "falloff", format!("{:.0}%", cfg.falloff * 100.0));
            ui.add(egui::Slider::new(&mut cfg.falloff, 0.0..=1.0).show_value(false))
                .card("each ghost farther away keeps this much of the one before");
            value_line(ui, "behind · ahead", format!("{} · {}", cfg.before, cfg.after));
            ui.horizontal(|ui| {
                plate::field(ui, egui::DragValue::new(&mut cfg.before).range(0..=OnionCfg::MAX_DEPTH))
                    .card("ghosts behind (0–5)");
                plate::field(ui, egui::DragValue::new(&mut cfg.after).range(0..=OnionCfg::MAX_DEPTH))
                    .card("ghosts ahead (0–5)");
            });
            ui.horizontal_wrapped(|ui| {
                for m in OnionMode::ALL {
                    if ui
                        .selectable_label(cfg.mode == m, m.label())
                        .card(match m {
                            OnionMode::Drawings => "ghost the previous and next distinct drawings",
                            OnionMode::Frames => "ghost what the column shows 1, 2, … frames away",
                            OnionMode::Keys => "ghost the previous and next genga (key) drawings",
                        })
                        .clicked()
                    {
                        cfg.mode = m;
                    }
                }
            });
            // Keys mode ghosts only marked keys: say so when there are none
            // (a silent empty onion reads as a bug).
            if cfg.mode == OnionMode::Keys
                && !state.cut().drawing_defs.values().any(|d| d.kind == DrawingKind::Key)
            {
                ui.label(
                    egui::RichText::new("no drawings marked genga (key) yet — mark one from its cell's menu")
                        .small()
                        .color(plate::LEGEND),
                );
            }
            plate::latch(ui, &mut cfg.other_columns, "other columns")
                .card("ghost every other column that renders too");
        });
        if cfg != self.onion {
            self.onion = cfg.sane();
            self.onion_request = Some(self.onion);
        }

        // GM6: THE LIGHT TABLE — pins on every frame, shift & trace.
        egui::CollapsingHeader::new("light table").show(ui, |ui| {
            if let Some(here) = state.current_drawing() {
                let pinned = state.view.light.is_pinned(here);
                let words = if pinned { "unpin this drawing" } else { "pin this drawing" };
                if crate::menus::hover_tip(ui.button(words), Action::PinToLightTable, "").clicked() {
                    state.view.light.toggle(state.view.active_column, here);
                }
            }
            let shown = state.view.light.shown(state.cut());
            let target = state.view.light.target();
            for (pid, _) in &shown {
                let name = state.cut().drawing(*pid).map(|d| d.name.clone()).unwrap_or_default();
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(target == Some(*pid), format!("pin {name}"))
                        .card("shift & trace moves this pin")
                        .clicked()
                    {
                        state.view.light.selected = Some(*pid);
                    }
                    if ui.small_button("unpin").card("stop ghosting this drawing").clicked() {
                        state.view.light.unpin(*pid);
                    }
                });
            }
            let mut lt = self.light_table;
            value_line(ui, "pin opacity", format!("{:.0}%", lt.opacity * 100.0));
            ui.add(egui::Slider::new(&mut lt.opacity, 0.1..=1.0).show_value(false))
                .card("every pin shows at this opacity");
            if lt != self.light_table {
                self.light_table = lt;
                self.light_table_request = Some(lt);
            }
            let mut st = state.view.light.shift_trace;
            crate::menus::hover_tip(
                plate::latch(ui, &mut st, "shift & trace"),
                Action::ShiftTrace,
                "while on, a drag on the canvas moves the selected pin (Shift+drag turns it); the pen rests",
            );
            state.view.light.shift_trace = st;
            if !state.view.light.offsets.is_empty()
                && crate::menus::hover_tip(ui.button("reset shift"), Action::ResetShift, "").clicked()
            {
                state.view.light.reset();
            }
        });

        // GM6: genga to douga — this frame's drawing's kind, its spacing
        // chart toward the next key, and the inbetweens it makes.
        if let Some(here) = state.current_drawing() {
            let busy = self.stroke_active();
            egui::CollapsingHeader::new("genga · douga").show(ui, |ui| {
                let kind = state.cut().drawing_kind(here);
                value_line(ui, "kind", kind.label().to_string());
                ui.horizontal_wrapped(|ui| {
                    for (k, verb) in [
                        (DrawingKind::Key, Action::KindKey),
                        (DrawingKind::Breakdown, Action::KindBreakdown),
                        (DrawingKind::Inbetween, Action::KindInbetween),
                        (DrawingKind::Unset, Action::KindUnset),
                    ] {
                        let label = ui.selectable_label(kind == k, k.label());
                        if crate::menus::hover_tip(label, verb, "").clicked() && !busy {
                            state.set_drawing_kind(here, k);
                        }
                    }
                });
                if kind == DrawingKind::Key {
                    let chart = state.cut().drawing_def(here).chart;
                    value_line(
                        ui,
                        "chart",
                        chart.as_ref().map_or("none".to_string(), |c| format!("{} marks", c.marks.len())),
                    );
                    ui.horizontal_wrapped(|ui| {
                        for (name, marks, tip) in crate::doc::CHART_PRESETS {
                            if ui.small_button(*name).card(*tip).clicked() && !busy {
                                state.set_chart(here, marks);
                            }
                        }
                        if chart.is_some()
                            && ui.small_button("clear").card("remove the chart").clicked()
                            && !busy
                        {
                            state.set_chart(here, &[]);
                        }
                    });
                    // The chart editor: any count of marks, even or closing
                    // up toward one key; then drag a mark on the chart.
                    value_line(ui, "marks", self.chart_count.to_string());
                    plate::field(ui, egui::DragValue::new(&mut self.chart_count).range(1..=crate::doc::CHART_MAX_MARKS))
                        .card("how many inbetweens the chart asks for (1–15)");
                    ui.horizontal_wrapped(|ui| {
                        for spacing in crate::doc::ChartSpacing::ALL {
                            if ui.small_button(spacing.label()).card(spacing.help()).clicked() && !busy {
                                state.set_chart(here, &crate::doc::chart_marks(spacing, self.chart_count));
                            }
                        }
                    });
                    if let Some(c) = &chart {
                        self.chart_editor(ui, state, here, &c.marks, busy);
                    }
                    if chart.is_some()
                        && crate::menus::hover_tip(ui.button("make inbetweens"), Action::MakeInbetweens, "")
                            .clicked()
                        && !busy
                    {
                        state.make_inbetweens();
                    }
                }
                if crate::menus::hover_tip(ui.button("duplicate cel"), Action::DuplicateCel, "").clicked()
                    && !busy
                {
                    state.duplicate_cel();
                }
            });
        }
        ui.add_space(10.0);
        plate::legend(ui, "paper");
        ui.add_space(2.0);
        let mut f = self.show_field;
        plate::latch(ui, &mut f, "field")
            .card("the 12-field chart (field n is n/12 of the frame) + centre cross");
        self.show_field = f;
        let mut sa = self.show_safe;
        plate::latch(ui, &mut sa, "safe")
            .card("action safe (93%) and title safe (90%) + centre cross");
        self.show_safe = sa;
        let mut pg = self.show_peg;
        plate::latch(ui, &mut pg, "peg").card("the registration peg bar");
        self.show_peg = pg;
        ui.add_space(10.0);
        plate::legend(ui, "view");
        ui.add_space(2.0);
        let fit = plate::icon_button(ui, Icon::Fit, "fit", "");
        if crate::menus::hover_tip(fit, Action::ZoomFit, "").clicked() {
            self.fit_view();
        }
        // GM4: absolute — 100% is one paper px to one screen point.
        value_line(ui, "zoom", format!("{:3.0}%", self.abs_zoom() * 100.0));
        // GM4: the turn and the mirror stand by the zoom — the angle, the
        // upright detent (armed when upright; a click turns it back) and
        // the mirror latch.
        value_line(ui, "turn", format!("{:4.0}°", self.rotation_deg));
        let upright = plate::detent(ui, self.rotation_deg == 0.0, "upright");
        if crate::menus::hover_tip(upright, Action::ResetRotation, "").clicked() {
            self.set_view_rotation(0.0);
        }
        let mut mirrored = self.mirror;
        let flip = plate::latch(ui, &mut mirrored, "mirror");
        crate::menus::hover_tip(flip, Action::MirrorView, "");
        if mirrored != self.mirror {
            self.toggle_mirror();
        }
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
            .card("input device — click for pen settings")
            .clicked()
        {
            self.request_pen_settings = true;
        }
    }

    /// The Brushes float: the old brush rail, the pencil box and the deck's
    /// pen aids. Its essentials stand first, the rest behind `More`. Every
    /// slider is a bare track; labels ride ABOVE the tracks as engraved
    /// value lines.
    pub(crate) fn brush_rail_ui(
        &mut self,
        ui: &mut egui::Ui,
        presets: &[BrushPreset],
        state: &mut AppState,
    ) {
        let title = if self.erasing {
            "eraser".to_string()
        } else {
            self.armed_preset.clone().unwrap_or_else(|| "brush".into())
        };
        // Never the bare word: the rail's keys are spoken `Brush` and `Eraser`.
        plate::legend(ui, &format!("{}: {title}", crate::vocab::DRAWING_WITH));
        ui.add_space(8.0);
        // THE PENCIL BOX (genga room charter): the three named pencils.
        // Arming a pencil arms the brush; the Tally ring breaks when the
        // live brush drifts off the preset.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // Three across what the float leaves: its handle takes a gutter.
            let wide = (ui.available_width() - 2.0 * 6.0) / 3.0;
            for (i, p) in presets.iter().enumerate() {
                if matches!(p.name.as_str(), "atari" | "genga" | "shusei") {
                    self.pencil_slot(ui, p, i + 1, wide, state);
                }
            }
        });
        self.brush_library_ui(ui, presets);
        ui.add_space(8.0);
        ui.spacing_mut().slider_width = ui.available_width();
        // The route's size, as the rail's key has it: this float opens on
        // the vector brush too.
        let (mut px, (lo, hi)) = (self.active_px(), self.px_range());
        value_line(ui, "size", format!("{px:.1} px"));
        if ui.add(egui::Slider::new(&mut px, lo..=hi).show_value(false)).changed() {
            self.set_brush_px(px, state);
        }
        ui.add_space(4.0);
        value_line(ui, "opacity", format!("{:.2}", self.brush_opacity));
        ui.add(egui::Slider::new(&mut self.brush_opacity, 0.05..=1.0).show_value(false));
        ui.add_space(4.0);
        self.steady_row(ui);
        self.keep_inside_row(ui);
        ui.add_space(4.0);
        egui::CollapsingHeader::new("More").id_salt("brushes_more").show(ui, |ui| {
            // The fold stands indented, and `set_width` above it is only a
            // floor: a slider of the body's width would widen the float.
            ui.spacing_mut().slider_width = ui.available_width();
            self.brush_more_ui(ui);
            self.edge_rows(ui);
            ui.add_space(8.0);
            self.brush_imports_ui(ui);
        });
    }

    /// The Brushes float's `More`: flow and the pen's dynamics.
    fn brush_more_ui(&mut self, ui: &mut egui::Ui) {
        value_line(ui, "flow", format!("{:.2}", self.brush_flow));
        ui.add(egui::Slider::new(&mut self.brush_flow, 0.05..=1.0).show_value(false));
        ui.add_space(10.0);
        plate::legend(ui, "pressure");
        ui.add_space(2.0);
        let mut ps = self.dyn_size;
        plate::latch(ui, &mut ps, "p·size").card("pressure drives size");
        self.dyn_size = ps;
        let mut po = self.dyn_opacity;
        plate::latch(ui, &mut po, "p·opac").card("pressure drives opacity");
        self.dyn_opacity = po;
        ui.add_space(4.0);
        value_line(ui, "min size", format!("{:.2}", self.min_size));
        ui.add(egui::Slider::new(&mut self.min_size, 0.0..=1.0).show_value(false))
            .card("size at zero pressure, as a fraction of the brush");
        ui.add_space(10.0);
        plate::legend(ui, "tilt");
        ui.add_space(2.0);
        let mut ts = self.tilt_size;
        plate::latch(ui, &mut ts, "t·size").card("tilting broadens the stroke");
        self.tilt_size = ts;
        let mut to = self.tilt_opacity;
        plate::latch(ui, &mut to, "t·opac").card("tilting lightens the stroke");
        self.tilt_opacity = to;
        let mut tsh = self.tilt_shape;
        plate::latch(ui, &mut tsh, "t·shape")
            .card("the stamp flattens and turns with the pen's lean");
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
    }

    /// THE LIBRARY (PSD-brush-library): imported Krita presets, browsed and
    /// ARMED here; edited only in Settings → Brushes.
    fn brush_library_ui(&mut self, ui: &mut egui::Ui, presets: &[BrushPreset]) {
        ui.add_space(10.0);
        value_line(ui, "library", format!("{}", presets.len()));
        ui.add_space(2.0);
        if presets.len() > 6 {
            plate::text_field(
                ui,
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
        let scroll = egui::ScrollArea::vertical()
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
                            painter.rect_filled(rect, plate::R_CONTROL, plate::WELL);
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
                                        plate::UNTINTED,
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
                                    plate::R_CONTROL,
                                    egui::Stroke::new(2.0, plate::TALLY),
                                    egui::StrokeKind::Inside,
                                );
                            } else if resp.hovered() {
                                painter.rect_stroke(
                                    rect,
                                    plate::R_CONTROL,
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
                            if resp.card(hover).clicked() {
                                arm = Some((*p).clone());
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
            });
        plate::note_viewport(ui, scroll.inner_rect, [false, true]);
        if let Some(p) = arm {
            self.apply_preset(&p);
        }
    }

    /// The float's way out to more brushes: imports and the Brush maker.
    fn brush_imports_ui(&mut self, ui: &mut egui::Ui) {
        if ui
            .button("import brushes…")
            .card(
                "Krita .kpp presets and community .bundle packs — they \
                 paint with OUR brush at the preset's size and strength",
            )
            .clicked()
        {
            self.request_brush_import = true;
        }
        if !crate::kpp::installed_krita_paths().is_empty()
            && ui
                .add(egui::Button::new("import installed Krita's brushes").wrap())
                .card(
                    "everything the Krita on this machine ships — its \
                     bundles, its presets, and your %APPDATA%/krita \
                     brushes — mapped onto OUR brush honestly",
                )
                .clicked()
        {
            self.request_krita_scan = true;
        }
        if ui.button("Make a brush").card("opens the brush maker").clicked() {
            self.request_brush_maker = true;
        }
    }

    /// The Color float: the deck's picker and pencil case, over the dish.
    pub(crate) fn color_ui(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        ui.horizontal(|ui| {
            // Custom colour picker (any RGB); the swatches beside it are presets.
            let mut rgb = [
                self.brush_color[0],
                self.brush_color[1],
                self.brush_color[2],
            ];
            if ui
                .color_edit_button_srgb(&mut rgb)
                .card("brush colour")
                .changed()
            {
                self.brush_color = [rgb[0], rgb[1], rgb[2], self.brush_color[3]];
            }
            for (i, c) in SWATCHES.iter().enumerate() {
                let selected = self.brush_color == *c;
                let hover = ["ink", "ao — construction", "aka — correction", "white"][i];
                if plate::swatch(
                    ui,
                    plate::chip([c[0], c[1], c[2], c[3]]),
                    selected,
                    hover,
                )
                .card(hover)
                .clicked()
                {
                    self.brush_color = *c;
                }
            }
        });
        ui.add_space(8.0);
        self.dish_ui(ui, state);
    }

    /// `Keep inside strokes` (L): ink can only recolor pixels the active
    /// layer already has coverage on — never paint outside its silhouette.
    /// Combines with brush or eraser, so Brushes and Tool options share it.
    pub(crate) fn keep_inside_row(&mut self, ui: &mut egui::Ui) {
        let mut lock = self.alpha_lock;
        crate::menus::hover_tip(
            plate::tool_latch(
                ui,
                &mut lock,
                Icon::Lock,
                crate::vocab::KEEP_INSIDE_STROKES_SHORT,
                crate::vocab::KEEP_INSIDE_STROKES,
            ),
            Action::ToggleAlphaLock,
            "ink only recolors pixels this layer already has \
            coverage on; the silhouette can't grow",
        );
        self.alpha_lock = lock;
    }

    /// The Brush card's line: where a stroke lands, and with what. A fill
    /// names its role too, the one job of the retired ARMED line.
    pub(crate) fn drawing_with(&self, state: &AppState) -> String {
        let pencil = match &self.armed_preset {
            _ if self.erasing => "eraser",
            Some(name) => name.as_str(),
            None => "brush",
        };
        let mut line = format!(
            "{}: {} · {pencil}",
            crate::vocab::DRAWING_WITH,
            state.active_layer_name()
        );
        if matches!(self.tool, CanvasTool::Fill | CanvasTool::LassoFill) {
            let mut roles = state.palettes.characters.iter().flat_map(|ch| &ch.roles);
            if let Some(role) = roles.find(|r| r.color[..3] == self.brush_color[..3]) {
                line.push_str(&format!(" · {}", role.name));
            }
        }
        line
    }

    /// The Tool options float: the armed tool's rows, the old deck's OPTIONS
    /// row laid out top down.
    pub(crate) fn tool_options_ui(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        use crate::menus::hover_tip;
        match self.tool {
            CanvasTool::Select => {
                plate::legend(ui, "selection shape");
                ui.horizontal(|ui| {
                    if plate::tool_button(
                        ui,
                        self.sel_shape == SelShape::Lasso,
                        Icon::Lasso,
                        "lasso",
                        "lasso selection",
                    )
                    .card("lasso selection")
                    .clicked()
                    {
                        self.sel_shape = SelShape::Lasso;
                    }
                    if plate::tool_button(
                        ui,
                        self.sel_shape == SelShape::Rect,
                        Icon::Marquee,
                        "rect",
                        "rect selection",
                    )
                    .card("rectangle selection")
                    .clicked()
                    {
                        self.sel_shape = SelShape::Rect;
                    }
                });
            }
            CanvasTool::Fill | CanvasTool::LassoFill => {
                // The `Fill` chooser: the rail says `Fill`, so this row speaks
                // its own name (1.5 rule 9). Lasso fill loops a region and
                // fills it feathered.
                ui.horizontal(|ui| {
                    let fill_on = self.tool == CanvasTool::Fill;
                    if hover_tip(
                        plate::tool_button(ui, fill_on, Icon::Fill, "Fill", "Fill a region"),
                        Action::FillTool,
                        "click a region; line art bounds it. gap closes line \
                        breaks; under tucks the flat beneath the lines; cel/layer \
                        picks the boundary reference. Shift+click refills a painted \
                        flat (nuri-naoshi); Ctrl+Shift+click replaces its colour on \
                        the whole layer",
                    )
                    .clicked()
                    {
                        self.set_tool(CanvasTool::Fill, state);
                    }
                    let lf_on = self.tool == CanvasTool::LassoFill;
                    let draw = crate::vocab::DRAW_TO_FILL;
                    if hover_tip(
                        plate::tool_button(ui, lf_on, Icon::Lasso, draw, draw),
                        Action::LassoFillTool,
                        "softness feathers the edge inward, grow shifts the edge; \
                        one loop = one undo step",
                    )
                    .clicked()
                    {
                        self.set_tool(CanvasTool::LassoFill, state);
                    }
                });
                ui.add_space(4.0);
                if self.tool == CanvasTool::Fill {
                    // Engraved label BEFORE, 64pt value (shiage:
                    // leak-chasing nudges these several times a cel).
                    ui.horizontal(|ui| {
                        plate::legend(ui, "gap");
                        let mut gap = self.fill_gap;
                        plate::field(ui, egui::DragValue::new(&mut gap).range(0..=8));
                        self.fill_gap = gap;
                        plate::legend(ui, "under");
                        let mut grow = self.fill_grow;
                        plate::field(ui, egui::DragValue::new(&mut grow).range(0..=4));
                        self.fill_grow = grow;
                    });
                    ui.horizontal(|ui| {
                        if plate::detent(ui, self.fill_ref_cel, "cel")
                            .card(
                                "boundaries come from the whole cel (all visible layers)",
                            )
                            .clicked()
                        {
                            self.fill_ref_cel = true;
                        }
                        if plate::detent(ui, !self.fill_ref_cel, "layer")
                            .card("boundaries come from the active layer only")
                            .clicked()
                        {
                            self.fill_ref_cel = false;
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        plate::legend(ui, "softness");
                        let mut soft = self.lasso_soft;
                        plate::field(ui, egui::DragValue::new(&mut soft).range(0.0..=64.0));
                        self.lasso_soft = soft;
                        plate::legend(ui, "grow");
                        let mut grow = self.lasso_grow;
                        plate::field(ui, egui::DragValue::new(&mut grow).range(-16.0..=16.0));
                        self.lasso_grow = grow;
                    });
                }
            }
            // GM18: the panel tool's gestures, said where its options would
            // sit (the gutters are the Manga pane's).
            CanvasTool::Panel => {
                plate::legend(ui, "panel (koma)");
                ui.label(
                    egui::RichText::new(
                        "drag on paper: a panel · across a panel: a split · \
                         from a VP or the horizon: moved",
                    )
                    .small()
                    .color(plate::legend_dim()),
                );
            }
            CanvasTool::Paint => {
                plate::legend(ui, if self.erasing { "eraser options" } else { "brush options" });
                self.keep_inside_row(ui);
            }
        }
    }

    /// `Steady stroke`: an essential of the Brushes float. GM4: the pen aids
    /// sit with the brush.
    fn steady_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            plate::legend(ui, crate::vocab::STEADY_STROKE);
            let mut stab = self.stabilizer;
            let field = plate::field(
                ui,
                egui::DragValue::new(&mut stab).range(0..=crate::stabilizer::MAX_LEVEL),
            );
            plate::speak(&field, egui::WidgetType::DragValue, None, crate::vocab::STEADY_STROKE);
            field.card(
                "tebure hosei (stabilization): 0 = raw pen; \
                 higher pulls the ink on a longer string",
            );
            if stab != self.stabilizer {
                self.stabilizer = stab;
                self.stabilizer_request = Some(stab);
            }
        });
    }

    /// `Edge softness` and `Pixel edges`, in the Brushes float's `More`.
    fn edge_rows(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            plate::legend(ui, crate::vocab::EDGE_SOFTNESS);
            let mut hard = self.brush_hardness;
            // Shown as a percent (the stored edge stays 0–1).
            let field = plate::field(
                ui,
                egui::DragValue::new(&mut hard)
                    .range(0.0..=1.0)
                    .speed(0.01)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                    .custom_parser(|s| {
                        let s = s.trim().trim_end_matches('%').trim();
                        s.parse::<f64>().ok().map(|v| v / 100.0)
                    }),
            );
            plate::speak(&field, egui::WidgetType::DragValue, None, crate::vocab::EDGE_SOFTNESS);
            field.card("the dab's edge: toward 100% hard, toward 0% soft");
            self.brush_hardness = hard;
        });
        if plate::detent(ui, self.brush_aliased, crate::vocab::PIXEL_EDGES)
            .card(
                "hard 0/255 edges, no smoothing (pixel-exact fills): \
                 flow, pressure opacity and paper grain are off",
            )
            .clicked()
        {
            self.brush_aliased = !self.brush_aliased;
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
    ) {
        self.color_law = state.engine.project.color_law;
        self.pen_curve = pen.pressure_curve.clone();
        self.stabilizer = pen.stabilizer.min(crate::stabilizer::MAX_LEVEL);
        // GM18: never mid-stroke — a live stroke keeps the hold it began with.
        self.snap_rulers = state.manga.snap_rulers(state.cut());
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

        // (Nothing lives fixed in the canvas: the tools ride over it on the
        // rail, rail.rs. The paper gets the whole pane.)
        let mut paint = paint;
        if paint.is_none() {
            self.raster = false;
        }
        // GM20: the layer rect follows the cut and its paper, GPU or not.
        self.sync_layer_rect(state);

        // R-FLIP: momentary, polled (not an Action — actions fire on
        // press; the flip lives exactly as long as the key is down). Never
        // while typing, playing, mid-stroke, or in composite review. The
        // rail's Flip key is the same hold for a hand on the pen.
        let r_down = ui.input(|i| i.key_down(egui::Key::R)) && !ui.ctx().egui_wants_keyboard_input();
        self.flip_held = (r_down || self.rail_flip)
            && !self.stroke_active()
            && !state.view.playing
            && !self.composite_view;

        // ---- Canvas area --------------------------------------------------
        let rect = ui.available_rect_before_wrap();
        self.dbg_rect = rect;
        self.dbg_layer = ui.layer_id();
        self.dbg_preview = false;
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        self.clipboard_events(ui, rect, state);

        // Paper size = the project's chosen resolution (set at creation).
        let paper_w = state.engine.project.width as f32;
        let paper_h = state.engine.project.height as f32;

        // View transform (GM4 ViewXform): fit paper into rect, then the
        // user's zoom / pan / turn / mirror on top. AU4: with the strip closed
        // the pane stands 56 taller than the strip will leave it, and that can
        // run a page under the rail: Fit stops at the rail's reach, and never
        // under the page's strip-open size (spec 13, the AU4 notes).
        let fit_in = |tall: f32| (rect.width() / paper_w).min(tall / paper_h) * 0.94;
        let strip_open = fit_in(crate::rail::reference(rect.height(), self.strip_closed));
        let clear = (rect.width() - 2.0 * crate::floats::LEFT_X) / paper_w;
        let fit = clear.clamp(strip_open, fit_in(rect.height())).max(0.01);
        self.last_fit = fit;
        self.xf.center = rect.center();
        self.xf.paper = vec2(paper_w, paper_h);
        self.sync_xf();

        // GM4 SPACE HAND: held Space (not typing) takes the pointer — a
        // drag pans, Shift+drag turns (Ctrl snaps to 15°). A hold that
        // began mid-stroke never engages; releasing Space restores the
        // tool. Works during playback too: it only moves the view.
        let space = ui.input(|i| i.key_down(egui::Key::Space))
            && !ui.ctx().egui_wants_keyboard_input();
        if space && !self.space_was_down {
            // A pen-busy hold never engages; a lifted transform resting
            // between drags does (the hand moves only the view).
            self.hand_engaged = !self.pen_busy();
            self.hand_turn = None;
            self.hand_view = self.view_now();
            crate::shortcuts::clear_hand_drag(ui.ctx());
        }
        if !space {
            self.hand_engaged = false;
        }
        if !space && !self.hand_held {
            self.hand_turn = None;
        }
        self.space_was_down = space;
        // AU4: the rail's Hand is the same hold, kept for a hand on the pen.
        let space_hand = space && self.hand_engaged;
        let hand = space_hand || self.hand_held;
        if hand {
            let m = ui.input(|i| i.modifiers);
            let pointer = response.interact_pointer_pos();
            if response.dragged_by(egui::PointerButton::Primary) {
                // Only a Space hold has a tap to swallow.
                if space_hand {
                    crate::shortcuts::note_hand_drag(ui.ctx());
                }
                if m.shift
                    && let Some(p) = pointer
                {
                    let c = rect.center();
                    let ang = (p.y - c.y).atan2(p.x - c.x).to_degrees();
                    let (start, start_ang, start_pan) =
                        *self.hand_turn.get_or_insert((self.rotation_deg, ang, self.pan));
                    let mut target = start + (ang - start_ang);
                    if m.command {
                        target = (target / ROTATE_STEP_DEG).round() * ROTATE_STEP_DEG;
                    }
                    self.pan = start_pan;
                    self.rotation_deg = start;
                    self.set_view_rotation(target);
                } else {
                    self.hand_turn = None;
                    self.pan += response.drag_delta();
                }
                self.sync_xf();
            }
            if response.drag_stopped() {
                self.hand_turn = None;
            }
            // The rail's Hand outlives the pointer's stay: its cursor is the
            // canvas's, not the rail's or the panels'.
            if response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if space_hand || response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
        }

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
                    let before = self.xf.screen_to_paper(mouse);
                    // The relative 0.2–10× range, widened to the keyed
                    // ladder's ends so a wheel never snaps a keyed zoom back.
                    let lo = (0.2f32).min(ZOOM_LADDER[0] / fit);
                    let hi = (10.0f32).max(ZOOM_LADDER[ZOOM_LADDER.len() - 1] / fit);
                    self.zoom = (self.zoom * (scroll * 0.0015).exp()).clamp(lo, hi);
                    self.sync_xf();
                    let after = self.xf.paper_to_screen(before);
                    self.pan += mouse - after;
                    self.sync_xf();
                }
            }
            // Middle-drag pans.
            if response.dragged_by(egui::PointerButton::Middle) {
                self.pan += response.drag_delta();
                self.sync_xf();
            }
        }
        // GM4: whatever moved the view during the Space hold (the hand, a
        // wheel zoom, a middle drag, a keyed turn) makes it a drag.
        if space && self.view_now() != self.hand_view {
            crate::shortcuts::note_hand_drag(ui.ctx());
        }

        let xf = self.xf;
        let scale = xf.zoom;
        let to_screen = |p: Pos2| -> Pos2 { xf.paper_to_screen(p) };
        let to_paper = |s: Pos2| -> Pos2 { xf.screen_to_paper(s) };

        // Session presence: our pointer, in paper space, when over the paper
        // (GM20: the paper beyond the frame too).
        let pa = state.paper_area();
        let (pa0, pa1) = (
            pos2(pa.x as f32, pa.y as f32),
            pos2(pa.x as f32 + pa.w as f32, pa.y as f32 + pa.h as f32),
        );
        self.presence_pos = response
            .hover_pos()
            .map(|hp| to_paper(hp))
            .filter(|pp| pp.x >= pa0.x && pp.y >= pa0.y && pp.x <= pa1.x && pp.y <= pa1.y)
            .map(|pp| [pp.x, pp.y]);

        // ---- Paper --------------------------------------------------------
        // Upright (unturned) the paper is the old axis-aligned rect; turned,
        // it is the corners' polygon and `paper_rect` its screen box (text
        // labels anchor to the box). GM20: the sheet is the paper the tools
        // paint on (`pa`) — the frame when paper = frame, as before; a wider
        // sheet is laid down whole, the frame outlined on it, and paper the
        // paint layer does not reach (past a capped window) veiled.
        let upright = self.rotation_deg == 0.0;
        let paper_rect = xf.paper_bounds();
        // The pasteboard: the desk, whatever the pane's island is.
        painter.rect_filled(rect, 0, plate::DESK);
        let paper_fill = if self.miss_check {
            crate::plate::STEEL
        } else {
            crate::plate::PAPER
        };
        let edge = egui::Stroke::new(1.0, crate::plate::INK2);
        if pa == IRect::frame(paper_w as u32, paper_h as u32) {
            if upright {
                painter.add(plate::paper_shadow(paper_rect));
                painter.rect_filled(paper_rect, 0, paper_fill);
                painter.rect_stroke(paper_rect, 0, edge, egui::StrokeKind::Outside);
            } else {
                painter.add(egui::Shape::convex_polygon(
                    xf.paper_corners().to_vec(),
                    paper_fill,
                    edge,
                ));
            }
        } else {
            let quad = |r: IRect| -> Vec<Pos2> {
                let (x0, y0) = (r.x as f32, r.y as f32);
                let (x1, y1) = (x0 + r.w as f32, y0 + r.h as f32);
                [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                    .iter()
                    .map(|&(x, y)| to_screen(pos2(x, y)))
                    .collect()
            };
            let veil = if self.miss_check {
                crate::plate::STEEL
            } else {
                crate::plate::paper_veil()
            };
            let strips = unpainted_strips(pa, &self.layer_rect);
            let frame_edge = egui::Stroke::new(1.0, crate::plate::INK2);
            if upright {
                let sheet = Rect::from_points(&quad(pa));
                painter.add(plate::paper_shadow(sheet));
                painter.rect_filled(sheet, 0, paper_fill);
                painter.rect_stroke(sheet, 0, edge, egui::StrokeKind::Outside);
                for s in strips {
                    painter.rect_filled(Rect::from_points(&quad(s)), 0, veil);
                }
                painter.rect_stroke(paper_rect, 0, frame_edge, egui::StrokeKind::Inside);
            } else {
                painter.add(egui::Shape::convex_polygon(quad(pa), paper_fill, edge));
                for s in strips {
                    painter.add(egui::Shape::convex_polygon(quad(s), veil, egui::Stroke::NONE));
                }
                painter.add(egui::Shape::closed_line(xf.paper_corners().to_vec(), frame_edge));
            }
        }
        // Paper furniture (Phase 4): field / safe / peg guides in the blue
        // pencil — the scaffold ink, printed under the drawing like a real
        // layout sheet.
        let ao = plate::alpha(plate::PENCIL_AO, 110);
        // GM10: the camera furniture is paper-space geometry mapped through
        // the view, upright or turned alike: the extended paper's outline,
        // the field chart with its numbers, the safe frames, the centre
        // cross, and the output camera's frame over them.
        let furniture = Furniture::of(
            state.cut(),
            state.view.frame,
            (paper_w as u32, paper_h as u32),
            self.show_field,
            self.show_safe,
        );
        {
            let g = egui::Stroke::new(1.0, ao);
            let outline = |pts: [(f32, f32); 4], stroke: egui::Stroke| -> egui::Shape {
                let pts = pts.iter().map(|&(x, y)| to_screen(pos2(x, y))).collect();
                egui::Shape::closed_line(pts, stroke)
            };
            if let Some(p) = furniture.paper {
                let (x, y) = (p.x as f32, p.y as f32);
                let (w, h) = (p.w as f32, p.h as f32);
                let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
                painter.add(outline(corners, egui::Stroke::new(1.0, crate::plate::INK3)));
            }
            // The guides sit on the camera's frame (the frame itself with
            // no camera), so they are drawn through `quad`.
            let on_paper = |p: (f32, f32)| {
                let (x, y) = furniture.on_paper(p);
                to_screen(pos2(x, y))
            };
            for (n, r) in furniture.fields.iter().enumerate() {
                painter.add(outline(furniture.quad(r), g));
                painter.text(
                    on_paper((r.x, r.y)) + vec2(2.0, 1.0),
                    egui::Align2::LEFT_TOP,
                    format!("{}", n + 1),
                    egui::FontId::monospace(9.0),
                    plate::AO,
                );
            }
            if let Some(s) = furniture.safe {
                painter.add(outline(furniture.quad(&s.action), g));
                painter.add(outline(furniture.quad(&s.title), g));
            }
            if self.show_field || self.show_safe {
                // 10 screen px arms, in the camera frame's units (a T.U.
                // camera's frame is smaller on the paper).
                let (cx, cy) = (paper_w * 0.5, paper_h * 0.5);
                let arm = 10.0 / scale.max(1e-6) * furniture.view.map_or(1.0, |m| m.zoom);
                painter.line_segment([on_paper((cx - arm, cy)), on_paper((cx + arm, cy))], g);
                painter.line_segment([on_paper((cx, cy - arm)), on_paper((cx, cy + arm))], g);
            }
            if let Some(c) = furniture.camera {
                // The scaffold ink at full strength and heavier than the
                // guides (red is kept for refusals: the plate law).
                painter.add(outline(c, egui::Stroke::new(2.0, crate::plate::PENCIL_AO)));
            }
        }
        self.furniture = furniture;
        if !upright {
            // GM4: turned, the guides are paper-space shapes mapped through
            // the view (the peg bar's screen sizes become paper sizes).
            let g = egui::Stroke::new(1.0, ao);
            let paper_box = |c: Pos2, half: egui::Vec2| -> egui::Shape {
                let pts = [
                    pos2(c.x - half.x, c.y - half.y),
                    pos2(c.x + half.x, c.y - half.y),
                    pos2(c.x + half.x, c.y + half.y),
                    pos2(c.x - half.x, c.y + half.y),
                ];
                egui::Shape::closed_line(pts.iter().map(|p| to_screen(*p)).collect(), g)
            };
            if self.show_peg {
                let u = 1.0 / scale.max(1e-6);
                let pc = pos2(paper_w * 0.5, paper_h * 0.95);
                painter.circle_stroke(to_screen(pc), 5.0, g);
                for dx in [-70.0f32, 70.0] {
                    painter.add(paper_box(pc + vec2(dx * u, 0.0), vec2(10.0, 4.5) * u));
                }
            }
        }
        if upright {
            let g = egui::Stroke::new(1.0, ao);
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
        self.retire_line_anchor(state);
        // GM6: shift & trace has the pointer while its latch is on and a pin
        // is there to move — the pen rests, and the chip below says so.
        let tracing = !state.view.playing
            && !hand
            && state.view.light.shift_trace
            && state.view.light.target().is_some()
            && !self.pen_busy();
        if hand && !state.view.playing {
            // GM4: the Space hand has the pointer — no tool input, no tap.
        } else if tracing {
            self.shift_trace_input(ui, &response, &to_paper, pos2(paper_w * 0.5, paper_h * 0.5), state);
            // The move hand, not the brush ring: a drag shifts the pin.
            if response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
            }
        } else if !state.view.playing {
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
                CanvasTool::Panel => {
                    self.koma_input(ui, &response, &to_paper, state);
                }
            }
        } else {
            self.current.clear();
            self.touch_active = false;
            // GM18: a panel-tool drag under playback lands nowhere.
            self.koma = None;
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
        // GM9: an FX subtree is still rendering on the worker.
        let mut composite_pending = false;
        if self.composite_view {
            use crate::viewer::GraphView as GV;
            match graph {
                GV::Ready(id) => composite_tex = Some(id),
                GV::Rendering(id) => {
                    composite_tex = Some(id);
                    composite_pending = true;
                }
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
            self.ensure_layer(state, p);
            self.run_remote_wet(p);
            self.run_replays(p, state);
        }
        if composite_tex.is_none()
            && self.raster
            && let Some(p) = paint.as_deref_mut()
        {
            self.ensure_layer(state, p);

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
                    // GM20: the readback covers the layer rect; art outside
                    // a capped or shrunk layer stays as the engine holds it
                    // — on the LATCHED layer the commit lands on.
                    let before = state.commit_target_tiles(self.stroke_layer_slot);
                    let tiles = paint::merge_outside(&before, p.read_tiles(), &p.rect());
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
                        // AU1: the pen finished one here; the host inks it.
                        state.strokes_done = state.strokes_done.wrapping_add(1);
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
            let pins = state.view.light.shown(state.cut());
            if state.view.onion || !pins.is_empty() || self.flip_held {
                // Computed once per frame, not per ghost: the active
                // layer's role name is what "line-only ghost" locks onto.
                // The FLIP shows the whole previous cel — no filter.
                let filter = (!self.flip_held && state.view.onion_layer_only)
                    .then(|| (state.active_layer_name(), state.view.active_role));
                let f = filter.as_ref().map(|(n, r)| (n.as_str(), *r));
                if self.flip_held {
                    // The FLIP borrows slot 0 at full identity: the whole
                    // previous cel, not a ghost (unchanged by GM6).
                    let prev = state.onion_neighbors()[0];
                    match prev.and_then(|id| state.drawing_composite(id, None, false)) {
                        Some((slices, hash)) => p.set_onion(0, Some(&slices), hash),
                        None => p.set_onion(0, None, 0),
                    }
                } else if state.view.onion {
                    // GM6: slot 0 = every ghost behind, slot 1 = every ghost
                    // ahead, each side composited with its weights into its
                    // one texture (farthest first, the nearest on top at
                    // full; the slot's tint carries the nearest's opacity).
                    let set = onion_set(
                        state.cut(),
                        state.view.active_column,
                        state.view.frame,
                        &self.onion,
                    );
                    let plan = onion_slot_plan(&set, &pins);
                    for slot in [0, 1] {
                        let mut slices = Vec::new();
                        let mut bytes = Vec::new();
                        for &(id, k) in &plan[slot] {
                            // PSD-cel-layers: ghosts honour each layer's onion flag.
                            if let Some((s, h)) = state.drawing_composite(id, f, true) {
                                slices.extend(s.into_iter().map(|(t, o)| (t, o * k)));
                                bytes.extend_from_slice(&h.to_le_bytes());
                                bytes.extend_from_slice(&k.to_bits().to_le_bytes());
                            }
                        }
                        if slices.is_empty() {
                            p.set_onion(slot, None, 0);
                        } else {
                            p.set_onion(slot, Some(&slices), anim_core::value::fnv1a(&bytes));
                        }
                    }
                } else {
                    p.set_onion(0, None, 0);
                    p.set_onion(1, None, 0);
                }
                // GM6: THE LIGHT TABLE's own slot (it no longer borrows
                // slot 1): every pin, shift & trace offsets baked in.
                // The hash first: the shift resample runs only when a pin's
                // pixels or offset changed, never per frame.
                let mut bytes = Vec::new();
                for &(id, shift) in &pins {
                    if let Some((_, h)) = state.drawing_composite(id, f, true) {
                        bytes.extend_from_slice(&h.to_le_bytes());
                        for v in [shift.dx, shift.dy, shift.rot] {
                            bytes.extend_from_slice(&v.to_bits().to_le_bytes());
                        }
                    }
                }
                let hash = anim_core::value::fnv1a(&bytes);
                if bytes.is_empty() {
                    p.set_onion(2, None, 0);
                } else if p.onion_hash(2) != Some(hash) {
                    let center = pos2(
                        state.engine.project.width as f32 * 0.5,
                        state.engine.project.height as f32 * 0.5,
                    );
                    let mut shifted: Vec<(BTreeMap<TileCoord, Arc<TileData>>, f32)> = Vec::new();
                    for &(id, shift) in &pins {
                        if let Some((s, _)) = state.drawing_composite(id, f, true) {
                            shifted.extend(
                                s.into_iter().map(|(t, o)| (shift_tiles(t, shift, center), o)),
                            );
                        }
                    }
                    let slices: Vec<crate::paint::LayerSlice<'_>> =
                        shifted.iter().map(|(t, o)| (t, *o)).collect();
                    p.set_onion(2, Some(&slices), hash);
                }
            } else {
                p.set_onion(0, None, 0);
                p.set_onion(1, None, 0);
                p.set_onion(2, None, 0);
            }

            // ---- Multi-column raster display (B4): every OTHER column's
            // own resolved cel, refreshed every frame like onion (cheap —
            // sync_other_column is a no-op when the content hash matches).
            // GM5: only the columns that render (render_split skips hidden
            // and Ref ones). Prune first so a removed or hidden column's
            // texture doesn't leak; the active column's is kept for when
            // another column becomes active (as before).
            let active = state.view.active_column;
            let (below, above) = state.cut().render_split(active);
            let live_cols: Vec<ColumnId> = below.into_iter().chain(above).collect();
            let keep: Vec<ColumnId> = live_cols.iter().copied().chain([active]).collect();
            p.prune_other_columns(&keep);
            for &col in &live_cols {
                let composite = state.column_composite(col);
                p.sync_other_column(col, composite.as_ref().map(|(s, h)| (s.as_slice(), *h)));
            }
        }

        // GM22: the 3D underlay's picture (edit view only: composite view
        // shows the model as it delivers). Its job never starts mid-stroke.
        let stroking = self.stroke_active();
        let underlay = if composite_tex.is_none() {
            self.underlay.sync(state, ui.ctx(), stroking)
        } else {
            None
        };
        self.underlay_drawn = None;
        self.composite_shown = composite_tex.is_some();
        // Its picture is still to come (a first one, or this frame's): the
        // lamp, as View3D shows it.
        let underlay_pending = composite_tex.is_none()
            && self.underlay.busy()
            && crate::view3d::model_node(state.cut(), state.selected_node).is_some();

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
            paint_paper_image(&painter, &xf, id, plate::UNTINTED);
            // Painted overlay tag (zero layout impact) so the mode is never
            // ambiguous — this view looks like art, not like a mode.
            let tag = plate::picture_tag(
                &painter,
                pos2(paper_rect.left() + 8.0, paper_rect.top() + 6.0),
                "composite view",
            );
            if composite_pending {
                // GM9: the lamp, and in words — an FX render is pending
                // (the last result shows until it lands).
                painter.circle_filled(pos2(tag.right() + 12.0, tag.center().y), 4.0, crate::plate::TALLY);
                plate::picture_tag(&painter, pos2(tag.right() + 24.0, tag.top()), "rendering FX…");
            }
        }
        if let Some(note) = composite_note {
            // Red overrules the hand; the wash holds it on any desk.
            let words = painter.layout_no_wrap(note.to_string(), egui::FontId::proportional(13.0), plate::AKA);
            let pill = Rect::from_center_size(
                pos2(rect.center().x, rect.top() + 34.0),
                words.size() + vec2(16.0, 4.0),
            );
            painter.rect_filled(pill, plate::R_PILL, plate::RED_WASH);
            painter.galley(pill.min + vec2(8.0, 2.0), words, plate::AKA);
        }

        // 0. GM22: the 3D underlay, faded under every column, on the frame,
        // at the light table's opacity (the lightbox's pin opacity).
        if let Some(u) = underlay {
            let opacity = self.light_table.opacity.clamp(0.0, 1.0);
            paint_paper_image(&painter, &xf, u.tex, crate::plate::faded(opacity));
            let p = &state.engine.project;
            self.underlay_drawn = Some(UnderlayDrawn {
                node: u.node,
                frame: u.frame,
                rect: anim_core::camera::IRect::frame(p.width, p.height),
                opacity,
            });
        }

        // 1. The columns UNDER the active one (GM5: Cut::render_split, the
        // order export, eval and graphcomp use — BG, then Cel, then Book,
        // each band in sheet order; hidden and Ref columns never draw): each
        // column's own resolved RASTER cel (B4 — was invisible in edit view
        // before; only composite view showed it) plus any legacy vector
        // strokes. The columns OVER it draw at 3c. Reborrow, not move —
        // `paint` is consumed by value at 3b below, so the over stack's
        // textures are looked up here.
        let (below, above) = cut.render_split(active_col);
        self.stacks = (below.clone(), above.clone());
        let other_tex = paint.as_deref(); // Option<&PaintLayer>, Copy
        let tex_of = |col: ColumnId| {
            other_tex
                .filter(|_| self.raster)
                .and_then(|p| p.other_column_id(col))
        };
        let above: Vec<(ColumnId, Option<egui::TextureId>)> =
            above.into_iter().map(|col| (col, tex_of(col))).collect();
        let draw_column = |col: ColumnId, tex: Option<egui::TextureId>| {
            if let Some(id) = tex {
                paint_paper_image(&painter, &xf, id, plate::UNTINTED);
            }
            if let Some(id) = cut.xsheet.column(col).and_then(|c| c.resolve(frame))
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
        };
        if edit_view {
            for &col in &below {
                draw_column(col, tex_of(col));
            }
        }

        // 2a. THE LIGHT TABLE (vector path; GM6 generalises the ghost pin):
        // every pinned drawing haunts every frame in Ao at the light table's
        // opacity, moved by its shift & trace offset — display only.
        if edit_view && !self.flip_held {
            let a = (255.0 * self.light_table.opacity.clamp(0.0, 1.0)) as u8;
            let center = pos2(paper_w * 0.5, paper_h * 0.5);
            for (pid, shift) in state.view.light.shown(cut) {
                if state.resolve_at(active_col, frame) == Some(pid) && shift.is_identity() {
                    continue;
                }
                if let Some(d) = cut.drawing(pid) {
                    let moved = |p: Pos2| to_screen(shift.apply(p, center));
                    draw_strokes(
                        &painter,
                        &d.strokes,
                        &moved,
                        scale,
                        Some(ghost_tint(
                            [plate::PENCIL_AO.r(), plate::PENCIL_AO.g(), plate::PENCIL_AO.b()],
                            a,
                        )),
                        &self.pen_curve,
                    );
                }
            }
        }

        // 2. Onion ghosts of the active column, under its current drawing.
        if edit_view && state.view.onion && !self.flip_held {
            let ghosts = onion_ghosts(state, active_col, frame, &self.onion);
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
            // Ghost BEHIND leans Ao (continuity), ghost AHEAD leans
            // Legend (spec defect 14) — as multiply tints for now; the
            // true re-ink lands on the VECTOR path in Phase 4, and the
            // raster blit shader stays untouched (Do-NOT-build).
            let ga = (255.0 * self.onion.opacity.clamp(0.0, 1.0)) as u8;
            if self.flip_held {
                // THE FLIP: the previous drawing full-strength, alone —
                // the current cel steps aside until R lifts.
                if let Some(id) = p.onion_id(0) {
                    paint_paper_image(&painter, &xf, id, plate::UNTINTED);
                }
            } else {
                // GM6: the light table's pins first (SCAFFOLD, in Ao at the
                // light table's opacity), then the ghosts behind (Ao) and
                // ahead (the quiet Legend lean).
                if let Some(id) = p.onion_id(2) {
                    let la = (255.0 * self.light_table.opacity.clamp(0.0, 1.0)) as u8;
                    paint_paper_image(&painter, &xf, id, ghost_tint(ONION_BEHIND_TINT, la));
                }
                if state.view.onion {
                    if let Some(id) = p.onion_id(0) {
                        paint_paper_image(&painter, &xf, id, ghost_tint(ONION_BEHIND_TINT, ga));
                    }
                    if let Some(id) = p.onion_id(1) {
                        paint_paper_image(&painter, &xf, id, ghost_tint(ONION_AHEAD_TINT, ga));
                    }
                }
                // Layers under the active one (per-layer opacities baked in).
                // GM20: the sandwich is layer-sized — its quads cover the
                // layer rect, the paper beyond the frame included.
                let lr = p.rect();
                paint_layer_image(&painter, &xf, &lr, p.below_id(), plate::UNTINTED);
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
                    paint_layer_image(&painter, &xf, &lr, p.texture_id(), plate::tint(t));
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
                        paint_layer_image(&painter, &xf, &lr, p.wet_id(), plate::tint(wa));
                    }
                }
                // Layers over the active one.
                paint_layer_image(&painter, &xf, &lr, p.above_id(), plate::UNTINTED);
                // STAGE 2: other artists' LIVE ink, straight from their
                // dab streams (alpha pre-scaled; cleared at commit).
                if !self.remote_live.is_empty() {
                    paint_layer_image(&painter, &xf, &lr, p.remote_id(), plate::UNTINTED);
                }
            }
        }

        // 3c. The columns OVER the active one (GM5: render_split's upper
        // part — a Book overlay, the cels after it in sheet order): raster
        // cel and vector strokes, above the active column and its sandwich.
        if edit_view {
            for &(col, tex) in &above {
                draw_column(col, tex);
            }
        }

        // 4. In-progress stroke preview (vector mode only — the raster layer
        //    already shows the live stroke when raster is on).
        if edit_view && !self.raster && self.current.len() >= 2 {
            let c = self.brush_color;
            let color = plate::chip([c[0], c[1], c[2], c[3]]);
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

        // GM18: a manga page's guides — its bleed edge, trim and inner
        // frame, its panels and rulers — and the panel tool's drag, over
        // the art in either view.
        if let Some(page) = anim_core::manga::Page::of(state.cut()) {
            let armed = self.tool == CanvasTool::Panel;
            crate::manga_tools::paint_page(ui, &painter, state, &page, &to_screen, armed, self.koma.as_ref());
        }

        // Empty-cell hint (vector mode only).
        if edit_view
            && !self.raster
            && !state.view.playing
            && state.current_drawing().is_none()
            && self.current.is_empty()
        {
            plate::picture_note(
                &painter,
                paper_rect.center(),
                "empty cell — draw to create a new drawing here",
            );
        }

        // GM6: the key's spacing chart (タイミング指示) where the key animator
        // draws it — in the paper's top-right margin, in Ao (the blue pencil
        // that never photographs): this key at the top rule, the next key
        // at the bottom one, a tick per inbetween.
        if edit_view
            && !state.view.playing
            && let Some(here) = state.current_drawing()
            && let Some(chart) = state.cut().drawing_def(here).chart
        {
            let h = (paper_rect.height() * 0.3).clamp(48.0, 160.0);
            let anchor = to_screen(pos2(paper_w * 0.96, paper_h * 0.05));
            let glyph = Rect::from_min_size(anchor - vec2(16.0, 0.0), vec2(16.0, h));
            chart_glyph(&painter, glyph, &chart.marks, plate::PENCIL_AO);
            let to = state.cut().drawing(chart.to).map(|d| d.name.clone()).unwrap_or_default();
            let resp = ui.interact(glyph, ui.id().with("canvas_chart"), Sense::hover());
            plate::speak(
                &resp,
                egui::WidgetType::Label,
                None,
                &format!("spacing chart on the paper: {} marks toward {to}", chart.marks.len()),
            );
        }

        // Playback indicator as a canvas OVERLAY (painted, zero layout impact
        // — a toolbar label here used to reflow the row and shift the view).
        if self.flip_held {
            plate::picture_note(
                &painter,
                pos2(rect.center().x, rect.top() + 16.0),
                "Flip — previous drawing (release R)",
            );
        }
        if state.view.playing {
            plate::picture_note(
                &painter,
                pos2(rect.center().x, rect.top() + 16.0),
                "Playing — space to stop",
            );
        }
        // GM6: shift & trace rests the pen — the paper says so (the Tally
        // dot: the latch is ARMED), and a click on the chip ends the latch.
        if tracing {
            let text = "SHIFT & TRACE — pen resting · click to end";
            let font = egui::FontId::proportional(12.0);
            let w = ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font.clone(), plate::LEGEND).size().x);
            // Below the FLIP line and composite view's note.
            let chip = Rect::from_center_size(pos2(rect.center().x, rect.top() + 58.0), vec2(w + 30.0, 20.0));
            let resp = ui.interact(chip, ui.id().with("shift_trace_chip"), Sense::click());
            plate::speak(&resp, egui::WidgetType::Button, Some(true), "shift & trace: pen resting (click to end)");
            painter.rect_filled(chip, plate::R_PILL, plate::WELL);
            painter.circle_filled(pos2(chip.left() + 11.0, chip.center().y), 3.5, plate::TALLY);
            painter.text(
                pos2(chip.left() + 21.0, chip.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                font,
                plate::LEGEND,
            );
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                state.view.light.shift_trace = false;
                state.status = "shift & trace off — the pen draws again".into();
            }
        }

        // The refusal reaches the paper: a fresh refusal flashes the
        // canvas edge in red, so NO is seen from the pen tip (Foot law).
        if state.refusal_seq != self.flash_seq {
            self.flash_seq = state.refusal_seq;
            self.flash_since = ui.input(|i| i.time);
        }
        let fage = ui.input(|i| i.time) - self.flash_since;
        if fage < 0.9 && state.refusal_seq != 0 {
            let fppp = ui.ctx().pixels_per_point();
            let flash = egui::Stroke::new(crate::plate::device_px(2.0, fppp), crate::plate::AKA);
            if upright {
                painter.rect_stroke(paper_rect, 0, flash, egui::StrokeKind::Outside);
            } else {
                // GM4: turned, the flash rides the paper's own edge.
                painter.add(egui::Shape::closed_line(xf.paper_corners().to_vec(), flash));
            }
            ui.ctx().request_repaint();
        }

        // ---- LASSO FILL preview: the loop so far, blue dashed, closed.
        if self.tool == CanvasTool::LassoFill && self.lasso_pts.len() >= 2 {
            let mut pts: Vec<Pos2> =
                self.lasso_pts.iter().map(|p| to_screen(*p)).collect();
            pts.push(pts[0]);
            painter.extend(egui::Shape::dashed_line(
                &pts,
                egui::Stroke::new(1.0, plate::BLUE),
                4.0,
                3.0,
            ));
        }

        // ---- SESSION PRESENCE (PSD-session-room): the other artists.
        // Same-frame peers draw at full presence — blue pencil cursor ring,
        // name tag, live wet ghost; peers on other frames are named at the
        // paper's top edge in Legend (you know where they are, they never
        // paint over your frame).
        if !self.peers.is_empty() {
            let mut away = 0;
            for peer in &self.peers.clone() {
                if peer.frame == state.view.frame {
                    if let Some(c) = peer.cursor {
                        let sp = to_screen(pos2(c[0], c[1]));
                        painter.circle_stroke(sp, 5.0, egui::Stroke::new(1.5, crate::plate::PENCIL_AO));
                        if peer.pen_down {
                            painter.circle_filled(sp, 2.0, crate::plate::PENCIL_AO);
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
                                plate::alpha(plate::PENCIL_AO, 200),
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

        // GM22: the 3D underlay's picture is on its way — the lamp, and in
        // words, in the canvas's corner past the rule (View3D's dot).
        if underlay_pending {
            let at = rect.left_top() + vec2(24.0, 14.0);
            painter.circle_filled(at, 4.0, crate::plate::TALLY);
            painter.text(
                at + vec2(8.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "3D underlay rendering…",
                egui::FontId::proportional(12.0),
                crate::plate::LEGEND,
            );
        }

        // ---- THE RULE's mirror (spec §5.4): the active column's run
        // marks at the canvas's left edge, at the sheet's own 18pt pitch —
        // "how long is this hold, am I on ones, which frame" answered in
        // peripheral vision at the pen tip, like a VU meter at a desk.
        // AU4: it mirrors the sheet, so it stands with the sheet, in the
        // inset the rail leaves it.
        if !self.composite_view && self.sheet_shown {
            let count = state.frame_count();
            let strip = Rect::from_min_max(
                pos2(rect.left(), rect.top() + 6.0),
                pos2(rect.left() + crate::rail::RAIL_X, rect.bottom() - 6.0),
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
        && !hand // GM4: the Space hand shows its own grab cursor
        && !tracing // GM6: shift & trace's move hand (the pen rests)
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
                ellipse(r * aspect, r, plate::shade(170));
                ellipse(
                    (r * aspect - 1.0).max(0.5),
                    (r - 1.0).max(0.5),
                    plate::halo(170),
                );
            } else {
                painter.circle_stroke(
                    pos,
                    r,
                    egui::Stroke::new(1.0, plate::shade(170)),
                );
                painter.circle_stroke(
                    pos,
                    (r - 1.0).max(0.5),
                    egui::Stroke::new(1.0, plate::halo(170)),
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
                    egui::Stroke::new(3.0, plate::halo(150)),
                );
                painter.line_segment(
                    [pos, tip],
                    egui::Stroke::new(1.2, plate::shade(200)),
                );
            }
            if self.erasing {
                painter.circle_filled(pos, 1.3, plate::shade(180));
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
            // GM20: a stroke on another rect than the layer's rebuilds the
            // layer (clearing the wet target) — it waits for the local pen
            // stroke to land too.
            if let Some(SeqTask::Stroke(rs)) = self.replay_inbox.front()
                && (!self.current.is_empty() || self.raster_stroke_done)
                && self.replay_rect(state, DrawingId(rs.drawing)) != p.rect()
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
                    // v0.2.4 (the X-sheet law's eye): structure edits
                    // are the corruption-prone kind — name them.
                    let structural = cmds
                        .iter()
                        .filter(|c| {
                            matches!(
                                c,
                                anim_core::command::Command::AddDrawing { .. }
                                    | anim_core::command::Command::SetCell { .. }
                            )
                        })
                        .count();
                    if structural > 0 {
                        crate::net::slog(format!(
                            "XSHEET {structural} structure cmd(s) from {origin} applying"
                        ));
                    }
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
                            // GM5: a peer may have removed our cut or column.
                            state.keep_view_live();
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
            // GM20: back onto the canvas's own rect (a replay paints on its
            // stroke's cut's paper; a peer's edit may have moved the paper).
            self.ensure_layer(state, p);
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
        // GM20: on the stroke's own cut's paper (run_replays puts the
        // canvas's rect back afterwards).
        let rect = self.replay_rect(state, did);
        self.layer_onto(p, rect);
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
        // GM20: art outside a capped or shrunk layer stays put.
        let tiles = paint::merge_outside(&base, p.read_tiles(), &p.rect());
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
        let base = self.ink();
        let flow = self.brush_flow.clamp(0.0, 1.0);
        // GM4: the brush's own edge; aliased rides the hardness sentinel.
        let hardness = if self.brush_aliased {
            crate::paint::ALIASED_HARDNESS
        } else {
            self.brush_hardness.clamp(0.0, 1.0)
        };
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
            // (in paper axes — GM4: the tilt was turned back by the view's
            // angle and unmirrored when it was sampled, `dir_to_paper`).
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

    /// GM6: shift & trace — a drag moves the selected pin by the paper
    /// distance travelled; Shift+drag turns it about the paper centre.
    /// View state only: no command, no undo step, no pixel touched.
    fn shift_trace_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        to_paper: &impl Fn(Pos2) -> Pos2,
        center: Pos2,
        state: &mut AppState,
    ) {
        let Some(p) = response.interact_pointer_pos().map(to_paper) else {
            self.shift_drag = None;
            return;
        };
        if response.dragged_by(egui::PointerButton::Primary) {
            if let Some(last) = self.shift_drag {
                if ui.input(|i| i.modifiers.shift) {
                    let a0 = (last.y - center.y).atan2(last.x - center.x);
                    let a1 = (p.y - center.y).atan2(p.x - center.x);
                    state.view.light.nudge(0.0, 0.0, a1 - a0);
                } else {
                    state.view.light.nudge(p.x - last.x, p.y - last.y, 0.0);
                }
            }
            self.shift_drag = Some(p);
        }
        if response.drag_stopped() {
            self.shift_drag = None;
        }
    }

    /// GM6: the system clipboard keys over the canvas. egui-winit turns
    /// Ctrl+C / Ctrl+V into Copy / Paste events (no key event), so the
    /// canvas answers them here, when the pointer is over it and no field
    /// is typing: Copy asks for Copy pixels; a Paste carrying the text the
    /// canvas handed over for the last pixel copy asks for Paste pixels,
    /// and other text is refused. A frame that also carries the chord's key
    /// leaves it to the action, so one press never acts twice.
    fn clipboard_events(&mut self, ui: &egui::Ui, rect: Rect, state: &mut AppState) {
        use crate::config::Action;
        // A fresh pixel copy (keys, menu or palette) hands the system
        // clipboard its marker text, so the paste key sends it back.
        if let Some(clip) = &state.pixel_clip
            && state.pixel_clip_text.is_none()
        {
            let p = &clip.patch;
            let text = format!("Gaban Studio pixels: {}x{} at {}, {}", p.w, p.h, p.x0, p.y0);
            ui.ctx().copy_text(text.clone());
            state.pixel_clip_text = Some(text);
        }
        if !ui.rect_contains_pointer(rect) || ui.ctx().text_edit_focused() {
            return;
        }
        let (copy, paste, keyed) = ui.input(|i| {
            let mut out = (false, None, false);
            for e in &i.events {
                match e {
                    egui::Event::Copy => out.0 = true,
                    egui::Event::Paste(text) => out.1 = Some(text.clone()),
                    _ => {}
                }
            }
            out.2 = i.modifiers.command
                && [egui::Key::C, egui::Key::V]
                    .iter()
                    .any(|k| i.key_pressed(*k));
            out
        });
        if keyed {
            return;
        }
        if copy {
            self.asked.push(Action::CopyPixels);
        } else if let Some(text) = paste {
            // The system hands text back with its own line ends.
            let plain = |t: &str| t.replace("\r\n", "\n").trim_end().to_string();
            if state.pixel_clip.is_some()
                && state.pixel_clip_text.as_deref().map(plain) == Some(plain(&text))
            {
                self.asked.push(Action::PastePixels);
            } else {
                state.refuse("refused — the clipboard holds text, not pixels (copy pixels first)");
            }
        }
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
            if state.refuse_if_locked() {
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

    /// GM18: the panel (koma) tool's input — a click selects the panel
    /// under it; a drag from a vanishing point or the horizon (within
    /// [`crate::manga_tools::HANDLE_REACH`] of it on screen) moves it, else
    /// draws a panel on bare paper or splits the one it crosses. The page
    /// changes once, when the drag lands (one undo step,
    /// [`crate::manga_tools::koma_gesture`]); Esc drops it.
    fn koma_input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        to_paper: &impl Fn(Pos2) -> Pos2,
        state: &mut AppState,
    ) {
        use anim_core::manga::{self, Page, Rulers};
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }
        if self.koma.is_some()
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.koma = None;
            state.status = "panel drag dropped".into();
            return;
        }
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
            && let Some((_, polys)) = manga::panels_of(state.cut())
        {
            let p = to_paper(pos);
            if let Some(i) = manga::panel_at(&polys, [p.x, p.y]) {
                state.manga.selected = i;
                state.status = format!("panel {} selected", i + 1);
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            if Page::of(state.cut()).is_none() {
                state.refuse("refused — this cut is no page (New page makes one)");
                return;
            }
            // From where the press landed, not where the drag was seen.
            let Some(origin) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos()) else {
                return;
            };
            let from = to_paper(origin);
            let reach = crate::manga_tools::HANDLE_REACH / self.xf.zoom.max(1e-6);
            self.koma = Some(crate::manga_tools::KomaDrag {
                from: [from.x, from.y],
                to: [from.x, from.y],
                handle: Rulers::of(state.cut()).handle_at([from.x, from.y], reach),
                reach,
            });
        }
        let Some(k) = &mut self.koma else {
            return;
        };
        if let Some(pos) = response.interact_pointer_pos().or(ui.input(|i| i.pointer.latest_pos())) {
            let p = to_paper(pos);
            k.to = [p.x, p.y];
        }
        if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            let k = *k;
            self.koma = None;
            crate::manga_tools::koma_gesture(state, k);
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
        let (bx0, by0, bx1, by1) = (x0 - pad, y0 - pad, x1 + pad, y1 + pad);
        let clip_to = |a: IRect| {
            let (x0, y0) = (bx0.max(a.x), by0.max(a.y));
            let (x1, y1) = (bx1.min(a.x + a.w as i32 - 1), by1.min(a.y + a.h as i32 - 1));
            (x0 <= x1 && y0 <= y1).then_some((x0, y0, x1, y1))
        };
        // GM20: the paper's edges, beyond the frame when it is wider — the
        // GPU's window of a capped paper. A loop wholly off it is refused on
        // the Aka lane, as a fill click there is.
        let Some((x0, y0, x1, y1)) = clip_to(self.sync_layer_rect(state).irect()) else {
            state.refuse(if clip_to(state.paper_area()).is_some() {
                crate::doc::PAST_WINDOW
            } else {
                crate::doc::OUTSIDE_PAPER
            });
            return;
        };
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
        let c = self.ink();
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
        // GM4 EYEDROPPER: Alt+click picks the composited colour instead.
        if ui.input(|i| i.modifiers.alt) {
            if (response.clicked_by(egui::PointerButton::Primary)
                || response.drag_started_by(egui::PointerButton::Primary))
                && let Some(pos) = response.interact_pointer_pos().or(response.hover_pos())
                && rect.contains(pos)
            {
                self.eyedrop(pos, to_paper, state);
            }
            return;
        }
        // The canvas senses clicks too, so a still press starts no drag: it
        // lands as a click on release (as the eyedropper's does), a press
        // that moves as a drag start — one fill either way.
        if !(response.clicked_by(egui::PointerButton::Primary)
            || response.drag_started_by(egui::PointerButton::Primary))
        {
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
        // GM7 REFILL: Shift+click recolours the clicked flat (bounded by the
        // line), Ctrl+Shift+click every texel of its colour on the layer —
        // inside the active selection, like every fill. Exact texel work on
        // the CPU, so it needs no GPU.
        let m = ui.input(|i| i.modifiers);
        // GM20: every fill stays where the canvas paints — the paper, or the
        // GPU's window of a capped one (what lies past it is not shown).
        let area = self.sync_layer_rect(state).irect();
        if m.shift {
            let p = to_paper(pos);
            let seed = (p.x.floor() as i32, p.y.floor() as i32);
            let clip = self.selection_poly();
            state.refill_at(
                seed,
                self.brush_color,
                m.command,
                self.fill_grow,
                area,
                clip.as_deref(),
            );
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
        if state.refuse_if_locked() {
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
        // GM20: the flood ranges over the paper, beyond the frame too.
        let mask = match anim_core::fill::flood_fill_mask_in(
            &reference,
            seed,
            area,
            &opts,
            clip.as_deref(),
        ) {
            Ok(m) => m,
            Err(r) => {
                use anim_core::fill::FillRefusal as FR;
                state.status = match r {
                    FR::OffPaper => {
                        state.refuse_off_area(seed);
                        return;
                    }
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
        let c = self.ink();
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
        // GM5: a locked column refuses at pen-down, before any ink (raster
        // or vector) — never a whole stroke drawn, then dropped.
        if state.refuse_if_column_locked() {
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
        if self.raster && state.refuse_if_locked() {
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
        self.tap_begin(p);
        self.push_point(StrokePoint {
            x: p.x,
            y: p.y,
            pressure: p0,
            tilt: self.xf.dir_to_paper(self.smoothed_tilt),
        });
        true
    }

    /// End the live stroke (release pos/force are unreliable — the taper is
    /// synthesized from the committed points instead).
    fn stroke_end(&mut self, state: &mut AppState) {
        if self.touch_active {
            self.stab_catch_up();
            self.finish_stroke(state);
            self.mouse_lockout = 1;
        }
        self.touch_active = false;
        self.seed_pending = false;
    }

    /// GM4: a new stroke's tap, and the stabilizer's string tied at its
    /// first paper point (string length in screen points, so `1 / zoom`).
    fn tap_begin(&mut self, p: Pos2) {
        self.stroke_tap.clear();
        let mut s =
            crate::stabilizer::Stabilizer::new(self.stabilizer, 1.0 / self.xf.zoom.max(1e-6));
        s.feed([p.x, p.y]);
        self.stab = Some(s);
        self.stab_raw = p;
        // GM18: the rulers pick the stroke's line once the pen has moved
        // SNAP_PICK_PX screen px.
        self.ruler_snap = self.snap_rulers.clone().map(|r| {
            let mut snap = anim_core::manga::Snap::new(r, anim_core::manga::SNAP_PICK_PX / self.xf.zoom.max(1e-6));
            snap.feed([p.x, p.y]);
            snap
        });
    }

    /// GM18: a raw paper point held to the rulers' line (as it is when
    /// their snap is off).
    pub(crate) fn ruler_feed(&mut self, p: Pos2) -> Pos2 {
        match self.ruler_snap.as_mut() {
            Some(s) => {
                let q = s.feed([p.x, p.y]);
                pos2(q[0], q[1])
            }
            None => p,
        }
    }

    /// GM18: one raw paper point held to the rulers, then through the
    /// stabilizer — the pen's path and the mouse's alike.
    fn held_feed(&mut self, p: Pos2) -> Option<Pos2> {
        let held = self.ruler_feed(p);
        self.stab_feed(held)
    }

    /// GM4: one raw paper point through the stabilizer — the ink point to
    /// draw, or None while the pen is inside the string's slack.
    fn stab_feed(&mut self, p: Pos2) -> Option<Pos2> {
        self.stab_raw = p;
        match self.stab.as_mut() {
            Some(s) => s.feed([p.x, p.y]).map(|q| pos2(q[0], q[1])),
            None => Some(p),
        }
    }

    /// GM4: pen-up — reel the string in, so the stroke ends on the pen-up
    /// point (nothing at level 0).
    fn stab_catch_up(&mut self) {
        let Some(mut s) = self.stab.take() else {
            return;
        };
        let Some(last) = self.current.last().copied() else {
            return;
        };
        for q in s.finish([self.stab_raw.x, self.stab_raw.y]) {
            self.push_point(StrokePoint {
                x: q[0],
                y: q[1],
                ..last
            });
        }
    }

    /// GM4: append to the live stroke and its tap.
    fn push_point(&mut self, sp: StrokePoint) {
        self.stroke_tap.push(StrokeSample {
            x: sp.x,
            y: sp.y,
            pressure: sp.pressure,
            tilt: sp.tilt,
        });
        self.current.push(sp);
    }

    /// GM4: Shift+click after a stroke — a straight segment from where the
    /// last stroke ended to the click, a point every ~2 screen points, each
    /// exactly on the segment. The stroke's own guards and commit run.
    #[allow(clippy::too_many_arguments)]
    fn rule_line(
        &mut self,
        from: Pos2,
        click: Pos2,
        force: Option<f32>,
        rect: Rect,
        to_paper: &impl Fn(Pos2) -> Pos2,
        state: &mut AppState,
        from_mouse: bool,
    ) -> bool {
        if !self.stroke_start(click, force, Some([0.0, 0.0]), rect, to_paper, state) {
            return false;
        }
        let to = to_paper(click);
        let first = self.current[0];
        self.current.clear();
        self.stroke_tap.clear();
        self.stab = None;
        let n = (((to - from).length() * self.xf.zoom / 2.0).ceil() as usize).max(1);
        for k in 0..=n {
            let t = k as f32 / n as f32;
            let p = from.lerp(to, t);
            self.push_point(StrokePoint {
                x: p.x,
                y: p.y,
                ..first
            });
        }
        self.seed_pending = false;
        self.stroke_from_mouse = from_mouse;
        self.stroke_end(state);
        true
    }

    /// GM4 EYEDROPPER: the composited colour under `screen` into the brush,
    /// as the chip that paints it; the brush keeps its own alpha (a soft
    /// edge's coverage is not a colour).
    fn eyedrop(&mut self, screen: Pos2, to_paper: &impl Fn(Pos2) -> Pos2, state: &mut AppState) {
        let p = to_paper(screen);
        match pick_chip(state, p.x, p.y, self.composite_view) {
            Some([r, g, b]) => {
                self.brush_color = [r, g, b, self.brush_color[3]];
                // GM7 CHIP PICK: an exact chip of the active palette becomes
                // the active chip (its pot rings); any other colour, none.
                state.active_chip = state.chip_of([r, g, b]);
                let chip = state
                    .active_chip
                    .and_then(|ri| state.palettes.characters.get(state.active_character)?.roles.get(ri))
                    .map(|role| format!(" — chip '{}'", role.name))
                    .unwrap_or_default();
                state.status = format!("picked #{r:02x}{g:02x}{b:02x}{chip}");
            }
            None => state.refuse("nothing to pick — the paper is bare there"),
        }
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
        // GM4: Alt = the eyedropper (no ink); Shift on a fresh press after
        // a stroke = rule a straight line on from where it ended.
        let mods = ui.input(|i| i.modifiers);
        let picking = mods.alt && !self.touch_active && self.current.is_empty();
        let ruling = mods.shift && !mods.alt && !self.touch_active && self.current.is_empty();

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
                                if picking {
                                    if rect.contains(s.pos) {
                                        self.eyedrop(s.pos, to_paper, state);
                                    }
                                } else if ruling && let Some(a) = self.line_anchor {
                                    let f = s.pressure.filter(|f| *f > 0.0).or(Some(self.last_pressure));
                                    self.rule_line(a, s.pos, f, rect, to_paper, state, false);
                                } else {
                                    self.stroke_start(
                                        s.pos, s.pressure, s.tilt, rect, to_paper, state,
                                    );
                                }
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
                            self.seen_pen = true; // a pen on the canvas — not a mouse
                            if picking {
                                if rect.contains(*pos) {
                                    self.eyedrop(*pos, to_paper, state);
                                }
                            } else if ruling && let Some(a) = self.line_anchor {
                                let f = force.filter(|f| *f > 0.0).or(Some(self.last_pressure));
                                self.rule_line(a, *pos, f, rect, to_paper, state, false);
                            } else {
                                self.stroke_start(
                                    *pos, *force, Some([0.0, 0.0]), rect, to_paper, state,
                                );
                            }
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
            // GM4: Alt+click picks; Shift+click rules on from the last
            // stroke's end. Neither inks a drag of its own.
            let click = response.clicked_by(egui::PointerButton::Primary)
                || (response.drag_started_by(egui::PointerButton::Primary)
                    && (picking || (ruling && self.line_anchor.is_some())));
            if (picking || ruling) && self.current.is_empty() {
                if click
                    && let Some(pos) = response.interact_pointer_pos().or(response.hover_pos())
                {
                    if picking {
                        if rect.contains(pos) {
                            self.eyedrop(pos, to_paper, state);
                        }
                    } else if let Some(a) = self.line_anchor {
                        self.rule_line(a, pos, Some(MOUSE_PRESSURE), rect, to_paper, state, true);
                        // The mouse never set the pen lockout; keep it so.
                        self.mouse_lockout = 0;
                    }
                }
                if picking || self.line_anchor.is_some() {
                    return;
                }
            }
            if response.drag_started_by(egui::PointerButton::Primary) {
                if self.composite_view {
                    state.refuse("refused — composite view is read-only (C to edit)");
                    return;
                }
                // GM5: the column lock, as at the pen's pen-down.
                if state.refuse_if_column_locked() {
                    return;
                }
                if self.raster && state.active_layer_props().is_some_and(|p| !p.visible) {
                    state.refuse(format!(
                        "refused — layer '{}' is hidden (A to switch, or click its eye)",
                        state.active_layer_name()
                    ));
                    return;
                }
                if self.raster && state.refuse_if_locked() {
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
                    self.tap_begin(p);
                    self.push_point(StrokePoint {
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
                if let Some(p) = response.interact_pointer_pos()
                    && let Some(p) = self.held_feed(to_paper(p))
                {
                    self.push_point(StrokePoint {
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
                self.stab_catch_up();
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
        // GM4: through the stabilizer's string; inside its slack the point
        // banks like a stationary sample (pressure and tilt still track).
        // GM18: held to the perspective rulers first, when their snap is on.
        let fed = self.held_feed(to_paper(pos));
        let p = fed
            .or_else(|| self.current.last().map(|l| pos2(l.x, l.y)))
            .unwrap_or(self.stab_raw);
        let moved = if fed.is_none() {
            0.0
        } else {
            self.current
                .last()
                .map(|last| ((p.x - last.x).powi(2) + (p.y - last.y).powi(2)).sqrt() * scale)
                .unwrap_or(f32::INFINITY)
        };

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
                let pt = self.xf.dir_to_paper(t);
                if self.dabs_flushed == 0
                    && let Some(first) = self.current.first_mut()
                {
                    first.tilt = pt;
                    if let Some(s) = self.stroke_tap.first_mut() {
                        s.tilt = pt;
                    }
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
                if let Some(s) = self.stroke_tap.first_mut() {
                    s.pressure = raw;
                }
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
            let pt = self.xf.dir_to_paper(self.smoothed_tilt);
            if !self.raster
                && let Some(lp) = self.current.last_mut()
            {
                lp.pressure = self.smoothed_pressure;
                lp.tilt = pt;
                if let Some(s) = self.stroke_tap.last_mut() {
                    s.pressure = self.smoothed_pressure;
                    s.tilt = pt;
                }
            }
            return;
        }
        self.push_point(StrokePoint {
            x: p.x,
            y: p.y,
            pressure: self.smoothed_pressure,
            tilt: self.xf.dir_to_paper(self.smoothed_tilt),
        });
    }
    fn finish_stroke(&mut self, state: &mut AppState) {
        // GM4: Shift+click rules on from where this stroke ends, on this
        // spot, until anything else happens (`retire_line_anchor`).
        self.line_anchor = self.current.last().map(|l| pos2(l.x, l.y));
        self.anchor_at = anchor_at(state);
        self.anchor_rev = None;
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
        self.anchor_rev = Some(state.engine.revision());
    }
}

/// Cel-layer identity ink for a role name (orientation aid), under the
/// IDENTITY AMENDMENT (spec §3.5, owner-ratified 2026-08-18): the
/// pipeline's own pencil code in the plate's own tokens — rough is the
/// ao-enpitsu, correction is the sakkan's aka. Identity is always a
/// small fill beside the role's NAME, never text or an edge, so it can
/// never be read as a refusal or a ghost.
/// PSD-cel-layers: identity ink by ROLE — the plate's own tokens (no hue
/// outside the eight; no green anywhere — spec §3), with the colour-trace
/// convention carried as strength: the boundary LINES wear their region's
/// ink at full strength, the regions wear it dimmed.
pub fn role_chip_color(role: anim_core::model::LayerRole) -> Color32 {
    use anim_core::model::LayerRole as R;
    match role {
        R::Line => plate::STRUCK,
        R::Color => plate::LEGEND,
        R::Shadow => plate::alpha(plate::PENCIL_AO, 150), // ao, dimmed
        R::Highlight => plate::PAPER,
        R::Rough => plate::PENCIL_AO,
        R::ShadowLine => plate::alpha(plate::PENCIL_AO, 230),
        R::HighlightLine => plate::alpha(plate::PENCIL_AKA, 170),
        R::Correction => plate::PENCIL_AKA,
        R::Reference => plate::INK3,
        R::Custom => plate::LEGEND,
    }
}

/// A layer's identity ink: its own chip override, else its role's.
pub fn props_chip_color(p: &anim_core::model::LayerProps) -> Color32 {
    match p.chip {
        Some(rgba) => plate::chip(rgba),
        None => role_chip_color(p.role),
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

/// GM4 EYEDROPPER: the composited pixel at paper (x, y) of the current
/// frame, premultiplied linear f32 — the column stack (one tile per
/// drawing, not the frame), or the graph's output in composite view.
/// None off the paper or where it is bare.
fn pick_sample(state: &AppState, x: f32, y: f32, composite: bool) -> Option<[f32; 4]> {
    let (w, h) = (state.engine.project.width, state.engine.project.height);
    // GM20: anywhere on the paper; the graph renders the frame only, so
    // beyond it the pick reads the column stack.
    let a = state.paper_area();
    let (xi, yi) = (x.floor(), y.floor());
    let (ax1, ay1) = (a.x as f32 + a.w as f32, a.y as f32 + a.h as f32);
    if xi < a.x as f32 || yi < a.y as f32 || xi >= ax1 || yi >= ay1 {
        return None;
    }
    let (xi, yi) = (xi as i32, yi as i32);
    let in_frame = xi >= 0 && yi >= 0 && (xi as u32) < w && (yi as u32) < h;
    let cut = state.cut();
    let frame = state.view.frame;
    let law = state.engine.project.color_law;
    let px = (composite && in_frame)
        .then(|| {
            let (ux, uy) = (xi as u32, yi as u32);
            anim_core::export::graph_composite_pixel_law(cut, law, frame, w, h, ux, uy)
        })
        .flatten()
        .unwrap_or_else(|| anim_core::export::composite_pixel_at(cut, frame, xi, yi));
    (anim_core::export::straight_rgba8(&px)[3] > 0).then_some(px)
}

/// GM4 EYEDROPPER: the composited RGBA (straight, 8-bit) at paper pixel
/// (x, y) — the exact bytes a render of the frame holds there (the tests'
/// window on the pick; the brush takes [`pick_chip`]).
#[cfg(test)]
pub fn pick_color(state: &AppState, x: f32, y: f32, composite: bool) -> Option<[u8; 4]> {
    pick_sample(state, x, y, composite).map(|px| anim_core::export::straight_rgba8(&px))
}

/// GM4 EYEDROPPER: the brush chip that paints the pixel at (x, y) — the
/// stored colour goes back through the project's colour law (GM7), the
/// exact inverse of the [`CanvasView::ink`] that put it there.
pub fn pick_chip(state: &AppState, x: f32, y: f32, composite: bool) -> Option<[u8; 3]> {
    let px = pick_sample(state, x, y, composite)?;
    let a = px[3].clamp(0.0, 1.0);
    let law = state.engine.project.color_law;
    Some([law.decode(px[0] / a), law.decode(px[1] / a), law.decode(px[2] / a)])
}

impl CanvasView {
    /// The brush colour as the stored (straight) value. ONE encode for the
    /// whole app: [`ColorLaw::encode`] is the same function Solid nodes
    /// render with (the CPU and GPU graph compositors), so "a Solid matches
    /// painting that colour" is a structural law, not two copies that
    /// could drift (GM7 D8).
    pub(crate) fn ink(&self) -> [f32; 4] {
        self.color_law.encode_rgba(self.brush_color)
    }
}

/// The v0.3 brush conversion, for the tests that pin Legacy pixels.
#[cfg(test)]
fn linear_rgba(c: [u8; 4]) -> [f32; 4] {
    ColorLaw::Legacy.encode_rgba(c)
}

// ---- GM6: the onion's depth and modes, and the light table ---------------

/// Which drawings the onion ghosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum OnionMode {
    /// The previous / next distinct drawings in the column (today's onion).
    #[default]
    Drawings,
    /// Fixed frame offsets: what the column shows 1, 2, … frames away.
    Frames,
    /// The previous / next Key (genga) drawings.
    Keys,
}

impl OnionMode {
    pub const ALL: [OnionMode; 3] = [OnionMode::Drawings, OnionMode::Frames, OnionMode::Keys];

    pub fn label(self) -> &'static str {
        match self {
            OnionMode::Drawings => "drawings",
            OnionMode::Frames => "frames",
            OnionMode::Keys => "keys (genga)",
        }
    }
}

/// GM6: the onion's settings — kept in the config's `onion` section.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OnionCfg {
    /// Ghosts behind and ahead, 0–5 each (0 = none on that side).
    pub before: u8,
    pub after: u8,
    pub mode: OnionMode,
    /// Also ghost every other column that renders.
    pub other_columns: bool,
    /// The nearest ghost's opacity (the old onion strength).
    pub opacity: f32,
    /// Each step farther multiplies the opacity by this.
    pub falloff: f32,
}

impl Default for OnionCfg {
    fn default() -> Self {
        Self {
            before: 1,
            after: 1,
            mode: OnionMode::Drawings,
            other_columns: false,
            opacity: 0.45,
            falloff: 0.6,
        }
    }
}

impl OnionCfg {
    pub const MAX_DEPTH: u8 = 5;

    /// The weight of a ghost `rank` steps away (1 = the nearest).
    pub fn weight(&self, rank: u32) -> f32 {
        let falloff = self.falloff.clamp(0.0, 1.0);
        self.opacity.clamp(0.0, 1.0) * falloff.powi(rank.saturating_sub(1) as i32)
    }

    /// Back in range after a hand-edited config.
    pub fn sane(self) -> Self {
        Self {
            before: self.before.min(Self::MAX_DEPTH),
            after: self.after.min(Self::MAX_DEPTH),
            opacity: if self.opacity.is_finite() { self.opacity.clamp(0.05, 1.0) } else { 0.45 },
            falloff: if self.falloff.is_finite() { self.falloff.clamp(0.0, 1.0) } else { 0.6 },
            ..self
        }
    }
}

/// GM6: the light table's settings — kept in the config's `light_table`
/// section (the pins and offsets themselves are session view state).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LightTableCfg {
    /// The fixed opacity every pin shows at.
    pub opacity: f32,
}

impl Default for LightTableCfg {
    fn default() -> Self {
        Self { opacity: 0.45 }
    }
}

/// GM6: the drawings the onion ghosts on `column` at `frame`, each with its
/// signed offset (−k behind, +k ahead: the rank in drawings and keys
/// mode, the frame distance in frames mode) and its weight. A side of
/// depth 0 ghosts nothing; the frame's own key never ghosts; a drawing
/// ghosts once (the nearest wins), and the drawing held over the frame
/// never ghosts ahead. At depth 1 in drawings mode this is exactly
/// `AppState::onion_neighbors`. With `other_columns`, every other column
/// that renders adds its own set after the active column's. This is the
/// GPU slots' set (on a held frame the held drawing is the reference you
/// draw the new cel over, so it ghosts behind).
pub fn onion_set(
    cut: &anim_core::model::Cut,
    column: ColumnId,
    frame: u32,
    cfg: &OnionCfg,
) -> Vec<(DrawingId, i32, f32)> {
    onion_set_with(cut, column, frame, cfg, false)
}

/// GM6: the vector path's onion — [`onion_set`], except that the drawing a
/// column shows at `frame` never ghosts there: the vector path draws it as
/// the frame itself, so on a held frame the side behind looks past it to
/// the drawing before instead of spending a rank on it.
pub fn vector_onion_set(
    cut: &anim_core::model::Cut,
    column: ColumnId,
    frame: u32,
    cfg: &OnionCfg,
) -> Vec<(DrawingId, i32, f32)> {
    onion_set_with(cut, column, frame, cfg, true)
}

/// GM6: what each GPU onion texture holds — slot 0 every ghost behind,
/// slot 1 every ghost ahead (farthest first, each with its weight over its
/// side's nearest: the nearest lies on top at full and the slot's tint
/// carries its opacity), slot 2 every light-table pin. Depth 5 on both
/// sides and any number of pins still fill [`crate::paint::ONION_SLOTS`]
/// textures (the spec's GPU budget).
pub fn onion_slot_plan(
    set: &[(DrawingId, i32, f32)],
    pins: &[(DrawingId, PinShift)],
) -> [Vec<(DrawingId, f32)>; crate::paint::ONION_SLOTS] {
    let side = |behind: bool| -> Vec<(DrawingId, f32)> {
        let mut side: Vec<&(DrawingId, i32, f32)> =
            set.iter().filter(|g| (g.1 < 0) == behind).collect();
        side.sort_by_key(|g| std::cmp::Reverse(g.1.unsigned_abs()));
        let near = side.iter().map(|g| g.2).fold(0.0f32, f32::max);
        side.into_iter()
            .map(|&(id, _, w)| (id, if near > 0.0 { w / near } else { 1.0 }))
            .collect()
    };
    [side(true), side(false), pins.iter().map(|(id, _)| (*id, 1.0)).collect()]
}

fn onion_set_with(
    cut: &anim_core::model::Cut,
    column: ColumnId,
    frame: u32,
    cfg: &OnionCfg,
    past_shown: bool,
) -> Vec<(DrawingId, i32, f32)> {
    let mut out = Vec::new();
    let mut cols = vec![column];
    if cfg.other_columns {
        let (below, above) = cut.render_split(column);
        cols.extend(below.into_iter().chain(above).filter(|c| *c != column));
    }
    for c in cols {
        if let Some(col) = cut.xsheet.column(c) {
            column_onion(cut, col, frame, cfg, past_shown, &mut out);
        }
    }
    out
}

fn column_onion(
    cut: &anim_core::model::Cut,
    col: &anim_core::xsheet::Column,
    frame: u32,
    cfg: &OnionCfg,
    past_shown: bool,
    out: &mut Vec<(DrawingId, i32, f32)>,
) {
    use anim_core::model::DrawingKind;
    use anim_core::xsheet::Exposure;
    let count = cut.frame_count.max(1);
    let held = col.resolve(frame);
    // The drawing that never ghosts here: the frame's own key, or (the
    // vector path) whatever the frame shows, held or keyed.
    let own = if past_shown {
        held
    } else {
        match col.key_at(frame) {
            Some(Exposure::Drawing(d)) => Some(d),
            _ => None,
        }
    };
    let before = cfg.before.min(OnionCfg::MAX_DEPTH) as u32;
    let after = cfg.after.min(OnionCfg::MAX_DEPTH) as u32;
    // True when `d` joined the set at this offset.
    let push = |out: &mut Vec<(DrawingId, i32, f32)>, d: DrawingId, offset: i32, rank: u32| {
        if Some(d) == own || out.iter().any(|(o, _, _)| *o == d) {
            return false;
        }
        out.push((d, offset, cfg.weight(rank)));
        true
    };
    match cfg.mode {
        OnionMode::Drawings => {
            let mut rank = 0;
            for f in (0..frame).rev() {
                if rank == before {
                    break;
                }
                if let Some(d) = col.resolve(f)
                    && push(out, d, -(rank as i32 + 1), rank + 1)
                {
                    rank += 1;
                }
            }
            let mut rank = 0;
            for f in frame.saturating_add(1)..count {
                if rank == after {
                    break;
                }
                if let Some(d) = col.resolve(f)
                    && Some(d) != held
                    && push(out, d, rank as i32 + 1, rank + 1)
                {
                    rank += 1;
                }
            }
        }
        OnionMode::Frames => {
            for k in 1..=before.min(frame) {
                if let Some(d) = col.resolve(frame - k) {
                    push(out, d, -(k as i32), k);
                }
            }
            for k in 1..=after {
                let f = frame.saturating_add(k);
                if f >= count {
                    break;
                }
                if let Some(d) = col.resolve(f)
                    && Some(d) != held
                {
                    push(out, d, k as i32, k);
                }
            }
        }
        OnionMode::Keys => {
            let is_key = |d: DrawingId| cut.drawing_kind(d) == DrawingKind::Key;
            let (mut rank, mut f) = (0, frame);
            while rank < before
                && let Some((kf, d)) = col.prev_key(f, is_key)
            {
                if push(out, d, -(rank as i32 + 1), rank + 1) {
                    rank += 1;
                }
                f = kf;
            }
            let (mut rank, mut f) = (0, frame);
            while rank < after
                && let Some((kf, d)) = col.next_key(f, is_key)
            {
                if kf >= count {
                    break;
                }
                if Some(d) != held && push(out, d, rank as i32 + 1, rank + 1) {
                    rank += 1;
                }
                f = kf;
            }
        }
    }
}

/// GM6: one pinned ghost's shift & trace offset — view only, never in the
/// document or the config.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PinShift {
    /// Paper-pixel translation.
    pub dx: f32,
    pub dy: f32,
    /// Turn about the paper centre, radians (clockwise on y-down paper).
    pub rot: f32,
}

impl PinShift {
    pub fn is_identity(&self) -> bool {
        self.dx == 0.0 && self.dy == 0.0 && self.rot == 0.0
    }

    /// Where the paper point `p` of the pinned drawing shows.
    pub fn apply(&self, p: Pos2, center: Pos2) -> Pos2 {
        let v = rotate_vec(p - center, self.rot.to_degrees());
        center + v + vec2(self.dx, self.dy)
    }

    /// The paper point of the pinned drawing that shows at `q`.
    pub fn invert(&self, q: Pos2, center: Pos2) -> Pos2 {
        center + rotate_vec(q - center - vec2(self.dx, self.dy), -self.rot.to_degrees())
    }
}

/// GM6: THE LIGHT TABLE — pinned (column, drawing) pairs shown on every
/// frame at the light table's opacity, plus the shift & trace offsets.
/// View state, per cut in effect (a pin of another cut's drawing does not
/// resolve here and waits); never document data.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LightTable {
    /// The first pin is the old single ghost pin.
    pub pins: Vec<(ColumnId, DrawingId)>,
    pub offsets: std::collections::HashMap<DrawingId, PinShift>,
    /// The pin a shift & trace drag moves (None or unpinned = the first).
    pub selected: Option<DrawingId>,
    /// The "shift & trace" latch: while on, a canvas drag moves the
    /// selected pin and the pen rests.
    pub shift_trace: bool,
}

impl LightTable {
    pub fn is_pinned(&self, d: DrawingId) -> bool {
        self.pins.iter().any(|(_, p)| *p == d)
    }

    /// Pin or unpin `d`; true = it is pinned now.
    pub fn toggle(&mut self, column: ColumnId, d: DrawingId) -> bool {
        if self.is_pinned(d) {
            self.unpin(d);
            false
        } else {
            self.pins.push((column, d));
            true
        }
    }

    pub fn unpin(&mut self, d: DrawingId) {
        self.pins.retain(|(_, p)| *p != d);
        self.offsets.remove(&d);
        if self.selected == Some(d) {
            self.selected = None;
        }
    }

    /// The first pin (the old ghost pin).
    pub fn first(&self) -> Option<DrawingId> {
        self.pins.first().map(|(_, d)| *d)
    }

    /// The pin shift & trace moves.
    pub fn target(&self) -> Option<DrawingId> {
        self.selected.filter(|d| self.is_pinned(*d)).or_else(|| self.first())
    }

    pub fn shift(&self, d: DrawingId) -> PinShift {
        self.offsets.get(&d).copied().unwrap_or_default()
    }

    /// Move (and turn) the target pin, view only.
    pub fn nudge(&mut self, dx: f32, dy: f32, rot: f32) {
        if let Some(d) = self.target() {
            let s = self.offsets.entry(d).or_default();
            s.dx += dx;
            s.dy += dy;
            s.rot += rot;
        }
    }

    /// Reset puts every pin back where it belongs.
    pub fn reset(&mut self) {
        self.offsets.clear();
    }

    /// The pins of drawings in `cut`, with their offsets, in pin order.
    pub fn shown(&self, cut: &anim_core::model::Cut) -> Vec<(DrawingId, PinShift)> {
        self.pins
            .iter()
            .filter(|(_, d)| cut.drawing(*d).is_some())
            .map(|(_, d)| (*d, self.shift(*d)))
            .collect()
    }
}

/// GM6: a pinned cel's tiles as shift & trace shows them — every texel
/// pulled from where `shift` says it came from (nearest texel), display
/// only. The identity shares the tiles untouched.
pub fn shift_tiles(
    tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    shift: PinShift,
    center: Pos2,
) -> BTreeMap<TileCoord, Arc<TileData>> {
    use anim_core::raster::{TILE, TILE_LEN};
    if shift.is_identity() || tiles.is_empty() {
        return tiles.clone();
    }
    let t = TILE as i32;
    // The shifted bounds of every source tile.
    let mut lo = pos2(f32::MAX, f32::MAX);
    let mut hi = pos2(f32::MIN, f32::MIN);
    for &(tx, ty) in tiles.keys() {
        for (cx, cy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let p = pos2(((tx + cx) * t) as f32, ((ty + cy) * t) as f32);
            let q = shift.apply(p, center);
            lo = lo.min(q);
            hi = hi.max(q);
        }
    }
    let (t0x, t0y) = ((lo.x.floor() as i32).div_euclid(t), (lo.y.floor() as i32).div_euclid(t));
    let (t1x, t1y) = ((hi.x.ceil() as i32).div_euclid(t), (hi.y.ceil() as i32).div_euclid(t));
    let mut out = BTreeMap::new();
    for ty in t0y..=t1y {
        for tx in t0x..=t1x {
            // A turned sparse cel leaves most of its bounds empty: a tile
            // whose corners map back to no source tile reads nothing, so it
            // is skipped whole (the map is affine — the corners bound it).
            let (mut slo, mut shi) = (pos2(f32::MAX, f32::MAX), pos2(f32::MIN, f32::MIN));
            for (cx, cy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let s = shift.invert(pos2(((tx + cx) * t) as f32, ((ty + cy) * t) as f32), center);
                slo = slo.min(s);
                shi = shi.max(s);
            }
            let (s0x, s0y) = ((slo.x.floor() as i32).div_euclid(t), (slo.y.floor() as i32).div_euclid(t));
            let (s1x, s1y) = ((shi.x.ceil() as i32).div_euclid(t), (shi.y.ceil() as i32).div_euclid(t));
            if !tiles
                .keys()
                .any(|&(x, y)| (s0x..=s1x).contains(&x) && (s0y..=s1y).contains(&y))
            {
                continue;
            }
            let mut px = vec![0u16; TILE_LEN];
            let mut any = false;
            // Neighbouring texels read the same source tile: look it up
            // once per change, not once per texel.
            let mut cached: (TileCoord, Option<&Arc<TileData>>) = ((i32::MIN, i32::MIN), None);
            for row in 0..TILE {
                for col in 0..TILE {
                    let q = pos2(
                        (tx * t + col as i32) as f32 + 0.5,
                        (ty * t + row as i32) as f32 + 0.5,
                    );
                    let s = shift.invert(q, center);
                    let (sx, sy) = (s.x.floor() as i32, s.y.floor() as i32);
                    let key = (sx.div_euclid(t), sy.div_euclid(t));
                    if cached.0 != key {
                        cached = (key, tiles.get(&key));
                    }
                    let Some(src) = cached.1 else {
                        continue;
                    };
                    let si = (sy.rem_euclid(t) * t + sx.rem_euclid(t)) as usize * 4;
                    let di = (row * TILE + col) * 4;
                    px[di..di + 4].copy_from_slice(&src.rgba[si..si + 4]);
                    any |= src.rgba[si + 3] != 0;
                }
            }
            if any {
                out.insert((tx, ty), Arc::new(TileData::from_vec(px)));
            }
        }
    }
    out
}

/// The onion's ghosts of `column` at `frame` for the vector path, each
/// with its tint, farthest first so the nearest lies on top. Ghost BEHIND
/// = Ao (continuity — where the motion came from); ghost AHEAD = Legend
/// (the plate stating what is next). Spec defect 14: the old red/green
/// pair is dead — Aka never sits on the paper for hours, and there is no
/// green in this application. The drawing shown here never ghosts, and on
/// a held frame the side behind starts past it ([`vector_onion_set`]).
pub(crate) fn onion_ghosts(
    state: &AppState,
    column: ColumnId,
    frame: u32,
    cfg: &OnionCfg,
) -> Vec<(anim_core::ids::DrawingId, Color32)> {
    let current = state.resolve_at(column, frame);
    let mut set: Vec<(DrawingId, i32, f32)> = vector_onion_set(state.cut(), column, frame, cfg)
        .into_iter()
        .filter(|(id, _, _)| Some(*id) != current)
        .collect();
    set.sort_by_key(|(_, offset, _)| std::cmp::Reverse(offset.unsigned_abs()));
    set.into_iter()
        .map(|(id, offset, weight)| {
            let base = if offset < 0 { plate::PENCIL_AO } else { plate::PENCIL_LEGEND };
            let alpha = (255.0 * weight.clamp(0.0, 1.0)) as u8;
            (id, ghost_tint([base.r(), base.g(), base.b()], alpha))
        })
        .collect()
}

/// GM6: the classic spacing-chart glyph (タイミング指示) in `rect`: a rule
/// from the key (top) to the next key (bottom), long ticks at both keys and
/// a short tick at each mark.
pub fn chart_glyph(painter: &egui::Painter, rect: Rect, marks: &[f32], ink: Color32) {
    let stroke = egui::Stroke::new(plate::device_px(1.0, painter.ctx().pixels_per_point()), ink);
    let x = rect.center().x;
    painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], stroke);
    let w = rect.width() * 0.5;
    for (t, half) in [(0.0, w), (1.0, w)]
        .into_iter()
        .chain(marks.iter().map(|m| (m.clamp(0.0, 1.0), w * 0.6)))
    {
        let y = rect.top() + rect.height() * t;
        painter.line_segment([pos2(x - half, y), pos2(x + half, y)], stroke);
    }
}

/// The onion's raster tints, unchanged since spec defect 14: Ao's paper
/// wash behind (the light table's pins wear it too), Legend's ahead. Aka
/// is never an onion tint (D7 keeps it for refusals).
pub const ONION_BEHIND_TINT: [u8; 3] = [160, 195, 232];
pub const ONION_AHEAD_TINT: [u8; 3] = [205, 206, 199];

/// A ghost's colour: `rgb` at `alpha`, unmultiplied.
fn ghost_tint(rgb: [u8; 3], alpha: u8) -> Color32 {
    plate::chip([rgb[0], rgb[1], rgb[2], alpha])
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
        let color = tint.unwrap_or_else(|| plate::chip([c[0], c[1], c[2], c[3]]));
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

/// GM4: the canvas view and the pen aids, headless (no window, no GPU:
/// the harness canvas runs without a paint layer, so strokes take the
/// vector path and the tap records them).
#[cfg(test)]
mod viewnav_tests {
    use super::*;
    use crate::config::{Keymap, clashes, default_binds};
    use crate::testkit::{CTRL, EditorHarness};
    use egui::{Key, Modifiers};

    fn xf(rot: f32, mirror: bool, zoom: f32) -> ViewXform {
        ViewXform {
            zoom,
            pan: vec2(13.0, -7.5),
            rotation_deg: rot,
            mirror,
            center: pos2(640.0, 400.0),
            paper: vec2(1920.0, 1080.0),
        }
    }

    fn harness() -> EditorHarness {
        let mut h = EditorHarness::new();
        h.run();
        h
    }

    /// A gently curving paper path near the paper's centre.
    fn paper_path() -> Vec<Pos2> {
        (0..9)
            .map(|k| {
                let k = k as f32;
                pos2(820.0 + k * 40.0, 520.0 + (k * 0.7).sin() * 50.0)
            })
            .collect()
    }

    /// Drag `paper` through the harness under the view as set, and return
    /// the stroke's tap.
    fn tap_for(rot: f32, mirror: bool, stabilizer: u8) -> (Vec<StrokeSample>, ViewXform) {
        let mut h = harness();
        h.rig_mut().config.pen.stabilizer = stabilizer;
        h.editor_mut().canvas.set_view_rotation(rot);
        if mirror {
            h.editor_mut().canvas.toggle_mirror();
        }
        h.run();
        let xf = h.editor().canvas.view_xform();
        let screen: Vec<Pos2> = paper_path().iter().map(|p| xf.paper_to_screen(*p)).collect();
        h.drag_path(&screen);
        (h.editor().canvas.stroke_tap.clone(), xf)
    }

    fn strokes_on_frame(h: &EditorHarness) -> usize {
        h.state().cut().drawings.iter().map(|d| d.strokes.len()).sum()
    }

    #[test]
    fn viewnav_screen_paper_round_trip() {
        let pts = [pos2(0.0, 0.0), pos2(1920.0, 1080.0), pos2(333.3, 812.9), pos2(960.0, 540.0)];
        for rot in [0.0, 15.0, 90.0, 137.0] {
            for mirror in [false, true] {
                for zoom in [0.25, 1.0, 3.7] {
                    let v = xf(rot, mirror, zoom);
                    for p in pts {
                        let back = v.screen_to_paper(v.paper_to_screen(p));
                        assert!(
                            (back - p).length() < 1e-3,
                            "rot {rot} mirror {mirror} zoom {zoom}: {p:?} came back {back:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn viewnav_rotated_view_taps_the_same_paper_path() {
        let (upright, _) = tap_for(0.0, false, 0);
        let (turned, xf90) = tap_for(90.0, false, 0);
        assert_eq!(xf90.rotation_deg, 90.0);
        assert!(upright.len() >= 5, "the stroke was tapped: {}", upright.len());
        assert_eq!(upright.len(), turned.len());
        for (a, b) in upright.iter().zip(&turned) {
            let d = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
            assert!(d <= 0.5, "{a:?} vs {b:?}: {d} px apart");
        }
    }

    #[test]
    fn viewnav_mirror_changes_only_the_display() {
        let (plain, xf0) = tap_for(0.0, false, 0);
        let (mirrored, xfm) = tap_for(0.0, true, 0);
        assert!(xfm.mirror && !xf0.mirror);
        // The display differs: an off-centre paper point lands elsewhere.
        let p = pos2(200.0, 300.0);
        assert!((xf0.paper_to_screen(p) - xfm.paper_to_screen(p)).length() > 10.0);
        // The paper receives the same samples.
        assert_eq!(plain.len(), mirrored.len());
        for (a, b) in plain.iter().zip(&mirrored) {
            assert!((a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3, "{a:?} vs {b:?}");
            assert_eq!((a.pressure, a.tilt), (b.pressure, b.tilt));
        }
    }

    #[test]
    fn viewnav_zoom_keys_walk_the_ladder_100_and_fit() {
        let mut h = harness();
        let fit = h.editor().canvas.abs_zoom();
        h.key_mods(CTRL, Key::Equals);
        let z1 = h.editor().canvas.abs_zoom();
        assert_eq!(z1, ladder_step(&ZOOM_LADDER, fit, true));
        assert!(ZOOM_LADDER.contains(&z1), "{z1} is a ladder stop");
        h.key_mods(CTRL, Key::Equals);
        let z2 = h.editor().canvas.abs_zoom();
        assert_eq!(z2, ladder_step(&ZOOM_LADDER, z1, true));
        h.key_mods(CTRL, Key::Minus);
        assert_eq!(h.editor().canvas.abs_zoom(), z1);
        // 100%: one paper px is one screen point.
        h.key_mods(CTRL, Key::Num1);
        let v = h.editor().canvas.view_xform();
        assert!((v.zoom - 1.0).abs() < 1e-5, "100% is zoom 1, got {}", v.zoom);
        let step = v.paper_to_screen(pos2(101.0, 100.0)) - v.paper_to_screen(pos2(100.0, 100.0));
        assert!((step.length() - 1.0).abs() < 1e-4);
        // Fit centres the paper in the pane at the fit scale.
        h.key_mods(CTRL, Key::Num0);
        let v = h.editor().canvas.view_xform();
        let centre = v.paper_to_screen(pos2(960.0, 540.0));
        assert!((centre - h.editor().canvas.dbg_rect.center()).length() < 1e-3);
        assert!((v.zoom - fit).abs() < 1e-5);
        // The ladder's ends hold.
        assert_eq!(ladder_step(&ZOOM_LADDER, 16.0, true), 16.0);
        assert_eq!(ladder_step(&ZOOM_LADDER, 0.125, false), 0.125);
        let fired: Vec<Action> = h.performed().iter().map(|(a, _)| *a).collect();
        for a in [Action::ZoomIn, Action::ZoomOut, Action::Zoom100, Action::ZoomFit] {
            assert!(fired.contains(&a), "{a:?} fired from its chord");
        }
    }

    #[test]
    fn viewnav_space_drag_pans_without_ink_or_playback() {
        let mut h = harness();
        let strokes0 = strokes_on_frame(&h);
        let pan0 = h.editor().canvas.view_xform().pan;
        let c = h.editor().canvas.dbg_rect.center();
        h.hold_key(Key::Space);
        h.drag_path(&[c, c + vec2(30.0, 0.0), c + vec2(60.0, 20.0), c + vec2(90.0, 40.0)]);
        let pan1 = h.editor().canvas.view_xform().pan;
        assert!((pan1 - pan0).length() > 30.0, "the hand panned: {pan0:?} → {pan1:?}");
        assert!(h.editor().canvas.stroke_tap.is_empty(), "the hand inks nothing");
        assert_eq!(strokes_on_frame(&h), strokes0);
        assert!(!h.state().view.playing);
        h.release_key(Key::Space);
        assert!(!h.state().view.playing, "a dragged hold is not a tap");
        assert!(!h.performed().iter().any(|(a, _)| *a == Action::PlayPause));
        // Releasing Space hands the pointer back to the brush.
        h.drag_path(&[c, c + vec2(30.0, 0.0), c + vec2(60.0, 10.0)]);
        assert!(!h.editor().canvas.stroke_tap.is_empty(), "the brush is back");
        assert_eq!(strokes_on_frame(&h), strokes0 + 1);
    }

    #[test]
    fn viewnav_space_tap_plays_once_and_r_still_flips() {
        let mut h = harness();
        h.key(Key::Space);
        let plays = h.performed().iter().filter(|(a, _)| *a == Action::PlayPause).count();
        assert_eq!(plays, 1, "one tap, one PlayPause");
        assert!(h.state().view.playing);
        h.key(Key::Space);
        assert!(!h.state().view.playing);
        h.hold_key(Key::R);
        h.run();
        assert!(h.editor().canvas.flip_held, "holding R still flips");
        h.release_key(Key::R);
        assert!(!h.editor().canvas.flip_held);
    }

    #[test]
    fn viewnav_brackets_walk_the_size_ladder() {
        let mut h = harness();
        h.editor_mut().canvas.raster_brush_px = 14.0;
        h.key(Key::CloseBracket);
        assert_eq!(h.editor().canvas.brush_size(), 16.0);
        h.key(Key::CloseBracket);
        assert_eq!(h.editor().canvas.brush_size(), 20.0);
        h.key(Key::OpenBracket);
        h.key(Key::OpenBracket);
        h.key(Key::OpenBracket);
        assert_eq!(h.editor().canvas.brush_size(), 12.0);
        // Off the ladder, a step lands on the next stop; the ends hold.
        h.editor_mut().canvas.raster_brush_px = 15.0;
        h.key(Key::CloseBracket);
        assert_eq!(h.editor().canvas.brush_size(), 16.0);
        h.editor_mut().canvas.raster_brush_px = 300.0;
        h.key(Key::CloseBracket);
        assert_eq!(h.editor().canvas.brush_size(), 300.0);
        h.editor_mut().canvas.raster_brush_px = 1.0;
        h.key(Key::OpenBracket);
        assert_eq!(h.editor().canvas.brush_size(), 1.0);
    }

    #[test]
    fn viewnav_hardness_and_aliased_persist_in_the_preset() {
        let mut c = CanvasView::new();
        assert_eq!(c.brush_hardness, crate::config::default_hardness());
        c.brush_hardness = 0.4;
        c.brush_aliased = true;
        let p = c.snapshot_preset("pixel".into());
        assert_eq!((p.hardness, p.aliased), (0.4, true));
        let json = serde_json::to_string(&p).unwrap();
        let back: BrushPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        let mut d = CanvasView::new();
        d.apply_preset(&back);
        assert_eq!((d.brush_hardness, d.brush_aliased), (0.4, true));
        // A preset saved before GM4 reads back with the old edge.
        let mut old = serde_json::to_value(&BrushPreset::default()).unwrap();
        old.as_object_mut().unwrap().remove("hardness");
        old.as_object_mut().unwrap().remove("aliased");
        let old: BrushPreset = serde_json::from_value(old).unwrap();
        assert_eq!((old.hardness, old.aliased), (0.85, false));
    }

    #[test]
    fn viewnav_stabilizer_tail_lands_on_pen_up_in_the_canvas() {
        let (tap, xf) = tap_for(0.0, false, 5);
        let (raw, _) = tap_for(0.0, false, 0);
        let pen_up = paper_path().last().copied().unwrap();
        let last = tap.last().expect("tapped");
        let d = ((last.x - pen_up.x).powi(2) + (last.y - pen_up.y).powi(2)).sqrt();
        assert!(d < 1e-2, "the tail caught up to pen-up: {d} px off");
        assert_ne!(tap.len(), raw.len(), "level 5 is not the raw pen");
        assert_eq!(xf.rotation_deg, 0.0);
    }

    #[test]
    fn viewnav_shift_click_rules_a_straight_line() {
        let mut h = harness();
        let c = h.editor().canvas.dbg_rect.center();
        h.drag_path(&[c, c + vec2(20.0, 0.0), c + vec2(40.0, 10.0), c + vec2(60.0, 20.0)]);
        let strokes = strokes_on_frame(&h);
        let anchor = h.editor().canvas.line_anchor.expect("the stroke left an anchor");
        let xf = h.editor().canvas.view_xform();
        let click = c + vec2(-90.0, 70.0);
        h.set_mods(Modifiers::SHIFT);
        h.click_pos(click);
        h.set_mods(Modifiers::NONE);
        let end = xf.screen_to_paper(click);
        let tap = h.editor().canvas.stroke_tap.clone();
        assert!(tap.len() >= 10, "the line was tapped: {} samples", tap.len());
        let seg = end - anchor;
        for s in &tap {
            let p = pos2(s.x, s.y);
            let t = ((p - anchor).dot(seg) / seg.length_sq()).clamp(0.0, 1.0);
            let off = (p - (anchor + seg * t)).length();
            assert!(off <= 0.5, "{p:?} is {off} px off the segment");
        }
        let (first, last) = (tap[0], tap[tap.len() - 1]);
        assert!((pos2(first.x, first.y) - anchor).length() < 1e-3);
        assert!((pos2(last.x, last.y) - end).length() < 1e-3);
        assert_eq!(strokes_on_frame(&h), strokes + 1, "the line committed as a stroke");
    }

    #[test]
    fn viewnav_eyedropper_picks_the_rendered_pixel() {
        use anim_core::raster::{TILE, TILE_LEN, f32_to_f16_bits};
        let mut h = harness();
        let slot = h.state().active_slot_resolved().expect("an armed layer");
        let rgba = [0.25f32, 0.5, 0.125, 1.0];
        let mut px = vec![0u16; TILE_LEN];
        for i in 0..TILE * TILE {
            for ch in 0..4 {
                px[i * 4 + ch] = f32_to_f16_bits(rgba[ch]);
            }
        }
        h.state_mut()
            .commit_raster(vec![((15, 8), Arc::new(TileData::from_vec(px)))], slot)
            .expect("the tile committed");
        let (x, y) = (970u32, 530u32);
        let (w, ht) = (h.state().engine.project.width, h.state().engine.project.height);
        let frame = anim_core::export::render_frame(h.state().cut(), h.state().view.frame, w, ht);
        let i = ((y * w + x) * 4) as usize;
        let want = [frame[i], frame[i + 1], frame[i + 2], frame[i + 3]];
        assert_eq!(want[3], 255, "the known pixel is inked");
        let got = pick_color(h.state(), x as f32 + 0.5, y as f32 + 0.5, false);
        assert_eq!(got, Some(want));
        assert_eq!(pick_color(h.state(), 5.5, 5.5, false), None, "bare paper picks nothing");
        // Alt+click with the brush puts that colour in the brush — as the
        // chip that paints it (the stored pixel is linear), the brush's
        // alpha kept; repainting the chip lays the stored colour back.
        h.editor_mut().canvas.brush_color = [1, 2, 3, 255];
        h.run();
        let s = h.editor().canvas.view_xform().paper_to_screen(pos2(x as f32 + 0.5, y as f32 + 0.5));
        h.set_mods(Modifiers::ALT);
        h.click_pos(s);
        h.set_mods(Modifiers::NONE);
        let chip = pick_chip(h.state(), x as f32 + 0.5, y as f32 + 0.5, false).expect("inked");
        assert_eq!(h.editor().canvas.brush_color, [chip[0], chip[1], chip[2], 255]);
        let back = linear_rgba(h.editor().canvas.brush_color);
        for c in 0..3 {
            assert!((back[c] - rgba[c]).abs() < 4e-3, "channel {c}: {} vs {}", back[c], rgba[c]);
        }
        assert!(h.editor().canvas.stroke_tap.is_empty(), "the pick inks nothing");
    }

    #[test]
    fn viewnav_new_verbs_are_actions_with_free_chords() {
        let verbs = [
            Action::ZoomIn,
            Action::ZoomOut,
            Action::Zoom100,
            Action::RotateViewLeft,
            Action::RotateViewRight,
            Action::ResetRotation,
            Action::MirrorView,
            Action::BrushSmaller,
            Action::BrushBigger,
        ];
        for km in [Keymap::Gaban, Keymap::Industry] {
            let binds = default_binds(km);
            assert_eq!(clashes(&binds), vec![], "{km:?} has no chord conflict");
            for a in verbs {
                assert!(Action::ALL.contains(&a));
                assert!((1..=120).contains(&a.help().chars().count()), "{a:?} has help");
                let b = binds.iter().find(|b| b.action == a).expect("bound");
                let c = b.chord.as_ref().unwrap_or_else(|| panic!("{a:?} has a chord in {km:?}"));
                for k in &c.keys {
                    assert!(egui::Key::from_name(k).is_some(), "{k} is a key");
                    assert_ne!(k, "R", "R stays the flip");
                }
            }
        }
    }

    #[test]
    fn viewnav_keyed_rotate_mirror_and_tilt_turn_with_the_view() {
        let mut h = harness();
        h.key_mods(CTRL, Key::CloseBracket);
        h.key_mods(CTRL, Key::CloseBracket);
        assert_eq!(h.editor().canvas.view_rotation(), 30.0);
        h.key_mods(CTRL, Key::OpenBracket);
        assert_eq!(h.editor().canvas.view_rotation(), 15.0);
        // The pane centre's paper point holds still while the view turns.
        let v = h.editor().canvas.view_xform();
        let mid = v.screen_to_paper(h.editor().canvas.dbg_rect.center());
        h.key_mods(CTRL, Key::Backslash);
        assert_eq!(h.editor().canvas.view_rotation(), 0.0);
        let v0 = h.editor().canvas.view_xform();
        assert!((v0.screen_to_paper(h.editor().canvas.dbg_rect.center()) - mid).length() < 1e-2);
        h.key(Key::H);
        assert!(h.editor().canvas.view_mirrored());
        // A screen tilt reaches the paper turned back by the view angle.
        for rot in [0.0, 15.0, 90.0, 137.0] {
            let v = xf(rot, false, 1.0);
            let t = v.dir_to_paper([0.6, 0.0]);
            let back = rotate_vec(vec2(t[0], t[1]), rot);
            assert!((back - vec2(0.6, 0.0)).length() < 1e-5);
        }
        let t = xf(90.0, false, 1.0).dir_to_paper([1.0, 0.0]);
        assert!((t[0] - 0.0).abs() < 1e-5 && (t[1] + 1.0).abs() < 1e-5, "{t:?}");
    }

    #[test]
    fn viewnav_paper_image_is_one_quad_at_the_turned_corners() {
        let v = xf(90.0, true, 0.5);
        let mesh = paper_image_mesh(&v, egui::TextureId::default(), plate::PAPER);
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.indices, vec![0, 1, 2, 0, 2, 3]);
        let corners = v.paper_corners();
        let uvs = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
        for (k, vert) in mesh.vertices.iter().enumerate() {
            assert_eq!(vert.pos, corners[k]);
            assert_eq!(vert.uv, uvs[k]);
        }
        // Turned 90°, the paper's screen box stands on its short side.
        let b = v.paper_bounds();
        assert!((b.width() - 540.0).abs() < 1e-2 && (b.height() - 960.0).abs() < 1e-2);
    }

    /// Commit one solid tile of premultiplied linear `rgba` at `coord` on
    /// the armed layer.
    fn ink_tile(h: &mut EditorHarness, coord: (i32, i32), rgba: [f32; 4]) {
        use anim_core::raster::{TILE, TILE_LEN, f32_to_f16_bits};
        let slot = h.state().active_slot_resolved().expect("an armed layer");
        let mut px = vec![0u16; TILE_LEN];
        for i in 0..TILE * TILE {
            for ch in 0..4 {
                px[i * 4 + ch] = f32_to_f16_bits(rgba[ch]);
            }
        }
        h.state_mut()
            .commit_raster(vec![(coord, Arc::new(TileData::from_vec(px)))], slot)
            .expect("the tile committed");
    }

    /// The armed layer's stored texel at paper (x, y), premultiplied.
    fn texel(h: &EditorHarness, x: i32, y: i32) -> [f32; 4] {
        use anim_core::raster::{TILE, f16_bits_to_f32};
        let t = TILE as i32;
        let Some(tile) = h
            .state()
            .active_layer_tiles()
            .and_then(|m| m.get(&(x.div_euclid(t), y.div_euclid(t))))
        else {
            return [0.0; 4];
        };
        let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4) as usize;
        std::array::from_fn(|c| f16_bits_to_f32(tile.rgba[i + c]))
    }

    /// Alt+click the paper point `p` through the view as it stands.
    fn alt_click(h: &mut EditorHarness, p: Pos2) {
        let s = h.editor().canvas.view_xform().paper_to_screen(p);
        h.set_mods(Modifiers::ALT);
        h.click_pos(s);
        h.set_mods(Modifiers::NONE);
    }

    /// One solid tile inked at paper (960..1024, 512..576), the view at
    /// 200 % turned `rot` (and mirrored), the Select tool armed and the
    /// layer lifted into a floating transform (the CPU half of a lift —
    /// the harness has no GPU to preview with).
    fn lifted(rot: f32, mirror: bool) -> EditorHarness {
        let mut h = harness();
        ink_tile(&mut h, (15, 8), [0.25, 0.5, 0.125, 1.0]);
        {
            let c = &mut h.editor_mut().canvas;
            c.set_abs_zoom(2.0);
            c.set_view_rotation(rot);
            if mirror {
                c.toggle_mirror();
            }
        }
        let ctx = h.ctx();
        let ed = h.editor_mut();
        ed.canvas.set_tool(CanvasTool::Select, &mut ed.state);
        assert!(ed.canvas.lift_floating(&mut ed.state, &ctx, None).is_some(), "lifted");
        h.run();
        h
    }

    #[test]
    fn viewnav_lifted_selection_moves_by_the_paper_delta_turned_and_mirrored() {
        let mut h = lifted(90.0, true);
        let xf = h.editor().canvas.view_xform();
        let s = |p: Pos2| xf.paper_to_screen(p);
        // Press beside the patch's middle; the drag takes hold there and
        // carries it 100 px right, 40 px down on the paper.
        let grab = pos2(992.0, 544.0);
        let to = grab + vec2(100.0, 40.0);
        let mut path = vec![s(grab) + vec2(-10.0, 0.0), s(grab)];
        for k in 1..=4 {
            path.push(s(grab.lerp(to, k as f32 / 4.0)));
        }
        h.drag_path(&path);
        let t = h.editor().canvas.floating.as_ref().expect("still lifted").translate;
        assert!((t - vec2(100.0, 40.0)).length() < 0.05, "moved by the paper delta: {t:?}");
        let ed = h.editor_mut();
        ed.canvas.finish_gesture(&mut ed.state);
        h.run();
        // The committed patch sits at (1060..1124, 552..616).
        assert!(texel(&h, 1092, 584)[3] > 0.99, "the patch landed under the pen");
        assert!(texel(&h, 1061, 553)[3] > 0.99 && texel(&h, 1122, 614)[3] > 0.99);
        assert!(texel(&h, 1057, 584)[3] < 0.01 && texel(&h, 1092, 549)[3] < 0.01);
        assert!(texel(&h, 992, 544)[3] < 0.01, "lifted off its old place");
    }

    #[test]
    fn viewnav_lifted_selection_turns_the_paper_way_under_a_mirror() {
        let mut h = lifted(90.0, true);
        let xf = h.editor().canvas.view_xform();
        let (pivot, corner) = {
            let f = h.editor().canvas.floating.as_ref().expect("lifted");
            (f.pivot + f.translate, f.corners()[1])
        };
        // Just outside the top-right corner is the turn handle; sweep it
        // +30° about the pivot on the paper.
        let arm = (corner - pivot) * 1.25;
        let at = |deg: f32| xf.paper_to_screen(pivot + rotate_vec(arm, deg));
        let d = (at(0.0) - xf.paper_to_screen(corner)).length();
        assert!(d > 12.0 && d <= 28.0, "the grab is on the turn handle: {d}");
        h.drag_path(&[at(-6.0), at(0.0), at(10.0), at(20.0), at(30.0)]);
        let r = h.editor().canvas.floating.as_ref().expect("lifted").rotate.to_degrees();
        assert!((r - 30.0).abs() < 0.1, "turned +30° on the paper, got {r}");
    }

    #[test]
    fn viewnav_space_hand_pans_over_a_lifted_selection() {
        let mut h = lifted(0.0, false);
        let pan0 = h.editor().canvas.view_xform().pan;
        // Grab the lifted patch itself: the hand, not the move, takes it.
        let c = h.editor().canvas.view_xform().paper_to_screen(pos2(992.0, 544.0));
        h.hold_key(Key::Space);
        h.drag_path(&[c, c + vec2(30.0, 0.0), c + vec2(60.0, 20.0)]);
        h.release_key(Key::Space);
        let pan1 = h.editor().canvas.view_xform().pan;
        assert!((pan1 - pan0).length() > 30.0, "the hand panned: {pan0:?} → {pan1:?}");
        let f = h.editor().canvas.floating.as_ref().expect("still lifted");
        assert_eq!(f.translate, vec2(0.0, 0.0), "the hand moved the view, not the ink");
        assert!(!h.performed().iter().any(|(a, _)| *a == Action::PlayPause));
    }

    #[test]
    fn viewnav_space_hold_that_zooms_is_not_a_tap() {
        let mut h = harness();
        h.hold_key(Key::Space);
        h.key_mods(CTRL, Key::Equals);
        h.run();
        h.release_key(Key::Space);
        let fired: Vec<Action> = h.performed().iter().map(|(a, _)| *a).collect();
        assert!(fired.contains(&Action::ZoomIn), "the zoom fired");
        assert!(!fired.contains(&Action::PlayPause), "a hold that moved the view is no tap");
        assert!(!h.state().view.playing);
    }

    #[test]
    fn viewnav_shift_space_drag_turns_and_ctrl_snaps_to_15() {
        let mut h = harness();
        let c = h.editor().canvas.dbg_rect.center();
        let arc = |degs: &[f32]| -> Vec<Pos2> {
            degs.iter()
                .map(|d| {
                    let a = d.to_radians();
                    c + vec2(a.cos(), a.sin()) * 150.0
                })
                .collect()
        };
        // The turn takes hold at the second point (5°) and follows to 45°.
        h.hold_key(Key::Space);
        h.set_mods(Modifiers::SHIFT);
        h.drag_path(&arc(&[0.0, 5.0, 15.0, 25.0, 35.0, 45.0]));
        h.set_mods(Modifiers::NONE);
        h.release_key(Key::Space);
        let turned = h.editor().canvas.view_rotation();
        assert!((turned - 40.0).abs() < 0.5, "Shift+Space+drag turned 40°, got {turned}");
        assert!(h.editor().canvas.stroke_tap.is_empty(), "the hand inks nothing");
        // With Ctrl the turn snaps to 15° steps.
        h.hold_key(Key::Space);
        h.set_mods(Modifiers { shift: true, ..CTRL });
        h.drag_path(&arc(&[0.0, 5.0, 14.0, 23.0, 32.0]));
        h.set_mods(Modifiers::NONE);
        h.release_key(Key::Space);
        let snapped = h.editor().canvas.view_rotation();
        assert_ne!(snapped, turned, "the snapped drag turned too");
        assert!((snapped / ROTATE_STEP_DEG - (snapped / ROTATE_STEP_DEG).round()).abs() < 1e-4);
        assert!(!h.performed().iter().any(|(a, _)| *a == Action::PlayPause));
    }

    #[test]
    fn viewnav_line_anchor_retires_off_its_frame_and_on_undo() {
        let mut h = harness();
        let c = h.editor().canvas.dbg_rect.center();
        let stroke = [c, c + vec2(20.0, 0.0), c + vec2(40.0, 10.0), c + vec2(60.0, 20.0)];
        let shift_click = |h: &mut EditorHarness| {
            h.set_mods(Modifiers::SHIFT);
            h.click_pos(c + vec2(-90.0, 70.0));
            h.set_mods(Modifiers::NONE);
        };
        h.drag_path(&stroke);
        assert!(h.editor().canvas.line_anchor.is_some(), "the stroke left an anchor");
        // On the next frame the last stroke is not under the pen.
        h.perform(Action::NextFrame);
        let strokes = strokes_on_frame(&h);
        shift_click(&mut h);
        assert_eq!(strokes_on_frame(&h), strokes, "no line from another frame's stroke");
        assert!(h.editor().canvas.line_anchor.is_none());
        // An undone stroke takes its anchor with it.
        h.perform(Action::PrevFrame);
        h.drag_path(&stroke);
        assert!(h.editor().canvas.line_anchor.is_some());
        h.perform(Action::Undo);
        let strokes = strokes_on_frame(&h);
        shift_click(&mut h);
        assert_eq!(strokes_on_frame(&h), strokes, "no line from an undone stroke");
        // A tool switch drops it too.
        h.drag_path(&stroke);
        let ed = h.editor_mut();
        ed.canvas.set_tool(CanvasTool::LassoFill, &mut ed.state);
        ed.canvas.set_tool(CanvasTool::Paint, &mut ed.state);
        assert!(h.editor().canvas.line_anchor.is_none());
    }

    #[test]
    fn viewnav_fit_holds_the_turned_paper_in_the_pane() {
        let mut h = harness();
        h.editor_mut().canvas.set_view_rotation(45.0);
        h.key_mods(CTRL, Key::Num0);
        let canvas = &h.editor().canvas;
        assert_eq!(canvas.view_rotation(), 45.0, "Fit keeps the turn");
        let pane = canvas.dbg_rect;
        let b = canvas.view_xform().paper_bounds();
        assert!(pane.expand(1e-2).contains_rect(b), "{b:?} inside {pane:?}");
        assert!(
            b.width() >= pane.width() * 0.9 || b.height() >= pane.height() * 0.9,
            "fitted, not shrunk: {b:?} in {pane:?}"
        );
        assert!((b.center() - pane.center()).length() < 1e-2, "centred");
    }

    #[test]
    fn viewnav_eyedropper_brings_back_the_painted_chip_turned_and_mirrored() {
        let mut h = harness();
        h.editor_mut().canvas.set_view_rotation(90.0);
        h.editor_mut().canvas.toggle_mirror();
        h.run();
        let p = pos2(970.5, 530.5);
        // Darks included: an 8-bit linear read would lose them (32 → 34).
        for chip in [[32u8, 64, 200], [1, 2, 3], [10, 20, 30], [128, 128, 128], [255, 128, 0]] {
            ink_tile(&mut h, (15, 8), linear_rgba([chip[0], chip[1], chip[2], 255]));
            h.editor_mut().canvas.brush_color = [9, 9, 9, 200];
            h.run();
            alt_click(&mut h, p);
            let got = h.editor().canvas.brush_color;
            for c in 0..3 {
                assert!(got[c].abs_diff(chip[c]) <= 1, "{chip:?} came back {got:?}");
            }
            assert_eq!(got[3], 200, "the brush keeps its alpha");
        }
        // A half-covered pixel: its colour comes back, its coverage does not.
        let lin = linear_rgba([200, 100, 50, 255]);
        ink_tile(&mut h, (15, 8), [lin[0] * 0.5, lin[1] * 0.5, lin[2] * 0.5, 0.5]);
        h.editor_mut().canvas.brush_color = [9, 9, 9, 255];
        h.run();
        alt_click(&mut h, p);
        let got = h.editor().canvas.brush_color;
        for (c, want) in [200u8, 100, 50].into_iter().enumerate() {
            assert!(got[c].abs_diff(want) <= 1, "half alpha came back {got:?}");
        }
        assert_eq!(got[3], 255, "the brush keeps its alpha");
    }

    #[test]
    fn viewnav_lasso_fill_lands_the_same_turned_and_mirrored() {
        // Edges vertical, level, or 4 px across per px down, so every
        // scanline crossing sits on a half pixel — the fill cannot flip a
        // pixel on the view's rounding.
        let loop_paper = [
            pos2(800.5, 500.25),
            pos2(1000.5, 500.25),
            pos2(1200.5, 550.25),
            pos2(1200.5, 650.25),
            pos2(800.5, 650.25),
        ];
        let fill = |rot: f32, mirror: bool| -> EditorHarness {
            let mut h = harness();
            h.editor_mut().canvas.set_view_rotation(rot);
            if mirror {
                h.editor_mut().canvas.toggle_mirror();
            }
            let ed = h.editor_mut();
            ed.canvas.set_tool(CanvasTool::LassoFill, &mut ed.state);
            h.run();
            let xf = h.editor().canvas.view_xform();
            let mut path = vec![xf.paper_to_screen(loop_paper[0]) + vec2(-10.0, 0.0)];
            path.extend(loop_paper.iter().map(|p| xf.paper_to_screen(*p)));
            h.drag_path(&path);
            h
        };
        let upright = fill(0.0, false);
        let turned = fill(90.0, true);
        assert!(texel(&upright, 900, 600)[3] > 0.99, "the loop filled");
        assert!(texel(&upright, 790, 600)[3] < 0.01, "and only the loop");
        for y in (480..680).step_by(3) {
            for x in (780..1220).step_by(3) {
                let (a, b) = (texel(&upright, x, y), texel(&turned, x, y));
                for c in 0..4 {
                    assert!((a[c] - b[c]).abs() < 1e-3, "({x}, {y}): {a:?} vs {b:?}");
                }
            }
        }
    }

    #[test]
    fn viewnav_fill_tool_seeds_under_the_pen_turned_and_mirrored() {
        use anim_core::raster::{TILE, TILE_LEN, f16_bits_to_f32, f32_to_f16_bits};
        // A closed ink box, a 2 px rim round tile (20, 12) — paper
        // (1280..1344, 768..832), well off the paper's centre: a seed the
        // view put anywhere else lands on its rim or floods the paper.
        let mut state = AppState::new_project("fill", 1920, 1080, 24, 300.0);
        state.new_drawing_at_frame();
        let slot = state.active_slot_resolved().expect("an armed layer");
        let mut px = vec![0u16; TILE_LEN];
        for y in 0..TILE {
            for x in 0..TILE {
                if x < 2 || y < 2 || x >= TILE - 2 || y >= TILE - 2 {
                    for ch in 0..4 {
                        px[(y * TILE + x) * 4 + ch] = f32_to_f16_bits(1.0);
                    }
                }
            }
        }
        state
            .commit_raster(vec![((20, 12), Arc::new(TileData::from_vec(px)))], slot)
            .expect("the box committed");
        // The fill tool's own input under a turned, mirrored, panned view
        // (its GPU gate set by hand: the harness has no paint layer).
        let xf = ViewXform {
            zoom: 0.5,
            pan: vec2(-40.0, 25.0),
            rotation_deg: 90.0,
            mirror: true,
            center: pos2(400.0, 300.0),
            paper: vec2(1920.0, 1080.0),
        };
        let mut h = egui_kittest::Harness::builder()
            .with_size(vec2(800.0, 600.0))
            .build_ui_state(
                move |ui, (canvas, state): &mut (CanvasView, AppState)| {
                    let rect = ui.max_rect();
                    let response = ui.allocate_rect(rect, Sense::click_and_drag());
                    canvas.raster = true;
                    canvas.fill_input(ui, &response, rect, &|s| xf.screen_to_paper(s), state);
                },
                (CanvasView::new(), state),
            );
        h.step();
        // Press on the box's middle; the fill seeds where the drag takes
        // hold, 8 screen px on (16 paper px) — still inside the rim.
        let at = xf.paper_to_screen(pos2(1312.0, 800.0));
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        h.input_mut().events.push(egui::Event::PointerMoved(at));
        h.input_mut().events.push(button(at, true));
        h.step();
        h.input_mut().events.push(egui::Event::PointerMoved(at + vec2(8.0, 0.0)));
        h.step();
        h.input_mut().events.push(button(at + vec2(8.0, 0.0), false));
        h.step();
        let (_, state) = h.state();
        let alpha = |x: i32, y: i32| -> f32 {
            let t = TILE as i32;
            state
                .active_layer_tiles()
                .and_then(|m| m.get(&(x.div_euclid(t), y.div_euclid(t))))
                .map(|tile| {
                    let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4 + 3) as usize;
                    f16_bits_to_f32(tile.rgba[i])
                })
                .unwrap_or(0.0)
        };
        assert!(
            alpha(1312, 800) > 0.99 && alpha(1284, 772) > 0.99,
            "the box filled: {}",
            state.status
        );
        assert!(alpha(1250, 800) < 0.01, "and not past its rim");
        assert!(alpha(500, 300) < 0.01, "the paper outside the box is bare");
    }

    #[test]
    fn viewnav_pen_tilt_reaches_the_paper_turned_back() {
        let mut h = harness();
        h.rig_mut().config.pen.stabilizer = 0;
        h.editor_mut().canvas.set_view_rotation(90.0);
        h.run();
        let xf = h.editor().canvas.view_xform();
        // The pen leans right on screen; turned 90°, that is paper-up.
        let tilt = [0.5f32, 0.0];
        let sample = |p: Pos2, phase| PenSample {
            pos: xf.paper_to_screen(p),
            pressure: Some(0.6),
            tilt: Some(tilt),
            phase,
        };
        let path = paper_path();
        let mut pen = vec![sample(path[0], PenPhase::Down)];
        pen.extend(path[1..].iter().map(|p| sample(*p, PenPhase::Move)));
        pen.push(sample(path[path.len() - 1], PenPhase::Up));
        h.rig_mut().pen = pen;
        h.run();
        let tap = h.editor().canvas.stroke_tap.clone();
        assert!(tap.len() >= 5, "the pen stroke was tapped: {}", tap.len());
        let want = xf.dir_to_paper(tilt);
        assert!(want[0].abs() < 1e-5 && (want[1] + 0.5).abs() < 1e-5, "{want:?}");
        for s in &tap {
            let off = (s.tilt[0] - want[0]).abs() + (s.tilt[1] - want[1]).abs();
            assert!(off < 1e-4, "tilt {:?} reached the paper, want {want:?}", s.tilt);
        }
        for (s, p) in tap.iter().zip(&path) {
            assert!((pos2(s.x, s.y) - *p).length() < 0.5, "{s:?} vs {p:?}");
        }
    }
}

/// GM10: the camera furniture the canvas draws, headless (the harness runs
/// the editor without a window or GPU and the canvas records what it drew).
#[cfg(test)]
mod camview_tests {
    use crate::config::Action;
    use crate::testkit::EditorHarness;
    use anim_core::camera::{self, CameraState, IRect};
    use anim_core::fx::FxValue;
    use anim_core::graph::NodeKind;

    fn harness() -> EditorHarness {
        let mut h = EditorHarness::new();
        h.run();
        h
    }

    fn frame_size(h: &EditorHarness) -> (u32, u32) {
        let p = &h.state().engine.project;
        (p.width, p.height)
    }

    #[test]
    fn camview_the_paper_outline_is_the_cuts_paper_and_undoes_to_none() {
        let mut h = harness();
        let (w, fh) = frame_size(&h);
        assert_eq!(h.editor().canvas.furniture.paper, None, "the frame is the paper: no outline");
        let wide = IRect { x: -540, y: 0, w: w + 1080, h: fh };
        h.state_mut().set_cut_paper(wide);
        h.run();
        assert_eq!(h.editor().canvas.furniture.paper, Some(wide), "the outline is the cut's paper");
        // A paper no renderer can take is refused and changes nothing.
        h.state_mut().set_cut_paper(IRect { x: 0, y: 0, w: 0, h: fh });
        h.run();
        assert_eq!(h.editor().canvas.furniture.paper, Some(wide));
        // Setting it was one undo step.
        h.perform(Action::Undo);
        h.run();
        assert_eq!(h.editor().canvas.furniture.paper, None);
        assert_eq!(camera::paper(h.state().cut(), w, fh), IRect::frame(w, fh));
    }

    #[test]
    fn camview_the_camera_rect_sits_on_the_paper_where_the_output_camera_looks() {
        let mut h = harness();
        let (w, fh) = frame_size(&h);
        assert_eq!(h.editor().canvas.furniture.camera, None, "no camera, no frame rect");
        let s = h.state_mut();
        s.graph_add_node(NodeKind::camera(), (200.0, 0.0));
        let cam = s.selected_node.expect("the new camera is selected");
        s.graph_set_output(cam);
        let Ok(NodeKind::Camera { planes, params, binds }) =
            s.cut().graph.node(cam).map(|n| n.kind.clone())
        else {
            panic!("a Camera node");
        };
        let mut params = params;
        params.set("pan_x", FxValue::Float(300.0));
        params.set("zoom", FxValue::Float(2.0));
        s.graph_set_kind(cam, NodeKind::Camera { planes, params, binds });
        h.run();
        let frame = h.state().view.frame;
        let st = CameraState::at(h.state().cut(), cam, frame).expect("the camera resolves");
        let drawn = h.editor().canvas.furniture.camera.expect("the camera's frame is drawn");
        assert_eq!(drawn, camera::frame_rect_on_paper(&st, w, fh));
        // Zoom 2 shows half the frame, centred 300 px right of the frame's centre.
        let (cx, cy) = (w as f32 / 2.0 + 300.0, fh as f32 / 2.0);
        let (hw, hh) = (w as f32 / 4.0, fh as f32 / 4.0);
        let want = [(cx - hw, cy - hh), (cx + hw, cy - hh), (cx + hw, cy + hh), (cx - hw, cy + hh)];
        for (d, e) in drawn.iter().zip(want) {
            assert!((d.0 - e.0).abs() < 1e-3 && (d.1 - e.1).abs() < 1e-3, "{drawn:?} vs {want:?}");
        }
    }

    #[test]
    fn camview_the_guide_toggles_draw_exactly_the_camera_module_rects() {
        let mut h = harness();
        let (w, fh) = frame_size(&h);
        let f = &h.editor().canvas.furniture;
        assert!(f.fields.is_empty() && f.safe.is_none(), "both toggles start off");
        h.editor_mut().canvas.show_field = true;
        h.run();
        let f = &h.editor().canvas.furniture;
        assert_eq!(f.fields, camera::field_guide(w, fh));
        assert_eq!(f.fields.len(), camera::FIELDS as usize);
        assert_eq!(f.safe, None, "the safe frames wait for their own toggle");
        h.editor_mut().canvas.show_field = false;
        h.editor_mut().canvas.show_safe = true;
        h.run();
        let f = &h.editor().canvas.furniture;
        assert!(f.fields.is_empty());
        assert_eq!(f.safe, Some(camera::safe_frames(w, fh)));
    }

    /// A Camera feeding the output, panned `pan_x` paper px at `zoom`.
    fn output_camera(h: &mut EditorHarness, pan_x: f32, zoom: f32) -> anim_core::ids::NodeId {
        let s = h.state_mut();
        s.graph_add_node(NodeKind::camera(), (200.0, 0.0));
        let cam = s.selected_node.expect("the new camera is selected");
        s.graph_set_output(cam);
        let Ok(NodeKind::Camera { planes, mut params, binds }) =
            s.cut().graph.node(cam).map(|n| n.kind.clone())
        else {
            panic!("a Camera node");
        };
        params.set("pan_x", FxValue::Float(pan_x));
        params.set("zoom", FxValue::Float(zoom));
        s.graph_set_kind(cam, NodeKind::Camera { planes, params, binds });
        h.run();
        cam
    }

    #[test]
    fn camview_the_field_guide_and_safe_frames_ride_the_camera_frame_on_the_paper() {
        let mut h = harness();
        let (w, fh) = frame_size(&h);
        h.editor_mut().canvas.show_field = true;
        h.editor_mut().canvas.show_safe = true;
        h.run();
        // No camera: the guides sit on the frame itself.
        let f = h.editor().canvas.furniture.clone();
        assert_eq!(f.view, None);
        assert_eq!(f.quad(&f.fields[0]), f.fields[0].corners());
        // A camera 300 px right at zoom 2 (T.U.) carries them along.
        let cam = output_camera(&mut h, 300.0, 2.0);
        let st = CameraState::at(h.state().cut(), cam, h.state().view.frame).expect("the camera resolves");
        let f = h.editor().canvas.furniture.clone();
        assert_eq!(f.view, Some(st.plane_map(0.0, w, fh)));
        // The 12th field is the camera's whole frame: the camera rect itself.
        let twelve = f.quad(f.fields.last().expect("12 fields"));
        assert_eq!(Some(twelve), f.camera);
        // Action safe is 93% of the half-size frame, centred 300 px right.
        let action = f.quad(&f.safe.expect("the safe frames").action);
        let (cx, cy) = (w as f32 / 2.0 + 300.0, fh as f32 / 2.0);
        let (hw, hh) = (w as f32 * 0.93 / 4.0, fh as f32 * 0.93 / 4.0);
        let want = [(cx - hw, cy - hh), (cx + hw, cy - hh), (cx + hw, cy + hh), (cx - hw, cy + hh)];
        for (d, e) in action.iter().zip(want) {
            assert!((d.0 - e.0).abs() < 1e-2 && (d.1 - e.1).abs() < 1e-2, "{action:?} vs {want:?}");
        }
        // The module rects themselves stay the frame's (criterion 9).
        assert_eq!(f.fields, camera::field_guide(w, fh));
    }

    #[test]
    fn camview_paper_size_is_a_verb_whose_window_sets_the_paper_in_one_step() {
        let mut h = harness();
        let (w, fh) = frame_size(&h);
        // D6: in the registry, the Cut menu and the palette.
        assert!(Action::ALL.contains(&Action::PaperSize));
        assert_eq!(Action::PaperSize.menu(), Some("Cut/Paper size…"));
        assert_eq!(crate::cmdpal::ranked("paper size", &[]).first(), Some(&Action::PaperSize));
        for word in ["hiki", "BG pan", "kami"] {
            assert!(crate::cmdpal::ranked(word, &[]).contains(&Action::PaperSize), "{word}");
        }
        // perform opens the window on the cut's paper.
        h.perform(Action::PaperSize);
        h.run();
        let form = h.editor().paper_form.as_ref().expect("the window is open");
        assert_eq!(form.rect, IRect::frame(w, fh));
        assert!(h.has("set paper"));
        // A preset fills the fields centred on the frame; "set paper" sets it.
        h.click("2× wide");
        h.run();
        let wide = IRect { x: -(w as i32) / 2, y: 0, w: 2 * w, h: fh };
        assert_eq!(h.editor().paper_form.as_ref().map(|f| f.rect), Some(wide));
        h.click("set paper");
        h.run();
        assert_eq!(camera::paper(h.state().cut(), w, fh), wide);
        assert_eq!(h.editor().canvas.furniture.paper, Some(wide));
        assert!(h.editor().paper_form.is_none(), "set closes the window");
        h.perform(Action::Undo);
        h.run();
        assert_eq!(camera::paper(h.state().cut(), w, fh), IRect::frame(w, fh), "one undo step");
        // A draft typed on one cut never lands on another: a cut switch
        // re-reads the new cut's paper.
        h.perform(Action::PaperSize);
        h.run();
        h.click("2× tall");
        h.run();
        let scene = h.state().view.scene;
        h.state_mut().new_cut(scene);
        h.run();
        let form = h.editor().paper_form.as_ref().expect("still open");
        assert_eq!(form.cut, h.state().at());
        assert_eq!(form.rect, IRect::frame(w, fh));
    }
}

/// GM20: painting on the extended paper — the tools and the canvas's layer
/// rect over a paper wider than the frame (3000×1080 at x −540).
#[cfg(test)]
pub(crate) mod extpaper_tests {
    use super::*;
    use crate::config::Action;
    use crate::measure::{effective, ratio};
    use crate::paint::{LayerRect, layer_rect, memory_text};
    use crate::testkit::EditorHarness;
    use anim_core::camera::IRect;
    use anim_core::raster::{TILE, TILE_LEN, TileData, f16_bits_to_f32, f32_to_f16_bits};
    use egui::Modifiers;
    use std::sync::Arc;

    const WIDE: IRect = IRect { x: -540, y: 0, w: 3000, h: 1080 };

    /// A project on the wide paper with an own cel at frame 0.
    fn wide_state() -> AppState {
        let mut state = AppState::new_project("ext", 1920, 1080, 24, 300.0);
        state.new_drawing_at_frame();
        state.set_cut_paper(WIDE);
        state
    }

    /// Commit opaque `rgba` texels at the given frame pixels to the armed layer.
    pub(crate) fn paint_texels(state: &mut AppState, texels: &[(i32, i32)], rgba: [f32; 4]) {
        let t = TILE as i32;
        let mut map: std::collections::BTreeMap<(i32, i32), Vec<u16>> = Default::default();
        if let Some(have) = state.active_layer_tiles() {
            for (c, tile) in have {
                map.insert(*c, tile.rgba.to_vec());
            }
        }
        for (x, y) in texels {
            let tile = map.entry((x.div_euclid(t), y.div_euclid(t))).or_insert_with(|| vec![0; TILE_LEN]);
            let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4) as usize;
            for c in 0..4 {
                tile[i + c] = f32_to_f16_bits(rgba[c]);
            }
        }
        let slot = state.active_slot_resolved().expect("an armed layer");
        let tiles = map.into_iter().map(|(k, v)| (k, Arc::new(TileData::from_vec(v)))).collect();
        state.commit_raster(tiles, slot).expect("committed");
    }

    fn ring(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        for x in x0..=x1 {
            v.extend([(x, y0), (x, y1)]);
        }
        for y in y0..=y1 {
            v.extend([(x0, y), (x1, y)]);
        }
        v
    }

    fn texel(state: &AppState, x: i32, y: i32) -> [f32; 4] {
        let t = TILE as i32;
        let Some(tile) = state.active_layer_tiles().and_then(|m| m.get(&(x.div_euclid(t), y.div_euclid(t)))) else {
            return [0.0; 4];
        };
        let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4) as usize;
        std::array::from_fn(|c| f16_bits_to_f32(tile.rgba[i + c]))
    }

    /// The canvas's fill input alone, the GPU gate set by hand (the plain
    /// fill's own path), the whole wide paper in view at 25 %.
    fn fill_rig(state: AppState) -> (egui_kittest::Harness<'static, (CanvasView, AppState)>, ViewXform) {
        fill_rig_at(state, 0.25)
    }

    /// The fill rig at `zoom`, the frame centred in its 800×600 pane.
    fn fill_rig_at(
        state: AppState,
        zoom: f32,
    ) -> (egui_kittest::Harness<'static, (CanvasView, AppState)>, ViewXform) {
        let xf = ViewXform {
            zoom,
            pan: vec2(0.0, 0.0),
            rotation_deg: 0.0,
            mirror: false,
            center: pos2(400.0, 300.0),
            paper: vec2(1920.0, 1080.0),
        };
        let mut h = egui_kittest::Harness::builder().with_size(vec2(800.0, 600.0)).build_ui_state(
            move |ui, (canvas, state): &mut (CanvasView, AppState)| {
                let rect = ui.max_rect();
                let response = ui.allocate_rect(rect, Sense::click_and_drag());
                canvas.raster = true;
                canvas.fill_input(ui, &response, rect, &|s| xf.screen_to_paper(s), state);
            },
            (CanvasView::new(), state),
        );
        h.step();
        (h, xf)
    }

    fn click(h: &mut egui_kittest::Harness<'static, (CanvasView, AppState)>, at: Pos2, modifiers: Modifiers) {
        h.input_mut().modifiers = modifiers;
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers };
        h.input_mut().events.push(egui::Event::PointerMoved(at));
        h.input_mut().events.push(button(true));
        h.step();
        h.input_mut().events.push(button(false));
        h.step();
        h.input_mut().modifiers = Modifiers::NONE;
        h.step();
    }

    #[test]
    fn auseam_stroke_signal_vector_route() {
        let mut h = EditorHarness::new();
        let c = h.editor().canvas.dbg_rect.center();
        let path = [c, c + vec2(30.0, 0.0), c + vec2(60.0, 10.0)];
        h.drag_path(&path);
        assert_eq!(h.state().strokes_done, 1, "on its own cel");
        // A frame with no cel of its own: the stroke makes one, the other arm.
        h.state_mut().goto(40);
        h.drag_path(&path);
        assert_eq!(h.state().strokes_done, 2, "on a new cel");
        let ink: usize = h.state().cut().drawings.iter().map(|d| d.strokes.len()).sum();
        assert_eq!(ink, 2, "the counter counted what landed");
        // Refused at pen down: nothing landed, nothing counted.
        h.perform(Action::ToggleColumnLocked);
        let seq = h.state().refusal_seq;
        h.drag_path(&path);
        assert_ne!(h.state().refusal_seq, seq);
        assert_eq!(h.state().strokes_done, 2);
    }

    #[test]
    fn auseam_stroke_signal_raster_route() {
        let mut state = wide_state();
        paint_texels(&mut state, &ring(0, 0, 9, 9), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(state.strokes_done, 1, "on its own cel");
        state.goto(40);
        paint_texels(&mut state, &ring(20, 20, 29, 29), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(state.strokes_done, 2, "on a new cel");
        // An eraser stroke on a frame with no cel: nothing lands.
        state.goto(80);
        let slot = state.active_slot_resolved().expect("an armed layer");
        assert!(state.commit_raster(Vec::new(), slot).is_none());
        assert_eq!(state.strokes_done, 2);
        // A stroke that changed no pixel of its own cel: the answer still
        // says where, and nothing landed to count.
        state.goto(40);
        let same = state.active_layer_tiles().expect("its own cel").iter().map(|(c, t)| (*c, t.clone())).collect();
        let (edits, undo) = (state.engine.revision(), state.engine.history_len());
        assert!(state.commit_raster(same, slot).is_some());
        assert_eq!((state.engine.revision(), state.engine.history_len()), (edits, undo));
        assert_eq!(state.strokes_done, 2, "an empty diff is no stroke");
    }

    #[test]
    fn auseam_stroke_signal_skips_fill_and_paste() {
        let mut state = wide_state();
        paint_texels(&mut state, &ring(400, 400, 600, 600), [0.1, 0.1, 0.1, 1.0]);
        let done = state.strokes_done;
        let (mut h, xf) = fill_rig(state);
        click(&mut h, xf.paper_to_screen(pos2(500.5, 500.5)), Modifiers::NONE);
        let state = &mut h.state_mut().1;
        assert!(texel(state, 500, 500)[3] > 0.99, "filled: {}", state.status);
        assert_eq!(state.strokes_done, done, "a fill is no stroke");
        // A paste on the frame's own cel lands through the region edit.
        let square = [(395.0, 395.0), (605.0, 395.0), (605.0, 605.0), (395.0, 605.0)];
        assert!(state.copy_pixels(&square), "{}", state.status);
        state.goto(40);
        state.new_drawing_at_frame();
        state.paste_pixels();
        assert!(texel(state, 500, 500)[3] > 0.99, "pasted: {}", state.status);
        assert_eq!(state.strokes_done, done, "own cel");
        // One on a frame with no cel lands the way a raster stroke does.
        state.goto(80);
        let cels = state.cut().drawings.len();
        state.paste_pixels();
        assert_eq!(state.cut().drawings.len(), cels + 1, "{}", state.status);
        assert_eq!(state.strokes_done, done, "`land_raster`, not `commit_raster`");
    }

    /// The real route: a pen stamped on the GPU, read back at pen-up.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn auseam_stroke_signal_on_a_real_device() {
        let mut h = EditorHarness::with_gpu(wide_state());
        h.run_frames(4);
        assert!(h.editor().canvas.raster, "a paint layer is up");
        let tiles = |h: &EditorHarness| h.state().active_layer_tiles().map_or(0, |t| t.len());
        assert_eq!(tiles(&h), 0);
        let c = h.editor().canvas.dbg_rect.center();
        h.drag_path(&[c, c + vec2(40.0, 0.0), c + vec2(80.0, 12.0)]);
        h.run_frames(8);
        assert_eq!(h.state().strokes_done, 1, "{}", h.state().status);
        assert!(tiles(&h) > 0, "the stroke came back from the device");
    }

    /// A guest's streamed stroke: the host commits it, the pen finished here.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn auseam_stroke_signal_on_a_guests_stream() {
        let mut h = EditorHarness::with_gpu(wide_state());
        h.run_frames(4);
        let canvas = &mut h.editor_mut().canvas;
        assert!(canvas.raster, "a paint layer is up");
        (canvas.is_guest, canvas.guest_ready, canvas.session_live) = (true, true, true);
        let c = canvas.dbg_rect.center();
        h.drag_path(&[c, c + vec2(40.0, 0.0), c + vec2(80.0, 12.0)]);
        h.run_frames(8);
        assert_eq!(h.state().strokes_done, 1, "{}", h.state().status);
        assert!(h.state().status.starts_with("stroke sent"), "{}", h.state().status);
        let tiles = h.state().active_layer_tiles().map_or(0, |t| t.len());
        assert_eq!(tiles, 0, "nothing committed on the guest's side");
    }

    #[test]
    fn extpaper_fill_refill_and_replace_left_of_the_frame_are_one_undo_step() {
        let mut state = wide_state();
        paint_texels(&mut state, &ring(-400, 400, -200, 600), [0.1, 0.1, 0.1, 1.0]);
        let (mut h, xf) = fill_rig(state);
        let at = xf.paper_to_screen(pos2(-299.5, 500.5));
        let n0 = h.state().1.engine.history_len();
        click(&mut h, at, Modifiers::NONE);
        {
            let (_, s) = h.state();
            assert_eq!(s.engine.history_len(), n0 + 1, "one undo step: {}", s.status);
            assert!(texel(s, -300, 500)[3] > 0.99 && texel(s, -399, 401)[3] > 0.99, "filled beyond the frame");
            assert!(s.active_layer_tiles().is_some_and(|m| m.contains_key(&(-5, 7))), "a tile at negative x");
            assert!(texel(s, -450, 500)[3] < 0.01, "not past the ring");
        }
        h.state_mut().1.undo();
        assert!(texel(&h.state().1, -300, 500)[3] < 0.01, "undo removes the fill");
        click(&mut h, at, Modifiers::NONE);
        let flat = texel(&h.state().1, -300, 500);
        // Shift+click: refill the flat in a new colour — one step, undone.
        h.state_mut().0.brush_color = [200, 40, 30, 255];
        let n1 = h.state().1.engine.history_len();
        click(&mut h, at, Modifiers::SHIFT);
        let refilled = texel(&h.state().1, -300, 500);
        assert_eq!(h.state().1.engine.history_len(), n1 + 1, "refill: {}", h.state().1.status);
        assert_ne!(refilled, flat);
        h.state_mut().1.undo();
        assert_eq!(texel(&h.state().1, -300, 500), flat, "undo restores the flat");
        // Ctrl+Shift+click: replace every texel of that colour — the same.
        let all = Modifiers { shift: true, ctrl: true, command: true, ..Modifiers::NONE };
        click(&mut h, at, all);
        assert_eq!(h.state().1.engine.history_len(), n1 + 1, "replace: {}", h.state().1.status);
        assert_ne!(texel(&h.state().1, -300, 500), flat);
        h.state_mut().1.undo();
        assert_eq!(texel(&h.state().1, -300, 500), flat);
        // Outside the paper: refused on the Aka lane, nothing changes.
        let off = xf.paper_to_screen(pos2(-599.5, 500.5));
        let n2 = h.state().1.engine.history_len();
        let keys = |s: &AppState| -> Vec<((i32, i32), u64)> {
            s.active_layer_tiles().map(|m| m.iter().map(|(c, t)| (*c, t.hash)).collect()).unwrap_or_default()
        };
        let before = keys(&h.state().1);
        for mods in [Modifiers::NONE, Modifiers::SHIFT, all] {
            h.state_mut().1.refusal.clear();
            click(&mut h, off, mods);
            let (_, s) = h.state();
            assert_eq!(s.refusal, crate::doc::OUTSIDE_PAPER, "{mods:?}");
            assert_eq!(s.engine.history_len(), n2, "{mods:?}: no change");
            assert_eq!(keys(s), before, "{mods:?}: the layer is untouched");
        }
    }

    #[test]
    fn extpaper_select_all_lasso_and_eyedropper_cover_the_paper() {
        let mut state = wide_state();
        let mut canvas = CanvasView::new();
        canvas.select_all(&mut state);
        let sel = canvas.selection.clone().expect("selected");
        assert_eq!(sel, vec![pos2(-540.0, 0.0), pos2(2460.0, 0.0), pos2(2460.0, 1080.0), pos2(-540.0, 1080.0)]);
        canvas.selection = None;
        // A lasso fill across the paper's left edge stops at it.
        let pts = [pos2(-700.0, 400.0), pos2(-400.0, 400.0), pos2(-400.0, 600.0), pos2(-700.0, 600.0)];
        canvas.commit_lasso_fill(&pts, &mut state);
        assert!(texel(&state, -530, 500)[3] > 0.5, "filled: {}", state.status);
        let t = TILE as i32;
        for ((tx, ty), tile) in state.active_layer_tiles().expect("tiles") {
            for i in 0..TILE * TILE {
                let x = tx * t + (i % TILE) as i32;
                let a = f16_bits_to_f32(tile.rgba[i * 4 + 3]);
                assert!(x >= -540 || a == 0.0, "no texel left of the paper: ({x}, {ty})");
            }
        }
        // The eyedropper picks what is painted left of the frame.
        paint_texels(&mut state, &[(-100, 500)], [0.5, 0.25, 0.125, 1.0]);
        let px = pick_sample(&state, -99.5, 500.5, false).expect("a pick beyond the frame");
        assert_eq!(px, [0.5, 0.25, 0.125, 1.0]);
        assert_eq!(pick_sample(&state, -99.5, 500.5, true), Some(px), "the composite pick too");
        assert_eq!(pick_sample(&state, -600.5, 500.5, false), None, "off the paper");
        // Paper = frame: all as before.
        state.set_cut_paper(IRect::frame(1920, 1080));
        canvas.select_all(&mut state);
        assert_eq!(
            canvas.selection.clone().expect("selected"),
            vec![pos2(0.0, 0.0), pos2(1920.0, 0.0), pos2(1920.0, 1080.0), pos2(0.0, 1080.0)]
        );
        assert_eq!(pick_sample(&state, -99.5, 500.5, false), None);
        // A lasso across the frame's left edge stops at it, as before; one
        // wholly left of it is refused on the Aka lane.
        let pts = [pos2(-300.0, 700.0), pos2(100.0, 700.0), pos2(100.0, 900.0), pos2(-300.0, 900.0)];
        canvas.commit_lasso_fill(&pts, &mut state);
        assert!(texel(&state, 10, 800)[3] > 0.5, "filled: {}", state.status);
        assert_eq!(texel(&state, -10, 800)[3], 0.0, "nothing left of the frame");
        let n = state.engine.history_len();
        state.refusal.clear();
        let pts = [pos2(-300.0, 700.0), pos2(-100.0, 700.0), pos2(-100.0, 900.0), pos2(-300.0, 900.0)];
        canvas.commit_lasso_fill(&pts, &mut state);
        assert_eq!(state.refusal, crate::doc::OUTSIDE_PAPER);
        assert_eq!(state.engine.history_len(), n);
    }

    #[test]
    fn extpaper_presence_follows_the_pointer_beyond_the_frame() {
        let mut h = EditorHarness::new();
        h.run();
        h.state_mut().set_cut_paper(WIDE);
        h.editor_mut().canvas.set_abs_zoom(0.3);
        h.run();
        let s = h.editor().canvas.view_xform().paper_to_screen(pos2(-100.0, 500.0));
        h.point_at(s);
        let p = h.editor().canvas.presence_pos.expect("over the paper");
        assert!((p[0] + 100.0).abs() < 1.0 && (p[1] - 500.0).abs() < 1.0, "{p:?}");
        // Paper = frame: the same spot is off the paper, as before.
        h.state_mut().set_cut_paper(IRect::frame(1920, 1080));
        h.run();
        h.point_at(s);
        assert_eq!(h.editor().canvas.presence_pos, None);
    }

    #[test]
    fn extpaper_canvas_keeps_its_layer_rect_and_follows_the_paper() {
        let mut h = EditorHarness::new();
        h.run();
        let frame = LayerRect::frame(1920, 1080);
        assert_eq!(h.editor().canvas.layer_rect, frame, "paper = frame: the old layer");
        h.state_mut().set_cut_paper(WIDE);
        h.run();
        let wide = h.editor().canvas.layer_rect;
        assert_eq!((wide.origin, wide.size, wide.capped), ((-540, 0), (3000, 1080), false));
        // A new cut has its own (frame) paper.
        let scene = h.state().view.scene;
        h.state_mut().new_cut(scene);
        h.run();
        assert_eq!(h.editor().canvas.layer_rect, frame, "follows the cut switch");
        // A paper set while a stroke is in progress lands after the stroke.
        let c = h.editor().canvas.view_xform().paper_to_screen(pos2(960.0, 540.0));
        h.press_and_move(c, c + vec2(30.0, 10.0), 3);
        assert!(h.editor().canvas.stroke_active(), "a stroke in progress");
        h.state_mut().set_cut_paper(WIDE);
        h.point_at(c + vec2(32.0, 12.0));
        assert_eq!(h.editor().canvas.layer_rect, frame, "held mid-stroke");
        h.release(c + vec2(32.0, 12.0));
        h.run();
        assert_eq!(h.editor().canvas.layer_rect, layer_rect(WIDE, (1920, 1080), 8192), "then the paper's");
    }

    #[test]
    fn extpaper_paper_size_window_shows_the_cap_and_the_memory() {
        let mut h = EditorHarness::new();
        h.run();
        assert_eq!(h.editor().canvas.max_layer_dim, 8192, "the editor's limit");
        let huge = IRect { x: -4040, y: 0, w: 10000, h: 1080 };
        h.state_mut().set_cut_paper(huge);
        h.perform(Action::PaperSize);
        h.run();
        assert!(h.has_text_containing(
            "this GPU paints at most 8192 px a side: painting covers 8192×1080 of the 10000×1080 paper"
        ));
        h.state_mut().set_cut_paper(WIDE);
        h.perform(Action::PaperSize);
        h.run();
        let cols = h.state().cut().render_order().len().saturating_sub(1);
        let text = memory_text(&layer_rect(WIDE, (1920, 1080), 8192), cols);
        assert!(h.has_text_containing(&text), "{text}");
        assert!(!h.has_text_containing("this GPU paints at most"), "3000 px fits");
        assert!(!h.has_text_containing("does not cover the whole frame"), "it holds the frame");
        // A paper off the frame says painting stays on the frame.
        h.state_mut().set_cut_paper(IRect { x: 100, y: 0, w: 3000, h: 1080 });
        h.perform(Action::PaperSize);
        h.run();
        assert!(h.has_text_containing(
            "this paper does not cover the whole frame: painting stays on the frame"
        ));
    }

    /// fnv1a, as the engine hashes tile bytes.
    fn fnv(bytes: impl Iterator<Item = u8>) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    /// The armed layer's tiles as (count, fnv over each coord and hash).
    fn tile_set(state: &AppState) -> (usize, u64) {
        let tiles = state.active_layer_tiles().cloned().unwrap_or_default();
        let mut bytes = Vec::new();
        for ((x, y), t) in &tiles {
            bytes.extend(x.to_le_bytes());
            bytes.extend(y.to_le_bytes());
            bytes.extend(t.hash.to_le_bytes());
        }
        (tiles.len(), fnv(bytes.into_iter()))
    }

    #[test]
    fn extpaper_paper_equal_frame_commits_the_pre_gm20_tiles() {
        // Criterion 6: on paper = frame the fill, refill and replace commit
        // the very tiles the pre-GM20 build (64df613) committed — its values,
        // pinned as literals.
        let mut state = AppState::new_project("pin", 1920, 1080, 24, 300.0);
        state.new_drawing_at_frame();
        paint_texels(&mut state, &ring(100, 100, 300, 300), [0.1, 0.1, 0.1, 1.0]);
        assert_eq!(tile_set(&state), (12, 17_910_685_424_988_270_061), "the inked ring");
        let (mut h, xf) = fill_rig(state);
        let at = xf.paper_to_screen(pos2(200.5, 200.5));
        let n0 = h.state().1.engine.history_len();
        click(&mut h, at, Modifiers::NONE);
        assert_eq!(h.state().1.engine.history_len(), n0 + 1, "{}", h.state().1.status);
        assert_eq!(tile_set(&h.state().1), (16, 17_019_425_477_342_794_235), "the fill");
        h.state_mut().0.brush_color = [200, 40, 30, 255];
        click(&mut h, at, Modifiers::SHIFT);
        assert_eq!(h.state().1.engine.history_len(), n0 + 2, "{}", h.state().1.status);
        assert_eq!(tile_set(&h.state().1), (16, 2_941_027_151_995_740_320), "the refill");
        h.state_mut().1.undo();
        let all = Modifiers { shift: true, ctrl: true, command: true, ..Modifiers::NONE };
        click(&mut h, at, all);
        assert_eq!(h.state().1.engine.history_len(), n0 + 2, "{}", h.state().1.status);
        assert_eq!(tile_set(&h.state().1), (16, 2_941_027_151_995_740_320), "the replace");
    }

    /// Two screen rects within half a point.
    fn near(a: Rect, b: Rect) -> bool {
        (a.min - b.min).length() < 0.5 && (a.max - b.max).length() < 0.5
    }

    #[test]
    fn extpaper_the_sheet_is_the_whole_paper_with_the_frame_on_it() {
        let mut h = EditorHarness::new();
        h.run();
        h.state_mut().set_cut_paper(WIDE);
        h.editor_mut().canvas.set_abs_zoom(0.25);
        h.run();
        let xf = h.editor().canvas.view_xform();
        let screen = |x0: f32, y0: f32, x1: f32, y1: f32| {
            Rect::from_two_pos(xf.paper_to_screen(pos2(x0, y0)), xf.paper_to_screen(pos2(x1, y1)))
        };
        let (sheet, frame) = (screen(-540.0, 0.0, 2460.0, 1080.0), screen(0.0, 0.0, 1920.0, 1080.0));
        let papers = h.filled_rects(plate::PAPER);
        assert!(papers.iter().any(|r| near(*r, sheet)), "the whole paper laid down: {papers:?}");
        assert!(!papers.iter().any(|r| near(*r, frame)), "not the frame alone");
        let outlines = h.outlined_rects(plate::LEGEND);
        assert!(outlines.iter().any(|r| near(*r, frame)), "the frame drawn on it: {outlines:?}");
        assert!(h.filled_rects(plate::paper_veil()).is_empty(), "3000 px: nothing veiled");
        // Capped (10000 px at 8192): the paper past the window is veiled.
        h.state_mut().set_cut_paper(IRect { x: -4040, y: 0, w: 10000, h: 1080 });
        h.editor_mut().canvas.set_abs_zoom(0.05);
        h.run();
        let xf = h.editor().canvas.view_xform();
        let screen = |x0: f32, x1: f32| {
            Rect::from_two_pos(xf.paper_to_screen(pos2(x0, 0.0)), xf.paper_to_screen(pos2(x1, 1080.0)))
        };
        let veils = h.filled_rects(plate::paper_veil());
        assert_eq!(veils.len(), 2, "{veils:?}");
        assert!(veils.iter().any(|r| near(*r, screen(-4040.0, -3136.0))), "{veils:?}");
        assert!(veils.iter().any(|r| near(*r, screen(5056.0, 5960.0))), "{veils:?}");
        assert!(h.filled_rects(plate::PAPER).iter().any(|r| near(*r, screen(-4040.0, 5960.0))));
        // Paper = frame: the sheet is the frame, as before.
        h.state_mut().set_cut_paper(IRect::frame(1920, 1080));
        h.editor_mut().canvas.set_abs_zoom(0.25);
        h.run();
        let xf = h.editor().canvas.view_xform();
        let frame = Rect::from_two_pos(xf.paper_to_screen(pos2(0.0, 0.0)), xf.paper_to_screen(pos2(1920.0, 1080.0)));
        assert!(near(frame, xf.paper_bounds()));
        let papers = h.filled_rects(plate::PAPER);
        assert!(papers.iter().any(|r| near(*r, frame)), "{papers:?}");
        let wide = Rect::from_two_pos(xf.paper_to_screen(pos2(-540.0, 0.0)), xf.paper_to_screen(pos2(2460.0, 1080.0)));
        assert!(!papers.iter().any(|r| near(*r, wide)));
        assert!(h.filled_rects(plate::paper_veil()).is_empty());
    }

    /// The miss check turns the sheet dark, so a stray pale mark jumps out:
    /// it reads a token of its own, not a surface that went light in the look.
    #[test]
    fn aulook_miss_check_paper_stays_dark() {
        let mut h = EditorHarness::new();
        h.run();
        h.state_mut().set_cut_paper(IRect::frame(1920, 1080));
        h.editor_mut().canvas.set_abs_zoom(0.25);
        h.run();
        let paper = h.editor().canvas.view_xform().paper_bounds();
        assert!(h.filled_rects(plate::PAPER).iter().any(|r| near(*r, paper)), "white until it is checked");
        h.editor_mut().canvas.miss_check = true;
        h.run();
        let dark = h.filled_rects(plate::STEEL);
        assert!(dark.iter().any(|r| near(*r, paper)), "{dark:?}");
        assert!(!h.filled_rects(plate::PAPER).iter().any(|r| near(*r, paper)), "no white left under it");
        let steel = effective(plate::STEEL, 1.0, [0.0; 3]);
        let mark = ratio(effective(plate::ON_FILL, 1.0, steel), steel);
        assert!(mark > 13.6, "a white miss on it reads at {mark:.2}");
    }

    /// The veil over paper the GPU does not paint reads as steel through the
    /// sheet: darker than paper by day, the same steel under the miss check.
    #[test]
    fn aulook_paper_veil_stays_dark() {
        let mut h = EditorHarness::new();
        h.run();
        h.state_mut().set_cut_paper(IRect { x: -4040, y: 0, w: 10000, h: 1080 });
        h.editor_mut().canvas.set_abs_zoom(0.05);
        h.run();
        assert_eq!(h.filled_rects(plate::paper_veil()).len(), 2, "either side of the window");
        let white = [1.0; 3];
        let veiled = ratio(effective(plate::paper_veil(), 1.0, white), white);
        assert!(veiled > 2.4, "the veiled paper against the paper: {veiled:.2}");
        h.editor_mut().canvas.miss_check = true;
        h.run();
        assert!(h.filled_rects(plate::paper_veil()).is_empty(), "the check paints it solid");
        assert!(h.filled_rects(plate::STEEL).len() >= 3, "the sheet and both strips");
    }

    #[test]
    fn extpaper_a_capped_paper_paints_only_its_window() {
        // 10000 px wide at the 8192 side limit: the window is x −3136 to 5056.
        let huge = IRect { x: -4040, y: 0, w: 10000, h: 1080 };
        let mut state = AppState::new_project("cap", 1920, 1080, 24, 300.0);
        state.new_drawing_at_frame();
        state.set_cut_paper(huge);
        let window = layer_rect(huge, (1920, 1080), 8192);
        assert_eq!((window.origin, window.size, window.capped), ((-3136, 0), (8192, 1080), true));
        assert_eq!(
            unpainted_strips(state.paper_area(), &window),
            vec![IRect { x: -4040, y: 0, w: 904, h: 1080 }, IRect { x: 5056, y: 0, w: 904, h: 1080 }],
            "the paper either side of the window is veiled"
        );
        assert_eq!(unpainted_strips(WIDE, &layer_rect(WIDE, (1920, 1080), 8192)), Vec::new());
        // A ring across the window's left edge, and one grey flat on each side.
        let grey = [0.5, 0.5, 0.5, 1.0];
        paint_texels(&mut state, &ring(-3300, 400, -3000, 600), [0.1, 0.1, 0.1, 1.0]);
        paint_texels(&mut state, &[(-3500, 300), (-3050, 300)], grey);
        let (mut h, xf) = fill_rig_at(state, 0.05);
        assert_eq!(h.state().0.max_layer_dim, 8192);
        // A fill inside the window stops at its edge, inside the ring too.
        let n = h.state().1.engine.history_len();
        click(&mut h, xf.paper_to_screen(pos2(-3049.5, 500.5)), Modifiers::NONE);
        {
            let s = &h.state().1;
            assert_eq!(s.engine.history_len(), n + 1, "{}", s.status);
            assert!(texel(s, -3136, 500)[3] > 0.99 && texel(s, -3010, 500)[3] > 0.99, "filled");
            assert!(texel(s, -3137, 500)[3] < 0.01, "not past the window");
        }
        // Past the window on the paper, and off the paper: every fill is
        // refused on the Aka lane, the layer untouched.
        let all = Modifiers { shift: true, ctrl: true, command: true, ..Modifiers::NONE };
        let keys = |s: &AppState| -> Vec<((i32, i32), u64)> {
            s.active_layer_tiles().map(|m| m.iter().map(|(c, t)| (*c, t.hash)).collect()).unwrap_or_default()
        };
        let before = keys(&h.state().1);
        let past = xf.paper_to_screen(pos2(-3499.5, 300.5));
        let off = xf.paper_to_screen(pos2(-5999.5, 300.5));
        for (at, why) in [(past, crate::doc::PAST_WINDOW), (off, crate::doc::OUTSIDE_PAPER)] {
            for mods in [Modifiers::NONE, Modifiers::SHIFT, all] {
                h.state_mut().1.refusal.clear();
                click(&mut h, at, mods);
                let s = &h.state().1;
                assert_eq!(s.refusal, why, "{mods:?}");
                assert_eq!(s.engine.history_len(), n + 1, "{mods:?}: no change");
                assert_eq!(keys(s), before, "{mods:?}: the layer is untouched");
            }
        }
        // A replace in the window leaves the same grey past it.
        h.state_mut().0.brush_color = [200, 40, 30, 255];
        click(&mut h, xf.paper_to_screen(pos2(-3049.5, 300.5)), all);
        {
            let s = &h.state().1;
            assert_eq!(s.engine.history_len(), n + 2, "replace: {}", s.status);
            assert_ne!(texel(s, -3050, 300), grey, "replaced in the window");
            assert_eq!(texel(s, -3500, 300), grey, "kept past it");
        }
        // The lasso fill and select-all keep to the window too.
        let (canvas, state) = h.state_mut();
        let n = state.engine.history_len();
        let lasso = |x0: f32, x1: f32| [pos2(x0, 700.0), pos2(x1, 700.0), pos2(x1, 900.0), pos2(x0, 900.0)];
        for (pts, why) in [
            (lasso(-3600.0, -3300.0), crate::doc::PAST_WINDOW),
            (lasso(-6000.0, -5000.0), crate::doc::OUTSIDE_PAPER),
        ] {
            state.refusal.clear();
            canvas.commit_lasso_fill(&pts, state);
            assert_eq!(state.refusal, why);
            assert_eq!(state.engine.history_len(), n, "nothing filled");
        }
        canvas.commit_lasso_fill(&lasso(-3300.0, -3000.0), state);
        assert!(texel(state, -3126, 800)[3] > 0.5, "filled in the window: {}", state.status);
        assert_eq!(texel(state, -3140, 800)[3], 0.0, "not past it");
        canvas.select_all(state);
        assert_eq!(
            canvas.selection.clone().expect("selected"),
            vec![pos2(-3136.0, 0.0), pos2(5056.0, 0.0), pos2(5056.0, 1080.0), pos2(-3136.0, 1080.0)]
        );
    }

    #[test]
    fn extpaper_a_pen_up_keeps_the_art_of_the_layer_it_lands_on() {
        // The readback's beyond-the-layer merge reads the LATCHED layer of
        // the frame's own cel (`commit_raster`'s target), never the armed
        // or the displayed one.
        let keys = |m: &BTreeMap<TileCoord, Arc<TileData>>| -> Vec<((i32, i32), u64)> {
            m.iter().map(|(c, t)| (*c, t.hash)).collect()
        };
        let mut state = wide_state();
        paint_texels(&mut state, &[(-500, 500)], [0.5, 0.5, 0.5, 1.0]);
        let slot = state.active_slot_resolved().expect("armed");
        let armed = keys(state.active_layer_tiles().expect("painted"));
        assert_eq!(keys(&state.commit_target_tiles(slot)), armed);
        assert!(state.commit_target_tiles(1 - slot.min(1)).is_empty(), "the cel's other layer");
        // A held frame shows the held drawing; a stroke there makes a fresh
        // cel, whose before is empty.
        state.view.onion = false;
        state.goto(1);
        assert_eq!(state.own_key_drawing(), None);
        assert_eq!(state.active_layer_tiles().map(keys), Some(armed), "the held drawing shows");
        assert!(state.commit_target_tiles(slot).is_empty());
    }

    #[test]
    fn extpaper_a_replay_paints_on_its_own_cuts_paper() {
        // Cut A on the frame, cut B on the wide paper: a stroke on B's
        // drawing replays into B's rect whichever cut this viewer shows and
        // whatever rect its canvas holds — every peer, the same tiles.
        let mut state = AppState::new_project("rp", 1920, 1080, 24, 300.0);
        state.new_drawing_at_frame();
        let on_a = state.own_key_drawing().expect("A's drawing");
        let (scene, cut_a) = (state.view.scene, state.view.cut);
        state.new_cut(scene);
        let cut_b = state.view.cut;
        state.set_cut_paper(WIDE);
        state.new_drawing_at_frame();
        let on_b = state.own_key_drawing().expect("B's drawing");
        state.goto_cut(scene, cut_a);
        assert_eq!(state.view.cut, cut_a);
        let mut canvas = CanvasView::new();
        let (frame, wide) = (LayerRect::frame(1920, 1080), layer_rect(WIDE, (1920, 1080), 8192));
        assert_eq!(canvas.sync_layer_rect(&state), frame, "the viewer is on A");
        assert_eq!(canvas.replay_rect(&state, on_b), wide);
        assert_eq!(canvas.replay_rect(&state, on_a), frame);
        // A rect held under the local pen does not leak into a replay.
        canvas.layer_rect = layer_rect(IRect { x: -100, y: 0, w: 2100, h: 1080 }, (1920, 1080), 8192);
        assert_eq!(canvas.replay_rect(&state, on_b), wide);
        // A peer whose GPU paints 16384 a side replays at the room's limit.
        state.goto_cut(scene, cut_b);
        state.set_cut_paper(IRect { x: -4040, y: 0, w: 10000, h: 1080 });
        state.goto_cut(scene, cut_a);
        let big = CanvasView::new().with_layer_limit(paint::room_limit(16384));
        assert_eq!(big.replay_rect(&state, on_b), canvas.replay_rect(&state, on_b));
        assert_eq!(canvas.replay_rect(&state, on_b).origin, (-3136, 0));
        // A drawing no cut holds replays (and fails) on the frame.
        assert_eq!(canvas.replay_rect(&state, DrawingId(u64::MAX - 1)), frame);
    }

    #[test]
    fn extpaper_every_paint_layer_resize_goes_through_the_one_helper() {
        // Criterion 7: the paint layer is resized in ONE place (the canvas's
        // `layer_onto`), onto a rect from THE layer-rect helper (`rect_of`);
        // the viewer asks the canvas. The graph's pool (`g`) is frame-sized
        // and no paint layer.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let owner = |text: &str, at: usize| -> String {
            let head = &text[..at];
            let start = head.rfind(" fn ").map_or(0, |i| i + 4);
            head[start..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()
        };
        let receiver = |text: &str, at: usize| -> String {
            let r: String =
                text[..at].chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            r.chars().rev().collect()
        };
        let (mut resizes, mut rects, mut ontos) = (Vec::new(), Vec::new(), Vec::new());
        let mut viewer = 0;
        for entry in std::fs::read_dir(dir).expect("src") {
            let path = entry.expect("an entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let name = path.file_name().expect("a name").to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).expect("readable");
            for (at, _) in text.match_indices(concat!(".ensure", "_size(")) {
                if receiver(&text, at) != "g" {
                    resizes.push((name.clone(), owner(&text, at), receiver(&text, at)));
                }
            }
            for (at, _) in text.match_indices(concat!("paint::layer", "_rect(")) {
                rects.push((name.clone(), owner(&text, at)));
            }
            for (at, _) in text.match_indices(concat!("self.layer", "_onto(")) {
                ontos.push((name.clone(), owner(&text, at)));
            }
            viewer += text.matches(concat!("self.canvas.ensure", "_layer(&self.state, p)")).count();
        }
        resizes.sort();
        rects.sort();
        ontos.sort();
        let s = |a: &str, b: &str| (a.to_string(), b.to_string());
        let (file, f) = s("canvas.rs", "layer_onto");
        assert_eq!(resizes, vec![(file, f, "p".to_string())], "one resize, in the canvas");
        assert_eq!(ontos, vec![s("canvas.rs", "ensure_layer"), s("canvas.rs", "replay_one")]);
        // The canvas's one helper, and the Paper size window's preview of a
        // rect it has not set yet.
        assert_eq!(rects, vec![s("canvas.rs", "rect_of"), s("main.rs", "paper_size_window")]);
        assert_eq!(viewer, 1, "the viewer sizes the layer through the canvas");
    }
}
