//! AU15: the panels re-dressed. Every panel speaks the one language and fits
//! its smaller minimum (spec 7.1).

use anim_core::command::Command;
use anim_core::xsheet::ColumnKind;
use eframe::egui;
use egui::vec2;
use egui_dock::NodeIndex;

use crate::aurail_tests::window;
use crate::config::Action;
use crate::doc::AppState;
use crate::measure::{self, Scope};
use crate::newproject::NewProjectForm;
use crate::testkit::EditorHarness;
use crate::workspace::{Pane, Stage};
use crate::{
    PROBE_ROOMS, PROBE_SCALES, Performed, ProbeRoom, board_panel, cmdpal, manga_tools, plate, render_queue, sample,
    templates, vocab,
};

const SIZES: [(f32, f32); 4] = [(1600.0, 900.0), (1280.0, 800.0), (1280.0, 720.0), (1024.0, 640.0)];

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

/// The three documents the budgets and the audit read: `name`, its state.
fn docs() -> [(&'static str, AppState); 3] {
    [
        ("sample", sample::build_sample()),
        ("blank drawing", templates::READY.build()),
        ("blank animation", NewProjectForm::default().new_state()),
    ]
}

/// A person's editor on `state`, showing `pane` alone.
fn alone(state: AppState, pane: Pane) -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(state);
    window(&mut h, 1600.0, 900.0, 1.0);
    h.editor_mut().dock = egui_dock::DockState::new(vec![pane]);
    settle(&mut h);
    h
}

/// Spec 7.1, row for row.
#[test]
fn audress_panel_minimums_are_the_table() {
    let table = [
        (Pane::XSheet, (240.0, 250.0)),
        (Pane::Layers, (200.0, 140.0)),
        (Pane::Forge, (240.0, 220.0)),
        (Pane::Palette, (160.0, 140.0)),
        (Pane::NodeGraph, (360.0, 260.0)),
        (Pane::Viewer(0), (160.0, 90.0)),
        (Pane::History, (180.0, 160.0)),
        (Pane::Curves, (360.0, 200.0)),
        (Pane::PaintCheck, (220.0, 140.0)),
        (Pane::RenderQueue, (280.0, 220.0)),
        (Pane::Board, (170.0, 220.0)),
        (Pane::Episode, (300.0, 100.0)),
        (Pane::View3D, (240.0, 200.0)),
        (Pane::Manga, (280.0, 320.0)),
    ];
    for (pane, (w, tall)) in table {
        assert_eq!(pane.min_size(), vec2(w, tall), "{pane:?}");
    }
    assert_eq!(Pane::Canvas.min_size(), vec2(0.0, 0.0), "the paper fits any slot");
}

/// Every fold in the app, stored and open.
fn all_open() -> std::collections::BTreeMap<String, bool> {
    const KEYS: [&str; 13] = [
        plate::MORE_SHEET,
        plate::MORE_LAYERS,
        plate::MORE_FORGE,
        plate::MORE_COLORS,
        plate::MORE_GRAPH,
        plate::MORE_RESULT,
        plate::MORE_CHECK,
        plate::MORE_EXPORT,
        plate::MORE_SHOTS,
        plate::MORE_EPISODE,
        plate::MORE_3D,
        plate::MORE_MANGA,
        plate::MORE_BRUSHES,
    ];
    KEYS.into_iter().map(|key| (key.to_string(), true)).collect()
}

/// All but the paper, which fits any slot, and the old Brush pane, which no
/// dock keeps.
fn shown_in_a_slot(pane: Pane) -> bool {
    !matches!(pane, Pane::Canvas | Pane::Brush)
}

/// Where `pane`'s body stands this frame.
fn body(h: &EditorHarness, pane: Pane) -> egui::Rect {
    h.editor().pane_rects.iter().find(|(p, ..)| *p == pane).unwrap_or_else(|| panic!("{pane:?} on screen")).1
}

/// `pane` in a slot of `size`, the paper beside it and a wide Result under both.
fn slot(h: &mut EditorHarness, pane: Pane, size: egui::Vec2) -> egui::Rect {
    const DOCK: egui::Vec2 = vec2(1600.0, 800.0);
    let (mut across, mut down) = (1.0 - (size.x + 20.0) / DOCK.x, ((size.y + 50.0) / DOCK.y).min(0.9));
    for _ in 0..3 {
        let mut dock = egui_dock::DockState::new(vec![Pane::Canvas]);
        let tree = dock.main_surface_mut();
        let [top, _] = tree.split_below(NodeIndex::root(), down, vec![Pane::Viewer(1)]);
        tree.split_right(top, across, vec![pane]);
        h.editor_mut().dock = dock;
        settle(h);
        let now = body(h, pane);
        across += (now.width() - size.x) / DOCK.x;
        down -= (now.height() - size.y) / DOCK.y;
    }
    body(h, pane)
}

