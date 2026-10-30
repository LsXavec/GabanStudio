//! GM11 DELIVERY: what a finished cut becomes. Image sequences (PNG16,
//! TIFF16, EXR half, PNG8) are written here; video (H.264, H.265, ProRes,
//! DNxHR) goes through an ffmpeg the user installed, whose arguments are
//! pinned here (the app spawns it; this crate never does). Also the render
//! purposes (View / Final / LineTest), file-name patterns, burn-ins, the
//! delivery presets and the batch render's command line.
//!
//! Every delivered frame is the CPU golden render ([`crate::export`]), so a
//! delivery is exactly what the composite view shows (NEVER-DO 2).

use std::path::{Path, PathBuf};

use crate::color::ColorLaw;
use crate::command::CutRef;
use crate::export::{RenderEnv, composite_for, graph_composite_env};
use crate::fx::Image;
use crate::model::{Cut, LayerRole, Project};
use crate::text::{self, Face};

// ---- Purpose -----------------------------------------------------------------

/// What a render is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Purpose {
    /// Today's look: every visible layer (the composite view).
    #[default]
    View,
    /// The delivery: no Reference, Rough or Correction layers and no Ref
    /// columns.
    Final,
    /// The line test: the line roles only, black on white.
    LineTest,
}

impl Purpose {
    pub const ALL: [Purpose; 3] = [Purpose::View, Purpose::Final, Purpose::LineTest];

    pub fn label(self) -> &'static str {
        match self {
            Purpose::View => "view",
            Purpose::Final => "final",
            Purpose::LineTest => "line test",
        }
    }

    /// Whether a cel layer of `role` renders for this purpose.
    pub fn keeps(self, role: LayerRole) -> bool {
        match self {
            Purpose::View => true,
            Purpose::Final => !matches!(
                role,
                LayerRole::Reference | LayerRole::Rough | LayerRole::Correction
            ),
            Purpose::LineTest => role.is_line(),
        }
    }
}

/// How a delivered frame renders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOpts {
    pub purpose: Purpose,
    /// 1.0, or 0.5 for a half-size proxy (a 2×2 box average).
    pub scale: f32,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            purpose: Purpose::View,
            scale: 1.0,
        }
    }
}

/// `cut` with every drawing's layers cut down to what `purpose` keeps —
/// the graph path's sources then flatten exactly as
/// [`crate::model::Drawing::flatten_for`] does. Tiles are Arc'd: cheap.
pub fn purpose_cut(cut: &Cut, purpose: Purpose) -> Cut {
    let mut c = cut.clone();
    retain_purpose(&mut c, purpose);
    c
}

/// [`purpose_cut`] in place: a job's snapshot is filtered once, and each
/// frame then renders from it without another clone.
pub fn retain_purpose(cut: &mut Cut, purpose: Purpose) {
    for d in &mut cut.drawings {
        d.layers.retain(|l| purpose.keeps(l.props.role));
    }
}

/// Whether `cut` holds a layer that `purpose` leaves out.
fn drops_any(cut: &Cut, purpose: Purpose) -> bool {
    cut.drawings
        .iter()
        .any(|d| d.layers.iter().any(|l| !purpose.keeps(l.props.role)))
}

/// One frame for delivery, premultiplied, in stored values (D8), at the
/// project's `width × height` times `opts.scale`. View and Final run the
/// graph when its output is wired and the column stack otherwise (hidden
/// and Ref columns never render); LineTest is the sheet's line layers
/// only, black on opaque white (no graph: a line test checks the drawings
/// and their timing).
pub fn render(
    cut: &Cut,
    law: ColorLaw,
    frame: u32,
    width: u32,
    height: u32,
    opts: &RenderOpts,
) -> Image {
    render_env(cut, law, frame, width, height, opts, &RenderEnv::default())
}

/// GM17: [`render`] reading the project's folder (`env.base_dir`, where an
/// ImageSequence's relative paths resolve) and reporting what it could not
/// read (a missing frame: transparent, one warning) through `env`.
pub fn render_env(
    cut: &Cut,
    law: ColorLaw,
    frame: u32,
    width: u32,
    height: u32,
    opts: &RenderOpts,
    env: &RenderEnv,
) -> Image {
    let px = match opts.purpose {
        Purpose::View => graph_composite_env(cut, law, frame, width, height, env)
            .unwrap_or_else(|| composite_for(cut, frame, width, height, Purpose::View)),
        Purpose::Final => match cut.graph.output {
            Some(_) => {
                // A snapshot already filtered ([`retain_purpose`]) renders
                // as it is; only a cut with something to drop is cloned.
                let filtered;
                let source = if drops_any(cut, Purpose::Final) {
                    filtered = purpose_cut(cut, Purpose::Final);
                    &filtered
                } else {
                    cut
                };
                graph_composite_env(source, law, frame, width, height, env)
                    .unwrap_or_else(|| vec![0.0; width as usize * height as usize * 4])
            }
            None => composite_for(cut, frame, width, height, Purpose::Final),
        },
        Purpose::LineTest => {
            let mut px = composite_for(cut, frame, width, height, Purpose::LineTest);
            for p in px.chunks_exact_mut(4) {
                let ink = 1.0 - p[3].clamp(0.0, 1.0);
                p.copy_from_slice(&[ink, ink, ink, 1.0]);
            }
            px
        }
    };
    let img = Image::from_px(width, height, px);
    if opts.scale < 0.75 { half(&img) } else { img }
}

