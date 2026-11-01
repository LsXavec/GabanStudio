//! Compositing FX (GM9, D9): blur (bokashi), glow (tōkakō, T-kō), diffusion
//! (difyūjon, the soft diffusion filter) and grade, implemented ONCE, here,
//! on premultiplied f32 RGBA frames. The CPU golden renderer (`export`'s
//! Effect arm) calls [`FxKind::apply`]; the GPU compositor never
//! re-implements them — it renders an FX subtree through this module on a
//! worker and uploads the result (NEVER-DO 2), so the composite view, the
//! viewer and export agree.
//!
//! Every kind declares its parameters as [`ParamSpec`]s: the node graph's
//! inspector is generated from them, a stored value that is missing takes the
//! spec default, and every parameter can be bound to an X-sheet param column
//! (the Transform binds' law; a colour's column mixes its chip toward a second
//! chip). Later parts add FX by declaring specs: GM13's satsuei look (para,
//! light wrap, rim light, vignette, grain, motion blur, LUT, matte, palette
//! map) is declared here and computed once in [`crate::look`].
//!
//! Formulas (pinned by tests/fx_tests.rs — change them together):
//! - **Blur** radius r: a Gaussian with σ = r/2, done as three separable box
//!   passes whose widths come from boxes-for-Gauss ([`box_sizes`]). Each pass
//!   is a running sum, O(pixels) whatever r is. r = 0 is the identity;
//!   outside the image is transparent, or with "repeat edges" the edge pixel
//!   repeated.
//! - **Glow**: `bright = max(luma(unpremult) − threshold, 0)/(1 − threshold)`,
//!   which multiplies the pixel (premultiplied, so coverage carries) and the
//!   tint; `glow = blur(bright, radius) × intensity`; `out = input + glow`
//!   (Add, clamped only at the finishers). A wired matte input multiplies
//!   `bright` by the matte's alpha.
//! - **Diffusion**: `d = blur(input, radius)`;
//!   `out = mix(input, screen(input, d), mix)`. The overlay mode takes the
//!   Overlay blend's curve instead of screen (files say "soft light" for it
//!   from before it was named for what it is). Both are `export`'s blend
//!   formulas.
//! - **Grade**, on unpremultiplied colour:
//!   `c = pow(max(0, gain·(c + lift·(1−c))), 1/gamma) · 2^exposure`; then
//!   saturation mixes with Rec.709 luma; then temperature and tint are
//!   per-channel gains ([`TEMP_GAIN`]); then it premultiplies again.

use std::collections::BTreeMap;

use rayon::prelude::*;

use crate::color::ColorLaw;
use crate::graph::BlendMode;
use crate::ids::ParamId;
use crate::look;

/// Rec.709 luma weights: the glow's threshold and the grade's saturation.
pub const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Grade: temperature +1 multiplies red by 1 + TEMP_GAIN and blue by
/// 1 − TEMP_GAIN (warm); tint +1 multiplies green by 1 − TEMP_GAIN
/// (magenta). Negative values swing the other way.
pub const TEMP_GAIN: f32 = 0.25;

/// A premultiplied RGBA f32 frame, row-major, top row first.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    /// Premultiplied RGBA, four floats per pixel.
    pub px: Vec<f32>,
}

impl Image {
    /// A transparent frame.
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            px: vec![0.0; w as usize * h as usize * 4],
        }
    }

    /// Wrap a premultiplied buffer of exactly `w × h × 4` floats.
    pub fn from_px(w: u32, h: u32, px: Vec<f32>) -> Self {
        assert_eq!(
            px.len(),
            w as usize * h as usize * 4,
            "Image::from_px: {w}x{h} takes {} floats",
            w as usize * h as usize * 4
        );
        Self { w, h, px }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [f32; 4] {
        let i = (y as usize * self.w as usize + x as usize) * 4;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, p: [f32; 4]) {
        let i = (y as usize * self.w as usize + x as usize) * 4;
        self.px[i..i + 4].copy_from_slice(&p);
    }
}

// ---- Parameter specs ---------------------------------------------------------

/// What a parameter holds, with its range and default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamKind {
    /// A number in `min..=max`; `step` is the inspector's drag speed.
    Float {
        min: f32,
        max: f32,
        default: f32,
        step: f32,
    },
    /// A chip (straight sRGB bytes), entering the maths through the
    /// project's colour law like every chip (D8).
    Color { default: [u8; 4] },
    Bool { default: bool },
    /// One of `options` (stored by name, so reordering never remaps a file).
    /// `renamed` reads an option's former names as that option.
    Choice {
        options: &'static [&'static str],
        default: usize,
        renamed: &'static [(&'static str, usize)],
    },
    /// GM13: a key into the cut's embedded files (`Cut::blobs`) — a LUT's
    /// .cube text. Set by loading a file, never by a column.
    Blob,
    /// GM13: exact chip → chip pairs (a palette variant's map), set from the
    /// palette, never by a column.
    ColorMap,
    /// GM16: a string (a Text generator's lines), typed in the inspector,
    /// never driven by a column.
    Text { default: &'static str },
    /// GM18: polygons in frame px (a Panels generator's panels), drawn with
    /// the manga room's panel tool, never driven by a column.
    Polys,
}

/// One parameter of an FX kind: the inspector, the defaults and the binds
/// all read it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    /// Stable storage key (JSON, binds).
    pub key: &'static str,
    pub label: &'static str,
    /// One plain sentence for the tooltip.
    pub help: &'static str,
    pub kind: ParamKind,
}

