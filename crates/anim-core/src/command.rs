//! Command-based editing with exact inverses.
//!
//! Every mutation of a document goes through a `Command`. Applying one returns
//! (a) the inverse command list that restores the prior state exactly, and
//! (b) the "invalidation roots" — nodes whose cached values (and downstream)
//! are no longer valid. The Engine owns history stacks built from these.

use std::collections::HashSet;

use crate::error::{EngineError, Result};
use crate::graph::{Node, NodeKind};
use crate::ids::*;
use crate::model::{AudioClip, CelLayer, Cut, Drawing, LayerProps, Project, Stroke};
use crate::raster::TileDiff;
use crate::xsheet::{Column, ColumnDef, Exposure, ParamColumn, ParamKey};

/// GM5: one cell of a SetCells batch. `before` is what the cell holds now
/// (checked on apply, so a stale batch refuses instead of drifting) and
/// `after` what it gets; None = no key there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CellEdit {
    pub column: ColumnId,
    pub frame: u32,
    pub before: Option<Exposure>,
    pub after: Option<Exposure>,
}

// Not `serde` — commands can carry `Arc<TileData>` (large pixel buffers), and
// the edit history lives in memory, never persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CutRef {
    pub scene: SceneId,
    pub cut: CutId,
}

// serde: RUNTIME wire only (the session mirror) — the disk format is
// SQLite via store.rs and never touches these derives.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Command {
    AddDrawing {
        at: CutRef,
        id: DrawingId,
        /// Position in the cut's drawing library (0..=len). User-created
        /// drawings append (pass `drawings.len()`); the inverse of a
        /// RemoveDrawing carries the original index so undo restores the
        /// library order byte-for-byte.
        index: usize,
        name: String,
        /// Usually empty for user-created drawings; carries full artwork when
        /// this command is the inverse of a RemoveDrawing.
        strokes: Vec<Stroke>,
        /// The drawing's full layer stack (ids + props + tiles). Usually the
        /// app's default template for user-created cels; carries everything
        /// when inverting a RemoveDrawing, so redo-stack commands referencing
        /// those LayerIds stay valid after undo.
        layers: Vec<CelLayer>,
    },
    /// Apply a tile-level raster edit (paint/erase) to ONE cel layer.
    /// Inverse swaps before/after.
    PaintTiles {
        at: CutRef,
        id: DrawingId,
        layer: LayerId,
        diff: TileDiff,
    },
    /// Insert a cel layer at `index` (0 = bottom; may equal len = on top).
    AddCelLayer {
        at: CutRef,
        drawing: DrawingId,
        index: usize,
        /// Usually a fresh empty layer; carries tiles when inverting a
        /// RemoveCelLayer.
        layer: CelLayer,
    },
    /// Remove a cel layer by id. The engine allows removing the last layer
    /// ("keep >= 1" is app policy, matching the remove-column precedent).
    RemoveCelLayer {
        at: CutRef,
        drawing: DrawingId,
        layer: LayerId,
    },
    /// Move a cel layer to `to_index` (remove-then-insert semantics, so
    /// move(i->j) then move(j->i) is an exact identity). Validates rather than
    /// clamps — a clamped command's recorded params would differ from its
    /// effect and its inverse would not be self-evidently exact.
    MoveCelLayer {
        at: CutRef,
        drawing: DrawingId,
        layer: LayerId,
        to_index: usize,
    },
    /// Replace a layer's whole property block (name/visible/opacity). One
    /// gesture = one apply; the exact inverse is the prior block.
    SetCelLayerProps {
        at: CutRef,
        drawing: DrawingId,
        layer: LayerId,
        props: LayerProps,
    },
    /// Import an image asset (PNG bytes) into the cut's library at `index`.
    /// The decode is validated BEFORE any mutation — bad bytes reject the
    /// whole batch atomically. Inverse: RemoveImage.
    AddImage {
        at: CutRef,
        id: ImageId,
        index: usize,
        name: String,
        bytes: Vec<u8>,
    },
    /// Remove an image asset. ImageSource nodes that still reference it stay
    /// wired and render transparent (the missing-image sentinel — the same
    /// tolerance as deleted columns). Inverse restores the exact bytes at
    /// the original library position.
    RemoveImage { at: CutRef, id: ImageId },
    RemoveDrawing {
        at: CutRef,
        id: DrawingId,
    },
    /// Rename a drawing. Undoable (unlike scene/cut renames): drawing names
    /// live in eval recipes, so a rename invalidates its consumers.
    RenameDrawing {
        at: CutRef,
        id: DrawingId,
        name: String,
    },
    /// Append one pen stroke to a drawing.
    AddStroke {
        at: CutRef,
        id: DrawingId,
        stroke: Stroke,
    },
    /// Remove a drawing's most recent stroke (the exact inverse of AddStroke).
    PopStroke {
        at: CutRef,
        id: DrawingId,
    },
    /// `key: Some(exposure)` sets a key at the frame; `None` clears the key
    /// (the previous key's hold then extends over it).
    SetCell {
        at: CutRef,
        column: ColumnId,
        frame: u32,
        key: Option<Exposure>,
    },
    /// Set/replace/remove the cut's scratch audio clip (C4). One clip per
    /// cut; the inverse carries the previous clip (Arc'd bytes — cheap).
    SetCutAudio {
        at: CutRef,
        audio: Option<AudioClip>,
    },
    /// Set/clear a key on a PARAMETER column (camera etc.) — the numeric
    /// sibling of SetCell, same exact-inverse shape.
    SetParamKey {
        at: CutRef,
        column: ParamId,
        frame: u32,
        key: Option<ParamKey>,
    },
    AddNode {
        at: CutRef,
        id: NodeId,
        kind: NodeKind,
    },
    RemoveNode {
        at: CutRef,
        id: NodeId,
    },
    Connect {
        at: CutRef,
        from: NodeId,
        from_pin: u16,
        to: NodeId,
        to_pin: u16,
    },
    Disconnect {
        at: CutRef,
        to: NodeId,
        to_pin: u16,
    },
    /// Parameter edits are expressed as replacing the node's kind (same
    /// variant, new params). If the input count shrinks, truncated
    /// connections are recorded in the inverse.
    SetNodeKind {
        at: CutRef,
        id: NodeId,
        kind: NodeKind,
    },
    SetOutput {
        at: CutRef,
        node: Option<NodeId>,
    },

    // ---- GM5: sheet scaffolding and sheet edits (D4) ---------------------
    /// Insert a drawing column at `index` (0 = first in the sheet). Usually
    /// a fresh empty column; carries its keys and definition when it is the
    /// inverse of a RemoveColumn, so undo restores id, keys and def.
    AddColumn {
        at: CutRef,
        index: usize,
        column: Column,
        def: ColumnDef,
    },
    /// Remove a column with its keys and definition (the engine allows the
    /// last one; "keep >= 1" is app policy).
    RemoveColumn { at: CutRef, id: ColumnId },
    RenameColumn {
        at: CutRef,
        id: ColumnId,
        name: String,
    },
    /// Move a column to `to_index` (remove-then-insert, like MoveCelLayer).
    MoveColumn {
        at: CutRef,
        id: ColumnId,
        to_index: usize,
    },
    AddParamColumn {
        at: CutRef,
        index: usize,
        column: ParamColumn,
    },
    RemoveParamColumn { at: CutRef, id: ParamId },
    /// Insert a whole cut into `scene` at `index`: a fresh cut, or the
    /// snapshot a RemoveCut took.
    AddCut {
        scene: SceneId,
        index: usize,
        cut: Box<Cut>,
    },
    RemoveCut { at: CutRef },
    RenameCut { at: CutRef, name: String },
    /// The cut's length. Keys past the new end are kept (hidden), never
    /// deleted, so shortening and lengthening again loses nothing.
    SetFrameCount { at: CutRef, frames: u32 },
    /// Open `count` frames at `frame` across the WHOLE cut (D4): every
    /// column's keys, every param key and every memo at or after it move
    /// later, and the cut grows by `count`.
    InsertFrames { at: CutRef, frame: u32, count: u32 },
    /// Close `count` frames at `frame` across the whole cut. The inverse
    /// re-opens them and restores the removed keys, param keys and memos.
    DeleteFrames { at: CutRef, frame: u32, count: u32 },
    /// A batch of cell edits as one command; the inverse is the batch
    /// reversed with before/after swapped.
    SetCells { at: CutRef, cells: Vec<CellEdit> },
    /// D3: set (Some) or remove (None) a cut meta key. `before` must match.
    SetCutMeta {
        at: CutRef,
        key: String,
        before: Option<String>,
        after: Option<String>,
    },
    SetColumnDef {
        at: CutRef,
        column: ColumnId,
        def: ColumnDef,
    },
    /// The memo/dialogue row: Some(text) sets the frame's memo, None (or
    /// empty text) clears it.
    SetMemo {
        at: CutRef,
        frame: u32,
        text: Option<String>,
    },
}

