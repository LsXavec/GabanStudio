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

use rayon::prelude::*;

use crate::camera::{self, CameraState, IRect};
use crate::color::ColorLaw;
use crate::deliver::Purpose;
use crate::fx::Image;
use crate::graph::{BlendMode, Node, NodeKind, PlaneSpec};
use crate::ids::NodeId;
use crate::model::Cut;
use crate::raster::{TILE, f16_bits_to_f32};

/// GM10: where a node renders, in frame coordinates. Everything renders
/// into the frame itself, except a Camera's planes, which render into the
/// cut's paper rect; `fw × fh` is the frame either way (a Transform turns
/// about the frame's centre wherever the area sits). At the frame's own
/// rect every arm is exactly the pre-camera renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Area {
    rect: IRect,
    fw: u32,
    fh: u32,
}

impl Area {
    fn frame(width: u32, height: u32) -> Self {
        Self {
            rect: IRect::frame(width, height),
            fw: width,
            fh: height,
        }
    }

    fn len(&self) -> usize {
        self.rect.w as usize * self.rect.h as usize * 4
    }

    fn is_frame(&self) -> bool {
        self.rect == IRect::frame(self.fw, self.fh)
    }
}

/// Splat one drawing's flattened tiles over a premultiplied f32 accumulator
/// covering `rect` (frame coordinates; GM10: a paper may start left of or
/// above the frame, where tiles have negative coordinates).
fn splat_flat(
    acc: &mut [f32],
    flat: &std::collections::BTreeMap<crate::raster::TileCoord, std::sync::Arc<crate::raster::TileData>>,
    rect: IRect,
) {
    let w = rect.w as usize;
    let h = rect.h as usize;
    for ((tx, ty), tile) in flat {
        let px0 = *tx as i64 * TILE as i64 - rect.x as i64;
        let py0 = *ty as i64 * TILE as i64 - rect.y as i64;
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
                let dst = &mut acc[((py as usize) * w + px as usize) * 4..][..4];
                over_texel(dst, s);
            }
        }
    }
}

/// Premultiplied src-over of one f16 texel onto one f32 pixel.
fn over_texel(dst: &mut [f32], s: &[u16]) {
    let sa = f16_bits_to_f32(s[3]);
    if sa <= 0.0 {
        return; // premultiplied: fully transparent texel
    }
    let keep = 1.0 - sa;
    for c in 0..4 {
        dst[c] = f16_bits_to_f32(s[c]) + dst[c] * keep;
    }
}

/// Premultiplied f32 full-frame composite of every rendering column at
/// `frame`, in [`Cut::render_order`] (BG, Cel, Book bands; hidden and Ref
/// columns skipped — an all-Cel cut keeps plain sheet order).
fn composite(cut: &Cut, frame: u32, width: u32, height: u32) -> Vec<f32> {
    composite_for(cut, frame, width, height, Purpose::View)
}

/// GM11: the column composite as a render purpose sees each cel
/// ([`crate::model::Drawing::flatten_for`]); View is exactly [`composite`].
pub fn composite_for(cut: &Cut, frame: u32, width: u32, height: u32, purpose: Purpose) -> Vec<f32> {
    let mut acc = vec![0.0f32; width as usize * height as usize * 4];
    for col in cut.render_order().into_iter().filter_map(|id| cut.xsheet.column(id)) {
        let Some(id) = col.resolve(frame) else {
            continue;
        };
        let Some(d) = cut.drawing(id) else {
            continue;
        };
        splat_flat(
            &mut acc,
            &d.flatten_for(purpose),
            IRect::frame(width, height),
        );
    }
    acc
}

/// ONE pixel of the column composite at `frame` (premultiplied f32): the
/// same flatten and src-over as [`render_frame`], touching only the tile
/// under (x, y) in each drawing — the eyedropper's read, without the frame.
pub fn composite_pixel(cut: &Cut, frame: u32, x: u32, y: u32) -> [f32; 4] {
    let t = TILE as u32;
    let coord = ((x / t) as i32, (y / t) as i32);
    let i = (((y % t) * t + x % t) * 4) as usize;
    let mut px = [0.0f32; 4];
    for col in cut.render_order().into_iter().filter_map(|id| cut.xsheet.column(id)) {
        let Some(id) = col.resolve(frame) else {
            continue;
        };
        let Some(d) = cut.drawing(id) else {
            continue;
        };
        if let Some(tile) = d.flatten_tile(coord) {
            over_texel(&mut px, &tile.rgba[i..i + 4]);
        }
    }
    px
}