impl ParamSpec {
    pub fn default_value(&self) -> FxValue {
        match self.kind {
            ParamKind::Float { default, .. } => FxValue::Float(default),
            ParamKind::Color { default } => FxValue::Color(default),
            ParamKind::Bool { default } => FxValue::Bool(default),
            ParamKind::Choice { options, default, .. } => {
                FxValue::Choice(options[default].to_string())
            }
            ParamKind::Blob => FxValue::Blob {
                blob: String::new(),
            },
            ParamKind::ColorMap => FxValue::Map {
                map: Vec::new(),
                from: String::new(),
            },
            ParamKind::Text { default } => FxValue::Text {
                text: default.to_string(),
            },
            ParamKind::Polys => FxValue::Polys { polys: Vec::new() },
        }
    }

    /// Can a param column drive it? A number, a flag and a choice read the
    /// column's value, and a colour mixes its chip toward its second chip
    /// ([`ParamSpec::to_key`]) by the column's 0–1. A file and a colour map
    /// (GM13) cannot, nor a text (GM16).
    pub fn bindable(&self) -> bool {
        match self.kind {
            ParamKind::Float { .. }
            | ParamKind::Color { .. }
            | ParamKind::Bool { .. }
            | ParamKind::Choice { .. } => true,
            ParamKind::Blob | ParamKind::ColorMap | ParamKind::Text { .. } | ParamKind::Polys => false,
        }
    }

    /// Where a bound colour keeps its second chip (the column's 1; the
    /// stored chip is its 0). Missing → the spec default.
    pub fn to_key(&self) -> String {
        format!("{}.to", self.key)
    }

    /// A stored value read through this spec: missing, wrong-typed or
    /// non-finite → the default; a number is clamped into range; a choice's
    /// former name → that option; an unknown choice name → the default.
    pub fn read(&self, v: Option<&FxValue>) -> FxValue {
        match (self.kind, v) {
            (ParamKind::Float { min, max, .. }, Some(FxValue::Float(x))) if x.is_finite() => {
                FxValue::Float(x.clamp(min, max))
            }
            (ParamKind::Color { .. }, Some(FxValue::Color(c))) => FxValue::Color(*c),
            (ParamKind::Bool { .. }, Some(FxValue::Bool(b))) => FxValue::Bool(*b),
            (ParamKind::Choice { options, renamed, .. }, Some(FxValue::Choice(s))) => {
                if options.contains(&s.as_str()) {
                    FxValue::Choice(s.clone())
                } else if let Some((_, i)) = renamed.iter().find(|(old, _)| *old == s.as_str()) {
                    FxValue::Choice(options[*i].to_string())
                } else {
                    self.default_value()
                }
            }
            (ParamKind::Blob, Some(v @ FxValue::Blob { .. }))
            | (ParamKind::ColorMap, Some(v @ FxValue::Map { .. }))
            | (ParamKind::Text { .. }, Some(v @ FxValue::Text { .. })) => v.clone(),
            // GM18: a point that is not a number is dropped, and a polygon
            // left with fewer than three points with it.
            (ParamKind::Polys, Some(FxValue::Polys { polys })) => FxValue::Polys {
                polys: polys
                    .iter()
                    .map(|p| p.iter().copied().filter(|q| q.iter().all(|v| v.is_finite())).collect::<Vec<_>>())
                    .filter(|p| p.len() >= 3)
                    .collect(),
            },
            _ => self.default_value(),
        }
    }

    /// A bound column's value `x` read through this spec: a number clamped
    /// into range, a flag on at 0.5 and above, a choice by its rounded
    /// index, a colour mixed from its stored chip (0) toward its second chip
    /// (1) per byte, rounded, with `x` clamped to 0–1. None for a non-finite
    /// value (the stored value stands).
    pub fn from_column(&self, x: f32, params: &FxParams) -> Option<FxValue> {
        if !x.is_finite() {
            return None;
        }
        match self.kind {
            ParamKind::Float { min, max, .. } => Some(FxValue::Float(x.clamp(min, max))),
            ParamKind::Bool { .. } => Some(FxValue::Bool(x >= 0.5)),
            ParamKind::Choice { options, .. } => {
                let i = (x.round().max(0.0) as usize).min(options.len() - 1);
                Some(FxValue::Choice(options[i].to_string()))
            }
            ParamKind::Color { .. } => {
                let (FxValue::Color(a), FxValue::Color(b)) = (
                    self.read(params.get(self.key)),
                    self.read(params.get(&self.to_key())),
                ) else {
                    return None;
                };
                let t = x.clamp(0.0, 1.0);
                let mix = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
                Some(FxValue::Color([mix(0), mix(1), mix(2), mix(3)]))
            }
            ParamKind::Blob | ParamKind::ColorMap | ParamKind::Text { .. } | ParamKind::Polys => None,
        }
    }
}

/// One stored parameter value (untagged JSON: a number, a bool, an RGBA
/// array, a choice name, GM13's `{"blob": key}` and
/// `{"map": [[chip, chip], ...], "from": variant}` objects, GM16's
/// `{"text": string}` and GM18's `{"polys": [[[x, y], ...], ...]}`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FxValue {
    Bool(bool),
    Float(f32),
    Color([u8; 4]),
    Choice(String),
    /// GM13: a key into `Cut::blobs` ("" = no file).
    Blob { blob: String },
    /// GM13: exact chip → chip pairs, and the palette variant they were
    /// taken from (`palette/variant`, what the app refreshes them by).
    Map {
        map: Vec<([u8; 4], [u8; 4])>,
        #[serde(default)]
        from: String,
    },
    /// GM16: a text parameter's string, as `{"text": "..."}` (a bare string
    /// is a choice name).
    Text { text: String },
    /// GM18: polygons in frame px, each a list of `[x, y]` points (a Panels
    /// generator's panels).
    Polys { polys: Vec<Vec<[f32; 2]>> },
}

