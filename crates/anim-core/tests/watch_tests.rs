//! Workflow Watch: fire and near-miss streams for each check at both levels,
//! the tile box of a step, the key spans, and retirement.

use anim_core::command::{Command, CutRef};
use anim_core::ids::*;
use anim_core::model::{Cut, Drawing, DrawingDef, DrawingKind, SpacingChart, Stroke};
use anim_core::raster::TileDiff;
use anim_core::watch::*;
use anim_core::xsheet::{Column, Exposure};

const P: &str = "C:/shots/a.anim";
const PAPER: f64 = 1920.0 * 1080.0;

fn at() -> CutRef {
    CutRef { scene: SceneId(1), cut: CutId(2) }
}

fn other_cut() -> CutRef {
    CutRef { scene: SceneId(1), cut: CutId(3) }
}

fn spot(d: u64) -> Spot {
    Spot {
        at: at(),
        key: DrawKey::Drawing(DrawingId(d)),
        drawing: Some(DrawingId(d)),
        frames: FrameRange { from: 4, to: 7 },
    }
}

fn watch(level: Level) -> Watch {
    let mut w = Watch::new(Book::default());
    w.book.set_level(level);
    w.open_project(P);
    w
}

fn fires(out: &[Output], check: CheckId) -> usize {
    out.iter()
        .filter(|o| matches!(o, Output::Fire(f) if f.check == check))
        .count()
}

fn tiles(x: i32, y: i32) -> TileBox {
    TileBox { x0: x, y0: y, x1: x + 1, y1: y + 1 }
}

fn undo(w: &mut Watch, now: f64, d: u64, b: TileBox) -> Vec<Output> {
    w.feed(now, Event::UndoOrErase { spot: spot(d), tiles: b })
}

/// `n` undos a second apart on one spot; returns the fires.
fn undo_run(w: &mut Watch, n: usize) -> usize {
    (0..n)
        .map(|i| fires(&undo(w, i as f64, 9, tiles(3, 3)), CheckId::UndoLoop))
        .sum()
}

// ---- undo loop ------------------------------------------------------------

#[test]
fn watch_undo_loop_fires_at_each_levels_count() {
    for (level, n) in [(Level::Rookie, 6), (Level::Amateur, 4)] {
        assert_eq!(undo_run(&mut watch(level), n - 1), 0, "{level:?} one short");
        assert_eq!(undo_run(&mut watch(level), n), 1, "{level:?} at the count");
    }
}

#[test]
fn watch_undo_loop_needs_the_run_inside_sixty_seconds() {
    let mut w = watch(Level::Amateur);
    let n: usize = (0..4)
        .map(|i| fires(&undo(&mut w, i as f64 * 21.0, 9, tiles(3, 3)), CheckId::UndoLoop))
        .sum();
    assert_eq!(n, 0, "63 s from first to last is outside the window");

    let mut w = watch(Level::Amateur);
    let n: usize = (0..4)
        .map(|i| fires(&undo(&mut w, i as f64 * 20.0, 9, tiles(3, 3)), CheckId::UndoLoop))
        .sum();
    assert_eq!(n, 1, "exactly 60 s is inside");
}

#[test]
fn watch_undo_loop_overlap_is_half_of_the_smaller_box() {
    // Half of a 2x2 box overlaps: a run. A quarter does not.
    let mut w = watch(Level::Amateur);
    let run = [tiles(0, 0), tiles(1, 0), tiles(2, 0), tiles(3, 0)];
    let n: usize = run
        .iter()
        .enumerate()
        .map(|(i, b)| fires(&undo(&mut w, i as f64, 9, *b), CheckId::UndoLoop))
        .sum();
    assert_eq!(n, 1);

    let mut w = watch(Level::Amateur);
    let run = [tiles(0, 0), tiles(1, 1), tiles(2, 2), tiles(3, 3)];
    let n: usize = run
        .iter()
        .enumerate()
        .map(|(i, b)| fires(&undo(&mut w, i as f64, 9, *b), CheckId::UndoLoop))
        .sum();
    assert_eq!(n, 0);

    // A big box and a small one inside it: the smaller box is the measure.
    let big = TileBox { x0: 0, y0: 0, x1: 9, y1: 9 };
    assert_eq!(big.overlap(TileBox::around(4, 4)), 1.0);
}

