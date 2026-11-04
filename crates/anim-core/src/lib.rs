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

pub mod audio;
pub mod board;
pub mod camera;
pub mod color;
pub mod command;
pub mod deliver;
pub mod edit;
pub mod error;
pub mod eval;
pub mod export;
pub mod fill;
pub mod fx;
pub mod generator;
pub mod gif;
pub mod graph;
pub mod ids;
pub mod look;
pub mod manga;
pub mod model;
pub mod r3d;
pub mod raster;
pub mod sheet_ops;
pub mod store;
pub mod text;
pub mod value;
pub mod watch;
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
    label: String,
    redo: Vec<Command>,
    undo: Vec<Command>,
    /// SESSION v2 (research/PSD-session-v2.md): who authored this step.
    /// None = this machine's own hand. RUNTIME ONLY — never serialized,
    /// so saved files keep their exact schema (NEVER-DO 4).
    author: Option<String>,
    /// GM2: this step's revision serial — unique per applied state, never
    /// reused, so "undo back to the saved state" reads clean again.
    serial: u64,
}

/// GM3: one row of the history pane — a step's label, who made it, and
/// whether it is applied (undo stack) or waiting to be redone (redo stack).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub label: String,
    /// None = this machine's own hand.
    pub author: Option<String>,
    pub applied: bool,
    /// The document's revision once this step is applied — compare it with
    /// the saved revision to mark the saved point.
    pub revision: u64,
}

#[derive(Default)]
struct History {
    /// VecDeque so trimming the oldest step (when over `limit`) is O(1).
    undo_stack: std::collections::VecDeque<Applied>,
    redo_stack: Vec<Applied>,
    /// Max undo steps kept; 0 = unlimited. Bounds undo RAM (raster tile diffs).
    limit: usize,
    /// GM2: the last serial handed out (serials start at 1).
    serial: u64,
    /// GM2: the revision of the state under the bottom of the undo stack
    /// (0 = as loaded; a trimmed step's serial once the limit drops it).
    base: u64,
}

impl History {
    fn next_serial(&mut self) -> u64 {
        self.serial += 1;
        self.serial
    }

    fn trim(&mut self) {
        if self.limit > 0 {
            while self.undo_stack.len() > self.limit {
                if let Some(old) = self.undo_stack.pop_front() {
                    self.base = old.serial;
                }
            }
        }
    }

