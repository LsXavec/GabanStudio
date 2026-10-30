//! GM11 THE RENDER QUEUE (tabbed into the satsuei and henshū rooms):
//! delivery jobs — this cut, every cut of its scene, or a frame range —
//! each with a preset, an output folder and a file-name pattern, rendered
//! one after another on a worker thread from a snapshot taken when the job
//! starts, so later edits never leak in (NEVER-DO 6: the pen never waits).
//! Progress per job; Stop lands after the current frame; a finished job
//! offers "show folder". The same frame loop runs the windowless batch
//! render (`animstudio --render`, main.rs).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use anim_core::color::ColorLaw;
use anim_core::command::CutRef;
use anim_core::deliver::{
    self, AudioIn, BurnIns, Codec, Format, Names, PRESETS, Preset, Purpose, Slate,
};
use anim_core::fx::Image;
use anim_core::model::{Cut, Project};
use eframe::egui;

use crate::config::RenderConfig;
use crate::doc::AppState;
use crate::plate;

/// The worker's answer for a job the artist stopped (not a failure).
pub const CANCELLED: &str = "stopped";

/// What a video preset says when no ffmpeg was found.
pub const NEEDS_FFMPEG: &str = "needs ffmpeg (ffmpeg.org); image sequences work without it";

/// Where a job stands.
#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed(String),
    Stopped,
}

/// One delivery job, as the queue lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub at: CutRef,
    /// The list's row: the cut's name and, for a range, its frames.
    pub label: String,
    /// The frames asked for (inclusive, 0-based); None = the whole cut,
    /// its length read when the job starts.
    pub frames: Option<(u32, u32)>,
    /// The frames it renders: the ask against the cut as it was added, and
    /// again at its start (the progress counts these).
    pub span: (u32, u32),
    /// An index into [`PRESETS`].
    pub preset: usize,
    pub burn_ins: BurnIns,
    /// A half-size proxy (50%) whatever the preset's scale.
    pub half: bool,
    /// An image sequence also writes the range's mixdown WAV beside it.
    pub mixdown: bool,
    pub out: PathBuf,
    pub pattern: String,
    pub state: JobState,
    /// Frames rendered so far.
    pub done: usize,
    /// GM15: an Episode job — every cut of `at.scene` back to back, one
    /// sequence or one video (`at.cut` is the scene's first cut).
    pub episode: bool,
}

impl Job {
    pub fn total(&self) -> usize {
        (self.span.1.saturating_sub(self.span.0) + 1) as usize
    }

    pub fn preset(&self) -> &'static Preset {
        &PRESETS[self.preset.min(PRESETS.len() - 1)]
    }
}

/// The pure queue: jobs in the order added, one rendering at a time.
#[derive(Debug, Default)]
pub struct Queue {
    pub jobs: Vec<Job>,
    /// Started and not stopped: a job that ends starts the next one.
    pub running: bool,
}

impl Queue {
    pub fn push(&mut self, job: Job) {
        self.jobs.push(job);
    }

    /// The job rendering now.
    pub fn current(&self) -> Option<usize> {
        self.jobs.iter().position(|j| j.state == JobState::Running)
    }

    /// Start the first queued job (in order); None when one is already
    /// rendering or none waits.
    pub fn start_next(&mut self) -> Option<usize> {
        if self.current().is_some() {
            return None;
        }
        let Some(i) = self.jobs.iter().position(|j| j.state == JobState::Queued) else {
            self.running = false;
            return None;
        };
        self.jobs[i].state = JobState::Running;
        self.jobs[i].done = 0;
        self.running = true;
        Some(i)
    }

    /// One more frame of job `i` is out.
    pub fn frame_done(&mut self, i: usize) {
        if let Some(j) = self.jobs.get_mut(i) {
            j.done = (j.done + 1).min(j.total());
        }
    }

    /// The worker's answer for job `i`: Ok = done, [`CANCELLED`] = stopped,
    /// another Err = failed. While the queue runs, a done or failed job
    /// hands on to the next queued one (returned, already marked running);
    /// a stopped job stops the queue.
    pub fn finish(&mut self, i: usize, result: Result<usize, String>) -> Option<usize> {
        let Some(j) = self.jobs.get_mut(i) else {
            return None;
        };
        j.state = match result {
            Ok(_) => JobState::Done,
            Err(e) if e == CANCELLED => JobState::Stopped,
            Err(e) => JobState::Failed(e),
        };
        if j.state == JobState::Stopped {
            self.running = false;
        }
        if self.running {
            self.start_next()
        } else {
            None
        }
    }

    /// Stop: nothing more starts (the running job stops after its frame).
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Drop every finished, failed or stopped job.
    pub fn clear_finished(&mut self) {
        self.jobs
            .retain(|j| matches!(j.state, JobState::Queued | JobState::Running));
    }
}

/// `frames` against a cut of `frame_count` frames: None = all of them; a
/// range is kept inside the cut.
pub fn resolve(frame_count: u32, frames: Option<(u32, u32)>) -> (u32, u32) {
    let last = frame_count.saturating_sub(1);
    frames.map_or((0, last), |(a, b)| {
        (a.min(last), b.min(last).max(a.min(last)))
    })
}

/// Everything a job renders from, taken when it starts (the cut is a
/// snapshot: Arc'd tiles, cheap; a Final job's is filtered once here, not
/// per frame).
pub struct JobSpec {
    pub cut: Cut,
    pub law: ColorLaw,
    pub size: (u32, u32),
    pub fps: u32,
    pub frames: (u32, u32),
    /// The job's own copy: a proxy job halves its scale.
    pub preset: Preset,
    pub burn_ins: BurnIns,
    pub slate: Slate,
    pub out: PathBuf,
    pub pattern: String,
    /// An image sequence also writes the range's mixdown WAV (on unless
    /// the queue's form turns it off).
    pub mixdown: bool,
    /// GM15: an Episode job's cuts; `cut` is then the first of them and
    /// `frames` spans the whole episode.
    pub episode: Option<deliver::Episode>,
    /// GM17: the project's folder (relative ImageSequence frames) and what
    /// the job could not read, reported once when it ends.
    pub env: Arc<anim_core::export::RenderEnv>,
}

impl JobSpec {
    /// GM17: the job reading relative frames from `dir`, the project's
    /// folder (None for an unsaved project).
    pub fn with_base_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.env = Arc::new(anim_core::export::RenderEnv::new(dir));
        self
    }

    /// GM15: the Episode job for `scene` of `project`: every cut in order,
    /// named as the episode ("episode" in the pattern's cut field). None
    /// when the scene is gone or has no cut.
    pub fn episode(
        project: &Project,
        scene: anim_core::ids::SceneId,
        preset: &Preset,
        burn_ins: BurnIns,
        out: PathBuf,
        pattern: String,
        artist: String,
    ) -> Option<JobSpec> {
        let ep = deliver::Episode::of(project, scene, preset.purpose)?;
        let total = ep.frame_count();
        Some(JobSpec {
            cut: ep.cuts[0].clone(),
            law: project.color_law,
            size: (project.width, project.height),
            fps: project.fps,
            frames: (0, total.max(1) - 1),
            preset: *preset,
            burn_ins,
            slate: Slate {
                names: ep.names.clone(),
                fps: project.fps,
                date: today(),
                artist,
            },
            out,
            pattern,
            mixdown: true,
            episode: Some(ep),
            env: Arc::default(),
        })
    }
    /// The job for cut `at` of `project` over `frames` (None = the whole
    /// cut; a range is kept inside the cut). None when the cut is gone.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project: &Project,
        at: CutRef,
        frames: Option<(u32, u32)>,
        preset: &Preset,
        burn_ins: BurnIns,
        out: PathBuf,
        pattern: String,
        artist: String,
    ) -> Option<JobSpec> {
        let scene = project.scene(at.scene)?;
        let cut = project.cut(at.scene, at.cut)?;
        let frames = resolve(cut.frame_count, frames);
        let mut snapshot = cut.clone();
        if preset.purpose == Purpose::Final {
            deliver::retain_purpose(&mut snapshot, Purpose::Final);
        }
        Some(JobSpec {
            cut: snapshot,
            law: project.color_law,
            size: (project.width, project.height),
            fps: project.fps,
            frames,
            preset: *preset,
            burn_ins,
            slate: Slate {
                names: Names {
                    project: project.name.clone(),
                    scene: scene.name.clone(),
                    cut: cut.name.clone(),
                },
                fps: project.fps,
                date: today(),
                artist,
            },
            out,
            pattern,
            mixdown: true,
            episode: None,
            env: Arc::default(),
        })
    }

    fn frame(&self, f: u32) -> Image {
        // GM15: an episode frame is its own cut's frame, burnt in with
        // that cut's names (the files carry the episode's).
        if let Some((i, local)) = self.episode.as_ref().and_then(|ep| ep.locate(f)) {
            let ep = self.episode.as_ref().expect("located above");
            let slate = Slate {
                names: Names {
                    cut: ep.cuts[i].name.clone(),
                    ..self.slate.names.clone()
                },
                ..self.slate.clone()
            };
            return deliver::deliver_frame_env(
                &ep.cuts[i],
                self.law,
                local,
                self.size,
                &self.preset,
                self.burn_ins,
                &slate,
                &self.env,
            );
        }
        deliver::deliver_frame_env(
            &self.cut,
            self.law,
            f,
            self.size,
            &self.preset,
            self.burn_ins,
            &self.slate,
            &self.env,
        )
    }

    /// Write the mixdown of the job's frames to `path` (the episode's cuts
    /// end to end for an Episode job); Ok(false) when nothing sounds.
    fn write_mixdown(&self, path: &Path, from: u32, to: u32) -> Result<bool, String> {
        match &self.episode {
            Some(ep) => ep.write_mixdown(path, self.fps.max(1)),
            None => deliver::write_mixdown(path, &self.cut, self.fps.max(1), from, to),
        }
    }

    fn has_sound(&self) -> bool {
        match &self.episode {
            Some(ep) => ep.cuts.iter().any(anim_core::audio::has_sound),
            None => anim_core::audio::has_sound(&self.cut),
        }
    }
}

