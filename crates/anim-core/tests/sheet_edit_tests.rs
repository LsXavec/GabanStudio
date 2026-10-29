//! GM5: the X-sheet engine — schema v11 side tables, column kinds, the memo
//! row, and every sheet edit as an exact-inverse Command (or a pure
//! sheet_ops builder over them).

use std::sync::Arc;

use anim_core::Engine;
use anim_core::command::{CellEdit, Command, CutRef};
use anim_core::export::{render_frame, render_graph_frame};
use anim_core::graph::NodeKind;
use anim_core::ids::*;
use anim_core::model::{CelLayer, Cut};
use anim_core::raster::{TILE_LEN, TileData};
use anim_core::sheet_ops::{self, PasteMode};
use anim_core::xsheet::{
    Column, ColumnDef, ColumnKind, Exposure, ParamColumn, ParamInterp, ParamKey,
};

const ONE: u16 = 0x3C00; // f16 1.0
const RED: [u16; 4] = [ONE, 0, 0, ONE];
const GREEN: [u16; 4] = [0, ONE, 0, ONE];
const BLUE: [u16; 4] = [0, 0, ONE, ONE];

fn tile(rgba: [u16; 4]) -> Arc<TileData> {
    Arc::new(TileData::from_vec(
        rgba.iter().copied().cycle().take(TILE_LEN).collect(),
    ))
}

/// One scene, one cut of `frames`, `ncols` columns named A, B, ... and
/// `ndrawings` drawings named "1", "2", ... (one cel layer each), history
/// cleared.
struct Fx {
    engine: Engine,
    at: CutRef,
    cols: Vec<ColumnId>,
    d: Vec<DrawingId>,
    layers: Vec<LayerId>,
}

fn fixture(frames: u32, ncols: usize, ndrawings: usize) -> Fx {
    let mut engine = Engine::new("sheet");
    let scene = engine.add_scene("S");
    let cut = engine.add_cut(scene, "C", frames).unwrap();
    let at = CutRef { scene, cut };
    let cols = (0..ncols)
        .map(|i| engine.add_column(at, ((b'A' + i as u8) as char).to_string()).unwrap())
        .collect();
    let mut d = Vec::new();
    let mut layers = Vec::new();
    let mut cmds = Vec::new();
    for i in 0..ndrawings {
        let id = engine.alloc_drawing_id();
        let layer = engine.alloc_layer_id();
        cmds.push(Command::AddDrawing {
            at,
            id,
            index: i,
            name: (i + 1).to_string(),
            strokes: vec![],
            layers: vec![CelLayer::new(layer, "paint")],
        });
        d.push(id);
        layers.push(layer);
    }
    if !cmds.is_empty() {
        engine.apply("drawings", cmds).unwrap();
    }
    engine.clear_history();
    Fx {
        engine,
        at,
        cols,
        d,
        layers,
    }
}

impl Fx {
    fn cut(&self) -> &Cut {
        self.engine.project.cut(self.at.scene, self.at.cut).unwrap()
    }
    fn col(&self, i: usize) -> &Column {
        self.cut().xsheet.column(self.cols[i]).unwrap()
    }
    fn keys(&self, i: usize) -> Vec<(u32, Exposure)> {
        self.col(i).keys().collect()
    }
    fn dr(&self, i: usize) -> Exposure {
        Exposure::Drawing(self.d[i])
    }
    fn apply(&mut self, cmds: Vec<Command>) {
        self.engine.apply("edit", cmds).unwrap();
    }
    /// Key column `i` with `(frame, exposure)` pairs (one step).
    fn key(&mut self, i: usize, keys: &[(u32, Exposure)]) {
        let column = self.cols[i];
        let cells = keys
            .iter()
            .map(|&(frame, e)| CellEdit {
                column,
                frame,
                before: self.col(i).key_at(frame),
                after: Some(e),
            })
            .collect();
        let at = self.at;
        self.apply(vec![Command::SetCells { at, cells }]);
    }
    fn paint(&mut self, drawing: usize, coord: (i32, i32), rgba: [u16; 4]) {
        let (at, id, layer) = (self.at, self.d[drawing], self.layers[drawing]);
        self.apply(vec![Command::PaintTiles {
            at,
            id,
            layer,
            diff: vec![(coord, None, Some(tile(rgba)))],
        }]);
    }
    fn def(&mut self, i: usize, def: ColumnDef) {
        let (at, column) = (self.at, self.cols[i]);
        self.apply(vec![Command::SetColumnDef { at, column, def }]);
    }
}

/// Fold of every drawing's content hash, in project order.
fn content_hash(engine: &Engine) -> Vec<u64> {
    engine
        .project
        .scenes
        .iter()
        .flat_map(|s| s.cuts.iter())
        .flat_map(|c| c.drawings.iter().map(|d| d.content_hash()))
        .collect()
}

fn temp_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn kind(kind: ColumnKind) -> ColumnDef {
    ColumnDef {
        kind,
        ..ColumnDef::default()
    }
}

// ---- 1: inverse fuzz -------------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

