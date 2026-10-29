//! GM12: real-time playback's pure parts — the audio clock that picks the
//! frame to show from the sample frames the sound has played, the RAM
//! preview's frame cache, and the scrub snippets a stopped playhead asks
//! for. No device, no window, no wall clock: the Editor feeds positions in.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use anim_core::ids::CutId;
use anim_core::model::Cut;

// ---- The audio clock -----------------------------------------------------------

/// The audio clock: the frame on screen is the frame the sound is on —
/// `start + floor(played · fps / rate)` — never one ahead of it. When the
/// UI falls behind, the frames between are passed over and counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayClock {
    /// The frame the sound started from.
    pub start: u32,
    pub fps: u32,
    /// Sample frames per second of the sound being played.
    pub rate: u32,
    /// The frame last shown.
    pub shown: u32,
    /// Frames passed over to keep up with the sound since the clock started.
    pub dropped: u64,
}

impl PlayClock {
    pub fn new(start: u32, fps: u32, rate: u32) -> Self {
        Self {
            start,
            fps: fps.max(1),
            rate: rate.max(1),
            shown: start,
            dropped: 0,
        }
    }

    /// The frame the sound is on after `played` sample frames.
    pub fn frame_at(&self, played: u64) -> u32 {
        let f = played as u128 * self.fps as u128 / self.rate as u128;
        (self.start as u128 + f).min(u32::MAX as u128) as u32
    }

    /// Move to the sound's frame after `played` sample frames, counting the
    /// frames passed over since the last one shown. Never moves back: a
    /// position that reads smaller holds the frame.
    pub fn advance(&mut self, played: u64) -> u32 {
        let f = self.frame_at(played);
        if f > self.shown {
            self.dropped += (f - self.shown - 1) as u64;
            self.shown = f;
        }
        self.shown
    }
}

/// Where the playhead goes when the clock reaches a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Show this frame.
    Show(u32),
    /// Past the range's end with loop on: back to its first frame.
    Wrap(u32),
    /// Past the range's end with loop off: hold its last frame and stop.
    End(u32),
}

/// The playhead's next place for `frame` in the play range `lo..=hi`. A
/// frame before the range plays on into it (play from the cursor).
pub fn place(frame: u32, (lo, hi): (u32, u32), looping: bool) -> Step {
    if frame <= hi {
        Step::Show(frame)
    } else if looping {
        Step::Wrap(lo)
    } else {
        Step::End(hi)
    }
}

/// The play range: the loop range when it lies inside the cut, else the
/// whole cut.
pub fn play_range(loop_range: Option<(u32, u32)>, frame_count: u32) -> (u32, u32) {
    let last = frame_count.max(1) - 1;
    match loop_range {
        Some((lo, hi)) if lo <= hi && hi <= last => (lo, hi),
        _ => (0, last),
    }
}

// ---- Scrubbing -------------------------------------------------------------------

/// A stretch of the mix to sound once: `len` sample frames from `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snippet {
    pub start: u64,
    pub len: u64,
}

/// The snippets a playhead move from `prev` to `now` asks for: with scrub
/// on and playback stopped, one frame's length of sound at the frame
/// landed on (a fast drag sounds where it lands, not a queue of late
/// frames); nothing while playing, with scrub off, or without a move.
pub fn scrub_requests(
    prev: u32,
    now: u32,
    playing: bool,
    scrub: bool,
    fps: u32,
    rate: u32,
) -> Vec<Snippet> {
    if playing || !scrub || prev == now {
        return Vec::new();
    }
    let (fps, rate) = (fps.max(1) as u64, rate as u64);
    let start = now as u64 * rate / fps;
    let end = (now as u64 + 1) * rate / fps;
    vec![Snippet {
        start,
        len: end - start,
    }]
}

/// What the cut's mix is made of — a key that moves whenever what should
/// sound does (a track added or removed, a mute, a gain, an offset, a
/// clip swapped by undo, the rate or the length).
pub fn mix_key(cut: &Cut, fps: u32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (cut.id, fps, cut.frame_count).hash(&mut h);
    for (i, props, clip) in anim_core::audio::tracks(cut) {
        i.hash(&mut h);
        props.name.hash(&mut h);
        props.kind.hash(&mut h);
        props.offset.hash(&mut h);
        props.gain_db.to_bits().hash(&mut h);
        (props.mute, props.solo).hash(&mut h);
        (Arc::as_ptr(&clip.bytes) as *const u8 as usize).hash(&mut h);
    }
    h.finish()
}