#[test]
fn watch_undo_loop_another_drawing_starts_a_new_run() {
    let mut w = watch(Level::Amateur);
    let n: usize = [9, 9, 8, 9]
        .iter()
        .enumerate()
        .map(|(i, d)| fires(&undo(&mut w, i as f64, *d, tiles(3, 3)), CheckId::UndoLoop))
        .sum();
    assert_eq!(n, 0);
}

#[test]
fn watch_undo_loop_counts_eraser_strokes_with_undos() {
    let mut w = watch(Level::Amateur);
    let area = tiles(3, 3).paper();
    let erase = Event::Stroke { spot: spot(9), area, erased: true, paper_area: PAPER };
    let draw = Event::Stroke { spot: spot(9), area, erased: false, paper_area: PAPER };
    let mut n = 0;
    n += fires(&w.feed(0.0, erase.clone()), CheckId::UndoLoop);
    n += fires(&w.feed(1.0, draw), CheckId::UndoLoop);
    n += fires(&w.feed(2.0, erase.clone()), CheckId::UndoLoop);
    n += fires(&undo(&mut w, 3.0, 9, tiles(3, 3)), CheckId::UndoLoop);
    assert_eq!(n, 0, "the plain stroke is not an erase");
    n += fires(&w.feed(4.0, erase), CheckId::UndoLoop);
    assert_eq!(n, 1);
}

#[test]
fn watch_undo_loop_fire_carries_its_spot_and_box_and_starts_over() {
    let mut w = watch(Level::Amateur);
    let mut last = Vec::new();
    for i in 0..4 {
        last = undo(&mut w, i as f64, 9, tiles(3, 3));
    }
    let Some(Output::Fire(f)) = last.first() else { panic!("no fire: {last:?}") };
    assert_eq!(f.drawing, Some(DrawingId(9)));
    assert_eq!(f.frames, FrameRange { from: 4, to: 7 });
    assert_eq!(f.evidence, Evidence::Area(tiles(3, 3).paper()));
    assert_eq!(f.delivery, Delivery::Card);
    assert_eq!(fires(&undo(&mut w, 4.0, 9, tiles(3, 3)), CheckId::UndoLoop), 0);
    assert_eq!(w.book.fires(P, CutId(2), CheckId::UndoLoop), 1);
}

// ---- tile box of a step ---------------------------------------------------

fn paint(id: u64, coords: &[(i32, i32)]) -> Command {
    let diff: TileDiff = coords.iter().map(|c| (*c, None, None)).collect();
    Command::PaintTiles { at: at(), id: DrawingId(id), layer: LayerId(1), diff }
}

#[test]
fn watch_step_box_spans_every_painted_tile_of_a_step() {
    let step = step_box(&[paint(5, &[(1, 2), (4, 3)]), paint(5, &[(0, 7)])]).unwrap();
    assert_eq!(step.tiles, TileBox { x0: 0, y0: 2, x1: 4, y1: 7 });
    assert_eq!(step.key, DrawKey::Drawing(DrawingId(5)));
}

#[test]
fn watch_step_box_keys_a_minted_drawing_by_its_cell() {
    let add = Command::AddDrawing {
        at: at(),
        id: DrawingId(5),
        index: 0,
        name: "A1".into(),
        strokes: Vec::new(),
        layers: Vec::new(),
    };
    let set = Command::SetCell {
        at: at(),
        column: ColumnId(3),
        frame: 12,
        key: Some(Exposure::Drawing(DrawingId(5))),
    };
    let step = step_box(&[add, set, paint(5, &[(1, 1)])]).unwrap();
    assert_eq!(step.key, DrawKey::Cell { column: ColumnId(3), frame: 12 });
}

