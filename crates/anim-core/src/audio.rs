//! GM12 SOUND: a cut's sound tracks (dialogue, SE, music) — decoding (WAV,
//! MP3, OGG Vorbis and FLAC through symphonia, routed by the file's magic
//! bytes), the mix (offset, gain, mute, solo), the per-frame RMS envelope
//! the sheet's waveform and the lip flap read, the 3-mouth lip flap (D14)
//! and the mixdown WAV deliveries mux.
//!
//! The first track is `Cut.audio` (the v0.3 scratch clip, so its API, its
//! `cut_audio` row and its tests are unchanged) with its settings in
//! `Cut.audio_props`; the tracks after it are `Cut.tracks`. Commands number
//! the tracks from 0 = the first. Audio never enters the render graph, and
//! the engine stays headless: playback is the app's.

use std::path::Path;

use crate::command::{Command, CutRef};
use crate::error::{EngineError, Result};
use crate::ids::{ColumnId, DrawingId};
use crate::model::{AudioClip, Cut, MAX_AUDIO_BYTES, MAX_AUDIO_SAMPLES, Project};
use crate::xsheet::Exposure;

/// The rate every mix runs at (playback and deliveries).
pub const MIX_RATE: u32 = 48_000;
/// A track's level range in dB (0 = as recorded; mute silences).
pub const MIN_GAIN_DB: f32 = -60.0;
pub const MAX_GAIN_DB: f32 = 24.0;

// ---- Tracks --------------------------------------------------------------------

/// What a track carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum TrackKind {
    #[default]
    Dialogue,
    Effects,
    Music,
    Other,
}

impl TrackKind {
    pub const ALL: [TrackKind; 4] = [
        TrackKind::Dialogue,
        TrackKind::Effects,
        TrackKind::Music,
        TrackKind::Other,
    ];

    pub fn key(self) -> &'static str {
        match self {
            TrackKind::Dialogue => "dialogue",
            TrackKind::Effects => "effects",
            TrackKind::Music => "music",
            TrackKind::Other => "other",
        }
    }

    /// An unknown key (a later build's kind) reads as Other, never an error.
    pub fn from_key(key: &str) -> TrackKind {
        TrackKind::ALL
            .into_iter()
            .find(|k| k.key() == key)
            .unwrap_or(TrackKind::Other)
    }

    /// The house label: the studio term plus English.
    pub fn label(self) -> &'static str {
        match self {
            TrackKind::Dialogue => "serifu (dialogue)",
            TrackKind::Effects => "SE (effects)",
            TrackKind::Music => "BGM (music)",
            TrackKind::Other => "other",
        }
    }
}

/// A track's mix settings (its clip aside).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackProps {
    pub name: String,
    pub kind: TrackKind,
    /// The sheet frame the clip's first sample sounds on (negative = it
    /// starts before the cut). Frame-indexed: insert and delete frames
    /// ripple it (D4).
    pub offset: i32,
    /// Level in dB, within [`MIN_GAIN_DB`, `MAX_GAIN_DB`].
    pub gain_db: f32,
    pub mute: bool,
    pub solo: bool,
}

impl Default for TrackProps {
    /// The first track as v0.3 played it: dialogue from frame 0, as recorded.
    fn default() -> Self {
        TrackProps::new("dialogue", TrackKind::Dialogue)
    }
}

impl TrackProps {
    pub fn new(name: impl Into<String>, kind: TrackKind) -> Self {
        Self {
            name: name.into(),
            kind,
            offset: 0,
            gain_db: 0.0,
            mute: false,
            solo: false,
        }
    }

    /// The linear gain of `gain_db`.
    pub fn gain(&self) -> f32 {
        10f32.powf(self.gain_db / 20.0)
    }

    /// Settings a command may set: a finite gain inside the range.
    pub fn check(&self) -> Result<()> {
        if !self.gain_db.is_finite() || !(MIN_GAIN_DB..=MAX_GAIN_DB).contains(&self.gain_db) {
            return Err(EngineError::InvalidCommand(format!(
                "track gain {} dB outside {MIN_GAIN_DB}..={MAX_GAIN_DB}",
                self.gain_db
            )));
        }
        Ok(())
    }

