//! Golden tests for the M1 engine: X-sheet holds, evaluation, caching &
//! partial invalidation, cycle safety, undo/redo exactness, SQLite round-trip.

use anim_core::command::{Command, CutRef};
use anim_core::error::EngineError;
use anim_core::graph::{BlendMode, NodeKind};
use anim_core::ids::*;
use anim_core::xsheet::Exposure;
use anim_core::Engine;

/// Standard fixture: one scene, one 24-frame cut, one column with
/// drawings A (frame 0) and B (frame 6), graph:
///   DrawingSource -> Transform -> Blend(in0) <- Solid(in1) ; Blend -> Output
struct Fixture {
    engine: Engine,
    at: CutRef,
    scene: SceneId,
    cut: CutId,
    col: ColumnId,
    d_a: DrawingId,
    d_b: DrawingId,
    n_src: NodeId,
    n_xform: NodeId,
    n_solid: NodeId,
    n_blend: NodeId,
    n_out: NodeId,
}

fn fixture() -> Fixture {
    let mut engine = Engine::new("test project");
    let scene = engine.add_scene("SC01");
    let cut = engine.add_cut(scene, "cut A", 24).unwrap();
    let at = CutRef { scene, cut };
    let col = engine.add_column(at, "A").unwrap();

    let d_a = engine.alloc_drawing_id();
    let d_b = engine.alloc_drawing_id();
    let n_src = engine.alloc_node_id();
    let n_xform = engine.alloc_node_id();
    let n_solid = engine.alloc_node_id();
    let n_blend = engine.alloc_node_id();
    let n_out = engine.alloc_node_id();

    engine
        .apply(
            "build fixture",
            vec![
                Command::AddDrawing {
                    at,
                    id: d_a,
                    name: "luffy_a".into(),
                    strokes: vec![],
                },
                Command::AddDrawing {
                    at,
                    id: d_b,
                    name: "luffy_b".into(),
                    strokes: vec![],
                },
                Command::SetCell {
                    at,
                    column: col,
                    frame: 0,
                    key: Some(Exposure::Drawing(d_a)),
                },
                Command::SetCell {
                    at,
                    column: col,
                    frame: 6,
                    key: Some(Exposure::Drawing(d_b)),
                },
                Command::AddNode {
                    at,
                    id: n_src,
                    kind: NodeKind::DrawingSource { column: col },
                },
                Command::AddNode {
                    at,
                    id: n_xform,
                    kind: NodeKind::Transform {
                        translate: (2.0, 0.0),
                        scale: 1.0,
                        rotate_deg: 0.0,
                    },
                },
                Command::AddNode {
                    at,
                    id: n_solid,
                    kind: NodeKind::Solid {
                        rgba: [16, 32, 48, 255],
                    },
                },
                Command::AddNode {
                    at,
                    id: n_blend,
                    kind: NodeKind::Blend {
                        mode: BlendMode::Normal,
                    },
                },
                Command::AddNode {
                    at,
                    id: n_out,
                    kind: NodeKind::Output,
                },
                Command::Connect {
                    at,
                    from: n_src,
                    from_pin: 0,
                    to: n_xform,
                    to_pin: 0,
                },
                Command::Connect {
                    at,
                    from: n_xform,
                    from_pin: 0,
                    to: n_blend,
                    to_pin: 0,
                },
                Command::Connect {
                    at,
                    from: n_solid,
                    from_pin: 0,
                    to: n_blend,
                    to_pin: 1,
                },
                Command::Connect {
                    at,
                    from: n_blend,
                    from_pin: 0,
                    to: n_out,
                    to_pin: 0,
                },
                Command::SetOutput {
                    at,
                    node: Some(n_out),
                },
            ],
        )
        .unwrap();

    // The fixture build is setup, not part of any test's edit history.
    engine.clear_history();

    Fixture {
        engine,
        at,
        scene,
        cut,
        col,
        d_a,
        d_b,
        n_src,
        n_xform,
        n_solid,
        n_blend,
        n_out,
    }
}

