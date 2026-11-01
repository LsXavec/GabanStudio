//! THE HEADLESS UI TESTKIT (GM1, D10): drives the REAL editor through egui
//! frames and finds widgets by their AccessKit names — no window, no wgpu
//! device, no audio device, no network, no child process. Each harness
//! owns a fresh temp data dir; nothing here reaches the user's
//! `%APPDATA%/AnimStudio`. Every test carries `testkit` in its name (the
//! part's gate counts them).

// The D10 API is the contract later parts drive; not every verb has a
// test caller yet.
#![allow(dead_code)]

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use eframe::egui;
use eframe::egui_wgpu::{RenderState, RendererOptions};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};

use crate::canvas::CanvasTool;
use crate::config::{self, Action, Config};
use crate::doc::AppState;
use crate::newproject::NewProjectForm;
use crate::workspace::{Stage, Workspace, Workspaces};
use crate::{AppRequest, Editor, Notes, Performed, shortcuts};

/// Ctrl as the platform reports it on Windows/Linux (the chord matcher
/// reads `command`).
pub const CTRL: egui::Modifiers = egui::Modifiers {
    ctrl: true,
    command: true,
    ..egui::Modifiers::NONE
};

/// One real device at work in this process at a time. The kit picks WARP, the
/// CPU adapter, and two harnesses painting side by side die in d3d10warp.dll
/// (an access violation in 1 run of 10); devices that only open and close do
/// not. Without this, `auseam_ -- --ignored` crashes on the default threads.
static GPU_TURN: Mutex<()> = Mutex::new(());

/// A wgpu device with no window and no surface, and the turn that goes with
/// it. Only ignored tests reach it.
fn gpu() -> (RenderState, MutexGuard<'static, ()>) {
    let turn = GPU_TURN.lock().unwrap_or_else(PoisonError::into_inner);
    (create_render_state(default_wgpu_setup(), RendererOptions::PREDICTABLE), turn)
}

/// A primary-button press (or release) at `pos`.
fn primary(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// One touch of a pen, as egui-winit reports it.
fn pen(phase: egui::TouchPhase, pos: egui::Pos2) -> egui::Event {
    egui::Event::Touch {
        device_id: egui::TouchDeviceId(1),
        id: egui::TouchId(1),
        phase,
        pos,
        force: Some(0.5),
    }
}

/// What the harness drives each frame: the editor, the shell's config, the
/// shell's notes, and the log of every shortcut dispatched with what
/// perform returned.
pub struct Rig {
    pub editor: Editor,
    pub config: Config,
    /// The shell's once-per-session notes (the config load's included),
    /// said on the editor's Aka lane the way the app says them.
    pub notes: Notes,
    pub performed: Vec<(Action, Performed)>,
    /// GM2: what the editor asked the shell for (logged, never carried out).
    pub requests: Vec<AppRequest>,
    /// GM4: native pen samples the next frame hands the canvas — the
    /// tablet thread's place (empty = no tablet, as before).
    pub pen: Vec<crate::PenSample>,
    dressed: bool,
    /// AU1: the boot this rig started under; a rebuilt editor keeps it.
    boot: Boot,
    /// AU1: arm rooms the way the App does. Off under `Boot::Frozen`: today's
    /// Rig never arms, and tests about something else depend on that.
    arms: bool,
    /// AU1: the room last armed (the App keeps its own).
    pub stack_room_seen: &'static str,
    /// AU1: a test's seeded fault, painted over the editor every frame so the
    /// audits have something to catch.
    pub seed: Option<Box<dyn FnMut(&mut egui::Ui)>>,
    /// AU1: the rect the editor was last laid out in.
    pub root: egui::Rect,
}

impl Rig {
    /// A `Ui` harness sits in a frame 8 pt in from the window; the App has
    /// none. `Boot::Frozen` keeps the inset, because the tests about
    /// something else and BASELINE were written under it. The other two
    /// boots are the person's window, so they lay out from its edge.
    fn frame(&mut self, ui: &mut egui::Ui) {
        if self.boot == Boot::Frozen {
            return self.drive(ui);
        }
        let window = ui.ctx().content_rect();
        let mut flush = ui.new_child(egui::UiBuilder::new().max_rect(window));
        flush.set_clip_rect(window);
        self.drive(&mut flush);
    }

    fn drive(&mut self, ui: &mut egui::Ui) {
        self.root = ui.max_rect();
        if !self.dressed {
            // The app's own dressing; fonts go live on the next pass, so
            // this first frame draws nothing.
            crate::dress(ui.ctx());
            ui.ctx().all_styles_mut(|s| s.visuals.text_cursor.blink = false);
            self.dressed = true;
            ui.ctx().request_repaint();
            return;
        }
        self.notes.say(&mut self.editor);
        let pen = std::mem::take(&mut self.pen);
        // AU9: the frozen boot keeps every fold open (spec 1.5.2).
        self.editor.more_default = self.boot == Boot::Frozen;
        if self.arms {
            self.editor.arm_room(&self.config.layers, &mut self.stack_room_seen);
        }
        let Rig { editor, config, .. } = self;
        let mut presets_dirty = false;
        // GM6: the onion and light table settings, fed as the App feeds them.
        editor.canvas.onion = config.onion.sane();
        editor.canvas.light_table = config.light_table;
        // GM12: audio scrub (a flip since the last frame stored first) and
        // the RAM preview cap, as the App feeds them.
        if let Some(on) = editor.scrub_request.take() {
            config.perf.scrub_audio = on;
        }
        editor.scrub_audio = config.perf.scrub_audio;
        editor.state.ram.set_cap_mb(config.perf.ram_preview_mb);
        editor.show_start = config.ui.show_welcome;
        editor.say.feed(&config.ui);
        editor.ui(
            ui,
            &config.pen,
            &config.layers,
            &mut config.presets,
            &mut presets_dirty,
            &pen,
            config.ui.lock_positions,
            None,
            &config.keybinds,
        );
        if let Some(seed) = &mut self.seed {
            seed(ui);
        }
        // GM2: the shell's outbox and autosave interval, as the App wires them.
        self.requests.append(&mut self.editor.requests);
        if let Some(on) = self.editor.scrub_request.take() {
            self.config.perf.scrub_audio = on;
        }
        // AU3: the Timing sheet button's choice, saved as the App saves it.
        if let Some(on) = self.editor.sheet_request.take() {
            self.config.ui.sheet_in_draw = on;
            if let Some(dir) = &self.editor.data_dir {
                self.config.save_to(dir).expect("the rig's config saves");
            }
        }
        // AU6: the Start float's latch and the lock's verb, likewise.
        let (show, lock) = (self.editor.show_start_request.take(), self.editor.lock_request.take());
        if let Some(on) = show {
            self.config.ui.show_welcome = on;
        }
        if let Some(on) = lock {
            self.config.ui.lock_positions = on;
        }
        // AU7: Hints, and what the Say line has taught.
        let taught = self.editor.say.store(&mut self.config.ui);
        if let (true, Some(dir)) = (show.is_some() || lock.is_some() || taught, &self.editor.data_dir) {
            self.config.save_to(dir).expect("the rig's config saves");
        }
        // GM4: the OPTIONS row's stabilizer level, saved as the App saves it.
        if let Some(level) = self.editor.canvas.stabilizer_request.take() {
            self.config.pen.stabilizer = level;
        }
        // GM6: and the light box's onion / light table edits.
        if let Some(cfg) = self.editor.canvas.onion_request.take() {
            self.config.onion = cfg;
        }
        if let Some(cfg) = self.editor.canvas.light_table_request.take() {
            self.config.light_table = cfg;
        }
        self.editor.autosave.minutes = self.config.session.autosave_minutes;
        // GM11: the render queue's settings, as the App feeds them (its
        // "show folder" is left for the test to read — nothing opens).
        self.editor
            .state
            .render_queue
            .configure(&self.config.render, &self.config.session.username);
        // The shell's shortcut dispatch, minus the dialogs: requests are
        // logged, never carried out. GM2: it stands down while the "Save
        // changes?" modal is up, as the App's does; GM3: and while the
        // command palette has the keys.
        if self.editor.leave_pending() || self.editor.palette.open {
            return;
        }
        let here = self.editor.shortcut_ctx(ui.ctx());
        for a in shortcuts::collect(ui.ctx(), &self.config.keybinds, here) {
            self.editor.presets.clone_from(&self.config.presets);
            let a = self.editor.aimed(a, ui.ctx());
            let r = self.editor.perform(a);
            self.performed.push((a, r));
        }
    }
}

/// AU1: how a harness starts (the spec's three boots). `Frozen` keeps today's
/// behaviour for every test about something else, so a change of default
/// cannot disturb it; `Shipped` is a fresh install past its first launch, the
/// Start float closed; `FirstRun` is that first launch: the ready paper, with
/// the float open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Boot {
    Frozen,
    Shipped,
    FirstRun,
}

/// The whole editor on a 1280×800 screen.
pub struct EditorHarness {
    h: Harness<'static, Rig>,
    dir: tempfile::TempDir,
    /// The device a GPU harness rebuilds its editor on; None is the suite.
    rs: Option<RenderState>,
    /// Last, so the turn ends after the harness and its device have gone.
    _turn: Option<MutexGuard<'static, ()>>,
}

impl EditorHarness {
    /// A 1920×1080, 24 fps project with one drawing at frame 0, at 100 %.
    pub fn new() -> Self {
        let mut state = AppState::new_project("testkit", 1920, 1080, 24, 300.0);
        state.new_drawing_at_frame();
        Self::with_state(state)
    }

    pub fn with_state(state: AppState) -> Self {
        Self::with_dir(state, tempfile::tempdir().expect("temp data dir"))
    }

    /// Over a data dir the test prepared (a bad config.json, say): config
    /// and workspaces load from it exactly as the app loads them.
    pub fn with_dir(state: AppState, dir: tempfile::TempDir) -> Self {
        Self::build(Some(state), dir, Boot::Frozen, None)
    }

    /// The editor over a real wgpu device with no window: a paint layer, a
    /// compositor, real raster strokes. Only ignored tests call this; the
    /// suite stays free of a GPU device.
    pub fn with_gpu(state: AppState) -> Self {
        Self::build(Some(state), tempfile::tempdir().expect("temp data dir"), Boot::Frozen, Some(gpu()))
    }

    /// A fresh install past its first launch: the ready paper, the Start
    /// float closed.
    pub fn as_shipped() -> Self {
        Self::with_dir_shipped(tempfile::tempdir().expect("temp data dir"))
    }

    /// What a fresh install's first launch shows: the ready paper, and the
    /// Start float over it.
    pub fn as_first_run() -> Self {
        Self::with_dir_first_run(tempfile::tempdir().expect("temp data dir"))
    }

    /// A first launch over a data dir the test prepared: an upgrader's, say.
    pub fn with_dir_first_run(dir: tempfile::TempDir) -> Self {
        Self::build(None, dir, Boot::FirstRun, None)
    }

    /// A first launch's steps, over another document: the sweeps' aspects.
    pub fn with_state_first_run(state: AppState) -> Self {
        Self::build(Some(state), tempfile::tempdir().expect("temp data dir"), Boot::FirstRun, None)
    }

    pub fn with_state_shipped(state: AppState) -> Self {
        Self::build(Some(state), tempfile::tempdir().expect("temp data dir"), Boot::Shipped, None)
    }

    /// The ready paper over a data dir the test prepared.
    pub fn with_dir_shipped(dir: tempfile::TempDir) -> Self {
        Self::build(None, dir, Boot::Shipped, None)
    }

    pub fn with_gpu_shipped(state: AppState) -> Self {
        Self::build(Some(state), tempfile::tempdir().expect("temp data dir"), Boot::Shipped, Some(gpu()))
    }

    /// The ready paper on a real device: the blank Draw's pictures.
    pub fn with_gpu_ready() -> Self {
        Self::build(None, tempfile::tempdir().expect("temp data dir"), Boot::Shipped, Some(gpu()))
    }

    /// The ready paper on a real device, over a data dir the test prepared: an
    /// upgrader's pictures.
    pub fn with_gpu_dir_shipped(dir: tempfile::TempDir) -> Self {
        Self::build(None, dir, Boot::Shipped, Some(gpu()))
    }

    /// A first launch on a real device: the Start float's pictures.
    pub fn with_gpu_first_run() -> Self {
        Self::build(None, tempfile::tempdir().expect("temp data dir"), Boot::FirstRun, Some(gpu()))
    }

    /// THE FUNNEL: every constructor ends here, so no test can start outside
    /// the boot rule. With no document it is the ready paper, entered before
    /// its first frame, as the App enters it.
    fn build(
        state: Option<AppState>,
        dir: tempfile::TempDir,
        boot: Boot,
        gpu: Option<(RenderState, MutexGuard<'static, ()>)>,
    ) -> Self {
        let (rs, _turn) = gpu.unzip();
        let (mut config, config_notes) = Config::load_from(dir.path());
        if boot == Boot::Frozen {
            Self::frozen(&mut config);
        }
        let paper = crate::templates::ANIMATION;
        let ready = state.is_none() || boot == Boot::FirstRun;
        let state = state.unwrap_or_else(|| crate::templates::INTENTS[paper].build());
        let mut editor = Editor::boot(state, rs.as_ref(), Some(dir.path()), &config.ui);
        if ready {
            editor.enter_intent(paper);
        }
        // What the App's `Startup::Welcome` does to the paper it made.
        if boot == Boot::FirstRun {
            crate::start_over(&mut editor, &mut config);
        }
        let mut notes = Notes::default();
        for n in config_notes {
            notes.push(n);
        }
        let rig = Rig {
            editor,
            config,
            notes,
            performed: Vec::new(),
            requests: Vec::new(),
            pen: Vec::new(),
            dressed: false,
            boot,
            arms: boot != Boot::Frozen,
            stack_room_seen: "",
            seed: None,
            root: egui::Rect::NOTHING,
        };
        let builder = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .with_pixels_per_point(1.0)
            .with_max_steps(12)
            .with_wait_for_pending_images(false);
        let builder = match &rs {
            Some(rs) => builder.renderer(WgpuTestRenderer::from_render_state(rs.clone())),
            None => builder,
        };
        let h = builder.build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
        let mut me = Self { h, dir, rs, _turn };
        me.run();
        me
    }

    /// Everything the frozen boot holds still, in one place (with `arms`,
    /// which it switches off). Written into the config before the editor is
    /// built, so a boot reads it. Later parts add a line each time a default
    /// moves.
    fn frozen(config: &mut Config) {
        config.ui.lock_positions = false;
        config.ui.sheet_in_draw = true;
        config.ui.hints_off = true;
    }

    /// A new editor over `state` in this harness's data dir and on its
    /// device, the way the App swaps documents: the rig, and so the boot it
    /// started under, stays.
    pub fn rebuild_editor(&mut self, state: AppState) {
        let rs = self.rs.clone();
        let dir = self.dir.path().to_path_buf();
        let rig = self.h.state_mut();
        rig.editor = Editor::boot(state, rs.as_ref(), Some(&dir), &rig.config.ui);
    }

    pub fn boot(&self) -> Boot {
        self.h.state().boot
    }

    /// AU6: what the App does with a Start choice past the guard: the seam
    /// runs and the result is installed, so a test strokes on the document
    /// that entry really makes. A dialog stays a logged request.
    pub fn answer_start(&mut self, action: crate::welcome::WelcomeAction) {
        let rs = self.rs.clone();
        let dir = self.dir.path().to_path_buf();
        let rig = self.h.state_mut();
        match crate::apply_start(action, rs.as_ref(), Some(&dir), &rig.config.ui) {
            crate::StartOutcome::Replace(editor) => rig.editor = *editor,
            crate::StartOutcome::Request(r) => rig.requests.push(r),
            crate::StartOutcome::Failed(why) => rig.notes.push(why),
            crate::StartOutcome::Stay => {}
        }
        self.run();
    }

    /// AU9: the save dialog's answer: the last logged `ExportTo` goes through
    /// `Editor::export_to` with `path`, as the App sends what a person chose.
    /// No dialog opens; a journey counts this as the dialog's one action.
    pub fn answer_export(&mut self, path: &std::path::Path) {
        let ctx = self.ctx();
        let rig = self.h.state_mut();
        let asked = rig.requests.iter().rposition(|r| matches!(r, crate::AppRequest::ExportTo(_)));
        let asked = asked.unwrap_or_else(|| panic!("no export was asked for: {:?}", rig.requests));
        let crate::AppRequest::ExportTo(kind) = rig.requests.remove(asked) else { unreachable!() };
        rig.editor.export_to(&ctx, kind, path);
        self.run();
    }

    /// AU8: the running export to its end. Only the App polls the queue each
    /// frame, so this drives the same `poll` until no worker is left, with a
    /// ceiling so a stuck job fails instead of hanging the suite.
    pub fn run_export_to_end(&mut self) {
        let ctx = self.ctx();
        let ceiling = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while self.state().render_queue.busy() {
            crate::render_queue::poll(self.state_mut(), &ctx);
            assert!(std::time::Instant::now() < ceiling, "the export never finished");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        self.run();
    }

    /// AU1: the harness itself: the audits read its frame output.
    pub fn kit(&self) -> &Harness<'static, Rig> {
        &self.h
    }

    pub fn kit_mut(&mut self) -> &mut Harness<'static, Rig> {
        &mut self.h
    }

