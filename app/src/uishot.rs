//! Headless pictures of the real editor: `cargo test -p animstudio uishot --
//! --ignored` with UISHOT_DIR set. No window opens; a GPU adapter draws the
//! same egui output the app presents, so a redesign is looked at last, on
//! real pixels, without anything surfacing on the desktop.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anim_core::command::Command;
use anim_core::xsheet::Exposure;
use eframe::egui;

use crate::canvas::CanvasTool;
use crate::config::{Action, SettingsCategory};
use crate::doc::AppState;
use crate::floats::Float;
use crate::measure::room_tag;
use crate::newproject::NewProjectForm;
use crate::plate;
use crate::say::{Spoken, Verb};
use crate::testkit::EditorHarness;
use crate::viewer::GraphView;
use crate::workspace::Stage;
use crate::worksafe::{Autosave, Recovery, RunningRecord, recovery_ui};
use crate::{PROBE_ROOMS, ProbeRoom};

/// The sizes every room is shot at, and what each adds to the file name.
const SIZES: [(u32, u32, &str); 2] = [(1600, 900, ""), (1280, 800, "-1280")];

/// `shots-before`'s files: the After set answers each under the same name.
const SHOTS_BEFORE: [&str; 6] = [
    "room-0-Layout.png",
    "room-1-Drawing.png",
    "room-2-InkPaint.png",
    "room-3-Composite.png",
    "room-4-Edit.png",
    "room-5-Manga.png",
];

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(std::env::var("UISHOT_DIR").expect("set UISHOT_DIR to the output folder"));
    std::fs::create_dir_all(&dir).expect("make UISHOT_DIR");
    dir
}

fn shot_name(i: usize, room: ProbeRoom, suffix: &str) -> String {
    format!("room-{i}-{}{suffix}.png", room_tag(room))
}

/// Every picture a run owes: its file name and its size in pixels.
fn shots_owed() -> Vec<(String, u32, u32)> {
    let rooms = PROBE_ROOMS.into_iter().enumerate();
    let owed = SIZES.iter().flat_map(|(w, h, suffix)| {
        rooms.clone().map(move |(i, room)| (shot_name(i, room, suffix), *w, *h))
    });
    owed.collect()
}

