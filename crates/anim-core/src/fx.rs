//! Compositing FX (GM9, D9): blur, glow (T-kō), diffusion (tōkakō and the
//! soft diffusion filter) and grade, implemented ONCE, here, on premultiplied
//! f32 RGBA frames. The CPU golden renderer (`export`'s Effect arm) calls
//! [`FxKind::apply`]; the GPU compositor never re-implements them — it renders
//! an FX subtree through this module on a worker and uploads the result
//! (NEVER-DO 2), so the composite view, the viewer and export agree.
//!
//! Every kind declares its parameters as [`ParamSpec`]s: the node graph's
//! inspector is generated from them, a stored value that is missing takes the
//! spec default, and every number can be bound to an X-sheet param column (the
//! Transform binds' law). Later parts add FX by declaring specs.
//!
//! Formulas (pinned by tests/fx_tests.rs — change them together):
//! - **Blur** radius r: a Gaussian with σ = r/2, done as three separable box
//!   passes whose widths come from boxes-for-Gauss ([`box_sizes`]). Each pass
//!   is a running sum, O(pixels) whatever r is. r = 0 is the identity;
//!   outside the image is transparent.
//! - **Glow**: `bright = max(luma(unpremult) − threshold, 0)/(1 − threshold)`,
//!   which multiplies the pixel (premultiplied, so coverage carries) and the
//!   tint; `glow = blur(bright, radius) × intensity`; `out = input + glow`
//!   (Add, clamped only at the finishers). A wired matte input multiplies
//!   `bright` by the matte's alpha.
//! - **Diffusion**: `d = blur(input, radius)`;
//!   `out = mix(input, screen(input, d), mix)`. Soft light takes the Overlay
//!   blend's curve instead of screen. Both are `export`'s blend formulas.
//! - **Grade**, on unpremultiplied colour:
//!   `c = pow(max(0, gain·(c + lift·(1−c))), 1/gamma) · 2^exposure`; then
//!   saturation mixes with Rec.709 luma; then temperature and tint are
//!   per-channel gains ([`TEMP_GAIN`]); then it premultiplies again.

use std::collections::BTreeMap;

use rayon::prelude::*;

use crate::color::ColorLaw;
use crate::graph::BlendMode;
use crate::ids::ParamId;

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
    Choice {
        options: &'static [&'static str],
        default: usize,
    },
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
            ParamKind::Choice { options, default } => FxValue::Choice(options[default].to_string()),
        }
    }

    /// Can a param column drive it? A number, a flag or a choice can; a
    /// colour has no single number to key.
    pub fn bindable(&self) -> bool {
        !matches!(self.kind, ParamKind::Color { .. })
    }

    /// A stored value read through this spec: missing, wrong-typed or
    /// non-finite → the default; a number is clamped into range; an unknown
    /// choice name → the default.
    pub fn read(&self, v: Option<&FxValue>) -> FxValue {
        match (self.kind, v) {
            (ParamKind::Float { min, max, .. }, Some(FxValue::Float(x))) if x.is_finite() => {
                FxValue::Float(x.clamp(min, max))
            }
            (ParamKind::Color { .. }, Some(FxValue::Color(c))) => FxValue::Color(*c),
            (ParamKind::Bool { .. }, Some(FxValue::Bool(b))) => FxValue::Bool(*b),
            (ParamKind::Choice { options, .. }, Some(FxValue::Choice(s)))
                if options.contains(&s.as_str()) =>
            {
                FxValue::Choice(s.clone())
            }
            _ => self.default_value(),
        }
    }

    /// A bound column's value read through this spec: a number clamped into
    /// range, a flag on at 0.5 and above, a choice by its rounded index.
    /// None for a colour or a non-finite value (the stored value stands).
    pub fn from_column(&self, x: f32) -> Option<FxValue> {
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
            ParamKind::Color { .. } => None,
        }
    }
}

/// One stored parameter value (untagged JSON: a number, a bool, an RGBA
/// array or a choice name).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FxValue {
    Bool(bool),
    Float(f32),
    Color([u8; 4]),
    Choice(String),
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
        let values = kind
            .specs()
            .iter()
            .map(|s| {
                binds
                    .get(s.key)
                    .filter(|_| s.bindable())
                    .and_then(&column)
                    .and_then(|x| s.from_column(x))
                    .unwrap_or_else(|| s.read(params.get(s.key)))
            })
            .collect();
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
            ParamKind::Choice { options, default } => {
                options.iter().position(|o| o == s).unwrap_or(default)
            }
            _ => 0,
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
}

const fn float(
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
        help: "Screen brightens like transmitted light; soft light adds contrast like a diffusion filter.",
        kind: ParamKind::Choice {
            options: &["screen", "soft light"],
            default: 0,
        },
    },
];

