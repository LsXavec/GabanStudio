//! GM17: 3D layout and toon render — the loaders, the camera, the
//! rasteriser, toon bands, outlines, passes, the Model3D node through the
//! graph and the store, View3D's navigation maths, "Set cut camera from
//! view", image sequences and JPEG/TIFF stills. Headless: every model and
//! image is built here; frames stay at or under 64 × 64.

use std::path::Path;
use std::sync::Arc;

use anim_core::Engine;
use anim_core::color::ColorLaw;
use anim_core::command::{Command, CutRef};
use anim_core::deliver::{self, RenderOpts};
use anim_core::export::{RenderEnv, render_node_px, straight_rgba8};
use anim_core::fx::{FxParams, FxValue, Image};
use anim_core::graph::NodeKind;
use anim_core::ids::*;
use anim_core::model::{Cut, ImageAsset, ModelAsset};
use anim_core::r3d::{self, Camera3, Mesh, Model, ModelValues, Orbit, Scene3, View, Xform};
use anim_core::xsheet::{ParamInterp, ParamKey};

const CUBE_OBJ: &str = "o cube
v -1 -1 -1
v 1 -1 -1
v 1 1 -1
v -1 1 -1
v -1 -1 1
v 1 -1 1
v 1 1 1
v -1 1 1
usemtl red
f 1 4 3 2
f 5 6 7 8
f 1 2 6 5
f 2 3 7 6
f 3 4 8 7
f 4 1 5 8
";

const RED_MTL: &str = "newmtl red\nKd 1 0 0\n";

fn cube() -> Model {
    r3d::load_obj(CUBE_OBJ.as_bytes()).unwrap()
}

/// A camera on +Z looking at the origin with a 50 mm lens.
fn front_camera(distance: f32) -> Camera3 {
    Camera3 {
        eye: [0.0, 0.0, distance],
        target: [0.0; 3],
        roll_deg: 0.0,
        focal_mm: 50.0,
        near: 0.05,
        far: 100.0,
        ortho: false,
    }
}

/// A square of half-size `s` facing +Z at depth `z`, as one object.
fn quad(s: f32, z: f32, color: [u8; 4]) -> Mesh {
    Mesh {
        name: "quad".into(),
        positions: vec![[-s, -s, z], [s, -s, z], [s, s, z], [-s, s, z]],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        colors: vec![color; 2],
    }
}

/// A UV sphere of radius 1 (flat faces).
fn sphere(color: [u8; 4]) -> Model {
    let (rings, segs) = (16u32, 24u32);
    let mut positions = Vec::new();
    for r in 0..=rings {
        let th = std::f32::consts::PI * r as f32 / rings as f32;
        for s in 0..segs {
            let ph = std::f32::consts::TAU * s as f32 / segs as f32;
            positions.push([th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin()]);
        }
    }
    let mut triangles = Vec::new();
    for r in 0..rings {
        for s in 0..segs {
            let (a, b) = (r * segs + s, r * segs + (s + 1) % segs);
            let (c, d) = (a + segs, b + segs);
            triangles.push([a, c, b]);
            triangles.push([b, c, d]);
        }
    }
    let colors = vec![color; triangles.len()];
    Model {
        meshes: vec![Mesh {
            name: "sphere".into(),
            positions,
            triangles,
            colors,
        }],
    }
}

fn values(set: &[(&str, FxValue)]) -> ModelValues {
    let mut p = r3d::default_params();
    for (k, v) in set {
        p.set(k, v.clone());
    }
    ModelValues::of(&p)
}

fn n(x: f32) -> FxValue {
    FxValue::Float(x)
}

/// The front camera at `d` as Model3D parameters.
fn front(d: f32) -> Vec<(&'static str, FxValue)> {
    vec![
        ("cam_x", n(0.0)),
        ("cam_y", n(0.0)),
        ("cam_z", n(d)),
        ("target_x", n(0.0)),
        ("target_y", n(0.0)),
        ("target_z", n(0.0)),
        ("focal", n(50.0)),
    ]
}

fn rgba8(img: &Image, x: u32, y: u32) -> [u8; 4] {
    straight_rgba8(&img.pixel(x, y))
}

// ---- Loaders ---------------------------------------------------------------------

#[test]
fn an_obj_cube_loads_as_twelve_triangles_inside_plus_minus_one() {
    let m = cube();
    assert_eq!(m.triangle_count(), 12);
    assert_eq!(m.bbox(), Some(([-1.0; 3], [1.0; 3])));
    assert_eq!(m.meshes[0].colors[0], r3d::DEFAULT_COLOR, "no library: the default grey");
    // The library embedded at import gives the faces their colour, and the
    // bytes stay one OBJ.
    let embedded = r3d::embed_mtl(CUBE_OBJ.as_bytes(), RED_MTL);
    let red = r3d::load("obj", &embedded).unwrap();
    assert_eq!(red.triangle_count(), 12);
    assert!(red.meshes[0].colors.iter().all(|c| *c == [255, 0, 0, 255]));
    assert!(r3d::load_obj(b"# nothing here\n").is_err(), "no faces is refused");
}

/// A .glb holding one triangle with a base colour, placed by its node.
fn triangle_glb() -> Vec<u8> {
    glb_with_nodes(r#"[{"mesh":0,"translation":[0,0,2]}]"#)
}

/// A .glb whose scene is node 0 of `nodes` (a JSON array), over one green
/// triangle as mesh 0.
fn glb_with_nodes(nodes: &str) -> Vec<u8> {
    let mut bin = Vec::new();
    for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
        "nodes":NODES,
        "meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],
        "materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0,1,0,1]}}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],
        "buffers":[{"byteLength":36}]}"#
        .replace("NODES", nodes);
    let mut json = json.into_bytes();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut glb = Vec::new();
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin);
    glb
}

