//! The exposure sheet (X-sheet) — the master timing artifact.
//!
//! Core law of the whole application (verified against the real anime
//! pipeline): drawings and timing are SEPARATE. A column holds sparse timing
//! *keys*; a key exposes a drawing (or explicit emptiness) and HOLDS until the
//! next key. Retiming never duplicates or touches artwork.

use std::collections::BTreeMap;

use crate::ids::{ColumnId, DrawingId, ParamId};

/// An explicit key on the sheet: either "show this drawing from here" or
/// "explicitly nothing from here" (ends the previous hold).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Exposure {
    Drawing(DrawingId),
    Empty,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Column {
    pub id: ColumnId,
    pub name: String,
    /// frame -> key. Sparse: frames between keys resolve via hold semantics.
    keys: BTreeMap<u32, Exposure>,
}

impl Column {
    pub fn new(id: ColumnId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            keys: BTreeMap::new(),
        }
    }

    /// Set a key at `frame`. Returns the previous key at that exact frame.
    pub fn set_key(&mut self, frame: u32, exposure: Exposure) -> Option<Exposure> {
        self.keys.insert(frame, exposure)
    }

    /// Remove the key at `frame` (the previous key's hold then extends over it).
    /// Returns the removed key.
    pub fn clear_key(&mut self, frame: u32) -> Option<Exposure> {
        self.keys.remove(&frame)
    }

    /// The key at this exact frame, if any.
    pub fn key_at(&self, frame: u32) -> Option<Exposure> {
        self.keys.get(&frame).copied()
    }

    /// Hold-aware resolution: the drawing visible at `frame` is the one exposed
    /// by the nearest key at-or-before `frame`. Before the first key: nothing.
    pub fn resolve(&self, frame: u32) -> Option<DrawingId> {
        match self.keys.range(..=frame).next_back() {
            Some((_, Exposure::Drawing(d))) => Some(*d),
            Some((_, Exposure::Empty)) | None => None,
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = (u32, Exposure)> + '_ {
        self.keys.iter().map(|(f, e)| (*f, *e))
    }

    /// All frames that have keys referencing the given drawing.
    pub fn keys_referencing(&self, drawing: DrawingId) -> Vec<u32> {
        self.keys
            .iter()
            .filter(|(_, e)| **e == Exposure::Drawing(drawing))
            .map(|(f, _)| *f)
            .collect()
    }

    // ---- GM5: the sheet verbs, as pure edits of one column ---------------
    // Commands and sheet_ops builders run these on a copy and diff the keys,
    // so every verb lands as an exact-inverse SetCells batch.

    /// The frame of the last key, if any.
    pub fn last_key(&self) -> Option<u32> {
        self.keys.keys().next_back().copied()
    }

    /// What `frame` shows, as a key value: the hold in force, or Empty
    /// before the first key (both show nothing).
    pub fn shown(&self, frame: u32) -> Exposure {
        match self.resolve(frame) {
            Some(d) => Exposure::Drawing(d),
            None => Exposure::Empty,
        }
    }

    /// The exposure in force at `frame`: its key frame and the frame of the
    /// next key (None = it holds on past the cut's end). None before the
    /// first key.
    pub fn exposure_at(&self, frame: u32) -> Option<(u32, Option<u32>)> {
        let (&k, _) = self.keys.range(..=frame).next_back()?;
        let after = (std::ops::Bound::Excluded(frame), std::ops::Bound::Unbounded);
        Some((k, self.keys.range(after).next().map(|(f, _)| *f)))
    }

    /// Open `n` frames at `frame`: every key at or after it moves `n` later.
    /// False (and nothing moves) when a key would pass `u32::MAX`.
    pub fn insert_frames(&mut self, frame: u32, n: u32) -> bool {
        insert_frames_in(&mut self.keys, frame, n)
    }

    /// Close `n` frames at `frame`: keys inside `[frame, frame + n)` are
    /// removed and returned first (the inverse restores them), later keys
    /// move `n` earlier, and the frames after keep showing what they
    /// showed: when a removed key was holding over them, a key re-exposing
    /// it lands at `frame`, returned second (the inverse lifts it).
    pub fn delete_frames(&mut self, frame: u32, n: u32) -> (Vec<(u32, Exposure)>, Option<Exposure>) {
        let tail = frame.checked_add(n).map(|end| self.shown(end));
        let gone = delete_frames_in(&mut self.keys, frame, n);
        let rekey = tail.filter(|&t| !self.keys.contains_key(&frame) && self.shown(frame) != t);
        if let Some(t) = rekey {
            self.keys.insert(frame, t);
        }
        (gone, rekey)
    }

    /// Hold ±: lengthen (+) or shorten (−) the exposure in force at `frame`
    /// by `delta` frames, rippling every later key. An exposure with no key
    /// after it runs to `end` (the cut's frame count): it cannot grow, and
    /// shortening it ends it early with an Empty key. Shortening by its
    /// whole length removes it. False when nothing changed.
    pub fn hold_delta(&mut self, frame: u32, delta: i32, end: u32) -> bool {
        let Some((k, next)) = self.exposure_at(frame) else {
            return false;
        };
        if delta > 0 {
            return match next {
                Some(n) => self.insert_frames(n, delta as u32),
                None => false,
            };
        }
        if delta == 0 {
            return false;
        }
        let stop = next.unwrap_or(end.max(k + 1));
        let d = delta.unsigned_abs().min(stop - k);
        match next {
            Some(n) => {
                self.delete_frames(n - d, d);
            }
            None if d == stop - k => {
                self.keys.remove(&k);
            }
            None => {
                self.keys.insert(stop - d, Exposure::Empty);
            }
        }
        true
    }

    /// The exposures shown in `[from, to)` as runs (exposure, length),
    /// neighbours showing the same thing merged.
    pub fn runs(&self, from: u32, to: u32) -> Vec<(Exposure, u32)> {
        let mut out: Vec<(Exposure, u32)> = Vec::new();
        let mut f = from;
        while f < to {
            let e = self.shown(f);
            let after = (std::ops::Bound::Excluded(f), std::ops::Bound::Excluded(to));
            let next = self.keys.range(after).next().map(|(k, _)| *k).unwrap_or(to);
            match out.last_mut() {
                Some((last, len)) if *last == e => *len += next - f,
                _ => out.push((e, next - f)),
            }
            f = next;
        }
        out
    }

    /// Replace the frames `[from, from + old_len)` with `runs`, moving every
    /// later key by the length difference. The frames after keep showing
    /// what they showed (a key is added where a run would otherwise hold on
    /// over them). The one rewrite under retime, reverse and paste. False
    /// (and nothing changes) when a key would pass `u32::MAX`.
    pub fn splice(&mut self, from: u32, old_len: u32, runs: &[(Exposure, u32)]) -> bool {
        let new_len: u64 = runs.iter().map(|r| r.1 as u64).sum();
        let old_end = from as u64 + old_len as u64;
        let grow = new_len.saturating_sub(old_len as u64);
        let top = self.last_key().map_or(0, |l| l as u64).max(old_end);
        if from as u64 + new_len > u32::MAX as u64 || top + grow > u32::MAX as u64 {
            return false;
        }
        let (old_end, new_end) = (old_end as u32, from + new_len as u32);
        let tail_shown = self.shown(old_end);
        let had_key = self.keys.contains_key(&from);
        let mut rest = self.keys.split_off(&from);
        let after = rest.split_off(&old_end);
        let mut f = from;
        for (i, &(e, len)) in runs.iter().enumerate() {
            if len == 0 {
                continue;
            }
            if (i == 0 && had_key) || self.shown(f) != e {
                self.keys.insert(f, e);
            }
            f += len;
        }
        for (k, e) in after {
            self.keys.insert(k - old_end + new_end, e);
        }
        if !self.keys.contains_key(&new_end) && self.shown(new_end) != tail_shown {
            self.keys.insert(new_end, tail_shown);
        }
        true
    }

    /// The runs a retime to `step`s writes over `[from, to)`: each drawing
    /// holds `step` frames; an empty (kara) run is a deliberate blank and
    /// keeps its length.
    pub fn retimed_runs(&self, from: u32, to: u32, step: u32) -> Vec<(Exposure, u32)> {
        self.runs(from, to)
            .into_iter()
            .map(|(e, len)| match e {
                Exposure::Drawing(_) => (e, step.max(1)),
                Exposure::Empty => (e, len),
            })
            .collect()
    }

    /// Retime `[from, to)` to `step`s (see [`Column::retimed_runs`]). The
    /// range start stays put; what follows ripples.
    pub fn retime(&mut self, from: u32, to: u32, step: u32) -> bool {
        let runs = self.retimed_runs(from, to, step);
        self.splice(from, to.saturating_sub(from), &runs)
    }

    /// Reverse `[from, to)`: the exposures run in the opposite order, each
    /// keeping its own length. Nothing outside the range moves.
    pub fn reverse(&mut self, from: u32, to: u32) -> bool {
        let mut runs = self.runs(from, to);
        runs.reverse();
        self.splice(from, to.saturating_sub(from), &runs)
    }
}