/// Render a job's frames in order — the worker's loop and the batch
/// render's. `tick(frame)` follows each frame; `cancel` is read before
/// each one, so Stop lands after the frame in hand. Ok = frames written.
pub fn render_job(
    spec: &JobSpec,
    ffmpeg: Option<&Path>,
    cancel: &AtomicBool,
    tick: &mut dyn FnMut(u32),
) -> Result<usize, String> {
    std::fs::create_dir_all(&spec.out)
        .map_err(|e| format!("create {}: {e}", spec.out.display()))?;
    let format = spec.preset.format;
    if let Format::Video(codec) = format {
        let ffmpeg = ffmpeg.ok_or_else(|| NEEDS_FFMPEG.to_string())?;
        return render_video(spec, codec, ffmpeg, cancel, tick);
    }
    // A pattern that does not name the frame gets `_{frame:04}`: one file
    // per frame, never each frame over the last.
    let pattern = deliver::sequence_pattern(&spec.pattern);
    let mut n = 0;
    for f in spec.frames.0..=spec.frames.1 {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        let name = deliver::expand_pattern(&pattern, &spec.slate.names, f + 1, format.ext());
        deliver::write_frame(&spec.out.join(name), &spec.frame(f), format, spec.law)?;
        n += 1;
        tick(f);
    }
    // GM12: the range's mixdown beside the frames, named as a video of the
    // pattern would be (no file when no track sounds, or the form's
    // "mixdown WAV" latch is off).
    if spec.mixdown {
        let wav = deliver::video_name(&spec.pattern, &spec.slate.names, "wav");
        spec.write_mixdown(&spec.out.join(wav), spec.frames.0, spec.frames.1)?;
    }
    Ok(n)
}

/// A video job: frames piped as rawvideo RGBA into ffmpeg (spawned
/// through `proc::command`, no console window), the cut's WAV muxed as
/// v0.3's MP4 export muxed it.
fn render_video(
    spec: &JobSpec,
    codec: Codec,
    ffmpeg: &Path,
    cancel: &AtomicBool,
    tick: &mut dyn FnMut(u32),
) -> Result<usize, String> {
    use std::io::Write;
    let (w, h) = spec.preset.out_size(spec.size.0, spec.size.1);
    let fps = spec.fps.max(1);
    let total = spec.frames.1 - spec.frames.0 + 1;
    let tmp = std::env::temp_dir().join(format!(
        "animstudio_render_{}_{}",
        std::process::id(),
        spec.cut.id.0
    ));
    let audio_path = tmp.join("audio.wav");
    // GM12: every sounding track mixed down over the whole cut — the same
    // seek and length pick the range out of it as out of the clip before.
    let audio = if spec.has_sound() {
        std::fs::create_dir_all(&tmp).map_err(|e| format!("temp dir: {e}"))?;
        let last = spec.cut.frame_count.max(1) - 1;
        spec.write_mixdown(&audio_path, 0, last)
            .map_err(|e| format!("audio temp: {e}"))?;
        Some(AudioIn {
            path: &audio_path,
            offset: spec.frames.0 as f64 / fps as f64,
            secs: total as f64 / fps as f64,
        })
    } else {
        None
    };
    let out = spec.out.join(deliver::video_name(
        &spec.pattern,
        &spec.slate.names,
        codec.ext(),
    ));
    let args = deliver::ffmpeg_args(codec, fps, w, h, spec.preset.alpha, audio, &out);
    let mut child = crate::proc::command(ffmpeg)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("running ffmpeg: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("ffmpeg took no input")?;
    let mut n = 0;
    let mut result = Ok(());
    for f in spec.frames.0..=spec.frames.1 {
        if cancel.load(Ordering::Relaxed) {
            result = Err(CANCELLED.to_string());
            break;
        }
        let img = spec.frame(f);
        let bytes: Vec<u8> = img
            .px
            .chunks_exact(4)
            .flat_map(anim_core::export::straight_rgba8)
            .collect();
        if let Err(e) = stdin.write_all(&bytes) {
            result = Err(format!("ffmpeg stopped reading: {e}"));
            break;
        }
        n += 1;
        tick(f);
    }
    drop(stdin);
    if result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|e| format!("ffmpeg: {e}"));
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    match status? {
        s if s.success() => Ok(n),
        s => Err(format!("ffmpeg failed (exit {:?})", s.code())),
    }
}

/// The worker's messages: a frame is out, what the job could not read (just
/// before it ends, when anything), or the job ended.
pub enum Progress {
    Frame,
    Warnings(Vec<String>),
    Done(Result<usize, String>),
}

/// Run a job on its own thread (it owns its snapshot).
pub fn spawn(
    spec: JobSpec,
    ffmpeg: Option<PathBuf>,
    cancel: Arc<AtomicBool>,
    ctx: Option<egui::Context>,
) -> Receiver<Progress> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let ticks = tx.clone();
        let repaint = ctx.clone();
        let result = render_job(&spec, ffmpeg.as_deref(), &cancel, &mut |_| {
            let _ = ticks.send(Progress::Frame);
            if let Some(c) = &repaint {
                c.request_repaint();
            }
        });
        let warnings = spec.env.warnings();
        if !warnings.is_empty() {
            let _ = tx.send(Progress::Warnings(warnings));
        }
        let _ = tx.send(Progress::Done(result));
        if let Some(c) = &ctx {
            c.request_repaint();
        }
    });
    rx
}

struct Worker {
    job: usize,
    rx: Receiver<Progress>,
    cancel: Arc<AtomicBool>,
    /// Said with the finished job (cels whose legacy vector strokes no
    /// render includes, as v0.3's exports said).
    note: String,
}

impl Drop for Worker {
    /// A document closed mid-render stops its job after the frame in hand
    /// (the thread owns its snapshot; nothing would read its progress).
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The new-job form.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub preset: usize,
    pub burn_ins: BurnIns,
    /// Render at half size (a 50% proxy) whatever the preset's scale.
    pub half: bool,
    /// An image sequence also writes the range's mixdown WAV beside it.
    pub mixdown: bool,
    /// The output folder; empty = the configured default, else a `render`
    /// folder beside the project file.
    pub out: String,
    pub pattern: String,
    /// "+ frames" adds this inclusive range (0-based); None = the whole
    /// cut (the fields show it until one is moved).
    pub range: Option<(u32, u32)>,
    /// "+ selected cuts" queues these, one job each, in the project's
    /// order (picked in the pane's "pick cuts" fold-out, scene by scene).
    pub picked: Vec<CutRef>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            preset: 0,
            burn_ins: PRESETS[0].burn_ins,
            half: false,
            mixdown: true,
            out: String::new(),
            pattern: deliver::DEFAULT_PATTERN.into(),
            range: None,
            picked: Vec::new(),
        }
    }
}

/// The queue, its worker and its form — the document's (it lives on
/// `AppState`, like the paint check).
#[derive(Default)]
pub struct RenderQueue {
    pub queue: Queue,
    pub draft: Draft,
    worker: Option<Worker>,
    /// Settings ▸ General: ffmpeg's location (empty = search) and the
    /// default output folder, fed by the shell each frame.
    pub config: RenderConfig,
    /// The artist burn-in: the session's user name, fed with the config.
    pub artist: String,
    /// The search's answer for the configured location: (that location,
    /// the ffmpeg found). None = not searched yet.
    pub ffmpeg: Option<(String, Option<PathBuf>)>,
    /// "show folder" asks the shell to open this.
    pub show_folder: Option<PathBuf>,
}

impl RenderQueue {
    /// The shell's settings, each frame; a changed ffmpeg location is
    /// searched again.
    pub fn configure(&mut self, cfg: &RenderConfig, artist: &str) {
        if &self.config != cfg {
            self.config = cfg.clone();
        }
        if self.artist != artist {
            self.artist = artist.to_string();
        }
    }

