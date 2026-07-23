//! AnimStudio — app shell (M2: X-sheet + drawing loop; + New Project flow).
//!
//! Two screens: a startup New Project dialog (sets resolution/fps), then the
//! editor (top bar, left X-sheet, central canvas, status bar).

mod canvas;
mod config;
mod doc;
mod export;
mod floatcanvas;
mod floatwin;
mod graphcomp;
mod kpp;
mod newproject;
mod nodegraph_panel;
mod paint;
mod palette;
mod palette_panel;
mod viewer;
mod workspace;
mod xsheet_panel;

use config::{Action, BrushPreset, Config, FrameLatency, LayersConfig, PenConfig, RebindCapture, SettingsCategory};
use doc::AppState;
use egui_dock::{DockArea, DockState};
use workspace::{Pane, Stage, Workspace, Workspaces};
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

/// Scratch-track playback (C4): a rodio output stream + sink, created
/// lazily on the first play with a clip present. The engine owns the
/// decoded samples; this only streams them from the playhead. A missing
/// audio device degrades to silent playback (the app never blocks on it).
struct AudioOut {
    /// Keeps the OS audio stream alive for the sink's lifetime.
    _stream: rodio::OutputStream,
    sink: rodio::Sink,
}

/// Zero-copy rodio source over the engine's Arc'd decoded samples — a
/// restart must never clone a multi-hundred-MB sample buffer.
struct ClipSource {
    data: std::sync::Arc<Vec<f32>>,
    pos: usize,
    channels: u16,
    rate: u32,
}

impl Iterator for ClipSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let v = self.data.get(self.pos).copied();
        self.pos += 1;
        v
    }
}

