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

// ---- SESSION v2: author-tagged history (research/PSD-session-v2.md) -------
// The laws proved here: an author undoes only their OWN last step; the undo
// is REFUSED while a later step touches the same drawing+layer (NEVER-DO 3,
// no out-of-order surgery); and the tag is runtime-only, so a saved file's
// schema is untouched.

/// Two layers on one drawing, so authors can work disjointly.
fn two_layer_rig(
    engine: &mut Engine,
) -> (CutRef, anim_core::ids::DrawingId, anim_core::ids::LayerId, anim_core::ids::LayerId) {
    let scene = engine.add_scene("SC");
    let cut = engine.add_cut(scene, "cut", 24).unwrap();
    let at = CutRef { scene, cut };
    let col = engine.add_column(at, "A").unwrap();
    let d = engine.alloc_drawing_id();
    let l_line = engine.alloc_layer_id();
    let l_color = engine.alloc_layer_id();
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
                    layers: vec![
                        CelLayer::new(l_color, "color"),
                        CelLayer::new(l_line, "line"),
                    ],
                },
                Command::SetCell { at, column: col, frame: 0, key: Some(Exposure::Drawing(d)) },
            ],
        )
        .unwrap();
    (at, d, l_line, l_color)
}

fn dab(at: CutRef, d: anim_core::ids::DrawingId, layer: anim_core::ids::LayerId, x: i32) -> Command {
    Command::PaintTiles {
        at,
        id: d,
        layer,
        // TileDiff carries (coord, before, after) — before is None on a
        // fresh layer, which is what makes undo byte-exact.
        diff: tiles_with_texels(&[(x, 4, [1.0, 1.0, 1.0, 1.0])])
            .into_iter()
            .map(|(k, v)| (k, None, Some(v)))
            .collect(),
    }
}

#[test]
fn each_author_undoes_only_their_own_step() {
    let mut engine = Engine::new("session");
    let (at, d, line, color) = two_layer_rig(&mut engine);
    let clean = engine.project.clone();

    engine.set_author(None); // the host's own hand
    engine.apply("host stroke", vec![dab(at, d, line, 10)]).unwrap();
    engine.set_author(Some("kotori".into()));
    engine.apply("guest stroke", vec![dab(at, d, color, 40)]).unwrap();
    engine.set_author(None);

    // The guest undoes THEIRS — the host's stroke must survive.
    engine.undo_last_by(Some("kotori")).expect("guest may undo their own");
    let layers = &engine.project.cut(at.scene, at.cut).unwrap().drawing(d).unwrap().layers;
    let color_l = layers.iter().find(|l| l.props.name == "color").unwrap();
    let line_l = layers.iter().find(|l| l.props.name == "line").unwrap();
    assert!(color_l.raster.tiles.is_empty(), "the guest's own stroke is gone");
    assert!(!line_l.raster.tiles.is_empty(), "the host's stroke survived");

    // And the host undoes theirs, back to the clean rig.
    engine.undo_last_by(None).expect("host may undo their own");
    assert_eq!(engine.project, clean, "both undos restore the rig exactly");
}

#[test]
fn undo_refuses_when_a_later_edit_shares_the_layer() {
    let mut engine = Engine::new("session");
    let (at, d, line, _color) = two_layer_rig(&mut engine);

    engine.set_author(Some("kotori".into()));
    engine.apply("guest stroke", vec![dab(at, d, line, 10)]).unwrap();
    engine.set_author(None);
    engine.apply("host stroke", vec![dab(at, d, line, 20)]).unwrap();

    // The guest's step is buried under one touching the SAME layer.
    assert!(
        engine.undo_last_by(Some("kotori")).is_err(),
        "out-of-order undo on a shared layer must refuse"
    );
    // The host undoes theirs first; now the guest's is safely on top.
    engine.undo_last_by(None).unwrap();
    assert!(engine.undo_last_by(Some("kotori")).is_ok(), "now it is safe");
}

#[test]
fn author_tags_never_reach_the_document() {
    // The file is a SQLite container (page state varies run to run), so the
    // honest claim is about the DOCUMENT: a project saved from a tagged
    // history loads back identical to one saved from an untagged history.
    // The tag is session metadata and must never become document data.
    let dir = std::env::temp_dir().join("animstudio_v2_author_test");
    let _ = std::fs::create_dir_all(&dir);

    let mut tagged = Engine::new("session");
    let (at, d, line, _c) = two_layer_rig(&mut tagged);
    tagged.set_author(Some("kotori".into()));
    tagged.apply("guest stroke", vec![dab(at, d, line, 10)]).unwrap();
    let p1 = dir.join("tagged.animproj");
    tagged.save(&p1).unwrap();

    let mut plain = Engine::new("session");
    let (at2, d2, line2, _c2) = two_layer_rig(&mut plain);
    plain.apply("guest stroke", vec![dab(at2, d2, line2, 10)]).unwrap();
    let p2 = dir.join("plain.animproj");
    plain.save(&p2).unwrap();

    let back_tagged = Engine::load(&p1).unwrap();
    let back_plain = Engine::load(&p2).unwrap();
    assert_eq!(
        back_tagged.project, back_plain.project,
        "a tagged history must save the same document as an untagged one"
    );
    // And the reloaded history is empty — tags cannot survive a round trip.
    assert!(!back_tagged.can_undo(), "history (and its tags) never persists");
}