#[test]
fn a_minimal_glb_built_here_loads_placed_and_coloured() {
    let m = r3d::load("glb", &triangle_glb()).unwrap();
    assert_eq!(m.triangle_count(), 1);
    assert_eq!(m.bbox(), Some(([0.0, 0.0, 2.0], [1.0, 1.0, 2.0])), "the node's translation applies");
    assert_eq!(m.meshes[0].colors[0], [0, 255, 0, 255], "the base colour factor, as a chip");
    assert!(r3d::load("glb", b"not a model").is_err());
    assert_eq!(r3d::format_for_extension("GLB"), Some("glb"));
    assert_eq!(r3d::format_for_extension("fbx"), None);
}

#[test]
fn a_gltf_whose_nodes_loop_or_nest_deep_loads_without_overflowing_the_stack() {
    // Node 0 lists itself as its child: a malformed file, read (once) or
    // refused, never followed round forever.
    match r3d::load("glb", &glb_with_nodes(r#"[{"mesh":0,"children":[0]}]"#)) {
        Ok(m) => assert_eq!(m.triangle_count(), 1, "a node under itself is entered once"),
        Err(e) => assert!(!e.is_empty()),
    }
    // Two nodes under each other.
    let pair = r3d::load("glb", &glb_with_nodes(r#"[{"mesh":0,"children":[1]},{"children":[0]}]"#));
    if let Ok(m) = pair {
        assert_eq!(m.triangle_count(), 1);
    }
    // A chain 20 000 nodes deep, the mesh at its foot, each a step up.
    let depth = 20_000;
    let nodes: Vec<String> = (0..depth)
        .map(|i| match i + 1 < depth {
            true => format!(r#"{{"children":[{}],"translation":[0,0.001,0]}}"#, i + 1),
            false => r#"{"mesh":0}"#.to_string(),
        })
        .collect();
    let m = r3d::load("glb", &glb_with_nodes(&format!("[{}]", nodes.join(",")))).unwrap();
    assert_eq!(m.triangle_count(), 1);
    let (lo, _) = m.bbox().unwrap();
    assert!((lo[1] - 0.001 * (depth - 1) as f32).abs() < 0.05, "every parent's transform applies: {lo:?}");
}

// ---- Camera ------------------------------------------------------------------------

#[test]
fn the_target_projects_to_the_frame_centre_and_roll_keeps_it_there() {
    for roll in [0.0, 30.0] {
        let cam = Camera3 {
            eye: [3.5, 2.5, 5.0],
            target: [0.5, 0.25, -1.0],
            roll_deg: roll,
            ..front_camera(5.0)
        };
        let (x, y, depth) = cam.project(cam.target, 64, 36).unwrap();
        assert!((x - 32.0).abs() < 1e-3 && (y - 18.0).abs() < 1e-3, "roll {roll}: ({x}, {y})");
        assert!((depth - r3d::length(r3d::sub(cam.target, cam.eye))).abs() < 1e-4);
    }
    // Up is up and right is right; behind the camera does not project.
    let cam = front_camera(5.0);
    let (_, y_up, _) = cam.project([0.0, 1.0, 0.0], 64, 64).unwrap();
    let (x_right, _, _) = cam.project([1.0, 0.0, 0.0], 64, 64).unwrap();
    assert!(y_up < 32.0 && x_right > 32.0);
    assert_eq!(cam.project([0.0, 0.0, 6.0], 64, 64), None);
}

#[test]
fn a_50mm_lens_on_the_36mm_back_sees_39_6_degrees_across() {
    let deg = r3d::focal_to_fov(50.0).to_degrees();
    assert!((deg - 39.598).abs() < 0.001, "{deg}");
    assert!(r3d::focal_to_fov(85.0) < r3d::focal_to_fov(50.0), "a longer lens sees less");
    // The frame's edge is the angle's edge.
    let cam = front_camera(5.0);
    let half = 5.0 * (r3d::focal_to_fov(50.0) / 2.0).tan();
    let (x, _, _) = cam.project([half, 0.0, 0.0], 64, 36).unwrap();
    assert!((x - 64.0).abs() < 1e-3, "{x}");
}

// ---- The rasteriser ------------------------------------------------------------------

#[test]
fn a_front_facing_quad_covers_its_area_within_one_edge() {
    let cam = front_camera(5.0);
    // tan(fov/2) = 0.36: half-size 0.9 at 5 units is exactly 16 px of 32.
    for s in [0.9f32, 0.7, 0.33] {
        let mut scene = Scene3::default();
        scene.add_model(&Model { meshes: vec![quad(s, 0.0, [255; 4])] }, &Xform::IDENTITY, 1);
        let b = r3d::rasterize(&scene, &cam, 64, 64);
        let half = s / (5.0 * 0.36) * 32.0;
        let area = (2.0 * half).powi(2);
        let got = b.covered() as f32;
        // Within one edge's worth of pixels (the boundary ring) of the area,
        // and exactly the pixel centres inside it.
        assert!((got - area).abs() <= 8.0 * half, "half-size {s}: {got} px, area {area}");
        let side = (0..64).filter(|x| (*x as f32 + 0.5 - 32.0).abs() < half).count();
        assert_eq!(b.covered(), side * side, "half-size {s}");
    }
    // Pixel-aligned edges: the top-left rule counts the shared diagonal once.
    let mut scene = Scene3::default();
    scene.add_model(&Model { meshes: vec![quad(0.9, 0.0, [255; 4])] }, &Xform::IDENTITY, 1);
    assert_eq!(r3d::rasterize(&scene, &cam, 64, 64).covered(), 1024);
}

#[test]
fn the_nearer_triangle_wins_the_depth_test_in_either_order() {
    let cam = front_camera(5.0);
    let (far_q, near_q) = (quad(0.8, 0.0, [255, 0, 0, 255]), quad(0.5, 1.0, [0, 0, 255, 255]));
    for order in [[&far_q, &near_q], [&near_q, &far_q]] {
        let mut scene = Scene3::default();
        let model = Model {
            meshes: order.iter().map(|m| (*m).clone()).collect(),
        };
        scene.add_model(&model, &Xform::IDENTITY, 1);
        let b = r3d::rasterize(&scene, &cam, 64, 64);
        let centre = 32 * 64 + 32;
        assert_eq!(b.color[centre], [0, 0, 255, 255], "the nearer quad shows");
        assert!((b.depth[centre] - 4.0).abs() < 1e-4, "{}", b.depth[centre]);
        assert_eq!(b.color[32 * 64 + 4], [0, 0, 0, 0], "outside both: empty");
    }
}

// ---- Toon, outline and passes ---------------------------------------------------------

#[test]
fn a_sphere_shows_exactly_steps_bands_and_the_shadow_chip_multiplies() {
    let model = sphere([200, 200, 200, 255]);
    let shadow = [128, 64, 255, 255];
    for steps in [2.0f32, 3.0] {
        let mut set = front(4.0);
        set.extend([
            ("steps", n(steps)),
            ("outline", n(0.0)),
            ("shadow", FxValue::Color(shadow)),
        ]);
        let img = r3d::render(&model, &values(&set), ColorLaw::Srgb, 64, 64);
        let mut shades: Vec<[u8; 4]> = (0..64 * 64)
            .map(|i| rgba8(&img, i % 64, i / 64))
            .filter(|p| p[3] > 0)
            .collect();
        shades.sort();
        shades.dedup();
        assert_eq!(shades.len(), steps as usize, "steps {steps}: {shades:?}");
        assert!(shades.contains(&[200, 200, 200, 255]), "the lit band is the base");
        assert!(shades.contains(&[100, 50, 200, 255]), "band 0 is base × shadow: {shades:?}");
    }
}

#[test]
fn a_cube_outline_is_the_outline_colour_at_its_width_and_width_zero_draws_none() {
    let model = cube();
    let ink = [10, 200, 30, 255];
    for width in [1.0f32, 2.0, 3.0, 4.0, 5.0] {
        let mut set = front(5.0);
        set.extend([
            ("outline", n(width)),
            ("outline_color", FxValue::Color(ink)),
            ("crease", n(180.0)),
        ]);
        let v = values(&set);
        let img = r3d::render(&model, &v, ColorLaw::Srgb, 64, 64);
        // Looking straight at the front face: the silhouette is a square
        // and the row through its centre crosses its left edge once.
        let row: Vec<bool> = (0..64).map(|x| rgba8(&img, x, 32) == ink).collect();
        let first = row.iter().position(|o| *o).expect("an outline");
        let run = row[first..].iter().take_while(|o| **o).count();
        assert_eq!(run, width as usize, "width {width}");
        // Every line pixel is within (width − 1)/2 px of the silhouette: of
        // an edge pixel's centre for an odd width, of the crack between it
        // and its empty neighbour for an even one (in doubled px, where a
        // crack is a whole number).
        let mut scene = Scene3::default();
        scene.add_model(&model, &v.xform(), 1);
        let b = r3d::rasterize(&scene, &v.camera(), 64, 64);
        let odd = width as i32 % 2 == 1;
        let mut seeds = Vec::new();
        for y in 0..64i32 {
            for x in 0..64i32 {
                if b.id[(y * 64 + x) as usize] == 0 {
                    continue;
                }
                for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if (0..64).contains(&nx) && (0..64).contains(&ny) && b.id[(ny * 64 + nx) as usize] == 0 {
                        seeds.push(if odd { (2 * x, 2 * y) } else { (2 * x + dx, 2 * y + dy) });
                    }
                }
            }
        }
        let reach2 = (width as i32 - 1).pow(2);
        let lines = r3d::outline(&b, width, 180.0);
        for (i, on) in lines.iter().enumerate() {
            let (x, y) = ((i % 64) as i32, (i / 64) as i32);
            let near = seeds.iter().any(|(sx, sy)| (2 * x - sx).pow(2) + (2 * y - sy).pow(2) <= reach2);
            assert_eq!(*on, near, "width {width} at ({x}, {y})");
        }
        // A fractional width rounds to whole px, and a hairline is one px.
        if width == 2.0 {
            assert_eq!(r3d::outline(&b, 2.4, 180.0), lines, "2.4 px draws as 2");
        }
        if width == 1.0 {
            assert_eq!(r3d::outline(&b, 0.3, 180.0), lines, "0.3 px draws as 1");
        }
        // The lines pass is the outline alone.
        let lines_pass = r3d::render(&model, &values(&[set.clone(), vec![("pass", FxValue::Choice("lines".into()))]].concat()), ColorLaw::Srgb, 64, 64);
        let inked = (0..64 * 64).filter(|i| rgba8(&lines_pass, i % 64, i / 64) == ink).count();
        assert_eq!(inked, lines.iter().filter(|o| **o).count());
    }
    let mut set = front(5.0);
    set.extend([("outline", n(0.0)), ("outline_color", FxValue::Color(ink))]);
    set.push(("pass", FxValue::Choice("lines".into())));
    let img = r3d::render(&model, &values(&set), ColorLaw::Srgb, 64, 64);
    assert!(img.px.iter().all(|v| *v == 0.0), "width 0: no outline at all");
}

#[test]
fn the_depth_pass_grows_with_distance() {
    // A floor receding from the camera.
    let floor = Mesh {
        name: "floor".into(),
        positions: vec![[-2.0, -1.0, 2.0], [2.0, -1.0, 2.0], [2.0, -1.0, -8.0], [-2.0, -1.0, -8.0]],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        colors: vec![[255; 4]; 2],
    };
    let model = Model { meshes: vec![floor] };
    let mut set = front(5.0);
    set.extend([("near", n(1.0)), ("far", n(20.0)), ("pass", FxValue::Choice("depth".into()))]);
    let v = values(&set);
    let img = r3d::render(&model, &v, ColorLaw::Srgb, 64, 64);
    let mut scene = Scene3::default();
    scene.add_model(&model, &v.xform(), 1);
    let b = r3d::rasterize(&scene, &v.camera(), 64, 64);
    let mut seen: Vec<(f32, f32)> = (0..64 * 64usize)
        .filter(|i| b.id[*i] != 0)
        .map(|i| (b.depth[i], img.px[i * 4]))
        .collect();
    assert!(seen.len() > 200);
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(seen.windows(2).all(|w| w[1].1 >= w[0].1), "monotonic with distance");
    assert!(seen.last().unwrap().1 > seen[0].1 + 0.2, "and it does grow");
}

#[test]
fn the_id_matte_gives_each_object_its_own_colour() {
    let model = Model {
        meshes: (0..3)
            .map(|k| {
                let mut q = quad(0.4, 0.0, [255; 4]);
                for p in &mut q.positions {
                    p[0] += (k as f32 - 1.0) * 1.0;
                }
                q
            })
            .collect(),
    };
    let mut set = front(5.0);
    set.push(("pass", FxValue::Choice("id matte".into())));
    let img = r3d::render(&model, &values(&set), ColorLaw::Srgb, 64, 64);
    let (l, m, r) = (rgba8(&img, 32 - 18, 32), rgba8(&img, 32, 32), rgba8(&img, 32 + 18, 32));
    assert!(l != m && m != r && l != r, "{l:?} {m:?} {r:?}");
    assert_eq!(m, r3d::id_chip(2));
    let chips: Vec<[u8; 4]> = (1..=12).map(r3d::id_chip).collect();
    for (i, a) in chips.iter().enumerate() {
        assert!(chips[i + 1..].iter().all(|b| b != a), "ids 1..12 all differ");
    }
}

// ---- The node through the graph and the store ------------------------------------------

struct Rig {
    engine: Engine,
    at: CutRef,
    node: NodeId,
    model: ModelId,
}

fn rig() -> Rig {
    let mut engine = Engine::new("r3d tests");
    let scene = engine.add_scene("SC");
    let cut = engine.add_cut(scene, "cut", 24).unwrap();
    let at = CutRef { scene, cut };
    let model = engine.alloc_model_id();
    let asset = ModelAsset::new("cube.obj", "obj", r3d::embed_mtl(CUBE_OBJ.as_bytes(), RED_MTL));
    let (node, out) = (engine.alloc_node_id(), engine.alloc_node_id());
    engine
        .apply(
            "rig",
            vec![
                Command::SetCutModel { at, id: model, before: None, after: Some(asset) },
                Command::AddNode { at, id: node, kind: NodeKind::model3d(model) },
                Command::AddNode { at, id: out, kind: NodeKind::Output },
                Command::Connect { at, from: node, from_pin: 0, to: out, to_pin: 0 },
                Command::SetOutput { at, node: Some(out) },
            ],
        )
        .unwrap();
    Rig { engine, at, node, model }
}

impl Rig {
    fn cut(&self) -> &Cut {
        self.engine.project.cut(self.at.scene, self.at.cut).unwrap()
    }

    fn kind(&self) -> NodeKind {
        self.cut().graph.node(self.node).unwrap().kind.clone()
    }
}

#[test]
fn a_bound_camera_moves_the_picture_and_every_render_repeats() {
    let mut r = rig();
    let at = r.at;
    let column = r.engine.add_param_column(at, "cam x").unwrap();
    let NodeKind::Model3D { model, params, mut binds } = r.kind() else { unreachable!() };
    binds.set("cam_x", Some(column));
    let key = |frame, value| Command::SetParamKey {
        at,
        column,
        frame,
        key: Some(ParamKey { value, interp: ParamInterp::Linear, handles: None }),
    };
    r.engine
        .apply(
            "bind",
            vec![
                Command::SetNodeKind { at, id: r.node, kind: NodeKind::Model3D { model, params, binds } },
                key(0, -3.0),
                key(10, 3.0),
            ],
        )
        .unwrap();
    let px = |r: &Rig, f| render_node_px(r.cut(), ColorLaw::Srgb, r.node, f, 64, 36);
    let (a, b) = (px(&r, 0), px(&r, 10));
    assert!(a.iter().any(|v| *v > 0.0), "the cube shows");
    assert_ne!(a, b, "the keyed camera moves the picture");
    assert_eq!(px(&r, 5), px(&r, 5), "deterministic");
    let h = |r: &mut Rig, f| r.engine.eval(at.scene, at.cut, f).unwrap().hash();
    assert_ne!(h(&mut r, 0), h(&mut r, 10));
    assert_eq!(h(&mut r, 3), h(&mut r, 3));
    // The node's camera at frame 5 is halfway.
    let NodeKind::Model3D { params, binds, .. } = r.kind() else { unreachable!() };
    assert!(r.cut().model_at(&params, &binds, 5).camera().eye[0].abs() < 1e-5);
    // The delivered frame is the node's picture.
    let img = deliver::render(r.cut(), ColorLaw::Srgb, 10, 64, 36, &RenderOpts::default());
    assert_eq!(img.px, b);
    let _ = r.model;
}

#[test]
fn cut_models_round_trip_byte_identical_and_the_edit_undoes_exactly() {
    let mut r = rig();
    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("r3d_models.animproj");
    // A second model whose bytes are every byte value.
    let glb = ModelAsset::new("tri.glb", "glb", triangle_glb());
    let raw = ModelAsset::new("raw.obj", "obj", (0..=255u8).collect());
    let (g, w) = (r.engine.alloc_model_id(), r.engine.alloc_model_id());
    let at = r.at;
    r.engine
        .apply(
            "models",
            vec![
                Command::SetCutModel { at, id: g, before: None, after: Some(glb) },
                Command::SetCutModel { at, id: w, before: None, after: Some(raw) },
            ],
        )
        .unwrap();
    r.engine.save(&path).unwrap();
    let back = Engine::load(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let (a, b) = (r.cut(), back.project.cut(at.scene, at.cut).unwrap());
    assert_eq!(a.models.len(), 3);
    assert_eq!(a.models, b.models, "names, formats, bytes and hashes");
    for (id, m) in &a.models {
        assert_eq!(m.bytes.as_slice(), b.models[id].bytes.as_slice(), "byte-identical");
    }
    assert_eq!(a.graph, b.graph);
    let f = 0;
    assert_eq!(render_node_px(a, ColorLaw::Srgb, r.node, f, 32, 18), render_node_px(b, ColorLaw::Srgb, r.node, f, 32, 18));
    // The edit's inverse removes exactly what it added; a stale edit is
    // refused.
    r.engine.undo().unwrap();
    assert_eq!(r.cut().models.len(), 1);
    let stale = Command::SetCutModel { at, id: r.model, before: None, after: None };
    assert!(r.engine.apply("stale", vec![stale]).is_err());
    let _ = Arc::new(());
}

// ---- View3D navigation and "Set cut camera from view" -------------------------------------

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-5)
}

#[test]
fn orbit_maths_round_trips() {
    for (yaw, pitch) in [(0.0f32, 0.0f32), (35.0, 20.0), (-120.0, -45.0), (170.0, 80.0)] {
        let o = Orbit {
            target: [0.5, 1.0, -2.0],
            yaw_deg: yaw,
            pitch_deg: pitch,
            distance: 7.0,
            focal_mm: 35.0,
            ortho: false,
        };
        let back = Orbit::from_eye(o.eye(), o.target, o.focal_mm);
        assert!((back.yaw_deg - yaw).abs() < 1e-3 && (back.pitch_deg - pitch).abs() < 1e-3, "{back:?}");
        assert!((back.distance - 7.0).abs() < 1e-4);
        // The view camera looks at the target from the eye.
        let cam = o.camera();
        assert!(close(cam.basis().forward, r3d::normalize(r3d::sub(o.target, o.eye()))));
    }
    let mut o = Orbit::default();
    let start = o;
    o.orbit(25.0, -10.0);
    o.orbit(-25.0, 10.0);
    assert!((o.yaw_deg - start.yaw_deg).abs() < 1e-3 && (o.pitch_deg - start.pitch_deg).abs() < 1e-3);
    o.zoom(3.0);
    assert!(o.distance < start.distance);
    o.zoom(-3.0);
    assert!((o.distance - start.distance).abs() < 1e-3);
    // A pan slides the target across the view, never along it.
    o.pan(40.0, 0.0, 360.0);
    let moved = r3d::sub(o.target, start.target);
    assert!(r3d::length(moved) > 0.0 && r3d::dot(moved, start.basis().forward).abs() < 1e-4);
    assert!(r3d::dot(moved, start.basis().right) < 0.0, "drag right: the scene follows the pointer");
}

#[test]
fn numpad_views_set_exact_orientations() {
    let cases = [
        (View::Front, [0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (View::Back, [0.0, 0.0, 1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        (View::Right, [-1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        (View::Left, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        (View::Top, [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        (View::Bottom, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    for (view, forward, right, up) in cases {
        let mut o = Orbit::default();
        o.orbit(33.0, 12.0);
        o.set_view(view);
        let b = o.basis();
        assert!(close(b.forward, forward) && close(b.right, right) && close(b.up, up), "{view:?}: {b:?}");
    }
}

#[test]
fn framing_a_model_shows_all_of_it_at_any_size_and_frame_all_keeps_the_view_direction() {
    for (scale, offset) in [(1.0f32, [0.0f32, 0.0, 0.0]), (40.0, [120.0, -30.0, 9.0]), (0.01, [0.0, 0.02, 0.0])] {
        let placed = Xform { translate: offset, rotate_deg: [0.0, 25.0, 0.0], scale };
        let bbox = r3d::placed_bbox(&cube(), &placed).unwrap();
        for (w, h) in [(64u32, 36u32), (36, 64)] {
            let cam = r3d::frame_box(bbox, 35.0, w as f32 / h as f32);
            let d = r3d::length(r3d::sub(cam.eye, cam.target));
            assert!(close(cam.target, r3d::mul(r3d::add(bbox.0, bbox.1), 0.5)), "aimed at the middle");
            assert!((cam.near - d / 1000.0).abs() <= d * 1e-6, "near is a thousandth of the distance");
            let radius = r3d::length(r3d::sub(bbox.1, bbox.0)) * 0.5;
            assert!((cam.far - (d + 4.0 * radius)).abs() <= cam.far * 1e-5);
            for i in 0..8 {
                let (lo, hi) = bbox;
                let corner = [
                    if i & 1 == 0 { lo[0] } else { hi[0] },
                    if i & 2 == 0 { lo[1] } else { hi[1] },
                    if i & 4 == 0 { lo[2] } else { hi[2] },
                ];
                let (x, y, z) = cam.project(corner, w, h).expect("in front of the camera");
                assert!(x > 0.0 && x < w as f32 && y > 0.0 && y < h as f32, "scale {scale} {w}×{h}: {corner:?} at ({x}, {y})");
                assert!(z > cam.near && z < cam.far);
            }
        }
    }
    // Frame all: the view turns no way, it closes on the box.
    let mut o = Orbit::default();
    o.orbit(70.0, -20.0);
    let (yaw, pitch) = (o.yaw_deg, o.pitch_deg);
    let bbox = ([100.0, 100.0, 100.0], [102.0, 101.0, 100.5]);
    o.frame(bbox, 16.0 / 9.0);
    assert_eq!((o.yaw_deg, o.pitch_deg), (yaw, pitch));
    assert!(close(o.target, [101.0, 100.5, 100.25]));
    let cam = r3d::frame_box(bbox, o.focal_mm, 16.0 / 9.0);
    assert!((o.distance - r3d::length(r3d::sub(cam.eye, cam.target))).abs() < 1e-3);
}

#[test]
fn set_cut_camera_from_view_keys_every_camera_parameter_in_one_undo_step() {
    let mut r = rig();
    let (at, node, frame) = (r.at, r.node, 6);
    let mut view = Orbit::default();
    view.orbit(40.0, 15.0);
    view.focal_mm = 85.0;
    let cam = view.camera();
    let need = r3d::camera_columns_needed(r.cut(), node);
    assert_eq!(need, r3d::CAMERA_KEYS.len(), "nothing bound yet");
    let ids: Vec<ParamId> = (0..need).map(|_| r.engine.alloc_param_id()).collect();
    let cmds = r3d::key_camera(r.cut(), at, node, &cam, frame, &mut ids.into_iter()).unwrap();
    let before = r.engine.history_len();
    r.engine.apply("Set cut camera from view", cmds).unwrap();
    assert_eq!(r.engine.history_len(), before + 1, "one undo step");
    let NodeKind::Model3D { params, binds, .. } = r.kind() else { unreachable!() };
    let got = r.cut().model_at(&params, &binds, frame).camera();
    assert!(close(got.eye, cam.eye) && close(got.target, cam.target), "{got:?} vs {cam:?}");
    assert!((got.focal_mm - 85.0).abs() < 1e-5 && got.roll_deg == 0.0);
    assert_eq!(r3d::camera_columns_needed(r.cut(), node), 0);
    // Again from another view at another frame: keys only, no new columns.
    view.set_view(View::Right);
    let cmds = r3d::key_camera(r.cut(), at, node, &view.camera(), 12, &mut std::iter::empty()).unwrap();
    assert!(cmds.iter().all(|c| matches!(c, Command::SetParamKey { .. })));
    r.engine.apply("again", cmds).unwrap();
    r.engine.undo().unwrap();
    r.engine.undo().unwrap();
    assert!(r.cut().xsheet.params.is_empty(), "undone in one step each");
    let NodeKind::Model3D { binds, .. } = r.kind() else { unreachable!() };
    assert!(binds.0.is_empty());
}

#[test]
fn every_model3d_node_shoots_through_the_one_cut_camera_with_id_chips_of_its_own() {
    let mut r = rig();
    let (at, first, frame) = (r.at, r.node, 3);
    let model2 = r.engine.alloc_model_id();
    let second = r.engine.alloc_node_id();
    let asset = ModelAsset::new("cube2.obj", "obj", r3d::embed_mtl(CUBE_OBJ.as_bytes(), RED_MTL));
    let add = vec![
        Command::SetCutModel { at, id: model2, before: None, after: Some(asset) },
        Command::AddNode { at, id: second, kind: NodeKind::model3d(model2) },
    ];
    r.engine.apply("second model", add).unwrap();
    let mut view = Orbit::default();
    view.orbit(-30.0, 10.0);
    let cam = view.camera();
    // Keyed from the second node: one batch binds both.
    let need = r3d::camera_columns_needed(r.cut(), second);
    assert_eq!(need, r3d::CAMERA_KEYS.len(), "no camera column yet");
    let ids: Vec<ParamId> = (0..need).map(|_| r.engine.alloc_param_id()).collect();
    let cmds = r3d::key_camera(r.cut(), at, second, &cam, frame, &mut ids.into_iter()).unwrap();
    let before = r.engine.history_len();
    r.engine.apply("Set cut camera from view", cmds).unwrap();
    assert_eq!(r.engine.history_len(), before + 1, "one undo step");
    let of = |r: &Rig, id: NodeId| match &r.cut().graph.node(id).unwrap().kind {
        NodeKind::Model3D { params, binds, .. } => (params.clone(), binds.clone()),
        _ => unreachable!(),
    };
    let ((p1, b1), (p2, b2)) = (of(&r, first), of(&r, second));
    assert_eq!(b1, b2, "both bound to the same columns");
    for (p, b) in [(&p1, &b1), (&p2, &b2)] {
        let got = r.cut().model_at(p, b, frame).camera();
        assert!(close(got.eye, cam.eye) && close(got.target, cam.target), "{got:?} vs {cam:?}");
    }
    let names: Vec<&str> = r.cut().xsheet.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names.len(), r3d::CAMERA_KEYS.len());
    assert!(names.contains(&"cut camera x") && names.contains(&"cut camera target y"), "{names:?}");
    assert!(names.contains(&"cut camera focal length"), "{names:?}");
    assert_eq!(r3d::camera_columns_needed(r.cut(), first), 0, "the first node keys the same columns");
    let cmds = r3d::key_camera(r.cut(), at, first, &cam, 9, &mut std::iter::empty()).unwrap();
    assert!(cmds.iter().all(|c| matches!(c, Command::SetParamKey { .. })), "keys only");
    // The ID matte: the two nodes' objects get different chips.
    let (id1, id2) = (r3d::first_object_id(r.cut(), first), r3d::first_object_id(r.cut(), second));
    assert_eq!((id1, id2), (1, 1 + r3d::OBJECTS_PER_NODE));
    let mut set_pass = Vec::new();
    for (id, (mut params, binds), model) in [(first, (p1, b1), r.model), (second, (p2, b2), model2)] {
        params.set("pass", FxValue::Choice("id matte".into()));
        set_pass.push(Command::SetNodeKind { at, id, kind: NodeKind::Model3D { model, params, binds } });
    }
    r.engine.apply("id matte", set_pass).unwrap();
    let centre = |id: NodeId| {
        let px = render_node_px(r.cut(), ColorLaw::Srgb, id, frame, 32, 32);
        let i = (16 * 32 + 16) * 4;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    let (c1, c2) = (centre(first), centre(second));
    assert!(c1[3] == 1.0 && c2[3] == 1.0, "both cubes cover the centre: {c1:?} {c2:?}");
    assert_ne!(c1, c2, "two nodes, two chips");
    assert_ne!(r3d::id_chip(id1), r3d::id_chip(id2));
}

// ---- Image sequences and stills -------------------------------------------------------------

fn png_bytes(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    use image::ImageEncoder as _;
    let px: Vec<u8> = (0..w * h).flat_map(|_| rgba).collect();
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&px, w, h, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

#[test]
fn a_hash_pattern_names_the_frame_file() {
    assert_eq!(r3d::expand_hashes("shot_####.png", 7), "shot_0007.png");
    assert_eq!(r3d::expand_hashes("a_##.exr", 123), "a_123.exr", "a longer number is written whole");
    assert_eq!(r3d::expand_hashes("still.png", 9), "still.png");
    let file = r3d::sequence_file("shot_####.png", 0, 0, false, 7, None).unwrap();
    assert_eq!(file.to_string_lossy(), "shot_0007.png");
    // File `first` shows at frame `offset`.
    assert_eq!(r3d::sequence_number(101, 4, 4), Some(101));
    assert_eq!(r3d::sequence_number(101, 4, 10), Some(107));
    assert_eq!(r3d::sequence_number(101, 4, 3), None);
}

fn seq_rig(pattern: &str, first: u32, hold_last: bool) -> (Engine, CutRef) {
    let mut engine = Engine::new("seq tests");
    let scene = engine.add_scene("SC");
    let cut = engine.add_cut(scene, "cut", 24).unwrap();
    let at = CutRef { scene, cut };
    let (node, out) = (engine.alloc_node_id(), engine.alloc_node_id());
    let kind = NodeKind::ImageSequence { pattern: pattern.into(), first, offset: 0, hold_last, fit: false };
    engine
        .apply(
            "rig",
            vec![
                Command::AddNode { at, id: node, kind },
                Command::AddNode { at, id: out, kind: NodeKind::Output },
                Command::Connect { at, from: node, from_pin: 0, to: out, to_pin: 0 },
                Command::SetOutput { at, node: Some(out) },
            ],
        )
        .unwrap();
    (engine, at)
}

#[test]
fn relative_frames_resolve_against_the_project_folder_and_a_missing_one_warns_once() {
    let dir = std::env::temp_dir().join("anim_core_tests").join("r3d_seq");
    std::fs::create_dir_all(dir.join("renders")).unwrap();
    let red = png_bytes(8, 6, [255, 0, 0, 255]);
    std::fs::write(dir.join("renders").join("shot_0003.png"), &red).unwrap();
    std::fs::remove_file(dir.join("renders").join("shot_0004.png")).ok();
    let (engine, at) = seq_rig("renders/shot_####.png", 0, false);
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    let env = RenderEnv::new(Some(dir.clone()));
    let opts = RenderOpts::default();
    let img = deliver::render_env(cut, ColorLaw::Srgb, 3, 32, 18, &opts, &env);
    assert_eq!(rgba8(&img, 1, 1), [255, 0, 0, 255], "the file, from the project's folder");
    assert_eq!(rgba8(&img, 7, 5), [255, 0, 0, 255]);
    assert_eq!(rgba8(&img, 8, 1), [0, 0, 0, 0], "at its native size");
    assert!(env.warnings().is_empty());
    // Frame 4 has no file: transparent, reported once however often shown.
    for _ in 0..2 {
        let img = deliver::render_env(cut, ColorLaw::Srgb, 4, 32, 18, &opts, &env);
        assert!(img.px.iter().all(|v| *v == 0.0));
    }
    assert_eq!(env.warnings().len(), 1, "{:?}", env.warnings());
    assert!(env.warnings()[0].contains("shot_0004.png"));
    // Hold last: frame 9 shows the nearest earlier file.
    let (held, at) = seq_rig("renders/shot_####.png", 0, true);
    let cut = held.project.cut(at.scene, at.cut).unwrap();
    let env = RenderEnv::new(Some(dir.clone()));
    let img = deliver::render_env(cut, ColorLaw::Srgb, 9, 32, 18, &opts, &env);
    assert_eq!(rgba8(&img, 1, 1), [255, 0, 0, 255]);
    assert!(env.warnings().is_empty());
    // No project folder (an unsaved project): a relative pattern reads
    // nothing — not the app's working folder — and says why, once.
    let env = RenderEnv::default();
    for _ in 0..2 {
        let img = deliver::render_env(cut, ColorLaw::Srgb, 3, 32, 18, &opts, &env);
        assert!(img.px.iter().all(|v| *v == 0.0));
    }
    assert_eq!(env.warnings(), vec![r3d::NO_FOLDER.to_string()]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_new_sequence_counts_from_one_and_a_picked_file_names_its_pattern() {
    let NodeKind::ImageSequence { first, offset, hold_last, fit, .. } = NodeKind::image_sequence("shot_####.png")
    else {
        unreachable!()
    };
    assert_eq!((first, offset, hold_last, fit), (1, 0, false, true), "file 1 at the first frame, fitted");
    // The sheet's frame 7 (index 6) shows shot_0007.png.
    let file = r3d::sequence_file("shot_####.png", first, offset, false, 6, None).unwrap();
    assert_eq!(file.to_string_lossy(), "shot_0007.png");
    // Picking one file of a render names the run.
    let base = std::env::temp_dir().join("gaban_project");
    let picked = base.join("renders").join("shot_0001.png");
    assert_eq!(r3d::pattern_from_file(&picked, Some(&base)), ("renders/shot_####.png".to_string(), 1));
    let (outside, n) = r3d::pattern_from_file(&std::env::temp_dir().join("elsewhere").join("bg_120.exr"), Some(&base));
    assert!(outside.ends_with("bg_###.exr") && Path::new(&outside).is_absolute(), "{outside}");
    assert_eq!(n, 120);
    assert_eq!(r3d::pattern_from_file(&base.join("0042.png"), Some(&base)), ("####.png".to_string(), 42));
    assert_eq!(r3d::pattern_from_file(&base.join("plate.png"), Some(&base)), ("plate.png".to_string(), 1));
}

#[test]
fn fit_scales_a_file_of_another_size_into_the_frame_centred() {
    let dir = std::env::temp_dir().join("anim_core_tests").join("r3d_fit");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("half_0001.png"), png_bytes(16, 9, [0, 0, 255, 255])).unwrap();
    std::fs::write(dir.join("square_0001.png"), png_bytes(8, 8, [0, 255, 0, 255])).unwrap();
    std::fs::write(dir.join("same_0001.png"), png_bytes(32, 18, [255, 0, 0, 128])).unwrap();
    let frame = |pattern: &str, fit: bool| {
        let mut warns = Vec::new();
        let px = r3d::sequence_frame(
            pattern,
            1,
            0,
            false,
            fit,
            0,
            Some(&dir),
            ColorLaw::Srgb,
            (32, 18),
            (0, 0, 32, 18),
            &mut |w| warns.push(w),
        );
        assert!(warns.is_empty(), "{warns:?}");
        Image { w: 32, h: 18, px }
    };
    // Half the frame's size: scaled 2×, the frame filled edge to edge.
    let img = frame("half_####.png", true);
    for (x, y) in [(0, 0), (31, 17), (16, 9)] {
        assert_eq!(rgba8(&img, x, y), [0, 0, 255, 255], "({x}, {y})");
    }
    // Without fit it sits at its own size from the top-left.
    let img = frame("half_####.png", false);
    assert_eq!(rgba8(&img, 15, 8), [0, 0, 255, 255]);
    assert_eq!(rgba8(&img, 16, 9), [0, 0, 0, 0]);
    // A square: as tall as the frame, centred, clear either side.
    let img = frame("square_####.png", true);
    assert_eq!(rgba8(&img, 16, 0), [0, 255, 0, 255]);
    assert_eq!(rgba8(&img, 16, 17), [0, 255, 0, 255]);
    assert_eq!(rgba8(&img, 2, 9), [0, 0, 0, 0]);
    assert_eq!(rgba8(&img, 29, 9), [0, 0, 0, 0]);
    // The frame's own size is native either way, its alpha kept.
    assert_eq!(frame("same_####.png", true).px, frame("same_####.png", false).px);
    assert_eq!(rgba8(&frame("same_####.png", true), 5, 5), [255, 0, 0, 128]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_project_from_before_gm17_still_loads_and_old_node_json_reads_with_defaults() {
    // Node JSON without the optional fields reads with their defaults (a
    // sequence saved before `fit` is native-size).
    let full = NodeKind::ImageSequence { pattern: "a_##.png".into(), first: 3, offset: 2, hold_last: true, fit: true };
    let mut json = serde_json::to_value(&full).unwrap();
    let inner = json.get_mut("ImageSequence").and_then(|v| v.as_object_mut()).expect("externally tagged");
    for key in ["first", "offset", "hold_last", "fit"] {
        inner.remove(key);
    }
    let old: NodeKind = serde_json::from_value(json).unwrap();
    assert_eq!(old, NodeKind::ImageSequence { pattern: "a_##.png".into(), first: 0, offset: 0, hold_last: false, fit: false });
    let mut json = serde_json::to_value(NodeKind::model3d(ModelId(7))).unwrap();
    let inner = json.get_mut("Model3D").and_then(|v| v.as_object_mut()).unwrap();
    inner.remove("params");
    inner.remove("binds");
    let NodeKind::Model3D { model, params, binds } = serde_json::from_value(json).unwrap() else { unreachable!() };
    assert_eq!(model, ModelId(7));
    assert!(params.0.is_empty() && binds.0.is_empty());
    assert_eq!(ModelValues::of(&params), ModelValues::of(&r3d::default_params()), "missing values read as defaults");
    // A v10 file: no cut_models table and no 3D nodes. It loads, with no
    // models.
    let r = rig();
    let dir = std::env::temp_dir().join(format!("animstudio_r3d_old_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.animproj");
    r.engine.save(&path).unwrap();
    assert_eq!(Engine::load(&path).unwrap().project, r.engine.project, "v11 round-trips");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("UPDATE meta SET value = '10' WHERE key = 'schema_version'", ()).unwrap();
        conn.execute("DROP TABLE cut_models", ()).unwrap();
        conn.execute("DELETE FROM links WHERE from_node = ?1", (r.node.0 as i64,)).unwrap();
        conn.execute("DELETE FROM nodes WHERE id = ?1", (r.node.0 as i64,)).unwrap();
    }
    let old = Engine::load(&path).unwrap();
    let cut = old.project.cut(r.at.scene, r.at.cut).unwrap();
    assert!(cut.models.is_empty(), "pre-v11: no models, and the load succeeds");
    assert!(cut.graph.node(r.node).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn jpeg_and_tiff_stills_decode_to_their_size_and_import_as_png() {
    let (w, h) = (12u32, 9u32);
    let rgb = image::RgbImage::from_pixel(w, h, image::Rgb([30, 140, 220]));
    for format in [image::ImageFormat::Jpeg, image::ImageFormat::Tiff] {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(rgb.clone()).write_to(&mut bytes, format).unwrap();
        let bytes = bytes.into_inner();
        let still = r3d::decode_still(&bytes).unwrap();
        assert_eq!((still.w, still.h), (w, h), "{format:?}");
        assert!(!still.linear);
        let png = r3d::still_to_png(&bytes).unwrap();
        let asset = ImageAsset::from_png(ImageId(1), "still", png).unwrap();
        assert_eq!((asset.width, asset.height), (w, h), "{format:?} imports as an 8-bit PNG");
    }
    let png = png_bytes(4, 3, [1, 2, 3, 255]);
    assert_eq!(r3d::still_to_png(&png).unwrap(), png, "a PNG is stored verbatim");
    assert!(r3d::decode_still(b"nope").is_err());
}

#[test]
fn the_model3d_parameters_have_defaults_and_a_new_node_renders_its_model() {
    let v = ModelValues::of(&FxParams::default());
    assert_eq!(v.values.len(), r3d::MODEL_SPECS.len());
    assert_eq!(v.toon().steps, 2);
    assert_eq!(v.choice("pass"), 0);
    // The default camera frames a unit model.
    let img = r3d::render(&cube(), &v, ColorLaw::Srgb, 64, 36);
    let covered = img.px.chunks_exact(4).filter(|p| p[3] > 0.0).count();
    assert!(covered > 64 * 36 / 8 && covered < 64 * 36, "{covered}");
    for s in r3d::MODEL_SPECS {
        assert!(r3d::spec(s.key).is_some());
    }
}
