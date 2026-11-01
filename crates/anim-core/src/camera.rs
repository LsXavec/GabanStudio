//! GM10: the cut's paper and the Camera node — pan, T.U./T.B. (zoom),
//! rotate, shake, multiplane parallax and depth of field — implemented ONCE,
//! here, as pure maths that `export`'s Camera arm renders on the CPU in every
//! view (D9: the GPU compositor uploads that render, never a second one).
//!
//! Laws (pinned by tests/camera_tests.rs — change them together):
//! - **Paper**: `cut_meta["camera.paper"] = {"x":0,"y":0,"w":3000,"h":1080}`,
//!   a rect in frame coordinates (tiles are i32, so it may start left of or
//!   above the frame). Missing or unreadable, it is the frame itself.
//! - **Sample map**: the output pixel centre (u, v) of a W × H frame sees a
//!   plane at depth z at `p = C + R(θ)·((u − W/2, v − H/2)/zoom) + shake(t)`,
//!   `C = (W/2 + pan_x·f, H/2 + pan_y·f)`, with the parallax
//!   `f = 1/(1 + k·z)` (farther planes, z > 0, move less; nearer, z < 0,
//!   more) and θ clockwise on screen, the Transform node's sense. A plane is
//!   sampled with `export`'s one bilinear sampler; planes lay far to near
//!   (larger z first; equal depths keep pin order, pin 0 lowest),
//!   premultiplied src-over.
//! - **Shake**: per axis, a sum of three sines (frequencies freq·{1, 2.13,
//!   3.71}, weights 1, ½, ¼) whose phases are hashed from the seed and axis,
//!   normalised and scaled by the amplitude, then held inside it as a
//!   vector. Amplitude or frequency 0 is no offset. t is the frame and freq
//!   is in cycles per frame (the camera knows no frame rate).
//! - **Depth of field**: before sampling, each plane is blurred with the FX
//!   core blur ([`crate::fx::blur`]) of radius `aperture·|z − focus_z|` px.
//! - **Field guide**: 12 fields; field n is n/12 of the frame, centred.
//!   **Safe frames**: action 93 %, title 90 % of the frame, centred.

use crate::command::{Command, CutRef};
use crate::fx::{self, FxBinds, FxParams, FxValue, ParamSpec};
use crate::graph::NodeKind;
use crate::ids::NodeId;
use crate::model::Cut;

/// The cut meta key of the paper rect (D3; this part owns its JSON).
pub const PAPER_KEY: &str = "camera.paper";

/// The largest paper side, and the farthest its corner may sit from the
/// frame's, in px (a paper is rendered whole under a camera).
pub const MAX_PAPER: u32 = 16384;

/// The largest paper area, in px (32 Mpx). A camera renders each plane as
/// a premultiplied f32 buffer of the whole paper, and a blurred plane or a
/// Blend under it holds several at once: at 32 Mpx one buffer is 512 MiB,
/// while a 16384² paper would be 4 GiB and abort the render on a failed
/// allocation. It still takes a 4K frame three times wide (11520×2160).
pub const MAX_PAPER_AREA: u64 = 1 << 25;

/// A paper `scale_w × scale_h` the size of the frame, centred on it (the
/// Paper size presets: 1.5× or 2× wide for a BG pan or hiki, 2× tall for a
/// PAN up).
pub fn centred_paper(width: u32, height: u32, scale_w: f32, scale_h: f32) -> IRect {
    let w = (width as f32 * scale_w).round().max(1.0) as u32;
    let h = (height as f32 * scale_h).round().max(1.0) as u32;
    IRect {
        x: (width as i32 - w as i32) / 2,
        y: (height as i32 - h as i32) / 2,
        w,
        h,
    }
}

/// The paper's limits in words (the Paper size window and its refusal).
pub fn paper_limits() -> String {
    format!(
        "each side 1 to {MAX_PAPER} px, at most {} Mpx, the corner within {MAX_PAPER} px of the frame's",
        MAX_PAPER_AREA >> 20
    )
}

/// The fields of the field guide.
pub const FIELDS: u32 = 12;

