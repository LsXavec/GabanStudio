//! AU3: the flush Draw chassis: one top row, six steps, and a canvas with
//! nothing around it (research/PSD-approachable-ui.md, AU3).

use eframe::egui;
use egui::vec2;
use egui_dock::{DockState, Node};

use crate::config::{self, Action, Config};
use crate::doc::AppState;
use crate::measure::{self, Scope};
use crate::testkit::EditorHarness;
use crate::workspace::{Pane, Stage, Workspace, Workspaces};
use crate::{PROBE_ROOMS, PROBE_SCALES, plate, probe_run_in, sample, steps, templates, vocab};

const SIZES: [(f32, f32); 4] = [(1600.0, 900.0), (1280.0, 800.0), (1280.0, 720.0), (1024.0, 640.0)];
const ROW_H: f32 = 44.0;

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

fn panes(h: &EditorHarness) -> Vec<Pane> {
    h.editor().dock.iter_all_tabs().map(|(_, p)| *p).collect()
}

/// A window `w`×`h` points at 100%, at a stored scale: the App's zoom, capped
/// by the window. The harness holds the size in zoomed points.
fn window(harness: &mut EditorHarness, w: f32, h: f32, stored: f32) {
    let scale = crate::effective_scale(stored, w, h);
    harness.kit_mut().set_size(vec2(w / scale, h / scale));
    harness.kit_mut().set_pixels_per_point(scale);
    settle(harness);
}

/// The rect of a named widget in points: the harness names them in pixels.
fn points(h: &EditorHarness, name: &str) -> egui::Rect {
    let (r, ppp) = (h.rect(name), h.ctx().pixels_per_point());
    egui::Rect::from_min_max((r.min.to_vec2() / ppp).to_pos2(), (r.max.to_vec2() / ppp).to_pos2())
}

fn workspace(name: &str, dock: DockState<Pane>) -> Workspace {
    Workspace { name: name.into(), dock, preset: None, view: None }
}

/// Null `x` and `y` anywhere under `v`: a rect that cannot be read back.
fn null_rects(v: &serde_json::Value) -> usize {
    match v {
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(k, v)| if matches!(k.as_str(), "x" | "y") && v.is_null() { 1 } else { null_rects(v) })
            .sum(),
        serde_json::Value::Array(items) => items.iter().map(null_rects).sum(),
        _ => 0,
    }
}

fn workspace_panes(w: &Workspace) -> Vec<Pane> {
    w.dock.iter_all_tabs().map(|(_, p)| *p).collect()
}

// ---- the flush dock ----

/// The kill test: if this one fails, the chassis falls back to a bare host.
#[test]
fn audock_flush_body_is_the_centre_rect() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    let centre = plate::dock_rect(&h.ctx()).expect("a dock");
    assert!(centre.is_finite() && centre.is_positive(), "{centre:?}");
    assert_eq!(panes(&h), [Pane::Canvas]);
    assert_eq!(plate::pane_bodies(&h.ctx()), [centre], "one body, the whole centre");
    let style = crate::dock_style(&h.ctx().global_style(), &h.editor().dock);
    assert_eq!(style.tab_bar.height, 0.0);
    assert_eq!(style.tab.tab_body.inner_margin, egui::Margin::ZERO);
    assert!(style.dock_area_padding.is_none());
    // A second tab brings the tab bar back.
    h.editor_mut().open_pane(Pane::History);
    settle(&mut h);
    let style = crate::dock_style(&h.ctx().global_style(), &h.editor().dock);
    assert_eq!(style.tab_bar.height, 34.0);
}

