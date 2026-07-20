//! Document model: Project -> Scenes -> Cuts.
//!
//! The Cut is the atomic production unit (straight from the anime pipeline):
//! it owns a drawing library, an X-sheet, and a node graph. The X-sheet
//! references drawings by id; the graph references X-sheet columns by id.

use std::sync::Arc;

use crate::graph::Graph;
use crate::ids::*;
use crate::raster::{RasterLayer, TileCoord, TileData};
use crate::xsheet::XSheet;

/// One sampled pen point in paper coordinates (resolution independent).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    /// 0..1 tablet pressure (0.5 for mouse input).
    pub pressure: f32,
    /// Pen tilt from vertical in radians, [x, y] per octotablet's convention
    /// ([+,+] = right + toward the user). [0, 0] = vertical pen — and also
    /// mouse/legacy input, so old files load with a vertical pen (serde
    /// default). NOT folded into `content_hash`: vector rendering doesn't
    /// consume tilt yet, and the hash law folds only what reaches pixels —
    /// fold it the day a vector renderer draws tilt-shaped ribbons.
    #[serde(default)]
    pub tilt: [f32; 2],
}

/// A single pen stroke. Rendered width = base_width * point pressure.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stroke {
    pub points: Vec<StrokePoint>,
    pub base_width: f32,
    pub color: [u8; 4],
}

/// Per-layer compositing properties. Kept as ONE struct so `SetCelLayerProps`
/// replaces it wholesale — the exact inverse is trivially the prior struct.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerProps {
    /// Free text: "line", "color", "shadow", "correction", ...
    pub name: String,
    pub visible: bool,
    /// 0..=1, applied when compositing this layer into the cel.
    pub opacity: f32,
}

impl Default for LayerProps {
    fn default() -> Self {
        Self {
            name: "paint".into(),
            visible: true,
            opacity: 1.0,
        }
    }
}

/// One paint layer INSIDE a cel (the anime separation: douga line art, shiage
/// color under it, shadow between, sakkan correction above). Orthogonal to
/// X-sheet columns, which are layers ACROSS TIME. Id-addressed so commands
/// stay exact under any undo/reorder interleaving.
#[derive(Debug, Clone, PartialEq)]
pub struct CelLayer {
    pub id: LayerId,
    pub props: LayerProps,
    pub raster: RasterLayer,
}

impl CelLayer {
    pub fn new(id: LayerId, name: impl Into<String>) -> Self {
        Self {
            id,
            props: LayerProps {
                name: name.into(),
                ..LayerProps::default()
            },
            raster: RasterLayer::empty(),
        }
    }

    /// Folds the properties that affect the composited PIXELS — visibility,
    /// opacity, and the raster content. The NAME is deliberately excluded so
    /// renaming a layer never invalidates the eval cache or forces a GPU
    /// re-sync.
    pub fn content_hash(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::with_capacity(16);
        bytes.push(self.props.visible as u8);
        bytes.extend_from_slice(&self.props.opacity.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.raster.content_hash().to_le_bytes());
        crate::value::fnv1a(&bytes)
    }
}

/// A cel: vector strokes under an ordered stack of raster paint layers.
///
/// `layers[0]` is the BOTTOM of the stack. An empty stack = a vector-only cel
/// (the legacy pre-v5 case); the app keeps raster cels at >= 1 layer by
/// policy, but the ENGINE allows zero (matching the remove-column precedent).
///
/// Not `serde` — the store persists drawings field-by-field (strokes as JSON,
/// tiles as BLOBs), and layers hold large pixel buffers.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    pub id: DrawingId,
    pub name: String,
    pub strokes: Vec<Stroke>,
    pub layers: Vec<CelLayer>,
}

