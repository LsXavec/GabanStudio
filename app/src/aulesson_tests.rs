//! AU27: the lesson frame: a probe lesson over the real editor. No lesson ships
//! here: `PROBE` is this file's, and `lessons::table()` reads `TABLE`.

use std::sync::{Arc, LazyLock};

use anim_core::command::Command;
use anim_core::fx::{FxKind, FxValue};
use anim_core::generator::GenKind;
use anim_core::graph::NodeKind;
use anim_core::ids::NodeId;
use anim_core::manga::{self, Rulers};
use anim_core::model::{DrawingKind, LayerRole, Stroke, StrokePoint};
use anim_core::raster::{TILE, TILE_LEN, TileData};
use anim_core::xsheet::{Exposure, ParamInterp, ParamKey};
use eframe::egui::{Pos2, pos2, vec2};

use crate::aurail_tests::window;
use crate::canvas::CanvasTool;
use crate::config::{Action, BrushPreset};
use crate::lesson::{self, LEAVE, SHOW_ME};
use crate::lessons::{self, Class, Done, Latch, Lesson, Path, PathKind, Show, Target, Task};
use crate::say::{self, Facts};
use crate::steps::ProjectKind;
use crate::templates::{self, Template};
use crate::testkit::EditorHarness;
use crate::welcome::{Welcome, WelcomeAction, WelcomeTab};
use crate::workspace::Stage;
use crate::{PROBE_SCALES, StartOutcome, measure, plate, steps, vocab};

fn settle(h: &mut EditorHarness) {
    h.run_frames(4);
}

const SIZES: [(f32, f32); 4] = [(1600.0, 900.0), (1280.0, 800.0), (1280.0, 720.0), (1024.0, 640.0)];
const WORST: (f32, f32) = (832.0, 520.0);

const FOUR_THREE: Template = Template { kind: Some(ProjectKind::Drawing), width: 1200, height: 900, ..templates::READY };
const SQUARE: Template = Template { kind: Some(ProjectKind::Drawing), width: 1000, height: 1000, ..templates::READY };
/// Slow enough that the Rig's clock, a tenth of a second a step at most, shows
/// a pass each of its frames.
const FILM: Template = Template { kind: Some(ProjectKind::Animation), width: 1200, height: 900, cut_frames: 24, fps: 5, ..templates::READY };

/// A hand's line: the art rules of spec 5.14 read the probe too.
fn line(a: [f32; 2], b: [f32; 2], kind: PathKind) -> Path {
    lessons::art::hand(&[a, b], 4.0, kind, a[0] as u32 ^ a[1] as u32)
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
    Task { say, why, step, done, ring: &[Target::Paper], guide: &[], stand_in: &[], hand: None, cells: &[], miss: None, show }
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
    Task {
        ring: &[Target::Action(Action::ExportStill)],
        ..task("Save it as a picture.", None, Stage::Export, Done::Exported(1), Show::Perform(Action::ExportStill))
    },
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

/// The probe in the words of the lessons file's first lesson: what a person
/// meets on the card, and so what its pictures read.
const SPOKEN: &[Task] = &[
    Task {
        say: "This is Deckle, a strip of animation paper that tore off and got away. Draw over the blue line.",
        why: Some("Draw is where the picture starts."),
        ..TASKS[0]
    },
    Task { say: "Deckle is ready. Press {Color}.", why: Some("Color is where the picture gets its night."), ..TASKS[1] },
    Task { say: "Press {Effects}.", why: Some("Effects is where light gets added, after the picture is finished."), ..TASKS[2] },
    Task { say: "Press {Export}.", why: Some("Export turns your project into a file anyone can open."), ..TASKS[3] },
    Task { say: "Save your picture.", ..TASKS[4] },
];

/// The chip's task, in those words.
const NIGHT: &[Task] = &[
    Task {
        say: "Pick {Night} on this card.",
        step: Stage::Drawing,
        done: Done::Picked([1, 2, 3]),
        ring: &[Target::Named(lesson::CHIP)],
        ..NEVER
    },
    NEVER,
];

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
    let closing = "You can draw, undo, erase, fill a whole sky and add light.";
    rows.push(Lesson { id: "probe-spoken", title: "Up Late", tasks: SPOKEN, closing, ..PROBE });
    rows.push(Lesson { id: "probe-night", title: "Up Late", tasks: NIGHT, closing, ..PROBE });
    rows
});

/// The probes alone: a cell of the Lessons page holds its class's one lesson,
/// so a real lesson stands where the probe of its class and type would.
fn probes() -> &'static [Lesson] {
    &TABLE[lessons::ALL.len()..]
}

fn row(id: &str) -> usize {
    TABLE.iter().position(|l| l.id == id).expect("a row of the table")
}

