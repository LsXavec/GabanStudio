//! GM18: manga pages — page sizes and guides, panels and their gutters,
//! the page graph's clip, screen tone, perspective rulers and page export.

use std::sync::Arc;

use anim_core::Engine;
use anim_core::color::ColorLaw;
use anim_core::command::{Command, CutRef};
use anim_core::deliver::{self, PageFormat, PageMode, PageOpts};
use anim_core::export::{graph_composite, render_node_px};
use anim_core::fx::{FxBinds, FxKind, FxParams, FxValue, FxValues, Image};
use anim_core::generator::{GenKind, GenValues, Target};
use anim_core::graph::NodeKind;
use anim_core::ids::*;
use anim_core::manga::{self, DotShape, Page, Poly, PxRect, Rulers, Screen, Snap};
use anim_core::model::{CelLayer, Cut, LayerRole};
use anim_core::raster::{TILE, TILE_LEN, TileData, f16_bits_to_f32, f32_to_f16_bits};
use anim_core::xsheet::Exposure;

const RED: [f32; 4] = [0.8, 0.1, 0.1, 1.0];

// ---- Helpers ----------------------------------------------------------------------

/// A tile with `rgba` in the box x0..x1 × y0..y1.
fn block(x0: usize, x1: usize, y0: usize, y1: usize, rgba: [f32; 4]) -> Arc<TileData> {
    let mut px = vec![0u16; TILE_LEN];
    for y in y0..y1.min(TILE) {
        for x in x0..x1.min(TILE) {
            for c in 0..4 {
                px[(y * TILE + x) * 4 + c] = f32_to_f16_bits(rgba[c]);
            }
        }
    }
    Arc::new(TileData::from_vec(px))
}

/// `rgba` as it reads back from a tile (f16).
fn stored(rgba: [f32; 4]) -> [f32; 4] {
    rgba.map(|v| f16_bits_to_f32(f32_to_f16_bits(v)))
}

fn px(img: &[f32], w: usize, x: usize, y: usize) -> [f32; 4] {
    let i = (y * w + x) * 4;
    [img[i], img[i + 1], img[i + 2], img[i + 3]]
}

/// A one-frame page cut whose INK column is RED all over its first tile,
/// its graph the page graph of `panels` (a `border` px border).
fn page_rig(panels: Vec<Poly>, border: f32) -> (Engine, CutRef, ColumnId) {
    let mut e = Engine::new("manga tests");
    let scene = e.add_scene("book");
    let c = e.add_cut(scene, "P01", 1).unwrap();
    let at = CutRef { scene, cut: c };
    let ink = e.add_column(at, "INK").unwrap();
    let (d, l) = (e.alloc_drawing_id(), e.alloc_layer_id());
    e.apply(
        "art",
        vec![
            Command::AddDrawing {
                at,
                id: d,
                index: 0,
                name: "ink".into(),
                strokes: vec![],
                layers: vec![CelLayer::with_role(l, LayerRole::Color.key(), LayerRole::Color)],
            },
            Command::PaintTiles {
                at,
                id: d,
                layer: l,
                diff: vec![((0, 0), None, Some(block(0, 64, 0, 64, RED)))],
            },
            Command::SetCell {
                at,
                column: ink,
                frame: 0,
                key: Some(Exposure::Drawing(d)),
            },
        ],
    )
    .unwrap();
    let cut = e.project.cut(scene, c).unwrap().clone();
    let cmds = manga::page_graph(&cut, at, || e.alloc_node_id(), panels, border);
    e.apply("page graph", cmds).unwrap();
    (e, at, ink)
}

fn tone_values(set: &[(&str, FxValue)]) -> FxValues {
    let mut params = FxParams::defaults(FxKind::Tone);
    for (k, v) in set {
        params.set(k, v.clone());
    }
    FxValues::resolve(FxKind::Tone, &params, &FxBinds::default(), |_| None)
}