#[test]
fn watch_step_box_is_silent_for_vector_and_mixed_steps() {
    let stroke = Stroke { points: Vec::new(), base_width: 1.0, color: [0; 4] };
    let add = Command::AddStroke { at: at(), id: DrawingId(5), stroke };
    assert_eq!(step_box(&[add]), None, "vector");
    assert_eq!(step_box(&[paint(5, &[])]), None, "no tiles");
    assert_eq!(step_box(&[paint(5, &[(0, 0)]), paint(6, &[(0, 0)])]), None, "two drawings");
}

// ---- over-polishing -------------------------------------------------------

/// One small stroke, then `secs` of active frames a second apart with a
/// stroke every 10 s. Returns the fires.
fn polish(w: &mut Watch, zoom: f32, secs: u32) -> usize {
    w.feed(0.0, Event::Zoom { zoom });
    w.feed(0.0, Event::Proximity { in_range: true });
    let area = PaperBox { x0: 100.0, y0: 100.0, x1: 200.0, y1: 200.0 };
    let stroke = Event::Stroke { spot: spot(9), area, erased: false, paper_area: PAPER };
    w.feed(0.0, stroke.clone());
    let mut n = 0;
    for s in 1..=secs {
        let now = s as f64;
        if s % 10 == 0 {
            w.feed(now, stroke.clone());
        }
        n += fires(&w.feed(now, Event::Frame { pen_down: false }), CheckId::OverPolish);
    }
    n
}

#[test]
fn watch_polish_fires_at_each_levels_time() {
    for (level, secs) in [(Level::Rookie, 180), (Level::Amateur, 90)] {
        assert_eq!(polish(&mut watch(level), 4.0, secs - 1), 0, "{level:?} one short");
        assert_eq!(polish(&mut watch(level), 4.0, secs), 1, "{level:?} at the time");
    }
}

#[test]
fn watch_polish_needs_four_hundred_percent() {
    assert_eq!(polish(&mut watch(Level::Amateur), 3.99, 200), 0);
}

#[test]
fn watch_polish_a_zoom_out_ends_the_run() {
    let mut w = watch(Level::Amateur);
    assert_eq!(polish(&mut w, 4.0, 80), 0);
    w.feed(80.5, Event::Zoom { zoom: 1.0 });
    w.feed(80.6, Event::Zoom { zoom: 4.0 });
    w.feed(81.0, Event::Frame { pen_down: false });
    let out = w.feed(100.0, Event::Frame { pen_down: false });
    assert_eq!(fires(&out, CheckId::OverPolish), 0);
}

#[test]
fn watch_polish_region_is_the_union_of_strokes_under_five_percent() {
    let limit = PAPER * POLISH_REGION;
    let small = PaperBox { x0: 0.0, y0: 0.0, x1: 100.0, y1: 100.0 };
    let side = (limit as f32).sqrt();
    let far = PaperBox { x0: side, y0: side, x1: side + 10.0, y1: side + 10.0 };
    let mut w = watch(Level::Amateur);
    w.feed(0.0, Event::Zoom { zoom: 4.0 });
    w.feed(0.0, Event::Proximity { in_range: true });
    let stroke = |area| Event::Stroke { spot: spot(9), area, erased: false, paper_area: PAPER };
    w.feed(0.0, stroke(small));
    let mut n = 0;
    for s in 1..=120 {
        let now = s as f64;
        // Each stroke alone is tiny; together they cover too much paper, so
        // the timer keeps starting again.
        if s % 10 == 0 {
            w.feed(now, stroke(if s % 20 == 0 { small } else { far }));
        }
        n += fires(&w.feed(now, Event::Frame { pen_down: false }), CheckId::OverPolish);
    }
    assert_eq!(n, 0);
}