impl Drawing {
    pub fn new(id: DrawingId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            strokes: Vec::new(),
            layers: Vec::new(),
        }
    }

    pub fn layer(&self, id: LayerId) -> Option<&CelLayer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut CelLayer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    /// Stable content hash — folded into the evaluator's recipe so editing
    /// artwork (vector OR raster) invalidates cached values naturally.
    /// Canonical little-endian byte fold; the raster part folds per-layer
    /// hashes IN STACK ORDER (a reorder changes the composite; a rename does
    /// not — see [`CelLayer::content_hash`]), never raw pixels per eval.
    pub fn content_hash(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::with_capacity(16 + self.strokes.len() * 16);
        for stroke in &self.strokes {
            bytes.extend_from_slice(&stroke.base_width.to_bits().to_le_bytes());
            bytes.extend_from_slice(&stroke.color);
            for p in &stroke.points {
                bytes.extend_from_slice(&p.x.to_bits().to_le_bytes());
                bytes.extend_from_slice(&p.y.to_bits().to_le_bytes());
                bytes.extend_from_slice(&p.pressure.to_bits().to_le_bytes());
                // tilt deliberately excluded — see `StrokePoint::tilt`.
            }
        }
        if !self.layers.is_empty() {
            bytes.extend_from_slice(b"layers");
            for layer in &self.layers {
                bytes.extend_from_slice(&layer.content_hash().to_le_bytes());
            }
        }
        crate::value::fnv1a(&bytes)
    }

    /// CPU composite of the visible layers, bottom -> top, premultiplied-over
    /// with per-layer opacity. Tile values are f16 BIT PATTERNS (the GPU's
    /// Rgba16Float texels — see the encoding law on `TileData`), so the math
    /// decodes to f32, blends linearly, and re-encodes — the same arithmetic
    /// the GPU compositor performs. Plain bytes — the headless law holds.
    ///
    /// This is the GOLDEN REFERENCE the GPU compositor is tested against, and
    /// the future export/eval path. It is NOT the interactive display path.
    pub fn flatten(&self) -> std::collections::BTreeMap<TileCoord, Arc<TileData>> {
        use crate::raster::{TILE_LEN, f16_bits_to_f32, f32_to_f16_bits};

        // Collect every coordinate any visible layer touches.
        let mut coords: std::collections::BTreeSet<TileCoord> = std::collections::BTreeSet::new();
        for layer in &self.layers {
            if layer.props.visible && layer.props.opacity > 0.0 {
                coords.extend(layer.raster.tiles.keys().copied());
            }
        }

        let mut out = std::collections::BTreeMap::new();
        for coord in coords {
            // f32 working buffer, linear premultiplied.
            let mut acc = vec![0.0f32; TILE_LEN];
            for layer in &self.layers {
                if !layer.props.visible || layer.props.opacity <= 0.0 {
                    continue;
                }
                let Some(tile) = layer.raster.tiles.get(&coord) else {
                    continue;
                };
                let op = layer.props.opacity.clamp(0.0, 1.0);
                // src-over: out = src*op + out*(1 - src.a*op), premultiplied.
                for (px, spx) in acc.chunks_exact_mut(4).zip(tile.rgba.chunks_exact(4)) {
                    let sa = f16_bits_to_f32(spx[3]) * op;
                    let keep = 1.0 - sa;
                    for c in 0..4 {
                        let s = f16_bits_to_f32(spx[c]) * op;
                        px[c] = s + px[c] * keep;
                    }
                }
            }
            let rgba: Vec<u16> = acc
                .iter()
                .map(|v| f32_to_f16_bits(v.clamp(0.0, 1.0)))
                .collect();
            let tile = TileData::from_vec(rgba);
            if !tile.is_empty() {
                out.insert(coord, Arc::new(tile));
            }
        }
        out
    }
}

/// An image asset owned by a CUT (background plate, reference/storyboard
/// underlay, scanned pencil art — the satsuei "cels over BG" model keeps
/// BGs per cut). The ORIGINAL encoded bytes are the persisted truth (saved
/// verbatim, never re-encoded); premultiplied-f16 tiles are decoded once at
/// construction so every render path speaks the same currency as cel
/// layers. COLOR LAW: file channel values are kept AS-IS (premultiplied but
/// no EOTF applied) — texel space = file space, so importing a PNG and
/// exporting it round-trips bit-clean, consistent with export writing texel
/// values directly as sRGB bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageAsset {
    pub id: ImageId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Encoded source bytes (PNG), persisted verbatim.
    pub bytes: std::sync::Arc<Vec<u8>>,
    /// Decoded premultiplied-f16 tiles anchored at (0,0), derived
    /// deterministically from `bytes`.
    pub tiles: std::collections::BTreeMap<TileCoord, std::sync::Arc<TileData>>,
    /// fnv1a of the encoded bytes — the content address eval recipes fold.
    pub content_hash: u64,
}

