//! GM15 editorial: the cut order (MoveCut) and its save, the running time
//! and its target, production status and assignee in cut meta, cuts made
//! from storyboard panels, the pinned CMX3600 EDL, the Episode render and
//! its mixdown, the colour designation sheet, retake notes in the frame
//! ripple, "Export cut as project" and the episode frame map. Every file
//! lands in a temp dir; no process is spawned.

use std::sync::Arc;

use anim_core::Engine;
use anim_core::board::{self, CutStatus, ProdInfo, RetakeNote, SheetChip, SheetPalette, SheetVariant};
use anim_core::color::ColorLaw;
use anim_core::command::{Command, CutRef};
use anim_core::deliver::{self, Episode, Purpose, RenderOpts};
use anim_core::export::straight_rgba8;
use anim_core::fx::Image;
use anim_core::ids::{ColumnId, DrawingId, SceneId};
use anim_core::model::{AudioClip, CelLayer, DrawingDef, LayerRole};
use anim_core::raster::{TILE, TILE_LEN, TileData, f32_to_f16_bits};
use anim_core::xsheet::{ColumnKind, Exposure};

const W: u32 = 64;
const H: u32 = 36;

/// One tile with a `w × h` block of `rgba` at (x0, y0).
fn block(x0: usize, y0: usize, w: usize, h: usize, rgba: [f32; 4]) -> Arc<TileData> {
    let mut px = vec![0u16; TILE_LEN];
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            for c in 0..4 {
                px[(y * TILE + x) * 4 + c] = f32_to_f16_bits(rgba[c]);
            }
        }
    }
    Arc::new(TileData::from_vec(px))
}

/// Add a drawing of one colour layer inked with a block at `x0`, exposed
/// on `column` at `frame`.
fn ink(e: &mut Engine, at: CutRef, column: ColumnId, frame: u32, x0: usize, rgba: [f32; 4]) -> DrawingId {
    let (d, l) = (e.alloc_drawing_id(), e.alloc_layer_id());
    let index = e.project.cut(at.scene, at.cut).unwrap().drawings.len();
    e.apply(
        "ink",
        vec![
            Command::AddDrawing {
                at,
                id: d,
                index,
                name: format!("d{frame}"),
                strokes: vec![],
                layers: vec![CelLayer::with_role(l, "color", LayerRole::Color)],
            },
            Command::PaintTiles {
                at,
                id: d,
                layer: l,
                diff: vec![((0, 0), None, Some(block(x0, 4, 6, 8, rgba)))],
            },
            Command::SetCell {
                at,
                column,
                frame,
                key: Some(Exposure::Drawing(d)),
            },
        ],
    )
    .unwrap();
    d
}

/// Scene "s01" of project "gaban" with cuts c001, c002 and c003 of 24, 36
/// and 48 frames; each shows its own block on column A, a second drawing
/// half way through.
fn episode() -> (Engine, SceneId, Vec<CutRef>) {
    let mut e = Engine::new("gaban");
    let scene = e.add_scene("s01");
    let mut cuts = Vec::new();
    for (i, frames) in [24u32, 36, 48].into_iter().enumerate() {
        let cut = e.add_cut(scene, format!("c{:03}", i + 1), frames).unwrap();
        let at = CutRef { scene, cut };
        let col = e.add_column(at, "A").unwrap();
        let tint = [0.2 * i as f32, 0.5, 1.0 - 0.3 * i as f32, 1.0];
        ink(&mut e, at, col, 0, 8 * i, tint);
        ink(&mut e, at, col, frames / 2, 8 * i + 20, [1.0, 0.0, 0.0, 1.0]);
        cuts.push(at);
    }
    e.clear_history();
    (e, scene, cuts)
}

fn fnv(img: &Image) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in &img.px {
        for b in v.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// A 16-bit mono WAV: one second of a 440 Hz tone at `rate`.
fn tone(rate: u32) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
        for i in 0..rate {
            let v = (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.25;
            w.write_sample((v * 32767.0).round() as i16).unwrap();
        }
        w.finalize().unwrap();
    }
    out.into_inner()
}

