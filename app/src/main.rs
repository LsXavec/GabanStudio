//! AnimStudio — app shell (M2: X-sheet + drawing loop; + New Project flow).
//!
//! Two screens: a startup New Project dialog (sets resolution/fps), then the
//! editor (top bar, left X-sheet, central canvas, status bar).

mod canvas;
mod config;
mod doc;
mod export;
mod graphcomp;
mod kpp;
mod newproject;
mod nodegraph_panel;
mod paint;
mod workspace;
mod xsheet_panel;

use config::{Action, BrushPreset, Config, FrameLatency, LayersConfig, PenConfig, RebindCapture, SettingsCategory};
use doc::AppState;
use egui_dock::{DockArea, DockState};
use workspace::{Pane, Workspace, Workspaces, draw_workspace};
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use newproject::{FormAction, NewProjectForm};
use canvas::{PenPhase, PenSample};
use paint::PaintLayer;
use std::time::Duration;

fn main() -> eframe::Result<()> {
    // Headless layout probe: runs the REAL panel/canvas layout on the CPU and
    // prints the canvas rect per scripted step, so any drawing-area movement
    // is a measurable number instead of an on-rig observation.
    if std::env::var_os("ANIMSTUDIO_PROBE").is_some() {
        probe();
        return Ok(());
    }
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
    /// Native tablet input: samples streamed from the dedicated tablet
    /// thread (the octotablet Manager lives ENTIRELY on that thread — RTS
    /// init can block on window messages, which froze the app when done on
    /// the UI thread; a worker can block harmlessly while the UI pumps).
    tablet_rx: Option<std::sync::mpsc::Receiver<PenSample>>,
    /// The spawn ran (attempted once, on the first frame).
    tablet_boot_tried: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        let backend = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| format!("{:?}", rs.adapter.get_info().backend))
            .unwrap_or_else(|| "none".to_string());
        // The Manager is built LAZILY on the first UI frame, NOT here:
        // RealTimeStylus initialization on the UI thread can block on window
        // messages, and App::new runs BEFORE the event loop pumps — building
        // here deadlocked the app at startup (Event Log: AppHangB1).
        Self {
            render_state: cc.wgpu_render_state.clone(),
            editor: None,
            new_form: None,
            config,
            settings_open: false,
            settings_category: SettingsCategory::default(),
            capturing: None,
            backend,
            tablet_rx: None,
            tablet_boot_tried: false,
        }
    }

    /// Collect this frame's native pen samples from the tablet thread.
    /// A disconnected channel (thread failed/panicked) drops the backend for
    /// the session — egui Touch keeps working.
    fn pump_tablet(&mut self) -> Vec<PenSample> {
        let Some(rx) = &self.tablet_rx else {
            return Vec::new();
        };
        let mut out = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(s) => out.push(s),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    eprintln!(
                        "native tablet thread ended — falling back to standard pen input"
                    );
                    self.tablet_rx = None;
                    break;
                }
            }
        }
        out
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
        // Tool switches are canvas-owned; set_tool refuses mid-stroke itself
        // and commits any floating transform when leaving Select.
        if action == Action::SelectTool {
            let next = if ed.canvas.tool == canvas::CanvasTool::Select {
                canvas::CanvasTool::Paint
            } else {
                canvas::CanvasTool::Select
            };
            ed.canvas.set_tool(next, &mut ed.state);
            return;
        }
        if action == Action::BrushTool {
            ed.canvas.set_tool(canvas::CanvasTool::Paint, &mut ed.state);
            return;
        }
        if action == Action::FillTool {
            let next = if ed.canvas.tool == canvas::CanvasTool::Fill {
                canvas::CanvasTool::Paint
            } else {
                canvas::CanvasTool::Fill
            };
            ed.canvas.set_tool(next, &mut ed.state);
            return;
        }
        if action == Action::SelectAll {
            let (w, h) = (
                ed.state.engine.project.width as f32,
                ed.state.engine.project.height as f32,
            );
            ed.canvas.select_all(&mut ed.state, w, h);
            return;
        }
        // Composite view swaps what the canvas renders — blocked mid-stroke
        // (the live stroke's sandwich would vanish under it).
        if action == Action::ToggleCompositeView {
            if !ed.canvas.stroke_active() {
                ed.canvas.composite_view = !ed.canvas.composite_view;
                ed.state.status = if ed.canvas.composite_view {
                    "composite view — the node graph's output (C to edit)".into()
                } else {
                    "edit view".into()
                };
            }
            return;
        }
        // Brush presets apply to the canvas; blocked mid-stroke (dab size/alpha
        // read live state — a mid-stroke swap would bend the stroke).
        let preset_idx = match action {
            Action::Preset1 => Some(0),
            Action::Preset2 => Some(1),
            Action::Preset3 => Some(2),
            Action::Preset4 => Some(3),
            Action::Preset5 => Some(4),
            Action::Preset6 => Some(5),
            Action::Preset7 => Some(6),
            Action::Preset8 => Some(7),
            _ => None,
        };
        if let Some(i) = preset_idx {
            if !ed.canvas.stroke_active()
                && let Some(p) = self.config.presets.get(i)
            {
                ed.canvas.apply_preset(p);
                ed.state.status = format!("brush: {}", p.name);
            }
            return;
        }
        // Save/Open land any in-flight gesture FIRST: the saved file must
        // contain what the screen shows, and an Open's commit must not cross
        // into the newly loaded project.
        if matches!(action, Action::Save | Action::SaveAs | Action::Open) {
            ed.canvas.finish_gesture(&mut ed.state);
        }
        // STROKE GUARD: while a stroke is live, actions that would retarget or
        // orphan its pen-up commit are dropped — frame nav would commit onto
        // the wrong frame, an A-cycle onto the wrong layer, undo/clear against
        // a vanished target. (Tool toggles are safe: the stroke latches them.)
        if ed.canvas.stroke_active()
            && matches!(
                action,
                Action::PlayPause
                    | Action::NextFrame
                    | Action::PrevFrame
                    | Action::FirstFrame
                    | Action::LastFrame
                    | Action::NewDrawing
                    | Action::ClearCel
                    | Action::ClearCelAll
                    | Action::CycleCelLayer
                    | Action::CycleCelLayerBack
                    | Action::ClearFrameKey
                    | Action::RemoveColumn
                    | Action::Undo
                    | Action::Redo
            )
        {
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
            Action::ClearCelAll => s.clear_current_cel_all(),
            Action::ClearFrameKey => s.clear_key_at_frame(),
            Action::RemoveColumn => s.remove_active_column(),
            Action::ToggleOnion => s.onion = !s.onion,
            Action::ToggleEraser
            | Action::ToggleCompositeView
            | Action::SelectTool
            | Action::BrushTool
            | Action::FillTool
            | Action::SelectAll => {} // handled above
            Action::Preset1
            | Action::Preset2
            | Action::Preset3
            | Action::Preset4
            | Action::Preset5
            | Action::Preset6
            | Action::Preset7
            | Action::Preset8 => {} // handled above
            Action::CycleCelLayer => s.cycle_layer(false),
            Action::CycleCelLayerBack => s.cycle_layer(true),
            Action::Undo => s.undo(),
            Action::Redo => s.redo(),
            Action::Save => s.save(false),
            Action::SaveAs => s.save(true),
            Action::Open => s.open(),
            Action::NewProject => {}
        }
    }
}

