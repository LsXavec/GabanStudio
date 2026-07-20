//! Golden tests for the CPU graph renderer (`export::render_graph_frame`) —
//! the reference the GPU graph compositor is verified against. These pin the
//! pixel semantics defined in research/gpu-graph-compositor-design.md:
//! Solid colour law (sRGB→linear, matches the brush path), all four blend
//! formulas, the Transform law (centre-origin, y-down clockwise, integer
//! translates exact), hold resolution, and graph≡column equivalence.

use std::sync::Arc;

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::export::{render_frame, render_graph_frame, srgb_to_linear};
use anim_core::graph::{BlendMode, NodeKind};
use anim_core::ids::*;
use anim_core::model::CelLayer;
use anim_core::raster::{TILE_LEN, TileData, f32_to_f16_bits};
use anim_core::xsheet::Exposure;

const W: u32 = 64;
const H: u32 = 64;

/// One-cut rig: a column with one single-layer drawing exposed at frame 0.
struct Rig {
    engine: Engine,
    at: CutRef,
    col: ColumnId,
    d: DrawingId,
    layer: LayerId,
}

fn rig() -> Rig {
    let mut engine = Engine::new("graph render tests");
    let scene = engine.add_scene("SC");
    let cut = engine.add_cut(scene, "cut", 24).unwrap();
    let at = CutRef { scene, cut };
    let col = engine.add_column(at, "A").unwrap();
    let d = engine.alloc_drawing_id();
    let layer = engine.alloc_layer_id();
    engine
        .apply(
            "rig",
            vec![
                Command::AddDrawing {
                    at,
                    id: d,
                    index: 0,
                    name: "art".into(),
                    strokes: vec![],
                    layers: vec![CelLayer::new(layer, "paint")],
                },
                Command::SetCell {
                    at,
                    column: col,
                    frame: 0,
                    key: Some(Exposure::Drawing(d)),
                },
            ],
        )
        .unwrap();
    Rig { engine, at, col, d, layer }
}

impl Rig {
    fn cut(&self) -> &anim_core::model::Cut {
        self.engine.project.cut(self.at.scene, self.at.cut).unwrap()
    }

    fn node(&mut self, kind: NodeKind) -> NodeId {
        let id = self.engine.alloc_node_id();
        let at = self.at;
        self.engine
            .apply("add node", vec![Command::AddNode { at, id, kind }])
            .unwrap();
        id
    }

    fn connect(&mut self, from: NodeId, to: NodeId, pin: u16) {
        let at = self.at;
        self.engine
            .apply(
                "connect",
                vec![Command::Connect { at, from, from_pin: 0, to, to_pin: pin }],
            )
            .unwrap();
    }

    fn set_output(&mut self, node: NodeId) {
        let at = self.at;
        self.engine
            .apply("output", vec![Command::SetOutput { at, node: Some(node) }])
            .unwrap();
    }