#[test]
fn audock_flush_dock_saves_and_reloads() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    let list = vec![workspace("drawn", h.editor().dock.clone()), workspace("never drawn", Stage::Drawing.dock())];
    let dir = tempfile::tempdir().unwrap();
    Workspaces { list }.save_to(dir.path()).unwrap();
    let text = std::fs::read_to_string(Workspaces::path_in(dir.path())).unwrap();
    assert_eq!(null_rects(&serde_json::from_str(&text).unwrap()), 0);
    let (back, notes) = Workspaces::load_from(dir.path());
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(back.list.len(), 2);
    for w in back.list {
        assert_eq!(workspace_panes(&w), [Pane::Canvas], "{}", w.name);
        // And it draws, flush.
        h.editor_mut().dock = w.dock;
        settle(&mut h);
        assert_eq!(plate::pane_bodies(&h.ctx()), [plate::dock_rect(&h.ctx()).unwrap()], "{}", w.name);
    }
}

#[test]
fn audock_every_save_path_normalises() {
    // The menu's save, of a dock that has never been drawn.
    let mut h = EditorHarness::as_shipped();
    h.editor_mut().workspaces.list.push(workspace("saved", Stage::Drawing.dock()));
    h.editor_mut().save_workspaces();
    let text = std::fs::read_to_string(Workspaces::path_in(h.data_dir())).unwrap();
    assert_eq!(null_rects(&serde_json::from_str(&text).unwrap()), 0);
    // The host's send to its guests.
    h.editor_mut().dock = Stage::Drawing.dock();
    let sent = h.editor().host_layout().expect("a layout to send");
    assert_eq!(null_rects(&serde_json::from_str(&sent).unwrap()), 0);
    let back: Workspace = serde_json::from_str(&sent).expect("a guest can read it");
    assert_eq!(workspace_panes(&back), [Pane::Canvas]);
    // A dock that was drawn stays what it was.
    let w = workspace("wide", Stage::dock_with_sheet());
    assert_eq!(workspace_panes(&w.normalised().unwrap()), workspace_panes(&w));
}

#[test]
fn audock_old_workspaces_still_load() {
    // As the last release wrote one: never normalised, with its Brush band.
    let old = workspace("old", DockState::new(vec![Pane::Brush, Pane::Canvas]));
    let text = serde_json::to_string(&Workspaces { list: vec![old] }).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(Workspaces::path_in(dir.path()), text).unwrap();
    let mut h = EditorHarness::with_dir_shipped(dir);
    assert_eq!(h.editor().workspaces.list.len(), 1, "{}", h.describe());
    let dock = h.editor().workspaces.list[0].dock.clone();
    h.editor_mut().dock = dock;
    settle(&mut h);
    assert_eq!(panes(&h), [Pane::Canvas], "the retired pane still leaves on sight");
    assert!(!h.data_dir().join("workspaces.json.bad").exists());
}

#[test]
fn audock_recovers_null_rect_workspaces() {
    let fixture = include_str!("fixtures/workspaces-null-rects.json");
    let raw: serde_json::Value = serde_json::from_str(fixture).unwrap();
    assert!(null_rects(&raw) > 0, "the fixture is the real, broken file");
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(Workspaces::path_in(dir.path()), fixture).unwrap();
    let (saved, notes) = Workspaces::load_from(dir.path());
    assert!(notes.is_empty(), "{notes:?}");
    assert!(!dir.path().join("workspaces.json.bad").exists());
    let items = raw["list"].as_array().unwrap();
    assert_eq!(saved.list.len(), items.len());
    let names: Vec<&str> = saved.list.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["draw", "timing"]);
    for (w, item) in saved.list.iter().zip(items) {
        assert_eq!(item["name"], w.name.as_str());
        let tabs = item["dock"]["surfaces"].to_string();
        for pane in workspace_panes(w) {
            assert!(tabs.contains(&serde_json::to_string(&pane).unwrap()), "{} lost {pane:?}", w.name);
        }
        assert_eq!(w.preset.as_ref(), item["preset"].as_str().map(str::to_string).as_ref());
    }
    // Saved back, the file is whole.
    saved.save_to(dir.path()).unwrap();
    let text = std::fs::read_to_string(Workspaces::path_in(dir.path())).unwrap();
    assert_eq!(null_rects(&serde_json::from_str(&text).unwrap()), 0);
}

