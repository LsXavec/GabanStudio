//! GM5: pure X-sheet edit builders. Each reads the project and returns the
//! `Vec<Command>` for one gesture — the caller applies it with
//! `Engine::apply` as ONE undo step. Nothing here mutates the sheet: the
//! column verbs (hold, retime, reverse, paste) run on a copy of the column
//! and diff the keys into a `SetCells` batch, so every verb inherits that
//! command's exact inverse and its lock refusal. The sheet panel calls these
//! and adds no edit logic of its own.

use crate::command::{CellEdit, Command, CutRef};
use crate::error::{EngineError, Result};
use crate::ids::*;
use crate::model::{CelLayer, Cut, Drawing, Project};
use crate::xsheet::{Column, Exposure};

/// How a pasted block meets the cells already at the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteMode {
    /// The block replaces as many frames as it spans; nothing else moves.
    Overwrite,
    /// The block opens its own frames: what was at the target ripples later.
    Insert,
}

/// A copied range of cells: per column (left to right) the exposures shown
/// over the range as runs, plus the drawings they reference, so a paste
/// into another cut can copy the artwork.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// The cut it was copied from. Pasting back into it reuses the drawings.
    pub source: CutRef,
    /// Frames the block spans.
    pub len: u32,
    /// One entry per copied column: (exposure, length) runs summing to `len`.
    pub columns: Vec<Vec<(Exposure, u32)>>,
    /// Every drawing the runs reference, once each, in first-use order.
    pub drawings: Vec<Drawing>,
}

fn cut_at(project: &Project, at: CutRef) -> Result<&Cut> {
    project
        .scene(at.scene)
        .ok_or(EngineError::UnknownScene(at.scene))?
        .cuts
        .iter()
        .find(|c| c.id == at.cut)
        .ok_or(EngineError::UnknownCut(at.cut))
}

fn column_at(cut: &Cut, id: ColumnId) -> Result<&Column> {
    cut.xsheet.column(id).ok_or(EngineError::UnknownColumn(id))
}

/// `name` when it is free, else the first free of `name` + a, b, ... z,
/// then `name`-2, -3, ... (a pasted "1" beside the cut's own "1" is "1a").
fn free_name(name: &str, taken: &std::collections::HashSet<String>) -> String {
    if !taken.contains(name) {
        return name.to_string();
    }
    ('a'..='z')
        .map(|c| format!("{name}{c}"))
        .chain((2u32..).map(|n| format!("{name}-{n}")))
        .find(|n| !taken.contains(n))
        .expect("an unbounded sequence has a free name")
}

/// The cell edits that turn `before` into `after` (same column).
pub fn cells_diff(before: &Column, after: &Column) -> Vec<CellEdit> {
    let mut frames: Vec<u32> = before.keys().chain(after.keys()).map(|(f, _)| f).collect();
    frames.sort_unstable();
    frames.dedup();
    frames
        .into_iter()
        .filter_map(|frame| {
            let (b, a) = (before.key_at(frame), after.key_at(frame));
            (b != a).then_some(CellEdit {
                column: before.id,
                frame,
                before: b,
                after: a,
            })
        })
        .collect()
}

/// Run `edit` on a copy of each column and gather the diffs into one
/// `SetCells` (empty when nothing changed). `edit` returning false means a
/// key would pass the last frame number.
fn edit_columns(
    project: &Project,
    at: CutRef,
    columns: &[ColumnId],
    mut edit: impl FnMut(&mut Column) -> bool,
) -> Result<Vec<Command>> {
    let cut = cut_at(project, at)?;
    let mut cells = Vec::new();
    for &id in columns {
        let col = column_at(cut, id)?;
        let mut copy = col.clone();
        if !edit(&mut copy) {
            return Err(EngineError::InvalidCommand(
                "edit would push a key past the last frame number".into(),
            ));
        }
        cells.extend(cells_diff(col, &copy));
    }
    Ok(if cells.is_empty() {
        vec![]
    } else {
        vec![Command::SetCells { at, cells }]
    })
}

