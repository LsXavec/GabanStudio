//! GM13 the satsuei look: role passes and the trace-line absorb are the
//! flatten's arithmetic on chosen layers, every new FX is the identity at
//! its neutral setting and matches its formula at one other, the .cube
//! reader reads and refuses, the motion blur's kernel is pinned, the
//! palette map is exact, and the satsuei template builds its pinned chain
//! in one undo step and renders on the CPU. Headless; files only in temp.

use std::sync::Arc;

use anim_core::Engine;
use anim_core::color::ColorLaw;
use anim_core::command::{Command, CutRef};
use anim_core::export::graph_composite;
use anim_core::export::graph_composite_pixel_law;
use anim_core::fx::{FxBinds, FxCtx, FxKind, FxParams, FxValue, FxValues, Image, blur};
use anim_core::graph::{self, NodeKind};
use anim_core::ids::*;
use anim_core::look::{
    self, ABSORB_RADIUS, PAINT_PASS, Tiles, absorb_trace_lines, apply_lut, directional_blur,
    effect_with, filter_roles, palette_map, parse_cube, satsuei_template,
};
use anim_core::model::{CelLayer, Cut, Drawing, LayerRole};
use anim_core::raster::{TILE, TILE_LEN, TileData, f16_bits_to_f32, f32_to_f16_bits};
use anim_core::xsheet::{ColumnDef, ColumnKind, Exposure};

const RED: [f32; 4] = [0.8, 0.1, 0.1, 1.0];
const GREEN: [f32; 4] = [0.1, 0.7, 0.2, 1.0];
const BLUE: [f32; 4] = [0.1, 0.2, 0.9, 1.0];
const SHADE: [f32; 4] = [0.3, 0.2, 0.5, 1.0];
const SHINE: [f32; 4] = [1.0, 0.95, 0.8, 1.0];

// ---- Helpers ----------------------------------------------------------------------

/// One tile painted by `f` (premultiplied; None = bare).
fn tile(f: impl Fn(usize, usize) -> Option<[f32; 4]>) -> Arc<TileData> {
    let mut px = vec![0u16; TILE_LEN];
    for y in 0..TILE {
        for x in 0..TILE {
            if let Some(p) = f(x, y) {
                for c in 0..4 {
                    px[(y * TILE + x) * 4 + c] = f32_to_f16_bits(p[c]);
                }
            }
        }
    }
    Arc::new(TileData::from_vec(px))
}

/// A tile with `rgba` in the box x0..x1 × y0..y1.
fn block(x0: usize, x1: usize, y0: usize, y1: usize, rgba: [f32; 4]) -> Arc<TileData> {
    tile(|x, y| (x >= x0 && x < x1 && y >= y0 && y < y1).then_some(rgba))
}

fn layer(id: u64, role: LayerRole, t: Arc<TileData>) -> CelLayer {
    let mut l = CelLayer::with_role(LayerId(id), role.key(), role);
    l.raster.tiles.insert((0, 0), t);
    l
}

fn drawing(layers: Vec<CelLayer>) -> Drawing {
    let mut d = Drawing::new(DrawingId(1), "cel");
    d.layers = layers;
    d
}

/// A flattened pixel, decoded (0 where there is no tile).
fn at(tiles: &Tiles, x: usize, y: usize) -> [f32; 4] {
    match tiles.get(&(0, 0)) {
        Some(t) => [0, 1, 2, 3].map(|c| f16_bits_to_f32(t.rgba[(y * TILE + x) * 4 + c])),
        None => [0.0; 4],
    }
}

/// `rgba` as it reads back from a tile (f16).
fn stored(rgba: [f32; 4]) -> [f32; 4] {
    rgba.map(|v| f16_bits_to_f32(f32_to_f16_bits(v)))
}

fn values(kind: FxKind, set: &[(&str, FxValue)]) -> FxValues {
    let mut params = FxParams::defaults(kind);
    for (k, v) in set {
        params.set(k, v.clone());
    }
    FxValues::resolve(kind, &params, &FxBinds::default(), |_| None)
}

fn run_at(kind: FxKind, set: &[(&str, FxValue)], inputs: &[Option<&Image>], frame: u32) -> Image {
    let src = inputs[0].unwrap();
    let ctx = FxCtx { frame, blobs: None };
    kind.apply_in(&values(kind, set), ColorLaw::Srgb, inputs, src.w, src.h, ctx)
}

fn run(kind: FxKind, set: &[(&str, FxValue)], src: &Image) -> Image {
    run_at(kind, set, &[Some(src)], 0)
}