#[test]
fn audock_open_pane_splits_a_single_leaf() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    h.editor_mut().open_pane(Pane::History);
    settle(&mut h);
    let tree = h.editor().dock.main_surface();
    let leaves: Vec<&[Pane]> = tree.iter().filter_map(Node::tabs).collect();
    assert_eq!(leaves, [&[Pane::Canvas][..], &[Pane::History][..]], "the paper stays in view");
    match tree.root_node() {
        Some(Node::Horizontal(split)) => assert_eq!(split.fraction, 0.70),
        _ => panic!("not split left and right"),
    }
    assert!(h.editor().canvas.dbg_rect.is_positive(), "the canvas still draws");
    // A dock of several leaves keeps its rule: beside the sheet.
    h.editor_mut().enter_room(Stage::InkPaint);
    h.editor_mut().open_pane(Pane::Episode);
    assert!(panes(&h).contains(&Pane::Episode));
    // The Window menu's + Viewer splits too.
    h.editor_mut().enter_room(Stage::Drawing);
    settle(&mut h);
    h.click("window");
    h.click("+ Viewer");
    settle(&mut h);
    let leaves = h.editor().dock.main_surface().iter().filter_map(Node::tabs).count();
    assert_eq!(leaves, 2);
    assert!(panes(&h).contains(&Pane::Viewer(0)));
}

/// A middle click on the first tab: where egui_dock closes one, and the only
/// door the tab has to the harness (its tabs speak no name). The leaf's
/// collapse key holds the bar's first 24 points.
fn middle_click_first_tab(h: &mut EditorHarness) {
    h.middle_click_pos(plate::pane_bodies(&h.ctx())[0].left_top() + vec2(48.0, -14.0));
    settle(h);
}

#[test]
fn audock_canvas_tab_cannot_close() {
    let mut h = EditorHarness::as_shipped();
    h.editor_mut().dock = DockState::new(vec![Pane::Canvas, Pane::History]);
    settle(&mut h);
    middle_click_first_tab(&mut h);
    assert_eq!(panes(&h), [Pane::Canvas, Pane::History], "the Canvas tab stays");
    // The same click on any other tab closes it.
    h.editor_mut().dock = DockState::new(vec![Pane::History, Pane::Canvas]);
    settle(&mut h);
    middle_click_first_tab(&mut h);
    assert_eq!(panes(&h), [Pane::Canvas], "a panel closes");
    // Under the lock a recipe's panes hold, and one opened by hand still goes.
    h.editor_mut().enter_room(Stage::Edit);
    let recipe = h.editor().locked_panes();
    assert!(recipe.contains(&Pane::Canvas) && recipe.contains(&Pane::XSheet));
    assert!(!recipe.contains(&Pane::History));
}

// ---- Draw and the sheet ----

#[test]
fn audock_draw_is_canvas_alone_when_shipped() {
    for mut h in [EditorHarness::as_shipped(), EditorHarness::as_first_run()] {
        settle(&mut h);
        assert_eq!(h.editor().stage, Some(Stage::Drawing));
        assert_eq!(panes(&h), [Pane::Canvas], "{:?}", h.boot());
        assert!(!h.rig().config.ui.sheet_in_draw);
        assert!(!h.rig().config.ui.lock_positions || h.editor().locked_panes() == [Pane::Canvas]);
    }
    let mut h = EditorHarness::as_shipped();
    h.perform(Action::RoomFinishing);
    h.perform(Action::RoomDrawing);
    assert_eq!(panes(&h), [Pane::Canvas]);
    assert_eq!(Stage::Drawing.dock().iter_all_tabs().count(), 1);
    // No recipe keeps the retired Brush band.
    for st in Stage::ALL {
        assert!(st.dock().iter_all_tabs().all(|(_, p)| *p != Pane::Brush), "{st:?}");
    }
    assert!(Stage::dock_with_sheet().iter_all_tabs().all(|(_, p)| *p != Pane::Brush));
    assert!(Stage::Manga.dock().iter_all_tabs().all(|(_, p)| *p != Pane::Brush));
}