fn names(e: &Engine, scene: SceneId) -> Vec<String> {
    e.project
        .scene(scene)
        .unwrap()
        .cuts
        .iter()
        .map(|c| c.name.clone())
        .collect()
}

#[test]
fn move_cut_has_an_exact_inverse_and_the_order_persists() {
    let (mut e, scene, _) = episode();
    let before = e.project.scene(scene).unwrap().clone();
    e.apply("reorder", vec![Command::MoveCut { scene, from: 0, to: 2 }])
        .unwrap();
    assert_eq!(names(&e, scene), ["c002", "c003", "c001"]);
    e.undo().unwrap();
    assert_eq!(e.project.scene(scene).unwrap(), &before, "undo restores the order exactly");
    e.redo().unwrap();
    assert_eq!(names(&e, scene), ["c002", "c003", "c001"]);
    e.apply("reorder", vec![Command::MoveCut { scene, from: 2, to: 1 }])
        .unwrap();
    assert_eq!(names(&e, scene), ["c002", "c001", "c003"]);
    // Out of range is refused and changes nothing.
    assert!(e.apply("bad", vec![Command::MoveCut { scene, from: 3, to: 0 }]).is_err());
    assert_eq!(names(&e, scene), ["c002", "c001", "c003"]);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("order.animproj");
    e.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    assert_eq!(names(&loaded, scene), ["c002", "c001", "c003"], "the order persists");
}

#[test]
fn format_trt_and_the_target_difference_are_pinned() {
    assert_eq!(board::format_trt(30_972, 24), "21:30+12");
    assert_eq!(board::format_trt(0, 24), "00:00+00");
    assert_eq!(board::format_trt(108, 24), "00:04+12");
    assert_eq!(board::format_trt(30_972, 30), "17:12+12");
    // 28 frames under, over, and on a 31,000-frame target.
    assert_eq!(board::format_trt_diff(30_972, 31_000, 24), "-00:01+04");
    assert_eq!(board::format_trt_diff(31_028, 31_000, 24), "+00:01+04");
    assert_eq!(board::format_trt_diff(31_000, 31_000, 24), "±00:00+00");
    assert_eq!(board::trt_mark(30_972, 31_000), board::TrtMark::Under);
    assert_eq!(board::trt_mark(31_028, 31_000), board::TrtMark::Over);
    assert_eq!(board::trt_mark(31_000, 31_000), board::TrtMark::OnTarget);
    assert_eq!(board::parse_trt("21:30+12", 24), Some(30_972));
    assert_eq!(board::parse_trt("21:30", 24), Some(30_960));
    assert_eq!(board::parse_trt("108", 24), Some(108));
    assert_eq!(board::parse_trt("21:60+00", 24), None);
    assert_eq!(board::parse_trt("21:30+24", 24), None);

    let (mut e, scene, _) = episode();
    assert_eq!(board::trt_frames(e.project.scene(scene).unwrap()), 108);
    assert_eq!(board::episode_target(&e.project), None);
    e.project
        .app_meta
        .insert(board::TARGET_KEY.into(), "120".into());
    assert_eq!(board::episode_target(&e.project), Some(120));
    assert_eq!(board::format_trt_diff(108, 120, 24), "-00:00+12");
}

