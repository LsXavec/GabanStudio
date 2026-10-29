//! GM10: camera, paper and multiplane — the paper rect, the Camera node's
//! sample map (pan, T.U./T.B., rotate, shake, parallax, depth of field),
//! the field guide and safe frames, and today's Transform camera rendering
//! byte-identically.

use std::sync::Arc;

use anim_core::Engine;
use anim_core::camera::{self, CameraState, IRect};
use anim_core::color::ColorLaw;
use anim_core::command::{Command, CutRef};
use anim_core::export::{render_graph_frame, render_node_px, render_node_rect};
use anim_core::fx::{FxBinds, FxValue, Image, blur};
use anim_core::graph::{NodeKind, PlaneSpec, TransformBinds};
use anim_core::ids::*;
use anim_core::model::CelLayer;
use anim_core::raster::{TILE, TILE_LEN, TileData, f32_to_f16_bits};
use anim_core::xsheet::{Exposure, ParamInterp, ParamKey};

/// FNV-1a over bytes: the pinned-render fingerprint.
fn fnv(hash: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *hash ^= *b as u64;
        *hash = hash.wrapping_mul(0x100_0000_01b3);
    }
}

/// One cut with a column showing one single-layer drawing at frame 0.
struct Rig {
    engine: Engine,
    at: CutRef,
    col: ColumnId,
    d: DrawingId,
    layer: LayerId,
}

