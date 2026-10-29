//! GPU graph compositor — executes `cut.graph` (DrawingSource / Solid /
//! Transform / Blend / Output) into a displayable texture. The render path
//! behind the canvas's COMPOSITE VIEW and the truth playback shows there.
//!
//! Correctness contract: pixel semantics are DEFINED by the CPU reference in
//! `anim_core::export::render_graph_frame` (see
//! research/gpu-graph-compositor-design.md) — this module mirrors those
//! formulas on the GPU. The blend shader's four formulas and the transform's
//! centre-origin forward map must match the CPU's bit-for-bit in structure.
//!
//! Dirty key: the engine evaluator's output Value HASH (`engine.eval`) — the
//! M1 promise made real. Same hash → the cached output texture is reused with
//! zero GPU work; command-driven invalidation (paint, undo, node edits,
//! X-sheet changes) flows through the hash for free.
//!
//! DrawingSource composites reuse `PaintLayer::composite_layers_into` (the
//! scratch + opacity-buffer machinery keeps ONE owner; its one-write-one-submit
//! LAW is preserved). Transform/Blend run here with their own pipelines, one
//! encoder+submit per node pass — the same uniform-aliasing law as opacity_buf,
//! and graphs are tiny so submits only happen when the hash changes.
//!
//! GM9 (D9): an Effect node has no GPU twin — its maths lives once, in
//! `anim_core::fx`. Its whole subtree renders on the CPU, on a worker
//! thread, from a `Cut` snapshot; the result uploads into a texture filed
//! under (node, frame, eval hash). Until it lands the last upload shows
//! and `rendering()` is true (the views' TALLY dot) — the pen never waits.

use anim_core::color::ColorLaw;
use anim_core::graph::{BlendMode, NodeKind};
use anim_core::ids::{ColumnId, NodeId};
use anim_core::model::{Cut, Drawing};
use eframe::egui;
use eframe::egui_wgpu::{RenderState, Renderer};
use egui::mutex::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, mpsc};
use wgpu::util::DeviceExt;

use crate::paint::{LayerSlice, PaintLayer};

/// A pooled intermediate texture (not egui-registered — never displayed).
/// Only the view is used; it keeps the underlying texture alive.
struct PoolTex {
    view: wgpu::TextureView,
    free: bool,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct QuadVert {
    pos: [f32; 2], // clip space
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlendUniform {
    mode: u32,
    _pad: [u32; 3],
}

/// Textured quad: vertices arrive already transformed to clip space (the
/// forward transform runs on the CPU per node — 4 vertices, no inverse
/// needed). The quad clips itself: pixels outside it stay transparent.
/// KNOWN DEVIATION vs the CPU reference: inside the quad, within half a
/// texel of the source border, ClampToEdge repeats the edge texel where the
/// CPU fades to transparency — a sub-pixel fringe on FRACTIONAL transforms
/// only (integer transforms are exact). The CPU law is the export truth.
const XFORM_SHADER: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    var out: VsOut;
    out.pos = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(src, smp, in.uv);
}
"#;

/// Two-input blend, one fullscreen pass, formulas identical to the CPU
/// reference's `blend_into` (premultiplied; alpha = Porter-Duff over for
/// every mode; Add may exceed 1 transiently — the f16 target holds it and
/// the display/export finishers clamp).
const BLEND_SHADER: &str = r#"
struct BU { mode: u32, _p0: u32, _p1: u32, _p2: u32 };
@group(0) @binding(0) var bot: texture_2d<f32>;
@group(0) @binding(1) var top: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;
@group(0) @binding(3) var<uniform> u: BU;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    var corners = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0),
    );
    let p = corners[vid];
    var out: VsOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
    return out;
}

// Mirrors anim-core export.rs blend_into EXACTLY (a = bottom/d, b = top/s);
// change both together or the composite view lies about the export.
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let a = textureSample(bot, smp, in.uv); // input 0 (bottom)
    let b = textureSample(top, smp, in.uv); // input 1 (top)
    let oa = b.a + a.a * (1.0 - b.a);
    var rgb: vec3<f32>;
    switch u.mode {
    case 0u: { rgb = b.rgb + a.rgb * (1.0 - b.a); }                       // Normal
    case 1u: { rgb = b.rgb * a.rgb + b.rgb * (1.0 - a.a) + a.rgb * (1.0 - b.a); } // Multiply
    case 2u: { rgb = b.rgb + a.rgb; }                                     // Add
    case 3u: { rgb = b.rgb + a.rgb - b.rgb * a.rgb; }                     // Screen
    case 4u: { rgb = max(a.rgb - b.rgb, vec3<f32>(0.0)); }                // Subtract
    case 5u: { rgb = min(b.rgb * a.a, a.rgb * b.a) + b.rgb * (1.0 - a.a) + a.rgb * (1.0 - b.a); } // Darken
    case 6u: { rgb = max(b.rgb * a.a, a.rgb * b.a) + b.rgb * (1.0 - a.a) + a.rgb * (1.0 - b.a); } // Lighten
    default: {                                                            // Overlay
    var cs = vec3<f32>(0.0);
    if (b.a > 0.0) { cs = b.rgb / b.a; }
    var cb = vec3<f32>(0.0);
    if (a.a > 0.0) { cb = a.rgb / a.a; }
    let lo = 2.0 * cs * cb;
    let hi = vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - cs) * (vec3<f32>(1.0) - cb);
    let curve = select(hi, lo, cb <= vec3<f32>(0.5));
    rgb = b.a * a.a * curve + b.rgb * (1.0 - a.a) + a.rgb * (1.0 - b.a);
        }
    }
    return vec4<f32>(rgb, oa);
}
"#;