/// ONE pixel of the graph composite at `frame` (premultiplied f32). The
/// graph's transforms resample whole images, so this renders the frame;
/// `None` = no wired output.
pub fn graph_composite_pixel(
    cut: &Cut,
    frame: u32,
    width: u32,
    height: u32,
    x: u32,
    y: u32,
) -> Option<[f32; 4]> {
    graph_composite_pixel_law(cut, ColorLaw::Legacy, frame, width, height, x, y)
}

/// [`graph_composite_pixel`] under the project's colour law (D8: a Solid
/// node's chip goes through [`ColorLaw::encode`]).
pub fn graph_composite_pixel_law(
    cut: &Cut,
    law: ColorLaw,
    frame: u32,
    width: u32,
    height: u32,
    x: u32,
    y: u32,
) -> Option<[f32; 4]> {
    let acc = graph_composite(cut, law, frame, width, height)?;
    let i = ((y * width + x) * 4) as usize;
    acc.get(i..i + 4)?.try_into().ok()
}

/// One premultiplied f32 pixel → straight RGBA8 (transparent stays 0).
pub fn straight_rgba8(px: &[f32]) -> [u8; 4] {
    let mut o = [0u8; 4];
    let a = px[3].clamp(0.0, 1.0);
    if a > 0.0 {
        for c in 0..3 {
            o[c] = ((px[c] / a).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    o[3] = (a * 255.0).round() as u8;
    o
}

/// Premultiplied f32 → straight RGBA8 over a transparent background.
fn to_straight_rgba8(acc: &[f32]) -> Vec<u8> {
    let mut out = vec![0u8; acc.len()];
    for (o, px) in out.chunks_exact_mut(4).zip(acc.chunks_exact(4)) {
        o.copy_from_slice(&straight_rgba8(px));
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

/// sRGB→linear EOTF: the Legacy law's chip encoding
/// ([`ColorLaw::encode`], which the brush, the fills and a Solid node all
/// go through, so a Solid renders the same pixels as painting its colour).
pub fn srgb_to_linear(v: u8) -> f32 {
    let s = v as f32 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear→sRGB OETF, the exact inverse of [`srgb_to_linear`]: a stored
/// linear channel back to the chip byte that paints it (the eyedropper's
/// way home). Every byte round-trips.
pub fn linear_to_srgb(v: f32) -> u8 {
    (linear_to_srgb_f(v).clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The same OETF unquantised: what [`linear_to_srgb`] rounds to a byte,
/// and the per-texel remap of the Legacy→sRGB conversion
/// ([`crate::color::convert_legacy_to_srgb`]).
pub fn linear_to_srgb_f(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Blend `top` over `bottom` per `mode`, premultiplied, in place on `bottom`.
/// Formulas pinned by graph_render_tests — the GPU blend shader mirrors them
/// exactly. Alpha is Porter-Duff over for every mode; Add's RGB may exceed 1
/// transiently (clamped at the RGBA8 finishers; the GPU's f16 targets hold it).
// Premultiplied separable blends. `s` = top layer, `d` = bottom, both
// premultiplied linear. Union alpha throughout. Darken/Lighten use the
// alpha-correct separable form min/max(s·da, d·sa) + s(1−da) + d(1−sa);
// Overlay evaluates its curve on UNpremultiplied colour (guarded ÷alpha)
// then recomposites — the standard Porter-Duff separable-blend shape.
// The GPU shader (graphcomp BLEND_SHADER) mirrors these formulas EXACTLY;
// change both together or the composite view lies about the export.
// GM9: the diffusion FX screens / soft-lights through these same formulas.
pub(crate) fn blend_into(bottom: &mut [f32], top: &[f32], mode: BlendMode) {
    for (d, s) in bottom.chunks_exact_mut(4).zip(top.chunks_exact(4)) {
        let (sa, da) = (s[3], d[3]);
        let out_a = sa + da * (1.0 - sa);
        for c in 0..3 {
            d[c] = match mode {
                BlendMode::Normal => s[c] + d[c] * (1.0 - sa),
                BlendMode::Multiply => s[c] * d[c] + s[c] * (1.0 - da) + d[c] * (1.0 - sa),
                BlendMode::Add => s[c] + d[c],
                BlendMode::Screen => s[c] + d[c] - s[c] * d[c],
                BlendMode::Subtract => (d[c] - s[c]).max(0.0),
                BlendMode::Darken => {
                    (s[c] * da).min(d[c] * sa) + s[c] * (1.0 - da) + d[c] * (1.0 - sa)
                }
                BlendMode::Lighten => {
                    (s[c] * da).max(d[c] * sa) + s[c] * (1.0 - da) + d[c] * (1.0 - sa)
                }
                BlendMode::Overlay => {
                    let cs = if sa > 0.0 { s[c] / sa } else { 0.0 };
                    let cb = if da > 0.0 { d[c] / da } else { 0.0 };
                    let b = if cb <= 0.5 {
                        2.0 * cs * cb
                    } else {
                        1.0 - 2.0 * (1.0 - cs) * (1.0 - cb)
                    };
                    sa * da * b + s[c] * (1.0 - da) + d[c] * (1.0 - sa)
                }
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
    area: Area,
    translate: (f32, f32),
    scale: f32,
    rotate_deg: f32,
) -> Vec<f32> {
    let w = area.rect.w as usize;
    let h = area.rect.h as usize;
    let mut out = vec![0.0f32; w * h * 4];
    if scale.abs() < 1e-6 {
        return out;
    }
    // GM10: C is the frame's centre, in the area's pixels (the area is the
    // frame everywhere but a camera's paper, where x = y = 0 changes nothing).
    let (cx, cy) = (
        area.fw as f32 / 2.0 - area.rect.x as f32,
        area.fh as f32 / 2.0 - area.rect.y as f32,
    );
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
            out[(py * w + px) * 4..][..4].copy_from_slice(&bilinear(src, w, h, sx, sy));
        }
    }
    out
}

/// The one bilinear sampler — the Transform node's, (GM10) the camera's
/// and (GM13) the motion blur's: `src` (`w × h`, premultiplied) at (sx, sy)
/// in pixel units, pixel centres at +0.5.
#[inline]
pub(crate) fn bilinear(src: &[f32], w: usize, h: usize, sx: f32, sy: f32) -> [f32; 4] {
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
    px_acc
}

/// GM10: a Camera's frame (`st` resolved at `frame`, output `fw × fh`):
/// each wired plane rendered at the cut's paper rect, blurred by its depth
/// of field (the FX core blur), then sampled through its plane map with the
/// one bilinear sampler and laid far to near, premultiplied src-over.
#[allow(clippy::too_many_arguments)]
fn camera_frame(
    cut: &Cut,
    law: ColorLaw,
    node: &Node,
    planes: &[PlaneSpec],
    st: &CameraState,
    frame: u32,
    (fw, fh): (u32, u32),
    visiting: &mut HashSet<NodeId>,
) -> Vec<f32> {
    let paper = camera::paper(cut, fw, fh);
    let plane_area = Area {
        rect: paper,
        fw,
        fh,
    };
    let (pw, ph) = (paper.w as usize, paper.h as usize);
    let (ox, oy) = (paper.x as f32, paper.y as f32);
    let mut out = vec![0.0f32; fw as usize * fh as usize * 4];
    let depths: Vec<f32> = planes.iter().map(|p| p.depth).collect();
    for i in camera::plane_order(&depths) {
        let Some((src, _)) = node.inputs.get(i).copied().flatten() else {
            continue;
        };
        let z = depths[i];
        let mut plane = render_node(cut, law, src, frame, plane_area, visiting);
        let r = st.dof_radius(z);
        if r > 0.0 {
            plane = crate::fx::blur(&Image::from_px(paper.w, paper.h, plane), r, r).px;
        }
        let map = st.plane_map(z, fw, fh);
        out.par_chunks_mut(fw as usize * 4)
            .enumerate()
            .for_each(|(py, row)| {
                for (px, d) in row.chunks_exact_mut(4).enumerate() {
                    let (sx, sy) = map.map(px as f32 + 0.5, py as f32 + 0.5);
                    let s = bilinear(&plane, pw, ph, sx - ox, sy - oy);
                    let keep = 1.0 - s[3];
                    for c in 0..4 {
                        d[c] = s[c] + d[c] * keep;
                    }
                }
            });
    }
    out
}

/// GM10: a frame-size render placed into `area` at the frame's own place
/// (a camera feeding another camera's plane).
fn place_frame(frame_px: &[f32], area: Area) -> Vec<f32> {
    let mut out = vec![0.0f32; area.len()];
    let (fw, aw, ah) = (area.fw as i64, area.rect.w as i64, area.rect.h as i64);
    for y in 0..area.fh as i64 {
        let ay = y - area.rect.y as i64;
        if !(0..ah).contains(&ay) {
            continue;
        }
        for x in 0..fw {
            let ax = x - area.rect.x as i64;
            if !(0..aw).contains(&ax) {
                continue;
            }
            let (s, d) = (((y * fw + x) * 4) as usize, ((ay * aw + ax) * 4) as usize);
            out[d..d + 4].copy_from_slice(&frame_px[s..s + 4]);
        }
    }
    out
}

/// Recursively render one node to a premultiplied f32 frame.
/// The graph structure guarantees acyclicity (connect() rejects cycles), but
/// the visiting set keeps a corrupted file from recursing forever.
fn render_node(
    cut: &Cut,
    law: ColorLaw,
    id: NodeId,
    frame: u32,
    area: Area,
    visiting: &mut HashSet<NodeId>,
) -> Vec<f32> {
    let (width, height) = (area.rect.w, area.rect.h);
    let empty = || vec![0.0f32; area.len()];
    if !visiting.insert(id) {
        return empty();
    }
    let Ok(node) = cut.graph.node(id) else {
        visiting.remove(&id);
        return empty();
    };
    let input = |slot: usize, visiting: &mut HashSet<NodeId>| -> Vec<f32> {
        match node.inputs.get(slot).copied().flatten() {
            Some((src, _pin)) => render_node(cut, law, src, frame, area, visiting),
            None => empty(),
        }
    };
    let out = match &node.kind {
        NodeKind::DrawingSource {
            column,
            roles,
            absorb_trace,
        } => {
            let mut acc = empty();
            // A hidden or Ref column yields transparent (GM5).
            if let Some(col) = cut.xsheet.column(*column)
                && cut.column_renders(*column)
                && let Some(did) = col.resolve(frame)
                && let Some(d) = cut.drawing(did)
            {
                // GM13: a role pass and the trace-line absorb (plain: the
                // flatten, exactly).
                let tiles = crate::look::source_tiles(d, roles.as_deref(), *absorb_trace);
                splat_flat(&mut acc, &tiles, area.rect);
            }
            acc
        }
        NodeKind::ImageSource { image } => {
            let mut acc = empty();
            if let Some(img) = cut.image(*image) {
                splat_flat(&mut acc, &img.tiles, area.rect);
            }
            acc
        }
        NodeKind::Solid { rgba } => {
            // D8: the chip enters through the project's law, like paint.
            let px = law.premultiplied(*rgba);
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
            binds,
        } => {
            let child = input(0, visiting);
            let (t, s, r) = cut.transform_at(*translate, *scale, *rotate_deg, binds, frame);
            transform_image(&child, area, t, s, r)
        }
        NodeKind::Blend { mode } => {
            let mut bottom = input(0, visiting);
            let top = input(1, visiting);
            blend_into(&mut bottom, &top, *mode);
            bottom
        }
        NodeKind::Output => input(0, visiting),
        NodeKind::Effect { fx, params, binds } => {
            // GM9 (D9): the one implementation of every FX. An unwired pin
            // is None (the glow's matte then restricts nothing).
            let ins: Vec<Option<Image>> = (0..fx.inputs())
                .map(|slot| {
                    node.inputs
                        .get(slot)
                        .copied()
                        .flatten()
                        .map(|_| Image::from_px(width, height, input(slot, visiting)))
                })
                .collect();
            let refs: Vec<Option<&Image>> = ins.iter().map(Option::as_ref).collect();
            let v = cut.fx_at(*fx, params, binds, frame);
            // GM13: Grain reads the frame, a LUT the cut's embedded file.
            let ctx = crate::fx::FxCtx {
                frame,
                blobs: Some(&cut.blobs),
            };
            fx.apply_in(&v, law, &refs, width, height, ctx).px
        }
        NodeKind::Camera {
            planes,
            params,
            binds,
        } => {
            // GM10 (D9): the camera sees the frame; under another camera's
            // plane its frame lands at the frame's place on that paper.
            let st = CameraState::resolve(cut, params, binds, frame);
            let px = camera_frame(cut, law, node, planes, &st, frame, (area.fw, area.fh), visiting);
            if area.is_frame() {
                px
            } else {
                place_frame(&px, area)
            }
        }
    };
    visiting.remove(&id);
    out
}

/// GM9 (D9): one node's subtree rendered at `frame` (premultiplied f32,
/// `width × height × 4`) — what the GPU compositor's worker renders for an
/// FX subtree and uploads, so the composite view shows the export's pixels.
pub fn render_node_px(
    cut: &Cut,
    law: ColorLaw,
    node: NodeId,
    frame: u32,
    width: u32,
    height: u32,
) -> Vec<f32> {
    let mut visiting = HashSet::new();
    render_node(cut, law, node, frame, Area::frame(width, height), &mut visiting)
}

/// GM10: one node's subtree rendered into `rect` (frame coordinates) of a
/// `width × height` frame (premultiplied f32, `rect.w × rect.h × 4`):
/// sources land at their own place on it, so a rect larger than the frame
/// shows the paper beyond it. At the frame's own rect it is exactly
/// [`render_node_px`]; a Camera's planes render this way at the paper.
pub fn render_node_rect(
    cut: &Cut,
    law: ColorLaw,
    node: NodeId,
    frame: u32,
    rect: IRect,
    width: u32,
    height: u32,
) -> Vec<f32> {
    let mut visiting = HashSet::new();
    let area = Area {
        rect,
        fw: width,
        fh: height,
    };
    render_node(cut, law, node, frame, area, &mut visiting)
}

/// Execute `cut.graph` at `frame` into a premultiplied f32 frame.
/// `None` when no output node is wired — the caller falls back to the
/// column-stack composite. GM11: the delivery renderer's graph path.
pub fn graph_composite(
    cut: &Cut,
    law: ColorLaw,
    frame: u32,
    width: u32,
    height: u32,
) -> Option<Vec<f32>> {
    let output = cut.graph.output?;
    let mut visiting = HashSet::new();
    Some(render_node(cut, law, output, frame, Area::frame(width, height), &mut visiting))
}

/// Graph-path render to straight RGBA8 over transparent (PNG-with-alpha).
/// `None` = no wired graph output; use [`render_frame`] instead. Legacy
/// law; [`render_graph_frame_law`] takes the project's.
pub fn render_graph_frame(cut: &Cut, frame: u32, width: u32, height: u32) -> Option<Vec<u8>> {
    render_graph_frame_law(cut, ColorLaw::Legacy, frame, width, height)
}

/// [`render_graph_frame`] under a colour law (D8). Stored values export
/// the same under both laws; only a Solid node's chip differs.
pub fn render_graph_frame_law(
    cut: &Cut,
    law: ColorLaw,
    frame: u32,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    graph_composite(cut, law, frame, width, height).map(|acc| to_straight_rgba8(&acc))
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
    render_graph_frame_over_law(cut, ColorLaw::Legacy, frame, width, height, bg)
}

/// [`render_graph_frame_over`] under a colour law (D8).
pub fn render_graph_frame_over_law(
    cut: &Cut,
    law: ColorLaw,
    frame: u32,
    width: u32,
    height: u32,
    bg: [u8; 3],
) -> Option<Vec<u8>> {
    graph_composite(cut, law, frame, width, height).map(|acc| to_rgba8_over(&acc, bg))
}
