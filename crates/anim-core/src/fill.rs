//! Flood fill — the ink-&-paint (shiage) verb, headless. The anime model,
//! not the photo one: the fill region is the connected TRANSPARENT area
//! around the seed, bounded by inked pixels of a REFERENCE image (usually
//! the whole cel's flatten, so line art bounds a fill landing on the color
//! layer). Two industry behaviors are built in:
//!
//! * **Gap closing** — line art has small breaks; boundaries are dilated by
//!   ceil(gap/2) before the flood so gaps up to `gap_px` act closed.
//! * **Grow (paint under the lines)** — the filled region is dilated back
//!   out by the same amount PLUS `grow_px`, so flats tuck under
//!   anti-aliased line edges instead of leaving a pale halo.
//!
//! Fills are HARD-EDGED by design (cel flats, not airbrushes). The result
//! is applied to the target layer as one `PaintTiles` diff — one click =
//! one exact undo step, same law as every other edit.
//!
//! HONEST LIMITS: legacy VECTOR strokes are not rasterized here, so they do
//! not bound fills (the same limitation as export); grow can cross a line
//! thinner than itself by (grow − thickness) px — the same accepted artifact
//! as CSP/Krita's fill expansion.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::raster::{
    TILE, TILE_LEN, TileCoord, TileData, TileDiff, f16_bits_to_f32, f32_to_f16_bits,
};

#[derive(Clone, Copy, Debug)]
pub struct FillOpts {
    /// Reference alpha at or above this is a boundary ("ink").
    pub threshold: f32,
    /// Line-art gaps up to this many pixels act closed.
    pub gap_px: u32,
    /// How far the fill extends UNDER the boundary lines.
    pub grow_px: u32,
    /// Never paint over the reference's own inked pixels. For LAYER-mode
    /// fills (reference == target layer) "tucking under the lines" would
    /// overwrite the very lines bounding the fill — protection keeps grow
    /// useful for gap recovery while the ink survives. Cel-mode fills leave
    /// this off: the lines live on another layer, tucking under is the point.
    pub protect_ink: bool,
}

impl Default for FillOpts {
    fn default() -> Self {
        Self {
            threshold: 0.1,
            gap_px: 2,
            grow_px: 2,
            protect_ink: false,
        }
    }
}

/// A filled region as a full-canvas bitmask (row-major, w×h).
pub struct FillMask {
    pub w: u32,
    pub h: u32,
    pub bits: Vec<bool>,
}

impl FillMask {
    pub fn get(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as u32) < self.w
            && (y as u32) < self.h
            && self.bits[y as usize * self.w as usize + x as usize]
    }

    pub fn count(&self) -> usize {
        self.bits.iter().filter(|b| **b).count()
    }
}

/// Separable square (Chebyshev) dilation by radius `r`, in place —
/// two sliding-window passes, O(w·h) regardless of r.
fn dilate(bits: &mut [bool], w: usize, h: usize, r: usize) {
    if r == 0 {
        return;
    }
    let mut tmp = vec![false; w * h];
    // Rows.
    for y in 0..h {
        let row = &bits[y * w..(y + 1) * w];
        let mut count = 0usize; // trues in the current window
        for &b in row.iter().take(w.min(r)) {
            count += b as usize;
        }
        for x in 0..w {
            if x + r < w {
                count += row[x + r] as usize;
            }
            if x > r {
                count -= row[x - r - 1] as usize;
            }
            tmp[y * w + x] = count > 0;
        }
    }
    // Columns.
    for x in 0..w {
        let mut count = 0usize;
        for y in 0..h.min(r) {
            count += tmp[y * w + x] as usize;
        }
        for y in 0..h {
            if y + r < h {
                count += tmp[(y + r) * w + x] as usize;
            }
            if y > r {
                count -= tmp[(y - r - 1) * w + x] as usize;
            }
            bits[y * w + x] = count > 0;
        }
    }
}

/// Why a fill produced no region — callers word their refusal messages from
/// the actual cause, not a guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRefusal {
    /// Seed outside the canvas.
    OffPaper,
    /// Seed on (gap-closed) ink — fills flow into empty regions.
    OnInk,
    /// Seed outside the active selection (fills respect selections; an
    /// outside click is a no-op, the industry rule).
    OutsideSelection,
    /// The selection excluded every filled pixel.
    ClippedOut,
}

