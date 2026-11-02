//! AU28: the guide, Show me, the art's checks and the one gate (spec 5.7, 5.9,
//! 5.13 to 5.15). The gate reads only a lesson's own tables, so a lesson that
//! lands is walked with no new test code.

use std::collections::BTreeMap;
use std::path::{Path as FilePath, PathBuf};
use std::sync::Arc;

use anim_core::camera::IRect;
use anim_core::fill::{FillOpts, flood_fill_mask_in};
use anim_core::manga::{Page, slice_webtoon};
use anim_core::raster::{TILE, TILE_LEN, TileData, f32_to_f16_bits};
use anim_core::store;
use eframe::egui::epaint::CircleShape;
use eframe::egui::{self, Pos2, Shape, pos2};
use egui_kittest::kittest::Queryable;

use crate::aulesson_tests::{along, export_asked, open, pen};
use crate::aurail_tests::sized;
use crate::auguide_tests::{stand, stand_on_the_tasks_before};
use crate::canvas::extpaper_tests::{paint_texels, texel};
use crate::canvas::{CanvasTool, CanvasView, GuideLine, NEEDS_ENGINE};
use crate::config::Action;
use crate::doc::{self, AppState, PathInk};
use crate::lesson::{self, BY_HAND, HOLD, LEAVE, PRESS_SECS, REVEAL_SECS, SHOW_ME, SHOW_WHERE};
use crate::lessons::art::{self, Pt};
use crate::lessons::{self, Class, Done, Lesson, Path, PathKind, Show, Target, Task};
use crate::measure::{Act, Recorder};
use crate::rail::Key;
use crate::render_queue::ExportKind;
use crate::steps::ProjectKind;
use crate::testkit::EditorHarness;
use crate::workspace::Stage;
use crate::{AppRequest, Pane, Performed, ausay_tests, plate, steps, visible_panes};

/// Past this a line is a line and not a press (spec 5.14).
const LONG: f32 = 60.0;
/// A pen drag's frames, along any path: a long line is not a long test.
const PEN_FRAMES: usize = 24;
/// The Rig's clock, a tenth of a second a step, over a lesson's slowest pass.
const PLAY_STEPS: usize = 600;
/// The default gap the closed-shape checks read at.
const GAP: u32 = 2;

pub(crate) fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

pub(crate) fn at(h: &EditorHarness) -> usize {
    h.editor().lesson.as_ref().expect("a lesson stands").task
}

/// The last row `for_gate` gives is the probe, until the last lesson part.
fn probe() -> Lesson {
    *lessons::for_gate().last().expect("the probe")
}

fn row(id: &str) -> Lesson {
    *lessons::table().iter().find(|l| l.id == id).expect("a row of the table")
}

/// A task on the paper with nothing but its `Done` and its `Show`.
const fn on_paper(done: Done, show: Show) -> Task {
    Task { say: "Draw.", why: None, step: Stage::Drawing, done, ring: &[Target::Paper], guide: &[], stand_in: &[], miss: None, show }
}

const NEVER: Task = on_paper(Done::Strokes(u32::MAX), Show::Where);

/// `tasks` as a run of the probe, over the probe's own paper, from the door.
fn run_of(tasks: &'static [Task]) -> EditorHarness {
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    h.editor_mut().begin_lesson(Lesson { tasks, ..probe() }, 0);
    settle(&mut h);
    h
}

/// The probe's paper with two lines to trace, and with a fill to do: the shots'.
pub(crate) fn traced_lesson() -> Lesson {
    Lesson { tasks: TWO_LINES, ..probe() }
}

pub(crate) fn fill_lesson() -> Lesson {
    Lesson { tasks: FILL, ..probe() }
}

/// `points` on the screen as the view stands.
fn on_screen(h: &EditorHarness, points: &[[f32; 3]]) -> Vec<Pos2> {
    let xf = h.editor().canvas.view_xform();
    let skip = (points.len() / PEN_FRAMES).max(1);
    let last = points.last().copied();
    let walked = points.iter().step_by(skip).copied().chain(last);
    walked.map(|p| xf.paper_to_screen(pos2(p[0], p[1]))).collect()
}

/// The paper's texels under `path`.
fn under(path: &Path) -> Vec<(i32, i32)> {
    let mut cells = std::collections::BTreeSet::new();
    doc::path_cover(&path.points, path.width, |x, y, _| {
        cells.insert((x, y));
    });
    cells.into_iter().collect()
}

// ---- The one gate (spec 5.15) ----

/// Who answers a lesson's tasks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Hand {
    ShowMe,
    Pen,
    /// AU23: the card's button and nothing else: one press a task, and the
    /// ring never climbed.
    Alone,
}

/// What a lesson ended in.
pub(crate) enum File {
    One(ExportKind, PathBuf),
    Pages(PathBuf),
}

/// A ride's files, and the folder they stand in, kept for as long as they are read.
pub(crate) struct Ridden {
    _dir: tempfile::TempDir,
    pub(crate) files: Vec<File>,
}

/// `path`'s stroke by the pen route: a Rig stroke takes the vector route, so
/// the pixels are `paint_texels`'s, and they are no second stroke. An eraser's
/// stroke is `commit_path`'s `erase` (spec 5.15).
fn pen_line(go: &mut Recorder, step: &str, art: &[Path], index: u16) {
    let path = &art[usize::from(index)];
    if path.kind == PathKind::Wipe {
        go.act(step, Act::Answer(Box::new(move |h| assert!(h.editor_mut().lesson_land(&[index]), "the wipe landed"))));
        return;
    }
    let drawn = on_screen(go.harness(), &path.points);
    go.act(step, Act::Stroke(&drawn));
    let state = go.harness().state_mut();
    let strokes = state.strokes_done;
    paint_texels(state, &under(path), [0.0, 0.0, 0.0, 1.0]);
    state.strokes_done = strokes;
}

/// A task's own lines, which a person would draw.
fn own_lines(task: &Task) -> &'static [u16] {
    match task.show {
        Show::Trace(paths) => paths,
        _ => task.stand_in,
    }
}

/// `lines` as a person's own art: strokes or fill seeds.
fn stand_in(go: &mut Recorder, step: &str, art: &[Path], lines: &[u16], hand: Hand) {
    for &index in lines {
        let path = &art[usize::from(index)];
        match (path.kind, hand) {
            (PathKind::Seed, _) => {
                let seed = pos2(path.points[0][0], path.points[0][1]);
                let fill = move |h: &mut EditorHarness| {
                    let e = h.editor_mut();
                    if e.canvas.slot_at_pen_down(true, &mut e.state).is_some() {
                        e.canvas.fill_at(seed, &mut e.state);
                    }
                };
                go.act(step, Act::Answer(Box::new(fill)));
            }
            (_, Hand::Pen) => pen_line(go, step, art, index),
            (_, Hand::ShowMe | Hand::Alone) => go.act(step, Act::Answer(Box::new(move |h| assert!(h.editor_mut().lesson_land(&[index]), "the line landed")))),
        }
    }
}

/// A click where the ring stands, and nowhere else, until the chain's last
/// target has its rect (spec 5.8). The driver never names a word.
fn climb(go: &mut Recorder, step: &str, task: &Task) {
    let last = task.ring.last().copied();
    for _ in 0..=task.ring.len() + 2 {
        let stands = go.harness().ring_stands();
        let (target, rect) = stands.unwrap_or_else(|| panic!("{step}: the ring stands nowhere"));
        if Some(target) == last {
            return;
        }
        go.act(step, Act::Answer(Box::new(move |h| {
            h.click_at(rect);
            settle(h);
        })));
    }
    panic!("{step}: {last:?} never got a rect");
}

/// The task rings a control: the paper is pointed at by the guide or the
/// sentence, and has no ring.
fn rings(task: &Task) -> bool {
    !matches!(task.ring.last(), None | Some(Target::Paper))
}

/// The ring stands on this task. An Experienced lesson's stands only when asked
/// (spec 5.2), and a task of theirs on the paper has no button to ask with.
fn ringed(lesson: &Lesson, task: &Task) -> bool {
    lesson.class != Class::Experienced || rings(task)
}

/// A tap of the armed Fill at `seed`. The Rig's pen route refuses one for want
/// of an engine (spec 5.9), and `fill_at` is that tap past the gate.
fn tap(h: &mut EditorHarness, seed: [f32; 2]) {
    assert_eq!(h.editor().canvas.tool, CanvasTool::Fill, "Fill is armed");
    let e = h.editor_mut();
    if e.canvas.slot_at_pen_down(true, &mut e.state).is_some() {
        e.canvas.fill_at(pos2(seed[0], seed[1]), &mut e.state);
    }
}

/// What a person's hand does on the control the ring came to, as its task asks:
/// never the card's button. A task this does not answer stops the ride by name.
fn by_hand(go: &mut Recorder, step: &str, task: &Task, rect: egui::Rect) {
    /// `Longer`, from `Medium`'s 2 to the 5 the lesson holds.
    const LONGER: usize = 3;
    /// `Size` up, in screen points, and `Strength` down, of its track.
    const SIZE_DRAG: f32 = 40.0;
    const STRENGTH_LEFT: f32 = 0.025;
    match task.done {
        Done::BrushChanged => {
            let from = rect.center();
            go.act(step, Act::Drag(&[from, from - egui::vec2(0.0, SIZE_DRAG)]));
        }
        Done::Strength => {
            let from = rect.center();
            go.act(step, Act::Drag(&[from, pos2(rect.left() + rect.width() * STRENGTH_LEFT, from.y)]));
        }
        Done::Hold { .. } => {
            for _ in 0..LONGER {
                go.act(step, Act::Answer(Box::new(move |h| {
                    h.click_at(rect);
                    settle(h);
                })));
            }
        }
        Done::Filled { seed, .. } => go.act(step, Act::Answer(Box::new(move |h| {
            h.click_at(rect);
            settle(h);
            tap(h, seed);
        }))),
        // Each picture by the bottom row, whose ring moves on to the next white one.
        Done::FilledOn(n) => {
            let filled = |h: &EditorHarness| h.state().cut().drawings.iter().filter(|d| lesson::color_layer_art(d)).count();
            let from = filled(go.harness());
            for pose in from..n as usize {
                go.act(step, Act::Answer(Box::new(move |h| {
                    let (_, rect) = h.ring_stands().expect("the next picture");
                    h.click_at(rect);
                    settle(h);
                    tap(h, lessons::pose_seed(pose));
                    settle(h);
                })));
            }
        }
        _ => go.act(step, Act::Answer(Box::new(move |h| h.click_at(rect)))),
    }
}

/// The act that answers `task`: the card's button, the pen, or the control the
/// ring came to.
fn answer(go: &mut Recorder, step: &str, lesson: &Lesson, art: &[Path], task: &Task, hand: Hand) {
    if ringed(lesson, task) && hand != Hand::Alone {
        climb(go, step, task);
    }
    let shows = lesson.class != Class::Experienced && !matches!(task.show, Show::Where);
    let control = rings(task);
    let traced = matches!(task.show, Show::Trace(_));
    if shows && (hand != Hand::Pen || !(traced || control)) {
        // AU23: the climb left a menu open, and a person shuts it first.
        if egui::Popup::is_any_open(&go.harness().ctx()) {
            go.act(step, Act::Key(egui::Key::Escape));
            settle(go.harness());
        }
        go.act(step, Act::Click(SHOW_ME));
        if let Show::Trace(paths) = task.show {
            let secs = REVEAL_SECS * paths.len() as f64 + 0.2;
            go.act(step, Act::Answer(Box::new(move |h| h.wait(secs))));
        }
    } else if !own_lines(task).is_empty() {
        stand_in(go, step, art, own_lines(task), hand);
    } else if control {
        let (_, rect) = go.harness().ring_stands().expect("climbed");
        by_hand(go, step, task, rect);
    } else if let Show::Perform(action) = task.show {
        go.act(step, Act::Perform(action));
    } else if let Show::Run(run) = task.show {
        go.act(step, Act::Answer(Box::new(move |h| run(h.editor_mut()))));
    } else {
        panic!("{step}: no route answers this task");
    }
}