#[test]
fn status_and_assignee_round_trip_through_cut_meta_as_one_undo_step() {
    let (mut e, _, cuts) = episode();
    let at = cuts[1];
    let cut = |e: &Engine| e.project.cut(at.scene, at.cut).unwrap().clone();
    assert_eq!(ProdInfo::of(&cut(&e)), ProdInfo::default());
    let meta_before = cut(&e).meta.clone();

    let cmds = board::set_status(&cut(&e), at, Some(CutStatus::Genga));
    assert_eq!(cmds.len(), 1);
    e.apply("status", cmds).unwrap();
    assert_eq!(cut(&e).meta.get(board::STATUS_KEY).map(String::as_str), Some("\"genga\""));
    assert_eq!(ProdInfo::of(&cut(&e)).status, Some(CutStatus::Genga));
    e.undo().unwrap();
    assert_eq!(cut(&e).meta, meta_before, "one undo step takes the status back");
    e.redo().unwrap();

    e.apply("assign", board::set_assignee(&cut(&e), at, "  Mori  ")).unwrap();
    assert_eq!(ProdInfo::of(&cut(&e)).assignee, "Mori");
    // Setting what is already there is no command at all.
    assert!(board::set_assignee(&cut(&e), at, "Mori").is_empty());
    // A key this build cannot read stays as it is.
    e.apply(
        "future",
        vec![Command::SetCutMeta {
            at,
            key: "prod.future".into(),
            before: None,
            after: Some("{\"x\":1}".into()),
        }],
    )
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prod.animproj");
    e.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    let back = loaded.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(
        ProdInfo::of(back),
        ProdInfo {
            status: Some(CutStatus::Genga),
            assignee: "Mori".into(),
            notes: vec![],
        }
    );
    assert_eq!(back.meta.get("prod.future").map(String::as_str), Some("{\"x\":1}"));
    for s in CutStatus::ALL {
        assert_eq!(CutStatus::from_key(s.key()), Some(s));
    }
    // Clearing removes the key.
    e.apply("clear", board::set_status(&cut(&e), at, None)).unwrap();
    assert!(!cut(&e).meta.contains_key(board::STATUS_KEY));
}

#[test]
fn the_status_filter_returns_exactly_the_matching_cuts() {
    let (mut e, scene, cuts) = episode();
    let set = |e: &mut Engine, i: usize, s: CutStatus, who: &str| {
        let at = cuts[i];
        let c = e.project.cut(at.scene, at.cut).unwrap().clone();
        let mut cmds = board::set_status(&c, at, Some(s));
        cmds.extend(board::set_assignee(&c, at, who));
        e.apply("prod", cmds).unwrap();
    };
    set(&mut e, 0, CutStatus::Paint, "Aoi");
    set(&mut e, 2, CutStatus::Paint, "Mori");
    let s = e.project.scene(scene).unwrap();
    assert_eq!(board::filter_cuts(s, Some(CutStatus::Paint), None), [0, 2]);
    assert_eq!(board::filter_cuts(s, Some(CutStatus::Paint), Some("Mori")), [2]);
    assert_eq!(board::filter_cuts(s, Some(CutStatus::Done), None), Vec::<usize>::new());
    assert_eq!(board::filter_cuts(s, None, Some("")), [1], "nobody has c002 yet");
    assert_eq!(board::filter_cuts(s, None, None), [0, 1, 2]);
}

