//! GM14 curves: the new interpolations (EaseIn, EaseOut, Bezier with
//! handles), the Bezier time solve, the store's interp text, and the
//! SetParamKeys batch the curves pane drags with.

use anim_core::Engine;
use anim_core::command::{Command, CutRef, ParamKeyEdit};
use anim_core::graph::{NodeKind, TransformBinds};
use anim_core::ids::*;
use anim_core::sheet_ops;
use anim_core::xsheet::{BezierSeg, HandleMode, Handles, ParamColumn, ParamInterp, ParamKey};

fn key(value: f32, interp: ParamInterp) -> ParamKey {
    ParamKey {
        value,
        interp,
        handles: None,
    }
}

fn bez(value: f32, h: Handles) -> ParamKey {
    ParamKey {
        value,
        interp: ParamInterp::Bezier,
        handles: Some(h),
    }
}

fn handles(in_dt: f32, in_dv: f32, out_dt: f32, out_dv: f32, mode: HandleMode) -> Handles {
    Handles {
        in_dt,
        in_dv,
        out_dt,
        out_dv,
        mode,
    }
}

/// The pinned sample: (0, 0) out handle (+4, +8) to (12, 10) whose flat
/// in handle is 6 frames long. Pins from an independent bisection solver
/// (.work/gaban/gm14_pins.py).
fn sample_curve() -> ParamColumn {
    let mut col = ParamColumn::new(ParamId(1), "cam.x");
    col.set_key(0, bez(0.0, handles(0.0, 0.0, 4.0, 8.0, HandleMode::Broken)));
    col.set_key(12, bez(10.0, handles(6.0, 5.0, 0.0, 0.0, HandleMode::Flat)));
    col
}

#[test]
fn bezier_endpoints_are_exact() {
    let mut col = ParamColumn::new(ParamId(1), "cam.x");
    col.set_key(3, bez(1.5, handles(2.0, 9.0, 40.0, -77.0, HandleMode::Broken)));
    col.set_key(17, bez(-2.25, handles(-3.0, 1e3, 1.0, 1.0, HandleMode::Broken)));
    assert_eq!(col.resolve(3), Some(1.5));
    assert_eq!(col.resolve(17), Some(-2.25));
    assert_eq!(col.sample(3.0), Some(1.5));
    assert_eq!(col.sample(17.0), Some(-2.25));
    let seg = col.segment(3, 17);
    assert_eq!(seg.value_at(3.0), 1.5);
    assert_eq!(seg.value_at(17.0), -2.25);
}

#[test]
fn linear_equivalent_handles_match_linear_at_every_frame() {
    // Handles ON the straight line, of assorted lengths (including the
    // whole segment): the curve must stay the line, frame by frame.
    let (fa, fb, va, vb) = (2u32, 26u32, -3.0f32, 9.0f32);
    let slope = (vb - va) / (fb - fa) as f32;
    let mut linear = ParamColumn::new(ParamId(1), "lin");
    linear.set_key(fa, key(va, ParamInterp::Linear));
    linear.set_key(fb, key(vb, ParamInterp::Linear));
    for (out_dt, in_dt) in [(8.0f32, 8.0f32), (2.4, 19.2), (24.0, 24.0), (0.5, 1.0)] {
        let mut col = ParamColumn::new(ParamId(2), "bez");
        col.set_key(fa, bez(va, handles(0.0, 0.0, out_dt, slope * out_dt, HandleMode::Broken)));
        col.set_key(fb, bez(vb, handles(in_dt, slope * in_dt, 0.0, 0.0, HandleMode::Broken)));
        for f in 0..=30 {
            let (a, b) = (col.resolve(f).unwrap(), linear.resolve(f).unwrap());
            assert!((a - b).abs() < 1e-4, "frame {f} ({out_dt},{in_dt}): {a} vs {b}");
        }
    }
}

