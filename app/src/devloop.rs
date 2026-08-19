//! Dev loop — restart the app on rebuild WITHOUT losing your place.
//! (Live handover first exercised on the rig 2026-08-17.)
//!
//! ROOT (ratified): this stands on reloaded code and live state agreeing about
//! memory layout; if that's weak, this is rubble.
//!
//! So it does not reload code. There is no dylib and no `hot-lib-reloader`
//! anywhere in this crate, and NEVER-DO 1 forbids adding one. The new code
//! arrives the only way that cannot disagree about layout: a fresh process.
//! State crosses the boundary as a FILE in the document's own format — the
//! same path a normal save/open takes — so there is nothing for a changed
//! struct to reinterpret.
//!
//! The cycle:
//!   1. `cargo build` replaces the binary.
//!   2. The running app notices the watched exe's mtime moved, and settled.
//!   3. It refuses if that would cost work (NEVER-DO 2), and retries later.
//!   4. Otherwise: autosave → write session → spawn the new binary → exit.
//!   5. The new process restores, and only THEN clears the pending flag.
//!
//! Inert unless `ANIMSTUDIO_DEVLOOP=1`. Every path in this file is gated on
//! that, including the restore — a normal launch of the same binary must
//! behave exactly as it always did.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

/// How often to stat the exe. Cheap, but not every frame at 120fps.
const POLL_EVERY: Duration = Duration::from_millis(400);
/// A build writes the exe in pieces; wait for the mtime to stop moving before
/// treating it as finished. Restarting into a half-written binary is a crash.
const SETTLE: Duration = Duration::from_millis(600);
/// A pending session older than this is not ours to restore — the process that
/// wrote it died long ago, and silently reopening a stale scratch document is
/// worse than starting clean.
const SESSION_FRESH: Duration = Duration::from_secs(60 * 60);

/// Set on the shadow copy so it knows not to re-shadow itself.
const SHADOW_ENV: &str = "ANIMSTUDIO_DEVLOOP_SHADOW";
const WATCH_ENV: &str = "ANIMSTUDIO_DEVLOOP_WATCH";

/// True when the owner asked for the dev loop. Everything here is gated on it.
pub fn armed() -> bool {
    std::env::var_os("ANIMSTUDIO_DEVLOOP").is_some()
}

/// PSD-shipping (2026-08-19): the update relaunch reuses THIS session
/// machinery. RESUME arms only the restore path — never the watcher.
pub fn resume_armed() -> bool {
    std::env::var_os("ANIMSTUDIO_RESUME").is_some()
}

/// The watched binary's mtime, sampled at the TOP of `main` — before window and
/// GPU init, which take hundreds of ms and are long enough for a rebuild to
/// land unnoticed. Sampling it inside `App::new` (after that init) meant a
/// build finishing during startup was either missed or mistaken for a fresh
/// one, so the shadow could sit there running older code than the file it was
/// watching. The whole instrument rests on "the build he actually runs".
static BASELINE: OnceLock<Option<SystemTime>> = OnceLock::new();

fn session_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(base)
            .join("AnimStudio")
            .join("dev_session.json"),
    )
}

/// The autosave lives beside the session file, never in `target/` — a
/// `cargo clean` must not be able to delete the owner's work.
fn autosave_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(base)
            .join("AnimStudio")
            .join("dev_autosave.animproj"),
    )
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Session {
    /// Set by the exiting process, cleared by the one that SUCCESSFULLY
    /// restores — never before the document is actually back on screen.
    pub pending: bool,
    /// The file to reopen. Always the autosave: the handover writes the live
    /// document there so nothing depends on the owner having saved.
    pub project: Option<PathBuf>,
    /// The document's TRUE save target, carried across untouched.
    ///
    /// This field exists because its absence was a critical defect: the
    /// restore adopted the autosave's path as `file_path`, so the next Ctrl+S
    /// silently wrote the project into `%APPDATA%` scratch storage while the
    /// real file froze at its pre-restart state — and the status line said
    /// "saved". `None` means the document genuinely had no file yet, and Save
    /// must prompt.
    pub origin: Option<PathBuf>,
    /// Playhead, so you come back to the frame you were looking at.
    pub frame: u32,
    /// When the handover happened; a stale session is ignored (SESSION_FRESH).
    pub written_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Session {
    /// Read a pending session WITHOUT clearing it.
    ///
    /// Clearing on read was a defect: the flag was consumed before the load was
    /// attempted, so any load failure (a truncated autosave from a disk-full
    /// handover, a project the new binary can no longer open) silently threw
    /// the only pointer to the work away and started with an empty editor.
    /// The caller clears it once the document is genuinely restored.
    pub fn peek_pending() -> Option<Self> {
        if !armed() && !resume_armed() {
            return None;
        }
        let p = session_path()?;
        let s: Self = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
        if !s.pending {
            return None;
        }
        let age = now_ms().saturating_sub(s.written_ms);
        if Duration::from_millis(age) > SESSION_FRESH {
            return None;
        }
        Some(s)
    }

    /// Mark the handover complete. Called only after a successful restore.
    pub fn clear_pending() {
        let Some(p) = session_path() else { return };
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Ok(mut s) = serde_json::from_str::<Self>(&text) {
                s.pending = false;
                let _ = serde_json::to_string_pretty(&s).map(|t| std::fs::write(&p, t));
            }
        }
    }

    /// Write the session. Errors are RETURNED, not swallowed: the caller is
    /// about to exit the process, and exiting after a failed session write
    /// strands a good autosave that nothing will ever reopen.
    pub fn save(&self) -> Result<(), String> {
        let p = session_path().ok_or("no APPDATA to write the dev session into")?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&p, text).map_err(|e| e.to_string())
    }
}