#[test]
fn cuts_from_panels_make_two_cuts_of_60_and_48_frames_as_one_undo_step() {
    let mut e = Engine::new("animatic");
    let scene = e.add_scene("s01");
    let cut = e.add_cut(scene, "animatic", 108).unwrap();
    let at = CutRef { scene, cut };
    let board_col = e.add_column(at, "BOARD").unwrap();
    let panels = [
        ink(&mut e, at, board_col, 0, 4, [1.0, 0.0, 0.0, 1.0]),
        ink(&mut e, at, board_col, 36, 12, [0.0, 1.0, 0.0, 1.0]),
        ink(&mut e, at, board_col, 60, 20, [0.0, 0.0, 1.0, 1.0]),
    ];
    // Panels 1 and 2 are cut 1, panel 3 is cut 2 (DrawingDef JSON).
    let defs = panels
        .iter()
        .zip([1, 1, 2])
        .map(|(d, n)| Command::SetDrawingDef {
            at,
            drawing: *d,
            before: DrawingDef::default(),
            after: DrawingDef {
                cut: Some(n),
                ..DrawingDef::default()
            },
        })
        .collect();
    e.apply("numbers", defs).unwrap();
    let source = e.project.cut(scene, cut).unwrap().clone();
    let def = source.drawing_def(panels[2]);
    assert_eq!(DrawingDef::from_json(&def.to_json()), Some(def.clone()));
    assert_eq!(
        board::panels(&source, board_col)
            .iter()
            .map(|p| (p.start, p.frames, p.cut))
            .collect::<Vec<_>>(),
        [(0, 36, 1), (36, 24, 1), (60, 48, 2)]
    );
    e.clear_history();
    let before = e.project.scene(scene).unwrap().clone();

    let cmds = board::cuts_from_panels(&mut e.project, at, board_col).unwrap();
    e.apply("make cuts from panels", cmds).unwrap();
    let s = e.project.scene(scene).unwrap();
    assert_eq!(
        s.cuts.iter().map(|c| (c.name.as_str(), c.frame_count)).collect::<Vec<_>>(),
        [("animatic", 108), ("C001", 60), ("C002", 48)]
    );
    // The pictures, in order, on a Ref column: copies with fresh ids.
    let exposed = |c: &anim_core::model::Cut| {
        let col = &c.xsheet.columns[0];
        assert_eq!(col.name, "BOARD");
        assert_eq!(c.column_def(col.id).kind, ColumnKind::Ref);
        col.keys()
            .map(|(f, x)| match x {
                Exposure::Drawing(d) => {
                    let d = c.drawing(d).unwrap();
                    (f, d.layers[0].raster.content_hash())
                }
                Exposure::Empty => panic!("no empty keys"),
            })
            .collect::<Vec<_>>()
    };
    let art = |d: DrawingId| source.drawing(d).unwrap().layers[0].raster.content_hash();
    assert_eq!(exposed(&s.cuts[1]), [(0, art(panels[0])), (36, art(panels[1]))]);
    assert_eq!(exposed(&s.cuts[2]), [(0, art(panels[2]))]);
    assert!(s.cuts[1].drawings.iter().all(|d| !panels.contains(&d.id)));
    e.undo().unwrap();
    assert_eq!(e.project.scene(scene).unwrap(), &before, "one undo restores the scene exactly");
}

#[test]
fn the_edl_of_three_cuts_is_pinned() {
    let (mut e, scene, cuts) = episode();
    let edl = board::edl_cmx3600("gaban", e.project.scene(scene).unwrap(), 24, deliver::DEFAULT_PATTERN, "mov");
    assert_eq!(
        edl,
        "TITLE: gaban s01\n\
         FCM: NON-DROP FRAME\n\
         \n\
         001  AX       V     C        00:00:00:00 00:00:01:00 01:00:00:00 01:00:01:00\n\
         * FROM CLIP NAME: gaban_s01_c001.mov\n\
         \n\
         002  AX       V     C        00:00:00:00 00:00:01:12 01:00:01:00 01:00:02:12\n\
         * FROM CLIP NAME: gaban_s01_c002.mov\n\
         \n\
         003  AX       V     C        00:00:00:00 00:00:02:00 01:00:02:12 01:00:04:12\n\
         * FROM CLIP NAME: gaban_s01_c003.mov\n"
    );
    // A cut with sound gets an audio event naming its mixdown.
    let clip = AudioClip::from_bytes("tone.wav", tone(48_000)).unwrap();
    e.apply("sound", vec![Command::SetCutAudio { at: cuts[1], audio: Some(clip) }])
        .unwrap();
    let edl = board::edl_cmx3600("gaban", e.project.scene(scene).unwrap(), 24, deliver::DEFAULT_PATTERN, "mov");
    assert!(edl.contains(
        "\n003  AX       AA    C        00:00:00:00 00:00:01:12 01:00:01:00 01:00:02:12\n\
         * FROM CLIP NAME: gaban_s01_c002.wav\n"
    ));
    assert!(edl.contains("\n004  AX       V     C        00:00:00:00 00:00:02:00 01:00:02:12 01:00:04:12\n"));
}

