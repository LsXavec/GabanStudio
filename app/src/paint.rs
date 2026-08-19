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

use anim_core::ids::ColumnId;
use anim_core::raster::{TILE, TileCoord, TileData};
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

/// Alpha-lock blend (Krita lock-alpha): the dab's colour contribution is
/// masked by the DESTINATION's existing alpha (src_factor = DstAlpha), so a
/// stroke can only recolor pixels that already have coverage — it can never
/// grow the layer's silhouette. The alpha channel is left completely
/// UNCHANGED (src_factor Zero, dst_factor One): coverage is locked exactly
/// as it was, not just "hard to extend". Premultiplied throughout, so a
/// partial-alpha destination (a soft edge) proportionally limits how much
/// colour a full-strength dab can deposit there — verified by hand: dst
/// opaque + full dab -> full recolor, coverage unchanged; dst transparent +
/// any dab -> zero change; dst partial + full dab -> full recolor AT THAT
/// alpha, same coverage.
const ALPHA_LOCK_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::DstAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// Which pipeline a raster dab batch stamps with. Only ever used for the
/// DIRECT-to-active paths (eraser and alpha-lock both bypass the wet
/// buffer — alpha-lock specifically because its mask must read the ACTIVE
/// layer's real coverage, and the wet buffer starts every stroke
/// transparent, which would mask out the whole stroke).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PaintMode {
    Ink,
    Erase,
    AlphaLock,
}

/// One brush dab in layer texel space (= project pixel space).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Dab {
    pub center: [f32; 2],
    /// MINOR radius: the ellipse's short semi-axis (= the circle radius when
    /// `aspect` is 1).
    pub radius: f32,
    pub hardness: f32,
    /// Straight (non-premultiplied) linear RGBA; premultiplied in the shader.
    pub color: [f32; 4],
    /// Unit direction of the ellipse's MAJOR axis (tilt-shaped dabs stamp a
    /// flattened footprint along the pen's lean). [1, 0] for round dabs.
    pub dir: [f32; 2],
    /// Major/minor axis ratio, >= 1. Exactly 1.0 = today's round dab, and the
    /// shader math degenerates to the plain radial falloff bit-for-bit.
    pub aspect: f32,
    /// PSD-brush-engine: 1.0 = shape this dab by the armed TIP MASK
    /// texture instead of the procedural falloff. 0.0 everywhere the
    /// engine is absent — and then the fragment math is the old body
    /// verbatim (NEVER-DO 1).
    pub tip: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    inv_size: [f32; 2],
    _pad: [f32; 2],
    /// [enabled, texel_u, texel_v, strength] — paper-grain sampling in
    /// PAPER space (uv = texel * position) so strokes never swim.
    grain: [f32; 4],
    /// [tip frame count, 0, 0, 0] — a GIH atlas stacks frames vertically.
    misc: [f32; 4],
}

/// An onion-skin ghost texture (adjacent cel composite), drawn tinted under
/// the current cel's sandwich.
struct OnionSlot {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    tex_id: egui::TextureId,
    hash: u64,
    width: u32,
    height: u32,
}

/// One full-canvas Rgba16Float render/display target.
struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    tex_id: egui::TextureId,
}

/// One layer's contribution to a projection: its tiles + display opacity.
/// (Only VISIBLE layers should be passed; visibility is filtered by the caller.)
pub type LayerSlice<'a> = (&'a BTreeMap<TileCoord, Arc<TileData>>, f32);

