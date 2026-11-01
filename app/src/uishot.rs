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
/// step; the empty cell's line; and Show me's ring in the Export step.
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
    let mut h = EditorHarness::with_gpu_animation();
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
    // As a Start card's document, or the Export step's Result has nothing.
    s.auto_comp();
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
    shoot(&mut h, ProbeRoom::Stage(Stage::Export));
}

/// AU10: Hide tools: the first time, with its card; a refusal on that card;
/// the dot alone on the paper; the dot in a step whose tabs reach the corner.
const HIDE: [(&str, u32, u32); 8] = [
    ("hide-tools", 1600, 900),
    ("hide-refusal", 1600, 900),
    ("hide-dot", 1600, 900),
    ("hide-timing", 1600, 900),
    ("hide-tools-1280", 1280, 800),
    ("hide-refusal-1280", 1280, 800),
    ("hide-dot-1280", 1280, 800),
    ("hide-timing-1280", 1280, 800),
];

fn hide_owed() -> Vec<(String, u32, u32)> {
    HIDE.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

fn hide_moments(dir: &Path) {
    let mut shots = HIDE.iter().map(|(state, w, h)| (dir.join(format!("{state}.png")), egui::vec2(*w as f32, *h as f32)));
    let draw = ProbeRoom::Stage(Stage::Drawing);
    for _ in 0..2 {
        let mut h = EditorHarness::with_gpu_animation();
        let mut shoot = |h: &mut EditorHarness, room: Option<ProbeRoom>| {
            let (name, size) = shots.next().expect("a shot owed");
            h.kit_mut().set_size(size);
            if let Some(room) = room {
                arrive(h, room);
            }
            h.shot(size, &name);
        };
        h.perform(Action::HideTools);
        shoot(&mut h, Some(draw));
        // T under a live stroke, after `arrive`, whose frames outlast the four
        // seconds it dwells. The shot's own frames do not.
        let at = h.editor().canvas.view_xform().paper_bounds().center();
        let to = at + egui::vec2(120.0, 30.0);
        h.press_and_move(at, to, 4);
        assert!(h.editor().canvas.stroke_active(), "a live stroke");
        h.hold_key(egui::Key::T);
        shoot(&mut h, None);
        let refused = h.painted_text().join(" ");
        assert!(refused.contains("finish the stroke first"), "on the dot's card: {refused}");
        h.release_key(egui::Key::T);
        h.release(to);
        // Past the refusal's dwell, and back once: the row has been answered.
        h.wait(4.2);
        h.perform(Action::HideTools);
        h.run_frames(4);
        h.perform(Action::HideTools);
        shoot(&mut h, Some(draw));
        let timing = ProbeRoom::Stage(Stage::Edit);
        timing.enter(h.editor_mut());
        shoot(&mut h, Some(timing));
    }
}

/// AU18: the three bars, each on its first step, and a step key a drawing's bar
/// refuses; a drawing and a comic on the steps they share, a drawing's dots after
/// its first stroke, the remark as it grows, and a drawing opened over an
/// animation, whose strip does not outlive it.
const KINDS: [(&str, u32, u32); 20] = [
    ("bar-drawing", 1600, 900),
    ("bar-refused", 1600, 900),
    ("drawing-stroked", 1600, 900),
    ("drawing-color", 1600, 900),
    ("drawing-effects", 1600, 900),
    ("drawing-grew", 1600, 900),
    ("bar-animation", 1600, 900),
    ("drawing-over-animation", 1600, 900),
    ("bar-comic", 1600, 900),
    ("comic-color", 1600, 900),
    ("bar-drawing-1280", 1280, 800),
    ("bar-refused-1280", 1280, 800),
    ("drawing-stroked-1280", 1280, 800),
    ("drawing-color-1280", 1280, 800),
    ("drawing-effects-1280", 1280, 800),
    ("drawing-grew-1280", 1280, 800),
    ("bar-animation-1280", 1280, 800),
    ("drawing-over-animation-1280", 1280, 800),
    ("bar-comic-1280", 1280, 800),
    ("comic-color-1280", 1280, 800),
];

fn kinds_owed() -> Vec<(String, u32, u32)> {
    KINDS.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

fn kinds_moments(dir: &Path) {
    let mut shots = KINDS.iter().map(|(state, w, h)| (dir.join(format!("{state}.png")), egui::vec2(*w as f32, *h as f32)));
    let saved = tempfile::tempdir().expect("a temp dir");
    let picture = saved.path().join("picture.animproj");
    anim_core::store::save(&crate::templates::READY.build().engine.project, &picture).expect("saved");
    for _ in 0..2 {
        let mut shoot = |h: &mut EditorHarness, room: Option<Stage>| {
            let (name, size) = shots.next().expect("a shot owed");
            h.kit_mut().set_size(size);
            if let Some(room) = room {
                arrive(h, ProbeRoom::Stage(room));
            }
            h.shot(size, &name);
        };
        let mut h = EditorHarness::with_gpu_ready();
        shoot(&mut h, Some(Stage::Drawing));
        // Stays where it is: arriving again would wipe the refusal.
        h.perform(Action::RoomEdit);
        h.run_frames(4);
        shoot(&mut h, None);
        crate::ausay_tests::raster_line(&mut h);
        shoot(&mut h, Some(Stage::Drawing));
        h.perform(Action::RoomFinishing);
        shoot(&mut h, Some(Stage::InkPaint));
        h.perform(Action::RoomComposite);
        shoot(&mut h, Some(Stage::Composite));
        drop(h);
        let mut h = EditorHarness::with_gpu_ready();
        h.run_frames(4);
        h.perform(Action::AddDrawingAfter);
        h.run_frames(4);
        shoot(&mut h, None);
        drop(h);
        let mut h = EditorHarness::with_gpu_animation();
        shoot(&mut h, Some(Stage::Drawing));
        h.perform(Action::AddDrawingAfter);
        h.run_frames(4);
        let ctx = h.ctx();
        h.editor_mut().go(crate::Leave::OpenPath(picture.clone()), &ctx);
        shoot(&mut h, Some(Stage::Drawing));
        drop(h);
        let mut h = EditorHarness::with_gpu_shipped(crate::templates::ALL[crate::templates::COMIC].build());
        shoot(&mut h, Some(Stage::Manga));
        h.perform(Action::RoomFinishing);
        shoot(&mut h, Some(Stage::InkPaint));
    }
}

/// AU10: an upgrader's first launch: both chords that moved, in the one
/// sentence the lane holds until the artist acts.
const KEYS: [(&str, u32, u32); 2] = [("say-keys-moved", 1600, 900), ("say-keys-moved-1280", 1280, 800)];

fn keys_owed() -> Vec<(String, u32, u32)> {
    KEYS.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

fn keys_moments(dir: &Path) {
    for (state, w, height) in KEYS {
        let size = egui::vec2(w as f32, height as f32);
        let mut h = EditorHarness::with_gpu_dir_shipped(crate::aukeys_tests::upgraders_dir());
        h.kit_mut().set_size(size);
        arrive(&mut h, ProbeRoom::Stage(Stage::Drawing));
        h.shot(size, &dir.join(format!("{state}.png")));
        let said = h.painted_text().join(" ");
        assert!(said.contains(crate::aukeys_tests::KEYS_MOVED), "on the lane: {said}");
    }
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

/// AU9: the row stands under the Export panel's choices, on the frozen boot's
/// open fold: its last chip scrolled up brings them all.
fn queue_row_up(h: &mut EditorHarness) {
    h.scroll_to(anim_core::deliver::PRESETS[anim_core::deliver::PRESETS.len() - 1].name);
    h.run_frames(4);
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
        queue_row_up(&mut h);
        h.shot(size, &dir.join(format!("{state}.png")));
    }
}

/// AU9: the Export panel as a person meets it, on a shipped boot and a PC with
/// no ffmpeg: each document's choices, Show me's ring, a Done card, the fold
/// open, a job on its way, one that failed, the pane holding its foot in the
/// Effects step; and the lane's words from another step, during and after.
const EXPORT: [(&str, u32, u32); 28] = [
    ("export-animation", 1600, 900),
    ("export-animation-1280", 1280, 800),
    ("export-ring", 1600, 900),
    ("export-ring-1280", 1280, 800),
    ("export-one-frame", 1600, 900),
    ("export-one-frame-1280", 1280, 800),
    ("export-no-art", 1600, 900),
    ("export-no-art-1280", 1280, 800),
    ("export-done", 1600, 900),
    ("export-done-1280", 1280, 800),
    ("export-more", 1600, 900),
    ("export-more-1280", 1280, 800),
    ("say-exported", 1600, 900),
    ("say-exported-1280", 1280, 800),
    ("export-file-menu", 1600, 900),
    ("export-file-menu-1280", 1280, 800),
    ("export-running", 1600, 900),
    ("export-running-1280", 1280, 800),
    ("export-failed", 1600, 900),
    ("export-failed-1280", 1280, 800),
    ("export-comic", 1600, 900),
    ("export-comic-1280", 1280, 800),
    ("export-comic-done", 1600, 900),
    ("export-comic-done-1280", 1280, 800),
    ("export-effects", 1600, 900),
    ("export-effects-1280", 1280, 800),
    ("say-exporting", 1600, 900),
    ("say-exporting-1280", 1280, 800),
];

fn export_owed() -> Vec<(String, u32, u32)> {
    EXPORT.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

/// A ball that crosses `frames` drawings: `12 drawings, 1.0 s` at 12 fps, with
/// the composite every template makes, so the Result shows it.
fn bouncing(frames: u32) -> AppState {
    let mut s = AppState::new_project("ball", 1280, 720, 12, 72.0);
    s.auto_comp();
    s.set_cut_length(frames);
    for frame in 0..frames as i32 {
        s.goto(frame as u32);
        s.new_drawing_at_frame();
        let (x, y) = (240 + 70 * frame, 200 + (frame - 6).pow(2) * 8);
        let disc = (-48..=48).flat_map(|dx| (-48..=48).map(move |dy| (dx, dy)));
        let ball: Vec<(i32, i32)> = disc.filter(|(dx, dy)| dx * dx + dy * dy <= 48 * 48).map(|(dx, dy)| (x + dx, y + dy)).collect();
        crate::canvas::extpaper_tests::paint_texels(&mut s, &ball, [0.85, 0.2, 0.15, 1.0]);
    }
    s.goto(0);
    s
}

fn export_moments(dir: &Path) {
    use crate::render_queue::{self, ExportKind};
    let work = tempfile::tempdir().expect("a temp folder");
    let file = work.path().join("ball.gif");
    let wall = work.path().join("wall");
    std::fs::write(&wall, b"a file, not a folder").expect("a file");
    let room = ProbeRoom::Stage(Stage::Export);
    for (state, w, height) in EXPORT {
        let size = egui::vec2(w as f32, height as f32);
        let comic = state.contains("comic");
        let mut h = match state {
            _ if state.contains("no-art") || comic => EditorHarness::with_gpu_ready(),
            _ if state.contains("one-frame") => EditorHarness::with_gpu_shipped(bouncing(1)),
            _ => EditorHarness::with_gpu_shipped(bouncing(12)),
        };
        h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
        h.kit_mut().set_size(size);
        h.run_frames(4);
        if comic {
            h.answer_start(crate::welcome::WelcomeAction::Template(crate::templates::COMIC));
            h.run_frames(4);
            let ink: Vec<(i32, i32)> = (120..420).flat_map(|x| (120..140).map(move |y| (x, y))).collect();
            crate::canvas::extpaper_tests::paint_texels(h.state_mut(), &ink, [0.1, 0.1, 0.12, 1.0]);
        }
        let ctx = h.ctx();
        // These are about staying where the person stands.
        let stays = state.starts_with("say-") || state.contains("file-menu");
        if state.contains("ring") {
            h.editor_mut().show_me(crate::welcome::EXPORT_STEP);
        } else if state.contains("effects") {
            h.editor_mut().enter_room(Stage::Composite);
            h.run_frames(4);
            assert_eq!(h.perform(crate::config::Action::ShowRenderQueue), crate::Performed::Done);
        } else if !stays {
            room.enter(h.editor_mut());
        }
        if comic && state.contains("done") {
            h.state_mut().begin_page_export(work.path(), None);
            h.state_mut().wait_page_export();
        } else if state.contains("failed") {
            h.editor_mut().export_to(&ctx, ExportKind::Gif, &wall.join("ball.gif"));
            h.run_export_to_end();
        } else if state.contains("done") || state.contains("exported") {
            h.editor_mut().export_to(&ctx, ExportKind::Gif, &file);
            h.run_export_to_end();
        }
        // A job that stands still for its picture: the sender holds it open.
        let moving = state.contains("running") || state.contains("effects");
        let _held = moving.then(|| render_queue::held_job(h.state_mut(), &file, 5));
        arrive(&mut h, room);
        if state.contains("more") {
            h.click(crate::export_step::MORE_OPTIONS);
            h.run_frames(4);
        }
        // The machine's last remark has its dwell first, but for the one
        // picture of the remark itself. That starts here: a harness frame is a
        // quarter of a second, so `arrive` alone outlasts the four it stands.
        if state.contains("exporting") {
            h.editor_mut().export_to(&ctx, ExportKind::Gif, &file);
        } else {
            h.wait(4.2);
        }
        if state.contains("file-menu") {
            h.click("file");
            h.run_frames(4);
            h.click("Export… ⏵");
            h.run_frames(4);
            assert!(h.has(crate::config::Action::ExportGif.label()), "{state} shows its rows");
        }
        h.shot(size, &dir.join(format!("{state}.png")));
        if state.contains("exporting") {
            let said = h.painted_text().iter().any(|words| words == "Exporting ball.gif…");
            assert!(said, "{state} shows its remark: {:?}", h.painted_text());
            h.run_export_to_end();
        }
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
    export_moments(&dir);
    check_shots(&dir, &export_owed()).expect("the Export step's moments");
    hide_moments(&dir);
    check_shots(&dir, &hide_owed()).expect("Hide tools' moments");
    keys_moments(&dir);
    check_shots(&dir, &keys_owed()).expect("the chords that moved");
    kinds_moments(&dir);
    check_shots(&dir, &kinds_owed()).expect("the three bars");
}

/// Only the ignored pass takes them, so this holds it to them, at both sizes:
/// it takes every one, then checks the folder, and the check misses any one of
/// them, which is what `shots-after/` gets a copy of.
#[test]
fn aukeys_the_ignored_pass_owes_the_hide_tools_shots() {
    let pass = include_str!("uishot.rs").split("fn uishot_rooms()").nth(1).expect("the pass");
    for group in ["hide", "keys"] {
        let taken = pass.find(&format!("{group}_moments(&dir);")).expect("it takes them");
        let checked = pass.find(&format!("check_shots(&dir, &{group}_owed())")).expect("it checks them");
        assert!(taken < checked, "taken, then checked");
    }
    let owed: Vec<_> = hide_owed().into_iter().chain(keys_owed()).collect();
    assert_eq!(owed.len(), 10);
    for moment in ["hide-tools", "hide-refusal", "hide-dot", "hide-timing", "say-keys-moved"] {
        assert!(owed.contains(&(format!("{moment}.png"), 1600, 900)), "{moment} at 1600");
        assert!(owed.contains(&(format!("{moment}-1280.png"), 1280, 800)), "{moment} at 1280");
    }
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), owed.len(), "no two shots share a name: {names:?}");
    let elsewhere = say_owed().into_iter().chain(export_owed()).chain(queue_owed());
    assert!(elsewhere.into_iter().all(|o| !names.contains(o.0.as_str())), "no name is another group's");
    let dir = tempfile::tempdir().unwrap();
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
fn auexport_shots_owe_every_moment_at_both_sizes() {
    let owed = export_owed();
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), owed.len(), "no two shots share a name: {names:?}");
    let moments = [
        "export-animation",
        "export-ring",
        "export-one-frame",
        "export-no-art",
        "export-done",
        "export-more",
        "say-exported",
        "export-file-menu",
        "export-running",
        "export-failed",
        "export-comic",
        "export-comic-done",
        "export-effects",
        "say-exporting",
    ];
    for moment in moments {
        assert!(owed.contains(&(format!("{moment}.png"), 1600, 900)), "{moment} at 1600");
        assert!(owed.contains(&(format!("{moment}-1280.png"), 1280, 800)), "{moment} at 1280");
    }
    let elsewhere = say_owed().into_iter().chain(start_owed()).chain(strip_owed()).chain(rail_owed()).chain(queue_owed());
    assert!(elsewhere.into_iter().all(|o| !names.contains(o.0.as_str())), "no name is another group's");
}

/// Only the ignored pass writes the Export step's shots, so this holds it to
/// them: it takes every one, then checks the folder, and the check misses any
/// one of them, which is what `shots-after/` gets a copy of.
#[test]
fn auexport_the_ignored_pass_owes_the_export_shots() {
    let pass = include_str!("uishot.rs").split("fn uishot_rooms()").nth(1).expect("the pass");
    let taken = pass.find("export_moments(&dir);").expect("it takes them");
    let checked = pass.find("check_shots(&dir, &export_owed())").expect("it checks them");
    assert!(taken < checked, "taken, then checked");
    let dir = tempfile::tempdir().unwrap();
    let owed = export_owed();
    assert_eq!(owed.len(), 28);
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
        queue_row_up(&mut h);
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
    assert_eq!(shots_owed().len() + states_owed().len() + shipped.len(), 60);
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
    // Sixteen and the Export step's two, at the same two sizes.
    assert_eq!(owed.len(), 18);
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), 18, "no two shots share a name: {names:?}");
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
