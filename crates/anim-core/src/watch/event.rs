//! What Workflow Watch is told, what it answers, and the tile box of a step.

use crate::command::{Command, CutRef};
use crate::ids::{ColumnId, DrawingId};
use crate::raster::{TILE, TileDiff};
use crate::xsheet::Exposure;

use super::spans::KeySpan;

/// The checks. A map key in the saved profile: never rename a variant.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum CheckId {
    UndoLoop,
    KeyTime,
    OverPolish,
}

impl CheckId {
    pub const ALL: [CheckId; 3] = [CheckId::UndoLoop, CheckId::KeyTime, CheckId::OverPolish];
}

/// Which drawing a step worked on. Drawing on an empty cell mints a new id
/// every time, so a step that made its drawing is known by its cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawKey {
    Drawing(DrawingId),
    Cell { column: ColumnId, frame: u32 },
}

/// A frame range, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRange {
    pub from: u32,
    pub to: u32,
}

/// Where a step landed. `drawing` is None once an undo has removed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spot {
    pub at: CutRef,
    pub key: DrawKey,
    pub drawing: Option<DrawingId>,
    pub frames: FrameRange,
}

/// A box in paper units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaperBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl PaperBox {
    pub fn from_array(b: [f32; 4]) -> Self {
        Self { x0: b[0].min(b[2]), y0: b[1].min(b[3]), x1: b[0].max(b[2]), y1: b[1].max(b[3]) }
    }

    pub fn union(self, o: PaperBox) -> PaperBox {
        PaperBox {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    pub fn area(self) -> f64 {
        (self.x1 - self.x0).max(0.0) as f64 * (self.y1 - self.y0).max(0.0) as f64
    }
}

/// A box of tiles, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileBox {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl TileBox {
    pub fn around(x: i32, y: i32) -> Self {
        Self { x0: x, y0: y, x1: x, y1: y }
    }

    /// The tiles a paper box touches.
    pub fn of_paper(b: PaperBox) -> Self {
        let t = TILE as f32;
        Self {
            x0: (b.x0 / t).floor() as i32,
            y0: (b.y0 / t).floor() as i32,
            x1: (b.x1 / t).floor() as i32,
            y1: (b.y1 / t).floor() as i32,
        }
    }

    pub fn union(self, o: TileBox) -> TileBox {
        TileBox {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    pub fn area(self) -> i64 {
        (self.x1 as i64 - self.x0 as i64 + 1) * (self.y1 as i64 - self.y0 as i64 + 1)
    }

    /// The share of the smaller box that the other one covers.
    pub fn overlap(self, o: TileBox) -> f64 {
        let w = self.x1.min(o.x1) as i64 - self.x0.max(o.x0) as i64 + 1;
        let h = self.y1.min(o.y1) as i64 - self.y0.max(o.y0) as i64 + 1;
        if w <= 0 || h <= 0 {
            return 0.0;
        }
        (w * h) as f64 / self.area().min(o.area()) as f64
    }

    /// The box in paper units, for Show me's highlight.
    pub fn paper(self) -> PaperBox {
        let t = TILE as f32;
        PaperBox {
            x0: self.x0 as f32 * t,
            y0: self.y0 as f32 * t,
            x1: (self.x1 + 1) as f32 * t,
            y1: (self.y1 + 1) as f32 * t,
        }
    }
}

fn diff_box(diff: &TileDiff) -> Option<TileBox> {
    diff.iter()
        .map(|(c, _, _)| TileBox::around(c.0, c.1))
        .reduce(TileBox::union)
}

/// A step the undo loop can read: one drawing, painted tiles, their box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepBox {
    pub at: CutRef,
    pub key: DrawKey,
    pub drawing: DrawingId,
    pub tiles: TileBox,
}

/// Read one history step. None for anything but raster paint on one drawing:
/// the undo loop is raster only, so vector strokes and sheet edits stay silent.
pub fn step_box(commands: &[Command]) -> Option<StepBox> {
    let mut found: Option<(CutRef, DrawingId, TileBox)> = None;
    for c in commands {
        if let Command::PaintTiles { at, id, diff, .. } = c {
            let Some(b) = diff_box(diff) else { continue };
            found = match found {
                None => Some((*at, *id, b)),
                Some((a, d, _)) if a != *at || d != *id => return None,
                Some((a, d, u)) => Some((a, d, u.union(b))),
            };
        }
    }
    let (at, drawing, tiles) = found?;
    let minted = commands
        .iter()
        .any(|c| matches!(c, Command::AddDrawing { id, .. } if *id == drawing));
    let cell = commands.iter().find_map(|c| match c {
        Command::SetCell { column, frame, key: Some(Exposure::Drawing(d)), .. }
            if minted && *d == drawing =>
        {
            Some(DrawKey::Cell { column: *column, frame: *frame })
        }
        _ => None,
    });
    Some(StepBox { at, key: cell.unwrap_or(DrawKey::Drawing(drawing)), drawing, tiles })
}

/// What the app tells Watch. Each is sent at most once a frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A frame passed. `pen_down` = a stroke is live.
    Frame { pen_down: bool },
    /// A stroke landed. `area` is the stroke's box; `paper_area` the paper's
    /// area, read for each stroke. An eraser stroke also counts as an erase.
    Stroke { spot: Spot, area: PaperBox, erased: bool, paper_area: f64 },
    /// An undo, or a Clear selection, with the tiles it touched. A whole-layer
    /// clear is never sent: its box would overlap everything.
    UndoOrErase { spot: Spot, tiles: TileBox },
    /// The current drawing changed. None = the cell holds no drawing yet.
    Focus { at: CutRef, drawing: Option<DrawingId> },
    /// The view's zoom changed (4.0 = 400%).
    Zoom { zoom: f32 },
    /// The pen came into range, or left it.
    Proximity { in_range: bool },
    /// The in-betweens of a cut may have changed: judge its spans.
    Spans { at: CutRef, spans: Vec<KeySpan> },
    /// A cut was exported or marked done.
    CutFinished { at: CutRef },
}

/// What a fire points at, for Show me.
#[derive(Debug, Clone, PartialEq)]
pub enum Evidence {
    Area(PaperBox),
    Drawings(Vec<DrawingId>),
}

/// How a fire reaches the artist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Card,
    /// A retired check: the sheet cell's quiet dot, never a card.
    Dot,
}

/// One check firing.
#[derive(Debug, Clone, PartialEq)]
pub struct Fire {
    pub check: CheckId,
    pub at: CutRef,
    pub drawing: Option<DrawingId>,
    pub frames: FrameRange,
    pub evidence: Evidence,
    pub delivery: Delivery,
}

/// A notice shown once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    Retired { check: CheckId, shots: u32 },
    NextLevel { next: super::Level },
}

/// What one event produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    Fire(Fire),
    Notice(Notice),
}
