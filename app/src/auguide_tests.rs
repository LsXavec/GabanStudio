//! AU21: the handles and the ring (spec 5.5, 5.8). A chain is walked the way a
//! person walks it: a click where the ring stands, and nowhere else, until the
//! last target has a rect.

use anim_core::model::LayerRole;
use eframe::egui::epaint::RectShape;
use eframe::egui::{self, Color32, Id, LayerId, Order, Rect, Shape, pos2, vec2};

use crate::aulesson_tests::{along, open, pen};
use crate::aurail_tests::sized;
use crate::canvas::extpaper_tests::paint_texels;
use crate::config::Action;
use crate::floats::Float;
use crate::lessons::{self, Class, Done, Lesson, Show, Target, Task};
use crate::rail::Key;
use crate::steps::ProjectKind;
use crate::testkit::EditorHarness;
use crate::workspace::Stage;
use crate::{Pane, plate, sample, steps, templates};

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
    run_of(h, &[task], class);
    h.editor_mut().enter_room(step);
    settle(h);
}

/// `tasks` as a run of this class, on the open document.
fn run_of(h: &mut EditorHarness, tasks: &[Task], class: Class) {
    let tasks: &'static [Task] = Box::leak(tasks.to_vec().into_boxed_slice());
    let kind = h.editor().kind().unwrap_or(ProjectKind::Drawing);
    h.editor_mut().begin_lesson(Lesson { kind, class, tasks, ..lessons::PROBED }, 0);
}

/// Clicks where the ring stands until `chain`'s last target has its rect, and
/// answers each place it stood. A chain that never gets there is an `Err`
/// naming the target, and so is a stop the ring was not stroked at.
fn walk(h: &mut EditorHarness, chain: &[Target]) -> Result<Vec<Target>, String> {
    let last = chain[chain.len() - 1];
    let mut stood = Vec::new();
    for _ in 0..=chain.len() + 2 {
        let Some((target, rect)) = h.ring_stands() else {
            return Err(format!("{last:?}: the ring stands nowhere, after {stood:?}"));
        };
        stood.push(target);
        if target != Target::Paper && h.ring_painted().is_none() {
            return Err(format!("{target:?} was not stroked at {rect:?}, after {stood:?}"));
        }
        if target == last {
            return Ok(stood);
        }
        h.click_at(rect);
        settle(h);
    }
    Err(format!("{last:?} never got a rect, after {stood:?}"))
}

/// The paper rings nothing: a chain that ends on it has no control to reach.
fn ends_on_paper(chain: &[Target]) -> bool {
    matches!(chain.last(), None | Some(Target::Paper))
}

/// Every task of every lesson of the gate, seated by its index in the lesson's
/// own document, in its own step: a pill's task on the step before it, or the
/// pill would already be pressed. What `answer` reads depends on no `Done`, so
/// each lesson that lands is walked with no new test code.
/// The tasks before `i`, each by its own `Show me`, no frame between them.
pub(crate) fn stand_on_the_tasks_before(h: &mut EditorHarness, lesson: &Lesson, i: usize) {
    let ctx = h.ctx();
    for (j, task) in lesson.tasks[..i].iter().enumerate() {
        let e = h.editor_mut();
        e.enter_room(task.step);
        e.begin_lesson(*lesson, j);
        if let Show::Trace(paths) = task.show {
            e.lesson_land(paths);
        } else {
            e.lesson_show(&ctx);
        }
        // A pass ends and the head goes home: no frame ran it here.
        if task.done == Done::Played {
            e.state.view.playing = false;
            e.state.goto(0);
        }
    }
    // The panes lay out what these left, before a ring asks them to scroll.
    if i > 0 {
        settle(h);
    }
}

fn each_task(size: (f32, f32), mut answer: impl FnMut(&mut EditorHarness, &Lesson, usize)) {
    for lesson in lessons::for_gate() {
        for (i, task) in lesson.tasks.iter().enumerate() {
            let mut h = EditorHarness::as_shipped();
            sized(&mut h, size.0, size.1);
            open(&mut h, lesson.id);
            // A task past the first stands on what the first ones drew: the
            // Export step offers nothing over a blank paper.
            if i > 0 {
                let e = h.editor_mut();
                e.canvas.slot_at_pen_down(true, &mut e.state).expect("a cel to draw on");
                paint_texels(&mut e.state, &[(10, 10)], [0.0, 0.0, 0.0, 1.0]);
            }
            // AU23: and on what they left: frame 7 of a strip, a page's panels.
            if !ends_on_paper(task.ring) {
                stand_on_the_tasks_before(&mut h, lesson, i);
            }
            let earlier = lesson.tasks[..i].iter().rev().map(|t| t.step).find(|s| *s != task.step);
            let room = if matches!(task.done, Done::OnStep(_)) { earlier } else { Some(task.step) };
            // A door refits its panes: a room already entered keeps its scroll.
            if let Some(room) = room.filter(|r| h.editor().stage != Some(*r)) {
                h.editor_mut().enter_room(room);
            }
            h.editor_mut().begin_lesson(*lesson, i);
            settle(&mut h);
            assert_eq!(h.editor().lesson.as_ref().map(|r| r.task), Some(i), "{} seats task {i}", lesson.id);
            answer(&mut h, lesson, i);
        }
    }
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
    /// A drawing with ink on it: what the Export step offers as one picture.
    Paint,
}

