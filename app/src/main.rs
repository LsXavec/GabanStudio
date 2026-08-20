//! AnimStudio — app shell (M2: X-sheet + drawing loop; + New Project flow).
//!
//! Two screens: a startup New Project dialog (sets resolution/fps), then the
//! editor (top bar, left X-sheet, central canvas, status bar).

mod canvas;
mod config;
mod devloop;
mod doc;
mod export;
mod floatcanvas;
mod forge;
mod floatwin;
mod graphcomp;
mod icons;
mod kpp;
mod brushbank;
mod kritares;
mod net;
mod newproject;
mod nodegraph_panel;
mod paint;
mod palette;
mod palette_panel;
mod plate;
mod runs;
mod uidump;
mod update;
mod viewer;
mod workspace;
mod xsheet_panel;

use canvas::{PenPhase, PenSample};
use config::{
    Action, BrushPreset, Config, FrameLatency, LayersConfig, PenConfig, RebindCapture,
    SettingsCategory,
};
use doc::AppState;
use eframe::egui;
use eframe::egui_wgpu::RenderState;
use egui_dock::{DockArea, DockState};
use icons::Icon;
use newproject::{FormAction, NewProjectForm};
use paint::PaintLayer;
use std::time::Duration;
use workspace::{Pane, Stage, Workspace, Workspaces};

fn main() -> eframe::Result<()> {
    // Headless layout probe: runs the REAL panel/canvas layout on the CPU and
    // prints the canvas rect per scripted step, so any drawing-area movement
    // is a measurable number instead of an on-rig observation.
    if std::env::var_os("ANIMSTUDIO_PROBE").is_some() {
        probe();
        return Ok(());
    }
    // Dev loop only: hand over to a shadow copy so cargo can overwrite this
    // binary while the app stays open (Windows locks a running exe).
    let staging_failed = match devloop::stage_shadow() {
        devloop::Staging::HandedOver => return Ok(()),
        devloop::Staging::Continue => None,
        // Staging failed: we are running from — and locking — the binary cargo
        // must rewrite. Carry the reason into the app so it can SAY the loop is
        // off, instead of watching a file that can never change.
        devloop::Staging::Failed(why) => {
            eprintln!("devloop: {why}");
            Some(why)
        }
    };
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
            cc.egui_ctx.set_visuals(plate::visuals());
            plate::install_fonts(&cc.egui_ctx);
            Ok(Box::new(App::new(cc, config, staging_failed.clone())))
        }),
    )
}