#[test]
fn auto_handles_on_two_keys_are_linear() {
    let mut auto = ParamColumn::new(ParamId(1), "auto");
    auto.set_key(0, key(0.0, ParamInterp::Bezier));
    auto.set_key(10, key(5.0, ParamInterp::Bezier));
    for f in 0..=10 {
        let v = auto.resolve(f).unwrap();
        assert!((v - f as f32 * 0.5).abs() < 1e-4, "frame {f}: {v}");
    }
    // Auto handles are Catmull-Rom: a middle key's slope runs through
    // its neighbours, each handle 1/3 of its segment long.
    auto.set_key(20, key(0.0, ParamInterp::Bezier));
    let h = auto.handles_at(10).unwrap();
    assert_eq!(h.mode, HandleMode::Auto);
    assert_eq!((h.in_dt, h.out_dt), (10.0 / 3.0, 10.0 / 3.0));
    assert_eq!((h.in_dv, h.out_dv), (0.0, 0.0));
}

#[test]
fn bezier_sample_curve_pins() {
    let col = sample_curve();
    for (frame, want) in [
        (1u32, 1.9495978595187666f64),
        (3, 5.365038626979448),
        (6, 8.571393254542407),
        (9, 9.76130797856948),
        (11, 9.979247124286438),
    ] {
        let got = col.resolve(frame).unwrap() as f64;
        assert!((got - want).abs() < 1e-5, "frame {frame}: {got} vs {want}");
        let fine = col.sample(frame as f64).unwrap();
        assert!((fine - want).abs() < 1e-9, "sample {frame}: {fine} vs {want}");
    }
    // A symmetric S between flat handles passes its midpoint exactly.
    let mut s = ParamColumn::new(ParamId(2), "s");
    s.set_key(10, bez(2.0, handles(0.0, 0.0, 5.0, 0.0, HandleMode::Smooth)));
    s.set_key(30, bez(-4.0, handles(5.0, 0.0, 0.0, 0.0, HandleMode::Smooth)));
    for (frame, want) in [(12u32, 1.761268107749964f64), (20, -1.0), (28, -3.7612681077499635)] {
        let got = s.sample(frame as f64).unwrap();
        assert!((got - want).abs() < 1e-9, "S frame {frame}: {got} vs {want}");
    }
}

#[test]
fn time_solve_is_monotonic_for_any_clamped_handles() {
    // Handle lengths far outside the segment (negative, longer than it)
    // are clamped, so frame never runs backwards along the curve and the
    // solve lands on the asked frame.
    let lengths = [-5.0, 0.0, 0.3, 2.0, 7.5, 10.0, 25.0, 1e6];
    for &a in &lengths {
        for &b in &lengths {
            let seg = BezierSeg::new((0.0, 0.0), (a, 3.0), (b, -2.0), (10.0, 1.0));
            let mut prev_s = 0.0;
            let mut prev_x = f64::NEG_INFINITY;
            for i in 0..=200 {
                let s = i as f64 / 200.0;
                let x = seg.x_at(s);
                assert!(x >= prev_x - 1e-12, "x runs backwards at s={s} (a={a}, b={b})");
                prev_x = x;
            }
            for i in 0..=100 {
                let x = i as f64 * 0.1;
                let s = seg.solve(x);
                assert!((0.0..=1.0).contains(&s));
                assert!(s >= prev_s - 1e-12, "solve not monotonic at x={x} (a={a}, b={b})");
                assert!((seg.x_at(s) - x).abs() < 1e-6, "residual at x={x} (a={a}, b={b})");
                prev_s = s;
            }
        }
    }
}

#[test]
fn ease_in_and_ease_out_pinned() {
    for (interp, pins) in [
        (ParamInterp::EaseIn, [0.0f32, 0.0625, 0.25, 1.0]),
        (ParamInterp::EaseOut, [0.0f32, 0.4375, 0.75, 1.0]),
    ] {
        let mut col = ParamColumn::new(ParamId(1), "cam.x");
        col.set_key(0, key(0.0, interp));
        col.set_key(4, key(1.0, interp));
        // t = 0, 0.25, 0.5, 1
        for (frame, want) in [0u32, 1, 2, 4].into_iter().zip(pins) {
            assert_eq!(col.resolve(frame), Some(want), "{interp:?} frame {frame}");
            assert_eq!(col.sample(frame as f64), Some(want as f64), "{interp:?} sample {frame}");
        }
    }
}

#[test]
fn ease_is_unchanged_smoothstep() {
    let mut col = ParamColumn::new(ParamId(1), "cam.x");
    col.set_key(0, key(0.0, ParamInterp::Ease));
    col.set_key(4, key(1.0, ParamInterp::Ease));
    assert_eq!(col.resolve(1), Some(0.15625));
    assert_eq!(col.resolve(2), Some(0.5));
    assert_eq!(col.resolve(3), Some(0.84375));
}