impl std::fmt::Display for FxValue {
    /// The eval recipe's spelling. Plain `{}` floats are Rust's shortest
    /// exact round trip — never quantise a hash key (eval.rs's law).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{b}"),
            Self::Float(x) => write!(f, "{x}"),
            Self::Color(c) => write!(f, "#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]),
            Self::Choice(s) => write!(f, "{s}"),
            // The file's bytes join the recipe in eval (a key alone would
            // keep a replaced LUT's cache).
            Self::Blob { blob } => write!(f, "blob:{blob:?}"),
            Self::Map { map, from } => {
                write!(f, "map({from:?}:")?;
                for (a, b) in map {
                    write!(f, "{}>{};", Self::Color(*a), Self::Color(*b))?;
                }
                write!(f, ")")
            }
            Self::Text { text } => write!(f, "text:{text:?}"),
            // Every point exactly (plain `{}` floats), polygon by polygon.
            Self::Polys { polys } => {
                write!(f, "polys(")?;
                for p in polys {
                    write!(f, "[")?;
                    for q in p {
                        write!(f, "{},{};", q[0], q[1])?;
                    }
                    write!(f, "]")?;
                }
                write!(f, ")")
            }
        }
    }
}

/// A node's stored parameters by key. Missing keys take the spec default,
/// so a file from before a parameter existed still opens.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct FxParams(pub BTreeMap<String, FxValue>);

impl FxParams {
    /// Every spec of `kind` written out at its default (a new node).
    pub fn defaults(kind: FxKind) -> Self {
        Self(
            kind.specs()
                .iter()
                .map(|s| (s.key.to_string(), s.default_value()))
                .collect(),
        )
    }

    pub fn get(&self, key: &str) -> Option<&FxValue> {
        self.0.get(key)
    }

    pub fn set(&mut self, key: &str, v: FxValue) {
        self.0.insert(key.to_string(), v);
    }
}

/// Which X-sheet param column drives each parameter (by spec key). A key
/// with no entry is static.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct FxBinds(pub BTreeMap<String, ParamId>);

impl FxBinds {
    pub fn get(&self, key: &str) -> Option<ParamId> {
        self.0.get(key).copied()
    }

    /// Bind `key` to a column, or make it static with None.
    pub fn set(&mut self, key: &str, param: Option<ParamId>) {
        match param {
            Some(p) => self.0.insert(key.to_string(), p),
            None => self.0.remove(key),
        };
    }

    pub fn references(&self, param: ParamId) -> bool {
        self.0.values().any(|p| *p == param)
    }
}

/// Every spec's value at one frame, in spec order: a bound column with a
/// value at the frame wins, else the stored value, else the spec default.
/// The one resolve law of Effect and (GM10) Camera parameters.
pub fn resolve_specs(
    specs: &[ParamSpec],
    params: &FxParams,
    binds: &FxBinds,
    column: impl Fn(ParamId) -> Option<f32>,
) -> Vec<FxValue> {
    specs
        .iter()
        .map(|s| {
            binds
                .get(s.key)
                .filter(|_| s.bindable())
                .and_then(&column)
                .and_then(|x| s.from_column(x, params))
                .unwrap_or_else(|| s.read(params.get(s.key)))
        })
        .collect()
}

/// Every parameter of a kind resolved at one frame, in spec order — what
/// [`FxKind::apply`] reads and what the eval hash spells.
#[derive(Debug, Clone, PartialEq)]
pub struct FxValues {
    pub kind: FxKind,
    pub values: Vec<FxValue>,
}

impl FxValues {
    /// Resolve: a bound column with a value at the frame wins, else the
    /// stored value, else the spec default (`column` reads a param column
    /// at the frame, as `Cut::param_value` does).
    pub fn resolve(
        kind: FxKind,
        params: &FxParams,
        binds: &FxBinds,
        column: impl Fn(ParamId) -> Option<f32>,
    ) -> Self {
        let values = resolve_specs(kind.specs(), params, binds, column);
        Self { kind, values }
    }

    /// The defaults alone (no stored values, no binds).
    pub fn defaults(kind: FxKind) -> Self {
        Self::resolve(kind, &FxParams::default(), &FxBinds::default(), |_| None)
    }

    fn value(&self, key: &str) -> &FxValue {
        let i = self
            .kind
            .specs()
            .iter()
            .position(|s| s.key == key)
            .unwrap_or_else(|| panic!("{} has no parameter '{key}'", self.kind.name()));
        &self.values[i]
    }

    pub fn float(&self, key: &str) -> f32 {
        match self.value(key) {
            FxValue::Float(x) => *x,
            v => panic!("'{key}' is not a number: {v:?}"),
        }
    }

    pub fn color(&self, key: &str) -> [u8; 4] {
        match self.value(key) {
            FxValue::Color(c) => *c,
            v => panic!("'{key}' is not a colour: {v:?}"),
        }
    }

    pub fn flag(&self, key: &str) -> bool {
        match self.value(key) {
            FxValue::Bool(b) => *b,
            v => panic!("'{key}' is not a flag: {v:?}"),
        }
    }

    /// A choice's index into its spec's options.
    pub fn choice(&self, key: &str) -> usize {
        let (FxValue::Choice(s), Some(spec)) = (self.value(key), self.kind.spec(key)) else {
            panic!("'{key}' is not a choice");
        };
        match spec.kind {
            ParamKind::Choice {
                options, default, ..
            } => options.iter().position(|o| o == s).unwrap_or(default),
            _ => 0,
        }
    }

    /// GM13: a file parameter's key into `Cut::blobs` ("" = none).
    pub fn blob(&self, key: &str) -> &str {
        match self.value(key) {
            FxValue::Blob { blob } => blob,
            v => panic!("'{key}' is not a file: {v:?}"),
        }
    }

    /// GM13: a colour map parameter's chip pairs.
    pub fn map(&self, key: &str) -> &[([u8; 4], [u8; 4])] {
        match self.value(key) {
            FxValue::Map { map, .. } => map,
            v => panic!("'{key}' is not a colour map: {v:?}"),
        }
    }