impl Doc {
    fn harness(self) -> EditorHarness {
        match self {
            Doc::Drawing => EditorHarness::as_shipped(),
            Doc::Plain => EditorHarness::as_animation(),
            Doc::Sample => EditorHarness::with_state_shipped(sample::build_sample()),
            Doc::Paint => {
                let mut h = EditorHarness::as_shipped();
                crate::aulesson_tests::texels(&mut h, &[(100, 100)], 0x3C00);
                h
            }
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
    (Doc::Sample, Stage::Drawing, &[named("menu.more")]),
    (Doc::Sample, Stage::Drawing, &[named("menu.more.panels")]),
    (Doc::Sample, Stage::Drawing, &[named("menu.animation")]),
    (Doc::Sample, Stage::Drawing, &[named("menu.animation.track")]),
    (Doc::Plain, Stage::Drawing, &[named("band.fold"), named("band.speed"), named("speed.medium")]),
    (Doc::Sample, Stage::Edit, &[named("band.loop"), named("band.fold"), Target::Action(Action::LoopIn)]),
    (Doc::Sample, Stage::Edit, &[named("band.loop")]),
    (Doc::Sample, Stage::Edit, &[Target::Cell { track: 0, frame: 0 }, named("band.longer")]),
    (Doc::Sample, Stage::InkPaint, &[named("band.sheet"), Target::Pane(Pane::XSheet), Target::Cell { track: 0, frame: 0 }]),
    (Doc::Sample, Stage::Drawing, &[named("band.fold"), named("band.sheet")]),
    // Below the fold of the sheet at the small windows: amateur task 8's cell,
    // with no ancestor. The sheet comes to it.
    (Doc::Sample, Stage::Edit, &[Target::Cell { track: 0, frame: 16 }]),
    (Doc::Sample, Stage::Export, &[named("export.more"), named("export.transparent")]),
    (Doc::Sample, Stage::Export, &[Target::Action(Action::ExportStill)]),
    // The same action on the lead card, and a lead the choice line never draws.
    (Doc::Paint, Stage::Export, &[Target::Action(Action::ExportStill)]),
    (Doc::Sample, Stage::Export, &[Target::Action(Action::ExportGif)]),
    (Doc::Drawing, Stage::Drawing, &[Target::Rail(Key::More), Target::Rail(Key::Color)]),
    (Doc::Drawing, Stage::Drawing, &[Target::Rail(Key::More), Target::Rail(Key::Hand)]),
    (Doc::Sample, Stage::Drawing, &[Target::Rail(Key::More), Target::Rail(Key::Flip)]),
    (Doc::Drawing, Stage::Drawing, &[Target::Step(Stage::InkPaint)]),
    (Doc::Drawing, Stage::Drawing, &[Target::Pane(Pane::Canvas)]),
    (Doc::Drawing, Stage::InkPaint, &[named("swatch.black")]),
    (Doc::Drawing, Stage::InkPaint, &[named("swatch.white")]),
    (Doc::Drawing, Stage::InkPaint, &[named("swatch.yellow")]),
    (Doc::Drawing, Stage::InkPaint, &[named("swatch.brown")]),
    (Doc::Drawing, Stage::InkPaint, &[named("swatch.red")]),
];

/// The `Named` and `menu.*` handles this part owns (spec 5.5). The walks must
/// stand on each, or its site is gone.
const OWNED: &[&str] = &[
    "band.fold", "band.speed", "speed.medium", "band.loop", "band.longer", "band.sheet", "strip.next",
    "layers.add", "layers.custom", "layers.eye", "layers.role", "brush.sketch", "brush.clean", "steady.row",
    "keep.row", "export.more", "export.transparent", "options.chart", "lip.generate", "menu.view",
    "menu.animation", "menu.animation.track", "menu.more", "menu.more.panels", "swatch.black", "swatch.white",
    "swatch.yellow", "swatch.brown", "swatch.red",
];

/// A control and the name it answers to, in the chain that reaches it.
const CONTROLS: &[(Doc, &[Target], &str)] = &[
    (Doc::Drawing, &[Target::Rail(Key::Brush)], "Brush"),
    (Doc::Drawing, &[Target::Rail(Key::Fill)], "Fill"),
    (Doc::Drawing, &[Target::Action(Action::FillTool)], "Fill"),
    (Doc::Drawing, &[Target::Step(Stage::InkPaint)], "Color"),
    (Doc::Drawing, &[named("menu.view"), Target::Action(Action::MirrorView)], "Mirror"),
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
        each_task(size, |h, lesson, i| {
            let task = lesson.tasks[i];
            if ends_on_paper(task.ring) {
                assert_eq!(h.ring_rect(), None, "{size:?} {} task {i}: the paper rings nothing", lesson.id);
                return;
            }
            if let Err(why) = walk(h, task.ring) {
                found.push(format!("{size:?} {} task {i}: {why}", lesson.id));
            } else if matches!(task.done, Done::OnStep(_)) {
                let rect = h.ring_rect().expect("the walk stopped on it");
                h.click_at(rect);
                settle(h);
                assert_eq!(h.editor().stage, Some(task.step), "{size:?}: the ringed pill is the step");
            }
        });
    }
    assert!(found.is_empty(), "{} findings:\n{}", found.len(), found.join("\n"));
}

/// Spec 5.8's second rule: stopping on an ancestor proves nothing, so the walk
/// clicks until the last target has a rect, and a chain that never gets there
/// fails by name. The lessons' own chains walk with the table's, so one cannot
/// drift from the other, and the table keeps the handles no lesson uses yet.
#[test]
fn auguide_ring_reaches_the_last_target() {
    let mut found = Vec::new();
    let mut stood: Vec<Target> = Vec::new();
    for size in SIZES {
        for (doc, step, chain) in CHAINS {
            let mut h = at(*doc, size);
            stand(&mut h, chain, *step);
            match walk(&mut h, chain) {
                Ok(places) => stood.extend(places),
                Err(why) => found.push(format!("{size:?} in {}: {why}", step.name())),
            }
        }
        each_task(size, |h, lesson, i| {
            let ring = lesson.tasks[i].ring;
            if !ends_on_paper(ring)
                && let Err(why) = walk(h, ring)
            {
                found.push(format!("{size:?} {} task {i}: {why}", lesson.id));
            }
        });
    }
    assert!(found.is_empty(), "{} findings:\n{}", found.len(), found.join("\n"));

    // Spec 5.5: every site this part owns answered, each by the handle alone.
    let mut h = EditorHarness::with_state_shipped(film().build());
    sized(&mut h, 1600.0, 900.0);
    strip_up_for_color(&mut h);
    stood.extend(h.ring_stands().map(|s| s.0));
    let missing: Vec<_> = OWNED.iter().filter(|handle| !stood.contains(&named(handle))).collect();
    assert!(missing.is_empty(), "no walk stood on {missing:?}");

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

    // The paper is found and rings nothing.
    const PAPER: &[Target] = &[Target::Rail(Key::Brush), Target::Paper];
    let mut h = at(Doc::Drawing, SIZES[0]);
    stand(&mut h, PAPER, Stage::Drawing);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(Target::Paper));
    assert_eq!(h.ring_rect(), None);

