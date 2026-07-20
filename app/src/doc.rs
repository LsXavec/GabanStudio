//! App-side document state: the Engine plus the user's current position in it
//! (active cut, column, frame, selection) and transport/playback state.
//! All document mutations route through Engine commands — undo covers
//! everything the artist does here.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::ids::*;
use anim_core::model::{Cut, Stroke};
use anim_core::model::{CelLayer, LayerProps};
use anim_core::raster::{TileCoord, TileData, TileDiff};
use anim_core::xsheet::Exposure;

/// Bottom→top display slices of cel layers: (tiles, effective opacity).
pub type LayerSlices<'a> = Vec<(&'a BTreeMap<TileCoord, Arc<TileData>>, f32)>;

/// The user's CURSOR into the document: which scene/cut/frame/column/layer
/// is being looked at, and how (onion, playback). This is LENS-DOCK Phase
/// 2's ViewContext, split from the document so panels can eventually hold or
/// share one per link-channel. TODAY there is exactly ONE (the "Main"
/// context, owned by AppState) — behavior is byte-identical to the fused
/// struct; only the field paths moved.
pub struct ViewContext {
    pub scene: SceneId,
    pub cut: CutId,
    pub active_column: ColumnId,
    pub frame: u32,
    pub selected_drawing: Option<DrawingId>,
    pub playing: bool,
    pub(crate) play_acc: f32,
    pub onion: bool,
    /// Onion ghosts show ONLY the layer matching the active layer's NAME
    /// (classic douga "line-only ghost" — inking against the neighbour's
    /// line without its color/shadow bleeding through). Off = whole-cel
    /// ghost (today's default behavior).
    pub onion_layer_only: bool,
    /// Which cel layer is being edited, counted FROM THE TOP of the stack
    /// (0 = topmost — "line" in the default template). Position-based so it
    /// follows across frames; clamped per drawing when stacks are shallower.
    pub active_layer_slot: usize,
    /// Loop playback at the end of the cut (off = stop on the last frame).
    pub loop_playback: bool,
}

/// The document (engine + where it lives on disk) plus the single "Main"
/// view context and app-transient UI furniture. Mutations still route
/// exclusively through `engine.apply` — the split changes no behavior.
pub struct AppState {
    pub engine: Engine,
    pub view: ViewContext,
    pub file_path: Option<PathBuf>,
    pub status: String,
    /// Continuation handles (column, key-frame) the user has explicitly dragged
    /// into position. Such a hold stays statically visible even when it holds to
    /// its natural end (no Empty terminator); un-touched holds stay hidden.
    pub positioned_holds: HashSet<(ColumnId, u32)>,
    /// Character color palettes (B5) — project-persisted, not undo-tracked
    /// (see palette.rs). Loaded on New/Open, written into the project's
    /// opaque meta right before every file save.
    pub palettes: crate::palette::Palettes,
    /// Transient UI state: the Palette pane's "new character" name field.
    pub palette_new_name: String,
    /// Transient layer-strip UI state: an in-progress rename (layer, buffer).
    pub strip_rename: Option<(LayerId, String)>,
    /// Transient drawing-library UI state: an in-progress rename.
    pub drawing_rename: Option<(DrawingId, String)>,
    /// Transient layer-strip UI state: an in-progress opacity drag
    /// (layer, live value) — committed as ONE SetCelLayerProps at gesture end.
    pub strip_opacity: Option<(LayerId, f32)>,
    /// Node-graph pane: world position per node (session-only for now; the
    /// engine graph itself is persisted and undoable, positions are view
    /// furniture).
    pub node_positions: HashMap<u64, (f32, f32)>,
    /// Node-graph pane: an in-progress wire drag from a node's output pin.
    pub pending_wire: Option<NodeId>,
    /// Node-graph pane: selected node (its params edit in the inspector row).
    pub selected_node: Option<NodeId>,
    /// Node-graph pane: view pan.
    pub graph_pan: (f32, f32),
    /// X-sheet pane: the selected PARAMETER column (its keying strip shows
    /// under the toolbar) — session-only view state.
    pub param_sel: Option<anim_core::ids::ParamId>,
    /// X-sheet keying strip: live value buffer + which (column, frame) it
    /// was seeded for (reseeded whenever either changes).
    pub param_buf: f32,
    pub param_buf_at: Option<(anim_core::ids::ParamId, u32)>,
    /// X-sheet keying strip: sticky interpolation choice for new keys.
    pub param_interp: anim_core::xsheet::ParamInterp,
}

impl AppState {
    /// Create a project with a chosen resolution (from the New Project dialog).
    pub fn new_project(
        name: impl Into<String>,
        width: u32,
        height: u32,
        fps: u32,
        dpi: f32,
    ) -> Self {
        let mut engine = Engine::new(name);
        engine.project.width = width.max(1);
        engine.project.height = height.max(1);
        engine.project.fps = fps.clamp(1, 240);
        engine.project.dpi = dpi.max(1.0);
        let scene = engine.add_scene("SC01");
        let cut = engine.add_cut(scene, "CUT01", 48).expect("fresh scene");
        let at = CutRef { scene, cut };
        let active_column = engine.add_column(at, "A").expect("fresh cut");
        engine.clear_history();
        Self {
            engine,
            view: ViewContext {
                scene,
                cut,
                active_column,
                frame: 0,
                selected_drawing: None,
                playing: false,
                play_acc: 0.0,
                onion: true,
                onion_layer_only: false,
                active_layer_slot: 0,
                loop_playback: true,
            },
            file_path: None,
            status: "new project — draw on the canvas to create frame 1".into(),
            positioned_holds: HashSet::new(),
            palettes: Default::default(),
            palette_new_name: String::new(),
            strip_rename: None,
            drawing_rename: None,
            strip_opacity: None,
            node_positions: HashMap::new(),
            pending_wire: None,
            selected_node: None,
            graph_pan: (0.0, 0.0),
            param_sel: None,
            param_buf: 0.0,
            param_buf_at: None,
            param_interp: anim_core::xsheet::ParamInterp::Ease,
        }
    }

    /// Load a project file into a ready AppState (used by the startup dialog).
    pub fn load_from(path: std::path::PathBuf) -> std::result::Result<Self, String> {
        let engine = Engine::load(&path).map_err(|e| e.to_string())?;
        Self::adopt(engine, path)
    }

    /// Show a file picker and load the chosen project. None = user cancelled.
    pub fn pick_and_open() -> Option<std::result::Result<Self, String>> {
        let path = rfd::FileDialog::new()
            .add_filter("AnimStudio project", &["animproj"])
            .pick_file()?;
        Some(Self::load_from(path))
    }

