//! The drawing canvas: a fixed-size logical "paper" (resolution independent)
//! rendered into the panel with pan/zoom. Pen strokes land in paper
//! coordinates so artwork never depends on window size or zoom level.
//!
//! Input reuses the M0-validated path: Windows Ink pen arrives as egui Touch
//! events with real pressure; mouse is the no-pressure fallback.

use anim_core::ids::ColumnId;
use anim_core::model::{Stroke, StrokePoint};
use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, pos2, vec2};

use crate::doc::AppState;

pub const PAPER_W: f32 = 1920.0;
pub const PAPER_H: f32 = 1080.0;

const SWATCHES: [[u8; 4]; 4] = [
    [25, 25, 30, 255],   // ink
    [200, 40, 40, 255],  // red (shadow lines, corrections)
    [40, 80, 200, 255],  // blue (roughs/layout, like blue pencil)
    [245, 245, 245, 255],// white
];

pub struct CanvasView {
    zoom: f32,
    pan: egui::Vec2,
    pub brush_width: f32,
    pub brush_color: [u8; 4],
    current: Vec<StrokePoint>,
    touch_active: bool,
}

impl CanvasView {
    pub fn new() -> Self {
        Self {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            brush_width: 3.0,
            brush_color: SWATCHES[0],
            current: Vec::new(),
            touch_active: false,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, state: &mut AppState) {
        // ---- Toolbar ------------------------------------------------------
        ui.horizontal(|ui| {
            ui.add(
                egui::Slider::new(&mut self.brush_width, 0.5..=16.0)
                    .text("brush")
                    .fixed_decimals(1),
            );
            ui.separator();
            for c in SWATCHES {
                let selected = self.brush_color == c;
                let swatch = egui::RichText::new("⬤")
                    .size(16.0)
                    .color(Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]));
                if ui.selectable_label(selected, swatch).clicked() {
                    self.brush_color = c;
                }
            }
            ui.separator();
            if ui.button("fit view").clicked() {
                self.zoom = 1.0;
                self.pan = egui::Vec2::ZERO;
            }
            if state.playing {
                ui.separator();
                ui.label(
                    egui::RichText::new("PLAYING (space to stop)")
                        .color(Color32::from_rgb(120, 220, 140)),
                );
            }
        });

        // ---- Canvas area --------------------------------------------------
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        // View transform: fit paper into rect, then user zoom/pan on top.
        let fit = ((rect.width() / PAPER_W).min(rect.height() / PAPER_H) * 0.94).max(0.01);
        let scale = fit * self.zoom;
        let origin = rect.center() - vec2(PAPER_W, PAPER_H) * scale * 0.5 + self.pan;
        let to_screen = |p: Pos2| -> Pos2 { origin + p.to_vec2() * scale };
        let to_paper = |s: Pos2| -> Pos2 { ((s - origin) / scale).to_pos2() };