#[test]
fn audock_sheet_button_is_one_click_and_remembered() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    assert!(!h.has(vocab::HIDE_TIMING_SHEET));
    h.click(vocab::SHOW_TIMING_SHEET);
    settle(&mut h);
    assert!(panes(&h).contains(&Pane::XSheet));
    assert!(h.has(vocab::HIDE_TIMING_SHEET), "{}", h.describe());
    assert!(h.rig().config.ui.sheet_in_draw, "remembered");
    let saved = Config::load_from(h.data_dir()).0;
    assert!(saved.ui.sheet_in_draw, "and saved");
    // Out to a step and back, it is still there.
    h.perform(Action::RoomFinishing);
    h.perform(Action::RoomDrawing);
    settle(&mut h);
    assert!(panes(&h).contains(&Pane::XSheet));
    h.click(vocab::HIDE_TIMING_SHEET);
    settle(&mut h);
    assert_eq!(panes(&h), [Pane::Canvas]);
    assert!(!h.rig().config.ui.sheet_in_draw);
    assert!(!Config::load_from(h.data_dir()).0.ui.sheet_in_draw);
    // Absent where the recipe holds the sheet.
    for action in [Action::RoomLayout, Action::RoomEdit] {
        h.perform(action);
        settle(&mut h);
        assert!(!h.has(vocab::SHOW_TIMING_SHEET) && !h.has(vocab::HIDE_TIMING_SHEET), "{action:?}");
    }
    // Elsewhere it opens the pane, and the Draw choice stays what it was.
    h.perform(Action::Layout3dRoom);
    settle(&mut h);
    h.click(vocab::SHOW_TIMING_SHEET);
    settle(&mut h);
    assert!(panes(&h).contains(&Pane::XSheet));
    assert!(!h.rig().config.ui.sheet_in_draw);
}

#[test]
fn audock_sheet_choice_survives_a_restart() {
    let mut first = EditorHarness::as_shipped();
    settle(&mut first);
    first.click(vocab::SHOW_TIMING_SHEET);
    settle(&mut first);
    assert!(config::config_path_in(first.data_dir()).exists(), "saved at the click");
    let h = EditorHarness::with_dir_shipped(first.quit());
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert!(panes(&h).contains(&Pane::XSheet), "{:?}", panes(&h));
    assert!(h.has(vocab::HIDE_TIMING_SHEET));
}

#[test]
fn audock_sheet_button_keeps_a_hand_arrangement() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    h.editor_mut().open_pane(Pane::History);
    settle(&mut h);
    h.click(vocab::SHOW_TIMING_SHEET);
    settle(&mut h);
    assert_eq!(panes(&h).len(), 3, "{:?}", panes(&h));
    assert!(panes(&h).contains(&Pane::History) && panes(&h).contains(&Pane::XSheet));
    assert!(h.rig().config.ui.sheet_in_draw);
    h.click(vocab::HIDE_TIMING_SHEET);
    settle(&mut h);
    assert_eq!(panes(&h), [Pane::Canvas, Pane::History], "only the sheet left");
    assert!(!h.rig().config.ui.sheet_in_draw);
    // A recipe whose divider moved is still the recipe.
    h.perform(Action::ResetLayout);
    h.click(vocab::SHOW_TIMING_SHEET);
    settle(&mut h);
    if let Some(Node::Horizontal(split) | Node::Vertical(split)) = h.editor_mut().dock.main_surface_mut().root_node_mut() {
        split.fraction = 0.4;
    }
    settle(&mut h);
    h.click(vocab::HIDE_TIMING_SHEET);
    settle(&mut h);
    assert_eq!(panes(&h), [Pane::Canvas]);
}

#[test]
fn audock_reset_reads_the_sheet_switch() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    h.editor_mut().open_pane(Pane::History);
    h.perform(Action::ResetLayout);
    assert_eq!(panes(&h), [Pane::Canvas]);
    h.click(vocab::SHOW_TIMING_SHEET);
    settle(&mut h);
    let with_sheet = panes(&h);
    h.editor_mut().open_pane(Pane::History);
    h.perform(Action::ResetLayout);
    assert_eq!(panes(&h), with_sheet);
}