/// A half-size proxy: each output pixel the mean of its 2×2 block
/// (premultiplied, so edges stay clean).
fn half(img: &Image) -> Image {
    let (w, h) = ((img.w / 2).max(1), (img.h / 2).max(1));
    let mut out = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            let mut n = 0.0f32;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (sx, sy) = (2 * x + dx, 2 * y + dy);
                if sx < img.w && sy < img.h {
                    let p = img.pixel(sx, sy);
                    for c in 0..4 {
                        acc[c] += p[c];
                    }
                    n += 1.0;
                }
            }
            out.set_pixel(x, y, acc.map(|v| v / n));
        }
    }
    out
}

/// `img` laid over an opaque background chip (stored values) — for formats
/// and presets without alpha. Today's video background is white.
pub fn over(img: &Image, bg: [u8; 3]) -> Image {
    let b = bg.map(|v| v as f32 / 255.0);
    let mut out = img.clone();
    for p in out.px.chunks_exact_mut(4) {
        let keep = 1.0 - p[3].clamp(0.0, 1.0);
        for c in 0..3 {
            p[c] += b[c] * keep;
        }
        p[3] = 1.0;
    }
    out
}

// ---- Image sequences -----------------------------------------------------------

/// Straight (un-premultiplied) 16-bit RGBA of `img`: colour
/// `round(clamp(c / a) · 65535)`, alpha `round(clamp(a) · 65535)`, a fully
/// transparent pixel all zeros. What PNG16 and TIFF16 store.
pub fn quantize16(img: &Image) -> Vec<u16> {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let mut out = vec![0u16; img.px.len()];
    for (o, p) in out.chunks_exact_mut(4).zip(img.px.chunks_exact(4)) {
        let a = p[3].clamp(0.0, 1.0);
        if a > 0.0 {
            for c in 0..3 {
                o[c] = q(p[c] / a);
            }
        }
        o[3] = q(a);
    }
    out
}

fn save16(path: &Path, img: &Image, format: image::ImageFormat) -> Result<(), String> {
    let buf =
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(img.w, img.h, quantize16(img))
            .ok_or_else(|| format!("{}: frame buffer has the wrong size", path.display()))?;
    buf.save_with_format(path, format)
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// PNG, 16 bits per channel, straight RGBA ([`quantize16`]).
pub fn write_png16(path: &Path, img: &Image) -> Result<(), String> {
    save16(path, img, image::ImageFormat::Png)
}

/// TIFF, 16 bits per channel, straight RGBA ([`quantize16`]).
pub fn write_tiff16(path: &Path, img: &Image) -> Result<(), String> {
    save16(path, img, image::ImageFormat::Tiff)
}

/// PNG, 8 bits per channel, straight RGBA — today's PNG sequence bytes.
pub fn write_png8(path: &Path, img: &Image) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(img.px.len());
    for p in img.px.chunks_exact(4) {
        bytes.extend_from_slice(&crate::export::straight_rgba8(p));
    }
    let buf = image::RgbaImage::from_raw(img.w, img.h, bytes)
        .ok_or_else(|| format!("{}: frame buffer has the wrong size", path.display()))?;
    buf.save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// sRGB EOTF on one straight channel (0..1).
fn srgb_eotf(s: f32) -> f32 {
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// D8: EXR is scene-linear and premultiplied. A Legacy project stores
/// linear values already; an Srgb project stores the chip's gamma values,
/// so its colour is linearised (straight, then premultiplied again).
pub fn scene_linear(p: [f32; 4], law: ColorLaw) -> [f32; 4] {
    match law {
        ColorLaw::Legacy => p,
        ColorLaw::Srgb => {
            let a = p[3];
            if a <= 0.0 {
                return [0.0; 4];
            }
            let mut o = [0.0, 0.0, 0.0, a];
            for c in 0..3 {
                o[c] = srgb_eotf((p[c] / a).clamp(0.0, 1.0)) * a;
            }
            o
        }
    }
}

/// OpenEXR, half-float RGBA, scene-linear and premultiplied
/// ([`scene_linear`]).
pub fn write_exr(path: &Path, img: &Image, law: ColorLaw) -> Result<(), String> {
    use exr::prelude::f16;
    exr::prelude::write_rgba_file(path, img.w as usize, img.h as usize, |x, y| {
        let p = scene_linear(img.pixel(x as u32, y as u32), law);
        (
            f16::from_f32(p[0]),
            f16::from_f32(p[1]),
            f16::from_f32(p[2]),
            f16::from_f32(p[3]),
        )
    })
    .map_err(|e| format!("write {}: {e}", path.display()))
}

/// Write one frame in an image-sequence `format` (a video format is an
/// error: video frames go to ffmpeg).
pub fn write_frame(path: &Path, img: &Image, format: Format, law: ColorLaw) -> Result<(), String> {
    match format {
        Format::Png16 => write_png16(path, img),
        Format::Tiff16 => write_tiff16(path, img),
        Format::Exr => write_exr(path, img, law),
        Format::Png8 => write_png8(path, img),
        Format::Video(_) => Err("a video frame goes to ffmpeg, not to a file".into()),
    }
}

// ---- Video ---------------------------------------------------------------------

/// A video codec ffmpeg delivers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    H264,
    H265,
    ProRes422Hq,
    ProRes4444,
    DnxhrHq,
}

impl Codec {
    /// The container: MP4 for the review codecs, QuickTime for masters.
    pub fn ext(self) -> &'static str {
        match self {
            Codec::H264 | Codec::H265 => "mp4",
            Codec::ProRes422Hq | Codec::ProRes4444 | Codec::DnxhrHq => "mov",
        }
    }
}

