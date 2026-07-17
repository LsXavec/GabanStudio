//! Document model: Project -> Scenes -> Cuts.
//!
//! The Cut is the atomic production unit (straight from the anime pipeline):
//! it owns a drawing library, an X-sheet, and a node graph. The X-sheet
//! references drawings by id; the graph references X-sheet columns by id.

use crate::graph::Graph;
use crate::ids::*;
use crate::raster::RasterLayer;
use crate::xsheet::XSheet;

/// One sampled pen point in paper coordinates (resolution independent).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    /// 0..1 tablet pressure (0.5 for mouse input).
    pub pressure: f32,
}

/// A single pen stroke. Rendered width = base_width * point pressure.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stroke {
    pub points: Vec<StrokePoint>,
    pub base_width: f32,
    pub color: [u8; 4],
}

/// A cel: vector strokes and/or one raster layer, composited raster-over-vector.
///
/// Hybrid model (Krita-like): raster is the primary paint surface, vector stays
/// available for resolution-independent line work. A fuller multi-layer stack
/// (`Vec<Layer>`) can generalize this later; one optional raster layer covers
/// frame-by-frame raster animation now.
///
/// Not `serde` — the store persists drawings field-by-field (strokes as JSON,
/// tiles as BLOBs), and `RasterLayer` holds large pixel buffers.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing {
    pub id: DrawingId,
    pub name: String,
    pub strokes: Vec<Stroke>,
    pub raster: Option<RasterLayer>,
}

impl Drawing {
    pub fn new(id: DrawingId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            strokes: Vec::new(),
            raster: None,
        }
    }

    /// Stable content hash — folded into the evaluator's recipe so editing
    /// artwork (vector OR raster) invalidates cached values naturally.
    /// Canonical little-endian byte fold; the raster part folds per-tile
    /// hashes (see [`RasterLayer::content_hash`]), never raw pixels per eval.
    pub fn content_hash(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::with_capacity(16 + self.strokes.len() * 16);
        for stroke in &self.strokes {
            bytes.extend_from_slice(&stroke.base_width.to_bits().to_le_bytes());
            bytes.extend_from_slice(&stroke.color);
            for p in &stroke.points {
                bytes.extend_from_slice(&p.x.to_bits().to_le_bytes());
                bytes.extend_from_slice(&p.y.to_bits().to_le_bytes());
                bytes.extend_from_slice(&p.pressure.to_bits().to_le_bytes());
            }
        }
        if let Some(raster) = &self.raster {
            bytes.extend_from_slice(b"raster");
            bytes.extend_from_slice(&raster.content_hash().to_le_bytes());
        }
        crate::value::fnv1a(&bytes)
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
}

impl Cut {
    pub fn drawing(&self, id: DrawingId) -> Option<&Drawing> {
        self.drawings.iter().find(|d| d.id == id)
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
