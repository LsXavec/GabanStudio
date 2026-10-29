//! GM9 compositing FX: blur, glow, diffusion and grade (anim_core::fx), the
//! Effect node through the CPU golden renderer, the eval hash and the store.
//! The formulas are pinned in fx.rs's header; these tests pin them in turn.

use anim_core::Engine;
use anim_core::color::ColorLaw;
use anim_core::command::{Command, CutRef};
use anim_core::export::{graph_composite_pixel, srgb_to_linear, straight_rgba8};
use anim_core::fx::{
    FxBinds, FxKind, FxParams, FxValue, FxValues, Image, LUMA, ParamKind, TEMP_GAIN, blur,
    box_sizes,
};
use anim_core::graph::{self, NodeKind};
use anim_core::ids::*;
use anim_core::xsheet::{ParamInterp, ParamKey};

// ---- helpers -------------------------------------------------------------------

/// An effect's values: its defaults with `set` written over them.
fn values(kind: FxKind, set: &[(&str, FxValue)]) -> FxValues {
    let mut p = FxParams::defaults(kind);
    for (k, v) in set {
        p.set(k, v.clone());
    }
    FxValues::resolve(kind, &p, &FxBinds::default(), |_| None)
}

fn run(kind: FxKind, set: &[(&str, FxValue)], src: &Image) -> Image {
    kind.apply(&values(kind, set), ColorLaw::Legacy, &[Some(src)], src.w, src.h)
}

/// A transparent `w × h` frame with one opaque white pixel.
fn dot(w: u32, h: u32, x: u32, y: u32) -> Image {
    let mut img = Image::new(w, h);
    img.set_pixel(x, y, [1.0; 4]);
    img
}

/// A frame from straight RGBA8, premultiplied (what the tiles hold).
fn from_rgba8(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
    let mut img = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let c = f(x, y);
            let a = c[3] as f32 / 255.0;
            img.set_pixel(
                x,
                y,
                [
                    c[0] as f32 / 255.0 * a,
                    c[1] as f32 / 255.0 * a,
                    c[2] as f32 / 255.0 * a,
                    a,
                ],
            );
        }
    }
    img
}

fn bytes(img: &Image) -> Vec<[u8; 4]> {
    img.px.chunks_exact(4).map(straight_rgba8).collect()
}

/// A colourful test frame with opaque, part-covered and empty pixels.
fn swatches() -> Image {
    from_rgba8(16, 16, |x, y| {
        let a = match (x + y) % 4 {
            0 => 255,
            1 => 128,
            2 => 0,
            _ => 37,
        };
        [(x * 16) as u8, (y * 16) as u8, ((x * y) % 256) as u8, a]
    })
}

// ---- Blur ------------------------------------------------------------------------

#[test]
fn blur_of_a_white_dot_is_symmetric_and_keeps_its_energy() {
    let src = dot(65, 65, 32, 32);
    let out = blur(&src, 6.0, 6.0);
    for c in 0..4 {
        let sum: f32 = out.px.iter().skip(c).step_by(4).sum();
        assert!((sum - 1.0).abs() < 1e-3, "channel {c} sums to {sum}");
    }
    for dy in 0..12u32 {
        for dx in 0..12u32 {
            let v = out.pixel(32 + dx, 32 + dy)[0];
            for (x, y) in [(32 - dx, 32 + dy), (32 + dx, 32 - dy), (32 - dx, 32 - dy), (32 + dy, 32 + dx)] {
                assert!((out.pixel(x, y)[0] - v).abs() < 1e-6, "asymmetric at ({dx},{dy})");
            }
        }
    }
    assert!(out.pixel(32, 32)[0] < 0.1, "the dot spread out");
}

#[test]
fn blur_radius_zero_is_the_identity() {
    let src = swatches();
    assert_eq!(blur(&src, 0.0, 0.0), src);
    assert_eq!(run(FxKind::Blur, &[("radius_x", FxValue::Float(0.0)), ("radius_y", FxValue::Float(0.0))], &src), src);
    // One axis at a time: a horizontal blur never moves energy across rows.
    let only_x = blur(&dot(33, 9, 16, 4), 4.0, 0.0);
    for y in [0, 3, 5, 8] {
        assert!(only_x.px[(y * 33 * 4) as usize..((y + 1) * 33 * 4) as usize].iter().all(|&v| v == 0.0));
    }
}

