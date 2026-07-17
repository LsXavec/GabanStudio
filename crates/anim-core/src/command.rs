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
use crate::model::{Cut, Drawing, Project, Stroke};
use crate::xsheet::Exposure;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CutRef {
    pub scene: SceneId,
    pub cut: CutId,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Command {
    AddDrawing {
        at: CutRef,
        id: DrawingId,
        name: String,
        /// Usually empty for user-created drawings; carries full artwork when
        /// this command is the inverse of a RemoveDrawing.
        strokes: Vec<Stroke>,
    },
    RemoveDrawing {
        at: CutRef,
        id: DrawingId,
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
}

impl Command {
    pub fn cut_ref(&self) -> CutRef {
        match self {
            Command::AddDrawing { at, .. }
            | Command::RemoveDrawing { at, .. }
            | Command::AddStroke { at, .. }
            | Command::PopStroke { at, .. }
            | Command::SetCell { at, .. }
            | Command::AddNode { at, .. }
            | Command::RemoveNode { at, .. }
            | Command::Connect { at, .. }
            | Command::Disconnect { at, .. }
            | Command::SetNodeKind { at, .. }
            | Command::SetOutput { at, .. } => *at,
        }
    }
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

/// Nodes to invalidate when a drawing's artwork changes: the DrawingSource
/// nodes of every column that currently exposes it.
fn stroke_invalidation_roots(cut: &Cut, id: DrawingId) -> Vec<NodeId> {
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

pub fn apply_command(project: &mut Project, cmd: &Command) -> Result<AppliedEffect> {
    match cmd {
        Command::AddDrawing {
            at,
            id,
            name,
            strokes,
        } => {
            let cut = cut_mut(project, *at)?;
            if cut.drawing(*id).is_some() {
                return Err(EngineError::InvalidCommand(format!(
                    "drawing {id} already exists"
                )));
            }
            cut.drawings.push(Drawing {
                id: *id,
                name: name.clone(),
                strokes: strokes.clone(),
            });
            Ok(AppliedEffect {
                inverse: vec![Command::RemoveDrawing { at: *at, id: *id }],
                invalidation_roots: vec![],
            })
        }

        Command::AddStroke { at, id, stroke } => {
            let cut = cut_mut(project, *at)?;
            let roots = stroke_invalidation_roots(cut, *id);
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
            let roots = stroke_invalidation_roots(cut, *id);
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
                name: drawing.name.clone(),
                strokes: drawing.strokes.clone(),
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
    }
}

/// Expand invalidation roots to the full downstream closure for a cut.
pub fn invalidation_closure(project: &Project, at: CutRef, roots: &[NodeId]) -> HashSet<NodeId> {
    match project.cut(at.scene, at.cut) {
        Some(cut) => cut.graph.downstream_closure(roots),
        None => roots.iter().copied().collect(),
    }
}
