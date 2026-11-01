//! AU14's tests: a glow is one tap, a camera move is two framings, and no
//! wire is drawn for either.

use anim_core::camera::{CAMERA_SPECS, CameraState, output_camera};
use anim_core::deliver::{self, Purpose, RenderOpts};
use anim_core::fx::{FxKind, FxValue};
use anim_core::generator::GenKind;
use anim_core::graph::{BlendMode, NodeKind};
use anim_core::ids::{NodeId, ParamId};
use anim_core::xsheet::ParamInterp;
use eframe::egui;
use egui::{Key, Pos2, Rect, vec2};

use crate::canvas::extpaper_tests::paint_texels;
use crate::config::Action;
use crate::doc::{AppState, CAMERA_ADDED, CAMERA_NO_TRACKS, CAMERA_START_SET, GLOW_NEEDS_COLOR, NO_OUTPUT_FOR_EFFECTS};
use crate::fxui::strength_keys;
use crate::measure::{self, Act, Recorder};
use crate::nodegraph_panel::{FULL_SETUP_WARNS, GRAPH_SET_UP, PICK_A_BOX, SHELF_MORE};
use crate::lessons::Target;
use crate::plate;
use crate::steps::ProjectKind;
use crate::testkit::EditorHarness;
use crate::viewer::{FRAME_BOX_SAYS, Framing, NOTHING_TO_SHOW, SET_UP_COMPOSITE, camera_done};
use crate::workspace::{Pane, Stage};
use crate::{auguide_tests, templates, vocab};

const SIZES: [(f32, f32); 3] = [(1600.0, 900.0), (1280.0, 800.0), (832.0, 600.0)];
const PICK_AN_EFFECT: &str = "Pick an effect to add it to the picture.";

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

fn sized(h: &mut EditorHarness, w: f32, tall: f32) {
    h.kit_mut().set_size(vec2(w, tall));
    settle(h);
}

fn enter(h: &mut EditorHarness, stage: Stage) {
    h.editor_mut().enter_room(stage);
    settle(h);
}

fn pane_rect(h: &EditorHarness, pane: Pane) -> Rect {
    let rects = &h.editor().pane_rects;
    rects.iter().find(|(p, ..)| *p == pane).map(|(_, r, _)| *r).unwrap_or_else(|| panic!("{pane:?}: {rects:?}"))
}

fn says(h: &EditorHarness, words: &str) -> bool {
    h.painted_text().iter().any(|w| w == words)
}

/// The Effects step of a new animation with a drawing on it.
fn film_room(w: f32, tall: f32) -> EditorHarness {
    let mut s = auguide_tests::film().build();
    paint_texels(&mut s, &[(40, 40), (41, 40)], [0.1, 0.1, 0.1, 1.0]);
    let mut h = EditorHarness::with_state_shipped(s);
    sized(&mut h, w, tall);
    enter(&mut h, Stage::Composite);
    h
}

/// A cut with one drawing, its graph still empty.
fn one_drawing() -> AppState {
    let mut s = AppState::new_project("t", 640, 360, 24, 72.0);
    s.set_cut_length(12);
    s.new_drawing_at_frame();
    s
}

/// [`one_drawing`] with the sheet's composite built.
fn composed() -> AppState {
    let mut s = one_drawing();
    assert!(s.auto_comp());
    s
}

fn output(s: &AppState) -> NodeId {
    s.cut().graph.output.expect("an Output")
}

fn feed_of(s: &AppState, node: NodeId) -> Option<(NodeId, u16)> {
    s.cut().graph.node(node).expect("the node").inputs.first().copied().flatten()
}

fn nodes(s: &AppState) -> usize {
    s.cut().graph.nodes().count()
}

fn glow() -> NodeKind {
    NodeKind::effect(FxKind::Glow)
}

fn float(s: &AppState, node: NodeId, key: &str) -> f32 {
    let NodeKind::Effect { params, .. } = &s.cut().graph.node(node).expect("the node").kind else {
        panic!("not an effect");
    };
    match params.get(key) {
        Some(FxValue::Float(v)) => *v,
        other => panic!("{key}: {other:?}"),
    }
}

/// The three tracks Camera move bound on `cam`.
fn tracks(s: &AppState, cam: NodeId) -> [ParamId; 3] {
    let NodeKind::Camera { binds, .. } = &s.cut().graph.node(cam).expect("the camera").kind else {
        panic!("not a camera");
    };
    ["pan_x", "pan_y", "zoom"].map(|k| binds.get(k).unwrap_or_else(|| panic!("{k} is not bound")))
}

fn key(s: &AppState, column: ParamId, frame: u32) -> Option<anim_core::xsheet::ParamKey> {
    s.cut().xsheet.param(column).and_then(|p| p.key_at(frame))
}

/// Does a lesson's ring stand on this handle, on the Effects step?
fn stands(h: &mut EditorHarness, handle: &'static [Target]) -> bool {
    auguide_tests::stand(h, handle, Stage::Composite);
    plate::ring_stands(&h.ctx()).is_some()
}

const GLOW_MARK: &[Target] = &[Target::Named("shelf.glow")];
const CAMERA_MARK: &[Target] = &[Target::Named("shelf.camera")];
const ADD_MARK: &[Target] = &[Target::Named("shelf.add")];
const STRENGTH_MARK: &[Target] = &[Target::Named("shelf.strength")];

// ---- Wiring ----

#[test]
fn aufx_insert_on_output_with_a_feed() {
    let mut s = composed();
    let out = output(&s);
    let feed = feed_of(&s, out).expect("the composite feeds its Output");
    let added = s.shelf_add(glow()).expect("added");
    assert_eq!(feed_of(&s, added), Some(feed), "the old feed goes to input 0");
    assert_eq!(feed_of(&s, out), Some((added, 0)), "and the new node feeds the Output");
    assert_eq!(s.selected_node, Some(added), "Strength reads it");
    // A second one stands after the first: the feed keeps its output pin.
    let next = s.shelf_add(NodeKind::effect(FxKind::Blur)).expect("added");
    assert_eq!(feed_of(&s, next), Some((added, 0)));
    assert_eq!(feed_of(&s, out), Some((next, 0)));
}

#[test]
fn aufx_insert_on_a_bare_output() {
    let mut s = one_drawing();
    s.graph_add_node(NodeKind::Output, (0.0, 0.0));
    let out = s.selected_node.expect("the Output");
    s.graph_set_output(out);
    let added = s.shelf_add(glow()).expect("added");
    assert_eq!(feed_of(&s, out), Some((added, 0)));
    assert_eq!(feed_of(&s, added), None, "nothing fed the Output");
    assert_eq!(nodes(&s), 2);
}

#[test]
fn aufx_insert_builds_the_composite_from_no_graph() {
    let mut s = one_drawing();
    let bare = composed();
    let auto = nodes(&bare);
    let auto_feed = bare.cut().graph.node(output(&bare)).ok().and_then(|n| n.inputs[0]).is_some();
    assert!(auto_feed, "the sheet's composite ends in a feed");
    let added = s.shelf_add(glow()).expect("added");
    let out = output(&s);
    assert_eq!(nodes(&s), auto + 1, "the auto composite, then the effect");
    assert_eq!(feed_of(&s, out), Some((added, 0)), "the effect feeds the Output");
    let source = feed_of(&s, added).expect("the composite feeds the effect").0;
    assert!(matches!(s.cut().graph.node(source).unwrap().kind, NodeKind::DrawingSource { .. }), "the first row");
}

