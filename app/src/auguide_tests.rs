//! AU21: the handles and the ring (spec 5.5, 5.8). A chain is walked the way a
//! person walks it: a click where the ring stands, and nowhere else, until the
//! last target has a rect.

use anim_core::model::LayerRole;
use eframe::egui::{Id, LayerId, Order, Rect, pos2, vec2};

use crate::aulesson_tests::{along, open, pen};
use crate::aurail_tests::sized;
use crate::config::Action;
use crate::floats::Float;
use crate::lessons::{self, Class, Done, Lesson, Show, Target, Task};
use crate::rail::Key;
use crate::steps::ProjectKind;
use crate::testkit::EditorHarness;
use crate::workspace::Stage;
use crate::{Pane, sample, steps, templates};

/// The spec's four, its worst, and the one that comes to the Pocket tier.
const SIZES: [(f32, f32); 6] =
    [(1600.0, 900.0), (1280.0, 800.0), (1280.0, 720.0), (1024.0, 640.0), (832.0, 520.0), POCKET];
const POCKET: (f32, f32) = (853.0, 480.0);

const fn named(handle: &'static str) -> Target {
    Target::Named(handle)
}

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

/// `chain` as a run's one task, which never ticks.
pub(crate) fn stand(h: &mut EditorHarness, chain: &'static [Target], step: Stage) {
    stand_as(h, chain, step, Class::Rookie);
}

fn stand_as(h: &mut EditorHarness, chain: &'static [Target], step: Stage, class: Class) {
    let task = Task { step, done: Done::Strokes(u32::MAX), ring: chain, show: Show::Where, ..lessons::PROBED.tasks[0] };
    let tasks: &'static [Task] = Box::leak(Box::new([task]));
    let kind = h.editor().kind().unwrap_or(ProjectKind::Drawing);
    h.editor_mut().begin_lesson(Lesson { kind, class, tasks, ..lessons::PROBED }, 0);
    h.editor_mut().enter_room(step);
    settle(h);
}

/// Clicks where the ring stands until `chain`'s last target has its rect, and
/// answers each place it stood. A chain that never gets there is an `Err`
/// naming the target.
fn walk(h: &mut EditorHarness, chain: &[Target]) -> Result<Vec<Target>, String> {
    let last = chain[chain.len() - 1];
    let mut stood = Vec::new();
    for _ in 0..=chain.len() + 2 {
        let Some((target, rect)) = h.ring_stands() else {
            return Err(format!("{last:?}: the ring stands nowhere, after {stood:?}"));
        };
        stood.push(target);
        if target == last {
            return Ok(stood);
        }
        h.click_at(rect);
        settle(h);
    }
    Err(format!("{last:?} never got a rect, after {stood:?}"))
}

/// What a chain stands on: which document, and which step.
#[derive(Clone, Copy)]
enum Doc {
    /// A new drawing.
    Drawing,
    /// A new animation, one drawing, on the plain band.
    Plain,
    /// The sample cut: drawings, on the full band.
    Sample,
}

impl Doc {
    fn harness(self) -> EditorHarness {
        match self {
            Doc::Drawing => EditorHarness::as_shipped(),
            Doc::Plain => EditorHarness::as_animation(),
            Doc::Sample => EditorHarness::with_state_shipped(sample::build_sample()),
        }
    }
}