/// `dir` holds every picture owed, each a PNG of its size.
fn check_shots(dir: &Path, owed: &[(String, u32, u32)]) -> Result<(), String> {
    for (name, w, h) in owed {
        let file = std::fs::File::open(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
        let reader = png::Decoder::new(file).read_info().map_err(|e| format!("{name}: {e}"))?;
        let (got_w, got_h) = (reader.info().width, reader.info().height);
        if (got_w, got_h) != (*w, *h) {
            return Err(format!("{name}: {got_w}x{got_h}, owed {w}x{h}"));
        }
    }
    Ok(())
}

/// The room, arrived: thumbnails decoded, the compositor's picture back.
/// Its render runs on its own thread, so this one waits in real time. The
/// wait is bounded, and a viewer left with no picture stops the walk there:
/// a shot of its empty state is no shot of the room.
fn arrive(h: &mut EditorHarness, room: ProbeRoom) {
    h.run_frames(30);
    for _ in 0..200 {
        if !matches!(h.editor().dbg_graph, GraphView::Rendering(_)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
        h.run_frames(1);
    }
    let graph = h.editor().dbg_graph;
    let empty = matches!(graph, GraphView::Rendering(_) | GraphView::NoGpu);
    assert!(!empty, "{}: the picture never came: {graph:?}", room_tag(room));
}

fn walk(h: &mut EditorHarness, dir: &Path, size: egui::Vec2, suffix: &str) {
    h.kit_mut().set_size(size);
    for (i, room) in PROBE_ROOMS.into_iter().enumerate() {
        room.enter(h.editor_mut());
        arrive(h, room);
        h.shot(size, &dir.join(shot_name(i, room, suffix)));
    }
}

/// The menus, by what opens them.
const MENUS: [&str; 9] = ["file", "edit", "view", "sheet", "cut", "project", crate::vocab::STEP, "window", "help"];

/// The states off the rooms' own floors, by file name.
const STATES: [&str; 8] = [
    "palette",
    "f1-sheet",
    "save-prompt",
    "recovery",
    "settings-general",
    "settings-ui",
    "settings-session",
    "new-cut",
];

fn state_name(state: &str, suffix: &str) -> String {
    format!("state-{state}{suffix}.png")
}

/// Every picture of a state a run owes, kept apart from the rooms' sixteen.
fn states_owed() -> Vec<(String, u32, u32)> {
    let menus = MENUS.map(|menu| format!("menu-{menu}"));
    let states = menus.iter().map(String::as_str).chain(STATES);
    let owed = SIZES.iter().flat_map(|(w, h, suffix)| {
        states.clone().map(move |state| (state_name(state, suffix), *w, *h))
    });
    owed.collect()
}

/// The App's settings window on a page of its own: the App holds it, and no
/// harness holds the App. A default `Config` carries no secret.
fn settings(page: SettingsCategory) -> Box<dyn FnMut(&mut egui::Ui)> {
    let mut config = crate::config::Config::default();
    let (mut open, mut category, mut capturing) = (true, page, None);
    let (mut session_action, mut layout_act, mut join_room) = (None, None, None);
    let (mut check_now, mut show_autosaves, mut layout_name) = (false, false, String::new());
    Box::new(move |ui| {
        crate::config::settings_window(
            ui.ctx(),
            &mut open,
            &mut config,
            &mut capturing,
            &mut category,
            "winit",
            None,
            &mut session_action,
            "",
            false,
            false,
            &[],
            "",
            &mut check_now,
            &[],
            &mut join_room,
            &[],
            false,
            &mut layout_name,
            &mut layout_act,
            None,
            &mut show_autosaves,
            None,
        );
    })
}

/// The recovery offer in its modal: the App's too.
fn recovery() -> Box<dyn FnMut(&mut egui::Ui)> {
    let now = 1_000_000;
    let record = RunningRecord { pid: 7, open: Some("Cut 12.animproj".into()), autosave: None, started: 1 };
    let autosaves = vec![Autosave { path: "a.animproj".into(), at: now - 30 }];
    let mut offer = Recovery { record_path: "running-7-1.json".into(), record, autosaves, selected: 0 };
    Box::new(move |ui| {
        let frame = plate::dialog_frame(&ui.ctx().global_style());
        egui::Modal::new(egui::Id::new("uishot_recovery"))
            .frame(frame)
            .show(ui.ctx(), |ui| recovery_ui(ui, &mut offer, now, 0));
    })
}

/// The new-cut form, as an open editor shows it.
fn new_cut() -> Box<dyn FnMut(&mut egui::Ui)> {
    let mut form = NewProjectForm::default();
    Box::new(move |ui| {
        egui::Window::new("New Project")
            .frame(plate::dialog_frame(ui.style()))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                crate::newproject::form_ui(ui, &mut form, true);
            });
    })
}

fn states(h: &mut EditorHarness, dir: &Path, size: egui::Vec2, suffix: &str) {
    h.kit_mut().set_size(size);
    h.editor_mut().enter_room(Stage::Drawing);
    arrive(h, ProbeRoom::Stage(Stage::Drawing));
    for menu in MENUS {
        h.click(menu);
        h.run_frames(4);
        assert!(egui::Popup::is_any_open(&h.ctx()), "the {menu} shot shows {menu} open");
        h.shot(size, &dir.join(state_name(&format!("menu-{menu}"), suffix)));
        // Not a second click: a long menu can stand over its own label.
        h.key(egui::Key::Escape);
        h.run_frames(4);
        assert!(!egui::Popup::is_any_open(&h.ctx()), "{menu} closed before the next shot");
    }
    for (state, toggle) in [("palette", Action::CommandPalette), ("f1-sheet", Action::ShortcutSheet)] {
        h.perform(toggle);
        h.run_frames(4);
        h.shot(size, &dir.join(state_name(state, suffix)));
        h.perform(toggle);
    }
    h.editor_mut().leave = Some(crate::Leave::Open);
    h.run_frames(4);
    h.shot(size, &dir.join(state_name("save-prompt", suffix)));
    h.editor_mut().leave = None;
    let seeded = [
        ("recovery", recovery()),
        ("settings-general", settings(SettingsCategory::General)),
        ("settings-ui", settings(SettingsCategory::UiFeatures)),
        ("settings-session", settings(SettingsCategory::Session)),
        ("new-cut", new_cut()),
    ];
    for (state, seed) in seeded {
        h.rig_mut().seed = Some(seed);
        h.run_frames(4);
        h.shot(size, &dir.join(state_name(state, suffix)));
    }
    h.rig_mut().seed = None;
}

