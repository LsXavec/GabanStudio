//! THE RULE's data model (spec §5.1) — a drawing's life as one interval.
//!
//! The core crate stores timing as sparse keys with hold semantics, so runs
//! are the model's NATIVE shape; the old per-row `key_at` rendering painted
//! 47 disconnected islands out of what the document says is one duration.
//! This module asks the sheet the right question, once per paint.
//!
//! Governed by research/PSD-editor-repaint.md: this file READS the document
//! and never writes it.

use anim_core::ids::DrawingId;
use anim_core::xsheet::Exposure;

/// One exposed interval: `drawing` shows from `start` through `end`
/// inclusive, in sheet frames.
pub struct Run {
    pub start: u32,
    pub end: u32,
    pub drawing: DrawingId,
}

/// Everything THE RULE paints for one column.
pub struct ColumnMarks {
    pub runs: Vec<Run>,
    /// Frames carrying an explicit Empty key (the hold-terminator ring).
    pub empties: Vec<u32>,
}

/// Build a column's marks by walking its keys. `key_at` is the column's own
/// query (kept as a closure so this module names no concrete column type).
pub fn column_marks(count: u32, key_at: impl Fn(u32) -> Option<Exposure>) -> ColumnMarks {
    let mut runs = Vec::new();
    let mut empties = Vec::new();
    let mut open: Option<(DrawingId, u32)> = None;
    for f in 0..count {
        match key_at(f) {
            Some(Exposure::Drawing(d)) => {
                if let Some((pd, start)) = open.take() {
                    runs.push(Run {
                        start,
                        end: f.saturating_sub(1),
                        drawing: pd,
                    });
                }
                open = Some((d, f));
            }
            Some(Exposure::Empty) => {
                if let Some((pd, start)) = open.take() {
                    runs.push(Run {
                        start,
                        end: f.saturating_sub(1),
                        drawing: pd,
                    });
                }
                empties.push(f);
            }
            None => {}
        }
    }
    if let Some((pd, start)) = open {
        runs.push(Run {
            start,
            end: count.saturating_sub(1),
            drawing: pd,
        });
    }
    ColumnMarks { runs, empties }
}

/// The rule weight for the boundary ABOVE frame `f`, if any:
/// second (fps) > half (fps/2) > beat (fps/4). Returns the tier 0..=2
/// (0 = beat, 1 = half, 2 = second) so callers map tiers to inks.
pub fn rule_tier(f: u32, fps: u32) -> Option<u8> {
    if fps == 0 {
        return None;
    }
    if f % fps == 0 {
        Some(2)
    } else if fps >= 2 && f % (fps / 2).max(1) == 0 {
        Some(1)
    } else if fps >= 4 && f % (fps / 4).max(1) == 0 {
        Some(0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn marks_of(keys: &[(u32, Option<u32>)], count: u32) -> ColumnMarks {
        // (frame, Some(raw drawing id)) = Drawing key, None = Empty key.
        let map: BTreeMap<u32, Option<DrawingId>> = keys
            .iter()
            .map(|(f, d)| (*f, d.map(|raw| DrawingId(raw as u64))))
            .collect();
        column_marks(count, |f| {
            map.get(&f).map(|d| match d {
                Some(id) => Exposure::Drawing(*id),
                None => Exposure::Empty,
            })
        })
    }

    #[test]
    fn holds_become_single_runs_not_islands() {
        let m = marks_of(&[(0, Some(1)), (6, Some(2)), (12, None)], 24);
        assert_eq!(m.runs.len(), 2);
        assert_eq!((m.runs[0].start, m.runs[0].end), (0, 5));
        assert_eq!((m.runs[1].start, m.runs[1].end), (6, 11));
        assert_eq!(m.empties, vec![12]);
    }

    #[test]
    fn a_run_reaches_the_sheet_end_without_a_terminator() {
        let m = marks_of(&[(3, Some(7))], 48);
        assert_eq!(m.runs.len(), 1);
        assert_eq!((m.runs[0].start, m.runs[0].end), (3, 47));
        assert!(m.empties.is_empty());
    }

    #[test]
    fn adjacent_keys_touch_without_overlap() {
        let m = marks_of(&[(0, Some(1)), (1, Some(2))], 4);
        assert_eq!((m.runs[0].start, m.runs[0].end), (0, 0));
        assert_eq!((m.runs[1].start, m.runs[1].end), (1, 3));
    }

    #[test]
    fn rule_tiers_at_24fps() {
        // seconds at 0/24, halves at 12, beats at 6/18.
        assert_eq!(rule_tier(0, 24), Some(2));
        assert_eq!(rule_tier(24, 24), Some(2));
        assert_eq!(rule_tier(12, 24), Some(1));
        assert_eq!(rule_tier(6, 24), Some(0));
        assert_eq!(rule_tier(18, 24), Some(0));
        assert_eq!(rule_tier(7, 24), None);
    }
}