#[test]
fn xsheet_hold_semantics() {
    let f = fixture();
    let cut = f.engine.project.cut(f.scene, f.cut).unwrap();
    let col = cut.xsheet.column(f.col).unwrap();

    // Key at 0 holds through 5, key at 6 holds onward.
    for frame in 0..6 {
        assert_eq!(col.resolve(frame), Some(f.d_a), "frame {frame}");
    }
    for frame in 6..24 {
        assert_eq!(col.resolve(frame), Some(f.d_b), "frame {frame}");
    }
}

#[test]
fn xsheet_empty_key_ends_hold_and_pre_first_key_is_empty() {
    let mut f = fixture();
    f.engine
        .apply(
            "empty key at 12",
            vec![Command::SetCell {
                at: f.at,
                column: f.col,
                frame: 12,
                key: Some(Exposure::Empty),
            }],
        )
        .unwrap();
    let cut = f.engine.project.cut(f.scene, f.cut).unwrap();
    let col = cut.xsheet.column(f.col).unwrap();
    assert_eq!(col.resolve(11), Some(f.d_b));
    assert_eq!(col.resolve(12), None);
    assert_eq!(col.resolve(23), None);

    // A column whose first key is at frame 3 resolves nothing before it.
    let col2 = f.engine.add_column(f.at, "B").unwrap();
    f.engine
        .apply(
            "late first key",
            vec![Command::SetCell {
                at: f.at,
                column: col2,
                frame: 3,
                key: Some(Exposure::Drawing(f.d_a)),
            }],
        )
        .unwrap();
    let cut = f.engine.project.cut(f.scene, f.cut).unwrap();
    let col2 = cut.xsheet.column(col2).unwrap();
    assert_eq!(col2.resolve(0), None);
    assert_eq!(col2.resolve(2), None);
    assert_eq!(col2.resolve(3), Some(f.d_a));
}

#[test]
fn eval_produces_exact_recipes_and_respects_timing() {
    let mut f = fixture();
    let hash_a = f
        .engine
        .project
        .cut(f.scene, f.cut)
        .unwrap()
        .drawing(f.d_a)
        .unwrap()
        .content_hash();
    let v0 = f.engine.eval(f.scene, f.cut, 0).unwrap();
    assert_eq!(
        v0.recipe(),
        format!(
            "out(blend(Normal, xform(t=(2.000,0.000),s=1.000,r=0.000, drawing({}:'luffy_a'#{:016x})), solid(#102030ff)))",
            f.d_a, hash_a
        )
    );
    let v6 = f.engine.eval(f.scene, f.cut, 6).unwrap();
    assert!(v6.recipe().contains("luffy_b"));
    assert_ne!(v0.hash(), v6.hash());

    // Determinism: same input -> same hash, every time.
    let v0_again = f.engine.eval(f.scene, f.cut, 0).unwrap();
    assert_eq!(v0.hash(), v0_again.hash());
}

#[test]
fn eval_caches_and_invalidates_only_downstream() {
    let mut f = fixture();
    let frames = 24u32;
    for frame in 0..frames {
        f.engine.eval(f.scene, f.cut, frame).unwrap();
    }
    let after_first = f.engine.evaluator.stats;
    // 5 nodes x 24 frames, each computed exactly once.
    assert_eq!(after_first.computed, 5 * frames as u64);

    // Re-evaluating everything touches only the cache.
    for frame in 0..frames {
        f.engine.eval(f.scene, f.cut, frame).unwrap();
    }
    let after_second = f.engine.evaluator.stats;
    assert_eq!(after_second.computed, after_first.computed, "no recompute");
    assert!(after_second.cache_hits > after_first.cache_hits);

    // Edit the Transform's params: Transform, Blend, Output recompute (3 nodes);
    // DrawingSource and Solid (upstream / sibling) stay cached.
    f.engine
        .apply(
            "nudge transform",
            vec![Command::SetNodeKind {
                at: f.at,
                id: f.n_xform,
                kind: NodeKind::Transform {
                    translate: (5.0, 1.0),
                    scale: 2.0,
                    rotate_deg: 15.0,
                },
            }],
        )
        .unwrap();
    for frame in 0..frames {
        f.engine.eval(f.scene, f.cut, frame).unwrap();
    }
    let after_edit = f.engine.evaluator.stats;
    assert_eq!(
        after_edit.computed - after_second.computed,
        3 * frames as u64,
        "exactly the downstream closure recomputes"
    );
}