    pub fn with_ppp(mut self, ppp: f32) -> Self {
        self.h.set_pixels_per_point(ppp);
        self.run();
        self
    }

    /// The editor at this size in points, as a PNG: wgpu with no window and
    /// no surface. Only the ignored `uishot` tests call this; the suite
    /// stays free of a GPU device.
    pub fn shot(&mut self, size: egui::Vec2, path: &Path) {
        self.h.set_size(size);
        self.run();
        self.h.step();
        let image = self.h.render().expect("headless render");
        image.save(path).expect("write the shot");
    }

    /// Run until egui stops asking for an immediate repaint (bounded; a
    /// live playhead keeps asking, which is not an error here).
    pub fn run(&mut self) {
        let _ = self.h.try_run();
    }

    pub fn run_frames(&mut self, n: usize) {
        self.h.run_steps(n);
    }

    /// Let `secs` pass on the egui clock in one frame — dwell timers and
    /// fades without stepping through every frame between.
    pub fn wait(&mut self, secs: f64) {
        let now = self.h.ctx.input(|i| i.time);
        self.h.input_mut().time = Some(now + secs);
        self.h.step();
        // The raw input keeps an explicit time: hand the clock back.
        self.h.input_mut().time = None;
        self.run();
    }

    /// Click the ONE widget with this accessible name; none or several is
    /// a panic that prints the whole tree.
    #[track_caller]
    pub fn click(&mut self, label: &str) {
        self.click_at(label, 0, true);
    }

    /// Click the n-th of several widgets sharing a name.
    #[track_caller]
    pub fn click_nth(&mut self, label: &str, n: usize) {
        self.click_at(label, n, false);
    }

    #[track_caller]
    fn click_at(&mut self, label: &str, n: usize, only: bool) {
        let count = self.h.query_all_by_label(label).count();
        if count == 0 || (only && count > 1) || n >= count {
            panic!(
                "click({label:?}): {count} widget(s) carry that name\n{}",
                self.describe()
            );
        }
        self.h.query_all_by_label(label).nth(n).expect("counted").click();
        self.run();
    }

    #[track_caller]
    pub fn hover(&mut self, label: &str) {
        let count = self.h.query_all_by_label(label).count();
        assert!(count == 1, "hover({label:?}): {count} widget(s)\n{}", self.describe());
        self.h.get_by_label(label).hover();
        self.run();
    }

    /// GM6: scroll the ONE widget with this name into view (an AccessKit
    /// ScrollIntoView, as a screen reader asks) — a control below a scroll
    /// area's fold is clicked where it then shows.
    #[track_caller]
    pub fn scroll_to(&mut self, label: &str) {
        let count = self.h.query_all_by_label(label).count();
        assert!(count == 1, "scroll_to({label:?}): {count} widget(s)\n{}", self.describe());
        self.h.get_by_label(label).scroll_to_me();
        self.run();
    }

    /// Give the n-th text field keyboard focus the way Tab or a screen
    /// reader does (an AccessKit Focus action; fields carry no name of
    /// their own). A pointer click would also land, but inside a menu it
    /// closes the menu first.
    #[track_caller]
    pub fn focus_text_field(&mut self, n: usize) {
        let role = egui::accesskit::Role::TextInput;
        let count = self.h.query_all_by_role(role).count();
        assert!(n < count, "text field {n} of {count}\n{}", self.describe());
        self.h.query_all_by_role(role).nth(n).expect("counted").focus();
        self.run();
    }

    /// GM13: click the ONE combo box showing `value` — a combo carries its
    /// value, not a name (`ComboBox = "…"` in [`describe`](Self::describe)).
    #[track_caller]
    pub fn click_combo(&mut self, value: &str) {
        let role = egui::accesskit::Role::ComboBox;
        let shows = |n: &egui_kittest::Node<'_>| n.value().as_deref() == Some(value);
        let count = self.h.query_all_by_role(role).filter(|n| shows(n)).count();
        assert!(count == 1, "click_combo({value:?}): {count} combo box(es)\n{}", self.describe());
        self.h
            .query_all_by_role(role)
            .find(|n| shows(n))
            .expect("counted")
            .click();
        self.run();
    }

    /// Does an egui text field hold keyboard focus right now?
    pub fn typing(&self) -> bool {
        self.h.ctx.text_edit_focused()
    }

    /// The text of every text field on screen.
    pub fn text_fields(&self) -> Vec<String> {
        self.h
            .query_all_by_role(egui::accesskit::Role::TextInput)
            .map(|n| n.value().unwrap_or_default())
            .collect()
    }

    /// One key, as a keyboard sends it: press (+ the typed character when
    /// no Ctrl/Alt is held) in one frame, release in the next.
    pub fn key(&mut self, key: egui::Key) {
        self.key_mods(egui::Modifiers::NONE, key);
    }

    pub fn key_mods(&mut self, mods: egui::Modifiers, key: egui::Key) {
        let input = self.h.input_mut();
        input.modifiers = mods;
        input.events.push(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: mods,
        });
        let name = key.name();
        if !mods.ctrl && !mods.alt && !mods.command && name.chars().count() == 1 {
            let text = if mods.shift {
                name.to_string()
            } else {
                name.to_lowercase()
            };
            input.events.push(egui::Event::Text(text));
        }
        self.h.step();
        let input = self.h.input_mut();
        input.events.push(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: false,
            repeat: false,
            modifiers: mods,
        });
        input.modifiers = egui::Modifiers::NONE;
        self.h.step();
        self.run();
    }

    pub fn type_text(&mut self, text: &str) {
        self.h.input_mut().events.push(egui::Event::Text(text.to_string()));
        self.h.step();
        self.run();
    }

    /// Press at `from`, move in `steps` frames, release at `to`.
    pub fn drag(&mut self, from: egui::Pos2, to: egui::Pos2, steps: usize) {
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let input = self.h.input_mut();
        input.events.push(egui::Event::PointerMoved(from));
        input.events.push(button(from, true));
        self.h.step();
        let steps = steps.max(1);
        for i in 1..=steps {
            let p = from + (to - from) * (i as f32 / steps as f32);
            self.h.input_mut().events.push(egui::Event::PointerMoved(p));
            self.h.step();
        }
        self.h.input_mut().events.push(button(to, false));
        self.h.step();
        self.run();
    }

    /// GM3: the pointer rests at `pos` (shortcuts aim at the pane under it).
    pub fn point_at(&mut self, pos: egui::Pos2) {
        self.h.input_mut().events.push(egui::Event::PointerMoved(pos));
        self.h.step();
        self.run();
    }

    /// GM3: press at `from` and move to `to` in `steps` frames with the
    /// button still down — a stroke in progress; [`release`](Self::release)
    /// lifts it.
    pub fn press_and_move(&mut self, from: egui::Pos2, to: egui::Pos2, steps: usize) {
        let input = self.h.input_mut();
        input.events.push(egui::Event::PointerMoved(from));
        input.events.push(primary(from, true));
        self.h.step();
        let steps = steps.max(1);
        for i in 1..=steps {
            let p = from + (to - from) * (i as f32 / steps as f32);
            self.h.input_mut().events.push(egui::Event::PointerMoved(p));
            self.h.step();
        }
    }

    pub fn release(&mut self, at: egui::Pos2) {
        self.h.input_mut().events.push(primary(at, false));
        self.h.step();
        self.run();
    }

    /// GM4: hold these modifiers over the frames that follow (NONE lifts).
    pub fn set_mods(&mut self, mods: egui::Modifiers) {
        self.h.input_mut().modifiers = mods;
    }

    /// GM4: press `key` and keep it down (the Space hand) — no release.
    pub fn hold_key(&mut self, key: egui::Key) {
        let input = self.h.input_mut();
        let modifiers = input.modifiers;
        input.events.push(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        });
        self.h.step();
    }

    /// GM4: lift a key [`hold_key`](Self::hold_key) pressed.
    pub fn release_key(&mut self, key: egui::Key) {
        let input = self.h.input_mut();
        let modifiers = input.modifiers;
        input.events.push(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: false,
            repeat: false,
            modifiers,
        });
        self.h.step();
        self.run();
    }

    /// GM4: the primary button down at `pts[0]`, through every point (one
    /// frame each), up at the last — under the held modifiers.
    pub fn drag_path(&mut self, pts: &[egui::Pos2]) {
        let button = |pos, pressed, modifiers| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        let input = self.h.input_mut();
        let m = input.modifiers;
        input.events.push(egui::Event::PointerMoved(pts[0]));
        input.events.push(button(pts[0], true, m));
        self.h.step();
        for p in &pts[1..] {
            self.h.input_mut().events.push(egui::Event::PointerMoved(*p));
            self.h.step();
        }
        let last = *pts.last().unwrap();
        self.h.input_mut().events.push(button(last, false, m));
        self.h.step();
        self.run();
    }

    /// GM4: a click at `pos` (press and release, no movement) under the
    /// held modifiers.
    pub fn click_pos(&mut self, pos: egui::Pos2) {
        self.drag_path(&[pos]);
    }

    /// GM14: a secondary-button click at `pos` (a context menu opens).
    pub fn right_click_pos(&mut self, pos: egui::Pos2) {
        self.button_click_pos(egui::PointerButton::Secondary, pos);
    }

    /// AU3: a middle-button click at `pos` (a dock tab closes).
    pub fn middle_click_pos(&mut self, pos: egui::Pos2) {
        self.button_click_pos(egui::PointerButton::Middle, pos);
    }

    /// One frame down and one up, so egui still calls it a click.
    fn button_click_pos(&mut self, button: egui::PointerButton, pos: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.h.input_mut().events.push(egui::Event::PointerMoved(pos));
        self.h.input_mut().events.push(button(true));
        self.h.step();
        self.h.input_mut().events.push(button(false));
        self.h.step();
        self.run();
    }

    /// GM14: one raw input event for a frame — the system clipboard's
    /// Copy / Cut / Paste, which egui-winit sends instead of Ctrl+C/X/V keys.
    pub fn event(&mut self, event: egui::Event) {
        self.h.input_mut().events.push(event);
        self.h.step();
        self.run();
    }

    /// AU4: a float open, by the call its rail key and menu row end in. No
    /// test says where a control lives, only which float holds it.
    pub fn open_float(&mut self, f: crate::floats::Float) {
        let busy = self.editor().canvas.stroke_active();
        self.editor_mut().floats.open(f, busy);
        self.run();
        self.run_frames(2);
    }

    /// AU4: a pen comes down at `pos`, in egui-winit's order: no hover before
    /// it, and a Touch ahead of the pointer it emulates. One frame only: a
    /// `run` here can spend a second of the harness clock while a scroll view
    /// keeps repainting, and a press that long is no click.
    pub fn pen_down(&mut self, pos: egui::Pos2) {
        let events = [pen(egui::TouchPhase::Start, pos), egui::Event::PointerMoved(pos), primary(pos, true)];
        self.h.input_mut().events.extend(events);
        self.h.step();
    }

    pub fn pen_move(&mut self, pos: egui::Pos2) {
        let events = [pen(egui::TouchPhase::Move, pos), egui::Event::PointerMoved(pos)];
        self.h.input_mut().events.extend(events);
        self.h.step();
    }

    /// AU6: a Move from a tablet that reports no force.
    pub fn pen_move_flat(&mut self, pos: egui::Pos2) {
        let mut touch = pen(egui::TouchPhase::Move, pos);
        if let egui::Event::Touch { force, .. } = &mut touch {
            *force = None;
        }
        self.h.input_mut().events.extend([touch, egui::Event::PointerMoved(pos)]);
        self.h.step();
    }

    /// The pen lifts, and the pointer it emulated is gone with it.
    pub fn pen_up(&mut self, pos: egui::Pos2) {
        let events = [pen(egui::TouchPhase::End, pos), primary(pos, false), egui::Event::PointerGone];
        self.h.input_mut().events.extend(events);
        self.h.step();
        self.run();
    }

    pub fn pen_tap(&mut self, pos: egui::Pos2) {
        self.pen_down(pos);
        self.pen_up(pos);
    }

    /// GM2: the window's close button — one frame with a Close event on the
    /// root viewport. Returns the viewport commands that frame sent.
    pub fn request_close(&mut self) -> Vec<egui::ViewportCommand> {
        self.h
            .input_mut()
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .events
            .push(egui::ViewportEvent::Close);
        self.h.step();
        let cmds = self.root_commands();
        self.run();
        cmds
    }

    /// GM20: the screen rects the last frame filled with `fill` (the
    /// canvas's paper sheet and veils among them), nested shapes included.
    pub fn filled_rects(&self, fill: egui::Color32) -> Vec<egui::Rect> {
        fn walk(s: &egui::Shape, fill: egui::Color32, out: &mut Vec<egui::Rect>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, fill, out)),
                egui::Shape::Rect(r) if r.fill == fill => out.push(r.rect),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for c in &self.h.output().shapes {
            walk(&c.shape, fill, &mut out);
        }
        out
    }

    /// AU6: the centres and radii of the circles the last frame filled with
    /// `fill`, nested shapes included.
    pub fn filled_circles(&self, fill: egui::Color32) -> Vec<(egui::Pos2, f32)> {
        fn walk(s: &egui::Shape, fill: egui::Color32, out: &mut Vec<(egui::Pos2, f32)>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, fill, out)),
                egui::Shape::Circle(c) if c.fill == fill => out.push((c.center, c.radius)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for c in &self.h.output().shapes {
            walk(&c.shape, fill, &mut out);
        }
        out
    }

    /// AU6: where the last frame painted `word`: the left of its first line.
    pub fn painted_at(&self, word: &str) -> egui::Pos2 {
        fn walk(s: &egui::Shape, word: &str) -> Option<egui::Pos2> {
            match s {
                egui::Shape::Vec(v) => v.iter().find_map(|s| walk(s, word)),
                egui::Shape::Text(t) if t.galley.text() == word => Some(t.pos),
                _ => None,
            }
        }
        let found = self.h.output().shapes.iter().find_map(|c| walk(&c.shape, word));
        found.unwrap_or_else(|| panic!("{word:?} was not painted"))
    }

    /// AU4: the words the last frame painted, nested shapes included: a word
    /// painted by hand speaks to no tree.
    pub fn painted_text(&self) -> Vec<String> {
        fn walk(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for c in &self.h.output().shapes {
            walk(&c.shape, &mut out);
        }
        out
    }

    /// GM20: the screen rects the last frame outlined in `color` (fill-less
    /// rects: the frame drawn on a wider paper sheet).
    pub fn outlined_rects(&self, color: egui::Color32) -> Vec<egui::Rect> {
        fn walk(s: &egui::Shape, color: egui::Color32, out: &mut Vec<egui::Rect>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, color, out)),
                egui::Shape::Rect(r) if r.stroke.color == color && r.fill.a() == 0 => out.push(r.rect),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for c in &self.h.output().shapes {
            walk(&c.shape, color, &mut out);
        }
        out
    }

    /// GM22: the meshes the last frame painted with texture `tex` (a paper
    /// image's quad, as the canvas drew it), nested shapes included.
    pub fn meshes_with(&self, tex: egui::TextureId) -> Vec<egui::Mesh> {
        fn walk(s: &egui::Shape, tex: egui::TextureId, out: &mut Vec<egui::Mesh>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, tex, out)),
                egui::Shape::Mesh(m) if m.texture_id == tex => out.push((**m).clone()),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for c in &self.h.output().shapes {
            walk(&c.shape, tex, &mut out);
        }
        out
    }

    /// The viewport commands the last frame sent to the root window.
    pub fn root_commands(&self) -> Vec<egui::ViewportCommand> {
        self.h
            .output()
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|v| v.commands.clone())
            .unwrap_or_default()
    }

    /// GM2: press-and-hold the ONE widget with this name past a DANGER's
    /// hold time, then release. Returns every root viewport command sent
    /// on the way.
    #[track_caller]
    pub fn hold(&mut self, label: &str) -> Vec<egui::ViewportCommand> {
        let c = self.rect(label).center();
        let button = |pressed| egui::Event::PointerButton {
            pos: c,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut cmds = Vec::new();
        let input = self.h.input_mut();
        input.events.push(egui::Event::PointerMoved(c));
        input.events.push(button(true));
        self.h.step();
        cmds.extend(self.root_commands());
        let now = self.h.ctx.input(|i| i.time);
        self.h.input_mut().time = Some(now + 0.5);
        self.h.step();
        cmds.extend(self.root_commands());
        self.h.input_mut().time = None;
        self.h.input_mut().events.push(button(false));
        self.h.step();
        cmds.extend(self.root_commands());
        self.run();
        cmds
    }

    pub fn has(&self, label: &str) -> bool {
        self.h.query_all_by_label(label).next().is_some()
    }

    /// GM3: is some text on screen (a tooltip, a row) containing `part`?
    pub fn has_text_containing(&self, part: &str) -> bool {
        self.h.query_all_by_label_contains(part).next().is_some()
    }

    /// AU4: how many widgets carry this name.
    pub fn query_count(&self, label: &str) -> usize {
        self.h.query_all_by_label(label).count()
    }

    /// GM3: the ONE widget with this name — enabled or greyed (None when
    /// it is not on screen).
    pub fn node_enabled(&self, label: &str) -> Option<bool> {
        let node = self.h.query_by_label(label)?;
        Some(!node.accesskit_node().is_disabled())
    }

    /// GM3: the ONE widget with this name — a toggle on or off (None when
    /// it is not on screen or not a toggle).
    pub fn node_toggled(&self, label: &str) -> Option<bool> {
        let node = self.h.query_by_label(label)?;
        let toggled = node.accesskit_node().toggled()?;
        Some(toggled == egui::accesskit::Toggled::True)
    }

    #[track_caller]
    pub fn rect(&self, label: &str) -> egui::Rect {
        self.h.get_by_label(label).rect()
    }

    /// The AccessKit tree as text: one line per named node, indented.
    pub fn describe(&self) -> String {
        describe_tree(&self.h)
    }

    /// The document state (the editor's AppState).
    pub fn state(&self) -> &AppState {
        &self.h.state().editor.state
    }

    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.h.state_mut().editor.state
    }

    /// The whole rig: the shell's config, notes and the performed log.
    pub fn rig(&self) -> &Rig {
        self.h.state()
    }

    pub fn rig_mut(&mut self) -> &mut Rig {
        self.h.state_mut()
    }

    pub fn editor(&self) -> &Editor {
        &self.h.state().editor
    }

    pub fn editor_mut(&mut self) -> &mut Editor {
        &mut self.h.state_mut().editor
    }

    /// GM4: the harness's egui context (a texture a test lifts lives here).
    pub fn ctx(&self) -> egui::Context {
        self.h.ctx.clone()
    }

    /// Every shortcut dispatched so far, with what perform returned.
    pub fn performed(&self) -> &[(Action, Performed)] {
        &self.h.state().performed
    }

    /// Perform an action the way the shell does (presets refreshed first),
    /// then let a frame run.
    pub fn perform(&mut self, a: Action) -> Performed {
        let rig = self.h.state_mut();
        rig.editor.presets.clone_from(&rig.config.presets);
        let r = rig.editor.perform(a);
        self.run_frames(1);
        r
    }

    pub fn data_dir(&self) -> &Path {
        self.dir.path()
    }

    /// AU3: the app quits and its data dir stays, for the next launch.
    pub fn quit(self) -> tempfile::TempDir {
        self.dir
    }
}