#[test]
fn blur_variance_is_the_boxes_for_gauss_variance() {
    // A box of odd width w has variance (w² − 1)/12; three of them add up.
    for sigma in [0.5f32, 1.0, 2.5, 3.0, 7.3, 40.0] {
        let sizes = box_sizes(sigma, 3);
        assert!(sizes.iter().all(|w| w % 2 == 1), "odd widths {sizes:?}");
        let var: f64 = sizes.iter().map(|&w| ((w * w) as f64 - 1.0) / 12.0).sum();
        let wl = sizes[0] as f64;
        let s2 = (sigma as f64).powi(2);
        assert!((var - s2).abs() <= (wl + 1.0) / 6.0 + 1e-9, "σ {sigma}: {var} vs {s2}");
    }
    assert_eq!(box_sizes(0.0, 3), vec![1, 1, 1], "σ 0 is the identity");
    // The measured spread of a blurred dot is exactly that variance.
    let out = blur(&dot(81, 1, 40, 0), 6.0, 0.0);
    let want: f64 = box_sizes(3.0, 3).iter().map(|&w| ((w * w) as f64 - 1.0) / 12.0).sum();
    let got: f64 = (0..81u32)
        .map(|x| (x as f64 - 40.0).powi(2) * out.pixel(x, 0)[0] as f64)
        .sum();
    assert!((got - want).abs() < 1e-3, "variance {got} vs {want}");
}

#[test]
fn blur_outside_the_image_is_transparent() {
    let mut src = Image::new(32, 32);
    src.px.fill(1.0);
    let out = blur(&src, 4.0, 4.0);
    assert!((out.pixel(16, 16)[3] - 1.0).abs() < 1e-6, "the inside stays covered");
    assert!(out.pixel(0, 16)[3] < 0.9, "the edge fades: {:?}", out.pixel(0, 16));
    assert!(out.pixel(0, 0)[3] < out.pixel(0, 16)[3], "a corner fades more");
}

#[test]
fn blur_repeat_edges_keeps_a_full_frame_full() {
    // Off by default; on, the samples past the frame are its edge pixels, so
    // a flat opaque frame stays flat and opaque to the corners.
    assert!(!values(FxKind::Blur, &[]).flag("repeat_edges"));
    let src = from_rgba8(32, 32, |_, _| [200, 100, 50, 255]);
    let out = run(FxKind::Blur, &[("repeat_edges", FxValue::Bool(true))], &src);
    for (o, s) in out.px.iter().zip(&src.px) {
        assert!((o - s).abs() < 1e-6, "{o} vs {s}");
    }
    let faded = run(FxKind::Blur, &[], &src);
    assert!(faded.pixel(0, 0)[3] < 0.9, "off, the corner fades");
    // A dot on the edge keeps more of itself: what spreads past the edge
    // comes back as the edge pixel, where transparency would lose it.
    let mut edge = Image::new(16, 1);
    edge.set_pixel(0, 0, [1.0; 4]);
    let kept = run(FxKind::Blur, &[("repeat_edges", FxValue::Bool(true))], &edge);
    let lost = run(FxKind::Blur, &[], &edge);
    assert!(kept.pixel(0, 0)[3] > lost.pixel(0, 0)[3]);
}

// ---- Glow ------------------------------------------------------------------------

#[test]
fn glow_leaves_an_image_below_the_threshold_unchanged() {
    let src = from_rgba8(24, 24, |x, y| [100, 90, (x * 4) as u8, if y % 3 == 0 { 255 } else { 120 }]);
    let out = run(FxKind::Glow, &[("threshold", FxValue::Float(0.7))], &src);
    assert_eq!(out, src);
}