/// Every handle this part owns, in the chain a lesson finds it by (the lessons
/// file's, `Rail(More)` first wherever the Pocket tier holds the key back).
const CHAINS: &[(Doc, Stage, &[Target])] = &[
    (Doc::Drawing, Stage::Drawing, &[Target::Rail(Key::More), Target::Rail(Key::Size)]),
    (Doc::Sample, Stage::Drawing, &[Target::Rail(Key::More), Target::Rail(Key::Layers), named("layers.eye")]),
    (Doc::Sample, Stage::Drawing, &[Target::Rail(Key::More), Target::Rail(Key::Layers), named("layers.role")]),
    (Doc::Sample, Stage::Drawing, &[Target::Rail(Key::More), Target::Rail(Key::Layers), named("layers.add"), named("layers.custom")]),
    (Doc::Drawing, Stage::Drawing, &[Target::Rail(Key::Brush), Target::Float(Float::Brushes), named("keep.row")]),
    (Doc::Drawing, Stage::Drawing, &[Target::Rail(Key::Brush), Target::Float(Float::Brushes), named("brush.sketch")]),
    (Doc::Drawing, Stage::Drawing, &[Target::Rail(Key::Brush), Target::Float(Float::Brushes), named("brush.clean")]),
    (Doc::Drawing, Stage::Drawing, &[Target::Rail(Key::Brush), Target::Float(Float::Brushes), named("steady.row")]),
    (Doc::Drawing, Stage::Drawing, &[named("menu.view"), Target::Action(Action::MirrorView)]),
    (Doc::Sample, Stage::Drawing, &[named("menu.view"), Target::Action(Action::CanvasOptions), named("options.chart")]),
    (Doc::Sample, Stage::Drawing, &[named("menu.animation"), named("menu.animation.track"), Target::Action(Action::LipFlap), named("lip.generate")]),
    (Doc::Sample, Stage::Drawing, &[named("menu.more"), named("menu.more.panels"), Target::Action(Action::ShowCurves)]),
    (Doc::Plain, Stage::Drawing, &[named("band.fold"), named("band.speed"), named("speed.medium")]),
    (Doc::Sample, Stage::Edit, &[named("band.loop"), named("band.fold"), Target::Action(Action::LoopIn)]),
    (Doc::Sample, Stage::Edit, &[Target::Cell { track: 0, frame: 0 }, named("band.longer")]),
    (Doc::Sample, Stage::InkPaint, &[named("band.sheet"), Target::Pane(Pane::XSheet), Target::Cell { track: 0, frame: 0 }]),
    (Doc::Sample, Stage::Export, &[named("export.more"), named("export.transparent")]),
    (Doc::Sample, Stage::Export, &[Target::Action(Action::ExportStill)]),
    (Doc::Drawing, Stage::Drawing, &[Target::Step(Stage::InkPaint)]),
    (Doc::Drawing, Stage::Drawing, &[Target::Pane(Pane::Canvas)]),
];

/// A control and the name it answers to, in the chain that reaches it.
const CONTROLS: &[(Doc, &[Target], &str)] = &[
    (Doc::Drawing, &[Target::Rail(Key::Brush)], "Brush"),
    (Doc::Drawing, &[Target::Rail(Key::Fill)], "Fill"),
    (Doc::Drawing, &[Target::Action(Action::FillTool)], "Fill"),
    (Doc::Drawing, &[Target::Step(Stage::InkPaint)], "Color"),
    (Doc::Drawing, &[named("menu.view"), Target::Action(Action::MirrorView)], "Mirror view"),
    (Doc::Plain, &[named("band.fold"), named("band.speed"), named("speed.medium")], "Medium"),
    (Doc::Plain, &[Target::Action(Action::AddDrawingAfter)], "Add drawing (band)"),
];

/// The target's own rect, for the walk's stop: a ring's rect from the control.
fn inside(outer: Rect, inner: Rect) -> bool {
    outer.expand(1.0).contains_rect(inner)
}

fn at(doc: Doc, (w, tall): (f32, f32)) -> EditorHarness {
    let mut h = doc.harness();
    sized(&mut h, w, tall);
    h
}

/// The ring stands on the control itself: the rect the person sees, the one
/// that answers a click, at every size, for every task of every lesson, by the
/// driver's click alone.
#[test]
fn auguide_ring_stands_on_its_control() {
    let mut found = Vec::new();
    for size in SIZES {
        for (doc, chain, name) in CONTROLS {
            let mut h = at(*doc, size);
            stand(&mut h, chain, Stage::Drawing);
            match walk(&mut h, chain) {
                Ok(_) => {
                    let ring = h.ring_rect().expect("the walk stopped on it");
                    let rects: Vec<Rect> = h.rects(name);
                    if !rects.iter().any(|r| inside(ring, *r) && inside(*r, ring)) {
                        found.push(format!("{size:?} {name}: {ring:?} is none of {rects:?}"));
                    }
                }
                Err(why) => found.push(format!("{size:?}: {why}")),
            }
        }
        for lesson in lessons::for_gate() {
            let mut h = EditorHarness::as_shipped();
            sized(&mut h, size.0, size.1);
            open(&mut h, lesson.id);
            while let Some(task) = h.editor().lesson.as_ref().and_then(|run| lesson.tasks.get(run.task)) {
                let line = along([200.0, 200.0], [600.0, 500.0]);
                let last = task.ring.last().copied();
                match task.done {
                    Done::Strokes(_) => {
                        assert_eq!(h.ring_rect(), None, "{size:?}: the paper rings nothing");
                        pen(&mut h, &line);
                    }
                    Done::OnStep(_) => {
                        let (target, rect) = h.ring_stands().expect("a pill on the bar");
                        assert_eq!(Some(target), last, "{size:?}");
                        h.click_at(rect);
                        settle(&mut h);
                    }
                    _ => break,
                }
            }
        }
    }
    assert!(found.is_empty(), "{} findings:\n{}", found.len(), found.join("\n"));
}

