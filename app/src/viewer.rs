//! The composite VIEWER pane — the first multi-instance pane (LENS-DOCK
//! Phase 1). Read-only: it displays the node graph's rendered output (the
//! same texture composite view and export truth come from) with its own
//! zoom/pan, independent per instance. Put one beside the canvas for the
//! Finishing room: paint in the edit canvas, watch the composite live here.
//!
//! Interaction: wheel = zoom at cursor, any drag = pan, double-click = fit.
//! No document mutation happens here, so none of the canvas guards apply.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use anim_core::command::CutRef;
use anim_core::deliver::{self, Purpose, RenderOpts};
use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, pos2, vec2};

use crate::config::Action;
use crate::doc::AppState;

/// What the shared per-frame graph execution produced — every consumer
/// (canvas composite view, viewer panes) words its empty-state from the
/// ACTUAL cause instead of guessing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GraphView {
    /// No visible consumer needed the graph this frame (nothing executed).
    Off,
    /// The cut's graph has no wired Output node.
    NoOutput,
    /// wgpu unavailable (probe/headless or GPU init failure).
    NoGpu,
    /// The evaluator returned an error (corrupt graph state).
    EvalFailed,
    Ready(egui::TextureId),
    /// GM9: shown, while a CPU subtree (an Effect) renders on the worker —
    /// the texture holds its last result; the views light a TALLY dot.
    Rendering(egui::TextureId),
}

/// GM11: the composite as the master delivers it — [`Purpose::Final`]: no
/// rough, reference or correction (sakkan) layers and no Ref columns —
/// rendered by the CPU on a worker from a snapshot (D9) and shown by every
/// composite view in place of the graph's GPU render while its latch is
/// on. The last finished frame stays up, with the TALLY dot, while the
/// next one renders; one render runs at a time.
#[derive(Default)]
pub struct FinalLook {
    /// The latch: every viewer's "final look" and View ▸ Final look.
    pub on: bool,
    /// The views' texture (made the first time the look is shown).
    tex: Option<egui::TextureHandle>,
    /// What the texture holds.
    shown: Option<LookKey>,
    /// The last finished frame, as uploaded (straight sRGB bytes).
    pub image: Option<Arc<egui::ColorImage>>,
    /// The frame on the worker, and where its answer comes (with what it
    /// could not read).
    pending: Option<(LookKey, Receiver<(Arc<egui::ColorImage>, Vec<String>)>)>,
    /// GM17: what finished renders reported, waiting for the status lane.
    warnings: Vec<String>,
}

/// What a final-look frame shows: another cut, frame or size, any edit (a
/// new revision), or the project saved to another folder, renders again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LookKey {
    at: CutRef,
    frame: u32,
    revision: u64,
    size: (u32, u32),
    folder: u64,
}

/// A hash of the project's folder (part of [`LookKey`]).
fn folder_key(dir: Option<&std::path::Path>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    dir.hash(&mut h);
    h.finish()
}