    /// Adopt a loaded engine, pointing the UI at its first scene/cut/column.
    fn adopt(engine: Engine, path: PathBuf) -> Result<Self, String> {
        let scene = engine.project.scenes.first().ok_or("project has no scenes")?;
        let cut = scene.cuts.first().ok_or("project has no cuts")?;
        let column = cut
            .xsheet
            .columns
            .first()
            .ok_or("cut has no X-sheet columns")?;
        Ok(Self {
            view: ViewContext {
                scene: scene.id,
                cut: cut.id,
                active_column: column.id,
                frame: 0,
                selected_drawing: None,
                playing: false,
                play_acc: 0.0,
                onion: true,
                onion_layer_only: false,
                active_layer_slot: 0,
                loop_playback: true,
            },
            file_path: Some(path),
            status: "project loaded".into(),
            palettes: crate::palette::Palettes::load_from(&engine.project),
            palette_new_name: String::new(),
            engine,
            positioned_holds: HashSet::new(),
            strip_rename: None,
            drawing_rename: None,
            strip_opacity: None,
            node_positions: HashMap::new(),
            pending_wire: None,
            selected_node: None,
            graph_pan: (0.0, 0.0),
            param_sel: None,
            param_buf: 0.0,
            param_buf_at: None,
            param_interp: anim_core::xsheet::ParamInterp::Ease,
        })
    }

    // ---- Accessors --------------------------------------------------------

    pub fn at(&self) -> CutRef {
        CutRef {
            scene: self.view.scene,
            cut: self.view.cut,
        }
    }

    pub fn cut(&self) -> &Cut {
        self.engine
            .project
            .cut(self.view.scene, self.view.cut)
            .expect("current cut exists")
    }

    pub fn frame_count(&self) -> u32 {
        self.cut().frame_count.max(1)
    }

    pub fn fps(&self) -> u32 {
        self.engine.project.fps.max(1)
    }

    pub fn resolve_at(&self, column: ColumnId, frame: u32) -> Option<DrawingId> {
        self.cut()
            .xsheet
            .column(column)
            .and_then(|c| c.resolve(frame))
    }

    /// The drawing under the pen right now (active column @ current frame),
    /// following holds — this is what's *displayed*.
    pub fn current_drawing(&self) -> Option<DrawingId> {
        self.resolve_at(self.view.active_column, self.view.frame)
    }

    /// The drawing keyed at *exactly* this frame (not a hold from an earlier
    /// frame). Editing targets this: drawing on a held/empty frame makes a NEW
    /// cel here rather than editing the held drawing — standard frame-by-frame.
    pub fn own_key_drawing(&self) -> Option<DrawingId> {
        match self
            .cut()
            .xsheet
            .column(self.view.active_column)?
            .key_at(self.view.frame)
        {
            Some(Exposure::Drawing(d)) => Some(d),
            _ => None,
        }
    }

    /// Per-column drawing name, e.g. D1A, D2A on column A; D1B on column B.
    fn next_drawing_name(&self) -> String {
        match self.cut().xsheet.column(self.view.active_column) {
            Some(col) => {
                let distinct: std::collections::HashSet<DrawingId> = col
                    .keys()
                    .filter_map(|(_, e)| match e {
                        Exposure::Drawing(d) => Some(d),
                        Exposure::Empty => None,
                    })
                    .collect();
                format!("D{}{}", distinct.len() + 1, col.name)
            }
            None => format!("D{}", self.cut().drawings.len() + 1),
        }
    }

    // ---- Editing ----------------------------------------------------------

    /// Commit a finished pen stroke. If the current cell is empty, a new
    /// drawing is created and exposed here first — create + expose + stroke
    /// is ONE undo step, so Ctrl+Z after drawing on an empty frame leaves
    /// no orphan drawing behind.
    pub fn commit_stroke(&mut self, stroke: Stroke) {
        let at = self.at();
        match self.own_key_drawing() {
            Some(id) => {
                let r = self
                    .engine
                    .apply("draw", vec![Command::AddStroke { at, id, stroke }]);
                self.report(r, "stroke");
            }
            None => {
                let name = self.next_drawing_name();
                let id = self.engine.alloc_drawing_id();
                let layers = self.new_cel_layers();
                let index = self.cut().drawings.len(); // append to the library
                let r = self.engine.apply(
                    "draw (new drawing)",
                    vec![
                        Command::AddDrawing {
                            at,
                            id,
                            index,
                            name,
                            strokes: vec![],
                            layers,
                        },
                        Command::SetCell {
                            at,
                            column: self.view.active_column,
                            frame: self.view.frame,
                            key: Some(Exposure::Drawing(id)),
                        },
                        Command::AddStroke { at, id, stroke },
                    ],
                );
                if r.is_ok() {
                    self.view.selected_drawing = Some(id);
                }
                self.report(r, "new drawing");
            }
        }
    }

    /// Commit a select/move/scale/rotate gesture: the edit-math diff becomes
    /// ONE `PaintTiles` command against the target LATCHED at lift, so a
    /// mid-gesture frame/layer change can never retarget it. One gesture =
    /// one undo step, by construction (the diff is its own exact inverse).
    pub fn commit_region_edit(
        &mut self,
        label: &str,
        id: DrawingId,
        layer: LayerId,
        diff: anim_core::raster::TileDiff,
    ) {
        let at = self.at();
        let n = diff.len();
        let r = self
            .engine
            .apply(label, vec![Command::PaintTiles { at, id, layer, diff }]);
        match r {
            Ok(_) => self.status = format!("{label} applied ({n} tile(s))"),
            Err(e) => self.status = format!("{label} failed: {e:?}"),
        }
    }

    /// Flatten of the DISPLAYED drawing's visible layers — the fill tool's
    /// "cel" reference: line art bounds a fill no matter which layer it
    /// lands on (the shiage workflow).
    pub fn display_cel_flatten(
        &self,
    ) -> Option<std::collections::BTreeMap<TileCoord, Arc<TileData>>> {
        let d = self.cut().drawing(self.display_drawing()?)?;
        Some(d.flatten())
    }