/// A frame of varied colour and coverage (premultiplied).
fn swatch(w: u32, h: u32) -> Image {
    let mut img = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let a = ((x + y) % 4 + 1) as f32 / 4.0;
            let s = [x as f32 / w as f32, y as f32 / h as f32, 0.5];
            img.set_pixel(x, y, [s[0] * a, s[1] * a, s[2] * a, a]);
        }
    }
    img
}

fn solid(w: u32, h: u32, p: [f32; 4]) -> Image {
    Image::from_px(w, h, p.repeat((w * h) as usize))
}

fn f(x: f32) -> FxValue {
    FxValue::Float(x)
}

fn cut(e: &Engine, at: CutRef) -> &Cut {
    e.project.cut(at.scene, at.cut).unwrap()
}

/// The graph under `id`, spelled: kinds, passes and pins (`-` unwired).
fn spell(c: &Cut, id: NodeId) -> String {
    let n = c.graph.node(id).unwrap();
    let name = match &n.kind {
        NodeKind::DrawingSource {
            column,
            roles,
            absorb_trace,
        } => {
            let col = c.xsheet.column(*column).unwrap().name.clone();
            match roles {
                None => col,
                Some(r) => format!(
                    "{col}[{}{}]",
                    r.iter().map(|r| r.key()).collect::<Vec<_>>().join(","),
                    if *absorb_trace { ";absorb" } else { "" }
                ),
            }
        }
        NodeKind::Blend { mode } => format!("{mode}"),
        k => k.name().to_string(),
    };
    let ins: Vec<String> = n
        .inputs
        .iter()
        .map(|i| match i {
            Some((src, _)) => spell(c, *src),
            None => "-".into(),
        })
        .collect();
    if ins.is_empty() { name } else { format!("{name}({})", ins.join(", ")) }
}

/// A cut with a BG column (one Color layer) and a cel column A — paint, a
/// shadow, a highlight, a shadow trace line and the line — each showing
/// one drawing from frame 0, and an auto-comp style graph (A → Output).
fn satsuei_rig() -> (Engine, CutRef) {
    let mut e = Engine::new("look tests");
    let scene = e.add_scene("s01");
    let c = e.add_cut(scene, "c001", 2).unwrap();
    let at = CutRef { scene, cut: c };
    let bg = e.add_column(at, "BG").unwrap();
    let a = e.add_column(at, "A").unwrap();
    let mut cmds = vec![Command::SetColumnDef {
        at,
        column: bg,
        def: ColumnDef {
            kind: ColumnKind::Bg,
            hidden: false,
            locked: false,
        },
    }];
    let cels: [(ColumnId, Vec<(LayerRole, Arc<TileData>)>); 2] = [
        (bg, vec![(LayerRole::Color, block(0, 64, 0, 64, [0.4, 0.6, 0.9, 1.0]))]),
        (
            a,
            vec![
                (LayerRole::Color, block(16, 40, 8, 28, [0.9, 0.7, 0.6, 1.0])),
                (LayerRole::Shadow, block(16, 28, 8, 28, SHADE)),
                (LayerRole::Highlight, block(32, 38, 10, 14, SHINE)),
                (LayerRole::ShadowLine, block(28, 29, 8, 28, BLUE)),
                (LayerRole::Line, block(15, 16, 8, 28, [0.05, 0.05, 0.05, 1.0])),
            ],
        ),
    ];
    let mut paint = Vec::new();
    for (index, (column, roles)) in cels.into_iter().enumerate() {
        let d = e.alloc_drawing_id();
        let mut layers = Vec::new();
        for (role, t) in roles {
            let id = e.alloc_layer_id();
            layers.push(CelLayer::with_role(id, role.key(), role));
            paint.push(Command::PaintTiles {
                at,
                id: d,
                layer: id,
                diff: vec![((0, 0), None, Some(t))],
            });
        }
        cmds.push(Command::AddDrawing {
            at,
            id: d,
            index,
            name: format!("cel {index}"),
            strokes: vec![],
            layers,
        });
        cmds.push(Command::SetCell {
            at,
            column,
            frame: 0,
            key: Some(Exposure::Drawing(d)),
        });
    }
    cmds.extend(paint);
    let (src, out) = (e.alloc_node_id(), e.alloc_node_id());
    cmds.extend([
        Command::AddNode { at, id: src, kind: NodeKind::source(a) },
        Command::AddNode { at, id: out, kind: NodeKind::Output },
        Command::Connect { at, from: src, from_pin: 0, to: out, to_pin: 0 },
        Command::SetOutput { at, node: Some(out) },
    ]);
    e.apply("rig", cmds).unwrap();
    (e, at)
}

