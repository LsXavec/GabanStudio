//! The exposure sheet (X-sheet) — the master timing artifact.
//!
//! Core law of the whole application (verified against the real anime
//! pipeline): drawings and timing are SEPARATE. A column holds sparse timing
//! *keys*; a key exposes a drawing (or explicit emptiness) and HOLDS until the
//! next key. Retiming never duplicates or touches artwork.

use std::collections::BTreeMap;

use crate::ids::{ColumnId, DrawingId};

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

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct XSheet {
    pub columns: Vec<Column>,
}

impl XSheet {
    pub fn column(&self, id: ColumnId) -> Option<&Column> {
        self.columns.iter().find(|c| c.id == id)
    }

    pub fn column_mut(&mut self, id: ColumnId) -> Option<&mut Column> {
        self.columns.iter_mut().find(|c| c.id == id)
    }
}