#[test]
fn watch_polish_counts_only_active_time() {
    // Out of range and pen up: the clock stands still.
    let mut w = watch(Level::Amateur);
    w.feed(0.0, Event::Zoom { zoom: 4.0 });
    let area = PaperBox { x0: 0.0, y0: 0.0, x1: 50.0, y1: 50.0 };
    let stroke = Event::Stroke { spot: spot(9), area, erased: false, paper_area: PAPER };
    let mut n = 0;
    w.feed(0.0, stroke.clone());
    for s in 1..=100 {
        if s % 10 == 0 {
            w.feed(s as f64, stroke.clone());
        }
        n += fires(&w.feed(s as f64, Event::Frame { pen_down: false }), CheckId::OverPolish);
    }
    assert_eq!(n, 0);
}

#[test]
fn watch_polish_a_long_gap_is_not_continuous_work() {
    let mut w = watch(Level::Amateur);
    assert_eq!(polish(&mut w, 4.0, 80), 0);
    // 80 s, the last stroke at 80 s. A long break, then work resumes.
    let out = w.feed(80.0 + POLISH_GAP_SECS + 1.0, Event::Frame { pen_down: false });
    assert_eq!(fires(&out, CheckId::OverPolish), 0);
    let late = 200.0;
    assert_eq!(fires(&w.feed(late, Event::Frame { pen_down: false }), CheckId::OverPolish), 0);
}

#[test]
fn watch_one_frame_gap_of_hours_adds_a_clamped_second() {
    let mut w = watch(Level::Amateur);
    w.feed(0.0, Event::Focus { at: at(), drawing: Some(DrawingId(1)) });
    w.feed(1.0, Event::Frame { pen_down: true });
    w.feed(36_000.0, Event::Frame { pen_down: true });
    let secs = w.book.active(P, CutId(2), DrawingId(1));
    assert_eq!(secs, 2.0 * MAX_FRAME_SECS, "not ten hours");
}

// ---- key check ------------------------------------------------------------

fn span() -> KeySpan {
    KeySpan {
        column: ColumnId(1),
        frames: FrameRange { from: 0, to: 12 },
        from: SpanEnd { drawing: DrawingId(1), is_key: true },
        to: SpanEnd { drawing: DrawingId(5), is_key: false },
        inbetweens: vec![DrawingId(2), DrawingId(3)],
    }
}

/// `secs` seconds of pen-down time on a drawing.
fn work(w: &mut Watch, now: &mut f64, id: u64, secs: u32) {
    w.feed(*now, Event::Focus { at: at(), drawing: Some(DrawingId(id)) });
    for _ in 0..secs {
        *now += 1.0;
        w.feed(*now, Event::Frame { pen_down: true });
    }
}

fn key_fires(level: Level, key: u32, a: u32, b: u32) -> usize {
    let mut w = watch(level);
    let mut now = 0.0;
    work(&mut w, &mut now, 1, key);
    work(&mut w, &mut now, 2, a);
    work(&mut w, &mut now, 3, b);
    let out = w.feed(now, Event::Spans { at: at(), spans: vec![span()] });
    fires(&out, CheckId::KeyTime)
}

#[test]
fn watch_key_check_fires_under_each_levels_ratio() {
    // In-between mean is 20 s.
    assert_eq!(key_fires(Level::Rookie, 9, 10, 30), 1, "9 < 10");
    assert_eq!(key_fires(Level::Rookie, 10, 10, 30), 0, "exactly half is not under");
    assert_eq!(key_fires(Level::Amateur, 19, 10, 30), 1, "19 < 20");
    assert_eq!(key_fires(Level::Amateur, 20, 10, 30), 0, "exactly the mean is not under");
}

#[test]
fn watch_key_check_silent_without_recorded_time_on_both_sides() {
    assert_eq!(key_fires(Level::Amateur, 5, 0, 30), 0, "an in-between has no time");
    assert_eq!(key_fires(Level::Amateur, 0, 10, 30), 0, "the key has no time");
}