#[test]
fn inverse_fuzz_200_sheet_commands_undo_all_restores_everything() {
    let mut fx = fixture(24, 3, 3);
    fx.paint(0, (0, 0), RED);
    let (s0, k0) = (fx.cut().xsheet.columns[0].id, fx.dr(0));
    fx.apply(vec![Command::SetCell {
        at: fx.at,
        column: s0,
        frame: 0,
        key: Some(k0),
    }]);
    fx.engine.clear_history();
    let scenes0 = fx.engine.project.scenes.clone();
    let hash0 = content_hash(&fx.engine);

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut applied = 0;
    let mut extra_cuts: Vec<CutId> = Vec::new();
    for step in 0..200 {
        let at = fx.at;
        let cut = fx.cut().clone();
        let n = cut.frame_count as u64;
        let cols: Vec<ColumnId> = cut.xsheet.columns.iter().map(|c| c.id).collect();
        let pick = |r: &mut Rng| cols[r.below(cols.len() as u64) as usize];
        let exposure = |r: &mut Rng| match r.below(4) {
            0 => None,
            1 => Some(Exposure::Empty),
            i => Some(Exposure::Drawing(fx.d[(i as usize + step) % fx.d.len()])),
        };
        let cmds: anim_core::error::Result<Vec<Command>> = match rng.below(19) {
            0..=2 => {
                let column = pick(&mut rng);
                let frame = rng.below(n + 4) as u32;
                let before = cut.xsheet.column(column).unwrap().key_at(frame);
                let after = exposure(&mut rng);
                Ok(vec![Command::SetCells {
                    at,
                    cells: vec![CellEdit {
                        column,
                        frame,
                        before,
                        after,
                    }],
                }])
            }
            3 => Ok(vec![Command::InsertFrames {
                at,
                frame: rng.below(n + 1) as u32,
                count: 1 + rng.below(3) as u32,
            }]),
            4 => Ok(vec![Command::DeleteFrames {
                at,
                frame: rng.below(n) as u32,
                count: 1 + rng.below(3) as u32,
            }]),
            5 => Ok(vec![Command::SetFrameCount {
                at,
                frames: 1 + rng.below(40) as u32,
            }]),
            6 => Ok(vec![Command::SetMemo {
                at,
                frame: rng.below(n) as u32,
                text: (rng.below(3) > 0).then(|| format!("memo {step}")),
            }]),
            7 => {
                let key = ["scene_no", "cut_no", "note"][rng.below(3) as usize].to_string();
                Ok(vec![Command::SetCutMeta {
                    at,
                    before: cut.meta.get(&key).cloned(),
                    after: (rng.below(3) > 0).then(|| format!("v{step}")),
                    key,
                }])
            }
            8 => Ok(vec![Command::SetColumnDef {
                at,
                column: pick(&mut rng),
                def: ColumnDef {
                    kind: ColumnKind::ALL[rng.below(4) as usize],
                    hidden: rng.below(3) == 0,
                    locked: rng.below(6) == 0,
                },
            }]),
            9 => {
                let mut column = Column::new(fx.engine.alloc_column_id(), format!("N{step}"));
                column.set_key(rng.below(n) as u32, fx.dr(rng.below(3) as usize));
                Ok(vec![Command::AddColumn {
                    at,
                    index: rng.below(cols.len() as u64 + 1) as usize,
                    column,
                    def: kind(ColumnKind::Book),
                }])
            }
            10 if cols.len() > 1 => Ok(vec![Command::RemoveColumn {
                at,
                id: pick(&mut rng),
            }]),
            11 => Ok(vec![Command::MoveColumn {
                at,
                id: pick(&mut rng),
                to_index: rng.below(cols.len() as u64) as usize,
            }]),
            12 => Ok(vec![Command::RenameColumn {
                at,
                id: pick(&mut rng),
                name: format!("R{step}"),
            }]),
            13 => {
                if let Some(p) = cut.xsheet.params.first().filter(|_| rng.below(2) == 0) {
                    Ok(vec![Command::RemoveParamColumn { at, id: p.id }])
                } else {
                    let mut p = ParamColumn::new(fx.engine.alloc_param_id(), "P");
                    p.set_key(
                        rng.below(n) as u32,
                        ParamKey {
                            value: step as f32,
                            interp: ParamInterp::Linear,
                        },
                    );
                    Ok(vec![Command::AddParamColumn {
                        at,
                        index: 0,
                        column: p,
                    }])
                }
            }
            14 => sheet_ops::hold_delta(
                &fx.engine.project,
                at,
                pick(&mut rng),
                rng.below(n) as u32,
                [1, -1, 2, -2][rng.below(4) as usize],
            ),
            15 => {
                let from = rng.below(n) as u32;
                sheet_ops::retime_range(
                    &fx.engine.project,
                    at,
                    &cols,
                    from,
                    from + 1 + rng.below(6) as u32,
                    1 + rng.below(3) as u32,
                )
            }
            16 => {
                let from = rng.below(n) as u32;
                sheet_ops::reverse_range(&fx.engine.project, at, &cols, from, from + 2 + rng.below(6) as u32)
            }
            17 => {
                let from = rng.below(n) as u32;
                sheet_ops::copy_block(&fx.engine.project, at, &cols[..1], from, from + 3).and_then(|b| {
                    let mode = if rng.below(2) == 0 { PasteMode::Insert } else { PasteMode::Overwrite };
                    sheet_ops::paste_block(&mut fx.engine.project, &b, at, pick(&mut rng), rng.below(n) as u32, mode)
                })
            }
            _ => {
                if let Some(id) = extra_cuts.pop() {
                    Ok(vec![Command::RemoveCut {
                        at: CutRef { scene: at.scene, cut: id },
                    }])
                } else {
                    let id = fx.engine.alloc_cut_id();
                    extra_cuts.push(id);
                    Ok(vec![Command::AddCut {
                        scene: at.scene,
                        index: 1,
                        cut: Box::new(Cut::new(id, "extra", 12)),
                    }])
                }
            }
        };
        let Ok(cmds) = cmds else { continue };
        if !cmds.is_empty() && fx.engine.apply("fuzz", cmds).is_ok() {
            applied += 1;
        }
    }
    assert!(applied >= 120, "only {applied} of 200 fuzz commands applied");
    let scenes1 = fx.engine.project.scenes.clone();
    for _ in 0..applied {
        fx.engine.undo().unwrap();
    }
    assert_eq!(fx.engine.project.scenes, scenes0, "undo all restores the structure");
    assert_eq!(content_hash(&fx.engine), hash0, "undo all restores content_hash");
    for _ in 0..applied {
        fx.engine.redo().unwrap();
    }
    assert_eq!(fx.engine.project.scenes, scenes1, "redo all replays exactly");
}