#[test]
fn xsheet_edit_invalidates_source_chain() {
    let mut f = fixture();
    for frame in 0..24 {
        f.engine.eval(f.scene, f.cut, frame).unwrap();
    }
    let before = f.engine.evaluator.stats;

    // Retime: move B's key from 6 to 12. Source, Transform, Blend, Output
    // recompute (4 nodes); Solid stays cached.
    f.engine
        .apply(
            "retime",
            vec![
                Command::SetCell {
                    at: f.at,
                    column: f.col,
                    frame: 6,
                    key: None,
                },
                Command::SetCell {
                    at: f.at,
                    column: f.col,
                    frame: 12,
                    key: Some(Exposure::Drawing(f.d_b)),
                },
            ],
        )
        .unwrap();
    for frame in 0..24 {
        f.engine.eval(f.scene, f.cut, frame).unwrap();
    }
    let after = f.engine.evaluator.stats;
    assert_eq!(after.computed - before.computed, 4 * 24);

    // And the retime actually changed what's on screen at frame 8.
    let v8 = f.engine.eval(f.scene, f.cut, 8).unwrap();
    assert!(v8.recipe().contains("luffy_a"), "frame 8 now holds A");
}

#[test]
fn cycles_are_rejected() {
    let mut f = fixture();
    // blend feeds xform which feeds blend: refuse.
    let err = f
        .engine
        .apply(
            "make a cycle",
            vec![Command::Connect {
                at: f.at,
                from: f.n_blend,
                from_pin: 0,
                to: f.n_xform,
                to_pin: 0,
            }],
        )
        .unwrap_err();
    assert!(matches!(err, EngineError::InvalidCommand(_)));

    // Self-loops refuse too.
    let err = f
        .engine
        .apply(
            "self loop",
            vec![Command::Connect {
                at: f.at,
                from: f.n_xform,
                from_pin: 0,
                to: f.n_xform,
                to_pin: 0,
            }],
        )
        .unwrap_err();
    assert!(matches!(err, EngineError::InvalidCommand(_)));
}

#[test]
fn unconnected_inputs_evaluate_as_empty() {
    let mut f = fixture();
    f.engine
        .apply(
            "unplug blend input 1",
            vec![Command::Disconnect {
                at: f.at,
                to: f.n_blend,
                to_pin: 1,
            }],
        )
        .unwrap();
    let v = f.engine.eval(f.scene, f.cut, 0).unwrap();
    assert!(v.recipe().ends_with("empty))"));
}

#[test]
fn undo_redo_restore_exact_states() {
    let mut f = fixture();
    let baseline = f.engine.project.clone();
    let v_before = f.engine.eval(f.scene, f.cut, 6).unwrap();

    // Three separate undo steps: retime, param edit, node removal.
    f.engine
        .apply(
            "retime",
            vec![Command::SetCell {
                at: f.at,
                column: f.col,
                frame: 6,
                key: None,
            }],
        )
        .unwrap();
    f.engine
        .apply(
            "recolor solid",
            vec![Command::SetNodeKind {
                at: f.at,
                id: f.n_solid,
                kind: NodeKind::Solid {
                    rgba: [255, 0, 0, 255],
                },
            }],
        )
        .unwrap();
    f.engine
        .apply(
            "remove blend",
            vec![Command::RemoveNode {
                at: f.at,
                id: f.n_blend,
            }],
        )
        .unwrap();

    let edited = f.engine.project.clone();

    // Undo everything: byte-for-byte the original document.
    f.engine.undo().unwrap();
    f.engine.undo().unwrap();
    f.engine.undo().unwrap();
    assert_eq!(f.engine.project, baseline);
    assert!(!f.engine.can_undo());

    // Evaluation agrees, not just structure.
    let v_after_undo = f.engine.eval(f.scene, f.cut, 6).unwrap();
    assert_eq!(v_before.hash(), v_after_undo.hash());

    // Redo everything: byte-for-byte the edited document.
    f.engine.redo().unwrap();
    f.engine.redo().unwrap();
    f.engine.redo().unwrap();
    assert_eq!(f.engine.project, edited);
    assert!(!f.engine.can_redo());
}

