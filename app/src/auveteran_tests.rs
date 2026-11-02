//! AU25: the three Experienced lessons: what their files hold, and that all nine
//! stand (`research/LESSONS-approachable-ui.md`). The one gate walks them with
//! every other lesson; this reads what only these three say.

use std::path::PathBuf;

use anim_core::graph::{Node, NodeKind};
use anim_core::generator::GenKind;
use anim_core::manga;

use crate::auguide_show_tests::{File, Hand, Picture, Ridden, iou, ride, settle, thumb};
use crate::auguide_tests::stand_on_the_tasks_before;
use crate::aulesson_tests::open;
use crate::aurail_tests::sized;
use crate::config::Action;
use crate::doc::path_cover;
use crate::lesson::SHOW_WHERE;
use crate::lessons::{self, Class, Done, Lesson, Path, PathKind, Show, Target};
use crate::render_queue::ExportKind;
use crate::steps::ProjectKind;
use crate::testkit::EditorHarness;

/// What `auguide_finishes_differ` asks of any two of the nine (spec AU25).
const IOU_MOST: f32 = 0.45;

fn veteran(id: &str) -> Lesson {
    *lessons::ALL.iter().find(|l| l.id == id).expect("an Experienced lesson")
}

fn veterans() -> Vec<Lesson> {
    lessons::ALL.iter().filter(|l| l.class == Class::Experienced).copied().collect()
}

fn ridden(id: &str, hand: Hand) -> (EditorHarness, Ridden) {
    let lesson = veteran(id);
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, id);
    let ride = ride(&mut h, &lesson, hand);
    (h, ride)
}

/// Spec AU25: nine stand, three of each type and class, each sentence the
/// lessons file's, nothing of theirs on the paper, and `Show me where` alone.
#[test]
fn auveteran_nine_stand() {
    assert_eq!(lessons::ALL.len(), 9);
    for kind in [ProjectKind::Drawing, ProjectKind::Animation, ProjectKind::Comic] {
        for class in Class::ALL {
            let cell = lessons::ALL.iter().filter(|l| l.kind == kind && l.class == class).count();
            assert_eq!(cell, 1, "{kind:?} {class:?}");
        }
    }
    let ids: Vec<&str> = veterans().iter().map(|l| l.id).collect();
    assert_eq!(ids, ["plate_one", "desk_to_lamp", "only_word"]);
    let kinds: Vec<ProjectKind> = veterans().iter().map(|l| l.kind).collect();
    assert_eq!(kinds, [ProjectKind::Drawing, ProjectKind::Animation, ProjectKind::Comic]);
    let sizes: Vec<(u32, u32)> = veterans().iter().map(|l| (l.paper.width, l.paper.height)).collect();
    assert_eq!(sizes, [(1500, 2000), (1024, 768), (2591, 3624)]);
    let (paper, table) = (veteran("only_word").paper, &crate::templates::ALL[lessons::MANGA_B5]);
    assert_eq!(table.name, "Manga B5");
    assert_eq!(paper.columns, table.columns, "the real table's tracks");
    assert_eq!(paper.page.map(|page| page()), table.page.map(|page| page()), "the real table's page");
    let counts: Vec<usize> = veterans().iter().map(|l| l.tasks.len()).collect();
    assert_eq!(counts, [12, 15, 11]);
    let ends = ["About 45 minutes. Ends in a transparent PNG.", "About 60 minutes. Ends in a PNG sequence and a GIF.", "About 60 minutes. Ends in a mono page PNG."];
    for (lesson, ends) in veterans().iter().zip(ends) {
        assert!(lesson.hover.contains(ends), "{:?} for {ends:?}", lesson.hover);
    }

    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../research/LESSONS-approachable-ui.md");
    let file = std::fs::read_to_string(file).expect("the lessons file").replace("\\|", "|");
    for lesson in veterans() {
        assert!(file.contains(lesson.closing), "{}: its closing", lesson.id);
        for task in lesson.tasks {
            assert!(task.guide.is_empty(), "{}: {}: nothing on the paper", lesson.id, task.say);
            assert!(matches!(task.show, Show::Where), "{}: {}: `Show me where` only", lesson.id, task.say);
            for sentence in std::iter::once(task.say).chain(task.why) {
                assert!(file.contains(sentence), "{}: {sentence:?} is not the lessons file's", lesson.id);
            }
        }
    }
}