/// AU3: what only a shipped editor shows, by file name and size: the flush
/// canvas, the sheet it asks for, the place pill, the row and foot gone
/// narrow. 832×520 is the room 1024×640 leaves at 150%.
const SHIPPED: [(&str, u32, u32); 8] = [
    ("flush", 1600, 900),
    ("flush-1280", 1280, 800),
    ("sheet-on", 1600, 900),
    ("sheet-on-1280", 1280, 800),
    ("place-1280", 1280, 800),
    ("narrow", 1024, 640),
    ("narrow-menu", 1024, 640),
    ("narrow-place", 832, 520),
];

fn shipped_name(state: &str) -> String {
    format!("shipped-{state}.png")
}

fn shipped_owed() -> Vec<(String, u32, u32)> {
    SHIPPED.iter().map(|(state, w, h)| (shipped_name(state), *w, *h)).collect()
}

fn shipped(dir: &Path) {
    let mut h = EditorHarness::with_gpu_shipped(crate::sample::build_sample());
    let draw = ProbeRoom::Stage(Stage::Drawing);
    let mut shots = SHIPPED.iter().map(|(state, w, h)| (shipped_name(state), egui::vec2(*w as f32, *h as f32)));
    let mut shoot = |h: &mut EditorHarness, room: ProbeRoom| {
        let (name, size) = shots.next().expect("a shot owed");
        h.kit_mut().set_size(size);
        arrive(h, room);
        h.shot(size, &dir.join(name));
    };
    for _ in SIZES {
        h.run_frames(4);
        shoot(&mut h, draw);
    }
    h.click(crate::vocab::SHOW_TIMING_SHEET);
    for _ in SIZES {
        shoot(&mut h, draw);
    }
    h.click(crate::vocab::HIDE_TIMING_SHEET);
    let place = ProbeRoom::Layout3d;
    place.enter(h.editor_mut());
    shoot(&mut h, place);
    h.editor_mut().enter_room(Stage::Drawing);
    shoot(&mut h, draw);
    h.click("Menu");
    shoot(&mut h, draw);
    h.key(egui::Key::Escape);
    place.enter(h.editor_mut());
    shoot(&mut h, place);
}

/// AU4: the rail's smaller tiers, and a float on each side at once.
const RAIL: [(&str, u32, u32); 3] = [
    ("rail-compact", 832, 520),
    ("rail-pocket", 832, 440),
    ("rail-both-sides", 1024, 640),
];

/// AU4: what shows for a moment or under one tool: the first boot's Draw, a
/// hover card, the dots' popover, the Size bubble at both ends of its range,
/// and Tool options for the tools that have rows of their own. The OS window
/// is not here: a harness has no second window (spec 13, item 4).
const MOMENTS: [(&str, u32, u32); 8] = [
    ("rail-blank-draw", 1600, 900),
    ("rail-hover-card", 1600, 900),
    ("rail-pocket-more", 832, 440),
    ("rail-size-small", 1600, 900),
    ("rail-size-large", 1600, 900),
    ("options-fill", 1600, 900),
    ("options-select", 1600, 900),
    ("options-eraser", 1600, 900),
];

fn float_name(f: Float, suffix: &str) -> String {
    format!("float-{f:?}{suffix}.png")
}

fn rail_owed() -> Vec<(String, u32, u32)> {
    let floats = SIZES.iter().flat_map(|(w, h, suffix)| {
        Float::ALL.into_iter().map(move |f| (float_name(f, suffix), *w, *h))
    });
    let states = RAIL.iter().chain(&MOMENTS);
    floats.chain(states.map(|(state, w, h)| (format!("{state}.png"), *w, *h))).collect()
}

/// AU4: the moments, in `MOMENTS`' order, the blank boot's picture first.
fn moments(h: &mut EditorHarness, dir: &Path) {
    let draw = ProbeRoom::Stage(Stage::Drawing);
    let mut shots = MOMENTS.iter().map(|(state, w, h)| {
        (dir.join(format!("{state}.png")), egui::vec2(*w as f32, *h as f32))
    });
    let (name, size) = shots.next().expect("a shot owed");
    h.kit_mut().set_size(size);
    arrive(h, draw);
    h.shot(size, &name);

    let (name, size) = shots.next().expect("a shot owed");
    h.hover("Brush color");
    h.wait(1.0);
    h.shot(size, &name);

    let (name, pocket) = shots.next().expect("a shot owed");
    h.kit_mut().set_size(pocket);
    arrive(h, draw);
    h.click("More tools");
    h.shot(pocket, &name);
    h.click("More tools");

    for pull in [400.0, -400.0] {
        let (name, size) = shots.next().expect("a shot owed");
        h.kit_mut().set_size(size);
        arrive(h, draw);
        let at = h.rect("Size").center();
        let to = at + egui::vec2(0.0, pull);
        h.press_and_move(at, to, 8);
        h.shot(size, &name);
        h.release(to);
    }
    h.run_frames(4);

    for tool in [Some(CanvasTool::Fill), Some(CanvasTool::Select), None] {
        let (name, size) = shots.next().expect("a shot owed");
        let editor = h.editor_mut();
        match tool {
            Some(tool) => editor.canvas.set_tool(tool, &mut editor.state),
            None => editor.canvas.arm_paint(true, &mut editor.state),
        }
        h.open_float(Float::ToolOptions);
        arrive(h, draw);
        h.shot(size, &name);
    }
}