// ---- 2: insert / delete frames ---------------------------------------------

#[test]
fn insert_three_frames_at_10_ripples_columns_params_and_memos() {
    let mut fx = fixture(48, 2, 3);
    let (a, b, c) = (fx.dr(0), fx.dr(1), fx.dr(2));
    fx.key(0, &[(0, a), (8, b), (10, c), (12, Exposure::Empty), (30, a)]);
    fx.key(1, &[(5, b), (20, c)]);
    let at = fx.at;
    let p = fx.engine.add_param_column(at, "pan").unwrap();
    let key = |v| Some(ParamKey { value: v, interp: ParamInterp::Linear });
    fx.apply(vec![
        Command::SetParamKey { at, column: p, frame: 9, key: key(1.0) },
        Command::SetParamKey { at, column: p, frame: 10, key: key(2.0) },
        Command::SetParamKey { at, column: p, frame: 40, key: key(3.0) },
        Command::SetMemo { at, frame: 10, text: Some("セリフ".into()) },
        Command::SetMemo { at, frame: 4, text: Some("before".into()) },
    ]);
    let before = fx.engine.project.scenes.clone();

    fx.apply(vec![Command::InsertFrames { at, frame: 10, count: 3 }]);
    assert_eq!(fx.cut().frame_count, 51);
    assert_eq!(fx.keys(0), vec![(0, a), (8, b), (13, c), (15, Exposure::Empty), (33, a)]);
    assert_eq!(fx.keys(1), vec![(5, b), (23, c)]);
    let pk: Vec<u32> = fx.cut().xsheet.param(p).unwrap().keys().map(|k| k.0).collect();
    assert_eq!(pk, vec![9, 13, 43]);
    let memos: Vec<u32> = fx.cut().memos.keys().copied().collect();
    assert_eq!(memos, vec![4, 13]);

    fx.apply(vec![Command::DeleteFrames { at, frame: 10, count: 3 }]);
    assert_eq!(fx.engine.project.scenes, before, "deleting the 3 frames restores exactly");
}

#[test]
fn delete_frames_undo_restores_removed_keys_params_and_memos() {
    let mut fx = fixture(24, 1, 2);
    let (a, b) = (fx.dr(0), fx.dr(1));
    fx.key(0, &[(0, a), (4, b), (5, a), (9, b)]);
    let at = fx.at;
    let p = fx.engine.add_param_column(at, "x").unwrap();
    fx.apply(vec![
        Command::SetParamKey {
            at,
            column: p,
            frame: 5,
            key: Some(ParamKey { value: 7.0, interp: ParamInterp::Hold }),
        },
        Command::SetMemo { at, frame: 4, text: Some("gone".into()) },
    ]);
    let before = fx.engine.project.scenes.clone();
    fx.apply(vec![Command::DeleteFrames { at, frame: 4, count: 3 }]);
    assert_eq!(fx.cut().frame_count, 21);
    assert_eq!(fx.keys(0), vec![(0, a), (6, b)]);
    assert!(fx.cut().memos.is_empty());
    assert_eq!(fx.cut().xsheet.param(p).unwrap().keys().count(), 0);
    fx.engine.undo().unwrap();
    assert_eq!(fx.engine.project.scenes, before);
    // A cut keeps at least one frame.
    let r = fx.engine.apply("all", vec![Command::DeleteFrames { at, frame: 0, count: 24 }]);
    assert!(r.is_err());
}

// ---- 3-5: retime, hold, reverse --------------------------------------------

#[test]
fn retime_range_to_twos_holds_each_drawing_two_frames_and_ripples() {
    let mut fx = fixture(24, 1, 4);
    let (a, b, c, d) = (fx.dr(0), fx.dr(1), fx.dr(2), fx.dr(3));
    fx.key(0, &[(2, a), (3, b), (4, c), (5, d)]);
    let before = fx.engine.project.scenes.clone();
    let cmds = sheet_ops::retime_range(&fx.engine.project, fx.at, &[fx.cols[0]], 2, 5, 2).unwrap();
    fx.apply(cmds);
    // The range start (2) stays; a, b, c hold 2 each; d ripples from 5 to 8.
    assert_eq!(fx.keys(0), vec![(2, a), (4, b), (6, c), (8, d)]);
    fx.engine.undo().unwrap();
    assert_eq!(fx.engine.project.scenes, before);
    // Threes from ones, and back to ones.
    let cmds = sheet_ops::retime_range(&fx.engine.project, fx.at, &[fx.cols[0]], 2, 5, 3).unwrap();
    fx.apply(cmds);
    assert_eq!(fx.keys(0), vec![(2, a), (5, b), (8, c), (11, d)]);
}