#[test]
fn audock_frozen_boot_keeps_the_sheet() {
    let mut h = EditorHarness::new();
    settle(&mut h);
    assert!(panes(&h).contains(&Pane::XSheet), "the tests about something else keep their floor");
    assert!(h.editor().sheet_in_draw && h.rig().config.ui.sheet_in_draw);
    assert!(!h.rig().config.ui.lock_positions);
    assert!(!h.rig().config.ui.sheet_in_draw || panes(&h).len() > 1);
    // And a rebuilt editor under the same boot.
    h.rebuild_editor(sample::build_sample());
    h.run();
    assert!(panes(&h).contains(&Pane::XSheet));
}

#[test]
fn audock_templates_start_the_sheet_where_they_say() {
    for (i, t) in templates::ALL.iter().enumerate() {
        let ed = crate::open_template(i, None, None, &Default::default());
        assert_eq!(ed.sheet_in_draw, t.sheet_on, "{}", t.name);
        assert_eq!(ed.stage, Some(t.room), "{}", t.name);
    }
    let on: Vec<&str> = templates::ALL.iter().filter(|t| t.sheet_on).map(|t| t.name).collect();
    assert_eq!(on, ["TV anime HD", "Feature UHD"]);
    // The choice outlives the template: the App copies it into the config.
    let mut h = EditorHarness::as_shipped();
    *h.editor_mut() = crate::open_template(0, None, Some(h.data_dir()), &Default::default());
    settle(&mut h);
    assert!(h.rig().config.ui.sheet_in_draw);
    h.perform(Action::RoomDrawing);
    assert!(panes(&h).contains(&Pane::XSheet));
}

#[test]
fn audock_sheet_recipe_passes_the_layout_probe() {
    for ppp in PROBE_SCALES {
        let run = probe_run_in(ppp, 100, Pane::Canvas, |e| {
            e.sheet_in_draw = true;
            e.enter_room(Stage::Drawing);
        });
        assert_eq!((run.moved, run.unmeasured), (0, 0), "{ppp}\n{}", run.lines.join("\n"));
    }
}

// ---- the top row ----

#[test]
fn audock_top_row_is_one_row_at_every_size() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    for (w, tall) in SIZES {
        for stored in PROBE_SCALES {
            window(&mut h, w, tall, stored);
            let at = format!("{w}×{tall} at {stored}");
            let row = points(&h, vocab::STEP_DRAW).center().y;
            for name in [vocab::STEP_PLAN, vocab::STEP_EXPORT, "Undo (toolbar)", "Search"] {
                let r = points(&h, name);
                assert!((r.center().y - row).abs() < 0.5, "{name} off the row at {at}");
                assert!(r.top() >= 0.0 && r.bottom() <= ROW_H, "{name} at {r:?} at {at}");
            }
            let dock = plate::dock_rect(&h.ctx()).expect("a dock");
            assert!((dock.top() - ROW_H).abs() < 0.5, "the row stands 44 at {at}: {dock:?}");
            let last = points(&h, vocab::STEP_EXPORT);
            let keys = points(&h, "Undo (toolbar)");
            assert!(last.right() + steps::GROUP_GAP <= keys.left(), "steps run into the actions at {at}");
            let right = h.ctx().content_rect().right();
            assert!((points(&h, "Search").right() - (right - 8.0)).abs() < 0.5, "8 from the edge at {at}");
        }
    }
}

