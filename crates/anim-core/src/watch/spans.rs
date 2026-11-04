//! The key check's spans: which in-betweens lie between which keys.

use std::collections::BTreeSet;

use crate::ids::{ColumnId, DrawingId};
use crate::model::{Cut, DrawingKind};
use crate::xsheet::Exposure;

use super::event::FrameRange;

/// One end of a span. A breakdown bounds a span but is not judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanEnd {
    pub drawing: DrawingId,
    pub is_key: bool,
}

/// The in-betweens between two ends, all with content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySpan {
    pub column: ColumnId,
    pub frames: FrameRange,
    pub from: SpanEnd,
    pub to: SpanEnd,
    pub inbetweens: Vec<DrawingId>,
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Key,
    Breakdown,
    Inbetween,
    /// Not marked: no guess, so a span holding one is never judged.
    Unknown,
}

fn has_content(cut: &Cut, id: DrawingId) -> bool {
    cut.drawings.iter().any(|d| {
        d.id == id && (!d.strokes.is_empty() || d.layers.iter().any(|l| !l.raster.tiles.is_empty()))
    })
}

fn roles(cut: &Cut) -> impl Fn(DrawingId) -> Role + '_ {
    let chart_ends: BTreeSet<DrawingId> = cut
        .drawing_defs
        .values()
        .filter_map(|d| d.chart.as_ref().map(|c| c.to))
        .collect();
    move |id| match cut.drawing_kind(id) {
        DrawingKind::Key => Role::Key,
        DrawingKind::Breakdown => Role::Breakdown,
        DrawingKind::Inbetween => Role::Inbetween,
        DrawingKind::Unset if chart_ends.contains(&id) => Role::Key,
        DrawingKind::Unset => Role::Unknown,
    }
}

struct Open {
    from: SpanEnd,
    first_frame: u32,
    inbetweens: Vec<DrawingId>,
    /// An unmarked or empty in-between lies inside, so this span is dropped.
    spoiled: bool,
}

/// Every span of a cut that can be judged: both ends key-like or breakdowns,
/// at least one in-between, every in-between marked and holding content.
pub fn key_spans(cut: &Cut) -> Vec<KeySpan> {
    let role = roles(cut);
    let mut out = Vec::new();
    for column in &cut.xsheet.columns {
        let mut open: Option<Open> = None;
        for (frame, exposure) in column.keys() {
            let Exposure::Drawing(id) = exposure else {
                open = None;
                continue;
            };
            match role(id) {
                r @ (Role::Key | Role::Breakdown) => {
                    let end = SpanEnd { drawing: id, is_key: r == Role::Key };
                    if let Some(o) = open.take() {
                        if !o.spoiled && !o.inbetweens.is_empty() {
                            out.push(KeySpan {
                                column: column.id,
                                frames: FrameRange { from: o.first_frame, to: frame },
                                from: o.from,
                                to: end,
                                inbetweens: o.inbetweens,
                            });
                        }
                    }
                    open = Some(Open {
                        from: end,
                        first_frame: frame,
                        inbetweens: Vec::new(),
                        spoiled: false,
                    });
                }
                Role::Inbetween => {
                    if let Some(o) = open.as_mut() {
                        o.spoiled |= !has_content(cut, id);
                        if !o.inbetweens.contains(&id) {
                            o.inbetweens.push(id);
                        }
                    }
                }
                Role::Unknown => {
                    if let Some(o) = open.as_mut() {
                        o.spoiled = true;
                    }
                }
            }
        }
    }
    out
}
