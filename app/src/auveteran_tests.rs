//! AU25: the three Experienced lessons: what their files hold, and that all nine
//! stand (`research/LESSONS-approachable-ui.md`). The one gate walks them with
//! every other lesson; this reads what only these three say.

use std::path::PathBuf;

use anim_core::graph::{Node, NodeKind};
use anim_core::generator::GenKind;
use anim_core::manga;
use anim_core::model::LayerRole;
use anim_core::xsheet::Exposure;
use eframe::egui::{self, Pos2};

use crate::auguide_show_tests::{File, Hand, Picture, Ridden, at, click_cell, iou, ride, ride_tasks, settle, thumb};
use crate::auguide_tests::stand_on_the_tasks_before;
use crate::aulesson_tests::open;
use crate::aurail_tests::sized;
use crate::canvas::extpaper_tests::paint_texels;
use crate::config::Action;
use crate::doc::path_cover;
use crate::lesson::{SHOW_WHERE, number};
use crate::lessons::{self, Class, Done, Lesson, Path, PathKind, Show, Target};
use crate::render_queue::ExportKind;
use crate::steps::ProjectKind;
use crate::testkit::EditorHarness;
use crate::workspace::Stage;
use crate::{plate, sample, templates};

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
    assert_eq!(counts, [13, 15, 12]);
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
            for sentence in std::iter::once(task.say).chain(task.why).chain(task.miss) {
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
    // W stays blank in flight: its plane does not fly, and the keys carry theirs.
    let hangs = lessons::LAMP_KEYS[2];
    assert!((0..hangs).all(|frame| wings.resolve(frame).is_none()), "W is blank until the moth lands");
    // The hang's 12 rows, beaten at Lip flap's own two-row hold: six keys is all
    // of them, and each of the three wings takes its turn on the way.
    let beats: Vec<(u32, Exposure)> = wings.keys().filter(|(_, shown)| *shown != Exposure::Empty).collect();
    assert!(beats.len() >= 6, "{beats:?}: the wings beat");
    assert!(beats.windows(2).all(|run| run[1].0 - run[0].0 <= 3), "{beats:?}: a beat of 2 to 3 rows");
    let mut drawn: Vec<Exposure> = beats.iter().map(|(_, shown)| *shown).collect();
    drawn.sort_by_key(|shown| format!("{shown:?}"));
    drawn.dedup();
    assert_eq!(drawn.len(), 3, "{beats:?}: closed, half and open all land");

    // The last frame's wings stand on the landed body, and none strays.
    let art = (veteran("desk_to_lamp").art)();
    let far = art[..3].iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| {
        let [l, t, r, u] = p.bounds();
        [b[0].min(l - p.width), b[1].min(t - p.width), b[2].max(r + p.width), b[3].max(u + p.width)]
    });
    let [x, y] = lessons::WINGS_AT;
    assert!((far[0]..far[2]).contains(&x) && (far[1]..far[3]).contains(&y), "{far:?}: the wings' reach holds the landed body");
    let Some(id) = wings.resolve(pictures.len() as u32 - 1) else { panic!("a wing on the last frame") };
    let drawing = cut.drawing(id).expect("the wing");
    let inked = drawing.layers.iter().map(|l| crate::lesson::ink_in(l, [0.0, 0.0, 1024.0, 768.0])).sum::<u32>();
    let near = drawing.layers.iter().map(|l| crate::lesson::ink_in(l, far)).sum::<u32>();
    assert!(inked > 0 && near == inked, "{near} of {inked} wing pixels within reach of the body");

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
    assert!(moved > 1200, "the dust moved {moved} pixels");
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
    let polys = manga::panels_of(cut).map_or_else(Vec::new, |(_, polys)| polys);
    assert!(polys.len() >= 5, "{} panels", polys.len());
    let tone = &cut.xsheet.columns[usize::from(lessons::TRACK_TONE)];
    assert!(manga::is_tone_column(cut, tone.id), "the TONE track");
    assert!(tone.keys().next().is_some(), "tone on the TONE track");
    let reached = crate::lesson::reached(&cut.graph);
    let balloon = reached.iter().find_map(|n| match n {
        NodeKind::Generator { generator: GenKind::Balloon, params, .. } => Some([number(params, "x")?, number(params, "y")?]),
        _ => None,
    });
    // Its middle is a fraction of the page, as a generator's place is.
    let [x, y] = balloon.expect("a balloon");
    let project = &state.engine.project;
    let centre = [x * project.width as f32, y * project.height as f32];
    assert_eq!(manga::panel_at(&polys, centre), Some(polys.len() - 1), "the balloon at {centre:?} stands in the last panel");
    let word = |n: &&NodeKind| matches!(n, NodeKind::Generator { params, .. } if params.0.values().any(|v| matches!(v, anim_core::fx::FxValue::Text { text, .. } if text == lessons::THE_WORD)));
    assert!(reached.iter().any(|n| word(n)), "the word");
    assert_eq!(state.page_hand_built(), None, "no DANGER edge");

    // The flood went on INK: the wall prints black, not as a screen of dots, and
    // the dive's panel, which it was never to reach, holds none of it.
    let ink = &cut.xsheet.columns[usize::from(lessons::TRACK_INK)];
    let color_of = |column: &anim_core::xsheet::Column| {
        let drawing = column.resolve(0).and_then(|id| cut.drawing(id));
        let colors = drawing.map_or(&[][..], |d| &d.layers[..]);
        colors.iter().filter(|l| l.props.role == LayerRole::Color).map(|l| crate::lesson::ink_in(l, [0.0, 0.0, 2591.0, 3624.0])).sum::<u32>()
    };
    assert!(color_of(ink) > 10_000, "the flood is on the INK track: {}, on TONE {}", color_of(ink), color_of(tone));
    assert_eq!(color_of(tone), 0, "and none of it on TONE");
    for [x, y] in lessons::WALL_CORNERS {
        let wall = pages[0].pixel(x as i32, y as i32);
        assert_eq!(wall[..3], [0, 0, 0], "the wall at {x}, {y}");
    }
    let drawing = ink.resolve(0).and_then(|id| cut.drawing(id)).expect("the page's ink");
    let spilt: u32 = drawing.layers.iter().filter(|l| l.props.role == LayerRole::Color).map(|l| crate::lesson::ink_in(l, [0.0, 0.0, 1200.0, 3624.0])).sum();
    assert_eq!(spilt, 0, "the dive's panel keeps its paper");

    // The lines run down, thinned, and cross the page: no panel holds them.
    let lines = reached.iter().find_map(|n| match n {
        NodeKind::Generator { generator: GenKind::SpeedLines, params, .. } => Some((number(params, "angle")?, number(params, "count")?)),
        _ => None,
    });
    let (angle, count) = lines.expect("the speed lines");
    assert_eq!((angle, count), (90.0, 30.0), "down the dive, and thin");
}

