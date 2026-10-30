//! GM13 (D9, D15): the satsuei look — role passes, the trace-line absorb
//! (iro-tore, 色トレス), para, light wrap, rim light, vignette, grain,
//! directional motion blur, .cube LUTs, mattes and palette maps — and the
//! one-click satsuei graph. Implemented once, here, on premultiplied f32
//! frames and the cel's f16 tiles: the Effect node runs them through `fx`,
//! and every view renders them on the CPU (NEVER-DO 2).
//!
//! Formulas (pinned by tests/look_tests.rs — change them together):
//! - **Role pass** ([`filter_roles`]): `Drawing::flatten` of only the chosen
//!   roles' layers — the same arithmetic.
//! - **Absorb** ([`absorb_trace_lines`]): each ShadowLine / HighlightLine
//!   pixel takes the straight colour of the paint (Color, Shadow, Highlight
//!   flattened) under it; where there is none, of the nearest paint pixel
//!   by the chamfer 3-4 distance within `r` px (ties: the upper row, then
//!   the left). Its own coverage stays. With no paint in reach it is kept.
//! - **Para**: `t` along the angle across the frame (linear) or out from the
//!   centre over `radius` × the half-diagonal (radial), clamped to 0–1;
//!   `g = mix(A, B, t)`; on unpremultiplied colour
//!   `c += (blend(c, g) − c) · amount · g.a · matte`, coverage kept. The
//!   blends are `export`'s Multiply, Screen and Overlay curves.
//! - **Light wrap**: `m` = the matte's (else the image's) alpha;
//!   `edge = m · (1 − blur(m, r))`; `out.rgb += blur(bg, r).rgb · edge · intensity`.
//! - **Rim**: `rim = a(p) · (1 − a(p + width · toward the light))`, blurred
//!   by the softness and cut back to the alpha; `out.rgb += colour · rim · intensity`.
//! - **Vignette**: `v = amount · colour.a · smoothstep((d − radius)/softness)`,
//!   `d` the distance from the centre over the half-diagonal;
//!   `out.rgb += (colour · a − out.rgb) · v`.
//! - **Grain**: `n ∈ [−1, 1)` hashed from the grain cell, the frame, the seed
//!   (and the channel unless mono); `out.rgb = max(0, out.rgb + n · amount · a)`.
//! - **Motion blur**: the mean of `n = ⌈L⌉ + 1` bilinear taps (`export`'s one
//!   sampler) spaced `L/(n − 1)` along θ, centred on the pixel. L = 0 is the
//!   identity.
//! - **LUT**: trilinear in the .cube lattice on unpremultiplied
//!   display-encoded colour, mixed by `mix`; no file, or one that does not
//!   parse, leaves the image as it is.
//! - **Matte**: `m` = the alpha or luma of the matte input (else of the
//!   image), inverted, then choked (min) or spread (max) over a square of
//!   |choke| px. Out: the image times `m` (by its own matte: its colour
//!   re-covered by `m`), or `m` itself as white.
//! - **Palette map**: a pixel bit-equal to a chip's hard-fill texel becomes
//!   the paired chip's texel; every other pixel is untouched.

use std::collections::BTreeMap;
use std::sync::Arc;

use rayon::prelude::*;

use crate::color::{ColorLaw, is_paint_role};
use crate::command::{Command, CutRef};
use crate::fx::{self, FxKind, FxValue, FxValues, Image, LUMA, ParamKind};
use crate::graph::{BlendMode, NodeKind, PlaneSpec, place_node};
use crate::ids::{ColumnId, NodeId};
use crate::model::{Cut, Drawing, LayerRole};
use crate::raster::{TILE, TileCoord, TileData, f16_bits_to_f32, f32_to_f16_bits};
use crate::xsheet::ColumnKind;

/// A flattened cel: tiles by coordinate.
pub type Tiles = BTreeMap<TileCoord, Arc<TileData>>;

/// How far (px, chamfer) a trace line reaches for paint when none is
/// under it.
pub const ABSORB_RADIUS: u32 = 3;

/// The satsuei template's paint pass: the paint, with the trace lines it
/// absorbs.
pub const PAINT_PASS: &[LayerRole] = &[
    LayerRole::Color,
    LayerRole::Shadow,
    LayerRole::Highlight,
    LayerRole::ShadowLine,
    LayerRole::HighlightLine,
];

// ---- Role passes and the trace-line absorb --------------------------------------

/// The roles the absorb recolours: the shadow and highlight trace lines.
pub fn is_trace_role(role: LayerRole) -> bool {
    matches!(role, LayerRole::ShadowLine | LayerRole::HighlightLine)
}

/// A role pass: `d` flattened from only the layers of `roles`.
pub fn filter_roles(d: &Drawing, roles: &[LayerRole]) -> Tiles {
    d.flatten_where(&|role| roles.contains(&role))
}

/// What a DrawingSource renders of `d`: every visible layer, or only
/// `roles`' layers, after the trace-line absorb when `absorb_trace`.
pub fn source_tiles(d: &Drawing, roles: Option<&[LayerRole]>, absorb_trace: bool) -> Tiles {
    let absorbed;
    let d = if absorb_trace {
        absorbed = absorb_trace_lines(d, ABSORB_RADIUS);
        &absorbed
    } else {
        d
    };
    match roles {
        Some(r) => filter_roles(d, r),
        None => d.flatten(),
    }
}