#[test]
fn watch_key_check_never_judges_a_breakdown_end() {
    // Drawing 5 is the to-end, a breakdown with little time.
    let mut w = watch(Level::Amateur);
    let mut now = 0.0;
    work(&mut w, &mut now, 1, 100);
    work(&mut w, &mut now, 2, 40);
    work(&mut w, &mut now, 3, 40);
    work(&mut w, &mut now, 5, 1);
    let out = w.feed(now, Event::Spans { at: at(), spans: vec![span()] });
    assert_eq!(fires(&out, CheckId::KeyTime), 0);
}

#[test]
fn watch_key_check_tells_each_key_once_and_names_the_drawings() {
    let mut w = watch(Level::Amateur);
    let mut now = 0.0;
    work(&mut w, &mut now, 1, 1);
    work(&mut w, &mut now, 2, 10);
    work(&mut w, &mut now, 3, 10);
    let spans = Event::Spans { at: at(), spans: vec![span()] };
    let out = w.feed(now, spans.clone());
    let Some(Output::Fire(f)) = out.first() else { panic!("no fire") };
    let ids = vec![DrawingId(1), DrawingId(2), DrawingId(3)];
    assert_eq!(f.evidence, Evidence::Drawings(ids));
    assert_eq!(f.frames, FrameRange { from: 0, to: 12 });
    assert!(w.feed(now, spans).is_empty(), "told once");
}

#[test]
fn watch_active_time_waits_for_the_first_stroke_on_an_empty_cell() {
    let mut w = watch(Level::Amateur);
    w.feed(0.0, Event::Focus { at: at(), drawing: None });
    w.feed(0.5, Event::Proximity { in_range: true });
    w.feed(1.0, Event::Frame { pen_down: false });
    w.feed(2.0, Event::Frame { pen_down: false });
    let area = PaperBox { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 };
    let stroke = Event::Stroke { spot: spot(7), area, erased: false, paper_area: PAPER };
    w.feed(3.0, stroke);
    assert_eq!(w.book.active(P, CutId(2), DrawingId(7)), 2.0);
}

#[test]
fn watch_active_time_needs_the_pen_down_or_in_range() {
    let mut w = watch(Level::Amateur);
    w.feed(0.0, Event::Focus { at: at(), drawing: Some(DrawingId(1)) });
    w.feed(1.0, Event::Frame { pen_down: false });
    assert_eq!(w.book.active(P, CutId(2), DrawingId(1)), 0.0);
}

// ---- key spans ------------------------------------------------------------

fn sheet(marks: &[(u64, DrawingKind)]) -> Cut {
    let mut cut = Cut::new(CutId(2), "C1", 48);
    let mut column = Column::new(ColumnId(1), "A");
    for (i, (id, kind)) in marks.iter().enumerate() {
        let mut d = Drawing::new(DrawingId(*id), format!("A{id}"));
        d.strokes.push(Stroke { points: Vec::new(), base_width: 1.0, color: [0; 4] });
        cut.drawings.push(d);
        column.set_key(i as u32 * 2, Exposure::Drawing(DrawingId(*id)));
        cut.set_drawing_def(DrawingId(*id), DrawingDef { kind: *kind, ..DrawingDef::default() });
    }
    cut.xsheet.columns.push(column);
    cut
}

use DrawingKind::{Breakdown, Inbetween, Key, Unset};

#[test]
fn watch_spans_read_keys_and_in_betweens_from_the_sheet() {
    let cut = sheet(&[(1, Key), (2, Inbetween), (3, Inbetween), (4, Key)]);
    let spans = key_spans(&cut);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].from, SpanEnd { drawing: DrawingId(1), is_key: true });
    assert_eq!(spans[0].to, SpanEnd { drawing: DrawingId(4), is_key: true });
    assert_eq!(spans[0].inbetweens, vec![DrawingId(2), DrawingId(3)]);
    assert_eq!(spans[0].frames, FrameRange { from: 0, to: 6 });
}