/// B4: the dive is free-hand: the snap the canyon stood on is off by the moth's
/// task, and the stroke keeps the bend the same stroke snapped would lose.
#[test]
fn auveteran_only_word_dive_keeps_its_curve() {
    let lesson = veteran("only_word");
    let task = lesson.tasks.iter().position(|t| matches!(t.done, Done::All(parts) if parts.contains(&Done::Snap(false)))).expect("snap off");
    let art = (lesson.art)();
    let moth: Vec<[f32; 3]> = art[usize::from(lesson.tasks[task].stand_in[0])].points.iter().step_by(4).take(16).copied().collect();
    let bend = |snap: bool| {
        let mut h = EditorHarness::as_shipped();
        sized(&mut h, 1280.0, 800.0);
        open(&mut h, lesson.id);
        ride_tasks(&mut h, &lesson, 0..task, Hand::ShowMe);
        h.rig_mut().config.pen.stabilizer = 0;
        h.state_mut().manga.snap = snap;
        h.run_frames(2);
        assert_eq!(h.editor().canvas.snap_rulers.is_some(), snap, "the snap the stroke meets");
        let xf = h.editor().canvas.view_xform();
        let path: Vec<Pos2> = moth.iter().map(|p| xf.paper_to_screen(egui::pos2(p[0], p[1]))).collect();
        h.drag_path(&path);
        let tap = h.editor().canvas.stroke_tap.clone();
        assert!(tap.len() >= 6, "{} samples tapped", tap.len());
        let (a, z) = (tap[0], tap[tap.len() - 1]);
        let (dx, dy) = (z.x - a.x, z.y - a.y);
        let long = (dx * dx + dy * dy).sqrt().max(1.0);
        tap.iter().map(|s| ((s.x - a.x) * dy - (s.y - a.y) * dx).abs() / long).fold(0.0f32, f32::max)
    };
    let (free, snapped) = (bend(false), bend(true));
    assert!(free > 30.0, "the moth's stroke bends {free} px");
    assert!(snapped < 1.0, "the snap would have held it to {snapped} px");
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
    let round = |at: [f32; 2], wide: f32, width: f32| {
        let ring = (0..=96).map(|i| i as f32 / 96.0 * std::f32::consts::TAU);
        let points = ring.map(|t| [at[0] + wide * t.cos(), at[1] + wide * t.sin(), 1.0]).collect();
        Path { points, width, kind: PathKind::Finish, over: false }
    };
    // The demo's ring as `demo_circle` lays it on the ready paper, and the ball as
    // the sample draws it, both at their own sizes.
    let (w, h) = (templates::READY.width as f32, templates::READY.height as f32);
    let circle = round([w * 0.3, h * 0.5], w.min(h) / 6.0, (w.min(h) / 120.0).max(2.0));
    let (at, size) = (sample::ball_centre(0), sample::SIZE);
    let ball = round([at.0, at.1], sample::RADIUS / 2.0, sample::RADIUS);
    let (ring, disc) = (thumb(&[&circle], (templates::READY.width, templates::READY.height)), thumb(&[&ball], size));
    for (id, x) in &thumbs {
        assert!(iou(x, &ring) <= IOU_MOST, "{id} and the demo circle: {}", iou(x, &ring));
        assert!(iou(x, &disc) <= IOU_MOST, "{id} and the sample's ball: {}", iou(x, &disc));
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

/// A ringed cell is whole and takes its click: at the foot of a slim sheet it
/// stood under the across bar, half cut, and the click scrolled the sheet away.
#[test]
fn auveteran_a_ringed_cell_takes_its_click() {
    let lesson = veteran("desk_to_lamp");
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, lesson.id);
    settle(&mut h);
    h.click(SHOW_WHERE);
    settle(&mut h);
    let Some((Target::Cell { track, frame }, rect)) = h.ring_stands() else { panic!("the ring on a cell") };
    assert!(rect.width() >= 80.0, "the whole cell shows: {rect:?}");
    h.click_at(rect);
    settle(&mut h);
    let state = h.state();
    assert_eq!(state.view.frame, frame, "the row took the click");
    assert_eq!(state.view.active_column, state.cut().xsheet.columns[usize::from(track)].id, "and the track");
}

/// The card's picture, read from what the frame filled: a paper the height of
/// `PICTURE_H`, which the canvas's own paper never is at this size.
fn pictures_on_card(h: &EditorHarness) -> usize {
    h.filled_rects(plate::PAPER).iter().filter(|r| (r.height() - crate::lesson::PICTURE_H).abs() < 1.0).count()
}

/// The lessons file: `None until the end`: a learner's plate is not compared with
/// ours while they make it. An Amateur's card keeps its picture, so the read is one.
#[test]
fn auveteran_no_picture_until_the_end() {
    for lesson in veterans() {
        let mut h = EditorHarness::as_shipped();
        sized(&mut h, 1280.0, 800.0);
        open(&mut h, lesson.id);
        settle(&mut h);
        assert_eq!(pictures_on_card(&h), 0, "{}: no picture to compare with", lesson.id);
        // The one task `at_the_finish` seats reads as done on the next frame.
        let h = at_the_finish(lesson.id);
        assert_eq!(pictures_on_card(&h), 1, "{}: the end has one", lesson.id);
    }
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, "coat_of_shavings");
    settle(&mut h);
    assert_eq!(pictures_on_card(&h), 1, "an Amateur's card shows its picture throughout");
}

