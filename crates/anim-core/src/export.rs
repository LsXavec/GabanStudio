//! Headless frame export: composite a cut's columns at one frame into a flat
//! RGBA8 image. Pure CPU — the same premultiplied-over math as the GPU
//! sandwich compositor (tile values are f16 bit patterns; see the encoding
//! law on [`crate::raster::TileData`]), assembled full-frame.
//!
//! Column order = X-sheet order (first column = bottom). Each drawing's
//! visible cel layers are flattened bottom→top first. Vector strokes are NOT
//! rasterized here (raster layers only) — callers should surface that
//! honestly if a cut still carries legacy vector artwork.

use crate::model::Cut;
use crate::raster::{TILE, f16_bits_to_f32};

/// Premultiplied f32 full-frame composite of every column at `frame`.
fn composite(cut: &Cut, frame: u32, width: u32, height: u32) -> Vec<f32> {
    let w = width as usize;
    let h = height as usize;
    let mut acc = vec![0.0f32; w * h * 4];

    for col in &cut.xsheet.columns {
        let Some(id) = col.resolve(frame) else {
            continue;
        };
        let Some(d) = cut.drawing(id) else {
            continue;
        };
        let flat = d.flatten();
        for ((tx, ty), tile) in &flat {
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
    acc
}

/// Render one frame to straight (un-premultiplied) RGBA8 over a TRANSPARENT
/// background — for PNG sequences that keep alpha.
pub fn render_frame(cut: &Cut, frame: u32, width: u32, height: u32) -> Vec<u8> {
    let acc = composite(cut, frame, width, height);
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

/// Render one frame composited over an opaque background colour — for video
/// formats without alpha (RGB values land in the alpha=255 RGBA8 output).
pub fn render_frame_over(
    cut: &Cut,
    frame: u32,
    width: u32,
    height: u32,
    bg: [u8; 3],
) -> Vec<u8> {
    let acc = composite(cut, frame, width, height);
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

/// Count drawings that still carry legacy vector strokes (not exported).
pub fn vector_stroke_cels(cut: &Cut) -> usize {
    cut.drawings.iter().filter(|d| !d.strokes.is_empty()).count()
}