/// Per-side dimension cap for imported images (BG plates, not gigapixel
/// scans). Enforced BEFORE any pixel allocation so a crafted header can't
/// drive an OOM, and kept small enough that all size arithmetic fits usize.
pub const MAX_IMAGE_DIM: u32 = 16_384;
/// Total pixel cap (~64 Mpx, an 8k×8k plate) — same rationale.
pub const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;

impl ImageAsset {
    /// Decode a PNG into an asset. EXPAND normalizes palette and sub-8-bit
    /// images to plain 8-bit channels (scanned 1-bit line art, indexed
    /// exports); 16-bit files are rejected as a plain error. Every failure
    /// is an `Err`, never a panic — commands reject atomically and loads
    /// report Corrupt.
    pub fn from_png(id: ImageId, name: impl Into<String>, bytes: Vec<u8>) -> Result<Self, String> {
        use crate::raster::{TILE, TILE_LEN, f32_to_f16_bits};

        let mut decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().map_err(|e| format!("png: {e}"))?;
        {
            // Caps from the HEADER, before any pixel-sized allocation — a
            // few-hundred-byte file declaring huge dimensions must fail
            // cleanly, not OOM.
            let info = reader.info();
            let (w, h) = (info.width, info.height);
            if w == 0 || h == 0 {
                return Err("png: empty image".into());
            }
            if w > MAX_IMAGE_DIM || h > MAX_IMAGE_DIM
                || (w as u64) * (h as u64) > MAX_IMAGE_PIXELS
            {
                return Err(format!(
                    "png: too large ({w}×{h}; max {MAX_IMAGE_DIM} per side, \
                     {MAX_IMAGE_PIXELS} pixels total)"
                ));
            }
        }
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(|e| format!("png: {e}"))?;
        // Depth check BEFORE any slicing: EXPAND leaves 16-bit as-is, and a
        // sub-8-bit buffer would be smaller than the slices below expect.
        if info.bit_depth != png::BitDepth::Eight {
            return Err(format!("png: unsupported bit depth {:?}", info.bit_depth));
        }
        let (w, h) = (info.width, info.height);
        let n = w as usize * h as usize; // fits: capped at 64 Mpx above
        let frame = &buf[..info.buffer_size()];
        let rgba: Vec<u8> = match info.color_type {
            png::ColorType::Rgba => frame[..n * 4].to_vec(),
            png::ColorType::Rgb => frame[..n * 3]
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::Grayscale => frame[..n]
                .iter()
                .flat_map(|&g| [g, g, g, 255])
                .collect(),
            png::ColorType::GrayscaleAlpha => frame[..n * 2]
                .chunks_exact(2)
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            // Indexed cannot appear after EXPAND; anything else is exotic.
            other => return Err(format!("png: unsupported color type {other:?}")),
        };

        // Tile it: straight u8 → premultiplied f16 (no EOTF — see color law).
        let mut tiles = std::collections::BTreeMap::new();
        let t1x = (w as i32 - 1).div_euclid(TILE as i32);
        let t1y = (h as i32 - 1).div_euclid(TILE as i32);
        for ty in 0..=t1y {
            for tx in 0..=t1x {
                let mut px = vec![0u16; TILE_LEN];
                let mut any = false;
                for row in 0..TILE {
                    let sy = ty as i64 * TILE as i64 + row as i64;
                    if sy >= h as i64 {
                        break;
                    }
                    for cx in 0..TILE {
                        let sx = tx as i64 * TILE as i64 + cx as i64;
                        if sx >= w as i64 {
                            break;
                        }
                        let s = &rgba[(sy as usize * w as usize + sx as usize) * 4..][..4];
                        let a = s[3] as f32 / 255.0;
                        if a <= 0.0 {
                            continue;
                        }
                        let i = (row * TILE + cx) * 4;
                        for c in 0..3 {
                            px[i + c] = f32_to_f16_bits(s[c] as f32 / 255.0 * a);
                        }
                        px[i + 3] = f32_to_f16_bits(a);
                        any = true;
                    }
                }
                if any {
                    tiles.insert(
                        (tx, ty),
                        std::sync::Arc::new(TileData::from_vec(px)),
                    );
                }
            }
        }
        let content_hash = crate::value::fnv1a(&bytes);
        Ok(Self {
            id,
            name: name.into(),
            width: w,
            height: h,
            bytes: std::sync::Arc::new(bytes),
            tiles,
            content_hash,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    pub id: CutId,
    pub name: String,
    pub frame_count: u32,
    pub drawings: Vec<Drawing>,
    pub xsheet: XSheet,
    pub graph: Graph,
    /// Image assets owned by this cut (BG plates, references).
    pub images: Vec<ImageAsset>,
}

impl Cut {
    pub fn drawing(&self, id: DrawingId) -> Option<&Drawing> {
        self.drawings.iter().find(|d| d.id == id)
    }

    pub fn image(&self, id: ImageId) -> Option<&ImageAsset> {
        self.images.iter().find(|i| i.id == id)
    }

    /// A parameter column's value at `frame` (None = column missing or has
    /// no keys — the caller falls back to its static value).
    pub fn param_value(&self, id: ParamId, frame: u32) -> Option<f32> {
        self.xsheet.param(id).and_then(|p| p.resolve(frame))
    }

    /// A Transform node's EFFECTIVE params at `frame`: each component takes
    /// its bound parameter column's value when bound and resolvable, else
    /// the static value. The single source of truth for all three render
    /// paths (evaluator hash, CPU export, GPU compositor) — they must agree
    /// or the composite view would lie about the export.
    pub fn transform_at(
        &self,
        translate: (f32, f32),
        scale: f32,
        rotate_deg: f32,
        binds: &crate::graph::TransformBinds,
        frame: u32,
    ) -> ((f32, f32), f32, f32) {
        let get = |bind: Option<ParamId>, fallback: f32| {
            bind.and_then(|id| self.param_value(id, frame)).unwrap_or(fallback)
        };
        (
            (get(binds.tx, translate.0), get(binds.ty, translate.1)),
            get(binds.scale, scale),
            get(binds.rotate, rotate_deg),
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub id: SceneId,
    pub name: String,
    pub cuts: Vec<Cut>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub name: String,
    pub fps: u32,
    /// Frame resolution in pixels — the "paper" every cut is drawn against.
    /// Project-wide (a film has one resolution), set at project creation.
    pub width: u32,
    pub height: u32,
    /// Pixels per inch, for print/export scaling. Purely metadata for drawing.
    pub dpi: f32,
    pub scenes: Vec<Scene>,
    /// Monotonic id counter; all entity ids come from here.
    pub next_id: u64,
    /// Opaque, app-owned project data (character color palettes, and
    /// anything similar later — e.g. per-project layout) that the ENGINE
    /// never interprets, only round-trips: string keys/values persisted
    /// verbatim under the `app.` prefix in the store's free-form `meta`
    /// table. No UI/app types leak into anim-core this way, and no schema
    /// version bump is needed to add a new key (the table already accepts
    /// arbitrary rows) — see store.rs's save/load for the exact convention.
    pub app_meta: std::collections::BTreeMap<String, String>,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fps: 24, // anime standard
            width: 1920,
            height: 1080,
            dpi: 300.0,
            scenes: Vec::new(),
            next_id: 1,
            app_meta: std::collections::BTreeMap::new(),
        }
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn scene(&self, id: SceneId) -> Option<&Scene> {
        self.scenes.iter().find(|s| s.id == id)
    }

    pub fn scene_mut(&mut self, id: SceneId) -> Option<&mut Scene> {
        self.scenes.iter_mut().find(|s| s.id == id)
    }

    pub fn cut(&self, scene: SceneId, cut: CutId) -> Option<&Cut> {
        self.scene(scene)?.cuts.iter().find(|c| c.id == cut)
    }

    pub fn cut_mut(&mut self, scene: SceneId, cut: CutId) -> Option<&mut Cut> {
        self.scene_mut(scene)?.cuts.iter_mut().find(|c| c.id == cut)
    }
}