fn rig(frames: u32) -> Rig {
    let mut engine = Engine::new("camera tests");
    let scene = engine.add_scene("SC");
    let cut = engine.add_cut(scene, "cut", frames).unwrap();
    let at = CutRef { scene, cut };
    let col = engine.add_column(at, "BG").unwrap();
    let (d, layer) = (engine.alloc_drawing_id(), engine.alloc_layer_id());
    engine
        .apply(
            "rig",
            vec![
                Command::AddDrawing {
                    at,
                    id: d,
                    index: 0,
                    name: "bg".into(),
                    strokes: vec![],
                    layers: vec![CelLayer::new(layer, "paint")],
                },
                Command::SetCell { at, column: col, frame: 0, key: Some(Exposure::Drawing(d)) },
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
        let (at, id) = (self.at, self.engine.alloc_node_id());
        self.engine.apply("node", vec![Command::AddNode { at, id, kind }]).unwrap();
        id
    }

    fn connect(&mut self, from: NodeId, to: NodeId, pin: u16) {
        let at = self.at;
        let c = Command::Connect { at, from, from_pin: 0, to, to_pin: pin };
        self.engine.apply("connect", vec![c]).unwrap();
    }

    fn output(&mut self, from: NodeId) -> NodeId {
        let out = self.node(NodeKind::Output);
        self.connect(from, out, 0);
        let at = self.at;
        self.engine.apply("out", vec![Command::SetOutput { at, node: Some(out) }]).unwrap();
        out
    }

    /// Paint whole tiles of the drawing: `f(x, y)` is the premultiplied
    /// texel at paper pixel (x, y), for every tile in `tiles`.
    fn paint(&mut self, tiles: &[(i32, i32)], f: impl Fn(i64, i64) -> [f32; 4]) {
        let (d, layer) = (self.d, self.layer);
        self.paint_on(d, layer, tiles, f);
    }

    /// A second column with its own one-layer drawing at frame 0.
    fn column(&mut self, name: &str) -> (ColumnId, DrawingId, LayerId) {
        let at = self.at;
        let col = self.engine.add_column(at, name).unwrap();
        let (d, layer) = (self.engine.alloc_drawing_id(), self.engine.alloc_layer_id());
        let add = Command::AddDrawing {
            at,
            id: d,
            index: 0,
            name: name.into(),
            strokes: vec![],
            layers: vec![CelLayer::new(layer, "paint")],
        };
        let cell = Command::SetCell { at, column: col, frame: 0, key: Some(Exposure::Drawing(d)) };
        self.engine.apply("column", vec![add, cell]).unwrap();
        (col, d, layer)
    }

    fn paint_on(
        &mut self,
        d: DrawingId,
        layer: LayerId,
        tiles: &[(i32, i32)],
        f: impl Fn(i64, i64) -> [f32; 4],
    ) {
        let t = TILE as i64;
        let diff = tiles
            .iter()
            .map(|&(tx, ty)| {
                let mut px = vec![0u16; TILE_LEN];
                for y in 0..t {
                    for x in 0..t {
                        let v = f(tx as i64 * t + x, ty as i64 * t + y);
                        let i = ((y * t + x) * 4) as usize;
                        for c in 0..4 {
                            px[i + c] = f32_to_f16_bits(v[c]);
                        }
                    }
                }
                ((tx, ty), None, Some(Arc::new(TileData::from_vec(px))))
            })
            .collect();
        let (at, id) = (self.at, d);
        self.engine.apply("paint", vec![Command::PaintTiles { at, id, layer, diff }]).unwrap();
    }

    /// A Camera node over `planes` (source, depth), its parameters set, and
    /// wired to a new Output.
    fn camera(&mut self, planes: &[(NodeId, f32)], set: &[(&str, f32)]) -> (NodeId, NodeId) {
        let mut params = camera::default_params();
        for (k, v) in set {
            params.set(k, FxValue::Float(*v));
        }
        let planes_spec = planes.iter().map(|&(_, depth)| PlaneSpec { depth }).collect();
        let cam = self.node(NodeKind::Camera { planes: planes_spec, params, binds: FxBinds::default() });
        for (pin, &(src, _)) in planes.iter().enumerate() {
            self.connect(src, cam, pin as u16);
        }
        let out = self.output(cam);
        (cam, out)
    }

    fn set_paper(&mut self, rect: IRect, w: u32, h: u32) {
        let cmd = camera::set_paper(self.cut(), self.at, rect, w, h).unwrap();
        self.engine.apply("paper", vec![cmd]).unwrap();
    }

    fn set_kind(&mut self, id: NodeId, kind: NodeKind) {
        let at = self.at;
        self.engine.apply("kind", vec![Command::SetNodeKind { at, id, kind }]).unwrap();
    }

    fn hash(&mut self, frame: u32) -> u64 {
        self.engine.eval(self.at.scene, self.at.cut, frame).unwrap().hash()
    }

    fn render(&self, node: NodeId, frame: u32, w: u32, h: u32) -> Vec<f32> {
        render_node_px(self.cut(), ColorLaw::Legacy, node, frame, w, h)
    }
}

/// A single opaque white texel at (mx, my), transparent elsewhere.
fn marker(mx: i64, my: i64) -> impl Fn(i64, i64) -> [f32; 4] {
    move |x, y| if (x, y) == (mx, my) { [1.0; 4] } else { [0.0; 4] }
}

/// Pixel (x, y) of a premultiplied `w`-wide buffer.
fn at(px: &[f32], w: u32, x: u32, y: u32) -> [f32; 4] {
    let i = ((y * w + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// The most opaque pixel of a `w`-wide buffer.
fn brightest(px: &[f32], w: u32) -> (u32, u32, f32) {
    let (i, a) = px
        .chunks_exact(4)
        .map(|p| p[3])
        .enumerate()
        .fold((0, f32::MIN), |best, (i, a)| if a > best.1 { (i, a) } else { best });
    ((i as u32) % w, (i as u32) / w, a)
}

fn bits(px: &[f32]) -> Vec<u32> {
    px.iter().map(|v| v.to_bits()).collect()
}

/// A `w`-wide crop of a `src_w`-wide buffer starting at column `x0`.
fn crop(src: &[f32], src_w: u32, x0: u32, w: u32, h: u32) -> Vec<f32> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let s = ((y * src_w + x0) * 4) as usize;
        out.extend_from_slice(&src[s..s + (w * 4) as usize]);
    }
    out
}

impl Rig {
    fn param(&mut self, name: &str, keys: &[(u32, f32, ParamInterp)]) -> ParamId {
        let at = self.at;
        let column = self.engine.add_param_column(at, name).unwrap();
        for &(frame, value, interp) in keys {
            let key = Some(ParamKey { value, interp, handles: None });
            self.engine.apply("key", vec![Command::SetParamKey { at, column, frame, key }]).unwrap();
        }
        column
    }
}

/// A smooth, every-texel-different premultiplied pattern (alpha varies too).
fn pattern(x: i64, y: i64) -> [f32; 4] {
    let a = 0.25 + ((x * 7 + y * 3).rem_euclid(97) as f32) / 128.0;
    let r = ((x * 5).rem_euclid(251) as f32) / 251.0;
    let g = ((y * 11).rem_euclid(241) as f32) / 241.0;
    let b = (((x + y) * 13).rem_euclid(239) as f32) / 239.0;
    [r * a, g * a, b * a, a]
}

// ---- Today's camera keeps rendering byte-identically ------------------------------

/// The pre-GM10 render of a Transform bound to cam.x / cam.y / cam.scale
/// (frames 0, 5, 11, 17, 23 at 64×64: the premultiplied f32 bits of the
/// output node and the exported RGBA8), pinned at e9aaf6d, before this
/// part changed the renderer.
const TRANSFORM_CAMERA_FINGERPRINT: u64 = 0x977e_fc7f_926d_2028;

#[test]
fn a_transform_bound_to_cam_columns_renders_as_before_this_part() {
    let mut r = rig(24);
    r.paint(&[(0, 0)], pattern);
    let cam_x = r.param(
        "cam.x",
        &[(0, 0.0, ParamInterp::Linear), (11, 7.25, ParamInterp::Ease), (23, -3.5, ParamInterp::Hold)],
    );
    let cam_y = r.param("cam.y", &[(0, 2.0, ParamInterp::Linear), (23, -5.75, ParamInterp::Linear)]);
    let cam_scale =
        r.param("cam.scale", &[(0, 1.0, ParamInterp::Linear), (23, 1.5, ParamInterp::Ease)]);
    let src = r.node(NodeKind::source(r.col));
    let binds = TransformBinds { tx: Some(cam_x), ty: Some(cam_y), scale: Some(cam_scale), rotate: None };
    let xf = r.node(NodeKind::Transform { translate: (0.0, 0.0), scale: 1.0, rotate_deg: 3.0, binds });
    r.connect(src, xf, 0);
    let out = r.output(xf);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for frame in [0, 5, 11, 17, 23] {
        let px = render_node_px(r.cut(), ColorLaw::Legacy, out, frame, 64, 64);
        for v in &px {
            fnv(&mut hash, &v.to_bits().to_le_bytes());
        }
        fnv(&mut hash, &render_graph_frame(r.cut(), frame, 64, 64).unwrap());
    }
    assert_eq!(hash, TRANSFORM_CAMERA_FINGERPRINT, "the Transform camera's pixels moved: {hash:#018x}");
}

// ---- Paper -------------------------------------------------------------------------

#[test]
fn the_default_paper_is_the_frame_and_a_set_paper_reads_back_and_undoes() {
    let mut r = rig(48);
    let frame = IRect::frame(1920, 1080);
    assert_eq!(camera::paper(r.cut(), 1920, 1080), frame);
    assert_eq!(camera::paper_meta(r.cut()), None);
    assert!(camera::set_paper(r.cut(), r.at, frame, 1920, 1080).is_none(), "nothing changes");
    let wide = IRect { x: -64, y: 0, w: 3000, h: 1080 };
    r.set_paper(wide, 1920, 1080);
    assert_eq!(r.cut().meta.get(camera::PAPER_KEY).unwrap(), r#"{"x":-64,"y":0,"w":3000,"h":1080}"#);
    assert_eq!(camera::paper(r.cut(), 1920, 1080), wide);
    r.engine.undo().unwrap();
    assert_eq!(camera::paper(r.cut(), 1920, 1080), frame);
    assert!(!r.cut().meta.contains_key(camera::PAPER_KEY), "undo removes the key");
    r.engine.redo().unwrap();
    // Back to the frame stores no key at all.
    r.set_paper(frame, 1920, 1080);
    assert!(!r.cut().meta.contains_key(camera::PAPER_KEY));
    // Unreadable or impossible papers read as the frame; none can be set.
    for bad in ["not json", r#"{"x":0,"y":0,"w":0,"h":1080}"#, r#"{"x":0,"y":0,"w":99999,"h":10}"#] {
        let (at, before) = (r.at, r.cut().meta.get(camera::PAPER_KEY).cloned());
        let cmd = Command::SetCutMeta { at, key: camera::PAPER_KEY.into(), before, after: Some(bad.into()) };
        r.engine.apply("bad", vec![cmd]).unwrap();
        assert_eq!(camera::paper(r.cut(), 1920, 1080), frame, "{bad}");
    }
    assert!(camera::set_paper(r.cut(), r.at, IRect { x: 0, y: 0, w: 0, h: 5 }, 64, 64).is_none());
}

#[test]
fn a_paper_past_the_area_budget_is_refused_but_the_frame_never_is() {
    // A camera renders each plane over the whole paper in f32, so the paper
    // is bounded by area as well as by side: 16384² would need 4 GiB a buffer.
    let mut r = rig(48);
    let square = IRect { x: 0, y: 0, w: camera::MAX_PAPER, h: camera::MAX_PAPER };
    assert!(!square.is_valid_paper() && square.area() > camera::MAX_PAPER_AREA);
    assert!(camera::set_paper(r.cut(), r.at, square, 1920, 1080).is_none(), "refused");
    // 4K three frames wide is the budget's design case and still fits.
    let hiki = IRect { x: -3840, y: 0, w: 11520, h: 2160 };
    assert!(hiki.is_valid_paper() && hiki.area() <= camera::MAX_PAPER_AREA);
    r.set_paper(hiki, 3840, 2160);
    assert_eq!(camera::paper(r.cut(), 3840, 2160), hiki);
    // One pixel past the budget is refused; a stored one reads as the frame.
    let over = IRect { x: 0, y: 0, w: 8192, h: (camera::MAX_PAPER_AREA / 8192 + 1) as u32 };
    assert!(!over.is_valid_paper(), "{over:?}");
    let (at, before) = (r.at, r.cut().meta.get(camera::PAPER_KEY).cloned());
    let after = Some(camera::paper_to_json(over));
    r.engine.apply("over", vec![Command::SetCutMeta { at, key: camera::PAPER_KEY.into(), before, after }]).unwrap();
    assert_eq!(camera::paper(r.cut(), 3840, 2160), IRect::frame(3840, 2160));
    // A frame past the budget (DCI 8K) is still the paper it always was.
    let eight_k = IRect::frame(8192, 4320);
    assert!(!eight_k.is_valid_paper());
    assert!(camera::set_paper(r.cut(), r.at, eight_k, 8192, 4320).is_some(), "the frame clears the key");
}

#[test]
fn render_node_rect_at_the_frame_is_todays_render_and_a_wider_rect_shows_beyond_it() {
    // Criterion 1's engine half: no camera, the default paper — the rect
    // renderer at the frame is bit-for-bit the frame renderer.
    let mut r = rig(24);
    r.paint(&[(-1, 0), (0, 0), (1, 0)], pattern);
    let src = r.node(NodeKind::source(r.col));
    let xf = r.node(NodeKind::Transform {
        translate: (3.25, -1.5),
        scale: 1.2,
        rotate_deg: 7.0,
        binds: TransformBinds::default(),
    });
    r.connect(src, xf, 0);
    let law = ColorLaw::Legacy;
    for node in [src, xf] {
        let framed = render_node_rect(r.cut(), law, node, 0, IRect::frame(64, 64), 64, 64);
        assert_eq!(bits(&framed), bits(&r.render(node, 0, 64, 64)));
    }
    // A rect three tiles wide, starting a tile left of the frame: the frame
    // render sits at its middle tile, and the tiles either side show.
    let wide = render_node_rect(r.cut(), law, src, 0, IRect { x: -64, y: 0, w: 192, h: 64 }, 64, 64);
    assert_eq!(bits(&crop(&wide, 192, 64, 64, 64)), bits(&r.render(src, 0, 64, 64)));
    let left = at(&wide, 192, 10, 7);
    assert!((left[3] - pattern(-54, 7)[3]).abs() < 1e-3, "{left:?}");
    let right = at(&wide, 192, 150, 7);
    assert!((right[3] - pattern(86, 7)[3]).abs() < 1e-3, "{right:?}");
}

#[test]
fn a_pan_across_a_wide_paper_crops_it_pixel_exact_at_both_ends() {
    // Criterion 2: a 3000×1080 paper under a 1920×1080 frame, a BG painted
    // over all of it, and pan x keyed 0 → 1080 over 48 frames.
    let (w, h) = (1920, 1080);
    let mut r = rig(48);
    let paper = IRect { x: 0, y: 0, w: 3000, h: 1080 };
    r.set_paper(paper, w, h);
    let tiles: Vec<(i32, i32)> =
        (0..47).flat_map(|tx| (0..17).map(move |ty| (tx, ty))).collect();
    r.paint(&tiles, pattern);
    let pan = r.param("cam pan x", &[(0, 0.0, ParamInterp::Linear), (47, 1080.0, ParamInterp::Linear)]);
    let src = r.node(NodeKind::source(r.col));
    let (cam, out) = r.camera(&[(src, 0.0)], &[]);
    let NodeKind::Camera { planes, params, mut binds } = r.cut().graph.node(cam).unwrap().kind.clone()
    else {
        unreachable!()
    };
    binds.set("pan_x", Some(pan));
    r.set_kind(cam, NodeKind::Camera { planes, params, binds });
    let whole = render_node_rect(r.cut(), ColorLaw::Legacy, src, 0, paper, w, h);
    for (frame, x0) in [(0, 0), (47, 1080)] {
        assert_eq!(CameraState::at(r.cut(), cam, frame).unwrap().pan_x, x0 as f32);
        let shot = r.render(out, frame, w, h);
        assert!(
            bits(&shot) == bits(&crop(&whole, paper.w, x0, w, h)),
            "frame {frame} is not the paper crop at x = {x0}"
        );
    }
    // Between the ends the camera is between them too.
    let mid = CameraState::at(r.cut(), cam, 24).unwrap().pan_x;
    assert!(mid > 500.0 && mid < 600.0, "{mid}");
}

// ---- The Camera node's sample map ---------------------------------------------------------

#[test]
fn zoom_two_shows_the_centre_half_like_the_transform_reference() {
    // Criterion 3: zoom 2 (T.U.) against a Transform scaling by 2 — the
    // reference through the same bilinear sampler.
    let mut r = rig(24);
    r.paint(&[(0, 0)], pattern);
    let src = r.node(NodeKind::source(r.col));
    let (_, out) = r.camera(&[(src, 0.0)], &[("zoom", 2.0)]);
    let shot = r.render(out, 0, 64, 64);
    let xf = r.node(NodeKind::Transform {
        translate: (0.0, 0.0),
        scale: 2.0,
        rotate_deg: 0.0,
        binds: TransformBinds::default(),
    });
    r.connect(src, xf, 0);
    let reference = r.render(xf, 0, 64, 64);
    let worst = shot.iter().zip(&reference).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(worst <= 1.0 / 255.0, "zoom 2 differs from the reference by {worst}");
    // The corners of the shot are the centre half's corners: output (0, 0)
    // sees paper (16.25, 16.25).
    let st = CameraState::at(r.cut(), r.cut().graph.nodes().find(|n| n.kind.name() == "Camera").unwrap().id, 0)
        .unwrap();
    assert_eq!(st.sample_map(0.0, 64, 64, 0.5, 0.5), (16.25, 16.25));
    assert_eq!(st.sample_map(0.0, 64, 64, 63.5, 63.5), (47.75, 47.75));
}

#[test]
fn rotate_ninety_moves_a_marker_where_the_map_says() {
    // Criterion 4: the camera turned 90° clockwise turns the picture the
    // other way: paper (40, 10) lands at output (10, 63 − 40).
    let mut r = rig(24);
    r.paint(&[(0, 0)], marker(40, 10));
    let src = r.node(NodeKind::source(r.col));
    let (cam, out) = r.camera(&[(src, 0.0)], &[("rotate_deg", 90.0)]);
    let shot = r.render(out, 0, 64, 64);
    let (x, y, a) = brightest(&shot, 64);
    assert_eq!((x, y), (10, 23));
    assert!(a > 0.99, "{a}");
    let st = CameraState::at(r.cut(), cam, 0).unwrap();
    let (px, py) = st.sample_map(0.0, 64, 64, 10.5, 23.5);
    assert!((px - 40.5).abs() < 1e-4 && (py - 10.5).abs() < 1e-4, "({px}, {py})");
}

#[test]
fn shake_is_seeded_bounded_and_still_at_zero_amplitude() {
    // Criterion 5.
    let mut moved = 0;
    for seed in 0..16u32 {
        for t in 0..240 {
            let t = t as f32;
            let a = camera::shake(seed, t, 7.5, 0.3);
            assert_eq!(a, camera::shake(seed, t, 7.5, 0.3), "the same seed shakes the same");
            assert!((a.0 * a.0 + a.1 * a.1).sqrt() <= 7.5, "{a:?} leaves the amplitude");
            assert_eq!(camera::shake(seed, t, 0.0, 0.3), (0.0, 0.0), "amplitude 0 is still");
            assert_eq!(camera::shake(seed, t, 7.5, 0.0), (0.0, 0.0), "frequency 0 is still");
            if a != (0.0, 0.0) {
                moved += 1;
            }
        }
    }
    assert!(moved > 3000, "it shakes: {moved}");
    let (a, b) = ((0..48).map(|t| camera::shake(1, t as f32, 5.0, 0.25)), (0..48).map(|t| camera::shake(2, t as f32, 5.0, 0.25)));
    assert!(a.zip(b).any(|(a, b)| a != b), "another seed shakes otherwise");
    // In the render: a shaken camera renders the same twice, and its
    // offset is the frame's shake.
    let mut r = rig(24);
    r.paint(&[(0, 0)], pattern);
    let src = r.node(NodeKind::source(r.col));
    let (cam, out) = r.camera(&[(src, 0.0)], &[("shake_amp", 3.0), ("shake_seed", 9.0)]);
    assert_eq!(bits(&r.render(out, 5, 64, 64)), bits(&r.render(out, 5, 64, 64)));
    let st = CameraState::at(r.cut(), cam, 5).unwrap();
    assert_eq!(st.shake(), camera::shake(9, 5.0, 3.0, 0.25));
    let (u, v) = (20.5, 30.5);
    let still = CameraState { shake_amp: 0.0, ..st };
    let (sx, sy) = st.shake();
    let (a, b) = (st.sample_map(0.0, 64, 64, u, v), still.sample_map(0.0, 64, 64, u, v));
    assert!((a.0 - b.0 - sx).abs() < 1e-4 && (a.1 - b.1 - sy).abs() < 1e-4);
}

#[test]
fn two_planes_at_depth_0_and_2_with_k_half_move_100_and_50_under_a_100_px_pan() {
    // Criterion 6: f = 1/(1 + k·z) — 1 at z 0, 1/2 at z 2.
    let (w, h) = (128, 64);
    let mut r = rig(24);
    r.set_paper(IRect { x: 0, y: 0, w: 256, h: 64 }, w, h);
    r.paint(&[(0, 0), (1, 0), (2, 0), (3, 0)], marker(150, 10));
    let (col_far, d_far, l_far) = r.column("far");
    r.paint_on(d_far, l_far, &[(0, 0), (1, 0), (2, 0), (3, 0)], marker(150, 40));
    let near = r.node(NodeKind::source(r.col));
    let far = r.node(NodeKind::source(col_far));
    let (cam, out) = r.camera(&[(near, 0.0), (far, 2.0)], &[("parallax_k", 0.5)]);
    let st = CameraState::at(r.cut(), cam, 0).unwrap();
    assert_eq!((st.parallax(0.0), st.parallax(2.0)), (1.0, 0.5));
    // At rest the camera frames x 0..128: both markers (x 150) are off it.
    assert_eq!(brightest(&r.render(out, 0, w, h), w).2, 0.0);
    let NodeKind::Camera { planes, mut params, binds } = r.cut().graph.node(cam).unwrap().kind.clone()
    else {
        unreachable!()
    };
    params.set("pan_x", FxValue::Float(100.0));
    r.set_kind(cam, NodeKind::Camera { planes, params, binds });
    let shot = r.render(out, 0, w, h);
    assert!(at(&shot, w, 50, 10)[3] > 0.99, "z 0 moved 100 px: 150 → 50");
    assert!(at(&shot, w, 100, 40)[3] > 0.99, "z 2 moved 50 px: 150 → 100");
    let lit = shot.chunks_exact(4).filter(|p| p[3] > 0.0).count();
    assert_eq!(lit, 2, "nothing else shows");
}

#[test]
fn depth_of_field_blurs_a_plane_by_aperture_times_its_distance_from_focus() {
    // Criterion 7: radius = aperture·|z − focus_z|, the FX core blur.
    assert_eq!(camera::dof_radius(2.0, 3.0, 1.0), 4.0);
    assert_eq!(camera::dof_radius(0.0, 9.0, 1.0), 0.0);
    let mut r = rig(24);
    r.paint(&[(0, 0)], marker(30, 30));
    let src = r.node(NodeKind::source(r.col));
    let sharp = r.render(src, 0, 64, 64);
    let shot = |r: &mut Rig, z: f32, set: &[(&str, f32)]| {
        let (_, out) = r.camera(&[(src, z)], set);
        r.render(out, 0, 64, 64)
    };
    let in_focus = shot(&mut r, 1.0, &[("aperture", 2.0), ("focus_z", 1.0)]);
    assert_eq!(bits(&in_focus), bits(&sharp), "at the focus: no blur");
    let no_aperture = shot(&mut r, 3.0, &[("aperture", 0.0), ("focus_z", 1.0)]);
    assert_eq!(bits(&no_aperture), bits(&sharp), "aperture 0: no blur");
    let off = shot(&mut r, 3.0, &[("aperture", 2.0), ("focus_z", 1.0)]);
    let expected = blur(&Image::from_px(64, 64, sharp.clone()), 4.0, 4.0).px;
    assert_eq!(bits(&off), bits(&expected), "two units off focus: the radius-4 blur");
    assert!(at(&off, 64, 30, 30)[3] < 0.5, "and it did blur");
}

#[test]
fn a_tile_at_negative_coordinates_renders_through_the_camera() {
    // Criterion 11: paper from x −64; a texel painted at frame x −30
    // (tile −1) shows once the camera pans 64 px left.
    let mut r = rig(24);
    r.paint(&[(-1, 0)], marker(-30, 20));
    let src = r.node(NodeKind::source(r.col));
    let (cam, out) = r.camera(&[(src, 0.0)], &[("pan_x", -64.0)]);
    // On the default paper there is nothing left of the frame to see.
    assert_eq!(brightest(&r.render(out, 0, 64, 64), 64).2, 0.0);
    r.set_paper(IRect { x: -64, y: 0, w: 128, h: 64 }, 64, 64);
    let shot = r.render(out, 0, 64, 64);
    let (x, y, a) = brightest(&shot, 64);
    assert_eq!((x, y), (34, 20), "−30 + 64");
    assert!(a > 0.99);
    let corners = camera::frame_rect_on_paper(&CameraState::at(r.cut(), cam, 0).unwrap(), 64, 64);
    assert_eq!(corners[0], (-64.0, 0.0));
}

#[test]
fn frame_rect_on_paper_follows_pan_zoom_and_rotate() {
    let st = CameraState::defaults(0);
    assert_eq!(camera::frame_rect_on_paper(&st, 1920, 1080), [(0.0, 0.0), (1920.0, 0.0), (1920.0, 1080.0), (0.0, 1080.0)]);
    let panned = CameraState { pan_x: 300.0, pan_y: -40.0, ..st };
    assert_eq!(camera::frame_rect_on_paper(&panned, 1920, 1080)[0], (300.0, -40.0));
    assert_eq!(camera::frame_rect_on_paper(&panned, 1920, 1080)[2], (2220.0, 1040.0));
    // T.U. 2: the centre half; T.B. 0.5: twice the frame about its centre.
    let tu = CameraState { zoom: 2.0, ..st };
    assert_eq!(camera::frame_rect_on_paper(&tu, 1920, 1080)[0], (480.0, 270.0));
    let tb = CameraState { zoom: 0.5, ..st };
    assert_eq!(camera::frame_rect_on_paper(&tb, 1920, 1080)[2], (2880.0, 1620.0));
    // Turned 90° clockwise, the frame's top-left corner sits top-right.
    let turned = CameraState { rotate_deg: 90.0, ..st };
    let tl = camera::frame_rect_on_paper(&turned, 64, 64)[0];
    assert!((tl.0 - 64.0).abs() < 1e-4 && tl.1.abs() < 1e-4, "{tl:?}");
}

// ---- Field guide and safe frames ------------------------------------------------------------

#[test]
fn field_guide_and_safe_frames_are_pinned_for_hd() {
    // Criterion 9's engine half.
    let fields = camera::field_guide(1920, 1080);
    assert_eq!(fields.len(), 12);
    for (i, f) in fields.iter().enumerate() {
        let n = (i + 1) as f32;
        let want = camera::Rect { x: 960.0 - 80.0 * n, y: 540.0 - 45.0 * n, w: 160.0 * n, h: 90.0 * n };
        assert_eq!(*f, want, "field {n}");
    }
    assert_eq!(fields[11], camera::Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 });
    let safe = camera::safe_frames(1920, 1080);
    let near = |a: camera::Rect, b: [f32; 4]| {
        [a.x, a.y, a.w, a.h].iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    };
    assert!(near(safe.action, [67.2, 37.8, 1785.6, 1004.4]), "{:?}", safe.action);
    assert!(near(safe.title, [96.0, 54.0, 1728.0, 972.0]), "{:?}", safe.title);
}

// ---- Binds, the eval hash and the store ----------------------------------------------------------

#[test]
fn a_bound_camera_animates_and_the_eval_hash_follows_it_and_the_paper() {
    let mut r = rig(24);
    r.paint(&[(0, 0)], pattern);
    let src = r.node(NodeKind::source(r.col));
    let (cam, _) = r.camera(&[(src, 0.0)], &[]);
    let h0 = r.hash(0);
    assert_eq!(r.hash(7), h0, "a still camera hashes alike on every frame");
    let pan = r.param("cam pan x", &[(0, 0.0, ParamInterp::Linear), (10, 50.0, ParamInterp::Linear)]);
    let NodeKind::Camera { planes, params, mut binds } = r.cut().graph.node(cam).unwrap().kind.clone()
    else {
        unreachable!()
    };
    assert!(!r.cut().graph.sources_of_param(pan).contains(&cam));
    binds.set("pan_x", Some(pan));
    r.set_kind(cam, NodeKind::Camera { planes: planes.clone(), params: params.clone(), binds: binds.clone() });
    assert!(r.cut().graph.sources_of_param(pan).contains(&cam), "a key edit invalidates the camera");
    assert_eq!(CameraState::at(r.cut(), cam, 5).unwrap().pan_x, 25.0);
    let (a, b, c) = (r.hash(0), r.hash(5), r.hash(10));
    assert!(a != b && b != c, "the pan moves the hash per frame");
    assert_eq!(r.hash(20), c, "held after the last key");
    // The paper and the planes' depths reach the hash; undo brings it back.
    r.set_paper(IRect { x: 0, y: 0, w: 128, h: 64 }, 64, 64);
    assert_ne!(r.hash(5), b);
    r.engine.undo().unwrap();
    assert_eq!(r.hash(5), b);
    r.set_kind(cam, NodeKind::Camera { planes: vec![PlaneSpec { depth: 2.0 }], params, binds });
    assert_ne!(r.hash(5), b);
    // A shaking camera's frames differ though its parameters hold still.
    let mut shaky = camera::default_params();
    shaky.set("shake_amp", FxValue::Float(4.0));
    r.set_kind(cam, NodeKind::Camera { planes, params: shaky, binds: FxBinds::default() });
    assert_ne!(r.hash(3), r.hash(4));
}

#[test]
fn camera_nodes_and_the_paper_round_trip_through_the_store() {
    let mut r = rig(24);
    r.paint(&[(-1, 0), (0, 0)], pattern);
    let (col_b, _, _) = r.column("B");
    let src = r.node(NodeKind::source(r.col));
    let src_b = r.node(NodeKind::source(col_b));
    let (cam, out) = r.camera(&[(src, 2.5), (src_b, -0.5)], &[("zoom", 1.25), ("aperture", 1.5)]);
    let pan = r.param("cam pan y", &[(0, -12.0, ParamInterp::Ease)]);
    let NodeKind::Camera { planes, params, mut binds } = r.cut().graph.node(cam).unwrap().kind.clone()
    else {
        unreachable!()
    };
    binds.set("pan_y", Some(pan));
    r.set_kind(cam, NodeKind::Camera { planes, params, binds });
    r.set_paper(IRect { x: -64, y: -8, w: 200, h: 90 }, 64, 64);
    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("camera_roundtrip.animproj");
    r.engine.save(&path).unwrap();
    let back = Engine::load(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let (a, b) = (r.cut(), back.project.cut(r.at.scene, r.at.cut).unwrap());
    assert_eq!(a.graph, b.graph, "planes, depths, params, binds and wires survive");
    assert_eq!(camera::paper(b, 64, 64), IRect { x: -64, y: -8, w: 200, h: 90 });
    let law = ColorLaw::Legacy;
    assert_eq!(
        bits(&render_node_px(a, law, out, 0, 64, 64)),
        bits(&render_node_px(b, law, out, 0, 64, 64)),
        "and the loaded file renders the same"
    );
}