/// GM5: open `n` frames at `frame` in any frame-keyed map (exposures,
/// param keys, memos — D4's ripple is the same for each). False, and
/// nothing moves, when a key would pass `u32::MAX`.
pub fn insert_frames_in<T>(keys: &mut BTreeMap<u32, T>, frame: u32, n: u32) -> bool {
    if n == 0 {
        return true;
    }
    if keys.range(frame..).next_back().is_some_and(|(l, _)| l.checked_add(n).is_none()) {
        return false;
    }
    let moved = keys.split_off(&frame);
    keys.extend(moved.into_iter().map(|(f, v)| (f + n, v)));
    true
}

/// GM5: close `n` frames at `frame` in any frame-keyed map: entries in
/// `[frame, frame + n)` are removed and returned, later ones move `n`
/// earlier.
pub fn delete_frames_in<T>(keys: &mut BTreeMap<u32, T>, frame: u32, n: u32) -> Vec<(u32, T)> {
    if n == 0 {
        return Vec::new();
    }
    let mut gone = keys.split_off(&frame);
    let later = match frame.checked_add(n) {
        Some(end) => gone.split_off(&end),
        None => BTreeMap::new(),
    };
    keys.extend(later.into_iter().map(|(f, v)| (f - n, v)));
    gone.into_iter().collect()
}

