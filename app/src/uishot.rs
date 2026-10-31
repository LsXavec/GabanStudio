//! Headless pictures of the real editor: `cargo test -p animstudio uishot --
//! --ignored` with UISHOT_DIR set. No window opens; a GPU adapter draws the
//! same egui output the app presents, so a redesign is looked at last, on
//! real pixels, without anything surfacing on the desktop.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;

use crate::config::{Action, SettingsCategory};
use crate::measure::room_tag;
use crate::newproject::NewProjectForm;
use crate::plate;
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
