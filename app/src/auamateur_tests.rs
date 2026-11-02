//! AU24: the three Amateur lessons: what their files hold and what their art
//! promises (`research/LESSONS-approachable-ui.md`). The one gate walks them with
//! every other lesson; this reads what only these three say.

use std::path::PathBuf;

use anim_core::fx::FxKind;
use anim_core::graph::{Node, NodeKind};
use anim_core::manga::{self, Rulers};
use eframe::egui::{self, Pos2};

use crate::auguide_show_tests::{File, Hand, Picture, Ridden, at, carry, lift, ride, ride_tasks, settle};
use crate::auguide_tests::stand_on_the_tasks_before;
use crate::aulesson_tests::open;
use crate::aurail_tests::sized;
use crate::doc::path_cover;
use crate::lesson::{HOLD, KEY, LEAVE, SHOW_ME};
use crate::lessons::art;
use crate::lessons::{self, CORNER, Class, Done, Lesson, PathKind, Show, coat, coat_seeds};
use crate::plate;
use crate::render_queue::ExportKind;
use crate::steps::ProjectKind;
use crate::testkit::EditorHarness;
use crate::workspace::Stage;

const NEAR: i32 = 8;
/// The night's levels a shaving's width from a rim, past pure night (the lessons file).
const NIGHT_NEAR: i32 = 20;
/// The three rims' yellow, and the rim brush's pixels the lock keeps on the night:
/// with the lock off that is 6,400.
const RIM_PIXELS: i32 = 40_000;
const RIM_ON_NIGHT: i32 = 250_000;
const LINE_REACH: i32 = 6;

fn amateur(id: &str) -> Lesson {
    *lessons::ALL.iter().find(|l| l.id == id).expect("an Amateur lesson")
}

fn amateurs() -> Vec<Lesson> {
    lessons::ALL.iter().filter(|l| l.class == Class::Amateur).copied().collect()
}

fn near(a: [u8; 4], b: [u8; 3], by: i32) -> bool {
    (0..3).all(|i| (i32::from(a[i]) - i32::from(b[i])).abs() <= by)
}

fn bright(p: [u8; 4]) -> i32 {
    p[..3].iter().map(|c| i32::from(*c)).sum()
}

fn rgb(i: usize) -> [u8; 3] {
    let [r, g, b, _] = plate::STARTER_SWATCHES[i].1.to_array();
    [r, g, b]
}

const YELLOW: usize = 4;
const BROWN: usize = 9;

fn night() -> [u8; 3] {
    let [r, g, b, _] = plate::LESSON_NIGHT.to_array();
    [r, g, b]
}

/// `id` from the door to its file, by `hand`, at a laptop's size.
fn ridden(id: &str, hand: Hand) -> (EditorHarness, Ridden) {
    let lesson = amateur(id);
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, id);
    let ride = ride(&mut h, &lesson, hand);
    (h, ride)
}

fn still_of(ride: &Ridden) -> Picture {
    match ride.files.last().expect("a file") {
        File::One(ExportKind::Still, path) => Picture::png(path),
        _ => panic!("a still"),
    }
}

/// The picture again, with its Glow switched off: what the light added.
fn without_glow(h: &mut EditorHarness, dir: &std::path::Path) -> Picture {
    let under = |n: &Node| matches!(n.kind, NodeKind::Effect { fx: FxKind::Glow, .. }).then(|| n.inputs[0].expect("the Glow's picture"));
    let under = h.state().cut().graph.nodes().find_map(under).expect("the Glow");
    h.editor_mut().state.graph_set_output(under.0);
    h.run();
    let path = dir.join("no-glow.png");
    let ctx = h.ctx();
    h.editor_mut().export_to(&ctx, ExportKind::Still, &path);
    h.run_export_to_end();
    Picture::png(&path)
}