/// A hand-built graph has no place to put it, and the shelf says what to do.
#[test]
fn aufx_no_output_disables_with_the_reason() {
    let mut s = one_drawing();
    s.graph_add_node(NodeKind::Solid { rgba: [255, 255, 255, 255] }, (0.0, 0.0));
    let before = (nodes(&s), s.engine.history_len());
    assert_eq!(s.shelf_refusal(), Some(NO_OUTPUT_FOR_EFFECTS));
    assert_eq!(s.shelf_add(glow()), None);
    assert!(!s.camera_move());
    assert_eq!((nodes(&s), s.engine.history_len()), before, "left as it is");

    let mut h = EditorHarness::with_state_shipped(s);
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h, Stage::Composite);
    for word in ["Glow", "Soft focus", "Blur", "Color look", vocab::CAMERA_MOVE, vocab::ADD_EFFECT] {
        assert_eq!(h.node_enabled(word), Some(false), "{word}");
    }
    assert!(says(&h, NO_OUTPUT_FOR_EFFECTS), "{:?}", h.painted_text());
}

#[test]
fn aufx_insert_is_one_undo() {
    for mut s in [one_drawing(), composed()] {
        let (before, history) = (s.cut().graph.clone(), s.engine.history_len());
        let meta = s.cut().meta.clone();
        s.shelf_add(glow()).expect("added");
        assert_eq!(s.engine.history_len(), history + 1, "one step");
        assert!(s.engine.undo().is_ok());
        assert_eq!(s.cut().graph.nodes().count(), before.nodes().count());
        assert_eq!(s.cut().graph.output, before.output, "the Output as it stood");
        assert_eq!(s.cut().meta, meta, "and its place");
    }
}

/// Generators have no picture input: they lay over the feed, not in series.
#[test]
fn aufx_generator_blends_over_the_output() {
    let mut s = composed();
    let out = output(&s);
    let feed = feed_of(&s, out).expect("a feed");
    let history = s.engine.history_len();
    let added = s.shelf_add(NodeKind::generator(GenKind::Text)).expect("added");
    assert_eq!(s.engine.history_len(), history + 1, "one step");
    let (blend, _) = feed_of(&s, out).expect("the Output's feed");
    let node = s.cut().graph.node(blend).unwrap();
    assert_eq!(node.kind, NodeKind::Blend { mode: BlendMode::Normal });
    assert_eq!(node.inputs[0], Some(feed), "the picture under it");
    assert_eq!(node.inputs[1], Some((added, 0)), "the generator over it");

    // Nothing to lay it over: it is the picture.
    let mut s = one_drawing();
    s.graph_add_node(NodeKind::Output, (0.0, 0.0));
    let out = s.selected_node.unwrap();
    s.graph_set_output(out);
    let added = s.shelf_add(NodeKind::generator(GenKind::Text)).expect("added");
    assert_eq!(feed_of(&s, out), Some((added, 0)));
}

/// Web 720p's graph: the auto composite's six, then the Glow, and no others.
#[test]
fn aufx_glow_on_web_720p_adds_one_node() {
    let template = templates::ALL.iter().find(|t| t.name == "Web 720p").expect("the template");
    let mut h = EditorHarness::with_state_shipped(template.build());
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h, Stage::Composite);
    if nodes(h.state()) == 0 {
        h.click("Glow");
    } else {
        assert_eq!(nodes(h.state()), 6, "the auto graph");
        h.click("Glow");
    }
    settle(&mut h);
    assert_eq!(nodes(h.state()), 7, "6 plus 1");
    assert_eq!(h.state().cut().graph.nodes().filter(|n| matches!(n.kind, NodeKind::Effect { .. })).count(), 1);
}

// ---- The picture ----

#[test]
fn aufx_glow_changes_the_output_at_once() {
    let mut s = composed();
    let dab: Vec<(i32, i32)> = (300..340).flat_map(|x| (160..200).map(move |y| (x, y))).collect();
    paint_texels(&mut s, &dab, [1.0, 1.0, 0.2, 1.0]);
    let opts = RenderOpts { purpose: Purpose::Final, scale: 1.0 };
    let render = |s: &AppState| {
        let (w, tall) = (s.engine.project.width, s.engine.project.height);
        deliver::render(s.cut(), s.engine.project.color_law, s.view.frame, w, tall, &opts)
    };
    let before = render(&s);
    s.shelf_add(glow()).expect("added");
    let after = render(&s);
    assert_eq!((before.w, before.h), (after.w, after.h));
    let at = |img: &anim_core::fx::Image, x: u32, y: u32| {
        let i = ((y * img.w + x) * 4) as usize;
        img.px[i..i + 3].to_vec()
    };
    // Just outside the dab, where only the halo reaches.
    let (near_before, near_after) = (at(&before, 345, 180), at(&after, 345, 180));
    let moved = near_before.iter().zip(&near_after).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(moved > 0.01, "the halo: {near_before:?} then {near_after:?}");
    assert_eq!(at(&before, 20, 20), at(&after, 20, 20), "far away nothing moved");
}

#[test]
fn aufx_strength_writes_one_parameter() {
    let mut h = film_room(1600.0, 900.0);
    assert!(!h.has(vocab::STRENGTH), "no effect yet, no slider");
    h.click("Glow");
    settle(&mut h);
    let id = h.state().selected_node.expect("the Glow is selected");
    let before = float(h.state(), id, "intensity");
    let others = |h: &EditorHarness| ["threshold", "radius"].map(|k| float(h.state(), id, k));
    let held = others(&h);
    let history = h.state().engine.history_len();
    let track = h.rect(vocab::STRENGTH);
    h.drag(track.left_center() + vec2(track.width() * 0.2, 0.0), track.left_center() + vec2(track.width() * 0.7, 0.0), 6);
    settle(&mut h);
    let after = float(h.state(), id, "intensity");
    assert!(after > before, "{before} then {after}");
    assert_eq!(others(&h), held, "one parameter");
    assert_eq!(h.state().engine.history_len(), history + 1, "one gesture, one undo");

    // Each shelf kind's one amount, from its own specs.
    for (word, key) in [("Soft focus", "mix"), ("Color look", "saturation")] {
        h.click(word);
        settle(&mut h);
        let id = h.state().selected_node.unwrap();
        let before = float(h.state(), id, key);
        let track = h.rect(vocab::STRENGTH);
        h.drag(track.left_center() + vec2(track.width() * 0.6, 0.0), track.left_center() + vec2(track.width() * 0.9, 0.0), 6);
        settle(&mut h);
        assert!(float(h.state(), id, key) > before, "{word}");
    }
    // Blur's stored pair moves as one, or it would streak sideways.
    h.click("Blur");
    settle(&mut h);
    let id = h.state().selected_node.unwrap();
    let track = h.rect(vocab::STRENGTH);
    h.drag(track.left_center() + vec2(track.width() * 0.3, 0.0), track.left_center() + vec2(track.width() * 0.8, 0.0), 6);
    settle(&mut h);
    let (x, y) = (float(h.state(), id, "radius_x"), float(h.state(), id, "radius_y"));
    assert!(x > 4.0 && x == y, "{x} and {y}");
}

// ---- Camera move ----

#[test]
fn aufx_camera_move_is_one_undo() {
    let mut s = composed();
    s.goto(3);
    let out = output(&s);
    let feed = feed_of(&s, out).expect("a feed");
    let (count, columns, history) = (nodes(&s), s.cut().xsheet.params.len(), s.engine.history_len());
    assert!(s.camera_move());
    assert_eq!(s.engine.history_len(), history + 1, "one step");
    let cam = output_camera(s.cut()).expect("the camera feeds the Output");
    assert_eq!(feed_of(&s, cam), Some(feed), "in series with its first plane");
    assert_eq!(feed_of(&s, out), Some((cam, 0)));
    assert_eq!(s.cut().xsheet.params.len(), columns + 3, "pan x, pan y, zoom");
    let want = [0.0, 0.0, 1.0];
    for (column, value) in tracks(&s, cam).into_iter().zip(want) {
        let held = key(&s, column, 3).expect("keyed at the current frame");
        assert_eq!(held.value, value);
        assert_eq!(held.interp, ParamInterp::Linear, "a Hold key would never move");
    }
    assert!(s.engine.undo().is_ok());
    assert_eq!((nodes(&s), s.cut().xsheet.params.len()), (count, columns));
    assert_eq!(feed_of(&s, out), Some(feed));
}