    /// Commit a finished raster stroke: the readback tiles become a `PaintTiles`
    /// edit targeting the layer at `slot` — the slot is LATCHED by the canvas at
    /// stroke start, so a mid-stroke A-cycle can never retarget the commit. If
    /// the current cell is empty, a new [color, line] cel is created and exposed
    /// here first; if the cel is a legacy vector-only drawing, the default stack
    /// is added — each as ONE undo step. Returns (drawing, layer) on a
    /// successful commit; None means nothing landed (canvas must re-sync).
    pub fn commit_raster(
        &mut self,
        new_tiles: Vec<(TileCoord, Arc<TileData>)>,
        slot: usize,
    ) -> Option<(DrawingId, LayerId)> {
        let at = self.at();
        match self.own_key_drawing() {
            None => {
                // Nothing painted (e.g. an eraser stroke on a held/blank frame):
                // don't create an empty cel that would blank the held display.
                if new_tiles.is_empty() {
                    return None;
                }
                let name = self.next_drawing_name();
                let id = self.engine.alloc_drawing_id();
                let layers = self.new_cel_layers();
                // Paint lands on the latched slot of the fresh template.
                let s = slot.min(layers.len() - 1);
                let layer = layers[layers.len() - 1 - s].id;
                let index = self.cut().drawings.len(); // append to the library
                let diff: TileDiff = new_tiles
                    .into_iter()
                    .map(|(c, t)| (c, None, Some(t)))
                    .collect();
                let r = self.engine.apply(
                    "paint (new cel)",
                    vec![
                        Command::AddDrawing {
                            at,
                            id,
                            index,
                            name,
                            strokes: vec![],
                            layers,
                        },
                        Command::SetCell {
                            at,
                            column: self.view.active_column,
                            frame: self.view.frame,
                            key: Some(Exposure::Drawing(id)),
                        },
                        Command::PaintTiles { at, id, layer, diff },
                    ],
                );
                let ok = r.is_ok();
                if ok {
                    self.view.selected_drawing = Some(id);
                }
                self.report(r, "painted (new cel)");
                ok.then_some((id, layer))
            }
            Some(id) => {
                let d = self.cut().drawing(id)?;
                if d.layers.is_empty() {
                    if new_tiles.is_empty() {
                        return None;
                    }
                    // Legacy vector-only cel: give it the default stack and
                    // paint onto the latched slot — one undo step, no dead end.
                    let layers = self.new_cel_layers();
                    let s = slot.min(layers.len() - 1);
                    let layer = layers[layers.len() - 1 - s].id;
                    let diff: TileDiff = new_tiles
                        .into_iter()
                        .map(|(c, t)| (c, None, Some(t)))
                        .collect();
                    let mut cmds: Vec<Command> = layers
                        .into_iter()
                        .enumerate()
                        .map(|(i, l)| Command::AddCelLayer {
                            at,
                            drawing: id,
                            index: i,
                            layer: l,
                        })
                        .collect();
                    cmds.push(Command::PaintTiles { at, id, layer, diff });
                    let r = self.engine.apply("paint (add layers)", cmds);
                    let ok = r.is_ok();
                    self.report(r, "painted (layers added)");
                    return ok.then_some((id, layer));
                }
                // Snapshot the LATCHED layer's tiles as the diff "before"
                // (never None here: the stack is non-empty).
                let target = Self::layer_at_slot(d, slot)?;
                let (layer, before): (_, BTreeMap<TileCoord, Arc<TileData>>) =
                    (target.id, target.raster.tiles.clone());
                let after: BTreeMap<TileCoord, Arc<TileData>> = new_tiles.into_iter().collect();

                let mut diff: TileDiff = Vec::new();
                for (coord, a) in &after {
                    if before.get(coord).map(|t| t.hash) != Some(a.hash) {
                        diff.push((*coord, before.get(coord).cloned(), Some(a.clone())));
                    }
                }
                for (coord, b) in &before {
                    if !after.contains_key(coord) {
                        diff.push((*coord, Some(b.clone()), None));
                    }
                }
                if diff.is_empty() {
                    return Some((id, layer));
                }
                let r = self
                    .engine
                    .apply("paint", vec![Command::PaintTiles { at, id, layer, diff }]);
                let ok = r.is_ok();
                self.report(r, "painted");
                ok.then_some((id, layer))
            }
        }
    }

    /// Layer stack for a NEW cel. Clones the STRUCTURE (names, visibility,
    /// opacity — never pixels) of the drawing currently HELD on the active
    /// column at the current frame (CSP's "new cel inherits the previous
    /// cel's stack" — pro stacks are per-scene consistent, so this saves
    /// rebuilding shadow/highlight/correction layers by hand on every new
    /// drawing). Falls back to the merged solo douga/shiage default —
    /// "color" bottom, "line" top — when there's nothing to clone: the
    /// column start, right after an Empty terminator, or a legacy
    /// vector-only held drawing (no layers).
    fn new_cel_layers(&mut self) -> Vec<CelLayer> {
        // Collect owned props first — self.cut() borrows self.engine.project
        // immutably, which would collide with alloc_layer_id()'s &mut below.
        let prev_props: Vec<LayerProps> = self
            .resolve_at(self.view.active_column, self.view.frame)
            .and_then(|id| self.cut().drawing(id))
            .map(|d| d.layers.iter().map(|l| l.props.clone()).collect())
            .unwrap_or_default();
        if !prev_props.is_empty() {
            return prev_props
                .into_iter()
                .map(|props| CelLayer {
                    id: self.engine.alloc_layer_id(),
                    props,
                    raster: Default::default(),
                })
                .collect();
        }
        vec![
            CelLayer::new(self.engine.alloc_layer_id(), "color"), // bottom
            CelLayer::new(self.engine.alloc_layer_id(), "line"),  // top
        ]
    }

    /// Resolve a layer SLOT (counted from the top, clamped) to a concrete
    /// layer of `d`. None only for layerless (vector-only) drawings.
    fn layer_at_slot(d: &anim_core::model::Drawing, slot: usize) -> Option<&CelLayer> {
        if d.layers.is_empty() {
            return None;
        }
        let slot = slot.min(d.layers.len() - 1);
        d.layers.get(d.layers.len() - 1 - slot)
    }

