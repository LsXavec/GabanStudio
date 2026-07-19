//! Region editing — the pure pixel math behind the app's select/move/scale/
//! rotate tool. Headless by law: everything here is CPU tile math with exact
//! results the app orchestrates into ONE `PaintTiles` command (whose diff is
//! its own exact inverse), so a whole transform gesture = one undo step.
//!
//! Flow: `lift_region`/`lift_all` cut the selected pixels out of a layer's
//! tiles into a [`FloatingPatch`] (premultiplied f32) plus the cleared
//! after-states of the touched tiles → the app previews the patch under a
//! live [`Affine`] → `transform_patch` resamples it (same inverse-map
//! bilinear law as the graph Transform node: out-of-patch taps are
//! transparent, |scale| < 1e-6 is degenerate) → `merge_patch` composites it
//! over the cleared layer and emits the `PaintTiles` diff.
//!
//! Texels are f16 BIT PATTERNS (see the encoding law on
//! [`crate::raster::TileData`]); this module decodes to f32 once at lift and
//! re-encodes once at merge, so an identity transform round-trips
//! bit-exactly.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::raster::{
    TILE, TILE_LEN, TileCoord, TileData, TileDiff, f16_bits_to_f32, f32_to_f16_bits,
};

/// Lifted pixels, premultiplied f32, anchored at (x0, y0) in paper space.
#[derive(Clone, Debug)]
pub struct FloatingPatch {
    pub x0: i32,
    pub y0: i32,
    pub w: u32,
    pub h: u32,
    /// Row-major RGBA, len = w*h*4, premultiplied.
    pub rgba: Vec<f32>,
}

impl FloatingPatch {
    fn sample(&self, x: i64, y: i64) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return [0.0; 4];
        }
        let i = ((y as usize) * self.w as usize + x as usize) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// Any pixel with non-zero alpha?
    pub fn has_ink(&self) -> bool {
        self.rgba.chunks_exact(4).any(|p| p[3] > 0.0)
    }
}

/// The gesture's transform: `p_out = pivot + R_θ · (s · (p_in − pivot)) + t`
/// — uniform scale then rotation about the pivot, then a pixel translate.
/// Same conventions as the graph Transform node (θ clockwise, y-down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub pivot: (f32, f32),
    pub translate: (f32, f32),
    pub rotate_rad: f32,
    pub scale: f32,
}

impl Affine {
    pub const IDENTITY_SCALE_EPS: f32 = 1e-6;

    pub fn identity(pivot: (f32, f32)) -> Self {
        Self {
            pivot,
            translate: (0.0, 0.0),
            rotate_rad: 0.0,
            scale: 1.0,
        }
    }

    pub fn is_identity(&self) -> bool {
        self.translate == (0.0, 0.0) && self.rotate_rad == 0.0 && self.scale == 1.0
    }

    /// Forward-map a paper-space point.
    pub fn apply(&self, p: (f32, f32)) -> (f32, f32) {
        let (sin, cos) = self.rotate_rad.sin_cos();
        let ox = (p.0 - self.pivot.0) * self.scale;
        let oy = (p.1 - self.pivot.1) * self.scale;
        (
            self.pivot.0 + cos * ox - sin * oy + self.translate.0,
            self.pivot.1 + sin * ox + cos * oy + self.translate.1,
        )
    }

    /// Inverse-map a paper-space point (assumes non-degenerate scale).
    fn unapply(&self, p: (f32, f32)) -> (f32, f32) {
        let (sin, cos) = self.rotate_rad.sin_cos();
        let ox = p.0 - self.pivot.0 - self.translate.0;
        let oy = p.1 - self.pivot.1 - self.translate.1;
        let inv_s = 1.0 / self.scale;
        (
            self.pivot.0 + (cos * ox + sin * oy) * inv_s,
            self.pivot.1 + (-sin * ox + cos * oy) * inv_s,
        )
    }
}