/// `id` opened the way a page card opens it.
pub(crate) fn open(h: &mut EditorHarness, id: &str) {
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
pub(crate) fn pen(h: &mut EditorHarness, path: &[[f32; 2]]) {
    let pts: Vec<Pos2> = path.iter().map(|p| on_screen(h, *p)).collect();
    h.pen_down(pts[0]);
    pts[1..].iter().for_each(|p| h.pen_move(*p));
    h.pen_up(pts[pts.len() - 1]);
    settle(h);
}

pub(crate) fn along(a: [f32; 2], b: [f32; 2]) -> Vec<[f32; 2]> {
    (0..=6).map(|i| { let t = i as f32 / 6.0; [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t] }).collect()
}

fn stroke() -> Stroke {
    let pt = |x: f32| StrokePoint { x, y: 40.0, pressure: 0.5, tilt: [0.0, 0.0] };
    Stroke { points: vec![pt(10.0), pt(40.0)], base_width: 2.0, color: [0, 0, 0, 255] }
}

/// Texels on the armed layer, `rgba` in half-float bits.
pub(crate) fn texels(h: &mut EditorHarness, cells: &[(i32, i32)], alpha: u16) {
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

/// Spec 4.5, on a table with lessons: one click.
#[test]
fn aulesson_page_is_one_click() {
    let mut h = EditorHarness::as_first_run();
    let mut w = Welcome::new(&[]);
    w.lessons = probes();
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
    assert!(h.has("Size"), "and the rail: {}", h.describe());
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

/// Spec 3.10: the band's key works in a lesson's Plan and remembers nothing:
/// the hide is the run's, and Leave gives the person's memory back.
#[test]
fn aulesson_sheet_key_remembers_nothing() {
    for hidden in [false, true] {
        let mut h = EditorHarness::as_animation();
        if hidden {
            h.hide_sheet();
        }
        open(&mut h, "probe-film");
        assert!(h.editor().sheet_on_screen(), "{}", h.describe());
        h.click(vocab::HIDE_TIMING_SHEET);
        settle(&mut h);
        assert!(!h.editor().sheet_on_screen(), "the key takes the sheet off: {}", h.describe());
        assert_eq!(h.editor().plan_sheet_hidden, hidden, "the memory stands");
        assert_eq!(h.rig().config.ui.plan_sheet_hidden(), hidden, "and the stored value");
        h.click(vocab::SHOW_TIMING_SHEET);
        settle(&mut h);
        assert!(h.editor().sheet_on_screen(), "and brings it back");
        h.click(vocab::HIDE_TIMING_SHEET);
        settle(&mut h);
        h.click(LEAVE);
        settle(&mut h);
        assert_eq!(h.editor().sheet_on_screen(), !hidden, "Leave: the memory's own sheet");
        assert_eq!(h.editor().plan_sheet_hidden, hidden);
        assert_eq!(h.rig().config.ui.plan_sheet_hidden(), hidden);
    }
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
                    let what = format!("{id} {} {w}x{tall} {scale}", bar.label);
                    let share = measure::paper_share(&h);
                    assert_eq!(share.paper_under_chrome, 0, "{what}");
                    // Timing, Effects and Export show their result, not the paper.
                    if matches!(bar.stage, Stage::Edit | Stage::Composite | Stage::Export) {
                        continue;
                    }
                    assert!(share.paper > 0.0, "{what}: no paper to cover");
                    let xf = h.editor().canvas.view_xform();
                    let right = xf.paper_to_screen(pos2(xf.paper.x, xf.paper.y)).x;
                    let card = h.ctx().content_rect().right() - lesson::card_width(h.ctx().content_rect().width());
                    assert!(right <= card + 0.5, "{what}: the paper's edge {right} past the card's {card}");
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

/// A lesson that walks every step of an animation's bar, `say` on a task of
/// its Effects step: the path at its longest, and off its step the wander line
/// on, which is the card at its fullest.
fn five_steps(say: &'static str) -> Lesson {
    let step = |stage| Task { step: stage, ring: &[], ..NEVER };
    let tasks = [
        Task { say, show: Show::Perform(Action::Undo), ..step(Stage::Composite) },
        step(Stage::Drawing),
        step(Stage::Edit),
        step(Stage::InkPaint),
        step(Stage::Export),
    ];
    Lesson { tasks: Box::leak(Box::new(tasks)), ..TABLE[row("probe-film")] }
}

/// The scroll view on the card.
fn card_view(h: &EditorHarness) -> eframe::egui::Rect {
    let ctx = h.ctx();
    let left = ctx.content_rect().right() - lesson::card_width(ctx.content_rect().width());
    let found = plate::viewports(&ctx).into_iter().find(|v| v.rect.left() >= left);
    found.expect("the card's scroll view").rect
}

/// Spec 5.6: at the worst size each task's sentence lies on the card, in the
/// room its scroll view leaves under the title, the strip and the path, its
/// reason a scroll away, and neither key leaves the view. All read off the card
/// itself, on a real five-step lesson with the wander line on.
#[test]
fn aulesson_card_fits_every_sentence() {
    let mut sentences = file_sentences();
    assert!(sentences.len() > 60, "the lessons file's tables: {}", sentences.len());
    sentences.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let sentence = |text: &str| text.split_once("*Why:").map_or(text, |(say, _)| say).trim().to_string();
    let says: Vec<String> = sentences.iter().map(|s| sentence(s)).collect();
    let longest = says.iter().max_by_key(|s| s.len()).expect("a sentence");
    let leak: &'static str = Box::leak(longest.clone().into_boxed_str());
    let mut h = opened("probe-film");
    window(&mut h, WORST.0, WORST.1, 1.0);
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(five_steps(leak), 0);
    settle(&mut h);
    assert_ne!(h.editor().stage, Some(Stage::Composite), "the person has wandered");
    assert!(h.painted_text().iter().any(|t| t.starts_with("Go back to")), "{:?}", h.painted_text());
    let ctx = h.ctx();
    assert_eq!(lesson::card_width(ctx.content_rect().width()), 180.0);
    let view = card_view(&h);
    let (block, size) = h.painted_block(leak);
    assert_eq!(size, 16.0, "the sentence stands at 16");
    assert!(block.bottom() <= view.bottom(), "{} chars ends {} past {view:?}", leak.len(), block.bottom());
    assert!(block.right() <= view.right() + 0.5, "{block:?} in {view:?}");
    // The room the real card leaves, and every sentence the lessons will say.
    let room = view.bottom() - block.top();
    for say in &says {
        let tall = ctx.fonts_mut(|f| f.layout(say.clone(), plate::Role::Heading.font(), plate::INK, view.width())).size();
        assert!(tall.x <= view.width() + 0.5, "{say}: {tall:?} wide");
        assert!(tall.y <= room, "{say}: {tall:?} in {room}");
    }
    for key in [SHOW_ME, LEAVE] {
        assert!(ctx.content_rect().contains_rect(h.rect(key)), "{key}: {:?}", h.rect(key));
        assert!(h.rect(key).top() >= view.bottom(), "{key} stands outside the scroll");
    }
    assert!(h.rect(SHOW_ME).bottom() <= h.rect(LEAVE).top());
}

/// Spec 5.6: the card's order, top to bottom: the title, the strip, the path,
/// the sentence at 16, its reason at 13, the finish picture, and the two keys
/// outside the scroll.
#[test]
fn aulesson_card_reads_in_order() {
    let mut h = opened("probe");
    pen(&mut h, &along([300.0, 300.0], [500.0, 300.0]));
    assert_eq!(at(&h), 1);
    window(&mut h, 1600.0, 900.0, 1.0);
    let (title, _) = h.painted_block(PROBE.title);
    let strip = h.rect(&format!("1 of {} done", TASKS.len()));
    let path_words = "Draw  Color  Effects  Export";
    let (path, _) = h.painted_block(path_words);
    let (say, say_size) = h.painted_block("Press Color.");
    let (why, why_size) = h.painted_block("Paint goes under the line.");
    assert_eq!((say_size, why_size), (16.0, 14.0));
    let order = [title.bottom(), strip.top(), strip.bottom(), path.top(), path.bottom(), say.top(), say.bottom(), why.top()];
    assert!(order.is_sorted(), "{order:?}");
    let view = card_view(&h);
    assert_eq!(strip.width(), view.width(), "the strip runs the card's width");
    assert!(view.bottom() <= h.rect(SHOW_ME).top() && h.rect(SHOW_ME).bottom() <= h.rect(LEAVE).top());
    let bottom = view.bottom();
    let card = h.filled_rects(plate::PAPER).into_iter().filter(|r| r.left() >= view.left() && r.bottom() <= bottom);
    let picture = card.max_by(|a, b| a.top().total_cmp(&b.top())).expect("the finish picture's paper");
    assert_eq!(picture.height(), 104.0, "{picture:?}");
    assert!(why.bottom() <= picture.top(), "the picture stands under the words");
    // The finish holds its place: under the words, over the keys.
    let one: &'static [Task] = Box::leak(Box::new([task("", None, Stage::Drawing, Done::OnStep(Stage::Drawing), Show::Where)]));
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks: one, ..PROBE }, 0);
    settle(&mut h);
    let (made, size) = h.painted_block(lesson::MADE);
    let (closing, _) = h.painted_block(PROBE.closing);
    assert_eq!(size, 16.0);
    assert!(made.bottom() <= closing.top() && closing.bottom() <= h.rect(LEAVE).top());
}

/// Spec 5.6: Deckle is a strip of paper that does not stretch: every pose runs
/// the same length, and it stands on the strip from before the first tick.
#[test]
fn aulesson_deckle_keeps_its_length() {
    let along = |spine: &[eframe::egui::Vec2]| spine.windows(2).map(|p| (p[1] - p[0]).length()).sum::<f32>();
    for (pose, standing) in [(0, false), (1, false), (2, false), (0, true)] {
        let run = along(&plate::deckle_spine(pose, standing));
        assert!((run - plate::DECKLE_LONG).abs() <= plate::DECKLE_LONG * 0.01, "pose {pose} {standing}: {run}");
    }
    let flat = plate::deckle_spine(0, false);
    let arch = plate::deckle_spine(1, false);
    let high = |s: &[eframe::egui::Vec2]| s.iter().map(|v| -v.y).fold(f32::MIN, f32::max);
    assert!(high(&arch) > high(&flat) + 4.0, "the arch reads at the card's scale");
    assert!(high(&plate::deckle_spine(0, true)) > high(&arch), "and standing up is the finish");
    // Every lesson keeps its dots or its short ticks in reach of each other.
    let width = lesson::card_width(WORST.0) - 24.0;
    let (inset, pitch) = plate::strip_stops(width, 20);
    assert!(inset >= plate::DECKLE_LONG / 2.0, "Deckle starts on the strip");
    assert!(pitch < plate::TICK_PITCH, "twenty tasks are short ticks, not touching dots: {pitch}");
    assert!(pitch >= 2.0, "and stay ticks: {pitch}");
    let (_, pitch) = plate::strip_stops(width, 5);
    assert!(pitch >= plate::TICK_PITCH, "five are dots: {pitch}");
}

/// One reader, both ways, on a fresh `room`: after `set` it does not hold,
/// `miss` lands an act that is not the outcome, and `hit` the outcome.
struct Reading {
    done: Done,
    room: &'static str,
    set: fn(&mut EditorHarness),
    miss: fn(&mut EditorHarness),
    hit: fn(&mut EditorHarness),
}

const fn reading(done: Done, room: &'static str, miss: fn(&mut EditorHarness), hit: fn(&mut EditorHarness)) -> Reading {
    Reading { done, room, set: |_| {}, miss, hit }
}

/// `Done`'s variants, each with its place in the list. No arm is a wildcard: a
/// new goal does not compile here until it has one, and with one it fails
/// `aulesson_done_reads_the_state` until it has a row.
const READERS: usize = 55;

fn reader(done: Done) -> usize {
    match done {
        Done::OnStep(_) => 0,
        Done::Ink(_) => 1,
        Done::Strokes(_) => 2,
        Done::KeyArt { .. } => 3,
        Done::InkOnLayer => 4,
        Done::InkInPanels(_) => 5,
        Done::BrushChanged => 6,
        Done::Armed(_) => 7,
        Done::Smoothing => 8,
        Done::Tool(_) => 9,
        Done::Undone => 10,
        Done::Erased(_) => 11,
        Done::Filled { .. } => 12,
        Done::Fills(_) => 13,
        Done::FilledOn(_) => 14,
        Done::Drawings(_) => 15,
        Done::Frame(..) => 16,
        Done::Played => 17,
        Done::Speed(_) => 18,
        Done::Hold { .. } => 19,
        Done::Shows { .. } => 20,
        Done::Kinds(_) => 21,
        Done::Loop => 22,
        Done::Layers(_) => 23,
        Done::LayerHidden => 24,
        Done::LayerRole(_) => 25,
        Done::Moved => 26,
        Done::Mirrored => 27,
        Done::Effect(_) => 28,
        Done::AnyEffect => 29,
        Done::Strength => 30,
        Done::CameraKeys(_) => 31,
        Done::Eased => 32,
        Done::Particles => 33,
        Done::Panels(_) => 34,
        Done::Pages(_) => 35,
        Done::Gutters => 36,
        Done::Rulers(_) => 37,
        Done::Tone => 38,
        Done::Generator(_) => 39,
        Done::Words => 40,
        Done::PageSeen => 41,
        Done::Lipped(_) => 42,
        Done::Inbetweens => 43,
        Done::Chart => 44,
        Done::Exported(_) => 45,
        Done::Preset(_) => 46,
        Done::Picked(_) => 47,
        Done::Locked(_) => 48,
        Done::PanelPicked(_) => 49,
        Done::All(_) => 50,
        Done::Snap(_) => 51,
        Done::Scattered(_) => 52,
        Done::OnTrack(_) => 53,
        Done::LinesDown => 54,
    }
}

/// Inked drawings on `frames` of the armed track, past its first.
fn keyed(h: &mut EditorHarness, frames: &[u32]) {
    let s = h.state_mut();
    for &f in frames {
        s.goto(f);
        if f > 0 {
            s.new_drawing_at_frame();
        }
        s.commit_stroke(stroke());
    }
    s.goto(0);
}

/// The armed track's key at `frame`.
fn key(h: &EditorHarness, frame: u32) -> Option<Exposure> {
    h.state().cut().xsheet.column(h.state().view.active_column).and_then(|c| c.key_at(frame))
}

fn cell(h: &mut EditorHarness, frame: u32, key: Option<Exposure>) {
    let s = h.state_mut();
    let (at, column) = (s.at(), s.view.active_column);
    s.engine.apply("cell", vec![Command::SetCell { at, column, frame, key }]).expect("a cell");
}

/// `rows` drawings a row each, and a blank past them: the run's range ends on
/// the last.
fn filmed(h: &mut EditorHarness, rows: u32) {
    let frames: Vec<u32> = (0..rows).collect();
    keyed(h, &frames);
    cell(h, rows, Some(Exposure::Empty));
}

fn rekey(h: &mut EditorHarness, from: u32, to: u32) {
    let was = key(h, from);
    cell(h, from, None);
    cell(h, to, was);
}

fn fill_bump(h: &mut EditorHarness) {
    h.state_mut().fills_done += 1;
}

fn node_added(h: &mut EditorHarness, kind: NodeKind) -> NodeId {
    h.state_mut().graph_add_node(kind, (0.0, 0.0));
    h.state().selected_node.expect("the node added")
}

fn node_edit(h: &mut EditorHarness, id: NodeId, edit: impl FnOnce(&mut anim_core::fx::FxParams)) {
    let s = h.state_mut();
    let at = s.at();
    let mut kind = s.cut().graph.node(id).expect("the node").kind.clone();
    match &mut kind {
        NodeKind::Effect { params, .. } | NodeKind::Generator { params, .. } => edit(params),
        _ => panic!("no parameters"),
    }
    s.engine.apply("edit", vec![Command::SetNodeKind { at, id, kind }]).expect("edited");
}

/// A node added, and the wire to the Output that makes it part of the picture.
fn wire_selected(h: &mut EditorHarness) {
    let id = h.state().selected_node.expect("a node");
    h.state_mut().graph_set_output(id);
}

fn wired(h: &mut EditorHarness, kind: NodeKind) -> NodeId {
    let id = node_added(h, kind);
    wire_selected(h);
    id
}

fn the_effect(h: &EditorHarness) -> NodeId {
    let mut nodes = h.state().cut().graph.nodes();
    nodes.find(|n| matches!(n.kind, NodeKind::Effect { .. })).expect("an effect").id
}

fn the_text(h: &EditorHarness) -> NodeId {
    let mut nodes = h.state().cut().graph.nodes();
    let text = |n: &&anim_core::graph::Node| matches!(n.kind, NodeKind::Generator { generator: GenKind::Text, .. });
    nodes.find(text).expect("a text").id
}

/// The speed lines `wire_lines` left.
fn the_lines(h: &EditorHarness) -> NodeId {
    let mut nodes = h.state().cut().graph.nodes();
    let lines = |n: &&anim_core::graph::Node| matches!(n.kind, NodeKind::Generator { generator: GenKind::SpeedLines, .. });
    nodes.find(lines).expect("the lines").id
}

/// Speed lines into the output.
fn wire_lines(h: &mut EditorHarness) {
    node_added(h, NodeKind::generator(GenKind::SpeedLines));
    wire_selected(h);
}

fn on_track(h: &mut EditorHarness, track: usize) {
    let s = h.state_mut();
    s.view.active_column = s.cut().xsheet.columns[track].id;
}

fn typed(words: &'static str) -> impl FnOnce(&mut anim_core::fx::FxParams) {
    move |params| {
        for v in params.0.values_mut() {
            if let FxValue::Text { text, .. } = v {
                *text = words.to_string();
            }
        }
    }
}

fn camera_key(h: &mut EditorHarness, frame: u32, interp: ParamInterp) {
    let s = h.state_mut();
    let column = s.cut().xsheet.params[0].id;
    s.goto(frame);
    s.set_param_key(column, Some(ParamKey { value: 1.0, interp, handles: None }));
}

fn panels(h: &mut EditorHarness, n: usize) {
    let s = h.state_mut();
    let at = s.at();
    let (id, _) = manga::panels_of(s.cut()).expect("a page's panels");
    let (w, tall) = (TABLE[row("probe-page")].paper.width as f32, TABLE[row("probe-page")].paper.height as f32);
    let side = w / n as f32;
    let slice = |i: usize| {
        let (left, right) = (side * i as f32, side * (i + 1) as f32);
        vec![[left, 0.0], [right, 0.0], [right, tall], [left, tall]]
    };
    let polys = (0..n).map(slice).collect();
    let edit = manga::set_panels(s.cut(), at, id, polys).expect("the panels' edit");
    s.engine.apply("panels", vec![edit]).expect("panels");
}

fn rulers(h: &mut EditorHarness, points: usize) {
    let s = h.state_mut();
    let at = s.at();
    let edit = Rulers::preset(points, 1000.0, 1000.0).set(s.cut(), at).expect("the rulers' edit");
    s.engine.apply("rulers", vec![edit]).expect("rulers");
}

fn preset(h: &mut EditorHarness, bank: &str, size_px: f32) {
    let new = BrushPreset { name: "Mine".into(), bank: bank.into(), size_px, ..Default::default() };
    h.rig_mut().config.presets.push(new);
}

/// Spec 5.4's smudge, on path 1 of the probe's art.
fn smudge(from: i32, to: i32) -> Vec<(i32, i32)> {
    (from..to).flat_map(|i| [(590 + i, 590 + i), (591 + i, 590 + i)]).collect()
}

/// A solid patch and the Select tool: what a lift lifts.
fn inked_patch(h: &mut EditorHarness) {
    let cells: Vec<(i32, i32)> = (500..564).flat_map(|y| (500..564).map(move |x| (x, y))).collect();
    texels(h, &cells, 0x3C00);
    let ed = h.editor_mut();
    ed.canvas.set_tool(CanvasTool::Select, &mut ed.state);
}

/// The lift, and its commit: the CPU half, the Rig has no GPU to preview with.
fn lift(h: &mut EditorHarness) {
    let ctx = h.ctx();
    let ed = h.editor_mut();
    assert!(ed.canvas.lift_floating(&mut ed.state, &ctx, None).is_some(), "lifted: {}", ed.state.status);
    h.run_frames(2);
}

fn land_gesture(h: &mut EditorHarness) {
    let ed = h.editor_mut();
    ed.canvas.finish_gesture(&mut ed.state);
}

fn move_patch(h: &mut EditorHarness) {
    lift(h);
    let grab = pos2(532.0, 532.0);
    let s = on_screen(h, [grab.x, grab.y]);
    let to = on_screen(h, [grab.x + 60.0, grab.y]);
    let path: Vec<Pos2> = [s + vec2(-6.0, 0.0), s].into_iter().chain((1..=4).map(|k| s.lerp(to, k as f32 / 4.0))).collect();
    h.drag_path(&path);
    land_gesture(h);
}

pub(crate) fn export_asked(h: &mut EditorHarness) {
    let ctx = h.ctx();
    h.editor_mut().invoke(Action::ExportStill, &ctx);
    settle(h);
}

fn export_landed(h: &mut EditorHarness) {
    let dir = tempfile::tempdir().expect("dir");
    let file = dir.path().join("still.png");
    h.answer_export(&file);
    h.run_export_to_end();
    settle(h);
    assert!(file.exists(), "the file landed");
}

/// Every play button press in one place: a person at the transport.
fn play_key(h: &mut EditorHarness) {
    h.perform(Action::PlayPause);
}

/// Frames until `until` holds. `None`: it never did.
fn frames_until(h: &mut EditorHarness, until: impl Fn(&EditorHarness) -> bool) -> Option<usize> {
    (0..1200).find(|_| {
        h.run_frames(1);
        until(h)
    })
}

/// A pass by the transport with the loop off, top to the last drawing.
fn pass(h: &mut EditorHarness) {
    h.state_mut().view.loop_playback = false;
    play_key(h);
    frames_until(h, |h| !h.state().view.playing).expect("the run ends");
}

/// Spec 5.4, every reader, both ways, off `readings`' fresh rooms. `reader`'s
/// list says none is missing. Moved lands through a real `commit_floating`
/// and Exported through the real export; the others read the outcome, so a
/// row reaches it the shortest way.
fn readings() -> Vec<Reading> {
    let stroked = |h: &mut EditorHarness| h.state_mut().commit_stroke(stroke());
    vec![
        reading(Done::OnStep(Stage::InkPaint), "probe", |h| h.editor_mut().enter_room(Stage::Composite), |h| h.editor_mut().enter_room(Stage::InkPaint)),
        reading(Done::Ink(&[0]), "probe", |h| pen(h, &along([590.0, 590.0], [610.0, 610.0])), |h| pen(h, &along([210.0, 205.0], [990.0, 205.0]))),
        reading(Done::Strokes(2), "probe", stroked, stroked),
        reading(Done::KeyArt { track: 0, frame: 1 }, "probe-film", |h| {
            h.state_mut().goto(1);
            h.state_mut().new_drawing_at_frame();
        }, |h| h.state_mut().commit_stroke(stroke())),
        reading(Done::InkOnLayer, "probe", |h| h.state_mut().commit_stroke(stroke()), |h| texels(h, &[(100, 100)], 0x3C00)),
        Reading {
            set: |h| panels(h, 2),
            ..reading(Done::InkInPanels(2), "probe-page", |h| {
                pen(h, &along([100.0, 100.0], [200.0, 100.0]));
                pen(h, &along([100.0, 200.0], [200.0, 200.0]));
            }, |h| {
                let w = TABLE[row("probe-page")].paper.width as f32;
                pen(h, &along([w - 200.0, 100.0], [w - 100.0, 100.0]));
            })
        },
        reading(Done::BrushChanged, "probe", |h| h.editor_mut().canvas.alpha_lock = true, |h| h.editor_mut().canvas.brush_width += 3.0),
        reading(Done::Armed("Clean"), "probe", |h| h.editor_mut().canvas.armed_preset = Some("Sketch".into()), |h| h.editor_mut().canvas.armed_preset = Some("Clean".into())),
        Reading {
            set: |h| h.rig_mut().config.pen.stabilizer = 5,
            ..reading(Done::Smoothing, "probe", |h| h.rig_mut().config.pen.stabilizer = 2, |h| h.rig_mut().config.pen.stabilizer = 8)
        },
        reading(Done::Tool(CanvasTool::Select), "probe", |h| h.editor_mut().canvas.tool = CanvasTool::Fill, |h| h.editor_mut().canvas.tool = CanvasTool::Select),
        reading(Done::Undone, "probe", stroked, |h| {
            h.perform(Action::Undo);
        }),
        Reading {
            set: |h| {
                texels(h, &smudge(0, 20), 0x3C00);
            },
            ..reading(Done::Erased(1), "probe", |h| texels(h, &smudge(0, 10), 0), |h| texels(h, &smudge(10, 20), 0))
        },
        reading(Done::Filled { seed: [100.0, 100.0], keep: Some([300.0, 300.0]) }, "probe", |h| {
            texels(h, &[(500, 500)], 0x3C00);
            fill_bump(h);
        }, |h| {
            texels(h, &[(100, 100)], 0x3C00);
            fill_bump(h);
        }),
        reading(Done::Fills(2), "probe", fill_bump, fill_bump),
        reading(Done::FilledOn(1), "probe", |h| texels(h, &[(100, 100)], 0x3C00), |h| {
            h.state_mut().layer_add_role(LayerRole::Color);
            texels(h, &[(100, 100)], 0x3C00);
        }),
        reading(Done::Drawings(2), "probe-film", |h| {
            h.state_mut().goto(1);
            h.state_mut().new_drawing_at_frame();
        }, |h| {
            h.state_mut().goto(2);
            h.state_mut().new_drawing_at_frame();
        }),
        reading(Done::Frame(3, 4), "probe", |h| h.state_mut().view.frame = 5, |h| h.state_mut().view.frame = 3),
        Reading {
            set: |h| filmed(h, 4),
            ..reading(Done::Played, "probe-film", |h| {
                play_key(h);
                h.run_frames(1);
                play_key(h);
            }, pass)
        },
        Reading {
            set: |h| keyed(h, &[0, 3, 6]),
            ..reading(Done::Speed(2), "probe-film", |h| rekey(h, 3, 2), |h| rekey(h, 6, 4))
        },
        Reading {
            set: |h| keyed(h, &[0, 3]),
            ..reading(Done::Hold { frame: 0, len: 2 }, "probe-film", |h| rekey(h, 3, 4), |h| rekey(h, 4, 2))
        },
        Reading {
            set: |h| keyed(h, &[0, 1]),
            ..reading(Done::Shows { frame: 2, drawing: 0 }, "probe-film", |h| cell(h, 2, Some(Exposure::Empty)), |h| {
                let first = key(h, 0);
                cell(h, 2, first);
            })
        },
        Reading {
            set: |h| keyed(h, &[0, 1]),
            ..reading(Done::Kinds(2), "probe-film", |h| {
                let id = h.state().cut().drawings[0].id;
                h.state_mut().set_drawing_kind(id, DrawingKind::Key);
            }, |h| {
                let id = h.state().cut().drawings[1].id;
                h.state_mut().set_drawing_kind(id, DrawingKind::Key);
            })
        },
        reading(Done::Loop, "probe-film", |h| h.state_mut().view.loop_playback = true, |h| h.state_mut().set_loop_in()),
        Reading {
            set: |h| keyed(h, &[0]),
            ..reading(Done::Layers(4), "probe", |h| h.state_mut().layer_add_role(LayerRole::Rough), |h| h.state_mut().layer_add_role(LayerRole::Rough))
        },
        Reading {
            set: |h| keyed(h, &[0]),
            ..reading(Done::LayerHidden, "probe", |h| {
                let layer = h.state().active_layer_id().expect("a layer");
                let props = anim_core::model::LayerProps { opacity: 0.5, ..h.state().active_layer_props().expect("props").clone() };
                h.state_mut().layer_set_props(layer, props);
            }, |h| {
                let layer = h.state().active_layer_id().expect("a layer");
                let props = anim_core::model::LayerProps { visible: false, ..h.state().active_layer_props().expect("props").clone() };
                h.state_mut().layer_set_props(layer, props);
            })
        },
        Reading {
            set: |h| keyed(h, &[0]),
            ..reading(Done::LayerRole(LayerRole::Shadow), "probe", |h| h.state_mut().layer_add_role(LayerRole::Highlight), |h| h.state_mut().layer_add_role(LayerRole::Shadow))
        },
        // A lift put down where it stood commits nothing.
        Reading { set: inked_patch, ..reading(Done::Moved, "probe", |h| {
            lift(h);
            land_gesture(h);
        }, move_patch) },
        reading(Done::Mirrored, "probe", |h| {
            h.perform(Action::MirrorView);
        }, |h| {
            h.perform(Action::MirrorView);
        }),
        // On the graph but off the picture, then wired to its Output.
        reading(Done::Effect(FxKind::Glow), "probe-film", |h| {
            node_added(h, NodeKind::effect(FxKind::Glow));
        }, wire_selected),
        reading(Done::AnyEffect, "probe-film", |h| {
            node_added(h, NodeKind::effect(FxKind::Glow));
        }, wire_selected),
        Reading {
            set: |h| {
                wired(h, NodeKind::effect(FxKind::Glow));
            },
            ..reading(Done::Strength, "probe-film", |h| {
                node_added(h, NodeKind::effect(FxKind::Glow));
            }, |h| {
                let id = the_effect(h);
                node_edit(h, id, |params| {
                    for v in params.0.values_mut() {
                        if let FxValue::Float(f) = v {
                            *f += 1.0;
                        }
                    }
                });
            })
        },
        Reading {
            set: |h| {
                h.state_mut().add_param_column("cam").expect("a column");
            },
            ..reading(Done::CameraKeys(2), "probe-film", |h| camera_key(h, 0, ParamInterp::Linear), |h| camera_key(h, 4, ParamInterp::Linear))
        },
        Reading {
            set: |h| {
                h.state_mut().add_param_column("cam").expect("a column");
            },
            ..reading(Done::Eased, "probe-film", |h| camera_key(h, 0, ParamInterp::Linear), |h| camera_key(h, 4, ParamInterp::Ease))
        },
        reading(Done::Particles, "probe-film", |h| {
            node_added(h, NodeKind::generator(GenKind::Particles));
        }, wire_selected),
        reading(Done::Panels(3), "probe-page", |h| panels(h, 2), |h| panels(h, 3)),
        reading(Done::Pages(3), "probe-page", |h| {
            let scene = h.state().view.scene;
            h.state_mut().new_page(scene);
        }, |h| {
            let scene = h.state().view.scene;
            h.state_mut().new_page(scene);
        }),
        reading(Done::Gutters, "probe-page", |h| h.state_mut().manga.selected = 1, |h| h.state_mut().manga.side_mm += 1.0),
        reading(Done::Rulers(2), "probe-page", |h| rulers(h, 1), |h| rulers(h, 2)),
        reading(Done::Tone, "probe-page", |h| h.state_mut().commit_stroke(stroke()), |h| {
            let cut = h.state().cut();
            let tone = cut.xsheet.columns.iter().find(|c| manga::is_tone_column(cut, c.id)).expect("the tone track").id;
            let s = h.state_mut();
            s.view.active_column = tone;
            s.new_drawing_at_frame();
            s.commit_stroke(stroke());
        }),
        reading(Done::Generator(GenKind::SpeedLines), "probe-film", |h| {
            wired(h, NodeKind::generator(GenKind::FocusLines));
        }, |h| {
            wired(h, NodeKind::generator(GenKind::SpeedLines));
        }),
        Reading {
            set: |h| {
                let id = wired(h, NodeKind::generator(GenKind::Text));
                node_edit(h, id, typed(""));
            },
            ..reading(Done::Words, "probe-film", |h| {
                let id = the_text(h);
                node_edit(h, id, typed("   "));
            }, |h| {
                let id = the_text(h);
                node_edit(h, id, typed("Kaboom"));
            })
        },
        reading(Done::PageSeen, "probe-page", |h| h.editor_mut().canvas.composite_view = true, |h| h.editor_mut().canvas.composite_view = false),
        // The sheet a generate leaves: a further key; a key replaced is none.
        Reading {
            set: |h| keyed(h, &[0, 1]),
            ..reading(Done::Lipped(0), "probe-film", |h| {
                let first = key(h, 0);
                cell(h, 1, first);
            }, |h| {
                let first = key(h, 0);
                cell(h, 2, first);
            })
        },
        // Blanks timed by the chart are not yet its inbetweens.
        Reading {
            set: |h| {
                keyed(h, &[0, 8]);
                let first = h.state().cut().drawings[0].id;
                h.state_mut().set_chart(first, &[0.25, 0.5, 0.75]);
            },
            ..reading(Done::Inbetweens, "probe-film", |h| assert!(h.state_mut().make_inbetweens(), "made"), |h| {
                for frame in [2, 4, 6] {
                    h.state_mut().goto(frame);
                    h.state_mut().commit_stroke(stroke());
                }
            })
        },
        Reading {
            set: |h| keyed(h, &[0, 4]),
            ..reading(Done::Chart, "probe-film", |h| {
                let id = h.state().cut().drawings[0].id;
                h.state_mut().set_drawing_kind(id, DrawingKind::Key);
            }, |h| {
                let id = h.state().cut().drawings[0].id;
                h.state_mut().set_chart(id, &[0.5]);
            })
        },
        // The dialog answered is the file landing: an unanswered one is a miss.
        Reading { set: |h| keyed(h, &[0]), ..reading(Done::Exported(1), "probe", export_asked, export_landed) },
        reading(Done::Preset("Comic"), "probe", |h| preset(h, "Other", 9.0), |h| preset(h, "Comic", 9.0)),
        reading(Done::Picked([1, 2, 3]), "probe", |h| h.editor_mut().canvas.brush_color = [200, 2, 3, 255], |h| h.editor_mut().canvas.brush_color = [1, 2, 3, 255]),
        reading(Done::Locked(true), "probe", |h| h.editor_mut().canvas.brush_width += 3.0, |h| h.editor_mut().canvas.alpha_lock = true),
        Reading {
            set: |h| panels(h, 3),
            ..reading(Done::PanelPicked(2), "probe-page", |h| h.state_mut().manga.selected = 1, |h| h.state_mut().manga.selected = 2)
        },
        Reading {
            set: |h| h.rig_mut().config.pen.stabilizer = 0,
            ..reading(Done::All(&[Done::Smoothing, Done::Strokes(1)]), "probe", |h| h.rig_mut().config.pen.stabilizer = 8, stroked)
        },
        reading(Done::Snap(false), "probe", |h| h.editor_mut().canvas.brush_width += 3.0, |h| h.state_mut().manga.snap = false),
        // A new brush with nothing raised is no scatter.
        reading(Done::Scattered("Comic"), "probe", |h| preset(h, "Comic", 9.0), |h| {
            let mut scattered = BrushPreset { name: "Scattered".into(), bank: "Comic".into(), ..Default::default() };
            scattered.engine = Some(crate::config::EngineDef { scatter: 1.0, ..Default::default() });
            h.rig_mut().config.presets.push(scattered);
        }),
        Reading {
            set: |h| {
                h.state_mut().add_column();
                h.state_mut().add_column();
                on_track(h, 0);
            },
            ..reading(Done::OnTrack(2), "probe-film", |h| on_track(h, 1), |h| on_track(h, 2))
        },
        // A direction alone leaves the page as black as it came.
        Reading {
            set: |h| wire_lines(h),
            ..reading(Done::LinesDown, "probe-film", |h| {
                let id = the_lines(h);
                node_edit(h, id, |p| p.set("angle", FxValue::Float(90.0)));
            }, |h| {
                let id = the_lines(h);
                node_edit(h, id, |p| {
                    p.set("angle", FxValue::Float(90.0));
                    p.set("count", FxValue::Float(30.0));
                });
            })
        },
    ]
}

/// Spec 5.4: each reader, by its table, both ways.
#[test]
fn aulesson_done_reads_the_state() {
    let rows = readings();
    let mut seen: Vec<usize> = rows.iter().map(|r| reader(r.done)).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen, (0..READERS).collect::<Vec<_>>(), "a reader with no row");
    for r in rows {
        let mut h = opened(r.room);
        (r.set)(&mut h);
        settle(&mut h);
        let step = h.editor().stage.unwrap_or(Stage::Drawing);
        run_on(&mut h, Task { done: r.done, step, ..NEVER });
        assert_eq!(at(&h), 0, "{:?} held before its act", r.done);
        (r.miss)(&mut h);
        settle(&mut h);
        assert_eq!(at(&h), 0, "{:?}: its miss ticked: {}", r.done, h.state().refusal);
        (r.hit)(&mut h);
        settle(&mut h);
        assert_eq!(at(&h), 1, "{:?}: its hit did not tick: {}", r.done, h.state().refusal);
    }
}

/// A run on `first`, its next task out of reach, on the document as it stands.
fn run_on(h: &mut EditorHarness, first: Task) {
    let tasks: &'static [Task] = Box::leak(Box::new([first, NEVER]));
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks, ..PROBE }, 0);
    settle(h);
}