/// Spec 5.8's second rule: stopping on an ancestor proves nothing, so the walk
/// clicks until the last target has a rect, and a chain that never gets there
/// fails by name.
#[test]
fn auguide_ring_reaches_the_last_target() {
    let mut found = Vec::new();
    for size in SIZES {
        for (doc, step, chain) in CHAINS {
            let mut h = at(*doc, size);
            stand(&mut h, chain, *step);
            if let Err(why) = walk(&mut h, chain) {
                found.push(format!("{size:?} in {}: {why}", step.name()));
            }
        }
    }
    assert!(found.is_empty(), "{} findings:\n{}", found.len(), found.join("\n"));

    // The walk itself: a name no site marks.
    const LOST: &[Target] = &[Target::Rail(Key::Brush), named("no.such")];
    let mut h = at(Doc::Drawing, SIZES[0]);
    stand(&mut h, LOST, Stage::Drawing);
    let why = walk(&mut h, LOST).expect_err("nothing marks it");
    assert!(why.contains("no.such"), "fails by name: {why}");
    const NOWHERE: &[Target] = &[named("no.such")];
    stand(&mut h, NOWHERE, Stage::Drawing);
    assert!(walk(&mut h, NOWHERE).expect_err("no ring").contains("no.such"));
}

/// R15: the ring is a stroke on a bare layer, so a pen pressed on it lands on
/// what is under it.
#[test]
fn auguide_ring_passes_the_pen() {
    const FLOAT: &[Target] = &[Target::Float(Float::Brushes)];
    let mut h = at(Doc::Drawing, SIZES[0]);
    h.open_float(Float::Brushes);
    stand(&mut h, FLOAT, Stage::Drawing);
    let ring = h.ring_rect().expect("the float is open");
    // On the stroke itself: the gap, then a line 2 wide.
    let on = pos2(ring.right() + crate::plate::RING_GAP + 1.0, ring.center().y);
    let layer = h.ctx().layer_id_at(on).expect("the pane is under it");
    assert_ne!(layer.order, Order::Tooltip, "no area of its own: {layer:?}");
    let ring_layer = LayerId::new(Order::Tooltip, Id::new("lesson_ring"));
    assert!(!h.ctx().memory(|m| m.layer_ids().any(|l| l == ring_layer && m.area_rect(l.id).is_some())));

    let host = h.editor().canvas.dbg_rect;
    assert!(host.contains(on), "the stroke of the ring stands over the canvas");
    let strokes = h.state().strokes_done;
    h.pen_down(on);
    (1..=6).for_each(|i| h.pen_move(on + vec2(i as f32 * 40.0, 0.0)));
    h.pen_up(on + vec2(240.0, 0.0));
    settle(&mut h);
    assert_eq!(h.state().strokes_done, strokes + 1, "the pen landed under the ring");
    assert_eq!(h.ring_rect(), Some(ring), "and the ring stands where it stood");
}