/// Spec AU24: six lessons stand, a second of each type, on papers no picker
/// offers, each sentence the lessons file's, and no Amateur task a `Guide`.
#[test]
fn auamateur_six_stand() {
    assert_eq!(lessons::ALL.len(), 6);
    let ids: Vec<&str> = amateurs().iter().map(|l| l.id).collect();
    assert_eq!(ids, ["coat_of_shavings", "still_in_the_coat", "too_much_coat"]);
    let kinds: Vec<ProjectKind> = amateurs().iter().map(|l| l.kind).collect();
    assert_eq!(kinds, [ProjectKind::Drawing, ProjectKind::Animation, ProjectKind::Comic]);
    let sizes: Vec<(u32, u32)> = amateurs().iter().map(|l| (l.paper.width, l.paper.height)).collect();
    assert_eq!(sizes, [(1400, 1050), (1024, 768), (888, 1243)]);
    let counts: Vec<usize> = amateurs().iter().map(|l| l.tasks.len()).collect();
    assert_eq!(counts, [18, 17, 12]);
    // Minutes a person can plan on, and what each ends in.
    let hovers: Vec<&str> = amateurs().iter().map(|l| l.hover).collect();
    let ends = ["About 20 minutes. Ends in a PNG.", "About 20 minutes. Ends in a looping GIF.", "About 25 minutes."];
    for (hover, ends) in hovers.iter().zip(ends) {
        assert!(hover.contains(ends), "{hover:?} for {ends:?}");
    }

    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../research/LESSONS-approachable-ui.md");
    let file = std::fs::read_to_string(file).expect("the lessons file").replace("\\|", "|");
    for lesson in amateurs() {
        let art = (lesson.art)();
        for task in lesson.tasks {
            let carried = task.guide.iter().map(|i| art[usize::from(*i)].kind);
            assert!(carried.into_iter().all(|k| k == PathKind::Landmark), "{}: {}: landmarks only", lesson.id, task.say);
            let trace = matches!(task.show, Show::Trace(_) | Show::Fill(_));
            assert!(!trace, "{}: {}: `Show me` never draws their art", lesson.id, task.say);
            for sentence in std::iter::once(task.say).chain(task.why) {
                assert!(file.contains(sentence), "{}: {sentence:?} is not the lessons file's", lesson.id);
            }
        }
        assert!(file.contains(lesson.closing), "{}: its closing", lesson.id);
        // Nothing a person draws is on the paper for them: no outline to trace.
        let shown = lesson.tasks.iter().flat_map(|t| t.guide.iter());
        assert!(shown.into_iter().all(|i| !matches!(art[usize::from(*i)].kind, PathKind::StandIn | PathKind::Guide)));
    }
}