pub struct PaintLayer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<RwLock<Renderer>>,
    pipeline: wgpu::RenderPipeline,
    /// Destination-out variant of `pipeline` for the eraser tool.
    erase_pipeline: wgpu::RenderPipeline,
    alpha_lock_pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    // PSD-brush-engine: the ARMED preset's tip mask + paper grain (one of
    // each, ever — NEVER-DO 3). Defaults are 1×1 white and never sampled
    // while no dab carries the tip flag / grain is disabled.
    dab_bind_layout: wgpu::BindGroupLayout,
    tip_sampler: wgpu::Sampler,
    grain_sampler: wgpu::Sampler,
    tip_tex: wgpu::Texture,
    grain_tex: wgpu::Texture,
    grain_params: [f32; 4],
    width: u32,
    height: u32,
    /// Sampler filter used to display the layers (Canvas scaling filter setting).
    filter: wgpu::FilterMode,
    /// Onion ghosts: [0]=previous cel, [1]=next cel (whole-cel composites).
    onion: [Option<OnionSlot>; 2],
    /// Raster projections of OTHER columns' cels at the current frame —
    /// edit-view multi-column display (B4). Same shape/lifecycle as an
    /// onion slot (content-hash gated, size-checked), keyed by column so
    /// the set naturally tracks however many columns the sheet has.
    other_cols: std::collections::HashMap<ColumnId, OnionSlot>,

    // --- Sandwich projections (Krita model): the ACTIVE layer is editable in
    // its own texture (bit-exact — read_tiles reads only this); everything
    // below/above it is composited into two display-only projections. VRAM is
    // constant in layer count and the eraser can only ever touch `active`.
    /// The active cel layer, full-strength pixels (display applies opacity).
    active: Target,
    /// Composite of visible layers UNDER the active one.
    below: Target,
    /// Composite of visible layers OVER the active one.
    above: Target,
    /// Staging texture for building projections/onion (upload one layer here,
    /// then blend into the target at its opacity). Never displayed.
    scratch: Target,
    // --- Wet buffer: the live brush stroke paints here, NOT onto the cel.
    // Displayed composited at the stroke opacity; merged onto the cel ONCE at
    // pen-up (composite_wet), so overlapping dabs never build past the opacity
    // ceiling — Krita's temporaryTarget / indirect-painting model.
    wet: Target,

    /// Fullscreen pass blending a sampled texture × opacity onto a target.
    composite_pipeline: wgpu::RenderPipeline,
    composite_layout: wgpu::BindGroupLayout,
    /// Bind group sampling the WET buffer (composite_wet → active).
    composite_bind_wet: wgpu::BindGroup,
    /// Bind group sampling SCRATCH (projection/onion building).
    composite_bind_scratch: wgpu::BindGroup,
    composite_sampler: wgpu::Sampler,
    /// Uniform: [opacity, 0, 0, 0]. LAW: one buffer — issue ONE write_buffer +
    /// ONE submit per composited layer; batching passes into a single encoder
    /// would make every pass read the last-written opacity (switch to dynamic
    /// offsets first if this is ever optimized).
    opacity_buf: wgpu::Buffer,
}

