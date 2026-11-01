//! AU27: the lesson frame: a probe lesson over the real editor. No lesson ships
//! here: `PROBE` is this file's, and `lessons::table()` reads `TABLE`.

use std::sync::{Arc, LazyLock};

use anim_core::model::{DrawingKind, Stroke, StrokePoint};
use anim_core::raster::{TILE, TILE_LEN, TileData};
use eframe::egui::{Pos2, pos2};

use crate::aurail_tests::window;
use crate::canvas::CanvasTool;
use crate::config::Action;
use crate::lesson::{self, LEAVE, SHOW_ME};
use crate::lessons::{self, Class, Done, Lesson, Path, PathKind, Show, Target, Task};
use crate::say::{self, Facts};
use crate::steps::ProjectKind;
use crate::templates::{self, Template};
use crate::testkit::EditorHarness;
use crate::welcome::{Welcome, WelcomeAction, WelcomeTab};
use crate::workspace::Stage;
use crate::{PROBE_SCALES, StartOutcome, measure, plate, steps};

fn settle(h: &mut EditorHarness) {
    h.run_frames(4);
}

const SIZES: [(f32, f32); 4] = [(1600.0, 900.0), (1280.0, 800.0), (1280.0, 720.0), (1024.0, 640.0)];
const WORST: (f32, f32) = (832.0, 520.0);

const FOUR_THREE: Template = Template { kind: Some(ProjectKind::Drawing), width: 1200, height: 900, ..templates::READY };
const SQUARE: Template = Template { kind: Some(ProjectKind::Drawing), width: 1000, height: 1000, ..templates::READY };
const FILM: Template = Template { kind: Some(ProjectKind::Animation), width: 1200, height: 900, ..templates::READY };

fn line(a: [f32; 2], b: [f32; 2], kind: PathKind) -> Path {
    let at = |t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, 1.0];
    Path { points: (0..=8).map(|i| at(i as f32 / 8.0)).collect(), width: 4.0, kind, over: false }
}

/// A long line, a dab, and the two lines one stroke cannot answer together.
fn probe_art() -> Vec<Path> {
    vec![
        line([200.0, 200.0], [1000.0, 200.0], PathKind::Finish),
        line([590.0, 590.0], [610.0, 610.0], PathKind::Landmark),
        line([200.0, 700.0], [500.0, 700.0], PathKind::Guide),
        line([540.0, 700.0], [840.0, 700.0], PathKind::Guide),
        line([200.0, 400.0], [400.0, 500.0], PathKind::Guide),
    ]
}

const fn task(say: &'static str, why: Option<&'static str>, step: Stage, done: Done, show: Show) -> Task {
    Task { say, why, step, done, ring: &[Target::Paper], guide: &[], stand_in: &[], miss: None, show }
}

const TASKS: &[Task] = &[
    Task { guide: &[0], stand_in: &[0], ..task("{Draw} one long line.", None, Stage::Drawing, Done::Strokes(1), Show::Trace(&[0])) },
    Task {
        ring: &[Target::Step(Stage::InkPaint)],
        ..task("Press {Color}.", Some("Paint goes under the line."), Stage::InkPaint, Done::OnStep(Stage::InkPaint), Show::Perform(Action::RoomFinishing))
    },
    Task {
        ring: &[Target::Step(Stage::Composite)],
        ..task("Press {Effects}.", Some("Light goes on last."), Stage::Composite, Done::OnStep(Stage::Composite), Show::Perform(Action::RoomComposite))
    },
    Task {
        ring: &[Target::Step(Stage::Export)],
        ..task("Press {Export}.", Some("A file is what you keep."), Stage::Export, Done::OnStep(Stage::Export), Show::Perform(Action::RoomExport))
    },
    task("Save it as a picture.", None, Stage::Export, Done::Exported(1), Show::Perform(Action::ExportStill)),
];

const PROBE: Lesson = Lesson {
    id: "probe",
    kind: ProjectKind::Drawing,
    class: Class::Rookie,
    title: "Probe",
    hover: "A line, a color, an effect and a file. About 2 minutes. Ends in a PNG.",
    paper: &FOUR_THREE,
    art: probe_art,
    finish: &[0],
    tasks: TASKS,
    closing: "You know the four steps a picture walks.",
    ..Lesson::BLANK
};

fn page_paper() -> &'static Template {
    templates::ALL.iter().find(|t| t.page.is_some()).expect("a page template")
}