/// Spec 5.4: a Play held on a blink is no watch; a whole pass is, by the
/// run's end and by a loop's wrap, and one that began on the last frame or
/// before the task did has to come round.
#[test]
fn aulesson_played_wants_a_whole_pass() {
    let film = || {
        let mut h = opened("probe-film");
        filmed(&mut h, 6);
        settle(&mut h);
        h
    };
    let watch = Task { done: Done::Played, step: Stage::Drawing, ..NEVER };
    let last = |h: &EditorHarness| h.state().play_range().1;

    // Pressed and stopped at once, from the top and from the middle.
    let mut h = film();
    assert_eq!(last(&h), 5, "the film's drawings fill the range");
    run_on(&mut h, watch);
    play_key(&mut h);
    h.run_frames(1);
    play_key(&mut h);
    settle(&mut h);
    assert_eq!(at(&h), 0, "Play pressed and stopped");
    play_key(&mut h);
    frames_until(&mut h, |h| h.state().view.frame == 3).expect("under way");
    assert_eq!(at(&h), 0, "under way");
    play_key(&mut h);
    settle(&mut h);
    assert_eq!(at(&h), 0, "stopped short: the stop's step back is no wrap");

    // The run to its end.
    h.state_mut().goto(0);
    settle(&mut h);
    pass(&mut h);
    assert_eq!(h.state().view.frame, 5, "it stopped on the last drawing");
    assert_eq!(at(&h), 1, "a pass to the end");

    // Round a loop: the tick comes while it plays.
    let mut h = film();
    h.state_mut().view.loop_playback = true;
    run_on(&mut h, watch);
    play_key(&mut h);
    frames_until(&mut h, |h| at(h) == 1).expect("a loop's pass ticks");
    assert!(h.state().view.playing, "with the loop still running");

    // Started on the last frame: the wrap begins the pass, it does not end it.
    let mut h = film();
    h.state_mut().view.loop_playback = true;
    run_on(&mut h, watch);
    h.state_mut().goto(5);
    h.state_mut().view.playing = true;
    frames_until(&mut h, |h| h.state().view.frame < 5).expect("it wraps");
    assert_eq!(at(&h), 0, "no pass has run yet");
    frames_until(&mut h, |h| at(h) == 1).expect("the next pass ticks");

    // Already going when the task opens: its end is not this task's.
    let mut h = film();
    h.state_mut().view.loop_playback = true;
    play_key(&mut h);
    frames_until(&mut h, |h| h.state().view.frame == 2).expect("under way");
    run_on(&mut h, watch);
    frames_until(&mut h, |h| h.state().view.frame < 2).expect("it wraps");
    assert_eq!(at(&h), 0, "the pass began before the task did");
    frames_until(&mut h, |h| at(h) == 1).expect("the next one is whole");
}