impl Command {
    pub fn cut_ref(&self) -> CutRef {
        match self {
            Command::AddImage { at, .. }
            | Command::RemoveImage { at, .. }
            | Command::AddDrawing { at, .. }
            | Command::PaintTiles { at, .. }
            | Command::AddCelLayer { at, .. }
            | Command::RemoveCelLayer { at, .. }
            | Command::MoveCelLayer { at, .. }
            | Command::SetCelLayerProps { at, .. }
            | Command::RemoveDrawing { at, .. }
            | Command::RenameDrawing { at, .. }
            | Command::AddStroke { at, .. }
            | Command::PopStroke { at, .. }
            | Command::SetCell { at, .. }
            | Command::SetCutAudio { at, .. }
            | Command::SetParamKey { at, .. }
            | Command::AddNode { at, .. }
            | Command::RemoveNode { at, .. }
            | Command::Connect { at, .. }
            | Command::Disconnect { at, .. }
            | Command::SetNodeKind { at, .. }
            | Command::SetOutput { at, .. }
            | Command::AddColumn { at, .. }
            | Command::RemoveColumn { at, .. }
            | Command::RenameColumn { at, .. }
            | Command::MoveColumn { at, .. }
            | Command::AddParamColumn { at, .. }
            | Command::RemoveParamColumn { at, .. }
            | Command::RemoveCut { at }
            | Command::RenameCut { at, .. }
            | Command::SetFrameCount { at, .. }
            | Command::InsertFrames { at, .. }
            | Command::DeleteFrames { at, .. }
            | Command::SetCells { at, .. }
            | Command::SetCutMeta { at, .. }
            | Command::SetColumnDef { at, .. }
            | Command::SetMemo { at, .. } => *at,
            Command::AddCut { scene, cut, .. } => CutRef {
                scene: *scene,
                cut: cut.id,
            },
        }
    }
}

/// GM5: a locked column's art and timing are protected — paint, strokes and
/// cel-layer edits (add, remove, move, props) on a drawing it exposes
/// refuse, naming the column (the app shows the reason on the Aka lane).
fn refuse_locked_art(cut: &Cut, id: DrawingId) -> Result<()> {
    for col in &cut.xsheet.columns {
        if cut.column_def(col.id).locked && !col.keys_referencing(id).is_empty() {
            return Err(locked(&col.name));
        }
    }
    Ok(())
}

/// GM5: the refusal every locked-column check returns.
fn locked(name: &str) -> EngineError {
    EngineError::InvalidCommand(format!("column {name} is locked"))
}

/// GM5: an insert or delete at `frame` would move (or remove) the keys of
/// a locked column at or after it — refused, the lock holds its timing.
fn refuse_locked_ripple(cut: &Cut, frame: u32) -> Result<()> {
    for col in &cut.xsheet.columns {
        if cut.column_def(col.id).locked && col.last_key().is_some_and(|l| l >= frame) {
            return Err(locked(&col.name));
        }
    }
    Ok(())
}

/// GM5: every node of a cut — the invalidation roots of an edit that moves
/// the whole timeline (or the whole cut).
fn all_nodes(cut: &Cut) -> Vec<NodeId> {
    cut.graph.nodes().map(|n| n.id).collect()
}

/// Result of applying one command.
pub struct AppliedEffect {
    /// Commands that exactly restore the pre-application state,
    /// in the order they must be applied.
    pub inverse: Vec<Command>,
    /// Nodes whose caches must be invalidated (downstream closure is
    /// expanded by the caller against the post-application graph).
    pub invalidation_roots: Vec<NodeId>,
}