/// The lowest word painted whole in `inside`.
fn lowest_word(h: &EditorHarness, inside: egui::Rect) -> Option<String> {
    let shapes = &h.kit().output().shapes;
    let shown = measure::texts_of(shapes).into_iter().filter(|(t, clip)| {
        let bound = t.visual_bounding_rect();
        !t.galley.text().trim().is_empty() && inside.contains_rect(bound) && clip.contains_rect(bound)
    });
    let low = shown.max_by(|a, b| a.0.visual_bounding_rect().bottom().total_cmp(&b.0.visual_bounding_rect().bottom()));
    low.map(|(t, _)| t.galley.text().to_string())
}

/// `word` stands whole in `inside`.
fn in_sight(h: &EditorHarness, word: &str, inside: egui::Rect) -> bool {
    let shapes = &h.kit().output().shapes;
    measure::texts_of(shapes).into_iter().any(|(t, clip)| {
        let bound = t.visual_bounding_rect();
        t.galley.text() == word && inside.expand(1.0).contains_rect(bound) && clip.expand(1.0).contains_rect(bound)
    })
}

/// Wheels down over `inside` until `word` stands whole there.
fn wheel_to(h: &mut EditorHarness, word: &str, inside: egui::Rect) -> bool {
    for _ in 0..24 {
        if in_sight(h, word, inside) {
            return true;
        }
        h.wheel_tick_at(inside.center(), -120.0);
        h.run();
        h.run_frames(4);
    }
    in_sight(h, word, inside)
}

/// The policy is the one spec 7.1 gives each panel, and each panel follows it:
/// under its minimum it stands at that minimum and scrolls on both axes, and
/// a panel of words takes a wheel to what fell out of the slot.
#[test]
fn audress_every_panel_has_a_scroll_policy() {
    use crate::workspace::PaneScroll::{Across, Pans, Rows, Sheet};
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    window(&mut h, 1600.0, 900.0, 1.0);
    h.editor_mut().panel_more = all_open();
    settle(&mut h);
    for pane in Pane::ALL.iter().copied() {
        let want = match pane {
            Pane::Canvas | Pane::NodeGraph | Pane::Viewer(_) | Pane::Curves | Pane::View3D => Pans,
            Pane::XSheet => Sheet,
            Pane::Episode => Across,
            _ => Rows,
        };
        assert_eq!(pane.scroll_policy(), want, "{pane:?}");
        if !shown_in_a_slot(pane) {
            continue;
        }
        let min = pane.min_size();
        // The same width both times: a wheel goes down, and what it finds
        // stands in the columns the slot shows.
        let whole = slot(&mut h, pane, vec2(min.x * 0.7, min.y + 80.0));
        // The sheet scrolls its rows in a view of its own: its last is the
        // shot's last frame.
        let last = match pane {
            Pane::XSheet => h.state().frame_count().to_string(),
            _ => lowest_word(&h, whole).unwrap_or_else(|| panic!("{pane:?} says something")),
        };
        let small = slot(&mut h, pane, vec2(min.x * 0.7, min.y * 0.6));
        let views = plate::viewports(&h.ctx());
        let both = views.iter().any(|v| v.axes == [true, true] && v.rect.intersects(small));
        assert!(both, "{pane:?} scrolls at its minimum in {small:?}: {views:?}");
        if want != Pans {
            assert!(wheel_to(&mut h, &last, small), "{pane:?}: a wheel reaches {last:?} in {small:?}\n{}", h.describe());
        }
    }
}

/// Every panel at its minimum and just above it, folds shut and open, on all
/// three documents: every word stands whole, or in a scroll that shows.
#[test]
fn audress_every_panel_fits_at_its_minimum() {
    let mut found = Vec::new();
    for (name, state) in docs() {
        let mut h = EditorHarness::with_state_shipped(state);
        window(&mut h, 1600.0, 900.0, 1.0);
        for pane in Pane::ALL.iter().copied().filter(|p| shown_in_a_slot(*p)) {
            for (open, more) in [(false, Default::default()), (true, all_open())] {
                h.editor_mut().panel_more = more;
                for above in [0.0, 15.0] {
                    let at = slot(&mut h, pane, pane.min_size() + vec2(above, above));
                    let clip = measure::clip_here(&h, Scope::All);
                    let here = format!("{name} {pane:?} at {at:?}, open {open}");
                    found.extend(clip.found.into_iter().map(|f| format!("{here}: {f}")));
                }
            }
        }
    }
    assert!(found.is_empty(), "{} findings:\n{}", found.len(), found.join("\n"));
}