#[test]
fn interp_text_round_trips_and_old_words_hold() {
    assert_eq!(key(1.0, ParamInterp::Hold).interp_text(), "hold");
    assert_eq!(key(1.0, ParamInterp::Linear).interp_text(), "linear");
    assert_eq!(key(1.0, ParamInterp::Ease).interp_text(), "ease");
    assert_eq!(key(1.0, ParamInterp::EaseIn).interp_text(), "ease_in");
    assert_eq!(key(1.0, ParamInterp::EaseOut).interp_text(), "ease_out");
    assert_eq!(key(1.0, ParamInterp::Bezier).interp_text(), "bezier");
    let h = bez(1.0, handles(0.1, -2.5, 1e-7, 123456.79, HandleMode::Smooth));
    assert_eq!(h.interp_text(), "bezier:0.1,-2.5,0.0000001,123456.79,smooth");
    for k in [
        key(0.0, ParamInterp::Hold),
        key(0.0, ParamInterp::Linear),
        key(0.0, ParamInterp::Ease),
        key(0.0, ParamInterp::EaseIn),
        key(0.0, ParamInterp::EaseOut),
        key(0.0, ParamInterp::Bezier),
        h,
        bez(0.0, handles(3.0, 0.0, 1.0 / 3.0, -0.0, HandleMode::Flat)),
        bez(0.0, handles(0.0, 0.0, 0.0, 0.0, HandleMode::Broken)),
        bez(0.0, handles(1.0, 1.0, 1.0, 1.0, HandleMode::Auto)),
    ] {
        let text = k.interp_text();
        assert_eq!(
            ParamKey::parse_interp_text(&text),
            Some((k.interp, k.handles)),
            "{text}"
        );
    }
    for bad in ["", "Hold", "smooth", "bezier:", "bezier:1,2,3,4", "bezier:1,2,3,4,wobbly", "bezier:NaN,0,0,0,auto"] {
        assert_eq!(ParamKey::parse_interp_text(bad), None, "{bad}");
    }
}

fn engine_with_param() -> (Engine, CutRef, ParamId) {
    let mut engine = Engine::new("curve test");
    let scene = engine.add_scene("SC01");
    let cut = engine.add_cut(scene, "cut A", 48).unwrap();
    let at = CutRef { scene, cut };
    let param = engine.add_param_column(at, "cam.x").unwrap();
    (engine, at, param)
}

#[test]
fn new_interps_and_handles_survive_sqlite_roundtrip() {
    let (mut engine, at, param) = engine_with_param();
    let keys = [
        (0, key(1.0, ParamInterp::EaseIn)),
        (4, key(2.0, ParamInterp::EaseOut)),
        (8, key(3.0, ParamInterp::Bezier)),
        (12, bez(-4.5, handles(0.1, 0.2, 2.0 / 3.0, -7.25, HandleMode::Broken))),
        (20, bez(6.0, handles(1.5, 0.0, 1.5, 0.0, HandleMode::Flat))),
        (30, key(0.0, ParamInterp::Ease)),
    ];
    let commands = keys
        .iter()
        .map(|&(frame, k)| Command::SetParamKey {
            at,
            column: param,
            frame,
            key: Some(k),
        })
        .collect();
    engine.apply("keys", commands).unwrap();

    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("curve_roundtrip.animproj");
    engine.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(loaded.project, engine.project);
}

