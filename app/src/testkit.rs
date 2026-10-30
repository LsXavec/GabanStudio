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

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::canvas::CanvasTool;
use crate::config::{self, Action, Config};
use crate::doc::AppState;
use crate::workspace::{Stage, Workspace, Workspaces};
use crate::{AppRequest, Editor, Notes, Performed, shortcuts};

/// Ctrl as the platform reports it on Windows/Linux (the chord matcher
/// reads `command`).
pub const CTRL: egui::Modifiers = egui::Modifiers {
    ctrl: true,
    command: true,
    ..egui::Modifiers::NONE
};

/// A primary-button press (or release) at `pos`.
fn primary(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
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
}

impl Rig {
    fn frame(&mut self, ui: &mut egui::Ui) {
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
        // GM2: the shell's outbox and autosave interval, as the App wires them.
        self.requests.append(&mut self.editor.requests);
        if let Some(on) = self.editor.scrub_request.take() {
            self.config.perf.scrub_audio = on;
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
            let r = self.editor.perform(a);
            self.performed.push((a, r));
        }
    }
}

/// The whole editor on a 1280×800 screen.
pub struct EditorHarness {
    h: Harness<'static, Rig>,
    dir: tempfile::TempDir,
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
        let (config, config_notes) = Config::load_from(dir.path());
        let editor = Editor::from_state(state, None, Some(dir.path()));
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
        };
        let h = Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .with_pixels_per_point(1.0)
            .with_max_steps(12)
            .with_wait_for_pending_images(false)
            .build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
        let mut me = Self { h, dir };
        me.run();
        me
    }

    pub fn with_ppp(mut self, ppp: f32) -> Self {
        self.h.set_pixels_per_point(ppp);
        self.run();
        self
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
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
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

    #[track_caller]
    pub fn click(&mut self, label: &str) {
        let count = self.h.query_all_by_label(label).count();
        assert!(count == 1, "click({label:?}): {count} widget(s)\n{}", self.describe());
        self.h.get_by_label(label).click();
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

    // ---- the harness itself ----

    #[test]
    fn testkit_editor_runs_headless() {
        let mut h = EditorHarness::new();
        h.run_frames(10);
        let ed = h.editor();
        assert!(ed.paint.is_none() && ed.graph.is_none(), "a GPU resource was created");
        assert!(ed.audio_out.is_none() && !ed.audio_tried, "an audio device was opened");
        let tree = h.describe();
        for room in [
            "room layout",
            "room genga (drawing)",
            "room shiage (ink & paint)",
            "room satsuei (composite)",
            "room henshū (edit)",
        ] {
            assert!(h.has(room), "room switcher lacks {room:?}:\n{tree}");
        }
        for tool in ["select", "fill", "lasso fill"] {
            assert!(h.has(tool), "tool deck lacks {tool:?}:\n{tree}");
        }
    }

    #[test]
    fn testkit_click_fill_tool_by_label() {
        let mut h = EditorHarness::new();
        assert!(h.editor().canvas.tool == CanvasTool::Paint);
        h.click("fill");
        assert!(h.editor().canvas.tool == CanvasTool::Fill);
    }

    #[test]
    #[should_panic(expected = "room genga (drawing)")]
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
        h.click(&format!("genga pencil ({digit})"));
        assert_eq!(h.editor().canvas.arming_pencil(), "genga");
        assert!(!h.editor().canvas.erasing);
        h.click("eraser");
        assert!(h.editor().canvas.erasing, "the eraser slot did not arm");
        assert_eq!(h.editor().canvas.arming_pencil(), "eraser");
        let tree = h.describe();
        let layer = h.state().active_layer_name();
        assert!(h.has(&format!("layer: {layer}")), "no layer chip {layer:?}\n{tree}");
        assert!(h.has(&format!("lock layer {layer}")), "no lock for {layer:?}\n{tree}");
        assert!(h.has(&format!("layer colour {layer}")), "no colour dot for {layer:?}\n{tree}");
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
        h.click("room");
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
        let dir = h.data_dir().to_path_buf();
        let mut state = AppState::new_project("again", 1920, 1080, 24, 300.0);
        state.new_drawing_at_frame();
        h.rig_mut().editor = Editor::from_state(state, None, Some(&dir));
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
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "rs") {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                out.push((name, std::fs::read_to_string(&path).unwrap()));
            }
        }
        out.sort();
        out
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

    /// The refreshed sample renders every frame through the CPU golden
    /// renderer, and carries its generated serifu line: a Dialogue track
    /// exactly as long as the cut, sounding in the mixdown a delivery muxes.
    #[test]
    fn ship_sample_renders_every_frame_and_speaks() {
        let s = crate::sample::build_sample();
        let at = s.at();
        let cut = s.engine.project.cut(at.scene, at.cut).unwrap();
        let (w, h) = crate::sample::SIZE;
        assert_eq!(cut.frame_count, crate::sample::FRAMES);
        for f in 0..cut.frame_count {
            let px = anim_core::export::render_graph_frame_law(cut, s.engine.project.color_law, f, w, h)
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

    /// The probe walks every room with a canvas — the spine, the manga
    /// room and the forge — and no room is left out of it.
    #[test]
    fn ship_probe_walks_every_room() {
        let stages: Vec<Stage> = crate::PROBE_ROOMS
            .iter()
            .filter_map(|r| match r {
                crate::ProbeRoom::Stage(st) => Some(*st),
                crate::ProbeRoom::Forge => None,
            })
            .collect();
        for st in [
            Stage::Layout,
            Stage::Drawing,
            Stage::InkPaint,
            Stage::Composite,
            Stage::Edit,
            Stage::Manga,
        ] {
            // Exhaustive: a new Stage fails to compile until it is listed.
            match st {
                Stage::Layout
                | Stage::Drawing
                | Stage::InkPaint
                | Stage::Composite
                | Stage::Edit
                | Stage::Manga => assert!(stages.contains(&st), "{} is not probed", st.name()),
            }
        }
        assert!(crate::PROBE_ROOMS.iter().any(|r| matches!(r, crate::ProbeRoom::Forge)));
    }

    fn probe_holds_still(room: crate::ProbeRoom) {
        for ppp in crate::PROBE_SCALES {
            let run = room.probe(ppp);
            let lines = run.lines.join("\n");
            assert_eq!(run.unmeasured, 0, "{} at {ppp}: no canvas rect\n{lines}", room.name());
            assert_eq!(run.moved, 0, "{} at {ppp}: the canvas moved\n{lines}", room.name());
        }
    }

    #[test]
    fn ship_probe_manga_room() {
        probe_holds_still(crate::ProbeRoom::Stage(Stage::Manga));
    }

    #[test]
    fn ship_probe_forge_room() {
        probe_holds_still(crate::ProbeRoom::Forge);
    }
}