/// The two framings: the box drags, End goes to the last frame, the box
/// drags again. Each is one undo step, and the frames between move.
#[test]
fn aufx_frame_box_keys_start_and_end() {
    let mut h = film_room(1600.0, 900.0);
    assert_eq!(h.editor().viewers[&0].drawn_box, None, "only while armed");
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    assert!(h.state().graph_ui.frame_box);
    let boxed = h.editor().viewers[&0].drawn_box.expect("the box stands on the Result");
    assert!(pane_rect(&h, Pane::Viewer(0)).contains_rect(boxed));
    assert!(says(&h, FRAME_BOX_SAYS), "{:?}", h.painted_text());
    let cam = output_camera(h.state().cut()).expect("a camera");
    let [px, py, zoom] = tracks(h.state(), cam);
    let view = h.editor().viewers[&0].drawn_box.unwrap();
    let start = view.center();

    let history = h.state().engine.history_len();
    h.drag(start, start + vec2(-30.0, 0.0), 6);
    settle(&mut h);
    assert_eq!(h.state().engine.history_len(), history + 1, "the first framing");
    let first = key(h.state(), px, 0).expect("the start").value;
    assert!(first < -1.0, "the box went left, so the camera did: {first}");
    assert_eq!(key(h.state(), py, 0).map(|k| k.value), Some(0.0));

    h.key(Key::End);
    settle(&mut h);
    let last = h.state().view.frame;
    assert_eq!(last, 11, "End goes to the last frame");
    h.drag(start, start + vec2(40.0, 0.0), 6);
    settle(&mut h);
    assert_eq!(h.state().engine.history_len(), history + 2, "the second framing");
    let end = key(h.state(), px, last).expect("the end").value;
    assert!(end > first, "{first} to {end}");
    assert!(key(h.state(), py, last).is_some() && key(h.state(), zoom, last).is_some(), "all three keyed");
    assert_eq!(key(h.state(), px, 0).map(|k| k.value), Some(first), "the start stands");
    let mid = CameraState::at(h.state().cut(), cam, 5).expect("the camera").pan_x;
    assert!(first < mid && mid < end, "it moves between them: {first}, {mid}, {end}");
}

/// One turn of the wheel runs over many frames, and is one framing.
#[test]
fn aufx_frame_box_wheel_is_one_undo() {
    let mut h = film_room(1600.0, 900.0);
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    let cam = output_camera(h.state().cut()).expect("a camera");
    let [_, _, zoom] = tracks(h.state(), cam);
    let history = h.state().engine.history_len();
    let at = h.editor().viewers[&0].drawn_box.unwrap().center();
    h.wheel_at(at, 240.0);
    h.wait(0.6);
    settle(&mut h);
    assert_eq!(h.state().engine.history_len(), history + 1, "one undo");
    assert!(key(h.state(), zoom, 0).expect("keyed").value > 1.0);
    assert_eq!(h.editor().viewers[&0].zoom, 1.0, "the box took the wheel, not the view");
}

#[test]
fn aufx_frame_box_stays_until_another_tool_or_a_second_tap() {
    let mut h = film_room(1600.0, 900.0);
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    assert!(h.state().graph_ui.frame_box);
    let (count, history) = (nodes(h.state()), h.state().engine.history_len());
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    assert!(!h.state().graph_ui.frame_box, "a second tap puts it away");
    assert_eq!(h.editor().viewers[&0].drawn_box, None);
    assert_eq!((nodes(h.state()), h.state().engine.history_len()), (count, history), "and writes nothing");

    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    assert!(h.state().graph_ui.frame_box, "the same camera, again");
    assert_eq!((nodes(h.state()), h.state().engine.history_len()), (count, history));
    h.editor_mut().canvas.tool = crate::canvas::CanvasTool::Select;
    settle(&mut h);
    assert!(!h.state().graph_ui.frame_box, "another tool ends it");

    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    assert!(h.state().graph_ui.frame_box);
    enter(&mut h, Stage::Drawing);
    assert!(!h.state().graph_ui.frame_box, "and so does leaving the step");
}

/// A framing reads the camera it moves: its zoom and turn set how far.
#[test]
fn aufx_framing_reads_through_the_camera() {
    let mut cam = CameraState::defaults(0);
    (cam.pan_x, cam.zoom, cam.rotate_deg) = (10.0, 2.0, 90.0);
    let framing = Framing { frame: 0, shift: (8.0, 0.0), factor: 1.5 };
    let (x, y, zoom) = framing.on(&cam);
    assert!((x - 10.0).abs() < 1e-3, "{x}");
    assert!((y - 4.0).abs() < 1e-3, "{y}");
    assert_eq!(zoom, 3.0);
    let far = Framing { frame: 0, shift: (0.0, 0.0), factor: 1000.0 };
    assert_eq!(far.on(&cam).2, 20.0, "the parameter's range holds");
}

#[test]
fn aufx_camera_move_reuses_the_feeding_camera() {
    let mut s = composed();
    let cam = s.shelf_add(NodeKind::camera()).expect("a camera");
    s.shelf_add(glow()).expect("above it");
    let count = nodes(&s);
    assert!(s.camera_move());
    assert_eq!(nodes(&s), count, "no second camera");
    assert_eq!(output_camera(s.cut()), Some(cam));
    let bound = tracks(&s, cam);
    // Asked again it finds every track: nothing to write.
    let (history, columns) = (s.engine.history_len(), s.cut().xsheet.params.len());
    assert!(s.camera_move());
    assert_eq!((s.engine.history_len(), s.cut().xsheet.params.len()), (history, columns));
    assert_eq!(tracks(&s, cam), bound);
}

#[test]
fn aufx_camera_keeps_planes_keys_and_binds() {
    let mut s = composed();
    let rotate = s.add_param_column("my turn").expect("a track");
    let pan = s.add_param_column("my pan").expect("a track");
    s.set_param_key(rotate, Some(anim_core::xsheet::ParamKey { value: 15.0, interp: ParamInterp::Ease, handles: None }));
    s.goto(6);
    s.set_param_key(pan, Some(anim_core::xsheet::ParamKey { value: 25.0, interp: ParamInterp::Hold, handles: None }));
    let mut kind = NodeKind::camera();
    let NodeKind::Camera { planes, params, binds } = &mut kind else {
        panic!("a camera");
    };
    planes.push(planes[0].clone());
    params.set("zoom", FxValue::Float(2.0));
    binds.set("rotate_deg", Some(rotate));
    binds.set("pan_x", Some(pan));
    let cam = s.shelf_add(kind).expect("a camera");
    let before = s.cut().graph.node(cam).unwrap().kind.clone();
    let held = s.cut().xsheet.param(pan).unwrap().keys().collect::<Vec<_>>();

    s.goto(2);
    assert!(s.camera_move());
    let NodeKind::Camera { planes, params, binds } = &s.cut().graph.node(cam).unwrap().kind else {
        panic!("a camera");
    };
    let NodeKind::Camera { planes: was, params: stored, .. } = &before else {
        panic!("a camera");
    };
    assert_eq!(planes, was, "its planes");
    assert_eq!(params, stored, "its stored values");
    assert_eq!(binds.get("rotate_deg"), Some(rotate), "its other binds");
    assert_eq!(binds.get("pan_x"), Some(pan), "a track it had stays its track");
    assert_eq!(s.cut().xsheet.param(pan).unwrap().keys().collect::<Vec<_>>(), held, "with its keys");
    let zoom = binds.get("zoom").expect("the new track");
    assert_eq!(key(&s, zoom, 2).map(|k| k.value), Some(2.0), "keyed with the value it had");
}

// ---- The shelf ----