/// B2: a click on another track is no lip flap, though that track holds more
/// keys than W did.
#[test]
fn auveteran_lipped_reads_its_own_track() {
    let lesson = veteran("desk_to_lamp");
    let i = lesson.tasks.iter().position(|t| t.done == Done::Lipped(lessons::TRACK_W)).expect("Lip flap's task");
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, lesson.id);
    stand_on_the_tasks_before(&mut h, &lesson, i);
    let e = h.editor_mut();
    e.enter_room(lesson.tasks[i].step);
    e.begin_lesson(lesson, i);
    settle(&mut h);
    let keys = |h: &EditorHarness, t: u8| h.state().cut().xsheet.columns[usize::from(t)].keys().count();
    assert!(keys(&h, lessons::TRACK_A) > keys(&h, lessons::TRACK_W), "the walk below proves nothing otherwise");
    click_cell(h.state_mut(), (lessons::TRACK_A, 0));
    settle(&mut h);
    assert_eq!(at(&h), i, "a change of track ticks nothing");
    click_cell(h.state_mut(), (lessons::TRACK_W, 46));
    h.state_mut().new_drawing_at_frame();
    settle(&mut h);
    assert_eq!(at(&h), i + 1, "a further key on W does");
}

/// B9: a file that landed with the latch off says what kept it off the task, and
/// the task's own words come back when the latch is on.
#[test]
fn auveteran_a_file_off_the_latch_says_why() {
    let cases: [(&str, &str, fn(&mut EditorHarness)); 2] = [
        ("plate_one", "That file kept its white paper.", |h| h.state_mut().render_queue.panel.transparent = true),
        ("only_word", "That page kept its greys.", |h| h.state_mut().manga.export.mode = anim_core::deliver::PageMode::Mono),
    ];
    for (id, says, latch) in cases {
        let lesson = veteran(id);
        let i = lesson.tasks.len() - 1;
        assert!(lesson.tasks[i].miss.is_some_and(|m| m.contains(says)), "{id}: the table's own line");
        let mut h = EditorHarness::as_shipped();
        sized(&mut h, 1280.0, 800.0);
        open(&mut h, id);
        let e = h.editor_mut();
        e.canvas.slot_at_pen_down(true, &mut e.state).expect("a cel to draw on");
        paint_texels(&mut e.state, &[(10, 10)], [0.0, 0.0, 0.0, 1.0]);
        e.enter_room(lesson.tasks[i].step);
        e.begin_lesson(lesson, i);
        settle(&mut h);
        let words = |h: &EditorHarness| h.painted_text().join(" ");
        assert!(!words(&h).contains(says), "{id}: nothing yet to say it of");
        let dir = tempfile::tempdir().expect("dir");
        let ctx = h.ctx();
        match id {
            "plate_one" => {
                h.editor_mut().invoke(Action::ExportStill, &ctx);
                settle(&mut h);
                h.answer_export(&dir.path().join("off.png"));
                h.run_export_to_end();
            }
            _ => {
                h.editor_mut().invoke(Action::ExportPages, &ctx);
                settle(&mut h);
                h.answer_pages(dir.path());
            }
        }
        settle(&mut h);
        assert_eq!(at(&h), i, "{id}: a file off the latch ticks nothing");
        assert!(words(&h).contains(says), "{id}: {}", words(&h));
        latch(&mut h);
        settle(&mut h);
        assert!(!words(&h).contains(says), "{id}: the latch is on");
        assert_eq!(at(&h), i, "{id}: and the file is still to make");
    }
}