    /// The `audio_tracks` row's JSON (with the clip's file name beside).
    pub fn to_json(&self, file: &str) -> String {
        serde_json::json!({
            "name": self.name,
            "kind": self.kind.key(),
            "offset": self.offset,
            "gain_db": self.gain_db,
            "mute": self.mute,
            "solo": self.solo,
            "file": file,
        })
        .to_string()
    }

    /// Read a row's JSON: missing keys take the defaults and an unknown
    /// kind reads as Other (a later build's row still opens); a gain out of
    /// range is clamped (files are a trust boundary). Returns the settings
    /// and the clip's file name; None when the text is not a JSON object.
    pub fn from_json(text: &str) -> Option<(TrackProps, String)> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let o = v.as_object()?;
        let d = TrackProps::default();
        let gain = o.get("gain_db").and_then(|g| g.as_f64()).unwrap_or(0.0) as f32;
        let props = TrackProps {
            name: o
                .get("name")
                .and_then(|n| n.as_str())
                .map_or(d.name, str::to_string),
            kind: o
                .get("kind")
                .and_then(|k| k.as_str())
                .map_or(d.kind, TrackKind::from_key),
            offset: o
                .get("offset")
                .and_then(|n| n.as_i64())
                .map_or(0, |n| n.clamp(i32::MIN as i64, i32::MAX as i64) as i32),
            gain_db: if gain.is_finite() {
                gain.clamp(MIN_GAIN_DB, MAX_GAIN_DB)
            } else {
                0.0
            },
            mute: o.get("mute").and_then(|b| b.as_bool()).unwrap_or(false),
            solo: o.get("solo").and_then(|b| b.as_bool()).unwrap_or(false),
        };
        let file = o
            .get("file")
            .and_then(|f| f.as_str())
            .unwrap_or("")
            .to_string();
        Some((props, file))
    }
}

/// A track after the first: its settings and its clip.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AudioTrack {
    pub props: TrackProps,
    pub clip: AudioClip,
}

/// Every track of the cut that has a clip, in order: (number, settings,
/// clip). Number 0 is the first track (`Cut.audio`); it is absent when the
/// cut has no first clip, and the rest keep their numbers.
pub fn tracks(cut: &Cut) -> Vec<(usize, &TrackProps, &AudioClip)> {
    cut.audio
        .iter()
        .map(|c| (0, &cut.audio_props, c))
        .chain(
            cut.tracks
                .iter()
                .enumerate()
                .map(|(i, t)| (i + 1, &t.props, &t.clip)),
        )
        .collect()
}

/// Track `index`'s settings (0 = the first track, which has settings even
/// without a clip).
pub fn track_props(cut: &Cut, index: usize) -> Option<&TrackProps> {
    match index {
        0 => Some(&cut.audio_props),
        i => cut.tracks.get(i - 1).map(|t| &t.props),
    }
}

/// Track `index`'s clip, if it has one.
pub fn track_clip(cut: &Cut, index: usize) -> Option<&AudioClip> {
    match index {
        0 => cut.audio.as_ref(),
        i => cut.tracks.get(i - 1).map(|t| &t.clip),
    }
}

/// The mute and solo rules: a muted track never sounds (mute wins), and
/// while any track is soloed only the soloed ones sound.
pub fn sounds(props: &TrackProps, any_solo: bool) -> bool {
    !props.mute && (!any_solo || props.solo)
}

/// Whether any track of the cut is soloed.
pub fn any_solo(cut: &Cut) -> bool {
    tracks(cut).iter().any(|(_, p, _)| p.solo)
}

/// Whether anything in the cut sounds at all.
pub fn has_sound(cut: &Cut) -> bool {
    let solo = any_solo(cut);
    tracks(cut).iter().any(|(_, p, _)| sounds(p, solo))
}

// ---- Decoding ------------------------------------------------------------------

/// A container the decoder knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Wav,
    Flac,
    Mp3,
    Ogg,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Wav => "wav",
            Format::Flac => "flac",
            Format::Mp3 => "mp3",
            Format::Ogg => "ogg",
        }
    }
}