#[test]
fn hold_plus_one_extends_the_exposure_and_ripples() {
    let mut fx = fixture(24, 1, 3);
    let (a, b, c) = (fx.dr(0), fx.dr(1), fx.dr(2));
    fx.key(0, &[(0, a), (2, b), (4, c)]);
    let cmds = sheet_ops::hold_delta(&fx.engine.project, fx.at, fx.cols[0], 1, 1).unwrap();
    fx.apply(cmds);
    assert_eq!(fx.keys(0), vec![(0, a), (3, b), (5, c)]);
    fx.engine.undo().unwrap();
    assert_eq!(fx.keys(0), vec![(0, a), (2, b), (4, c)]);
}

#[test]
fn hold_minus_one_on_a_one_frame_exposure_removes_it_and_ripples() {
    let mut fx = fixture(24, 1, 3);
    let (a, b, c) = (fx.dr(0), fx.dr(1), fx.dr(2));
    fx.key(0, &[(0, a), (2, b), (3, c), (5, a)]);
    let cmds = sheet_ops::hold_delta(&fx.engine.project, fx.at, fx.cols[0], 2, -1).unwrap();
    fx.apply(cmds);
    assert_eq!(fx.keys(0), vec![(0, a), (2, c), (4, a)]);
    // −1 on a longer exposure shortens it by one.
    let cmds = sheet_ops::hold_delta(&fx.engine.project, fx.at, fx.cols[0], 0, -1).unwrap();
    fx.apply(cmds);
    assert_eq!(fx.keys(0), vec![(0, a), (1, c), (3, a)]);
}

#[test]
fn reverse_flips_the_drawing_order_and_keeps_each_length() {
    let mut fx = fixture(24, 1, 3);
    let (a, b, c) = (fx.dr(0), fx.dr(1), fx.dr(2));
    fx.key(0, &[(0, a), (1, b), (3, c), (6, Exposure::Empty)]);
    let cmds = sheet_ops::reverse_range(&fx.engine.project, fx.at, &[fx.cols[0]], 0, 6).unwrap();
    fx.apply(cmds);
    assert_eq!(fx.keys(0), vec![(0, c), (3, b), (5, a), (6, Exposure::Empty)]);
    assert_eq!(fx.col(0).runs(0, 6), vec![(c, 3), (b, 2), (a, 1)]);
}

// ---- 6: typed entry --------------------------------------------------------

#[test]
fn expose_by_name_creates_or_reuses_in_one_step_and_x_empties() {
    let mut fx = fixture(24, 1, 2);
    let at = fx.at;
    let before = fx.engine.project.scenes.clone();
    let layer = fx.engine.alloc_layer_id();
    let cmds = sheet_ops::expose_by_name(
        &mut fx.engine.project,
        at,
        fx.cols[0],
        4,
        " 3 ",
        vec![CelLayer::new(layer, "paint")],
    )
    .unwrap();
    fx.apply(cmds);
    let three = fx.cut().drawings.iter().find(|d| d.name == "3").expect("drawing 3 created").id;
    assert_eq!(fx.col(0).key_at(4), Some(Exposure::Drawing(three)));
    fx.engine.undo().unwrap();
    assert_eq!(fx.engine.project.scenes, before, "creation and exposure are one undo step");

    // An existing name reuses its drawing.
    let cmds = sheet_ops::expose_by_name(&mut fx.engine.project, at, fx.cols[0], 4, "2", vec![]).unwrap();
    assert_eq!(cmds.len(), 1, "no new drawing when the name exists");
    fx.apply(cmds);
    assert_eq!(fx.col(0).key_at(4), Some(fx.dr(1)));
    let cmds = sheet_ops::expose_by_name(&mut fx.engine.project, at, fx.cols[0], 4, "x", vec![]).unwrap();
    fx.apply(cmds);
    assert_eq!(fx.col(0).key_at(4), Some(Exposure::Empty));
    assert_eq!(fx.cut().drawings.len(), 2);
}

// ---- 7: copy and paste -----------------------------------------------------

#[test]
fn paste_block_3x5_reproduces_its_exposures_within_the_cut() {
    let mut fx = fixture(24, 6, 3);
    let (a, b, c) = (fx.dr(0), fx.dr(1), fx.dr(2));
    fx.key(0, &[(0, a), (2, b)]);
    fx.key(1, &[(0, b), (1, Exposure::Empty), (3, c)]);
    fx.key(2, &[(1, c), (4, a)]);
    let at = fx.at;
    let block = sheet_ops::copy_block(&fx.engine.project, at, &fx.cols[0..3], 0, 5).unwrap();
    assert_eq!(block.columns.len(), 3);
    let cmds = sheet_ops::paste_block(&mut fx.engine.project, &block, at, fx.cols[3], 10, PasteMode::Overwrite).unwrap();
    fx.apply(cmds);
    for i in 0..3 {
        assert_eq!(fx.col(3 + i).runs(10, 15), fx.col(i).runs(0, 5), "column {i}");
    }
    assert_eq!(fx.cut().drawings.len(), 3, "within a cut the same drawings are referenced");
}