/// Spec AU24 acceptance 3: the PNG's size, the sketch gone, Deckle's own line, the
/// night at its corner, and only the rims catching the light.
#[test]
fn auamateur_coat_file() {
    let (mut h, ride) = ridden("coat_of_shavings", Hand::ShowMe);
    let picture = still_of(&ride);
    assert_eq!(picture.size, (1400, 1050));
    let corner = picture.pixel(CORNER[0] as i32, CORNER[1] as i32);
    assert!(near(corner, night(), NEAR), "{corner:?} at the corner");

    let lesson = amateur("coat_of_shavings");
    let art = (lesson.art)();
    // Three shavings, and the space where two overlap: each seed stands in wood.
    let seeds: Vec<_> = art.iter().filter(|p| p.kind == PathKind::Seed).collect();
    assert_eq!(seeds.len(), 4);
    for seed in seeds {
        let at = seed.points[0];
        let px = picture.pixel(at[0] as i32, at[1] as i32);
        assert!(near(px, rgb(BROWN), 30), "{px:?} at {at:?}");
    }
    // The sketch: nothing of it past the shaving that moved over its corner.
    let sketch = lesson.tasks[0].stand_in[0];
    let sketch = &art[usize::from(sketch)];
    let on_it = sketch.points.iter().filter(|p| near(picture.pixel(p[0] as i32, p[1] as i32), night(), NIGHT_NEAR)).count();
    let mut dark = 0;
    for p in &sketch.points {
        let px = picture.pixel(p[0] as i32, p[1] as i32);
        dark += i32::from(bright(px) < 40);
    }
    assert!(on_it * 10 >= sketch.points.len() * 6, "{on_it} of {} under the night", sketch.points.len());
    assert!(dark * 10 <= sketch.points.len() as i32, "{dark} black samples: the sketch reached the file");
    // Deckle: a black line, a white body, off the color layer.
    let black = &art[usize::from(lessons::COAT_DECKLE)];
    let inked = black.points.iter().filter(|p| bright(picture.pixel(p[0] as i32, p[1] as i32)) < 3 * 90).count();
    assert!(inked * 10 >= black.points.len() * 5, "{inked} of {}", black.points.len());
    let white = picture.count([255, 255, 255]);
    assert!(white > 1000, "{white} white px");
    // The white stops at the line: under the feet there is night and no body.
    let reach = |pick: fn(&[f32; 3]) -> f32| black.points.iter().map(pick).fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let (left, right) = reach(|p| p[0]);
    let (_, feet) = reach(|p| p[1]);
    let mut below = 0;
    for y in feet as i32 + 6..feet as i32 + 60 {
        for x in left as i32 - 40..right as i32 + 40 {
            below += i32::from(picture.pixel(x, y)[..3].iter().all(|c| *c > 200));
        }
    }
    assert_eq!(below, 0, "{below} white px past Deckle's feet");

    // The light: a rim brighter than with Glow off, the night beside it not.
    let dir = tempfile::tempdir().expect("dir");
    let dark_picture = without_glow(&mut h, dir.path());
    let mut lit = 0;
    let mut off = 0;
    let rim_points: Vec<[f32; 3]> = lesson.finish.iter().map(|i| &art[usize::from(*i)]).filter(|p| p.width > 20.0).flat_map(|p| p.points.iter().copied()).collect();
    assert!(rim_points.len() > 300);
    let yellow = rgb(YELLOW);
    for p in &rim_points {
        // A rim runs half off the wood: its side of the wood is inward.
        for (dx, dy) in [(-8, 0), (8, 0), (0, -8), (0, 8)] {
            let (x, y) = (p[0] as i32 + dx, p[1] as i32 + dy);
            if near(dark_picture.pixel(x, y), yellow, 12) {
                off += 1;
                lit += i32::from(bright(picture.pixel(x, y)) > bright(dark_picture.pixel(x, y)));
            }
        }
    }
    assert!(off > 100, "{off} rim pixels");
    assert!(lit * 10 >= off * 8, "{lit} of {off} rim pixels lit");
    // A shaving's width out, the night stands as it was poured.
    let mut read = 0;
    for p in rim_points.iter().step_by(7) {
        for (dx, dy) in [(-120, 0), (120, 0), (0, -120), (0, 120)] {
            let (x, y) = (p[0] as i32 + dx, p[1] as i32 + dy);
            if near(dark_picture.pixel(x, y), night(), 2) {
                read += 1;
                let px = picture.pixel(x, y);
                assert!(near(px, night(), NIGHT_NEAR), "{px:?} at ({x}, {y})");
            }
        }
    }
    assert!(read > 50, "{read} night pixels read");
}

/// Spec AU24: `Keep inside strokes` holds rims that overrun their wood: no yellow
/// past the wood, and with the lock off the same rims would land past it.
#[test]
fn auamateur_rims_stay_inside() {
    let lesson = amateur("coat_of_shavings");
    let art = (lesson.art)();
    let i = lesson.tasks.iter().position(|t| t.done == Done::Strokes(3)).expect("the rims");
    let rims = lesson.tasks[i].stand_in;
    // By design each overruns its wood by at least 10 px.
    let mut overrun = 0;
    for r in rims {
        let path = &art[usize::from(*r)];
        assert!(path.width * 0.5 >= 10.0, "{} wide", path.width);
        overrun += 1;
    }
    assert_eq!(overrun, 3);

    let (mut h, ride) = ridden("coat_of_shavings", Hand::ShowMe);
    let picture = still_of(&ride);
    // The light spreads past the wood: the wood's edge is read with it off.
    let dir = tempfile::tempdir().expect("dir");
    let plain = without_glow(&mut h, dir.path());
    let wood = wood_of(&plain, &art);
    let yellow = rgb(YELLOW);
    let (w, tall) = (plain.size.0 as i32, plain.size.1 as i32);
    // The fill runs in under the line, and a speck of it can show past: that is
    // wood too. A rim's overrun stands 15 px out, and this reaches a line's 6.
    let by_the_line = |x: i32, y: i32| {
        let around = (-LINE_REACH..=LINE_REACH).flat_map(|dy| (-LINE_REACH..=LINE_REACH).map(move |dx| (x + dx, y + dy)));
        around.into_iter().any(|(x, y)| x >= 0 && y >= 0 && x < w && y < tall && wood[(y * w + x) as usize])
    };
    let (mut inside, mut outside) = (0, 0);
    for y in 0..tall {
        for x in 0..w {
            if near(plain.pixel(x, y), yellow, 40) {
                if wood[(y * w + x) as usize] {
                    inside += 1;
                } else if !by_the_line(x, y) {
                    outside += 1;
                }
            }
        }
    }
    assert!(inside > RIM_PIXELS, "{inside} px of rim");
    assert_eq!(outside, 0, "{outside} yellow px stand past the wood");
    // The rims' whole width would reach the night: the lock holds all of it back.
    let mut night_under = 0;
    for r in rims {
        let path = &art[usize::from(*r)];
        path_cover(&path.points, path.width, |x, y, cover| {
            night_under += i32::from(cover >= 1.0 && near(picture.pixel(x, y), night(), 30));
        });
    }
    assert!(night_under > RIM_ON_NIGHT, "{night_under} rim-brush px stand on the night: the rims overran");
}

