//! NEVER LOSE WORK (GM2): the parts of the safety net that need no window.
//! The autosave scheduler (a pure clock the editor feeds — no wall clock
//! inside), where autosaves live and how many are kept, the running records
//! (one per running copy of the app) that tell the next start-up a session
//! did not end cleanly, the recovery dialog's body, and the crash log.
//! Every path takes the data dir (D11); the real one is resolved in main()
//! only. Nothing here writes the user's file: autosaves, running records
//! and crash logs all live in the data folder (NEVER-DO 5).

use std::path::{Path, PathBuf};

use anim_core::model::Project;
use eframe::egui;

use crate::config::SessionConfig;
use crate::doc::AppState;
use crate::plate;

/// The intervals Settings ▸ General offers, in minutes (0 = off).
pub const AUTOSAVE_CHOICES: [u32; 6] = [0, 1, 2, 5, 10, 15];
/// D13: autosave every 5 minutes unless the artist picks otherwise.
pub const AUTOSAVE_DEFAULT_MIN: u32 = 5;
/// D13: the newest autosaves kept per project.
pub const AUTOSAVE_KEEP: usize = 3;
/// The pen must have been up this long before an autosave starts.
pub const IDLE_S: f64 = 2.0;

/// When the next autosave is due. Pure: the caller passes the time (any
/// monotonic seconds — the editor passes egui's clock), so tests drive it
/// without sleeping.
pub struct AutosaveClock {
    /// The interval in minutes (0 = off); the shell keeps it in step with
    /// the config.
    pub minutes: u32,
    /// When the interval last restarted (an autosave, or the document
    /// last being clean).
    since: Option<f64>,
    /// When the pointer last came up (None while it is down).
    up_since: Option<f64>,
}

impl AutosaveClock {
    pub fn new(minutes: u32) -> Self {
        Self {
            minutes,
            since: None,
            up_since: None,
        }
    }

    /// One frame: true when an autosave should start now — the document is
    /// dirty, no stroke is in progress, the pointer has been up for
    /// `IDLE_S`, and the interval has run since the last one (or since the
    /// document was last clean). Never when off.
    pub fn tick(&mut self, now: f64, dirty: bool, stroke: bool, pointer_down: bool) -> bool {
        if pointer_down || stroke {
            self.up_since = None;
        } else if self.up_since.is_none() {
            self.up_since = Some(now);
        }
        if !dirty || self.since.is_none() {
            // Clean work needs no copy; the interval starts at the first
            // unsaved change.
            self.since = Some(now);
            return false;
        }
        if self.minutes == 0 || pointer_down || stroke {
            return false;
        }
        let idle = self.up_since.is_some_and(|t| now - t >= IDLE_S);
        let due = self
            .since
            .is_some_and(|t| now - t >= f64::from(self.minutes) * 60.0);
        if idle && due {
            self.since = Some(now);
            return true;
        }
        false
    }
}

/// Seconds since the Unix epoch (the shell's stamps; never read in tests).
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 32-bit FNV-1a as 8 hex digits: the document's identity in a file name.
fn hash8(s: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{h:08x}")
}

/// A name made file-safe for an autosave group (at most 40 characters).
fn file_safe(stem: &str) -> String {
    let safe: String = stem
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .take(40)
        .collect();
    if safe.is_empty() { "untitled".into() } else { safe }
}

/// A fresh autosave group for an untitled document: `<name>-<8 hex>` from
/// std's per-process random hash keys (every call differs, and no clock is
/// read), so two untitled projects that share a name — in one session or
/// in two copies of the app — never share or prune each other's autosaves.
pub fn untitled_group(name: &str) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write(name.as_bytes());
    format!("{}-{:08x}", file_safe(name), h.finish() as u32)
}

/// `<stem>-<hash8>`: one project's autosave group. A file's group is its
/// stem made file-safe plus a hash of its path (two `cut01.animproj` in
/// different folders stay apart); an untitled document brings its own
/// (`AppState::untitled_group`).
pub fn autosave_group(file: Option<&Path>, untitled: &str) -> String {
    match file {
        Some(p) => {
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("{}-{}", file_safe(&stem), hash8(&p.to_string_lossy().to_lowercase()))
        }
        None => untitled.to_string(),
    }
}

/// Where autosave number `seq` of a group goes:
/// `<dir>/autosave/<group>-<seq>.animproj`.
pub fn autosave_path_in(dir: &Path, group: &str, seq: u64) -> PathBuf {
    crate::config::autosave_dir_in(dir).join(format!("{group}-{seq}.animproj"))
}

/// The autosave group a path belongs to (None: not an autosave name).
pub fn group_of(path: &Path) -> Option<String> {
    parse_autosave(path).map(|(g, _)| g)
}