#[test]
fn paste_across_cuts_copies_drawings_with_new_ids_in_one_step() {
    let mut fx = fixture(24, 2, 2);
    fx.paint(0, (0, 0), RED);
    fx.paint(1, (1, 0), BLUE);
    let (a, b) = (fx.dr(0), fx.dr(1));
    fx.key(0, &[(0, a), (2, b)]);
    fx.key(1, &[(1, b)]);
    let block = sheet_ops::copy_block(&fx.engine.project, fx.at, &fx.cols, 0, 4).unwrap();

    let other = fx.engine.add_cut(fx.at.scene, "C2", 24).unwrap();
    let to = CutRef { scene: fx.at.scene, cut: other };
    let t0 = fx.engine.add_column(to, "A").unwrap();
    fx.engine.add_column(to, "B").unwrap();
    fx.engine.clear_history();
    let before = fx.engine.project.scenes.clone();
    let cmds = sheet_ops::paste_block(&mut fx.engine.project, &block, to, t0, 0, PasteMode::Overwrite).unwrap();
    fx.engine.apply("paste", cmds).unwrap();
    let src = fx.cut().clone();
    let dst = fx.engine.project.cut(to.scene, to.cut).unwrap().clone();
    assert_eq!(dst.drawings.len(), 2);
    for (s, d) in src.drawings.iter().zip(&dst.drawings) {
        assert_ne!(s.id, d.id, "a new drawing id");
        assert!(src.drawing(d.id).is_none());
        assert_ne!(s.layers[0].id, d.layers[0].id, "new layer ids");
        assert_eq!(s.name, d.name);
        let hashes = |x: &anim_core::model::Drawing| -> Vec<u64> { x.layers.iter().map(|l| l.content_hash()).collect() };
        assert_eq!(hashes(s), hashes(d), "same tiles");
    }
    let remap = |e: Exposure| match e {
        Exposure::Drawing(x) => Exposure::Drawing(dst.drawings[src.drawings.iter().position(|s| s.id == x).unwrap()].id),
        e => e,
    };
    for (i, col) in dst.xsheet.columns.iter().enumerate() {
        let want: Vec<(Exposure, u32)> = src.xsheet.columns[i].runs(0, 4).into_iter().map(|(e, n)| (remap(e), n)).collect();
        assert_eq!(col.runs(0, 4), want);
    }
    fx.engine.undo().unwrap();
    assert_eq!(fx.engine.project.scenes, before, "the paste is one undo step");
}

#[test]
fn paste_insert_mode_ripples_what_follows() {
    let mut fx = fixture(24, 2, 3);
    let (a, b, c) = (fx.dr(0), fx.dr(1), fx.dr(2));
    fx.key(0, &[(0, a), (1, b)]);
    fx.key(1, &[(0, c), (2, a), (5, b)]);
    let block = sheet_ops::copy_block(&fx.engine.project, fx.at, &fx.cols[0..1], 0, 2).unwrap();
    let cmds = sheet_ops::paste_block(&mut fx.engine.project, &block, fx.at, fx.cols[1], 2, PasteMode::Insert).unwrap();
    fx.apply(cmds);
    assert_eq!(fx.keys(1), vec![(0, c), (2, a), (3, b), (4, a), (7, b)]);
    // Overwrite at the same place moves nothing after the block.
    fx.engine.undo().unwrap();
    let cmds = sheet_ops::paste_block(&mut fx.engine.project, &block, fx.at, fx.cols[1], 2, PasteMode::Overwrite).unwrap();
    fx.apply(cmds);
    assert_eq!(fx.keys(1), vec![(0, c), (2, a), (3, b), (4, a), (5, b)]);
    // Not enough columns to the right: refused.
    let wide = sheet_ops::copy_block(&fx.engine.project, fx.at, &fx.cols, 0, 2).unwrap();
    assert!(sheet_ops::paste_block(&mut fx.engine.project, &wide, fx.at, fx.cols[1], 0, PasteMode::Overwrite).is_err());
}

// ---- 8: kinds in every render path -----------------------------------------

#[test]
fn ref_and_hidden_columns_are_absent_from_every_render_path() {
    let mut fx = fixture(4, 1, 1);
    fx.paint(0, (0, 0), RED);
    let k = fx.dr(0);
    fx.key(0, &[(0, k)]);
    let at = fx.at;
    let (src, out) = (fx.engine.alloc_node_id(), fx.engine.alloc_node_id());
    fx.apply(vec![
        Command::AddNode { at, id: src, kind: NodeKind::DrawingSource { column: fx.cols[0] } },
        Command::AddNode { at, id: out, kind: NodeKind::Output },
        Command::Connect { at, from: src, from_pin: 0, to: out, to_pin: 0 },
        Command::SetOutput { at, node: Some(out) },
    ]);
    let opaque = |buf: &[u8]| buf.chunks(4).filter(|p| p[3] > 0).count();
    let shown_col = render_frame(fx.cut(), 0, 64, 64);
    let shown_graph = render_graph_frame(fx.cut(), 0, 64, 64).unwrap();
    assert!(opaque(&shown_col) > 0 && opaque(&shown_graph) > 0);
    let shown_hash = fx.engine.eval(at.scene, at.cut, 0).unwrap().hash();

    // The reference: the same cut with the cell cleared shows nothing.
    fx.key(0, &[(0, Exposure::Empty)]);
    let empty_hash = fx.engine.eval(at.scene, at.cut, 0).unwrap().hash();
    fx.engine.undo().unwrap();
    assert_ne!(shown_hash, empty_hash);

    for def in [
        kind(ColumnKind::Ref),
        ColumnDef { hidden: true, ..ColumnDef::default() },
        ColumnDef { kind: ColumnKind::Bg, hidden: true, locked: false },
    ] {
        fx.def(0, def);
        assert_eq!(opaque(&render_frame(fx.cut(), 0, 64, 64)), 0, "{def:?} column render");
        assert_eq!(opaque(&render_graph_frame(fx.cut(), 0, 64, 64).unwrap()), 0, "{def:?} graph render");
        assert_eq!(fx.engine.eval(at.scene, at.cut, 0).unwrap().hash(), empty_hash, "{def:?} eval hash");
        assert!(fx.cut().render_order().is_empty());
        fx.engine.undo().unwrap();
    }
    assert_eq!(fx.engine.eval(at.scene, at.cut, 0).unwrap().hash(), shown_hash);
}