/// The file's wood, as a mask: out from each fill's seed through the brown and the
/// yellow on it, as far as the black line and the night.
fn wood_of(picture: &Picture, art: &[lessons::Path]) -> Vec<bool> {
    const WOOD_RED: u8 = 100;
    let (w, tall) = (picture.size.0 as i32, picture.size.1 as i32);
    let mut wood = vec![false; (w * tall) as usize];
    let seeds = art.iter().filter(|p| p.kind == PathKind::Seed);
    let mut stack: Vec<(i32, i32)> = seeds.map(|s| (s.points[0][0] as i32, s.points[0][1] as i32)).collect();
    while let Some((x, y)) = stack.pop() {
        if x < 0 || y < 0 || x >= w || y >= tall || wood[(y * w + x) as usize] || picture.pixel(x, y)[0] <= WOOD_RED {
            continue;
        }
        wood[(y * w + x) as usize] = true;
        stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
    }
    wood
}

/// Spec AU24 acceptance 4: the GIF: its pictures, its running time, the rest
/// drawing back at row 16, where the shiver's run ends, and a desk in every frame.
#[test]
fn auamateur_still_in_the_coat_file() {
    let (h, ride) = ridden("still_in_the_coat", Hand::ShowMe);
    let File::One(ExportKind::Gif, path) = ride.files.last().expect("a file") else { panic!("a GIF") };
    assert_eq!(h.state().shown_length(), 32);
    let (pictures, length) = Picture::gif(path);
    assert_eq!(pictures[0].size, (1024, 768));
    assert_eq!(length, (32 * 100 + 6) / 12, "about 2.67 s");
    assert_eq!(pictures.len(), 4, "rest, left, right, rest: a held drawing is one GIF frame");
    let same = |a: &Picture, b: &Picture| a.rgba == b.rgba;
    assert!(same(&pictures[0], &pictures[3]), "the rest comes back");
    assert!(!same(&pictures[0], &pictures[1]) && !same(&pictures[1], &pictures[2]) && !same(&pictures[0], &pictures[2]), "three pictures");
    for picture in &pictures {
        let desk = picture.pixel(40, 720);
        assert!(bright(desk) < 3 * 110, "the desk: {desk:?}");
    }
    // Brown wood and yellow rims on each pose.
    for (pose, picture) in pictures.iter().take(3).enumerate() {
        let [wood, rim] = coat_seeds(pose);
        let (a, b) = (picture.pixel(wood[0] as i32, wood[1] as i32), picture.pixel(rim[0] as i32, rim[1] as i32));
        assert!(near(a, rgb(BROWN), 50), "pose {pose}'s wood: {a:?}");
        assert!(near(b, rgb(YELLOW), 70) || bright(b) > bright(a), "pose {pose}'s rim: {b:?}");
    }
    let bytes = std::fs::read(path).expect("the GIF");
    assert!(bytes.windows(19).any(|w| w == b"\x21\xFF\x0BNETSCAPE2.0\x03\x01\x00\x00\x00"), "it loops for ever");
}

/// Loop is the person's to set: a range in the middle of the shot changes what
/// they play, and the file is the same 32 frames with or without it.
#[test]
fn auamateur_the_loop_range_leaves_the_file_alone() {
    let (mut h, ride) = ridden("still_in_the_coat", Hand::ShowMe);
    let File::One(ExportKind::Gif, whole) = ride.files.last().expect("a file") else { panic!("a GIF") };
    let (pictures, length) = Picture::gif(whole);
    let state = h.state_mut();
    state.goto(8);
    state.set_loop_in();
    state.goto(20);
    state.set_loop_out();
    assert_eq!(state.loop_range(), Some((8, 20)), "a range from the middle of the shot");
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("mid-range.gif");
    let ctx = h.ctx();
    h.editor_mut().export_to(&ctx, ExportKind::Gif, &path);
    h.run_export_to_end();
    let (again, again_length) = Picture::gif(&path);
    assert_eq!(again_length, length, "the same running time");
    assert_eq!(again.len(), pictures.len(), "the same pictures");
    assert!(again.iter().zip(&pictures).all(|(a, b)| a.rgba == b.rgba), "picture for picture");
}