/// The eval recipe's spelling of a role pass (None for a plain source):
/// its roles, the absorb, and the cel's layer roles in stack order.
pub fn pass_recipe(roles: Option<&[LayerRole]>, absorb_trace: bool, d: &Drawing) -> Option<String> {
    if roles.is_none() && !absorb_trace {
        return None;
    }
    let keys = |it: &mut dyn Iterator<Item = LayerRole>| it.map(|r| r.key()).collect::<Vec<_>>().join(",");
    Some(format!(
        "|pass(roles=[{}],absorb={absorb_trace},layers=[{}])",
        roles.map_or_else(|| "*".to_string(), |r| keys(&mut r.iter().copied())),
        keys(&mut d.layers.iter().map(|l| l.props.role)),
    ))
}

/// The trace-line absorb (色トレス): `d` with each ShadowLine and
/// HighlightLine pixel recoloured to the paint under it, else to the
/// nearest paint within `radius` px (chamfer 3-4), keeping its coverage;
/// a line pixel with no paint in reach keeps its colour. Other layers are
/// untouched.
pub fn absorb_trace_lines(d: &Drawing, radius: u32) -> Drawing {
    let mut out = d.clone();
    if !d.layers.iter().any(|l| is_trace_role(l.props.role) && !l.raster.tiles.is_empty()) {
        return out;
    }
    let paint = d.flatten_where(&is_paint_role);
    // Every offset in reach, nearest first (3 per straight step, 4 per
    // diagonal one); ties: the upper row, then the left.
    let r = radius as i32;
    let mut offsets: Vec<(i32, i32, i32)> = Vec::new();
    for dy in -r..=r {
        for dx in -r..=r {
            let (ax, ay) = (dx.abs(), dy.abs());
            let dist = 3 * ax.max(ay) + ax.min(ay);
            if dist <= 3 * r {
                offsets.push((dist, dy, dx));
            }
        }
    }
    offsets.sort_unstable();
    let t = TILE as i32;
    let paint_at = |x: i32, y: i32| -> Option<[f32; 4]> {
        let tile = paint.get(&(x.div_euclid(t), y.div_euclid(t)))?;
        let i = (y.rem_euclid(t) * t + x.rem_euclid(t)) as usize * 4;
        let p = [0, 1, 2, 3].map(|c| f16_bits_to_f32(tile.rgba[i + c]));
        (p[3] > 0.0).then_some(p)
    };
    for layer in out.layers.iter_mut().filter(|l| is_trace_role(l.props.role)) {
        let mut tiles = layer.raster.tiles.clone();
        for (&(tx, ty), tile) in &layer.raster.tiles {
            let mut rgba = tile.rgba.to_vec();
            let mut changed = false;
            for py in 0..TILE {
                for px in 0..TILE {
                    let i = (py * TILE + px) * 4;
                    let a = f16_bits_to_f32(rgba[i + 3]);
                    if a <= 0.0 {
                        continue;
                    }
                    let (x, y) = (tx * t + px as i32, ty * t + py as i32);
                    let Some(p) = offsets.iter().find_map(|&(_, dy, dx)| paint_at(x + dx, y + dy)) else {
                        continue;
                    };
                    for c in 0..3 {
                        rgba[i + c] = f32_to_f16_bits(p[c] / p[3] * a);
                    }
                    changed = true;
                }
            }
            if changed {
                tiles.insert((tx, ty), Arc::new(TileData::from_vec(rgba)));
            }
        }
        layer.raster.tiles = tiles;
    }
    out
}

// ---- The light FX ------------------------------------------------------------------

/// Snap a direction component that is zero but for rounding (cos 90°).
fn snap(v: f32) -> f32 {
    if v.abs() < 1e-6 { 0.0 } else { v }
}

/// The para's blend of a straight backdrop channel `cb` with the
/// gradient's `cs` (`export`'s curves on opaque colour): 0 multiply,
/// 1 screen, 2 overlay.
fn blend_channel(mode: usize, cb: f32, cs: f32) -> f32 {
    match mode {
        0 => cb * cs,
        1 => cb + cs - cb * cs,
        _ => {
            if cb <= 0.5 {
                2.0 * cs * cb
            } else {
                1.0 - 2.0 * (1.0 - cs) * (1.0 - cb)
            }
        }
    }
}

/// The alpha of `img` at pixel `i`, clamped to 0–1 (0 past its end).
fn alpha_at(img: &Image, i: usize) -> f32 {
    img.px.get(i * 4 + 3).copied().unwrap_or(0.0).clamp(0.0, 1.0)
}