    // A row in a card behind the dot it stands on until then: the dot and the
    // row share the handle, so the walk stops on the dot, and the ring goes in.
    const ROLE: &[Target] = &[Target::Rail(Key::Layers), named("layers.role")];
    let mut h = at(Doc::Sample, SIZES[0]);
    stand(&mut h, ROLE, Stage::Drawing);
    assert_eq!(walk(&mut h, ROLE).expect("Layers opens"), ROLE);
    let dot = h.ring_rect().expect("the row's dot");
    h.click_at(dot);
    settle(&mut h);
    let row = h.ring_rect().expect("the card's row");
    assert_ne!(row, dot, "the ring walked into the card");
    assert!(row.width() > dot.width(), "the row, not the dot: {row:?} after {dot:?}");
    h.click_at(row);
    settle(&mut h);
    let layers = h.state().strip_layers();
    assert!(layers.iter().any(|(_, l)| l.props.role == LayerRole::Rough), "the ringed row is Rough");
}

/// Spec 5.2: `Show me where` rings the control, for this task alone, and that
/// is all it does. A task on the paper has nothing to ring and offers no button.
#[test]
fn auguide_show_me_where_rings_only_when_pressed() {
    use crate::lesson::{LEAVE, SHOW_WHERE};
    const KEY: &[Target] = &[Target::Rail(Key::Brush)];
    let task = Task { step: Stage::Drawing, ring: KEY, show: Show::Where, ..lessons::PROBED.tasks[0] };
    let never = Task { done: Done::Strokes(u32::MAX), ..task };
    let mut h = at(Doc::Drawing, SIZES[0]);
    run_of(&mut h, &[task, never], Class::Experienced);
    settle(&mut h);
    assert_eq!(h.ring_stands(), None, "the route is theirs");
    let leave = h.rect(LEAVE);
    h.click(SHOW_WHERE);
    settle(&mut h);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(KEY[0]), "pressed");
    assert!(h.ring_painted().is_some());

    pen(&mut h, &along([200.0, 200.0], [600.0, 500.0]));
    assert_eq!(h.editor().lesson.as_ref().map(|r| r.task), Some(1), "the tick");
    assert_eq!(h.ring_stands(), None, "the press was for that task");
    assert_eq!(h.ring_painted(), None);
    assert!(h.has(SHOW_WHERE), "and the next task offers it again");

    // A task that performs, in an Experienced lesson: the button only rings.
    let perform = Task { done: Done::Strokes(u32::MAX), show: Show::Perform(Action::RoomFinishing), ..task };
    let mut h = at(Doc::Drawing, SIZES[0]);
    run_of(&mut h, &[perform], Class::Experienced);
    settle(&mut h);
    h.click(SHOW_WHERE);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Drawing), "nothing performed");
    assert_eq!(h.ring_stands().map(|s| s.0), Some(KEY[0]));

    // The same press in a Rookie lesson's card does the deed: the return above
    // is what stopped it.
    let mut h = at(Doc::Drawing, SIZES[0]);
    run_of(&mut h, &[perform], Class::Rookie);
    settle(&mut h);
    h.click(crate::lesson::SHOW_ME);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::InkPaint), "Show me performs");

    // On the paper there is nothing to ring.
    const PAPER: &[Target] = &[Target::Paper];
    for class in [Class::Experienced, Class::Amateur] {
        let mut h = at(Doc::Drawing, SIZES[0]);
        stand_as(&mut h, PAPER, Stage::Drawing, class);
        assert!(!h.has(SHOW_WHERE), "{}", h.describe());
        assert_eq!(h.rect(LEAVE), leave, "and Leave lesson stays under the hand");
    }
}