#[test]
fn an_episode_renders_every_cut_frame_for_frame_and_lays_the_mixdowns_end_to_end() {
    let (mut e, scene, cuts) = episode();
    let clip = AudioClip::from_bytes("tone.wav", tone(48_000)).unwrap();
    e.apply("sound", vec![Command::SetCutAudio { at: cuts[1], audio: Some(clip) }])
        .unwrap();
    let ep = Episode::of(&e.project, scene, Purpose::View).unwrap();
    assert_eq!(ep.frame_count(), 108);
    assert_eq!(ep.starts, [0, 24, 60]);
    let s = e.project.scene(scene).unwrap();
    let law = e.project.color_law;
    let opts = RenderOpts::default();
    let map = board::episode_frame_map(s);
    assert_eq!(map.len(), 108);
    let mut hashes = Vec::new();
    for (f, (i, local)) in map.iter().enumerate() {
        let ours = ep.render(f as u32, law, W, H, &opts).unwrap();
        let theirs = deliver::render(&s.cuts[*i], law, *local, W, H, &opts);
        assert_eq!(fnv(&ours), fnv(&theirs), "episode frame {f}");
        hashes.push(fnv(&ours));
    }
    assert!(ep.render(108, law, W, H, &opts).is_none());
    assert_ne!(hashes[0], hashes[24], "the cuts differ");

    let mix = ep.mixdown(24).unwrap();
    assert_eq!(mix.len() / 2, 216_000, "4.5 s at 48 kHz");
    assert!(mix[..48_000 * 2].iter().all(|v| *v == 0.0), "c001 is silent");
    let c002 = deliver::mixdown(&s.cuts[1], 24, 0, 35).unwrap();
    assert_eq!(&mix[48_000 * 2..120_000 * 2], &c002[..], "c002's mix in its slot");
    assert!(c002.iter().any(|v| *v != 0.0));

    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("ep.wav");
    assert!(ep.write_mixdown(&wav, 24).unwrap());
    let r = hound::WavReader::open(&wav).unwrap();
    assert_eq!((r.spec().sample_rate, r.spec().channels, r.duration()), (48_000, 2, 216_000));
}

/// A six-chip palette with one variant (evening).
fn palette() -> SheetPalette {
    let chips = [
        ("hada", "base", [250, 220, 200, 255]),
        ("hada kage", "kage (shadow)", [220, 180, 165, 255]),
        ("kami", "base", [40, 30, 60, 255]),
        ("kami high", "highlight", [90, 80, 140, 255]),
        ("fuku", "base", [255, 255, 255, 255]),
        ("line", "line", [0, 0, 0, 255]),
    ];
    SheetPalette {
        name: "Hana".into(),
        chips: chips
            .iter()
            .map(|(n, r, c)| SheetChip {
                name: n.to_string(),
                role: r.to_string(),
                color: *c,
            })
            .collect(),
        variants: vec![SheetVariant {
            name: "evening".into(),
            colors: chips.iter().map(|(_, _, c)| [c[0] / 2, c[1] / 2, c[2], 255]).collect(),
        }],
    }
}