#[test]
fn bg_and_book_bands_order_the_column_render() {
    let mut fx = fixture(4, 3, 3);
    for (i, rgba) in [RED, GREEN, BLUE].into_iter().enumerate() {
        fx.paint(i, (0, 0), rgba);
        let k = fx.dr(i);
        fx.key(i, &[(0, k)]);
    }
    let top = |fx: &Fx| render_frame(fx.cut(), 0, 64, 64)[0..4].to_vec();
    // All Cel: sheet order, the last column (blue) on top.
    let blue = top(&fx);
    assert!(blue[2] > 200 && blue[0] == 0, "{blue:?}");
    // A = Book (red), C = BG (blue): bands BG, Cel, Book → red on top.
    fx.def(0, kind(ColumnKind::Book));
    fx.def(2, kind(ColumnKind::Bg));
    assert_eq!(fx.cut().render_order(), vec![fx.cols[2], fx.cols[1], fx.cols[0]]);
    let red = top(&fx);
    assert!(red[0] > 200 && red[1] == 0 && red[2] == 0, "{red:?}");
    // B = Book too: the Book band keeps sheet order, so B (green) tops A.
    fx.def(1, kind(ColumnKind::Book));
    assert_eq!(fx.cut().render_order(), vec![fx.cols[2], fx.cols[0], fx.cols[1]]);
    let green = top(&fx);
    assert!(green[1] > 200 && green[0] == 0, "{green:?}");
}

#[test]
fn legacy_all_cel_cut_renders_in_sheet_order_byte_identical() {
    let mut fx = fixture(4, 3, 3);
    for (i, rgba) in [RED, GREEN, BLUE].into_iter().enumerate() {
        fx.paint(i, (i as i32, 0), rgba);
        fx.paint(i, (0, 1), rgba);
        let k = fx.dr(i);
        fx.key(i, &[(0, k)]);
    }
    assert_eq!(fx.cut().render_order(), fx.cols, "every column Cel: plain sheet order");
    let legacy = render_frame(fx.cut(), 0, 192, 128);
    // Explicit default defs change nothing (the map stays canonical).
    for i in 0..3 {
        fx.def(i, ColumnDef::default());
    }
    assert!(fx.cut().column_defs.is_empty());
    assert_eq!(render_frame(fx.cut(), 0, 192, 128), legacy);
    // Each column's own tile is its colour; the shared tile shows the last.
    let px = |x: usize, y: usize| legacy[(y * 192 + x) * 4..(y * 192 + x) * 4 + 4].to_vec();
    assert_eq!(px(0, 0)[0], 255);
    assert_eq!(px(64, 0)[1], 255);
    assert_eq!(px(128, 0)[2], 255);
    assert_eq!(px(0, 64), px(128, 0), "sheet order: the last column on top");
}

// ---- 9: structural undo ----------------------------------------------------

#[test]
fn remove_column_then_undo_restores_its_id_keys_and_def() {
    let mut fx = fixture(24, 3, 2);
    let (a, b) = (fx.dr(0), fx.dr(1));
    fx.key(1, &[(0, a), (3, b)]);
    fx.def(1, ColumnDef { kind: ColumnKind::Bg, hidden: true, locked: false });
    let before = fx.engine.project.scenes.clone();
    let (at, id) = (fx.at, fx.cols[1]);
    fx.apply(vec![Command::RemoveColumn { at, id }]);
    assert!(fx.cut().xsheet.column(id).is_none());
    assert!(fx.cut().column_defs.is_empty(), "the def leaves with its column");
    fx.engine.undo().unwrap();
    assert_eq!(fx.engine.project.scenes, before);
    assert_eq!(fx.cut().xsheet.columns[1].id, id);
    assert_eq!(fx.keys(1), vec![(0, a), (3, b)]);
    assert!(fx.cut().column_def(id).hidden);
}

#[test]
fn add_cut_then_undo_and_remove_cut_restores_the_snapshot() {
    let mut fx = fixture(24, 1, 1);
    let k = fx.dr(0);
    fx.key(0, &[(0, k)]);
    fx.engine.clear_history();
    let before = fx.engine.project.scenes.clone();
    let scene = fx.at.scene;
    let id = fx.engine.alloc_cut_id();
    fx.apply(vec![Command::AddCut { scene, index: 1, cut: Box::new(Cut::new(id, "C2", 36)) }]);
    assert_eq!(fx.engine.project.cut(scene, id).unwrap().frame_count, 36);
    // A duplicate id is refused.
    let dup = Command::AddCut { scene, index: 0, cut: Box::new(Cut::new(id, "dup", 1)) };
    assert!(fx.engine.apply("dup", vec![dup]).is_err());
    fx.engine.undo().unwrap();
    assert_eq!(fx.engine.project.scenes, before);

    fx.apply(vec![Command::RemoveCut { at: fx.at }]);
    assert!(fx.engine.project.cut(scene, fx.at.cut).is_none());
    fx.engine.undo().unwrap();
    assert_eq!(fx.engine.project.scenes, before, "the whole cut comes back");
}