/// The probe, then the papers and classes the frame is read on. Only the first
/// is a lesson the table rules read.
pub static TABLE: LazyLock<Vec<Lesson>> = LazyLock::new(|| {
    let mut rows = lessons::ALL.to_vec();
    rows.push(PROBE);
    rows.push(Lesson { id: "probe-square", class: Class::Amateur, title: "Square", paper: &SQUARE, ..PROBE });
    rows.push(Lesson { id: "probe-last", class: Class::Experienced, title: "Last", paper: &SQUARE, ..PROBE });
    rows.push(Lesson { id: "probe-film", kind: ProjectKind::Animation, title: "Film", paper: &FILM, ..PROBE });
    let page = page_paper();
    rows.push(Lesson { id: "probe-page", kind: ProjectKind::Comic, title: "Page", paper: page, ..PROBE });
    rows
});

fn row(id: &str) -> usize {
    TABLE.iter().position(|l| l.id == id).expect("a row of the table")
}

/// `id` opened the way a page card opens it.
fn open(h: &mut EditorHarness, id: &str) {
    h.answer_start(WelcomeAction::Lesson(row(id)));
    settle(h);
}

fn opened(id: &str) -> EditorHarness {
    let mut h = EditorHarness::as_shipped();
    open(&mut h, id);
    h
}

fn at(h: &EditorHarness) -> usize {
    h.editor().lesson.as_ref().expect("a lesson stands").task
}

fn on_screen(h: &EditorHarness, p: [f32; 2]) -> Pos2 {
    h.editor().canvas.view_xform().paper_to_screen(pos2(p[0], p[1]))
}

/// A stroke by the pen, through `path` on the paper.
fn pen(h: &mut EditorHarness, path: &[[f32; 2]]) {
    let pts: Vec<Pos2> = path.iter().map(|p| on_screen(h, *p)).collect();
    h.pen_down(pts[0]);
    pts[1..].iter().for_each(|p| h.pen_move(*p));
    h.pen_up(pts[pts.len() - 1]);
    settle(h);
}

fn along(a: [f32; 2], b: [f32; 2]) -> Vec<[f32; 2]> {
    (0..=6).map(|i| { let t = i as f32 / 6.0; [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t] }).collect()
}

fn stroke() -> Stroke {
    let pt = |x: f32| StrokePoint { x, y: 40.0, pressure: 0.5, tilt: [0.0, 0.0] };
    Stroke { points: vec![pt(10.0), pt(40.0)], base_width: 2.0, color: [0, 0, 0, 255] }
}

/// Texels on the armed layer, `rgba` in half-float bits.
fn texels(h: &mut EditorHarness, cells: &[(i32, i32)], alpha: u16) {
    let state = h.state_mut();
    let t = TILE as i32;
    let mut map: std::collections::BTreeMap<(i32, i32), Vec<u16>> = Default::default();
    if let Some(have) = state.active_layer_tiles() {
        for (c, tile) in have {
            map.insert(*c, tile.rgba.to_vec());
        }
    }
    for (x, y) in cells {
        let tile = map.entry((x.div_euclid(t), y.div_euclid(t))).or_insert_with(|| vec![0; TILE_LEN]);
        let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4) as usize;
        tile[i + 3] = alpha;
        tile[i] = alpha;
    }
    let slot = state.active_slot_resolved().expect("an armed layer");
    let tiles = map.into_iter().map(|(k, v)| (k, Arc::new(TileData::from_vec(v)))).collect();
    state.commit_raster(tiles, slot).expect("committed");
}

const NEVER: Task = task("", None, Stage::Drawing, Done::Strokes(u32::MAX), Show::Where);

/// A run whose first task is `done`: it does not hold, then `act` makes it.
fn reads(h: &mut EditorHarness, done: Done, act: impl FnOnce(&mut EditorHarness)) {
    let first = Task { done, step: h.editor().stage.unwrap_or(Stage::Drawing), ..NEVER };
    let tasks: &'static [Task] = Box::leak(Box::new([first, NEVER]));
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks, ..PROBE }, 0);
    settle(h);
    assert_eq!(at(h), 0, "{done:?} held before its act");
    act(h);
    settle(h);
    assert_eq!(at(h), 1, "{done:?} did not read its act");
}

/// Spec 4.5, on a table with lessons: one click.
#[test]
fn aulesson_page_is_one_click() {
    let mut h = EditorHarness::as_first_run();
    let mut w = Welcome::new(&[]);
    w.lessons = lessons::table();
    h.editor_mut().start = Some(w);
    settle(&mut h);
    assert!(!h.has(PROBE.title), "still on Start");
    h.click("Follow a lesson");
    settle(&mut h);
    assert!(h.has(PROBE.title), "{}", h.describe());
    assert!(h.editor().start.as_ref().is_some_and(|w| w.tab == WelcomeTab::Lessons));
}