#[test]
fn glow_halo_falls_off_monotonically_from_a_bright_dot() {
    let src = dot(49, 49, 24, 24);
    let out = run(
        FxKind::Glow,
        &[("threshold", FxValue::Float(0.5)), ("radius", FxValue::Float(8.0))],
        &src,
    );
    assert!(out.pixel(27, 24)[0] > 0.0 && out.pixel(27, 24)[3] > 0.0, "a halo with coverage");
    assert!(out.pixel(24, 24)[0] > 1.0, "the dot gains its own glow (Add)");
    for d in 1..20u32 {
        let (near, far) = (out.pixel(24 + d, 24)[0], out.pixel(24 + d + 1, 24)[0]);
        assert!(far <= near + 1e-7, "falloff rises at {d}: {near} -> {far}");
    }
    // The halo is the dot's brightness, blurred: bright = (1 − 0.5)/0.5 = 1.
    let halo = blur(&src, 8.0, 8.0);
    assert!((out.pixel(30, 24)[1] - halo.pixel(30, 24)[1]).abs() < 1e-6);
}

#[test]
fn glow_intensity_zero_is_the_identity_and_the_matte_gates_it() {
    let src = dot(33, 33, 16, 16);
    let off = run(FxKind::Glow, &[("intensity", FxValue::Float(0.0))], &src);
    assert_eq!(off, src);
    // A wired matte that covers nothing lets no glow out; a full one is no
    // restriction at all.
    let v = values(FxKind::Glow, &[("threshold", FxValue::Float(0.2))]);
    let none = Image::new(33, 33);
    let mut full = Image::new(33, 33);
    full.px.fill(1.0);
    let gated = FxKind::Glow.apply(&v, ColorLaw::Legacy, &[Some(&src), Some(&none)], 33, 33);
    assert_eq!(gated, src);
    let open = FxKind::Glow.apply(&v, ColorLaw::Legacy, &[Some(&src), Some(&full)], 33, 33);
    let free = FxKind::Glow.apply(&v, ColorLaw::Legacy, &[Some(&src), None], 33, 33);
    assert_eq!(open, free);
    assert_ne!(free, src);
}

// ---- Diffusion ----------------------------------------------------------------------

#[test]
fn diffusion_mix_zero_is_the_identity() {
    let src = swatches();
    for mode in ["screen", "overlay"] {
        let out = run(
            FxKind::Diffusion,
            &[("mix", FxValue::Float(0.0)), ("mode", FxValue::Choice(mode.into()))],
            &src,
        );
        assert_eq!(out, src, "{mode}");
    }
    // A file from before the rename says "soft light": it reads as overlay.
    let old = values(FxKind::Diffusion, &[("mode", FxValue::Choice("soft light".into()))]);
    assert_eq!(old.choice("mode"), 1);
    assert_eq!(old, values(FxKind::Diffusion, &[("mode", FxValue::Choice("overlay".into()))]));
}

#[test]
fn diffusion_mix_one_matches_the_pinned_values_on_a_ramp() {
    // A horizontal ramp c = x/63, opaque. Inside it the blur of a linear
    // ramp is the ramp, so screen(c, c) = 2c − c² and the Overlay curve
    // gives 2c² below one half, 1 − 2(1 − c)² above.
    let mut ramp = Image::new(64, 64);
    for y in 0..64 {
        for x in 0..64 {
            let v = x as f32 / 63.0;
            ramp.set_pixel(x, y, [v, v, v, 1.0]);
        }
    }
    let c = |x: u32| ramp.pixel(x, 32)[0];
    let pinned_screen = [(16u32, 1760.0f32 / 3969.0), (32, 3008.0 / 3969.0), (47, 3713.0 / 3969.0)];
    let pinned_soft = [(16u32, 512.0f32 / 3969.0), (32, 2047.0 / 3969.0), (47, 3457.0 / 3969.0)];
    for (mode, pinned) in [("screen", pinned_screen), ("overlay", pinned_soft)] {
        let out = run(
            FxKind::Diffusion,
            &[
                ("radius", FxValue::Float(4.0)),
                ("mix", FxValue::Float(1.0)),
                ("mode", FxValue::Choice(mode.into())),
            ],
            &ramp,
        );
        for (x, want) in pinned {
            let v = c(x);
            let exact = if mode == "screen" {
                2.0 * v - v * v
            } else if v <= 0.5 {
                2.0 * v * v
            } else {
                1.0 - 2.0 * (1.0 - v) * (1.0 - v)
            };
            let got = out.pixel(x, 32);
            assert!((got[0] - exact).abs() < 1e-5, "{mode} x {x}: {} vs {exact}", got[0]);
            assert!((got[0] - want).abs() < 1e-5, "{mode} x {x}: {} vs pinned {want}", got[0]);
            assert!((got[3] - 1.0).abs() < 1e-6);
        }
    }
}

