//! GM17: the View3D pane — the cut's Model3D node seen through a free view
//! camera with Blender's navigation, drawn by the engine's CPU renderer
//! (half size while the view moves, full size when it rests). "Set cut
//! camera from view" keys the cut camera (every Model3D node shoots
//! through it) to what the view sees, as one undo step.
//!
//! Navigation (Blender's): middle-drag orbits, Shift+middle-drag pans,
//! Ctrl+middle-drag and the wheel zoom, over the pane only. A pen has no
//! middle button, so Alt+drag does the same (Alt+Shift pans, Alt+Ctrl
//! zooms). The numpad's verbs are actions over the pane (D6, `Ctx::View3D`):
//! 1 / 3 / 7 front, right and top, Ctrl the opposite side, 0 the cut
//! camera, 5 ortho, Home frames all, Ctrl+Alt+0 keys the cut camera. egui
//! reads the keypad's digits as the row's, so over View3D they shadow the
//! pencil box's digits, 0's RAM preview and Home's first frame. The
//! toolbar's buttons ask for the same verbs (one dispatch).

use anim_core::color::ColorLaw;
use anim_core::fx::Image;
use anim_core::graph::NodeKind;
use anim_core::ids::NodeId;
use anim_core::model::Cut;
use anim_core::r3d::{self, Orbit, View};
use eframe::egui;

use crate::config::Action;
use crate::doc::AppState;

/// The widest render the pane draws (px): the CPU renderer runs on the UI
/// thread, and only when the view or what it shows changes.
pub const MAX_RENDER_W: u32 = 960;

/// Why a View3D verb has nothing to act on.
pub const NO_MODEL: &str = "no Model3D node — File ▸ Import 3D model…";

/// Why the cut camera is not keyed from an orthographic view.
pub const ORTHO_REFUSAL: &str = "the cut camera is perspective — press Ortho (numpad 5) to return";

/// The pane's navigation, in a dim line under its toolbar.
pub const NAV_LEGEND: &str = "middle-drag or Alt+drag orbit · Shift pan · Ctrl or wheel zoom";

/// GM17: the pane's session state — the view camera (framed on the cut
/// camera when first shown) and the last render, kept until something it
/// shows changes.
#[derive(Default)]
pub struct View3dState {
    pub orbit: Option<Orbit>,
    /// The size of the last render (px).
    pub rendered: Option<(u32, u32)>,
    texture: Option<(egui::TextureHandle, u64)>,
    /// The verbs the toolbar asked for, run by the editor through the one
    /// dispatch after the dock draws (D6).
    pub asked: Vec<Action>,
}

/// A numpad verb (Blender's keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    View(View),
    /// Numpad 0: look through the cut camera.
    CameraView,
    /// Numpad 5: orthographic, or perspective again.
    ToggleOrtho,
}

/// The toolbar's numpad buttons: label, its keys (here and in Blender),
/// verb.
const NAV_BUTTONS: &[(&str, &str, Action)] = &[
    ("Front", "1 over View3D (numpad 1 in Blender)", Action::View3dFront),
    ("Back", "Ctrl+1 over View3D (Ctrl+numpad 1 in Blender)", Action::View3dBack),
    ("Right", "3 over View3D (numpad 3 in Blender)", Action::View3dRight),
    ("Left", "Ctrl+3 over View3D (Ctrl+numpad 3 in Blender)", Action::View3dLeft),
    ("Top", "7 over View3D (numpad 7 in Blender)", Action::View3dTop),
    ("Bottom", "Ctrl+7 over View3D (Ctrl+numpad 7 in Blender)", Action::View3dBottom),
    (
        "Camera",
        "look through the cut camera — 0 over View3D (numpad 0 in Blender)",
        Action::View3dCamera,
    ),
    (
        "Ortho",
        "orthographic or perspective — 5 over View3D (numpad 5 in Blender)",
        Action::View3dOrtho,
    ),
];

/// The numpad verb an action runs (None: it is no numpad verb).
pub fn nav_of(a: Action) -> Option<Nav> {
    Some(match a {
        Action::View3dFront => Nav::View(View::Front),
        Action::View3dBack => Nav::View(View::Back),
        Action::View3dRight => Nav::View(View::Right),
        Action::View3dLeft => Nav::View(View::Left),
        Action::View3dTop => Nav::View(View::Top),
        Action::View3dBottom => Nav::View(View::Bottom),
        Action::View3dCamera => Nav::CameraView,
        Action::View3dOrtho => Nav::ToggleOrtho,
        _ => return None,
    })
}