/// Hold ±`delta` on the exposure in force at `frame`, rippling the column.
/// −1 on a one-frame exposure removes it. Refused, saying why, when
/// nothing is exposed there or +1 would grow the last exposure (it already
/// holds to the end of the cut). Frames in messages are 1-based, as the
/// sheet shows them.
pub fn hold_delta(
    project: &Project,
    at: CutRef,
    column: ColumnId,
    frame: u32,
    delta: i32,
) -> Result<Vec<Command>> {
    let cut = cut_at(project, at)?;
    let end = cut.frame_count;
    match column_at(cut, column)?.exposure_at(frame) {
        None => {
            return Err(EngineError::InvalidCommand(format!(
                "nothing is exposed at frame {} to hold",
                frame as u64 + 1
            )));
        }
        Some((_, None)) if delta > 0 => {
            return Err(EngineError::InvalidCommand(
                "the last drawing already holds to the end of the cut: lengthen the cut first"
                    .into(),
            ));
        }
        _ => {}
    }
    edit_columns(project, at, &[column], |c| {
        c.hold_delta(frame, delta, end);
        true
    })
}

/// Retime `[from, to)` of each column to `step`s (1, 2 or 3): each drawing
/// in the range holds `step` frames and an empty (kara) run keeps its
/// length. The range start stays and what follows ripples — by the same
/// amount in every column, so a block (a body and its mouth) stays in
/// sync: a column with fewer drawings holds its last exposure on until the
/// longest column's new range ends.
pub fn retime_range(
    project: &Project,
    at: CutRef,
    columns: &[ColumnId],
    from: u32,
    to: u32,
    step: u32,
) -> Result<Vec<Command>> {
    if step == 0 || from >= to {
        return Err(EngineError::InvalidCommand(
            "retime needs a range and a step of at least 1".into(),
        ));
    }
    let cut = cut_at(project, at)?;
    let mut plans = Vec::new();
    for &id in columns {
        let col = column_at(cut, id)?;
        plans.push((col, col.retimed_runs(from, to, step)));
    }
    let length = |runs: &[(Exposure, u32)]| runs.iter().map(|r| r.1 as u64).sum::<u64>();
    let longest = plans.iter().map(|(_, runs)| length(runs)).max().unwrap_or(0);
    let too_far = || EngineError::InvalidCommand("edit would push a key past the last frame number".into());
    let mut cells = Vec::new();
    for (col, mut runs) in plans {
        let pad = u32::try_from(longest - length(&runs)).map_err(|_| too_far())?;
        if let Some(last) = runs.last_mut() {
            last.1 = last.1.checked_add(pad).ok_or_else(too_far)?;
        }
        let mut copy = col.clone();
        if !copy.splice(from, to - from, &runs) {
            return Err(too_far());
        }
        cells.extend(cells_diff(col, &copy));
    }
    Ok(if cells.is_empty() {
        vec![]
    } else {
        vec![Command::SetCells { at, cells }]
    })
}

/// Reverse `[from, to)` of each column: the drawing order flips and each
/// exposure keeps its length.
pub fn reverse_range(
    project: &Project,
    at: CutRef,
    columns: &[ColumnId],
    from: u32,
    to: u32,
) -> Result<Vec<Command>> {
    if from >= to {
        return Err(EngineError::InvalidCommand("reverse needs a range".into()));
    }
    edit_columns(project, at, columns, |c| c.reverse(from, to))
}

/// Typed entry: `name` exposes the drawing with that name at `frame`,
/// creating a blank one (with `layers`, the app's new-cel template) when the
/// cut has none; `x` makes the cell Empty (カラ). One gesture.
pub fn expose_by_name(
    project: &mut Project,
    at: CutRef,
    column: ColumnId,
    frame: u32,
    name: &str,
    layers: Vec<CelLayer>,
) -> Result<Vec<Command>> {
    let name = name.trim();
    if name.is_empty() {
        return Err(EngineError::InvalidCommand(
            "type a drawing name, or x for an empty cell".into(),
        ));
    }
    let cut = cut_at(project, at)?;
    let before = column_at(cut, column)?.key_at(frame);
    let set = |after: Exposure| Command::SetCells {
        at,
        cells: vec![CellEdit {
            column,
            frame,
            before,
            after: Some(after),
        }],
    };
    if name.eq_ignore_ascii_case("x") {
        return Ok(if before == Some(Exposure::Empty) {
            vec![]
        } else {
            vec![set(Exposure::Empty)]
        });
    }
    if let Some(d) = cut.drawings.iter().find(|d| d.name == name) {
        return Ok(if before == Some(Exposure::Drawing(d.id)) {
            vec![]
        } else {
            vec![set(Exposure::Drawing(d.id))]
        });
    }
    let index = cut.drawings.len();
    let id = DrawingId(project.alloc_id());
    Ok(vec![
        Command::AddDrawing {
            at,
            id,
            index,
            name: name.to_string(),
            strokes: vec![],
            layers,
        },
        set(Exposure::Drawing(id)),
    ])
}