/// Solid → Effect(`fx`) → Output; returns the engine, the cut and the
/// effect node.
fn fx_rig(fx: FxKind) -> (Engine, CutRef, NodeId) {
    let mut e = Engine::new("look fx");
    let scene = e.add_scene("s01");
    let c = e.add_cut(scene, "c001", 4).unwrap();
    let at = CutRef { scene, cut: c };
    let (solid, node, out) = (e.alloc_node_id(), e.alloc_node_id(), e.alloc_node_id());
    e.apply(
        "rig",
        vec![
            Command::AddNode { at, id: solid, kind: NodeKind::Solid { rgba: [64, 128, 192, 255] } },
            Command::AddNode { at, id: node, kind: NodeKind::effect(fx) },
            Command::AddNode { at, id: out, kind: NodeKind::Output },
            Command::Connect { at, from: solid, from_pin: 0, to: node, to_pin: 0 },
            Command::Connect { at, from: node, from_pin: 0, to: out, to_pin: 0 },
            Command::SetOutput { at, node: Some(out) },
        ],
    )
    .unwrap();
    (e, at, node)
}

fn identity_cube() -> String {
    let mut s = String::from("TITLE \"identity\"\n# a comment\nLUT_3D_SIZE 2\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                s.push_str(&format!("{r} {g} {b}\n"));
            }
        }
    }
    s
}

fn inverting_cube() -> String {
    let mut s = String::from("LUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                s.push_str(&format!("{}.0 {}.0 {}.0\n", 1 - r, 1 - g, 1 - b));
            }
        }
    }
    s
}

// ---- Role passes and the absorb ---------------------------------------------------

#[test]
fn a_shadow_role_filter_is_the_flatten_of_only_the_shadow_layers() {
    let mut half = layer(3, LayerRole::Shadow, block(4, 20, 4, 12, SHADE));
    half.props.opacity = 0.5;
    let d = drawing(vec![
        layer(1, LayerRole::Color, block(0, 32, 0, 32, RED)),
        layer(2, LayerRole::Shadow, block(0, 10, 0, 16, SHADE)),
        half,
        layer(4, LayerRole::Line, block(8, 9, 0, 32, BLUE)),
    ]);
    let only = drawing(d.layers.iter().filter(|l| l.props.role == LayerRole::Shadow).cloned().collect());
    let pass = filter_roles(&d, &[LayerRole::Shadow]);
    assert_eq!(pass, only.flatten(), "pixel-exact: the same tiles");
    assert_eq!(look::source_tiles(&d, Some(&[LayerRole::Shadow]), false), pass);
    assert_eq!(look::source_tiles(&d, None, false), d.flatten(), "no filter: the flatten");
    assert_eq!(at(&pass, 20, 20), [0.0; 4], "the paint is left out");
}

#[test]
fn absorb_leaves_no_blue_and_takes_the_adjacent_paint() {
    let d = drawing(vec![
        layer(1, LayerRole::Color, tile(|x, y| (y < 16 && x != 10 && x != 11 && x < 24).then_some(if x < 10 { RED } else { GREEN }))),
        layer(2, LayerRole::ShadowLine, block(10, 12, 0, 16, BLUE)),
        layer(3, LayerRole::Line, block(30, 31, 0, 16, BLUE)),
    ]);
    let out = absorb_trace_lines(&d, ABSORB_RADIUS);
    let trace = out.layers[1].raster.tiles.clone();
    for y in 0..16 {
        assert_eq!(at(&trace, 10, y), stored(RED), "x=10 takes the paint beside it");
        assert_eq!(at(&trace, 11, y), stored(GREEN), "x=11 takes the paint beside it");
    }
    assert_eq!(out.layers[2], d.layers[2], "Line-role pixels are untouched");
    let flat = out.flatten();
    for y in 0..TILE {
        for x in 0..TILE {
            if x != 30 {
                assert_ne!(at(&flat, x, y), stored(BLUE), "no blue left at ({x}, {y})");
            }
        }
    }
    assert_eq!(at(&flat, 30, 3), stored(BLUE), "the line stays");
}

#[test]
fn absorb_takes_the_paint_under_a_line_keeps_its_coverage_and_lines_out_of_reach() {
    let half_blue = [0.05, 0.1, 0.45, 0.5];
    let d = drawing(vec![
        layer(1, LayerRole::Color, block(0, 8, 0, 8, RED)),
        layer(2, LayerRole::Shadow, block(4, 8, 0, 8, GREEN)),
        layer(3, LayerRole::HighlightLine, tile(|x, y| (y < 8 && (x == 2 || x == 20)).then_some(half_blue))),
    ]);
    let out = absorb_trace_lines(&d, ABSORB_RADIUS);
    let line = out.layers[2].raster.tiles.clone();
    let red = stored(RED);
    let want = [red[0] * 0.5, red[1] * 0.5, red[2] * 0.5, 0.5].map(|v| f16_bits_to_f32(f32_to_f16_bits(v)));
    assert_eq!(at(&line, 2, 3), want, "the paint under it, at the line's own coverage");
    assert_eq!(at(&line, 20, 3), stored(half_blue), "12 px from paint: kept");
    // The pass with the absorb is the flatten of the recoloured cel.
    assert_eq!(look::source_tiles(&d, Some(PAINT_PASS), true), filter_roles(&out, PAINT_PASS));
}