/// The cut's audio under a video: its WAV on disk, where the render's first
/// frame falls in it, and how long the video runs (seconds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioIn<'a> {
    pub path: &'a Path,
    pub offset: f64,
    pub secs: f64,
}

/// GM12: the cut's mix (every sounding track, [`crate::audio::mix`]) over
/// frames `from..=to`, interleaved stereo at [`crate::audio::MIX_RATE`];
/// None when nothing sounds.
pub fn mixdown(cut: &Cut, fps: u32, from: u32, to: u32) -> Option<Vec<f32>> {
    if !crate::audio::has_sound(cut) {
        return None;
    }
    let rate = crate::audio::MIX_RATE as u64;
    let fps = fps.max(1) as u64;
    let mut all = crate::audio::mix(cut, fps as u32, to.saturating_add(1), rate as u32);
    let skip = ((from as u64 * rate / fps) as usize * 2).min(all.len());
    all.drain(..skip);
    Some(all)
}

/// GM12: write the mixdown of frames `from..=to` to `path` as a 24-bit
/// stereo WAV (hound). Ok(false), and no file, when nothing sounds. A
/// video job writes the whole cut's (frames 0 to the end) and muxes it
/// through [`ffmpeg_args`] exactly as v0.3 muxed its one clip; an image
/// sequence gets its range's beside the frames.
pub fn write_mixdown(path: &Path, cut: &Cut, fps: u32, from: u32, to: u32) -> Result<bool, String> {
    match mixdown(cut, fps, from, to) {
        Some(mix) => crate::audio::write_wav(path, &mix, crate::audio::MIX_RATE).map(|_| true),
        None => Ok(false),
    }
}

// ---- GM15: the episode -------------------------------------------------------

/// GM15: the episode — every cut of a scene back to back, in the scene's
/// order ([`crate::board::episode_frame_map`]) — as the render queue's
/// Episode job renders it: one frame source over all the cuts, and their
/// mixdowns laid end to end. The cuts are snapshots (Arc'd tiles, cheap),
/// so a render thread owns them.
#[derive(Clone)]
pub struct Episode {
    /// The scene's cuts, in order (a Final job's filtered once, here).
    pub cuts: Vec<Cut>,
    /// Each cut's first frame in the episode.
    pub starts: Vec<u32>,
    /// The episode's names for files: the cut field is "episode".
    pub names: Names,
}

impl Episode {
    /// The episode of `scene` (None when the scene is gone or has no cut).
    pub fn of(project: &Project, scene: crate::ids::SceneId, purpose: Purpose) -> Option<Episode> {
        let s = project.scene(scene)?;
        if s.cuts.is_empty() {
            return None;
        }
        let mut at = 0u32;
        let mut starts = Vec::with_capacity(s.cuts.len());
        let cuts = s
            .cuts
            .iter()
            .map(|c| {
                starts.push(at);
                at = at.saturating_add(c.frame_count);
                let mut snapshot = c.clone();
                if purpose == Purpose::Final {
                    retain_purpose(&mut snapshot, Purpose::Final);
                }
                snapshot
            })
            .collect();
        Some(Episode {
            cuts,
            starts,
            names: Names {
                project: project.name.clone(),
                scene: s.name.clone(),
                cut: "episode".into(),
            },
        })
    }

