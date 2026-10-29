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
use anim_core::sheet_ops;
use anim_core::xsheet::{Column, ColumnDef, ColumnKind, Exposure, ParamColumn, ParamKey};

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

/// GM6: the spacing-chart presets (タイミング指示) — marks between the keys,
/// 0..1: even halves / thirds / quarters, or bunched toward one key; each
/// with the line its button says on hover.
pub const CHART_PRESETS: &[(&str, &[f32], &str)] = &[
    ("halves", &[0.5], "one inbetween, halfway between the keys"),
    ("thirds", &[1.0 / 3.0, 2.0 / 3.0], "two inbetweens, a third of the way apart"),
    ("quarters", &[0.25, 0.5, 0.75], "three inbetweens, evenly a quarter apart"),
    (
        "favour start",
        &[0.125, 0.25, 0.5],
        "three inbetweens halving toward this key (1/8, 1/4, 1/2): the motion leaves it slowly",
    ),
    (
        "favour end",
        &[0.5, 0.75, 0.875],
        "three inbetweens halving toward the next key (1/2, 3/4, 7/8): the motion arrives slowly",
    ),
];

/// GM6: the most marks the chart editor makes (inbetweens between two keys).
pub const CHART_MAX_MARKS: usize = 15;

/// GM6: how the chart editor lays `n` marks between the keys — evenly, or
/// closing up (tsume) toward one key on a quadratic ease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartSpacing {
    Even,
    /// mae-zume: the marks close up toward this key (a slow out).
    FavourStart,
    /// ato-zume: the marks close up toward the next key (a slow in).
    FavourEnd,
}

impl ChartSpacing {
    pub const ALL: [ChartSpacing; 3] = [
        ChartSpacing::Even,
        ChartSpacing::FavourStart,
        ChartSpacing::FavourEnd,
    ];

    /// The editor's button (D7: romaji plus English).
    pub fn label(self) -> &'static str {
        match self {
            ChartSpacing::Even => "even",
            ChartSpacing::FavourStart => "mae-zume (slow out)",
            ChartSpacing::FavourEnd => "ato-zume (slow in)",
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            ChartSpacing::Even => "the marks evenly apart between the keys",
            ChartSpacing::FavourStart => "the marks close up toward this key: the motion leaves it slowly",
            ChartSpacing::FavourEnd => "the marks close up toward the next key: the motion arrives slowly",
        }
    }
}

/// GM6: `n` chart marks (clamped to 1..=CHART_MAX_MARKS), ascending, each
/// strictly between the keys.
pub fn chart_marks(spacing: ChartSpacing, n: usize) -> Vec<f32> {
    let n = n.clamp(1, CHART_MAX_MARKS);
    (1..=n)
        .map(|i| {
            let t = i as f32 / (n + 1) as f32;
            match spacing {
                ChartSpacing::Even => t,
                ChartSpacing::FavourStart => t * t,
                ChartSpacing::FavourEnd => 1.0 - (1.0 - t) * (1.0 - t),
            }
        })
        .collect()
}

/// GM6: drag mark `i` of a chart to `t`: it stays strictly between its
/// neighbours (and the keys), so the marks keep their order.
pub fn move_chart_mark(marks: &[f32], i: usize, t: f32) -> Vec<f32> {
    let mut out = marks.to_vec();
    if i < out.len() && t.is_finite() {
        const GAP: f32 = 0.005;
        let lo = if i == 0 { 0.0 } else { out[i - 1] } + GAP;
        let hi = out.get(i + 1).copied().unwrap_or(1.0) - GAP;
        if lo <= hi {
            out[i] = t.clamp(lo, hi);
        }
    }
    out
}