    /// Settings ▸ General's "ffmpeg location", when one is typed.
    pub fn configured_ffmpeg(&self) -> Option<PathBuf> {
        let typed = self.config.ffmpeg.trim();
        (!typed.is_empty()).then(|| PathBuf::from(typed))
    }

    /// The ffmpeg video presets use, searched once per configured location
    /// (files only — nothing is spawned).
    pub fn ffmpeg_path(&mut self) -> Option<PathBuf> {
        let key = self.config.ffmpeg.trim().to_string();
        if self.ffmpeg.as_ref().is_none_or(|(k, _)| *k != key) {
            let found = crate::export::find_ffmpeg(self.configured_ffmpeg().as_deref());
            self.ffmpeg = Some((key, found));
        }
        self.ffmpeg.as_ref().and_then(|(_, p)| p.clone())
    }

    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }

    /// The folder a new job writes to.
    pub fn out_dir(&self, state_file: Option<&Path>) -> Option<PathBuf> {
        let typed = self.draft.out.trim();
        if !typed.is_empty() {
            return Some(PathBuf::from(typed));
        }
        let default = self.config.out_dir.trim();
        if !default.is_empty() {
            return Some(PathBuf::from(default));
        }
        state_file.and_then(Path::parent).map(|d| d.join("render"))
    }

    /// What an empty output-folder field means, said in the field: the
    /// default from Settings ▸ General, else `render` beside the project.
    pub fn out_hint(&self, state_file: Option<&Path>) -> String {
        let default = self.config.out_dir.trim();
        if !default.is_empty() {
            return default.to_string();
        }
        match state_file.and_then(Path::parent) {
            Some(dir) => dir.join("render").display().to_string(),
            None => "render/ beside the project (save it first)".into(),
        }
    }
}

/// `unix` seconds as a local YYYY-MM-DD (the burn-in's date).
pub fn civil_date(unix: u64, utc_offset_min: i32) -> String {
    let days = (unix as i64 + utc_offset_min as i64 * 60).div_euclid(86_400);
    // Howard Hinnant's days → civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn today() -> String {
    civil_date(
        crate::worksafe::unix_now(),
        crate::worksafe::utc_offset_min(),
    )
}

/// Add a job for cut `at` over `frames` (None = the whole cut, however
/// long it is when the job starts) from the form. Refuses on the Aka lane
/// without an output folder.
pub fn add_job(state: &mut AppState, at: CutRef, frames: Option<(u32, u32)>) -> bool {
    let Some(out) = state.render_queue.out_dir(state.file_path.as_deref()) else {
        state
            .refuse("the render queue needs an output folder: type one, or save the project first");
        return false;
    };
    let d = state.render_queue.draft.clone();
    push_job(state, at, frames, d.preset, d.burn_ins, d.half, out, d.pattern).is_some()
}

/// Queue one job; its index, or None when the cut is gone. A sequence's
/// mixdown WAV follows the form's latch.
#[allow(clippy::too_many_arguments)]
fn push_job(
    state: &mut AppState,
    at: CutRef,
    frames: Option<(u32, u32)>,
    preset: usize,
    burn_ins: BurnIns,
    half: bool,
    out: PathBuf,
    pattern: String,
) -> Option<usize> {
    let cut = state.engine.project.cut(at.scene, at.cut)?;
    let span = resolve(cut.frame_count, frames);
    let label = match frames {
        None => cut.name.clone(),
        Some(_) => format!("{} · {}–{}", cut.name, span.0 + 1, span.1 + 1),
    };
    let mixdown = state.render_queue.draft.mixdown;
    let q = &mut state.render_queue.queue;
    q.push(Job {
        at,
        label,
        frames,
        span,
        preset,
        burn_ins,
        half,
        mixdown,
        out,
        pattern,
        state: JobState::Queued,
        done: 0,
        episode: false,
    });
    Some(q.jobs.len() - 1)
}

/// GM15: "+ episode" — one job over every cut of `scene` in order, from
/// the form (its preset, burn-ins, folder and pattern). Refuses as
/// [`add_job`] does without a folder, and on a scene with no cut.
pub fn add_episode_job(state: &mut AppState, scene: anim_core::ids::SceneId) -> bool {
    let Some(first) = state
        .engine
        .project
        .scene(scene)
        .and_then(|s| s.cuts.first())
        .map(|c| c.id)
    else {
        state.refuse("refused — the scene has no cut to render");
        return false;
    };
    let Some(out) = state.render_queue.out_dir(state.file_path.as_deref()) else {
        state
            .refuse("the render queue needs an output folder: type one, or save the project first");
        return false;
    };
    let total = anim_core::board::trt_frames(state.engine.project.scene(scene).expect("found above"));
    let d = state.render_queue.draft.clone();
    let name = state.engine.project.scene(scene).map_or("", |s| s.name.as_str());
    let label = format!("episode {name} · {total} frames");
    // Said where it went (Render episode brings the queue forward).
    state.status = format!("episode {name} queued: {total} frames — see the Render Queue");
    let q = &mut state.render_queue.queue;
    q.push(Job {
        at: CutRef { scene, cut: first },
        label,
        frames: None,
        span: (0, (total.max(1) - 1) as u32),
        preset: d.preset,
        burn_ins: d.burn_ins,
        half: d.half,
        mixdown: d.mixdown,
        out,
        pattern: d.pattern,
        state: JobState::Queued,
        done: 0,
        episode: true,
    });
    true
}

/// "+ selected cuts": one job per picked cut, in the project's order.
/// The number queued.
pub fn add_picked(state: &mut AppState) -> usize {
    let picked = &state.render_queue.draft.picked;
    let cuts: Vec<CutRef> = state
        .engine
        .project
        .scenes
        .iter()
        .flat_map(|s| {
            s.cuts.iter().map(|c| CutRef {
                scene: s.id,
                cut: c.id,
            })
        })
        .filter(|c| picked.contains(c))
        .collect();
    let mut n = 0;
    for c in cuts {
        if !add_job(state, c, None) {
            break;
        }
        n += 1;
    }
    n
}

/// What File ▸ Export… queues (GM11: the menu's two items are queue jobs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    /// A PNG sequence into a folder (PNG 8-bit, alpha kept).
    Png,
    /// One MP4 file (H.264, on white).
    Mp4,
}

impl ExportKind {
    pub fn preset(self) -> usize {
        let name = match self {
            ExportKind::Png => "PNG 8-bit",
            ExportKind::Mp4 => "H.264 review",
        };
        PRESETS.iter().position(|p| p.name == name).unwrap_or(0)
    }
}

/// File ▸ Export…'s job for the current cut over `frames` (None = the
/// whole cut): a PNG sequence into the folder `dest` named by the form's
/// pattern, or an MP4 at the file `dest` (its folder; its name as the
/// pattern). No burn-ins, as v0.3's exports had none. The job's index.
pub fn export_job(
    state: &mut AppState,
    kind: ExportKind,
    dest: &Path,
    frames: Option<(u32, u32)>,
) -> Option<usize> {
    let (out, pattern) = match kind {
        ExportKind::Png => (dest.to_path_buf(), state.render_queue.draft.pattern.clone()),
        ExportKind::Mp4 => (
            dest.parent().map(Path::to_path_buf).unwrap_or_default(),
            dest.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| deliver::DEFAULT_PATTERN.into()),
        ),
    };
    let at = state.at();
    push_job(
        state,
        at,
        frames,
        kind.preset(),
        BurnIns::NONE,
        false,
        out,
        pattern,
    )
}

/// Start the queue (a no-op while a job renders).
pub fn start(state: &mut AppState, ctx: &egui::Context) {
    if let Some(i) = state.render_queue.queue.start_next() {
        launch(state, i, ctx);
    }
}

/// Stop: the running job stops after its current frame; nothing more
/// starts.
pub fn stop(state: &mut AppState) {
    let rq = &mut state.render_queue;
    rq.queue.stop();
    if let Some(w) = &rq.worker {
        w.cancel.store(true, Ordering::Relaxed);
    }
}

/// Snapshot job `i` (marked running) and hand it to a worker; a job whose
/// cut is gone fails at once and the next one starts.
fn launch(state: &mut AppState, mut i: usize, ctx: &egui::Context) {
    loop {
        let job = state.render_queue.queue.jobs[i].clone();
        let artist = state.render_queue.artist.clone();
        // The ask is read now: a whole-cut job renders the cut's length at
        // its start, not when it was added (an Episode job, the scene's
        // cuts as they are then).
        let spec = if job.episode {
            JobSpec::episode(
                &state.engine.project,
                job.at.scene,
                job.preset(),
                job.burn_ins,
                job.out.clone(),
                job.pattern.clone(),
                artist,
            )
        } else {
            JobSpec::new(
                &state.engine.project,
                job.at,
                job.frames,
                job.preset(),
                job.burn_ins,
                job.out.clone(),
                job.pattern.clone(),
                artist,
            )
        };
        let Some(mut spec) = spec.map(|s| s.with_base_dir(state.project_dir())) else {
            match state
                .render_queue
                .queue
                .finish(i, Err("its cut is gone".into()))
            {
                Some(next) => {
                    i = next;
                    continue;
                }
                None => return,
            }
        };
        if job.half {
            spec.preset.scale = 0.5;
        }
        spec.mixdown = job.mixdown;
        state.render_queue.queue.jobs[i].span = spec.frames;
        let note = match anim_core::export::vector_stroke_cels(&spec.cut) {
            0 => String::new(),
            n => format!(" ({n} cel(s) carry legacy vector strokes — not included)"),
        };
        let ffmpeg = state.render_queue.ffmpeg_path();
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = spawn(spec, ffmpeg, cancel.clone(), Some(ctx.clone()));
        state.render_queue.worker = Some(Worker {
            job: i,
            rx,
            cancel,
            note,
        });
        return;
    }
}