fn rail_and_floats(dir: &Path) {
    let mut h = EditorHarness::with_gpu_shipped(crate::sample::build_sample());
    let draw = ProbeRoom::Stage(Stage::Drawing);
    h.run_frames(4);
    for (w, height, suffix) in SIZES {
        let size = egui::vec2(w as f32, height as f32);
        h.kit_mut().set_size(size);
        for f in Float::ALL {
            h.open_float(f);
            arrive(&mut h, draw);
            h.shot(size, &dir.join(float_name(f, suffix)));
            h.editor_mut().floats.close(f);
        }
    }
    for (state, w, height) in RAIL {
        let size = egui::vec2(w as f32, height as f32);
        h.kit_mut().set_size(size);
        if state == "rail-both-sides" {
            h.open_float(Float::Brushes);
            h.open_float(Float::Layers);
        }
        arrive(&mut h, draw);
        h.shot(size, &dir.join(format!("{state}.png")));
    }
    drop(h);
    let mut h = EditorHarness::with_gpu_ready();
    h.run_frames(4);
    moments(&mut h, dir);
}

/// AU5: what the strip and the band show on a timed shot: the strip itself at
/// both sizes, the Speed popover, a sheet's card and its menu, and at the
/// narrowest size the fold and a sentence that wraps.
const STRIP: [(&str, u32, u32); 7] = [
    ("strip-plain", 1600, 900),
    ("strip-plain-1280", 1280, 800),
    ("strip-speed", 1600, 900),
    ("strip-card", 1280, 800),
    ("strip-menu", 1280, 800),
    ("strip-fold", 832, 520),
    ("strip-say", 832, 520),
];