/// Spec AU25, 3: the PNG leaves the paper bare and the roughs out of the file.
#[test]
fn auveteran_plate_one_file() {
    for hand in [Hand::ShowMe, Hand::Pen] {
        let (h, ride) = ridden("plate_one", hand);
        let [File::One(ExportKind::Still, path)] = ride.files.as_slice() else { panic!("one still") };
        let png = Picture::png(path);
        assert_eq!(png.size, (1500, 2000));
        for (x, y) in [(0, 0), (1499, 0), (0, 1999), (1499, 1999)] {
            assert_eq!(png.pixel(x, y)[3], 0, "{hand:?}: the corner at ({x}, {y}) is clear");
        }
        // A rough stands off every ink line, so all its pixels are its alone.
        let art = (veteran("plate_one").art)();
        let roughs: &[Path] = &art[6..12];
        let (mut rough, mut showing) = (0, 0);
        for path in roughs {
            path_cover(&path.points, path.width, |x, y, _| {
                rough += 1;
                showing += i32::from(png.pixel(x, y)[3] != 0);
            });
        }
        assert!(rough > 10_000, "{hand:?}: {rough} rough pixels were read");
        assert_eq!(showing, 0, "{hand:?}: of {rough} pixels only a rough was on");
        for ([x, y], _, tall) in lessons::PLATE_STAGES {
            let inside = png.pixel(x as i32, (y + tall * 0.55) as i32);
            assert_eq!(inside[3], 255, "{hand:?}: the stage at {y} is opaque: {inside:?}");
        }
        let cut = h.state().cut();
        let effect = crate::lesson::reached(&cut.graph).iter().any(|n| matches!(n, NodeKind::Effect { .. }));
        assert!(effect, "{hand:?}: an effect reaches Output");
    }
}

/// `pictures`' first and last differ by this many pixels.
fn apart(a: &Picture, b: &Picture) -> usize {
    a.rgba.chunks_exact(4).zip(b.rgba.chunks_exact(4)).filter(|(a, b)| a != b).count()
}

/// Spec AU25, 4: frames and a GIF of one count, the camera between the first and
/// the last, the dust, the wings past row 3, and no film.
#[test]
fn auveteran_desk_to_lamp_files() {
    let (mut h, ride) = ridden("desk_to_lamp", Hand::ShowMe);
    let [File::One(ExportKind::Png, frames), File::One(ExportKind::Gif, gif)] = ride.files.as_slice() else { panic!("frames and a GIF") };
    let pictures = Picture::folder(frames);
    assert!(pictures.len() >= 52, "{} frames", pictures.len());
    assert_eq!(pictures.len() as u32, h.state().shown_length());
    let (shown, length) = Picture::gif(gif);
    let fps = h.state().engine.project.fps;
    assert_eq!(length, pictures.len() as u32 * 100 / fps, "the GIF runs the frames' whole length ({} pictures)", shown.len());
    let camera = apart(&pictures[0], &pictures[pictures.len() - 1]);
    assert!(camera > 100_000, "the camera moved: {camera} pixels apart");

    let cut = h.state().cut();
    let wings = &cut.xsheet.columns[usize::from(lessons::TRACK_W)];
    assert!(wings.keys().any(|(frame, _)| frame > 2), "wing cells past row 3");
    let beats = wings.keys().count();
    assert!(beats > 6, "{beats} wing keys: the wings beat");

    // The same frame with the dust off: the picture under its own Over.
    let dust = |n: &Node| matches!(n.kind, NodeKind::Generator { generator: GenKind::Particles, .. }).then_some(n.id);
    let dust = cut.graph.nodes().find_map(dust).expect("the dust");
    let frame = pictures.len() - 1;
    h.state_mut().graph_remove_node(dust);
    settle(&mut h);
    let dir = tempfile::tempdir().expect("dir");
    let ctx = h.ctx();
    h.editor_mut().export_to(&ctx, ExportKind::Png, dir.path());
    h.run_export_to_end();
    let bare = Picture::folder(dir.path());
    let moved = apart(&pictures[frame], &bare[frame]);
    assert!(moved > 50, "the dust moved {moved} pixels");
}

