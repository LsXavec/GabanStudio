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