#[test]
fn remove_node_undo_restores_all_wiring() {
    let mut f = fixture();
    let baseline = f.engine.project.clone();

    // Blend sits mid-graph with 2 inputs, 1 consumer, and Output downstream.
    f.engine
        .apply(
            "remove blend",
            vec![Command::RemoveNode {
                at: f.at,
                id: f.n_blend,
            }],
        )
        .unwrap();

    // Its consumer's input must now be empty.
    let cut = f.engine.project.cut(f.scene, f.cut).unwrap();
    assert_eq!(cut.graph.node(f.n_out).unwrap().inputs[0], None);

    f.engine.undo().unwrap();
    assert_eq!(f.engine.project, baseline, "wiring fully restored");
}

#[test]
fn failed_batch_rolls_back_completely() {
    let mut f = fixture();
    let baseline = f.engine.project.clone();

    // Second command fails (unknown node) -> first must roll back.
    let err = f.engine.apply(
        "bad batch",
        vec![
            Command::SetCell {
                at: f.at,
                column: f.col,
                frame: 3,
                key: Some(Exposure::Drawing(f.d_a)),
            },
            Command::Connect {
                at: f.at,
                from: NodeId(999_999),
                from_pin: 0,
                to: f.n_out,
                to_pin: 0,
            },
        ],
    );
    assert!(err.is_err());
    assert_eq!(f.engine.project, baseline, "atomic batch");
    assert!(!f.engine.can_undo(), "failed batch leaves no history");
}

#[test]
fn removing_a_drawing_clears_and_restores_its_exposures() {
    let mut f = fixture();
    let baseline = f.engine.project.clone();

    f.engine
        .apply(
            "remove drawing B",
            vec![Command::RemoveDrawing { at: f.at, id: f.d_b }],
        )
        .unwrap();

    // Frame 6's key is gone; A's hold now extends across the whole cut.
    let v6 = f.engine.eval(f.scene, f.cut, 6).unwrap();
    assert!(v6.recipe().contains("luffy_a"));

    f.engine.undo().unwrap();
    assert_eq!(f.engine.project, baseline);
    let v6_back = f.engine.eval(f.scene, f.cut, 6).unwrap();
    assert!(v6_back.recipe().contains("luffy_b"));
}

fn test_stroke(seed: f32) -> anim_core::model::Stroke {
    anim_core::model::Stroke {
        points: vec![
            anim_core::model::StrokePoint {
                x: seed,
                y: 10.0,
                pressure: 0.4,
            },
            anim_core::model::StrokePoint {
                x: seed + 5.0,
                y: 12.0,
                pressure: 0.9,
            },
        ],
        base_width: 3.0,
        color: [20, 20, 25, 255],
    }
}