/// Spawn the dedicated tablet thread. The octotablet Manager is built AND
/// pumped there: RealTimeStylus initialization can block on window messages,
/// which froze the app when done on the UI thread (Event Log: AppHangB1) —
/// on a worker thread it blocks harmlessly while the UI keeps pumping.
/// Returns None if the window handle isn't extractable.
fn spawn_tablet_thread(
    frame: &eframe::Frame,
    ctx: egui::Context,
) -> Option<std::sync::mpsc::Receiver<PenSample>> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

    // Extract the Win32 handle parts (plain integers — sendable) on the UI
    // thread; the worker reconstructs a handle carrier from them.
    let raw = frame.window_handle().ok()?.as_raw();
    let RawWindowHandle::Win32(w32) = raw else {
        return None;
    };
    let hwnd = w32.hwnd;
    let hinstance = w32.hinstance;
    frame.display_handle().ok()?; // Windows display handle is a unit

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("tablet-input".into())
        .spawn(move || {
            // COM for this thread: octotablet only CoCreateInstances (it
            // relies on the caller's apartment). MTA = no message pumping
            // required here.
            #[link(name = "ole32")]
            unsafe extern "system" {
                fn CoInitializeEx(reserved: *mut std::ffi::c_void, coinit: u32) -> i32;
            }
            const COINIT_MULTITHREADED: u32 = 0x0;
            unsafe {
                CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED);
            }

            struct Handles {
                hwnd: std::num::NonZeroIsize,
                hinstance: Option<std::num::NonZeroIsize>,
            }
            impl HasWindowHandle for Handles {
                fn window_handle(
                    &self,
                ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
                {
                    let mut h = raw_window_handle::Win32WindowHandle::new(self.hwnd);
                    h.hinstance = self.hinstance;
                    // SAFETY: the eframe window outlives the app; an HWND is
                    // an opaque token — a stale one fails calls, it can't UB.
                    Ok(unsafe {
                        raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::Win32(h))
                    })
                }
            }
            impl HasDisplayHandle for Handles {
                fn display_handle(
                    &self,
                ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError>
                {
                    Ok(raw_window_handle::DisplayHandle::windows())
                }
            }
            let handles = Handles { hwnd, hinstance };

            // SAFETY: see Handles::window_handle.
            let built = std::panic::catch_unwind(|| unsafe {
                octotablet::Builder::new().build_raw(&handles)
            });
            let Ok(Ok(mut mgr)) = built else {
                eprintln!("native tablet backend failed to attach — using fallback pen input");
                return; // rx disconnects; the app falls back
            };

            let mut down = false;
            let mut last: Option<egui::Pos2> = None;
            let mut last_tilt: Option<[f32; 2]> = None;
            loop {
                let ppp = ctx.pixels_per_point();
                let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    drain_tablet(&mut mgr, ppp, down, last, last_tilt)
                }));
                let Ok((samples, d2, l2, t2)) = step else {
                    eprintln!("native tablet backend panicked — disabled for this session");
                    return;
                };
                down = d2;
                last = l2;
                last_tilt = t2;
                if !samples.is_empty() {
                    for s in samples {
                        if tx.send(s).is_err() {
                            return; // app side gone
                        }
                    }
                    ctx.request_repaint();
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        })
        .ok()?;
    Some(rx)
}

