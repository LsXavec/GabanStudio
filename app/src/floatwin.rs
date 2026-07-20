//! LENS-DOCK Phase 5, step 1: a REAL OS window (deferred egui viewport)
//! hosting a read-only composite viewer.
//!
//! This is the deliberate hardware-risk probe the windowing plan calls for:
//! it exercises every dangerous part of multi-window — OS viewport
//! creation, GPU texture sharing across windows (one shared wgpu
//! RenderState/Renderer, so TextureIds stay valid), and the Win11
//! mixed-DPI behavior on the XP-Pen + main-monitor rig — with ZERO input
//! complexity. Only after this window proves itself on the real rig does
//! the editable canvas (pen input, command intents) move out.
//!
//! DEFERRED (not immediate) viewport per the design: the pen display and
//! the main monitor run at different refresh rates; deferred viewports
//! repaint independently. The callback outlives the frame and may run on
//! its own cadence, so it must be Send + Sync + 'static — everything it
//! reads lives behind an Arc.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use egui::mutex::RwLock;
use egui::{Color32, Pos2, Rect, Sense, pos2, vec2};

/// The float window's world, written by the MAIN thread each frame and
/// read (plus zoom/pan mutated) by the viewport callback.
pub struct Shared {
    /// The graph output texture (same registration the in-app viewer panes
    /// use — the Renderer is shared across viewports). None = show `hint`.
    pub tex: Option<egui::TextureId>,
    /// Paper size in pixels (project resolution).
    pub paper: egui::Vec2,
    /// Why there's no image, when there isn't one.
    pub hint: String,
    // View state owned by the float window itself.
    zoom: f32,
    pan: egui::Vec2,
}

pub struct FloatViewer {
    /// Window existence; the window's ✕ stores false here.
    pub open: Arc<AtomicBool>,
    pub shared: Arc<RwLock<Shared>>,
}

impl FloatViewer {
    pub fn new() -> Self {
        Self {
            open: Arc::new(AtomicBool::new(false)),
            shared: Arc::new(RwLock::new(Shared {
                tex: None,
                paper: vec2(1.0, 1.0),
                hint: "no graph output".into(),
                zoom: 1.0,
                pan: egui::Vec2::ZERO,
            })),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }

    pub fn set_open(&self, open: bool) {
        self.open.store(open, Ordering::Relaxed);
    }

    pub const VIEWPORT_TITLE: &'static str = "AnimStudio — Viewer";

    pub fn viewport_id() -> egui::ViewportId {
        egui::ViewportId::from_hash_of("animstudio_float_viewer")
    }

    /// Called every main-thread frame while open: hand the viewport its
    /// callback. `ctx` is the MAIN context — egui routes the callback to
    /// the OS window.
    pub fn show(&self, ctx: &egui::Context) {
        let open = self.open.clone();
        let shared = self.shared.clone();
        ctx.show_viewport_deferred(
            Self::viewport_id(),
            egui::ViewportBuilder::default()
                .with_title(Self::VIEWPORT_TITLE)
                .with_inner_size([760.0, 560.0]),
            move |ctx, class| {
                if class == egui::ViewportClass::EmbeddedWindow {
                    // Backend can't do real OS windows — say so instead of
                    // silently drawing an in-app window.
                    egui::Window::new(Self::VIEWPORT_TITLE).show(ctx, |ui| {
                        ui.label("this platform can't open a real OS window here");
                    });
                    return;
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(ui, &shared);
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    open.store(false, Ordering::Relaxed);
                }
            },
        );
    }
}

/// The viewer body — same interaction law as the in-app viewer pane
/// (wheel = zoom at cursor, drag = pan, double-click = fit), reading
/// `Shared` instead of AppState because it runs across a thread boundary.
fn draw(ui: &mut egui::Ui, shared: &RwLock<Shared>) {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    let mut s = shared.write();
    let (paper_w, paper_h) = (s.paper.x.max(1.0), s.paper.y.max(1.0));
    let fit = ((rect.width() / paper_w).min(rect.height() / paper_h) * 0.94).max(0.01);
    let scale = fit * s.zoom;
    let origin = rect.center() - vec2(paper_w, paper_h) * scale * 0.5 + s.pan;
    let to_screen = |p: Pos2| -> Pos2 { origin + p.to_vec2() * scale };

    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0
            && let Some(mouse) = response.hover_pos()
        {
            let before = (mouse - origin) / scale;
            s.zoom = (s.zoom * (scroll * 0.0015).exp()).clamp(0.2, 10.0);
            let scale2 = fit * s.zoom;
            let origin2 = rect.center() - vec2(paper_w, paper_h) * scale2 * 0.5 + s.pan;
            let after = origin2 + before * scale2;
            s.pan += mouse - after;
        }
    }
    if response.dragged() {
        s.pan += response.drag_delta();
    }
    if response.double_clicked() {
        s.zoom = 1.0;
        s.pan = egui::Vec2::ZERO;
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

    match s.tex {
        Some(id) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(id, paper_rect, uv, Color32::WHITE);
            painter.text(
                pos2(paper_rect.left() + 8.0, paper_rect.top() + 6.0),
                egui::Align2::LEFT_TOP,
                "🎬 composite (OS window)",
                egui::FontId::proportional(12.0),
                Color32::from_rgba_unmultiplied(120, 190, 235, 200),
            );
        }
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &s.hint,
                egui::FontId::proportional(14.0),
                Color32::from_gray(150),
            );
        }
    }
}