fn strip_owed() -> Vec<(String, u32, u32)> {
    STRIP.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

/// Seven drawings on mixed holds, a gap, and one drawing exposed twice.
fn timed_shot() -> AppState {
    let mut s = NewProjectForm::default().new_state();
    s.set_cut_length(32);
    for frame in [0, 4, 6, 12, 14, 22, 26] {
        s.goto(frame);
        s.new_drawing_at_frame();
    }
    let (at, column, first) = (s.at(), s.view.active_column, crate::strip::sheets(&s)[0].shows);
    let gap = Command::SetCell { at, column, frame: 12, key: Some(Exposure::Empty) };
    let twice = Command::SetCell { at, column, frame: 22, key: Some(first) };
    s.engine.apply("shot", vec![gap, twice]).expect("two cells");
    s.goto(6);
    s
}

fn strip_moments(dir: &Path) {
    let mut h = EditorHarness::with_gpu_shipped(timed_shot());
    let draw = ProbeRoom::Stage(Stage::Drawing);
    let shots: Vec<_> = STRIP
        .iter()
        .map(|(state, w, h)| (dir.join(format!("{state}.png")), egui::vec2(*w as f32, *h as f32)))
        .collect();
    let mut shots = shots.into_iter();
    let mut next = |h: &mut EditorHarness| {
        let (name, size) = shots.next().expect("a shot owed");
        h.kit_mut().set_size(size);
        arrive(h, draw);
        (name, size)
    };
    for _ in 0..2 {
        let (name, size) = next(&mut h);
        h.shot(size, &name);
    }
    let (name, size) = next(&mut h);
    h.click("Speed");
    h.shot(size, &name);
    h.key(egui::Key::Escape);
    let (name, size) = next(&mut h);
    h.hover("Drawing 3, frames 7 to 12");
    h.wait(1.0);
    h.shot(size, &name);
    let (name, size) = next(&mut h);
    let at = h.rect("Drawing 3, frames 7 to 12").center();
    h.right_click_pos(at);
    h.shot(size, &name);
    h.key(egui::Key::Escape);
    let (name, size) = next(&mut h);
    h.click("…");
    h.shot(size, &name);
    h.key(egui::Key::Escape);
    let (name, size) = next(&mut h);
    let paper = h.editor().canvas.dbg_rect.center();
    h.point_at(paper);
    h.state_mut().refuse("refused — this sentence runs past the end of the narrow lane, and so it wraps to two rows");
    h.shot(size, &name);
}

/// AU6: the Start float: a fresh install's, at its worst case with two
/// recents, with `More sizes` open, on What's New, and a pen check's answer;
/// on the narrowest window, where its body scrolls.
const START: [(&str, u32, u32); 12] = [
    ("start-fresh", 1600, 900),
    ("start-fresh-1280", 1280, 800),
    ("start-recents", 1600, 900),
    ("start-recents-1280", 1280, 800),
    ("start-more-sizes", 1600, 900),
    ("start-more-sizes-1280", 1280, 800),
    ("start-whats-new", 1600, 900),
    ("start-whats-new-1280", 1280, 800),
    ("start-pen-fix", 1280, 800),
    ("start-pen-mouse", 1280, 800),
    ("start-pen-works", 1280, 800),
    ("start-narrow", 832, 520),
];

fn start_owed() -> Vec<(String, u32, u32)> {
    START.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

fn start_float(dir: &Path) {
    use crate::welcome::PenCheck;
    let names = tempfile::tempdir().expect("a folder for the recent files");
    let recent: Vec<PathBuf> = ["Ball bounce", "Walk cycle"]
        .iter()
        .map(|name| names.path().join(format!("{name}.animproj")))
        .collect();
    let project = crate::sample::build_sample().engine.project;
    for file in &recent {
        anim_core::store::save(&project, file).expect("a recent file");
    }
    let draw = ProbeRoom::Stage(Stage::Drawing);
    let mut h = EditorHarness::with_gpu_first_run();
    h.run_frames(4);
    for (state, w, height) in START {
        let size = egui::vec2(w as f32, height as f32);
        h.kit_mut().set_size(size);
        let mut w = crate::welcome::Welcome::new(&[]);
        if !state.contains("fresh") {
            w = crate::welcome::Welcome::new(&recent);
        }
        w.more_open = state.contains("more-sizes");
        if state.contains("whats-new") {
            w.tab = crate::welcome::WelcomeTab::WhatsNew;
        }
        let pens = [("pen-fix", PenCheck::NoPressure), ("pen-mouse", PenCheck::Mouse), ("pen-works", PenCheck::Works)];
        if let Some((_, said)) = pens.iter().find(|(tag, _)| state.contains(tag)) {
            w.pen = *said;
            w.pen_said = true;
        }
        h.editor_mut().start = Some(w);
        arrive(&mut h, draw);
        h.shot(size, &dir.join(format!("{state}.png")));
    }
}

/// AU7: the lane as it teaches: a hint with its button and dots, under the
/// fold, beside the sheet, and the longest sentence with `Fix it`; each empty
/// step; the empty cell's line; and Export's queue.
const SAY: [(&str, u32, u32); 14] = [
    ("say-hint", 1600, 900),
    ("say-hint-1280", 1280, 800),
    ("say-hint-fold", 832, 520),
    ("say-sheet", 1280, 800),
    ("say-fix-fold", 832, 520),
    ("say-plan", 1600, 900),
    ("say-plan-1280", 1280, 800),
    ("say-timing", 1600, 900),
    ("say-color", 1600, 900),
    ("say-effects", 1600, 900),
    ("say-effects-1280", 1280, 800),
    ("say-empty-cell", 1600, 900),
    ("say-empty-cell-1280", 1280, 800),
    ("say-export", 1600, 900),
];

fn say_owed() -> Vec<(String, u32, u32)> {
    SAY.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

fn say_moments(dir: &Path) {
    let mut shots = SAY.iter().map(|(state, w, h)| (dir.join(format!("{state}.png")), egui::vec2(*w as f32, *h as f32)));
    let mut shoot = |h: &mut EditorHarness, room: ProbeRoom| {
        let (name, size) = shots.next().expect("a shot owed");
        h.kit_mut().set_size(size);
        arrive(h, room);
        // The machine's last remark has its dwell first.
        h.wait(4.2);
        h.shot(size, &name);
    };
    let draw = ProbeRoom::Stage(Stage::Drawing);
    let mut h = EditorHarness::with_gpu_ready();
    for _ in 0..3 {
        shoot(&mut h, draw);
    }
    h.editor_mut().show_sheet();
    shoot(&mut h, draw);
    h.click(crate::vocab::HIDE_TIMING_SHEET);
    let verb = Some(Verb::Fix(Action::NewDrawing));
    h.editor_mut().say.pin = Some(Spoken { bit: None, words: crate::doc::HELD_FRAME.into(), verb });
    shoot(&mut h, draw);
    h.editor_mut().say.pin = None;
    for stage in [Stage::Layout, Stage::Layout, Stage::Edit, Stage::InkPaint, Stage::Composite, Stage::Composite] {
        let room = ProbeRoom::Stage(stage);
        room.enter(h.editor_mut());
        shoot(&mut h, room);
    }
    drop(h);
    let mut s = NewProjectForm::default().new_state();
    s.set_cut_length(6);
    s.goto(3);
    s.new_drawing_at_frame();
    // The canvas line waits for art, or the lane is saying the same thing.
    crate::canvas::extpaper_tests::paint_texels(&mut s, &[(10, 10)], [0.0, 0.0, 0.0, 1.0]);
    s.goto(0);
    let mut h = EditorHarness::with_gpu_shipped(s);
    draw.enter(h.editor_mut());
    for _ in 0..2 {
        shoot(&mut h, draw);
    }
    h.editor_mut().show_me(crate::welcome::EXPORT_STEP);
    shoot(&mut h, ProbeRoom::Stage(Stage::Edit));
}

/// AU8: the queue's preset row with its last two chips, wrapped: the tab in
/// its room, where the pane is narrow, and the pane alone.
const QUEUE: [(&str, u32, u32); 4] = [
    ("queue-room", 1600, 900),
    ("queue-room-1280", 1280, 800),
    ("queue-pane", 1600, 900),
    ("queue-pane-1280", 1280, 800),
];

fn queue_owed() -> Vec<(String, u32, u32)> {
    QUEUE.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

/// The room the queue's tab is shot in.
const QUEUE_ROOM: ProbeRoom = ProbeRoom::Stage(Stage::Composite);

/// No ffmpeg, the search's answer faked: the row a beginner's PC shows.
fn queue_ready(h: &mut EditorHarness) {
    h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
    let gif = anim_core::deliver::PRESETS.iter().position(|p| p.name == "GIF");
    h.state_mut().render_queue.draft.preset = gif.expect("the GIF preset");
}

/// What `state` shows, a frame short of showing it.
fn queue_screen(h: &mut EditorHarness, state: &str, size: egui::Vec2) {
    h.kit_mut().set_size(size);
    if state.contains("room") {
        QUEUE_ROOM.enter(h.editor_mut());
        let tab = h.editor().dock.find_tab(&crate::Pane::RenderQueue).expect("the queue's tab");
        let _ = h.editor_mut().dock.set_active_tab(tab);
    } else {
        h.editor_mut().dock = egui_dock::DockState::new(vec![crate::Pane::RenderQueue]);
    }
}

fn queue_moments(dir: &Path) {
    let mut h = EditorHarness::with_gpu(crate::sample::build_sample());
    queue_ready(&mut h);
    for (state, w, height) in QUEUE {
        let size = egui::vec2(w as f32, height as f32);
        queue_screen(&mut h, state, size);
        if state.contains("room") {
            arrive(&mut h, QUEUE_ROOM);
        } else {
            h.run_frames(4);
        }
        h.shot(size, &dir.join(format!("{state}.png")));
    }
}

#[test]
#[ignore = "needs a GPU adapter and UISHOT_DIR; writes PNGs"]
fn uishot_rooms() {
    let dir = out_dir();
    let mut h = EditorHarness::with_gpu(crate::sample::build_sample());
    h.run_frames(4);
    for (w, height, suffix) in SIZES {
        let size = egui::vec2(w as f32, height as f32);
        walk(&mut h, &dir, size, suffix);
        states(&mut h, &dir, size, suffix);
    }
    check_shots(&dir, &shots_owed()).expect("the shots the spec lists");
    check_shots(&dir, &states_owed()).expect("the states the look reaches");
    drop(h);
    shipped(&dir);
    check_shots(&dir, &shipped_owed()).expect("what only a shipped editor shows");
    rail_and_floats(&dir);
    check_shots(&dir, &rail_owed()).expect("the rail's tiers and every float");
    strip_moments(&dir);
    check_shots(&dir, &strip_owed()).expect("the strip's moments");
    start_float(&dir);
    check_shots(&dir, &start_owed()).expect("the Start float's moments");
    say_moments(&dir);
    check_shots(&dir, &say_owed()).expect("what the Say line teaches");
    queue_moments(&dir);
    check_shots(&dir, &queue_owed()).expect("the queue's preset row");
}

#[test]
fn augif_shots_owe_the_queue_row_at_both_sizes() {
    let owed = queue_owed();
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), owed.len(), "no two shots share a name: {names:?}");
    for state in ["queue-room", "queue-pane"] {
        assert!(owed.contains(&(format!("{state}.png"), 1600, 900)), "{state} at 1600");
        assert!(owed.contains(&(format!("{state}-1280.png"), 1280, 800)), "{state} at 1280");
    }
    let elsewhere = say_owed().into_iter().chain(start_owed()).chain(strip_owed()).chain(rail_owed());
    assert!(elsewhere.into_iter().all(|o| !names.contains(o.0.as_str())), "no name is another group's");
}

/// The screens those shots are of, with no device: the words for no ffmpeg, GIF
/// armed, and every preset's chip inside the window, at both sizes.
#[test]
fn augif_queue_shots_show_the_row_they_name() {
    let mut h = EditorHarness::with_state(crate::sample::build_sample());
    queue_ready(&mut h);
    for (state, w, height) in QUEUE {
        let size = egui::vec2(w as f32, height as f32);
        queue_screen(&mut h, state, size);
        h.run_frames(4);
        assert!(h.has_text_containing(crate::render_queue::NEEDS_FFMPEG), "{state}\n{}", h.describe());
        assert_eq!(h.node_toggled("GIF"), Some(true), "{state}: the armed chip\n{}", h.describe());
        for p in anim_core::deliver::PRESETS {
            let chip = h.rect(p.name);
            assert!(chip.right() <= size.x && chip.bottom() <= size.y, "{state}, {}: {chip:?}", p.name);
        }
    }
}

/// Only the ignored pass writes the queue's shots, so this holds it to them: it
/// takes all four, then checks the folder, and the check misses any one of them.
#[test]
fn augif_the_ignored_pass_owes_the_queue_shots() {
    let pass = include_str!("uishot.rs").split("fn uishot_rooms()").nth(1).expect("the pass");
    let taken = pass.find("queue_moments(&dir);").expect("it takes them");
    let checked = pass.find("check_shots(&dir, &queue_owed())").expect("it checks them");
    assert!(taken < checked, "taken, then checked");
    let dir = tempfile::tempdir().unwrap();
    let owed = queue_owed();
    assert_eq!(owed.len(), 4);
    for (name, w, h) in &owed {
        tiny_png(&dir.path().join(name), *w, *h);
    }
    assert_eq!(check_shots(dir.path(), &owed), Ok(()));
    for (name, w, h) in &owed {
        let file = dir.path().join(name);
        std::fs::remove_file(&file).unwrap();
        let missing = check_shots(dir.path(), &owed);
        assert!(missing.is_err_and(|e| e.starts_with(name.as_str())), "{name} not there");
        tiny_png(&file, *w, *h);
    }
}

#[test]
fn ausay_shots_owe_the_lane_and_each_empty_step() {
    let owed = say_owed();
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), owed.len(), "no two shots share a name: {names:?}");
    for state in ["say-hint", "say-plan", "say-effects", "say-empty-cell"] {
        assert!(names.contains(format!("{state}.png").as_str()), "{state} at 1600");
        assert!(names.contains(format!("{state}-1280.png").as_str()), "{state} at 1280");
    }
    let elsewhere = strip_owed().into_iter().chain(start_owed()).chain(shipped_owed()).chain(rail_owed());
    assert!(elsewhere.into_iter().all(|o| !names.contains(o.0.as_str())), "no name is another group's");
}

