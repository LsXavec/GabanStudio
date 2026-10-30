//! Pull-based, memoized graph evaluation — the Nuke/Fusion pattern that keeps
//! interactive fps honest with heavy content: each (node, frame) computes once;
//! edits invalidate only the edited node and its downstream closure.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
    /// GM22: files stat'ed for ImageSequence values.
    pub file_stats: u64,
}

#[derive(Default)]
pub struct Evaluator {
    cache: HashMap<(NodeId, u32), Value>,
    pub stats: EvalStats,
    /// GM22: the project's folder, where an ImageSequence's relative files
    /// are stat'ed (None: an unsaved project).
    base: Option<PathBuf>,
    /// GM22: each ImageSequence node's file stats, dropped with its values.
    seq_stats: HashMap<NodeId, crate::r3d::SeqStats>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self::default()
    }

    /// GM22: the project's folder (the app hands it over whenever the file
    /// path changes). Another folder drops every value — rare: a Save As,
    /// an Open.
    pub fn set_base(&mut self, base: Option<PathBuf>) {
        if self.base != base {
            self.base = base;
            self.invalidate_all();
        }
    }

    pub fn base(&self) -> Option<&Path> {
        self.base.as_deref()
    }

    /// Evaluate the cut's output node at `frame`.
    pub fn eval(&mut self, cut: &Cut, frame: u32) -> Result<Value> {
        let output = cut.graph.output.ok_or(EngineError::NoOutput)?;
        let mut visiting = HashSet::new();
        self.eval_node(cut, output, frame, &mut visiting)
    }

    /// GM9: evaluate any node of the cut at `frame` (the same memo).
    pub fn eval_at(&mut self, cut: &Cut, node: NodeId, frame: u32) -> Result<Value> {
        let mut visiting = HashSet::new();
        self.eval_node(cut, node, frame, &mut visiting)
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
            NodeKind::DrawingSource {
                column,
                roles,
                absorb_trace,
            } => {
                // A missing column (deleted while a DrawingSource still reads
                // it) is TOLERATED as transparent, not an error — both frame
                // renderers (CPU export + GPU compositor) already draw it that
                // way, and erroring here made composite view refuse a graph
                // that export would happily render. The distinct sentinel
                // keeps the hash honest: remove/re-add a column and the value
                // changes accordingly.
                match cut.xsheet.column(*column) {
                    None => Value::image(format!("missing-column({column})")),
                    // Hidden or Ref (GM5): transparent, like every frame
                    // renderer draws it.
                    Some(_) if !cut.column_renders(*column) => Value::image("empty".to_string()),
                    Some(col) => match col.resolve(frame) {
                        Some(d) => match cut.drawing(d) {
                            // Content hash in the recipe: editing artwork
                            // changes the value (and its hash) without any
                            // extra plumbing.
                            Some(dr) => {
                                let plain =
                                    format!("drawing({}:'{}'#{:016x})", d, dr.name, dr.content_hash());
                                // GM13: a role pass also spells its roles,
                                // the absorb and the cel's layer roles (the
                                // content hash leaves roles out, but a
                                // pass's pixels follow them).
                                Value::image(
                                    match crate::look::pass_recipe(roles.as_deref(), *absorb_trace, dr) {
                                        Some(pass) => format!("{plain}{pass}"),
                                        None => plain,
                                    },
                                )
                            }
                            None => Value::image(format!("drawing({d}:?)")),
                        },
                        None => Value::image("empty".to_string()),
                    },
                }
            }
            NodeKind::Solid { rgba } => Value::image(format!(
                "solid(#{:02x}{:02x}{:02x}{:02x})",
                rgba[0], rgba[1], rgba[2], rgba[3]
            )),
            // Content-addressed like DrawingSource; a missing asset is
            // TOLERATED as a sentinel (renders transparent) — the same law
            // as missing columns, so every consumer agrees.
            NodeKind::ImageSource { image } => match cut.image(*image) {
                Some(img) => Value::image(format!(
                    "image({}:'{}'#{:016x})",
                    image, img.name, img.content_hash
                )),
                None => Value::image(format!("missing-image({image})")),
            },
            NodeKind::Transform {
                translate,
                scale,
                rotate_deg,
                binds,
            } => {
                let child = input_value(0, self, visiting)?;
                // Hash the RESOLVED effective params (bound X-sheet columns
                // sampled at this frame): a camera move changes the value
                // per frame; identical resolved params across frames still
                // cache-hit. Plain `{}` float formatting is Rust's shortest
                // EXACT round-trip representation — never quantize here
                // ({:.3} made slow camera moves hash-collide across frames,
                // freezing the GPU view while the CPU export advanced).
                let (t, s, r) = cut.transform_at(*translate, *scale, *rotate_deg, binds, frame);
                Value::image(format!(
                    "xform(t=({},{}),s={},r={}, {})",
                    t.0,
                    t.1,
                    s,
                    r,
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
            NodeKind::Effect { fx, params, binds } => {
                // An unwired optional pin (the glow's matte) renders
                // differently from a wired, empty one, so it spells "-".
                let mut ins = Vec::with_capacity(fx.inputs());
                for slot in 0..fx.inputs() {
                    ins.push(match inputs.get(slot).copied().flatten() {
                        Some(_) => input_value(slot, self, visiting)?.recipe().to_string(),
                        None => "-".to_string(),
                    });
                }
                // The RESOLVED parameters (bound columns sampled at this
                // frame), like Transform: a keyed parameter changes the
                // value per frame; equal resolved values still cache-hit.
                let v = cut.fx_at(*fx, params, binds, frame);
                // GM13: Grain changes with the frame, and a LUT with its
                // file's bytes (the key alone would keep a replaced file's
                // cache). Empty for every other kind.
                let extra = crate::look::fx_recipe_extra(*fx, &v, cut, frame);
                Value::image(format!(
                    "fx({}, {}{extra}, [{}])",
                    fx.name(),
                    v.recipe(),
                    ins.join(", ")
                ))
            }
            NodeKind::Camera {
                planes,
                params,
                binds,
            } => {
                // GM10: every plane with its depth ("-" unwired), the
                // RESOLVED camera with this frame's shake (a still camera
                // still cache-hits across frames), and the paper, which
                // decides what lies beyond the frame ("frame" = the default).
                let mut ins = Vec::with_capacity(planes.len());
                for (slot, plane) in planes.iter().enumerate() {
                    let src = match inputs.get(slot).copied().flatten() {
                        Some(_) => input_value(slot, self, visiting)?.recipe().to_string(),
                        None => "-".to_string(),
                    };
                    ins.push(format!("z={}:{src}", plane.depth));
                }
                let st = crate::camera::CameraState::resolve(cut, params, binds, frame);
                let paper = crate::camera::paper_meta(cut)
                    .map(|p| format!("{},{},{},{}", p.x, p.y, p.w, p.h))
                    .unwrap_or_else(|| "frame".into());
                Value::image(format!(
                    "camera({}, paper={paper}, [{}])",
                    st.recipe(),
                    ins.join(", ")
                ))
            }
            NodeKind::Generator { generator, params, binds } => {
                // GM16: the RESOLVED parameters, the frame the picture moves
                // with ("-" when still: held lines, text and balloons
                // cache-hit across frames; on twos, frames 2k and 2k + 1
                // share one), and the matte pin ("-" unwired).
                let mut ins = Vec::with_capacity(generator.inputs());
                for slot in 0..generator.inputs() {
                    ins.push(match inputs.get(slot).copied().flatten() {
                        Some(_) => input_value(slot, self, visiting)?.recipe().to_string(),
                        None => "-".to_string(),
                    });
                }
                let v = cut.gen_at(*generator, params, binds, frame);
                let moving = v.motion(frame).map_or_else(|| "-".to_string(), |f| f.to_string());
                Value::image(format!(
                    "gen({}, {}, f={moving}, [{}])",
                    generator.name(),
                    v.recipe(),
                    ins.join(", ")
                ))
            }
            // GM17: the embedded model by its content (a re-imported file
            // changes the value) and the RESOLVED parameters (a keyed camera
            // moves per frame, a still one cache-hits); a missing model is
            // the sentinel that renders transparent.
            NodeKind::Model3D { model, params, binds } => match cut.models.get(model) {
                Some(m) => Value::image(format!(
                    "model3d({model}:'{}'#{:016x}, first={}, {})",
                    m.name,
                    m.content_hash,
                    crate::r3d::first_object_id(cut, id),
                    cut.model_at(params, binds, frame).recipe()
                )),
                None => Value::image(format!("missing-model({model})")),
            },
            // GM17: the file number this frame shows ("-" before the
            // sequence starts). The files are read at render, so a sequence
            // re-rendered on disk shows once the value changes (an edit, a
            // reload).
            NodeKind::ImageSequence {
                pattern,
                first,
                offset,
                hold_last,
                fit,
            } => {
                let n = crate::r3d::sequence_number(*first, *offset, frame)
                    .map_or_else(|| "-".to_string(), |n| n.to_string());
                // GM22: once its path resolves — an absolute pattern always,
                // a relative one from the project's folder — the shown
                // file's length and time (a stat, never a read; "-" when it
                // is missing): a file re-rendered on disk changes the value
                // at the next reload, an unchanged one keeps it. A relative
                // pattern with no folder keeps the plain value. The node's
                // stats are kept with its values: a held frame never
                // searches back over files already stat'ed.
                let base = self.base.as_deref();
                let resolves =
                    !pattern.trim().is_empty() && (base.is_some() || !Path::new(pattern).is_relative());
                let file = if resolves {
                    let stamp = self
                        .seq_stats
                        .entry(id)
                        .or_default()
                        .shown(pattern, *first, *offset, *hold_last, frame, base, &mut self.stats.file_stats)
                        .map_or_else(|| "-".to_string(), |(len, ns)| format!("{len}@{ns}"));
                    format!(", file={stamp}")
                } else {
                    String::new()
                };
                Value::image(format!("seq({pattern:?}, n={n}, hold={hold_last}, fit={fit}{file})"))
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
        self.seq_stats.retain(|id, _| !nodes.contains(id));
    }

    pub fn invalidate_all(&mut self) {
        self.cache.clear();
        self.seq_stats.clear();
    }

    pub fn cached_entries(&self) -> usize {
        self.cache.len()
    }
}