const SHADER: &str = r#"
struct U { inv_size: vec2<f32>, pad: vec2<f32>, grain: vec4<f32>, misc: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var tip_tex: texture_2d<f32>;
@group(0) @binding(2) var tip_samp: sampler;
@group(0) @binding(3) var grain_tex: texture_2d<f32>;
@group(0) @binding(4) var grain_samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) radius: f32,
    @location(2) hardness: f32,
    @location(3) color: vec4<f32>,
    @location(4) dir: vec2<f32>,
    @location(5) aspect: f32,
    @location(6) tip: f32,
    @location(7) center: vec2<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) hardness: f32,
    @location(3) color: vec4<f32>,
    @location(4) dir: vec2<f32>,
    @location(5) aspect: f32,
    @location(6) tip: f32,
) -> VsOut {
    var corners = array<vec2<f32>, 4>(
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0),
    vec2<f32>(-1.0,  1.0), vec2<f32>(1.0,  1.0),
    );
    let corner = corners[vid];
    // Quad covers the MAJOR extent (radius * aspect); aspect 1 = old size.
    let half_extent = radius * max(aspect, 1.0) + 1.5;   // +AA pad
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
    out.dir = dir;
    out.aspect = aspect;
    out.tip = tip;
    out.center = center;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Rotate local into the ellipse frame (major axis = dir), squash the
    // major coordinate by aspect: the radial falloff below then draws an
    // ellipse. dir=[1,0] aspect=1 reduces to the plain circle exactly.
    let xr = dot(in.local, in.dir);
    let yr = dot(in.local, vec2<f32>(-in.dir.y, in.dir.x));
    let ell = vec2<f32>(xr / max(in.aspect, 1.0), yr);
    let rr = dot(ell, ell) / max(in.radius * in.radius, 1e-6);
    var opa: f32;
    if (in.tip > 0.5) {
        // TIP MASK (PSD-brush-engine): the dab IS the stamp. uv spans
        // the dab's footprint in its rotated frame; outside = nothing.
        // Explicit LOD keeps sampling legal under per-instance branching.
        let uv = ell / max(in.radius, 0.001) * 0.5 + vec2<f32>(0.5, 0.5);
        let inb = f32(uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0);
        // GIH atlas: frames stacked vertically; the dab's frame rides in
        // tip (frame = tip - 1). Single-frame tips: frames = 1, fi = 0.
        let frames = max(u.misc.x, 1.0);
        let fi = clamp(floor(in.tip - 1.0), 0.0, frames - 1.0);
        let auv = vec2<f32>(uv.x, (clamp(uv.y, 0.0, 1.0) + fi) / frames);
        opa = textureSampleLevel(tip_tex, tip_samp, vec2<f32>(clamp(uv.x, 0.0, 1.0), auv.y), 0.0).a * inb;
    } else {
        let fw = max(fwidth(rr), 1e-5);
        let h = clamp(in.hardness, 0.01, 0.99);
        // MyPaint two-segment hardness falloff in rr = (r/radius)^2 space.
        var o = select(
        (h / (1.0 - h)) * (1.0 - rr),        // rr > h
        1.0 - rr * (1.0 / h - 1.0),          // rr <= h
        rr <= h,
        );
        o = clamp(o, 0.0, 1.0);
        let aa = clamp((1.0 - rr) / fw, 0.0, 1.0);   // analytic rim AA
        opa = o * aa;
    }
    var a = opa * in.color.a;
    if (u.grain.x > 0.5) {
        // Paper grain, sampled in PAPER space so it never swims with
        // the stroke: dark pattern texels eat ink (Krita's multiply).
        let guv = (in.center + in.local) * vec2<f32>(u.grain.y, u.grain.z);
        let g = textureSampleLevel(grain_tex, grain_samp, guv, 0.0);
        let luma = dot(g.rgb, vec3<f32>(0.299, 0.587, 0.114));
        a = a * (1.0 - u.grain.w * (1.0 - luma));
    }
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
        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let samp_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("dab_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1),
                samp_entry(2),
                tex_entry(3),
                samp_entry(4),
            ],
        });
        // Tip sampler clamps (a stamp ends at its edge); grain repeats
        // (paper tiles). Both linear.
        let tip_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tip_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let grain_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("grain_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let tip_tex = Self::make_rgba_tex(&device, &queue, 1, 1, &[255, 255, 255, 255], "tip_default");
        let grain_tex =
            Self::make_rgba_tex(&device, &queue, 1, 1, &[255, 255, 255, 255], "grain_default");
        let bind_group = Self::make_dab_bind_group(
            &device,
            &bind_layout,
            &uniform_buf,
            &tip_tex,
            &tip_sampler,
            &grain_tex,
            &grain_sampler,
        );
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
        let alpha_lock_pipeline =
            Self::make_dab_pipeline(&device, &pipeline_layout, &shader, ALPHA_LOCK_BLEND);
        let filter = wgpu::FilterMode::Linear;
        let active = Self::make_target(
            &device,
            &renderer,
            &queue,
            &uniform_buf,
            width,
            height,
            filter,
        );
        let wet = Self::make_target(
            &device,
            &renderer,
            &queue,
            &uniform_buf,
            width,
            height,
            filter,
        );
        let below = Self::make_target(
            &device,
            &renderer,
            &queue,
            &uniform_buf,
            width,
            height,
            filter,
        );
        let above = Self::make_target(
            &device,
            &renderer,
            &queue,
            &uniform_buf,
            width,
            height,
            filter,
        );
        let scratch = Self::make_target(
            &device,
            &renderer,
            &queue,
            &uniform_buf,
            width,
            height,
            filter,
        );

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
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
        let composite_bind_wet = Self::make_composite_bind(
            &device,
            &composite_layout,
            &wet.view,
            &composite_sampler,
            &opacity_buf,
        );
        let composite_bind_scratch = Self::make_composite_bind(
            &device,
            &composite_layout,
            &scratch.view,
            &composite_sampler,
            &opacity_buf,
        );
        Self {
            device,
            queue,
            renderer,
            pipeline,
            erase_pipeline,
            alpha_lock_pipeline,
            uniform_buf,
            bind_group,
            dab_bind_layout: bind_layout,
            tip_sampler,
            grain_sampler,
            tip_tex,
            grain_tex,
            grain_params: [0.0; 4],
            width,
            height,
            filter,
            onion: [None, None],
            other_cols: std::collections::HashMap::new(),
            active,
            below,
            above,
            scratch,
            wet,
            composite_pipeline,
            composite_layout,
            composite_bind_wet,
            composite_bind_scratch,
            composite_sampler,
            opacity_buf,
        }
    }

    /// Bind group for a composite pass sampling `src_view` (rebuilt whenever
    /// that view is recreated).
    fn make_composite_bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        src_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        opacity_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite_bg"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(src_view),
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

    fn make_rgba_tex(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        w: u32,
        h: u32,
        rgba: &[u8],
        label: &str,
    ) -> wgpu::Texture {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        tex
    }

    fn make_dab_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniform_buf: &wgpu::Buffer,
        tip: &wgpu::Texture,
        tip_sampler: &wgpu::Sampler,
        grain: &wgpu::Texture,
        grain_sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        let tv = tip.create_view(&Default::default());
        let gv = grain.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dab_bg"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&tv) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(tip_sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&gv) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(grain_sampler) },
            ],
        })
    }

    /// PSD-brush-engine: install the ARMED preset's tip mask and paper
    /// grain (or restore the defaults with None). One texture each —
    /// NEVER-DO 3. `grain` carries (w, h, rgba, scale, strength); the
    /// texel scale bakes the pattern's own size so the shader multiply
    /// stays two numbers.
    pub fn set_brush_resources(
        &mut self,
        tip: Option<(u32, u32, Vec<u8>)>,
        tip_frames: u32,
        grain: Option<(u32, u32, Vec<u8>, f32, f32)>,
    ) {
        let tip_frames = tip_frames.max(1) as f32;
        self.tip_tex = match &tip {
            Some((w, h, rgba)) => Self::make_rgba_tex(&self.device, &self.queue, *w, *h, rgba, "tip"),
            None => Self::make_rgba_tex(&self.device, &self.queue, 1, 1, &[255, 255, 255, 255], "tip_default"),
        };
        self.grain_params = match &grain {
            Some((w, h, _, scale, strength)) => {
                let s = scale.max(0.05);
                [
                    1.0,
                    1.0 / (*w as f32 * s),
                    1.0 / (*h as f32 * s),
                    strength.clamp(0.0, 1.0),
                ]
            }
            None => [0.0; 4],
        };
        self.grain_tex = match &grain {
            Some((w, h, rgba, _, _)) => {
                Self::make_rgba_tex(&self.device, &self.queue, *w, *h, rgba, "grain")
            }
            None => Self::make_rgba_tex(&self.device, &self.queue, 1, 1, &[255, 255, 255, 255], "grain_default"),
        };
        self.bind_group = Self::make_dab_bind_group(
            &self.device,
            &self.dab_bind_layout,
            &self.uniform_buf,
            &self.tip_tex,
            &self.tip_sampler,
            &self.grain_tex,
            &self.grain_sampler,
        );
        self.queue.write_buffer(
            &self.uniform_buf,
            0,
            bytemuck::bytes_of(&Uniforms {
                inv_size: [1.0 / self.width as f32, 1.0 / self.height as f32],
                _pad: [0.0, 0.0],
                grain: self.grain_params,
                misc: [tip_frames, 0.0, 0.0, 0.0],
            }),
        );
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
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 40,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: 44,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32,
                },
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
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
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
    ) -> Target {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("paint_layer"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC   // readback at pen-up
                | wgpu::TextureUsages::COPY_DST, // upload engine tiles (sync_from)
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        queue.write_buffer(
            uniform_buf,
            0,
            bytemuck::bytes_of(&Uniforms {
                inv_size: [1.0 / width as f32, 1.0 / height as f32],
                _pad: [0.0, 0.0],
                grain: [0.0; 4],
                misc: [1.0, 0.0, 0.0, 0.0],
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
        Target {
            texture,
            view,
            tex_id,
        }
    }

    /// Recreate every target if the project resolution changed (clears content).
    pub fn ensure_size(&mut self, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }
        // Free the old egui registrations first — they pin the old full-canvas
        // textures inside the renderer for the app's lifetime otherwise.
        {
            let mut renderer = self.renderer.write();
            for t in [
                &self.active,
                &self.wet,
                &self.below,
                &self.above,
                &self.scratch,
            ] {
                renderer.free_texture(&t.tex_id);
            }
            // Onion slots are dropped below (stale size) — free their
            // registrations too, or the old full-canvas textures stay pinned.
            for slot in self.onion.iter().flatten() {
                renderer.free_texture(&slot.tex_id);
            }
            for slot in self.other_cols.values() {
                renderer.free_texture(&slot.tex_id);
            }
        }
        let mk = |s: &Self| {
            Self::make_target(
                &s.device,
                &s.renderer,
                &s.queue,
                &s.uniform_buf,
                width,
                height,
                s.filter,
            )
        };
        self.active = mk(self);
        self.wet = mk(self);
        self.below = mk(self);
        self.above = mk(self);
        self.scratch = mk(self);
        self.width = width;
        self.height = height;
        self.onion = [None, None]; // stale size
        self.other_cols.clear(); // stale size
        self.composite_bind_wet = Self::make_composite_bind(
            &self.device,
            &self.composite_layout,
            &self.wet.view,
            &self.composite_sampler,
            &self.opacity_buf,
        );
        self.composite_bind_scratch = Self::make_composite_bind(
            &self.device,
            &self.composite_layout,
            &self.scratch.view,
            &self.composite_sampler,
            &self.opacity_buf,
        );
    }

    /// Switch the display sampler filter (Canvas scaling filter setting), live.
    /// Applies to every DISPLAYED target — active, wet, and both projections —
    /// so nothing "snaps" between live stroke and committed pixels.
    pub fn set_filter(&mut self, filter: wgpu::FilterMode) {
        if filter == self.filter {
            return;
        }
        self.filter = filter;
        let mut renderer = self.renderer.write();
        for t in [&self.active, &self.wet, &self.below, &self.above] {
            renderer.update_egui_texture_from_wgpu_texture(&self.device, &t.view, filter, t.tex_id);
        }
        // Onion ghosts and other-column projections sample with the same
        // filter as everything else.
        for slot in self.onion.iter().flatten().chain(self.other_cols.values()) {
            renderer.update_egui_texture_from_wgpu_texture(
                &self.device,
                &slot.view,
                filter,
                slot.tex_id,
            );
        }
    }

    /// egui id of the ACTIVE layer texture.
    pub fn texture_id(&self) -> egui::TextureId {
        self.active.tex_id
    }

    /// egui ids of the below/above sandwich projections.
    pub fn below_id(&self) -> egui::TextureId {
        self.below.tex_id
    }
    pub fn above_id(&self) -> egui::TextureId {
        self.above.tex_id
    }
    fn clear_view(&self, view: &wgpu::TextureView, label: &str) {
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
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
    }

    /// Clear the ACTIVE layer texture to transparent.
    pub fn clear_active(&mut self) {
        let view = self.active.view.clone();
        self.clear_view(&view, "clear_active");
    }

    /// Clear both sandwich projections (blank cel: nothing below or above).
    pub fn clear_projections(&mut self) {
        let below = self.below.view.clone();
        let above = self.above.view.clone();
        self.clear_view(&below, "clear_below");
        self.clear_view(&above, "clear_above");
    }

    /// Build one sandwich projection (`above == false` → below) by compositing
    /// the given VISIBLE layers bottom→top at their opacities. LAW: one
    /// write_buffer + one submit per layer — `opacity_buf` is a single uniform,
    /// so batching passes into one encoder would alias every pass to the
    /// last-written opacity.
    pub fn build_projection(&mut self, above: bool, layers: &[LayerSlice<'_>]) {
        let target = if above {
            self.above.view.clone()
        } else {
            self.below.view.clone()
        };
        self.composite_layers_into(&target, layers);
    }
    pub(crate) fn composite_layers_into(
        &mut self,
        target: &wgpu::TextureView,
        layers: &[LayerSlice<'_>],
    ) {
        self.clear_view(target, "clear_projection");
        let scratch_tex = self.scratch.texture.clone();
        let scratch_view = self.scratch.view.clone();
        for (tiles, opacity) in layers {
            // Upload this layer's tiles into scratch (clear + write_texture)...
            self.fill_texture(&scratch_tex, &scratch_view, tiles);
            // ...then blend scratch onto the target at the layer's opacity.
            self.queue.write_buffer(
                &self.opacity_buf,
                0,
                bytemuck::bytes_of(&[opacity.clamp(0.0, 1.0), 0.0, 0.0, 0.0]),
            );
            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("layer_composite"),
                });
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("layer_over"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
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
                pass.set_bind_group(0, &self.composite_bind_scratch, &[]);
                pass.draw(0..3, 0..1);
            }
            self.queue.submit(Some(enc.finish()));
        }
    }

    /// Stamp a batch of dabs onto the ACTIVE layer directly (accumulate —
    /// LoadOp::Load). `mode` selects Erase (destination-out) or AlphaLock
    /// (masked by the layer's OWN existing alpha) — both bypass the wet
    /// buffer for exactly that reason: their blend needs the active layer's
    /// real coverage as the destination, and the wet buffer starts every
    /// stroke transparent. Ordinary ink strokes go through `paint_wet` +
    /// `composite_wet` instead, for the opacity-ceiling behavior.
    pub fn paint(&mut self, dabs: &[Dab], mode: PaintMode) {
        let view = self.active.view.clone();
        self.paint_into(&view, dabs, mode);
    }

    /// Stamp a batch of brush dabs into the WET buffer (the live stroke).
    pub fn paint_wet(&mut self, dabs: &[Dab]) {
        let view = self.wet.view.clone();
        self.paint_into(&view, dabs, PaintMode::Ink);
    }
    fn paint_into(&mut self, view: &wgpu::TextureView, dabs: &[Dab], mode: PaintMode) {
        if dabs.is_empty() {
            return;
        }
        let instances = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("dab_instances"),
                contents: bytemuck::cast_slice(dabs),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
            pass.set_pipeline(match mode {
                PaintMode::Ink => &self.pipeline,
                PaintMode::Erase => &self.erase_pipeline,
                PaintMode::AlphaLock => &self.alpha_lock_pipeline,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, instances.slice(..));
            pass.draw(0..4, 0..dabs.len() as u32);
        }
        self.queue.submit(Some(enc.finish()));
    }

    /// Merge the wet buffer onto the ACTIVE layer at `opacity` (one
    /// premultiplied-over blend of the whole stroke — overlapping dabs can't
    /// exceed the ceiling), then clear the wet buffer. Call at pen-up, before
    /// `read_tiles`.
    pub fn composite_wet(&mut self, opacity: f32) {
        self.queue.write_buffer(
            &self.opacity_buf,
            0,
            bytemuck::bytes_of(&[opacity.clamp(0.0, 1.0), 0.0, 0.0, 0.0]),
        );
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("composite_wet"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("wet_over_active"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.active.view,
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
            pass.set_bind_group(0, &self.composite_bind_wet, &[]);
            pass.draw(0..3, 0..1);
        }
        // Clear the wet buffer in the same submission.
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear_wet"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.wet.view,
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
        let view = self.wet.view.clone();
        self.clear_view(&view, "clear_wet");
    }

    /// egui texture id of the wet buffer (drawn over the active layer at the
    /// stroke opacity while a brush stroke is live).
    pub fn wet_id(&self) -> egui::TextureId {
        self.wet.tex_id
    }

    /// Replace the ACTIVE layer texture with the engine's stored tiles (called
    /// when the displayed drawing/layer changes — frame switch, layer switch,
    /// undo, redo). Tiles carry the exact texel bytes: a bit-for-bit restore.
    pub fn sync_active(&mut self, tiles: &BTreeMap<TileCoord, Arc<TileData>>) {
        let texture = self.active.texture.clone();
        let view = self.active.view.clone();
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
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
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
                    origin: wgpu::Origin3d {
                        x: px as u32,
                        y: py as u32,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                tile.as_bytes(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(TILE as u32 * BPP),
                    rows_per_image: Some(TILE as u32),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Build an onion-skin ghost into slot 0 (previous) or 1 (next) as the
    /// WHOLE-CEL composite of the neighbour's visible layers. `hash` is the
    /// composite's content hash (skip rebuild if unchanged); `None` clears the
    /// slot.
    pub fn set_onion(&mut self, slot: usize, layers: Option<&[LayerSlice<'_>]>, hash: u64) {
        let Some(layers) = layers else {
            if let Some(old) = self.onion[slot].take() {
                self.renderer.write().free_texture(&old.tex_id);
            }
            return;
        };
        // Reuse the existing slot texture if the content is unchanged.
        if let Some(s) = &self.onion[slot]
            && s.hash == hash
            && s.width == self.width
            && s.height == self.height
        {
            return;
        }
        let (texture, view, tex_id) = match self.onion[slot].take() {
            Some(s) if s.width == self.width && s.height == self.height => {
                (s.texture, s.view, s.tex_id)
            }
            old => {
                // Size changed: free the stale registration before replacing.
                if let Some(old) = old {
                    self.renderer.write().free_texture(&old.tex_id);
                }
                let texture = Self::create_layer_texture(&self.device, self.width, self.height);
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let tex_id =
                    self.renderer
                        .write()
                        .register_native_texture(&self.device, &view, self.filter);
                (texture, view, tex_id)
            }
        };
        self.composite_layers_into(&view, layers);
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

    /// Sync ONE non-active column's raster projection (its own resolved
    /// drawing's visible layers, bottom→top) for edit-view multi-column
    /// display (B4) — same law as `set_onion`: content-hash + size gated,
    /// reuses the existing texture when nothing changed, frees on removal.
    /// `None` clears the column's slot (empty cel / vector-only / column
    /// resolves to nothing this frame).
    pub fn sync_other_column(
        &mut self,
        column: ColumnId,
        layers: Option<(&[LayerSlice<'_>], u64)>,
    ) {
        let Some((layers, hash)) = layers else {
            if let Some(old) = self.other_cols.remove(&column) {
                self.renderer.write().free_texture(&old.tex_id);
            }
            return;
        };
        if let Some(s) = self.other_cols.get(&column)
            && s.hash == hash
            && s.width == self.width
            && s.height == self.height
        {
            return;
        }
        let (texture, view, tex_id) = match self.other_cols.remove(&column) {
            Some(s) if s.width == self.width && s.height == self.height => {
                (s.texture, s.view, s.tex_id)
            }
            old => {
                if let Some(old) = old {
                    self.renderer.write().free_texture(&old.tex_id);
                }
                let texture = Self::create_layer_texture(&self.device, self.width, self.height);
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let tex_id =
                    self.renderer
                        .write()
                        .register_native_texture(&self.device, &view, self.filter);
                (texture, view, tex_id)
            }
        };
        self.composite_layers_into(&view, layers);
        self.other_cols.insert(
            column,
            OnionSlot {
                texture,
                view,
                tex_id,
                hash,
                width: self.width,
                height: self.height,
            },
        );
    }

    /// Drop any column projections for columns that no longer exist (a
    /// column was removed since the last frame) — `sync_other_column` only
    /// ever hears about columns that are STILL there, so removal needs an
    /// explicit prune or the freed column's texture would leak forever.
    pub fn prune_other_columns(&mut self, live: &[ColumnId]) {
        let stale: Vec<ColumnId> = self
            .other_cols
            .keys()
            .filter(|c| !live.contains(c))
            .copied()
            .collect();
        for column in stale {
            if let Some(old) = self.other_cols.remove(&column) {
                self.renderer.write().free_texture(&old.tex_id);
            }
        }
    }
    pub fn other_column_id(&self, column: ColumnId) -> Option<egui::TextureId> {
        self.other_cols.get(&column).map(|s| s.tex_id)
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
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.active.texture,
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
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
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