#[test]
fn austart_shots_owe_the_float_at_both_sizes() {
    let owed = start_owed();
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), owed.len(), "no two shots share a name: {names:?}");
    for state in ["start-fresh", "start-recents", "start-more-sizes", "start-whats-new"] {
        for (_, w, h) in [(0, 1600, 900), (0, 1280, 800)] {
            let suffix = if w == 1280 { "-1280" } else { "" };
            let name = format!("{state}{suffix}.png");
            assert!(owed.contains(&(name.clone(), w, h)), "{name}");
        }
    }
}

#[test]
fn austrip_shots_owe_the_strip_and_the_fold() {
    let owed = strip_owed();
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), owed.len(), "no two shots share a name: {names:?}");
    let narrow = owed.iter().filter(|o| o.1 < 1040).count();
    assert_eq!((owed.len(), narrow), (STRIP.len(), 2), "the fold and the wrap stand at the narrowest");
    let elsewhere = shipped_owed().into_iter().chain(rail_owed());
    assert!(elsewhere.into_iter().all(|o| !names.contains(o.0.as_str())), "no name is another group's");
}

#[test]
fn aurail_shots_owe_every_float_at_both_sizes() {
    let owed = rail_owed();
    assert_eq!(owed.len(), 2 * Float::ALL.len() + RAIL.len() + MOMENTS.len());
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), owed.len(), "no two shots share a name: {names:?}");
    let moments = [
        "rail-blank-draw",
        "rail-hover-card",
        "rail-pocket-more",
        "rail-size-small",
        "rail-size-large",
        "options-fill",
        "options-select",
        "options-eraser",
    ];
    for moment in moments {
        assert!(names.contains(format!("{moment}.png").as_str()), "{moment}: {names:?}");
    }
}

