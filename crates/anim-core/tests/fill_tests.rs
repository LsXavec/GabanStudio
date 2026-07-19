//! Golden tests for the flood fill (anim_core::fill) — the ink-&-paint verb.
//! Laws pinned: the flood is bounded by reference ink and canvas edges; gap
//! closing seals line breaks up to gap_px; grow extends the fill under the
//! lines; a fill through the Engine is one exact undo step; the cel-flatten
//! reference lets line art bound a fill landing on another layer.

use std::collections::BTreeMap;
use std::sync::Arc;

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::fill::{FillOpts, fill_diff, flood_fill_mask};
use anim_core::model::CelLayer;
use anim_core::raster::{TILE, TILE_LEN, TileCoord, TileData, f32_to_f16_bits};
use anim_core::xsheet::Exposure;

const W: u32 = 64;
const H: u32 = 64;

fn opts(gap: u32, grow: u32) -> FillOpts {
    FillOpts {
        threshold: 0.1,
        gap_px: gap,
        grow_px: grow,
        protect_ink: false,
    }
}

/// Reference tiles with opaque ink at the given paper pixels.
fn ink(texels: &[(i32, i32)]) -> BTreeMap<TileCoord, Arc<TileData>> {
    let mut map: BTreeMap<TileCoord, Vec<u16>> = BTreeMap::new();
    for (x, y) in texels {
        let (tx, ty) = (x.div_euclid(TILE as i32), y.div_euclid(TILE as i32));
        let (cx, cy) = (x.rem_euclid(TILE as i32), y.rem_euclid(TILE as i32));
        let tile = map.entry((tx, ty)).or_insert_with(|| vec![0u16; TILE_LEN]);
        let i = (cy as usize * TILE + cx as usize) * 4;
        tile[i] = f32_to_f16_bits(0.1);
        tile[i + 3] = f32_to_f16_bits(1.0);
    }
    map.into_iter()
        .map(|(k, v)| (k, Arc::new(TileData::from_vec(v))))
        .collect()
}

/// A hollow rectangle of ink: the classic enclosed region.
fn box_outline(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for x in x0..=x1 {
        v.push((x, y0));
        v.push((x, y1));
    }
    for y in y0..=y1 {
        v.push((x0, y));
        v.push((x1, y));
    }
    v
}

#[test]
fn empty_reference_fills_the_whole_canvas() {
    let mask = flood_fill_mask(&BTreeMap::new(), (10, 10), W, H, &opts(0, 0), None).unwrap();
    assert_eq!(mask.count(), (W * H) as usize);
}

#[test]
fn enclosed_region_fills_interior_only() {
    let reference = ink(&box_outline(10, 10, 30, 30));
    let mask = flood_fill_mask(&reference, (20, 20), W, H, &opts(0, 0), None).unwrap();
    // Interior = 19×19 (11..=29 both axes); no line pixels, nothing outside.
    assert_eq!(mask.count(), 19 * 19);
    assert!(mask.get(11, 11) && mask.get(29, 29));
    assert!(!mask.get(10, 20), "boundary not filled at gap 0 / grow 0");
    assert!(!mask.get(31, 20), "outside untouched");
}

#[test]
fn seed_on_ink_returns_none() {
    let reference = ink(&box_outline(10, 10, 30, 30));
    assert!(flood_fill_mask(&reference, (10, 10), W, H, &opts(0, 0), None).is_err());
    assert!(flood_fill_mask(&reference, (-1, 5), W, H, &opts(0, 0), None).is_err());
    assert!(flood_fill_mask(&reference, (64, 5), W, H, &opts(0, 0), None).is_err());
}