#[test]
fn aufx_display_map_leaves_stored_names() {
    let mut h = film_room(1600.0, 900.0);
    for (word, stored) in [("Soft focus", "Diffusion"), ("Color look", "Grade")] {
        h.click(word);
        settle(&mut h);
        let id = h.state().selected_node.expect("selected");
        let kind = &h.state().cut().graph.node(id).unwrap().kind;
        assert_eq!(kind.name(), stored, "the file keeps its word");
        let painted = h.painted_text();
        assert!(painted.iter().any(|w| w == word), "the title and the inspector: {painted:?}");
        assert!(!painted.iter().any(|w| w == stored), "{painted:?}");
    }
    // The add menu says the display word, and the old one still finds it.
    h.click(vocab::ADD_EFFECT);
    settle(&mut h);
    assert!(h.has("Soft focus"), "{}", h.describe());
    assert!(!h.has("Diffusion"));
    h.type_text("Diffusion");
    settle(&mut h);
    h.key(Key::Enter);
    settle(&mut h);
    let id = h.state().selected_node.expect("added");
    assert_eq!(h.state().cut().graph.node(id).unwrap().kind.name(), "Diffusion", "the trade word finds its row");
}

/// The shelf is one row 40 tall, whatever the pane, and its handles stand.
#[test]
fn aufx_shelf_stands_at_every_size() {
    for (w, tall) in SIZES {
        let mut h = film_room(w, tall);
        let pane = pane_rect(&h, Pane::NodeGraph);
        for word in ["Glow", vocab::CAMERA_MOVE, vocab::ADD_EFFECT, SHELF_MORE] {
            let at = h.rect(word);
            assert!(at.top() >= pane.top() && at.bottom() <= pane.top() + 44.0, "{word} at {w}: {at:?} in {pane:?}");
            assert!(at.right() <= pane.right(), "{word} at {w}: {at:?} in {pane:?}");
        }
        h.click("Glow");
        settle(&mut h);
        let strength = h.rect(vocab::STRENGTH);
        assert!(strength.right() <= pane.right() + 1.0, "at {w}: {strength:?} in {pane:?}");
        assert!(strength.left() >= h.rect(SHELF_MORE).right(), "at {w}");
        assert!(strength.width() >= 40.0, "a track a hand can drag at {w}: {strength:?}");
        for handle in [GLOW_MARK, CAMERA_MARK, ADD_MARK, STRENGTH_MARK] {
            assert!(stands(&mut h, handle), "{handle:?} at {w}");
        }
    }
    // Wide, all five stand; narrow, the rest wait under More.
    let h = film_room(1600.0, 900.0);
    for word in ["Soft focus", "Blur", "Color look"] {
        assert!(h.has(word), "{word}");
    }
}

/// A row of controls breaks before a field runs off the pane: at 1280 the
/// Glow's tint chip stood across the edge, at 1600 the camera's aperture.
#[test]
fn aufx_inspector_fields_stay_in_the_pane() {
    for (w, tall) in SIZES {
        let mut h = film_room(w, tall);
        let pane = pane_rect(&h, Pane::NodeGraph);
        for word in ["Glow", vocab::CAMERA_MOVE] {
            h.click(word);
            settle(&mut h);
            let fields = h.state().graph_ui.fx.fields.clone();
            assert!(!fields.is_empty(), "{word} at {w}");
            for (key, at) in fields {
                assert!(at.right() <= pane.right(), "{word} {key} at {w}: {at:?} in {pane:?}");
            }
        }
    }
}

/// A narrow pane folds the middle effects under More: they still add.
#[test]
fn aufx_narrow_shelf_folds_into_more() {
    let (w, tall) = SIZES[2];
    let mut h = film_room(w, tall);
    assert!(!h.has("Color look"), "folded at {w}: {}", h.describe());
    h.click(SHELF_MORE);
    settle(&mut h);
    h.click("Color look");
    settle(&mut h);
    let id = h.state().selected_node.expect("added");
    assert_eq!(h.state().cut().graph.node(id).unwrap().kind.name(), "Grade");
}

#[test]
fn aufx_camera_move_is_an_animations_alone() {
    let drawing = templates::Template { kind: Some(ProjectKind::Drawing), ..templates::READY };
    for state in [drawing.build(), templates::ALL[templates::COMIC].build()] {
        let mut h = EditorHarness::with_state_shipped(state);
        sized(&mut h, 1600.0, 900.0);
        enter(&mut h, Stage::Composite);
        assert!(h.has("Glow"), "{}", h.describe());
        assert!(!h.has(vocab::CAMERA_MOVE), "{}", h.describe());
    }
}

#[test]
fn aufx_full_finishing_waits_under_more_with_a_warning() {
    let mut h = film_room(1600.0, 900.0);
    assert!(!h.has(vocab::FULL_FINISHING_SETUP), "off the shelf");
    h.click(SHELF_MORE);
    settle(&mut h);
    assert!(says(&h, FULL_SETUP_WARNS), "{:?}", h.painted_text());
    h.click(vocab::FULL_FINISHING_SETUP);
    settle(&mut h);
    assert!(nodes(h.state()) > 7, "the whole chain: {}", nodes(h.state()));
}

#[test]
fn aufx_empty_graph_offers_the_standard_composite() {
    let mut h = EditorHarness::with_state_shipped(one_drawing());
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h, Stage::Composite);
    assert_eq!(nodes(h.state()), 0);
    assert!(says(&h, NOTHING_TO_SHOW), "the Result says so too");
    assert!(says(&h, SET_UP_COMPOSITE), "{:?}", h.painted_text());
    h.click(GRAPH_SET_UP);
    settle(&mut h);
    assert!(h.state().cut().graph.output.is_some());
    assert!(!h.has(GRAPH_SET_UP));
}

/// `Add effect` keeps Tab's menu and wires what it adds; Tab stays by hand.
#[test]
fn aufx_add_effect_wires_and_tab_does_not() {
    let mut h = film_room(1600.0, 900.0);
    h.click("Glow");
    settle(&mut h);
    let out = output(h.state());
    let feed = feed_of(h.state(), out);

    h.perform(Action::AddNode);
    settle(&mut h);
    h.type_text("Vignette");
    settle(&mut h);
    h.key(Key::Enter);
    settle(&mut h);
    let id = h.state().selected_node.expect("added");
    assert_eq!(h.state().cut().graph.node(id).unwrap().kind.name(), "Vignette");
    assert_eq!(feed_of(h.state(), out), feed, "the hand path wires nothing");
    assert_eq!(feed_of(h.state(), id), None);

    h.click(vocab::ADD_EFFECT);
    settle(&mut h);
    h.type_text("Rim");
    settle(&mut h);
    h.key(Key::Enter);
    settle(&mut h);
    let id = h.state().selected_node.expect("added");
    assert_eq!(feed_of(h.state(), out), Some((id, 0)), "Add effect puts it on the picture");
    assert_eq!(feed_of(h.state(), id), feed);
}

/// Say's answer is an effect reaching the Output: a node off to the side is
/// no answer, and the hint keeps its sentence.
#[test]
fn aufx_say_reads_an_effect_reaching_the_output() {
    let mut s = one_drawing();
    paint_texels(&mut s, &[(40, 40)], [0.1, 0.1, 0.1, 1.0]);
    let mut h = EditorHarness::with_state_shipped(s);
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h, Stage::Composite);
    let hint = |h: &EditorHarness| h.editor().say.hint.as_ref().map(|s| s.words.clone());
    assert_eq!(hint(&h).as_deref(), Some(PICK_AN_EFFECT));
    h.state_mut().graph_add_node(NodeKind::effect(FxKind::Blur), (0.0, 0.0));
    settle(&mut h);
    assert_eq!(hint(&h).as_deref(), Some(PICK_AN_EFFECT), "it reaches nothing yet");
    h.state_mut().auto_comp();
    settle(&mut h);
    h.click("Glow");
    settle(&mut h);
    assert_ne!(hint(&h).as_deref(), Some(PICK_AN_EFFECT), "that one did");
}