/// An opaque flat grey.
fn flat(w: u32, h: u32, grey: f32) -> Image {
    Image::from_px(w, h, [grey, grey, grey, 1.0].repeat((w * h) as usize))
}

fn tone(v: &FxValues, src: &Image) -> Image {
    FxKind::Tone.apply(v, ColorLaw::Srgb, &[Some(src)], src.w, src.h)
}

/// The share of the frame the tone inks.
fn coverage(img: &Image) -> f32 {
    img.px.chunks_exact(4).map(|p| p[3]).sum::<f32>() / (img.w * img.h) as f32
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
}

fn min_max(p: &Poly, axis: usize) -> (f32, f32) {
    p.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| (lo.min(q[axis]), hi.max(q[axis])))
}

// ---- Pages ----------------------------------------------------------------------------

#[test]
fn manga_b5_page_is_2591_by_3624_with_its_trim_and_inner_frame() {
    let p = Page::b5();
    assert_eq!((p.width, p.height, p.dpi), (2591, 3624, 350.0));
    assert_eq!(p.trim, PxRect { x: 41, y: 41, w: 2508, h: 3542 });
    assert_eq!(p.inner, PxRect { x: 262, y: 296, w: 2067, h: 3032 });
    assert_eq!(p.segment, None);
    assert_eq!(p.first_panel(), manga::rect_poly(p.inner));
}

#[test]
fn manga_a4_page_is_2976_by_4175_with_its_trim_and_inner_frame() {
    let p = Page::a4();
    assert_eq!((p.width, p.height, p.dpi), (2976, 4175, 350.0));
    assert_eq!(p.trim, PxRect { x: 41, y: 41, w: 2894, h: 4093 });
    assert_eq!(p.inner, PxRect { x: 248, y: 227, w: 2480, h: 3721 });
}

#[test]
fn manga_mm_round_to_px_at_350_dpi() {
    assert_eq!(manga::mm_px(5.0, 350.0), 69);
    assert_eq!(manga::mm_px(2.0, 350.0), 28);
    assert_eq!(manga::mm_px(0.5, 350.0), 7);
    assert_eq!(manga::mm_px(3.0, 350.0), 41);
    assert_eq!(deliver::px_per_metre(350.0), 13780);
    assert_eq!(deliver::px_per_metre(72.0), 2835);
}

#[test]
fn manga_page_lives_in_the_cut_meta_and_a_plain_cut_is_no_page() {
    let mut cut = Cut::new(CutId(1), "P01", 1);
    assert_eq!(Page::of(&cut), None);
    cut.meta.insert(manga::PAGE_KEY.into(), Page::a4().to_meta());
    assert_eq!(Page::of(&cut), Some(Page::a4()));
    let w = Page::webtoon();
    assert_eq!((w.width, w.height, w.segment), (800, 6400, Some(1280)));
    assert_eq!(w.first_panel(), vec![[0.0, 0.0], [800.0, 0.0], [800.0, 6400.0], [0.0, 6400.0]]);
}

// ---- Panels ---------------------------------------------------------------------------

#[test]
fn manga_split_across_leaves_a_69_px_gutter_in_one_undo_step() {
    let page = Page::b5();
    let full = page.first_panel();
    let (mut e, at, _) = page_rig(vec![full.clone()], 7.0);
    let cut = e.project.cut(at.scene, at.cut).unwrap();
    let (id, polys) = manga::panels_of(cut).unwrap();
    assert_eq!(polys, vec![full.clone()]);
    let mid = (page.inner.y + page.inner.h / 2) as f32;
    let (a, b) = ([0.0, mid], [page.width as f32, mid]);
    let gutter = manga::gutter_for(a, b, manga::GUTTER_SIDE_MM, manga::GUTTER_ROW_MM, page.dpi);
    assert_eq!(gutter, 69.0);
    let (top, bottom) = manga::split_panel(&full, a, b, gutter).unwrap();
    let ((t0, t1), (b0, b1)) = (min_max(&top, 1), min_max(&bottom, 1));
    assert_eq!(b0 - t1, 69.0, "{top:?} {bottom:?}");
    assert_eq!((t0, b1), (page.inner.y as f32, page.inner.bottom() as f32));
    assert_eq!(min_max(&top, 0), min_max(&full, 0));
    let depth = e.history_len();
    let cmd = manga::set_panels(cut, at, id, vec![top.clone(), bottom.clone()]).unwrap();
    e.apply("split panel", vec![cmd]).unwrap();
    assert_eq!(e.history_len(), depth + 1);
    let cut = e.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(manga::panels_of(cut).unwrap().1, vec![top, bottom]);
    e.undo().unwrap();
    let cut = e.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(manga::panels_of(cut).unwrap().1, vec![full]);
}

