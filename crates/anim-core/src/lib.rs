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
pub mod edit;
pub mod error;
pub mod eval;
pub mod export;
pub mod fill;
pub mod graph;
pub mod ids;
pub mod model;
pub mod raster;
pub mod store;
pub mod value;
pub mod xdts;
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
    /// SESSION v2 (research/PSD-session-v2.md): who authored this step.
    /// None = this machine's own hand. RUNTIME ONLY — never serialized,
    /// so saved files keep their exact schema (NEVER-DO 4).
    author: Option<String>,
}

#[derive(Default)]
struct History {
    /// VecDeque so trimming the oldest step (when over `limit`) is O(1).
    undo_stack: std::collections::VecDeque<Applied>,
    redo_stack: Vec<Applied>,
    /// Max undo steps kept; 0 = unlimited. Bounds undo RAM (raster tile diffs).
    limit: usize,
}

/// SESSION v2: the (drawing, layer) a command writes to, when it has
/// one. Steps with no such target are treated as touching everything
/// (they can never be jumped over) — the conservative side.
fn target_of(cmd: &Command) -> Option<(DrawingId, LayerId)> {
    match cmd {
        Command::PaintTiles { id, layer, .. } => Some((*id, *layer)),
        _ => None,
    }
}

/// The engine facade the app shell talks to.
pub struct Engine {
    pub project: Project,
    /// SESSION v2: the author the next applied step belongs to.
    current_author: Option<String>,
    /// SESSION MIRROR (PSD-session-v2 ruling 2026-08-19): when on, every
    /// successfully applied command batch is also logged for the host to
    /// stream to guests. RUNTIME ONLY — never serialized.
    pub mirror_log: bool,
    applied_log: Vec<Vec<Command>>,
    pub evaluator: Evaluator,
    history: History,
}

impl Engine {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            project: Project::new(name),
            evaluator: Evaluator::new(),
            history: History::default(),
            current_author: None,
            mirror_log: false,
            applied_log: Vec::new(),
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
            images: Vec::new(),
            audio: None,
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

    /// Rename a scene. Scaffolding op (not undoable — names are setup data,
    /// same tier as add_scene; they never enter eval recipes).
    pub fn rename_scene(&mut self, scene: SceneId, name: impl Into<String>) -> Result<()> {
        let s = self
            .project
            .scene_mut(scene)
            .ok_or(EngineError::UnknownScene(scene))?;
        s.name = name.into();
        Ok(())
    }

    /// Rename a cut. Scaffolding op (not undoable — matches rename_scene).
    pub fn rename_cut(&mut self, at: CutRef, name: impl Into<String>) -> Result<()> {
        let cut = self
            .project
            .cut_mut(at.scene, at.cut)
            .ok_or(EngineError::UnknownCut(at.cut))?;
        cut.name = name.into();
        Ok(())
    }

    /// Add a PARAMETER column (camera etc.). Scaffolding op (not undoable —
    /// matches `add_column`); keys on it are undoable via SetParamKey.
    pub fn add_param_column(&mut self, at: CutRef, name: impl Into<String>) -> Result<ParamId> {
        let id = ParamId(self.project.alloc_id());
        let name = name.into();
        let cut = self
            .project
            .cut_mut(at.scene, at.cut)
            .ok_or(EngineError::UnknownCut(at.cut))?;
        cut.xsheet.params.push(crate::xsheet::ParamColumn::new(id, name));
        Ok(id)
    }

    /// Remove a column and its exposures. Scaffolding op (not undoable — matches
    /// `add_column`). Drawings themselves stay in the cut library.
    pub fn remove_column(&mut self, at: CutRef, column: ColumnId) {
        if let Some(cut) = self.project.cut_mut(at.scene, at.cut) {
            cut.xsheet.columns.retain(|c| c.id != column);
            // A DrawingSource for this column now resolves nothing; clear caches.
            self.evaluator.invalidate_all();
        }
    }

    /// Allocate an id for a new drawing/node to be created via a Command.
    pub fn alloc_drawing_id(&mut self) -> DrawingId {
        DrawingId(self.project.alloc_id())
    }

    pub fn alloc_node_id(&mut self) -> NodeId {
        NodeId(self.project.alloc_id())
    }

    pub fn alloc_layer_id(&mut self) -> LayerId {
        LayerId(self.project.alloc_id())
    }

