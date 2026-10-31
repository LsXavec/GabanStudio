//! AnimStudio — app shell (M2: X-sheet + drawing loop; + New Project flow).
//!
//! Two screens: a startup New Project dialog (sets resolution/fps), then the
//! editor (top bar, left X-sheet, central canvas, status bar).

// Release builds are GUI-subsystem: no console window behind the app (GM1).
// CLI paths (the layout probe) re-attach to the launching terminal.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod board_panel;
mod canvas;
mod cmdpal;
mod config;
mod devloop;
mod doc;
mod export;
mod export_step;
mod floatcanvas;
mod floats;
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
mod manga_tools;
#[cfg(test)]
mod measure;
mod menus;
mod net;
mod newproject;
mod nodegraph_panel;
mod paint;
mod paintcheck;
mod palette;
mod palette_panel;
mod plate;
mod playback;
mod proc;
mod rail;
mod render_queue;
mod runs;
mod sample;
mod say;
mod shortcuts;
mod stabilizer;
mod steps;
mod strip;
mod templates;
#[cfg(test)]
mod testkit;
#[cfg(test)]
mod tidy_tests;
#[cfg(test)]
mod audock_tests;
#[cfg(test)]
mod auexport_tests;
#[cfg(test)]
#[path = "../../crates/anim-core/tests/support/gif_decode.rs"]
mod gif_decode;
#[cfg(test)]
mod aurail_tests;
#[cfg(test)]
mod ausay_tests;
#[cfg(test)]
mod austart_tests;
#[cfg(test)]
mod austrip_tests;
mod uidump;
#[cfg(test)]
mod uishot;
mod update;
mod view3d;
mod viewer;
mod vocab;
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
use plate::Card;
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
        Some(Stage::Manga) => "manga",
        // AU9: for the match alone. `arming_key` stands in front of every
        // read and write, so this string reaches no config.
        Some(Stage::Export) => "export",
        None => "custom",
    }
}