/// The format a file's first bytes declare — never its extension (a
/// renamed file still routes right).
pub fn sniff(bytes: &[u8]) -> Option<Format> {
    let b = |r: std::ops::Range<usize>| bytes.get(r).unwrap_or(&[]);
    if (b(0..4) == b"RIFF" || b(0..4) == b"RF64") && b(8..12) == b"WAVE" {
        Some(Format::Wav)
    } else if b(0..4) == b"fLaC" {
        Some(Format::Flac)
    } else if b(0..4) == b"OggS" {
        Some(Format::Ogg)
    } else if b(0..3) == b"ID3" || (bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0) {
        Some(Format::Mp3)
    } else {
        None
    }
}

/// Decode a WAV, MP3, OGG Vorbis or FLAC file to interleaved f32 samples in
/// [-1, 1]: (sample rate, channels, samples). The format comes from the
/// magic bytes; `ext_hint` is used only when they say nothing. A damaged
/// packet is skipped; every other failure is an `Err`, never a panic.
pub fn decode(bytes: &[u8], ext_hint: &str) -> std::result::Result<(u32, u16, Vec<f32>), String> {
    use symphonia::core::audio::{SampleBuffer, SignalSpec};
    use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
    use symphonia::core::errors::Error as DecodeError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    if bytes.len() > MAX_AUDIO_BYTES {
        return Err(format!(
            "audio: too large ({} MB; max {} MB)",
            bytes.len() / (1024 * 1024),
            MAX_AUDIO_BYTES / (1024 * 1024)
        ));
    }
    let mut hint = Hint::new();
    match sniff(bytes) {
        Some(f) => {
            hint.with_extension(f.ext());
        }
        None if !ext_hint.is_empty() => {
            hint.with_extension(ext_hint.trim_start_matches('.'));
        }
        None => {}
    }
    let source = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes.to_vec())), Default::default());
    // Gapless: MP3 and Vorbis drop the encoder's delay and padding, so a
    // clip lasts what was recorded and lines up with the sheet.
    let opts = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let probed = symphonia::default::get_probe()
        .format(&hint, source, &opts, &MetadataOptions::default())
        .map_err(|e| format!("audio: not a WAV, MP3, OGG or FLAC file ({e})"))?;
    let mut reader = probed.format;
    let track = reader
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("audio: the file has no sound track")?;
    let id = track.id;
    let mut rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut channels = track.codec_params.channels.map_or(0, |c| c.count() as u16);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("audio: {e}"))?;
    let mut out: Vec<f32> = Vec::new();
    let mut buf: Option<(SignalSpec, SampleBuffer<f32>)> = None;
    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(DecodeError::ResetRequired) => break,
            Err(e) => return Err(format!("audio: {e}")),
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(DecodeError::DecodeError(_)) => continue,
            Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("audio: {e}")),
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        channels = spec.channels.count() as u16;
        if decoded.frames() == 0 {
            continue;
        }
        let fits = buf
            .as_ref()
            .is_some_and(|(s, b)| *s == spec && b.capacity() >= decoded.capacity());
        if !fits {
            buf = Some((spec, SampleBuffer::new(decoded.capacity() as u64, spec)));
        }
        let (_, b) = buf.as_mut().expect("made above");
        b.copy_interleaved_ref(decoded);
        out.extend_from_slice(b.samples());
        if out.len() as u64 > MAX_AUDIO_SAMPLES {
            return Err("audio: longer than the 30-minute track cap".into());
        }
    }
    if channels == 0 || rate == 0 {
        return Err("audio: the file declares no channels or rate".into());
    }
    // The same sanitising law as the WAV path: non-finite → 0, clamped.
    for s in &mut out {
        *s = if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 };
    }
    Ok((rate, channels, out))
}