/// A drawing of eight layers, the most one can hold.
fn eight_layers() -> AppState {
    let mut state = sample::build_sample();
    while state.strip_layers().len() < 8 {
        let was = state.strip_layers().len();
        state.layer_add_role(anim_core::model::LayerRole::Custom);
        assert!(state.strip_layers().len() > was, "the sample's frame takes layers");
    }
    state
}

/// The wheel takes a person to the last row and to `Delete layer`.
fn layers_reach(h: &mut EditorHarness, inside: egui::Rect) {
    let names: Vec<String> = h.state().strip_layers().iter().map(|(_, l)| l.props.name.clone()).collect();
    let last = names.last().expect("eight of them");
    assert!(wheel_to(h, last, inside), "a wheel reaches {last:?} in {inside:?}\n{}", h.describe());
    assert!(wheel_to(h, "Delete layer", inside), "and the foot in {inside:?}\n{}", h.describe());
}

/// Eight layers in the rooms that dock them, and in a slot of 160: nothing
/// falls out of reach, and at the panel's minimum the foot stands while the
/// rows scroll. Blocker of the part's review.
#[test]
fn audress_docked_layers_reach_every_row() {
    for stage in [Stage::Edit, Stage::Manga] {
        for (w, tall) in SIZES {
            let mut h = EditorHarness::with_state_shipped(eight_layers());
            window(&mut h, w, tall, 1.0);
            ProbeRoom::Stage(stage).enter(h.editor_mut());
            settle(&mut h);
            let inside = body(&h, Pane::Layers);
            layers_reach(&mut h, inside);
        }
    }
    let mut h = EditorHarness::with_state_shipped(eight_layers());
    window(&mut h, 1600.0, 900.0, 1.0);
    let inside = slot(&mut h, Pane::Layers, vec2(240.0, 160.0));
    assert!(inside.contains_rect(h.rect("Delete layer")), "the foot stands in {inside:?}\n{}", h.describe());
    layers_reach(&mut h, inside);
    assert!(inside.contains_rect(h.rect("Delete layer")), "and stood still");
}

/// A fold stays as it was left, panel by panel, and a person's start closed.
/// One panel of each kind of fold: a head tag, a row, the Export panel's own,
/// Colors' and the float's.
#[test]
fn audress_more_is_remembered_per_panel() {
    let mut h = alone(sample::build_sample(), Pane::PaintCheck);
    assert!(!h.state().panel_more.is_open(plate::MORE_CHECK), "a person starts closed");
    assert!(!h.has("in this shot") && !h.has("this shot"), "what it looks for folds");
    h.click(plate::more_name(plate::MORE_CHECK));
    settle(&mut h);
    assert!(h.state().panel_more.is_open(plate::MORE_CHECK));
    assert_eq!(h.rig().config.ui.panel_more.get(plate::MORE_CHECK), Some(&true), "into the config");
    assert_eq!(h.rig().config.ui.panel_more.get(plate::MORE_LAYERS), None, "that panel's alone");
    assert!(!h.state().panel_more.is_open(plate::MORE_LAYERS), "that panel's alone");
    assert!(EditorHarness::new().state().panel_more.is_open(plate::MORE_LAYERS), "the frozen boot opens all");

    for (pane, name, key) in [
        (Pane::Layers, plate::more_name(plate::MORE_LAYERS), plate::MORE_LAYERS),
        (Pane::NodeGraph, plate::more_name(plate::MORE_GRAPH), plate::MORE_GRAPH),
        (Pane::RenderQueue, plate::more_name(plate::MORE_EXPORT), plate::MORE_EXPORT),
        (Pane::Palette, vocab::COLOR_DETAILS, plate::MORE_COLORS),
        (Pane::XSheet, plate::more_name(plate::MORE_SHEET), plate::MORE_SHEET),
        (Pane::Canvas, plate::more_name(plate::MORE_BRUSHES), plate::MORE_BRUSHES),
    ] {
        let mut h = alone(sample::build_sample(), pane);
        if pane == Pane::Canvas {
            h.open_float(crate::floats::Float::Brushes);
            settle(&mut h);
        }
        h.scroll_to(name);
        h.click(name);
        settle(&mut h);
        assert_eq!(h.rig().config.ui.panel_more.get(key), Some(&true), "{pane:?} into the config");
    }
}

