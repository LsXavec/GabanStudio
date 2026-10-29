//! Export's helpers: where an export goes, and where ffmpeg is.
//!
//! GM11: the exporting itself is the render queue's (render_queue.rs).
//! File ▸ Export…'s PNG sequence and MP4 are queue jobs (the "PNG 8-bit"
//! and "H.264 review" presets), rendered one after another on its worker
//! thread from an OWNED SNAPSHOT of the cut taken when the job starts — the
//! artist keeps drawing, and an edit never leaks into a running render. The
//! v0.3 exporters that ran beside it (C3) are gone: one path, one truth.

use std::path::{Path, PathBuf};

use crate::doc::AppState;

/// A default export location proposal next to the project file, if any.
pub fn suggest_dir(state: &AppState) -> Option<PathBuf> {
    state
        .file_path
        .as_ref()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// The ffmpeg executable's file name here.
const FFMPEG_EXE: &str = if cfg!(windows) {
    "ffmpeg.exe"
} else {
    "ffmpeg"
};

/// Locate ffmpeg with the real places: Settings ▸ General's "ffmpeg
/// location" (`configured`), then [`find_ffmpeg_in`]'s order.
pub fn find_ffmpeg(configured: Option<&Path>) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok();
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    find_ffmpeg_in(
        configured,
        exe.as_deref().and_then(Path::parent),
        std::env::var_os("PATH").as_deref(),
        local.as_deref(),
    )
}

/// Locate ffmpeg: the configured location (the file, or a folder holding
/// it) → next to our own exe (portable bundling) → PATH → the winget
/// package dir (winget edits the REGISTRY PATH, which processes launched
/// from an already-open shell never see — the app must not demand a reboot
/// to find a tool that's plainly installed). GM11: files only — PATH's
/// folders are scanned rather than `ffmpeg -version` spawned, so nothing
/// runs to look.
pub fn find_ffmpeg_in(
    configured: Option<&Path>,
    exe_dir: Option<&Path>,
    path_var: Option<&std::ffi::OsStr>,
    local_appdata: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(c) = configured {
        if c.is_file() {
            return Some(c.to_path_buf());
        }
        let inside = c.join(FFMPEG_EXE);
        if inside.is_file() {
            return Some(inside);
        }
    }
    if let Some(dir) = exe_dir {
        let local = dir.join(FFMPEG_EXE);
        if local.is_file() {
            return Some(local);
        }
    }
    if let Some(path) = path_var {
        for dir in std::env::split_paths(path) {
            let candidate = dir.join(FFMPEG_EXE);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    // winget install location: %LOCALAPPDATA%\Microsoft\WinGet\Packages\
    //   Gyan.FFmpeg_*\ffmpeg-*\bin\ffmpeg.exe
    let packages = local_appdata?.join("Microsoft/WinGet/Packages");
    let entries = std::fs::read_dir(&packages).ok()?;
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("Gyan.FFmpeg")
        {
            continue;
        }
        let Ok(subs) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        for sub in subs.flatten() {
            let candidate = sub.path().join("bin/ffmpeg.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