    /// Paint a single fully-opaque premultiplied texel at (x, y) in tile (0,0).
    fn paint_texel(&mut self, x: usize, y: usize, rgba: [f32; 4]) {
        let mut px = vec![0u16; TILE_LEN];
        let base = (y * 64 + x) * 4;
        for c in 0..4 {
            px[base + c] = f32_to_f16_bits(rgba[c]);
        }
        let at = self.at;
        let (id, layer) = (self.d, self.layer);
        self.engine
            .apply(
                "paint",
                vec![Command::PaintTiles {
                    at,
                    id,
                    layer,
                    diff: vec![((0, 0), None, Some(Arc::new(TileData::from_vec(px))))],
                }],
            )
            .unwrap();
    }
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

fn opaque_px_count(buf: &[u8]) -> usize {
    buf.chunks_exact(4).filter(|p| p[3] > 0).count()
}

/// Straight-RGBA8 a Solid node's colour the same way the renderer must:
/// sRGB→linear each channel (the value the premult pipeline carries), then
/// the finisher's unpremultiply round-trips it back out.
fn solid_expected(rgba: [u8; 4]) -> [u8; 4] {
    let a = rgba[3] as f32 / 255.0;
    let mut out = [0u8; 4];
    for c in 0..3 {
        out[c] = (srgb_to_linear(rgba[c]).clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    out[3] = (a * 255.0).round() as u8;
    out
}

#[test]
fn solid_fills_frame_with_linearized_color() {
    let mut r = rig();
    let s = r.node(NodeKind::Solid { rgba: [255, 0, 0, 255] });
    let out = r.node(NodeKind::Output);
    r.connect(s, out, 0);
    r.set_output(out);
    let img = render_graph_frame(r.cut(), 0, W, H).unwrap();
    // Pure red is exact through the EOTF: lin(255)=1.0.
    assert_eq!(px(&img, 0, 0), [255, 0, 0, 255]);
    assert_eq!(px(&img, 63, 63), [255, 0, 0, 255]);
    assert_eq!(opaque_px_count(&img), (W * H) as usize);

    // A mid-tone with partial alpha survives the premult round-trip.
    let mut r2 = rig();
    let c = [128, 64, 32, 128];
    let s2 = r2.node(NodeKind::Solid { rgba: c });
    let out2 = r2.node(NodeKind::Output);
    r2.connect(s2, out2, 0);
    r2.set_output(out2);
    let img2 = render_graph_frame(r2.cut(), 0, W, H).unwrap();
    assert_eq!(px(&img2, 10, 10), solid_expected(c));
}

#[test]
fn blend_modes_match_pinned_formulas() {
    // Iterate an OPAQUE and a TRANSLUCENT bottom: with da = 1 the Multiply
    // term s·(1−da) and the alpha law both degenerate (a dropped term or a
    // wrong alpha law like max(sa,da) passes) — the translucent case makes
    // every term of every formula load-bearing.
    let top = [192u8, 96, 48, 128];
    for bot in [[64u8, 128, 192, 255], [64, 128, 192, 180]] {
        let pa: Vec<f32> = {
            let a = bot[3] as f32 / 255.0;
            (0..3).map(|c| srgb_to_linear(bot[c]) * a).chain([a]).collect()
        };
        let pb: Vec<f32> = {
            let a = top[3] as f32 / 255.0;
            (0..3).map(|c| srgb_to_linear(top[c]) * a).chain([a]).collect()
        };
        for mode in [
            BlendMode::Normal,
            BlendMode::Multiply,
            BlendMode::Add,
            BlendMode::Screen,
        ] {
            let mut r = rig();
            let a = r.node(NodeKind::Solid { rgba: bot });
            let b = r.node(NodeKind::Solid { rgba: top });
            let blend = r.node(NodeKind::Blend { mode });
            let out = r.node(NodeKind::Output);
            r.connect(a, blend, 0);
            r.connect(b, blend, 1);
            r.connect(blend, out, 0);
            r.set_output(out);
            let img = render_graph_frame(r.cut(), 0, W, H).unwrap();

            // Hand-applied formula (premultiplied), then the finisher round-trip.
            let (sa, da) = (pb[3], pa[3]);
            let oa = sa + da * (1.0 - sa);
            let mut expect = [0u8; 4];
            for c in 0..3 {
                let v = match mode {
                    BlendMode::Normal => pb[c] + pa[c] * (1.0 - sa),
                    BlendMode::Multiply => {
                        pb[c] * pa[c] + pb[c] * (1.0 - da) + pa[c] * (1.0 - sa)
                    }
                    BlendMode::Add => pb[c] + pa[c],
                    BlendMode::Screen => pb[c] + pa[c] - pb[c] * pa[c],
                };
                expect[c] = ((v / oa).clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            expect[3] = (oa.clamp(0.0, 1.0) * 255.0).round() as u8;
            assert_eq!(px(&img, 5, 5), expect, "mode {mode} bot alpha {}", bot[3]);
        }
    }
}

#[test]
fn trivial_graph_matches_column_export_exactly() {
    let mut r = rig();
    r.paint_texel(10, 20, [0.5, 0.25, 0.125, 0.5]);
    r.paint_texel(40, 8, [0.0, 0.0, 0.0, 1.0]);
    let src = r.node(NodeKind::DrawingSource { column: r.col });
    let out = r.node(NodeKind::Output);
    r.connect(src, out, 0);
    r.set_output(out);
    let graph = render_graph_frame(r.cut(), 0, W, H).unwrap();
    let column = render_frame(r.cut(), 0, W, H);
    assert_eq!(graph, column, "DrawingSource→Output must equal the column path");
}

#[test]
fn transform_integer_translate_is_exact() {
    let mut r = rig();
    r.paint_texel(10, 20, [1.0, 1.0, 1.0, 1.0]);
    let src = r.node(NodeKind::DrawingSource { column: r.col });
    let xf = r.node(NodeKind::Transform {
        translate: (5.0, 3.0),
        scale: 1.0,
        rotate_deg: 0.0,
        binds: Default::default(),
    });
    let out = r.node(NodeKind::Output);
    r.connect(src, xf, 0);
    r.connect(xf, out, 0);
    r.set_output(out);
    let img = render_graph_frame(r.cut(), 0, W, H).unwrap();
    assert_eq!(px(&img, 15, 23), [255, 255, 255, 255], "texel moved by (+5,+3)");
    assert_eq!(px(&img, 10, 20)[3], 0, "original position now transparent");
    assert_eq!(opaque_px_count(&img), 1, "no bilinear bleed on integer moves");
}

#[test]
fn transform_identity_is_exact() {
    let mut r = rig();
    r.paint_texel(31, 7, [0.5, 0.5, 0.0, 0.5]);
    let src = r.node(NodeKind::DrawingSource { column: r.col });
    let xf = r.node(NodeKind::Transform {
        translate: (0.0, 0.0),
        scale: 1.0,
        rotate_deg: 0.0,
        binds: Default::default(),
    });
    let out = r.node(NodeKind::Output);
    r.connect(src, xf, 0);
    r.connect(xf, out, 0);
    r.set_output(out);
    let with_xf = render_graph_frame(r.cut(), 0, W, H).unwrap();

    let mut r2 = rig();
    r2.paint_texel(31, 7, [0.5, 0.5, 0.0, 0.5]);
    let src2 = r2.node(NodeKind::DrawingSource { column: r2.col });
    let out2 = r2.node(NodeKind::Output);
    r2.connect(src2, out2, 0);
    r2.set_output(out2);
    let plain = render_graph_frame(r2.cut(), 0, W, H).unwrap();
    assert_eq!(with_xf, plain);
}

#[test]
fn transform_rotate90_pins_centre_origin_clockwise() {
    // Texel (40, 32): centre (40.5, 32.5), offset (+8.5, +0.5) from C=(32,32).
    // 90° clockwise (y-down): (x, y) → (−y, x) ⇒ (−0.5, 8.5) ⇒ centre
    // (31.5, 40.5) = exactly pixel (31, 40) — a single-pixel landing.
    let mut r = rig();
    r.paint_texel(40, 32, [1.0, 0.0, 0.0, 1.0]);
    let src = r.node(NodeKind::DrawingSource { column: r.col });
    let xf = r.node(NodeKind::Transform {
        translate: (0.0, 0.0),
        scale: 1.0,
        rotate_deg: 90.0,
        binds: Default::default(),
    });
    let out = r.node(NodeKind::Output);
    r.connect(src, xf, 0);
    r.connect(xf, out, 0);
    r.set_output(out);
    let img = render_graph_frame(r.cut(), 0, W, H).unwrap();
    assert_eq!(px(&img, 31, 40), [255, 0, 0, 255]);
    assert_eq!(opaque_px_count(&img), 1);
}

#[test]
fn transform_scale_about_centre_and_degenerate_zero() {
    // Texel (32, 32): centre (32.5, 32.5) = offset (+0.5, +0.5) from C.
    // Scale 3 forward-maps it to (1.5, 1.5) ⇒ centre (33.5, 33.5) = exactly
    // pixel (33, 33), which inverse-maps to a dead-centre full-weight sample.
    // Kills both `inv_s = scale` (wrong inverse) and scale-about-origin bugs
    // (either displaces this landing). The scaled texel grows a bilinear
    // skirt — bounded, and absent far away.
    let mut r = rig();
    r.paint_texel(32, 32, [1.0, 0.0, 0.0, 1.0]);
    let src = r.node(NodeKind::DrawingSource { column: r.col });
    let xf = r.node(NodeKind::Transform {
        translate: (0.0, 0.0),
        scale: 3.0,
        rotate_deg: 0.0,
        binds: Default::default(),
    });
    let out = r.node(NodeKind::Output);
    r.connect(src, xf, 0);
    r.connect(xf, out, 0);
    r.set_output(out);
    let img = render_graph_frame(r.cut(), 0, W, H).unwrap();
    assert_eq!(px(&img, 33, 33), [255, 0, 0, 255], "scaled texel centre landing");
    assert_eq!(px(&img, 20, 20)[3], 0, "far field stays empty");
    let n = opaque_px_count(&img);
    assert!((2..=25).contains(&n), "3× texel = small bilinear blob, got {n}");

    // Degenerate law: |s| < 1e-6 renders fully transparent.
    let mut r2 = rig();
    r2.paint_texel(32, 32, [1.0, 0.0, 0.0, 1.0]);
    let src2 = r2.node(NodeKind::DrawingSource { column: r2.col });
    let xf2 = r2.node(NodeKind::Transform {
        translate: (0.0, 0.0),
        scale: 0.0,
        rotate_deg: 0.0,
        binds: Default::default(),
    });
    let out2 = r2.node(NodeKind::Output);
    r2.connect(src2, xf2, 0);
    r2.connect(xf2, out2, 0);
    r2.set_output(out2);
    let img2 = render_graph_frame(r2.cut(), 0, W, H).unwrap();
    assert_eq!(opaque_px_count(&img2), 0);
}

#[test]
fn transform_fractional_translate_pins_bilinear_edge_law() {
    // Half-pixel translate: the texel's coverage splits 50/50 between two
    // pixels — premult 0.5 each ⇒ straight [255,255,255,128]. Pins the
    // bilinear weights AND the fade-to-transparent edge law (the CPU/export
    // truth; the GPU's ClampToEdge fringe is the documented deviation).
    let mut r = rig();
    r.paint_texel(10, 20, [1.0, 1.0, 1.0, 1.0]);
    let src = r.node(NodeKind::DrawingSource { column: r.col });
    let xf = r.node(NodeKind::Transform {
        translate: (0.5, 0.0),
        scale: 1.0,
        rotate_deg: 0.0,
        binds: Default::default(),
    });
    let out = r.node(NodeKind::Output);
    r.connect(src, xf, 0);
    r.connect(xf, out, 0);
    r.set_output(out);
    let img = render_graph_frame(r.cut(), 0, W, H).unwrap();
    assert_eq!(px(&img, 10, 20), [255, 255, 255, 128]);
    assert_eq!(px(&img, 11, 20), [255, 255, 255, 128]);
    assert_eq!(opaque_px_count(&img), 2);
}

#[test]
fn solid_eotf_pinned_to_exact_bytes() {
    // Independent pin of the sRGB→linear EOTF: literal expected bytes, NOT
    // computed via srgb_to_linear (which would pass vacuously if the EOTF
    // drifted). lin(128)=0.21586→55, lin(64)=0.05127→13, lin(32)=0.01444→4.
    let mut r = rig();
    let s = r.node(NodeKind::Solid { rgba: [128, 64, 32, 255] });
    let out = r.node(NodeKind::Output);
    r.connect(s, out, 0);
    r.set_output(out);
    let img = render_graph_frame(r.cut(), 0, W, H).unwrap();
    assert_eq!(px(&img, 0, 0), [55, 13, 4, 255]);
}

#[test]
fn missing_column_renders_transparent_and_still_evaluates() {
    // Deleting a column a DrawingSource still reads is TOLERATED everywhere:
    // the evaluator yields a sentinel value (no error — composite view and
    // export must agree), and the renderer draws transparent.
    let mut r = rig();
    r.paint_texel(5, 5, [1.0, 0.0, 0.0, 1.0]);
    let src = r.node(NodeKind::DrawingSource { column: r.col });
    let out = r.node(NodeKind::Output);
    r.connect(src, out, 0);
    r.set_output(out);
    let at = r.at;
    let col = r.col;
    r.engine.remove_column(at, col);
    let v = r.engine.eval(at.scene, at.cut, 0).expect("eval tolerates a dead column");
    assert!(v.recipe().contains("missing-column"), "recipe: {}", v.recipe());
    let img = render_graph_frame(r.cut(), 0, W, H).unwrap();
    assert_eq!(opaque_px_count(&img), 0);
}

#[test]
fn holds_resolve_through_the_graph() {
    let mut r = rig();
    r.paint_texel(3, 3, [0.0, 0.5, 0.0, 0.5]);
    let src = r.node(NodeKind::DrawingSource { column: r.col });
    let out = r.node(NodeKind::Output);
    r.connect(src, out, 0);
    r.set_output(out);
    let f0 = render_graph_frame(r.cut(), 0, W, H).unwrap();
    let f5 = render_graph_frame(r.cut(), 5, W, H).unwrap();
    assert_eq!(f0, f5, "frame 5 holds the frame-0 drawing");
    assert!(opaque_px_count(&f0) > 0);
}

#[test]
fn no_wired_output_returns_none() {
    let mut r = rig();
    let s = r.node(NodeKind::Solid { rgba: [255, 255, 255, 255] });
    let out = r.node(NodeKind::Output);
    r.connect(s, out, 0);
    // Output node exists but graph.output was never set.
    assert!(render_graph_frame(r.cut(), 0, W, H).is_none());
}

#[test]
fn unconnected_inputs_render_transparent() {
    // Blend with only the TOP input wired = top over transparent = top alone.
    let c = [200u8, 40, 90, 180];
    let mut r = rig();
    let top = r.node(NodeKind::Solid { rgba: c });
    let blend = r.node(NodeKind::Blend { mode: BlendMode::Normal });
    let out = r.node(NodeKind::Output);
    r.connect(top, blend, 1);
    r.connect(blend, out, 0);
    r.set_output(out);
    let img = render_graph_frame(r.cut(), 0, W, H).unwrap();
    assert_eq!(px(&img, 0, 0), solid_expected(c));

    // A bare Output with nothing wired renders fully transparent.
    let mut r2 = rig();
    let out2 = r2.node(NodeKind::Output);
    r2.set_output(out2);
    let img2 = render_graph_frame(r2.cut(), 0, W, H).unwrap();
    assert_eq!(opaque_px_count(&img2), 0);
}
