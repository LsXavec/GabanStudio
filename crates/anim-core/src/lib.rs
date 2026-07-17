//! anim-core — the headless engine of the node-graph 2D animation suite.
//!
//! Architecture law: the document IS a node graph; UI workspaces are views.
//! This crate owns everything the UI shell must never own:
//!
//! * document model: Project -> Scene -> Cut (drawings + X-sheet + graph)
//! * X-sheet with hold semantics — timing separate from artwork
//! * pull-based memoized evaluation with downstream-only invalidation
//! * command-based editing with exact inverses (undo/redo)
//! * transactional SQLite persistence
//!
//! No I/O besides `save`/`load`, no rendering, no UI types. Fully testable.

pub mod command;
pub mod error;
pub mod eval;
pub mod graph;
pub mod ids;
pub mod model;
pub mod store;
pub mod value;
pub mod xsheet;

use std::collections::HashSet;
use std::path::Path;

use command::{AppliedEffect, Command, CutRef, apply_command, invalidation_closure};
use error::{EngineError, Result};
use eval::Evaluator;
use ids::*;
use model::{Cut, Project, Scene};
use value::Value;
use xsheet::Column;

/// One undoable step: a labeled batch of commands and its exact inverse.
struct Applied {
    #[allow(dead_code)] // surfaced in the UI's edit menu later
    label: String,
    redo: Vec<Command>,
    undo: Vec<Command>,
}

#[derive(Default)]
struct History {
    undo_stack: Vec<Applied>,
    redo_stack: Vec<Applied>,
}

/// The engine facade the app shell talks to.
pub struct Engine {
    pub project: Project,
    pub evaluator: Evaluator,
    history: History,
}

impl Engine {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            project: Project::new(name),
            evaluator: Evaluator::new(),
            history: History::default(),
        }
    }

    // ---- Structure setup (non-undoable scaffolding ops) -------------------
    // Creating scenes/cuts/columns is project scaffolding; editing inside a
    // cut is the undoable workflow. This split matches how artists work and
    // keeps M1's command set focused. Revisit if scaffolding needs undo.

    pub fn add_scene(&mut self, name: impl Into<String>) -> SceneId {
        let id = SceneId(self.project.alloc_id());
        self.project.scenes.push(Scene {
            id,
            name: name.into(),
            cuts: Vec::new(),
        });
        id
    }

    pub fn add_cut(
        &mut self,
        scene: SceneId,
        name: impl Into<String>,
        frame_count: u32,
    ) -> Result<CutId> {
        let id = CutId(self.project.alloc_id());
        let name = name.into();
        let scene_ref = self
            .project
            .scene_mut(scene)
            .ok_or(EngineError::UnknownScene(scene))?;
        scene_ref.cuts.push(Cut {
            id,
            name,
            frame_count,
            drawings: Vec::new(),
            xsheet: Default::default(),
            graph: Default::default(),
        });
        Ok(id)
    }

    pub fn add_column(&mut self, at: CutRef, name: impl Into<String>) -> Result<ColumnId> {
        let id = ColumnId(self.project.alloc_id());
        let name = name.into();
        let cut = self
            .project
            .cut_mut(at.scene, at.cut)
            .ok_or(EngineError::UnknownCut(at.cut))?;
        cut.xsheet.columns.push(Column::new(id, name));
        Ok(id)
    }

    /// Allocate an id for a new drawing/node to be created via a Command.
    pub fn alloc_drawing_id(&mut self) -> DrawingId {
        DrawingId(self.project.alloc_id())
    }

    pub fn alloc_node_id(&mut self) -> NodeId {
        NodeId(self.project.alloc_id())
    }

    // ---- Editing (undoable) ----------------------------------------------

    /// Apply a labeled batch of commands as ONE undo step.
    /// On mid-batch failure the already-applied commands are rolled back —
    /// a batch either fully applies or leaves the document untouched.
    pub fn apply(&mut self, label: impl Into<String>, commands: Vec<Command>) -> Result<()> {
        let mut inverses: Vec<Command> = Vec::new();
        let mut invalidate: HashSet<NodeId> = HashSet::new();

        for (i, cmd) in commands.iter().enumerate() {
            match apply_command(&mut self.project, cmd) {
                Ok(AppliedEffect {
                    inverse,
                    invalidation_roots,
                }) => {
                    invalidate.extend(invalidation_closure(
                        &self.project,
                        cmd.cut_ref(),
                        &invalidation_roots,
                    ));
                    // Prepend: undo must run in reverse application order.
                    inverses.splice(0..0, inverse);
                }
                Err(e) => {
                    // Roll back what already applied (inverses are in
                    // reverse order already).
                    for inv in &inverses {
                        let _ = apply_command(&mut self.project, inv);
                    }
                    return Err(EngineError::InvalidCommand(format!(
                        "batch failed at command {i}: {e}"
                    )));
                }
            }
        }

        self.evaluator.invalidate_nodes(&invalidate);
        self.history.undo_stack.push(Applied {
            label: label.into(),
            redo: commands,
            undo: inverses,
        });
        self.history.redo_stack.clear();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<()> {
        let step = self
            .history
            .undo_stack
            .pop()
            .ok_or(EngineError::NothingToUndo)?;
        self.replay(&step.undo)?;
        self.history.redo_stack.push(step);
        Ok(())
    }

    pub fn redo(&mut self) -> Result<()> {
        let step = self
            .history
            .redo_stack
            .pop()
            .ok_or(EngineError::NothingToRedo)?;
        self.replay(&step.redo)?;
        self.history.undo_stack.push(step);
        Ok(())
    }

    fn replay(&mut self, commands: &[Command]) -> Result<()> {
        let mut invalidate: HashSet<NodeId> = HashSet::new();
        for cmd in commands {
            let effect = apply_command(&mut self.project, cmd)?;
            invalidate.extend(invalidation_closure(
                &self.project,
                cmd.cut_ref(),
                &effect.invalidation_roots,
            ));
        }
        self.evaluator.invalidate_nodes(&invalidate);
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.history.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.history.redo_stack.is_empty()
    }

    /// Forget all undo/redo history (e.g. after setup scripts, or to mark a
    /// "clean slate" point). The document itself is untouched.
    pub fn clear_history(&mut self) {
        self.history.undo_stack.clear();
        self.history.redo_stack.clear();
    }

    // ---- Evaluation -------------------------------------------------------

    pub fn eval(&mut self, scene: SceneId, cut: CutId, frame: u32) -> Result<Value> {
        let cut_ref = self
            .project
            .cut(scene, cut)
            .ok_or(EngineError::UnknownCut(cut))?;
        self.evaluator.eval(cut_ref, frame)
    }

    // ---- Persistence ------------------------------------------------------

    pub fn save(&self, path: &Path) -> Result<()> {
        store::save(&self.project, path)
    }

    /// Load a project from disk. History and caches start fresh.
    pub fn load(path: &Path) -> Result<Self> {
        Ok(Self {
            project: store::load(path)?,
            evaluator: Evaluator::new(),
            history: History::default(),
        })
    }
}