/// The map comes back through a real boot, from the file, and the panel reads
/// it: a person's folds survive the quit.
#[test]
fn audress_more_survives_a_restart() {
    let mut first = EditorHarness::as_animation();
    settle(&mut first);
    first.editor_mut().dock = egui_dock::DockState::new(vec![Pane::PaintCheck]);
    settle(&mut first);
    first.click(plate::more_name(plate::MORE_CHECK));
    settle(&mut first);
    let mut h = EditorHarness::with_dir_animation(first.quit());
    settle(&mut h);
    assert!(h.state().panel_more.is_open(plate::MORE_CHECK), "the fold came back open");
    assert!(!h.state().panel_more.is_open(plate::MORE_LAYERS), "and only that one");
}

/// The fold the Export panel keeps of its own goes with the document it was
/// opened on no longer: the shared map answers for a later one.
#[test]
fn audress_export_fold_outlives_its_document() {
    let mut h = alone(sample::build_sample(), Pane::RenderQueue);
    h.click(plate::more_name(plate::MORE_EXPORT));
    settle(&mut h);
    assert!(h.has(vocab::EXPORT_QUEUE), "open\n{}", h.describe());
    *h.state_mut() = sample::build_sample();
    settle(&mut h);
    h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::RenderQueue]);
    settle(&mut h);
    assert!(h.has(vocab::EXPORT_QUEUE), "still open on the next document\n{}", h.describe());
}

/// A leaf saved collapsed comes back open: its corner buttons are gone.
#[test]
fn audress_leaf_saved_collapsed_opens() {
    use crate::workspace::{Workspace, Workspaces};
    let mut dock = egui_dock::DockState::new(vec![Pane::Canvas]);
    dock.main_surface_mut().split_right(NodeIndex::root(), 0.7, vec![Pane::XSheet]);
    for (_, node) in dock.iter_all_nodes_mut() {
        if let egui_dock::Node::Leaf(leaf) = node {
            leaf.collapsed = true;
        }
    }
    let saved = Workspace { name: "folded".into(), dock, preset: None, view: None };
    let collapsed = |w: &Workspace| w.dock.iter_all_nodes().any(|(_, n)| matches!(n, egui_dock::Node::Leaf(l) if l.collapsed));
    assert!(collapsed(&saved), "the fixture stands collapsed");
    assert!(!collapsed(&saved.normalised().expect("normal")), "every save path");
    let dir = tempfile::tempdir().expect("a dir");
    let text = serde_json::to_string(&Workspaces { list: vec![saved] }).expect("written");
    std::fs::write(Workspaces::path_in(dir.path()), text).expect("written");
    let (back, notes) = Workspaces::load_from(dir.path());
    assert!(notes.is_empty(), "{notes:?}");
    assert!(!collapsed(&back.list[0]), "and a file from before");
}

/// Spec 7.1: the Timing sheet's head is one line, and `Details` holds the rest.
#[test]
fn audress_sheet_header_folds_to_one_line() {
    let mut h = alone(sample::build_sample(), Pane::XSheet);
    assert!(!h.has(vocab::LENGTH) && !h.has(vocab::NOTES), "folded");
    let top = h.rect(plate::more_name(plate::MORE_SHEET)).top();
    let body = plate::pane_bodies(&h.ctx())[0];
    assert!(top - body.top() <= 44.0, "one line: {top} under {body:?}");
    h.click(plate::more_name(plate::MORE_SHEET));
    settle(&mut h);
    assert!(h.has(vocab::LENGTH) && h.has(vocab::NOTES), "and the rest under it");
}

/// Spec 7.1: at Plan's 1600×900 the sheet's chrome falls from 178 to about 90
/// and about 37 rows stand in sight.
#[test]
fn audress_sheet_gives_its_chrome_to_rows() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    window(&mut h, 1600.0, 900.0, 1.0);
    settle(&mut h);
    assert!(h.editor().in_plan(), "{:?}", h.editor().stage);
    let cell = |h: &EditorHarness, frame| h.state().sheet_ui.cell_center(0, frame).expect("the sheet drew its rows");
    let row_h = cell(&h, 1).y - cell(&h, 0).y;
    let view = plate::viewports(&h.ctx()).into_iter().rev().find(|v| v.rect.contains(cell(&h, 0))).expect("its scroll view");
    let (rows, pane) = (view.rect.height() / row_h, body(&h, Pane::XSheet));
    let chrome = cell(&h, 0).y - row_h / 2.0 - (pane.top() - plate::TAB_BAR_H);
    println!("Plan sheet at 1600×900: {rows:.1} rows, {chrome:.0} pt of chrome");
    assert!(rows >= 37.0, "{rows} rows");
    assert!(chrome <= 100.0, "{chrome} pt of chrome over the rows");
}