pub(crate) fn para(v: &FxValues, law: ColorLaw, src: &Image, matte: Option<&Image>) -> Image {
    let amount = v.float("amount");
    if amount <= 0.0 {
        return src.clone();
    }
    let (ca, cb) = (law.encode_rgba(v.color("color_a")), law.encode_rgba(v.color("color_b")));
    let radial = v.choice("shape") == 1;
    let mode = v.choice("mode");
    let (w, h) = (src.w as f32, src.h as f32);
    let (sin, cos) = v.float("angle").to_radians().sin_cos();
    let (sin, cos) = (snap(sin), snap(cos));
    let extent = ((w * cos).abs() + (h * sin).abs()).max(1e-6);
    let (cx, cy) = (v.float("center_x") * w, v.float("center_y") * h);
    let reach = (v.float("radius") * 0.5 * (w * w + h * h).sqrt()).max(1e-6);
    let width = src.w as usize;
    let mut out = src.clone();
    out.px
        .par_chunks_mut(width.max(1) * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, p) in row.chunks_exact_mut(4).enumerate() {
                let a = p[3];
                if a <= 0.0 {
                    continue;
                }
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = if radial {
                    (px - cx).hypot(py - cy) / reach
                } else {
                    0.5 + ((px - w / 2.0) * cos + (py - h / 2.0) * sin) / extent
                }
                .clamp(0.0, 1.0);
                let g = [0, 1, 2, 3].map(|c| ca[c] + (cb[c] - ca[c]) * t);
                let mut k = amount * g[3];
                if let Some(m) = matte {
                    k *= alpha_at(m, y * width + x);
                }
                if k <= 0.0 {
                    continue;
                }
                for c in 0..3 {
                    let s = p[c] / a;
                    p[c] = (s + (blend_channel(mode, s, g[c]) - s) * k) * a;
                }
            }
        });
    out
}

pub(crate) fn light_wrap(
    v: &FxValues,
    src: &Image,
    background: Option<&Image>,
    matte: Option<&Image>,
) -> Image {
    let intensity = v.float("intensity");
    let Some(bg) = background else {
        return src.clone(); // no background, no light to wrap
    };
    if intensity <= 0.0 {
        return src.clone();
    }
    let radius = v.float("radius");
    let edge_from = matte.unwrap_or(src);
    let mut m = Image::new(src.w, src.h);
    for (i, o) in m.px.chunks_exact_mut(4).enumerate() {
        o[3] = alpha_at(edge_from, i);
    }
    let soft_m = fx::blur(&m, radius, radius);
    let soft_bg = fx::blur(bg, radius, radius);
    let mut out = src.clone();
    out.px
        .par_chunks_mut(4)
        .zip(m.px.par_chunks(4))
        .zip(soft_m.px.par_chunks(4))
        .zip(soft_bg.px.par_chunks(4))
        .for_each(|(((p, mi), sm), light)| {
            let edge = mi[3] * (1.0 - sm[3].clamp(0.0, 1.0));
            if edge > 0.0 {
                for c in 0..3 {
                    p[c] += light[c] * edge * intensity;
                }
            }
        });
    out
}

pub(crate) fn rim(v: &FxValues, law: ColorLaw, src: &Image) -> Image {
    let (width, intensity) = (v.float("width"), v.float("intensity"));
    if width <= 0.0 || intensity <= 0.0 {
        return src.clone();
    }
    let color = law.premultiplied(v.color("color"));
    // Toward the light on screen (y down): 0 is the right, 90 above.
    let (sin, cos) = v.float("angle").to_radians().sin_cos();
    let (dx, dy) = (snap(cos) * width, -snap(sin) * width);
    let (w, h) = (src.w as usize, src.h as usize);
    let mut mask = Image::new(src.w, src.h);
    mask.px
        .par_chunks_mut(w.max(1) * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, o) in row.chunks_exact_mut(4).enumerate() {
                let a = alpha_at(src, y * w + x);
                if a <= 0.0 {
                    continue;
                }
                let toward = crate::export::bilinear(&src.px, w, h, x as f32 + 0.5 + dx, y as f32 + 0.5 + dy);
                o[3] = a * (1.0 - toward[3].clamp(0.0, 1.0));
            }
        });
    let soft = v.float("softness");
    let mask = if soft > 0.0 { fx::blur(&mask, soft, soft) } else { mask };
    let mut out = src.clone();
    out.px
        .par_chunks_mut(4)
        .zip(mask.px.par_chunks(4))
        .for_each(|(p, m)| {
            // A softened rim is cut back to the drawing.
            let k = m[3].min(p[3]) * intensity;
            if k > 0.0 {
                for c in 0..3 {
                    p[c] += color[c] * k;
                }
            }
        });
    out
}

pub(crate) fn vignette(v: &FxValues, law: ColorLaw, src: &Image) -> Image {
    let col = law.encode_rgba(v.color("color"));
    let strength = v.float("amount") * col[3];
    if strength <= 0.0 {
        return src.clone();
    }
    let (start, soft) = (v.float("radius"), v.float("softness").max(1e-6));
    let (w, h) = (src.w as f32, src.h as f32);
    let half_diag = (0.5 * (w * w + h * h).sqrt()).max(1e-6);
    let width = src.w as usize;
    let mut out = src.clone();
    out.px
        .par_chunks_mut(width.max(1) * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, p) in row.chunks_exact_mut(4).enumerate() {
                let d = (x as f32 + 0.5 - w / 2.0).hypot(y as f32 + 0.5 - h / 2.0) / half_diag;
                let t = ((d - start) / soft).clamp(0.0, 1.0);
                let k = strength * t * t * (3.0 - 2.0 * t);
                if k > 0.0 {
                    for c in 0..3 {
                        p[c] += (col[c] * p[3] - p[c]) * k;
                    }
                }
            }
        });
    out
}

