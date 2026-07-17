//! Document model: Project -> Scenes -> Cuts.
//!
//! The Cut is the atomic production unit (straight from the anime pipeline):
//! it owns a drawing library, an X-sheet, and a node graph. The X-sheet
//! references drawings by id; the graph references X-sheet columns by id.

use crate::graph::Graph;
use crate::ids::*;
use crate::xsheet::XSheet;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Drawing {
    pub id: DrawingId,
    pub name: String,
    // M2: raster/vector layers live here (content-addressed blobs on disk).
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