#[test]
fn watch_spans_breakdown_bounds_a_span_without_being_a_key() {
    let cut = sheet(&[(1, Key), (2, Inbetween), (3, Breakdown), (4, Inbetween), (5, Key)]);
    let spans = key_spans(&cut);
    assert_eq!(spans.len(), 2);
    assert!(!spans[0].to.is_key);
    assert!(!spans[1].from.is_key);
}

#[test]
fn watch_spans_an_unmarked_drawing_silences_its_span() {
    let cut = sheet(&[(1, Key), (2, Inbetween), (3, Unset), (4, Key)]);
    assert!(key_spans(&cut).is_empty());
}

#[test]
fn watch_spans_an_in_between_without_content_silences_its_span() {
    let mut cut = sheet(&[(1, Key), (2, Inbetween), (3, Key)]);
    cut.drawings[1].strokes.clear();
    assert!(key_spans(&cut).is_empty());
}

#[test]
fn watch_spans_an_empty_cell_breaks_the_chain() {
    let mut cut = sheet(&[(1, Key), (2, Inbetween), (3, Key)]);
    cut.xsheet.columns[0].set_key(1, Exposure::Empty);
    assert!(key_spans(&cut).is_empty());
}

#[test]
fn watch_spans_a_charts_target_counts_as_a_key_even_unmarked() {
    let mut cut = sheet(&[(1, Key), (2, Inbetween), (3, Unset)]);
    let chart = SpacingChart { to: DrawingId(3), marks: vec![0.5] };
    let def = DrawingDef { kind: Key, chart: Some(chart), ..DrawingDef::default() };
    cut.set_drawing_def(DrawingId(1), def);
    let spans = key_spans(&cut);
    assert_eq!(spans.len(), 1);
    assert!(spans[0].to.is_key);
}

// ---- retirement -----------------------------------------------------------

/// Finish cut `n` with `fires` undo-loop fires on it.
fn finish(w: &mut Watch, n: u64, fires: u32) -> Vec<Output> {
    let cut = CutRef { scene: SceneId(1), cut: CutId(n) };
    w.feed(0.0, Event::Focus { at: cut, drawing: None });
    let mut s = spot(9);
    s.at = cut;
    let count = w.book.level.thresholds().undo_count;
    let mut now = 0.0;
    for _ in 0..fires {
        for _ in 0..count {
            now += 1.0;
            w.feed(now, Event::UndoOrErase { spot: s, tiles: tiles(3, 3) });
        }
    }
    w.feed(now, Event::CutFinished { at: cut })
}

fn notices(out: &[Output]) -> Vec<Notice> {
    out.iter()
        .filter_map(|o| match o {
            Output::Notice(n) => Some(*n),
            _ => None,
        })
        .collect()
}

#[test]
fn watch_retires_exactly_at_the_streak_with_one_notice() {
    let mut w = watch(Level::Rookie);
    assert!(notices(&finish(&mut w, 1, 0)).is_empty());
    assert_eq!(w.book.streak(CheckId::UndoLoop), 1);
    assert!(notices(&finish(&mut w, 2, 0)).is_empty());
    assert_eq!(w.book.state(CheckId::UndoLoop), CheckState::On);
    let told = notices(&finish(&mut w, 3, 0));
    assert_eq!(w.book.state(CheckId::UndoLoop), CheckState::Retired);
    assert!(told.contains(&Notice::Retired { check: CheckId::UndoLoop, shots: 3 }));
}

#[test]
fn watch_streak_counts_the_allowance_and_resets_over_it() {
    let mut w = watch(Level::Rookie);
    finish(&mut w, 1, 2);
    assert_eq!(w.book.streak(CheckId::UndoLoop), 1, "2 fires is at the allowance");
    finish(&mut w, 2, 3);
    assert_eq!(w.book.streak(CheckId::UndoLoop), 0, "3 fires is over it");

    let mut w = watch(Level::Amateur);
    finish(&mut w, 1, 1);
    assert_eq!(w.book.streak(CheckId::UndoLoop), 1);
    finish(&mut w, 2, 2);
    assert_eq!(w.book.streak(CheckId::UndoLoop), 0);
}