    /// Out-of-order surgery (session undo/redo of a step that is not on
    /// top) produces a state no earlier revision described: give every
    /// step above `from` a fresh serial and every redo step a fresh one too.
    /// A false "dirty" is safe; a false "clean" loses work.
    fn reserial_from(&mut self, from: usize) {
        for i in from..self.undo_stack.len() {
            let s = self.next_serial();
            self.undo_stack[i].serial = s;
        }
        for i in 0..self.redo_stack.len() {
            let s = self.next_serial();
            self.redo_stack[i].serial = s;
        }
    }
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
    // cut is the undoable workflow. GM5 (D4): columns, param columns and
    // cuts now have undoable Commands (AddColumn, RemoveCut, ...) and the
    // app goes through them; these direct forms stay for setup code and
    // tests that build a document before its history starts.

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
        // GM5: a cut keeps at least one frame (a 0-frame cut's InsertFrames
        // and SetFrameCount could not be undone); the app always showed
        // one frame for it anyway.
        scene_ref.cuts.push(Cut::new(id, name, frame_count.max(1)));
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
    /// `add_column`). Drawings themselves stay in the cut library. Setup
    /// code only: it ignores the column's lock — the app removes columns
    /// through `Command::RemoveColumn`, which honours it and is undoable.
    pub fn remove_column(&mut self, at: CutRef, column: ColumnId) {
        if let Some(cut) = self.project.cut_mut(at.scene, at.cut) {
            cut.xsheet.columns.retain(|c| c.id != column);
            // GM5: its definition goes with it (never an orphan row).
            cut.column_defs.remove(&column);
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

    /// GM5: ids for the scaffolding Commands (AddColumn, AddParamColumn,
    /// AddCut) — minted here, never reused, so a redo stays valid.
    pub fn alloc_column_id(&mut self) -> ColumnId {
        ColumnId(self.project.alloc_id())
    }

    pub fn alloc_param_id(&mut self) -> ParamId {
        ParamId(self.project.alloc_id())
    }

    pub fn alloc_cut_id(&mut self) -> CutId {
        CutId(self.project.alloc_id())
    }

    /// GM17: an id for a model embedded through SetCutModel.
    pub fn alloc_model_id(&mut self) -> ids::ModelId {
        ids::ModelId(self.project.alloc_id())
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
        let serial = self.history.next_serial();
        self.history.undo_stack.push_back(Applied {
            label: label.into(),
            redo: commands,
            undo: inverses,
            author,
            serial,
        });
        // Bound the history: drop the oldest steps beyond the limit.
        self.history.trim();
        self.history.redo_stack.clear();
        Ok(())
    }

    /// GM2: the document's revision — the serial of the state the undo
    /// stack ends on. Every apply makes a new one; undo/redo return to the
    /// exact earlier values, so `revision() == saved` means "as saved".
    pub fn revision(&self) -> u64 {
        self.history
            .undo_stack
            .back()
            .map(|a| a.serial)
            .unwrap_or(self.history.base)
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
        self.history.trim();
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
        let on_top = idx + 1 == self.history.undo_stack.len();
        let step = self
            .history
            .undo_stack
            .remove(idx)
            .expect("index came from this stack");
        self.replay(&step.undo)?;
        let label = step.label.clone();
        self.history.redo_stack.push(step);
        if !on_top {
            self.history.reserial_from(idx);
        }
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
        let on_top = idx + 1 == self.history.redo_stack.len();
        let step = self.history.redo_stack.remove(idx);
        self.replay(&step.redo)?;
        let label = step.label.clone();
        self.history.undo_stack.push_back(step);
        if !on_top {
            let top = self.history.undo_stack.len() - 1;
            self.history.reserial_from(top);
        }
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

    /// GM3: every kept step, oldest first — the undo stack (applied), then
    /// the redo stack in the order redo would re-apply it (not applied).
    pub fn history(&self) -> Vec<HistoryEntry> {
        (0..self.history_len())
            .filter_map(|i| self.history_entry(i))
            .collect()
    }

    /// GM3: how many rows `history()` has, without building them.
    pub fn history_len(&self) -> usize {
        self.history.undo_stack.len() + self.history.redo_stack.len()
    }

    /// GM3: row `i` of `history()` alone — a pane draws only the rows on
    /// screen, never the whole list each frame.
    pub fn history_entry(&self, i: usize) -> Option<HistoryEntry> {
        let h = &self.history;
        let (a, applied) = match h.undo_stack.get(i) {
            Some(a) => (a, true),
            None => {
                // The redo stack's top is the next row after the undo stack.
                let j = i - h.undo_stack.len();
                let k = h.redo_stack.len().checked_sub(j)?.checked_sub(1)?;
                (&h.redo_stack[k], false)
            }
        };
        Some(HistoryEntry {
            label: a.label.clone(),
            author: a.author.clone(),
            applied,
            revision: a.serial,
        })
    }

    /// GM3: how many rows of `history()` are applied.
    pub fn position(&self) -> usize {
        self.history.undo_stack.len()
    }

    /// GM3: the revision under the oldest kept step (position 0).
    pub fn base_revision(&self) -> u64 {
        self.history.base
    }

    /// GM3: undo or redo single steps until `position` steps are applied
    /// (clamped to the history's length). Each step is the ordinary
    /// undo/redo, so revisions and the redo stack stay exact.
    pub fn undo_to(&mut self, position: usize) -> Result<()> {
        let target = position.min(self.history_len());
        while self.history.undo_stack.len() > target {
            self.undo()?;
        }
        while self.history.undo_stack.len() < target {
            self.redo()?;
        }
        Ok(())
    }

    /// Forget all undo/redo history (e.g. after setup scripts, or to mark a
    /// "clean slate" point). The document itself is untouched.
    pub fn clear_history(&mut self) {
        // The revision stays put: clearing history changes no content.
        self.history.base = self.revision();
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

    /// GM9: one node's value at `frame`, memoised like [`Self::eval`] — the
    /// GPU compositor keys an uploaded FX subtree by its hash (D9).
    pub fn eval_node(
        &mut self,
        scene: SceneId,
        cut: CutId,
        node: NodeId,
        frame: u32,
    ) -> Result<Value> {
        let cut_ref = self
            .project
            .cut(scene, cut)
            .ok_or(EngineError::UnknownCut(cut))?;
        self.evaluator.eval_at(cut_ref, node, frame)
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