#[test]
fn manga_split_down_takes_the_side_gutter_and_a_miss_splits_nothing() {
    let full = manga::rect_poly(PxRect { x: 10, y: 10, w: 100, h: 50 });
    let (a, b) = ([60.0, -5.0], [60.0, 80.0]);
    let g = manga::gutter_for(a, b, 2.0, 5.0, 350.0);
    assert_eq!(g, 28.0);
    let (left, right) = manga::split_panel(&full, a, b, g).unwrap();
    assert_eq!(min_max(&right, 0).0 - min_max(&left, 0).1, 28.0);
    assert_eq!((min_max(&left, 0).0, min_max(&right, 0).1), (10.0, 110.0));
    assert!(manga::split_panel(&full, [200.0, 0.0], [200.0, 100.0], g).is_none(), "the line misses");
    assert!(manga::split_panel(&full, [5.0, 5.0], [5.0, 5.0], g).is_none(), "no line");
    // A slanted cut: the halves are the panel less the gutter's strip.
    let (l, r) = manga::split_panel(&full, [40.0, 0.0], [80.0, 70.0], 10.0).unwrap();
    let chord = (50.0f32 * (40.0f32.hypot(70.0) / 70.0)).abs();
    let left_over = manga::area(&l).abs() + manga::area(&r).abs();
    assert!((left_over - (5000.0 - 10.0 * chord)).abs() < 0.5, "{left_over}");
    assert!(manga::contains(&l, [108.0, 12.0]) != manga::contains(&r, [108.0, 12.0]));
}

#[test]
fn manga_panels_draw_white_insides_borders_and_clear_gutters() {
    let a = manga::rect_poly(PxRect { x: 2, y: 2, w: 12, h: 28 });
    let b = manga::rect_poly(PxRect { x: 18, y: 2, w: 12, h: 28 });
    let kind = manga::panels_kind(vec![a, b], 1.0);
    let NodeKind::Generator { generator, params, binds } = &kind else {
        panic!("{kind:?}")
    };
    assert_eq!(*generator, GenKind::Panels);
    let v = GenValues::resolve(GenKind::Panels, params, binds, |_| None);
    let img = anim_core::generator::render(&v, ColorLaw::Srgb, None, 0, Target::frame(32, 32));
    assert_eq!(img.pixel(8, 16), [1.0; 4], "inside");
    assert_eq!(img.pixel(2, 16), [0.0, 0.0, 0.0, 1.0], "border");
    assert_eq!(img.pixel(16, 16), [0.0; 4], "gutter");
    assert_eq!(img.pixel(0, 0), [0.0; 4]);
    let polys = v.polys("panels").to_vec();
    assert_eq!(manga::panel_at(&polys, [20.0, 10.0]), Some(1));
    assert_eq!(manga::panel_at(&polys, [16.0, 10.0]), None);
    assert_eq!(
        manga::drag_panel([30.4, 10.6], [5.2, 40.0]),
        Some(vec![[5.0, 11.0], [30.0, 11.0], [30.0, 40.0], [5.0, 40.0]])
    );
    assert_eq!(manga::drag_panel([1.0, 1.0], [2.0, 50.0]), None, "too thin");
    // The polygons round-trip as `{"polys": ...}` (D5: old values unchanged).
    let value = FxValue::Polys { polys };
    let json = serde_json::to_string(&value).unwrap();
    assert!(json.starts_with("{\"polys\":"), "{json}");
    assert_eq!(serde_json::from_str::<FxValue>(&json).unwrap(), value);
}

