//! GM17: the View3D pane — the cut's Model3D node seen through a free view
//! camera with Blender's navigation, drawn by the engine's CPU renderer
//! (half size while the view moves, full size when it rests). "Set cut
//! camera from view" keys the node's camera to what the view sees, as one
//! undo step.
//!
//! Navigation (Blender's): middle-drag orbits, Shift+middle-drag pans,
//! Ctrl+middle-drag and the wheel zoom, over the pane only. The numpad's
//! views are the toolbar's buttons (egui reads the numpad's digits as the
//! row's, and those are chords elsewhere: 0 is RAM preview).

use anim_core::color::ColorLaw;
use anim_core::fx::Image;
use anim_core::graph::NodeKind;
use anim_core::ids::NodeId;
use anim_core::model::Cut;
use anim_core::r3d::{self, Orbit, View};
use eframe::egui;

use crate::doc::AppState;

/// The widest render the pane draws (px): the CPU renderer runs on the UI
/// thread, and only when the view or the node changes.
pub const MAX_RENDER_W: u32 = 960;

/// GM17: the pane's session state — the view camera (framed on the node's
/// camera when first shown) and the last render, kept until something it
/// shows changes.
#[derive(Default)]
pub struct View3dState {
    pub orbit: Option<Orbit>,
    /// The size of the last render (px).
    pub rendered: Option<(u32, u32)>,
    texture: Option<(egui::TextureHandle, u64)>,
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

/// The toolbar's numpad buttons: label, Blender's key, verb.
const NAV_BUTTONS: &[(&str, &str, Nav)] = &[
    ("Front", "numpad 1 in Blender", Nav::View(View::Front)),
    ("Back", "Ctrl+numpad 1 in Blender", Nav::View(View::Back)),
    ("Right", "numpad 3 in Blender", Nav::View(View::Right)),
    ("Left", "Ctrl+numpad 3 in Blender", Nav::View(View::Left)),
    ("Top", "numpad 7 in Blender", Nav::View(View::Top)),
    ("Bottom", "Ctrl+numpad 7 in Blender", Nav::View(View::Bottom)),
    ("Camera", "numpad 0 in Blender: look through the cut camera", Nav::CameraView),
    ("Ortho", "numpad 5 in Blender: orthographic or perspective", Nav::ToggleOrtho),
];

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
/// toon look, outline and pass, the view's camera instead of the node's.
/// None when `node` is no Model3D or its model is missing or unreadable.
pub fn render_view(cut: &Cut, node: NodeId, orbit: &Orbit, frame: u32, (w, h): (u32, u32), law: ColorLaw) -> Option<Image> {
    let NodeKind::Model3D { model, params, binds } = &cut.graph.node(node).ok()?.kind else {
        return None;
    };
    let m = r3d::model_of(cut.models.get(model)?).ok()?;
    let v = cut.model_at(params, binds, frame);
    let mut scene = r3d::Scene3::default();
    scene.add_model(&m, &v.xform(), 1);
    let b = r3d::rasterize(&scene, &orbit.camera(), w, h);
    Some(r3d::shade(&b, &v, law))
}

/// What a render shows, as one key: the view, the frame, the size and the
/// node (its parameters and its model's content).
fn render_key(cut: &Cut, node: NodeId, orbit: &Orbit, frame: u32, size: (u32, u32)) -> u64 {
    let kind = cut.graph.node(node).map(|n| format!("{:?}", n.kind)).unwrap_or_default();
    let model = match cut.graph.node(node).map(|n| &n.kind) {
        Ok(NodeKind::Model3D { model, .. }) => cut.models.get(model).map_or(0, |m| m.content_hash),
        _ => 0,
    };
    let text = format!("{orbit:?}|{frame}|{size:?}|{kind}|{model}");
    anim_core::value::fnv1a(text.as_bytes())
}

/// The View3D pane.
pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    let node = model_node(state.cut(), state.selected_node);
    let frame = state.view.frame;
    let cut_camera = node.and_then(|id| match &state.cut().graph.node(id).ok()?.kind {
        NodeKind::Model3D { params, binds, .. } => Some(state.cut().model_at(params, binds, frame).camera()),
        _ => None,
    });
    if state.view3d.orbit.is_none() {
        state.view3d.orbit = Some(cut_camera.map(|c| Orbit::from_camera(&c)).unwrap_or_default());
    }
    let mut orbit = state.view3d.orbit.unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        for (label, key, nav) in NAV_BUTTONS {
            let on = *nav == Nav::ToggleOrtho && orbit.ortho;
            if ui.selectable_label(on, *label).on_hover_text(*key).clicked()
                && let Some(cam) = cut_camera.or(Some(orbit.camera()))
            {
                apply_nav(&mut orbit, *nav, &cam);
            }
        }
        ui.separator();
        let set = ui
            .add_enabled(node.is_some(), egui::Button::new("Set cut camera from view"))
            .on_hover_text("key the Model3D node's camera to this view at the current frame (one undo step)")
            .on_disabled_hover_text("no Model3D node — File ▸ Import 3D model…");
        if set.clicked()
            && let Some(id) = node
        {
            state.set_cut_camera_from_view(id, &orbit);
        }
    });
    let Some(node) = node else {
        state.view3d.orbit = Some(orbit);
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("no 3D model in this cut — File ▸ Import 3D model…")
                    .color(crate::plate::legend_dim()),
            );
        });
        return;
    };
    let avail = ui.available_size();
    let (rect, resp) = ui.allocate_exact_size(avail, egui::Sense::click_and_drag());
    // Blender's navigation, over the pane only.
    let mut navigating = false;
    if resp.dragged_by(egui::PointerButton::Middle) {
        let d = resp.drag_delta();
        let mods = ui.input(|i| i.modifiers);
        if mods.shift {
            orbit.pan(d.x, d.y, rect.height());
        } else if mods.ctrl {
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
    let (fw, fh) = (state.engine.project.width.max(1) as f32, state.engine.project.height.max(1) as f32);
    let size = render_size((rect.width(), rect.height()), fw / fh, navigating);
    let key = render_key(state.cut(), node, &orbit, frame, size);
    if state.view3d.texture.as_ref().map(|(_, k)| *k) != Some(key) {
        let law = state.engine.project.color_law;
        if let Some(img) = render_view(state.cut(), node, &orbit, frame, size, law) {
            let bytes: Vec<u8> = img.px.chunks_exact(4).flat_map(anim_core::export::straight_rgba8).collect();
            let color = egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &bytes);
            let tex = ui.ctx().load_texture("view3d", color, egui::TextureOptions::LINEAR);
            state.view3d.texture = Some((tex, key));
            state.view3d.rendered = Some(size);
        }
    }
    if navigating {
        ui.ctx().request_repaint();
    }
    ui.painter_at(rect).rect_filled(rect, 0.0, crate::plate::WELL);
    if let Some((tex, _)) = &state.view3d.texture {
        let (w, h) = (rect.width(), rect.height());
        let fit = (w / (fw / fh)).min(h);
        let shown = egui::Rect::from_center_size(rect.center(), egui::vec2(fit * fw / fh, fit));
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
        assert!(NAV_BUTTONS.iter().any(|b| b.2 == Nav::View(View::Bottom)), "every view has a button");
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
        let img = render_view(state.cut(), node, &Orbit::default(), 0, (64, 36), ColorLaw::Srgb).unwrap();
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
        let size = h.state().view3d.rendered.expect("a render");
        assert!(size.0 > 0 && size.1 > 0 && size.0 <= MAX_RENDER_W);
        let before = h.state().engine.history_len();
        h.click("Set cut camera from view");
        h.run_frames(2);
        assert_eq!(h.state().engine.history_len(), before + 1, "one undo step");
        // An empty cut says how to begin.
        let mut empty = EditorHarness::new();
        empty.editor_mut().dock = egui_dock::DockState::new(vec![Pane::View3D]);
        empty.run_frames(2);
        assert!(empty.has_text_containing("Import 3D model"), "{}", empty.describe());
    }
}