// ---- J4 ----

/// Glow and pan: spec J4, seven actions from the Draw step, no wire drawn and
/// no room left, ending with a camera that moves.
#[test]
fn aufx_glow_and_pan_journey() {
    fn keep_room(r: &mut Recorder<'_>, read: &mut Vec<String>) {
        settle(r.harness());
        assert_eq!(r.harness().editor().stage, Some(Stage::Composite), "no room switch");
        assert!(r.harness().state().pending_wire.is_none(), "no wire");
        read.extend(r.harness().editor().say.hint.as_ref().map(|s| s.words.clone()));
        read.push(r.harness().state().status.clone());
    }
    let mut s = auguide_tests::film().build();
    paint_texels(&mut s, &[(40, 40), (41, 40)], [0.1, 0.1, 0.1, 1.0]);
    let mut h = EditorHarness::with_state_shipped(s);
    sized(&mut h, 1600.0, 900.0);
    let words = ["Glow", vocab::STRENGTH, vocab::CAMERA_MOVE, FRAME_BOX_SAYS, vocab::GO_TO_LAST_DRAWING, PICK_A_BOX];
    let mut read: Vec<String> = words.map(String::from).to_vec();
    let concepts = ["an effect goes on the picture at a tap", "strength is how much", "a camera moves between two framings"];

    let mut r = Recorder::new(&mut h, "glow and pan");
    r.act("go to Effects", Act::Click(vocab::STEP_EFFECTS));
    keep_room(&mut r, &mut read);
    r.act("tap Glow", Act::Click("Glow"));
    keep_room(&mut r, &mut read);
    let track = r.harness().rect(vocab::STRENGTH);
    let drag = [track.left_center() + vec2(track.width() * 0.2, 0.0), track.left_center() + vec2(track.width() * 0.6, 0.0)];
    r.act("set its strength", Act::Drag(&drag));
    keep_room(&mut r, &mut read);
    r.act("tap Camera move", Act::Click(vocab::CAMERA_MOVE));
    keep_room(&mut r, &mut read);
    let c = r.harness().editor().viewers[&0].drawn_box.expect("the box").center();
    let path = |dx: f32| -> Vec<Pos2> { (0..=6).map(|i| c + vec2(dx * i as f32 / 6.0, 0.0)).collect() };
    let start = path(-30.0);
    r.act("frame the start", Act::Drag(&start));
    keep_room(&mut r, &mut read);
    assert_eq!(r.harness().state().status, CAMERA_START_SET, "what comes next, in words");
    r.act("go to the end", Act::Click(vocab::GO_TO_LAST_DRAWING));
    keep_room(&mut r, &mut read);
    let end = path(40.0);
    r.act("frame the end", Act::Drag(&end));
    keep_room(&mut r, &mut read);
    let tally = r.tally();
    assert!(tally.actions() <= 8, "{tally:?}");
    assert_eq!(tally.actions(), 7, "{tally:?}");

    let labels: Vec<String> = h.state().engine.history().iter().map(|e| e.label.to_lowercase()).collect();
    assert!(!labels.iter().any(|l| l.contains("connect")), "0 wires: {labels:?}");
    assert!(concepts.len() <= 3);
    let jargon = measure::jargon_in(&read);
    assert!(jargon.len() <= 2, "{jargon:?} in {read:?}");
    let state = h.state();
    let cam = output_camera(state.cut()).expect("the camera");
    let (a, b) = (
        CameraState::at(state.cut(), cam, 0).expect("the start").pan_x,
        CameraState::at(state.cut(), cam, 11).expect("the end").pan_x,
    );
    assert!(a < b, "{a} to {b}");
    let glows = state.cut().graph.nodes().filter(|n| n.kind.name() == "Glow").count();
    assert_eq!(glows, 1);
}

/// The ignored pass owes the shelf at both sizes: taken, then checked.
#[test]
fn aufx_the_ignored_pass_owes_the_shelf_shots() {
    let pass = include_str!("uishot.rs").split("fn uishot_rooms()").nth(1).expect("the pass");
    let taken = pass.find("fx_moments(&dir);").expect("it takes them");
    let checked = pass.find("check_shots(&dir, &fx_owed())").expect("it checks them");
    assert!(taken < checked, "taken, then checked");
    let owed = crate::uishot::fx_owed();
    assert_eq!(owed.len(), 16, "{owed:?}");
    assert!(owed.iter().any(|(name, w, _)| name == "fx-frame-box-1280.png" && *w == 1280), "{owed:?}");
    for state in ["fx-empty", "fx-disabled", "fx-add-menu", "fx-framed"] {
        assert!(owed.iter().any(|(name, ..)| name.starts_with(state)), "{state}: {owed:?}");
    }
}

// ---- The inspector under the shelf ----

/// Where the inspector's field for `key` stands.
fn field(h: &EditorHarness, key: &str) -> Rect {
    let fields = &h.state().graph_ui.fx.fields;
    fields.iter().find(|(k, _)| *k == key).map(|(_, r)| *r).unwrap_or_else(|| panic!("{key}: {fields:?}"))
}

fn selected(h: &EditorHarness) -> NodeId {
    h.state().selected_node.expect("a selection")
}

/// The Effects step's shipped inspector runs after `Strength`, which reads
/// the same live slot: a drag there is still one undo on the one parameter.
#[test]
fn aufx_inspector_drag_is_one_undo_on_one_parameter() {
    let mut h = film_room(1600.0, 900.0);
    h.click("Glow");
    settle(&mut h);
    let id = selected(&h);
    let others = |h: &EditorHarness| ["threshold", "radius"].map(|k| float(h.state(), id, k));
    let (held, was, history) = (others(&h), float(h.state(), id, "intensity"), h.state().engine.history_len());
    let at = field(&h, "intensity").center();
    h.drag(at, at + vec2(40.0, 0.0), 8);
    settle(&mut h);
    assert_ne!(float(h.state(), id, "intensity"), was, "the drag landed");
    assert_eq!(others(&h), held, "on that parameter alone");
    assert_eq!(h.state().engine.history_len(), history + 1, "as one undo");

    h.click("Blur");
    settle(&mut h);
    let id = selected(&h);
    let (x, y, history) = (float(h.state(), id, "radius_x"), float(h.state(), id, "radius_y"), h.state().engine.history_len());
    let at = field(&h, "radius_x").center();
    h.drag(at, at + vec2(35.0, 0.0), 7);
    settle(&mut h);
    assert_ne!(float(h.state(), id, "radius_x"), x, "the drag landed");
    assert_eq!(float(h.state(), id, "radius_y"), y, "nobody touched radius y");
    assert_eq!(h.state().engine.history_len(), history + 1, "one undo");
}

/// Every kind's Strength names real amounts, and Blur's stored pair, its one
/// exception, keeps its direction: the shelf moves it by one ratio.
#[test]
fn aufx_strength_keeps_a_directional_blur() {
    for fx in FxKind::ALL {
        for key in strength_keys(*fx) {
            assert!(fx.spec(key).is_some(), "{fx:?} has no {key}");
        }
    }
    assert_eq!(strength_keys(FxKind::MotionBlur), ["length"], "one plain amount");
    let mut h = film_room(1600.0, 900.0);
    h.click("Blur");
    settle(&mut h);
    let id = selected(&h);
    for (x, y) in [(8.0, 0.0), (0.0, 8.0), (8.0, 4.0)] {
        let NodeKind::Effect { fx, mut params, binds } = h.state().cut().graph.node(id).unwrap().kind.clone() else {
            panic!("an effect");
        };
        params.set("radius_x", FxValue::Float(x));
        params.set("radius_y", FxValue::Float(y));
        h.state_mut().graph_set_kind(id, NodeKind::Effect { fx, params, binds });
        settle(&mut h);
        let history = h.state().engine.history_len();
        let track = h.rect(vocab::STRENGTH);
        h.drag(track.left_center() + vec2(track.width() * 0.5, 0.0), track.left_center() + vec2(track.width() * 0.8, 0.0), 6);
        settle(&mut h);
        let (nx, ny) = (float(h.state(), id, "radius_x"), float(h.state(), id, "radius_y"));
        assert!(nx.max(ny) > f32::max(x, y), "({x}, {y}) to ({nx}, {ny})");
        assert!((nx * y - ny * x).abs() < 1e-2 * nx.max(ny) * f32::max(x, y), "({x}, {y}) to ({nx}, {ny}): one direction");
        assert_eq!(h.state().engine.history_len(), history + 1, "one undo");
    }
}

