//! The composite VIEWER pane — the first multi-instance pane (LENS-DOCK
//! Phase 1). Read-only: it displays the node graph's rendered output (the
//! same texture composite view and export truth come from) with its own
//! zoom/pan, independent per instance. Put one beside the canvas for the
//! Finishing room: paint in the edit canvas, watch the composite live here.
//!
//! Interaction: wheel = zoom at cursor, any drag = pan, double-click = fit.
//! No document mutation happens here, so none of the canvas guards apply.

use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, pos2, vec2};

use crate::doc::AppState;

/// What the shared per-frame graph execution produced — every consumer
/// (canvas composite view, viewer panes) words its empty-state from the
/// ACTUAL cause instead of guessing.
#[derive(Clone, Copy, PartialEq)]
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
}

/// Per-instance view state (keyed by the pane's viewer id; session-only).
pub struct ViewerView {
    zoom: f32,
    pan: egui::Vec2,
}

impl Default for ViewerView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
        }
    }
}

pub fn ui(ui: &mut egui::Ui, state: &AppState, graph: GraphView, vs: &mut ViewerView) {
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
    painter.rect_filled(paper_rect, 2, Color32::from_rgb(242, 239, 233));
    painter.rect_stroke(
        paper_rect,
        2,
        egui::Stroke::new(1.0, Color32::from_gray(60)),
        egui::StrokeKind::Outside,
    );

    let hint = |painter: &egui::Painter, msg: &str| {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            egui::FontId::proportional(14.0),
            Color32::from_gray(150),
        );
    };
    match graph {
        GraphView::Ready(id) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(id, paper_rect, uv, Color32::WHITE);
            painter.text(
                pos2(paper_rect.left() + 8.0, paper_rect.top() + 6.0),
                egui::Align2::LEFT_TOP,
                "🎬 composite",
                egui::FontId::proportional(12.0),
                Color32::from_rgba_unmultiplied(120, 190, 235, 200),
            );
        }
        GraphView::NoOutput | GraphView::Off => hint(
            &painter,
            "no graph output — wire an Output node in the Node Graph pane",
        ),
        GraphView::EvalFailed => hint(
            &painter,
            "the graph failed to evaluate — check the Node Graph pane",
        ),
        GraphView::NoGpu => hint(&painter, "GPU unavailable — the viewer needs wgpu"),
    }
}