/// The dialogs the act asked for, answered from the log: the Rig never opens
/// one (spec 5.15).
fn dialogs(go: &mut Recorder, step: &str, id: &str, dir: &FilePath, files: &mut Vec<File>) {
    let asked = go.harness().rig().requests.iter().rev().find_map(|r| match r {
        AppRequest::ExportTo(kind) => Some(Some(*kind)),
        AppRequest::ExportPages => Some(None),
        _ => None,
    });
    let Some(asked) = asked else { return };
    let file = match asked {
        Some(kind @ ExportKind::Png) => {
            let frames = dir.join(format!("{id}-frames"));
            std::fs::create_dir_all(&frames).expect("the folder a person chose");
            File::One(kind, frames)
        }
        Some(kind @ ExportKind::Mp4) => File::One(kind, dir.join(format!("{id}.mp4"))),
        Some(kind @ ExportKind::Gif) => File::One(kind, dir.join(format!("{id}.gif"))),
        Some(kind @ ExportKind::Still) => File::One(kind, dir.join(format!("{id}.png"))),
        None => File::Pages(dir.join(format!("{id}-pages"))),
    };
    match &file {
        File::One(_, path) => {
            let path = path.clone();
            go.act(step, Act::Answer(Box::new(move |h| {
                h.answer_export(&path);
                h.run_export_to_end();
            })));
        }
        File::Pages(path) => {
            std::fs::create_dir_all(path).expect("the folder a person chose");
            let path = path.clone();
            go.act(step, Act::Answer(Box::new(move |h| h.answer_pages(&path))));
        }
    }
    files.push(file);
}

/// A file's first picture: width, height, RGBA.
pub(crate) struct Picture {
    pub(crate) size: (u32, u32),
    pub(crate) rgba: Vec<u8>,
}

impl Picture {
    pub(crate) fn png(file: &FilePath) -> Picture {
        let bytes = std::fs::File::open(file).unwrap_or_else(|_| panic!("{file:?} is there"));
        let mut read = png::Decoder::new(std::io::BufReader::new(bytes)).read_info().expect("a PNG");
        let mut data = vec![0; read.output_buffer_size()];
        let info = read.next_frame(&mut data).expect("its picture");
        assert_eq!(info.bit_depth, png::BitDepth::Eight, "{file:?}");
        let samples = info.color_type.samples();
        let pixel = |px: &[u8]| match samples {
            1 | 2 => [px[0], px[0], px[0], if samples == 2 { px[1] } else { 255 }],
            3 => [px[0], px[1], px[2], 255],
            _ => [px[0], px[1], px[2], px[3]],
        };
        let rgba = data[..info.buffer_size()].chunks_exact(samples).flat_map(pixel).collect();
        Picture { size: (info.width, info.height), rgba }
    }

    /// Each picture of the file, and its length in hundredths of a second. A
    /// held drawing is one GIF frame, standing for its whole run.
    pub(crate) fn gif(file: &FilePath) -> (Vec<Picture>, u32) {
        let bytes = std::fs::read(file).expect("the GIF");
        let gif = crate::gif_decode::decode(&bytes).expect("a GIF");
        let size = (u32::from(gif.size.0), u32::from(gif.size.1));
        let picture = |frame: &crate::gif_decode::Frame| Picture { size, rgba: frame.rgb.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect() };
        (gif.frames.iter().map(picture).collect(), gif.duration_cs())
    }

    /// Each PNG in `dir`, by name.
    pub(crate) fn folder(dir: &FilePath) -> Vec<Picture> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir).expect("the folder").flatten().map(|f| f.path()).collect();
        files.sort();
        files.iter().map(|f| Picture::png(f)).collect()
    }

    pub(crate) fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        let (w, h) = (self.size.0 as i32, self.size.1 as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            return [255, 255, 255, 0];
        }
        let i = (y * w + x) as usize * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// Something other than the sheet, this near `(x, y)`.
    fn inked_near(&self, x: f32, y: f32, reach: i32) -> bool {
        let spots = (-reach..=reach).flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)));
        spots.into_iter().any(|(dx, dy)| is_ink(self.pixel(x as i32 + dx, y as i32 + dy)))
    }

    fn blank(&self) -> bool {
        !self.rgba.chunks_exact(4).any(|px| is_ink([px[0], px[1], px[2], px[3]]))
    }

    /// The pixels of this colour, as near as eight bits keep it.
    pub(crate) fn count(&self, rgb: [u8; 3]) -> usize {
        let near = |px: &&[u8]| px.iter().zip(rgb).all(|(a, b)| a.abs_diff(b) <= 3);
        self.rgba.chunks_exact(4).filter(near).count()
    }
}

fn is_ink(px: [u8; 4]) -> bool {
    px[3] > 128 && px[..3].iter().any(|c| *c < 200)
}

/// A lesson's ending, read: `count` pictures, each the paper's size and none of
/// them blank, and nine samples in ten of each finish path on the ink of one of
/// them: a path may be any page's or frame's. `Err` says what it is not.
fn reads_the_finish(lesson: &Lesson, art: &[Path], pictures: &[Picture], count: usize) -> Result<(), String> {
    if pictures.len() != count {
        return Err(format!("{} pictures, for {count}", pictures.len()));
    }
    let paper = (lesson.paper.width, lesson.paper.height);
    for (i, picture) in pictures.iter().enumerate() {
        if picture.size != paper {
            return Err(format!("picture {i}: {:?}, on a paper of {paper:?}", picture.size));
        }
        if picture.blank() {
            return Err(format!("picture {i}: blank"));
        }
    }
    for index in lesson.finish {
        let path = &art[usize::from(*index)];
        let on = |picture: &Picture| path.points.iter().filter(|p| picture.inked_near(p[0], p[1], 3)).count();
        let met = pictures.iter().map(on).max().unwrap_or(0);
        if met * 10 < path.points.len() * 9 {
            let first = pictures.first().map(|p| p.pixel(path.points[0][0] as i32, path.points[0][1] as i32));
            return Err(format!("path {index}: {met} of {} on ink, {first:?} where it starts", path.points.len()));
        }
    }
    Ok(())
}

/// The page files a book writes: a cut each, a webtoon's in its segments.
fn pages_owed(h: &EditorHarness) -> usize {
    let state = h.state();
    let scene = state.engine.project.scene(state.at().scene).expect("the book");
    let slices = |c: &anim_core::model::Cut| Page::of(c).and_then(|p| Some(slice_webtoon(p.height, p.segment?).len()));
    scene.cuts.iter().map(|c| slices(c).unwrap_or(1)).sum()
}

/// A landed file, decoded: size, frame count, and ink where the paths are.
fn read_file(h: &EditorHarness, lesson: &Lesson, art: &[Path], file: &File) -> Result<(), String> {
    let frames = h.state().shown_length();
    match file {
        File::One(ExportKind::Still, path) => reads_the_finish(lesson, art, &[Picture::png(path)], 1),
        File::One(ExportKind::Gif, path) => {
            let (pictures, length) = Picture::gif(path);
            let fps = h.state().engine.project.fps;
            if length != frames * 100 / fps {
                return Err(format!("the GIF runs {length} cs, for {frames} frames at {fps} fps"));
            }
            reads_the_finish(lesson, art, &pictures, pictures.len())
        }
        File::One(ExportKind::Png, dir) => reads_the_finish(lesson, art, &Picture::folder(dir), frames as usize),
        File::Pages(dir) => reads_the_finish(lesson, art, &Picture::folder(dir), pages_owed(h)),
        // A film needs ffmpeg, and a lesson must not (the lessons file).
        File::One(ExportKind::Mp4, path) => Err(format!("{path:?}: no lesson ends in a film")),
    }
}

/// Each file a lesson landed, decoded and read.
fn read_files(h: &EditorHarness, lesson: &Lesson, art: &[Path], files: &[File], step: &str) {
    assert!(!files.is_empty(), "{step}: no file landed");
    for file in files {
        if let Err(why) = read_file(h, lesson, art, file) {
            panic!("{step}: {why}");
        }
    }
}

/// The saved `.animproj`, reopened, is the document that made the file.
fn reopens_equal(h: &EditorHarness, dir: &FilePath, step: &str) {
    let file = dir.join("lesson.animproj");
    let project = &h.state().engine.project;
    store::save(project, &file).expect("saved");
    let back = store::load(&file).expect("reopened");
    assert!(back == *project, "{step}: the reopened project differs");
}

/// `lesson` from where it stands to its file. One loop, on the lesson's own
/// tables: any refusal or ignored verb fails by name.
pub(crate) fn ride(h: &mut EditorHarness, lesson: &Lesson, hand: Hand) -> Ridden {
    ride_from(h, lesson, 0, hand)
}

/// The same, from task `from`.
pub(crate) fn ride_from(h: &mut EditorHarness, lesson: &Lesson, from: usize, hand: Hand) -> Ridden {
    let dir = tempfile::tempdir().expect("dir");
    let art = (lesson.art)();
    let mut files = Vec::new();
    let mut go = Recorder::new(h, lesson.id);
    for (i, task) in lesson.tasks.iter().enumerate().skip(from) {
        let step = format!("{} task {i} by {hand:?}", lesson.id);
        assert_eq!(at(go.harness()), i, "{step}: the run stands here");
        if lesson.class == Class::Experienced && rings(task) {
            go.act(&step, Act::Click(SHOW_WHERE));
        }
        if ringed(lesson, task) {
            assert!(go.harness().ring_stands().is_some(), "{step}: the ring has a rect");
        }
        answer(&mut go, &step, lesson, &art, task, hand);
        dialogs(&mut go, &step, lesson.id, dir.path(), &mut files);
        for _ in 0..PLAY_STEPS {
            if task.done != Done::Played || at(go.harness()) != i {
                break;
            }
            go.harness().wait(0.1);
        }
        settle(go.harness());
        assert_eq!(at(go.harness()), i + 1, "{step}: this act ticks this task and no other");
    }
    let name = format!("{} by {hand:?}", lesson.id);
    read_files(go.harness(), lesson, &art, &files, &name);
    reopens_equal(go.harness(), dir.path(), &name);
    Ridden { _dir: dir, files }
}

/// Spec 5.15: every lesson, every task, through the real editor Rig on a
/// first-run boot, to a real finished file, by Show me and by the pen. AU23: at
/// the two sizes a laptop opens at, where the rail and the band fold.
#[test]
fn auguide_every_lesson_ends_in_its_file() {
    for lesson in lessons::for_gate() {
        for hand in [Hand::ShowMe, Hand::Pen] {
            for size in [(1280.0, 800.0), (1024.0, 640.0)] {
                let mut h = EditorHarness::as_first_run();
                sized(&mut h, size.0, size.1);
                open(&mut h, lesson.id);
                ride(&mut h, lesson, hand);
            }
        }
    }
}

/// Spec 5.15: the loop takes every class: an Experienced lesson asks `Show me
/// where` before its ring stands, and an Amateur's button still shows.
#[test]
fn auguide_gate_rides_every_class() {
    for id in ["probe-square", "probe-last"] {
        for hand in [Hand::ShowMe, Hand::Pen] {
            let lesson = row(id);
            let mut h = EditorHarness::as_first_run();
            open(&mut h, id);
            ride(&mut h, &lesson, hand);
        }
    }
}

/// Spec 5.15: the gate's other endings, so a lesson that lands on them finds
/// them proved: a GIF of the film, its frames, the book's pages to a folder.
#[test]
fn auguide_gate_reads_a_gif_and_pages() {
    const TRACE: Task = Task { guide: &[0], ..on_paper(Done::Strokes(1), Show::Trace(&[0])) };
    const GIF: &[Task] = &[TRACE, on_paper(Done::Exported(1), Show::Perform(Action::ExportGif))];
    const FRAMES: &[Task] = &[TRACE, on_paper(Done::Exported(1), Show::Perform(Action::ExportPngSequence))];
    const PAGES: &[Task] = &[TRACE, Task { step: Stage::Drawing, ..on_paper(Done::Exported(1), Show::Perform(Action::ExportPages)) }];
    for (id, tasks) in [("probe-film", GIF), ("probe-film", FRAMES), ("probe-page", PAGES)] {
        let mut h = EditorHarness::as_shipped();
        open(&mut h, id);
        let lesson = Lesson { tasks, art: panel_art, ..row(id) };
        let step = h.editor().stage.expect("a step");
        let tasks: Vec<Task> = tasks.iter().map(|t| Task { step, ..*t }).collect();
        let lesson = Lesson { tasks: Box::leak(tasks.into_boxed_slice()), ..lesson };
        h.editor_mut().begin_lesson(lesson, 0);
        settle(&mut h);
        ride(&mut h, &lesson, Hand::ShowMe);
    }
}