/// The actual event drain, on the tablet thread.
/// In→Down→Pose*→Up→Out per octotablet's stream; Down carries no pose, so the
/// last hover pose seeds the stroke position — and its tilt, so a stroke
/// starts at the pen's real approach angle, not the previous stroke's exit.
fn drain_tablet(
    mgr: &mut octotablet::Manager,
    ppp: f32,
    mut down: bool,
    mut last: Option<egui::Pos2>,
    mut last_tilt: Option<[f32; 2]>,
) -> (Vec<PenSample>, bool, Option<egui::Pos2>, Option<[f32; 2]>) {
    let mut out = Vec::new();
    // On Windows the pump error type is uninhabited (pump can't fail);
    // keep the guard for platforms where it can.
    #[allow(irrefutable_let_patterns)]
    let Ok(events) = mgr.pump() else {
        return (out, down, last, last_tilt);
    };
    for event in events {
        let octotablet::events::Event::Tool { event, .. } = event else {
            continue;
        };
        use octotablet::events::ToolEvent as TE;
        match event {
            TE::Pose(pose) => {
                let pos = egui::pos2(pose.position[0] / ppp, pose.position[1] / ppp);
                last = Some(pos);
                if pose.tilt.is_some() {
                    last_tilt = pose.tilt; // hover poses carry tilt too
                }
                if down {
                    out.push(PenSample {
                        pos,
                        pressure: pose.pressure.get(),
                        tilt: pose.tilt,
                        phase: PenPhase::Move,
                    });
                } else if pose.tilt.is_some() {
                    // Proximity pose: forward the live tilt so the cursor
                    // needle and T° readout move before the stroke starts.
                    // Tilt-less hovers are dropped (nothing to show; the OS
                    // mouse-move already drives the cursor position).
                    out.push(PenSample {
                        pos,
                        pressure: None,
                        tilt: pose.tilt,
                        phase: PenPhase::Hover,
                    });
                }
            }
            TE::Down => {
                down = true;
                if let Some(pos) = last {
                    out.push(PenSample {
                        pos,
                        pressure: None, // first Pose supplies real pressure
                        tilt: last_tilt, // hover already reported the angle
                        phase: PenPhase::Down,
                    });
                }
            }
            TE::Up | TE::Out | TE::Removed => {
                if down {
                    out.push(PenSample {
                        pos: last.unwrap_or(egui::pos2(0.0, 0.0)),
                        pressure: None,
                        tilt: None,
                        phase: PenPhase::Up,
                    });
                }
                down = false;
            }
            _ => {}
        }
    }
    (out, down, last, last_tilt)
}

