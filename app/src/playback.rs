//! GM12: real-time playback — the audio clock that picks the frame to show
//! from the sample frames the sound has played, the scrub snippets a
//! stopped playhead asks for, the mix built on a worker, RAM preview (the
//! play range's composites drawn by the CPU on a worker into a frame cache
//! the viewer plays from), and the lip-flap (kuchipaku) window. No device,
//! no window, no wall clock in the pure parts: the Editor feeds positions
//! in.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use anim_core::Engine;
use anim_core::audio::{self, LIP_HOLD, LIP_THRESHOLDS, Mouth, TrackKind};
use anim_core::color::ColorLaw;
use anim_core::command::CutRef;
use anim_core::deliver::{self, Purpose, RenderOpts};
use anim_core::ids::{ColumnId, CutId, DrawingId};
use anim_core::model::Cut;
use anim_core::xsheet::Exposure;
use eframe::egui;

use crate::doc::AppState;
use crate::plate;

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

/// A loop range after one frame is inserted (or deleted) at `frame` (D4):
/// the frames from there on move with the sheet. None when the delete took
/// the range's only frame.
pub fn ripple_range((lo, hi): (u32, u32), frame: u32, inserted: bool) -> Option<(u32, u32)> {
    if inserted {
        let up = |f: u32| if f >= frame { f.saturating_add(1) } else { f };
        return Some((up(lo), up(hi)));
    }
    let lo2 = if lo > frame { lo - 1 } else { lo };
    let hi2 = if hi >= frame { hi.checked_sub(1)? } else { hi };
    (lo2 <= hi2).then_some((lo2, hi2))
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
    for (i, props, clip) in audio::tracks_iter(cut) {
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

/// The cut's mix, built on a worker (D9): mixing every track of a long
/// cut is too much for one UI frame. Playback starts the sound when it is
/// in; until then the UI clock steps the playhead.
pub struct MixJob {
    /// The [`mix_key`] it mixes for.
    pub key: u64,
    rx: Receiver<Vec<f32>>,
}

impl MixJob {
    /// Mix `cut` at [`audio::MIX_RATE`] on a worker. Only what sounds
    /// crosses: the clips (their samples Arc'd) and their settings.
    pub fn start(cut: &Cut, fps: u32, key: u64) -> Self {
        let mut sound = Cut::new(cut.id, "", cut.frame_count);
        sound.audio = cut.audio.clone();
        sound.audio_props = cut.audio_props.clone();
        sound.tracks = cut.tracks.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mix = audio::mix(&sound, fps, sound.frame_count, audio::MIX_RATE);
            let _ = tx.send(mix);
        });
        Self { key, rx }
    }

    /// The mix once it is in (Ok(None) while it is being made; Err when
    /// the worker stopped without one).
    pub fn poll(&self) -> Result<Option<Arc<Vec<f32>>>, ()> {
        match self.rx.try_recv() {
            Ok(m) => Ok(Some(Arc::new(m))),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(()),
        }
    }

    /// Tests: block until the mix is in (no sleep, no clock).
    #[cfg(test)]
    pub fn wait(self) -> Vec<f32> {
        self.rx.recv().expect("the mix worker answers")
    }
}

// ---- RAM preview -----------------------------------------------------------------

/// One composite held for RAM preview: RGBA8, `w × h`.
#[derive(Debug, Clone)]
pub struct CachedFrame {
    pub w: u32,
    pub h: u32,
    pub rgba: Arc<Vec<u8>>,
}

impl CachedFrame {
    pub fn bytes(&self) -> usize {
        self.rgba.len()
    }
}

struct Entry {
    hash: u64,
    used_at: u64,
    frame: CachedFrame,
}

/// RAM preview's store: composites keyed by (cut, frame), each with the
/// hash it was drawn for — a frame whose hash has moved on is stale and
/// goes on the next look. Bounded in bytes (the MB cap in Settings ▸
/// Performance): past the cap the least recently used frame goes first
/// (an ordered index, so eviction never scans the store).
pub struct FrameCache {
    cap: usize,
    used: usize,
    clock: u64,
    map: HashMap<(CutId, u32), Entry>,
    /// Last use → frame: the first entry is the least recently used.
    order: BTreeMap<u64, (CutId, u32)>,
}

/// A mebibyte, the cap's unit.
pub const MB: usize = 1024 * 1024;

/// The RAM preview cap's default, in MB (Settings ▸ Performance).
pub const DEFAULT_RAM_MB: u32 = 2048;

impl Default for FrameCache {
    fn default() -> Self {
        Self::new(DEFAULT_RAM_MB)
    }
}

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
            order: BTreeMap::new(),
        }
    }

    /// A new cap; frames past it go at once, oldest first.
    pub fn set_cap_mb(&mut self, mb: u32) {
        let cap = mb as usize * MB;
        if cap != self.cap {
            self.cap = cap;
            self.evict(0);
        }
    }

    pub fn cap_bytes(&self) -> usize {
        self.cap
    }

    pub fn used_bytes(&self) -> usize {
        self.used
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[cfg(test)]
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
        self.order.remove(&e.used_at);
        e.used_at = self.clock;
        self.order.insert(self.clock, (cut, frame));
        Some(e.frame.clone())
    }

    /// Whether the frame is held for `hash` (not a use).
    #[cfg(test)]
    pub fn contains(&self, cut: CutId, frame: u32, hash: u64) -> bool {
        self.map.get(&(cut, frame)).is_some_and(|e| e.hash == hash)
    }

    /// The hash the frame is held for, if it is held (the Ao bar's mark;
    /// not a use).
    pub fn held(&self, cut: CutId, frame: u32) -> Option<u64> {
        self.map.get(&(cut, frame)).map(|e| e.hash)
    }

    /// The frames of `cut` held, in no order.
    pub fn frames_of(&self, cut: CutId) -> Vec<u32> {
        self.map
            .keys()
            .filter(|(c, _)| *c == cut)
            .map(|(_, f)| *f)
            .collect()
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
        self.order.insert(self.clock, (cut, frame));
        self.map.insert(
            (cut, frame),
            Entry {
                hash,
                used_at: self.clock,
                frame: image,
            },
        );
    }

    /// Let one frame go (an edit changed it).
    pub fn remove(&mut self, cut: CutId, frame: u32) {
        if let Some(e) = self.map.remove(&(cut, frame)) {
            self.used -= e.frame.bytes();
            self.order.remove(&e.used_at);
        }
    }

    /// Drop the least recently used frames until `room` more bytes fit.
    fn evict(&mut self, room: usize) {
        while self.used + room > self.cap {
            let Some((_, (cut, frame))) = self.order.pop_first() else {
                break;
            };
            if let Some(e) = self.map.remove(&(cut, frame)) {
                self.used -= e.frame.bytes();
            }
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
        self.used = 0;
    }
}

