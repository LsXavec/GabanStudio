//! AnimStudio — app shell (M2: X-sheet + drawing loop; + New Project flow).
//!
//! Two screens: a startup New Project dialog (sets resolution/fps), then the
//! editor (top bar, left X-sheet, central canvas, status bar).

// Release builds are GUI-subsystem: no console window behind the app (GM1).
// CLI paths (the layout probe) re-attach to the launching terminal.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod canvas;
mod cmdpal;
mod config;
mod devloop;
mod doc;
mod export;
mod floatcanvas;
mod forge;
mod floatwin;
mod fxui;
mod graphcomp;
mod curves_panel;
mod history_panel;
mod icons;
mod kpp;
mod brushbank;
mod kritares;
mod menus;
mod net;
mod newproject;
mod nodegraph_panel;
mod paint;
mod paintcheck;
mod palette;
mod palette_panel;
mod plate;
mod proc;
mod render_queue;
mod runs;
mod sample;
mod shortcuts;
mod stabilizer;
mod templates;
#[cfg(test)]
mod testkit;
mod uidump;
mod update;
mod viewer;
mod welcome;
mod workspace;
mod worksafe;
mod xsheet_panel;

use anim_core::color::ColorLaw;
use anim_core::xsheet::ColumnKind;
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

/// A peer as the room lamp's hover names them — with their build, so
/// two machines cross-check without leaving the canvas (owner
/// 2026-08-19: "so i can cross verify that hes on the latest").
/// PSD-cel-layers Stage 4: the key a room's stack binding lives under
/// (off-spine custom rooms share one binding).
fn stage_key(stage: Option<Stage>) -> &'static str {
    match stage {
        Some(Stage::Layout) => "layout",
        Some(Stage::Drawing) => "drawing",
        // GM8: the ink & paint room keeps its old key (saved bindings).
        Some(Stage::InkPaint) => "finishing",
        Some(Stage::Composite) => "composite",
        Some(Stage::Edit) => "edit",
        None => "custom",
    }
}

fn peer_tag(p: &net::PeerView) -> String {
    if p.version.is_empty() {
        p.name.clone()
    } else {
        format!("{} · v{}", p.name, p.version)
    }
}

fn main() -> eframe::Result<()> {
    // Headless layout probe: runs the REAL panel/canvas layout on the CPU and
    // prints the canvas rect per scripted step, so any drawing-area movement
    // is a measurable number instead of an on-rig observation.
    if std::env::var_os("ANIMSTUDIO_PROBE").is_some() {
        attach_console();
        probe();
        return Ok(());
    }
    // GM11: the batch render (`--render <file | --sample> --preset <name>
    // --out <dir>`) runs one job and exits — no window, no dev loop.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if anim_core::deliver::is_cli(&args) {
        attach_console();
        std::process::exit(cli_exit(&args, config::data_dir().as_deref()));
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
    // THE real data dir (D11) is resolved here and handed down; everything
    // below takes a dir. Config up front so the surface (V-Sync / frame
    // latency) reflects it.
    let data_dir = config::data_dir();
    // GM2: a panic leaves crash.log (and its running record) in the data
    // folder.
    if let Some(dir) = &data_dir {
        worksafe::install_panic_hook(dir.clone());
    }
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
            .with_title("Gaban Studio"),
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            surface,
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "Gaban Studio",
        native_options,
        Box::new(move |cc| {
            dress(&cc.egui_ctx);
            Ok(Box::new(App::new(
                cc,
                config,
                staging_failed.clone(),
                data_dir.clone(),
            )))
        }),
    )
}

/// The plate visuals and the Plex type stack — the app, the layout probe
/// and the testkit all draw with the same dressing.
fn dress(ctx: &egui::Context) {
    ctx.set_visuals(plate::visuals());
    plate::install_fonts(ctx);
    // GM4: Ctrl+= / Ctrl+- / Ctrl+0 are the canvas's zoom verbs (D6
    // Actions); egui's own UI zoom would swallow those keys first.
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
}

/// A GUI-subsystem build has no console: re-attach to the launching
/// terminal so CLI output (the probe) is seen. No terminal = no-op.
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: AttachConsole takes a process id and touches no Rust state.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}
#[cfg(not(windows))]
fn attach_console() {}

/// GM11: the batch render's exit code — 0 rendered, 1 the job failed, 2
/// bad arguments (the usage printed).
fn cli_exit(args: &[String], data_dir: Option<&std::path::Path>) -> i32 {
    use anim_core::deliver::{CLI_USAGE, EXIT_FAILED, EXIT_OK, EXIT_USAGE};
    let job = match anim_core::deliver::parse_cli(args) {
        Ok(job) => job,
        Err(e) => {
            eprintln!("animstudio: {e}\n{CLI_USAGE}");
            return EXIT_USAGE;
        }
    };
    match run_cli_job(&job, data_dir) {
        Ok(n) => {
            println!("rendered {n} frame(s) into {}", job.out.display());
            EXIT_OK
        }
        Err(e) => {
            eprintln!("animstudio: the render failed — {e}");
            EXIT_FAILED
        }
    }
}

/// GM11: one batch job, synchronously, one line per frame: the file's (or
/// the in-memory sample's) first cut under the preset into `job.out`. The
/// data dir's config gives the ffmpeg location and the artist; None = the
/// defaults.
fn run_cli_job(
    job: &anim_core::deliver::CliJob,
    data_dir: Option<&std::path::Path>,
) -> Result<usize, String> {
    let project = match &job.source {
        anim_core::deliver::CliSource::File(path) => {
            anim_core::store::load(path).map_err(|e| format!("open {}: {e}", path.display()))?
        }
        anim_core::deliver::CliSource::Sample => sample::build_sample().engine.project.clone(),
    };
    let config = data_dir.map(|d| Config::load_from(d).0).unwrap_or_default();
    let scene = project.scenes.first().ok_or("the project has no scene")?;
    let cut = scene.cuts.first().ok_or("the project has no cut")?;
    let at = anim_core::command::CutRef {
        scene: scene.id,
        cut: cut.id,
    };
    let spec = render_queue::JobSpec::new(
        &project,
        at,
        None,
        job.preset,
        job.preset.burn_ins,
        job.out.clone(),
        anim_core::deliver::DEFAULT_PATTERN.into(),
        config.session.username.clone(),
    )
    .ok_or("the cut is gone")?;
    let typed = config.render.ffmpeg.trim();
    let configured = (!typed.is_empty()).then(|| std::path::PathBuf::from(typed));
    let ffmpeg = export::find_ffmpeg(configured.as_deref());
    let total = spec.frames.1 - spec.frames.0 + 1;
    let never = std::sync::atomic::AtomicBool::new(false);
    render_queue::render_job(&spec, ffmpeg.as_deref(), &never, &mut |f| {
        println!("frame {}/{total}", f - spec.frames.0 + 1);
    })
}

/// A slate room tab (spec §4.2): kanji over romaji caps, framed, all four
/// identical in structure; the active room is lit by the Tally lamp. `name`
/// is the room's house label (romaji + English, D7) — what it answers to.
fn slate_tab(
    ui: &mut egui::Ui,
    active: bool,
    kanji: &str,
    romaji: &str,
    name: &str,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(84.0, 34.0), egui::Sense::click());
    // The painted tab still answers to a name (AccessKit, D7).
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            active,
            format!("room {name}"),
        )
    });
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
    /// The data dir main() resolved (D11); editors persist into it.
    data_dir: Option<std::path::PathBuf>,
    /// The open project (None until one is created/opened).
    editor: Option<Editor>,
    /// When Some, the New Project dialog is showing (startup, or "New…").
    new_form: Option<NewProjectForm>,
    /// GM8: when Some (and nothing is open), the Welcome screen shows.
    welcome: Option<welcome::Welcome>,
    /// User config (rebindable keyboard shortcuts + performance).
    config: Config,
    settings_open: bool,
    settings_category: SettingsCategory,
    /// PSD-shipping: the published-build channel.
    update_rx: Option<std::sync::mpsc::Receiver<crate::update::UpdateEvent>>,
    update_ready: Option<(String, String, u64)>,
    update_swap: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    /// AUTO-UPDATE: the fully-downloaded build waiting beside the exe.
    /// Lamp click = instant relaunch; normal close = silent swap.
    update_staged: Option<std::path::PathBuf>,
    update_next_check: f64,
    update_note: String,
    /// SESSION PERF: last snapshot / presence send times (throttles).
    session_last_snap: f64,
    session_last_presence: f64,
    /// SESSION PERF 4: when the doc generation last MOVED — syncs wait
    /// for a quiet moment (guests watch strokes live via presence; the
    /// committed truth lands when the host pauses).
    frame_watch: Option<std::time::Instant>,
    /// STAGE 0: guest→host log-shipping timer, and the STATS heartbeat
    /// window (start time, frames counted, worst frame gap ms).
    dbg_ship_last: f64,
    stats_window: (f64, u32, f32),
    /// STAGE 1: the one global order. Host: last assigned. Guest: last
    /// applied; the snapshot's meta tells where to resume.
    session_seq: u64,
    session_applied_seq: u64,
    session_snapshot_seq: Option<u64>,
    session_snap_seq: u64,
    /// Guest: sequenced arrivals before the snapshot lands buffer here.
    session_buffer: Vec<SeqEv>,
    /// In-flight remote strokes + the session's content-addressed
    /// tip/grain images + which images THIS machine already announced.
    session_gather: std::collections::HashMap<u64, GatherStroke>,
    session_res: std::collections::HashMap<u64, (u32, u32, std::sync::Arc<Vec<u8>>)>,
    session_res_announced: std::collections::HashSet<u64>,
    /// Audit bookkeeping: hashes waiting for our replay to land, and
    /// replays (with THEIR changed coords) waiting for their hashes.
    session_audit: std::collections::HashMap<u64, net::StrokeHashesWire>,
    session_done: std::collections::HashMap<u64, Vec<(i32, i32)>>,
    /// STAGE 3: the engine's base author is armed with this artist's
    /// name while in a room (every entry carries its hand).
    session_author_armed: bool,
    /// STAGE 4: the host's arrangement (serialized Workspace), loadable
    /// from Settings ▸ Layout while in this session; and the preset
    /// name field's buffer.
    session_host_layout: Option<String>,
    layout_ws_name: String,
    /// v0.2.2: snapshots are REQUESTER-ONLY — who wants one (joiners +
    /// resync requesters), who the in-flight build is for, and the
    /// seq-assigning arrivals HELD while it builds (so a snapshot can
    /// never arrive stale — the first live test's livelock).
    session_snap_targets: std::collections::HashSet<u64>,
    session_snap_pending: Vec<u64>,
    session_ev_hold: Vec<net::NetEvent>,
    /// v0.2.5 (the shared pencil box): hosting — the serialized bank of
    /// this machine's usable brushes; joined — the host's presets,
    /// usable alongside our own (session-scoped, never saved).
    session_bank_json: Option<String>,
    session_presets: Vec<config::BrushPreset>,
    /// PSD-cel-layers: the room whose stack last armed the pen (a room
    /// change re-arms its stack's working layer once).
    stack_room_seen: &'static str,
    /// LAN DISCOVERY: rooms visible on this network (always listening).
    discovery: net::Discovery,
    /// SESSION PERF 2: the host's snapshot builder, off the UI thread
    /// (one in flight; the UI only forwards finished bytes).
    session_snap_rx: Option<std::sync::mpsc::Receiver<Option<Vec<u8>>>>,
    /// The guest's snapshot parser, off the UI thread (latest wins).
    session_load_rx: Option<std::sync::mpsc::Receiver<Result<doc::AppState, String>>>,
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
    /// What the artist must hear once (load resets, the tablet fallback),
    /// said on the first editor that can show it.
    notes: Notes,
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
    /// GM2: the start-up crash check ran (first frame).
    worksafe_started: bool,
    /// GM2: what this copy keeps in its own `<data>/running-<pid>-<t>.json`
    /// (removed on a clean exit and before a handover; left by a crash).
    running: worksafe::RunningRecord,
    /// GM2: crashed copies' offers, asked one at a time (the first). An
    /// unanswered or "Later" offer keeps its record for the next start.
    recovery: Vec<worksafe::Recovery>,
    /// GM2: (now, UTC offset) for the recovery rows, read at start-up.
    recovery_clock: (u64, i32),
    /// GM2: the document path last seen (the recent list follows it).
    doc_seen: Option<std::path::PathBuf>,
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

/// STAGE 1 (PSD-multiplayer-rescope): a remote stroke being gathered
/// off the wire — Begin's facts + the dab batches as they stream in.
struct GatherStroke {
    author: String,
    drawing: u64,
    layer_name: String,
    mode: u8,
    opacity: f32,
    tip: Option<(u64, u32, u32, u32)>,
    grain: Option<(u64, u32, u32, f32, f32)>,
    dabs: Vec<crate::paint::Dab>,
}

/// v0.2.5 (the shared pencil box): the bank a guest's presets arrive
/// under — filtered out of every config save, cleared with the room.
const SESSION_BANK: &str = "host session";

/// v0.2.5: one host preset on the wire — engine keys rewritten to
/// content hashes whose images already rode the session res cache.
#[derive(serde::Serialize, serde::Deserialize)]
struct BankEntry {
    preset: config::BrushPreset,
    /// (tip image hash, GIH frame count)
    tip: Option<(u64, u32)>,
    grain: Option<u64>,
}

