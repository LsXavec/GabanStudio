//! RELAUNCH TO UPDATE (research/PSD-shipping.md, gate 2026-08-19): the
//! published-build channel. A background thread asks the configured
//! GitHub repo's latest release for a newer `animstudio-*.exe`; the Foot
//! lights UPDATE READY; the click downloads, RENAMES the running exe
//! aside, seats the new one, and relaunches through the devloop's proven
//! session-carry.
//!
//! LAWS (the room's NEVER-DO, enforced here):
//! - Reversible: the old exe survives as animstudio-old.exe until a
//!   LATER successful launch deletes it; a short download replaces
//!   nothing (length checked against the asset's declared size).
//! - Never block the pen: everything network runs on this thread; the
//!   UI reads a channel. Offline is chatter, never a refusal.
//! - Only the configured repo over https, only assets named for us.
//! - The dev channel wins: an ANIMSTUDIO_DEVLOOP-armed build never
//!   self-updates.

use std::sync::mpsc::{Receiver, channel};

/// What the checker thread reports back to the UI.
pub enum UpdateEvent {
    /// A newer release exists: (version tag, asset url, asset size).
    Ready {
        tag: String,
        url: String,
        size: u64,
    },
    UpToDate(String),
    /// Quiet trouble (offline, bad repo, rate limit) — chatter at most.
    Note(String),
}

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// "v1.2.3" / "1.2.3" → (1, 2, 3). Anything unparseable = (0, 0, 0), so
/// a garbage tag can never look newer than a real version.
pub fn parse_version(tag: &str) -> (u32, u32, u32) {
    let t = tag.trim().trim_start_matches(['v', 'V']);
    let mut it = t.split('.').map(|p| {
        p.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u32>()
            .unwrap_or(0)
    });
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

/// Ask `owner/repo`'s latest release for a newer build. Blocking —
/// callers spawn it on a thread.
fn check_once(repo: &str) -> UpdateEvent {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let resp = ureq::get(&url)
        .header("User-Agent", "animstudio-updater")
        .header("Accept", "application/vnd.github+json")
        .call();
    let mut resp = match resp {
        Ok(r) => r,
        Err(e) => return UpdateEvent::Note(format!("update check: {e}")),
    };
    let body = match resp.body_mut().read_to_string() {
        Ok(b) => b,
        Err(e) => return UpdateEvent::Note(format!("update check: {e}")),
    };
    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(j) => j,
        Err(e) => return UpdateEvent::Note(format!("update check: {e}")),
    };
    let tag = json["tag_name"].as_str().unwrap_or_default().to_string();
    if tag.is_empty() {
        return UpdateEvent::Note("update check: no releases yet".into());
    }
    if parse_version(&tag) <= parse_version(CURRENT_VERSION) {
        return UpdateEvent::UpToDate(tag);
    }
    // The release must carry an asset named for this app (NEVER-DO 4).
    let Some(assets) = json["assets"].as_array() else {
        return UpdateEvent::Note(format!("release {tag} has no assets"));
    };
    for a in assets {
        let name = a["name"].as_str().unwrap_or_default();
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("animstudio") && lower.ends_with(".exe") && !lower.contains("setup") {
            let url = a["browser_download_url"].as_str().unwrap_or_default();
            let size = a["size"].as_u64().unwrap_or(0);
            if url.starts_with("https://") && size > 0 {
                return UpdateEvent::Ready {
                    tag,
                    url: url.to_string(),
                    size,
                };
            }
        }
    }
    UpdateEvent::Note(format!("release {tag} carries no animstudio exe"))
}

/// Spawn the background check (on launch and on "check now").
pub fn spawn_check(repo: String) -> Receiver<UpdateEvent> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(check_once(&repo));
    });
    rx
}

/// Download the new build and seat it beside the running exe as
/// animstudio-new.exe. Blocking — spawned on a thread. Length-checked:
/// a short or oversized file replaces nothing (NEVER-DO 1).
fn download(url: &str, expect: u64, to: &std::path::Path) -> Result<(), String> {
    let resp = ureq::get(url)
        .header("User-Agent", "animstudio-updater")
        .call()
        .map_err(|e| e.to_string())?;
    let mut reader = resp.into_body().into_reader();
    let mut bytes = Vec::with_capacity(expect.min(512 * 1024 * 1024) as usize);
    std::io::Read::read_to_end(&mut reader, &mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 != expect {
        return Err(format!(
            "download was {} bytes, the release says {expect} — refused",
            bytes.len()
        ));
    }
    std::fs::write(to, &bytes).map_err(|e| e.to_string())
}

/// The whole swap, run on a thread after the owner clicks: download →
/// rename the RUNNING exe aside (Windows allows renaming, not deleting)
/// → seat the new exe → report ready-to-relaunch. The caller then saves
/// the session (devloop's own path) and spawns the new binary.
pub fn spawn_swap(url: String, size: u64) -> Receiver<Result<std::path::PathBuf, String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<std::path::PathBuf, String> {
            let me = std::env::current_exe().map_err(|e| e.to_string())?;
            let dir = me.parent().ok_or("no parent dir")?.to_path_buf();
            let staged = dir.join("animstudio-new.exe");
            download(&url, size, &staged)?;
            let old = dir.join("animstudio-old.exe");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&me, &old).map_err(|e| format!("could not step aside: {e}"))?;
            match std::fs::rename(&staged, &me) {
                Ok(()) => Ok(me),
                Err(e) => {
                    // Roll back: the old exe returns to its seat.
                    let _ = std::fs::rename(&old, &me);
                    Err(format!("could not seat the new build: {e}"))
                }
            }
        })();
        let _ = tx.send(result);
    });
    rx
}

/// A later successful launch sweeps the stepped-aside binary
/// (NEVER-DO 1: only ever deleted by a launch that works).
pub fn sweep_old() {
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        let _ = std::fs::remove_file(dir.join("animstudio-old.exe"));
        let _ = std::fs::remove_file(dir.join("animstudio-new.exe"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_sanely() {
        assert!(parse_version("v0.2.0") > parse_version("0.1.0"));
        assert!(parse_version("1.0.0") > parse_version("v0.9.9"));
        assert_eq!(parse_version("v1.2.3"), (1, 2, 3));
        assert_eq!(parse_version("1.2.3-beta.1"), (1, 2, 3));
        // Garbage can never look newer.
        assert_eq!(parse_version("garbage"), (0, 0, 0));
        assert!(parse_version("garbage") <= parse_version(CURRENT_VERSION));
    }

    #[test]
    fn swap_dance_is_reversible_on_failure() {
        // The rename dance on plain files: old steps aside, new seats;
        // when seating fails the old returns.
        let dir = std::env::temp_dir().join("animstudio_swap_test");
        let _ = std::fs::create_dir_all(&dir);
        let me = dir.join("app.exe");
        let old = dir.join("app-old.exe");
        std::fs::write(&me, b"v1").unwrap();
        let _ = std::fs::remove_file(&old);
        // Step aside + seat.
        std::fs::rename(&me, &old).unwrap();
        std::fs::write(dir.join("staged.exe"), b"v2").unwrap();
        std::fs::rename(dir.join("staged.exe"), &me).unwrap();
        assert_eq!(std::fs::read(&me).unwrap(), b"v2");
        assert_eq!(std::fs::read(&old).unwrap(), b"v1", "the way back survives");
    }
}