/// Spec 5.3: opening goes through the one seam, and the Leave guard before it.
#[test]
fn aulesson_open_goes_through_the_leave_guard() {
    let mut h = EditorHarness::as_shipped();
    h.state_mut().commit_stroke(stroke());
    settle(&mut h);
    let ctx = h.ctx();
    h.editor_mut().choose_start(WelcomeAction::Lesson(row("probe")), &ctx);
    settle(&mut h);
    assert!(h.editor().leave_pending(), "unsaved work is asked about");
    assert!(h.editor().lesson.is_none(), "and nothing has opened");
    assert!(matches!(
        crate::apply_start(WelcomeAction::Lesson(999), None, None, &Default::default()),
        StartOutcome::Stay
    ));
}

/// Opening over a blank asks nothing; a lesson never made dirty never asks.
#[test]
fn aulesson_untouched_lesson_asks_nothing() {
    let mut h = EditorHarness::as_shipped();
    let ctx = h.ctx();
    h.editor_mut().choose_start(WelcomeAction::Lesson(row("probe")), &ctx);
    settle(&mut h);
    assert!(!h.editor().leave_pending(), "a blank asks nothing");
    let mut h = opened("probe");
    assert!(!h.state().is_dirty(), "app.lesson is no edit");
    h.click(LEAVE);
    settle(&mut h);
    assert!(!h.state().is_dirty(), "leaving a paper with no file sets nothing");
    assert!(h.editor().lesson.is_none());
    let ctx = h.ctx();
    h.editor_mut().choose_start(WelcomeAction::Lesson(row("probe")), &ctx);
    settle(&mut h);
    assert!(!h.editor().leave_pending());
}

/// Spec 5.3's first left-over.
#[test]
fn aulesson_shows_hidden_tools() {
    let mut h = EditorHarness::as_shipped();
    h.editor_mut().tools_hidden = true;
    h.editor_mut().begin_lesson(PROBE, 0);
    settle(&mut h);
    assert!(!h.editor().tools_hidden);
    assert!(h.has(LEAVE), "the card stands with the tools");
}

/// Spec 3.10: the sheet shows for the run, and the memory stands untouched.
#[test]
fn aulesson_ignores_a_hidden_sheet() {
    let mut h = EditorHarness::as_animation();
    h.hide_sheet();
    assert!(h.editor().plan_sheet_hidden);
    open(&mut h, "probe-film");
    assert!(h.editor().plan_sheet_hidden, "a boot reads the memory as it stood");
    assert!(h.editor().in_plan() && h.editor().sheet_on_screen(), "{}", h.describe());
    h.click(LEAVE);
    settle(&mut h);
    assert!(h.editor().plan_sheet_hidden, "Leave leaves the stored value");
    assert!(h.rig().config.ui.plan_sheet_hidden());
}

/// Spec 5.6: 4:3, square and page, every step on the bar, four sizes by three
/// scales.
#[test]
fn aulesson_card_covers_no_paper() {
    for id in ["probe", "probe-square", "probe-film", "probe-page"] {
        let mut h = opened(id);
        let lesson = TABLE[row(id)];
        for bar in steps::bar(lesson.kind) {
            h.editor_mut().enter_room(bar.stage);
            for (w, tall) in SIZES {
                for scale in PROBE_SCALES {
                    window(&mut h, w, tall, scale);
                    let share = measure::paper_share(&h);
                    assert_eq!(share.paper_under_chrome, 0, "{id} {} {w}x{tall} {scale}", bar.label);
                }
            }
        }
    }
}

/// The sentences the lessons will speak, tokens in words.
fn file_sentences() -> Vec<String> {
    let file = include_str!("../../research/LESSONS-approachable-ui.md");
    let row = |l: &&str| l.starts_with("| ") && l[2..].starts_with(|c: char| c.is_ascii_digit());
    let cell = |l: &str| l.split(" | ").nth(1).unwrap_or("").to_string();
    let spell = |s: String| lesson::resolve(&s, |t| Some(lesson::word(t).unwrap_or("Ctrl+Shift+Z").to_string()));
    file.lines().filter(row).map(cell).map(spell).collect()
}