#[test]
fn audock_shots_owe_the_shipped_boot_too() {
    let shipped = shipped_owed();
    assert_eq!(shipped.len(), 8);
    let names: BTreeSet<&str> = shipped.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), 8, "no two shots share a name: {names:?}");
    assert!(shipped.iter().all(|o| o.0.starts_with("shipped-")), "{names:?}");
    // The ignored pass checks all of these at once.
    assert_eq!(shots_owed().len() + states_owed().len() + shipped.len(), 58);
}

#[test]
fn aulook_shots_owe_every_state_at_both_sizes() {
    let owed = states_owed();
    assert_eq!(owed.len(), 2 * (MENUS.len() + STATES.len()));
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), owed.len(), "no two shots share a name: {names:?}");
    assert!(owed.iter().all(|o| o.0.starts_with("state-")), "the rooms' names stay the rooms'");
}

#[test]
fn auseam_shots_owed_are_sixteen_at_two_sizes_and_answer_shots_before() {
    let owed = shots_owed();
    assert_eq!(owed.len(), 16);
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), 16, "no two shots share a name: {names:?}");
    let sizes: BTreeSet<(u32, u32)> = owed.iter().map(|o| (o.1, o.2)).collect();
    assert_eq!(sizes, BTreeSet::from([(1280, 800), (1600, 900)]));
    for before in SHOTS_BEFORE {
        let after = owed.iter().find(|o| o.0 == before);
        assert_eq!(after.map(|o| (o.1, o.2)), Some((1600, 900)), "{before}");
    }
}

