//! Parameter columns (the C1 camera model): interpolation semantics,
//! SetParamKey undo exactness, Transform binding through the evaluator,
//! and SQLite round-trip.

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::graph::{NodeKind, TransformBinds};
use anim_core::ids::*;
use anim_core::xsheet::{ParamColumn, ParamInterp, ParamKey};

fn key(value: f32, interp: ParamInterp) -> ParamKey {
    ParamKey { value, interp }
}

// ---- Pure resolve semantics -------------------------------------------------

#[test]
fn resolve_empty_column_is_none() {
    let col = ParamColumn::new(ParamId(1), "cam.x");
    assert_eq!(col.resolve(0), None);
    assert_eq!(col.resolve(100), None);
}

#[test]
fn resolve_holds_first_value_before_first_key_and_last_after_last() {
    let mut col = ParamColumn::new(ParamId(1), "cam.x");
    col.set_key(10, key(100.0, ParamInterp::Linear));
    col.set_key(20, key(200.0, ParamInterp::Linear));
    assert_eq!(col.resolve(0), Some(100.0)); // camera holds its opening pose
    assert_eq!(col.resolve(10), Some(100.0));
    assert_eq!(col.resolve(20), Some(200.0));
    assert_eq!(col.resolve(999), Some(200.0));
}

#[test]
fn resolve_linear_interpolates_between_keys() {
    let mut col = ParamColumn::new(ParamId(1), "cam.x");
    col.set_key(10, key(100.0, ParamInterp::Linear));
    col.set_key(20, key(200.0, ParamInterp::Linear));
    assert_eq!(col.resolve(15), Some(150.0));
    assert_eq!(col.resolve(12), Some(120.0));
}

#[test]
fn resolve_hold_steps_at_the_next_key() {
    let mut col = ParamColumn::new(ParamId(1), "cam.x");
    col.set_key(10, key(1.0, ParamInterp::Hold));
    col.set_key(20, key(2.0, ParamInterp::Hold));
    assert_eq!(col.resolve(19), Some(1.0)); // held right up to the key
    assert_eq!(col.resolve(20), Some(2.0));
}

#[test]
fn resolve_ease_is_smoothstep() {
    let mut col = ParamColumn::new(ParamId(1), "cam.x");
    col.set_key(0, key(0.0, ParamInterp::Ease));
    col.set_key(4, key(1.0, ParamInterp::Ease));
    // t=0.5 -> 0.5 (symmetric), t=0.25 -> 0.15625 (slow start).
    assert_eq!(col.resolve(2), Some(0.5));
    assert_eq!(col.resolve(1), Some(0.15625));
}

#[test]
fn resolve_uses_the_earlier_keys_interp() {
    let mut col = ParamColumn::new(ParamId(1), "cam.x");
    col.set_key(0, key(0.0, ParamInterp::Hold));
    col.set_key(10, key(10.0, ParamInterp::Linear)); // interp of the SPAN AFTER it
    col.set_key(20, key(20.0, ParamInterp::Hold));
    assert_eq!(col.resolve(5), Some(0.0)); // first span holds
    assert_eq!(col.resolve(15), Some(15.0)); // second span is linear
}

// ---- Engine-level: undo exactness, eval, persistence -------------------------

fn engine_with_param() -> (Engine, CutRef, SceneId, CutId, ParamId) {
    let mut engine = Engine::new("param test");
    let scene = engine.add_scene("SC01");
    let cut = engine.add_cut(scene, "cut A", 24).unwrap();
    let at = CutRef { scene, cut };
    let param = engine.add_param_column(at, "cam.x").unwrap();
    (engine, at, scene, cut, param)
}

#[test]
fn set_param_key_undo_redo_is_exact() {
    let (mut engine, at, _, _, param) = engine_with_param();
    let before = engine.project.clone();

    engine
        .apply(
            "key 1",
            vec![Command::SetParamKey {
                at,
                column: param,
                frame: 0,
                key: Some(key(5.0, ParamInterp::Ease)),
            }],
        )
        .unwrap();
    engine
        .apply(
            "key 1 overwrite",
            vec![Command::SetParamKey {
                at,
                column: param,
                frame: 0,
                key: Some(key(7.0, ParamInterp::Hold)),
            }],
        )
        .unwrap();
    let after = engine.project.clone();

    engine.undo().unwrap(); // back to the 5.0/Ease key
    engine.undo().unwrap(); // back to no key
    assert_eq!(engine.project, before);
    engine.redo().unwrap();
    engine.redo().unwrap();
    assert_eq!(engine.project, after);
}