/// Spec 5.6: at the worst size each task's sentence lies on the card, its
/// reason a scroll away, and neither key leaves the view.
#[test]
fn aulesson_card_fits_every_sentence() {
    let mut sentences = file_sentences();
    assert!(sentences.len() > 60, "the lessons file's tables: {}", sentences.len());
    sentences.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut h = opened("probe");
    window(&mut h, WORST.0, WORST.1, 1.0);
    let ctx = h.ctx();
    assert_eq!(lesson::card_width(ctx.content_rect().width()), 180.0);
    let room = h.rect(SHOW_ME).top() - ctx.content_rect().top();
    let width = 180.0 - 24.0;
    for text in &sentences {
        let (say, why) = text.split_once("*Why:").unwrap_or((text, ""));
        let tall = ctx.fonts_mut(|f| f.layout(say.to_string(), plate::Role::Heading.font(), plate::INK, width)).size();
        assert!(tall.x <= width + 0.5, "{say}: {tall:?}");
        assert!(tall.y <= room, "{say}: {tall:?} in {room}");
        let why = ctx.fonts_mut(|f| f.layout(why.to_string(), plate::Role::Panel.font(), plate::INK, width)).size();
        assert!(why.x <= width + 0.5, "{text}");
    }
    let longest: &'static str = Box::leak(sentences[0].clone().into_boxed_str());
    let tasks: &'static [Task] = Box::leak(Box::new([Task { say: longest, show: Show::Perform(Action::Undo), ..NEVER }]));
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks, ..PROBE }, 0);
    settle(&mut h);
    let view = h.ctx().content_rect();
    for key in [SHOW_ME, LEAVE] {
        assert!(view.contains_rect(h.rect(key)), "{key}: {:?} in {view:?}", h.rect(key));
    }
}

/// Spec 5.4, each reader, both ways: `reads` proves it false, then true.
#[test]
fn aulesson_done_reads_the_state() {
    let mut h = opened("probe");
    reads(&mut h, Done::OnStep(Stage::InkPaint), |h| h.editor_mut().enter_room(Stage::InkPaint));
    h.editor_mut().enter_room(Stage::Drawing);
    settle(&mut h);
    reads(&mut h, Done::Strokes(1), |h| h.state_mut().commit_stroke(stroke()));
    reads(&mut h, Done::BrushChanged, |h| h.editor_mut().canvas.brush_width += 3.0);
    reads(&mut h, Done::Picked([1, 2, 3]), |h| h.editor_mut().canvas.brush_color = [1, 2, 3, 255]);
    reads(&mut h, Done::Smoothing, |h| h.rig_mut().config.pen.stabilizer += 10);
    reads(&mut h, Done::Tool(CanvasTool::Select), |h| h.editor_mut().canvas.tool = CanvasTool::Select);
    h.editor_mut().canvas.tool = CanvasTool::Paint;
    reads(&mut h, Done::Armed("Clean"), |h| h.editor_mut().canvas.armed_preset = Some("Clean".into()));
    reads(&mut h, Done::Locked(true), |h| h.editor_mut().canvas.alpha_lock = true);
    reads(&mut h, Done::Frame(3, 4), |h| h.state_mut().view.frame = 3);
    h.state_mut().view.frame = 0;
    reads(&mut h, Done::Undone, |h| {
        h.state_mut().commit_stroke(stroke());
        settle(h);
        h.perform(Action::Undo);
    });
    let more = h.state().cut().drawings.len() as u32 + 1;
    reads(&mut h, Done::Drawings(more), |h| {
        h.state_mut().commit_stroke(stroke());
        h.perform(Action::AddDrawingAfter);
    });
    h.state_mut().view.frame = 0;
    settle(&mut h);
    reads(&mut h, Done::Kinds(1), |h| {
        let id = h.state().cut().drawings[0].id;
        h.state_mut().set_drawing_kind(id, DrawingKind::Key);
    });
    reads(&mut h, Done::Exported(1), |h| h.editor_mut().lesson.as_mut().unwrap().exported += 1);
    reads(&mut h, Done::Mirrored, |h| {
        h.perform(Action::MirrorView);
        settle(h);
        h.perform(Action::MirrorView);
    });
    reads(&mut h, Done::Moved, |h| h.editor_mut().canvas.moves_done += 1);
    reads(&mut h, Done::PanelPicked(2), |h| h.state_mut().manga.selected = 2);
    reads(&mut h, Done::Gutters, |h| h.state_mut().manga.side_mm += 1.0);
    // Back off the top of the stabilizer's range: a level at the cap cannot rise.
    h.rig_mut().config.pen.stabilizer = 0;
    settle(&mut h);
    let list: &'static [Done] = &[Done::Smoothing, Done::Strokes(1)];
    reads(&mut h, Done::All(list), |h| {
        h.rig_mut().config.pen.stabilizer += 10;
        h.state_mut().commit_stroke(stroke());
    });
    // The pixels: an erasure and a fill read the armed layer's tiles.
    let smudge: Vec<(i32, i32)> = (0..20).flat_map(|i| [(590 + i, 590 + i), (591 + i, 590 + i)]).collect();
    texels(&mut h, &smudge, 0x3C00);
    settle(&mut h);
    reads(&mut h, Done::Erased(1), |h| {
        let wiped = smudge.clone();
        texels(h, &wiped, 0);
    });
}