/// One line inside the probe book's first panel, and so on any of their papers:
/// a page shows no ink past its panels, and the probe's own art runs in the
/// page's top margin.
fn panel_art() -> Vec<Path> {
    vec![art::hand(&[[400.0, 600.0], [1100.0, 600.0]], 8.0, PathKind::Guide, 3)]
}

/// A white picture of `paper`, with `paths` on it in ink.
fn drawn(paper: (u32, u32), paths: &[&Path]) -> Picture {
    let (w, h) = (paper.0 as i32, paper.1 as i32);
    let mut rgba = vec![255; (paper.0 * paper.1 * 4) as usize];
    for path in paths {
        doc::path_cover(&path.points, path.width.max(4.0), |x, y, _| {
            if (0..w).contains(&x) && (0..h).contains(&y) {
                rgba[(y * w + x) as usize * 4..][..3].fill(0);
            }
        });
    }
    Picture { size: paper, rgba }
}

/// Spec 5.15: an ending that is not the finish fails the reading: a page with
/// no trace on it, from the real route, and a frame count, a size or a mark
/// that are not the lesson's.
#[test]
fn auguide_gate_fails_a_blank_ending() {
    let lesson = Lesson { art: panel_art, ..row("probe-page") };
    let art = (lesson.art)();
    let dir = tempfile::tempdir().expect("dir");
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe-page");
    h.state_mut().begin_page_export(dir.path(), None);
    h.state_mut().wait_page_export();
    // The page's own panel lines are ink, so it is the path that reads this.
    let page = File::Pages(dir.path().to_path_buf());
    let why = read_file(&h, &lesson, &art, &page).expect_err("a blank page is no finish");
    assert!(why.contains("path 0"), "{why}");
    let paper = (lesson.paper.width, lesson.paper.height);
    let aside = art::hand(&[[400.0, 1900.0], [1100.0, 1900.0]], 8.0, PathKind::Guide, 3);
    let finish = || vec![drawn(paper, &[&art[0]]), drawn(paper, &[&aside])];
    assert_eq!(reads_the_finish(&lesson, &art, &finish(), 2), Ok(()), "the reading passes the real thing");
    assert!(reads_the_finish(&lesson, &art, &finish(), 3).is_err(), "a page short");
    assert!(reads_the_finish(&lesson, &art, &finish()[1..], 1).is_err(), "ink, but not on the path");
    let empty = reads_the_finish(&lesson, &art, &[drawn(paper, &[])], 1);
    assert!(empty.is_err_and(|why| why.contains("blank")), "a white picture");
    let small = [drawn((paper.0 / 2, paper.1), &[&art[0]])];
    assert!(reads_the_finish(&lesson, &art, &small, 1).is_err(), "half the paper");
    let film = File::One(ExportKind::Mp4, dir.path().join("film.mp4"));
    assert!(read_file(&h, &lesson, &art, &film).is_err(), "no lesson ends in a film");
}

/// Spec 5.15: every stroke task of every lesson ticks from a pen stroke through
/// `paper_to_screen`, at a real window size.
#[test]
fn auguide_pen_ticks_every_stroke_task() {
    let mut tasks = 0;
    for lesson in lessons::for_gate() {
        let art = (lesson.art)();
        for (i, task) in lesson.tasks.iter().enumerate() {
            let lines = own_lines(task);
            if lines.is_empty() || lines.iter().any(|l| art[usize::from(*l)].kind == PathKind::Seed) {
                continue;
            }
            for size in [(1600.0, 900.0), (1280.0, 800.0), (1024.0, 640.0)] {
                let mut h = EditorHarness::as_shipped();
                sized(&mut h, size.0, size.1);
                open(&mut h, lesson.id);
                stand_on_the_tasks_before(&mut h, lesson, i);
                // A room's door arms its tool: the task before chose another.
                if h.editor().stage != Some(task.step) {
                    h.editor_mut().enter_room(task.step);
                }
                h.editor_mut().begin_lesson(*lesson, i);
                settle(&mut h);
                let step = format!("{} task {i} at {size:?}", lesson.id);
                let mut go = Recorder::new(&mut h, lesson.id);
                lines.iter().for_each(|line| pen_line(&mut go, &step, &art, *line));
                settle(&mut h);
                assert_eq!(at(&h), i + 1, "{step}");
                tasks += 1;
            }
        }
    }
    assert!(tasks >= 3, "the probe alone has one, at three sizes: {tasks}");
}

// ---- The guide (spec 5.7) ----

/// Every shape of the last frame, flat, in paint order.
fn painted(h: &EditorHarness) -> Vec<Shape> {
    fn walk(s: &Shape, out: &mut Vec<Shape>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            other => out.push(other.clone()),
        }
    }
    let mut out = Vec::new();
    h.kit().output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// The guide's lines as last painted, in paint order: a ribbon lays a dab on
/// each sample and a quad between, so a dab with no quad before it starts a
/// line.
fn guide_on_screen(h: &EditorHarness) -> Vec<Vec<CircleShape>> {
    let mut out: Vec<Vec<CircleShape>> = Vec::new();
    let mut joined = false;
    for shape in painted(h) {
        match shape {
            Shape::Circle(dab) if dab.fill == plate::GUIDE => {
                if !joined {
                    out.push(Vec::new());
                }
                out.last_mut().expect("a line").push(dab);
                joined = false;
            }
            Shape::Path(quad) if quad.fill == plate::GUIDE => joined = true,
            _ => {}
        }
    }
    out
}

/// Spec 5.7: exported with the guide standing on the paper: the file holds not
/// one pixel of its colour, and the save no key it did not hold.
#[test]
fn auguide_guide_never_exports() {
    let dir = tempfile::tempdir().expect("dir");
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    assert!(!guide_on_screen(&h).is_empty(), "the guide stands on the paper");
    let blue = [plate::GUIDE.r(), plate::GUIDE.g(), plate::GUIDE.b()];
    let export = |h: &mut EditorHarness, name: &str| {
        export_asked(h);
        let file = dir.path().join(name);
        h.answer_export(&file);
        h.run_export_to_end();
        settle(h);
        Picture::png(&file)
    };
    assert_eq!(export(&mut h, "clean.png").count(blue), 0, "no guide in the file");
    // The check can fail: the colour itself, on the paper, is counted.
    let line = [[300.0, 300.0, 1.0], [340.0, 300.0, 1.0]];
    let e = h.editor_mut();
    let slot = e.canvas.slot_at_pen_down(true, &mut e.state).expect("a cel to land on");
    e.canvas.brush_color = [blue[0], blue[1], blue[2], 255];
    let rgba = e.canvas.ink();
    assert!(e.state.commit_path(&[PathInk { points: &line, width: 12.0, rgba, erase: false, inside: false }], slot));
    assert!(export(&mut h, "marked.png").count(blue) > 0, "a pixel of that colour would be read");
    // Saved, the guide is no key.
    let file = dir.path().join("saved.animproj");
    store::save(&h.state().engine.project, &file).expect("saved");
    let back = store::load(&file).expect("reopened");
    let keys: Vec<&str> = back.app_meta.keys().map(String::as_str).collect();
    assert!(keys.iter().all(|k| [steps::KIND_KEY, lesson::KEY].contains(k)), "{keys:?}");
}

/// Spec 5.7: the render and save modules do not hold the guide, and the canvas
/// reads it from nowhere but the lesson.
#[test]
fn auguide_guide_stays_out_of_render_and_save() {
    const OUT: [&str; 7] = ["render_queue.rs", "export.rs", "export_step.rs", "worksafe.rs", "config.rs", "workspace.rs", "doc.rs"];
    const HOLDS: [&str; 3] = ["canvas.rs", "lesson.rs", "plate.rs"];
    const TOKENS: [&str; 6] = ["canvas.guide", "canvas::Guide", "GuideLine", "plate::GUIDE", "self.guide", "PathKind::Guide"];
    let sources = ausay_tests::sources();
    for name in OUT {
        assert!(sources.iter().any(|(file, _)| file == name), "{name} is a source: the rule reads it");
    }
    assert!(sources.iter().filter(|(file, _)| file == "canvas.rs").any(|(_, s)| s.contains("self.guide")), "the rule can match");
    for (file, source) in &sources {
        for token in TOKENS {
            let lessons_own = file == "lessons.rs" && token == "PathKind::Guide";
            let held = HOLDS.contains(&file.as_str()) || lessons_own;
            assert!(held || !source.contains(token), "{file} reads {token}");
        }
    }
    let core = FilePath::new(env!("CARGO_MANIFEST_DIR")).join("../crates/anim-core/src/store.rs");
    let store = std::fs::read_to_string(core).expect("the store");
    assert!(!store.contains("guide_line") && !store.contains("GuideLine"));
}

/// Spec 5.7: the lines ride the view: after a zoom, a turn and a mirror each
/// lands where `paper_to_screen` says.
#[test]
fn auguide_guide_follows_the_view() {
    let art = (probe().art)();
    let line: Vec<Pos2> = art[0].points.iter().map(|p| pos2(p[0], p[1])).collect();
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    let mut was = Vec::new();
    let mut read = |h: &EditorHarness, how: &str| {
        let lines = guide_on_screen(h);
        assert_eq!(lines.len(), 1, "{how}: the first task's one line");
        let xf = h.editor().canvas.view_xform();
        let want: Vec<Pos2> = line.iter().map(|p| xf.paper_to_screen(*p)).collect();
        let centres: Vec<Pos2> = lines[0].iter().map(|dab| dab.center).collect();
        assert_eq!(centres.len(), want.len(), "{how}");
        centres.iter().zip(&want).for_each(|(a, b)| assert!(a.distance(*b) < 0.01, "{how}: {a:?} and {b:?}"));
        assert!(was != centres, "{how}: the line moved");
        was = centres;
    };
    read(&h, "as it opens");
    h.editor_mut().canvas.zoom_in();
    settle(&mut h);
    read(&h, "zoomed");
    h.editor_mut().canvas.set_view_rotation(30.0);
    settle(&mut h);
    read(&h, "turned");
    h.editor_mut().canvas.toggle_mirror();
    settle(&mut h);
    read(&h, "mirrored");
}

/// Spec 5.7: only the current task's lines stay blue, so the paper clears as
/// you go.
#[test]
fn auguide_only_the_current_task_is_blue() {
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    assert_eq!(guide_on_screen(&h).len(), 1);
    pen(&mut h, &along([200.0, 200.0], [1000.0, 200.0]));
    assert_eq!(at(&h), 1);
    assert!(guide_on_screen(&h).is_empty(), "the next task draws nothing");
}

/// Spec 5.7: under the art, unless `over` puts a line above an opaque
/// background.
#[test]
fn auguide_over_paints_above_the_art() {
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    pen(&mut h, &along([300.0, 300.0], [800.0, 300.0]));
    let line = |y: f32, over: bool| GuideLine::of(&[[300.0, y, 1.0], [800.0, y, 1.0]], 4.0, over);
    h.editor_mut().canvas.guide.lines = vec![line(320.0, true), line(340.0, false)];
    h.run_frames(2);
    let xf = h.editor().canvas.view_xform();
    let flat = painted(&h);
    let index_of = |y: f32| {
        let at = xf.paper_to_screen(pos2(300.0, y));
        let found = |s: &Shape| matches!(s, Shape::Circle(c) if c.fill == plate::GUIDE && c.center.distance(at) < 0.5);
        flat.iter().position(found).expect("the line was painted")
    };
    // The pen's stroke is the art here: the Rig's stroke takes the vector route,
    // which the render paints in its own place.
    let (left, right) = (xf.paper_to_screen(pos2(300.0, 300.0)).x, xf.paper_to_screen(pos2(800.0, 300.0)).x);
    let on_the_stroke = |s: &Shape| {
        let y = xf.paper_to_screen(pos2(300.0, 300.0)).y;
        matches!(s, Shape::Circle(c) if c.fill != plate::GUIDE && (c.center.y - y).abs() < 2.0 && (left..=right).contains(&c.center.x))
    };
    let art = flat.iter().position(on_the_stroke).expect("the art was painted");
    assert!(index_of(340.0) < art, "a plain line goes under the art");
    assert!(art < index_of(320.0), "an over line goes above it");
}

