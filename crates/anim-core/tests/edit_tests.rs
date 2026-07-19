//! Golden tests for the region-edit math (anim_core::edit) — the engine side
//! of the select/move/scale/rotate tool. Laws pinned: lift+identity+merge is
//! a byte-exact no-op (empty diff); integer moves are exact; rotation about
//! the gesture pivot follows the graph Transform convention; the assembled
//! PaintTiles diff round-trips through Engine undo to a byte-identical
//! project.

use std::collections::BTreeMap;
use std::sync::Arc;

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::edit::{Affine, lift_all, lift_region, merge_patch, transform_patch};
use anim_core::model::CelLayer;
use anim_core::raster::{TILE, TILE_LEN, TileCoord, TileData, TileDiff, f32_to_f16_bits};
use anim_core::xsheet::Exposure;

/// A tile map with single opaque texels at the given paper coords.
fn tiles_with_texels(texels: &[(i32, i32, [f32; 4])]) -> BTreeMap<TileCoord, Arc<TileData>> {
    let mut map: BTreeMap<TileCoord, Vec<u16>> = BTreeMap::new();
    for (x, y, rgba) in texels {
        let (tx, ty) = (x.div_euclid(TILE as i32), y.div_euclid(TILE as i32));
        let (cx, cy) = (x.rem_euclid(TILE as i32), y.rem_euclid(TILE as i32));
        let tile = map.entry((tx, ty)).or_insert_with(|| vec![0u16; TILE_LEN]);
        let i = (cy as usize * TILE + cx as usize) * 4;
        for c in 0..4 {
            tile[i + c] = f32_to_f16_bits(rgba[c]);
        }
    }
    map.into_iter()
        .map(|(k, v)| (k, Arc::new(TileData::from_vec(v))))
        .collect()
}

fn texel_of(
    tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    x: i32,
    y: i32,
) -> Option<[u16; 4]> {
    let (tx, ty) = (x.div_euclid(TILE as i32), y.div_euclid(TILE as i32));
    let (cx, cy) = (x.rem_euclid(TILE as i32), y.rem_euclid(TILE as i32));
    let t = tiles.get(&(tx, ty))?;
    let i = (cy as usize * TILE + cx as usize) * 4;
    Some([t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3]])
}

/// Apply a merge diff onto a tile map (what PaintTiles does inside a layer).
fn apply_diff(
    tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    diff: &TileDiff,
) -> BTreeMap<TileCoord, Arc<TileData>> {
    let mut out = tiles.clone();
    for (coord, _before, after) in diff {
        match after {
            Some(t) => {
                out.insert(*coord, t.clone());
            }
            None => {
                out.remove(coord);
            }
        }
    }
    out
}