/// Action safe and title safe, as fractions of the frame.
pub const ACTION_SAFE: f32 = 0.93;
pub const TITLE_SAFE: f32 = 0.90;

/// The smallest parallax denominator `1 + k·z`: a plane at or nearer than
/// the camera moves at most 20× the pan rather than flipping.
pub const MIN_PARALLAX_DENOM: f32 = 0.05;

/// The most planes the inspector adds (the engine takes any number).
pub const MAX_PLANES: usize = 8;

/// The Camera node's input pins, by plane (painted beside each pin).
pub const PLANE_NAMES: [&str; MAX_PLANES] =
    ["plane 1", "plane 2", "plane 3", "plane 4", "plane 5", "plane 6", "plane 7", "plane 8"];

/// What a plane pin takes (its tooltip).
pub const PLANE_HELP: &str =
    "one multiplane level: its depth z is 0 at the field, above 0 farther (a pan moves it less), below 0 nearer";

/// A rect of whole pixels in frame coordinates (the paper).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl IRect {
    /// The frame itself: the default paper.
    pub fn frame(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            w: width,
            h: height,
        }
    }

    /// Can it be a paper? Both sides 1..=MAX_PAPER, the area at most
    /// MAX_PAPER_AREA, the corner within MAX_PAPER of the frame's.
    pub fn is_valid_paper(&self) -> bool {
        (1..=MAX_PAPER).contains(&self.w)
            && (1..=MAX_PAPER).contains(&self.h)
            && self.area() <= MAX_PAPER_AREA
            && self.x.unsigned_abs() <= MAX_PAPER
            && self.y.unsigned_abs() <= MAX_PAPER
    }

    /// Its area in px.
    pub fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }
}

/// A rect in frame coordinates (the guides).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// `frac` of a `width × height` frame, centred on it.
    pub fn centred(width: u32, height: u32, frac: f32) -> Self {
        let (fw, fh) = (width as f32, height as f32);
        let (w, h) = (fw * frac, fh * frac);
        Self {
            x: (fw - w) / 2.0,
            y: (fh - h) / 2.0,
            w,
            h,
        }
    }

    /// Top-left, top-right, bottom-right, bottom-left.
    pub fn corners(&self) -> [(f32, f32); 4] {
        let (r, b) = (self.x + self.w, self.y + self.h);
        [(self.x, self.y), (r, self.y), (r, b), (self.x, b)]
    }
}

// ---- Paper ---------------------------------------------------------------------------

/// A stored paper value, read; None when it is not a usable paper.
pub fn paper_from_json(json: &str) -> Option<IRect> {
    serde_json::from_str::<IRect>(json)
        .ok()
        .filter(IRect::is_valid_paper)
}

pub fn paper_to_json(rect: IRect) -> String {
    serde_json::to_string(&rect).unwrap_or_default()
}

/// The cut's stored paper, when it has a usable one.
pub fn paper_meta(cut: &Cut) -> Option<IRect> {
    cut.meta.get(PAPER_KEY).and_then(|j| paper_from_json(j))
}

/// The cut's paper for a `width × height` frame: its stored rect, else the
/// frame itself.
pub fn paper(cut: &Cut, width: u32, height: u32) -> IRect {
    paper_meta(cut).unwrap_or(IRect::frame(width, height))
}

/// The edit that sets the cut's paper (the frame itself is stored as no
/// key, so the default paper leaves old cuts' meta as it was). None when
/// nothing would change or `rect` cannot be a paper (the frame always can:
/// it is what a cut renders with no key).
pub fn set_paper(cut: &Cut, at: CutRef, rect: IRect, width: u32, height: u32) -> Option<Command> {
    if rect != IRect::frame(width, height) && !rect.is_valid_paper() {
        return None;
    }
    let before = cut.meta.get(PAPER_KEY).cloned();
    let after = (rect != IRect::frame(width, height)).then(|| paper_to_json(rect));
    (after != before).then(|| Command::SetCutMeta {
        at,
        key: PAPER_KEY.to_string(),
        before,
        after,
    })
}