/// The lessons file's numbers on the sheet, and a loop that closes: `Show me` sets
/// the range round the whole shot. Any range ticks the task, and none reaches the
/// file (`auamateur_the_loop_range_leaves_the_file_alone`).
#[test]
fn auamateur_loop_closes() {
    let (mut h, _ride) = ridden("still_in_the_coat", Hand::ShowMe);
    let lesson = amateur("still_in_the_coat");
    assert!(h.state().view.loop_playback && h.state().loop_range().is_some(), "the ride's loop");
    let state = h.state();
    let column = &state.cut().xsheet.columns[usize::from(lessons::TRACK_A)];
    let keys: Vec<u32> = column.keys().map(|(at, _)| at).collect();
    assert_eq!(keys, [0, 12, 14, 15], "12, 2, 1, then the rest again: a drawing on a blank tail keeps to its row");
    assert_eq!(state.shown_length(), 32);
    assert_eq!(column.resolve(0), column.resolve(31), "the first and last rows show one drawing");
    assert!(column.resolve(0).is_some());

    // `Show me`, from no range, puts it round the shot.
    let Some(Show::Run(run)) = lesson.tasks.iter().find(|t| t.done == Done::Loop).map(|t| t.show) else { panic!("the loop's task") };
    h.state_mut().clear_loop();
    h.editor_mut().enter_room(Stage::Edit);
    settle(&mut h);
    h.state_mut().view.loop_playback = false;
    run(h.editor_mut());
    assert!(h.state().view.loop_playback);
    assert_eq!(h.state().loop_range(), Some((0, 31)), "round the shot");
}

/// The three poses' on-model read: the same wood, squeezed.
#[test]
fn auamateur_coat_keeps_its_wood() {
    let area = |points: &[art::Pt]| {
        let pairs = points.windows(2);
        pairs.map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1]).sum::<f32>().abs() * 0.5
    };
    let rest = area(&coat(0).edge(1.0));
    for pose in 1..3 {
        let squeezed = area(&coat(pose).edge(1.0));
        assert!((squeezed / rest - 1.0).abs() <= 0.05, "pose {pose}: {squeezed} of {rest}");
    }
    let lows: Vec<f32> = (0..3).map(|i| coat(i).edge(1.0).iter().fold(0.0, |low: f32, p| low.max(p[1]))).collect();
    assert!(lows.iter().all(|y| (y - lessons::STILL_FLOOR).abs() <= 3.0), "on the floor: {lows:?}");
}

/// Spec AU24 acceptance 5: two page files, six panels on the first, a ruler in
/// the second's meta and none in its file, and one brown region.
#[test]
fn auamateur_too_much_coat_files() {
    let (h, ride) = ridden("too_much_coat", Hand::ShowMe);
    let File::Pages(dir) = ride.files.last().expect("a file") else { panic!("the pages") };
    let pages = Picture::folder(dir);
    assert_eq!(pages.len(), 2, "two files");
    assert!(pages.iter().all(|p| p.size == (888, 1243)), "{:?}", pages.iter().map(|p| p.size).collect::<Vec<_>>());

    let state = h.state();
    let scene = state.engine.project.scene(state.at().scene).expect("the book");
    let [one, two] = &scene.cuts[..] else { panic!("two pages") };
    let panels = manga::panels_of(one).map_or(0, |(_, polys)| polys.len());
    assert!(panels >= 6, "{panels} panels on page one");
    assert_eq!(Rulers::of(two).vps.len(), 1, "page two's ruler");
    assert_eq!(Rulers::of(one).vps.len(), 0, "page one has none");

    // One brown region, the coat's, and it is the page's only colour.
    let brown = pages[1].count(rgb(BROWN));
    assert!(brown > 50_000, "{brown} brown px");
    assert_eq!(pages[0].count(rgb(BROWN)), 0, "page one spends none");
    let lesson = amateur("too_much_coat");
    let art = (lesson.art)();
    let seed = art.iter().find(|p| p.kind == PathKind::Seed).expect("the coat's seed").points[0];
    let reach = flood_brown(&pages[1], seed);
    assert!(reach * 10 >= brown * 9, "{reach} of {brown}: one region");
    // A ruler is a guide: the sky over the horizon stays paper.
    let sky = pages[1].pixel(444, 160);
    assert!(bright(sky) > 3 * 240, "{sky:?}: the rulers do not reach the file");
}