// ---- The page graph -------------------------------------------------------------------

#[test]
fn manga_page_graph_clips_the_art_to_the_panels() {
    let panel = manga::rect_poly(PxRect { x: 8, y: 8, w: 32, h: 32 });
    let (e, at, _) = page_rig(vec![panel], 2.0);
    let cut = e.project.cut(at.scene, at.cut).unwrap();
    let img = graph_composite(cut, ColorLaw::Srgb, 0, 64, 64).unwrap();
    for (x, y) in [(2, 2), (50, 20), (20, 50), (63, 63)] {
        assert_eq!(px(&img, 64, x, y), [1.0; 4], "outside ({x}, {y}): the bare paper");
    }
    for (x, y) in [(10, 10), (20, 20), (37, 37)] {
        assert_eq!(px(&img, 64, x, y), stored(RED), "inside ({x}, {y}): the art as it is");
    }
    for (x, y) in [(8, 20), (9, 20), (39, 20), (20, 8), (20, 39)] {
        assert_eq!(px(&img, 64, x, y), [0.0, 0.0, 0.0, 1.0], "border ({x}, {y})");
    }
    // The column's own clip: clear outside, untouched inside.
    let matte = cut
        .graph
        .nodes()
        .find(|n| matches!(n.kind, NodeKind::Effect { fx: FxKind::Matte, .. }))
        .unwrap()
        .id;
    let clip = render_node_px(cut, ColorLaw::Srgb, matte, 0, 64, 64);
    assert_eq!(px(&clip, 64, 2, 2), [0.0; 4]);
    assert_eq!(px(&clip, 64, 20, 20), stored(RED));
}

#[test]
fn manga_page_graph_is_one_step_and_a_rebuild_keeps_panels_and_tone() {
    let panel = manga::rect_poly(PxRect { x: 4, y: 4, w: 40, h: 40 });
    let (mut e, at, _) = page_rig(vec![panel.clone()], 2.0);
    let tone_column = e.add_column(at, "TONE").unwrap();
    let cut = e.project.cut(at.scene, at.cut).unwrap().clone();
    let depth = e.history_len();
    let cmds = manga::page_graph(&cut, at, || e.alloc_node_id(), Vec::new(), 2.0);
    e.apply("clip to panels", cmds).unwrap();
    assert_eq!(e.history_len(), depth + 1);
    let cut = e.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(manga::panels_of(cut).unwrap().1, vec![panel], "the panels carry over");
    let tone = cut
        .graph
        .nodes()
        .find(|n| matches!(n.kind, NodeKind::Effect { fx: FxKind::Tone, .. }))
        .expect("the TONE column goes through a Tone");
    let (src, _) = tone.inputs[0].unwrap();
    assert!(
        matches!(cut.graph.node(src).unwrap().kind, NodeKind::DrawingSource { column, .. } if column == tone_column)
    );
    let (id, mut kind) = (tone.id, tone.kind.clone());
    if let NodeKind::Effect { params, .. } = &mut kind {
        params.set("lpi", FxValue::Float(85.0));
    }
    e.apply("lpi", vec![Command::SetNodeKind { at, id, kind }]).unwrap();
    let cut = e.project.cut(at.scene, at.cut).unwrap().clone();
    let cmds = manga::page_graph(&cut, at, || e.alloc_node_id(), Vec::new(), 2.0);
    e.apply("again", cmds).unwrap();
    let cut = e.project.cut(at.scene, at.cut).unwrap();
    let lpi = cut.graph.nodes().find_map(|n| match &n.kind {
        NodeKind::Effect { fx: FxKind::Tone, params, .. } => params.get("lpi").cloned(),
        _ => None,
    });
    assert_eq!(lpi, Some(FxValue::Float(85.0)), "the tone's values carry over");
    for _ in 0..3 {
        e.undo().unwrap();
    }
    let cut = e.project.cut(at.scene, at.cut).unwrap();
    assert!(!cut.graph.nodes().any(|n| matches!(n.kind, NodeKind::Effect { fx: FxKind::Tone, .. })));
}