#[test]
fn the_colour_sheet_has_pinned_rects_exact_swatches_and_text_in_every_label() {
    let p = palette();
    let l = board::color_sheet_layout(&p);
    assert_eq!((l.width, l.height), (604, 332));
    let r = |x, y, w, h| board::Rect { x, y, w, h };
    assert_eq!(l.rows.len(), 6);
    assert_eq!(l.rows[0].swatches, [r(316, 80, 32, 32), r(452, 80, 32, 32)]);
    assert_eq!(l.rows[5].swatches, [r(316, 280, 32, 32), r(452, 280, 32, 32)]);
    assert_eq!(l.rows[0].name, r(16, 84, 152, 24));
    assert_eq!(l.rows[0].role, r(176, 84, 132, 24));
    assert_eq!(l.rows[0].hex, [r(356, 84, 88, 24), r(492, 84, 88, 24)]);
    assert_eq!(board::hex([250, 220, 200, 255]), "#FADCC8");
    assert_eq!(board::hex([1, 2, 3, 4]), "#01020304");

    let img = board::render_color_sheet(&p);
    assert_eq!((img.w, img.h), (604, 332));
    for (i, row) in l.rows.iter().enumerate() {
        for (c, sw) in row.swatches.iter().enumerate() {
            let want = p.color(i, c);
            let (x, y) = sw.center();
            let px = img.pixel(x, y);
            assert_eq!(px, ColorLaw::Srgb.encode_rgba(want), "chip {i} column {c}");
            assert_eq!(straight_rgba8(&px), want, "chip {i} column {c} as written");
        }
    }
    let inked = |rect: board::Rect| {
        (rect.y..rect.y + rect.h).any(|y| {
            (rect.x..rect.x + rect.w).any(|x| {
                let p = img.pixel(x, y);
                p[0] < 0.5 && p[1] < 0.5 && p[2] < 0.5
            })
        })
    };
    assert!(inked(l.title), "title");
    for (k, h) in l.heads.iter().enumerate() {
        assert!(inked(*h), "head {k}");
    }
    for (i, row) in l.rows.iter().enumerate() {
        assert!(inked(row.name), "name {i}");
        assert!(inked(row.role), "role {i}");
        for (c, h) in row.hex.iter().enumerate() {
            assert!(inked(*h), "hex {i} {c}");
        }
    }
    // The paper between labels stays white.
    assert_eq!(img.pixel(2, 2), [1.0; 4]);
}

#[test]
fn retake_notes_ride_insert_and_delete_frames_and_undo_puts_them_back() {
    let (mut e, _, cuts) = episode();
    let at = cuts[2];
    let cut = |e: &Engine| e.project.cut(at.scene, at.cut).unwrap().clone();
    for (frame, retake) in [(30, false), (5, false), (20, true)] {
        let note = RetakeNote {
            frame,
            text: format!("fix {frame}"),
            author: "kantoku".into(),
            resolved: false,
        };
        let cmds = board::add_note(&cut(&e), at, note, retake);
        e.apply("note", cmds).unwrap();
    }
    let p = ProdInfo::of(&cut(&e));
    assert_eq!(p.notes.iter().map(|n| n.frame).collect::<Vec<_>>(), [5, 20, 30]);
    assert_eq!(p.status, Some(CutStatus::Retake), "the third note set Retake");
    assert_eq!(p.open_notes(), 3);
    let frames = |e: &Engine| board::notes_of(&cut(e)).iter().map(|n| n.frame).collect::<Vec<_>>();

    let text = cut(&e).meta.get(board::NOTES_KEY).cloned();
    e.apply("insert", vec![Command::InsertFrames { at, frame: 10, count: 4 }])
        .unwrap();
    assert_eq!(frames(&e), [5, 24, 34]);
    e.undo().unwrap();
    assert_eq!(cut(&e).meta.get(board::NOTES_KEY), text.as_ref());

    e.apply("delete", vec![Command::DeleteFrames { at, frame: 18, count: 4 }])
        .unwrap();
    assert_eq!(frames(&e), [5, 18, 26], "a note inside the range stays at its start");
    e.undo().unwrap();
    assert_eq!(cut(&e).meta.get(board::NOTES_KEY), text.as_ref());
    e.redo().unwrap();
    assert_eq!(frames(&e), [5, 18, 26]);
    e.undo().unwrap();

    // Text in another hand (spacing, a field a newer build added) moves
    // and keeps its field; undo gives back the very bytes.
    let odd = "[ {\"frame\": 20, \"text\": \"x\", \"pen\": \"red\"} ]".to_string();
    e.apply(
        "odd",
        vec![Command::SetCutMeta {
            at,
            key: board::NOTES_KEY.into(),
            before: text.clone(),
            after: Some(odd.clone()),
        }],
    )
    .unwrap();
    e.apply("insert", vec![Command::InsertFrames { at, frame: 0, count: 2 }])
        .unwrap();
    let moved = cut(&e).meta.get(board::NOTES_KEY).cloned().unwrap();
    assert!(moved.contains("\"frame\":22") && moved.contains("\"pen\":\"red\""), "{moved}");
    e.undo().unwrap();
    assert_eq!(cut(&e).meta.get(board::NOTES_KEY), Some(&odd));
}