impl FinalLook {
    /// This frame's view for every composite consumer: the finished frame
    /// (Ready), or the last one kept up while this one renders
    /// (Rendering). Never waits; the snapshot is the one clone made here.
    pub fn view(&mut self, state: &AppState, ctx: &egui::Context) -> GraphView {
        let p = &state.engine.project;
        let dir = state.project_dir();
        let key = LookKey {
            at: state.at(),
            frame: state.view.frame,
            revision: state.engine.revision(),
            size: (p.width, p.height),
            folder: folder_key(dir.as_deref()),
        };
        if let Some((k, rx)) = &self.pending {
            match rx.try_recv() {
                Ok((img, warnings)) => {
                    let k = *k;
                    self.pending = None;
                    self.note(warnings);
                    self.show(k, img);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.pending = None,
            }
        }
        let tex = self
            .tex
            .get_or_insert_with(|| {
                let blank = egui::ColorImage::filled([1, 1], Color32::TRANSPARENT);
                ctx.load_texture("final look", blank, egui::TextureOptions::LINEAR)
            })
            .id();
        if self.shown == Some(key) {
            return GraphView::Ready(tex);
        }
        if self.pending.is_none()
            && let Some(cut) = p.cut(key.at.scene, key.at.cut)
        {
            let cut = cut.clone();
            let law = p.color_law;
            // A frame wider than the device's textures shows as the half
            // proxy (it is a look, not the delivery).
            let side = ctx.input(|i| i.max_texture_side);
            let scale = if key.size.0.max(key.size.1) as usize > side {
                0.5
            } else {
                1.0
            };
            let (tx, rx) = channel();
            let repaint = ctx.clone();
            std::thread::spawn(move || {
                let mut cut = cut;
                deliver::retain_purpose(&mut cut, Purpose::Final);
                let opts = RenderOpts {
                    purpose: Purpose::Final,
                    scale,
                };
                let env = anim_core::export::RenderEnv::new(dir);
                let img = deliver::render_env(&cut, law, key.frame, key.size.0, key.size.1, &opts, &env);
                let bytes: Vec<u8> = img
                    .px
                    .chunks_exact(4)
                    .flat_map(anim_core::export::straight_rgba8)
                    .collect();
                let size = [img.w as usize, img.h as usize];
                let image = Arc::new(egui::ColorImage::from_rgba_unmultiplied(size, &bytes));
                let _ = tx.send((image, env.warnings()));
                repaint.request_repaint();
            });
            self.pending = Some((key, rx));
        }
        GraphView::Rendering(tex)
    }

    /// GM17: what the renders could not read since the last call (each
    /// frame's warnings once).
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    fn note(&mut self, warnings: Vec<String>) {
        for w in warnings {
            if !self.warnings.contains(&w) {
                self.warnings.push(w);
            }
        }
    }

    /// GM22: files on disk changed (a sequence reload): the frame renders
    /// again, the last one kept up meanwhile.
    pub fn reread(&mut self) {
        self.shown = None;
    }

    fn show(&mut self, key: LookKey, img: Arc<egui::ColorImage>) {
        if let Some(t) = &mut self.tex {
            t.set(img.clone(), egui::TextureOptions::LINEAR);
        }
        self.image = Some(img);
        self.shown = Some(key);
    }

    /// Tests: block until the frame on the worker is in (no sleep, no
    /// clock).
    #[cfg(test)]
    pub fn wait(&mut self) {
        if let Some((k, rx)) = self.pending.take()
            && let Ok((img, warnings)) = rx.recv()
        {
            self.note(warnings);
            self.show(k, img);
        }
    }
}

/// Per-instance view state (keyed by the pane's viewer id; session-only).
pub struct ViewerView {
    zoom: f32,
    pan: egui::Vec2,
    /// GM10: the safe-frame overlay (action 93 %, title 90 %) is on.
    pub safe: bool,
    /// GM10: the safe frames drawn this frame, in frame pixels.
    pub(crate) drawn_safe: Option<anim_core::camera::SafeFrames>,
    /// AU7: the verb an empty state's button asked for.
    pub(crate) asked: Option<Action>,
}

impl Default for ViewerView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            safe: false,
            drawn_safe: None,
            asked: None,
        }
    }
}

/// AU7: the Result's empty state (spec 8.4).
pub const NOTHING_TO_SHOW: &str = "Nothing to show yet.";
const SET_UP_COMPOSITE: &str = "Set up standard composite";

/// AU7: an empty view: its sentence, and the one way on under it.
fn empty_state(ui: &mut egui::Ui, rect: Rect, words: &str, button: &str, verb: Action, vs: &mut ViewerView) {
    let gap = crate::plate::CTRL_H * 0.5 + 2.0;
    ui.painter_at(rect).text(
        rect.center() - vec2(0.0, gap),
        egui::Align2::CENTER_BOTTOM,
        words,
        crate::plate::Role::Chrome.font(),
        crate::plate::LEGEND,
    );
    // Measured in the Ui's own font: the plain panel tests dress none.
    let font = egui::TextStyle::Button.resolve(ui.style());
    let wide = ui.fonts_mut(|f| f.layout_no_wrap(button.to_string(), font, crate::plate::LEGEND).size().x) + 24.0;
    let at = Rect::from_center_size(rect.center() + vec2(0.0, 6.0), vec2(wide, crate::plate::CTRL_H));
    if ui.put(at, egui::Button::new(button)).clicked() {
        vs.asked = Some(verb);
    }
}