/// The brown region under `seed`, in px.
fn flood_brown(picture: &Picture, seed: [f32; 3]) -> usize {
    let (w, h) = (picture.size.0 as i32, picture.size.1 as i32);
    let mut seen = vec![false; (w * h) as usize];
    let mut stack = vec![(seed[0] as i32, seed[1] as i32)];
    let mut count = 0;
    while let Some((x, y)) = stack.pop() {
        if x < 0 || y < 0 || x >= w || y >= h || seen[(y * w + x) as usize] {
            continue;
        }
        seen[(y * w + x) as usize] = true;
        if near(picture.pixel(x, y), rgb(BROWN), 3) {
            count += 1;
            stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
        }
    }
    count
}

/// Spec AU24 acceptance 2: the finish stands on the paper only while `Hold to
/// see it` is held, and a task's own guide is a tick and nothing more.
#[test]
fn auamateur_hold_to_see_it_is_held() {
    let press = |h: &mut EditorHarness, at: Pos2, pressed: bool| {
        let button = egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        h.kit_mut().input_mut().events.extend([egui::Event::PointerMoved(at), button]);
        h.run_frames(3);
    };
    for lesson in amateurs() {
        let mut h = EditorHarness::as_shipped();
        sized(&mut h, 1280.0, 800.0);
        open(&mut h, lesson.id);
        let i = lesson.tasks.iter().position(|t| matches!(t.show, Show::Where) && !t.guide.is_empty()).unwrap_or(0);
        stand_on_the_tasks_before(&mut h, &lesson, i);
        h.editor_mut().begin_lesson(lesson, i);
        settle(&mut h);
        let idle = h.editor().canvas.guide.lines.len();
        assert!(idle <= lesson.tasks[i].guide.len(), "{}: {idle} lines idle", lesson.id);
        let rects = h.rects(HOLD);
        assert_eq!(rects.len(), 1, "{}: the card's third foot row", lesson.id);
        // A comic's guided task stands on its last page.
        let page = h.state().at().cut;
        let scene = h.state().engine.project.scene(h.state().at().scene).expect("the scene");
        let page = scene.cuts.iter().position(|c| c.id == page).expect("the open page");
        press(&mut h, rects[0].center(), true);
        assert_eq!(h.editor().canvas.guide.lines.len(), lesson.held(page).len(), "{}: the finish while held", lesson.id);
        press(&mut h, rects[0].center(), false);
        assert_eq!(h.editor().canvas.guide.lines.len(), idle, "{}: gone at the release", lesson.id);
    }
}

/// Spec AU24 acceptance 2: a comic's finish is two pages, and the hold shows the
/// one that is open: page one's six on page one, and the coat on page two.
#[test]
fn auamateur_hold_follows_the_open_page() {
    let lesson = amateur("too_much_coat");
    let art = (lesson.art)();
    let pencils = lesson.tasks.iter().position(|t| t.done == Done::Strokes(5)).expect("page two's task");
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, lesson.id);
    ride_tasks(&mut h, &lesson, 0..pencils, Hand::ShowMe);
    let spot = h.rects(HOLD)[0].center();
    let held = |h: &mut EditorHarness| {
        let button = |pressed| egui::Event::PointerButton { pos: spot, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        h.kit_mut().input_mut().events.extend([egui::Event::PointerMoved(spot), button(true)]);
        h.run_frames(3);
        let lines = h.editor().canvas.guide.lines.clone();
        h.kit_mut().input_mut().events.push(button(false));
        h.run_frames(3);
        lines
    };
    let shown = |lines: &[crate::canvas::GuideLine], want: &[u16]| {
        let first = |i: &u16| art[usize::from(*i)].points[0];
        let starts: Vec<[f32; 2]> = lines.iter().map(|l| [l.points[0].x, l.points[0].y]).collect();
        lines.len() == want.len() && want.iter().all(|i| starts.contains(&[first(i)[0], first(i)[1]]))
    };
    let [one, two] = lesson.pages else { panic!("two pages") };
    assert!(one.iter().chain(*two).all(|i| lesson.finish.contains(i)), "the two pages are the finish");
    assert_eq!(one.len() + two.len(), lesson.finish.len());
    let on_two = held(&mut h);
    assert!(shown(&on_two, two), "page two: {} lines", on_two.len());
    let at = h.state().at();
    let scene = h.state().engine.project.scene(at.scene).expect("the book");
    let first = scene.cuts[0].id;
    h.state_mut().goto_cut(at.scene, first);
    h.run_frames(3);
    let on_one = held(&mut h);
    assert!(shown(&on_one, one), "page one: {} lines", on_one.len());
    let second = h.state().engine.project.scene(at.scene).expect("the book").cuts[1].id;
    h.state_mut().goto_cut(at.scene, second);
    h.run_frames(3);
    let back = held(&mut h);
    assert!(shown(&back, two), "page two again: {} lines", back.len());
}