#[test]
fn old_drawing_source_json_loads_and_a_plain_source_writes_as_before() {
    let old = r#"{"DrawingSource":{"column":5}}"#;
    let k: NodeKind = serde_json::from_str(old).unwrap();
    assert_eq!(k, NodeKind::source(ColumnId(5)));
    assert_eq!(serde_json::to_string(&k).unwrap(), old, "peers and older builds read the same node");
    assert!(!k.is_filtered_source());
    let pass = NodeKind::pass(ColumnId(5), &[LayerRole::Shadow], true);
    let json = serde_json::to_string(&pass).unwrap();
    assert_eq!(serde_json::from_str::<NodeKind>(&json).unwrap(), pass);
    assert!(pass.is_filtered_source());
}

#[test]
fn a_role_pass_renders_through_the_graph_and_its_hash_follows_the_filter() {
    let (mut e, at) = satsuei_rig();
    let src = cut(&e, at).graph.nodes().find(|n| n.kind.input_count() == 0).unwrap().id;
    let column = cut(&e, at).xsheet.columns[1].id;
    let plain = e.eval(at.scene, at.cut, 0).unwrap().hash();
    let set = |e: &mut Engine, kind: NodeKind| {
        e.apply("pass", vec![Command::SetNodeKind { at, id: src, kind }]).unwrap();
        e.eval(at.scene, at.cut, 0).unwrap().hash()
    };
    let shadow = set(&mut e, NodeKind::pass(column, &[LayerRole::Shadow], false));
    let line = set(&mut e, NodeKind::pass(column, &[LayerRole::Line], false));
    assert!(plain != shadow && shadow != line, "each pass is its own value");
    // The shadow pass renders the shadow only (lines and paint left out).
    set(&mut e, NodeKind::pass(column, &[LayerRole::Shadow], false));
    let px = |e: &Engine, x, y| graph_composite_pixel_law(cut(e, at), ColorLaw::Srgb, 0, 64, 36, x, y).unwrap();
    assert_eq!(px(&e, 20, 12), stored(SHADE));
    assert_eq!(px(&e, 35, 20), [0.0; 4], "the paint is not in the shadow pass");
    assert_eq!(set(&mut e, NodeKind::source(column)), plain, "back to plain: the old value");
}

// ---- The light FX ------------------------------------------------------------------

#[test]
fn every_look_fx_at_its_neutral_setting_is_the_identity() {
    let src = swatch(16, 12);
    let bg = solid(16, 12, [0.9, 0.8, 0.2, 1.0]);
    let neutral: [(FxKind, Vec<(&str, FxValue)>); 9] = [
        (FxKind::Para, vec![("amount", f(0.0))]),
        (FxKind::LightWrap, vec![("intensity", f(0.0))]),
        (FxKind::Rim, vec![("intensity", f(0.0))]),
        (FxKind::Vignette, vec![("amount", f(0.0))]),
        (FxKind::Grain, vec![("amount", f(0.0))]),
        (FxKind::MotionBlur, vec![("length", f(0.0))]),
        (FxKind::Lut, vec![]),
        (FxKind::Matte, vec![]),
        (FxKind::PaletteMap, vec![]),
    ];
    let kinds: Vec<FxKind> = neutral.iter().map(|(k, _)| *k).collect();
    assert_eq!(kinds, FxKind::LOOK, "every look kind is checked");
    for (kind, set) in &neutral {
        let ins: Vec<Option<&Image>> = match kind {
            FxKind::LightWrap => vec![Some(&src), Some(&bg), None],
            _ => vec![Some(&src)],
        };
        assert_eq!(run_at(*kind, set, &ins, 3), src, "{} at neutral is bit-exact", kind.name());
    }
    // The light wrap with no background, and the LUT with no file, too.
    assert_eq!(run(FxKind::LightWrap, &[], &src), src);
    assert_eq!(run(FxKind::Lut, &[("mix", f(1.0))], &src), src);
}