// ---- Screen tone ----------------------------------------------------------------------

#[test]
fn manga_tone_covers_the_grey_at_60_lpi_45_degrees() {
    let v = tone_values(&[]);
    assert_eq!((v.float("lpi"), v.float("angle"), v.float("dpi")), (60.0, 45.0, 350.0));
    for pct in [10, 30, 50, 70, 90] {
        let out = tone(&v, &flat(128, 128, 1.0 - pct as f32 / 100.0));
        let c = coverage(&out) * 100.0;
        assert!((c - pct as f32).abs() <= 3.0, "{pct}%: {c}%");
    }
    // The other dots cover a half grey too.
    for shape in ["square", "line"] {
        let out = tone(&tone_values(&[("shape", FxValue::Choice(shape.into()))]), &flat(128, 128, 0.5));
        let c = coverage(&out) * 100.0;
        assert!((c - 50.0).abs() <= 3.0, "{shape}: {c}%");
    }
}

#[test]
fn manga_tone_is_bare_at_white_solid_at_black_and_1_bit_when_mono() {
    let v = tone_values(&[]);
    assert!(tone(&v, &flat(64, 64, 1.0)).px.iter().all(|&c| c == 0.0), "0%: no dots");
    assert!(tone(&v, &Image::new(64, 64)).px.iter().all(|&c| c == 0.0), "clear: no dots");
    let solid = tone(&v, &flat(64, 64, 0.0));
    assert!(solid.px.chunks_exact(4).all(|p| p == [0.0, 0.0, 0.0, 1.0]), "100%: solid ink");
    let mid = tone(&v, &flat(64, 64, 0.5));
    assert!(mid.px.chunks_exact(4).all(|p| p[3] == 0.0 || p == [0.0, 0.0, 0.0, 1.0]), "1-bit");
    let soft = tone(&tone_values(&[("mono", FxValue::Bool(false))]), &flat(64, 64, 0.5));
    assert!(soft.px.chunks_exact(4).any(|p| p[3] > 0.0 && p[3] < 1.0), "smoothed edges");
    // The ink is the tone's colour.
    let blue = tone(&tone_values(&[("color", FxValue::Color([0, 0, 255, 255]))]), &flat(8, 8, 0.0));
    assert_eq!(blue.pixel(3, 3), [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn manga_tone_dot_grid_turns_with_the_angle() {
    let s = Screen::new(350.0, 60.0, 45.0, DotShape::Round);
    assert_eq!(s.cell, 350.0 / 60.0);
    assert!(close(s.dot_centre(1, 0), [4.124_79, 4.124_79]), "{:?}", s.dot_centre(1, 0));
    assert!(close(s.dot_centre(0, 1), [-4.124_79, 4.124_79]), "{:?}", s.dot_centre(0, 1));
    let flat0 = Screen::new(350.0, 60.0, 0.0, DotShape::Round);
    assert!(close(flat0.dot_centre(1, 0), [5.833_33, 0.0]));
    assert!(close(flat0.dot_centre(2, 3), [11.666_67, 17.5]));
    // A light tone inks each dot centre and not half a cell along the row.
    let step = s.dot_centre(1, 0);
    for (i, j) in [(0, 0), (1, 0), (0, 1), (3, -2), (5, 7)] {
        let p = s.dot_centre(i, j);
        assert!(s.ink(p[0], p[1], 0.1), "dot ({i}, {j})");
        assert!(!s.ink(p[0] + step[0] / 2.0, p[1] + step[1] / 2.0, 0.1), "between ({i}, {j})");
    }
}

// ---- Perspective rulers ---------------------------------------------------------------

#[test]
fn manga_snap_one_point_takes_the_nearest_of_its_line_the_horizontal_and_the_vertical() {
    let r = Rulers { vps: vec![[100.0, 100.0]], horizon: Some(100.0) };
    let pick = |to: [f32; 2]| manga::snap_direction([0.0, 0.0], to, &r, 6.0).unwrap().dir;
    let h = std::f32::consts::FRAC_1_SQRT_2;
    assert!(close(pick([10.0, 9.0]), [h, h]));
    assert!(close(pick([-10.0, -11.0]), [h, h]), "away from the point is its line too");
    assert!(close(pick([10.0, 2.0]), [1.0, 0.0]));
    assert!(close(pick([1.0, -10.0]), [0.0, 1.0]));
    assert_eq!(manga::snap_direction([0.0, 0.0], [3.0, 4.0], &r, 6.0), None, "under 6 px: not yet");
    let line = manga::snap_direction([0.0, 0.0], [10.0, 9.0], &r, 6.0).unwrap();
    assert!(close(line.project([20.0, 18.0]), [19.0, 19.0]));
}

#[test]
fn manga_snap_two_and_three_points_are_pinned() {
    let two = Rulers { vps: vec![[-200.0, -50.0], [300.0, -50.0]], horizon: Some(-50.0) };
    let pick = |r: &Rulers, to: [f32; 2]| manga::snap_direction([0.0, 0.0], to, r, 6.0).unwrap().dir;
    let unit = |v: [f32; 2]| {
        let l = v[0].hypot(v[1]);
        [v[0] / l, v[1] / l]
    };
    assert!(close(pick(&two, [-10.0, -2.6]), unit([-200.0, -50.0])));
    assert!(close(pick(&two, [10.0, -1.6]), unit([300.0, -50.0])));
    assert!(close(pick(&two, [10.0, 0.5]), [1.0, 0.0]));
    assert!(close(pick(&two, [0.0, 10.0]), [0.0, 1.0]), "two points keep the vertical");
    let mut three = two.clone();
    three.vps.push([100.0, 1000.0]);
    assert!(close(pick(&three, [0.0, 10.0]), unit([100.0, 1000.0])), "the third point stands in for it");
    assert!(close(pick(&three, [10.0, 0.5]), [1.0, 0.0]));
}

#[test]
fn manga_snap_holds_the_stroke_from_6_px_and_off_is_the_identity() {
    let r = Rulers { vps: vec![[1000.0, 0.0]], horizon: Some(0.0) };
    let mut s = Snap::new(r, 6.0);
    assert_eq!(s.feed([0.0, 0.0]), [0.0, 0.0]);
    assert_eq!(s.feed([2.0, 1.0]), [0.0, 0.0], "held at the start until the line is picked");
    assert_eq!(s.feed([10.0, 3.0]), [10.0, 0.0]);
    assert_eq!(s.feed([20.0, -4.0]), [20.0, 0.0], "later points go onto the line");
    assert_eq!(s.line().unwrap().dir, [1.0, 0.0]);
    let mut off = Snap::new(Rulers::default(), 6.0);
    for p in [[0.0, 0.0], [3.0, 1.0], [10.0, 3.0], [25.0, 17.5]] {
        assert_eq!(off.feed(p), p, "no rulers: the identity");
    }
    assert_eq!(manga::snap_direction([0.0; 2], [10.0, 3.0], &Rulers::default(), 6.0), None);
}

#[test]
fn manga_rulers_live_in_the_cut_meta_one_step_each() {
    let (mut e, at, _) = page_rig(Vec::new(), 2.0);
    let cut = e.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(Rulers::of(cut), Rulers::default());
    let r = Rulers::preset(2, 640.0, 480.0);
    assert_eq!(r.vps.len(), 2);
    assert!((r.horizon.unwrap() - 192.0).abs() < 1e-3);
    let depth = e.history_len();
    let cmd = r.set(cut, at).unwrap();
    e.apply("rulers", vec![cmd]).unwrap();
    assert_eq!(e.history_len(), depth + 1);
    let cut = e.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(Rulers::of(cut), r);
    assert!(r.set(cut, at).is_none(), "no change, no step");
    assert_eq!(Rulers::preset(3, 640.0, 480.0).vps.len(), 3);
    e.undo().unwrap();
    let cut = e.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(Rulers::of(cut), Rulers::default());
}

// ---- Page export ----------------------------------------------------------------------

/// An opaque grey ramp, black at the left to white at the right.
fn ramp(w: u32, h: u32) -> Image {
    let mut img = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let g = x as f32 / (w - 1) as f32;
            img.set_pixel(x, y, [g, g, g, 1.0]);
        }
    }
    img
}

fn png_bytes(path: &std::path::Path) -> (png::Info<'static>, Vec<u8>) {
    let mut dec = png::Decoder::new(std::fs::File::open(path).unwrap());
    dec.set_transformations(png::Transformations::EXPAND);
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).unwrap();
    buf.truncate(frame.buffer_size());
    (reader.info().clone(), buf)
}