/// Ancestor first: the ring walks in as each opens, walks back out when one
/// shuts, and a closed fold's header stands for what it holds. The run decides
/// the rest: a step left rings the way back, Experienced rings only when asked,
/// and the paper rings nothing.
#[test]
fn auguide_ring_walks_the_chain() {
    const BRUSHES: &[Target] = &[Target::Rail(Key::Brush), Target::Float(Float::Brushes), named("brush.clean")];
    let mut h = at(Doc::Drawing, SIZES[0]);
    assert_eq!(h.ring_stands(), None, "no lesson, no ring");
    stand(&mut h, BRUSHES, Stage::Drawing);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(BRUSHES[0]));
    let stood = walk(&mut h, BRUSHES).expect("the Brushes float opens");
    assert_eq!(stood, [BRUSHES[0], BRUSHES[2]], "the float holds its row at once");
    h.editor_mut().floats.close(Float::Brushes);
    settle(&mut h);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(BRUSHES[0]), "back to the key");

    // A closed fold's header.
    const CHART: &[Target] = &[Target::Float(Float::CanvasOptions), named("options.chart")];
    let mut h = at(Doc::Sample, SIZES[0]);
    h.open_float(Float::CanvasOptions);
    stand(&mut h, CHART, Stage::Drawing);
    let (target, header) = h.ring_stands().expect("the header");
    assert_eq!(target, CHART[1]);
    h.click_at(header);
    settle(&mut h);
    let row = h.ring_rect().expect("the row");
    assert!(row.top() > header.bottom() - 1.0, "the row under the header, {row:?} after {header:?}");

    // Wandered: the pill, whatever the chain.
    const KEY: &[Target] = &[Target::Rail(Key::Brush)];
    let mut h = at(Doc::Drawing, SIZES[0]);
    stand(&mut h, KEY, Stage::Drawing);
    h.editor_mut().enter_room(Stage::InkPaint);
    settle(&mut h);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(Target::Step(Stage::Drawing)));

    // Experienced: only when pressed, and only for this task.
    let mut h = at(Doc::Drawing, SIZES[0]);
    stand_as(&mut h, KEY, Stage::Drawing, Class::Experienced);
    assert_eq!(h.ring_stands(), None, "the route is theirs");
    h.click(crate::lesson::SHOW_WHERE);
    settle(&mut h);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(KEY[0]), "pressed");

    // The paper is found and rings nothing.
    const PAPER: &[Target] = &[Target::Rail(Key::Brush), Target::Paper];
    let mut h = at(Doc::Drawing, SIZES[0]);
    stand(&mut h, PAPER, Stage::Drawing);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(Target::Paper));
    assert_eq!(h.ring_rect(), None);
}

/// Spec's acceptance 1: at the Pocket tier a held-back key rings `More` until
/// it opens. With room, `More` has no rect and the key rings at once.
#[test]
fn auguide_pocket_rings_more() {
    const SIZE: &[Target] = &[Target::Rail(Key::More), Target::Rail(Key::Size)];
    let mut h = at(Doc::Drawing, POCKET);
    stand(&mut h, SIZE, Stage::Drawing);
    assert!(h.has("More tools"), "Pocket\n{}", h.describe());
    assert_eq!(h.ring_stands().map(|s| s.0), Some(SIZE[0]), "the key waits behind the dots");
    assert_eq!(walk(&mut h, SIZE).expect("the dots open"), [SIZE[0], SIZE[1]]);

    let mut h = at(Doc::Drawing, SIZES[0]);
    stand(&mut h, SIZE, Stage::Drawing);
    assert!(!h.has("More tools"));
    assert_eq!(walk(&mut h, SIZE).expect("on the rail"), [SIZE[1]]);
}

/// Spec's acceptance 1: at 1024×640 a key behind the band's `…` rings `…` until
/// it opens. Over the fold width the key is on the band, and rings there.
#[test]
fn auguide_fold_rings_the_fold() {
    const SPEED: &[Target] = &[named("band.fold"), named("band.speed"), named("speed.medium")];
    let mut h = at(Doc::Plain, (1024.0, 640.0));
    stand(&mut h, SPEED, Stage::Drawing);
    pen(&mut h, &along([100.0, 100.0], [300.0, 300.0]));
    assert!(1024.0 < steps::FOLD);
    assert_eq!(walk(&mut h, SPEED).expect("the … opens"), [SPEED[0], SPEED[2]]);
    let medium = h.ring_rect().expect("the row");
    h.click_at(medium);
    settle(&mut h);
    assert_eq!(crate::strip::speed_of(h.state()), Some(2), "the ringed row is Medium");

    let mut h = at(Doc::Plain, (1280.0, 800.0));
    stand(&mut h, SPEED, Stage::Drawing);
    assert_eq!(walk(&mut h, SPEED).expect("the key opens"), [SPEED[1], SPEED[2]]);
}