pub struct GraphCompositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<RwLock<Renderer>>,
    width: u32,
    height: u32,
    filter: wgpu::FilterMode,

    /// The displayed output texture + its egui registration.
    out_texture: wgpu::Texture,
    out_view: wgpu::TextureView,
    out_id: egui::TextureId,
    pool: Vec<PoolTex>,
    xform_pipeline: wgpu::RenderPipeline,
    xform_layout: wgpu::BindGroupLayout,
    blend_pipeline: wgpu::RenderPipeline,
    blend_layout: wgpu::BindGroupLayout,
    blend_buf: wgpu::Buffer,
    sampler: wgpu::Sampler,

    /// Output Value hash of what `out_texture` currently holds.
    cached_hash: Option<u64>,
    /// The colour law the cached output was rendered under (D8: a Solid
    /// node's chip goes through it).
    law: ColorLaw,

    /// GM9: the CPU renders' uploads, their worker (started on first use)
    /// and this execution's eval hash of each CPU root.
    fx: FxCache<FxTex>,
    fx_worker: Option<FxWorker>,
    fx_hashes: HashMap<NodeId, u64>,
    /// An upload landed since `out_texture` was composed.
    fx_fresh: bool,
}

impl Drop for GraphCompositor {
    /// New/Open rebuilds the Editor (and this compositor with it) — free the
    /// egui registration or every project switch pins a full-project
    /// Rgba16Float texture in the renderer for the app's lifetime.
    fn drop(&mut self) {
        self.renderer.write().free_texture(&self.out_id);
    }
}