/// M1: `Select`'s move ticks on the person's own drag: a lifted piece that has
/// gone nowhere does not, the drag does, and the piece is set down.
#[test]
fn auamateur_the_drag_moves_the_piece() {
    let lesson = amateur("coat_of_shavings");
    let art = (lesson.art)();
    let i = lesson.tasks.iter().position(|t| t.done == Done::Moved).expect("the move");
    let &[lasso, drag] = lesson.tasks[i].stand_in else { panic!("a lasso and a drag") };
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, lesson.id);
    ride_tasks(&mut h, &lesson, 0..i, Hand::ShowMe);
    h.click(SHOW_ME);
    settle(&mut h);
    lift(&mut h, &art[usize::from(lasso)].points);
    assert_eq!(at(&h), i, "lifted, and gone nowhere");
    assert!(!h.editor().canvas.float_moved());
    carry(&mut h, &art[usize::from(drag)].points);
    settle(&mut h);
    assert_eq!(at(&h), i + 1, "the drag's end ticks the task");
    assert!(!h.editor().canvas.float_moved(), "and sets the piece down");
}

/// M2: `Press Play` on a looping sheet: it plays on while they watch, and the next
/// task stops it, or the canvas would take no pen in any room after. A stroke
/// lands at Color.
#[test]
fn auamateur_a_loop_stops_when_they_move_on() {
    let lesson = amateur("still_in_the_coat");
    let colour = lesson.tasks.iter().rposition(|t| t.step == Stage::InkPaint).expect("the fills");
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, lesson.id);
    ride_tasks(&mut h, &lesson, 0..colour, Hand::ShowMe);
    assert!(h.state().view.loop_playback, "the loop stays on");
    assert!(!h.state().view.playing, "the Color pill's task stopped it");
    let xf = h.editor().canvas.view_xform();
    let line = [(200.0, 200.0), (260.0, 240.0), (320.0, 210.0), (380.0, 250.0)];
    let path: Vec<Pos2> = line.iter().map(|&(x, y)| xf.paper_to_screen(egui::pos2(x, y))).collect();
    h.editor_mut().canvas.tool = crate::canvas::CanvasTool::Paint;
    h.run_frames(2);
    let strokes = h.state().strokes_done;
    h.drag_path(&path);
    assert_eq!(h.state().strokes_done, strokes + 1, "the pointer reaches the paper");
}