/// Spec 5.6: `Hold to see it` is the Amateur's: the finish, full size, while
/// the pointer is down.
#[test]
fn auguide_hold_shows_the_finish() {
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
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe-square");
    let lesson = row("probe-square");
    h.editor_mut().begin_lesson(lesson, 1);
    settle(&mut h);
    assert!(guide_on_screen(&h).is_empty(), "this task has no lines");
    let rects = h.rects(HOLD);
    assert_eq!(rects.len(), 1, "the card's third foot row");
    press(&mut h, rects[0].center(), true);
    assert_eq!(guide_on_screen(&h).len(), lesson.finish.len(), "the finish, while held");
    press(&mut h, rects[0].center(), false);
    assert!(guide_on_screen(&h).is_empty(), "the paper clears on letting go");
    for id in ["probe", "probe-last"] {
        let mut h = EditorHarness::as_shipped();
        open(&mut h, id);
        assert!(h.rects(HOLD).is_empty(), "{id}: an Amateur's row");
    }
}

/// Spec 5.6: the row shows a finish on the paper, so it stands where a paper
/// does, and `Leave lesson` stays under the hand when it goes.
#[test]
fn auguide_hold_stands_only_over_a_paper() {
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe-square");
    h.editor_mut().begin_lesson(row("probe-square"), 1);
    settle(&mut h);
    assert_eq!(h.rects(HOLD).len(), 1, "on the paper");
    let leave = h.rects(LEAVE);
    let walk = [(Stage::Composite, "behind the Viewer's tab"), (Stage::Export, "no Canvas pane")];
    for (stage, how) in walk {
        h.editor_mut().enter_room(stage);
        settle(&mut h);
        assert!(!visible_panes(&h.editor().dock).contains(&Pane::Canvas), "{how}: the fixture's premise");
        assert!(h.rects(HOLD).is_empty(), "{how}: nothing to see, so no row");
        assert_eq!(h.rects(LEAVE), leave, "{how}: the foot keeps its place");
    }
    h.editor_mut().enter_room(Stage::Drawing);
    settle(&mut h);
    assert_eq!(h.rects(HOLD).len(), 1, "back on the paper");
    h.editor_mut().canvas.composite_view = true;
    settle(&mut h);
    assert!(h.rects(HOLD).is_empty(), "a composite view hides the guide too");
}

// ---- Show me (spec 5.9) ----

const TWO_LINES: &[Task] = &[Task { guide: &[0, 2], ..on_paper(Done::Ink(&[0, 2]), Show::Trace(&[0, 2])) }, NEVER];

/// `Show me` pressed, a frame or two on: `h.click` runs until the clock has
/// landed the whole reveal.
fn press_show_me(h: &mut EditorHarness) {
    h.kit_mut().get_by_label(SHOW_ME).click();
    h.run_frames(3);
}

/// The samples of `path` that are painted on the armed layer.
fn alpha_along(h: &EditorHarness, path: &Path) -> usize {
    path.points.iter().filter(|p| texel(h.state(), p[0] as i32, p[1] as i32)[3] > 0.0).count()
}

/// Spec 5.9: a trace of two lines is one undo, and ticks as two strokes do.
#[test]
fn auguide_show_me_is_one_undo() {
    let art = (probe().art)();
    let mut h = run_of(TWO_LINES);
    let (undos, strokes) = (h.state().engine.undo_depth_by(None), h.state().strokes_done);
    press_show_me(&mut h);
    assert_eq!(h.state().strokes_done, strokes, "the hand is still on its way");
    h.wait(REVEAL_SECS * 2.0 + 0.2);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, strokes + 2, "a stroke each, to the signal");
    assert_eq!(h.state().engine.undo_depth_by(None), undos + 1, "one undo");
    assert_eq!(at(&h), 1, "it ticks as a stroke does");
    assert!(alpha_along(&h, &art[0]) > 0 && alpha_along(&h, &art[2]) > 0, "both lines landed");
    h.perform(Action::Undo);
    settle(&mut h);
    assert_eq!(alpha_along(&h, &art[0]) + alpha_along(&h, &art[2]), 0, "one undo takes both");
}

/// Spec 5.9: while the hand is on its way the line grows as ink, and a task it
/// has left shows it no more.
#[test]
fn auguide_reveal_grows_at_a_hands_pace() {
    let mut h = run_of(TWO_LINES);
    press_show_me(&mut h);
    let shown = |h: &EditorHarness| h.editor().canvas.guide.reveal.iter().map(|l| l.points.len()).sum::<usize>();
    let early = shown(&h);
    assert!(early > 0, "the hand has started");
    h.run_frames(3);
    assert!(shown(&h) > early, "and goes on");
    h.wait(REVEAL_SECS * 2.0);
    settle(&mut h);
    assert_eq!(shown(&h), 0, "landed, the reveal is gone");
}

/// Spec 5.9: pen down cancels the reveal: nothing lands, and the pen's own
/// stroke does.
#[test]
fn auguide_pen_down_cancels_the_reveal() {
    let art = (probe().art)();
    let mut h = run_of(TWO_LINES);
    let strokes = h.state().strokes_done;
    press_show_me(&mut h);
    assert!(!h.editor().canvas.guide.reveal.is_empty(), "the hand is on its way");
    let own = [[300.0, 500.0, 1.0], [420.0, 520.0, 1.0], [560.0, 500.0, 1.0]];
    let drawn = on_screen(&h, &own);
    h.pen_down(drawn[0]);
    h.pen_move(drawn[1]);
    h.run_frames(2);
    assert!(h.editor().canvas.guide.reveal.is_empty(), "pen down cancels");
    h.wait(REVEAL_SECS * 3.0);
    h.pen_move(drawn[2]);
    h.pen_up(drawn[2]);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, strokes + 1, "the pen's own stroke, and no other");
    assert_eq!(alpha_along(&h, &art[0]) + alpha_along(&h, &art[2]), 0, "nothing of the line landed");
    let drawing = h.state().current_drawing().and_then(|id| h.state().cut().drawing(id).cloned());
    let drawing = drawing.expect("the pen's cel");
    assert_eq!(drawing.strokes.len(), 1, "the pen's stroke stands");
    assert_eq!(at(&h), 0, "off the two lines, it answers neither");
}

/// `h`, past any reveal, with nothing to show for it.
fn landed_nothing(h: &mut EditorHarness, art: &[Path], strokes: u64, how: &str) {
    assert!(h.editor().canvas.guide.reveal.is_empty(), "{how}: the reveal goes");
    let undos = h.state().engine.undo_depth_by(None);
    h.wait(REVEAL_SECS * 3.0);
    settle(h);
    assert_eq!(h.state().strokes_done, strokes, "{how}: no stroke");
    assert_eq!(h.state().engine.undo_depth_by(None), undos, "{how}: no undo");
    assert_eq!(alpha_along(h, &art[0]) + alpha_along(h, &art[2]), 0, "{how}: no ink");
}

/// Spec 5.9: a reveal stands over the paper it set out for: a pill pressed on
/// the way, in any step's arming, lands nothing there, and the task stays open.
#[test]
fn auguide_a_pill_cancels_the_reveal() {
    let art = (probe().art)();
    for (pill, stage) in [(crate::vocab::STEP_COLOR, Stage::InkPaint), (crate::vocab::STEP_EXPORT, Stage::Export)] {
        let mut h = run_of(TWO_LINES);
        let strokes = h.state().strokes_done;
        press_show_me(&mut h);
        assert!(!h.editor().canvas.guide.reveal.is_empty(), "{pill}: the hand is on its way");
        h.kit_mut().get_by_label(pill).click();
        h.run_frames(3);
        assert_eq!(h.editor().stage, Some(stage));
        landed_nothing(&mut h, &art, strokes, pill);
        assert_eq!(at(&h), 0, "{pill}: the task is still to do");
    }
}

/// Spec 5.9: the same for what the ink would land on: the layer or the track
/// moved under the hand, or the paper gone behind a tab.
#[test]
fn auguide_a_moved_layer_cancels_the_reveal() {
    let art = (probe().art)();
    let moves: [(&str, fn(&mut EditorHarness)); 3] = [
        ("the layer", |h| h.state_mut().view.active_layer = "Other".to_string()),
        ("the track", |h| h.state_mut().view.active_column = anim_core::ids::ColumnId(u64::MAX)),
        ("the paper", |h| h.editor_mut().dock = Stage::Export.dock()),
    ];
    for (how, moved) in moves {
        let mut h = run_of(TWO_LINES);
        let strokes = h.state().strokes_done;
        press_show_me(&mut h);
        assert!(!h.editor().canvas.guide.reveal.is_empty(), "{how}: the hand is on its way");
        moved(&mut h);
        h.run_frames(2);
        landed_nothing(&mut h, &art, strokes, how);
    }
}

/// Spec 5.9: a paper the pen would refuse is refused at the press, as the
/// circle's is, with no hand sent out first.
#[test]
fn auguide_show_me_refuses_at_the_press() {
    let mut h = run_of(TWO_LINES);
    h.editor_mut().canvas.composite_view = true;
    settle(&mut h);
    let refusals = h.state().refusal_seq;
    press_show_me(&mut h);
    assert_eq!(h.state().refusal_seq, refusals + 1, "the pen's own refusal");
    assert!(h.editor().canvas.guide.reveal.is_empty(), "no hand set out");
    h.editor_mut().canvas.composite_view = false;
    settle(&mut h);
    pen(&mut h, &along([300.0, 800.0], [500.0, 800.0]));
    let layer = h.state().active_layer_id().expect("the armed layer");
    let props = anim_core::model::LayerProps { visible: false, ..h.state().active_layer_props().expect("props").clone() };
    h.state_mut().layer_set_props(layer, props);
    settle(&mut h);
    let refusals = h.state().refusal_seq;
    press_show_me(&mut h);
    assert_eq!(h.state().refusal_seq, refusals + 1, "a hidden layer");
    assert!(h.editor().canvas.guide.reveal.is_empty());
}

/// Deckle on the paper, as the Rookie lessons' guide will stand him.
fn deckle_art() -> Vec<Path> {
    art::deckle(&poses()[0], SEED).paths()
}

const DECKLE_TASKS: &[Task] = &[
    Task { guide: &[0, 1, 2, 3, 4], ..on_paper(Done::Strokes(5), Show::Trace(&[0, 1, 2, 3, 4])) },
    NEVER,
];

/// Deckle's guide, to trace.
pub(crate) fn deckle_lesson() -> Lesson {
    Lesson { art: deckle_art, tasks: DECKLE_TASKS, ..probe() }
}

/// Spec 5.13: a dab stands on the paper as a dot, round and as wide as it lands,
/// not a sliver of its two samples.
#[test]
fn auguide_a_dab_stands_round() {
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    h.editor_mut().begin_lesson(deckle_lesson(), 0);
    settle(&mut h);
    let art = deckle_art();
    let zoom = h.editor().canvas.view_xform().zoom;
    let lines = guide_on_screen(&h);
    assert_eq!(lines.len(), art.len(), "two lines and three dots");
    for (dabs, path) in lines[2..].iter().zip(&art[2..]) {
        assert!(path.is_press(), "the fixture's premise");
        let full = path.width * zoom * 0.5;
        assert_eq!(dabs.len(), path.points.len());
        dabs.iter().for_each(|dab| assert!((dab.radius - full).abs() <= full * 0.15, "{} for {full}", dab.radius));
    }
}

/// Spec 5.13: the guide tapers as the pen will: the line thins at its tips, to
/// the width it lands at.
#[test]
fn auguide_guide_tapers_as_it_lands() {
    let art = (probe().art)();
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    h.editor_mut().canvas.zoom_in();
    settle(&mut h);
    let zoom = h.editor().canvas.view_xform().zoom;
    let lines = guide_on_screen(&h);
    let (tip, body) = (&lines[0][0], &lines[0][lines[0].len() / 2]);
    assert!(tip.radius < body.radius, "{} and {}", tip.radius, body.radius);
    let half = |i: usize| art[0].width * art[0].points[i][2] * zoom * 0.5;
    assert!((body.radius - half(lines[0].len() / 2)).abs() < 0.01, "the width commit_path lands");
}