#[test]
fn audock_steps_show_in_order_and_run_their_actions() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    settle(&mut h);
    let lefts: Vec<f32> = steps::STEPS.iter().map(|s| h.rect(s.label).left()).collect();
    assert!(lefts.is_sorted(), "{lefts:?}");
    let labels: Vec<&str> = steps::STEPS.iter().map(|s| s.label).collect();
    assert_eq!(labels, ["Plan", "Draw", "Timing", "Color", "Effects", "Export"]);
    for step in &steps::STEPS {
        h.click(step.label);
        settle(&mut h);
        match step.stage {
            Some(st) => {
                assert_eq!(h.editor().stage, Some(st), "{}", step.label);
                assert_eq!(h.state().status, format!("step: {}", step.label));
                assert_eq!(h.node_toggled(step.label), Some(true), "the current pill says so");
            }
            None => assert!(panes(&h).contains(&Pane::RenderQueue), "{:?}", panes(&h)),
        }
    }
    // The stored words hold still under the new ones.
    assert_eq!(Stage::ALL.len(), 5);
    assert_eq!(crate::stage_key(Some(Stage::Drawing)), "drawing");
    assert_eq!(crate::stage_key(Some(Stage::InkPaint)), "finishing");
    // The old words still find the steps.
    assert!(Action::RoomDrawing.aliases().contains(&"genga"));
    assert!(Action::RoomFinishing.aliases().contains(&"shiage"));
    assert_eq!(Action::RoomDrawing.label(), "Draw step");
}

#[test]
fn audock_step_card_names_the_studio_word() {
    for (label, purpose, word) in [
        (vocab::STEP_PLAN, "Set up shots, timing and camera before drawing.", "layout"),
        (vocab::STEP_DRAW, "Draw on the paper, one drawing at a time.", "genga"),
        (vocab::STEP_TIMING, "Arrange when each drawing appears.", "henshū"),
        (vocab::STEP_COLOR, "Fill your drawings with color.", "shiage"),
        (vocab::STEP_EFFECTS, "Add glow, blur and camera moves.", "satsuei"),
    ] {
        let step = steps::STEPS.iter().find(|s| s.label == label).unwrap();
        assert_eq!(step.card(), format!("{purpose}\nStudio word: {word}"));
    }
    assert_eq!(steps::STEPS[5].card(), "Save a file to share.");
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    h.hover(vocab::STEP_DRAW);
    h.run_frames(3);
    let words = measure::reading(&h).texts;
    assert!(words.iter().any(|(_, t)| t.contains("Studio word: genga")), "{words:?}");
}

#[test]
fn audock_off_spine_pill_shows_and_clears() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    assert!(!h.has_text_containing("Current place"), "on the spine it is absent");
    let before = h.rect(vocab::STEP_EXPORT);
    h.perform(Action::Layout3dRoom);
    settle(&mut h);
    let name = format!("Current place: {}", vocab::LAYOUT_3D);
    assert!(h.has(&name), "{}", h.describe());
    assert_eq!(h.rect(vocab::STEP_EXPORT), before, "the steps hold still");
    assert!(h.rect(&name).left() >= before.right(), "after Export");
    h.click(vocab::STEP_DRAW);
    settle(&mut h);
    assert!(!h.has(&name));
    // Manga is a stage with no pill of its own.
    h.perform(Action::MangaRoom);
    settle(&mut h);
    assert!(h.has(&format!("Current place: {}", vocab::STEP_MANGA)), "{}", h.describe());
    // The forge and a saved arrangement name themselves.
    for room in PROBE_ROOMS {
        room.enter(h.editor_mut());
    }
    settle(&mut h);
    h.editor_mut().dock = Pane::forge_dock();
    h.editor_mut().go_off_spine("Ink test");
    settle(&mut h);
    assert!(h.has("Current place: Ink test"));
    assert_eq!(h.node_enabled("Current place: Ink test"), Some(true));
    let width = h.rect("Current place: Ink test").width();
    assert!((steps::PLACE_MIN..=steps::PLACE_MAX).contains(&width), "{width}");
    // Read-only: a click changes nothing.
    h.click("Current place: Ink test");
    assert!(h.has("Current place: Ink test"));
}

