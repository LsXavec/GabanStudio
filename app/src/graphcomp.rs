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

use anim_core::export::srgb_to_linear;
use anim_core::graph::{BlendMode, NodeKind};
use anim_core::ids::NodeId;
use anim_core::model::Cut;
use eframe::egui;
use eframe::egui_wgpu::{RenderState, Renderer};
use egui::mutex::RwLock;
use std::collections::HashSet;
use std::sync::Arc;
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
        };
        this.clear_view_now(&this.out_view.clone());
        this
    }

    fn make_texture(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
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
        self.out_id = self
            .renderer
            .write()
            .register_native_texture(&self.device, &self.out_view, self.filter);
        self.pool.clear();
        self.width = width;
        self.height = height;
        self.cached_hash = None;
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
    /// output texture holds. Returns the egui texture to display.
    pub fn execute(
        &mut self,
        output_hash: u64,
        cut: &Cut,
        frame: u32,
        paint: &mut PaintLayer,
    ) -> egui::TextureId {
        if self.cached_hash == Some(output_hash) {
            return self.out_id;
        }
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
            NodeKind::DrawingSource { column } => {
                let slot = self.acquire_cleared();
                if let Some(col) = cut.xsheet.column(*column)
                    && let Some(did) = col.resolve(frame)
                    && let Some(d) = cut.drawing(did)
                {
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
            NodeKind::Solid { rgba } => self.acquire_cleared_to(wgpu::Color {
                // Premultiplied linear — the same srgb_to_linear the CPU
                // reference and the brush dab path use.
                r: (srgb_to_linear(rgba[0]) * (rgba[3] as f32 / 255.0)) as f64,
                g: (srgb_to_linear(rgba[1]) * (rgba[3] as f32 / 255.0)) as f64,
                b: (srgb_to_linear(rgba[2]) * (rgba[3] as f32 / 255.0)) as f64,
                a: rgba[3] as f64 / 255.0,
            }),
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