/// Spec 5.4: an erasure and a fill read the cel their task began on. A step
/// to a blank drawing or another layer is no erasure and no fill.
#[test]
fn aulesson_pixels_read_their_own_cel() {
    let mut h = opened("probe-film");
    keyed(&mut h, &[0, 1]);
    texels(&mut h, &smudge(0, 20), 0x3C00);
    settle(&mut h);
    run_on(&mut h, Task { done: Done::Erased(1), step: Stage::Drawing, ..NEVER });
    h.state_mut().goto(1);
    settle(&mut h);
    assert_eq!(at(&h), 0, "a step to the blank drawing erased nothing");
    h.state_mut().goto(0);
    settle(&mut h);
    h.state_mut().layer_add_role(LayerRole::Rough);
    settle(&mut h);
    assert_eq!(at(&h), 0, "nor one to another layer");

    let mut h = opened("probe-film");
    keyed(&mut h, &[0, 1]);
    settle(&mut h);
    let done = Done::Filled { seed: [100.0, 100.0], keep: None };
    run_on(&mut h, Task { done, step: Stage::Drawing, ..NEVER });
    h.state_mut().goto(1);
    settle(&mut h);
    texels(&mut h, &[(100, 100)], 0x3C00);
    fill_bump(&mut h);
    settle(&mut h);
    assert_eq!(at(&h), 0, "a fill on the next drawing is not this one's");
    h.state_mut().goto(0);
    texels(&mut h, &[(100, 100)], 0x3C00);
    fill_bump(&mut h);
    settle(&mut h);
    assert_eq!(at(&h), 1, "its own cel's");

    // A leak: the fill reached the pixel it had to leave alone.
    let mut h = opened("probe");
    settle(&mut h);
    let done = Done::Filled { seed: [100.0, 100.0], keep: Some([300.0, 300.0]) };
    run_on(&mut h, Task { done, step: Stage::Drawing, ..NEVER });
    texels(&mut h, &[(100, 100), (300, 300)], 0x3C00);
    fill_bump(&mut h);
    settle(&mut h);
    assert_eq!(at(&h), 0, "a fill that leaked");
}