/// A place off the spine the chain goes to is no wandering: Plate One sends the
/// person to the Brush maker, a room that takes the step away. Any other place
/// off the spine rings the way back.
#[test]
fn auguide_chain_stands_off_the_spine() {
    const FORGE: &[Target] = &[named("menu.more"), Target::Pane(Pane::Forge)];
    let mut h = at(Doc::Drawing, SIZES[0]);
    stand(&mut h, FORGE, Stage::Drawing);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(FORGE[0]), "the room is not open yet");
    h.editor_mut().open_brush_maker();
    settle(&mut h);
    assert_eq!(h.editor().stage, None);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(FORGE[1]), "the ring stands on the task's own pane");
    let words = h.painted_text().join(" ");
    assert!(!words.contains("Go back to"), "{words}");

    const KEY: &[Target] = &[Target::Rail(Key::Brush)];
    let mut h = at(Doc::Drawing, SIZES[0]);
    stand(&mut h, KEY, Stage::Drawing);
    h.editor_mut().open_brush_maker();
    settle(&mut h);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(Target::Step(Stage::Drawing)), "the way back");
    assert!(h.painted_text().join(" ").contains("Go back to"));

    // The canvas stands in every room: naming it is no reason to stay.
    const CANVAS: &[Target] = &[Target::Pane(Pane::Canvas)];
    let mut h = at(Doc::Drawing, SIZES[0]);
    stand(&mut h, CANVAS, Stage::Drawing);
    h.editor_mut().open_brush_maker();
    settle(&mut h);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(Target::Step(Stage::Drawing)), "its step holds that pane");
}

/// `layers.eye` and `layers.role` mean the art beside the armed layer: the
/// sketch below it first, then art above it, then the armed layer itself.
#[test]
fn auguide_ringed_layer_is_the_art_below() {
    let mut h = at(Doc::Paint, SIZES[0]);
    h.state_mut().layer_add_role(LayerRole::Custom);
    settle(&mut h);
    let layers: Vec<_> = h.state().strip_layers().iter().map(|(_, l)| l.props.clone()).collect();
    let ids: Vec<_> = h.state().strip_layers().iter().map(|(_, l)| l.id).collect();
    let inked = h.state().strip_layers().iter().position(|(_, l)| !l.raster.tiles.is_empty());
    let (top, middle, bottom) = (0, 1, 2);
    assert_eq!((layers.len(), inked), (3, Some(middle)), "top first, the ink in the middle");
    let ringed = |h: &mut EditorHarness, armed: usize| {
        h.state_mut().arm_layer(&layers[armed]);
        assert_eq!(h.state().active_layer_id(), Some(ids[armed]), "{}", layers[armed].name);
        crate::xsheet_panel::ringed_layer(h.state(), h.state().active_layer_id())
    };

    assert_eq!(ringed(&mut h, middle), Some(ids[middle]), "no art beside it: the armed layer stands for itself");
    assert_eq!(ringed(&mut h, top), Some(ids[middle]), "the art below");

    h.state_mut().arm_layer(&layers[top]);
    crate::aulesson_tests::texels(&mut h, &[(100, 100)], 0x3C00);
    assert_eq!(ringed(&mut h, middle), Some(ids[top]), "art above, none below");

    // Art on both sides: the sketch under the pen's layer, as the name says.
    h.state_mut().arm_layer(&layers[bottom]);
    crate::aulesson_tests::texels(&mut h, &[(100, 100)], 0x3C00);
    assert_eq!(ringed(&mut h, middle), Some(ids[bottom]), "the art below the armed layer, not the art above it");

    // And the ring stands on that row's eye, the lowest of the three.
    const EYE: &[Target] = &[Target::Rail(Key::Layers), named("layers.eye")];
    stand(&mut h, EYE, Stage::Drawing);
    walk(&mut h, EYE).expect("Layers opens");
    let eye = h.ring_rect().expect("the eye");
    let row = h.rect(&layers[bottom].name);
    assert!((eye.center().y - row.center().y).abs() < 12.0, "the ringed eye {eye:?} is on {row:?}");
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

    // A row a small window holds below its fold is brought in, and answers.
    const LIP: &[Target] = &[named("menu.animation"), named("menu.animation.track"), Target::Action(Action::LipFlap)];
    let mut h = at(Doc::Sample, POCKET);
    stand(&mut h, LIP, Stage::Drawing);
    walk(&mut h, LIP).expect("the row");
    assert!(egui::Popup::is_any_open(&h.ctx()));
    let row = h.ring_rect().expect("the row");
    h.click_at(row);
    settle(&mut h);
    assert!(!egui::Popup::is_any_open(&h.ctx()), "the press closes a menu that scrolled");
}