/// What happened when the process tried to get out of its own binary's way.
pub enum Staging {
    /// A shadow was launched; this process is done and must exit.
    HandedOver,
    /// Nothing to do (loop disabled, or we already ARE the shadow).
    Continue,
    /// Staging failed. The loop must be treated as OFF: this process is
    /// running from — and therefore locking — the binary cargo needs to
    /// rewrite, so watching it would wait forever for a rebuild that can
    /// never land.
    Failed(String),
}

/// Windows will not let anything overwrite a RUNNING executable — `cargo
/// build` fails with "Access is denied (os error 5)" while the app is open.
/// That single OS rule would make the whole dev loop useless, so the running
/// process gets out of the way of its own binary: on startup it copies itself
/// to a sibling `*-devloop.exe`, launches that, and exits. The copy is what
/// you actually use; cargo's output path is left unlocked, and the copy
/// watches THAT path for the rebuild.
pub fn stage_shadow() -> Staging {
    // Sample the watched binary BEFORE anything slow happens.
    let _ = BASELINE.set(std::env::current_exe().ok().and_then(|e| {
        let watch = std::env::var_os(WATCH_ENV).map(PathBuf::from).unwrap_or(e);
        mtime(&watch)
    }));
    if !armed() || std::env::var_os(SHADOW_ENV).is_some() {
        return Staging::Continue;
    }
    let Ok(exe) = std::env::current_exe() else {
        return Staging::Failed("cannot locate our own binary".into());
    };
    let Some(dir) = exe.parent() else {
        return Staging::Failed("binary has no parent directory".into());
    };
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("app");
    let ext = exe.extension().and_then(|s| s.to_str()).unwrap_or("");
    let shadow = dir.join(if ext.is_empty() {
        format!("{stem}-devloop")
    } else {
        format!("{stem}-devloop.{ext}")
    });
    // The previous shadow may still be shutting down and holding the image
    // lock; a couple of quick retries covers the handover overlap.
    let mut copied = Err(String::new());
    for _ in 0..10 {
        match std::fs::copy(&exe, &shadow) {
            Ok(_) => {
                copied = Ok(());
                break;
            }
            Err(e) => {
                copied = Err(e.to_string());
                std::thread::sleep(Duration::from_millis(120));
            }
        }
    }
    if let Err(e) = copied {
        return Staging::Failed(format!("could not stage {}: {e}", shadow.display()));
    }
    match std::process::Command::new(&shadow)
        .env("ANIMSTUDIO_DEVLOOP", "1")
        .env(SHADOW_ENV, "1")
        // The shadow must watch the ORIGINAL binary — that is the one cargo
        // rewrites. Its own path is a copy that never changes.
        .env(WATCH_ENV, &exe)
        .spawn()
    {
        Ok(_) => Staging::HandedOver,
        Err(e) => Staging::Failed(format!("could not launch {}: {e}", shadow.display())),
    }
}

pub struct DevLoop {
    enabled: bool,
    /// The binary cargo rewrites — watched for the rebuild, and relaunched
    /// when it lands. When running as a shadow copy this is the ORIGINAL, not
    /// our own path (relaunching our own stale copy would loop on old code).
    watch: PathBuf,
    seen: Option<SystemTime>,
    candidate: Option<(SystemTime, std::time::Instant)>,
    last_poll: std::time::Instant,
    /// Surfaced in the status bar. A refusal the owner cannot see is
    /// indistinguishable from a dev loop that has quietly died.
    pub note: Option<String>,
}