#[test]
fn strokes_are_undoable_and_change_eval_content() {
    let mut f = fixture();
    let baseline = f.engine.project.clone();
    let v_before = f.engine.eval(f.scene, f.cut, 0).unwrap();

    f.engine
        .apply(
            "draw",
            vec![Command::AddStroke {
                at: f.at,
                id: f.d_a,
                stroke: test_stroke(1.0),
            }],
        )
        .unwrap();

    // Artwork edit shows up in the evaluated value (content hash changed)...
    let v_after = f.engine.eval(f.scene, f.cut, 0).unwrap();
    assert_ne!(v_before.hash(), v_after.hash(), "content edit must be seen");

    // ...and undo restores both the document and the evaluated value exactly.
    f.engine.undo().unwrap();
    assert_eq!(f.engine.project, baseline);
    let v_undone = f.engine.eval(f.scene, f.cut, 0).unwrap();
    assert_eq!(v_before.hash(), v_undone.hash());

    // Popping an empty drawing is rejected without corrupting anything.
    let err = f.engine.apply(
        "bad pop",
        vec![Command::PopStroke { at: f.at, id: f.d_a }],
    );
    assert!(err.is_err());
    assert_eq!(f.engine.project, baseline);
}

#[test]
fn stroke_edits_invalidate_only_the_source_chain() {
    let mut f = fixture();
    for frame in 0..24 {
        f.engine.eval(f.scene, f.cut, frame).unwrap();
    }
    let before = f.engine.evaluator.stats;

    // Drawing A is exposed on the column: editing it recomputes Source,
    // Transform, Blend, Output (4 nodes); Solid stays cached.
    f.engine
        .apply(
            "draw on A",
            vec![Command::AddStroke {
                at: f.at,
                id: f.d_a,
                stroke: test_stroke(2.0),
            }],
        )
        .unwrap();
    for frame in 0..24 {
        f.engine.eval(f.scene, f.cut, frame).unwrap();
    }
    let after = f.engine.evaluator.stats;
    assert_eq!(after.computed - before.computed, 4 * 24);
}

#[test]
fn strokes_survive_sqlite_roundtrip() {
    let mut f = fixture();
    f.engine
        .apply(
            "draw",
            vec![
                Command::AddStroke {
                    at: f.at,
                    id: f.d_a,
                    stroke: test_stroke(1.0),
                },
                Command::AddStroke {
                    at: f.at,
                    id: f.d_a,
                    stroke: test_stroke(7.0),
                },
            ],
        )
        .unwrap();

    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("strokes_{}.animproj", std::process::id()));
    let _ = std::fs::remove_file(&path);

    f.engine.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    assert_eq!(f.engine.project, loaded.project);

    let cut = loaded.project.cut(f.scene, f.cut).unwrap();
    assert_eq!(cut.drawing(f.d_a).unwrap().strokes.len(), 2);

    std::fs::remove_file(&path).unwrap();
}

#[test]
fn sqlite_roundtrip_preserves_everything() {
    let f = fixture();
    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("roundtrip_{}.animproj", std::process::id()));
    let _ = std::fs::remove_file(&path);

    f.engine.save(&path).unwrap();
    let mut loaded = Engine::load(&path).unwrap();

    assert_eq!(f.engine.project, loaded.project);

    // Loaded project evaluates to identical results.
    let mut original = f.engine;
    let a = original.eval(f.scene, f.cut, 6).unwrap();
    let b = loaded.eval(f.scene, f.cut, 6).unwrap();
    assert_eq!(a, b);

    std::fs::remove_file(&path).unwrap();
}

#[test]
fn save_is_a_full_rewrite_not_an_append() {
    let mut f = fixture();
    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("rewrite_{}.animproj", std::process::id()));
    let _ = std::fs::remove_file(&path);

    f.engine.save(&path).unwrap();

    // Mutate (remove a node) and save again over the same file.
    f.engine
        .apply(
            "remove solid",
            vec![Command::RemoveNode {
                at: f.at,
                id: f.n_solid,
            }],
        )
        .unwrap();
    f.engine.save(&path).unwrap();

    let loaded = Engine::load(&path).unwrap();
    assert_eq!(f.engine.project, loaded.project);
    let cut = loaded.project.cut(f.scene, f.cut).unwrap();
    assert!(!cut.graph.contains(f.n_solid), "stale rows must not survive");

    std::fs::remove_file(&path).unwrap();
}