/// The refusal names where its latch stands, and a row says it is locked while
/// the lock itself is folded.
#[test]
fn audress_folded_layers_say_what_stands_in_the_way() {
    use anim_core::model::LayerProps;
    let mut h = alone(sample::build_sample(), Pane::Layers);
    let (id, props): (_, LayerProps) = {
        let layers = h.state().strip_layers();
        let line = layers.iter().find(|(_, l)| l.props.role.is_line()).expect("a line layer");
        (line.1.id, line.1.props.clone())
    };
    h.state_mut().view.line_guard = true;
    h.state_mut().layer_set_props(id, LayerProps { locked: false, ..props.clone() });
    settle(&mut h);
    let before = h.kit().output().shapes.len();
    h.state_mut().layer_set_props(id, LayerProps { locked: true, ..props.clone() });
    settle(&mut h);
    assert!(h.kit().output().shapes.len() > before, "the lock's mark stands on the row");
    h.click(&props.name);
    settle(&mut h);
    assert_eq!(h.state().refusal, crate::xsheet_panel::LINE_GUARD_REFUSAL, "the refusal says where");
}

/// Outside the Effects step the graph speaks the shelf's words, and its
/// graph-wide verbs fold under the panel's `More`.
#[test]
fn audress_graph_is_dressed_in_every_step() {
    use crate::nodegraph_panel::PICK_A_BOX;
    let mut h = alone(sample::build_sample(), Pane::NodeGraph);
    h.state_mut().selected_node = None;
    settle(&mut h);
    let full = Action::BuildSatsuei.label();
    assert!(!h.has(full), "folded\n{}", h.describe());
    assert!(h.has(PICK_A_BOX) || h.painted_text().iter().any(|t| t == PICK_A_BOX), "{:?}", h.painted_text());
    assert!(!h.painted_text().iter().any(|t| t.contains("no node selected")), "{:?}", h.painted_text());
    h.click(plate::more_name(plate::MORE_GRAPH));
    settle(&mut h);
    assert!(h.has(full), "open\n{}", h.describe());
}

/// Spec AU15 item 6: in each panel the control a person came for leads, ahead
/// of its fold.
#[test]
fn audress_most_used_stands_first() {
    let h = alone(sample::build_sample(), Pane::PaintCheck);
    let leads = |h: &EditorHarness, lead: &str, key| h.rect(lead).top() <= h.rect(plate::more_name(key)).top();
    assert!(leads(&h, "Check paint", plate::MORE_CHECK), "{}", h.describe());
    let h = alone(templates::READY.build(), Pane::Board);
    assert!(leads(&h, "Add shot to this scene", plate::MORE_SHOTS), "{}", h.describe());
    let h = alone(sample::build_sample(), Pane::Manga);
    assert!(leads(&h, "New page", plate::MORE_MANGA), "{}", h.describe());
    let h = alone(sample::build_sample(), Pane::Layers);
    assert!(leads(&h, "+", plate::MORE_LAYERS), "{}", h.describe());
}