#[test]
fn diffusion_matches_the_pinned_values_where_the_soft_copy_differs() {
    // An opaque grey field (1/4) with one bright pixel (3/4). Radius 2 is
    // σ 1, one 3-wide box per axis, so the soft copy d is the 3×3 mean:
    // 11/36 on and beside the bright pixel, 1/4 away from it. Screen is
    // a + d − ad; overlay 2ad where a ≤ 1/2, else 1 − 2(1 − a)(1 − d).
    assert_eq!(box_sizes(1.0, 3), vec![1, 1, 3]);
    let mut field = Image::new(9, 9);
    for y in 0..9 {
        for x in 0..9 {
            let v = if (x, y) == (4, 4) { 0.75 } else { 0.25 };
            field.set_pixel(x, y, [v, v, v, 1.0]);
        }
    }
    // (x on row 4, pinned value at mix 1): the bright pixel, its neighbour,
    // and a pixel whose window misses it.
    let pinned = [
        ("screen", [(4u32, 119.0f32 / 144.0), (5, 69.0 / 144.0), (2, 7.0 / 16.0)]),
        ("overlay", [(4u32, 47.0 / 72.0), (5, 11.0 / 72.0), (2, 1.0 / 8.0)]),
    ];
    for (mode, rows) in pinned {
        for mix in [1.0f32, 0.5] {
            let out = run(
                FxKind::Diffusion,
                &[
                    ("radius", FxValue::Float(2.0)),
                    ("mix", FxValue::Float(mix)),
                    ("mode", FxValue::Choice(mode.into())),
                ],
                &field,
            );
            for (x, want) in rows {
                let a = field.pixel(x, 4)[0];
                let expect = a + (want - a) * mix;
                let got = out.pixel(x, 4);
                for c in 0..3 {
                    assert!((got[c] - expect).abs() < 1e-6, "{mode} mix {mix} x {x}: {got:?} vs {expect}");
                }
                assert!((got[3] - 1.0).abs() < 1e-6);
            }
        }
    }
}

// ---- Grade -----------------------------------------------------------------------------

#[test]
fn grade_neutral_is_the_identity_on_an_rgba8_round_trip() {
    let src = swatches();
    let out = run(FxKind::Grade, &[], &src);
    assert_eq!(bytes(&out), bytes(&src));
    // The defaults are the neutral settings.
    let v = FxValues::defaults(FxKind::Grade);
    for (k, n) in [("lift", 0.0), ("gamma", 1.0), ("gain", 1.0), ("saturation", 1.0), ("exposure", 0.0), ("temperature", 0.0), ("tint", 0.0)] {
        assert_eq!(v.float(k), n, "{k}");
    }
}

#[test]
fn grade_saturation_zero_gives_grey() {
    let src = swatches();
    let out = run(FxKind::Grade, &[("saturation", FxValue::Float(0.0))], &src);
    for (o, s) in out.px.chunks_exact(4).zip(src.px.chunks_exact(4)) {
        if s[3] == 0.0 {
            assert_eq!(o, s);
            continue;
        }
        let luma = (LUMA[0] * s[0] + LUMA[1] * s[1] + LUMA[2] * s[2]) / s[3] * s[3];
        for c in 0..3 {
            assert!((o[c] - luma).abs() < 1e-5, "{o:?} is not grey {luma}");
        }
        assert_eq!(o[3], s[3]);
    }
}