#[test]
fn audock_long_place_name_elides_in_the_pill() {
    let long = "An arrangement with a very long name indeed, past any pill";
    let mut h = EditorHarness::as_shipped();
    h.kit_mut().set_size(vec2(1280.0, 800.0));
    h.editor_mut().enter_room(Stage::Drawing);
    settle(&mut h);
    h.editor_mut().go_off_spine(long);
    settle(&mut h);
    let name = format!("Current place: {long}");
    assert!(h.has(&name), "it answers to the whole name\n{}", h.describe());
    assert!(h.rect(&name).width() <= steps::PLACE_MAX);
    let clip = measure::clip_here(&h, Scope::Chrome);
    assert_eq!(clip.elided, 1, "{:?}", clip.found);
    h.hover(&name);
    h.run_frames(3);
    let words = measure::reading(&h).texts;
    assert!(words.iter().any(|(_, t)| t == long), "the whole name in its hover card");
}

#[test]
fn audock_undo_redo_search_work_from_the_top_row() {
    let blank = AppState::new_project("audock", 1920, 1080, 24, 300.0);
    let mut h = EditorHarness::with_state_shipped(blank);
    settle(&mut h);
    assert_eq!(h.node_enabled("Undo (toolbar)"), Some(false), "a fresh document has no history");
    assert_eq!(h.node_enabled("Redo (toolbar)"), Some(false));
    h.state_mut().new_drawing_at_frame();
    settle(&mut h);
    let at = h.state().engine.position();
    assert!(at > 0, "an edit to undo");
    h.click("Undo (toolbar)");
    settle(&mut h);
    assert_eq!(h.state().engine.position(), at - 1);
    h.click("Redo (toolbar)");
    settle(&mut h);
    assert_eq!(h.state().engine.position(), at);
    assert!(!h.editor().palette.open);
    h.click("Search");
    settle(&mut h);
    assert!(h.editor().palette.open, "Search opens the palette");
    h.perform(Action::CommandPalette);
    // Dirty: the dot is there and has no word.
    settle(&mut h);
    assert!(h.has("unsaved changes"));
    let row = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, ROW_H));
    let words = measure::words_in(&h, row);
    assert!(words.iter().all(|w| !w.contains("unsaved")), "{words:?}");
}

#[test]
fn audock_row_speaks_plain_words_and_shows_search() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    for room in PROBE_ROOMS {
        room.enter(h.editor_mut());
        settle(&mut h);
        let row = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1600.0, ROW_H));
        let words = measure::words_in(&h, row);
        let first_japanese = |w: &String| {
            w.chars().any(|c| c >= '\u{3000}')
                || ["genga", "shiage", "satsuei", "henshū", "layout", "armed"].contains(&w.to_lowercase().as_str())
        };
        assert!(!words.iter().any(first_japanese), "{words:?}");
        assert!(words.iter().any(|w| w == "Search"), "a visible Search: {words:?}");
    }
}

#[test]
fn audock_effective_scale_caps() {
    let cap = crate::effective_scale;
    assert_eq!(cap(1.0, 1600.0, 900.0), 1.0);
    assert_eq!(cap(1.5, 1600.0, 900.0), 1.5);
    assert_eq!(cap(1.5, 1024.0, 640.0), 640.0 / 520.0, "at most 760×520 points of room");
    assert_eq!(cap(1.5, 1280.0, 720.0), 720.0 / 520.0);
    assert_eq!(cap(1.5, 1140.0, 900.0), 1.5);
    assert_eq!(cap(1.5, 950.0, 900.0), 1.25);
    assert_eq!(cap(1.0, 0.0, 0.0), 1.0, "a window with no size keeps the scale");
    for (w, tall) in SIZES {
        for stored in PROBE_SCALES {
            let scale = cap(stored, w, tall);
            assert!(scale <= stored && w / scale >= 760.0 - 0.01 && tall / scale >= 520.0 - 0.01);
        }
    }
    // The stored scale never moves.
    let mut h = EditorHarness::as_shipped();
    h.rig_mut().config.ui.scale = 150;
    window(&mut h, 1024.0, 640.0, 1.5);
    assert_eq!(h.rig().config.ui.scale, 150);
}

