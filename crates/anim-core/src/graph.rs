//! The per-cut node graph. The document IS a node graph; workspaces are views.
//!
//! M1 carries a deliberately small node vocabulary — just enough to prove
//! evaluation, caching, cycle safety, undo, and persistence. M2+ replaces the
//! symbolic values with real raster/vector data without changing this shape.

use std::collections::{BTreeMap, HashSet, VecDeque};

use crate::error::{EngineError, Result};
use crate::ids::{ColumnId, ImageId, NodeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BlendMode {
    Normal,
    Multiply,
    Add,
    Screen,
}

impl std::fmt::Display for BlendMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Add => "Add",
            Self::Screen => "Screen",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum NodeKind {
    /// Reads the cut's X-sheet: resolves `column` at the evaluated frame.
    /// This node is the bridge between timing and imagery.
    DrawingSource { column: ColumnId },
    /// Constant color layer.
    Solid { rgba: [u8; 4] },
    /// One of the cut's imported image assets (BG plate, reference).
    ImageSource { image: ImageId },
    /// Geometric transform of its single input.
    Transform {
        translate: (f32, f32),
        scale: f32,
        rotate_deg: f32,
    },
    /// Composites input 1 over input 0.
    Blend { mode: BlendMode },
    /// The cut's final image. One input.
    Output,
}

impl NodeKind {
    pub fn input_count(&self) -> usize {
        match self {
            Self::DrawingSource { .. } | Self::Solid { .. } | Self::ImageSource { .. } => 0,
            Self::Transform { .. } | Self::Output => 1,
            Self::Blend { .. } => 2,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::DrawingSource { .. } => "DrawingSource",
            Self::Solid { .. } => "Solid",
            Self::ImageSource { .. } => "ImageSource",
            Self::Transform { .. } => "Transform",
            Self::Blend { .. } => "Blend",
            Self::Output => "Output",
        }
    }
}

/// A connection source: the output pin of another node.
/// `pin` is always 0 in M1 (single-output nodes) but the wire format keeps it.
pub type OutPin = (NodeId, u16);

/// An incoming connection that was severed by a node removal:
/// (consumer node, consumer's input pin, source output pin).
pub type SeveredLink = (NodeId, u16, u16);

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    /// One slot per input pin; `None` = unconnected (evaluates as empty).
    pub inputs: Vec<Option<OutPin>>,
}

impl Node {
    pub fn new(id: NodeId, kind: NodeKind) -> Self {
        let inputs = vec![None; kind.input_count()];
        Self { id, kind, inputs }
    }
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Graph {
    /// BTreeMap for deterministic iteration (stable saves, stable tests).
    nodes: BTreeMap<NodeId, Node>,
    pub output: Option<NodeId>,
}

impl Graph {
    pub fn node(&self, id: NodeId) -> Result<&Node> {
        self.nodes.get(&id).ok_or(EngineError::UnknownNode(id))
    }

    pub fn node_mut(&mut self, id: NodeId) -> Result<&mut Node> {
        self.nodes.get_mut(&id).ok_or(EngineError::UnknownNode(id))
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.id, node);
    }

    /// Removes a node. Also clears any other node's input that referenced it.
    /// Returns (the removed node, the cleared incoming references as
    /// (to_node, to_pin, from_pin)) so the caller can build an exact inverse.
    pub fn remove_node(&mut self, id: NodeId) -> Result<(Node, Vec<SeveredLink>)> {
        let node = self.nodes.remove(&id).ok_or(EngineError::UnknownNode(id))?;
        let mut severed = Vec::new();
        for other in self.nodes.values_mut() {
            for (pin, input) in other.inputs.iter_mut().enumerate() {
                if let Some((src, src_pin)) = input
                    && *src == id {
                        severed.push((other.id, pin as u16, *src_pin));
                        *input = None;
                    }
            }
        }
        if self.output == Some(id) {
            self.output = None;
        }
        Ok((node, severed))
    }

    /// Connect `from`'s output pin into `to`'s input pin.
    /// Returns the input's previous connection (for inverse construction).
    pub fn connect(
        &mut self,
        from: NodeId,
        from_pin: u16,
        to: NodeId,
        to_pin: u16,
    ) -> Result<Option<OutPin>> {
        if !self.contains(from) {
            return Err(EngineError::UnknownNode(from));
        }
        let to_node = self.node(to)?;
        if to_pin as usize >= to_node.inputs.len() {
            return Err(EngineError::PinOutOfRange { node: to, pin: to_pin });
        }
        if self.would_cycle(from, to) {
            return Err(EngineError::WouldCycle { from, to });
        }
        let slot = &mut self.node_mut(to)?.inputs[to_pin as usize];
        Ok(slot.replace((from, from_pin)))
    }

    /// Clear `to`'s input pin. Returns what was connected there.
    pub fn disconnect(&mut self, to: NodeId, to_pin: u16) -> Result<Option<OutPin>> {
        let to_node = self.node_mut(to)?;
        if to_pin as usize >= to_node.inputs.len() {
            return Err(EngineError::PinOutOfRange { node: to, pin: to_pin });
        }
        Ok(to_node.inputs[to_pin as usize].take())
    }

    /// Would wiring `from` -> `to` create a cycle? True if `to` is already
    /// upstream of `from` (or they are the same node).
    pub fn would_cycle(&self, from: NodeId, to: NodeId) -> bool {
        if from == to {
            return true;
        }
        // Walk upstream from `from`; if we reach `to`, connecting would cycle.
        let mut stack = vec![from];
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if id == to {
                return true;
            }
            if let Some(node) = self.nodes.get(&id) {
                for input in node.inputs.iter().flatten() {
                    stack.push(input.0);
                }
            }
        }
        false
    }

    /// The set of nodes downstream of (fed by) any node in `roots`,
    /// including the roots themselves. This is the invalidation footprint
    /// of an edit: everything here recomputes, nothing else does.
    pub fn downstream_closure(&self, roots: &[NodeId]) -> HashSet<NodeId> {
        // Build reverse edges (consumer lists) by scanning inputs.
        let mut consumers: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for node in self.nodes.values() {
            for input in node.inputs.iter().flatten() {
                consumers.entry(input.0).or_default().push(node.id);
            }
        }
        let mut result: HashSet<NodeId> = HashSet::new();
        let mut queue: VecDeque<NodeId> = roots.iter().copied().collect();
        while let Some(id) = queue.pop_front() {
            if !result.insert(id) {
                continue;
            }
            if let Some(list) = consumers.get(&id) {
                queue.extend(list.iter().copied());
            }
        }
        result
    }

    /// All DrawingSource nodes reading the given column.
    pub fn sources_of_column(&self, column: ColumnId) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|n| matches!(n.kind, NodeKind::DrawingSource { column: c } if c == column))
            .map(|n| n.id)
            .collect()
    }

    /// All ImageSource nodes reading the given image asset (invalidation
    /// roots when the asset is added/removed).
    pub fn sources_of_image(&self, image: ImageId) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|n| matches!(n.kind, NodeKind::ImageSource { image: i } if i == image))
            .map(|n| n.id)
            .collect()
    }
}
