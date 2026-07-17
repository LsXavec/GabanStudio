//! Pull-based, memoized graph evaluation — the Nuke/Fusion pattern that keeps
//! interactive fps honest with heavy content: each (node, frame) computes once;
//! edits invalidate only the edited node and its downstream closure.

use std::collections::{HashMap, HashSet};

use crate::error::{EngineError, Result};
use crate::graph::NodeKind;
use crate::ids::NodeId;
use crate::model::Cut;
use crate::value::Value;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EvalStats {
    /// Node evaluations actually computed.
    pub computed: u64,
    /// Node evaluations served from cache.
    pub cache_hits: u64,
}

#[derive(Default)]
pub struct Evaluator {
    cache: HashMap<(NodeId, u32), Value>,
    pub stats: EvalStats,
}

impl Evaluator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluate the cut's output node at `frame`.
    pub fn eval(&mut self, cut: &Cut, frame: u32) -> Result<Value> {
        let output = cut.graph.output.ok_or(EngineError::NoOutput)?;
        let mut visiting = HashSet::new();
        self.eval_node(cut, output, frame, &mut visiting)
    }

    fn eval_node(
        &mut self,
        cut: &Cut,
        id: NodeId,
        frame: u32,
        visiting: &mut HashSet<NodeId>,
    ) -> Result<Value> {
        if let Some(v) = self.cache.get(&(id, frame)) {
            self.stats.cache_hits += 1;
            return Ok(v.clone());
        }
        if !visiting.insert(id) {
            return Err(EngineError::EvalCycle(id));
        }

        // Clone the node's shape up front so recursion doesn't fight the borrow.
        let (kind, inputs) = {
            let node = cut.graph.node(id)?;
            (node.kind.clone(), node.inputs.clone())
        };

        let input_value = |slot: usize,
                               this: &mut Self,
                               visiting: &mut HashSet<NodeId>|
         -> Result<Value> {
            match inputs.get(slot).copied().flatten() {
                Some((src, _pin)) => this.eval_node(cut, src, frame, visiting),
                None => Ok(Value::image("empty".to_string())),
            }
        };

        let value = match &kind {
            NodeKind::DrawingSource { column } => {
                let col = cut
                    .xsheet
                    .column(*column)
                    .ok_or(EngineError::UnknownColumn(*column))?;
                match col.resolve(frame) {
                    Some(d) => match cut.drawing(d) {
                        // Content hash in the recipe: editing artwork changes
                        // the value (and its hash) without any extra plumbing.
                        Some(dr) => Value::image(format!(
                            "drawing({}:'{}'#{:016x})",
                            d,
                            dr.name,
                            dr.content_hash()
                        )),
                        None => Value::image(format!("drawing({d}:?)")),
                    },
                    None => Value::image("empty".to_string()),
                }
            }
            NodeKind::Solid { rgba } => Value::image(format!(
                "solid(#{:02x}{:02x}{:02x}{:02x})",
                rgba[0], rgba[1], rgba[2], rgba[3]
            )),
            NodeKind::Transform {
                translate,
                scale,
                rotate_deg,
            } => {
                let child = input_value(0, self, visiting)?;
                Value::image(format!(
                    "xform(t=({:.3},{:.3}),s={:.3},r={:.3}, {})",
                    translate.0,
                    translate.1,
                    scale,
                    rotate_deg,
                    child.recipe()
                ))
            }
            NodeKind::Blend { mode } => {
                let a = input_value(0, self, visiting)?;
                let b = input_value(1, self, visiting)?;
                Value::image(format!("blend({}, {}, {})", mode, a.recipe(), b.recipe()))
            }
            NodeKind::Output => {
                let child = input_value(0, self, visiting)?;
                Value::image(format!("out({})", child.recipe()))
            }
        };

        visiting.remove(&id);
        self.stats.computed += 1;
        self.cache.insert((id, frame), value.clone());
        Ok(value)
    }

    /// Drop cached values for the given nodes (all frames).
    /// M2 refines this to frame ranges for X-sheet edits.
    pub fn invalidate_nodes(&mut self, nodes: &HashSet<NodeId>) {
        self.cache.retain(|(id, _), _| !nodes.contains(id));
    }

    pub fn invalidate_all(&mut self) {
        self.cache.clear();
    }

    pub fn cached_entries(&self) -> usize {
        self.cache.len()
    }
}