#[test]
fn export_cut_writes_a_project_with_just_that_cut() {
    let (mut e, scene, cuts) = episode();
    e.project
        .app_meta
        .insert("palettes".into(), "{\"characters\":[]}".into());
    let before = e.project.clone();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c002.animproj");
    anim_core::store::export_cut(&e.project, scene, cuts[1].cut, &path).unwrap();
    assert_eq!(e.project, before, "the source is untouched");

    let loaded = Engine::load(&path).unwrap();
    assert_eq!(loaded.project.scenes.len(), 1);
    assert_eq!(loaded.project.scenes[0].cuts.len(), 1);
    assert_eq!(
        (loaded.project.fps, loaded.project.width, loaded.project.height, loaded.project.color_law),
        (e.project.fps, e.project.width, e.project.height, e.project.color_law)
    );
    assert_eq!(loaded.project.app_meta.get("palettes"), e.project.app_meta.get("palettes"));
    let src = e.project.cut(scene, cuts[1].cut).unwrap().clone();
    let out = loaded.project.cut(scene, cuts[1].cut).unwrap().clone();
    let hashes = |c: &anim_core::model::Cut| c.drawings.iter().map(|d| d.content_hash()).collect::<Vec<_>>();
    assert_eq!(hashes(&out), hashes(&src), "drawings");
    assert_eq!(out.xsheet, src.xsheet, "sheet");
    assert_eq!(out.graph, src.graph, "graph");
    let (law, opts) = (e.project.color_law, RenderOpts::default());
    for f in [0, 17, 18, 35] {
        let a = fnv(&deliver::render(&src, law, f, W, H, &opts));
        let b = fnv(&deliver::render(&out, law, f, W, H, &opts));
        assert_eq!(a, b, "frame {f}");
    }
    assert!(anim_core::store::export_cut(&e.project, scene, anim_core::ids::CutId(9999), &path).is_err());
}

#[test]
fn episode_frame_map_is_right_across_cut_boundaries() {
    let (e, scene, _) = episode();
    let s = e.project.scene(scene).unwrap();
    let map = board::episode_frame_map(s);
    assert_eq!(map.len(), 108);
    assert_eq!(map[0], (0, 0));
    assert_eq!(map[23], (0, 23));
    assert_eq!(map[24], (1, 0));
    assert_eq!(map[59], (1, 35));
    assert_eq!(map[60], (2, 0));
    assert_eq!(map[107], (2, 47));
    assert_eq!(board::cut_starts(s), [0, 24, 60]);
    let ep = Episode::of(&e.project, scene, Purpose::View).unwrap();
    for (f, at) in map.iter().enumerate() {
        assert_eq!(board::locate(s, f as u64), Some(*at), "frame {f}");
        assert_eq!(ep.locate(f as u32), Some(*at), "frame {f}");
    }
    assert_eq!(board::locate(s, 108), None);
    assert_eq!(ep.locate(108), None);
}

#[test]
fn a_panel_cut_number_lives_in_the_drawing_def_json_and_defaults_to_order() {
    let def = DrawingDef {
        cut: Some(7),
        ..DrawingDef::default()
    };
    let json = def.to_json();
    assert!(json.contains("\"cut\":7"), "{json}");
    assert_eq!(DrawingDef::from_json(&json), Some(def));
    // Without a number the row is what it always was.
    assert!(!DrawingDef::default().to_json().contains("cut"));
    assert_eq!(DrawingDef::from_json("{\"kind\":\"key\",\"chart\":null}").unwrap().cut, None);

    let (e, _, cuts) = episode();
    let c = e.project.cut(cuts[2].scene, cuts[2].cut).unwrap();
    let col = c.xsheet.columns[0].id;
    let p = board::panels(c, col);
    assert_eq!(
        p.iter().map(|p| (p.start, p.frames, p.cut)).collect::<Vec<_>>(),
        [(0, 24, 1), (24, 24, 2)]
    );
}