/// Take the worker's progress (every frame, the pane shown or not). Never
/// waits.
pub fn poll(state: &mut AppState, ctx: &egui::Context) {
    let Some(w) = &state.render_queue.worker else {
        return;
    };
    let job = w.job;
    let note = w.note.clone();
    let mut finished = None;
    let mut frames = 0;
    let mut warnings = Vec::new();
    loop {
        match w.rx.try_recv() {
            Ok(Progress::Frame) => frames += 1,
            Ok(Progress::Warnings(w)) => warnings = w,
            Ok(Progress::Done(r)) => {
                finished = Some(r);
                break;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                finished = Some(Err("the render stopped without finishing".into()));
                break;
            }
        }
    }
    for _ in 0..frames {
        state.render_queue.queue.frame_done(job);
    }
    let Some(result) = finished else { return };
    state.render_queue.worker = None;
    let label = state
        .render_queue
        .queue
        .jobs
        .get(job)
        .map(|j| j.label.clone())
        .unwrap_or_default();
    match &result {
        Ok(n) => state.status = format!("rendered {label}: {n} frame(s){note}"),
        Err(e) if e == CANCELLED => state.status = format!("render of {label} stopped"),
        Err(e) => state.refuse(format!("the render of {label} did not finish — {e}")),
    }
    // GM17: what the job could not read (a missing frame), once per job.
    if let Some(first) = warnings.first() {
        let more = match warnings.len() {
            1 => String::new(),
            n => format!(" (and {} more)", n - 1),
        };
        state.status = format!("{} — {first}{more}", state.status);
    }
    if let Some(next) = state.render_queue.queue.finish(job, result) {
        launch(state, next, ctx);
    }
}

/// Tests: block until the running job ends (no sleep, no clock).
#[cfg(test)]
pub fn wait(state: &mut AppState, ctx: &egui::Context) {
    while state.render_queue.busy() {
        let w = state.render_queue.worker.as_ref().unwrap();
        let msg =
            w.rx.recv()
                .unwrap_or(Progress::Done(Err("worker gone".into())));
        let job = w.job;
        match msg {
            Progress::Frame => state.render_queue.queue.frame_done(job),
            Progress::Warnings(w) => state.note_render_warnings(w),
            Progress::Done(r) => {
                state.render_queue.worker = None;
                if let Some(next) = state.render_queue.queue.finish(job, r) {
                    launch(state, next, ctx);
                }
            }
        }
    }
}

/// The pane: the new-job form over the job list.
pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    poll(state, ui.ctx());
    let ffmpeg = state.render_queue.ffmpeg_path();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            plate::legend(ui, "render queue (delivery)");
            ui.separator();
            ui.label("preset");
            ui.horizontal_wrapped(|ui| {
                for (i, p) in PRESETS.iter().enumerate() {
                    let usable = !p.format.is_video() || ffmpeg.is_some();
                    let armed = state.render_queue.draft.preset == i;
                    let r = ui
                        .add_enabled_ui(usable, |ui| plate::detent(ui, armed, p.name))
                        .inner
                        .on_hover_text(preset_help(p))
                        .on_disabled_hover_text(NEEDS_FFMPEG);
                    if r.clicked() && !armed {
                        let d = &mut state.render_queue.draft;
                        d.preset = i;
                        d.burn_ins = p.burn_ins;
                    }
                }
            });
            match &ffmpeg {
                Some(path) => {
                    ui.label(
                        egui::RichText::new(format!("ffmpeg: {}", path.display()))
                            .size(10.0)
                            .color(plate::legend_dim()),
                    );
                }
                None => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(NEEDS_FFMPEG).color(plate::legend_dim()))
                            .on_hover_text("set its location on Settings ▸ General");
                        // The search is cached; one installed while the app
                        // is open is found when asked (files only, nothing
                        // runs to look).
                        if ui
                            .small_button("look again")
                            .on_hover_text(
                                "search for ffmpeg again (after installing it, or copying \
                                 it beside the app)",
                            )
                            .clicked()
                        {
                            state.render_queue.ffmpeg = None;
                        }
                    });
                }
            }
            let out_hint = state.render_queue.out_hint(state.file_path.as_deref());
            let d = &mut state.render_queue.draft;
            ui.horizontal_wrapped(|ui| {
                ui.label("burn-ins");
                let b = &mut d.burn_ins;
                for (on, label, tip) in [
                    (&mut b.project, "project", "the project's name, top left"),
                    (&mut b.cut, "cut", "the scene and cut, top left"),
                    (
                        &mut b.frame,
                        "frame (s+k)",
                        "the frame number and seconds+koma, as the timesheet counts, \
                         bottom left",
                    ),
                    (&mut b.timecode, "timecode", "HH:MM:SS:FF non-drop, bottom right"),
                    (&mut b.date, "date", "the render's date, top right"),
                    (
                        &mut b.artist,
                        "artist",
                        "your name from Settings ▸ Session, top right",
                    ),
                ] {
                    plate::latch(ui, on, label).on_hover_text(tip);
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("size");
                plate::latch(ui, &mut d.half, "half size (proxy)").on_hover_text(
                    "render at 50% (each pixel the mean of a 2×2 block): quick review \
                     copies of any preset",
                );
                ui.label("sound");
                plate::latch(ui, &mut d.mixdown, "mixdown WAV").on_hover_text(
                    "an image sequence also writes the range's sound mix as a WAV beside \
                     its frames (a video carries its sound either way)",
                );
            });
            ui.horizontal(|ui| {
                ui.label("output folder");
                ui.add(
                    egui::TextEdit::singleline(&mut d.out)
                        .hint_text(out_hint)
                        .desired_width(220.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label("file names");
                ui.add(egui::TextEdit::singleline(&mut d.pattern).desired_width(220.0))
                    .on_hover_text(
                        "{project} {scene} {cut} {frame} {frame:04}; an image sequence \
                         whose pattern has no frame gets _{frame:04}",
                    );
            });
            let at = state.at();
            let n = state.cut().frame_count.max(1);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("+ this cut")
                    .on_hover_text("the whole current cut (its length when the job starts)")
                    .clicked()
                {
                    add_job(state, at, None);
                }
                if ui
                    .button("+ every cut in the scene")
                    .on_hover_text("one job per cut of this scene")
                    .clicked()
                {
                    let cuts: Vec<CutRef> = state
                        .engine
                        .project
                        .scene(at.scene)
                        .map(|s| {
                            s.cuts
                                .iter()
                                .map(|c| CutRef {
                                    scene: at.scene,
                                    cut: c.id,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    for c in cuts {
                        if !add_job(state, c, None) {
                            break;
                        }
                    }
                }
                let picked = state.render_queue.draft.picked.len();
                if ui
                    .add_enabled(picked > 0, egui::Button::new("+ selected cuts"))
                    .on_hover_text(format!(
                        "one job per cut picked below ({picked} picked), in the project's order"
                    ))
                    .on_disabled_hover_text("pick cuts in \"pick cuts\" below first")
                    .clicked()
                {
                    add_picked(state);
                }
                // GM15: the scene's cuts back to back as ONE job.
                if ui
                    .button("+ episode")
                    .on_hover_text(
                        "every cut of this scene in order as one sequence or one video, \
                         their sound end to end",
                    )
                    .clicked()
                {
                    add_episode_job(state, at.scene);
                }
            });
            // The cuts to queue together: this scene's, as latches (picks
            // in other scenes are kept while this one shows).
            let scene_cuts: Vec<(CutRef, String)> = state
                .engine
                .project
                .scene(at.scene)
                .map(|s| {
                    s.cuts
                        .iter()
                        .map(|c| {
                            let r = CutRef {
                                scene: at.scene,
                                cut: c.id,
                            };
                            (r, c.name.clone())
                        })
                        .collect()
                })
                .unwrap_or_default();
            egui::CollapsingHeader::new("pick cuts")
                .id_salt("render queue pick cuts")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let picked = &mut state.render_queue.draft.picked;
                        for (c, name) in &scene_cuts {
                            let mut on = picked.contains(c);
                            if plate::latch(ui, &mut on, name)
                                .on_hover_text("queued by \"+ selected cuts\"")
                                .changed()
                            {
                                picked.retain(|p| p != c);
                                if on {
                                    picked.push(*c);
                                }
                            }
                        }
                    });
                });
            ui.horizontal_wrapped(|ui| {
                // Untouched, the range is the whole cut (as File ▸ Export's).
                let (a, b) = resolve(n, state.render_queue.draft.range);
                let (mut a1, mut b1) = (a + 1, b + 1);
                let moved_a = ui
                    .add(egui::DragValue::new(&mut a1).range(1..=n).prefix("from "))
                    .changed();
                let moved_b = ui
                    .add(egui::DragValue::new(&mut b1).range(1..=n).prefix("to "))
                    .changed();
                if moved_a || moved_b {
                    state.render_queue.draft.range = Some((a1 - 1, (b1 - 1).max(a1 - 1)));
                }
                if ui
                    .button("+ frames")
                    .on_hover_text("this cut, those frames only")
                    .clicked()
                {
                    let r = state.render_queue.draft.range;
                    add_job(state, at, r);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let rq = &state.render_queue;
                let waiting = rq.queue.jobs.iter().any(|j| j.state == JobState::Queued);
                if ui
                    .add_enabled(!rq.busy() && waiting, egui::Button::new("Start"))
                    .on_hover_text(
                        "render the queued jobs in order on a worker; you can keep drawing",
                    )
                    .clicked()
                {
                    start(state, ui.ctx());
                }
                if ui
                    .add_enabled(state.render_queue.busy(), egui::Button::new("Stop"))
                    .on_hover_text("stops after the frame in hand")
                    .clicked()
                {
                    stop(state);
                }
                // Not while a job renders: the worker knows its job by index.
                if ui
                    .add_enabled(
                        !state.render_queue.busy(),
                        egui::Button::new("clear finished"),
                    )
                    .on_hover_text("remove the done, failed and stopped jobs from the list")
                    .clicked()
                {
                    state.render_queue.queue.clear_finished();
                }
            });
            if state.render_queue.queue.jobs.is_empty() {
                ui.label(egui::RichText::new("no jobs — add one above").color(plate::legend_dim()));
            }
            let mut show = None;
            for job in &state.render_queue.queue.jobs {
                ui.horizontal(|ui| {
                    let (word, color) = match &job.state {
                        JobState::Queued => ("queued".to_string(), plate::legend_dim()),
                        JobState::Running => ("rendering".to_string(), plate::TALLY),
                        JobState::Done => ("done".to_string(), plate::AO),
                        JobState::Failed(e) => (format!("failed: {e}"), plate::AKA),
                        JobState::Stopped => ("stopped".to_string(), plate::legend_dim()),
                    };
                    let half = if job.half { " · 50%" } else { "" };
                    ui.label(format!("{} · {}{half}", job.label, job.preset().name));
                    // The plate's meter, as the export window had (AUDIT
                    // [15]); the count rides beside it.
                    plate::meter(ui, Some(job.done as f32 / job.total() as f32), 90.0);
                    ui.label(
                        egui::RichText::new(format!("{}/{}", job.done, job.total()))
                            .monospace()
                            .size(11.0)
                            .color(plate::STRUCK),
                    );
                    ui.label(egui::RichText::new(word).color(color));
                    if job.state == JobState::Done && ui.small_button("show folder").clicked() {
                        show = Some(job.out.clone());
                    }
                });
            }
            if show.is_some() {
                state.render_queue.show_folder = show;
            }
        });
}