/// Spec 5.9: a press is a touch, a fraction of a stroke's time: three dots do
/// not stand still for four seconds.
#[test]
fn auguide_a_press_takes_a_touch() {
    const DOTS: &[Task] = &[Task { guide: &[2, 3, 4], ..on_paper(Done::Strokes(3), Show::Trace(&[2, 3, 4])) }, NEVER];
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    h.editor_mut().begin_lesson(Lesson { art: deckle_art, tasks: DOTS, ..probe() }, 0);
    settle(&mut h);
    assert!(PRESS_SECS * 5.0 <= REVEAL_SECS, "a fraction of a stroke");
    press_show_me(&mut h);
    h.wait(PRESS_SECS * 3.0 + 0.2);
    settle(&mut h);
    assert_eq!(at(&h), 1, "landed");
}

/// Spec 5.9: an eraser's line: `Trace` of a wipe path takes the ink off, and
/// `Erased` reads it.
#[test]
fn auguide_a_wipe_erases_and_ticks() {
    fn wiped() -> Vec<Path> {
        let mut art = (probe().art)();
        art.push(art::hand(&[[200.0, 200.0], [1000.0, 200.0]], 24.0, PathKind::Wipe, 9));
        art
    }
    const TASKS: &[Task] = &[
        Task { guide: &[0], ..on_paper(Done::Strokes(1), Show::Trace(&[0])) },
        on_paper(Done::Erased(0), Show::Trace(&[5])),
        NEVER,
    ];
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    h.editor_mut().begin_lesson(Lesson { tasks: TASKS, art: wiped, ..probe() }, 0);
    settle(&mut h);
    let art = wiped();
    h.click(SHOW_ME);
    h.wait(REVEAL_SECS + 0.2);
    settle(&mut h);
    assert_eq!(at(&h), 1);
    assert!(alpha_along(&h, &art[0]) > 0, "the line stands");
    press_show_me(&mut h);
    assert!(h.editor().canvas.guide.reveal.is_empty(), "a wipe is no ink to reveal");
    assert!(!h.editor().canvas.guide.rubbing.is_empty(), "it is a soft mark on the way");
    h.wait(REVEAL_SECS + 0.2);
    settle(&mut h);
    assert_eq!(at(&h), 2, "the line is gone, and the task reads that");
    assert_eq!(alpha_along(&h, &art[0]), 0);
    assert!(h.editor().canvas.guide.rubbing.is_empty(), "landed, the mark is gone");
}

fn fresh(w: u32, h: u32) -> (AppState, usize) {
    let mut state = AppState::new_project("path", w, h, 24, 300.0);
    state.new_drawing_at_frame();
    let slot = state.active_slot_resolved().expect("an armed layer");
    (state, slot)
}

/// Spec 5.9: over a full page, only the texels near the segments are tested.
/// Ceiling set from the first measure, 111 072 on this diagonal, against the
/// page's 6 000 000 and the 5 801 600 of its bounding box's.
#[test]
fn auguide_commit_path_walks_segment_boxes() {
    const CEILING: u64 = 120_000;
    let points = [[20.0, 20.0, 1.0], [2980.0, 1980.0, 1.0]];
    let tested = doc::path_cover(&points, 6.0, |_, _, _| {});
    assert!(tested <= CEILING, "{tested} texels tested");
    let tiles = doc::path_tiles(&points, 6.0, [0.0, 0.0, 0.0, 1.0], false);
    assert!(tiles.len() <= 150, "{} tiles of the page's 1 978", tiles.len());
    // A hand's line of the same run, sampled every 3 px, costs the same order.
    let hand = art::hand(&[[20.0, 20.0], [2980.0, 1980.0]], 6.0, PathKind::Finish, 3);
    let tested = doc::path_cover(&hand.points, hand.width, |_, _, _| {});
    assert!(tested <= CEILING * 4, "{tested} texels tested, sampled");
    let (mut state, slot) = fresh(3000, 2000);
    let ink = PathInk { points: &points, width: 6.0, rgba: [0.0, 0.0, 0.0, 1.0], erase: false, inside: false };
    assert!(state.commit_path(&[ink], slot));
    let lit = |x: i32, y: i32| texel(&state, x, y)[3];
    assert!(lit(1500, 1000) > 0.5, "the line's middle");
    assert_eq!(lit(1500, 1100), 0.0, "and no box round it");
}

/// Spec 5.9: with `Keep inside strokes` on, no texel without alpha gains any;
/// with it off, the same path does.
#[test]
fn auguide_commit_path_keeps_inside() {
    let block: Vec<(i32, i32)> = (100..140).flat_map(|y| (100..140).map(move |x| (x, y))).collect();
    let points = [[60.0, 120.0, 1.0], [180.0, 120.0, 1.0]];
    let land = |inside: bool| {
        let (mut state, slot) = fresh(256, 256);
        paint_texels(&mut state, &block, [1.0, 0.0, 0.0, 1.0]);
        let ink = PathInk { points: &points, width: 8.0, rgba: [0.0, 0.0, 1.0, 1.0], erase: false, inside };
        assert!(state.commit_path(&[ink], slot));
        state
    };
    let held = land(true);
    let cells = || (0..256).flat_map(|y| (0..256).map(move |x| (x, y)));
    let stray = cells().filter(|(x, y)| !block.contains(&(*x, *y)) && texel(&held, *x, *y)[3] != 0.0).count();
    assert_eq!(stray, 0, "the lock keeps the empty paper empty");
    assert!(texel(&held, 120, 120)[2] > 0.5, "the ink landed on what was there");
    assert_eq!(texel(&held, 120, 120)[3], 1.0);
    let free = land(false);
    let stray = cells().filter(|(x, y)| !block.contains(&(*x, *y)) && texel(&free, *x, *y)[3] != 0.0).count();
    assert!(stray > 100, "the same path, unlocked: {stray} texels");
}

const FILL: &[Task] = &[on_paper(Done::Fills(1), Show::Fill([600.0, 450.0])), NEVER];

pub(crate) fn pick_fill(h: &mut EditorHarness) {
    let e = h.editor_mut();
    e.canvas.set_tool(CanvasTool::Fill, &mut e.state);
    settle(h);
}

/// Spec 5.9: with no paint engine a tap refuses as it did, the card says what
/// to press, and Show me fills on the CPU, so a Rookie finishes.
#[test]
fn auguide_refused_fill_points_to_show_me() {
    let mut h = run_of(FILL);
    pen(&mut h, &along([300.0, 300.0], [700.0, 300.0]));
    assert!(h.editor().paint.is_none(), "the Rig has no engine");
    assert!(!h.has_text_containing(BY_HAND), "nothing refused yet");
    pick_fill(&mut h);
    let spot = on_screen(&h, &[[600.0, 450.0, 1.0]])[0];
    let refusals = h.state().refusal_seq;
    h.click_pos(spot);
    settle(&mut h);
    assert_eq!(h.state().refusal_seq, refusals + 1, "the tap refuses as today");
    assert!(h.state().refusal.contains(NEEDS_ENGINE), "{}", h.state().refusal);
    assert!(h.has_text_containing(BY_HAND), "the card adds what to press");
    assert_eq!(at(&h), 0);
    let fills = h.state().fills_done;
    h.click(SHOW_ME);
    settle(&mut h);
    assert_eq!(h.state().fills_done, fills + 1, "fill_at is CPU work");
    assert_eq!(at(&h), 1, "the task ticks");
    assert!(!h.has_text_containing(BY_HAND), "the next task is another task");
}

/// Spec 5.9: an Experienced card has no such line: its button only rings.
#[test]
fn auguide_no_by_hand_line_for_the_experienced() {
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe-last");
    h.editor_mut().begin_lesson(Lesson { tasks: FILL, ..row("probe-last") }, 0);
    settle(&mut h);
    pick_fill(&mut h);
    let spot = on_screen(&h, &[[500.0, 500.0, 1.0]])[0];
    h.click_pos(spot);
    settle(&mut h);
    assert!(h.state().refusal.contains(NEEDS_ENGINE), "{}", h.state().refusal);
    assert!(!h.has_text_containing(BY_HAND));
}

/// Spec 5.9, 5.10: no lesson calls `enter_room`: with no canvas on screen the
/// circle does nothing, where outside a lesson it walks to one.
#[test]
fn auguide_circle_keeps_to_the_room_in_a_lesson() {
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    h.perform(Action::RoomExport);
    settle(&mut h);
    h.editor_mut().export_from = None;
    assert!(!visible_panes(&h.editor().dock).contains(&Pane::Canvas), "the room has no canvas");
    let stage = h.editor().stage;
    assert_eq!(h.editor_mut().demo_circle(), Performed::Ignored);
    settle(&mut h);
    assert_eq!(h.editor().stage, stage, "the person walks");
    h.click(LEAVE);
    settle(&mut h);
    assert_ne!(h.editor_mut().demo_circle(), Performed::Ignored, "out of a lesson it still goes to the canvas");
}

/// Spec 5.11: leaving refits the paper for the room the card gave back.
#[test]
fn auguide_leave_refits_the_paper() {
    let mut h = EditorHarness::as_shipped();
    open(&mut h, "probe");
    h.editor_mut().canvas.zoom_in();
    h.editor_mut().canvas.zoom_in();
    settle(&mut h);
    h.click(LEAVE);
    settle(&mut h);
    let left = h.editor().canvas.view_xform();
    h.editor_mut().canvas.fit_view();
    settle(&mut h);
    let fit = h.editor().canvas.view_xform();
    assert_eq!((left.zoom, left.pan), (fit.zoom, fit.pan), "left at a fit");
}

// ---- Deckle and the art's checks (spec 5.13, 5.14) ----

const SEED: u32 = 7;
const STRIP: f32 = 420.0;

fn poses() -> Vec<Vec<Pt>> {
    art::walk([120.0, 700.0], STRIP, 150.0, 7)
}

fn xy(path: &Path) -> Vec<Pt> {
    path.points.iter().map(|p| [p[0], p[1]]).collect()
}

/// Deckle's two sides without the turns round his ends: cut, then torn.
fn sides(pose: &[Pt]) -> [Vec<Pt>; 2] {
    let deckle = art::deckle(pose, SEED);
    [xy(&deckle.cut)[..deckle.sides[0]].to_vec(), xy(&deckle.torn)[..deckle.sides[1]].to_vec()]
}

fn cut_side(pose: &[Pt]) -> Vec<Pt> {
    let [cut, _] = sides(pose);
    cut
}

fn apart(a: Pt, b: Pt) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Spec 5.13: paper does not stretch: every pose's arc is the strip's within 1%.
#[test]
fn auguide_deckle_keeps_its_length() {
    let poses = poses();
    assert!(poses.len() >= 6);
    let flat = poses.iter().map(|p| apart(p[0], p[p.len() - 1])).fold(f32::MIN, f32::max);
    let arched = poses.iter().map(|p| apart(p[0], p[p.len() - 1])).fold(f32::MAX, f32::min);
    assert!(arched < flat * 0.8, "the poses bend: {arched} of {flat}");
    for (i, pose) in poses.iter().enumerate() {
        let spine = art::length(pose);
        assert!((spine / STRIP - 1.0).abs() <= 0.01, "pose {i}: spine {spine}");
        let drawn = art::length(&cut_side(pose));
        assert!((drawn / STRIP - 1.0).abs() <= 0.01, "pose {i}: the drawn side {drawn}");
    }
    // The torn side is the same paper: its nicks run it long, the same long on
    // every pose and through the walk's tightest bends.
    let flat = art::length(&sides(&art::spine([0.0, 0.0], [STRIP, 0.0], STRIP))[1]);
    let bends = [0.7, 0.45].map(|share| art::spine([0.0, 0.0], [STRIP * share, 0.0], STRIP));
    for (i, pose) in poses.iter().map(Vec::as_slice).chain(bends.iter().map(Vec::as_slice)).enumerate() {
        let torn = art::length(&sides(pose)[1]);
        assert!((torn / flat - 1.0).abs() <= 0.01, "pose {i}: the torn side {torn}, flat {flat}");
    }
}