struct Editor {
    state: AppState,
    canvas: canvas::CanvasView,
    /// GPU raster paint layer (None if wgpu is unavailable).
    paint: Option<PaintLayer>,
    /// GPU graph compositor — the composite-view render path (None like paint).
    graph: Option<graphcomp::GraphCompositor>,
    /// Set when the user clicks "New…"; the App picks it up to show the dialog.
    request_new: bool,
    /// Set when the user clicks "settings".
    request_settings: bool,
    /// The docking shell: every UI element is a movable pane over ONE document;
    /// a workspace is just a saved arrangement of these panes.
    dock: DockState<Pane>,
    /// Named, persisted workspaces (%APPDATA%/AnimStudio/workspaces.json).
    workspaces: Workspaces,
    /// Buffer for the "save workspace as" name field.
    ws_name: String,
    /// Buffer for the Presets pane's "save current as" name field.
    preset_name: String,
}

/// Renders each pane by borrowing the editor's parts (disjoint fields, so the
/// dock tree and the pane contents can be mutated in the same frame).
struct EditorTabs<'a> {
    state: &'a mut AppState,
    canvas: &'a mut canvas::CanvasView,
    paint: Option<&'a mut PaintLayer>,
    graph: Option<&'a mut graphcomp::GraphCompositor>,
    pen: &'a PenConfig,
    layers_cfg: &'a LayersConfig,
    presets: &'a mut Vec<BrushPreset>,
    presets_dirty: &'a mut bool,
    preset_name: &'a mut String,
    native_pen: &'a [PenSample],
}

impl egui_dock::TabViewer for EditorTabs<'_> {
    type Tab = Pane;

    fn title(&mut self, tab: &mut Pane) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Pane) {
        match tab {
            Pane::XSheet => xsheet_panel::ui(ui, self.state),
            Pane::Layers => xsheet_panel::cel_layers_strip(ui, self.state),
            Pane::Brush => {
                let raster_available = self.paint.is_some();
                self.canvas.brush_ui(ui, self.state, raster_available);
            }
            Pane::Presets => self.presets_ui(ui),
            Pane::NodeGraph => nodegraph_panel::ui(ui, self.state),
            Pane::Canvas => self.canvas.ui(
                ui,
                self.state,
                self.paint.as_deref_mut(),
                self.graph.as_deref_mut(),
                self.pen,
                self.layers_cfg,
                self.native_pen,
            ),
        }
    }

    // Everything except the canvas can be closed (re-add from the "panes"
    // menu); losing the canvas would strand the user.
    fn closeable(&mut self, tab: &mut Pane) -> bool {
        !matches!(tab, Pane::Canvas)
    }

    fn allowed_in_windows(&self, _tab: &mut Pane) -> bool {
        false
    }
}

