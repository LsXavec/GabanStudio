use eframe::egui;
use egui::{Color32, Pos2, Sense, Stroke, pos2, vec2};

#[derive(Clone, Copy)]
struct StrokePoint {
    pos: Pos2,
    pressure: f32,
}

/// Pen test canvas.
///
/// The diagnostic question this answers: does Windows Ink pen input reach us as
/// egui Touch events WITH pressure (`force`), or only as synthesized pointer
/// events (no pressure)? The event counters make the answer visible live.
pub struct PenCanvas {
    strokes: Vec<Vec<StrokePoint>>,
    current: Vec<StrokePoint>,
    touch_active: bool,

    // Rolling per-second event-rate counters
    touch_count: u32,
    pointer_count: u32,
    touch_rate: u32,
    pointer_rate: u32,
    window_start: f64,

    last_force: Option<f32>,
    saw_force_ever: bool,
}

impl PenCanvas {
    pub fn new() -> Self {
        Self {
            strokes: Vec::new(),
            current: Vec::new(),
            touch_active: false,
            touch_count: 0,
            pointer_count: 0,
            touch_rate: 0,
            pointer_rate: 0,
            window_start: 0.0,
            last_force: None,
            saw_force_ever: false,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("PEN CANVAS")
                .size(14.0)
                .strong()
                .color(Color32::from_rgb(120, 190, 255)),
        );

        ui.horizontal(|ui| {
            if ui.button("clear").clicked() {
                self.strokes.clear();
                self.current.clear();
            }
            let total: usize =
                self.strokes.iter().map(|s| s.len()).sum::<usize>() + self.current.len();
            ui.label(format!("{} strokes / {} pts", self.strokes.len(), total));
        });

        // Pressure verdict — the thing we're actually here to learn
        let (verdict, color) = if self.saw_force_ever {
            (
                format!(
                    "PRESSURE: REAL (last {:.2})",
                    self.last_force.unwrap_or(0.0)
                ),
                Color32::from_rgb(90, 220, 120),
            )
        } else {
            (
                "PRESSURE: none seen yet — draw with the pen".to_string(),
                Color32::from_rgb(230, 200, 80),
            )
        };
        ui.label(egui::RichText::new(verdict).strong().color(color));
        ui.label(format!(
            "events/s   touch: {}   pointer: {}",
            self.touch_rate, self.pointer_rate
        ));
        ui.add_space(4.0);

        // ---- Canvas ----
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4, Color32::from_rgb(20, 22, 26));
        painter.rect_stroke(
            rect,
            4,
            Stroke::new(1.0, Color32::from_gray(70)),
            egui::StrokeKind::Inside,
        );

        // ---- Input ----
        let (events, time, hover) = ui.input(|i| (i.events.clone(), i.time, i.pointer.hover_pos()));

        // Per-second rate window
        if time - self.window_start >= 1.0 {
            self.touch_rate = self.touch_count;
            self.pointer_rate = self.pointer_count;
            self.touch_count = 0;
            self.pointer_count = 0;
            self.window_start = time;
        }

        let mut touch_handled_this_frame = false;
        for event in &events {
            match event {
                egui::Event::Touch {
                    pos, force, phase, ..
                } => {
                    self.touch_count += 1;
                    if let Some(f) = force {
                        self.last_force = Some(*f);
                        if *f > 0.0 {
                            self.saw_force_ever = true;
                        }
                    }
                    if !rect.contains(*pos) {
                        continue;
                    }
                    touch_handled_this_frame = true;
                    let pt = StrokePoint {
                        pos: *pos,
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
                            if self.touch_active && !self.current.is_empty() {
                                self.current.push(pt);
                                self.strokes.push(std::mem::take(&mut self.current));
                            }
                            self.touch_active = false;
                        }
                    }
                }
                egui::Event::PointerMoved(_) => {
                    self.pointer_count += 1;
                }
                _ => {}
            }
        }

        // Mouse fallback (no pressure): only when no touch stream is active,
        // so pen events don't get double-recorded via synthesized pointer events.
        if !self.touch_active && !touch_handled_this_frame {
            if response.drag_started_by(egui::PointerButton::Primary) {
                self.current.clear();
                if let Some(p) = response.interact_pointer_pos() {
                    self.current.push(StrokePoint {
                        pos: p,
                        pressure: 0.5,
                    });
                }
            } else if response.dragged_by(egui::PointerButton::Primary) {
                if let Some(p) = response.interact_pointer_pos() {
                    self.current.push(StrokePoint {
                        pos: p,
                        pressure: 0.5,
                    });
                }
            } else if response.drag_stopped_by(egui::PointerButton::Primary)
                && !self.current.is_empty()
            {
                self.strokes.push(std::mem::take(&mut self.current));
            }
        }

        // ---- Render strokes ----
        for stroke in self.strokes.iter().chain(std::iter::once(&self.current)) {
            for pair in stroke.windows(2) {
                let w = 1.0 + pair[1].pressure * 7.0;
                painter.line_segment(
                    [pair[0].pos, pair[1].pos],
                    Stroke::new(w, Color32::from_gray(235)),
                );
            }
        }

        // ---- Latency crosshair ----
        // The gap between this crosshair and the head of your live stroke is the
        // visible input-to-paint lag. At high fps it should hug the pen tip.
        if let Some(h) = hover {
            if rect.contains(h) {
                let c = Color32::from_rgb(240, 90, 90);
                painter.line_segment(
                    [pos2(h.x - 10.0, h.y), pos2(h.x + 10.0, h.y)],
                    Stroke::new(1.0, c),
                );
                painter.line_segment(
                    [pos2(h.x, h.y - 10.0), pos2(h.x, h.y + 10.0)],
                    Stroke::new(1.0, c),
                );
            }
        }

        painter.text(
            rect.left_top() + vec2(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            "draw here with the pen — crosshair vs stroke head = perceived lag",
            egui::FontId::proportional(11.0),
            Color32::from_gray(110),
        );
    }
}