/// Spec 5.13: an eye, not a third hole: well over twice their size, off the
/// strip's middle line, and in the open; the holes run in a row by the straight
/// edge, and the torn edge is torn at a glance.
#[test]
fn auguide_deckle_has_an_eye_and_a_tear() {
    const WIDE: f32 = STRIP * 0.2;
    const TORN_DEEP: f32 = 8.0;
    let flat = art::spine([0.0, 0.0], [STRIP, 0.0], STRIP);
    for (i, pose) in std::iter::once(&flat).chain(&poses()).enumerate() {
        let deckle = art::deckle(pose, SEED);
        let [cut, torn] = sides(pose);
        let centre = |p: &Path| [(p.points[0][0] + p.points[1][0]) * 0.5, (p.points[0][1] + p.points[1][1]) * 0.5];
        let eye = centre(&deckle.eye);
        assert!(deckle.eye.width >= deckle.holes[0].width * 2.0, "pose {i}: {} and {}", deckle.eye.width, deckle.holes[0].width);
        let clear = cut.iter().chain(&torn).map(|p| apart(*p, eye)).fold(f32::MAX, f32::min);
        let touch = (deckle.eye.width + deckle.cut.width) * 0.5;
        assert!(clear >= touch + 1.0, "pose {i}: the eye touches a side: {clear} for {touch}");
        for hole in &deckle.holes {
            let near = cut.iter().map(|p| apart(*p, centre(hole))).fold(f32::MAX, f32::min);
            assert!(near <= WIDE * 0.3, "pose {i}: a hole {near} off the straight edge");
        }
    }
    let deckle = art::deckle(&flat, SEED);
    let eye = deckle.eye.points[0][1];
    let across = |y: f32| -y / WIDE;
    assert!((across(eye) - 0.5).abs() >= 0.08, "the eye stands on the middle line: {}", across(eye));
    let holes = deckle.holes.each_ref().map(|hole| hole.points[0][1]);
    assert!((holes[0] - holes[1]).abs() < 1.0, "a punched row");
    let [_, torn] = sides(&flat);
    let (low, high) = torn.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p[1]), h.max(p[1])));
    assert!(high - low >= TORN_DEEP, "the tear is {} deep", high - low);
}

/// Spec 5.13: from each pose to the next one foot stays put, so nothing skates,
/// and the strip still gets somewhere.
#[test]
fn auguide_deckle_feet_stay_planted() {
    let poses: Vec<Vec<Pt>> = poses().iter().map(|p| cut_side(p)).collect();
    for (i, pair) in poses.windows(2).enumerate() {
        let (a, b) = (&pair[0], &pair[1]);
        let tail = apart(a[0], b[0]);
        let head = apart(a[a.len() - 1], b[b.len() - 1]);
        assert!(tail.min(head) <= 2.0, "poses {i}, {}: tail {tail}, head {head}", i + 1);
        assert!(tail.max(head) >= 50.0, "poses {i}, {}: the other foot moves: tail {tail}, head {head}", i + 1);
    }
    let walked = poses[poses.len() - 1][0][0] - poses[0][0][0];
    assert!(walked >= 300.0, "it walked {walked}");
}

/// The art a lesson's gate reads.
fn lesson_lines() -> Vec<(String, Path)> {
    let lessons = lessons::for_gate().iter().flat_map(|l| {
        let art = (l.art)().into_iter().enumerate();
        art.map(|(i, p)| (format!("{} path {i}", l.id), p)).collect::<Vec<_>>()
    });
    lessons.collect()
}

/// The walk, and these bends: a share of the strip, tail to head.
fn bent_poses(shares: &[f32]) -> Vec<Vec<Pt>> {
    let bends = shares.iter().map(|share| art::spine([0.0, 0.0], [STRIP * share, 0.0], STRIP));
    poses().into_iter().chain(bends).collect()
}

/// That art, and Deckle, which every lesson draws from.
fn every_line() -> Vec<(String, Path)> {
    let deckle = bent_poses(&[1.0, 0.7, 0.45]).into_iter().enumerate().flat_map(|(i, pose)| {
        let lines = art::deckle(&pose, SEED).paths().into_iter().enumerate();
        lines.map(move |(n, p)| (format!("Deckle pose {i} line {n}"), p)).collect::<Vec<_>>()
    });
    lesson_lines().into_iter().chain(deckle).collect()
}

/// That art as a hand reads it. Deckle's corners stand where `sides` says, for
/// on an arch his end meets the side at so shallow a turn that no bend could
/// call it one, and a corner is no wobble. The bend at 0.45, past anything the
/// walk does, turns more than a parabola follows over `WINDOW`: read there, it
/// would read the arch and not the hand, so it stays out.
fn hand_lines() -> Vec<(String, Path)> {
    let deckle = bent_poses(&[1.0, 0.7]).into_iter().enumerate().flat_map(|(i, pose)| {
        let drawn = art::deckle(&pose, SEED);
        let [cut, torn] = drawn.sides;
        let part = |path: &Path, from: usize, to: usize| Path::new(path.points[from..to].to_vec(), path.width);
        let ends = [part(&drawn.cut, cut, drawn.cut.points.len()), part(&drawn.torn, torn, drawn.torn.points.len())];
        let lines = [part(&drawn.cut, 0, cut), part(&drawn.torn, 0, torn)].into_iter().chain(ends).enumerate();
        lines.map(move |(n, p)| (format!("Deckle pose {i} side {n}"), p)).collect::<Vec<_>>()
    });
    lesson_lines().into_iter().chain(deckle).collect()
}

/// `points` cut at every sample that turns sharply: a corner is the drawing's, no
/// wobble. Two corners can stand a sample or two apart, as the tear's nicks do,
/// and each sample of a corner is a cut, so what is left between is plain line.
fn runs(points: &[Pt]) -> Vec<Vec<Pt>> {
    const READ: usize = 2;
    const CORNER: f32 = 0.9;
    let heading = |a: Pt, b: Pt| (b[1] - a[1]).atan2(b[0] - a[0]);
    let (mut out, mut from) = (Vec::new(), 0);
    for i in READ..points.len().saturating_sub(READ) {
        let bend = heading(points[i], points[i + READ]) - heading(points[i - READ], points[i]);
        if bend.sin().atan2(bend.cos()).abs() > CORNER {
            out.push(points[from..=i].to_vec());
            from = i;
        }
    }
    out.push(points[from..].to_vec());
    out
}

/// `stray`'s window, in samples: 129 px, four of the hand's runs, so the smooth
/// curve keeps a drawn line and loses the hand.
const WINDOW: usize = 43;

/// How far sample `at` of `points` stands from the smooth curve: the parabola
/// fitted to the samples nearest it, up to `WINDOW`, the ends' window held in
/// from the end, so a short line and a line's ends are read like its middle.
fn stray(points: &[Pt], at: usize) -> f32 {
    let n = WINDOW.min(points.len());
    let from = at.saturating_sub(n / 2).min(points.len() - n);
    let window = &points[from..from + n];
    let xs: Vec<f64> = (0..n).map(|i| i as f64 - (n - 1) as f64 * 0.5).collect();
    let spread = xs.iter().map(|x| x * x).sum::<f64>() / n as f64;
    // The three powers, made orthogonal over the window: each reads alone.
    let bases: [&dyn Fn(f64) -> f64; 3] = [&|_| 1.0, &|x| x, &|x| x * x - spread];
    let fitted = |c: usize| -> f32 {
        let term = |base: &&dyn Fn(f64) -> f64| {
            let top: f64 = xs.iter().zip(window).map(|(x, p)| base(*x) * f64::from(p[c])).sum();
            let low: f64 = xs.iter().map(|x| base(*x) * base(*x)).sum();
            top / low.max(1e-9) * base(xs[at - from])
        };
        bases.iter().map(term).sum::<f64>() as f32
    };
    apart(points[at], [fitted(0), fitted(1)])
}

/// The hand readings of a line past `LONG`: one per stretch between its corners
/// that is past it too, and if the corners leave none, one for the line, its
/// widest stretch's. A line past 60 px always gets a reading.
fn hand_strays(path: &Path) -> Vec<f32> {
    let points = xy(path);
    if art::length(&points) <= LONG {
        return Vec::new();
    }
    let widest = |run: &Vec<Pt>| (0..run.len()).map(|i| stray(run, i)).fold(0.0, f32::max);
    let stretches = runs(&points);
    let long: Vec<f32> = stretches.iter().filter(|run| art::length(run) > LONG).map(widest).collect();
    if long.is_empty() { vec![stretches.iter().map(widest).fold(0.0, f32::max)] } else { long }
}

/// A line `long` px along, every sample ruled on its place.
fn ruled(long: f32) -> Path {
    let samples = (long / art::HAND_STEP) as usize;
    Path::new((0..=samples).map(|i| [i as f32 * art::HAND_STEP, 100.0, 1.0]).collect(), 2.0)
}

/// Spec 5.14: 0.4 to 1.6 px from the smooth curve, on every line past 60 px.
#[test]
fn auguide_art_wobbles_like_a_hand() {
    let mut read = 0;
    for (name, path) in hand_lines() {
        let strays = hand_strays(&path);
        assert_eq!(strays.is_empty(), art::length(&xy(&path)) <= LONG, "{name}: no reading");
        for stray in strays {
            assert!((0.4..=1.6).contains(&stray), "{name}: {stray}");
            read += 1;
        }
    }
    assert!(read >= 14, "{read} lines read");
    // A short line is a hand's too, and a line cut up by corners.
    for long in [66.0, 100.0, 180.0] {
        let line = art::hand(&[[0.0, 100.0], [long, 100.0]], 2.0, PathKind::Guide, 4);
        hand_strays(&line).iter().for_each(|s| assert!((0.4..=1.6).contains(s), "a hand's {long} px: {s}"));
        assert!(hand_strays(&ruled(long)).iter().all(|s| *s < 0.4), "ruled, {long} px");
    }
    assert!(!hand_strays(&ruled(100.0)).is_empty(), "the ruled 100 px line is read");
    let steps: Vec<[f32; 3]> = (0..=60).map(|i| [i as f32 * 3.0, ((i / 12) % 2 * 30) as f32, 1.0]).collect();
    let steps = hand_strays(&Path::new(steps, 2.0));
    assert!(!steps.is_empty() && steps.iter().all(|s| *s < 0.4), "ruled runs, cut at their corners: {steps:?}");
    let shaking: Vec<[f32; 3]> = (0..=60).map(|i| [i as f32 * 3.0, 100.0 + (i % 2) as f32 * 4.0, 1.0]).collect();
    assert!(hand_strays(&Path::new(shaking, 2.0)).iter().any(|s| *s > 1.6));
}