/// SplitMix64's finaliser.
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The grain's value for one cell, frame, seed and channel, in [−1, 1):
/// the same on every machine and every run.
fn noise(cell: (u64, u64), frame: u32, seed: u64, channel: u64) -> f32 {
    const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut z = mix64(seed.wrapping_add(GOLDEN));
    for part in [cell.0, cell.1, frame as u64, channel] {
        z = mix64(z ^ part.wrapping_add(GOLDEN));
    }
    (z >> 40) as f32 / (1u64 << 23) as f32 - 1.0
}

pub(crate) fn grain(v: &FxValues, src: &Image, frame: u32) -> Image {
    let amount = v.float("amount");
    if amount <= 0.0 {
        return src.clone();
    }
    let size = v.float("size").max(1.0);
    let seed = v.float("seed").max(0.0) as u64;
    let mono = v.flag("mono");
    let width = src.w as usize;
    let mut out = src.clone();
    out.px
        .par_chunks_mut(width.max(1) * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let cy = (y as f32 / size).floor() as u64;
            for (x, p) in row.chunks_exact_mut(4).enumerate() {
                let a = p[3];
                if a <= 0.0 {
                    continue;
                }
                let cell = ((x as f32 / size).floor() as u64, cy);
                for c in 0..3 {
                    let n = noise(cell, frame, seed, if mono { 0 } else { c as u64 });
                    p[c] = (p[c] + n * amount * a).max(0.0);
                }
            }
        });
    out
}

/// Directional motion blur: the mean of `⌈length⌉ + 1` bilinear taps spaced
/// evenly over `length` px along `angle_deg` (0 across, 90 down), centred
/// on each pixel. A length of 0 (or not a number) is the identity.
pub fn directional_blur(img: &Image, length: f32, angle_deg: f32) -> Image {
    if !(length.is_finite() && length > 0.0) {
        return img.clone();
    }
    let n = length.ceil() as usize + 1;
    let step = length / (n - 1) as f32;
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    let (dx, dy) = (snap(cos), snap(sin));
    let (w, h) = (img.w as usize, img.h as usize);
    let inv = 1.0 / n as f32;
    let mut out = Image::new(img.w, img.h);
    out.px
        .par_chunks_mut(w.max(1) * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, o) in row.chunks_exact_mut(4).enumerate() {
                let mut acc = [0.0f32; 4];
                for k in 0..n {
                    let t = -length / 2.0 + k as f32 * step;
                    let s = crate::export::bilinear(
                        &img.px,
                        w,
                        h,
                        x as f32 + 0.5 + t * dx,
                        y as f32 + 0.5 + t * dy,
                    );
                    for c in 0..4 {
                        acc[c] += s[c];
                    }
                }
                for c in 0..4 {
                    o[c] = acc[c] * inv;
                }
            }
        });
    out
}

// ---- LUTs ----------------------------------------------------------------------------

/// A parsed .cube 3D LUT.
#[derive(Debug, Clone, PartialEq)]
pub struct Lut3D {
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    /// `size³` output colours, red changing fastest (the .cube order).
    pub table: Vec<[f32; 3]>,
}

/// Read a .cube file: `LUT_3D_SIZE` (2–256), optional `DOMAIN_MIN`,
/// `DOMAIN_MAX` or `LUT_3D_INPUT_RANGE`, then `size³` lines of three
/// numbers. `TITLE`, comments (`#`) and other keywords are skipped. A 1D
/// LUT, a bad number, a wrong count or a missing size is an error that
/// says which.
pub fn parse_cube(text: &str) -> Result<Lut3D, String> {
    fn numbers<const N: usize>(line: usize, words: &[&str]) -> Result<[f32; N], String> {
        if words.len() != N {
            return Err(format!("line {line}: {} numbers where {N} belong", words.len()));
        }
        let mut out = [0.0f32; N];
        for (o, w) in out.iter_mut().zip(words) {
            *o = w
                .parse::<f32>()
                .ok()
                .filter(|x| x.is_finite())
                .ok_or_else(|| format!("line {line}: '{w}' is not a number"))?;
        }
        Ok(out)
    }
    let mut size: Option<usize> = None;
    let (mut dmin, mut dmax) = ([0.0f32; 3], [1.0f32; 3]);
    let mut table = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let words: Vec<&str> = raw.split('#').next().unwrap_or("").split_whitespace().collect();
        let Some(&head) = words.first() else {
            continue;
        };
        let rest = &words[1..];
        match head {
            "TITLE" => {}
            "LUT_3D_SIZE" => {
                let s = rest
                    .first()
                    .and_then(|w| w.parse::<usize>().ok())
                    .filter(|s| (2..=256).contains(s))
                    .ok_or_else(|| format!("line {line}: LUT_3D_SIZE takes a whole number from 2 to 256"))?;
                if size.replace(s).is_some() {
                    return Err(format!("line {line}: a second LUT_3D_SIZE"));
                }
            }
            "LUT_1D_SIZE" => return Err(format!("line {line}: a 1D LUT, not a 3D .cube")),
            "DOMAIN_MIN" => dmin = numbers::<3>(line, rest)?,
            "DOMAIN_MAX" => dmax = numbers::<3>(line, rest)?,
            "LUT_3D_INPUT_RANGE" => {
                let [lo, hi] = numbers::<2>(line, rest)?;
                (dmin, dmax) = ([lo; 3], [hi; 3]);
            }
            _ if head.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.')) => {
                table.push(numbers::<3>(line, &words)?);
            }
            _ => {} // another keyword (LUT_IN_VIDEO_RANGE, ...): not ours
        }
    }
    let size = size.ok_or("no LUT_3D_SIZE line")?;
    if table.len() != size.pow(3) {
        return Err(format!(
            "{} colours where LUT_3D_SIZE {size} takes {}",
            table.len(),
            size.pow(3)
        ));
    }
    if (0..3).any(|c| dmax[c] <= dmin[c]) {
        return Err("DOMAIN_MAX must be above DOMAIN_MIN".into());
    }
    Ok(Lut3D {
        size,
        domain_min: dmin,
        domain_max: dmax,
        table,
    })
}