fn tiny_png(path: &Path, w: u32, h: u32) {
    let mut png = png::Encoder::new(std::fs::File::create(path).unwrap(), w, h);
    png.set_color(png::ColorType::Grayscale);
    png.write_header().unwrap().write_image_data(&vec![0; (w * h) as usize]).unwrap();
}

#[test]
fn auseam_shots_check_reads_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let owed = |w| vec![("a.png".to_string(), 4, 3), ("b.png".to_string(), w, 2)];
    tiny_png(&dir.path().join("a.png"), 4, 3);
    let missing = check_shots(dir.path(), &owed(2));
    assert!(missing.is_err_and(|e| e.starts_with("b.png")), "a shot not there");
    tiny_png(&dir.path().join("b.png"), 2, 2);
    assert_eq!(check_shots(dir.path(), &owed(2)), Ok(()));
    let small = check_shots(dir.path(), &owed(5));
    assert_eq!(small, Err("b.png: 2x2, owed 5x2".to_string()));
    std::fs::write(dir.path().join("b.png"), b"no picture").unwrap();
    assert!(check_shots(dir.path(), &owed(2)).is_err(), "not a PNG");
}

/// A viewer with no device has no picture to wait for: the walk names the room.
#[test]
#[should_panic(expected = "Composite: the picture never came: NoGpu")]
fn auseam_shots_stop_on_a_viewer_with_no_picture() {
    let mut h = EditorHarness::with_state(crate::sample::build_sample());
    let room = ProbeRoom::Stage(Stage::Composite);
    room.enter(h.editor_mut());
    arrive(&mut h, room);
}

/// A room showing no viewer has no picture to owe.
#[test]
fn auseam_shots_pass_a_room_with_no_viewer() {
    let mut h = EditorHarness::with_state(crate::sample::build_sample());
    let room = ProbeRoom::Forge;
    room.enter(h.editor_mut());
    arrive(&mut h, room);
    assert_eq!(h.editor().dbg_graph, GraphView::Off);
}