#[test]
fn para_matches_the_pinned_gradient() {
    // A grey column; white at the top to black at the bottom, multiplied.
    let src = solid(1, 4, [0.5, 0.5, 0.5, 1.0]);
    let set = [
        ("color_a", FxValue::Color([255, 255, 255, 255])),
        ("color_b", FxValue::Color([0, 0, 0, 255])),
        ("angle", f(90.0)),
        ("amount", f(1.0)),
    ];
    let out = run(FxKind::Para, &set, &src);
    for (y, want) in [0.4375, 0.3125, 0.1875, 0.0625].into_iter().enumerate() {
        let p = out.pixel(0, y as u32);
        assert!((p[0] - want).abs() < 1e-6 && p[3] == 1.0, "row {y}: {p:?} not {want}");
    }
    // Screen at half amount: c + (screen − c)/2.
    let out = run(FxKind::Para, &[set[0].clone(), set[1].clone(), set[2].clone(), ("mode", FxValue::Choice("screen".into())), ("amount", f(0.5))], &src);
    let t = 0.125f32;
    let g = 1.0 - t;
    let want = 0.5 + ((0.5 + g - 0.5 * g) - 0.5) * 0.5;
    assert!((out.pixel(0, 0)[0] - want).abs() < 1e-6);
}

#[test]
fn light_wrap_adds_the_blurred_background_at_the_matte_edge() {
    let (w, h) = (12, 3);
    let mut fg = Image::new(w, h);
    for y in 0..h {
        for x in 6..w {
            fg.set_pixel(x, y, [0.2, 0.2, 0.2, 1.0]);
        }
    }
    let bg = solid(w, h, [0.9, 0.6, 0.3, 1.0]);
    let out = run_at(FxKind::LightWrap, &[("radius", f(4.0)), ("intensity", f(1.0))], &[Some(&fg), Some(&bg), None], 0);
    let mut m = Image::new(w, h);
    for i in 0..(w * h) as usize {
        m.px[i * 4 + 3] = fg.px[i * 4 + 3];
    }
    let (sm, sb) = (blur(&m, 4.0, 4.0), blur(&bg, 4.0, 4.0));
    for i in 0..(w * h) as usize {
        let edge = m.px[i * 4 + 3] * (1.0 - sm.px[i * 4 + 3]);
        for c in 0..3 {
            let want = fg.px[i * 4 + c] + sb.px[i * 4 + c] * edge;
            assert!((out.px[i * 4 + c] - want).abs() < 1e-6, "pixel {i} channel {c}");
        }
        assert_eq!(out.px[i * 4 + 3], fg.px[i * 4 + 3], "coverage kept");
    }
    assert_eq!(out.pixel(2, 1), [0.0; 4], "outside the character: nothing");
    assert!(out.pixel(6, 1)[0] > 0.2, "the edge takes the light");
}

#[test]
fn rim_lights_only_the_edge_that_faces_the_light() {
    let mut src = Image::new(9, 9);
    for y in 2..7 {
        for x in 2..7 {
            src.set_pixel(x, y, [0.25, 0.25, 0.25, 1.0]);
        }
    }
    let set = [("angle", f(0.0)), ("width", f(1.0)), ("softness", f(0.0)), ("intensity", f(1.0)), ("color", FxValue::Color([255, 255, 255, 255]))];
    let out = run(FxKind::Rim, &set, &src);
    for y in 2..7 {
        assert_eq!(out.pixel(6, y), [1.25, 1.25, 1.25, 1.0], "the right edge is lit");
        assert_eq!(out.pixel(5, y), src.pixel(5, y), "inside: untouched");
        assert_eq!(out.pixel(2, y), src.pixel(2, y), "the edge away from the light: untouched");
    }
    assert_eq!(out.pixel(7, 4), [0.0; 4], "outside stays bare");
}

#[test]
fn vignette_fades_the_corners_by_the_pinned_curve() {
    let src = solid(9, 9, [0.6, 0.6, 0.6, 1.0]);
    let set = [("amount", f(1.0)), ("radius", f(0.5)), ("softness", f(0.5))];
    let out = run(FxKind::Vignette, &set, &src);
    assert_eq!(out.pixel(4, 4), src.pixel(4, 4), "the centre is untouched");
    let half_diag = 0.5 * (162.0f32).sqrt();
    let d = (4.0f32 * 4.0 * 2.0).sqrt() / half_diag;
    let t = ((d - 0.5) / 0.5).clamp(0.0, 1.0);
    let k = t * t * (3.0 - 2.0 * t);
    let want = 0.6 + (0.0 - 0.6) * k;
    assert!((out.pixel(0, 0)[0] - want).abs() < 1e-6, "{:?} not {want}", out.pixel(0, 0));
    assert_eq!(out.pixel(0, 0)[3], 1.0);
}