const GRADE_SPECS: &[ParamSpec] = &[
    float("lift", "lift", "Raises (or lowers) the shadows while keeping white.", -1.0, 1.0, 0.0, 0.005),
    float("gamma", "gamma", "Bends the midtones: above 1 brightens, below 1 darkens.", 0.1, 10.0, 1.0, 0.01),
    float("gain", "gain", "Scales the whole range: the highlights move most.", 0.0, 8.0, 1.0, 0.01),
    float("saturation", "saturation", "0 is grey, 1 unchanged, above 1 richer colour.", 0.0, 4.0, 1.0, 0.01),
    float("exposure", "exposure", "Brightness in stops: +1 doubles, -1 halves.", -10.0, 10.0, 0.0, 0.01),
    float("temperature", "temperature", "Warmer (positive) or cooler (negative): red and blue gains.", -1.0, 1.0, 0.0, 0.01),
    float("tint", "tint", "Magenta (positive) or green (negative): a green gain.", -1.0, 1.0, 0.0, 0.01),
];

impl FxKind {
    pub const ALL: &'static [FxKind] = &[FxKind::Blur, FxKind::Glow, FxKind::Diffusion, FxKind::Grade];

    /// The node's name in the graph and the add-node menu.
    pub fn name(self) -> &'static str {
        match self {
            Self::Blur => "Blur",
            Self::Glow => "Glow",
            Self::Diffusion => "Diffusion",
            Self::Grade => "Grade",
        }
    }

    /// One plain sentence for menus and tooltips.
    pub fn help(self) -> &'static str {
        match self {
            Self::Blur => "Softens the image with a Gaussian blur (bokashi).",
            Self::Glow => "Adds a glow around the bright parts (T-kou).",
            Self::Diffusion => "Mixes in a soft copy for transmitted light or a diffusion filter (tokako).",
            Self::Grade => "Colour grade: lift, gamma, gain, saturation, exposure, temperature and tint.",
        }
    }

    /// Search words for the add-node menu (romaji and trade words).
    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Blur => &["bokashi", "gaussian", "soften"],
            Self::Glow => &["t-kou", "tkou", "light", "bloom"],
            Self::Diffusion => &["tokako", "transmitted light", "soft focus", "haze"],
            Self::Grade => &["colour", "color", "correction", "levels", "exposure"],
        }
    }

    /// Input pins: the image, and for Glow an optional matte.
    pub fn inputs(self) -> usize {
        self.input_names().len()
    }

    pub fn input_names(self) -> &'static [&'static str] {
        match self {
            Self::Glow => &["image", "matte"],
            _ => &["image"],
        }
    }

    pub fn specs(self) -> &'static [ParamSpec] {
        match self {
            Self::Blur => BLUR_SPECS,
            Self::Glow => GLOW_SPECS,
            Self::Diffusion => DIFFUSION_SPECS,
            Self::Grade => GRADE_SPECS,
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
        let empty;
        let src = match inputs.first().copied().flatten() {
            Some(img) => img,
            None => {
                empty = Image::new(w, h);
                &empty
            }
        };
        match self {
            Self::Blur => blur(src, v.float("radius_x"), v.float("radius_y")),
            Self::Glow => glow(v, law, src, inputs.get(1).copied().flatten()),
            Self::Diffusion => diffusion(v, src),
            Self::Grade => grade(v, src),
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
/// width `2r + 1` — samples outside the row are transparent.
fn box_rows(src: &[f32], w: usize, r: usize) -> Vec<f32> {
    if r == 0 || w == 0 {
        return src.to_vec();
    }
    let inv = 1.0 / (2 * r + 1) as f64;
    let mut out = vec![0.0f32; src.len()];
    out.par_chunks_mut(w * 4)
        .zip(src.par_chunks(w * 4))
        .for_each(|(o, s)| {
            let mut acc = [0.0f64; 4];
            for k in 0..r.min(w) {
                for c in 0..4 {
                    acc[c] += s[k * 4 + c] as f64;
                }
            }
            for x in 0..w {
                if x + r < w {
                    for c in 0..4 {
                        acc[c] += s[(x + r) * 4 + c] as f64;
                    }
                }
                for c in 0..4 {
                    o[x * 4 + c] = (acc[c] * inv) as f32;
                }
                if x >= r {
                    for c in 0..4 {
                        acc[c] -= s[(x - r) * 4 + c] as f64;
                    }
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
/// axis untouched.
pub fn blur(img: &Image, rx: f32, ry: f32) -> Image {
    let (w, h) = (img.w as usize, img.h as usize);
    let mut px = img.px.clone();
    if rx.is_finite() && rx > 0.0 {
        for b in box_sizes(rx / 2.0, 3) {
            px = box_rows(&px, w, (b - 1) / 2);
        }
    }
    if ry.is_finite() && ry > 0.0 {
        let mut t = transpose(&px, w, h);
        for b in box_sizes(ry / 2.0, 3) {
            t = box_rows(&t, h, (b - 1) / 2);
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
