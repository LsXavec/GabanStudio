//! AnimStudio — app shell (M2: X-sheet + drawing loop; + New Project flow).
//!
//! Two screens: a startup New Project dialog (sets resolution/fps), then the
//! editor (top bar, left X-sheet, central canvas, status bar).

mod canvas;
mod doc;
mod newproject;
mod paint;
mod xsheet_panel;

use doc::AppState;
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use newproject::{FormAction, NewProjectForm};
use paint::PaintLayer;

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
            Ok(Box::new(App::new(cc)))
        }),
    )
}

struct App {
    /// Shared wgpu context from eframe (for the GPU raster paint layer).
    render_state: Option<RenderState>,
    /// The open project (None until one is created/opened).
    editor: Option<Editor>,
    /// When Some, the New Project dialog is showing (startup, or "New…").
    new_form: Option<NewProjectForm>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            render_state: cc.wgpu_render_state.clone(),
            editor: None,
            new_form: None,
        }
    }
}

struct Editor {
    state: AppState,
    canvas: canvas::CanvasView,
    /// GPU raster paint layer (None if wgpu is unavailable).
    paint: Option<PaintLayer>,
    /// Set when the user clicks "New…"; the App picks it up to show the dialog.
    request_new: bool,
}

impl Editor {
    fn from_form(form: &NewProjectForm, rs: Option<&RenderState>) -> Self {
        let state = AppState::new_project(
            form.name.clone(),
            form.width,
            form.height,
            form.fps,
            form.dpi as f32,
        );
        let paint = rs.map(|rs| PaintLayer::new(rs, form.width, form.height));
        Self {
            state,
            canvas: canvas::CanvasView::new(),
            paint,
            request_new: false,
        }
    }

    fn from_state(state: AppState, rs: Option<&RenderState>) -> Self {
        let (w, h) = (state.engine.project.width, state.engine.project.height);
        let paint = rs.map(|rs| PaintLayer::new(rs, w, h));
        Self {
            state,
            canvas: canvas::CanvasView::new(),
            paint,
            request_new: false,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Nothing open and no dialog -> show the startup dialog.
        if self.editor.is_none() && self.new_form.is_none() {
            self.new_form = Some(NewProjectForm::default());
        }

        // Editor (if any) renders first as the base layer.
        if let Some(editor) = &mut self.editor {
            editor.ui(ui);
            if editor.request_new {
                editor.request_new = false;
                self.new_form = Some(NewProjectForm::default());
            }
        }

        // New Project dialog: full-screen when no project, modal window over one.
        if self.new_form.is_some() {
            self.show_new_dialog(ui);
        }

        ui.ctx().request_repaint();
    }
}

impl App {
    fn show_new_dialog(&mut self, ui: &mut egui::Ui) {
        let has_editor = self.editor.is_some();
        let mut form = self.new_form.take().expect("dialog shown with a form");
        let mut action = None;

        if has_editor {
            egui::Window::new("New Project")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    action = newproject::form_ui(ui, &mut form, true);
                });
        } else {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    egui::Frame::group(ui.style())
                        .inner_margin(24.0)
                        .show(ui, |ui| {
                            ui.set_max_width(440.0);
                            action = newproject::form_ui(ui, &mut form, false);
                        });
                });
            });
        }

        match action {
            Some(FormAction::Create) => {
                self.editor = Some(Editor::from_form(&form, self.render_state.as_ref()));
            }
            Some(FormAction::Open) => match AppState::pick_and_open() {
                Some(Ok(state)) => {
                    self.editor = Some(Editor::from_state(state, self.render_state.as_ref()));
                }
                Some(Err(_)) | None => {
                    // Load failed or cancelled — keep the dialog open.
                    self.new_form = Some(form);
                }
            },
            Some(FormAction::Cancel) => {
                // Only offered when an editor exists behind the dialog.
            }
            None => {
                self.new_form = Some(form); // still editing the form
            }
        }
    }
}

impl Editor {
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

    fn ui(&mut self, ui: &mut egui::Ui) {
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
                if ui.button("New…").clicked() {
                    self.request_new = true;
                }
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
                ui.separator();
                ui.label(
                    egui::RichText::new(format!(
                        "{}×{}  {}fps",
                        self.state.engine.project.width,
                        self.state.engine.project.height,
                        self.state.fps()
                    ))
                    .weak(),
                );
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
                self.canvas.ui(ui, &mut self.state, self.paint.as_mut());
            });
    }
}