impl GraphCompositor {
    pub fn new(rs: &RenderState, width: u32, height: u32) -> Self {
        let device = rs.device.clone();
        let queue = rs.queue.clone();
        let renderer = rs.renderer.clone();
        let filter = wgpu::FilterMode::Linear;
        let xform_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("graph_xform_shader"),
            source: wgpu::ShaderSource::Wgsl(XFORM_SHADER.into()),
        });
        let blend_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("graph_blend_shader"),
            source: wgpu::ShaderSource::Wgsl(BLEND_SHADER.into()),
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let smp_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let xform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graph_xform_bgl"),
            entries: &[tex_entry(0), smp_entry(1)],
        });
        let blend_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("graph_blend_bgl"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                smp_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let xform_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graph_xform_pl"),
            bind_group_layouts: &[Some(&xform_layout)],
            immediate_size: 0,
        });
        let blend_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("graph_blend_pl"),
            bind_group_layouts: &[Some(&blend_layout)],
            immediate_size: 0,
        });
        let quad_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVert>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };
        let target_state = [Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Rgba16Float,
            blend: Some(wgpu::BlendState::REPLACE),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let xform_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("graph_xform_pipeline"),
            layout: Some(&xform_pl),
            vertex: wgpu::VertexState {
                module: &xform_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[quad_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &xform_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &target_state,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let blend_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("graph_blend_pipeline"),
            layout: Some(&blend_pl),
            vertex: wgpu::VertexState {
                module: &blend_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &blend_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &target_state,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let blend_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("graph_blend_uniform"),
            size: std::mem::size_of::<BlendUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("graph_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let (out_texture, out_view) = Self::make_texture(&device, width, height);
        let out_id = renderer
            .write()
            .register_native_texture(&device, &out_view, filter);
        let this = Self {
            device,
            queue,
            renderer,
            width,
            height,
            filter,
            out_texture,
            out_view,
            out_id,
            pool: Vec::new(),
            xform_pipeline,
            xform_layout,
            blend_pipeline,
            blend_layout,
            blend_buf,
            sampler,
            cached_hash: None,
            law: ColorLaw::Legacy,
            fx: FxCache::default(),
            fx_worker: None,
            fx_hashes: HashMap::new(),
            fx_fresh: false,
        };
        this.clear_view_now(&this.out_view.clone());
        this
    }
    fn make_texture(
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("graph_node"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        (texture, view)
    }

    /// Recreate the output + drop the pool on a project-resolution change.
    pub fn ensure_size(&mut self, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        self.renderer.write().free_texture(&self.out_id);
        let (t, v) = Self::make_texture(&self.device, width, height);
        self.out_texture = t;
        self.out_view = v;
        self.out_id = self.renderer.write().register_native_texture(
            &self.device,
            &self.out_view,
            self.filter,
        );
        self.pool.clear();
        self.width = width;
        self.height = height;
        self.cached_hash = None;
        self.fx.clear();
        self.clear_view_now(&self.out_view.clone());
    }

    /// Match the canvas display filter (same setting as PaintLayer's targets).
    pub fn set_filter(&mut self, filter: wgpu::FilterMode) {
        if filter == self.filter {
            return;
        }
        self.filter = filter;
        self.renderer.write().update_egui_texture_from_wgpu_texture(
            &self.device,
            &self.out_view,
            filter,
            self.out_id,
        );
    }

    // ---- Execution ----------------------------------------------------------

    /// Execute the graph at `frame` if `output_hash` differs from what the
    /// output texture holds (or a CPU render landed). `fx_hashes` is the
    /// eval hash of each of `cpu_roots(cut)`. Returns the egui texture to
    /// display.
    pub fn execute(
        &mut self,
        output_hash: u64,
        fx_hashes: HashMap<NodeId, u64>,
        cut: &Cut,
        law: ColorLaw,
        frame: u32,
        paint: &mut PaintLayer,
    ) -> egui::TextureId {
        if self.law != law {
            self.fx.clear(); // D8: the renders were made under the old law
        }
        self.land_fx();
        if self.cached_hash == Some(output_hash) && self.law == law && !self.fx_fresh {
            return self.out_id;
        }
        self.fx_fresh = false;
        self.fx_hashes = fx_hashes;
        self.law = law;
        for p in &mut self.pool {
            p.free = true;
        }
        match cut.graph.output {
            Some(root) => {
                let mut visiting = HashSet::new();
                let slot = self.render_node(cut, root, frame, paint, &mut visiting);
                // Blit the result into the displayed output texture (identity
                // quad through the transform pipeline; 1:1 bilinear = exact).
                let src = self.pool[slot].view.clone();
                let out = self.out_view.clone();
                self.draw_quad(&src, &out, (0.0, 0.0), 1.0, 0.0);
                self.pool[slot].free = true;
            }
            None => {
                let out = self.out_view.clone();
                self.clear_view_now(&out);
            }
        }
        self.cached_hash = Some(output_hash);
        self.out_id
    }

    /// GM9: is a CPU render in flight? (The views show a TALLY dot and the
    /// caller keeps repainting until it lands.)
    pub fn rendering(&self) -> bool {
        self.fx.rendering()
    }

    /// Upload every CPU render the worker has finished (no CPU work here).
    fn land_fx(&mut self) {
        let Some(worker) = &self.fx_worker else {
            return;
        };
        let done: Vec<FxDone> = worker.done.try_iter().collect();
        for d in done {
            self.land_one(d);
        }
    }

    /// Upload one finished render (a size or law change since its job
    /// drops it).
    fn land_one(&mut self, d: FxDone) {
        let Some(texels) = d.texels else {
            // The render panicked: stop waiting (the lamp goes out) and
            // keep the last result.
            self.fx.fail(d.key, d.generation);
            return;
        };
        if d.generation != self.fx.generation()
            || texels.len() != (self.width * self.height * 4) as usize
        {
            return;
        }
        let size = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("graph_fx"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        // write_texture needs no 256-byte row alignment (unlike a buffer copy).
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(8 * self.width),
                rows_per_image: Some(self.height),
            },
            size,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        if self.fx.accept(d.key, d.generation, FxTex { view }) {
            self.fx_fresh = true;
        }
    }

    /// Render one node into a pool slot (post-order over the real graph).
    fn render_node(
        &mut self,
        cut: &Cut,
        id: NodeId,
        frame: u32,
        paint: &mut PaintLayer,
        visiting: &mut HashSet<NodeId>,
    ) -> usize {
        if !visiting.insert(id) {
            return self.acquire_cleared(); // corrupt-file cycle net
        }
        let Ok(node) = cut.graph.node(id) else {
            visiting.remove(&id);
            return self.acquire_cleared();
        };
        let kind = node.kind.clone();
        let inputs = node.inputs.clone();
        let slot = match &kind {
            NodeKind::DrawingSource { column, .. } if !kind.is_filtered_source() => {
                let slot = self.acquire_cleared();
                // A hidden or Ref column yields transparent (GM5).
                if let Some(d) = source_drawing(cut, *column, frame) {
                    // Same visibility/opacity filter as the CPU flatten law.
                    let slices: Vec<LayerSlice<'_>> = d
                        .layers
                        .iter()
                        .filter(|l| l.props.visible && l.props.opacity > 0.0)
                        .map(|l| (&l.raster.tiles, l.props.opacity))
                        .collect();
                    let view = self.pool[slot].view.clone();
                    paint.composite_layers_into(&view, &slices);
                }
                slot
            }
            NodeKind::ImageSource { image } => {
                let slot = self.acquire_cleared();
                if let Some(img) = cut.image(*image) {
                    // Same upload path as a cel layer at full opacity — the
                    // asset's tiles ARE the universal currency.
                    let slices: Vec<LayerSlice<'_>> = vec![(&img.tiles, 1.0)];
                    let view = self.pool[slot].view.clone();
                    paint.composite_layers_into(&view, &slices);
                }
                slot
            }
            NodeKind::Solid { rgba } => {
                // Premultiplied, through the project's colour law — the
                // same ColorLaw::encode the CPU reference and the brush use.
                let px = self.law.premultiplied(*rgba);
                self.acquire_cleared_to(wgpu::Color {
                    r: px[0] as f64,
                    g: px[1] as f64,
                    b: px[2] as f64,
                    a: rgba[3] as f64 / 255.0,
                })
            }
            NodeKind::Transform {
                translate,
                scale,
                rotate_deg,
                binds,
            } => {
                let child = self.render_input(cut, &inputs, 0, frame, paint, visiting);
                let slot = self.acquire_cleared();
                // Same resolved params as the evaluator hash and the CPU
                // reference — the three paths must agree.
                let (t, s, r) = cut.transform_at(*translate, *scale, *rotate_deg, binds, frame);
                if s.abs() >= 1e-6 {
                    let src = self.pool[child].view.clone();
                    let dst = self.pool[slot].view.clone();
                    self.draw_quad(&src, &dst, t, s, r);
                }
                self.pool[child].free = true;
                slot
            }
            NodeKind::Blend { mode } => {
                let a = self.render_input(cut, &inputs, 0, frame, paint, visiting);
                let b = self.render_input(cut, &inputs, 1, frame, paint, visiting);
                let slot = self.acquire_cleared();
                let (av, bv, dv) = (
                    self.pool[a].view.clone(),
                    self.pool[b].view.clone(),
                    self.pool[slot].view.clone(),
                );
                self.draw_blend(&av, &bv, &dv, *mode);
                self.pool[a].free = true;
                self.pool[b].free = true;
                slot
            }
            NodeKind::Output => self.render_input(cut, &inputs, 0, frame, paint, visiting),
            NodeKind::Effect { .. } | NodeKind::Camera { .. } | NodeKind::DrawingSource { .. } => {
                // GM9 (D9): the subtree renders on the CPU worker; this shows
                // its upload — the last one while a newer render is pending.
                // GM10: a Camera too (its paper and sampling are CPU maths);
                // GM13: a role pass or trace-line absorb (the plain source
                // took the arm above).
                let slot = self.acquire_cleared();
                if let Some(&hash) = self.fx_hashes.get(&id) {
                    let key = FxKey { node: id, frame, hash };
                    if self.fx.want(key) {
                        if self.fx_worker.is_none() {
                            self.fx_worker = FxWorker::spawn();
                        }
                        let job = FxJob {
                            key,
                            generation: self.fx.generation(),
                            cut: job_cut(cut, id, frame),
                            law: self.law,
                            width: self.width,
                            height: self.height,
                        };
                        let unsent = match &self.fx_worker {
                            Some(w) => w.jobs.send(job).err().map(|e| e.0),
                            None => Some(job),
                        };
                        if let Some(job) = unsent {
                            // No worker (no thread, or it is gone): render
                            // here this once rather than wait forever.
                            self.fx_worker = None;
                            let d = render_job_guarded(&job, render_job);
                            self.land_one(d);
                        }
                    }
                }
                if let Some((tex, _)) = self.fx.shown(id) {
                    let src = tex.view.clone();
                    let dst = self.pool[slot].view.clone();
                    self.draw_quad(&src, &dst, (0.0, 0.0), 1.0, 0.0);
                }
                slot
            }
        };
        visiting.remove(&id);
        slot
    }
    fn render_input(
        &mut self,
        cut: &Cut,
        inputs: &[Option<(NodeId, u16)>],
        pin: usize,
        frame: u32,
        paint: &mut PaintLayer,
        visiting: &mut HashSet<NodeId>,
    ) -> usize {
        match inputs.get(pin).copied().flatten() {
            Some((src, _)) => self.render_node(cut, src, frame, paint, visiting),
            None => self.acquire_cleared(),
        }
    }

    // ---- Pool + passes ------------------------------------------------------
    fn acquire(&mut self) -> usize {
        if let Some(i) = self.pool.iter().position(|p| p.free) {
            self.pool[i].free = false;
            return i;
        }
        let (_, view) = Self::make_texture(&self.device, self.width, self.height);
        self.pool.push(PoolTex { view, free: false });
        self.pool.len() - 1
    }
    fn acquire_cleared(&mut self) -> usize {
        self.acquire_cleared_to(wgpu::Color::TRANSPARENT)
    }
    fn acquire_cleared_to(&mut self, color: wgpu::Color) -> usize {
        let slot = self.acquire();
        let view = self.pool[slot].view.clone();
        self.clear_pass(&view, color);
        slot
    }
    fn clear_view_now(&self, view: &wgpu::TextureView) {
        self.clear_pass(view, wgpu::Color::TRANSPARENT);
    }
    fn clear_pass(&self, view: &wgpu::TextureView, color: wgpu::Color) {
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("graph_clear"),
            });
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("graph_clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.queue.submit(Some(enc.finish()));
    }

    /// Forward-transform law (must match the CPU reference):
    /// `p_out = C + R_θ · s · (p_in − C) + t`, C = canvas centre, t in px,
    /// positive θ clockwise on screen (y-down).
    fn draw_quad(
        &self,
        src: &wgpu::TextureView,
        dst: &wgpu::TextureView,
        translate: (f32, f32),
        scale: f32,
        rotate_deg: f32,
    ) {
        let (w, h) = (self.width as f32, self.height as f32);
        let (cx, cy) = (w / 2.0, h / 2.0);
        let (sin, cos) = rotate_deg.to_radians().sin_cos();
        let corner = |x: f32, y: f32, u: f32, v: f32| {
            let (ox, oy) = (x - cx, y - cy);
            let tx = cx + (cos * ox - sin * oy) * scale + translate.0;
            let ty = cy + (sin * ox + cos * oy) * scale + translate.1;
            QuadVert {
                pos: [tx / w * 2.0 - 1.0, 1.0 - ty / h * 2.0],
                uv: [u, v],
            }
        };
        let verts = [
            corner(0.0, 0.0, 0.0, 0.0),
            corner(w, 0.0, 1.0, 0.0),
            corner(0.0, h, 0.0, 1.0),
            corner(w, h, 1.0, 1.0),
        ];
        let vbuf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("graph_quad_verts"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("graph_xform_bg"),
            layout: &self.xform_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(src),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("graph_xform"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("graph_xform"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.xform_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_vertex_buffer(0, vbuf.slice(..));
            pass.draw(0..4, 0..1);
        }
        self.queue.submit(Some(enc.finish()));
    }

    /// One blend pass: LAW — one write_buffer + one submit per pass (the mode
    /// uniform is shared; batching passes would alias it, exactly like the
    /// opacity_buf law in paint.rs).
    fn draw_blend(
        &self,
        bottom: &wgpu::TextureView,
        top: &wgpu::TextureView,
        dst: &wgpu::TextureView,
        mode: BlendMode,
    ) {
        let mode_ix = match mode {
            BlendMode::Normal => 0u32,
            BlendMode::Multiply => 1,
            BlendMode::Add => 2,
            BlendMode::Screen => 3,
            BlendMode::Subtract => 4,
            BlendMode::Darken => 5,
            BlendMode::Lighten => 6,
            BlendMode::Overlay => 7,
        };
        self.queue.write_buffer(
            &self.blend_buf,
            0,
            bytemuck::bytes_of(&BlendUniform {
                mode: mode_ix,
                _pad: [0; 3],
            }),
        );
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("graph_blend_bg"),
            layout: &self.blend_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(bottom),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(top),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.blend_buf.as_entire_binding(),
                },
            ],
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("graph_blend"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("graph_blend"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.blend_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(enc.finish()));
    }
}

// ---- GM9: the CPU subtrees ----------------------------------------------------

/// Does this node's maths live only on the CPU? (D9: no forked FX maths;
/// GM10: nor camera maths — a Camera's planes render at the paper; GM13:
/// nor a role pass or a trace-line absorb.)
pub fn needs_cpu(kind: &NodeKind) -> bool {
    matches!(kind, NodeKind::Effect { .. } | NodeKind::Camera { .. }) || kind.is_filtered_source()
}

/// Does `node`, or anything feeding it, need the CPU?
pub fn subtree_needs_cpu(cut: &Cut, node: NodeId) -> bool {
    let mut seen = HashSet::new();
    let mut stack = vec![node];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Ok(n) = cut.graph.node(id) else {
            continue;
        };
        if needs_cpu(&n.kind) {
            return true;
        }
        stack.extend(n.inputs.iter().flatten().map(|(src, _)| *src));
    }
    false
}

/// The CPU nodes the GPU walk from the output reaches — each renders its
/// whole subtree on the worker, so nothing under one is listed.
pub fn cpu_roots(cut: &Cut) -> Vec<NodeId> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<NodeId> = cut.graph.output.into_iter().collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Ok(n) = cut.graph.node(id) else {
            continue;
        };
        if needs_cpu(&n.kind) {
            roots.push(id);
        } else {
            stack.extend(n.inputs.iter().flatten().map(|(src, _)| *src));
        }
    }
    roots
}

/// What a CPU render of `node` at `frame` reads, and nothing more: the
/// cut's sheet and graph with only the drawings and images that subtree
/// shows at that frame (their tiles are shared handles). A job's snapshot
/// then costs what the frame shows, not the whole cut — no other frames'
/// drawings, no audio, no memos. Every field is named, so a new one is a
/// decision here.
pub fn job_cut(cut: &Cut, node: NodeId, frame: u32) -> Cut {
    let mut drawings = HashSet::new();
    let mut images = HashSet::new();
    let mut seen = HashSet::new();
    let mut stack = vec![node];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Ok(n) = cut.graph.node(id) else {
            continue;
        };
        match &n.kind {
            NodeKind::DrawingSource { column, .. } => {
                drawings.extend(source_drawing(cut, *column, frame).map(|d| d.id));
            }
            NodeKind::ImageSource { image } => {
                images.insert(*image);
            }
            _ => {}
        }
        stack.extend(n.inputs.iter().flatten().map(|(src, _)| *src));
    }
    Cut {
        id: cut.id,
        name: cut.name.clone(),
        frame_count: cut.frame_count,
        drawings: cut
            .drawings
            .iter()
            .filter(|d| drawings.contains(&d.id))
            .cloned()
            .collect(),
        xsheet: cut.xsheet.clone(),
        graph: cut.graph.clone(),
        images: cut
            .images
            .iter()
            .filter(|i| images.contains(&i.id))
            .cloned()
            .collect(),
        audio: None,
        audio_props: Default::default(),
        tracks: Vec::new(),
        // GM10: the paper decides what a camera sees beyond the frame.
        meta: cut
            .meta
            .iter()
            .filter(|(k, _)| k.as_str() == anim_core::camera::PAPER_KEY)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        memos: Default::default(),
        column_defs: cut.column_defs.clone(),
        drawing_defs: Default::default(),
        // GM13: a LUT reads its embedded file (shared handles).
        blobs: cut.blobs.clone(),
    }
}