    /// `key=value,...` in spec order: the eval hash's part of the recipe.
    pub fn recipe(&self) -> String {
        self.kind
            .specs()
            .iter()
            .zip(&self.values)
            .map(|(s, v)| format!("{}={v}", s.key))
            .collect::<Vec<_>>()
            .join(",")
    }
}

// ---- The kinds ---------------------------------------------------------------

/// The effect an Effect node runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FxKind {
    Blur,
    Glow,
    Diffusion,
    Grade,
    // GM13: the satsuei look (maths in `crate::look`).
    Para,
    LightWrap,
    Rim,
    Vignette,
    Grain,
    MotionBlur,
    Lut,
    Matte,
    PaletteMap,
    // GM18: the manga screen tone (maths in `crate::manga`).
    Tone,
}

/// GM13: what an effect reads beyond its inputs and values — the frame
/// (Grain's pattern) and the cut's embedded files (a LUT's .cube).
#[derive(Debug, Clone, Copy, Default)]
pub struct FxCtx<'a> {
    pub frame: u32,
    pub blobs: Option<&'a BTreeMap<String, std::sync::Arc<Vec<u8>>>>,
}

/// A Float spec (GM10: the Camera node's specs use it too).
pub(crate) const fn float(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    min: f32,
    max: f32,
    default: f32,
    step: f32,
) -> ParamSpec {
    ParamSpec {
        key,
        label,
        help,
        kind: ParamKind::Float {
            min,
            max,
            default,
            step,
        },
    }
}

const BLUR_SPECS: &[ParamSpec] = &[
    float("radius_x", "radius x", "Horizontal blur radius in pixels (a Gaussian of sigma r/2); 0 leaves it sharp.", 0.0, 500.0, 4.0, 0.1),
    float("radius_y", "radius y", "Vertical blur radius in pixels (a Gaussian of sigma r/2); 0 leaves it sharp.", 0.0, 500.0, 4.0, 0.1),
    ParamSpec {
        key: "repeat_edges",
        label: "repeat edges",
        help: "On: the frame's edge pixels repeat past it, so a full frame keeps its edges. Off: past the edge is transparent.",
        kind: ParamKind::Bool { default: false },
    },
];

const GLOW_SPECS: &[ParamSpec] = &[
    float("threshold", "threshold", "Only colour brighter than this (Rec.709 luma, 0-1) glows.", 0.0, 1.0, 0.7, 0.01),
    float("radius", "radius", "How far the glow spreads, in pixels.", 0.0, 500.0, 16.0, 0.1),
    float("intensity", "intensity", "How strongly the glow is added on top; 0 turns it off.", 0.0, 8.0, 1.0, 0.01),
    ParamSpec {
        key: "tint",
        label: "tint",
        help: "The glow's colour: white keeps the source colour.",
        kind: ParamKind::Color {
            default: [255, 255, 255, 255],
        },
    },
];

const DIFFUSION_SPECS: &[ParamSpec] = &[
    float("radius", "radius", "How soft the diffused copy is, in pixels.", 0.0, 500.0, 12.0, 0.1),
    float("mix", "mix", "How much of the diffused look is mixed in; 0 is the source.", 0.0, 1.0, 0.35, 0.01),
    ParamSpec {
        key: "mode",
        label: "mode",
        help: "Screen lays the soft copy on as a bright haze; overlay deepens the contrast as a diffusion filter does.",
        kind: ParamKind::Choice {
            options: &["screen", "overlay"],
            default: 0,
            // It always ran the Overlay curve; files from before say so.
            renamed: &[("soft light", 1)],
        },
    },
];

const GRADE_SPECS: &[ParamSpec] = &[
    float("lift", "lift", "Raises (or lowers) the shadows while keeping white.", -1.0, 1.0, 0.0, 0.005),
    float("gamma", "gamma", "Bends the midtones: above 1 brightens, below 1 darkens.", 0.1, 10.0, 1.0, 0.01),
    float("gain", "gain", "Scales the whole range: the highlights move most.", 0.0, 8.0, 1.0, 0.01),
    float("saturation", "saturation", "0 is grey, 1 unchanged, above 1 richer colour.", 0.0, 4.0, 1.0, 0.01),
    float("exposure", "exposure", "Brightness in stops: +1 doubles, -1 halves.", -10.0, 10.0, 0.0, 0.01),
    float("temperature", "temperature (warm–cool)", "Warmer (positive) or cooler (negative): red and blue gains.", -1.0, 1.0, 0.0, 0.01),
    float("tint", "tint (magenta–green)", "Magenta (positive) or green (negative): a green gain.", -1.0, 1.0, 0.0, 0.01),
];

pub(crate) const fn color(key: &'static str, label: &'static str, help: &'static str, default: [u8; 4]) -> ParamSpec {
    ParamSpec {
        key,
        label,
        help,
        kind: ParamKind::Color { default },
    }
}

pub(crate) const fn choice(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    options: &'static [&'static str],
) -> ParamSpec {
    ParamSpec {
        key,
        label,
        help,
        kind: ParamKind::Choice {
            options,
            default: 0,
            renamed: &[],
        },
    }
}

pub(crate) const fn flag(key: &'static str, label: &'static str, help: &'static str, default: bool) -> ParamSpec {
    ParamSpec {
        key,
        label,
        help,
        kind: ParamKind::Bool { default },
    }
}

