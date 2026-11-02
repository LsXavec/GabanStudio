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

/// AU19: Plan's picture keeps number 0 and the file name `shots-before` gave
/// it: `plan_moments` takes it, on an animation, for no probe stands in Plan.
fn plan_shot(suffix: &str) -> String {
    format!("room-0-Layout{suffix}.png")
}

fn shot_name(i: usize, room: ProbeRoom, suffix: &str) -> String {
    format!("room-{}-{}{suffix}.png", i + 1, room_tag(room))
}

/// Every picture a run owes: its file name and its size in pixels.
fn shots_owed() -> Vec<(String, u32, u32)> {
    let rooms = PROBE_ROOMS.into_iter().enumerate();
    let owed = SIZES.iter().flat_map(|(w, h, suffix)| {
        let room_rows = rooms.clone().map(move |(i, room)| (shot_name(i, room, suffix), *w, *h));
        std::iter::once((plan_shot(suffix), *w, *h)).chain(room_rows)
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
    arrive_within(h, room, 200);
}

/// The looks a lesson's shot gives its picture: a comic's composed page takes
/// twenty seconds.
const PAGE_TRIES: usize = 1200;

/// `arrive`, over `tries` looks.
fn arrive_within(h: &mut EditorHarness, room: ProbeRoom, tries: usize) {
    h.run_frames(30);
    for _ in 0..tries {
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
    // The last room's float is that room's alone.
    h.editor_mut().start = None;
}

/// The menus and the submenus a shot opens, by the path that opens them.
const MENUS: [&str; 16] = [
    "File",
    "File/Shots",
    "File/Import",
    "File/Export",
    "Edit",
    "Edit/Tools",
    "View",
    "View/3D view",
    "Animation",
    "Animation/Track",
    "Animation/Curves",
    "Help",
    "More",
    "More/Steps",
    "More/Panels",
    "More/Saved arrangements",
];

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
    let menus = MENUS.map(|menu| format!("menu-{}", menu.replace(['/', ' '], "-")));
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
        h.open_path(menu);
        h.run_frames(4);
        assert!(egui::Popup::is_any_open(&h.ctx()), "the {menu} shot shows {menu} open");
        h.shot(size, &dir.join(state_name(&format!("menu-{}", menu.replace(['/', ' '], "-")), suffix)));
        // Not a second click: a long menu can stand over its own label.
        h.close_menus();
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
    // AU19: an animation opens on its sheet: these start with it hidden.
    h.click(crate::vocab::HIDE_TIMING_SHEET);
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
    h.click(crate::vocab::HIDE_TIMING_SHEET);
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
    h.click(crate::vocab::HIDE_TIMING_SHEET);
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
const START: [(&str, u32, u32); 16] = [
    ("start-fresh", 1600, 900),
    ("start-fresh-1280", 1280, 800),
    ("start-follow", 1600, 900),
    ("start-follow-1280", 1280, 800),
    ("start-lessons", 1600, 900),
    ("start-lessons-1280", 1280, 800),
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
        if !["fresh", "follow", "lessons"].iter().any(|tag| state.contains(tag)) {
            w = crate::welcome::Welcome::new(&recent);
        }
        w.more_open = state.contains("more-sizes");
        if state.contains("whats-new") {
            w.tab = crate::welcome::WelcomeTab::WhatsNew;
        }
        // AU20, AU23: on the shipped table.
        if state.contains("follow") || state.contains("lessons") {
            w.lessons = crate::lessons::ALL;
        }
        if state.contains("lessons") {
            w.tab = crate::welcome::WelcomeTab::Lessons;
        }
        let pens = [("pen-fix", PenCheck::NoPressure), ("pen-mouse", PenCheck::Mouse), ("pen-works", PenCheck::Works)];
        if let Some((_, said)) = pens.iter().find(|(tag, _)| state.contains(tag)) {
            w.pen = *said;
            w.pen_said = true;
        }
        h.editor_mut().open_start(w);
        arrive(&mut h, draw);
        h.shot(size, &dir.join(format!("{state}.png")));
    }
}

/// AU27: the card as a person meets it, from a real run of the probe in the
/// lessons' own words: each step of its bar at the full and the worst size, the
/// way back when they wander, a real finish and the page's tick, then Plan's
/// sheet and the chip.
fn lesson_card(dir: &Path) {
    use crate::aulesson_tests::{along, export_asked, open, pen};
    let full = (1600, 900);
    let fold = (832, 520);
    let at = |h: &mut EditorHarness, name: &str, (w, tall): (u32, u32)| {
        let size = egui::vec2(w as f32, tall as f32);
        h.kit_mut().set_size(size);
        arrive(h, ProbeRoom::Stage(Stage::Drawing));
        h.shot(size, &dir.join(format!("{name}.png")));
    };
    let both = |h: &mut EditorHarness, name: &str| {
        at(h, name, full);
        at(h, &format!("{name}-fold"), fold);
        h.kit_mut().set_size(egui::vec2(full.0 as f32, full.1 as f32));
        h.run_frames(4);
    };

    // A first run, from the page's card to the file, in the lessons' own words.
    let mut h = EditorHarness::with_gpu_ready();
    open(&mut h, "probe-spoken");
    both(&mut h, "lesson-draw");
    // The way back, on the card's top line, and the pill still free to press.
    h.editor_mut().enter_room(Stage::Composite);
    at(&mut h, "lesson-wander", fold);
    h.editor_mut().enter_room(Stage::Drawing);
    h.kit_mut().set_size(egui::vec2(full.0 as f32, full.1 as f32));
    h.run_frames(4);
    pen(&mut h, &along([210.0, 205.0], [990.0, 205.0]));
    for (pill, name) in [
        (crate::vocab::STEP_COLOR, "lesson-color"),
        (crate::vocab::STEP_EFFECTS, "lesson-effects"),
        (crate::vocab::STEP_EXPORT, "lesson-export"),
    ] {
        h.click(pill);
        h.run_frames(4);
        both(&mut h, name);
    }
    export_asked(&mut h);
    let out = tempfile::tempdir().expect("a folder for the file");
    h.answer_export(&out.path().join("up-late.png"));
    h.run_export_to_end();
    h.run_frames(4);
    assert!(h.editor().lesson.as_ref().is_some_and(|run| run.finished()), "the run's real finish");
    both(&mut h, "lesson-finish");

    // The page, with that finish ticked: its row takes the probe's cell.
    let mut rows = crate::lessons::table().to_vec();
    rows.retain(|l| l.id != "probe");
    let mut w = crate::welcome::Welcome::new(&[]);
    w.lessons = Box::leak(rows.into_boxed_slice());
    h.editor_mut().open_start(w.on_lessons());
    at(&mut h, "lesson-page", full);
    drop(h); // the device is one at a time: the next harness waits on this one

    // Plan: the sheet a lesson stands on, whatever the memory says.
    let mut h = EditorHarness::with_gpu_ready();
    open(&mut h, "probe-film");
    both(&mut h, "lesson-plan");
    drop(h);

    // The chip, on the task that rings it.
    let mut h = EditorHarness::with_gpu_ready();
    open(&mut h, "probe-night");
    at(&mut h, "lesson-chip", (1280, 800));
    drop(h);

    // AU28: the Amateur's third foot row, held: the finish, blue, on a paper
    // whose own task draws no line, so what shows is what the row shows.
    let mut h = EditorHarness::with_gpu_ready();
    open(&mut h, "probe-square");
    let square = crate::lessons::table().iter().find(|l| l.id == "probe-square").expect("the row");
    h.editor_mut().begin_lesson(*square, 1);
    for (name, size) in [("lesson-hold", full), ("lesson-hold-small", (1280, 800)), ("lesson-hold-pocket", fold)] {
        crate::aurail_tests::sized(&mut h, size.0 as f32, size.1 as f32);
        let spot = h.rects(crate::lesson::HOLD)[0].center();
        let button = |pressed| egui::Event::PointerButton {
            pos: spot,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        h.kit_mut().input_mut().events.extend([egui::Event::PointerMoved(spot), button(true)]);
        h.run_frames(3);
        assert!(!h.editor().canvas.guide.lines.is_empty(), "the shot stands inside the hold");
        // A shot's own frames would let go of the row.
        h.shot_as_it_stands(&dir.join(format!("{name}.png")));
        h.kit_mut().input_mut().events.push(button(false));
        h.run_frames(3);
    }
    drop(h);

    // AU28: Show me on its way: the ink partway down the first line.
    use egui_kittest::kittest::Queryable;
    let mut h = EditorHarness::with_gpu_ready();
    open(&mut h, "probe");
    h.editor_mut().begin_lesson(crate::auguide_show_tests::traced_lesson(), 0);
    crate::aurail_tests::sized(&mut h, full.0 as f32, full.1 as f32);
    h.kit_mut().get_by_label(crate::lesson::SHOW_ME).click();
    h.run_frames(8);
    assert!(!h.editor().canvas.guide.reveal.is_empty(), "the shot stands inside the reveal");
    h.shot_as_it_stands(&dir.join("lesson-reveal.png"));
    drop(h);

    // AU28: the tap a machine with no engine refuses, and the card's answer.
    let mut h = EditorHarness::with_gpu_ready();
    open(&mut h, "probe");
    h.editor_mut().begin_lesson(crate::auguide_show_tests::fill_lesson(), 0);
    h.editor_mut().paint = None;
    h.editor_mut().canvas.raster = false;
    crate::aurail_tests::sized(&mut h, full.0 as f32, full.1 as f32);
    crate::auguide_show_tests::pick_fill(&mut h);
    let spot = h.editor().canvas.view_xform().paper_to_screen(egui::pos2(600.0, 450.0));
    h.click_pos(spot);
    h.run_frames(4);
    assert!(h.has_text_containing(crate::lesson::BY_HAND), "the card's line");
    h.shot(egui::vec2(full.0 as f32, full.1 as f32), &dir.join("lesson-by-hand.png"));
    drop(h);

    // AU28: Deckle as a person traces him: two lines and three dots.
    let mut h = EditorHarness::with_gpu_ready();
    open(&mut h, "probe");
    h.editor_mut().begin_lesson(crate::auguide_show_tests::deckle_lesson(), 0);
    crate::aurail_tests::sized(&mut h, full.0 as f32, full.1 as f32);
    h.shot(egui::vec2(full.0 as f32, full.1 as f32), &dir.join("lesson-deckle-guide.png"));
    drop(h);

    // AU23, AU24: the Rookie and Amateur lessons, part way, on each window; then
    // the file each one ends in, as it left the dialog.
    use crate::auguide_show_tests::{File, Hand, ride, ride_tasks};
    use crate::lessons::Class;
    // An Amateur's art is the person's and `Show me` draws none of it: the ride
    // comes to the task as they do, so the shot shows what they would see.
    let seated = |lesson: &crate::lessons::Lesson, task: usize, size: (u32, u32), name: String| {
        let mut h = EditorHarness::with_gpu_ready();
        open(&mut h, lesson.id);
        crate::aurail_tests::sized(&mut h, size.0 as f32, size.1 as f32);
        if lesson.class != Class::Rookie {
            ride_tasks(&mut h, lesson, 0..task, Hand::ShowMe);
        } else {
            crate::auguide_tests::stand_on_the_tasks_before(&mut h, lesson, task);
            h.editor_mut().begin_lesson(*lesson, task);
        }
        // A room with a Result waits for its picture: a shot taken mid-render is
        // one of `Rendering effects…`.
        arrive_within(&mut h, ProbeRoom::Stage(lesson.tasks[task].step), PAGE_TRIES);
        h.shot(egui::vec2(size.0 as f32, size.1 as f32), &dir.join(format!("{name}.png")));
    };
    // AU25: Experienced, the card that asks nothing of the paper; then its ring,
    // asked for, in the Brush maker a sentence sends the person to.
    for (name, size) in [("", full), ("-1280", (1280, 800))] {
        let plate = crate::lessons::ALL.iter().find(|l| l.id == "plate_one").expect("the row");
        let mut h = EditorHarness::with_gpu_ready();
        open(&mut h, plate.id);
        crate::aurail_tests::sized(&mut h, size.0 as f32, size.1 as f32);
        ride_tasks(&mut h, plate, 0..5, Hand::ShowMe);
        h.editor_mut().perform(crate::config::Action::BrushMakerRoom);
        h.run_frames(4);
        h.click(crate::lesson::SHOW_WHERE);
        h.run_frames(8);
        h.shot(egui::vec2(size.0 as f32, size.1 as f32), &dir.join(format!("experienced-plate_one-asked{name}.png")));
        drop(h);
        // And the card the class ends on.
        let mut h = EditorHarness::with_gpu_ready();
        open(&mut h, plate.id);
        crate::aurail_tests::sized(&mut h, size.0 as f32, size.1 as f32);
        let step = h.editor().stage.expect("the lesson's step");
        let last = crate::lessons::Task { step, done: crate::lessons::Done::OnStep(step), ..crate::lessons::PROBED.tasks[0] };
        let tasks: &'static [crate::lessons::Task] = Box::leak(Box::new([last]));
        h.editor_mut().lesson = None;
        h.editor_mut().begin_lesson(crate::lessons::Lesson { tasks, ..*plate }, 0);
        h.run_frames(8);
        h.shot(egui::vec2(size.0 as f32, size.1 as f32), &dir.join(format!("experienced-plate_one-finish{name}.png")));
        drop(h);
    }
    for (lesson, task) in [(0, 8), (1, 6), (2, 7), (3, 4), (4, 7), (5, 7), (6, 8), (7, 11), (8, 7)] {
        let lesson = crate::lessons::ALL[lesson];
        let class = match lesson.class {
            Class::Rookie => "rookie",
            Class::Amateur => "amateur",
            Class::Experienced => "experienced",
        };
        for (name, size) in [("", full), ("-1280", (1280, 800))] {
            seated(&lesson, task, size, format!("{class}-{}{name}", lesson.id));
        }
        // The steps past the paper: Color and Export of each, and the two rooms
        // the card narrows at a laptop's size.
        let rooms = [
            ("color", Stage::InkPaint, full),
            ("export", Stage::Export, full),
            ("timing-1280", Stage::Edit, (1280, 800)),
            ("lettering-1280", Stage::Manga, (1280, 800)),
        ];
        for (name, stage, size) in rooms {
            // AU25: Export's preview of a full Manga B5 outruns the test
            // renderer's fixed wait; the page itself is read off its file.
            if lesson.id == "only_word" && stage == Stage::Export {
                continue;
            }
            let Some(task) = lesson.tasks.iter().rposition(|t| t.step == stage) else { continue };
            seated(&lesson, task, size, format!("{class}-{}-{name}", lesson.id));
        }
        // Plan's sheet and paper together, two poses in.
        if lesson.id == "still_in_the_coat" {
            for (name, size) in [("", full), ("-1280", (1280, 800))] {
                seated(&lesson, 2, size, format!("amateur-still_in_the_coat-plan{name}"));
            }
        }
        let mut h = EditorHarness::as_shipped();
        crate::aurail_tests::sized(&mut h, full.0 as f32, full.1 as f32);
        open(&mut h, lesson.id);
        let ridden = ride(&mut h, &lesson, Hand::ShowMe);
        let (from, kind) = match ridden.files.last().expect("a file") {
            File::One(_, path) => (path.clone(), path.extension().map(|e| e.to_string_lossy().into_owned())),
            File::Pages(pages) => {
                let page = std::fs::read_dir(pages).expect("the pages").flatten().next().expect("a page");
                (page.path(), Some("png".to_string()))
            }
        };
        let to = dir.join(format!("{class}-file-{}.{}", lesson.id, kind.expect("an extension")));
        std::fs::copy(&from, to).unwrap_or_else(|e| panic!("{from:?}: {e}"));
    }
}

/// AU27: the card's pictures, by file name.
const LESSON: [(&str, u32, u32); 68] = [
    ("lesson-draw", 1600, 900),
    ("lesson-draw-fold", 832, 520),
    ("lesson-wander", 832, 520),
    ("lesson-color", 1600, 900),
    ("lesson-color-fold", 832, 520),
    ("lesson-effects", 1600, 900),
    ("lesson-effects-fold", 832, 520),
    ("lesson-export", 1600, 900),
    ("lesson-export-fold", 832, 520),
    ("lesson-finish", 1600, 900),
    ("lesson-finish-fold", 832, 520),
    ("lesson-page", 1600, 900),
    ("lesson-plan", 1600, 900),
    ("lesson-plan-fold", 832, 520),
    ("lesson-chip", 1280, 800),
    ("lesson-hold", 1600, 900),
    ("lesson-hold-small", 1280, 800),
    ("lesson-hold-pocket", 832, 520),
    ("lesson-reveal", 1600, 900),
    ("lesson-by-hand", 1600, 900),
    ("lesson-deckle-guide", 1600, 900),
    ("rookie-up_late", 1600, 900),
    ("rookie-up_late-1280", 1280, 800),
    ("rookie-end_of_the_desk", 1600, 900),
    ("rookie-end_of_the_desk-1280", 1280, 800),
    ("rookie-not_a_pencil", 1600, 900),
    ("rookie-not_a_pencil-1280", 1280, 800),
    ("rookie-up_late-color", 1600, 900),
    ("rookie-up_late-export", 1600, 900),
    ("rookie-end_of_the_desk-color", 1600, 900),
    ("rookie-end_of_the_desk-export", 1600, 900),
    ("rookie-end_of_the_desk-timing-1280", 1280, 800),
    ("rookie-not_a_pencil-color", 1600, 900),
    ("rookie-not_a_pencil-export", 1600, 900),
    ("rookie-not_a_pencil-lettering-1280", 1280, 800),
    ("amateur-coat_of_shavings", 1600, 900),
    ("amateur-coat_of_shavings-1280", 1280, 800),
    ("amateur-still_in_the_coat", 1600, 900),
    ("amateur-still_in_the_coat-1280", 1280, 800),
    ("amateur-still_in_the_coat-plan", 1600, 900),
    ("amateur-still_in_the_coat-plan-1280", 1280, 800),
    ("amateur-too_much_coat", 1600, 900),
    ("amateur-too_much_coat-1280", 1280, 800),
    ("amateur-coat_of_shavings-color", 1600, 900),
    ("amateur-coat_of_shavings-export", 1600, 900),
    ("amateur-still_in_the_coat-color", 1600, 900),
    ("amateur-still_in_the_coat-export", 1600, 900),
    ("amateur-still_in_the_coat-timing-1280", 1280, 800),
    ("amateur-too_much_coat-color", 1600, 900),
    ("amateur-too_much_coat-export", 1600, 900),
    ("amateur-too_much_coat-lettering-1280", 1280, 800),
    ("experienced-plate_one-asked", 1600, 900),
    ("experienced-plate_one-asked-1280", 1280, 800),
    ("experienced-plate_one-finish", 1600, 900),
    ("experienced-plate_one-finish-1280", 1280, 800),
    ("experienced-plate_one", 1600, 900),
    ("experienced-plate_one-1280", 1280, 800),
    ("experienced-desk_to_lamp", 1600, 900),
    ("experienced-desk_to_lamp-1280", 1280, 800),
    ("experienced-only_word", 1600, 900),
    ("experienced-only_word-1280", 1280, 800),
    ("experienced-plate_one-color", 1600, 900),
    ("experienced-plate_one-export", 1600, 900),
    ("experienced-desk_to_lamp-color", 1600, 900),
    ("experienced-desk_to_lamp-export", 1600, 900),
    ("experienced-desk_to_lamp-timing-1280", 1280, 800),
    ("experienced-only_word-color", 1600, 900),
    ("experienced-only_word-lettering-1280", 1280, 800),
];

fn lesson_owed() -> Vec<(String, u32, u32)> {
    LESSON.iter().map(|(name, w, h)| (format!("{name}.png"), *w, *h)).collect()
}

/// AU21: the ring on a rail key, on a menu's word, on the row that opens.
const GUIDE: [&str; 3] = ["guide-rail", "guide-menu", "guide-row"];

/// AU21: the ring where its control waits: on `More` at the Pocket tier, on the
/// band's `…`, on a cell the sheet came to, on the strip's next sheet.
const GUIDE_AT: [(&str, u32, u32); 4] =
    [("guide-pocket", 853, 480), ("guide-fold", 1024, 640), ("guide-cell", 832, 520), ("guide-strip", 1600, 900)];

fn guide_owed() -> Vec<(String, u32, u32)> {
    let sized = |&(w, h, suffix): &(u32, u32, &str)| GUIDE.map(|state| (format!("{state}{suffix}.png"), w, h));
    let placed = GUIDE_AT.iter().map(|(name, w, h)| (format!("{name}.png"), *w, *h));
    SIZES.iter().flat_map(sized).chain(placed).collect()
}

/// AU21: one harness each, gone before the next: a GPU harness holds the device.
fn guide_placed(dir: &Path) {
    use crate::auguide_tests::{stand, strip_up_for_color};
    use crate::aurail_tests::sized;
    use crate::lessons::Target;
    let held = Target::Rail(crate::rail::Key::More);
    const POCKET: &[Target] = &[Target::Rail(crate::rail::Key::More), Target::Rail(crate::rail::Key::Size)];
    const FOLD: &[Target] = &[Target::Named("band.fold"), Target::Named("band.speed"), Target::Named("speed.medium")];
    const CELL: &[Target] = &[Target::Cell { track: 0, frame: 16 }];
    let shot = |mut h: EditorHarness, at: usize, chain: &'static [Target], step: Stage, rings: Target| {
        let (name, w, tall) = GUIDE_AT[at];
        sized(&mut h, w as f32, tall as f32);
        stand(&mut h, chain, step);
        // AU16: the Result pane's picture back: half drawn, two runs differ.
        arrive(&mut h, ProbeRoom::Stage(step));
        assert_eq!(h.ring_stands().map(|s| s.0), Some(rings), "{name}");
        h.shot(egui::vec2(w as f32, tall as f32), &dir.join(format!("{name}.png")));
    };
    shot(EditorHarness::with_gpu_ready(), 0, POCKET, Stage::Drawing, held);
    shot(EditorHarness::with_gpu_animation(), 1, FOLD, Stage::Drawing, FOLD[0]);
    let sample = crate::sample::build_sample();
    shot(EditorHarness::with_gpu_shipped(sample), 2, CELL, Stage::Edit, CELL[0]);
    let (name, w, tall) = GUIDE_AT[3];
    let mut h = EditorHarness::with_gpu_shipped(crate::auguide_tests::film().build());
    sized(&mut h, w as f32, tall as f32);
    strip_up_for_color(&mut h);
    assert!(h.ring_rect().is_some(), "{name}");
    h.shot(egui::vec2(w as f32, tall as f32), &dir.join(format!("{name}.png")));
}

fn guide_moments(dir: &Path) {
    use crate::auguide_tests::stand;
    use crate::lessons::Target;
    const RAIL: &[Target] = &[Target::Rail(crate::rail::Key::Size)];
    const MENU: &[Target] = &[Target::Named("menu.view"), Target::Action(Action::MirrorView)];
    for (w, tall, suffix) in SIZES {
        let size = egui::vec2(w as f32, tall as f32);
        let mut h = EditorHarness::with_gpu_ready();
        h.kit_mut().set_size(size);
        stand(&mut h, RAIL, Stage::Drawing);
        h.shot(size, &dir.join(format!("guide-rail{suffix}.png")));
        stand(&mut h, MENU, Stage::Drawing);
        h.shot(size, &dir.join(format!("guide-menu{suffix}.png")));
        let rect = h.ring_rect().expect("the ring stands on the word");
        h.click_at(rect);
        h.run_frames(4);
        h.shot(size, &dir.join(format!("guide-row{suffix}.png")));
    }
    guide_placed(dir);
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
    // AU19: an animation opens with the sheet on: the paper's own row is what
    // it says with the sheet hidden.
    h.run_frames(4);
    h.click(crate::vocab::HIDE_TIMING_SHEET);
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
    // AU19: the sheet back, for Plan's own row.
    h.editor_mut().show_sheet();
    for _ in 0..2 {
        shoot(&mut h, draw);
    }
    for stage in [Stage::Edit, Stage::InkPaint, Stage::Composite, Stage::Composite] {
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
    h.click(crate::vocab::HIDE_TIMING_SHEET);
    draw.enter(h.editor_mut());
    for _ in 0..2 {
        shoot(&mut h, draw);
    }
    h.editor_mut().show_me(crate::welcome::EXPORT_STEP);
    shoot(&mut h, ProbeRoom::Stage(Stage::Export));
}

/// AU19: Plan past its two room pictures: the sheet hidden, a new animation
/// with the row's lesson, also at the tightest room (1024×640 at 150% lays out
/// 832×520), the paper shapes the rail meets differently, and the Layers float
/// over the sheet, Plan's one road to the layers.
const PLAN: [(&str, u32, u32); 10] = [
    ("plan-hidden", 1600, 900),
    ("plan-hidden-1280", 1280, 800),
    ("plan-new", 1600, 900),
    ("plan-new-1280", 1280, 800),
    ("plan-new-832", 832, 520),
    ("plan-4x3", 1600, 900),
    ("plan-1x1", 1600, 900),
    ("plan-9x16", 1280, 800),
    ("plan-layers", 1600, 900),
    ("plan-layers-832", 832, 520),
];

fn plan_owed() -> Vec<(String, u32, u32)> {
    PLAN.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

fn plan_moments(dir: &Path) {
    let draw = ProbeRoom::Stage(Stage::Drawing);
    let mut h = EditorHarness::with_gpu_shipped(crate::sample::build_sample());
    draw.enter(h.editor_mut());
    for (w, height, suffix) in SIZES {
        let size = egui::vec2(w as f32, height as f32);
        h.kit_mut().set_size(size);
        arrive(&mut h, draw);
        h.shot(size, &dir.join(plan_shot(suffix)));
    }
    let mut shots = PLAN.iter().map(|(state, w, h)| (dir.join(format!("{state}.png")), egui::vec2(*w as f32, *h as f32)));
    let mut shoot = |h: &mut EditorHarness| {
        let (name, size) = shots.next().expect("a shot owed");
        h.kit_mut().set_size(size);
        arrive(h, draw);
        h.shot(size, &name);
    };
    h.click(crate::vocab::HIDE_TIMING_SHEET);
    shoot(&mut h);
    shoot(&mut h);
    drop(h);
    let mut h = EditorHarness::with_gpu_animation();
    shoot(&mut h);
    shoot(&mut h);
    shoot(&mut h);
    drop(h);
    for (width, height) in [(1440, 1080), (1080, 1080), (1080, 1920)] {
        let mut s = NewProjectForm { width, height, ..Default::default() }.new_state();
        crate::steps::ProjectKind::Animation.stamp(&mut s.engine.project);
        let mut h = EditorHarness::with_gpu_shipped(s);
        draw.enter(h.editor_mut());
        shoot(&mut h);
    }
    let mut h = EditorHarness::with_gpu_shipped(crate::sample::build_sample());
    draw.enter(h.editor_mut());
    h.open_float(Float::Layers);
    shoot(&mut h);
    shoot(&mut h);
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

/// AU22: the Comic step: a lettered page, its rows and its handle, and the same
/// page as it will print.
const PAGE: [(&str, u32, u32); 4] = [
    ("page-lettered", 1600, 900),
    ("page-see", 1600, 900),
    ("page-lettered-1280", 1280, 800),
    ("page-see-1280", 1280, 800),
];

fn page_owed() -> Vec<(String, u32, u32)> {
    PAGE.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

fn page_moments(dir: &Path) {
    use crate::aupage_tests::{press, type_words};
    let room = ProbeRoom::Stage(Stage::Manga);
    let lettered = |size: egui::Vec2| {
        let mut h = EditorHarness::with_gpu_ready();
        h.kit_mut().set_size(size);
        h.run_frames(4);
        h.answer_start(crate::welcome::WelcomeAction::Template(crate::templates::COMIC));
        h.run_frames(4);
        let ink: Vec<(i32, i32)> = (420..1200).flat_map(|x| (900..940).map(move |y| (x, y))).collect();
        crate::canvas::extpaper_tests::paint_texels(h.state_mut(), &ink, [0.1, 0.1, 0.12, 1.0]);
        press(&mut h, crate::vocab::SPEED_LINES);
        press(&mut h, crate::vocab::ADD_BALLOON);
        type_words(&mut h, "Is this the only word?");
        h
    };
    for pair in PAGE.chunks(2) {
        let size = egui::vec2(pair[0].1 as f32, pair[0].2 as f32);
        let mut h = lettered(size);
        arrive(&mut h, room);
        h.shot(size, &dir.join(format!("{}.png", pair[0].0)));
        press(&mut h, crate::vocab::SEE_THE_PAGE);
        // `Ready` is the picture asked for, not the queue done: a shot taken
        // sooner runs out kittest's ten-second wait on this size of page.
        for _ in 0..800 {
            std::thread::sleep(Duration::from_millis(25));
            h.run_frames(1);
        }
        arrive(&mut h, room);
        h.shot(size, &dir.join(format!("{}.png", pair[1].0)));
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

/// AU17: the same upgrader's very first launch: What's New open over the
/// paper, with the moved keys on the lane's calm row, not a red one.
const UPGRADE: [(&str, u32, u32); 2] = [("upgrade-first-launch", 1600, 900), ("upgrade-first-launch-1280", 1280, 800)];

fn upgrade_owed() -> Vec<(String, u32, u32)> {
    UPGRADE.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

fn upgrade_moments(dir: &Path) {
    for (state, w, height) in UPGRADE {
        let size = egui::vec2(w as f32, height as f32);
        let mut h = EditorHarness::with_gpu_dir_first_run(crate::aukeys_tests::upgraders_dir());
        h.kit_mut().set_size(size);
        h.run();
        h.run_frames(4);
        h.shot(size, &dir.join(format!("{state}.png")));
        let said = h.painted_text().join(" ");
        assert!(said.contains(crate::welcome::WHATS_NEW_BARS), "the page: {said}");
        assert!(said.contains(crate::aukeys_tests::KEYS_MOVED), "on the lane: {said}");
        assert!(h.state().refusal.is_empty(), "no refusal: {}", h.state().refusal);
    }
}

/// AU26: a new animation's twelve rows, three drawings and the blank tail they
/// leave, and Timing's two hold keys over them.
const TAIL: [(&str, u32, u32); 6] = [
    ("tail-twelve-rows", 1600, 900),
    ("tail-three-drawings", 1600, 900),
    ("tail-hold-keys", 1600, 900),
    ("tail-twelve-rows-1280", 1280, 800),
    ("tail-three-drawings-1280", 1280, 800),
    ("tail-hold-keys-1280", 1280, 800),
];

fn tail_owed() -> Vec<(String, u32, u32)> {
    TAIL.iter().map(|(state, w, h)| (format!("{state}.png"), *w, *h)).collect()
}

fn tail_moments(dir: &Path) {
    let mut shots = TAIL.iter().map(|(state, w, h)| (dir.join(format!("{state}.png")), egui::vec2(*w as f32, *h as f32)));
    for _ in 0..2 {
        let mut shoot = |h: &mut EditorHarness, room: Stage| {
            let (name, size) = shots.next().expect("a shot owed");
            h.kit_mut().set_size(size);
            arrive(h, ProbeRoom::Stage(room));
            h.shot(size, &name);
        };
        let mut h = EditorHarness::with_gpu_animation();
        shoot(&mut h, Stage::Drawing);
        crate::ausay_tests::raster_line(&mut h);
        for _ in 0..2 {
            h.perform(Action::AddDrawingAfter);
            h.run_frames(4);
            crate::ausay_tests::raster_line(&mut h);
        }
        shoot(&mut h, Stage::Drawing);
        h.perform(Action::RoomEdit);
        shoot(&mut h, Stage::Edit);
    }
}

/// AU13: the Color step: the paper first with the strip's pictures, the Add
/// color popover, Colors opened up, the old split with a held frame's row, and
/// a comic's Color.
const COLOR: [&str; 5] = ["color-room", "color-adding", "color-details", "color-beside", "color-comic"];
const COLOR_SIZES: [(&str, u32, u32); 2] = [("", 1600, 900), ("-1280", 1280, 800)];

fn color_owed() -> Vec<(String, u32, u32)> {
    let sized = |state: &&str| COLOR_SIZES.map(|(suffix, w, h)| (format!("{state}{suffix}.png"), w, h));
    COLOR.iter().flat_map(sized).collect()
}

fn color_moments(dir: &Path) {
    for (suffix, w, tall) in COLOR_SIZES {
        let size = egui::vec2(w as f32, tall as f32);
        let shoot = |h: &mut EditorHarness, state: &str| {
            h.run_frames(6);
            h.shot(size, &dir.join(format!("{state}{suffix}.png")));
        };
        let mut h = EditorHarness::with_gpu_animation();
        h.kit_mut().set_size(size);
        arrive(&mut h, ProbeRoom::Stage(Stage::Drawing));
        h.hide_sheet();
        let c = h.editor().canvas.dbg_rect.center();
        // Past its start, so the ring closes and a tap fills its inside.
        let ring = |r: f32| -> Vec<egui::Pos2> {
            let at = |n: u32| n as f32 / 32.0 * std::f32::consts::TAU;
            (0..=35).map(|n| c + r * egui::vec2(at(n).cos(), at(n).sin())).collect()
        };
        h.drag_path(&ring(60.0));
        h.run_frames(8);
        for r in [100.0, 150.0] {
            h.perform(Action::AddDrawingAfter);
            h.run_frames(4);
            h.drag_path(&ring(r));
            h.run_frames(8);
        }
        h.editor_mut().strip_open = true;
        h.editor_mut().enter_room(Stage::InkPaint);
        arrive(&mut h, ProbeRoom::Stage(Stage::InkPaint));
        h.click(&format!("{}: Red", crate::vocab::COLOR_WELL));
        h.click_pos(h.editor().canvas.dbg_rect.center());
        h.run_frames(8);
        shoot(&mut h, "color-room");
        h.click(crate::vocab::ADD_COLOR);
        shoot(&mut h, "color-adding");
        h.key(egui::Key::Escape);
        h.click(crate::vocab::COLOR_DETAILS);
        h.click(crate::vocab::EDIT_COLORS);
        shoot(&mut h, "color-details");
        h.click(crate::vocab::EDIT_COLORS);
        h.click(crate::vocab::RESULT_BESIDE);
        h.run_frames(4);
        h.state_mut().goto(0);
        h.perform(Action::HoldPlus);
        h.perform(Action::HoldPlus);
        h.state_mut().goto(1);
        // The verb's status gives way, and the Say line reads the hold.
        h.state_mut().status.clear();
        shoot(&mut h, "color-beside");
        drop(h);
        let comic = crate::templates::ALL[crate::templates::COMIC].build();
        let mut h = EditorHarness::with_gpu_shipped(comic);
        h.kit_mut().set_size(size);
        h.editor_mut().enter_room(Stage::InkPaint);
        arrive(&mut h, ProbeRoom::Stage(Stage::InkPaint));
        shoot(&mut h, "color-comic");
    }
}

/// AU15: the panels no walk stops on, docked beside the paper, folds shut.
const PANELS: [(&str, crate::workspace::Pane); 7] = [
    ("panel-shots", crate::workspace::Pane::Board),
    ("panel-check-paint", crate::workspace::Pane::PaintCheck),
    ("panel-episode", crate::workspace::Pane::Episode),
    ("panel-history", crate::workspace::Pane::History),
    ("panel-curves", crate::workspace::Pane::Curves),
    ("panel-forge", crate::workspace::Pane::Forge),
    ("panel-manga", crate::workspace::Pane::Manga),
];

pub(crate) fn panels_owed() -> Vec<(String, u32, u32)> {
    let sized = |(name, _): &(&str, crate::workspace::Pane)| {
        COLOR_SIZES.map(|(suffix, w, h)| (format!("{name}{suffix}.png"), w, h))
    };
    PANELS.iter().flat_map(sized).collect()
}

fn panel_moments(dir: &Path) {
    for (suffix, w, tall) in COLOR_SIZES {
        let size = egui::vec2(w as f32, tall as f32);
        for (name, pane) in PANELS {
            let mut h = EditorHarness::with_gpu_shipped(crate::sample::build_sample());
            h.kit_mut().set_size(size);
            h.run_frames(4);
            let mut dock = egui_dock::DockState::new(vec![crate::workspace::Pane::Canvas]);
            dock.main_surface_mut().split_right(egui_dock::NodeIndex::root(), 0.66, vec![pane]);
            h.editor_mut().dock = dock;
            h.run_frames(4);
            h.shot(size, &dir.join(format!("{name}{suffix}.png")));
        }
    }
}

/// AU14: the Effects step: a graph with nothing in it, a shelf disabled on a
/// hand-built graph, the sheet's graph before any effect, the shelf with an
/// effect on the picture, the Add effect menu, its More open, the frame box
/// on the Result, and a framed Result after a drag.
const FX: [&str; 8] =
    ["fx-empty", "fx-disabled", "fx-start", "fx-shelf", "fx-add-menu", "fx-more", "fx-frame-box", "fx-framed"];

pub(crate) fn fx_owed() -> Vec<(String, u32, u32)> {
    let sized = |state: &&str| COLOR_SIZES.map(|(suffix, w, h)| (format!("{state}{suffix}.png"), w, h));
    FX.iter().flat_map(sized).collect()
}

/// A cut with one drawing and no graph, for the shots that need its graph.
fn fx_bare() -> crate::doc::AppState {
    let mut s = crate::doc::AppState::new_project("shot", 1280, 720, 24, 72.0);
    s.set_cut_length(12);
    s.new_drawing_at_frame();
    s
}

fn fx_moments(dir: &Path) {
    for (suffix, w, tall) in COLOR_SIZES {
        let size = egui::vec2(w as f32, tall as f32);
        // Each shot waits for the picture, as a walk's room does: one taken
        // mid-render is a shot of `Rendering effects…`.
        let shoot = |h: &mut EditorHarness, state: &str| {
            arrive(h, ProbeRoom::Stage(Stage::Composite));
            h.shot(size, &dir.join(format!("{state}{suffix}.png")));
        };
        let room = |state| {
            let mut h = EditorHarness::with_gpu_shipped(state);
            h.kit_mut().set_size(size);
            h.run_frames(4);
            h.editor_mut().enter_room(Stage::Composite);
            h
        };
        // Each in its own block: a harness holds the GPU's turn until it goes,
        // and a shadowed one has not gone.
        {
            let mut h = room(fx_bare());
            shoot(&mut h, "fx-empty");
        }
        {
            let mut bare = fx_bare();
            bare.graph_add_node(anim_core::graph::NodeKind::Solid { rgba: [255, 255, 255, 255] }, (40.0, 40.0));
            let mut h = room(bare);
            shoot(&mut h, "fx-disabled");
        }

        let mut h = EditorHarness::with_gpu_shipped(crate::auguide_tests::film().build());
        h.kit_mut().set_size(size);
        arrive(&mut h, ProbeRoom::Stage(Stage::Drawing));
        h.hide_sheet();
        // Starter red: the shelf's Glow has something to light.
        h.editor_mut().canvas.brush_color = crate::plate::STARTER_SWATCHES[2].1.to_array();
        let c = h.editor().canvas.dbg_rect.center();
        let ring: Vec<egui::Pos2> = (0..=35)
            .map(|n| c + 70.0 * egui::Vec2::angled(n as f32 / 32.0 * std::f32::consts::TAU))
            .collect();
        h.drag_path(&ring);
        h.run_frames(8);
        h.editor_mut().enter_room(Stage::Composite);
        shoot(&mut h, "fx-start");
        h.click("Glow");
        shoot(&mut h, "fx-shelf");
        h.click(crate::vocab::ADD_EFFECT);
        shoot(&mut h, "fx-add-menu");
        h.key(egui::Key::Escape);
        h.run_frames(4);
        h.click(crate::nodegraph_panel::SHELF_MORE);
        shoot(&mut h, "fx-more");
        h.key(egui::Key::Escape);
        h.run_frames(4);
        h.click(crate::vocab::CAMERA_MOVE);
        shoot(&mut h, "fx-frame-box");
        let boxed = h.editor().viewers[&0].drawn_box.expect("the box stands").center();
        h.drag(boxed, boxed + egui::vec2(-30.0, 12.0), 6);
        shoot(&mut h, "fx-framed");
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
    // AU16: a name of its own, for the failed shot prints the path, and a temp
    // folder's name would make two runs' pictures differ.
    let wall = std::env::temp_dir().join("gaban-uishot-wall");
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
            h.open_path("File/Export");
            assert!(h.has(crate::config::Action::ExportGif.label()), "{state} shows its rows");
        }
        h.shot(size, &dir.join(format!("{state}.png")));
        if state.contains("exporting") {
            let said = h.painted_text().iter().any(|words| words == "Exporting ball.gif…");
            assert!(said, "{state} shows its remark: {:?}", h.painted_text());
            h.run_export_to_end();
        }
    }
    std::fs::remove_file(&wall).expect("the wall goes");
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
    check_shots(&dir, &states_owed()).expect("the states the look reaches");
    drop(h);
    plan_moments(&dir);
    check_shots(&dir, &shots_owed()).expect("the shots the spec lists");
    check_shots(&dir, &plan_owed()).expect("Plan's moments");
    shipped(&dir);
    check_shots(&dir, &shipped_owed()).expect("what only a shipped editor shows");
    rail_and_floats(&dir);
    check_shots(&dir, &rail_owed()).expect("the rail's tiers and every float");
    strip_moments(&dir);
    check_shots(&dir, &strip_owed()).expect("the strip's moments");
    start_float(&dir);
    check_shots(&dir, &start_owed()).expect("the Start float's moments");
    lesson_card(&dir);
    check_shots(&dir, &lesson_owed()).expect("the lesson card's moments");
    guide_moments(&dir);
    check_shots(&dir, &guide_owed()).expect("the ring");
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
    upgrade_moments(&dir);
    check_shots(&dir, &upgrade_owed()).expect("an upgrader's first launch");
    kinds_moments(&dir);
    check_shots(&dir, &kinds_owed()).expect("the three bars");
    page_moments(&dir);
    check_shots(&dir, &page_owed()).expect("the comic step");
    tail_moments(&dir);
    check_shots(&dir, &tail_owed()).expect("the twelve rows and the hold keys");
    color_moments(&dir);
    check_shots(&dir, &color_owed()).expect("the Color step");
    fx_moments(&dir);
    check_shots(&dir, &fx_owed()).expect("the Effects step");
    panel_moments(&dir);
    check_shots(&dir, &panels_owed()).expect("the panels no walk stops on");
}

/// Every list the pass owes, by the name it checks it under.
fn every_owed() -> Vec<(&'static str, Vec<(String, u32, u32)>)> {
    vec![
        ("shots_owed", shots_owed()),
        ("states_owed", states_owed()),
        ("shipped_owed", shipped_owed()),
        ("plan_owed", plan_owed()),
        ("rail_owed", rail_owed()),
        ("strip_owed", strip_owed()),
        ("start_owed", start_owed()),
        ("lesson_owed", lesson_owed()),
        ("guide_owed", guide_owed()),
        ("say_owed", say_owed()),
        ("queue_owed", queue_owed()),
        ("export_owed", export_owed()),
        ("hide_owed", hide_owed()),
        ("keys_owed", keys_owed()),
        ("upgrade_owed", upgrade_owed()),
        ("kinds_owed", kinds_owed()),
        ("page_owed", page_owed()),
        ("tail_owed", tail_owed()),
        ("color_owed", color_owed()),
        ("fx_owed", fx_owed()),
        ("panels_owed", panels_owed()),
    ]
}

/// AU16: a PNG's size and its pixels' bytes.
fn picture(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{e}"))?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file)).read_info().map_err(|e| format!("{e}"))?;
    let mut bytes = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut bytes).map_err(|e| format!("{e}"))?;
    bytes.truncate(info.buffer_size());
    Ok((info.width, info.height, bytes))
}

/// AU16: every picture in `base` against its namesake in `after`, by decoded
/// pixels: one line for each that moved, and none if nothing did.
fn pixel_diff(base: &Path, after: &Path) -> Vec<String> {
    let files = std::fs::read_dir(base).expect("the base shots").flatten();
    let mut names: Vec<String> = files.map(|f| f.file_name().to_string_lossy().to_string()).collect();
    names.retain(|name| name.ends_with(".png"));
    names.sort();
    let mut moved = Vec::new();
    for name in names {
        match (picture(&base.join(&name)), picture(&after.join(&name))) {
            (Ok((w, h, a)), Ok((got_w, got_h, b))) if (w, h) == (got_w, got_h) => {
                // A page's mono PNG holds under a byte a pixel: there, bytes.
                let width = (a.len() / (w * h) as usize).max(1);
                let differ = a.chunks(width).zip(b.chunks(width)).filter(|(x, y)| x != y).count();
                if differ > 0 {
                    moved.push(format!("{name}: {differ} pixels"));
                }
            }
            (Ok((w, h, _)), Ok((got_w, got_h, _))) => moved.push(format!("{name}: {w}x{h} became {got_w}x{got_h}")),
            (Err(e), _) | (_, Err(e)) => moved.push(format!("{name}: {e}")),
        }
    }
    moved
}

/// Spec AU16's gate, and more than its rooms: every picture the ignored pass
/// owes, at both sizes, against the part's start, so the sweep's diff covers
/// the lesson surfaces (feedback spec 1.11). `AUTOKEN_BASE` is a folder of
/// `uishot_rooms` run on the commit before the part. This module's name puts
/// the test in every `uishot` run, which sets no base: that run says so and
/// proves nothing here, and does not take the pictures twice.
#[test]
#[ignore = "needs a GPU adapter, UISHOT_DIR and AUTOKEN_BASE; writes PNGs"]
fn autoken_render_is_unchanged_by_the_removal() {
    let Some(base) = std::env::var_os("AUTOKEN_BASE").map(PathBuf::from) else {
        println!("AUTOKEN_BASE is not set: no pixel diff ran");
        return;
    };
    check_shots(&base, &shots_owed()).expect("the part's start's rooms");
    check_shots(&base, &lesson_owed()).expect("and its lesson surfaces");
    uishot_rooms();
    crate::auguide_show_tests::uishot_deckle_contact_sheet();
    let moved = pixel_diff(&base, &out_dir());
    assert!(moved.is_empty(), "{} pictures moved:\n{}", moved.len(), moved.join("\n"));
}

/// The diff behind that gate, which only the ignored pass runs: one pixel of
/// one picture is enough, and so is a picture gone or a size changed.
#[test]
fn autoken_the_pixel_diff_sees_one_pixel() {
    let write = |path: PathBuf, (w, h): (u32, u32), bytes: &[u8]| {
        let mut encoder = png::Encoder::new(std::fs::File::create(path).expect("a file"), w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().expect("a header").write_image_data(bytes).expect("its pixels");
    };
    let (base, after) = (tempfile::tempdir().expect("a folder"), tempfile::tempdir().expect("a folder"));
    let pixels = vec![200u8; 2 * 2 * 4];
    for name in ["same.png", "moved.png", "gone.png"] {
        write(base.path().join(name), (2, 2), &pixels);
    }
    write(base.path().join("sized.png"), (2, 2), &pixels);
    write(after.path().join("same.png"), (2, 2), &pixels);
    write(after.path().join("sized.png"), (1, 4), &pixels);
    let mut one = pixels.clone();
    one[7] = 199;
    write(after.path().join("moved.png"), (2, 2), &one);
    std::fs::write(base.path().join("note.txt"), "not a picture").expect("a note");
    let moved = pixel_diff(base.path(), after.path());
    assert_eq!(moved.len(), 3, "{moved:?}");
    assert!(moved[0].starts_with("gone.png: "), "{moved:?}");
    assert_eq!(moved[1], "moved.png: 1 pixels");
    assert_eq!(moved[2], "sized.png: 2x2 became 1x4");
    assert_eq!(pixel_diff(base.path(), base.path()), Vec::<String>::new(), "a folder equals itself");
}

/// Only the ignored pass writes them, so this holds it to the whole count:
/// each list checked there, no name twice, and the part's new panels taken
/// before they are checked.
#[test]
fn audress_the_ignored_pass_owes_every_shot() {
    let pass = include_str!("uishot.rs").split("fn uishot_rooms()").nth(1).expect("the pass");
    let taken = pass.find("panel_moments(&dir);").expect("it takes them");
    let checked = pass.find("check_shots(&dir, &panels_owed())").expect("it checks them");
    assert!(taken < checked, "taken, then checked");
    let lists = every_owed();
    for (name, _) in &lists {
        assert!(pass.contains(&format!("check_shots(&dir, &{name}())")), "{name} is checked");
    }
    let names: BTreeSet<&str> = lists.iter().flat_map(|(_, l)| l.iter().map(|o| o.0.as_str())).collect();
    let owed: usize = lists.iter().map(|(_, l)| l.len()).sum();
    assert_eq!(names.len(), owed, "no two shots share a name");
    assert_eq!(owed, 273 + 2 * PANELS.len() + 14 + 16 + 17 + 2, "the 273 owed before AU15, its panels at both sizes, AU23's and AU24's lessons: three at both and eight steps each, AU24's Plan at both, AU25's three at both, seven steps, its ring at both and its finish at both, and AU17's upgrader at both");
}

/// AU17: the final shots: every room, the upgrader's first launch and every
/// group the pass owes are taken and then checked, at both sizes. Each size
/// has every room; only what one window cannot show stands at one size.
#[test]
fn aurel_final_shots_stand_at_both_sizes() {
    let pass = include_str!("uishot.rs").split("fn uishot_rooms()").nth(1).expect("the pass");
    let last = pass.find("\n}\n").expect("the pass ends");
    let pass = &pass[..last];
    for (name, _) in every_owed() {
        let check = format!("check_shots(&dir, &{name}())");
        assert_eq!(pass.matches(&check).count(), 1, "{name} once");
    }
    for group in ["upgrade", "keys", "export", "start"] {
        let taken = pass.find(&format!("{group}_moments(&dir);")).or_else(|| pass.find(&format!("{group}_float(&dir);")));
        let checked = pass.find(&format!("check_shots(&dir, &{group}_owed())")).expect("it checks them");
        assert!(taken.expect("it takes them") < checked, "{group}: taken, then checked");
    }
    let lists = every_owed();
    let owed: Vec<&(String, u32, u32)> = lists.iter().flat_map(|(_, l)| l.iter()).collect();
    for (w, h, suffix) in SIZES {
        let rooms = PROBE_ROOMS.into_iter().enumerate().map(|(i, room)| shot_name(i, room, suffix));
        for name in rooms {
            assert!(owed.contains(&&(name.clone(), w, h)), "{name} at {w}");
        }
        let here = owed.iter().filter(|o| (o.1, o.2) == (w, h)).count();
        assert!(here >= 140, "{w}: {here}");
        println!("shots at {w}×{h}: {here}");
    }
    for state in ["upgrade-first-launch", "start-whats-new", "export-failed", "say-keys-moved"] {
        assert!(owed.iter().any(|o| o.0 == format!("{state}.png")), "{state} at 1600");
        assert!(owed.iter().any(|o| o.0 == format!("{state}-1280.png")), "{state} at 1280");
    }
}

/// Only the ignored pass takes them, so this holds it to them: all five of
/// the Color step's moments, at both sizes, taken and then checked.
#[test]
fn aucolor_the_ignored_pass_owes_the_color_shots() {
    let pass = include_str!("uishot.rs").split("fn uishot_rooms()").nth(1).expect("the pass");
    let taken = pass.find("color_moments(&dir);").expect("it takes them");
    let checked = pass.find("check_shots(&dir, &color_owed())").expect("it checks them");
    assert!(taken < checked, "taken, then checked");
    let owed = color_owed();
    assert_eq!(owed.len(), 10);
    for moment in COLOR {
        assert!(owed.contains(&(format!("{moment}.png"), 1600, 900)), "{moment} at 1600");
        assert!(owed.contains(&(format!("{moment}-1280.png"), 1280, 800)), "{moment} at 1280");
    }
}

#[test]
fn autail_the_ignored_pass_owes_the_tail_shots() {
    let pass = include_str!("uishot.rs").split("fn uishot_rooms()").nth(1).expect("the pass");
    let taken = pass.find("tail_moments(&dir);").expect("it takes them");
    let checked = pass.find("check_shots(&dir, &tail_owed())").expect("it checks them");
    assert!(taken < checked, "taken, then checked");
    let owed = tail_owed();
    assert_eq!(owed.len(), 6);
    for moment in ["tail-twelve-rows", "tail-three-drawings", "tail-hold-keys"] {
        assert!(owed.contains(&(format!("{moment}.png"), 1600, 900)), "{moment} at 1600");
        assert!(owed.contains(&(format!("{moment}-1280.png"), 1280, 800)), "{moment} at 1280");
    }
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
    assert_eq!(shots_owed().len() + states_owed().len() + shipped.len(), 76);
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
    // Sixteen, the Export step's two and the Lessons page's two, at the same
    // two sizes.
    assert_eq!(owed.len(), 20);
    let names: BTreeSet<&str> = owed.iter().map(|o| o.0.as_str()).collect();
    assert_eq!(names.len(), 20, "no two shots share a name: {names:?}");
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