/// Major: free work keeps its door: a room's stack arms its layer, a hidden one
/// too, and the pen is refused there. A lesson solves this in its own sentence.
#[test]
fn auveteran_free_work_keeps_the_door() {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1280.0, 800.0);
    open(&mut h, "plate_one");
    h.editor_mut().lesson = None;
    let e = h.editor_mut();
    e.enter_room(Stage::Drawing);
    e.canvas.slot_at_pen_down(true, &mut e.state).expect("a cel to draw on");
    paint_texels(&mut e.state, &[(10, 10)], [0.0, 0.0, 0.0, 1.0]);
    settle(&mut h);
    let armed = h.state().view.active_layer.clone();
    let id = h.state().active_layer_id().expect("the armed layer");
    let props = h.state().strip_layers().iter().find(|(_, l)| l.id == id).map(|(_, l)| l.props.clone()).expect("its row");
    h.state_mut().layer_set_props(id, anim_core::model::LayerProps { visible: false, ..props });
    h.editor_mut().enter_room(Stage::InkPaint);
    settle(&mut h);
    assert_ne!(h.state().view.active_layer, armed, "Color arms its own");
    h.editor_mut().enter_room(Stage::Drawing);
    settle(&mut h);
    assert_eq!(h.state().view.active_layer, armed, "the door arms its layer, hidden or not");
    let e = h.editor_mut();
    assert!(e.canvas.slot_at_pen_down(true, &mut e.state).is_none(), "and a hidden layer refuses the pen");
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