const PARA_SPECS: &[ParamSpec] = &[
    choice("shape", "shape", "Linear runs from colour A to colour B along the angle; radial runs from A at the centre out to B.", &["linear", "radial"]),
    color("color_a", "colour A", "The gradient's start: the top (linear, at 90) or the centre (radial).", [255, 214, 170, 255]),
    color("color_b", "colour B", "The gradient's end: the bottom (linear, at 90) or the rim (radial).", [96, 104, 168, 255]),
    float("angle", "angle", "Linear: the direction from A to B in degrees; 90 is top to bottom, 0 left to right.", -360.0, 360.0, 90.0, 0.5),
    float("center_x", "centre x", "Radial: the centre across the frame; 0 is the left edge, 1 the right.", -1.0, 2.0, 0.5, 0.005),
    float("center_y", "centre y", "Radial: the centre down the frame; 0 is the top edge, 1 the bottom.", -1.0, 2.0, 0.5, 0.005),
    float("radius", "radius", "Radial: where colour B is reached, as a share of the half-diagonal.", 0.01, 4.0, 1.0, 0.005),
    choice("mode", "mode", "Multiply shades, screen lights, overlay deepens the contrast.", &["multiply", "screen", "overlay"]),
    float("amount", "amount", "How strongly the para is laid on; 0 leaves the image as it is.", 0.0, 1.0, 0.5, 0.01),
];

const LIGHT_WRAP_SPECS: &[ParamSpec] = &[
    float("radius", "radius", "How far the background's light reaches into the character, in pixels.", 0.0, 200.0, 12.0, 0.1),
    float("intensity", "intensity", "How strongly the wrapped light is added; 0 turns it off.", 0.0, 4.0, 0.6, 0.01),
];

const RIM_SPECS: &[ParamSpec] = &[
    float("angle", "light from", "Where the light comes from, in degrees: 0 is the right, 90 above.", -360.0, 360.0, 45.0, 0.5),
    float("width", "width", "How deep the rim reaches in from the edge, in pixels.", 0.0, 64.0, 3.0, 0.1),
    float("softness", "softness", "Blurs the rim, in pixels; 0 keeps it hard.", 0.0, 64.0, 1.0, 0.1),
    float("intensity", "intensity", "How strongly the rim light is added; 0 turns it off.", 0.0, 4.0, 1.0, 0.01),
    color("color", "colour", "The rim light's colour.", [255, 246, 224, 255]),
];

const VIGNETTE_SPECS: &[ParamSpec] = &[
    float("amount", "amount", "How far the corners fade; 0 turns it off.", 0.0, 1.0, 0.35, 0.01),
    float("radius", "radius", "Where the fade starts, as a share of the half-diagonal.", 0.0, 2.0, 0.6, 0.005),
    float("softness", "softness", "How gradually it fades past the radius (a share of the half-diagonal).", 0.01, 2.0, 0.6, 0.005),
    color("color", "colour", "What the corners fade toward.", [0, 0, 0, 255]),
];

const GRAIN_SPECS: &[ParamSpec] = &[
    float("amount", "amount", "How strong the grain is; 0 turns it off.", 0.0, 1.0, 0.04, 0.001),
    float("size", "size", "One grain's size in pixels.", 1.0, 16.0, 1.0, 0.05),
    float("seed", "seed", "Another seed gives another grain, just as steady from frame to frame.", 0.0, 9999.0, 0.0, 1.0),
    flag("mono", "mono", "On: one grain for all three channels. Off: each channel its own.", true),
];

const MOTION_BLUR_SPECS: &[ParamSpec] = &[
    float("length", "length", "How far the image smears, in pixels; 0 leaves it sharp.", 0.0, 500.0, 12.0, 0.1),
    float("angle", "angle", "The smear's direction in degrees: 0 is across, 90 up and down.", -360.0, 360.0, 0.0, 0.5),
];

const LUT_SPECS: &[ParamSpec] = &[
    ParamSpec {
        key: "lut",
        label: "LUT file",
        help: "The .cube 3D LUT, stored inside the project so the shot renders on its own.",
        kind: ParamKind::Blob,
    },
    float("mix", "mix", "How much of the LUT's look is mixed in; 0 is the source.", 0.0, 1.0, 1.0, 0.01),
];

const MATTE_SPECS: &[ParamSpec] = &[
    choice("channel", "matte from", "The matte input's (else the image's own) alpha, or its luma.", &["alpha", "luma"]),
    flag("invert", "invert", "On: keeps what the matte leaves out.", false),
    float("choke", "choke / spread", "Shrinks (positive) or grows (negative) the matte, in pixels.", -64.0, 64.0, 0.0, 0.1),
    flag("white", "output matte", "On: outputs the matte itself, white, to drive a grade or a blend. Off: cuts the image by it.", false),
];

const PALETTE_MAP_SPECS: &[ParamSpec] = &[ParamSpec {
    key: "map",
    label: "variant",
    help: "The palette variant whose colours replace the model's: each exact chip colour becomes the variant's.",
    kind: ParamKind::ColorMap,
}];

const TONE_SPECS: &[ParamSpec] = &[
    float("lpi", "lines per inch", "The screen's line count along its angle; 60 is the common manga tone.", 10.0, 200.0, 60.0, 1.0),
    float("angle", "angle", "The dot rows' angle in degrees; 45 is the common screen angle.", -360.0, 360.0, 45.0, 0.5),
    choice("shape", "dot shape", "Round dots, square dots, or lines along the angle.", &["round", "square", "line"]),
    float("dpi", "page dpi", "The page's dots per inch, so the line count is true on paper; 350 is the manga standard.", 72.0, 1200.0, 350.0, 1.0),
    color("color", "ink", "The tone's ink.", [0, 0, 0, 255]),
    flag("mono", "1-bit", "On: every pixel is ink or nothing, as the print plate is. Off: the dots' edges are smoothed.", true),
];

impl FxKind {
    /// GM9's kinds: the add menu's FX category.
    pub const CORE: &'static [FxKind] = &[FxKind::Blur, FxKind::Glow, FxKind::Diffusion, FxKind::Grade];