fn rect_poly(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<(f32, f32)> {
    vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

#[test]
fn identity_gesture_is_a_byte_exact_noop() {
    let tiles = tiles_with_texels(&[
        (10, 20, [1.0, 0.5, 0.25, 1.0]),
        (70, 20, [0.5, 0.5, 0.5, 0.5]), // second tile
    ]);
    let lift = lift_all(&tiles).unwrap();
    let affine = Affine::identity((32.0, 32.0));
    let moved = transform_patch(&lift.patch, &affine);
    let diff = merge_patch(&tiles, &lift.cleared, &moved);
    assert!(diff.is_empty(), "identity lift+merge must be a no-op, got {} tiles", diff.len());
}

#[test]
fn integer_move_is_exact_and_inverse_restores() {
    let tiles = tiles_with_texels(&[(10, 20, [1.0, 0.0, 0.0, 1.0])]);
    let lift = lift_region(&tiles, &rect_poly(5.0, 15.0, 15.0, 25.0)).unwrap();
    let mut affine = Affine::identity((10.0, 20.0));
    affine.translate = (100.0, 3.0); // crosses into the next tile
    let moved = transform_patch(&lift.patch, &affine);
    let diff = merge_patch(&tiles, &lift.cleared, &moved);
    let after = apply_diff(&tiles, &diff);

    assert_eq!(texel_of(&after, 10, 20), None, "source tile removed (became empty)");
    let dst = texel_of(&after, 110, 23).expect("destination tile exists");
    assert_eq!(
        dst,
        [f32_to_f16_bits(1.0), 0, 0, f32_to_f16_bits(1.0)],
        "texel moved bit-exactly"
    );
    // The diff's inverse (swap before/after) restores the original map.
    let inverse: Vec<_> = diff
        .iter()
        .map(|(c, b, a)| (*c, a.clone(), b.clone()))
        .collect();
    let restored = apply_diff(&after, &inverse);
    assert_eq!(restored.len(), tiles.len());
    for (k, v) in &tiles {
        assert_eq!(restored.get(k).map(|t| &t.rgba), Some(&v.rgba));
    }
}

#[test]
fn selection_polygon_lifts_only_inside() {
    // Two texels; lasso around only the first.
    let tiles = tiles_with_texels(&[
        (10, 10, [1.0, 0.0, 0.0, 1.0]),
        (30, 10, [0.0, 1.0, 0.0, 1.0]),
    ]);
    let lift = lift_region(&tiles, &rect_poly(5.0, 5.0, 15.0, 15.0)).unwrap();
    assert!(lift.patch.has_ink());
    // Cleared tile keeps the second texel.
    let (coord, after) = &lift.cleared[0];
    assert_eq!(*coord, (0, 0));
    let t = after.as_ref().expect("tile still has the green texel");
    let i = (10 * TILE + 30) * 4;
    assert_eq!(t.rgba[i + 1], f32_to_f16_bits(1.0), "outside-selection texel untouched");
    let i_red = (10 * TILE + 10) * 4;
    assert_eq!(t.rgba[i_red + 3] & 0x7FFF, 0, "inside-selection texel lifted out");
}

#[test]
fn empty_selection_returns_none() {
    let tiles = tiles_with_texels(&[(10, 10, [1.0, 0.0, 0.0, 1.0])]);
    assert!(lift_region(&tiles, &rect_poly(40.0, 40.0, 50.0, 50.0)).is_none());
    let empty: BTreeMap<TileCoord, Arc<TileData>> = BTreeMap::new();
    assert!(lift_all(&empty).is_none());
}

#[test]
fn rotate_90_about_pivot_matches_transform_convention() {
    // Texel (40, 32), pivot (32.5, 32.5) — offset (+8, 0) from pivot at its
    // centre (40.5, 32.5). 90° clockwise (y-down) ⇒ offset (0, +8) ⇒ centre
    // (32.5, 40.5) = exactly pixel (32, 40).
    let tiles = tiles_with_texels(&[(40, 32, [0.0, 0.0, 1.0, 1.0])]);
    let lift = lift_all(&tiles).unwrap();
    let affine = Affine {
        pivot: (32.5, 32.5),
        translate: (0.0, 0.0),
        rotate_rad: std::f32::consts::FRAC_PI_2,
        scale: 1.0,
    };
    let moved = transform_patch(&lift.patch, &affine);
    let diff = merge_patch(&tiles, &lift.cleared, &moved);
    let after = apply_diff(&tiles, &diff);
    let dst = texel_of(&after, 32, 40).expect("rotated texel landed");
    assert_eq!(dst[2], f32_to_f16_bits(1.0));
    assert_eq!(dst[3], f32_to_f16_bits(1.0));
    // Source position is transparent (same tile as the destination, so the
    // tile itself survives — only the texel was lifted out).
    let src = texel_of(&after, 40, 32).expect("tile still exists");
    assert_eq!(src[3] & 0x7FFF, 0, "source texel cleared");
}

#[test]
fn scale_about_pivot_is_pinned() {
    // Texel (34, 32): centre (34.5, 32.5) = offset (+2, 0) from pivot
    // (32.5, 32.5). Scale 2 ⇒ offset (+4, 0) ⇒ centre (36.5, 32.5) = exactly
    // pixel (36, 32). Kills the reciprocal bug (inv_s = scale would land the
    // sample off-centre and split it across pixels) and pins scale-about-
    // pivot (an origin-anchored scale would land at x ≈ 68).
    let tiles = tiles_with_texels(&[(34, 32, [0.0, 1.0, 0.0, 1.0])]);
    let lift = lift_all(&tiles).unwrap();
    let mut affine = Affine::identity((32.5, 32.5));
    affine.scale = 2.0;
    let moved = transform_patch(&lift.patch, &affine);
    let diff = merge_patch(&tiles, &lift.cleared, &moved);
    let after = apply_diff(&tiles, &diff);
    let dst = texel_of(&after, 36, 32).expect("scaled texel landed");
    assert_eq!(dst[1], f32_to_f16_bits(1.0), "full green at the exact landing");
    assert_eq!(dst[3], f32_to_f16_bits(1.0), "fully opaque at the centre pixel");
}

#[test]
fn degenerate_scale_erases_the_selection() {
    // Scale ~0: the patch vanishes — the gesture becomes a plain cut/clear.
    let tiles = tiles_with_texels(&[(10, 10, [1.0, 0.0, 0.0, 1.0])]);
    let lift = lift_all(&tiles).unwrap();
    let mut affine = Affine::identity((10.5, 10.5));
    affine.scale = 0.0;
    let moved = transform_patch(&lift.patch, &affine);
    assert!(!moved.has_ink());
    let diff = merge_patch(&tiles, &lift.cleared, &moved);
    let after = apply_diff(&tiles, &diff);
    assert!(after.is_empty());
}

#[test]
fn gesture_through_engine_is_one_exact_undo_step() {
    let mut engine = Engine::new("edit test");
    let scene = engine.add_scene("SC");
    let cut = engine.add_cut(scene, "cut", 24).unwrap();
    let at = CutRef { scene, cut };
    let col = engine.add_column(at, "A").unwrap();
    let d = engine.alloc_drawing_id();
    let layer = engine.alloc_layer_id();
    let tiles = tiles_with_texels(&[(10, 20, [1.0, 1.0, 1.0, 1.0])]);
    let mut cel = CelLayer::new(layer, "paint");
    cel.raster.tiles = tiles.clone();
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
                    layers: vec![cel],
                },
                Command::SetCell { at, column: col, frame: 0, key: Some(Exposure::Drawing(d)) },
            ],
        )
        .unwrap();
    let baseline = engine.project.clone();

    // The app's commit: lift → transform → merge → ONE PaintTiles.
    let lift = lift_all(&tiles).unwrap();
    let mut affine = Affine::identity((10.5, 20.5));
    affine.translate = (7.0, 0.0);
    let moved = transform_patch(&lift.patch, &affine);
    let diff = merge_patch(&tiles, &lift.cleared, &moved);
    assert!(!diff.is_empty());
    engine
        .apply("transform selection", vec![Command::PaintTiles { at, id: d, layer, diff }])
        .unwrap();

    let after = &engine.project.cut(scene, cut).unwrap().drawing(d).unwrap().layers[0]
        .raster
        .tiles;
    assert_eq!(texel_of(after, 17, 20).map(|t| t[3]), Some(f32_to_f16_bits(1.0)));

    engine.undo().unwrap();
    assert_eq!(engine.project, baseline, "one undo restores byte-identical project");
}