/// Spec 5.4: the misses: each landed an act, none landed the outcome.
#[test]
fn aulesson_misses_do_not_tick() {
    let mut h = opened("probe");
    let task = |done| Task { done, step: Stage::Drawing, ..NEVER };
    let run = |h: &mut EditorHarness, done| {
        let tasks: &'static [Task] = Box::leak(Box::new([task(done), NEVER]));
        h.editor_mut().lesson = None;
        h.editor_mut().begin_lesson(Lesson { tasks, ..PROBE }, 0);
        settle(h);
    };
    // Play pressed and stopped at once.
    run(&mut h, Done::Played);
    h.state_mut().view.playing = true;
    settle(&mut h);
    h.state_mut().view.playing = false;
    settle(&mut h);
    assert_eq!(at(&h), 0, "Play pressed and stopped");
    // A stroke beside the smudge.
    let smudge: Vec<(i32, i32)> = (0..20).map(|i| (590 + i, 590 + i)).collect();
    texels(&mut h, &smudge, 0x3C00);
    run(&mut h, Done::Erased(1));
    texels(&mut h, &[(100, 100)], 0);
    settle(&mut h);
    assert_eq!(at(&h), 0, "a wipe beside the smudge");
    // One stroke across the two listed paths.
    run(&mut h, Done::Ink(&[2, 3]));
    pen(&mut h, &along([200.0, 700.0], [840.0, 700.0]));
    assert_eq!(at(&h), 0, "one stroke across both paths");
    // A cancelled export: nothing landed.
    run(&mut h, Done::Exported(1));
    let ctx = h.ctx();
    h.editor_mut().invoke(Action::ExportStill, &ctx);
    settle(&mut h);
    assert_eq!(at(&h), 0, "a dialog that was never answered");
    // And the route that does answer each: a dab is not an outline.
    run(&mut h, Done::Ink(&[0]));
    pen(&mut h, &along([590.0, 590.0], [610.0, 610.0]));
    assert_eq!(at(&h), 0, "a dab cannot tick an outline");
    pen(&mut h, &along([210.0, 205.0], [990.0, 205.0]));
    assert_eq!(at(&h), 1, "its own stroke does");
    run(&mut h, Done::Ink(&[2, 3]));
    pen(&mut h, &along([210.0, 700.0], [490.0, 700.0]));
    assert_eq!(at(&h), 0, "one of two");
    pen(&mut h, &along([550.0, 700.0], [830.0, 700.0]));
    assert_eq!(at(&h), 1, "each by its own stroke");
    // A pass that reaches the end ticks, however it was started.
    run(&mut h, Done::Played);
    h.state_mut().view.playing = true;
    settle(&mut h);
    let end = h.state().play_range().1;
    assert_eq!(end, 0, "one frame has no pass to watch");
}

/// Spec 5.4: a route the lesson did not name ticks, and a learner ahead sails
/// through. A tick is not taken back by Undo.
#[test]
fn aulesson_ahead_sails_through() {
    let mut h = opened("probe");
    assert_eq!(at(&h), 0);
    // Straight to Export, by the menu's verb, with a line drawn by the pen.
    h.editor_mut().enter_room(Stage::InkPaint);
    settle(&mut h);
    assert_eq!(at(&h), 0, "Draw's task waits for its line");
    h.editor_mut().enter_room(Stage::Drawing);
    settle(&mut h);
    pen(&mut h, &along([300.0, 300.0], [500.0, 300.0]));
    assert_eq!(at(&h), 1);
    h.perform(Action::Undo);
    settle(&mut h);
    assert_eq!(at(&h), 1, "ticks do not un-tick");
    let tasks: &'static [Task] = Box::leak(Box::new([
        task("", None, Stage::Drawing, Done::Strokes(0), Show::Where),
        task("", None, Stage::Drawing, Done::OnStep(Stage::Drawing), Show::Where),
        NEVER,
    ]));
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks, ..PROBE }, 0);
    settle(&mut h);
    assert_eq!(at(&h), 2, "two tasks already true, in one frame");
}

/// Spec 5.10: nothing is locked, and the top line points back.
#[test]
fn aulesson_wander_points_back() {
    let mut h = opened("probe");
    assert!(!h.painted_text().iter().any(|t| t.starts_with("Go back to")));
    h.editor_mut().enter_room(Stage::Composite);
    settle(&mut h);
    let word = format!("Go back to {}", crate::vocab::STEP_DRAW);
    assert!(h.painted_text().iter().any(|t| *t == word), "{:?}", h.painted_text());
    h.editor_mut().enter_room(Stage::Drawing);
    settle(&mut h);
    assert!(!h.painted_text().iter().any(|t| t.starts_with("Go back to")));
}