#[test]
fn grain_is_steady_for_a_frame_and_seed_and_moves_with_the_frame() {
    let src = solid(16, 16, [0.5, 0.5, 0.5, 1.0]);
    let set = [("amount", f(0.1))];
    let a = run_at(FxKind::Grain, &set, &[Some(&src)], 7);
    assert_eq!(a, run_at(FxKind::Grain, &set, &[Some(&src)], 7), "same frame, same seed: same pixels");
    assert_ne!(a, run_at(FxKind::Grain, &set, &[Some(&src)], 8), "another frame differs");
    assert_ne!(a, run_at(FxKind::Grain, &[("amount", f(0.1)), ("seed", f(3.0))], &[Some(&src)], 7), "another seed differs");
    assert!(a.px.iter().all(|v| (0.4..=0.6).contains(v) || *v == 1.0), "within ± amount");
    let mono = a.pixel(3, 5);
    assert!(mono[0] == mono[1] && mono[1] == mono[2], "mono: one grain for the three channels");
    // The eval recipe spells the frame, so the cache never repeats a grain.
    let (mut e, at, _) = fx_rig(FxKind::Grain);
    let h0 = e.eval(at.scene, at.cut, 0).unwrap().hash();
    assert_ne!(h0, e.eval(at.scene, at.cut, 1).unwrap().hash());
}

#[test]
fn motion_blur_length_zero_is_the_identity_and_a_dot_becomes_a_nine_pixel_run() {
    let mut dot = Image::new(21, 9);
    dot.set_pixel(10, 4, [1.0; 4]);
    assert_eq!(directional_blur(&dot, 0.0, 30.0), dot, "L = 0");
    let out = run(FxKind::MotionBlur, &[("length", f(8.0)), ("angle", f(0.0))], &dot);
    for y in 0..9 {
        for x in 0..21 {
            let p = out.pixel(x, y);
            if y == 4 && (6..=14).contains(&x) {
                assert!(p.iter().all(|v| (v - 1.0 / 9.0).abs() <= 1.0 / 255.0), "({x}, {y}) = {p:?}");
            } else {
                assert_eq!(p, [0.0; 4], "nothing at ({x}, {y})");
            }
        }
    }
    let mut tall = Image::new(9, 21);
    tall.set_pixel(4, 10, [1.0; 4]);
    let out = directional_blur(&tall, 8.0, 90.0);
    for y in 0..21 {
        for x in 0..9 {
            let p = out.pixel(x, y);
            if x == 4 && (6..=14).contains(&y) {
                assert!(p.iter().all(|v| (v - 1.0 / 9.0).abs() <= 1.0 / 255.0), "({x}, {y}) = {p:?}");
            } else {
                assert_eq!(p, [0.0; 4], "nothing at ({x}, {y})");
            }
        }
    }
}

#[test]
fn matte_cuts_by_alpha_or_luma_with_invert_and_choke_or_spread() {
    let src = solid(5, 1, [0.5, 0.5, 0.5, 1.0]);
    let mut m = Image::new(5, 1);
    for x in 1..4 {
        m.set_pixel(x, 0, [1.0; 4]);
    }
    let alpha = |set: &[(&str, FxValue)]| -> Vec<f32> {
        let out = run_at(FxKind::Matte, set, &[Some(&src), Some(&m)], 0);
        (0..5).map(|x| out.pixel(x, 0)[3]).collect()
    };
    assert_eq!(alpha(&[]), [0.0, 1.0, 1.0, 1.0, 0.0], "cut by the matte's alpha");
    assert_eq!(alpha(&[("choke", f(1.0))]), [0.0, 0.0, 1.0, 0.0, 0.0], "choked by a pixel");
    assert_eq!(alpha(&[("choke", f(-1.0))]), [1.0; 5], "spread by a pixel");
    assert_eq!(alpha(&[("invert", FxValue::Bool(true))]), [1.0, 0.0, 0.0, 0.0, 1.0]);
    let white = run_at(FxKind::Matte, &[("white", FxValue::Bool(true))], &[Some(&src), Some(&m)], 0);
    assert_eq!(white.pixel(2, 0), [1.0; 4], "the matte itself, white");
    assert_eq!(white.pixel(0, 0), [0.0; 4]);
    // Luma of the image's own colour: grey 0.5 re-covers it at 0.5.
    let own = run(FxKind::Matte, &[("channel", FxValue::Choice("luma".into()))], &src);
    let p = own.pixel(2, 0);
    assert!((p[3] - 0.5).abs() < 1e-6 && (p[0] - 0.25).abs() < 1e-6, "{p:?}");
}

// ---- LUTs ------------------------------------------------------------------------------

