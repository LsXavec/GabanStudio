//! NEVER LOSE WORK (GM2): the parts of the safety net that need no window.
//! The autosave scheduler (a pure clock the editor feeds — no wall clock
//! inside), where autosaves live and how many are kept, the running record
//! that tells the next start-up a session did not end cleanly, and the
//! crash log. Every path takes the data dir (D11); the real one is resolved
//! in main() only. Nothing here writes the user's file: autosaves, running
//! records and crash logs all live in the data folder (NEVER-DO 5).

use std::path::{Path, PathBuf};

use anim_core::model::Project;

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

/// `<stem>-<hash8>`: one project's autosave group. The stem is the file's
/// (or, untitled, the project's) name made file-safe; the hash tells two
/// `cut01.animproj` in different folders apart.
pub fn autosave_group(file: Option<&Path>, name: &str) -> String {
    let (stem, identity) = match file {
        Some(p) => (
            p.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            p.to_string_lossy().to_lowercase(),
        ),
        None => (name.to_string(), format!("untitled:{name}")),
    };
    let mut safe: String = stem
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .take(40)
        .collect();
    if safe.is_empty() {
        safe = "untitled".into();
    }
    format!("{safe}-{}", hash8(&identity))
}

/// Where autosave number `seq` of this document goes:
/// `<dir>/autosave/<stem>-<hash8>-<seq>.animproj`.
pub fn autosave_path_in(dir: &Path, file: Option<&Path>, name: &str, seq: u64) -> PathBuf {
    crate::config::autosave_dir_in(dir).join(format!("{}-{seq}.animproj", autosave_group(file, name)))
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
/// untitled) into the data dir, then prune to the newest `AUTOSAVE_KEEP`.
/// The user's file is only ever READ here (its unknown tables carry over);
/// no `.bak` is made. Runs on a worker thread.
pub fn write_autosave(
    dir: &Path,
    project: &Project,
    file: Option<&Path>,
    seq: u64,
) -> Result<PathBuf, String> {
    let path = autosave_path_in(dir, file, &project.name, seq);
    std::fs::create_dir_all(crate::config::autosave_dir_in(dir)).map_err(|e| e.to_string())?;
    anim_core::store::save_atomic_from(project, &path, file, false).map_err(|e| e.to_string())?;
    prune_autosaves(dir, AUTOSAVE_KEEP);
    Ok(path)
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

/// `<data>/running.json`: written at start-up, removed on a clean exit.
/// Finding one at start-up means the last session crashed.
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

pub fn running_path_in(dir: &Path) -> PathBuf {
    dir.join("running.json")
}

impl RunningRecord {
    pub fn write_in(&self, dir: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::config::write_atomic(&running_path_in(dir), &bytes)
    }

    pub fn read_in(dir: &Path) -> Option<Self> {
        let bytes = std::fs::read(running_path_in(dir)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn remove_in(dir: &Path) {
        let _ = std::fs::remove_file(running_path_in(dir));
    }
}

/// What a crashed session left worth offering: its document's autosaves
/// that are newer than the document on disk (all of them for an untitled
/// or missing document), newest first. No running record → nothing.
pub fn recovery_candidates(dir: &Path) -> Vec<PathBuf> {
    let Some(rec) = RunningRecord::read_in(dir) else {
        return Vec::new();
    };
    let group = match (&rec.autosave, &rec.open) {
        (Some(a), _) => match parse_autosave(a) {
            Some((g, _)) => g,
            None => return Vec::new(),
        },
        (None, Some(open)) => autosave_group(Some(open), ""),
        (None, None) => return Vec::new(),
    };
    let doc_time = rec
        .open
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());
    let mut found: Vec<(PathBuf, u64)> = list_autosaves(dir)
        .into_iter()
        .filter(|(_, g, _)| *g == group)
        .filter(|(p, _, _)| match doc_time {
            None => true,
            Some(doc) => std::fs::metadata(p)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t > doc),
        })
        .map(|(p, _, s)| (p, s))
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1));
    found.into_iter().map(|(p, _)| p).collect()
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

/// main() only: every panic also lands in `<dir>/crash.log` (running.json
/// stays behind, so the next start offers recovery). The previous hook
/// still runs.
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
        p.click("+ role");
        assert!(p.state().0.meta_dirty && p.state().0.is_dirty(), "a palette edit is unsaved work");
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
        for seq in 1..=5u64 {
            write_autosave(dir.path(), &s.engine.project, None, seq).unwrap();
        }
        // Another project's five, planted after: each group keeps its own 3.
        for seq in 1..=5u64 {
            let other = autosave_path_in(dir.path(), Some(&file), "x", seq);
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
        let mine = autosave_group(None, "ws");
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
        let auto = write_autosave(data.path(), &snap, Some(&file), 7).unwrap();
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
        let old = write_autosave(dir.path(), &s.engine.project, Some(&file), 1).unwrap();
        touch(&old, 1_900_000_000);
        let new = write_autosave(dir.path(), &s.engine.project, Some(&file), 2).unwrap();
        touch(&new, 2_100_000_000);
        assert!(recovery_candidates(dir.path()).is_empty(), "no running.json: nothing");
        let rec = RunningRecord {
            pid: 42,
            open: Some(file.clone()),
            autosave: Some(new.clone()),
            started: 1,
        };
        rec.write_in(dir.path()).unwrap();
        assert_eq!(RunningRecord::read_in(dir.path()), Some(rec));
        assert_eq!(recovery_candidates(dir.path()), vec![new.clone()], "only newer than the file");
        discard_autosaves(dir.path(), &[new.clone(), file.clone()]);
        assert!(!new.exists() && file.exists(), "discard never leaves the autosave folder");
        RunningRecord::remove_in(dir.path());
        assert!(recovery_candidates(dir.path()).is_empty());
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
}