/// Apply a numpad verb to the view (`camera` is the cut camera, for 0).
pub fn apply_nav(orbit: &mut Orbit, nav: Nav, camera: &r3d::Camera3) {
    match nav {
        Nav::View(v) => orbit.set_view(v),
        Nav::CameraView => *orbit = Orbit::from_camera(camera),
        Nav::ToggleOrtho => orbit.ortho = !orbit.ortho,
    }
}

/// The node the pane shows: the selected node when it is a Model3D, else
/// the graph's first.
pub fn model_node(cut: &Cut, selected: Option<NodeId>) -> Option<NodeId> {
    let is_model = |id: NodeId| matches!(cut.graph.node(id).map(|n| &n.kind), Ok(NodeKind::Model3D { .. }));
    selected
        .filter(|id| is_model(*id))
        .or_else(|| cut.graph.nodes().map(|n| n.id).find(|id| is_model(*id)))
}

/// The cut camera at the current frame, as the shown node resolves it.
pub fn cut_camera(state: &AppState) -> Option<r3d::Camera3> {
    let node = model_node(state.cut(), state.selected_node)?;
    match &state.cut().graph.node(node).ok()?.kind {
        NodeKind::Model3D { params, binds, .. } => {
            Some(state.cut().model_at(params, binds, state.view.frame).camera())
        }
        _ => None,
    }
}

/// The pane's view — framed on the cut camera the first time.
fn view_of(state: &mut AppState) -> Orbit {
    if state.view3d.orbit.is_none() {
        state.view3d.orbit = Some(cut_camera(state).map(|c| Orbit::from_camera(&c)).unwrap_or_default());
    }
    state.view3d.orbit.unwrap_or_default()
}

/// The project frame's width per height.
fn frame_aspect(state: &AppState) -> f32 {
    let p = &state.engine.project;
    p.width.max(1) as f32 / p.height.max(1) as f32
}

/// A numpad verb on the pane's view (a view: the document is untouched).
pub fn perform_nav(state: &mut AppState, nav: Nav) {
    let mut orbit = view_of(state);
    let cam = cut_camera(state).unwrap_or_else(|| orbit.camera());
    apply_nav(&mut orbit, nav, &cam);
    state.view3d.orbit = Some(orbit);
}

/// The box around node `node`'s model, placed as at `frame`; None when it
/// is no Model3D or its model does not read.
pub fn model_bbox(cut: &Cut, node: NodeId, frame: u32) -> Option<(r3d::V3, r3d::V3)> {
    let NodeKind::Model3D { model, params, binds } = &cut.graph.node(node).ok()?.kind else {
        return None;
    };
    let m = r3d::model_of(cut.models.get(model)?).ok()?;
    r3d::placed_bbox(&m, &cut.model_at(params, binds, frame).xform())
}

/// "Frame all" (Blender's Home): the view closes on the whole model,
/// keeping its direction and lens.
pub fn frame_all(state: &mut AppState) {
    let Some(node) = model_node(state.cut(), state.selected_node) else {
        state.refuse(format!("refused — {NO_MODEL}"));
        return;
    };
    let Some(bbox) = model_bbox(state.cut(), node, state.view.frame) else {
        state.refuse("refused — the model does not read, or has no faces");
        return;
    };
    let mut orbit = view_of(state);
    orbit.frame(bbox, frame_aspect(state));
    state.view3d.orbit = Some(orbit);
    state.status = "View3D: the whole model framed".into();
}

/// "Set cut camera from view" (Blender's Ctrl+Alt+numpad 0): the cut camera
/// keyed to the view at the current frame, as one undo step. Refused with
/// no model, and in ortho (the cut camera is perspective).
pub fn set_camera_from_view(state: &mut AppState) {
    let Some(node) = model_node(state.cut(), state.selected_node) else {
        state.refuse(format!("refused — {NO_MODEL}"));
        return;
    };
    let orbit = view_of(state);
    if orbit.ortho {
        state.refuse(format!("refused — {ORTHO_REFUSAL}"));
        return;
    }
    state.set_cut_camera_from_view(node, &orbit);
}

/// The render's size for a pane `avail` (points) showing a frame of
/// `aspect` (w / h): fitted inside, at most [`MAX_RENDER_W`] across, half
/// that while the view moves.
pub fn render_size(avail: (f32, f32), aspect: f32, navigating: bool) -> (u32, u32) {
    let aspect = aspect.max(1e-3);
    let w = avail.0.min(avail.1 * aspect).clamp(1.0, MAX_RENDER_W as f32);
    let (w, h) = (w, w / aspect);
    let k = if navigating { 0.5 } else { 1.0 };
    (((w * k).round() as u32).max(1), ((h * k).round() as u32).max(1))
}

