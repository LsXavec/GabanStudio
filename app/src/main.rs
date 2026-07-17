//! AnimStudio — app shell (M2: the X-sheet + drawing loop).
//! Panels: top bar (files/undo/transport), left X-sheet, central canvas.

mod canvas;
mod doc;
mod xsheet_panel;

use doc::AppState;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1720.0, 960.0])
            .with_title("AnimStudio"),
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
        "AnimStudio",
        native_options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(App::new()))
        }),
    )
}

struct App {
    state: AppState,
    canvas: canvas::CanvasView,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::new_default(),
            canvas: canvas::CanvasView::new(),
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let state = &mut self.state;
        ctx.input(|i| {
            let ctrl = i.modifiers.ctrl || i.modifiers.command;
            if i.key_pressed(egui::Key::Space) {
                state.toggle_play();
            }
            if i.key_pressed(egui::Key::ArrowLeft) {
                state.step(-1);
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                state.step(1);
            }
            if !ctrl && i.key_pressed(egui::Key::O) {
                state.onion = !state.onion;
            }
            if !ctrl && i.key_pressed(egui::Key::N) {
                state.new_drawing_at_frame();
            }
            if ctrl && i.key_pressed(egui::Key::Z) {
                state.undo();
            }
            if ctrl && i.key_pressed(egui::Key::Y) {
                state.redo();
            }
            if ctrl && i.key_pressed(egui::Key::S) {
                state.save(i.modifiers.shift);
            }
            if ctrl && i.key_pressed(egui::Key::O) {
                state.open();
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        self.state.tick(dt);
        self.shortcuts(ui.ctx());

        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("AnimStudio")
                        .strong()
                        .size(15.0)
                        .color(egui::Color32::from_rgb(120, 190, 255)),
                );
                ui.separator();
                if ui.button("open").clicked() {
                    self.state.open();
                }
                if ui.button("save").clicked() {
                    self.state.save(false);
                }
                ui.separator();

                let can_undo = self.state.engine.can_undo();
                let can_redo = self.state.engine.can_redo();
                if ui.add_enabled(can_undo, egui::Button::new("undo")).clicked() {
                    self.state.undo();
                }
                if ui.add_enabled(can_redo, egui::Button::new("redo")).clicked() {
                    self.state.redo();
                }
                ui.separator();

                // Transport.
                if ui.button("|<").clicked() {
                    self.state.goto(0);
                }
                if ui.button("<").clicked() {
                    self.state.step(-1);
                }
                let play_text = if self.state.playing { "stop" } else { "play" };
                if ui.button(play_text).clicked() {
                    self.state.toggle_play();
                }
                if ui.button(">").clicked() {
                    self.state.step(1);
                }
                if ui.button(">|").clicked() {
                    let last = self.state.frame_count() - 1;
                    self.state.goto(last);
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{:>3} / {}",
                        self.state.frame + 1,
                        self.state.frame_count()
                    ))
                    .monospace(),
                );
                ui.separator();
                ui.checkbox(&mut self.state.onion, "onion");
            });
        });

        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&self.state.status).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(
                            "space play · ←→ step · O onion · N new drawing · ctrl+Z undo · wheel zoom · mid-drag pan",
                        )
                        .weak()
                        .size(10.5),
                    );
                });
            });
        });

        egui::Panel::left("xsheet_panel")
            .default_size(330.0)
            .min_size(260.0)
            .resizable(true)
            .show(ui, |ui| {
                xsheet_panel::ui(ui, &mut self.state);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(egui::Color32::from_rgb(24, 26, 30)))
            .show(ui, |ui| {
                self.canvas.ui(ui, &mut self.state);
            });

        // Continuous repaint: playback timing + pen latency both want it.
        ui.ctx().request_repaint();
    }
}