#[test]
fn set_param_key_rejects_non_finite() {
    let (mut engine, at, _, _, param) = engine_with_param();
    let err = engine.apply(
        "bad key",
        vec![Command::SetParamKey {
            at,
            column: param,
            frame: 0,
            key: Some(key(f32::NAN, ParamInterp::Linear)),
        }],
    );
    assert!(err.is_err());
    let err = engine.apply(
        "bad key",
        vec![Command::SetParamKey {
            at,
            column: param,
            frame: 0,
            key: Some(key(f32::INFINITY, ParamInterp::Linear)),
        }],
    );
    assert!(err.is_err());
}

#[test]
fn set_param_key_on_unknown_column_errors() {
    let (mut engine, at, _, _, _) = engine_with_param();
    let err = engine.apply(
        "ghost column",
        vec![Command::SetParamKey {
            at,
            column: ParamId(9999),
            frame: 0,
            key: Some(key(1.0, ParamInterp::Hold)),
        }],
    );
    assert!(err.is_err());
}

/// A bound Transform's evaluator value must change when a param key moves
/// the camera (and undo must restore the old value) — this is the
/// invalidation path the GPU compositor keys its cache on.
#[test]
fn bound_transform_tracks_param_keys_through_eval() {
    let (mut engine, at, scene, cut, param) = engine_with_param();
    let n_xform = engine.alloc_node_id();
    let n_out = engine.alloc_node_id();
    engine
        .apply(
            "build graph",
            vec![
                Command::AddNode {
                    at,
                    id: n_xform,
                    kind: NodeKind::Transform {
                        translate: (1.0, 2.0),
                        scale: 1.0,
                        rotate_deg: 0.0,
                        binds: TransformBinds {
                            tx: Some(param),
                            ..Default::default()
                        },
                    },
                },
                Command::AddNode {
                    at,
                    id: n_out,
                    kind: NodeKind::Output,
                },
                Command::Connect {
                    at,
                    from: n_xform,
                    from_pin: 0,
                    to: n_out,
                    to_pin: 0,
                },
                Command::SetOutput { at, node: Some(n_out) },
            ],
        )
        .unwrap();

    // Unkeyed bound column falls back to the STATIC value.
    let v = engine.eval(scene, cut, 0).unwrap();
    assert!(v.recipe().contains("t=(1,2)"), "recipe: {}", v.recipe());

    // Key a pan: 0 at frame 0 → 100 at frame 10, linear.
    engine
        .apply(
            "pan",
            vec![
                Command::SetParamKey {
                    at,
                    column: param,
                    frame: 0,
                    key: Some(key(0.0, ParamInterp::Linear)),
                },
                Command::SetParamKey {
                    at,
                    column: param,
                    frame: 10,
                    key: Some(key(100.0, ParamInterp::Linear)),
                },
            ],
        )
        .unwrap();
    let v0 = engine.eval(scene, cut, 0).unwrap();
    let v5 = engine.eval(scene, cut, 5).unwrap();
    let v10 = engine.eval(scene, cut, 10).unwrap();
    assert!(v0.recipe().contains("t=(0,2)"), "recipe: {}", v0.recipe());
    assert!(v5.recipe().contains("t=(50,2)"), "recipe: {}", v5.recipe());
    assert!(v10.recipe().contains("t=(100,2)"), "recipe: {}", v10.recipe());

    // Undo the pan: the cache must invalidate back to the static fallback.
    engine.undo().unwrap();
    let v_back = engine.eval(scene, cut, 5).unwrap();
    assert!(
        v_back.recipe().contains("t=(1,2)"),
        "recipe: {}",
        v_back.recipe()
    );
}

#[test]
fn param_columns_and_binds_survive_sqlite_roundtrip() {
    let (mut engine, at, _, _, param) = engine_with_param();
    let n_xform = engine.alloc_node_id();
    engine
        .apply(
            "build",
            vec![
                Command::AddNode {
                    at,
                    id: n_xform,
                    kind: NodeKind::Transform {
                        translate: (0.0, 0.0),
                        scale: 1.0,
                        rotate_deg: 0.0,
                        binds: TransformBinds {
                            tx: Some(param),
                            scale: Some(param),
                            ..Default::default()
                        },
                    },
                },
                Command::SetParamKey {
                    at,
                    column: param,
                    frame: 0,
                    key: Some(key(-12.5, ParamInterp::Hold)),
                },
                Command::SetParamKey {
                    at,
                    column: param,
                    frame: 8,
                    key: Some(key(40.0, ParamInterp::Linear)),
                },
                Command::SetParamKey {
                    at,
                    column: param,
                    frame: 23,
                    key: Some(key(0.25, ParamInterp::Ease)),
                },
            ],
        )
        .unwrap();

    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("param_roundtrip.animproj");
    engine.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(loaded.project, engine.project);
}