/// What a CPU render is filed under: the node, the frame, and the
/// evaluator's hash of the node's value (its parameters, their bound
/// values and everything upstream) — anything that changes the pixels
/// changes the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FxKey {
    pub node: NodeId,
    pub frame: u32,
    pub hash: u64,
}

/// The upload cache's bookkeeping, apart from the device (tests drive it
/// with plain values): what each node shows, and what is in flight. One
/// result per node is kept — the last one, shown while a newer renders.
pub struct FxCache<T> {
    shown: HashMap<NodeId, (FxKey, T)>,
    pending: HashMap<NodeId, FxKey>,
    /// A key whose render panicked, per node: not asked for again (it
    /// would only fail again); an edit makes a new key, which is.
    failed: HashMap<NodeId, FxKey>,
    /// Bumped by `clear` (a colour law or size change): results of older
    /// jobs are dropped on arrival.
    generation: u64,
}

impl<T> Default for FxCache<T> {
    fn default() -> Self {
        Self {
            shown: HashMap::new(),
            pending: HashMap::new(),
            failed: HashMap::new(),
            generation: 0,
        }
    }
}

impl<T> FxCache<T> {
    /// Should a job for `key` be sent? False when it is shown, already in
    /// flight or failed; true marks it in flight (a newer key replaces an
    /// older one).
    pub fn want(&mut self, key: FxKey) -> bool {
        if self.shown.get(&key.node).is_some_and(|(k, _)| *k == key)
            || self.pending.get(&key.node) == Some(&key)
            || self.failed.get(&key.node) == Some(&key)
        {
            return false;
        }
        self.pending.insert(key.node, key);
        true
    }

