//! App-side document state: the Engine plus the user's current position in it
//! (active cut, column, frame, selection) and transport/playback state.
//! All document mutations route through Engine commands — undo covers
//! everything the artist does here.

use std::path::PathBuf;

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::ids::*;
use anim_core::model::{Cut, Stroke};
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

    /// The drawing under the pen right now (active column @ current frame).
    pub fn current_drawing(&self) -> Option<DrawingId> {
        self.resolve_at(self.active_column, self.frame)
    }

    fn next_drawing_name(&self) -> String {
        format!("D{}", self.cut().drawings.len() + 1)
    }

    // ---- Editing ----------------------------------------------------------

    /// Commit a finished pen stroke. If the current cell is empty, a new
    /// drawing is created and exposed here first — create + expose + stroke
    /// is ONE undo step, so Ctrl+Z after drawing on an empty frame leaves
    /// no orphan drawing behind.
    pub fn commit_stroke(&mut self, stroke: Stroke) {
        let at = self.at();
        match self.current_drawing() {
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
                            raster: None,
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

    /// Create an empty drawing and expose it at the current frame.
    pub fn new_drawing_at_frame(&mut self) {
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
                    raster: None,
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
