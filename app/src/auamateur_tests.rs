//! AU24: the three Amateur lessons: what their files hold and what their art
//! promises (`research/LESSONS-approachable-ui.md`). The one gate walks them with
//! every other lesson; this reads what only these three say.

use std::path::PathBuf;

use anim_core::fx::FxKind;
use anim_core::graph::{Node, NodeKind};
use anim_core::manga::{self, Rulers};
use eframe::egui::{self, Pos2};

use crate::auguide_show_tests::{File, Hand, Picture, Ridden, ride, settle};
use crate::auguide_tests::stand_on_the_tasks_before;
use crate::aulesson_tests::open;
use crate::aurail_tests::sized;
use crate::doc::path_cover;
use crate::lesson::HOLD;
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
    assert_eq!(counts, [18, 17, 11]);

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
                // Two differ from the file's, by AU24's notes: `cut` is a retired
                // word, and row 17 would leave the blank row 16 in the shot.
                let said = sentence.replace("split it into six panels", "cut six panels").replace("row 16", "row 17");
                assert!(file.contains(&said), "{}: {sentence:?} is not the lessons file's", lesson.id);
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
    let black = lesson.finish.iter().map(|i| &art[usize::from(*i)]).find(|p| p.kind == PathKind::Finish && p.width < 8.0 && p.points.len() > 200).expect("a line");
    let inked = black.points.iter().filter(|p| bright(picture.pixel(p[0] as i32, p[1] as i32)) < 3 * 90).count();
    assert!(inked * 10 >= black.points.len() * 5, "{inked} of {}", black.points.len());
    let white = picture.count([255, 255, 255]);
    assert!(white > 1000, "{white} white px");

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

    let (_h, ride) = ridden("coat_of_shavings", Hand::ShowMe);
    let picture = still_of(&ride);
    let yellow = rgb(YELLOW);
    let mut outside = 0;
    let mut inside = 0;
    for r in rims {
        let path = &art[usize::from(*r)];
        path_cover(&path.points, path.width, |x, y, cover| {
            if cover >= 1.0 && near(picture.pixel(x, y), yellow, 40) {
                // A yellow pixel out on the night: the wood stands on one side of
                // the edge, and the lock kept every other pixel out of it.
                let around = [(-4, 0), (4, 0), (0, -4), (0, 4), (-9, 0), (9, 0), (0, -9), (0, 9)];
                let wood = around.iter().any(|(dx, dy)| near(picture.pixel(x + dx, y + dy), rgb(BROWN), 40) || bright(picture.pixel(x + dx, y + dy)) > 3 * 150);
                if wood {
                    inside += 1;
                } else {
                    outside += 1;
                }
            }
        });
    }
    assert!(inside > 2000, "{inside} px of rim");
    assert_eq!(outside, 0, "{outside} yellow px stand clear of any wood");
    // The rims' whole width would reach the night: it holds none of it.
    let mut night_under = 0;
    for r in rims {
        let path = &art[usize::from(*r)];
        path_cover(&path.points, path.width, |x, y, cover| {
            night_under += i32::from(cover >= 1.0 && near(picture.pixel(x, y), night(), 30));
        });
    }
    assert!(night_under > 1000, "{night_under} rim-brush px stand on the night: the rims overran");
}

/// Spec AU24 acceptance 4: the GIF: its pictures, its running time, the rest
/// drawing back at row 17, and a desk in every frame.
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

/// The lessons file's numbers on the sheet, and a loop that closes: `Show me` sets
/// the range round the whole shot, and a range from mid-shot does not tick.
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
        press(&mut h, rects[0].center(), true);
        assert_eq!(h.editor().canvas.guide.lines.len(), lesson.finish.len(), "{}: the finish while held", lesson.id);
        press(&mut h, rects[0].center(), false);
        assert_eq!(h.editor().canvas.guide.lines.len(), idle, "{}: gone at the release", lesson.id);
    }
}