/// One colour through the LUT: trilinear between the 8 lattice points
/// around it; outside the domain clamps to its edge.
pub fn apply_lut(lut: &Lut3D, rgb: [f32; 3]) -> [f32; 3] {
    let n = lut.size;
    let mut i0 = [0usize; 3];
    let mut f = [0.0f32; 3];
    for c in 0..3 {
        let t = ((rgb[c] - lut.domain_min[c]) / (lut.domain_max[c] - lut.domain_min[c])).clamp(0.0, 1.0)
            * (n - 1) as f32;
        let i = (t.floor() as usize).min(n - 2);
        i0[c] = i;
        f[c] = t - i as f32;
    }
    let at = |r: usize, g: usize, b: usize| lut.table[r + g * n + b * n * n];
    let mut out = [0.0f32; 3];
    for (corner, wgt) in (0..8).map(|k| {
        let (dr, dg, db) = (k & 1, (k >> 1) & 1, (k >> 2) & 1);
        let w = [dr, dg, db]
            .iter()
            .zip(f)
            .map(|(&d, fc)| if d == 1 { fc } else { 1.0 - fc })
            .product::<f32>();
        (at(i0[0] + dr, i0[1] + dg, i0[2] + db), w)
    }) {
        for c in 0..3 {
            out[c] += corner[c] * wgt;
        }
    }
    out
}

/// The sRGB EOTF unquantised (`export::srgb_to_linear` before its byte).
fn srgb_eotf(s: f32) -> f32 {
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// A cut's file as worked out once: a file is an immutable shared handle
/// (`Cut::blobs`), so while a memo holds one its address names those
/// bytes. Its hash and its LUT are not redone on every frame's render.
struct FileMemo {
    file: Arc<Vec<u8>>,
    hash: u64,
    lut: Option<Option<Arc<Lut3D>>>,
}

/// How many files the memo keeps (a look rarely reads more than two).
const FILE_MEMOS: usize = 4;

fn with_memo<T>(file: &Arc<Vec<u8>>, f: impl FnOnce(&mut FileMemo) -> T) -> T {
    use std::sync::{Mutex, OnceLock};
    static MEMOS: OnceLock<Mutex<Vec<FileMemo>>> = OnceLock::new();
    let mut memos = MEMOS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let i = match memos.iter().position(|m| Arc::ptr_eq(&m.file, file)) {
        Some(i) => i,
        None => {
            if memos.len() >= FILE_MEMOS {
                memos.remove(0);
            }
            memos.push(FileMemo {
                file: file.clone(),
                hash: crate::value::fnv1a(file),
                lut: None,
            });
            memos.len() - 1
        }
    };
    f(&mut memos[i])
}

/// The content hash of a cut's file (FNV-1a), once per file.
pub fn file_hash(file: &Arc<Vec<u8>>) -> u64 {
    with_memo(file, |m| m.hash)
}

/// A cut's file read as a .cube LUT (None: it does not read), once per
/// file.
pub fn file_lut(file: &Arc<Vec<u8>>) -> Option<Arc<Lut3D>> {
    with_memo(file, |m| {
        let bytes = m.file.clone();
        m.lut
            .get_or_insert_with(|| {
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|t| parse_cube(t).ok())
                    .map(Arc::new)
            })
            .clone()
    })
}

pub(crate) fn lut_fx(v: &FxValues, law: ColorLaw, src: &Image, file: Option<&Arc<Vec<u8>>>) -> Image {
    let mix = v.float("mix");
    if mix <= 0.0 {
        return src.clone();
    }
    let Some(lut) = file.and_then(file_lut) else {
        return src.clone(); // no file, or not one that reads: as it is
    };
    // A .cube grades display-encoded colour: the Srgb law stores it; the
    // Legacy law stores light, so it goes out and back through the curve.
    let (shown, stored): (fn(f32) -> f32, fn(f32) -> f32) = match law {
        ColorLaw::Srgb => (|x| x, |x| x),
        ColorLaw::Legacy => (crate::export::linear_to_srgb_f, srgb_eotf),
    };
    let mut out = src.clone();
    out.px.par_chunks_mut(4).for_each(|p| {
        let a = p[3];
        if a <= 0.0 {
            return;
        }
        let c = [p[0] / a, p[1] / a, p[2] / a];
        let graded = apply_lut(&lut, c.map(shown)).map(stored);
        for i in 0..3 {
            p[i] = (c[i] + (graded[i] - c[i]) * mix) * a;
        }
    });
    out
}