/// One panel's ui fn in a fixed rect, with the same queries.
pub struct PanelHarness<S: 'static> {
    h: Harness<'static, (S, bool)>,
}

impl<S: 'static> PanelHarness<S> {
    pub fn new(
        size: egui::Vec2,
        state: S,
        mut panel: impl FnMut(&mut egui::Ui, &mut S) + 'static,
    ) -> Self {
        let h = Harness::builder()
            .with_size(size)
            .with_max_steps(12)
            .with_wait_for_pending_images(false)
            .build_ui_state(
                move |ui, (s, dressed): &mut (S, bool)| {
                    if !*dressed {
                        crate::dress(ui.ctx());
                        *dressed = true;
                        ui.ctx().request_repaint();
                        return;
                    }
                    panel(ui, s);
                },
                (state, false),
            );
        let mut me = Self { h };
        let _ = me.h.try_run();
        me
    }

    pub fn has(&self, label: &str) -> bool {
        self.h.query_all_by_label(label).next().is_some()
    }

    pub fn has_text_containing(&self, part: &str) -> bool {
        self.h.query_all_by_label_contains(part).next().is_some()
    }

    /// AU9: where the ONE widget with this name stands.
    #[track_caller]
    pub fn rect(&self, label: &str) -> egui::Rect {
        self.h.get_by_label(label).rect()
    }

    #[track_caller]
    pub fn click(&mut self, label: &str) {
        let count = self.h.query_all_by_label(label).count();
        assert!(count == 1, "click({label:?}): {count} widget(s)\n{}", self.describe());
        self.h.get_by_label(label).click();
        let _ = self.h.try_run();
    }

    /// AU4: scroll the ONE widget with this name into view.
    #[track_caller]
    pub fn scroll_to(&mut self, label: &str) {
        self.h.get_by_label(label).scroll_to_me();
        let _ = self.h.try_run();
    }

    /// AU4: the pointer rests on the ONE widget with this name for `secs` on
    /// the egui clock.
    #[track_caller]
    pub fn rest_on(&mut self, label: &str, secs: f64) {
        let count = self.h.query_all_by_label(label).count();
        assert!(count == 1, "rest_on({label:?}): {count} widget(s)\n{}", self.describe());
        self.h.get_by_label(label).hover();
        self.h.step();
        let now = self.h.ctx.input(|i| i.time);
        self.h.input_mut().time = Some(now + secs);
        self.h.step();
        self.h.input_mut().time = None;
        let _ = self.h.try_run();
    }

    /// GM2: press-and-hold the ONE widget with this name past a DANGER's
    /// hold time, then release (EditorHarness::hold, on one panel).
    #[track_caller]
    pub fn hold(&mut self, label: &str) {
        let count = self.h.query_all_by_label(label).count();
        assert!(count == 1, "hold({label:?}): {count} widget(s)\n{}", self.describe());
        let c = self.h.get_by_label(label).rect().center();
        let button = |pressed| egui::Event::PointerButton {
            pos: c,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let input = self.h.input_mut();
        input.events.push(egui::Event::PointerMoved(c));
        input.events.push(button(true));
        self.h.step();
        let now = self.h.ctx.input(|i| i.time);
        self.h.input_mut().time = Some(now + 0.5);
        self.h.step();
        self.h.input_mut().time = None;
        self.h.input_mut().events.push(button(false));
        self.h.step();
        let _ = self.h.try_run();
    }

    /// One key with no modifiers: press in one frame, release in the next.
    pub fn key(&mut self, key: egui::Key) {
        for pressed in [true, false] {
            self.h.input_mut().events.push(egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            self.h.step();
        }
        let _ = self.h.try_run();
    }

    /// GM21: `n` more frames with no input (a cached answer must not be
    /// worked out again each frame).
    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.h.step();
        }
    }

    pub fn describe(&self) -> String {
        describe_tree(&self.h)
    }

    pub fn ctx(&self) -> egui::Context {
        self.h.ctx.clone()
    }

    pub fn state(&self) -> &S {
        &self.h.state().0
    }

    pub fn state_mut(&mut self) -> &mut S {
        &mut self.h.state_mut().0
    }
}