        // Zoom at cursor.
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0
                && let Some(mouse) = response.hover_pos() {
                    let before = to_paper(mouse);
                    self.zoom = (self.zoom * (scroll * 0.0015).exp()).clamp(0.2, 10.0);
                    let scale2 = fit * self.zoom;
                    let origin2 = rect.center() - vec2(PAPER_W, PAPER_H) * scale2 * 0.5 + self.pan;
                    let after = origin2 + before.to_vec2() * scale2;
                    self.pan += mouse - after;
                }
        }
        // Middle-drag pans.
        if response.dragged_by(egui::PointerButton::Middle) {
            self.pan += response.drag_delta();
        }

        // ---- Paper --------------------------------------------------------
        let paper_rect = Rect::from_min_max(to_screen(pos2(0.0, 0.0)), to_screen(pos2(PAPER_W, PAPER_H)));
        painter.rect_filled(paper_rect, 2, Color32::from_rgb(242, 239, 233));
        painter.rect_stroke(
            paper_rect,
            2,
            egui::Stroke::new(1.0, Color32::from_gray(60)),
            egui::StrokeKind::Outside,
        );

        // ---- Pen input (edit mode only) -----------------------------------
        if !state.playing {
            self.handle_pen(ui, &response, rect, &to_paper, state);
        } else {
            self.current.clear();
            self.touch_active = false;
        }

        // ---- Render layers ------------------------------------------------
        let cut = state.cut();
        let active_col = state.active_column;
        let frame = state.frame;

        // 1. Non-active columns (in sheet order = layer order).
        for col in &cut.xsheet.columns {
            if col.id == active_col {
                continue;
            }
            if let Some(id) = col.resolve(frame)
                && let Some(d) = cut.drawing(id) {
                    draw_strokes(&painter, &d.strokes, &to_screen, scale, None);
                }
        }

        // 2. Onion ghosts of the active column, under its current drawing.
        if state.onion {
            let ghosts = onion_ghosts(state, active_col, frame);
            for (id, tint) in ghosts {
                if let Some(d) = cut.drawing(id) {
                    draw_strokes(&painter, &d.strokes, &to_screen, scale, Some(tint));
                }
            }
        }

        // 3. Active column's current drawing on top.
        if let Some(id) = state.resolve_at(active_col, frame)
            && let Some(d) = state.cut().drawing(id) {
                draw_strokes(&painter, &d.strokes, &to_screen, scale, None);
            }

        // 4. In-progress stroke preview.
        if self.current.len() >= 2 {
            let c = self.brush_color;
            let color = Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]);
            for pair in self.current.windows(2) {
                let w = (self.brush_width * pair[1].pressure * scale).max(0.5);
                painter.line_segment(
                    [
                        to_screen(pos2(pair[0].x, pair[0].y)),
                        to_screen(pos2(pair[1].x, pair[1].y)),
                    ],
                    egui::Stroke::new(w, color),
                );
            }
        }

        // Empty-cell hint.
        if !state.playing && state.current_drawing().is_none() && self.current.is_empty() {
            painter.text(
                paper_rect.center(),
                egui::Align2::CENTER_CENTER,
                "empty cell — draw to create a new drawing here",
                egui::FontId::proportional(15.0),
                Color32::from_gray(150),
            );
        }
    }

    fn handle_pen(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        to_paper: &impl Fn(Pos2) -> Pos2,
        state: &mut AppState,
    ) {
        let events = ui.input(|i| i.events.clone());
        let mut touch_seen = false;

        for event in &events {
            if let egui::Event::Touch {
                pos, force, phase, ..
            } = event
            {
                if !rect.contains(*pos) && !self.touch_active {
                    continue;
                }
                touch_seen = true;
                let p = to_paper(*pos);
                let pt = StrokePoint {
                    x: p.x,
                    y: p.y,
                    pressure: force.unwrap_or(0.5).max(0.05),
                };
                match phase {
                    egui::TouchPhase::Start => {
                        self.touch_active = true;
                        self.current.clear();
                        self.current.push(pt);
                    }
                    egui::TouchPhase::Move => {
                        if self.touch_active {
                            self.current.push(pt);
                        }
                    }
                    egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                        if self.touch_active {
                            self.current.push(pt);
                            self.finish_stroke(state);
                        }
                        self.touch_active = false;
                    }
                }
            }
        }

        // Mouse fallback (pressure 0.5), suppressed while a pen stream is live.
        if !self.touch_active && !touch_seen {
            if response.drag_started_by(egui::PointerButton::Primary) {
                self.current.clear();
                if let Some(p) = response.interact_pointer_pos() {
                    let p = to_paper(p);
                    self.current.push(StrokePoint {
                        x: p.x,
                        y: p.y,
                        pressure: 0.5,
                    });
                }
            } else if response.dragged_by(egui::PointerButton::Primary) {
                if let Some(p) = response.interact_pointer_pos() {
                    let p = to_paper(p);
                    self.current.push(StrokePoint {
                        x: p.x,
                        y: p.y,
                        pressure: 0.5,
                    });
                }
            } else if response.drag_stopped_by(egui::PointerButton::Primary)
                && !self.current.is_empty()
            {
                self.finish_stroke(state);
            }
        }
    }

    fn finish_stroke(&mut self, state: &mut AppState) {
        if self.current.len() < 2 {
            self.current.clear();
            return;
        }
        let stroke = Stroke {
            points: std::mem::take(&mut self.current),
            base_width: self.brush_width,
            color: self.brush_color,
        };
        state.commit_stroke(stroke);
    }
}

/// Previous/next distinct drawings on a column, with their ghost tints.
/// Classic onion convention: red = before, green = after, fading with depth.
fn onion_ghosts(
    state: &AppState,
    column: ColumnId,
    frame: u32,
) -> Vec<(anim_core::ids::DrawingId, Color32)> {
    let current = state.resolve_at(column, frame);
    let mut out = Vec::new();

    let mut collect = |range: Box<dyn Iterator<Item = u32>>, base: (u8, u8, u8)| {
        let mut found: Vec<anim_core::ids::DrawingId> = Vec::new();
        for f in range {
            if let Some(id) = state.resolve_at(column, f)
                && Some(id) != current && !found.contains(&id) {
                    found.push(id);
                    if found.len() == 2 {
                        break;
                    }
                }
        }
        for (depth, id) in found.into_iter().enumerate() {
            let alpha = if depth == 0 { 100 } else { 55 };
            out.push((
                id,
                Color32::from_rgba_unmultiplied(base.0, base.1, base.2, alpha),
            ));
        }
    };

    collect(Box::new((0..frame).rev()), (215, 70, 70));
    collect(
        Box::new(frame + 1..state.frame_count()),
        (70, 175, 90),
    );
    out
}

fn draw_strokes(
    painter: &egui::Painter,
    strokes: &[Stroke],
    to_screen: &impl Fn(Pos2) -> Pos2,
    scale: f32,
    tint: Option<Color32>,
) {
    for stroke in strokes {
        let c = stroke.color;
        let color =
            tint.unwrap_or_else(|| Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]));
        for pair in stroke.points.windows(2) {
            let w = (stroke.base_width * pair[1].pressure * scale).max(0.5);
            painter.line_segment(
                [
                    to_screen(pos2(pair[0].x, pair[0].y)),
                    to_screen(pos2(pair[1].x, pair[1].y)),
                ],
                egui::Stroke::new(w, color),
            );
        }
    }
}
