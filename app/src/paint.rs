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

use anim_core::camera::IRect;
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
    /// BF3c: the alpha-darken ceiling (Krita's dab opacity: the opacity
    /// sensors) for an imported preset's dab, whose `color[3]` then carries
    /// only the flow; 0 = plain over (every non-engine dab, unchanged).
    pub cap: f32,
}

/// GM4: the `Dab::hardness` that marks an ALIASED dab (any value above
/// 1.5). The flag rides the dab, not a uniform, so a peer's replayed
/// stroke keeps its own edge and the wire's Dab layout is unchanged.
pub const ALIASED_HARDNESS: f32 = 2.0;

/// GM4: the procedural dab's coverage at `dist` texels from a round dab's
/// centre — the CPU mirror of the shader's `fs_main` (tip masks aside).
/// The rim AA width is `fwidth(rr)` along the dab's radius, |d(rr)/dr| =
/// 2·dist/radius² per texel.
pub fn dab_coverage(dist: f32, radius: f32, hardness: f32, aliased: bool) -> f32 {
    let rr = (dist * dist) / (radius * radius).max(1e-6);
    if aliased || hardness > 1.5 {
        return if rr <= 1.0 { 1.0 } else { 0.0 };
    }
    let fw = (2.0 * dist / (radius * radius).max(1e-6)).max(1e-5);
    let h = hardness.clamp(0.01, 0.99);
    let o = if rr <= h {
        1.0 - rr * (1.0 / h - 1.0)
    } else {
        (h / (1.0 - h)) * (1.0 - rr)
    };
    let aa = ((1.0 - rr) / fw).clamp(0.0, 1.0);
    o.clamp(0.0, 1.0) * aa
}

/// GM4: a dab texel's final alpha — the CPU mirror of `fs_main`'s tail.
/// `shape` is the coverage (procedural or tip mask), `color_a` the dab's
/// alpha (brush alpha × flow × pressure opacity), `grain_keep` the paper
/// grain's multiplier (1 = no grain). An ALIASED dab is its shape cut at
/// half and nothing else: 0 or 1, never a grey.
pub fn dab_alpha(shape: f32, color_a: f32, grain_keep: f32, hardness: f32) -> f32 {
    if hardness > 1.5 {
        return if shape >= 0.5 { 1.0 } else { 0.0 };
    }
    shape * color_a * grain_keep
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    inv_size: [f32; 2],
    /// GM20: the frame pixel of layer texel (0, 0) — dabs arrive in frame
    /// pixels; [0, 0] for a frame layer.
    origin: [f32; 2],
    /// [enabled, texel_u, texel_v, strength] — paper-grain sampling in
    /// PAPER space (uv = texel * position) so strokes never swim.
    grain: [f32; 4],
    /// [tip frame count, 0, 0, 0] — a GIH atlas stacks frames vertically.
    misc: [f32; 4],
}

/// GM6: the onion's GPU budget — slot 0 the ghosts behind, 1 the ghosts
/// ahead, 2 the light table's pins (`canvas::onion_slot_plan` fills them):
/// any depth and any number of pins, never one texture per ghost.
pub const ONION_SLOTS: usize = 3;

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
    /// GM20: the layer's rect in frame pixels (`width`/`height` = its size);
    /// the onion and column slots stay frame-sized.
    rect: LayerRect,
    /// Bumped by every rebuild — a canvas that synced the old targets
    /// re-uploads (the viewer may rebuild them first).
    generation: u64,
    /// Sampler filter used to display the layers (Canvas scaling filter setting).
    filter: wgpu::FilterMode,
    /// Onion ghosts: [0]=the ghosts behind, [1]=the ghosts ahead (each side's
    /// whole-cel composites, weighted, in ONE texture — GM6: depth 5 adds
    /// no textures), [2]=the light table's pins. Three paper-size textures
    /// at most, never one per ghost.
    onion: [Option<OnionSlot>; ONION_SLOTS],
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
    /// STAGE 2 (PSD-multiplayer-rescope): other artists' LIVE ink —
    /// their dab batches accumulate here as they stream; drawn over the
    /// art and cleared when the strokes' commits replay.
    remote: Target,

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
struct U { inv_size: vec2<f32>, origin: vec2<f32>, grain: vec4<f32>, misc: vec4<f32> };
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
    @location(8) cap: f32,
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
    @location(7) cap: f32,
) -> VsOut {
    var corners = array<vec2<f32>, 4>(
    vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0),
    vec2<f32>(-1.0,  1.0), vec2<f32>(1.0,  1.0),
    );
    let corner = corners[vid];
    // Quad covers the MAJOR extent (radius * aspect); aspect 1 = old size.
    let half_extent = radius * max(aspect, 1.0) + 1.5;   // +AA pad
    let local = corner * half_extent;
    // GM20: frame pixels to layer texels (origin 0 for a frame layer).
    let texel = center + local - u.origin;
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
    out.cap = cap;
    return out;
}

fn dab_cover(in: VsOut) -> f32 {
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
    } else if (in.hardness > 1.5) {
        // GM4 ALIASED (hardness = ALIASED_HARDNESS): full cover inside the
        // rim, nothing outside, no rim AA — hard 0/255 lines.
        opa = select(0.0, 1.0, rr <= 1.0);
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
    if (in.hardness > 1.5) {
        // GM4 ALIASED: the shape alone decides (a tip mask cut at half);
        // flow, pressure opacity, colour alpha and grain never thin it —
        // every texel is 0 or 1 (CPU mirror: dab_alpha).
        a = select(0.0, 1.0, opa >= 0.5);
    }
    return a;
}

// BF3c: the dab's alpha-darken ceiling, 1 when the dab is plain over
// (cap 0) or aliased.
fn dab_cap(in: VsOut) -> f32 {
    return select(1.0, clamp(in.cap, 0.0, 1.0), in.cap > 0.0 && in.hardness <= 1.5);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Over: an alpha-darken dab without dual-source blending is over at
    // cover x ceiling (its pre-BF3c look).
    let a = dab_cover(in) * dab_cap(in);
    return vec4<f32>(in.color.rgb * a, a);       // premultiplied
}
"#;

/// BF3c: Krita's alpha darken (KoCompositeOpAlphaDarken) for the wet
/// buffer, through dual-source blending: blend_src(0) is the dab at its
/// ceiling (rgb x cap x k, cap x k), blend_src(1) the cover k, and the blend
/// One / OneMinusSrc1 gives dst + (cap - dst) x k per channel, so
/// overlapping dabs build toward the ceiling instead of toward 1. A plain
/// over dab (cap 0) has cap 1 here: exactly premultiplied over. Appended
/// to `SHADER` behind `enable dual_source_blending;`.
const DARKEN_FS: &str = r#"
struct DarkenOut {
    @location(0) @blend_src(0) c: vec4<f32>,
    @location(0) @blend_src(1) k: vec4<f32>,
};