/// Spec 5.4: `Exported` counts files landed on the lesson's latch, from the
/// task's start.
#[test]
fn aulesson_exported_counts_files_on_the_latch() {
    let mut h = opened("probe");
    keyed(&mut h, &[0]);
    settle(&mut h);
    export_asked(&mut h);
    export_landed(&mut h);
    run_on(&mut h, Task { done: Done::Exported(2), step: Stage::Drawing, ..NEVER });
    export_asked(&mut h);
    export_landed(&mut h);
    assert_eq!(at(&h), 0, "the file before the task is not the task's");
    export_asked(&mut h);
    export_landed(&mut h);
    assert_eq!(at(&h), 1, "two since it opened");

    let tasks: &'static [Task] = Box::leak(Box::new([Task { done: Done::Exported(1), step: Stage::Drawing, ..NEVER }, NEVER]));
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks, latch: Some(Latch::Transparent), ..PROBE }, 0);
    settle(&mut h);
    export_asked(&mut h);
    export_landed(&mut h);
    assert_eq!(at(&h), 0, "a file off the latch");
    h.state_mut().render_queue.panel.transparent = true;
    export_asked(&mut h);
    export_landed(&mut h);
    assert_eq!(at(&h), 1, "on it");
}

/// Spec 5.4: a brush saved over its own name with new numbers is a new brush;
/// a task on another bank never saw it.
#[test]
fn aulesson_preset_reads_its_bank() {
    let mut h = opened("probe");
    preset(&mut h, "Comic", 9.0);
    settle(&mut h);
    run_on(&mut h, Task { done: Done::Preset("Comic"), step: Stage::Drawing, ..NEVER });
    assert_eq!(at(&h), 0, "the bank as the task found it");
    h.rig_mut().config.presets.iter_mut().for_each(|p| p.size_px = 12.0);
    settle(&mut h);
    assert_eq!(at(&h), 1, "saved over with new numbers");
}

