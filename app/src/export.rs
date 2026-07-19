//! Export: PNG sequence (with alpha) and MP4 via the system ffmpeg.
//! Rendering is the engine's headless compositor (anim_core::export) — the
//! same math as the GPU display, so what you see is what exports.

use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anim_core::export::{render_frame, render_frame_over, vector_stroke_cels};

use crate::doc::AppState;

/// Export the whole cut as `frame_0001.png` … into `dir` (transparent
/// background, straight alpha). Returns (frames written, skipped-strokes note).
pub fn export_png_sequence(state: &AppState, dir: &Path) -> Result<(usize, String), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let cut = state.cut();
    let (w, h) = (state.engine.project.width, state.engine.project.height);
    let n = state.frame_count();
    for f in 0..n {
        let rgba = render_frame(cut, f, w, h);
        let path = dir.join(format!("frame_{:04}.png", f + 1));
        write_png(&path, w, h, &rgba)?;
    }
    Ok((n as usize, strokes_note(cut)))
}

/// Export the whole cut as an MP4 at `path` (white background) using the
/// system ffmpeg. Frames go through a temp dir; it is cleaned up afterwards.
pub fn export_mp4(state: &AppState, path: &Path) -> Result<(usize, String), String> {
    ffmpeg_available()?;
    let cut = state.cut();
    let (w, h) = (state.engine.project.width, state.engine.project.height);
    let n = state.frame_count();

    let tmp = std::env::temp_dir().join(format!("animstudio_export_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("temp dir: {e}"))?;
    for f in 0..n {
        let rgba = render_frame_over(cut, f, w, h, [255, 255, 255]);
        write_png(&tmp.join(format!("frame_{:04}.png", f + 1)), w, h, &rgba)?;
    }

    let pattern = tmp.join("frame_%04d.png");
    // Max-compatibility H.264: yuv420p (the flag hardware decoders require),
    // High@4.1 (universal on modern phones/TVs/browsers), CRF 18 (visually
    // lossless for line art), +faststart (moov atom up front so the file
    // streams/scrubs immediately when shared). Odd dimensions crop by 1px
    // (yuv420p subsampling needs even sizes).
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-framerate",
            &state.fps().to_string(),
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
    Ok((n as usize, strokes_note(cut)))
}

/// A default export location proposal next to the project file, if any.
pub fn suggest_dir(state: &AppState) -> Option<PathBuf> {
    state.file_path.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

fn strokes_note(cut: &anim_core::model::Cut) -> String {
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