// ---- The Camera node's parameters ----------------------------------------------------------

/// Pan range in paper px.
const PAN: f32 = 65536.0;

/// The Camera node's parameters: the inspector, the defaults and the binds
/// read them, as an Effect's read its kind's ([`crate::fx`]).
pub const CAMERA_SPECS: &[ParamSpec] = &[
    fx::float("pan_x", "pan x", "How far right the camera has moved across the paper, in paper px (a plane at depth z moves 1/(1 + k·z) as far).", -PAN, PAN, 0.0, 1.0),
    fx::float("pan_y", "pan y", "How far down the camera has moved across the paper, in paper px (a plane at depth z moves 1/(1 + k·z) as far).", -PAN, PAN, 0.0, 1.0),
    fx::float("zoom", "zoom in / zoom out", "1 frames the field; above 1 zooms in, below 1 zooms out.", 0.05, 20.0, 1.0, 0.01),
    fx::float("rotate_deg", "rotate", "Turns the camera clockwise by this many degrees (the picture turns the other way).", -3600.0, 3600.0, 0.0, 0.5),
    fx::float("shake_amp", "shake", "How far the camera shakes, in px; the offset never exceeds it, and 0 is still.", 0.0, 500.0, 0.0, 0.1),
    fx::float("shake_freq", "shake speed", "Shake cycles per frame: 0.25 is one wobble every 4 frames.", 0.0, 0.5, 0.25, 0.005),
    fx::float("shake_seed", "shake seed", "Picks the shake's pattern; one seed shakes the same way every render.", 0.0, 65535.0, 1.0, 1.0),
    fx::float("focus_z", "focus depth", "The plane depth in focus; planes nearer or farther blur by the aperture.", -100.0, 100.0, 0.0, 0.05),
    fx::float("aperture", "aperture", "Blur radius in px per unit of depth away from the focus; 0 keeps every plane sharp.", 0.0, 100.0, 0.0, 0.1),
    fx::float("parallax_k", "parallax", "How much depth slows a pan: a plane at depth z moves 1/(1 + k·z) as far.", 0.0, 10.0, 0.5, 0.01),
];

/// Every camera parameter at its default (a new node).
pub fn default_params() -> FxParams {
    FxParams(
        CAMERA_SPECS
            .iter()
            .map(|s| (s.key.to_string(), s.default_value()))
            .collect(),
    )
}

pub fn spec(key: &str) -> Option<&'static ParamSpec> {
    CAMERA_SPECS.iter().find(|s| s.key == key)
}

/// The parallax factor of a plane at depth `z`: `1/(1 + k·z)`, its
/// denominator held at [`MIN_PARALLAX_DENOM`] or more.
pub fn parallax(k: f32, z: f32) -> f32 {
    1.0 / (1.0 + k * z).max(MIN_PARALLAX_DENOM)
}

/// The depth-of-field blur radius of a plane at `z`:
/// `aperture·|z − focus_z|` px (0 for anything not a number).
pub fn dof_radius(aperture: f32, z: f32, focus_z: f32) -> f32 {
    let r = aperture * (z - focus_z).abs();
    if r.is_finite() { r.max(0.0) } else { 0.0 }
}