/// The room key the arming block reads. Export arms nothing: it keeps the key
/// last armed, `seen`, so the layer and the stack stay as they were.
fn arming_key(stage: Option<Stage>, seen: &'static str) -> &'static str {
    match stage {
        Some(Stage::Export) => seen,
        s => stage_key(s),
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
        viewport: main_viewport(),
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

/// The main window. AU3: never under 1024×640, the smallest gated size.
fn main_viewport() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_inner_size([1720.0, 960.0])
        .with_min_inner_size([1024.0, 640.0])
        .with_title("Gaban Studio")
}

/// The plate visuals and the Plex type stack — the app, the layout probe
/// and the testkit all draw with the same dressing.
fn dress(ctx: &egui::Context) {
    // One look: the same style in both slots under a pinned theme, so
    // Windows Light mode cannot fall back to stock egui.
    let style = plate::style();
    ctx.set_theme(egui::Theme::Light);
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style_of(egui::Theme::Dark, style);
    plate::install_fonts(ctx);
    ctx.add_plugin(plate::FocusRing);
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
            // Most errors end in the usage already: say it once.
            if e.contains(CLI_USAGE) {
                eprintln!("animstudio: {e}");
            } else {
                eprintln!("animstudio: {e}\n{CLI_USAGE}");
            }
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
/// the in-memory sample's) cut — the one `--scene` / `--cut` name, else the
/// first — under the preset into `job.out`. The data dir's config gives the
/// ffmpeg location and the artist; None = the defaults.
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
    let at = anim_core::deliver::pick_cut(&project, job.scene.as_deref(), job.cut.as_deref())?;
    // More than one cut to choose from: say which one this renders.
    let cuts: usize = project.scenes.iter().map(|s| s.cuts.len()).sum();
    if let (true, Some(scene), Some(cut)) = (
        cuts > 1,
        project.scene(at.scene),
        project.cut(at.scene, at.cut),
    ) {
        println!(
            "rendering cut \"{}\" of scene \"{}\" ({cuts} cuts; --scene / --cut pick another)",
            cut.name, scene.name
        );
    }
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
    // GM17: relative ImageSequence frames read from the file's folder.
    let spec = match &job.source {
        anim_core::deliver::CliSource::File(path) => spec.with_base_dir(path.parent().map(|d| d.to_path_buf())),
        anim_core::deliver::CliSource::Sample => spec,
    };
    let typed = config.render.ffmpeg.trim();
    let configured = (!typed.is_empty()).then(|| std::path::PathBuf::from(typed));
    let ffmpeg = export::find_ffmpeg(configured.as_deref());
    let total = spec.frames.1 - spec.frames.0 + 1;
    let never = std::sync::atomic::AtomicBool::new(false);
    let result = render_queue::render_job(&spec, ffmpeg.as_deref(), &never, &mut |f| {
        println!("frame {}/{total}", f - spec.frames.0 + 1);
    });
    for w in spec.env.warnings() {
        println!("warning: {w}");
    }
    result
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
    /// GM21: Settings ▸ General's ffmpeg answer while no editor is open
    /// (Welcome); an open editor's render queue answers instead.
    ffmpeg_queue: render_queue::RenderQueue,
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

impl AudioOut {
    /// The default output device. GM12 (NEVER-DO 7): a test build refuses
    /// before rodio is touched — tests never open an audio device.
    fn open() -> Result<AudioOut, String> {
        if cfg!(test) {
            return Err("tests never open an audio device".into());
        }
        let stream = rodio::OutputStreamBuilder::open_default_stream().map_err(|e| e.to_string())?;
        let sink = rodio::Sink::connect_new(stream.mixer());
        Ok(AudioOut {
            _stream: stream,
            sink,
        })
    }
}

/// Zero-copy rodio source over the engine's Arc'd decoded samples — a
/// restart must never clone a multi-hundred-MB sample buffer. GM12: it
/// ends at `end` (a scrub snippet) and publishes its position in `cursor`
/// — the audio clock reads how far the sound has got from it.
struct ClipSource {
    data: std::sync::Arc<Vec<f32>>,
    pos: usize,
    end: usize,
    cursor: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    channels: u16,
    rate: u32,
}

impl Iterator for ClipSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.end {
            return None;
        }
        let v = self.data.get(self.pos).copied();
        self.pos += 1;
        self.cursor
            .store(self.pos, std::sync::atomic::Ordering::Relaxed);
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
            // GM16: ◆, not ⌂, which no embedded face draws.
            e.preset.name = format!("◆ {}", e.preset.name);
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
                let layout = self.editor.as_ref().and_then(Editor::host_layout);
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
                                ed.say.new_document();
                                ed.state.status = "live from the host".into();
                            } else {
                                let rs = self.render_state.as_ref();
                                let mut ed =
                                    Editor::boot(st, rs, self.data_dir.as_deref(), &self.config.ui);
                                ed.stage = self.editor.as_ref().and_then(Editor::stage_kept);
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
                    Some(Editor::boot(
                        state,
                        cc.wgpu_render_state.as_ref(),
                        data_dir.as_deref(),
                        &config.ui,
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
            ffmpeg_queue: render_queue::RenderQueue::default(),
        }
    }

    /// GM2, once per frame: the start-up crash check (first frame), the
    /// editor's requests, the recent list and the running record.
    fn worksafe_shell(&mut self, ctx: &egui::Context) {
        // With no data dir there is no record to keep, and the editor's
        // requests (every Start choice among them) still go out.
        let Some(dir) = self.data_dir.as_deref() else {
            let requests = self.editor.as_mut().map(|ed| std::mem::take(&mut ed.requests));
            for r in requests.into_iter().flatten() {
                self.handle_request(ctx, r);
            }
            return;
        };
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
            .frame(plate::dialog_frame(&ctx.global_style()))
            .show(ctx, |ui| worksafe::recovery_ui(ui, offer, now, offset))
            .inner;
        let Some(choice) = choice else { return };
        let offer = self.recovery.remove(0);
        match choice {
            worksafe::RecoveryChoice::Open(r) => match &mut self.editor {
                Some(ed) => ed.leave(Leave::Recover(r), ctx),
                None => match worksafe::open_recovered(&r) {
                    Ok(state) => {
                        self.editor = Some(Editor::boot(
                            state,
                            self.render_state.as_ref(),
                            self.data_dir.as_deref(),
                            &self.config.ui,
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
                    canvas::CanvasTool::Panel => "panel (koma)",
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
        // GM11: a render queue job is never cut off half-written.
        if editor.state.render_queue.busy() {
            self.devloop.note = Some("rebuild ready — waiting for the render queue".into());
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
            // GM21: asked about unsaved work first, like Open.
            AppRequest::OpenSample => {
                let Some(ed) = &mut self.editor else { return };
                ed.leave(Leave::Sample, ctx);
            }
            AppRequest::ImportSound | AppRequest::AddSoundTrack => {
                let Some(ed) = &mut self.editor else { return };
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Sound (WAV, MP3, OGG, FLAC)", &["wav", "mp3", "ogg", "flac"])
                    .pick_file()
                {
                    let as_track = req == AppRequest::AddSoundTrack;
                    ed.state.begin_sound_import(&path, as_track, Some(ctx.clone()));
                }
            }
            AppRequest::LoadLut { node, key } => {
                let Some(ed) = &mut self.editor else { return };
                if let Some(path) = rfd::FileDialog::new().add_filter("3D LUT", &["cube"]).pick_file() {
                    ed.state.set_lut_file(node, key, &path);
                }
            }
            AppRequest::PickSequenceFrame { node } => {
                let Some(ed) = &mut self.editor else { return };
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("A frame (PNG, EXR, TIFF, JPEG)", &["png", "exr", "tif", "tiff", "jpg", "jpeg"]);
                if let Some(dir) = ed.state.project_dir() {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(path) = dialog.pick_file() {
                    ed.state.set_sequence_from_file(node, &path);
                }
            }
            // GM17: the model picker is the shell's; the import is one undo
            // step, and View3D comes forward to show it.
            AppRequest::Import3DModel => {
                let Some(ed) = &mut self.editor else { return };
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("3D model (glTF, OBJ)", anim_core::r3d::FORMATS)
                    .pick_file()
                {
                    let pos = (40.0, 40.0 + 90.0 * ed.state.cut().graph.nodes().count() as f32);
                    if ed.state.import_model(&path, pos).is_some() {
                        // Unwired (a hand-built graph with no Output): the
                        // graph comes forward, the node selected in it.
                        if ed.state.cut().graph.output.is_none() {
                            ed.open_pane(Pane::NodeGraph);
                        } else {
                            ed.open_pane(Pane::View3D);
                        }
                    }
                }
            }
            // AU6: a Start float choice, past the guard. The seam says what
            // it does; this is the part with a window to do it in.
            AppRequest::Start(action) => {
                let (rs, dir) = (self.render_state.as_ref(), self.data_dir.as_deref());
                match apply_start(action, rs, dir, &self.config.ui) {
                    StartOutcome::Replace(editor) => self.editor = Some(*editor),
                    StartOutcome::Request(r) => self.handle_request(ctx, r),
                    StartOutcome::Failed(why) => self.notes.push(why),
                    StartOutcome::Stay => {}
                }
            }
            // GM18: the pages' folder is the shell's; the Manga pane's
            // options say how they are written.
            AppRequest::ExportPages => {
                let Some(ed) = &mut self.editor else { return };
                let mut dialog = rfd::FileDialog::new().set_title("Export the pages into a folder");
                if let Some(dir) = ed.state.project_dir() {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(dir) = dialog.pick_folder() {
                    ed.state.begin_page_export(&dir, Some(ctx.clone()));
                }
            }
            // AU9: the plain exports' one dialog. Past it, `export_to` is the
            // same seam a test answers.
            AppRequest::ExportTo(kind) => {
                let Some(ed) = &mut self.editor else { return };
                if let Some(path) = export_dialog(&ed.state, kind) {
                    ed.export_to(ctx, kind, &path);
                }
            }
            AppRequest::OpenFile(path) => proc::open_file(&path),
            AppRequest::ShowInFolder(path) => proc::show_in_folder(&path),
        }
    }
}

/// AU9: where a plain export goes, asked once: a folder for a picture
/// sequence, a name you choose for the others.
fn export_dialog(state: &AppState, kind: render_queue::ExportKind) -> Option<std::path::PathBuf> {
    use render_queue::ExportKind;
    let mut dialog = rfd::FileDialog::new();
    if let Some(dir) = export::suggest_dir(state) {
        dialog = dialog.set_directory(dir);
    }
    let save = |dialog: rfd::FileDialog, title: &str, filter: &str, ext: &str, name: &str| {
        let dialog = dialog.set_title(title).add_filter(filter, &[ext]);
        dialog.set_file_name(format!("{name}.{ext}")).save_file()
    };
    match kind {
        ExportKind::Png => dialog.set_title("Export a picture sequence into a folder").pick_folder(),
        ExportKind::Gif => save(dialog, "Export an animated GIF", "Animated GIF", "gif", "animation"),
        ExportKind::Mp4 => save(dialog, "Export a video", "MP4 video", "mp4", "animation"),
        ExportKind::Still => save(dialog, "Export a picture", "PNG image", "png", "picture"),
    }
}

/// What [`Editor::perform`] did with an action.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppRequest {
    /// AU6: what the Start float chose (spec 8.1).
    Start(welcome::WelcomeAction),
    NewProject,
    Open,
    /// GM21: File ▸ Open the sample cut.
    OpenSample,
    Save,
    SaveAs,
    /// GM3: raised by the menus, the palette and their chords.
    Settings,
    Quit,
    /// GM12: File ▸ Import sound… and Add sound track… — the file picker
    /// is the shell's (it is a window); the decode then runs on a worker.
    ImportSound,
    AddSoundTrack,
    /// GM17: File ▸ Import 3D model… — the model picker is the shell's.
    Import3DModel,
    /// GM13: the inspector's "load .cube…" for this node's file parameter
    /// — the .cube picker is the shell's too (a window, on a real click).
    LoadLut {
        node: anim_core::ids::NodeId,
        key: &'static str,
    },
    /// GM17: an ImageSequence's "pick a frame…" — the image picker is the
    /// shell's; the file names the node's run.
    PickSequenceFrame { node: anim_core::ids::NodeId },
    /// GM18: File ▸ Export ▸ Manga pages… — the folder picker is the
    /// shell's; the pages then render and write on a worker.
    ExportPages,
    /// AU9: Animated GIF, Video (MP4), Picture sequence (PNG) and This frame
    /// (PNG), from the Export step or File ▸ Export ▸: the save dialog is the
    /// shell's, the job then runs in the queue.
    ExportTo(render_queue::ExportKind),
    /// AU9: `Open file`, and `Show in folder`: the operating system's, past
    /// a file already written.
    OpenFile(std::path::PathBuf),
    ShowInFolder(std::path::PathBuf),
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
            self.say.added |= action == Action::AddDrawingAfter;
        }
        #[cfg(test)]
        if out == Performed::Ignored {
            self.ignored.push(action);
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
            // AU6: the Start float again (spec 8.1): it opens nothing but a
            // float, so no guard asks; a card on it that replaces the document
            // goes through the guard itself.
            Action::WelcomeScreen => self.open_start(welcome::Welcome::over_editor(&self.recent)),
            // AU6: the same float, on its other tab.
            Action::WhatsNew => {
                let mut w = welcome::Welcome::over_editor(&self.recent);
                w.tab = welcome::WelcomeTab::WhatsNew;
                self.open_start(w);
            }
            // AU6: the shell stores it, as Settings does.
            Action::ToggleLockPositions => self.lock_request = Some(!self.ui_lock),
            // AU7: and this.
            Action::ToggleHints => self.say.hints_request = Some(self.say.off),
            // GM10: the cut's paper window (its "set paper" is the edit).
            Action::PaperSize => self.paper_form = Some(PaperForm::of(&self.state)),
            // GM11: the delivery jobs' pane, and the composite viewers'
            // final look (a view: the document is untouched).
            // AU9: this asks for the queue itself, so its fold opens with it.
            Action::ShowRenderQueue => {
                self.open_pane(Pane::RenderQueue);
                self.state.render_queue.panel.unfold();
            }
            // GM15: the editorial's panes; the Episode job (the queue comes
            // forward with it); the retake note window on this frame.
            Action::ShowCutBoard => self.open_pane(Pane::Board),
            Action::ShowEpisode => self.open_pane(Pane::Episode),
            Action::RenderEpisode => {
                let scene = self.state.at().scene;
                if render_queue::add_episode_job(&mut self.state, scene) {
                    self.open_pane(Pane::RenderQueue);
                }
            }
            Action::AddRetakeNote => {
                let (at, frame) = (self.state.at(), self.state.view.frame);
                self.state.board.open_note(at, frame);
            }
            Action::ToggleFinalLook => {
                self.final_look.on = !self.final_look.on;
                self.state.status = if self.final_look.on {
                    "final look: the composite as the master delivers it (no rough, reference or \
                     correction layers)"
                        .into()
                } else {
                    "final look off: the composite shows every layer".into()
                };
            }
            // GM22: the canvas's 3D underlay (a view, kept for the session:
            // no undo step, the document clean) — refused with no model.
            Action::Underlay3d => {
                let u = &mut self.canvas.underlay;
                if !u.on && view3d::model_node(self.state.cut(), self.state.selected_node).is_none() {
                    self.state.refuse(format!("refused — {}", view3d::NO_MODEL));
                } else {
                    u.on = !u.on;
                    self.state.status = if u.on {
                        "3D underlay: the cut's model under the drawing (a view, not in the export)".into()
                    } else {
                        "3D underlay off".into()
                    };
                }
            }
            // GM22: the sequences' files read again (a look at the disk: no
            // undo step, nothing dirty).
            Action::ReloadSequences => {
                let n = self.state.reload_sequences();
                if n > 0 {
                    self.final_look.reread();
                }
                self.state.status = match n {
                    0 => "no image sequence in the project".into(),
                    1 => "image sequence reloaded".into(),
                    n => format!("{n} image sequences reloaded"),
                };
            }
            Action::FrameCurves => curves_panel::frame_stored(&mut self.state),
            // AU5: Enter opens the sheet and starts typing in any place.
            Action::TypeIntoCell => {
                self.show_sheet();
                self.state.sheet_ui.ask_entry();
            }
            // AU5: the counter turns into a field for one frame number.
            Action::GoToFrame => self.frame_entry = Some(String::new()),
            // GM9: the graph comes forward and opens its add-node menu on
            // its next frame (the node it adds is the edit).
            Action::AddNode => {
                self.open_pane(Pane::NodeGraph);
                self.state.graph_ui.ask_add_menu();
            }
            // AU3: Reset stays where it is: a step takes its recipe back, and
            // a place off the spine the arrangement it opened with. The pill,
            // the lit step, the status and the dock agree either way.
            Action::ResetLayout => match (self.stage, &self.place_dock) {
                (Some(st), _) => {
                    self.dock = self.recipe(st);
                    self.state.status = format!("layout reset: {}", st.name());
                }
                (None, Some(dock)) => {
                    self.dock = dock.clone();
                    self.state.status = format!("layout reset: {}", self.place);
                }
                (None, None) => self.enter_room(Stage::Drawing),
            },
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
            Action::RoomExport => self.enter_room(Stage::Export),
            // GM17: the 3D layout room, built in and off the spine like the
            // forge's; View3D's numpad verbs move its view only.
            Action::Layout3dRoom => {
                self.dock = Pane::layout3d_dock();
                self.go_off_spine(vocab::LAYOUT_3D);
                self.state.status = "the 3D layout room".into();
            }
            // GM18: the manga room, off the spine (D12).
            Action::MangaRoom => self.enter_room(Stage::Manga),
            Action::View3dFrameAll => view3d::frame_all(&mut self.state),
            a => {
                if let Some(nav) = view3d::nav_of(a) {
                    view3d::perform_nav(&mut self.state, nav);
                }
            }
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
        // GM18: the panel (koma) tool, a toggle like the fills; it works on
        // a page only (set_tool says so elsewhere).
        if action == Action::PanelTool {
            let next = if ed.canvas.tool == canvas::CanvasTool::Panel {
                canvas::CanvasTool::Paint
            } else {
                canvas::CanvasTool::Panel
            };
            ed.canvas.set_tool(next, &mut ed.state);
            return;
        }
        if action == Action::SelectAll {
            ed.canvas.select_all(&mut ed.state); // GM20: the whole paper
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
        // AU4: a float, so it never opens under a live pen.
        if action == Action::CanvasOptions {
            let busy = ed.canvas.stroke_active();
            ed.floats.toggle(floats::Float::CanvasOptions, busy);
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
        if matches!(
            action,
            Action::Save | Action::SaveAs | Action::Open | Action::OpenSample
        ) {
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
            // AU5: the strip opens on the drawing that needs it; the one
            // refit that costs waits for the pen, this being guarded.
            Action::AddDrawingAfter => {
                if s.add_drawing_after() {
                    ed.strip_open = true;
                }
            }
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
            // GM12: the loop range, per cut, at the cursor.
            Action::LoopIn => s.set_loop_in(),
            Action::LoopOut => s.set_loop_out(),
            Action::ClearLoop => {
                if s.clear_loop() {
                    s.status = "loop range cleared — playback runs the whole cut".into();
                } else {
                    s.refuse("refused — this cut has no loop range");
                }
            }
            Action::ToggleScrub => {
                ed.scrub_audio = !ed.scrub_audio;
                ed.scrub_request = Some(ed.scrub_audio);
                s.status = if ed.scrub_audio {
                    "audio scrub on: a stopped playhead's move sounds its frame".into()
                } else {
                    "audio scrub off".into()
                };
            }
            Action::RamPreview => s.status = s.ram.toggle().into(),
            Action::LipFlap => match playback::LipForm::of(s) {
                Some(form) => ed.lip_form = Some(form),
                None => s.refuse("refused — import a dialogue track first (File ▸ Import sound…)"),
            },
            // GM12: the file picker is the shell's; one decode at a time.
            Action::ImportSound | Action::AddSoundTrack => match &s.sound_import {
                Some(job) => {
                    let msg = format!("refused — {} is still decoding", job.name);
                    s.refuse(msg);
                }
                None => {
                    *out = Performed::App(if action == Action::ImportSound {
                        AppRequest::ImportSound
                    } else {
                        AppRequest::AddSoundTrack
                    });
                }
            },
            // GM17: the model picker is the shell's; the cut camera keyed
            // from View3D's view is the document's (one undo step).
            Action::Import3DModel => *out = Performed::App(AppRequest::Import3DModel),
            Action::SetCutCameraFromView => view3d::set_camera_from_view(s),
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
            // GM13: and the satsuei look's chain, asked the same way.
            Action::BuildSatsuei => s.request_satsuei(),
            // GM15: the editorial's verbs on the open scene and cut (the
            // exports land in the render folder).
            Action::PlayEpisode => s.toggle_episode_play(),
            Action::ExportEdl => {
                s.export_scene_edl();
            }
            Action::ExportColorSheet => {
                s.export_color_sheets_here();
            }
            Action::ExportCutAsProject => {
                let at = s.at();
                s.export_cut_project(at);
            }
            Action::MakeCutsFromPanels => s.make_cuts_from_panels(),
            // GM18: a page after the book's last; the page graph rebuilt.
            Action::NewPage => {
                let scene = s.at().scene;
                s.new_page(scene);
            }
            Action::ClipToPanels => {
                s.clip_to_panels();
            }
            // GM18: the folder picker is the shell's; one export at a time,
            // and a book with no page has nothing to write.
            Action::ExportPages => match s.page_export_refusal() {
                Some(why) => s.refuse(format!("refused — {why}")),
                None => *out = Performed::App(AppRequest::ExportPages),
            },
            // AU9: the save dialog is the shell's; the job then runs on the
            // queue's worker. Video's search has its first answer here if the
            // menu has not asked yet: no dialog for a job that could only fail.
            Action::ExportGif => *out = Performed::App(AppRequest::ExportTo(render_queue::ExportKind::Gif)),
            Action::ExportMp4 => match s.render_queue.ffmpeg_path() {
                Some(_) => *out = Performed::App(AppRequest::ExportTo(render_queue::ExportKind::Mp4)),
                None => s.refuse(format!("refused — {}", export_step::NO_FFMPEG)),
            },
            Action::ExportPngSequence => *out = Performed::App(AppRequest::ExportTo(render_queue::ExportKind::Png)),
            Action::ExportStill => *out = Performed::App(AppRequest::ExportTo(render_queue::ExportKind::Still)),
            // The stroke latches its rulers at pen-down: a flip mid-stroke
            // holds from the next one.
            Action::ToggleRulerSnap => {
                s.manga.snap = !s.manga.snap;
                s.status = match (s.manga.snap, s.manga.snap_rulers(s.cut()).is_some()) {
                    (false, _) => "snap to rulers off — the pen draws free".into(),
                    (true, true) => "snap to rulers on — a stroke runs to a vanishing point, or level".into(),
                    (true, false) => "snap to rulers on — this page has no rulers yet (the Manga pane sets them)".into(),
                };
            }
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
            | Action::CanvasOptions
            | Action::SelectTool
            | Action::BrushTool
            | Action::FillTool
            | Action::LassoFillTool
            | Action::PanelTool
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
            Action::Open | Action::OpenSample if ed.canvas.is_guest => {
                s.refuse("refused — leave the room first (Settings › Session) to open your own file");
                *out = Performed::Ignored;
            }
            // GM21: the sample replaces the document, through the guard.
            Action::OpenSample => *out = Performed::App(AppRequest::OpenSample),
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
            | Action::RoomExport
            | Action::ShowCurves
            | Action::FrameCurves
            | Action::ShowPaintCheck
            | Action::ConvertColorsToSrgb
            | Action::ProjectSettings
            | Action::WelcomeScreen
            | Action::WhatsNew
            | Action::ToggleLockPositions
            | Action::ToggleHints
            | Action::PaperSize
            | Action::ShowRenderQueue
            | Action::ToggleFinalLook
            | Action::TypeIntoCell
            | Action::GoToFrame
            | Action::AddNode
            | Action::ShowCutBoard
            | Action::ShowEpisode
            | Action::RenderEpisode
            | Action::AddRetakeNote
            | Action::Layout3dRoom
            | Action::MangaRoom
            | Action::View3dFront
            | Action::View3dBack
            | Action::View3dRight
            | Action::View3dLeft
            | Action::View3dTop
            | Action::View3dBottom
            | Action::View3dCamera
            | Action::View3dOrtho
            | Action::View3dFrameAll
            | Action::Underlay3d
            | Action::ReloadSequences => {} // the chrome's, handled above
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
            | Action::RoomExport
            | Action::ShowCurves
            | Action::FrameCurves
            | Action::ShowPaintCheck
            | Action::ConvertColorsToSrgb
            | Action::ProjectSettings
            | Action::WelcomeScreen
            | Action::WhatsNew
            | Action::ToggleLockPositions
            | Action::ToggleHints
            | Action::PaperSize
            | Action::ShowRenderQueue
            | Action::ToggleFinalLook
            | Action::TypeIntoCell
            | Action::GoToFrame
            | Action::AddNode
            // GM15: the editorial's panes, the queue's Episode job and the
            // note window — the document is untouched until its "add note".
            | Action::ShowCutBoard
            | Action::ShowEpisode
            | Action::RenderEpisode
            | Action::AddRetakeNote
            // GM17: the 3D layout room, and View3D's view (a view: the
            // document is untouched).
            | Action::Layout3dRoom
            // GM18: the manga room (a view).
            | Action::MangaRoom
            | Action::View3dFront
            | Action::View3dBack
            | Action::View3dRight
            | Action::View3dLeft
            | Action::View3dTop
            | Action::View3dBottom
            | Action::View3dCamera
            | Action::View3dOrtho
            | Action::View3dFrameAll
            // GM22: the 3D underlay (a view) and the sequences read again
            // (the disk, not the document).
            | Action::Underlay3d
            | Action::ReloadSequences
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
            // AU5: the strip's add, the sheet coming up, and a jump: none
            // lands under a live stroke.
            | Action::AddDrawingAfter
            | Action::TypeIntoCell
            | Action::GoToFrame
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
            // GM12: no file picker, no lip-flap window, and no worker
            // starts mid-stroke (NEVER-DO 6).
            | Action::ImportSound
            | Action::AddSoundTrack
            | Action::RamPreview
            | Action::LipFlap
            // GM13: the satsuei graph replaces nodes the stroke may feed.
            | Action::BuildSatsuei
            // GM15: no cut switch, file write, cuts made, job queued or
            // note window opened under a live stroke (D7).
            | Action::PlayEpisode
            | Action::ExportEdl
            | Action::ExportColorSheet
            | Action::ExportCutAsProject
            | Action::MakeCutsFromPanels
            | Action::RenderEpisode
            | Action::AddRetakeNote
            // GM17: no model or node added, no camera keyed and no room
            // rebuilt under a live stroke.
            | Action::Import3DModel
            | Action::SetCutCameraFromView
            | Action::Layout3dRoom
            // GM18: no page made, graph rebuilt, room swapped, folder
            // picker opened or export worker started under a live stroke.
            | Action::MangaRoom
            | Action::NewPage
            | Action::ClipToPanels
            | Action::ExportPages
            // AU9: nor the Export step, a dialog or a job.
            | Action::RoomExport
            | Action::ExportGif
            | Action::ExportMp4
            | Action::ExportPngSequence
            | Action::ExportStill
            // GM22: nothing starts mid-stroke — no underlay turned on, no
            // sequence reread.
            | Action::Underlay3d
            | Action::ReloadSequences
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
    /// AU3: `ui.sheet_in_draw`, mirrored here because `enter_room` and
    /// `from_state` see no config. A template's sheet stays on this field,
    /// for this project.
    sheet_in_draw: bool,
    /// AU3: the Timing sheet button's choice, for the App and the Rig to
    /// remember. Only the button asks.
    sheet_request: Option<bool>,
    /// AU3: where the arrangement came from while `stage` is None: the
    /// off-spine pill's name, and the arrangement it opened with. Reset goes
    /// back to it, and the lock keeps its panes (a panel opened by hand
    /// keeps its close control).
    place: String,
    place_dock: Option<DockState<Pane>>,
    /// AU9: the way back out of the Export step: the stage it was entered
    /// from and the arrangement it left, whole. None outside the step.
    export_from: Option<(Option<Stage>, DockState<Pane>)>,
    /// AU9: the panels' folds before anyone presses them: closed for a
    /// person, open under the frozen boot (spec 1.5.2).
    pub(crate) more_default: bool,
    /// AU3: the panes the lock holds, and the stage and sheet choice they
    /// were read for. The dock asks every frame; the recipe changes rarely.
    held: Vec<Pane>,
    held_for: Option<(Option<Stage>, bool)>,
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
    /// GM11: the composite viewers' "final look" — the master's purpose
    /// rendered on a worker (File ▸ Export… itself adds render queue jobs).
    pub final_look: viewer::FinalLook,
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
    /// WHAT is sounding: (scene, cut, the mix's key — GM12: every track's
    /// settings and clip identity). A cut switch, a mute, or an undo/redo
    /// that swaps a clip mid-play changes this key → the sound restarts on
    /// the right mix instead of the stale one playing on.
    audio_key: Option<(u64, u64, u64)>,
    /// GM12: the cut's mix (stereo at `audio::MIX_RATE`) under its key —
    /// a seek restarts from it without mixing again.
    audio_mix: Option<(u64, std::sync::Arc<Vec<f32>>)>,
    /// GM12: the audio clock while the mix sounds, with the playing
    /// source's sample position and the sample it started from.
    audio_clock: Option<(
        playback::PlayClock,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        usize,
    )>,
    /// GM12: frames passed over to keep up since playback started (the
    /// transport's "dropped N").
    play_dropped: u64,
    /// GM12: playback was running last frame (a new run counts from 0).
    play_running: bool,
    /// GM12: this run started past the loop range's end, so it plays on
    /// to the cut's end (play from the cursor never jumps back).
    play_whole: bool,
    /// GM12: a stopped playhead's move sounds one frame of the mix — the
    /// config's `perf.scrub_audio`, fed in each frame.
    scrub_audio: bool,
    /// GM12: the scrub latch or verb flipped it; the shell stores it in
    /// the config (remembered across sessions).
    scrub_request: Option<bool>,
    /// GM12: the last scrub snippet sounded (the tests read it).
    last_snippet: Option<playback::Snippet>,
    /// GM12: the mix being made on a worker.
    mix_job: Option<playback::MixJob>,
    /// GM22: the window came back mid-stroke: the image sequences are read
    /// again on the first frame after the pen lifts.
    reload_pending: bool,
    /// GM12: Sheet ▸ Lip flap…'s window is open with this form.
    lip_form: Option<playback::LipForm>,
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
    /// pointer picks which shortcuts apply. AU4: with its layer, so a float
    /// over a pane speaks for itself; the rail and floats join as the panes
    /// they serve.
    pane_rects: Vec<(Pane, egui::Rect, egui::LayerId)>,
    /// AU4: who is open over the canvas. Not saved.
    floats: floats::Floats,
    /// AU6: the Start float, drawn in `Editor::ui`, so nothing but a stroke
    /// on its own rect is ever behind a question.
    pub start: Option<welcome::Welcome>,
    /// AU6: asked for during a stroke: it opens at pen-up (spec 4.5).
    start_waiting: Option<welcome::Welcome>,
    /// AU6: `strokes_done` when last looked at. A change is a finished stroke.
    strokes_seen: u64,
    /// AU7: the Say line's teaching (`say.rs`).
    pub say: say::Say,
    /// AU6: `config.ui.show_welcome`, fed by the shell like the lock, and the
    /// float's latch asking the shell to store it.
    pub show_start: bool,
    pub show_start_request: Option<bool>,
    /// AU6: the shell has a question up (the recovery offer, the New form):
    /// fed each frame, so a dropped file waits its turn.
    pub shell_asks: bool,
    /// AU6: Lock UI positions this frame, and the verb asking the shell to
    /// store a flip.
    ui_lock: bool,
    pub lock_request: Option<bool>,
    /// AU4: the strip's latch, for this session: set when a document opens
    /// with more than one frame, by Add drawing and by the Animation intent
    /// (AU6), and never cleared by itself. The band and the rail's reference
    /// height read it.
    strip_open: bool,
    /// AU4: the full-band latch (spec R17): set by the second drawing and
    /// never cleared in a session. The rail's Flip reads it, AU5's band too.
    full_band: bool,
    /// AU5: the simple timeline's gesture and scale. Not saved.
    strip: strip::Strip,
    /// AU5: the document the strip's latch and scale were set for, so a
    /// frame added later never refits the paper under a stroke.
    opened_doc: Option<String>,
    /// AU5: playback was running last frame: a stop is seen as it happens.
    was_playing: bool,
    /// AU5: the frame number being typed, while the counter is a field.
    frame_entry: Option<String>,
    /// AU1: every action `perform` answered `Ignored`, however it ran (a
    /// key, a click, the palette). The journey `Recorder` reads it.
    #[cfg(test)]
    pub ignored: Vec<Action>,
    /// AU1: this frame's graph view. The GPU shots wait on it.
    #[cfg(test)]
    pub dbg_graph: viewer::GraphView,
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

/// GM8 start-up route: the Start float unless its latch is off (then the
/// same paper with no float, AU6).
fn startup_shows_welcome(config: &Config) -> bool {
    config.ui.show_welcome
}

/// GM8: what the shell does about its start-up screens this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Startup {
    /// A cut, a form or the screen is up: leave it.
    Keep,
    /// AU6: the ready paper, with the Start float over it.
    Welcome,
    /// AU6: the ready paper alone.
    ReadyPaper,
    /// A cut opened under the start-up screen (a restore, a card): drop it.
    DropWelcome,
}

/// GM8: the start-up route from the facts alone — nothing open and no
/// dialog makes the ready paper (AU6), with the Start float unless its latch
/// is off; a cut opening drops a start-up screen, but not one asked for over
/// the cut (File ▸ Welcome screen…).
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
                Startup::ReadyPaper
            }
        }
        Some(w) if editor && !w.over_editor => Startup::DropWelcome,
        _ => Startup::Keep,
    }
}

/// AU3: the dock's style for this frame (spec 4.6). A dock of one tab draws
/// flush: no tab bar and no margin, so its pane body is the centre rect.
fn dock_style(base: &egui::Style, dock: &DockState<Pane>) -> egui_dock::Style {
    let mut s = plate::dock_style(base);
    s.dock_area_padding = None;
    // egui_dock clamps every divider so each side keeps `separator.extra`
    // pixels — the DEFAULT IS 175px, which silently blocked collapsing
    // panes. 26px keeps the tab bar grabbable while letting panes shrink to
    // slim rails.
    s.separator.extra = 26.0;
    s.separator.width = 6.0;
    let flush = dock.iter_all_tabs().count() == 1;
    s.tab_bar.height = if flush { 0.0 } else { 34.0 };
    if flush {
        s.tab.tab_body.inner_margin = egui::Margin::ZERO;
        s.tab.tab_body.stroke = egui::Stroke::NONE;
        s.tab_bar.hline_color = egui::Color32::TRANSPARENT;
    }
    s
}

/// AU3: `a` and `b` are one arrangement: the same leaves, tabs and splits,
/// wherever their dividers stand and whichever tabs are in front.
fn same_shape(a: &DockState<Pane>, b: &DockState<Pane>) -> bool {
    type Shape = Vec<(std::mem::Discriminant<egui_dock::Node<Pane>>, Vec<Pane>)>;
    let shape = |d: &DockState<Pane>| -> Shape {
        d.main_surface()
            .iter()
            .filter(|n| !matches!(n, egui_dock::Node::Empty))
            .map(|n| (std::mem::discriminant(n), n.tabs().unwrap_or_default().to_vec()))
            .collect()
    };
    a.iter_all_tabs().count() == b.iter_all_tabs().count() && shape(a) == shape(b)
}

/// AU3: the zoom a stored scale gets in a window `w`×`h` points at 100%: never
/// past what leaves 760×520 points of room. The stored scale never changes. A
/// window with no size yet keeps the scale.
fn effective_scale(stored: f32, w: f32, h: f32) -> f32 {
    if w > 0.0 && h > 0.0 { stored.min(w / 760.0).min(h / 520.0) } else { stored }
}

/// GM8: the UI scale (Settings ▸ General) as egui's zoom factor, capped by
/// the window (AU3).
fn apply_ui_scale(ctx: &egui::Context, config: &Config) {
    let window = ctx.content_rect().size() * ctx.zoom_factor();
    ctx.set_zoom_factor(effective_scale(config.ui.zoom(), window.x, window.y));
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
#[cfg(test)]
fn forget_recent(config: &mut Config, w: &mut welcome::Welcome, path: &std::path::Path) {
    config.session.recent.retain(|p| p != path);
    w.forget(path);
}

/// GM8: a template's project in its first room.
fn open_template(
    i: usize,
    rs: Option<&RenderState>,
    data_dir: Option<&std::path::Path>,
    ui: &config::UiConfig,
) -> Editor {
    let t = &templates::ALL[i.min(templates::ALL.len() - 1)];
    let mut editor = Editor::boot(t.build(), rs, data_dir, ui);
    editor.enter_template(t);
    editor
}

/// GM8: "Show me" on the Welcome screen — the sample, its empty column B
/// active on frame 0 (a new drawing needs an empty cell), then the step.
fn welcome_show_me(
    step: usize,
    rs: Option<&RenderState>,
    data_dir: Option<&std::path::Path>,
    ui: &config::UiConfig,
) -> (Editor, Performed) {
    let mut editor = Editor::boot(sample::build_practice(), rs, data_dir, ui);
    let did = editor.show_me(step);
    (editor, did)
}

/// AU6: an entry of `INTENTS`: one of the Start float's cards, or the ready
/// paper, which is the first of them.
fn open_intent(
    i: usize,
    rs: Option<&RenderState>,
    data_dir: Option<&std::path::Path>,
    ui: &config::UiConfig,
) -> Editor {
    let i = i.min(templates::INTENTS.len() - 1);
    let mut editor = Editor::boot(templates::INTENTS[i].build(), rs, data_dir, ui);
    editor.enter_intent(i);
    editor
}

/// AU6: the Start float over `editor`, at start-up: a new version's first
/// launch opens it on What's New, once. True when the config changed (the
/// caller saves).
fn start_over(editor: &mut Editor, config: &mut Config) -> bool {
    let (w, seen) = welcome::open_at_startup(&mut config.ui, &config.session.recent);
    editor.open_start(w);
    seen
}

/// AU6: what a Start choice does.
pub(crate) enum StartOutcome {
    /// Another document, in an editor of its own.
    Replace(Box<Editor>),
    /// The shell's to carry out: its dialogs, its windows.
    Request(AppRequest),
    /// It could not be done, and this says why. No document was touched.
    Failed(String),
    Stay,
}

/// AU6: what a Start choice does. No App, no window, no dialog: the unsaved
/// guard has already been past ([`Editor::choose_start`]).
pub(crate) fn apply_start(
    a: welcome::WelcomeAction,
    rs: Option<&RenderState>,
    dir: Option<&std::path::Path>,
    ui: &config::UiConfig,
) -> StartOutcome {
    use welcome::WelcomeAction as Choice;
    let replace = |editor: Editor| StartOutcome::Replace(Box::new(editor));
    match a {
        Choice::Template(i) => replace(open_template(i, rs, dir, ui)),
        Choice::Intent(i) => replace(open_intent(i, rs, dir, ui)),
        Choice::Custom => StartOutcome::Request(AppRequest::NewProject),
        Choice::Open => StartOutcome::Request(AppRequest::Open),
        Choice::OpenRecent(path) => match AppState::load_from(path) {
            Ok(state) => replace(Editor::boot(state, rs, dir, ui)),
            Err(msg) => StartOutcome::Failed(format!("could not open that project — {msg}")),
        },
        Choice::Sample => replace(Editor::boot(sample::build_sample(), rs, dir, ui)),
        Choice::ShowMe(step) => replace(welcome_show_me(step, rs, dir, ui).0),
        Choice::Settings => StartOutcome::Request(AppRequest::Settings),
        Choice::Remove(_) | Choice::Back => StartOutcome::Stay,
    }
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
    /// GM21: File ▸ Open the sample cut.
    Sample,
    /// AU6: a Start float choice that puts another document here.
    Start(welcome::WelcomeAction),
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

/// What the Loop latch means, on either transport.
fn loop_card() -> String {
    format!("Loop off: playback stops at the end of the loop range or the {}.", vocab::SHOT)
}

/// A transport key: its name and its card read the chord it has now, so a
/// rebound key never leaves the band out of date (AU5).
fn transport_key(ui: &mut egui::Ui, icon: Icon, word: &str, action: Action) -> egui::Response {
    let name = match menus::chord_label(ui.ctx(), action) {
        Some(chord) => format!("{word} ({chord})"),
        None => word.to_string(),
    };
    menus::hover_tip(plate::band_key_named(ui, icon, &name), action, "")
}

/// A band key's card in 4.5's shape, `Name (chord) — help`, with the band's
/// own word for the name and the chord it has now.
fn chord_card(resp: egui::Response, word: &str, action: Action, what: String) -> egui::Response {
    let ctx = resp.ctx.clone();
    plate::hover_card(resp, || match menus::chord_label(&ctx, action) {
        Some(chord) => format!("{word} ({chord}) — {what}"),
        None => format!("{word} — {what}"),
    })
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
    /// GM11: the viewers' "final look" latch (the Editor's FinalLook).
    final_look: &'a mut bool,
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
    pane_rects: &'a mut Vec<(Pane, egui::Rect, egui::LayerId)>,
    /// AU4: the overlay's share of the Editor. The rail reads the rest.
    floats: &'a mut floats::Floats,
    rail_off: bool,
    full_band: bool,
    layers_docked: bool,
    /// AU3: the current recipe's panes while Lock UI positions is on, which
    /// keep their place; a panel opened by hand keeps its close control.
    locked: &'a [Pane],
    /// AU9: what the Export panel reads from the Editor.
    export_view: export_step::View,
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
        self.pane_rects.push((*tab, ui.max_rect(), ui.layer_id()));
        plate::note_pane_body(ui);
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
            Pane::RenderQueue => render_queue::ui(ui, self.state, &self.export_view),
            Pane::Board => {
                let gesture_safe = !self.canvas.stroke_active();
                board_panel::ui_board(ui, self.state, gesture_safe)
            }
            Pane::Episode => board_panel::ui_episode(ui, self.state),
            Pane::View3D => view3d::ui(ui, self.state),
            Pane::Manga => {
                let gesture_safe = !self.canvas.stroke_active();
                let koma_armed = self.canvas.tool == canvas::CanvasTool::Panel;
                manga_tools::ui(ui, self.state, gesture_safe, koma_armed)
            }
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
                    self.canvas.dbg_layer = ui.layer_id();
                    self.canvas.dbg_preview = true;
                    let vs = self.viewers.entry(u8::MAX).or_default();
                    viewer::ui(ui, self.state, self.graph, vs, self.final_look, true);
                } else {
                    self.canvas.ui(
                        ui,
                        self.state,
                        self.paint.as_deref_mut(),
                        self.graph,
                        self.pen,
                        self.layers_cfg,
                        self.native_pen,
                    );
                    let host = self.canvas.dbg_rect;
                    let args = &mut rail::OverlayArgs {
                        raster_available: self.paint.is_some(),
                        canvas: self.canvas,
                        state: self.state,
                        presets: self.presets,
                        floats: self.floats,
                        manga: self.stage == Some(Stage::Manga),
                        rail_off: self.rail_off,
                        full_band: self.full_band,
                        layers_docked: self.layers_docked,
                        pane_rects: Some(self.pane_rects),
                    };
                    rail::overlay(ui, args, host);
                }
            }
            Pane::Viewer(id) => {
                let vs = self.viewers.entry(*id).or_default();
                viewer::ui(ui, self.state, self.graph, vs, self.final_look, false);
            }
        }
    }

    // Everything except the canvas can be closed (re-add from the "panes"
    // menu); losing the canvas would strand the user. The lock keeps the
    // recipe's panes too.
    fn is_closeable(&self, tab: &Pane) -> bool {
        !matches!(tab, Pane::Canvas) && !self.locked.contains(tab)
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
            plate::text_field(
                ui,
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
        let scroll = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, p) in self.presets.iter().enumerate() {
                    let hotkey = if i < 8 {
                        format!("{} ", i + 1)
                    } else {
                        "· ".into()
                    };
                    // The words stay ink; the pencil's colour rides on a dot.
                    let text = format!("{hotkey}{}  {:.0}px", p.name, p.size_px);
                    let row = ui.horizontal(|ui| {
                        let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
                        if let Some(c) = p.color {
                            plate::identity_dot(ui.painter(), dot.center(), 3.5, plate::chip([c[0], c[1], c[2], 255]));
                        }
                        ui.button(text).on_hover_text("apply preset").clicked()
                    });
                    if row.inner {
                        self.canvas.apply_preset(p);
                        self.state.status = format!("brush: {}", p.name);
                    }
                }
                if self.presets.is_empty() {
                    ui.label(egui::RichText::new("no presets — save one above").color(plate::legend_dim()));
                }
            });
        plate::note_viewport(ui, scroll.inner_rect, [false, true]);
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

    /// The layout a host sends a joiner, as loadable text. AU3: a dock that
    /// cannot be written whole is not sent.
    fn host_layout(&self) -> Option<String> {
        let layout = workspace::Workspace {
            name: "host's layout".into(),
            dock: self.dock.clone(),
            preset: None,
            view: Some(self.canvas.snapshot_view(&self.state)),
        };
        layout.normalised().ok().and_then(|w| serde_json::to_string(&w).ok())
    }

    /// Enter a spine room — the room switcher, the layout probe and the
    /// room verbs all come through here. The layout always swaps; the
    /// tool/view restore is all-or-nothing, so a live stroke keeps the old
    /// tool state (same law as saved workspaces).
    fn enter_room(&mut self, st: Stage) {
        if st == Stage::Export {
            return self.enter_export();
        }
        // AU9: the step Export was entered from is there as it was left: a
        // look at the result costs nothing, whichever way out is taken.
        if self.export_from.as_ref().is_some_and(|(from, _)| *from == Some(st)) {
            return self.leave_export();
        }
        self.dock = self.recipe(st);
        if self.canvas.apply_view(&st.view(), &mut self.state) {
            self.stage = Some(st);
            // LINE GUARD defaults ON in the ink & paint room (shiage
            // charter) and OFF elsewhere.
            self.state.view.line_guard = matches!(st, Stage::InkPaint);
            self.state.status = format!("{}: {}", vocab::STEP, st.name());
            self.export_left();
        }
    }

    /// AU9: the Export step swaps the dock and remembers the way back. It is
    /// the one step that arms nothing: no view is applied, the line guard
    /// stays, and the stroke guard refuses it outright rather than leave a
    /// dock swapped under a live stroke.
    fn enter_export(&mut self) {
        if self.canvas.stroke_active() {
            self.state.refuse(doc::FINISH_STROKE);
            return;
        }
        let dock = self.recipe(Stage::Export);
        if self.stage == Some(Stage::Export) {
            self.dock = dock;
        } else {
            let left = std::mem::replace(&mut self.dock, dock);
            self.export_from = Some((self.stage, left));
        }
        self.stage = Some(Stage::Export);
        self.say.exported = None;
        self.state.status = format!("{}: {}", vocab::STEP, Stage::Export.name());
    }

    /// AU9: where the person is, as the top row names it.
    fn place_here(&self) -> String {
        self.stage.map_or_else(|| self.place.clone(), |st| st.name().to_string())
    }

    /// AU9: the Export panel's reading of the Editor. The way back is worked
    /// out here each frame, so a document swapped under the step cannot take
    /// its name with it.
    fn export_view(&self) -> export_step::View {
        let back = (self.stage == Some(Stage::Export)).then(|| match &self.export_from {
            Some((Some(st), _)) => st.name().to_string(),
            Some((None, _)) => self.place.clone(),
            None => Stage::Drawing.name().to_string(),
        });
        export_step::View { back, more_default: self.more_default, range: self.export_range }
    }

    /// AU9: the step a new Editor carries across a session's join: never
    /// Export, whose way back and whose dock stay with this Editor.
    fn stage_kept(&self) -> Option<Stage> {
        match (self.stage, &self.export_from) {
            (Some(Stage::Export), Some((from, _))) => *from,
            (Some(Stage::Export), None) => Some(Stage::Drawing),
            (stage, _) => stage,
        }
    }

    /// AU9: `Back to Draw`: the step it came from, as it was left. Nothing was
    /// armed on the way in, so nothing is armed on the way out.
    fn leave_export(&mut self) {
        match self.export_from.take() {
            Some((stage, dock)) => {
                self.dock = dock;
                self.stage = stage;
                self.state.status = format!("{}: {}", vocab::STEP, self.place_here());
                self.export_left();
            }
            None => self.enter_room(Stage::Drawing),
        }
    }

    /// AU9: off the Export step: its ring and its card stay behind.
    fn export_left(&mut self) {
        self.export_from = None;
        let panel = &mut self.state.render_queue.panel;
        (panel.ring, panel.done) = (false, None);
    }

    /// AU3: the arrangement `st` opens in. Entering a step, booting and
    /// Reset all read it, so they agree.
    fn recipe(&self, st: Stage) -> DockState<Pane> {
        match st {
            Stage::Drawing if self.sheet_in_draw => Stage::dock_with_sheet(),
            _ => st.dock(),
        }
    }

    /// AU3: a template's project opens in the template's first step, its
    /// Timing sheet on where the template starts it on. That is this
    /// project's; the remembered choice is the button's alone.
    fn enter_template(&mut self, t: &templates::Template) {
        self.sheet_in_draw |= t.sheet_on;
        self.enter_room(t.room);
        // AU6: Timing opens on its Viewer with Select armed: an Animatic's
        // first move is a stroke on the board, so the canvas comes forward
        // with the brush.
        if t.room == Stage::Edit {
            self.open_pane(Pane::Canvas);
            self.canvas.arm_paint(false, &mut self.state);
        }
    }

    /// AU6: an entry of `INTENTS`, entered: Animation alone opens the strip.
    fn enter_intent(&mut self, i: usize) {
        self.enter_template(&templates::INTENTS[i]);
        self.strip_open = i == templates::ANIMATION;
    }

    /// AU6: the Start float, now or at pen-up. It stands on the right, so
    /// Layers makes way for it (spec 4.5).
    pub(crate) fn open_start(&mut self, w: welcome::Welcome) {
        if self.canvas.stroke_active() {
            self.start_waiting = Some(w);
            return;
        }
        self.floats.close(floats::Float::Layers);
        self.start = Some(w);
    }

    /// AU6: what the float chose. A choice that puts another document here
    /// goes through the unsaved guard first, inside the editor, so the Rig
    /// covers it; the others are the shell's, and their own paths ask.
    fn choose_start(&mut self, action: welcome::WelcomeAction, ctx: &egui::Context) {
        use welcome::WelcomeAction as Choice;
        if self.canvas.is_guest && (action.replaces_document() || action == Choice::Open) {
            self.state.refuse("refused — leave the room first (Settings › Session) to open your own file");
        } else if action.replaces_document() {
            self.leave(Leave::Start(action), ctx);
        } else {
            self.requests.push(AppRequest::Start(action));
        }
    }

    /// AU3: the dock is a place off the spine, named `place` on its pill.
    fn go_off_spine(&mut self, place: &str) {
        self.stage = None;
        self.export_left();
        self.place = place.to_string();
        self.place_dock = Some(self.dock.clone());
        self.held_for = None;
    }

    /// AU4: 3D and the Brush maker show no rail and no strip.
    fn rail_off(&self) -> bool {
        self.stage.is_none() && [vocab::LAYOUT_3D, vocab::BRUSH_MAKER].contains(&self.place.as_str())
    }

    /// AU4: the strip may show here. It never shares the screen with the
    /// sheet, which holds the same frames (spec R16).
    fn strip_room(&self) -> bool {
        !self.rail_off() && !visible_panes(&self.dock).contains(&Pane::XSheet)
    }

    /// AU4: the strip's latch is all that keeps it away.
    fn strip_closed(&self) -> bool {
        !self.strip_open && self.strip_room()
    }

    /// AU5: the band's question: the sheet is on screen, so the transport
    /// spreads out and the strip has nothing to add.
    fn sheet_on_screen(&self) -> bool {
        visible_panes(&self.dock).contains(&Pane::XSheet)
    }

    /// AU5: the one way a verb brings the Timing sheet up: Draw's own switch
    /// in Draw, which the room remembers, and the pane anywhere else.
    fn show_sheet(&mut self) {
        if self.stage == Some(Stage::Drawing) && self.dock.find_tab(&Pane::XSheet).is_none() {
            self.switch_sheet(false);
        } else {
            self.open_pane(Pane::XSheet);
        }
    }

    /// AU5: Delete over the strip means the drawing under the pointer, never
    /// a track. The one translation sits here, at chord dispatch and not in
    /// `perform`, so the menu's Remove track still removes a track and a
    /// rebound chord follows it (spec 7.2).
    pub(crate) fn aimed(&mut self, action: Action, ctx: &egui::Context) -> Action {
        let over = ctx.pointer_latest_pos().is_some_and(|p| self.strip.holds(p));
        if action != Action::RemoveColumn || !over {
            return action;
        }
        // The key sits on the run's first frame, whichever frame this is.
        if !self.canvas.stroke_active() {
            match self.strip.under() {
                Some(from) => self.state.goto(from),
                None => self.state.settle_on_run_start(),
            }
        }
        Action::ClearFrameKey
    }

    /// AU4: a drawing canvas stands somewhere this frame, for a float to
    /// open over: not Plan's preview, unless the OS window has it.
    fn canvas_shown(&self) -> bool {
        self.float_canvas.open
            || (self.stage != Some(Stage::Layout) && visible_panes(&self.dock).contains(&Pane::Canvas))
    }

    /// AU4: `Make a brush`, and the arrangement menu's Brush maker row.
    fn open_brush_maker(&mut self) {
        if self.canvas.stroke_active() {
            self.state.refuse("refused — finish the stroke first");
        } else {
            self.dock = Pane::forge_dock();
            // Off-spine like every custom room (saved rooms do the same):
            // otherwise a forge room entered from LAYOUT kept the stage, and
            // the Canvas pane stayed the read-only composite PREVIEW instead
            // of test paper — the owner's report.
            self.go_off_spine(vocab::BRUSH_MAKER);
            self.state.status = "the forge room".into();
        }
    }

    /// AU3: what the off-spine pill says; None on the spine. Manga is a
    /// stage, but it has no pill of its own.
    fn place_name(&self) -> Option<&str> {
        match self.stage {
            Some(Stage::Manga) => Some(vocab::STEP_MANGA),
            Some(_) => None,
            None if self.place.is_empty() => Some(vocab::ARRANGEMENT),
            None => Some(&self.place),
        }
    }

    /// AU3: the panes Lock UI positions holds: the current recipe's, or the
    /// place's off the spine.
    fn locked_panes(&mut self) -> &[Pane] {
        let key = (self.stage, self.sheet_in_draw);
        if self.held_for != Some(key) {
            let dock = self.stage.map(|st| self.recipe(st));
            let dock = dock.as_ref().or(self.place_dock.as_ref());
            self.held = dock.map_or(Vec::new(), |d| d.iter_all_tabs().map(|(_, t)| *t).collect());
            self.held_for = Some(key);
        }
        &self.held
    }

    /// Every Editor is born through this or [`Editor::boot_form`].
    pub fn boot(
        state: AppState,
        rs: Option<&RenderState>,
        dir: Option<&std::path::Path>,
        ui: &config::UiConfig,
    ) -> Editor {
        let mut editor = Editor::from_state(state, rs, dir);
        editor.apply_ui(ui);
        editor
    }

    /// [`Editor::boot`] over a New Project form.
    pub fn boot_form(
        form: &NewProjectForm,
        rs: Option<&RenderState>,
        dir: Option<&std::path::Path>,
        ui: &config::UiConfig,
    ) -> Editor {
        let mut editor = Editor::from_form(form, rs, dir);
        editor.apply_ui(ui);
        editor
    }

    /// Copy the remembered fields in, and rebuild the dock when the recipe
    /// then differs.
    fn apply_ui(&mut self, ui: &config::UiConfig) {
        self.say.feed(ui);
        let changed = self.sheet_in_draw != ui.sheet_in_draw;
        self.sheet_in_draw = ui.sheet_in_draw;
        if changed && let Some(st @ Stage::Drawing) = self.stage {
            self.dock = self.recipe(st);
        }
    }

    /// PSD-cel-layers Stage 4: the current ROOM's stack becomes the new-cel
    /// template (held-stack clone still wins inside doc), and the strip gets
    /// the pickable list. AU1: moved out of the App, so a Rig under a shipped
    /// boot arms exactly what the App arms; `seen` is the caller's own memory
    /// of the room it last armed.
    fn arm_room(&mut self, layers_cfg: &LayersConfig, seen: &mut &'static str) {
        let room_key = arming_key(self.stage, seen);
        let stack = layers_cfg.stack_for_room(room_key);
        // Entering a room arms its stack's working layer (shiage →
        // color, LO → rough), the way a room re-arms its tool. Never
        // under a live pen.
        if *seen != room_key && !self.canvas.stroke_active() {
            *seen = room_key;
            let (n, r) = stack.armed();
            self.state.view.active_layer = n;
            self.state.view.active_role = r;
        }
        self.state.new_cel_active = stack.armed();
        self.state.new_cel_template = stack.layers.clone();
        self.state.stack_current = stack.name.clone();
        self.state.stacks = layers_cfg.all_stacks();
    }

    fn from_form(
        form: &NewProjectForm,
        rs: Option<&RenderState>,
        data_dir: Option<&std::path::Path>,
    ) -> Self {
        // GM7: the form's project is an Srgb one (newproject.rs). GM21: the
        // layers take the project's paper (a page template's is its page's).
        let state = form.new_state();
        let (w, h) = (state.engine.project.width, state.engine.project.height);
        let paint = rs.map(|rs| PaintLayer::new(rs, w, h));
        let graph = rs.map(|rs| graphcomp::GraphCompositor::new(rs, w, h));
        // GM20: the room's side limit — every peer caps and replays alike.
        let max_dim = paint::room_limit(
            rs.map_or(paint::DEFAULT_MAX_DIM, |rs| rs.device.limits().max_texture_dimension_2d),
        );
        let (workspaces, load_notes) = Self::load_workspaces(data_dir);
        Self {
            data_dir: data_dir.map(|d| d.to_path_buf()),
            presets: Vec::new(),
            state,
            canvas: canvas::CanvasView::new().with_layer_limit(max_dim),
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
            sheet_in_draw: false,
            sheet_request: None,
            place: String::new(),
            place_dock: None,
            export_from: None,
            more_default: false,
            held: Vec::new(),
            held_for: None,
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
            final_look: Default::default(),
            export_range: None,
            float_viewer: floatwin::FloatViewer::new(),
            float_canvas: floatcanvas::FloatCanvas::default(),
            audio_out: None,
            audio_tried: false,
            audio_playing: false,
            audio_prev_frame: 0,
            audio_key: None,
            audio_mix: None,
            audio_clock: None,
            play_dropped: 0,
            play_running: false,
            play_whole: false,
            scrub_audio: true,
            scrub_request: None,
            last_snippet: None,
            mix_job: None,
            reload_pending: false,
            lip_form: None,
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
            floats: floats::Floats::default(),
            start: None,
            start_waiting: None,
            strokes_seen: 0,
            say: say::Say::default(),
            show_start: true,
            show_start_request: None,
            shell_asks: false,
            ui_lock: false,
            lock_request: None,
            strip_open: false,
            full_band: false,
            strip: strip::Strip::default(),
            opened_doc: None,
            was_playing: false,
            frame_entry: None,
            #[cfg(test)]
            ignored: Vec::new(),
            #[cfg(test)]
            dbg_graph: viewer::GraphView::Off,
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
        // GM20: the room's side limit — every peer caps and replays alike.
        let max_dim = paint::room_limit(
            rs.map_or(paint::DEFAULT_MAX_DIM, |rs| rs.device.limits().max_texture_dimension_2d),
        );
        let (workspaces, load_notes) = Self::load_workspaces(data_dir);
        Self {
            data_dir: data_dir.map(|d| d.to_path_buf()),
            presets: Vec::new(),
            state,
            canvas: canvas::CanvasView::new().with_layer_limit(max_dim),
            paint,
            graph,
            request_new: false,
            request_update: false,
            update_tag: None,
            request_settings: false,
            request_settings_pen: false,
            dock: Stage::Drawing.dock(),
            stage: Some(Stage::Drawing),
            sheet_in_draw: false,
            sheet_request: None,
            place: String::new(),
            place_dock: None,
            export_from: None,
            more_default: false,
            held: Vec::new(),
            held_for: None,
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
            final_look: Default::default(),
            export_range: None,
            float_viewer: floatwin::FloatViewer::new(),
            float_canvas: floatcanvas::FloatCanvas::default(),
            audio_out: None,
            audio_tried: false,
            audio_playing: false,
            audio_prev_frame: 0,
            audio_key: None,
            audio_mix: None,
            audio_clock: None,
            play_dropped: 0,
            play_running: false,
            play_whole: false,
            scrub_audio: true,
            scrub_request: None,
            last_snippet: None,
            mix_job: None,
            reload_pending: false,
            lip_form: None,
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
            floats: floats::Floats::default(),
            start: None,
            start_waiting: None,
            strokes_seen: 0,
            say: say::Say::default(),
            show_start: true,
            show_start_request: None,
            shell_asks: false,
            ui_lock: false,
            lock_request: None,
            strip_open: false,
            full_band: false,
            strip: strip::Strip::default(),
            opened_doc: None,
            was_playing: false,
            frame_entry: None,
            #[cfg(test)]
            ignored: Vec::new(),
            #[cfg(test)]
            dbg_graph: viewer::GraphView::Off,
            quick_start: [false; 5],
            project_form: None,
        }
    }

    /// GM8: "Show me" on Quick start step `i` (AU7: spec 8.3): do the step.
    /// The export's enters the Export step with its first choice ringed: the
    /// tick is the person's, when their file lands.
    pub(crate) fn show_me(&mut self, i: usize) -> Performed {
        match welcome::STEPS.get(i).map(|s| s.show_me) {
            Some(welcome::ShowMe::Circle) => self.demo_circle(),
            Some(welcome::ShowMe::NextCircle) => self.next_circle(),
            Some(welcome::ShowMe::Perform(a)) => self.perform(a),
            Some(welcome::ShowMe::Play) if self.state.view.playing => Performed::Done,
            Some(welcome::ShowMe::Play) => self.perform(Action::PlayPause),
            Some(welcome::ShowMe::Export) => {
                let entered = self.perform(Action::RoomExport);
                self.state.render_queue.panel.ring = entered == Performed::Done;
                entered
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
            .frame(plate::dialog_frame(&ctx.global_style()))
            .open(&mut open)
            .default_width(340.0)
            .collapsible(false)
            .show(ctx, |ui| {
                plate::surface(ui);
                egui::Grid::new("project_settings").num_columns(2).show(ui, |ui| {
                    ui.label("name");
                    plate::text_field(ui, egui::TextEdit::singleline(&mut form.name));
                    ui.end_row();
                    // The paper is anchored top-left: art keeps its place.
                    const RESIZE_NOTE: &str =
                        "the canvas grows or crops from the top-left; drawings are not scaled";
                    ui.label("resolution").on_hover_text(RESIZE_NOTE);
                    ui.horizontal(|ui| {
                        plate::field(ui, egui::DragValue::new(&mut form.width).range(16..=16384))
                            .on_hover_text(RESIZE_NOTE);
                        ui.label("×");
                        plate::field(ui, egui::DragValue::new(&mut form.height).range(16..=16384))
                            .on_hover_text(RESIZE_NOTE);
                        let g = gcd(form.width, form.height).max(1);
                        ui.label(
                            egui::RichText::new(format!("{}:{}", form.width / g, form.height / g))
                                .color(plate::legend_dim()),
                        );
                    });
                    ui.end_row();
                    ui.label("fps");
                    plate::field(ui, egui::DragValue::new(&mut form.fps).range(1..=240));
                    ui.end_row();
                    ui.label("default cut length");
                    plate::field(ui, egui::DragValue::new(&mut form.cut_frames).range(1..=100_000).suffix(" frames"));
                    ui.end_row();
                    ui.label("dpi");
                    plate::field(ui, egui::DragValue::new(&mut form.dpi).range(1.0..=2400.0));
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
        let max_dim = self.canvas.max_layer_dim;
        let other_cols = self.state.cut().render_order().len().saturating_sub(1);
        let mut open = true;
        let mut set = false;
        egui::Window::new("Paper size")
            .frame(plate::dialog_frame(&ctx.global_style()))
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
                        plate::field(ui, egui::DragValue::new(&mut form.rect.x).range(-max..=max).prefix("x "))
                            .on_hover_text(CORNER);
                        plate::field(ui, egui::DragValue::new(&mut form.rect.y).range(-max..=max).prefix("y "))
                            .on_hover_text(CORNER);
                    });
                    ui.end_row();
                    const SIZE: &str = "the paper's width and height in px (the frame is the default)";
                    ui.label("size").on_hover_text(SIZE);
                    ui.horizontal(|ui| {
                        plate::field(ui, egui::DragValue::new(&mut form.rect.w).range(1..=camera::MAX_PAPER).prefix("w "))
                            .on_hover_text(SIZE);
                        plate::field(ui, egui::DragValue::new(&mut form.rect.h).range(1..=camera::MAX_PAPER).prefix("h "))
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
                // GM20: what this GPU paints of the paper, and what it costs.
                let lr = paint::layer_rect(form.rect, (fw, fh), max_dim);
                if let Some(cap) = paint::cap_text(form.rect, &lr, max_dim) {
                    ui.label(egui::RichText::new(cap).color(plate::AKA));
                }
                if let Some(note) = paint::coverage_text(form.rect, (fw, fh)) {
                    ui.label(egui::RichText::new(note).color(plate::legend_dim()));
                }
                ui.label(
                    egui::RichText::new(paint::memory_text(&lr, other_cols)).color(plate::legend_dim()),
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
        // AU4: the topmost layer answers, so a float speaks for what it
        // covers. A popup over everything falls through to the focus.
        let under = ctx.pointer_latest_pos().and_then(|p| {
            let top = ctx.layer_id_at(p);
            self.pane_rects.iter().find(|(_, r, layer)| r.contains(p) && top == Some(*layer))
        });
        let pane = under
            .map(|(pane, ..)| *pane)
            .or_else(|| self.dock.find_active_focused().map(|(_, t)| *t));
        match pane {
            Some(Pane::Canvas) => config::Ctx::Canvas,
            Some(Pane::XSheet) => config::Ctx::Sheet,
            Some(Pane::NodeGraph) => config::Ctx::NodeGraph,
            Some(Pane::Curves) => config::Ctx::Curves,
            Some(Pane::View3D) => config::Ctx::View3D,
            _ => config::Ctx::Global,
        }
    }

    /// GM3: open a pane, or bring it to the front when it is already
    /// docked. A new one tabs in beside the layer strip (or the sheet);
    /// AU3: on a dock of one leaf it splits off to the right, so the paper
    /// stays in view.
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
            None => self.add_pane(pane),
        }
    }

    /// AU3: a pane with nowhere to tab in: split a dock of one leaf, else
    /// join the focused leaf. The Canvas leaf is never the one: a click on
    /// the paper focuses it, and a tab there would cover the paper.
    fn add_pane(&mut self, pane: Pane) {
        let tree = self.dock.main_surface_mut();
        if tree.iter().filter(|n| n.is_leaf()).count() == 1 {
            tree.split_right(egui_dock::NodeIndex::root(), 0.70, vec![pane]);
            return;
        }
        let free = |n: &egui_dock::Node<Pane>| n.tabs().is_some_and(|t| !t.contains(&Pane::Canvas));
        let leaf = tree
            .focused_leaf()
            .filter(|&i| free(&tree[i]))
            .or_else(|| (0..tree.len()).map(egui_dock::NodeIndex).find(|&i| free(&tree[i])));
        if let Some(leaf) = leaf {
            tree.set_focused_node(leaf);
        }
        tree.push_to_focused_leaf(pane);
    }

    /// AU8: the seam behind the save dialog: what happens to the path a person
    /// chose, so a test can end in a real file with no dialog. AU9: the job
    /// starts where the person stands, with no pane opened.
    pub(crate) fn export_to(
        &mut self,
        ctx: &egui::Context,
        kind: render_queue::ExportKind,
        path: &std::path::Path,
    ) {
        self.start_export(ctx, kind, path, false);
    }

    /// GM11: File ▸ Export…'s PNG sequence (`dest` a folder), or its MP4, GIF
    /// or still (`dest` the file) — a render queue job for this cut over the
    /// menu's frame range (None = the whole cut; a still is the current
    /// frame), the queue brought forward and started (a job already rendering
    /// finishes first).
    pub(crate) fn queue_export(
        &mut self,
        ctx: &egui::Context,
        kind: render_queue::ExportKind,
        dest: &std::path::Path,
    ) {
        self.start_export(ctx, kind, dest, true);
    }

    fn start_export(
        &mut self,
        ctx: &egui::Context,
        kind: render_queue::ExportKind,
        dest: &std::path::Path,
        queue_forward: bool,
    ) {
        let Some(i) = render_queue::export_job(&mut self.state, kind, dest, self.export_range)
        else {
            return;
        };
        if queue_forward {
            self.open_pane(Pane::RenderQueue);
            self.state.render_queue.panel.unfold();
        } else {
            self.state.render_queue.queue.jobs[i].asked = Some(kind);
        }
        render_queue::start(&mut self.state, ctx);
        let job = &self.state.render_queue.queue.jobs[i];
        self.state.status = if queue_forward {
            format!(
                "queued {} ({}) — the render queue renders it while you work",
                job.label,
                job.preset().name
            )
        } else if kind == render_queue::ExportKind::Png {
            format!("Exporting {} pictures…", job.total())
        } else {
            format!("Exporting {}…", dest.file_name().unwrap_or_default().display())
        };
    }

    /// GM3: may this action run now? Err says why not, in the artist's
    /// words — a menu item that cannot run is greyed and says so.
    pub fn can_perform(&self, action: Action) -> Result<(), &'static str> {
        let guest = self.canvas.is_guest;
        let shared = guest || self.canvas.session_live;
        let s = &self.state;
        match action {
            Action::CanvasOptions if !self.canvas_shown() => Err("no canvas here"),
            Action::Undo if !shared && !s.engine.can_undo() => Err("nothing to undo"),
            Action::Redo if !shared && !s.engine.can_redo() => {
                Err("nothing to redo — undo something first")
            }
            Action::Save | Action::SaveAs if guest => Err("the host owns the file"),
            Action::Open | Action::OpenSample if guest => {
                Err("leave the room first to open your own file")
            }
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
            // GM13: the toolbar button's own refusal, said the same way.
            Action::BuildSatsuei if s.satsuei_refusal().is_some() => {
                Err(s.satsuei_refusal().unwrap_or("no drawing column"))
            }
            a if stroke_guarded(a) && self.canvas.stroke_active() => {
                Err("finish the stroke first")
            }
            // GM18: a project that is no book has no page to copy; one
            // export at a time, of a book with a page.
            Action::NewPage if s.new_page_refusal().is_some() => {
                Err(s.new_page_refusal().unwrap_or("this project is not a manga book"))
            }
            Action::ExportPages if s.page_export_refusal().is_some() => {
                Err(s.page_export_refusal().unwrap_or("this book has no page"))
            }
            // AU9: video needs ffmpeg. The search's answer is cached, so a row
            // greyed here asks nothing of the disk.
            Action::ExportMp4 if s.render_queue.ffmpeg_cached() == Some(None) => Err(export_step::NO_FFMPEG),
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
            // GM12: one sound decodes at a time; the loop range and the
            // lip flap need something to act on.
            Action::ImportSound | Action::AddSoundTrack if s.sound_import.is_some() => {
                Err("a sound file is still decoding")
            }
            Action::ClearLoop if s.loop_range().is_none() => Err("this cut has no loop range"),
            Action::LipFlap if anim_core::audio::tracks_iter(s.cut()).next().is_none() => {
                Err("import a dialogue track first")
            }
            // GM15: the editorial's verbs need what they act on.
            Action::MakeCutsFromPanels if s.panel_column().is_none() => {
                Err("open the animatic (a cut with a BOARD column) first")
            }
            Action::ExportEdl
            | Action::ExportColorSheet
            | Action::ExportCutAsProject
            | Action::RenderEpisode
                if s.render_queue.out_dir(s.file_path.as_deref()).is_none() =>
            {
                Err("type a render folder first, or save the project")
            }
            Action::ExportColorSheet if s.palettes.characters.is_empty() => {
                Err("no character palette yet")
            }
            // GM17: View3D's verbs need a model to act on, and the cut
            // camera is perspective.
            Action::SetCutCameraFromView | Action::View3dFrameAll
                if view3d::model_node(s.cut(), s.selected_node).is_none() =>
            {
                Err(view3d::NO_MODEL)
            }
            Action::SetCutCameraFromView if s.view3d.orbit.is_some_and(|o| o.ortho) => {
                Err(view3d::ORTHO_REFUSAL)
            }
            // GM22: the underlay turns on only over a model (off, always).
            Action::Underlay3d
                if !self.canvas.underlay.on && view3d::model_node(s.cut(), s.selected_node).is_none() =>
            {
                Err(view3d::NO_MODEL)
            }
            _ => Ok(()),
        }
    }

    /// GM3: a toggle's state now (its menu item shows it checked); None
    /// for a verb that is not a toggle.
    pub fn is_on(&self, action: Action) -> Option<bool> {
        match action {
            Action::ToggleOnion => Some(self.state.view.onion),
            Action::ToggleCompositeView => Some(self.canvas.composite_view),
            Action::CanvasOptions => Some(self.floats.is_open(floats::Float::CanvasOptions)),
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
            // GM11: the composite viewers' delivery look.
            Action::ToggleFinalLook => Some(self.final_look.on),
            // GM22: the canvas's 3D underlay.
            Action::Underlay3d => Some(self.canvas.underlay.on),
            // GM12: audio scrub, and a RAM preview pass running.
            Action::ToggleScrub => Some(self.scrub_audio),
            Action::RamPreview => Some(self.state.ram.busy()),
            // GM15: the episode playing on through the scene's cuts.
            Action::PlayEpisode => Some(self.state.board.episode_play && self.state.view.playing),
            // GM17: View3D's ortho latch.
            Action::View3dOrtho => Some(self.state.view3d.orbit.is_some_and(|o| o.ortho)),
            // GM18: the pen's ruler snap, and the panel tool armed.
            Action::ToggleRulerSnap => Some(self.state.manga.snap),
            Action::PanelTool => Some(self.canvas.tool == canvas::CanvasTool::Panel),
            Action::ToggleLockPositions => Some(self.ui_lock),
            Action::ToggleHints => Some(!self.say.off),
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
        match r.clone() {
            Performed::App(AppRequest::NewProject) => self.leave(Leave::New, ctx),
            Performed::App(AppRequest::Open) => self.leave(Leave::Open, ctx),
            Performed::App(AppRequest::OpenSample) => self.leave(Leave::Sample, ctx),
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
            // Land any in-flight gesture in the CURRENT project first — its
            // commit must not cross into the opened one.
            Leave::Open => {
                self.canvas.finish_gesture(&mut self.state);
                self.state.open();
            }
            // GM21: the sample is a fresh document; the check below resyncs.
            Leave::Sample => {
                self.canvas.finish_gesture(&mut self.state);
                self.state = sample::build_sample();
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
            // AU6: the shell swaps the editor; a gesture lands here first.
            Leave::Start(action) => {
                self.canvas.finish_gesture(&mut self.state);
                self.requests.push(AppRequest::Start(action));
            }
        }
        if self.state.untitled_group != doc {
            // Another document: the canvas resyncs, and the last autosave
            // belonged to the one before (the running record must not pair
            // them) — so does one still being written. AU6: and a document
            // chosen is the end of the Start float.
            self.canvas.engine_changed();
            self.autosave_job = None;
            self.last_autosave = None;
            self.last_autosave_at = None;
            self.start = None;
            self.start_waiting = None;
            self.say.new_document();
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
        self.satsuei_modal(ctx);
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
        let frame = plate::dialog_frame(&ctx.global_style());
        egui::Modal::new(egui::Id::new("gm8_auto_comp")).frame(frame).show(ctx, |ui| {
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

    /// GM13: "Build satsuei graph" over a graph shaped by hand — the same
    /// DANGER confirm (it replaces every node; undo brings them back).
    fn satsuei_modal(&mut self, ctx: &egui::Context) {
        let Some(n) = self.state.satsuei_confirm else {
            return;
        };
        let mut choice = None;
        let frame = plate::dialog_frame(&ctx.global_style());
        egui::Modal::new(egui::Id::new("gm13_satsuei")).frame(frame).show(ctx, |ui| {
            plate::legend(ui, "build satsuei graph");
            ui.label(format!(
                "Replace this hand-built graph ({n} nodes) with the satsuei chain? Undo brings it back."
            ));
            ui.label(
                egui::RichText::new(
                    "passes per character, diffusion, para, light wrap, grade, grain and vignette",
                )
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
                self.state.satsuei_confirm = None;
                self.state.build_satsuei();
            }
            Some(false) => self.state.satsuei_confirm = None,
            None => {}
        }
    }

    fn convert_modal(&mut self, ctx: &egui::Context) {
        if !self.state.convert_confirm {
            return;
        }
        let mut choice = None;
        let frame = plate::dialog_frame(&ctx.global_style());
        egui::Modal::new(egui::Id::new("gm7_convert_colours")).frame(frame).show(ctx, |ui| {
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
        let frame = plate::dialog_frame(&ctx.global_style());
        egui::Modal::new(egui::Id::new("gm2_save_changes")).frame(frame).show(ctx, |ui| {
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
        // Nothing open and no dialog -> the ready paper (AU6), with the Start
        // float over it unless its "Show at startup" latch is off.
        let route = startup_route(
            self.editor.is_some(),
            self.new_form.is_some(),
            self.welcome.as_ref(),
            &self.config,
        );
        match route {
            Startup::Welcome | Startup::ReadyPaper => {
                let (rs, dir) = (self.render_state.as_ref(), self.data_dir.as_deref());
                let mut paper = open_intent(templates::ANIMATION, rs, dir, &self.config.ui);
                // GM19: a new version's first launch opens on What's New.
                if route == Startup::Welcome && start_over(&mut paper, &mut self.config) {
                    self.config.save();
                }
                self.editor = Some(paper);
            }
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
        // (marked ◆; the save-back below filters them out — session-
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
            let room_key = arming_key(editor.stage, self.stack_room_seen);
            editor.arm_room(&layers_cfg, &mut self.stack_room_seen);
            // GM6: the onion and light table settings live in the config.
            editor.canvas.onion = self.config.onion.sane();
            editor.canvas.light_table = self.config.light_table;
            // GM12: audio scrub is remembered (a flip since the last frame
            // — the latch or the verb's chord — is saved first), and the
            // RAM preview cap is Settings ▸ Performance's.
            if let Some(on) = editor.scrub_request.take() {
                self.config.perf.scrub_audio = on;
                self.config.save();
            }
            editor.scrub_audio = self.config.perf.scrub_audio;
            editor.state.ram.set_cap_mb(self.config.perf.ram_preview_mb);
            editor.show_start = self.config.ui.show_welcome;
            editor.say.feed(&self.config.ui);
            editor.shell_asks = !self.recovery.is_empty() || self.new_form.is_some();
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
            if let Some(on) = editor.scrub_request.take() {
                self.config.perf.scrub_audio = on;
                self.config.save();
            }
            // AU3: the Timing sheet button's choice, remembered.
            if let Some(on) = editor.sheet_request.take() {
                self.config.ui.sheet_in_draw = on;
                self.config.save();
            }
            // AU6: the Start float's latch, and the lock's verb.
            if let Some(on) = editor.show_start_request.take() {
                self.config.ui.show_welcome = on;
                self.config.save();
            }
            if let Some(on) = editor.lock_request.take() {
                self.config.ui.lock_positions = on;
                self.config.save();
            }
            // AU7: Hints, and what the Say line has taught.
            if editor.say.store(&mut self.config.ui) {
                self.config.save();
            }
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
                let action = match &mut self.editor {
                    Some(ed) => ed.aimed(action, &ctx),
                    None => action,
                };
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
        // GM21: the General page shares the render queue's ffmpeg answer
        // (with no editor open, the shell's own queue answers).
        let (raster_toggle, ffmpeg_queue) = match self.editor.as_mut() {
            Some(ed) => (
                ed.paint.is_some().then_some(&mut ed.canvas.raster),
                Some(&mut ed.state.render_queue),
            ),
            None => (None, Some(&mut self.ffmpeg_queue)),
        };
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
            ffmpeg_queue,
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
                        ed.go_off_spine(&name);
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
                        ed.go_off_spine(&w.name);
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
                        .fill(plate::ISLAND)
                        .corner_radius(plate::R_CONTROL)
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
        // GM8: Custom size… from the Welcome screen can go back to it.
        let can_cancel = has_editor || self.welcome.is_some();
        let mut form = self.new_form.take().expect("dialog shown with a form");
        let mut action = None;
        if has_editor {
            egui::Window::new("New Project")
                .frame(plate::dialog_frame(ui.style()))
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
                    plate::dialog_frame(ui.style())
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
                let mut editor = Editor::boot_form(
                    &form,
                    self.render_state.as_ref(),
                    self.data_dir.as_deref(),
                    &self.config.ui,
                );
                // GM8: a template's cut opens in its first room.
                if let Some(t) = form.template() {
                    editor.enter_template(t);
                }
                self.editor = Some(editor);
                // A Welcome screen behind the form has done its job.
                self.welcome = None;
            }
            Some(FormAction::Open) => match AppState::pick_and_open() {
                Some(Ok(state)) => {
                    self.editor = Some(Editor::boot(
                        state,
                        self.render_state.as_ref(),
                        self.data_dir.as_deref(),
                        &self.config.ui,
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
    for room in PROBE_ROOMS {
        for ppp in PROBE_SCALES {
            println!("---- {} · pixels_per_point {ppp} ----", room.name());
            let run = room.probe(ppp);
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

/// GM19: a room the probe walks — a stage (the spine, and the manga room
/// off it), or a built-in arrangement off the spine, entered as its button
/// or verb enters it: the forge's, and the 3D layout room, whose View3D is
/// the surface under the pointer there (it holds still where the other
/// rooms' canvas does).
#[derive(Clone, Copy, Debug)]
enum ProbeRoom {
    Stage(Stage),
    Forge,
    Layout3d,
}

/// Every room: the CLI probe and the ship tests walk these.
const PROBE_ROOMS: [ProbeRoom; 9] = [
    ProbeRoom::Stage(Stage::Layout),
    ProbeRoom::Stage(Stage::Drawing),
    ProbeRoom::Stage(Stage::InkPaint),
    ProbeRoom::Stage(Stage::Composite),
    ProbeRoom::Stage(Stage::Edit),
    ProbeRoom::Stage(Stage::Manga),
    ProbeRoom::Forge,
    ProbeRoom::Layout3d,
    // AU9: last, so the shots of the rooms before it keep their numbers.
    ProbeRoom::Stage(Stage::Export),
];

impl ProbeRoom {
    fn name(self) -> &'static str {
        match self {
            ProbeRoom::Stage(st) => st.name(),
            ProbeRoom::Forge => "forge",
            ProbeRoom::Layout3d => "3D layout (LO)",
        }
    }

    /// Into this room the way its button or verb goes (AU1: the audits walk
    /// the same rooms).
    fn enter(self, editor: &mut Editor) {
        match self {
            ProbeRoom::Stage(st) => editor.enter_room(st),
            ProbeRoom::Forge => {
                editor.dock = Pane::forge_dock();
                editor.go_off_spine(vocab::BRUSH_MAKER);
            }
            ProbeRoom::Layout3d => {
                editor.perform(Action::Layout3dRoom);
            }
        }
    }

    fn probe(self, ppp: f32) -> ProbeRun {
        match self {
            // AU9: this step has no paper: its panel is what must hold still.
            ProbeRoom::Stage(Stage::Export) => {
                probe_run_in(ppp, 100, Pane::RenderQueue, |editor| self.enter(editor))
            }
            ProbeRoom::Stage(st) => probe_run(st, ppp),
            ProbeRoom::Forge => probe_run_in(ppp, 100, Pane::Canvas, |editor| self.enter(editor)),
            ProbeRoom::Layout3d => {
                probe_run_in(ppp, 100, Pane::View3D, |editor| self.enter(editor))
            }
        }
    }
}

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
    probe_run_in(ppp, ui_scale, Pane::Canvas, |editor| editor.enter_room(stage))
}

/// GM19: the pass itself, in the room `enter` sets up, holding `still`
/// still: the canvas by its own rect, any other pane by its tab's.
fn probe_run_in(ppp: f32, ui_scale: u32, still: Pane, enter: impl FnOnce(&mut Editor)) -> ProbeRun {
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
    let mut editor = Editor::boot(state, None, None, &config.ui);
    enter(&mut editor);
    if let Some(tab) = editor.dock.find_tab(&still) {
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
            editor.pane_rects.clear();
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
            let r = if still == Pane::Canvas {
                editor.canvas.dbg_rect
            } else {
                editor
                    .pane_rects
                    .iter()
                    .find(|(p, ..)| *p == still)
                    .map_or(egui::Rect::NOTHING, |(_, r, _)| *r)
            };
            if !(r.is_finite() && r.is_positive()) {
                if label != "settle" {
                    unmeasured += 1;
                }
                let pane = still.title().to_lowercase();
                lines.push(format!("{label:26} UNMEASURED — the {pane} pane recorded no rect"));
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
        let fps = self.state.fps().max(1);
        // GM12: what sounds is the mix of every track, keyed by all of it.
        let key = anim_core::audio::has_sound(self.state.cut()).then(|| {
            (
                self.state.view.scene.0,
                self.state.view.cut.0,
                playback::mix_key(self.state.cut(), fps),
            )
        });
        // Max frames one tick can legitimately advance, with 3x headroom.
        let max_step = ((fps as f32 * dt).ceil() as u32)
            .saturating_mul(3)
            .max(4);
        // GM12: under the audio clock the playhead moves only with the
        // sound — any other frame is a seek or a loop wrap.
        let jumped = self.audio_playing
            && match &self.audio_clock {
                Some((clock, _, _)) => frame != clock.shown,
                None => {
                    frame < self.audio_prev_frame
                        || frame > self.audio_prev_frame.saturating_add(max_step)
                }
            };
        let clip_changed = self.audio_playing && self.audio_key != key;
        let start = playing && key.is_some() && (!self.audio_playing || jumped || clip_changed);
        if start && self.start_audio(frame) {
            self.audio_key = key;
        } else if self.audio_playing && (start || !playing || key.is_none()) {
            // Stopped — or the new mix is still being made: the UI clock
            // steps the playhead until it is in.
            if let Some(out) = &self.audio_out {
                out.sink.stop();
            }
            self.audio_playing = false;
            self.audio_key = None;
            self.audio_clock = None;
        }
        // GM12: scrub — a stopped playhead's move sounds one frame (not
        // the frame a run ended on: it was running last frame).
        if key.is_some() && !self.audio_playing {
            let rate = anim_core::audio::MIX_RATE;
            let prev = self.audio_prev_frame;
            let moving = playing || self.play_running;
            for s in playback::scrub_requests(prev, frame, moving, self.scrub_audio, fps, rate) {
                self.last_snippet = Some(s);
                self.start_snippet(s);
            }
        }
        self.audio_prev_frame = frame;
    }

    /// The output device, opened on first need (once: a missing device is
    /// never retried every frame). False = playback stays silent.
    fn ensure_audio_out(&mut self) -> bool {
        if self.audio_out.is_none() && !self.audio_tried {
            self.audio_tried = true;
            match AudioOut::open() {
                Ok(out) => self.audio_out = Some(out),
                Err(e) => {
                    // Once, not per frame; playback stays silent but works.
                    eprintln!("audio device unavailable: {e}");
                    self.state.status = "no audio device — playing silent".into();
                }
            }
        }
        self.audio_out.is_some()
    }

    /// GM12: the cut's mix for playback — mixed once per key, so a seek or
    /// a scrub restarts from the samples already made. The mixing runs on
    /// a worker (D9): None until it is in (the sound starts then).
    fn playback_mix(&mut self) -> Option<std::sync::Arc<Vec<f32>>> {
        let fps = self.state.fps().max(1);
        let cut = self.state.cut();
        if !anim_core::audio::has_sound(cut) {
            return None;
        }
        let key = playback::mix_key(cut, fps);
        if let Some((k, mix)) = &self.audio_mix
            && *k == key
        {
            return Some(mix.clone());
        }
        let polled = match &self.mix_job {
            Some(job) if job.key == key => job.poll(),
            _ => {
                // (Playback repaints every frame; the job needs no nudge.)
                self.mix_job = Some(playback::MixJob::start(cut, fps, key));
                return None;
            }
        };
        match polled {
            Ok(None) => None,
            Ok(Some(mix)) => {
                self.mix_job = None;
                self.audio_mix = Some((key, mix.clone()));
                Some(mix)
            }
            Err(()) => {
                // The worker died without a mix: play silent rather than
                // asking again every frame.
                self.mix_job = None;
                let len = (cut.frame_count as u64 * anim_core::audio::MIX_RATE as u64
                    / fps as u64) as usize;
                let silent = std::sync::Arc::new(vec![0.0f32; len * 2]);
                self.audio_mix = Some((key, silent.clone()));
                self.state.refuse("refused — the sound mix failed; playing silent");
                Some(silent)
            }
        }
    }

    /// Start the mix sounding from `frame` under the audio clock; false
    /// when there is no device or the mix is not in yet.
    fn start_audio(&mut self, frame: u32) -> bool {
        if !self.ensure_audio_out() {
            return false;
        }
        let Some(mix) = self.playback_mix() else {
            return false;
        };
        let fps = self.state.fps().max(1);
        let rate = anim_core::audio::MIX_RATE;
        // Whole sample-frames, THEN interleave — start is always aligned to
        // a left-channel sample of the stereo mix.
        let start = ((frame as u64 * rate as u64 / fps as u64) as usize * 2).min(mix.len());
        let cursor = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(start));
        let Some(out) = &self.audio_out else {
            return false;
        };
        out.sink.stop();
        out.sink.append(ClipSource {
            end: mix.len(),
            data: mix,
            pos: start,
            cursor: cursor.clone(),
            channels: 2,
            rate,
        });
        out.sink.play();
        self.audio_playing = true;
        self.audio_clock = Some((playback::PlayClock::new(frame, fps, rate), cursor, start));
        true
    }

    /// GM12: sound one scrub snippet of the mix, cutting off the last.
    fn start_snippet(&mut self, s: playback::Snippet) {
        if !self.ensure_audio_out() {
            return;
        }
        let Some(mix) = self.playback_mix() else {
            return;
        };
        let start = (s.start as usize * 2).min(mix.len());
        let end = ((s.start + s.len) as usize * 2).min(mix.len());
        let Some(out) = &self.audio_out else { return };
        out.sink.stop();
        out.sink.append(ClipSource {
            data: mix,
            pos: start,
            end,
            cursor: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(start)),
            channels: 2,
            rate: anim_core::audio::MIX_RATE,
        });
        out.sink.play();
    }

    /// GM12: one UI frame of playback. While the mix sounds, the audio
    /// clock leads: the frame shown is the frame the sound is on, and the
    /// frames the UI was too slow for are passed over and counted. Without
    /// sound the UI clock steps the playhead (`AppState::tick`). Either way
    /// the loop range wraps (or ends) playback, then the sound follows.
    fn play_tick(&mut self, dt: f32) {
        let playing = self.state.view.playing;
        let prev = self.state.view.frame;
        if playing && !self.play_running {
            self.play_dropped = 0;
            // A run started past the loop range plays on to the cut's end
            // (play from the cursor never jumps back).
            self.play_whole = prev > self.state.play_range().1;
        }
        self.play_running = playing;
        // GM15: Play episode plays each cut whole, then on into the next.
        let episode = playing && self.state.board.episode_play;
        if !playing {
            self.state.board.episode_play = false;
        }
        let whole = (0, self.state.frame_count() - 1);
        let range = if self.play_whole || episode {
            whole
        } else {
            self.state.play_range()
        };
        let looping = self.state.view.loop_playback && !episode;
        let reached = match &mut self.audio_clock {
            Some((clock, cursor, start)) if playing && self.audio_playing => {
                let pos = cursor.load(std::sync::atomic::Ordering::Relaxed);
                let before = clock.dropped;
                let f = clock.advance((pos.saturating_sub(*start) / 2) as u64);
                self.play_dropped += clock.dropped - before;
                // The mix ends with the cut: a sound that has run out is
                // past the end even where fps does not divide the rate.
                let ran_out = self.audio_out.as_ref().is_some_and(|o| o.sink.empty());
                Some(if ran_out { f.max(range.1 + 1) } else { f })
            }
            _ => None,
        };
        let step = match reached {
            Some(f) => Some(playback::place(f, range, looping)),
            None => {
                self.state.tick(dt);
                let now = self.state.view.frame;
                if playing && now > prev + 1 {
                    self.play_dropped += (now - prev - 1) as u64;
                }
                // The UI clock knows only the cut's ends: past the loop
                // range's end (or wrapped at the cut's) is the range's end.
                let past = now > range.1
                    || (now < prev && prev <= range.1)
                    || (episode && !self.state.view.playing);
                (playing && (range != whole || self.play_whole || episode) && past)
                    .then(|| playback::place(range.1 + 1, range, looping))
            }
        };
        match step {
            // GM15: the episode runs on into the scene's next cut (its
            // sound restarts with it), passing over a cut with no columns
            // yet (nothing to show); after the last one — or when the next
            // will not open — it stops, never refusing again every frame.
            Some(playback::Step::Wrap(f)) | Some(playback::Step::End(f)) if episode => {
                let at = self.state.at();
                let next = self.state.engine.project.scene(at.scene).and_then(|s| {
                    let i = s.cuts.iter().position(|c| c.id == at.cut)?;
                    s.cuts[i + 1..]
                        .iter()
                        .find(|c| !c.xsheet.columns.is_empty())
                        .map(|c| c.id)
                });
                if let Some(cut) = next {
                    self.state.goto_cut(at.scene, cut);
                }
                if next.is_some() && self.state.at().cut != at.cut {
                    self.state.view.playing = true;
                    self.play_whole = false;
                } else {
                    self.state.view.frame = f;
                    self.state.view.playing = false;
                    self.state.board.episode_play = false;
                }
            }
            // A run that played on past the loop range loops the range
            // once it wraps at the cut's end.
            Some(playback::Step::Wrap(_)) if self.play_whole => {
                self.play_whole = false;
                self.state.view.frame = self.state.play_range().0;
            }
            Some(playback::Step::Show(f)) | Some(playback::Step::Wrap(f)) => {
                self.state.view.frame = f;
            }
            Some(playback::Step::End(f)) => {
                self.state.view.frame = f;
                self.state.view.playing = false;
            }
            None => {}
        }
        self.sync_audio(dt);
    }

    /// THE TRANSPORT ROW (spec 4.4): always the band's last 36, plain with
    /// the sheet off screen and full with it on. The right lane is laid out
    /// FIRST from the row's right edge so a long Say line can never push it
    /// off the screen (the clipping the owner's frame_007 debug outlines
    /// caught).
    fn transport_row(&mut self, ui: &mut egui::Ui) {
        let full = self.sheet_on_screen();
        // Under the fold width the rarer keys wait behind one `…`.
        let fold = ui.ctx().content_rect().width() < steps::FOLD;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.band_right(ui, full, fold);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    self.band_left(ui, full, fold);
                    self.say_line(ui);
                });
            });
        });
    }

    /// The band's right slots, last first. On a blank document the plain
    /// ones wait in reserved slots, and arrive with the second drawing.
    fn band_right(&mut self, ui: &mut egui::Ui, full: bool, fold: bool) {
        ui.add_space(8.0);
        self.sheet_button(ui);
        self.relaunch_key(ui);
        if full {
            if !fold {
                self.sound_keys(ui);
            }
        } else {
            if !fold {
                if self.full_band {
                    self.speed_key(ui);
                    self.ghost_key(ui);
                } else {
                    plate::band_slot(ui, "Speed");
                    plate::band_slot(ui, vocab::GHOST_FRAMES);
                }
            }
            let add = plate::band_action(ui, Icon::Plus, "Add drawing", "Add drawing (band)");
            if menus::hover_tip(add, Action::AddDrawingAfter, "").clicked() {
                self.invoke(Action::AddDrawingAfter, ui.ctx());
            }
        }
        // GM12: frames passed over to keep up with the sound. A clean run
        // says nothing.
        if self.play_dropped > 0 {
            let dropped = format!("dropped {}", self.play_dropped);
            playback::readout(ui, &dropped, 96.0, plate::LEGEND)
                .card("Frames passed over to keep up with the sound.");
        }
    }

    /// The band's left slots: the transport, and the frame.
    fn band_left(&mut self, ui: &mut egui::Ui, full: bool, fold: bool) {
        let gesture = self.canvas.stroke_active();
        let arrived = full || self.full_band;
        if full && !fold {
            self.first_frame_key(ui);
        }
        if !arrived {
            plate::band_key_slot(ui);
        } else if transport_key(ui, Icon::StepBack, "previous frame", Action::PrevFrame).clicked()
            && !gesture
        {
            self.state.step(-1);
        }
        let (pi, word) = if self.state.view.playing {
            (Icon::Pause, "stop")
        } else {
            (Icon::Play, "play")
        };
        if transport_key(ui, pi, word, Action::PlayPause).clicked() {
            self.invoke(Action::PlayPause, ui.ctx());
        }
        if !arrived {
            plate::band_key_slot(ui);
        } else if transport_key(ui, Icon::StepFwd, "next frame", Action::NextFrame).clicked()
            && !gesture
        {
            self.state.step(1);
        }
        if full && !fold {
            self.last_frame_key(ui);
        }
        if self.frame_entry.is_some() {
            self.frame_entry_ui(ui);
        } else if full {
            self.frame_field(ui);
        } else {
            self.counter(ui);
        }
        if full {
            self.loop_latch(ui);
        }
        if fold {
            if arrived {
                self.fold_menu(ui, full);
            } else {
                plate::band_key_slot(ui);
            }
        } else if full {
            self.loop_keys(ui);
        }
    }

    /// The `…` under the fold width: the rarer half of either transport.
    fn fold_menu(&mut self, ui: &mut egui::Ui, full: bool) {
        let card = if full {
            "First and last frame, the loop range and the sound."
        } else {
            "Ghost frames and speed."
        };
        let more = ui.menu_button("…", |ui| {
            if full {
                ui.horizontal(|ui| {
                    self.first_frame_key(ui);
                    self.last_frame_key(ui);
                });
                ui.horizontal(|ui| self.loop_keys(ui));
                ui.horizontal(|ui| self.sound_keys(ui));
            } else {
                self.ghost_key(ui);
                // The rows stand here: a popover opened from inside this one
                // would shut both on its first click.
                plate::legend(ui, "Speed");
                self.speed_rows(ui);
            }
        });
        if more.inner.is_none() {
            more.response.card(card);
        }
    }

    /// The cut's facts, as the counter and the frame field carry them.
    fn cut_facts(&self) -> String {
        let project = &self.state.engine.project;
        format!(
            "{}, {} frames, {} fps, {}×{}",
            self.state.cut().name,
            self.state.frame_count(),
            self.state.fps(),
            project.width,
            project.height,
        )
    }

    /// The plain transport's counter: a tap types a frame (`GoToFrame`), and
    /// its width holds while the frame rolls over (D7).
    fn counter(&mut self, ui: &mut egui::Ui) {
        let count = self.state.frame_count();
        let words = format!("Frame {} of {count}", self.state.view.frame + 1);
        let key = plate::counter_key(ui, &words, &format!("Frame {count} of {count}"));
        if menus::hover_tip(key, Action::GoToFrame, &self.cut_facts()).clicked() {
            self.invoke(Action::GoToFrame, ui.ctx());
        }
    }

    /// The full transport's frame field. The one number an animator most
    /// wants to type into stops being the one number that cannot be typed
    /// into.
    fn frame_field(&mut self, ui: &mut egui::Ui) {
        let count = self.state.frame_count();
        let mut f1 = self.state.view.frame + 1;
        // The cut's facts ride the counter's card (AU3).
        let facts = format!("Current frame: type or drag.\n{}", self.cut_facts());
        if plate::field(ui, egui::DragValue::new(&mut f1).range(1..=count))
            .card(facts)
            .changed()
            && !self.canvas.stroke_active()
        {
            self.state.goto(f1 - 1);
        }
        ui.label(
            egui::RichText::new(format!("/ {count}"))
                .monospace()
                .color(plate::LEGEND),
        );
    }

    /// `GoToFrame`'s field, in the counter's place: Enter goes there, and
    /// anything else leaves the frame where it was.
    fn frame_entry_ui(&mut self, ui: &mut egui::Ui) {
        let Some(text) = &mut self.frame_entry else {
            return;
        };
        let field = egui::TextEdit::singleline(text).id_salt("frame_entry").desired_width(72.0);
        let resp = plate::field(ui, field);
        plate::speak(&resp, egui::WidgetType::TextEdit, None, "frame number");
        let gone = resp.lost_focus();
        // The Enter that ends the number is spent here: over the strip the
        // chord dispatch after the panels would read it as `TypeIntoCell`.
        let typed = gone && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        if !gone {
            if !resp.has_focus() {
                resp.request_focus();
            }
            return;
        }
        let words = self.frame_entry.take().unwrap_or_default();
        if !typed || self.canvas.stroke_active() {
            return;
        }
        match words.trim().parse::<u32>() {
            Ok(frame) if frame >= 1 => self.state.goto(frame - 1),
            _ => self.state.refuse("Type a frame number."),
        }
    }

    /// Ghost frames, on the plain transport's right.
    fn ghost_key(&mut self, ui: &mut egui::Ui) {
        let mut on = self.state.view.onion;
        let name = "Ghost frames toggle";
        let key = plate::band_latch(ui, &mut on, Icon::Ghosts, vocab::GHOST_FRAMES, name);
        if menus::hover_tip(key, Action::ToggleOnion, "").changed() {
            self.invoke(Action::ToggleOnion, ui.ctx());
        }
    }

    /// Speed: its popover sets every drawing's hold at once, and the loop.
    fn speed_key(&mut self, ui: &mut egui::Ui) {
        let key = plate::band_action(ui, Icon::Speed, "Speed", "Speed");
        let popup = egui::Popup::menu(&key);
        if popup.show(|ui| self.speed_rows(ui)).is_none() {
            key.card(format!("How long each drawing holds, and whether the {} loops.", vocab::SHOT));
        }
    }

    /// One of three, and the fact on each row. Loop stands apart: it is on
    /// or off whatever the speed.
    fn speed_rows(&mut self, ui: &mut egui::Ui) {
        let now = strip::speed_of(&self.state);
        for (word, step) in [("Fast", 1), ("Medium", 2), ("Slow", 3)] {
            let fact = if step == 1 { "1 frame each".to_string() } else { format!("{step} frames each") };
            if plate::detent_fact(ui, now == Some(step), word, &fact, "Medium").clicked() {
                if self.canvas.stroke_active() {
                    self.state.refuse(doc::FINISH_STROKE);
                } else {
                    self.state.set_speed(step);
                }
            }
        }
        ui.separator();
        let mut lp = self.state.view.loop_playback;
        plate::latch(ui, &mut lp, vocab::LOOP).card(loop_card());
        self.state.view.loop_playback = lp;
    }

    /// PSD-shipping: UPDATE READY — the published build moved past this one.
    /// Tally lamp; the click IS the relaunch (session carried like a dev-loop
    /// restart).
    fn relaunch_key(&mut self, ui: &mut egui::Ui) {
        if let Some(tag) = self.update_tag.clone() {
            ui.separator();
            if ui
                .button(
                    egui::RichText::new(format!("relaunch to update ({tag})"))
                        .size(10.5)
                        .color(plate::STRUCK),
                )
                .card("Download the published build and relaunch. Your session carries over.")
                .clicked()
            {
                self.request_update = true;
            }
            let (dot, _) =
                ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.5, plate::TALLY);
        }
    }

    /// THE SAY LINE (spec 8.2): one reserved lane and one voice, a refusal
    /// first, the machine's last remark after it, and what `say.rs` picked
    /// third, its action button straight after its words.
    fn say_line(&mut self, ui: &mut egui::Ui) {
        // REFUSAL: the machine overruled the hand. Dwells 4s; a repeat of
        // the same refusal re-flashes.
        if self.state.refusal_seq != self.refusal_seen_seq {
            self.refusal_seen_seq = self.state.refusal_seq;
            self.refusal_since = ui.input(|i| i.time);
        }
        // A NOTICE (a reset config, a lost tablet) waits for the artist: the
        // dwell starts at the first key or click after it shows, so it is
        // never missed. (Raw events: a widget that consumed the key still
        // saw the artist act.)
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
        // CHATTER: the machine's last remark, decaying — a stale "painted"
        // must never train the eye to ignore this. A step down to the
        // quietest ink, not a fade: text is opaque, so it never falls under
        // its floor on the way out.
        if self.state.status != self.status_seen {
            self.status_seen = self.state.status.clone();
            self.status_since = ui.input(|i| i.time);
        }
        let now = ui.input(|i| i.time);
        let (rage, age) = (now - self.refusal_since, now - self.status_since);
        let refusing = rage < 4.0 && !self.state.refusal.is_empty();
        let saying = age < 4.0 && !self.state.status.is_empty();
        // Whatever waits behind a dwell comes up when it ends, with no event.
        if refusing || saying {
            let dwell = if refusing { 4.0 - rage } else { 4.0 - age };
            ui.ctx().request_repaint_after_secs(dwell as f32);
        }
        let hint = if refusing || saying { None } else { self.say.hint.clone() };
        // The rule stands only beside words, and its room stays either way.
        ui.spacing_mut().item_spacing.x = 4.0;
        let (rule, _) = ui.allocate_exact_size(egui::vec2(6.0, plate::CHROME_H), egui::Sense::hover());
        if refusing || saying || hint.is_some() {
            let ink = ui.visuals().widgets.noninteractive.bg_stroke;
            ui.painter().vline(rule.center().x, rule.y_range(), ink);
        }
        // The action button's room stays free for it (spec 8.2). Where no
        // dots stand a hint gets back what its own button leaves, and words
        // with no button beside them have the lane: a refusal outranks every
        // hint, and its longest words need all of it at 832. The dots keep
        // the lane's right end, past the longest words and their button, and
        // a Picture start's closed strip has no animation for them to count.
        let lane = ui.available_rect_before_wrap();
        let verb = hint.as_ref().and_then(|h| h.verb);
        let fold = ui.ctx().content_rect().width() < steps::FOLD;
        let animating = !self.strip_closed() || self.full_band;
        let dots = !fold && !refusing && animating && lane.width() >= plate::SAY_DOTS_LANE;
        let spare = match verb {
            _ if dots => plate::SAY_ACTION + plate::SAY_DOTS,
            Some(verb) => plate::say_action_width(ui, verb.label()) + 6.0,
            None if refusing || hint.is_some() => 0.0,
            None => plate::SAY_ACTION,
        };
        let words = egui::vec2((lane.width() - spare).max(0.0), plate::CHROME_H);
        let mut words_end = lane.left();
        ui.allocate_ui_with_layout(words, egui::Layout::left_to_right(egui::Align::Center), |ui| {
            if refusing {
                plate::refusal(ui, &self.state.refusal);
            } else if saying {
                let ink = if age < 3.0 { plate::LEGEND } else { plate::INK3 };
                plate::say(ui, &self.state.status, ink);
            } else if let Some(hint) = &hint {
                words_end = plate::say(ui, &hint.words, plate::LEGEND).rect.right();
            }
        });
        self.say.stands(hint.as_ref().and_then(|h| h.bit), now);
        let row = egui::Rect::from_min_max(egui::pos2(lane.left(), rule.top()), egui::pos2(lane.right(), rule.bottom()));
        // Spec 8.3: the place, in dots. Painted: no control, no words, and a
        // card for whoever rests on them.
        self.say.dots_painted = dots && !self.say.off && self.quick_start.iter().any(|done| !done);
        if self.say.dots_painted {
            let ticks = self.quick_start;
            plate::hover_card(plate::say_dots(ui, row, &ticks), || say::place_words(&ticks));
        }
        if let Some(verb) = verb {
            let beside = row.with_max_x(words_end + 6.0 + plate::say_action_width(ui, verb.label()));
            if plate::say_action(ui, beside, verb.label()).card(verb.card()).clicked() {
                self.say_act(verb, ui.ctx());
            }
        }
    }

    /// AU3: the Timing sheet button: one click, remembered in Draw. Absent
    /// in Plan and Timing, whose recipes hold the sheet; everywhere else a
    /// plain action that opens the pane, and never lit, because a click
    /// there cannot hide it.
    fn sheet_button(&mut self, ui: &mut egui::Ui) {
        if matches!(self.stage, Some(Stage::Layout | Stage::Edit)) {
            return;
        }
        let draw = self.stage == Some(Stage::Drawing);
        let shown = self.dock.find_tab(&Pane::XSheet).is_some();
        let word = vocab::SHEET_BUTTON;
        let key = if draw {
            let mut on = shown;
            let name = if shown { vocab::HIDE_TIMING_SHEET } else { vocab::SHOW_TIMING_SHEET };
            plate::band_latch(ui, &mut on, Icon::Sheet, word, name)
        } else {
            plate::band_action(ui, Icon::Sheet, word, vocab::SHOW_TIMING_SHEET)
        };
        let key = key.card(if draw && shown {
            format!("Hides the {word}.")
        } else {
            format!("Shows the {word} for exact frame work.")
        });
        if key.clicked() {
            if self.canvas.stroke_active() {
                self.state.refuse(doc::FINISH_STROKE);
            } else if draw {
                self.switch_sheet(shown);
            } else {
                self.open_pane(Pane::XSheet);
            }
        }
    }

    /// AU3: Draw with the sheet or without it, remembered. A dock that is
    /// neither recipe was arranged by hand, so only the sheet's tab comes or
    /// goes and the rest of the arrangement stays.
    fn switch_sheet(&mut self, shown: bool) {
        self.sheet_in_draw = !shown;
        self.sheet_request = Some(!shown);
        if same_shape(&self.dock, &Stage::Drawing.dock())
            || same_shape(&self.dock, &Stage::dock_with_sheet())
        {
            self.dock = self.recipe(Stage::Drawing);
        } else if let Some(tab) = self.dock.find_tab(&Pane::XSheet) {
            self.dock.remove_tab(tab);
        } else {
            self.open_pane(Pane::XSheet);
        }
    }

    /// The transport's first key: Home.
    fn first_frame_key(&mut self, ui: &mut egui::Ui) {
        if transport_key(ui, Icon::SkipBack, "first frame", Action::FirstFrame).clicked()
            && !self.canvas.stroke_active()
        {
            self.state.goto(0);
        }
    }

    /// The transport's last key: End.
    fn last_frame_key(&mut self, ui: &mut egui::Ui) {
        if transport_key(ui, Icon::SkipFwd, "last frame", Action::LastFrame).clicked()
            && !self.canvas.stroke_active()
        {
            let last = self.state.frame_count() - 1;
            self.state.goto(last);
        }
    }

    /// Loop, on the full transport's row at any width.
    fn loop_latch(&mut self, ui: &mut egui::Ui) {
        let mut lp = self.state.view.loop_playback;
        plate::latch(ui, &mut lp, vocab::LOOP).card(loop_card());
        self.state.view.loop_playback = lp;
    }

    /// The loop range, on the row or in the `…` when the window is narrow.
    fn loop_keys(&mut self, ui: &mut egui::Ui) {
        // GM12: the loop range: the two keys mark the playhead and × plays
        // the whole shot again. Every slot keeps its width whatever it
        // reads (D7).
        let shot = vocab::SHOT;
        let start = playback::transport_key(ui, vocab::LOOP_START, vocab::LOOP_START, 66.0);
        let said = "Playback starts on this frame.".to_string();
        if chord_card(start, vocab::LOOP_START, Action::LoopIn, said).clicked() {
            self.invoke(Action::LoopIn, ui.ctx());
        }
        let end = playback::transport_key(ui, vocab::LOOP_END, vocab::LOOP_END, 60.0);
        let said = "Playback ends on this frame.".to_string();
        if chord_card(end, vocab::LOOP_END, Action::LoopOut, said).clicked() {
            self.invoke(Action::LoopOut, ui.ctx());
        }
        let range = self.state.loop_range();
        let text = match range {
            Some((a, b)) => format!("{}–{}", a + 1, b + 1),
            None => "—".to_string(),
        };
        playback::readout(ui, &text, 64.0, plate::LEGEND)
            .card(format!("The loop range, first frame to last. A dash means the whole {shot} plays."));
        let clear = ui.add_enabled_ui(range.is_some(), |ui| {
            let key = playback::transport_key(ui, "×", "clear loop range", 22.0);
            if range.is_some() {
                let said = format!("The whole {shot} plays again.");
                chord_card(key, "Clear the loop range", Action::ClearLoop, said)
            } else {
                key.card(format!("This {shot} has no loop range to clear."))
            }
        });
        if clear.inner.clicked() {
            self.invoke(Action::ClearLoop, ui.ctx());
        }
    }

    /// The sound and the rate. Right to left on the row, left to right in
    /// the `…`: the slots read the same. Just over the fold the words would
    /// take the Say line under its floor (spec 4.4): there the key is its
    /// icon, and the name and the card keep them.
    fn sound_keys(&mut self, ui: &mut egui::Ui) {
        let (fps, shot, word) = (self.state.fps(), vocab::SHOT, vocab::SOUND_WHILE_DRAGGING);
        playback::readout(ui, &format!("{fps} {}", vocab::FPS), 48.0, plate::LEGEND)
            .card(format!("This {shot} plays {fps} frames each second."));
        let mut scrub = self.scrub_audio;
        let key = if (steps::FOLD..steps::SOUND_WORDS).contains(&ui.ctx().content_rect().width()) {
            plate::band_latch(ui, &mut scrub, Icon::Sound, "", word)
        } else {
            plate::latch(ui, &mut scrub, word)
        };
        let said = "Dragging or stepping a stopped playhead plays that frame's sound.".to_string();
        if chord_card(key, word, Action::ToggleScrub, said).changed() {
            self.scrub_audio = scrub;
            self.scrub_request = Some(scrub);
        }
    }

    /// AU6: a project file dropped on the window opens, past the unsaved
    /// guard; anything else is said no to. The first project wins. A question
    /// on screen is answered first: a drop never takes its place.
    fn open_dropped(&mut self, ctx: &egui::Context) {
        let paths: Vec<_> =
            ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if paths.is_empty() {
            return;
        }
        let is_project = |p: &&std::path::PathBuf| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("animproj"))
        };
        let asking = self.shell_asks || self.leave.is_some() || self.leave_after_save.is_some();
        match paths.iter().find(is_project) {
            Some(_) if self.canvas.is_guest => {
                self.state.refuse("refused — leave the room first (Settings › Session) to open your own file");
            }
            Some(_) if asking => self.state.refuse("refused — answer the question on screen first"),
            Some(path) => self.leave(Leave::OpenPath(path.clone()), ctx),
            None => self.state.refuse("refused — only a project file (.animproj) opens here"),
        }
    }

    /// AU6: Esc closes the Start float, when nothing nearer has the key: a
    /// modal, a menu, or a field that keeps it. Under a stroke it waits for
    /// the stroke, whose end closes it.
    fn start_esc(&mut self, ctx: &egui::Context) {
        if self.start.is_none()
            || self.palette.open
            || self.canvas.stroke_active()
            || ctx.memory(|m| m.top_modal_layer()).is_some()
            || egui::Popup::is_any_open(ctx)
            || ctx.egui_wants_keyboard_input()
        {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.start = None;
        }
    }

    /// AU6: a line drawn for `Check your pen` was the check's, not the
    /// paper's: it comes off through Undo's own route, so the document is as
    /// it stood and a card after the check has nothing to ask about.
    fn lift_check_line(&mut self) {
        if !self.canvas.stroke_active() {
            self.perform_into(Action::Undo, &mut Performed::Done);
        }
    }

    /// AU6: the Start float over the pane the dock just drew (spec 8.1), and
    /// what ends it: a finished stroke, the Layers float opening, its `×`.
    fn start_ui(&mut self, ctx: &egui::Context, dock: egui::Rect) {
        let stroked = self.state.strokes_done != self.strokes_seen;
        self.strokes_seen = self.state.strokes_done;
        // A finished stroke accepts the paper under it, and stays on it. One
        // that `Check your pen` is listening for is that check's line instead.
        if stroked && let Some(w) = &mut self.start {
            match w.pen.read(self.canvas.pen_verdict()) {
                Some(pen) => {
                    w.pen_said = pen != w.pen;
                    w.pen = pen;
                    self.lift_check_line();
                    self.say.check_line = true;
                }
                None => self.start = None,
            }
        }
        if !self.canvas.stroke_active()
            && let Some(w) = self.start_waiting.take()
        {
            self.open_start(w);
        }
        if self.floats.is_open(floats::Float::Layers) {
            self.start = None;
        }
        let Some(mut w) = self.start.take() else { return };
        let on_canvas = visible_panes(&self.dock).contains(&Pane::Canvas) && !self.float_canvas.open;
        let host = if on_canvas { self.canvas.dbg_rect } else { dock };
        let (pos, size) = floats::start_place(host);
        let page = w.tab as usize;
        let head = floats::Head { words: welcome::HEADING, role: plate::Role::Title, start: true, page };
        let mut show = self.show_start;
        let mut action = None;
        let (shown, close) = floats::frame(ctx, head, pos, size, host, |ui| {
            action = welcome::float_body(ui, &mut w, &mut show);
        });
        // Over the canvas it is the canvas's for the keys. With no canvas on
        // screen it claims no pane, and the focused pane answers them.
        if on_canvas {
            self.pane_rects.push((Pane::Canvas, shown.rect, shown.layer_id));
        }
        if show != self.show_start {
            self.show_start = show;
            self.show_start_request = Some(show);
        }
        if !close {
            self.start = Some(w);
        }
        if let Some(action) = action {
            self.choose_start(action, ctx);
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
        // AU5: from here, so the strip's entry in the band survives the dock.
        self.pane_rects.clear();
        self.strip.begin();
        self.ui_lock = ui_lock;
        // GM2: never lose work — close guard, save modal, title, autosave.
        self.worksafe_frame(ui.ctx());
        self.open_dropped(ui.ctx());
        // AU5: a document opened. The strip opens with it when it holds more
        // than one frame, and its scale is worked out afresh. A frame added
        // later opens nothing: that refit would land under a stroke.
        if self.opened_doc.as_deref() != Some(self.state.untitled_group.as_str()) {
            self.opened_doc = Some(self.state.untitled_group.clone());
            self.strip_open |= self.state.frame_count() > 1;
            self.strip.opened();
        }
        // The band's keys and the rail's Flip arrive on one frame.
        self.full_band |= self.state.cut().drawings.len() > 1;
        // AU5: the hold trap (R19): with the sheet off screen nothing shows a
        // held frame's number, so a stop lands on the run's first frame.
        let playing = self.state.view.playing;
        if self.was_playing && !playing && !self.sheet_on_screen() && !self.canvas.stroke_active() {
            self.state.settle_after_stop();
        }
        self.was_playing = playing;
        // GM7 CHIP PICK: the active chip lasts while the brush holds its
        // colour (a wheel, preset or key that moves the brush clears it).
        self.state.sync_active_chip(self.canvas.brush_color);
        // GM13: the Palette map nodes follow the palettes — one undo step
        // once an edit ends (the pointer is up); and the inspector's .cube
        // picker is the shell's (a window, only on a real click).
        let down = ui.input(|i| i.pointer.any_down());
        self.state.follow_palette_variants(down);
        if let Some((node, key)) = self.state.graph_ui.lut_pick.take() {
            self.requests.push(AppRequest::LoadLut { node, key });
        }
        if let Some(node) = self.state.graph_ui.sequence_pick.take() {
            self.requests.push(AppRequest::PickSequenceFrame { node });
        }
        // GM3: the chords every tooltip names; the palette (its keys taken
        // before anything else reads them) and the help windows.
        menus::publish(ui.ctx(), binds);
        self.palette_ui(ui.ctx(), binds);
        self.help_windows(ui.ctx(), binds);
        self.start_esc(ui.ctx());
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
        // GM12: a sound file decoded on its worker lands as one edit, and
        // RAM preview keeps its frames true to the document (an edit drops
        // just the frames it changed) before playback reads them.
        self.state.poll_sound_import();
        // GM18: and the pages written on theirs say so.
        self.state.poll_page_export();
        let p = &self.state.engine.project;
        let look = playback::RamLook {
            at: self.state.at(),
            size: (p.width, p.height),
            law: p.color_law,
            final_look: self.final_look.on,
        };
        let range = self.state.play_range();
        // GM22: the evaluator stats sequence files from the project's
        // folder; returning to the window reads them again (a render done
        // meanwhile shows), never mid-stroke.
        self.state.sync_eval_base();
        // A return during a stroke reads them when the pen lifts.
        if ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true)))) {
            self.reload_pending = true;
        }
        if self.reload_pending && !self.canvas.stroke_active() {
            self.reload_pending = false;
            if self.state.reload_sequences() > 0 {
                self.final_look.reread();
            }
        }
        let s = &mut self.state;
        let dir = s.project_dir();
        s.ram.set_base_dir(dir);
        match s.ram.sync(&mut s.engine, look, range, Some(ui.ctx())) {
            Some(Ok(msg)) => s.status = msg,
            Some(Err(msg)) => s.refuse(msg),
            None => {}
        }
        let dt = ui.ctx().input(|i| i.stable_dt).min(0.1);
        self.play_tick(dt);
        let row_frame = plate::desk_frame(ui.style()).inner_margin(egui::Margin::symmetric(8, 6));
        egui::Panel::top("top_bar").frame(row_frame).exact_size(44.0).show_separator_line(false).show(ui, |ui| {
            // Owner's directive (2026-08-17): the bar read small. The Chrome
            // role, scoped to this panel only, with wider breathing room:
            // the bar is the one surface read from arm's length.
            {
                use egui::TextStyle;
                let st = ui.style_mut();
                st.text_styles.insert(TextStyle::Body, plate::Role::Chrome.font());
                st.text_styles.insert(TextStyle::Button, plate::Role::Chrome.font());
                st.spacing.icon_width = 18.0;
                st.spacing.button_padding.x = 6.0;
                plate::bare_words(st);
            }
            // THE TOP ROW (AU3, spec 4.2): the menus, the steps, the actions.
            let row = ui.max_rect();
            let window = ui.ctx().content_rect().width();
            let place = self.place_name().map(str::to_string);
            let narrow = steps::narrow(window, place.is_some());
            let file_menu_was_open = std::mem::take(&mut self.file_menu_open);
            let mut menus = |ui: &mut egui::Ui| {
                // File commands live in ONE detent.
                plate::menu_word(ui, "file", |ui| {
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
                    self.menu_item(ui, Action::OpenSample);
                    self.menu_item(ui, Action::Open);
                    ui.menu_button("Open Recent", |ui| {
                        if self.recent_shown.is_empty() {
                            ui.label(egui::RichText::new("Nothing yet").color(plate::legend_dim()));
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
                        // AU9: the plain exports: a dialog each, and the job
                        // runs where the person stands. Other formats opens
                        // the Export step. The rows below stay as they were.
                        self.state.render_queue.search_ffmpeg();
                        self.menu_item(ui, Action::ExportGif);
                        self.menu_item(ui, Action::ExportMp4);
                        self.menu_item(ui, Action::ExportPngSequence);
                        self.menu_item(ui, Action::ExportStill);
                        if ui.button("Other formats…").on_hover_text(Action::RoomExport.help()).clicked() {
                            ui.close();
                            self.invoke(Action::RoomExport, ui.ctx());
                            self.state.render_queue.panel.unfold();
                        }
                        ui.separator();
                        // GM11: every export is a render queue job, rendered
                        // on its worker while the artist keeps drawing; the
                        // queue itself has every preset.
                        if ui
                            .button("open the render queue (all presets)")
                            .on_hover_text(
                                "Window ▸ Render Queue: PNG16, TIFF16, EXR, ProRes, the line \
                                 test, burn-ins and a slate",
                            )
                            .clicked()
                        {
                            ui.close();
                            self.perform(Action::ShowRenderQueue);
                        }
                        ui.separator();
                        // Frame range (C3): 1-based in the UI (matches the
                        // "N / total" frame readout elsewhere), 0-based
                        // internally. None = whole cut (read when the job
                        // starts) until a field is moved, so it can never
                        // point past a cut that's since gotten shorter.
                        let n = self.state.frame_count();
                        let (mut a, mut b) = render_queue::resolve(n, self.export_range);
                        let shown = (a, b);
                        ui.horizontal(|ui| {
                            ui.label("frames");
                            let mut a1 = a + 1;
                            let mut b1 = b + 1;
                            if plate::field(ui, egui::DragValue::new(&mut a1).range(1..=b1)).changed() {
                                a = a1 - 1;
                            }
                            ui.label("–");
                            if plate::field(ui, egui::DragValue::new(&mut b1).range(a1..=n)).changed() {
                                b = b1 - 1;
                            }
                        });
                        if (a, b) != shown {
                            self.export_range = Some((a, b));
                        }
                        if ui
                            .small_button("whole cut")
                            .on_hover_text("reset the range to frame 1 – last")
                            .clicked()
                        {
                            self.export_range = None;
                        }
                        ui.separator();

                        // GM11: each picks where, then queues its job over
                        // this range and starts the queue (queue_export).
                        if ui
                            .button("PNG sequence…")
                            .on_hover_text(
                                "a render queue job (PNG 8-bit): one PNG per frame, transparent \
                                 background — it starts at once",
                            )
                            .clicked()
                        {
                            ui.close();
                            let mut dlg = rfd::FileDialog::new();
                            if let Some(dir) = export::suggest_dir(&self.state) {
                                dlg = dlg.set_directory(dir);
                            }
                            if let Some(dir) = dlg.pick_folder() {
                                let ctx = ui.ctx().clone();
                                self.queue_export(&ctx, render_queue::ExportKind::Png, &dir);
                            }
                        }
                        let ffmpeg = self.state.render_queue.ffmpeg_path().is_some();
                        let mp4 = ui
                            .add_enabled(ffmpeg, egui::Button::new("MP4 video…"))
                            .on_hover_text(
                                "a render queue job (H.264 review): white background — it starts \
                                 at once",
                            )
                            .on_disabled_hover_text(render_queue::NEEDS_FFMPEG);
                        if mp4.clicked() {
                            ui.close();
                            let mut dlg = rfd::FileDialog::new()
                                .add_filter("MP4 video", &["mp4"])
                                .set_file_name("animation.mp4");
                            if let Some(dir) = export::suggest_dir(&self.state) {
                                dlg = dlg.set_directory(dir);
                            }
                            if let Some(path) = dlg.save_file() {
                                let ctx = ui.ctx().clone();
                                self.queue_export(&ctx, render_queue::ExportKind::Mp4, &path);
                            }
                        }
                        // GM15: the editorial's exports, written into the
                        // render folder: the EDL, the iro shitei and the
                        // open cut as a project for a freelancer.
                        ui.separator();
                        self.menu_item(ui, Action::ExportEdl);
                        self.menu_item(ui, Action::ExportColorSheet);
                        self.menu_item(ui, Action::ExportCutAsProject);
                        // GM18: the book's pages, into a folder the shell
                        // asks for (the Manga pane's options).
                        self.menu_item(ui, Action::ExportPages);
                    });
                    ui.separator();
                    // GM12: the sound verbs (WAV, MP3, OGG Vorbis or FLAC,
                    // decoded on a worker). A track's removal is a DANGER
                    // in its sheet header's context menu.
                    self.menu_item(ui, Action::ImportSound);
                    self.menu_item(ui, Action::AddSoundTrack);
                    // GM17: a glTF or OBJ model, embedded in the cut.
                    self.menu_item(ui, Action::Import3DModel);
                    // GM22: a sequence re-rendered on disk, read again.
                    self.menu_item(ui, Action::ReloadSequences);
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
                plate::menu_word(ui, "cut", |ui| {
                    self.menu_items(ui, "Cut");
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("now editing: {}", self.state.cut().name))
                            .color(plate::legend_dim()),
                    );
                    // Rename the current cut (one undo step, GM21) or scene
                    // (scaffolding, not undoable). Empty-buffer pattern: type
                    // a new name and confirm; no pre-seeding to fight.
                    ui.horizontal(|ui| {
                        plate::text_field(
                            ui,
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

                // GM3: was "ws" — the rooms, then the saved arrangements.
                plate::menu_word(ui, vocab::STEP, |ui| {
                    self.menu_items(ui, "Room");
                    ui.separator();
                    ui.label(egui::RichText::new("save current arrangement as:").color(plate::legend_dim()));
                    ui.horizontal(|ui| {
                        plate::text_field(ui, egui::TextEdit::singleline(&mut self.ws_name));
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
                        self.open_brush_maker();
                        ui.close();
                    }
                    // GM17: THE 3D LAYOUT ROOM, built in beside the forge's
                    // and off the spine like it — the Room menu's verb (D6).
                    if ui
                        .button("3D layout room")
                        .on_hover_text(
                            "View3D beside a composite viewer, the node graph under them — \
                             set the cut camera from the view; View ▸ 3D underlay draws the \
                             model faded under the canvas's drawing",
                        )
                        .clicked()
                    {
                        self.invoke(Action::Layout3dRoom, ui.ctx());
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
                            // GM16: a word, not 🖌, which no embedded face draws.
                            ui.menu_button(format!("brush: {bound}"), |ui| {
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
                        self.go_off_spine(&name);
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
                plate::menu_word(ui, "window", |ui| {
                    for pane in Pane::ALL {
                        // The Brush window is RETIRED (owner 2026-08-17):
                        // its settings live on the canvas deck + right rail.
                        // GM14: the Curves pane is the registry's own item
                        // (Window ▸ Curves (graph editor), listed below);
                        // GM7: so is Window ▸ Nuri check (paint check), and
                        // GM11: Window ▸ Render Queue (delivery), and GM15:
                        // the shinkō board and the henshū timeline.
                        if matches!(
                            *pane,
                            Pane::Brush
                                | Pane::Curves
                                | Pane::PaintCheck
                                | Pane::RenderQueue
                                | Pane::Board
                                | Pane::Episode
                        ) {
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
                        self.add_pane(Pane::Viewer(id));
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
            };
            let mut strip = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(row)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            if narrow {
                plate::menu_word(&mut strip, "Menu", |ui| menus(ui));
            } else {
                menus(&mut strip);
            }
            let menus_end = strip.min_rect().right();
            // The actions first: the steps' last pill takes what they leave.
            let search_w = if narrow { plate::PILL_H } else { plate::SEARCH_W };
            let actions_left = row.right() - 2.0 * plate::PILL_H - search_w;
            let dot = egui::Rect::from_center_size(
                egui::pos2(actions_left - 10.0, row.center().y),
                egui::vec2(12.0, 16.0),
            );
            // THE STEPS. Switching re-lenses the SAME document — layout +
            // tool/view state swap, data never forks. Tool/view restore is
            // all-or-nothing; a live stroke keeps the old tool state (same
            // law as saved workspaces).
            let steps_width: f32 = steps::STEPS.iter().map(|s| plate::pill_width(ui, s.label)).sum();
            // AU5: the room lamp stands here only during a session.
            let lamp = egui::Rect::from_center_size(
                dot.center() - egui::vec2(dot.width() + 2.0, 0.0),
                dot.size(),
            );
            let limit = if room.is_some() { lamp.left() } else { dot.left() } - steps::GROUP_GAP;
            let spare = limit - (menus_end + steps::GROUP_GAP) - steps_width;
            let place_width = place
                .as_deref()
                .map_or(0.0, |name| steps::place_width(plate::pill_width(ui, name), spare));
            let steps_x = steps::island_x(window, menus_end, steps_width, place_width, limit);
            let island = egui::Rect::from_min_size(
                egui::pos2(steps_x, row.top()),
                egui::vec2(steps_width + place_width, row.height()),
            );
            ui.painter().rect_filled(island, plate::R_PILL, plate::ISLAND);
            let mut pills = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(island)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            pills.spacing_mut().item_spacing.x = 0.0;
            for step in &steps::STEPS {
                let current = step.stage.is_some() && step.stage == self.stage;
                let primary = step.stage == Some(Stage::Export);
                if plate::step_pill(&mut pills, step.label, current, primary)
                    .on_hover_ui(|ui| {
                        ui.label(step.card());
                    })
                    .clicked()
                {
                    self.invoke(step.action, ui.ctx());
                }
            }
            if let Some(name) = &place {
                // Draw is the way back; the pill only says where this is.
                let card = match self.stage {
                    Some(st) => st.describes(),
                    None => name,
                };
                plate::place_pill(&mut pills, name, place_width).on_hover_text(card);
            }
            // THE ROOM LAMP (session): being in a room is a CONFIGURATION — a
            // plate fact, never Aka. It names the role and who is here, so
            // the artist never opens Settings to answer "am I connected?".
            if let Some((words, names)) = &room {
                let key = ui.interact(lamp, ui.id().with("room_lamp"), egui::Sense::hover());
                ui.painter().circle_filled(lamp.center(), 3.5, plate::TALLY);
                plate::speak(&key, egui::WidgetType::Label, None, words);
                let who = match names.as_slice() {
                    [] => "no one else here yet".to_string(),
                    names => format!("with: {}", names.join(", ")),
                };
                key.card(format!("{words}\n{who}"));
            }
            // GM2: the real dirty mark (revision vs saved + meta edits), not
            // "has undo history". A painted dot in reserved space, so it
            // moves nothing when it lights.
            if self.state.is_dirty() {
                ui.painter().circle_filled(dot.center(), 3.0, plate::TALLY);
                // GM2: the dot says what it means — on hover, and to screen
                // readers and the testkit (D7).
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
            // THE ACTIONS, on their own island. Undo and Redo speak apart
            // from the Edit menu's rows.
            let actions = egui::Rect::from_min_max(egui::pos2(actions_left, row.top()), row.right_bottom());
            ui.painter().rect_filled(actions, plate::R_PILL, plate::ISLAND);
            let mut keys = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(actions)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            keys.spacing_mut().item_spacing.x = 0.0;
            for (action, icon, name) in [
                (Action::Undo, Icon::Undo, "Undo (toolbar)"),
                (Action::Redo, Icon::Redo, "Redo (toolbar)"),
            ] {
                let can = self.can_perform(action);
                let key = keys.add_enabled_ui(can.is_ok(), |ui| plate::island_key(ui, icon, name)).inner;
                let key = match can {
                    Ok(()) => menus::hover_tip(key, action, ""),
                    Err(why) => key.on_disabled_hover_text(why),
                };
                if key.clicked() {
                    self.invoke(action, ui.ctx());
                }
            }
            let search = plate::search_key(&mut keys, !narrow);
            if menus::hover_tip(search, Action::CommandPalette, "").clicked() {
                self.invoke(Action::CommandPalette, ui.ctx());
            }
        });

        // THE FOOT (spec §4.1): transport is GLOBAL state, bolted to the
        // chassis — a tape deck's motor is never on a removable reel.
        // AU4: the band's two exact heights (spec 4.4). The strip's 56 stand
        // above the transport row, which is always the last 36.
        let strip = self.strip_open && self.strip_room();
        let rows = if strip { rail::STRIP_H } else { 0.0 };
        let band = egui::Panel::bottom("status_bar").exact_size(plate::BAND_H + rows);
        band.frame(plate::desk_frame(ui.style())).show_separator_line(false).show(ui, |ui| {
            if strip {
                let size = egui::vec2(ui.available_width(), rows);
                let rect = egui::Rect::from_min_size(ui.cursor().min, size);
                let safe = !self.canvas.stroke_active();
                let mut room = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                let at = strip::show(&mut room, &mut self.state, &mut self.strip, safe);
                // The sheet's chords answer over the strip (spec 7.2).
                self.pane_rects.push((Pane::XSheet, at, ui.layer_id()));
                ui.add_space(rows);
            }
            self.transport_row(ui);
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
            let (cut_id, frame) = (self.state.view.cut, self.state.view.frame);
            graph = if self.state.view.playing
                && let Some(id) = self.state.ram.shown(cut_id, frame, ui.ctx())
            {
                // GM12: RAM preview — a held frame plays from memory.
                viewer::GraphView::Ready(id)
            } else if self.final_look.on {
                // GM11: the master's look, rendered by the CPU on a worker
                // (D9) — no GPU needed; the last frame stays up meanwhile.
                let look = self.final_look.view(&self.state, ui.ctx());
                self.state.note_render_warnings(self.final_look.take_warnings());
                look
            } else if self.state.cut().graph.output.is_none() {
                viewer::GraphView::NoOutput
            } else if let (Some(g), Some(p)) = (&mut self.graph, &mut self.paint) {
                let (w, h) = (
                    self.state.engine.project.width,
                    self.state.engine.project.height,
                );
                // GM20: the paint layer on the paper's rect (the canvas's
                // one resize); the graph's pool stays frame-sized.
                self.canvas.ensure_layer(&self.state, p);
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
                        g.set_base_dir(self.state.project_dir());
                        let cut = self.state.cut();
                        let law = self.state.engine.project.color_law;
                        let id = g.execute(hash, fx, cut, law, frame, p);
                        self.state.note_render_warnings(g.take_warnings());
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
        #[cfg(test)]
        {
            self.dbg_graph = graph;
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
                        sh.hint = viewer::NOTHING_TO_SHOW.into();
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
        if ui_lock {
            self.locked_panes();
        }
        let rail_off = self.rail_off();
        let panes = visible_panes(&self.dock);
        let layers_docked = panes.contains(&Pane::Layers);
        self.canvas.strip_closed = self.strip_closed();
        self.canvas.sheet_shown = panes.contains(&Pane::XSheet);
        self.canvas.empty_cell_hint = self.say.empty_cell_due(&self.state);
        let export_view = self.export_view();
        let mut tabs = EditorTabs {
            stage: self.stage,
            state: &mut self.state,
            canvas: &mut self.canvas,
            paint: self.paint.as_mut(),
            graph,
            viewers: &mut self.viewers,
            final_look: &mut self.final_look.on,
            pen,
            layers_cfg,
            presets,
            presets_dirty,
            preset_name: &mut self.preset_name,
            forge: &mut self.forge,
            native_pen,
            float_canvas_open: self.float_canvas.open,
            pane_rects: &mut self.pane_rects,
            floats: &mut self.floats,
            rail_off,
            full_band: self.full_band,
            layers_docked,
            locked: if ui_lock { &self.held } else { &[] },
            export_view,
        };
        let dock_style = dock_style(ui.style().as_ref(), &self.dock);
        let dock_rect = ui.available_rect_before_wrap();
        plate::note_dock(ui, dock_rect);
        // UI lock (Settings -> UI Features): the layout freezes — no tab
        // dragging and no splits, and the recipe's panes stay open — but the
        // arrangement is kept.
        DockArea::new(&mut self.dock)
            .style(dock_style)
            .draggable_tabs(!ui_lock)
            .show_close_buttons(true)
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
        // GM15: and the shinkō board's and the henshū timeline's buttons.
        for action in std::mem::take(&mut self.state.board.asked) {
            self.invoke(action, ui.ctx());
        }
        // GM6: and the canvas's (the system Copy / Paste events over it).
        for action in std::mem::take(&mut self.canvas.asked) {
            self.invoke(action, ui.ctx());
        }
        // GM17: and View3D's toolbar (its numpad views, Frame all, the cut
        // camera from the view).
        for action in std::mem::take(&mut self.state.view3d.asked) {
            self.invoke(action, ui.ctx());
        }
        // GM18: and the Manga pane's New page, Clip to panels, panel tool,
        // ruler snap and Export pages.
        for action in std::mem::take(&mut self.state.manga.asked) {
            self.invoke(action, ui.ctx());
        }
        // AU9: and the Export panel's choices and its Done card.
        for ask in std::mem::take(&mut self.state.render_queue.panel.asked) {
            match ask {
                export_step::Ask::Do(action) => {
                    self.invoke(action, ui.ctx());
                }
                export_step::Ask::Back => self.leave_export(),
                export_step::Ask::App(request) => self.requests.push(request),
            }
        }
        // AU4: and the rail's and the floats'. The dock is free here.
        if std::mem::take(&mut self.floats.focus_layers) {
            self.open_pane(Pane::Layers);
        }
        if std::mem::take(&mut self.canvas.request_brush_maker) {
            self.open_brush_maker();
        }
        // AU7: and a viewer's empty state.
        let asked: Vec<Action> = self.viewers.values_mut().filter_map(|v| v.asked.take()).collect();
        for action in asked {
            self.invoke(action, ui.ctx());
        }
        self.start_ui(ui.ctx(), dock_rect);
        self.say_read(ui.ctx());

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
                        // AU4: its own rail, at its own tier; no strip here,
                        // and what is under the pointer here is no dock's.
                        self.canvas.strip_closed = false;
                        self.canvas.ui(
                            ui,
                            &mut self.state,
                            self.paint.as_mut(),
                            graph,
                            pen,
                            layers_cfg,
                            native_pen,
                        );
                        let host = self.canvas.dbg_rect;
                        let args = &mut rail::OverlayArgs {
                            raster_available: self.paint.is_some(),
                            canvas: &mut self.canvas,
                            state: &mut self.state,
                            presets,
                            floats: &mut self.floats,
                            manga: self.stage == Some(Stage::Manga),
                            rail_off: false,
                            full_band: self.full_band,
                            layers_docked: false,
                            pane_rects: None,
                        };
                        rail::overlay(ui, args, host);
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