#[test]
fn gap_closing_seals_a_line_break() {
    // Box with a 3-px gap punched in the top edge.
    let mut outline = box_outline(10, 10, 30, 30);
    outline.retain(|(x, y)| !(*y == 10 && (19..=21).contains(x)));
    let reference = ink(&outline);

    // Without gap closing the flood escapes through the break.
    let leaky = flood_fill_mask(&reference, (20, 20), W, H, &opts(0, 0), None).unwrap();
    assert!(
        leaky.get(5, 5),
        "gap 0: the fill must leak out of the broken box (count {})",
        leaky.count()
    );

    // gap 4 closes a 3-px break; the fill stays inside (plus the dilation
    // band around the interior — but nowhere near the far canvas corner).
    let sealed = flood_fill_mask(&reference, (20, 20), W, H, &opts(4, 0), None).unwrap();
    assert!(!sealed.get(5, 5), "gap 4: no leak to the far field");
    assert!(sealed.get(20, 20), "interior still filled");
}

#[test]
fn grow_extends_under_the_lines() {
    let reference = ink(&box_outline(10, 10, 30, 30));
    let grown = flood_fill_mask(&reference, (20, 20), W, H, &opts(0, 2), None).unwrap();
    // Grow measures from the REGION edge (interior x=11): grow 2 covers the
    // 1-px line (x=10) and one pixel beyond (x=9) — tucked under the line…
    assert!(grown.get(10, 20) && grown.get(30, 20), "line pixels covered");
    assert!(grown.get(9, 20), "grow reaches just past the line");
    // …but never further into the far field.
    assert!(!grown.get(8, 20), "grow is bounded");
    // BOTH AXES: a rows-only (or cols-only) dilation must fail here — this
    // exact mutation once survived the suite.
    assert!(grown.get(20, 10) && grown.get(20, 30), "vertical line pixels covered");
    assert!(grown.get(20, 9), "grow reaches past the TOP edge too");
    assert!(!grown.get(20, 8), "vertical grow is bounded");
}

#[test]
fn gap_closing_seals_a_vertical_break_too() {
    // Same as the horizontal gap test but the break is in the LEFT edge —
    // sealing it needs the dilation's COLUMN pass (axis-symmetry pin).
    let mut outline = box_outline(10, 10, 30, 30);
    outline.retain(|(x, y)| !(*x == 10 && (19..=21).contains(y)));
    let reference = ink(&outline);
    let leaky = flood_fill_mask(&reference, (20, 20), W, H, &opts(0, 0), None).unwrap();
    assert!(leaky.get(5, 5), "gap 0: leaks through the vertical break");
    let sealed = flood_fill_mask(&reference, (20, 20), W, H, &opts(4, 0), None).unwrap();
    assert!(!sealed.get(5, 5), "gap 4: sealed");
    assert!(sealed.get(20, 20), "interior still filled");
}

#[test]
fn non_convex_region_floods_around_the_bend() {
    // A U-shape: a wall hangs from the box's top edge down to y=25 at x=20,
    // splitting the upper interior into two arms connected only underneath.
    // A directional-seeding scanline bug fills one arm and misses the other.
    let mut outline = box_outline(10, 10, 30, 30);
    for y in 10..=25 {
        outline.push((20, y));
    }
    let reference = ink(&outline);
    let mask = flood_fill_mask(&reference, (15, 15), W, H, &opts(0, 0), None).unwrap();
    assert!(mask.get(15, 15), "seeded arm filled");
    assert!(mask.get(25, 15), "far arm reached around the bend");
    assert!(mask.get(20, 28), "the connecting channel under the wall filled");
    assert!(!mask.get(20, 20), "the wall itself is not filled");
    assert!(!mask.get(5, 5), "nothing escapes the box");
}

#[test]
fn protect_ink_keeps_layer_mode_lines_alive() {
    // Layer mode: the reference IS the target. grow 2 without protection
    // covers the boundary; with protect_ink the lines survive while the
    // fill still recovers up to the line edge.
    let reference = ink(&box_outline(10, 10, 30, 30));
    let mut o = opts(0, 2);
    o.protect_ink = true;
    let mask = flood_fill_mask(&reference, (20, 20), W, H, &o, None).unwrap();
    assert!(!mask.get(10, 20), "boundary ink protected");
    assert!(!mask.get(20, 10), "vertical boundary ink protected");
    assert!(mask.get(11, 20), "fill still reaches the line edge");
    assert!(mask.get(9, 20), "grow still recovers past the line");
}