#[test]
fn rename_move_param_and_length_commands_undo_exactly() {
    let mut fx = fixture(24, 3, 0);
    let at = fx.at;
    let before = fx.engine.project.scenes.clone();
    let p = ParamColumn::new(fx.engine.alloc_param_id(), "zoom");
    let pid = p.id;
    fx.apply(vec![
        Command::RenameColumn { at, id: fx.cols[0], name: "BG".into() },
        Command::MoveColumn { at, id: fx.cols[0], to_index: 2 },
        Command::AddParamColumn { at, index: 0, column: p },
        Command::RenameCut { at, name: "C-renamed".into() },
        Command::SetFrameCount { at, frames: 72 },
    ]);
    let cut = fx.cut();
    assert_eq!(cut.xsheet.columns[2].name, "BG");
    assert_eq!(cut.xsheet.params[0].id, pid);
    assert_eq!((cut.name.as_str(), cut.frame_count), ("C-renamed", 72));
    fx.apply(vec![Command::RemoveParamColumn { at, id: pid }]);
    assert!(fx.cut().xsheet.params.is_empty());
    fx.engine.undo().unwrap();
    fx.engine.undo().unwrap();
    assert_eq!(fx.engine.project.scenes, before);
    // Length stays at least one frame; exposures past the end are kept.
    assert!(fx.engine.apply("zero", vec![Command::SetFrameCount { at, frames: 0 }]).is_err());
}

// ---- 10: store -------------------------------------------------------------

fn store_fixture() -> Fx {
    let mut fx = fixture(24, 2, 2);
    fx.paint(0, (0, 0), RED);
    let (a, b) = (fx.dr(0), fx.dr(1));
    fx.key(0, &[(0, a), (6, b)]);
    let at = fx.at;
    fx.apply(vec![
        Command::SetCutMeta { at, key: "cut_no".into(), before: None, after: Some("C012".into()) },
        Command::SetMemo { at, frame: 3, text: Some("「行くぞ」".into()) },
        Command::SetColumnDef { at, column: fx.cols[1], def: ColumnDef { kind: ColumnKind::Book, hidden: true, locked: true } },
    ]);
    fx
}

#[test]
fn store_v11_round_trip_keeps_meta_memos_and_column_defs() {
    let fx = store_fixture();
    let path = temp_path("gm5_v11_round_trip.gaban");
    fx.engine.save(&path).unwrap();
    let back = Engine::load(&path).unwrap();
    assert_eq!(back.project, fx.engine.project);
    let cut = back.project.cut(fx.at.scene, fx.at.cut).unwrap();
    assert_eq!(cut.meta.get("cut_no").map(String::as_str), Some("C012"));
    assert_eq!(cut.memos.get(&3).map(String::as_str), Some("「行くぞ」"));
    assert_eq!(cut.column_def(fx.cols[1]), ColumnDef { kind: ColumnKind::Book, hidden: true, locked: true });
    let conn = rusqlite::Connection::open(&path).unwrap();
    let v: String = conn.query_row("SELECT value FROM meta WHERE key='schema_version'", (), |r| r.get(0)).unwrap();
    assert_eq!(v, "11");
    drop(conn);
    std::fs::remove_file(&path).unwrap();
}

