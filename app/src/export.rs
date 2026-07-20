//! Export: PNG sequence (with alpha) and MP4 via the system ffmpeg.
//! Rendering is the engine's headless compositor (anim_core::export) — the
//! same math as the GPU display, so what you see is what exports.
//!
//! When the cut's node graph has a wired Output, export renders THROUGH the
//! graph (the composite-view truth); otherwise it falls back to the plain
//! column-stack composite. The note returned to the status bar says which.
//!
//! Both exporters run on a DEDICATED WORKER THREAD (C3) — the same shape as
//! the tablet backend's thread: rendering a big cut can take real seconds,
//! and doing that inline on the UI thread would freeze the app for the
//! whole export. The thread operates on an OWNED SNAPSHOT of the cut
//! (`Cut` is `Clone`), never a live reference — the artist can keep drawing
//! in a different cut while an export runs, and the export itself can never
//! be edited out from under it mid-render (a live-reference export would
//! render an inconsistent frame-to-frame moving target instead).

use std::io::BufWriter;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::Arc;

use anim_core::export::{
    render_frame, render_frame_over, render_graph_frame, render_graph_frame_over,
    vector_stroke_cels,
};
use anim_core::model::Cut;
use eframe::egui;

use crate::doc::AppState;

/// Progress/completion messages from a background export job. `Frame` is
/// deliberately just a tick (no frame number) — the receiver already knows
/// the job's total and counts completions itself, keeping the channel light
/// for a long export.
pub enum ExportProgress {
    Frame,
    /// MP4 only: all frames rendered, now inside the ffmpeg subprocess —
    /// there's no cheap way to get ITS progress, so the UI shows this as an
    /// indeterminate "encoding…" state instead of a stalled progress bar.
    Encoding,
    Done(Result<(usize, String), String>),
}

/// Sentinel error string for a user-cancelled job — checked by the poller,
/// not a real failure, so it gets its own status message instead of
/// "export failed: cancelled".
pub const CANCELLED: &str = "cancelled";

/// Kick off a PNG-sequence export (one file per frame, transparent
/// background) on a worker thread. Output files are named by SOURCE frame
/// number (`frame_0011.png` for source frame 10), so exporting a sub-range
/// stays self-documenting and re-exporting a different range never clobbers
/// unrelated files.
pub fn spawn_png_sequence(
    cut: Cut,
    width: u32,
    height: u32,
    dir: PathBuf,
    range: RangeInclusive<u32>,
    ctx: egui::Context,
    cancel: Arc<AtomicBool>,
) -> Receiver<ExportProgress> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<(usize, String), String> {
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("create {}: {e}", dir.display()))?;
            let mut n = 0usize;
            for f in range {
                if cancel.load(Ordering::Relaxed) {
                    return Err(CANCELLED.into());
                }
                let rgba = render_graph_frame(&cut, f, width, height)
                    .unwrap_or_else(|| render_frame(&cut, f, width, height));
                write_png(&dir.join(format!("frame_{:04}.png", f + 1)), width, height, &rgba)?;
                n += 1;
                let _ = tx.send(ExportProgress::Frame);
                ctx.request_repaint();
            }
            Ok((n, format!("{}{}", graph_note(&cut), strokes_note(&cut))))
        })();
        let _ = tx.send(ExportProgress::Done(result));
        ctx.request_repaint();
    });
    rx
}

/// Kick off an MP4 export (white background, H.264) on a worker thread.
/// Frames render to a sequentially-numbered temp dir (invisible to the
/// user, cleaned up after — its numbering doesn't need to match source
/// frame numbers, unlike the PNG-sequence path), then the system `ffmpeg`
/// encodes it.
#[allow(clippy::too_many_arguments)] // one call site; a params struct would be noise
pub fn spawn_mp4(
    cut: Cut,
    width: u32,
    height: u32,
    fps: u32,
    path: PathBuf,
    range: RangeInclusive<u32>,
    ctx: egui::Context,
    cancel: Arc<AtomicBool>,
) -> Receiver<ExportProgress> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<(usize, String), String> {
            ffmpeg_available()?;
            let tmp =
                std::env::temp_dir().join(format!("animstudio_export_{}", std::process::id()));
            std::fs::create_dir_all(&tmp).map_err(|e| format!("temp dir: {e}"))?;
            let mut n = 0usize;
            for f in range {
                if cancel.load(Ordering::Relaxed) {
                    let _ = std::fs::remove_dir_all(&tmp);
                    return Err(CANCELLED.into());
                }
                let rgba = render_graph_frame_over(&cut, f, width, height, [255, 255, 255])
                    .unwrap_or_else(|| render_frame_over(&cut, f, width, height, [255, 255, 255]));
                write_png(&tmp.join(format!("frame_{:04}.png", n + 1)), width, height, &rgba)?;
                n += 1;
                let _ = tx.send(ExportProgress::Frame);
                ctx.request_repaint();
            }
            let _ = tx.send(ExportProgress::Encoding);
            ctx.request_repaint();

            let pattern = tmp.join("frame_%04d.png");
            // Max-compatibility H.264: yuv420p (the flag hardware decoders
            // require), High@4.1 (universal on modern phones/TVs/browsers),
            // CRF 18 (visually lossless for line art), +faststart (moov atom
            // up front so the file streams/scrubs immediately when shared).
            // Odd dimensions crop by 1px (yuv420p subsampling needs even
            // sizes).
            let status = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-framerate",
                    &fps.to_string(),
                    "-i",
                    &pattern.to_string_lossy(),
                    "-c:v",
                    "libx264",
                    "-profile:v",
                    "high",
                    "-level:v",
                    "4.1",
                    "-pix_fmt",
                    "yuv420p",
                    "-crf",
                    "18",
                    "-preset",
                    "medium",
                    "-movflags",
                    "+faststart",
                    "-vf",
                    "crop=trunc(iw/2)*2:trunc(ih/2)*2",
                    &path.to_string_lossy(),
                ])
                .status()
                .map_err(|e| format!("running ffmpeg: {e}"))?;
            let _ = std::fs::remove_dir_all(&tmp);
            if !status.success() {
                return Err(format!("ffmpeg failed (exit {:?})", status.code()));
            }
            Ok((n, format!("{}{}", graph_note(&cut), strokes_note(&cut))))
        })();
        let _ = tx.send(ExportProgress::Done(result));
        ctx.request_repaint();
    });
    rx
}

/// Which render path exported — surfaced in the status message.
fn graph_note(cut: &Cut) -> &'static str {
    if cut.graph.output.is_some() {
        " [node graph]"
    } else {
        " [column stack]"
    }
}

/// A default export location proposal next to the project file, if any.
pub fn suggest_dir(state: &AppState) -> Option<PathBuf> {
    state.file_path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

fn strokes_note(cut: &Cut) -> String {
    match vector_stroke_cels(cut) {
        0 => String::new(),
        n => format!(" ({n} cel(s) carry legacy vector strokes — not included)"),
    }
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let file =
        std::fs::File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| format!("png header: {e}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| format!("png write: {e}"))?;
    Ok(())
}

fn ffmpeg_available() -> Result<(), String> {
    match std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(s) if s.success() => Ok(()),
        _ => Err(
            "ffmpeg not found on PATH — install it (e.g. `winget install ffmpeg`) \
             or export a PNG sequence instead"
                .into(),
        ),
    }
}
