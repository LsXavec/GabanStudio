//! AnimStudio — app shell (M2: X-sheet + drawing loop; + New Project flow).
//!
//! Two screens: a startup New Project dialog (sets resolution/fps), then the
//! editor (top bar, left X-sheet, central canvas, status bar).

mod canvas;
mod config;
mod doc;
mod newproject;
mod paint;
mod xsheet_panel;

use config::{Action, Config, FrameLatency, PenConfig, RebindCapture, SettingsCategory};
use doc::AppState;
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use newproject::{FormAction, NewProjectForm};
use paint::PaintLayer;
use std::time::Duration;

fn main() -> eframe::Result<()> {
    // Load config up front so the surface (V-Sync / frame latency) reflects it.
    let config = Config::load();
    let surface = eframe::egui_wgpu::SurfaceConfig {
        present_mode: if config.perf.vsync {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        },
        desired_maximum_frame_latency: Some(match config.perf.frame_latency {
            FrameLatency::Low => 1,
            FrameLatency::Throughput => 2,
        }),
    };
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1720.0, 960.0])
            .with_title("AnimStudio"),
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            surface,
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "AnimStudio",
        native_options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(App::new(cc, config)))
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
    /// User config (rebindable keyboard shortcuts + performance).
    config: Config,
    settings_open: bool,
    settings_category: SettingsCategory,
    /// In-progress rebind capture (Settings).
    capturing: Option<RebindCapture>,
    /// Active GPU backend name (for the Performance page's Renderer row).
    backend: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        let backend = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| format!("{:?}", rs.adapter.get_info().backend))
            .unwrap_or_else(|| "none".to_string());
        Self {
            render_state: cc.wgpu_render_state.clone(),
            editor: None,
            new_form: None,
            config,
            settings_open: false,
            settings_category: SettingsCategory::default(),
            capturing: None,
            backend,
        }
    }

    /// Run an action bound to a keyboard shortcut.
    fn perform(&mut self, action: Action) {
        if action == Action::NewProject {
            self.new_form = Some(NewProjectForm::default());
            return;
        }
        let Some(ed) = &mut self.editor else { return };
        // Handled on the canvas (owns the tool state), before borrowing ed.state.
        if action == Action::ToggleEraser {
            ed.canvas.toggle_eraser();
            return;
        }
        let s = &mut ed.state;
        match action {
            Action::PlayPause => s.toggle_play(),
            Action::NextFrame => s.step(1),
            Action::PrevFrame => s.step(-1),
            Action::FirstFrame => s.goto(0),
            Action::LastFrame => {
                let last = s.frame_count() - 1;
                s.goto(last);
            }
            Action::NewDrawing => s.new_drawing_at_frame(),
            Action::ClearCel => s.clear_current_raster(),
            Action::ClearFrameKey => s.clear_key_at_frame(),
            Action::RemoveColumn => s.remove_active_column(),
            Action::ToggleOnion => s.onion = !s.onion,
            Action::ToggleEraser => {} // handled above
            Action::Undo => s.undo(),
            Action::Redo => s.redo(),
            Action::Save => s.save(false),
            Action::SaveAs => s.save(true),
            Action::Open => s.open(),
            Action::NewProject => {}
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
    /// Set when the user clicks "settings".
    request_settings: bool,
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
            request_settings: false,
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
            request_settings: false,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Nothing open and no dialog -> show the startup dialog.
        if self.editor.is_none() && self.new_form.is_none() {
            self.new_form = Some(NewProjectForm::default());
        }

        // Apply live settings (read as locals to avoid borrowing self.config
        // while self.editor is borrowed mutably).
        let undo_limit = self.config.perf.undo_limit;
        let canvas_filter = self.config.perf.canvas_filter.wgpu();
        let pen = self.config.pen.clone();

        // Editor (if any) renders first as the base layer.
        if let Some(editor) = &mut self.editor {
            editor.state.engine.set_undo_limit(undo_limit);
            if let Some(p) = &mut editor.paint {
                p.set_filter(canvas_filter);
            }
            editor.ui(ui, &pen);
            if editor.request_new {
                editor.request_new = false;
                self.new_form = Some(NewProjectForm::default());
            }
            if editor.request_settings {
                editor.request_settings = false;
                self.settings_open = true;
            }
        }

        // Keyboard-shortcut dispatch — skipped while a dialog is up or while
        // capturing a rebind (so the captured key doesn't also fire an action).
        if self.editor.is_some() && self.new_form.is_none() && self.capturing.is_none() {
            let fired: Vec<Action> = ui.ctx().input(|i| {
                Action::ALL
                    .iter()
                    .copied()
                    .filter(|a| self.config.triggered(*a, i))
                    .collect()
            });
            for action in fired {
                self.perform(action);
            }
        }

        // Settings window (Krita-style: shortcuts + performance).
        config::settings_window(
            ui.ctx(),
            &mut self.settings_open,
            &mut self.config,
            &mut self.capturing,
            &mut self.settings_category,
            &self.backend,
        );

        // Optional FPS overlay.
        if self.config.perf.show_fps {
            let dt = ui.ctx().input(|i| i.stable_dt).max(1e-4);
            egui::Area::new("fps_overlay".into())
                .anchor(egui::Align2::RIGHT_TOP, [-10.0, 34.0])
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(egui::Color32::from_black_alpha(160))
                        .inner_margin(4.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("{:.0} fps", 1.0 / dt))
                                    .monospace()
                                    .color(egui::Color32::from_rgb(120, 220, 140)),
                            );
                        });
                });
        }

        // New Project dialog: full-screen when no project, modal window over one.
        if self.new_form.is_some() {
            self.show_new_dialog(ui);
        }

        // Cap the redraw rate (playback still runs at the project frame rate).
        let playing_fps = self
            .editor
            .as_ref()
            .filter(|e| e.state.playing)
            .map(|e| e.state.fps());
        let target = playing_fps
            .map(|f| f.max(self.config.perf.fps_cap))
            .unwrap_or(self.config.perf.fps_cap)
            .max(1);
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f32(1.0 / target as f32));
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
    fn ui(&mut self, ui: &mut egui::Ui, pen: &PenConfig) {
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        self.state.tick(dt);

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
                if ui.button("⚙ settings").on_hover_text("keyboard shortcuts & config").clicked() {
                    self.request_settings = true;
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
                            "space play · S/F prev/next frame · E new cel · D clear · O onion · ctrl+Z undo · wheel zoom · mid-drag pan · ⚙ settings to rebind",
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
                self.canvas.ui(ui, &mut self.state, self.paint.as_mut(), pen);
            });
    }
}
