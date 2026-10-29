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
use anim_core::model::{CelLayer, LayerProps};
use anim_core::model::{Cut, Stroke};
use anim_core::raster::{TileCoord, TileData, TileDiff};
use anim_core::xsheet::Exposure;

/// Bottom→top display slices of cel layers: (tiles, effective opacity).
pub type LayerSlices<'a> = Vec<(&'a BTreeMap<TileCoord, Arc<TileData>>, f32)>;

/// PSD-cel-layers Stage 4: what the strip asks of the App (which owns the
/// config and the room binding).
#[derive(Debug, Clone)]
pub enum StackRequest {
    /// Bind this stack to the current room's new cels.
    Pick(String),
    /// Save this cel's STRUCTURE as a user stack under this name.
    Save(String),
}

/// The merged solo douga/shiage pair — "color" bottom, "line" top — the
/// fallback new-cel template (and the Solo merged stack preset).
pub fn solo_template() -> Vec<LayerProps> {
    use anim_core::model::LayerRole;
    [LayerRole::Color, LayerRole::Line]
        .into_iter()
        .map(|r| LayerProps {
            name: r.default_name().into(),
            role: r,
            ..LayerProps::default()
        })
        .collect()
}

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
    /// The ACTIVE LAYER, tracked BY NAME (spec defect 16): the silent
    /// slot-clamp is dead. If this name is absent on the cel under the
    /// playhead, the state is UNARMED — no layer receives ink and the pen
    /// refuses. Silence is never an acceptable response to state changing
    /// under the hand.
    pub active_layer: String,
    /// PSD-cel-layers: the ROLE of the armed layer, remembered beside its
    /// name. Follow matches by name first; when a cel has no layer of
    /// that name, exactly one layer holding this role is the target (a
    /// rename no longer unarms the pen). Custom never falls back.
    pub active_role: anim_core::model::LayerRole,
    /// Loop playback at the end of the cut (off = stop on the last frame).
    pub loop_playback: bool,
    /// LINE GUARD (shiage charter): while on, the `line`-named layer
    /// cannot be ARMED as the ink target — cycling skips it, tapping its
    /// row refuses. Arming-only: the pen path is untouched. Defaults ON
    /// when entering the Finishing room, OFF elsewhere.
    pub line_guard: bool,
    /// THE GHOST PIN (room ratified 2026-08-17): a library drawing pinned
    /// to the light box — rendered in Ao at the onion strength on EVERY
    /// frame, with no sheet entry, no export, no document change. Pure
    /// display; dies with the session (NEVER-DO 1 of its room).
    pub ghost_pin: Option<DrawingId>,
}