#[test]
fn manga_png_page_carries_350_dpi_and_mono_is_1_bit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mono.png");
    let opts = PageOpts { format: PageFormat::Png, mode: PageMode::Mono, ..Default::default() };
    deliver::write_page(&path, &ramp(32, 8), &opts, 350.0).unwrap();
    let (info, bytes) = png_bytes(&path);
    let dims = info.pixel_dims.unwrap();
    assert_eq!((dims.xppu, dims.yppu, dims.unit), (13780, 13780, png::Unit::Meter));
    assert_eq!((info.bit_depth, info.color_type), (png::BitDepth::One, png::ColorType::Grayscale));
    assert_eq!(bytes.len(), 32 * 8);
    assert!(bytes.iter().all(|&b| b == 0 || b == 255), "mono is 0 or 255");
    assert_eq!((bytes[0], bytes[31]), (0, 255));
    // Grey and colour carry the same pHYs.
    for mode in [PageMode::Grey, PageMode::Color] {
        let path = dir.path().join(format!("{mode:?}.png"));
        deliver::write_page(&path, &ramp(32, 8), &PageOpts { mode, ..opts }, 350.0).unwrap();
        let (info, bytes) = png_bytes(&path);
        assert_eq!(info.pixel_dims.unwrap().xppu, 13780);
        assert_eq!(bytes.len(), 32 * 8 * if mode == PageMode::Grey { 1 } else { 3 });
    }
}