// ---- RAM preview -----------------------------------------------------------------
// The store is built and pinned here; the worker that fills it, the Ao bar
// and the viewer's read are GM12's left items — until then only the tests
// use it (hence the dead-code allowances, as config.rs's).

/// One composite held for RAM preview: RGBA8, `w × h`.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CachedFrame {
    pub w: u32,
    pub h: u32,
    pub rgba: Arc<Vec<u8>>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl CachedFrame {
    pub fn bytes(&self) -> usize {
        self.rgba.len()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
struct Entry {
    hash: u64,
    used_at: u64,
    frame: CachedFrame,
}

/// RAM preview's store: composites keyed by (cut, frame), each with the
/// eval hash it was drawn for — a frame whose hash has moved on is stale
/// and goes on the next look. Bounded in bytes (the MB cap in Settings ▸
/// Performance): past the cap the least recently used frame goes first.
#[cfg_attr(not(test), allow(dead_code))]
pub struct FrameCache {
    cap: usize,
    used: usize,
    clock: u64,
    map: HashMap<(CutId, u32), Entry>,
}

/// A mebibyte, the cap's unit.
#[cfg_attr(not(test), allow(dead_code))]
pub const MB: usize = 1024 * 1024;

#[cfg_attr(not(test), allow(dead_code))]
impl FrameCache {
    pub fn new(cap_mb: u32) -> Self {
        Self::with_cap_bytes(cap_mb as usize * MB)
    }

    pub fn with_cap_bytes(cap: usize) -> Self {
        Self {
            cap,
            used: 0,
            clock: 0,
            map: HashMap::new(),
        }
    }

    /// A new cap; frames past it go at once, oldest first.
    pub fn set_cap_mb(&mut self, mb: u32) {
        self.cap = mb as usize * MB;
        self.evict(0);
    }

    pub fn used_bytes(&self) -> usize {
        self.used
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The frame drawn for `hash`, marked as just used; a frame held for an
    /// older hash is dropped and reads as missing.
    pub fn get(&mut self, cut: CutId, frame: u32, hash: u64) -> Option<CachedFrame> {
        let stale = self.map.get(&(cut, frame))?.hash != hash;
        if stale {
            self.remove(cut, frame);
            return None;
        }
        self.clock += 1;
        let e = self.map.get_mut(&(cut, frame))?;
        e.used_at = self.clock;
        Some(e.frame.clone())
    }

    /// Whether the frame is held for `hash` (the Ao bar's mark; not a use).
    pub fn contains(&self, cut: CutId, frame: u32, hash: u64) -> bool {
        self.map.get(&(cut, frame)).is_some_and(|e| e.hash == hash)
    }

    /// Hold `image` as frame `frame` of `cut` drawn for `hash`, making room
    /// by dropping the least recently used frames. A frame larger than the
    /// whole cap is not held.
    pub fn insert(&mut self, cut: CutId, frame: u32, hash: u64, image: CachedFrame) {
        self.remove(cut, frame);
        let n = image.bytes();
        if n > self.cap {
            return;
        }
        self.evict(n);
        self.clock += 1;
        self.used += n;
        self.map.insert(
            (cut, frame),
            Entry {
                hash,
                used_at: self.clock,
                frame: image,
            },
        );
    }

    fn remove(&mut self, cut: CutId, frame: u32) {
        if let Some(e) = self.map.remove(&(cut, frame)) {
            self.used -= e.frame.bytes();
        }
    }

    /// Drop the least recently used frames until `room` more bytes fit.
    fn evict(&mut self, room: usize) {
        while self.used + room > self.cap {
            let Some(key) = self
                .map
                .iter()
                .min_by_key(|(_, e)| e.used_at)
                .map(|(k, _)| *k)
            else {
                break;
            };
            self.remove(key.0, key.1);
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.used = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anim_core::command::Command;

    #[test]
    fn playback_clock_shows_the_floor_of_time_times_fps() {
        let c = PlayClock::new(0, 24, 48_000);
        // One frame is 2000 sample frames at 24 fps and 48 kHz.
        assert_eq!(c.frame_at(0), 0);
        assert_eq!(c.frame_at(1999), 0);
        assert_eq!(c.frame_at(2000), 1);
        assert_eq!(c.frame_at(47_999), 23);
        assert_eq!(c.frame_at(48_000), 24);
        // From the cursor: the start frame is added, not rounded.
        let c = PlayClock::new(10, 30, 44_100);
        assert_eq!(c.frame_at(1469), 10);
        assert_eq!(c.frame_at(1470), 11);
        assert_eq!(c.frame_at(44_100 * 2), 70);
    }

    #[test]
    fn playback_clock_counts_the_frames_it_passes_over_and_never_runs_ahead() {
        let mut c = PlayClock::new(0, 24, 48_000);
        assert_eq!(c.advance(2000), 1);
        assert_eq!(c.dropped, 0);
        // The UI stalled: frames 2, 3 and 4 were never shown.
        assert_eq!(c.advance(10_000), 5);
        assert_eq!(c.dropped, 3);
        // A position that reads smaller holds the frame.
        assert_eq!(c.advance(9_000), 5);
        assert_eq!(c.dropped, 3);
        // Never ahead: the frame shown has started by the time the sound
        // reaches it, and the next one has not, at every position.
        let mut c = PlayClock::new(3, 24, 44_100);
        let mut played = 0u64;
        while played < 44_100 * 3 {
            let f = c.advance(played) as u64;
            assert!((f - 3) * 44_100 <= played * 24, "ahead at {played}");
            assert!((f - 3 + 1) * 44_100 > played * 24, "behind at {played}");
            played += 977;
        }
        assert!(c.dropped == 0, "977-sample steps never pass over a frame");
        // The range: loop wraps to its start, loop off holds its end, and
        // a frame before the range plays on into it.
        assert_eq!(place(12, (10, 20), true), Step::Show(12));
        assert_eq!(place(21, (10, 20), true), Step::Wrap(10));
        assert_eq!(place(21, (10, 20), false), Step::End(20));
        assert_eq!(place(4, (10, 20), true), Step::Show(4));
        assert_eq!(play_range(Some((10, 20)), 48), (10, 20));
        assert_eq!(play_range(Some((10, 60)), 48), (0, 47));
        assert_eq!(play_range(None, 48), (0, 47));
    }

    fn frame(bytes: usize) -> CachedFrame {
        CachedFrame {
            w: (bytes / 4) as u32,
            h: 1,
            rgba: Arc::new(vec![0; bytes]),
        }
    }

    #[test]
    fn playback_cache_evicts_the_least_recently_used_beyond_the_cap() {
        let cut = CutId(7);
        let mut c = FrameCache::new(2);
        c.insert(cut, 0, 1, frame(MB));
        c.insert(cut, 1, 1, frame(MB));
        assert_eq!(c.used_bytes(), 2 * MB);
        // Frame 0 is used again, so frame 1 is now the oldest.
        assert!(c.get(cut, 0, 1).is_some());
        c.insert(cut, 2, 1, frame(MB));
        assert_eq!(c.used_bytes(), 2 * MB);
        assert!(c.contains(cut, 0, 1));
        assert!(!c.contains(cut, 1, 1), "the least recently used goes");
        assert!(c.contains(cut, 2, 1));
        // A frame larger than the whole cap is never held.
        c.insert(cut, 3, 1, frame(3 * MB));
        assert!(!c.contains(cut, 3, 1));
        assert_eq!(c.len(), 2);
        // A smaller cap drops the oldest at once.
        c.set_cap_mb(1);
        assert_eq!(c.used_bytes(), MB);
        assert!(c.contains(cut, 2, 1) && !c.contains(cut, 0, 1));
    }

    #[test]
    fn playback_cache_drops_a_frame_whose_eval_hash_changed() {
        let cut = CutId(7);
        let mut c = FrameCache::new(8);
        c.insert(cut, 4, 0xAA, frame(1024));
        assert!(c.get(cut, 4, 0xAA).is_some());
        // The edit moved the frame's eval hash: the old composite is stale.
        assert!(c.get(cut, 4, 0xBB).is_none());
        assert!(!c.contains(cut, 4, 0xAA), "a stale frame goes on the look");
        assert_eq!(c.used_bytes(), 0);
        // Another cut's frame 4 is its own entry.
        c.insert(cut, 4, 0xBB, frame(1024));
        c.insert(CutId(8), 4, 0xBB, frame(1024));
        assert_eq!(c.len(), 2);
        c.clear();
        assert!(c.is_empty() && c.used_bytes() == 0);
    }

    #[test]
    fn playback_scrub_one_frame_move_asks_for_one_frame_of_sound() {
        // Stopped, scrub on, one frame forward: exactly one snippet, one
        // frame long (2000 sample frames at 24 fps, 48 kHz), at the new frame.
        let r = scrub_requests(10, 11, false, true, 24, 48_000);
        assert_eq!(
            r,
            vec![Snippet {
                start: 22_000,
                len: 2000
            }]
        );
        // One frame back sounds the frame landed on.
        assert_eq!(scrub_requests(11, 10, false, true, 24, 48_000).len(), 1);
        // Nothing while playing, with scrub off, or without a move.
        assert!(scrub_requests(10, 11, true, true, 24, 48_000).is_empty());
        assert!(scrub_requests(10, 11, false, false, 24, 48_000).is_empty());
        assert!(scrub_requests(10, 10, false, true, 24, 48_000).is_empty());
        // At 30 fps and 44.1 kHz a frame is 1470 sample frames.
        assert_eq!(
            scrub_requests(0, 1, false, true, 30, 44_100),
            vec![Snippet {
                start: 1470,
                len: 1470
            }]
        );
    }

    #[test]
    fn playback_mix_key_moves_with_what_should_sound() {
        let mut cut = Cut::new(CutId(1), "c", 24);
        let k0 = mix_key(&cut, 24);
        cut.audio = Some(clip());
        let k1 = mix_key(&cut, 24);
        assert_ne!(k0, k1);
        cut.audio_props.mute = true;
        assert_ne!(mix_key(&cut, 24), k1);
        cut.audio_props.mute = false;
        assert_eq!(mix_key(&cut, 24), k1, "the same settings, the same key");
        assert_ne!(mix_key(&cut, 30), k1);
    }

    /// A WAV of 0.1 s of a quiet ramp, 16-bit mono at 8 kHz, made here.
    fn wav_bytes() -> Vec<u8> {
        let samples: Vec<i16> = (0..800).map(|i| (i * 8) as i16).collect();
        let data = samples.len() as u32 * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&8000u32.to_le_bytes());
        b.extend_from_slice(&16_000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        for s in samples {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    fn clip() -> anim_core::model::AudioClip {
        anim_core::model::AudioClip::from_bytes("ramp.wav", wav_bytes()).expect("the ramp decodes")
    }

    #[test]
    fn playback_with_sound_never_builds_an_audio_out_in_tests() {
        let mut state = crate::doc::AppState::new_project("t", 320, 240, 24, 300.0);
        let at = state.at();
        state
            .engine
            .apply(
                "set audio",
                vec![Command::SetCutAudio {
                    at,
                    audio: Some(clip()),
                }],
            )
            .expect("the clip is set");
        let mut h = crate::testkit::EditorHarness::with_state(state);
        h.state_mut().view.playing = true;
        h.run_frames(3);
        // The Editor reached for a device to sound the cut — and a test
        // build refuses before rodio is touched.
        assert!(h.editor().audio_tried, "playback with sound asked for a device");
        assert!(h.editor().audio_out.is_none(), "no audio device in tests");
        assert!(!h.editor().audio_playing);
        // Scrubbing while stopped asks for nothing either.
        h.state_mut().view.playing = false;
        h.editor_mut().scrub_audio = true;
        h.run_frames(1);
        h.state_mut().step(1);
        h.run_frames(2);
        assert!(h.editor().audio_out.is_none());
    }
}