#[test]
fn selection_clip_confines_the_fill() {
    // Fills respect selections: an empty reference would fill the whole
    // canvas, but a selection rect confines it exactly.
    let clip = [(5.0, 5.0), (20.0, 5.0), (20.0, 20.0), (5.0, 20.0)];
    let mask =
        flood_fill_mask(&BTreeMap::new(), (10, 10), W, H, &opts(0, 0), Some(&clip)).unwrap();
    assert_eq!(mask.count(), 15 * 15, "exactly the selection's interior");
    assert!(mask.get(10, 10));
    assert!(!mask.get(25, 10), "outside the selection untouched");
}

#[test]
fn seed_outside_the_selection_is_refused() {
    use anim_core::fill::FillRefusal;
    // Industry rule: the selection is the editable domain — a click outside
    // it is a no-op even when the flooded region OVERLAPS the selection.
    let clip = [(5.0, 5.0), (20.0, 5.0), (20.0, 20.0), (5.0, 20.0)];
    let r = flood_fill_mask(&BTreeMap::new(), (40, 40), W, H, &opts(0, 0), Some(&clip));
    assert_eq!(r.err(), Some(FillRefusal::OutsideSelection));
    // And a selection whose interior the seed misses entirely:
    let far = [(40.0, 40.0), (50.0, 40.0), (50.0, 50.0), (40.0, 50.0)];
    let boxed = ink(&box_outline(10, 10, 30, 30));
    let r2 = flood_fill_mask(&boxed, (20, 20), W, H, &opts(0, 0), Some(&far));
    assert_eq!(r2.err(), Some(FillRefusal::OutsideSelection));
}

#[test]
fn selection_border_is_a_wall_no_tunnelling() {
    // A 1-px ink wall spans the selection top-to-bottom; the regions left and
    // right of it connect ONLY through unselected territory below. The flood
    // must not walk around the wall's end outside the selection and come
    // back in: the far side stays empty.
    let clip = [(5.0, 5.0), (25.0, 5.0), (25.0, 20.0), (5.0, 20.0)];
    let wall: Vec<(i32, i32)> = (2..=23).map(|y| (12, y)).collect();
    let reference = ink(&wall);
    let mask =
        flood_fill_mask(&reference, (8, 10), W, H, &opts(0, 0), Some(&clip)).unwrap();
    assert!(mask.get(8, 10), "seeded side filled");
    assert!(!mask.get(16, 10), "far side of the wall NOT reachable within the selection");
    assert!(!mask.get(12, 10), "the wall itself stays empty");
}

#[test]
fn clip_reapplies_after_grow() {
    // grow pushes the fill outward AFTER the flood; the selection must clip
    // that expansion too (moving the re-clip before the grow dilate would
    // let grow escape the selection — this pins the order).
    let clip = [(11.0, 11.0), (30.0, 11.0), (30.0, 30.0), (11.0, 30.0)];
    let reference = ink(&box_outline(10, 10, 30, 30));
    let mask =
        flood_fill_mask(&reference, (20, 20), W, H, &opts(0, 2), Some(&clip)).unwrap();
    assert!(mask.get(11, 20), "fill inside the selection");
    assert!(
        !mask.get(9, 20),
        "grow may not escape the selection even though grow 2 reaches x=9 unclipped"
    );
}

#[test]
fn fill_diff_writes_color_and_skips_untouched_tiles() {
    let reference = ink(&box_outline(2, 2, 20, 20));
    let mask = flood_fill_mask(&reference, (10, 10), W, H, &opts(0, 0), None).unwrap();
    let target: BTreeMap<TileCoord, Arc<TileData>> = BTreeMap::new();
    let color = [0.5, 0.25, 0.0, 1.0]; // opaque premult
    let diff = fill_diff(&target, &mask, color);
    // Region sits entirely in tile (0,0) on a 64×64 canvas.
    assert_eq!(diff.len(), 1);
    let (coord, before, after) = &diff[0];
    assert_eq!(*coord, (0, 0));
    assert!(before.is_none());
    let t = after.as_ref().unwrap();
    let i = (10 * TILE + 10) * 4;
    assert_eq!(t.rgba[i], f32_to_f16_bits(0.5));
    assert_eq!(t.rgba[i + 3], f32_to_f16_bits(1.0));
    let j = (30 * TILE + 30) * 4; // outside the box: untouched
    assert_eq!(t.rgba[j + 3] & 0x7FFF, 0);
}