/// STAGE 1: one sequenced arrival on the guest lane (applied in host
/// order; buffered until the join snapshot lands).
enum SeqEv {
    End(net::StrokeEndWire),
    Cmds(net::CmdBatchWire),
    Hashes(net::StrokeHashesWire),
    /// STAGE 3: a per-artist undo/redo, replayed on identical history.
    Undo { author: String, redo: bool, seq: u64 },
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
                    format!("{}'s room", cfg.username.trim()),
                    !cfg.require_key,
                    cfg.username.trim().to_string(),
                ) {
                    Ok(h) => {
                        self.session_status = format!("hosting on port {} — share the key", h.port);
                        self.session = net::Session::Hosting(h);
                        self.session_sent_gen = u64::MAX;
                        self.session_lane_reset();
                        // v0.2.5: the shared pencil box — built once,
                        // every joiner receives it.
                        self.build_session_bank();
                        // STAGE 0: a fresh room starts a fresh remote log —
                        // guests' numbered lines land here, in order.
                        if let Some(base) = std::env::var_os("APPDATA") {
                            let dir = std::path::PathBuf::from(base).join("AnimStudio");
                            let _ = std::fs::create_dir_all(&dir);
                            let _ = std::fs::write(
                                dir.join("session_log_remote.txt"),
                                format!(
                                    "remote session log · host build v{}\n",
                                    crate::update::CURRENT_VERSION
                                ),
                            );
                        }
                    }
                    Err(e) => self.session_status = format!("could not host: {e}"),
                }
            }
            config::SessionAction::StopHost | config::SessionAction::Leave => {
                self.session = net::Session::Idle;
                self.session_peers.clear();
                self.session_status = "offline".into();
                net::slog_ship_arm(false);
                self.session_lane_reset();
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
                let host_ver = c.host_version.clone();
                self.session = net::Session::Joined(c);
                self.session_peers.clear();
                self.session_lane_reset();
                // v0.2.4: a newer host updates this build RIGHT AWAY.
                self.version_gap_check(&host_ver, "the host");
                // STAGE 0: from here on, this machine's numbered log lines
                // ship to the host every ~2s.
                net::slog_ship_arm(true);
                net::slog("debug channel armed — lines ship to the host");
            }
            Err(e) => self.connect_error = e,
        }
    }

    /// Once per frame: drain net events, apply snapshots, push presence.
    /// PSD-shipping: read the updater threads' channels; light the
    /// lamp; when a swap lands, relaunch through the devloop's proven
    /// session-carry (NEVER-DO 2).
    fn update_pump(&mut self, _ctx: &egui::Context) {
        if let Some(rx) = &self.update_rx
            && let Ok(ev) = rx.try_recv()
        {
            self.update_rx = None;
            match ev {
                crate::update::UpdateEvent::Ready { tag, url, size } => {
                    // AUTO-UPDATE: stage immediately in the background —
                    // the lamp click becomes instant, and a normal close
                    // installs it silently.
                    self.update_note = format!("update {tag} downloading…");
                    if self.update_swap.is_none() && self.update_staged.is_none() {
                        self.update_swap =
                            Some(crate::update::spawn_stage(url.clone(), size));
                    }
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
                Ok(staged) => {
                    // Downloaded, verified, waiting. The Foot lamp offers
                    // the instant relaunch; closing installs it silently.
                    let tag = self
                        .update_ready
                        .as_ref()
                        .map(|(t, _, _)| t.clone())
                        .unwrap_or_default();
                    self.update_note = format!("update {tag} ready");
                    self.update_staged = Some(staged);
                }
                Err(why) => {
                    self.update_note = format!("update failed — {why}");
                }
            }
        }
    }

    /// GM2: a handover exits without `on_exit`, so this copy's running
    /// record goes first — left behind, the next start would offer the
    /// carried-over work as a crash.
    fn retire_running(&self) {
        if let (Some(dir), true) = (&self.data_dir, self.worksafe_started) {
            self.running.remove_in(dir);
        }
    }

    /// GM2: the handover failed and this copy runs on — its record again.
    fn rearm_running(&self) {
        if let (Some(dir), true) = (&self.data_dir, self.worksafe_started) {
            let _ = self.running.write_in(dir);
        }
    }

    /// The handover into the freshly seated build: autosave + session
    /// (the devloop's own machinery), spawn with RESUME armed, exit.
    fn relaunch_into(&mut self, new_exe: std::path::PathBuf, ctx: &egui::Context) {
        let Some(editor) = &mut self.editor else {
            // No document open — nothing to carry; just hand over.
            let _ = proc::command(&new_exe)
                .env("ANIMSTUDIO_RESUME", "1")
                .spawn();
            self.retire_running();
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
        match proc::command(&new_exe)
            .env("ANIMSTUDIO_RESUME", "1")
            .spawn()
        {
            Ok(_) => {
                self.retire_running();
                std::process::exit(0)
            }
            Err(e) => {
                self.update_note = format!("could not start the new build ({e})");
            }
        }
    }

    /// STAGE 1: forget everything session-scoped (order lane, gathers,
    /// resource cache). Runs at every room start/join/end.
    fn session_lane_reset(&mut self) {
        self.session_seq = 0;
        self.session_applied_seq = 0;
        self.session_snapshot_seq = None;
        self.session_snap_seq = 0;
        self.session_buffer.clear();
        self.session_gather.clear();
        self.session_res.clear();
        self.session_res_announced.clear();
        self.session_audit.clear();
        self.session_done.clear();
        self.session_author_armed = false;
        self.session_host_layout = None;
        self.session_snap_targets.clear();
        self.session_snap_pending.clear();
        self.session_ev_hold.clear();
        self.session_bank_json = None;
        self.session_presets.clear();
        // v0.2.1 (audit): stale cross-room channels must die with the
        // room — a late snapshot from an OLD room must never replace
        // this document, and guest_ready must never survive into the
        // next join (it bypassed the buffer-until-snapshot law).
        self.session_load_rx = None;
        self.session_snap_rx = None;
        self.session_snapshot_seq = None;
        if let Some(ed) = &mut self.editor {
            ed.canvas.guest_ready = false;
            ed.canvas.replay_inbox.clear();
            ed.canvas.replay_done.clear();
            ed.canvas.stroke_outbox.clear();
            ed.canvas.engine_changed();
        }
    }

    /// Guest side: one stroke frame off the host's broadcast.
    fn guest_stroke_frame(&mut self, frame: net::StrokeFrame) {
        match frame {
            net::StrokeFrame::Res { hash, w, h, rgba } => {
                self.session_res
                    .insert(hash, (w, h, std::sync::Arc::new(rgba)));
            }
            net::StrokeFrame::Begin(b) => {
                self.session_gather.insert(
                    b.stroke_id,
                    GatherStroke {
                        author: b.author,
                        drawing: b.drawing,
                        layer_name: b.layer_name,
                        mode: b.mode,
                        opacity: b.opacity,
                        tip: b.tip,
                        grain: b.grain,
                        dabs: Vec::new(),
                    },
                );
            }
            net::StrokeFrame::Dabs { stroke_id, dabs } => {
                if let Some(g) = self.session_gather.get_mut(&stroke_id) {
                    g.dabs.extend(dabs.iter().copied());
                    // STAGE 2: ink strokes preview LIVE in the overlay
                    // (erase/alpha-lock appear at commit).
                    if g.mode == 0
                        && let Some(ed) = &mut self.editor
                    {
                        ed.canvas
                            .remote_wet_inbox
                            .push((stroke_id, g.opacity, dabs));
                    }
                }
            }
            net::StrokeFrame::End(e) => self.seq_event(SeqEv::End(e)),
            net::StrokeFrame::Hashes(hw) => self.seq_event(SeqEv::Hashes(hw)),
            // v0.2.2: binary repair — same application as the legacy
            // JSON path, ~10-20x lighter on the wire.
            net::StrokeFrame::Repair {
                drawing,
                layer_name,
                tiles,
            } => {
                net::slog(format!(
                    "REPAIR apply '{layer_name}': {} tile(s)",
                    tiles.len()
                ));
                if let Some(ed) = &mut self.editor {
                    ed.state.apply_repair_tiles(
                        anim_core::ids::DrawingId(drawing),
                        &layer_name,
                        tiles,
                    );
                    ed.canvas.engine_changed();
                }
            }
        }
    }

    /// Host side: one stroke frame from a guest's pen.
    fn host_stroke_frame(&mut self, _client: u64, author: String, frame: net::StrokeFrame) {
        match frame {
            net::StrokeFrame::Res { hash, w, h, rgba } => {
                self.session_res
                    .insert(hash, (w, h, std::sync::Arc::new(rgba)));
            }
            net::StrokeFrame::Begin(b) => {
                self.session_gather.insert(
                    b.stroke_id,
                    GatherStroke {
                        author,
                        drawing: b.drawing,
                        layer_name: b.layer_name,
                        mode: b.mode,
                        opacity: b.opacity,
                        tip: b.tip,
                        grain: b.grain,
                        dabs: Vec::new(),
                    },
                );
            }
            net::StrokeFrame::Dabs { stroke_id, dabs } => {
                if let Some(g) = self.session_gather.get_mut(&stroke_id) {
                    g.dabs.extend(dabs.iter().copied());
                    // STAGE 2: ink strokes preview LIVE in the overlay
                    // (erase/alpha-lock appear at commit).
                    if g.mode == 0
                        && let Some(ed) = &mut self.editor
                    {
                        ed.canvas
                            .remote_wet_inbox
                            .push((stroke_id, g.opacity, dabs));
                    }
                }
            }
            net::StrokeFrame::End(e) => {
                // v0.2.1 (audit): a guest's ABORT — drop its gather and
                // overlay, sequence the no-op for everyone.
                if !e.ok {
                    self.session_gather.remove(&e.stroke_id);
                    if let Some(ed) = &mut self.editor {
                        ed.canvas.remote_wet_end.push(e.stroke_id);
                    }
                    self.session_seq += 1;
                    if let net::Session::Hosting(h) = &self.session {
                        h.send_stroke_frame(net::enc_end(&net::StrokeEndWire {
                            author: author.clone(),
                            stroke_id: e.stroke_id,
                            seq: self.session_seq,
                            ok: false,
                        }));
                    }
                    return;
                }
                let Some(g) = self.session_gather.remove(&e.stroke_id) else {
                    net::slog(format!(
                        "END for unknown stroke {} from {author}",
                        e.stroke_id
                    ));
                    return;
                };
                match self.resolve_replay(e.stroke_id, g) {
                    Ok(rs) => {
                        if let Some(ed) = &mut self.editor {
                            ed.canvas.replay_inbox.push_back(canvas::SeqTask::Stroke(rs));
                        }
                    }
                    Err(why) => {
                        net::slog(format!(
                            "stroke {} from {author} refused: {why}",
                            e.stroke_id
                        ));
                        // STAGE 2: drop its live overlay on the host too.
                        if let Some(ed) = &mut self.editor {
                            ed.canvas.remote_wet_end.push(e.stroke_id);
                        }
                        // A sequenced no-op keeps every lane in step and
                        // clears the other guests' gathers.
                        self.session_seq += 1;
                        if let net::Session::Hosting(h) = &self.session {
                            h.send_stroke_frame(net::enc_end(&net::StrokeEndWire {
                                author: author.clone(),
                                stroke_id: e.stroke_id,
                                seq: self.session_seq,
                                ok: false,
                            }));
                            h.send_refusal(&author, &why);
                        }
                    }
                }
            }
            net::StrokeFrame::Hashes(_) | net::StrokeFrame::Repair { .. } => {}
        }
    }

    /// Both roles: turn a finished gather into a replay-ready stroke
    /// (resource hashes resolve to the session cache's bytes).
    fn resolve_replay(
        &self,
        stroke_id: u64,
        g: GatherStroke,
    ) -> Result<canvas::ReplayStroke, String> {
        let tip = match g.tip {
            None => None,
            Some((hash, _, _, frames)) => match self.session_res.get(&hash) {
                Some((w, h, b)) => Some((*w, *h, b.clone(), frames)),
                None => return Err(format!("tip image {hash:016x} never arrived")),
            },
        };
        let grain = match g.grain {
            None => None,
            Some((hash, _, _, scale, strength)) => match self.session_res.get(&hash) {
                Some((w, h, b)) => Some((*w, *h, b.clone(), scale, strength)),
                None => return Err(format!("grain image {hash:016x} never arrived")),
            },
        };
        Ok(canvas::ReplayStroke {
            stroke_id,
            author: g.author,
            drawing: g.drawing,
            layer_name: g.layer_name,
            mode: g.mode,
            opacity: g.opacity,
            dabs: g.dabs,
            tip,
            grain,
        })
    }

    /// Guest lane: sequenced arrivals apply in host order once the join
    /// snapshot has landed; before that they buffer (NEVER-DO 2).
    /// v0.2.6 (third live test's log): the lane ALSO freezes the moment
    /// a snapshot is announced (meta seen) until its swap — applying
    /// events during the transfer/parse window advanced applied_seq
    /// past the doc's as_of, and on a slow link every snapshot arrived
    /// "stale" forever (the livelock's second life). Buffered events
    /// with seq ≤ meta skip at the drain; the rest apply in order.
    fn seq_event(&mut self, ev: SeqEv) {
        let ready = self
            .editor
            .as_ref()
            .is_some_and(|ed| ed.canvas.guest_ready);
        let snapshot_inbound =
            self.session_snapshot_seq.is_some() || self.session_load_rx.is_some();
        if !ready || snapshot_inbound {
            self.session_buffer.push(ev);
            return;
        }
        self.apply_seq_event(ev);
    }

    /// v0.2.6: unfreeze the lane — drain everything buffered while a
    /// snapshot was inbound (dups vs the doc skip via the seq gate).
    fn drain_session_buffer(&mut self) {
        if self.session_buffer.is_empty() {
            return;
        }
        net::slog(format!(
            "LANE thaw: draining {} buffered event(s)",
            self.session_buffer.len()
        ));
        let buffered = std::mem::take(&mut self.session_buffer);
        for ev in buffered {
            self.apply_seq_event(ev);
        }
    }

    fn apply_seq_event(&mut self, ev: SeqEv) {
        match ev {
            SeqEv::End(e) => {
                if e.seq <= self.session_applied_seq {
                    // Already inside the snapshot we applied.
                    self.session_gather.remove(&e.stroke_id);
                    return;
                }
                if e.seq != self.session_applied_seq + 1 {
                    net::slog(format!(
                        "SEQ GAP want={} got={} — resync",
                        self.session_applied_seq + 1,
                        e.seq
                    ));
                    if let net::Session::Joined(c) = &mut self.session {
                        c.send_resync_request();
                    }
                    return;
                }
                self.session_applied_seq = e.seq;
                let Some(g) = self.session_gather.remove(&e.stroke_id) else {
                    if e.ok {
                        net::slog(format!(
                            "SEQ {} unknown stroke {} — resync",
                            e.seq, e.stroke_id
                        ));
                        if let net::Session::Joined(c) = &mut self.session {
                            c.send_resync_request();
                        }
                    }
                    if let Some(ed) = &mut self.editor {
                        ed.canvas.remote_wet_end.push(e.stroke_id);
                    }
                    return;
                };
                if !e.ok {
                    // STAGE 2: the stroke died at the host — its live
                    // overlay drops.
                    if let Some(ed) = &mut self.editor {
                        ed.canvas.remote_wet_end.push(e.stroke_id);
                    }
                    return;
                }
                match self.resolve_replay(e.stroke_id, g) {
                    Ok(rs) => {
                        if let Some(ed) = &mut self.editor {
                            ed.canvas.replay_inbox.push_back(canvas::SeqTask::Stroke(rs));
                        }
                    }
                    Err(why) => {
                        net::slog(format!(
                            "SEQ {} stroke {}: {why} — resync",
                            e.seq, e.stroke_id
                        ));
                        if let net::Session::Joined(c) = &mut self.session {
                            c.send_resync_request();
                        }
                    }
                }
            }
            SeqEv::Cmds(batch) => {
                let seq = batch.seq;
                if seq <= self.session_applied_seq {
                    return;
                }
                if seq != self.session_applied_seq + 1 {
                    net::slog(format!(
                        "SEQ GAP (cmds) want={} got={seq} — resync",
                        self.session_applied_seq + 1
                    ));
                    if let net::Session::Joined(c) = &mut self.session {
                        c.send_resync_request();
                    }
                    return;
                }
                self.session_applied_seq = seq;
                // STAGE 3: our own echo — the prediction already applied
                // (with our hand on it); only the number advances.
                let mine = batch.origin == self.config.session.username.trim();
                if mine {
                    return;
                }
                net::slog(format!(
                    "SEQ {seq} cmds from {} n={} — queued",
                    batch.origin,
                    batch.cmds.len()
                ));
                // v0.2.3 (the X-sheet law): commands ride the SAME
                // ordered queue as strokes — a delete-frame can no
                // longer leapfrog the stroke it follows.
                if let Some(ed) = &mut self.editor {
                    ed.canvas.replay_inbox.push_back(canvas::SeqTask::Cmds {
                        origin: batch.origin,
                        cmds: batch.cmds,
                    });
                }
            }
            SeqEv::Undo { author, redo, seq } => {
                if seq <= self.session_applied_seq {
                    return;
                }
                if seq != self.session_applied_seq + 1 {
                    net::slog(format!(
                        "SEQ GAP (undo) want={} got={seq} — resync",
                        self.session_applied_seq + 1
                    ));
                    if let net::Session::Joined(c) = &mut self.session {
                        c.send_resync_request();
                    }
                    return;
                }
                self.session_applied_seq = seq;
                net::slog(format!("SEQ {seq} UNDONE {author} redo={redo} — queued"));
                // v0.2.3: undos ride the one ordered queue too.
                if let Some(ed) = &mut self.editor {
                    ed.canvas
                        .replay_inbox
                        .push_back(canvas::SeqTask::Undo { author, redo });
                }
            }
            SeqEv::Hashes(hw) => {
                // Ordered after its End; the replay itself lands on the
                // next canvas frame — match whichever side arrives last.
                if let Some(changed) = self.session_done.remove(&hw.stroke_id) {
                    self.audit_stroke(&hw, &changed);
                } else {
                    self.session_audit.insert(hw.stroke_id, hw);
                }
            }
        }
    }

    /// NEVER-DO 3: compare the committed tiles against the host's
    /// hashes; drifted coords get exact texels back, quietly, logged.
    /// v0.2.1 (audit): tiles only THIS machine changed are drift by
    /// construction (the host never touched them) — repaired too; and
    /// the layer is located project-wide, not through the viewer's cut.
    fn audit_stroke(&mut self, hw: &net::StrokeHashesWire, my_changed: &[(i32, i32)]) {
        let bad: Vec<(i32, i32)> = self
            .editor
            .as_ref()
            .and_then(|ed| {
                let did = anim_core::ids::DrawingId(hw.drawing);
                ed.state.locate_layer(did, &hw.layer_name).map(|l| {
                    let mut bad: Vec<(i32, i32)> = hw
                        .tiles
                        .iter()
                        .filter(|(x, y, h)| {
                            l.raster.tiles.get(&(*x, *y)).map(|t| t.hash).unwrap_or(0)
                                != *h
                        })
                        .map(|(x, y, _)| (*x, *y))
                        .collect();
                    for c in my_changed {
                        if !hw.tiles.iter().any(|(x, y, _)| (*x, *y) == *c)
                            && !bad.contains(c)
                        {
                            bad.push(*c);
                        }
                    }
                    bad
                })
            })
            .unwrap_or_default();
        if bad.is_empty() {
            return;
        }
        net::slog(format!(
            "AUDIT stroke={} drift={} tile(s) — repairing",
            hw.stroke_id,
            bad.len()
        ));
        if let net::Session::Joined(c) = &mut self.session {
            c.send_repair_request(hw.drawing, &hw.layer_name, bad);
        }
    }

    /// v0.2.5 (owner: "after you join all the hosts Usable brushes work
    /// alongside your own installed ones"): serialize this machine's
    /// preset list for the room. Tip/grain images go content-addressed
    /// into the session res cache (every joiner receives that whole
    /// cache already); the presets travel with rewritten "net<hash>"
    /// keys. Runs once per hosting start.
    fn build_session_bank(&mut self) {
        let mut entries: Vec<BankEntry> = Vec::new();
        for p in &self.config.presets {
            let mut e = BankEntry {
                preset: p.clone(),
                tip: None,
                grain: None,
            };
            if let Some(eng) = &mut e.preset.engine {
                if let Some(key) = eng.tip_key.clone()
                    && let Some(dir) = crate::kpp::tips_dir()
                    && let Some((w, h, bytes)) =
                        canvas::load_cache_png(&dir.join(format!("{key}.png")))
                {
                    let frames: u32 = std::fs::read_to_string(dir.join(format!("{key}.frames")))
                        .ok()
                        .and_then(|t| t.trim().parse().ok())
                        .unwrap_or(1);
                    let hash = canvas::wire_hash(&bytes);
                    self.session_res
                        .insert(hash, (w, h, std::sync::Arc::new(bytes)));
                    eng.tip_key = Some(format!("net{hash:016x}"));
                    e.tip = Some((hash, frames));
                } else if eng.tip_key.is_some() {
                    // The tip file is missing on THIS machine — the
                    // preset paints procedurally here, so it does there.
                    eng.tip_key = None;
                }
                if let Some(key) = eng.grain_key.clone()
                    && let Some(dir) = crate::kpp::grains_dir()
                    && let Some((w, h, bytes)) =
                        canvas::load_cache_png(&dir.join(format!("{key}.png")))
                {
                    let hash = canvas::wire_hash(&bytes);
                    self.session_res
                        .insert(hash, (w, h, std::sync::Arc::new(bytes)));
                    eng.grain_key = Some(format!("net{hash:016x}"));
                    e.grain = Some(hash);
                } else if eng.grain_key.is_some() {
                    eng.grain_key = None;
                }
            }
            entries.push(e);
        }
        let n = entries.len();
        self.session_bank_json = serde_json::to_string(&entries).ok();
        net::slog(format!(
            "BANK built: {n} preset(s), {} image(s) in the session cache",
            self.session_res.len()
        ));
    }

    /// v0.2.5: install the host's bank — materialize the images this
    /// machine lacks into its brush caches (content-hash names, exact
    /// bytes), and shelve the presets session-scoped beside our own.
    fn install_session_bank(&mut self, json: &str) {
        let Ok(entries) = serde_json::from_str::<Vec<BankEntry>>(json) else {
            net::slog("BANK parse failed");
            return;
        };
        let mut installed = Vec::with_capacity(entries.len());
        for mut e in entries {
            if let Some((hash, frames)) = e.tip {
                if let (Some(dir), Some((w, h, bytes))) =
                    (crate::kpp::tips_dir(), self.session_res.get(&hash))
                {
                    let path = dir.join(format!("net{hash:016x}.png"));
                    if !path.exists()
                        && let Err(err) = crate::kpp::write_cache_png(&path, *w, *h, bytes)
                    {
                        net::slog(format!("BANK tip write failed: {err}"));
                    }
                    let _ = std::fs::write(
                        dir.join(format!("net{hash:016x}.frames")),
                        frames.to_string(),
                    );
                } else if let Some(eng) = &mut e.preset.engine {
                    net::slog(format!("BANK tip image {hash:016x} missing — procedural"));
                    eng.tip_key = None;
                }
            }
            if let Some(hash) = e.grain {
                if let (Some(dir), Some((w, h, bytes))) =
                    (crate::kpp::grains_dir(), self.session_res.get(&hash))
                {
                    let path = dir.join(format!("net{hash:016x}.png"));
                    if !path.exists()
                        && let Err(err) = crate::kpp::write_cache_png(&path, *w, *h, bytes)
                    {
                        net::slog(format!("BANK grain write failed: {err}"));
                    }
                } else if let Some(eng) = &mut e.preset.engine {
                    eng.grain_key = None;
                }
            }
            // Marked by name AND bank: visible whose it is, filtered
            // out of every config save, gone with the room.
            e.preset.name = format!("⌂ {}", e.preset.name);
            e.preset.bank = SESSION_BANK.to_string();
            installed.push(e.preset);
        }
        net::slog(format!("BANK installed: {} host preset(s)", installed.len()));
        if let Some(ed) = &mut self.editor {
            ed.state.status = format!(
                "the host's pencil box arrived — {} brush(es) beside your own",
                installed.len()
            );
        }
        self.session_presets = installed;
    }

    /// v0.2.4 (owner): "It should send right away to the Host or the
    /// Host right away to the user" — a newer build in the room makes
    /// the OLDER side fetch its update IMMEDIATELY, either direction,
    /// instead of waiting out the 15-minute checker. The staged build
    /// then installs by lamp click or on close, as always.
    fn version_gap_check(&mut self, other_ver: &str, who: &str) {
        if other_ver.trim().is_empty() {
            return;
        }
        let mine = crate::update::parse_version(crate::update::CURRENT_VERSION);
        let theirs = crate::update::parse_version(other_ver);
        if theirs > mine {
            net::slog(format!(
                "VERSION {who} runs v{other_ver} > v{} — fetching the update NOW",
                crate::update::CURRENT_VERSION
            ));
            self.update_note = format!("{who} runs v{other_ver} — updating this build now…");
            if let Some(ed) = &mut self.editor {
                ed.state.status =
                    format!("{who} is on a newer build — the update is fetching now");
            }
            if self.update_rx.is_none()
                && self.update_staged.is_none()
                && !self.config.update_repo.trim().is_empty()
            {
                self.update_rx = Some(crate::update::spawn_check(
                    self.config.update_repo.trim().to_string(),
                ));
            }
        } else if theirs < mine {
            net::slog(format!(
                "VERSION {who} runs v{other_ver} < v{} — their side updates on join",
                crate::update::CURRENT_VERSION
            ));
            if let Some(ed) = &mut self.editor {
                ed.state.status = format!(
                    "{who} is on v{other_ver} — their build is fetching the update"
                );
            }
        }
    }

    /// v0.2.1 (audit): strokes the canvas committed LAST frame take
    /// their numbers BEFORE this frame's arrivals are processed — the
    /// wire's seq order matches the host's document order. Guests use
    /// the same drain to match audits against their replays.
    fn drain_replay_done(&mut self, me: &str) {
        // v0.2.2: while a snapshot builds, nothing new takes a number —
        // the document under construction must contain every seq that
        // precedes it (host-side; a guest never builds snapshots).
        if matches!(self.session, net::Session::Hosting(_)) && self.session_snap_rx.is_some()
        {
            return;
        }
        if matches!(self.session, net::Session::Hosting(_)) {
            let dones = self
                .editor
                .as_mut()
                .map(|ed| std::mem::take(&mut ed.canvas.replay_done))
                .unwrap_or_default();
            for d in dones {
                self.session_seq += 1;
                let seq = self.session_seq;
                let author = if d.author.is_empty() {
                    me.to_string()
                } else {
                    d.author.clone()
                };
                let hashes: Vec<(i32, i32, u64)> = if d.ok {
                    self.editor
                        .as_ref()
                        .and_then(|ed| {
                            // Project-wide, never the viewer's cut.
                            ed.state
                                .locate_layer(
                                    anim_core::ids::DrawingId(d.drawing),
                                    &d.layer_name,
                                )
                                .map(|l| {
                                    d.changed
                                        .iter()
                                        .map(|(x, y)| {
                                            (
                                                *x,
                                                *y,
                                                l.raster
                                                    .tiles
                                                    .get(&(*x, *y))
                                                    .map(|t| t.hash)
                                                    .unwrap_or(0),
                                            )
                                        })
                                        .collect()
                                })
                        })
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                net::slog(format!(
                    "SEQ {seq} stroke={} author={author} ok={} tiles={}",
                    d.stroke_id,
                    d.ok,
                    d.changed.len()
                ));
                if let net::Session::Hosting(h) = &self.session {
                    h.send_stroke_frame(net::enc_end(&net::StrokeEndWire {
                        author: author.clone(),
                        stroke_id: d.stroke_id,
                        seq,
                        ok: d.ok,
                    }));
                    if d.ok {
                        h.send_stroke_frame(net::enc_hashes(&net::StrokeHashesWire {
                            stroke_id: d.stroke_id,
                            seq,
                            drawing: d.drawing,
                            layer_name: d.layer_name.clone(),
                            tiles: hashes,
                        }));
                    } else if !d.author.is_empty() {
                        h.send_refusal(&d.author, &d.why);
                    }
                }
            }
        }
        // Guest: a replay landed — run any waiting audit; a failed
        // replay means our mirror slipped somewhere: resync.
        if matches!(self.session, net::Session::Joined(_)) {
            let dones = self
                .editor
                .as_mut()
                .map(|ed| std::mem::take(&mut ed.canvas.replay_done))
                .unwrap_or_default();
            for d in dones {
                if !d.ok {
                    net::slog(format!(
                        "REPLAY failed stroke={} ({}) — resync",
                        d.stroke_id, d.why
                    ));
                    if let net::Session::Joined(c) = &mut self.session {
                        c.send_resync_request();
                    }
                    continue;
                }
                if let Some(hw) = self.session_audit.remove(&d.stroke_id) {
                    self.audit_stroke(&hw, &d.changed);
                } else {
                    self.session_done.insert(d.stroke_id, d.changed);
                }
            }
            // v0.2.3: a queued command batch or undo that would not
            // apply — our document split from the host's: resync.
            let fails = self
                .editor
                .as_mut()
                .map(|ed| std::mem::take(&mut ed.canvas.seq_task_fail))
                .unwrap_or_default();
            for why in fails {
                net::slog(format!("SEQ TASK failed: {why} — resync"));
                if let net::Session::Joined(c) = &mut self.session {
                    c.send_resync_request();
                }
            }
        }
    }

    fn session_pump(&mut self, ctx: &egui::Context) {
        let me = self.config.session.username.trim().to_string();
        self.drain_replay_done(&me);
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
        let mut joiners: Vec<u64> = Vec::new();
        // v0.2.2: while a snapshot builds, seq-assigning arrivals HOLD —
        // the document being built must contain everything numbered
        // before it, so a snapshot can never arrive stale (the first
        // live test's livelock: 26MB storms behind a moving lane).
        let snap_building = self.session_snap_rx.is_some();
        if !snap_building && !self.session_ev_hold.is_empty() {
            let held = std::mem::take(&mut self.session_ev_hold);
            events.splice(0..0, held);
        }
        for ev in events {
            if snap_building
                && matches!(
                    ev,
                    net::NetEvent::GuestCmds { .. } | net::NetEvent::UndoRequest { .. }
                )
            {
                self.session_ev_hold.push(ev);
                continue;
            }
            match ev {
                net::NetEvent::Status(st) => self.session_status = st,
                net::NetEvent::PeerJoined { id, name, version } => {
                    self.session_status = format!("{name} joined");
                    // AUDIT [44]: this reached Settings only — invisible
                    // to someone who is drawing. Chatter, not a refusal.
                    if let Some(ed) = &mut self.editor {
                        ed.state.status = format!("{name} joined the room");
                    }
                    fresh_join = true;
                    joiners.push(id);
                    self.session_snap_targets.insert(id);
                    net::slog(format!("PEER joined: {name} (client {id}, v{version})"));
                    // v0.2.4: a newer build in the room updates THIS one
                    // right away (and an older peer's gap is announced).
                    self.version_gap_check(&version, &name);
                    self.session_peers.insert(
                        id,
                        net::PeerView {
                            name,
                            version,
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
                        net::slog(format!("PEER left: {who}"));
                        if let Some(ed) = &mut self.editor {
                            ed.state.status = format!("{who} left the room");
                        }
                        // v0.2.1 (audit): their mid-flight strokes die
                        // with them — gathers drop, overlays clear, and
                        // (hosting) every peer hears the sequenced no-op.
                        let stale: Vec<u64> = self
                            .session_gather
                            .iter()
                            .filter(|(_, g)| g.author == who)
                            .map(|(k, _)| *k)
                            .collect();
                        for sid in stale {
                            self.session_gather.remove(&sid);
                            if let Some(ed) = &mut self.editor {
                                ed.canvas.remote_wet_end.push(sid);
                            }
                            if matches!(self.session, net::Session::Hosting(_)) {
                                self.session_seq += 1;
                                if let net::Session::Hosting(h) = &self.session {
                                    h.send_stroke_frame(net::enc_end(
                                        &net::StrokeEndWire {
                                            author: who.clone(),
                                            stroke_id: sid,
                                            seq: self.session_seq,
                                            ok: false,
                                        },
                                    ));
                                }
                            }
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
                        version: String::new(),
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
                // STAGE 1: sequenced command batches ride the same lane
                // as strokes — one order for everything (NEVER-DO 2).
                net::NetEvent::Commands(batch) => {
                    self.seq_event(SeqEv::Cmds(batch));
                }
                // STAGE 3: a guest's predicted batch — apply with their
                // hand on it, then it takes the next number and goes to
                // every peer (the origin skips its own echo).
                net::NetEvent::GuestCmds { origin, mut cmds } => {
                    let outcome = self.editor.as_mut().map(|ed| {
                        // v0.2.1 (LAW 1): the wire strips PaintTiles
                        // befores — rebuild them from OUR document.
                        ed.state.rebuild_paint_befores(&mut cmds);
                        let was = ed.state.engine.mirror_log;
                        ed.state.engine.mirror_log = false;
                        let prev = ed.state.engine.author();
                        ed.state.engine.set_author(Some(origin.clone()));
                        let r = ed.state.engine.apply("remote edit", cmds.clone());
                        ed.state.engine.set_author(prev);
                        ed.state.engine.mirror_log = was;
                        if r.is_ok() {
                            ed.state.doc_gen = ed.state.doc_gen.wrapping_add(1);
                            // GM5: a guest may have removed our cut or column.
                            ed.state.keep_view_live();
                            ed.canvas.engine_changed();
                        }
                        r
                    });
                    match outcome {
                        Some(Ok(())) => {
                            self.session_seq += 1;
                            net::slog(format!(
                                "SEQ {} cmds from {origin} n={}",
                                self.session_seq,
                                cmds.len()
                            ));
                            if let net::Session::Hosting(h) = &self.session {
                                h.send_commands(net::CmdBatchWire {
                                    seq: self.session_seq,
                                    origin: origin.clone(),
                                    cmds,
                                });
                            }
                        }
                        Some(Err(e)) => {
                            net::slog(format!("guest cmds from {origin} refused: {e:?}"));
                            if let net::Session::Hosting(h) = &self.session {
                                h.send_refusal(&origin, &format!("{e:?}"));
                            }
                        }
                        None => {}
                    }
                }
                net::NetEvent::Undone { author, redo, seq } => {
                    self.seq_event(SeqEv::Undo { author, redo, seq });
                }
                net::NetEvent::Stroke(frame) => {
                    self.guest_stroke_frame(frame);
                }
                net::NetEvent::GuestStroke {
                    client,
                    author,
                    frame,
                } => {
                    self.host_stroke_frame(client, author, frame);
                }
                net::NetEvent::SnapshotMeta(s) => {
                    self.session_snapshot_seq = Some(s);
                }
                // STAGE 4: the host's arrangement — held, never applied
                // unasked; Settings ▸ Layout offers it as a LOAD.
                net::NetEvent::HostLayout(json) => {
                    self.session_host_layout = Some(json);
                }
                // v0.2.5: the host's pencil box — usable alongside our
                // own for the session's life.
                net::NetEvent::BrushBank(json) => {
                    self.install_session_bank(&json);
                }
                net::NetEvent::RepairRequest {
                    client,
                    drawing,
                    layer_name,
                    coords,
                } => {
                    net::slog(format!(
                        "REPAIR req client={client} drawing={drawing} '{layer_name}' tiles={}",
                        coords.len()
                    ));
                    let tiles: Vec<(i32, i32, Vec<u16>)> = self
                        .editor
                        .as_ref()
                        .and_then(|ed| {
                            // v0.2.1: project-wide, never the viewer's cut.
                            ed.state
                                .locate_layer(anim_core::ids::DrawingId(drawing), &layer_name)
                                .map(|l| {
                                    coords
                                        .iter()
                                        .map(|(x, y)| {
                                            (
                                                *x,
                                                *y,
                                                l.raster
                                                    .tiles
                                                    .get(&(*x, *y))
                                                    .map(|t| t.rgba.to_vec())
                                                    .unwrap_or_default(),
                                            )
                                        })
                                        .collect()
                                })
                        })
                        .unwrap_or_default();
                    if let net::Session::Hosting(h) = &self.session {
                        h.send_repair(client, drawing, &layer_name, tiles);
                    }
                }
                net::NetEvent::RepairTiles {
                    drawing,
                    layer_name,
                    tiles,
                } => {
                    net::slog(format!(
                        "REPAIR apply '{layer_name}': {} tile(s)",
                        tiles.len()
                    ));
                    if let Some(ed) = &mut self.editor {
                        ed.state.apply_repair_tiles(
                            anim_core::ids::DrawingId(drawing),
                            &layer_name,
                            tiles,
                        );
                        ed.canvas.engine_changed();
                    }
                }
                net::NetEvent::ResyncNeeded { client } => {
                    net::slog(format!("RESYNC requested by client {client}"));
                    self.session_snap_targets.insert(client);
                }
                // STAGE 0: a guest machine reporting in — append its
                // numbered lines to the host's remote log, arrival-ordered
                // (TCP keeps per-peer order; the #seq exposes any gap).
                net::NetEvent::DebugLog { from, lines } => {
                    if let Some(base) = std::env::var_os("APPDATA") {
                        let dir = std::path::PathBuf::from(base).join("AnimStudio");
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(dir.join("session_log_remote.txt"))
                        {
                            use std::io::Write as _;
                            for l in &lines {
                                let _ = writeln!(f, "[{from}] {l}");
                            }
                        }
                    }
                }
                net::NetEvent::Snapshot(bytes) => {
                    // Inflate (the wire carries deflated documents).
                    let bytes = {
                        use std::io::Read as _;
                        let mut dec =
                            flate2::read::DeflateDecoder::new(bytes.as_slice());
                        let mut out = Vec::new();
                        match dec.read_to_end(&mut out) {
                            Ok(_) => out,
                            Err(_) => bytes, // pre-compression peer
                        }
                    };
                    // ONE TRUTH (NEVER-DO 1): the host's document replaces
                    // ours wholesale, never merged. SESSION PERF 2: the
                    // PARSE runs on a worker — this arm only writes the
                    // bytes and spawns it; the swap happens in the drain
                    // below once parsing finishes (latest snapshot wins).
                    let path = std::env::temp_dir().join("animstudio_session_in.animproj");
                    if std::fs::write(&path, &bytes).is_ok() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.session_load_rx = Some(rx);
                        std::thread::spawn(move || {
                            let _ = tx.send(doc::AppState::load_from(path));
                        });
                    }
                }
                // STAGE 1: the tile wire is RETIRED — a peer still
                // sending EditTiles runs a pre-rescope build.
                net::NetEvent::EditTiles { author, .. } => {
                    net::slog(format!(
                        "LEGACY EditTiles from {author} — build predates the stroke wire"
                    ));
                    if let net::Session::Hosting(h) = &self.session {
                        h.send_refusal(
                            &author,
                            "your build predates the stroke wire — update Gaban Studio",
                        );
                    }
                }
                // HOST: a guest asking to undo/redo their own step.
                // STAGE 3: a successful one is SEQUENCED — every peer
                // (the asker included) replays the same per-artist undo
                // on its identical history. No snapshot.
                net::NetEvent::UndoRequest { author, redo } => {
                    let outcome = self.editor.as_mut().map(|ed| {
                        let was = ed.state.engine.mirror_log;
                        ed.state.engine.mirror_log = false;
                        let r = ed.state.remote_history(&author, redo);
                        ed.state.engine.mirror_log = was;
                        r
                    });
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
                            self.session_seq += 1;
                            net::slog(format!(
                                "SEQ {} UNDONE {author} redo={redo}",
                                self.session_seq
                            ));
                            if let net::Session::Hosting(h) = &self.session {
                                h.send_undone(&author, redo, self.session_seq);
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
                    // STAGE 3: our refused PREDICTION may already sit in
                    // this document — take the host's truth fresh.
                    // v0.2.1 (audit): with the CAUSE in the log.
                    net::slog(format!("RESYNC: refused by the host — {why}"));
                    if let net::Session::Joined(c) = &mut self.session {
                        c.send_resync_request();
                    }
                }
                net::NetEvent::Ended(why) => {
                    self.session = net::Session::Idle;
                    self.session_peers.clear();
                    net::slog_ship_arm(false);
                    self.session_lane_reset();
                    if let Some(ed) = &mut self.editor {
                        ed.canvas.guest_ready = false;
                    }
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
        // SESSION PERF: presence at 20Hz, not frame rate — the wet
        // clone + JSON per frame was measurable on both ends.
        let presence_due = {
            let t = ctx.input(|i| i.time);
            if t - self.session_last_presence > 0.05 {
                self.session_last_presence = t;
                true
            } else {
                false
            }
        };
        let presence = if presence_due {
            self.editor.as_ref().map(|ed| net::PresenceOut {
                frame: ed.state.view.frame,
                cursor: ed.canvas.presence_pos,
                pen_down: ed.canvas.stroke_active(),
                // STAGE 2: the wet ghost is RETIRED — the dab stream IS
                // the live view now, at zero extra wire cost.
                wet: Vec::new(),
            })
        } else {
            None
        };
        let now = ctx.input(|i| i.time);
        // STAGE 0: the STATS heartbeat — 5s windows of frame rate, worst
        // frame gap, and wire totals, both roles, only while in a room.
        if !matches!(self.session, net::Session::Idle) {
            let dt_ms = ctx.input(|i| i.unstable_dt) * 1000.0;
            // v0.2.1 (audit): count the SOCKET truth, not the UI's peer
            // list (they disagreed in the first live test), and carry
            // the queue depths — stalls and leaks become readable.
            let peers = match &self.session {
                net::Session::Hosting(h) => h.peer_count(),
                _ => self.session_peers.len(),
            };
            let rq = self
                .editor
                .as_ref()
                .map(|ed| ed.canvas.replay_inbox.len())
                .unwrap_or(0);
            let gathers = self.session_gather.len();
            let (start, frames, gap_max) = &mut self.stats_window;
            if *start == 0.0 {
                *start = now;
            }
            *frames += 1;
            *gap_max = gap_max.max(dt_ms);
            if now - *start >= 5.0 {
                let (tx_b, rx_b) = net::wire_totals();
                net::slog(format!(
                    "STATS fps={:.0} gapmax={:.0}ms tx={}KB rx={}KB peers={} rq={} gathers={}",
                    *frames as f64 / (now - *start),
                    gap_max,
                    tx_b / 1024,
                    rx_b / 1024,
                    peers,
                    rq,
                    gathers,
                ));
                *start = now;
                *frames = 0;
                *gap_max = 0.0;
            }
        } else if self.stats_window.1 != 0 {
            self.stats_window = (0.0, 0, 0.0);
        }
        // STAGE 0: a guest flushes its numbered lines to the host every 2s.
        if matches!(self.session, net::Session::Joined(_)) && now - self.dbg_ship_last > 2.0 {
            self.dbg_ship_last = now;
            let lines = net::slog_ship_drain();
            if !lines.is_empty()
                && let net::Session::Joined(c) = &mut self.session
            {
                c.send_debug(lines);
            }
        }
        // SESSION MIRROR: the log rides the hosting state; every applied
        // batch streams to guests (tiny), and whole documents go out only
        // for joins and resyncs.
        let hosting_now = matches!(self.session, net::Session::Hosting(_));
        let in_room = !matches!(self.session, net::Session::Idle);
        // STAGE 1: canvas knows a session is live (strokes stream).
        if let Some(ed) = &mut self.editor {
            ed.canvas.session_live = in_room;
        }
        // STAGE 3: BOTH roles log applied commands (the host to
        // broadcast, a guest to send predictions), and the engine's
        // base author is this artist's name — every history entry
        // carries its hand, on every machine, so per-artist undo
        // replays identically everywhere.
        if let Some(ed) = &mut self.editor {
            if ed.state.engine.mirror_log != in_room {
                ed.state.engine.mirror_log = in_room;
                let _ = ed.state.engine.drain_applied();
            }
            if self.session_author_armed != in_room {
                self.session_author_armed = in_room;
                ed.state
                    .engine
                    .set_author(in_room.then(|| me.clone()));
            }
        }
        if hosting_now && self.session_snap_rx.is_none() {
            // STAGE 1: every batch takes the next number in the one
            // global order (strokes share the same lane). v0.2.2: held
            // while a snapshot builds (the log keeps accumulating).
            let batches = self
                .editor
                .as_mut()
                .map(|ed| ed.state.engine.drain_applied())
                .unwrap_or_default();
            if !batches.is_empty()
                && let net::Session::Hosting(h) = &self.session
            {
                for b in batches {
                    self.session_seq += 1;
                    h.send_commands(net::CmdBatchWire {
                        seq: self.session_seq,
                        origin: me.clone(),
                        cmds: b,
                    });
                }
            }
        } else if in_room {
            // STAGE 3: a guest's local edits are PREDICTIONS — applied
            // here already, sent whole to the host to be sequenced; the
            // echo (origin == me) is skipped on arrival.
            let batches = self
                .editor
                .as_mut()
                .map(|ed| ed.state.engine.drain_applied())
                .unwrap_or_default();
            if !batches.is_empty()
                && let net::Session::Joined(c) = &mut self.session
            {
                for b in batches {
                    net::slog(format!("PREDICT cmds n={}", b.len()));
                    c.send_cmds(&me, b);
                }
            }
        }
        let mut snapshot: Option<Vec<u8>> = None;
        if let net::Session::Hosting(_) = &self.session {
            // SESSION PERF: a snapshot is a FULL document save on the UI
            // thread — it must never ride a pen-up directly. Send only
            // when the doc changed, the pen is up, and the last one is
            // at least 2.5s old (a fresh join always forces one).
            let stamp = self.editor.as_ref().map_or(0u64, |ed| ed.state.doc_gen);
            let pen_busy = self
                .editor
                .as_ref()
                .is_some_and(|ed| ed.canvas.stroke_active());
            let _ = stamp;
            // STAGE 3: history rewrites are SEQUENCED (Undone) — the
            // snapshot path serves joins and resync requests only.
            let _ = self
                .editor
                .as_mut()
                .map(|ed| std::mem::take(&mut ed.state.history_dirty));
            // v0.2.2: requester-only. A build starts when someone wants
            // one; the finished document goes to exactly those clients.
            let due = !self.session_snap_targets.is_empty()
                && !pen_busy
                && now - self.session_last_snap > 1.0;
            // STAGE 1/4 (v0.2.2: to the JOINER only): a fresh joiner
            // needs the session's images before any Begin that
            // references them, and gets the host's loadable layout.
            if fresh_join && let net::Session::Hosting(h) = &self.session {
                let layout = self
                    .editor
                    .as_ref()
                    .map(|ed| workspace::Workspace {
                        name: "host's layout".into(),
                        dock: ed.dock.clone(),
                        preset: None,
                        view: Some(ed.canvas.snapshot_view(&ed.state)),
                    })
                    .and_then(|w| serde_json::to_string(&w).ok());
                for j in &joiners {
                    for (hash, (w, hh, b)) in &self.session_res {
                        h.send_stroke_frame_to(*j, net::enc_res(*hash, *w, *hh, b));
                    }
                    if let Some(json) = &layout {
                        h.send_layout_to(*j, json.clone());
                    }
                    // v0.2.5: the host's usable brushes, after their
                    // images (same writer — ordered).
                    if let Some(json) = &self.session_bank_json {
                        h.send_bank_to(*j, json.clone());
                    }
                }
            }
            if due && self.session_snap_rx.is_none() {
                self.session_snap_pending = self.session_snap_targets.drain().collect();
                net::slog(format!(
                    "SNAPSHOT building for {:?} as_of={}",
                    self.session_snap_pending, self.session_seq
                ));
                self.session_sent_gen = stamp;
                self.session_last_snap = now;
                // STAGE 1: the order stamp this document will contain —
                // the clone below happens in this same frame, before any
                // later seq is assigned.
                self.session_snap_seq = self.session_seq;
                // SESSION PERF 2: the save runs on a WORKER — the UI
                // thread only clones the Arc-tiled project (cheap).
                if let Some(ed) = &mut self.editor {
                    let project = ed.state.snapshot_project();
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.session_snap_rx = Some(rx);
                    std::thread::spawn(move || {
                        let path =
                            std::env::temp_dir().join("animstudio_session_out.animproj");
                        let bytes = anim_core::store::save(&project, &path)
                            .ok()
                            .and_then(|_| std::fs::read(&path).ok())
                            .map(|raw| {
                                // 90MB LOG LINE (2026-08-19): raster f16
                                // tiles deflate hugely — compress on this
                                // worker before the wire.
                                use flate2::{Compression, write::DeflateEncoder};
                                use std::io::Write as _;
                                let raw_len = raw.len();
                                let mut enc = DeflateEncoder::new(
                                    Vec::new(),
                                    Compression::fast(),
                                );
                                let _ = enc.write_all(&raw);
                                let out = enc.finish().unwrap_or(raw);
                                crate::net::slog(format!(
                                    "SNAPSHOT built raw={raw_len} deflated={} bytes",
                                    out.len()
                                ));
                                out
                            });
                        let _ = tx.send(bytes);
                    });
                }
            }
            if let Some(rx) = &self.session_snap_rx
                && let Ok(built) = rx.try_recv()
            {
                self.session_snap_rx = None;
                snapshot = built;
            }
        }
        // STAGE 1 (PSD-multiplayer-rescope): the stroke wire.
        // Begin/Dabs/End leave here; unseen tip/grain images are
        // announced FIRST on the same writer (order is the contract).
        let stroke_out: Vec<canvas::StrokeMsg> = self
            .editor
            .as_mut()
            .map(|ed| std::mem::take(&mut ed.canvas.stroke_outbox))
            .unwrap_or_default();
        let history_req = self
            .editor
            .as_mut()
            .and_then(|ed| ed.canvas.history_request.take());
        // STAGE 3: the HOST's undo is per-artist too — one law for every
        // hand. It runs through the same machinery a guest's request
        // does, then goes out sequenced.
        if hosting_now
            && let Some(redo) = history_req
            && self.session_snap_rx.is_some()
        {
            // v0.2.2: held — it would take a number the building
            // snapshot cannot contain. Re-armed for the next pump.
            if let Some(ed) = &mut self.editor {
                ed.canvas.history_request = Some(redo);
            }
        } else if hosting_now && let Some(redo) = history_req {
            let outcome = self.editor.as_mut().map(|ed| {
                let was = ed.state.engine.mirror_log;
                ed.state.engine.mirror_log = false;
                let r = ed.state.remote_history(&me, redo);
                ed.state.engine.mirror_log = was;
                r
            });
            match outcome {
                Some(Ok(())) => {
                    if let Some(ed) = &mut self.editor {
                        ed.canvas.engine_changed();
                    }
                    self.session_seq += 1;
                    net::slog(format!(
                        "SEQ {} UNDONE {me} (host) redo={redo}",
                        self.session_seq
                    ));
                    if let net::Session::Hosting(h) = &self.session {
                        h.send_undone(&me, redo, self.session_seq);
                    }
                }
                Some(Err(why)) => {
                    if let Some(ed) = &mut self.editor {
                        ed.state.refuse(format!("refused — {why}"));
                    }
                }
                None => {}
            }
        }
        let mut wire_frames: Vec<Vec<u8>> = Vec::new();
        for msg in &stroke_out {
            if let canvas::StrokeMsg::Begin(info) = msg {
                for (hash, w, h, bytes) in [
                    info.tip.as_ref().map(|(r, b)| (r.hash, r.w, r.h, b.clone())),
                    info.grain
                        .as_ref()
                        .map(|(g, b)| (g.hash, g.w, g.h, b.clone())),
                ]
                .into_iter()
                .flatten()
                {
                    if self.session_res_announced.insert(hash) {
                        self.session_res.insert(hash, (w, h, bytes.clone()));
                        wire_frames.push(net::enc_res(hash, w, h, &bytes));
                    }
                }
            }
        }
        for msg in stroke_out {
            match msg {
                canvas::StrokeMsg::Begin(info) => {
                    let b = net::StrokeBeginWire {
                        author: me.clone(),
                        stroke_id: info.stroke_id,
                        drawing: info.drawing,
                        layer_name: info.layer_name.clone(),
                        mode: info.mode,
                        opacity: info.opacity,
                        tip: info
                            .tip
                            .as_ref()
                            .map(|(r, _)| (r.hash, r.w, r.h, r.frames)),
                        grain: info
                            .grain
                            .as_ref()
                            .map(|(g, _)| (g.hash, g.w, g.h, g.scale, g.strength)),
                    };
                    // A guest registers its OWN stroke too — the echo
                    // replays from this gather like anyone else's.
                    if matches!(self.session, net::Session::Joined(_)) {
                        self.session_gather.insert(
                            info.stroke_id,
                            GatherStroke {
                                author: me.clone(),
                                drawing: b.drawing,
                                layer_name: b.layer_name.clone(),
                                mode: b.mode,
                                opacity: b.opacity,
                                tip: b.tip,
                                grain: b.grain,
                                dabs: Vec::new(),
                            },
                        );
                    }
                    wire_frames.push(net::enc_begin(&b));
                }
                canvas::StrokeMsg::Dabs { stroke_id, dabs } => {
                    if let Some(g) = self.session_gather.get_mut(&stroke_id) {
                        g.dabs.extend(dabs.iter().copied());
                    }
                    wire_frames.push(net::enc_dabs(stroke_id, &dabs));
                }
                canvas::StrokeMsg::End { stroke_id } => {
                    wire_frames.push(net::enc_end(&net::StrokeEndWire {
                        author: me.clone(),
                        stroke_id,
                        seq: 0,
                        ok: true,
                    }));
                }
                // v0.2.1 (audit): a stroke that died locally tells the
                // peers — gathers and overlays must not leak.
                canvas::StrokeMsg::Abort { stroke_id } => {
                    net::slog(format!("ABORT stroke={stroke_id}"));
                    self.session_gather.remove(&stroke_id);
                    if matches!(self.session, net::Session::Hosting(_)) {
                        self.session_seq += 1;
                        wire_frames.push(net::enc_end(&net::StrokeEndWire {
                            author: me.clone(),
                            stroke_id,
                            seq: self.session_seq,
                            ok: false,
                        }));
                    } else {
                        wire_frames.push(net::enc_end(&net::StrokeEndWire {
                            author: me.clone(),
                            stroke_id,
                            seq: 0,
                            ok: false,
                        }));
                    }
                }
            }
        }
        match &mut self.session {
            net::Session::Hosting(h) => {
                if let Some(p) = &presence {
                    h.send_presence(p);
                }
                for f in wire_frames {
                    h.send_stroke_frame(f);
                }
                // v0.2.2: the finished document goes to exactly the
                // clients that asked (joiners + resync requesters).
                if let Some(b) = &snapshot {
                    for c in self.session_snap_pending.drain(..) {
                        net::slog(format!(
                            "SNAPSHOT sent to client {c} as_of={}",
                            self.session_snap_seq
                        ));
                        h.send_snapshot_to(c, self.session_snap_seq, b);
                    }
                }
            }
            net::Session::Joined(c) => {
                if let Some(p) = &presence {
                    c.send_presence(p);
                }
                for f in wire_frames {
                    c.send_stroke_frame(f);
                }
                if let Some(redo) = history_req {
                    c.send_undo(&me, redo);
                }
            }
            net::Session::Idle => {}
        }
        // (v0.2.1: the replay_done drains moved to the TOP of the pump —
        // strokes committed last frame take their numbers before this
        // frame's arrivals are processed, so the wire's seq order
        // matches the host's document order.)
        // SESSION PERF 2 drain: a snapshot parsed on the worker swaps in
        // HERE (every frame, not only when new events arrive).
        // v0.2.1 (audit): mid-stroke it WAITS in the channel — dropping
        // it silently hung the guest until the next resync. It swaps on
        // the first pen-idle frame (never under a live gesture).
        let mid_stroke = self
            .editor
            .as_ref()
            .is_some_and(|ed| ed.canvas.stroke_active());
        if !mid_stroke
            && let Some(rx) = &self.session_load_rx
            && let Ok(parsed) = rx.try_recv()
        {
            self.session_load_rx = None;
            {
                match parsed {
                    Ok(mut st) => {
                        st.file_path = None; // never ours to save over
                        // STAGE 1: the meta says which sequenced events
                        // this document already contains. One older than
                        // what we've applied would step the lane BACKWARD
                        // — refuse it and ask for a fresh truth instead.
                        let meta = self.session_snapshot_seq.take().unwrap_or(0);
                        if meta < self.session_applied_seq {
                            net::slog(format!(
                                "SNAPSHOT stale as_of={meta} applied={} — re-requesting",
                                self.session_applied_seq
                            ));
                            if let net::Session::Joined(c) = &mut self.session {
                                c.send_resync_request();
                            }
                            // v0.2.6: the lane is unfrozen until the
                            // fresh meta arrives — let it flow.
                            self.drain_session_buffer();
                        } else {
                            let same_size = self.editor.as_ref().is_some_and(|ed| {
                                ed.state.engine.project.width == st.engine.project.width
                                    && ed.state.engine.project.height
                                        == st.engine.project.height
                            });
                            // v0.2.1 (audit): the id HIGH-WATER survives
                            // the swap — rewinding to the partition base
                            // re-minted already-used ids.
                            let prev_next = self
                                .editor
                                .as_ref()
                                .map(|ed| ed.state.engine.project.next_id)
                                .unwrap_or(0);
                            if same_size {
                                let ed = self.editor.as_mut().unwrap();
                                let frame = ed
                                    .state
                                    .view
                                    .frame
                                    .min(st.frame_count().saturating_sub(1));
                                ed.state = st;
                                ed.state.view.frame = frame;
                                ed.canvas.engine_changed();
                                ed.state.status = "live from the host".into();
                            } else {
                                let rs = self.render_state.as_ref();
                                let mut ed = Editor::from_state(st, rs, self.data_dir.as_deref());
                                ed.stage = self.editor.as_ref().and_then(|o| o.stage);
                                ed.canvas.engine_changed();
                                ed.state.status = "live from the host".into();
                                self.editor = Some(ed);
                            }
                            // v0.2.1 (audit): guest_ready is set on the
                            // editor that SURVIVES the swap — setting it
                            // before the branch left a replacement
                            // editor frozen forever.
                            if let Some(ed) = &mut self.editor {
                                ed.canvas.guest_ready = true;
                                // Queued replays predate this document
                                // (their seqs are inside it) — replaying
                                // them would double-apply. Their overlay
                                // entries drop with them.
                                let stale: Vec<u64> = ed
                                    .canvas
                                    .replay_inbox
                                    .iter()
                                    .filter_map(|t| match t {
                                        canvas::SeqTask::Stroke(r) => Some(r.stroke_id),
                                        _ => None,
                                    })
                                    .collect();
                                for sid in stale {
                                    ed.canvas.remote_wet_end.push(sid);
                                }
                                ed.canvas.replay_inbox.clear();
                            }
                            self.session_audit.clear();
                            self.session_done.clear();
                            // STAGE 3: this guest allocates ids in its
                            // own 2^48 partition — parallel creations
                            // can never collide with the host's or
                            // another guest's.
                            if let (net::Session::Joined(c), Some(ed)) =
                                (&self.session, &mut self.editor)
                            {
                                let base = c.peer_id << 48;
                                let next = &mut ed.state.engine.project.next_id;
                                *next = (*next).max(base).max(prev_next);
                            }
                            // The fresh engine lost the base author —
                            // the pump re-arms it next frame.
                            self.session_author_armed = false;
                            // Resume the lane exactly where the document
                            // leaves off, then apply what buffered.
                            self.session_applied_seq = meta;
                            net::slog(format!("SNAPSHOT applied as_of={meta}"));
                            self.drain_session_buffer();
                            self.session_status = "file synced".into();
                        }
                    }
                    Err(e) => {
                        // v0.2.1 (audit): a failed parse was silent — the
                        // joining guest just hung. Log + ask again.
                        net::slog(format!("SNAPSHOT parse refused: {e} — re-requesting"));
                        self.session_status = format!("snapshot refused: {e}");
                        // v0.2.6: the meta paired with THIS failed doc
                        // must not freeze the lane or mislabel the next.
                        self.session_snapshot_seq = None;
                        if let net::Session::Joined(c) = &mut self.session {
                            c.send_resync_request();
                        }
                        self.drain_session_buffer();
                    }
                }
            }
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
        data_dir: Option<std::path::PathBuf>,
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
            let (path, loaded) = restore_handover(&s)?;
            match loaded {
                Ok(state) => {
                    devloop::Session::clear_pending();
                    Some(Editor::from_state(
                        state,
                        cc.wgpu_render_state.as_ref(),
                        data_dir.as_deref(),
                    ))
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
            data_dir,
            editor: restored,
            new_form: None,
            welcome: None,
            update_rx: {
                // The dev channel wins: an armed dev build never
                // self-updates (NEVER-DO 4). Sweep any stepped-aside
                // binary — this launch worked (NEVER-DO 1).
                crate::update::sweep_old();
                net::slog_init();
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
            update_staged: None,
            update_next_check: 0.0,
            update_note: String::new(),
            session_last_snap: 0.0,
            session_last_presence: 0.0,
            frame_watch: None,
            dbg_ship_last: 0.0,
            stats_window: (0.0, 0, 0.0),
            session_seq: 0,
            session_applied_seq: 0,
            session_snapshot_seq: None,
            session_snap_seq: 0,
            session_buffer: Vec::new(),
            session_gather: std::collections::HashMap::new(),
            session_res: std::collections::HashMap::new(),
            session_res_announced: std::collections::HashSet::new(),
            session_audit: std::collections::HashMap::new(),
            session_done: std::collections::HashMap::new(),
            session_author_armed: false,
            session_host_layout: None,
            layout_ws_name: String::new(),
            session_snap_targets: std::collections::HashSet::new(),
            session_snap_pending: Vec::new(),
            session_ev_hold: Vec::new(),
            session_bank_json: None,
            session_presets: Vec::new(),
            stack_room_seen: "",
            discovery: net::spawn_discovery(),
            session_snap_rx: None,
            session_load_rx: None,
            settings_category: SettingsCategory::default(),
            capturing: None,
            backend,
            tablet_rx: None,
            tablet_boot_tried: false,
            notes: Notes::default(),
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
            worksafe_started: false,
            running: worksafe::RunningRecord::default(),
            recovery: Vec::new(),
            recovery_clock: (0, 0),
            doc_seen: None,
        }
    }

    /// GM2, once per frame: the start-up crash check (first frame), the
    /// editor's requests, the recent list and the running record.
    fn worksafe_shell(&mut self, ctx: &egui::Context) {
        let Some(dir) = self.data_dir.as_deref() else { return };
        if !self.worksafe_started {
            self.worksafe_started = true;
            // Other copies' records BEFORE writing this one's. A live copy
            // (a second window) is never offered; a dead one that left no
            // newer autosave has nothing to say, so its record goes.
            for offer in worksafe::dead_sessions(dir, &worksafe::process_alive) {
                if offer.autosaves.is_empty() {
                    let _ = std::fs::remove_file(&offer.record_path);
                } else {
                    self.recovery.push(offer);
                }
            }
            self.recovery_clock = (worksafe::unix_now(), worksafe::utc_offset_min());
            self.running = worksafe::RunningRecord {
                pid: std::process::id(),
                open: None,
                autosave: None,
                started: worksafe::unix_now(),
            };
            let _ = self.running.write_in(dir);
        }
        let mut requests = Vec::new();
        if let Some(ed) = &mut self.editor {
            requests = std::mem::take(&mut ed.requests);
            ed.autosave.minutes = self.config.session.autosave_minutes;
            // GM11: the render queue's settings, its worker read every frame
            // (the pane shown or not), and its "show folder" opened here.
            ed.state
                .render_queue
                .configure(&self.config.render, &self.config.session.username);
            render_queue::poll(&mut ed.state, ctx);
            if let Some(dir) = ed.state.render_queue.show_folder.take() {
                proc::show_folder(&dir);
            }
            let mut recent_changed = match ed.recent_edit.take() {
                Some(RecentEdit::Remove(p)) => {
                    self.config.session.recent.retain(|r| r != &p);
                    true
                }
                Some(RecentEdit::Clear) => {
                    self.config.session.recent.clear();
                    true
                }
                None => false,
            };
            recent_changed |= worksafe::follow_recent(
                &mut self.doc_seen,
                ed.state.file_path.as_deref(),
                &mut self.config.session,
            );
            if recent_changed {
                self.config.save();
            }
            ed.set_recent(&self.config.session.recent);
            // The record is rewritten only when what it says changes.
            if self.running.open != ed.state.file_path
                || self.running.autosave != ed.last_autosave
            {
                self.running.open = ed.state.file_path.clone();
                self.running.autosave = ed.last_autosave.clone();
                let _ = self.running.write_in(dir);
            }
        }
        for r in requests {
            self.handle_request(ctx, r);
        }
        self.recovery_modal(ctx);
    }

    /// GM2: the crash-recovery offer — its autosaves as rows (the newest
    /// picked), then Open recovered / Discard / Later. Open goes through
    /// the editor's unsaved guard: it never drops a newer document unasked.
    fn recovery_modal(&mut self, ctx: &egui::Context) {
        let Some(offer) = self.recovery.first_mut() else { return };
        let (now, offset) = self.recovery_clock;
        let choice = egui::Modal::new(egui::Id::new("gm2_recovery"))
            .show(ctx, |ui| worksafe::recovery_ui(ui, offer, now, offset))
            .inner;
        let Some(choice) = choice else { return };
        let offer = self.recovery.remove(0);
        match choice {
            worksafe::RecoveryChoice::Open(r) => match &mut self.editor {
                Some(ed) => ed.leave(Leave::Recover(r), ctx),
                None => match worksafe::open_recovered(&r) {
                    Ok(state) => {
                        self.editor = Some(Editor::from_state(
                            state,
                            self.render_state.as_ref(),
                            self.data_dir.as_deref(),
                        ));
                        self.new_form = None;
                        let _ = std::fs::remove_file(&r.record);
                    }
                    // No editor to say it in: the offer comes back next
                    // start (its record stays).
                    Err(e) => eprintln!("recovery: could not open the autosave — {e}"),
                },
            },
            worksafe::RecoveryChoice::Discard => {
                // (Offers exist only with a data folder: the shell found them there.)
                if let Some(dir) = &self.data_dir {
                    worksafe::discard_recovery(dir, &offer);
                }
            }
            // The record stays: the next start offers it again.
            worksafe::RecoveryChoice::Later => {}
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
                    canvas::CanvasTool::LassoFill => "lasso fill",
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
            // (The relaunch exits on success; it returns only on failure.)
            self.retire_running();
            let why = self.devloop.relaunch();
            self.rearm_running();
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
        // GM2's modals too: a pending "Save changes?" or recovery offer is
        // answered before the copy that asked goes away.
        if self.settings_open
            || self.new_form.is_some()
            || self.welcome.is_some()
            || editor.leave_pending()
            || !self.recovery.is_empty()
        {
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
        self.retire_running();
        let why = self.devloop.relaunch();
        self.rearm_running();
        self.devloop.note = Some(format!("⚠ relaunch failed: {why}"));
        self.devloop.give_up();
    }

    /// Collect this frame's native pen samples from the tablet thread.
    fn pump_tablet(&mut self) -> Vec<PenSample> {
        pump_tablet(&mut self.tablet_rx, &mut self.notes)
    }

    /// Run an action bound to a keyboard shortcut: the editor performs it
    /// (the one dispatch, D6); what only the shell can do comes back as a
    /// request and is carried out here.
    fn perform(&mut self, ctx: &egui::Context, action: Action) {
        let performed = match &mut self.editor {
            Some(ed) => {
                ed.presets.clone_from(&self.config.presets);
                ed.perform(action)
            }
            None => match action {
                Action::NewProject => Performed::App(AppRequest::NewProject),
                Action::Settings => Performed::App(AppRequest::Settings),
                Action::Quit => Performed::App(AppRequest::Quit),
                _ => Performed::Ignored,
            },
        };
        if let Performed::App(req) = performed {
            self.handle_request(ctx, req);
        }
    }

    /// The shell-level verbs: dialogs, the file pickers, the window.
    fn handle_request(&mut self, ctx: &egui::Context, req: AppRequest) {
        match req {
            // GM2: New/Open drop the open document — the guard asks first.
            AppRequest::NewProject => match &mut self.editor {
                Some(ed) => ed.leave(Leave::New, ctx),
                None => self.new_form = Some(NewProjectForm::default()),
            },
            AppRequest::Settings => self.settings_open = true,
            // The close request itself is guarded (Editor::worksafe_frame).
            AppRequest::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            AppRequest::Save | AppRequest::SaveAs | AppRequest::Open => {
                let Some(ed) = &mut self.editor else { return };
                match req {
                    AppRequest::Save => ed.state.save(false),
                    AppRequest::SaveAs => ed.state.save(true),
                    _ => ed.leave(Leave::Open, ctx),
                }
            }
        }
    }
}

/// What [`Editor::perform`] did with an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Performed {
    /// The editor carried it out.
    Done,
    /// Nothing changed: refused (the reason is said on the Aka lane — mid-
    /// stroke, the host owns the file), blocked mid-stroke, or an empty slot.
    Ignored,
    /// Only the shell can do this (dialogs, the window).
    App(AppRequest),
}

/// The shell-level verbs an action can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRequest {
    NewProject,
    Open,
    Save,
    SaveAs,
    /// GM3: raised by the menus, the palette and their chords.
    Settings,
    Quit,
}

impl Editor {
    /// THE one dispatch for every [`Action`] (D6): shortcuts now, menus and
    /// the command palette later all land here. Editor state only — the
    /// file dialogs and the window belong to the shell, so those come back
    /// as [`Performed::App`].
    pub fn perform(&mut self, action: Action) -> Performed {
        let seq = self.state.refusal_seq;
        let mut out = Performed::Done;
        self.perform_into(action, &mut out);
        // A refusal anywhere down the verb (set_tool mid-stroke, a locked
        // layer) means it was not done.
        if out == Performed::Done && self.state.refusal_seq != seq {
            out = Performed::Ignored;
        }
        // GM3: the palette lists recent commands first, however they ran
        // (a shortcut, a menu or the palette itself).
        if out != Performed::Ignored {
            self.palette.note_run(action);
            // GM8: the first run of a Quick start step's verb ticks it.
            welcome::tick(&mut self.quick_start, action);
        }
        out
    }

    fn perform_into(&mut self, action: Action, out: &mut Performed) {
        if action == Action::NewProject {
            *out = Performed::App(AppRequest::NewProject);
            return;
        }
        // STROKE GUARD: while a stroke is live, actions that would retarget or
        // orphan its pen-up commit are dropped — frame nav would commit onto
        // the wrong frame, an A-cycle onto the wrong layer, undo/clear against
        // a vanished target. (Tool toggles are safe: the stroke latches them.)
        // GM3: checked before the chrome's verbs — nothing modal opens and
        // the dock is not rebuilt under a live stroke (D7).
        if self.canvas.stroke_active() && stroke_guarded(action) {
            self.state.refuse("refused — finish the stroke first");
            *out = Performed::Ignored;
            return;
        }
        // GM3: the chrome's own verbs — windows, panes and rooms; the
        // document is untouched.
        match action {
            Action::Settings => *out = Performed::App(AppRequest::Settings),
            Action::Quit => *out = Performed::App(AppRequest::Quit),
            Action::CommandPalette => self.palette.toggle(),
            Action::ShortcutSheet => self.help.sheet = !self.help.sheet,
            Action::QuickStart => self.help.quick_start = !self.help.quick_start,
            Action::About => self.help.about = !self.help.about,
            Action::History => self.open_pane(Pane::History),
            // GM14: the graph editor, its framing, and typing on the sheet
            // cursor's cell (the sheet comes forward and its entry takes
            // the keys on its next frame).
            Action::ShowCurves => self.open_pane(Pane::Curves),
            // GM7: the paint check pane, and the colour-law migration's
            // DANGER confirm (the convert itself runs from the confirm).
            Action::ShowPaintCheck => self.open_pane(Pane::PaintCheck),
            Action::ConvertColorsToSrgb => self.state.convert_confirm = true,
            // GM8: the project's settings window (its Apply is the edit).
            Action::ProjectSettings => {
                self.project_form = Some(ProjectForm::of(&self.state.engine.project));
            }
            // GM8: the Welcome screen again, past the unsaved guard (a
            // choice on it may open another document); the shell shows it.
            Action::WelcomeScreen => {
                if !self.hold_leave(Leave::Welcome) {
                    self.request_welcome = true;
                }
            }
            // GM10: the cut's paper window (its "set paper" is the edit).
            Action::PaperSize => self.paper_form = Some(PaperForm::of(&self.state)),
            Action::FrameCurves => curves_panel::frame_stored(&mut self.state),
            Action::TypeIntoCell => {
                self.open_pane(Pane::XSheet);
                self.state.sheet_ui.ask_entry();
            }
            // GM9: the graph comes forward and opens its add-node menu on
            // its next frame (the node it adds is the edit).
            Action::AddNode => {
                self.open_pane(Pane::NodeGraph);
                self.state.graph_ui.ask_add_menu();
            }
            Action::ResetLayout => {
                let st = self.stage.unwrap_or(Stage::Drawing);
                self.dock = st.dock();
                self.state.status = format!("layout reset: {}", st.name());
            }
            Action::ZoomFit => self.canvas.fit_view(),
            // GM4: the view verbs move the view only, never the paper.
            Action::ZoomIn => self.canvas.zoom_in(),
            Action::ZoomOut => self.canvas.zoom_out(),
            Action::Zoom100 => self.canvas.zoom_100(),
            Action::RotateViewLeft => self.canvas.rotate_view(-canvas::ROTATE_STEP_DEG),
            Action::RotateViewRight => self.canvas.rotate_view(canvas::ROTATE_STEP_DEG),
            Action::ResetRotation => self.canvas.set_view_rotation(0.0),
            Action::MirrorView => self.canvas.toggle_mirror(),
            Action::RoomLayout => self.enter_room(Stage::Layout),
            Action::RoomDrawing => self.enter_room(Stage::Drawing),
            Action::RoomFinishing => self.enter_room(Stage::InkPaint),
            Action::RoomComposite => self.enter_room(Stage::Composite),
            Action::RoomEdit => self.enter_room(Stage::Edit),
            _ => {}
        }
        if is_chrome(action) {
            return;
        }
        let ed = self;
        // Handled on the canvas (owns the tool state), before borrowing ed.state.
        if action == Action::ToggleEraser {
            ed.canvas.toggle_eraser();
            return;
        }
        if action == Action::ToggleAlphaLock {
            ed.canvas.toggle_alpha_lock();
            return;
        }
        // GM4: `[` / `]` step the brush size — never mid-stroke (the live
        // stroke's dabs would change size under the pen), like a preset.
        if matches!(action, Action::BrushSmaller | Action::BrushBigger) {
            if !ed.canvas.stroke_active() {
                ed.canvas.brush_size_step(action == Action::BrushBigger);
            }
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
        // GM3: the deck's lasso fill, a toggle like the flood fill.
        if action == Action::LassoFillTool {
            let next = if ed.canvas.tool == canvas::CanvasTool::LassoFill {
                canvas::CanvasTool::Paint
            } else {
                canvas::CanvasTool::LassoFill
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
        // GM6: the pixel clipboard reads the canvas's selection.
        if action == Action::CopyPixels {
            match ed.canvas.selection_poly() {
                Some(poly) => {
                    if !ed.state.copy_pixels(&poly) {
                        *out = Performed::Ignored;
                    }
                }
                None => {
                    ed.state.refuse("refused — select pixels first, then copy");
                    *out = Performed::Ignored;
                }
            }
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
            } else {
                *out = Performed::Ignored;
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
                *out = Performed::Ignored; // until a pot is armed
                if nchars > 0 {
                    let ci = ed.state.active_character.min(nchars - 1);
                    if let Some(role) = ed.state.palettes.characters[ci].roles.get(i).cloned() {
                        ed.canvas.brush_color = role.color;
                        // GM7: the pot it armed is the active chip.
                        ed.state.active_chip = Some(i);
                        ed.state.status = format!(
                            "pot: {} · {}",
                            ed.state.palettes.characters[ci].name, role.name
                        );
                        *out = Performed::Done;
                    }
                }
                return;
            }
            if !ed.canvas.stroke_active()
                && let Some(p) = ed.presets.get(i)
            {
                ed.canvas.apply_preset(p);
                ed.state.status = format!("brush: {}", p.name);
            } else {
                *out = Performed::Ignored;
            }
            return;
        }
        // Save/Open land any in-flight gesture FIRST: the saved file must
        // contain what the screen shows, and an Open's commit must not cross
        // into the newly loaded project.
        if matches!(action, Action::Save | Action::SaveAs | Action::Open) {
            ed.canvas.finish_gesture(&mut ed.state);
        }
        // GM3: with a selection, Delete (ClearCel) clears only inside it.
        let selection = match action {
            Action::ClearCel => ed.canvas.selection_poly(),
            _ => None,
        };
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
            Action::ClearCel => match &selection {
                Some(poly) => s.clear_current_region(poly),
                None => s.clear_current_raster(),
            },
            Action::ClearCelAll => s.clear_current_cel_all(),
            Action::ClearFrameKey => s.clear_key_at_frame(),
            Action::RemoveColumn => s.remove_active_column(),
            // GM5: the sheet verbs, on the sheet cursor (the current cell,
            // or the active column from the cursor down).
            Action::InsertFrames => s.insert_frame(),
            Action::DeleteFrames => s.delete_frame(),
            Action::HoldPlus => s.hold(1),
            Action::HoldMinus => s.hold(-1),
            Action::RetimeOnes => s.retime(1),
            Action::RetimeTwos => s.retime(2),
            Action::RetimeThrees => s.retime(3),
            Action::Reverse => s.reverse_timing(),
            Action::ColumnKindCel => s.set_column_kind(ColumnKind::Cel),
            Action::ColumnKindBg => s.set_column_kind(ColumnKind::Bg),
            Action::ColumnKindBook => s.set_column_kind(ColumnKind::Book),
            Action::ColumnKindRef => s.set_column_kind(ColumnKind::Ref),
            Action::ToggleColumnHidden => s.toggle_column_hidden(),
            Action::ToggleColumnLocked => s.toggle_column_locked(),
            Action::CopyCells => {
                s.copy_cells();
            }
            Action::CutCells => s.cut_cells(),
            Action::PasteCells => s.paste_cells(false),
            Action::PasteCellsInsert => s.paste_cells(true),
            // GM14: the curves pane's key verbs, on its selected keys.
            Action::InterpHold
            | Action::InterpLinear
            | Action::InterpEase
            | Action::InterpEaseIn
            | Action::InterpEaseOut
            | Action::InterpBezier
            | Action::HandlesAuto
            | Action::HandlesSmooth
            | Action::HandlesFlat
            | Action::HandlesBroken => curves_panel::key_verb(s, action),
            Action::ToggleOnion => s.view.onion = !s.view.onion,
            // GM6: the genga-to-douga verbs, on this frame's drawing.
            Action::PastePixels => s.paste_pixels(),
            Action::DuplicateCel => s.duplicate_cel(),
            Action::MakeInbetweens => {
                s.make_inbetweens();
            }
            // GM8: the auto-comp — the whole graph in one undo step (a
            // graph shaped by hand asks first, DANGER).
            Action::BuildComposite => s.request_auto_comp(),
            Action::KindKey
            | Action::KindBreakdown
            | Action::KindInbetween
            | Action::KindUnset => match s.current_drawing() {
                Some(d) => {
                    let kind = match action {
                        Action::KindKey => anim_core::model::DrawingKind::Key,
                        Action::KindBreakdown => anim_core::model::DrawingKind::Breakdown,
                        Action::KindInbetween => anim_core::model::DrawingKind::Inbetween,
                        _ => anim_core::model::DrawingKind::Unset,
                    };
                    s.set_drawing_kind(d, kind);
                }
                None => s.refuse("refused — no drawing on this frame to mark"),
            },
            Action::PinToLightTable => match s.current_drawing() {
                Some(d) => {
                    let col = s.view.active_column;
                    let on = s.view.light.toggle(col, d);
                    s.status = if on { "pinned to the light table" } else { "unpinned" }.into();
                }
                None => s.refuse("refused — no drawing on this frame to pin"),
            },
            Action::ShiftTrace => s.view.light.shift_trace = !s.view.light.shift_trace,
            Action::ResetShift => s.view.light.reset(),
            Action::CopyPixels => {}
            Action::ToggleEraser
            | Action::ToggleAlphaLock
            | Action::ToggleCompositeView
            | Action::SelectTool
            | Action::BrushTool
            | Action::FillTool
            | Action::LassoFillTool
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
                } else if ed.canvas.session_live {
                    // STAGE 3: hosting a room, undo is per-artist for
                    // the host too — routed through the same sequenced
                    // path a guest's request takes (one law, every hand).
                    ed.canvas.history_request = Some(redo);
                    return;
                } else if redo {
                    s.redo();
                } else {
                    s.undo();
                }
            }
            // OWNER RULING 2026-08-19: a guest is a 1:1 control window —
            // the HOST owns the file; guest strokes already live there.
            Action::Save | Action::SaveAs if ed.canvas.is_guest => {
                s.refuse("refused — the host owns the file; your strokes already live there");
                *out = Performed::Ignored;
            }
            Action::Open if ed.canvas.is_guest => {
                s.refuse("refused — leave the room first (Settings › Session) to open your own file");
                *out = Performed::Ignored;
            }
            // The file dialogs block the window: the shell runs them.
            Action::Save => *out = Performed::App(AppRequest::Save),
            Action::SaveAs => *out = Performed::App(AppRequest::SaveAs),
            Action::Open => *out = Performed::App(AppRequest::Open),
            Action::NewProject => {}
            Action::CommandPalette
            | Action::ShortcutSheet
            | Action::QuickStart
            | Action::About
            | Action::History
            | Action::Settings
            | Action::Quit
            | Action::ResetLayout
            | Action::ZoomFit
            | Action::ZoomIn
            | Action::ZoomOut
            | Action::Zoom100
            | Action::RotateViewLeft
            | Action::RotateViewRight
            | Action::ResetRotation
            | Action::MirrorView
            | Action::RoomLayout
            | Action::RoomDrawing
            | Action::RoomFinishing
            | Action::RoomComposite
            | Action::RoomEdit
            | Action::ShowCurves
            | Action::FrameCurves
            | Action::ShowPaintCheck
            | Action::ConvertColorsToSrgb
            | Action::ProjectSettings
            | Action::WelcomeScreen
            | Action::PaperSize
            | Action::TypeIntoCell
            | Action::AddNode => {} // the chrome's, handled above
            Action::BrushSmaller | Action::BrushBigger => {} // the canvas's, above
        }
    }
}

/// GM3: the chrome's own verbs (windows, panes, rooms) — never the
/// document's.
fn is_chrome(action: Action) -> bool {
    matches!(
        action,
        Action::CommandPalette
            | Action::ShortcutSheet
            | Action::QuickStart
            | Action::About
            | Action::History
            | Action::Settings
            | Action::Quit
            | Action::ResetLayout
            | Action::ZoomFit
            | Action::ZoomIn
            | Action::ZoomOut
            | Action::Zoom100
            | Action::RotateViewLeft
            | Action::RotateViewRight
            | Action::ResetRotation
            | Action::MirrorView
            | Action::RoomLayout
            | Action::RoomDrawing
            | Action::RoomFinishing
            | Action::RoomComposite
            | Action::RoomEdit
            | Action::ShowCurves
            | Action::FrameCurves
            | Action::ShowPaintCheck
            | Action::ConvertColorsToSrgb
            | Action::ProjectSettings
            | Action::WelcomeScreen
            | Action::PaperSize
            | Action::TypeIntoCell
            | Action::AddNode
    )
}

/// STROKE GUARD: while a stroke is live, actions that would retarget or
/// orphan its pen-up commit are dropped — frame nav would commit onto the
/// wrong frame, an A-cycle onto the wrong layer, undo/clear against a
/// vanished target. (Tool toggles are safe: the stroke latches them.)
/// GM3: also the palette (a modal) and the layout reset (the dock).
fn stroke_guarded(action: Action) -> bool {
    matches!(
        action,
        Action::CommandPalette
            | Action::ResetLayout
            | Action::PlayPause
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
            // GM5: the sheet never changes under a live stroke.
            | Action::InsertFrames
            | Action::DeleteFrames
            | Action::HoldPlus
            | Action::HoldMinus
            | Action::RetimeOnes
            | Action::RetimeTwos
            | Action::RetimeThrees
            | Action::Reverse
            | Action::ColumnKindCel
            | Action::ColumnKindBg
            | Action::ColumnKindBook
            | Action::ColumnKindRef
            | Action::ToggleColumnHidden
            | Action::ToggleColumnLocked
            // GM14: the cell clipboard edits the sheet too, and so do the
            // curves pane's key verbs.
            | Action::CutCells
            | Action::PasteCells
            | Action::PasteCellsInsert
            | Action::InterpHold
            | Action::InterpLinear
            | Action::InterpEase
            | Action::InterpEaseIn
            | Action::InterpEaseOut
            | Action::InterpBezier
            | Action::HandlesAuto
            | Action::HandlesSmooth
            | Action::HandlesFlat
            | Action::HandlesBroken
            // GM6: the genga-to-douga verbs edit the document too.
            | Action::CopyPixels
            | Action::PastePixels
            | Action::DuplicateCel
            | Action::MakeInbetweens
            | Action::KindKey
            | Action::KindBreakdown
            | Action::KindInbetween
            | Action::KindUnset
            // GM8: nothing modal opens over a live stroke (D7).
            | Action::WelcomeScreen
            // GM9: nor the add-node menu (it brings a pane forward).
            | Action::AddNode
    )
}

/// The pen line the artist hears when the native tablet backend drops out.
const TABLET_LOST: &str = "tablet: native pen input failed — using standard pen input";

/// Drain the tablet thread's samples. A disconnected channel (the backend
/// failed to attach, panicked or ended) drops the backend for the session —
/// egui Touch keeps working — and the artist is told once, on the Aka lane:
/// a release build has no console to read.
fn pump_tablet(
    rx: &mut Option<std::sync::mpsc::Receiver<PenSample>>,
    notes: &mut Notes,
) -> Vec<PenSample> {
    let Some(r) = rx.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    loop {
        match r.try_recv() {
            Ok(s) => out.push(s),
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if cfg!(debug_assertions) {
                    eprintln!("native tablet thread ended — falling back to standard pen input");
                }
                notes.push(TABLET_LOST.to_string());
                *rx = None;
                break;
            }
        }
    }
    out
}

/// What the artist must hear once, said on the editor's Aka lane (D7) as
/// one NOTICE that waits for the artist: the tolerant loads' resets
/// (config.json, workspaces.json) and the tablet fallback. Pending notes
/// join into one line; a note said once this session is not said again —
/// every new project, open or join rebuilds the editor, which re-reads
/// workspaces.json.
#[derive(Default)]
struct Notes {
    pending: Vec<String>,
    said: std::collections::HashSet<String>,
}

impl Notes {
    fn push(&mut self, note: String) {
        if !self.said.contains(&note) && !self.pending.contains(&note) {
            self.pending.push(note);
        }
    }

    /// Say everything pending, the editor's own load notes included.
    fn say(&mut self, ed: &mut Editor) {
        for n in std::mem::take(&mut ed.load_notes) {
            self.push(n);
        }
        if self.pending.is_empty() {
            return;
        }
        ed.state.refuse(self.pending.join(" · "));
        ed.notice_seq = Some(ed.state.refusal_seq);
        self.said.extend(self.pending.drain(..));
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
            // The artist hears of a failure from pump_tablet (the Aka
            // lane); these lines are the developer's detail.
            let Ok(Ok(mut mgr)) = built else {
                if cfg!(debug_assertions) {
                    eprintln!("native tablet backend failed to attach — using fallback pen input");
                }
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
                    if cfg!(debug_assertions) {
                        eprintln!("native tablet backend panicked — disabled for this session");
                    }
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
    /// The data dir workspaces persist into (D11; None = not persisted —
    /// the layout probe).
    data_dir: Option<std::path::PathBuf>,
    /// The brush presets the 1–8 keys arm: the shell's config copy,
    /// refreshed before every [`Editor::perform`].
    presets: Vec<BrushPreset>,
    state: AppState,
    canvas: canvas::CanvasView,
    /// GPU raster paint layer (None if wgpu is unavailable).
    paint: Option<PaintLayer>,
    /// GPU graph compositor — the composite-view render path (None like paint).
    graph: Option<graphcomp::GraphCompositor>,
    /// Set when the user clicks "New…"; the App picks it up to show the dialog.
    request_new: bool,
    /// GM8: File ▸ Welcome screen… was answered; the App shows the screen.
    request_welcome: bool,
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
    /// The refusal seq of a NOTICE ([`Notes::say`]): it holds the Aka lane
    /// until the artist's first key or click, then dwells as usual.
    notice_seq: Option<u32>,
    /// What this editor's workspaces.json load dropped, not yet said (the
    /// shell's [`Notes`] takes it on the next frame).
    load_notes: Vec<String>,
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
    /// GM10: the Paper size window's draft, while it is open.
    pub paper_form: Option<PaperForm>,
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
    /// GM2: what the artist asked for that would drop unsaved work — the
    /// "Save changes?" modal shows while this is Some.
    leave: Option<Leave>,
    /// GM2: "Save" was chosen in that modal — resume this once it lands.
    leave_after_save: Option<Leave>,
    /// GM2: the close was answered; the next close request goes through.
    pub close_ok: bool,
    /// GM2: requests for the shell (Save As from the modal); drained each
    /// frame by the App (the testkit's rig logs them).
    pub requests: Vec<AppRequest>,
    /// GM2: the recent files (newest first), mirrored from the config
    /// (`set_recent`).
    recent: Vec<std::path::PathBuf>,
    /// GM2: the recent files with their "still on disk" flags, checked when
    /// the list changes and when the file menu opens — never every frame.
    recent_shown: Vec<(std::path::PathBuf, bool)>,
    /// GM2: the file menu was open last frame (its opening re-checks disk).
    file_menu_open: bool,
    /// GM2: an edit the artist made to the recent list — the shell applies it.
    pub recent_edit: Option<RecentEdit>,
    /// GM2: when the next autosave is due (the shell sets `minutes`).
    pub autosave: worksafe::AutosaveClock,
    autosave_job: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    /// GM2: the last autosave written for this document (for the running
    /// record).
    pub last_autosave: Option<std::path::PathBuf>,
    /// GM2: when it was written (Unix seconds; the slate dot's hover).
    last_autosave_at: Option<u64>,
    /// GM2: autosaves are failing — said once on the Aka lane per streak.
    autosave_failing: bool,
    /// GM2: what the window title was last made from (dirty, file, name);
    /// it is rebuilt and sent only when one of them changes.
    title_from: Option<(bool, Option<std::path::PathBuf>, String)>,
    /// GM3: the command palette (Ctrl+K).
    pub palette: cmdpal::Palette,
    /// GM3: the help windows (F1's shortcut sheet, quick start, about).
    pub help: menus::HelpWindows,
    /// GM3: where each pane was drawn this frame — the pane under the
    /// pointer picks which shortcuts apply.
    pane_rects: Vec<(Pane, egui::Rect)>,
    /// GM8: the Quick start steps done this session (the App ORs them
    /// into `config.ui.quick_start`).
    pub quick_start: [bool; 5],
    /// GM8: the Project Settings window's draft, while it is open.
    pub project_form: Option<ProjectForm>,
}

/// GM10: the Paper size window's draft — the paper being typed for the cut
/// it was opened on (a cut switch re-reads it, so one cut's typing never
/// lands on another).
pub struct PaperForm {
    pub cut: anim_core::command::CutRef,
    pub rect: anim_core::camera::IRect,
}

impl PaperForm {
    fn of(s: &AppState) -> Self {
        let (w, h) = (s.engine.project.width, s.engine.project.height);
        Self {
            cut: s.at(),
            rect: anim_core::camera::paper(s.cut(), w, h),
        }
    }
}

/// GM8: the Project Settings window's draft (applied as one command).
pub struct ProjectForm {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub cut_frames: u32,
    pub dpi: f32,
}

impl ProjectForm {
    fn of(p: &anim_core::model::Project) -> Self {
        Self {
            name: p.name.clone(),
            width: p.width,
            height: p.height,
            fps: p.fps,
            cut_frames: p.cut_frames(),
            dpi: p.dpi,
        }
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// GM8 start-up route: the Welcome screen unless its latch is off (then
/// the New Project form, as before).
fn startup_shows_welcome(config: &Config) -> bool {
    config.ui.show_welcome
}

/// GM8: what the shell does about its start-up screens this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Startup {
    /// A cut, a form or the screen is up: leave it.
    Keep,
    Welcome,
    NewForm,
    /// A cut opened under the start-up screen (a restore, a card): drop it.
    DropWelcome,
}

/// GM8: the start-up route from the facts alone — nothing open and no
/// dialog shows the Welcome screen (or the New Project form with its latch
/// off); a cut opening drops the start-up screen, but not one asked for
/// over the cut (File ▸ Welcome screen…).
fn startup_route(
    editor: bool,
    form: bool,
    welcome: Option<&welcome::Welcome>,
    config: &Config,
) -> Startup {
    match welcome {
        None if !editor && !form => {
            if startup_shows_welcome(config) {
                Startup::Welcome
            } else {
                Startup::NewForm
            }
        }
        Some(w) if editor && !w.over_editor => Startup::DropWelcome,
        _ => Startup::Keep,
    }
}

/// GM8: the UI scale (Settings ▸ General) as egui's zoom factor.
fn apply_ui_scale(ctx: &egui::Context, config: &Config) {
    ctx.set_zoom_factor(config.ui.zoom());
}

/// GM8: OR this session's Quick start ticks into the config; true when
/// one is new (the caller saves).
fn merge_quick_start(config: &mut Config, ticks: &[bool; 5]) -> bool {
    let mut changed = false;
    for (c, t) in config.ui.quick_start.iter_mut().zip(ticks) {
        if *t && !*c {
            *c = true;
            changed = true;
        }
    }
    changed
}

/// GM8: Remove on a Welcome recent card — off the config's list and the
/// screen (the caller saves the config; the file is never touched).
fn forget_recent(config: &mut Config, w: &mut welcome::Welcome, path: &std::path::Path) {
    config.session.recent.retain(|p| p != path);
    w.forget(path);
}

/// GM8: a template's project in its first room.
fn open_template(i: usize, rs: Option<&RenderState>, data_dir: Option<&std::path::Path>) -> Editor {
    let t = &templates::ALL[i.min(templates::ALL.len() - 1)];
    let mut editor = Editor::from_state(t.build(), rs, data_dir);
    editor.enter_room(t.room);
    editor
}

/// GM8: "Show me" on the Welcome screen — the sample, its empty column B
/// active on frame 0 (a new drawing needs an empty cell), then the step.
fn welcome_show_me(
    step: usize,
    rs: Option<&RenderState>,
    data_dir: Option<&std::path::Path>,
) -> (Editor, Performed) {
    let mut editor = Editor::from_state(sample::build_practice(), rs, data_dir);
    let did = editor.show_me(step);
    (editor, did)
}

/// GM2: an action that drops the open document — asked about first when
/// the document has unsaved changes.
#[derive(Debug, Clone, PartialEq)]
pub enum Leave {
    Close,
    New,
    Open,
    OpenPath(std::path::PathBuf),
    /// The recovery dialog's "Open recovered" over a document that has
    /// work of its own (a handover restore, say): asked about like an Open.
    Recover(worksafe::Recovered),
    /// GM8: File ▸ Welcome screen… — its cards open other documents.
    Welcome,
}

/// GM2: an edit to the recent list from the Open Recent menu.
#[derive(Debug, Clone, PartialEq)]
pub enum RecentEdit {
    Remove(std::path::PathBuf),
    Clear,
}

/// GM2: the window title — `●` leads it exactly while there are unsaved
/// changes.
pub fn window_title(state: &AppState) -> String {
    let name = state
        .file_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| state.engine.project.name.clone());
    if state.is_dirty() {
        format!("● {name} — Gaban Studio")
    } else {
        format!("{name} — Gaban Studio")
    }
}

/// The document a devloop / self-update handover carried across, as the
/// restart reopens it: (the carried copy, the loaded document). None when
/// the session names no document.
fn restore_handover(
    s: &devloop::Session,
) -> Option<(std::path::PathBuf, Result<AppState, String>)> {
    let path = s.project.clone()?;
    let loaded = AppState::load_from(path.clone()).map(|mut state| {
        // The autosave is scaffolding. The document's REAL save target is
        // carried across in `origin` and restored unconditionally: `None`
        // for a document that never had a file (Save prompts), the true path
        // otherwise. Deriving this from a flag was the critical defect — a
        // real project came back pointing at %APPDATA% scratch storage and
        // the next Ctrl+S wrote there.
        state.file_path = s.origin.clone();
        let last = state.frame_count().saturating_sub(1);
        state.goto(s.frame.min(last));
        state.status = "restored after rebuild".into();
        // GM2: whether the carried copy matched the real file is not known
        // here, so it reads unsaved: a false "unsaved" costs one question at
        // close, a false "saved" closes over the work without asking.
        state.meta_dirty = true;
        state
    });
    Some((path, loaded))
}

/// GM2: an Open Recent entry — the file's name, then its folder's.
fn recent_label(p: &std::path::Path) -> String {
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match p.parent().and_then(|d| d.file_name()) {
        Some(dir) => format!("{name} — {}", dir.to_string_lossy()),
        None => name,
    }
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
    /// GM3: where each pane is drawn (the shortcut context reads it).
    pane_rects: &'a mut Vec<(Pane, egui::Rect)>,
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
        self.pane_rects.push((*tab, ui.max_rect()));
        match tab {
            Pane::History => {
                let gesture_safe = !self.canvas.stroke_active();
                let shared = self.canvas.is_guest || self.canvas.session_live;
                history_panel::ui(ui, self.state, gesture_safe, shared)
            }
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
                     (the window menu can reopen it)",
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
            Pane::Curves => {
                let gesture_safe = !self.canvas.stroke_active();
                curves_panel::ui(ui, self.state, gesture_safe)
            }
            Pane::PaintCheck => paintcheck::ui(ui, self.state, self.canvas),
            Pane::RenderQueue => render_queue::ui(ui, self.state),
            Pane::Canvas if self.float_canvas_open => {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(
                            "canvas is open in its own window — window ▾ to bring it back",
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
                    // The preview fills the pane: its rect is the drawing
                    // area the layout probe measures here.
                    self.canvas.dbg_rect = ui.available_rect_before_wrap();
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
    /// The user's saved workspaces from the data dir (none without one),
    /// with what the tolerant load dropped — the shell says that once
    /// ([`Notes`]), so a rebuilt editor never repeats it.
    fn load_workspaces(data_dir: Option<&std::path::Path>) -> (Workspaces, Vec<String>) {
        let Some(dir) = data_dir else {
            return (Workspaces { list: Vec::new() }, Vec::new());
        };
        Workspaces::load_from(dir)
    }

    /// Persist the workspace list into the data dir (none = not persisted).
    fn save_workspaces(&self) {
        if let Some(dir) = &self.data_dir {
            let _ = self.workspaces.save_to(dir);
        }
    }

    /// Enter a spine room — the room switcher, the layout probe and the
    /// room verbs all come through here. The layout always swaps; the
    /// tool/view restore is all-or-nothing, so a live stroke keeps the old
    /// tool state (same law as saved workspaces).
    fn enter_room(&mut self, st: Stage) {
        self.dock = st.dock();
        if self.canvas.apply_view(&st.view(), &mut self.state) {
            self.stage = Some(st);
            // LINE GUARD defaults ON in the ink & paint room (shiage
            // charter) and OFF elsewhere.
            self.state.view.line_guard = matches!(st, Stage::InkPaint);
            self.state.status = format!("room: {}", st.name());
        }
    }

    fn from_form(
        form: &NewProjectForm,
        rs: Option<&RenderState>,
        data_dir: Option<&std::path::Path>,
    ) -> Self {
        // GM7: the form's project is an Srgb one (newproject.rs).
        let state = form.new_state();
        let paint = rs.map(|rs| PaintLayer::new(rs, form.width, form.height));
        let graph = rs.map(|rs| graphcomp::GraphCompositor::new(rs, form.width, form.height));
        let (workspaces, load_notes) = Self::load_workspaces(data_dir);
        Self {
            data_dir: data_dir.map(|d| d.to_path_buf()),
            presets: Vec::new(),
            state,
            canvas: canvas::CanvasView::new(),
            paint,
            graph,
            request_new: false,
            request_welcome: false,
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
            notice_seq: None,
            load_notes,
            workspaces,
            ws_name: String::new(),
            rename_buf: String::new(),
            paper_form: None,
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
            leave: None,
            leave_after_save: None,
            close_ok: false,
            requests: Vec::new(),
            recent: Vec::new(),
            recent_shown: Vec::new(),
            file_menu_open: false,
            recent_edit: None,
            autosave: worksafe::AutosaveClock::new(worksafe::AUTOSAVE_DEFAULT_MIN),
            autosave_job: None,
            last_autosave: None,
            last_autosave_at: None,
            autosave_failing: false,
            title_from: None,
            palette: cmdpal::Palette::default(),
            help: menus::HelpWindows::default(),
            pane_rects: Vec::new(),
            quick_start: [false; 5],
            project_form: None,
        }
    }
    fn from_state(
        state: AppState,
        rs: Option<&RenderState>,
        data_dir: Option<&std::path::Path>,
    ) -> Self {
        let (w, h) = (state.engine.project.width, state.engine.project.height);
        let paint = rs.map(|rs| PaintLayer::new(rs, w, h));
        let graph = rs.map(|rs| graphcomp::GraphCompositor::new(rs, w, h));
        let (workspaces, load_notes) = Self::load_workspaces(data_dir);
        Self {
            data_dir: data_dir.map(|d| d.to_path_buf()),
            presets: Vec::new(),
            state,
            canvas: canvas::CanvasView::new(),
            paint,
            graph,
            request_new: false,
            request_welcome: false,
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
            notice_seq: None,
            load_notes,
            workspaces,
            ws_name: String::new(),
            rename_buf: String::new(),
            paper_form: None,
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
            leave: None,
            leave_after_save: None,
            close_ok: false,
            requests: Vec::new(),
            recent: Vec::new(),
            recent_shown: Vec::new(),
            file_menu_open: false,
            recent_edit: None,
            autosave: worksafe::AutosaveClock::new(worksafe::AUTOSAVE_DEFAULT_MIN),
            autosave_job: None,
            last_autosave: None,
            last_autosave_at: None,
            autosave_failing: false,
            title_from: None,
            palette: cmdpal::Palette::default(),
            help: menus::HelpWindows::default(),
            pane_rects: Vec::new(),
            quick_start: [false; 5],
            project_form: None,
        }
    }

    /// GM8: "Show me" on Quick start step `i`: do the step, or open its room.
    /// Marking a key on an empty cell draws the drawing first (step 2 right
    /// after opening a cut).
    pub(crate) fn show_me(&mut self, i: usize) -> Performed {
        match welcome::STEPS.get(i).map(|s| s.show_me) {
            Some(welcome::ShowMe::Perform(a)) => {
                if a == Action::KindKey && self.state.current_drawing().is_none() {
                    self.perform(Action::NewDrawing);
                }
                self.perform(a)
            }
            Some(welcome::ShowMe::Try(a, st)) => {
                if self.can_perform(a).is_ok() {
                    self.perform(a)
                } else {
                    self.enter_room(st);
                    Performed::Done
                }
            }
            Some(welcome::ShowMe::Export) => {
                self.enter_room(Stage::Edit);
                self.state.status = "export: File ▸ Export… — a PNG sequence or an MP4".into();
                Performed::Done
            }
            None => Performed::Ignored,
        }
    }

    /// GM8 Project Settings: name, size (with its aspect), fps, default
    /// cut length and dpi — Apply is one undoable `SetProjectProps`; the
    /// colour law shows read-only beside its Convert verb.
    fn project_settings_window(&mut self, ctx: &egui::Context) {
        let Some(form) = self.project_form.as_mut() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut convert = false;
        let law = self.state.engine.project.color_law;
        egui::Window::new("Project settings")
            .open(&mut open)
            .default_width(340.0)
            .collapsible(false)
            .show(ctx, |ui| {
                plate::surface(ui);
                egui::Grid::new("project_settings").num_columns(2).show(ui, |ui| {
                    ui.label("name");
                    ui.text_edit_singleline(&mut form.name);
                    ui.end_row();
                    // The paper is anchored top-left: art keeps its place.
                    const RESIZE_NOTE: &str =
                        "the canvas grows or crops from the top-left; drawings are not scaled";
                    ui.label("resolution").on_hover_text(RESIZE_NOTE);
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut form.width).range(16..=16384))
                            .on_hover_text(RESIZE_NOTE);
                        ui.label("×");
                        ui.add(egui::DragValue::new(&mut form.height).range(16..=16384))
                            .on_hover_text(RESIZE_NOTE);
                        let g = gcd(form.width, form.height).max(1);
                        ui.label(
                            egui::RichText::new(format!("{}:{}", form.width / g, form.height / g))
                                .color(plate::legend_dim()),
                        );
                    });
                    ui.end_row();
                    ui.label("fps");
                    ui.add(egui::DragValue::new(&mut form.fps).range(1..=240));
                    ui.end_row();
                    ui.label("default cut length");
                    ui.add(egui::DragValue::new(&mut form.cut_frames).range(1..=100_000).suffix(" frames"));
                    ui.end_row();
                    ui.label("dpi");
                    ui.add(egui::DragValue::new(&mut form.dpi).range(1.0..=2400.0));
                    ui.end_row();
                    ui.label("colour law");
                    ui.horizontal(|ui| {
                        plate::value(ui, law.label());
                        if law != anim_core::color::ColorLaw::Srgb
                            && ui.small_button("Convert…").clicked()
                        {
                            convert = true;
                        }
                    });
                    ui.end_row();
                });
                ui.separator();
                if ui.button("Apply").clicked() {
                    apply = true;
                }
            });
        if apply {
            let after = anim_core::model::ProjectProps {
                name: form.name.clone(),
                width: form.width.max(1),
                height: form.height.max(1),
                fps: form.fps.max(1),
                dpi: form.dpi.max(1.0),
                default_cut_frames: Some(form.cut_frames.max(1)),
            };
            // The tiles never move. The GPU paint layer and compositor are
            // KEPT: both follow the project size on their own next frame
            // (`ensure_size` in the canvas and the graph view), the same way
            // an undone resize or a host's resize mirrored to a guest does.
            self.state.set_project_props(after);
        }
        if convert {
            self.perform(Action::ConvertColorsToSrgb);
        }
        if !open || apply {
            self.project_form = None;
        }
    }

    /// GM10 Cut ▸ Paper size: the cut's paper (kami) in frame pixels —
    /// typed, or a preset centred on the frame — set as one undo step.
    fn paper_size_window(&mut self, ctx: &egui::Context) {
        use anim_core::camera::{self, IRect};
        if self.paper_form.as_ref().is_some_and(|f| f.cut != self.state.at()) {
            self.paper_form = Some(PaperForm::of(&self.state));
        }
        let Some(form) = self.paper_form.as_mut() else {
            return;
        };
        let (fw, fh) = (self.state.engine.project.width, self.state.engine.project.height);
        let frame = IRect::frame(fw, fh);
        let now = camera::paper(self.state.cut(), fw, fh);
        let mut open = true;
        let mut set = false;
        egui::Window::new("Paper size")
            .open(&mut open)
            .default_width(320.0)
            .collapsible(false)
            .show(ctx, |ui| {
                plate::surface(ui);
                plate::legend(ui, "paper (BG sheet)").on_hover_text(
                    "kami (paper): the sheet the camera pans over; make it wider than the frame for a \
                     BG pan or hiki slide",
                );
                let max = camera::MAX_PAPER as i32;
                egui::Grid::new("paper_size").num_columns(2).show(ui, |ui| {
                    const CORNER: &str = "the paper's top-left in frame px; below 0 reaches left of or above the frame";
                    ui.label("corner").on_hover_text(CORNER);
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut form.rect.x).range(-max..=max).prefix("x "))
                            .on_hover_text(CORNER);
                        ui.add(egui::DragValue::new(&mut form.rect.y).range(-max..=max).prefix("y "))
                            .on_hover_text(CORNER);
                    });
                    ui.end_row();
                    const SIZE: &str = "the paper's width and height in px (the frame is the default)";
                    ui.label("size").on_hover_text(SIZE);
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut form.rect.w).range(1..=camera::MAX_PAPER).prefix("w "))
                            .on_hover_text(SIZE);
                        ui.add(egui::DragValue::new(&mut form.rect.h).range(1..=camera::MAX_PAPER).prefix("h "))
                            .on_hover_text(SIZE);
                    });
                    ui.end_row();
                });
                // The studio's usual papers, centred on the frame.
                ui.horizontal(|ui| {
                    for (label, sw, sh, tip) in [
                        ("1.5× wide", 1.5, 1.0, "half a frame more, centred: a hiki slide or a short BG pan"),
                        ("2× wide", 2.0, 1.0, "a frame more, centred: a BG pan of half a frame either way"),
                        ("2× tall", 1.0, 2.0, "a frame taller, centred: a PAN up or down"),
                    ] {
                        if ui.button(label).on_hover_text(tip).clicked() {
                            form.rect = camera::centred_paper(fw, fh, sw, sh);
                        }
                    }
                    if ui
                        .button("paper = frame")
                        .on_hover_text("the paper is the frame again (the default)")
                        .clicked()
                    {
                        form.rect = frame;
                    }
                });
                let ok = form.rect == frame || form.rect.is_valid_paper();
                ui.label(
                    egui::RichText::new(format!(
                        "{}×{} = {:.1} Mpx · {}",
                        form.rect.w,
                        form.rect.h,
                        form.rect.area() as f64 / (1u64 << 20) as f64,
                        camera::paper_limits()
                    ))
                    .color(plate::legend_dim()),
                );
                ui.separator();
                if ui
                    .add_enabled(ok && form.rect != now, egui::Button::new("set paper"))
                    .on_hover_text("the cut's paper becomes this rect (one undo step)")
                    .on_disabled_hover_text(if ok {
                        "this is the cut's paper already"
                    } else {
                        "past the paper's limits (see above)"
                    })
                    .clicked()
                {
                    set = true;
                }
            });
        let rect = form.rect;
        if set {
            self.state.set_cut_paper(rect);
        }
        if !open || set {
            self.paper_form = None;
        }
    }

    /// GM3: which pane the keys are aimed at — the pane under the
    /// pointer, else the focused pane (shortcuts.rs resolves shared chords
    /// by it).
    pub fn shortcut_ctx(&mut self, ctx: &egui::Context) -> config::Ctx {
        let pane = ctx
            .pointer_latest_pos()
            .and_then(|p| self.pane_rects.iter().find(|(_, r)| r.contains(p)))
            .map(|(pane, _)| *pane)
            .or_else(|| self.dock.find_active_focused().map(|(_, t)| *t));
        match pane {
            Some(Pane::Canvas) => config::Ctx::Canvas,
            Some(Pane::XSheet) => config::Ctx::Sheet,
            Some(Pane::NodeGraph) => config::Ctx::NodeGraph,
            Some(Pane::Curves) => config::Ctx::Curves,
            _ => config::Ctx::Global,
        }
    }

    /// GM3: open a pane, or bring it to the front when it is already
    /// docked. A new one tabs in beside the layer strip (or the sheet).
    pub fn open_pane(&mut self, pane: Pane) {
        if let Some(path) = self.dock.find_tab(&pane) {
            let _ = self.dock.set_active_tab(path);
            self.dock.set_focused_node_and_surface(path.node_path());
            return;
        }
        let beside = [Pane::Layers, Pane::XSheet]
            .iter()
            .find_map(|p| self.dock.find_tab(p));
        match beside {
            Some(path) => self.dock[path.node_path()].append_tab(pane),
            None => self.dock.main_surface_mut().push_to_focused_leaf(pane),
        }
    }

    /// GM3: may this action run now? Err says why not, in the artist's
    /// words — a menu item that cannot run is greyed and says so.
    pub fn can_perform(&self, action: Action) -> Result<(), &'static str> {
        let guest = self.canvas.is_guest;
        let shared = guest || self.canvas.session_live;
        let s = &self.state;
        match action {
            Action::Undo if !shared && !s.engine.can_undo() => Err("nothing to undo"),
            Action::Redo if !shared && !s.engine.can_redo() => {
                Err("nothing to redo — undo something first")
            }
            Action::Save | Action::SaveAs if guest => Err("the host owns the file"),
            Action::Open if guest => Err("leave the room first to open your own file"),
            // GM7: the migration rewrites every texel and clears history.
            Action::ConvertColorsToSrgb if shared => Err("leave the room first — it rewrites every cel"),
            Action::ConvertColorsToSrgb if s.engine.project.color_law == ColorLaw::Srgb => {
                Err("this project already stores exact sRGB chips")
            }
            Action::ConvertColorsToSrgb if s.file_path.is_none() => {
                Err("save the project first — the backup goes beside its file")
            }
            // GM8: the project's size and rate are the host's in a room.
            Action::ProjectSettings if guest => Err("the host owns the project settings"),
            Action::WelcomeScreen if guest => Err("leave the room first to open your own file"),
            Action::BuildComposite if s.cut().render_order().is_empty() => {
                Err("no column reaches the composite")
            }
            a if stroke_guarded(a) && self.canvas.stroke_active() => {
                Err("finish the stroke first")
            }
            // The cheap state checks: the verb would refuse or do nothing.
            Action::PrevCut | Action::NextCut
                if s.engine.project.scenes.iter().map(|sc| sc.cuts.len()).sum::<usize>() < 2 =>
            {
                Err("this is the only cut")
            }
            Action::RemoveColumn if s.cut().xsheet.columns.len() <= 1 => {
                Err("a cut keeps at least one column")
            }
            Action::ClearFrameKey
                if s
                    .cut()
                    .xsheet
                    .column(s.view.active_column)
                    .and_then(|c| c.key_at(s.view.frame))
                    .is_none() =>
            {
                Err("no key on this frame")
            }
            Action::ClearCel | Action::ClearCelAll if s.own_key_drawing().is_none() => {
                Err("no drawing of this frame's own to clear")
            }
            // GM5: the sheet verbs that would refuse.
            Action::DeleteFrames if s.frame_count() <= 1 => Err("a cut keeps at least one frame"),
            Action::HoldPlus
            | Action::HoldMinus
            | Action::RetimeOnes
            | Action::RetimeTwos
            | Action::RetimeThrees
            | Action::Reverse
                if s.active_column_def().locked =>
            {
                Err("this column is locked")
            }
            Action::HoldPlus | Action::HoldMinus if s.exposure_here().is_none() => {
                Err("nothing is exposed on this frame")
            }
            Action::HoldPlus if s.exposure_here().is_some_and(|(_, next)| next.is_none()) => {
                Err("the last drawing already holds to the end of the cut")
            }
            // GM14: nothing on the sheet clipboard yet.
            Action::PasteCells | Action::PasteCellsInsert if s.sheet_ui.clipboard.is_none() => {
                Err("copy cells first")
            }
            // GM14: the key verbs act on the curves pane's selected keys.
            Action::InterpHold
            | Action::InterpLinear
            | Action::InterpEase
            | Action::InterpEaseIn
            | Action::InterpEaseOut
            | Action::InterpBezier
            | Action::HandlesAuto
            | Action::HandlesSmooth
            | Action::HandlesFlat
            | Action::HandlesBroken
                if s.curves.selected.is_empty() =>
            {
                Err("select keys in the curves pane first")
            }
            Action::FrameCurves if s.curves.plot.is_none() => Err("open the curves pane first"),
            // GM6: the genga-to-douga verbs that would refuse.
            Action::CopyPixels if self.canvas.selection_poly().is_none() => {
                Err("select pixels first")
            }
            Action::PastePixels if s.pixel_clip.is_none() => Err("copy pixels first"),
            Action::DuplicateCel
            | Action::KindKey
            | Action::KindBreakdown
            | Action::KindInbetween
            | Action::KindUnset
            | Action::PinToLightTable
                if s.current_drawing().is_none() =>
            {
                Err("no drawing on this frame")
            }
            Action::MakeInbetweens
                if s.current_drawing().is_none_or(|d| s.cut().drawing_def(d).chart.is_none()) =>
            {
                Err("give this key a spacing chart first")
            }
            Action::ResetShift if s.view.light.offsets.is_empty() => Err("no pin is shifted"),
            _ => Ok(()),
        }
    }

    /// GM3: a toggle's state now (its menu item shows it checked); None
    /// for a verb that is not a toggle.
    pub fn is_on(&self, action: Action) -> Option<bool> {
        match action {
            Action::ToggleOnion => Some(self.state.view.onion),
            Action::ToggleCompositeView => Some(self.canvas.composite_view),
            Action::MissCheck => Some(self.canvas.miss_check),
            Action::ToggleLoop => Some(self.state.view.loop_playback),
            Action::MirrorView => Some(self.canvas.view_mirrored()),
            // GM5: the active column's definition.
            Action::ToggleColumnHidden => Some(self.state.active_column_def().hidden),
            Action::ToggleColumnLocked => Some(self.state.active_column_def().locked),
            Action::ColumnKindCel => Some(self.state.active_column_def().kind == ColumnKind::Cel),
            Action::ColumnKindBg => Some(self.state.active_column_def().kind == ColumnKind::Bg),
            Action::ColumnKindBook => Some(self.state.active_column_def().kind == ColumnKind::Book),
            Action::ColumnKindRef => Some(self.state.active_column_def().kind == ColumnKind::Ref),
            // GM6: the light table's latch and this frame's pin.
            Action::ShiftTrace => Some(self.state.view.light.shift_trace),
            Action::PinToLightTable => Some(
                self.state.current_drawing().is_some_and(|d| self.state.view.light.is_pinned(d)),
            ),
            _ => None,
        }
    }

    /// GM3: run an action from the menus or the command palette — the
    /// same one dispatch as a shortcut (D6). What only the shell can do
    /// goes the way the slate always sent it: New/Open through the unsaved
    /// guard, the rest to the shell's outbox.
    pub fn invoke(&mut self, action: Action, ctx: &egui::Context) -> Performed {
        if let Err(why) = self.can_perform(action) {
            self.state.refuse(format!("refused — {why}"));
            return Performed::Ignored;
        }
        let r = self.perform(action);
        match r {
            Performed::App(AppRequest::NewProject) => self.leave(Leave::New, ctx),
            Performed::App(AppRequest::Open) => self.leave(Leave::Open, ctx),
            Performed::App(req) => self.requests.push(req),
            Performed::Done | Performed::Ignored => {}
        }
        r
    }

    /// GM2: carry out `what` now when nothing would be lost (or this
    /// machine is a guest — the host owns the document); otherwise ask.
    pub fn leave(&mut self, what: Leave, ctx: &egui::Context) {
        if !self.hold_leave(what.clone()) {
            self.go(what, ctx);
        }
    }

    /// GM2: hold `what` for the "Save changes?" modal when it would drop
    /// unsaved work; true when held. A save still landing decides first —
    /// it may leave nothing unsaved. One leave is pending at a time, the
    /// newest wins, and closing the window outranks New/Open (a close
    /// arriving over a pending New must not be dropped).
    fn hold_leave(&mut self, what: Leave) -> bool {
        if self.canvas.is_guest || !self.state.is_dirty() {
            return false;
        }
        let slot = if self.leave_after_save.is_some()
            || (self.leave.is_none() && self.state.saving.is_some())
        {
            &mut self.leave_after_save
        } else {
            &mut self.leave
        };
        if *slot != Some(Leave::Close) {
            *slot = Some(what);
        }
        true
    }

    /// GM2: the "Save changes?" modal is up — the shell's shortcuts stand
    /// down until it is answered (Ctrl+Z, tool keys and Ctrl+N must not act
    /// behind a question about the document).
    pub fn leave_pending(&self) -> bool {
        self.leave.is_some()
    }

    /// GM2: the shell mirrors the config's recent list here; the menu's
    /// "on disk" flags are checked now, not every frame.
    pub fn set_recent(&mut self, list: &[std::path::PathBuf]) {
        if self.recent != list {
            self.recent = list.to_vec();
            self.recent_shown = worksafe::on_disk(&self.recent);
        }
    }

    fn go(&mut self, what: Leave, ctx: &egui::Context) {
        // Every new AppState brings a fresh untitled group, so a changed one
        // below means the document was replaced.
        let doc = self.state.untitled_group.clone();
        match what {
            Leave::Close => {
                self.close_ok = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Leave::New => self.request_new = true,
            Leave::Welcome => self.request_welcome = true,
            // Land any in-flight gesture in the CURRENT project first — its
            // commit must not cross into the opened one.
            Leave::Open => {
                self.canvas.finish_gesture(&mut self.state);
                self.state.open();
            }
            Leave::OpenPath(p) => {
                self.canvas.finish_gesture(&mut self.state);
                self.state.open_path(p);
            }
            Leave::Recover(r) => {
                self.canvas.finish_gesture(&mut self.state);
                match worksafe::open_recovered(&r) {
                    Ok(state) => {
                        self.state = state;
                        // The work is open again: the crashed copy's record
                        // has done its job.
                        let _ = std::fs::remove_file(&r.record);
                    }
                    Err(e) => self.state.refuse(format!("could not open the autosave — {e}")),
                }
            }
        }
        if self.state.untitled_group != doc {
            // Another document: the canvas resyncs, and the last autosave
            // belonged to the one before (the running record must not pair
            // them) — so does one still being written.
            self.canvas.engine_changed();
            self.autosave_job = None;
            self.last_autosave = None;
            self.last_autosave_at = None;
        }
    }

    /// GM2: the per-frame safety net — window close guard, the "Save
    /// changes?" modal, the save that resumes a leave, the title, autosave.
    fn worksafe_frame(&mut self, ctx: &egui::Context) {
        if self.state.poll_save() {
            // On disk: this document's autosaves are all older than the
            // file now and could never be offered — clear them away.
            if let (Some(dir), Some(a)) = (&self.data_dir, self.last_autosave.take())
                && let Some(g) = worksafe::group_of(&a)
            {
                worksafe::drop_group(dir, &g);
            }
        }
        // A window close (title-bar ×, Alt+F4, Quit) is held while the
        // document has unsaved changes and nobody answered yet. During a
        // save that is landing, the question waits for it (it may leave
        // nothing unsaved); over a pending New/Open, the close wins.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.close_ok
            && self.hold_leave(Leave::Close)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        // "Save" was chosen: go once the save lands clean; a failed or
        // cancelled save asks again (never leaves work behind silently).
        if let Some(what) = self.leave_after_save.take() {
            if self.state.saving.is_some() {
                self.leave_after_save = Some(what);
            } else if self.state.is_dirty() {
                self.leave = Some(what);
            } else {
                self.go(what, ctx);
            }
        }
        self.leave_modal(ctx);
        self.convert_modal(ctx);
        self.auto_comp_modal(ctx);
        // The title is rebuilt only when what it shows changes.
        let dirty = self.state.is_dirty();
        let stale = self.title_from.as_ref().is_none_or(|(d, f, n)| {
            *d != dirty || *f != self.state.file_path || *n != self.state.engine.project.name
        });
        if stale {
            self.title_from = Some((
                dirty,
                self.state.file_path.clone(),
                self.state.engine.project.name.clone(),
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(window_title(&self.state)));
        }
        self.autosave_tick(ctx);
    }

    /// GM7: Convert colours to sRGB — a held DANGER that says every step
    /// before it runs (D8).
    /// GM8: "Build composite from sheet" over a graph shaped by hand — a
    /// DANGER confirm, since it replaces every node (undo brings them back).
    fn auto_comp_modal(&mut self, ctx: &egui::Context) {
        let Some(n) = self.state.auto_comp_confirm else {
            return;
        };
        let mut choice = None;
        egui::Modal::new(egui::Id::new("gm8_auto_comp")).show(ctx, |ui| {
            plate::legend(ui, "build composite from sheet");
            ui.label(format!("Replace the node graph ({n} nodes)? Undo brings it back."));
            ui.label(
                egui::RichText::new("every rendered column, in sheet order, chained into the output")
                    .color(plate::legend_dim()),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if plate::danger_hint(ui, "Replace", "replaces every node of this cut's graph") {
                    choice = Some(true);
                }
                if ui.button("Cancel").on_hover_text("Esc").clicked() {
                    choice = Some(false);
                }
            });
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => {
                self.state.auto_comp_confirm = None;
                self.state.auto_comp();
            }
            Some(false) => self.state.auto_comp_confirm = None,
            None => {}
        }
    }

    fn convert_modal(&mut self, ctx: &egui::Context) {
        if !self.state.convert_confirm {
            return;
        }
        let mut choice = None;
        egui::Modal::new(egui::Id::new("gm7_convert_colours")).show(ctx, |ui| {
            plate::legend(ui, "convert colours to sRGB");
            ui.label("Store this project's colours as exact sRGB chips?");
            for step in [
                "1. A backup of the project as it is now goes to <name>.pre-srgb.animproj beside it.",
                "2. Every painted pixel is remapped, so flat chips export as exactly the chip.",
                "3. The undo history is cleared (its steps hold the old values).",
                "4. The project is left unsaved — Save writes it.",
            ] {
                ui.label(egui::RichText::new(step).color(plate::legend_dim()));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if plate::danger_hint(
                    ui,
                    "Convert",
                    "rewrites every cel's colours and clears the undo history",
                ) {
                    choice = Some(true);
                }
                if ui.button("Cancel").on_hover_text("Esc").clicked() {
                    choice = Some(false);
                }
            });
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => {
                self.state.convert_confirm = false;
                self.canvas.finish_gesture(&mut self.state);
                if let Err(why) = self.state.convert_to_srgb() {
                    self.state.refuse(format!("refused — {why}"));
                }
            }
            Some(false) => self.state.convert_confirm = false,
            None => {}
        }
    }

    fn leave_modal(&mut self, ctx: &egui::Context) {
        let Some(what) = self.leave.clone() else { return };
        let name = self
            .state
            .file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "this untitled project".into());
        let mut choice = None;
        egui::Modal::new(egui::Id::new("gm2_save_changes")).show(ctx, |ui| {
            plate::legend(ui, "save changes");
            ui.label("Save changes?");
            ui.label(
                egui::RichText::new(format!("{name} has edits that are not on disk."))
                    .color(plate::legend_dim()),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Save").on_hover_text("Enter").clicked() {
                    choice = Some(0);
                }
                // Discarding work is destructive: a held DANGER (D7).
                if plate::danger_hint(
                    ui,
                    "Don't Save",
                    "goes on without saving; the edits since your last save are dropped",
                ) {
                    choice = Some(1);
                }
                if ui.button("Cancel").on_hover_text("Esc").clicked() {
                    choice = Some(2);
                }
            });
            // The keys a dialog answers to; consumed, so no shortcut reads
            // them too once the modal is gone.
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                    choice = Some(2);
                } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                    choice = Some(0);
                }
            });
        });
        match choice {
            Some(0) => {
                self.leave = None;
                self.canvas.finish_gesture(&mut self.state);
                if self.state.file_path.is_none() {
                    // Untitled: Save means Save As — the shell owns dialogs.
                    self.requests.push(AppRequest::SaveAs);
                } else {
                    self.state.save(false);
                }
                self.leave_after_save = Some(what);
            }
            Some(1) => {
                self.leave = None;
                self.canvas.finish_gesture(&mut self.state);
                self.go(what, ctx);
            }
            Some(2) => self.leave = None,
            _ => {}
        }
    }

    /// GM2: autosave into the data folder when it is due (never the
    /// user's file, never mid-stroke, never on the UI thread).
    fn autosave_tick(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.autosave_job {
            let landed = match rx.try_recv() {
                Ok(r) => Some(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("the autosave worker stopped".into()))
                }
            };
            match landed {
                None => {}
                Some(Ok(path)) => {
                    self.autosave_job = None;
                    // A quiet trace: the net is there and working.
                    let at = worksafe::unix_now();
                    self.state.status =
                        format!("autosaved {}", worksafe::clock_hm(at, worksafe::utc_offset_min()));
                    self.last_autosave = Some(path);
                    self.last_autosave_at = Some(at);
                    self.autosave_failing = false;
                }
                Some(Err(e)) => {
                    self.autosave_job = None;
                    // The safety net is off: said once per failing streak,
                    // on the Aka lane — a grey line would fade unread.
                    if !self.autosave_failing {
                        self.state
                            .refuse(format!("AUTOSAVE FAILED — {e}. Your work is only in memory."));
                    }
                    self.autosave_failing = true;
                }
            }
        }
        if self.data_dir.is_none() || self.canvas.is_guest || self.autosave_job.is_some() {
            return;
        }
        let (now, down) = ctx.input(|i| (i.time, i.pointer.any_down()));
        let dirty = self.state.is_dirty();
        if !self.autosave.tick(now, dirty, self.canvas.stroke_active(), down) {
            return;
        }
        let Some(dir) = self.data_dir.clone() else { return };
        let project = self.state.snapshot_project();
        let file = self.state.file_path.clone();
        let group = worksafe::autosave_group(file.as_deref(), &self.state.untitled_group);
        let seq = worksafe::unix_now();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = worksafe::write_autosave(&dir, &project, file.as_deref(), &group, seq);
            let _ = tx.send(r);
        });
        self.autosave_job = Some(rx);
    }
}

