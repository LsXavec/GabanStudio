//! Phase 1 layers-per-cel tests: layer commands with exact inverses, hash
//! laws, CPU flatten golden values, schema v5 round-trip, and the v4 → v5
//! one-layer migration (fixture forged directly with rusqlite).

use std::sync::Arc;

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::ids::*;
use anim_core::model::{CelLayer, LayerProps};
use anim_core::raster::{TILE_LEN, TileData};
use anim_core::xsheet::Exposure;

fn solid_tile(v: u16) -> Arc<TileData> {
    Arc::new(TileData::from_vec(vec![v; TILE_LEN]))
}

/// Fixture: one cut, one column, one drawing with a 3-layer stack
/// (bottom → top: color, shadow, line), exposed at frame 0.
struct LayerFixture {
    engine: Engine,
    at: CutRef,
    d: DrawingId,
    color: LayerId,
    shadow: LayerId,
    line: LayerId,
}

fn layer_fixture() -> LayerFixture {
    let mut engine = Engine::new("layers");
    let scene = engine.add_scene("S");
    let cut = engine.add_cut(scene, "C", 12).unwrap();
    let at = CutRef { scene, cut };
    let col = engine.add_column(at, "A").unwrap();
    let d = engine.alloc_drawing_id();
    let color = engine.alloc_layer_id();
    let shadow = engine.alloc_layer_id();
    let line = engine.alloc_layer_id();
    engine
        .apply(
            "cel with stack",
            vec![
                Command::AddDrawing {
                    at,
                    id: d,
                    index: 0,
                    name: "cel".into(),
                    strokes: vec![],
                    layers: vec![
                        CelLayer::new(color, "color"),
                        CelLayer::new(shadow, "shadow"),
                        CelLayer::new(line, "line"),
                    ],
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
    engine.clear_history();
    LayerFixture {
        engine,
        at,
        d,
        color,
        shadow,
        line,
    }
}

fn drawing<'a>(f: &'a LayerFixture) -> &'a anim_core::model::Drawing {
    f.engine
        .project
        .cut(f.at.scene, f.at.cut)
        .unwrap()
        .drawing(f.d)
        .unwrap()
}

// ---- Structural commands: exact inverses ---------------------------------

#[test]
fn add_remove_move_setprops_all_undo_exactly() {
    let mut f = layer_fixture();
    let baseline = f.engine.project.clone();

    // Add a "highlight" layer between shadow and line (index 2).
    let hl = f.engine.alloc_layer_id();
    // Snapshot AFTER the id allocation so undo comparisons aren't confused by
    // the next_id counter (allocation is not part of the undoable command).
    let baseline_after_alloc = f.engine.project.clone();
    f.engine
        .apply(
            "add highlight",
            vec![Command::AddCelLayer {
                at: f.at,
                drawing: f.d,
                index: 2,
                layer: CelLayer::new(hl, "highlight"),
            }],
        )
        .unwrap();
    assert_eq!(
        drawing(&f).layers.iter().map(|l| l.props.name.as_str()).collect::<Vec<_>>(),
        vec!["color", "shadow", "highlight", "line"]
    );
    f.engine.undo().unwrap();
    assert_eq!(f.engine.project, baseline_after_alloc);
    f.engine.redo().unwrap();

    // Move highlight to the top.
    f.engine
        .apply(
            "move",
            vec![Command::MoveCelLayer {
                at: f.at,
                drawing: f.d,
                layer: hl,
                to_index: 3,
            }],
        )
        .unwrap();
    assert_eq!(drawing(&f).layers[3].id, hl);
    f.engine.undo().unwrap();
    assert_eq!(drawing(&f).layers[2].id, hl);

    // Props replace + exact inverse.
    let before_props = drawing(&f).layer(f.shadow).unwrap().props.clone();
    f.engine
        .apply(
            "dim shadow",
            vec![Command::SetCelLayerProps {
                at: f.at,
                drawing: f.d,
                layer: f.shadow,
                props: LayerProps {
                    name: "shadow 2".into(),
                    visible: false,
                    opacity: 0.5,
                },
            }],
        )
        .unwrap();
    assert_eq!(drawing(&f).layer(f.shadow).unwrap().props.opacity, 0.5);
    f.engine.undo().unwrap();
    assert_eq!(drawing(&f).layer(f.shadow).unwrap().props, before_props);

    // Remove a layer carrying tiles; undo restores position AND pixels.
    f.engine
        .apply(
            "paint shadow",
            vec![Command::PaintTiles {
                at: f.at,
                id: f.d,
                layer: f.shadow,
                diff: vec![((1, 1), None, Some(solid_tile(0x7777)))],
            }],
        )
        .unwrap();
    let with_paint = f.engine.project.clone();
    f.engine
        .apply(
            "remove shadow",
            vec![Command::RemoveCelLayer {
                at: f.at,
                drawing: f.d,
                layer: f.shadow,
            }],
        )
        .unwrap();
    assert!(drawing(&f).layer(f.shadow).is_none());
    f.engine.undo().unwrap();
    assert_eq!(f.engine.project, with_paint);
    assert_eq!(drawing(&f).layer_index(f.shadow), Some(1));

    // Unwind everything back to the start. The id counter advances on
    // allocation (not undoable, by design) — normalize it for the comparison.
    while f.engine.can_undo() {
        f.engine.undo().unwrap();
    }
    let mut undone = f.engine.project.clone();
    undone.next_id = baseline.next_id;
    assert_eq!(undone, baseline);
}

#[test]
fn move_validates_instead_of_clamping() {
    let mut f = layer_fixture();
    let baseline = f.engine.project.clone();
    let err = f.engine.apply(
        "bad move",
        vec![Command::MoveCelLayer {
            at: f.at,
            drawing: f.d,
            layer: f.line,
            to_index: 3, // len is 3; max valid target is 2
        }],
    );
    assert!(err.is_err());
    assert_eq!(f.engine.project, baseline);
}

#[test]
fn interleaved_layer_ops_fuzz_undo_all_redo_all() {
    // Deterministic pseudo-random interleaving of every layer command;
    // undo-all must equal the initial document, redo-all the final one.
    let mut f = layer_fixture();
    let mut lcg: u64 = 0x12345678;
    let mut rand = move || {
        lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (lcg >> 33) as usize
    };

    let initial = f.engine.project.clone();
    let mut applied = 0usize;
    for step in 0..60 {
        let n_layers = drawing(&f).layers.len();
        let pick = rand() % 5;
        let result = match pick {
            0 => {
                let id = f.engine.alloc_layer_id();
                let index = if n_layers == 0 { 0 } else { rand() % (n_layers + 1) };
                f.engine.apply(
                    "fuzz add",
                    vec![Command::AddCelLayer {
                        at: f.at,
                        drawing: f.d,
                        index,
                        layer: CelLayer::new(id, format!("L{step}")),
                    }],
                )
            }
            1 if n_layers > 0 => {
                let victim = drawing(&f).layers[rand() % n_layers].id;
                f.engine.apply(
                    "fuzz remove",
                    vec![Command::RemoveCelLayer {
                        at: f.at,
                        drawing: f.d,
                        layer: victim,
                    }],
                )
            }
            2 if n_layers > 1 => {
                let mover = drawing(&f).layers[rand() % n_layers].id;
                f.engine.apply(
                    "fuzz move",
                    vec![Command::MoveCelLayer {
                        at: f.at,
                        drawing: f.d,
                        layer: mover,
                        to_index: rand() % n_layers,
                    }],
                )
            }
            3 if n_layers > 0 => {
                let target = drawing(&f).layers[rand() % n_layers].id;
                f.engine.apply(
                    "fuzz props",
                    vec![Command::SetCelLayerProps {
                        at: f.at,
                        drawing: f.d,
                        layer: target,
                        props: LayerProps {
                            name: format!("renamed {step}"),
                            visible: step % 2 == 0,
                            opacity: (step as f32 % 10.0) / 10.0,
                        },
                    }],
                )
            }
            4 if n_layers > 0 => {
                let target = drawing(&f).layers[rand() % n_layers].id;
                let coord = ((rand() % 4) as i32, (rand() % 4) as i32);
                let before = drawing(&f)
                    .layer(target)
                    .unwrap()
                    .raster
                    .tiles
                    .get(&coord)
                    .cloned();
                f.engine.apply(
                    "fuzz paint",
                    vec![Command::PaintTiles {
                        at: f.at,
                        id: f.d,
                        layer: target,
                        diff: vec![(coord, before, Some(solid_tile(step as u16 * 977)))],
                    }],
                )
            }
            _ => continue,
        };
        if result.is_ok() {
            applied += 1;
        }
    }
    assert!(applied > 20, "fuzz should actually apply commands");
    let fin = f.engine.project.clone();

    while f.engine.can_undo() {
        f.engine.undo().unwrap();
    }
    // The id counter moves forward on allocations (not undoable); compare
    // everything else by resetting it.
    let mut undone = f.engine.project.clone();
    undone.next_id = initial.next_id;
    assert_eq!(undone, initial);

    while f.engine.can_redo() {
        f.engine.redo().unwrap();
    }
    assert_eq!(f.engine.project, fin);
}

#[test]
fn remove_drawing_restores_full_stack_and_exposures() {
    let mut f = layer_fixture();
    // Paint distinct tiles on all three layers.
    for (i, layer) in [f.color, f.shadow, f.line].into_iter().enumerate() {
        f.engine
            .apply(
                "paint",
                vec![Command::PaintTiles {
                    at: f.at,
                    id: f.d,
                    layer,
                    diff: vec![((i as i32, 0), None, Some(solid_tile(0x1000 * (i as u16 + 1))))],
                }],
            )
            .unwrap();
    }
    let before = f.engine.project.clone();

    f.engine
        .apply("remove", vec![Command::RemoveDrawing { at: f.at, id: f.d }])
        .unwrap();
    assert!(
        f.engine
            .project
            .cut(f.at.scene, f.at.cut)
            .unwrap()
            .drawing(f.d)
            .is_none()
    );

    f.engine.undo().unwrap();
    assert_eq!(f.engine.project, before);
    // Stack order, ids, tiles, and the X-sheet key all came back.
    let d = drawing(&f);
    assert_eq!(
        d.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
        vec![f.color, f.shadow, f.line]
    );
}

#[test]
fn remove_first_of_two_drawings_undo_restores_library_order() {
    // The library order is user-visible and persisted (ord column): undoing a
    // RemoveDrawing must restore the drawing at its ORIGINAL index, not append.
    let mut f = layer_fixture();
    let d2 = f.engine.alloc_drawing_id();
    let l2 = f.engine.alloc_layer_id();
    f.engine
        .apply(
            "second drawing",
            vec![Command::AddDrawing {
                at: f.at,
                id: d2,
                index: 1,
                name: "cel2".into(),
                strokes: vec![],
                layers: vec![CelLayer::new(l2, "paint")],
            }],
        )
        .unwrap();
    let before = f.engine.project.clone();

    // Remove the FIRST drawing (index 0) — the old push-based inverse would
    // put it back at the END.
    f.engine
        .apply("remove first", vec![Command::RemoveDrawing { at: f.at, id: f.d }])
        .unwrap();
    f.engine.undo().unwrap();
    assert_eq!(f.engine.project, before);
    let cut = f.engine.project.cut(f.at.scene, f.at.cut).unwrap();
    assert_eq!(cut.drawings[0].id, f.d);
    assert_eq!(cut.drawings[1].id, d2);
}

#[test]
fn cross_drawing_duplicate_layer_id_is_rejected() {
    // The store's cel_layers PRIMARY KEY demands cut-wide unique layer ids;
    // the engine must refuse a document the store couldn't save.
    let mut f = layer_fixture();
    let baseline = f.engine.project.clone();
    let d2 = f.engine.alloc_drawing_id();
    let err = f.engine.apply(
        "reuse layer id",
        vec![Command::AddDrawing {
            at: f.at,
            id: d2,
            index: 1,
            name: "cel2".into(),
            strokes: vec![],
            layers: vec![CelLayer::new(f.color, "sneaky")], // id already used in f.d
        }],
    );
    assert!(err.is_err());
    let err2 = f.engine.apply(
        "reuse via AddCelLayer",
        vec![Command::AddCelLayer {
            at: f.at,
            drawing: f.d,
            index: 0,
            layer: CelLayer::new(f.color, "sneaky"),
        }],
    );
    assert!(err2.is_err());
    let mut p = f.engine.project.clone();
    p.next_id = baseline.next_id;
    assert_eq!(p, baseline);
}

#[test]
fn out_of_range_opacity_is_rejected() {
    let mut f = layer_fixture();
    for bad in [-0.5f32, 1.5, f32::NAN, f32::INFINITY] {
        let err = f.engine.apply(
            "bad opacity",
            vec![Command::SetCelLayerProps {
                at: f.at,
                drawing: f.d,
                layer: f.line,
                props: LayerProps {
                    name: "line".into(),
                    visible: true,
                    opacity: bad,
                },
            }],
        );
        assert!(err.is_err(), "opacity {bad} should be rejected");
    }
}

#[test]
fn duplicate_coord_paint_diff_undoes_exactly() {
    // A diff touching the same coord twice: forward = last write wins;
    // the reversed inverse must restore the ORIGINAL state, not the midpoint.
    let mut f = layer_fixture();
    let before = f.engine.project.clone();
    f.engine
        .apply(
            "double write",
            vec![Command::PaintTiles {
                at: f.at,
                id: f.d,
                layer: f.color,
                diff: vec![
                    ((0, 0), None, Some(solid_tile(0x1111))),
                    ((0, 0), Some(solid_tile(0x1111)), Some(solid_tile(0x2222))),
                ],
            }],
        )
        .unwrap();
    assert_eq!(
        drawing(&f).layer(f.color).unwrap().raster.tiles[&(0, 0)].rgba[0],
        0x2222
    );
    f.engine.undo().unwrap();
    assert_eq!(f.engine.project, before);
}

#[test]
fn batch_failure_mid_batch_leaves_document_untouched() {
    let mut f = layer_fixture();
    let baseline = f.engine.project.clone();
    // A merge-down-like batch whose middle command references a bogus layer.
    let bogus = LayerId(999_999);
    let err = f.engine.apply(
        "broken merge",
        vec![
            Command::PaintTiles {
                at: f.at,
                id: f.d,
                layer: f.color,
                diff: vec![((0, 0), None, Some(solid_tile(0xAAAA)))],
            },
            Command::RemoveCelLayer {
                at: f.at,
                drawing: f.d,
                layer: bogus, // fails here
            },
            Command::SetCelLayerProps {
                at: f.at,
                drawing: f.d,
                layer: f.line,
                props: LayerProps::default(),
            },
        ],
    );
    assert!(err.is_err());
    assert_eq!(f.engine.project, baseline);
}

// ---- Hash laws ------------------------------------------------------------

#[test]
fn hash_law_pixels_order_props_count_rename_does_not() {
    let mut f = layer_fixture();
    let h0 = drawing(&f).content_hash();

    // Rename: hash UNCHANGED.
    f.engine
        .apply(
            "rename",
            vec![Command::SetCelLayerProps {
                at: f.at,
                drawing: f.d,
                layer: f.line,
                props: LayerProps {
                    name: "linework".into(),
                    ..drawing(&f).layer(f.line).unwrap().props.clone()
                },
            }],
        )
        .unwrap();
    assert_eq!(drawing(&f).content_hash(), h0, "rename must not invalidate");

    // Visibility: hash CHANGES.
    f.engine
        .apply(
            "hide",
            vec![Command::SetCelLayerProps {
                at: f.at,
                drawing: f.d,
                layer: f.line,
                props: LayerProps {
                    name: "linework".into(),
                    visible: false,
                    opacity: 1.0,
                },
            }],
        )
        .unwrap();
    let h_hidden = drawing(&f).content_hash();
    assert_ne!(h_hidden, h0);

    // Opacity: hash CHANGES.
    f.engine.undo().unwrap();
    f.engine
        .apply(
            "dim",
            vec![Command::SetCelLayerProps {
                at: f.at,
                drawing: f.d,
                layer: f.line,
                props: LayerProps {
                    name: "linework".into(),
                    visible: true,
                    opacity: 0.5,
                },
            }],
        )
        .unwrap();
    assert_ne!(drawing(&f).content_hash(), h0);
    f.engine.undo().unwrap();

    // Paint: hash CHANGES.
    f.engine
        .apply(
            "paint",
            vec![Command::PaintTiles {
                at: f.at,
                id: f.d,
                layer: f.color,
                diff: vec![((0, 0), None, Some(solid_tile(0xBEEF)))],
            }],
        )
        .unwrap();
    let h_painted = drawing(&f).content_hash();
    assert_ne!(h_painted, h0);

    // Reorder: hash CHANGES — but only when the layers are DISTINGUISHABLE
    // (reordering identical empty layers is a no-op composite, and the hash
    // correctly agrees). color now carries paint, so moving it matters.
    f.engine
        .apply(
            "reorder",
            vec![Command::MoveCelLayer {
                at: f.at,
                drawing: f.d,
                layer: f.color,
                to_index: 2,
            }],
        )
        .unwrap();
    assert_ne!(drawing(&f).content_hash(), h_painted);
}

// ---- Flatten golden values --------------------------------------------------

#[test]
fn flatten_composites_opacity_overlap_and_missing_tiles() {
    let mut f = layer_fixture();
    // color: opaque mid-gray on tile (0,0) and (1,0).
    // line: 50%-opacity solid on tile (0,0) only (overlap), via layer opacity.
    let gray = {
        // premultiplied: solid alpha, half-intensity color
        let mut v = vec![0u16; TILE_LEN];
        for px in v.chunks_exact_mut(4) {
            px[0] = 0x8000;
            px[1] = 0x8000;
            px[2] = 0x8000;
            px[3] = 0xFFFF;
        }
        Arc::new(TileData::from_vec(v))
    };
    let ink = {
        let mut v = vec![0u16; TILE_LEN];
        for px in v.chunks_exact_mut(4) {
            px[0] = 0xFFFF;
            px[1] = 0;
            px[2] = 0;
            px[3] = 0xFFFF;
        }
        Arc::new(TileData::from_vec(v))
    };
    f.engine
        .apply(
            "setup",
            vec![
                Command::PaintTiles {
                    at: f.at,
                    id: f.d,
                    layer: f.color,
                    diff: vec![
                        ((0, 0), None, Some(gray.clone())),
                        ((1, 0), None, Some(gray.clone())),
                    ],
                },
                Command::PaintTiles {
                    at: f.at,
                    id: f.d,
                    layer: f.line,
                    diff: vec![((0, 0), None, Some(ink))],
                },
                Command::SetCelLayerProps {
                    at: f.at,
                    drawing: f.d,
                    layer: f.line,
                    props: LayerProps {
                        name: "line".into(),
                        visible: true,
                        opacity: 0.5,
                    },
                },
            ],
        )
        .unwrap();

    let flat = drawing(&f).flatten();
    assert_eq!(flat.len(), 2);

    // Tile (1,0): color only — passes through unchanged.
    let t10 = &flat[&(1, 0)].rgba;
    assert_eq!(&t10[0..4], &[0x8000, 0x8000, 0x8000, 0xFFFF]);

    // Tile (0,0): line at 50% over gray.
    // src(prem, x0.5) = (0.5, 0, 0, 0.5); out = src + dst*(1-0.5)
    // r = 0.5 + 0.5*0.5 = 0.75 ; g/b = 0.25 ; a = 1.0
    let t00 = &flat[&(0, 0)].rgba;
    let px = [t00[0], t00[1], t00[2], t00[3]];
    let close = |v: u16, expect: f32| ((v as f32 / 65535.0) - expect).abs() < 0.002;
    assert!(close(px[0], 0.75), "r was {}", px[0] as f32 / 65535.0);
    assert!(close(px[1], 0.25), "g was {}", px[1] as f32 / 65535.0);
    assert!(close(px[2], 0.25), "b was {}", px[2] as f32 / 65535.0);
    assert!(close(px[3], 1.0), "a was {}", px[3] as f32 / 65535.0);

    // Hidden layers contribute nothing.
    f.engine
        .apply(
            "hide line",
            vec![Command::SetCelLayerProps {
                at: f.at,
                drawing: f.d,
                layer: f.line,
                props: LayerProps {
                    name: "line".into(),
                    visible: false,
                    opacity: 0.5,
                },
            }],
        )
        .unwrap();
    let flat2 = drawing(&f).flatten();
    assert_eq!(&flat2[&(0, 0)].rgba[0..4], &[0x8000, 0x8000, 0x8000, 0xFFFF]);
}

// ---- Persistence: v5 round-trip + v4 migration ------------------------------

#[test]
fn three_layer_stack_survives_sqlite_roundtrip() {
    let mut f = layer_fixture();
    f.engine
        .apply(
            "paint",
            vec![
                Command::PaintTiles {
                    at: f.at,
                    id: f.d,
                    layer: f.shadow,
                    diff: vec![((2, -1), None, Some(solid_tile(0x1234)))],
                },
                Command::SetCelLayerProps {
                    at: f.at,
                    drawing: f.d,
                    layer: f.shadow,
                    props: LayerProps {
                        name: "shadow".into(),
                        visible: false,
                        opacity: 0.25,
                    },
                },
            ],
        )
        .unwrap();

    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("layers_{}.animproj", std::process::id()));
    let _ = std::fs::remove_file(&path);

    f.engine.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    assert_eq!(f.engine.project, loaded.project);

    // Re-save + re-load is stable.
    loaded.save(&path).unwrap();
    let again = Engine::load(&path).unwrap();
    assert_eq!(loaded.project, again.project);

    std::fs::remove_file(&path).unwrap();
}