/// What RAM preview's frames are drawn for besides the document: the cut,
/// the frame size, the colour law and the final look. Another look starts
/// the store afresh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RamLook {
    pub at: CutRef,
    pub size: (u32, u32),
    pub law: ColorLaw,
    pub final_look: bool,
}

impl RamLook {
    fn hash(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.at.scene, self.at.cut, self.size, self.law, self.final_look).hash(&mut h);
        h.finish()
    }

    /// Half size past 2048 px on the long side (a look, not the delivery).
    fn scale(&self) -> f32 {
        if self.size.0.max(self.size.1) > 2048 {
            0.5
        } else {
            1.0
        }
    }

    /// The bytes one held frame takes.
    fn frame_bytes(&self) -> usize {
        let (w, h) = if self.scale() < 0.75 {
            ((self.size.0 / 2).max(1), (self.size.1 / 2).max(1))
        } else {
            self.size
        };
        w as usize * h as usize * 4
    }
}

/// The key a RAM preview frame is held under: its eval hash (the graph's
/// recipe, when the cut has an output) and what the column stack shows on
/// it — each column's place in the stack and the drawing it exposes (its
/// content and layer roles) — with the look. An edit moves the key of
/// exactly the frames it changes.
fn frame_hash(
    cut: &Cut,
    eval: Option<u64>,
    frame: u32,
    look: u64,
    memo: &mut HashMap<DrawingId, u64>,
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (look, eval, frame).hash(&mut h);
    for col in &cut.xsheet.columns {
        let def = cut.column_def(col.id);
        (col.id, def.renders(), def.kind).hash(&mut h);
        let shown = col.resolve(frame);
        shown.hash(&mut h);
        if let Some(d) = shown.and_then(|id| cut.drawing(id)) {
            memo.entry(d.id)
                .or_insert_with(|| d.content_hash())
                .hash(&mut h);
            for l in &d.layers {
                l.props.role.hash(&mut h);
            }
        }
    }
    h.finish()
}

/// The eval hash of each of `frames` (None where the cut has no output).
fn evals(engine: &mut Engine, at: CutRef, frames: &[u32]) -> Vec<Option<u64>> {
    frames
        .iter()
        .map(|&f| engine.eval(at.scene, at.cut, f).ok().map(|v| v.hash()))
        .collect()
}

/// A pass of the worker over the frames not yet held.
struct RamPass {
    rx: Receiver<(u32, u64, CachedFrame)>,
    total: usize,
    done: usize,
    stop: Arc<AtomicBool>,
}

impl Drop for RamPass {
    /// A pass let go (an edit, another look, the verb again) stops after
    /// the frame it is on.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// RAM preview (Sheet ▸ Play ▸ Render RAM preview): the play range's
/// composites drawn by the CPU on a worker (D9) into a [`FrameCache`],
/// and played from it — the viewer shows a held frame while playing, the
/// sheet marks each held frame with an Ao bar. An edit drops just the
/// frames it changed; another cut, size, colour law or look starts afresh.
#[derive(Default)]
pub struct RamPreview {
    cache: FrameCache,
    look: Option<RamLook>,
    /// The revision the held frames were checked at.
    checked: u64,
    pass: Option<RamPass>,
    /// The verb asked for a pass (started on the next sync).
    requested: bool,
    tex: Option<egui::TextureHandle>,
    /// What the texture holds.
    shown: Option<(CutId, u32, u64)>,
}

impl RamPreview {
    /// The verb: a pass running stops; otherwise one is asked for.
    pub fn toggle(&mut self) -> &'static str {
        if self.pass.take().is_some() {
            "RAM preview stopped"
        } else {
            self.requested = true;
            "RAM preview: rendering the play range…"
        }
    }

    /// A pass is drawing frames.
    pub fn busy(&self) -> bool {
        self.pass.is_some()
    }

    /// The running pass's frames done and to do.
    pub fn progress(&self) -> Option<(usize, usize)> {
        self.pass.as_ref().map(|p| (p.done, p.total))
    }

    /// Whether frame `frame` of `cut` is held (the Ao bar).
    pub fn held(&self, cut: CutId, frame: u32) -> bool {
        self.cache.held(cut, frame).is_some()
    }

    /// The frames held and the megabytes they take.
    pub fn usage(&self) -> (usize, usize) {
        (self.cache.len(), self.cache.used_bytes().div_ceil(MB))
    }

    /// Settings ▸ Performance's cap.
    pub fn set_cap_mb(&mut self, mb: u32) {
        self.cache.set_cap_mb(mb);
    }

    /// Each UI frame, before playback: keep the held frames true to the
    /// document and take in the worker's frames. Another look clears the
    /// store; an edit (a new revision) stops a running pass and drops each
    /// held frame whose key moved; a requested pass starts. Returns a line
    /// for the status (Ok) or the refusal lane (Err).
    pub fn sync(
        &mut self,
        engine: &mut Engine,
        look: RamLook,
        range: (u32, u32),
        repaint: Option<&egui::Context>,
    ) -> Option<Result<String, String>> {
        let rev = engine.revision();
        if self.look != Some(look) {
            self.cache.clear();
            self.pass = None;
            self.look = Some(look);
            self.checked = rev;
        }
        if rev != self.checked {
            // The pass draws the document as it was: let it go.
            self.pass = None;
            self.recheck(engine, look);
            self.checked = rev;
        }
        let mut msg = None;
        let mut finished = false;
        if let Some(pass) = &mut self.pass {
            loop {
                match pass.rx.try_recv() {
                    Ok((f, hash, img)) => {
                        self.cache.insert(look.at.cut, f, hash, img);
                        pass.done += 1;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = true;
                        break;
                    }
                }
            }
        }
        if finished {
            self.pass = None;
            let (n, mb) = self.usage();
            msg = Some(Ok(format!(
                "RAM preview: {n} frames held ({mb} of {} MB)",
                self.cache.cap_bytes() / MB
            )));
        }
        if std::mem::take(&mut self.requested) {
            msg = Some(self.start(engine, look, range, repaint));
        }
        msg
    }

    /// Drop each held frame of the look's cut that an edit changed (its
    /// key moved) or that the cut no longer has.
    fn recheck(&mut self, engine: &mut Engine, look: RamLook) {
        let frames = self.cache.frames_of(look.at.cut);
        if frames.is_empty() {
            return;
        }
        let evals = evals(engine, look.at, &frames);
        let Some(cut) = engine.project.cut(look.at.scene, look.at.cut) else {
            self.cache.clear();
            return;
        };
        let (lh, mut memo) = (look.hash(), HashMap::new());
        for (f, eval) in frames.into_iter().zip(evals) {
            let now = (f < cut.frame_count).then(|| frame_hash(cut, eval, f, lh, &mut memo));
            if now.is_none() || self.cache.held(look.at.cut, f) != now {
                self.cache.remove(look.at.cut, f);
            }
        }
    }