/// Linear resampling of interleaved `samples` from `from` Hz to `to` Hz.
/// The length is the input's duration at the new rate, rounded to the
/// nearest sample frame.
pub fn resample_linear(samples: &[f32], channels: u16, from: u32, to: u32) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    let n = samples.len() / ch;
    if from == to || from == 0 || to == 0 || n == 0 {
        return samples.to_vec();
    }
    let m = ((n as u64 * to as u64 + from as u64 / 2) / from as u64) as usize;
    let step = from as f64 / to as f64;
    let mut out = Vec::with_capacity(m * ch);
    for j in 0..m {
        let pos = j as f64 * step;
        let i0 = (pos.floor() as usize).min(n - 1);
        let i1 = (i0 + 1).min(n - 1);
        let t = (pos - i0 as f64) as f32;
        for c in 0..ch {
            let (a, b) = (samples[i0 * ch + c], samples[i1 * ch + c]);
            out.push(a + (b - a) * t);
        }
    }
    out
}

// ---- Mix -----------------------------------------------------------------------

/// The cut's mix over its first `frames` frames at `out_rate`: interleaved
/// stereo, every sounding track (see [`sounds`]) resampled, scaled by its
/// gain and placed at its offset (a mono clip feeds both sides), summed
/// and clamped to [-1, 1]. `frames · out_rate / fps` sample frames long.
pub fn mix(cut: &Cut, fps: u32, frames: u32, out_rate: u32) -> Vec<f32> {
    let fps = fps.max(1) as i64;
    let rate = out_rate.max(1) as i64;
    let len = (frames as i64 * rate / fps) as usize;
    let mut out = vec![0.0f32; len * 2];
    let solo = any_solo(cut);
    for (_, props, clip) in tracks(cut) {
        if !sounds(props, solo) {
            continue;
        }
        let g = props.gain();
        let start = (props.offset as i64 * rate).div_euclid(fps);
        let ch = clip.channels.max(1) as usize;
        let resampled;
        let src: &[f32] = if clip.sample_rate == out_rate {
            &clip.samples
        } else {
            resampled = resample_linear(&clip.samples, clip.channels, clip.sample_rate, out_rate);
            &resampled
        };
        let n = src.len() / ch;
        let first = (-start).max(0) as usize;
        for i in first..n {
            let o = start + i as i64;
            if o >= len as i64 {
                break;
            }
            let o = o as usize;
            let (l, r) = if ch == 1 {
                (src[i], src[i])
            } else {
                (src[i * ch], src[i * ch + 1])
            };
            out[2 * o] += l * g;
            out[2 * o + 1] += r * g;
        }
    }
    for s in &mut out {
        *s = s.clamp(-1.0, 1.0);
    }
    out
}

/// Write interleaved stereo `samples` at `rate` as a 24-bit PCM WAV
/// (hound) — the mixdown deliveries carry.
pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> std::result::Result<(), String> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| format!("mixdown: {e}"))?;
    const FULL: f32 = 8_388_607.0;
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * FULL).round() as i32)
            .map_err(|e| format!("mixdown: {e}"))?;
    }
    w.finalize().map_err(|e| format!("mixdown: {e}"))
}

// ---- Envelope and lip flap -------------------------------------------------------

/// The RMS level of `clip` on each of `frames` sheet frames when its first
/// sample sounds on frame `offset`: every channel's samples in the frame's
/// span, divided by the whole span (a frame the clip only partly reaches
/// reads lower; one it never reaches reads 0).
pub fn envelope(clip: &AudioClip, offset: i32, fps: u32, frames: u32) -> Vec<f32> {
    let fps = fps.max(1) as i64;
    let rate = clip.sample_rate as i64;
    let ch = clip.channels.max(1) as usize;
    let n = (clip.samples.len() / ch) as i64;
    (0..frames as i64)
        .map(|f| {
            let rel = f - offset as i64;
            let a = (rel * rate).div_euclid(fps);
            let b = ((rel + 1) * rate).div_euclid(fps);
            let span = (b - a).max(1);
            let (lo, hi) = (a.clamp(0, n) as usize, b.clamp(0, n) as usize);
            if hi <= lo {
                return 0.0;
            }
            let sum: f64 = clip.samples[lo * ch..hi * ch]
                .iter()
                .map(|s| (*s as f64) * (*s as f64))
                .sum();
            (sum / (span as f64 * ch as f64)).sqrt() as f32
        })
        .collect()
}