    /// Frames in the whole episode.
    pub fn frame_count(&self) -> u32 {
        self.cuts.iter().fold(0u32, |n, c| n.saturating_add(c.frame_count))
    }

    /// Episode frame `frame` as (cut index, frame within that cut).
    pub fn locate(&self, frame: u32) -> Option<(usize, u32)> {
        let i = self.starts.partition_point(|s| *s <= frame).checked_sub(1)?;
        let local = frame - self.starts[i];
        (local < self.cuts[i].frame_count).then_some((i, local))
    }

    /// Episode frame `frame`, rendered as its own cut renders it.
    pub fn render(
        &self,
        frame: u32,
        law: ColorLaw,
        width: u32,
        height: u32,
        opts: &RenderOpts,
    ) -> Option<Image> {
        let (i, f) = self.locate(frame)?;
        Some(render(&self.cuts[i], law, f, width, height, opts))
    }

    /// The cuts' mixdowns laid end to end, interleaved stereo at
    /// [`crate::audio::MIX_RATE`]: each cut fills exactly its frames' slot
    /// (cut to it, or silence after a short clip; a silent cut is silence),
    /// so sound stays on its pictures. None when no cut sounds.
    pub fn mixdown(&self, fps: u32) -> Option<Vec<f32>> {
        if !self.cuts.iter().any(crate::audio::has_sound) {
            return None;
        }
        let rate = crate::audio::MIX_RATE as u64;
        let fps = fps.max(1) as u64;
        let at = |frame: u64| (frame * rate / fps) as usize * 2;
        let mut out = Vec::with_capacity(at(self.frame_count() as u64));
        for (c, start) in self.cuts.iter().zip(&self.starts) {
            let start = *start as u64;
            let slot = at(start + c.frame_count as u64) - at(start);
            match mixdown(c, fps as u32, 0, c.frame_count.saturating_sub(1)) {
                Some(mut mix) => {
                    mix.resize(slot, 0.0);
                    out.extend(mix);
                }
                None => out.resize(out.len() + slot, 0.0),
            }
        }
        Some(out)
    }

    /// Write [`Episode::mixdown`] to `path` (24-bit stereo WAV); Ok(false),
    /// and no file, when no cut sounds.
    pub fn write_mixdown(&self, path: &Path, fps: u32) -> Result<bool, String> {
        match self.mixdown(fps) {
            Some(mix) => crate::audio::write_wav(path, &mix, crate::audio::MIX_RATE).map(|_| true),
            None => Ok(false),
        }
    }
}

/// Crop odd sizes by a pixel: 4:2:0 and 4:2:2 need even dimensions.
const EVEN: &str = "crop=trunc(iw/2)*2:trunc(ih/2)*2";

/// The pinned ffmpeg arguments for one video delivery. Frames arrive as
/// rawvideo RGBA on stdin (`w × h` at `fps`); `alpha` keeps ProRes 4444's
/// alpha. With audio, the WAV rides along exactly as v0.3's MP4 export
/// muxed it — input seek to the range's start, trimmed to the video's
/// length — as AAC 192k in MP4 and as 24-bit PCM in QuickTime. Without
/// audio there is no audio argument at all.
#[allow(clippy::too_many_arguments)]
pub fn ffmpeg_args(
    codec: Codec,
    fps: u32,
    w: u32,
    h: u32,
    alpha: bool,
    audio: Option<AudioIn<'_>>,
    out: &Path,
) -> Vec<String> {
    let fps = fps.max(1);
    let mut a: Vec<String> = [
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-video_size",
        &format!("{w}x{h}"),
        "-framerate",
        &fps.to_string(),
        "-i",
        "-",
    ]
    .map(String::from)
    .to_vec();
    if let Some(au) = audio {
        a.extend(
            [
                "-ss",
                &format!("{:.6}", au.offset),
                "-i",
                &au.path.to_string_lossy(),
                "-map",
                "0:v",
                "-map",
                "1:a",
            ]
            .map(String::from),
        );
        let codec_a: &[&str] = match codec {
            Codec::H264 | Codec::H265 => &["-c:a", "aac", "-b:a", "192k"],
            _ => &["-c:a", "pcm_s24le"],
        };
        a.extend(codec_a.iter().map(|s| s.to_string()));
        a.extend(["-t".to_string(), format!("{:.6}", au.secs)]);
    }
    let video: &[&str] = match codec {
        // v0.3's MP4: max-compatibility H.264 (High@4.1, yuv420p, CRF 18,
        // the moov atom up front).
        Codec::H264 => &[
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
            EVEN,
        ],
        Codec::H265 => &[
            "-c:v",
            "libx265",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "20",
            "-preset",
            "medium",
            "-tag:v",
            "hvc1",
            "-movflags",
            "+faststart",
            "-vf",
            EVEN,
        ],
        Codec::ProRes422Hq => &[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "3",
            "-pix_fmt",
            "yuv422p10le",
            "-vendor",
            "apl0",
            "-vf",
            EVEN,
        ],
        Codec::ProRes4444 if alpha => &[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "4",
            "-pix_fmt",
            "yuva444p10le",
            "-vendor",
            "apl0",
        ],
        Codec::ProRes4444 => &[
            "-c:v",
            "prores_ks",
            "-profile:v",
            "4",
            "-pix_fmt",
            "yuv444p10le",
            "-vendor",
            "apl0",
        ],
        Codec::DnxhrHq => &[
            "-c:v",
            "dnxhd",
            "-profile:v",
            "dnxhr_hq",
            "-pix_fmt",
            "yuv422p",
            "-vf",
            EVEN,
        ],
    };
    a.extend(video.iter().map(|s| s.to_string()));
    a.push(out.to_string_lossy().into_owned());
    a
}

