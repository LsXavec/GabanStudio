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
//!
//! GM22: the pane's render and the canvas's 3D underlay run on a worker,
//! one keyed job at a time ([`JobLogic`], FinalLook's law): the last picture
//! stays up with the TALLY dot until the next one lands.

use std::path::PathBuf;
use std::sync::mpsc;

use anim_core::color::ColorLaw;
use anim_core::fx::Image;
use anim_core::graph::NodeKind;
use anim_core::ids::NodeId;
use anim_core::model::{Cut, ModelAsset};
use anim_core::r3d::{self, Orbit, View};
use eframe::egui;

use crate::config::Action;
use crate::doc::AppState;

/// The widest render the pane (and the underlay) draws (px): the CPU
/// renderer runs on a worker, and only when the view or what it shows
/// changes.
pub const MAX_RENDER_W: u32 = 960;

/// Why a job ended with no picture and no reason: its worker panicked (the
/// channel closed before it answered).
pub const WORKER_LOST: &str = "the render stopped before it finished";

/// Why a View3D verb has nothing to act on.
pub const NO_MODEL: &str = "no 3D model yet — File ▸ Import ▸ Import 3D model…";

/// Why the cut camera is not keyed from an orthographic view.
pub const ORTHO_REFUSAL: &str = "the shot camera is perspective — press Ortho (numpad 5) to return";

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
    /// GM22: the render's worker and its texture.
    pub worker: Worker3d,
    /// The verbs the toolbar asked for, run by the editor through the one
    /// dispatch after the dock draws (D6).
    pub asked: Vec<Action>,
}

impl View3dState {
    /// Tests: block until the pane's job is in (it lands next frame).
    #[cfg(test)]
    pub fn wait(&mut self) {
        self.worker.wait();
    }
}

/// GM22: one keyed job at a time, the law the View3D pane and the 3D
/// underlay share (FinalLook's): a key unlike the picture's starts a job
/// unless one runs; then only the newest key waits, and starts when the
/// running job lands (a late picture still replaces the old one). A key
/// that failed is said once and never resubmitted.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct JobLogic {
    /// The key of the picture shown.
    shown: Option<u64>,
    /// The key of the job running.
    pending: Option<u64>,
    /// The newest key asked for.
    wanted: Option<u64>,
    /// The last key that failed.
    failed: Option<u64>,
    /// The last reason said (the same one is not said twice in a row).
    said: Option<String>,
    /// Jobs started so far.
    pub started: u32,
}

impl JobLogic {
    /// Ask for `key`'s picture; true when its job is to start now.
    pub fn want(&mut self, key: u64) -> bool {
        self.wanted = Some(key);
        self.next() == Some(key)
    }

    /// Start the newest key when nothing runs and it has no picture (and
    /// did not fail): the key whose job is to start.
    pub fn next(&mut self) -> Option<u64> {
        let key = self.wanted?;
        if self.pending.is_some() || self.shown == Some(key) || self.failed == Some(key) {
            return None;
        }
        self.pending = Some(key);
        self.started += 1;
        Some(key)
    }

    /// The running job ended: its picture (Ok) is now shown, or it failed —
    /// the reason to say, unless it was just said.
    pub fn land(&mut self, result: Result<(), String>) -> Option<String> {
        let key = self.pending.take()?;
        match result {
            Ok(()) => {
                self.shown = Some(key);
                self.said = None;
                None
            }
            Err(why) => {
                self.failed = Some(key);
                (self.said.as_ref() != Some(&why)).then(|| {
                    self.said = Some(why.clone());
                    why
                })
            }
        }
    }

    /// The worker went away without answering (it panicked): a failure.
    pub fn lost(&mut self) -> Option<String> {
        self.land(Err(WORKER_LOST.to_string()))
    }

    /// The TALLY dot: a job runs, or the newest key's picture is to come.
    pub fn busy(&self) -> bool {
        self.pending.is_some() || self.wanted.is_some_and(|k| self.shown != Some(k) && self.failed != Some(k))
    }

    /// The key of the picture shown.
    pub fn shown(&self) -> Option<u64> {
        self.shown
    }

    /// Tests: the key of the job running.
    #[cfg(test)]
    pub fn pending(&self) -> Option<u64> {
        self.pending
    }
}

/// What [`Worker3d::poll`] took in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landed {
    /// A picture, now the texture (its size, px).
    Picture([usize; 2]),
    /// Why the job failed, to say once.
    Failed(String),
}

/// GM22: a [`JobLogic`] and its job on a worker thread (FinalLook's
/// pattern): the picture lands in the one texture, kept until the next.
#[derive(Default)]
pub struct Worker3d {
    pub jobs: JobLogic,
    answer: Option<mpsc::Receiver<Result<egui::ColorImage, String>>>,
    tex: Option<egui::TextureHandle>,
}