/// `points`' least-squares circle: `(centre, radius)`, Kasa's.
fn circle_of(points: &[Pt]) -> Option<(Pt, f32)> {
    let mean = |c: usize| points.iter().map(|p| p[c]).sum::<f32>() / points.len() as f32;
    let (mx, my) = (mean(0), mean(1));
    let (mut uu, mut uv, mut vv, mut uuu, mut vvv, mut uvv, mut vuu) = (0.0f64, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for p in points {
        let (u, v) = (f64::from(p[0] - mx), f64::from(p[1] - my));
        uu += u * u;
        uv += u * v;
        vv += v * v;
        uuu += u * u * u;
        vvv += v * v * v;
        uvv += u * v * v;
        vuu += v * u * u;
    }
    let det = uu * vv - uv * uv;
    if det.abs() < 1e-6 {
        return None;
    }
    let (a, b) = ((uuu + uvv) * 0.5, (vvv + vuu) * 0.5);
    let (uc, vc) = ((a * vv - b * uv) / det, (b * uu - a * uv) / det);
    let radius = (uc * uc + vc * vc + (uu + vv) / points.len() as f64).sqrt();
    Some(([mx + uc as f32, my + vc as f32], radius as f32))
}

/// How far `points` go round, in degrees.
fn turning(points: &[Pt]) -> f32 {
    let heading = |w: &[Pt]| (w[1][1] - w[0][1]).atan2(w[1][0] - w[0][0]);
    let bends = points.windows(3).map(|w| {
        let bend = heading(&w[1..]) - heading(&w[..2]);
        bend.sin().atan2(bend.cos())
    });
    bends.sum::<f32>().abs().to_degrees()
}

/// A stock arc: every sample within 3% of one circle, and past `STOCK_TURN` round
/// it. Below that any gentle curve fits a circle within 3% (a parabola does at
/// 95 degrees and not at 100), so the reading would not mean a circle.
const STOCK_TURN: f32 = 100.0;

fn is_stock(points: &[Pt]) -> bool {
    circle_of(points).is_some_and(|(centre, radius)| {
        let near = points.iter().all(|p| (apart(*p, centre) / radius - 1.0).abs() <= 0.03);
        near && turning(points) >= STOCK_TURN
    })
}

/// `degrees` of a compass's circle.
fn compass(degrees: f32) -> Vec<Pt> {
    let at = |i: usize| (i as f32 * degrees / 48.0).to_radians();
    (0..=48).map(at).map(|a| [200.0 + 60.0 * a.cos(), 200.0 + 60.0 * a.sin()]).collect()
}

/// Spec 5.14: no long line is a stock shape. A press is exempt.
#[test]
fn auguide_art_has_no_stock_shape() {
    for (name, path) in every_line() {
        let points = xy(&path);
        if art::length(&points) > LONG {
            assert!(!is_stock(&points), "{name} fits a circle");
        }
    }
    for degrees in [360.0, 200.0, 120.0] {
        assert!(is_stock(&compass(degrees)), "a compass's {degrees} degrees is read");
        assert!(is_stock(&xy(&art::hand(&compass(degrees), 2.0, PathKind::Guide, 4))), "a shaky {degrees} degrees too");
    }
    let straight = art::hand(&[[0.0, 100.0], [300.0, 100.0]], 2.0, PathKind::Guide, 4);
    assert!(!is_stock(&xy(&straight)), "a straight line is no arc");
    let parabola = |half: f32| -> Vec<Pt> { (-48..=48).map(|i| i as f32 / 48.0).map(|t| [t * 200.0, t * t * 100.0 * half]).collect() };
    assert!(!is_stock(&parabola(4.0)), "a steep parabola is no arc");
    assert!(!is_stock(&parabola(0.7)), "a gentle curve stays under the floor");
}

/// The most lines a Rookie traces on one drawing, presses not counted. A
/// comic's page is one drawing and its panels are the pictures: the lessons
/// file's number there is per panel, and each panel is a task (AU23).
pub(crate) fn trace_load(lesson: &Lesson) -> usize {
    let art = (lesson.art)();
    let (mut load, mut most) = (0, 0);
    for task in lesson.tasks {
        let panel = lesson.kind == ProjectKind::Comic && matches!(task.done, Done::Ink(_));
        if panel || matches!(task.done, Done::Drawings(_)) {
            load = 0;
        }
        load += own_lines(task).iter().filter(|i| !art[usize::from(**i)].is_press()).count();
        most = most.max(load);
    }
    most
}

/// Spec 5.14: a Rookie traces at most six lines to a drawing, and none runs off
/// the paper's margin. Presses do not count; a short line does.
#[test]
fn auguide_trace_load_fits_a_rookie() {
    const LOAD: usize = 6;
    const MARGIN: f32 = 0.03;
    const SEVEN: &[Task] = &[Task { guide: &[0, 1, 2, 3, 4, 5, 6], ..on_paper(Done::Strokes(7), Show::Trace(&[0, 1, 2, 3, 4, 5, 6])) }];
    fn sevens() -> Vec<Path> {
        (0..7).map(|i| art::hand(&[[100.0, 100.0 + i as f32 * 20.0], [130.0, 100.0 + i as f32 * 20.0]], 4.0, PathKind::Guide, i)).collect()
    }
    assert_eq!(trace_load(&Lesson { art: sevens, tasks: SEVEN, ..probe() }), 7, "seven 30 px lines are seven lines");
    assert_eq!(trace_load(&Lesson { art: deckle_art, tasks: DECKLE_TASKS, ..probe() }), 2, "two lines and three presses");
    for lesson in lessons::for_gate().iter().filter(|l| l.class == Class::Rookie) {
        let art = (lesson.art)();
        let most = trace_load(lesson);
        assert!(most <= LOAD, "{}: {most} lines on one drawing", lesson.id);
        let (w, h) = (lesson.paper.width as f32, lesson.paper.height as f32);
        let (mx, my) = (w * MARGIN, h * MARGIN);
        for path in art.iter().filter(|p| !matches!(p.kind, PathKind::Seed | PathKind::Wipe)) {
            let b = path.bounds();
            assert!(b[0] >= mx && b[1] >= my && b[2] <= w - mx && b[3] <= h - my, "{}: {b:?} past the margin", lesson.id);
        }
    }
}

/// `paths` as an ink layer, to flood.
fn inked(paths: &[Path]) -> BTreeMap<(i32, i32), Arc<TileData>> {
    let t = TILE as i32;
    let one = f32_to_f16_bits(1.0);
    let mut tiles: BTreeMap<(i32, i32), Vec<u16>> = BTreeMap::new();
    for path in paths {
        doc::path_cover(&path.points, path.width, |x, y, cover| {
            if cover >= 0.5 {
                let tile = tiles.entry((x.div_euclid(t), y.div_euclid(t))).or_insert_with(|| vec![0; TILE_LEN]);
                tile[((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4 + 3) as usize] = one;
            }
        });
    }
    tiles.into_iter().map(|(c, v)| (c, Arc::new(TileData::from_vec(v)))).collect()
}

/// The flood from `seed` over `paper`, at `gap`: `None` where it finds no
/// region at all.
pub(crate) fn flood(paths: &[Path], seed: (i32, i32), paper: (u32, u32), gap: u32) -> Option<anim_core::fill::FillMask> {
    let opts = FillOpts { threshold: 0.5, gap_px: gap, grow_px: 0, protect_ink: false };
    let area = IRect { x: 0, y: 0, w: paper.0, h: paper.1 };
    flood_fill_mask_in(&inked(paths), seed, area, &opts, None).ok()
}

/// Spec 5.14: the outside flood never reaches Deckle's body, in any pose.
#[test]
fn auguide_deckle_shuts_at_gap_two() {
    let paper = (1200, 900);
    for (i, pose) in poses().iter().enumerate() {
        let d = art::deckle(pose, SEED);
        let (cut, torn) = (xy(&d.cut), xy(&d.torn));
        let (a, b) = (cut[cut.len() / 2], torn[torn.len() / 2]);
        let body = (((a[0] + b[0]) * 0.5) as i32, ((a[1] + b[1]) * 0.5) as i32);
        let lines = d.paths();
        let outside = flood(&lines, (4, 4), paper, GAP).expect("the paper round it");
        assert!(!outside.get(body.0, body.1), "pose {i}: the outside reached {body:?}");
        let inside = flood(&lines, body, paper, GAP).unwrap_or_else(|| panic!("pose {i}: {body:?} is a seed"));
        assert!(!inside.get(4, 4), "pose {i}: the body leaked");
    }
}

/// A seed's flood stays off the paper's edge.
pub(crate) fn bounds_a_region(paths: &[Path], seed: [f32; 2], paper: (u32, u32), gap: u32) -> bool {
    flood(paths, (seed[0] as i32, seed[1] as i32), paper, gap).is_some_and(|mask| {
        let (w, h) = (paper.0 as i32, paper.1 as i32);
        (0..w).all(|x| !mask.get(x, 0) && !mask.get(x, h - 1)) && (0..h).all(|y| !mask.get(0, y) && !mask.get(w - 1, y))
    })
}

/// Spec 5.14: every `Show::Fill` seed of every lesson bounds a region at the
/// lesson's gap.
#[test]
fn auguide_guide_closes_where_filled() {
    let default_gap = CanvasView::new().fill_gap;
    assert_eq!(default_gap, GAP);
    for lesson in lessons::for_gate() {
        let art = (lesson.art)();
        let lines: Vec<&Path> = art.iter().filter(|p| !matches!(p.kind, PathKind::Seed | PathKind::Wipe)).collect();
        let lines: Vec<Path> = lines.iter().map(|p| Path { points: p.points.clone(), ..Path::new(Vec::new(), p.width) }).collect();
        let paper = (lesson.paper.width, lesson.paper.height);
        for (i, task) in lesson.tasks.iter().enumerate() {
            if let Show::Fill(seed) = task.show {
                let gap = lesson.fill_gap.unwrap_or(default_gap);
                assert!(bounds_a_region(&lines, seed, paper, gap), "{} task {i}: {seed:?} leaks", lesson.id);
            }
        }
    }
    // The check reads: Deckle bounds its body, and a seed beside it does not.
    let pose = &poses()[0];
    let d = art::deckle(pose, SEED);
    let (cut, torn) = (xy(&d.cut), xy(&d.torn));
    let (a, b) = (cut[cut.len() / 2], torn[torn.len() / 2]);
    let lines = d.paths();
    assert!(bounds_a_region(&lines, [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5], (1200, 900), GAP));
    assert!(!bounds_a_region(&lines, [4.0, 4.0], (1200, 900), GAP));
}

/// A finish, 64×64.
pub(crate) fn thumb(paths: &[&Path], paper: (u32, u32)) -> Vec<bool> {
    const SIDE: usize = 64;
    let mut out = vec![false; SIDE * SIDE];
    let (sx, sy) = (SIDE as f32 / paper.0 as f32, SIDE as f32 / paper.1 as f32);
    for path in paths {
        let small: Vec<[f32; 3]> = path.points.iter().map(|p| [p[0] * sx, p[1] * sy, 1.0]).collect();
        doc::path_cover(&small, (path.width * sx).max(1.0), |x, y, _| {
            if (0..SIDE as i32).contains(&x) && (0..SIDE as i32).contains(&y) {
                out[y as usize * SIDE + x as usize] = true;
            }
        });
    }
    out
}

pub(crate) fn iou(a: &[bool], b: &[bool]) -> f32 {
    let both = a.iter().zip(b).filter(|(a, b)| **a && **b).count();
    let either = a.iter().zip(b).filter(|(a, b)| **a || **b).count();
    both as f32 / either.max(1) as f32
}

/// Spec 5.14: over all pairs of the nine, no two finishes share more than 0.45
/// of their ink, so a comic cannot be an animation's drawing again.
#[test]
fn auguide_finishes_differ() {
    let finishes: Vec<(&str, Vec<bool>)> = lessons::ALL.iter().map(|l| {
        let art = (l.art)();
        let lines: Vec<&Path> = l.finish.iter().map(|i| &art[usize::from(*i)]).collect();
        (l.id, thumb(&lines, (l.paper.width, l.paper.height)))
    }).collect();
    for (i, (a, ink)) in finishes.iter().enumerate() {
        for (b, other) in &finishes[i + 1..] {
            let shared = iou(ink, other);
            assert!(shared <= 0.45, "{a} and {b} share {shared}");
        }
    }
    // The check reads: one pose against itself, against a pose far along.
    let paper = (1200, 900);
    let lines = |pose: &[Pt]| art::deckle(pose, SEED).paths();
    let poses = poses();
    let (near, far) = (lines(&poses[0]), lines(&poses[6]));
    let near = thumb(&near.iter().collect::<Vec<_>>(), paper);
    let far = thumb(&far.iter().collect::<Vec<_>>(), paper);
    assert_eq!(iou(&near, &near), 1.0);
    assert!(iou(&near, &far) <= 0.45, "{}", iou(&near, &far));
}

/// A white sheet to draw on for the Lead's eye.
struct Sheet {
    w: usize,
    h: usize,
    gray: Vec<u8>,
}

impl Sheet {
    fn new(w: usize, h: usize) -> Sheet {
        Sheet { w, h, gray: vec![255; w * h] }
    }

    fn ink(&mut self, points: &[[f32; 3]], width: f32) {
        let (w, h, gray) = (self.w, self.h, &mut self.gray);
        doc::path_cover(points, width, |x, y, cover| {
            if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                let px = &mut gray[y as usize * w + x as usize];
                *px = (*px).min(255 - (cover * 235.0) as u8);
            }
        });
    }

    fn line(&mut self, from: Pt, to: Pt, width: f32) {
        self.ink(&[[from[0], from[1], 1.0], [to[0], to[1], 1.0]], width);
    }

    fn paths(&mut self, paths: &[Path], scale: f32, place: impl Fn(Pt) -> Pt) {
        for path in paths {
            let scaled: Vec<[f32; 3]> = path.points.iter().map(|p| {
                let q = place([p[0], p[1]]);
                [q[0], q[1], p[2]]
            }).collect();
            self.ink(&scaled, (path.width * scale).max(1.5));
        }
    }

    /// `digit` in seven strokes, `size` tall, its top left at `at`.
    fn digit(&mut self, digit: usize, at: Pt, size: f32) {
        const STROKES: [[usize; 2]; 7] = [[0, 1], [1, 3], [3, 5], [4, 5], [2, 4], [0, 2], [2, 3]];
        const LIT: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];
        let corner = |i: usize| [at[0] + (i % 2) as f32 * size * 0.55, at[1] + (i / 2) as f32 * size * 0.5];
        for (n, [from, to]) in STROKES.into_iter().enumerate() {
            if LIT[digit] >> n & 1 == 1 {
                self.line(corner(from), corner(to), 2.5);
            }
        }
    }

    fn write(&self, file: &FilePath) {
        let file = std::fs::File::create(file).expect("the sheet");
        let mut png = png::Encoder::new(std::io::BufWriter::new(file), self.w as u32, self.h as u32);
        png.set_color(png::ColorType::Grayscale);
        png.set_depth(png::BitDepth::Eight);
        png.write_header().expect("header").write_image_data(&self.gray).expect("pixels");
    }
}