    /// A result arrived: shown (true) unless a `clear` came after its job.
    /// A superseded result still shows — it is newer than what showed, the
    /// worker answers in order — while the node keeps waiting for its key.
    pub fn accept(&mut self, key: FxKey, generation: u64, item: T) -> bool {
        if generation != self.generation {
            return false;
        }
        if self.pending.get(&key.node) == Some(&key) {
            self.pending.remove(&key.node);
        }
        self.shown.insert(key.node, (key, item));
        true
    }

    /// A render panicked: the node stops waiting for it (no lamp lit
    /// forever) and keeps what it showed.
    pub fn fail(&mut self, key: FxKey, generation: u64) {
        if generation != self.generation {
            return;
        }
        if self.pending.get(&key.node) == Some(&key) {
            self.pending.remove(&key.node);
        }
        self.failed.insert(key.node, key);
    }

    /// What `node` shows now, and whether it is exactly `key`.
    pub fn shown(&self, node: NodeId) -> Option<(&T, FxKey)> {
        self.shown.get(&node).map(|(k, t)| (t, *k))
    }

    /// Is any render still in flight?
    pub fn rendering(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn clear(&mut self) {
        self.shown.clear();
        self.pending.clear();
        self.failed.clear();
        self.generation += 1;
    }
}

/// One CPU render for the worker: a snapshot ([`job_cut`]), so the
/// document stays free.
pub struct FxJob {
    pub key: FxKey,
    pub generation: u64,
    pub cut: Cut,
    pub law: ColorLaw,
    pub width: u32,
    pub height: u32,
}

/// A finished render: Rgba16Float texels (premultiplied), ready to upload;
/// None when the render panicked.
pub struct FxDone {
    pub key: FxKey,
    pub generation: u64,
    pub texels: Option<Vec<u16>>,
}

/// The worker's whole job — the export's own `render_node`, as f16 bits.
pub fn render_job(job: &FxJob) -> FxDone {
    let px = anim_core::export::render_node_px(
        &job.cut,
        job.law,
        job.key.node,
        job.key.frame,
        job.width,
        job.height,
    );
    FxDone {
        key: job.key,
        generation: job.generation,
        texels: Some(px.into_iter().map(anim_core::raster::f32_to_f16_bits).collect()),
    }
}

/// `render` with its panic caught: the job answers "failed" (no texels)
/// instead of taking the worker down with the key left in flight.
pub fn render_job_guarded(job: &FxJob, render: impl FnOnce(&FxJob) -> FxDone) -> FxDone {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| render(job))).unwrap_or(FxDone {
        key: job.key,
        generation: job.generation,
        texels: None,
    })
}