/// A new animation of twelve rows.
pub(crate) const fn film() -> templates::Template {
    templates::Template { kind: Some(ProjectKind::Animation), cut_frames: 12, ..templates::READY }
}

/// Ink on rows 4 and 7, the Color step's task on `strip.next`, and the strip up.
pub(crate) fn strip_up_for_color(h: &mut EditorHarness) {
    const NEXT: &[Target] = &[named("strip.next")];
    let line = along([100.0, 100.0], [300.0, 300.0]);
    for frame in [3, 6] {
        h.state_mut().view.frame = frame;
        settle(h);
        pen(h, &line);
    }
    stand(h, NEXT, Stage::InkPaint);
    // Open paper: the Color step has no sheet, so the strip stands there.
    assert!(!h.editor().dock.iter_all_tabs().any(|(_, p)| *p == Pane::XSheet), "the step's own recipe");
    h.editor_mut().strip_open = true;
    settle(h);
}

/// `strip.next` stands on the first sheet with ink and no color: a blank sheet
/// is passed over, a filled one too, and a tap lands on the run's first frame.
#[test]
fn auguide_strip_rings_the_next_to_color() {
    let mut h = EditorHarness::with_state_shipped(film().build());
    sized(&mut h, 1600.0, 900.0);
    strip_up_for_color(&mut h);
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

/// A wished cell comes into view once, however far below the fold, as a marked
/// row does. That moved this test's first assert, which had pinned the gap:
/// row 201 used to wait on `band.sheet`, whose press in Plan hides the sheet.
/// After that the person's own scroll stands: with the row scrolled off, the
/// chain falls back on its ancestor, and the ring is on the row as the sheet
/// scrolls it, clipped to what shows.
#[test]
fn auguide_cell_ring_follows_the_scroll() {
    const CELL: &[Target] = &[named("band.sheet"), Target::Cell { track: 0, frame: 200 }];
    let long = templates::Template { kind: Some(ProjectKind::Animation), cut_frames: 240, ..templates::READY };
    let mut h = EditorHarness::with_state_shipped(long.build());
    sized(&mut h, 1280.0, 800.0);
    stand(&mut h, CELL, Stage::Drawing);
    assert!(h.editor().in_plan(), "Plan's sheet");
    let (target, first) = h.ring_stands().expect("the row");
    assert_eq!(target, CELL[1], "row 201 was below the fold, and the sheet came to it");
    assert_eq!(h.state().view.frame, 0, "the cursor stayed where it was");

    h.wheel_at(first.center(), 3000.0);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(CELL[0]), "the person's scroll stands, and the chain falls back");
    h.run_frames(30);
    assert_eq!(h.ring_stands().map(|s| s.0), Some(CELL[0]), "and nothing pulls it back");

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

/// A row in a body that scrolls is brought into view once. A row that arrived
/// in view is met the same: scrolled off by the person, it stays off, and the
/// ring stands on what holds it.
#[test]
fn auguide_person_scroll_stands() {
    const SKETCH: &[Target] = &[Target::Float(Float::Brushes), named("brush.sketch")];
    let mut h = at(Doc::Drawing, POCKET);
    h.open_float(Float::Brushes);
    stand(&mut h, SKETCH, Stage::Drawing);
    let (target, row) = h.ring_stands().expect("the row");
    assert_eq!(target, SKETCH[1], "in view from the start");
    h.wheel_at(row.center(), -3000.0);
    let (target, rect) = h.ring_stands().expect("the float");
    assert_eq!(target, SKETCH[0], "scrolled off, and left there: {rect:?}");
    h.run_frames(30);
    assert_eq!(h.ring_stands(), Some((target, rect)), "nothing pulls it back");
}

/// The bare rects the frame stroked in this ink, at this width.
fn ring_strokes(h: &EditorHarness, ink: Color32, width: f32) -> Vec<RectShape> {
    let shapes = crate::austrip_tests::painted(h).into_iter();
    let rects = shapes.filter_map(|s| if let Shape::Rect(r) = s { Some(r) } else { None });
    rects.filter(|r| r.stroke.color == ink && r.stroke.width == width && r.fill == Color32::TRANSPARENT).collect()
}

/// The ring carries its own ground: 2 pt of `BLUE_DEEP` and a 1 pt `ISLAND`
/// halo outside it, a stadium round a pill. `BLUE_DEEP` alone is lost on the
/// `BLUE` of a pressed pill and the `STEEL` of the strip.
#[test]
fn auguide_ring_has_its_halo() {
    const PILL: &[Target] = &[Target::Step(Stage::InkPaint)];
    const KEY: &[Target] = &[Target::Rail(Key::Brush)];
    for (chain, pill) in [(PILL, true), (KEY, false)] {
        let mut h = at(Doc::Drawing, SIZES[0]);
        stand(&mut h, chain, Stage::Drawing);
        let round = h.ring_painted().expect("stroked");
        let line = round.expand(plate::RING_GAP);
        let blue = ring_strokes(&h, plate::BLUE_DEEP, 2.0);
        let blue = blue.iter().find(|r| r.rect == line).expect("the stroke");
        let halo = ring_strokes(&h, plate::ISLAND, 1.0);
        let halo = halo.iter().find(|r| r.rect == line.expand(2.0)).expect("the halo");
        let wanted = if pill { (line.height() / 2.0) as u8 } else { plate::R_CONTROL + plate::RING_GAP as u8 };
        assert_eq!(blue.corner_radius.nw, wanted, "{chain:?}");
        assert_eq!(halo.corner_radius.nw, wanted + 2, "{chain:?}");
    }
}

/// Spec's must-not-change: the Export step's own `panel.ring` is the mark it
/// was, on calm ground, with no halo; and the rail gained its marks and nothing
/// else (the key enum's visibility, the import, seven lines).
#[test]
fn auguide_panel_ring_and_rail_stand() {
    let mut h = at(Doc::Sample, SIZES[0]);
    h.editor_mut().enter_room(Stage::Export);
    settle(&mut h);
    h.state_mut().render_queue.panel.ring = true;
    settle(&mut h);
    let ring = h.state().render_queue.panel.ring_at.expect("a ring");
    let line = ring.shrink(2.0);
    let blue = ring_strokes(&h, plate::BLUE_DEEP, 2.0);
    let blue = blue.iter().find(|r| r.rect == line).expect("the 2 pt stroke, 2 clear of the control");
    assert_eq!(blue.corner_radius.nw, plate::R_CONTROL + plate::RING_GAP as u8);
    assert!(!ring_strokes(&h, plate::ISLAND, 1.0).iter().any(|r| r.rect == ring), "no halo");
    assert_eq!(h.ring_painted(), None, "and the lesson's ring has no part in it");

    let rail = include_str!("rail.rs");
    let read = |word: &str| rail.lines().filter(|l| l.contains(word)).count();
    assert_eq!(read("use crate::lessons::Target;"), 1);
    assert_eq!(read("plate::mark(ui, resp.interact_rect, Target::Rail("), 7);
    assert_eq!(read("Target::"), 7 + 1 - 1, "nothing else in the rail knows the ring");
    assert_eq!(read("lessons"), 1);
    assert_eq!(read("pub(crate) enum Key"), 1);
    assert_eq!(read("stroke_ring") + read("wish(") + read("mark_row") + read("first_sight"), 0);
}

/// An open menu or a dialog stands over the control and the ring waits under it
/// (it would cross the rows), and comes back with the control. It still knows
/// where it stands. A float over a pane leaves the rest of the pane to ring.
#[test]
fn auguide_ring_gives_way_to_what_covers_it() {
    const KEY: &[Target] = &[Target::Rail(Key::Brush)];
    let mut h = at(Doc::Drawing, SIZES[1]);
    stand(&mut h, KEY, Stage::Drawing);
    let key = h.ring_rect().expect("the key");
    assert_eq!(h.ring_painted(), Some(key));
    h.click(crate::vocab::MENU_FILE);
    settle(&mut h);
    let top = h.ctx().layer_id_at(key.center()).expect("the menu");
    assert!(top.order >= Order::Foreground, "the menu is over the key: {top:?}");
    assert_eq!(h.ring_rect(), Some(key), "it knows");
    assert_eq!(h.ring_painted(), None, "and paints nothing over the rows");
    h.key(egui::Key::Escape);
    settle(&mut h);
    assert_eq!(h.ring_painted(), Some(key), "back with the control");

    const CANVAS: &[Target] = &[Target::Pane(Pane::Canvas)];
    let mut h = at(Doc::Drawing, POCKET);
    h.open_float(Float::Brushes);
    stand(&mut h, CANVAS, Stage::Drawing);
    assert!(h.ring_painted().is_some(), "a float covers a part of the pane");
}

/// A control flush with the window keeps every side of its ring: the rect is
/// held inside by the gap, the stroke and the halo, and the place it stands
/// stays the control's own. The docked panes keep 6 pt of margin today, so this
/// is the plate's own frame, with the window itself as the control.
#[test]
fn auguide_ring_stays_on_the_window() {
    let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
    let target = Target::Pane(Pane::Canvas);
    let ctx = egui::Context::default();
    ctx.begin_pass(egui::RawInput { screen_rect: Some(window), ..Default::default() });
    plate::wish(&ctx, vec![target]);
    plate::mark_on(&ctx, LayerId::background(), window, target);
    plate::stroke_ring(&ctx);
    let _ = ctx.end_pass();
    let reach = plate::RING_GAP + 2.0 + 1.0;
    let painted = plate::ring_painted(&ctx).expect("stroked");
    assert_eq!(plate::ring_stands(&ctx), Some((target, window)), "it stands on all of the control");
    assert_eq!(painted, window.shrink(reach), "every side of the ring and its halo is on the window");
}

/// Spec 5.5: the sites that come in families, each reached by its handle: the
/// floats, the pane of every step, the pills of every bar, every row a menu
/// files. And a site is found by what it is: no mark reads a word, or a rename
/// would lose the ring.
#[test]
fn auguide_every_site_is_marked() {
    fn leak(targets: Vec<Target>) -> &'static [Target] {
        Box::leak(targets.into_boxed_slice())
    }
    for float in Float::ALL {
        let mut h = at(Doc::Sample, SIZES[0]);
        h.open_float(float);
        stand(&mut h, leak(vec![Target::Float(float)]), Stage::Drawing);
        assert_eq!(h.ring_stands().map(|s| s.0), Some(Target::Float(float)));
    }

    let comic = templates::Template { kind: Some(ProjectKind::Comic), ..templates::READY };
    let docs = [
        EditorHarness::as_shipped(),
        EditorHarness::with_state_shipped(sample::build_sample()),
        EditorHarness::with_state_shipped(comic.build()),
    ];
    for mut h in docs {
        sized(&mut h, SIZES[0].0, SIZES[0].1);
        let kind = h.editor().kind().expect("a document");
        let bar = steps::bar(kind);
        for step in bar {
            h.editor_mut().enter_room(step.stage);
            let shown = crate::visible_panes(&h.editor().dock);
            assert!(!shown.is_empty());
            let panes = leak(shown.iter().map(|p| Target::Pane(*p)).collect());
            stand(&mut h, panes, step.stage);
            let found = plate::ring_found(&h.ctx());
            assert!(panes.iter().all(|p| found.contains(p)), "{kind:?} {}: {found:?} of {panes:?}", step.label);
        }
        let pills = leak(bar.iter().map(|s| Target::Step(s.stage)).collect());
        stand(&mut h, pills, bar[0].stage);
        let found = plate::ring_found(&h.ctx());
        assert!(pills.iter().all(|p| found.contains(p)), "{kind:?}: {found:?}");
    }

    let actions = leak(Action::ALL.iter().map(|a| Target::Action(*a)).collect());
    let mut h = at(Doc::Sample, SIZES[0]);
    stand(&mut h, actions, Stage::Drawing);
    let mut filed = 0;
    // Each menu, then each submenu: a row is marked where it stands. The
    // Steps rows a bar lacks stand nowhere.
    let bar = |action: &Action| {
        steps::row_of(*action).is_none_or(|step| steps::bar(ProjectKind::Animation).iter().any(|on| on.label == step.label))
    };
    for place in crate::menus::places() {
        h.open_path(place);
        settle(&mut h);
        let found = plate::ring_found(&h.ctx());
        for action in Action::ALL.iter().filter(|a| bar(a)) {
            let path = action.menu().unwrap_or("");
            if path.rsplit_once('/').is_some_and(|(parent, _)| parent == place) {
                filed += 1;
                assert!(found.contains(&Target::Action(*action)), "{path}");
            }
        }
        h.close_menus();
    }
    assert!(filed > 150, "the menus' rows were read: {filed}");

    const WORDS: [&str; 4] = ["== \"", ".as_str()", ".contains(\"", ".starts_with(\""];
    let sources = [
        ("canvas.rs", include_str!("canvas.rs")),
        ("export_step.rs", include_str!("export_step.rs")),
        ("floats.rs", include_str!("floats.rs")),
        ("lesson.rs", include_str!("lesson.rs")),
        ("main.rs", include_str!("main.rs")),
        ("menus.rs", include_str!("menus.rs")),
        ("playback.rs", include_str!("playback.rs")),
        ("rail.rs", include_str!("rail.rs")),
        ("strip.rs", include_str!("strip.rs")),
        ("xsheet_panel.rs", include_str!("xsheet_panel.rs")),
    ];
    let mut sites = 0;
    for (name, text) in sources {
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let code = line.trim_start();
            let site = ["plate::mark", "mark_word(", "mark_narrow("].iter().any(|w| code.contains(w));
            if !site || code.starts_with("//") || code.starts_with("pub fn") {
                continue;
            }
            sites += 1;
            // Stored preset keys, the same as `Done::Armed` reads: no display word.
            if code.starts_with("\"atari\"") || code.starts_with("\"genga\"") {
                continue;
            }
            for near in &lines[i.saturating_sub(2)..=i] {
                let read = WORDS.iter().find(|w| near.contains(**w));
                assert!(read.is_none(), "{name}:{}: a site reads a word: {near}", i + 1);
            }
        }
    }
    assert!(sites >= 30, "the sites were read: {sites}");
}