// ---- Presets -------------------------------------------------------------------

/// What a delivery writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Png16,
    Tiff16,
    Exr,
    Png8,
    Video(Codec),
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Png16 | Format::Png8 => "png",
            Format::Tiff16 => "tif",
            Format::Exr => "exr",
            Format::Video(c) => c.ext(),
        }
    }

    pub fn is_video(self) -> bool {
        matches!(self, Format::Video(_))
    }
}

/// Which corner texts a frame carries (Plex Mono, white on a dark band).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BurnIns {
    pub project: bool,
    pub cut: bool,
    /// The frame number and its seconds+koma.
    pub frame: bool,
    pub timecode: bool,
    pub date: bool,
    pub artist: bool,
}

impl BurnIns {
    pub const NONE: BurnIns = BurnIns {
        project: false,
        cut: false,
        frame: false,
        timecode: false,
        date: false,
        artist: false,
    };
    pub const REVIEW: BurnIns = BurnIns {
        project: true,
        cut: true,
        frame: true,
        timecode: true,
        date: true,
        artist: true,
    };
    pub const LINE_TEST: BurnIns = BurnIns {
        project: false,
        cut: true,
        frame: true,
        timecode: true,
        date: false,
        artist: false,
    };

    pub fn is_empty(self) -> bool {
        self == BurnIns::NONE
    }
}

/// A delivery preset: what it writes and how its frames render.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub format: Format,
    /// Keep alpha; otherwise frames lie on white.
    pub alpha: bool,
    pub burn_ins: BurnIns,
    pub purpose: Purpose,
    /// 1.0 or 0.5.
    pub scale: f32,
}

const fn preset(
    name: &'static str,
    format: Format,
    alpha: bool,
    burn_ins: BurnIns,
    purpose: Purpose,
) -> Preset {
    Preset {
        name,
        format,
        alpha,
        burn_ins,
        purpose,
        scale: 1.0,
    }
}

/// Every delivery preset, image sequences first (they need no ffmpeg).
pub const PRESETS: &[Preset] = &[
    preset(
        "Master PNG 16-bit",
        Format::Png16,
        true,
        BurnIns::NONE,
        Purpose::Final,
    ),
    preset(
        "TIFF 16-bit",
        Format::Tiff16,
        true,
        BurnIns::NONE,
        Purpose::Final,
    ),
    preset(
        "EXR half (linear)",
        Format::Exr,
        true,
        BurnIns::NONE,
        Purpose::Final,
    ),
    preset(
        "PNG 8-bit",
        Format::Png8,
        true,
        BurnIns::NONE,
        Purpose::View,
    ),
    // The animator's douga/genga check as frames: works without ffmpeg.
    preset(
        "Line test PNG",
        Format::Png8,
        false,
        BurnIns::LINE_TEST,
        Purpose::LineTest,
    ),
    preset(
        "H.264 review",
        Format::Video(Codec::H264),
        false,
        BurnIns::REVIEW,
        Purpose::View,
    ),
    preset(
        "H.265",
        Format::Video(Codec::H265),
        false,
        BurnIns::NONE,
        Purpose::Final,
    ),
    preset(
        "ProRes 422 HQ",
        Format::Video(Codec::ProRes422Hq),
        false,
        BurnIns::NONE,
        Purpose::Final,
    ),
    preset(
        "ProRes 4444 (alpha)",
        Format::Video(Codec::ProRes4444),
        true,
        BurnIns::NONE,
        Purpose::Final,
    ),
    preset(
        "DNxHR HQ",
        Format::Video(Codec::DnxhrHq),
        false,
        BurnIns::NONE,
        Purpose::Final,
    ),
    preset(
        "Line test",
        Format::Video(Codec::H264),
        false,
        BurnIns::LINE_TEST,
        Purpose::LineTest,
    ),
];