#[test]
fn parse_cube_reads_an_identity_and_a_pinned_inverting_lut() {
    let id = parse_cube(&identity_cube()).unwrap();
    assert_eq!(id.size, 2);
    for rgb in [[0.0, 0.0, 0.0], [0.25, 0.5, 0.75], [1.0, 0.1, 0.9], [0.333, 0.666, 0.999]] {
        let out = apply_lut(&id, rgb);
        for c in 0..3 {
            assert!((out[c] - rgb[c]).abs() <= 1.0 / 255.0, "{rgb:?} → {out:?}");
        }
    }
    let inv = parse_cube(&inverting_cube()).unwrap();
    let out = apply_lut(&inv, [0.25, 0.5, 0.75]);
    for (o, want) in out.iter().zip([0.75, 0.5, 0.25]) {
        assert!((o - want).abs() < 1e-6, "{out:?}");
    }
}

#[test]
fn a_malformed_cube_is_an_error_not_a_panic() {
    let cases = [
        ("", "no size"),
        ("LUT_3D_SIZE 2\n0 0 0\n", "too few colours"),
        ("LUT_3D_SIZE 1\n0 0 0\n", "size out of range"),
        ("LUT_3D_SIZE two\n", "not a number"),
        ("LUT_1D_SIZE 4\n0 0 0\n0 0 0\n0 0 0\n0 0 0\n", "a 1D LUT"),
        ("LUT_3D_SIZE 2\n0 0 x\n", "a bad colour"),
        ("LUT_3D_SIZE 2\n0 0\n", "two numbers"),
    ];
    for (text, why) in cases {
        assert!(parse_cube(text).is_err(), "{why}: {text:?}");
    }
    let mut big = inverting_cube();
    big.push_str("0 0 0\n");
    assert!(parse_cube(&big).is_err(), "one colour too many");
    assert!(parse_cube(&inverting_cube().replace("DOMAIN_MAX 1 1 1", "DOMAIN_MAX 0 1 1")).is_err());
}