fn preset_help(p: &Preset) -> String {
    let what = match p.format {
        Format::Png16 => "PNG, 16 bits a channel",
        Format::Tiff16 => "TIFF, 16 bits a channel",
        Format::Exr => "OpenEXR half float, scene-linear",
        Format::Png8 => "PNG, 8 bits a channel",
        Format::Video(Codec::H264) => "H.264 MP4 (ffmpeg)",
        Format::Video(Codec::H265) => "H.265 MP4 (ffmpeg)",
        Format::Video(Codec::ProRes422Hq) => "ProRes 422 HQ QuickTime (ffmpeg)",
        Format::Video(Codec::ProRes4444) => "ProRes 4444 QuickTime with alpha (ffmpeg)",
        Format::Video(Codec::DnxhrHq) => "DNxHR HQ QuickTime (ffmpeg)",
    };
    let alpha = if p.alpha { "alpha kept" } else { "on white" };
    let look = match p.purpose {
        Purpose::Final => "final: no rough, reference or correction (sakkan) layers, no Ref columns",
        Purpose::View => "view: everything the composite view shows",
        Purpose::LineTest => "line test: line layers only, black on white",
    };
    let burn = if p.burn_ins.is_empty() {
        ""
    } else {
        "; burn-ins"
    };
    format!("{what}; {look}; {alpha}{burn}")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use anim_core::command::Command;
    use anim_core::model::{CelLayer, LayerRole};
    use anim_core::raster::{TILE, TILE_LEN, TileData, f32_to_f16_bits};

    use super::*;
    use crate::config::Action;
    use crate::testkit::EditorHarness;
    use crate::workspace::{Pane, Stage};

    /// A 64×36, 3-frame project: one drawing (a line layer inked in a block)
    /// on frame 1, held.
    fn small() -> AppState {
        let mut s = AppState::new_project("show", 64, 36, 24, 72.0);
        let at = s.at();
        s.engine
            .apply("len", vec![Command::SetFrameCount { at, frames: 3 }])
            .expect("3 frames");
        let column = s.view.active_column;
        let (d, l) = (s.engine.alloc_drawing_id(), s.engine.alloc_layer_id());
        let mut px = vec![0u16; TILE_LEN];
        for y in 4..12 {
            for x in 8..20 {
                for c in 0..4 {
                    px[(y * TILE + x) * 4 + c] = f32_to_f16_bits([0.0, 0.0, 0.5, 1.0][c]);
                }
            }
        }
        s.engine
            .apply(
                "art",
                vec![
                    Command::AddDrawing {
                        at,
                        id: d,
                        index: 0,
                        name: "1".into(),
                        strokes: vec![],
                        layers: vec![CelLayer::with_role(l, "line", LayerRole::Line)],
                    },
                    Command::PaintTiles {
                        at,
                        id: d,
                        layer: l,
                        diff: vec![((0, 0), None, Some(Arc::new(TileData::from_vec(px))))],
                    },
                    Command::SetCell {
                        at,
                        column,
                        frame: 0,
                        key: Some(anim_core::xsheet::Exposure::Drawing(d)),
                    },
                ],
            )
            .expect("the drawing");
        s
    }

    fn job(i: usize, out: &Path) -> Job {
        Job {
            at: CutRef {
                scene: anim_core::ids::SceneId(1),
                cut: anim_core::ids::CutId(i as u64),
            },
            label: format!("c{i}"),
            frames: None,
            span: (0, 2),
            preset: 0,
            burn_ins: BurnIns::NONE,
            half: false,
            mixdown: true,
            out: out.to_path_buf(),
            pattern: deliver::DEFAULT_PATTERN.into(),
            state: JobState::Queued,
            done: 0,
            episode: false,
        }
    }

    fn spec(s: &AppState, preset: &str, out: &Path) -> JobSpec {
        let p = Preset::named(preset).unwrap();
        JobSpec::new(
            &s.engine.project,
            s.at(),
            None,
            p,
            p.burn_ins,
            out.into(),
            deliver::DEFAULT_PATTERN.into(),
            "mika".into(),
        )
        .unwrap()
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    /// A written PNG's size, bit depth and its pixel at (x, y) (16-bit RGBA).
    fn png16(path: &Path, x: u32, y: u32) -> ((u32, u32), png::BitDepth, [u16; 4]) {
        let file = std::fs::File::open(path).unwrap();
        let mut reader = png::Decoder::new(std::io::BufReader::new(file))
            .read_info()
            .unwrap();
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let i = ((y * info.width + x) * 8) as usize;
        let px = [0, 1, 2, 3].map(|c| u16::from_be_bytes([buf[i + 2 * c], buf[i + 2 * c + 1]]));
        ((info.width, info.height), info.bit_depth, px)
    }

    #[test]
    fn render_queue_runs_jobs_in_order_and_counts_progress() {
        let mut q = Queue::default();
        for i in 0..3 {
            q.push(job(i, Path::new("o")));
        }
        assert_eq!(q.start_next(), Some(0));
        assert_eq!(q.start_next(), None, "one job renders at a time");
        for _ in 0..5 {
            q.frame_done(0);
        }
        assert_eq!(q.jobs[0].done, 3, "progress never passes the job's frames");
        // A done job hands on to the next, in order; a failure too.
        assert_eq!(q.finish(0, Ok(3)), Some(1));
        assert_eq!(q.jobs[0].state, JobState::Done);
        assert_eq!(q.current(), Some(1));
        assert_eq!(q.finish(1, Err("disk full".into())), Some(2));
        assert_eq!(q.jobs[1].state, JobState::Failed("disk full".into()));
        // A stopped job stops the queue.
        assert_eq!(q.finish(2, Err(CANCELLED.into())), None);
        assert_eq!(q.jobs[2].state, JobState::Stopped);
        assert!(!q.running);
        q.push(job(3, Path::new("o")));
        q.stop();
        assert_eq!(q.start_next(), Some(3));
        assert_eq!(q.finish(3, Ok(3)), None, "the last job ends the run");
        assert!(!q.running);
        q.clear_finished();
        assert!(q.jobs.is_empty());
        assert_eq!(civil_date(1_761_768_000, -300), "2025-10-29");
        assert_eq!(civil_date(0, 0), "1970-01-01");
    }

    #[test]
    fn render_queue_stop_lands_after_the_current_frame_and_snapshots_hold() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = small();
        let sp = spec(&s, "Master PNG 16-bit", dir.path());
        // An edit after the snapshot does not leak into the job.
        let at = s.at();
        s.engine
            .apply("len", vec![Command::SetFrameCount { at, frames: 5 }])
            .unwrap();
        s.engine.project.name = "edited".into();
        assert_eq!(sp.frames, (0, 2));
        let cancel = AtomicBool::new(false);
        let mut ticks = Vec::new();
        let r = render_job(&sp, None, &cancel, &mut |f| {
            ticks.push(f);
            cancel.store(true, Ordering::Relaxed);
        });
        assert_eq!(r, Err(CANCELLED.to_string()));
        assert_eq!(ticks, vec![0], "the frame in hand finished, no more");
        assert_eq!(
            files(dir.path()),
            vec!["show_SC01_CUT01_0001.png".to_string()]
        );
        // A video job without ffmpeg refuses, renders nothing, spawns nothing.
        let video = spec(&s, "ProRes 422 HQ", dir.path());
        let none = AtomicBool::new(false);
        assert_eq!(
            render_job(&video, None, &none, &mut |_| {}),
            Err(NEEDS_FFMPEG.to_string())
        );
    }

    #[test]
    fn render_queue_worker_renders_a_three_frame_job() {
        let dir = tempfile::tempdir().unwrap();
        let s = small();
        let sp = spec(&s, "Master PNG 16-bit", dir.path());
        let rx = spawn(sp, None, Arc::new(AtomicBool::new(false)), None);
        let mut frames = 0;
        let result = loop {
            match rx.recv().unwrap() {
                Progress::Frame => frames += 1,
                Progress::Warnings(w) => panic!("nothing to warn of: {w:?}"),
                Progress::Done(r) => break r,
            }
        };
        assert_eq!(result, Ok(3));
        assert_eq!(frames, 3);
        let names = files(dir.path());
        assert_eq!(names.len(), 3, "{names:?}");
        let (size, depth, px) = png16(&dir.path().join(&names[0]), 10, 6);
        assert_eq!((size, depth), ((64, 36), png::BitDepth::Sixteen));
        assert_eq!(px, [0, 0, 32768, 65535]);
    }

    #[test]
    fn render_queue_find_ffmpeg_in_searches_in_order() {
        use crate::export::find_ffmpeg_in;
        let exe = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let root = tempfile::tempdir().unwrap();
        let mk = |sub: &str| {
            let d = root.path().join(sub);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(exe), b"").unwrap();
            d
        };
        let configured = mk("configured");
        let exe_dir = mk("app");
        let on_path = mk("bin");
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let path_var = std::env::join_paths([empty.clone(), on_path.clone()]).unwrap();
        let local = root.path().join("local");
        let winget = local.join("Microsoft/WinGet/Packages/Gyan.FFmpeg_Microsoft.Winget.Source/ffmpeg-8.1.2-full_build/bin");
        std::fs::create_dir_all(&winget).unwrap();
        std::fs::write(winget.join("ffmpeg.exe"), b"").unwrap();
        let all = |c: Option<&Path>,
                   e: Option<&Path>,
                   p: Option<&std::ffi::OsStr>,
                   l: Option<&Path>| { find_ffmpeg_in(c, e, p, l) };
        let cfg_file = configured.join(exe);
        assert_eq!(
            all(
                Some(&cfg_file),
                Some(&exe_dir),
                Some(&path_var),
                Some(&local)
            ),
            Some(cfg_file.clone())
        );
        // A configured folder holding it works too.
        assert_eq!(all(Some(&configured), None, None, None), Some(cfg_file));
        assert_eq!(
            all(None, Some(&exe_dir), Some(&path_var), Some(&local)),
            Some(exe_dir.join(exe))
        );
        // A configured path that is not there falls through.
        let gone = root.path().join("gone/ffmpeg.exe");
        assert_eq!(
            all(Some(&gone), None, Some(&path_var), Some(&local)),
            Some(on_path.join(exe))
        );
        assert_eq!(
            all(None, Some(&empty), None, Some(&local)),
            Some(winget.join("ffmpeg.exe"))
        );
        assert_eq!(
            all(None, Some(&empty), Some(empty.as_os_str()), Some(&empty)),
            None
        );
        assert_eq!(all(None, None, None, None), None);
    }

    #[test]
    fn render_queue_sits_in_the_composite_and_edit_rooms_and_greys_video_without_ffmpeg() {
        for st in [Stage::Composite, Stage::Edit] {
            assert!(
                st.dock()
                    .iter_all_tabs()
                    .any(|(_, p)| *p == Pane::RenderQueue),
                "{st:?}"
            );
        }
        assert_eq!(Pane::RenderQueue.title(), "Render Queue");
        assert!(Pane::ALL.contains(&Pane::RenderQueue));
        let mut h = EditorHarness::new();
        // No ffmpeg anywhere (the search's answer, faked: tests scan nothing).
        h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
        h.perform(Action::RoomComposite);
        h.run_frames(3);
        // The room shows the tab; brought forward, the pane draws.
        let tab = h
            .editor()
            .dock
            .find_tab(&Pane::RenderQueue)
            .expect("a Render Queue tab");
        let _ = h.editor_mut().dock.set_active_tab(tab);
        h.run_frames(2);
        assert!(h.has_text_containing("RENDER QUEUE"), "{}", h.describe());
        assert!(h.has_text_containing(NEEDS_FFMPEG), "{}", h.describe());
        assert_eq!(h.node_enabled("Master PNG 16-bit"), Some(true));
        assert_eq!(h.node_enabled("EXR half (linear)"), Some(true));
        for video in [
            "H.264 review",
            "ProRes 4444 (alpha)",
            "DNxHR HQ",
            "Line test",
        ] {
            assert_eq!(h.node_enabled(video), Some(false), "{video}");
        }
        // The line test as frames needs none.
        assert_eq!(h.node_enabled("Line test PNG"), Some(true));
        // With one found, they come alive.
        h.state_mut().render_queue.ffmpeg =
            Some((String::new(), Some(PathBuf::from("C:/x/ffmpeg.exe"))));
        h.run_frames(2);
        assert_eq!(h.node_enabled("ProRes 4444 (alpha)"), Some(true));
        assert!(!h.has_text_containing(NEEDS_FFMPEG));
    }

    #[test]
    fn render_queue_pane_adds_starts_and_finishes_a_job() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = EditorHarness::with_state(small());
        h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
        h.state_mut().render_queue.draft.out = dir.path().display().to_string();
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::RenderQueue]);
        h.run();
        h.click("+ this cut");
        h.run();
        assert_eq!(h.state().render_queue.queue.jobs.len(), 1);
        h.click("Start");
        h.run();
        let ctx = h.ctx();
        wait(h.state_mut(), &ctx);
        h.run();
        let j = &h.state().render_queue.queue.jobs[0];
        assert_eq!((j.state.clone(), j.done), (JobState::Done, 3));
        assert_eq!(files(dir.path()).len(), 3);
        assert!(h.has("show folder"));
        h.click("show folder");
        h.run();
        // The shell opens it; the harness only sees the ask.
        assert_eq!(
            h.state_mut().render_queue.show_folder.take(),
            Some(dir.path().to_path_buf())
        );
    }

    #[test]
    fn render_queue_cli_job_writes_png16_frames() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("show.animproj");
        anim_core::store::save(&small().engine.project, &file).unwrap();
        let out = dir.path().join("out");
        let args: Vec<String> = [
            "--render",
            &file.display().to_string(),
            "--preset",
            "Master PNG 16-bit",
            "--out",
            &out.display().to_string(),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let job = deliver::parse_cli(&args).unwrap();
        let data = tempfile::tempdir().unwrap();
        assert_eq!(crate::run_cli_job(&job, Some(data.path())), Ok(3));
        let names = files(&out);
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(names.iter().all(|n| n.ends_with(".png")));
        let (size, depth, _) = png16(&out.join(&names[2]), 0, 0);
        assert_eq!((size, depth), ((64, 36), png::BitDepth::Sixteen));
        // A missing file is a failed job (exit 1), bad arguments exit 2.
        let missing = deliver::CliJob {
            source: deliver::CliSource::File(dir.path().join("nope.animproj")),
            ..job.clone()
        };
        assert!(crate::run_cli_job(&missing, Some(data.path())).is_err());
        assert_eq!(
            crate::cli_exit(&["--render".into()], Some(data.path())),
            deliver::EXIT_USAGE
        );
        let sample: Vec<String> = [
            "--render",
            "--sample",
            "--preset",
            "PNG 8-bit",
            "--out",
            &dir.path().join("s").display().to_string(),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            crate::cli_exit(&sample, Some(data.path())),
            deliver::EXIT_OK
        );
        assert!(!files(&dir.path().join("s")).is_empty());
    }

    // ---- review fixes ----

    /// `small()` with a sakkan (correction) layer on its drawing: a red
    /// block at 30..40 × 20..30, clear of the line's 8..20 × 4..12.
    fn small_with_sakkan() -> AppState {
        let mut s = small();
        let at = s.at();
        let d = s.cut().drawings[0].id;
        let l = s.engine.alloc_layer_id();
        let mut px = vec![0u16; TILE_LEN];
        for y in 20..30 {
            for x in 30..40 {
                for c in 0..4 {
                    px[(y * TILE + x) * 4 + c] = f32_to_f16_bits([1.0, 0.0, 0.0, 1.0][c]);
                }
            }
        }
        s.engine
            .apply(
                "sakkan",
                vec![
                    Command::AddCelLayer {
                        at,
                        drawing: d,
                        index: 1,
                        layer: CelLayer::with_role(l, "sakkan", LayerRole::Correction),
                    },
                    Command::PaintTiles {
                        at,
                        id: d,
                        layer: l,
                        diff: vec![((0, 0), None, Some(Arc::new(TileData::from_vec(px))))],
                    },
                ],
            )
            .expect("the sakkan layer");
        s
    }

    #[test]
    fn render_queue_a_whole_cut_job_renders_the_cut_as_it_is_at_start() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = small();
        s.render_queue.ffmpeg = Some((String::new(), None));
        s.render_queue.draft.out = dir.path().display().to_string();
        // "+ frames" with its fields untouched is the whole cut too.
        assert_eq!(Draft::default().range, None);
        let at = s.at();
        assert!(add_job(&mut s, at, None));
        assert_eq!(s.render_queue.queue.jobs[0].frames, None);
        // Lengthened after it was queued: the job renders the new length…
        s.engine
            .apply("len", vec![Command::SetFrameCount { at, frames: 5 }])
            .unwrap();
        let ctx = egui::Context::default();
        start(&mut s, &ctx);
        assert_eq!(s.render_queue.queue.jobs[0].span, (0, 4));
        // …as the cut was at Start: edits after it (longer still, frame 1
        // emptied) stay out.
        let column = s.view.active_column;
        s.engine
            .apply(
                "edit",
                vec![
                    Command::SetFrameCount { at, frames: 7 },
                    Command::SetCell {
                        at,
                        column,
                        frame: 0,
                        key: None,
                    },
                ],
            )
            .unwrap();
        wait(&mut s, &ctx);
        let j = &s.render_queue.queue.jobs[0];
        assert_eq!((j.state.clone(), j.done, j.total()), (JobState::Done, 5, 5));
        let names = files(dir.path());
        assert_eq!(names.len(), 5, "{names:?}");
        let (_, _, px) = png16(&dir.path().join(&names[0]), 10, 6);
        assert_eq!(px, [0, 0, 32768, 65535], "frame 1's drawing, as it was at Start");
        // An empty output folder means the configured default, else
        // `render` beside the project file — the field's hint says which.
        let mut rq = RenderQueue::default();
        assert_eq!(rq.out_hint(None), "render/ beside the project (save it first)");
        let file = Path::new("C:/show/ep1.animproj");
        assert_eq!(
            rq.out_hint(Some(file)),
            Path::new("C:/show").join("render").display().to_string()
        );
        rq.config.out_dir = "D:/deliveries".into();
        assert_eq!(rq.out_hint(Some(file)), "D:/deliveries");
    }

    #[test]
    fn render_queue_is_a_window_verb_with_chords_and_opens_from_any_room() {
        use crate::config::{Ctx, Group, Keymap, clashes, default_binds};
        let a = Action::ShowRenderQueue;
        assert_eq!(a.menu(), Some("Window/Render Queue (delivery)"));
        assert_eq!((a.group(), a.context()), (Group::Window, Ctx::Global));
        assert!(a.help().contains("render queue") && a.help().len() <= 120);
        for word in [
            "render",
            "export",
            "deliver",
            "output",
            "line test",
            "prores",
            "satsuei out",
        ] {
            assert!(a.aliases().contains(&word), "{word}");
            assert!(crate::cmdpal::score(word, a).is_some(), "{word}");
        }
        assert_eq!(crate::cmdpal::ranked("render queue", &[]).first(), Some(&a));
        // One chord in both keymaps (never R, the flip), clashing with none;
        // the final look sits on Shift with the composite view's C.
        for &km in Keymap::ALL {
            let binds = default_binds(km);
            let chord = crate::cmdpal::chord_of(&binds, a);
            assert_eq!(chord.as_deref(), Some("Ctrl+Shift+E"), "{km:?}");
            let fin = crate::cmdpal::chord_of(&binds, Action::ToggleFinalLook);
            assert_eq!(fin.as_deref(), Some("Shift+C"), "{km:?}");
            assert!(clashes(&binds).is_empty(), "{km:?}: {:?}", clashes(&binds));
        }
        // The drawing room has no queue: the verb brings one, forward.
        let mut h = EditorHarness::new();
        h.perform(Action::RoomDrawing);
        h.run();
        assert!(h.editor().dock.find_tab(&Pane::RenderQueue).is_none());
        h.perform(a);
        h.run();
        assert!(h.editor().dock.find_tab(&Pane::RenderQueue).is_some());
        assert!(h.has_text_containing("RENDER QUEUE"), "{}", h.describe());
        // The edit room's queue sits behind the node graph; the chord
        // brings it forward.
        h.perform(Action::RoomEdit);
        h.run();
        assert!(!h.has_text_containing("RENDER QUEUE"), "{}", h.describe());
        let ctrl_shift = egui::Modifiers {
            shift: true,
            ..crate::testkit::CTRL
        };
        h.key_mods(ctrl_shift, egui::Key::E);
        assert_eq!(h.performed().last().map(|p| p.0), Some(a));
        assert!(h.has_text_containing("RENDER QUEUE"), "{}", h.describe());
    }

    #[test]
    fn render_queue_final_look_hides_the_sakkan_layer_in_the_viewer() {
        let mut h = EditorHarness::with_state(small_with_sakkan());
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Viewer(0)]);
        h.run();
        assert_eq!(Action::ToggleFinalLook.menu(), Some("View/Final look (delivery)"));
        assert_eq!(h.editor().is_on(Action::ToggleFinalLook), Some(false));
        assert_eq!(h.node_toggled("final look"), Some(false), "{}", h.describe());
        // The viewer's latch turns it on: the frame renders on a worker
        // (no GPU here — the look is the CPU's).
        h.click("final look");
        assert!(h.editor().final_look.on);
        h.run_frames(2);
        h.editor_mut().final_look.wait();
        h.run();
        assert_eq!(h.node_toggled("final look"), Some(true));
        let img = h.editor().final_look.image.clone().expect("a final-look frame");
        assert_eq!(img.size, [64, 36]);
        let alpha = |x: usize, y: usize| img.pixels[y * 64 + x].a();
        assert_eq!(alpha(10, 6), 255, "the line is delivered");
        assert_eq!(alpha(35, 25), 0, "the sakkan layer is not");
        // The composite view's look of the same frame has it: the toggle
        // is what hid it.
        let s = h.state();
        let law = s.engine.project.color_law;
        let view = deliver::render(s.cut(), law, 0, 64, 36, &deliver::RenderOpts::default());
        assert!(view.pixel(35, 25)[3] > 0.99);
        // The verb (View ▸ Final look, Shift+C) turns it off again.
        h.perform(Action::ToggleFinalLook);
        h.run();
        assert_eq!(h.editor().is_on(Action::ToggleFinalLook), Some(false));
        assert_eq!(h.node_toggled("final look"), Some(false));
    }

    #[test]
    fn render_queue_file_export_is_a_queue_job_that_starts_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = EditorHarness::with_state(small());
        h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
        h.perform(Action::RoomDrawing);
        h.run();
        // The menu: the queue itself first; the MP4 greyed without ffmpeg.
        h.click("file");
        h.hover("Export… ⏵");
        assert_eq!(h.node_enabled("PNG sequence…"), Some(true), "{}", h.describe());
        assert_eq!(h.node_enabled("MP4 video…"), Some(false));
        h.click("open the render queue (all presets)");
        assert!(h.editor().dock.find_tab(&Pane::RenderQueue).is_some());
        // PNG sequence…, past its folder dialog: a PNG 8-bit job over the
        // whole cut, the queue forward and started.
        let ctx = h.ctx();
        h.editor_mut()
            .queue_export(&ctx, ExportKind::Png, dir.path());
        let j = h.state().render_queue.queue.jobs[0].clone();
        assert_eq!(
            (j.preset().name, j.frames, j.burn_ins),
            ("PNG 8-bit", None, BurnIns::NONE)
        );
        assert_eq!(j.out, dir.path());
        assert!(h.state().render_queue.busy(), "started at once");
        assert!(h.editor().quick_start[crate::welcome::EXPORT_STEP]);
        wait(h.state_mut(), &ctx);
        h.run();
        assert_eq!(h.state().render_queue.queue.jobs[0].state, JobState::Done);
        assert_eq!(files(dir.path()).len(), 3);
        assert!(h.has_text_containing("RENDER QUEUE"), "{}", h.describe());
        // MP4 video…: H.264 review into the file's folder, named by it,
        // over the menu's range.
        let mp4 = dir.path().join("take2.mp4");
        let i = export_job(h.state_mut(), ExportKind::Mp4, &mp4, Some((1, 2))).unwrap();
        let j = &h.state().render_queue.queue.jobs[i];
        assert_eq!(
            (j.preset().name, j.frames, j.span),
            ("H.264 review", Some((1, 2)), (1, 2))
        );
        assert_eq!((j.out.as_path(), j.pattern.as_str()), (dir.path(), "take2"));
        assert!(j.label.ends_with("2–3"), "{}", j.label);
    }

    #[test]
    fn render_queue_selected_cuts_queue_in_the_projects_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = small();
        let first = s.at();
        s.new_cut(first.scene);
        s.new_cut(first.scene);
        s.goto_cut(first.scene, first.cut);
        let cuts: Vec<CutRef> = s.engine.project.scenes[0]
            .cuts
            .iter()
            .map(|c| CutRef {
                scene: first.scene,
                cut: c.id,
            })
            .collect();
        assert_eq!(cuts.len(), 3);
        let mut h = EditorHarness::with_state(s);
        h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
        h.state_mut().render_queue.draft.out = dir.path().display().to_string();
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::RenderQueue]);
        h.run();
        assert_eq!(h.node_enabled("+ selected cuts"), Some(false), "nothing picked");
        h.click("pick cuts");
        h.click("CUT03");
        h.click("CUT01");
        assert_eq!(h.state().render_queue.draft.picked, vec![cuts[2], cuts[0]]);
        h.click("+ selected cuts");
        let queued: Vec<CutRef> = h
            .state()
            .render_queue
            .queue
            .jobs
            .iter()
            .map(|j| j.at)
            .collect();
        assert_eq!(queued, vec![cuts[0], cuts[2]], "the project's order");
        // Its latch again drops a cut from the pick.
        h.click("CUT03");
        assert_eq!(h.state().render_queue.draft.picked, vec![cuts[0]]);
    }

    #[test]
    fn render_queue_line_test_png_proxies_and_frameless_patterns_need_no_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = small();
        // The line test as frames: black line on white, no ffmpeg.
        let p = Preset::named("Line test PNG").unwrap();
        assert!(!p.format.is_video());
        let sp = JobSpec::new(
            &s.engine.project,
            s.at(),
            None,
            p,
            BurnIns::NONE,
            dir.path().join("lt"),
            deliver::DEFAULT_PATTERN.into(),
            "mika".into(),
        )
        .unwrap();
        let img = sp.frame(0);
        let (ink, paper) = (img.pixel(10, 6), img.pixel(40, 30));
        assert!(ink[..3].iter().all(|c| *c < 0.01) && ink[3] > 0.99, "{ink:?}");
        assert!(paper.iter().all(|c| *c > 0.99), "{paper:?}");
        let none = AtomicBool::new(false);
        assert_eq!(render_job(&sp, None, &none, &mut |_| {}), Ok(3));
        assert_eq!(files(&dir.path().join("lt")).len(), 3);
        // A pattern without a frame number still writes a file a frame, and
        // the proxy latch halves the frame.
        s.render_queue.ffmpeg = Some((String::new(), None));
        s.render_queue.draft.out = dir.path().join("half").display().to_string();
        s.render_queue.draft.pattern = "{cut}".into();
        s.render_queue.draft.half = true;
        let at = s.at();
        assert!(add_job(&mut s, at, None));
        let ctx = egui::Context::default();
        start(&mut s, &ctx);
        wait(&mut s, &ctx);
        let names = files(&dir.path().join("half"));
        assert_eq!(
            names,
            vec!["CUT01_0001.png", "CUT01_0002.png", "CUT01_0003.png"]
        );
        let (size, _, _) = png16(&dir.path().join("half").join(&names[0]), 0, 0);
        assert_eq!(size, (32, 18), "half size");
    }

    #[test]
    fn render_queue_mixdown_wav_latch_writes_the_sound_beside_a_sequence_or_not() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = small();
        let at = s.at();
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../crates/anim-core/tests/fixtures/tone.wav"
        ))
        .unwrap();
        let clip = anim_core::model::AudioClip::from_bytes("tone.wav", bytes).unwrap();
        s.engine
            .apply("sound", vec![Command::SetCutAudio { at, audio: Some(clip) }])
            .expect("the sound");
        assert!(s.render_queue.draft.mixdown, "the latch starts on");
        s.render_queue.ffmpeg = Some((String::new(), None));
        s.render_queue.draft.preset = PRESETS.iter().position(|p| p.name == "PNG 8-bit").unwrap();
        s.render_queue.draft.burn_ins = BurnIns::NONE;
        s.render_queue.draft.out = dir.path().join("on").display().to_string();
        assert!(add_job(&mut s, at, None));
        s.render_queue.draft.mixdown = false;
        s.render_queue.draft.out = dir.path().join("off").display().to_string();
        assert!(add_job(&mut s, at, None));
        assert_eq!(
            s.render_queue.queue.jobs.iter().map(|j| j.mixdown).collect::<Vec<_>>(),
            vec![true, false],
            "each job keeps the latch as it was added"
        );
        let ctx = egui::Context::default();
        start(&mut s, &ctx);
        wait(&mut s, &ctx);
        let wavs = |d: &str| {
            files(&dir.path().join(d))
                .into_iter()
                .filter(|n| n.ends_with(".wav"))
                .count()
        };
        assert_eq!(files(&dir.path().join("on")).len(), 4, "three frames and the WAV");
        assert_eq!(wavs("on"), 1);
        assert_eq!(files(&dir.path().join("off")).len(), 3, "the frames alone");
        assert_eq!(wavs("off"), 0);
        // Named as the range's video would be: no separator left dangling
        // where the frame token was. As long as the range: 3 frames at 24
        // fps, 48 kHz stereo, the tone in it.
        let wav = dir.path().join("on").join("show_SC01_CUT01.wav");
        let bytes = std::fs::read(&wav).expect("the WAV under the pattern's name");
        let mix = anim_core::model::AudioClip::from_bytes("mix.wav", bytes).unwrap();
        assert_eq!((mix.sample_rate, mix.channels), (anim_core::audio::MIX_RATE, 2));
        assert_eq!(mix.samples.len(), 3 * 48_000 / 24 * 2, "three frames of sound");
        assert!(mix.samples.iter().any(|v| v.abs() > 0.1), "the tone, not silence");
    }

    #[test]
    fn render_queue_cli_renders_the_cut_it_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = small();
        let first = s.at();
        s.new_cut(first.scene);
        let second = s.at();
        s.engine
            .apply(
                "len",
                vec![Command::SetFrameCount {
                    at: second,
                    frames: 2,
                }],
            )
            .unwrap();
        let file = dir.path().join("two.animproj");
        anim_core::store::save(&s.engine.project, &file).unwrap();
        let args = |cut: &str, out: &str| -> Vec<String> {
            [
                "--render",
                &file.display().to_string(),
                "--preset",
                "PNG 8-bit",
                "--out",
                &dir.path().join(out).display().to_string(),
                "--cut",
                cut,
            ]
            .iter()
            .map(|s| s.to_string())
            .collect()
        };
        let data = tempfile::tempdir().unwrap();
        // Names ignore case; no name is the first cut.
        let job = deliver::parse_cli(&args("cut02", "b")).unwrap();
        assert_eq!(crate::run_cli_job(&job, Some(data.path())), Ok(2), "CUT02's frames");
        let unnamed = deliver::CliJob {
            cut: None,
            ..job.clone()
        };
        assert_eq!(crate::run_cli_job(&unnamed, Some(data.path())), Ok(3), "CUT01's");
        // A cut that is not there fails the job, saying what is.
        let missing = deliver::parse_cli(&args("CUT09", "c")).unwrap();
        let err = crate::run_cli_job(&missing, Some(data.path())).unwrap_err();
        assert!(err.contains("no cut named \"CUT09\"") && err.contains("CUT01, CUT02"), "{err}");
        assert_eq!(
            crate::cli_exit(&args("CUT09", "c"), Some(data.path())),
            deliver::EXIT_FAILED
        );
    }
}
