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

/// One 64×64 premultiplied-RGBA16 tile plus its content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileData {
    /// Row-major RGBA16, length [`TILE_LEN`], premultiplied alpha.
    pub rgba: Box<[u16]>,
    /// fnv1a over the little-endian bytes — the content-address of this tile.
    pub hash: u64,
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

    /// True if every pixel is fully transparent (alpha 0) — such tiles need
    /// not be stored (the layer's absence of a tile means "empty there").
    pub fn is_empty(&self) -> bool {
        // Premultiplied: transparent => all channels 0. Check alpha channel.
        self.rgba.chunks_exact(4).all(|px| px[3] == 0)
    }
}

/// One raster surface: a sparse grid of tiles. PURE pixels — opacity and other
/// compositing properties live on the owning [`crate::model::CelLayer`], so one
/// surface has exactly one home for each property.
/// A missing tile is fully transparent, so a blank layer costs nothing.
#[derive(Debug, Clone, PartialEq, Default)]
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