// ---- A tap changes the picture, or says why ----

/// A dab of `color` in a new animation, with its sheet's graph built.
fn dabbed(color: [f32; 4]) -> AppState {
    let mut s = auguide_tests::film().build();
    let dab: Vec<(i32, i32)> = (600..680).flat_map(|x| (320..400).map(move |y| (x, y))).collect();
    paint_texels(&mut s, &dab, color);
    assert!(s.auto_comp());
    s
}

fn starter_red() -> [f32; 4] {
    let law = auguide_tests::film().build().engine.project.color_law;
    law.encode_rgba(plate::STARTER_SWATCHES[2].1.to_array())
}

fn final_picture(s: &AppState) -> anim_core::fx::Image {
    let opts = RenderOpts { purpose: Purpose::Final, scale: 1.0 };
    let (w, tall) = (s.engine.project.width, s.engine.project.height);
    deliver::render(s.cut(), s.engine.project.color_law, s.view.frame, w, tall, &opts)
}

/// How far the picture moved at its worst, on the 0 to 1 a screen shows.
fn most_moved(a: &anim_core::fx::Image, b: &anim_core::fx::Image) -> f32 {
    let seen = |v: f32| v.clamp(0.0, 1.0);
    a.px.iter().zip(&b.px).map(|(x, y)| (seen(*x) - seen(*y)).abs()).fold(0.0, f32::max)
}

/// The art a newcomer has: every shelf effect changes it at the tap, with the
/// amount it opens on. The kinds' stored defaults stay where they were.
#[test]
fn aufx_every_shelf_effect_changes_colored_art() {
    for fx in [FxKind::Glow, FxKind::Diffusion, FxKind::Blur, FxKind::Grade] {
        let mut s = dabbed(starter_red());
        let before = final_picture(&s);
        s.shelf_add(NodeKind::effect(fx)).expect("added");
        let moved = most_moved(&before, &final_picture(&s));
        assert!(moved > 2.0 / 255.0, "{fx:?} moved {moved}");
        assert_ne!(s.status, GLOW_NEEDS_COLOR, "{fx:?}");
    }
    let default = |fx: FxKind, key: &str| fx.spec(key).expect(key).default_value();
    assert_eq!(default(FxKind::Glow, "threshold"), FxValue::Float(0.7), "the finishing chain's");
    assert_eq!(default(FxKind::Grade, "saturation"), FxValue::Float(1.0));
}

/// A newcomer's first art is a line, not a filled shape, and the tap has to
/// show on that: a glow nobody can see is the same as one that did not run.
#[test]
fn aufx_glow_shows_on_a_thin_line() {
    let mut s = auguide_tests::film().build();
    let (cx, cy) = (s.engine.project.width as f32 / 2.0, s.engine.project.height as f32 / 2.0);
    let turn = (0..4000).map(|n| n as f32 / 4000.0 * std::f32::consts::TAU);
    let ring: Vec<(i32, i32)> = turn
        .flat_map(|t| [140.0, 141.0].map(|r| ((cx + r * t.cos()) as i32, (cy + r * t.sin()) as i32)))
        .collect();
    paint_texels(&mut s, &ring, starter_red());
    assert!(s.auto_comp());
    let before = final_picture(&s);
    s.shelf_add(glow()).expect("added");
    let after = final_picture(&s);
    let seen = |v: f32| v.clamp(0.0, 1.0);
    let moved = |(a, b): (&[f32], &[f32])| (0..3).any(|c| (seen(a[c]) - seen(b[c])).abs() > 16.0 / 255.0);
    let plain = before.px.chunks(4).zip(after.px.chunks(4)).filter(|p| moved(*p)).count();
    assert!(plain >= 1000, "{plain} pixels moved by 16/255");
}

/// Near-black has nothing to light. The tap says so, in place of a dead slider.
#[test]
fn aufx_glow_on_dark_ink_says_why() {
    let mut h = EditorHarness::with_state_shipped(dabbed([0.1, 0.1, 0.1, 1.0]));
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h, Stage::Composite);
    let before = final_picture(h.state());
    h.click("Glow");
    settle(&mut h);
    assert_eq!(most_moved(&before, &final_picture(h.state())), 0.0, "nothing to light");
    assert_eq!(h.state().status, GLOW_NEEDS_COLOR);
    assert!(says(&h, GLOW_NEEDS_COLOR), "{:?}", h.painted_text());
    // The same tap on color says nothing of the kind.
    let mut h = EditorHarness::with_state_shipped(dabbed(starter_red()));
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h, Stage::Composite);
    h.click("Glow");
    settle(&mut h);
    assert_ne!(h.state().status, GLOW_NEEDS_COLOR);
}

// ---- A camera move a newcomer can finish ----

#[test]
fn aufx_camera_move_says_each_next_step() {
    let mut h = three_drawings();
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    assert_eq!(h.state().status, CAMERA_ADDED);
    assert!(says(&h, CAMERA_ADDED), "{:?}", h.painted_text());
    let c = h.editor().viewers[&0].drawn_box.expect("the box").center();
    h.drag(c, c + vec2(-30.0, 0.0), 6);
    settle(&mut h);
    assert_eq!(h.state().status, CAMERA_START_SET);
    assert!(says(&h, CAMERA_START_SET), "{:?}", h.painted_text());
    h.click(vocab::GO_TO_LAST_DRAWING);
    settle(&mut h);
    h.drag(c, c + vec2(40.0, 0.0), 6);
    settle(&mut h);
    let done = camera_done(&h.ctx());
    assert_eq!(done, "Camera move done. Press Space to play.", "the keymap's own chord");
    assert_eq!(h.state().status, done);
    assert!(says(&h, &done), "{:?}", h.painted_text());
}

/// Three drawings and a blank tail on the Effects step.
fn three_drawings() -> EditorHarness {
    let mut s = auguide_tests::film().build();
    let ink = [0.8, 0.2, 0.2, 1.0];
    paint_texels(&mut s, &[(40, 40), (41, 40)], ink);
    for _ in 0..2 {
        assert!(s.add_drawing_after());
        paint_texels(&mut s, &[(40, 40), (41, 40)], ink);
    }
    s.goto(0);
    let mut h = EditorHarness::with_state_shipped(s);
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h, Stage::Composite);
    h
}

/// The way to the end stands on the Result at every size, clear of the latches.
#[test]
fn aufx_last_drawing_stands_clear_at_every_size() {
    for (w, tall) in SIZES {
        let mut h = film_room(w, tall);
        h.click(vocab::CAMERA_MOVE);
        settle(&mut h);
        let at = h.rect(vocab::GO_TO_LAST_DRAWING);
        assert!(pane_rect(&h, Pane::Viewer(0)).contains_rect(at), "at {w}: {at:?}");
        assert!(!at.intersects(h.rect(vocab::FULL_QUALITY)), "at {w}: {at:?} over {:?}", h.rect(vocab::FULL_QUALITY));
    }
}