    /// The live active layer of `d` (current slot).
    fn active_layer_of<'a>(&self, d: &'a anim_core::model::Drawing) -> Option<&'a CelLayer> {
        Self::layer_at_slot(d, self.view.active_layer_slot)
    }

    /// Props of the layer the NEXT stroke will actually edit — based on the
    /// frame's OWN cel only. None on held/blank frames (the stroke will create
    /// a fresh template cel: visible, opacity 1), so chip/tint never describe
    /// a held drawing's layer the commit won't touch.
    pub fn active_layer_props(&self) -> Option<&anim_core::model::LayerProps> {
        let id = self.own_key_drawing()?;
        let d = self.cut().drawing(id)?;
        self.active_layer_of(d).map(|l| &l.props)
    }

    /// Name of the active layer — template names on held/blank frames
    /// (the layer the next stroke will land on).
    pub fn active_layer_name(&self) -> String {
        if let Some(p) = self.active_layer_props() {
            return p.name.clone();
        }
        // Fresh-cel template is [color, line]; slot 0 = line (top).
        match self.view.active_layer_slot.min(1) {
            0 => "line".into(),
            _ => "color".into(),
        }
    }

    /// Cycle the active layer (A / Shift+A). Wraps within the OWN cel's stack
    /// depth; on held/blank frames, the new-cel template depth (2).
    pub fn cycle_layer(&mut self, back: bool) {
        let n = self
            .own_key_drawing()
            .and_then(|id| self.cut().drawing(id))
            .map(|d| d.layers.len())
            .filter(|n| *n > 0)
            .unwrap_or(2);
        let slot = self.view.active_layer_slot.min(n - 1);
        self.view.active_layer_slot = if back { (slot + n - 1) % n } else { (slot + 1) % n };
        self.status = format!("layer: {}", self.active_layer_name());
    }

    /// Clear this frame's own cel raster (undoable). No-op on a held frame that
    /// doesn't own a cel, so clearing never wipes a drawing shared by others.
    pub fn clear_current_raster(&mut self) {
        let Some(id) = self.own_key_drawing() else {
            return;
        };
        let at = self.at();
        let (layer, before): (_, BTreeMap<TileCoord, Arc<TileData>>) =
            match self.cut().drawing(id).and_then(|d| self.active_layer_of(d)) {
                Some(l) if !l.raster.tiles.is_empty() => (l.id, l.raster.tiles.clone()),
                _ => return,
            };
        let diff: TileDiff = before
            .into_iter()
            .map(|(c, b)| (c, Some(b), None))
            .collect();
        let r = self
            .engine
            .apply("clear layer", vec![Command::PaintTiles { at, id, layer, diff }]);
        self.report(r, "layer cleared");
    }

    /// Clear EVERY layer of this frame's own cel (Shift+D) — one undo step.
    pub fn clear_current_cel_all(&mut self) {
        let Some(id) = self.own_key_drawing() else {
            return;
        };
        let at = self.at();
        let clears: Vec<Command> = match self.cut().drawing(id) {
            Some(d) => d
                .layers
                .iter()
                .filter(|l| !l.raster.tiles.is_empty())
                .map(|l| Command::PaintTiles {
                    at,
                    id,
                    layer: l.id,
                    diff: l
                        .raster
                        .tiles
                        .iter()
                        .map(|(c, b)| (*c, Some(b.clone()), None))
                        .collect(),
                })
                .collect(),
            None => return,
        };
        if clears.is_empty() {
            return;
        }
        let r = self.engine.apply("clear cel", clears);
        self.report(r, "cel cleared (all layers)");
    }

    // ---- Layer strip operations (all one-command = one undo step) ---------

    /// The own cel's layer list for the strip, TOP-FIRST (strip row order),
    /// with each layer's stack index. Empty on held/blank frames.
    pub fn strip_layers(&self) -> Vec<(usize, &CelLayer)> {
        let Some(id) = self.own_key_drawing() else {
            return Vec::new();
        };
        let Some(d) = self.cut().drawing(id) else {
            return Vec::new();
        };
        d.layers.iter().enumerate().rev().collect()
    }

    fn own_drawing_id(&self) -> Option<DrawingId> {
        self.own_key_drawing()
    }

    /// Replace one layer's props (rename / eye / opacity-drag end).
    pub fn layer_set_props(&mut self, layer: LayerId, props: anim_core::model::LayerProps) {
        let Some(id) = self.own_drawing_id() else {
            return;
        };
        let at = self.at();
        let r = self.engine.apply(
            "layer properties",
            vec![Command::SetCelLayerProps {
                at,
                drawing: id,
                layer,
                props,
            }],
        );
        self.report(r, "layer updated");
    }

    /// Remove a layer (refused on the last one — a raster cel keeps >= 1).
    pub fn layer_remove(&mut self, layer: LayerId) {
        let Some(id) = self.own_drawing_id() else {
            return;
        };
        if self.cut().drawing(id).map(|d| d.layers.len()) <= Some(1) {
            self.status = "a cel keeps at least one layer".into();
            return;
        }
        let at = self.at();
        let r = self.engine.apply(
            "remove layer",
            vec![Command::RemoveCelLayer {
                at,
                drawing: id,
                layer,
            }],
        );
        self.report(r, "layer removed");
    }

    /// Move a layer one step up (toward the top) or down in the stack.
    pub fn layer_move(&mut self, layer: LayerId, up: bool) {
        let Some(id) = self.own_drawing_id() else {
            return;
        };
        let Some(d) = self.cut().drawing(id) else {
            return;
        };
        let Some(idx) = d.layer_index(layer) else {
            return;
        };
        let to = if up { idx + 1 } else { idx.wrapping_sub(1) };
        if to >= d.layers.len() {
            return; // already at the end
        }
        let at = self.at();
        let r = self.engine.apply(
            "move layer",
            vec![Command::MoveCelLayer {
                at,
                drawing: id,
                layer,
                to_index: to,
            }],
        );
        self.report(r, "layer moved");
    }

    /// Anime-pipeline stack rank for the preset names (bottom → top):
    /// color < shadow < highlight < rough < line < correction.
    fn preset_rank(name: &str) -> Option<u8> {
        match name {
            "color" => Some(0),
            "shadow" => Some(1),
            "highlight" => Some(2),
            "rough" => Some(3),
            "line" => Some(4),
            "correction" => Some(5),
            _ => None,
        }
    }

    /// Add a preset layer at its anime-correct stack position: directly above
    /// the topmost existing layer whose known rank is <= the preset's (bottom
    /// if none). "empty" (no rank) inserts above the active layer.
    pub fn layer_add_preset(&mut self, name: &str) {
        let Some(id) = self.own_drawing_id() else {
            self.status = "no cel on this frame — draw or press E first".into();
            return;
        };
        let Some(d) = self.cut().drawing(id) else {
            return;
        };
        if d.layers.len() >= 8 {
            self.status = "layer cap (8) reached on this cel".into();
            return;
        }
        let index = match Self::preset_rank(name) {
            Some(rank) => d
                .layers
                .iter()
                .rposition(|l| Self::preset_rank(&l.props.name).is_some_and(|r| r <= rank))
                .map(|i| i + 1)
                .unwrap_or(0),
            None => {
                // "empty": directly above the active layer.
                self.active_layer_of(d)
                    .and_then(|l| d.layer_index(l.id))
                    .map(|i| i + 1)
                    .unwrap_or(d.layers.len())
            }
        };
        let at = self.at();
        // The generic "empty" menu item makes a plain renameable "layer".
        let lname = if name == "empty" { "layer" } else { name };
        let layer = CelLayer::new(self.engine.alloc_layer_id(), lname);
        let new_id = layer.id;
        let r = self.engine.apply(
            "add layer",
            vec![Command::AddCelLayer {
                at,
                drawing: id,
                index,
                layer,
            }],
        );
        if r.is_ok() {
            // Activate the new layer (slot counted from the top).
            if let Some(d) = self.cut().drawing(id)
                && let Some(i) = d.layer_index(new_id)
            {
                self.view.active_layer_slot = d.layers.len() - 1 - i;
            }
        }
        self.report(r, "layer added");
    }

    /// Nearest distinct drawings before / after the current frame on the active
    /// column (for onion skin). [0] = previous, [1] = next.
    ///
    /// Excludes the frame's OWN key, not the resolved (held) drawing: on a held
    /// or blank frame — where you draw a NEW cel — the drawing being held here is
    /// the very reference you want, so it must count as the "previous" ghost. If
    /// we excluded the resolved drawing (as `current_drawing`), a held frame's
    /// previous ghost would skip past it to the drawing before it, and the held
    /// drawing would vanish the instant you start drawing the new cel.
    pub fn onion_neighbors(&self) -> [Option<DrawingId>; 2] {
        let cur = self.own_key_drawing();
        let Some(col) = self.cut().xsheet.column(self.view.active_column) else {
            return [None, None];
        };
        let mut prev = None;
        for f in (0..self.view.frame).rev() {
            if let Some(d) = col.resolve(f)
                && Some(d) != cur {
                    prev = Some(d);
                    break;
                }
        }
        let mut next = None;
        for f in (self.view.frame + 1)..self.frame_count() {
            if let Some(d) = col.resolve(f)
                && Some(d) != cur
                && Some(d) != prev {
                    // Skip the drawing still held through these frames (it's the
                    // `prev` ghost) so a mid-hold frame doesn't show it as both.
                    next = Some(d);
                    break;
                }
        }
        [prev, next]
    }

    /// WHOLE-CEL composite of a drawing's visible layers (bottom→top slices +
    /// composite hash) — the onion ghost is the cel as it looks, all layers.
    /// Composite of `id`'s visible layers, optionally restricted to layers
    /// matching `filter_name` (the per-layer onion: a neighbour cel with no
    /// layer of that name contributes nothing, not a wrong-role ghost).
    pub fn drawing_composite(
        &self,
        id: DrawingId,
        filter_name: Option<&str>,
    ) -> Option<(LayerSlices<'_>, u64)> {
        let d = self.cut().drawing(id)?;
        let mut bytes: Vec<u8> = Vec::new();
        let mut slices = Vec::new();
        for l in &d.layers {
            if l.props.visible
                && l.props.opacity > 0.0
                && filter_name.is_none_or(|n| l.props.name == n)
            {
                bytes.extend_from_slice(&l.content_hash().to_le_bytes());
                slices.push((&l.raster.tiles, l.props.opacity));
            }
        }
        if slices.is_empty() {
            return None;
        }
        Some((slices, anim_core::value::fnv1a(&bytes)))
    }

    /// The composite of the drawing resolved (hold-aware) on `column` at the
    /// CURRENT frame — a non-active column's raster art for edit-view
    /// multi-column display (B4). None when the column holds nothing yet or
    /// resolves to a vector-only drawing.
    pub fn column_composite(&self, column: ColumnId) -> Option<(LayerSlices<'_>, u64)> {
        let id = self.resolve_at(column, self.view.frame)?;
        self.drawing_composite(id, None)
    }

    /// Which drawing the GPU canvas layer should DISPLAY at the current frame:
    /// - the frame's OWN cel if it has one (you're editing that cel);
    /// - nothing (blank) on a held/empty frame while onion is ON — you're about
    ///   to draw a NEW cel here, so the canvas is blank and the drawing held on
    ///   this frame shows as the onion ghost instead of a solid copy that would
    ///   appear to vanish the moment the new cel clears the layer;
    /// - the held (resolved) drawing when onion is OFF, for normal viewing.
    fn display_drawing(&self) -> Option<DrawingId> {
        if let Some(own) = self.own_key_drawing() {
            Some(own)
        } else if self.view.onion {
            None
        } else {
            self.current_drawing()
        }
    }

    /// Tiles of the ACTIVE layer of the displayed drawing (for the GPU active
    /// texture). None = blank (held frame w/ onion, or layerless drawing).
    pub fn active_layer_tiles(&self) -> Option<&BTreeMap<TileCoord, Arc<TileData>>> {
        let id = self.display_drawing()?;
        let d = self.cut().drawing(id)?;
        self.active_layer_of(d).map(|l| &l.raster.tiles)
    }

    /// Identity of the displayed ACTIVE layer (drawing, layer, content hash) —
    /// decides when the GPU active texture re-syncs. Blank display: the key
    /// must be UNIQUE PER FRAME (u64::MAX sentinel + frame), else navigating
    /// between two blank frames keeps the same key, the re-sync is skipped,
    /// and the layer shows stale pixels (a real historical bug).
    pub fn active_layer_key(&self) -> (u64, u64, u64) {
        match self.display_drawing() {
            Some(id) => match self.cut().drawing(id).and_then(|d| self.active_layer_of(d)) {
                Some(l) => (id.0, l.id.0, l.content_hash()),
                None => (id.0, u64::MAX, 0), // layerless (vector-only) drawing
            },
            None => (u64::MAX, self.view.frame as u64, 0),
        }
    }

    /// Visible layers strictly BELOW / ABOVE the active one in the displayed
    /// drawing (bottom→top slices for the sandwich projections).
    fn sandwich(&self, above: bool) -> LayerSlices<'_> {
        let Some(id) = self.display_drawing() else {
            return Vec::new();
        };
        let Some(d) = self.cut().drawing(id) else {
            return Vec::new();
        };
        let Some(active) = self.active_layer_of(d) else {
            return Vec::new();
        };
        let ai = d.layer_index(active.id).expect("active layer is in the stack");
        d.layers
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                l.props.visible
                    && l.props.opacity > 0.0
                    && if above { *i > ai } else { *i < ai }
            })
            .map(|(_, l)| (&l.raster.tiles, l.props.opacity))
            .collect()
    }

    pub fn below_layers(&self) -> LayerSlices<'_> {
        self.sandwich(false)
    }

    pub fn above_layers(&self) -> LayerSlices<'_> {
        self.sandwich(true)
    }

    /// Content key of one sandwich half — folds the displayed drawing id, the
    /// contributing layers' (id, content_hash), and the blank-frame sentinel,
    /// so frame flips, layer switches, visibility/opacity edits, reorders and
    /// undo into a buried layer all re-trigger the projection rebuild.
    fn stack_key(&self, above: bool) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.push(above as u8);
        match self.display_drawing() {
            Some(id) => {
                bytes.extend_from_slice(&id.0.to_le_bytes());
                if let Some(d) = self.cut().drawing(id)
                    && let Some(active) = self.active_layer_of(d)
                {
                    let ai = d.layer_index(active.id).expect("active in stack");
                    bytes.extend_from_slice(&(ai as u64).to_le_bytes());
                    for (i, l) in d.layers.iter().enumerate() {
                        let in_half = if above { i > ai } else { i < ai };
                        if in_half && l.props.visible && l.props.opacity > 0.0 {
                            bytes.extend_from_slice(&l.id.0.to_le_bytes());
                            bytes.extend_from_slice(&l.content_hash().to_le_bytes());
                        }
                    }
                }
            }
            None => {
                // Blank display: unique per frame (same law as active_layer_key).
                bytes.extend_from_slice(&u64::MAX.to_le_bytes());
                bytes.extend_from_slice(&(self.view.frame as u64).to_le_bytes());
            }
        }
        anim_core::value::fnv1a(&bytes)
    }

    pub fn below_stack_key(&self) -> u64 {
        self.stack_key(false)
    }

    pub fn above_stack_key(&self) -> u64 {
        self.stack_key(true)
    }

    /// Create an empty raster cel and expose it at the current frame — but only
    /// if this frame doesn't already have its own key (don't overwrite a cel).
    pub fn new_drawing_at_frame(&mut self) {
        if self.own_key_drawing().is_some() {
            self.status = "this frame already has a drawing".into();
            return;
        }
        let at = self.at();
        let name = self.next_drawing_name();
        let id = self.engine.alloc_drawing_id();
        let layers = self.new_cel_layers();
        let index = self.cut().drawings.len(); // append to the library
        let r = self.engine.apply(
            "new drawing",
            vec![
                Command::AddDrawing {
                    at,
                    id,
                    index,
                    name,
                    strokes: vec![],
                    layers,
                },
                Command::SetCell {
                    at,
                    column: self.view.active_column,
                    frame: self.view.frame,
                    key: Some(Exposure::Drawing(id)),
                },
            ],
        );
        if r.is_ok() {
            self.view.selected_drawing = Some(id);
        }
        self.report(r, "new drawing");
    }

    /// Expose the selected library drawing at the current frame (a hold key).
    pub fn expose_selected(&mut self) {
        let Some(id) = self.view.selected_drawing else {
            self.status = "select a drawing in the library first".into();
            return;
        };
        let at = self.at();
        let r = self.engine.apply(
            "expose drawing",
            vec![Command::SetCell {
                at,
                column: self.view.active_column,
                frame: self.view.frame,
                key: Some(Exposure::Drawing(id)),
            }],
        );
        self.report(r, "exposed");
    }

    /// Move/place/remove a hold-length terminator (an Exposure::Empty key) that
    /// ends a drawing's exposure early — the frames between it and the next
    /// drawing then show blank. `old`/`new` are the terminator frames; None =
    /// no terminator (holds to the next drawing). Undoable + saved.
    pub fn set_hold_terminator(&mut self, column: ColumnId, old: Option<u32>, new: Option<u32>) {
        if old == new {
            return;
        }
        let at = self.at();
        let mut cmds = Vec::new();
        if let Some(o) = old
            && Some(o) != new {
                cmds.push(Command::SetCell {
                    at,
                    column,
                    frame: o,
                    key: None,
                });
            }
        if let Some(n) = new {
            cmds.push(Command::SetCell {
                at,
                column,
                frame: n,
                key: Some(Exposure::Empty),
            });
        }
        if !cmds.is_empty() {
            let r = self.engine.apply("hold length", cmds);
            self.report(r, "hold length");
        }
    }

    /// Clear the key at the current frame (previous hold extends over it).
    pub fn clear_key_at_frame(&mut self) {
        let at = self.at();
        let r = self.engine.apply(
            "clear key",
            vec![Command::SetCell {
                at,
                column: self.view.active_column,
                frame: self.view.frame,
                key: None,
            }],
        );
        self.report(r, "key cleared");
    }

    pub fn add_column(&mut self) {
        let at = self.at();
        let name = format!("{}", (b'A' + (self.cut().xsheet.columns.len() as u8 % 26)) as char);
        match self.engine.add_column(at, name) {
            Ok(id) => {
                self.view.active_column = id;
                self.status = "column added".into();
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// Import a WAV as this cut's scratch audio track (one undo step;
    /// replaces any existing clip — the inverse restores it).
    pub fn import_audio(&mut self, path: &std::path::Path) {
        let at = self.at();
        // Size gate BEFORE reading: the picker filters by extension only,
        // so a mislabeled multi-GB file must fail here, not after a full
        // read into memory.
        match std::fs::metadata(path) {
            Ok(m) if m.len() > anim_core::model::MAX_AUDIO_BYTES as u64 => {
                self.status = format!(
                    "audio import failed: file is {} MB (max {} MB)",
                    m.len() / (1024 * 1024),
                    anim_core::model::MAX_AUDIO_BYTES / (1024 * 1024)
                );
                return;
            }
            Err(e) => {
                self.status = format!("audio read failed: {e}");
                return;
            }
            Ok(_) => {}
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.status = format!("audio read failed: {e}");
                return;
            }
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "audio.wav".into());
        let clip = match anim_core::model::AudioClip::from_wav(name, bytes) {
            Ok(c) => c,
            Err(e) => {
                self.status = format!("audio import failed: {e}");
                return;
            }
        };
        let secs = clip.seconds();
        let cname = clip.name.clone();
        if let Err(e) = self.engine.apply(
            "set audio",
            vec![Command::SetCutAudio { at, audio: Some(clip) }],
        ) {
            self.status = format!("error: {e}");
        } else {
            self.status = format!("audio: {cname} ({secs:.1}s)");
        }
    }

    /// Remove the cut's scratch audio (one undo step).
    pub fn remove_audio(&mut self) {
        let at = self.at();
        if let Err(e) = self
            .engine
            .apply("remove audio", vec![Command::SetCutAudio { at, audio: None }])
        {
            self.status = format!("error: {e}");
        } else {
            self.status = "audio removed".into();
        }
    }

    /// Export the current cut's timing as an .xdts exposure sheet.
    pub fn export_xdts(&mut self, path: &std::path::Path) {
        let scene_name = self
            .engine
            .project
            .scenes
            .iter()
            .find(|s| s.id == self.view.scene)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let text = anim_core::xdts::export(self.cut(), &scene_name);
        match std::fs::write(path, text) {
            Ok(()) => self.status = format!("exposure sheet exported: {}", path.display()),
            Err(e) => self.status = format!("XDTS export failed: {e}"),
        }
    }

    /// Import an .xdts exposure sheet as a NEW cut in the current scene:
    /// columns from its CELL tracks, one empty drawing (standard
    /// colour+line stack) per distinct cell number, keys as one undo step.
    /// Never touches an existing cut.
    pub fn import_xdts(&mut self, path: &std::path::Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.status = format!("XDTS read failed: {e}");
                return;
            }
        };
        let sheet = match anim_core::xdts::parse(&text) {
            Ok(s) => s,
            Err(e) => {
                self.status = format!("XDTS import failed: {e}");
                return;
            }
        };

        // Scaffolding: the new cut + its columns (non-undoable, like every
        // scene/cut/column creation).
        let scene = self.view.scene;
        let cut_id = match self.engine.add_cut(scene, sheet.name.clone(), sheet.duration) {
            Ok(id) => id,
            Err(e) => {
                self.status = format!("error: {e}");
                return;
            }
        };
        let at = CutRef { scene, cut: cut_id };
        let mut col_ids = Vec::new();
        for col in &sheet.columns {
            match self.engine.add_column(at, col.name.clone()) {
                Ok(id) => col_ids.push(id),
                Err(e) => {
                    self.status = format!("error: {e}");
                    return;
                }
            }
        }
        if col_ids.is_empty() {
            // A sheet with no tracks still needs a drawable column.
            let _ = self.engine.add_column(at, "A");
        }

        // One drawing per distinct cell number (sorted for stable library
        // order), each with the standard colour+line stack; then the keys.
        // ONE batch = one undo step for the whole timing import.
        let mut cells: Vec<String> = sheet
            .columns
            .iter()
            .flat_map(|c| c.keys.iter().filter_map(|(_, v)| v.clone()))
            .collect();
        cells.sort();
        cells.dedup();
        let mut commands = Vec::new();
        let mut drawing_of = std::collections::HashMap::new();
        for (index, cell) in cells.iter().enumerate() {
            let id = self.engine.alloc_drawing_id();
            let layers = vec![
                CelLayer::new(self.engine.alloc_layer_id(), "color"),
                CelLayer::new(self.engine.alloc_layer_id(), "line"),
            ];
            drawing_of.insert(cell.clone(), id);
            commands.push(Command::AddDrawing {
                at,
                id,
                index,
                name: cell.clone(),
                strokes: vec![],
                layers,
            });
        }
        for (col, col_id) in sheet.columns.iter().zip(&col_ids) {
            for (frame, value) in &col.keys {
                if *frame >= sheet.duration {
                    continue; // out-of-range key in a foreign file — drop it
                }
                let key = match value {
                    Some(cell) => anim_core::xsheet::Exposure::Drawing(drawing_of[cell]),
                    None => anim_core::xsheet::Exposure::Empty,
                };
                commands.push(Command::SetCell {
                    at,
                    column: *col_id,
                    frame: *frame,
                    key: Some(key),
                });
            }
        }
        if let Err(e) = self.engine.apply("import XDTS timing", commands) {
            self.status = format!("XDTS import failed: {e}");
            return;
        }
        let n = cells.len();
        self.goto_cut(scene, cut_id);
        self.status = format!(
            "imported exposure sheet '{}' — {} column(s), {} cel slot(s); draw into them",
            sheet.name,
            sheet.columns.len(),
            n
        );
    }

    /// Rename a drawing (one undo step; consumers re-render — the name is
    /// part of the eval recipe).
    pub fn rename_drawing(&mut self, id: DrawingId, name: &str) {
        let at = self.at();
        if let Err(e) = self.engine.apply(
            "rename drawing",
            vec![Command::RenameDrawing {
                at,
                id,
                name: name.to_string(),
            }],
        ) {
            self.status = format!("error: {e}");
        }
    }

    /// Rename the cut being edited (scaffolding, like scene/cut creation).
    pub fn rename_current_cut(&mut self, name: &str) {
        let at = self.at();
        match self.engine.rename_cut(at, name) {
            Ok(()) => self.status = format!("cut renamed to {name}"),
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// Rename the scene being edited (scaffolding).
    pub fn rename_current_scene(&mut self, name: &str) {
        match self.engine.rename_scene(self.view.scene, name) {
            Ok(()) => self.status = format!("scene renamed to {name}"),
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// Add a PARAMETER column (camera etc.) — scaffolding, like add_column;
    /// keys on it are undoable. Returns the new id for immediate binding.
    pub fn add_param_column(&mut self, name: &str) -> Option<anim_core::ids::ParamId> {
        let at = self.at();
        match self.engine.add_param_column(at, name) {
            Ok(id) => {
                self.status = format!("param column '{name}' added");
                Some(id)
            }
            Err(e) => {
                self.status = format!("error: {e}");
                None
            }
        }
    }

    /// Set/clear a key on a parameter column at the current frame (one
    /// undo step).
    pub fn set_param_key(
        &mut self,
        column: anim_core::ids::ParamId,
        key: Option<anim_core::xsheet::ParamKey>,
    ) {
        let at = self.at();
        let frame = self.view.frame;
        let label = if key.is_some() { "set param key" } else { "clear param key" };
        if let Err(e) = self.engine.apply(
            label,
            vec![Command::SetParamKey {
                at,
                column,
                frame,
                key,
            }],
        ) {
            self.status = format!("error: {e}");
        }
    }

    /// Remove the selected (active) column. Keeps at least one column.
    pub fn remove_active_column(&mut self) {
        if self.cut().xsheet.columns.len() <= 1 {
            self.status = "can't remove the last column".into();
            return;
        }
        let at = self.at();
        let removed = self.view.active_column;
        // Pick a neighbour to become active before removing.
        let cols: Vec<ColumnId> = self.cut().xsheet.columns.iter().map(|c| c.id).collect();
        let idx = cols.iter().position(|&c| c == removed).unwrap_or(0);
        let new_active = cols
            .get(idx + 1)
            .or_else(|| idx.checked_sub(1).and_then(|i| cols.get(i)))
            .copied()
            .unwrap_or(removed);
        self.engine.remove_column(at, removed);
        self.view.active_column = new_active;
        self.view.selected_drawing = None;
        self.status = "column removed".into();
    }

    pub fn undo(&mut self) {
        if self.engine.undo().is_ok() {
            self.status = "undo".into();
        }
        self.sanitize();
    }

    pub fn redo(&mut self) {
        if self.engine.redo().is_ok() {
            self.status = "redo".into();
        }
        self.sanitize();
    }

    /// Selection can dangle after undo of a creation command — never let a
    /// dead id escape into the UI.
    fn sanitize(&mut self) {
        if let Some(id) = self.view.selected_drawing
            && self.cut().drawing(id).is_none() {
                self.view.selected_drawing = None;
            }
        self.view.frame = self.view.frame.min(self.frame_count() - 1);
        // The param keying strip's value buffer mirrors engine state that
        // undo/redo may have just moved — force a reseed so the strip never
        // shows (and silently re-keys) an undone value.
        self.param_buf_at = None;
    }

    fn report(&mut self, r: anim_core::error::Result<()>, ok_msg: &str) {
        match r {
            Ok(()) => self.status = ok_msg.into(),
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    // ---- Transport --------------------------------------------------------

    pub fn goto(&mut self, frame: u32) {
        self.view.frame = frame.min(self.frame_count() - 1);
        self.view.play_acc = 0.0;
    }

    pub fn step(&mut self, delta: i64) {
        let n = self.frame_count() as i64;
        let f = (self.view.frame as i64 + delta).rem_euclid(n);
        self.goto(f as u32);
    }

    pub fn toggle_play(&mut self) {
        self.view.playing = !self.view.playing;
        self.view.play_acc = 0.0;
    }

    // ---- Scene/cut navigation (C2) -----------------------------------------
    // The model has always supported multiple scenes and multiple cuts per
    // scene (Project.scenes: Vec<Scene>, Scene.cuts: Vec<Cut>); there was
    // just no UI to create or switch between them — every project was
    // permanently stuck on its first SC01/CUT01. Scene/cut/column creation
    // is deliberately NON-UNDOABLE scaffolding (see the law on Engine::
    // add_scene) — same tier as New Project itself, not the moment-to-moment
    // drawing workflow.

    /// Switch to a different cut (any scene) — re-derives `active_column` as
    /// the new cut's OWN first column (columns never cross cuts) and resets
    /// the frame cursor to 0 (a different cut is a different timeline, not a
    /// scrub position worth preserving). View PREFERENCES (onion, loop, tool
    /// mode) are left alone — those are yours, not the cut's.
    pub fn goto_cut(&mut self, scene: SceneId, cut: CutId) {
        let found = self
            .engine
            .project
            .cut(scene, cut)
            .and_then(|c| c.xsheet.columns.first().map(|col| (col.id, c.name.clone())));
        let Some((column, name)) = found else {
            self.status = "that cut has no columns yet — add one first".into();
            return;
        };
        self.view.scene = scene;
        self.view.cut = cut;
        self.view.active_column = column;
        self.view.frame = 0;
        self.view.selected_drawing = None;
        self.view.playing = false;
        self.view.active_layer_slot = 0;
        self.status = format!("cut: {name}");
    }

    /// Add a new scene with a first cut and column, and switch to it — the
    /// same boot New Project performs, minus the whole-project reset.
    /// Auto-named on the project's own SC../CUT.. convention (a rename UI
    /// is a natural fast-follow; the name field already supports it).
    pub fn new_scene(&mut self) {
        let n = self.engine.project.scenes.len() + 1;
        let scene = self.engine.add_scene(format!("SC{n:02}"));
        let Ok(cut) = self.engine.add_cut(scene, "CUT01", 48) else {
            self.status = "new scene failed".into();
            return;
        };
        if let Err(e) = self.engine.add_column(CutRef { scene, cut }, "A") {
            self.status = format!("new scene failed: {e:?}");
            return;
        }
        self.goto_cut(scene, cut);
    }

    /// Add a new cut (+ its first column) to an existing scene, and switch
    /// to it.
    pub fn new_cut(&mut self, scene: SceneId) {
        let n = self
            .engine
            .project
            .scenes
            .iter()
            .find(|s| s.id == scene)
            .map(|s| s.cuts.len() + 1)
            .unwrap_or(1);
        let Ok(cut) = self.engine.add_cut(scene, format!("CUT{n:02}"), 48) else {
            self.status = "new cut failed".into();
            return;
        };
        if let Err(e) = self.engine.add_column(CutRef { scene, cut }, "A") {
            self.status = format!("new cut failed: {e:?}");
            return;
        }
        self.goto_cut(scene, cut);
    }

    pub fn tick(&mut self, dt: f32) {
        if !self.view.playing {
            return;
        }
        self.view.play_acc += dt;
        let frame_time = 1.0 / self.fps() as f32;
        while self.view.play_acc >= frame_time {
            self.view.play_acc -= frame_time;
            if self.view.frame + 1 >= self.frame_count() {
                if self.view.loop_playback {
                    self.view.frame = 0;
                } else {
                    self.view.playing = false; // hold on the last frame
                    self.view.play_acc = 0.0;
                    break;
                }
            } else {
                self.view.frame += 1;
            }
        }
    }

    // ---- Node graph (all mutations = engine commands = undoable) -----------

    /// Import a PNG as a cut image asset AND wire an ImageSource node for it
    /// — ONE batch, one undo step (undo removes both the node and the asset).
    pub fn graph_import_image(&mut self, path: &std::path::Path, pos: (f32, f32)) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.status = format!("image import failed: {e}");
                return;
            }
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".into());
        let at = self.at();
        let index = self.cut().images.len();
        let image = self.engine.alloc_image_id();
        let node = self.engine.alloc_node_id();
        let r = self.engine.apply(
            "import image",
            vec![
                Command::AddImage { at, id: image, index, name, bytes },
                Command::AddNode {
                    at,
                    id: node,
                    kind: anim_core::graph::NodeKind::ImageSource { image },
                },
            ],
        );
        if r.is_ok() {
            self.node_positions.insert(node.0, pos);
            self.selected_node = Some(node);
        }
        self.report(r, "image imported");
    }

    pub fn graph_add_node(&mut self, kind: anim_core::graph::NodeKind, pos: (f32, f32)) {
        let at = self.at();
        let id = self.engine.alloc_node_id();
        let r = self
            .engine
            .apply("add node", vec![Command::AddNode { at, id, kind }]);
        if r.is_ok() {
            self.node_positions.insert(id.0, pos);
            self.selected_node = Some(id);
        }
        self.report(r, "node added");
    }

    pub fn graph_remove_node(&mut self, id: NodeId) {
        let at = self.at();
        let r = self
            .engine
            .apply("remove node", vec![Command::RemoveNode { at, id }]);
        if r.is_ok() {
            self.node_positions.remove(&id.0);
            if self.selected_node == Some(id) {
                self.selected_node = None;
            }
        }
        self.report(r, "node removed");
    }

    pub fn graph_connect(&mut self, from: NodeId, to: NodeId, to_pin: u16) {
        let at = self.at();
        let r = self.engine.apply(
            "connect",
            vec![Command::Connect {
                at,
                from,
                from_pin: 0,
                to,
                to_pin,
            }],
        );
        self.report(r, "connected");
    }

    pub fn graph_disconnect(&mut self, to: NodeId, to_pin: u16) {
        let at = self.at();
        let r = self
            .engine
            .apply("disconnect", vec![Command::Disconnect { at, to, to_pin }]);
        self.report(r, "disconnected");
    }

    pub fn graph_set_output(&mut self, node: NodeId) {
        let at = self.at();
        let r = self.engine.apply(
            "set output",
            vec![Command::SetOutput {
                at,
                node: Some(node),
            }],
        );
        self.report(r, "output set");
    }

    pub fn graph_set_kind(&mut self, id: NodeId, kind: anim_core::graph::NodeKind) {
        let at = self.at();
        let r = self
            .engine
            .apply("node parameters", vec![Command::SetNodeKind { at, id, kind }]);
        self.report(r, "node updated");
    }

    /// Give every unplaced node a default spot: sources in a left column,
    /// processors centre, output right — a readable first layout.
    pub fn ensure_node_layout(&mut self) {
        let mut new_positions: Vec<(u64, (f32, f32))> = Vec::new();
        {
            let cut = self.cut();
            let (mut n_src, mut n_mid, mut n_out) = (0, 0, 0);
            for node in cut.graph.nodes() {
                if self.node_positions.contains_key(&node.id.0) {
                    continue;
                }
                use anim_core::graph::NodeKind::*;
                let (col_x, row) = match node.kind {
                    DrawingSource { .. } | Solid { .. } | ImageSource { .. } => {
                        n_src += 1;
                        (40.0, n_src - 1)
                    }
                    Transform { .. } | Blend { .. } => {
                        n_mid += 1;
                        (260.0, n_mid - 1)
                    }
                    Output => {
                        n_out += 1;
                        (480.0, n_out - 1)
                    }
                };
                new_positions.push((node.id.0, (col_x, 40.0 + row as f32 * 90.0)));
            }
        }
        for (id, pos) in new_positions {
            self.node_positions.insert(id, pos);
        }
    }

    // ---- Files ------------------------------------------------------------

    pub fn save(&mut self, force_dialog: bool) {
        let path = if force_dialog || self.file_path.is_none() {
            rfd::FileDialog::new()
                .add_filter("AnimStudio project", &["animproj"])
                .set_file_name("untitled.animproj")
                .save_file()
        } else {
            self.file_path.clone()
        };
        let Some(path) = path else { return };
        // Palettes are project data but outside the undo/Command system —
        // freshen the project's opaque meta right before the actual write,
        // the same point every other "current in-memory state" gets synced.
        self.palettes.save_into(&mut self.engine.project);
        match self.engine.save(&path) {
            Ok(()) => {
                self.status = format!("saved {}", path.display());
                self.file_path = Some(path);
            }
            Err(e) => self.status = format!("save failed: {e}"),
        }
    }

    pub fn open(&mut self) {
        match Self::pick_and_open() {
            Some(Ok(new_state)) => *self = new_state,
            Some(Err(msg)) => self.status = format!("open failed: {msg}"),
            None => {}
        }
    }
}