#[test]
fn set_param_keys_is_one_exact_undo_step() {
    let (mut engine, at, param) = engine_with_param();
    engine
        .apply(
            "seed",
            vec![
                Command::SetParamKey { at, column: param, frame: 0, key: Some(key(0.0, ParamInterp::Linear)) },
                Command::SetParamKey { at, column: param, frame: 10, key: Some(key(5.0, ParamInterp::Bezier)) },
            ],
        )
        .unwrap();
    let before = engine.project.clone();
    let edits = vec![
        ParamKeyEdit { column: param, frame: 10, before: Some(key(5.0, ParamInterp::Bezier)), after: None },
        ParamKeyEdit {
            column: param,
            frame: 12,
            before: None,
            after: Some(bez(7.0, handles(1.0, 2.0, 3.0, 4.0, HandleMode::Smooth))),
        },
        ParamKeyEdit { column: param, frame: 0, before: Some(key(0.0, ParamInterp::Linear)), after: Some(key(1.0, ParamInterp::EaseOut)) },
    ];
    engine.apply("drag", vec![Command::SetParamKeys { at, keys: edits }]).unwrap();
    let after = engine.project.clone();
    let cut = engine.project.scenes[0].cuts[0].xsheet.param(param).unwrap().clone();
    assert_eq!(cut.keys().count(), 2);
    assert_eq!(cut.key_at(12).unwrap().value, 7.0);
    engine.undo().unwrap();
    assert_eq!(engine.project, before);
    engine.redo().unwrap();
    assert_eq!(engine.project, after);
}

#[test]
fn set_param_keys_refuses_stale_or_bad_batches_untouched() {
    let (mut engine, at, param) = engine_with_param();
    engine
        .apply("seed", vec![Command::SetParamKey { at, column: param, frame: 4, key: Some(key(2.0, ParamInterp::Linear)) }])
        .unwrap();
    let before = engine.project.clone();
    let good = ParamKeyEdit { column: param, frame: 9, before: None, after: Some(key(1.0, ParamInterp::Linear)) };
    let batches = [
        // stale: frame 4 holds a key, the edit thinks it is empty
        vec![good, ParamKeyEdit { column: param, frame: 4, before: None, after: Some(key(3.0, ParamInterp::Hold)) }],
        // non-finite handles
        vec![good, ParamKeyEdit {
            column: param,
            frame: 20,
            before: None,
            after: Some(bez(1.0, handles(f32::NAN, 0.0, 1.0, 0.0, HandleMode::Broken))),
        }],
        // handles on a non-Bezier key would not round-trip through the store
        vec![good, ParamKeyEdit {
            column: param,
            frame: 20,
            before: None,
            after: Some(ParamKey { value: 1.0, interp: ParamInterp::Linear, handles: Some(handles(1.0, 0.0, 1.0, 0.0, HandleMode::Broken)) }),
        }],
        // unknown column
        vec![good, ParamKeyEdit { column: ParamId(9999), frame: 1, before: None, after: Some(key(1.0, ParamInterp::Hold)) }],
    ];
    for batch in batches {
        assert!(engine.apply("bad", vec![Command::SetParamKeys { at, keys: batch }]).is_err());
        assert_eq!(engine.project, before, "a refused batch leaves the keys untouched");
    }
}

#[test]
fn moving_keys_onto_a_keyed_frame_replaces_never_duplicates() {
    let (mut engine, at, param) = engine_with_param();
    let seed: Vec<Command> = [(0u32, 0.0f32), (4, 1.0), (8, 2.0), (12, 3.0)]
        .into_iter()
        .map(|(frame, v)| Command::SetParamKey { at, column: param, frame, key: Some(key(v, ParamInterp::Linear)) })
        .collect();
    engine.apply("seed", seed).unwrap();
    let before = engine.project.clone();
    // Drag the keys at 4 and 8 four frames later: 8 lands on 12 (replacing
    // it) and 4 lands on 8 (the frame the drag itself vacated).
    let cmds = sheet_ops::move_param_keys(&engine.project, at, &[(param, 4), (param, 8)], 4, 0.5).unwrap();
    assert_eq!(cmds.len(), 1, "one command, one undo step");
    engine.apply("drag keys", cmds).unwrap();
    let col = engine.project.scenes[0].cuts[0].xsheet.param(param).unwrap();
    let got: Vec<(u32, f32)> = col.keys().map(|(f, k)| (f, k.value)).collect();
    assert_eq!(got, vec![(0, 0.0), (8, 1.5), (12, 2.5)]);
    engine.undo().unwrap();
    assert_eq!(engine.project, before);
    // A drag past frame 0 stops the whole selection at 0.
    let cmds = sheet_ops::move_param_keys(&engine.project, at, &[(param, 4), (param, 8)], -9, 0.0).unwrap();
    engine.apply("drag left", cmds).unwrap();
    let col = engine.project.scenes[0].cuts[0].xsheet.param(param).unwrap();
    let got: Vec<(u32, f32)> = col.keys().map(|(f, k)| (f, k.value)).collect();
    assert_eq!(got, vec![(0, 1.0), (4, 2.0), (12, 3.0)]);
    // No move, no command.
    assert!(sheet_ops::move_param_keys(&engine.project, at, &[(param, 0)], 0, 0.0).unwrap().is_empty());
}