impl Worker3d {
    /// Take the running job's answer when it is in: a picture replaces the
    /// texture's pixels; a failure, or a worker gone without answering, is
    /// said once. None while it renders (or nothing runs).
    pub fn poll(&mut self, ctx: &egui::Context, name: &str) -> Option<Landed> {
        let got = match self.answer.as_ref()?.try_recv() {
            Ok(got) => got,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.answer = None;
                return self.jobs.lost().map(Landed::Failed);
            }
        };
        self.answer = None;
        match got {
            Ok(img) => {
                let size = img.size;
                match &mut self.tex {
                    Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
                    None => self.tex = Some(ctx.load_texture(name, img, egui::TextureOptions::LINEAR)),
                }
                self.jobs.land(Ok(()));
                Some(Landed::Picture(size))
            }
            Err(why) => self.jobs.land(Err(why)).map(Landed::Failed),
        }
    }

    /// Run the job [`JobLogic::want`] started: `render` on its own thread,
    /// and a repaint however it ends (its answer, or a panic that closes
    /// the channel unanswered), so the UI takes it in.
    pub fn start(&mut self, ctx: &egui::Context, render: impl FnOnce() -> Result<Image, String> + Send + 'static) {
        /// The job's end of the channel: dropped — answered, or unwound by
        /// a panic — it closes the channel, then asks for the repaint.
        struct Reply {
            tx: Option<mpsc::Sender<Result<egui::ColorImage, String>>>,
            ctx: egui::Context,
        }
        impl Drop for Reply {
            fn drop(&mut self) {
                drop(self.tx.take());
                self.ctx.request_repaint();
            }
        }
        let (tx, rx) = mpsc::channel();
        self.answer = Some(rx);
        let reply = Reply { tx: Some(tx), ctx: ctx.clone() };
        std::thread::spawn(move || {
            let answer = render().map(|img| {
                let bytes: Vec<u8> = img.px.chunks_exact(4).flat_map(anim_core::export::straight_rgba8).collect();
                egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &bytes)
            });
            if let Some(tx) = &reply.tx {
                let _ = tx.send(answer);
            }
        });
    }

    /// The last picture (None before the first lands).
    pub fn texture(&self) -> Option<&egui::TextureHandle> {
        self.tex.as_ref()
    }

    /// Tests: block until the running job answers (no sleep, no clock); its
    /// answer stays for the next poll.
    #[cfg(test)]
    pub fn wait(&mut self) {
        let Some(rx) = self.answer.take() else {
            return;
        };
        let (tx, again) = mpsc::channel();
        if let Ok(got) = rx.recv() {
            let _ = tx.send(got);
        }
        // A worker gone without answering stays gone (the channel closed).
        self.answer = Some(again);
    }
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
        "look through the shot camera — 0 over View3D (numpad 0 in Blender)",
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
/// when `node` is no Model3D or its model is missing or unreadable. GM22:
/// the pane renders through [`view_job`] on its worker; this is the same
/// render in one call, for tests.
#[cfg(test)]
pub fn render_view(
    cut: &Cut,
    node: NodeId,
    orbit: &Orbit,
    frame: u32,
    size: (u32, u32),
    law: ColorLaw,
    frame_w: u32,
) -> Option<Image> {
    view_job(cut, node, orbit, frame, size, law, frame_w)?.render().ok()
}

/// GM22: one View3D render with all it reads taken off the document, for
/// the worker: the model (its bytes shared, not copied), the node's values
/// at the frame with the outline scaled, the view's camera.
pub struct ViewJob {
    model: ModelAsset,
    values: r3d::ModelValues,
    first: u32,
    camera: r3d::Camera3,
    size: (u32, u32),
    law: ColorLaw,
}

/// The job [`render_view`] runs; None when `node` is no Model3D or its
/// model is missing.
pub fn view_job(
    cut: &Cut,
    node: NodeId,
    orbit: &Orbit,
    frame: u32,
    (w, h): (u32, u32),
    law: ColorLaw,
    frame_w: u32,
) -> Option<ViewJob> {
    let NodeKind::Model3D { model, params, binds } = &cut.graph.node(node).ok()?.kind else {
        return None;
    };
    let mut values = cut.model_at(params, binds, frame);
    let width = values.float("outline");
    if width > 0.0 {
        values.set_float("outline", width * w as f32 / frame_w.max(1) as f32);
    }
    Some(ViewJob {
        model: cut.models.get(model)?.clone(),
        values,
        first: r3d::first_object_id(cut, node),
        camera: orbit.camera(),
        size: (w, h),
        law,
    })
}

impl ViewJob {
    /// The picture, or why the model does not read.
    pub fn render(&self) -> Result<Image, String> {
        let m = r3d::model_of(&self.model).map_err(|e| format!("3D model {}: {e}", self.model.name))?;
        let mut scene = r3d::Scene3::default();
        scene.add_model(&m, &self.values.xform(), self.first);
        let b = r3d::rasterize(&scene, &self.camera, self.size.0, self.size.1);
        Ok(r3d::shade(&b, &self.values, self.law))
    }
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
            .add_enabled(node.is_some() && !orbit.ortho, egui::Button::new("Set shot camera from view"))
            .on_hover_text(
                "key the cut camera to this view at the current frame (one undo step) — Ctrl+Alt+0 \
                 over View3D (Ctrl+Alt+numpad 0 in Blender)",
            )
            .on_disabled_hover_text(if node.is_none() { NO_MODEL } else { ORTHO_REFUSAL });
        if set.clicked() {
            state.view3d.asked.push(Action::SetCutCameraFromView);
        }
        // AU15: the key legend is all it folds, and no model has no view to
        // take keys.
        if node.is_some() {
            crate::plate::more_tag(ui, &mut state.panel_more, crate::plate::MORE_3D, crate::vocab::MENU_MORE);
        }
    });
    let Some(node) = node else {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("No 3D model in this shot — File ▸ Import ▸ Import 3D model…")
                    .color(crate::plate::legend_dim()),
            );
        });
        return;
    };
    // AU15: the view's key legend is what this panel has to disclose.
    if state.panel_more.is_open(crate::plate::MORE_3D) {
        ui.label(egui::RichText::new(NAV_LEGEND).small().color(crate::plate::legend_dim()));
    }
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
    // GM22: the render runs on the worker; the last picture stays up (the
    // one texture, its pixels replaced in place) until the next lands.
    match state.view3d.worker.poll(ui.ctx(), "view3d") {
        Some(Landed::Picture([w, h])) => state.view3d.rendered = Some((w as u32, h as u32)),
        Some(Landed::Failed(why)) => state.refuse(format!("View3D: {why}")),
        None => {}
    }
    if state.view3d.worker.jobs.want(key) {
        match view_job(state.cut(), node, &orbit, frame, size, law, fw) {
            Some(job) => state.view3d.worker.start(ui.ctx(), move || job.render()),
            None => {
                if let Some(why) = state.view3d.worker.jobs.land(Err("the 3D model is missing".into())) {
                    state.refuse(format!("View3D: {why}"));
                }
            }
        }
    }
    if navigating {
        ui.ctx().request_repaint();
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, crate::plate::WELL);
    if let Some(tex) = state.view3d.worker.texture() {
        let (w, h) = (rect.width(), rect.height());
        let fit = (w / aspect).min(h);
        let shown = egui::Rect::from_center_size(rect.center(), egui::vec2(fit * aspect, fit));
        let image = egui::Image::new(egui::load::SizedTexture::new(tex.id(), shown.size()));
        ui.put(shown, image);
    }
    if state.view3d.worker.jobs.busy() {
        // The lamp, and in words (the viewers' FX dot).
        let at = rect.left_top() + egui::vec2(10.0, 10.0);
        painter.circle_filled(at, 4.0, crate::plate::TALLY);
        painter.text(
            at + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "rendering…",
            crate::plate::Role::Small.font(),
            crate::plate::LEGEND,
        );
    }
}

