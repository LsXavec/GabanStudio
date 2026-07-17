mod canvas;
mod fps;
mod node_graph;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1720.0, 960.0])
            .with_title("AnimStudio M0 Spike — stack validation"),
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            surface: eframe::egui_wgpu::SurfaceConfig {
                present_mode: wgpu::PresentMode::AutoNoVsync,
                desired_maximum_frame_latency: Some(1),
            },
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "AnimStudio M0 Spike",
        native_options,
        Box::new(|cc| Ok(Box::new(M0App::new(cc)))),
    )
}

struct M0App {
    stats: fps::FrameStats,
    graph: node_graph::NodeGraph,
    canvas: canvas::PenCanvas,
}

impl M0App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        Self {
            stats: fps::FrameStats::new(),
            graph: node_graph::NodeGraph::generate(500),
            canvas: canvas::PenCanvas::new(),
        }
    }
}

impl eframe::App for M0App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let work_start = std::time::Instant::now();
        self.stats.tick();

        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("M0 SPIKE")
                        .size(16.0)
                        .strong()
                        .color(egui::Color32::from_rgb(120, 190, 255)),
                );
                ui.separator();

                let fps = self.stats.fps();
                let fps_color = if fps >= 240.0 {
                    egui::Color32::from_rgb(90, 220, 120)
                } else if fps >= 120.0 {
                    egui::Color32::from_rgb(230, 200, 80)
                } else {
                    egui::Color32::from_rgb(240, 90, 90)
                };
                ui.label(
                    egui::RichText::new(format!("{fps:>5.0} FPS"))
                        .size(20.0)
                        .strong()
                        .color(fps_color),
                );
                ui.label(format!(
                    "avg {:.2} ms   p99 {:.2} ms   worst {:.2} ms",
                    self.stats.avg_ms(),
                    self.stats.p99_ms(),
                    self.stats.worst_ms()
                ));
                ui.separator();
                // Honest number under a driver fps cap: pure UI-build cost + what
                // fps that cost alone would allow.
                ui.label(
                    egui::RichText::new(format!(
                        "work {:.2} ms  (uncapped ≈{:.0} fps)",
                        self.stats.work_avg_ms(),
                        self.stats.uncapped_fps_estimate()
                    ))
                    .color(egui::Color32::from_rgb(150, 200, 255)),
                );
                ui.separator();

                ui.label("nodes:");
                for n in [100usize, 500, 1000, 2000] {
                    if ui.button(format!("{n}")).clicked() {
                        self.graph = node_graph::NodeGraph::generate(n);
                    }
                }
                ui.checkbox(&mut self.graph.cull, "cull offscreen");
                ui.label(format!(
                    "painted {}/{}",
                    self.graph.painted_last_frame,
                    self.graph.node_count()
                ));
                ui.separator();
                ui.label("vsync OFF (AutoNoVsync)");
            });
        });

        egui::Panel::bottom("frame_plot")
            .exact_size(78.0)
            .show(ui, |ui| {
                self.stats.plot(ui);
            });

        egui::Panel::right("pen_panel")
            .default_size(430.0)
            .min_size(320.0)
            .resizable(true)
            .show(ui, |ui| {
                self.canvas.ui(ui);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(egui::Color32::from_rgb(18, 20, 24)))
            .show(ui, |ui| {
                self.graph.ui(ui);
            });

        self.stats
            .tick_work(work_start.elapsed().as_secs_f32() * 1000.0);

        // Uncapped: repaint immediately, every frame. This is the whole point of the spike.
        ui.ctx().request_repaint();
    }
}
