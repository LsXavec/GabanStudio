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
    /// Brush preset list: click to apply, save the current brush by name.
    Presets,
    /// The cut's compositing node graph (view + edit; engine-backed).
    NodeGraph,
    /// A read-only composite VIEWER: the node graph's rendered output with
    /// its own zoom/pan — the first multi-instance pane (LENS-DOCK Phase 1).
    /// Put one beside the canvas for the Finishing room: paint in the edit
    /// canvas, watch the graph result live next to it. The id keeps several
    /// viewers distinct in the dock tree (and keys their view state).
    Viewer(u8),
}

impl Pane {
    /// Every SINGLETON pane kind, for the "panes" add-menu (viewers are
    /// multi-instance and get their own add entry).
    pub const ALL: &'static [Pane] = &[
        Pane::Canvas,
        Pane::XSheet,
        Pane::Layers,
        Pane::Brush,
        Pane::Presets,
        Pane::NodeGraph,
    ];

    /// How many simultaneous viewers the add-menu offers.
    pub const MAX_VIEWERS: u8 = 4;

    pub fn title(&self) -> String {
        match self {
            Pane::Canvas => "Canvas".into(),
            Pane::XSheet => "X-Sheet".into(),
            Pane::Layers => "Cel Layers".into(),
            Pane::Brush => "Brush".into(),
            Pane::Presets => "Presets".into(),
            Pane::NodeGraph => "Node Graph".into(),
            Pane::Viewer(0) => "Viewer".into(),
            Pane::Viewer(n) => format!("Viewer {}", n + 1),
        }
    }
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

/// The tool/view state a workspace restores on entry — what makes a room a
/// ROOM (LENS-DOCK: workspace = layout + tool/mode + view). A Finishing
/// room re-arms the fill tool with composite view beside it; the Draw room
/// comes back holding the brush with onion on.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceView {
    pub tool: crate::canvas::CanvasTool,
    pub composite_view: bool,
    pub onion: bool,
    pub sel_shape: crate::canvas::SelShape,
    pub fill_ref_cel: bool,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Workspace {
    pub name: String,
    pub dock: DockState<Pane>,
    /// Brush preset (by name) applied automatically when this workspace is
    /// entered — each workflow stage keeps its own brush.
    #[serde(default)]
    pub preset: Option<String>,
    /// Tool/view state restored on entry (None = legacy workspace: layout
    /// and preset only).
    #[serde(default)]
    pub view: Option<WorkspaceView>,
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
    /// TOLERANT per-workspace parse: one unreadable workspace (e.g. saved by
    /// a newer build with pane kinds this build doesn't know) must not reset
    /// every workspace — the readable ones survive.
    pub fn load() -> Self {
        if let Some(path) = Self::path()
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(items) = v.get("list").and_then(|l| l.as_array())
        {
            let list: Vec<Workspace> = items
                .iter()
                .filter_map(|it| serde_json::from_value(it.clone()).ok())
                .collect();
            if !list.is_empty() {
                return Self { list };
            }
        }
        Self {
            list: vec![
                Workspace {
                    name: "draw".into(),
                    dock: draw_workspace(),
                    preset: None,
                    view: None,
                },
                Workspace {
                    name: "timing".into(),
                    dock: timing_workspace(),
                    preset: None,
                    view: None,
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