/// A uniform 0..1 from (seed, axis, term) — splitmix64.
fn hash01(seed: u32, axis: u32, term: u32) -> f64 {
    let mut z = ((seed as u64) << 16 | (axis as u64) << 8 | term as u64)
        .wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// The camera shake offset at time `t` (frames): deterministic, never
/// longer than `amp`, and (0, 0) when `amp` or `freq` is 0.
pub fn shake(seed: u32, t: f32, amp: f32, freq: f32) -> (f32, f32) {
    if !(amp > 0.0 && freq > 0.0 && t.is_finite() && amp.is_finite() && freq.is_finite()) {
        return (0.0, 0.0);
    }
    const MULT: [f64; 3] = [1.0, 2.13, 3.71];
    const WEIGHT: [f64; 3] = [1.0, 0.5, 0.25];
    let tau = std::f64::consts::TAU;
    let axis = |a: u32| -> f64 {
        let mut s = 0.0;
        for i in 0..3 {
            let phase = hash01(seed, a, i as u32) * tau;
            s += WEIGHT[i] * (tau * freq as f64 * MULT[i] * t as f64 + phase).sin();
        }
        s / (WEIGHT[0] + WEIGHT[1] + WEIGHT[2])
    };
    let amp64 = amp as f64;
    let (mut x, mut y) = (axis(0) * amp64, axis(1) * amp64);
    let len = (x * x + y * y).sqrt();
    if len > amp64 {
        x *= amp64 / len;
        y *= amp64 / len;
    }
    let (x, y) = (x as f32, y as f32);
    // Held inside the amplitude after the f32 rounding too.
    let len = (x * x + y * y).sqrt();
    if len > amp { (x * (amp / len), y * (amp / len)) } else { (x, y) }
}

// ---- The camera at one frame ----------------------------------------------------------------

/// A Camera node's parameters resolved at one frame (bound columns sampled
/// there) — what `export` renders and the eval hash spells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraState {
    pub frame: u32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
    pub rotate_deg: f32,
    pub shake_amp: f32,
    pub shake_freq: f32,
    pub shake_seed: u32,
    pub focus_z: f32,
    pub aperture: f32,
    pub parallax_k: f32,
}

/// One plane's precomputed sample map (see the module law).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneMap {
    /// C plus this frame's shake.
    pub cx: f32,
    pub cy: f32,
    pub cos: f32,
    pub sin: f32,
    pub zoom: f32,
    pub half_w: f32,
    pub half_h: f32,
}

impl PlaneMap {
    /// The paper point (frame coordinates) the output point (u, v) sees.
    pub fn map(&self, u: f32, v: f32) -> (f32, f32) {
        let dx = (u - self.half_w) / self.zoom;
        let dy = (v - self.half_h) / self.zoom;
        (
            self.cx + (self.cos * dx - self.sin * dy),
            self.cy + (self.sin * dx + self.cos * dy),
        )
    }
}

impl CameraState {
    /// The Camera `node` of `cut` at `frame`; None when it is not a Camera.
    pub fn at(cut: &Cut, node: NodeId, frame: u32) -> Option<Self> {
        match &cut.graph.node(node).ok()?.kind {
            NodeKind::Camera { params, binds, .. } => Some(Self::resolve(cut, params, binds, frame)),
            _ => None,
        }
    }

    /// Each parameter from its bound param column when bound and keyed at
    /// `frame`, else its stored value, else its spec default — the Effect
    /// node's law ([`fx::resolve_specs`]).
    pub fn resolve(cut: &Cut, params: &FxParams, binds: &FxBinds, frame: u32) -> Self {
        let values = fx::resolve_specs(CAMERA_SPECS, params, binds, |id| cut.param_value(id, frame));
        Self::from_values(&values, frame)
    }

    /// The defaults alone (no stored values, no binds).
    pub fn defaults(frame: u32) -> Self {
        let values =
            fx::resolve_specs(CAMERA_SPECS, &FxParams::default(), &FxBinds::default(), |_| None);
        Self::from_values(&values, frame)
    }

    fn from_values(values: &[FxValue], frame: u32) -> Self {
        let f = |key: &str| {
            CAMERA_SPECS
                .iter()
                .position(|s| s.key == key)
                .and_then(|i| match values.get(i) {
                    Some(FxValue::Float(x)) => Some(*x),
                    _ => None,
                })
                .unwrap_or(0.0)
        };
        Self {
            frame,
            pan_x: f("pan_x"),
            pan_y: f("pan_y"),
            zoom: f("zoom"),
            rotate_deg: f("rotate_deg"),
            shake_amp: f("shake_amp"),
            shake_freq: f("shake_freq"),
            shake_seed: f("shake_seed").round().max(0.0) as u32,
            focus_z: f("focus_z"),
            aperture: f("aperture"),
            parallax_k: f("parallax_k"),
        }
    }