    /// Start a pass over the frames of `lo..=hi` not held, as many as the
    /// cap holds.
    fn start(
        &mut self,
        engine: &mut Engine,
        look: RamLook,
        (lo, hi): (u32, u32),
        repaint: Option<&egui::Context>,
    ) -> Result<String, String> {
        let one = look.frame_bytes();
        let cap = self.cache.cap_bytes();
        if one > cap {
            return Err(format!(
                "refused — one frame ({} MB) is larger than the RAM preview cap ({} MB); \
                 raise it in Settings ▸ Performance",
                one.div_ceil(MB),
                cap / MB
            ));
        }
        let frames: Vec<u32> = (lo..=hi).collect();
        let evals = evals(engine, look.at, &frames);
        let Some(cut) = engine.project.cut(look.at.scene, look.at.cut) else {
            return Err("refused — RAM preview: the cut is gone".into());
        };
        let (lh, mut memo) = (look.hash(), HashMap::new());
        let mut todo = Vec::new();
        let mut held = 0;
        for (f, eval) in frames.iter().copied().zip(evals) {
            let hash = frame_hash(cut, eval, f, lh, &mut memo);
            if self.cache.held(cut.id, f) == Some(hash) {
                held += 1;
            } else {
                todo.push((f, hash));
            }
        }
        let missing = todo.len();
        todo.truncate((cap / one).saturating_sub(held));
        if todo.is_empty() {
            return if missing == 0 {
                Ok(format!("RAM preview: frames {}–{} are held", lo + 1, hi + 1))
            } else {
                Err(format!(
                    "refused — the RAM preview cap ({} MB) is full; raise it in Settings ▸ Performance",
                    cap / MB
                ))
            };
        }
        let total = todo.len();
        let snapshot = cut.clone();
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let repaint = repaint.cloned();
        let purpose = if look.final_look {
            Purpose::Final
        } else {
            Purpose::View
        };
        let (size, law, scale) = (look.size, look.law, look.scale());
        std::thread::spawn(move || {
            let mut cut = snapshot;
            if purpose == Purpose::Final {
                deliver::retain_purpose(&mut cut, Purpose::Final);
            }
            let opts = RenderOpts { purpose, scale };
            for (f, hash) in todo {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                let img = deliver::render(&cut, law, f, size.0, size.1, &opts);
                let rgba: Vec<u8> = img
                    .px
                    .chunks_exact(4)
                    .flat_map(anim_core::export::straight_rgba8)
                    .collect();
                let frame = CachedFrame {
                    w: img.w,
                    h: img.h,
                    rgba: Arc::new(rgba),
                };
                if tx.send((f, hash, frame)).is_err() {
                    break;
                }
                if let Some(ctx) = &repaint {
                    ctx.request_repaint();
                }
            }
        });
        self.pass = Some(RamPass {
            rx,
            total,
            done: 0,
            stop,
        });
        Ok(if total < missing {
            format!(
                "RAM preview: rendering {total} of {missing} frames — the {} MB cap holds no more",
                cap / MB
            )
        } else {
            format!("RAM preview: rendering {total} frames…")
        })
    }

    /// The held frame of `cut` at `frame` as a texture for the views (the
    /// viewer plays from it), or None when it is not held.
    pub fn shown(&mut self, cut: CutId, frame: u32, ctx: &egui::Context) -> Option<egui::TextureId> {
        let hash = self.cache.held(cut, frame)?;
        if self.shown != Some((cut, frame, hash)) {
            let img = self.cache.get(cut, frame, hash)?;
            let image =
                egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.rgba);
            match &mut self.tex {
                Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.tex =
                        Some(ctx.load_texture("ram preview", image, egui::TextureOptions::LINEAR));
                }
            }
            self.shown = Some((cut, frame, hash));
        }
        self.tex.as_ref().map(|t| t.id())
    }

    /// Tests: block until the running pass is done, taking its frames (no
    /// sleep, no clock).
    #[cfg(test)]
    pub fn wait(&mut self) {
        let Some(look) = self.look else {
            return;
        };
        if let Some(pass) = self.pass.take() {
            while let Ok((f, hash, img)) = pass.rx.recv() {
                self.cache.insert(look.at.cut, f, hash, img);
            }
        }
    }

    /// Tests: what the views' texture holds.
    #[cfg(test)]
    pub fn showing(&self) -> Option<(CutId, u32)> {
        self.shown.map(|(c, f, _)| (c, f))
    }
}

// ---- The transport's slots -------------------------------------------------------

/// A small painted key of the transport: `text` in a fixed `w`, answering
/// to `name` (D7: a key never moves under the pointer).
pub(crate) fn transport_key(ui: &mut egui::Ui, text: &str, name: &str, w: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, plate::CTRL_H), egui::Sense::click());
    let ink = if !ui.is_enabled() {
        plate::legend_dim()
    } else if resp.hovered() {
        plate::STRUCK
    } else {
        plate::LEGEND
    };
    let p = ui.painter();
    p.rect_stroke(
        rect.shrink(2.0),
        0.0,
        egui::Stroke::new(1.0, plate::legend_dim()),
        egui::StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::monospace(11.0),
        ink,
    );
    plate::speak(&resp, egui::WidgetType::Button, None, name);
    resp
}

/// A fixed-width readout: `text` in `w`, whatever it reads (D7).
pub(crate) fn readout(ui: &mut egui::Ui, text: &str, w: f32, ink: egui::Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, plate::CTRL_H), egui::Sense::hover());
    ui.painter().text(
        rect.left_center() + egui::vec2(2.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::monospace(11.0),
        ink,
    );
    plate::speak(&resp, egui::WidgetType::Label, None, text);
    resp
}

// ---- Lip flap (kuchipaku) ------------------------------------------------------

/// The key of a track's envelope: the track's clip, offset and the cut's
/// length and rate — the sheet's waveform and the lip-flap window cache
/// under it.
pub fn envelope_key(cut: &Cut, track: usize, fps: u32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (cut.id, track, fps, cut.frame_count).hash(&mut h);
    if let (Some(p), Some(c)) = (audio::track_props(cut, track), audio::track_clip(cut, track)) {
        p.offset.hash(&mut h);
        (Arc::as_ptr(&c.samples) as *const f32 as usize).hash(&mut h);
    }
    h.finish()
}

/// A track's level on each frame of the cut ([`audio::envelope`]), scaled
/// so its loudest frame reads 1; zeros without a clip.
pub fn track_envelope(cut: &Cut, track: usize, fps: u32) -> Vec<f32> {
    match (audio::track_props(cut, track), audio::track_clip(cut, track)) {
        (Some(p), Some(c)) => audio::normalize(&audio::envelope(c, p.offset, fps, cut.frame_count)),
        _ => vec![0.0; cut.frame_count as usize],
    }
}