/// `final_look` is the shared [`FinalLook`] latch: its deck control sits
/// beside "safe", and `graph` is the final look's frame while it is on.
pub fn ui(ui: &mut egui::Ui, state: &AppState, graph: GraphView, vs: &mut ViewerView, final_look: &mut bool) {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let paper_w = state.engine.project.width as f32;
    let paper_h = state.engine.project.height as f32;
    let fit = ((rect.width() / paper_w).min(rect.height() / paper_h) * 0.94).max(0.01);
    let scale = fit * vs.zoom;
    let origin = rect.center() - vec2(paper_w, paper_h) * scale * 0.5 + vs.pan;
    let to_screen = |p: Pos2| -> Pos2 { origin + p.to_vec2() * scale };

    // Zoom at cursor; any drag pans (the viewer is read-only, so the primary
    // button is free); double-click refits.
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0
            && let Some(mouse) = response.hover_pos()
        {
            let before = (mouse - origin) / scale;
            vs.zoom = (vs.zoom * (scroll * 0.0015).exp()).clamp(0.2, 10.0);
            let scale2 = fit * vs.zoom;
            let origin2 = rect.center() - vec2(paper_w, paper_h) * scale2 * 0.5 + vs.pan;
            let after = origin2 + before * scale2;
            vs.pan += mouse - after;
        }
    }
    if response.dragged() {
        vs.pan += response.drag_delta();
    }
    if response.double_clicked() {
        vs.zoom = 1.0;
        vs.pan = egui::Vec2::ZERO;
    }
    let paper_rect =
        Rect::from_min_max(to_screen(pos2(0.0, 0.0)), to_screen(pos2(paper_w, paper_h)));
    painter.rect_filled(rect, 0, crate::plate::RESULT);
    painter.rect_filled(paper_rect, 0, crate::plate::PAPER);
    let hint = |painter: &egui::Painter, msg: &str| {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            crate::plate::Role::Chrome.font(),
            crate::plate::LEGEND,
        );
    };
    match graph {
        GraphView::Ready(id) | GraphView::Rendering(id) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(id, paper_rect, uv, crate::plate::UNTINTED);
            let tag = crate::plate::picture_tag(
                &painter,
                pos2(paper_rect.left() + 8.0, paper_rect.top() + 6.0),
                if *final_look {
                    "🎬 final look"
                } else {
                    "🎬 composite"
                },
            );
            if matches!(graph, GraphView::Rendering(_)) {
                // GM9: the lamp, and in words — an FX render is pending
                // (GM11: or the final look's CPU frame).
                painter.circle_filled(pos2(tag.right() + 14.0, tag.center().y), 4.0, crate::plate::TALLY);
                crate::plate::picture_tag(
                    &painter,
                    pos2(tag.right() + 28.0, tag.top()),
                    if *final_look {
                        "rendering final look…"
                    } else {
                        "rendering FX…"
                    },
                );
            }
        }
        GraphView::NoOutput | GraphView::Off => {
            empty_state(ui, rect, NOTHING_TO_SHOW, SET_UP_COMPOSITE, Action::BuildComposite, vs);
        }
        GraphView::EvalFailed => hint(
            &painter,
            "the graph failed to evaluate — check the Node Graph pane",
        ),
        GraphView::NoGpu => hint(&painter, "GPU unavailable — the viewer needs wgpu"),
    }
    // GM10: the safe frames over the composite, from the camera module (the
    // canvas's safe toggle draws the same rects); the pane's latch turns
    // them on.
    vs.drawn_safe = vs
        .safe
        .then(|| anim_core::camera::safe_frames(paper_w as u32, paper_h as u32));
    if let Some(s) = vs.drawn_safe {
        let g = egui::Stroke::new(1.0, crate::plate::PENCIL_AO);
        for r in [s.action, s.title] {
            let pts = r.corners().iter().map(|&(x, y)| to_screen(pos2(x, y))).collect();
            painter.add(egui::Shape::closed_line(pts, g));
        }
    }
    // The latches ride on a float their own size: the neutral grey around the
    // paper is too close to their words to carry them. Each latch ends in
    // the 8 pt the float starts with.
    let safe_w = crate::plate::latch_width(ui, "safe");
    let look_w = crate::plate::latch_width(ui, "final look");
    let row = Rect::from_min_size(rect.left_bottom() + vec2(4.0, -34.0), vec2(8.0 + safe_w + look_w, 34.0));
    crate::plate::float_ground(ui, row);
    let at = Rect::from_min_size(row.min + vec2(8.0, 4.0), vec2(safe_w, crate::plate::CTRL_H));
    ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
        crate::plate::latch(ui, &mut vs.safe, "safe")
            .on_hover_text("action safe (93%) and title safe (90%) over the composite")
    });
    // GM11: the delivery's look, beside it (a fixed slot: nothing moves).
    let at = Rect::from_min_size(row.min + vec2(8.0 + safe_w, 4.0), vec2(look_w, crate::plate::CTRL_H));
    ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
        crate::plate::latch(ui, final_look, "final look").on_hover_text(
            "the composite as the master delivers it: no rough, reference or correction \
             (sakkan) layers, no Ref columns — rendered on a worker",
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::EditorHarness;

    #[test]
    fn camview_the_viewer_safe_overlay_draws_the_camera_module_frames() {
        let h = EditorHarness::new();
        let state = h.state();
        let (w, fh) = (state.engine.project.width, state.engine.project.height);
        let ctx = egui::Context::default();
        let mut vs = ViewerView::default();
        let mut fin = false;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            super::ui(ui, state, GraphView::Off, &mut vs, &mut fin)
        });
        assert_eq!(vs.drawn_safe, None, "off by default");
        vs.safe = true;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            super::ui(ui, state, GraphView::Off, &mut vs, &mut fin)
        });
        assert_eq!(vs.drawn_safe, Some(anim_core::camera::safe_frames(w, fh)));
    }
}