#[test]
fn grade_exposure_and_the_rest_match_the_formula() {
    let src = swatches();
    let out = run(FxKind::Grade, &[("exposure", FxValue::Float(1.0))], &src);
    for (o, s) in out.px.chunks_exact(4).zip(src.px.chunks_exact(4)) {
        for c in 0..3 {
            assert!((o[c] - 2.0 * s[c]).abs() < 1e-5, "{o:?} vs 2×{s:?}");
        }
        assert_eq!(o[3], s[3]);
    }
    // Every term at once on one half-covered pixel.
    let px = from_rgba8(1, 1, |_, _| [200, 100, 50, 128]);
    let set = [
        ("lift", 0.1),
        ("gamma", 2.0),
        ("gain", 1.2),
        ("saturation", 0.5),
        ("exposure", -0.5),
        ("temperature", 0.4),
        ("tint", -0.2),
    ];
    let got = run(FxKind::Grade, &set.map(|(k, v)| (k, FxValue::Float(v))), &px).pixel(0, 0);
    let a = px.pixel(0, 0)[3];
    let mut c = [0usize, 1, 2].map(|i| px.pixel(0, 0)[i] / a);
    for ch in &mut c {
        *ch = (1.2 * (*ch + 0.1 * (1.0 - *ch))).max(0.0).powf(0.5) * 2f32.powf(-0.5);
    }
    let l = LUMA[0] * c[0] + LUMA[1] * c[1] + LUMA[2] * c[2];
    let gains = [1.0 + 0.4 * TEMP_GAIN, 1.0 + 0.2 * TEMP_GAIN, 1.0 - 0.4 * TEMP_GAIN];
    for i in 0..3 {
        let want = (l + (c[i] - l) * 0.5) * gains[i] * a;
        assert!((got[i] - want).abs() < 1e-5, "channel {i}: {} vs {want}", got[i]);
    }
}

// ---- The Effect node in the graph ---------------------------------------------------

/// A cut whose graph is Solid → Effect(`kind`) → Output, and a param column.
struct Rig {
    engine: Engine,
    at: CutRef,
    solid: NodeId,
    fx: NodeId,
    param: ParamId,
}

fn rig(kind: FxKind) -> Rig {
    let mut engine = Engine::new("fx tests");
    let scene = engine.add_scene("SC");
    let cut = engine.add_cut(scene, "cut", 24).unwrap();
    let at = CutRef { scene, cut };
    let param = engine.add_param_column(at, "fx.exposure").unwrap();
    let (solid, fx, out) = (engine.alloc_node_id(), engine.alloc_node_id(), engine.alloc_node_id());
    engine
        .apply(
            "rig",
            vec![
                Command::AddNode { at, id: solid, kind: NodeKind::Solid { rgba: [64, 128, 192, 255] } },
                Command::AddNode { at, id: fx, kind: NodeKind::effect(kind) },
                Command::AddNode { at, id: out, kind: NodeKind::Output },
                Command::Connect { at, from: solid, from_pin: 0, to: fx, to_pin: 0 },
                Command::Connect { at, from: fx, from_pin: 0, to: out, to_pin: 0 },
                Command::SetOutput { at, node: Some(out) },
            ],
        )
        .unwrap();
    Rig { engine, at, solid, fx, param }
}

impl Rig {
    fn kind(&self) -> NodeKind {
        let cut = self.engine.project.cut(self.at.scene, self.at.cut).unwrap();
        cut.graph.node(self.fx).unwrap().kind.clone()
    }

    fn set_kind(&mut self, kind: NodeKind) {
        let (at, id) = (self.at, self.fx);
        self.engine.apply("params", vec![Command::SetNodeKind { at, id, kind }]).unwrap();
    }

    fn key(&mut self, frame: u32, value: f32) {
        let (at, column) = (self.at, self.param);
        let key = Some(ParamKey { value, interp: ParamInterp::Linear, handles: None });
        self.engine.apply("key", vec![Command::SetParamKey { at, column, frame, key }]).unwrap();
    }

    fn pixel(&self, frame: u32) -> [f32; 4] {
        let cut = self.engine.project.cut(self.at.scene, self.at.cut).unwrap();
        graph_composite_pixel(cut, frame, 8, 8, 4, 4).unwrap()
    }

    fn hash(&mut self, frame: u32) -> u64 {
        self.engine.eval(self.at.scene, self.at.cut, frame).unwrap().hash()
    }
}