/// Which end of `pose` stands where it does in `other`.
fn planted(pose: &[Pt], other: &[Pt]) -> Pt {
    let (a, b) = (cut_side(pose), cut_side(other));
    let (tail, head) = (apart(a[0], b[0]), apart(a[a.len() - 1], b[b.len() - 1]));
    if tail <= head { a[0] } else { a[a.len() - 1] }
}

/// Spec 5.14: Deckle's bends, his walk as one film strip on one ground line, and
/// the finishes, on one sheet for the Lead's eye. Each frame of the strip stands
/// him where he really is, numbers it and marks the planted foot, read from the
/// poses. A PNG in `UISHOT_DIR`, no window.
#[test]
#[ignore = "needs UISHOT_DIR; writes a PNG"]
fn uishot_deckle_contact_sheet() {
    let dir = PathBuf::from(std::env::var("UISHOT_DIR").expect("set UISHOT_DIR to the output folder"));
    std::fs::create_dir_all(&dir).expect("make UISHOT_DIR");
    contact_sheet(&dir);
}

/// The sheet, in `dir`, and the box of each lesson's finish on it.
pub(crate) fn contact_sheet(dir: &FilePath) -> (PathBuf, Vec<[i32; 4]>) {
    const WIDE: usize = 1800;
    const MARGIN: f32 = 30.0;
    const BEND_LINE: f32 = 230.0;
    const BEND_SCALE: f32 = 0.6;
    const GROUND: f32 = 380.0;
    const WALK_SCALE: f32 = 0.27;
    const FINISHES_AT: usize = 430;
    const COLUMNS: usize = 3;
    const CELL: (usize, usize) = (600, 300);
    const SCALE: f32 = 0.5;
    let finishes: Vec<Vec<Path>> = lessons::ALL.iter().map(|l| {
        let mut art: Vec<Option<Path>> = (l.art)().into_iter().map(Some).collect();
        l.finish.iter().filter_map(|i| art[usize::from(*i)].take()).collect()
    }).collect();
    let mut sheet = Sheet::new(WIDE, FINISHES_AT + finishes.len().div_ceil(COLUMNS) * CELL.1);
    // The bends: the strip at full size, then two of its arches.
    let flat = art::spine([0.0, 0.0], [STRIP, 0.0], STRIP);
    sheet.paths(&art::deckle(&flat, SEED).paths(), 1.0, |p| [MARGIN + p[0], BEND_LINE + p[1]]);
    for (i, share) in [0.7, 0.45].into_iter().enumerate() {
        let pose = art::spine([0.0, 0.0], [STRIP * share, 0.0], STRIP);
        let left = MARGIN + STRIP + 80.0 + i as f32 * STRIP * 0.7;
        sheet.paths(&art::deckle(&pose, SEED).paths(), BEND_SCALE, |p| [left + p[0] * BEND_SCALE, BEND_LINE + p[1] * BEND_SCALE]);
    }
    // The walk.
    let walk = poses();
    let [start, _] = [walk[0][0], walk[0][walk[0].len() - 1]];
    let frame = (WIDE as f32 - 2.0 * MARGIN) / walk.len() as f32;
    sheet.line([MARGIN, GROUND], [WIDE as f32 - MARGIN, GROUND], 2.0);
    for (i, pose) in walk.iter().enumerate() {
        let left = MARGIN + i as f32 * frame;
        let place = |p: Pt| [left + (p[0] - start[0]) * WALK_SCALE, GROUND + (p[1] - start[1]) * WALK_SCALE];
        sheet.paths(&art::deckle(pose, SEED).paths(), WALK_SCALE, place);
        sheet.line([left - 8.0, GROUND - 8.0], [left - 8.0, GROUND + 8.0], 1.5);
        sheet.digit(i + 1, [left + 2.0, GROUND - 100.0], 20.0);
        let other = if i + 1 < walk.len() { i + 1 } else { i - 1 };
        let [x, y] = place(planted(pose, &walk[other]));
        sheet.line([x - 7.0, y + 16.0], [x, y + 4.0], 2.0);
        sheet.line([x + 7.0, y + 16.0], [x, y + 4.0], 2.0);
        sheet.line([x - 7.0, y + 16.0], [x + 7.0, y + 16.0], 2.0);
    }
    // Each lesson's finish.
    let mut cells = Vec::new();
    for (n, paths) in finishes.iter().enumerate() {
        let (cx, cy) = ((n % COLUMNS * CELL.0) as f32, (FINISHES_AT + n / COLUMNS * CELL.1) as f32);
        cells.push([cx as i32, cy as i32, cx as i32 + CELL.0 as i32, cy as i32 + CELL.1 as i32]);
        let all: Vec<f32> = paths.iter().flat_map(|p| p.bounds()).collect();
        let (lx, ly) = (all.iter().step_by(2).fold(f32::MAX, |a, b| a.min(*b)), all[1..].iter().step_by(2).fold(f32::MAX, |a, b| a.min(*b)));
        let (rx, ry) = (all.iter().step_by(2).fold(f32::MIN, |a, b| a.max(*b)), all[1..].iter().step_by(2).fold(f32::MIN, |a, b| a.max(*b)));
        let scale = SCALE.min((CELL.0 - 40) as f32 / (rx - lx)).min((CELL.1 - 40) as f32 / (ry - ly));
        sheet.paths(paths, scale, |p| [(p[0] - lx) * scale + cx + 20.0, (p[1] - ly) * scale + cy + 20.0]);
    }
    let file = dir.join("deckle-sheet.png");
    sheet.write(&file);
    (file, cells)
}

/// Spec 5.3, main spec 1.5 rules 5 and 11: on a real device, for each type, the
/// door arms the raster brush, every rail key a lesson rings has a rect, and a
/// raster stroke and a fill land.
#[test]
#[ignore = "needs a GPU"]
fn auguide_real_route_with_gpu() {
    const FILL_KEY: &[Target] = &[Target::Rail(Key::Fill)];
    const ERASER_KEY: &[Target] = &[Target::Rail(Key::Eraser)];
    let table = lessons::table();
    let kinds = [ProjectKind::Drawing, ProjectKind::Animation, ProjectKind::Comic];
    for kind in kinds {
        let lesson = table.iter().find(|l| l.kind == kind).expect("a row of each type");
        let mut h = EditorHarness::with_gpu_ready();
        h.editor_mut().canvas.raster = false;
        open(&mut h, lesson.id);
        assert!(h.editor().canvas.raster, "{}: the door armed the raster brush", lesson.id);
        assert!(h.editor().paint.is_some(), "a real engine");
        let strokes = h.state().strokes_done;
        pen(&mut h, &along([200.0, 500.0], [700.0, 500.0]));
        assert_eq!(h.state().strokes_done, strokes + 1, "{}: a raster stroke", lesson.id);
        assert!(h.state().active_layer_tiles().is_some_and(|t| !t.is_empty()), "{}: on the layer", lesson.id);
        pick_fill(&mut h);
        let fills = h.state().fills_done;
        let spot = on_screen(&h, &[[450.0, 300.0, 1.0]])[0];
        h.click_pos(spot);
        settle(&mut h);
        assert_eq!(h.state().fills_done, fills + 1, "{}: a real fill", lesson.id);
        let rung = lessons::for_gate().iter().filter(|l| l.kind == kind).flat_map(|l| l.tasks).flat_map(|t| t.ring);
        let rung: Vec<(Target, Stage)> = rung.filter(|t| matches!(t, Target::Rail(_))).map(|t| (*t, Stage::Drawing)).collect();
        let here = h.editor().stage.expect("the lesson's step");
        let chains = [FILL_KEY, ERASER_KEY].into_iter().map(|c| (c, here));
        let own = rung.iter().map(|(t, s)| (&*Box::leak(vec![*t].into_boxed_slice()), *s));
        for (chain, step) in chains.chain(own).collect::<Vec<(&'static [Target], Stage)>>() {
            stand(&mut h, chain, step);
            assert!(h.ring_rect().is_some(), "{}: {chain:?} has a rect", lesson.id);
        }
        drop(h); // the device is one at a time: the next harness waits on this one
    }
    // The select tool's refusal stands past the engine's own gate, so it needs
    // one: with the raster route off, a lift says what the fill's tap says.
    let mut h = EditorHarness::with_gpu_ready();
    open(&mut h, "probe");
    let e = h.editor_mut();
    e.canvas.set_tool(CanvasTool::Select, &mut e.state);
    e.canvas.raster = false;
    e.canvas.selection = Some(vec![pos2(100.0, 100.0), pos2(900.0, 100.0), pos2(900.0, 800.0), pos2(100.0, 800.0)]);
    settle(&mut h);
    pen(&mut h, &along([300.0, 300.0], [500.0, 300.0]));
    assert!(h.state().refusal.contains(NEEDS_ENGINE), "{}", h.state().refusal);
}

/// Both refusals say one sentence, kept in one place, and the card answers it.
#[test]
fn auguide_refused_words_are_one_sentence() {
    assert!(BY_HAND.contains(SHOW_ME), "the card names its own button");
    let sources = ausay_tests::sources();
    let canvas = &sources.iter().find(|(name, _)| name == "canvas.rs").expect("the canvas").1;
    assert_eq!(canvas.matches(NEEDS_ENGINE).count(), 1, "the sentence stands once");
    assert_eq!(canvas.matches("{NEEDS_ENGINE}").count(), 2, "and both refusals say it");
}

/// The part's own list: the named tests stand, the real route stays out of the
/// plain run, and the part's floor of tests holds.
#[test]
fn auguide_named_tests_stand() {
    const NAMED: [&str; 13] = [
        "auguide_every_lesson_ends_in_its_file",
        "auguide_pen_ticks_every_stroke_task",
        "auguide_guide_never_exports",
        "auguide_guide_stays_out_of_render_and_save",
        "auguide_guide_follows_the_view",
        "auguide_show_me_is_one_undo",
        "auguide_pen_down_cancels_the_reveal",
        "auguide_commit_path_walks_segment_boxes",
        "auguide_commit_path_keeps_inside",
        "auguide_fill_at_matches_fill_input",
        "auguide_refused_fill_points_to_show_me",
        "auguide_deckle_keeps_its_length",
        "auguide_deckle_feet_stay_planted",
    ];
    const FLOOR: usize = 16;
    let src = FilePath::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let read = |name: &str| {
        let file = std::fs::read_to_string(src.join(name)).expect("the tests");
        file.lines().map(str::trim).collect::<Vec<_>>().join("\n")
    };
    let source = ["auguide_show_tests.rs", "auguide_tests.rs", "canvas.rs"].map(read).join("\n");
    let plain = |name: &str| source.contains(&format!("#[test]\nfn {name}()"));
    NAMED.iter().for_each(|name| assert!(plain(name), "{name} runs in the plain gate"));
    let real = "#[test]\n#[ignore = \"needs a GPU\"]\nfn auguide_real_route_with_gpu()";
    assert!(source.contains(real), "the real route is the ignored one");
    let tests = source.split("#[test]\n").skip(1).filter(|rest| {
        let line = rest.lines().find(|l| l.starts_with("fn ")).unwrap_or("");
        line.starts_with("fn auguide_")
    });
    assert!(tests.count() >= FLOOR, "the part owes {FLOOR} tests");
}
