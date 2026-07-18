//! GPU raster paint layer of the raster brush engine.
//!
//! Owns one `Rgba16Float` layer texture at the project resolution and an
//! instanced soft-round-dab pipeline. This texture always mirrors the CURRENT
//! drawing's raster cel:
//! * `sync_from` uploads the engine's stored tiles into it (on frame switch /
//!   undo);
//! * `paint` stamps live dabs during a stroke;
//! * `read_tiles` reads it back at pen-up so the app can commit a `PaintTiles`
//!   command to the engine (undo + persistence).
//!
//! Premultiplied "over" blending, ink at opacity 1 (wet-buffer anti-darkening
//! is Phase 3). The texel bytes ARE the engine's tile bytes — opaque to the
//! headless engine, round-tripped bit-for-bit.

use anim_core::raster::{TileCoord, TileData, TILE};
use eframe::egui;
use eframe::egui_wgpu::{RenderState, Renderer};
use egui::mutex::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Bytes per RGBA16 pixel.
const BPP: u32 = 8;

/// Eraser blend: result = dst * (1 - src.alpha). A dab's coverage subtracts
/// from the layer instead of adding ink, so painting with it erases to true
/// transparency (and keeps the premultiplied invariant, since rgb and a are
/// scaled by the same factor).
const ERASE_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// One brush dab in layer texel space (= project pixel space).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Dab {
    pub center: [f32; 2],
    pub radius: f32,
    pub hardness: f32,
    /// Straight (non-premultiplied) linear RGBA; premultiplied in the shader.
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    inv_size: [f32; 2],
    _pad: [f32; 2],
}

/// An onion-skin ghost texture (adjacent cel), drawn tinted under the current.
struct OnionSlot {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    tex_id: egui::TextureId,
    hash: u64,
    width: u32,
    height: u32,
}

pub struct PaintLayer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<RwLock<Renderer>>,
    pipeline: wgpu::RenderPipeline,
    /// Destination-out variant of `pipeline` for the eraser tool.
    erase_pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    tex_id: egui::TextureId,
    width: u32,
    height: u32,
    /// Sampler filter used to display the layer (Canvas scaling filter setting).
    filter: wgpu::FilterMode,
    /// Onion ghosts: [0]=previous cel, [1]=next cel.
    onion: [Option<OnionSlot>; 2],

    // --- Wet buffer: the live brush stroke paints here, NOT onto the cel.
    // Displayed composited at the stroke opacity; merged onto the cel ONCE at
    // pen-up (composite_wet), so overlapping dabs never build past the opacity
    // ceiling — Krita's temporaryTarget / indirect-painting model.
    wet_texture: wgpu::Texture,
    wet_view: wgpu::TextureView,
    wet_tex_id: egui::TextureId,
    /// Fullscreen pass that blends the wet buffer onto the cel at an opacity.
    composite_pipeline: wgpu::RenderPipeline,
    composite_layout: wgpu::BindGroupLayout,
    composite_bind: wgpu::BindGroup,
    composite_sampler: wgpu::Sampler,
    /// Uniform: [opacity, 0, 0, 0].
    opacity_buf: wgpu::Buffer,
}

const SHADER: &str = r#"
struct U { inv_size: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> u: U;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) radius: f32,
    @location(2) hardness: f32,
    @location(3) color: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) hardness: f32,
    @location(3) color: vec4<f32>,
) -> VsOut {
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0,  1.0), vec2<f32>(1.0,  1.0),
    );
    let corner = corners[vid];
    let half_extent = radius + 1.5;          // +AA pad
    let local = corner * half_extent;
    let texel = center + local;
    let clip = vec2<f32>(
        texel.x * u.inv_size.x * 2.0 - 1.0,
        1.0 - texel.y * u.inv_size.y * 2.0,  // flip Y
    );
    var out: VsOut;
    out.pos = vec4<f32>(clip, 0.0, 1.0);
    out.local = local;
    out.radius = radius;
    out.hardness = hardness;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let rr = dot(in.local, in.local) / max(in.radius * in.radius, 1e-6);
    let fw = max(fwidth(rr), 1e-5);
    let h = clamp(in.hardness, 0.01, 0.99);
    // MyPaint two-segment hardness falloff in rr = (r/radius)^2 space.
    var opa = select(
        (h / (1.0 - h)) * (1.0 - rr),        // rr > h
        1.0 - rr * (1.0 / h - 1.0),          // rr <= h
        rr <= h,
    );
    opa = clamp(opa, 0.0, 1.0);
    let aa = clamp((1.0 - rr) / fw, 0.0, 1.0);   // analytic rim AA
    let a = opa * aa * in.color.a;
    return vec4<f32>(in.color.rgb * a, a);       // premultiplied
}
"#;