/// Point-in-polygon (even-odd crossing rule) at pixel centres. A rect
/// selection is just a 4-point polygon through the same path.
fn contains(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) {
            let t = (y - yi) / (yj - yi);
            if x < xi + t * (xj - xi) {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Integer AABB of a polygon, expanded to pixel bounds.
fn poly_bbox(poly: &[(f32, f32)]) -> Option<(i32, i32, i32, i32)> {
    let mut it = poly.iter();
    let first = it.next()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.0, first.1, first.0, first.1);
    for p in it {
        x0 = x0.min(p.0);
        y0 = y0.min(p.1);
        x1 = x1.max(p.0);
        y1 = y1.max(p.1);
    }
    Some((
        x0.floor() as i32,
        y0.floor() as i32,
        x1.ceil() as i32,
        y1.ceil() as i32,
    ))
}

/// The lift result: the floating pixels plus the after-states of every tile
/// the lift touched (`None` = the tile became empty and should be removed).
pub struct Lift {
    pub patch: FloatingPatch,
    pub cleared: Vec<(TileCoord, Option<Arc<TileData>>)>,
}

/// Cut the pixels inside `poly` (paper-space polygon, ≥3 points) out of
/// `tiles`. Returns `None` if the selection covers no inked pixels — a lift
/// that would just clear nothing.
pub fn lift_region(
    tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    poly: &[(f32, f32)],
) -> Option<Lift> {
    let (bx0, by0, bx1, by1) = poly_bbox(poly)?;
    lift_masked(tiles, bx0, by0, bx1, by1, |x, y| contains(poly, x, y))
}

/// Lift EVERY inked pixel of the layer (select-all / whole-layer move).
pub fn lift_all(tiles: &BTreeMap<TileCoord, Arc<TileData>>) -> Option<Lift> {
    let mut it = tiles.keys();
    let first = it.next()?;
    let (mut tx0, mut ty0, mut tx1, mut ty1) = (first.0, first.1, first.0, first.1);
    for t in it {
        tx0 = tx0.min(t.0);
        ty0 = ty0.min(t.1);
        tx1 = tx1.max(t.0);
        ty1 = ty1.max(t.1);
    }
    lift_masked(
        tiles,
        tx0 * TILE as i32,
        ty0 * TILE as i32,
        (tx1 + 1) * TILE as i32,
        (ty1 + 1) * TILE as i32,
        |_, _| true,
    )
}

fn lift_masked(
    tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    bx0: i32,
    by0: i32,
    bx1: i32,
    by1: i32,
    mask: impl Fn(f32, f32) -> bool,
) -> Option<Lift> {
    let w = (bx1 - bx0).max(0) as u32;
    let h = (by1 - by0).max(0) as u32;
    if w == 0 || h == 0 {
        return None;
    }
    let mut patch = FloatingPatch {
        x0: bx0,
        y0: by0,
        w,
        h,
        rgba: vec![0.0; w as usize * h as usize * 4],
    };
    let mut cleared: Vec<(TileCoord, Option<Arc<TileData>>)> = Vec::new();

    // Tiles the bbox overlaps.
    let t0x = bx0.div_euclid(TILE as i32);
    let t0y = by0.div_euclid(TILE as i32);
    let t1x = (bx1 - 1).div_euclid(TILE as i32);
    let t1y = (by1 - 1).div_euclid(TILE as i32);
    let mut lifted_any = false;

    for ty in t0y..=t1y {
        for tx in t0x..=t1x {
            let Some(tile) = tiles.get(&(tx, ty)) else {
                continue;
            };
            let mut out = tile.rgba.to_vec();
            let mut touched = false;
            for row in 0..TILE {
                let py = ty * TILE as i32 + row as i32;
                if py < by0 || py >= by1 {
                    continue;
                }
                for cx in 0..TILE {
                    let px = tx * TILE as i32 + cx as i32;
                    if px < bx0 || px >= bx1 {
                        continue;
                    }
                    let i = (row * TILE + cx) * 4;
                    if out[i + 3] & 0x7FFF == 0 {
                        continue; // transparent texel — nothing to lift
                    }
                    // Mask test at the pixel CENTRE.
                    if !mask(px as f32 + 0.5, py as f32 + 0.5) {
                        continue;
                    }
                    let pi = ((py - by0) as usize * w as usize + (px - bx0) as usize) * 4;
                    for c in 0..4 {
                        patch.rgba[pi + c] = f16_bits_to_f32(out[i + c]);
                        out[i + c] = 0;
                    }
                    touched = true;
                    lifted_any = true;
                }
            }
            if touched {
                let after = TileData::from_vec(out);
                cleared.push((
                    (tx, ty),
                    if after.is_empty() {
                        None
                    } else {
                        Some(Arc::new(after))
                    },
                ));
            }
        }
    }
    if !lifted_any {
        return None;
    }
    Some(Lift { patch, cleared })
}

/// Cap on a transformed patch's pixel dimensions — a wild scale gesture must
/// not allocate gigabytes. At the cap the gesture still previews; the commit
/// resamples into this bounded buffer. The capped window is CENTRED ON THE
/// GESTURE'S FIXED POINT (pivot + translate), so what survives is the
/// content nearest the pivot — the far overhang is what's lost, like
/// dragging art off any finite canvas.
pub const MAX_PATCH_DIM: u32 = 8192;

/// Resample `patch` under `affine` into a new axis-aligned patch. Inverse-map
/// bilinear; taps outside the source are transparent (the graph Transform
/// law). A degenerate scale returns an empty 1×1 patch.
pub fn transform_patch(patch: &FloatingPatch, affine: &Affine) -> FloatingPatch {
    let empty = || FloatingPatch {
        x0: 0,
        y0: 0,
        w: 1,
        h: 1,
        rgba: vec![0.0; 4],
    };
    if affine.scale.abs() < Affine::IDENTITY_SCALE_EPS {
        return empty();
    }
    // Output bounds = AABB of the forward-mapped source corners.
    let corners = [
        (patch.x0 as f32, patch.y0 as f32),
        ((patch.x0 + patch.w as i32) as f32, patch.y0 as f32),
        (patch.x0 as f32, (patch.y0 + patch.h as i32) as f32),
        (
            (patch.x0 + patch.w as i32) as f32,
            (patch.y0 + patch.h as i32) as f32,
        ),
    ];
    let mapped: Vec<(f32, f32)> = corners.iter().map(|c| affine.apply(*c)).collect();
    let Some((bx0, by0, bx1, by1)) = poly_bbox(&mapped) else {
        return empty();
    };
    // Bilinear support extends past the mapped-corner AABB: an edge texel
    // contributes up to 0.5·|s|·(|sin|+|cos|) beyond it (rotation fringe;
    // grows with scale-up). Pad the output bounds or that fringe is cropped.
    let (sin, cos) = affine.rotate_rad.sin_cos();
    let pad = (0.5 * affine.scale.abs() * (sin.abs() + cos.abs())).ceil() as i32 + 1;
    let (mut ox0, mut oy0) = (bx0 - pad, by0 - pad);
    let (ox1, oy1) = (bx1 + pad, by1 + pad);
    let full_w = (ox1 - ox0).max(1) as u32;
    let full_h = (oy1 - oy0).max(1) as u32;
    let ow = full_w.min(MAX_PATCH_DIM);
    let oh = full_h.min(MAX_PATCH_DIM);
    // A capped window anchors on the gesture's FIXED POINT so on-canvas
    // content near the pivot survives; only the far overhang is dropped.
    if full_w > MAX_PATCH_DIM {
        let cx = affine.pivot.0 + affine.translate.0;
        ox0 = ((cx - ow as f32 / 2.0).round() as i32).clamp(ox0, ox1 - ow as i32);
    }
    if full_h > MAX_PATCH_DIM {
        let cy = affine.pivot.1 + affine.translate.1;
        oy0 = ((cy - oh as f32 / 2.0).round() as i32).clamp(oy0, oy1 - oh as i32);
    }
    let mut out = FloatingPatch {
        x0: ox0,
        y0: oy0,
        w: ow,
        h: oh,
        rgba: vec![0.0; ow as usize * oh as usize * 4],
    };
    for py in 0..oh as i64 {
        for px in 0..ow as i64 {
            // Output pixel centre → source sample position (patch-local).
            let (sx, sy) = affine.unapply((
                (out.x0 as i64 + px) as f32 + 0.5,
                (out.y0 as i64 + py) as f32 + 0.5,
            ));
            let ix = sx - patch.x0 as f32 - 0.5;
            let iy = sy - patch.y0 as f32 - 0.5;
            let x0f = ix.floor();
            let y0f = iy.floor();
            let (fx, fy) = (ix - x0f, iy - y0f);
            let mut acc = [0.0f32; 4];
            for (dy, wy) in [(0i64, 1.0 - fy), (1, fy)] {
                if wy <= 0.0 {
                    continue;
                }
                for (dx, wx) in [(0i64, 1.0 - fx), (1, fx)] {
                    if wx <= 0.0 {
                        continue;
                    }
                    let s = patch.sample(x0f as i64 + dx, y0f as i64 + dy);
                    let wgt = wx * wy;
                    for c in 0..4 {
                        acc[c] += s[c] * wgt;
                    }
                }
            }
            let i = (py as usize * ow as usize + px as usize) * 4;
            out.rgba[i..i + 4].copy_from_slice(&acc);
        }
    }
    out
}

/// Composite the (already transformed) `patch` over the lifted layer and emit
/// the `PaintTiles` diff: for every tile whose content changes,
/// `(coord, before-from-base, after)`. No-op tiles are skipped, so an
/// identity gesture yields an empty diff (the app skips the command).
pub fn merge_patch(
    base: &BTreeMap<TileCoord, Arc<TileData>>,
    cleared: &[(TileCoord, Option<Arc<TileData>>)],
    patch: &FloatingPatch,
) -> TileDiff {
    use std::collections::BTreeSet;

    let cleared_map: BTreeMap<TileCoord, &Option<Arc<TileData>>> =
        cleared.iter().map(|(c, t)| (*c, t)).collect();

    // Every tile the lift touched or the patch bbox covers.
    let mut coords: BTreeSet<TileCoord> = cleared_map.keys().copied().collect();
    let t0x = patch.x0.div_euclid(TILE as i32);
    let t0y = patch.y0.div_euclid(TILE as i32);
    let t1x = (patch.x0 + patch.w as i32 - 1).div_euclid(TILE as i32);
    let t1y = (patch.y0 + patch.h as i32 - 1).div_euclid(TILE as i32);
    for ty in t0y..=t1y {
        for tx in t0x..=t1x {
            coords.insert((tx, ty));
        }
    }

    let mut diff = Vec::new();
    for coord in coords {
        let before = base.get(&coord).cloned();
        // Start from the cleared state (lift applied), else the base tile.
        let start: Option<Arc<TileData>> = match cleared_map.get(&coord) {
            Some(after_clear) => (*after_clear).clone(),
            None => before.clone(),
        };
        let mut px: Vec<f32> = match &start {
            Some(t) => t.rgba.iter().map(|&b| f16_bits_to_f32(b)).collect(),
            None => vec![0.0; TILE_LEN],
        };
        // Splat the patch region overlapping this tile, premultiplied over.
        let (tx, ty) = coord;
        let mut changed_by_patch = false;
        for row in 0..TILE {
            let py = ty * TILE as i32 + row as i32;
            let sy = py as i64 - patch.y0 as i64;
            if sy < 0 || sy >= patch.h as i64 {
                continue;
            }
            for cx in 0..TILE {
                let pxx = tx * TILE as i32 + cx as i32;
                let sx = pxx as i64 - patch.x0 as i64;
                if sx < 0 || sx >= patch.w as i64 {
                    continue;
                }
                let s = patch.sample(sx, sy);
                if s[3] <= 0.0 {
                    continue;
                }
                let i = (row * TILE + cx) * 4;
                let keep = 1.0 - s[3];
                for c in 0..4 {
                    px[i + c] = s[c] + px[i + c] * keep;
                }
                changed_by_patch = true;
            }
        }
        let cleared_this = cleared_map.contains_key(&coord);
        if !changed_by_patch && !cleared_this {
            continue; // patch bbox covered it but contributed nothing
        }
        let after_tile = TileData::from_vec(px.iter().map(|&v| f32_to_f16_bits(v)).collect());
        let after = if after_tile.is_empty() {
            None
        } else {
            Some(Arc::new(after_tile))
        };
        // Skip no-ops (identity gestures reassemble the original bytes).
        let same = match (&before, &after) {
            (None, None) => true,
            (Some(a), Some(b)) => a.hash == b.hash && a.rgba == b.rgba,
            _ => false,
        };
        if !same {
            diff.push((coord, before, after));
        }
    }
    diff
}