#[test]
fn manga_tiff_page_carries_350_an_inch_and_grey_is_rec709_luma() {
    use tiff::decoder::{Decoder, DecodingResult, ifd::Value};
    use tiff::tags::Tag;
    let img = Image::from_px(4, 2, [200.0 / 255.0, 100.0 / 255.0, 50.0 / 255.0, 1.0].repeat(8));
    // 0.2126·200 + 0.7152·100 + 0.0722·50 = 117.65.
    assert_eq!(deliver::page_samples(&img, PageMode::Grey, 0.5), vec![118; 8]);
    assert_eq!(deliver::page_samples(&img, PageMode::Mono, 0.5), vec![0; 8]);
    assert_eq!(deliver::page_samples(&img, PageMode::Mono, 0.4), vec![255; 8]);
    let dir = tempfile::tempdir().unwrap();
    for mode in [PageMode::Color, PageMode::Grey, PageMode::Mono] {
        let path = dir.path().join(format!("{mode:?}.tif"));
        let opts = PageOpts { format: PageFormat::Tiff, mode, ..Default::default() };
        deliver::write_page(&path, &img, &opts, 350.0).unwrap();
        let mut dec = Decoder::new(std::fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(dec.dimensions().unwrap(), (4, 2));
        for tag in [Tag::XResolution, Tag::YResolution] {
            match dec.get_tag(tag).unwrap() {
                Value::Rational(n, d) => assert_eq!(n / d, 350, "{mode:?}"),
                v => panic!("{mode:?}: {v:?}"),
            }
        }
        assert_eq!(dec.get_tag_unsigned::<u16>(Tag::ResolutionUnit).unwrap(), 2, "per inch");
        let bits = dec.find_tag_unsigned_vec::<u16>(Tag::BitsPerSample).unwrap().unwrap();
        match mode {
            PageMode::Mono => assert_eq!(bits, vec![1]),
            PageMode::Grey => {
                assert_eq!(bits, vec![8]);
                match dec.read_image().unwrap() {
                    DecodingResult::U8(v) => assert_eq!(v, vec![118; 8]),
                    _ => panic!("not 8-bit"),
                }
            }
            PageMode::Color => assert_eq!(bits, vec![8, 8, 8]),
        }
    }
}

#[test]
fn manga_webtoon_exports_in_1280_px_segments() {
    assert_eq!(manga::slice_webtoon(6400, 1280).len(), 5);
    assert_eq!(manga::slice_webtoon(3000, 1280), vec![(0, 1280), (1280, 1280), (2560, 440)]);
    assert_eq!(manga::slice_webtoon(1280, 1280), vec![(0, 1280)]);
    // End to end on a small strip: 40 px down in 16 px segments.
    let mut e = Engine::new("strip");
    e.project.width = 16;
    e.project.height = 40;
    let scene = e.add_scene("book");
    let c = e.add_cut(scene, "EP01", 1).unwrap();
    let mut page = Page::webtoon();
    (page.width, page.height, page.segment) = (16, 40, Some(16));
    let at = CutRef { scene, cut: c };
    let meta = Command::SetCutMeta { at, key: manga::PAGE_KEY.into(), before: None, after: Some(page.to_meta()) };
    e.apply("page", vec![meta]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let files = deliver::export_pages(&e.project, scene, dir.path(), &PageOpts::default()).unwrap();
    assert_eq!(files.len(), 3);
    assert!(files[2].ends_with("EP01_03.png"), "{files:?}");
    let heights: Vec<u32> = files.iter().map(|f| image::image_dimensions(f).unwrap().1).collect();
    assert_eq!(heights, vec![16, 16, 8]);
}

#[test]
fn manga_print_page_exports_with_trim_marks_round_it() {
    let mut e = Engine::new("book");
    let page = Page::print("test", [10.0, 12.0], 1.0, [8.0, 10.0], 72.0);
    (e.project.width, e.project.height) = (page.width, page.height);
    let scene = e.add_scene("book");
    let c = e.add_cut(scene, "P01", 1).unwrap();
    let at = CutRef { scene, cut: c };
    let meta = Command::SetCutMeta { at, key: manga::PAGE_KEY.into(), before: None, after: Some(page.to_meta()) };
    e.apply("page", vec![meta]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let opts = PageOpts { trim_marks: true, ..Default::default() };
    let files = deliver::export_pages(&e.project, scene, dir.path(), &opts).unwrap();
    let pad = manga::mm_px(10.0, 72.0);
    assert_eq!(
        image::image_dimensions(&files[0]).unwrap(),
        (page.width + 2 * pad, page.height + 2 * pad)
    );
    let marked = image::open(&files[0]).unwrap().to_rgb8();
    // A mark in the top margin on the trim's left line; bare paper beside it.
    assert_eq!(marked.get_pixel(pad + page.trim.x, 1).0, [0, 0, 0]);
    assert_eq!(marked.get_pixel(pad + page.trim.x + 6, 1).0, [255, 255, 255]);
}