/// Copy `[from, to)` of `columns` (left to right).
pub fn copy_block(
    project: &Project,
    at: CutRef,
    columns: &[ColumnId],
    from: u32,
    to: u32,
) -> Result<Block> {
    if from >= to || columns.is_empty() {
        return Err(EngineError::InvalidCommand("copy needs a range of cells".into()));
    }
    let cut = cut_at(project, at)?;
    let mut block = Block {
        source: at,
        len: to - from,
        columns: Vec::new(),
        drawings: Vec::new(),
    };
    for &id in columns {
        let runs = column_at(cut, id)?.runs(from, to);
        for (e, _) in &runs {
            if let Exposure::Drawing(d) = e
                && !block.drawings.iter().any(|x| x.id == *d)
            {
                block
                    .drawings
                    .push(cut.drawing(*d).ok_or(EngineError::UnknownDrawing(*d))?.clone());
            }
        }
        block.columns.push(runs);
    }
    Ok(block)
}

/// Paste `block` with its top-left cell at (`column`, `frame`) of cut `at`,
/// filling as many columns rightwards as it has. Within its own cut the
/// cells reference the same drawings; into another cut the drawings are
/// copied with new drawing and layer ids (same artwork). One gesture.
pub fn paste_block(
    project: &mut Project,
    block: &Block,
    at: CutRef,
    column: ColumnId,
    frame: u32,
    mode: PasteMode,
) -> Result<Vec<Command>> {
    let cut = cut_at(project, at)?;
    let start = cut
        .xsheet
        .columns
        .iter()
        .position(|c| c.id == column)
        .ok_or(EngineError::UnknownColumn(column))?;
    let width = block.columns.len();
    if start + width > cut.xsheet.columns.len() {
        return Err(EngineError::InvalidCommand(format!(
            "the block spans {width} columns; only {} from here",
            cut.xsheet.columns.len() - start
        )));
    }
    let targets: Vec<Column> = cut.xsheet.columns[start..start + width].to_vec();
    let library_len = cut.drawings.len();

    let mut commands = Vec::new();
    let mut map: Vec<(DrawingId, DrawingId)> = Vec::new();
    // A copy never takes a name the target cut already uses: typed entry
    // finds a drawing by its name, so "1" must stay the one it was.
    let mut taken: std::collections::HashSet<String> =
        cut.drawings.iter().map(|d| d.name.clone()).collect();
    if at != block.source {
        for (i, d) in block.drawings.iter().enumerate() {
            let name = free_name(&d.name, &taken);
            taken.insert(name.clone());
            let id = DrawingId(project.alloc_id());
            let layers = d
                .layers
                .iter()
                .map(|l| CelLayer {
                    id: LayerId(project.alloc_id()),
                    ..l.clone()
                })
                .collect();
            commands.push(Command::AddDrawing {
                at,
                id,
                index: library_len + i,
                name,
                strokes: d.strokes.clone(),
                layers,
            });
            map.push((d.id, id));
        }
    }
    let remap = |e: Exposure| match e {
        Exposure::Drawing(d) => Exposure::Drawing(
            map.iter().find(|(from, _)| *from == d).map_or(d, |(_, to)| *to),
        ),
        Exposure::Empty => Exposure::Empty,
    };
    let old_len = match mode {
        PasteMode::Overwrite => block.len,
        PasteMode::Insert => 0,
    };
    let mut cells = Vec::new();
    for (col, runs) in targets.iter().zip(&block.columns) {
        let runs: Vec<(Exposure, u32)> = runs.iter().map(|&(e, n)| (remap(e), n)).collect();
        let mut copy = col.clone();
        if !copy.splice(frame, old_len, &runs) {
            return Err(EngineError::InvalidCommand(
                "paste would push a key past the last frame number".into(),
            ));
        }
        cells.extend(cells_diff(col, &copy));
    }
    if !cells.is_empty() {
        commands.push(Command::SetCells { at, cells });
    }
    Ok(commands)
}