impl EditorTabs<'_> {
    /// The Presets pane: click applies; save the current brush under a name.
    fn presets_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(self.preset_name)
                    .hint_text("name…")
                    .desired_width(110.0),
            );
            let name = self.preset_name.trim().to_string();
            if ui
                .button("save current")
                .on_hover_text("snapshot the current brush as a preset")
                .clicked()
                && !name.is_empty()
            {
                let snap = self.canvas.snapshot_preset(name.clone());
                if let Some(existing) = self.presets.iter_mut().find(|p| p.name == name) {
                    *existing = snap;
                } else {
                    self.presets.push(snap);
                }
                *self.presets_dirty = true;
                self.preset_name.clear();
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, p) in self.presets.iter().enumerate() {
                let hotkey = if i < 8 {
                    format!("{} ", i + 1)
                } else {
                    "· ".into()
                };
                let mut text = egui::RichText::new(format!(
                    "{hotkey}{}  {:.0}px",
                    p.name, p.size_px
                ));
                if let Some(c) = p.color {
                    text = text.color(egui::Color32::from_rgb(c[0], c[1], c[2]));
                }
                if ui.button(text).on_hover_text("apply preset").clicked() {
                    self.canvas.apply_preset(p);
                    self.state.status = format!("brush: {}", p.name);
                }
            }
            if self.presets.is_empty() {
                ui.label(egui::RichText::new("no presets — save one above").weak());
            }
        });
    }
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
        let graph = rs.map(|rs| graphcomp::GraphCompositor::new(rs, form.width, form.height));
        Self {
            state,
            canvas: canvas::CanvasView::new(),
            paint,
            graph,
            request_new: false,
            request_settings: false,
            dock: {
                let ws = Workspaces::load();
                ws.list.first().map(|w| w.dock.clone()).unwrap_or_else(draw_workspace)
            },
            workspaces: Workspaces::load(),
            ws_name: String::new(),
            preset_name: String::new(),
        }
    }

    fn from_state(state: AppState, rs: Option<&RenderState>) -> Self {
        let (w, h) = (state.engine.project.width, state.engine.project.height);
        let paint = rs.map(|rs| PaintLayer::new(rs, w, h));
        let graph = rs.map(|rs| graphcomp::GraphCompositor::new(rs, w, h));
        Self {
            state,
            canvas: canvas::CanvasView::new(),
            paint,
            graph,
            request_new: false,
            request_settings: false,
            dock: {
                let ws = Workspaces::load();
                ws.list.first().map(|w| w.dock.clone()).unwrap_or_else(draw_workspace)
            },
            workspaces: Workspaces::load(),
            ws_name: String::new(),
            preset_name: String::new(),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Native tablet backend: build on the FIRST frame — the message pump
        // is running now, so RealTimeStylus init can't deadlock on window
        // messages (the startup-hang failure mode). Opt-in via Settings → Pen;
        // ANIMSTUDIO_NO_TABLET=1 force-disables.
        if !self.tablet_boot_tried {
            self.tablet_boot_tried = true;
            let want = (self.config.pen.native_tablet
                || std::env::var_os("ANIMSTUDIO_TABLET").is_some())
                && std::env::var_os("ANIMSTUDIO_NO_TABLET").is_none();
            if want {
                self.tablet_rx = spawn_tablet_thread(frame, ui.ctx().clone());
            }
        }
        // Nothing open and no dialog -> show the startup dialog.
        if self.editor.is_none() && self.new_form.is_none() {
            self.new_form = Some(NewProjectForm::default());
        }

        // Apply live settings (read as locals to avoid borrowing self.config
        // while self.editor is borrowed mutably).
        let undo_limit = self.config.perf.undo_limit;
        let canvas_filter = self.config.perf.canvas_filter.wgpu();
        let pen = self.config.pen.clone();
        let layers_cfg = self.config.layers.clone();
        let mut presets = self.config.presets.clone();
        let mut presets_dirty = false;
        let native_pen = self.pump_tablet();

        // Editor (if any) renders first as the base layer.
        if let Some(editor) = &mut self.editor {
            editor.state.engine.set_undo_limit(undo_limit);
            if let Some(p) = &mut editor.paint {
                p.set_filter(canvas_filter);
            }
            if let Some(g) = &mut editor.graph {
                g.set_filter(canvas_filter);
            }
            editor.ui(ui, &pen, &layers_cfg, &mut presets, &mut presets_dirty, &native_pen);
            if presets_dirty {
                self.config.presets = presets.clone();
                self.config.save();
            }
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

/// Headless layout probe (ANIMSTUDIO_PROBE=1): drives Editor::ui through the
/// scenarios that historically nudged the canvas — play/stop, stepping between
/// a cel frame and an empty frame, undo/redo — and prints the canvas rect
/// after every step. Any `MOVED` line is a layout-stability bug.
fn probe() {
    // Two passes: 100% and 125% display scale (fractional scales can add
    // pixel-rounding jitter that 1.0 never shows).
    for ppp in [1.0f32, 1.25] {
        println!("---- pixels_per_point {ppp} ----");
        probe_at(ppp);
    }
}

fn probe_at(ppp: f32) {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(ppp);
    let config = Config::default();
    let pen = config.pen.clone();
    let layers_cfg = config.layers.clone();

    let mut state = AppState::new_project("probe", 1920, 1080, 24, 300.0);
    state.new_drawing_at_frame(); // cel at frame 0; frame 1 stays empty
    let mut editor = Editor::from_state(state, None);

    let raw = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };

    let mut last: Option<egui::Rect> = None;
    let mut moved = 0usize;
    let run = |editor: &mut Editor, label: &str, last: &mut Option<egui::Rect>, moved: &mut usize| {
        ctx.begin_pass(raw());
        let mut root = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("probe_root"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1280.0, 800.0),
            )),
        );
        let mut presets = config.presets.clone();
        let mut presets_dirty = false;
        editor.ui(&mut root, &pen, &layers_cfg, &mut presets, &mut presets_dirty, &[]);
        let _ = ctx.end_pass();
        let r = editor.canvas.dbg_rect;
        let note = match *last {
            Some(p)
                if (r.left() - p.left()).abs() > 0.01
                    || (r.top() - p.top()).abs() > 0.01
                    || (r.width() - p.width()).abs() > 0.01
                    || (r.height() - p.height()).abs() > 0.01 =>
            {
                // Settle steps exist to absorb first-frames layout (font
                // metrics, wrapped-row heights); only movement DURING the
                // scripted scenarios is the bug class.
                if label != "settle" {
                    *moved += 1;
                }
                format!(
                    "  MOVED dL{:+.2} dT{:+.2} dW{:+.2} dH{:+.2}",
                    r.left() - p.left(),
                    r.top() - p.top(),
                    r.width() - p.width(),
                    r.height() - p.height()
                )
            }
            _ => String::new(),
        };
        println!(
            "{label:26} L{:8.2} T{:7.2} W{:8.2} H{:7.2}{note}",
            r.left(),
            r.top(),
            r.width(),
            r.height()
        );
        *last = Some(r);
    };

    for _ in 0..4 {
        run(&mut editor, "settle", &mut last, &mut moved);
    }
    // Scenario 1: step cel-frame <-> empty-frame (strip rows <-> note).
    for i in 0..6 {
        let f = if i % 2 == 0 { 1 } else { 0 };
        editor.state.goto(f);
        run(&mut editor, if f == 0 { "goto cel frame" } else { "goto empty frame" }, &mut last, &mut moved);
    }
    // Scenario 2: play / stop.
    editor.state.toggle_play();
    for _ in 0..6 {
        run(&mut editor, "playing", &mut last, &mut moved);
    }
    editor.state.toggle_play();
    for _ in 0..2 {
        run(&mut editor, "stopped", &mut last, &mut moved);
    }
    // Scenario 3: undo / redo (removes and restores the cel + strip rows).
    for _ in 0..2 {
        editor.state.undo();
        run(&mut editor, "after undo", &mut last, &mut moved);
        editor.state.redo();
        run(&mut editor, "after redo", &mut last, &mut moved);
    }

    println!();
    if moved == 0 {
        println!("PROBE PASS: canvas rect never moved");
    } else {
        println!("PROBE FAIL: canvas rect moved {moved} time(s)");
    }
}