impl eframe::App for App {
    fn on_exit(&mut self) {
        // AUTO-UPDATE (owner's word: "on close it should update"): a
        // FULLY-staged build installs silently at exit, so the next
        // launch is the new version. Never blocks exit on an unfinished
        // download; a devloop-armed build never stages one.
        if let Some(staged) = self.update_staged.take() {
            let _ = crate::update::swap_staged(&staged);
        }
        // GM2: a clean exit removes this copy's own running record — never
        // another copy's, and never a crashed copy's that "Later" kept.
        self.retire_running();
    }


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
        // GM8: the UI scale (Settings ▸ General), as egui's zoom factor.
        apply_ui_scale(ui.ctx(), &self.config);
        // Nothing open and no dialog -> the Welcome screen (GM8), or the
        // New Project form when its "Show at startup" latch is off.
        match startup_route(
            self.editor.is_some(),
            self.new_form.is_some(),
            self.welcome.as_ref(),
            &self.config,
        ) {
            Startup::Welcome => {
                self.welcome = Some(welcome::Welcome::new(&self.config.session.recent));
            }
            Startup::NewForm => self.new_form = Some(NewProjectForm::default()),
            Startup::DropWelcome => self.welcome = None,
            Startup::Keep => {}
        }
        // GM8: this session's Quick start ticks reach the config, and the
        // editor's checklist shows every tick ever made.
        if let Some(e) = &mut self.editor {
            if merge_quick_start(&mut self.config, &e.quick_start) {
                self.config.save();
            }
            e.quick_start = self.config.ui.quick_start;
        }