/// A slate room tab (spec §4.2): kanji over romaji caps, framed, all four
/// identical in structure; the active room is lit by the Tally lamp.
fn slate_tab(ui: &mut egui::Ui, active: bool, kanji: &str, romaji: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(84.0, 34.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, 0.0, plate::WELL);
        p.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(
                1.0,
                if resp.hovered() && !active {
                    plate::LEGEND
                } else {
                    plate::legend_dim()
                },
            ),
            egui::StrokeKind::Inside,
        );
        let ink = if active || resp.hovered() {
            plate::STRUCK
        } else {
            plate::LEGEND
        };
        p.text(
            egui::pos2(rect.center().x, rect.top() + 11.0),
            egui::Align2::CENTER_CENTER,
            kanji,
            egui::FontId::new(13.0, egui::FontFamily::Proportional),
            ink,
        );
        p.text(
            egui::pos2(rect.center().x, rect.bottom() - 9.0),
            egui::Align2::CENTER_CENTER,
            romaji.to_uppercase(),
            egui::FontId::new(8.5, plate::semibold()),
            ink,
        );
        if active {
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 2.0, rect.bottom() - 3.0),
                egui::pos2(rect.right() - 2.0, rect.bottom()),
            );
            p.rect_filled(bar, 0.0, plate::TALLY);
        }
    }
    resp
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
    /// PSD-shipping: the published-build channel.
    update_rx: Option<std::sync::mpsc::Receiver<crate::update::UpdateEvent>>,
    update_ready: Option<(String, String, u64)>,
    update_swap: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    update_note: String,
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
    /// Dev loop: restart-on-rebuild (see devloop.rs and the ratified stanza in
    /// research/PSD-devloop-uihook.md). Inert without ANIMSTUDIO_DEVLOOP=1.
    devloop: devloop::DevLoop,
    /// UI hook: on-demand frame + state dump (see uidump.rs).
    uidump: uidump::UiDump,
    /// THE SESSION (research/PSD-session-room.md): the room this app is
    /// in, the peers it draws, and the 2FA connect window's state.
    session: net::Session,
    session_status: String,
    session_peers: std::collections::HashMap<u64, net::PeerView>,
    connect_open: bool,
    connect_addr: String,
    connect_code: String,
    connect_error: String,
    /// Host: the document generation last broadcast (change-driven
    /// snapshots — one truth, never a merge).
    session_sent_gen: u64,
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
    // ---- THE SESSION (research/PSD-session-room.md) ----------------------
    fn session_act(&mut self, a: config::SessionAction) {
        match a {
            config::SessionAction::StartHost => {
                let cfg = &self.config.session;
                if cfg.api_key.trim().is_empty() || cfg.totp_secret.trim().is_empty() {
                    self.session_status = "generate a room key and TOTP secret first".into();
                    return;
                }
                match net::Host::start(
                    cfg.host_port,
                    cfg.api_key.trim().to_string(),
                    cfg.totp_secret.trim().to_string(),
                ) {
                    Ok(h) => {
                        self.session_status = format!("hosting on port {} — share the key", h.port);
                        self.session = net::Session::Hosting(h);
                        self.session_sent_gen = u64::MAX;
                    }
                    Err(e) => self.session_status = format!("could not host: {e}"),
                }
            }
            config::SessionAction::StopHost | config::SessionAction::Leave => {
                self.session = net::Session::Idle;
                self.session_peers.clear();
                self.session_status = "offline".into();
            }
            config::SessionAction::OpenConnect => {
                self.connect_error.clear();
                self.connect_addr = self.config.session.last_addr.clone();
                self.connect_open = true;
            }
        }
    }
    fn session_join(&mut self, addr: String, code: String) {
        let cfg = &self.config.session;
        if cfg.api_key.trim().is_empty() {
            self.connect_error = "set the room key in Settings -> Session first".into();
            return;
        }
        match net::Client::connect(&addr, cfg.api_key.trim(), &code, cfg.username.trim()) {
            Ok(c) => {
                self.connect_open = false;
                self.connect_code.clear();
                self.connect_error.clear();
                self.config.session.last_addr = addr;
                self.config.save();
                self.session_status = "connected — waiting for the host's file".into();
                self.session = net::Session::Joined(c);
                self.session_peers.clear();
            }
            Err(e) => self.connect_error = e,
        }
    }

    /// Once per frame: drain net events, apply snapshots, push presence.
    /// PSD-shipping: read the updater threads' channels; light the
    /// lamp; when a swap lands, relaunch through the devloop's proven
    /// session-carry (NEVER-DO 2).
    fn update_pump(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.update_rx
            && let Ok(ev) = rx.try_recv()
        {
            self.update_rx = None;
            match ev {
                crate::update::UpdateEvent::Ready { tag, url, size } => {
                    self.update_note = format!("update {tag} available");
                    self.update_ready = Some((tag, url, size));
                }
                crate::update::UpdateEvent::UpToDate(tag) => {
                    self.update_note = format!(
                        "up to date (v{} · latest release {tag})",
                        crate::update::CURRENT_VERSION
                    );
                }
                crate::update::UpdateEvent::Note(n) => self.update_note = n,
            }
        }
        if let Some(rx) = &self.update_swap
            && let Ok(result) = rx.try_recv()
        {
            self.update_swap = None;
            match result {
                Ok(new_exe) => {
                    // The new build is seated. Save the session exactly as
                    // the devloop does, then hand over.
                    self.relaunch_into(new_exe, ctx);
                }
                Err(why) => {
                    self.update_note = format!("update failed — {why}");
                    if let Some(ed) = &mut self.editor {
                        ed.state.refuse(format!("update refused — {why}"));
                    }
                }
            }
        }
    }

    /// The handover into the freshly seated build: autosave + session
    /// (the devloop's own machinery), spawn with RESUME armed, exit.
    fn relaunch_into(&mut self, new_exe: std::path::PathBuf, ctx: &egui::Context) {
        let Some(editor) = &mut self.editor else {
            // No document open — nothing to carry; just hand over.
            let _ = std::process::Command::new(&new_exe)
                .env("ANIMSTUDIO_RESUME", "1")
                .spawn();
            std::process::exit(0);
        };
        let Some(target) = devloop::autosave_target() else {
            self.update_note = "update seated — restart the app to finish".into();
            return;
        };
        editor
            .state
            .palettes
            .save_into(&mut editor.state.engine.project);
        if let Err(e) = editor.state.engine.save(&target) {
            self.update_note = format!("update seated, but the autosave failed ({e}) — save your work and restart");
            return;
        }
        let session = devloop::Session {
            pending: true,
            project: Some(target),
            origin: editor.state.file_path.clone(),
            frame: editor.state.view.frame,
            written_ms: devloop::session_stamp(),
        };
        if let Err(e) = session.save() {
            self.update_note = format!("update seated, but the session write failed ({e}) — restart by hand");
            return;
        }
        ctx.request_repaint();
        match std::process::Command::new(&new_exe)
            .env("ANIMSTUDIO_RESUME", "1")
            .spawn()
        {
            Ok(_) => std::process::exit(0),
            Err(e) => {
                self.update_note = format!("could not start the new build ({e})");
            }
        }
    }

    fn session_pump(&mut self, ctx: &egui::Context) {
        let mut events: Vec<net::NetEvent> = Vec::new();
        match &self.session {
            net::Session::Hosting(h) => {
                while let Ok(ev) = h.events.try_recv() {
                    events.push(ev);
                }
            }
            net::Session::Joined(c) => {
                while let Ok(ev) = c.events.try_recv() {
                    events.push(ev);
                }
            }
            net::Session::Idle => {}
        }
        let mut fresh_join = false;
        for ev in events {
            match ev {
                net::NetEvent::Status(st) => self.session_status = st,
                net::NetEvent::PeerJoined { id, name } => {
                    self.session_status = format!("{name} joined");
                    // AUDIT [44]: this reached Settings only — invisible
                    // to someone who is drawing. Chatter, not a refusal.
                    if let Some(ed) = &mut self.editor {
                        ed.state.status = format!("{name} joined the room");
                    }
                    fresh_join = true;
                    self.session_peers.insert(
                        id,
                        net::PeerView {
                            name,
                            frame: 0,
                            cursor: None,
                            pen_down: false,
                            wet: Vec::new(),
                        },
                    );
                }
                net::NetEvent::PeerLeft { id } => {
                    if let Some(p) = self.session_peers.remove(&id) {
                        self.session_status = format!("{} left", p.name);
                        let who = p.name.clone();
                        if let Some(ed) = &mut self.editor {
                            ed.state.status = format!("{who} left the room");
                        }
                    }
                }
                net::NetEvent::Presence {
                    id,
                    frame,
                    cursor,
                    pen_down,
                    wet,
                } => {
                    let e = self.session_peers.entry(id).or_insert(net::PeerView {
                        name: if id == 0 {
                            "host".into()
                        } else {
                            format!("artist {id}")
                        },
                        frame,
                        cursor,
                        pen_down,
                        wet: Vec::new(),
                    });
                    e.frame = frame;
                    e.cursor = cursor;
                    e.pen_down = pen_down;
                    e.wet = wet;
                }
                net::NetEvent::Snapshot(bytes) => {
                    // ONE TRUTH (NEVER-DO 1): the host's document replaces
                    // ours wholesale, never merged; refused outright while a
                    // gesture is live (the next snapshot lands instead).
                    let busy = self
                        .editor
                        .as_ref()
                        .is_some_and(|ed| ed.canvas.stroke_active());
                    if !busy {
                        let path = std::env::temp_dir().join("animstudio_session_in.animproj");
                        if std::fs::write(&path, &bytes).is_ok() {
                            match doc::AppState::load_from(path) {
                                Ok(st) => {
                                    let rs = self.render_state.as_ref();
                                    let mut ed = Editor::from_state(st, rs);
                                    ed.stage = self.editor.as_ref().and_then(|o| o.stage);
                                    ed.canvas.engine_changed();
                                    // The mirror is not ours to save over.
                                    ed.state.file_path = None;
                                    ed.state.status = "live from the host".into();
                                    self.editor = Some(ed);
                                    self.session_status = "file synced".into();
                                    if let Some(ed) = &mut self.editor {
                                        ed.state.status =
                                            "the host's file arrived".into();
                                    }
                                }
                                Err(e) => self.session_status = format!("snapshot refused: {e}"),
                            }
                        }
                    }
                }
                // HOST: a guest's finished stroke. One writer — OUR engine
                // applies it, by drawing id + layer NAME, tagged with its
                // author; a refusal goes home instead of a guess.
                net::NetEvent::EditTiles {
                    author,
                    drawing,
                    layer_name,
                    tiles,
                } => {
                    let tiles: Vec<(
                        anim_core::raster::TileCoord,
                        std::sync::Arc<anim_core::raster::TileData>,
                    )> = tiles
                        .into_iter()
                        // TileData::from_vec re-hashes on arrival, so a
                        // malformed patch can never poison the host's
                        // content addressing — and it length-checks.
                        .filter(|(_, _, t)| t.len() == anim_core::raster::TILE_LEN)
                        .map(|(x, y, texels)| {
                            (
                                (x, y),
                                std::sync::Arc::new(anim_core::raster::TileData::from_vec(texels)),
                            )
                        })
                        .collect();
                    let outcome = self.editor.as_mut().map(|ed| {
                        ed.state.apply_remote_tiles(
                            &author,
                            anim_core::ids::DrawingId(drawing),
                            &layer_name,
                            tiles,
                        )
                    });
                    match outcome {
                        Some(Err(why)) => {
                            if let net::Session::Hosting(h) = &self.session {
                                h.send_refusal(&author, &why);
                            }
                            self.session_status = format!("{author}'s edit refused: {why}");
                        }
                        Some(Ok(())) => {
                            if let Some(ed) = &mut self.editor {
                                ed.canvas.engine_changed();
                            }
                        }
                        None => {}
                    }
                }
                // HOST: a guest asking to undo/redo their own step.
                net::NetEvent::UndoRequest { author, redo } => {
                    let outcome = self
                        .editor
                        .as_mut()
                        .map(|ed| ed.state.remote_history(&author, redo));
                    match outcome {
                        Some(Err(why)) => {
                            if let net::Session::Hosting(h) = &self.session {
                                h.send_refusal(&author, &why);
                            }
                        }
                        Some(Ok(())) => {
                            if let Some(ed) = &mut self.editor {
                                ed.canvas.engine_changed();
                            }
                        }
                        None => {}
                    }
                }
                // GUEST: the host refused something of ours — the Aka lane.
                net::NetEvent::EditRefused(why) => {
                    if let Some(ed) = &mut self.editor {
                        ed.state.refuse(format!("refused by the host — {why}"));
                    }
                    self.session_status = format!("refused: {why}");
                }
                net::NetEvent::Ended(why) => {
                    self.session = net::Session::Idle;
                    self.session_peers.clear();
                    // AUDIT [13]: this reached ONLY the Settings page, so
                    // the artist kept drawing believing they were still
                    // in the room. Losing the room contradicts what they
                    // think is happening — Aka, at the pen.
                    if let Some(ed) = &mut self.editor {
                        ed.state.refuse(
                            "the room closed — you are working on your own copy again",
                        );
                    }
                    self.session_status = why;
                }
            }
        }
        let presence = self.editor.as_ref().map(|ed| net::PresenceOut {
            frame: ed.state.view.frame,
            cursor: ed.canvas.presence_pos,
            pen_down: ed.canvas.stroke_active(),
            wet: ed.canvas.presence_wet(),
        });
        let mut snapshot: Option<Vec<u8>> = None;
        if let net::Session::Hosting(_) = &self.session {
            // Change-driven: undo depth is a cheap generation stamp; a
            // fresh join always forces one.
            let stamp = self.editor.as_ref().map_or(0u64, |ed| ed.state.doc_gen);
            if fresh_join || stamp != self.session_sent_gen {
                self.session_sent_gen = stamp;
                snapshot = self
                    .editor
                    .as_mut()
                    .and_then(|ed| ed.state.snapshot_bytes());
            }
        }
        // V2: a guest's finished strokes leave here (one writer — the
        // host applies them and sends the truth back).
        let outbox: Vec<(u64, String, Vec<(i32, i32, Vec<u16>)>)> = self
            .editor
            .as_mut()
            .map(|ed| std::mem::take(&mut ed.canvas.edit_outbox))
            .unwrap_or_default();
        let history_req = self
            .editor
            .as_mut()
            .and_then(|ed| ed.canvas.history_request.take());
        let me = self.config.session.username.trim().to_string();
        match &mut self.session {
            net::Session::Hosting(h) => {
                if let Some(p) = &presence {
                    h.send_presence(p);
                }
                if let Some(b) = &snapshot {
                    h.send_snapshot(b);
                }
            }
            net::Session::Joined(c) => {
                if let Some(p) = &presence {
                    c.send_presence(p);
                }
                for (drawing, layer_name, tiles) in outbox {
                    c.send_edit(&me, drawing, &layer_name, tiles);
                }
                if let Some(redo) = history_req {
                    c.send_undo(&me, redo);
                }
            }
            net::Session::Idle => {}
        }
        let guest = matches!(self.session, net::Session::Joined(_));
        if let Some(ed) = &mut self.editor {
            ed.canvas.is_guest = guest;
        }
        if !matches!(self.session, net::Session::Idle) {
            let peers: Vec<net::PeerView> = self.session_peers.values().cloned().collect();
            if let Some(ed) = &mut self.editor {
                ed.canvas.peers = peers;
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else if let Some(ed) = &mut self.editor
            && !ed.canvas.peers.is_empty()
        {
            ed.canvas.peers.clear();
        }
    }
    fn new(
        cc: &eframe::CreationContext<'_>,
        config: Config,
        staging_failed: Option<String>,
    ) -> Self {
        let backend = cc
            .wgpu_render_state
            .as_ref()
            .map(|rs| format!("{:?}", rs.adapter.get_info().backend))
            .unwrap_or_else(|| "none".to_string());
        // The Manager is built LAZILY on the first UI frame, NOT here:
        // RealTimeStylus initialization on the UI thread can block on window
        // messages, and App::new runs BEFORE the event loop pumps — building
        // here deadlocked the app at startup (Event Log: AppHangB1).
        // Dev loop restore: a pending session means the previous process
        // handed over after a rebuild. The document comes back through the
        // ordinary load path — a file in the project's own format — which is
        // exactly why no struct layout can disagree across the restart.
        // The pending flag is cleared only AFTER the document is genuinely
        // back — clearing it on read threw away the only pointer to the work
        // whenever the load failed, and did it silently.
        let mut restore_note: Option<String> = None;
        let restored = devloop::Session::peek_pending().and_then(|s| {
            let path = s.project.clone()?;
            match doc::AppState::load_from(path.clone()) {
                Ok(mut state) => {
                    // The autosave is scaffolding. The document's REAL save
                    // target is carried across in `origin` and restored
                    // unconditionally: `None` for a document that never had a
                    // file (Save prompts), the true path otherwise. Deriving
                    // this from a flag was the critical defect — a real
                    // project came back pointing at %APPDATA% scratch storage
                    // and the next Ctrl+S wrote there.
                    state.file_path = s.origin.clone();
                    let last = state.frame_count().saturating_sub(1);
                    state.goto(s.frame.min(last));
                    state.status = "restored after rebuild".into();
                    devloop::Session::clear_pending();
                    Some(Editor::from_state(state, cc.wgpu_render_state.as_ref()))
                }
                Err(e) => {
                    // Leave `pending` set: the autosave is still on disk and
                    // still the newest copy of the work. Say so loudly rather
                    // than opening an empty editor over the top of it.
                    restore_note = Some(format!(
                        "⚠ could not reopen the autosave ({e}) — your work is still at {}",
                        path.display()
                    ));
                    None
                }
            }
        });
        Self {
            render_state: cc.wgpu_render_state.clone(),
            editor: restored,
            new_form: None,
            update_rx: {
                // The dev channel wins: an armed dev build never
                // self-updates (NEVER-DO 4). Sweep any stepped-aside
                // binary — this launch worked (NEVER-DO 1).
                crate::update::sweep_old();
                if !devloop::armed() && !config.update_repo.trim().is_empty() {
                    Some(crate::update::spawn_check(
                        config.update_repo.trim().to_string(),
                    ))
                } else {
                    None
                }
            },
            config,
            settings_open: false,
            update_ready: None,
            update_swap: None,
            update_note: String::new(),
            settings_category: SettingsCategory::default(),
            capturing: None,
            backend,
            tablet_rx: None,
            tablet_boot_tried: false,
            devloop: {
                let mut d = devloop::DevLoop::new(staging_failed);
                if let Some(n) = restore_note {
                    d.note = Some(n);
                }
                d
            },
            uidump: uidump::UiDump::new(),
            session: net::Session::Idle,
            session_status: "offline".into(),
            session_peers: std::collections::HashMap::new(),
            connect_open: false,
            connect_addr: String::new(),
            connect_code: String::new(),
            connect_error: String::new(),
            session_sent_gen: u64::MAX,
        }
    }

    /// What the app believes it is showing, read from the SAME state that drew
    /// the frame. Nothing here is recomputed for the dump's benefit — a value
    /// that had to be derived specially would be a second source of truth, and
    /// a second source of truth is how the instrument starts lying.
    fn describe_ui(&self, ctx: &egui::Context, screen: egui::Rect) -> String {
        let ppp = ctx.pixels_per_point();
        let Some(ed) = &self.editor else {
            return uidump::Describe {
                project: "<no project open>",
                width: 0,
                height: 0,
                fps: 0,
                frame: 0,
                frame_count: 0,
                scenes: 0,
                stage: None,
                panes: Vec::new(),
                tool: "-",
                brush: "-",
                playing: false,
                has_file: false,
                file_path: None,
                pixels_per_point: ppp,
                screen,
            }
            .render();
        };
        // Only the panes actually ON SCREEN — the same helper the renderer
        // uses to decide what to draw. `iter_all_tabs()` walks every tab of
        // every node including ones hidden behind a stack, so the record
        // listed panes the image did not contain.
        let panes: Vec<String> = visible_panes(&ed.dock)
            .into_iter()
            .map(|p| format!("{p:?}"))
            .collect();
        let proj = &ed.state.engine.project;
        uidump::Describe {
            project: &proj.name,
            width: proj.width,
            height: proj.height,
            fps: proj.fps,
            frame: ed.state.view.frame as usize,
            frame_count: ed.state.frame_count() as usize,
            scenes: proj.scenes.len(),
            stage: ed.stage.map(|s| s.name()),
            panes,
            // The REAL armed tool, read from the field that drove this frame's
            // input handling. Deriving it from `composite_view` reported
            // "draw" while the select tool was active — a false value in a
            // record whose only job is to be true.
            tool: if ed.canvas.composite_view {
                "composite view"
            } else if ed.canvas.erasing {
                "eraser"
            } else {
                match ed.canvas.tool {
                    canvas::CanvasTool::Paint => "brush",
                    canvas::CanvasTool::Select => "select",
                    canvas::CanvasTool::Fill => "fill",
                }
            },
            brush: "-",
            playing: ed.state.view.playing,
            // `AppState` has no modification tracking, so this reports what it
            // can actually see: whether the document has a file yet. Labelling
            // that "(modified)" claimed knowledge the app does not have.
            has_file: ed.state.file_path.is_some(),
            file_path: ed.state.file_path.as_ref().map(|p| p.display().to_string()),
            pixels_per_point: ppp,
            screen,
        }
        .render()
    }

    /// Hand over to a freshly built binary, without costing work.
    ///
    /// NEVER-DO 2 lives here: this refuses unless the document is provably
    /// recoverable and no gesture is in flight. Every refusal returns and is
    /// retried on a later frame — a refused restart is never a lost restart.
    fn try_handover(&mut self, ctx: &egui::Context) {
        let Some(_) = &mut self.editor else {
            // Nothing open: no document to preserve, so nothing to write.
            let why = self.devloop.relaunch();
            self.devloop.note = Some(format!("relaunch failed: {why}"));
            self.devloop.give_up();
            return;
        };
        let editor = self.editor.as_mut().expect("checked above");
        // A stroke in progress has points that exist only in the paint layer's
        // live buffer; restarting mid-gesture would drop them.
        if editor.canvas.stroke_active() {
            self.devloop.note = Some("rebuild ready — finishing your stroke first".into());
            return;
        }
        if editor.export_job.is_some() {
            self.devloop.note = Some("rebuild ready — waiting for the export".into());
            return;
        }
        if self.settings_open || self.new_form.is_some() {
            self.devloop.note = Some("rebuild ready — close the dialog to apply".into());
            return;
        }
        // Always autosave, and always restore from the autosave: the user's own
        // file is never written behind their back, and nothing is ever lost.
        let Some(target) = devloop::autosave_target() else {
            self.devloop.note = Some("rebuild ready — no autosave path; not restarting".into());
            return;
        };
        editor
            .state
            .palettes
            .save_into(&mut editor.state.engine.project);
        if let Err(e) = editor.state.engine.save(&target) {
            self.devloop.note = Some(format!(
                "⚠ rebuild ready — autosave FAILED ({e}); not restarting"
            ));
            // Not a refusal — retrying this every 400ms would hammer the disk
            // and never succeed. Wait for the next rebuild.
            self.devloop.give_up();
            return;
        }
        let session = devloop::Session {
            pending: true,
            project: Some(target),
            // The document's true save target, carried across untouched.
            origin: editor.state.file_path.clone(),
            frame: editor.state.view.frame,
            written_ms: devloop::session_stamp(),
        };
        // If the session cannot be written, the autosave on disk is an orphan
        // nothing will reopen. Exiting now would look identical to success.
        if let Err(e) = session.save() {
            self.devloop.note = Some(format!(
                "⚠ rebuild ready — session write FAILED ({e}); not restarting"
            ));
            self.devloop.give_up();
            return;
        }
        ctx.request_repaint();
        let why = self.devloop.relaunch();
        self.devloop.note = Some(format!("⚠ relaunch failed: {why}"));
        self.devloop.give_up();
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
                    eprintln!("native tablet thread ended — falling back to standard pen input");
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
        // MISS CHECK (shiage): the hole-hunt ground flip — pure display.
        if action == Action::MissCheck {
            ed.canvas.miss_check = !ed.canvas.miss_check;
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
            // THE POT LENS (shiage charter, the grammar's sole key
            // exception): with FILL armed, 1–8 arm the active character's
            // pots; with the brush armed they arm brush presets as
            // everywhere else. The Slate names what got armed.
            if ed.canvas.arming_pencil() == "fill" && !ed.canvas.stroke_active() {
                let nchars = ed.state.palettes.characters.len();
                if nchars > 0 {
                    let ci = ed.state.active_character.min(nchars - 1);
                    if let Some(role) = ed.state.palettes.characters[ci].roles.get(i).cloned() {
                        ed.canvas.brush_color = role.color;
                        ed.state.status = format!(
                            "pot: {} · {}",
                            ed.state.palettes.characters[ci].name, role.name
                        );
                    }
                }
                return;
            }
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
                    | Action::PrevCut
                    | Action::NextCut
            )
        {
            ed.state.refuse("refused — finish the stroke first");
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
            // One verb, one key (room charters): Q/W step the exposure —
            // jump to the adjacent KEY on the active column, not the
            // adjacent frame.
            Action::PrevKey => s.goto_adjacent_key(false),
            Action::NextKey => s.goto_adjacent_key(true),
            // Handled above (canvas state, not doc state) — the match must
            // still be exhaustive.
            Action::MissCheck => {}
            Action::PrevCut => s.step_cut(false),
            Action::NextCut => s.step_cut(true),
            Action::ToggleLoop => s.view.loop_playback = !s.view.loop_playback,
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
            // SESSION v2: in someone's room, undo/redo travel to the host
            // — it owns the one history, and honours only THIS artist's
            // own last step (refusing when a later edit shares the layer).
            Action::Undo | Action::Redo => {
                let redo = action == Action::Redo;
                if ed.canvas.is_guest {
                    ed.canvas.history_request = Some(redo);
                    ed.state.status = if redo {
                        "asked the host to redo your last step…".into()
                    } else {
                        "asked the host to undo your last step…".into()
                    };
                    return;
                } else if redo {
                    s.redo();
                } else {
                    s.undo();
                }
            }
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
                        pressure: None,  // first Pose supplies real pressure
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
    /// PSD-shipping: UPDATE READY clicked in the Foot (App acts on it).
    request_update: bool,
    /// The ready update's tag, threaded from the App for the Foot lamp.
    update_tag: Option<String>,
    /// Set by the canvas's INPUT plate field: open Settings at the pen page.
    request_settings_pen: bool,
    /// The docking shell: every UI element is a movable pane over ONE document;
    /// a workspace is just a saved arrangement of these panes.
    dock: DockState<Pane>,
    /// CHATTER decay (the Foot): the last status string seen, and when it
    /// changed — a stale "painted" must never sit in the lane forever.
    status_seen: String,
    status_since: f64,
    /// REFUSAL lane bookkeeping (Aka, dwells 4s; repeats re-flash by seq).
    refusal_seen_seq: u32,
    refusal_since: f64,
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
    /// THE BRUSH FORGE (PSD-brush-forge): the draft under the hammer.
    forge: forge::ForgeState,
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
type SceneCutTree = Vec<(
    anim_core::ids::SceneId,
    String,
    Vec<(anim_core::ids::CutId, String)>,
)>;

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
        // AUDIT [6]: same words as the keyboard guard (main.rs) but in
        // the chatter lane — half the app's refusals were grey.
        state.refuse("refused — finish the stroke first");
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
    /// The active spine stage — panes may lens their CONTENT by room.
    stage: Option<Stage>,
    pen: &'a PenConfig,
    layers_cfg: &'a LayersConfig,
    presets: &'a mut Vec<BrushPreset>,
    presets_dirty: &'a mut bool,
    preset_name: &'a mut String,
    forge: &'a mut forge::ForgeState,
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
        // Every pane sits on the plate's grain (owner's amendment): laid
        // down first, so all content paints over the material.
        plate::surface(ui);
        match tab {
            Pane::XSheet => {
                // The henshu lens (charter delta 4): the sheet grows its
                // edit-room HEAD only in the Edit stage; content lensing,
                // dock recipes untouched.
                let edit_room = self.stage == Some(Stage::Edit);
                let gesture_safe = !self.canvas.stroke_active();
                xsheet_panel::ui(ui, self.state, edit_room, gesture_safe)
            }
            Pane::Layers => {
                let gesture_safe = !self.canvas.stroke_active();
                xsheet_panel::cel_layers_strip(ui, self.state, gesture_safe)
            }
            Pane::Brush => {
                // The deck moved onto the canvas itself (owner's directive
                // 2026-08-17) — this pane is a pointer, not a duplicate.
                plate::legend(ui, "brush");
                ui.label(
                    "the brush deck rides on the canvas now — close this pane \
                     (panes menu can reopen it)",
                );
            }
            Pane::Presets => self.presets_ui(ui),
            Pane::Forge => forge::ui(
                ui,
                self.forge,
                self.presets,
                self.presets_dirty,
                self.canvas,
                &mut self.state.status,
            ),
            Pane::Palette => palette_panel::ui(ui, self.state, self.canvas),
            Pane::NodeGraph => nodegraph_panel::ui(ui, self.state),
            Pane::Canvas if self.float_canvas_open => {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(
                            "canvas is open in its own window — panes ▾ to bring it back",
                        )
                        .color(plate::legend_dim()),
                    );
                });
            }
            Pane::Canvas => {
                if self.stage == Some(Stage::Layout) {
                    // Owner (2026-08-17): the Layout room's blank canvas is
                    // the cut's PREVIEW — the animation plays here, composed
                    // by the node graph, while drawing stays in genga.
                    let vs = self.viewers.entry(u8::MAX).or_default();
                    viewer::ui(ui, self.state, self.graph, vs);
                } else {
                    self.canvas.ui(
                        ui,
                        self.state,
                        self.paint.as_deref_mut(),
                        self.graph,
                        self.pen,
                        self.layers_cfg,
                        self.native_pen,
                        self.presets,
                    )
                }
            }
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
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, p) in self.presets.iter().enumerate() {
                    let hotkey = if i < 8 {
                        format!("{} ", i + 1)
                    } else {
                        "· ".into()
                    };
                    let mut text =
                        egui::RichText::new(format!("{hotkey}{}  {:.0}px", p.name, p.size_px));
                    if let Some(c) = p.color {
                        text = text.color(egui::Color32::from_rgb(c[0], c[1], c[2]));
                    }
                    if ui.button(text).on_hover_text("apply preset").clicked() {
                        self.canvas.apply_preset(p);
                        self.state.status = format!("brush: {}", p.name);
                    }
                }
                if self.presets.is_empty() {
                    ui.label(egui::RichText::new("no presets — save one above").color(plate::legend_dim()));
                }
            });
    }
}