/// Compute the fill region for a click at `seed` (paper pixels). `clip` (a
/// paper-space polygon, e.g. the select tool's active selection) confines
/// the fill: the selection border is a WALL during the flood (no tunnelling
/// through unselected territory), an outside seed is refused, and the final
/// mask is re-clipped after grow.
pub fn flood_fill_mask(
    reference: &BTreeMap<TileCoord, Arc<TileData>>,
    seed: (i32, i32),
    width: u32,
    height: u32,
    opts: &FillOpts,
    clip: Option<&[(f32, f32)]>,
) -> Result<FillMask, FillRefusal> {
    let (w, h) = (width as usize, height as usize);
    if w == 0
        || h == 0
        || seed.0 < 0
        || seed.1 < 0
        || seed.0 >= width as i32
        || seed.1 >= height as i32
    {
        return Err(FillRefusal::OffPaper);
    }
    let clip = clip.filter(|p| p.len() >= 3);
    // Selection bbox in pixel bounds — culls the per-pixel polygon tests
    // (everything outside the bbox is outside the polygon by definition).
    let clip_bbox = clip.map(|poly| {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (px, py) in poly {
            x0 = x0.min(*px);
            y0 = y0.min(*py);
            x1 = x1.max(*px);
            y1 = y1.max(*py);
        }
        (
            (x0.floor() as i64).clamp(0, w as i64) as usize,
            (y0.floor() as i64).clamp(0, h as i64) as usize,
            (x1.ceil() as i64).clamp(0, w as i64) as usize,
            (y1.ceil() as i64).clamp(0, h as i64) as usize,
        )
    });
    if let Some(poly) = clip
        && !crate::edit::contains(poly, seed.0 as f32 + 0.5, seed.1 as f32 + 0.5)
    {
        return Err(FillRefusal::OutsideSelection);
    }

    // Boundary grid from the reference: alpha ≥ threshold = ink.
    let mut boundary = vec![false; w * h];
    for ((tx, ty), tile) in reference {
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
                let a = f16_bits_to_f32(tile.rgba[(row * TILE + cx) * 4 + 3]);
                if a >= opts.threshold {
                    boundary[py as usize * w + px as usize] = true;
                }
            }
        }
    }

    // Close gaps: fatten every line by ceil(gap/2) so breaks up to gap_px
    // act sealed for the flood.
    let r_close = opts.gap_px.div_ceil(2) as usize;
    let mut closed = boundary.clone();
    dilate(&mut closed, w, h, r_close);

    // The selection border is a WALL for the traversal (applied AFTER the
    // gap dilation so selection edges aren't fattened like ink): the flood
    // can never tunnel around a line through unselected territory.
    if let (Some(poly), Some((bx0, by0, bx1, by1))) = (clip, clip_bbox) {
        for y in 0..h {
            let inside_band = y >= by0 && y < by1;
            for x in 0..w {
                let i = y * w + x;
                if closed[i] {
                    continue;
                }
                // Outside the bbox = outside the polygon (cheap cull); inside
                // it, the real even-odd test decides.
                let outside = !inside_band
                    || x < bx0
                    || x >= bx1
                    || !crate::edit::contains(poly, x as f32 + 0.5, y as f32 + 0.5);
                if outside {
                    closed[i] = true;
                }
            }
        }
    }

    let seed_i = seed.1 as usize * w + seed.0 as usize;
    if closed[seed_i] {
        return Err(FillRefusal::OnInk); // ink, or the gap-closing band
    }

    // Scanline flood over the non-ink grid. Canvas edges are walls.
    let mut bits = vec![false; w * h];
    let mut stack = vec![(seed.0 as usize, seed.1 as usize)];
    while let Some((sx, sy)) = stack.pop() {
        let rowbase = sy * w;
        if bits[rowbase + sx] || closed[rowbase + sx] {
            continue;
        }
        // Expand to the span's left edge.
        let mut x0 = sx;
        while x0 > 0 && !closed[rowbase + x0 - 1] && !bits[rowbase + x0 - 1] {
            x0 -= 1;
        }
        let mut x = x0;
        let (mut up_open, mut down_open) = (false, false);
        while x < w && !closed[rowbase + x] && !bits[rowbase + x] {
            bits[rowbase + x] = true;
            if sy > 0 {
                let above = rowbase - w + x;
                let open = !closed[above] && !bits[above];
                if open && !up_open {
                    stack.push((x, sy - 1));
                }
                up_open = open;
            }
            if sy + 1 < h {
                let below = rowbase + w + x;
                let open = !closed[below] && !bits[below];
                if open && !down_open {
                    stack.push((x, sy + 1));
                }
                down_open = open;
            }
            x += 1;
        }
    }

    // Push the fill back to the true line edges (undo the closing band) and
    // grow_px further UNDER the ink. gap 0 + grow 0 = the exact flood.
    dilate(&mut bits, w, h, r_close + opts.grow_px as usize);

    // Layer-mode protection: the reference IS the target — never paint over
    // its own inked pixels.
    if opts.protect_ink {
        for (b, ink) in bits.iter_mut().zip(&boundary) {
            *b = *b && !ink;
        }
    }

    // Re-clip after grow (the dilate above can push paint back across the
    // selection border). Bbox-culled: outside the bbox = outside the poly.
    if let (Some(poly), Some((bx0, by0, bx1, by1))) = (clip, clip_bbox) {
        for y in 0..h {
            let inside_band = y >= by0 && y < by1;
            for x in 0..w {
                if !bits[y * w + x] {
                    continue;
                }
                if !inside_band
                    || x < bx0
                    || x >= bx1
                    || !crate::edit::contains(poly, x as f32 + 0.5, y as f32 + 0.5)
                {
                    bits[y * w + x] = false;
                }
            }
        }
        if !bits.iter().any(|b| *b) {
            return Err(FillRefusal::ClippedOut);
        }
    }

    Ok(FillMask {
        w: width,
        h: height,
        bits,
    })
}