/// Spec AU25, 5: one mono page, on its full size, panels, tone, a balloon and
/// the one word, and no page rebuilt by hand.
#[test]
fn auveteran_only_word_file() {
    let (h, ride) = ridden("only_word", Hand::ShowMe);
    let [File::Pages(dir)] = ride.files.as_slice() else { panic!("a folder of pages") };
    let pages = Picture::folder(dir);
    assert_eq!(pages.len(), 1, "one page");
    assert_eq!(pages[0].size, (2591, 3624));
    let grey = pages[0].rgba.chunks_exact(4).filter(|px| !(px[..3] == [0, 0, 0] || px[..3] == [255, 255, 255])).count();
    assert_eq!(grey, 0, "every pixel black or white");
    let black = pages[0].count([0, 0, 0]);
    assert!(black > 10_000, "{black} black pixels: the wall went to print");

    let state = h.state();
    let cut = state.cut();
    let panels = manga::panels_of(cut).map_or(0, |(_, polys)| polys.len());
    assert!(panels >= 5, "{panels} panels");
    let tone = &cut.xsheet.columns[usize::from(lessons::TRACK_TONE)];
    assert!(manga::is_tone_column(cut, tone.id), "the TONE track");
    assert!(tone.keys().next().is_some(), "tone on the TONE track");
    let reached = crate::lesson::reached(&cut.graph);
    assert!(reached.iter().any(|n| matches!(n, NodeKind::Generator { generator: GenKind::Balloon, .. })), "a balloon");
    let word = |n: &&NodeKind| matches!(n, NodeKind::Generator { params, .. } if params.0.values().any(|v| matches!(v, anim_core::fx::FxValue::Text { text, .. } if text == lessons::THE_WORD)));
    assert!(reached.iter().any(|n| word(n)), "the word");
    assert_eq!(state.page_hand_built(), None, "no DANGER edge");
}

/// Spec AU25, 6: nothing of the nine needs ffmpeg, a sound file's dialog or the
/// Tab key, and `Add effect` is the door to Particles.
#[test]
fn auveteran_no_lesson_needs_ffmpeg() {
    const NEEDS: [Action; 3] = [Action::ExportMp4, Action::ImportSound, Action::AddSoundTrack];
    for lesson in lessons::ALL {
        for task in lesson.tasks {
            for action in NEEDS {
                assert!(!task.ring.contains(&Target::Action(action)), "{}: {}", lesson.id, task.say);
                assert!(!matches!(task.show, Show::Perform(a) if a == action), "{}: {}", lesson.id, task.say);
            }
            for word in ["Tab", "MP4", "ffmpeg"] {
                assert!(!task.say.contains(word), "{}: {}", lesson.id, task.say);
            }
        }
    }
    let lamp = veteran("desk_to_lamp");
    let dust = lamp.tasks.iter().find(|t| t.done == Done::Particles).expect("the dust");
    assert!(dust.say.contains("{Add effect}") && dust.ring == [Target::Named("shelf.add")], "{}", dust.say);
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "desk_to_lamp");
    assert!(anim_core::audio::tracks_iter(h.state().cut()).next().is_some(), "the hum is on the sheet");
    assert!(h.rig().requests.is_empty(), "no dialog asked for it");
}