/// `env` scaled so its loudest frame reads 1 (all zeros stay zeros) — the
/// lip flap's thresholds are fractions of the line's peak.
pub fn normalize(env: &[f32]) -> Vec<f32> {
    let peak = env.iter().copied().fold(0.0f32, f32::max);
    if peak <= 0.0 {
        return env.to_vec();
    }
    env.iter().map(|v| v / peak).collect()
}

/// One of the three anime mouths (kuchipaku, D14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mouth {
    Closed,
    Half,
    Open,
}

impl Mouth {
    /// 0 closed, 1 half, 2 open — the slot of its drawing.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// The default minimum hold: a mouth shows at least two frames (D14).
pub const LIP_HOLD: u32 = 2;
/// The default thresholds, as fractions of the line's peak level.
pub const LIP_THRESHOLDS: (f32, f32) = (0.2, 0.5);
/// Hysteresis: a mouth closes down a step only below this fraction of the
/// threshold that opened it, so a level hovering at a threshold does not
/// flap.
pub const LIP_HYSTERESIS: f32 = 0.8;

/// The mouth on each frame of `env`: Open at `hi` and above, Half at `lo`
/// and above, else Closed — opening at the thresholds, closing only below
/// [`LIP_HYSTERESIS`] of them — and no mouth changes before it has shown
/// `min_hold` frames. The mouth before the first frame counts as a closed
/// one long held, so the first frame may open at once (and a leading
/// closed run continues it, whatever its length).
pub fn lip_flap(env: &[f32], (lo, hi): (f32, f32), min_hold: u32) -> Vec<Mouth> {
    let level = |v: f32, lo: f32, hi: f32| {
        if v >= hi {
            Mouth::Open
        } else if v >= lo {
            Mouth::Half
        } else {
            Mouth::Closed
        }
    };
    let mut out = Vec::with_capacity(env.len());
    let mut cur = Mouth::Closed;
    let mut held = min_hold;
    for &v in env {
        let up = level(v, lo, hi);
        let down = level(v, lo * LIP_HYSTERESIS, hi * LIP_HYSTERESIS);
        let want = if up > cur {
            up
        } else if down < cur {
            down
        } else {
            cur
        };
        if want != cur && held >= min_hold {
            cur = want;
            held = 0;
        }
        held = held.saturating_add(1);
        out.push(cur);
    }
    out
}

/// The sheet edit Generate makes: from frame `from`, `column` shows
/// `drawings` (closed, half, open) as `mouths` says — one `SetCells`, so
/// one undo step, refused on a locked column like any cell edit. The
/// frames after the range keep showing what they showed.
pub fn lip_flap_commands(
    project: &Project,
    at: CutRef,
    column: ColumnId,
    drawings: [DrawingId; 3],
    from: u32,
    mouths: &[Mouth],
) -> Result<Vec<Command>> {
    let cut = project
        .cut(at.scene, at.cut)
        .ok_or(EngineError::UnknownCut(at.cut))?;
    let col = cut
        .xsheet
        .column(column)
        .ok_or(EngineError::UnknownColumn(column))?;
    if let Some(d) = drawings.iter().find(|d| cut.drawing(**d).is_none()) {
        return Err(EngineError::UnknownDrawing(*d));
    }
    let mut runs: Vec<(Exposure, u32)> = Vec::new();
    for m in mouths {
        let e = Exposure::Drawing(drawings[m.index()]);
        match runs.last_mut() {
            Some((last, n)) if *last == e => *n += 1,
            _ => runs.push((e, 1)),
        }
    }
    let mut copy = col.clone();
    if !copy.splice(from, mouths.len() as u32, &runs) {
        return Err(EngineError::InvalidCommand(
            "edit would push a key past the last frame number".into(),
        ));
    }
    let cells = crate::sheet_ops::cells_diff(col, &copy);
    Ok(if cells.is_empty() {
        vec![]
    } else {
        vec![Command::SetCells { at, cells }]
    })
}
