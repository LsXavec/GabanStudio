//! The per-cut node graph. The document IS a node graph; workspaces are views.
//!
//! M1 carries a deliberately small node vocabulary — just enough to prove
//! evaluation, caching, cycle safety, undo, and persistence. M2+ replaces the
//! symbolic values with real raster/vector data without changing this shape.

use std::collections::{BTreeMap, HashSet, VecDeque};

use crate::command::{Command, CutRef};
use crate::error::{EngineError, Result};
use crate::fx::{FxBinds, FxKind, FxParams};
use crate::generator::{GenKind, Preset};
use crate::ids::{ColumnId, ImageId, ModelId, NodeId, ParamId};
use crate::model::{Cut, LayerRole};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BlendMode {
    Normal,
    Multiply,
    Add,
    Screen,
    /// Top subtracts from bottom, clamped at zero (glow/mask removal).
    Subtract,
    /// Per-channel darker of the two (separable min, alpha-correct).
    Darken,
    /// Per-channel lighter of the two (separable max, alpha-correct).
    Lighten,
    /// Contrast texture blend (2·s·b below mid, screen-like above).
    Overlay,
}

impl BlendMode {
    /// Every mode, for the inspector's picker.
    pub const ALL: &'static [BlendMode] = &[
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Add,
        BlendMode::Screen,
        BlendMode::Subtract,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Overlay,
    ];
}

impl std::fmt::Display for BlendMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Add => "Add",
            Self::Screen => "Screen",
            Self::Subtract => "Subtract",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::Overlay => "Overlay",
        };
        f.write_str(s)
    }
}

/// Which X-sheet parameter column (if any) drives each Transform param.
/// `None` = the node's static value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct TransformBinds {
    pub tx: Option<ParamId>,
    pub ty: Option<ParamId>,
    pub scale: Option<ParamId>,
    pub rotate: Option<ParamId>,
}