/// The node's picture through `orbit` at `frame`, `w × h`: its placement,
/// toon look, pass and ID chips (as the node renders), the view's camera
/// instead of the cut's, and the outline scaled from the project frame's
/// width `frame_w` to `w` (a line looks as thick as it will deliver). None
/// when `node` is no Model3D or its model is missing or unreadable.
pub fn render_view(
    cut: &Cut,
    node: NodeId,
    orbit: &Orbit,
    frame: u32,
    (w, h): (u32, u32),
    law: ColorLaw,
    frame_w: u32,
) -> Option<Image> {
    let NodeKind::Model3D { model, params, binds } = &cut.graph.node(node).ok()?.kind else {
        return None;
    };
    let m = r3d::model_of(cut.models.get(model)?).ok()?;
    let mut v = cut.model_at(params, binds, frame);
    let width = v.float("outline");
    if width > 0.0 {
        v.set_float("outline", width * w as f32 / frame_w.max(1) as f32);
    }
    let mut scene = r3d::Scene3::default();
    scene.add_model(&m, &v.xform(), r3d::first_object_id(cut, node));
    let b = r3d::rasterize(&scene, &orbit.camera(), w, h);
    Some(r3d::shade(&b, &v, law))
}

/// What a render shows, as one key: the view, the size, the law, the
/// project width (the outline's scale) and the node's recipe — its model's
/// content, its ID chips and its parameters resolved at the frame (a still
/// node keeps its render across frames).
fn render_key(cut: &Cut, node: NodeId, orbit: &Orbit, frame: u32, size: (u32, u32), law: ColorLaw, frame_w: u32) -> u64 {
    let recipe = match cut.graph.node(node).map(|n| &n.kind) {
        Ok(NodeKind::Model3D { model, params, binds }) => format!(
            "{model}#{:016x}|first={}|{}",
            cut.models.get(model).map_or(0, |m| m.content_hash),
            r3d::first_object_id(cut, node),
            cut.model_at(params, binds, frame).recipe()
        ),
        _ => String::new(),
    };
    let text = format!("{orbit:?}|{size:?}|{law:?}|{frame_w}|{recipe}");
    anim_core::value::fnv1a(text.as_bytes())
}