// ---- Mattes and palette maps -------------------------------------------------------

/// The min (choke) or max (spread) of `m` over a `(2r+1)²` square, the
/// frame's edge clamping the square.
fn extreme_filter(m: &[f32], w: usize, h: usize, r: usize, take_min: bool) -> Vec<f32> {
    let pick = |a: f32, b: f32| if take_min { a.min(b) } else { a.max(b) };
    let init = if take_min { f32::INFINITY } else { f32::NEG_INFINITY };
    let mut rows = vec![0.0f32; m.len()];
    for y in 0..h {
        let row = &m[y * w..(y + 1) * w];
        for x in 0..w {
            let (lo, hi) = (x.saturating_sub(r), (x + r).min(w - 1));
            rows[y * w + x] = row[lo..=hi].iter().copied().fold(init, pick);
        }
    }
    let mut out = vec![0.0f32; m.len()];
    for y in 0..h {
        let (lo, hi) = (y.saturating_sub(r), (y + r).min(h - 1));
        for x in 0..w {
            out[y * w + x] = (lo..=hi).map(|yy| rows[yy * w + x]).fold(init, pick);
        }
    }
    out
}

pub(crate) fn matte(v: &FxValues, src: &Image, matte: Option<&Image>) -> Image {
    let luma = v.choice("channel") == 1;
    let invert = v.flag("invert");
    let choke = v.float("choke");
    let white = v.flag("white");
    let r = choke.abs().round() as usize;
    if matte.is_none() && !luma && !invert && r == 0 && !white {
        return src.clone(); // the image's own alpha, as it is
    }
    let from = matte.unwrap_or(src);
    let (w, h) = (src.w as usize, src.h as usize);
    let mut m: Vec<f32> = (0..w * h)
        .map(|i| {
            let x = if luma {
                let p = from.px.get(i * 4..i * 4 + 3).unwrap_or(&[0.0; 3]);
                (LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2]).clamp(0.0, 1.0)
            } else {
                alpha_at(from, i)
            };
            if invert { 1.0 - x } else { x }
        })
        .collect();
    if r > 0 && w > 0 && h > 0 {
        m = extreme_filter(&m, w, h, r, choke > 0.0);
    }
    let own = matte.is_none();
    let mut out = src.clone();
    out.px
        .par_chunks_mut(4)
        .zip(m.par_iter())
        .for_each(|(p, &k)| {
            if white {
                p.copy_from_slice(&[k; 4]);
            } else if own {
                // Re-covered by its own matte: its colour, k's coverage.
                let s = if p[3] > 0.0 { k / p[3] } else { 0.0 };
                for c in p.iter_mut() {
                    *c *= s;
                }
            } else {
                for c in p.iter_mut() {
                    *c *= k;
                }
            }
        });
    out
}

/// Palette map: each pixel bit-equal to a `from` chip's hard-fill texel
/// (through `law`) becomes its `to` chip's texel; every other pixel, and
/// every pixel when `pairs` is empty, is untouched. The first pair of a
/// repeated chip wins.
pub fn palette_map(img: &Image, law: ColorLaw, pairs: &[([u8; 4], [u8; 4])]) -> Image {
    let texel = |c: [u8; 4]| law.texel(c).map(f16_bits_to_f32);
    let table: Vec<([f32; 4], [f32; 4])> = pairs
        .iter()
        .filter(|(a, b)| a != b)
        .map(|&(a, b)| (texel(a), texel(b)))
        .collect();
    if table.is_empty() {
        return img.clone();
    }
    let mut out = img.clone();
    out.px.par_chunks_mut(4).for_each(|p| {
        if let Some((_, to)) = table.iter().find(|(from, _)| from[..] == p[..]) {
            p.copy_from_slice(to);
        }
    });
    out
}

/// The eval recipe's extra for a look kind: Grain's frame, and each file
/// parameter's content hash ("-" when the file is missing). Empty for the
/// other kinds, so their recipes are as they were.
pub fn fx_recipe_extra(fx: FxKind, v: &FxValues, cut: &Cut, frame: u32) -> String {
    let mut extra = String::new();
    if fx.uses_frame() {
        extra.push_str(&format!(",frame={frame}"));
    }
    for s in fx.specs().iter().filter(|s| matches!(s.kind, ParamKind::Blob)) {
        match cut.blobs.get(v.blob(s.key)) {
            Some(b) => extra.push_str(&format!(",{}#{:016x}", s.key, file_hash(b))),
            None => extra.push_str(&format!(",{}#-", s.key)),
        }
    }
    extra
}

// ---- The satsuei template --------------------------------------------------------------

/// An Effect node of `fx` at its defaults but for `values`.
pub fn effect_with(fx: FxKind, values: &[(&str, FxValue)]) -> NodeKind {
    let mut kind = NodeKind::effect(fx);
    if let NodeKind::Effect { params, .. } = &mut kind {
        for (key, v) in values {
            params.set(key, v.clone());
        }
    }
    kind
}

