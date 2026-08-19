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
    /// THE BRUSH FORGE (PSD-brush-forge) — its own tab, owner's word.
    Forge,
    /// Character color palettes (B5) — click a role swatch to load it as
    /// the brush colour; project-persisted, not undo-tracked.
    Palette,
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
        Pane::Forge,
        Pane::Palette,
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
            Pane::Forge => "Brush Forge".into(),
            Pane::Palette => "Palette".into(),
            Pane::NodeGraph => "Node Graph".into(),
            Pane::Viewer(0) => "Viewer".into(),
            Pane::Viewer(n) => format!("Viewer {}", n + 1),
        }
    }
}

/// The workflow trees (LENS-DOCK Phase 3): a FIXED ORDERED STAGE SPINE on
/// the merged solo pipeline — Layout → Drawing → Finishing → Edit. Stages
/// are compiled-in rooms (always available, can't be deleted or renamed);
/// the user customizes by rearranging and Save-as-ing into the ordinary
/// workspace list. The re-lensing invariant is absolute: switching stage
/// swaps layout + tool/view state only — it NEVER touches document data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// Story/layout: rough the shot against reference (imported BG plates /
    /// storyboards live in the node graph as ImageSource nodes).
    Layout,
    /// Genga + douga + sakkan merged: the daily drawing room.
    Drawing,
    /// Paint + composite merged: fill against the line column with the
    /// character palette at hand, graph result live alongside.
    Finishing,
    /// Timing + review: the X-sheet is the master artifact (camera columns
    /// will land HERE too, per the C1 decision), viewer + graph beside it.
    Edit,
}

impl Stage {
    /// The spine, in pipeline order.
    pub const ALL: &'static [Stage] =
        &[Stage::Layout, Stage::Drawing, Stage::Finishing, Stage::Edit];

    /// Button label: the anime-pipeline term (romaji) + English, so the
    /// rooms read instantly for both audiences. ("Layout" is already the
    /// industry loanword in Japanese — レイアウト — so it stands alone.)
    pub fn name(self) -> &'static str {
        match self {
            Stage::Layout => "layout",
            Stage::Drawing => "genga (drawing)",
            Stage::Finishing => "shiage (paint)",
            Stage::Edit => "henshū (edit)",
        }
    }

    /// Hover text: which real pipeline stages this room merges.
    pub fn describes(self) -> &'static str {
        match self {
            Stage::Layout => "ekonte + layout — storyboard and framing against reference",
            Stage::Drawing => "genga + dōga + sakkan — keys, inbetweens, supervision",
            Stage::Finishing => "shiage + satsuei — ink & paint with the composite live beside you",
            Stage::Edit => "henshū — timing and review; the sheet is the master",
        }
    }

    /// The stage's opinionated default layout.
    /// (fraction = share of the top/left child of the split.)
    pub fn dock(self) -> DockState<Pane> {
        match self {
            Stage::Layout => {
                // Big canvas; node graph tabbed behind the sheet (reference
                // images are graph nodes).
                let mut ds = DockState::new(vec![Pane::Canvas]);
                let tree = ds.main_surface_mut();
                let [canvas, rail] =
                    tree.split_left(NodeIndex::root(), 0.22, vec![Pane::XSheet, Pane::NodeGraph]);
                tree.split_below(rail, 0.6, vec![Pane::Layers]);
                tree.split_above(canvas, 0.11, vec![Pane::Brush]);
                ds
            }
            Stage::Drawing => {
                // The classic draw room; presets tabbed behind the layer
                // strip (each drawing pass keeps its own pencil).
                let mut ds = DockState::new(vec![Pane::Canvas]);
                let tree = ds.main_surface_mut();
                let [canvas, rail] = tree.split_left(NodeIndex::root(), 0.24, vec![Pane::XSheet]);
                tree.split_below(rail, 0.62, vec![Pane::Layers, Pane::Presets, Pane::Forge]);
                tree.split_above(canvas, 0.11, vec![Pane::Brush]);
                ds
            }
            Stage::Finishing => {
                // Edit canvas + live composite viewer SIDE BY SIDE (the A2
                // motivation), palette under the viewer.
                let mut ds = DockState::new(vec![Pane::Canvas]);
                let tree = ds.main_surface_mut();
                let [canvas, rail] = tree.split_left(NodeIndex::root(), 0.18, vec![Pane::XSheet]);
                tree.split_below(rail, 0.55, vec![Pane::Layers]);
                let [canvas, side] = tree.split_right(canvas, 0.58, vec![Pane::Viewer(0)]);
                tree.split_below(side, 0.55, vec![Pane::Palette]);
                tree.split_above(canvas, 0.11, vec![Pane::Brush]);
                ds
            }
            Stage::Edit => {
                // Sheet takes the left half; result viewer (canvas tabbed
                // behind it for quick fixes) over the node graph.
                let mut ds = DockState::new(vec![Pane::Viewer(0), Pane::Canvas]);
                let tree = ds.main_surface_mut();
                let [view, sheet] = tree.split_left(NodeIndex::root(), 0.45, vec![Pane::XSheet]);
                tree.split_below(sheet, 0.72, vec![Pane::Layers]);
                tree.split_below(view, 0.62, vec![Pane::NodeGraph]);
                ds
            }
        }
    }

    /// The tool/view state the stage re-arms on entry — the room's verb.
    pub fn view(self) -> WorkspaceView {
        use crate::canvas::{CanvasTool, SelShape};
        let base = WorkspaceView {
            tool: CanvasTool::Paint,
            composite_view: false,
            onion: false,
            onion_layer_only: false,
            sel_shape: SelShape::Rect,
            fill_ref_cel: false,
        };
        match self {
            Stage::Layout => base,
            Stage::Drawing => WorkspaceView {
                onion: true,
                ..base
            },
            Stage::Finishing => WorkspaceView {
                tool: CanvasTool::Fill,
                fill_ref_cel: true,
                ..base
            },
            Stage::Edit => WorkspaceView {
                tool: CanvasTool::Select,
                ..base
            },
        }
    }
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
    /// Added after `onion` — a workspace saved before this field existed
    /// deserializes it as `false` (whole-cel ghost, today's default).
    #[serde(default)]
    pub onion_layer_only: bool,
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
        Some(
            PathBuf::from(base)
                .join("AnimStudio")
                .join("workspaces.json"),
        )
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
        // No seeded defaults anymore: the stage spine provides the built-in
        // rooms; this list is purely the user's own saved arrangements.
        Self { list: Vec::new() }
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