/// The View3D pane.
pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    let node = model_node(state.cut(), state.selected_node);
    let frame = state.view.frame;
    let mut orbit = view_of(state);
    ui.horizontal_wrapped(|ui| {
        for (label, keys, action) in NAV_BUTTONS {
            let on = *action == Action::View3dOrtho && orbit.ortho;
            if ui.selectable_label(on, *label).on_hover_text(*keys).clicked() {
                state.view3d.asked.push(*action);
            }
        }
        let all = ui
            .add_enabled(node.is_some(), egui::Button::new("Frame all"))
            .on_hover_text("frame the whole model, keeping the view's direction — Home over View3D (Home in Blender)")
            .on_disabled_hover_text(NO_MODEL);
        if all.clicked() {
            state.view3d.asked.push(Action::View3dFrameAll);
        }
        ui.separator();
        let set = ui
            .add_enabled(node.is_some() && !orbit.ortho, egui::Button::new("Set cut camera from view"))
            .on_hover_text(
                "key the cut camera to this view at the current frame (one undo step) — Ctrl+Alt+0 \
                 over View3D (Ctrl+Alt+numpad 0 in Blender)",
            )
            .on_disabled_hover_text(if node.is_none() { NO_MODEL } else { ORTHO_REFUSAL });
        if set.clicked() {
            state.view3d.asked.push(Action::SetCutCameraFromView);
        }
    });
    let Some(node) = node else {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("no 3D model in this cut — File ▸ Import 3D model…")
                    .color(crate::plate::legend_dim()),
            );
        });
        return;
    };
    ui.label(egui::RichText::new(NAV_LEGEND).small().color(crate::plate::legend_dim()));
    let avail = ui.available_size();
    let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
    // Blender's navigation, over the pane only; Alt+drag stands in for the
    // middle button a pen does not have.
    let mut navigating = false;
    let mods = ui.input(|i| i.modifiers);
    if resp.dragged_by(egui::PointerButton::Middle) || (mods.alt && resp.dragged_by(egui::PointerButton::Primary)) {
        let d = resp.drag_delta();
        if mods.shift {
            orbit.pan(d.x, d.y, rect.height());
        } else if mods.command {
            orbit.zoom(-d.y / 20.0);
        } else {
            orbit.orbit(d.x, d.y);
        }
        navigating = true;
    }
    if resp.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            orbit.zoom(wheel / 50.0);
            navigating = true;
        }
    }
    state.view3d.orbit = Some(orbit);
    let (fw, fh) = (state.engine.project.width.max(1), state.engine.project.height.max(1));
    let aspect = fw as f32 / fh as f32;
    let size = render_size((rect.width(), rect.height()), aspect, navigating);
    let law = state.engine.project.color_law;
    let key = render_key(state.cut(), node, &orbit, frame, size, law, fw);
    if state.view3d.texture.as_ref().map(|(_, k)| *k) != Some(key)
        && let Some(img) = render_view(state.cut(), node, &orbit, frame, size, law, fw)
    {
        let bytes: Vec<u8> = img.px.chunks_exact(4).flat_map(anim_core::export::straight_rgba8).collect();
        let color = egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &bytes);
        match &mut state.view3d.texture {
            // The one texture, its pixels replaced in place.
            Some((tex, k)) => {
                tex.set(color, egui::TextureOptions::LINEAR);
                *k = key;
            }
            None => {
                let tex = ui.ctx().load_texture("view3d", color, egui::TextureOptions::LINEAR);
                state.view3d.texture = Some((tex, key));
            }
        }
        state.view3d.rendered = Some(size);
    }
    if navigating {
        ui.ctx().request_repaint();
    }
    ui.painter_at(rect).rect_filled(rect, 0.0, crate::plate::WELL);
    if let Some((tex, _)) = &state.view3d.texture {
        let (w, h) = (rect.width(), rect.height());
        let fit = (w / aspect).min(h);
        let shown = egui::Rect::from_center_size(rect.center(), egui::vec2(fit * aspect, fit));
        let image = egui::Image::new(egui::load::SizedTexture::new(tex.id(), shown.size()));
        ui.put(shown, image);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::EditorHarness;
    use crate::workspace::Pane;

    const CUBE_OBJ: &str = "o cube\nv -1 -1 -1\nv 1 -1 -1\nv 1 1 -1\nv -1 1 -1\nv -1 -1 1\nv 1 -1 1\nv 1 1 1\nv -1 1 1\n\
        f 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

    /// A project with the cube imported from a file in `dir`.
    fn with_cube(dir: &std::path::Path) -> (AppState, NodeId) {
        let mut state = AppState::new_project("view3d", 1920, 1080, 24, 300.0);
        let path = dir.join("cube.obj");
        std::fs::write(&path, CUBE_OBJ).unwrap();
        let node = state.import_model(&path, (40.0, 40.0)).expect("the cube imports");
        (state, node)
    }

    /// Where pane `pane` was drawn last frame.
    fn pane_rect(h: &EditorHarness, pane: Pane) -> egui::Rect {
        let r = h.editor().pane_rects.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r);
        r.unwrap_or_else(|| panic!("{pane:?} is docked"))
    }

    #[test]
    fn view3d_numpad_views_turn_the_view_exactly_and_zero_looks_through_the_cut_camera() {
        let cam = r3d::ModelValues::of(&r3d::default_params()).camera();
        let mut o = Orbit::default();
        o.orbit(30.0, 10.0);
        apply_nav(&mut o, Nav::View(View::Top), &cam);
        let b = o.basis();
        assert!((b.forward[1] + 1.0).abs() < 1e-6 && (b.up[2] + 1.0).abs() < 1e-6, "{b:?}");
        apply_nav(&mut o, Nav::View(View::Right), &cam);
        assert!((o.basis().forward[0] + 1.0).abs() < 1e-6);
        apply_nav(&mut o, Nav::ToggleOrtho, &cam);
        assert!(o.ortho && o.camera().ortho);
        apply_nav(&mut o, Nav::CameraView, &cam);
        let eye = o.eye();
        assert!((0..3).all(|k| (eye[k] - cam.eye[k]).abs() < 1e-4), "{eye:?} vs {:?}", cam.eye);
        assert!(
            NAV_BUTTONS.iter().any(|b| nav_of(b.2) == Some(Nav::View(View::Bottom))),
            "every view has a button"
        );
    }

    #[test]
    fn view3d_renders_at_half_size_while_navigating_and_fits_the_frame() {
        let full = render_size((800.0, 600.0), 16.0 / 9.0, false);
        assert_eq!(full, (800, 450));
        assert_eq!(render_size((800.0, 600.0), 16.0 / 9.0, true), (400, 225));
        assert_eq!(render_size((4000.0, 3000.0), 16.0 / 9.0, false).0, MAX_RENDER_W);
        assert_eq!(render_size((300.0, 100.0), 16.0 / 9.0, false), (178, 100));
    }

    #[test]
    fn view3d_import_is_one_undo_step_and_an_unreadable_file_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, node) = with_cube(dir.path());
        assert_eq!(state.cut().models.len(), 1);
        assert_eq!(model_node(state.cut(), None), Some(node));
        let img = render_view(state.cut(), node, &Orbit::default(), 0, (64, 36), ColorLaw::Srgb, 1920).unwrap();
        assert!(img.px.chunks_exact(4).any(|p| p[3] > 0.0), "the view shows the cube");
        state.engine.undo().unwrap();
        assert!(state.cut().models.is_empty() && state.cut().graph.node(node).is_err(), "one step");
        let bad = dir.path().join("broken.obj");
        std::fs::write(&bad, "# no faces\n").unwrap();
        let len = state.engine.history_len();
        assert_eq!(state.import_model(&bad, (0.0, 0.0)), None);
        assert_eq!(state.import_model(&dir.path().join("model.fbx"), (0.0, 0.0)), None);
        assert_eq!(state.engine.history_len(), len, "refused: nothing changes");
    }

    #[test]
    fn view3d_set_cut_camera_from_view_keys_the_node_in_one_undo_step() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, node) = with_cube(dir.path());
        state.view.frame = 4;
        let mut view = Orbit::default();
        view.orbit(-50.0, 20.0);
        let before = state.engine.history_len();
        state.set_cut_camera_from_view(node, &view);
        assert_eq!(state.engine.history_len(), before + 1, "{}", state.status);
        let NodeKind::Model3D { params, binds, .. } = state.cut().graph.node(node).unwrap().kind.clone() else {
            unreachable!()
        };
        let cam = state.cut().model_at(&params, &binds, 4).camera();
        let eye = view.eye();
        assert!((0..3).all(|k| (cam.eye[k] - eye[k]).abs() < 1e-4), "{:?} vs {eye:?}", cam.eye);
        assert_eq!(state.cut().xsheet.params.len(), r3d::CAMERA_KEYS.len(), "a column per camera parameter");
        state.engine.undo().unwrap();
        assert!(state.cut().xsheet.params.is_empty());
    }

    #[test]
    fn view3d_pane_draws_the_model_and_its_button_keys_the_camera() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _node) = with_cube(dir.path());
        let mut h = EditorHarness::with_state(state);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::View3D]);
        h.run_frames(3);
        assert!(h.has("Set cut camera from view"), "{}", h.describe());
        assert!(h.has("Frame all") && h.has_text_containing(NAV_LEGEND), "{}", h.describe());
        let size = h.state().view3d.rendered.expect("a render");
        assert!(size.0 > 0 && size.1 > 0 && size.0 <= MAX_RENDER_W);
        let before = h.state().engine.history_len();
        h.click("Set cut camera from view");
        h.run_frames(2);
        assert_eq!(h.state().engine.history_len(), before + 1, "one undo step");
        // In ortho the button stands down and says why; the verb refuses.
        h.click("Ortho");
        h.run_frames(2);
        assert!(h.state().view3d.orbit.is_some_and(|o| o.ortho));
        assert_eq!(h.node_enabled("Set cut camera from view"), Some(false));
        assert_eq!(h.editor().can_perform(Action::SetCutCameraFromView), Err(ORTHO_REFUSAL));
        let len = h.state().engine.history_len();
        assert_eq!(h.perform(Action::SetCutCameraFromView), crate::Performed::Ignored);
        assert_eq!(h.state().engine.history_len(), len, "nothing keyed from an ortho view");
        // An empty cut says how to begin.
        let mut empty = EditorHarness::new();
        empty.editor_mut().dock = egui_dock::DockState::new(vec![Pane::View3D]);
        empty.run_frames(2);
        assert!(empty.has_text_containing("Import 3D model"), "{}", empty.describe());
    }

    #[test]
    fn view3d_numpad_keys_drive_the_view_over_the_pane_and_stay_the_old_keys_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _node) = with_cube(dir.path());
        let mut h = EditorHarness::with_state(state);
        let mut dock = egui_dock::DockState::new(vec![Pane::View3D]);
        dock.main_surface_mut().split_right(egui_dock::NodeIndex::root(), 0.5, vec![Pane::XSheet]);
        h.editor_mut().dock = dock;
        h.run_frames(3);
        h.point_at(pane_rect(&h, Pane::View3D).center());
        let view = |h: &EditorHarness| h.state().view3d.orbit.expect("the pane's view");
        let ctrl = egui::Modifiers::COMMAND;
        for (mods, key, yaw, pitch) in [
            (egui::Modifiers::NONE, egui::Key::Num7, 0.0, 90.0),
            (ctrl, egui::Key::Num7, 0.0, -90.0),
            (egui::Modifiers::NONE, egui::Key::Num3, 90.0, 0.0),
            (ctrl, egui::Key::Num3, -90.0, 0.0),
            (ctrl, egui::Key::Num1, 180.0, 0.0),
            (egui::Modifiers::NONE, egui::Key::Num1, 0.0, 0.0),
        ] {
            h.key_mods(mods, key);
            let o = view(&h);
            assert_eq!((o.yaw_deg, o.pitch_deg), (yaw, pitch), "{mods:?} {key:?}");
        }
        h.key(egui::Key::Num5);
        assert!(view(&h).ortho, "5 is ortho");
        h.key(egui::Key::Num5);
        assert!(!view(&h).ortho, "and perspective again");
        // 0 looks through the cut camera (not the RAM preview); Home frames
        // the model (not the first frame).
        h.editor_mut().state.goto(3);
        let mut moved = view(&h);
        moved.orbit(80.0, -30.0);
        h.state_mut().view3d.orbit = Some(moved);
        h.key(egui::Key::Num0);
        let cam = cut_camera(h.state()).expect("the cut camera");
        let eye = view(&h).eye();
        assert!((0..3).all(|k| (eye[k] - cam.eye[k]).abs() < 1e-3), "{eye:?} vs {:?}", cam.eye);
        assert!(!h.state().ram.busy(), "0 over View3D is not the RAM preview");
        let before = view(&h);
        h.key(egui::Key::Home);
        assert_eq!(h.state().view.frame, 3, "Home over View3D keeps the frame");
        let after = view(&h);
        assert_eq!((after.yaw_deg, after.pitch_deg), (before.yaw_deg, before.pitch_deg), "the direction kept");
        // Ctrl+Alt+0 keys the cut camera, one undo step.
        let len = h.state().engine.history_len();
        h.key_mods(egui::Modifiers::COMMAND | egui::Modifiers::ALT, egui::Key::Num0);
        assert_eq!(h.state().engine.history_len(), len + 1, "{}", h.state().refusal);
        // Over the sheet the keys are their old verbs, and the view stays.
        h.point_at(pane_rect(&h, Pane::XSheet).center());
        let kept = view(&h);
        h.key(egui::Key::Num7);
        assert_eq!(view(&h), kept, "7 over the sheet leaves the view alone");
        let last = h.performed().last().map(|(a, _)| *a);
        assert_eq!(last, Some(Action::Preset7), "{:?}", h.performed());
        h.key(egui::Key::Home);
        assert_eq!(h.state().view.frame, 0, "Home over the sheet is the first frame");
        // The bindings hold no clash in either preset.
        for &km in crate::config::Keymap::ALL {
            assert_eq!(crate::config::clashes(&crate::config::default_binds(km)), vec![], "{km:?}");
        }
    }

    #[test]
    fn view3d_alt_drag_navigates_with_a_pen_and_frame_all_shows_the_whole_model() {
        let dir = tempfile::tempdir().unwrap();
        let (state, node) = with_cube(dir.path());
        let mut h = EditorHarness::with_state(state);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::View3D]);
        h.run_frames(3);
        let c = pane_rect(&h, Pane::View3D).center();
        let start = h.state().view3d.orbit.expect("the pane's view");
        // A plain pen drag does nothing; Alt+drag orbits.
        h.drag(c, c + egui::vec2(60.0, 0.0), 4);
        assert_eq!(h.state().view3d.orbit, Some(start), "a plain drag leaves the view");
        h.set_mods(egui::Modifiers::ALT);
        h.drag(c, c + egui::vec2(60.0, 0.0), 4);
        h.set_mods(egui::Modifiers::NONE);
        let turned = h.state().view3d.orbit.unwrap();
        assert!(turned.yaw_deg != start.yaw_deg && turned.target == start.target, "{turned:?}");
        // Alt+Shift pans (the target moves), Alt+Ctrl zooms (the distance).
        h.set_mods(egui::Modifiers::ALT | egui::Modifiers::SHIFT);
        h.drag(c, c + egui::vec2(40.0, 10.0), 4);
        let panned = h.state().view3d.orbit.unwrap();
        assert!(panned.target != turned.target && panned.yaw_deg == turned.yaw_deg, "{panned:?}");
        h.set_mods(egui::Modifiers::ALT | egui::Modifiers::COMMAND);
        h.drag(c, c + egui::vec2(0.0, -40.0), 4);
        h.set_mods(egui::Modifiers::NONE);
        let zoomed = h.state().view3d.orbit.unwrap();
        assert!(zoomed.distance < panned.distance, "{zoomed:?}");
        // Frame all: the whole cube in the frame, the direction kept.
        h.click("Frame all");
        h.run_frames(2);
        let framed = h.state().view3d.orbit.unwrap();
        assert_eq!((framed.yaw_deg, framed.pitch_deg), (zoomed.yaw_deg, zoomed.pitch_deg));
        let img = render_view(h.state().cut(), node, &framed, 0, (96, 54), ColorLaw::Srgb, 1920).unwrap();
        let covered = |x: u32, y: u32| img.px[((y * 96 + x) * 4 + 3) as usize] > 0.0;
        assert!(covered(48, 27), "the model is in the middle");
        assert!((0..96).all(|x| !covered(x, 0) && !covered(x, 53)), "and nowhere cut by the frame's edge");
        assert!((0..54).all(|y| !covered(0, y) && !covered(95, y)));
    }

    #[test]
    fn view3d_draws_the_outline_at_its_own_scale_of_the_frame() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, node) = with_cube(dir.path());
        let NodeKind::Model3D { model, mut params, binds } = state.cut().graph.node(node).unwrap().kind.clone() else {
            unreachable!()
        };
        params.0.insert("outline".into(), anim_core::fx::FxValue::Float(8.0));
        state.graph_set_kind(node, NodeKind::Model3D { model, params, binds });
        let ink = |img: &Image| {
            let c = ColorLaw::Srgb.premultiplied([40, 32, 40, 255]);
            img.px.chunks_exact(4).filter(|p| (0..4).all(|k| (p[k] - c[k]).abs() < 1e-6)).count()
        };
        let o = Orbit::default();
        let size = (192, 108);
        let at_frame = render_view(state.cut(), node, &o, 0, size, ColorLaw::Srgb, 192).unwrap();
        let at_half = render_view(state.cut(), node, &o, 0, size, ColorLaw::Srgb, 384).unwrap();
        // At half the project's width the 8 px line draws 4 px wide: as the
        // node's own 4 px line does at its full size.
        let NodeKind::Model3D { model, mut params, binds } = state.cut().graph.node(node).unwrap().kind.clone() else {
            unreachable!()
        };
        params.0.insert("outline".into(), anim_core::fx::FxValue::Float(4.0));
        state.graph_set_kind(node, NodeKind::Model3D { model, params, binds });
        let four = render_view(state.cut(), node, &o, 0, size, ColorLaw::Srgb, 192).unwrap();
        assert!(ink(&at_half) < ink(&at_frame), "{} vs {}", ink(&at_half), ink(&at_frame));
        assert_eq!(at_half.px, four.px, "half the frame, half the line");
    }

    #[test]
    fn view3d_import_wires_the_model_under_the_composite_in_one_undo_step() {
        use anim_core::graph::BlendMode;
        let dir = tempfile::tempdir().unwrap();
        let (mut state, node) = with_cube(dir.path());
        // No graph before: the sheet's composite is built, the model under it.
        let out = state.cut().graph.output.expect("an Output");
        let feed = state.cut().graph.node(out).unwrap().inputs[0].expect("the Output is fed").0;
        let blend = state.cut().graph.node(feed).unwrap().clone();
        assert!(matches!(blend.kind, NodeKind::Blend { mode: BlendMode::Normal }), "{:?}", blend.kind);
        assert_eq!(blend.inputs[0].map(|p| p.0), Some(node), "the model below");
        let top = blend.inputs[1].expect("the drawings above").0;
        assert!(matches!(state.cut().graph.node(top).unwrap().kind, NodeKind::DrawingSource { .. }));
        assert!(state.status.contains("under the composite"), "{}", state.status);
        // A second model goes under the composite as it now stands.
        let path = dir.path().join("cube.obj");
        let second = state.import_model(&path, (40.0, 200.0)).unwrap();
        let feed2 = state.cut().graph.node(out).unwrap().inputs[0].unwrap().0;
        let b2 = state.cut().graph.node(feed2).unwrap().clone();
        assert_eq!((b2.inputs[0].map(|p| p.0), b2.inputs[1].map(|p| p.0)), (Some(second), Some(feed)));
        // One undo step takes the model, its blend and its wiring back.
        state.engine.undo().unwrap();
        assert_eq!(state.cut().graph.node(out).unwrap().inputs[0].map(|p| p.0), Some(feed));
        assert!(state.cut().graph.node(second).is_err() && state.cut().graph.node(feed2).is_err());
        // A hand-built graph with no Output stays as it is, and says so.
        let mut bare = AppState::new_project("bare", 1920, 1080, 24, 300.0);
        bare.graph_add_node(NodeKind::Blend { mode: BlendMode::Normal }, (0.0, 0.0));
        let lone = bare.import_model(&path, (0.0, 90.0)).unwrap();
        assert!(bare.cut().graph.output.is_none());
        assert!(bare.cut().graph.nodes().all(|n| n.inputs.iter().flatten().all(|p| p.0 != lone)));
        assert!(bare.status.contains("connect it to the Output in the node graph"), "{}", bare.status);
    }

    #[test]
    fn view3d_a_relative_sequence_renders_through_the_cpu_worker_job_from_the_project_folder() {
        use crate::graphcomp::{FxJob, FxKey, job_cut, render_job};
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("renders")).unwrap();
        let mut red = Image::new(16, 9);
        for p in red.px.chunks_exact_mut(4) {
            p.copy_from_slice(&[1.0, 0.0, 0.0, 1.0]);
        }
        anim_core::deliver::write_png8(&dir.path().join("renders").join("shot_0001.png"), &red).unwrap();
        let mut state = AppState::new_project("seq", 16, 9, 24, 300.0);
        state.graph_add_node(NodeKind::image_sequence("renders/shot_####.png"), (0.0, 0.0));
        let node = state.selected_node.expect("the sequence node");
        // The project saved in `dir`: its folder is where the frames read.
        state.file_path = Some(dir.path().join("seq.animproj"));
        assert_eq!(state.project_dir().as_deref(), Some(dir.path()));
        let job = |base_dir| FxJob {
            key: FxKey { node, frame: 0, hash: 1 },
            generation: 0,
            cut: job_cut(state.cut(), node, 0),
            law: state.engine.project.color_law,
            width: 16,
            height: 9,
            base_dir,
        };
        let done = render_job(&job(state.project_dir()));
        assert!(done.warnings.is_empty(), "{:?}", done.warnings);
        let px = done.texels.expect("rendered");
        let at = |i: usize| anim_core::raster::f16_bits_to_f32(px[(4 * 16 + 8) * 4 + i]);
        assert!(at(0) > 0.99 && at(1) < 0.01 && at(3) > 0.99, "frame 1 of the run, read from the folder");
        // Unsaved, the relative run is refused with the reason, once.
        let done = render_job(&job(None));
        assert_eq!(done.warnings, vec![r3d::NO_FOLDER.to_string()]);
        let px = done.texels.expect("rendered");
        assert!(px.iter().all(|b| anim_core::raster::f16_bits_to_f32(*b) == 0.0), "transparent");
        // The status lane says it once.
        state.note_render_warnings(done.warnings.clone());
        assert_eq!(state.status, r3d::NO_FOLDER);
        // "pick a frame…": the picked file names its run from 1, relative
        // to the project's folder, in one undo step.
        let len = state.engine.history_len();
        state.set_sequence_from_file(node, &dir.path().join("renders").join("shot_0001.png"));
        assert_eq!(state.engine.history_len(), len + 1);
        let NodeKind::ImageSequence { pattern, first, .. } = &state.cut().graph.node(node).unwrap().kind else {
            unreachable!()
        };
        assert_eq!((pattern.as_str(), *first), ("renders/shot_####.png", 1));
    }

    #[test]
    fn view3d_import_embeds_every_material_library_a_line_names_and_says_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.mtl"), "newmtl red\nKd 1 0 0\n").unwrap();
        let obj = format!("mtllib a.mtl  b.mtl\nusemtl red\n{CUBE_OBJ}");
        let path = dir.path().join("lib cube.obj");
        std::fs::write(&path, obj).unwrap();
        let mut state = AppState::new_project("mtl", 1920, 1080, 24, 300.0);
        let node = state.import_model(&path, (0.0, 0.0)).expect("imports");
        assert!(state.status.contains("no material library b.mtl"), "{}", state.status);
        let NodeKind::Model3D { model, .. } = &state.cut().graph.node(node).unwrap().kind else {
            unreachable!()
        };
        let bytes = String::from_utf8_lossy(&state.cut().models.get(model).unwrap().bytes).into_owned();
        assert!(bytes.contains("Kd 1 0 0"), "a.mtl travels inside the project");
    }
}
