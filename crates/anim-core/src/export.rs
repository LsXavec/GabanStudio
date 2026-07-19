//! Headless frame rendering: composite a cut at one frame into a flat RGBA8
//! image. Pure CPU — the same premultiplied-over math as the GPU compositors
//! (tile values are f16 bit patterns; see the encoding law on
//! [`crate::raster::TileData`]), assembled full-frame.
//!
//! Two paths:
//! - **Column composite** ([`render_frame`]): X-sheet order (first column =
//!   bottom), each drawing's visible cel layers flattened bottom→top. The
//!   legacy/fallback path.
//! - **Graph composite** ([`render_graph_frame`]): executes `cut.graph` —
//!   DrawingSource/Solid/Transform/Blend/Output with real pixel semantics.
//!   This is the GOLDEN REFERENCE for the app's GPU graph compositor (the
//!   same role `Drawing::flatten` plays for the layer sandwich) and the
//!   export truth once a graph output is wired. Returns `None` when the
//!   graph has no wired output — callers fall back to the column path.
//!
//! Vector strokes are NOT rasterized here (raster layers only) — callers
//! should surface that honestly if a cut still carries legacy vector artwork.

use std::collections::HashSet;

use crate::graph::{BlendMode, NodeKind};
use crate::ids::NodeId;
use crate::model::Cut;
use crate::raster::{TILE, f16_bits_to_f32};

/// Splat one drawing's flattened tiles over a premultiplied f32 accumulator.
fn splat_flat(
    acc: &mut [f32],
    flat: &std::collections::BTreeMap<crate::raster::TileCoord, std::sync::Arc<crate::raster::TileData>>,
    width: u32,
    height: u32,
) {
    let w = width as usize;
    let h = height as usize;
    for ((tx, ty), tile) in flat {
        let px0 = *tx as i64 * TILE as i64;
        let py0 = *ty as i64 * TILE as i64;
        for row in 0..TILE {
            let py = py0 + row as i64;
            if py < 0 || py >= h as i64 {
                continue;
            }
            for cx in 0..TILE {
                let px = px0 + cx as i64;
                if px < 0 || px >= w as i64 {
                    continue;
                }
                let s = &tile.rgba[(row * TILE + cx) * 4..][..4];
                let sa = f16_bits_to_f32(s[3]);
                if sa <= 0.0 {
                    continue; // premultiplied: fully transparent texel
                }
                let dst = &mut acc[((py as usize) * w + px as usize) * 4..][..4];
                let keep = 1.0 - sa;
                for c in 0..4 {
                    dst[c] = f16_bits_to_f32(s[c]) + dst[c] * keep;
                }
            }
        }
    }
}

/// Premultiplied f32 full-frame composite of every column at `frame`.
fn composite(cut: &Cut, frame: u32, width: u32, height: u32) -> Vec<f32> {
    let mut acc = vec![0.0f32; width as usize * height as usize * 4];
    for col in &cut.xsheet.columns {
        let Some(id) = col.resolve(frame) else {
            continue;
        };
        let Some(d) = cut.drawing(id) else {
            continue;
        };
        splat_flat(&mut acc, &d.flatten(), width, height);
    }
    acc
}