    /// This frame's shake offset.
    pub fn shake(&self) -> (f32, f32) {
        shake(self.shake_seed, self.frame as f32, self.shake_amp, self.shake_freq)
    }

    pub fn parallax(&self, z: f32) -> f32 {
        parallax(self.parallax_k, z)
    }

    pub fn dof_radius(&self, z: f32) -> f32 {
        dof_radius(self.aperture, z, self.focus_z)
    }

    /// The sample map of a plane at depth `z` for a `width × height` frame.
    pub fn plane_map(&self, z: f32, width: u32, height: u32) -> PlaneMap {
        let f = self.parallax(z);
        let (sx, sy) = self.shake();
        let (half_w, half_h) = (width as f32 / 2.0, height as f32 / 2.0);
        let (sin, cos) = self.rotate_deg.to_radians().sin_cos();
        PlaneMap {
            cx: half_w + self.pan_x * f + sx,
            cy: half_h + self.pan_y * f + sy,
            cos,
            sin,
            zoom: self.zoom,
            half_w,
            half_h,
        }
    }

    /// The paper point the output point (u, v) sees on a plane at depth z.
    pub fn sample_map(&self, z: f32, width: u32, height: u32, u: f32, v: f32) -> (f32, f32) {
        self.plane_map(z, width, height).map(u, v)
    }

    /// `key=value,...` of the resolved parameters and this frame's shake
    /// offset: the eval hash's part of the recipe (plain `{}` floats, never
    /// quantised — eval.rs's law).
    pub fn recipe(&self) -> String {
        let (sx, sy) = self.shake();
        format!(
            "pan=({},{}),zoom={},rot={},shake=({},{}),focus={},aperture={},k={}",
            self.pan_x, self.pan_y, self.zoom, self.rotate_deg, sx, sy, self.focus_z, self.aperture,
            self.parallax_k
        )
    }
}

/// The camera's frame on the paper (the z = 0 plane): the output frame's
/// corners — top-left, top-right, bottom-right, bottom-left — through the
/// sample map, in frame coordinates.
pub fn frame_rect_on_paper(state: &CameraState, width: u32, height: u32) -> [(f32, f32); 4] {
    let map = state.plane_map(0.0, width, height);
    Rect::frame_rect(width, height).corners().map(|(u, v)| map.map(u, v))
}

impl Rect {
    /// The whole `width × height` frame.
    pub fn frame_rect(width: u32, height: u32) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: width as f32,
            h: height as f32,
        }
    }
}

/// The order a Camera's planes lay in: far to near (larger z first), equal
/// depths in pin order.
pub fn plane_order(depths: &[f32]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..depths.len()).collect();
    order.sort_by(|&a, &b| depths[b].total_cmp(&depths[a]));
    order
}

/// The camera feeding the cut's output — the first Camera met walking up
/// from the Output (None without one). The canvas draws its frame.
pub fn output_camera(cut: &Cut) -> Option<NodeId> {
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<NodeId> = cut.graph.output.into_iter().collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Ok(n) = cut.graph.node(id) else {
            continue;
        };
        if matches!(n.kind, NodeKind::Camera { .. }) {
            return Some(id);
        }
        // Pin 0 first, so the lowest input's camera wins a tie.
        stack.extend(n.inputs.iter().rev().flatten().map(|(src, _)| *src));
    }
    None
}

// ---- Field guide and safe frames ----------------------------------------------------------

/// The 12-field guide of a `width × height` frame: field n (index n − 1) is
/// n/12 of the frame, centred; field 12 is the frame.
pub fn field_guide(width: u32, height: u32) -> Vec<Rect> {
    (1..=FIELDS)
        .map(|n| Rect::centred(width, height, n as f32 / FIELDS as f32))
        .collect()
}

/// Action safe (93 %) and title safe (90 %), centred.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SafeFrames {
    pub action: Rect,
    pub title: Rect,
}

pub fn safe_frames(width: u32, height: u32) -> SafeFrames {
    SafeFrames {
        action: Rect::centred(width, height, ACTION_SAFE),
        title: Rect::centred(width, height, TITLE_SAFE),
    }
}