/// Spec 5.4: the misses: each landed an act, none landed the outcome.
#[test]
fn aulesson_misses_do_not_tick() {
    let mut h = opened("probe");
    let task = |done| Task { done, step: Stage::Drawing, ..NEVER };
    let run = |h: &mut EditorHarness, done| run_on(h, task(done));
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
    // One frame has no pass to watch.
    run(&mut h, Done::Played);
    assert_eq!(h.state().play_range().1, 0);
    h.state_mut().view.playing = true;
    settle(&mut h);
    assert_eq!(at(&h), 0, "a one-frame run");
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
    // The card's top line: it takes the title's place, over everything else.
    assert!(!h.painted_text().iter().any(|t| t == PROBE.title), "{:?}", h.painted_text());
    let view = card_view(&h);
    let (back, _) = h.painted_block(&word);
    assert_eq!(back.top(), view.top(), "the first thing on the card");
    let strip = h.rect(&format!("0 of {} done", TASKS.len()));
    assert!(back.bottom() <= strip.top());
    h.editor_mut().enter_room(Stage::Drawing);
    settle(&mut h);
    assert!(!h.painted_text().iter().any(|t| t.starts_with("Go back to")));
    assert!(h.painted_text().iter().any(|t| t == PROBE.title));
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
    w.lessons = probes();
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
    assert!(h.rig().requests.iter().any(|r| *r == crate::AppRequest::Start(WelcomeAction::Lesson(row("coat_of_shavings")))));
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

/// Spec 5.10: where `lesson` leaves the bar's walk: every pill pressed, in bar
/// order, each step's first task its pill, and nothing off the bar. Plan and
/// Draw both lens one stage, and a stage has one pill to walk.
fn bar_walked(lesson: &Lesson) -> Result<(), String> {
    let mut bar: Vec<Stage> = steps::bar(lesson.kind).iter().map(|s| s.stage).collect();
    bar.dedup();
    let mut walked = Vec::new();
    for t in lesson.tasks {
        if walked.last() == Some(&t.step) {
            continue;
        }
        if !walked.is_empty() && t.done != Done::OnStep(t.step) {
            return Err(format!("{}: the first task of a step is its pill: {}", lesson.id, t.say));
        }
        walked.push(t.step);
    }
    if walked == bar { Ok(()) } else { Err(format!("{}: walked {walked:?} of {bar:?}", lesson.id)) }
}

#[test]
fn aulesson_tasks_walk_the_bar() {
    for lesson in rules() {
        bar_walked(lesson).unwrap_or_else(|why| panic!("{why}"));
    }
    // The rule has teeth: a lesson past a pill does not walk, nor one out of order.
    let skip: Vec<Task> = TASKS.iter().filter(|t| t.step != Stage::Composite).copied().collect();
    let skipped = Lesson { tasks: Box::leak(skip.into_boxed_slice()), ..PROBE };
    assert!(bar_walked(&skipped).is_err(), "a pill skipped");
    let mut swap = TASKS.to_vec();
    swap.swap(1, 2);
    let swapped = Lesson { tasks: Box::leak(swap.into_boxed_slice()), ..PROBE };
    assert!(bar_walked(&swapped).is_err(), "two pills swapped");
    let short = Lesson { tasks: &TASKS[..1], ..PROBE };
    assert!(bar_walked(&short).is_err(), "a lesson that stops short of Export");
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

/// `text` holds `word`, whole, or its plural; a phrase, anywhere. A control's
/// token is no word of the sentence.
fn says(text: &str, word: &str) -> bool {
    let plain = lesson::resolve(text, |_| Some(String::new())).to_lowercase();
    if word.contains(' ') {
        return plain.contains(word);
    }
    let plural = |w: &str| [format!("{word}s"), format!("{word}es")].iter().any(|p| p == w);
    plain.split(|c: char| !c.is_alphanumeric() && c != '-').any(|w| w == word || plural(w))
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
    assert!(["frames", "layers", "keys", "holds", "loops"].iter().all(|w| says(w, &w[..w.len() - 1])), "and plurals");
}

/// Spec 5.1 keeps these for the hover cards: a reader there may see the trade's
/// own word. Nothing else in a lesson says them.
const ROMAJI: &[&str] = &["genga", "shiage", "satsuei", "henshu", "henshū", "shaku", "atari", "shusei", "shinko", "shinkō"];

/// The main spec's section 3, the left column, in full. `in` and `out` stay
/// out of the list: they are prepositions before they are the old loop keys.
const OLD: &[&str] = &[
    "room",
    "network room",
    "workspace",
    "layout",
    "manga pages",
    "brush forge",
    "cut",
    "cel",
    "column",
    "x-sheet",
    "xsheet",
    "onion",
    "lasso fill",
    "alpha lock",
    "lock",
    "comp",
    "composite",
    "composite view",
    "hardness",
    "aliased",
    "stabilizer",
    "memo",
    "library",
    "cel layers",
    "preset",
    "stack",
    "armed",
    "render queue",
    "node graph",
    "build satsuei graph",
    "build composite",
    "viewer",
    "layout canvas",
    "curves",
    "edit model",
    "role",
    "line guard",
    "lift key",
    "henshū timeline",
    "bind",
    "t.u.",
    "t.b.",
    "hold +",
    "hold −",
    "solo merged",
    "next key",
    "previous key",
    "scrub",
];

#[test]
fn aulesson_no_old_words() {
    for lesson in rules() {
        for text in spoken(lesson) {
            for word in OLD {
                assert!(!says(text, word), "{}: {word} in {text}", lesson.id);
            }
        }
        for text in spoken(lesson).into_iter().filter(|t| *t != lesson.hover) {
            for word in ROMAJI {
                assert!(!says(text, word), "{}: {word} in {text}", lesson.id);
            }
        }
    }
    assert!(says("scrubs", "scrub") && says("two cels", "cel") && says("the render queue", "render queue"));
    assert!(says("the genga pass", "genga"), "the romaji check itself");
}

/// Controls `WORDS` names that a later part builds. A word that lands in the
/// sources has to leave this list.
const LATER: [&str; 1] = ["Custom layer"];

/// Spec 5.3: `WORDS`' literals are the controls' own labels: each stands in
/// the app's sources as a label, or a later part owes it.
#[test]
fn aulesson_words_are_real_labels() {
    let sources = [
        include_str!("main.rs"),
        include_str!("rail.rs"),
        include_str!("config.rs"),
        include_str!("plate.rs"),
        include_str!("vocab.rs"),
        include_str!("playback.rs"),
        include_str!("manga_tools.rs"),
        include_str!("menus.rs"),
        include_str!("nodegraph_panel.rs"),
        include_str!("export_step.rs"),
        include_str!("../../crates/anim-core/src/generator.rs"),
    ]
    .concat();
    let labelled = |w: &str| {
        [format!("\"{w}\""), format!("/{w}\""), format!("\"{w} (")].iter().any(|label| sources.contains(label.as_str()))
    };
    for (token, word) in lesson::WORDS {
        // The card's own, and the run's: no control to pin them to.
        if [lesson::NIGHT, SHOW_ME, "panel 3"].contains(word) {
            continue;
        }
        // The transport's key speaks it in lower case, as the band's keys do.
        let label = if *word == "Play" { "play" } else { word };
        assert_eq!(labelled(label), !LATER.contains(word), "{{{token}}}: {word}");
    }
}

/// Build 6: the probe, from its page's card to its file, as a person walks it:
/// by the pen, by each pill, by the export's dialog.
#[test]
fn aulesson_probe_runs_to_its_file() {
    let dir = tempfile::tempdir().expect("dir");
    let data = dir.path().to_path_buf();
    let out = tempfile::tempdir().expect("dir");
    let still = out.path().join("line.png");
    let mut h = EditorHarness::with_dir_shipped(dir);
    open(&mut h, "probe");
    assert_eq!(at(&h), 0);
    pen(&mut h, &along([210.0, 205.0], [990.0, 205.0]));
    assert_eq!(at(&h), 1, "the line");
    for (i, pill) in [vocab::STEP_COLOR, vocab::STEP_EFFECTS, vocab::STEP_EXPORT].into_iter().enumerate() {
        h.click(pill);
        settle(&mut h);
        assert_eq!(at(&h), 2 + i, "{pill}");
    }
    export_asked(&mut h);
    settle(&mut h);
    assert_eq!(at(&h), 4, "the dialog is open, nothing landed");
    h.answer_export(&still);
    h.run_export_to_end();
    settle(&mut h);
    assert!(still.exists(), "the file");
    assert!(std::fs::metadata(&still).expect("its size").len() > 0);
    assert_eq!(at(&h), TASKS.len(), "the run is finished");
    assert_eq!(lesson::finished(Some(&data)), ["probe"], "lessons.txt holds it");
    assert!(h.painted_text().iter().any(|t| t == lesson::MADE), "{:?}", h.painted_text());
    assert!(h.painted_text().iter().any(|t| t == PROBE.closing), "{:?}", h.painted_text());
    assert!(h.has(lesson::NEXT), "{}", h.describe());
}

/// Spec 5.11: a clean lesson paper, and a file opened from the menu: a document
/// swapped under a run is not the run's. It does not tick on the file, write its
/// key into it or finish, and the card goes.
#[test]
fn aulesson_opened_file_is_left_alone() {
    let work = tempfile::tempdir().expect("dir");
    let file = work.path().join("mine.animproj");
    let mut other = EditorHarness::as_animation();
    keyed(&mut other, &[0, 1]);
    anim_core::store::save(&other.state().engine.project, &file).expect("saved");
    let on_disk = other.state().cut().drawings.len() as u32;
    assert!(on_disk >= 2);

    let dir = tempfile::tempdir().expect("dir");
    let data = dir.path().to_path_buf();
    let mut h = EditorHarness::with_dir_shipped(dir);
    open(&mut h, "probe");
    // A goal the opened file answers, the last task of the run.
    let one: &'static [Task] = Box::leak(Box::new([Task { done: Done::Drawings(on_disk), step: Stage::Drawing, ..NEVER }]));
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks: one, ..PROBE }, 0);
    settle(&mut h);
    assert_eq!(at(&h), 0);
    assert!(!h.state().is_dirty());
    h.editor_mut().set_recent(std::slice::from_ref(&file));
    h.open_path("File/Open recent");
    h.click(&crate::recent_label(&file));
    settle(&mut h);
    assert_eq!(h.state().file_path.as_deref(), Some(file.as_path()), "the file opened");
    assert!(h.editor().lesson.is_none(), "and the run ended");
    assert!(!h.has(LEAVE), "the card went");
    assert!(!h.state().engine.project.app_meta.contains_key(lesson::KEY), "no key in the person's file");
    assert!(!h.state().is_dirty(), "the file is as on disk");
    assert!(lesson::finished(Some(&data)).is_empty(), "no false finish");
}