/// The FX worker thread. Jobs queued for one node collapse to the newest
/// (scrubbing never builds a backlog).
struct FxWorker {
    jobs: mpsc::Sender<FxJob>,
    done: mpsc::Receiver<FxDone>,
}

impl FxWorker {
    fn spawn() -> Option<Self> {
        let (jobs, job_rx) = mpsc::channel::<FxJob>();
        let (done_tx, done) = mpsc::channel::<FxDone>();
        std::thread::Builder::new()
            .name("gaban-fx".into())
            .spawn(move || {
                while let Ok(first) = job_rx.recv() {
                    let mut batch: Vec<FxJob> = vec![first];
                    batch.extend(job_rx.try_iter());
                    let mut newest: HashMap<NodeId, FxJob> = HashMap::new();
                    for j in batch {
                        newest.insert(j.key.node, j);
                    }
                    for j in newest.into_values() {
                        if done_tx.send(render_job_guarded(&j, render_job)).is_err() {
                            return; // the compositor is gone
                        }
                    }
                }
            })
            .ok()?;
        Some(Self { jobs, done })
    }
}

/// An uploaded CPU render.
struct FxTex {
    view: wgpu::TextureView,
}

/// The drawing a DrawingSource on `column` shows at `frame`. A hidden or
/// Ref column yields none, so its slot stays transparent (GM5) — the same
/// rule as the CPU reference's `render_graph_frame`.
fn source_drawing(cut: &Cut, column: ColumnId, frame: u32) -> Option<&Drawing> {
    let col = cut.xsheet.column(column)?;
    if !cut.column_renders(column) {
        return None;
    }
    cut.drawing(col.resolve(frame)?)
}