/// GM22: the canvas's 3D underlay — the cut's Model3D node as it delivers
/// (export's renderer, at most [`MAX_RENDER_W`] across), drawn faded under
/// the drawing on the frame. A view: off by default, kept for the session,
/// never in the document or the export. Nothing starts mid-stroke or while
/// playing; then the last finished picture holds.
#[derive(Default)]
pub struct Underlay {
    pub on: bool,
    worker: Worker3d,
    /// The node and frame of the job running, and of the picture shown.
    asked: Option<(NodeId, u32)>,
    made: Option<(NodeId, u32)>,
}

/// The underlay this frame: its texture and the node and frame it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnderlayPicture {
    pub tex: egui::TextureId,
    pub node: NodeId,
    pub frame: u32,
}

impl Underlay {
    /// Take in a finished picture, start the job the current frame wants
    /// (unless `stroking` or playing), and the picture to draw (None: off,
    /// no Model3D node, or none finished yet).
    pub fn sync(&mut self, state: &mut AppState, ctx: &egui::Context, stroking: bool) -> Option<UnderlayPicture> {
        if !self.on {
            return None;
        }
        let node = model_node(state.cut(), state.selected_node)?;
        match self.worker.poll(ctx, "underlay3d") {
            Some(Landed::Picture(_)) => self.made = self.asked,
            Some(Landed::Failed(why)) => state.refuse(format!("3D underlay: {why}")),
            None => {}
        }
        let (scene, cut, frame) = (state.view.scene, state.view.cut, state.view.frame);
        let size = underlay_size(state.engine.project.width, state.engine.project.height);
        let law = state.engine.project.color_law;
        // The node's eval value keys it (memo hits): a still model keeps
        // its picture across frames, a keyed camera moves it.
        let value = state.engine.eval_node(scene, cut, node, frame).map_or(0, |v| v.hash());
        let key = underlay_key(node, value, size, law);
        if !stroking && !state.view.playing && self.worker.jobs.want(key) {
            let snapshot = crate::graphcomp::job_cut(state.cut(), node, frame);
            let base = state.project_dir();
            self.asked = Some((node, frame));
            self.worker.start(ctx, move || underlay_render(&snapshot, node, frame, size, law, base));
        }
        let tex = self.worker.texture()?.id();
        let (node, made) = self.made?;
        // The picture is this frame's when its key is this frame's.
        let frame = if self.worker.jobs.shown() == Some(key) { frame } else { made };
        Some(UnderlayPicture { tex, node, frame })
    }

    /// The TALLY dot: on, with a picture still to come (a first one, or
    /// the current frame's).
    pub fn busy(&self) -> bool {
        self.on && self.worker.jobs.busy()
    }

    /// Tests: jobs started so far.
    #[cfg(test)]
    pub fn started(&self) -> u32 {
        self.worker.jobs.started
    }

    /// Tests: the picture's texture (None before the first lands).
    #[cfg(test)]
    pub fn texture_id(&self) -> Option<egui::TextureId> {
        self.worker.texture().map(|t| t.id())
    }

    /// Tests: block until the underlay's job is in (it lands next sync).
    #[cfg(test)]
    pub fn wait(&mut self) {
        self.worker.wait();
    }
}

/// The underlay's render size for a `w × h` project frame: the frame's
/// shape, at most [`MAX_RENDER_W`] across (the canvas stretches it over the
/// frame).
pub fn underlay_size(w: u32, h: u32) -> (u32, u32) {
    let (w, h) = (w.max(1), h.max(1));
    if w <= MAX_RENDER_W {
        return (w, h);
    }
    let sh = (h as u64 * MAX_RENDER_W as u64 + w as u64 / 2) / w as u64;
    (MAX_RENDER_W, (sh as u32).max(1))
}

/// What an underlay picture shows, as one key: the node, its eval value at
/// the frame, the size and the law.
fn underlay_key(node: NodeId, value: u64, size: (u32, u32), law: ColorLaw) -> u64 {
    anim_core::value::fnv1a(format!("underlay|{node:?}|{value:016x}|{size:?}|{law:?}").as_bytes())
}