impl rodio::Source for ClipSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// State for a running background export (C3) — polled once per frame
/// (`Editor::poll_export`), same drain pattern as `App::pump_tablet`. Lives
/// on `Editor` (not `App`): it's scoped to the open project, same as `dock`
/// or `workspaces`.
struct ExportJob {
    rx: std::sync::mpsc::Receiver<export::ExportProgress>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// "PNG sequence" / "MP4" — for the progress window title and the
    /// completion status message.
    kind: &'static str,
    total: usize,
    done: usize,
    /// MP4 only: frame rendering finished, now inside the (unprogressable)
    /// ffmpeg subprocess.
    encoding: bool,
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
        if action == Action::ToggleAlphaLock {
            ed.canvas.toggle_alpha_lock();
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
            Action::ToggleOnion => s.view.onion = !s.view.onion,
            Action::ToggleEraser
            | Action::ToggleAlphaLock
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
    /// Which spine stage the current layout came from (highlight only) —
    /// None after applying a custom saved workspace. Deliberately NOT
    /// cleared when panes are rearranged: the highlight means "this is the
    /// room I'm working from", not "the layout is pixel-identical".
    stage: Option<Stage>,
    /// Named, persisted workspaces (%APPDATA%/AnimStudio/workspaces.json).
    workspaces: Workspaces,
    /// Buffer for the "save workspace as" name field.
    ws_name: String,
    /// Buffer for the cut ▾ menu's rename field.
    rename_buf: String,
    /// Buffer for the Presets pane's "save current as" name field.
    preset_name: String,
    /// Per-instance viewer-pane state (zoom/pan), keyed by the pane's viewer
    /// id — the first multi-instance pane state (session-only).
    viewers: std::collections::HashMap<u8, viewer::ViewerView>,
    /// In-flight background export (C3) — None = no export running. Export
    /// menu buttons are disabled while this is Some, so only one job runs
    /// at a time (its own worker thread, same shape as the tablet backend).
    export_job: Option<ExportJob>,
    /// Export frame-range buffer (0-based, inclusive), shown in the export
    /// menu — None = "whole cut" (recomputed fresh from the current cut's
    /// length every time the menu renders, so it's never stale).
    export_range: Option<(u32, u32)>,
    /// Phase 5 step 1: the floating OS viewer window (deferred viewport).
    float_viewer: floatwin::FloatViewer,
    /// Phase 5 step 2: the editable canvas as its own OS window (immediate
    /// viewport — see floatcanvas.rs for why immediate, not deferred).
    float_canvas: floatcanvas::FloatCanvas,
    /// Scratch-audio output (C4) — None until first play (or unavailable).
    audio_out: Option<AudioOut>,
    /// Device open was attempted (never retry a missing device every frame).
    audio_tried: bool,
    /// A clip is currently sounding through the sink.
    audio_playing: bool,
    /// Last frame seen by sync_audio — detects loop wraps and mid-play seeks.
    audio_prev_frame: u32,
    /// WHAT is sounding: (scene, cut, clip identity via the bytes Arc's
    /// address). A cut switch or an undo/redo that swaps the clip mid-play
    /// changes this key → the sound restarts on the right clip instead of
    /// the stale one playing on.
    audio_key: Option<(u64, u64, usize)>,
}

/// (scene, name, [(cut, name)]) — the "cut ▾" menu's owned snapshot of the
/// project's scene/cut tree, cloned once per open so switching mid-menu
/// never fights a live borrow of `self.state`.
type SceneCutTree = Vec<(anim_core::ids::SceneId, String, Vec<(anim_core::ids::CutId, String)>)>;

/// Land any floating transform and report whether it's now safe to switch
/// editing context — false = a live PEN stroke is still in progress and the
/// caller must refuse. The general law behind every context transition that
/// could otherwise orphan a gesture: cut/scene switching, workspace/stage
/// switching, Save/Open, and docking the canvas back from its floating OS
/// window (Phase 5 step 2) — nothing else ever resets `touch_active`, so an
/// un-landed transition there would permanently lock out painting.
fn guard_gesture(canvas: &mut canvas::CanvasView, state: &mut AppState) -> bool {
    canvas.finish_gesture(state);
    if canvas.stroke_active() {
        state.status = "refused — finish the stroke first".into();
        false
    } else {
        true
    }
}

/// The ACTIVE tab of every dock leaf — the panes actually rendered this
/// frame. Tabs hidden behind another in a stack don't render, so they must
/// not consume per-frame work (graph executions).
fn visible_panes(dock: &DockState<Pane>) -> Vec<Pane> {
    let mut out = Vec::new();
    for node in dock.main_surface().iter() {
        if let egui_dock::Node::Leaf(leaf) = node
            && let Some(t) = leaf.tabs.get(leaf.active.0)
        {
            out.push(*t);
        }
    }
    out
}

/// Renders each pane by borrowing the editor's parts (disjoint fields, so the
/// dock tree and the pane contents can be mutated in the same frame).
struct EditorTabs<'a> {
    state: &'a mut AppState,
    canvas: &'a mut canvas::CanvasView,
    paint: Option<&'a mut PaintLayer>,
    /// The node graph's per-frame render status, executed ONCE by Editor::ui
    /// for every VISIBLE consumer (canvas composite view + viewer panes).
    graph: viewer::GraphView,
    viewers: &'a mut std::collections::HashMap<u8, viewer::ViewerView>,
    pen: &'a PenConfig,
    layers_cfg: &'a LayersConfig,
    presets: &'a mut Vec<BrushPreset>,
    presets_dirty: &'a mut bool,
    preset_name: &'a mut String,
    native_pen: &'a [PenSample],
    /// The canvas is currently rendered in its own OS window this frame —
    /// the dock's Canvas tab shows a placeholder instead of double-driving
    /// the one CanvasView from two places.
    float_canvas_open: bool,
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
            Pane::Palette => palette_panel::ui(ui, self.state, self.canvas),
            Pane::NodeGraph => nodegraph_panel::ui(ui, self.state),
            Pane::Canvas if self.float_canvas_open => {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(
                            "canvas is open in its own window — panes ▾ to bring it back",
                        )
                        .weak(),
                    );
                });
            }
            Pane::Canvas => self.canvas.ui(
                ui,
                self.state,
                self.paint.as_deref_mut(),
                self.graph,
                self.pen,
                self.layers_cfg,
                self.native_pen,
            ),
            Pane::Viewer(id) => {
                let vs = self.viewers.entry(*id).or_default();
                viewer::ui(ui, self.state, self.graph, vs);
            }
        }
    }

    // Everything except the canvas can be closed (re-add from the "panes"
    // menu); losing the canvas would strand the user.
    fn closeable(&mut self, tab: &mut Pane) -> bool {
        !matches!(tab, Pane::Canvas)
    }

    // Closing a viewer drops its zoom/pan — re-adding its id later must give
    // a FRESH viewer, not a ghost of the old view state.
    fn on_close(&mut self, tab: &mut Pane) -> egui_dock::widgets::tab_viewer::OnCloseResponse {
        if let Pane::Viewer(id) = tab {
            self.viewers.remove(id);
        }
        egui_dock::widgets::tab_viewer::OnCloseResponse::Close
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
            // Every project opens in the Drawing room — the daily driver
            // and the spine's canonical default.
            dock: Stage::Drawing.dock(),
            stage: Some(Stage::Drawing),
            workspaces: Workspaces::load(),
            ws_name: String::new(),
            rename_buf: String::new(),
            preset_name: String::new(),
            viewers: Default::default(),
            export_job: None,
            export_range: None,
            float_viewer: floatwin::FloatViewer::new(),
            float_canvas: floatcanvas::FloatCanvas::default(),
            audio_out: None,
            audio_tried: false,
            audio_playing: false,
            audio_prev_frame: 0,
            audio_key: None,
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
            dock: Stage::Drawing.dock(),
            stage: Some(Stage::Drawing),
            workspaces: Workspaces::load(),
            ws_name: String::new(),
            rename_buf: String::new(),
            preset_name: String::new(),
            viewers: Default::default(),
            export_job: None,
            export_range: None,
            float_viewer: floatwin::FloatViewer::new(),
            float_canvas: floatcanvas::FloatCanvas::default(),
            audio_out: None,
            audio_tried: false,
            audio_playing: false,
            audio_prev_frame: 0,
            audio_key: None,
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
            let collect = |i: &egui::InputState| -> Vec<Action> {
                Action::ALL
                    .iter()
                    .copied()
                    .filter(|a| self.config.triggered(*a, i))
                    .collect()
            };
            let mut fired: Vec<Action> = ui.ctx().input(collect);
            // Keyboard events land in whichever VIEWPORT has focus. The
            // popout canvas (Phase 5 step 2) is its own OS window, so a
            // shortcut pressed there arrives in ITS input state, not the
            // root's — check it too. A physical keypress reaches exactly one
            // viewport's input, so a deduped OR can never double-fire (undo
            // twice from one Ctrl+Z).
            let float_open = self.editor.as_ref().is_some_and(|e| e.float_canvas.open);
            if float_open {
                let more =
                    ui.ctx().input_for(floatcanvas::FloatCanvas::viewport_id(), collect);
                for a in more {
                    if !fired.contains(&a) {
                        fired.push(a);
                    }
                }
            }
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
            .filter(|e| e.state.view.playing)
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
    /// Keep scratch-audio playback in step with the playhead: start when
    /// playback starts, stop when it stops (or the clip is removed/undone),
    /// restart on a loop wrap, a mid-play seek, or when WHAT should be
    /// sounding changes (cut switch, undo/redo swapping the clip). Called
    /// once per UI frame, right after `state.tick` advances the playhead;
    /// `dt` is that tick's clamped delta — the seek threshold must scale
    /// with it (at 48+ project fps a single slow UI frame legitimately
    /// advances the playhead by 5+, and a fixed threshold caused a restart
    /// stutter loop).
    fn sync_audio(&mut self, dt: f32) {
        let playing = self.state.view.playing;
        let frame = self.state.view.frame;
        let key = self.state.cut().audio.as_ref().map(|c| {
            (
                self.state.view.scene.0,
                self.state.view.cut.0,
                std::sync::Arc::as_ptr(&c.bytes) as *const u8 as usize,
            )
        });
        // Max frames one tick can legitimately advance, with 3x headroom.
        let max_step = ((self.state.fps().max(1) as f32 * dt).ceil() as u32)
            .saturating_mul(3)
            .max(4);
        let jumped = self.audio_playing
            && (frame < self.audio_prev_frame
                || frame > self.audio_prev_frame.saturating_add(max_step));
        let clip_changed = self.audio_playing && self.audio_key != key;
        if playing && key.is_some() && (!self.audio_playing || jumped || clip_changed) {
            self.start_audio(frame);
            self.audio_key = key;
        } else if self.audio_playing && (!playing || key.is_none()) {
            if let Some(out) = &self.audio_out {
                out.sink.stop();
            }
            self.audio_playing = false;
            self.audio_key = None;
        }
        self.audio_prev_frame = frame;
    }

    fn start_audio(&mut self, frame: u32) {
        if self.audio_out.is_none() && !self.audio_tried {
            self.audio_tried = true;
            match rodio::OutputStreamBuilder::open_default_stream() {
                Ok(stream) => {
                    let sink = rodio::Sink::connect_new(stream.mixer());
                    self.audio_out = Some(AudioOut { _stream: stream, sink });
                }
                Err(e) => {
                    // Once, not per frame; playback stays silent but works.
                    eprintln!("audio device unavailable: {e}");
                    self.state.status = "no audio device — playing silent".into();
                }
            }
        }
        let Some(out) = &self.audio_out else { return };
        let Some(clip) = &self.state.cut().audio else { return };
        let fps = self.state.fps().max(1) as u64;
        // Whole sample-frames, THEN interleave — start is always aligned to
        // a channel-0 sample.
        let start = ((frame as u64 * clip.sample_rate as u64) / fps) * clip.channels as u64;
        let start = (start as usize).min(clip.samples.len());
        out.sink.stop();
        out.sink.append(ClipSource {
            data: clip.samples.clone(),
            pos: start,
            channels: clip.channels,
            rate: clip.sample_rate,
        });
        out.sink.play();
        self.audio_playing = true;
    }

    /// Drain this frame's export-progress messages. A disconnected channel
    /// (the worker thread panicked) is treated as a silent failure — the
    /// job just disappears rather than spinning forever; a real crash there
    /// would already have printed to stderr.
    fn poll_export(&mut self) {
        let Some(job) = &mut self.export_job else { return };
        let mut finished: Option<String> = None;
        loop {
            match job.rx.try_recv() {
                Ok(export::ExportProgress::Frame) => job.done += 1,
                Ok(export::ExportProgress::Encoding) => job.encoding = true,
                Ok(export::ExportProgress::Done(result)) => {
                    finished = Some(match result {
                        Ok((n, note)) => format!("exported {n} frame(s) ({}){note}", job.kind),
                        Err(e) if e == export::CANCELLED => "export cancelled".into(),
                        Err(e) => format!("{} export failed: {e}", job.kind),
                    });
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finished = Some(format!("{} export ended unexpectedly", job.kind));
                    break;
                }
            }
        }
        if let Some(status) = finished {
            self.export_job = None;
            self.state.status = status;
        }
    }

    /// The export-progress window (Krita-style small modal, matching
    /// `config::settings_window`'s pattern) — a Cancel button and either a
    /// determinate bar (frame rendering) or an indeterminate one (MP4's
    /// ffmpeg subprocess, which has no cheap progress signal).
    fn export_progress_window(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.export_job else { return };
        let mut open = true;
        egui::Window::new(format!("Exporting — {}", job.kind))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.set_min_width(260.0);
                if job.encoding {
                    ui.add(egui::ProgressBar::new(1.0).text("encoding video…").animate(true));
                } else {
                    let frac = if job.total > 0 {
                        job.done as f32 / job.total as f32
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .text(format!("{} / {} frames", job.done, job.total)),
                    );
                }
                if ui.button("cancel").clicked() {
                    job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            });
        // The window's own ✕ also cancels — closing it must not orphan the
        // worker thread silently rendering in the background.
        if !open && let Some(job) = &self.export_job {
            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn ui(
        &mut self,
        ui: &mut egui::Ui,
        pen: &PenConfig,
        layers_cfg: &LayersConfig,
        presets: &mut Vec<BrushPreset>,
        presets_dirty: &mut bool,
        native_pen: &[PenSample],
    ) {
        self.poll_export();
        self.export_progress_window(ui.ctx());
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        self.state.tick(dt);
        self.sync_audio(dt);

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
                    // Frame range (C3): 1-based in the UI (matches the
                    // "N / total" frame readout elsewhere), 0-based
                    // internally. None = whole cut — recomputed fresh every
                    // time the menu opens so it can never point past a cut
                    // that's since gotten shorter.
                    let n = self.state.frame_count();
                    let (mut a, mut b) = self.export_range.unwrap_or((0, n.saturating_sub(1)));
                    a = a.min(n.saturating_sub(1));
                    b = b.clamp(a, n.saturating_sub(1));
                    ui.horizontal(|ui| {
                        ui.label("frames");
                        let mut a1 = a + 1;
                        let mut b1 = b + 1;
                        if ui.add(egui::DragValue::new(&mut a1).range(1..=b1)).changed() {
                            a = a1 - 1;
                        }
                        ui.label("–");
                        if ui.add(egui::DragValue::new(&mut b1).range(a1..=n)).changed() {
                            b = b1 - 1;
                        }
                    });
                    self.export_range = Some((a, b));
                    if ui
                        .small_button("whole cut")
                        .on_hover_text("reset the range to frame 1 – last")
                        .clicked()
                    {
                        self.export_range = None;
                    }
                    ui.separator();

                    // Both spawns are near-identical: clone the cut (an
                    // owned, immutable snapshot the worker thread renders
                    // from — never a live reference, see export.rs), hand it
                    // to a background thread, and track the job for the
                    // progress window. Buttons disable while one is running
                    // (one export at a time).
                    let busy = self.export_job.is_some();
                    if ui
                        .add_enabled(!busy, egui::Button::new("PNG sequence…"))
                        .on_hover_text("one PNG per frame, transparent background")
                        .clicked()
                    {
                        ui.close();
                        let mut dlg = rfd::FileDialog::new();
                        if let Some(dir) = export::suggest_dir(&self.state) {
                            dlg = dlg.set_directory(dir);
                        }
                        if let Some(dir) = dlg.pick_folder() {
                            let cut = self.state.cut().clone();
                            let (w, h) = (
                                self.state.engine.project.width,
                                self.state.engine.project.height,
                            );
                            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                            let rx = export::spawn_png_sequence(
                                cut,
                                w,
                                h,
                                dir,
                                a..=b,
                                ui.ctx().clone(),
                                cancel.clone(),
                            );
                            self.export_job = Some(ExportJob {
                                rx,
                                cancel,
                                kind: "PNG sequence",
                                total: (b - a + 1) as usize,
                                done: 0,
                                encoding: false,
                            });
                            self.state.status = "exporting…".into();
                        }
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("MP4 video…"))
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
                            let cut = self.state.cut().clone();
                            let (w, h) = (
                                self.state.engine.project.width,
                                self.state.engine.project.height,
                            );
                            let fps = self.state.fps();
                            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                            let rx = export::spawn_mp4(
                                cut,
                                w,
                                h,
                                fps,
                                path,
                                a..=b,
                                ui.ctx().clone(),
                                cancel.clone(),
                            );
                            self.export_job = Some(ExportJob {
                                rx,
                                cancel,
                                kind: "MP4",
                                total: (b - a + 1) as usize,
                                done: 0,
                                encoding: false,
                            });
                            self.state.status = "exporting…".into();
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
                let play_text = if self.state.view.playing { "stop" } else { "play" };
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
                        self.state.view.frame + 1,
                        self.state.frame_count()
                    ))
                    .monospace(),
                );
                ui.separator();
                ui.checkbox(&mut self.state.view.onion, "onion");
                // Fixed-width regardless of onion state (UI-STABILITY law:
                // nothing variable-width in the toolbar) — disabled, not
                // hidden, when onion is off.
                ui.add_enabled(
                    self.state.view.onion,
                    egui::Checkbox::new(&mut self.state.view.onion_layer_only, "line only"),
                )
                .on_hover_text(
                    "ghost only the layer matching your active layer's name \
                     (e.g. line-on-line) instead of the whole neighbour cel",
                );
                ui.checkbox(&mut self.state.view.loop_playback, "loop")
                    .on_hover_text("off = playback stops on the last frame");
                ui.separator();
                // THE STAGE SPINE (workflow trees, LENS-DOCK Phase 3): the
                // four fixed pipeline rooms, in order. Switching re-lenses
                // the SAME document — layout + tool/view state swap, data
                // never forks. Tool/view restore is all-or-nothing; a live
                // stroke keeps the old tool state (same law as saved
                // workspaces below).
                for st in Stage::ALL {
                    if ui
                        .selectable_label(self.stage == Some(*st), st.name())
                        .on_hover_text(st.describes())
                        .clicked()
                    {
                        self.dock = st.dock();
                        if self.canvas.apply_view(&st.view(), &mut self.state) {
                            self.stage = Some(*st);
                            self.state.status = format!("room: {}", st.name());
                        }
                    }
                }
                ui.menu_button("ws ▾", |ui| {
                    ui.label(egui::RichText::new("save current arrangement as:").weak());
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.ws_name);
                        let name = self.ws_name.trim().to_string();
                        if ui.button("save").clicked() && !name.is_empty() {
                            // Saving a room captures the CURRENT tool/view
                            // state along with the layout.
                            let view = Some(self.canvas.snapshot_view(&self.state));
                            if let Some(w) =
                                self.workspaces.list.iter_mut().find(|w| w.name == name)
                            {
                                w.dock = self.dock.clone();
                                w.view = view;
                            } else {
                                self.workspaces.list.push(Workspace {
                                    name,
                                    dock: self.dock.clone(),
                                    preset: None,
                                    view,
                                });
                            }
                            self.workspaces.save();
                            self.ws_name.clear();
                            ui.close();
                        }
                    });
                    ui.separator();
                    let mut apply: Option<usize> = None;
                    let mut remove: Option<usize> = None;
                    let mut assign: Option<(usize, Option<String>)> = None;
                    for (i, w) in self.workspaces.list.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui
                                .button(&w.name)
                                .on_hover_text("apply this saved arrangement")
                                .clicked()
                            {
                                apply = Some(i);
                                ui.close();
                            }
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
                    if let Some(i) = apply {
                        let name = self.workspaces.list[i].name.clone();
                        self.dock = self.workspaces.list[i].dock.clone();
                        // A custom room is off-spine — no stage highlighted.
                        self.stage = None;
                        // Same all-or-nothing tool/view + brush gate as the
                        // stage buttons.
                        let view_ok = match self.workspaces.list[i].view {
                            Some(v) => self.canvas.apply_view(&v, &mut self.state),
                            None => !self.canvas.stroke_active(),
                        };
                        self.state.status = format!("workspace: {name}");
                        if view_ok
                            && let Some(pname) = self.workspaces.list[i].preset.clone()
                            && let Some(p) = presets.iter().find(|p| p.name == pname)
                        {
                            self.canvas.apply_preset(p);
                            self.state.status = format!("workspace {name} — brush: {pname}");
                        }
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
                    // Viewers are multi-instance: add up to MAX_VIEWERS.
                    let next_id = (0..Pane::MAX_VIEWERS).find(|id| {
                        !self
                            .dock
                            .iter_all_tabs()
                            .any(|(_, t)| *t == Pane::Viewer(*id))
                    });
                    if ui
                        .add_enabled(next_id.is_some(), egui::Button::new("+ Viewer"))
                        .on_hover_text(
                            "a read-only composite viewer (the node graph's render) — \
                             dock one beside the canvas to paint and watch the result live",
                        )
                        .clicked()
                        && let Some(id) = next_id
                    {
                        self.dock
                            .main_surface_mut()
                            .push_to_focused_leaf(Pane::Viewer(id));
                        ui.close();
                    }
                    ui.separator();
                    // Phase 5 step 1: the viewer as a REAL OS window — drag
                    // it onto the second monitor / pen display.
                    let mut floating = self.float_viewer.is_open();
                    if ui
                        .checkbox(&mut floating, "viewer in an OS window")
                        .on_hover_text(
                            "open the composite viewer as a separate real window \
                             (drag it to another monitor)",
                        )
                        .changed()
                    {
                        self.float_viewer.set_open(floating);
                        ui.close();
                    }
                    // Phase 5 step 2: the EDITABLE canvas as a real OS window
                    // — draw directly on the pen display. Docking it back
                    // goes through the SAME guard as Save/Open/workspace
                    // switching: a live pen stroke refuses the transition
                    // rather than being silently orphaned (mid-stroke this
                    // would otherwise strand touch_active forever — nothing
                    // else ever resets it).
                    let mut floating = self.float_canvas.open;
                    if ui
                        .checkbox(&mut floating, "canvas in an OS window")
                        .on_hover_text(
                            "open the drawing canvas as a separate real window \
                             (drag it onto the pen display) — every tool works \
                             there exactly as it does docked",
                        )
                        .changed()
                    {
                        if floating {
                            self.float_canvas.open = true;
                            ui.close();
                        } else if guard_gesture(&mut self.canvas, &mut self.state) {
                            self.float_canvas.open = false;
                            ui.close();
                        }
                        // Refused: leave float_canvas.open at its prior value
                        // (still true) — the checkbox reflects it again next
                        // frame; the status line already explains why.
                    }
                });
                // Scene/cut navigation (C2): the model always supported
                // more than one, there was just no way to reach them.
                ui.menu_button("cut ▾", |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "now editing: {}",
                            self.state.cut().name
                        ))
                        .weak(),
                    );
                    // Rename the current cut/scene (scaffolding, like
                    // creation — not undoable). Empty-buffer pattern: type
                    // a new name and confirm; no pre-seeding to fight.
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.rename_buf)
                                .hint_text("new name…")
                                .desired_width(110.0),
                        );
                        let name = self.rename_buf.trim().to_string();
                        if ui
                            .add_enabled(!name.is_empty(), egui::Button::new("rename cut"))
                            .clicked()
                        {
                            self.state.rename_current_cut(&name);
                            self.rename_buf.clear();
                        }
                        if ui
                            .add_enabled(!name.is_empty(), egui::Button::new("rename scene"))
                            .clicked()
                        {
                            self.state.rename_current_scene(&name);
                            self.rename_buf.clear();
                        }
                    });
                    // XDTS interop: the industry exposure-sheet exchange
                    // format (Toei/OpenToonz/CSP). Export = this cut's
                    // timing; import = a NEW cut built from the file.
                    ui.horizontal(|ui| {
                        if ui
                            .button("⇥ XDTS…")
                            .on_hover_text("export this cut's timing as an exposure sheet (.xdts)")
                            .clicked()
                        {
                            ui.close();
                            let mut dlg = rfd::FileDialog::new()
                                .add_filter("exposure sheet", &["xdts"])
                                .set_file_name(format!("{}.xdts", self.state.cut().name));
                            if let Some(dir) = export::suggest_dir(&self.state) {
                                dlg = dlg.set_directory(dir);
                            }
                            if let Some(path) = dlg.save_file() {
                                self.state.export_xdts(&path);
                            }
                        }
                        if ui
                            .button("⇤ XDTS…")
                            .on_hover_text("import an exposure sheet (.xdts) as a NEW cut")
                            .clicked()
                        {
                            ui.close();
                            if guard_gesture(&mut self.canvas, &mut self.state)
                                && let Some(path) = rfd::FileDialog::new()
                                    .add_filter("exposure sheet", &["xdts"])
                                    .pick_file()
                            {
                                self.state.import_xdts(&path);
                            }
                        }
                    });
                    ui.separator();
                    let scenes: SceneCutTree = self
                        .state
                        .engine
                        .project
                        .scenes
                        .iter()
                        .map(|s| {
                            (
                                s.id,
                                s.name.clone(),
                                s.cuts.iter().map(|c| (c.id, c.name.clone())).collect(),
                            )
                        })
                        .collect();
                    for (scene_id, scene_name, cuts) in &scenes {
                        ui.label(egui::RichText::new(scene_name).strong());
                        for (cut_id, cut_name) in cuts {
                            let current = *scene_id == self.state.view.scene
                                && *cut_id == self.state.view.cut;
                            if ui
                                .selectable_label(current, format!("    {cut_name}"))
                                .clicked()
                                && !current
                                && guard_gesture(&mut self.canvas, &mut self.state)
                            {
                                self.state.goto_cut(*scene_id, *cut_id);
                                ui.close();
                            }
                        }
                        if ui
                            .small_button("+ cut")
                            .on_hover_text("add a cut to this scene")
                            .clicked()
                            && guard_gesture(&mut self.canvas, &mut self.state)
                        {
                            self.state.new_cut(*scene_id);
                            ui.close();
                        }
                        ui.add_space(4.0);
                    }
                    ui.separator();
                    if ui
                        .button("+ scene")
                        .on_hover_text("a new scene with its first cut")
                        .clicked()
                        && guard_gesture(&mut self.canvas, &mut self.state)
                    {
                        self.state.new_scene();
                        ui.close();
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
        // Execute the node graph ONCE per frame, only when a VISIBLE consumer
        // needs it (the active tab of a leaf — hidden tabs behind a stack
        // don't render, so they don't pay for executions). The evaluator hash
        // is the dirty key, so an unchanged frame costs nothing. Running
        // BEFORE the dock renders means pane-driven edits (node-graph pane)
        // reach viewers one frame later — accepted: egui repaints
        // continuously during any interaction.
        let visible = visible_panes(&self.dock);
        let viewers_open = visible.iter().any(|t| matches!(t, Pane::Viewer(_)));
        // The dock's Canvas tab renders a placeholder (not canvas.ui) whenever
        // the canvas is floated — its dock-visibility no longer reflects
        // whether canvas.ui will actually run this frame, so OR in the float
        // explicitly (same reasoning as float_open below for the viewer).
        let composite_needed = self.canvas.composite_view
            && (visible.contains(&Pane::Canvas) || self.float_canvas.open);
        // The floating OS viewer is a consumer too (it may be the ONLY one).
        let float_open = self.float_viewer.is_open();
        let mut graph = viewer::GraphView::Off;
        if viewers_open || composite_needed || float_open {
            graph = if self.state.cut().graph.output.is_none() {
                viewer::GraphView::NoOutput
            } else if let (Some(g), Some(p)) = (&mut self.graph, &mut self.paint) {
                let (w, h) = (
                    self.state.engine.project.width,
                    self.state.engine.project.height,
                );
                p.ensure_size(w, h);
                g.ensure_size(w, h);
                let (scene, cutid, frame) =
                    (self.state.view.scene, self.state.view.cut, self.state.view.frame);
                match self.state.engine.eval(scene, cutid, frame) {
                    Ok(v) => {
                        let hash = v.hash();
                        let cut = self.state.cut();
                        viewer::GraphView::Ready(g.execute(hash, cut, frame, p))
                    }
                    Err(_) => viewer::GraphView::EvalFailed,
                }
            } else {
                viewer::GraphView::NoGpu
            };
        }

        // Feed + drive the floating OS viewer window. The texture id stays
        // the same across frames (the compositor reuses its out texture),
        // so the float must be repainted whenever content may have moved —
        // the main context repaints on any activity, and we forward that.
        if float_open {
            {
                let mut sh = self.float_viewer.shared.write();
                sh.paper = egui::vec2(
                    self.state.engine.project.width as f32,
                    self.state.engine.project.height as f32,
                );
                match graph {
                    viewer::GraphView::Ready(id) => sh.tex = Some(id),
                    viewer::GraphView::NoOutput | viewer::GraphView::Off => {
                        sh.tex = None;
                        sh.hint =
                            "no graph output — wire an Output node in the Node Graph pane".into();
                    }
                    viewer::GraphView::EvalFailed => {
                        sh.tex = None;
                        sh.hint = "the graph failed to evaluate — check the Node Graph pane".into();
                    }
                    viewer::GraphView::NoGpu => {
                        sh.tex = None;
                        sh.hint = "GPU unavailable — the viewer needs wgpu".into();
                    }
                }
            }
            self.float_viewer.show(ui.ctx());
            ui.ctx()
                .request_repaint_of(floatwin::FloatViewer::viewport_id());
        }

        let mut tabs = EditorTabs {
            state: &mut self.state,
            canvas: &mut self.canvas,
            paint: self.paint.as_mut(),
            graph,
            viewers: &mut self.viewers,
            pen,
            layers_cfg,
            presets,
            presets_dirty,
            preset_name: &mut self.preset_name,
            native_pen,
            float_canvas_open: self.float_canvas.open,
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

        // Phase 5 step 2: the canvas in its own OS window. Tabs' borrows of
        // state/canvas/paint end above, so this can borrow them fresh.
        // Immediate viewport: runs INLINE, right here, so it can capture
        // these by plain mutable reference — see floatcanvas.rs for why
        // that's the deliberate trade-off.
        if self.float_canvas.open {
            let open = ui.ctx().show_viewport_immediate(
                floatcanvas::FloatCanvas::viewport_id(),
                egui::ViewportBuilder::default()
                    .with_title(floatcanvas::FloatCanvas::TITLE)
                    .with_inner_size([960.0, 720.0]),
                |ui, class| {
                    if class == egui::ViewportClass::EmbeddedWindow {
                        egui::CentralPanel::default().show(ui, |ui| {
                            ui.label("this platform can't open a real OS window here");
                        });
                        return true;
                    }
                    egui::CentralPanel::default().show(ui, |ui| {
                        self.canvas.ui(
                            ui,
                            &mut self.state,
                            self.paint.as_mut(),
                            graph,
                            pen,
                            layers_cfg,
                            native_pen,
                        );
                    });
                    // The OS ✕ is a REQUEST, not a command — same law as
                    // Save/Open/workspace switching (guard_gesture): a live
                    // pen stroke refuses the transition instead of being
                    // silently orphaned (nothing else ever resets
                    // touch_active, so an un-landed close here would
                    // permanently lock out painting). Refusing just means
                    // NOT tearing the viewport down; the window stays open
                    // and the status line explains why.
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        !guard_gesture(&mut self.canvas, &mut self.state)
                    } else {
                        true
                    }
                },
            );
            if !open {
                self.float_canvas.open = false;
            }
        }
    }
}