/// M3: no file holds `Keep inside strokes`: a resumed run has it on from the task
/// that turned it, and `Leave lesson` hands back what was there. So for the snap.
#[test]
fn auamateur_lock_and_snap_are_borrowed() {
    // The snap the run found: a new document's has it on.
    let resumed = |id: &str, task: usize, snap: bool| {
        let mut h = EditorHarness::as_first_run();
        sized(&mut h, 1280.0, 800.0);
        open(&mut h, id);
        let e = h.editor_mut();
        e.state.manga.snap = snap;
        e.begin_lesson(amateur(id), 0);
        e.state.engine.project.app_meta.insert(KEY.to_string(), format!("{id}/{task}"));
        e.state.untitled_group.push_str(" reopened");
        assert!(e.lesson_door(), "{id}: the run resumed");
        h
    };
    let lesson = amateur("coat_of_shavings");
    let locked = lesson.tasks.iter().position(|t| t.done == Done::Locked(true)).expect("the lock");
    let on_the_lock = resumed(lesson.id, locked, true);
    assert!(!on_the_lock.editor().canvas.alpha_lock, "not yet");
    let mut past_it = resumed(lesson.id, locked + 1, true);
    assert!(past_it.editor().canvas.alpha_lock, "the rims' task stands on the lock");
    past_it.click(LEAVE);
    past_it.run_frames(3);
    assert!(!past_it.editor().canvas.alpha_lock, "left as it was found");

    let lesson = amateur("too_much_coat");
    let on = lesson.tasks.iter().position(|t| t.say.contains("turn snap on")).expect("the ruler");
    let off = lesson.tasks.iter().position(|t| t.done == Done::Snap(false)).expect("snap off");
    let mut snapped = resumed(lesson.id, on + 1, false);
    assert!(snapped.state().manga.snap, "page two's pencils stand on the snap");
    snapped.click(LEAVE);
    snapped.run_frames(3);
    assert!(!snapped.state().manga.snap, "left as it was found");
    let mut free = resumed(lesson.id, off + 1, true);
    assert!(!free.state().manga.snap, "the coat's task stands free of it");
    free.click(LEAVE);
    free.run_frames(3);
    assert!(free.state().manga.snap, "and hands it back on");
}

/// B1: the coat curls: the snap the pencils stood on is off by the coat's task. A
/// stroke along its edge keeps its bend, and the same stroke snapped has none.
#[test]
fn auamateur_the_coat_curls() {
    let lesson = amateur("too_much_coat");
    let art = (lesson.art)();
    let task = lesson.tasks.iter().position(|t| t.done == Done::Snap(false)).expect("snap off") + 1;
    let edge: Vec<[f32; 3]> = art[usize::from(lesson.tasks[task].stand_in[0])].points.iter().step_by(4).take(24).copied().collect();
    let bend = |snap: Option<bool>| {
        let mut h = EditorHarness::as_shipped();
        sized(&mut h, 1280.0, 800.0);
        open(&mut h, lesson.id);
        ride_tasks(&mut h, &lesson, 0..task, Hand::ShowMe);
        h.rig_mut().config.pen.stabilizer = 0;
        if let Some(snap) = snap {
            h.state_mut().manga.snap = snap;
        }
        h.run_frames(2);
        assert_eq!(h.editor().canvas.snap_rulers.is_some(), snap == Some(true), "the ride leaves the snap off");
        let xf = h.editor().canvas.view_xform();
        let path: Vec<Pos2> = edge.iter().map(|p| xf.paper_to_screen(egui::pos2(p[0], p[1]))).collect();
        h.drag_path(&path);
        let tap = h.editor().canvas.stroke_tap.clone();
        assert!(tap.len() >= 6, "{} samples tapped", tap.len());
        let (a, z) = (tap[0], tap[tap.len() - 1]);
        let (dx, dy) = (z.x - a.x, z.y - a.y);
        let long = (dx * dx + dy * dy).sqrt().max(1.0);
        tap.iter().map(|s| ((s.x - a.x) * dy - (s.y - a.y) * dx).abs() / long).fold(0.0f32, f32::max)
    };
    let free = bend(None);
    assert!(free > 30.0, "the coat's edge bends {free} px");
    let held = bend(Some(true));
    assert!(held < 1.0, "the snap would have held it to {held} px");
}

/// Minor: only what the last task's own ring opened goes when the sentence turns
/// to the paper: a float the person opened stays.
#[test]
fn auamateur_only_the_rings_float_closes() {
    use crate::floats::Float;
    use crate::lessons::Target;
    let lesson = amateur("coat_of_shavings");
    let ringed = |t: &lessons::Task| t.ring.contains(&Target::Rail(crate::rail::Key::Layers));
    let i = (0..lesson.tasks.len() - 1).find(|i| ringed(&lesson.tasks[*i]) && lesson.tasks[i + 1].ring == [Target::Paper]).expect("a layer task, then the paper");
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    open(&mut h, lesson.id);
    ride_tasks(&mut h, &lesson, 0..i, Hand::ShowMe);
    h.editor_mut().floats.open(Float::Brushes, false);
    settle(&mut h);
    ride_tasks(&mut h, &lesson, i..i + 1, Hand::Pen);
    let floats = &h.editor().floats;
    assert!(!floats.is_open(Float::Layers), "the ring's own float goes");
    assert!(floats.is_open(Float::Brushes), "the person's stays");
}