/// The session mirror sends commands as JSON: a SetParamKeys batch — new
/// interps and stored handles included — crosses the wire unchanged, and a
/// key written before handles existed (no "handles" field) still reads.
#[test]
fn set_param_keys_crosses_the_json_wire_with_its_handles() {
    let (_, at, param) = engine_with_param();
    let cmd = Command::SetParamKeys {
        at,
        keys: vec![
            ParamKeyEdit {
                column: param,
                frame: 3,
                before: None,
                after: Some(bez(2.5, handles(1.0, -0.5, 4.0, 8.25, HandleMode::Broken))),
            },
            ParamKeyEdit {
                column: param,
                frame: 9,
                before: Some(key(1.0, ParamInterp::EaseIn)),
                after: Some(key(1.5, ParamInterp::EaseOut)),
            },
            ParamKeyEdit { column: param, frame: 12, before: Some(key(0.0, ParamInterp::Hold)), after: None },
        ],
    };
    let text = serde_json::to_string(&cmd).unwrap();
    assert!(text.contains("SetParamKeys") && text.contains("Broken"), "{text}");
    let back: Command = serde_json::from_str(&text).unwrap();
    assert_eq!(back, cmd);

    let new = Command::SetParamKey { at, column: param, frame: 4, key: Some(key(1.0, ParamInterp::Linear)) };
    let wire = serde_json::to_string(&new).unwrap();
    let old = wire.replace(",\"handles\":null", "");
    assert_ne!(old, wire, "the key carries a handles field: {wire}");
    let read: Command = serde_json::from_str(&old).unwrap();
    assert_eq!(read, new);
}

/// The picture follows the handles: a bound Transform's eval hash (what the
/// compositor caches frames on) moves between the keys when a handle bends,
/// stays put on the keys themselves, and undo brings the old hash back.
#[test]
fn bending_a_handle_changes_the_eval_hash_between_the_keys() {
    let (mut engine, at, param) = engine_with_param();
    let (n_xform, n_out) = (engine.alloc_node_id(), engine.alloc_node_id());
    let binds = TransformBinds { tx: Some(param), ..Default::default() };
    engine
        .apply(
            "build graph",
            vec![
                Command::AddNode {
                    at,
                    id: n_xform,
                    kind: NodeKind::Transform { translate: (1.0, 2.0), scale: 1.0, rotate_deg: 0.0, binds },
                },
                Command::AddNode { at, id: n_out, kind: NodeKind::Output },
                Command::Connect { at, from: n_xform, from_pin: 0, to: n_out, to_pin: 0 },
                Command::SetOutput { at, node: Some(n_out) },
            ],
        )
        .unwrap();
    let flat = bez(0.0, handles(0.0, 0.0, 4.0, 0.0, HandleMode::Flat));
    engine
        .apply(
            "keys",
            vec![
                Command::SetParamKey { at, column: param, frame: 0, key: Some(flat) },
                Command::SetParamKey { at, column: param, frame: 12, key: Some(key(10.0, ParamInterp::Linear)) },
            ],
        )
        .unwrap();
    let hash = |e: &mut Engine, frame| e.eval(at.scene, at.cut, frame).unwrap().hash();
    let (k0, mid, k12) = (hash(&mut engine, 0), hash(&mut engine, 6), hash(&mut engine, 12));

    let bent = bez(0.0, handles(0.0, 0.0, 4.0, 9.0, HandleMode::Broken));
    let edit = ParamKeyEdit { column: param, frame: 0, before: Some(flat), after: Some(bent) };
    engine.apply("bend handle", vec![Command::SetParamKeys { at, keys: vec![edit] }]).unwrap();
    assert_ne!(hash(&mut engine, 6), mid, "the curve between the keys moved");
    assert_eq!(hash(&mut engine, 0), k0, "a key's own frame keeps its value");
    assert_eq!(hash(&mut engine, 12), k12);
    engine.undo().unwrap();
    assert_eq!(hash(&mut engine, 6), mid, "undo restores the old picture");
}