    /// GM13's satsuei kinds: the add menu's Look category.
    pub const LOOK: &'static [FxKind] = &[
        FxKind::Para,
        FxKind::LightWrap,
        FxKind::Rim,
        FxKind::Vignette,
        FxKind::Grain,
        FxKind::MotionBlur,
        FxKind::Lut,
        FxKind::Matte,
        FxKind::PaletteMap,
    ];

    /// GM18's kinds: the add menu's Manga category.
    pub const MANGA: &'static [FxKind] = &[FxKind::Tone];

    pub const ALL: &'static [FxKind] = &[
        FxKind::Blur,
        FxKind::Glow,
        FxKind::Diffusion,
        FxKind::Grade,
        FxKind::Para,
        FxKind::LightWrap,
        FxKind::Rim,
        FxKind::Vignette,
        FxKind::Grain,
        FxKind::MotionBlur,
        FxKind::Lut,
        FxKind::Matte,
        FxKind::PaletteMap,
        FxKind::Tone,
    ];

    /// The node's name in the graph and the add-node menu.
    pub fn name(self) -> &'static str {
        match self {
            Self::Blur => "Blur",
            Self::Glow => "Glow",
            Self::Diffusion => "Diffusion",
            Self::Grade => "Grade",
            Self::Para => "Para",
            Self::LightWrap => "Light wrap",
            Self::Rim => "Rim light",
            Self::Vignette => "Vignette",
            Self::Grain => "Grain",
            Self::MotionBlur => "Motion blur",
            Self::Lut => "LUT",
            Self::Matte => "Matte",
            Self::PaletteMap => "Palette map",
            Self::Tone => "Tone",
        }
    }

    /// One plain sentence for menus and tooltips.
    pub fn help(self) -> &'static str {
        match self {
            Self::Blur => "Softens the image.",
            Self::Glow => "Adds a glow around the bright parts.",
            Self::Diffusion => "Mixes in a soft copy of the image.",
            Self::Grade => "Changes brightness, contrast, saturation, exposure, temperature and tint.",
            Self::Para => "Lays a two-color gradient over the image: light or shade across the shot.",
            Self::LightWrap => "Bleeds the background's light into the edges of the character.",
            Self::Rim => "A rim of light along the edges that face the light.",
            Self::Vignette => "Fades the corners of the frame toward a color.",
            Self::Grain => "Film grain: steady for a frame and seed, different on the next frame.",
            Self::MotionBlur => "Smears the image along a direction: speed and camera shake.",
            Self::Lut => "Changes colors through a .cube color table stored in the project.",
            Self::Matte => "Masks the image by its transparency or brightness, with invert and spread.",
            Self::PaletteMap => "Swaps the palette's colors for a variant's: day, evening, night.",
            Self::Tone => "Screen tone: the image's grey as a pattern of dots, by density and angle.",
        }
    }

    /// The studio's own word (romaji), shown beside the name in the
    /// add-node menu; None where the English is the trade word.
    pub fn term(self) -> Option<&'static str> {
        match self {
            Self::Blur => Some("bokashi"),
            Self::Glow => Some("tōkakō"),
            Self::Diffusion => Some("difyūjon"),
            // GM13: the satsuei desk's words; Para is its own trade word.
            Self::LightWrap => Some("raito rappu"),
            Self::Rim => Some("rimu raito"),
            Self::Grain => Some("noizu"),
            Self::MotionBlur => Some("nagashi (bure)"),
            Self::Tone => Some("tōn (amiten)"),
            Self::Grade
            | Self::Para
            | Self::Vignette
            | Self::Lut
            | Self::Matte
            | Self::PaletteMap => None,
        }
    }

    /// Search words for the add-node menu (romaji, as typed with or
    /// without long marks, and trade words).
    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Blur => &["bokashi", "gaussian", "soften"],
            Self::Glow => &[
                "tōkakō",
                "toukakou",
                "tokakou",
                "tokako",
                "t-kō",
                "t-kou",
                "tkou",
                "transmitted light",
                "light",
                "bloom",
            ],
            Self::Diffusion => &[
                "difyūjon",
                "difyujon",
                "diffusion",
                "diffusion filter",
                "soft focus",
                "haze",
                "soft light",
            ],
            Self::Grade => &["grade", "colour", "color", "correction", "levels", "exposure"],
            Self::Para => &["para", "gradient", "shade", "light", "satsuei"],
            Self::LightWrap => &["wrap", "raito rappu", "edge light", "background light", "satsuei"],
            Self::Rim => &["rim", "rimu raito", "edge light", "backlight"],
            Self::Vignette => &["corners", "edge darkening", "lens"],
            Self::Grain => &["noise", "noizu", "film grain", "texture"],
            Self::MotionBlur => &["directional blur", "speed", "smear", "camera shake", "nagashi", "bure"],
            Self::Lut => &["lut", "cube", ".cube", "look", "grade", "colour", "color"],
            Self::Matte => &["matte", "mask", "key", "choke", "spread", "cut out"],
            Self::PaletteMap => &["variant", "time of day", "evening", "night", "recolour", "recolor"],
            Self::Tone => &["screentone", "screen tone", "tōn", "ton", "amiten", "halftone", "dots", "manga"],
        }
    }

    /// GM13: does the output change from frame to frame by itself (Grain)?
    /// The eval recipe then spells the frame.
    pub fn uses_frame(self) -> bool {
        matches!(self, Self::Grain)
    }

    /// Input pins: the image, and for Glow an optional matte.
    pub fn inputs(self) -> usize {
        self.input_names().len()
    }

    pub fn input_names(self) -> &'static [&'static str] {
        match self {
            Self::Glow | Self::Para | Self::Matte => &["image", "matte"],
            Self::LightWrap => &["image", "background", "matte"],
            _ => &["image"],
        }
    }

    /// What each input pin takes (its tooltip in the graph), by pin.
    pub fn input_help(self) -> &'static [&'static str] {
        match self {
            Self::Glow => &[
                "the picture that glows",
                "optional; only where this is opaque glows",
            ],
            Self::Para => &[
                "the picture the para lies on",
                "optional; only where this is opaque gets the para",
            ],
            Self::Matte => &[
                "the picture to cut",
                "optional; the matte (else the picture's own)",
            ],
            Self::LightWrap => &[
                "the picture with the character in it",
                "the background whose light wraps in",
                "optional; the character whose edges take the light (else the picture's alpha)",
            ],
            _ => &["the picture the effect works on"],
        }
    }

    pub fn specs(self) -> &'static [ParamSpec] {
        match self {
            Self::Blur => BLUR_SPECS,
            Self::Glow => GLOW_SPECS,
            Self::Diffusion => DIFFUSION_SPECS,
            Self::Grade => GRADE_SPECS,
            Self::Para => PARA_SPECS,
            Self::LightWrap => LIGHT_WRAP_SPECS,
            Self::Rim => RIM_SPECS,
            Self::Vignette => VIGNETTE_SPECS,
            Self::Grain => GRAIN_SPECS,
            Self::MotionBlur => MOTION_BLUR_SPECS,
            Self::Lut => LUT_SPECS,
            Self::Matte => MATTE_SPECS,
            Self::PaletteMap => PALETTE_MAP_SPECS,
            Self::Tone => TONE_SPECS,
        }
    }

    pub fn spec(self, key: &str) -> Option<&'static ParamSpec> {
        self.specs().iter().find(|s| s.key == key)
    }

    /// Run the effect on its inputs (one per pin; None = unwired, which for
    /// the image is a transparent `w × h` frame). `law` encodes colour
    /// parameters as every chip is encoded (D8).
    pub fn apply(
        self,
        v: &FxValues,
        law: ColorLaw,
        inputs: &[Option<&Image>],
        w: u32,
        h: u32,
    ) -> Image {
        self.apply_in(v, law, inputs, w, h, FxCtx::default())
    }

    /// [`Self::apply`] at a frame, with the cut's embedded files (GM13:
    /// Grain reads the frame, a LUT its file).
    pub fn apply_in(
        self,
        v: &FxValues,
        law: ColorLaw,
        inputs: &[Option<&Image>],
        w: u32,
        h: u32,
        ctx: FxCtx<'_>,
    ) -> Image {
        let empty;
        let src = match inputs.first().copied().flatten() {
            Some(img) => img,
            None => {
                empty = Image::new(w, h);
                &empty
            }
        };
        let pin = |i: usize| inputs.get(i).copied().flatten();
        match self {
            Self::Blur => blur_edges(
                src,
                v.float("radius_x"),
                v.float("radius_y"),
                v.flag("repeat_edges"),
            ),
            Self::Glow => glow(v, law, src, pin(1)),
            Self::Diffusion => diffusion(v, src),
            Self::Grade => grade(v, src),
            Self::Para => look::para(v, law, src, pin(1)),
            Self::LightWrap => look::light_wrap(v, src, pin(1), pin(2)),
            Self::Rim => look::rim(v, law, src),
            Self::Vignette => look::vignette(v, law, src),
            Self::Grain => look::grain(v, src, ctx.frame),
            Self::MotionBlur => look::directional_blur(src, v.float("length"), v.float("angle")),
            Self::Lut => {
                let file = ctx.blobs.and_then(|b| b.get(v.blob("lut")));
                look::lut_fx(v, law, src, file)
            }
            Self::Matte => look::matte(v, src, pin(1)),
            Self::PaletteMap => look::palette_map(src, law, v.map("map")),
            Self::Tone => crate::manga::tone(v, law, src),
        }
    }
}