#[test]
fn a_bound_parameter_animates_formula_exact_between_keys() {
    let mut r = rig(FxKind::Grade);
    let NodeKind::Effect { fx, params, mut binds } = r.kind() else { unreachable!() };
    binds.set("exposure", Some(r.param));
    r.set_kind(NodeKind::Effect { fx, params, binds });
    r.key(0, 0.0);
    r.key(10, 1.0);
    // The Solid's chip enters through the Legacy law (sRGB → linear).
    let base = [64u8, 128, 192].map(srgb_to_linear);
    let (f0, f5, f10) = (r.pixel(0), r.pixel(5), r.pixel(10));
    for c in 0..3 {
        assert!((f0[c] - base[c]).abs() < 1e-6, "frame 0 is exposure 0");
        assert!((f10[c] - base[c] * 2.0).abs() < 1e-6, "frame 10 is exposure +1");
        assert!((f5[c] - base[c] * 0.5f32.exp2()).abs() < 1e-6, "frame 5 is halfway");
    }
    assert_ne!(f0, f10);
    // A bound value outside the spec's range is clamped into it.
    r.key(10, 50.0);
    assert!((r.pixel(10)[0] - base[0] * 10f32.exp2()).abs() < 1e-3);
}

#[test]
fn a_bound_tint_mixes_its_two_chips_by_the_column() {
    // A colour's column (0–1) mixes its stored chip toward its second chip
    // (`tint.to`), per byte, rounded.
    let mut r = rig(FxKind::Glow);
    let NodeKind::Effect { fx, mut params, mut binds } = r.kind() else { unreachable!() };
    let spec = FxKind::Glow.spec("tint").unwrap();
    assert!(spec.bindable());
    params.set("threshold", FxValue::Float(0.0));
    params.set("tint", FxValue::Color([255, 0, 0, 255]));
    params.set(&spec.to_key(), FxValue::Color([0, 0, 255, 255]));
    binds.set("tint", Some(r.param));
    r.set_kind(NodeKind::Effect { fx, params: params.clone(), binds: binds.clone() });
    r.key(0, 0.0);
    r.key(10, 1.0);
    let tint = |r: &Rig, frame: u32| {
        let cut = r.engine.project.cut(r.at.scene, r.at.cut).unwrap();
        cut.fx_at(fx, &params, &binds, frame).color("tint")
    };
    assert_eq!(tint(&r, 0), [255, 0, 0, 255]);
    assert_eq!(tint(&r, 5), [128, 0, 128, 255], "halfway: 127.5 rounds up");
    assert_eq!(tint(&r, 10), [0, 0, 255, 255]);
    let (f0, f10) = (r.pixel(0), r.pixel(10));
    assert!(f0[0] > f10[0] && f10[2] > f0[2], "red glow, then blue: {f0:?} {f10:?}");
    // Frame 10 renders exactly as the second chip, static.
    let mut fixed = params.clone();
    fixed.set("tint", FxValue::Color([0, 0, 255, 255]));
    r.set_kind(NodeKind::Effect { fx, params: fixed, binds: FxBinds::default() });
    assert_eq!(r.pixel(10), f10);
}

#[test]
fn eval_hash_follows_parameters_and_bound_values() {
    let mut r = rig(FxKind::Blur);
    let h0 = r.hash(0);
    assert_eq!(r.hash(0), h0, "stable");
    assert_eq!(r.hash(3), h0, "nothing animates: every frame alike");
    let NodeKind::Effect { fx, mut params, binds } = r.kind() else { unreachable!() };
    params.set("radius_x", FxValue::Float(9.5));
    r.set_kind(NodeKind::Effect { fx, params: params.clone(), binds: binds.clone() });
    let h1 = r.hash(0);
    assert_ne!(h1, h0, "a parameter edit changes the hash");
    r.engine.undo().unwrap();
    assert_eq!(r.hash(0), h0, "undo brings the hash back");
    r.engine.redo().unwrap();
    // Bind radius_y; a key on the bound column changes the value, a key on
    // a column nobody reads does not.
    let mut b = binds.clone();
    b.set("radius_y", Some(r.param));
    r.set_kind(NodeKind::Effect { fx, params, binds: b });
    let h2 = r.hash(0);
    assert_eq!(h2, h1, "a bound column with no keys falls back to the stored value");
    r.key(0, 2.0);
    let h3 = r.hash(0);
    assert_ne!(h3, h2, "the bound value changed");
    let other = r.engine.add_param_column(r.at, "unrelated").unwrap();
    let (at, key) = (r.at, Some(ParamKey { value: 7.0, interp: ParamInterp::Hold, handles: None }));
    r.engine.apply("k", vec![Command::SetParamKey { at, column: other, frame: 0, key }]).unwrap();
    assert_eq!(r.hash(0), h3, "an unread column leaves it alone");
    // The solid under it still reaches the hash.
    let (at, id) = (r.at, r.solid);
    r.engine
        .apply("c", vec![Command::SetNodeKind { at, id, kind: NodeKind::Solid { rgba: [1, 2, 3, 255] } }])
        .unwrap();
    assert_ne!(r.hash(0), h3);
}