/// The right-click verbs: a registry row each, a place to find it, and the
/// same edit as the menu it twins, undone in one step.
#[test]
fn audress_context_twins_exist() {
    const TWINS: [(&str, Action); 8] = [
        ("board_panel card_menu", Action::ExportCutAsProject),
        ("nodegraph_panel body", Action::SetGraphOutput),
        ("nodegraph_panel body", Action::RemoveNode),
        ("xsheet_panel library", Action::RemoveDrawing),
        ("xsheet_panel sound head", Action::RemoveSoundTrack),
        ("xsheet_panel cell_menu", Action::UsePanelOrder),
        ("xsheet_panel cell_menu", Action::ResolveRetakeNote),
        ("xsheet_panel layer_card", Action::ToggleLayerLocked),
    ];
    let reach = cmdpal::ranked("", &[]);
    assert_eq!(reach.len(), Action::ALL.len(), "palette reach is the registry");
    for (site, action) in TWINS.into_iter().chain([("curves_panel", Action::InterpEase)]) {
        assert!(action.menu().is_some() && reach.contains(&action), "{site}: {action:?}");
        let help = action.help();
        assert!((1..=120).contains(&help.chars().count()) && help.ends_with('.'), "{action:?}: {help}");
    }

    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    let graph = |h: &EditorHarness| h.state().cut().graph.nodes().count();
    let node = h.state().cut().graph.nodes().map(|n| n.id).find(|id| Some(*id) != h.state().cut().graph.output);
    let node = node.expect("the sample has a graph");
    h.state_mut().selected_node = None;
    assert!(h.editor().can_perform(Action::SetGraphOutput).is_err(), "nothing selected");
    h.state_mut().selected_node = Some(node);
    let was = h.state().cut().graph.output;
    assert_eq!(h.perform(Action::SetGraphOutput), Performed::Done);
    assert_eq!(h.state().cut().graph.output, Some(node));
    assert_eq!(h.perform(Action::Undo), Performed::Done);
    assert_eq!(h.state().cut().graph.output, was, "one step");
    let nodes = graph(&h);
    assert_eq!(h.perform(Action::RemoveNode), Performed::Done);
    assert_eq!(graph(&h), nodes - 1);
    assert_eq!(h.perform(Action::Undo), Performed::Done);
    assert_eq!(graph(&h), nodes, "one step");

    let drawing = h.state().current_drawing().expect("the sample's frame is drawn");
    assert_eq!(h.perform(Action::RemoveDrawing), Performed::Done);
    assert!(h.state().cut().drawing(drawing).is_none());
    assert_eq!(h.perform(Action::Undo), Performed::Done);
    assert!(h.state().cut().drawing(drawing).is_some(), "one step");

    let locked = h.state().active_layer_props().expect("a layer").locked;
    assert_eq!(h.perform(Action::ToggleLayerLocked), Performed::Done);
    assert_eq!(h.state().active_layer_props().expect("a layer").locked, !locked);
    assert_eq!(h.editor().is_on(Action::ToggleLayerLocked), Some(!locked));
    assert_eq!(h.perform(Action::Undo), Performed::Done);

    let (at, frame) = (h.state().at(), h.state().view.frame);
    assert!(h.editor().can_perform(Action::ResolveRetakeNote).is_err(), "no note yet");
    assert!(h.state_mut().add_retake_note(at, frame, "fix the arm", false));
    assert_eq!(h.perform(Action::ResolveRetakeNote), Performed::Done);
    assert!(anim_core::board::notes_of(h.state().cut())[0].resolved);
    assert!(h.editor().can_perform(Action::ResolveRetakeNote).is_err(), "none open now");

    let mut s = AppState::new_project("t", 320, 240, 24, 300.0);
    let at = s.at();
    let board = s.engine.add_column(at, "BOARD").expect("the track");
    s.view.active_column = board;
    s.set_column_kind(ColumnKind::Ref);
    s.goto(0);
    s.new_drawing_at_frame();
    let panel = s.current_drawing().expect("the panel");
    s.set_panel_cut(panel, Some(7));
    let audio = crate::playback::tests::clip_of(200);
    s.engine.apply("set audio", vec![Command::SetCutAudio { at, audio: Some(audio) }]).expect("the clip");
    let mut h = EditorHarness::with_state_shipped(s);
    assert_eq!(h.perform(Action::UsePanelOrder), Performed::Done);
    assert_eq!(h.state().cut().drawing_def(panel).cut, None);
    assert!(h.editor().can_perform(Action::UsePanelOrder).is_err(), "its order already");
    assert_eq!(h.perform(Action::RemoveSoundTrack), Performed::Done);
    assert_eq!(anim_core::audio::tracks_iter(h.state().cut()).count(), 0);
    assert!(h.editor().can_perform(Action::RemoveSoundTrack).is_err(), "no sound now");
}

/// Spec 2.6 and 2.8: tabs are full stadiums, the current one in the wash, and
/// the bar insets them so the pill's ends stand clear of the body.
#[test]
fn audress_tab_bar_uses_the_look() {
    let dock = plate::dock_style(&egui::Style::default());
    let stadium = egui::CornerRadius::same(plate::TAB_RADIUS);
    for tab in [&dock.tab.active, &dock.tab.focused, &dock.tab.active_with_kb_focus, &dock.tab.focused_with_kb_focus] {
        assert_eq!(tab.bg_fill, plate::BLUE_WASH);
        assert_eq!(tab.text_color, plate::BLUE_DEEP);
        assert_eq!(tab.corner_radius, stadium);
    }
    assert_eq!(dock.tab.inactive.corner_radius, stadium, "every tab is a pill");
    assert_eq!(dock.tab.hovered.bg_fill, plate::WELL);
    assert_eq!(dock.tab.inactive.bg_fill, egui::Color32::TRANSPARENT, "only the current tab stands on a ground");
    let tall = plate::TAB_BAR_H - 2.0 * f32::from(plate::TAB_INSET);
    assert!(f32::from(plate::TAB_RADIUS) * 2.0 >= tall, "a full stadium: radius {} on {tall}", plate::TAB_RADIUS);
}

