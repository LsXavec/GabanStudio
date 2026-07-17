use eframe::egui;
use std::collections::VecDeque;
use std::time::Instant;

const WINDOW: usize = 600; // ~2.5s of history at 240fps

pub struct FrameStats {
    last: Option<Instant>,
    times_ms: VecDeque<f32>,
    // Time actually spent building the UI each frame — excludes the present/vsync/
    // driver-cap wait, so it stays honest under an NVIDIA Max Frame Rate cap.
    work_ms: VecDeque<f32>,
}

impl FrameStats {
    pub fn new() -> Self {
        Self {
            last: None,
            times_ms: VecDeque::with_capacity(WINDOW),
            work_ms: VecDeque::with_capacity(WINDOW),
        }
    }

    pub fn tick_work(&mut self, ms: f32) {
        if self.work_ms.len() >= WINDOW {
            self.work_ms.pop_front();
        }
        self.work_ms.push_back(ms);
    }

    pub fn work_avg_ms(&self) -> f32 {
        let n = self.work_ms.len().min(120);
        if n == 0 {
            return 0.0;
        }
        self.work_ms.iter().rev().take(n).sum::<f32>() / n as f32
    }

    /// Theoretical fps if only UI-build cost existed (driver cap removed).
    pub fn uncapped_fps_estimate(&self) -> f32 {
        let w = self.work_avg_ms();
        if w <= 0.0 { 0.0 } else { 1000.0 / w }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        if let Some(prev) = self.last {
            let dt_ms = now.duration_since(prev).as_secs_f32() * 1000.0;
            if self.times_ms.len() >= WINDOW {
                self.times_ms.pop_front();
            }
            self.times_ms.push_back(dt_ms);
        }
        self.last = Some(now);
    }

    pub fn fps(&self) -> f32 {
        let n = self.times_ms.len().min(120);
        if n == 0 {
            return 0.0;
        }
        let sum: f32 = self.times_ms.iter().rev().take(n).sum();
        if sum <= 0.0 { 0.0 } else { 1000.0 * n as f32 / sum }
    }

    pub fn avg_ms(&self) -> f32 {
        let n = self.times_ms.len().min(120);
        if n == 0 {
            return 0.0;
        }
        self.times_ms.iter().rev().take(n).sum::<f32>() / n as f32
    }

    pub fn p99_ms(&self) -> f32 {
        if self.times_ms.is_empty() {
            return 0.0;
        }
        let mut v: Vec<f32> = self.times_ms.iter().copied().collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((v.len() as f32) * 0.99) as usize;
        v[idx.min(v.len() - 1)]
    }

    pub fn worst_ms(&self) -> f32 {
        self.times_ms.iter().copied().fold(0.0, f32::max)
    }

    /// Bar chart of recent frame times with 240fps / 120fps threshold lines.
    pub fn plot(&self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0, egui::Color32::from_rgb(14, 15, 18));

        let max_ms: f32 = 12.0; // vertical scale: 12ms ceiling
        let n = self.times_ms.len();
        if n > 0 {
            let bar_w = (rect.width() / WINDOW as f32).max(1.0);
            for (i, &ms) in self.times_ms.iter().enumerate() {
                let frac = (ms / max_ms).min(1.0);
                let h = frac * rect.height();
                let x = rect.left() + i as f32 * bar_w;
                let color = if ms <= 4.17 {
                    egui::Color32::from_rgb(70, 190, 100) // >= 240 fps
                } else if ms <= 8.33 {
                    egui::Color32::from_rgb(220, 190, 70) // >= 120 fps
                } else {
                    egui::Color32::from_rgb(230, 80, 80)
                };
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(x, rect.bottom() - h),
                        egui::pos2(x + bar_w, rect.bottom()),
                    ),
                    0,
                    color,
                );
            }
        }

        // Threshold lines
        for (ms, label) in [(4.17f32, "240"), (8.33, "120")] {
            let y = rect.bottom() - (ms / max_ms) * rect.height();
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(1.0, egui::Color32::from_gray(90)),
            );
            painter.text(
                egui::pos2(rect.right() - 4.0, y - 2.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{label} fps"),
                egui::FontId::proportional(10.0),
                egui::Color32::from_gray(150),
            );
        }
    }
}
