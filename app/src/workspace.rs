//! Workspaces: named, persisted arrangements of dockable panes over the ONE
//! shared document (the production model — isolated rooms, shared parameters).
//! Saved to %APPDATA%/AnimStudio/workspaces.json.

use std::path::PathBuf;

use egui_dock::{DockState, NodeIndex};

/// One dockable UI element. The document underneath is shared; panes are
/// windows onto it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Pane {
    Canvas,
    XSheet,
    /// The cel-layers strip (was embedded at the bottom of the X-sheet).
    Layers,
    /// Brush & tool controls (was the canvas toolbar).
    Brush,
}

/// The "Draw" room: brush bar over a big canvas; X-sheet rail with the layer
/// strip under it.
pub fn draw_workspace() -> DockState<Pane> {
    let mut ds = DockState::new(vec![Pane::Canvas]);
    let tree = ds.main_surface_mut();
    // fraction = share of the top/left child of the split.
    let [canvas, rail] = tree.split_left(NodeIndex::root(), 0.24, vec![Pane::XSheet]);
    tree.split_below(rail, 0.62, vec![Pane::Layers]);
    tree.split_above(canvas, 0.11, vec![Pane::Brush]);
    ds
}

/// The "Timing" room: X-sheet takes half the screen.
pub fn timing_workspace() -> DockState<Pane> {
    let mut ds = DockState::new(vec![Pane::Canvas]);
    let tree = ds.main_surface_mut();
    let [canvas, rail] = tree.split_left(NodeIndex::root(), 0.5, vec![Pane::XSheet]);
    tree.split_below(rail, 0.7, vec![Pane::Layers]);
    tree.split_above(canvas, 0.11, vec![Pane::Brush]);
    ds
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Workspace {
    pub name: String,
    pub dock: DockState<Pane>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Workspaces {
    pub list: Vec<Workspace>,
}

impl Workspaces {
    fn path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(base).join("AnimStudio").join("workspaces.json"))
    }

    /// Load saved workspaces; a missing/unreadable file seeds the defaults.
    pub fn load() -> Self {
        if let Some(path) = Self::path()
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(ws) = serde_json::from_str::<Workspaces>(&text)
            && !ws.list.is_empty()
        {
            return ws;
        }
        Self {
            list: vec![
                Workspace {
                    name: "draw".into(),
                    dock: draw_workspace(),
                },
                Workspace {
                    name: "timing".into(),
                    dock: timing_workspace(),
                },
            ],
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}