/// GM6: where `n` inbetweens land between keys exposed at `fa` and `fb`:
/// the span split into n+1 equal steps, rounded, each at least one frame
/// after the last. None when the span is too short (or n is 0).
pub fn inbetween_frames(fa: u32, fb: u32, n: usize) -> Option<Vec<u32>> {
    let span = fb.checked_sub(fa)? as usize;
    if n == 0 || span < n + 1 {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    let mut prev = fa;
    for i in 1..=n {
        let step = ((i * span) as f64 / (n + 1) as f64).round() as u32;
        let f = (fa + step).max(prev + 1);
        out.push(f);
        prev = f;
    }
    Some(out)
}

/// A name's number: (text before its first digit run, the number, the
/// text after) — "D12A" is ("D", 12, "A").
fn split_number(name: &str) -> Option<(&str, u64, &str)> {
    let start = name.find(|c: char| c.is_ascii_digit())?;
    let len = name[start..]
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(name.len() - start);
    let n = name[start..start + len].parse().ok()?;
    Some((&name[..start], n, &name[start + len..]))
}

/// GM6: the names of `n` inbetweens from key `from` to key `to`: the free
/// numbers strictly between the keys' numbers when there are enough
/// (spread evenly when there are more), otherwise the from-key's name
/// plus a letter (`1a`, `1b`, …), never a name already `taken`.
pub fn inbetween_names(
    from: &str,
    to: &str,
    n: usize,
    taken: &HashSet<String>,
) -> Vec<String> {
    if let (Some((p, a, s)), Some((p2, b, s2))) = (split_number(from), split_number(to))
        && (p, s) == (p2, s2)
    {
        let (lo, hi) = (a.min(b), a.max(b));
        // The taken numbers strictly between, sorted: the free ones are
        // counted and picked from these, never listed (keys 1 and 99999999
        // must not build a hundred million names).
        let mut held: Vec<u64> = taken
            .iter()
            .filter_map(|t| {
                let (tp, k, ts) = split_number(t)?;
                ((tp, ts) == (p, s) && k > lo && k < hi && *t == format!("{p}{k}{s}")).then_some(k)
            })
            .collect();
        held.sort_unstable();
        held.dedup();
        let m = (hi - lo).saturating_sub(1) - held.len() as u64;
        if m >= n as u64 {
            // The j-th of n picks sits nearest (j+1)/(n+1) of the way along
            // the m free numbers (u128: no overflow for any key numbers).
            let (m, n1) = (m as u128, n as u128 + 1);
            let at = |j: usize| ((2 * (j as u128 + 1) * (m + 1) + n1) / (2 * n1) - 1) as u64;
            // The i-th free number: step over every taken one at or below.
            let free = |i: u64| {
                let mut k = lo + 1 + i;
                for h in &held {
                    if *h <= k {
                        k += 1;
                    } else {
                        break;
                    }
                }
                k
            };
            let mut picked: Vec<String> = (0..n).map(|j| format!("{p}{}{s}", free(at(j)))).collect();
            if a > b {
                picked.reverse();
            }
            return picked;
        }
    }
    let mut out = Vec::with_capacity(n);
    let mut k = 0usize;
    while out.len() < n {
        // a … z, then a1 … z1, and so on — never a taken name.
        let letter = (b'a' + (k % 26) as u8) as char;
        let round = k / 26;
        let name = if round == 0 {
            format!("{from}{letter}")
        } else {
            format!("{from}{letter}{round}")
        };
        if !taken.contains(&name) && !out.contains(&name) {
            out.push(name);
        }
        k += 1;
    }
    out
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
    /// THE LIGHT TABLE (GM6; generalises the ghost pin, room ratified
    /// 2026-08-17): drawings pinned to the light box — rendered in Ao on
    /// EVERY frame, with no sheet entry, no export, no document change.
    /// The first pin is the old ghost pin. Pure display; the pins and the
    /// shift & trace offsets die with the session (NEVER-DO 1 of its room).
    pub light: crate::canvas::LightTable,
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
    /// GM14: the sheet panel's range anchor, cell clipboard and field
    /// buffers — session-only view state.
    pub sheet_ui: crate::xsheet_panel::SheetUi,
    /// GM14: the curves pane's shown columns, key selection and view.
    pub curves: crate::curves_panel::CurvesState,
    /// GM6: the pixel clipboard — the last copied selection of a layer, at
    /// its paper position (session-only; the cell clipboard is sheet_ui's).
    pub pixel_clip: Option<anim_core::edit::ClipImage>,
    /// GM6: the text the canvas handed the system clipboard for that copy
    /// (None = not handed yet), so the paste key's text can tell our own
    /// pixels from text copied elsewhere since.
    pub pixel_clip_text: Option<String>,
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
                light: Default::default(),
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
            sheet_ui: Default::default(),
            curves: Default::default(),
            pixel_clip: None,
            pixel_clip_text: None,
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
                light: Default::default(),
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
            sheet_ui: Default::default(),
            curves: Default::default(),
            pixel_clip: None,
            pixel_clip_text: None,
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

    /// GM5: the exposure under the sheet cursor — its key frame and the
    /// next key's frame (None = it holds to the end). None before the
    /// active column's first key.
    pub fn exposure_here(&self) -> Option<(u32, Option<u32>)> {
        self.cut()
            .xsheet
            .column(self.view.active_column)?
            .exposure_at(self.view.frame)
    }

    /// GM5: the active column's definition (kind, hidden, locked).
    pub fn active_column_def(&self) -> ColumnDef {
        self.cut().column_def(self.view.active_column)
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
        // GM5: the column lock first — it covers every layer of the cel.
        if self.refuse_if_column_locked() {
            return true;
        }
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

    /// GM5: the column lock, checked at pen-down before any ink — the
    /// artist never draws a whole stroke the engine then drops (D7: the pen
    /// comes first). The engine's rule: the active column is locked, or this
    /// frame's own drawing is exposed by a locked column. True = refused
    /// (the refusal is already on the Aka lane).
    pub fn refuse_if_column_locked(&mut self) -> bool {
        let cut = self.cut();
        let active = self.view.active_column;
        let own = self.own_key_drawing();
        let locked = |id: ColumnId| cut.column_def(id).locked;
        let by_active = cut.xsheet.column(active).filter(|c| locked(c.id));
        let by_exposure = || {
            let d = own?;
            cut.xsheet
                .columns
                .iter()
                .find(|c| locked(c.id) && !c.keys_referencing(d).is_empty())
        };
        let Some(name) = by_active.or_else(by_exposure).map(|c| c.name.clone()) else {
            return false;
        };
        self.refuse(format!(
            "refused — column {name} is locked: unlock it in the sheet to draw"
        ));
        true
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

    /// GM3: clear only the pixels inside `poly` (the selection, paper
    /// space) on this frame's own cel's active layer — Delete with a
    /// selection, the other studios' habit. One undo step; like
    /// [`clear_current_raster`](Self::clear_current_raster), a no-op where
    /// there is nothing to clear.
    pub fn clear_current_region(&mut self, poly: &[(f32, f32)]) {
        let Some(id) = self.own_key_drawing() else {
            return;
        };
        let at = self.at();
        let Some(l) = self.cut().drawing(id).and_then(|d| self.active_layer_of(d)) else {
            return;
        };
        let Some(lift) = anim_core::edit::lift_region(&l.raster.tiles, poly) else {
            return;
        };
        let diff: TileDiff = lift
            .cleared
            .into_iter()
            .map(|(c, after)| (c, l.raster.tiles.get(&c).cloned(), after))
            .collect();
        let layer = l.id;
        let r = self.engine.apply(
            "clear selection",
            vec![Command::PaintTiles {
                at,
                id,
                layer,
                diff,
            }],
        );
        self.report(r, "erased inside the selection");
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

    // ---- GM6: genga to douga --------------------------------------------

    /// Mark a drawing's kind (Unset clears its chart too) — one undo step;
    /// a kind never changes pixels.
    pub fn set_drawing_kind(&mut self, id: DrawingId, kind: anim_core::model::DrawingKind) {
        let before = self.cut().drawing_def(id);
        let after = anim_core::model::DrawingDef {
            kind,
            chart: if kind == anim_core::model::DrawingKind::Unset {
                None
            } else {
                before.chart.clone()
            },
        };
        self.apply_drawing_def(id, before, after, &format!("mark {}", kind.label()));
    }

    /// Give a key a spacing chart toward the next key in its column (no
    /// marks = remove the chart). Marking the chart makes the drawing a key.
    pub fn set_chart(&mut self, id: DrawingId, marks: &[f32]) {
        use anim_core::model::{DrawingDef, DrawingKind, SpacingChart};
        let before = self.cut().drawing_def(id);
        let chart = if marks.is_empty() {
            None
        } else {
            let col = self.cut().xsheet.column(self.view.active_column);
            // From the exposure under the cursor when it shows this drawing
            // (a drawing exposed twice charts toward the key after the one
            // you are on), else from its first exposure.
            let here = col
                .filter(|c| c.resolve(self.view.frame) == Some(id))
                .and_then(|c| c.exposure_at(self.view.frame))
                .map(|(f, _)| f);
            let start = here.or_else(|| col.and_then(|c| c.keys_referencing(id).first().copied()));
            match col.zip(start).and_then(|(c, f)| c.next_key(f, |d| d != id)) {
                Some((_, to)) => Some(SpacingChart {
                    to,
                    marks: marks.to_vec(),
                }),
                None => {
                    self.refuse("refused — a chart points at the NEXT key; expose one after this one");
                    return;
                }
            }
        };
        let after = DrawingDef {
            kind: DrawingKind::Key,
            chart,
        };
        self.apply_drawing_def(id, before, after, "spacing chart");
    }

    fn apply_drawing_def(
        &mut self,
        id: DrawingId,
        before: anim_core::model::DrawingDef,
        after: anim_core::model::DrawingDef,
        label: &str,
    ) {
        if before == after {
            return;
        }
        let at = self.at();
        let r = self.engine.apply(
            label,
            vec![Command::SetDrawingDef {
                at,
                drawing: id,
                before,
                after,
            }],
        );
        self.report(r, label);
    }

    /// Make inbetweens: the chart on this frame's key becomes blank
    /// Inbetween drawings (named between the keys, the from-key's layer
    /// stack, structure only) exposed between the two keys' exposure
    /// starts — keys, defs and cells in ONE undo step. False = refused.
    pub fn make_inbetweens(&mut self) -> bool {
        match self.inbetween_commands() {
            Ok(cmds) => {
                let n = cmds
                    .iter()
                    .filter(|c| matches!(c, Command::AddDrawing { .. }))
                    .count();
                let r = self.engine.apply("make inbetweens", cmds);
                let ok = r.is_ok();
                self.report(r, &format!("{n} inbetween(s) made"));
                ok
            }
            Err(why) => {
                self.refuse(format!("refused — {why}"));
                false
            }
        }
    }

    /// The commands `make_inbetweens` applies (D1: one batch).
    fn inbetween_commands(&mut self) -> Result<Vec<Command>, String> {
        use anim_core::model::{DrawingDef, DrawingKind};
        let at = self.at();
        let column = self.view.active_column;
        let from = self.current_drawing().ok_or("no drawing on this frame")?;
        let chart = self
            .cut()
            .drawing_def(from)
            .chart
            .ok_or("this drawing has no spacing chart")?;
        let col = self.cut().xsheet.column(column).ok_or("no active column")?;
        let (fa, _) = col.exposure_at(self.view.frame).ok_or("nothing exposed here")?;
        let name_of = |d: DrawingId| self.cut().drawing(d).map(|x| x.name.clone()).unwrap_or_default();
        // The very next key must be the chart's: whatever else sits between
        // the two (a breakdown, a key, earlier inbetweens, a blank) is the
        // animator's work and is never cleared — it refuses instead.
        let lost = "the chart's next key is not exposed after this one in this column";
        let (fb, next) = col.keys().find(|(f, _)| *f > fa).ok_or(lost)?;
        match next {
            Exposure::Drawing(d) if d == chart.to => {}
            Exposure::Drawing(d) => {
                if col.next_key(fa, |k| k == chart.to).is_none() {
                    return Err(lost.into());
                }
                let at = fb + 1;
                return Err(match self.cut().drawing_kind(d) {
                    DrawingKind::Inbetween => format!(
                        "inbetween {} is already timed between these keys (frame {at}); undo or clear its cells first",
                        name_of(d)
                    ),
                    kind => format!(
                        "{} {} sits between these keys (frame {at}); chart {} toward it instead",
                        match kind {
                            DrawingKind::Breakdown => "breakdown",
                            DrawingKind::Key => "key",
                            _ => "drawing",
                        },
                        name_of(d),
                        name_of(from),
                    ),
                });
            }
            Exposure::Empty => {
                if col.next_key(fa, |k| k == chart.to).is_none() {
                    return Err(lost.into());
                }
                return Err(format!(
                    "a blank cell sits between these keys (frame {}); clear it first",
                    fb + 1
                ));
            }
        }
        let n = chart.marks.len();
        let frames = inbetween_frames(fa, fb, n)
            .ok_or_else(|| format!("{n} inbetweens need at least {} frames between the keys", n + 1))?;
        let taken: HashSet<String> = self.cut().drawings.iter().map(|d| d.name.clone()).collect();
        let names = inbetween_names(&name_of(from), &name_of(chart.to), n, &taken);
        let specs: Vec<LayerProps> = match self.cut().drawing(from) {
            Some(d) if !d.layers.is_empty() => d.layers.iter().map(|l| l.props.clone()).collect(),
            _ => self.new_cel_specs(),
        };
        let mut cmds: Vec<Command> = Vec::new();
        let mut index = self.cut().drawings.len();
        for (name, frame) in names.into_iter().zip(frames) {
            let id = self.engine.alloc_drawing_id();
            let layers = specs
                .iter()
                .map(|props| CelLayer {
                    id: self.engine.alloc_layer_id(),
                    props: props.clone(),
                    raster: Default::default(),
                })
                .collect();
            cmds.push(Command::AddDrawing {
                at,
                id,
                index,
                name,
                strokes: vec![],
                layers,
            });
            cmds.push(Command::SetDrawingDef {
                at,
                drawing: id,
                before: DrawingDef::default(),
                after: DrawingDef {
                    kind: DrawingKind::Inbetween,
                    chart: None,
                },
            });
            cmds.push(Command::SetCell {
                at,
                column,
                frame,
                key: Some(Exposure::Drawing(id)),
            });
            index += 1;
        }
        Ok(cmds)
    }

    /// Duplicate cel: this frame's drawing — strokes and its whole layer
    /// stack, pixels shared — under the next free number, exposed here in
    /// place of the original. One undo step.
    pub fn duplicate_cel(&mut self) {
        let at = self.at();
        let Some(src) = self.current_drawing().and_then(|id| self.cut().drawing(id)).cloned() else {
            self.refuse("refused — no drawing on this frame to duplicate");
            return;
        };
        let name = self.next_drawing_name();
        let id = self.engine.alloc_drawing_id();
        let layers = src
            .layers
            .iter()
            .map(|l| CelLayer {
                id: self.engine.alloc_layer_id(),
                props: l.props.clone(),
                raster: l.raster.clone(),
            })
            .collect();
        let index = self.cut().drawings.len();
        let mut cmds = vec![Command::AddDrawing {
            at,
            id,
            index,
            name: name.clone(),
            strokes: src.strokes.clone(),
            layers,
        }];
        // The copy keeps what the original is (its kind and chart).
        let def = self.cut().drawing_def(src.id);
        if def != anim_core::model::DrawingDef::default() {
            cmds.push(Command::SetDrawingDef {
                at,
                drawing: id,
                before: Default::default(),
                after: def,
            });
        }
        cmds.push(Command::SetCell {
            at,
            column: self.view.active_column,
            frame: self.view.frame,
            key: Some(Exposure::Drawing(id)),
        });
        let r = self.engine.apply("duplicate cel", cmds);
        if r.is_ok() {
            self.view.selected_drawing = Some(id);
        }
        self.report(r, &format!("{} duplicated as {name}", src.name));
    }

    /// The pixel clipboard: copy the selected pixels of the armed layer of
    /// the drawing shown here. False = nothing copied (refused).
    pub fn copy_pixels(&mut self, poly: &[(f32, f32)]) -> bool {
        let clip = self.current_drawing().and_then(|id| {
            let d = self.cut().drawing(id)?;
            let layer = self.active_layer_of(d)?;
            anim_core::edit::copy_region(d, layer.id, poly)
        });
        match clip {
            Some(c) => {
                self.pixel_clip = Some(c);
                // The canvas hands the system clipboard fresh text for it.
                self.pixel_clip_text = None;
                self.status = "pixels copied".into();
                true
            }
            None => {
                self.refuse("refused — no ink in the selection on the armed layer");
                false
            }
        }
    }

    /// Paste the copied pixels in place (the same paper position) on the
    /// armed layer of this frame's own cel — a held or blank frame gets a
    /// new cel first. One undo step.
    pub fn paste_pixels(&mut self) {
        let Some(clip) = self.pixel_clip.clone() else {
            self.refuse("refused — copy pixels first");
            return;
        };
        if self.refuse_if_locked() {
            return;
        }
        let at = self.at();
        let target = self
            .own_key_drawing()
            .and_then(|id| self.cut().drawing(id))
            .filter(|d| !d.layers.is_empty())
            .and_then(|d| Some((d.id, self.active_layer_of(d)?.id)));
        if let Some((id, layer)) = target {
            let cmds = {
                let d = self.cut().drawing(id).expect("checked above");
                anim_core::edit::paste_region(at, d, layer, &clip)
            };
            // Through the region edit's own commit (one PaintTiles).
            for c in cmds {
                if let Command::PaintTiles { id, layer, diff, .. } = c {
                    self.commit_region_edit("paste pixels", id, layer, diff);
                }
            }
            return;
        }
        if self.own_key_drawing().is_some_and(|id| {
            self.cut().drawing(id).is_some_and(|d| !d.layers.is_empty())
        }) {
            self.refuse("refused — no armed layer on this cel to paste into");
            return;
        }
        // A held / blank frame (or a vector-only cel): the paste becomes
        // the new cel's paint, as a stroke would.
        let Some(slot) = self.active_slot_resolved() else {
            self.refuse("refused — no armed layer to paste into");
            return;
        };
        let tiles: Vec<(TileCoord, Arc<TileData>)> =
            anim_core::edit::merge_patch(&BTreeMap::new(), &[], &clip.patch)
                .into_iter()
                .filter_map(|(c, _, after)| after.map(|t| (c, t)))
                .collect();
        self.commit_raster(tiles, slot);
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
        self.view.light.unpin(id);
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
    /// Add a drawing column at the end of the sheet and make it active —
    /// one undo step (GM5: Command::AddColumn).
    pub fn add_column(&mut self) {
        let at = self.at();
        let index = self.cut().xsheet.columns.len();
        let name = format!("{}", (b'A' + (index as u8 % 26)) as char);
        let id = self.engine.alloc_column_id();
        let r = self.engine.apply(
            "add column",
            vec![Command::AddColumn {
                at,
                index,
                column: Column::new(id, name),
                def: ColumnDef::default(),
            }],
        );
        if r.is_ok() {
            self.view.active_column = id;
        }
        self.report(r, "column added");
    }

    // ---- GM5: the sheet verbs, on the sheet cursor ------------------------
    // The current cell (active column, current frame), or the active column
    // from the cursor down. Each is one undo step; a refusal says why on
    // the Aka lane (a locked column, nothing to hold, the last frame).

    /// Apply a sheet_ops builder's batch as one undo step. `done` is the
    /// remark when it lands, `nothing` the remark when there is nothing to
    /// change (the column is already on twos, say).
    fn apply_sheet(
        &mut self,
        label: &str,
        built: anim_core::error::Result<Vec<Command>>,
        done: &str,
        nothing: &str,
    ) {
        match built {
            Ok(cmds) if cmds.is_empty() => self.status = nothing.into(),
            Ok(cmds) => {
                let r = self.engine.apply(label, cmds);
                self.report(r, done);
            }
            Err(e) => self.report(Err(e), done),
        }
    }

    /// The active column's name, for the sheet verbs' remarks.
    fn active_column_name(&self) -> String {
        self.cut()
            .xsheet
            .column(self.view.active_column)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }

    /// Insert one frame at the cursor across the whole cut (D4): every
    /// column's keys, the param keys and the memos from here on move one
    /// frame later, and the cut grows by one.
    pub fn insert_frame(&mut self) {
        let at = self.at();
        let frame = self.view.frame;
        let r = self.engine.apply(
            "insert frame",
            vec![Command::InsertFrames {
                at,
                frame,
                count: 1,
            }],
        );
        self.report(r, &format!("frame inserted at {}", frame + 1));
    }

    /// Delete the cursor's frame across the whole cut (D4); what the next
    /// frame showed stays shown. A cut keeps at least one frame.
    pub fn delete_frame(&mut self) {
        let at = self.at();
        let frame = self.view.frame;
        let r = self.engine.apply(
            "delete frame",
            vec![Command::DeleteFrames {
                at,
                frame,
                count: 1,
            }],
        );
        if r.is_ok() {
            self.view.frame = frame.min(self.frame_count() - 1);
        }
        self.report(r, &format!("frame {} deleted", frame + 1));
    }

    /// Hold ±1 on the exposure under the cursor, rippling the column; −1 on
    /// a one-frame exposure removes it.
    pub fn hold(&mut self, delta: i32) {
        let built = sheet_ops::hold_delta(
            &self.engine.project,
            self.at(),
            self.view.active_column,
            self.view.frame,
            delta,
        );
        let label = if delta > 0 { "hold +1" } else { "hold -1" };
        self.apply_sheet(label, built, label, "nothing to hold here");
    }

    /// Retime the active column from the cursor down to `step`s (on ones,
    /// twos or threes): each drawing holds `step` frames, a kara run keeps
    /// its length, the cursor's frame stays put and what follows ripples.
    pub fn retime(&mut self, step: u32) {
        // GM14: a selected range of cells retimes just that range.
        let (columns, from, to) = self
            .sheet_range()
            .unwrap_or_else(|| (vec![self.view.active_column], self.view.frame, self.frame_count()));
        let built =
            sheet_ops::retime_range(&self.engine.project, self.at(), &columns, from, to, step);
        let nothing = format!("already on {step}s from frame {} down", from + 1);
        self.apply_sheet(
            &format!("retime to {step}s"),
            built,
            &format!("retimed to {step}s"),
            &nothing,
        );
    }

    /// Reverse the active column's timing from the cursor down: the drawing
    /// order flips and each exposure keeps its length.
    pub fn reverse_timing(&mut self) {
        let (columns, from, to) = self
            .sheet_range()
            .unwrap_or_else(|| (vec![self.view.active_column], self.view.frame, self.frame_count()));
        let built = sheet_ops::reverse_range(&self.engine.project, self.at(), &columns, from, to);
        self.apply_sheet(
            "reverse timing",
            built,
            "timing reversed",
            "nothing to reverse from here down",
        );
    }

    /// Set the active column's definition — one undo step. Allowed on a
    /// locked column: that is how it unlocks.
    fn set_active_column_def(&mut self, label: &str, def: ColumnDef, done: &str) {
        let at = self.at();
        let column = self.view.active_column;
        let r = self
            .engine
            .apply(label, vec![Command::SetColumnDef { at, column, def }]);
        self.report(r, done);
    }

    /// The active column's kind: Cel, BG (haikei), Book (an overlay held
    /// above the cels) or Ref (a guide, never rendered).
    pub fn set_column_kind(&mut self, kind: ColumnKind) {
        let name = self.active_column_name();
        let def = self.active_column_def();
        if def.kind == kind {
            self.status = format!("column {name} is already {}", kind.label());
            return;
        }
        let done = format!("column {name} is now {}", kind.label());
        self.set_active_column_def("column kind", ColumnDef { kind, ..def }, &done);
    }

    /// Hide or show the active column: a hidden column is out of every
    /// composite (the canvas, export and the graph).
    pub fn toggle_column_hidden(&mut self) {
        let name = self.active_column_name();
        let def = self.active_column_def();
        let hidden = !def.hidden;
        let (label, done) = if hidden {
            ("hide column", format!("column {name} hidden"))
        } else {
            ("show column", format!("column {name} shown"))
        };
        self.set_active_column_def(label, ColumnDef { hidden, ..def }, &done);
    }

    /// Lock or unlock the active column: a locked column refuses paint and
    /// sheet edits until it is unlocked.
    pub fn toggle_column_locked(&mut self) {
        let name = self.active_column_name();
        let def = self.active_column_def();
        let locked = !def.locked;
        let (label, done) = if locked {
            ("lock column", format!("column {name} locked"))
        } else {
            ("unlock column", format!("column {name} unlocked"))
        };
        self.set_active_column_def(label, ColumnDef { locked, ..def }, &done);
    }

    // ---- GM14: the sheet panel's range, clipboard and fields --------------

    /// The selected range of cells: (columns left to right, first frame,
    /// end frame exclusive). None when the selection is just the cursor's
    /// cell — and the range lives only while the cursor stays where the
    /// selection left it (a frame step elsewhere collapses it).
    pub fn sheet_range(&self) -> Option<(Vec<ColumnId>, u32, u32)> {
        let (anchor, head) = (self.sheet_ui.anchor?, self.sheet_ui.head?);
        if head != (self.view.active_column, self.view.frame) || anchor == head {
            return None;
        }
        let cols = &self.cut().xsheet.columns;
        let ia = cols.iter().position(|c| c.id == anchor.0)?;
        let ih = cols.iter().position(|c| c.id == head.0)?;
        let ids = cols[ia.min(ih)..=ia.max(ih)].iter().map(|c| c.id).collect();
        Some((ids, anchor.1.min(head.1), anchor.1.max(head.1) + 1))
    }

    /// Put the cursor on (column, frame) with the range running from
    /// `anchor` (the same cell for a plain click).
    pub fn select_cells(&mut self, anchor: (ColumnId, u32), column: ColumnId, frame: u32) {
        self.view.active_column = column;
        self.goto(frame);
        self.sheet_ui.anchor = Some(anchor);
        self.sheet_ui.head = Some((column, self.view.frame));
    }

    /// The range, else the cursor's own cell.
    fn sheet_cells(&self) -> (Vec<ColumnId>, u32, u32) {
        self.sheet_range().unwrap_or_else(|| {
            (vec![self.view.active_column], self.view.frame, self.view.frame + 1)
        })
    }

    /// Copy the selected cells to the sheet clipboard (no undo step); false
    /// when nothing was copied (the clipboard keeps what it held).
    pub fn copy_cells(&mut self) -> bool {
        let (columns, from, to) = self.sheet_cells();
        match sheet_ops::copy_block(&self.engine.project, self.at(), &columns, from, to) {
            Ok(block) => {
                self.status = format!(
                    "copied {} × {} cells",
                    block.columns.len(),
                    block.len
                );
                self.sheet_ui.clipboard = Some(block);
                // The sheet hands the new block's text to the system.
                self.sheet_ui.clip_text = None;
                true
            }
            Err(e) => {
                self.report(Err(e), "copy");
                false
            }
        }
    }

    /// Copy the selected cells, then empty them — one undo step. A failed
    /// copy cuts nothing (the clipboard still holds an older block).
    pub fn cut_cells(&mut self) {
        if !self.copy_cells() {
            return;
        }
        let Some(block) = &self.sheet_ui.clipboard else {
            return;
        };
        let (columns, from, _) = self.sheet_cells();
        let blank = sheet_ops::Block {
            columns: vec![vec![(Exposure::Empty, block.len)]; block.columns.len()],
            drawings: Vec::new(),
            ..block.clone()
        };
        let at = self.at();
        let built = sheet_ops::paste_block(
            &mut self.engine.project,
            &blank,
            at,
            columns[0],
            from,
            sheet_ops::PasteMode::Overwrite,
        );
        self.apply_sheet("cut cells", built, "cells cut", "nothing to cut");
    }

    /// Paste the sheet clipboard at the range's top-left (else the
    /// cursor): over the cells there, or (insert) pushing them later. One
    /// undo step.
    pub fn paste_cells(&mut self, insert: bool) {
        let Some(block) = self.sheet_ui.clipboard.clone() else {
            self.refuse("nothing to paste: copy cells first (Ctrl+C over the sheet)");
            return;
        };
        let (columns, frame, _) = self.sheet_cells();
        let mode = if insert {
            sheet_ops::PasteMode::Insert
        } else {
            sheet_ops::PasteMode::Overwrite
        };
        let at = self.at();
        let built =
            sheet_ops::paste_block(&mut self.engine.project, &block, at, columns[0], frame, mode);
        let label = if insert { "paste cells (insert)" } else { "paste cells" };
        self.apply_sheet(label, built, "cells pasted", "nothing to paste");
    }

    /// The typed entry on the cursor's cell: a drawing name exposes that
    /// drawing (made fresh when the cut has none of that name), "x" makes
    /// the cell empty. One undo step; blank text changes nothing.
    pub fn enter_cell(&mut self, text: &str) {
        let name = text.trim();
        if name.is_empty() {
            return;
        }
        let layers = self.new_cel_layers();
        let at = self.at();
        let (column, frame) = (self.view.active_column, self.view.frame);
        let built =
            sheet_ops::expose_by_name(&mut self.engine.project, at, column, frame, name, layers);
        self.apply_sheet(
            "enter cell",
            built,
            &format!("{name} at frame {}", frame + 1),
            "already showing that",
        );
    }

    /// Set the cut's length in frames — one undo step.
    pub fn set_cut_length(&mut self, frames: u32) {
        if frames == self.frame_count() {
            return;
        }
        let at = self.at();
        let r = self
            .engine
            .apply("cut length", vec![Command::SetFrameCount { at, frames }]);
        if r.is_ok() {
            self.view.frame = self.view.frame.min(frames.saturating_sub(1));
        }
        self.report(r, &format!("the cut is {frames} frames"));
    }

    /// Set (or, with blank text, clear) the memo on `frame` — one undo step.
    pub fn set_memo(&mut self, frame: u32, text: &str) {
        let text = text.trim();
        let new = (!text.is_empty()).then(|| text.to_string());
        if self.cut().memos.get(&frame) == new.as_ref() {
            return;
        }
        let at = self.at();
        let r = self
            .engine
            .apply("memo", vec![Command::SetMemo { at, frame, text: new }]);
        self.report(r, &format!("memo at frame {}", frame + 1));
    }

    /// Hide or show one column from its header (the column becomes the
    /// active one, as a header click does).
    pub fn toggle_hidden_of(&mut self, column: ColumnId) {
        self.view.active_column = column;
        self.toggle_column_hidden();
    }

    /// Lock or unlock one column from its header.
    pub fn toggle_locked_of(&mut self, column: ColumnId) {
        self.view.active_column = column;
        self.toggle_column_locked();
    }

    /// Step one column's kind Cel → BG → Book → Ref → Cel from its badge.
    pub fn cycle_kind_of(&mut self, column: ColumnId) {
        self.view.active_column = column;
        let next = match self.active_column_def().kind {
            ColumnKind::Cel => ColumnKind::Bg,
            ColumnKind::Bg => ColumnKind::Book,
            ColumnKind::Book => ColumnKind::Ref,
            ColumnKind::Ref => ColumnKind::Cel,
        };
        self.set_column_kind(next);
    }

    // ---- GM14: the curves pane's edits ------------------------------------

    /// Move the selected param keys by whole frames and a value delta as
    /// ONE undo step (a curves-pane drag); a key landing on another
    /// replaces it.
    pub fn move_param_keys(&mut self, keys: &[(ParamId, u32)], frames: i64, dv: f32) -> bool {
        let built =
            sheet_ops::move_param_keys(&self.engine.project, self.at(), keys, frames, dv);
        let before = self.engine.position();
        self.apply_sheet("move keys", built, "keys moved", "keys stay put");
        self.engine.position() != before
    }

    /// Replace param keys in place (interp or handle edits) as ONE undo
    /// step. Each entry is (column, frame, new key).
    pub fn set_param_keys(&mut self, label: &str, keys: Vec<(ParamId, u32, ParamKey)>) {
        let at = self.at();
        let mut edits = Vec::new();
        for (column, frame, after) in keys {
            let before = self.cut().xsheet.param(column).and_then(|p| p.key_at(frame));
            if before != Some(after) {
                edits.push(anim_core::command::ParamKeyEdit {
                    column,
                    frame,
                    before,
                    after: Some(after),
                });
            }
        }
        if edits.is_empty() {
            return;
        }
        let r = self
            .engine
            .apply(label, vec![Command::SetParamKeys { at, keys: edits }]);
        self.report(r, label);
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

    /// Add a PARAMETER column (camera etc.) — one undo step (GM5:
    /// Command::AddParamColumn), like add_column; keys on it are undoable
    /// too. Returns the new id for immediate binding.
    pub fn add_param_column(&mut self, name: &str) -> Option<anim_core::ids::ParamId> {
        let at = self.at();
        let index = self.cut().xsheet.params.len();
        let id = self.engine.alloc_param_id();
        let r = self.engine.apply(
            "add param column",
            vec![Command::AddParamColumn {
                at,
                index,
                column: ParamColumn::new(id, name),
            }],
        );
        let added = r.is_ok();
        self.report(r, &format!("param column '{name}' added"));
        added.then_some(id)
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

    /// Remove the selected (active) column — one undo step (GM5:
    /// Command::RemoveColumn; undo brings it back with its keys and
    /// definition). Keeps at least one column; a locked column refuses.
    pub fn remove_active_column(&mut self) {
        if self.cut().xsheet.columns.len() <= 1 {
            self.refuse("refused — a cut keeps at least one column");
            return;
        }
        let at = self.at();
        let removed = self.view.active_column;
        // The neighbour that becomes active once it is gone.
        let cols: Vec<ColumnId> = self.cut().xsheet.columns.iter().map(|c| c.id).collect();
        let idx = cols.iter().position(|&c| c == removed).unwrap_or(0);
        let new_active = cols
            .get(idx + 1)
            .or_else(|| idx.checked_sub(1).and_then(|i| cols.get(i)))
            .copied()
            .unwrap_or(removed);
        let r = self
            .engine
            .apply("remove column", vec![Command::RemoveColumn { at, id: removed }]);
        if r.is_ok() {
            self.view.active_column = new_active;
            self.view.selected_drawing = None;
        }
        self.report(r, "column removed");
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

    /// GM3: the history pane's jump — single undo/redo steps until
    /// `position` steps are applied.
    pub fn undo_to(&mut self, position: usize) {
        let from = self.engine.position();
        let ok = self.engine.undo_to(position).is_ok();
        let now = self.engine.position();
        if now != from {
            self.status = format!("history: step {now}");
            self.doc_gen = self.doc_gen.wrapping_add(1);
            self.history_moved();
        }
        if !ok {
            self.refuse("refused — that step could not be replayed");
        }
        self.sanitize();
    }

    /// GM5: undo, redo or a peer's edit can take away the cut or the column
    /// the view stands on (undo of New cut, of Add column) or shorten the
    /// cut under the cursor: step onto a live one — the scene's last cut,
    /// else the first cut anywhere; the last column — before anything
    /// reads `cut()`.
    pub fn keep_view_live(&mut self) {
        let p = &self.engine.project;
        if p.cut(self.view.scene, self.view.cut).is_none() {
            let here = p
                .scenes
                .iter()
                .find(|s| s.id == self.view.scene)
                .and_then(|s| s.cuts.last().map(|c| (s.id, c)));
            let anywhere = || p.scenes.iter().find_map(|s| s.cuts.first().map(|c| (s.id, c)));
            let Some((scene, cut)) = here.or_else(anywhere) else {
                return; // no cut at all: nothing to stand on
            };
            let (cut, first) = (cut.id, cut.xsheet.columns.first().map(|c| c.id));
            self.view.scene = scene;
            self.view.cut = cut;
            self.view.active_column = first.unwrap_or(self.view.active_column);
            self.view.frame = 0;
            self.view.selected_drawing = None;
            self.view.playing = false;
        }
        let cut = self.cut();
        let last = cut.xsheet.columns.last().map(|c| c.id);
        if cut.xsheet.column(self.view.active_column).is_none()
            && let Some(last) = last
        {
            self.view.active_column = last;
        }
        self.view.frame = self.view.frame.min(self.frame_count() - 1);
    }

    /// Selection can dangle after undo of a creation command — never let a
    /// dead id escape into the UI.
    fn sanitize(&mut self) {
        self.keep_view_live();
        if let Some(id) = self.view.selected_drawing
            && self.cut().drawing(id).is_none()
        {
            self.view.selected_drawing = None;
        }
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
    /// to it — one undo step (GM5: Command::AddCut; undoing it steps the
    /// view back onto the scene's last cut).
    pub fn new_cut(&mut self, scene: SceneId) {
        let index = self
            .engine
            .project
            .scenes
            .iter()
            .find(|s| s.id == scene)
            .map(|s| s.cuts.len())
            .unwrap_or(0);
        let id = self.engine.alloc_cut_id();
        let mut cut = Cut::new(id, format!("CUT{:02}", index + 1), 48);
        let column = self.engine.alloc_column_id();
        cut.xsheet.columns.push(Column::new(column, "A"));
        let r = self.engine.apply(
            "new cut",
            vec![Command::AddCut {
                scene,
                index,
                cut: Box::new(cut),
            }],
        );
        match r {
            Ok(()) => {
                self.doc_gen = self.doc_gen.wrapping_add(1);
                self.goto_cut(scene, id);
            }
            Err(e) => self.refuse(format!("refused — the cut could not be created ({e})")),
        }
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

/// GM5: the sheet verbs through `Editor::perform` — the one dispatch the
/// menus, the palette and the chords share (D6) — and what the canvas does
/// with the column definitions.
#[cfg(test)]
mod sheetedit_tests {
    use anim_core::command::Command;
    use anim_core::ids::{ColumnId, DrawingId};
    use anim_core::xsheet::{ColumnKind, Exposure};
    use eframe::egui;

    use crate::Performed;
    use crate::config::{self, Action, Ctx, Group, Keymap};
    use crate::testkit::{CTRL, EditorHarness};
    use crate::workspace::Pane;

    /// Every GM5 verb.
    const VERBS: [Action; 14] = [
        Action::InsertFrames,
        Action::DeleteFrames,
        Action::HoldPlus,
        Action::HoldMinus,
        Action::RetimeOnes,
        Action::RetimeTwos,
        Action::RetimeThrees,
        Action::Reverse,
        Action::ColumnKindCel,
        Action::ColumnKindBg,
        Action::ColumnKindBook,
        Action::ColumnKindRef,
        Action::ToggleColumnHidden,
        Action::ToggleColumnLocked,
    ];

    /// A harness whose column A shows drawings keyed at frames 0, 4 and 8
    /// of 48 (the last holds to the end), the cursor on frame 0.
    fn three_drawings() -> (EditorHarness, [DrawingId; 3]) {
        let mut h = EditorHarness::new();
        let s = h.state_mut();
        for f in [4, 8] {
            s.goto(f);
            s.new_drawing_at_frame();
        }
        s.goto(0);
        let ids = [0, 4, 8].map(|f| s.resolve_at(s.view.active_column, f).expect("keyed"));
        h.run();
        (h, ids)
    }

    /// The active column's keys.
    fn keys(h: &EditorHarness) -> Vec<(u32, Exposure)> {
        let s = h.state();
        let col = s.cut().xsheet.column(s.view.active_column).expect("active column");
        col.keys().collect()
    }

    /// Keys `frames[i]` → `drawings[i]`.
    fn drawn(frames: &[u32], drawings: &[DrawingId]) -> Vec<(u32, Exposure)> {
        frames
            .iter()
            .zip(drawings)
            .map(|(f, d)| (*f, Exposure::Drawing(*d)))
            .collect()
    }

    fn strokes(h: &EditorHarness) -> usize {
        h.state().cut().drawings.iter().map(|d| d.strokes.len()).sum()
    }

    fn sheet_centre(h: &EditorHarness) -> egui::Pos2 {
        let r = h.editor().pane_rects.iter().find(|(p, _)| *p == Pane::XSheet);
        r.map(|(_, r)| r.center()).expect("the drawing room docks the sheet")
    }

    #[test]
    fn sheetedit_timing_verbs_work_on_the_cursor_and_undo_exactly() {
        let (mut h, [d0, d1, d2]) = three_drawings();
        let start = keys(&h);
        assert_eq!(start, drawn(&[0, 4, 8], &[d0, d1, d2]));
        let steps = h.state().engine.position();
        let cases = [
            (4, Action::HoldPlus, drawn(&[0, 4, 9], &[d0, d1, d2]), 48),
            (4, Action::HoldMinus, drawn(&[0, 4, 7], &[d0, d1, d2]), 48),
            (4, Action::InsertFrames, drawn(&[0, 5, 9], &[d0, d1, d2]), 49),
            (2, Action::DeleteFrames, drawn(&[0, 3, 7], &[d0, d1, d2]), 47),
            (0, Action::RetimeOnes, drawn(&[0, 1, 2], &[d0, d1, d2]), 48),
            (0, Action::RetimeTwos, drawn(&[0, 2, 4], &[d0, d1, d2]), 48),
            (0, Action::RetimeThrees, drawn(&[0, 3, 6], &[d0, d1, d2]), 48),
            // Each exposure keeps its length; past the cut's end the frames
            // keep showing what they showed (a hidden key at 48).
            (0, Action::Reverse, drawn(&[0, 40, 44, 48], &[d2, d1, d0, d2]), 48),
        ];
        for (frame, verb, want, frames) in cases {
            h.state_mut().goto(frame);
            assert_eq!(h.perform(verb), Performed::Done, "{verb:?}: {}", h.state().refusal);
            assert_eq!(keys(&h), want, "{verb:?}");
            assert_eq!(h.state().frame_count(), frames, "{verb:?}");
            assert_eq!(h.state().engine.position(), steps + 1, "{verb:?} is one step");
            assert_eq!(h.perform(Action::Undo), Performed::Done);
            assert_eq!(keys(&h), start, "{verb:?} undone exactly");
            assert_eq!(h.state().frame_count(), 48, "{verb:?} undone exactly");
        }
    }

    #[test]
    fn sheetedit_refusals_say_why_on_the_aka_lane() {
        let (mut h, _) = three_drawings();
        // Hold +1 on the bottom drawing: it already holds to the end.
        h.state_mut().goto(8);
        let why = "the last drawing already holds to the end of the cut";
        assert_eq!(h.editor().can_perform(Action::HoldPlus), Err(why));
        assert_eq!(h.perform(Action::HoldPlus), Performed::Ignored);
        assert!(h.state().refusal.contains("lengthen the cut first"), "{}", h.state().refusal);
        // A locked column refuses timing edits, naming the column.
        assert_eq!(h.perform(Action::ToggleColumnLocked), Performed::Done);
        assert_eq!(h.editor().is_on(Action::ToggleColumnLocked), Some(true));
        h.state_mut().goto(4);
        assert_eq!(h.editor().can_perform(Action::HoldMinus), Err("this column is locked"));
        let steps = h.state().engine.position();
        for verb in [
            Action::HoldMinus,
            Action::RetimeTwos,
            Action::Reverse,
            Action::InsertFrames,
            Action::DeleteFrames,
        ] {
            let seq = h.state().refusal_seq;
            assert_eq!(h.perform(verb), Performed::Ignored, "{verb:?}");
            assert_ne!(h.state().refusal_seq, seq, "{verb:?} said no");
            assert!(h.state().refusal.contains("column A is locked"), "{verb:?}: {}", h.state().refusal);
        }
        assert_eq!(h.state().engine.position(), steps, "nothing landed");
        // The lock never locks itself: it comes off, and the edit lands.
        assert_eq!(h.perform(Action::ToggleColumnLocked), Performed::Done);
        assert_eq!(h.editor().is_on(Action::ToggleColumnLocked), Some(false));
        assert_eq!(h.perform(Action::HoldMinus), Performed::Done);
    }

    #[test]
    fn sheetedit_column_kind_and_hidden_are_undoable_toggles() {
        let mut h = EditorHarness::new();
        let kind = |h: &EditorHarness| h.state().active_column_def().kind;
        assert_eq!(h.editor().is_on(Action::ColumnKindCel), Some(true));
        let kinds = [
            (Action::ColumnKindBg, ColumnKind::Bg),
            (Action::ColumnKindBook, ColumnKind::Book),
            (Action::ColumnKindRef, ColumnKind::Ref),
            (Action::ColumnKindCel, ColumnKind::Cel),
        ];
        for (verb, k) in kinds {
            assert_eq!(h.perform(verb), Performed::Done, "{verb:?}");
            assert_eq!(kind(&h), k);
            assert_eq!(h.editor().is_on(verb), Some(true), "{verb:?} shows checked");
        }
        // The kind it already has is a remark, not a step.
        let steps = h.state().engine.position();
        h.perform(Action::ColumnKindCel);
        assert_eq!(h.state().engine.position(), steps);
        assert!(h.state().status.contains("already Cel"), "{}", h.state().status);
        h.perform(Action::ToggleColumnHidden);
        assert!(h.state().active_column_def().hidden);
        assert_eq!(h.editor().is_on(Action::ToggleColumnHidden), Some(true));
        // Undo walks it all back: shown, then Ref, Book, BG and Cel.
        h.perform(Action::Undo);
        assert!(!h.state().active_column_def().hidden);
        for k in [ColumnKind::Ref, ColumnKind::Book, ColumnKind::Bg, ColumnKind::Cel] {
            assert_eq!(h.perform(Action::Undo), Performed::Done);
            assert_eq!(kind(&h), k);
        }
    }

    #[test]
    fn sheetedit_remove_column_then_undo_brings_it_back_with_its_keys() {
        let mut h = EditorHarness::new();
        let a = h.state().view.active_column;
        h.state_mut().add_column();
        let b = h.state().view.active_column;
        assert_ne!(a, b, "the new column is active");
        let s = h.state_mut();
        s.new_drawing_at_frame();
        s.goto(3);
        s.new_drawing_at_frame();
        assert_eq!(h.perform(Action::ColumnKindBook), Performed::Done);
        let col = |h: &EditorHarness| h.state().cut().xsheet.column(b).cloned();
        let before = col(&h).expect("column B");
        assert_eq!(before.keys().count(), 2);
        assert_eq!(h.perform(Action::RemoveColumn), Performed::Done);
        assert!(col(&h).is_none(), "B is gone");
        assert_eq!(h.state().view.active_column, a, "its neighbour is active");
        assert_eq!(h.perform(Action::Undo), Performed::Done);
        assert_eq!(col(&h), Some(before), "B is back, keys and all");
        assert_eq!(h.state().cut().column_def(b).kind, ColumnKind::Book, "and its kind");
        // The steps before the removal still undo: the kind, both drawings,
        // then the column itself, and the view steps onto a live column.
        for _ in 0..3 {
            assert_eq!(h.perform(Action::Undo), Performed::Done);
        }
        h.state_mut().view.active_column = b;
        assert_eq!(h.perform(Action::Undo), Performed::Done);
        assert!(col(&h).is_none(), "undo of Add column takes B away");
        assert_eq!(h.state().view.active_column, a, "the view stands on a live column");
        assert_eq!(h.perform(Action::Redo), Performed::Done);
        assert!(col(&h).is_some_and(|c| c.keys().count() == 0), "redo adds it back, empty");
    }

    #[test]
    fn sheetedit_new_cut_and_param_column_are_undo_steps() {
        let mut h = EditorHarness::new();
        let (scene, cut0) = (h.state().view.scene, h.state().view.cut);
        h.state_mut().new_cut(scene);
        h.run();
        let cut1 = h.state().view.cut;
        assert_ne!(cut1, cut0, "the view moved onto the new cut");
        assert_eq!(h.state().cut().name, "CUT02");
        assert_eq!(h.state().cut().xsheet.columns.len(), 1, "with its column A");
        assert_eq!(h.perform(Action::Undo), Performed::Done);
        assert!(h.state().engine.project.cut(scene, cut1).is_none(), "the cut is gone");
        assert_eq!(h.state().view.cut, cut0, "the view is back on a live cut");
        let a = h.state().view.active_column;
        assert!(h.state().cut().xsheet.column(a).is_some(), "and a live column");
        assert_eq!(h.perform(Action::Redo), Performed::Done);
        assert!(h.state().engine.project.cut(scene, cut1).is_some(), "redo brings it back");
        // A parameter column (camera) is one step too.
        let p = h.state_mut().add_param_column("cam").expect("added");
        assert!(h.state().cut().xsheet.param(p).is_some());
        assert_eq!(h.perform(Action::Undo), Performed::Done);
        assert!(h.state().cut().xsheet.param(p).is_none(), "undo takes it away");
    }

    #[test]
    fn sheetedit_canvas_stacks_follow_the_render_order() {
        let mut h = EditorHarness::new();
        let mut cols: Vec<ColumnId> = vec![h.state().view.active_column];
        for _ in 0..4 {
            h.state_mut().add_column();
            cols.push(h.state().view.active_column);
        }
        let [a, b, c, d, e] = cols[..] else {
            unreachable!("five columns")
        };
        // B is a BG right of A on the sheet, C a Ref guide, D hidden.
        let defs = [
            (b, Action::ColumnKindBg),
            (c, Action::ColumnKindRef),
            (d, Action::ToggleColumnHidden),
        ];
        for (col, verb) in defs {
            h.state_mut().view.active_column = col;
            assert_eq!(h.perform(verb), Performed::Done, "{verb:?}");
        }
        h.state_mut().view.active_column = a;
        h.run();
        let (below, above) = h.editor().canvas.stacks.clone();
        assert_eq!(below, vec![b], "the BG draws under the active cel");
        assert_eq!(above, vec![e], "the cel after it draws over it");
        // From the last cel the whole render order is under it.
        h.state_mut().view.active_column = e;
        h.run();
        assert_eq!(h.editor().canvas.stacks, (vec![b, a], vec![]));
        // The Ref and the hidden column are in neither stack.
        for (below, above) in [h.editor().canvas.stacks.clone()] {
            assert!(!below.contains(&c) && !above.contains(&c), "Ref never draws");
            assert!(!below.contains(&d) && !above.contains(&d), "hidden never draws");
        }
    }

    #[test]
    fn sheetedit_a_locked_column_refuses_the_stroke_at_pen_down() {
        let mut h = EditorHarness::new();
        let c = h.editor().canvas.dbg_rect.center();
        let path = [c, c + egui::vec2(30.0, 0.0), c + egui::vec2(60.0, 10.0)];
        assert_eq!(h.perform(Action::ToggleColumnLocked), Performed::Done);
        let (steps, rows, n) = (
            h.state().engine.position(),
            h.state().engine.history_len(),
            strokes(&h),
        );
        // On its own cel, and on a held frame (a new cel would land in it).
        for frame in [0, 5] {
            h.state_mut().goto(frame);
            let seq = h.state().refusal_seq;
            h.drag_path(&path);
            assert_ne!(h.state().refusal_seq, seq, "frame {frame}: refused");
            let want = "column A is locked: unlock it in the sheet to draw";
            assert!(h.state().refusal.contains(want), "{}", h.state().refusal);
            assert!(h.editor().canvas.stroke_tap.is_empty(), "no stroke began");
        }
        assert_eq!(h.state().engine.position(), steps, "no history entry");
        assert_eq!(h.state().engine.history_len(), rows);
        assert_eq!(strokes(&h), n);
        // A cel a locked column exposes is locked wherever it is drawn on
        // (the engine's rule): B shows A's cel and B is locked.
        h.perform(Action::ToggleColumnLocked);
        h.state_mut().goto(0);
        let a = h.state().view.active_column;
        let d0 = h.state().own_key_drawing().expect("A's cel");
        h.state_mut().add_column();
        let b = h.state().view.active_column;
        let at = h.state().at();
        let expose = Command::SetCell {
            at,
            column: b,
            frame: 0,
            key: Some(Exposure::Drawing(d0)),
        };
        h.state_mut().engine.apply("expose", vec![expose]).expect("exposed");
        h.perform(Action::ToggleColumnLocked);
        h.state_mut().view.active_column = a;
        h.drag_path(&path);
        assert!(h.state().refusal.contains("column B is locked"), "{}", h.state().refusal);
        assert_eq!(strokes(&h), n);
        // Unlocked, the same stroke lands.
        h.state_mut().view.active_column = b;
        h.perform(Action::ToggleColumnLocked);
        h.state_mut().view.active_column = a;
        h.drag_path(&path);
        assert_eq!(strokes(&h), n + 1, "{}", h.state().refusal);
    }

    #[test]
    fn sheetedit_verbs_are_sheet_actions_with_free_chords_in_both_keymaps() {
        for km in Keymap::ALL {
            let binds = config::default_binds(*km);
            assert_eq!(config::clashes(&binds), vec![], "{km:?} has no chord conflict");
            for a in VERBS {
                assert!(Action::ALL.contains(&a));
                assert_eq!((a.group(), a.context()), (Group::Sheet, Ctx::Sheet), "{a:?}");
                assert!(a.menu().is_some_and(|m| m.starts_with("Sheet/")), "{a:?}");
                assert!((1..=120).contains(&a.help().chars().count()), "{a:?} has help");
                let b = binds.iter().find(|b| b.action == a).expect("bound");
                let c = b.chord.as_ref().unwrap_or_else(|| panic!("{a:?} has a chord in {km:?}"));
                for k in &c.keys {
                    assert!(egui::Key::from_name(k).is_some(), "{k} is a key");
                    assert_ne!(k, "R", "R stays the flip");
                }
            }
        }
        // The palette finds them by the sheet's own words.
        let words = [
            ("twos", Action::RetimeTwos),
            ("ones", Action::RetimeOnes),
            ("threes", Action::RetimeThrees),
            ("hold", Action::HoldPlus),
            ("insert frames", Action::InsertFrames),
            ("delete frames", Action::DeleteFrames),
            ("reverse", Action::Reverse),
            ("haikei", Action::ColumnKindBg),
        ];
        for (query, want) in words {
            assert_eq!(crate::cmdpal::ranked(query, &[])[0], want, "{query}");
        }
    }

    #[test]
    fn sheetedit_chords_and_the_palette_reach_the_sheet_cursor() {
        let (mut h, [d0, d1, d2]) = three_drawings();
        let start = keys(&h);
        h.point_at(sheet_centre(&h));
        // = and - hold the cursor's drawing a frame longer, then shorter.
        h.key(egui::Key::Equals);
        assert_eq!(keys(&h), drawn(&[0, 5, 9], &[d0, d1, d2]));
        h.key(egui::Key::Minus);
        assert_eq!(keys(&h), start);
        // Alt+2 retimes the column to twos; Ctrl+Z takes it back.
        let alt = egui::Modifiers {
            alt: true,
            ..egui::Modifiers::NONE
        };
        h.key_mods(alt, egui::Key::Num2);
        assert_eq!(keys(&h), drawn(&[0, 2, 4], &[d0, d1, d2]));
        h.key_mods(CTRL, egui::Key::Z);
        assert_eq!(keys(&h), start);
        // Insert opens a frame at the cursor.
        h.key(egui::Key::Insert);
        assert_eq!(h.state().frame_count(), 49);
        // Shift+Delete closes it — over the sheet only.
        let shift = egui::Modifiers {
            shift: true,
            ..egui::Modifiers::NONE
        };
        h.point_at(h.editor().canvas.dbg_rect.center());
        h.key_mods(shift, egui::Key::Delete);
        assert_eq!(h.state().frame_count(), 49, "Shift+Delete over the canvas kept the frame");
        h.point_at(sheet_centre(&h));
        h.key_mods(shift, egui::Key::Delete);
        assert_eq!(h.state().frame_count(), 48);
        assert_eq!(keys(&h), start, "frame 0 still shows d0");
        // Alt+L locks the column and unlocks it again.
        h.key_mods(alt, egui::Key::L);
        assert!(h.state().active_column_def().locked);
        h.key_mods(alt, egui::Key::L);
        assert!(!h.state().active_column_def().locked);
        // The palette runs a verb by its sheet word.
        h.perform(Action::CommandPalette);
        h.type_text("threes");
        h.key(egui::Key::Enter);
        assert_eq!(keys(&h), drawn(&[0, 3, 6], &[d0, d1, d2]));
    }
}

#[cfg(test)]
mod lighttable_tests {
    use std::collections::{BTreeMap, HashSet};
    use std::sync::Arc;

    use anim_core::ids::DrawingId;
    use anim_core::model::DrawingKind;
    use anim_core::raster::{TILE, TILE_LEN, TileData, f32_to_f16_bits};
    use eframe::egui;

    use anim_core::command::Command;
    use anim_core::xsheet::Exposure;

    use super::{
        AppState, CHART_MAX_MARKS, ChartSpacing, chart_marks, inbetween_frames, inbetween_names,
        move_chart_mark,
    };
    use crate::canvas::{
        ONION_AHEAD_TINT, ONION_BEHIND_TINT, OnionCfg, OnionMode, PinShift, onion_ghosts,
        onion_set, onion_slot_plan, shift_tiles, vector_onion_set,
    };
    use crate::config::{Action, Config};
    use crate::plate;
    use crate::testkit::EditorHarness;
    use crate::workspace::Pane;
    use crate::xsheet_panel::{CellMark, cell_mark};

    /// Column A keyed at `frames` (frame 0 is the harness's own drawing),
    /// the cursor on frame 0; the drawings in frame order.
    fn keyed(frames: &[u32]) -> (EditorHarness, Vec<DrawingId>) {
        let mut h = EditorHarness::new();
        let s = h.state_mut();
        for &f in frames.iter().filter(|f| **f > 0) {
            s.goto(f);
            s.new_drawing_at_frame();
        }
        s.goto(0);
        let ids = frames
            .iter()
            .map(|&f| s.resolve_at(s.view.active_column, f).expect("keyed"))
            .collect();
        h.run();
        (h, ids)
    }

    fn set_at(s: &AppState, frame: u32, cfg: &OnionCfg) -> Vec<(DrawingId, i32)> {
        onion_set(s.cut(), s.view.active_column, frame, cfg)
            .into_iter()
            .map(|(d, o, _)| (d, o))
            .collect()
    }

    fn depth(before: u8, after: u8, mode: OnionMode) -> OnionCfg {
        OnionCfg {
            before,
            after,
            mode,
            ..OnionCfg::default()
        }
    }

    /// One solid tile of premultiplied `v` at `coord` on the armed layer.
    fn ink_tile(h: &mut EditorHarness, coord: (i32, i32), v: f32) {
        let slot = h.state().active_slot_resolved().expect("an armed layer");
        let mut px = vec![0u16; TILE_LEN];
        for x in px.iter_mut() {
            *x = f32_to_f16_bits(v);
        }
        h.state_mut()
            .commit_raster(vec![(coord, Arc::new(TileData::from_vec(px)))], slot)
            .expect("the tile committed");
    }

    fn hashes(s: &AppState) -> Vec<u64> {
        s.cut().drawings.iter().map(|d| d.content_hash()).collect()
    }

    /// Expose drawing `d` at `frame` of the active column (one SetCell).
    fn expose(s: &mut AppState, frame: u32, d: DrawingId) {
        let (at, column) = (s.at(), s.view.active_column);
        let key = Some(Exposure::Drawing(d));
        s.engine
            .apply("expose", vec![Command::SetCell { at, column, frame, key }])
            .expect("exposed");
    }

    /// The canvas pane's centre in the drawing room.
    fn canvas_center(h: &EditorHarness) -> egui::Pos2 {
        let c = h.editor().pane_rects.iter().find(|(p, _)| *p == Pane::Canvas).map(|(_, r)| r.center());
        c.expect("the drawing room docks the canvas")
    }

    #[test]
    fn lighttable_onion_depth_one_is_the_old_neighbours_on_every_frame() {
        let (mut h, _) = keyed(&[0, 4, 8]);
        let cfg = OnionCfg::default();
        for f in 0..h.state().frame_count() {
            h.state_mut().goto(f);
            let s = h.state();
            let set = set_at(s, f, &cfg);
            let behind = set.iter().find(|(_, o)| *o < 0).map(|(d, _)| *d);
            let ahead = set.iter().find(|(_, o)| *o > 0).map(|(d, _)| *d);
            assert_eq!([behind, ahead], s.onion_neighbors(), "frame {f}");
        }
    }

    #[test]
    fn lighttable_onion_depth_zero_is_empty_and_five_reaches_every_drawing() {
        let (h, d) = keyed(&[0, 2, 4, 6, 8, 10, 12]);
        let s = h.state();
        assert!(set_at(s, 6, &depth(0, 0, OnionMode::Drawings)).is_empty());
        assert_eq!(
            set_at(s, 6, &depth(5, 5, OnionMode::Drawings)),
            vec![(d[2], -1), (d[1], -2), (d[0], -3), (d[4], 1), (d[5], 2), (d[6], 3)]
        );
        // Each step farther fades by the falloff.
        let w: Vec<f32> = onion_set(s.cut(), s.view.active_column, 6, &depth(3, 0, OnionMode::Drawings))
            .into_iter()
            .map(|(_, _, w)| w)
            .collect();
        let cfg = OnionCfg::default();
        let want = [cfg.opacity, cfg.opacity * cfg.falloff, cfg.opacity * cfg.falloff * cfg.falloff];
        assert_eq!(w.len(), want.len());
        assert!(w.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6), "{w:?}");
    }

    #[test]
    fn lighttable_onion_frames_mode_counts_frames_not_drawings() {
        let (h, d) = keyed(&[0, 4, 8]);
        // Mid-hold of the second drawing: one frame back is itself, two
        // back the first; ahead, the hold is skipped and the third is 3 on.
        let set = set_at(h.state(), 5, &depth(2, 4, OnionMode::Frames));
        assert_eq!(set, vec![(d[1], -1), (d[0], -2), (d[2], 3)]);
    }

    #[test]
    fn lighttable_onion_keys_mode_skips_drawings_that_are_not_keys() {
        let (mut h, d) = keyed(&[0, 4, 8, 12]);
        let s = h.state_mut();
        for k in [d[0], d[2]] {
            s.set_drawing_kind(k, DrawingKind::Key);
        }
        let set = set_at(s, 4, &depth(1, 1, OnionMode::Keys));
        assert_eq!(set, vec![(d[0], -1), (d[2], 1)]);
        // Unmarked drawings never ghost in key mode.
        let all = set_at(s, 12, &depth(5, 5, OnionMode::Keys));
        assert_eq!(all, vec![(d[2], -1), (d[0], -2)]);
    }

    #[test]
    fn lighttable_onion_other_columns_join_only_when_asked() {
        let (mut h, d) = keyed(&[0, 4]);
        let s = h.state_mut();
        let a = s.view.active_column;
        s.add_column();
        s.goto(0);
        s.new_drawing_at_frame();
        let b = s.current_drawing().expect("column B's drawing");
        let mine = onion_set(s.cut(), a, 4, &OnionCfg::default());
        assert_eq!(mine.iter().map(|x| x.0).collect::<Vec<_>>(), vec![d[0]]);
        let cfg = OnionCfg {
            other_columns: true,
            ..OnionCfg::default()
        };
        let all: Vec<DrawingId> = onion_set(s.cut(), a, 4, &cfg).into_iter().map(|x| x.0).collect();
        assert_eq!(all, vec![d[0], b]);
    }

    #[test]
    fn lighttable_onion_config_comes_back_in_range() {
        let wild = OnionCfg {
            before: 9,
            after: 200,
            opacity: f32::NAN,
            falloff: 3.0,
            ..OnionCfg::default()
        }
        .sane();
        assert_eq!((wild.before, wild.after), (OnionCfg::MAX_DEPTH, OnionCfg::MAX_DEPTH));
        assert_eq!(wild.opacity, OnionCfg::default().opacity);
        assert_eq!(wild.falloff, 1.0);
        assert_eq!(OnionCfg::default().weight(1), OnionCfg::default().opacity);
    }

    #[test]
    fn lighttable_onion_tints_are_unchanged() {
        assert_eq!(ONION_BEHIND_TINT, [160, 195, 232]);
        assert_eq!(ONION_AHEAD_TINT, [205, 206, 199]);
        assert_eq!(plate::AO.to_array(), [83, 137, 196, 255]);
        assert_eq!(plate::LEGEND.to_array(), [139, 141, 131, 255]);
    }

    #[test]
    fn lighttable_cell_marks_follow_the_drawing_kind() {
        assert_eq!(cell_mark(DrawingKind::Key), CellMark::Circle);
        assert_eq!(cell_mark(DrawingKind::Breakdown), CellMark::Underline);
        assert_eq!(cell_mark(DrawingKind::Inbetween), CellMark::None);
        assert_eq!(cell_mark(DrawingKind::Unset), CellMark::None);
    }

    #[test]
    fn lighttable_marking_a_key_names_the_sheet_cell_and_never_touches_pixels() {
        let mut h = EditorHarness::new();
        h.run();
        assert!(!h.has_text_containing("at frame 1: genga (key)"));
        let before = hashes(h.state());
        let steps = h.state().engine.position();
        h.perform(Action::KindKey);
        h.run();
        assert!(h.has_text_containing("at frame 1: genga (key)"), "{}", h.describe());
        let d = h.state().current_drawing().expect("a drawing");
        assert_eq!(h.state().cut().drawing_kind(d), DrawingKind::Key);
        assert_eq!(h.state().engine.position(), steps + 1, "one undo step");
        assert_eq!(hashes(h.state()), before, "a kind never changes pixels");
        h.perform(Action::KindBreakdown);
        h.run();
        assert!(h.has_text_containing("at frame 1: breakdown"));
        h.state_mut().undo();
        h.state_mut().undo();
        assert_eq!(h.state().cut().drawing_kind(d), DrawingKind::Unset);
    }

    #[test]
    fn lighttable_pins_show_on_every_frame_and_never_touch_the_document() {
        let (mut h, d) = keyed(&[0, 4, 8]);
        let before = hashes(h.state());
        let steps = h.state().engine.position();
        h.perform(Action::PinToLightTable);
        assert!(h.state().view.light.is_pinned(d[0]));
        for f in [0, 4, 8, 30] {
            h.state_mut().goto(f);
            h.run();
            let shown: Vec<DrawingId> = h.state().view.light.shown(h.state().cut()).iter().map(|x| x.0).collect();
            assert_eq!(shown, vec![d[0]], "frame {f}");
        }
        h.state_mut().view.light.nudge(12.0, -4.0, 0.1);
        h.run();
        let moved = PinShift {
            dx: 12.0,
            dy: -4.0,
            rot: 0.1,
        };
        assert_eq!(h.state().view.light.shift(d[0]), moved);
        // A second pin, selected, moves alone; pinning again unpins.
        h.state_mut().goto(4);
        h.perform(Action::PinToLightTable);
        h.state_mut().view.light.selected = Some(d[1]);
        h.state_mut().view.light.nudge(1.0, 0.0, 0.0);
        assert_eq!(h.state().view.light.shift(d[1]).dx, 1.0);
        assert_eq!(h.state().view.light.shift(d[0]), moved);
        h.perform(Action::ResetShift);
        assert!(h.state().view.light.offsets.is_empty());
        h.perform(Action::PinToLightTable);
        assert!(!h.state().view.light.is_pinned(d[1]));
        assert_eq!(hashes(h.state()), before, "pins are view state");
        assert_eq!(h.state().engine.position(), steps, "no undo step");
    }

    #[test]
    fn lighttable_shift_and_trace_drag_moves_the_pin_not_the_pen() {
        let (mut h, d) = keyed(&[0, 4]);
        h.perform(Action::PinToLightTable);
        h.state_mut().goto(4);
        h.state_mut().view.light.shift_trace = true;
        h.run();
        let strokes = |h: &EditorHarness| h.state().cut().drawings.iter().map(|d| d.strokes.len()).sum::<usize>();
        let (before, steps) = (strokes(&h), h.state().engine.position());
        let c = h.editor().pane_rects.iter().find(|(p, _)| *p == Pane::Canvas).map(|(_, r)| r.center());
        let c = c.expect("the drawing room docks the canvas");
        h.drag(c, c + egui::vec2(60.0, 0.0), 6);
        h.run();
        let s = h.state().view.light.shift(d[0]);
        assert!(s.dx > 1.0 && s.dy.abs() < 1e-3 && s.rot == 0.0, "{s:?}");
        assert_eq!((strokes(&h), h.state().engine.position()), (before, steps), "the pen rested");
        h.perform(Action::ResetShift);
        assert!(h.state().view.light.shift(d[0]).is_identity());
    }

    #[test]
    fn lighttable_shift_tiles_moves_ink_and_the_identity_shares_tiles() {
        let mut px = vec![0u16; TILE_LEN];
        for x in px.iter_mut() {
            *x = f32_to_f16_bits(0.5);
        }
        let mut tiles = BTreeMap::new();
        tiles.insert((0, 0), Arc::new(TileData::from_vec(px)));
        let center = egui::pos2(960.0, 540.0);
        let same = shift_tiles(&tiles, PinShift::default(), center);
        assert!(Arc::ptr_eq(&same[&(0, 0)], &tiles[&(0, 0)]));
        let shift = PinShift {
            dx: TILE as f32,
            dy: 0.0,
            rot: 0.0,
        };
        let moved = shift_tiles(&tiles, shift, center);
        assert!(moved.get(&(1, 0)).is_some_and(|t| !t.is_empty()));
        assert!(moved.get(&(0, 0)).is_none_or(|t| t.is_empty()));
        // A turned pin maps back exactly.
        let turn = PinShift {
            dx: 3.0,
            dy: -7.0,
            rot: 0.4,
        };
        let p = egui::pos2(100.0, 40.0);
        let q = turn.invert(turn.apply(p, center), center);
        assert!((q - p).length() < 1e-3, "{q:?}");
    }

    #[test]
    fn lighttable_inbetween_frames_split_the_span_evenly() {
        assert_eq!(inbetween_frames(0, 8, 3), Some(vec![2, 4, 6]));
        assert_eq!(inbetween_frames(0, 4, 3), Some(vec![1, 2, 3]));
        assert_eq!(inbetween_frames(10, 16, 1), Some(vec![13]));
        assert_eq!(inbetween_frames(0, 3, 3), None, "too short");
        assert_eq!(inbetween_frames(8, 0, 1), None, "backwards");
        assert_eq!(inbetween_frames(0, 8, 0), None);
    }

    #[test]
    fn lighttable_inbetween_names_number_between_the_keys_or_letter_the_first() {
        let none = HashSet::new();
        assert_eq!(inbetween_names("1", "5", 3, &none), ["2", "3", "4"]);
        assert_eq!(inbetween_names("1", "9", 1, &none), ["5"]);
        assert_eq!(inbetween_names("5", "1", 3, &none), ["4", "3", "2"]);
        assert_eq!(inbetween_names("1", "2", 3, &none), ["1a", "1b", "1c"]);
        let taken: HashSet<String> = ["3".to_string(), "1a".to_string()].into();
        assert_eq!(inbetween_names("1", "5", 3, &taken), ["1b", "1c", "1d"]);
        assert_eq!(inbetween_names("A1", "A5", 2, &none), ["A2", "A4"]);
    }

    #[test]
    fn lighttable_make_inbetweens_fills_the_chart_in_one_undo_step() {
        let (mut h, d) = keyed(&[0, 8]);
        let s = h.state_mut();
        let col = s.view.active_column;
        s.set_chart(d[0], &[0.25, 0.5, 0.75]);
        let def = s.cut().drawing_def(d[0]);
        assert_eq!(def.kind, DrawingKind::Key);
        assert_eq!(def.chart.as_ref().map(|c| c.to), Some(d[1]));
        let name = |s: &AppState, id: DrawingId| s.cut().drawing(id).map(|x| x.name.clone()).unwrap_or_default();
        let taken: HashSet<String> = s.cut().drawings.iter().map(|x| x.name.clone()).collect();
        let want = inbetween_names(&name(s, d[0]), &name(s, d[1]), 3, &taken);
        let (count, steps) = (s.cut().drawings.len(), s.engine.position());
        assert!(s.make_inbetweens(), "{}", s.status);
        assert_eq!(s.engine.position(), steps + 1, "one undo step");
        let frames: Vec<u32> = s.cut().xsheet.column(col).unwrap().keys().map(|(f, _)| f).collect();
        assert_eq!(frames, vec![0, 2, 4, 6, 8]);
        for (f, want) in [2, 4, 6].into_iter().zip(&want) {
            let ib = s.resolve_at(col, f).expect("an inbetween");
            assert_eq!(s.cut().drawing_kind(ib), DrawingKind::Inbetween);
            assert_eq!(&name(s, ib), want);
        }
        s.undo();
        let frames: Vec<u32> = s.cut().xsheet.column(col).unwrap().keys().map(|(f, _)| f).collect();
        assert_eq!(frames, vec![0, 8]);
        assert_eq!(s.cut().drawings.len(), count);
        // Without a chart there is nothing to fill.
        s.goto(8);
        assert!(!s.make_inbetweens());
    }

    #[test]
    fn lighttable_duplicate_cel_copies_the_layers_under_a_new_name() {
        let mut h = EditorHarness::new();
        ink_tile(&mut h, (0, 0), 0.5);
        let s = h.state_mut();
        let col = s.view.active_column;
        let src = s.resolve_at(col, 0).expect("a drawing");
        let steps = s.engine.position();
        s.duplicate_cel();
        assert_eq!(s.engine.position(), steps + 1, "one undo step");
        let dup = s.resolve_at(col, 0).expect("the copy");
        assert_ne!(dup, src);
        let (a, b) = (s.cut().drawing(src).unwrap(), s.cut().drawing(dup).unwrap());
        assert_ne!(a.name, b.name);
        assert_eq!(a.layers.len(), b.layers.len());
        for (la, lb) in a.layers.iter().zip(&b.layers) {
            assert_ne!(la.id, lb.id);
            assert_eq!(la.content_hash(), lb.content_hash());
        }
        s.undo();
        assert_eq!(s.resolve_at(col, 0), Some(src));
        assert!(s.cut().drawing(dup).is_none());
    }

    #[test]
    fn lighttable_copied_pixels_paste_in_place_into_another_drawing() {
        let (mut h, _) = keyed(&[0, 8]);
        ink_tile(&mut h, (1, 1), 0.5);
        let src = h.state().active_layer_tiles().and_then(|m| m.get(&(1, 1)).cloned());
        let src = src.expect("the inked tile");
        let s = h.state_mut();
        assert!(!s.copy_pixels(&[(300.0, 300.0), (310.0, 300.0), (310.0, 310.0)]), "no ink there");
        let square = [(56.0, 56.0), (136.0, 56.0), (136.0, 136.0), (56.0, 136.0)];
        assert!(s.copy_pixels(&square));
        s.goto(8);
        let steps = s.engine.position();
        s.paste_pixels();
        assert_eq!(s.engine.position(), steps + 1, "one undo step");
        let got = s.active_layer_tiles().and_then(|m| m.get(&(1, 1)).cloned());
        assert_eq!(got.expect("pasted in place").rgba, src.rgba);
        s.undo();
        assert!(s.active_layer_tiles().is_none_or(|m| m.get(&(1, 1)).is_none_or(|t| t.is_empty())));
    }

    #[test]
    fn lighttable_config_keeps_the_onion_and_the_light_table() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let mut cfg = Config::default();
        cfg.onion = OnionCfg {
            before: 3,
            after: 2,
            mode: OnionMode::Keys,
            other_columns: true,
            opacity: 0.6,
            falloff: 0.5,
        };
        cfg.light_table.opacity = 0.7;
        cfg.save_to(dir.path()).expect("saved");
        let (back, _) = Config::load_from(dir.path());
        assert_eq!(back.onion, cfg.onion);
        assert_eq!(back.light_table, cfg.light_table);
    }

    #[test]
    fn lighttable_vector_ghosts_look_past_the_held_drawing() {
        let (mut h, d) = keyed(&[0, 4]);
        let s = h.state_mut();
        let col = s.view.active_column;
        let vector = |s: &AppState, f: u32, cfg: &OnionCfg| -> Vec<(DrawingId, i32)> {
            vector_onion_set(s.cut(), col, f, cfg).into_iter().map(|(d, o, _)| (d, o)).collect()
        };
        let ghosts = |s: &AppState, f: u32, cfg: &OnionCfg| -> Vec<DrawingId> {
            onion_ghosts(s, col, f, cfg).into_iter().map(|(d, _)| d).collect()
        };
        let cfg = OnionCfg::default();
        // Mid-hold the GPU slot ghosts the held drawing behind (the cel you
        // draw the next one over); the vector path draws it as the frame
        // itself, so its ghost behind is the drawing before.
        assert_eq!(set_at(s, 5, &cfg), vec![(d[1], -1)]);
        assert_eq!(vector(s, 5, &cfg), vec![(d[0], -1)]);
        assert_eq!(ghosts(s, 5, &cfg), vec![d[0]]);
        // On the key itself the two agree.
        assert_eq!(set_at(s, 4, &cfg), vec![(d[0], -1)]);
        assert_eq!(vector(s, 4, &cfg), vec![(d[0], -1)]);
        assert_eq!(ghosts(s, 4, &cfg), vec![d[0]]);
        // Keys mode looks past it the same way.
        for k in [d[0], d[1]] {
            s.set_drawing_kind(k, DrawingKind::Key);
        }
        let keys = depth(1, 1, OnionMode::Keys);
        assert_eq!(set_at(s, 5, &keys), vec![(d[1], -1)]);
        assert_eq!(vector(s, 5, &keys), vec![(d[0], -1)]);
        assert_eq!(ghosts(s, 5, &keys), vec![d[0]]);
    }

    #[test]
    fn lighttable_onion_set_matrix_every_mode_depth_and_side() {
        let (mut h, d) = keyed(&[0, 2, 4, 6, 8, 10, 12]);
        let s = h.state_mut();
        let a = s.view.active_column;
        for k in [d[0], d[2], d[4], d[6]] {
            s.set_drawing_kind(k, DrawingKind::Key);
        }
        s.add_column();
        s.goto(0);
        s.new_drawing_at_frame();
        let e = s.current_drawing().expect("column B's drawing");
        use OnionMode::{Drawings, Frames, Keys};
        // Frame 6 shows d3 (not a key); d0, d2, d4 and d6 are keys.
        let table = [
            (Drawings, (0, 0), vec![]),
            (Drawings, (1, 1), vec![(d[2], -1), (d[4], 1)]),
            (Drawings, (2, 0), vec![(d[2], -1), (d[1], -2)]),
            (Drawings, (0, 2), vec![(d[4], 1), (d[5], 2)]),
            (
                Drawings,
                (5, 5),
                vec![(d[2], -1), (d[1], -2), (d[0], -3), (d[4], 1), (d[5], 2), (d[6], 3)],
            ),
            (Frames, (0, 0), vec![]),
            (Frames, (1, 1), vec![(d[2], -1)]),
            (Frames, (5, 5), vec![(d[2], -1), (d[1], -3), (d[0], -5), (d[4], 2), (d[5], 4)]),
            (Keys, (0, 0), vec![]),
            (Keys, (1, 1), vec![(d[2], -1), (d[4], 1)]),
            (Keys, (5, 5), vec![(d[2], -1), (d[0], -2), (d[4], 1), (d[6], 2)]),
        ];
        for (mode, (before, after), want) in table {
            for others in [false, true] {
                let cfg = OnionCfg {
                    other_columns: others,
                    ..depth(before, after, mode)
                };
                let got: Vec<(DrawingId, i32)> =
                    onion_set(s.cut(), a, 6, &cfg).into_iter().map(|(d, o, _)| (d, o)).collect();
                // Column B holds its one drawing over frame 6: it ghosts one
                // behind, never ahead (it is held there), and never in keys
                // mode (it is not marked).
                let mut want = want.clone();
                if others && before > 0 && mode != Keys {
                    want.push((e, -1));
                }
                assert_eq!(got, want, "{mode:?} {before}/{after}, other columns {others}");
            }
        }
    }

    #[test]
    fn lighttable_onion_gpu_budget_is_three_textures_and_no_new_shaders() {
        let (h, d) = keyed(&[0, 2, 4, 6, 8, 10, 12]);
        let s = h.state();
        let set = onion_set(s.cut(), s.view.active_column, 6, &depth(5, 5, OnionMode::Drawings));
        let pins: Vec<(DrawingId, PinShift)> =
            [d[0], d[3], d[6]].into_iter().map(|p| (p, PinShift::default())).collect();
        // Depth 5 on both sides and three pins: still three textures.
        let plan = onion_slot_plan(&set, &pins);
        assert_eq!(crate::paint::ONION_SLOTS, 3);
        let ids = |slot: &[(DrawingId, f32)]| slot.iter().map(|x| x.0).collect::<Vec<_>>();
        // Farthest first, so the nearest lies on top.
        assert_eq!(ids(&plan[0]), vec![d[0], d[1], d[2]]);
        assert_eq!(ids(&plan[1]), vec![d[6], d[5], d[4]]);
        assert_eq!(ids(&plan[2]), vec![d[0], d[3], d[6]]);
        // Each ghost weighs against its side's nearest (the slot's tint
        // carries that one's opacity).
        let f = OnionCfg::default().falloff;
        for slot in &plan[..2] {
            let k: Vec<f32> = slot.iter().map(|x| x.1).collect();
            assert!(k.iter().zip([f * f, f, 1.0]).all(|(a, b)| (a - b).abs() < 1e-5), "{k:?}");
        }
        assert!(plan[2].iter().all(|x| x.1 == 1.0));
        // No new shader: the slots go through the paint pipeline's onion
        // pass (the needles are split so this file never counts itself).
        let (mut modules, mut stages, mut wgsl) = (0, 0, 0);
        let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        for entry in std::fs::read_dir(src).expect("the app's sources") {
            let path = entry.expect("a source entry").path();
            match path.extension().and_then(|x| x.to_str()) {
                Some("rs") => {
                    let text = std::fs::read_to_string(&path).expect("a source file");
                    modules += text.matches(concat!("create_shader", "_module(")).count();
                    for stage in ["vertex", "fragment", "compute"] {
                        stages += text.matches(&format!("@{stage}")).count();
                    }
                }
                Some("wgsl") => wgsl += 1,
                _ => {}
            }
        }
        assert!((1..=4).contains(&modules), "{modules} shader modules (4 before GM6)");
        assert!(stages <= 8, "{stages} shader entry points (8 before GM6)");
        assert_eq!(wgsl, 0, "no .wgsl files");
    }

    #[test]
    fn lighttable_make_inbetweens_refuses_rather_than_clear_a_drawing_between() {
        let (mut h, d) = keyed(&[0, 8]);
        let s = h.state_mut();
        let col = s.view.active_column;
        s.set_chart(d[0], &[0.25, 0.5, 0.75]);
        // A breakdown drawn between the keys after the chart was set.
        s.goto(4);
        s.new_drawing_at_frame();
        let bd = s.current_drawing().expect("the breakdown");
        s.set_drawing_kind(bd, DrawingKind::Breakdown);
        s.goto(0);
        let (count, steps) = (s.cut().drawings.len(), s.engine.position());
        assert!(!s.make_inbetweens());
        assert!(s.refusal.contains("breakdown"), "{}", s.refusal);
        let key = |s: &AppState, f: u32| s.cut().xsheet.column(col).unwrap().key_at(f);
        assert_eq!(key(s, 4), Some(Exposure::Drawing(bd)), "the breakdown stays");
        assert_eq!((s.cut().drawings.len(), s.engine.position()), (count, steps));
        // Charted toward the breakdown instead, it fills.
        s.set_chart(d[0], &[0.5]);
        assert_eq!(s.cut().drawing_def(d[0]).chart.map(|c| c.to), Some(bd));
        assert!(s.make_inbetweens(), "{}", s.refusal);
        let frames: Vec<u32> = s.cut().xsheet.column(col).unwrap().keys().map(|(f, _)| f).collect();
        assert_eq!(frames, vec![0, 2, 4, 8]);
        assert_eq!(s.cut().drawing_kind(bd), DrawingKind::Breakdown);
        // A blank cell between is the animator's too.
        s.goto(4);
        s.set_chart(bd, &[0.5]);
        let (at, column) = (s.at(), col);
        let blank = Some(Exposure::Empty);
        s.engine
            .apply("blank", vec![Command::SetCell { at, column, frame: 6, key: blank }])
            .expect("a blank cell");
        let steps = s.engine.position();
        assert!(!s.make_inbetweens());
        assert!(s.refusal.contains("blank"), "{}", s.refusal);
        assert_eq!(key(s, 6), Some(Exposure::Empty));
        assert_eq!(s.engine.position(), steps);
    }

    #[test]
    fn lighttable_a_chart_points_from_the_exposure_under_the_cursor() {
        // A at 0, B at 4, A again at 8, X at 12, B again at 16.
        let (mut h, d) = keyed(&[0, 4]);
        let (a, b) = (d[0], d[1]);
        let s = h.state_mut();
        let col = s.view.active_column;
        expose(s, 8, a);
        s.goto(12);
        s.new_drawing_at_frame();
        let x = s.current_drawing().expect("X");
        expose(s, 16, b);
        s.goto(0);
        s.set_chart(a, &[0.5]);
        assert_eq!(s.cut().drawing_def(a).chart.map(|c| c.to), Some(b));
        // On A's second exposure that chart's key (B) is not next: X is,
        // and X is never cleared.
        s.goto(8);
        let steps = s.engine.position();
        assert!(!s.make_inbetweens());
        assert!(s.refusal.contains("sits between these keys"), "{}", s.refusal);
        assert_eq!(s.resolve_at(col, 12), Some(x));
        assert_eq!(s.engine.position(), steps);
        // Charted from here, it points at X and fills 8..12.
        s.set_chart(a, &[0.5]);
        assert_eq!(s.cut().drawing_def(a).chart.map(|c| c.to), Some(x));
        assert!(s.make_inbetweens(), "{}", s.refusal);
        let ib = s.resolve_at(col, 10).expect("the inbetween");
        assert_eq!(s.cut().drawing_kind(ib), DrawingKind::Inbetween);
        assert_eq!((s.resolve_at(col, 12), s.resolve_at(col, 16)), (Some(x), Some(b)));
    }

    #[test]
    fn lighttable_chart_editor_marks_any_count_and_seven_land_on_ones() {
        for spacing in ChartSpacing::ALL {
            for n in [1, 3, 7, CHART_MAX_MARKS] {
                let m = chart_marks(spacing, n);
                assert_eq!(m.len(), n, "{spacing:?} {n}");
                assert!(m.windows(2).all(|w| w[0] < w[1]), "{spacing:?}: {m:?}");
                assert!(m.iter().all(|t| *t > 0.0 && *t < 1.0), "{spacing:?}: {m:?}");
            }
        }
        assert_eq!(chart_marks(ChartSpacing::Even, 1), vec![0.5]);
        assert_eq!(chart_marks(ChartSpacing::Even, 99).len(), CHART_MAX_MARKS);
        assert_eq!(chart_marks(ChartSpacing::Even, 0).len(), 1);
        // Mae-zume closes up toward this key, ato-zume toward the next.
        let start = chart_marks(ChartSpacing::FavourStart, 3);
        let end = chart_marks(ChartSpacing::FavourEnd, 3);
        assert!(start[1] - start[0] < start[2] - start[1], "{start:?}");
        assert!(end[2] - end[1] < end[1] - end[0], "{end:?}");
        // A dragged mark stays strictly between its neighbours and the keys.
        let q = [0.25, 0.5, 0.75];
        let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert_eq!(move_chart_mark(&q, 1, 0.6), vec![0.25, 0.6, 0.75]);
        assert!(near(move_chart_mark(&q, 0, -3.0)[0], 0.005));
        assert!(near(move_chart_mark(&q, 1, 0.9)[1], 0.745));
        assert!(near(move_chart_mark(&q, 2, 2.0)[2], 0.995));
        assert_eq!(move_chart_mark(&q, 5, 0.1), q.to_vec());
        assert_eq!(move_chart_mark(&q, 1, f32::NAN), q.to_vec());
        // Seven marks between keys eight frames apart: douga on ones (the
        // marks space the drawing, the frames split evenly).
        let (mut h, d) = keyed(&[0, 8]);
        let s = h.state_mut();
        let col = s.view.active_column;
        s.set_chart(d[0], &chart_marks(ChartSpacing::FavourEnd, 7));
        assert!(s.make_inbetweens(), "{}", s.refusal);
        let frames: Vec<u32> = s.cut().xsheet.column(col).unwrap().keys().map(|(f, _)| f).collect();
        assert_eq!(frames, (0..=8).collect::<Vec<u32>>());
        for f in 1..8 {
            let ib = s.resolve_at(col, f).expect("an inbetween");
            assert_eq!(s.cut().drawing_kind(ib), DrawingKind::Inbetween, "frame {f}");
        }
    }

    #[test]
    fn lighttable_system_copy_and_paste_events_reach_the_canvas() {
        let (mut h, _) = keyed(&[0, 8]);
        ink_tile(&mut h, (1, 1), 0.5);
        let src = h.state().active_layer_tiles().and_then(|m| m.get(&(1, 1)).cloned());
        let src = src.expect("the inked tile");
        let square = [(56.0, 56.0), (136.0, 56.0), (136.0, 136.0), (56.0, 136.0)];
        let poly = square.iter().map(|&(x, y)| egui::pos2(x, y)).collect();
        h.editor_mut().canvas.set_selection(poly);
        h.point_at(canvas_center(&h));
        // Ctrl+C reaches the app as a Copy event, with no key event.
        h.event(egui::Event::Copy);
        h.run();
        assert!(h.state().pixel_clip.is_some(), "{}", h.state().refusal);
        let text = h.state().pixel_clip_text.clone().expect("the system clipboard's text");
        // Text copied elsewhere is not pixels.
        let steps = h.state().engine.position();
        h.event(egui::Event::Paste("a line of text".into()));
        assert!(h.state().refusal.contains("not pixels"), "{}", h.state().refusal);
        assert_eq!(h.state().engine.position(), steps);
        // Ctrl+V hands the copy's text back (the system's line end on it):
        // the pixels paste in place on frame 9's drawing.
        h.state_mut().goto(8);
        h.run();
        h.event(egui::Event::Paste(format!("{text}\r\n")));
        assert_eq!(h.state().engine.position(), steps + 1, "one undo step");
        let got = h.state().active_layer_tiles().and_then(|m| m.get(&(1, 1)).cloned());
        assert_eq!(got.expect("pasted in place").rgba, src.rgba);
    }

    #[test]
    fn lighttable_shift_and_trace_says_the_pen_rests_and_a_click_ends_it() {
        let (mut h, d) = keyed(&[0, 4]);
        h.perform(Action::PinToLightTable);
        assert!(h.state().view.light.is_pinned(d[0]));
        h.state_mut().goto(4);
        h.run();
        let chip = "shift & trace: pen resting (click to end)";
        assert!(!h.has(chip));
        // Alt+T latches it, in both presets.
        h.key_mods(egui::Modifiers::ALT, egui::Key::T);
        assert!(h.state().view.light.shift_trace, "{}", h.describe());
        assert!(h.has(chip), "{}", h.describe());
        h.click(chip);
        assert!(!h.state().view.light.shift_trace);
        assert!(!h.has(chip));
        assert!(h.state().status.contains("the pen draws again"), "{}", h.state().status);
    }

    #[test]
    fn lighttable_a_cells_context_menu_marks_its_drawing_kind() {
        let (mut h, d) = keyed(&[0, 4]);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::XSheet]);
        h.run();
        h.run();
        let cell = |h: &EditorHarness, f: u32| h.state().sheet_ui.cell_center(0, f).expect("the sheet's rows");
        let steps = h.state().engine.position();
        // A held cell marks the drawing held there.
        h.right_click_pos(cell(&h, 2));
        h.click("genga (key)");
        assert_eq!(h.state().cut().drawing_kind(d[0]), DrawingKind::Key);
        assert_eq!(h.state().engine.position(), steps + 1, "one undo step");
        assert!(h.has_text_containing("at frame 1: genga (key)"), "{}", h.describe());
        // The cursor's own cell (the typed entry sits on it) answers too.
        h.right_click_pos(cell(&h, 0));
        h.click("breakdown");
        assert_eq!(h.state().cut().drawing_kind(d[0]), DrawingKind::Breakdown);
        h.right_click_pos(cell(&h, 6));
        h.click("douga (inbetween)");
        assert_eq!(h.state().cut().drawing_kind(d[1]), DrawingKind::Inbetween);
        assert_eq!(h.state().cut().drawing_kind(d[0]), DrawingKind::Breakdown);
        assert_eq!(h.state().engine.position(), steps + 3);
    }

    #[test]
    fn lighttable_the_chart_shows_on_the_paper_of_its_key() {
        let (mut h, d) = keyed(&[0, 8]);
        let said = "spacing chart on the paper: 3 marks toward";
        assert!(!h.has_text_containing(said));
        h.state_mut().set_chart(d[0], &[0.25, 0.5, 0.75]);
        h.run();
        let to = h.state().cut().drawing(d[1]).map(|x| x.name.clone()).expect("the next key");
        assert!(h.has_text_containing(&format!("{said} {to}")), "{}", h.describe());
        // Over the key's hold too; the next key has no chart of its own.
        h.state_mut().goto(5);
        h.run();
        assert!(h.has_text_containing(said));
        h.state_mut().goto(8);
        h.run();
        assert!(!h.has_text_containing(said));
    }

    #[test]
    fn lighttable_removing_a_drawing_undoes_with_its_kind_and_chart() {
        let (mut h, d) = keyed(&[0, 8]);
        let s = h.state_mut();
        let col = s.view.active_column;
        s.set_chart(d[0], &[0.25, 0.5, 0.75]);
        let def = s.cut().drawing_def(d[0]);
        assert_eq!((def.kind, def.chart.as_ref().map(|c| c.to)), (DrawingKind::Key, Some(d[1])));
        s.remove_drawing_from_library(d[0]);
        assert!(s.cut().drawing(d[0]).is_none());
        assert_eq!(s.cut().drawing_def(d[0]), Default::default(), "the definition goes with it");
        s.undo();
        assert!(s.cut().drawing(d[0]).is_some());
        assert_eq!(s.cut().drawing_def(d[0]), def, "and comes back with it");
        assert_eq!(s.cut().xsheet.column(col).unwrap().key_at(0), Some(Exposure::Drawing(d[0])));
        s.redo();
        assert_eq!(s.cut().drawing_def(d[0]), Default::default());
        s.undo();
        assert_eq!(s.cut().drawing_def(d[0]), def);
    }

    #[test]
    fn lighttable_duplicate_cel_keeps_the_kind_under_the_next_free_number() {
        let (mut h, d) = keyed(&[0, 8]);
        let s = h.state_mut();
        let col = s.view.active_column;
        s.set_chart(d[0], &[0.5]);
        let def = s.cut().drawing_def(d[0]);
        // Column A shows two drawings: the copy is the third.
        let want = s.next_drawing_name();
        assert_eq!(want, "D3A");
        s.duplicate_cel();
        let dup = s.resolve_at(col, 0).expect("the copy");
        assert_ne!(dup, d[0]);
        assert_eq!(s.cut().drawing(dup).map(|x| x.name.as_str()), Some("D3A"));
        assert_eq!(s.cut().drawing_def(dup), def, "the copy is what the original is");
        s.undo();
        assert!(s.cut().drawing(dup).is_none());
        assert_eq!(s.cut().drawing_def(dup), Default::default());
        assert_eq!(s.cut().drawing_def(d[0]), def);
    }

    #[test]
    fn lighttable_inbetween_names_skip_taken_numbers_and_never_list_the_span() {
        let taken: HashSet<String> = ["5".to_string()].into();
        assert_eq!(inbetween_names("1", "9", 3, &taken), ["3", "6", "7"]);
        // "05" is a different name from 5: only the plain number holds it.
        let padded: HashSet<String> = ["05".to_string()].into();
        assert_eq!(inbetween_names("1", "9", 1, &padded), ["5"]);
        // Keys a hundred billion apart pick without listing the span.
        let none = HashSet::new();
        assert_eq!(inbetween_names("1", "100000000001", 1, &none), ["50000000001"]);
        // The largest number never overflows: no room between, so letters.
        let max = u64::MAX.to_string();
        assert_eq!(inbetween_names(&max, &max, 2, &none), [format!("{max}a"), format!("{max}b")]);
    }

    #[test]
    fn lighttable_the_chart_editor_lays_and_drags_the_marks() {
        let (mut h, d) = keyed(&[0, 8]);
        // The lightbox fold-out slides out at the canvas's left edge; its
        // genga · douga section opens on a click.
        let edge = {
            let r = h.editor().canvas.dbg_rect;
            egui::pos2(r.left() + 4.0, r.top() + 4.0)
        };
        h.point_at(edge);
        h.wait(0.5);
        h.click("genga · douga");
        h.click("genga (key)");
        assert_eq!(h.state().cut().drawing_kind(d[0]), DrawingKind::Key);
        // Open, the section runs past the harness's short paper: the
        // fold-out scrolls to what is below its foot.
        h.scroll_to("ato-zume (slow in)");
        h.click("ato-zume (slow in)");
        let chart = h.state().cut().drawing_def(d[0]).chart.expect("a chart");
        assert_eq!((chart.to, chart.marks.clone()), (d[1], chart_marks(ChartSpacing::FavourEnd, 3)));
        // Drag the middle mark up to 0.6 of the way: one undo step.
        let steps = h.state().engine.position();
        let editor = "spacing chart: 3 marks (drag a mark to move it)";
        h.scroll_to(editor);
        let r = h.rect(editor);
        let paper = h.editor().canvas.dbg_rect;
        assert!(paper.contains_rect(r), "the chart editor shows: {r:?} in {paper:?}");
        let at = |t: f32| egui::pos2(r.center().x, r.top() + 8.0 + (r.height() - 16.0) * t);
        h.drag(at(chart.marks[1]), at(0.6), 6);
        let moved = h.state().cut().drawing_def(d[0]).chart.expect("the chart").marks;
        assert_eq!(h.state().engine.position(), steps + 1, "one undo step");
        assert!((moved[1] - 0.6).abs() < 1e-3, "{moved:?}");
        assert_eq!((moved[0], moved[2]), (chart.marks[0], chart.marks[2]));
        // And its inbetweens land between the keys.
        h.scroll_to("make inbetweens");
        h.click("make inbetweens");
        let col = h.state().view.active_column;
        let frames: Vec<u32> = h.state().cut().xsheet.column(col).unwrap().keys().map(|(f, _)| f).collect();
        assert_eq!(frames, vec![0, 2, 4, 6, 8]);
    }

    #[test]
    fn lighttable_shift_tiles_turns_a_sparse_cel_texel_for_texel() {
        // Two inked tiles far apart, turned and moved: skipping the empty
        // bounds and caching the lookups change no texel of the plain map.
        let tile = |v: f32| {
            let mut px = vec![0u16; TILE_LEN];
            for (i, x) in px.iter_mut().enumerate() {
                // A ramp, so a texel read from the wrong place shows.
                *x = f32_to_f16_bits(v * (i % 97) as f32 / 97.0);
            }
            Arc::new(TileData::from_vec(px))
        };
        let mut tiles = BTreeMap::new();
        tiles.insert((2, 1), tile(0.5));
        tiles.insert((12, 7), tile(1.0));
        let center = egui::pos2(960.0, 540.0);
        let shift = PinShift {
            dx: 37.0,
            dy: -11.0,
            rot: 0.3,
        };
        let got = shift_tiles(&tiles, shift, center);
        let t = TILE as i32;
        let mut want = BTreeMap::new();
        for ty in -8..28 {
            for tx in -8..40 {
                let mut px = vec![0u16; TILE_LEN];
                let mut any = false;
                for row in 0..TILE {
                    for col in 0..TILE {
                        let q = egui::pos2(
                            (tx * t + col as i32) as f32 + 0.5,
                            (ty * t + row as i32) as f32 + 0.5,
                        );
                        let s = shift.invert(q, center);
                        let (sx, sy) = (s.x.floor() as i32, s.y.floor() as i32);
                        if let Some(src) = tiles.get(&(sx.div_euclid(t), sy.div_euclid(t))) {
                            let si = (sy.rem_euclid(t) * t + sx.rem_euclid(t)) as usize * 4;
                            let di = (row * TILE + col) * 4;
                            px[di..di + 4].copy_from_slice(&src.rgba[si..si + 4]);
                            any |= src.rgba[si + 3] != 0;
                        }
                    }
                }
                if any {
                    want.insert((tx, ty), px);
                }
            }
        }
        assert!(want.len() >= 2, "{:?}", want.keys().collect::<Vec<_>>());
        assert_eq!(got.keys().collect::<Vec<_>>(), want.keys().collect::<Vec<_>>());
        for (k, px) in &want {
            assert!(got[k].rgba[..] == px[..], "tile {k:?}");
        }
    }
}