/// Composite `color` (premultiplied) over the target layer wherever the mask
/// is set, as a `PaintTiles` diff. No-op tiles are skipped.
pub fn fill_diff(
    target: &BTreeMap<TileCoord, Arc<TileData>>,
    mask: &FillMask,
    color: [f32; 4],
) -> TileDiff {
    if color[3] <= 0.0 {
        return Vec::new();
    }
    let t0x = 0i32;
    let t0y = 0i32;
    let t1x = (mask.w as i32 - 1).div_euclid(TILE as i32);
    let t1y = (mask.h as i32 - 1).div_euclid(TILE as i32);
    let keep = 1.0 - color[3];

    let mut diff = Vec::new();
    for ty in t0y..=t1y {
        for tx in t0x..=t1x {
            // Any mask coverage in this tile?
            let mut any = false;
            'scan: for row in 0..TILE {
                for cx in 0..TILE {
                    if mask.get(tx * TILE as i32 + cx as i32, ty * TILE as i32 + row as i32) {
                        any = true;
                        break 'scan;
                    }
                }
            }
            if !any {
                continue;
            }
            let before = target.get(&(tx, ty)).cloned();
            let mut px: Vec<f32> = match &before {
                Some(t) => t.rgba.iter().map(|&b| f16_bits_to_f32(b)).collect(),
                None => vec![0.0; TILE_LEN],
            };
            for row in 0..TILE {
                for cx in 0..TILE {
                    if !mask.get(tx * TILE as i32 + cx as i32, ty * TILE as i32 + row as i32) {
                        continue;
                    }
                    let i = (row * TILE + cx) * 4;
                    for c in 0..4 {
                        px[i + c] = color[c] + px[i + c] * keep;
                    }
                }
            }
            let after_tile =
                TileData::from_vec(px.iter().map(|&v| f32_to_f16_bits(v)).collect());
            let after = if after_tile.is_empty() {
                None
            } else {
                Some(Arc::new(after_tile))
            };
            let same = match (&before, &after) {
                (None, None) => true,
                (Some(a), Some(b)) => a.hash == b.hash && a.rgba == b.rgba,
                _ => false,
            };
            if !same {
                diff.push(((tx, ty), before, after));
            }
        }
    }
    diff
}
