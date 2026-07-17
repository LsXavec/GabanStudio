//! App-side document state: the Engine plus the user's current position in it
//! (active cut, column, frame, selection) and transport/playback state.
//! All document mutations route through Engine commands — undo covers
//! everything the artist does here.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::ids::*;
use anim_core::model::{Cut, Stroke};
use anim_core::raster::{RasterLayer, TileCoord, TileData, TileDiff};
use anim_core::xsheet::Exposure;

pub struct AppState {
    pub engine: Engine,
    pub scene: SceneId,
    pub cut: CutId,
    pub active_column: ColumnId,
    pub frame: u32,
    pub selected_drawing: Option<DrawingId>,
    pub playing: bool,
    play_acc: f32,
    pub onion: bool,
    pub file_path: Option<PathBuf>,
    pub status: String,
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
            scene,
            cut,
            active_column,
            frame: 0,
            selected_drawing: None,
            playing: false,
            play_acc: 0.0,
            onion: true,
            file_path: None,
            status: "new project — draw on the canvas to create frame 1".into(),
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
            scene: scene.id,
            cut: cut.id,
            active_column: column.id,
            frame: 0,
            selected_drawing: None,
            playing: false,
            play_acc: 0.0,
            onion: true,
            file_path: Some(path),
            status: "project loaded".into(),
            engine,
        })
    }

    // ---- Accessors --------------------------------------------------------

    pub fn at(&self) -> CutRef {
        CutRef {
            scene: self.scene,
            cut: self.cut,
        }
    }

    pub fn cut(&self) -> &Cut {
        self.engine
            .project
            .cut(self.scene, self.cut)
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
        self.resolve_at(self.active_column, self.frame)
    }

    /// The drawing keyed at *exactly* this frame (not a hold from an earlier
    /// frame). Editing targets this: drawing on a held/empty frame makes a NEW
    /// cel here rather than editing the held drawing — standard frame-by-frame.
    pub fn own_key_drawing(&self) -> Option<DrawingId> {
        match self
            .cut()
            .xsheet
            .column(self.active_column)?
            .key_at(self.frame)
        {
            Some(Exposure::Drawing(d)) => Some(d),
            _ => None,
        }
    }

    /// Per-column drawing name, e.g. D1A, D2A on column A; D1B on column B.
    fn next_drawing_name(&self) -> String {
        match self.cut().xsheet.column(self.active_column) {
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
                let r = self.engine.apply(
                    "draw (new drawing)",
                    vec![
                        Command::AddDrawing {
                            at,
                            id,
                            name,
                            strokes: vec![],
                            raster: Some(RasterLayer::empty()),
                        },
                        Command::SetCell {
                            at,
                            column: self.active_column,
                            frame: self.frame,
                            key: Some(Exposure::Drawing(id)),
                        },
                        Command::AddStroke { at, id, stroke },
                    ],
                );
                if r.is_ok() {
                    self.selected_drawing = Some(id);
                }
                self.report(r, "new drawing");
            }
        }
    }

    /// Commit a finished raster stroke: the readback tiles become a `PaintTiles`
    /// edit (undoable + persisted). If the current cell is empty, a new raster
    /// cel is created and exposed here first — all as ONE undo step. Returns the
    /// drawing the paint landed on (None if it couldn't be committed).
    pub fn commit_raster(&mut self, new_tiles: Vec<(TileCoord, Arc<TileData>)>) -> Option<DrawingId> {
        let at = self.at();
        match self.own_key_drawing() {
            None => {
                let name = self.next_drawing_name();
                let id = self.engine.alloc_drawing_id();
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
                            name,
                            strokes: vec![],
                            raster: Some(RasterLayer::empty()),
                        },
                        Command::SetCell {
                            at,
                            column: self.active_column,
                            frame: self.frame,
                            key: Some(Exposure::Drawing(id)),
                        },
                        Command::PaintTiles { at, id, diff },
                    ],
                );
                if r.is_ok() {
                    self.selected_drawing = Some(id);
                }
                self.report(r, "painted (new cel)");
                Some(id)
            }
            Some(id) => {
                // Snapshot the drawing's current tiles as the diff "before".
                let before: BTreeMap<TileCoord, Arc<TileData>> = match self
                    .cut()
                    .drawing(id)
                    .and_then(|d| d.raster.as_ref())
                {
                    Some(raster) => raster.tiles.clone(),
                    None => {
                        self.status =
                            "this cel is vector-only; raster paint isn't wired here yet".into();
                        return None;
                    }
                };
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
                    return Some(id);
                }
                let r = self.engine.apply("paint", vec![Command::PaintTiles { at, id, diff }]);
                self.report(r, "painted");
                Some(id)
            }
        }
    }

    /// Clear this frame's own cel raster (undoable). No-op on a held frame that
    /// doesn't own a cel, so clearing never wipes a drawing shared by others.
    pub fn clear_current_raster(&mut self) {
        let Some(id) = self.own_key_drawing() else {
            return;
        };
        let at = self.at();
        let before: BTreeMap<TileCoord, Arc<TileData>> =
            match self.cut().drawing(id).and_then(|d| d.raster.as_ref()) {
                Some(raster) if !raster.tiles.is_empty() => raster.tiles.clone(),
                _ => return,
            };
        let diff: TileDiff = before
            .into_iter()
            .map(|(c, b)| (c, Some(b), None))
            .collect();
        let r = self.engine.apply("clear cel", vec![Command::PaintTiles { at, id, diff }]);
        self.report(r, "cel cleared");
    }

    /// Nearest distinct drawings before / after the current frame on the active
    /// column (for onion skin). [0] = previous, [1] = next.
    pub fn onion_neighbors(&self) -> [Option<DrawingId>; 2] {
        let cur = self.current_drawing();
        let Some(col) = self.cut().xsheet.column(self.active_column) else {
            return [None, None];
        };
        let mut prev = None;
        for f in (0..self.frame).rev() {
            if let Some(d) = col.resolve(f)
                && Some(d) != cur {
                    prev = Some(d);
                    break;
                }
        }
        let mut next = None;
        for f in (self.frame + 1)..self.frame_count() {
            if let Some(d) = col.resolve(f)
                && Some(d) != cur {
                    next = Some(d);
                    break;
                }
        }
        [prev, next]
    }

    /// Raster tiles + content hash of a specific drawing (for onion upload).
    pub fn drawing_raster(
        &self,
        id: DrawingId,
    ) -> Option<(&BTreeMap<TileCoord, Arc<TileData>>, u64)> {
        let r = self.cut().drawing(id)?.raster.as_ref()?;
        Some((&r.tiles, r.content_hash()))
    }

    /// The raster tiles of the current cel (for uploading to the GPU layer).
    pub fn current_raster_tiles(&self) -> Option<&BTreeMap<TileCoord, Arc<TileData>>> {
        let id = self.current_drawing()?;
        self.cut().drawing(id)?.raster.as_ref().map(|r| &r.tiles)
    }

    /// Identity of the current cel's raster (drawing id, raster content hash) —
    /// used to decide when the GPU layer needs re-syncing.
    pub fn current_raster_key(&self) -> (u64, u64) {
        match self.current_drawing() {
            Some(id) => {
                let h = self
                    .cut()
                    .drawing(id)
                    .and_then(|d| d.raster.as_ref())
                    .map(|r| r.content_hash())
                    .unwrap_or(0);
                (id.0, h)
            }
            None => (0, 0),
        }
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
        let r = self.engine.apply(
            "new drawing",
            vec![
                Command::AddDrawing {
                    at,
                    id,
                    name,
                    strokes: vec![],
                    raster: Some(RasterLayer::empty()),
                },
                Command::SetCell {
                    at,
                    column: self.active_column,
                    frame: self.frame,
                    key: Some(Exposure::Drawing(id)),
                },
            ],
        );
        if r.is_ok() {
            self.selected_drawing = Some(id);
        }
        self.report(r, "new drawing");
    }

    /// Expose the selected library drawing at the current frame (a hold key).
    pub fn expose_selected(&mut self) {
        let Some(id) = self.selected_drawing else {
            self.status = "select a drawing in the library first".into();
            return;
        };
        let at = self.at();
        let r = self.engine.apply(
            "expose drawing",
            vec![Command::SetCell {
                at,
                column: self.active_column,
                frame: self.frame,
                key: Some(Exposure::Drawing(id)),
            }],
        );
        self.report(r, "exposed");
    }

    /// Clear the key at the current frame (previous hold extends over it).
    pub fn clear_key_at_frame(&mut self) {
        let at = self.at();
        let r = self.engine.apply(
            "clear key",
            vec![Command::SetCell {
                at,
                column: self.active_column,
                frame: self.frame,
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
                self.active_column = id;
                self.status = "column added".into();
            }
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    /// Remove the selected (active) column. Keeps at least one column.
    pub fn remove_active_column(&mut self) {
        if self.cut().xsheet.columns.len() <= 1 {
            self.status = "can't remove the last column".into();
            return;
        }
        let at = self.at();
        let removed = self.active_column;
        // Pick a neighbour to become active before removing.
        let cols: Vec<ColumnId> = self.cut().xsheet.columns.iter().map(|c| c.id).collect();
        let idx = cols.iter().position(|&c| c == removed).unwrap_or(0);
        let new_active = cols
            .get(idx + 1)
            .or_else(|| idx.checked_sub(1).and_then(|i| cols.get(i)))
            .copied()
            .unwrap_or(removed);
        self.engine.remove_column(at, removed);
        self.active_column = new_active;
        self.selected_drawing = None;
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
        if let Some(id) = self.selected_drawing
            && self.cut().drawing(id).is_none() {
                self.selected_drawing = None;
            }
        self.frame = self.frame.min(self.frame_count() - 1);
    }

    fn report(&mut self, r: anim_core::error::Result<()>, ok_msg: &str) {
        match r {
            Ok(()) => self.status = ok_msg.into(),
            Err(e) => self.status = format!("error: {e}"),
        }
    }

    // ---- Transport --------------------------------------------------------

    pub fn goto(&mut self, frame: u32) {
        self.frame = frame.min(self.frame_count() - 1);
        self.play_acc = 0.0;
    }

    pub fn step(&mut self, delta: i64) {
        let n = self.frame_count() as i64;
        let f = (self.frame as i64 + delta).rem_euclid(n);
        self.goto(f as u32);
    }

    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
        self.play_acc = 0.0;
    }

    pub fn tick(&mut self, dt: f32) {
        if !self.playing {
            return;
        }
        self.play_acc += dt;
        let frame_time = 1.0 / self.fps() as f32;
        while self.play_acc >= frame_time {
            self.play_acc -= frame_time;
            self.frame = (self.frame + 1) % self.frame_count();
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