/// Forge a minimal schema-v4 file directly (the legacy single-raster format)
/// and prove it loads as exactly one "paint" layer with opacity preserved,
/// vector-only drawings stay layerless, and the re-save round-trips as v5.
#[test]
fn v4_file_migrates_to_one_layer() {
    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("v4_{}.animproj", std::process::id()));
    let _ = std::fs::remove_file(&path);

    // Build the v4 file byte-for-byte the way the old store did.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE scenes(id INTEGER PRIMARY KEY, name TEXT NOT NULL, ord INTEGER NOT NULL);
             CREATE TABLE cuts(id INTEGER PRIMARY KEY, scene_id INTEGER NOT NULL, name TEXT NOT NULL,
                 frame_count INTEGER NOT NULL, output_node INTEGER, ord INTEGER NOT NULL);
             CREATE TABLE drawings(id INTEGER PRIMARY KEY, cut_id INTEGER NOT NULL, name TEXT NOT NULL,
                 payload TEXT NOT NULL, ord INTEGER NOT NULL);
             CREATE TABLE columns(id INTEGER PRIMARY KEY, cut_id INTEGER NOT NULL, name TEXT NOT NULL,
                 ord INTEGER NOT NULL);
             CREATE TABLE cells(column_id INTEGER NOT NULL, frame INTEGER NOT NULL,
                 exposure TEXT NOT NULL, drawing_id INTEGER, PRIMARY KEY(column_id, frame));
             CREATE TABLE nodes(id INTEGER PRIMARY KEY, cut_id INTEGER NOT NULL, kind TEXT NOT NULL);
             CREATE TABLE links(cut_id INTEGER NOT NULL, to_node INTEGER NOT NULL, to_pin INTEGER NOT NULL,
                 from_node INTEGER NOT NULL, from_pin INTEGER NOT NULL, PRIMARY KEY(to_node, to_pin));
             CREATE TABLE raster_layers(drawing_id INTEGER PRIMARY KEY, opacity REAL NOT NULL);
             CREATE TABLE tiles(drawing_id INTEGER NOT NULL, tx INTEGER NOT NULL, ty INTEGER NOT NULL,
                 bytes BLOB NOT NULL, PRIMARY KEY(drawing_id, tx, ty));",
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO meta VALUES ('schema_version','4');
             INSERT INTO meta VALUES ('name','legacy');
             INSERT INTO meta VALUES ('fps','24');
             INSERT INTO meta VALUES ('width','1920');
             INSERT INTO meta VALUES ('height','1080');
             INSERT INTO meta VALUES ('dpi','300');
             INSERT INTO meta VALUES ('next_id','10');
             INSERT INTO scenes VALUES (1,'S',0);
             INSERT INTO cuts VALUES (2,1,'C',12,NULL,0);
             INSERT INTO drawings VALUES (3,2,'raster cel','[]',0);
             INSERT INTO drawings VALUES (4,2,'vector cel','[]',1);
             INSERT INTO columns VALUES (5,2,'A',0);
             INSERT INTO cells VALUES (5,0,'drawing',3);
             INSERT INTO raster_layers VALUES (3,0.75);",
        )
        .unwrap();
        let tile = solid_tile(0x4242);
        conn.execute(
            "INSERT INTO tiles(drawing_id, tx, ty, bytes) VALUES (3, 1, 2, ?1)",
            (tile.as_bytes(),),
        )
        .unwrap();
    }

    let engine = Engine::load(&path).unwrap();
    let cut = engine.project.cut(SceneId(1), CutId(2)).unwrap();

    // The raster drawing became exactly one "paint" layer, opacity preserved.
    let d = cut.drawing(DrawingId(3)).unwrap();
    assert_eq!(d.layers.len(), 1);
    let layer = &d.layers[0];
    assert_eq!(layer.props.name, "paint");
    assert_eq!(layer.props.opacity, 0.75);
    assert!(layer.props.visible);
    assert_eq!(layer.raster.tiles.len(), 1);
    assert_eq!(layer.raster.tiles[&(1, 2)].rgba[0], 0x4242);
    // The minted LayerId came from next_id (>= 10), never colliding.
    assert!(layer.id.0 >= 10);
    assert!(engine.project.next_id > 10);

    // The vector-only drawing stays layerless.
    assert!(cut.drawing(DrawingId(4)).unwrap().layers.is_empty());

    // Re-save (now v5) and re-load: identical, and the legacy rows are gone.
    engine.save(&path).unwrap();
    let again = Engine::load(&path).unwrap();
    assert_eq!(engine.project, again.project);
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        let legacy_tiles: i64 = conn
            .query_row("SELECT COUNT(*) FROM tiles", (), |r| r.get(0))
            .unwrap();
        assert_eq!(legacy_tiles, 0, "legacy rows purged on v5 save");
        let v: String = conn
            .query_row("SELECT value FROM meta WHERE key='schema_version'", (), |r| r.get(0))
            .unwrap();
        assert_eq!(v, "5");
    }

    std::fs::remove_file(&path).unwrap();
}