#[test]
fn watch_a_cut_counts_once_however_often_it_finishes() {
    let mut w = watch(Level::Rookie);
    finish(&mut w, 1, 0);
    finish(&mut w, 1, 0);
    let again = w.feed(1.0, Event::CutFinished { at: CutRef { scene: SceneId(1), cut: CutId(1) } });
    assert!(again.is_empty());
    assert_eq!(w.book.streak(CheckId::UndoLoop), 1);
}

#[test]
fn watch_retired_check_gives_a_dot_never_a_card() {
    let mut w = watch(Level::Amateur);
    for n in 1..=3 {
        finish(&mut w, n, 0);
    }
    assert_eq!(w.book.state(CheckId::UndoLoop), CheckState::Retired);
    let mut last = Vec::new();
    for i in 0..4 {
        last = undo(&mut w, 100.0 + i as f64, 9, tiles(3, 3));
    }
    let Some(Output::Fire(f)) = last.first() else { panic!("no dot") };
    assert_eq!(f.delivery, Delivery::Dot);

    w.book.dots = false;
    for i in 0..4 {
        assert!(undo(&mut w, 200.0 + i as f64, 9, tiles(3, 3)).is_empty());
    }
}

#[test]
fn watch_my_style_silences_a_check_and_survives_saving() {
    let mut w = watch(Level::Amateur);
    w.book.my_style(CheckId::UndoLoop);
    let json = serde_json::to_string(&w.book).unwrap();
    let mut w = Watch::new(serde_json::from_str(&json).unwrap());
    w.open_project(P);
    assert_eq!(w.book.state(CheckId::UndoLoop), CheckState::MyStyle);
    assert_eq!(undo_run(&mut w, 8), 0);
    finish(&mut w, 1, 0);
    assert_eq!(w.book.streak(CheckId::UndoLoop), 0, "my style keeps no streak");
}

#[test]
fn watch_book_round_trips_times_fires_and_notices() {
    let mut w = watch(Level::Amateur);
    w.feed(0.0, Event::Focus { at: at(), drawing: Some(DrawingId(1)) });
    w.feed(1.0, Event::Frame { pen_down: true });
    undo_run(&mut w, 4);
    finish(&mut w, 2, 0);
    let json = serde_json::to_string(&w.book).unwrap();
    let back: Book = serde_json::from_str(&json).unwrap();
    assert_eq!(back, w.book);
    let empty: Book = serde_json::from_str("{}").unwrap();
    assert_eq!(empty, Book::default(), "an older file loads with defaults");
    assert!(empty.enabled && empty.dots);
}

#[test]
fn watch_every_check_off_gives_nothing_and_counts_nothing() {
    let mut w = watch(Level::Amateur);
    w.book.enabled = false;
    assert_eq!(undo_run(&mut w, 8), 0);
    assert_eq!(polish(&mut w, 4.0, 200), 0);
    assert!(w.feed(1.0, Event::CutFinished { at: at() }).is_empty());
    assert_eq!(w.book, {
        let mut b = Book::default();
        b.set_level(Level::Amateur);
        b.enabled = false;
        b
    });
}

#[test]
fn watch_level_up_is_suggested_once_when_all_are_retired() {
    let mut w = watch(Level::Rookie);
    let mut told = Vec::new();
    for n in 1..=5 {
        told.extend(notices(&finish(&mut w, n, 0)));
    }
    let level_ups = told.iter().filter(|n| matches!(n, Notice::NextLevel { .. })).count();
    assert_eq!(level_ups, 1);
    assert!(told.contains(&Notice::NextLevel { next: Level::Amateur }));
    assert_eq!(w.book.level, Level::Rookie, "the level never changes by itself");
}

#[test]
fn watch_top_level_suggests_nothing() {
    let mut w = watch(Level::Amateur);
    let told: Vec<Notice> = (1..=3).flat_map(|n| notices(&finish(&mut w, n, 0))).collect();
    assert_eq!(told.len(), 3, "three retirements only: {told:?}");
}

