//! Tile-based raster storage — the CPU-side pixel authority (Krita/MyPaint model).
//!
//! Pixels live here as plain bytes (`Arc<TileData>`), never GPU types, so the
//! headless-engine law holds: anim-core stays testable with no wgpu. The app
//! owns the GPU; it reads painted tiles back into here and uploads them for
//! display. Tiles are 64×64 premultiplied RGBA16, content-hashed so identical
//! content dedupes and the evaluator's cache survives save/load.

use std::collections::BTreeMap;
use std::sync::Arc;

/// Tile edge length in pixels (matches Krita/MyPaint).
pub const TILE: usize = 64;
/// u16 count per tile: 64×64 pixels × 4 channels (RGBA16).
pub const TILE_LEN: usize = TILE * TILE * 4;

/// Tile grid coordinate (tile-space, not pixels). Tile `(tx, ty)` covers
/// pixels `[tx*64 .. tx*64+64) × [ty*64 .. ty*64+64)`.
pub type TileCoord = (i32, i32);

/// A raster edit as a set of per-tile before/after states. `None` = the tile
/// was/should-be absent (transparent). This is the payload of the `PaintTiles`
/// command; its exact inverse just swaps `before`/`after` on every entry, so
/// raster paint slots into the existing undo history like any other command.
pub type TileDiff = Vec<(TileCoord, Option<Arc<TileData>>, Option<Arc<TileData>>)>;

/// One 64×64 premultiplied-RGBA16F tile plus its content hash.
///
/// ENCODING LAW: each u16 is an IEEE-754 HALF-FLOAT BIT PATTERN (the GPU's
/// Rgba16Float texel format, copied byte-for-byte by the readback/upload
/// paths and persisted as BLOBs) — NOT a 0..=65535 normalized integer. Any
/// CPU math over tile values must go through [`f16_bits_to_f32`] /
/// [`f32_to_f16_bits`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TileData {
    /// Row-major RGBA16F bit patterns, length [`TILE_LEN`], premultiplied alpha.
    pub rgba: Box<[u16]>,
    /// fnv1a over the little-endian bytes — the content-address of this tile.
    pub hash: u64,
}

/// Decode an IEEE-754 binary16 bit pattern to f32.
pub fn f16_bits_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) as u32) << 31;
    let exp = ((bits >> 10) & 0x1F) as u32;
    let frac = (bits & 0x3FF) as u32;
    let out = match (exp, frac) {
        (0, 0) => sign,                          // ±0
        (0, _) => {
            // Subnormal: normalize into f32.
            let mut e: i32 = -14 + 127;
            let mut f = frac;
            while f & 0x400 == 0 {
                f <<= 1;
                e -= 1;
            }
            sign | ((e as u32) << 23) | ((f & 0x3FF) << 13)
        }
        (0x1F, 0) => sign | 0x7F80_0000,          // ±inf
        (0x1F, _) => sign | 0x7FC0_0000,          // NaN
        _ => sign | ((exp + 127 - 15) << 23) | (frac << 13),
    };
    f32::from_bits(out)
}

/// Encode an f32 to the nearest IEEE-754 binary16 bit pattern
/// (round-to-nearest-even; overflow saturates to ±inf).
pub fn f32_to_f16_bits(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 31) as u16) << 15;
    let exp = ((b >> 23) & 0xFF) as i32;
    let frac = b & 0x7F_FFFF;
    if exp == 0xFF {
        // inf / NaN
        return sign | 0x7C00 | if frac != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1F {
        return sign | 0x7C00; // overflow → inf
    }
    if e <= 0 {
        if e < -10 {
            return sign; // underflow → ±0
        }
        // Subnormal half.
        let frac = frac | 0x80_0000;
        let shift = (14 - e) as u32; // 11..=24
        let half = frac >> shift;
        let rem = frac & ((1u32 << shift) - 1);
        let halfway = 1u32 << (shift - 1);
        let rounded =
            half + u32::from(rem > halfway || (rem == halfway && (half & 1) == 1));
        return sign | rounded as u16;
    }
    let half = (frac >> 13) as u16;
    let rem = frac & 0x1FFF;
    let mut out = sign | ((e as u16) << 10) | half;
    if rem > 0x1000 || (rem == 0x1000 && (half & 1) == 1) {
        out = out.wrapping_add(1); // may carry into the exponent — correct
    }
    out
}

impl TileData {
    pub fn new(rgba: Box<[u16]>) -> Self {
        assert_eq!(rgba.len(), TILE_LEN, "tile must be TILE_LEN u16s");
        let hash = crate::value::fnv1a(bytemuck::cast_slice(&rgba));
        Self { rgba, hash }
    }

    pub fn from_vec(v: Vec<u16>) -> Self {
        Self::new(v.into_boxed_slice())
    }

    /// The raw little-endian bytes (for BLOB persistence).
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.rgba)
    }

    /// Rebuild from persisted little-endian bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != TILE_LEN * 2 {
            return None;
        }
        let rgba: Vec<u16> = bytemuck::cast_slice::<u8, u16>(bytes).to_vec();
        Some(Self::from_vec(rgba))
    }

    /// True if every pixel is fully transparent (alpha ±0.0 in f16 bits) —
    /// such tiles need not be stored (a missing tile means "empty there").
    pub fn is_empty(&self) -> bool {
        // Premultiplied: transparent => alpha 0. f16 ±0.0 = 0x0000 / 0x8000.
        self.rgba.chunks_exact(4).all(|px| px[3] & 0x7FFF == 0)
    }
}

/// One raster surface: a sparse grid of tiles. PURE pixels — opacity and other
/// compositing properties live on the owning [`crate::model::CelLayer`], so one
/// surface has exactly one home for each property.
/// A missing tile is fully transparent, so a blank layer costs nothing.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct RasterLayer {
    /// BTreeMap (not HashMap) so `content_hash` folds tiles in a deterministic
    /// order — hash stability is load-bearing for the eval cache and tests.
    pub tiles: BTreeMap<TileCoord, Arc<TileData>>,
}

impl RasterLayer {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Content-addressable hash: folds `(coord, tile.hash)` in sorted order.
    /// Cheap — no pixel bytes, just per-tile hashes. Pixels only; the owning
    /// CelLayer folds its own props (opacity/visibility) on top.
    pub fn content_hash(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::with_capacity(self.tiles.len() * 16);
        for ((tx, ty), tile) in &self.tiles {
            bytes.extend_from_slice(&tx.to_le_bytes());
            bytes.extend_from_slice(&ty.to_le_bytes());
            bytes.extend_from_slice(&tile.hash.to_le_bytes());
        }
        crate::value::fnv1a(&bytes)
    }
}