// ---- SESSION MIRROR (PSD-session-v2 ruling 2026-08-19) --------------------

#[test]
fn a_mirror_replaying_the_stream_matches_the_host_exactly() {
    // Same rig, same name, both sides.
    let mut host = Engine::new("rig");
    let (at, d, line, color) = two_layer_rig(&mut host);
    let mut guest = Engine::new("rig");
    let _ = two_layer_rig(&mut guest);
    guest.clear_history(); // the rig's own setup steps are not the mirror's
    assert_eq!(host.project, guest.project, "identical starting rigs");

    // Host draws with the mirror log on; the guest replays the stream.
    host.mirror_log = true;
    host.apply("a", vec![dab(at, d, line, 10)]).unwrap();
    host.apply("b", vec![dab(at, d, color, 40)]).unwrap();
    for batch in host.drain_applied() {
        guest.apply_mirror(&batch).unwrap();
    }
    assert_eq!(host.project, guest.project, "the stream IS the document");
    assert!(!guest.can_undo(), "a mirror carries no history of its own");

    // The honest limit, PINNED: an undo on the host is not a streamed
    // command — after one, a pure replay diverges and the host must
    // resync (a fresh join snapshot).
    host.undo().unwrap();
    host.apply("c", vec![dab(at, d, line, 20)]).unwrap();
    for batch in host.drain_applied() {
        guest.apply_mirror(&batch).unwrap();
    }
    assert_ne!(
        host.project, guest.project,
        "after a host undo the stream alone is NOT enough — resync required"
    );
}

#[test]
fn per_artist_undo_replays_identically_on_replicas() {
    // STAGE 3 (PSD-multiplayer-rescope): every machine tags every entry
    // with the acting author (its OWN hand included, via the session's
    // base author), applies the one sequenced order, and replays the
    // same per-artist undo. Two replicas must stay byte-identical
    // through interleaved authors, undos, and redos.
    let mut a = Engine::new("rig");
    let (at, d, line, color) = two_layer_rig(&mut a);
    let mut b = Engine::new("rig");
    let _ = two_layer_rig(&mut b);
    a.clear_history();
    b.clear_history();
    assert_eq!(a.project, b.project, "identical starting rigs");

    // The one sequenced order, replayed on both machines.
    let script: Vec<(&str, Vec<Command>)> = vec![
        ("host", vec![dab(at, d, line, 10)]),
        ("ami", vec![dab(at, d, color, 40)]),
        ("host", vec![dab(at, d, line, 20)]),
        ("ami", vec![dab(at, d, color, 50)]),
    ];
    for (author, cmds) in &script {
        for e in [&mut a, &mut b] {
            e.set_author(Some(author.to_string()));
            e.apply("step", cmds.clone()).unwrap();
        }
    }
    assert_eq!(a.project, b.project, "same order, same documents");

    // "If they undo, Their Last stroke Gets overwritten" — ami's undo
    // surrenders the SAME step on both machines; host ink survives.
    let la = a.undo_last_by(Some("ami")).unwrap();
    let lb = b.undo_last_by(Some("ami")).unwrap();
    assert_eq!(la, lb, "both replicas surrendered the same step");
    assert_eq!(a.project, b.project, "identical after ami's undo");

    // The host's own undo obeys the same law — one law, every hand.
    a.undo_last_by(Some("host")).unwrap();
    b.undo_last_by(Some("host")).unwrap();
    assert_eq!(a.project, b.project, "identical after the host's undo");

    // Redos replay in reverse order of the undos, identically.
    a.redo_last_by(Some("host")).unwrap();
    b.redo_last_by(Some("host")).unwrap();
    a.redo_last_by(Some("ami")).unwrap();
    b.redo_last_by(Some("ami")).unwrap();
    assert_eq!(a.project, b.project, "identical after both redos");
}

#[test]
fn commands_roundtrip_through_serde() {
    let mut e = Engine::new("wire");
    let (at, d, line, _c) = two_layer_rig(&mut e);
    let batch = vec![dab(at, d, line, 10)];
    let bytes = serde_json::to_vec(&batch).unwrap();
    let back: Vec<anim_core::command::Command> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(batch, back, "the wire carries commands losslessly");
}