/// Saved data is sacred: a walk changes neither the document's bytes nor the
/// config, and the ring's whole memory is a temp of the frame.
#[test]
fn auguide_ring_saves_nothing() {
    const BRUSHES: &[Target] = &[Target::Rail(Key::Brush), Target::Float(Float::Brushes), named("brush.clean")];
    let dir = tempfile::tempdir().unwrap();
    let saved = |h: &EditorHarness, name: &str| {
        let file = dir.path().join(name);
        anim_core::store::save(&h.state().engine.project, &file).expect("saved");
        std::fs::read(file).expect("read back")
    };
    let config = |h: &EditorHarness| serde_json::to_string(&h.kit().state().config).expect("a config");
    let mut h = at(Doc::Sample, SIZES[0]);
    stand(&mut h, BRUSHES, Stage::Drawing);
    let (bytes, settings) = (saved(&h, "before.animproj"), config(&h));
    assert_eq!(saved(&h, "again.animproj"), bytes, "a save is the same bytes twice");
    walk(&mut h, BRUSHES).expect("the row");
    h.run_frames(4);
    assert_eq!(saved(&h, "after.animproj"), bytes, "the document");
    assert_eq!(config(&h), settings, "the config");

    let block = |text: &'static str, from: &str, to: &str| {
        let start = text.find(from).expect("the block's head");
        &text[start..start + text[start..].find(to).expect("its end")]
    };
    let lessons = include_str!("lessons.rs");
    let plate = include_str!("plate.rs");
    let marks = block(plate, "/// AU21: a task's chain this frame", "/// AU20: a lesson on the Lessons page");
    assert!(!marks.contains("persisted") && !marks.contains("Serialize"), "the marks live in the frame's temp");
    assert!(marks.contains("get_temp_mut_or_default::<Marks>"));
    let target = block(lessons, "#[derive(Clone, Copy, PartialEq, Debug)]\npub enum Target", "}");
    assert!(!target.contains("Serialize"), "a `Target` is the lesson table's, never a file's");
    assert!(!include_str!("rail.rs").contains("Serialize"), "`rail::Key` either");
}