/// Nodes to invalidate when a drawing's artwork changes (strokes, layer
/// pixels, or layer structure/props): the DrawingSource nodes of every column
/// that currently exposes it.
fn artwork_invalidation_roots(cut: &Cut, id: DrawingId) -> Vec<NodeId> {
    let mut roots = Vec::new();
    for col in &cut.xsheet.columns {
        if !col.keys_referencing(id).is_empty() {
            roots.extend(cut.graph.sources_of_column(col.id));
        }
    }
    roots
}

fn cut_mut(project: &mut Project, at: CutRef) -> Result<&mut Cut> {
    project
        .scene_mut(at.scene)
        .ok_or(EngineError::UnknownScene(at.scene))?
        .cuts
        .iter_mut()
        .find(|c| c.id == at.cut)
        .ok_or(EngineError::UnknownCut(at.cut))
}

/// Layer ids must be unique CUT-WIDE (the store's cel_layers PRIMARY KEY is
/// the layer id, so a duplicate anywhere would make the document unsaveable;
/// ids are minted from the project counter, so any reuse is a caller bug).
fn validate_layer_id_free(cut: &Cut, id: LayerId) -> Result<()> {
    if cut.drawings.iter().any(|d| d.layer(id).is_some()) {
        return Err(EngineError::InvalidCommand(format!(
            "layer id {id} already exists in this cut"
        )));
    }
    Ok(())
}

/// Opacity is part of the persisted format and the compositing math: reject
/// out-of-range and non-finite values at the command boundary (validate,
/// don't clamp — same law as MoveCelLayer) so NaN can never reach flatten()
/// or the GPU compositor.
fn validate_props(props: &LayerProps) -> Result<()> {
    if !props.opacity.is_finite() || !(0.0..=1.0).contains(&props.opacity) {
        return Err(EngineError::InvalidCommand(format!(
            "layer opacity {} outside 0..=1",
            props.opacity
        )));
    }
    Ok(())
}