/// A track as the pickers name it: its number, name and kind.
pub fn track_name(cut: &Cut, track: usize) -> String {
    match audio::track_props(cut, track) {
        Some(p) => format!("{} {} — {}", track + 1, p.name, p.kind.label()),
        None => format!("{}", track + 1),
    }
}

/// The drawings a column's mouths are picked from: those it exposes, in
/// sheet order, then the rest of the cut's library.
pub fn column_drawings(cut: &Cut, column: ColumnId) -> Vec<DrawingId> {
    let mut out: Vec<DrawingId> = Vec::new();
    if let Some(col) = cut.xsheet.column(column) {
        for (_, e) in col.keys() {
            if let Exposure::Drawing(d) = e
                && !out.contains(&d)
                && cut.drawing(d).is_some()
            {
                out.push(d);
            }
        }
    }
    for d in &cut.drawings {
        if !out.contains(&d.id) {
            out.push(d.id);
        }
    }
    out
}

/// Sheet ▸ Lip flap…'s form (D14): which track's level drives which
/// column's three mouth drawings over which frames, at which thresholds.
pub struct LipForm {
    pub at: CutRef,
    /// The dialogue track (0 = the first track).
    pub track: usize,
    pub column: ColumnId,
    /// The closed, half and open drawings.
    pub mouths: [Option<DrawingId>; 3],
    /// The frames, 0-based and inclusive.
    pub from: u32,
    pub to: u32,
    /// The half and open thresholds, as fractions of the line's peak.
    pub lo: f32,
    pub hi: f32,
    /// The track's envelope, under its [`envelope_key`].
    env: Option<(u64, Arc<Vec<f32>>)>,
}

impl LipForm {
    /// The defaults: the first serifu (dialogue) track with sound, else
    /// the first track with any; the active column and its first three
    /// drawings; the loop range, else the whole cut; [`LIP_THRESHOLDS`].
    /// None when the cut has no sound.
    pub fn of(state: &AppState) -> Option<LipForm> {
        let cut = state.cut();
        let track = audio::tracks_iter(cut)
            .find(|(_, p, _)| p.kind == TrackKind::Dialogue)
            .or_else(|| audio::tracks_iter(cut).next())
            .map(|(i, _, _)| i)?;
        let column = state.view.active_column;
        let ds = column_drawings(cut, column);
        let (from, to) = state.play_range();
        Some(LipForm {
            at: state.at(),
            track,
            column,
            mouths: [ds.first().copied(), ds.get(1).copied(), ds.get(2).copied()],
            from,
            to,
            lo: LIP_THRESHOLDS.0,
            hi: LIP_THRESHOLDS.1,
            env: None,
        })
    }

    /// The three mouths, once every slot has a drawing.
    pub fn mouths_set(&self) -> Option<[DrawingId; 3]> {
        Some([self.mouths[0]?, self.mouths[1]?, self.mouths[2]?])
    }

    /// The track's normalized level on each frame of the cut (cached).
    pub fn envelope(&mut self, cut: &Cut, fps: u32) -> Arc<Vec<f32>> {
        let key = envelope_key(cut, self.track, fps);
        if let Some((k, e)) = &self.env
            && *k == key
        {
            return e.clone();
        }
        let e = Arc::new(track_envelope(cut, self.track, fps));
        self.env = Some((key, e.clone()));
        e
    }

    /// The mouth on each frame of the range, as the thresholds read the
    /// envelope ([`audio::lip_flap`], held at least [`LIP_HOLD`] frames).
    pub fn mouths_for(&mut self, cut: &Cut, fps: u32) -> Vec<Mouth> {
        let env = self.envelope(cut, fps);
        let (a, b) = (self.from as usize, (self.to as usize + 1).min(env.len()));
        let span: &[f32] = if a < b { &env[a..b] } else { &[] };
        audio::lip_flap(span, (self.lo, self.hi), LIP_HOLD)
    }
}