/// The underlay's picture: node `node` as export renders it at `w × h`
/// from a snapshot; what it cannot read (a model that does not parse, a
/// missing file) fails the job, said once.
pub fn underlay_render(
    cut: &Cut,
    node: NodeId,
    frame: u32,
    (w, h): (u32, u32),
    law: ColorLaw,
    base: Option<PathBuf>,
) -> Result<Image, String> {
    let env = anim_core::export::RenderEnv::new(base);
    let px = anim_core::export::render_node_px_env(cut, law, node, frame, w, h, &env);
    match env.warnings().into_iter().next() {
        Some(why) => Err(why),
        None => Ok(Image::from_px(w, h, px)),
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
        let r = h.editor().pane_rects.iter().find(|(p, ..)| *p == pane).map(|(_, r, _)| *r);
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
        assert!(h.has("Set shot camera from view"), "{}", h.describe());
        assert!(h.has("Frame all") && h.has_text_containing(NAV_LEGEND), "{}", h.describe());
        // GM22: the render is the worker's; it lands the frame after.
        h.state_mut().view3d.wait();
        h.run();
        let size = h.state().view3d.rendered.expect("a render");
        assert!(size.0 > 0 && size.1 > 0 && size.0 <= MAX_RENDER_W);
        let before = h.state().engine.history_len();
        h.click("Set shot camera from view");
        h.run_frames(2);
        assert_eq!(h.state().engine.history_len(), before + 1, "one undo step");
        // In ortho the button stands down and says why; the verb refuses.
        h.click("Ortho");
        h.run_frames(2);
        assert!(h.state().view3d.orbit.is_some_and(|o| o.ortho));
        assert_eq!(h.node_enabled("Set shot camera from view"), Some(false));
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
        assert!(state.status.contains("under the picture"), "{}", state.status);
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
        assert!(bare.status.contains("connect it to the Output in the Effects graph"), "{}", bare.status);
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

    // ---- GM22: the worker, sequence reload and the 3D underlay ------------------------------

    /// A small project (192 × 108: the test pictures stay small) with the cube.
    fn small_cube(dir: &std::path::Path) -> (AppState, NodeId) {
        let mut state = AppState::new_project("underlay", 192, 108, 24, 300.0);
        let path = dir.join("cube.obj");
        std::fs::write(&path, CUBE_OBJ).unwrap();
        let node = state.import_model(&path, (40.0, 40.0)).expect("the cube imports");
        (state, node)
    }

    #[test]
    fn worker3d_only_the_newest_key_waits_and_a_late_picture_still_replaces_the_old() {
        let (a, b, c) = (1, 2, 3);
        let mut j = JobLogic::default();
        assert!(j.want(a), "A starts");
        assert!(!j.want(b) && !j.want(c), "B, then C, wait while A runs");
        assert_eq!((j.started, j.pending()), (1, Some(a)));
        assert!(j.busy());
        assert_eq!(j.land(Ok(())), None);
        assert_eq!(j.shown(), Some(a), "A's late picture replaces the old one");
        assert!(j.busy(), "the dot stays on: C is to come");
        assert_eq!(j.next(), Some(c), "the next job is C's — B never renders");
        assert_eq!(j.started, 2);
        assert!(!j.want(c), "C runs already");
        assert_eq!(j.land(Ok(())), None);
        assert_eq!(j.shown(), Some(c));
        assert!(!j.busy(), "C's picture clears the dot");
        assert!(!j.want(c) && j.next().is_none() && j.started == 2, "a shown key starts nothing");
    }

    #[test]
    fn worker3d_a_failed_or_lost_job_is_said_once_and_its_key_never_resubmitted() {
        // A model that does not parse, rendered on the worker.
        let dir = tempfile::tempdir().unwrap();
        let (state, node) = small_cube(dir.path());
        let broken = || {
            let mut job = view_job(state.cut(), node, &Orbit::default(), 0, (32, 18), ColorLaw::Srgb, 192).unwrap();
            job.model.bytes = std::sync::Arc::new(b"# no faces\n".to_vec());
            job.model.content_hash = 0xbad;
            job
        };
        let ctx = egui::Context::default();
        let mut w = Worker3d::default();
        assert!(w.jobs.want(10));
        let job = broken();
        w.start(&ctx, move || job.render());
        w.wait();
        let Some(Landed::Failed(why)) = w.poll(&ctx, "t") else {
            panic!("the job fails")
        };
        assert!(why.starts_with("3D model "), "{why}");
        assert!(!w.jobs.want(10) && w.jobs.started == 1, "the failed key is not resubmitted");
        assert!(!w.jobs.busy(), "and no dot for it");
        // Another key failing the same way is not said again.
        assert!(w.jobs.want(11));
        let job = broken();
        w.start(&ctx, move || job.render());
        w.wait();
        assert_eq!(w.poll(&ctx, "t"), None, "said once");
        // A worker that panicked: its channel closes unanswered — the same.
        let mut lost = Worker3d::default();
        assert!(lost.jobs.want(20));
        let (tx, rx) = mpsc::channel::<Result<egui::ColorImage, String>>();
        lost.answer = Some(rx);
        drop(tx);
        assert_eq!(lost.poll(&ctx, "t"), Some(Landed::Failed(WORKER_LOST.into())));
        assert!(!lost.jobs.want(20) && lost.jobs.started == 1, "not resubmitted");
        assert!(lost.jobs.want(21));
        assert_eq!(lost.jobs.lost(), None, "a second loss is not said again");
        assert!(!lost.jobs.want(21));
    }

    #[test]
    fn worker3d_a_view_change_starts_one_job_and_the_pane_keeps_its_picture_and_dot_until_it_lands() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _node) = small_cube(dir.path());
        let mut p = crate::testkit::PanelHarness::new(egui::vec2(240.0, 200.0), state, ui);
        let settle = |p: &mut crate::testkit::PanelHarness<AppState>| {
            for _ in 0..4 {
                p.state_mut().view3d.wait();
                p.frames(1);
                if !p.state().view3d.worker.jobs.busy() {
                    break;
                }
            }
        };
        settle(&mut p);
        let size = p.state().view3d.rendered.expect("the first picture");
        assert!(size.0 <= 240 && size.1 <= 240, "{size:?}");
        let tex = p.state().view3d.worker.texture().map(|t| t.id()).expect("its texture");
        let started = p.state().view3d.worker.jobs.started;
        // The view turns: one job starts; the last picture stays up, with
        // the dot, until it lands.
        p.state_mut().view3d.orbit.as_mut().expect("the view").orbit(40.0, 0.0);
        p.frames(1);
        let jobs = &p.state().view3d.worker.jobs;
        assert_eq!(jobs.started, started + 1, "one job");
        assert!(jobs.busy() && jobs.pending().is_some(), "the dot is on");
        assert_eq!(p.state().view3d.worker.texture().map(|t| t.id()), Some(tex), "the last picture stays up");
        p.frames(2);
        assert_eq!(p.state().view3d.worker.jobs.started, started + 1, "frames while it runs start nothing");
        settle(&mut p);
        assert!(!p.state().view3d.worker.jobs.busy(), "its picture clears the dot");
        assert_eq!(p.state().view3d.worker.texture().map(|t| t.id()), Some(tex), "the one texture, replaced");
        assert_eq!(p.state().view3d.worker.jobs.started, started + 1);
        // The pane's ui never renders itself: the worker does.
        let src = include_str!("view3d.rs");
        let body = &src[src.find("pub fn ui(").expect("the pane's ui")..];
        let body = &body[..body.find("\n}\n").expect("its end")];
        assert!(!body.contains("render_view(") && !body.contains("rasterize("), "the pane renders on the worker");
    }

    #[test]
    fn worker3d_a_sequence_value_folds_its_files_length_and_time_once_its_path_resolves() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("renders")).unwrap();
        let file = dir.path().join("renders").join("shot_0001.png");
        anim_core::deliver::write_png8(&file, &Image::new(4, 4)).unwrap();
        let len = std::fs::metadata(&file).unwrap().len();
        let mut state = AppState::new_project("seq", 16, 9, 24, 300.0);
        state.graph_add_node(NodeKind::image_sequence("renders/shot_####.png"), (0.0, 0.0));
        let node = state.selected_node.expect("the sequence node");
        let (scene, cut) = (state.view.scene, state.view.cut);
        let value = |s: &mut AppState, n| s.engine.eval_node(scene, cut, n, 0).unwrap().recipe().to_string();
        // Unsaved, a relative pattern keeps today's value (pinned).
        state.sync_eval_base();
        assert_eq!(value(&mut state, node), "seq(\"renders/shot_####.png\", n=1, hold=false, fit=true)");
        // Saved: from the project's folder, the shown file's length and time.
        state.file_path = Some(dir.path().join("seq.animproj"));
        state.sync_eval_base();
        let saved = value(&mut state, node);
        assert!(saved.contains(&format!(", file={len}@")), "{saved}");
        state.engine.evaluator.invalidate_all();
        assert_eq!(value(&mut state, node), saved, "neither changed: the same value");
        // Its time alone changes it.
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(2_000_000);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(t).unwrap();
        state.engine.evaluator.invalidate_all();
        let touched = value(&mut state, node);
        assert!(touched != saved && touched.ends_with(&format!("file={len}@2000000000000000)")), "{touched}");
        // An absolute pattern resolves with no folder.
        state.file_path = None;
        state.sync_eval_base();
        let abs = dir.path().join("renders").join("shot_####.png").to_string_lossy().into_owned();
        state.graph_add_node(NodeKind::image_sequence(abs), (0.0, 90.0));
        let absolute = state.selected_node.expect("the second node");
        let v = value(&mut state, absolute);
        assert!(v.contains(&format!("file={len}@2000000000000000")), "{v}");
    }

    #[test]
    fn worker3d_reload_and_regained_focus_reread_only_the_sequences_and_their_downstream() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("renders")).unwrap();
        let shot = |n: u32| dir.path().join("renders").join(format!("shot_{n:04}.png"));
        anim_core::deliver::write_png8(&shot(1), &Image::new(4, 4)).unwrap();
        anim_core::deliver::write_png8(&shot(2), &Image::new(4, 4)).unwrap();
        let mut state = AppState::new_project("seq", 16, 9, 24, 300.0);
        state.graph_add_node(NodeKind::Solid { rgba: [9, 9, 9, 255] }, (0.0, 120.0));
        let solid = state.selected_node.expect("the solid");
        state.graph_add_node(NodeKind::image_sequence("renders/shot_####.png"), (0.0, 0.0));
        let seq = state.selected_node.expect("the sequence node");
        state.file_path = Some(dir.path().join("seq.animproj"));
        let mut h = EditorHarness::with_state(state);
        h.run_frames(2);
        let (scene, cut) = (h.state().view.scene, h.state().view.cut);
        let eval = |h: &mut EditorHarness, n, f| h.state_mut().engine.eval_node(scene, cut, n, f).unwrap().hash();
        let before: Vec<u64> = (0..2).map(|f| eval(&mut h, seq, f)).collect();
        assert_ne!(before[0], before[1]);
        eval(&mut h, solid, 0);
        // File 2 re-rendered on disk (another length): the memo holds it.
        anim_core::deliver::write_png8(&shot(2), &Image::new(8, 8)).unwrap();
        assert_eq!(eval(&mut h, seq, 1), before[1], "nothing reads it before a reload");
        let doc = |h: &EditorHarness| (h.state().engine.history_len(), h.state().engine.revision(), h.state().is_dirty());
        let was = doc(&h);
        h.perform(Action::ReloadSequences);
        let hits = h.state().engine.evaluator.stats.cache_hits;
        eval(&mut h, solid, 0);
        assert_eq!(h.state().engine.evaluator.stats.cache_hits, hits + 1, "the solid kept its value");
        assert_eq!(eval(&mut h, seq, 0), before[0], "frame 0's file is as it was");
        assert_ne!(eval(&mut h, seq, 1), before[1], "frame 1 shows its new file");
        assert_eq!(doc(&h), was, "no undo step, nothing dirtier");
        // The window regaining focus reads them again too.
        let kept = eval(&mut h, seq, 0);
        anim_core::deliver::write_png8(&shot(1), &Image::new(8, 8)).unwrap();
        h.event(egui::Event::WindowFocused(true));
        assert_ne!(eval(&mut h, seq, 0), kept, "frame 0 shows its new file");
        assert_eq!(doc(&h), was);
    }

    #[test]
    fn worker3d_a_reload_keeps_the_ram_previews_unchanged_frames_and_its_pass_unless_a_frame_it_draws_moved() {
        let dir = tempfile::tempdir().unwrap();
        let shot = |n: u32, side: u32| {
            let path = dir.path().join(format!("shot_{n:04}.png"));
            anim_core::deliver::write_png8(&path, &Image::new(side, side)).unwrap();
        };
        shot(1, 4);
        shot(2, 4);
        // The sequence is the cut's output: the RAM preview holds its frames.
        let mut s = AppState::new_project("ram", 16, 9, 24, 300.0);
        s.graph_add_node(NodeKind::image_sequence("shot_####.png"), (0.0, 0.0));
        let seq = s.selected_node.expect("the sequence node");
        s.graph_add_node(NodeKind::Output, (200.0, 0.0));
        let out = s.selected_node.expect("the output node");
        s.graph_connect(seq, out, 0);
        s.graph_set_output(out);
        s.file_path = Some(dir.path().join("ram.animproj"));
        s.sync_eval_base();
        let folder = s.project_dir();
        s.ram.set_base_dir(folder);
        let p = &s.engine.project;
        let look = crate::playback::RamLook { at: s.at(), size: (p.width, p.height), law: p.color_law, final_look: false };
        let cut = s.view.cut;
        let sync = |s: &mut AppState| s.ram.sync(&mut s.engine, look, (0, 1), None);
        let held = |s: &AppState| [0, 1].map(|f| s.ram.held(cut, f));
        // A reload with nothing re-rendered (the window coming back) lets
        // the running pass carry on: every frame it draws lands.
        s.ram.toggle();
        assert_eq!(sync(&mut s), Some(Ok("RAM preview: rendering 2 frames…".to_string())));
        assert_eq!(s.reload_sequences(), 1);
        let msg = sync(&mut s);
        assert!(!matches!(&msg, Some(Ok(m)) if m.contains("rendering")), "not started over: {msg:?}");
        s.ram.wait();
        assert_eq!(held(&s), [true, true], "the pass was not let go");
        // Shot 2 re-rendered: frame 0 stays held, frame 1 drops.
        shot(2, 8);
        s.reload_sequences();
        assert_eq!(sync(&mut s), None);
        assert_eq!(held(&s), [true, false]);
        // Re-rendered again while a pass draws it: the pass starts over, and
        // frame 1 is held under its newest file (a later reload keeps it).
        s.ram.toggle();
        assert_eq!(sync(&mut s), Some(Ok("RAM preview: rendering 1 frames…".to_string())));
        shot(2, 6);
        s.reload_sequences();
        sync(&mut s);
        s.ram.wait();
        s.reload_sequences();
        assert_eq!(sync(&mut s), None);
        assert_eq!(held(&s), [true, true], "drawn again from the new file");
    }

    #[test]
    fn worker3d_the_underlay_is_refused_with_no_model_and_draws_the_cube_without_touching_the_document() {
        let mut empty = EditorHarness::new();
        assert_eq!(empty.editor().can_perform(Action::Underlay3d), Err(NO_MODEL));
        assert_eq!(empty.perform(Action::Underlay3d), crate::Performed::Ignored);
        assert!(!empty.editor().canvas.underlay.on);
        let dir = tempfile::tempdir().unwrap();
        let (state, node) = small_cube(dir.path());
        let mut h = EditorHarness::with_state(state);
        h.run_frames(2);
        assert_eq!(h.editor().is_on(Action::Underlay3d), Some(false), "off by default");
        assert_eq!(h.editor().canvas.underlay_drawn, None);
        // The export's pixels, pinned: the underlay is a view.
        let export = |h: &EditorHarness| {
            let s = h.state();
            let env = anim_core::export::RenderEnv::default();
            let opts = anim_core::deliver::RenderOpts::default();
            let img = anim_core::deliver::render_env(s.cut(), s.engine.project.color_law, 0, 192, 108, &opts, &env);
            let bytes: Vec<u8> = img.px.iter().flat_map(|v| v.to_le_bytes()).collect();
            anim_core::value::fnv1a(&bytes)
        };
        let off = export(&h);
        let doc = |h: &EditorHarness| (h.state().engine.history_len(), h.state().engine.revision(), h.state().is_dirty());
        let was = doc(&h);
        h.perform(Action::Underlay3d);
        assert_eq!(h.editor().is_on(Action::Underlay3d), Some(true));
        h.run_frames(2);
        h.editor_mut().canvas.underlay.wait();
        h.run();
        let drawn = h.editor().canvas.underlay_drawn.expect("the canvas drew the underlay");
        let frame = anim_core::camera::IRect::frame(192, 108);
        assert_eq!(drawn, crate::canvas::UnderlayDrawn { node, frame: 0, rect: frame, opacity: 0.45 });
        assert_eq!(doc(&h), was, "a view: no undo step, the document as clean");
        assert_eq!(export(&h), off, "the export is the same with it on");
        assert_eq!(off, 0x6423_45a1_fbfa_3bc4, "pinned");
        // The quad it painted: its picture on the frame's corners, faded to
        // the light table's opacity — and the lightbox's pin opacity fades
        // it too.
        let quad = |h: &EditorHarness| {
            let tex = h.editor().canvas.underlay.texture_id().expect("its picture");
            let meshes = h.meshes_with(tex);
            assert_eq!(meshes.len(), 1, "one quad");
            let corners: Vec<egui::Pos2> = meshes[0].vertices.iter().map(|v| v.pos).collect();
            assert_eq!(corners, h.editor().canvas.view_xform().paper_corners().to_vec(), "on the frame");
            meshes[0].vertices[0].color
        };
        assert_eq!(quad(&h), crate::plate::faded(0.45));
        // (The shell feeds the canvas the config's light table each frame.)
        h.rig_mut().config.light_table.opacity = 0.7;
        h.run();
        assert_eq!(h.editor().canvas.underlay_drawn.map(|d| d.opacity), Some(0.7));
        assert_eq!(quad(&h), crate::plate::faded(0.7));
        // The app's own export — a render queue job from this state —
        // writes the same file with the underlay on as off.
        let out = tempfile::tempdir().unwrap();
        let queued = |h: &mut EditorHarness, name: &str| {
            let ctx = h.ctx();
            let s = h.state_mut();
            s.render_queue.ffmpeg = Some((String::new(), None));
            s.render_queue.draft.out = out.path().join(name).display().to_string();
            let at = s.at();
            assert!(crate::render_queue::add_job(s, at, Some((0, 0))));
            crate::render_queue::start(s, &ctx);
            crate::render_queue::wait(s, &ctx);
            let files: Vec<_> = std::fs::read_dir(out.path().join(name)).unwrap().flatten().collect();
            assert_eq!(files.len(), 1, "one frame written");
            std::fs::read(files[0].path()).unwrap()
        };
        let with_it = queued(&mut h, "on");
        h.perform(Action::Underlay3d);
        h.run_frames(2);
        assert_eq!(h.editor().canvas.underlay_drawn, None, "off: nothing drawn");
        assert_eq!(export(&h), off);
        assert_eq!(queued(&mut h, "off"), with_it, "the delivered file is the same");
        assert_eq!(doc(&h), was);
    }

    #[test]
    fn worker3d_the_underlay_keys_on_the_nodes_value_and_starts_nothing_mid_stroke_or_while_playing() {
        fn shown(u: &mut Underlay, s: &mut AppState, ctx: &egui::Context, stroking: bool) -> Option<(NodeId, u32)> {
            u.sync(s, ctx, stroking).map(|p| (p.node, p.frame))
        }
        let dir = tempfile::tempdir().unwrap();
        let (mut state, node) = small_cube(dir.path());
        let ctx = egui::Context::default();
        let mut u = Underlay { on: true, ..Default::default() };
        assert_eq!(shown(&mut u, &mut state, &ctx, false), None, "nothing finished yet");
        assert_eq!(u.started(), 1);
        u.wait();
        assert_eq!(shown(&mut u, &mut state, &ctx, false), Some((node, 0)));
        // A still model scrubbed: the picture is every frame's — no job.
        state.view.frame = 3;
        assert_eq!(shown(&mut u, &mut state, &ctx, false), Some((node, 3)));
        assert_eq!(u.started(), 1, "the same key: no job");
        // A keyed camera: each frame's picture is its own.
        let mut view = Orbit::default();
        state.view.frame = 0;
        state.set_cut_camera_from_view(node, &view);
        view.orbit(-60.0, 20.0);
        state.view.frame = 4;
        state.set_cut_camera_from_view(node, &view);
        state.view.frame = 0;
        shown(&mut u, &mut state, &ctx, false);
        assert_eq!(u.started(), 2, "the camera keyed: frame 0's key moved");
        assert!(u.busy(), "the dot: its picture is on its way");
        u.wait();
        assert_eq!(shown(&mut u, &mut state, &ctx, false), Some((node, 0)));
        assert!(!u.busy(), "it landed: no dot");
        state.view.frame = 1;
        assert_eq!(shown(&mut u, &mut state, &ctx, true), Some((node, 0)), "mid-stroke the last picture holds");
        assert_eq!(u.started(), 2, "a stroke in progress starts no job");
        shown(&mut u, &mut state, &ctx, false);
        assert_eq!(u.started(), 3, "the keyed camera's next frame starts one");
        u.wait();
        assert_eq!(shown(&mut u, &mut state, &ctx, false), Some((node, 1)));
        // Playing: the last finished picture holds, and no job starts.
        state.view.playing = true;
        state.view.frame = 2;
        assert_eq!(shown(&mut u, &mut state, &ctx, false), Some((node, 1)));
        assert_eq!(u.started(), 3, "no job while playing");
    }

    #[test]
    fn worker3d_under_the_pen_the_underlay_starts_no_job_and_a_regained_focus_reads_the_files_once_it_lifts() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("shot_0001.png");
        anim_core::deliver::write_png8(&shot, &Image::new(4, 4)).unwrap();
        let (mut state, _) = small_cube(dir.path());
        state.graph_add_node(NodeKind::image_sequence("shot_####.png"), (0.0, 200.0));
        let seq = state.selected_node.expect("the sequence node");
        state.file_path = Some(dir.path().join("cut.animproj"));
        state.new_drawing_at_frame();
        let mut h = EditorHarness::with_state(state);
        h.run();
        let (scene, cut) = (h.state().view.scene, h.state().view.cut);
        let value = |h: &mut EditorHarness| h.state_mut().engine.eval_node(scene, cut, seq, 0).unwrap().hash();
        let was = value(&mut h);
        // The pen goes down; the underlay is ticked, and the window comes
        // back with the shot re-rendered, while it is down (the stroke tap).
        let c = h.editor().canvas.view_xform().paper_to_screen(egui::pos2(96.0, 54.0));
        h.press_and_move(c, c + egui::vec2(20.0, 8.0), 3);
        assert!(h.editor().canvas.stroke_active(), "a stroke in progress");
        h.editor_mut().canvas.underlay.on = true;
        anim_core::deliver::write_png8(&shot, &Image::new(8, 8)).unwrap();
        h.event(egui::Event::WindowFocused(true));
        assert!(h.editor().canvas.stroke_active(), "still drawing");
        assert_eq!(h.editor().canvas.underlay.started(), 0, "no underlay job mid-stroke");
        assert_eq!(value(&mut h), was, "nor are the files read under the pen");
        // The pen lifts: the reload and the underlay's job follow.
        h.release(c + egui::vec2(20.0, 8.0));
        assert!(!h.editor().canvas.stroke_active());
        assert_ne!(value(&mut h), was, "read once the pen is up");
        assert_eq!(h.editor().canvas.underlay.started(), 1, "the job starts once the pen is up");
    }

    #[test]
    fn worker3d_a_ticked_underlay_that_shows_nothing_says_why_under_its_menu_item() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _) = small_cube(dir.path());
        let mut h = EditorHarness::with_state(state);
        h.run();
        let note = |h: &EditorHarness| h.editor().menu_note(Action::Underlay3d);
        assert_eq!(note(&h), None, "off: nothing to say");
        h.perform(Action::Underlay3d);
        h.editor_mut().canvas.underlay.wait();
        h.run();
        assert!(h.editor().canvas.underlay_drawn.is_some());
        assert_eq!(note(&h), None, "drawn and seen: nothing to say");
        // A BG column showing a drawing on the frame covers it.
        h.state_mut().new_drawing_at_frame();
        h.state_mut().set_column_kind(anim_core::xsheet::ColumnKind::Bg);
        h.run();
        assert!(h.editor().canvas.underlay_drawn.is_some(), "drawn, under the BG");
        let bg = "under the Background track: hide it on the sheet to see the 3D model";
        assert_eq!(note(&h).as_deref(), Some(bg));
        // A cut with no Model3D node.
        let scene = h.state().view.scene;
        h.state_mut().new_cut(scene);
        h.run();
        assert_eq!(note(&h).as_deref(), Some("not shown: this shot has no 3D model"));
        assert!(h.editor().canvas.underlay.on, "still ticked");
    }

    #[test]
    fn worker3d_the_new_verbs_have_help_a_place_and_no_chord_and_the_room_says_the_underlay_is_in() {
        use crate::config::{Ctx, Group, Keymap, clashes, default_binds};
        assert_eq!(Action::ReloadSequences.menu(), Some("File/Import/Reload image sequences"));
        assert_eq!(Action::Underlay3d.menu(), Some("View/3D view/3D underlay"));
        assert_eq!((Action::ReloadSequences.group(), Action::Underlay3d.group()), (Group::File, Group::View));
        assert_eq!(Action::Underlay3d.context(), Ctx::Canvas);
        for a in [Action::ReloadSequences, Action::Underlay3d] {
            assert!((1..=120).contains(&a.help().chars().count()), "{a:?}");
            assert!(Action::ALL.contains(&a), "{a:?}");
            for &km in Keymap::ALL {
                assert_eq!(crate::cmdpal::chord_of(&default_binds(km), a), None, "{a:?} has no chord");
            }
        }
        for &km in Keymap::ALL {
            assert_eq!(clashes(&default_binds(km)), vec![], "{km:?}");
        }
        for alias in ["refresh", "reload renders", "reload sequence"] {
            assert!(Action::ReloadSequences.aliases().contains(&alias), "{alias}");
        }
        // The studio's words for it: the 3D LO (layout), the genzu.
        for alias in ["3d lo", "3d layout", "layout under", "genzu"] {
            assert!(Action::Underlay3d.aliases().contains(&alias), "{alias}");
        }
        // The room says the underlay is in this build, in its help, which is
        // its menu row's hover.
        let help = Action::Layout3dRoom.help();
        assert!(!help.contains("not in this build") && help.contains("3D underlay"), "{help}");
        assert!(crate::menus::ORDER.contains(&crate::menus::Row::A(Action::ReloadSequences)), "its row");
    }

    #[test]
    fn worker3d_old_configs_and_projects_still_load_and_the_new_views_save_nothing() {
        use crate::config::{Config, clashes, config_path_in};
        // A config from before GM22: neither new verb in its keybinds. It
        // loads with nothing to say, the verbs filled in without a chord.
        let dir = tempfile::tempdir().unwrap();
        let mut v = serde_json::to_value(Config::default()).unwrap();
        let binds = v["keybinds"].as_array_mut().unwrap();
        let all = binds.len();
        binds.retain(|b| !matches!(b["action"].as_str(), Some("ReloadSequences" | "Underlay3d")));
        assert_eq!(binds.len(), all - 2);
        std::fs::write(config_path_in(dir.path()), serde_json::to_vec_pretty(&v).unwrap()).unwrap();
        let (cfg, notes) = Config::load_from(dir.path());
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(cfg.keybinds.len(), Action::ALL.len(), "the new verbs are filled in");
        for a in [Action::ReloadSequences, Action::Underlay3d] {
            assert!(cfg.binding(a).is_some_and(|b| b.chord.is_none()), "{a:?} has no chord");
        }
        assert_eq!(clashes(&cfg.keybinds), vec![], "no key fires two verbs");
        // The verbs come last and are stored by name; an older one still reads.
        assert!(Action::ALL.ends_with(&[Action::ReloadSequences, Action::Underlay3d]));
        assert_eq!(serde_json::to_string(&Action::Underlay3d).unwrap(), "\"Underlay3d\"");
        assert_eq!(serde_json::from_str::<Action>("\"ColumnKindBg\"").unwrap(), Action::ColumnKindBg);
        // A project the store wrote before GM22 (GM22 saves no new field)
        // with the cube and a sequence opens clean, nothing evaluated …
        let work = tempfile::tempdir().unwrap();
        anim_core::deliver::write_png8(&work.path().join("shot_0001.png"), &Image::new(4, 4)).unwrap();
        let (mut s, _) = small_cube(work.path());
        s.graph_add_node(NodeKind::image_sequence("shot_####.png"), (0.0, 200.0));
        let seq = s.selected_node.expect("the sequence node");
        let old = s.snapshot_project();
        let file = work.path().join("old.animproj");
        anim_core::store::save(&old, &file).unwrap();
        let st = AppState::load_from(file.clone()).unwrap();
        assert_eq!(st.engine.project, old);
        assert!(!st.is_dirty(), "an old file opens clean");
        assert_eq!(st.engine.evaluator.cached_entries(), 0);
        // … and the underlay, a reload and the sequence read from the
        // project's folder leave it as it was: the evaluator is not saved.
        let mut h = EditorHarness::with_state(st);
        h.perform(Action::Underlay3d);
        h.editor_mut().canvas.underlay.wait();
        h.run();
        assert!(h.editor().canvas.underlay_drawn.is_some());
        h.perform(Action::ReloadSequences);
        let (scene, cut) = (h.state().view.scene, h.state().view.cut);
        let value = h.state_mut().engine.eval_node(scene, cut, seq, 0).unwrap();
        assert!(!value.recipe().contains("file=-"), "read from the folder: {}", value.recipe());
        assert!(h.state().engine.evaluator.stats.file_stats > 0);
        assert!(!h.state().is_dirty());
        let now = h.state_mut().snapshot_project();
        assert_eq!(now, old);
        // Saved again: the same tables, the same project.
        let again = work.path().join("again.animproj");
        anim_core::store::save(&now, &again).unwrap();
        let tables = |f: &std::path::Path| {
            let db = rusqlite::Connection::open(f).unwrap();
            let mut q = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").unwrap();
            q.query_map([], |r| r.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap()
        };
        assert_eq!(tables(&again), tables(&file));
        assert_eq!(anim_core::Engine::load(&again).unwrap().project, old);
    }
}
