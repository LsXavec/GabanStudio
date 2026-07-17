//! Document model: Project -> Scenes -> Cuts.
//!
//! The Cut is the atomic production unit (straight from the anime pipeline):
//! it owns a drawing library, an X-sheet, and a node graph. The X-sheet
//! references drawings by id; the graph references X-sheet columns by id.

use crate::graph::Graph;
use crate::ids::*;
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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Drawing {
    pub id: DrawingId,
    pub name: String,
    /// M2: vector strokes. M3+ adds raster layers alongside.
    pub strokes: Vec<Stroke>,
}

impl Drawing {
    /// Stable content hash — folded into the evaluator's recipe so editing
    /// artwork invalidates cached values naturally (and later drives
    /// render-cache dirtiness). Canonical little-endian byte fold, no serde.
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
        crate::value::fnv1a(&bytes)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Scene {
    pub id: SceneId,
    pub name: String,
    pub cuts: Vec<Cut>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub name: String,
    pub fps: u32,
    pub scenes: Vec<Scene>,
    /// Monotonic id counter; all entity ids come from here.
    pub next_id: u64,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fps: 24, // anime standard
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