pub fn apply_command(project: &mut Project, cmd: &Command) -> Result<AppliedEffect> {
    match cmd {
        Command::AddDrawing {
            at,
            id,
            index,
            name,
            strokes,
            layers,
        } => {
            let cut = cut_mut(project, *at)?;
            if cut.drawing(*id).is_some() {
                return Err(EngineError::InvalidCommand(format!(
                    "drawing {id} already exists"
                )));
            }
            if *index > cut.drawings.len() {
                return Err(EngineError::InvalidCommand(format!(
                    "drawing index {index} out of range (library has {})",
                    cut.drawings.len()
                )));
            }
            // Layer ids: distinct within the stack AND unused cut-wide;
            // props valid.
            for (i, l) in layers.iter().enumerate() {
                if layers[i + 1..].iter().any(|o| o.id == l.id) {
                    return Err(EngineError::InvalidCommand(format!(
                        "duplicate layer id {} in AddDrawing",
                        l.id
                    )));
                }
                validate_layer_id_free(cut, l.id)?;
                validate_props(&l.props)?;
            }
            cut.drawings.insert(
                *index,
                Drawing {
                    id: *id,
                    name: name.clone(),
                    strokes: strokes.clone(),
                    layers: layers.clone(),
                },
            );
            Ok(AppliedEffect {
                inverse: vec![Command::RemoveDrawing { at: *at, id: *id }],
                invalidation_roots: vec![],
            })
        }

        Command::AddImage {
            at,
            id,
            index,
            name,
            bytes,
        } => {
            let cut = cut_mut(project, *at)?;
            if cut.image(*id).is_some() {
                return Err(EngineError::InvalidCommand(format!(
                    "image {id} already exists"
                )));
            }
            if *index > cut.images.len() {
                return Err(EngineError::InvalidCommand(format!(
                    "image index {index} out of range (library has {})",
                    cut.images.len()
                )));
            }
            // Decode BEFORE mutating — a bad file must reject atomically.
            let asset = crate::model::ImageAsset::from_png(*id, name.clone(), bytes.clone())
                .map_err(EngineError::InvalidCommand)?;
            cut.images.insert(*index, asset);
            // Dangling ImageSource nodes (e.g. a redo after remove) now
            // resolve again — their cached values must drop.
            let roots = cut.graph.sources_of_image(*id);
            Ok(AppliedEffect {
                inverse: vec![Command::RemoveImage { at: *at, id: *id }],
                invalidation_roots: roots,
            })
        }

        Command::RemoveImage { at, id } => {
            let cut = cut_mut(project, *at)?;
            let idx = cut
                .images
                .iter()
                .position(|i| i.id == *id)
                .ok_or(EngineError::UnknownImage(*id))?;
            let roots = cut.graph.sources_of_image(*id);
            let img = cut.images.remove(idx);
            Ok(AppliedEffect {
                inverse: vec![Command::AddImage {
                    at: *at,
                    id: *id,
                    index: idx, // restore the original library position
                    name: img.name.clone(),
                    bytes: (*img.bytes).clone(),
                }],
                invalidation_roots: roots,
            })
        }

        Command::PaintTiles {
            at,
            id,
            layer,
            diff,
        } => {
            let cut = cut_mut(project, *at)?;
            refuse_locked_art(cut, *id)?;
            let roots = artwork_invalidation_roots(cut, *id);
            let drawing = cut
                .drawings
                .iter_mut()
                .find(|d| d.id == *id)
                .ok_or(EngineError::UnknownDrawing(*id))?;
            let cel_layer = drawing
                .layer_mut(*layer)
                .ok_or(EngineError::UnknownLayer(*layer))?;
            // Install the `after` tiles; remove where `after` is None.
            for (coord, _before, after) in diff {
                match after {
                    Some(tile) => {
                        cel_layer.raster.tiles.insert(*coord, tile.clone());
                    }
                    None => {
                        cel_layer.raster.tiles.remove(coord);
                    }
                }
            }
            // Exact inverse: swap before/after on every entry, REVERSED — so a
            // diff that touches the same coord twice still unwinds exactly
            // (last-write-wins forward, first-write-restored backward).
            let inverse_diff: TileDiff = diff
                .iter()
                .rev()
                .map(|(c, b, a)| (*c, a.clone(), b.clone()))
                .collect();
            Ok(AppliedEffect {
                inverse: vec![Command::PaintTiles {
                    at: *at,
                    id: *id,
                    layer: *layer,
                    diff: inverse_diff,
                }],
                invalidation_roots: roots,
            })
        }

        Command::AddCelLayer {
            at,
            drawing,
            index,
            layer,
        } => {
            let cut = cut_mut(project, *at)?;
            refuse_locked_art(cut, *drawing)?;
            let roots = artwork_invalidation_roots(cut, *drawing);
            validate_layer_id_free(cut, layer.id)?;
            validate_props(&layer.props)?;
            let d = cut
                .drawings
                .iter_mut()
                .find(|d| d.id == *drawing)
                .ok_or(EngineError::UnknownDrawing(*drawing))?;
            if *index > d.layers.len() {
                return Err(EngineError::InvalidCommand(format!(
                    "layer index {index} out of range (stack has {})",
                    d.layers.len()
                )));
            }
            d.layers.insert(*index, layer.clone());
            Ok(AppliedEffect {
                inverse: vec![Command::RemoveCelLayer {
                    at: *at,
                    drawing: *drawing,
                    layer: layer.id,
                }],
                invalidation_roots: roots,
            })
        }

        Command::RemoveCelLayer {
            at,
            drawing,
            layer,
        } => {
            let cut = cut_mut(project, *at)?;
            refuse_locked_art(cut, *drawing)?;
            let roots = artwork_invalidation_roots(cut, *drawing);
            let d = cut
                .drawings
                .iter_mut()
                .find(|d| d.id == *drawing)
                .ok_or(EngineError::UnknownDrawing(*drawing))?;
            let idx = d
                .layer_index(*layer)
                .ok_or(EngineError::UnknownLayer(*layer))?;
            let removed = d.layers.remove(idx);
            Ok(AppliedEffect {
                inverse: vec![Command::AddCelLayer {
                    at: *at,
                    drawing: *drawing,
                    index: idx,
                    layer: removed, // full clone: props + tiles (Arc = cheap)
                }],
                invalidation_roots: roots,
            })
        }

        Command::MoveCelLayer {
            at,
            drawing,
            layer,
            to_index,
        } => {
            let cut = cut_mut(project, *at)?;
            refuse_locked_art(cut, *drawing)?;
            let roots = artwork_invalidation_roots(cut, *drawing);
            let d = cut
                .drawings
                .iter_mut()
                .find(|d| d.id == *drawing)
                .ok_or(EngineError::UnknownDrawing(*drawing))?;
            let from = d
                .layer_index(*layer)
                .ok_or(EngineError::UnknownLayer(*layer))?;
            if *to_index >= d.layers.len() {
                return Err(EngineError::InvalidCommand(format!(
                    "move target {to_index} out of range (stack has {})",
                    d.layers.len()
                )));
            }
            let moved = d.layers.remove(from);
            d.layers.insert(*to_index, moved);
            Ok(AppliedEffect {
                inverse: vec![Command::MoveCelLayer {
                    at: *at,
                    drawing: *drawing,
                    layer: *layer,
                    to_index: from,
                }],
                invalidation_roots: roots,
            })
        }

        Command::SetCelLayerProps {
            at,
            drawing,
            layer,
            props,
        } => {
            validate_props(props)?;
            let cut = cut_mut(project, *at)?;
            refuse_locked_art(cut, *drawing)?;
            let roots = artwork_invalidation_roots(cut, *drawing);
            let d = cut
                .drawings
                .iter_mut()
                .find(|d| d.id == *drawing)
                .ok_or(EngineError::UnknownDrawing(*drawing))?;
            let cel_layer = d
                .layer_mut(*layer)
                .ok_or(EngineError::UnknownLayer(*layer))?;
            let prev = std::mem::replace(&mut cel_layer.props, props.clone());
            Ok(AppliedEffect {
                inverse: vec![Command::SetCelLayerProps {
                    at: *at,
                    drawing: *drawing,
                    layer: *layer,
                    props: prev,
                }],
                invalidation_roots: roots,
            })
        }

        Command::AddStroke { at, id, stroke } => {
            let cut = cut_mut(project, *at)?;
            refuse_locked_art(cut, *id)?;
            let roots = artwork_invalidation_roots(cut, *id);
            let drawing = cut
                .drawings
                .iter_mut()
                .find(|d| d.id == *id)
                .ok_or(EngineError::UnknownDrawing(*id))?;
            drawing.strokes.push(stroke.clone());
            Ok(AppliedEffect {
                inverse: vec![Command::PopStroke { at: *at, id: *id }],
                invalidation_roots: roots,
            })
        }

        Command::PopStroke { at, id } => {
            let cut = cut_mut(project, *at)?;
            refuse_locked_art(cut, *id)?;
            let roots = artwork_invalidation_roots(cut, *id);
            let drawing = cut
                .drawings
                .iter_mut()
                .find(|d| d.id == *id)
                .ok_or(EngineError::UnknownDrawing(*id))?;
            let stroke = drawing.strokes.pop().ok_or_else(|| {
                EngineError::InvalidCommand(format!("drawing {id} has no strokes to pop"))
            })?;
            Ok(AppliedEffect {
                inverse: vec![Command::AddStroke {
                    at: *at,
                    id: *id,
                    stroke,
                }],
                invalidation_roots: roots,
            })
        }

        Command::RemoveDrawing { at, id } => {
            let cut = cut_mut(project, *at)?;
            // It lifts keys from every column that exposes it.
            refuse_locked_art(cut, *id)?;
            let idx = cut
                .drawings
                .iter()
                .position(|d| d.id == *id)
                .ok_or(EngineError::UnknownDrawing(*id))?;
            let drawing = cut.drawings.remove(idx);

            // Clear every X-sheet key referencing this drawing; record them so
            // the inverse restores timing exactly.
            let mut restore_cells = Vec::new();
            let mut touched_columns = Vec::new();
            for col in &mut cut.xsheet.columns {
                let frames = col.keys_referencing(*id);
                if !frames.is_empty() {
                    touched_columns.push(col.id);
                }
                for frame in frames {
                    col.clear_key(frame);
                    restore_cells.push(Command::SetCell {
                        at: *at,
                        column: col.id,
                        frame,
                        key: Some(Exposure::Drawing(*id)),
                    });
                }
            }

            let mut inverse = vec![Command::AddDrawing {
                at: *at,
                id: *id,
                index: idx, // restore the original library position
                name: drawing.name.clone(),
                strokes: drawing.strokes.clone(),
                layers: drawing.layers.clone(), // full stack: ids+props+tiles
            }];
            inverse.extend(restore_cells);

            let mut roots = Vec::new();
            for col in touched_columns {
                roots.extend(cut.graph.sources_of_column(col));
            }
            Ok(AppliedEffect {
                inverse,
                invalidation_roots: roots,
            })
        }

        Command::SetCell {
            at,
            column,
            frame,
            key,
        } => {
            let cut = cut_mut(project, *at)?;
            // Validate referenced drawing exists before touching the column.
            if let Some(Exposure::Drawing(d)) = key
                && cut.drawing(*d).is_none() {
                    return Err(EngineError::UnknownDrawing(*d));
                }
            if cut.column_def(*column).locked
                && let Some(c) = cut.xsheet.column(*column)
            {
                return Err(locked(&c.name));
            }
            let col = cut
                .xsheet
                .column_mut(*column)
                .ok_or(EngineError::UnknownColumn(*column))?;
            let prev = match key {
                Some(exposure) => col.set_key(*frame, *exposure),
                None => col.clear_key(*frame),
            };
            Ok(AppliedEffect {
                inverse: vec![Command::SetCell {
                    at: *at,
                    column: *column,
                    frame: *frame,
                    key: prev,
                }],
                invalidation_roots: cut.graph.sources_of_column(*column),
            })
        }

        Command::RenameDrawing { at, id, name } => {
            let cut = cut_mut(project, *at)?;
            let roots = artwork_invalidation_roots(cut, *id);
            let d = cut
                .drawings
                .iter_mut()
                .find(|d| d.id == *id)
                .ok_or(EngineError::UnknownDrawing(*id))?;
            let prev = std::mem::replace(&mut d.name, name.clone());
            Ok(AppliedEffect {
                inverse: vec![Command::RenameDrawing {
                    at: *at,
                    id: *id,
                    name: prev,
                }],
                invalidation_roots: roots,
            })
        }

        Command::SetCutAudio { at, audio } => {
            let cut = cut_mut(project, *at)?;
            let prev = std::mem::replace(&mut cut.audio, audio.clone());
            Ok(AppliedEffect {
                inverse: vec![Command::SetCutAudio { at: *at, audio: prev }],
                // Audio never enters the render graph — nothing to invalidate.
                invalidation_roots: Vec::new(),
            })
        }

        Command::SetParamKey {
            at,
            column,
            frame,
            key,
        } => {
            // Values are part of the persisted format and the transform
            // math: reject non-finite at the boundary (validate, don't
            // clamp — same law as layer opacity) so NaN never reaches the
            // interpolator or the GPU.
            if let Some(k) = key
                && !k.value.is_finite()
            {
                return Err(EngineError::InvalidCommand(format!(
                    "param key value {} is not finite",
                    k.value
                )));
            }
            let cut = cut_mut(project, *at)?;
            let col = cut
                .xsheet
                .param_mut(*column)
                .ok_or_else(|| EngineError::InvalidCommand(format!(
                    "unknown param column {column}"
                )))?;
            let prev = match key {
                Some(k) => col.set_key(*frame, *k),
                None => col.clear_key(*frame),
            };
            Ok(AppliedEffect {
                inverse: vec![Command::SetParamKey {
                    at: *at,
                    column: *column,
                    frame: *frame,
                    key: prev,
                }],
                invalidation_roots: cut.graph.sources_of_param(*column),
            })
        }

        Command::AddNode { at, id, kind } => {
            let cut = cut_mut(project, *at)?;
            if cut.graph.contains(*id) {
                return Err(EngineError::InvalidCommand(format!(
                    "node {id} already exists"
                )));
            }
            cut.graph.add_node(Node::new(*id, kind.clone()));
            Ok(AppliedEffect {
                inverse: vec![Command::RemoveNode { at: *at, id: *id }],
                invalidation_roots: vec![*id],
            })
        }

        Command::RemoveNode { at, id } => {
            let cut = cut_mut(project, *at)?;
            let was_output = cut.graph.output == Some(*id);
            let (node, severed) = cut.graph.remove_node(*id)?;

            let mut inverse = vec![Command::AddNode {
                at: *at,
                id: *id,
                kind: node.kind.clone(),
            }];
            // Restore the removed node's own inputs...
            for (pin, input) in node.inputs.iter().enumerate() {
                if let Some((src, src_pin)) = input {
                    inverse.push(Command::Connect {
                        at: *at,
                        from: *src,
                        from_pin: *src_pin,
                        to: *id,
                        to_pin: pin as u16,
                    });
                }
            }
            // ...and the connections other nodes had into it.
            for (to, to_pin, from_pin) in &severed {
                inverse.push(Command::Connect {
                    at: *at,
                    from: *id,
                    from_pin: *from_pin,
                    to: *to,
                    to_pin: *to_pin,
                });
            }
            if was_output {
                inverse.push(Command::SetOutput {
                    at: *at,
                    node: Some(*id),
                });
            }

            // Direct consumers were severed; their downstream is still intact
            // in the graph, so they are valid closure roots. Include the
            // removed id itself so its own cache entries drop.
            let mut roots: Vec<NodeId> = severed.iter().map(|(to, _, _)| *to).collect();
            roots.push(*id);
            Ok(AppliedEffect {
                inverse,
                invalidation_roots: roots,
            })
        }

        Command::Connect {
            at,
            from,
            from_pin,
            to,
            to_pin,
        } => {
            let cut = cut_mut(project, *at)?;
            let prev = cut.graph.connect(*from, *from_pin, *to, *to_pin)?;
            let inverse = match prev {
                Some((p_from, p_from_pin)) => vec![Command::Connect {
                    at: *at,
                    from: p_from,
                    from_pin: p_from_pin,
                    to: *to,
                    to_pin: *to_pin,
                }],
                None => vec![Command::Disconnect {
                    at: *at,
                    to: *to,
                    to_pin: *to_pin,
                }],
            };
            Ok(AppliedEffect {
                inverse,
                invalidation_roots: vec![*to],
            })
        }

        Command::Disconnect { at, to, to_pin } => {
            let cut = cut_mut(project, *at)?;
            let prev = cut.graph.disconnect(*to, *to_pin)?;
            let inverse = match prev {
                Some((p_from, p_from_pin)) => vec![Command::Connect {
                    at: *at,
                    from: p_from,
                    from_pin: p_from_pin,
                    to: *to,
                    to_pin: *to_pin,
                }],
                None => vec![], // was already empty: no-op inverse
            };
            Ok(AppliedEffect {
                inverse,
                invalidation_roots: vec![*to],
            })
        }

        Command::SetNodeKind { at, id, kind } => {
            let cut = cut_mut(project, *at)?;
            let node = cut.graph.node_mut(*id)?;
            let prev_kind = node.kind.clone();
            let prev_inputs = node.inputs.clone();

            node.kind = kind.clone();
            let new_count = kind.input_count();
            node.inputs.resize(new_count, None);

            let mut inverse = vec![Command::SetNodeKind {
                at: *at,
                id: *id,
                kind: prev_kind,
            }];
            // Connections truncated by a shrink get restored on undo.
            for (pin, input) in prev_inputs.iter().enumerate().skip(new_count) {
                if let Some((src, src_pin)) = input {
                    inverse.push(Command::Connect {
                        at: *at,
                        from: *src,
                        from_pin: *src_pin,
                        to: *id,
                        to_pin: pin as u16,
                    });
                }
            }
            Ok(AppliedEffect {
                inverse,
                invalidation_roots: vec![*id],
            })
        }

        Command::SetOutput { at, node } => {
            let cut = cut_mut(project, *at)?;
            if let Some(n) = node
                && !cut.graph.contains(*n) {
                    return Err(EngineError::UnknownNode(*n));
                }
            let prev = cut.graph.output;
            cut.graph.output = *node;
            Ok(AppliedEffect {
                inverse: vec![Command::SetOutput {
                    at: *at,
                    node: prev,
                }],
                // Per-node caches are unaffected by which node is "the output".
                invalidation_roots: vec![],
            })
        }

        // ---- GM5 ----------------------------------------------------------
        Command::AddColumn {
            at,
            index,
            column,
            def,
        } => {
            let cut = cut_mut(project, *at)?;
            if cut.xsheet.column(column.id).is_some() {
                return Err(EngineError::InvalidCommand(format!(
                    "column {} already exists",
                    column.id
                )));
            }
            if *index > cut.xsheet.columns.len() {
                return Err(EngineError::InvalidCommand(format!(
                    "column index {index} out of range (sheet has {})",
                    cut.xsheet.columns.len()
                )));
            }
            // A locked column cannot be removed, so it is never added
            // locked either: the inverse (RemoveColumn) must always apply.
            // Add it, then lock it with SetColumnDef.
            if def.locked {
                return Err(EngineError::InvalidCommand(format!(
                    "column {} is added unlocked; lock it after",
                    column.name
                )));
            }
            for (_, e) in column.keys() {
                if let Exposure::Drawing(d) = e
                    && cut.drawing(d).is_none()
                {
                    return Err(EngineError::UnknownDrawing(d));
                }
            }
            cut.xsheet.columns.insert(*index, column.clone());
            cut.set_column_def(column.id, *def);
            Ok(AppliedEffect {
                inverse: vec![Command::RemoveColumn {
                    at: *at,
                    id: column.id,
                }],
                // A DrawingSource left reading this id resolves again.
                invalidation_roots: cut.graph.sources_of_column(column.id),
            })
        }

        Command::RemoveColumn { at, id } => {
            let cut = cut_mut(project, *at)?;
            let idx = cut
                .xsheet
                .columns
                .iter()
                .position(|c| c.id == *id)
                .ok_or(EngineError::UnknownColumn(*id))?;
            // The lock protects the column itself, not only its cells.
            if cut.column_def(*id).locked {
                return Err(locked(&cut.xsheet.columns[idx].name));
            }
            let column = cut.xsheet.columns.remove(idx);
            let def = cut.column_defs.remove(id).unwrap_or_default();
            Ok(AppliedEffect {
                inverse: vec![Command::AddColumn {
                    at: *at,
                    index: idx,
                    column,
                    def,
                }],
                // Its DrawingSources now read a missing column (transparent).
                invalidation_roots: cut.graph.sources_of_column(*id),
            })
        }

        Command::RenameColumn { at, id, name } => {
            let cut = cut_mut(project, *at)?;
            let col = cut
                .xsheet
                .column_mut(*id)
                .ok_or(EngineError::UnknownColumn(*id))?;
            let prev = std::mem::replace(&mut col.name, name.clone());
            Ok(AppliedEffect {
                inverse: vec![Command::RenameColumn {
                    at: *at,
                    id: *id,
                    name: prev,
                }],
                // Column names never enter eval recipes.
                invalidation_roots: vec![],
            })
        }

        Command::MoveColumn { at, id, to_index } => {
            let cut = cut_mut(project, *at)?;
            let from = cut
                .xsheet
                .columns
                .iter()
                .position(|c| c.id == *id)
                .ok_or(EngineError::UnknownColumn(*id))?;
            if *to_index >= cut.xsheet.columns.len() {
                return Err(EngineError::InvalidCommand(format!(
                    "move target {to_index} out of range (sheet has {})",
                    cut.xsheet.columns.len()
                )));
            }
            if cut.column_def(*id).locked {
                return Err(locked(&cut.xsheet.columns[from].name));
            }
            let moved = cut.xsheet.columns.remove(from);
            cut.xsheet.columns.insert(*to_index, moved);
            Ok(AppliedEffect {
                inverse: vec![Command::MoveColumn {
                    at: *at,
                    id: *id,
                    to_index: from,
                }],
                // Sheet order steers the column path only; graph values
                // read columns by id.
                invalidation_roots: vec![],
            })
        }

        Command::AddParamColumn { at, index, column } => {
            if column.keys().any(|(_, k)| !k.value.is_finite()) {
                return Err(EngineError::InvalidCommand(
                    "param column carries a non-finite key".into(),
                ));
            }
            let cut = cut_mut(project, *at)?;
            if cut.xsheet.param(column.id).is_some() {
                return Err(EngineError::InvalidCommand(format!(
                    "param column {} already exists",
                    column.id
                )));
            }
            if *index > cut.xsheet.params.len() {
                return Err(EngineError::InvalidCommand(format!(
                    "param column index {index} out of range (sheet has {})",
                    cut.xsheet.params.len()
                )));
            }
            cut.xsheet.params.insert(*index, column.clone());
            Ok(AppliedEffect {
                inverse: vec![Command::RemoveParamColumn {
                    at: *at,
                    id: column.id,
                }],
                invalidation_roots: cut.graph.sources_of_param(column.id),
            })
        }

        Command::RemoveParamColumn { at, id } => {
            let cut = cut_mut(project, *at)?;
            let idx = cut
                .xsheet
                .params
                .iter()
                .position(|p| p.id == *id)
                .ok_or_else(|| EngineError::InvalidCommand(format!("unknown param column {id}")))?;
            let column = cut.xsheet.params.remove(idx);
            Ok(AppliedEffect {
                inverse: vec![Command::AddParamColumn {
                    at: *at,
                    index: idx,
                    column,
                }],
                // Bound transforms fall back to their static values.
                invalidation_roots: cut.graph.sources_of_param(*id),
            })
        }

        Command::AddCut { scene, index, cut } => {
            if cut.frame_count == 0 {
                return Err(EngineError::InvalidCommand(
                    "a cut keeps at least one frame".into(),
                ));
            }
            if project
                .scenes
                .iter()
                .any(|s| s.cuts.iter().any(|c| c.id == cut.id))
            {
                return Err(EngineError::InvalidCommand(format!(
                    "cut {} already exists",
                    cut.id
                )));
            }
            let s = project
                .scene_mut(*scene)
                .ok_or(EngineError::UnknownScene(*scene))?;
            if *index > s.cuts.len() {
                return Err(EngineError::InvalidCommand(format!(
                    "cut index {index} out of range (scene has {})",
                    s.cuts.len()
                )));
            }
            s.cuts.insert(*index, (**cut).clone());
            Ok(AppliedEffect {
                inverse: vec![Command::RemoveCut {
                    at: CutRef {
                        scene: *scene,
                        cut: cut.id,
                    },
                }],
                invalidation_roots: all_nodes(cut),
            })
        }

        Command::RemoveCut { at } => {
            let s = project
                .scene_mut(at.scene)
                .ok_or(EngineError::UnknownScene(at.scene))?;
            let idx = s
                .cuts
                .iter()
                .position(|c| c.id == at.cut)
                .ok_or(EngineError::UnknownCut(at.cut))?;
            let cut = s.cuts.remove(idx);
            let roots = all_nodes(&cut);
            Ok(AppliedEffect {
                inverse: vec![Command::AddCut {
                    scene: at.scene,
                    index: idx,
                    cut: Box::new(cut),
                }],
                invalidation_roots: roots,
            })
        }

        Command::RenameCut { at, name } => {
            let cut = cut_mut(project, *at)?;
            let prev = std::mem::replace(&mut cut.name, name.clone());
            Ok(AppliedEffect {
                inverse: vec![Command::RenameCut { at: *at, name: prev }],
                invalidation_roots: vec![],
            })
        }

        Command::SetFrameCount { at, frames } => {
            if *frames == 0 {
                return Err(EngineError::InvalidCommand(
                    "a cut keeps at least one frame".into(),
                ));
            }
            let cut = cut_mut(project, *at)?;
            let prev = std::mem::replace(&mut cut.frame_count, *frames);
            Ok(AppliedEffect {
                inverse: vec![Command::SetFrameCount {
                    at: *at,
                    frames: prev,
                }],
                // Per-frame values do not depend on the cut's length.
                invalidation_roots: vec![],
            })
        }

        Command::InsertFrames { at, frame, count } => {
            let cut = cut_mut(project, *at)?;
            // Frames in messages are 1-based, as the sheet shows them. A
            // cut with no frames is refused: its inverse would delete
            // every frame, which a cut never allows.
            if *count == 0 || *frame > cut.frame_count || cut.frame_count == 0 {
                return Err(EngineError::InvalidCommand(format!(
                    "cannot insert {count} frame(s) at frame {}: the cut has {}",
                    *frame as u64 + 1,
                    cut.frame_count
                )));
            }
            let grown = cut.frame_count.checked_add(*count);
            let fits = |last: Option<u32>| last.is_none_or(|l| l < *frame || l.checked_add(*count).is_some());
            if grown.is_none()
                || !cut.xsheet.columns.iter().all(|c| fits(c.last_key()))
                || !cut.xsheet.params.iter().all(|p| fits(p.keys().last().map(|k| k.0)))
                || !fits(cut.memos.keys().next_back().copied())
            {
                return Err(EngineError::InvalidCommand(
                    "insert would push a key past the last frame number".into(),
                ));
            }
            refuse_locked_ripple(cut, *frame)?;
            for col in &mut cut.xsheet.columns {
                col.insert_frames(*frame, *count);
            }
            for p in &mut cut.xsheet.params {
                p.insert_frames(*frame, *count);
            }
            crate::xsheet::insert_frames_in(&mut cut.memos, *frame, *count);
            cut.frame_count = grown.expect("checked above");
            Ok(AppliedEffect {
                inverse: vec![Command::DeleteFrames {
                    at: *at,
                    frame: *frame,
                    count: *count,
                }],
                invalidation_roots: all_nodes(cut),
            })
        }

        Command::DeleteFrames { at, frame, count } => {
            let cut = cut_mut(project, *at)?;
            if *count == 0
                || *count >= cut.frame_count
                || frame.checked_add(*count).is_none_or(|end| end > cut.frame_count)
            {
                return Err(EngineError::InvalidCommand(format!(
                    "cannot delete {count} frame(s) at frame {}: the cut has {} and keeps at \
                     least one",
                    *frame as u64 + 1,
                    cut.frame_count
                )));
            }
            refuse_locked_ripple(cut, *frame)?;
            let mut inverse = vec![Command::InsertFrames {
                at: *at,
                frame: *frame,
                count: *count,
            }];
            // The frames after the range keep showing what they showed: a
            // column whose removed key held over them gets a key at `frame`
            // (Column::delete_frames). Once the inverse re-opens the range
            // that key sits at frame + count; it is lifted ahead of the
            // restore of the removed keys.
            let mut lifts = Vec::new();
            let mut cells = Vec::new();
            for col in &mut cut.xsheet.columns {
                let (gone, rekey) = col.delete_frames(*frame, *count);
                if let Some(e) = rekey {
                    lifts.push(CellEdit {
                        column: col.id,
                        frame: *frame + *count,
                        before: Some(e),
                        after: None,
                    });
                }
                for (f, e) in gone {
                    cells.push(CellEdit {
                        column: col.id,
                        frame: f,
                        before: None,
                        after: Some(e),
                    });
                }
            }
            lifts.extend(cells);
            if !lifts.is_empty() {
                inverse.push(Command::SetCells {
                    at: *at,
                    cells: lifts,
                });
            }
            for p in &mut cut.xsheet.params {
                for (f, k) in p.delete_frames(*frame, *count) {
                    inverse.push(Command::SetParamKey {
                        at: *at,
                        column: p.id,
                        frame: f,
                        key: Some(k),
                    });
                }
            }
            for (f, text) in crate::xsheet::delete_frames_in(&mut cut.memos, *frame, *count) {
                inverse.push(Command::SetMemo {
                    at: *at,
                    frame: f,
                    text: Some(text),
                });
            }
            cut.frame_count -= *count;
            Ok(AppliedEffect {
                inverse,
                invalidation_roots: all_nodes(cut),
            })
        }

        Command::SetCells { at, cells } => {
            let cut = cut_mut(project, *at)?;
            // Validate the whole batch first (in order, so a cell edited
            // twice checks against its first edit), then apply: a refused
            // batch leaves the sheet untouched.
            let mut seen: std::collections::HashMap<(ColumnId, u32), Option<Exposure>> =
                std::collections::HashMap::new();
            for c in cells {
                let col = cut
                    .xsheet
                    .column(c.column)
                    .ok_or(EngineError::UnknownColumn(c.column))?;
                if cut.column_def(c.column).locked {
                    return Err(locked(&col.name));
                }
                if let Some(Exposure::Drawing(d)) = c.after
                    && cut.drawing(d).is_none()
                {
                    return Err(EngineError::UnknownDrawing(d));
                }
                let now = seen
                    .get(&(c.column, c.frame))
                    .copied()
                    .unwrap_or_else(|| col.key_at(c.frame));
                if now != c.before {
                    return Err(EngineError::InvalidCommand(format!(
                        "column {}, frame {} changed since this edit was made",
                        col.name,
                        c.frame as u64 + 1
                    )));
                }
                seen.insert((c.column, c.frame), c.after);
            }
            let mut roots = Vec::new();
            for c in cells {
                let col = cut.xsheet.column_mut(c.column).expect("validated above");
                match c.after {
                    Some(e) => col.set_key(c.frame, e),
                    None => col.clear_key(c.frame),
                };
                roots.extend(cut.graph.sources_of_column(c.column));
            }
            roots.sort();
            roots.dedup();
            let inverse_cells = cells
                .iter()
                .rev()
                .map(|c| CellEdit {
                    before: c.after,
                    after: c.before,
                    ..*c
                })
                .collect();
            Ok(AppliedEffect {
                inverse: vec![Command::SetCells {
                    at: *at,
                    cells: inverse_cells,
                }],
                invalidation_roots: roots,
            })
        }

        Command::SetCutMeta {
            at,
            key,
            before,
            after,
        } => {
            let cut = cut_mut(project, *at)?;
            if key.is_empty() {
                return Err(EngineError::InvalidCommand("empty cut meta key".into()));
            }
            if cut.meta.get(key) != before.as_ref() {
                return Err(EngineError::InvalidCommand(format!(
                    "cut meta '{key}' changed since this edit was made"
                )));
            }
            match after {
                Some(v) => cut.meta.insert(key.clone(), v.clone()),
                None => cut.meta.remove(key),
            };
            Ok(AppliedEffect {
                inverse: vec![Command::SetCutMeta {
                    at: *at,
                    key: key.clone(),
                    before: after.clone(),
                    after: before.clone(),
                }],
                // A key's owner part invalidates what it renders (D3).
                invalidation_roots: all_nodes(cut),
            })
        }

        Command::SetColumnDef { at, column, def } => {
            let cut = cut_mut(project, *at)?;
            if cut.xsheet.column(*column).is_none() {
                return Err(EngineError::UnknownColumn(*column));
            }
            let prev = cut.column_def(*column);
            cut.set_column_def(*column, *def);
            Ok(AppliedEffect {
                inverse: vec![Command::SetColumnDef {
                    at: *at,
                    column: *column,
                    def: prev,
                }],
                // Hidden/Ref turn its DrawingSources transparent.
                invalidation_roots: cut.graph.sources_of_column(*column),
            })
        }

        Command::SetMemo { at, frame, text } => {
            let cut = cut_mut(project, *at)?;
            let prev = match text.as_deref().filter(|t| !t.is_empty()) {
                Some(t) => cut.memos.insert(*frame, t.to_string()),
                None => cut.memos.remove(frame),
            };
            Ok(AppliedEffect {
                inverse: vec![Command::SetMemo {
                    at: *at,
                    frame: *frame,
                    text: prev,
                }],
                // Memos never reach pixels.
                invalidation_roots: vec![],
            })
        }
    }
}

/// Expand invalidation roots to the full downstream closure for a cut.
pub fn invalidation_closure(project: &Project, at: CutRef, roots: &[NodeId]) -> HashSet<NodeId> {
    match project.cut(at.scene, at.cut) {
        Some(cut) => cut.graph.downstream_closure(roots),
        None => roots.iter().copied().collect(),
    }
}