/// `End` runs past what plays on a blank tail, so the one-tap route has to
/// land where the export ends: the move has to be in the film.
#[test]
fn aufx_last_drawing_lands_inside_the_export() {
    let mut h = three_drawings();
    let shown = h.state().shown_length();
    assert!(1 < shown && shown < h.state().frame_count(), "a blank tail: {shown}");
    assert!(!h.has(vocab::GO_TO_LAST_DRAWING), "only with the box");
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    assert_eq!(h.node_enabled(vocab::GO_TO_LAST_DRAWING), Some(true));
    let c = h.editor().viewers[&0].drawn_box.expect("the box").center();
    h.drag(c, c + vec2(-30.0, 0.0), 6);
    settle(&mut h);
    h.key(Key::End);
    settle(&mut h);
    assert_eq!(h.state().view.frame, h.state().frame_count() - 1, "End stays the sheet's last frame");
    h.click(vocab::GO_TO_LAST_DRAWING);
    settle(&mut h);
    let last = h.state().view.frame;
    assert_eq!(last, shown - 1, "the last frame the Export cards pass");
    assert_eq!(h.node_enabled(vocab::GO_TO_LAST_DRAWING), Some(false), "already there");
    h.drag(c, c + vec2(40.0, 0.0), 6);
    settle(&mut h);
    let cam = output_camera(h.state().cut()).expect("a camera");
    let [px, ..] = tracks(h.state(), cam);
    let end = key(h.state(), px, last).expect("the end key lands inside the export").value;
    assert!(key(h.state(), px, h.state().frame_count() - 1).is_none(), "and none on the blank tail");
    let start = key(h.state(), px, 0).expect("the start").value;
    let mid = CameraState::at(h.state().cut(), cam, last / 2).expect("the camera").pan_x;
    assert!(start < mid && mid < end, "the export sees the whole move: {start}, {mid}, {end}");
}

/// The wheel's gesture ends on rest alone, so a run of ticks over a moving
/// playhead is one framing, keyed where it began.
#[test]
fn aufx_wheel_over_a_moving_playhead_is_one_framing() {
    let mut h = film_room(1600.0, 900.0);
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    let cam = output_camera(h.state().cut()).expect("a camera");
    let [_, _, zoom] = tracks(h.state(), cam);
    let history = h.state().engine.history_len();
    let at = h.editor().viewers[&0].drawn_box.unwrap().center();
    for frame in [1, 2, 3] {
        h.wheel_tick_at(at, 120.0);
        h.run_frames(1);
        h.state_mut().goto(frame);
        h.run_frames(1);
        assert_eq!(h.state().engine.history_len(), history, "still turning");
    }
    h.wait(0.6);
    settle(&mut h);
    assert_eq!(h.state().engine.history_len(), history + 1, "one framing");
    assert!(key(h.state(), zoom, 0).expect("where it began").value > 1.0);
    assert!((1..=3).all(|f| key(h.state(), zoom, f).is_none()), "and nowhere else");
}

/// A pen has no wheel: a corner of the box draws it in, one undo, the place
/// kept. A box that shrinks is a camera that zooms in.
#[test]
fn aufx_frame_box_corner_zooms_in_one_undo() {
    let mut h = film_room(1600.0, 900.0);
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    let cam = output_camera(h.state().cut()).expect("a camera");
    let [px, py, zoom] = tracks(h.state(), cam);
    let boxed = h.editor().viewers[&0].drawn_box.expect("the box");
    let history = h.state().engine.history_len();
    let corner = boxed.right_bottom();
    h.drag(corner, corner + (boxed.center() - corner) * 0.25, 8);
    settle(&mut h);
    assert_eq!(h.state().engine.history_len(), history + 1, "one undo");
    let value = key(h.state(), zoom, 0).expect("keyed").value;
    assert!((value - 4.0 / 3.0).abs() < 0.05, "a quarter in: {value}");
    assert_eq!(key(h.state(), px, 0).map(|k| k.value), Some(0.0), "the place kept");
    assert_eq!(key(h.state(), py, 0).map(|k| k.value), Some(0.0));
    assert_eq!(h.editor().viewers[&0].drawn_box, Some(boxed), "the box home around the new frame");
}

/// The box holds its edge against the Result's grey, where `BLUE` alone reads
/// 1.5:1, and stands where a person can see it: arming brings it forward.
#[test]
fn aufx_frame_box_stands_out_and_comes_forward() {
    assert!(plate::contrast(plate::ON_FILL, plate::RESULT) >= 3.0, "the edge under the blue");
    let mut h = film_room(1600.0, 900.0);
    h.editor_mut().open_pane(Pane::Canvas);
    settle(&mut h);
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    let boxed = h.editor().viewers[&0].drawn_box.expect("the Result came forward with its box");
    assert_eq!(h.painted_fill_at(boxed.right_bottom()), Some(plate::ON_FILL), "a grip to take hold of");
}

#[test]
fn aufx_armed_camera_move_is_a_filled_shape() {
    let mut h = film_room(1600.0, 900.0);
    let at = h.rect(vocab::CAMERA_MOVE).center();
    let away = egui::pos2(4.0, 4.0);
    h.point_at(away);
    settle(&mut h);
    let rest = h.painted_fill_at(at);
    assert_ne!(rest, Some(plate::BLUE), "at rest");
    h.click(vocab::CAMERA_MOVE);
    h.point_at(away);
    settle(&mut h);
    assert_eq!(h.painted_fill_at(at), Some(plate::BLUE), "armed");
    assert!(plate::contrast(plate::ON_FILL, plate::BLUE) >= 4.5, "with words that read on it");
    h.click(vocab::CAMERA_MOVE);
    h.point_at(away);
    settle(&mut h);
    assert_eq!(h.painted_fill_at(at), rest, "and back");
}

/// An undo can take the tracks from a camera that stays, and the box with
/// them: nothing would land. A framing that still comes says so.
#[test]
fn aufx_frame_box_goes_with_its_tracks() {
    let mut h = film_room(1600.0, 900.0);
    h.state_mut().shelf_add(NodeKind::camera()).expect("a camera");
    settle(&mut h);
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    assert!(h.state().graph_ui.frame_box);
    h.perform(Action::Undo);
    settle(&mut h);
    assert!(output_camera(h.state().cut()).is_some(), "the camera stands");
    assert!(!h.state().graph_ui.frame_box, "its box does not");
    assert_eq!(h.editor().viewers[&0].drawn_box, None);
    let framing = Framing { frame: 0, shift: (4.0, 0.0), factor: 1.0 };
    h.state_mut().frame_camera(&framing, "done");
    assert_eq!(h.state().refusal, CAMERA_NO_TRACKS, "aloud");
}

// ---- The pane ----

#[test]
fn aufx_no_selection_says_one_plain_line() {
    let mut h = film_room(832.0, 600.0);
    h.state_mut().selected_node = None;
    settle(&mut h);
    assert!(says(&h, PICK_A_BOX), "{:?}", h.painted_text());
    let painted = h.painted_text().join(" ");
    assert!(!painted.contains("no node selected"), "{painted}");
    assert!(!painted.contains("right-click a node"), "{painted}");
}

/// A person clicking from the Glow to the Camera finds the graph where it was.
#[test]
fn aufx_selection_never_moves_the_graph() {
    for (w, tall) in SIZES {
        let mut h = film_room(w, tall);
        h.click("Glow");
        settle(&mut h);
        let glow = selected(&h);
        h.click(vocab::CAMERA_MOVE);
        settle(&mut h);
        let cam = selected(&h);
        let out = output(h.state());
        let place = |h: &EditorHarness| {
            let rects = &h.state().graph_ui.node_rects;
            rects.iter().find(|(id, _)| *id == out).map(|(_, r)| *r).expect("the Output")
        };
        h.state_mut().selected_node = None;
        settle(&mut h);
        let home = place(&h);
        for node in [glow, cam, out] {
            h.state_mut().selected_node = Some(node);
            settle(&mut h);
            assert_eq!(place(&h), home, "at {w}");
        }
        h.state_mut().selected_node = Some(cam);
        settle(&mut h);
        h.click(vocab::DETAILS);
        settle(&mut h);
        assert_eq!(place(&h), home, "with the Details open at {w}");
    }
}