impl Preset {
    /// The preset called `name` (case and surrounding space ignored).
    pub fn named(name: &str) -> Option<&'static Preset> {
        let n = name.trim();
        PRESETS.iter().find(|p| p.name.eq_ignore_ascii_case(n))
    }

    pub fn opts(&self) -> RenderOpts {
        RenderOpts {
            purpose: self.purpose,
            scale: self.scale,
        }
    }

    /// The output size of a `width × height` project under this preset.
    pub fn out_size(&self, width: u32, height: u32) -> (u32, u32) {
        if self.scale < 0.75 {
            ((width / 2).max(1), (height / 2).max(1))
        } else {
            (width, height)
        }
    }
}

// ---- Names and burn-ins --------------------------------------------------------

/// The names a file-name pattern and a burn-in can say.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Names {
    pub project: String,
    pub scene: String,
    pub cut: String,
}

/// The default file-name pattern.
pub const DEFAULT_PATTERN: &str = "{project}_{scene}_{cut}_{frame:04}";

/// A file-name part with every character a path cannot hold (`< > : " / \
/// | ? *` and control characters) turned into `_`.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Expand `pattern` for frame number `frame` (counted from 1, as the sheet
/// counts) and add `.ext`. Tokens: `{project}`, `{scene}`, `{cut}`,
/// `{frame}` and `{frame:0N}` (zero-padded to N, at most
/// [`MAX_FRAME_PAD`]); an unknown token stays as
/// written. The result is one file name: characters a path cannot hold
/// become `_`, and trailing dots and spaces (which Windows drops) go.
pub fn expand_pattern(pattern: &str, names: &Names, frame: u32, ext: &str) -> String {
    expand(pattern, names, Some(frame), ext)
}

/// A video's file name: the pattern without its frame token (and the
/// separator it leaves dangling at the end).
pub fn video_name(pattern: &str, names: &Names, ext: &str) -> String {
    expand(pattern, names, None, ext)
}

/// The widest `{frame:0N}` pad: a u32 frame has at most 10 digits, and a
/// wider one (`{frame:070000}`) would overflow the formatter.
pub const MAX_FRAME_PAD: usize = 10;

/// Whether `pattern` names the frame (`{frame}` or `{frame:0N}`) — else
/// every frame of an image sequence would get one file name.
pub fn has_frame_token(pattern: &str) -> bool {
    let n = Names::default();
    expand(pattern, &n, Some(1), "") != expand(pattern, &n, Some(2), "")
}

/// An image sequence's pattern: `pattern`, with `_{frame:04}` added when it
/// does not name the frame (each frame keeps its own file).
pub fn sequence_pattern(pattern: &str) -> String {
    if has_frame_token(pattern) {
        pattern.to_string()
    } else {
        format!("{pattern}_{{frame:04}}")
    }
}