    pub fn alloc_image_id(&mut self) -> ImageId {
        ImageId(self.project.alloc_id())
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
        if self.mirror_log {
            self.applied_log.push(commands.clone());
        }
        let author = self.current_author.clone();
        self.history.undo_stack.push_back(Applied {
            label: label.into(),
            redo: commands,
            undo: inverses,
            author,
        });
        // Bound the history: drop the oldest steps beyond the limit.
        if self.history.limit > 0 {
            while self.history.undo_stack.len() > self.history.limit {
                self.history.undo_stack.pop_front();
            }
        }
        self.history.redo_stack.clear();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<()> {
        let step = self
            .history
            .undo_stack
            .pop_back()
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
        self.history.undo_stack.push_back(step);
        Ok(())
    }

    /// Cap the number of undo steps kept (0 = unlimited). Trims immediately.
    pub fn set_undo_limit(&mut self, limit: usize) {
        self.history.limit = limit;
        if limit > 0 {
            while self.history.undo_stack.len() > limit {
                self.history.undo_stack.pop_front();
            }
        }
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

    /// SESSION v2: label the NEXT applied step with this author (None =
    /// this machine's own hand). The host sets it around a guest's edit
    /// so history knows whose hand each step came from.
    pub fn set_author(&mut self, author: Option<String>) {
        self.current_author = author;
    }

    /// STAGE 3 (PSD-multiplayer-rescope): the author currently armed —
    /// transient taggers save and RESTORE this instead of resetting to
    /// None (a session arms the local artist's name as the base, so
    /// every history entry carries its hand on every machine).
    pub fn author(&self) -> Option<String> {
        self.current_author.clone()
    }

    /// Undo the most recent step authored by `author`, but ONLY while it
    /// is still safe to remove: nothing applied after it touches the same
    /// drawing+layer (NEVER-DO 3 — no out-of-order surgery). Returns the
    /// step's label on success.
    pub fn undo_last_by(&mut self, author: Option<&str>) -> Result<String> {
        let idx = self
            .history
            .undo_stack
            .iter()
            .rposition(|a| a.author.as_deref() == author)
            .ok_or(EngineError::NothingToUndo)?;
        // Safety: every step ABOVE it must be disjoint in target.
        let mine: Vec<(DrawingId, LayerId)> =
            self.history.undo_stack[idx].redo.iter().filter_map(target_of).collect();
        for later in self.history.undo_stack.iter().skip(idx + 1) {
            for t in later.redo.iter().filter_map(target_of) {
                if mine.contains(&t) {
                    return Err(EngineError::InvalidCommand(
                        "a later edit touches the same layer — undo refused".into(),
                    ));
                }
            }
        }
        let step = self
            .history
            .undo_stack
            .remove(idx)
            .expect("index came from this stack");
        self.replay(&step.undo)?;
        let label = step.label.clone();
        self.history.redo_stack.push(step);
        Ok(label)
    }

    /// Redo the most recent redoable step authored by `author`.
    pub fn redo_last_by(&mut self, author: Option<&str>) -> Result<String> {
        let idx = self
            .history
            .redo_stack
            .iter()
            .rposition(|a| a.author.as_deref() == author)
            .ok_or(EngineError::NothingToRedo)?;
        let step = self.history.redo_stack.remove(idx);
        self.replay(&step.redo)?;
        let label = step.label.clone();
        self.history.undo_stack.push_back(step);
        Ok(label)
    }

    /// How many steps this author can still undo (the Foot's lamps).
    pub fn undo_depth_by(&self, author: Option<&str>) -> usize {
        self.history
            .undo_stack
            .iter()
            .filter(|a| a.author.as_deref() == author)
            .count()
    }

    /// SESSION MIRROR: everything applied since the last drain (the
    /// host streams these to guests each frame).
    pub fn drain_applied(&mut self) -> Vec<Vec<Command>> {
        std::mem::take(&mut self.applied_log)
    }

    /// SESSION MIRROR: apply a streamed batch to a guest's mirror —
    /// NO history (one truth: the mirror never undoes on its own).
    /// Best-effort: a batch that no longer fits the mirror is skipped
    /// whole (the next join snapshot reconciles).
    pub fn apply_mirror(&mut self, commands: &[Command]) -> Result<()> {
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
            current_author: None,
            mirror_log: false,
            applied_log: Vec::new(),
        })
    }
}