// ---- GM5: column kinds -------------------------------------------------------

/// What a column is in the composite. The column path renders in bands —
/// Bg, then Cel, then Book (an overlay held above the cels) — each band in
/// sheet order; Ref is a guide and never renders.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ColumnKind {
    #[default]
    Cel,
    Bg,
    Book,
    Ref,
}

impl ColumnKind {
    pub const ALL: [ColumnKind; 4] = [
        ColumnKind::Cel,
        ColumnKind::Bg,
        ColumnKind::Book,
        ColumnKind::Ref,
    ];

    /// Stable persisted key (the column_defs JSON). Never rename these.
    pub fn key(self) -> &'static str {
        match self {
            ColumnKind::Cel => "cel",
            ColumnKind::Bg => "bg",
            ColumnKind::Book => "book",
            ColumnKind::Ref => "ref",
        }
    }

    pub fn from_key(key: &str) -> Option<ColumnKind> {
        ColumnKind::ALL.into_iter().find(|k| k.key() == key)
    }

    /// The badge on the sheet.
    pub fn label(self) -> &'static str {
        match self {
            ColumnKind::Cel => "Cel",
            ColumnKind::Bg => "BG",
            ColumnKind::Book => "Book",
            ColumnKind::Ref => "Ref",
        }
    }

    /// The badge's tooltip: the studio term, then what it does (D7).
    pub fn tooltip(self) -> &'static str {
        match self {
            ColumnKind::Cel => "cel: a drawing column, composited in sheet order",
            ColumnKind::Bg => "haikei (background): drawn under every cel",
            ColumnKind::Book => "book: an overlay held above the cels",
            ColumnKind::Ref => "ref (guide): shown for reference, never rendered",
        }
    }

    /// The render band, bottom → top. A Ref column sits in the Cel band for
    /// ordering (the canvas splits around it) but never renders.
    pub fn band(self) -> u8 {
        match self {
            ColumnKind::Bg => 0,
            ColumnKind::Cel | ColumnKind::Ref => 1,
            ColumnKind::Book => 2,
        }
    }
}

/// A column's definition: kind, hidden (out of every composite) and
/// locked (paint and sheet edits refuse). Lives beside the column in
/// `Cut::column_defs`, never in `content_hash`; the default is a plain
/// cel column and is never stored.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
pub struct ColumnDef {
    pub kind: ColumnKind,
    pub hidden: bool,
    pub locked: bool,
}

impl ColumnDef {
    /// Does this column reach the composite at all?
    pub fn renders(&self) -> bool {
        !self.hidden && self.kind != ColumnKind::Ref
    }

    /// The store's JSON row.
    pub fn to_json(&self) -> String {
        serde_json::json!({
            "kind": self.kind.key(),
            "hidden": self.hidden,
            "locked": self.locked,
        })
        .to_string()
    }

    /// Read a store row. None = not a JSON object (the caller reports the
    /// file corrupt); missing fields default and an unknown kind (a newer
    /// build's) reads as Cel rather than refusing the file.
    pub fn from_json(text: &str) -> Option<ColumnDef> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let o = v.as_object()?;
        let flag = |k: &str| o.get(k).and_then(|b| b.as_bool()).unwrap_or(false);
        Some(ColumnDef {
            kind: o
                .get("kind")
                .and_then(|k| k.as_str())
                .and_then(ColumnKind::from_key)
                .unwrap_or_default(),
            hidden: flag("hidden"),
            locked: flag("locked"),
        })
    }
}