/// The document (engine + where it lives on disk) plus the single "Main"
/// view context and app-transient UI furniture. Mutations still route
/// exclusively through `engine.apply` — the split changes no behavior.
pub struct AppState {
    pub engine: Engine,
    pub view: ViewContext,
    pub file_path: Option<PathBuf>,
    pub status: String,
    /// THE REFUSAL channel (the Foot's Aka lane): the machine overruled
    /// the hand. Separate from `status` chatter; `refusal_seq` bumps on
    /// every refusal so repeats re-flash.
    pub refusal: String,
    /// SESSION MIRROR: true after any undo/redo — the pinned contract
    /// says mirrors need a fresh snapshot after history rewrites.
    pub history_dirty: bool,
    pub refusal_seq: u32,
    /// SESSION generation stamp: bumps on every reported commit and on
    /// undo/redo — the host's cheap "the document moved" signal (the
    /// engine's history is private, and rightly so).
    pub doc_gen: u64,
    /// GM2: the engine revision last written to `file_path`. Dirty = the
    /// engine moved off it, or `meta_dirty` (see `is_dirty`).
    pub saved_rev: u64,
    /// GM2: an edit outside the undo system since the last save —
    /// scaffolding (columns, cuts, scenes, renames) and palette edits
    /// change the file without moving the revision.
    pub meta_dirty: bool,
    /// GM2: the save running on a worker thread, if any.
    pub saving: Option<SaveJob>,
    /// GM2: this document's autosave group while it has no file — its own,
    /// so two untitled projects that share a name never share (or prune)
    /// each other's autosaves. A file's group comes from its path instead.
    pub untitled_group: String,
    /// THE SWATCH BOARD's active character tab (shiage; UI-transient).
    pub active_character: usize,
    /// EDIT MODEL latch: the palette board exposes its editing controls
    /// only while this is on (model edits propagate everywhere).
    pub palette_edit: bool,
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
    /// PSD-cel-layers: the stack (bottom → top, structure only) a stroke
    /// on an empty frame creates when there's no held cel to clone. Set
    /// by the App from the chosen stack preset every frame; defaults to
    /// the merged solo pair.
    pub new_cel_template: Vec<LayerProps>,
    /// The layer a fresh template cel arms (by name + role).
    pub new_cel_active: (String, anim_core::model::LayerRole),
    /// PSD-cel-layers Stage 4: every pickable stack (built-in + the
    /// artist's own) and the one bound to the current room — refreshed by
    /// the App each frame; the strip's `stack ▾` menu reads them.
    pub stacks: Vec<crate::config::StackPreset>,
    pub stack_current: String,
    /// Strip → App requests (the App owns the config).
    pub stack_request: Option<StackRequest>,
    pub stack_name_buf: String,
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
        let untitled_group = crate::worksafe::untitled_group(&engine.project.name);
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
                active_layer: "line".into(),
                active_role: anim_core::model::LayerRole::Line,
                loop_playback: true,
                ghost_pin: None,
                line_guard: false,
            },
            file_path: None,
            refusal: String::new(),
            history_dirty: false,
            refusal_seq: 0,
            doc_gen: 0,
            saved_rev: 0,
            meta_dirty: false,
            saving: None,
            untitled_group,
            active_character: 0,
            palette_edit: false,
            status: "new project — draw on the canvas to create frame 1".into(),
            positioned_holds: HashSet::new(),
            palettes: Default::default(),
            palette_new_name: String::new(),
            strip_rename: None,
            drawing_rename: None,
            strip_opacity: None,
            new_cel_template: solo_template(),
            new_cel_active: ("line".into(), anim_core::model::LayerRole::Line),
            stacks: Vec::new(),
            stack_current: String::new(),
            stack_request: None,
            stack_name_buf: String::new(),
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
        let scene = engine
            .project
            .scenes
            .first()
            .ok_or("project has no scenes")?;
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
                active_layer: "line".into(),
                active_role: anim_core::model::LayerRole::Line,
                loop_playback: true,
                ghost_pin: None,
                line_guard: false,
            },
            file_path: Some(path),
            refusal: String::new(),
            history_dirty: false,
            refusal_seq: 0,
            doc_gen: 0,
            saved_rev: engine.revision(),
            meta_dirty: false,
            saving: None,
            untitled_group: crate::worksafe::untitled_group(&engine.project.name),
            active_character: 0,
            palette_edit: false,
            status: "project loaded".into(),
            palettes: crate::palette::Palettes::load_from(&engine.project),
            palette_new_name: String::new(),
            engine,
            positioned_holds: HashSet::new(),
            strip_rename: None,
            drawing_rename: None,
            strip_opacity: None,
            new_cel_template: solo_template(),
            new_cel_active: ("line".into(), anim_core::model::LayerRole::Line),
            stacks: Vec::new(),
            stack_current: String::new(),
            stack_request: None,
            stack_name_buf: String::new(),
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
    /// Jump to the adjacent KEY on the active column (Q/W and the
    /// shiage cel chevrons) — the sheet's own step, a read then a goto.
    pub fn goto_adjacent_key(&mut self, forward: bool) {
        let target = {
            let here = self.view.frame;
            let count = self.frame_count();
            let active = self.view.active_column;
            let cut = self.cut();
            cut.xsheet
                .columns
                .iter()
                .find(|c| c.id == active)
                .and_then(|col| {
                    if forward {
                        (here + 1..count).find(|f| col.key_at(*f).is_some())
                    } else {
                        (0..here).rev().find(|f| col.key_at(*f).is_some())
                    }
                })
        };
        if let Some(f) = target {
            self.goto(f);
        }
    }

    /// Step to the previous/next CUT in shooting order (henshu head
    /// chevrons, PageUp/PageDown). A view move on existing goto_cut.
    pub fn step_cut(&mut self, forward: bool) {
        let flat: Vec<(anim_core::ids::SceneId, anim_core::ids::CutId)> = self
            .engine
            .project
            .scenes
            .iter()
            .flat_map(|sc| sc.cuts.iter().map(|c| (sc.id, c.id)))
            .collect();
        let Some(pos) = flat
            .iter()
            .position(|(s, c)| *s == self.view.scene && *c == self.view.cut)
        else {
            return;
        };
        let n = flat.len();
        if n < 2 {
            return;
        }
        let next = if forward {
            (pos + 1) % n
        } else {
            (pos + n - 1) % n
        };
        let (s, c) = flat[next];
        self.goto_cut(s, c);
    }

    /// Refuse the hand: routed to the Foot's Aka lane and the canvas
    /// edge flash. Chatter (`status`) is for remarks; this is for NO.
    /// SESSION MIRROR: set whenever undo/redo rewrites the past — the
    /// pinned contract says a mirror must be resynced after it.
    pub fn history_moved(&mut self) {
        self.history_dirty = true;
    }

    pub fn refuse(&mut self, msg: impl Into<String>) {
        self.refusal = msg.into();
        self.refusal_seq = self.refusal_seq.wrapping_add(1);
    }
    fn next_drawing_name(&self) -> String {
        let taken: std::collections::HashSet<&str> = self
            .cut()
            .drawings
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        let (mut n, suffix) = match self.cut().xsheet.column(self.view.active_column) {
            Some(col) => {
                let distinct: std::collections::HashSet<DrawingId> = col
                    .keys()
                    .filter_map(|(_, e)| match e {
                        Exposure::Drawing(d) => Some(d),
                        Exposure::Empty => None,
                    })
                    .collect();
                (distinct.len() + 1, col.name.clone())
            }
            None => (self.cut().drawings.len() + 1, String::new()),
        };
        // Uniqueness law: a library with three "D1A"s is unreadable and
        // undeletable-by-eye (owner, 2026-08-17). Bump until unused.
        loop {
            let name = format!("D{n}{suffix}");
            if !taken.contains(name.as_str()) {
                return name;
            }
            n += 1;
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
        let r = self.engine.apply(
            label,
            vec![Command::PaintTiles {
                at,
                id,
                layer,
                diff,
            }],
        );
        match r {
            Ok(_) => self.status = format!("{label} applied ({n} tile(s))"),
            // AUDIT [45]: this showed the engine's Debug enum to the
            // artist, e.g. InvalidTarget { drawing: DrawingId(7) }.
            Err(e) => self.refuse(format!("refused — {label} did not apply ({e})")),
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
                        Command::PaintTiles {
                            at,
                            id,
                            layer,
                            diff,
                        },
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
                    cmds.push(Command::PaintTiles {
                        at,
                        id,
                        layer,
                        diff,
                    });
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
                let r = self.engine.apply(
                    "paint",
                    vec![Command::PaintTiles {
                        at,
                        id,
                        layer,
                        diff,
                    }],
                );
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
        // Structure first (owned) — self.cut() borrows self.engine.project
        // immutably, which would collide with alloc_layer_id()'s &mut below.
        let specs = self.new_cel_specs();
        specs
            .into_iter()
            .map(|props| CelLayer {
                id: self.engine.alloc_layer_id(),
                props,
                raster: Default::default(),
            })
            .collect()
    }

    /// The STRUCTURE (bottom → top) a new cel on this frame would get:
    /// the held cel's stack cloned (ratified precedence — pro stacks are
    /// per-scene consistent), else the chosen stack preset.
    pub fn new_cel_specs(&self) -> Vec<LayerProps> {
        let prev_props: Vec<LayerProps> = self
            .resolve_at(self.view.active_column, self.view.frame)
            .and_then(|id| self.cut().drawing(id))
            .map(|d| d.layers.iter().map(|l| l.props.clone()).collect())
            .unwrap_or_default();
        if !prev_props.is_empty() {
            return prev_props;
        }
        if self.new_cel_template.is_empty() {
            solo_template()
        } else {
            self.new_cel_template.clone()
        }
    }

    /// PSD-cel-layers: resolve the armed layer in a stack — by NAME first,
    /// then by ROLE when that name is absent and exactly ONE layer holds
    /// the armed role (a renamed layer keeps its job). Custom never falls
    /// back; ambiguity never guesses. Returns the stack index (bottom = 0).
    fn resolve_armed(&self, props: &[&LayerProps]) -> Option<usize> {
        if let Some(i) = props
            .iter()
            .rposition(|p| p.name == self.view.active_layer)
        {
            return Some(i);
        }
        let role = self.view.active_role;
        if role == anim_core::model::LayerRole::Custom {
            return None;
        }
        let mut hits = props
            .iter()
            .enumerate()
            .filter(|(_, p)| p.role == role)
            .map(|(i, _)| i);
        let first = hits.next()?;
        if hits.next().is_some() {
            return None;
        }
        Some(first)
    }

    /// Arm a layer as the ink target: name AND role, together, always.
    pub fn arm_layer(&mut self, props: &LayerProps) {
        self.view.active_layer = props.name.clone();
        self.view.active_role = props.role;
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

    /// The live active layer of `d` — by NAME (defect 16), then by ROLE
    /// when the name is absent (PSD-cel-layers; see `resolve_armed`).
    fn active_layer_of<'a>(&self, d: &'a anim_core::model::Drawing) -> Option<&'a CelLayer> {
        let props: Vec<&LayerProps> = d.layers.iter().map(|l| &l.props).collect();
        self.resolve_armed(&props).map(|i| &d.layers[i])
    }

    /// Resolve the armed layer to a top-first slot on the current cel — or
    /// on the REAL new-cel template for held/blank frames (the held cel's
    /// cloned stack, else the chosen preset — never a hardcoded pair).
    /// None = UNARMED: the pen must refuse rather than misroute (defect 16).
    pub fn active_slot_resolved(&self) -> Option<usize> {
        if let Some(id) = self.own_key_drawing() {
            let d = self.cut().drawing(id)?;
            let props: Vec<&LayerProps> = d.layers.iter().map(|l| &l.props).collect();
            return self.resolve_armed(&props).map(|i| d.layers.len() - 1 - i);
        }
        let specs = self.new_cel_specs();
        let props: Vec<&LayerProps> = specs.iter().collect();
        self.resolve_armed(&props).map(|i| specs.len() - 1 - i)
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

    /// The armed layer on this frame's OWN cel, as resolved (name, then
    /// role) — what the strip highlights and DELETE LAYER targets.
    pub fn active_layer_id(&self) -> Option<LayerId> {
        let id = self.own_key_drawing()?;
        let d = self.cut().drawing(id)?;
        self.active_layer_of(d).map(|l| l.id)
    }

    /// PSD-cel-layers: the edit-lock guard, shared by every painting
    /// tool (pen, mouse, fill, lasso fill, select). True = refused (the
    /// refusal is already on the Aka lane). A lock is the artist's own
    /// "don't touch this" — overruling the hand is exactly a refusal.
    pub fn refuse_if_locked(&mut self) -> bool {
        // On a held/blank frame the stroke will CREATE a cel from the
        // template — a locked template layer (the Shiage preset locks its
        // lines) must refuse exactly like a locked layer on a real cel.
        let locked = match self.active_layer_props() {
            Some(p) => p.locked,
            None if self.own_key_drawing().is_none() => {
                let specs = self.new_cel_specs();
                let props: Vec<&LayerProps> = specs.iter().collect();
                self.resolve_armed(&props).is_some_and(|i| specs[i].locked)
            }
            None => false,
        };
        if locked {
            self.refuse(format!(
                "refused — layer '{}' is locked (unlock it in its layer card)",
                self.active_layer_name()
            ));
            return true;
        }
        false
    }

    /// Name of the active layer — template names on held/blank frames
    /// (the layer the next stroke will land on).
    pub fn active_layer_name(&self) -> String {
        self.view.active_layer.clone()
    }

    /// Cycle the active layer (A / Shift+A). Wraps within the OWN cel's stack
    /// depth; on held/blank frames, the new-cel template depth (2).
    pub fn cycle_layer(&mut self, back: bool) {
        // Top-first, like the strip. Held/blank frames cycle the REAL
        // new-cel template (cloned held stack, else the chosen preset).
        let stack: Vec<LayerProps> = self
            .own_key_drawing()
            .and_then(|id| self.cut().drawing(id))
            .map(|d| d.layers.iter().rev().map(|l| l.props.clone()).collect())
            .filter(|v: &Vec<LayerProps>| !v.is_empty())
            .unwrap_or_else(|| self.new_cel_specs().into_iter().rev().collect());
        // LINE GUARD: cycling skips every LINE-ROLE layer while guarded —
        // renamed or not (PSD-cel-layers NEVER-DO 1) — unless nothing else
        // exists to arm.
        let stack: Vec<LayerProps> = if self.view.line_guard {
            let filtered: Vec<LayerProps> =
                stack.iter().filter(|p| !p.role.is_line()).cloned().collect();
            if filtered.is_empty() { stack } else { filtered }
        } else {
            stack
        };
        if stack.is_empty() {
            return;
        }
        let bottom_first: Vec<&LayerProps> = stack.iter().rev().collect();
        let cur = self
            .resolve_armed(&bottom_first)
            .map(|i| stack.len() - 1 - i)
            .unwrap_or(0);
        let n = stack.len();
        let next = if back {
            (cur + n - 1) % n
        } else {
            (cur + 1) % n
        };
        let pick = stack[next].clone();
        self.arm_layer(&pick);
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
        let r = self.engine.apply(
            "clear layer",
            vec![Command::PaintTiles {
                at,
                id,
                layer,
                diff,
            }],
        );
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
            self.refuse("refused — a cel keeps at least one layer");
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

    /// PSD-cel-layers: where a layer of `role` belongs in `stack` (bottom
    /// → top): directly above the topmost layer whose ROLE rank is <= its
    /// own (bottom if none). Custom has no rank: above `custom_at`.
    pub fn rank_insert_index(
        stack: &[&LayerProps],
        role: anim_core::model::LayerRole,
        custom_at: usize,
    ) -> usize {
        match role.rank() {
            Some(rank) => stack
                .iter()
                .rposition(|p| p.role.rank().is_some_and(|r| r <= rank))
                .map(|i| i + 1)
                .unwrap_or(0),
            None => custom_at.min(stack.len()),
        }
    }

    /// Add a layer of `role` at its pipeline position (ranked by ROLE, not
    /// by name — a renamed shadow still orders as a shadow). Custom inserts
    /// above the active layer. Arms the new layer. One undo step.
    pub fn layer_add_role(&mut self, role: anim_core::model::LayerRole) {
        let Some(id) = self.own_drawing_id() else {
            self.refuse("refused — no cel on this frame (draw, or press E)");
            return;
        };
        let Some(d) = self.cut().drawing(id) else {
            return;
        };
        if d.layers.len() >= 8 {
            self.refuse("refused — this cel already has 8 layers");
            return;
        }
        let above_active = self
            .active_layer_of(d)
            .and_then(|l| d.layer_index(l.id))
            .map(|i| i + 1)
            .unwrap_or(d.layers.len());
        let stack: Vec<&LayerProps> = d.layers.iter().map(|l| &l.props).collect();
        let index = Self::rank_insert_index(&stack, role, above_active);
        let at = self.at();
        let layer = CelLayer::with_role(self.engine.alloc_layer_id(), role.default_name(), role);
        let props = layer.props.clone();
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
            self.arm_layer(&props);
        }
        self.report(r, "layer added");
    }

    /// PSD-cel-layers NEVER-DO 2: plan the ADDITIVE conform of one cel to
    /// `preset` — only the layers it LACKS (by role; custom layers by
    /// name), each inserted at its role rank in a simulated stack so the
    /// batch's indices stay exact in order. Painted layers are never
    /// touched, renamed, or moved. None = the result would pass the cap
    /// (checked BEFORE any id is allocated).
    fn plan_conform(
        &mut self,
        at: CutRef,
        drawing: DrawingId,
        existing: &[LayerProps],
        preset: &crate::config::StackPreset,
    ) -> Option<Vec<Command>> {
        use anim_core::model::LayerRole;
        let mut sim: Vec<LayerProps> = existing.to_vec();
        let missing: Vec<LayerProps> = preset
            .layers
            .iter()
            .filter(|spec| {
                if spec.role == LayerRole::Custom {
                    !existing.iter().any(|p| p.name == spec.name)
                } else {
                    !existing.iter().any(|p| p.role == spec.role)
                }
            })
            .cloned()
            .collect();
        if sim.len() + missing.len() > 8 {
            return None;
        }
        let mut cmds = Vec::with_capacity(missing.len());
        for spec in missing {
            let idx = {
                let refs: Vec<&LayerProps> = sim.iter().collect();
                Self::rank_insert_index(&refs, spec.role, sim.len())
            };
            let layer = CelLayer {
                id: self.engine.alloc_layer_id(),
                props: spec.clone(),
                raster: Default::default(),
            };
            sim.insert(idx, spec);
            cmds.push(Command::AddCelLayer {
                at,
                drawing,
                index: idx,
                layer,
            });
        }
        Some(cmds)
    }

    fn stack_named(&self, name: &str) -> Option<crate::config::StackPreset> {
        self.stacks.iter().find(|s| s.name == name).cloned()
    }

    /// Give THIS cel the layers of `preset` it lacks — one undo step.
    pub fn apply_stack_to_cel(&mut self, preset_name: &str) {
        let Some(preset) = self.stack_named(preset_name) else {
            return;
        };
        let Some(id) = self.own_drawing_id() else {
            self.refuse("refused — no cel on this frame (draw, or press E)");
            return;
        };
        let existing: Vec<LayerProps> = match self.cut().drawing(id) {
            Some(d) if !d.layers.is_empty() => d.layers.iter().map(|l| l.props.clone()).collect(),
            _ => {
                self.refuse("refused — this cel has no layer stack (paint on it first)");
                return;
            }
        };
        let at = self.at();
        match self.plan_conform(at, id, &existing, &preset) {
            None => self.refuse(format!(
                "refused — '{}' would take this cel past 8 layers",
                preset.name
            )),
            Some(cmds) if cmds.is_empty() => {
                self.status = format!("this cel already has every '{}' layer", preset.name);
            }
            Some(cmds) => {
                let n = cmds.len();
                let r = self.engine.apply("apply layer stack", cmds);
                self.report(r, &format!("added {n} layer(s) from '{}'", preset.name));
            }
        }
    }

    /// Conform EVERY layered cel in this cut to `preset` (additive) — one
    /// undo step for the whole cut. Cels the preset would push past the
    /// cap are skipped, and counted in the status.
    pub fn conform_cut(&mut self, preset_name: &str) {
        let Some(preset) = self.stack_named(preset_name) else {
            return;
        };
        let at = self.at();
        let cels: Vec<(DrawingId, Vec<LayerProps>)> = self
            .cut()
            .drawings
            .iter()
            .filter(|d| !d.layers.is_empty())
            .map(|d| (d.id, d.layers.iter().map(|l| l.props.clone()).collect()))
            .collect();
        let mut cmds = Vec::new();
        let (mut touched, mut skipped) = (0usize, 0usize);
        for (id, existing) in cels {
            match self.plan_conform(at, id, &existing, &preset) {
                None => skipped += 1,
                Some(c) if c.is_empty() => {}
                Some(c) => {
                    touched += 1;
                    cmds.extend(c);
                }
            }
        }
        if cmds.is_empty() {
            self.status = if skipped > 0 {
                format!("nothing added — {skipped} cel(s) would pass 8 layers")
            } else {
                format!("every cel already has the '{}' layers", preset.name)
            };
            return;
        }
        let r = self.engine.apply("conform cut", cmds);
        let msg = if skipped > 0 {
            format!("'{}' → {touched} cel(s) conformed, {skipped} skipped (cap 8)", preset.name)
        } else {
            format!("'{}' → {touched} cel(s) conformed", preset.name)
        };
        self.report(r, &msg);
    }

    /// This cel's STRUCTURE as a stack preset (never pixels).
    pub fn current_stack_as_preset(&self, name: &str) -> Option<crate::config::StackPreset> {
        let id = self.own_key_drawing()?;
        let d = self.cut().drawing(id)?;
        if d.layers.is_empty() {
            return None;
        }
        let active = self
            .active_layer_of(d)
            .map(|l| l.props.name.clone())
            .unwrap_or_else(|| d.layers.last().unwrap().props.name.clone());
        Some(crate::config::StackPreset {
            name: name.to_string(),
            layers: d.layers.iter().map(|l| l.props.clone()).collect(),
            active,
        })
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
                && Some(d) != cur
            {
                prev = Some(d);
                break;
            }
        }
        let mut next = None;
        for f in (self.view.frame + 1)..self.frame_count() {
            if let Some(d) = col.resolve(f)
                && Some(d) != cur
                && Some(d) != prev
            {
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
    /// Composite of `id`'s visible layers, optionally restricted to the
    /// layer the pen is armed on — matched by NAME or by its non-custom
    /// ROLE (PSD-cel-layers: a neighbour whose line layer is called
    /// "clean" still ghosts under "line-only"). A neighbour with no such
    /// layer contributes nothing, never a wrong-role ghost.
    /// `ghost` = this composite feeds an ONION ghost: layers whose
    /// definition opts out of ghosting (`onion: false`) are skipped. Real
    /// displays (multi-column view, the flip) pass false — the flag is a
    /// ghosting choice, never a visibility one.
    pub fn drawing_composite(
        &self,
        id: DrawingId,
        filter: Option<(&str, anim_core::model::LayerRole)>,
        ghost: bool,
    ) -> Option<(LayerSlices<'_>, u64)> {
        use anim_core::model::LayerRole;
        let d = self.cut().drawing(id)?;
        let mut bytes: Vec<u8> = Vec::new();
        let mut slices = Vec::new();
        for l in &d.layers {
            let matches = filter.is_none_or(|(n, r)| {
                l.props.name == n || (r != LayerRole::Custom && l.props.role == r)
            });
            if l.props.visible
                && l.props.opacity > 0.0
                && (!ghost || l.props.onion)
                && matches
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
        self.drawing_composite(id, None, false)
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
        let ai = d
            .layer_index(active.id)
            .expect("active layer is in the stack");
        d.layers
            .iter()
            .enumerate()
            .filter(|(i, l)| {
                l.props.visible && l.props.opacity > 0.0 && if above { *i > ai } else { *i < ai }
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
            self.refuse("refused — this frame already has a drawing");
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
            self.refuse("refused — pick a drawing in the library first");
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
            && Some(o) != new
        {
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
    /// Remove a drawing from the library (owner, 2026-08-17: the library
    /// had no removal). Lifts every key that exposes it first, so the
    /// sheet never points at a ghost — all in one undoable step.
    pub fn remove_drawing_from_library(&mut self, id: DrawingId) {
        let at = self.at();
        let mut cmds = Vec::new();
        {
            let cut = self.cut();
            for col in &cut.xsheet.columns {
                for (frame, e) in col.keys() {
                    if matches!(e, Exposure::Drawing(d) if
        d
        == id)
                    {
                        cmds.push(Command::SetCell {
                            at,
                            column: col.id,
                            frame,
                            key: None,
                        });
                    }
                }
            }
        }
        cmds.push(Command::RemoveDrawing { at, id });
        let r = self.engine.apply("remove drawing", cmds);
        if self.view.selected_drawing == Some(id) {
            self.view.selected_drawing = None;
        }
        if self.view.ghost_pin == Some(id) {
            self.view.ghost_pin = None;
        }
        self.report(r, "drawing removed from library");
    }
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
        let name = format!(
            "{}",
            (b'A' + (self.cut().xsheet.columns.len() as u8 % 26)) as char
        );
        match self.engine.add_column(at, name) {
            Ok(id) => {
                self.view.active_column = id;
                self.meta_dirty = true;
                self.status = "column added".into();
            }
            Err(e) => self.refuse(format!("could not add a column — {e}")),
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
            vec![Command::SetCutAudio {
                at,
                audio: Some(clip),
            }],
        ) {
            self.status = format!("error: {e}");
        } else {
            self.status = format!("audio: {cname} ({secs:.1}s)");
        }
    }

    /// Remove the cut's scratch audio (one undo step).
    pub fn remove_audio(&mut self) {
        let at = self.at();
        if let Err(e) = self.engine.apply(
            "remove audio",
            vec![Command::SetCutAudio { at, audio: None }],
        ) {
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
        self.meta_dirty = true;
        let scene = self.view.scene;
        let cut_id = match self
            .engine
            .add_cut(scene, sheet.name.clone(), sheet.duration)
        {
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
        // order), each with the CHOSEN stack preset; then the keys.
        // ONE batch = one undo step for the whole timing import.
        let template = if self.new_cel_template.is_empty() {
            solo_template()
        } else {
            self.new_cel_template.clone()
        };
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
            let layers: Vec<CelLayer> = template
                .iter()
                .map(|props| CelLayer {
                    id: self.engine.alloc_layer_id(),
                    props: props.clone(),
                    raster: Default::default(),
                })
                .collect();
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
        self.meta_dirty = true;
        match self.engine.rename_cut(at, name) {
            Ok(()) => self.status = format!("cut renamed to {name}"),
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// Rename the scene being edited (scaffolding).
    pub fn rename_current_scene(&mut self, name: &str) {
        self.meta_dirty = true;
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
                self.meta_dirty = true;
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
        let label = if key.is_some() {
            "set param key"
        } else {
            "clear param key"
        };
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
            self.refuse("refused — a cut keeps at least one column");
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
        self.meta_dirty = true;
        self.view.active_column = new_active;
        self.view.selected_drawing = None;
        self.status = "column removed".into();
    }
    pub fn undo(&mut self) {
        if self.engine.undo().is_ok() {
            self.status = "undo".into();
            self.doc_gen = self.doc_gen.wrapping_add(1);
            self.history_moved();
        }
        self.sanitize();
    }
    pub fn redo(&mut self) {
        if self.engine.redo().is_ok() {
            self.status = "redo".into();
            self.doc_gen = self.doc_gen.wrapping_add(1);
            self.history_moved();
        }
        self.sanitize();
    }

    /// Selection can dangle after undo of a creation command — never let a
    /// dead id escape into the UI.
    fn sanitize(&mut self) {
        if let Some(id) = self.view.selected_drawing
            && self.cut().drawing(id).is_none()
        {
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
            Ok(()) => {
                self.status = ok_msg.into();
                self.doc_gen = self.doc_gen.wrapping_add(1);
            }
            // AUDIT [14]: ~30 document commands report through here, and
            // a failure was a grey remark prefixed "error:" — a category
            // name rather than a statement of what did not happen.
            Err(e) => self.refuse(format!("refused — {ok_msg} did not happen ({e})")),
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
        // Starting playback parked on the LAST frame (exactly where a
        // non-loop run stops) rewinds to the top first — otherwise the
        // first tick would hit the end condition and stop again instantly,
        // making the play press feel dead (and silently disarming the
        // playback the user thought they started).
        if self.view.playing && self.view.frame + 1 >= self.frame_count() {
            self.view.frame = 0;
        }
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
            self.refuse("refused — that cut has no columns yet (add one first)");
            return;
        };
        self.view.scene = scene;
        self.view.cut = cut;
        self.view.active_column = column;
        self.view.frame = 0;
        self.view.selected_drawing = None;
        self.view.playing = false;
        // A fresh cut arms the template's own target (name + role).
        let (n, r) = self.new_cel_active.clone();
        self.view.active_layer = n;
        self.view.active_role = r;
        self.status = format!("cut: {name}");
    }

    /// Add a new scene with a first cut and column, and switch to it — the
    /// same boot New Project performs, minus the whole-project reset.
    /// Auto-named on the project's own SC../CUT.. convention (a rename UI
    /// is a natural fast-follow; the name field already supports it).
    pub fn new_scene(&mut self) {
        let n = self.engine.project.scenes.len() + 1;
        self.meta_dirty = true;
        let scene = self.engine.add_scene(format!("SC{n:02}"));
        let Ok(cut) = self.engine.add_cut(scene, "CUT01", 48) else {
            self.refuse("refused — the scene could not be created");
            return;
        };
        if let Err(e) = self.engine.add_column(CutRef { scene, cut }, "A") {
            self.refuse(format!("refused — the scene could not be created ({e})"));
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
        self.meta_dirty = true;
        let Ok(cut) = self.engine.add_cut(scene, format!("CUT{n:02}"), 48) else {
            self.refuse("refused — the cut could not be created");
            return;
        };
        if let Err(e) = self.engine.add_column(CutRef { scene, cut }, "A") {
            self.refuse(format!("refused — the cut could not be created ({e})"));
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
                Command::AddImage {
                    at,
                    id: image,
                    index,
                    name,
                    bytes,
                },
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
        let r = self.engine.apply(
            "node parameters",
            vec![Command::SetNodeKind { at, id, kind }],
        );
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

    /// SESSION v2 (research/PSD-session-v2.md): apply a guest's tile
    /// patch to the drawing they named, on the layer they named — the
    /// HOST's engine is the only writer, and the step is tagged with its
    /// author so that artist can undo their own work. Returns Err(why)
    /// when the identity is gone; the caller sends that home (NEVER-DO 2).
    pub fn apply_remote_tiles(
        &mut self,
        author: &str,
        drawing: DrawingId,
        layer_name: &str,
        tiles: Vec<(TileCoord, Option<Arc<TileData>>)>,
    ) -> Result<(), String> {
        let at = self.at();
        let Some(d) = self.cut().drawing(drawing) else {
            return Err(format!("drawing {drawing} is not in this cut any more"));
        };
        let Some(layer) = d
            .layers
            .iter()
            .find(|l| l.props.name == layer_name)
            .map(|l| l.id)
        else {
            return Err(format!("no '{layer_name}' layer on that cel"));
        };
        // The before-state comes from OUR document — the guest's patch is
        // only the after-state, so undo stays exact on the host's truth.
        let before: std::collections::BTreeMap<TileCoord, Arc<TileData>> = d
            .layers
            .iter()
            .find(|l| l.id == layer)
            .map(|l| l.raster.tiles.clone())
            .unwrap_or_default();
        let diff: TileDiff = tiles
            .into_iter()
            .map(|(c, t)| (c, before.get(&c).cloned(), t))
            .filter(|(_, b, a)| !(b.is_none() && a.is_none()))
            .collect();
        if diff.is_empty() {
            return Ok(());
        }
        let n = diff.len();
        // STAGE 3: restore the session's BASE author (the local artist's
        // name), never bare None — every entry carries its hand.
        let prev = self.engine.author();
        self.engine.set_author(Some(author.to_string()));
        let r = self.engine.apply(
            "remote stroke",
            vec![Command::PaintTiles {
                at,
                id: drawing,
                layer,
                diff,
            }],
        );
        self.engine.set_author(prev);
        match r {
            Ok(()) => {
                self.status = format!("{author} painted ({n} tile(s))");
                self.doc_gen = self.doc_gen.wrapping_add(1);
                Ok(())
            }
            Err(e) => Err(format!("{e:?}")),
        }
    }

    /// v0.2.1 (audit): locate a drawing's layer ANYWHERE in the project
    /// — replay, audit and repair must not depend on which cut the
    /// VIEWER happens to be browsing.
    pub fn locate_layer(
        &self,
        drawing: DrawingId,
        layer_name: &str,
    ) -> Option<&anim_core::model::CelLayer> {
        self.engine
            .project
            .scenes
            .iter()
            .flat_map(|s| s.cuts.iter())
            .find_map(|c| c.drawings.iter().find(|d| d.id == drawing))
            .and_then(|d| d.layers.iter().find(|l| l.props.name == layer_name))
    }

    /// v0.2.1 (LAW 1): incoming command batches carry PaintTiles with
    /// the BEFORE slot stripped (pixels never ride the wire twice) —
    /// rebuild befores from THIS document, so the history inverse stays
    /// exact. A drawing created earlier in the same batch is absent
    /// here, and its befores are correctly None (a fresh layer).
    pub fn rebuild_paint_befores(&self, cmds: &mut [Command]) {
        for cmd in cmds {
            if let Command::PaintTiles { id, layer, diff, .. } = cmd {
                let tiles = self
                    .engine
                    .project
                    .scenes
                    .iter()
                    .flat_map(|s| s.cuts.iter())
                    .find_map(|c| c.drawings.iter().find(|d| d.id == *id))
                    .and_then(|d| d.layers.iter().find(|l| l.id == *layer))
                    .map(|l| &l.raster.tiles);
                if let Some(tiles) = tiles {
                    for (coord, before, _) in diff.iter_mut() {
                        *before = tiles.get(coord).cloned();
                    }
                }
            }
        }
    }

    /// STAGE 1 (PSD-multiplayer-rescope, the audit's repair): set exact
    /// tiles OUTSIDE history — the stroke's history entry stays as this
    /// machine replayed it; a later redo re-audits (NEVER-DO 3's loop).
    /// Empty texels remove the tile.
    pub fn apply_repair_tiles(
        &mut self,
        drawing: DrawingId,
        layer_name: &str,
        tiles: Vec<(i32, i32, Vec<u16>)>,
    ) {
        let layer = self
            .engine
            .project
            .scenes
            .iter_mut()
            .flat_map(|s| s.cuts.iter_mut())
            .find_map(|c| c.drawings.iter_mut().find(|d| d.id == drawing))
            .and_then(|d| d.layers.iter_mut().find(|l| l.props.name == layer_name));
        let Some(l) = layer else { return };
        for (x, y, t) in tiles {
            if t.is_empty() {
                l.raster.tiles.remove(&(x, y));
            } else if t.len() == anim_core::raster::TILE_LEN {
                l.raster
                    .tiles
                    .insert((x, y), Arc::new(TileData::from_vec(t)));
            }
        }
        self.doc_gen = self.doc_gen.wrapping_add(1);
    }

    /// SESSION v2: undo/redo one artist's own last step (the host runs it
    /// for everyone — one writer). Err(why) is sent home to that artist.
    pub fn remote_history(&mut self, author: &str, redo: bool) -> Result<(), String> {
        let r = if redo {
            self.engine.redo_last_by(Some(author))
        } else {
            self.engine.undo_last_by(Some(author))
        };
        match r {
            Ok(label) => {
                self.status = format!("{author}: {} {label}", if redo { "redid" } else { "undid" });
                self.doc_gen = self.doc_gen.wrapping_add(1);
                self.history_moved();
                self.sanitize();
                Ok(())
            }
            Err(e) => Err(format!("{e:?}")),
        }
    }

    /// SESSION PERF: everything a worker thread needs to build the
    /// snapshot OFF the UI thread — a clone of the project (Arc-tiled,
    /// cheap) with the palettes already folded in.
    pub fn snapshot_project(&mut self) -> anim_core::model::Project {
        self.palettes.save_into(&mut self.engine.project);
        self.engine.project.clone()
    }

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
        self.begin_save(path);
    }

    /// GM2: unsaved changes exist — the ● in the title and on the slate.
    /// A save still on its worker is not on disk yet, so it counts.
    pub fn is_dirty(&self) -> bool {
        self.meta_dirty
            || self.saving.as_ref().is_some_and(|j| j.meta_was_dirty)
            || self.engine.revision() != self.saved_rev
    }

    /// GM2: the palettes, for a write. They live outside undo, so a write
    /// through here marks the document dirty (the swatch board's DERIVE
    /// and the eye's SET; the palette pane marks its own field edits).
    pub fn palettes_mut(&mut self) -> &mut crate::palette::Palettes {
        self.meta_dirty = true;
        &mut self.palettes
    }

    /// GM2: write the document to `path` on a worker thread (NEVER-DO 6 —
    /// the pen never waits on the disk). The worker gets a snapshot: edits
    /// made after this call are not in the file and stay dirty. The write
    /// is atomic and keeps the previous version as `.bak`
    /// (`store::save_atomic_from`).
    pub fn begin_save(&mut self, path: PathBuf) {
        if let Some(job) = &mut self.saving {
            // Ctrl+S again while the disk is busy (muscle memory on a big
            // cut) is not a contradiction: one more save, from a fresh
            // snapshot, once this one lands.
            job.again = Some(path);
            self.status = "saving… (saving again when this lands)".into();
            return;
        }
        // Palettes are project data but outside the undo/Command system —
        // freshen the project's opaque meta right before the snapshot, the
        // same point every other "current in-memory state" gets synced.
        let project = self.snapshot_project();
        // Save As inherits the unknown tables of the file it came from.
        let template = self.file_path.clone();
        let rev = self.engine.revision();
        let meta_was_dirty = std::mem::take(&mut self.meta_dirty);
        let (tx, rx) = std::sync::mpsc::channel();
        let target = path.clone();
        std::thread::spawn(move || {
            let r = anim_core::store::save_atomic_from(
                &project,
                &target,
                template.as_deref(),
                true,
            );
            let _ = tx.send(r.map_err(|e| e.to_string()));
        });
        self.status = format!("saving {}…", path.display());
        self.saving = Some(SaveJob {
            rx,
            rev,
            path,
            meta_was_dirty,
            again: None,
        });
    }

    /// GM2: collect a finished background save (the shell calls this every
    /// frame). True when a save just landed.
    pub fn poll_save(&mut self) -> bool {
        let Some(job) = &self.saving else { return false };
        let r = match job.rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(_) => Err("the save worker stopped".into()),
        };
        self.finish_save(r)
    }

    /// GM2: block until the running save — and any save queued behind it —
    /// lands (tests; the shell polls). True when the last one landed.
    #[cfg(test)]
    pub fn wait_save(&mut self) -> bool {
        let mut landed = false;
        while let Some(job) = &self.saving {
            let r = job
                .rx
                .recv()
                .unwrap_or_else(|_| Err("the save worker stopped".into()));
            landed = self.finish_save(r);
        }
        landed
    }

    /// GM2 tests: a save that stays on the disk until the test lands it —
    /// send its result on the returned sender (no thread, no timing).
    #[cfg(test)]
    pub fn save_in_flight(&mut self, path: PathBuf) -> std::sync::mpsc::Sender<Result<(), String>> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.saving = Some(SaveJob {
            rx,
            rev: self.engine.revision(),
            path,
            meta_was_dirty: std::mem::take(&mut self.meta_dirty),
            again: None,
        });
        tx
    }

    fn finish_save(&mut self, r: Result<(), String>) -> bool {
        let Some(job) = self.saving.take() else { return false };
        let landed = match r {
            Ok(()) => {
                self.saved_rev = job.rev;
                self.status = format!("saved {}", job.path.display());
                self.file_path = Some(job.path);
                true
            }
            // AUDIT [2]: a save that did not happen is a CONTRADICTION,
            // not a remark — Aka lane + canvas edge flash, never a grey
            // line that fades in four seconds.
            Err(e) => {
                self.meta_dirty |= job.meta_was_dirty;
                self.refuse(format!("SAVE FAILED — {e}. Your work is NOT on disk."));
                false
            }
        };
        // The save asked for while this one ran. A repeat that would write
        // the same bytes to the same file is skipped — it would only turn
        // `.bak` into a copy of what was just saved.
        if let Some(path) = job.again
            && (!landed || self.is_dirty() || self.file_path.as_ref() != Some(&path))
        {
            self.begin_save(path);
        }
        landed
    }

    pub fn open(&mut self) {
        match Self::pick_and_open() {
            Some(Ok(new_state)) => *self = new_state,
            Some(Err(msg)) => self.refuse(format!("could not open that file — {msg}")),
            None => {}
        }
    }

    /// GM2: open a known path (the recent list).
    pub fn open_path(&mut self, path: PathBuf) {
        match Self::load_from(path) {
            Ok(new_state) => *self = new_state,
            Err(msg) => self.refuse(format!("could not open that file — {msg}")),
        }
    }
}

/// GM2: a save running on a worker thread — the UI never waits on the disk.
pub struct SaveJob {
    rx: std::sync::mpsc::Receiver<Result<(), String>>,
    /// The revision the snapshot was taken at.
    rev: u64,
    path: PathBuf,
    /// `meta_dirty` as it was when the snapshot was taken (restored if the
    /// save fails).
    meta_was_dirty: bool,
    /// A save asked for while this one ran (its path): started from a
    /// fresh snapshot when this one lands.
    again: Option<PathBuf>,
}

#[cfg(test)]
mod transport_tests {
    use super::AppState;
    fn state() -> AppState {
        AppState::new_project("t", 64, 48, 24, 72.0)
    }

    /// The dead-play-press bug: with loop off, playback stops PARKED on the
    /// last frame; pressing play there must rewind to the top, not flip
    /// `playing` on and have the first tick instantly stop it again.
    #[test]
    fn play_at_the_end_rewinds_instead_of_instantly_stopping() {
        let mut s = state();
        s.view.loop_playback = false;
        let last = s.frame_count() - 1;
        s.goto(last);
        s.toggle_play();
        assert!(s.view.playing);
        assert_eq!(s.view.frame, 0, "parked at the end, play rewinds first");
        // A full run then stops ON the last frame again (the hold-at-end law).
        for _ in 0..s.frame_count() * 2 {
            s.tick(1.0 / 24.0);
        }
        assert!(!s.view.playing);
        assert_eq!(s.view.frame, last);
    }

    /// Seek-then-play: a mid-cut playhead must be respected (the rewind
    /// applies ONLY when parked at the very end).
    #[test]
    fn click_seek_then_play_starts_from_the_clicked_frame() {
        let mut s = state();
        s.view.loop_playback = false;
        s.goto(10);
        s.toggle_play();
        assert_eq!(s.view.frame, 10, "mid-cut play does not rewind");
        s.tick(1.0 / 24.0);
        assert_eq!(s.view.frame, 11);
    }

    /// Seeking DURING playback keeps playing from the new position.
    #[test]
    fn seek_while_playing_continues_from_there() {
        let mut s = state();
        s.toggle_play();
        s.tick(1.0 / 24.0);
        s.goto(20);
        assert!(s.view.playing);
        s.tick(1.0 / 24.0);
        assert_eq!(s.view.frame, 21);
    }
}

#[cfg(test)]
mod layer_role_tests {
    //! PSD-cel-layers Stage 2 pins: behaviour keys off ROLE, never name.
    use super::AppState;
    use anim_core::model::{LayerProps, LayerRole};
    use anim_core::raster::{TILE_LEN, TileData};
    use std::sync::Arc;

    fn state() -> AppState {
        AppState::new_project("t", 64, 48, 24, 72.0)
    }

    fn spec(role: LayerRole) -> LayerProps {
        LayerProps {
            name: role.default_name().into(),
            role,
            ..LayerProps::default()
        }
    }

    /// Paint one tile on the current (empty) frame: creates a template cel.
    fn make_cel(s: &mut AppState) {
        let slot = s.active_slot_resolved().expect("armed on the template");
        let tile = Arc::new(TileData::from_vec(vec![0x3C00; TILE_LEN]));
        s.commit_raster(vec![((0, 0), tile)], slot).expect("cel created");
    }

    fn stack(s: &AppState) -> Vec<(String, LayerRole)> {
        s.strip_layers()
            .iter()
            .rev()
            .map(|(_, l)| (l.props.name.clone(), l.props.role))
            .collect()
    }

    #[test]
    fn a_new_cel_gets_the_chosen_template_with_roles() {
        let mut s = state();
        s.new_cel_template = vec![
            spec(LayerRole::Color),
            spec(LayerRole::Shadow),
            spec(LayerRole::ShadowLine),
            spec(LayerRole::Line),
        ];
        s.view.active_layer = "shadow".into();
        s.view.active_role = LayerRole::Shadow;
        make_cel(&mut s);
        let got = stack(&s);
        assert_eq!(got.len(), 4);
        assert_eq!(got[1], ("shadow".into(), LayerRole::Shadow));
        assert_eq!(got[2].1, LayerRole::ShadowLine);
        // The stroke landed on the ARMED template layer (shadow), not a
        // hardcoded "line = top slot".
        let armed = s.active_layer_props().unwrap();
        assert_eq!(armed.role, LayerRole::Shadow);
    }

    #[test]
    fn a_renamed_layer_keeps_the_pen_armed_by_role() {
        let mut s = state();
        make_cel(&mut s); // solo template, armed on line
        let lid = s.active_layer_id().expect("line armed");
        let props = s.active_layer_props().unwrap().clone();
        // Rename WITHOUT re-arming (as a peer's edit would): the old name
        // is gone, the role still finds its layer (the premortem's first
        // failure — a rename stripping a layer of its job).
        s.layer_set_props(
            lid,
            LayerProps {
                name: "genga L".into(),
                ..props
            },
        );
        assert_eq!(s.active_layer_id(), Some(lid), "role fallback resolves");
        assert!(s.active_slot_resolved().is_some(), "the pen stays armed");
        // Ambiguity never guesses: a SECOND line-role layer unarms.
        s.layer_add_role(LayerRole::Line);
        s.view.active_layer = "gone".into();
        s.view.active_role = LayerRole::Line;
        assert_eq!(s.active_layer_id(), None, "two line layers: refuse to guess");
    }

    #[test]
    fn the_line_guard_skips_every_line_role_renamed_or_not() {
        let mut s = state();
        s.new_cel_template = vec![
            spec(LayerRole::Color),
            spec(LayerRole::ShadowLine),
            LayerProps {
                name: "clean".into(),
                role: LayerRole::Line,
                ..LayerProps::default()
            },
        ];
        s.view.active_layer = "color".into();
        s.view.active_role = LayerRole::Color;
        make_cel(&mut s);
        s.view.line_guard = true;
        for _ in 0..6 {
            s.cycle_layer(false);
            assert!(
                !s.active_layer_props().unwrap().role.is_line(),
                "guarded cycling never arms a line role"
            );
        }
    }

    #[test]
    fn layers_insert_at_their_role_rank() {
        let mut s = state();
        make_cel(&mut s); // [color, line]
        s.layer_add_role(LayerRole::Correction);
        s.layer_add_role(LayerRole::Shadow);
        s.layer_add_role(LayerRole::ShadowLine);
        let roles: Vec<LayerRole> = stack(&s).into_iter().map(|(_, r)| r).collect();
        assert_eq!(
            roles,
            vec![
                LayerRole::Color,
                LayerRole::Shadow,
                LayerRole::ShadowLine,
                LayerRole::Line,
                LayerRole::Correction
            ],
            "pipeline order by ROLE rank"
        );
    }

    fn with_stacks(s: &mut AppState) {
        s.stacks = crate::config::builtin_stacks();
    }

    #[test]
    fn applying_a_stack_only_adds_and_never_touches_pixels() {
        // NEVER-DO 2: the Shiage stack onto a painted [color, line] cel.
        let mut s = state();
        with_stacks(&mut s);
        make_cel(&mut s); // paints the armed line layer
        let before: Vec<(anim_core::ids::LayerId, u64)> = s
            .strip_layers()
            .iter()
            .map(|(_, l)| (l.id, l.raster.content_hash()))
            .collect();
        let doc_before = s.engine.project.clone();
        s.apply_stack_to_cel("Shiage (full)");
        let roles: Vec<LayerRole> = stack(&s).into_iter().map(|(_, r)| r).collect();
        assert_eq!(
            roles,
            vec![
                LayerRole::Color,
                LayerRole::Shadow,
                LayerRole::Highlight,
                LayerRole::ShadowLine,
                LayerRole::HighlightLine,
                LayerRole::Line,
                LayerRole::Correction
            ]
        );
        // Every pre-existing layer: same id, same pixels.
        for (id, h) in &before {
            let l = s
                .strip_layers()
                .into_iter()
                .find(|(_, l)| l.id == *id)
                .map(|(_, l)| l.raster.content_hash());
            assert_eq!(l, Some(*h), "painted layers untouched");
        }
        // One undo step restores the document exactly.
        s.undo();
        assert_eq!(s.engine.project.scenes, doc_before.scenes, "one step, exact");
    }

    #[test]
    fn a_stack_that_would_pass_the_cap_is_refused_whole() {
        let mut s = state();
        with_stacks(&mut s);
        make_cel(&mut s); // 2 layers
        for _ in 0..5 {
            s.layer_add_role(LayerRole::Custom); // → 7 layers
        }
        let snapshot = s.engine.project.scenes.clone();
        s.apply_stack_to_cel("Shiage (full)"); // needs 5 more → 12 > 8
        assert_eq!(s.engine.project.scenes, snapshot, "atomic refusal");
        assert!(s.refusal.contains("8 layers"));
    }

    #[test]
    fn conform_cut_is_one_undo_step_across_every_cel() {
        let mut s = state();
        with_stacks(&mut s);
        make_cel(&mut s);
        s.goto(3);
        make_cel(&mut s);
        let snapshot = s.engine.project.scenes.clone();
        s.conform_cut("Genga"); // adds rough + correction to both
        let cels: Vec<usize> = s.cut().drawings.iter().map(|d| d.layers.len()).collect();
        assert_eq!(cels, vec![4, 4], "both cels conformed");
        s.undo();
        assert_eq!(s.engine.project.scenes, snapshot, "one undo step for the cut");
    }

    #[test]
    fn a_locked_template_layer_refuses_on_a_blank_frame() {
        // The hole found while wiring Stage 4: no own cel yet, so the lock
        // must come from the TEMPLATE layer the stroke would create.
        let mut s = state();
        let shiage = crate::config::builtin_stacks()
            .into_iter()
            .find(|p| p.name == "Shiage (full)")
            .unwrap();
        s.new_cel_template = shiage.layers.clone();
        s.view.active_layer = "line".into();
        s.view.active_role = LayerRole::Line;
        assert!(s.own_key_drawing().is_none(), "blank frame");
        assert!(s.refuse_if_locked(), "the template's line is locked");
        s.view.active_layer = "color".into();
        s.view.active_role = LayerRole::Color;
        assert!(!s.refuse_if_locked(), "color is not");
    }

    #[test]
    fn the_builtin_stacks_are_sound() {
        let all = crate::config::builtin_stacks();
        assert!(all.iter().any(|s| s.name == "Solo merged"));
        for s in &all {
            assert!(s.layers.len() <= 8, "{}: within the cap", s.name);
            let ranks: Vec<u8> = s.layers.iter().filter_map(|l| l.role.rank()).collect();
            assert!(
                ranks.windows(2).all(|w| w[0] < w[1]),
                "{}: bottom→top in pipeline order",
                s.name
            );
            assert!(
                s.layers.iter().any(|l| l.name == s.active),
                "{}: its armed layer exists",
                s.name
            );
        }
        // A room bound to a deleted stack falls back, never strands.
        let mut cfg = crate::config::LayersConfig::default();
        cfg.room_stack.insert("drawing".into(), "gone".into());
        assert_eq!(cfg.stack_for_room("drawing").name, "Solo merged");
    }

    #[test]
    fn a_locked_layer_refuses_the_pen() {
        let mut s = state();
        make_cel(&mut s);
        let lid = s.active_layer_id().unwrap();
        let props = s.active_layer_props().unwrap().clone();
        assert!(!s.refuse_if_locked());
        s.layer_set_props(
            lid,
            LayerProps {
                locked: true,
                ..props
            },
        );
        assert!(s.refuse_if_locked(), "locked = refused");
        assert!(s.refusal.contains("locked"));
    }
}