/// Spec AU25, 7: no two of the nine read alike, and none is the demo's circle or
/// the sample's ball.
#[test]
fn auveteran_classes_differ() {
    let arts: Vec<Vec<Path>> = lessons::ALL.iter().map(|l| (l.art)()).collect();
    let small = |(lesson, art): (&Lesson, &Vec<Path>)| {
        let row = if lesson.thumb.is_empty() { lesson.finish } else { lesson.thumb };
        let paths: Vec<&Path> = row.iter().map(|i| &art[usize::from(*i)]).collect();
        (lesson.id, thumb(&paths, (lesson.paper.width, lesson.paper.height)))
    };
    let thumbs: Vec<(&str, Vec<bool>)> = lessons::ALL.iter().zip(&arts).map(small).collect();
    let mut pairs = 0;
    for (i, (a, x)) in thumbs.iter().enumerate() {
        for (b, y) in &thumbs[i + 1..] {
            pairs += 1;
            assert!(iou(x, y) <= IOU_MOST, "{a} and {b}: {}", iou(x, y));
        }
    }
    assert_eq!(pairs, 36);
    let round = |at: [f32; 2], wide: f32| {
        let ring = (0..=96).map(|i| i as f32 / 96.0 * std::f32::consts::TAU);
        let points = ring.map(|t| [at[0] + wide * t.cos(), at[1] + wide * t.sin(), 1.0]).collect();
        Path { points, width: 40.0, kind: PathKind::Finish, over: false }
    };
    let (circle, ball) = (round([50.0, 50.0], 30.0), round([50.0, 60.0], 18.0));
    for (id, x) in &thumbs {
        assert!(iou(x, &thumb(&[&circle], (100, 100))) <= IOU_MOST, "{id} and the demo circle");
        assert!(iou(x, &thumb(&[&ball], (100, 100))) <= IOU_MOST, "{id} and the sample's ball");
    }
}

/// Spec 5.2: an Experienced task rings nothing until `Show me where` is pressed,
/// and its card holds no picture to be compared with until the end.
#[test]
fn auveteran_no_ring_until_asked() {
    for lesson in veterans() {
        let last = lesson.tasks.iter().rposition(|t| !matches!(t.ring.last(), None | Some(Target::Paper))).expect("a ring");
        for i in [0, last] {
            let mut h = EditorHarness::as_shipped();
            sized(&mut h, 1280.0, 800.0);
            open(&mut h, lesson.id);
            stand_on_the_tasks_before(&mut h, &lesson, i);
            let e = h.editor_mut();
            e.enter_room(lesson.tasks[i].step);
            e.begin_lesson(lesson, i);
            settle(&mut h);
            assert_eq!(h.ring_stands(), None, "{} task {i}: no ring before it is asked", lesson.id);
            assert!(!h.has(crate::lesson::SHOW_ME), "{} task {i}: no `Show me`", lesson.id);
            h.click(SHOW_WHERE);
            settle(&mut h);
            assert!(h.ring_stands().is_some(), "{} task {i}: asked, it stands", lesson.id);
        }
    }
}

/// `id` at its finish: a single task, the step it opens on.
fn at_the_finish(id: &str) -> EditorHarness {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, id);
    let step = h.editor().stage.expect("the lesson's step");
    let task = lessons::Task { step, done: Done::OnStep(step), ..lessons::PROBED.tasks[0] };
    let tasks: &'static [lessons::Task] = Box::leak(Box::new([task]));
    let lesson = *lessons::table().iter().find(|l| l.id == id).expect("a row");
    h.editor_mut().lesson = None;
    h.editor_mut().begin_lesson(Lesson { tasks, ..lesson }, 0);
    settle(&mut h);
    h
}

/// What the Amateurs left: their `Next lesson` is their type's Experienced row,
/// no stand-in now, and the last class's is the Lessons page.
#[test]
fn auveteran_next_walks_to_the_last_class() {
    let row = |id: &str| lessons::table().iter().position(|l| l.id == id).expect("a row");
    for (amateur, next) in [("coat_of_shavings", "plate_one"), ("still_in_the_coat", "desk_to_lamp"), ("too_much_coat", "only_word")] {
        let mut h = at_the_finish(amateur);
        h.click(crate::lesson::NEXT);
        settle(&mut h);
        let asked = crate::AppRequest::Start(crate::welcome::WelcomeAction::Lesson(row(next)));
        assert!(h.rig().requests.contains(&asked), "{amateur}: {next} is next");
        let mut h = at_the_finish(next);
        h.click(crate::lesson::NEXT);
        settle(&mut h);
        assert!(h.editor().start.as_ref().is_some_and(|w| w.tab == crate::welcome::WelcomeTab::Lessons), "{next}: the page");
    }
}