fn expand(pattern: &str, names: &Names, frame: Option<u32>, ext: &str) -> String {
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let token = &after[..close];
        let value = match token {
            "project" => Some(names.project.clone()),
            "scene" => Some(names.scene.clone()),
            "cut" => Some(names.cut.clone()),
            "frame" => Some(frame.map(|f| f.to_string()).unwrap_or_default()),
            t => t
                .strip_prefix("frame:")
                .and_then(|w| w.parse::<usize>().ok())
                .map(|w| w.min(MAX_FRAME_PAD))
                .map(|w| frame.map(|f| format!("{f:0w$}")).unwrap_or_default()),
        };
        match value {
            Some(v) => out.push_str(&v),
            None => {
                out.push('{');
                out.push_str(token);
                out.push('}');
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    let mut stem = sanitize(&out);
    let trim: &[char] = if frame.is_some() {
        &['.', ' ']
    } else {
        &['.', ' ', '_', '-']
    };
    while stem.ends_with(trim) {
        stem.pop();
    }
    if stem.is_empty() {
        stem.push_str("render");
    }
    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

/// Frame `frame` (0-based) as seconds+koma, the sheet's count: frame 30 at
/// 24 fps is "1+07" (koma counted from 1, like the sheet's rows).
pub fn seconds_koma(frame: u32, fps: u32) -> String {
    let fps = fps.max(1);
    format!("{}+{:02}", frame / fps, frame % fps + 1)
}

/// Non-drop timecode HH:MM:SS:FF of frame `frame` (0-based).
pub fn timecode(frame: u32, fps: u32) -> String {
    let fps = fps.max(1);
    let (secs, ff) = (frame / fps, frame % fps);
    format!(
        "{:02}:{:02}:{:02}:{:02}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        ff
    )
}

/// What a burn-in states besides the frame: the names, the rate, the
/// render's date and the artist.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Slate {
    pub names: Names,
    pub fps: u32,
    pub date: String,
    pub artist: String,
}

/// Stamp the chosen burn-ins on `img` at frame `frame` (0-based): project
/// and cut top left, date and artist top right, the frame and its s+k
/// bottom left, the timecode bottom right — white Plex Mono, each on a
/// dark band, sized to the frame's height. No burn-ins leave `img`
/// untouched.
pub fn burn_in(img: &mut Image, which: BurnIns, slate: &Slate, frame: u32) {
    if which.is_empty() {
        return;
    }
    let join = |parts: &[(bool, String)]| {
        parts
            .iter()
            .filter(|(on, s)| *on && !s.is_empty())
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>()
            .join("  ")
    };
    let n = &slate.names;
    let top_left = join(&[
        (which.project, n.project.clone()),
        (
            which.cut,
            [n.scene.as_str(), n.cut.as_str()]
                .join(" ")
                .trim()
                .to_string(),
        ),
    ]);
    let top_right = join(&[
        (which.date, slate.date.clone()),
        (which.artist, slate.artist.clone()),
    ]);
    let bottom_left = join(&[(
        which.frame,
        format!("{:04}  {}", frame + 1, seconds_koma(frame, slate.fps)),
    )]);
    let bottom_right = join(&[(which.timecode, timecode(frame, slate.fps))]);
    let px = (img.h as f32 / 28.0).clamp(8.0, 48.0);
    let pad = (px * 0.3).round() as i64;
    let margin = (px * 0.5).round() as i64;
    for (text, right, bottom) in [
        (top_left, false, false),
        (top_right, true, false),
        (bottom_left, false, true),
        (bottom_right, true, true),
    ] {
        if text.is_empty() {
            continue;
        }
        let glyphs = text::raster_line(Face::Mono, &text, px, [255, 255, 255, 255]);
        let (bw, bh) = (glyphs.w as i64 + 2 * pad, glyphs.h as i64 + 2 * pad);
        let x0 = if right {
            img.w as i64 - margin - bw
        } else {
            margin
        };
        let y0 = if bottom {
            img.h as i64 - margin - bh
        } else {
            margin
        };
        fill_over(img, x0, y0, bw, bh, [0.0, 0.0, 0.0, 0.6]);
        paste_over(img, &glyphs, x0 + pad, y0 + pad);
    }
}

/// Premultiplied src-over of a flat colour over a rectangle (clipped).
fn fill_over(img: &mut Image, x0: i64, y0: i64, w: i64, h: i64, c: [f32; 4]) {
    let keep = 1.0 - c[3];
    for y in y0.max(0)..(y0 + h).min(img.h as i64) {
        for x in x0.max(0)..(x0 + w).min(img.w as i64) {
            let i = (y as usize * img.w as usize + x as usize) * 4;
            for k in 0..4 {
                img.px[i + k] = c[k] + img.px[i + k] * keep;
            }
        }
    }
}

/// Premultiplied src-over of `top` with its corner at (x0, y0) (clipped).
fn paste_over(img: &mut Image, top: &Image, x0: i64, y0: i64) {
    for ty in 0..top.h as i64 {
        let y = y0 + ty;
        if y < 0 || y >= img.h as i64 {
            continue;
        }
        for tx in 0..top.w as i64 {
            let x = x0 + tx;
            if x < 0 || x >= img.w as i64 {
                continue;
            }
            let s = top.pixel(tx as u32, ty as u32);
            let i = (y as usize * img.w as usize + x as usize) * 4;
            let keep = 1.0 - s[3];
            for k in 0..4 {
                img.px[i + k] = s[k] + img.px[i + k] * keep;
            }
        }
    }
}

/// One delivered frame under `preset`: rendered for its purpose and scale,
/// laid on white when it keeps no alpha, then burnt in.
#[allow(clippy::too_many_arguments)]
pub fn deliver_frame(
    cut: &Cut,
    law: ColorLaw,
    frame: u32,
    size: (u32, u32),
    preset: &Preset,
    burn_ins: BurnIns,
    slate: &Slate,
) -> Image {
    deliver_frame_env(cut, law, frame, size, preset, burn_ins, slate, &RenderEnv::default())
}

/// GM17: [`deliver_frame`] reading the project's folder and reporting
/// through `env` (see [`render_env`]).
#[allow(clippy::too_many_arguments)]
pub fn deliver_frame_env(
    cut: &Cut,
    law: ColorLaw,
    frame: u32,
    (width, height): (u32, u32),
    preset: &Preset,
    burn_ins: BurnIns,
    slate: &Slate,
    env: &RenderEnv,
) -> Image {
    let mut img = render_env(cut, law, frame, width, height, &preset.opts(), env);
    if !preset.alpha {
        img = over(&img, [255, 255, 255]);
    }
    burn_in(&mut img, burn_ins, slate, frame);
    img
}

// ---- The batch render's command line --------------------------------------------

/// What the batch render renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliSource {
    File(PathBuf),
    /// The welcome screen's sample cut, built in memory.
    Sample,
}

/// `animstudio --render <file.animproj | --sample> --preset <name> --out <dir>
/// [--scene <name>] [--cut <name>]`.
#[derive(Debug, Clone, PartialEq)]
pub struct CliJob {
    pub source: CliSource,
    pub preset: &'static Preset,
    pub out: PathBuf,
    /// `--scene`: the scene to look in (None = every scene).
    pub scene: Option<String>,
    /// `--cut`: the cut to render (None = the first cut, see [`pick_cut`]).
    pub cut: Option<String>,
}

/// Exit codes of the batch render.
pub const EXIT_OK: i32 = 0;
pub const EXIT_FAILED: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

pub const CLI_USAGE: &str = "usage: animstudio --render <file.animproj | --sample> --preset <name> \
     --out <dir> [--scene <name>] [--cut <name>]";

/// The cut a batch job renders: the cut named `cut` in the scene named
/// `scene` (names ignore case; an unnamed scene is any scene), else the
/// first cut of that scene, else of the project. Err says what is there.
pub fn pick_cut(
    project: &Project,
    scene: Option<&str>,
    cut: Option<&str>,
) -> Result<CutRef, String> {
    let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
    let scenes: Vec<_> = project
        .scenes
        .iter()
        .filter(|s| scene.is_none_or(|n| same(&s.name, n)))
        .collect();
    if scenes.is_empty() {
        let all: Vec<&str> = project.scenes.iter().map(|s| s.name.as_str()).collect();
        return Err(match scene {
            Some(n) => format!("no scene named \"{n}\" — scenes: {}", all.join(", ")),
            None => "the project has no scene".into(),
        });
    }
    let found = scenes.iter().find_map(|s| {
        s.cuts
            .iter()
            .find(|c| cut.is_none_or(|n| same(&c.name, n)))
            .map(|c| CutRef {
                scene: s.id,
                cut: c.id,
            })
    });
    found.ok_or_else(|| match cut {
        Some(n) => {
            let all: Vec<&str> = scenes
                .iter()
                .flat_map(|s| s.cuts.iter().map(|c| c.name.as_str()))
                .collect();
            format!("no cut named \"{n}\" — cuts: {}", all.join(", "))
        }
        None => "the project has no cut".into(),
    })
}

/// Whether the command line asks for a batch render.
pub fn is_cli(args: &[String]) -> bool {
    args.iter().any(|a| a == "--render")
}

/// Parse the batch render's arguments (the program name left off). An
/// `Err` is a usage error (exit [`EXIT_USAGE`]) that says why.
pub fn parse_cli(args: &[String]) -> Result<CliJob, String> {
    let (mut source, mut preset, mut out) = (None, None, None);
    let (mut scene, mut cut) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value\n{CLI_USAGE}"))
        };
        match a.as_str() {
            "--render" => {
                let v = value("--render")?;
                source = Some(if v == "--sample" {
                    CliSource::Sample
                } else {
                    CliSource::File(PathBuf::from(v))
                });
            }
            "--preset" => {
                let v = value("--preset")?;
                let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
                preset = Some(Preset::named(&v).ok_or_else(|| {
                    format!("unknown preset \"{v}\" — one of: {}", names.join(", "))
                })?);
            }
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--scene" => scene = Some(value("--scene")?),
            "--cut" => cut = Some(value("--cut")?),
            other => return Err(format!("unknown argument \"{other}\"\n{CLI_USAGE}")),
        }
    }
    Ok(CliJob {
        source: source.ok_or_else(|| format!("--render needs a file or --sample\n{CLI_USAGE}"))?,
        preset: preset.ok_or_else(|| format!("--preset is missing\n{CLI_USAGE}"))?,
        out: out.ok_or_else(|| format!("--out is missing\n{CLI_USAGE}"))?,
        scene,
        cut,
    })
}