/// Spec 5.10: tools work during a lesson.
#[test]
fn aulesson_tools_stay_live() {
    let mut h = opened("probe");
    let before = h.state().strokes_done;
    h.perform(Action::RoomComposite);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Composite), "no pill is locked");
    h.perform(Action::RoomDrawing);
    settle(&mut h);
    pen(&mut h, &along([300.0, 300.0], [500.0, 300.0]));
    assert_eq!(h.state().strokes_done, before + 1, "the pen draws");
}

/// Spec 5.12: the lane keeps refusals and the pen's row; the chain and the
/// dots do not speak.
#[test]
fn aulesson_say_keeps_refusals() {
    let f = Facts { lesson: true, refused_twice: Some("refused".into()), ..Default::default() };
    let spoke = say::read(&f, &mut 0, None, |_| None).expect("the refusal row");
    assert_eq!(spoke.bit, Some(say::REFUSED_TWICE));
    let f = Facts { lesson: true, flat_pen: true, ..Default::default() };
    let spoke = say::read(&f, &mut 0, None, |_| None).expect("the pen row");
    assert_eq!(spoke.bit, Some(say::FLAT_PEN));
    let free = Facts { stage: Some(Stage::Drawing), ..Default::default() };
    assert!(say::read(&free, &mut 0, None, |_| None).is_some() || say::empty_step(&free, false).is_some());
    let f = Facts { lesson: true, ..free };
    assert!(say::read(&f, &mut 0, None, |_| None).is_none(), "the chain");
    assert!(say::empty_step(&f, false).is_none(), "the empty steps");
    let h = opened("probe");
    assert!(!h.editor().say.dots_painted, "the dots hide");
}

/// Spec 5.12: a lesson's run leaves `hints_seen` and `quick_start` as found.
#[test]
fn aulesson_hints_bits_untouched() {
    let mut h = opened("probe");
    let (seen, quick) = (h.editor().say.seen, h.editor().quick_start);
    pen(&mut h, &along([300.0, 300.0], [500.0, 300.0]));
    for step in [Action::RoomFinishing, Action::RoomComposite, Action::RoomExport] {
        h.perform(step);
        settle(&mut h);
    }
    assert_eq!(at(&h), 4, "the run went on");
    assert_eq!(h.editor().say.seen, seen);
    assert_eq!(h.editor().quick_start, quick);
}

/// Spec 5.11: the document stays; what the lesson borrowed goes back.
#[test]
fn aulesson_leave_keeps_the_document() {
    let mut h = EditorHarness::as_shipped();
    let (gap, ink) = (h.editor().canvas.fill_gap, h.editor().canvas.brush_color);
    let bright = Lesson { fill_gap: Some(gap + 5), ink: Some([9, 9, 9, 255]), ..PROBE };
    h.editor_mut().begin_lesson(bright, 0);
    h.state_mut().engine.project.app_meta.insert(lesson::KEY.into(), "probe/0".into());
    settle(&mut h);
    assert_eq!(h.editor().canvas.fill_gap, gap + 5);
    assert_eq!(h.editor().canvas.brush_color, [9, 9, 9, 255]);
    pen(&mut h, &along([300.0, 300.0], [500.0, 300.0]));
    h.click(LEAVE);
    settle(&mut h);
    assert_eq!((h.editor().canvas.fill_gap, h.editor().canvas.brush_color), (gap, ink));
    assert!(!h.state().engine.project.app_meta.contains_key(lesson::KEY));
    assert!(h.state().has_art(), "the work stays");
    assert!(!h.has(LEAVE), "the card goes");
}

/// Spec 5.11: progress outlives a save, and a reopened file picks up the task.
#[test]
fn aulesson_leave_then_resume() {
    let mut h = opened("probe");
    pen(&mut h, &along([300.0, 300.0], [500.0, 300.0]));
    assert_eq!(at(&h), 1);
    assert_eq!(h.state().engine.project.app_meta.get(lesson::KEY).map(String::as_str), Some("probe/1"));
    let dir = tempfile::tempdir().expect("dir");
    let file = dir.path().join("mine.animproj");
    anim_core::store::save(&h.state().engine.project, &file).expect("saved");
    let mut h = EditorHarness::as_shipped();
    h.answer_start(WelcomeAction::OpenRecent(file.clone()));
    settle(&mut h);
    assert_eq!(at(&h), 1, "the same task");
    assert!(h.editor().lesson.as_ref().is_some_and(|run| run.lesson.id == "probe"));
    // An id this build does not know is ignored and kept.
    let mut state = h.state().engine.project.clone();
    state.app_meta.insert(lesson::KEY.into(), "from-the-future/2".into());
    anim_core::store::save(&state, &file).expect("saved");
    h.answer_start(WelcomeAction::OpenRecent(file.clone()));
    settle(&mut h);
    assert!(h.editor().lesson.is_none());
    assert_eq!(h.state().engine.project.app_meta.get(lesson::KEY).map(String::as_str), Some("from-the-future/2"));
    // A guest never runs one.
    let mut h = EditorHarness::as_shipped();
    h.editor_mut().canvas.is_guest = true;
    h.state_mut().engine.project.app_meta.insert(lesson::KEY.into(), "probe/1".into());
    assert!(!h.editor_mut().lesson_door());
}