/// (group, seq) of an autosave file name, or None for anything else.
fn parse_autosave(path: &Path) -> Option<(String, u64)> {
    if path.extension()? != "animproj" {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let (group, seq) = stem.rsplit_once('-')?;
    Some((group.to_string(), seq.parse().ok()?))
}

/// Every autosave in the data dir: (path, group, seq).
fn list_autosaves(dir: &Path) -> Vec<(PathBuf, String, u64)> {
    let Ok(rd) = std::fs::read_dir(crate::config::autosave_dir_in(dir)) else {
        return Vec::new();
    };
    rd.flatten()
        .map(|e| e.path())
        .filter_map(|p| parse_autosave(&p).map(|(g, s)| (p, g, s)))
        .collect()
}

/// Keep the newest `keep` autosaves of every project; returns how many
/// older ones were removed.
pub fn prune_autosaves(dir: &Path, keep: usize) -> usize {
    let mut all = list_autosaves(dir);
    all.sort_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)));
    let mut removed = 0;
    let mut seen: Option<(&str, usize)> = None;
    for (path, group, _) in &all {
        let n = match seen {
            Some((g, n)) if g == group => n + 1,
            _ => 1,
        };
        seen = Some((group, n));
        if n > keep && std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Write one autosave of `project` (the document at `file`; None =
/// untitled) into the data dir as `group`'s number `seq`, then prune to the
/// newest `AUTOSAVE_KEEP`. The user's file is only ever READ here (its
/// unknown tables carry over); no `.bak` is made. Runs on a worker thread.
pub fn write_autosave(
    dir: &Path,
    project: &Project,
    file: Option<&Path>,
    group: &str,
    seq: u64,
) -> Result<PathBuf, String> {
    let path = autosave_path_in(dir, group, seq);
    std::fs::create_dir_all(crate::config::autosave_dir_in(dir)).map_err(|e| e.to_string())?;
    anim_core::store::save_atomic_from(project, &path, file, false).map_err(|e| e.to_string())?;
    prune_autosaves(dir, AUTOSAVE_KEEP);
    Ok(path)
}

/// A save landed: every autosave of `group` is older than the file now, so
/// none of them could ever be offered — remove them rather than leave them
/// in the data folder for good (an untitled group would never be pruned
/// again once its document has a file).
pub fn drop_group(dir: &Path, group: &str) {
    for (path, g, _) in list_autosaves(dir) {
        if g == group {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Delete these autosaves (the recovery dialog's Discard) — only files
/// inside the data dir's autosave folder, never anything else.
pub fn discard_autosaves(dir: &Path, paths: &[PathBuf]) {
    let folder = crate::config::autosave_dir_in(dir);
    for p in paths {
        if p.parent() == Some(folder.as_path()) && parse_autosave(p).is_some() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// `<data>/running-<pid>-<started>.json`: one per running copy of the app
/// (artists run two — a reference cut and the working cut), written at
/// start-up and removed by its own copy on a clean exit. A record whose
/// process is gone means that session did not end cleanly.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunningRecord {
    #[serde(default)]
    pub pid: u32,
    /// The document that was open (None = untitled).
    #[serde(default)]
    pub open: Option<PathBuf>,
    /// The newest autosave that session wrote.
    #[serde(default)]
    pub autosave: Option<PathBuf>,
    /// When it started (Unix seconds).
    #[serde(default)]
    pub started: u64,
}

/// The record file of the copy with this pid and start stamp.
pub fn running_path_in(dir: &Path, pid: u32, started: u64) -> PathBuf {
    dir.join(format!("running-{pid}-{started}.json"))
}

impl RunningRecord {
    /// This copy's own record file (a crashed copy's pid may be reused, so
    /// the start stamp is in the name too: a new copy never overwrites it).
    pub fn path_in(&self, dir: &Path) -> PathBuf {
        running_path_in(dir, self.pid, self.started)
    }

    pub fn write_in(&self, dir: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::config::write_atomic(&self.path_in(dir), &bytes)
    }

    pub fn read(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Remove this copy's own record — never another copy's.
    pub fn remove_in(&self, dir: &Path) {
        let _ = std::fs::remove_file(self.path_in(dir));
    }
}

/// Every running record in the data dir, oldest start first, with its
/// file (the first GM2 build's single `running.json` included).
pub fn running_records(dir: &Path) -> Vec<(PathBuf, RunningRecord)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut all: Vec<(PathBuf, RunningRecord)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n == "running.json" || (n.starts_with("running-") && n.ends_with(".json"))
            })
        })
        .filter_map(|p| RunningRecord::read(&p).map(|r| (p, r)))
        .collect();
    all.sort_by_key(|(_, r)| r.started);
    all
}

/// Whether the copy that wrote `rec` is still running: its pid is a live
/// process that was created no later than the record was written. Windows
/// reuses pids, so a younger process under the same number is someone else.
#[cfg(windows)]
pub fn process_alive(rec: &RunningRecord) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, FILETIME, GetLastError, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: plain Win32 queries on a pid with out-pointers to locals; the
    // handle is closed before returning.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, rec.pid);
        if h.is_null() {
            // Gone — or a process this user may not inspect, which exists:
            // never offer the work of a session that may be live.
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        let mut code = 0u32;
        let running = GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE as u32;
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        let timed = GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user) != 0;
        CloseHandle(h);
        if !running || !timed {
            return running;
        }
        // FILETIME counts 100 ns ticks from 1601; Unix seconds from 1970.
        let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        let created_unix = (ticks / 10_000_000).saturating_sub(11_644_473_600);
        created_unix <= rec.started.saturating_add(1)
    }
}

#[cfg(not(windows))]
pub fn process_alive(rec: &RunningRecord) -> bool {
    Path::new(&format!("/proc/{}", rec.pid)).exists()
}

/// One autosave a recovery offer lists.
#[derive(Debug, Clone, PartialEq)]
pub struct Autosave {
    pub path: PathBuf,
    /// When it was written (Unix seconds — the file's modified time).
    pub at: u64,
}

/// A copy of the app that did not close cleanly, and what it left.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovery {
    /// Its running record's file: removed once its work is open again or
    /// discarded; "Later" leaves it, so the next start offers again.
    pub record_path: PathBuf,
    pub record: RunningRecord,
    /// Its document's autosaves newer than the document on disk (all of
    /// them for an untitled or missing document), newest first.
    pub autosaves: Vec<Autosave>,
    /// The row the dialog has picked (0 = the newest).
    pub selected: usize,
}

impl Recovery {
    /// The selected autosave, as the work to open.
    pub fn picked(&self) -> Option<Recovered> {
        let a = self.autosaves.get(self.selected)?;
        Some(Recovered {
            autosave: a.path.clone(),
            origin: self.record.open.clone(),
            record: self.record_path.clone(),
        })
    }
}

fn modified_unix(path: &Path) -> Option<u64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs())
}

/// Every copy whose record is here but whose process is gone (`alive` says
/// which are running — `process_alive` in the app, a stub in tests), with
/// what it left; possibly nothing, when its document had no newer
/// autosave. Oldest crash first.
pub fn dead_sessions(dir: &Path, alive: &dyn Fn(&RunningRecord) -> bool) -> Vec<Recovery> {
    running_records(dir)
        .into_iter()
        .filter(|(_, rec)| !alive(rec))
        .map(|(record_path, rec)| {
            let group = match (&rec.autosave, &rec.open) {
                (Some(a), _) => group_of(a),
                (None, Some(open)) => Some(autosave_group(Some(open), "")),
                (None, None) => None,
            };
            let doc_time = rec
                .open
                .as_ref()
                .and_then(|p| std::fs::metadata(p).ok())
                .and_then(|m| m.modified().ok());
            let mut autosaves: Vec<(Autosave, u64)> = list_autosaves(dir)
                .into_iter()
                .filter(|(_, g, _)| Some(g) == group.as_ref())
                .filter(|(p, _, _)| match doc_time {
                    None => true,
                    Some(doc) => std::fs::metadata(p)
                        .and_then(|m| m.modified())
                        .is_ok_and(|t| t > doc),
                })
                .map(|(p, _, s)| {
                    let at = modified_unix(&p).unwrap_or(0);
                    (Autosave { path: p, at }, s)
                })
                .collect();
            autosaves.sort_by(|a, b| b.1.cmp(&a.1));
            Recovery {
                record_path,
                record: rec,
                autosaves: autosaves.into_iter().map(|(a, _)| a).collect(),
                selected: 0,
            }
        })
        .collect()
}

/// What crashed sessions left worth offering: the dead sessions with at
/// least one autosave newer than their document. No dead record → nothing.
/// (The shell walks `dead_sessions` itself, to sweep the empty ones too.)
#[cfg_attr(not(test), allow(dead_code))]
pub fn recovery_candidates(dir: &Path, alive: &dyn Fn(&RunningRecord) -> bool) -> Vec<Recovery> {
    dead_sessions(dir, alive)
        .into_iter()
        .filter(|r| !r.autosaves.is_empty())
        .collect()
}

/// The recovery dialog's Discard: delete the offered autosaves and the
/// dead session's record.
pub fn discard_recovery(dir: &Path, offer: &Recovery) {
    let paths: Vec<PathBuf> = offer.autosaves.iter().map(|a| a.path.clone()).collect();
    discard_autosaves(dir, &paths);
    let _ = std::fs::remove_file(&offer.record_path);
}

/// The work "Open recovered" brings back: an autosave, the file it belongs
/// to (None = untitled), and the dead session's record (removed once the
/// work is open — not before, so a cancelled open is offered again).
#[derive(Debug, Clone, PartialEq)]
pub struct Recovered {
    pub autosave: PathBuf,
    pub origin: Option<PathBuf>,
    pub record: PathBuf,
}

/// The recovered work as a document: the autosave's content, belonging to
/// the file it was autosaved from (or untitled, still in its own autosave
/// group), and NOT on disk there yet — it reads dirty until saved.
pub fn open_recovered(r: &Recovered) -> Result<AppState, String> {
    let mut state = AppState::load_from(r.autosave.clone())?;
    state.file_path = r.origin.clone();
    if let Some(g) = group_of(&r.autosave).filter(|_| r.origin.is_none()) {
        state.untitled_group = g;
    }
    state.meta_dirty = true;
    state.status = "recovered from autosave — save to keep it".into();
    Ok(state)
}