/// Spec 5.11: Leave off a saved file marks it to save, and the file comes back
/// with no lesson.
#[test]
fn aulesson_left_lesson_does_not_resume() {
    let work = tempfile::tempdir().expect("dir");
    let file = work.path().join("mine.animproj");
    let mut h = opened("probe");
    pen(&mut h, &along([300.0, 300.0], [500.0, 300.0]));
    h.state_mut().file_path = Some(file.clone());
    h.click(LEAVE);
    settle(&mut h);
    assert!(h.state().meta_dirty, "the key's going is an edit to a saved file");
    anim_core::store::save(&h.state().engine.project, &file).expect("saved");
    let mut h = EditorHarness::as_shipped();
    h.answer_start(WelcomeAction::OpenRecent(file));
    settle(&mut h);
    assert!(h.editor().lesson.is_none(), "no card");
    assert!(!h.has(LEAVE));
}

/// Spec 5.11: a finished file is the person's: its card does not come back and
/// its key goes, and a file with a path says so.
#[test]
fn aulesson_finished_file_opens_free() {
    let work = tempfile::tempdir().expect("dir");
    let file = work.path().join("mine.animproj");
    let mut h = opened("probe");
    let end = format!("probe/{}", TASKS.len());
    h.state_mut().engine.project.app_meta.insert(lesson::KEY.into(), end);
    anim_core::store::save(&h.state().engine.project, &file).expect("saved");
    let mut h = EditorHarness::as_shipped();
    h.answer_start(WelcomeAction::OpenRecent(file));
    settle(&mut h);
    assert!(h.editor().lesson.is_none(), "no card");
    assert!(!h.state().engine.project.app_meta.contains_key(lesson::KEY), "the key has gone");
    assert!(h.state().meta_dirty, "and the file knows it");
}