/// Spec's acceptance 1: a menu row rings its menu word until it opens, and at
/// `narrow` the collapsed `Menu` word rings for it. The last click is the row's.
#[test]
fn auguide_menu_rings_the_menu() {
    const MIRROR: &[Target] = &[named("menu.view"), Target::Action(Action::MirrorView)];
    let mut h = at(Doc::Drawing, SIZES[0]);
    stand(&mut h, MIRROR, Stage::Drawing);
    let word = h.ring_rect().expect("the word");
    assert_eq!(walk(&mut h, MIRROR).expect("the menu opens"), MIRROR);
    assert!(word.height() > 0.0);
    let row = h.ring_rect().expect("the row");
    h.click_at(row);
    settle(&mut h);
    assert!(h.editor().canvas.view_mirrored(), "the ringed row is Mirror view");

    let mut h = at(Doc::Drawing, (1024.0, 640.0));
    assert!(steps::narrow(1024.0, false), "the words collapse here");
    stand(&mut h, MIRROR, Stage::Drawing);
    let closed = h.ring_rect().expect("the Menu word");
    let (head, _) = h.ring_stands().expect("stands");
    assert_eq!(head, MIRROR[0], "Menu marks every menu handle");
    assert_eq!(walk(&mut h, MIRROR).expect("Menu, then its row"), [MIRROR[0], MIRROR[0], MIRROR[1]]);
    let row = h.ring_rect().expect("the row");
    assert_ne!(row, closed, "the ring walked in");
    h.click_at(row);
    settle(&mut h);
    assert!(h.editor().canvas.view_mirrored(), "and it lands");
}

/// `strip.next` stands on the first sheet with ink and no color: a blank sheet
/// is passed over, a filled one too, and a tap lands on the run's first frame.
#[test]
fn auguide_strip_rings_the_next_to_color() {
    const NEXT: &[Target] = &[named("strip.next")];
    let film = templates::Template { kind: Some(ProjectKind::Animation), cut_frames: 12, ..templates::READY };
    let mut h = EditorHarness::with_state_shipped(film.build());
    sized(&mut h, 1600.0, 900.0);
    let line = along([100.0, 100.0], [300.0, 300.0]);
    for frame in [3, 6] {
        h.state_mut().view.frame = frame;
        settle(&mut h);
        pen(&mut h, &line);
    }
    stand(&mut h, NEXT, Stage::InkPaint);
    // Open paper: the strip stands where no sheet does, as the Color step has it.
    h.editor_mut().dock = Stage::Drawing.dock();
    h.editor_mut().strip_open = true;
    settle(&mut h);
    let first = h.ring_rect().expect("two inked drawings on the strip");
    h.state_mut().view.frame = 8;
    settle(&mut h);
    assert_eq!(h.ring_rect(), Some(first), "the first, wherever the cursor stands");
    h.click_at(first);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 3, "the run's first frame");
    h.state_mut().layer_add_role(LayerRole::Color);
    crate::aulesson_tests::texels(&mut h, &[(100, 100)], 0x3C00);
    settle(&mut h);
    let second = h.ring_rect().expect("the other drawing");
    assert!(second.left() > first.right() - 1.0, "the filled sheet is passed over");
}

/// A cell's ring is on the row as the sheet scrolls it, clipped to what shows,
/// and with the row off screen the chain falls back on its ancestor.
#[test]
fn auguide_cell_ring_follows_the_scroll() {
    const CELL: &[Target] = &[named("band.sheet"), Target::Cell { track: 0, frame: 200 }];
    let long = templates::Template { kind: Some(ProjectKind::Animation), cut_frames: 240, ..templates::READY };
    let mut h = EditorHarness::with_state_shipped(long.build());
    sized(&mut h, 1280.0, 800.0);
    stand(&mut h, CELL, Stage::Drawing);
    assert!(h.editor().in_plan(), "Plan's sheet");
    assert_eq!(h.ring_stands().map(|s| s.0), Some(CELL[0]), "row 201 is below the fold");

    h.state_mut().view.frame = 200;
    settle(&mut h);
    let (target, first) = h.ring_stands().expect("the row");
    assert_eq!(target, CELL[1], "the sheet came to the cursor");
    h.state_mut().view.frame = 196;
    settle(&mut h);
    h.state_mut().view.frame = 180;
    settle(&mut h);
    h.state_mut().view.frame = 215;
    settle(&mut h);
    let moved = h.ring_rect().expect("still on screen");
    assert_ne!(moved.top(), first.top(), "the ring went with the row");
    assert_eq!(moved.left(), first.left(), "and stayed on its track");
}