@fragment
fn fs_darken(in: VsOut) -> DarkenOut {
    let k = dab_cover(in);
    let cap = dab_cap(in);
    var out: DarkenOut;
    out.c = vec4<f32>(in.color.rgb * cap * k, cap * k);
    out.k = vec4<f32>(k, k, k, k);
    return out;
}
"#;

/// BF3c: the blend `DARKEN_FS` is drawn with.
const DARKEN_BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrc1,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrc1Alpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// BF3c: the wet-buffer blend's CPU mirror for one channel of alpha: `dst`
/// the wet alpha, `k` the dab's cover (`dab_alpha`), `cap` its ceiling
/// (`Dab::cap`, 0 = over). With dual-source blending (`dual`) the dab
/// lerps toward its ceiling; without, it is over at cover x ceiling.
pub fn wet_blend(dst: f32, k: f32, cap: f32, hardness: f32, dual: bool) -> f32 {
    let c = if cap > 0.0 && hardness <= 1.5 { cap.clamp(0.0, 1.0) } else { 1.0 };
    if dual {
        c * k + dst * (1.0 - k)
    } else {
        let a = k * c;
        a + dst * (1.0 - a)
    }
}

/// Fullscreen pass: sample the wet buffer, scale by the stroke opacity (uniform
/// multiply of a premultiplied color = correct opacity in linear space), blend
/// over the cel with premultiplied-over.
const COMPOSITE_SHADER: &str = r#"
struct CU { opacity: vec4<f32>, window: vec4<f32> };
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
    let sampled = textureSample(wet_tex, wet_smp, in.uv);
    // GM20: a frame-sized target reads its window of a paper-sized layer
    // texel for texel (window = [x, y, on, 0]); off = the stretched sample.
    if (cu.window.z > 0.5) {
        let at = vec2<i32>(floor(in.pos.xy)) + vec2<i32>(cu.window.xy);
        return textureLoad(wet_tex, at, 0) * cu.opacity.x;
    }
    return sampled * cu.opacity.x;
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
        // BF3c: ink is alpha darken where the device has dual-source
        // blending (`DARKEN_FS`; over for every cap-0 dab), else over.
        let pipeline = if device.features().contains(wgpu::Features::DUAL_SOURCE_BLENDING) {
            let darken = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("dab_darken_shader"),
                source: wgpu::ShaderSource::Wgsl(
                    format!("enable dual_source_blending;\n{SHADER}\n{DARKEN_FS}").into(),
                ),
            });
            Self::make_dab_pipeline(&device, &pipeline_layout, &darken, DARKEN_BLEND, "fs_darken")
        } else {
            Self::make_dab_pipeline(
                &device,
                &pipeline_layout,
                &shader,
                wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
                "fs_main",
            )
        };
        let erase_pipeline =
            Self::make_dab_pipeline(&device, &pipeline_layout, &shader, ERASE_BLEND, "fs_main");
        let alpha_lock_pipeline =
            Self::make_dab_pipeline(&device, &pipeline_layout, &shader, ALPHA_LOCK_BLEND, "fs_main");
        let filter = wgpu::FilterMode::Linear;
        // GM20: born frame-sized; the canvas's `ensure_size` moves it onto
        // the paper's layer rect.
        let rect = LayerRect::frame(width, height);
        let mk = || Self::make_target(&device, &renderer, &queue, &uniform_buf, &rect, filter);
        let active = mk();
        let wet = mk();
        let remote = mk();
        let below = mk();
        let above = mk();
        let scratch = mk();

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
            size: 32, // composite_uniform: opacity + GM20's window
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
            rect,
            generation: 0,
            filter,
            onion: Default::default(),
            other_cols: std::collections::HashMap::new(),
            active,
            below,
            above,
            scratch,
            wet,
            remote,
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
                origin: [self.rect.origin.0 as f32, self.rect.origin.1 as f32],
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
        fs_entry: &str,
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
                wgpu::VertexAttribute {
                    offset: 48,
                    shader_location: 7,
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
                entry_point: Some(fs_entry),
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
        rect: &LayerRect,
        filter: wgpu::FilterMode,
    ) -> Target {
        let (width, height) = rect.size;
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
                origin: [rect.origin.0 as f32, rect.origin.1 as f32],
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

    /// Recreate every target if the layer rect changed (clears content):
    /// GM20 — the rect is the paper's ([`layer_rect`]), the frame by default.
    pub fn ensure_size(&mut self, rect: LayerRect) {
        if rect == self.rect {
            return;
        }
        // Free the old egui registrations first — they pin the old full-canvas
        // textures inside the renderer for the app's lifetime otherwise.
        {
            let mut renderer = self.renderer.write();
            for t in [
                &self.active,
                &self.wet,
                &self.remote,
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
                &rect,
                s.filter,
            )
        };
        self.active = mk(self);
        self.wet = mk(self);
        self.remote = mk(self);
        self.below = mk(self);
        self.above = mk(self);
        self.scratch = mk(self);
        (self.width, self.height) = rect.size;
        self.rect = rect;
        self.generation += 1;
        self.onion = Default::default(); // stale size
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

    /// GM20: the rect the targets cover now.
    pub fn rect(&self) -> LayerRect {
        self.rect
    }

    /// GM20: how many times the targets were rebuilt (content cleared).
    pub fn generation(&self) -> u64 {
        self.generation
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
        // Layer-sized projections: the whole layer, stretched 1:1.
        self.composite_layers_window(&target, layers, None);
    }

    /// Composite `layers` into a FRAME-sized target (the graph's pool, the
    /// onion and column slots): GM20 — a paper-sized layer gives its frame
    /// window, texel for texel; a frame layer composites as before.
    pub(crate) fn composite_layers_into(
        &mut self,
        target: &wgpu::TextureView,
        layers: &[LayerSlice<'_>],
    ) {
        let window = (!self.rect.is_frame()).then(|| self.rect.frame_window());
        self.composite_layers_window(target, layers, window);
    }

    fn composite_layers_window(
        &mut self,
        target: &wgpu::TextureView,
        layers: &[LayerSlice<'_>],
        window: Option<(u32, u32)>,
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
                bytemuck::cast_slice(&composite_uniform(opacity.clamp(0.0, 1.0), window)),
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
            bytemuck::cast_slice(&composite_uniform(opacity.clamp(0.0, 1.0), None)),
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

    /// STAGE 2: other artists' live ink — dabs accumulate as their
    /// batches stream in (alpha pre-scaled by the canvas).
    pub fn paint_remote(&mut self, dabs: &[Dab]) {
        let view = self.remote.view.clone();
        self.paint_into(&view, dabs, PaintMode::Ink);
    }

    pub fn clear_remote(&mut self) {
        let view = self.remote.view.clone();
        self.clear_view(&view, "clear_remote");
    }

    pub fn remote_id(&self) -> egui::TextureId {
        self.remote.tex_id
    }

    /// PSD-multiplayer-rescope Stage 1: replay a remote stroke's INK dabs
    /// through the SAME pipelines a local stroke uses — dabs stamped into
    /// the (transient) scratch target, then one over-blend onto ACTIVE at
    /// the stroke's opacity. The wet buffer is deliberately untouched: a
    /// guest keeps its own stroke wet until the host's echo, and a replay
    /// must never eat that. Erase/AlphaLock replays use `paint` directly,
    /// exactly like local strokes.
    pub fn replay_ink(&mut self, dabs: &[Dab], opacity: f32) {
        let scratch = self.scratch.view.clone();
        self.clear_view(&scratch, "clear_replay");
        self.paint_into(&scratch, dabs, PaintMode::Ink);
        self.queue.write_buffer(
            &self.opacity_buf,
            0,
            bytemuck::cast_slice(&composite_uniform(opacity.clamp(0.0, 1.0), None)),
        );
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("composite_replay"),
            });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("replay_over_active"),
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
            pass.set_bind_group(0, &self.composite_bind_scratch, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(enc.finish()));
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
        for (coord, tile) in tiles {
            // GM20: the tile's part on the layer rect (negative tiles too).
            let Some(s) = tile_span(*coord, &self.rect) else {
                continue;
            };
            let at = ((s.src.1 * TILE as u32 + s.src.0) * BPP) as usize;
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: s.dst.0,
                        y: s.dst.1,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &tile.as_bytes()[at..],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(TILE as u32 * BPP),
                    rows_per_image: Some(TILE as u32),
                },
                wgpu::Extent3d {
                    width: s.size.0,
                    height: s.size.1,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Build an onion-skin ghost into slot 0 (behind), 1 (ahead) or 2 (the
    /// light table's pins) as the WHOLE-CEL composite of the ghosts' visible
    /// layers (bottom→top; a ghost's weight rides in its opacities). `hash` is the
    /// composite's content hash (skip rebuild if unchanged); `None` clears the
    /// slot.
    pub fn set_onion(&mut self, slot: usize, layers: Option<&[LayerSlice<'_>]>, hash: u64) {
        let Some(layers) = layers else {
            if let Some(old) = self.onion[slot].take() {
                self.renderer.write().free_texture(&old.tex_id);
            }
            return;
        };
        // Reuse the existing slot texture if the content is unchanged. GM20:
        // slots are frame-sized whatever the paper (the layer's frame window).
        let (fw, fh) = self.rect.frame;
        if let Some(s) = &self.onion[slot]
            && s.hash == hash
            && s.width == fw
            && s.height == fh
        {
            return;
        }
        let (texture, view, tex_id) = match self.onion[slot].take() {
            Some(s) if s.width == fw && s.height == fh => (s.texture, s.view, s.tex_id),
            old => {
                // Size changed: free the stale registration before replacing.
                if let Some(old) = old {
                    self.renderer.write().free_texture(&old.tex_id);
                }
                let texture = Self::create_layer_texture(&self.device, fw, fh);
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
            width: fw,
            height: fh,
        });
    }
    pub fn onion_id(&self, slot: usize) -> Option<egui::TextureId> {
        self.onion[slot].as_ref().map(|s| s.tex_id)
    }

    /// GM6: the content hash a slot holds (a caller with costly slices —
    /// the light table's shifted pins — checks it before building them).
    pub fn onion_hash(&self, slot: usize) -> Option<u64> {
        self.onion[slot].as_ref().map(|s| s.hash)
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
        let (fw, fh) = self.rect.frame; // frame-sized, as the onion slots
        if let Some(s) = self.other_cols.get(&column)
            && s.hash == hash
            && s.width == fw
            && s.height == fh
        {
            return;
        }
        let (texture, view, tex_id) = match self.other_cols.remove(&column) {
            Some(s) if s.width == fw && s.height == fh => (s.texture, s.view, s.tex_id),
            old => {
                if let Some(old) = old {
                    self.renderer.write().free_texture(&old.tex_id);
                }
                let texture = Self::create_layer_texture(&self.device, fw, fh);
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
                width: fw,
                height: fh,
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
    /// nothing. GM20: a paper-sized layer can outgrow one staging buffer
    /// (`max_buffer_size`), so it reads back in row bands ([`readback_bands`]),
    /// each gathered into tiles as it lands — one band when it fits.
    pub fn read_tiles(&self) -> Vec<(TileCoord, Arc<TileData>)> {
        let padded_bpr = padded_bpr(self.width);
        let max_buffer = self.device.limits().max_buffer_size;
        let mut gather = TileGather::new(&self.rect);
        for (y0, rows) in readback_bands(&self.rect, padded_bpr as u64, max_buffer) {
            let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size: padded_bpr as u64 * rows as u64,
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
                    origin: wgpu::Origin3d { x: 0, y: y0, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_bpr),
                        rows_per_image: Some(rows),
                    },
                },
                wgpu::Extent3d {
                    width: self.width,
                    height: rows,
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
            gather.band(&data, padded_bpr as usize, y0, rows);
            drop(data);
            staging.unmap();
        }
        gather.finish()
    }
}

/// A readback row of a `width`-texel layer, padded to wgpu's 256-byte copy
/// alignment.
pub fn padded_bpr(width: u32) -> u32 {
    (width * BPP).div_ceil(256) * 256
}

/// GM20: the row bands a readback of `rect` copies, as (first layer row,
/// rows). Each band's staging buffer holds at most `max_buffer` bytes (the
/// device's `max_buffer_size`: a whole 8190×4097 layer needs 268,500,992,
/// past wgpu's default 256 MiB, and an over-size buffer is a validation
/// panic), and a band is cut on a tile row when it holds one, so its tiles
/// come out whole. One band when the layer fits.
pub fn readback_bands(rect: &LayerRect, padded_bpr: u64, max_buffer: u64) -> Vec<(u32, u32)> {
    let h = rect.size.1;
    let fit = (max_buffer / padded_bpr.max(1)).clamp(1, h.max(1) as u64) as u32;
    let t = TILE as u32;
    // The layer rows where a tile row starts: first_edge, first_edge + 64, …
    let first_edge = (t - rect.origin.1.rem_euclid(TILE as i32) as u32) % t;
    let mut bands = Vec::new();
    let mut y = 0;
    while y < h {
        let mut end = y + fit.min(h - y);
        if end < h && end >= first_edge {
            let edge = first_edge + (end - first_edge) / t * t;
            if edge > y {
                end = edge;
            }
        }
        bands.push((y, end - y));
        y = end;
    }
    bands
}

/// GM20: a layer readback gathered into engine tiles through [`tile_span`],
/// band by band (rows of `padded_bpr` bytes from the layer's left edge):
/// every tile the layer touches, negative coords included; fully-transparent
/// tiles dropped. A tile is done when its last row on the layer has landed.
pub struct TileGather {
    rect: LayerRect,
    open: BTreeMap<TileCoord, Vec<u16>>,
    out: Vec<(TileCoord, Arc<TileData>)>,
}

impl TileGather {
    pub fn new(rect: &LayerRect) -> Self {
        Self {
            rect: *rect,
            open: BTreeMap::new(),
            out: Vec::new(),
        }
    }

    /// Take layer rows `y0..y0 + rows` (`data` holds those rows).
    pub fn band(&mut self, data: &[u8], padded_bpr: usize, y0: u32, rows: u32) {
        let r = self.rect;
        if r.size.0 == 0 || r.size.1 == 0 || rows == 0 {
            return;
        }
        let t = TILE as i32;
        let (ox, oy) = r.origin;
        let (b0, b1) = (y0, y0 + rows);
        for ty in (oy + b0 as i32).div_euclid(t)..=(oy + b1 as i32 - 1).div_euclid(t) {
            for tx in ox.div_euclid(t)..=(ox + r.size.0 as i32 - 1).div_euclid(t) {
                let Some(s) = tile_span((tx, ty), &r) else {
                    continue;
                };
                // The tile's layer rows inside this band.
                let (r0, r1) = (s.dst.1.max(b0), (s.dst.1 + s.size.1).min(b1));
                if r0 >= r1 {
                    continue;
                }
                let tile = self
                    .open
                    .entry((tx, ty))
                    .or_insert_with(|| vec![0u16; anim_core::raster::TILE_LEN]);
                for ly in r0..r1 {
                    let src_row =
                        (ly - b0) as usize * padded_bpr + s.dst.0 as usize * BPP as usize;
                    let dst_row =
                        ((ly - s.dst.1 + s.src.1) as usize * TILE + s.src.0 as usize) * 4;
                    for px in 0..s.size.0 as usize {
                        let sb = src_row + px * BPP as usize;
                        for c in 0..4 {
                            let lo = data[sb + c * 2] as u16;
                            let hi = data[sb + c * 2 + 1] as u16;
                            tile[dst_row + px * 4 + c] = lo | (hi << 8);
                        }
                    }
                }
                if s.dst.1 + s.size.1 <= b1 {
                    let px = self.open.remove(&(tx, ty)).unwrap_or_default();
                    let td = TileData::from_vec(px);
                    if !td.is_empty() {
                        self.out.push(((tx, ty), Arc::new(td)));
                    }
                }
            }
        }
    }

    /// The tiles, in tile-row order (every band landed).
    pub fn finish(self) -> Vec<(TileCoord, Arc<TileData>)> {
        self.out
    }
}

/// GM20: the paint layer's own rect in frame pixels: the paper when it
/// holds the whole frame (else the frame), each side capped by the GPU's
/// texture limit. Layer texel (0, 0) is frame pixel `origin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerRect {
    /// Frame pixel of layer texel (0, 0); negative left of / above the frame.
    pub origin: (i32, i32),
    pub size: (u32, u32),
    /// The paper is wider or taller than the GPU paints: the rect is a
    /// window of it that holds the frame.
    pub capped: bool,
    /// The frame (project) size the layer serves.
    pub frame: (u32, u32),
}

/// The texture side assumed before a device reports its own (wgpu's
/// default `max_texture_dimension_2d`).
pub const DEFAULT_MAX_DIM: u32 = 8192;

/// GM20: the side a canvas paints at most: its device's own limit, never
/// past [`DEFAULT_MAX_DIM`]. egui-wgpu asks every device for exactly that,
/// so every peer in a room caps a paper to the same window and replays a
/// stroke into the same rect (the same tiles for the hash audit).
pub fn room_limit(device_max: u32) -> u32 {
    device_max.min(DEFAULT_MAX_DIM)
}

impl LayerRect {
    /// The pre-GM20 layer: exactly the frame.
    pub fn frame(width: u32, height: u32) -> Self {
        Self {
            origin: (0, 0),
            size: (width, height),
            capped: false,
            frame: (width, height),
        }
    }

    pub fn is_frame(&self) -> bool {
        self.origin == (0, 0) && self.size == self.frame
    }

    /// The layer texel of frame pixel (0, 0): the frame window (frame-sized)
    /// that the viewer, the graph and the onion read starts here.
    pub fn frame_window(&self) -> (u32, u32) {
        (self.origin.0.unsigned_abs(), self.origin.1.unsigned_abs())
    }

    /// The rect in frame pixels: where the canvas paints, so where the
    /// tools edit (the whole paper unless the GPU caps it).
    pub fn irect(&self) -> IRect {
        IRect {
            x: self.origin.0,
            y: self.origin.1,
            w: self.size.0,
            h: self.size.1,
        }
    }
}

/// Is frame pixel `p` inside `area`?
pub fn holds(area: IRect, p: (i32, i32)) -> bool {
    let (x, y) = (p.0 as i64 - area.x as i64, p.1 as i64 - area.y as i64);
    x >= 0 && y >= 0 && x < area.w as i64 && y < area.h as i64
}

/// The paper the tools and the layer cover: the cut's paper when it holds
/// the whole frame, else the frame (a paper off the frame paints as before).
pub fn paint_area(paper: IRect, frame: (u32, u32)) -> IRect {
    let holds = paper.x <= 0
        && paper.y <= 0
        && paper.x as i64 + paper.w as i64 >= frame.0 as i64
        && paper.y as i64 + paper.h as i64 >= frame.1 as i64;
    if holds && paper.w > 0 && paper.h > 0 {
        paper
    } else {
        IRect::frame(frame.0, frame.1)
    }
}

/// THE layer rect of a paper (one helper for every `ensure_size` caller):
/// the [`paint_area`], each side over `max_dim` cut to a `max_dim` window
/// centred on the frame and kept inside the paper.
pub fn layer_rect(paper: IRect, frame: (u32, u32), max_dim: u32) -> LayerRect {
    let area = paint_area(paper, frame);
    let axis = |p0: i32, plen: u32, flen: u32| -> (i32, u32, bool) {
        if plen <= max_dim {
            return (p0, plen, false);
        }
        let len = max_dim.max(flen);
        let lo = p0 as i64;
        let hi = p0 as i64 + plen as i64 - len as i64;
        let start = ((flen as i64 - len as i64) / 2).clamp(lo, hi);
        (start as i32, len, true)
    };
    let (x, w, cx) = axis(area.x, area.w, frame.0);
    let (y, h, cy) = axis(area.y, area.h, frame.1);
    LayerRect {
        origin: (x, y),
        size: (w, h),
        capped: cx || cy,
        frame,
    }
}

/// The part of `tile` on the layer: `src` its texel offset inside the
/// tile, `dst` the layer texel it lands on, `size` the overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileSpan {
    pub src: (u32, u32),
    pub dst: (u32, u32),
    pub size: (u32, u32),
}

/// Where engine tile `tile` (frame tile coords, negative legal) lies on the
/// layer `rect`; None when it is off the layer. Upload and readback both go
/// through it.
pub fn tile_span(tile: TileCoord, rect: &LayerRect) -> Option<TileSpan> {
    let t = TILE as i64;
    let axis = |tc: i32, o: i32, n: u32| -> Option<(u32, u32, u32)> {
        let lo = tc as i64 * t - o as i64; // layer texel of the tile's texel 0
        let (a, b) = (lo.max(0), (lo + t).min(n as i64));
        (a < b).then(|| ((a - lo) as u32, a as u32, (b - a) as u32))
    };
    let (sx, dx, w) = axis(tile.0, rect.origin.0, rect.size.0)?;
    let (sy, dy, h) = axis(tile.1, rect.origin.1, rect.size.1)?;
    Some(TileSpan {
        src: (sx, sy),
        dst: (dx, dy),
        size: (w, h),
    })
}

/// Assemble a layer readback (`data`: rows of `padded_bpr` bytes from layer
/// texel (0, 0)) into engine tiles through [`tile_span`]: every tile the
/// layer touches, negative coords included; fully-transparent tiles dropped.
/// One band of a [`TileGather`].
pub fn layer_tiles(
    data: &[u8],
    padded_bpr: usize,
    rect: &LayerRect,
) -> Vec<(TileCoord, Arc<TileData>)> {
    let mut gather = TileGather::new(rect);
    gather.band(data, padded_bpr, 0, rect.size.1);
    gather.finish()
}

/// Keep `before`'s texels outside the layer rect in a readback `read` (the
/// layer's tiles): a capped or shrunk layer never deletes art it cannot
/// see. Paper = frame layers return `read` unchanged when nothing lies
/// beyond the frame.
pub fn merge_outside(
    before: &BTreeMap<TileCoord, Arc<TileData>>,
    read: Vec<(TileCoord, Arc<TileData>)>,
    rect: &LayerRect,
) -> Vec<(TileCoord, Arc<TileData>)> {
    let mut out: BTreeMap<TileCoord, Arc<TileData>> = read.into_iter().collect();
    let row_len = TILE * 4;
    for (coord, tile) in before {
        let Some(s) = tile_span(*coord, rect) else {
            // Wholly off the layer (the readback never holds it): the
            // engine's tile stands as it is.
            if tile.is_empty() {
                out.remove(coord);
            } else {
                out.insert(*coord, tile.clone());
            }
            continue;
        };
        if s.size == (TILE as u32, TILE as u32) {
            continue; // wholly on the layer: the readback is the truth
        }
        // Across the layer's edge: the readback inside the span, the
        // engine's texels around it, a row slice at a time.
        let mut px = match out.get(coord) {
            Some(t) => t.rgba.to_vec(),
            None => vec![0u16; anim_core::raster::TILE_LEN],
        };
        let (a, b) = (s.src.0 as usize * 4, (s.src.0 + s.size.0) as usize * 4);
        for row in 0..TILE {
            let at = row * row_len;
            if (s.src.1..s.src.1 + s.size.1).contains(&(row as u32)) {
                px[at..at + a].copy_from_slice(&tile.rgba[at..at + a]);
                px[at + b..at + row_len].copy_from_slice(&tile.rgba[at + b..at + row_len]);
            } else {
                px[at..at + row_len].copy_from_slice(&tile.rgba[at..at + row_len]);
            }
        }
        let td = TileData::from_vec(px);
        if td.is_empty() {
            out.remove(coord);
        } else {
            out.insert(*coord, Arc::new(td));
        }
    }
    out.into_iter().collect()
}

/// GPU memory the paint layer holds for `rect` with `other_columns` other
/// columns shown: six layer-sized targets (active, wet, remote, below,
/// above, scratch) and the frame-sized onion and column slots, 8 bytes a
/// texel (Rgba16Float).
pub fn layer_memory(rect: &LayerRect, other_columns: usize) -> u64 {
    let layer = rect.size.0 as u64 * rect.size.1 as u64;
    let frame = rect.frame.0 as u64 * rect.frame.1 as u64;
    (6 * layer + (ONION_SLOTS + other_columns) as u64 * frame) * BPP as u64
}

/// The Paper size window's line for a capped layer, None when the GPU
/// paints the whole paper. It names the covered span on each cut axis, in
/// frame px, so a long pan can be planned on the part that paints.
pub fn cap_text(paper: IRect, rect: &LayerRect, max_dim: u32) -> Option<String> {
    let px = |v: i64| if v < 0 { format!("−{}", -v) } else { v.to_string() };
    let span = |axis: &str, p0: i32, plen: u32, r0: i32, rlen: u32| {
        let (p0, r0) = (p0 as i64, r0 as i64);
        format!(
            "{axis} {} to {} of {} to {}",
            px(r0),
            px(r0 + rlen as i64),
            px(p0),
            px(p0 + plen as i64)
        )
    };
    rect.capped.then(|| {
        let mut spans = Vec::new();
        if rect.size.0 < paper.w {
            spans.push(span("x", paper.x, paper.w, rect.origin.0, rect.size.0));
        }
        if rect.size.1 < paper.h {
            spans.push(span("y", paper.y, paper.h, rect.origin.1, rect.size.1));
        }
        format!(
            "this GPU paints at most {max_dim} px a side: painting covers {}×{} of the {}×{} \
             paper — {}",
            rect.size.0,
            rect.size.1,
            paper.w,
            paper.h,
            spans.join(", ")
        )
    })
}

/// The Paper size window's note for a paper that does not hold the whole
/// frame: [`paint_area`] keeps painting on the frame. None when it holds it.
pub fn coverage_text(paper: IRect, frame: (u32, u32)) -> Option<&'static str> {
    (paint_area(paper, frame) != paper)
        .then_some("this paper does not cover the whole frame: painting stays on the frame")
}

/// The Paper size window's memory line: the layer's GPU memory in MiB.
pub fn memory_text(rect: &LayerRect, other_columns: usize) -> String {
    let mib = layer_memory(rect, other_columns) as f64 / (1024.0 * 1024.0);
    format!("painting uses {mib:.1} MiB of GPU memory (8 bytes a texel)")
}

/// The composite pass's uniform: [opacity, 0, 0, 0] then the window —
/// [x, y, on, 0]. `window` Some reads the source texture from that texel
/// at the target's own size (a layer-sized source into a frame-sized
/// target); None samples it stretched, the pre-GM20 pass.
pub fn composite_uniform(opacity: f32, window: Option<(u32, u32)>) -> [f32; 8] {
    match window {
        Some((x, y)) => [opacity, 0.0, 0.0, 0.0, x as f32, y as f32, 1.0, 0.0],
        None => [opacity, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    }
}

#[cfg(test)]
mod viewnav_tests {
    use super::*;

    #[test]
    fn viewnav_aliased_coverage_is_binary_and_the_wgsl_validates() {
        for radius in [0.5f32, 1.0, 3.0, 7.5, 20.0] {
            for k in 0..=300 {
                let d = k as f32 * radius * 1.5 / 300.0;
                let c = dab_coverage(d, radius, 0.85, true);
                assert!(c == 0.0 || c == 1.0, "aliased r {radius} d {d}: {c}");
                assert_eq!(c, dab_coverage(d, radius, ALIASED_HARDNESS, false));
                let soft = dab_coverage(d, radius, 0.85, false);
                assert!((0.0..=1.0).contains(&soft));
                if d > radius {
                    assert_eq!(soft, 0.0, "nothing outside the rim");
                }
            }
            assert_eq!(dab_coverage(0.0, radius, 0.3, false), 1.0, "full at the centre");
        }
        // Softer hardness, less ink halfway out.
        assert!(dab_coverage(5.0, 10.0, 0.2, false) < dab_coverage(5.0, 10.0, 0.9, false));
        let module = naga::front::wgsl::parse_str(SHADER).expect("SHADER parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("SHADER validates");
    }

    /// BF3c: the dual-source darken shader parses and validates, and the
    /// wet blend's mirror is Krita's alpha darken below the ceiling and
    /// exactly premultiplied over for a cap-0 dab.
    #[test]
    fn bf3c_darken_shader_validates_and_wet_blend_is_alpha_darken() {
        let src = format!("enable dual_source_blending;\n{SHADER}\n{DARKEN_FS}");
        let module = naga::front::wgsl::parse_str(&src).expect("darken shader parses");
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .expect("darken shader validates");
        // KoCompositeOpAlphaDarken, flow f, mask m, opacity o, dst d < o:
        // d + (o - d) m f.
        for (d, k, cap) in [(0.0f32, 0.5f32, 0.3f32), (0.1, 0.25, 0.6), (0.29, 1.0, 0.3)] {
            let want = d + (cap - d) * k;
            assert!((wet_blend(d, k, cap, 0.5, true) - want).abs() < 1e-6);
            // Without dual source: over at k x cap.
            assert!((wet_blend(d, k, cap, 0.5, false) - (k * cap + d * (1.0 - k * cap))).abs() < 1e-6);
        }
        // Dabs at one ceiling never pass it however many overlap.
        let mut d = 0.0;
        for _ in 0..200 {
            d = wet_blend(d, 0.4, 0.3, 0.5, true);
        }
        assert!(d <= 0.3 + 1e-6 && d > 0.299, "builds to the ceiling, got {d}");
        // Cap 0 and aliased dabs: plain over, both paths.
        for dual in [true, false] {
            assert_eq!(wet_blend(0.2, 0.5, 0.0, 0.5, dual), 0.5 + 0.2 * 0.5);
            assert_eq!(wet_blend(0.2, 1.0, 0.3, ALIASED_HARDNESS, dual), 1.0);
        }
    }

    #[test]
    fn viewnav_aliased_dabs_ignore_flow_pressure_tips_and_grain() {
        // A thinned dab (alpha 0.4 = flow × pressure opacity) over grain,
        // procedural and tip-shaped: aliased, every texel is 0 or 1.
        for radius in [0.5f32, 3.0, 20.0] {
            for k in 0..=200 {
                let d = k as f32 * radius * 1.5 / 200.0;
                let shape = dab_coverage(d, radius, 0.85, true);
                let a = dab_alpha(shape, 0.4, 0.7, ALIASED_HARDNESS);
                assert_eq!(a, shape, "aliased r {radius} d {d}: {a}");
                assert!(a == 0.0 || a == 1.0);
            }
        }
        for mask in [0.0f32, 0.2, 0.49, 0.5, 0.75, 1.0] {
            let a = dab_alpha(mask, 0.4, 0.7, ALIASED_HARDNESS);
            assert_eq!(a, if mask >= 0.5 { 1.0 } else { 0.0 }, "tip mask {mask}");
        }
        // Soft dabs keep every thinning.
        assert!((dab_alpha(0.5, 0.4, 0.7, 0.85) - 0.14).abs() < 1e-6);
        // The shader's tail is the same law, after the grain.
        let tail = SHADER.find("a = select(0.0, 1.0, opa >= 0.5);").expect("aliased tail");
        let grain = SHADER.find("a = a * (1.0 - u.grain.w").expect("grain");
        assert!(tail > grain, "the aliased cut comes after every thinning");
    }
}

/// GM20: painting on the extended paper — the layer rect, its tile spans,
/// the readback, the memory and the composite window, all pure.
#[cfg(test)]
mod extpaper_tests {
    use super::*;
    use anim_core::fill::{FillOpts, FillRefusal, flood_fill_mask, flood_fill_mask_in};
    use anim_core::raster::{TILE_LEN, f32_to_f16_bits};

    const FRAME: (u32, u32) = (1920, 1080);
    const WIDE: IRect = IRect { x: -540, y: 0, w: 3000, h: 1080 };

    /// A tile whose texel (x, y) holds a value unique to the tile and texel.
    fn marked_tile(seed: u16) -> Arc<TileData> {
        let px = (0..TILE_LEN).map(|i| (i as u16).wrapping_mul(7).wrapping_add(seed) | 1).collect();
        Arc::new(TileData::from_vec(px))
    }

    /// The CPU mirror of `fill_texture` + `read_tiles`: upload every tile
    /// through `tile_span`, read the layer back through `layer_tiles`.
    fn mirror(tiles: &BTreeMap<TileCoord, Arc<TileData>>, rect: &LayerRect) -> Vec<(TileCoord, Arc<TileData>)> {
        let bpr = (rect.size.0 * BPP).div_ceil(256) as usize * 256;
        let mut layer = vec![0u8; bpr * rect.size.1 as usize];
        for (coord, tile) in tiles {
            let Some(s) = tile_span(*coord, rect) else { continue };
            let bytes = tile.as_bytes();
            for row in 0..s.size.1 as usize {
                let src = ((s.src.1 as usize + row) * TILE + s.src.0 as usize) * BPP as usize;
                let dst = (s.dst.1 as usize + row) * bpr + s.dst.0 as usize * BPP as usize;
                let n = s.size.0 as usize * BPP as usize;
                layer[dst..dst + n].copy_from_slice(&bytes[src..src + n]);
            }
        }
        layer_tiles(&layer, bpr, rect)
    }

    /// Reference tiles with opaque ink at the given frame pixels.
    fn ink(texels: &[(i32, i32)]) -> BTreeMap<TileCoord, Arc<TileData>> {
        let t = TILE as i32;
        let mut map: BTreeMap<TileCoord, Vec<u16>> = BTreeMap::new();
        for (x, y) in texels {
            let tile = map.entry((x.div_euclid(t), y.div_euclid(t))).or_insert_with(|| vec![0; TILE_LEN]);
            let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4) as usize;
            tile[i + 3] = f32_to_f16_bits(1.0);
        }
        map.into_iter().map(|(k, v)| (k, Arc::new(TileData::from_vec(v)))).collect()
    }

    fn ring(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        for x in x0..=x1 {
            v.extend([(x, y0), (x, y1)]);
        }
        for y in y0..=y1 {
            v.extend([(x0, y), (x1, y)]);
        }
        v
    }

    fn exact() -> FillOpts {
        FillOpts { threshold: 0.1, gap_px: 0, grow_px: 0, protect_ink: false }
    }

    #[test]
    fn extpaper_layer_rect_pins() {
        let f = layer_rect(IRect::frame(1920, 1080), FRAME, DEFAULT_MAX_DIM);
        assert_eq!((f.origin, f.size, f.capped), ((0, 0), (1920, 1080), false));
        assert_eq!(f, LayerRect::frame(1920, 1080));
        let w = layer_rect(WIDE, FRAME, DEFAULT_MAX_DIM);
        assert_eq!((w.origin, w.size, w.capped), ((-540, 0), (3000, 1080), false));
        // A paper wider than the GPU paints: an 8192 window holding the frame.
        let huge = IRect { x: -4040, y: 0, w: 10000, h: 1080 };
        let c = layer_rect(huge, FRAME, 8192);
        assert_eq!((c.size, c.capped), ((8192, 1080), true));
        assert!(c.origin.0 <= 0 && c.origin.0 + 8192 >= 1920, "holds the frame: {c:?}");
        assert!(c.origin.0 >= -4040 && c.origin.0 + 8192 <= 10000 - 4040, "inside the paper");
        // A paper off the frame paints the frame, as before.
        let off = IRect { x: 100, y: 0, w: 3000, h: 1080 };
        assert_eq!(layer_rect(off, FRAME, 8192), LayerRect::frame(1920, 1080));
    }

    #[test]
    fn extpaper_tile_span_and_cpu_mirror_round_trip() {
        let rect = LayerRect { origin: (-540, -64), size: (3000, 1208), capped: false, frame: FRAME };
        // Inside: the whole tile. (-8, -1) starts at frame (-512, -64).
        let s = tile_span((-8, -1), &rect).expect("on the layer");
        assert_eq!((s.src, s.dst, s.size), ((0, 0), (28, 0), (64, 64)));
        // Across the left edge (-540): only its right part.
        let s = tile_span((-9, 0), &rect).expect("partly on");
        assert_eq!((s.src, s.dst, s.size), ((36, 0), (0, 64), (28, 64)));
        // Off the layer: nothing.
        assert_eq!(tile_span((-10, 0), &rect), None);
        assert_eq!(tile_span((0, -2), &rect), None);
        // Upload -> readback returns the same tile map, negative tiles too.
        let tiles: BTreeMap<TileCoord, Arc<TileData>> =
            [(-8, -1), (0, 0), (5, 3), (-3, 10), (-1, -1)].into_iter().zip(1u16..).map(|(c, s)| (c, marked_tile(s))).collect();
        let back: BTreeMap<_, _> = mirror(&tiles, &rect).into_iter().collect();
        assert_eq!(back.keys().collect::<Vec<_>>(), tiles.keys().collect::<Vec<_>>());
        for (c, t) in &tiles {
            assert_eq!(back[c].rgba, t.rgba, "tile {c:?} round-trips bit for bit");
        }
        // A tile across the edge comes back as its on-layer part only.
        let edge: BTreeMap<_, _> = [((-9, 0), marked_tile(9))].into_iter().collect();
        let back = mirror(&edge, &rect);
        assert_eq!(back.len(), 1);
        let (_, t) = &back[0];
        assert_eq!(&t.rgba[..36 * 4], &[0u16; 36 * 4][..], "left of the paper: bare");
        assert_eq!(&t.rgba[36 * 4..64 * 4], &edge[&(-9, 0)].rgba[36 * 4..64 * 4]);
        // The frame window: layer texel (540, 64), frame-sized.
        assert_eq!(rect.frame_window(), (540, 64));
        assert_eq!(rect.frame, FRAME);
    }

    #[test]
    fn extpaper_flood_fill_over_the_paper() {
        let area = WIDE;
        // An inked ring left of the frame: the fill stays inside it.
        let r = ink(&ring(-400, 400, -200, 600));
        let m = flood_fill_mask_in(&r, (-300, 500), area, &exact(), None).expect("fills");
        assert!(m.get(-300, 500) && m.get(-399, 401) && !m.get(-450, 500) && !m.get(-400, 500));
        assert_eq!(m.count(), 199 * 199, "the ring's inside, beyond the frame");
        // A ring cut by the paper's left edge fills only up to the edge.
        let r = ink(&ring(-700, 400, -450, 600));
        let m = flood_fill_mask_in(&r, (-500, 500), area, &exact(), None).expect("fills");
        assert!(m.get(-540, 500) && !m.get(-541, 500), "the paper edge is a wall");
        assert_eq!(m.count(), (540 - 451 + 1) * 199);
        // Off the paper: refused.
        let e = flood_fill_mask_in(&r, (-600, 500), area, &exact(), None).err();
        assert_eq!(e, Some(FillRefusal::OffPaper));
        // Over the frame rect: the frame fill bit for bit (gap + grow on).
        let r = ink(&ring(100, 100, 300, 300));
        let opts = FillOpts::default();
        let a = flood_fill_mask(&r, (200, 200), 1920, 1080, &opts, None).expect("frame");
        let b = flood_fill_mask_in(&r, (200, 200), IRect::frame(1920, 1080), &opts, None).expect("in");
        assert_eq!((a.x0, a.y0, a.w, a.h), (b.x0, b.y0, b.w, b.h));
        assert!(a.bits == b.bits);
        assert_eq!(a.count(), 203 * 203, "the pinned pre-GM20 frame fill");
    }

    /// fnv1a, as the engine hashes tile bytes.
    fn fnv(bytes: impl Iterator<Item = u8>) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    #[test]
    fn extpaper_paper_equal_frame_is_the_old_layer() {
        let f = layer_rect(IRect::frame(1920, 1080), FRAME, DEFAULT_MAX_DIM);
        assert!(f.is_frame());
        // The pinned fill mask: the frame fill (gap + grow on) of an inked
        // ring, bit for bit as the pre-GM20 build (64df613) made it.
        let r = ink(&ring(100, 100, 300, 300));
        let m = flood_fill_mask(&r, (200, 200), 1920, 1080, &FillOpts::default(), None).expect("frame");
        let bits = fnv(m.bits.iter().map(|b| *b as u8));
        assert_eq!((m.x0, m.y0, m.w, m.h, m.count()), (0, 0, 1920, 1080, 41_209));
        assert_eq!(bits, 0x11c7_2ae7_6398_6440, "the mask's bits as 64df613 filled them");
        // The old upload rule: tiles at px/py >= 0 inside the frame, cut at
        // its right/bottom edge.
        let s = tile_span((29, 16), &f).expect("the corner tile");
        assert_eq!((s.src, s.dst, s.size), ((0, 0), (1856, 1024), (64, 56)));
        assert_eq!(tile_span((-1, 0), &f), None);
        assert_eq!(tile_span((30, 0), &f), None);
        assert_eq!(tile_span((0, 17), &f), None);
        // The pinned committed tile set: a readback of in-frame art merges
        // to itself, tile for tile.
        let tiles: BTreeMap<TileCoord, Arc<TileData>> =
            [(0, 0), (29, 15), (12, 7)].into_iter().zip(3u16..).map(|(c, s)| (c, marked_tile(s))).collect();
        let read = mirror(&tiles, &f);
        let merged = merge_outside(&tiles, read.clone(), &f);
        assert_eq!(merged.len(), 3);
        for ((a, ta), (b, tb)) in merged.iter().zip(&read) {
            assert_eq!((a, &ta.rgba), (b, &tb.rgba));
        }
        // A frame layer composites as before (no window).
        assert_eq!(composite_uniform(0.5, None), [0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn extpaper_capped_layer_keeps_the_art_it_cannot_see() {
        let huge = IRect { x: -4040, y: 0, w: 10000, h: 1080 };
        let c = layer_rect(huge, FRAME, 8192);
        let t = TILE as i32;
        let far = (c.origin.0 - 1).div_euclid(t) - 1; // a tile wholly left of the window
        let before: BTreeMap<TileCoord, Arc<TileData>> = [((far, 2), marked_tile(5))].into_iter().collect();
        let merged = merge_outside(&before, Vec::new(), &c);
        assert_eq!(merged.len(), 1, "the off-layer tile survives an empty readback");
        assert_eq!(merged[0].1.rgba, before[&(far, 2)].rgba);
    }

    #[test]
    fn extpaper_memory_and_cap_text() {
        let w = layer_rect(WIDE, FRAME, DEFAULT_MAX_DIM);
        // 6 layer targets of 3000×1080 + 3 onion slots (+ columns) of the frame, 8 B a texel.
        assert_eq!(layer_memory(&w, 0), 205_286_400);
        assert_eq!(layer_memory(&w, 1), 221_875_200);
        assert_eq!(layer_memory(&LayerRect::frame(1920, 1080), 0), 9 * 1920 * 1080 * 8);
        assert_eq!(memory_text(&w, 0), "painting uses 195.8 MiB of GPU memory (8 bytes a texel)");
        assert_eq!(cap_text(WIDE, &w, 8192), None);
        let huge = IRect { x: -4040, y: 0, w: 10000, h: 1080 };
        let c = layer_rect(huge, FRAME, 8192);
        assert_eq!(
            cap_text(huge, &c, 8192).as_deref(),
            Some(
                "this GPU paints at most 8192 px a side: painting covers 8192×1080 of the \
                 10000×1080 paper — x −3136 to 5056 of −4040 to 5960"
            )
        );
        // A paper that does not hold the frame paints the frame, and says so.
        let off = IRect { x: 100, y: 0, w: 3000, h: 1080 };
        assert_eq!(
            coverage_text(off, FRAME),
            Some("this paper does not cover the whole frame: painting stays on the frame")
        );
        assert_eq!(coverage_text(WIDE, FRAME), None);
        assert_eq!(coverage_text(IRect::frame(1920, 1080), FRAME), None);
    }

    #[test]
    fn extpaper_readback_bands_fit_the_device_buffer_limit() {
        // A valid paper whose whole-layer readback is past wgpu's default
        // max_buffer_size: it holds the frame and is not capped (both sides
        // within 8192), but one staging buffer would be 65536 × 4097 bytes.
        let paper = IRect { x: -3000, y: -1500, w: 8190, h: 4097 };
        assert!(paper.is_valid_paper());
        let r = layer_rect(paper, FRAME, DEFAULT_MAX_DIM);
        assert_eq!((r.origin, r.size, r.capped), ((-3000, -1500), (8190, 4097), false));
        const LIMIT: u64 = 256 << 20; // wgpu's Limits::default().max_buffer_size
        let bpr = padded_bpr(r.size.0) as u64;
        assert_eq!(bpr * r.size.1 as u64, 268_500_992, "past the limit as one buffer");
        let bands = readback_bands(&r, bpr, LIMIT);
        assert!(bands.len() > 1, "{bands:?}");
        let mut y = 0;
        for &(y0, rows) in &bands {
            assert_eq!(y0, y, "bands run on without a gap");
            assert!(rows > 0 && bpr * rows as u64 <= LIMIT, "{rows} rows fit one buffer");
            y += rows;
            if y < r.size.1 {
                assert_eq!((r.origin.1 + y as i32).rem_euclid(TILE as i32), 0, "cut on a tile row");
            }
        }
        assert_eq!(y, r.size.1, "every row read back");
        // A layer that fits reads back in one band, as before.
        let w = layer_rect(WIDE, FRAME, DEFAULT_MAX_DIM);
        assert_eq!(readback_bands(&w, padded_bpr(3000) as u64, LIMIT), vec![(0, 1080)]);
    }

    #[test]
    fn extpaper_banded_gather_equals_the_one_band_readback() {
        // A small layer off the tile grid, its readback cut into bands on
        // tile rows and (a tiny limit) through the middle of tile rows.
        let rect = LayerRect { origin: (-100, -36), size: (230, 200), capped: false, frame: (128, 128) };
        let bpr = padded_bpr(rect.size.0) as usize;
        let mut layer = vec![0u8; bpr * rect.size.1 as usize];
        for (i, b) in layer.iter_mut().enumerate() {
            let (row, col) = (i / bpr, i % bpr);
            if col < rect.size.0 as usize * BPP as usize && (row * 7 + col / 8) % 5 != 0 {
                *b = (i % 251) as u8 | 1;
            }
        }
        let whole = layer_tiles(&layer, bpr, &rect);
        assert!(!whole.is_empty());
        for limit in [bpr as u64 * 100, bpr as u64 * 10, bpr as u64] {
            let mut gather = TileGather::new(&rect);
            for (y0, rows) in readback_bands(&rect, bpr as u64, limit) {
                let band = &layer[y0 as usize * bpr..(y0 + rows) as usize * bpr];
                gather.band(band, bpr, y0, rows);
            }
            let banded = gather.finish();
            assert_eq!(banded.len(), whole.len(), "limit {limit}");
            for ((a, ta), (b, tb)) in banded.iter().zip(&whole) {
                assert_eq!((a, &ta.rgba), (b, &tb.rgba), "limit {limit}");
            }
        }
    }

    #[test]
    fn extpaper_room_limit_is_one_side_for_every_peer() {
        // Every peer paints at most DEFAULT_MAX_DIM a side, whatever its
        // GPU allows, so a capped paper's window (and a replay's rect) is the
        // same on every peer.
        assert_eq!(room_limit(16384), DEFAULT_MAX_DIM);
        assert_eq!(room_limit(8192), 8192);
        assert_eq!(room_limit(4096), 4096);
        let huge = IRect { x: -4040, y: 0, w: 10000, h: 1080 };
        assert_eq!(layer_rect(huge, FRAME, room_limit(16384)), layer_rect(huge, FRAME, room_limit(8192)));
    }

    #[test]
    fn extpaper_shaders_take_the_origin_and_the_window() {
        for (name, src) in [("SHADER", SHADER), ("COMPOSITE_SHADER", COMPOSITE_SHADER)] {
            let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{name}: {e}"));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{name} validates: {e:?}"));
        }
        assert!(SHADER.contains("let texel = center + local - u.origin;"));
        assert!(COMPOSITE_SHADER.contains("textureLoad(wet_tex, at, 0)"));
        assert_eq!(composite_uniform(1.0, Some((540, 64))), [1.0, 0.0, 0.0, 0.0, 540.0, 64.0, 1.0, 0.0]);
        assert_eq!(std::mem::size_of::<Uniforms>(), 48, "the dab uniform keeps its layout");
    }
}
