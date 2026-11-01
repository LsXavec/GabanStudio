//! Workspaces: named, persisted arrangements of dockable panes over the ONE
//! shared document (the production model — isolated rooms, shared parameters).
//! Saved to workspaces.json in the data dir (%APPDATA%/AnimStudio).

use std::path::{Path, PathBuf};

use egui_dock::{DockState, NodeIndex};

use crate::vocab;

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
    /// GM3: every undo step, current one marked; click one to go there.
    History,
    /// GM14: the param columns' values over frames, as editable curves.
    Curves,
    /// GM7: holes and stray colours across a cut, found on a worker.
    PaintCheck,
    /// GM11: delivery jobs (presets, folders, patterns) rendered on a worker.
    RenderQueue,
    /// GM15: the scene's cuts as cards — status, assignee, notes.
    Board,
    /// GM15: the episode's cuts in order, TRT against the target.
    Episode,
    /// GM17: the cut's 3D layout through a free view camera (Blender's
    /// navigation); "Set cut camera from view" keys a Model3D's camera.
    View3D,
    /// GM18: the book's pages, the panel tool, tones and perspective rulers.
    Manga,
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
        Pane::History,
        Pane::Curves,
        Pane::PaintCheck,
        Pane::RenderQueue,
        Pane::Board,
        Pane::Episode,
        Pane::View3D,
        Pane::Manga,
    ];

    /// THE FORGE ROOM (PSD-brush-forge, owner amendment 2026-08-19:
    /// "It should really have its own workspace window"): a whole-window
    /// arrangement for brush design — the forge tall on the left with
    /// the quick list under it, and the canvas as the test paper.
    pub fn forge_dock() -> DockState<Pane> {
        let mut ds = DockState::new(vec![Pane::Canvas]);
        let tree = ds.main_surface_mut();
        let [_canvas, forge] = tree.split_left(NodeIndex::root(), 0.30, vec![Pane::Forge]);
        tree.split_below(forge, 0.68, vec![Pane::Presets, Pane::Palette]);
        ds
    }

    /// GM17: THE 3D LAYOUT ROOM (off the stage spine, like the Forge): the
    /// View3D pane beside a viewer of the composite, the node graph under
    /// them.
    pub fn layout3d_dock() -> DockState<Pane> {
        let mut ds = DockState::new(vec![Pane::View3D]);
        let tree = ds.main_surface_mut();
        let [view, _graph] = tree.split_below(NodeIndex::root(), 0.66, vec![Pane::NodeGraph]);
        tree.split_right(view, 0.6, vec![Pane::Viewer(0)]);
        ds
    }

    /// How many simultaneous viewers the add-menu offers.
    pub const MAX_VIEWERS: u8 = 4;
    pub fn title(&self) -> String {
        match self {
            Pane::Canvas => "Canvas".into(),
            Pane::XSheet => vocab::TIMING_SHEET.into(),
            Pane::Layers => vocab::LAYERS.into(),
            Pane::Brush => "Brush".into(),
            Pane::Presets => vocab::BRUSHES.into(),
            Pane::Forge => vocab::BRUSH_MAKER.into(),
            Pane::Palette => "Palette".into(),
            Pane::NodeGraph => vocab::EFFECTS_GRAPH.into(),
            Pane::History => "History".into(),
            Pane::Curves => vocab::MOTION_CURVES.into(),
            Pane::PaintCheck => "Nuri check (paint check)".into(),
            Pane::RenderQueue => vocab::EXPORT_QUEUE.into(),
            // GM15: the studio's words first (D7), as the Window menu says
            // them.
            Pane::Board => "Shinkō board (cut board)".into(),
            Pane::Episode => "Henshū timeline (episode)".into(),
            Pane::View3D => "View3D — 3D LO (layout)".into(),
            Pane::Manga => "Manga (pages)".into(),
            Pane::Viewer(0) => vocab::RESULT.into(),
            Pane::Viewer(n) => format!("{} {}", vocab::RESULT, n + 1),
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
    /// Ink & paint (GM8 D12: was "Finishing"; its room key stays
    /// "finishing" so saved stack bindings keep working): fill against the
    /// line column with the character palette at hand, result alongside.
    InkPaint,
    /// Satsuei (GM8 D12): the node graph large, viewer and palette beside
    /// it, curves tabbed with the graph; composite view on.
    Composite,
    /// Timing + review: the X-sheet is the master artifact (camera columns
    /// will land HERE too, per the C1 decision), viewer + graph beside it.
    Edit,
    /// GM18 (D12): the manga room, OFF the spine (never in [`Stage::ALL`]):
    /// the page large, the Manga pane and the layers beside it.
    Manga,
    /// AU9: the Export step, also off [`Stage::ALL`], with no stored key: the
    /// result beside the Export panel. It swaps the dock and nothing else, so
    /// it arms no tool, layer or view (`Editor::enter_room`).
    Export,
}

impl Stage {
    /// The spine, in pipeline order.
    pub const ALL: &'static [Stage] = &[
        Stage::Layout,
        Stage::Drawing,
        Stage::InkPaint,
        Stage::Composite,
        Stage::Edit,
    ];

    /// The step's word on screen. The studio's own word stays in the hover
    /// card (`steps::CATALOG`) and in the aliases.
    pub fn name(self) -> &'static str {
        match self {
            Stage::Layout => vocab::STEP_PLAN,
            Stage::Drawing => vocab::STEP_DRAW,
            Stage::InkPaint => vocab::STEP_COLOR,
            Stage::Composite => vocab::STEP_EFFECTS,
            Stage::Edit => vocab::STEP_TIMING,
            Stage::Manga => vocab::STEP_MANGA,
            Stage::Export => vocab::STEP_EXPORT,
        }
    }

    /// Hover text: what the step is for.
    pub fn describes(self) -> &'static str {
        self.card().1
    }

    /// GM8 step card: (studio word, one-line purpose, the tools it arms).
    /// AU18: every stage reads its words from `steps::CATALOG`.
    pub fn card(self) -> (&'static str, &'static str, &'static str) {
        let arms = match self {
            Stage::Layout => "brush",
            Stage::Drawing => "brush · onion skin",
            Stage::InkPaint => "fill · line guard",
            Stage::Composite => "composite view",
            Stage::Edit => "select",
            Stage::Manga => "brush · panels · rulers",
            Stage::Export => "nothing",
        };
        let (word, purpose) = crate::steps::of(self).map_or(("", ""), |step| (step.studio_words, step.purpose));
        (word, purpose, arms)
    }

    /// AU1: the classic draw room, with the Timing sheet; presets tabbed
    /// behind the layer strip (each drawing pass keeps its own pencil). AU19:
    /// the frozen boot's alone: `Editor::recipe` picks it there when
    /// `sheet_in_draw` is on.
    pub fn dock_with_sheet() -> DockState<Pane> {
        let mut ds = DockState::new(vec![Pane::Canvas]);
        let tree = ds.main_surface_mut();
        let [_canvas, rail] = tree.split_left(NodeIndex::root(), 0.24, vec![Pane::XSheet]);
        tree.split_below(rail, 0.62, vec![Pane::Layers, Pane::Presets, Pane::Forge]);
        ds
    }

    /// The stage's opinionated default layout.
    /// (fraction = share of the top/left child of the split.)
    pub fn dock(self) -> DockState<Pane> {
        match self {
            // AU19: Plan's one source, read by `Editor::recipe`: the sheet whole
            // at the left and a live canvas. No step stands on `Stage::Layout`.
            Stage::Layout => {
                let mut ds = DockState::new(vec![Pane::Canvas]);
                ds.main_surface_mut().split_left(NodeIndex::root(), 0.22, vec![Pane::XSheet]);
                ds
            }
            // AU3: the paper alone.
            Stage::Drawing => DockState::new(vec![Pane::Canvas]),
            Stage::InkPaint => {
                // Edit canvas + live composite viewer SIDE BY SIDE (the A2
                // motivation), palette under the viewer — GM7: the nuri
                // check tabbed behind it, the room's last pass.
                let mut ds = DockState::new(vec![Pane::Canvas]);
                let tree = ds.main_surface_mut();
                let [canvas, rail] = tree.split_left(NodeIndex::root(), 0.18, vec![Pane::XSheet]);
                tree.split_below(rail, 0.55, vec![Pane::Layers]);
                let [_canvas, side] = tree.split_right(canvas, 0.58, vec![Pane::Viewer(0)]);
                tree.split_below(side, 0.55, vec![Pane::Palette, Pane::PaintCheck]);
                ds
            }
            Stage::Composite => {
                // The graph large with curves tabbed beside it; the result
                // viewer (canvas tabbed behind it) and the palette at right,
                // the render queue (GM11) tabbed behind it.
                let mut ds = DockState::new(vec![Pane::NodeGraph, Pane::Curves]);
                let tree = ds.main_surface_mut();
                let [_, side] =
                    tree.split_right(NodeIndex::root(), 0.58, vec![Pane::Viewer(0), Pane::Canvas]);
                tree.split_below(side, 0.62, vec![Pane::Palette, Pane::RenderQueue]);
                ds
            }
            Stage::Edit => {
                // Sheet takes the left half; result viewer (canvas tabbed
                // behind it for quick fixes) over the node graph, the render
                // queue (GM11), the cut board and the episode timeline
                // (GM15) tabbed behind it.
                let mut ds = DockState::new(vec![Pane::Viewer(0), Pane::Canvas]);
                let tree = ds.main_surface_mut();
                let [view, sheet] = tree.split_left(NodeIndex::root(), 0.45, vec![Pane::XSheet]);
                tree.split_below(sheet, 0.72, vec![Pane::Layers]);
                tree.split_below(
                    view,
                    0.62,
                    vec![Pane::NodeGraph, Pane::RenderQueue, Pane::Board, Pane::Episode],
                );
                ds
            }
            Stage::Manga => {
                // The page large; the Manga pane (pages, panels, tones,
                // rulers) on the left over the layers.
                let mut ds = DockState::new(vec![Pane::Canvas]);
                let tree = ds.main_surface_mut();
                let [_canvas, rail] = tree.split_left(NodeIndex::root(), 0.26, vec![Pane::Manga]);
                tree.split_below(rail, 0.7, vec![Pane::Layers, Pane::NodeGraph]);
                ds
            }
            Stage::Export => {
                // The picture being exported, and the choices beside it.
                let mut ds = DockState::new(vec![Pane::Viewer(0)]);
                ds.main_surface_mut().split_right(NodeIndex::root(), 0.72, vec![Pane::RenderQueue]);
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
            Stage::InkPaint => WorkspaceView {
                tool: CanvasTool::Fill,
                fill_ref_cel: true,
                ..base
            },
            Stage::Composite => WorkspaceView {
                composite_view: true,
                ..base
            },
            Stage::Edit => WorkspaceView {
                tool: CanvasTool::Select,
                ..base
            },
            // Export never applies a view; the arm is here for the match.
            Stage::Manga | Stage::Export => base,
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

/// AU3: a rect egui_dock has not laid out yet is infinite, serde_json writes
/// infinity as `null`, and a `null` cannot be read back: inside any object
/// holding `min` and `max`, a null `x` or `y` becomes 0.0. Nothing else
/// changes; the dock measures every rect afresh on its next frame.
fn finite_rects(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            if map.contains_key("min") && map.contains_key("max") {
                for corner in ["min", "max"] {
                    for axis in ["x", "y"] {
                        let at = map.get_mut(corner).and_then(|c| c.get_mut(axis));
                        if let Some(n) = at.filter(|n| n.is_null()) {
                            *n = serde_json::Value::from(0.0);
                        }
                    }
                }
            }
            map.values_mut().for_each(finite_rects);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(finite_rects),
        _ => {}
    }
}

impl Workspace {
    /// AU3: this workspace as every save path writes it. A dock never drawn
    /// holds infinite rects (see [`finite_rects`]); egui_dock has no public
    /// write to a node's rect, so the repair goes through JSON.
    pub fn normalised(&self) -> serde_json::Result<Workspace> {
        let mut v = serde_json::to_value(self)?;
        finite_rects(&mut v);
        serde_json::from_value(v)
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Workspaces {
    pub list: Vec<Workspace>,
}

impl Workspaces {
    /// `workspaces.json` inside a data dir (D11).
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join("workspaces.json")
    }

    /// Load saved workspaces from `dir`; a missing file is an empty list.
    /// TOLERANT per-workspace parse: one unreadable workspace (e.g. saved by
    /// a newer build with pane kinds this build doesn't know) must not reset
    /// every workspace — the readable ones survive, the original bytes are
    /// kept as `workspaces.json.bad` and one note says how many were
    /// dropped. A leftover `workspaces.json.tmp` is never read. AU3: a file
    /// from before [`Workspace::normalised`] has its null rects repaired
    /// first, so a workspace saved undrawn loads like any other.
    pub fn load_from(dir: &Path) -> (Self, Vec<String>) {
        let path = Self::path_in(dir);
        // No seeded defaults anymore: the stage spine provides the built-in
        // rooms; this list is purely the user's own saved arrangements.
        let mut list = Vec::new();
        let mut notes = Vec::new();
        if let Ok(bytes) = std::fs::read(&path) {
            // None = the file itself is unreadable (not JSON, no list).
            let mut dropped = Some(0usize);
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(v) => match v.get("list").and_then(|l| l.as_array()) {
                    Some(items) => {
                        for it in items {
                            let mut it = it.clone();
                            finite_rects(&mut it);
                            match serde_json::from_value::<Workspace>(it) {
                                Ok(w) => list.push(w),
                                Err(_) => dropped = dropped.map(|n| n + 1),
                            }
                        }
                    }
                    None => dropped = None,
                },
                Err(_) => dropped = None,
            }
            let said = match dropped {
                Some(0) => None,
                Some(1) => Some("1 saved workspace could not be read and was skipped".to_string()),
                Some(n) => Some(format!("{n} saved workspaces could not be read and were skipped")),
                None => Some("your saved workspaces could not be read".to_string()),
            };
            if let Some(said) = said {
                let mut bad = path.as_os_str().to_owned();
                bad.push(".bad");
                let _ = std::fs::write(PathBuf::from(bad), &bytes);
                notes.push(format!("{said} — the original is kept as workspaces.json.bad"));
            }
        }
        (Self { list }, notes)
    }

    /// Save to `dir/workspaces.json` atomically (`.tmp` + rename), each
    /// workspace [`Workspace::normalised`].
    pub fn save_to(&self, dir: &Path) -> std::io::Result<()> {
        let list = self.list.iter().map(Workspace::normalised).collect::<Result<_, _>>();
        let saved = Workspaces { list: list.map_err(std::io::Error::other)? };
        let text = serde_json::to_string_pretty(&saved).map_err(std::io::Error::other)?;
        crate::config::write_atomic(&Self::path_in(dir), text.as_bytes())
    }
}