fn describe_tree<S>(h: &Harness<'_, S>) -> String {
    fn walk<'t>(n: egui_kittest::Node<'t>, depth: usize, out: &mut String) {
        let ak = n.accesskit_node();
        let (label, value) = (ak.label(), ak.value());
        let field = matches!(
            ak.role(),
            egui::accesskit::Role::TextInput | egui::accesskit::Role::MultilineTextInput
        );
        let named = label.is_some() || value.is_some() || field || ak.is_focused();
        if named {
            out.push_str(&"  ".repeat(depth));
            out.push_str(&format!("{:?}", ak.role()));
            if let Some(l) = label {
                out.push_str(&format!(" {l:?}"));
            }
            if let Some(v) = value {
                out.push_str(&format!(" = {v:?}"));
            }
            if ak.is_focused() {
                out.push_str(" (focused)");
            }
            out.push('\n');
        }
        for c in n.children() {
            walk(c, depth + usize::from(named), out);
        }
    }
    let mut out = String::new();
    walk(h.root(), 0, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Pane;

    // ---- the harness itself ----

    #[test]
    fn testkit_editor_runs_headless() {
        let mut h = EditorHarness::new();
        h.run_frames(10);
        let ed = h.editor();
        assert!(ed.paint.is_none() && ed.graph.is_none(), "a GPU resource was created");
        assert!(ed.audio_out.is_none() && !ed.audio_tried, "an audio device was opened");
        let tree = h.describe();
        for step in crate::steps::STEPS {
            assert!(h.has(step.label), "step switcher lacks {:?}:\n{tree}", step.label);
        }
        for tool in ["Select", "Fill"] {
            assert!(h.has(tool), "the rail lacks {tool:?}:\n{tree}");
        }
        h.click("Fill");
        h.open_float(crate::floats::Float::ToolOptions);
        assert!(h.has(crate::vocab::DRAW_TO_FILL), "Tool options lack it:\n{}", h.describe());
    }

    #[test]
    fn testkit_click_fill_tool_by_label() {
        let mut h = EditorHarness::new();
        assert!(h.editor().canvas.tool == CanvasTool::Paint);
        h.click("Fill");
        assert!(h.editor().canvas.tool == CanvasTool::Fill);
    }

    #[test]
    #[should_panic(expected = "Effects")]
    fn testkit_click_missing_label_shows_the_tree() {
        let mut h = EditorHarness::new();
        h.click("no such widget");
    }

    #[test]
    fn testkit_icon_only_controls_answer_to_names() {
        use crate::icons::Icon;
        let p = PanelHarness::new(egui::vec2(400.0, 80.0), (), |ui, _| {
            ui.horizontal(|ui| {
                crate::plate::icon_button(ui, Icon::SkipBack, "", "first frame");
                crate::plate::tool_button(ui, false, Icon::Fill, "", "");
                // Compact: no caption, the full name all the same.
                crate::plate::tool_button(ui, false, Icon::Lasso, "", "lasso fill");
                crate::plate::swatch(ui, crate::plate::PAPER, false, "skin shadow");
                crate::plate::swatch(ui, crate::plate::AKA, false, "");
            });
        });
        let tree = p.describe();
        assert!(p.has("first frame"), "{tree}");
        assert!(p.has("fill"), "{tree}");
        assert!(p.has("lasso fill") && !p.has("lasso"), "{tree}");
        assert!(p.has("colour well: skin shadow"), "{tree}");
        let hex = format!("colour well: {}", crate::plate::AKA.to_hex());
        assert!(p.has(&hex), "{tree}");
    }

    /// The pencil box, the cel queue's neighbours and the layer strip's
    /// painted controls answer to studio names, and clicking by name works.
    #[test]
    fn testkit_pencil_box_answers_to_names() {
        let mut h = EditorHarness::new();
        let digit = 1 + h
            .rig()
            .config
            .presets
            .iter()
            .position(|p| p.name == "genga")
            .expect("the pencil box has a genga pencil");
        h.open_float(crate::floats::Float::Brushes);
        h.click(&format!("genga pencil ({digit})"));
        assert_eq!(h.editor().canvas.arming_pencil(), "genga");
        assert!(!h.editor().canvas.erasing);
        h.click("Eraser");
        assert!(h.editor().canvas.erasing, "the eraser key did not arm");
        assert_eq!(h.editor().canvas.arming_pencil(), "eraser");
        let tree = h.describe();
        let layer = h.state().active_layer_name();
        assert!(h.has(&format!("lock layer {layer}")), "no lock for {layer:?}\n{tree}");
        assert!(h.has(&format!("layer colour {layer}")), "no colour dot for {layer:?}\n{tree}");
        // AU4: the deck's `layer:` chip said where a stroke lands. On a
        // shipped editor that is the Layers float's lit row.
        let mut h = EditorHarness::with_state_shipped(crate::sample::build_sample());
        let layer = h.state().active_layer_name();
        assert!(!h.has(&format!("lock layer {layer}")), "shipped Draw has no list until it is asked");
        h.open_float(crate::floats::Float::Layers);
        let tree = h.describe();
        assert!(h.has(&format!("lock layer {layer}")), "no row for {layer:?}\n{tree}");
        assert_eq!(h.node_toggled(&layer), Some(true), "{layer:?} is not the lit row\n{tree}");
    }

    // ---- shortcuts: the text-focus rule ----

    #[test]
    fn testkit_tool_chords_switch_without_focus() {
        let mut h = EditorHarness::new();
        h.key(egui::Key::G);
        assert!(h.editor().canvas.tool == CanvasTool::Fill);
        h.key(egui::Key::B);
        assert!(h.editor().canvas.tool == CanvasTool::Paint);
    }

    /// Open the room menu (GM3: was "ws") and focus its name field.
    fn focus_field(h: &mut EditorHarness) {
        h.click(crate::vocab::STEP);
        h.focus_text_field(0);
        assert!(h.typing(), "no text field took focus\n{}", h.describe());
    }

    #[test]
    fn testkit_typing_in_a_field_keeps_the_tool() {
        let mut h = EditorHarness::new();
        focus_field(&mut h);
        h.key(egui::Key::G);
        assert!(
            h.editor().canvas.tool == CanvasTool::Paint,
            "G fired over a text field: {:?}\n{}",
            h.text_fields(),
            h.describe()
        );
        let ed = h.editor_mut();
        ed.canvas.set_tool(CanvasTool::Fill, &mut ed.state);
        h.key(egui::Key::B);
        assert!(h.editor().canvas.tool == CanvasTool::Fill, "B fired over a text field");
        assert_eq!(h.editor().ws_name, "gb");
        assert!(h.performed().is_empty(), "{:?}", h.performed());
    }

    #[test]
    fn testkit_ctrl_s_in_a_field_still_saves() {
        let mut h = EditorHarness::new();
        focus_field(&mut h);
        h.key_mods(CTRL, egui::Key::S);
        assert_eq!(h.performed(), &[(Action::Save, Performed::App(AppRequest::Save))]);
        // Ctrl+Z is the field's own undo, never the document's.
        let before = h.editor().state.doc_gen;
        h.key_mods(CTRL, egui::Key::Z);
        assert_eq!(h.performed().len(), 1, "{:?}", h.performed());
        assert_eq!(h.editor().state.doc_gen, before);
    }

    // ---- Editor::perform, the one dispatch ----

    #[test]
    fn testkit_every_action_performs() {
        let mut h = EditorHarness::new();
        for &a in Action::ALL {
            let r = h.perform(a);
            let want = match a {
                Action::Save => Some(AppRequest::Save),
                Action::SaveAs => Some(AppRequest::SaveAs),
                Action::Open => Some(AppRequest::Open),
                // GM21: through the unsaved guard, like Open.
                Action::OpenSample => Some(AppRequest::OpenSample),
                Action::NewProject => Some(AppRequest::NewProject),
                // GM3: the menus' and palette's shell verbs.
                Action::Settings => Some(AppRequest::Settings),
                Action::Quit => Some(AppRequest::Quit),
                // GM12: the sound pickers are the shell's file dialogs.
                Action::ImportSound => Some(AppRequest::ImportSound),
                Action::AddSoundTrack => Some(AppRequest::AddSoundTrack),
                // GM17: the model picker is the shell's file dialog.
                Action::Import3DModel => Some(AppRequest::Import3DModel),
                // AU9: the plain exports' save dialogs.
                Action::ExportGif => Some(AppRequest::ExportTo(crate::render_queue::ExportKind::Gif)),
                Action::ExportMp4 => Some(AppRequest::ExportTo(crate::render_queue::ExportKind::Mp4)),
                Action::ExportPngSequence => Some(AppRequest::ExportTo(crate::render_queue::ExportKind::Png)),
                Action::ExportStill => Some(AppRequest::ExportTo(crate::render_queue::ExportKind::Still)),
                _ => None,
            };
            match want {
                Some(req) => assert_eq!(r, Performed::App(req), "{a:?}"),
                None => assert!(!matches!(r, Performed::App(_)), "{a:?} asked the shell: {r:?}"),
            }
        }
        h.run_frames(3);
    }

    #[test]
    fn testkit_perform_keeps_the_shell_semantics() {
        let mut h = EditorHarness::new();
        // Undo / redo walk the one history.
        let g0 = h.editor().state.doc_gen;
        assert_eq!(h.perform(Action::Undo), Performed::Done);
        assert_eq!(h.editor().state.status, "undo");
        assert_eq!(h.perform(Action::Redo), Performed::Done);
        assert_eq!(h.editor().state.status, "redo");
        assert_eq!(h.editor().state.doc_gen, g0 + 2);
        // A guest's undo travels to the host instead.
        h.editor_mut().canvas.is_guest = true;
        h.perform(Action::Undo);
        assert_eq!(h.editor().canvas.history_request, Some(false));
        assert_eq!(h.editor().state.doc_gen, g0 + 2);
        assert_eq!(h.perform(Action::Save), Performed::Ignored, "the host owns the file");
        h.editor_mut().canvas.is_guest = false;
        h.editor_mut().canvas.history_request = None;
        // Tool switches toggle exactly as before.
        let tool = |h: &EditorHarness| h.editor().canvas.tool;
        h.perform(Action::SelectTool);
        assert!(tool(&h) == CanvasTool::Select);
        h.perform(Action::SelectTool);
        assert!(tool(&h) == CanvasTool::Paint);
        h.perform(Action::FillTool);
        assert!(tool(&h) == CanvasTool::Fill);
        h.perform(Action::FillTool);
        assert!(tool(&h) == CanvasTool::Paint);
        h.perform(Action::SelectTool);
        h.perform(Action::BrushTool);
        assert!(tool(&h) == CanvasTool::Paint);
        // Onion flips the light box.
        let onion = h.editor().state.view.onion;
        h.perform(Action::ToggleOnion);
        assert_eq!(h.editor().state.view.onion, !onion);
    }

    // ---- config + workspaces: tolerant load, atomic save ----

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).expect("write into the temp dir");
    }

    #[test]
    fn testkit_config_bad_section_keeps_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.update_repo = "someone/else".into();
        cfg.perf.vsync = !cfg.perf.vsync;
        let mut v = serde_json::to_value(&cfg).unwrap();
        v["ui"] = serde_json::json!("not a section");
        for b in v["keybinds"].as_array_mut().unwrap() {
            if b["action"] == "BrushTool" {
                b["chord"]["keys"] = serde_json::json!(["N"]);
            }
        }
        v["keybinds"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({ "action": "Teleport", "chord": null }));
        let bytes = serde_json::to_vec_pretty(&v).unwrap();
        write(&config::config_path_in(dir.path()), &bytes);

        let (got, notes) = Config::load_from(dir.path());
        assert_eq!(notes.len(), 1, "{notes:?}");
        // In the artist's words, not the file's keys.
        assert!(
            notes[0].contains("interface reset to defaults")
                && notes[0].contains("1 shortcut dropped")
                && notes[0].contains("config.json.bad"),
            "{notes:?}"
        );
        assert_eq!(got.update_repo, "someone/else");
        assert_eq!(got.perf.vsync, cfg.perf.vsync);
        assert_eq!(got.keybinds.len(), Action::ALL.len());
        let brush = got.chord_for(Action::BrushTool).map(|c| c.keys.clone());
        assert_eq!(brush, Some(vec!["N".to_string()]));
        let save = got.chord_for(Action::Save).cloned();
        assert_eq!(save, Config::default().chord_for(Action::Save).cloned());
        let kept = std::fs::read(dir.path().join("config.json.bad")).unwrap();
        assert_eq!(kept, bytes, "config.json.bad holds the original bytes");
    }

    #[test]
    fn testkit_config_save_is_atomic_and_byte_stable() {
        let dir = tempfile::tempdir().unwrap();
        let path = config::config_path_in(dir.path());
        let tmp = dir.path().join("config.json.tmp");
        let (cfg, _) = Config::load_from(dir.path());
        cfg.save_to(dir.path()).unwrap();
        let first = std::fs::read(&path).unwrap();
        assert!(!tmp.exists(), "the .tmp is renamed away");
        // A save cut short leaves a .tmp: never read.
        write(&tmp, b"{ torn");
        let (back, notes) = Config::load_from(dir.path());
        assert!(notes.is_empty(), "{notes:?}");
        std::fs::remove_file(&tmp).unwrap();
        back.save_to(dir.path()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), first, "round trip is byte-stable");
        // The write goes through <file>.tmp: with that path blocked the old
        // file survives untouched.
        std::fs::create_dir(&tmp).unwrap();
        let mut changed = back.clone();
        changed.update_repo = "x/y".into();
        assert!(changed.save_to(dir.path()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), first);
    }

    #[test]
    fn testkit_workspaces_drop_only_the_bad_one() {
        let dir = tempfile::tempdir().unwrap();
        // A dock as a room saves it: laid out (a never-shown DockState
        // holds NaN rects, which JSON cannot carry).
        let dock = EditorHarness::new().editor().dock.clone();
        let ws = |name: &str| Workspace {
            name: name.into(),
            dock: dock.clone(),
            preset: None,
            view: None,
        };
        let list = Workspaces {
            list: vec![ws("a"), ws("b")],
        };
        let mut v = serde_json::to_value(&list).unwrap();
        v["list"]
            .as_array_mut()
            .unwrap()
            .insert(1, serde_json::json!({ "name": "broken", "dock": 7 }));
        let bytes = serde_json::to_vec(&v).unwrap();
        write(&Workspaces::path_in(dir.path()), &bytes);

        let (got, notes) = Workspaces::load_from(dir.path());
        let names: Vec<&str> = got.list.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("1 saved workspace could not be read"), "{notes:?}");
        assert_eq!(std::fs::read(dir.path().join("workspaces.json.bad")).unwrap(), bytes);
        got.save_to(dir.path()).unwrap();
        assert!(!dir.path().join("workspaces.json.tmp").exists());
        let (again, notes) = Workspaces::load_from(dir.path());
        assert_eq!(again.list.len(), 2);
        assert!(notes.is_empty(), "{notes:?}");
    }

    /// A bad config.json and a bad workspaces.json in the data dir: ONE
    /// notice on the editor's Aka lane names both, it waits for the artist
    /// instead of fading, and a rebuilt editor (new project, open, join)
    /// does not say it again.
    #[test]
    fn testkit_load_notes_show_once_on_the_aka_lane() {
        let dir = tempfile::tempdir().unwrap();
        write(&config::config_path_in(dir.path()), br#"{ "ui": "not a section" }"#);
        write(
            &Workspaces::path_in(dir.path()),
            br#"{ "list": [ { "name": "broken", "dock": 7 } ] }"#,
        );
        let mut state = AppState::new_project("testkit", 1920, 1080, 24, 300.0);
        state.new_drawing_at_frame();
        let mut h = EditorHarness::with_dir(state, dir);
        let said = h.state().refusal.clone();
        assert!(
            said.contains("interface reset to defaults") && said.contains("saved workspace"),
            "{said:?}"
        );
        assert!(h.has(&said), "the notice is not on screen\n{}", h.describe());
        let seq = h.state().refusal_seq;
        // A minute with no input: a plain refusal fades after 4 s.
        h.wait(60.0);
        assert!(h.has(&said), "the notice faded before the artist acted");
        assert_eq!(h.state().refusal_seq, seq, "said twice");
        // The artist acts; then it dwells and goes.
        h.key(egui::Key::Escape);
        h.wait(5.0);
        assert!(!h.has(&said), "the notice never left");
        // A rebuilt editor re-reads workspaces.json; the note is not new.
        let mut state = AppState::new_project("again", 1920, 1080, 24, 300.0);
        state.new_drawing_at_frame();
        h.rebuild_editor(state);
        h.run();
        assert!(h.state().refusal.is_empty(), "said again: {:?}", h.state().refusal);
    }

    /// The native tablet thread dying leaves the artist a reason on the Aka
    /// lane (a release build has no console).
    #[test]
    fn testkit_lost_tablet_is_said() {
        let (tx, rx) = std::sync::mpsc::channel::<crate::PenSample>();
        drop(tx);
        let mut rx = Some(rx);
        let mut h = EditorHarness::new();
        let got = crate::pump_tablet(&mut rx, &mut h.rig_mut().notes);
        assert!(got.is_empty() && rx.is_none(), "the dead backend is dropped");
        h.run();
        let said = h.state().refusal.clone();
        assert!(said.contains("tablet") && said.contains("standard pen input"), "{said:?}");
        assert!(h.has(&said), "{}", h.describe());
    }

    #[test]
    fn testkit_paths_stay_in_the_temp_dir() {
        let h = EditorHarness::new();
        let dir = h.data_dir().to_path_buf();
        assert!(dir.starts_with(std::env::temp_dir()), "{}", dir.display());
        assert!(config::config_path_in(&dir).starts_with(&dir));
        assert!(Workspaces::path_in(&dir).starts_with(&dir));
        assert!(config::autosave_dir_in(&dir).starts_with(&dir));
        assert_eq!(h.editor().data_dir.as_deref(), Some(dir.as_path()));
        h.editor().save_workspaces();
        assert!(Workspaces::path_in(&dir).exists(), "workspaces persist into the harness dir");
        // The real data dir refuses outright under test.
        assert!(std::panic::catch_unwind(config::data_dir).is_err());
    }

    // ---- the layout probe, every room at every scale ----

    fn probe_room(stage: Stage) {
        for ppp in crate::PROBE_SCALES {
            let run = crate::probe_run(stage, ppp);
            let lines = run.lines.join("\n");
            println!("---- {} · pixels_per_point {ppp} ----\n{lines}", stage.name());
            // A probe that measured nothing must not pass.
            assert_eq!(
                run.unmeasured,
                0,
                "{} at {ppp}: no canvas rect\n{lines}",
                stage.name()
            );
            assert_eq!(run.moved, 0, "{} at {ppp}: the canvas moved\n{lines}", stage.name());
        }
    }

    #[test]
    fn testkit_probe_layout_room() {
        probe_room(Stage::Layout);
    }

    #[test]
    fn testkit_probe_drawing_room() {
        probe_room(Stage::Drawing);
    }

    #[test]
    fn testkit_probe_finishing_room() {
        // GM8 (D12): the room is ink & paint now; its key stays "finishing".
        probe_room(Stage::InkPaint);
    }

    #[test]
    fn testkit_probe_composite_room() {
        probe_room(Stage::Composite);
    }

    #[test]
    fn testkit_probe_edit_room() {
        probe_room(Stage::Edit);
    }

    // ---- source ratchets ----

    fn sources() -> Vec<(String, String)> {
        sources_under(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
    }

    /// Every `.rs` under `root`, named from `root` with forward slashes. AU1:
    /// the folder is flat today; a later subfolder stays under the ratchets.
    fn sources_under(root: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut dirs = vec![root.to_path_buf()];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                    out.push((name, std::fs::read_to_string(&path).unwrap()));
                }
            }
        }
        out.sort();
        out
    }

    fn source(name: &str) -> String {
        sources().into_iter().find(|(n, _)| n == name).unwrap().1
    }

    #[test]
    fn testkit_spawns_go_through_proc_and_release_is_gui() {
        let needle = concat!("Command", "::new(");
        for (name, text) in sources() {
            if name == "proc.rs" || name == "devloop.rs" {
                continue;
            }
            assert!(!text.contains(needle), "{name} spawns outside proc::command");
        }
        let main = sources().into_iter().find(|(n, _)| n == "main.rs").unwrap().1;
        assert!(main.contains(concat!("windows_subsystem", " = \"windows\"")));
    }

    /// Colour literals outside plate.rs: 86 at 56679ad, driven to 0 by the
    /// ship part (GM19) — every colour outside the plate is a token or a
    /// derived plate helper. The count may only go down (D7), so it stays 0.
    const COLOUR_LITERALS_PINNED: usize = 0;

    fn colour_literals(text: &str) -> usize {
        let prefix = concat!("Color32", "::");
        let calls = [
            "from_rgb(",
            "from_rgba_unmultiplied(",
            "from_rgba_premultiplied(",
            "from_gray(",
            "from_black_alpha(",
            "from_white_alpha(",
        ];
        let mut n = 0;
        for (at, _) in text.match_indices(prefix) {
            let rest = &text[at + prefix.len()..];
            if calls.iter().any(|c| rest.starts_with(c)) {
                n += 1;
            } else if rest.starts_with(|c: char| c.is_ascii_uppercase()) {
                let ident: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                    .collect();
                if ident != "TRANSPARENT" {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn testkit_colour_literal_ratchet() {
        let mut total = 0;
        for (name, text) in sources() {
            if name == "plate.rs" {
                continue;
            }
            let n = colour_literals(&text);
            if n > 0 {
                println!("{name:22} {n}");
            }
            total += n;
        }
        println!("total {total} (pinned {COLOUR_LITERALS_PINNED})");
        assert!(
            total <= COLOUR_LITERALS_PINNED,
            "colour literals rose to {total} (pinned {COLOUR_LITERALS_PINNED}) — use plate tokens"
        );
    }

    // ---- GM19: the ship gates ----

    /// Not one colour literal outside plate.rs, file by file — and the
    /// counter still counts, so the zero is not a blind scanner's.
    #[test]
    fn ship_no_colour_literal_outside_the_plate() {
        let planted = concat!(
            "Color32", "::from_rgb(1, 2, 3); Color32", "::WHITE; Color32",
            "::TRANSPARENT; egui::Color32", "::from_black_alpha(9)"
        );
        assert_eq!(colour_literals(planted), 3, "the counter skips only TRANSPARENT");
        let left: Vec<(String, usize)> = sources()
            .into_iter()
            .filter(|(name, _)| name != "plate.rs")
            .map(|(name, text)| (name, colour_literals(&text)))
            .filter(|&(_, n)| n > 0)
            .collect();
        assert_eq!(left, vec![], "use plate tokens or plate helpers");
        assert_eq!(COLOUR_LITERALS_PINNED, 0);
    }

    /// A file's shipped part: everything before its first test module.
    fn shipped(text: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        let end = lines
            .windows(2)
            .position(|w| w[0].trim() == "#[cfg(test)]" && w[1].trim_start().starts_with("mod "))
            .unwrap_or(lines.len());
        lines[..end].join("\n")
    }

    /// Chips made of numbers in a file's shipped part: a UI colour slipping
    /// past the count above as if the animator had picked it.
    fn literal_chips(text: &str) -> usize {
        let text = shipped(text);
        let call = concat!("chip", "([");
        text.match_indices(call)
            .filter(|(at, _)| text[at + call.len()..].starts_with(|c: char| c.is_ascii_digit()))
            .count()
    }

    /// `plate::chip` shows a colour the animator made, exactly as stored;
    /// outside plate.rs and the tests none is made of numbers (the picker's
    /// ends are plate::PICK_WHITE and PICK_BLACK) — and the counter counts.
    #[test]
    fn ship_no_numeric_chip_outside_the_plate() {
        let planted = concat!("plate::chip", "([1, 2, 3, 4]); plate::chip", "([c[0], c[1], c[2], 255])");
        assert_eq!(literal_chips(planted), 1, "only the chip of numbers counts");
        let in_tests = concat!("fn a() {}\n#[cfg(test)]\nmod tests {\n", "chip", "([9, 9, 9, 9])\n}");
        assert_eq!(literal_chips(in_tests), 0, "a test may build one");
        let left: Vec<(String, usize)> = sources()
            .into_iter()
            .filter(|(name, _)| name != "plate.rs")
            .map(|(name, text)| (name, literal_chips(&text)))
            .filter(|&(_, n)| n > 0)
            .collect();
        assert_eq!(left, vec![], "a UI colour is a plate token");
    }

    /// Every Action is described in one plain sentence, has a group and a
    /// place (a menu path, or the palette for a deck tool), and neither
    /// keymap binds one chord twice in one context.
    #[test]
    fn ship_every_action_is_described_placed_and_clash_free() {
        use crate::config::{Group, Keymap};
        let mut seen = std::collections::HashSet::new();
        for &a in Action::ALL {
            assert!(seen.insert(a), "{a:?} is listed twice in Action::ALL");
            assert!(!a.label().trim().is_empty(), "{a:?} has no name");
            let help = a.help();
            assert!(
                (1..=120).contains(&help.chars().count()) && help.ends_with('.'),
                "{a:?} help is not one plain sentence: {help:?}"
            );
            assert!(Group::ALL.contains(&a.group()), "{a:?}");
            match a.menu() {
                Some(path) => assert!(path.split_once('/').is_some_and(|(m, i)| !m.is_empty() && !i.is_empty()), "{a:?}: {path}"),
                None => assert_eq!(a.group(), Group::Tool, "{a:?} is in no menu and not a deck tool"),
            }
        }
        for &km in Keymap::ALL {
            let binds = config::default_binds(km);
            assert_eq!(binds.len(), Action::ALL.len(), "{km:?} binds every Action");
            assert_eq!(config::clashes(&binds), vec![], "{km:?}");
        }
    }

    /// 0.5.0 in the app, the installer and the wire floor, and Help ▸ About
    /// says it.
    #[test]
    fn ship_version_is_0_5_0_everywhere() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.5.0");
        assert_eq!(crate::update::CURRENT_VERSION, "0.5.0");
        assert_eq!(crate::net::MIN_PEER_VERSION, "0.5.0");
        assert_eq!(crate::welcome::VERSION, "0.5.0");
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/installer/Cargo.toml");
        let text = std::fs::read_to_string(&manifest).unwrap();
        let version = text
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("version"))
            .expect("the installer's [package] version");
        assert_eq!(version, "version = \"0.5.0\"", "{}", manifest.display());
        let mut h = EditorHarness::new();
        h.perform(Action::About);
        h.run();
        assert!(h.has("version 0.5.0"), "{}", h.describe());
    }

    /// The refreshed sample shows off the release — a camera pan over a BG
    /// wider than the frame, glow and diffusion on the ball, particles over
    /// the comp, keys and inbetweens — renders every frame through the CPU
    /// golden renderer, and carries its generated serifu line: a Dialogue
    /// track exactly as long as the cut, sounding in the mixdown a delivery
    /// muxes.
    #[test]
    fn ship_sample_renders_every_frame_and_speaks() {
        use anim_core::fx::FxKind;
        use anim_core::graph::NodeKind;
        let (s, parts) = crate::sample::build_showcase();
        let at = s.at();
        let cut = s.engine.project.cut(at.scene, at.cut).unwrap();
        let law = s.engine.project.color_law;
        let (w, h) = crate::sample::SIZE;
        let last = crate::sample::FRAMES - 1;
        assert_eq!(cut.frame_count, crate::sample::FRAMES);
        // Each showcase node is in the graph and feeds the output.
        let node = |id| cut.graph.node(id).unwrap();
        let out = cut.graph.output.expect("the output is wired");
        for (part, id) in [
            ("camera", parts.camera),
            ("glow", parts.glow),
            ("diffusion", parts.diffusion),
            ("particles", parts.particles),
        ] {
            assert!(cut.graph.downstream_closure(&[id]).contains(&out), "the {part} feeds nothing");
        }
        assert!(matches!(node(parts.camera).kind, NodeKind::Camera { .. }));
        assert!(matches!(
            node(parts.particles).kind,
            NodeKind::Generator { generator: anim_core::generator::GenKind::Particles, .. }
        ));
        // Glow, then diffusion, on the ball's column A.
        let ball = cut.xsheet.columns.iter().find(|c| c.name == "A").unwrap().id;
        assert!(matches!(node(parts.glow).kind, NodeKind::Effect { fx: FxKind::Glow, .. }));
        let glow_in = node(parts.glow).inputs[0].expect("the glow is fed").0;
        assert!(matches!(node(glow_in).kind, NodeKind::DrawingSource { column, .. } if column == ball));
        assert!(matches!(node(parts.diffusion).kind, NodeKind::Effect { fx: FxKind::Diffusion, .. }));
        assert_eq!(node(parts.diffusion).inputs[0].map(|(src, _)| src), Some(parts.glow));
        // Keys and inbetweens on the ball.
        let kinds: Vec<_> = cut.drawings.iter().map(|d| cut.drawing_def(d.id).kind).collect();
        let count = |k| kinds.iter().filter(|&&x| x == k).count();
        assert_eq!(count(anim_core::model::DrawingKind::Key), crate::sample::KEYS.len());
        assert_eq!(count(anim_core::model::DrawingKind::Inbetween), 12 - crate::sample::KEYS.len());
        // The BG is wider than the frame, and the camera pans across it:
        // PAN px from the paper's left end on frame 0 onto the frame on the
        // last, so its picture slides by exactly that.
        let paper = anim_core::camera::paper(cut, w, h);
        assert_eq!((paper.x, paper.w), (-(crate::sample::PAN as i32), w + crate::sample::PAN));
        let NodeKind::Camera { params, binds, .. } = &node(parts.camera).kind else {
            unreachable!()
        };
        let pan = |f| anim_core::camera::CameraState::resolve(cut, params, binds, f).pan_x;
        assert_eq!((pan(0), pan(last)), (-(crate::sample::PAN as f32), 0.0));
        let first = anim_core::export::render_node_px(cut, law, parts.camera, 0, w, h);
        let end = anim_core::export::render_node_px(cut, law, parts.camera, last, w, h);
        assert!(first != end, "the camera did not move");
        let at_px = |px: &[f32], x: u32, y: u32| px[((y * w + x) * 4) as usize..][..4].to_vec();
        for y in (0..h).step_by(7) {
            for x in (0..w - crate::sample::PAN).step_by(5) {
                let (a, b) = (at_px(&end, x, y), at_px(&first, x + crate::sample::PAN, y));
                assert!(a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-4), "({x}, {y}): {a:?} vs {b:?}");
            }
        }
        // The petals draw.
        let petals = anim_core::export::render_node_px(cut, law, parts.particles, 0, w, h);
        assert!(petals.chunks_exact(4).any(|p| p[3] > 0.0), "no particle on frame 0");
        for f in 0..cut.frame_count {
            let px = anim_core::export::render_graph_frame_law(cut, law, f, w, h)
                .unwrap_or_else(|| panic!("frame {f}: the sample's output is not wired"));
            assert_eq!(px.len(), (w * h * 4) as usize, "frame {f}");
            assert!(px.chunks_exact(4).all(|p| p[3] == 255), "frame {f}: the plate leaves a hole");
        }
        assert!(anim_core::audio::has_sound(cut), "the sample is silent");
        assert_eq!(cut.audio_props.kind, anim_core::audio::TrackKind::Dialogue);
        let clip = cut.audio.as_ref().unwrap();
        let secs = clip.samples.len() as f64 / (clip.sample_rate as f64 * clip.channels as f64);
        assert!((secs - cut.frame_count as f64 / 24.0).abs() < 1e-9, "{secs} s");
        let mix = anim_core::deliver::mixdown(cut, 24, 0, cut.frame_count - 1).expect("a mixdown");
        assert!(mix.iter().any(|v| v.abs() > 0.05), "the line is inaudible");
        // Silence between the syllables, a voice inside them.
        let rate = crate::sample::VOICE_RATE as usize;
        let at_frame = |f: f32| clip.samples[(f / 24.0 * rate as f32) as usize].abs();
        assert_eq!(at_frame(1.0), 0.0, "frame 1 is before the first syllable");
        let peak = (rate * 5 / 24..rate * 6 / 24).map(|i| clip.samples[i].abs()).fold(0.0, f32::max);
        assert!(peak > 0.1, "the first syllable peaks at {peak}");
        assert_eq!(crate::sample::dialogue_wav(), crate::sample::dialogue_wav(), "made the same every time");
    }

    /// The first launch of a new version opens the Welcome screen on What's
    /// New, grouped by persona; Start brings the cards back.
    #[test]
    fn ship_whats_new_is_grouped_by_persona_and_opens_first() {
        let personas: Vec<&str> = crate::welcome::WHATS_NEW.iter().map(|(p, _)| *p).collect();
        assert_eq!(personas, ["Animator", "Painter", "Compositor", "3D", "Manga", "Editor"]);
        assert!(crate::welcome::WHATS_NEW.iter().all(|(_, items)| !items.is_empty()));
        assert!(crate::welcome::whats_new_due(""), "a first launch is due");
        assert!(!crate::welcome::whats_new_due(crate::welcome::VERSION), "seen once, not again");
        assert!(Config::default().ui.whats_new_seen.is_empty());
        let mut w = crate::welcome::Welcome::new(&[]);
        w.tab = crate::welcome::WelcomeTab::WhatsNew;
        let binds = Config::default().keybinds;
        let mut h = PanelHarness::new(egui::vec2(1280.0, 800.0), (w, true), move |ui, (w, show)| {
            let _ = crate::welcome::ui(ui, w, show, &[false; 5], &binds);
        });
        assert!(h.has("What's New in 0.5.0"), "{}", h.describe());
        for p in personas {
            assert!(h.has(p), "{p}\n{}", h.describe());
        }
        assert!(!h.has("New from template"), "the Start cards hide behind the tab");
        h.click("Start");
        assert!(h.has("New from template"), "{}", h.describe());
        assert!(!h.has("What's New in 0.5.0"));
    }

    /// The start-up route (Startup::Welcome) through
    /// `welcome::open_at_startup`: a config that never saw this version opens
    /// What's New and records it (the caller saves); the next launch opens on
    /// Start and leaves the config alone.
    #[test]
    fn ship_startup_opens_whats_new_once() {
        use crate::welcome::{VERSION, WelcomeTab, open_at_startup};
        for seen in ["", "0.3.0"] {
            let mut ui = Config::default().ui;
            ui.whats_new_seen = seen.into();
            let (w, changed) = open_at_startup(&mut ui, &[]);
            assert_eq!((w.tab, changed), (WelcomeTab::WhatsNew, true), "seen {seen:?}");
            assert_eq!(ui.whats_new_seen, VERSION, "the version is recorded");
            let (w, changed) = open_at_startup(&mut ui, &[]);
            assert_eq!((w.tab, changed), (WelcomeTab::Start, false), "the next launch");
            assert_eq!(ui.whats_new_seen, VERSION);
        }
    }

    /// A config 0.3.0 wrote (no keymap, no UI scale, no `whats_new_seen`)
    /// loads whole: nothing reset, the artist's settings kept, the newer ones
    /// at their defaults, and What's New due on the first 0.5 launch. No
    /// config at all (a fresh install) opens on Start, and a config this
    /// build saved reads its version back.
    #[test]
    fn ship_older_configs_still_load() {
        use crate::welcome::{VERSION, WelcomeTab, open_at_startup};
        let dir = tempfile::tempdir().unwrap();
        // The sections and fields 0.3.0's Config wrote (v0.3.0 config.rs).
        let chord = |key: &str, ctrl: bool| {
            serde_json::json!({ "keys": [key], "ctrl": ctrl, "shift": false, "alt": false })
        };
        let old = serde_json::json!({
            "keybinds": [
                { "action": "BrushTool", "chord": chord("N", false) },
                { "action": "Save", "chord": chord("S", true) }
            ],
            "perf": {
                "vsync": true, "frame_latency": "Throughput", "canvas_filter": "Nearest",
                "fps_cap": 60, "show_fps": true, "undo_limit": 200
            },
            "ui": { "lock_positions": true, "exact_descriptions": false },
            "session": {
                "username": "lancer", "api_key": "", "totp_secret": "", "host_port": 7878,
                "last_addr": "", "require_key": false
            },
            "update_repo": "someone/else"
        });
        write(&config::config_path_in(dir.path()), &serde_json::to_vec_pretty(&old).unwrap());
        let (got, notes) = Config::load_from(dir.path());
        assert!(notes.is_empty(), "nothing reset: {notes:?}");
        assert!(!dir.path().join("config.json.bad").exists());
        assert!(got.ui.lock_positions && got.perf.vsync && got.perf.show_fps);
        let perf = (got.perf.frame_latency, got.perf.fps_cap, got.perf.undo_limit);
        assert_eq!(perf, (config::FrameLatency::Throughput, 60, 200));
        assert_eq!((got.session.username.as_str(), got.session.host_port), ("lancer", 7878));
        assert_eq!(got.update_repo, "someone/else");
        let brush = got.chord_for(Action::BrushTool).map(|c| c.keys.clone());
        assert_eq!(brush, Some(vec!["N".to_string()]), "the artist's chord stays");
        assert_eq!(got.keybinds.len(), Action::ALL.len(), "the newer verbs are filled in");
        let fresh = Config::default();
        assert_eq!((got.ui.scale, got.ui.show_welcome), (fresh.ui.scale, fresh.ui.show_welcome));
        assert_eq!(got.ui.whats_new_seen, "", "0.3 never showed a What's New");
        let mut ui = got.ui.clone();
        assert_eq!(open_at_startup(&mut ui, &[]).0.tab, WelcomeTab::WhatsNew);
        // Saved after that launch: not due again.
        let mut seen = got;
        seen.ui = ui;
        seen.save_to(dir.path()).unwrap();
        let (back, _) = Config::load_from(dir.path());
        assert_eq!(back.ui.whats_new_seen, VERSION);
        // A fresh install: no file, nothing older to compare with.
        let empty = tempfile::tempdir().unwrap();
        let (cfg, notes) = Config::load_from(empty.path());
        assert!(notes.is_empty(), "{notes:?}");
        let mut ui = cfg.ui.clone();
        assert_eq!(open_at_startup(&mut ui, &[]).0.tab, WelcomeTab::Start);
    }

    /// The README a newcomer reads first covers the release: what it does by
    /// who reaches for it, a quick start, keyboard basics that match the
    /// Gaban keymap, the building steps the release gate runs, and where the
    /// research lives (every path it names is there), at this version.
    #[test]
    fn ship_readme_covers_the_release() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let readme = std::fs::read_to_string(root.join("README.md")).expect("the README");
        let section = |heading: &str| -> String {
            let (_, body) = readme
                .split_once(&format!("\n{heading}"))
                .unwrap_or_else(|| panic!("the README has no {heading:?}"));
            body.split("\n## ").next().unwrap_or_default().to_string()
        };
        assert!(readme.contains(&format!("Version {}.", crate::welcome::VERSION)));
        let features = section("## What it does, by who reaches for it");
        for persona in ["Animator", "Painter", "Compositor", "3D artist", "Manga artist", "Editor"] {
            assert!(features.contains(&format!("**{persona}")), "no {persona} in the features");
        }
        let quick = section("## Quick start");
        for step in ["Welcome screen", "sample", "What's New", "Quick start checklist"] {
            assert!(quick.contains(step), "no {step:?} in the quick start");
        }
        let keys = section("## Keyboard basics");
        let cfg = Config::default();
        let rows: [(&str, &[Action]); 8] = [
            ("the command palette", &[Action::CommandPalette]),
            ("the shortcut sheet", &[Action::ShortcutSheet]),
            ("play / stop", &[Action::PlayPause]),
            ("next / previous frame", &[Action::NextFrame, Action::PrevFrame]),
            ("next / previous key", &[Action::NextKey, Action::PrevKey]),
            ("new drawing", &[Action::NewDrawing]),
            ("eraser", &[Action::ToggleEraser]),
            ("undo, redo", &[Action::Undo, Action::Redo]),
        ];
        for (does, actions) in rows {
            let (cell, _) = keys
                .lines()
                .filter_map(|l| l.strip_prefix("| ")?.split_once(" | "))
                .find(|(_, d)| d.starts_with(does))
                .unwrap_or_else(|| panic!("no keyboard row for {does:?}"));
            let named: Vec<&str> = cell.split([' ', ',', '(', ')', '/']).filter(|t| !t.is_empty()).collect();
            for a in actions {
                for c in cfg.binding(*a).expect("bound").chords() {
                    assert!(named.contains(&c.label().as_str()), "{does}: {} not in {cell:?}", c.label());
                }
            }
        }
        let building = section("## Building");
        for step in [
            "cargo test -p animstudio -p anim-core",
            "cargo build --release -p animstudio ",
            "bash tools/smoke_render.sh",
            "cargo build --release -p animstudio-installer",
        ] {
            assert!(building.contains(step), "no {step:?} under Building");
        }
        let places = section("## Where things live");
        assert!(places.contains("`research/`") && places.contains("`PSD-modernize.md`"));
        // The research files are named bare, under research/.
        for name in places.split('`').skip(1).step_by(2).filter(|n| n.contains(['/', '.'])) {
            let here = root.join(name).exists() || root.join("research").join(name).exists();
            assert!(here, "the README names {name}, which is not there");
        }
    }

    // Every pane alone in the dock, 30 headless frames at each probe scale,
    // over the sample (so the panes have a cut to show): no panic, and the
    // pane is still there — except the retired Brush pane (owner
    // 2026-08-17), which the editor removes on sight. One test per pane
    // keeps each under D0's 2 s.
    fn smoke_pane(pane: crate::workspace::Pane) {
        for ppp in crate::PROBE_SCALES {
            let mut h = EditorHarness::with_state(crate::sample::build_sample()).with_ppp(ppp);
            h.editor_mut().dock = egui_dock::DockState::new(vec![pane]);
            h.run_frames(30);
            let kept = h.editor().dock.find_tab(&pane).is_some();
            let retired = pane == crate::workspace::Pane::Brush;
            assert_eq!(kept, !retired, "{pane:?} at {ppp}: kept {kept}, retired {retired}");
        }
    }

    macro_rules! ship_pane_smoke {
        ($($name:ident: $pane:expr,)*) => {
            /// The panes the smoke tests below drive.
            const SMOKED_PANES: &[crate::workspace::Pane] = &[$($pane),*];
            $(
                #[test]
                fn $name() {
                    smoke_pane($pane);
                }
            )*
        };
    }

    ship_pane_smoke! {
        ship_pane_canvas: crate::workspace::Pane::Canvas,
        ship_pane_xsheet: crate::workspace::Pane::XSheet,
        ship_pane_layers: crate::workspace::Pane::Layers,
        ship_pane_brush: crate::workspace::Pane::Brush,
        ship_pane_presets: crate::workspace::Pane::Presets,
        ship_pane_forge: crate::workspace::Pane::Forge,
        ship_pane_palette: crate::workspace::Pane::Palette,
        ship_pane_node_graph: crate::workspace::Pane::NodeGraph,
        ship_pane_viewer: crate::workspace::Pane::Viewer(0),
        ship_pane_history: crate::workspace::Pane::History,
        ship_pane_curves: crate::workspace::Pane::Curves,
        ship_pane_paint_check: crate::workspace::Pane::PaintCheck,
        ship_pane_render_queue: crate::workspace::Pane::RenderQueue,
        ship_pane_board: crate::workspace::Pane::Board,
        ship_pane_episode: crate::workspace::Pane::Episode,
        ship_pane_view3d: crate::workspace::Pane::View3D,
        ship_pane_manga: crate::workspace::Pane::Manga,
    }

    /// The smoke list names every Pane variant: the add-menu's singletons
    /// and a viewer (the exhaustive match fails to compile on a new one).
    #[test]
    fn ship_pane_smoke_covers_every_pane() {
        use crate::workspace::Pane;
        let variant = |p: &Pane| match p {
            Pane::Canvas
            | Pane::XSheet
            | Pane::Layers
            | Pane::Brush
            | Pane::Presets
            | Pane::Forge
            | Pane::Palette
            | Pane::NodeGraph
            | Pane::History
            | Pane::Curves
            | Pane::PaintCheck
            | Pane::RenderQueue
            | Pane::Board
            | Pane::Episode
            | Pane::View3D
            | Pane::Manga => p.title(),
            Pane::Viewer(_) => "a viewer".into(),
        };
        for p in Pane::ALL {
            assert!(SMOKED_PANES.contains(p), "{} is not smoke-tested", variant(p));
        }
        assert!(SMOKED_PANES.iter().any(|p| matches!(p, Pane::Viewer(_))));
        assert_eq!(SMOKED_PANES.len(), Pane::ALL.len() + 1);
    }

    /// The probe walks every room — the spine, the manga room, the forge
    /// and the 3D layout room — and no room is left out of it.
    #[test]
    fn ship_probe_walks_every_room() {
        let stages: Vec<Stage> = crate::PROBE_ROOMS
            .iter()
            .filter_map(|r| match r {
                crate::ProbeRoom::Stage(st) => Some(*st),
                crate::ProbeRoom::Forge | crate::ProbeRoom::Layout3d => None,
            })
            .collect();
        for st in [
            Stage::Layout,
            Stage::Drawing,
            Stage::InkPaint,
            Stage::Composite,
            Stage::Edit,
            Stage::Manga,
            Stage::Export,
        ] {
            // Exhaustive: a new Stage fails to compile until it is listed.
            match st {
                Stage::Layout
                | Stage::Drawing
                | Stage::InkPaint
                | Stage::Composite
                | Stage::Edit
                | Stage::Manga
                | Stage::Export => assert!(stages.contains(&st), "{} is not probed", st.name()),
            }
        }
        assert!(crate::PROBE_ROOMS.iter().any(|r| matches!(r, crate::ProbeRoom::Forge)));
        assert!(crate::PROBE_ROOMS.iter().any(|r| matches!(r, crate::ProbeRoom::Layout3d)));
    }

    fn probe_holds_still(room: crate::ProbeRoom) {
        for ppp in crate::PROBE_SCALES {
            let run = room.probe(ppp);
            let lines = run.lines.join("\n");
            assert_eq!(run.unmeasured, 0, "{} at {ppp}: no pane rect\n{lines}", room.name());
            assert_eq!(run.moved, 0, "{} at {ppp}: the held pane moved\n{lines}", room.name());
        }
    }

    #[test]
    fn ship_probe_manga_room() {
        probe_holds_still(crate::ProbeRoom::Stage(Stage::Manga));
    }

    /// The 3D layout room, entered by its verb: its View3D (the surface
    /// under the pointer there) holds still through the probe's scenarios,
    /// and it is the room the verb builds.
    #[test]
    fn ship_probe_layout3d_room() {
        probe_holds_still(crate::ProbeRoom::Layout3d);
        let mut h = EditorHarness::new();
        h.perform(Action::Layout3dRoom);
        h.run();
        use crate::workspace::Pane;
        for pane in [Pane::View3D, Pane::NodeGraph, Pane::Viewer(0)] {
            assert!(h.editor().dock.find_tab(&pane).is_some(), "{pane:?}");
        }
    }

    #[test]
    fn ship_probe_forge_room() {
        probe_holds_still(crate::ProbeRoom::Forge);
    }

    // ---- AU1: the test seams ----

    /// The `impl EditorHarness` block.
    fn harness_impl() -> String {
        let kit = source("testkit.rs").replace("\r\n", "\n");
        let from = kit.find(concat!("impl Editor", "Harness {")).unwrap();
        let len = kit[from..].find("\n}\n").unwrap();
        kit[from..from + len].to_string()
    }

    #[test]
    fn auseam_every_constructor_goes_through_the_funnel() {
        let body = harness_impl();
        assert_eq!(body.matches(concat!("Harness", "::builder()")).count(), 1, "one builder");
        let builds = concat!("Editor", "::boot(");
        assert_eq!(body.matches(builds).count(), 2, "`build` and `rebuild_editor` alone");
        let mut constructors = 0;
        for piece in body.split("\n    pub fn ").skip(1) {
            let sig = piece.lines().next().unwrap();
            if sig.ends_with("-> Self {") && !sig.contains("self") {
                constructors += 1;
                let fn_body = piece.split("\n    }\n").next().unwrap();
                let funnel = [concat!("Self", "::build("), concat!("Self", "::with_")];
                assert!(funnel.iter().any(|f| fn_body.contains(f)), "{sig} skips the funnel");
            }
        }
        assert!(constructors >= 9, "found only {constructors}");
        // And what the funnel does with each, live. The two GPU constructors
        // read the same way in the text above; the suite opens no device.
        let blank = || NewProjectForm::default().new_state();
        let dir = || tempfile::tempdir().unwrap();
        for (h, boot) in [
            (EditorHarness::new(), Boot::Frozen),
            (EditorHarness::with_state(blank()), Boot::Frozen),
            (EditorHarness::with_dir(blank(), dir()), Boot::Frozen),
            (EditorHarness::new().with_ppp(1.5), Boot::Frozen),
            (EditorHarness::as_shipped(), Boot::Shipped),
            (EditorHarness::with_state_shipped(blank()), Boot::Shipped),
            (EditorHarness::with_dir_shipped(dir()), Boot::Shipped),
            (EditorHarness::as_first_run(), Boot::FirstRun),
        ] {
            assert_eq!(h.boot(), boot);
            assert_eq!(h.rig().arms, boot != Boot::Frozen);
        }
    }

    #[test]
    fn auseam_shipped_boot_is_flush_to_the_window() {
        use egui::containers::panel::PanelState;
        for mut h in [EditorHarness::as_shipped(), EditorHarness::as_first_run()] {
            h.kit_mut().set_size(egui::vec2(1600.0, 900.0));
            h.run();
            let window = h.ctx().content_rect();
            assert_eq!(h.rig().root, window, "{:?}", h.boot());
            let panel = |id: &str| PanelState::load(&h.ctx(), egui::Id::new(id)).expect(id).outer_rect;
            let (top, band) = (panel("top_bar"), panel("status_bar"));
            assert_eq!((top.min, top.right()), (window.min, window.right()), "the top row");
            assert_eq!((band.left(), band.max), (window.left(), window.max), "the band");
        }
        // The frozen boot stays in the harness's frame: BASELINE stands on it.
        let frozen = EditorHarness::new();
        assert_eq!(frozen.rig().root, frozen.ctx().content_rect().shrink(8.0));
    }

    /// A harness on a real device keeps the process's one turn while it lives.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn auseam_gpu_harness_holds_the_turn() {
        let h = EditorHarness::with_gpu(AppState::new_project("turn", 64, 64, 24, 1.0));
        assert!(GPU_TURN.try_lock().is_err(), "a second harness would paint beside it");
        drop(h);
    }

    /// The Lead's rule: no test or tool opens a window. The device the ignored
    /// tests use has no display to make a surface on, the test kit never
    /// ships, and only the App's own two files hold a door to the desktop.
    #[test]
    fn auseam_nothing_under_test_opens_a_window() {
        let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = default_wgpu_setup() else {
            panic!("a new headless instance, never a device from elsewhere");
        };
        assert!(setup.display_handle.is_none(), "nothing to make a surface on");
        assert!(setup.instance_descriptor.display.is_none(), "nor an instance that could");
        assert!(source("testkit.rs").contains(concat!("create_render_state(default_wgpu", "_setup(),")));
        let manifest = include_str!("../Cargo.toml");
        let (mut section, mut kit) = ("", Vec::new());
        for line in manifest.lines() {
            if line.starts_with('[') {
                section = line;
            }
            if line.starts_with("egui_kittest") {
                kit.push(section);
            }
        }
        assert_eq!(kit, ["[dev-dependencies]"]);
        let doors = [
            concat!("run_", "native"),
            concat!("create_", "surface"),
            concat!("compatible_", "surface"),
            concat!("Viewport", "Builder"),
            concat!("show_", "viewport"),
        ];
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = sources();
        files.extend(sources_under(&workspace.join("tools")));
        files.extend(sources_under(&workspace.join("crates")));
        let opened: Vec<&str> = files
            .iter()
            .filter(|(_, text)| doors.iter().any(|d| text.contains(d)))
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(opened, ["floatwin.rs", "main.rs"]);
        // And live: no room asks for a second window.
        let mut h = EditorHarness::as_shipped();
        for room in crate::PROBE_ROOMS {
            room.enter(h.editor_mut());
            h.run();
            let windows = &h.kit().output().viewport_output;
            assert!(windows.len() == 1 && windows.contains_key(&egui::ViewportId::ROOT), "{room:?}");
        }
    }

    /// Today's Drawing recipe, spelled out: `dock()` will move on, and what
    /// the frozen boot holds must not.
    fn todays_drawing_recipe() -> egui_dock::DockState<Pane> {
        let mut ds = egui_dock::DockState::new(vec![Pane::Canvas]);
        let tree = ds.main_surface_mut();
        let root = egui_dock::NodeIndex::root();
        let [_, rail] = tree.split_left(root, 0.24, vec![Pane::XSheet]);
        tree.split_below(rail, 0.62, vec![Pane::Layers, Pane::Presets, Pane::Forge]);
        ds
    }

    #[test]
    fn auseam_frozen_boot_is_todays_recipe() {
        // An install that locked its positions moves nothing under this boot.
        let dir = tempfile::tempdir().unwrap();
        let mut saved = Config::default();
        saved.ui.lock_positions = true;
        saved.save_to(dir.path()).unwrap();
        let mut h = EditorHarness::with_dir(NewProjectForm::default().new_state(), dir);
        assert!(!h.rig().config.ui.lock_positions, "positions unlocked");
        let dock = |d: &egui_dock::DockState<Pane>| serde_json::to_value(d).unwrap();
        h.editor_mut().enter_room(Stage::Drawing);
        assert_eq!(dock(&h.editor().dock), dock(&todays_drawing_recipe()));
        assert_eq!(dock(&Stage::dock_with_sheet()), dock(&todays_drawing_recipe()));
        assert_ne!(dock(&Stage::Drawing.dock()), dock(&Stage::dock_with_sheet()), "Draw is the canvas alone");
        h.run();
        assert!(h.editor().pane_rects.iter().any(|(p, ..)| *p == Pane::XSheet), "the sheet is on");
        // No room arms: the rooms' stacks never reach the document.
        let before = what_is_armed(&h);
        h.editor_mut().enter_room(Stage::InkPaint);
        h.run_frames(3);
        assert_eq!(what_is_armed(&h), before);
        assert!(h.state().stacks.is_empty());
        assert_eq!(h.rig().stack_room_seen, "");
    }

    #[test]
    fn auseam_with_state_is_frozen_too() {
        let blank = || NewProjectForm::default().new_state();
        let mut h = EditorHarness::with_state(blank());
        assert_eq!(h.boot(), Boot::Frozen);
        assert!(!h.rig().arms && !h.rig().config.ui.lock_positions);
        // A rebuilt editor keeps the boot it was started under, either way.
        h.rebuild_editor(blank());
        h.run_frames(2);
        assert_eq!(h.boot(), Boot::Frozen);
        assert!(h.state().stacks.is_empty());
        let mut shipped = EditorHarness::as_shipped();
        shipped.rebuild_editor(blank());
        shipped.run_frames(2);
        assert_eq!(shipped.boot(), Boot::Shipped);
        assert!(!shipped.state().stacks.is_empty(), "armed after the rebuild");
    }

    #[test]
    fn auseam_sources_walk_is_recursive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("top.rs"), "top").unwrap();
        std::fs::write(dir.path().join("a/b/deep.rs"), "deep").unwrap();
        std::fs::write(dir.path().join("a/skip.txt"), "not rust").unwrap();
        let found = sources_under(dir.path());
        let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["a/b/deep.rs", "top.rs"]);
        assert_eq!(found[0].1, "deep");
        // Today's flat names stay what the ratchets call them.
        assert!(sources().iter().any(|(n, _)| n == "main.rs"));
    }

    /// The suite's size, in `#[test]`s and assert uses across `sources()`.
    /// Each part raises these to its own count; none lowers them.
    const TEST_FLOOR: usize = 872;
    const ASSERT_FLOOR: usize = 5365;

    fn suite_counts(files: &[(String, String)]) -> (usize, usize) {
        let test = concat!("#[", "test]");
        let asserts = [concat!("assert", "!("), concat!("assert", "_eq!("), concat!("assert", "_ne!(")];
        let (mut tests, mut uses) = (0, 0);
        for (_, text) in files {
            tests += text.matches(test).count();
            uses += asserts.iter().map(|a| text.matches(a).count()).sum::<usize>();
        }
        (tests, uses)
    }

    #[test]
    fn auseam_assert_floor_holds() {
        let planted = concat!(
            "#[", "test] fn a() { assert", "!(x); assert", "_eq!(y, z); assert", "_ne!(y, z); }"
        );
        assert_eq!(suite_counts(&[("a.rs".into(), planted.into())]), (1, 3), "the counter counts");
        let files = sources();
        let (tests, uses) = suite_counts(&files);
        println!("tests {tests} (floor {TEST_FLOOR}), asserts {uses} (floor {ASSERT_FLOOR})");
        assert!(tests >= TEST_FLOOR, "tests fell to {tests}, under {TEST_FLOOR}");
        assert!(uses >= ASSERT_FLOOR, "asserts fell to {uses}, under {ASSERT_FLOOR}");
        // Near enough the count to bite: losing one big test file falls through.
        let (_, big) = files.iter().map(|f| (suite_counts(std::slice::from_ref(f)).1, f)).max().unwrap();
        let rest: Vec<_> = files.iter().filter(|f| f.0 != big.0).cloned().collect();
        let (fewer, less) = suite_counts(&rest);
        assert!(fewer < TEST_FLOOR && less < ASSERT_FLOOR, "the floors are too far down");
    }

    #[test]
    fn auseam_vocab_table_is_pinned() {
        use crate::vocab::TABLE;
        assert!(TABLE.len() >= 53, "section 3 has 53 rows: {}", TABLE.len());
        let files = sources();
        let declared = files.iter().find(|(n, _)| n == "vocab.rs").unwrap();
        let rows = declared.1.lines().filter(|l| l.starts_with("pub const ")).count();
        assert_eq!(rows, TABLE.len() + 1, "every constant is pinned; TABLE is the other line");
        let elsewhere: Vec<String> = files
            .iter()
            .filter(|(n, _)| n != "vocab.rs")
            .map(|(_, text)| text.to_lowercase())
            .collect();
        for (name, value, today) in TABLE {
            assert_eq!(value, today, "{name} moved: a word flips in its own part");
            let word = today.to_lowercase();
            let reads = format!("vocab::{name}").to_lowercase();
            assert!(
                elsewhere.iter().any(|t| t.contains(&word) || t.contains(&reads)),
                "{name}: {today:?} is on no screen"
            );
        }
    }

    /// The block as the App held it at 8e61baf, its one App field passed in:
    /// `Editor::arm_room` is held to it.
    fn todays_block(editor: &mut Editor, layers_cfg: &config::LayersConfig, seen: &mut &'static str) {
        let room_key = crate::stage_key(editor.stage);
        let stack = layers_cfg.stack_for_room(room_key);
        if *seen != room_key && !editor.canvas.stroke_active() {
            *seen = room_key;
            let (n, r) = stack.armed();
            editor.state.view.active_layer = n;
            editor.state.view.active_role = r;
        }
        editor.state.new_cel_active = stack.armed();
        editor.state.new_cel_template = stack.layers.clone();
        editor.state.stack_current = stack.name.clone();
        editor.state.stacks = layers_cfg.all_stacks();
    }

    fn what_is_armed(h: &EditorHarness) -> String {
        let s = h.state();
        format!(
            "{:?}",
            (
                &s.view.active_layer,
                &s.view.active_role,
                &s.new_cel_active,
                &s.new_cel_template,
                &s.stack_current,
                &s.stacks,
            )
        )
    }

    #[test]
    fn auseam_arm_room_matches_todays_block() {
        let (mut moved, mut kept) = (EditorHarness::new(), EditorHarness::new());
        let layers = moved.rig().config.layers.clone();
        let (mut seen_moved, mut seen_kept) = ("", "");
        // The rooms that block knew: the Export step came later, and arms
        // nothing (`auexport_arming_key_skips_export`).
        let rooms = crate::PROBE_ROOMS.into_iter().filter(|r| !matches!(r, crate::ProbeRoom::Stage(Stage::Export)));
        for room in rooms {
            for _twice in 0..2 {
                room.enter(moved.editor_mut());
                room.enter(kept.editor_mut());
                moved.editor_mut().arm_room(&layers, &mut seen_moved);
                todays_block(kept.editor_mut(), &layers, &mut seen_kept);
                assert_eq!(what_is_armed(&moved), what_is_armed(&kept), "{room:?}");
                assert_eq!(seen_moved, seen_kept, "{room:?}");
                // Someone picks another working layer: a room re-entered
                // while `seen` holds it must leave that choice alone.
                moved.state_mut().view.active_layer = "picked".into();
                kept.state_mut().view.active_layer = "picked".into();
            }
        }
        // Under a live pen the working layer holds, and the tail still runs.
        moved.editor_mut().enter_room(Stage::Drawing);
        kept.editor_mut().enter_room(Stage::Drawing);
        for h in [&mut moved, &mut kept] {
            h.run();
            let c = h.editor().canvas.dbg_rect.center();
            h.press_and_move(c, c + egui::vec2(40.0, 0.0), 4);
            assert!(h.editor().canvas.stroke_active());
            h.state_mut().view.active_layer = "under the pen".into();
            h.state_mut().stack_current.clear();
        }
        let (mut seen_moved, mut seen_kept) = ("elsewhere", "elsewhere");
        moved.editor_mut().arm_room(&layers, &mut seen_moved);
        todays_block(kept.editor_mut(), &layers, &mut seen_kept);
        assert_eq!(what_is_armed(&moved), what_is_armed(&kept));
        assert_eq!((seen_moved, seen_kept), ("elsewhere", "elsewhere"), "not armed under a pen");
        assert_eq!(moved.state().view.active_layer, "under the pen");
        assert!(!moved.state().stack_current.is_empty(), "the template is the room's");
    }

    #[test]
    fn auseam_app_holds_no_arming_block() {
        let main = source("main.rs").replace("\r\n", "\n");
        let arm = main.find(concat!("fn arm", "_room(")).expect("Editor::arm_room");
        let end = arm + main[arm..].find("\n    }\n").unwrap();
        let outside = |needle: &str| {
            main.match_indices(needle).filter(|(at, _)| !(arm..end).contains(at)).count()
        };
        for writes in [
            concat!("stack_for", "_room("),
            concat!("new_cel", "_active ="),
            concat!("new_cel", "_template ="),
            concat!("stack", "_current ="),
        ] {
            assert_eq!(outside(writes), 0, "{writes} belongs in arm_room");
        }
        let call = concat!(".arm", "_room(&layers_cfg, &mut self.stack_room_seen)");
        assert_eq!(main.matches(call).count(), 1, "the App arms through it, once a frame");
    }

    /// AU3: the remembered Draw switch cannot be forgotten by one more door:
    /// the two raw constructors have one caller each, and no other file has
    /// any. The test kit and the App come through the boots too.
    #[test]
    fn audock_every_editor_is_built_through_boot() {
        let raw = [concat!("Editor::from", "_state("), concat!("Editor::from", "_form(")];
        let selfish = [concat!("Self::from", "_state("), concat!("Self::from", "_form(")];
        let mut built = 0;
        for (name, text) in sources() {
            for word in raw {
                let at_most = usize::from(name == "main.rs");
                assert!(text.matches(word).count() <= at_most, "{name} builds an Editor with {word}");
                built += text.matches(word).count();
            }
            for word in selfish {
                assert_eq!(text.matches(word).count(), 0, "{name} goes round the boot with {word}");
            }
        }
        assert_eq!(built, 2, "each raw constructor is called once, by its boot");
        for raw in ["from_state", "from_form"] {
            assert!(source("main.rs").contains(&format!("fn {raw}(")), "{raw} keeps its name");
        }
    }

    #[test]
    fn auseam_rig_arms_unless_frozen() {
        let room = crate::stage_key(Some(Stage::InkPaint));
        let blank = || NewProjectForm::default().new_state();
        let mut shipped = EditorHarness::with_state_shipped(blank());
        let mut frozen = EditorHarness::with_state(blank());
        let before = what_is_armed(&frozen);
        for h in [&mut shipped, &mut frozen] {
            h.editor_mut().enter_room(Stage::InkPaint);
            h.run_frames(3);
        }
        // As the App arms: the room's stack, its working layer, the strip's list.
        let want = shipped.rig().config.layers.stack_for_room(room);
        let s = shipped.state();
        assert_eq!(s.stack_current, want.name);
        assert_eq!(format!("{:?}", s.view.active_role), format!("{:?}", want.armed().1));
        assert_eq!(s.view.active_layer, want.armed().0);
        assert_eq!(shipped.rig().stack_room_seen, room);
        assert_ne!(what_is_armed(&shipped), before);
        // The Rig under the frozen boot never did, and tests depend on it.
        assert_eq!(what_is_armed(&frozen), before);
        assert_eq!(frozen.rig().stack_room_seen, "");
    }

    const SCROLL_AREA: &str = concat!("ScrollArea", "::");

    /// Where `ScrollArea`s open (true) and notes close them, in file order.
    fn scroll_marks(text: &str) -> Vec<(usize, bool)> {
        let note = concat!("note_", "viewport(");
        let mut marks: Vec<(usize, bool)> = text.match_indices(SCROLL_AREA).map(|(at, _)| (at, true)).collect();
        marks.extend(text.match_indices(note).map(|(at, _)| (at, false)));
        marks.sort();
        marks
    }

    /// `ScrollArea`s with no note after them. A note closes the latest open
    /// area, so nested ones read right.
    fn unregistered_scrolls(text: &str) -> usize {
        let (mut open, mut strays) = (0usize, 0);
        for (_, opens) in scroll_marks(text) {
            match (opens, open) {
                (true, _) => open += 1,
                (false, 0) => strays += 1,
                (false, _) => open -= 1,
            }
        }
        open + strays
    }

    /// Notes not naming the axes the area they close scrolls.
    fn misnoted_scrolls(text: &str) -> usize {
        let (mut open, mut wrong) = (Vec::new(), 0);
        for (at, opens) in scroll_marks(text) {
            if opens {
                open.push(&text[at + SCROLL_AREA.len()..]);
            } else if let Some(built) = open.pop() {
                let axes = if built.starts_with("vertical") {
                    "[false, true]"
                } else if built.starts_with("horizontal") {
                    "[true, false]"
                } else {
                    "[true, true]"
                };
                wrong += usize::from(!text[at..].lines().next().unwrap_or("").contains(axes));
            }
        }
        wrong
    }

    #[test]
    fn auseam_every_scroll_area_is_registered() {
        let one_left = concat!(
            "ScrollArea", "::vertical().show(ui, f); ScrollArea", "::both().show(ui, g); ",
            "plate::note_", "viewport(ui, r, [true, true]);"
        );
        assert_eq!(unregistered_scrolls(one_left), 1, "the counter counts");
        assert_eq!(misnoted_scrolls(one_left), 0, "`both` is both");
        let crossed = concat!("ScrollArea", "::horizontal().show(ui, f); note_", "viewport(ui, r, [false, true]);");
        assert_eq!(misnoted_scrolls(crossed), 1, "the axes are read");
        let mut sites = 0;
        for (name, text) in sources() {
            // The plate defines the note; measure's own test leaves one out.
            if name == "plate.rs" || name == "measure.rs" {
                continue;
            }
            assert_eq!(unregistered_scrolls(&text), 0, "{name}: add plate::note_viewport");
            assert_eq!(misnoted_scrolls(&text), 0, "{name}: name the axes that scroll");
            sites += text.matches(concat!("note_", "viewport(")).count();
        }
        assert!(sites >= 19, "the spec's 19 sites, found {sites}");
    }

    // ---- AU2: what the look holds everywhere, and what it left alone ----

    /// Is the byte at `at` inside a `plate` FIELD call? Only the last one
    /// opened before it can hold it.
    fn in_a_field(text: &str, at: usize) -> bool {
        let routes = ["plate::field", "plate::text_field", "plate::pattern_field"];
        let last = routes.iter().filter_map(|route| text[..at].rfind(route)).max();
        last.is_some_and(|from| {
            let mut depth = 0;
            for c in text[from..at].chars() {
                match c {
                    '(' => depth += 1,
                    ')' if depth == 1 => return false,
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            depth > 0
        })
    }

    /// A file's fields, how many stand frameless, and those outside the
    /// plate's FIELD. A `Slider` with its value box off holds no field.
    fn fields_in(text: &str) -> (usize, usize, Vec<String>) {
        let calls = [
            "TextEdit::singleline(",
            "TextEdit::multiline(",
            "DragValue::new(",
            "Slider::new(",
            "text_edit_singleline(",
            "text_edit_multiline(",
        ];
        let (mut sites, mut frameless, mut bare) = (0, 0, Vec::new());
        for call in calls {
            for (at, _) in text.match_indices(call) {
                sites += 1;
                let statement = &text[at..at + text[at..].find(';').unwrap_or(text.len() - at)];
                if in_a_field(text, at) {
                    continue;
                }
                if call.starts_with("Slider") && statement.contains("show_value(false)") {
                    continue;
                }
                if statement.contains("Frame::NONE") {
                    frameless += 1;
                    continue;
                }
                bare.push(statement.lines().next().unwrap_or(statement).to_string());
            }
        }
        (sites, frameless, bare)
    }

    /// Every field is the `WELL` with the 1 pt `INK3` edge (spec 2.6), which
    /// only `plate` paints: at AU2's review 0 of 33 `TextEdit`s had it. The
    /// sheet's entry alone goes frameless: its cell is its frame.
    #[test]
    fn aulook_every_field_goes_through_the_plate() {
        let seeded = "\
            ui.add(egui::TextEdit::singleline(&mut a));\n\
            ui.text_edit_multiline(&mut b);\n\
            ui.add(egui::DragValue::new(&mut c).speed(1));\n\
            ui.add(egui::Slider::new(&mut d, 0..=9));\n\
            ui.add(egui::Slider::new(&mut e, 0..=9).show_value(false));\n\
            plate::field(ui, egui::DragValue::new(&mut f));\n\
            let r = plate::text_field(\n    ui,\n    egui::TextEdit::multiline(&mut g),\n);\n\
            plate::field(ui, f); ui.add(egui::DragValue::new(&mut h));\n\
            match k { A => plate::field(ui, b), B => ui.add(egui::DragValue::new(&mut i)) };\n\
            ui.add(egui::TextEdit::singleline(&mut j).frame(egui::Frame::NONE));\n";
        let (sites, frameless, bare) = fields_in(seeded);
        assert_eq!((sites, frameless, bare.len()), (10, 1, 6), "{bare:?}");
        let (mut sites, mut frameless, mut bare) = (0, 0, Vec::new());
        for (name, text) in sources().iter().filter(|(name, _)| name != "plate.rs") {
            let (here, loose, found) = fields_in(&shipped(text));
            sites += here;
            frameless += loose;
            bare.extend(found.into_iter().map(|f| format!("{name}: {f}")));
        }
        assert!(bare.is_empty(), "{} fields outside the plate:\n{}", bare.len(), bare.join("\n"));
        assert_eq!(frameless, 1, "the sheet's entry, and nothing else");
        assert!(sites >= 90, "the scan read {sites} sites");
    }

    /// A stock checkbox takes the style's rest: no edge, and a radius that
    /// rounds its 14 pt box to a `WELL` disc nobody reads as a control. The
    /// latch is the ringed box, and it answers to the same calls.
    #[test]
    fn aulook_every_checkbox_is_a_latch() {
        let sources = sources();
        let stock = sources.iter().filter(|(_, text)| shipped(text).contains(".checkbox("));
        let found: Vec<&str> = stock.map(|(name, _)| name.as_str()).collect();
        assert!(found.is_empty(), "stock checkboxes in {found:?}");
    }

    /// The app's windows are the three it always opened, call by call: the
    /// file by file rule above would pass a fourth in `main.rs`.
    #[test]
    fn aulook_nothing_opens_a_window() {
        let boot = concat!("run_", "native(");
        let main_view = concat!("show_", "viewport_immediate(");
        let float = concat!("show_", "viewport_deferred(");
        let mut found = Vec::new();
        for (name, text) in sources() {
            for call in [boot, main_view, float] {
                let n = text.matches(call).count();
                if n > 0 {
                    found.push((name.clone(), call, n));
                }
            }
        }
        let want = [("floatwin.rs", float, 1), ("main.rs", boot, 1), ("main.rs", main_view, 1)];
        assert_eq!(found, want.map(|(name, call, n)| (name.to_string(), call, n)));
    }

    fn digest(text: &str) -> u64 {
        text.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
    }

    /// The item opening on the line that starts with `head`, indent and all,
    /// through the line that closes it: a shader's `"#;`, an item's own `}`.
    /// An empty `head` is the whole file.
    fn item_of(text: &str, head: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        if head.is_empty() {
            return lines.join("\n");
        }
        let first = lines.iter().position(|l| l.starts_with(head));
        let first = first.unwrap_or_else(|| panic!("no line starts {head:?}"));
        let indent = head.len() - head.trim_start().len();
        let close = if head.starts_with("const ") { "\"#;".to_string() } else { " ".repeat(indent) + "}" };
        let last = (first..lines.len()).find(|&i| lines[i] == close);
        lines[first..=last.unwrap_or_else(|| panic!("{head:?} never closes"))].join("\n")
    }

    /// What the look was told to leave alone, by digest: the shaders, the
    /// wire, the stroke commits and undo, the dock's save and the pen route.
    /// Each digest was read from the commit before AU2, so a match says it
    /// did not touch them. A later part that owns one re-pins it here, and
    /// says so in its commit body.
    const LEFT_ALONE: [(&str, &str, u64); 15] = [
        ("graphcomp.rs", "const XFORM_SHADER", 0x95f089600de90992),
        ("graphcomp.rs", "const BLEND_SHADER", 0x35d164fec72ac70c),
        ("paint.rs", "const SHADER", 0xa0fde5985464fb0f),
        ("paint.rs", "const COMPOSITE_SHADER", 0x208a30bfac9192e5),
        ("net.rs", "", 0xe23a7ac3a222dd11),
        ("doc.rs", "    pub fn commit_stroke", 0xa2fcb632dfd8a2a1),
        ("doc.rs", "    pub fn commit_raster", 0xdc168671a7f290e0),
        ("doc.rs", "    fn land_raster", 0x39b08904694b93d1),
        ("doc.rs", "    pub fn undo(", 0xe15edd6f411d2b83),
        ("doc.rs", "    pub fn redo(", 0xcaa16d99af9d7dbd),
        ("doc.rs", "    pub fn undo_to(", 0x387309403f18550e),
        ("workspace.rs", "    pub fn load_from", 0xeedf52910f122df8),
        ("workspace.rs", "    pub fn save_to", 0x18ceee7d714dc4d4),
        // AU4: `seen_pen` latches past the layer gate, and only there.
        ("canvas.rs", "    fn handle_pen(", 0x999c4cb09447e79e),
        ("main.rs", "fn pump_tablet(", 0x16ed0af8f504e210),
    ];

    #[test]
    fn aulook_the_seams_stand_as_they_stood() {
        assert_eq!(digest("a"), 0xaf63dc4c8601ec8c, "FNV-1a");
        let seeded = "fn before() {\n}\n    fn item(\n        a: u8,\n    ) {\n    }\n    fn after() {\n    }\n";
        assert_eq!(item_of(seeded, "    fn item("), "    fn item(\n        a: u8,\n    ) {\n    }");
        let shader = "const A: &str = r#\"\nfn f() {\n}\n\"#;\nconst B: u8 = 1;\n";
        assert_eq!(item_of(shader, "const A"), "const A: &str = r#\"\nfn f() {\n}\n\"#;");
        let files = sources();
        for (file, head, pinned) in LEFT_ALONE {
            let text = &files.iter().find(|(name, _)| name == file).unwrap().1;
            let got = digest(&item_of(text, head));
            assert_eq!(got, pinned, "{file} {head:?} moved to 0x{got:016x}: is this part meant to?");
        }
    }

    /// The config's sections and the `ui` section's keys: its shape, and only
    /// its names, so nothing of a session's secrets can reach a pin.
    #[test]
    fn aulook_config_shape_stands() {
        let config = serde_json::to_value(Config::default()).unwrap();
        let keys = |v: &serde_json::Value| v.as_object().unwrap().keys().cloned().collect::<Vec<String>>();
        assert_eq!(
            keys(&config),
            [
                "keybinds", "keymap", "layers", "light_table", "onion", "pen", "perf", "presets", "render",
                "session", "ui", "update_repo",
            ]
        );
        assert_eq!(
            keys(&config["ui"]),
            [
                "exact_descriptions", "hints_off", "hints_seen", "keymap_rev", "lock_positions", "quick_start",
                "scale", "sheet_in_draw", "show_welcome", "whats_new_seen",
            ]
        );
    }

    /// AU2's leave-out list: no renames, no menu moves, no new Actions. The
    /// parts that make them (AU3, AU4, AU5, AU6, AU7, AU9, AU10, AU11) re-pin
    /// this and say so in their commit bodies. AU4 brought the rail and
    /// `CanvasOptions`, AU5 `AddDrawingAfter` and `GoToFrame`, AU6 `WhatsNew`
    /// and `ToggleLockPositions`, AU7 `ToggleHints`, AU9 `RoomExport` and the
    /// four plain exports, AU10 `HideTools`.
    #[test]
    fn aulook_what_was_left_out_stays_out() {
        let actions: String =
            Action::ALL.iter().map(|a| format!("{a:?}|{}|{:?}\n", a.label(), a.menu())).collect();
        let panes: String = Pane::ALL.iter().map(|p| format!("{p:?}|{}\n", p.title())).collect();
        assert_eq!((Action::ALL.len(), digest(&actions)), (166, 0x941d1b60acffda57), "the Actions, their words, their menus");
        assert_eq!((Pane::ALL.len(), digest(&panes)), (16, 0x20742d9b6c3fefed), "the panes and their titles");
    }
}