/// The working animator: no lesson, no ring, and a frame's bill the same on a
/// long cut as a short one (the marks are per control, not per cell), with the
/// walks only a lesson reads left unwalked. No key was added for any of it.
#[test]
fn auguide_no_lesson_pays_nothing() {
    use plate::{MARK_COST, SCAN_COST};
    // The sheet's frames, then the strip's over open paper, Layers up.
    fn bill(h: &mut EditorHarness) -> (usize, usize) {
        h.editor_mut().floats.open(Float::Layers, false);
        let read = |h: &mut EditorHarness| {
            settle(h);
            MARK_COST.set(0);
            SCAN_COST.set(0);
            h.run_frames(8);
            (MARK_COST.get(), SCAN_COST.get())
        };
        h.editor_mut().enter_room(Stage::InkPaint);
        let sheet = read(h);
        h.editor_mut().dock = Stage::Drawing.dock();
        h.editor_mut().strip_open = true;
        let strip = read(h);
        (sheet.0 + strip.0, sheet.1 + strip.1)
    }
    let cut = |rows: u32| {
        let long = templates::Template { kind: Some(ProjectKind::Animation), cut_frames: rows, ..templates::READY };
        let mut h = EditorHarness::with_state_shipped(long.build());
        sized(&mut h, 1600.0, 900.0);
        pen(&mut h, &along([100.0, 100.0], [300.0, 300.0]));
        h
    };
    let (mut short, mut long) = (cut(12), cut(240));
    let (short_bill, long_bill) = (bill(&mut short), bill(&mut long));
    assert!(short_bill.0 > 0, "the frames were read");
    assert_eq!(short_bill.0, long_bill.0, "a mark is a control's, not a cell's");
    assert_eq!((short_bill.1, long_bill.1), (0, 0), "no lesson, no walk");
    assert_eq!(short.ring_stands(), None);
    assert_eq!(short.ring_painted(), None);
    for step in steps::bar(ProjectKind::Animation) {
        short.editor_mut().enter_room(step.stage);
        settle(&mut short);
        assert_eq!(short.ring_stands(), None, "{}", step.label);
    }

    // The guards are what stands between: a wish walks.
    const WISHES: [&[Target]; 3] = [&[named("strip.next")], &[named("layers.eye")], &[named("layers.role")]];
    for chain in WISHES {
        let mut h = cut(12);
        stand(&mut h, chain, Stage::InkPaint);
        assert!(bill(&mut h).1 > 0, "{chain:?}");
    }

    assert!(!Action::ALL.iter().any(|a| format!("{a:?}").contains("Ring")), "no action, so no chord");
    let plate = include_str!("plate.rs");
    let start = plate.find("/// AU21: a task's chain this frame").expect("the marks");
    let end = plate.find("/// AU20: a lesson on the Lessons page").expect("their end");
    for word in ["input(", "input_mut(", "key_pressed", "consume_key"] {
        assert!(!plate[start..end].contains(word), "the ring reads no key: {word}");
    }
}