impl TransformBinds {
    pub fn references(&self, param: ParamId) -> bool {
        [self.tx, self.ty, self.scale, self.rotate].contains(&Some(param))
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum NodeKind {
    /// Reads the cut's X-sheet: resolves `column` at the evaluated frame.
    /// This node is the bridge between timing and imagery.
    /// GM13: `roles` passes only those cel layers (the Shadow pass, the
    /// Highlight pass, paint only, lines only); `absorb_trace` recolours the
    /// trace lines into the paint first ([`crate::look`]). Both default off,
    /// are left out of the JSON when off (old files and peers read the same
    /// node), and a filtered source renders on the CPU in every view (D9).
    DrawingSource {
        column: ColumnId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        roles: Option<Vec<LayerRole>>,
        #[serde(default, skip_serializing_if = "is_false")]
        absorb_trace: bool,
    },
    /// Constant color layer.
    Solid { rgba: [u8; 4] },
    /// One of the cut's imported image assets (BG plate, reference).
    ImageSource { image: ImageId },
    /// Geometric transform of its single input. Each param uses its bound
    /// X-sheet parameter column when set (and resolvable at the frame),
    /// falling back to the static value here — the C1 camera model.
    Transform {
        translate: (f32, f32),
        scale: f32,
        rotate_deg: f32,
        /// Added post-v6; older stored nodes deserialize as unbound.
        #[serde(default)]
        binds: TransformBinds,
    },
    /// Composites input 1 over input 0.
    Blend { mode: BlendMode },
    /// The cut's final image. One input.
    Output,
    /// GM9: a compositing effect (blur, glow, diffusion, grade), rendered by
    /// [`crate::fx`] on the CPU in every view (D9). Each parameter takes its
    /// bound X-sheet param column's value when bound and resolvable, else
    /// its stored value, else its spec default.
    Effect {
        fx: FxKind,
        #[serde(default)]
        params: FxParams,
        #[serde(default)]
        binds: FxBinds,
    },
    /// GM10: the camera over the cut's paper — one input pin per plane, each
    /// at its depth; its output is the frame as the camera sees it, rendered
    /// by [`crate::camera`]'s law on the CPU in every view (D9). Parameters
    /// ([`crate::camera::CAMERA_SPECS`]) resolve and bind as an Effect's do.
    Camera {
        planes: Vec<PlaneSpec>,
        #[serde(default)]
        params: FxParams,
        #[serde(default)]
        binds: FxBinds,
    },
    /// GM16: a picture made from parameters — particles, speed or focus
    /// lines, lettering, a balloon — drawn by [`crate::generator`] on the
    /// CPU in every view (D9). Parameters resolve and bind as an Effect's do.
    Generator {
        generator: GenKind,
        #[serde(default)]
        params: FxParams,
        #[serde(default)]
        binds: FxBinds,
    },
    /// GM17: one of the cut's embedded 3D models, toon-rendered by
    /// [`crate::r3d`] through its own camera on the CPU in every view (D9).
    /// Parameters ([`crate::r3d::MODEL_SPECS`]) resolve and bind as an
    /// Effect's do, so a camera move is keyed on the sheet. A model no
    /// longer in the cut renders transparent.
    Model3D {
        model: ModelId,
        #[serde(default)]
        params: FxParams,
        #[serde(default)]
        binds: FxBinds,
    },
    /// GM17 (D15): frames rendered by another app, referenced on disk and
    /// never embedded. `pattern`'s run of `#` is the file number, padded to
    /// the run's length (`renders/shot_####.png`); a relative pattern is
    /// read from the project's folder. Cut frame `offset` shows file
    /// `first`; a missing file renders transparent (and is reported), or
    /// with `hold_last` the nearest earlier file holds. With `fit` a file
    /// whose size differs from the frame is scaled to fit it, centred;
    /// without, it sits at its own size from the frame's top-left (a node
    /// saved before `fit` existed reads as without).
    ImageSequence {
        pattern: String,
        #[serde(default)]
        first: u32,
        #[serde(default)]
        offset: i32,
        #[serde(default)]
        hold_last: bool,
        #[serde(default)]
        fit: bool,
    },
}

/// GM10: one plane of a Camera node. `depth` is its z: 0 is the field,
/// z > 0 farther (a pan moves it less), z < 0 nearer (more).
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PlaneSpec {
    #[serde(default)]
    pub depth: f32,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl NodeKind {
    /// A plain DrawingSource: every visible layer of the column's drawing.
    pub fn source(column: ColumnId) -> Self {
        Self::DrawingSource {
            column,
            roles: None,
            absorb_trace: false,
        }
    }

    /// GM13: a role pass of `column` — only `roles`' layers, with the trace
    /// lines absorbed into the paint first when `absorb_trace`.
    pub fn pass(column: ColumnId, roles: &[LayerRole], absorb_trace: bool) -> Self {
        Self::DrawingSource {
            column,
            roles: Some(roles.to_vec()),
            absorb_trace,
        }
    }

    /// GM13: a DrawingSource that filters roles or absorbs trace lines (a
    /// CPU node in every view).
    pub fn is_filtered_source(&self) -> bool {
        matches!(self, Self::DrawingSource { roles, absorb_trace, .. } if roles.is_some() || *absorb_trace)
    }

    /// GM10: a new Camera node with one plane at depth 0 and every
    /// parameter at its default.
    pub fn camera() -> Self {
        Self::Camera {
            planes: vec![PlaneSpec::default()],
            params: crate::camera::default_params(),
            binds: FxBinds::default(),
        }
    }

    /// GM9: a new Effect node of `fx` with every parameter at its default.
    pub fn effect(fx: FxKind) -> Self {
        Self::Effect {
            fx,
            params: FxParams::defaults(fx),
            binds: FxBinds::default(),
        }
    }

    /// GM16: a new Generator node of `generator` with every parameter at
    /// its default.
    pub fn generator(generator: GenKind) -> Self {
        Self::Generator {
            generator,
            params: generator.defaults(),
            binds: FxBinds::default(),
        }
    }

    /// GM16: a Particles generator set to a preset's look.
    pub fn particles(preset: Preset) -> Self {
        Self::Generator {
            generator: GenKind::Particles,
            params: preset.params(),
            binds: FxBinds::default(),
        }
    }

    /// GM17: a new Model3D node of `model` with every parameter at its
    /// default.
    pub fn model3d(model: ModelId) -> Self {
        Self::Model3D {
            model,
            params: crate::r3d::default_params(),
            binds: FxBinds::default(),
        }
    }

    /// GM17: a new ImageSequence node reading `pattern` from file 1 (where
    /// renderers start counting) at frame 0, fitted to the frame.
    pub fn image_sequence(pattern: impl Into<String>) -> Self {
        Self::ImageSequence {
            pattern: pattern.into(),
            first: 1,
            offset: 0,
            hold_last: false,
            fit: true,
        }
    }

    pub fn input_count(&self) -> usize {
        match self {
            Self::DrawingSource { .. }
            | Self::Solid { .. }
            | Self::ImageSource { .. }
            | Self::Model3D { .. }
            | Self::ImageSequence { .. } => 0,
            Self::Transform { .. } | Self::Output => 1,
            Self::Blend { .. } => 2,
            Self::Effect { fx, .. } => fx.inputs(),
            Self::Camera { planes, .. } => planes.len(),
            Self::Generator { generator, .. } => generator.inputs(),
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
            Self::Effect { fx, .. } => fx.name(),
            Self::Camera { .. } => "Camera",
            Self::Generator { generator, .. } => generator.name(),
            Self::Model3D { .. } => "Model3D",
            Self::ImageSequence { .. } => "ImageSequence",
        }
    }

    /// Does any parameter of this node read the given param column? (The
    /// invalidation roots of a param key edit.)
    pub fn references(&self, param: ParamId) -> bool {
        match self {
            Self::Transform { binds, .. } => binds.references(param),
            Self::Effect { binds, .. }
            | Self::Camera { binds, .. }
            | Self::Generator { binds, .. }
            | Self::Model3D { binds, .. } => binds.references(param),
            _ => false,
        }
    }
}

/// GM9 (D3): the node graph's layout lives in cut meta, one key per node —
/// `graph.layout.<node id>` = JSON `[x, y]` in graph-space points — so two
/// artists placing different nodes never edit the same key (one whole-layout
/// value made every add, move and removal a precondition on every other
/// node's place). Positions are not rendered, so setting one invalidates
/// nothing.
pub const LAYOUT_KEY: &str = "graph.layout";

/// The layout key of one node.
pub fn layout_key(node: NodeId) -> String {
    format!("{LAYOUT_KEY}.{}", node.0)
}

/// Is `key` a layout key (an edit that renders nothing)?
pub fn is_layout_key(key: &str) -> bool {
    key.strip_prefix(LAYOUT_KEY)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// A node's place from its layout value (an unreadable one is no place: the
/// pane gives the node a default spot rather than refusing the cut).
pub fn pos_from_json(json: &str) -> Option<(f32, f32)> {
    serde_json::from_str::<(f32, f32)>(json)
        .ok()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
}

/// A node's layout value.
pub fn pos_to_json(pos: (f32, f32)) -> String {
    serde_json::to_string(&pos).unwrap_or_else(|_| "[0.0,0.0]".into())
}

/// Every saved place in a cut's meta, by node.
pub fn layout_from_meta(meta: &BTreeMap<String, String>) -> BTreeMap<NodeId, (f32, f32)> {
    let prefix = format!("{LAYOUT_KEY}.");
    meta.range(prefix.clone()..)
        .take_while(|(k, _)| k.starts_with(&prefix))
        .filter_map(|(k, v)| {
            let id = k[prefix.len()..].parse::<u64>().ok()?;
            Some((NodeId(id), pos_from_json(v)?))
        })
        .collect()
}

/// The edit that saves `node`'s place as `pos` (None forgets it). It
/// touches that node's key alone; None when nothing would change.
pub fn place_node(
    cut: &Cut,
    at: CutRef,
    node: NodeId,
    pos: Option<(f32, f32)>,
) -> Option<Command> {
    let key = layout_key(node);
    let before = cut.meta.get(&key).cloned();
    let after = pos.map(pos_to_json);
    (after != before).then_some(Command::SetCutMeta {
        at,
        key,
        before,
        after,
    })
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
            .filter(|n| matches!(n.kind, NodeKind::DrawingSource { column: c, .. } if c == column))
            .map(|n| n.id)
            .collect()
    }

    /// All Transform and Effect nodes with any param bound to the given
    /// parameter column (invalidation roots when a param key changes).
    pub fn sources_of_param(&self, param: ParamId) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|n| n.kind.references(param))
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