// ---- Blur ----------------------------------------------------------------------

/// Boxes-for-Gauss: the odd widths of `n` box passes whose combined
/// variance approximates a Gaussian of `sigma` (Kutskir, after W. Jarosz):
/// `w_ideal = sqrt(12σ²/n + 1)`, `wl` the odd floor, `wu = wl + 2`, and the
/// first `m = round((12σ² − n·wl² − 4n·wl − 3n)/(−4wl − 4))` passes take
/// `wl`. σ = 0 gives all ones (the identity).
pub fn box_sizes(sigma: f32, n: usize) -> Vec<usize> {
    let s2 = (sigma.max(0.0) as f64).powi(2);
    let nf = n as f64;
    let w_ideal = (12.0 * s2 / nf + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let wlf = wl as f64;
    let m_ideal = (12.0 * s2 - nf * wlf * wlf - 4.0 * nf * wlf - 3.0 * nf) / (-4.0 * wlf - 4.0);
    let m = (m_ideal.round() as i64).clamp(0, n as i64);
    (0..n as i64)
        .map(|i| if i < m { wl as usize } else { wu as usize })
        .collect()
}

/// One horizontal box pass of radius `r` over every row: a running sum (f64,
/// so adding and dropping samples leaves no drift), divided by the full
/// width `2r + 1` — samples outside the row are transparent, or with
/// `repeat` the row's end pixel.
fn box_rows(src: &[f32], w: usize, r: usize, repeat: bool) -> Vec<f32> {
    if r == 0 || w == 0 {
        return src.to_vec();
    }
    let inv = 1.0 / (2 * r + 1) as f64;
    let (wi, ri) = (w as isize, r as isize);
    let mut out = vec![0.0f32; src.len()];
    out.par_chunks_mut(w * 4)
        .zip(src.par_chunks(w * 4))
        .for_each(|(o, s)| {
            // Sample k of the row, outside it included.
            let at = |k: isize, c: usize| -> f64 {
                if (0..wi).contains(&k) {
                    s[k as usize * 4 + c] as f64
                } else if repeat {
                    s[k.clamp(0, wi - 1) as usize * 4 + c] as f64
                } else {
                    0.0
                }
            };
            let mut acc = [0.0f64; 4];
            for k in -ri..ri {
                for c in 0..4 {
                    acc[c] += at(k, c);
                }
            }
            for x in 0..wi {
                for c in 0..4 {
                    acc[c] += at(x + ri, c);
                }
                for c in 0..4 {
                    o[x as usize * 4 + c] = (acc[c] * inv) as f32;
                }
                for c in 0..4 {
                    acc[c] -= at(x - ri, c);
                }
            }
        });
    out
}

/// `w × h` → `h × w` (the vertical passes run as rows of the transpose).
fn transpose(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; src.len()];
    if w == 0 || h == 0 {
        return out;
    }
    out.par_chunks_mut(h * 4).enumerate().for_each(|(x, col)| {
        for y in 0..h {
            let s = (y * w + x) * 4;
            col[y * 4..y * 4 + 4].copy_from_slice(&src[s..s + 4]);
        }
    });
    out
}