#[test]
fn effect_nodes_and_the_graph_layout_round_trip_through_the_store() {
    let mut r = rig(FxKind::Glow);
    let NodeKind::Effect { fx, mut params, mut binds } = r.kind() else { unreachable!() };
    params.set("tint", FxValue::Color([255, 200, 120, 255]));
    params.set("intensity", FxValue::Float(1.75));
    binds.set("radius", Some(r.param));
    r.set_kind(NodeKind::Effect { fx, params, binds });
    // The glow's matte pin, wired; a grade beside it with a choice-less spec.
    let (at, grade, diff) = (r.at, r.engine.alloc_node_id(), r.engine.alloc_node_id());
    let (solid, glow) = (r.solid, r.fx);
    let mut dkind = NodeKind::effect(FxKind::Diffusion);
    if let NodeKind::Effect { params, .. } = &mut dkind {
        params.set("mode", FxValue::Choice("soft light".into()));
    }
    let cut = r.engine.project.cut(at.scene, at.cut).unwrap();
    let place_glow = graph::place_node(cut, at, glow, Some((12.5, -40.0))).unwrap();
    let place_grade = graph::place_node(cut, at, grade, Some((300.0, 80.25))).unwrap();
    r.engine
        .apply(
            "more",
            vec![
                Command::AddNode { at, id: grade, kind: NodeKind::effect(FxKind::Grade) },
                Command::AddNode { at, id: diff, kind: dkind },
                Command::Connect { at, from: solid, from_pin: 0, to: glow, to_pin: 1 },
                place_glow,
                place_grade,
            ],
        )
        .unwrap();
    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fx_roundtrip.animproj");
    r.engine.save(&path).unwrap();
    let back = Engine::load(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let (a, b) = (
        r.engine.project.cut(at.scene, at.cut).unwrap(),
        back.project.cut(at.scene, at.cut).unwrap(),
    );
    assert_eq!(a.graph, b.graph, "every Effect, param, bind and wire survives");
    let pos = graph::layout_from_meta(&b.meta);
    assert_eq!(pos.get(&glow), Some(&(12.5, -40.0)));
    assert_eq!(pos.get(&grade), Some(&(300.0, 80.25)));
    assert_eq!(pos.len(), 2, "a node never placed has no key");
    // And the loaded file renders the same.
    assert_eq!(
        graph_composite_pixel(a, 0, 8, 8, 4, 4),
        graph_composite_pixel(b, 0, 8, 8, 4, 4)
    );
}

#[test]
fn two_artists_place_nodes_apart_and_undo_their_own_add_alone() {
    // Each node's place is its own meta key: artist A's add (node and place)
    // and artist B's move of another node, both made against the same
    // state, both land; undoing A's add takes A's node and place and leaves
    // B's move standing.
    let mut r = rig(FxKind::Blur);
    let (at, blur) = (r.at, r.fx);
    let added = r.engine.alloc_node_id();
    let cut = r.engine.project.cut(at.scene, at.cut).unwrap();
    let a_place = graph::place_node(cut, at, added, Some((300.0, 40.0))).unwrap();
    let b_place = graph::place_node(cut, at, blur, Some((120.0, 90.0))).unwrap();
    let before = r.pixel(0);
    r.engine.set_author(Some("A".into()));
    let add = Command::AddNode { at, id: added, kind: NodeKind::effect(FxKind::Glow) };
    r.engine.apply("add node", vec![add, a_place]).unwrap();
    r.engine.set_author(Some("B".into()));
    r.engine.apply("move node", vec![b_place]).unwrap();
    r.engine.set_author(None);
    let layout = |r: &Rig| {
        let cut = r.engine.project.cut(r.at.scene, r.at.cut).unwrap();
        graph::layout_from_meta(&cut.meta)
    };
    assert_eq!(layout(&r), [(blur, (120.0, 90.0)), (added, (300.0, 40.0))].into());
    assert_eq!(r.engine.undo_last_by(Some("A")).unwrap(), "add node");
    let cut = r.engine.project.cut(at.scene, at.cut).unwrap();
    assert!(cut.graph.node(added).is_err(), "A's node is gone");
    assert_eq!(layout(&r), [(blur, (120.0, 90.0))].into(), "B's move stands");
    // B can still take back its own move; a place renders nothing.
    assert_eq!(r.engine.undo_last_by(Some("B")).unwrap(), "move node");
    assert!(layout(&r).is_empty());
    assert_eq!(r.pixel(0), before);
}

#[test]
fn old_node_json_loads_unchanged_and_missing_params_take_defaults() {
    // Pre-GM9 shapes still parse as they always did.
    let t: NodeKind =
        serde_json::from_str(r#"{"Transform":{"translate":[1.0,2.0],"scale":1.0,"rotate_deg":0.0}}"#).unwrap();
    assert!(matches!(t, NodeKind::Transform { binds, .. } if binds == Default::default()));
    let s: NodeKind = serde_json::from_str(r#"{"Solid":{"rgba":[1,2,3,4]}}"#).unwrap();
    assert_eq!(s, NodeKind::Solid { rgba: [1, 2, 3, 4] });
    // An Effect with no params (or params from a later build) reads its
    // defaults for what is missing and ignores what it does not know.
    let e: NodeKind = serde_json::from_str(r#"{"Effect":{"fx":"Blur"}}"#).unwrap();
    let NodeKind::Effect { fx, params, binds } = &e else { panic!("{e:?}") };
    assert_eq!(fx.inputs(), 1);
    let v = FxValues::resolve(*fx, params, binds, |_| None);
    assert_eq!(v, FxValues::defaults(FxKind::Blur));
    let e: NodeKind = serde_json::from_str(
        r#"{"Effect":{"fx":"Diffusion","params":{"mix":0.9,"mode":"no such mode","future":true,"radius":"wrong type"},"binds":{}}}"#,
    )
    .unwrap();
    let NodeKind::Effect { fx, params, binds } = &e else { panic!("{e:?}") };
    let v = FxValues::resolve(*fx, params, binds, |_| None);
    assert_eq!(v.float("mix"), 0.9);
    assert_eq!(v.choice("mode"), 0, "an unknown choice reads the default");
    assert_eq!(v.float("radius"), 12.0, "a wrong-typed value reads the default");
    // Every kind's specs are sane: unique keys, defaults inside the range.
    for kind in FxKind::ALL {
        let specs = kind.specs();
        for (i, s) in specs.iter().enumerate() {
            assert!(specs[i + 1..].iter().all(|o| o.key != s.key), "{kind:?} repeats {}", s.key);
            assert!(!s.help.is_empty());
            if let ParamKind::Float { min, max, default, step } = s.kind {
                assert!(min <= default && default <= max && step > 0.0, "{kind:?}.{}", s.key);
            }
            if let ParamKind::Choice { options, default, renamed } = s.kind {
                let named = |i: usize| i < options.len();
                assert!(named(default) && renamed.iter().all(|(_, i)| named(*i)), "{kind:?}.{}", s.key);
            }
        }
        assert_eq!(NodeKind::effect(*kind).input_count(), kind.inputs());
        assert_eq!(NodeKind::effect(*kind).name(), kind.name());
    }
}

#[test]
fn a_param_key_invalidates_the_effect_that_reads_it() {
    let mut r = rig(FxKind::Grade);
    let NodeKind::Effect { fx, params, mut binds } = r.kind() else { unreachable!() };
    assert!(!r.kind().references(r.param));
    binds.set("gain", Some(r.param));
    r.set_kind(NodeKind::Effect { fx, params, binds });
    assert!(r.kind().references(r.param));
    let cut = r.engine.project.cut(r.at.scene, r.at.cut).unwrap();
    assert_eq!(cut.graph.sources_of_param(r.param), vec![r.fx]);
    // Through the cache: a keyed gain re-renders without any other edit.
    let before = r.pixel(0);
    r.key(0, 0.5);
    let after = r.pixel(0);
    assert!((after[0] - before[0] * 0.5).abs() < 1e-6, "{before:?} -> {after:?}");
    let h = r.hash(0);
    r.key(0, 0.25);
    assert_ne!(r.hash(0), h, "the memoised value was dropped");
}