impl DevLoop {
    /// `staging_failed` forces the loop off — see `Staging::Failed`.
    pub fn new(staging_failed: Option<String>) -> Self {
        let exe = std::env::current_exe().unwrap_or_default();
        let watch = std::env::var_os(WATCH_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| exe.clone());
        let seen = BASELINE.get().copied().flatten().or_else(|| mtime(&watch));
        Self {
            enabled: armed() && staging_failed.is_none(),
            watch,
            seen,
            candidate: None,
            last_poll: std::time::Instant::now(),
            note: staging_failed.map(|e| format!("dev loop OFF — {e}")),
        }
    }

    /// Has a rebuild landed and settled? Call once per frame; it self-throttles.
    pub fn poll(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        if self.last_poll.elapsed() < POLL_EVERY {
            return false;
        }
        self.last_poll = std::time::Instant::now();
        let Some(now) = mtime(&self.watch) else {
            return false;
        };
        if Some(now) == self.seen {
            return false;
        }
        match self.candidate {
            Some((t, first)) if t == now => first.elapsed() >= SETTLE,
            _ => {
                self.candidate = Some((now, std::time::Instant::now()));
                self.note = Some("rebuild detected — waiting for it to settle".into());
                false
            }
        }
    }

    /// Stop asking about the rebuild we just failed to act on.
    ///
    /// `poll` deliberately keeps returning true so a REFUSED restart (mid
    /// stroke, export running) is retried. But a failed relaunch or a failed
    /// autosave is not a refusal — without this the app would re-serialise the
    /// entire project to disk every 400ms, forever, with nothing on screen to
    /// say why the editor had started stuttering.
    pub fn give_up(&mut self) {
        self.seen = mtime(&self.watch);
        self.candidate = None;
    }

    /// Hand over to the new binary. On success this process exits, so the only
    /// way this returns is failure — and it returns the reason.
    #[must_use]
    pub fn relaunch(&self) -> String {
        // Launch the freshly built ORIGINAL, deliberately WITHOUT the shadow
        // marker: it will stage a new copy of itself and hand over to that.
        // Relaunching our own (stale) copy would run the old code forever —
        // the loop would look alive and change nothing.
        let mut cmd = std::process::Command::new(&self.watch);
        cmd.env("ANIMSTUDIO_DEVLOOP", "1");
        cmd.env_remove(SHADOW_ENV);
        cmd.env_remove(WATCH_ENV);
        match cmd.spawn() {
            Ok(_) => std::process::exit(0),
            Err(e) => e.to_string(),
        }
    }
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).ok()?.modified().ok()
}

/// The autosave target for the handover.
pub fn autosave_target() -> Option<PathBuf> {
    let p = autosave_path()?;
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    Some(p)
}

pub fn session_stamp() -> u64 {
    now_ms()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The invariant that protects the owner's file.
    ///
    /// A restart autosaves to scratch storage and reopens from it, so the
    /// document's TRUE save target has to survive the trip in its own field.
    /// The first version of this code inferred it from a boolean instead, and
    /// inferred it backwards: a project with a real file came back with
    /// `file_path` pointing at `%APPDATA%\dev_autosave.animproj`, so the next
    /// Ctrl+S wrote the work into scratch while the real file froze — and the
    /// status line said "saved". This test exists so that cannot return.
    #[test]
    fn the_real_save_target_survives_a_handover() {
        let autosave =
            PathBuf::from(r"C:\Users\x\AppData\Roaming\AnimStudio\dev_autosave.animproj");
        let real = PathBuf::from(r"C:\art\shot01.animproj");
        let saved_doc = Session {
            pending: true,
            project: Some(autosave.clone()),
            origin: Some(real.clone()),
            frame: 7,
            written_ms: 1_000,
        };
        let round: Session =
            serde_json::from_str(&serde_json::to_string(&saved_doc).unwrap()).unwrap();
        assert_eq!(
            round.origin.as_ref(),
            Some(&real),
            "a document with a file must come back pointing at THAT file"
        );
        assert_ne!(
            round.origin, round.project,
            "the save target must never be the scratch autosave"
        );
        assert_eq!(round.frame, 7, "the playhead comes back too");

        // A document that never had a file must come back with none, so Save
        // prompts instead of silently choosing scratch storage.
        let new_doc = Session {
            pending: true,
            project: Some(autosave),
            origin: None,
            frame: 0,
            written_ms: 1_000,
        };
        let round: Session =
            serde_json::from_str(&serde_json::to_string(&new_doc).unwrap()).unwrap();
        assert!(
            round.origin.is_none(),
            "an unsaved document must not inherit the autosave as its file"
        );
    }
}