impl Editor {
    fn from_form(form: &NewProjectForm, rs: Option<&RenderState>) -> Self {
        // AUDIT [31] follow-on: the name field now starts EMPTY so its
        // hint is reachable, so an untyped name must still produce a
        // named cut rather than a blank one.
        let name = match form.name.trim() {
            "" => "Untitled".to_string(),
            n => n.to_string(),
        };
        let state = AppState::new_project(
            name,
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
            request_update: false,
            update_tag: None,
            request_settings_pen: false,
            // Every project opens in the Drawing room — the daily driver
            // and the spine's canonical default.
            dock: Stage::Drawing.dock(),
            stage: Some(Stage::Drawing),
            status_seen: String::new(),
            status_since: 0.0,
            refusal_seen_seq: 0,
            refusal_since: -10.0,
            workspaces: Workspaces::load(),
            ws_name: String::new(),
            rename_buf: String::new(),
            preset_name: String::new(),
            forge: forge::ForgeState::default(),
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
            request_update: false,
            update_tag: None,
            request_settings: false,
            request_settings_pen: false,
            dock: Stage::Drawing.dock(),
            stage: Some(Stage::Drawing),
            status_seen: String::new(),
            status_since: 0.0,
            refusal_seen_seq: 0,
            refusal_since: -10.0,
            workspaces: Workspaces::load(),
            ws_name: String::new(),
            rename_buf: String::new(),
            preset_name: String::new(),
            forge: forge::ForgeState::default(),
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
        // UI hook: drain any screenshot reply from last frame, then honour a
        // dump request. F12 is deliberately not rebindable — it is an
        // instrument, not an editing action, and it must work identically on
        // the build the owner runs (root: same build, or it lies).
        // A reply from last frame lands here, at the top of the NEXT frame —
        // which is the state the captured frame ended in, i.e. what the pixels
        // actually show. The description is taken now, not at request time.
        if let Some(img) = self.uidump.take_image(ui.ctx()) {
            let desc = self.describe_ui(ui.ctx(), ui.max_rect());
            self.uidump.write(img, desc);
        }
        if ui.input(|i| i.key_pressed(egui::Key::F12)) || self.uidump.auto_pending() {
            self.uidump.request(ui.ctx());
        }
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
            let room = match &self.session {
                net::Session::Hosting(_) => Some((
                    format!("hosting · {} joined", self.session_peers.len()),
                    self.session_peers
                        .values()
                        .map(|p| p.name.clone())
                        .collect(),
                )),
                net::Session::Joined(_) => Some((
                    "in a room".to_string(),
                    self.session_peers
                        .values()
                        .map(|p| p.name.clone())
                        .collect(),
                )),
                net::Session::Idle => None,
            };
            editor.update_tag = self.update_ready.as_ref().map(|(t, _, _)| t.clone());
            editor.ui(
                ui,
                &pen,
                &layers_cfg,
                &mut presets,
                &mut presets_dirty,
                &native_pen,
                self.config.ui.lock_positions,
                room,
            );
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
            if editor.request_settings_pen {
                editor.request_settings_pen = false;
                self.settings_open = true;
                self.settings_category = config::SettingsCategory::Pen;
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
                let more = ui
                    .ctx()
                    .input_for(floatcanvas::FloatCanvas::viewport_id(), collect);
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

        // Exact UI descriptions: one global read per frame (plate serves
        // every control's hover from it).
        plate::set_exact(self.config.ui.exact_descriptions);

        // Settings window (Krita-style: shortcuts + performance).
        let raster_toggle = self
            .editor
            .as_mut()
            .filter(|ed| ed.paint.is_some())
            .map(|ed| &mut ed.canvas.raster);
        let mut session_action: Option<config::SessionAction> = None;
        let peer_names: Vec<String> = self
            .session_peers
            .values()
            .map(|p| p.name.clone())
            .collect();
        let mut check_now = false;
        let hosting = matches!(self.session, net::Session::Hosting(_));
        let joined = matches!(self.session, net::Session::Joined(_));
        config::settings_window(
            ui.ctx(),
            &mut self.settings_open,
            &mut self.config,
            &mut self.capturing,
            &mut self.settings_category,
            &self.backend,
            raster_toggle,
            &mut session_action,
            &self.session_status,
            hosting,
            joined,
            &peer_names,
            &self.update_note,
            &mut check_now,
        );
        if let Some(a) = session_action {
            self.session_act(a);
        }
        // PSD-shipping: "check now" from the Plugins page.
        if check_now && !self.config.update_repo.trim().is_empty() {
            self.update_note = "checking…".into();
            self.update_rx = Some(crate::update::spawn_check(
                self.config.update_repo.trim().to_string(),
            ));
        }
        // THE CONNECT WINDOW: the 2FA code lives here and nowhere else.
        if let Some((addr, code)) = config::connect_window(
            ui.ctx(),
            &mut self.connect_open,
            &mut self.connect_addr,
            &mut self.connect_code,
            &self.connect_error,
            false,
        ) {
            self.session_join(addr, code);
        }
        self.session_pump(ui.ctx());
        // PSD-shipping: the published-build channel (a thread talks to
        // GitHub; this only reads channels — never blocks the pen).
        self.update_pump(ui.ctx());
        // The Foot's UPDATE READY click: start the download+swap thread,
        // guarded like a devloop handover (no stroke, no dialogs).
        if let Some(ed) = &mut self.editor
            && std::mem::take(&mut ed.request_update)
        {
            let busy = ed.canvas.stroke_active() || self.settings_open || self.new_form.is_some();
            if busy {
                ed.state
                    .refuse("refused — finish the stroke / close dialogs, then update");
            } else if let Some((_tag, url, size)) = self.update_ready.clone()
                && self.update_swap.is_none()
            {
                self.update_note = "downloading the published build…".into();
                ed.state.status = "downloading the published build…".into();
                self.update_swap = Some(crate::update::spawn_swap(url, size));
            }
        }

        // Optional FPS overlay.
        if self.config.perf.show_fps {
            let dt = ui.ctx().input(|i| i.stable_dt).max(1e-4);
            egui::Area::new("fps_overlay".into())
                .anchor(egui::Align2::RIGHT_TOP, [-10.0, 96.0])
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(egui::Color32::from_black_alpha(160))
                        .inner_margin(4.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("{:.0} fps", 1.0 / dt))
                                    .monospace()
                                    .color(plate::LEGEND),
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

        // Dev loop LAST, not first. The only gesture guard is
        // `canvas.stroke_active()`, and the canvas does not set that until it
        // has consumed this frame's input — so polling at the top of the frame
        // could hand over between the pen going down and the stroke existing,
        // which is precisely the mid-gesture restart NEVER-DO 2 forbids.
        if self.devloop.poll() {
            self.try_handover(ui.ctx());
        }
        // Both instruments write their messages to fields; without this they
        // are written and never read, so a failed autosave and a successful
        // one look identical. The editor's status line is the channel that
        // actually reaches the screen.
        if let Some(msg) = self.devloop.note.take().or_else(|| self.uidump.last.take()) {
            if let Some(ed) = &mut self.editor {
                ed.state.status = msg;
            } else {
                eprintln!("{msg}");
            }
        }
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
                // AUDIT [22]: the only surface in the app with no plate
                // grain — and the first one an artist ever sees.
                plate::surface(ui);
                ui.vertical_centered(|ui| {
                    ui.add_space(80.0);
                    egui::Frame::new()
                        .fill(plate::WELL)
                        .stroke(egui::Stroke::new(1.0, plate::legend_dim()))
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
                // AUDIT [16]: a failed load used to vanish — the same
                // dialog reappeared with no reason. Carry the reason back
                // onto the form so the screen can say what went wrong.
                Some(Err(msg)) => {
                    form.error = Some(format!("could not open that project — {msg}"));
                    self.new_form = Some(form);
                }
                None => {
                    // Cancelled: not a failure, and it clears any old reason.
                    form.error = None;
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
    let run =
        |editor: &mut Editor, label: &str, last: &mut Option<egui::Rect>, moved: &mut usize| {
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
            editor.ui(
                &mut root,
                &pen,
                &layers_cfg,
                &mut presets,
                &mut presets_dirty,
                &[],
                config.ui.lock_positions,
                None,
            );
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
        run(
            &mut editor,
            if f == 0 {
                "goto cel frame"
            } else {
                "goto empty frame"
            },
            &mut last,
            &mut moved,
        );
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
                    self.audio_out = Some(AudioOut {
                        _stream: stream,
                        sink,
                    });
                }
                Err(e) => {
                    // Once, not per frame; playback stays silent but works.
                    eprintln!("audio device unavailable: {e}");
                    self.state.status = "no audio device — playing silent".into();
                }
            }
        }
        let Some(out) = &self.audio_out else { return };
        let Some(clip) = &self.state.cut().audio else {
            return;
        };
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
        let Some(job) = &mut self.export_job else {
            return;
        };
        // (String, failed?) — a failure refuses, success and cancel chatter.
        let mut finished: Option<(String, bool)> = None;
        loop {
            match job.rx.try_recv() {
                Ok(export::ExportProgress::Frame) => job.done += 1,
                Ok(export::ExportProgress::Encoding) => job.encoding = true,
                Ok(export::ExportProgress::Done(result)) => {
                    finished = Some(match result {
                        Ok((n, note)) => {
                            (format!("exported {n} frame(s) ({}){note}", job.kind), false)
                        }
                        Err(e) if e == export::CANCELLED => {
                            ("export cancelled".into(), false)
                        }
                        Err(e) => (
                            format!("the {} export did not finish — {e}", job.kind),
                            true,
                        ),
                    });
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finished = Some((
                        format!("the {} export stopped without finishing", job.kind),
                        true,
                    ));
                    break;
                }
            }
        }
        if let Some((status, failed)) = finished {
            self.export_job = None;
            // AUDIT [19]: a failure after minutes of rendering used to
            // fade out of the chatter lane in four seconds.
            if failed {
                self.state.refuse(status);
            } else {
                self.state.status = status;
            }
        }
    }

    /// THE FOOT's right lane: the room lamp, the tape counter, and the pen
    /// diagnostics. Laid out FIRST from the row's right edge so a long
    /// chatter line can never push it off the screen (the clipping the
    /// owner's frame_007 debug outlines caught).
    fn foot_right(&mut self, ui: &mut egui::Ui, room: &Option<(String, Vec<String>)>) {
        // PERSISTENT lane: history as a tape counter, and the
        // pen diagnostics (fixed-width, never in a toolbar).
        ui.add_space(8.0);
        // PSD-shipping: UPDATE READY — the published build moved past
        // this one. Tally lamp; the click IS the relaunch (session
        // carried like a dev-loop restart).
        if let Some(tag) = self.update_tag.clone() {
            if ui
                .button(
                    egui::RichText::new(format!("relaunch to update ({tag})"))
                        .size(10.5)
                        .color(plate::STRUCK),
                )
                .on_hover_text(
                    "download the published build and relaunch — your session                      carries over exactly like a dev-loop restart",
                )
                .clicked()
            {
                self.request_update = true;
            }
            let (dot, _) =
                ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.5, plate::TALLY);
            ui.separator();
        }
        // THE ROOM LAMP (session): being in a room is a
        // CONFIGURATION — a plate fact, never Aka. It names
        // the role and who is here, so the artist never opens
        // Settings to answer "am I connected?".
        {
            if let Some((txt, names)) = &room {
                ui.label(egui::RichText::new(txt).size(10.5).color(plate::STRUCK))
                    .on_hover_text(if names.is_empty() {
                        "no one else here yet".to_string()
                    } else {
                        format!("with: {}", names.join(", "))
                    });
                let _ = &txt;
                // The lamp itself: Tally = live room.
                let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.5, plate::TALLY);
                ui.separator();
            }
        }
        let can_redo = self.state.engine.can_redo();
        let can_undo = self.state.engine.can_undo();
        let mut tape = egui::text::LayoutJob::default();
        let seg = |job: &mut egui::text::LayoutJob, t: &str, on: bool| {
            job.append(
                t,
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::new(10.5, plate::semibold()),
                    color: if on {
                        plate::LEGEND
                    } else {
                        plate::legend_dim()
                    },
                    ..Default::default()
                },
            );
        };
        seg(&mut tape, "REDO", can_redo);
        seg(&mut tape, " · ", false);
        seg(&mut tape, "UNDO", can_undo);
        ui.label(tape);
        ui.separator();
        let (diag, healthy, mouse_mode) = self.canvas.pressure_diag();
        if mouse_mode {
            ui.label(egui::RichText::new("input: mouse — no pressure").color(plate::LEGEND));
        } else {
            ui.label(
                egui::RichText::new(diag)
                    .monospace()
                    .size(11.0)
                    .color(if healthy {
                        plate::STRUCK
                    } else {
                        plate::legend_dim()
                    }),
            );
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
                plate::surface(ui);
                ui.set_min_width(260.0);
                // AUDIT [15]: this was a stock egui ProgressBar — the one
                // surface the artist stares at for minutes, speaking a
                // different language from the app behind it.
                if job.encoding {
                    plate::legend(ui, "encoding video");
                    plate::meter(ui, None, 240.0);
                } else {
                    let frac = if job.total > 0 {
                        job.done as f32 / job.total as f32
                    } else {
                        0.0
                    };
                    plate::legend(ui, "rendering frames");
                    plate::meter(ui, Some(frac), 240.0);
                    ui.label(
                        egui::RichText::new(format!("{} / {}", job.done, job.total))
                            .monospace()
                            .size(11.0)
                            .color(plate::STRUCK),
                    );
                }
                ui.add_space(6.0);
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
        ui_lock: bool,
        room: Option<(String, Vec<String>)>,
    ) {
        // THE BRUSH LIBRARY (PSD-brush-library): the rail raised the
        // flag; the import runs HERE, where presets are mutable, and
        // never mid-stroke (NEVER-DO 3).
        if self.canvas.request_brush_import && !self.canvas.stroke_active() {
            self.canvas.request_brush_import = false;
            if let Some(paths) = rfd::FileDialog::new()
                .add_filter(
                    "brush files",
                    &["kpp", "bundle", "abr", "brush", "brushset", "gbr", "gih", "png"],
                )
                .pick_files()
            {
                let r = crate::brushbank::import_any(&paths, presets);
                let purged = crate::brushbank::purge_unreal(presets);
                if r.ok > 0 || purged > 0 {
                    *presets_dirty = true;
                }
                if r.ok == 0 && r.failed > 0 {
                    self.state.refuse(format!(
                        "refused — none of those files imported ({} unreadable)",
                        r.failed
                    ));
                } else {
                    self.state.status = format!(
                        "imported {} brush(es) · {} duplicate(s) · {} failed",
                        r.ok, r.dup, r.failed
                    );
                }
            }
        }
        if self.canvas.request_krita_scan && !self.canvas.stroke_active() {
            self.canvas.request_krita_scan = false;
            let paths = crate::kpp::installed_krita_paths();
            let cached = crate::kpp::cache_krita_resource_dirs();
            let _ = cached;
            let before: std::collections::HashSet<String> =
                presets.iter().map(|p| p.name.clone()).collect();
            let (ok, dup, failed) = crate::kpp::import_files(&paths, presets);
            for p in presets.iter_mut() {
                if !before.contains(&p.name) && p.bank.is_empty() {
                    p.bank = "krita".into();
                }
            }
            let purged = crate::brushbank::purge_unreal(presets);
            if ok > 0 || purged > 0 {
                *presets_dirty = true;
            }
            if ok == 0 && dup == 0 {
                self.state.refuse(format!(
                    "refused — no Krita brushes found on this machine ({failed} unreadable)"
                ));
            } else {
                self.state.status = format!(
                    "imported {ok} Krita brush(es) · {dup} skipped · {failed} unreadable · {purged} unreal removed"
                );
            }
        }
        self.poll_export();
        self.export_progress_window(ui.ctx());
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        self.state.tick(dt);
        self.sync_audio(dt);
        egui::Panel::top("top_bar").show(ui, |ui| {
            plate::surface(ui);
            // Owner's directive (2026-08-17): the bar read small. One size
            // class up, scoped to this panel only — 13pt controls, taller
            // hit targets, wider breathing room. The global scale is law
            // elsewhere; the bar is the one surface read from arm's length.
            {
                use egui::{FontFamily, FontId, TextStyle};
                let st = ui.style_mut();
                st.text_styles
                    .insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
                st.text_styles.insert(
                    TextStyle::Button,
                    FontId::new(13.0, FontFamily::Proportional),
                );
                st.text_styles.insert(
                    TextStyle::Monospace,
                    FontId::new(12.5, FontFamily::Monospace),
                );
                st.spacing.button_padding = egui::vec2(8.0, 4.0);
                st.spacing.interact_size.y = 24.0;
                st.spacing.item_spacing.x = 8.0;
                st.spacing.icon_width = 18.0;
            }
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                // THE SLATE (spec §4.1): file commands live in ONE
                // detent; the wordmark is deleted — the operator knows
                // what he opened.
                ui.menu_button("file", |ui| {
                    if ui.button("New…").clicked() {
                        self.request_new = true;
                        ui.close();
                    }
                    if ui.button("Open…").clicked() {
                        // Land any in-flight gesture in the CURRENT project first
                        // — its commit must not cross into the opened one.
                        self.canvas.finish_gesture(&mut self.state);
                        self.state.open();
                        ui.close();
                    }
                    if ui.button("Save").clicked() {
                        // The saved file must contain what the screen shows.
                        self.canvas.finish_gesture(&mut self.state);
                        self.state.save(false);
                        ui.close();
                    }
                    ui.menu_button("Export…", |ui| {
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
                            if ui
                                .add(egui::DragValue::new(&mut a1).range(1..=b1))
                                .changed()
                            {
                                a = a1 - 1;
                            }
                            ui.label("–");
                            if ui
                                .add(egui::DragValue::new(&mut b1).range(a1..=n))
                                .changed()
                            {
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
                                let cancel =
                                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
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
                                let cancel =
                                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
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
                    // Scratch audio (moved from the x-sheet header — the sheet
                    // is for time, the slate detent is for the file's parts).
                    match self.state.cut().audio.as_ref().map(|a| a.name.clone()) {
                        Some(name) => {
                            if ui.button(format!("Remove audio ({name})")).clicked() {
                                self.state.remove_audio();
                                ui.close();
                            }
                        }
                        None => {
                            if ui.button("Import WAV…").clicked() {
                                ui.close();
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("WAV audio", &["wav"])
                                    .pick_file()
                                {
                                    self.state.import_audio(&path);
                                }
                            }
                        }
                    }
                    if ui
                        .button("Settings…")
                        .on_hover_text("keyboard shortcuts & config")
                        .clicked()
                    {
                        self.request_settings = true;
                        ui.close();
                    }
                });

                // Slate identity: filename + dirty mark, then the cut's
                // vitals as engraved fields (transport lives in the FOOT;
                // undo/redo live on the keys and the Foot's counter).
                let fname = self
                    .state
                    .file_path
                    .as_ref()
                    .and_then(|p| p.file_stem().map(|os| os.to_string_lossy().to_string()))
                    .unwrap_or_else(|| "untitled".into());
                // AUDIT [3]: these were variable-width labels sitting
                // BEFORE the room tabs, so the tabs slid sideways the
                // instant the dirty mark appeared — on the first stroke
                // of every session. Both fields are now fixed slots with
                // elision inside, and the dirty mark is a painted Tally
                // dot in permanently reserved space (a character would
                // change the string's width).
                let dirty = self.state.engine.can_undo();
                let (frect, _) =
                    ui.allocate_exact_size(egui::vec2(150.0, 20.0), egui::Sense::hover());
                {
                    let p = ui.painter();
                    let mut shown = fname.clone();
                    if shown.chars().count() > 16 {
                        shown = shown.chars().take(15).collect::<String>() + "…";
                    }
                    p.text(
                        egui::pos2(frect.left(), frect.center().y),
                        egui::Align2::LEFT_CENTER,
                        shown,
                        egui::FontId::new(14.0, egui::FontFamily::Proportional),
                        plate::STRUCK,
                    );
                    if dirty {
                        p.circle_filled(
                            egui::pos2(frect.right() - 6.0, frect.center().y),
                            3.0,
                            plate::TALLY,
                        );
                    }
                }
                let (vrect, _) =
                    ui.allocate_exact_size(egui::vec2(200.0, 20.0), egui::Sense::hover());
                ui.painter().text(
                    egui::pos2(vrect.left(), vrect.center().y),
                    egui::Align2::LEFT_CENTER,
                    format!(
                        "{} · {}f · {:.3} · {}×{}",
                        self.state.cut().name.chars().take(8).collect::<String>(),
                        self.state.frame_count(),
                        self.state.fps() as f32,
                        self.state.engine.project.width,
                        self.state.engine.project.height,
                    ),
                    egui::FontId::new(11.5, egui::FontFamily::Monospace),
                    plate::LEGEND,
                );
                ui.separator();
                // (Onion + line-only moved to the canvas lightbox rail —
                // Phase 4: onion is a property of the light box.)
                // THE STAGE SPINE (workflow trees, LENS-DOCK Phase 3): the
                // four fixed pipeline rooms, in order. Switching re-lenses
                // the SAME document — layout + tool/view state swap, data
                // never forks. Tool/view restore is all-or-nothing; a live
                // stroke keeps the old tool state (same law as saved
                // workspaces below).
                for st in Stage::ALL {
                    let (kanji, romaji) = match st {
                        Stage::Layout => ("レイアウト", "layout"),
                        Stage::Drawing => ("原画", "genga"),
                        Stage::Finishing => ("仕上げ", "shiage"),
                        Stage::Edit => ("編集", "henshū"),
                    };
                    if slate_tab(ui, self.stage == Some(*st), kanji, romaji)
                        .on_hover_text(st.describes())
                        .clicked()
                    {
                        self.dock = st.dock();
                        if self.canvas.apply_view(&st.view(), &mut self.state) {
                            self.stage = Some(*st);
                            // LINE GUARD defaults ON in the Finishing room
                            // (shiage charter) and OFF elsewhere.
                            self.state.view.line_guard = matches!(st, Stage::Finishing);
                            self.state.status = format!("room: {}", st.name());
                        }
                    }
                }
                ui.menu_button("ws", |ui| {
                    ui.label(egui::RichText::new("save current arrangement as:").color(plate::legend_dim()));
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
                    // THE FORGE ROOM (PSD-brush-forge amendment): built in,
                    // above the saved rooms — one click arranges the whole
                    // window for brush design.
                    if ui
                        .button("Brush Forge room")
                        .on_hover_text(
                            "the brush-design arrangement — forge tall on the                              left, quick list and palette under it, the canvas                              as test paper",
                        )
                        .clicked()
                    {
                        if self.canvas.stroke_active() {
                            self.state.refuse("refused — finish the stroke first");
                        } else {
                            self.dock = Pane::forge_dock();
                            self.state.status = "the forge room".into();
                        }
                        ui.close();
                    }
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
                            // AUDIT [11]: this was the smallest target in
                            // the row, one item from "apply". Destruction
                            // must be findable by feel, not by precision.
                            if plate::danger(ui, "DELETE ROOM") {
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
                ui.menu_button("panes", |ui| {
                    for pane in Pane::ALL {
                        // The Brush window is RETIRED (owner 2026-08-17):
                        // its settings live on the canvas deck + right rail.
                        if *pane == Pane::Brush {
                            continue;
                        }
                        let present = self.dock.iter_all_tabs().any(|(_, t)| t == pane);
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
                        .add(|ui: &mut egui::Ui| {
                            plate::latch(ui, &mut floating, "viewer in an OS window")
                        })
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
                        .add(|ui: &mut egui::Ui| {
                            plate::latch(ui, &mut floating, "canvas in an OS window")
                        })
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
                ui.menu_button("cut", |ui| {
                    ui.label(
                        egui::RichText::new(format!("now editing: {}", self.state.cut().name))
                            .color(plate::legend_dim()),
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
                            .button("export XDTS…")
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
                            .button("import XDTS…")
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
            });
            // Row 2 — THE ARMING LINE (spec §4.2): what the pen will do
            // RIGHT NOW: drawing · column · layer · pencil.
            ui.horizontal(|ui| {
                plate::legend(ui, "armed");
                let dname = {
                    let cut = self.state.cut();
                    self.state
                        .current_drawing()
                        .and_then(|id| cut.drawing(id))
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| "—".into())
                };
                let col = {
                    let cut = self.state.cut();
                    cut.xsheet
                        .columns
                        .iter()
                        .find(|c| c.id == self.state.view.active_column)
                        .map(|c| c.name.clone())
                        .unwrap_or_default()
                };
                // With fill armed, the pot's NAME rides the arming line —
                // kills the "what colour am I holding" saccade (shiage).
                let mut arming = format!(
                    "{dname} · col {col} · {} · {}",
                    self.state.active_layer_name(),
                    self.canvas.arming_pencil(),
                );
                if self.canvas.arming_pencil() == "fill" {
                    for ch in &self.state.palettes.characters {
                        for r in &ch.roles {
                            if r.color[..3] == self.canvas.brush_color[..3] {
                                arming.push_str(&format!(" · {}", r.name));
                            }
                        }
                    }
                }
                ui.label(
                    egui::RichText::new(arming)
                        .monospace()
                        .size(11.5)
                        .color(plate::STRUCK),
                );
            });
            ui.add_space(2.0);
        });

        // THE FOOT (spec §4.1): transport is GLOBAL state, bolted to the
        // chassis — a tape deck's motor is never on a removable reel.
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            plate::surface(ui);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.foot_right(ui, &room);
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let gesture = self.canvas.stroke_active();
                        if plate::icon_button(ui, Icon::SkipBack, "", "first frame (Home)")
                            .clicked()
                            && !gesture
                        {
                            self.state.goto(0);
                        }
                        if plate::icon_button(ui, Icon::StepBack, "", "previous frame (S)")
                            .clicked()
                            && !gesture
                        {
                            self.state.step(-1);
                        }
                        let (pi, ph) = if self.state.view.playing {
                            (Icon::Pause, "stop (Space)")
                        } else {
                            (Icon::Play, "play (Space)")
                        };
                        if plate::icon_button(ui, pi, "", ph).clicked() {
                            self.state.toggle_play();
                        }
                        if plate::icon_button(ui, Icon::StepFwd, "", "next frame (F)").clicked()
                            && !gesture
                        {
                            self.state.step(1);
                        }
                        if plate::icon_button(ui, Icon::SkipFwd, "", "last frame (End)").clicked()
                            && !gesture
                        {
                            let last = self.state.frame_count() - 1;
                            self.state.goto(last);
                        }
                        // The one number an animator most wants to type into
                        // stops being the one number that cannot be typed into.
                        let count = self.state.frame_count();
                        let mut f1 = self.state.view.frame + 1;
                        if ui
                            .add(egui::DragValue::new(&mut f1).range(1..=count))
                            .on_hover_text("current frame — type or drag")
                            .changed()
                            && !gesture
                        {
                            self.state.goto(f1 - 1);
                        }
                        ui.label(
                            egui::RichText::new(format!("/ {count}"))
                                .monospace()
                                .color(plate::LEGEND),
                        );
                        let mut lp = self.state.view.loop_playback;
                        plate::latch(ui, &mut lp, "loop")
                            .on_hover_text("off = playback stops on the last frame");
                        self.state.view.loop_playback = lp;
                        ui.label(
                            egui::RichText::new(format!("{:.3}", self.state.fps() as f32))
                                .monospace()
                                .size(11.0)
                                .color(plate::LEGEND),
                        );
                        ui.separator();
                        // REFUSAL lane (Aka): the machine overruled the hand.
                        // Dwells 4s; a repeat of the same refusal re-flashes.
                        if self.state.refusal_seq != self.refusal_seen_seq {
                            self.refusal_seen_seq = self.state.refusal_seq;
                            self.refusal_since = ui.input(|i| i.time);
                        }
                        let rage = ui.input(|i| i.time) - self.refusal_since;
                        if rage < 4.0 && !self.state.refusal.is_empty() {
                            ui.label(egui::RichText::new(&self.state.refusal).color(plate::AKA));
                            ui.separator();
                        }
                        // CHATTER lane: the machine's last remark, decaying — a
                        // stale "painted" must never train the eye to ignore this.
                        if self.state.status != self.status_seen {
                            self.status_seen = self.state.status.clone();
                            self.status_since = ui.input(|i| i.time);
                        }
                        let age = ui.input(|i| i.time) - self.status_since;
                        if age < 4.0 && !self.state.status.is_empty() {
                            let fade = (1.0 - ((age - 3.0).max(0.0)) as f32).clamp(0.0, 1.0);
                            ui.label(
                                egui::RichText::new(&self.state.status)
                                    .color(plate::LEGEND.gamma_multiply(fade)),
                            );
                        }
                    });
                });
            });
            ui.add_space(2.0);
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
        // The Brush pane is retired (owner 2026-08-17): any copy still in
        // a saved arrangement is removed on sight — the deck + rail carry
        // the brush now.
        while let Some(loc) = self.dock.find_tab(&Pane::Brush) {
            self.dock.remove_tab(loc);
        }
        let visible = visible_panes(&self.dock);
        let viewers_open = visible.iter().any(|t| matches!(t, Pane::Viewer(_)))
            // The Layout room's canvas renders the preview, so it consumes
            // the graph exactly like a viewer pane.
            ||
            (
            self.stage == Some(Stage::Layout) && visible.contains(&Pane::Canvas));
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
                let (scene, cutid, frame) = (
                    self.state.view.scene,
                    self.state.view.cut,
                    self.state.view.frame,
                );
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
            stage: self.stage,
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
            forge: &mut self.forge,
            native_pen,
            float_canvas_open: self.float_canvas.open,
        };
        let mut dock_style = egui_dock::Style::from_egui(ui.style().as_ref());
        // egui_dock clamps every divider so each side keeps `separator.extra`
        // pixels — the DEFAULT IS 175px, which silently blocked collapsing
        // panes (e.g. the Brush band above the canvas). 26px keeps the tab bar
        // grabbable while letting panes shrink to slim rails.
        dock_style.separator.extra = 26.0;
        // UI lock (Settings -> UI Features): the layout freezes — no tab
        // dragging, no splits, no closing — but the arrangement is kept.
        DockArea::new(&mut self.dock)
            .style(dock_style)
            .draggable_tabs(!ui_lock)
            .show_close_buttons(!ui_lock)
            .allowed_splits(if ui_lock {
                egui_dock::AllowedSplits::None
            } else {
                egui_dock::AllowedSplits::All
            })
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
                            presets,
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