#[test]
fn semi_transparent_fill_composites_over() {
    // 50% black over an existing 100% white texel = over-blend, not replace.
    let mut white = vec![0u16; TILE_LEN];
    let i = (5 * TILE + 5) * 4;
    for c in 0..4 {
        white[i + c] = f32_to_f16_bits(1.0);
    }
    let target: BTreeMap<TileCoord, Arc<TileData>> =
        [((0, 0), Arc::new(TileData::from_vec(white)))].into();
    let mask = flood_fill_mask(&BTreeMap::new(), (5, 5), W, H, &opts(0, 0), None).unwrap();
    let diff = fill_diff(&target, &mask, [0.0, 0.0, 0.0, 0.5]);
    let after = diff
        .iter()
        .find(|(c, _, _)| *c == (0, 0))
        .and_then(|(_, _, a)| a.as_ref())
        .unwrap();
    // white·(1−0.5) + black = 0.5 grey, alpha 1.
    assert_eq!(after.rgba[i], f32_to_f16_bits(0.5));
    assert_eq!(after.rgba[i + 3], f32_to_f16_bits(1.0));
}

#[test]
fn shiage_flow_line_layer_bounds_fill_on_color_layer() {
    // The real workflow: a two-layer cel (color under line). The LINE layer
    // holds a box outline; the fill references the CEL FLATTEN and lands on
    // the COLOR layer — one engine apply, one exact undo step.
    let mut engine = Engine::new("fill test");
    let scene = engine.add_scene("SC");
    let cut = engine.add_cut(scene, "cut", 24).unwrap();
    let at = CutRef { scene, cut };
    let col = engine.add_column(at, "A").unwrap();
    let d = engine.alloc_drawing_id();
    let color_layer = engine.alloc_layer_id();
    let line_layer = engine.alloc_layer_id();
    let mut line = CelLayer::new(line_layer, "line");
    line.raster.tiles = ink(&box_outline(10, 10, 30, 30));
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
                    layers: vec![CelLayer::new(color_layer, "color"), line],
                },
                Command::SetCell { at, column: col, frame: 0, key: Some(Exposure::Drawing(d)) },
            ],
        )
        .unwrap();
    let baseline = engine.project.clone();

    let drawing = engine.project.cut(scene, cut).unwrap().drawing(d).unwrap();
    let reference = drawing.flatten(); // line art via the cel flatten
    let mask = flood_fill_mask(&reference, (20, 20), W, H, &opts(2, 2), None).unwrap();
    let target = &drawing.layer(color_layer).unwrap().raster.tiles;
    let diff = fill_diff(target, &mask, [0.2, 0.4, 0.8, 1.0]);
    assert!(!diff.is_empty());
    engine
        .apply("flood fill", vec![Command::PaintTiles { at, id: d, layer: color_layer, diff }])
        .unwrap();

    let filled = engine.project.cut(scene, cut).unwrap().drawing(d).unwrap();
    let color_tiles = &filled.layer(color_layer).unwrap().raster.tiles;
    let t = color_tiles.get(&(0, 0)).expect("color layer gained ink");
    let i = (20 * TILE + 20) * 4;
    assert_eq!(t.rgba[i + 3], f32_to_f16_bits(1.0), "interior filled on the color layer");
    // The LINE layer is untouched.
    assert_eq!(
        filled.layer(line_layer).unwrap().raster.content_hash(),
        baseline
            .cut(scene, cut)
            .unwrap()
            .drawing(d)
            .unwrap()
            .layer(line_layer)
            .unwrap()
            .raster
            .content_hash()
    );

    engine.undo().unwrap();
    assert_eq!(engine.project, baseline, "one undo removes the whole fill");
}