#[test]
fn audock_chrome_clip_audit_passes() {
    let mut found = Vec::new();
    for doc in [sample::build_sample(), crate::newproject::NewProjectForm::default().new_state()] {
        let mut h = EditorHarness::with_state_shipped(doc);
        for (w, tall) in SIZES {
            for stored in PROBE_SCALES {
                window(&mut h, w, tall, stored);
                let clip = measure::clip_audit(&mut h, Scope::Chrome);
                found.extend(clip.found.into_iter().map(|f| format!("{w}×{tall} at {stored}: {f}")));
            }
        }
    }
    assert!(found.is_empty(), "{} findings:\n{}", found.len(), found.join("\n"));
}

#[test]
fn audock_band_lanes_keep_apart() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    for (w, tall) in SIZES {
        for stored in PROBE_SCALES {
            window(&mut h, w, tall, stored);
            let wide = h.ctx().content_rect().width() >= steps::FOLD;
            let last = if wide { "dropped 0" } else { "…" };
            let left = points(&h, last).right();
            let right = points(&h, vocab::SHOW_TIMING_SHEET).left();
            assert!(left <= right, "{last} runs into the right lane at {w}×{tall} at {stored}");
            assert_eq!(h.has("first frame (Home)"), wide);
            assert!(h.has("play (Space)"), "Play never folds");
        }
    }
    // The fold keeps every key one click away. The harness clicks at 100%
    // only: at 832 points it is the same row 1024×640 gives at 150%.
    window(&mut h, 832.0, 640.0, 1.0);
    h.click("…");
    settle(&mut h);
    for key in ["first frame (Home)", "last frame (End)", "loop in", "loop out", "clear loop range", "scrub"] {
        assert!(h.has(key), "{key} in the fold\n{}", h.describe());
    }
}

#[test]
fn audock_narrow_row_collapses_to_menu() {
    const MENUS: [&str; 9] = ["file", "edit", "view", "sheet", "cut", "project", vocab::STEP, "window", "help"];
    let mut h = EditorHarness::as_shipped();
    h.kit_mut().set_size(vec2(steps::WIDE, 800.0));
    settle(&mut h);
    assert!(!h.has("Menu") && h.has("file"), "the menus show from 1120");
    assert!(h.rect("Search").width() >= plate::SEARCH_W, "a wide Search");
    h.kit_mut().set_size(vec2(steps::WIDE - 1.0, 800.0));
    settle(&mut h);
    assert!(h.has("Menu") && !h.has("file"), "{}", h.describe());
    assert!(h.rect("Search").width() <= plate::PILL_H, "the lens alone");
    h.click("Menu");
    settle(&mut h);
    for menu in MENUS {
        assert!(h.has(&format!("{menu} ⏵")), "{menu} under Menu\n{}", h.describe());
    }
    h.click("edit ⏵");
    settle(&mut h);
    assert!(h.has("Undo"), "{}", h.describe());
    // The off-spine pill needs its own room.
    let mut h = EditorHarness::as_shipped();
    h.perform(Action::Layout3dRoom);
    h.kit_mut().set_size(vec2(steps::WIDE_PLACE, 800.0));
    settle(&mut h);
    assert!(!h.has("Menu"));
    let pill = h.rect(&format!("Current place: {}", vocab::LAYOUT_3D));
    assert!(pill.right() + steps::GROUP_GAP <= h.rect("Undo (toolbar)").left());
    h.kit_mut().set_size(vec2(steps::WIDE_PLACE - 1.0, 800.0));
    settle(&mut h);
    assert!(h.has("Menu"));
    // Every gated size leaves the pill its minimum.
    for (w, tall) in SIZES {
        for stored in PROBE_SCALES {
            window(&mut h, w, tall, stored);
            let name = format!("Current place: {}", vocab::LAYOUT_3D);
            let pill = points(&h, &name);
            assert!(pill.width() >= steps::PLACE_MIN - 0.5, "{w}×{tall} at {stored}");
            let keys = points(&h, "Undo (toolbar)");
            assert!(pill.right() + steps::GROUP_GAP <= keys.left(), "{w}×{tall} at {stored}");
        }
    }
}