/// One tap on Camera move leaves the eye on the Result: the framing alone,
/// the rest a fold away, and the fold remembered.
#[test]
fn aufx_camera_shows_its_framing_first() {
    let mut h = film_room(1600.0, 900.0);
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    let keys = |h: &EditorHarness| h.state().graph_ui.fx.fields.iter().map(|(k, _)| *k).collect::<Vec<_>>();
    assert_eq!(keys(&h), ["pan_x", "pan_y", "zoom"]);
    assert!(!h.has("+ plane"), "{}", h.describe());
    h.click(vocab::DETAILS);
    settle(&mut h);
    assert!(keys(&h).contains(&"shake_seed") && keys(&h).contains(&"aperture"), "{:?}", keys(&h));
    assert!(h.has("+ plane"));
    assert!(h.rig().config.ui.camera_details(), "remembered");
    // The hand-wiring path has no fold: everything stands, as it always did.
    let mut s = composed();
    s.shelf_add(NodeKind::camera()).expect("a camera");
    let mut frozen = EditorHarness::with_state(s);
    frozen.editor_mut().open_pane(Pane::NodeGraph);
    settle(&mut frozen);
    assert!(frozen.state().graph_ui.fx.fields.iter().any(|(k, _)| *k == "aperture"));
}

/// Each legend's letters stand level with its field, wherever it lands in a
/// row. Its rect is no witness: the row can stretch it round letters on the
/// floor.
#[test]
fn aufx_legends_centre_on_their_fields() {
    let mut h = film_room(1600.0, 900.0);
    h.click("Glow");
    settle(&mut h);
    let level = |h: &EditorHarness, word: &str, key: &str| {
        let (legend, at) = (h.painted_ink(word).center().y, field(h, key).center().y);
        assert!((legend - at).abs() <= 2.0, "{word} at {legend}, its field at {at}");
    };
    for (word, key) in [("Threshold", "threshold"), ("Radius", "radius"), ("Intensity", "intensity")] {
        level(&h, word, key);
    }
    h.click(vocab::CAMERA_MOVE);
    settle(&mut h);
    h.click(vocab::DETAILS);
    settle(&mut h);
    for (word, key) in [("Pan x", "pan_x"), ("Pan y", "pan_y"), ("Zoom in", "zoom"), ("Rotate", "rotate_deg")] {
        level(&h, word, key);
    }
}

/// Each effect moves the Output a column on: the view follows just enough to
/// keep what was added, and the Output it feeds, on the pane.
#[test]
fn aufx_new_effect_stands_in_view() {
    let mut h = film_room(1280.0, 800.0);
    let pane = pane_rect(&h, Pane::NodeGraph);
    for _ in 0..4 {
        h.state_mut().shelf_add(glow()).expect("added");
        settle(&mut h);
        let place = |id: NodeId| {
            let rects = &h.state().graph_ui.node_rects;
            rects.iter().find(|(n, _)| *n == id).map(|(_, r)| *r).expect("drawn")
        };
        let (new, out) = (place(selected(&h)), place(output(h.state())));
        assert!(pane.contains_rect(new), "{new:?} in {pane:?}");
        assert!(pane.contains_rect(out), "{out:?} in {pane:?}");
    }
}

// ---- What this part promised to leave ----

/// FNV-1a of the stretch of `source` from `head` to the `tail` after it, line
/// endings aside: the diff read, kept as a check.
fn fingerprint(source: &str, head: &str, tail: &str) -> u64 {
    let text = source.replace("\r\n", "\n");
    let from = text.find(head).unwrap_or_else(|| panic!("{head}"));
    let to = from + text[from..].find(tail).unwrap_or_else(|| panic!("{tail}"));
    text.as_bytes()[from..to].iter().fold(0xcbf29ce484222325, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

/// The code the spec says this part leaves, each against its text at the
/// commit before the part. A part that means to change one re-pins it here.
#[test]
fn aufx_guarded_code_stands() {
    const METHOD: &str = "\n    }\n";
    let doc = include_str!("doc.rs");
    let table = [
        (doc, "fn wire_under_composite(", METHOD, 0xf70b2164c8cd7c50_u64),
        (doc, "pub fn import_model(", METHOD, 0x5fc94b36903924f9),
        (doc, "pub fn set_cut_camera_from_view(", METHOD, 0x1abc664eb2084707),
        (include_str!("graphcomp.rs"), "fn render_node(", METHOD, 0xc3a8df3a7f11ff21),
        (include_str!("../../crates/anim-core/src/look.rs"), "pub fn satsuei_template(", "\n}\n", 0x14aba9b6659106d5),
    ];
    for (source, head, tail, want) in table {
        assert_eq!(fingerprint(source, head, tail), want, "{head} changed");
    }
    // Its one caller stays its one caller, and the new path does not call it.
    let calls = |source: &str, name: &str| source.matches(name).count();
    assert_eq!(calls(doc, ".wire_under_composite("), 1, "import_model's");
    let shelf = [include_str!("main.rs"), include_str!("nodegraph_panel.rs"), include_str!("fxui.rs"), include_str!("viewer.rs")];
    assert_eq!(shelf.iter().map(|s| calls(s, ".wire_under_composite(")).sum::<usize>(), 0);
    assert_eq!(calls(doc, ".set_cut_camera_from_view("), 0, "3D's alone");
}

/// What is stored stays as it was: effect names, camera parameters and their
/// keys, Tab, the step's dock, the config's shape, the schema. The finishing
/// chain has its own spelled test, and its template is fingerprinted above.
#[test]
fn aufx_stored_vocabulary_stands() {
    let names: Vec<&str> = FxKind::ALL.iter().map(|f| f.name()).collect();
    let want = [
        "Blur", "Glow", "Diffusion", "Grade", "Para", "Light wrap", "Rim light", "Vignette", "Grain", "Motion blur",
        "LUT", "Matte", "Palette map", "Tone",
    ];
    assert_eq!(names, want);
    let keys: Vec<&str> = CAMERA_SPECS.iter().map(|s| s.key).collect();
    let want = [
        "pan_x", "pan_y", "zoom", "rotate_deg", "shake_amp", "shake_freq", "shake_seed", "focus_z", "aperture", "parallax_k",
    ];
    assert_eq!(keys, want);
    let dock = Stage::Composite.dock();
    let panes: Vec<Pane> = dock.iter_all_tabs().map(|(_, p)| *p).collect();
    let want = [Pane::NodeGraph, Pane::Curves, Pane::Viewer(0), Pane::Canvas, Pane::Palette, Pane::RenderQueue];
    assert_eq!(panes, want);
    let ui = serde_json::to_value(crate::config::UiConfig::default()).expect("the section");
    let fields: Vec<&str> = ui.as_object().expect("a map").keys().map(String::as_str).collect();
    let want = [
        "exact_descriptions", "hints_off", "hints_seen", "keymap_rev", "lock_positions", "quick_start", "result_in_color",
        "scale", "sheet_in_draw", "show_welcome", "whats_new_seen",
    ];
    assert_eq!(fields, want, "the ui section's shape: `panel_more` stays out until something folds");
    let mut opened = crate::config::UiConfig::default();
    opened.set_camera_details(true);
    let stored = serde_json::to_value(&opened).expect("the section");
    assert_eq!(stored["panel_more"], serde_json::json!({"camera_details": true}), "a key in the map that was there");
    let store = include_str!("../../crates/anim-core/src/store.rs");
    assert!(store.contains("const SCHEMA_VERSION: i64 = 11;"), "the schema");

    // Tab stays the add verb over the graph, on the shipped step too.
    let mut h = film_room(1600.0, 900.0);
    h.point_at(pane_rect(&h, Pane::NodeGraph).center());
    h.key(Key::Tab);
    settle(&mut h);
    assert!(h.state().graph_ui.add_menu.is_some(), "{}", h.describe());
}