/// The recovery dialog's answer.
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryChoice {
    /// Open the selected autosave.
    Open(Recovered),
    Discard,
    Later,
}

/// `HH:MM` local time of a Unix second (`utc_offset_min`: local − UTC).
pub fn clock_hm(unix: u64, utc_offset_min: i32) -> String {
    let local = unix as i64 + i64::from(utc_offset_min) * 60;
    let day = local.rem_euclid(86_400);
    format!("{:02}:{:02}", day / 3600, day % 3600 / 60)
}

/// How long before `now` something happened, in plain words.
pub fn age_text(now: u64, at: u64) -> String {
    let s = now.saturating_sub(at);
    match s {
        0..60 => "under a minute ago".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..172_800 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86_400),
    }
}

/// The local time zone's offset from UTC, in minutes, as it is now.
#[cfg(windows)]
pub fn utc_offset_min() -> i32 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    // SAFETY: GetTimeZoneInformation fills the zeroed plain-data struct.
    unsafe {
        let mut tz: TIME_ZONE_INFORMATION = std::mem::zeroed();
        let id = GetTimeZoneInformation(&mut tz);
        // 2 = TIME_ZONE_ID_DAYLIGHT. Bias is UTC − local, in minutes.
        let extra = if id == 2 { tz.DaylightBias } else { tz.StandardBias };
        -(tz.Bias + extra)
    }
}

#[cfg(not(windows))]
pub fn utc_offset_min() -> i32 {
    0
}

/// The recovery dialog's body: which document the crashed copy had open,
/// one row per autosave (local time and age, the newest preselected), then
/// Open recovered / Discard (a held DANGER) / Later. `now` (Unix seconds)
/// and the UTC offset are passed in, so tests drive it with no clock.
/// Escape is Later.
pub fn recovery_ui(
    ui: &mut egui::Ui,
    offer: &mut Recovery,
    now: u64,
    utc_offset_min: i32,
) -> Option<RecoveryChoice> {
    let mut choice = None;
    plate::legend(ui, "recovery");
    ui.label("Recover unsaved work?");
    let (what, rows_are) = match &offer.record.open {
        Some(p) => (
            p.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
            "its autosaves newer than the file",
        ),
        // Never saved: there is no file to compare with — every autosave.
        None => ("an untitled project".to_string(), "its autosaves"),
    };
    ui.label(
        egui::RichText::new(format!(
            "Gaban Studio did not close cleanly while {what} was open — {rows_are}:"
        ))
        .color(plate::legend_dim()),
    );
    if let Some(p) = &offer.record.open {
        ui.label(egui::RichText::new(p.display().to_string()).color(plate::legend_dim()));
    }
    ui.add_space(4.0);
    for (i, a) in offer.autosaves.iter().enumerate() {
        let mut row = format!("{} · {}", clock_hm(a.at, utc_offset_min), age_text(now, a.at));
        if i == 0 {
            row.push_str(" (newest)");
        }
        if plate::detent(ui, offer.selected == i, &row)
            .on_hover_text(a.path.display().to_string())
            .clicked()
        {
            offer.selected = i;
        }
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Open recovered").clicked() {
            choice = offer.picked().map(RecoveryChoice::Open);
        }
        if plate::danger_hint(ui, "Discard", "deletes these autosaves for good") {
            choice = Some(RecoveryChoice::Discard);
        }
        if ui.button("Later").clicked() {
            choice = Some(RecoveryChoice::Later);
        }
    });
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        choice = Some(RecoveryChoice::Later);
    }
    choice
}

/// The recent list follows the open document: a path not seen last frame
/// (an Open, a Save As, a recovered file) goes to the front. True when the
/// list changed (the caller saves the config).
pub fn follow_recent(
    seen: &mut Option<PathBuf>,
    open: Option<&Path>,
    session: &mut SessionConfig,
) -> bool {
    if seen.as_deref() == open {
        return false;
    }
    *seen = open.map(Path::to_path_buf);
    match open {
        Some(p) => {
            session.push_recent(p);
            true
        }
        None => false,
    }
}

/// Each path with its "still on disk" flag (the Open Recent menu checks
/// them when it opens, not every frame).
pub fn on_disk(paths: &[PathBuf]) -> Vec<(PathBuf, bool)> {
    paths.iter().map(|p| (p.clone(), p.is_file())).collect()
}

/// One panic, as the crash log records it.
#[derive(Debug, Clone, PartialEq)]
pub struct CrashInfo {
    /// Unix seconds.
    pub time: u64,
    pub message: String,
    /// `file:line:col`, or "unknown".
    pub location: String,
}

impl CrashInfo {
    pub fn new(
        time: u64,
        payload: &(dyn std::any::Any + Send),
        location: Option<&std::panic::Location<'_>>,
    ) -> Self {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(non-text panic payload)".into());
        let location = location
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        Self {
            time,
            message,
            location,
        }
    }
}

/// The crash log's entry for one panic.
pub fn crash_report(info: &CrashInfo) -> String {
    format!(
        "[{}] Gaban Studio {} panicked: {}\n  at {}\n",
        info.time,
        env!("CARGO_PKG_VERSION"),
        info.message,
        info.location
    )
}

/// Append `info` to `<dir>/crash.log`.
pub fn write_crash_log(dir: &Path, info: &CrashInfo) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("crash.log"))?;
    f.write_all(crash_report(info).as_bytes())
}