        // Apply live settings (read as locals to avoid borrowing self.config
        // while self.editor is borrowed mutably).
        let undo_limit = self.config.perf.undo_limit;
        let canvas_filter = self.config.perf.canvas_filter.wgpu();
        let pen = self.config.pen.clone();
        let layers_cfg = self.config.layers.clone();
        let mut presets = self.config.presets.clone();
        // v0.2.5: the host's brushes ride beside our own while joined
        // (marked ⌂; the save-back below filters them out — session-
        // scoped, never persisted).
        presets.extend(self.session_presets.iter().cloned());
        let mut presets_dirty = false;
        let native_pen = self.pump_tablet();

        // Editor (if any) renders first as the base layer.
        if let Some(editor) = &mut self.editor {
            // What the tolerant loads reset (config.json at startup, this
            // editor's workspaces.json) and a lost tablet: one notice on
            // the Aka lane, each said once per session.
            for n in self.config.load_notes.drain(..) {
                self.notes.push(n);
            }
            self.notes.say(editor);
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
                    self.session_peers.values().map(peer_tag).collect(),
                )),
                net::Session::Joined(_) => Some((
                    "in a room".to_string(),
                    self.session_peers.values().map(peer_tag).collect(),
                )),
                net::Session::Idle => None,
            };
            editor.update_tag = self.update_ready.as_ref().map(|(t, _, _)| t.clone());
            // PSD-cel-layers Stage 4: the current ROOM's stack becomes the
            // new-cel template (held-stack clone still wins inside doc),
            // and the strip gets the pickable list.
            let room_key = stage_key(editor.stage);
            let stack = layers_cfg.stack_for_room(room_key);
            // Entering a room arms its stack's working layer (shiage →
            // color, LO → rough), the way a room re-arms its tool. Never
            // under a live pen.
            if self.stack_room_seen != room_key && !editor.canvas.stroke_active() {
                self.stack_room_seen = room_key;
                let (n, r) = stack.armed();
                editor.state.view.active_layer = n;
                editor.state.view.active_role = r;
            }
            editor.state.new_cel_active = stack.armed();
            editor.state.new_cel_template = stack.layers.clone();
            editor.state.stack_current = stack.name.clone();
            editor.state.stacks = layers_cfg.all_stacks();
            // GM6: the onion and light table settings live in the config.
            editor.canvas.onion = self.config.onion.sane();
            editor.canvas.light_table = self.config.light_table;
            editor.ui(
                ui,
                &pen,
                &layers_cfg,
                &mut presets,
                &mut presets_dirty,
                &native_pen,
                self.config.ui.lock_positions,
                room,
                &self.config.keybinds,
            );
            // Stage 4: the strip's stack requests (the App owns config).
            if let Some(req) = editor.state.stack_request.take() {
                match req {
                    doc::StackRequest::Pick(name) => {
                        editor.state.status = format!("new cels in this room get '{name}'");
                        self.config
                            .layers
                            .room_stack
                            .insert(room_key.to_string(), name);
                        self.config.save();
                    }
                    doc::StackRequest::Save(name) => {
                        match editor.state.current_stack_as_preset(&name) {
                            Some(preset) => {
                                let stacks = &mut self.config.layers.stacks;
                                match stacks.iter_mut().find(|s| s.name == name) {
                                    Some(s) => *s = preset,
                                    None => stacks.push(preset),
                                }
                                self.config.save();
                                editor.state.status = format!("stack '{name}' saved");
                            }
                            None => editor
                                .state
                                .refuse("refused — no layered cel on this frame to save"),
                        }
                    }
                }
            }
            // GM4: the OPTIONS row's stabilizer level persists with the pen.
            if let Some(level) = editor.canvas.stabilizer_request.take() {
                self.config.pen.stabilizer = level;
                self.config.save();
            }
            // GM6: a light box edit of the onion or the light table persists
            // (the pins and shift offsets are view state and never do).
            if let Some(cfg) = editor.canvas.onion_request.take() {
                self.config.onion = cfg;
                self.config.save();
            }
            if let Some(cfg) = editor.canvas.light_table_request.take() {
                self.config.light_table = cfg;
                self.config.save();
            }
            if presets_dirty {
                // v0.2.5: the host's session brushes never persist.
                self.config.presets = presets
                    .iter()
                    .filter(|p| p.bank != SESSION_BANK)
                    .cloned()
                    .collect();
                self.config.save();
            }
            if editor.request_new {
                editor.request_new = false;
                self.new_form = Some(NewProjectForm::default());
            }
            if editor.request_welcome {
                editor.request_welcome = false;
                self.welcome = Some(welcome::Welcome::over_editor(&self.config.session.recent));
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
        // GM2: requests from the save modal, the recent list, the running
        // record, the recovery offer.
        self.worksafe_shell(&ui.ctx().clone());

        // Keyboard-shortcut dispatch — skipped while a dialog is up (GM2's
        // "Save changes?" and recovery modals included: nothing acts behind
        // a question about the document) or while capturing a rebind (so the
        // captured key doesn't also fire an action).
        // GM3: the command palette has the keys while it is open.
        if self
            .editor
            .as_ref()
            .is_some_and(|e| !e.leave_pending() && !e.palette.open)
            && self.recovery.is_empty()
            && self.new_form.is_none()
            && self.welcome.is_none()
            && self.capturing.is_none()
        {
            let here = match &mut self.editor {
                Some(ed) => ed.shortcut_ctx(ui.ctx()),
                None => config::Ctx::Global,
            };
            let mut fired: Vec<Action> = shortcuts::collect(ui.ctx(), &self.config.keybinds, here);
            // Keyboard events land in whichever VIEWPORT has focus. The
            // popout canvas (Phase 5 step 2) is its own OS window, so a
            // shortcut pressed there arrives in ITS input state, not the
            // root's — check it too. A physical keypress reaches exactly one
            // viewport's input, so a deduped OR can never double-fire (undo
            // twice from one Ctrl+Z).
            let float_open = self.editor.as_ref().is_some_and(|e| e.float_canvas.open);
            if float_open {
                let more = shortcuts::collect_for(
                    ui.ctx(),
                    floatcanvas::FloatCanvas::viewport_id(),
                    &self.config.keybinds,
                    config::Ctx::Canvas,
                );
                for a in more {
                    if !fired.contains(&a) {
                        fired.push(a);
                    }
                }
            }
            let ctx = ui.ctx().clone();
            for action in fired {
                self.perform(&ctx, action);
            }
        }

        // Exact UI descriptions: one global read per frame (plate serves
        // every control's hover from it).
        plate::set_exact(self.config.ui.exact_descriptions);

        // Settings window (Krita-style: shortcuts + performance).
        // STAGE 4: the Layout page's inputs (computed BEFORE the raster
        // toggle borrows the editor mutably for the window's lifetime).
        let layout_names: Vec<String> = self
            .editor
            .as_ref()
            .map(|ed| ed.workspaces.list.iter().map(|w| w.name.clone()).collect())
            .unwrap_or_default();
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
        // LAN DISCOVERY: a fresh snapshot of visible rooms for the page,
        // and the click-out slot.
        let mut lan_rooms: Vec<net::Beacon> =
            self.discovery.rooms.lock().unwrap().values().cloned().collect();
        lan_rooms.sort_by(|a, b| a.name.cmp(&b.name));
        let mut join_room: Option<String> = None;
        let hosting = matches!(self.session, net::Session::Hosting(_));
        let joined = matches!(self.session, net::Session::Joined(_));
        // STAGE 4: the Layout page's click-out slot.
        let mut layout_act: Option<config::LayoutAct> = None;
        // GM2: the General page shows the autosave folder; its button asks
        // for the folder here (built only while the window is open).
        let autosave_dir = self
            .data_dir
            .as_deref()
            .filter(|_| self.settings_open)
            .map(config::autosave_dir_in);
        let mut show_autosaves = false;
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
            &lan_rooms,
            &mut join_room,
            &layout_names,
            self.session_host_layout.is_some(),
            &mut self.layout_ws_name,
            &mut layout_act,
            autosave_dir.as_deref(),
            &mut show_autosaves,
        );
        if show_autosaves && let Some(dir) = &autosave_dir {
            // Before the first autosave lands the folder may not exist yet.
            let _ = std::fs::create_dir_all(dir);
            proc::show_folder(dir);
        }
        if let Some(a) = session_action {
            self.session_act(a);
        }
        // STAGE 4: the Layout page's verdicts — the App owns the dock
        // and the workspace list; presets persist per-user across
        // hosted sessions, and the host's layout loads on request only.
        if let Some(act) = layout_act
            && let Some(ed) = &mut self.editor
        {
            match act {
                config::LayoutAct::Save(name) => {
                    let view = Some(ed.canvas.snapshot_view(&ed.state));
                    if let Some(w) = ed.workspaces.list.iter_mut().find(|w| w.name == name) {
                        w.dock = ed.dock.clone();
                        w.view = view;
                    } else {
                        ed.workspaces.list.push(workspace::Workspace {
                            name,
                            dock: ed.dock.clone(),
                            preset: None,
                            view,
                        });
                    }
                    ed.save_workspaces();
                }
                config::LayoutAct::Load(name) => {
                    if let Some(w) = ed
                        .workspaces
                        .list
                        .iter()
                        .find(|w| w.name == name)
                        .cloned()
                    {
                        ed.dock = w.dock;
                        ed.stage = None;
                        if let Some(v) = w.view {
                            let _ = ed.canvas.apply_view(&v, &mut ed.state);
                        }
                        ed.state.status = format!("layout preset: {name}");
                    }
                }
                config::LayoutAct::Delete(name) => {
                    ed.workspaces.list.retain(|w| w.name != name);
                    ed.save_workspaces();
                }
                config::LayoutAct::LoadHost => {
                    if let Some(json) = &self.session_host_layout
                        && let Ok(w) = serde_json::from_str::<workspace::Workspace>(json)
                    {
                        ed.dock = w.dock;
                        ed.stage = None;
                        if let Some(v) = w.view {
                            let _ = ed.canvas.apply_view(&v, &mut ed.state);
                        }
                        ed.state.status = "the host's layout".into();
                    }
                }
            }
        }
        // LAN DISCOVERY: one-click join (open rooms need no key — the
        // host skips the check; any key string satisfies the wire).
        if let Some(addr) = join_room {
            self.config.session.last_addr = addr.clone();
            self.config.save();
            if let Some(ed) = &mut self.editor {
                ed.canvas.guest_ready = false;
            }
            self.session_join(addr, String::new());
        }
        // PSD-shipping: "check now" from the Plugins page.
        if check_now && !self.config.update_repo.trim().is_empty() {
            self.update_note = "checking…".into();
            self.update_rx = Some(crate::update::spawn_check(
                self.config.update_repo.trim().to_string(),
            ));
        }
        // THE CONNECT WINDOW: the 2FA code lives here and nowhere else.
        // OWNER AMENDMENT 2026-08-19: the 2FA window is dormant — the
        // Connect click joins with the key alone (code "").
        if std::mem::take(&mut self.connect_open) {
            let addr = self.connect_addr.trim().to_string();
            if addr.is_empty() {
                self.session_status = "enter the host's address first".into();
            } else {
                self.session_join(addr, String::new());
            }
        }
        // WIRE LOG: frame stalls while a session is live are the lag —
        // log them with what the frame was doing.
        {
            let now = std::time::Instant::now();
            if let Some(last) = self.frame_watch {
                let gap = now.duration_since(last).as_millis();
                if gap > 100 && !matches!(self.session, net::Session::Idle) {
                    net::slog(format!("STALL frame-gap={gap}ms"));
                }
            }
            self.frame_watch = Some(now);
        }
        self.session_pump(ui.ctx());
        // PSD-shipping: the published-build channel (a thread talks to
        // GitHub; this only reads channels — never blocks the pen).
        self.update_pump(ui.ctx());
        // AUTO-UPDATE: long sessions re-check every 15 minutes (launch
        // checked once already; dev builds have update_rx wired off).
        {
            let t = ui.ctx().input(|i| i.time);
            if self.update_next_check == 0.0 {
                self.update_next_check = t + 900.0;
            } else if t > self.update_next_check
                && self.update_rx.is_none()
                && self.update_ready.is_none()
                && !devloop::armed()
                && !self.config.update_repo.trim().is_empty()
            {
                self.update_next_check = t + 900.0;
                self.update_rx = Some(crate::update::spawn_check(
                    self.config.update_repo.trim().to_string(),
                ));
            }
        }
        // The Foot's UPDATE READY click: start the download+swap thread,
        // guarded like a devloop handover (no stroke, no dialogs).
        if let Some(ed) = &mut self.editor
            && std::mem::take(&mut ed.request_update)
        {
            let busy = ed.canvas.stroke_active()
                || self.settings_open
                || self.new_form.is_some()
                || ed.leave_pending()
                || !self.recovery.is_empty();
            if busy {
                ed.state
                    .refuse("refused — finish the stroke / close dialogs, then update");
            } else if let Some(staged) = self.update_staged.clone() {
                // Already downloaded: swap and relaunch NOW.
                match crate::update::swap_staged(&staged) {
                    Ok(new_exe) => {
                        self.update_staged = None;
                        self.relaunch_into(new_exe, ui.ctx());
                    }
                    Err(why) => {
                        self.update_note = format!("update failed — {why}");
                        ed.state.refuse(format!("update refused — {why}"));
                    }
                }
            } else if self.update_ready.is_some() {
                ed.state.status = "still downloading the update…".into();
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
        // The Welcome screen likewise (GM8: a modal over an open cut).
        if self.new_form.is_some() {
            self.show_new_dialog(ui);
        } else if self.welcome.is_some() {
            self.show_welcome(ui);
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
    /// GM8: the Welcome screen — the central area when nothing is open, a
    /// modal over the cut when File ▸ Welcome screen… asked — and what its
    /// cards ask. A card that opens a document ends the screen.
    fn show_welcome(&mut self, ui: &mut egui::Ui) {
        let Some(mut w) = self.welcome.take() else {
            return;
        };
        let mut show = self.config.ui.show_welcome;
        let ticks = self.config.ui.quick_start;
        let binds = &self.config.keybinds;
        let mut action = None;
        let over_cut = self.editor.is_some();
        if over_cut {
            let ctx = ui.ctx().clone();
            let room = ctx.content_rect();
            let modal = egui::Modal::new(egui::Id::new("gm8_welcome")).show(&ctx, |ui| {
                ui.set_width((room.width() - 120.0).clamp(320.0, 980.0));
                ui.set_max_height((room.height() - 120.0).max(240.0));
                welcome::ui(ui, &mut w, &mut show, &ticks, binds)
            });
            // Esc or a click outside: back to the cut.
            let back = modal.should_close();
            action = modal.inner.or(back.then_some(welcome::WelcomeAction::Back));
        } else {
            egui::CentralPanel::default().show(ui, |ui| {
                action = welcome::ui(ui, &mut w, &mut show, &ticks, binds);
            });
        }
        if show != self.config.ui.show_welcome {
            self.config.ui.show_welcome = show;
            self.config.save();
        }
        let (rs, dir) = (self.render_state.as_ref(), self.data_dir.as_deref());
        let opened = |state: AppState| Editor::from_state(state, rs, dir);
        let mut done = false;
        match action {
            Some(welcome::WelcomeAction::Template(i)) => {
                self.editor = Some(open_template(i, rs, dir));
                done = true;
            }
            // The form shows over the screen; its Cancel comes back here.
            Some(welcome::WelcomeAction::Custom) => {
                self.new_form = Some(NewProjectForm::default());
            }
            Some(welcome::WelcomeAction::Open) => match AppState::pick_and_open() {
                Some(Ok(state)) => {
                    self.editor = Some(opened(state));
                    done = true;
                }
                Some(Err(msg)) => {
                    self.notes.push(format!("could not open that project — {msg}"));
                }
                None => {}
            },
            Some(welcome::WelcomeAction::OpenRecent(path)) => {
                match AppState::load_from(path.clone()) {
                    Ok(state) => {
                        self.editor = Some(opened(state));
                        done = true;
                    }
                    Err(msg) => {
                        self.notes.push(format!("could not open that project — {msg}"));
                    }
                }
            }
            Some(welcome::WelcomeAction::Remove(path)) => {
                forget_recent(&mut self.config, &mut w, &path);
                self.config.save();
            }
            Some(welcome::WelcomeAction::Sample) => {
                self.editor = Some(opened(sample::build_sample()));
                done = true;
            }
            Some(welcome::WelcomeAction::ShowMe(step)) => {
                self.editor = Some(welcome_show_me(step, rs, dir).0);
                done = true;
            }
            // Over a cut the modal would sit above the Settings window.
            Some(welcome::WelcomeAction::Settings) => {
                self.settings_open = true;
                done = over_cut;
            }
            Some(welcome::WelcomeAction::Back) => done = true,
            None => {}
        }
        if !done {
            self.welcome = Some(w);
        }
    }

    fn show_new_dialog(&mut self, ui: &mut egui::Ui) {
        let has_editor = self.editor.is_some();
        // GM8: Custom size… from the Welcome screen can go back to it.
        let can_cancel = has_editor || self.welcome.is_some();
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
                            action = newproject::form_ui(ui, &mut form, can_cancel);
                        });
                });
            });
        }
        match action {
            Some(FormAction::Create) => {
                let mut editor =
                    Editor::from_form(&form, self.render_state.as_ref(), self.data_dir.as_deref());
                // GM8: a template's cut opens in its first room.
                if let Some(t) = form.template() {
                    editor.enter_room(t.room);
                }
                self.editor = Some(editor);
                // A Welcome screen behind the form has done its job.
                self.welcome = None;
            }
            Some(FormAction::Open) => match AppState::pick_and_open() {
                Some(Ok(state)) => {
                    self.editor = Some(Editor::from_state(
                        state,
                        self.render_state.as_ref(),
                        self.data_dir.as_deref(),
                    ));
                    self.welcome = None;
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
                // Only offered when an editor or the Welcome screen is
                // behind the dialog; that is what shows again.
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
    // Every room, at each display scale (fractional scales can add
    // pixel-rounding jitter that 1.0 never shows).
    let (mut moved, mut blind) = (0usize, 0usize);
    for &stage in Stage::ALL {
        for ppp in PROBE_SCALES {
            println!("---- {} · pixels_per_point {ppp} ----", stage.name());
            let run = probe_run(stage, ppp);
            for line in &run.lines {
                println!("{line}");
            }
            moved += run.moved;
            blind += run.unmeasured;
        }
    }
    println!();
    if moved == 0 && blind == 0 {
        println!("PROBE PASS: canvas rect never moved");
    } else {
        println!("PROBE FAIL: canvas rect moved {moved} time(s), unmeasured {blind} time(s)");
    }
}

/// The display scales the probe walks (the testkit runs the same set).
const PROBE_SCALES: [f32; 3] = [1.0, 1.25, 1.5];

/// What one probe pass saw.
struct ProbeRun {
    /// One line per step: the rect, and any MOVED / UNMEASURED note.
    lines: Vec<String>,
    /// Scripted steps (after settle) where the rect moved.
    moved: usize,
    /// Scripted steps where the canvas pane recorded no rect at all — a
    /// probe that measured nothing must fail, never pass.
    unmeasured: usize,
}

/// One probe pass in one room at one scale — THE scenario, shared by the
/// CLI probe and the testkit test. The room is entered the way the room
/// switcher enters it, and the Canvas tab is brought to the front (the
/// Edit room keeps it behind the viewer, where the artist reaches it for a
/// quick fix); the Layout room's canvas pane is the cut's preview, which
/// is measured in its place.
fn probe_run(stage: Stage, ppp: f32) -> ProbeRun {
    probe_run_zoom(stage, ppp, 100)
}

/// GM8: the same pass under a UI scale (percent), applied the way the App
/// applies it — egui's zoom factor over the display's scale.
fn probe_run_zoom(stage: Stage, ppp: f32, ui_scale: u32) -> ProbeRun {
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(ppp);
    dress(&ctx);
    let mut config = Config::default();
    config.ui.scale = ui_scale;
    if ui_scale != 100 {
        // The headless display is 1 px per point, so the zoom factor IS
        // the scale over it: `ppp` × the UI scale.
        ctx.set_zoom_factor(ppp * config.ui.zoom());
    }
    let pen = config.pen.clone();
    let layers_cfg = config.layers.clone();
    let mut state = AppState::new_project("probe", 1920, 1080, 24, 300.0);
    state.new_drawing_at_frame(); // cel at frame 0; frame 1 stays empty
    let mut editor = Editor::from_state(state, None, None);
    editor.enter_room(stage);
    if let Some(tab) = editor.dock.find_tab(&Pane::Canvas) {
        let _ = editor.dock.set_active_tab(tab);
    }
    let mut lines: Vec<String> = Vec::new();
    let mut unmeasured = 0usize;
    let raw = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };
    let mut last: Option<egui::Rect> = None;
    let mut moved = 0usize;
    let mut run =
        |editor: &mut Editor, label: &str, last: &mut Option<egui::Rect>, moved: &mut usize| {
            // Each pass must record its own rect: a stale one would hide a
            // pane that stopped drawing.
            editor.canvas.dbg_rect = egui::Rect::NOTHING;
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
                &config.keybinds,
            );
            let _ = ctx.end_pass();
            let r = editor.canvas.dbg_rect;
            if !(r.is_finite() && r.is_positive()) {
                if label != "settle" {
                    unmeasured += 1;
                }
                lines.push(format!("{label:26} UNMEASURED — the canvas pane recorded no rect"));
                return;
            }
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
            lines.push(format!(
                "{label:26} L{:8.2} T{:7.2} W{:8.2} H{:7.2}{note}",
                r.left(),
                r.top(),
                r.width(),
                r.height()
            ));
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
    ProbeRun {
        lines,
        moved,
        unmeasured,
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
        // The build's version — two machines cross-checked at a glance
        // (owner 2026-08-19).
        ui.label(
            egui::RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                .monospace()
                .size(9.5)
                .color(plate::legend_dim()),
        );
        ui.separator();
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
        binds: &[config::Binding],
    ) {
        // GM2: never lose work — close guard, save modal, title, autosave.
        self.worksafe_frame(ui.ctx());
        // GM7 CHIP PICK: the active chip lasts while the brush holds its
        // colour (a wheel, preset or key that moves the brush clears it).
        self.state.sync_active_chip(self.canvas.brush_color);
        // GM3: the chords every tooltip names; the palette (its keys taken
        // before anything else reads them) and the help windows.
        menus::publish(ui.ctx(), binds);
        self.palette_ui(ui.ctx(), binds);
        self.help_windows(ui.ctx(), binds);
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
                let file_menu_was_open = std::mem::take(&mut self.file_menu_open);
                ui.menu_button("file", |ui| {
                    // GM2: the recent files are checked on disk as the menu
                    // opens, not on every frame it stays open.
                    self.file_menu_open = true;
                    if !file_menu_was_open {
                        self.recent_shown = worksafe::on_disk(&self.recent);
                    }
                    // GM2: New/Open/Recent go through the unsaved guard
                    // (GM3: the registry's items, Editor::invoke).
                    self.menu_item(ui, Action::NewProject);
                    // GM8: templates, recent files and the sample again.
                    self.menu_item(ui, Action::WelcomeScreen);
                    self.menu_item(ui, Action::Open);
                    ui.menu_button("Open Recent", |ui| {
                        if self.recent_shown.is_empty() {
                            ui.label(
                                egui::RichText::new("no recent files").color(plate::legend_dim()),
                            );
                        }
                        for (p, present) in self.recent_shown.clone() {
                            // Cut files repeat across episode folders
                            // (ep01/c012, ep02/c012): the folder tells them
                            // apart at a glance.
                            let name = recent_label(&p);
                            if present {
                                if ui
                                    .button(&name)
                                    .on_hover_text(p.display().to_string())
                                    .clicked()
                                {
                                    self.leave(Leave::OpenPath(p), ui.ctx());
                                    ui.close();
                                }
                            } else {
                                // Missing: greyed, with its own Remove.
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{name} (missing)"))
                                            .color(plate::legend_dim()),
                                    )
                                    .on_hover_text(p.display().to_string());
                                    if ui.small_button("remove").clicked() {
                                        self.recent_edit = Some(RecentEdit::Remove(p.clone()));
                                    }
                                });
                            }
                        }
                        if !self.recent_shown.is_empty() {
                            ui.separator();
                            // Forgets the list only; no file is touched.
                            if ui
                                .button("clear recent")
                                .on_hover_text("empties this list — no file is removed")
                                .clicked()
                            {
                                self.recent_edit = Some(RecentEdit::Clear);
                                ui.close();
                            }
                        }
                    });
                    // Save lands the gesture first (Editor::perform): the
                    // saved file must contain what the screen shows.
                    self.menu_item(ui, Action::Save);
                    self.menu_item(ui, Action::SaveAs);
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
                                    self.state.engine.project.color_law,
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
                                // GM8: the Quick start's last step.
                                self.quick_start[welcome::EXPORT_STEP] = true;
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
                                    self.state.engine.project.color_law,
                                    w,
                                    h,
                                    fps,
                                    path,
                                    a..=b,
                                    ui.ctx().clone(),
                                    cancel.clone(),
                                    self.state.render_queue.configured_ffmpeg(),
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
                                self.quick_start[welcome::EXPORT_STEP] = true;
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
                    ui.separator();
                    self.menu_item(ui, Action::Settings);
                    self.menu_item(ui, Action::Quit);
                });
                // GM3: the registry's own menus.
                self.plain_menus(ui);
                // GM7: the project's colour law and its migration (a DANGER
                // confirm), before the cuts it holds.
                self.project_menu(ui);
                // Scene/cut navigation (C2): the model always supported
                // more than one, there was just no way to reach them.
                // GM3: beside the sheet menu, in the spec's menu order.
                ui.menu_button("cut", |ui| {
                    self.menu_items(ui, "Cut");
                    ui.separator();
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
                    // GM10: Cut ▸ Paper size… is a verb (listed above
                    // with the cut's others); its window holds the fields.
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

                // Slate identity: filename + dirty mark, then the cut's
                // vitals as engraved fields (transport lives in the FOOT;
                // undo/redo live on the keys and the Foot's counter).
                // The identity: the file's stem once one exists; before
                // the first save, the NAME the artist gave the project
                // (owner's report: "Drawing boog" showed as untitled).
                // "untitled" only when both are absent.
                let fname = self
                    .state
                    .file_path
                    .as_ref()
                    .and_then(|p| p.file_stem().map(|os| os.to_string_lossy().to_string()))
                    .or_else(|| {
                        let n = self.state.engine.project.name.trim();
                        (!n.is_empty() && n != "Untitled").then(|| n.to_string())
                    })
                    .unwrap_or_else(|| "untitled".into());
                // AUDIT [3]: these were variable-width labels sitting
                // BEFORE the room tabs, so the tabs slid sideways the
                // instant the dirty mark appeared — on the first stroke
                // of every session. Both fields are now fixed slots with
                // elision inside, and the dirty mark is a painted Tally
                // dot in permanently reserved space (a character would
                // change the string's width).
                // GM2: the real dirty mark (revision vs saved + meta edits),
                // not "has undo history".
                let dirty = self.state.is_dirty();
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
                if dirty {
                    // GM2: the dot says what it means — on hover, and to
                    // screen readers and the testkit (D7).
                    let dot = egui::Rect::from_center_size(
                        egui::pos2(frect.right() - 6.0, frect.center().y),
                        egui::vec2(12.0, 16.0),
                    );
                    let resp = ui.interact(dot, ui.id().with("gm2_unsaved"), egui::Sense::hover());
                    plate::speak(&resp, egui::WidgetType::Label, None, "unsaved changes");
                    // Built only while hovered, never every frame.
                    let at = self.last_autosave_at;
                    let _ = resp.on_hover_ui(|ui| {
                        ui.label(match at {
                            Some(t) => format!(
                                "unsaved changes — not in your file until you save \
                                 (autosaved {})",
                                worksafe::clock_hm(t, worksafe::utc_offset_min())
                            ),
                            None => "unsaved changes — not in your file until you save".into(),
                        });
                    });
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
                        Stage::InkPaint => ("仕上げ", "shiage"),
                        Stage::Composite => ("撮影", "satsuei"),
                        Stage::Edit => ("編集", "henshū"),
                    };
                    // GM8 room card: term, purpose and the tools it arms.
                    let (term, purpose, arms) = st.card();
                    if slate_tab(ui, self.stage == Some(*st), kanji, romaji, st.name())
                        .on_hover_text(format!(
                            "{term} — {purpose}\n{}\narms: {arms}",
                            st.describes()
                        ))
                        .clicked()
                    {
                        self.enter_room(*st);
                    }
                }
                // GM3: was "ws" — the rooms, then the saved arrangements.
                ui.menu_button("room", |ui| {
                    self.menu_items(ui, "Room");
                    ui.separator();
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
                            self.save_workspaces();
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
                            // Off-spine like every custom room (saved rooms
                            // do the same): otherwise a forge room entered
                            // from LAYOUT kept the stage, and the Canvas
                            // pane stayed the read-only composite PREVIEW
                            // instead of test paper — the owner's report.
                            self.stage = None;
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
                        self.save_workspaces();
                    }
                    if let Some(i) = remove {
                        self.workspaces.list.remove(i);
                        self.save_workspaces();
                    }
                });
                // Open or bring forward each pane (a pane lives only while
                // it's in the dock tree). GM3: was "panes".
                ui.menu_button("window", |ui| {
                    for pane in Pane::ALL {
                        // The Brush window is RETIRED (owner 2026-08-17):
                        // its settings live on the canvas deck + right rail.
                        // GM14: the Curves pane is the registry's own item
                        // (Window ▸ Curves (graph editor), listed below);
                        // GM7: so is Window ▸ Nuri check (paint check).
                        if matches!(*pane, Pane::Brush | Pane::Curves | Pane::PaintCheck) {
                            continue;
                        }
                        let present = self.dock.iter_all_tabs().any(|(_, t)| t == pane);
                        let hover = if present { "bring it forward" } else { "open it" };
                        if ui.button(pane.title()).on_hover_text(hover).clicked() {
                            self.open_pane(*pane);
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
                    ui.separator();
                    self.menu_items(ui, "Window");
                });
                self.help_menu(ui);
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
                        // A NOTICE (a reset config, a lost tablet) waits for
                        // the artist: the dwell starts at the first key or
                        // click after it shows, so it is never missed. (Raw
                        // events: a widget that consumed the key still saw
                        // the artist act.)
                        if self.notice_seq == Some(self.state.refusal_seq) {
                            let acted = ui.input(|i| {
                                i.pointer.any_pressed()
                                    || i.raw.events.iter().any(|e| {
                                        matches!(e, egui::Event::Key { pressed: true, .. })
                                    })
                            });
                            if acted {
                                self.notice_seq = None;
                            } else {
                                self.refusal_since = ui.input(|i| i.time);
                            }
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
                        // GM9: each CPU subtree's own eval hash keys its
                        // worker render (memo hits after the eval above).
                        let roots = graphcomp::cpu_roots(self.state.cut());
                        let fx: std::collections::HashMap<_, _> = roots
                            .into_iter()
                            .filter_map(|n| {
                                let v = self.state.engine.eval_node(scene, cutid, n, frame).ok()?;
                                Some((n, v.hash()))
                            })
                            .collect();
                        let cut = self.state.cut();
                        let law = self.state.engine.project.color_law;
                        let id = g.execute(hash, fx, cut, law, frame, p);
                        if g.rendering() {
                            // Poll for the worker's result; the pen never waits.
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
                            viewer::GraphView::Rendering(id)
                        } else {
                            viewer::GraphView::Ready(id)
                        }
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
                    viewer::GraphView::Ready(id) | viewer::GraphView::Rendering(id) => {
                        sh.tex = Some(id)
                    }
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
            pane_rects: &mut self.pane_rects,
        };
        tabs.pane_rects.clear();
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
        // GM14: the verbs a pane asked for (the curves pane's F and its
        // key menu) run through the one dispatch, as a menu item does (D6).
        for action in std::mem::take(&mut self.state.curves.asked) {
            self.invoke(action, ui.ctx());
        }
        // GM6: and the canvas's (the system Copy / Paste events over it).
        for action in std::mem::take(&mut self.canvas.asked) {
            self.invoke(action, ui.ctx());
        }

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