/// The template's batch as it is built: commands, ids and places (GM18's
/// page graph builds with it too).
pub(crate) struct Build<'a, F: FnMut() -> NodeId> {
    pub(crate) cut: &'a Cut,
    pub(crate) at: CutRef,
    pub(crate) alloc: F,
    pub(crate) cmds: Vec<Command>,
}

impl<F: FnMut() -> NodeId> Build<'_, F> {
    /// A node at a place in the graph pane.
    pub(crate) fn add(&mut self, kind: NodeKind, pos: (f32, f32)) -> NodeId {
        let id = (self.alloc)();
        self.cmds.push(Command::AddNode { at: self.at, id, kind });
        self.cmds.extend(place_node(self.cut, self.at, id, Some(pos)));
        id
    }

    pub(crate) fn wire(&mut self, from: NodeId, to: NodeId, to_pin: u16) {
        self.cmds.push(Command::Connect {
            at: self.at,
            from,
            from_pin: 0,
            to,
            to_pin,
        });
    }

    /// `over` blended onto `under` (None: `over` alone).
    pub(crate) fn stack(&mut self, under: Option<NodeId>, over: NodeId, mode: BlendMode, pos: (f32, f32)) -> NodeId {
        let Some(under) = under else {
            return over;
        };
        let blend = self.add(NodeKind::Blend { mode }, pos);
        self.wire(under, blend, 0);
        self.wire(over, blend, 1);
        blend
    }

    /// A node fed by `input` on its first pin.
    pub(crate) fn then(&mut self, input: NodeId, kind: NodeKind, pos: (f32, f32)) -> NodeId {
        let id = self.add(kind, pos);
        self.wire(input, id, 0);
        id
    }
}

/// The columns the satsuei template builds from: every column but a Ref,
/// in band order (BG, Cel, Book; sheet order within a band). A hidden one
/// is in too — its source draws nothing until the column is shown again.
pub fn template_columns(cut: &Cut) -> Vec<ColumnId> {
    let mut order: Vec<(u8, usize, ColumnId)> = cut
        .xsheet
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| (cut.column_def(c.id).kind, i, c.id))
        .filter(|(kind, _, _)| *kind != ColumnKind::Ref)
        .map(|(kind, i, id)| (kind.band(), i, id))
        .collect();
    order.sort();
    order.into_iter().map(|(_, _, id)| id).collect()
}

/// Does node `id`, or anything feeding it, draw `column`?
fn draws_column(cut: &Cut, id: NodeId, column: ColumnId) -> bool {
    let mut todo = vec![id];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = todo.pop() {
        let Ok(n) = cut.graph.node(id) else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        if matches!(n.kind, NodeKind::DrawingSource { column: c, .. } if c == column) {
            return true;
        }
        todo.extend(n.inputs.iter().flatten().map(|&(src, _)| src));
    }
    false
}

/// The template's shadow tone: a cool multiply over the painted kage (the
/// paint pass already holds the shadow colour, so this only cools and
/// deepens it a little — red 0.81, green 0.9, blue 0.99 of the paint).
pub fn shadow_tone() -> NodeKind {
    effect_with(
        FxKind::Grade,
        &[("gain", FxValue::Float(0.9)), ("temperature", FxValue::Float(-0.4))],
    )
}