/// main() only: every panic also lands in `<dir>/crash.log` (the copy's
/// running record stays behind, so the next start offers recovery). The
/// previous hook still runs.
pub fn install_panic_hook(dir: PathBuf) {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let c = CrashInfo::new(unix_now(), info.payload(), info.location());
        let _ = write_crash_log(&dir, &c);
        prev(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    use crate::canvas::CanvasView;
    use crate::doc::AppState;
    use crate::testkit::{EditorHarness, PanelHarness};
    use crate::{AppRequest, window_title};

    fn doc() -> AppState {
        AppState::new_project("ws", 64, 48, 24, 72.0)
    }

    /// Set a file's modified time (recovery compares autosaves to the doc).
    fn touch(path: &Path, secs: u64) {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    #[test]
    fn worksafe_dirty_follows_revision() {
        let mut s = doc();
        assert!(!s.is_dirty(), "a new project is clean");
        s.new_drawing_at_frame();
        assert!(s.is_dirty(), "one gesture makes it dirty");
        s.undo();
        assert!(!s.is_dirty(), "undo back to the saved state is clean");
        s.redo();
        assert!(s.is_dirty(), "redo is dirty again");
        let dir = tempfile::tempdir().unwrap();
        s.begin_save(dir.path().join("a.animproj"));
        assert!(s.is_dirty(), "not clean until the save lands");
        assert!(s.wait_save());
        assert!(!s.is_dirty(), "save makes it clean");
        s.undo();
        assert!(s.is_dirty(), "undo past the saved state is dirty");
        s.redo();
        assert!(!s.is_dirty());
        // Scaffolding lives outside undo but is still unsaved work.
        s.add_column();
        assert!(s.is_dirty(), "a new column is unsaved work");
    }

    #[test]
    fn worksafe_palette_edit_sets_dirty() {
        let mut s = doc();
        s.palettes
            .characters
            .push(crate::palette::CharacterPalette::new("luffy"));
        s.palette_edit = true;
        assert!(!s.is_dirty());
        let mut p = PanelHarness::new(
            egui::vec2(320.0, 400.0),
            (s, CanvasView::new()),
            |ui, (st, cv): &mut (AppState, CanvasView)| crate::palette_panel::ui(ui, st, cv),
        );
        p.click(&format!("+ {}", crate::vocab::COLOR_NAME));
        assert!(p.state().0.meta_dirty && p.state().0.is_dirty(), "a palette edit is unsaved work");
        // The paint dish writes the palettes too: the row's held DERIVE and
        // the eye's held SET (they once bypassed the mark).
        let dish = |s: AppState, cv: CanvasView| {
            PanelHarness::new(
                egui::vec2(1200.0, 300.0),
                (s, cv),
                |ui, (st, cv): &mut (AppState, CanvasView)| {
                    ui.horizontal(|ui| cv.dish_ui(ui, st));
                },
            )
        };
        let mut s = doc();
        s.palettes
            .characters
            .push(crate::palette::CharacterPalette::new("zoro"));
        let mut p = dish(s, CanvasView::new());
        assert!(!p.state().0.is_dirty());
        p.hold("Make shadow");
        let role = |s: &AppState, name: &str| {
            s.palettes.characters[0]
                .roles
                .iter()
                .find(|r| r.name == name)
                .map(|r| r.color)
                .unwrap()
        };
        let st = &p.state().0;
        assert_eq!(
            role(st, "shadow"),
            crate::palette::derive_shadow(role(st, "normal")),
            "DERIVE fired"
        );
        assert!(st.meta_dirty && st.is_dirty(), "DERIVE is unsaved work");
        let mut s = doc();
        s.palettes
            .characters
            .push(crate::palette::CharacterPalette::new("zoro"));
        let before = s.palettes.characters[0].roles[0].color;
        let mut cv = CanvasView::new();
        cv.brush_color = [10, 20, 30, 255];
        cv.eye_target = Some((0, 0));
        let mut p = dish(s, cv);
        assert!(!p.state().0.is_dirty());
        p.hold("set");
        let (st, cv) = p.state();
        assert_eq!(st.palettes.characters[0].roles[0].color, cv.brush_color, "SET fired");
        assert_ne!(st.palettes.characters[0].roles[0].color, before);
        assert!(st.meta_dirty && st.is_dirty(), "SET is unsaved work");
    }

    #[test]
    fn worksafe_window_title_marks_dirty() {
        let mut s = doc();
        assert_eq!(window_title(&s), "ws — Gaban Studio");
        s.new_drawing_at_frame();
        assert!(window_title(&s).starts_with("● ws"), "{}", window_title(&s));
        s.undo();
        assert!(!window_title(&s).contains('●'));
        s.file_path = Some(PathBuf::from("C:/films/cut01.animproj"));
        s.meta_dirty = true;
        assert_eq!(window_title(&s), "● cut01 — Gaban Studio");
    }

    #[test]
    fn worksafe_close_guard_asks_first() {
        let mut h = EditorHarness::new();
        assert!(h.state().is_dirty(), "the testkit project has a gesture");
        let cmds = h.request_close();
        assert!(cmds.contains(&egui::ViewportCommand::CancelClose), "{cmds:?}");
        assert!(h.has("Save changes?"), "{}", h.describe());
        // Cancel: the modal goes, the window stays.
        h.click("Cancel");
        assert!(!h.has("Save changes?"));
        assert!(!h.editor().close_ok);
        // Don't Save (a held DANGER): the window closes.
        let cmds = h.request_close();
        assert!(cmds.contains(&egui::ViewportCommand::CancelClose));
        let cmds = h.hold("Don't Save");
        assert!(cmds.contains(&egui::ViewportCommand::Close), "{cmds:?}");
        assert!(h.editor().close_ok && !h.has("Save changes?"));
    }

    #[test]
    fn worksafe_clean_close_goes_through() {
        let mut h = EditorHarness::with_state(doc());
        let cmds = h.request_close();
        assert!(!cmds.contains(&egui::ViewportCommand::CancelClose), "{cmds:?}");
        assert!(!h.has("Save changes?"));
    }

    #[test]
    fn worksafe_save_on_untitled_requests_save_as() {
        let mut h = EditorHarness::new();
        assert!(h.state().file_path.is_none());
        h.request_close();
        h.click("Save");
        // The shell owns the file dialog: the editor asks, it never opens one.
        assert_eq!(h.rig().requests, vec![AppRequest::SaveAs]);
        assert!(!h.editor().close_ok, "nothing saved, nothing closed");
    }

    #[test]
    fn worksafe_background_save_matches_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snap.animproj");
        let mut s = doc();
        s.new_drawing_at_frame();
        let snapshot = s.snapshot_project();
        s.begin_save(path.clone());
        // An edit after the snapshot: not in the file, and still dirty.
        s.view.frame = 5;
        s.new_drawing_at_frame();
        assert!(s.wait_save());
        let loaded = anim_core::Engine::load(&path).unwrap();
        assert_eq!(loaded.project, snapshot, "the file is the snapshot");
        assert_ne!(loaded.project, s.engine.project, "later edits stay out");
        let cut = |p: &Project| p.scenes[0].cuts[0].drawings.len();
        assert_eq!(cut(&loaded.project), cut(&snapshot));
        assert!(s.is_dirty(), "the edit after the snapshot is unsaved");
    }

    #[test]
    fn worksafe_save_keeps_unknown_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.animproj");
        let mut s = doc();
        s.begin_save(path.clone());
        assert!(s.wait_save());
        {
            let c = rusqlite::Connection::open(&path).unwrap();
            c.execute_batch("CREATE TABLE zz_unknown (v TEXT); INSERT INTO zz_unknown VALUES ('kept');")
                .unwrap();
        }
        s.new_drawing_at_frame();
        s.begin_save(path.clone());
        assert!(s.wait_save());
        let c = rusqlite::Connection::open(&path).unwrap();
        let v: String = c
            .query_row("SELECT v FROM zz_unknown", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "kept", "a newer build's table survives the save");
        assert!(!dir.path().join("future.animproj.tmp").exists());
    }

    #[test]
    fn worksafe_second_save_keeps_bak() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cut.animproj");
        let bak = dir.path().join("cut.animproj.bak");
        let mut s = doc();
        s.begin_save(path.clone());
        assert!(s.wait_save());
        let first = anim_core::Engine::load(&path).unwrap().project;
        assert!(!bak.exists(), "a first save has nothing to back up");
        s.new_drawing_at_frame();
        s.begin_save(path.clone());
        assert!(s.wait_save());
        let kept = anim_core::Engine::load(&bak).unwrap().project;
        assert_eq!(kept, first, ".bak holds the previous version");
        assert_ne!(anim_core::Engine::load(&path).unwrap().project, first);
    }

    #[test]
    fn worksafe_autosave_clock_rules() {
        let mut c = AutosaveClock::new(5);
        assert!(!c.tick(0.0, true, false, false), "never at once");
        assert!(!c.tick(100.0, true, false, false), "interval not run");
        assert!(!c.tick(301.0, true, false, true), "never with the pen down");
        assert!(!c.tick(302.0, true, true, false), "never mid-stroke");
        assert!(!c.tick(303.0, true, false, false), "pen up 0 s: not idle yet");
        assert!(!c.tick(304.5, true, false, false), "pen up 1.5 s: not idle yet");
        assert!(c.tick(305.0, true, false, false), "dirty, idle 2 s, due");
        assert!(!c.tick(306.0, true, false, false), "the interval restarted");
        // Clean: never, and the interval restarts at the next change.
        assert!(!c.tick(700.0, false, false, false));
        assert!(!c.tick(701.0, true, false, false), "dirty just now: not due");
        assert!(c.tick(1001.0, true, false, false));
        // Off: never.
        let mut off = AutosaveClock::new(0);
        off.tick(0.0, true, false, false);
        assert!(!off.tick(10_000.0, true, false, false), "off is off");
    }

    #[test]
    fn worksafe_prune_keeps_newest_three() {
        let dir = tempfile::tempdir().unwrap();
        let s = doc();
        let file = PathBuf::from("C:/films/cut01.animproj");
        let mine = s.untitled_group.clone();
        for seq in 1..=5u64 {
            write_autosave(dir.path(), &s.engine.project, None, &mine, seq).unwrap();
        }
        // Another project's five, planted after: each group keeps its own 3.
        let theirs = autosave_group(Some(&file), &mine);
        for seq in 1..=5u64 {
            let other = autosave_path_in(dir.path(), &theirs, seq);
            std::fs::write(&other, b"not a project").unwrap();
        }
        assert_eq!(prune_autosaves(dir.path(), AUTOSAVE_KEEP), 2);
        let mut names: Vec<String> = std::fs::read_dir(crate::config::autosave_dir_in(dir.path()))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 6, "{names:?}");
        for seq in 3..=5 {
            assert!(names.contains(&format!("{mine}-{seq}.animproj")), "{names:?}");
        }
        assert!(!names.iter().any(|n| n.ends_with("-1.animproj") || n.ends_with("-2.animproj")));
    }

    #[test]
    fn worksafe_autosave_writes_only_the_data_dir() {
        let data = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let file = work.path().join("cut.animproj");
        let mut s = doc();
        s.begin_save(file.clone());
        assert!(s.wait_save());
        let before = std::fs::read(&file).unwrap();
        s.new_drawing_at_frame();
        let snap = s.snapshot_project();
        let group = autosave_group(Some(&file), &s.untitled_group);
        let auto = write_autosave(data.path(), &snap, Some(&file), &group, 7).unwrap();
        assert!(auto.starts_with(data.path().join("autosave")), "{auto:?}");
        assert_eq!(std::fs::read(&file).unwrap(), before, "the user's file is untouched");
        assert!(!work.path().join("cut.animproj.bak").exists());
        assert_eq!(anim_core::Engine::load(&auto).unwrap().project, snap);
        assert!(s.is_dirty(), "an autosave is not a save");
    }

    #[test]
    fn worksafe_recovery_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let file = work.path().join("cut.animproj");
        let s = doc();
        anim_core::store::save(&s.engine.project, &file).unwrap();
        touch(&file, 2_000_000_000);
        let group = autosave_group(Some(&file), &s.untitled_group);
        let old = write_autosave(dir.path(), &s.engine.project, Some(&file), &group, 1).unwrap();
        touch(&old, 1_900_000_000);
        let new = write_autosave(dir.path(), &s.engine.project, Some(&file), &group, 2).unwrap();
        touch(&new, 2_100_000_000);
        let dead = |_: &RunningRecord| false;
        assert!(recovery_candidates(dir.path(), &dead).is_empty(), "no running record: nothing");
        let rec = RunningRecord {
            pid: 42,
            open: Some(file.clone()),
            autosave: Some(new.clone()),
            started: 1,
        };
        rec.write_in(dir.path()).unwrap();
        assert_eq!(RunningRecord::read(&rec.path_in(dir.path())), Some(rec.clone()));
        let found = recovery_candidates(dir.path(), &dead);
        assert_eq!(found.len(), 1, "one dead copy");
        let paths: Vec<&PathBuf> = found[0].autosaves.iter().map(|a| &a.path).collect();
        assert_eq!(paths, vec![&new], "only newer than the file");
        discard_autosaves(dir.path(), &[new.clone(), file.clone()]);
        assert!(!new.exists() && file.exists(), "discard never leaves the autosave folder");
        rec.remove_in(dir.path());
        assert!(recovery_candidates(dir.path(), &dead).is_empty());
    }

    #[test]
    fn worksafe_crash_report_and_log() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("boom at frame 12"));
        let here = std::panic::Location::caller();
        let info = CrashInfo::new(1_234, payload.as_ref(), Some(here));
        let report = crash_report(&info);
        assert!(report.contains("boom at frame 12"), "{report}");
        assert!(report.contains(&format!("{}:{}", here.file(), here.line())), "{report}");
        let str_payload: Box<dyn std::any::Any + Send> = Box::new("static boom");
        assert_eq!(CrashInfo::new(0, str_payload.as_ref(), None).message, "static boom");
        let dir = tempfile::tempdir().unwrap();
        write_crash_log(dir.path(), &info).unwrap();
        write_crash_log(dir.path(), &info).unwrap();
        let log = std::fs::read_to_string(dir.path().join("crash.log")).unwrap();
        assert_eq!(log.matches("boom at frame 12").count(), 2, "the log appends");
    }

    #[test]
    fn worksafe_recent_files() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cfg, _) = crate::config::Config::load_from(dir.path());
        assert_eq!(cfg.session.autosave_minutes, AUTOSAVE_DEFAULT_MIN);
        let real = dir.path().join("real.animproj");
        std::fs::write(&real, b"x").unwrap();
        for i in 0..14 {
            cfg.session.push_recent(&dir.path().join(format!("p{i}.animproj")));
        }
        cfg.session.push_recent(&real);
        cfg.session.push_recent(&dir.path().join("p13.animproj"));
        cfg.session.push_recent(&real);
        let r = &cfg.session.recent;
        assert_eq!(r.len(), crate::config::RECENT_MAX, "capped");
        assert_eq!(r[0], real, "newest first");
        assert_eq!(r[1], dir.path().join("p13.animproj"));
        assert_eq!(r.iter().filter(|p| **p == real).count(), 1, "deduped");
        cfg.session.autosave_minutes = 10;
        cfg.save_to(dir.path()).unwrap();
        let (back, notes) = crate::config::Config::load_from(dir.path());
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(back.session.recent, cfg.session.recent, "survives the round trip");
        assert_eq!(back.session.autosave_minutes, 10);
        let flags = back.session.recent_entries();
        assert!(flags[0].1, "the real file is present");
        assert!(!flags[1].1, "a missing file is flagged");
    }

    #[test]
    fn worksafe_handover_restore_reads_dirty() {
        // The devloop / self-update handover: the live document goes to a
        // scratch copy (the engine's own save, as try_handover writes it),
        // the session names it and the real file.
        let scratch = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let carried = scratch.path().join("carried.animproj");
        let real = work.path().join("cut.animproj");
        let mut s = doc();
        s.new_drawing_at_frame();
        s.engine.save(&carried).unwrap();
        let session = crate::devloop::Session {
            pending: true,
            project: Some(carried.clone()),
            origin: Some(real.clone()),
            frame: 99,
            written_ms: 0,
        };
        let (path, loaded) = crate::restore_handover(&session).expect("names a document");
        assert_eq!(path, carried);
        let state = loaded.unwrap();
        assert_eq!(state.file_path, Some(real.clone()), "Save goes to the real file");
        assert!(state.is_dirty(), "the carried work is not known to be on disk");
        assert!(!real.exists(), "a restore writes nothing");
        // So closing over it asks.
        let mut h = EditorHarness::with_state(state);
        let cmds = h.request_close();
        assert!(cmds.contains(&egui::ViewportCommand::CancelClose), "{cmds:?}");
        assert!(h.has("Save changes?"), "{}", h.describe());
        assert!(crate::restore_handover(&crate::devloop::Session::default()).is_none());
    }

    #[test]
    fn worksafe_modal_holds_the_keys() {
        let mut h = EditorHarness::new();
        h.request_close();
        assert!(h.has("Save changes?"));
        let (rev, fired) = (h.state().engine.revision(), h.performed().len());
        h.key_mods(crate::testkit::CTRL, egui::Key::Z);
        assert_eq!(h.state().engine.revision(), rev, "no undo behind the question");
        assert_eq!(h.performed().len(), fired, "no shortcut fired");
        assert!(h.has("Save changes?"));
        h.key(egui::Key::Escape);
        assert!(!h.has("Save changes?"), "Esc is Cancel");
        assert!(!h.editor().close_ok);
        // The control: with the modal gone the same key undoes.
        h.key_mods(crate::testkit::CTRL, egui::Key::Z);
        assert_ne!(h.state().engine.revision(), rev, "the binding is live: {:?}", h.performed());
        // Enter is Save: untitled, it asks the shell for Save As (which the
        // rig never runs — a cancelled Save As, so the question comes back)...
        h.state_mut().meta_dirty = true;
        h.request_close();
        h.key(egui::Key::Enter);
        assert_eq!(h.rig().requests, vec![AppRequest::SaveAs], "Enter is Save");
        assert!(h.has("Save changes?"), "nothing saved: asked again");
        // ...with a file it saves there, and the close goes on once it lands.
        let work = tempfile::tempdir().unwrap();
        let path = work.path().join("enter.animproj");
        h.state_mut().file_path = Some(path.clone());
        h.key(egui::Key::Enter);
        h.state_mut().wait_save();
        h.run_frames(1);
        assert!(path.is_file(), "saved");
        assert!(h.editor().close_ok && !h.has("Save changes?"), "then closed");
    }

    /// The slate's file menu, one item (a click inside a menu closes it).
    fn file_menu(h: &mut EditorHarness, item: &str) {
        h.click(crate::vocab::MENU_FILE);
        h.click(item);
    }

    #[test]
    fn worksafe_new_and_open_ask_first() {
        // The slate's New… / Open… — and the shell's Ctrl+N / Ctrl+O, which
        // carry out the editor's request through the same Editor::leave.
        let mut clean = EditorHarness::with_state(doc());
        file_menu(&mut clean, "New…");
        assert!(!clean.has("Save changes?"), "nothing to lose: no question");
        assert!(clean.editor().request_new);
        let mut h = EditorHarness::new();
        file_menu(&mut h, "New…");
        assert!(h.has("Save changes?"), "{}", h.describe());
        assert!(!h.editor().request_new, "held for the answer");
        h.click("Cancel");
        assert!(!h.has("Save changes?") && !h.editor().request_new);
        file_menu(&mut h, "New…");
        h.hold("Don't Save");
        assert!(h.editor().request_new, "Don't Save goes on to New");
        // Open asks the same (Cancel only: going on opens a file dialog).
        h.editor_mut().request_new = false;
        file_menu(&mut h, "Open…");
        assert!(h.has("Save changes?"));
        h.click("Cancel");
        assert!(!h.has("Save changes?") && h.state().is_dirty());
        assert!(h.rig().requests.is_empty(), "nothing went to the shell");
    }

    #[test]
    fn worksafe_close_outranks_a_pending_new() {
        let mut h = EditorHarness::new();
        file_menu(&mut h, "New…");
        assert!(h.has("Save changes?"));
        let cmds = h.request_close();
        assert!(cmds.contains(&egui::ViewportCommand::CancelClose), "{cmds:?}");
        let cmds = h.hold("Don't Save");
        assert!(cmds.contains(&egui::ViewportCommand::Close), "the close won: {cmds:?}");
        assert!(h.editor().close_ok && !h.editor().request_new);
    }

    #[test]
    fn worksafe_open_recent_asks_first() {
        assert_eq!(
            crate::recent_label(Path::new("C:/films/ep01/c012.animproj")),
            "c012.animproj — ep01",
            "the folder tells repeated cut names apart"
        );
        let work = tempfile::tempdir().unwrap();
        let file = work.path().join("c012.animproj");
        anim_core::store::save(&doc().engine.project, &file).unwrap();
        let gone = work.path().join("gone.animproj");
        let mut h = EditorHarness::new();
        h.editor_mut().set_recent(&[file.clone(), gone.clone()]);
        // A click inside a menu closes it: open it afresh for each one.
        let open_recent = |h: &mut EditorHarness| {
            h.open_path("File/Open recent");
        };
        open_recent(&mut h);
        assert!(h.has(&format!("{} (missing)", crate::recent_label(&gone))), "{}", h.describe());
        h.click("Remove");
        assert_eq!(h.editor().recent_edit, Some(crate::RecentEdit::Remove(gone)));
        h.close_menus();
        open_recent(&mut h);
        h.click("Clear recent");
        assert_eq!(h.editor().recent_edit, Some(crate::RecentEdit::Clear));
        // Opening a recent file over unsaved work asks first.
        open_recent(&mut h);
        h.click(&crate::recent_label(&file));
        assert!(h.has("Save changes?"), "{}", h.describe());
        assert!(h.state().file_path.is_none(), "nothing replaced yet");
        h.hold("Don't Save");
        assert_eq!(h.state().file_path.as_deref(), Some(file.as_path()));
        assert!(!h.state().is_dirty(), "the opened file is as on disk");
    }

    #[test]
    fn worksafe_second_save_is_queued_not_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("twice.animproj");
        let mut s = doc();
        s.new_drawing_at_frame();
        let refusals = s.refusal_seq;
        s.begin_save(path.clone());
        // Ctrl+S again, after an edit, while the first is on the disk.
        s.view.frame = 5;
        s.new_drawing_at_frame();
        let second = s.snapshot_project();
        s.begin_save(path.clone());
        assert_eq!(s.refusal_seq, refusals, "a second Ctrl+S is never refused");
        assert!(s.wait_save(), "both landed");
        assert_eq!(s.refusal_seq, refusals);
        let on_disk = anim_core::Engine::load(&path).unwrap().project;
        assert_eq!(on_disk, second, "the file holds the later snapshot");
        assert!(!s.is_dirty(), "everything is on disk");
    }

    #[test]
    fn worksafe_close_waits_for_a_landing_save() {
        let work = tempfile::tempdir().unwrap();
        let path = work.path().join("cut.animproj");
        // The save lands: clean, so the held close goes on, unasked.
        let mut h = EditorHarness::new();
        h.state_mut().file_path = Some(path.clone());
        let land = h.state_mut().save_in_flight(path.clone());
        let cmds = h.request_close();
        assert!(cmds.contains(&egui::ViewportCommand::CancelClose), "{cmds:?}");
        assert!(!h.has("Save changes?"), "the landing save may answer it");
        assert!(!h.editor().close_ok);
        land.send(Ok(())).unwrap();
        h.run_frames(2);
        assert!(h.editor().close_ok, "landed clean: the close goes on");
        assert!(!h.has("Save changes?"));
        // The save fails: the question comes up instead.
        let mut h = EditorHarness::new();
        h.state_mut().file_path = Some(path.clone());
        let land = h.state_mut().save_in_flight(path);
        h.request_close();
        land.send(Err("disk full".into())).unwrap();
        h.run_frames(2);
        assert!(!h.editor().close_ok, "never closes over unsaved work");
        assert!(h.has("Save changes?"), "{}", h.describe());
    }

    #[test]
    fn worksafe_running_records_per_copy() {
        let dir = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let s = doc();
        // Two copies ran (a reference cut and the working cut): one
        // crashed, one is still open.
        let mut recs = Vec::new();
        for (pid, name) in [(101u32, "crashed"), (202, "alive")] {
            let file = work.path().join(format!("{name}.animproj"));
            anim_core::store::save(&s.engine.project, &file).unwrap();
            touch(&file, 2_000_000_000);
            let group = autosave_group(Some(&file), &s.untitled_group);
            let a = write_autosave(dir.path(), &s.engine.project, Some(&file), &group, 1).unwrap();
            touch(&a, 2_100_000_000);
            let rec = RunningRecord {
                pid,
                open: Some(file),
                autosave: Some(a),
                started: u64::from(pid),
            };
            rec.write_in(dir.path()).unwrap();
            recs.push(rec);
        }
        assert_ne!(recs[0].path_in(dir.path()), recs[1].path_in(dir.path()), "a file per copy");
        assert_eq!(running_records(dir.path()).len(), 2);
        let alive = |r: &RunningRecord| r.pid == 202;
        let offers = recovery_candidates(dir.path(), &alive);
        assert_eq!(offers.len(), 1, "a live copy's work is never offered");
        assert_eq!(offers[0].record, recs[0]);
        // A clean exit removes its own record, never another copy's.
        recs[1].remove_in(dir.path());
        assert!(!recs[1].path_in(dir.path()).exists());
        assert!(recs[0].path_in(dir.path()).exists());
        // The real check on this test's own process.
        let me = RunningRecord {
            pid: std::process::id(),
            started: u64::MAX,
            ..Default::default()
        };
        assert!(process_alive(&me), "this process runs");
        if cfg!(windows) {
            let reused = RunningRecord {
                pid: std::process::id(),
                started: 0,
                ..Default::default()
            };
            assert!(!process_alive(&reused), "a younger process under an old pid is another");
        }
    }

    #[test]
    fn worksafe_recovery_dialog_lists_autosaves() {
        const NOW: u64 = 1_700_000_000; // 22:13:20 UTC
        let file = PathBuf::from("C:/films/cut01.animproj");
        let at = |seq: u64| PathBuf::from(format!("auto/cut01-0a0b0c0d-{seq}.animproj"));
        let offer = Recovery {
            record_path: PathBuf::from("running-7-1.json"),
            record: RunningRecord {
                pid: 7,
                open: Some(file.clone()),
                autosave: None,
                started: 1,
            },
            autosaves: vec![
                Autosave { path: at(3), at: NOW - 30 },
                Autosave { path: at(2), at: NOW - 600 },
                Autosave { path: at(1), at: NOW - 7_200 },
            ],
            selected: 0,
        };
        let dialog = |offer: Recovery| {
            PanelHarness::new(
                egui::vec2(560.0, 360.0),
                (offer, None),
                |ui, (offer, out): &mut (Recovery, Option<RecoveryChoice>)| {
                    if let Some(c) = recovery_ui(ui, offer, NOW, 0) {
                        *out = Some(c);
                    }
                },
            )
        };
        let mut p = dialog(offer.clone());
        assert!(p.has("Recover unsaved work?"), "{}", p.describe());
        assert!(p.has(
            "Gaban Studio did not close cleanly while cut01.animproj was open — \
             its autosaves newer than the file:"
        ));
        for row in ["22:12 · under a minute ago (newest)", "22:03 · 10 min ago", "20:13 · 2 h ago"] {
            assert!(p.has(row), "{row}\n{}", p.describe());
        }
        assert_eq!(p.state().0.selected, 0, "the newest is picked");
        p.click("22:03 · 10 min ago");
        assert_eq!(p.state().0.selected, 1);
        p.click("Open recovered");
        let picked = Recovered {
            autosave: at(2),
            origin: Some(file),
            record: PathBuf::from("running-7-1.json"),
        };
        assert_eq!(p.state().1, Some(RecoveryChoice::Open(picked)), "Open opens the row picked");
        p.state_mut().1 = None;
        p.key(egui::Key::Escape);
        assert_eq!(p.state().1, Some(RecoveryChoice::Later), "Esc is Later");
        p.state_mut().1 = None;
        p.click("Discard");
        assert_eq!(p.state().1, None, "Discard is held, a click does nothing");
        p.hold("Discard");
        assert_eq!(p.state().1, Some(RecoveryChoice::Discard));
        // Untitled: no file to compare with.
        let mut untitled = offer;
        untitled.record.open = None;
        let p = dialog(untitled);
        assert!(p.has(
            "Gaban Studio did not close cleanly while an untitled project was open — its autosaves:"
        ), "{}", p.describe());
    }

    #[test]
    fn worksafe_recovery_choices_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = doc();
        let group = s.untitled_group.clone();
        let first = write_autosave(dir.path(), &s.snapshot_project(), None, &group, 1).unwrap();
        s.new_drawing_at_frame();
        let newest = s.snapshot_project();
        let second = write_autosave(dir.path(), &newest, None, &group, 2).unwrap();
        let rec = RunningRecord {
            pid: 9,
            open: None,
            autosave: Some(second.clone()),
            started: 5,
        };
        rec.write_in(dir.path()).unwrap();
        let dead = |_: &RunningRecord| false;
        let offers = recovery_candidates(dir.path(), &dead);
        assert_eq!(offers.len(), 1);
        let paths: Vec<&PathBuf> = offers[0].autosaves.iter().map(|a| &a.path).collect();
        assert_eq!(paths, vec![&second, &first], "untitled: every autosave, newest first");
        // Later touches nothing: the next start offers the same again.
        assert_eq!(recovery_candidates(dir.path(), &dead), offers, "Later keeps the record");
        // Open: the picked autosave, untitled, unsaved, in its own group.
        let r = offers[0].picked().unwrap();
        assert_eq!(r.autosave, second);
        let st = open_recovered(&r).unwrap();
        assert_eq!(st.engine.project, newest);
        assert!(st.is_dirty() && st.file_path.is_none());
        assert_eq!(st.untitled_group, group, "its next autosaves join the same group");
        // Through the editor: over a clean document it opens at once and
        // the dead copy's record goes...
        let ctx = egui::Context::default();
        let mut ed = crate::Editor::boot(doc(), None, Some(dir.path()), &Default::default());
        ed.leave(crate::Leave::Recover(r.clone()), &ctx);
        assert_eq!(ed.state.engine.project, newest);
        assert!(!rec.path_in(dir.path()).exists(), "the work is open again: the record goes");
        // ...over unsaved work it asks first and replaces nothing.
        rec.write_in(dir.path()).unwrap();
        let mut busy = doc();
        busy.new_drawing_at_frame();
        let mut ed = crate::Editor::boot(busy, None, Some(dir.path()), &Default::default());
        ed.leave(crate::Leave::Recover(r), &ctx);
        assert!(ed.leave_pending(), "never replaces unsaved work unasked");
        assert_ne!(ed.state.engine.project, newest);
        assert!(rec.path_in(dir.path()).exists());
        // Discard: the autosaves and the record are gone.
        discard_recovery(dir.path(), &offers[0]);
        assert!(!first.exists() && !second.exists() && !rec.path_in(dir.path()).exists());
        assert!(recovery_candidates(dir.path(), &dead).is_empty());
    }

    #[test]
    fn worksafe_old_files_still_load() {
        // A config.json from before GM2: no recent list, no interval.
        let dir = tempfile::tempdir().unwrap();
        let mut v = serde_json::to_value(crate::config::Config::default()).unwrap();
        let session = v["session"].as_object_mut().unwrap();
        assert!(session.remove("recent").is_some() && session.remove("autosave_minutes").is_some());
        let bytes = serde_json::to_vec_pretty(&v).unwrap();
        std::fs::write(crate::config::config_path_in(dir.path()), bytes).unwrap();
        let (cfg, notes) = crate::config::Config::load_from(dir.path());
        assert!(notes.is_empty(), "nothing to report: {notes:?}");
        assert_eq!(cfg.session.autosave_minutes, AUTOSAVE_DEFAULT_MIN);
        assert!(cfg.session.recent.is_empty());
        assert!(!dir.path().join("config.json.bad").exists(), "nothing was reset");
        // A project the pre-GM2 save wrote (plain store::save — no .tmp,
        // no .bak) opens clean and saves the GM2 way over itself.
        let work = tempfile::tempdir().unwrap();
        let file = work.path().join("old.animproj");
        let mut s = doc();
        s.new_drawing_at_frame();
        let old = s.snapshot_project();
        anim_core::store::save(&old, &file).unwrap();
        let mut st = AppState::load_from(file.clone()).unwrap();
        assert_eq!(st.engine.project, old);
        assert!(!st.is_dirty(), "an old file opens clean");
        st.meta_dirty = true;
        st.begin_save(file.clone());
        assert!(st.wait_save() && !st.is_dirty());
        assert_eq!(anim_core::Engine::load(&file).unwrap().project, st.snapshot_project());
        let bak = work.path().join("old.animproj.bak");
        assert_eq!(anim_core::Engine::load(&bak).unwrap().project, old, ".bak holds the old file");
    }

    #[test]
    fn worksafe_slate_dot_says_unsaved() {
        let mut h = EditorHarness::new();
        assert!(h.has("unsaved changes"), "the dot speaks after a gesture\n{}", h.describe());
        let work = tempfile::tempdir().unwrap();
        h.state_mut().begin_save(work.path().join("dot.animproj"));
        assert!(h.state_mut().wait_save());
        h.run_frames(1);
        assert!(!h.has("unsaved changes"), "gone once saved");
    }

    #[test]
    fn worksafe_untitled_groups_never_collide() {
        let (a, b) = (doc(), doc());
        assert_ne!(a.untitled_group, b.untitled_group, "two untitled 'ws' keep apart");
        assert!(a.untitled_group.starts_with("ws-"), "{}", a.untitled_group);
        assert_eq!(autosave_group(None, &a.untitled_group), a.untitled_group);
        let dir = tempfile::tempdir().unwrap();
        let p = autosave_path_in(dir.path(), &a.untitled_group, 7);
        assert_eq!(group_of(&p), Some(a.untitled_group.clone()));
        // A file's group is the file's, whichever document autosaved it.
        let ep1 = PathBuf::from("C:/films/ep01/c012.animproj");
        let ep2 = PathBuf::from("C:/films/ep02/c012.animproj");
        assert_eq!(autosave_group(Some(&ep1), "x"), autosave_group(Some(&ep1), "y"));
        assert_ne!(autosave_group(Some(&ep1), "x"), autosave_group(Some(&ep2), "x"));
        // A landed save drops its own group's autosaves, no other group's.
        write_autosave(dir.path(), &a.engine.project, None, &a.untitled_group, 1).unwrap();
        write_autosave(dir.path(), &a.engine.project, None, &a.untitled_group, 2).unwrap();
        let other = write_autosave(dir.path(), &b.engine.project, None, &b.untitled_group, 1).unwrap();
        drop_group(dir.path(), &a.untitled_group);
        let left: Vec<PathBuf> = std::fs::read_dir(crate::config::autosave_dir_in(dir.path()))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(left, vec![other]);
    }

    #[test]
    fn worksafe_clock_and_age_words() {
        assert_eq!(clock_hm(1_700_000_000, 0), "22:13");
        assert_eq!(clock_hm(1_700_000_000, 60), "23:13", "UTC+1");
        assert_eq!(clock_hm(1_700_000_000, -23 * 60), "23:13", "the day before");
        assert_eq!(age_text(100, 70), "under a minute ago");
        assert_eq!(age_text(1_000, 400), "10 min ago");
        assert_eq!(age_text(10_000, 2_800), "2 h ago");
        assert_eq!(age_text(1_000_000, 0), "11 days ago");
        assert_eq!(age_text(5, 10), "under a minute ago", "a clock set back is no age");
    }

    #[test]
    fn worksafe_recent_follows_the_open_document() {
        let mut session = SessionConfig::default();
        let a = PathBuf::from("C:/films/a.animproj");
        let b = PathBuf::from("C:/films/b.animproj");
        let mut seen = None;
        assert!(!follow_recent(&mut seen, None, &mut session), "untitled: nothing to list");
        assert!(follow_recent(&mut seen, Some(&a), &mut session), "an Open lists it");
        assert!(!follow_recent(&mut seen, Some(&a), &mut session), "once, not every frame");
        assert!(follow_recent(&mut seen, Some(&b), &mut session), "a Save As too");
        assert_eq!(session.recent, vec![b.clone(), a.clone()]);
        assert!(!follow_recent(&mut seen, None, &mut session));
        assert!(follow_recent(&mut seen, Some(&a), &mut session), "back to the front");
        assert_eq!(session.recent, vec![a, b]);
    }

    #[test]
    fn worksafe_settings_say_where_autosaves_are() {
        // After a mistaken Don't Save the autosaves are the only copy:
        // Settings ▸ General names the folder, and its button only raises
        // the flag (the shell opens the folder; the page spawns nothing).
        type Page = (crate::config::Config, bool);
        let folder = PathBuf::from("C:/data/AnimStudio/autosave");
        let shown = folder.display().to_string();
        let mut p = PanelHarness::new(
            egui::vec2(420.0, 320.0),
            (crate::config::Config::default(), false),
            move |ui, (cfg, show): &mut Page| {
                crate::config::general_page(ui, cfg, Some(folder.as_path()), show, None)
            },
        );
        assert!(p.has(&shown), "{}", p.describe());
        assert!(!p.state().1);
        p.click("show autosave folder");
        assert!(p.state().1, "the click asks the shell");
        // No data folder: nothing to name, no button.
        let p = PanelHarness::new(
            egui::vec2(420.0, 320.0),
            (crate::config::Config::default(), false),
            |ui, (cfg, show): &mut Page| crate::config::general_page(ui, cfg, None, show, None),
        );
        assert!(!p.has("show autosave folder"), "{}", p.describe());
    }
}