/// Gaussian blur with radius `rx` across and `ry` down (σ = r/2 each), as
/// three box passes per axis. A radius of 0 (or not a number) leaves that
/// axis untouched. Outside the image is transparent.
pub fn blur(img: &Image, rx: f32, ry: f32) -> Image {
    blur_edges(img, rx, ry, false)
}

/// [`blur`], with `repeat` sampling past the edges as the edge pixel (the
/// Blur node's "repeat edges").
pub fn blur_edges(img: &Image, rx: f32, ry: f32, repeat: bool) -> Image {
    let (w, h) = (img.w as usize, img.h as usize);
    let mut px = img.px.clone();
    if rx.is_finite() && rx > 0.0 {
        for b in box_sizes(rx / 2.0, 3) {
            px = box_rows(&px, w, (b - 1) / 2, repeat);
        }
    }
    if ry.is_finite() && ry > 0.0 {
        let mut t = transpose(&px, w, h);
        for b in box_sizes(ry / 2.0, 3) {
            t = box_rows(&t, h, (b - 1) / 2, repeat);
        }
        px = transpose(&t, h, w);
    }
    Image {
        w: img.w,
        h: img.h,
        px,
    }
}

// ---- Glow, diffusion, grade ------------------------------------------------------

/// Rec.709 luma of a premultiplied pixel's unpremultiplied colour (0 when
/// transparent).
fn luma_unpremult(p: &[f32]) -> f32 {
    if p[3] <= 0.0 {
        return 0.0;
    }
    (LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2]) / p[3]
}

fn glow(v: &FxValues, law: ColorLaw, src: &Image, matte: Option<&Image>) -> Image {
    let threshold = v.float("threshold");
    let radius = v.float("radius");
    let intensity = v.float("intensity");
    let tint = law.encode_rgba(v.color("tint"));
    let span = 1.0 - threshold;
    if intensity <= 0.0 || span <= 0.0 {
        return src.clone();
    }
    let w = src.w as usize;
    let mut bright = Image::new(src.w, src.h);
    bright
        .px
        .par_chunks_mut(w.max(1) * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, b) in row.chunks_exact_mut(4).enumerate() {
                let i = (y * w + x) * 4;
                let p = &src.px[i..i + 4];
                let mut k = (luma_unpremult(p) - threshold).max(0.0) / span;
                if let Some(m) = matte {
                    k *= m.px.get(i + 3).copied().unwrap_or(0.0).clamp(0.0, 1.0);
                }
                if k > 0.0 {
                    for c in 0..3 {
                        b[c] = p[c] * k * tint[c];
                    }
                    b[3] = p[3] * k;
                }
            }
        });
    if bright.px.iter().all(|&x| x == 0.0) {
        // Nothing above the threshold: the input, exactly.
        return src.clone();
    }
    let halo = blur(&bright, radius, radius);
    let mut out = src.clone();
    out.px
        .par_iter_mut()
        .zip(halo.px.par_iter())
        .for_each(|(o, g)| *o += g * intensity);
    out
}

fn diffusion(v: &FxValues, src: &Image) -> Image {
    let radius = v.float("radius");
    let mix = v.float("mix");
    if mix <= 0.0 {
        return src.clone();
    }
    let soft = blur(src, radius, radius);
    let mut lit = src.px.clone();
    let mode = if v.choice("mode") == 0 {
        BlendMode::Screen
    } else {
        BlendMode::Overlay
    };
    crate::export::blend_into(&mut lit, &soft.px, mode);
    let mut out = src.clone();
    out.px
        .par_iter_mut()
        .zip(lit.par_iter())
        .for_each(|(o, l)| *o += (l - *o) * mix);
    out
}

fn grade(v: &FxValues, src: &Image) -> Image {
    let lift = v.float("lift");
    let inv_gamma = 1.0 / v.float("gamma");
    let gain = v.float("gain");
    let sat = v.float("saturation");
    let ex = v.float("exposure").exp2();
    let temp = v.float("temperature");
    let tint = v.float("tint");
    let gains = [
        1.0 + temp * TEMP_GAIN,
        1.0 - tint * TEMP_GAIN,
        1.0 - temp * TEMP_GAIN,
    ];
    let mut out = src.clone();
    out.px.par_chunks_mut(4).for_each(|p| {
        let a = p[3];
        if a <= 0.0 {
            return; // transparent stays transparent
        }
        let mut c = [p[0] / a, p[1] / a, p[2] / a];
        for ch in &mut c {
            *ch = (gain * (*ch + lift * (1.0 - *ch))).max(0.0).powf(inv_gamma) * ex;
        }
        if sat != 1.0 {
            let l = LUMA[0] * c[0] + LUMA[1] * c[1] + LUMA[2] * c[2];
            for ch in &mut c {
                *ch = l + (*ch - l) * sat;
            }
        }
        for (ch, g) in c.iter_mut().zip(gains) {
            *ch *= g;
        }
        for i in 0..3 {
            p[i] = c[i] * a;
        }
    });
    out
}