#[cfg(test)]
mod shader_tests {
    // cargo check never parses the WGSL strings — without this, a shader
    // typo is a startup panic on the rig instead of a red test.
    #[test]
    fn shaders_parse_as_valid_wgsl() {
        naga::front::wgsl::parse_str(super::XFORM_SHADER).expect("XFORM_SHADER");
        naga::front::wgsl::parse_str(super::BLEND_SHADER).expect("BLEND_SHADER");
    }
}

#[cfg(test)]
mod source_tests {
    use crate::config::Action;
    use crate::testkit::EditorHarness;

    /// The GPU path's slot selection, without a device: a hidden or Ref
    /// column feeds its DrawingSource nothing; BG, Book and Cel feed it.
    #[test]
    fn graphcomp_hidden_and_ref_columns_feed_their_source_nothing() {
        let mut h = EditorHarness::new();
        let col = h.state().view.active_column;
        let shown = |h: &EditorHarness| super::source_drawing(h.state().cut(), col, 0).is_some();
        assert!(shown(&h), "the cel feeds its source");
        h.perform(Action::ToggleColumnHidden);
        assert!(!shown(&h), "hidden feeds nothing");
        h.perform(Action::ToggleColumnHidden);
        for (verb, renders) in [
            (Action::ColumnKindRef, false),
            (Action::ColumnKindBg, true),
            (Action::ColumnKindBook, true),
        ] {
            h.perform(verb);
            assert_eq!(shown(&h), renders, "{verb:?}");
        }
    }
}

#[cfg(test)]
mod fx_tests {
    use super::*;
    use crate::doc::AppState;
    use crate::testkit::EditorHarness;
    use anim_core::fx::{FxKind, FxParams, FxValue};

    /// Solid → `fx` → Output, the output set: (solid, fx node, output).
    fn rig(s: &mut AppState, fx: FxKind) -> (NodeId, NodeId, NodeId) {
        let mut add = |kind: NodeKind, x: f32| {
            s.graph_add_node(kind, (x, 0.0));
            s.selected_node.unwrap()
        };
        let solid = add(NodeKind::Solid { rgba: [64, 128, 192, 255] }, 0.0);
        let node = add(NodeKind::effect(fx), 200.0);
        let out = add(NodeKind::Output, 400.0);
        s.graph_connect(solid, node, 0);
        s.graph_connect(node, out, 0);
        s.graph_set_output(out);
        (solid, node, out)
    }

    #[test]
    fn fxnode_subtree_needs_cpu_above_an_effect_only() {
        let mut h = EditorHarness::new();
        let s = h.state_mut();
        let (solid, glow, out) = rig(s, FxKind::Glow);
        assert!(needs_cpu(&s.cut().graph.node(glow).unwrap().kind));
        assert!(subtree_needs_cpu(s.cut(), out), "above an Effect");
        assert!(subtree_needs_cpu(s.cut(), glow), "the Effect itself");
        assert!(!subtree_needs_cpu(s.cut(), solid), "below it, the GPU");
        assert_eq!(cpu_roots(s.cut()), vec![glow]);
        // A plain graph: Solid → Output.
        s.graph_connect(solid, out, 0);
        assert!(!subtree_needs_cpu(s.cut(), out), "a plain graph stays on the GPU");
        assert!(cpu_roots(s.cut()).is_empty());
    }

    #[test]
    fn fxnode_cache_key_follows_the_eval_hash() {
        let mut h = EditorHarness::new();
        let s = h.state_mut();
        let (solid, grade, _) = rig(s, FxKind::Grade);
        let (scene, cut) = (s.view.scene, s.view.cut);
        let hash = |s: &mut AppState| s.engine.eval_node(scene, cut, grade, 0).unwrap().hash();
        let h0 = hash(s);
        assert_eq!(hash(s), h0, "stable while nothing changes");
        let k0 = FxKey { node: grade, frame: 0, hash: h0 };
        let mut cache: FxCache<Vec<u16>> = FxCache::default();
        assert!(cache.want(k0), "a miss sends a job");
        assert!(!cache.want(k0), "in flight: not sent twice");
        assert!(cache.rendering());
        // The worker's job, run here: no thread, no device.
        let law = s.engine.project.color_law;
        let job = FxJob {
            key: k0,
            generation: cache.generation(),
            cut: job_cut(s.cut(), grade, 0),
            law,
            width: 8,
            height: 8,
        };
        let done = render_job(&job);
        // A neutral grade is the export's own pixels of its input.
        let plain: Vec<u16> = anim_core::export::render_node_px(s.cut(), law, solid, 0, 8, 8)
            .into_iter()
            .map(anim_core::raster::f32_to_f16_bits)
            .collect();
        let texels = done.texels.expect("rendered");
        assert_eq!(texels, plain);
        assert!(cache.accept(done.key, done.generation, texels));
        assert!(!cache.rendering(), "landed");
        assert!(!cache.want(k0), "a hit sends nothing");
        // A parameter edit changes the hash, so the key; the last result
        // shows while the new one renders.
        let mut params = FxParams::defaults(FxKind::Grade);
        params.set("exposure", FxValue::Float(1.0));
        let binds = Default::default();
        s.graph_set_kind(grade, NodeKind::Effect { fx: FxKind::Grade, params, binds });
        let h1 = hash(s);
        assert_ne!(h1, h0, "the eval hash follows the parameter");
        let k1 = FxKey { hash: h1, ..k0 };
        assert!(cache.want(k1), "a new key is a miss");
        assert_eq!(cache.shown(grade).map(|(_, k)| k), Some(k0));
        assert!(cache.rendering());
        // Another frame is another key.
        assert_ne!(FxKey { frame: 1, ..k1 }, k1);
        // A law or size change clears it: older jobs' results are dropped.
        let old = cache.generation();
        cache.clear();
        assert!(!cache.accept(k1, old, Vec::new()), "a result from before the clear");
        assert!(cache.shown(grade).is_none() && !cache.rendering());
    }