/// Spec 5.11: a file that names a lesson this build lacks holds its kind too.
#[test]
fn aulesson_unknown_id_holds_promotion() {
    let mut h = opened("probe");
    h.editor_mut().lesson = None;
    h.state_mut().engine.project.app_meta.insert(lesson::KEY.into(), "from-the-future/2".into());
    h.perform(Action::AddDrawingAfter);
    h.state_mut().view.frame = 1;
    h.state_mut().commit_stroke(stroke());
    h.perform(Action::AddDrawingAfter);
    settle(&mut h);
    assert_eq!(ProjectKind::grown(&h.state().engine.project), Some(ProjectKind::Animation));
    assert_eq!(h.editor().kind(), Some(ProjectKind::Drawing), "the key holds it");
}

/// Spec 5.11: a list that cannot be read is no empty list: a finish does not
/// write over the ids in it.
#[test]
fn aulesson_broken_list_survives_a_finish() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join(lesson::FINISHED_FILE);
    let broken = [0xFF, 0xFE, 0x00];
    std::fs::write(&path, broken).expect("broken");
    let mut h = EditorHarness::with_dir_shipped(dir);
    let one: &'static [Task] = Box::leak(Box::new([task("", None, Stage::Drawing, Done::OnStep(Stage::Drawing), Show::Where)]));
    h.editor_mut().begin_lesson(Lesson { tasks: one, ..PROBE }, 0);
    settle(&mut h);
    assert!(h.editor().lesson.as_ref().is_some_and(|run| run.finished()), "the person's finish stands");
    assert_eq!(std::fs::read(&path).expect("the list"), broken, "and the file is as it was");
}

/// Spec 5.5, 5.6: the chip comes with a task that rings it, stays for the run,
/// and arms the lesson's ink.
#[test]
fn aulesson_night_chip_arms_the_ink() {
    let chip = format!("{}: {}", vocab::COLOR_WELL, lesson::NIGHT);
    let mut h = opened("probe");
    assert!(!h.has(&chip), "no task rings it");
    let rung = Task { ring: &[Target::Named(lesson::CHIP)], done: Done::OnStep(Stage::Drawing), step: Stage::Drawing, ..NEVER };
    let tasks: &'static [Task] = Box::leak(Box::new([NEVER, rung, NEVER]));
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks, ..PROBE }, 0);
    settle(&mut h);
    assert!(!h.has(&chip), "not before its task");
    h.editor_mut().lesson.as_mut().expect("a run").task = 1;
    settle(&mut h);
    assert_eq!(at(&h), 2, "the task ticked");
    assert!(h.has(&chip), "it stays: {}", h.describe());
    h.click(&chip);
    settle(&mut h);
    assert_eq!(h.editor().canvas.brush_color, plate::chip_bytes(plate::LESSON_NIGHT));
}

/// Spec 1.8, 8.5: a lesson runs with Hints off, and turning them off does not
/// touch it.
#[test]
fn aulesson_runs_with_hints_off() {
    let mut h = EditorHarness::as_shipped();
    h.rig_mut().config.ui.hints_off = true;
    settle(&mut h);
    open(&mut h, "probe");
    assert!(h.has(LEAVE), "the card stands");
    pen(&mut h, &along([300.0, 300.0], [500.0, 300.0]));
    assert_eq!(at(&h), 1, "and ticks");
    let mut h = opened("probe");
    h.rig_mut().config.ui.hints_off = true;
    settle(&mut h);
    assert!(h.has(LEAVE), "turned off under a run");
    pen(&mut h, &along([300.0, 300.0], [500.0, 300.0]));
    assert_eq!(at(&h), 1);
}

/// Spec 5.3: the raster brush a lesson needs, armed at the door, and the
/// person's switch given back on leaving. The Rig has no engine.
#[test]
#[ignore = "needs a GPU"]
fn aulesson_arms_a_raster_brush() {
    let mut h = EditorHarness::with_gpu_ready();
    h.editor_mut().canvas.raster = false;
    open(&mut h, "probe");
    assert!(h.editor().canvas.raster, "the rail's Fill and Eraser are there");
    h.click(LEAVE);
    settle(&mut h);
    assert!(h.editor().canvas.raster, "a fresh paper's switch is its own default");
    drop(h); // the device is one at a time: the next harness waits on this one
    let mut h = EditorHarness::with_gpu_ready();
    h.editor_mut().canvas.raster = false;
    h.editor_mut().begin_lesson(PROBE, 0);
    h.editor_mut().lesson_arm();
    settle(&mut h);
    assert!(h.editor().canvas.raster);
    h.click(LEAVE);
    settle(&mut h);
    assert!(!h.editor().canvas.raster, "the person's own");
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