fn budget(state: AppState, name: &str, ceiling: usize) {
    let mut h = EditorHarness::with_state_shipped(state);
    window(&mut h, 1600.0, 900.0, 1.0);
    let mut read = Vec::new();
    for room in PROBE_ROOMS {
        room.enter(h.editor_mut());
        settle(&mut h);
        read.push((room.name(), measure::census(&mut h).controls));
    }
    println!("{name}: {read:?}");
    assert!(read.iter().all(|(_, n)| *n <= ceiling), "{name} over {ceiling}: {read:?}");
}

#[test]
fn audress_crowded_rooms_hold_the_budget() {
    budget(templates::READY.build(), "blank drawing", 45);
    budget(NewProjectForm::default().new_state(), "blank animation", 45);
}

#[test]
fn audress_sample_holds_the_budget() {
    budget(sample::build_sample(), "sample", 80);
}

/// Spec 8.4's three left: the Export queue, Manga pages and Shots.
#[test]
fn audress_empty_states_are_complete() {
    let mut h = EditorHarness::with_state(NewProjectForm::default().new_state());
    window(&mut h, 1600.0, 900.0, 1.0);
    h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::RenderQueue]);
    settle(&mut h);
    assert!(h.has(render_queue::NOTHING_WAITING), "the queue\n{}", h.describe());

    let h = alone(NewProjectForm::default().new_state(), Pane::Manga);
    assert!(h.has(manga_tools::NO_PAGES) && h.has("New page"), "the book\n{}", h.describe());

    let mut h = alone(NewProjectForm::default().new_state(), Pane::Board);
    assert!(h.has(board_panel::ONE_SHOT), "one shot\n{}", h.describe());
    let scene = h.state().at().scene;
    let shots = |h: &EditorHarness| h.state().engine.project.scene(scene).map_or(0, |s| s.cuts.len());
    assert_eq!(shots(&h), 1);
    h.click("Add shot to this scene");
    settle(&mut h);
    assert_eq!(shots(&h), 2, "the button adds it");
    assert!(!h.has(board_panel::ONE_SHOT), "the sentence goes with it");
}

/// Every word and control of every room stands whole in its panel, or in a
/// scroll that reaches it: 4 sizes × 3 scales, on all three documents, with
/// the folds shut and again with every one of them open.
#[test]
fn audress_all_panels_clip_audit_passes() {
    let mut found = Vec::new();
    for (name, state) in docs() {
        let mut h = EditorHarness::with_state_shipped(state);
        for (open, more) in [(false, Default::default()), (true, all_open())] {
            h.editor_mut().panel_more = more;
            for (w, tall) in SIZES {
                for stored in PROBE_SCALES {
                    window(&mut h, w, tall, stored);
                    // A clipped word, a control off the window and an elided
                    // word are each one line of `found`.
                    let clip = measure::clip_audit(&mut h, Scope::All);
                    let here = format!("{name} {w}×{tall} at {stored}, open {open}");
                    found.extend(clip.found.into_iter().map(|f| format!("{here}: {f}")));
                }
            }
        }
    }
    assert!(found.is_empty(), "{} findings:\n{}", found.len(), found.join("\n"));
}

/// Each room of the sample cut against its `(room, shapes, vertices)`, and
/// every room against the baseline's slowest tessellation.
fn over(rooms: [(Stage, usize, usize); 6]) -> Vec<String> {
    /// The slowest room of the baseline, in the test profile.
    const TESSELLATE_US: u128 = 407;
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    window(&mut h, 1600.0, 900.0, 1.0);
    let mut over = Vec::new();
    for (stage, most_shapes, most_verts) in rooms {
        ProbeRoom::Stage(stage).enter(h.editor_mut());
        settle(&mut h);
        let (shapes, verts, micros) = cost(&h);
        println!("{stage:?} {shapes} shapes, {verts} vertices, {micros} us");
        if shapes > most_shapes || verts > most_verts || micros > TESSELLATE_US {
            over.push(format!("{stage:?} {shapes}/{most_shapes} shapes, {verts}/{most_verts} vertices, {micros} us"));
        }
    }
    over
}