#[test]
fn a_lut_node_renders_its_embedded_cube_and_the_file_survives_a_save() {
    let (mut e, at, node) = fx_rig(FxKind::Lut);
    let pixel = |e: &Engine| graph_composite_pixel_law(cut(e, at), ColorLaw::Srgb, 0, 8, 8, 4, 4).unwrap();
    let plain = pixel(&e);
    let h0 = e.eval(at.scene, at.cut, 0).unwrap().hash();
    let bytes = Arc::new(inverting_cube().into_bytes());
    e.apply(
        "load LUT",
        vec![
            Command::SetCutBlob { at, key: "lut.invert".into(), before: None, after: Some(bytes.clone()) },
            Command::SetNodeKind {
                at,
                id: node,
                kind: effect_with(FxKind::Lut, &[("lut", FxValue::Blob { blob: "lut.invert".into() })]),
            },
        ],
    )
    .unwrap();
    let inverted = pixel(&e);
    for (c, v) in [64u8, 128, 192].into_iter().enumerate() {
        let want = 1.0 - v as f32 / 255.0;
        assert!((inverted[c] - want).abs() <= 1.0 / 255.0, "{inverted:?}");
    }
    let h1 = e.eval(at.scene, at.cut, 0).unwrap().hash();
    assert_ne!(h0, h1);
    // Another file under the same key: another value.
    let ident = Arc::new(identity_cube().into_bytes());
    e.apply("swap", vec![Command::SetCutBlob { at, key: "lut.invert".into(), before: Some(bytes.clone()), after: Some(ident.clone()) }]).unwrap();
    assert_ne!(e.eval(at.scene, at.cut, 0).unwrap().hash(), h1, "the file's bytes are in the recipe");
    e.undo().unwrap();
    // A stale before is refused.
    assert!(e.apply("stale", vec![Command::SetCutBlob { at, key: "lut.invert".into(), before: Some(ident), after: None }]).is_err());
    // The file travels with the project.
    let dir = tempfile_dir();
    let path = dir.join("lut_roundtrip.animproj");
    e.save(&path).unwrap();
    let back = Engine::load(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(cut(&back, at).blobs, cut(&e, at).blobs);
    assert_eq!(pixel(&back), inverted, "the loaded cut renders on its own");
    // Undo takes the file and the node back: the source as it was.
    e.undo().unwrap();
    assert!(cut(&e, at).blobs.is_empty());
    assert_eq!(pixel(&e), plain);
}

fn tempfile_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- Palette maps --------------------------------------------------------------------

#[test]
fn palette_map_swaps_exact_chips_and_leaves_every_other_pixel() {
    let law = ColorLaw::Srgb;
    let (skin, skin_night) = ([222u8, 178, 140, 255], [120u8, 110, 150, 255]);
    let (shade, shade_night) = ([96u8, 112, 192, 255], [40u8, 50, 110, 255]);
    let texel = |c: [u8; 4]| law.texel(c).map(f16_bits_to_f32);
    let mut img = Image::new(4, 1);
    img.set_pixel(0, 0, texel(skin));
    img.set_pixel(1, 0, texel(shade));
    let edge = texel(skin).map(|v| v * 0.5); // an anti-aliased edge of the chip
    img.set_pixel(2, 0, edge);
    img.set_pixel(3, 0, [0.3, 0.3, 0.3, 1.0]);
    let pairs = [(skin, skin_night), (shade, shade_night)];
    let out = palette_map(&img, law, &pairs);
    assert_eq!(out.pixel(0, 0), texel(skin_night));
    assert_eq!(out.pixel(1, 0), texel(shade_night));
    assert_eq!(out.pixel(2, 0), edge, "not the chip exactly: untouched");
    assert_eq!(out.pixel(3, 0), [0.3, 0.3, 0.3, 1.0]);
    // Through the node: the same.
    let node = run(FxKind::PaletteMap, &[("map", FxValue::Map { map: pairs.to_vec(), from: "Ball/night".into() })], &img);
    assert_eq!(node, out);
}

// ---- Node edits and the template ---------------------------------------------------

#[test]
fn a_look_parameter_edit_is_one_undo_step_with_an_exact_inverse() {
    let (mut e, at, node) = fx_rig(FxKind::Para);
    let before = cut(&e, at).graph.node(node).unwrap().kind.clone();
    let depth = e.history_len();
    let kind = effect_with(FxKind::Para, &[("amount", f(0.8)), ("shape", FxValue::Choice("radial".into()))]);
    e.apply("node parameters", vec![Command::SetNodeKind { at, id: node, kind: kind.clone() }]).unwrap();
    assert_eq!(e.history_len(), depth + 1);
    e.undo().unwrap();
    assert_eq!(cut(&e, at).graph.node(node).unwrap().kind, before, "exact inverse");
    e.redo().unwrap();
    assert_eq!(cut(&e, at).graph.node(node).unwrap().kind, kind);
}

#[test]
fn the_satsuei_template_builds_the_pinned_chain_in_one_step_and_renders_on_the_cpu() {
    let (mut e, at) = satsuei_rig();
    let before = cut(&e, at).graph.clone();
    let meta = cut(&e, at).meta.clone();
    let depth = e.history_len();
    let mut next = e.project.next_id;
    let cmds = satsuei_template(cut(&e, at), at, || {
        next += 1;
        NodeId(next - 1)
    });
    e.project.next_id = next;
    e.apply("build satsuei graph", cmds).unwrap();
    assert_eq!(e.history_len(), depth + 1, "one undo step");
    let c = cut(&e, at);
    let fg = "Normal(Screen(Multiply(A[color,shadow,highlight,shadow_line,highlight_line;absorb], \
              Grade(Matte(A[shadow], -))), Glow(A[highlight], -)), A[line])";
    let cam = "Camera(BG)";
    let want = format!(
        "Output(Vignette(Grain(Grade(Light wrap(Para(Diffusion(Normal({cam}, {fg})), -), {cam}, {fg})))))"
    );
    assert_eq!(spell(c, c.graph.output.unwrap()), want);
    assert_eq!(c.graph.nodes().count(), 20, "the old nodes are gone");
    assert_eq!(graph::layout_from_meta(&c.meta).len(), 20, "every node has its place");
    // Every cel source is a pass, and the Output's feed is an Effect: the
    // whole subtree renders on the CPU.
    let feed = c.graph.node(c.graph.output.unwrap()).unwrap().inputs[0].unwrap().0;
    assert!(matches!(c.graph.node(feed).unwrap().kind, NodeKind::Effect { fx: FxKind::Vignette, .. }));
    let passes = c.graph.nodes().filter(|n| n.kind.is_filtered_source()).count();
    assert_eq!(passes, 4, "paint, shadow, highlight and lines");
    let px = graph_composite(c, ColorLaw::Srgb, 0, 64, 36).expect("the chain renders");
    assert!(px.iter().all(|v| v.is_finite()));
    // The character is in it: the lit paint differs from the bare BG.
    let bg = [px[(2 * 64 + 2) * 4], px[(2 * 64 + 2) * 4 + 1]];
    let skin = [px[(20 * 64 + 35) * 4], px[(20 * 64 + 35) * 4 + 1]];
    assert_ne!(bg, skin);
    e.undo().unwrap();
    assert_eq!(cut(&e, at).graph, before, "undo restores the graph exactly");
    assert_eq!(cut(&e, at).meta, meta);
}