// ---- Parameter columns (the C1 camera decision) -----------------------------
// Camera moves — and every future animated parameter — live on the SAME sheet
// as the drawings: a parameter column maps frame → number, and graph node
// params (Transform translate/scale/rotate) BIND to a column instead of
// holding a static value. The exposure sheet stays the master timing artifact.

/// How the value travels FROM a key TO the next key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ParamInterp {
    /// Step: hold this value until the next key (the sheet's native law).
    Hold,
    /// Straight line to the next key's value.
    Linear,
    /// Smoothstep ease-in-out to the next key's value (the classic camera
    /// slide: gentle start, gentle stop).
    Ease,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParamKey {
    pub value: f32,
    pub interp: ParamInterp,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParamColumn {
    pub id: ParamId,
    pub name: String,
    /// frame -> key. Sparse, like a drawing column.
    keys: BTreeMap<u32, ParamKey>,
}

impl ParamColumn {
    pub fn new(id: ParamId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            keys: BTreeMap::new(),
        }
    }

    /// Set a key at `frame`. Returns the previous key at that exact frame.
    pub fn set_key(&mut self, frame: u32, key: ParamKey) -> Option<ParamKey> {
        self.keys.insert(frame, key)
    }

    /// Remove the key at `frame`. Returns the removed key.
    pub fn clear_key(&mut self, frame: u32) -> Option<ParamKey> {
        self.keys.remove(&frame)
    }

    /// The key at this exact frame, if any.
    pub fn key_at(&self, frame: u32) -> Option<ParamKey> {
        self.keys.get(&frame).copied()
    }

    pub fn keys(&self) -> impl Iterator<Item = (u32, ParamKey)> + '_ {
        self.keys.iter().map(|(f, k)| (*f, *k))
    }

    /// GM5: open `n` frames at `frame` (InsertFrames ripples camera keys
    /// with the cels — D4). False when a key would pass `u32::MAX`.
    pub fn insert_frames(&mut self, frame: u32, n: u32) -> bool {
        insert_frames_in(&mut self.keys, frame, n)
    }

    /// GM5: close `n` frames at `frame`; the keys inside are returned.
    pub fn delete_frames(&mut self, frame: u32, n: u32) -> Vec<(u32, ParamKey)> {
        delete_frames_in(&mut self.keys, frame, n)
    }

    /// The value at `frame`.
    /// - No keys at all → `None` (a bound node falls back to its static value).
    /// - Before the first key → the first key's value (the camera holds its
    ///   opening position; it never jumps from an unrelated default).
    /// - At/after the last key → the last key's value.
    /// - Between two keys → interpolated per the EARLIER key's `interp`.
    pub fn resolve(&self, frame: u32) -> Option<f32> {
        let first = self.keys.iter().next()?;
        if frame <= *first.0 {
            return Some(first.1.value);
        }
        let (fa, ka) = self.keys.range(..=frame).next_back()?;
        // Excluded bound (not `frame + 1..`): frame == u32::MAX must take
        // the hold-last branch, not overflow.
        let after = (std::ops::Bound::Excluded(frame), std::ops::Bound::Unbounded);
        match self.keys.range(after).next() {
            None => Some(ka.value),
            Some((fb, kb)) => {
                let t = (frame - fa) as f32 / (fb - fa) as f32;
                let t = match ka.interp {
                    ParamInterp::Hold => return Some(ka.value),
                    ParamInterp::Linear => t,
                    ParamInterp::Ease => t * t * (3.0 - 2.0 * t),
                };
                Some(ka.value + (kb.value - ka.value) * t)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct XSheet {
    pub columns: Vec<Column>,
    /// Parameter columns (camera etc.), rendered after the drawing columns.
    #[serde(default)]
    pub params: Vec<ParamColumn>,
}

impl XSheet {
    pub fn column(&self, id: ColumnId) -> Option<&Column> {
        self.columns.iter().find(|c| c.id == id)
    }

    pub fn column_mut(&mut self, id: ColumnId) -> Option<&mut Column> {
        self.columns.iter_mut().find(|c| c.id == id)
    }

    pub fn param(&self, id: ParamId) -> Option<&ParamColumn> {
        self.params.iter().find(|p| p.id == id)
    }

    pub fn param_mut(&mut self, id: ParamId) -> Option<&mut ParamColumn> {
        self.params.iter_mut().find(|p| p.id == id)
    }
}