/// Spec 2.7: a drawing that grows mid-lesson keeps its bar; Leave promotes.
#[test]
fn aulesson_holds_promotion() {
    let mut h = opened("probe");
    assert_eq!(h.editor().kind(), Some(ProjectKind::Drawing));
    h.perform(Action::AddDrawingAfter);
    h.state_mut().view.frame = 1;
    h.state_mut().commit_stroke(stroke());
    h.perform(Action::AddDrawingAfter);
    settle(&mut h);
    let grown = ProjectKind::grown(&h.state().engine.project);
    assert_eq!(grown, Some(ProjectKind::Animation), "the document has grown");
    assert_eq!(h.editor().kind(), Some(ProjectKind::Drawing), "the lesson holds its bar");
    h.click(LEAVE);
    settle(&mut h);
    assert_eq!(h.editor().kind(), Some(ProjectKind::Animation), "and Leave lets it run");
}

/// Spec 5.11: finishing writes the id; the page ticks it; a broken file is no
/// error and no tick.
#[test]
fn aulesson_finish_ticks_the_grid() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().to_path_buf();
    let mut h = EditorHarness::with_dir_shipped(dir);
    assert!(lesson::finished(Some(&path)).is_empty(), "missing means nothing ticked");
    let one: &'static [Task] = Box::leak(Box::new([task("", None, Stage::Drawing, Done::OnStep(Stage::Drawing), Show::Where)]));
    h.editor_mut().begin_lesson(Lesson { tasks: one, ..PROBE }, 0);
    settle(&mut h);
    assert!(h.editor().lesson.as_ref().is_some_and(|run| run.finished()));
    assert_eq!(lesson::finished(Some(&path)), ["probe"]);
    assert!(h.painted_text().iter().any(|t| t == lesson::MADE), "{:?}", h.painted_text());
    assert!(h.painted_text().iter().any(|t| t == PROBE.closing));
    assert!(h.has(lesson::NEXT));
    assert!(h.state().file_path.is_none() && h.has(LEAVE), "the document stays open");
    let mut w = Welcome::new(&[]);
    w.lessons = lessons::table();
    h.editor_mut().open_start(w.on_lessons());
    settle(&mut h);
    assert_eq!(h.editor().start.as_ref().map(|w| w.ticks_painted), Some(1));
    std::fs::write(path.join(lesson::FINISHED_FILE), [0xFF, 0xFE, 0x00]).expect("broken");
    assert!(lesson::finished(Some(&path)).is_empty(), "a broken file is no error");
}

/// `Next lesson`, across the classes, and home after the last.
#[test]
fn aulesson_next_walks_the_classes() {
    let mut h = opened("probe");
    let one: &'static [Task] = Box::leak(Box::new([task("", None, Stage::Drawing, Done::OnStep(Stage::Drawing), Show::Where)]));
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks: one, ..PROBE }, 0);
    settle(&mut h);
    h.click(lesson::NEXT);
    settle(&mut h);
    assert!(h.rig().requests.iter().any(|r| *r == crate::AppRequest::Start(WelcomeAction::Lesson(row("probe-square")))));
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks: one, ..TABLE[row("probe-last")] }, 0);
    settle(&mut h);
    h.click(lesson::NEXT);
    settle(&mut h);
    assert!(h.editor().start.as_ref().is_some_and(|w| w.tab == WelcomeTab::Lessons));
}

fn rules() -> impl Iterator<Item = &'static Lesson> {
    lessons::for_gate().iter()
}

/// Spec 5.10: every pill pressed, in bar order, and nothing wanders off the bar.
#[test]
fn aulesson_tasks_walk_the_bar() {
    for lesson in rules() {
        let bar: Vec<Stage> = steps::bar(lesson.kind).iter().map(|s| s.stage).collect();
        assert!(lesson.tasks.iter().all(|t| bar.contains(&t.step)), "{}", lesson.id);
        let mut walked = vec![lesson.tasks[0].step];
        for t in lesson.tasks {
            if t.step != *walked.last().expect("a step") {
                assert_eq!(t.done, Done::OnStep(t.step), "{}: the first task of a step is its pill", t.say);
                walked.push(t.step);
            }
        }
        let order: Vec<usize> = walked.iter().map(|s| bar.iter().position(|b| b == s).expect("on the bar")).collect();
        assert!(order.is_sorted(), "{}: {walked:?}", lesson.id);
        assert_eq!(walked.last(), Some(&Stage::Export), "{}", lesson.id);
    }
}