/// Save, then strip the v11 side tables and stamp `version` (a file from
/// before GM5), running `extra` SQL too.
fn forge_old(fx: &Fx, name: &str, version: &str, extra: &str) -> std::path::PathBuf {
    let path = temp_path(name);
    fx.engine.save(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(&format!(
        "DROP TABLE cut_meta; DROP TABLE column_defs; DROP TABLE cut_memos;
         UPDATE meta SET value='{version}' WHERE key='schema_version'; {extra}"
    ))
    .unwrap();
    path
}

#[test]
fn store_v9_file_loads_with_empty_meta_memos_and_defs() {
    let fx = store_fixture();
    let path = forge_old(&fx, "gm5_v9.gaban", "9", "");
    let back = Engine::load(&path).unwrap();
    let cut = back.project.cut(fx.at.scene, fx.at.cut).unwrap();
    assert!(cut.meta.is_empty() && cut.memos.is_empty() && cut.column_defs.is_empty());
    assert_eq!(cut.xsheet, fx.cut().xsheet, "the sheet itself loads as before");
    assert_eq!(cut.render_order(), fx.cols, "a v9 cut is all Cel");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn store_v10_file_loads_and_resaves_keeping_its_extra_table() {
    let fx = store_fixture();
    let path = forge_old(
        &fx,
        "gm5_v10.gaban",
        "10",
        "CREATE TABLE cel_layer_groups(group_id INTEGER PRIMARY KEY, name TEXT NOT NULL);
         INSERT INTO cel_layer_groups VALUES (1, 'hair');",
    );
    let mut back = Engine::load(&path).unwrap();
    assert!(back.project.cut(fx.at.scene, fx.at.cut).unwrap().meta.is_empty());
    let at = fx.at;
    back.apply("memo", vec![Command::SetMemo { at, frame: 1, text: Some("kept".into()) }]).unwrap();
    back.save(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let name: String = conn.query_row("SELECT name FROM cel_layer_groups WHERE group_id = 1", (), |r| r.get(0)).unwrap();
    assert_eq!(name, "hair", "the unknown table survives the re-save");
    drop(conn);
    let again = Engine::load(&path).unwrap();
    assert_eq!(again.project.cut(at.scene, at.cut).unwrap().memos.get(&1).map(String::as_str), Some("kept"));
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn store_v12_file_is_refused_as_from_a_newer_gaban_studio() {
    let fx = store_fixture();
    let path = temp_path("gm5_v12.gaban");
    fx.engine.save(&path).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE meta SET value='12' WHERE key='schema_version'", ()).unwrap();
    drop(conn);
    let err = Engine::load(&path).err().expect("v12 refused").to_string();
    assert!(err.contains("newer Gaban Studio"), "{err}");
    std::fs::remove_file(&path).unwrap();
}

// ---- 11: memo --------------------------------------------------------------

#[test]
fn memo_text_round_trips_and_is_undoable() {
    let mut fx = fixture(24, 1, 0);
    let at = fx.at;
    fx.apply(vec![Command::SetMemo { at, frame: 5, text: Some("走る".into()) }]);
    fx.apply(vec![Command::SetMemo { at, frame: 5, text: Some("止まる".into()) }]);
    assert_eq!(fx.cut().memos.get(&5).map(String::as_str), Some("止まる"));
    fx.engine.undo().unwrap();
    assert_eq!(fx.cut().memos.get(&5).map(String::as_str), Some("走る"));
    // Empty text clears the row; undo brings it back.
    fx.apply(vec![Command::SetMemo { at, frame: 5, text: Some(String::new()) }]);
    assert!(fx.cut().memos.is_empty());
    fx.engine.undo().unwrap();
    fx.engine.undo().unwrap();
    assert!(fx.cut().memos.is_empty());
    fx.engine.redo().unwrap();
    let path = temp_path("gm5_memo.gaban");
    fx.engine.save(&path).unwrap();
    let back = Engine::load(&path).unwrap();
    assert_eq!(back.project.cut(at.scene, at.cut).unwrap().memos.get(&5).map(String::as_str), Some("走る"));
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn cut_meta_is_checked_against_its_before_and_undoable() {
    let mut fx = fixture(24, 1, 0);
    let at = fx.at;
    let set = |before: Option<&str>, after: Option<&str>| Command::SetCutMeta {
        at,
        key: "scene_no".into(),
        before: before.map(String::from),
        after: after.map(String::from),
    };
    fx.apply(vec![set(None, Some("S01"))]);
    assert!(fx.engine.apply("stale", vec![set(None, Some("S02"))]).is_err(), "stale before refused");
    fx.apply(vec![set(Some("S01"), Some("S02"))]);
    fx.engine.undo().unwrap();
    assert_eq!(fx.cut().meta.get("scene_no").map(String::as_str), Some("S01"));
    fx.engine.undo().unwrap();
    assert!(fx.cut().meta.is_empty());
}

// ---- 12: locks -------------------------------------------------------------

#[test]
fn locked_column_refuses_sheet_edits_and_paint_with_the_reason() {
    let mut fx = fixture(24, 2, 2);
    let (a, b) = (fx.dr(0), fx.dr(1));
    fx.key(0, &[(0, a), (4, b)]);
    fx.def(0, ColumnDef { locked: true, ..ColumnDef::default() });
    let at = fx.at;
    let before = fx.engine.project.scenes.clone();
    let cells = Command::SetCells {
        at,
        cells: vec![CellEdit { column: fx.cols[0], frame: 2, before: None, after: Some(b) }],
    };
    let err = fx.engine.apply("cells", vec![cells]).unwrap_err().to_string();
    assert!(err.contains("locked") && err.contains('A'), "{err}");
    let paint = Command::PaintTiles { at, id: fx.d[0], layer: fx.layers[0], diff: vec![((0, 0), None, Some(tile(RED)))] };
    let err = fx.engine.apply("paint", vec![paint]).unwrap_err().to_string();
    assert!(err.contains("locked"), "{err}");
    let err = fx.engine.apply("cell", vec![Command::SetCell { at, column: fx.cols[0], frame: 1, key: Some(a) }]).unwrap_err().to_string();
    assert!(err.contains("locked"), "{err}");
    let err = fx.engine.apply("insert", vec![Command::InsertFrames { at, frame: 2, count: 1 }]).unwrap_err().to_string();
    assert!(err.contains("locked"), "{err}");
    let hold = sheet_ops::hold_delta(&fx.engine.project, at, fx.cols[0], 0, 1).unwrap();
    assert!(fx.engine.apply("hold", hold).is_err());
    assert_eq!(fx.engine.project.scenes, before, "every refusal leaves the sheet untouched");
    // The unlocked neighbour still edits; unlocking restores edits.
    fx.key(1, &[(2, b)]);
    fx.def(0, ColumnDef::default());
    fx.key(0, &[(2, b)]);
    assert_eq!(fx.col(0).key_at(2), Some(b));
}

// ---- wire ------------------------------------------------------------------

#[test]
fn gm5_commands_round_trip_through_serde() {
    let fx = store_fixture();
    let cut = fx.cut().clone();
    let cmds = vec![
        Command::AddCut { scene: fx.at.scene, index: 0, cut: Box::new(cut) },
        Command::SetCells {
            at: fx.at,
            cells: vec![CellEdit { column: fx.cols[0], frame: 3, before: None, after: Some(Exposure::Empty) }],
        },
        Command::SetColumnDef { at: fx.at, column: fx.cols[0], def: kind(ColumnKind::Ref) },
        Command::DeleteFrames { at: fx.at, frame: 1, count: 2 },
    ];
    let wire = serde_json::to_string(&cmds).unwrap();
    let back: Vec<Command> = serde_json::from_str(&wire).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), wire);
    let Command::AddCut { cut, .. } = &back[0] else { panic!("AddCut") };
    assert_eq!(**cut, *fx.cut(), "the cut snapshot survives the wire");
}