/// Premultiplied f32 → straight RGBA8 over a transparent background.
fn to_straight_rgba8(acc: &[f32]) -> Vec<u8> {
    let mut out = vec![0u8; acc.len()];
    for (o, px) in out.chunks_exact_mut(4).zip(acc.chunks_exact(4)) {
        let a = px[3].clamp(0.0, 1.0);
        if a > 0.0 {
            for c in 0..3 {
                o[c] = ((px[c] / a).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
        o[3] = (a * 255.0).round() as u8;
    }
    out
}

/// Premultiplied f32 → opaque RGBA8 composited over a background colour.
fn to_rgba8_over(acc: &[f32], bg: [u8; 3]) -> Vec<u8> {
    let bgf = [
        bg[0] as f32 / 255.0,
        bg[1] as f32 / 255.0,
        bg[2] as f32 / 255.0,
    ];
    let mut out = vec![0u8; acc.len()];
    for (o, px) in out.chunks_exact_mut(4).zip(acc.chunks_exact(4)) {
        let a = px[3].clamp(0.0, 1.0);
        for c in 0..3 {
            let v = px[c] + bgf[c] * (1.0 - a); // src-over on opaque bg
            o[c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        o[3] = 255;
    }
    out
}

/// Render one frame to straight (un-premultiplied) RGBA8 over a TRANSPARENT
/// background — for PNG sequences that keep alpha. Column-stack path.
pub fn render_frame(cut: &Cut, frame: u32, width: u32, height: u32) -> Vec<u8> {
    to_straight_rgba8(&composite(cut, frame, width, height))
}

/// Render one frame composited over an opaque background colour — for video
/// formats without alpha. Column-stack path.
pub fn render_frame_over(
    cut: &Cut,
    frame: u32,
    width: u32,
    height: u32,
    bg: [u8; 3],
) -> Vec<u8> {
    to_rgba8_over(&composite(cut, frame, width, height), bg)
}

/// Count drawings that still carry legacy vector strokes (not exported).
pub fn vector_stroke_cels(cut: &Cut) -> usize {
    cut.drawings.iter().filter(|d| !d.strokes.is_empty()).count()
}

// ---- Graph composite --------------------------------------------------------

/// sRGB→linear EOTF for node colours. MUST match the app's brush-dab
/// conversion (`linear_rgba` in canvas.rs) so a Solid node renders the same
/// pixels as painting that colour; alpha stays straight.
pub fn srgb_to_linear(v: u8) -> f32 {
    let s = v as f32 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Blend `top` over `bottom` per `mode`, premultiplied, in place on `bottom`.
/// Formulas pinned by graph_render_tests — the GPU blend shader mirrors them
/// exactly. Alpha is Porter-Duff over for every mode; Add's RGB may exceed 1
/// transiently (clamped at the RGBA8 finishers; the GPU's f16 targets hold it).
fn blend_into(bottom: &mut [f32], top: &[f32], mode: BlendMode) {
    for (d, s) in bottom.chunks_exact_mut(4).zip(top.chunks_exact(4)) {
        let (sa, da) = (s[3], d[3]);
        let out_a = sa + da * (1.0 - sa);
        for c in 0..3 {
            d[c] = match mode {
                BlendMode::Normal => s[c] + d[c] * (1.0 - sa),
                BlendMode::Multiply => s[c] * d[c] + s[c] * (1.0 - da) + d[c] * (1.0 - sa),
                BlendMode::Add => s[c] + d[c],
                BlendMode::Screen => s[c] + d[c] - s[c] * d[c],
            };
        }
        d[3] = out_a;
    }
}

/// Forward transform law: `p_out = C + R_θ · s · (p_in − C) + t`, with C the
/// canvas centre, t in pixels, positive θ clockwise on screen (y-down).
/// Rendered by INVERSE mapping with bilinear sampling; outside the source
/// rect is transparent. |s| < 1e-6 renders fully transparent (a degenerate
/// scale has no meaningful inverse).
fn transform_image(
    src: &[f32],
    width: u32,
    height: u32,
    translate: (f32, f32),
    scale: f32,
    rotate_deg: f32,
) -> Vec<f32> {
    let w = width as usize;
    let h = height as usize;
    let mut out = vec![0.0f32; w * h * 4];
    if scale.abs() < 1e-6 {
        return out;
    }
    let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
    let rad = rotate_deg.to_radians();
    // Inverse rotation (transpose of forward R in y-down coords).
    let (sin, cos) = rad.sin_cos();
    let inv_s = 1.0 / scale;
    for py in 0..h {
        for px in 0..w {
            // Pixel centre → inverse map → source sample position.
            let ox = px as f32 + 0.5 - cx - translate.0;
            let oy = py as f32 + 0.5 - cy - translate.1;
            let sx = (cos * ox + sin * oy) * inv_s + cx;
            let sy = (-sin * ox + cos * oy) * inv_s + cy;
            // Bilinear in texel-index space; out-of-bounds taps contribute
            // transparency (the outer half-texel of a fractionally-placed
            // image fades out). KNOWN DEVIATION: the GPU compositor's
            // ClampToEdge sampler instead repeats the border texel inside
            // that half-texel band, so display and export can differ by a
            // sub-pixel edge fringe on FRACTIONAL transforms (integer
            // transforms are exact on both). This CPU law is the export
            // truth; revisit with a bounds-checked shader if it ever shows.
            let ix = sx - 0.5;
            let iy = sy - 0.5;
            let x0 = ix.floor();
            let y0 = iy.floor();
            let (fx, fy) = (ix - x0, iy - y0);
            let mut px_acc = [0.0f32; 4];
            for (dy, wy) in [(0i64, 1.0 - fy), (1, fy)] {
                let ty = y0 as i64 + dy;
                if wy <= 0.0 || ty < 0 || ty >= h as i64 {
                    continue;
                }
                for (dx, wx) in [(0i64, 1.0 - fx), (1, fx)] {
                    let tx = x0 as i64 + dx;
                    if wx <= 0.0 || tx < 0 || tx >= w as i64 {
                        continue;
                    }
                    let sp = &src[((ty as usize) * w + tx as usize) * 4..][..4];
                    let wgt = wx * wy;
                    for c in 0..4 {
                        px_acc[c] += sp[c] * wgt;
                    }
                }
            }
            out[(py * w + px) * 4..][..4].copy_from_slice(&px_acc);
        }
    }
    out
}

/// Recursively render one node to a premultiplied f32 frame.
/// The graph structure guarantees acyclicity (connect() rejects cycles), but
/// the visiting set keeps a corrupted file from recursing forever.
fn render_node(
    cut: &Cut,
    id: NodeId,
    frame: u32,
    width: u32,
    height: u32,
    visiting: &mut HashSet<NodeId>,
) -> Vec<f32> {
    let empty = || vec![0.0f32; width as usize * height as usize * 4];
    if !visiting.insert(id) {
        return empty();
    }
    let Ok(node) = cut.graph.node(id) else {
        visiting.remove(&id);
        return empty();
    };
    let input = |slot: usize, visiting: &mut HashSet<NodeId>| -> Vec<f32> {
        match node.inputs.get(slot).copied().flatten() {
            Some((src, _pin)) => render_node(cut, src, frame, width, height, visiting),
            None => empty(),
        }
    };
    let out = match &node.kind {
        NodeKind::DrawingSource { column } => {
            let mut acc = empty();
            if let Some(col) = cut.xsheet.column(*column)
                && let Some(did) = col.resolve(frame)
                && let Some(d) = cut.drawing(did)
            {
                splat_flat(&mut acc, &d.flatten(), width, height);
            }
            acc
        }
        NodeKind::Solid { rgba } => {
            let a = rgba[3] as f32 / 255.0;
            let px = [
                srgb_to_linear(rgba[0]) * a,
                srgb_to_linear(rgba[1]) * a,
                srgb_to_linear(rgba[2]) * a,
                a,
            ];
            let mut acc = empty();
            for chunk in acc.chunks_exact_mut(4) {
                chunk.copy_from_slice(&px);
            }
            acc
        }
        NodeKind::Transform {
            translate,
            scale,
            rotate_deg,
        } => {
            let child = input(0, visiting);
            transform_image(&child, width, height, *translate, *scale, *rotate_deg)
        }
        NodeKind::Blend { mode } => {
            let mut bottom = input(0, visiting);
            let top = input(1, visiting);
            blend_into(&mut bottom, &top, *mode);
            bottom
        }
        NodeKind::Output => input(0, visiting),
    };
    visiting.remove(&id);
    out
}

/// Execute `cut.graph` at `frame` into a premultiplied f32 frame.
/// `None` when no output node is wired — the caller falls back to the
/// column-stack composite.
fn graph_composite(cut: &Cut, frame: u32, width: u32, height: u32) -> Option<Vec<f32>> {
    let output = cut.graph.output?;
    let mut visiting = HashSet::new();
    Some(render_node(cut, output, frame, width, height, &mut visiting))
}

/// Graph-path render to straight RGBA8 over transparent (PNG-with-alpha).
/// `None` = no wired graph output; use [`render_frame`] instead.
pub fn render_graph_frame(cut: &Cut, frame: u32, width: u32, height: u32) -> Option<Vec<u8>> {
    graph_composite(cut, frame, width, height).map(|acc| to_straight_rgba8(&acc))
}

/// Graph-path render over an opaque background (video).
/// `None` = no wired graph output; use [`render_frame_over`] instead.
pub fn render_graph_frame_over(
    cut: &Cut,
    frame: u32,
    width: u32,
    height: u32,
    bg: [u8; 3],
) -> Option<Vec<u8>> {
    graph_composite(cut, frame, width, height).map(|acc| to_rgba8_over(&acc, bg))
}