/// Fullscreen pass: sample the wet buffer, scale by the stroke opacity (uniform
/// multiply of a premultiplied color = correct opacity in linear space), blend
/// over the cel with premultiplied-over.
const COMPOSITE_SHADER: &str = r#"
struct CU { opacity: vec4<f32> };
@group(0) @binding(0) var wet_tex: texture_2d<f32>;
@group(0) @binding(1) var wet_smp: sampler;
@group(0) @binding(2) var<uniform> cu: CU;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    // Fullscreen triangle.
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0),
    );
    let p = corners[vid];
    var out: VsOut;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(wet_tex, wet_smp, in.uv) * cu.opacity.x;
}
"#;

impl PaintLayer {
    pub fn new(rs: &RenderState, width: u32, height: u32) -> Self {
        let device = rs.device.clone();
        let queue = rs.queue.clone();
        let renderer = rs.renderer.clone();

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dab_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dab_uniform"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("dab_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dab_bg"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dab_pl"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        // Brush = premultiplied "over" (adds ink); eraser = destination-out
        // (subtracts coverage). Same dab geometry/shader, different blend.
        let pipeline = Self::make_dab_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        let erase_pipeline =
            Self::make_dab_pipeline(&device, &pipeline_layout, &shader, ERASE_BLEND);

        let filter = wgpu::FilterMode::Linear;
        let (texture, view, tex_id) =
            Self::make_target(&device, &renderer, &queue, &uniform_buf, width, height, filter);
        let (wet_texture, wet_view, wet_tex_id) =
            Self::make_target(&device, &renderer, &queue, &uniform_buf, width, height, filter);

        // Wet→cel composite: sampled wet texture × opacity uniform, blended over.
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite_shader"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE_SHADER.into()),
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
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
        let composite_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite_pl"),
            bind_group_layouts: &[Some(&composite_layout)],
            immediate_size: 0,
        });
        let composite_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("composite_pipeline"),
                layout: Some(&composite_pl),
                vertex: wgpu::VertexState {
                    module: &composite_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &composite_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
        let composite_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("composite_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let opacity_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("opacity_uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let composite_bind = Self::make_composite_bind(
            &device,
            &composite_layout,
            &wet_view,
            &composite_sampler,
            &opacity_buf,
        );

        Self {
            device,
            queue,
            renderer,
            pipeline,
            erase_pipeline,
            uniform_buf,
            bind_group,
            texture,
            view,
            tex_id,
            width,
            height,
            filter,
            onion: [None, None],
            wet_texture,
            wet_view,
            wet_tex_id,
            composite_pipeline,
            composite_layout,
            composite_bind,
            composite_sampler,
            opacity_buf,
        }
    }

    /// Bind group for the wet→cel composite (rebuilt when the wet view changes).
    fn make_composite_bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        wet_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        opacity_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite_bg"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(wet_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: opacity_buf.as_entire_binding(),
                },
            ],
        })
    }

    /// Build a soft-round-dab render pipeline with the given blend (brush = over,
    /// eraser = destination-out). Shares the shader and instance layout.
    fn make_dab_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        shader: &wgpu::ShaderModule,
        blend: wgpu::BlendState,
    ) -> wgpu::RenderPipeline {
        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Dab>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32 },
                wgpu::VertexAttribute { offset: 12, shader_location: 2, format: wgpu::VertexFormat::Float32 },
                wgpu::VertexAttribute { offset: 16, shader_location: 3, format: wgpu::VertexFormat::Float32x4 },
            ],
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dab_pipeline"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[instance_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Create a blank Rgba16Float layer texture with the paint-layer usages.
    fn create_layer_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    /// Create the layer texture, clear it, register it with egui, and push the
    /// matching size uniform. Returns the texture + view + egui texture id.
    fn make_target(
        device: &wgpu::Device,
        renderer: &Arc<RwLock<Renderer>>,
        queue: &wgpu::Queue,
        uniform_buf: &wgpu::Buffer,
        width: u32,
        height: u32,
        filter: wgpu::FilterMode,
    ) -> (wgpu::Texture, wgpu::TextureView, egui::TextureId) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("paint_layer"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC   // readback at pen-up
                | wgpu::TextureUsages::COPY_DST,  // upload engine tiles (sync_from)
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        queue.write_buffer(
            uniform_buf,
            0,
            bytemuck::bytes_of(&Uniforms {
                inv_size: [1.0 / width as f32, 1.0 / height as f32],
                _pad: [0.0, 0.0],
            }),
        );

        // Clear the fresh texture to transparent.
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("clear_paint_layer"),
        });
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
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
        queue.submit(Some(enc.finish()));

        let tex_id = renderer
            .write()
            .register_native_texture(device, &view, filter);
        (texture, view, tex_id)
    }

    /// Recreate the layer if the project resolution changed (clears content).
    pub fn ensure_size(&mut self, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        // Free the old egui registrations first — they pin the old full-canvas
        // textures inside the renderer for the app's lifetime otherwise.
        {
            let mut renderer = self.renderer.write();
            renderer.free_texture(&self.tex_id);
            renderer.free_texture(&self.wet_tex_id);
        }
        let (texture, view, tex_id) = Self::make_target(
            &self.device,
            &self.renderer,
            &self.queue,
            &self.uniform_buf,
            width,
            height,
            self.filter,
        );
        self.texture = texture;
        self.view = view;
        self.tex_id = tex_id;
        self.width = width;
        self.height = height;
        self.onion = [None, None]; // stale size

        // Wet buffer follows the layer size; its bind group references the view.
        let (wet_texture, wet_view, wet_tex_id) = Self::make_target(
            &self.device,
            &self.renderer,
            &self.queue,
            &self.uniform_buf,
            width,
            height,
            self.filter,
        );
        self.wet_texture = wet_texture;
        self.wet_view = wet_view;
        self.wet_tex_id = wet_tex_id;
        self.composite_bind = Self::make_composite_bind(
            &self.device,
            &self.composite_layout,
            &self.wet_view,
            &self.composite_sampler,
            &self.opacity_buf,
        );
    }

    /// Switch the display sampler filter (Canvas scaling filter setting), live.
    pub fn set_filter(&mut self, filter: wgpu::FilterMode) {
        if filter == self.filter {
            return;
        }
        self.filter = filter;
        let mut renderer = self.renderer.write();
        renderer.update_egui_texture_from_wgpu_texture(
            &self.device,
            &self.view,
            filter,
            self.tex_id,
        );
        // The wet buffer previews the very pixels being committed — it must
        // sample with the same filter as the cel or the stroke "snaps" at pen-up.
        renderer.update_egui_texture_from_wgpu_texture(
            &self.device,
            &self.wet_view,
            filter,
            self.wet_tex_id,
        );
    }

    pub fn texture_id(&self) -> egui::TextureId {
        self.tex_id
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Clear the whole layer to transparent.
    pub fn clear(&mut self) {
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("clear"),
        });
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
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
        self.queue.submit(Some(enc.finish()));
    }

    /// Stamp a batch of dabs onto the CEL layer (accumulate — LoadOp::Load).
    /// `erase` selects the destination-out pipeline so the dabs subtract coverage
    /// instead of adding ink. (The eraser paints the cel directly; brush strokes
    /// go through `paint_wet` + `composite_wet`.)
    pub fn paint(&mut self, dabs: &[Dab], erase: bool) {
        let view = self.view.clone();
        self.paint_into(&view, dabs, erase);
    }

    /// Stamp a batch of brush dabs into the WET buffer (the live stroke).
    pub fn paint_wet(&mut self, dabs: &[Dab]) {
        let view = self.wet_view.clone();
        self.paint_into(&view, dabs, false);
    }

    fn paint_into(&mut self, view: &wgpu::TextureView, dabs: &[Dab], erase: bool) {
        if dabs.is_empty() {
            return;
        }
        let instances = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dab_instances"),
            contents: bytemuck::cast_slice(dabs),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("paint_dabs"),
        });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("dabs"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(if erase { &self.erase_pipeline } else { &self.pipeline });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, instances.slice(..));
            pass.draw(0..4, 0..dabs.len() as u32);
        }
        self.queue.submit(Some(enc.finish()));
    }

    /// Merge the wet buffer onto the cel at `opacity` (one premultiplied-over
    /// blend of the whole stroke — overlapping dabs can't exceed the ceiling),
    /// then clear the wet buffer. Call at pen-up, before `read_tiles`.
    pub fn composite_wet(&mut self, opacity: f32) {
        self.queue.write_buffer(
            &self.opacity_buf,
            0,
            bytemuck::bytes_of(&[opacity.clamp(0.0, 1.0), 0.0, 0.0, 0.0]),
        );
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("composite_wet"),
        });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wet_over_cel"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, &self.composite_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        // Clear the wet buffer in the same submission.
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear_wet"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.wet_view,
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
        self.queue.submit(Some(enc.finish()));
    }

    /// Clear the wet buffer without compositing (abandoned stroke).
    pub fn clear_wet(&mut self) {
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("clear_wet"),
        });
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear_wet"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.wet_view,
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
        self.queue.submit(Some(enc.finish()));
    }

    /// egui texture id of the wet buffer (drawn over the cel at the stroke
    /// opacity while a brush stroke is live).
    pub fn wet_id(&self) -> egui::TextureId {
        self.wet_tex_id
    }

    /// Replace the layer's contents with the engine's stored tiles (called when
    /// the current drawing changes — frame switch, undo, redo). Tiles carry the
    /// exact texel bytes, so this is a bit-for-bit restore.
    pub fn sync_from(&mut self, tiles: &BTreeMap<TileCoord, Arc<TileData>>) {
        let texture = self.texture.clone();
        let view = self.view.clone();
        self.fill_texture(&texture, &view, tiles);
    }

    /// Clear a texture and upload the given tiles into it (shared by the main
    /// layer sync and the onion-skin slots).
    fn fill_texture(
        &self,
        texture: &wgpu::Texture,
        view: &wgpu::TextureView,
        tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    ) {
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("clear_layer"),
        });
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
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
        self.queue.submit(Some(enc.finish()));

        for ((tx, ty), tile) in tiles {
            let px = tx * TILE as i32;
            let py = ty * TILE as i32;
            if px < 0 || py < 0 || px as u32 >= self.width || py as u32 >= self.height {
                continue;
            }
            let w = (self.width - px as u32).min(TILE as u32);
            let h = (self.height - py as u32).min(TILE as u32);
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: px as u32, y: py as u32, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                tile.as_bytes(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(TILE as u32 * BPP),
                    rows_per_image: Some(TILE as u32),
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        }
    }

    /// Upload an onion-skin cel into slot 0 (previous) or 1 (next). `hash` is the
    /// cel's raster content hash (skip re-upload if unchanged); `None` tiles
    /// clears the slot. Returns the egui texture id to draw, if any.
    pub fn set_onion(
        &mut self,
        slot: usize,
        tiles: Option<&BTreeMap<TileCoord, Arc<TileData>>>,
        hash: u64,
    ) {
        let Some(tiles) = tiles else {
            self.onion[slot] = None;
            return;
        };
        // Reuse the existing slot texture if the content is unchanged.
        if let Some(s) = &self.onion[slot]
            && s.hash == hash && s.width == self.width && s.height == self.height {
                return;
            }
        let (texture, view, tex_id) = match self.onion[slot].take() {
            Some(s) if s.width == self.width && s.height == self.height => {
                (s.texture, s.view, s.tex_id)
            }
            _ => {
                let texture = Self::create_layer_texture(&self.device, self.width, self.height);
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let tex_id = self.renderer.write().register_native_texture(
                    &self.device,
                    &view,
                    wgpu::FilterMode::Linear,
                );
                (texture, view, tex_id)
            }
        };
        self.fill_texture(&texture, &view, tiles);
        self.onion[slot] = Some(OnionSlot {
            texture,
            view,
            tex_id,
            hash,
            width: self.width,
            height: self.height,
        });
    }

    pub fn onion_id(&self, slot: usize) -> Option<egui::TextureId> {
        self.onion[slot].as_ref().map(|s| s.tex_id)
    }

    /// Read the whole layer back into tiles (called once at pen-up). Blocking
    /// map (a small hitch at commit — the async staging ring is a later
    /// optimization). Fully-transparent tiles are dropped so blank areas cost
    /// nothing.
    pub fn read_tiles(&self) -> Vec<(TileCoord, Arc<TileData>)> {
        let unpadded_bpr = self.width * BPP;
        let padded_bpr = unpadded_bpr.div_ceil(256) * 256;
        let buf_size = padded_bpr as u64 * self.height as u64;

        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: buf_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));

        // Block until the copy completes and the buffer is mapped.
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        let _ = rx.recv();

        let data = staging.slice(..).get_mapped_range();
        let tiles_x = self.width.div_ceil(TILE as u32);
        let tiles_y = self.height.div_ceil(TILE as u32);
        let mut out = Vec::new();
        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let mut tile = vec![0u16; anim_core::raster::TILE_LEN];
                for row in 0..TILE as u32 {
                    let py = ty * TILE as u32 + row;
                    if py >= self.height {
                        break;
                    }
                    let copy_px = (self.width - tx * TILE as u32).min(TILE as u32);
                    let src_row = py as usize * padded_bpr as usize
                        + (tx * TILE as u32) as usize * BPP as usize;
                    let dst_row = row as usize * TILE * 4;
                    for px in 0..copy_px as usize {
                        let sb = src_row + px * BPP as usize;
                        for c in 0..4 {
                            let lo = data[sb + c * 2] as u16;
                            let hi = data[sb + c * 2 + 1] as u16;
                            tile[dst_row + px * 4 + c] = lo | (hi << 8);
                        }
                    }
                }
                let td = TileData::from_vec(tile);
                if !td.is_empty() {
                    out.push(((tx as i32, ty as i32), Arc::new(td)));
                }
            }
        }
        drop(data);
        staging.unmap();
        out
    }
}