    /// Two tiles of ink on the cel the active column shows at this frame.
    fn ink(s: &mut AppState) {
        use anim_core::raster::{TILE_LEN, TileData};
        let ink = Arc::new(TileData::from_vec(vec![0x3C00; TILE_LEN]));
        let slot = s.active_slot_resolved().expect("an armed layer");
        s.commit_raster(vec![((0, 0), ink.clone()), ((1, 0), ink)], slot);
    }

    #[test]
    fn camview_a_camera_takes_the_cpu_path_and_its_job_keeps_the_paper() {
        assert!(needs_cpu(&NodeKind::camera()), "the camera's maths are CPU maths");
        let mut h = EditorHarness::new();
        let s = h.state_mut();
        let (w, fh) = (s.engine.project.width, s.engine.project.height);
        let wide = anim_core::camera::IRect { x: -100, y: 0, w: w + 200, h: fh };
        s.set_cut_paper(wide);
        s.graph_add_node(NodeKind::camera(), (200.0, 0.0));
        let cam = s.selected_node.unwrap();
        let slim = job_cut(s.cut(), cam, 0);
        assert_eq!(anim_core::camera::paper(&slim, w, fh), wide, "the job sees the paper");
        assert_eq!(slim.meta.len(), 1, "and no other meta");
    }

    #[test]
    fn fxnode_a_job_holds_only_what_its_frame_shows_and_renders_the_same() {
        use crate::config::Action;
        let mut h = EditorHarness::new();
        ink(h.state_mut());
        h.perform(Action::NextFrame);
        h.perform(Action::NewDrawing);
        ink(h.state_mut());
        let s = h.state_mut();
        let col = s.view.active_column;
        s.graph_add_node(NodeKind::source(col), (0.0, 0.0));
        let src = s.selected_node.unwrap();
        s.graph_add_node(NodeKind::effect(FxKind::Blur), (200.0, 0.0));
        let blur = s.selected_node.unwrap();
        s.graph_connect(src, blur, 0);
        let (cut, law) = (s.cut(), s.engine.project.color_law);
        assert!(cut.drawings.len() >= 2, "a drawing on each frame");
        for frame in [0, 1] {
            let shown = source_drawing(cut, col, frame).map(|d| d.id);
            assert!(shown.is_some(), "frame {frame} shows a drawing");
            let slim = job_cut(cut, blur, frame);
            let held: Vec<_> = slim.drawings.iter().map(|d| d.id).collect();
            assert_eq!(held, Vec::from_iter(shown), "frame {frame}: only its own drawing");
            assert!(slim.audio.is_none() && slim.meta.is_empty() && slim.memos.is_empty());
            let px = anim_core::export::render_node_px(&slim, law, blur, frame, 24, 24);
            assert!(px.chunks_exact(4).any(|p| p[3] > 0.0), "frame {frame}: the ink renders");
            assert_eq!(
                px,
                anim_core::export::render_node_px(cut, law, blur, frame, 24, 24),
                "frame {frame}: the snapshot renders the cut's own pixels"
            );
        }
    }

    #[test]
    fn fxnode_a_render_that_panics_stops_the_wait_and_is_not_retried() {
        let k = FxKey { node: NodeId(7), frame: 0, hash: 1 };
        let mut cache: FxCache<Vec<u16>> = FxCache::default();
        assert!(cache.want(k));
        let job = FxJob {
            key: k,
            generation: cache.generation(),
            cut: Cut::new(anim_core::ids::CutId(1), "empty", 1),
            law: ColorLaw::default(),
            width: 1,
            height: 1,
        };
        // The worker survives it (this panic is the test's own).
        let done = render_job_guarded(&job, |_| panic!("deliberate test panic: a broken FX"));
        assert!(done.texels.is_none(), "a panic answers no pixels");
        cache.fail(done.key, done.generation);
        assert!(!cache.rendering(), "nothing left in flight: the lamp goes out");
        assert!(!cache.want(k), "the same key is not sent again");
        assert!(cache.want(FxKey { hash: 2, ..k }), "an edit tries again");
        assert!(cache.rendering());
        // And a render that does not panic passes through untouched.
        let fine = render_job_guarded(&job, render_job);
        assert_eq!(fine.texels.map(|t| t.len()), Some(4));
    }
}