/// Spec 5.3: every `OnStep` task carries its reason.
#[test]
fn aulesson_step_reason_on_every_change() {
    for lesson in rules() {
        for t in lesson.tasks {
            if matches!(t.done, Done::OnStep(_)) {
                assert!(t.why.is_some_and(|w| !w.trim().is_empty()), "{}: {}", lesson.id, t.say);
            }
        }
    }
}

/// Spec 5.2's class rule, as far as a table reads.
#[test]
fn aulesson_class_is_honest() {
    for lesson in rules() {
        let art = (lesson.art)();
        for t in lesson.tasks {
            let guides = t.guide.iter().filter(|i| art[usize::from(**i)].kind == PathKind::Guide).count();
            match lesson.class {
                Class::Rookie => assert!(!matches!(t.show, Show::Where), "{}: {}", lesson.id, t.say),
                Class::Amateur => assert_eq!(guides, 0, "{}: {}", lesson.id, t.say),
                Class::Experienced => assert!(t.guide.is_empty(), "{}: {}", lesson.id, t.say),
            }
        }
    }
}

fn spoken(lesson: &Lesson) -> Vec<&'static str> {
    let tasks = lesson.tasks.iter().flat_map(|t| [Some(t.say), t.why, t.miss]).flatten();
    tasks.chain([lesson.closing, lesson.hover, lesson.title]).collect()
}

/// `text` holds `word`, whole. A control's token is no word of the sentence.
fn says(text: &str, word: &str) -> bool {
    let plain = lesson::resolve(text, |_| Some(String::new())).to_lowercase();
    plain.split(|c: char| !c.is_alphanumeric() && c != '-').any(|w| w == word)
}

/// Spec 5.2: no jargon to a Rookie.
#[test]
fn aulesson_rookie_words() {
    const OUT: [&str; 11] = ["frame", "layer", "key", "hold", "loop", "onion", "cel", "node", "composite", "exposure", "fps"];
    for lesson in rules().filter(|l| l.class == Class::Rookie) {
        for text in spoken(lesson) {
            for word in OUT {
                assert!(!says(text, word), "{}: {word} in {text}", lesson.id);
            }
        }
    }
    assert!(says("a frame of it", "frame") && !says("{Loop} it", "loop"), "the check itself");
}

/// The main spec's section 3: the old words stay out of a lesson.
#[test]
fn aulesson_no_old_words() {
    const OLD: &[&str] = &[
        "room", "workspace", "genga", "shiage", "satsuei", "shot-cut", "column", "x-sheet", "stack", "viewer", "shaku",
        "memo", "armed", "stabilizer", "hardness", "aliased",
    ];
    for lesson in rules() {
        for text in spoken(lesson) {
            for word in OLD {
                assert!(!says(text, word), "{}: {word} in {text}", lesson.id);
            }
        }
    }
}

/// Spec 5.3: every token in the lessons file and the table has a word.
#[test]
fn aulesson_tokens_resolve() {
    let run_words = |t: &str| t == "rest drawing" || lesson::chord_action(t).is_some() || lesson::word(t).is_some();
    let file = include_str!("../../research/LESSONS-approachable-ui.md");
    let row = |l: &&str| l.starts_with("| ") && l[2..].starts_with(|c: char| c.is_ascii_digit());
    let says = file.lines().filter(row).filter_map(|l| l.split(" | ").nth(1));
    let mut tokens: Vec<&str> = Vec::new();
    for text in rules().flat_map(spoken).chain(says) {
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else { break };
            tokens.push(&rest[open + 1..open + close]);
            rest = &rest[open + close + 1..];
        }
    }
    assert!(tokens.len() > 40, "{}", tokens.len());
    let unknown: Vec<_> = tokens.iter().filter(|t| !run_words(t)).collect();
    assert!(unknown.is_empty(), "{unknown:?}");
    assert_eq!(lesson::resolve("Press {Draw} and {nope}.", |t| lesson::word(t).map(str::to_string)), "Press Draw and {nope}.");
    assert_eq!(lesson::word("Night"), Some(lesson::NIGHT));
    assert_eq!(lesson::chord_action("chord:Undo"), Some(Action::Undo));
    let source = include_str!("lessons.rs").replace("Action::label", "");
    assert!(!source.contains(".label()"), "lesson text never uses Action::label()");
}