impl Editor {
    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        pen: &PenConfig,
        layers_cfg: &LayersConfig,
        presets: &mut Vec<BrushPreset>,
        presets_dirty: &mut bool,
        native_pen: &[PenSample],
    ) {
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
                    // Land any in-flight gesture in the CURRENT project first
                    // — its commit must not cross into the opened one.
                    self.canvas.finish_gesture(&mut self.state);
                    self.state.open();
                }
                if ui.button("⚙ settings").on_hover_text("keyboard shortcuts & config").clicked() {
                    self.request_settings = true;
                }
                if ui.button("save").clicked() {
                    // The saved file must contain what the screen shows.
                    self.canvas.finish_gesture(&mut self.state);
                    self.state.save(false);
                }
                ui.menu_button("export", |ui| {
                    if ui
                        .button("PNG sequence…")
                        .on_hover_text("one PNG per frame, transparent background")
                        .clicked()
                    {
                        ui.close();
                        let mut dlg = rfd::FileDialog::new();
                        if let Some(dir) = export::suggest_dir(&self.state) {
                            dlg = dlg.set_directory(dir);
                        }
                        if let Some(dir) = dlg.pick_folder() {
                            self.state.status = match export::export_png_sequence(&self.state, &dir)
                            {
                                Ok((n, note)) => {
                                    format!("exported {n} PNG frames to {}{note}", dir.display())
                                }
                                Err(e) => format!("PNG export failed: {e}"),
                            };
                        }
                    }
                    if ui
                        .button("MP4 video…")
                        .on_hover_text("white background — needs ffmpeg on PATH")
                        .clicked()
                    {
                        ui.close();
                        let mut dlg = rfd::FileDialog::new()
                            .add_filter("MP4 video", &["mp4"])
                            .set_file_name("animation.mp4");
                        if let Some(dir) = export::suggest_dir(&self.state) {
                            dlg = dlg.set_directory(dir);
                        }
                        if let Some(path) = dlg.save_file() {
                            self.state.status = match export::export_mp4(&self.state, &path) {
                                Ok((n, note)) => {
                                    format!("exported {n} frames to {}{note}", path.display())
                                }
                                Err(e) => format!("MP4 export failed: {e}"),
                            };
                        }
                    }
                });
                ui.separator();

                // Guarded like the keybinds: history must not move under a
                // live stroke or floating transform (the gesture's commit
                // would land on rewound state with stale before-values).
                let gesture = self.canvas.stroke_active();
                let can_undo = self.state.engine.can_undo() && !gesture;
                let can_redo = self.state.engine.can_redo() && !gesture;
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
                ui.checkbox(&mut self.state.loop_playback, "loop")
                    .on_hover_text("off = playback stops on the last frame");
                ui.separator();
                // Workspaces: saved pane arrangements over the same document.
                for i in 0..self.workspaces.list.len().min(6) {
                    let name = self.workspaces.list[i].name.clone();
                    if ui
                        .button(&name)
                        .on_hover_text("switch workspace (same document, new room)")
                        .clicked()
                    {
                        self.dock = self.workspaces.list[i].dock.clone();
                        // Workflow-stage brush: entering a bound workspace
                        // loads its preset automatically.
                        if let Some(pname) = self.workspaces.list[i].preset.clone()
                            && let Some(p) = presets.iter().find(|p| p.name == pname)
                        {
                            self.canvas.apply_preset(p);
                            self.state.status = format!("workspace {name} — brush: {pname}");
                        }
                    }
                }
                ui.menu_button("ws ▾", |ui| {
                    ui.label(egui::RichText::new("save current arrangement as:").weak());
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.ws_name);
                        let name = self.ws_name.trim().to_string();
                        if ui.button("save").clicked() && !name.is_empty() {
                            if let Some(w) =
                                self.workspaces.list.iter_mut().find(|w| w.name == name)
                            {
                                w.dock = self.dock.clone();
                            } else {
                                self.workspaces.list.push(Workspace {
                                    name,
                                    dock: self.dock.clone(),
                                    preset: None,
                                });
                            }
                            self.workspaces.save();
                            self.ws_name.clear();
                            ui.close();
                        }
                    });
                    ui.separator();
                    let mut remove: Option<usize> = None;
                    let mut assign: Option<(usize, Option<String>)> = None;
                    for (i, w) in self.workspaces.list.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(&w.name);
                            // Bound brush preset for this workspace (workflow
                            // stage keeps its own brush).
                            let bound = w.preset.clone().unwrap_or_else(|| "no brush".into());
                            ui.menu_button(format!("🖌 {bound}"), |ui| {
                                if ui.button("no brush").clicked() {
                                    assign = Some((i, None));
                                    ui.close();
                                }
                                for p in presets.iter() {
                                    if ui.button(&p.name).clicked() {
                                        assign = Some((i, Some(p.name.clone())));
                                        ui.close();
                                    }
                                }
                            });
                            if ui.small_button("✕").on_hover_text("delete workspace").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some((i, pname)) = assign {
                        self.workspaces.list[i].preset = pname;
                        self.workspaces.save();
                    }
                    if let Some(i) = remove {
                        self.workspaces.list.remove(i);
                        self.workspaces.save();
                    }
                });
                // Re-open closed panes (a pane lives only while it's in the
                // dock tree).
                ui.menu_button("panes ▾", |ui| {
                    for pane in Pane::ALL {
                        let present = self
                            .dock
                            .iter_all_tabs()
                            .any(|(_, t)| t == pane);
                        if ui
                            .add_enabled(!present, egui::Button::new(pane.title()))
                            .clicked()
                        {
                            self.dock.main_surface_mut().push_to_focused_leaf(*pane);
                            ui.close();
                        }
                    }
                });
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
                    // Pen diagnostics live HERE, fixed-width monospace — never
                    // in the canvas toolbar, where changing text reflowed the
                    // row and read as the workspace shifting.
                    let (diag, healthy, mouse_mode) = self.canvas.pressure_diag();
                    if mouse_mode {
                        ui.label(
                            egui::RichText::new("⚠ MOUSE — enable Windows Ink + Pen Mode")
                                .strong()
                                .color(egui::Color32::from_rgb(235, 90, 80)),
                        );
                    } else {
                        ui.label(egui::RichText::new(diag).monospace().size(11.0).color(
                            if healthy {
                                egui::Color32::from_rgb(120, 200, 140)
                            } else {
                                egui::Color32::from_gray(140)
                            },
                        ));
                    }
                });
            });
        });

        // The docking shell replaces the fixed left/central panels: panes are
        // draggable/stackable windows onto the one document; workspaces are
        // saved arrangements (top-bar buttons swap them).
        let mut tabs = EditorTabs {
            state: &mut self.state,
            canvas: &mut self.canvas,
            paint: self.paint.as_mut(),
            graph: self.graph.as_mut(),
            pen,
            layers_cfg,
            presets,
            presets_dirty,
            preset_name: &mut self.preset_name,
            native_pen,
        };
        let mut dock_style = egui_dock::Style::from_egui(ui.style().as_ref());
        // egui_dock clamps every divider so each side keeps `separator.extra`
        // pixels — the DEFAULT IS 175px, which silently blocked collapsing
        // panes (e.g. the Brush band above the canvas). 26px keeps the tab bar
        // grabbable while letting panes shrink to slim rails.
        dock_style.separator.extra = 26.0;
        DockArea::new(&mut self.dock)
            .style(dock_style)
            .show_inside(ui, &mut tabs);
    }
}