/// GM13 "Build satsuei graph": the cut's graph becomes the satsuei chain,
/// one batch (the old nodes and their places go first, so the batch is the
/// whole graph and one undo step); every node stays an ordinary, editable
/// node. `alloc` hands out node ids, as for the auto-comp. It builds from
/// [`template_columns`] (a hidden column too).
///
/// - The BG columns (bottom → top, Normal) through a Camera. The graph's
///   first Camera keeps its kind — the layout room's keys, binds and planes
///   — and each BG column goes on the plane it fed there (else the first).
///   The characters and Book stay held to the frame, off the camera.
/// - Each cel column: its paint pass ([`PAINT_PASS`], trace lines absorbed),
///   its Shadow pass → Matte (white) → Grade ([`shadow_tone`]) multiplied
///   over the paint, its Highlight pass → Glow screened on, and its Line
///   pass laid over (Normal); the characters stack in sheet order.
/// - Book columns over the characters (Normal).
/// - The characters over the BG camera, then Diffusion, Para, Light wrap
///   (background: the BG camera; matte: the characters), Grade, Grain and
///   Vignette into the Output.
pub fn satsuei_template(cut: &Cut, at: CutRef, alloc: impl FnMut() -> NodeId) -> Vec<Command> {
    let mut b = Build {
        cut,
        at,
        alloc,
        cmds: Vec::new(),
    };
    // The camera work carries over: the first Camera's kind (never fewer
    // than one plane), and the plane each BG column fed.
    let old_cam = cut.graph.nodes().find(|n| matches!(n.kind, NodeKind::Camera { .. }));
    let mut cam_kind = old_cam.map_or_else(NodeKind::camera, |n| n.kind.clone());
    if let NodeKind::Camera { planes, .. } = &mut cam_kind
        && planes.is_empty()
    {
        planes.push(PlaneSpec::default());
    }
    let plane_count = cam_kind.input_count().max(1);
    let plane_of = |column: ColumnId| -> usize {
        old_cam
            .and_then(|cam| {
                cam.inputs
                    .iter()
                    .position(|i| i.is_some_and(|(src, _)| draws_column(cut, src, column)))
            })
            .filter(|&pin| pin < plane_count)
            .unwrap_or(0)
    };
    for n in cut.graph.nodes() {
        b.cmds.push(Command::RemoveNode { at, id: n.id });
    }
    for n in cut.graph.nodes() {
        b.cmds.extend(place_node(cut, at, n.id, None));
    }
    const COL: f32 = 220.0;
    const ROW: f32 = 90.0;
    let mut row = 0.0f32;
    let mut next_row = || {
        row += 1.0;
        (row - 1.0) * ROW
    };
    // The BG's stack on each camera plane, then the characters and Book.
    let mut bg: Vec<Option<NodeId>> = vec![None; plane_count];
    let (mut chars, mut book): (Option<NodeId>, Option<NodeId>) = (None, None);
    for column in template_columns(cut) {
        match cut.column_def(column).kind {
            ColumnKind::Bg => {
                let y = next_row();
                let src = b.add(NodeKind::source(column), (0.0, y));
                let plane = plane_of(column);
                bg[plane] = Some(b.stack(bg[plane], src, BlendMode::Normal, (COL, y)));
            }
            ColumnKind::Book => {
                let y = next_row();
                let src = b.add(NodeKind::source(column), (0.0, y));
                book = Some(b.stack(book, src, BlendMode::Normal, (4.0 * COL, y)));
            }
            // A Ref column never renders (and is not in the columns).
            ColumnKind::Ref => {}
            ColumnKind::Cel => {
                let (y0, y1, y2, y3) = (next_row(), next_row(), next_row(), next_row());
                let paint = b.add(NodeKind::pass(column, PAINT_PASS, true), (0.0, y0));
                let shadow = b.add(NodeKind::pass(column, &[LayerRole::Shadow], false), (0.0, y1));
                let matte = b.then(shadow, effect_with(FxKind::Matte, &[("white", FxValue::Bool(true))]), (COL, y1));
                let tone = b.then(matte, shadow_tone(), (2.0 * COL, y1));
                let shaded = b.stack(Some(paint), tone, BlendMode::Multiply, (3.0 * COL, y1));
                let high = b.add(NodeKind::pass(column, &[LayerRole::Highlight], false), (0.0, y2));
                let glow = b.then(
                    high,
                    effect_with(
                        FxKind::Glow,
                        &[
                            ("threshold", FxValue::Float(0.0)),
                            ("radius", FxValue::Float(8.0)),
                            ("intensity", FxValue::Float(0.6)),
                        ],
                    ),
                    (COL, y2),
                );
                let lit = b.stack(Some(shaded), glow, BlendMode::Screen, (3.0 * COL, y2));
                let lines = b.add(NodeKind::pass(column, &[LayerRole::Line], false), (0.0, y3));
                let character = b.stack(Some(lit), lines, BlendMode::Normal, (3.0 * COL, y3));
                chars = Some(b.stack(chars, character, BlendMode::Normal, (4.0 * COL, y3)));
            }
        }
    }
    let y = (row - 1.0).max(0.0) * ROW;
    // The camera films the BG; a kept camera stays even with no BG to
    // film, so its keys are never lost.
    let cam = (old_cam.is_some() || bg.iter().any(Option::is_some)).then(|| {
        let cam = b.add(cam_kind, (2.0 * COL, 0.0));
        for (pin, stack) in bg.iter().enumerate() {
            if let Some(stack) = *stack {
                b.wire(stack, cam, pin as u16);
            }
        }
        cam
    });
    let fg = match (chars, book) {
        (Some(c), Some(k)) => Some(b.stack(Some(c), k, BlendMode::Normal, (5.0 * COL, y))),
        (c, k) => c.or(k),
    };
    let full = match (cam, fg) {
        (Some(c), Some(f)) => Some(b.stack(Some(c), f, BlendMode::Normal, (6.0 * COL, y))),
        (c, f) => c.or(f),
    };
    let out = if let Some(full) = full {
        let mut x = 7.0 * COL;
        let mut step = |b: &mut Build<'_, _>, input: NodeId, kind: NodeKind| {
            x += COL;
            b.then(input, kind, (x - COL, y))
        };
        let diffusion = step(&mut b, full, effect_with(FxKind::Diffusion, &[("mix", FxValue::Float(0.2))]));
        let para = step(&mut b, diffusion, effect_with(FxKind::Para, &[("amount", FxValue::Float(0.25))]));
        let wrap = step(&mut b, para, NodeKind::effect(FxKind::LightWrap));
        if let Some(c) = cam {
            b.wire(c, wrap, 1);
        }
        if let Some(f) = fg {
            b.wire(f, wrap, 2);
        }
        let grade = step(&mut b, wrap, NodeKind::effect(FxKind::Grade));
        let grain = step(&mut b, grade, NodeKind::effect(FxKind::Grain));
        let vignette = step(&mut b, grain, NodeKind::effect(FxKind::Vignette));
        step(&mut b, vignette, NodeKind::Output)
    } else {
        b.add(NodeKind::Output, (7.0 * COL, y))
    };
    b.cmds.push(Command::SetOutput { at, node: Some(out) });
    b.cmds
}