impl crate::Editor {
    /// Sheet ▸ Lip flap… — a non-modal window: the dialogue track, the
    /// mouth column and its closed, half and open drawings, the frames, and
    /// the two thresholds over the track's level; Generate writes the
    /// mouths as one undo step. It closes when the cut changes.
    pub(crate) fn lip_flap_window(&mut self, ctx: &egui::Context) {
        if self.lip_form.as_ref().is_some_and(|f| f.at != self.state.at()) {
            self.lip_form = None;
        }
        let Some(mut form) = self.lip_form.take() else {
            return;
        };
        let fps = self.state.fps();
        let safe = !self.canvas.stroke_active();
        let mut open = true;
        let mut generate = false;
        let s = &self.state;
        let cut = s.cut();
        let last = cut.frame_count.max(1) - 1;
        egui::Window::new("Lip flap")
            .open(&mut open)
            .default_width(360.0)
            .collapsible(false)
            .show(ctx, |ui| {
                plate::surface(ui);
                plate::legend(ui, "kuchipaku (lip flap)").on_hover_text(
                    "the mouth column follows the dialogue's level: closed, half or open on each frame",
                );
                egui::Grid::new("lip_flap").num_columns(2).show(ui, |ui| {
                    ui.label("dialogue");
                    egui::ComboBox::from_id_salt("lip_track")
                        .selected_text(track_name(cut, form.track))
                        .show_ui(ui, |ui| {
                            for (i, _, _) in audio::tracks_iter(cut) {
                                ui.selectable_value(&mut form.track, i, track_name(cut, i));
                            }
                        });
                    ui.end_row();
                    ui.label("column");
                    let col_name = |id: ColumnId| {
                        cut.xsheet
                            .column(id)
                            .map_or_else(|| "?".to_string(), |c| c.name.clone())
                    };
                    let before = form.column;
                    egui::ComboBox::from_id_salt("lip_column")
                        .selected_text(col_name(form.column))
                        .show_ui(ui, |ui| {
                            for c in &cut.xsheet.columns {
                                ui.selectable_value(&mut form.column, c.id, c.name.clone());
                            }
                        });
                    if form.column != before {
                        let ds = column_drawings(cut, form.column);
                        form.mouths = [ds.first().copied(), ds.get(1).copied(), ds.get(2).copied()];
                    }
                    ui.end_row();
                    let ds = column_drawings(cut, form.column);
                    let name = |d: Option<DrawingId>| {
                        d.and_then(|d| cut.drawing(d))
                            .map_or_else(|| "—".to_string(), |d| d.name.clone())
                    };
                    for (slot, label) in ["closed", "half", "open"].into_iter().enumerate() {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(("lip_mouth", slot))
                            .selected_text(name(form.mouths[slot]))
                            .show_ui(ui, |ui| {
                                for &d in &ds {
                                    ui.selectable_value(&mut form.mouths[slot], Some(d), name(Some(d)));
                                }
                            });
                        ui.end_row();
                    }
                    ui.label("frames").on_hover_text("the frames to write (the loop range by default)");
                    ui.horizontal(|ui| {
                        let mut a = form.from + 1;
                        let mut b = form.to + 1;
                        ui.add(egui::DragValue::new(&mut a).range(1..=last + 1).prefix("in "));
                        ui.add(egui::DragValue::new(&mut b).range(1..=last + 1).prefix("out "));
                        form.from = a - 1;
                        form.to = (b - 1).max(form.from);
                    });
                    ui.end_row();
                    ui.label("half at").on_hover_text("the level (of the line's peak) that opens the mouth half");
                    ui.add(egui::Slider::new(&mut form.lo, 0.01..=0.95).fixed_decimals(2));
                    ui.end_row();
                    ui.label("open at").on_hover_text("the level (of the line's peak) that opens the mouth wide");
                    ui.add(egui::Slider::new(&mut form.hi, 0.05..=1.0).fixed_decimals(2));
                    ui.end_row();
                });
                form.hi = form.hi.max(form.lo + 0.01).min(1.0);
                form.lo = form.lo.min(form.hi - 0.01);
                // The preview: the line's level per frame of the range, in
                // the mouth each frame gets, under the two thresholds.
                let env = form.envelope(cut, fps);
                let mouths = form.mouths_for(cut, fps);
                let (w, h) = (ui.available_width().max(120.0), 56.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 0.0, plate::WELL);
                let n = mouths.len().max(1) as f32;
                let bar = rect.width() / n;
                for (i, m) in mouths.iter().enumerate() {
                    let v = env.get(form.from as usize + i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
                    let x = rect.left() + i as f32 * bar;
                    let top = rect.bottom() - v * rect.height();
                    let ink = match m {
                        Mouth::Open => plate::LEGEND,
                        Mouth::Half => plate::legend_dim(),
                        Mouth::Closed => plate::rule_beat(),
                    };
                    p.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(x, top),
                            egui::pos2((x + bar - 1.0).max(x + 1.0), rect.bottom()),
                        ),
                        0.0,
                        ink,
                    );
                }
                for t in [form.lo, form.hi] {
                    let y = rect.bottom() - t * rect.height();
                    p.line_segment(
                        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                        egui::Stroke::new(1.0, plate::AO),
                    );
                }
                let opens = mouths.iter().filter(|m| **m == Mouth::Open).count();
                let halves = mouths.iter().filter(|m| **m == Mouth::Half).count();
                ui.label(
                    egui::RichText::new(format!(
                        "{} frames: {opens} open, {halves} half, {} closed",
                        mouths.len(),
                        mouths.len() - opens - halves
                    ))
                    .color(plate::legend_dim()),
                );
                ui.separator();
                let ready = form.mouths_set().is_some();
                if ui
                    .add_enabled(ready && safe, egui::Button::new("generate"))
                    .on_hover_text("write the mouths into the column over the frames (one undo step)")
                    .on_disabled_hover_text(if ready {
                        "not mid-stroke"
                    } else {
                        "pick a closed, a half and an open drawing"
                    })
                    .clicked()
                {
                    generate = true;
                }
            });
        if generate {
            self.state.generate_lip_flap(&mut form);
        }
        if open {
            self.lip_form = Some(form);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anim_core::command::Command;
    use std::sync::atomic::AtomicUsize;

    use crate::config::Action;
    use crate::testkit::EditorHarness;
    use crate::workspace::Pane;
    use crate::{AppRequest, Performed};

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
        wav_of(&samples)
    }