/// The spec's own target, the egui baseline of 2026-10 room by room.
#[test]
fn audress_shapes_and_vertices_hold() {
    let over = over([
        (Stage::Layout, 400, 5154),
        (Stage::Drawing, 551, 6816),
        (Stage::InkPaint, 615, 6946),
        (Stage::Composite, 298, 6060),
        (Stage::Edit, 555, 9124),
        (Stage::Manga, 431, 7586),
    ]);
    assert!(over.is_empty(), "over the baseline: {over:?}");
}

/// One frame's shapes, its vertices, and the best of 40 tessellations: the
/// suite runs beside this on every core, and a burst lasts longer than five.
fn cost(h: &EditorHarness) -> (usize, usize, u128) {
    let ctx = h.ctx();
    let shapes = h.kit().output().shapes.clone();
    let mut best = u128::MAX;
    let mut verts = 0;
    for _ in 0..40 {
        let at = std::time::Instant::now();
        let meshes = ctx.tessellate(shapes.clone(), ctx.pixels_per_point());
        best = best.min(at.elapsed().as_micros());
        verts = meshes
            .iter()
            .map(|c| match &c.primitive {
                egui::epaint::Primitive::Mesh(m) => m.vertices.len(),
                egui::epaint::Primitive::Callback(_) => 0,
            })
            .sum();
    }
    (shapes.len(), verts, best)
}

/// Feedback 4.3's tightest room for Plan: an effective 832×520, alone and
/// beside the lesson card. The left column keeps the sheet's 240.
#[test]
fn audress_plan_keeps_its_sheet_column() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    window(&mut h, 832.0, 520.0, 1.0);
    settle(&mut h);
    assert!(h.editor().in_plan(), "{:?}", h.editor().stage);
    let alone = body(&h, Pane::XSheet);
    assert!(alone.width() >= Pane::XSheet.min_size().x, "{alone:?}");
    crate::aulesson_tests::open(&mut h, "probe-film");
    ProbeRoom::Stage(Stage::Layout).enter(h.editor_mut());
    settle(&mut h);
    let beside = body(&h, Pane::XSheet);
    assert!(beside.width() >= Pane::XSheet.min_size().x, "beside the card: {beside:?}");
}

/// Spec 7.1's Effects row: the graph large and in front. A person's lower
/// leaf folds behind the Result to hold spec 10's ceiling, every pane still
/// a tab away; the frozen boot keeps the recipe it always had.
#[test]
fn audress_effects_stands_the_graph_in_front() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    window(&mut h, 1600.0, 900.0, 1.0);
    ProbeRoom::Stage(Stage::Composite).enter(h.editor_mut());
    settle(&mut h);
    assert!(body(&h, Pane::NodeGraph).width() >= 800.0, "large: {:?}", body(&h, Pane::NodeGraph));
    let dock = &h.editor().dock;
    for pane in [Pane::Viewer(0), Pane::Canvas, Pane::Palette, Pane::RenderQueue, Pane::Curves] {
        assert!(dock.find_tab(&pane).is_some(), "{pane:?} a tab away");
    }
    let (ours, today) = (Stage::dock_effects(), Stage::Composite.dock());
    assert_eq!(ours.iter_all_tabs().count(), today.iter_all_tabs().count(), "the same panes in both");
    assert_ne!(ours.main_surface().num_tabs(), 0);
    assert!(ours.main_surface().iter().filter(|n| n.is_leaf()).count() < today.main_surface().iter().filter(|n| n.is_leaf()).count());
}

/// A pin wires by the pointer alone, and holds no Tab stop it could not use:
/// the two hard gates of spec 10 hold on that.
#[test]
fn audress_graph_pins_are_no_tab_stops() {
    let h = alone(sample::build_sample(), Pane::NodeGraph);
    let pins = h.ctx().viewport(|v| {
        let mut pins = Vec::new();
        for (_, rects) in v.prev_pass.widgets.layers() {
            pins.extend(rects.iter().filter(|r| r.sense.senses_drag() && r.rect.size() == vec2(16.0, 16.0)).map(|r| r.sense));
        }
        pins
    });
    assert!(!pins.is_empty(), "the sample's graph shows its pins");
    assert!(pins.iter().all(|s| !s.is_focusable() && s.senses_click()), "{pins:?}");
}