#[test]
fn watch_a_check_turned_back_on_fires_and_is_not_told_twice() {
    let mut w = watch(Level::Amateur);
    for n in 1..=3 {
        finish(&mut w, n, 0);
    }
    w.book.turn_on(CheckId::UndoLoop);
    assert_eq!(w.book.streak(CheckId::UndoLoop), 0);
    assert_eq!(undo_run(&mut w, 4), 1);
    let told: Vec<Notice> = (4..=6).flat_map(|n| notices(&finish(&mut w, n, 0))).collect();
    assert_eq!(w.book.state(CheckId::UndoLoop), CheckState::Retired);
    assert!(told.is_empty(), "the card was shown once: {told:?}");
}

#[test]
fn watch_a_new_level_brings_retired_checks_back() {
    let mut w = watch(Level::Rookie);
    w.book.my_style(CheckId::OverPolish);
    for n in 1..=3 {
        finish(&mut w, n, 0);
    }
    w.book.set_level(Level::Amateur);
    assert_eq!(w.book.state(CheckId::UndoLoop), CheckState::On);
    assert_eq!(w.book.streak(CheckId::UndoLoop), 0);
    assert_eq!(w.book.state(CheckId::OverPolish), CheckState::MyStyle);
}

#[test]
fn watch_a_check_set_aside_does_not_hold_back_the_next_level() {
    let mut w = watch(Level::Rookie);
    w.book.my_style(CheckId::OverPolish);
    let told: Vec<Notice> = (1..=3).flat_map(|n| notices(&finish(&mut w, n, 0))).collect();
    assert!(told.contains(&Notice::NextLevel { next: Level::Amateur }));
}

// ---- the document ---------------------------------------------------------

#[test]
fn watch_a_cut_change_resets_the_runs_but_not_the_book() {
    let mut w = watch(Level::Amateur);
    w.feed(0.0, Event::Focus { at: at(), drawing: Some(DrawingId(9)) });
    assert_eq!(undo_run(&mut w, 3), 0);
    w.feed(3.0, Event::Focus { at: other_cut(), drawing: Some(DrawingId(9)) });
    w.feed(3.5, Event::Focus { at: at(), drawing: Some(DrawingId(9)) });
    assert_eq!(fires(&undo(&mut w, 4.0, 9, tiles(3, 3)), CheckId::UndoLoop), 0);
}

#[test]
fn watch_a_document_swap_clears_memory_and_keeps_saved_counters() {
    let mut w = watch(Level::Amateur);
    undo_run(&mut w, 4);
    assert_eq!(undo_run(&mut w, 3), 0);
    w.open_project("C:/shots/b.anim");
    assert_eq!(fires(&undo(&mut w, 10.0, 9, tiles(3, 3)), CheckId::UndoLoop), 0);
    assert_eq!(w.book.fires("C:/shots/b.anim", CutId(2), CheckId::UndoLoop), 0);
    assert_eq!(w.book.fires(P, CutId(2), CheckId::UndoLoop), 1, "same ids, another project");
}

#[test]
fn watch_an_unsaved_project_keeps_counters_until_its_first_save() {
    let mut w = watch(Level::Amateur);
    w.open_project("");
    undo_run(&mut w, 4);
    assert_eq!(w.book.fires("", CutId(2), CheckId::UndoLoop), 1);
    w.project_saved("C:/shots/new.anim");
    assert_eq!(w.book.fires("", CutId(2), CheckId::UndoLoop), 0);
    assert_eq!(w.book.fires("C:/shots/new.anim", CutId(2), CheckId::UndoLoop), 1);

    w.project_saved("C:/shots/copy.anim");
    assert_eq!(w.book.fires("C:/shots/copy.anim", CutId(2), CheckId::UndoLoop), 0, "afresh");

    w.open_project("");
    undo_run(&mut w, 4);
    w.open_project("");
    assert_eq!(w.book.fires("", CutId(2), CheckId::UndoLoop), 0, "never saved, gone");
}