    /// 16-bit mono samples at 8 kHz as a WAV file's bytes.
    fn wav_of(samples: &[i16]) -> Vec<u8> {
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

    /// A line whose level on each 1/24 s frame is `levels[f]` (a square
    /// wave at 8 kHz, so a frame's RMS is its level).
    fn line_of(levels: &[f32]) -> anim_core::model::AudioClip {
        let n = levels.len() * 8000 / 24;
        let samples: Vec<i16> = (0..n)
            .map(|i| {
                let v = (levels[(i * 24 / 8000).min(levels.len() - 1)] * 32767.0) as i16;
                if i % 2 == 0 { v } else { -v }
            })
            .collect();
        anim_core::model::AudioClip::from_bytes("serifu.wav", wav_of(&samples)).expect("the line decodes")
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

    /// A 320×240, 24 fps project whose cut (48 frames) sounds `clip()`.
    fn sounding() -> AppState {
        let mut state = AppState::new_project("t", 320, 240, 24, 300.0);
        let at = state.at();
        state
            .engine
            .apply("set audio", vec![Command::SetCutAudio { at, audio: Some(clip()) }])
            .expect("the clip is set");
        state
    }

    /// The editor's key for the sound its cut plays (as `sync_audio` makes it).
    fn sound_key(h: &EditorHarness) -> (u64, u64, u64) {
        let s = h.state();
        (s.view.scene.0, s.view.cut.0, mix_key(s.cut(), s.fps()))
    }

    #[test]
    fn playback_mix_is_made_on_a_worker_and_taken_in_when_it_lands() {
        let mut h = EditorHarness::with_state(sounding());
        let key = mix_key(h.state().cut(), 24);
        // The first ask starts the worker and returns at once: the UI frame
        // never mixes (D9).
        assert!(h.editor().mix_job.is_none());
        assert!(h.editor_mut().playback_mix().is_none());
        let job = h.editor_mut().mix_job.take().expect("a worker mixes");
        assert_eq!(job.key, key);
        let cut = h.state().cut();
        let want = audio::mix(cut, 24, cut.frame_count, audio::MIX_RATE);
        let got = job.wait();
        assert_eq!(got.len(), 48 * 2000 * 2, "the cut's length, stereo at 48 kHz");
        assert_eq!(got, want, "the worker's mix is the mix");
        // In: the next ask takes it, and the ones after reuse it.
        let (tx, rx) = channel();
        tx.send(got).unwrap();
        h.editor_mut().mix_job = Some(MixJob { key, rx });
        let mix = h.editor_mut().playback_mix().expect("the mix is in");
        assert!(h.editor().mix_job.is_none());
        let again = h.editor_mut().playback_mix().expect("kept");
        assert!(Arc::ptr_eq(&mix, &again), "mixed once per key");
        // A gain change is a new key; a worker that died plays silent and
        // says so once, on the Aka lane.
        let mut louder = h.state().cut().audio_props.clone();
        louder.gain_db = 6.0;
        h.state_mut().set_track_props(0, louder);
        let key = mix_key(h.state().cut(), 24);
        let (tx, rx) = channel::<Vec<f32>>();
        drop(tx);
        h.editor_mut().mix_job = Some(MixJob { key, rx });
        let silent = h.editor_mut().playback_mix().expect("silence stands in");
        assert_eq!(silent.len(), 48 * 2000 * 2);
        assert!(silent.iter().all(|s| *s == 0.0));
        assert_eq!(h.state().refusal, "refused — the sound mix failed; playing silent");
    }

    fn fixture(ext: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/anim-core/tests/fixtures")
            .join(format!("tone.{ext}"))
    }

    #[test]
    fn playback_imports_wav_mp3_ogg_flac_on_a_worker_one_step_each() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = AppState::new_project("t", 320, 240, 24, 300.0);
        // The first file fills the first track; each further one is a track
        // of the kind its name says (a plain name is effects).
        let files = [
            ("serifu_01.wav", "wav", TrackKind::Dialogue),
            ("bgm01.mp3", "mp3", TrackKind::Music),
            ("se_door.ogg", "ogg", TrackKind::Effects),
            ("tone.flac", "flac", TrackKind::Effects),
        ];
        for (i, (name, ext, _)) in files.iter().enumerate() {
            let path = dir.path().join(name);
            std::fs::copy(fixture(ext), &path).unwrap();
            let steps = s.engine.position();
            s.begin_sound_import(&path, false, None);
            assert_eq!(s.status, format!("decoding {name}…"));
            assert_eq!(s.engine.position(), steps, "nothing lands before the worker's file");
            s.wait_sound_import();
            assert!(s.sound_import.is_none());
            assert_eq!(s.engine.position(), steps + 1, "{name}: one undo step");
            // The scrub hint rides the cut's first sound only.
            assert_eq!(s.status.contains("audio scrub"), i == 0, "{}", s.status);
        }
        let got: Vec<(String, TrackKind)> = audio::tracks_iter(s.cut())
            .map(|(_, p, _)| (p.name.clone(), p.kind))
            .collect();
        let want: Vec<(String, TrackKind)> = files
            .iter()
            .map(|(n, _, k)| (n.rsplit_once('.').unwrap().0.to_string(), *k))
            .collect();
        assert_eq!(got, want);
        assert!(audio::tracks_iter(s.cut()).all(|(_, _, c)| c.seconds() > 0.0));
        // One file at a time; a file that does not decode is refused.
        let junk = dir.path().join("junk.wav");
        std::fs::write(&junk, b"not a sound").unwrap();
        s.begin_sound_import(&junk, true, None);
        s.begin_sound_import(&dir.path().join("tone.flac"), true, None);
        assert_eq!(s.refusal, "refused — junk.wav is still decoding");
        let steps = s.engine.position();
        s.wait_sound_import();
        assert!(s.refusal.starts_with("refused — junk.wav did not import ("), "{}", s.refusal);
        assert_eq!(s.engine.position(), steps);
    }

    #[test]
    fn playback_audio_clock_leads_the_playhead_and_counts_dropped_frames() {
        let mut h = EditorHarness::with_state(sounding());
        assert!(h.has("dropped 0"), "the slot always draws\n{}", h.describe());
        // The sound is playing (as a device would play it), the UI behind.
        let cursor = Arc::new(AtomicUsize::new(0));
        let key = sound_key(&h);
        let ed = h.editor_mut();
        ed.state.view.playing = true;
        ed.audio_playing = true;
        ed.audio_key = Some(key);
        ed.audio_clock = Some((PlayClock::new(0, 24, audio::MIX_RATE), cursor.clone(), 0));
        // 5 frames of stereo samples played: frame 5 shows, 1–4 were passed over.
        cursor.store(5 * 2000 * 2, Ordering::Relaxed);
        h.run_frames(1);
        assert_eq!(h.state().view.frame, 5, "the frame the sound is on");
        assert_eq!(h.editor().play_dropped, 4);
        assert!(h.has("dropped 4"), "{}", h.describe());
        // The UI clock does not step it: until the sound moves, it holds.
        h.run_frames(2);
        assert_eq!(h.state().view.frame, 5);
        cursor.store(6 * 2000 * 2 + 10, Ordering::Relaxed);
        h.run_frames(1);
        assert_eq!(h.state().view.frame, 6);
        assert_eq!(h.editor().play_dropped, 4, "a frame in step drops none");
        assert!(h.editor().audio_playing, "the sound plays on");
    }

    #[test]
    fn playback_loop_range_is_per_cut_ripples_and_a_start_past_it_plays_on() {
        let mut s = AppState::new_project("t", 320, 240, 24, 300.0);
        let at = s.at();
        s.engine
            .apply("len", vec![Command::SetFrameCount { at, frames: 24 }])
            .unwrap();
        s.goto(5);
        s.set_loop_in();
        s.goto(10);
        s.set_loop_out();
        assert_eq!(s.loop_range(), Some((5, 10)));
        assert_eq!(s.status, "loop range 6–11");
        // Another cut has its own (none yet); back here, this one holds.
        let (scene, first) = (s.view.scene, s.view.cut);
        s.new_cut(scene);
        assert_ne!(s.view.cut, first);
        assert_eq!(s.loop_range(), None);
        assert_eq!(s.play_range(), (0, s.frame_count() - 1));
        s.goto_cut(scene, first);
        assert_eq!(s.loop_range(), Some((5, 10)));
        // A frame inserted before it moves it on; one deleted inside it
        // shortens it (D4).
        s.goto(2);
        s.insert_frame();
        assert_eq!(s.loop_range(), Some((6, 11)));
        s.goto(8);
        s.delete_frame();
        assert_eq!(s.loop_range(), Some((6, 10)));
        assert_eq!(s.frame_count(), 24);
        // Play on the range's last frame starts again from its first.
        s.goto(10);
        s.toggle_play();
        assert!(s.view.playing);
        assert_eq!(s.view.frame, 6);
        s.toggle_play();
        // Play from past the range: on to the cut's end, then the range loops.
        let mut h = EditorHarness::with_state(s);
        h.state_mut().goto(20);
        h.state_mut().toggle_play();
        assert_eq!(h.state().view.frame, 20, "play from the cursor never jumps back");
        let tick = 1.0 / 24.0;
        let mut seen = Vec::new();
        for _ in 0..9 {
            h.editor_mut().play_tick(tick);
            seen.push(h.state().view.frame);
        }
        assert_eq!(seen, [21, 22, 23, 6, 7, 8, 9, 10, 6]);
        // Loop off: the run stops on the range's last frame.
        h.state_mut().view.loop_playback = false;
        for _ in 0..6 {
            h.editor_mut().play_tick(tick);
        }
        assert_eq!(h.state().view.frame, 10);
        assert!(!h.state().view.playing);
    }

    #[test]
    fn playback_scrub_is_remembered_and_never_sounds_the_stop() {
        let mut h = EditorHarness::with_state(sounding());
        // On by default, from the config; the latch shows it.
        assert!(h.rig().config.perf.scrub_audio);
        assert!(h.editor().scrub_audio);
        assert_eq!(h.node_toggled("scrub"), Some(true), "{}", h.describe());
        // A stopped step sounds one frame of the frame landed on.
        h.state_mut().step(1);
        h.run_frames(1);
        assert_eq!(h.editor().last_snippet, Some(Snippet { start: 2000, len: 2000 }));
        // The verb turns it off, and the config keeps it.
        assert_eq!(h.perform(Action::ToggleScrub), Performed::Done);
        assert!(!h.editor().scrub_audio);
        assert!(!h.rig().config.perf.scrub_audio, "remembered");
        assert_eq!(h.editor().is_on(Action::ToggleScrub), Some(false));
        h.editor_mut().last_snippet = None;
        h.state_mut().step(1);
        h.run_frames(1);
        assert_eq!(h.editor().last_snippet, None);
        // The latch turns it on again.
        h.click("scrub");
        assert!(h.rig().config.perf.scrub_audio);
        assert!(h.editor().scrub_audio);
        // A run under the sound's clock that ends (loop off, range 3–6)
        // does not sound the frame it stops on.
        h.state_mut().goto(2);
        h.state_mut().set_loop_in();
        h.state_mut().goto(5);
        h.state_mut().set_loop_out();
        h.state_mut().goto(2);
        h.run_frames(1);
        h.state_mut().view.loop_playback = false;
        let cursor = Arc::new(AtomicUsize::new(0));
        let key = sound_key(&h);
        let ed = h.editor_mut();
        ed.state.view.playing = true;
        ed.audio_playing = true;
        ed.audio_key = Some(key);
        ed.audio_clock = Some((PlayClock::new(2, 24, audio::MIX_RATE), cursor.clone(), 0));
        ed.last_snippet = None;
        ed.play_tick(0.0);
        assert_eq!(ed.state.view.frame, 2);
        cursor.store(6 * 2000 * 2, Ordering::Relaxed);
        ed.play_tick(0.0);
        assert_eq!(ed.state.view.frame, 5, "held on the range's last frame");
        assert!(!ed.state.view.playing);
        assert_eq!(ed.last_snippet, None, "no stray scrub at the stop");
        ed.play_tick(0.0);
        assert_eq!(ed.last_snippet, None);
        // The next step does sound.
        ed.state.step(1);
        ed.play_tick(0.0);
        assert_eq!(ed.last_snippet, Some(Snippet { start: 12_000, len: 2000 }));
    }

    #[test]
    fn playback_verbs_have_menus_and_chords_and_run_through_perform() {
        use crate::config::{Ctx, Group, Keymap, clashes, default_binds};
        let verbs = [
            (Action::ImportSound, "File/Import sound…", Group::File, "Ctrl+I"),
            (Action::AddSoundTrack, "File/Add sound track…", Group::File, "Ctrl+Shift+I"),
            (Action::LoopIn, "Sheet/Play/Loop in", Group::Playback, "Shift+I"),
            (Action::LoopOut, "Sheet/Play/Loop out", Group::Playback, "Shift+O"),
            (Action::ClearLoop, "Sheet/Play/Clear loop range", Group::Playback, "Alt+X"),
            (Action::ToggleScrub, "Sheet/Play/Audio scrub", Group::Playback, "Shift+S"),
            (Action::RamPreview, "Sheet/Play/Render RAM preview", Group::Playback, "0"),
        ];
        for &km in Keymap::ALL {
            let binds = default_binds(km);
            for (a, menu, group, chord) in verbs {
                assert_eq!(a.menu(), Some(menu));
                assert_eq!((a.group(), a.context()), (group, Ctx::Global), "{a:?}");
                assert!(!a.help().is_empty() && a.help().len() <= 120, "{a:?}");
                assert!(!a.aliases().is_empty(), "{a:?}");
                let got = crate::cmdpal::chord_of(&binds, a);
                assert_eq!(got.as_deref(), Some(chord), "{a:?} in {km:?}");
            }
            assert!(clashes(&binds).is_empty(), "{km:?}: {:?}", clashes(&binds));
        }
        let lip = Action::LipFlap;
        assert_eq!((lip.label(), lip.menu()), ("kuchipaku (lip flap)…", Some("Sheet/Lip flap…")));
        assert_eq!((lip.group(), lip.context()), (Group::Sheet, Ctx::Sheet));
        for word in ["lip sync", "kuchipaku", "mouth"] {
            assert!(lip.aliases().contains(&word), "{word}");
            assert!(crate::cmdpal::score(word, lip).is_some(), "{word}");
        }
        // One dispatch: the pickers are the shell's, the rest the editor's.
        let mut h = EditorHarness::new();
        assert_eq!(h.perform(Action::ImportSound), Performed::App(AppRequest::ImportSound));
        assert_eq!(h.perform(Action::AddSoundTrack), Performed::App(AppRequest::AddSoundTrack));
        h.state_mut().goto(3);
        assert_eq!(h.perform(Action::LoopIn), Performed::Done);
        h.state_mut().goto(6);
        assert_eq!(h.perform(Action::LoopOut), Performed::Done);
        assert_eq!(h.state().loop_range(), Some((3, 6)));
        assert_eq!(h.perform(Action::ClearLoop), Performed::Done);
        assert_eq!(h.state().loop_range(), None);
        assert_eq!(h.editor().can_perform(Action::ClearLoop), Err("this cut has no loop range"));
        assert_eq!(h.perform(Action::ClearLoop), Performed::Ignored);
        assert_eq!(h.state().refusal, "refused — this cut has no loop range");
        assert_eq!(h.perform(Action::LipFlap), Performed::Ignored, "no sound yet");
        assert!(h.editor().lip_form.is_none());
        // And the keys reach them.
        h.state_mut().goto(4);
        h.key_mods(egui::Modifiers::SHIFT, egui::Key::I);
        assert_eq!(h.performed().last().map(|p| p.0), Some(Action::LoopIn));
        assert_eq!(h.state().loop_range(), Some((4, 47)));
    }

    #[test]
    fn playback_transport_slots_hold_still_when_the_loop_range_is_set() {
        let mut h = EditorHarness::new();
        let (scrub, dropped) = (h.rect("scrub"), h.rect("dropped 0"));
        assert!(h.has("—"), "the range slot reads — while unset\n{}", h.describe());
        assert_eq!(h.node_enabled("clear loop range"), Some(false));
        h.state_mut().goto(4);
        h.click("loop in");
        assert_eq!(h.state().loop_range(), Some((4, 47)));
        assert!(h.has("5–48"), "{}", h.describe());
        assert_eq!(h.node_enabled("clear loop range"), Some(true));
        // D7: nothing after the range moved under the pointer.
        assert_eq!(h.rect("scrub"), scrub);
        assert_eq!(h.rect("dropped 0"), dropped);
        h.state_mut().goto(9);
        h.click("loop out");
        assert_eq!(h.state().loop_range(), Some((4, 9)));
        h.click("clear loop range");
        assert_eq!(h.state().loop_range(), None);
        assert_eq!(h.rect("scrub"), scrub);
    }

    #[test]
    fn playback_track_header_mute_click_and_gain_drag_are_one_step_each() {
        let mut h = EditorHarness::with_state(sounding());
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::XSheet]);
        h.run();
        // The picker's value names the track: number, name, kind.
        let picked = "ComboBox = \"1 dialogue — serifu (dialogue)\"";
        assert!(h.describe().contains(picked), "{}", h.describe());
        let steps = h.state().engine.position();
        h.click("mute track");
        assert!(h.state().cut().audio_props.mute);
        assert_eq!(h.state().engine.position(), steps + 1, "a click, one step");
        assert_eq!(h.node_toggled("mute track"), Some(true));
        // A gain drag shows its value as it goes and lands once, on release.
        let r = h.rect("track gain (dB)");
        h.drag(r.center(), r.center() + egui::vec2(60.0, 0.0), 6);
        let gain = h.state().cut().audio_props.gain_db;
        assert!(gain > 1.0 && gain <= 6.0, "{gain}");
        assert_eq!(h.state().engine.position(), steps + 2, "a drag, one step");
        h.perform(Action::Undo);
        assert_eq!(h.state().cut().audio_props.gain_db, 0.0);
        assert!(h.state().cut().audio_props.mute, "undo takes the drag alone");
    }

    #[test]
    fn playback_lip_flap_generate_writes_the_three_mouths_as_one_step() {
        let mut s = AppState::new_project("t", 320, 240, 24, 300.0);
        let at = s.at();
        s.engine
            .apply("len", vec![Command::SetFrameCount { at, frames: 12 }])
            .unwrap();
        // The closed, half and open mouths: drawings 1, 2 and 3.
        for f in 0..3 {
            s.goto(f);
            s.new_drawing_at_frame();
        }
        s.goto(0);
        let line = line_of(&[0.0, 0.0, 0.0, 0.0, 0.8, 0.8, 0.8, 0.8, 0.24, 0.24, 0.24, 0.24]);
        s.engine
            .apply("line", vec![Command::SetCutAudio { at, audio: Some(line) }])
            .unwrap();
        let ds: Vec<DrawingId> = s.cut().drawings.iter().map(|d| d.id).collect();
        let mut h = EditorHarness::with_state(s);
        assert_eq!(h.perform(Action::LipFlap), Performed::Done);
        h.run();
        // The legend (plate legends read in capitals) in its own window.
        assert!(h.has("Lip flap") && h.has("KUCHIPAKU (LIP FLAP)"), "{}", h.describe());
        let form = h.editor().lip_form.as_ref().expect("the window's form");
        assert_eq!(form.track, 0, "the dialogue track");
        assert_eq!(form.mouths, [Some(ds[0]), Some(ds[1]), Some(ds[2])]);
        assert_eq!((form.from, form.to), (0, 11), "the whole cut (no loop range)");
        assert_eq!((form.lo, form.hi), LIP_THRESHOLDS);
        assert!(h.has_text_containing("12 frames: 4 open, 4 half, 4 closed"), "{}", h.describe());
        let steps = h.state().engine.position();
        h.click("generate");
        assert_eq!(h.state().engine.position(), steps + 1, "one undo step");
        let shown: Vec<Exposure> = (0..12).map(|f| h.state().cut().xsheet.columns[0].shown(f)).collect();
        let d = |i: usize| Exposure::Drawing(ds[i]);
        let want = [0, 0, 0, 0, 2, 2, 2, 2, 1, 1, 1, 1].map(d);
        assert_eq!(shown, want);
        assert_eq!(h.state().status, "lip flap: frames 1–12 written (one undo step)");
        // The window stays (non-modal); undo takes the whole write back.
        assert!(h.editor().lip_form.is_some());
        h.perform(Action::Undo);
        assert_eq!(h.state().cut().xsheet.columns[0].shown(1), d(1));
    }

    #[test]
    fn playback_ram_preview_holds_the_play_range_and_an_edit_drops_only_its_frames() {
        let mut s = AppState::new_project("t", 320, 240, 24, 300.0);
        let at = s.at();
        s.engine
            .apply("len", vec![Command::SetFrameCount { at, frames: 6 }])
            .unwrap();
        for f in [0, 3] {
            s.goto(f);
            s.new_drawing_at_frame();
        }
        s.goto(0);
        let cut = s.view.cut;
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Viewer(0)]);
        h.run();
        assert_eq!(h.rig().config.perf.ram_preview_mb, DEFAULT_RAM_MB);
        // The verb draws the play range on a worker.
        assert_eq!(h.perform(Action::RamPreview), Performed::Done);
        assert!(h.state().ram.busy());
        assert_eq!(h.editor().is_on(Action::RamPreview), Some(true));
        assert_eq!(h.state().status, "RAM preview: rendering 6 frames…");
        h.state_mut().ram.wait();
        let held = |h: &EditorHarness| (0..6).map(|f| h.state().ram.held(cut, f)).collect::<Vec<_>>();
        assert_eq!(held(&h), [true; 6]);
        assert_eq!(h.state().ram.usage(), (6, 2), "6 × 300 KB");
        // Again with every frame held: nothing to draw.
        h.perform(Action::RamPreview);
        assert_eq!(h.state().status, "RAM preview: frames 1–6 are held");
        // An edit drops just the frames it changed: a drawing keyed on the
        // last frame changes that frame alone.
        h.state_mut().goto(5);
        h.state_mut().new_drawing_at_frame();
        h.run_frames(1);
        assert_eq!(held(&h), [true, true, true, true, true, false]);
        // Playing, the viewer shows the held frame from memory.
        h.state_mut().goto(1);
        h.state_mut().view.playing = true;
        h.run_frames(1);
        let f = h.state().view.frame;
        assert!(f < 5, "{f}");
        assert_eq!(h.state().ram.showing(), Some((cut, f)));
        h.state_mut().view.playing = false;
        // A smaller cap holds fewer (300 KB a frame: 1 MB holds 3), and a
        // full cap is said, not ignored.
        h.rig_mut().config.perf.ram_preview_mb = 1;
        h.run_frames(1);
        assert_eq!(h.state().ram.usage().0, 3);
        h.perform(Action::RamPreview);
        assert_eq!(
            h.state().refusal,
            "refused — the RAM preview cap (1 MB) is full; raise it in Settings ▸ Performance"
        );
        // A frame bigger than the cap is refused before any work.
        let s = h.state_mut();
        let mut big = RamPreview::default();
        big.set_cap_mb(1);
        let look = RamLook {
            at: s.at(),
            size: (1920, 1080),
            law: s.engine.project.color_law,
            final_look: false,
        };
        big.toggle();
        let r = big.sync(&mut s.engine, look, (0, 5), None);
        assert!(
            matches!(&r, Some(Err(m)) if m.contains("larger than the RAM preview cap")),
            "{r:?}"
        );
        assert!(!big.busy());
    }
}
