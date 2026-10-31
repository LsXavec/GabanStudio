//! AU9: the Export step: a beginner ends in a real file (spec 5.5, 8.2, 8.3,
//! 9). The prefix is `auexport_`. No test here opens a dialog or a file: a
//! dialog's answer is `h.answer_export`, and `Open file` stays a logged request.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui::vec2;

use crate::canvas::CanvasTool;
use crate::canvas::extpaper_tests::paint_texels;
use crate::config::Action;
use crate::doc::{self, AppState};
use crate::export_step::{self, Offer};
use crate::measure::{self, Act, Recorder};
use crate::render_queue::{self, ExportKind, JobState};
use crate::say::{self, Verb};
use crate::testkit::{EditorHarness, PanelHarness};
use crate::welcome::{self, EXPORT_STEP, WelcomeAction};
use crate::workspace::Stage;
use crate::{AppRequest, Pane, Performed, arming_key, stage_key, templates, visible_panes};

const INK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const GIF: &str = "Animated GIF";
const VIDEO: &str = "Video (MP4)";
const SEQUENCE: &str = "Picture sequence (PNG)";
const STILL: &str = "This frame (PNG)";
const OPEN: &str = "Open file (say line)";
const BACK: &str = "Back to Draw";

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

fn sized(h: &mut EditorHarness, w: f32, tall: f32) {
    h.kit_mut().set_size(vec2(w, tall));
    h.kit_mut().set_pixels_per_point(1.0);
    settle(h);
}

/// `n` drawings, each with ink of its own, on paper small enough that a job
/// ends quickly.
fn ball(n: u32) -> AppState {
    let mut state = AppState::new_project("ball", 64, 36, 12, 72.0);
    state.set_cut_length(n);
    for frame in 0..n {
        state.goto(frame);
        state.new_drawing_at_frame();
        paint_texels(&mut state, &[(4 * frame as i32, 18), (4 * frame as i32 + 1, 18)], INK);
    }
    state.goto(0);
    state
}

/// A shipped install at 1600×900 on `state`, with no encoder on this PC.
fn shipped(state: AppState) -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(state);
    h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
    sized(&mut h, 1600.0, 900.0);
    h
}

fn enter(h: &mut EditorHarness) {
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(h);
    assert_eq!(h.editor().stage, Some(Stage::Export));
}

fn says(h: &EditorHarness, words: &str) -> bool {
    h.painted_text().iter().any(|w| w == words)
}

fn lane(h: &EditorHarness) -> Option<String> {
    h.editor().say.hint.as_ref().map(|s| s.words.clone())
}

/// The remark gone, as it is after its dwell.
fn quiet(h: &mut EditorHarness) {
    h.wait(4.2);
    settle(h);
}

/// What a row pressed asks of the App.
fn asked(h: &EditorHarness) -> Vec<AppRequest> {
    h.rig().requests.clone()
}

/// `action`, as a menu row or the palette runs it.
fn invoke(h: &mut EditorHarness, action: Action) -> Performed {
    let ctx = h.ctx();
    let done = h.editor_mut().invoke(action, &ctx);
    settle(h);
    done
}

/// The GIF job behind `action`, from the row to the file.
fn export_gif(h: &mut EditorHarness, action: Action, file: &Path) {
    assert!(matches!(invoke(h, action), Performed::App(AppRequest::ExportTo(ExportKind::Gif))));
    h.answer_export(file);
    h.run_export_to_end();
    settle(h);
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

/// The tool, the layer and the view, as a person left them.
fn look(h: &EditorHarness) -> String {
    let ed = h.editor();
    let (kept, view) = (ed.canvas.snapshot_view(&ed.state), &ed.state.view);
    let tool = (kept.tool, kept.composite_view, kept.onion, kept.onion_layer_only, kept.sel_shape, kept.fill_ref_cel);
    let layer = (&view.active_layer, view.active_role, view.active_column, view.line_guard);
    format!("{tool:?} {layer:?} {:?}", ed.canvas.view_xform())
}

/// Spec 5.5: video needs ffmpeg, which is not bundled. The row says why and
/// offers what works now, and neither way in reaches a dialog with no encoder.
#[test]
fn auexport_mp4_without_ffmpeg_says_why_and_offers_gif() {
    let mut h = shipped(ball(3));
    enter(&mut h);
    assert!(says(&h, export_step::NO_FFMPEG), "{:?}", h.painted_text());
    assert!(!h.has(VIDEO), "no row to press");
    assert_eq!(invoke(&mut h, Action::ExportMp4), Performed::Ignored, "the palette's way in");
    assert!(h.state().refusal.ends_with(export_step::NO_FFMPEG), "with the same reason");
    assert!(asked(&h).is_empty(), "{:?}", asked(&h));
    h.click(export_step::GIF_INSTEAD);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ExportTo(ExportKind::Gif)]);

    // An encoder on this PC: the plain row, and the reason gone.
    let mut h = shipped(ball(3));
    h.state_mut().render_queue.ffmpeg = Some((String::new(), Some(PathBuf::from("ffmpeg.exe"))));
    enter(&mut h);
    assert!(!says(&h, export_step::NO_FFMPEG));
    assert!(!h.has(export_step::GIF_INSTEAD));
    h.click(VIDEO);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ExportTo(ExportKind::Mp4)]);
}

/// Spec 5.3: entering swaps only the dock. Seeded away from everything Draw
/// arms, so this cannot pass by default, and read after the arming frames ran
/// and after the way back.
#[test]
fn auexport_step_changes_no_tool_layer_or_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(ball(3));
    let ed = h.editor_mut();
    ed.canvas.set_tool(CanvasTool::Select, &mut ed.state);
    ed.state.view.onion = !ed.state.view.onion;
    ed.state.view.line_guard = true;
    ed.state.view.active_layer = "color".into();
    h.perform(Action::ZoomIn);
    settle(&mut h);
    assert_eq!(h.editor().canvas.tool, CanvasTool::Select, "the seed holds");
    let before = look(&h);
    let dock = visible_panes(&h.editor().dock);

    enter(&mut h);
    assert_eq!(look(&h), before, "on the way in");
    export_gif(&mut h, Action::ExportGif, &dir.path().join("ball.gif"));
    assert_eq!(look(&h), before, "with a job run in it");

    h.click(BACK);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert_eq!(look(&h), before, "and on the way back");
    assert_eq!(visible_panes(&h.editor().dock), dock, "the dock it left");
}

/// Spec 5.3: the stroke guard refuses the step outright: no dock swaps under a
/// live pen, by the verb or by its key.
#[test]
fn auexport_step_is_refused_mid_stroke() {
    let mut h = shipped(ball(3));
    let start = h.editor().canvas.dbg_rect.center();
    let end = start + vec2(40.0, 10.0);
    let dock = visible_panes(&h.editor().dock);
    h.press_and_move(start, end, 4);
    assert!(h.editor().canvas.stroke_active(), "a live stroke");
    let seq = h.state().refusal_seq;
    assert_eq!(h.perform(Action::RoomExport), Performed::Ignored);
    assert_eq!(h.state().refusal, "refused — finish the stroke first");
    assert_eq!(h.state().refusal_seq, seq + 1);
    h.key(egui::Key::F9);
    assert_eq!(h.editor().stage, Some(Stage::Drawing), "the key is the same verb");
    // The pill and the room verbs' own road meets the same refusal.
    h.editor_mut().enter_room(Stage::Export);
    assert_eq!(h.state().refusal, doc::FINISH_STROKE);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert_eq!(visible_panes(&h.editor().dock), dock);
    assert!(h.editor().canvas.stroke_active(), "the stroke goes on");
    h.release(end);
    settle(&mut h);
    enter(&mut h);
}

/// Spec 5.3 and AU9's gate: `Stage::ALL` still lists five, and Export arms
/// nothing and writes nothing: the stored keys stay what they were.
#[test]
fn auexport_arming_key_skips_export() {
    assert_eq!(Stage::ALL.len(), 5);
    assert!(!Stage::ALL.contains(&Stage::Export), "off the stored list");
    assert!(Stage::ALL.iter().all(|s| stage_key(Some(*s)) != "export"));
    for seen in ["drawing", "shiage", ""] {
        assert_eq!(arming_key(Some(Stage::Export), seen), seen, "it keeps what was armed");
    }
    for stage in Stage::ALL {
        assert_eq!(arming_key(Some(*stage), "drawing"), stage_key(Some(*stage)), "the others read as they did");
    }
    assert_eq!(arming_key(None, "drawing"), stage_key(None));

    let mut h = shipped(ball(3));
    h.editor_mut().enter_room(Stage::InkPaint);
    settle(&mut h);
    let room = h.rig().stack_room_seen;
    let stored = |h: &EditorHarness| serde_json::to_string(&h.rig().config.layers).unwrap();
    let (armed, layers) = (h.state().view.active_layer.clone(), stored(&h));
    enter(&mut h);
    assert_eq!(h.rig().stack_room_seen, room, "the last room armed stands");
    assert_eq!(h.state().view.active_layer, armed);
    assert_eq!(stored(&h), layers);
    assert!(!serde_json::to_string(&h.rig().config).unwrap().contains("\"export\""), "no stored key");
}

/// Spec 5.5's table, read fresh: what shows follows what the document holds,
/// and each choice is the one its label says.
#[test]
fn auexport_choices_follow_the_document() {
    // An animation: four choices, GIF first and solid.
    let mut h = shipped(ball(3));
    enter(&mut h);
    assert_eq!(Offer::of(h.state()), Offer::Animation);
    assert!(says(&h, export_step::NO_FFMPEG), "video's row, with no encoder");
    for (label, kind) in [(GIF, ExportKind::Gif), (SEQUENCE, ExportKind::Png), (STILL, ExportKind::Still)] {
        h.click(label);
        settle(&mut h);
        assert_eq!(asked(&h), [AppRequest::ExportTo(kind)], "{label}");
        h.rig_mut().requests.clear();
    }
    assert!(!h.has(export_step::PNG_IMAGE) && !h.has(export_step::OTHER_FORMATS));
    assert!(h.rect(GIF).bottom() < h.rect(export_step::GIF_INSTEAD).top(), "GIF leads");
    assert!(h.rect(SEQUENCE).bottom() < h.rect(STILL).top(), "the table's order");
    let (top, more) = (h.rect(STILL).bottom(), h.rect(export_step::MORE_OPTIONS).top());
    assert!(top < more, "and the fold after them");

    // One frame: no range to choose, one PNG.
    let mut h = shipped(ball(1));
    enter(&mut h);
    assert_eq!(Offer::of(h.state()), Offer::OneFrame);
    for gone in [GIF, SEQUENCE, STILL, VIDEO] {
        assert!(!h.has(gone), "{gone}");
    }
    h.click(export_step::PNG_IMAGE);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ExportTo(ExportKind::Still)]);
    h.click(export_step::OTHER_FORMATS);
    settle(&mut h);
    assert_eq!(h.state().render_queue.panel.more, Some(true), "Other formats is the fold");
    assert!(h.has_text_containing(crate::vocab::EXPORT_QUEUE), "{}", h.describe());

    // A comic: its pages, from the existing route.
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    h.answer_start(WelcomeAction::Template(templates::COMIC));
    settle(&mut h);
    paint_texels(h.state_mut(), &[(10, 10), (11, 10)], INK);
    enter(&mut h);
    assert_eq!(Offer::of(h.state()), Offer::Comic);
    assert_eq!(Action::ExportPages.label(), "Export pages");
    assert!(h.has("Export pages") && h.has(export_step::OTHER_FORMATS));
    assert!(!h.has(GIF) && !h.has(export_step::PNG_IMAGE));
    h.rig_mut().requests.clear();
    h.click("Export pages");
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ExportPages]);

    // The document changing changes the offer, with no step taken.
    let mut h = shipped(ball(1));
    enter(&mut h);
    h.state_mut().set_cut_length(4);
    settle(&mut h);
    assert!(h.has(GIF) && !h.has(export_step::PNG_IMAGE), "{}", h.describe());
}

/// Spec 5.5: the file is written first, so the card can only tell the truth.
/// A job that wrote no file leaves no card.
#[test]
fn auexport_done_card_tells_the_truth() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(ball(12));
    enter(&mut h);
    let file = dir.path().join("ball.gif");
    export_gif(&mut h, Action::ExportGif, &file);
    let bytes = std::fs::metadata(&file).unwrap().len();
    let shown = h.state().render_queue.panel.done.clone().expect("a card");
    assert_eq!((shown.file.as_path(), shown.bytes), (file.as_path(), bytes), "the file itself");
    assert!(says(&h, "Exported ball.gif"), "{:?}", h.painted_text());
    let line = format!("12 drawings, 1.0 s, {}", render_queue::size_words(bytes));
    assert!(says(&h, &line), "{line} in {:?}", h.painted_text());
    assert!(says(&h, export_step::OPEN_FILE) && says(&h, export_step::SHOW_IN_FOLDER) && says(&h, BACK));
    assert_eq!(lane(&h), None, "the card reports, not the lane");
    assert!(!h.has(export_step::STOP_EXPORT), "nothing is running");
    assert!(says(&h, export_step::NOTHING_WAITING), "{:?}", h.painted_text());

    // Holds are counted as drawings, and time as frames: nothing is assumed.
    let mut h = shipped(ball(3));
    h.state_mut().set_cut_length(24);
    enter(&mut h);
    let held = dir.path().join("held.gif");
    export_gif(&mut h, Action::ExportGif, &held);
    let size = render_queue::size_words(std::fs::metadata(&held).unwrap().len());
    let line = format!("3 drawings, 2.0 s, {size}");
    assert!(says(&h, &line), "{line} in {:?}", h.painted_text());

    // A still has no duration to give.
    let mut h = shipped(ball(1));
    enter(&mut h);
    let still = dir.path().join("still.png");
    assert!(matches!(invoke(&mut h, Action::ExportStill), Performed::App(_)));
    h.answer_export(&still);
    h.run_export_to_end();
    settle(&mut h);
    let size = render_queue::size_words(std::fs::metadata(&still).unwrap().len());
    assert!(says(&h, "Exported still.png"), "{:?}", h.painted_text());
    assert!(says(&h, &format!("1 drawing, {size}")), "{:?}", h.painted_text());

    // A folder that cannot hold it: the job fails, and no card says otherwise.
    let mut h = shipped(ball(3));
    enter(&mut h);
    let wall = dir.path().join("wall");
    std::fs::write(&wall, b"a file, not a folder").unwrap();
    let nowhere = wall.join("ball.gif");
    export_gif(&mut h, Action::ExportGif, &nowhere);
    assert!(matches!(h.state().render_queue.queue.jobs[0].state, JobState::Failed(_)), "the job failed");
    assert_eq!(h.state().render_queue.panel.done, None);
    assert!(!says(&h, "Exported ball.gif"), "{:?}", h.painted_text());
}

/// Spec 8.2 and AU9's item 4: a `File ▸ Export ▸` row outside the step stays
/// where it is, and the lane reports when the file lands, each time. Nothing
/// is stored for it: this is news, not a lesson.
#[test]
fn auexport_file_menu_export_reports_in_the_say_line() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(ball(3));
    let dock = visible_panes(&h.editor().dock);
    assert!(!dock.contains(&Pane::RenderQueue));
    let first = dir.path().join("ball.gif");
    export_gif(&mut h, Action::ExportGif, &first);
    assert_eq!(h.editor().stage, Some(Stage::Drawing), "it stays where it is");
    assert_eq!(visible_panes(&h.editor().dock), dock, "and opens no pane");
    assert_eq!(h.state().render_queue.panel.done, None, "and no card");
    quiet(&mut h);
    assert_eq!(lane(&h).as_deref(), Some("Exported ball.gif."));
    assert!(says(&h, "Exported ball.gif."), "{:?}", h.painted_text());
    assert_eq!(h.editor().say.hint.as_ref().and_then(|s| s.verb), Some(Verb::OpenFile));
    assert!(h.has(OPEN), "{}", h.describe());
    h.wait(30.0);
    assert_eq!(lane(&h).as_deref(), Some("Exported ball.gif."), "no timer takes it");
    h.click(OPEN);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::OpenFile(first.clone())]);
    assert_ne!(lane(&h).as_deref(), Some("Exported ball.gif."), "the press answers it");
    assert!(!h.has(OPEN));
    assert_eq!(h.rig().config.ui.hints_seen & say::EXPORTED, 0, "never stored");

    // A second file says so too, and it goes when the work goes on.
    let second = dir.path().join("hop.gif");
    export_gif(&mut h, Action::ExportGif, &second);
    assert_eq!(lane(&h).as_deref(), Some("Exported hop.gif."));
    paint_texels(h.state_mut(), &[(30, 30)], INK);
    settle(&mut h);
    assert_ne!(lane(&h).as_deref(), Some("Exported hop.gif."), "a stroke overtakes it");
    let third = dir.path().join("skip.gif");
    export_gif(&mut h, Action::ExportGif, &third);
    assert_eq!(lane(&h).as_deref(), Some("Exported skip.gif."));
    assert_eq!(h.perform(Action::AddDrawingAfter), Performed::Done);
    settle(&mut h);
    assert_ne!(lane(&h).as_deref(), Some("Exported skip.gif."), "and so does a drawing");

    // A sequence's name is what landed, and Open file opens its first picture.
    let pictures = dir.path().join("frames");
    std::fs::create_dir(&pictures).unwrap();
    assert!(matches!(invoke(&mut h, Action::ExportPngSequence), Performed::App(_)));
    h.answer_export(&pictures);
    h.run_export_to_end();
    settle(&mut h);
    let count = h.state().frame_count();
    assert_eq!(file_names(&pictures).len(), count as usize);
    assert_eq!(lane(&h), Some(format!("Exported {count} pictures.")));

    // Hints off: no report and no button, and the machine's remark stands.
    let mut h = shipped(ball(3));
    assert_eq!(h.perform(Action::ToggleHints), Performed::Done);
    settle(&mut h);
    export_gif(&mut h, Action::ExportGif, &first);
    assert_eq!(lane(&h), None);
    assert!(!h.has(OPEN));
    assert!(h.state().status.starts_with("rendered"), "{}", h.state().status);
    assert!(h.editor().quick_start[EXPORT_STEP], "the tick is the file's, hints or no hints");
}

/// Spec 5.5 and 1.5 rule 10: both are `AppRequest`s, and the back button names
/// where the person came from.
#[test]
fn auexport_open_and_show_are_requests() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(ball(3));
    h.editor_mut().enter_room(Stage::Edit);
    settle(&mut h);
    let timing = visible_panes(&h.editor().dock);
    enter(&mut h);
    let file = dir.path().join("ball.gif");
    export_gif(&mut h, Action::ExportGif, &file);
    h.click(export_step::OPEN_FILE);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::OpenFile(file.clone())]);
    h.rig_mut().requests.clear();
    h.click(export_step::SHOW_IN_FOLDER);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ShowInFolder(file.clone())]);
    assert!(h.state().render_queue.panel.done.is_some(), "the card stays for the next press");
    h.click("Back to Timing");
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Edit));
    assert_eq!(visible_panes(&h.editor().dock), timing);
    assert_eq!(h.state().render_queue.panel.done, None, "the card stays behind");

    // Only the App's request handler reaches the system.
    let opens: usize = crate::ausay_tests::sources()
        .iter()
        .filter(|(name, _)| !name.ends_with("proc.rs"))
        .map(|(_, text)| text.matches("proc::open_file(").count() + text.matches("proc::show_in_folder(").count())
        .sum();
    assert_eq!(opens, 2, "one call each");
}

/// Spec 8.4: the empty state says so and points at the way to fix it, with no
/// choice that could only fail.
#[test]
fn auexport_no_art_says_so() {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    assert!(!h.state().has_art());
    enter(&mut h);
    assert_eq!(Offer::of(h.state()), Offer::NoArt);
    assert!(says(&h, export_step::NO_ART), "{:?}", h.painted_text());
    assert!(says(&h, export_step::NOTHING_WAITING), "{:?}", h.painted_text());
    for gone in [GIF, VIDEO, SEQUENCE, STILL, export_step::PNG_IMAGE, export_step::OTHER_FORMATS] {
        assert!(!h.has(gone), "{gone}");
    }
    h.click(say::GO_TO_DRAW);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
}

/// Ink a `Rig` stroke does not leave: its stroke takes the vector route, and
/// no render holds vectors. The pixels stand in for the pen: one action, the
/// stroke's, and every frame's ink in a place of its own.
fn pose_ink(h: &mut EditorHarness, pose: i32) {
    let at = (100 + 80 * pose, 300 - 20 * pose);
    let blot: Vec<(i32, i32)> = (0..24).flat_map(|x| (0..24).map(move |y| (at.0 + x, at.1 + y))).collect();
    paint_texels(h.state_mut(), &blot, INK);
    h.run();
}

/// What the lane puts in front of a person on a journey.
fn read_lane(h: &EditorHarness, read: &mut Vec<String>) {
    read.extend(h.editor().say.hint.as_ref().map(|s| s.words.clone()));
}

/// Spec 9's J1, on the path a first-time animator takes: 12 poses, a play and
/// three presses to a real GIF. The journeys' `New concept` column is by hand,
/// so it stands by the step that meets it.
#[test]
fn auexport_ball_journey() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ball.gif");
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1600.0, 900.0);
    assert!(h.editor().start.is_some(), "the Start float stands over the ready paper");
    let c = h.editor().canvas.dbg_rect.center();
    let path = [c, c + vec2(30.0, 0.0), c + vec2(60.0, 10.0)];
    let mut read: Vec<String> = Vec::new();
    let mut concepts = Vec::new();
    let mut r = Recorder::new(&mut h, "ball");
    read_lane(r.harness(), &mut read);
    for pose in 0..12 {
        if pose == 1 {
            concepts.push("a drawing is one picture of the animation");
        }
        if pose > 0 {
            r.act("add a drawing", Act::Click("Add drawing (band)"));
            read_lane(r.harness(), &mut read);
        }
        r.act("draw a pose", Act::Stroke(&path));
        pose_ink(r.harness(), pose);
        read_lane(r.harness(), &mut read);
    }
    concepts.push("playing shows the drawings in a row");
    r.act("play", Act::Click("play (Space)"));
    read_lane(r.harness(), &mut read);
    concepts.push("exporting makes a file to share");
    r.act("go to export", Act::Click("Export"));
    read_lane(r.harness(), &mut read);
    r.act("choose the GIF", Act::Click(GIF));
    r.act("name it", Act::Answer(Box::new(|h| h.answer_export(&file))));
    r.harness().run_export_to_end();

    let tally = r.tally();
    assert_eq!(tally.actions(), 27, "{tally:?}");
    assert!(tally.actions() <= 28);
    assert_eq!(tally.strokes, 12);
    let overhead = tally.actions() - tally.strokes - 11;
    assert!(overhead <= 5, "{overhead}: {tally:?}");
    assert!(concepts.len() <= 3, "{concepts:?}");
    read.extend(["Add drawing", "Export", GIF, "Save"].map(String::from));
    let jargon = measure::jargon_in(&read);
    assert!(jargon.len() <= 2, "{jargon:?} in {read:?}");

    // A real 12-frame GIF of 1.0 s at 12 fps, in a temp folder.
    let gif = crate::gif_decode::decode(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(gif.frames.len(), 12);
    assert_eq!(gif.duration_cs(), 100, "1.0 s");
    assert_ne!(gif.frames[0].rgb, gif.frames[11].rgb, "the ball moved");
    assert_eq!(file_names(dir.path()), ["ball.gif"]);
    let h = r.harness();
    assert_eq!(h.state().engine.project.fps, 12);
    assert_eq!(h.editor().quick_start, [true; 5], "and every dot is ticked");
    let bytes = std::fs::metadata(&file).unwrap().len();
    let line = format!("12 drawings, 1.0 s, {}", render_queue::size_words(bytes));
    assert!(says(h, &line), "{line} in {:?}", h.painted_text());
}

/// Spec 9's J2: one picture, one named PNG, four presses and nothing to read.
#[test]
fn auexport_still_journey() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("picture.png");
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1600.0, 900.0);
    let mut read: Vec<String> = Vec::new();
    let mut concepts = Vec::new();
    let mut r = Recorder::new(&mut h, "still");
    r.act("say what it is", Act::Click("A drawing"));
    assert_eq!(
        r.harness().rig().requests,
        [AppRequest::Start(WelcomeAction::Intent(templates::PICTURE))],
        "the card's own request"
    );
    r.harness().rig_mut().requests.clear();
    r.harness().answer_start(WelcomeAction::Intent(templates::PICTURE));
    settle(r.harness());
    read_lane(r.harness(), &mut read);
    let c = r.harness().editor().canvas.dbg_rect.center();
    let path = [c, c + vec2(30.0, 0.0), c + vec2(60.0, 10.0)];
    concepts.push("the paper is ready to draw on");
    r.act("paint", Act::Stroke(&path));
    pose_ink(r.harness(), 0);
    read_lane(r.harness(), &mut read);
    concepts.push("exporting makes a file to share");
    r.act("go to export", Act::Click("Export"));
    read_lane(r.harness(), &mut read);
    r.act("choose the PNG", Act::Click(export_step::PNG_IMAGE));
    r.act("name it", Act::Answer(Box::new(|h| h.answer_export(&file))));
    r.harness().run_export_to_end();

    let tally = r.tally();
    let overhead = tally.actions() - tally.strokes;
    assert!(overhead <= 4, "{overhead}: {tally:?}");
    assert!(concepts.len() <= 2, "{concepts:?}");
    read.extend(["A drawing", "Export", export_step::PNG_IMAGE, "Save"].map(String::from));
    let jargon = measure::jargon_in(&read);
    assert!(jargon.len() <= 1, "{jargon:?} in {read:?}");

    assert_eq!(file_names(dir.path()), ["picture.png"], "exactly one, by the name it was given");
    let png = std::fs::File::open(&file).unwrap();
    let reader = png::Decoder::new(std::io::BufReader::new(png)).read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (1920, 1080));
    assert!(r.harness().editor().quick_start[EXPORT_STEP]);
}

/// Spec 8.3: the tick is a finished file on disk, on either route. A dialog
/// open, a job running and a job that failed are not a file.
#[test]
fn auexport_quick_start_ends_in_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let tick = |h: &EditorHarness| h.editor().quick_start[EXPORT_STEP];

    // In the Export step.
    let mut h = shipped(ball(3));
    enter(&mut h);
    assert!(!tick(&h), "a step entered is no file");
    assert!(matches!(invoke(&mut h, Action::ExportGif), Performed::App(_)));
    assert!(!tick(&h), "a dialog is no file");
    let file = dir.path().join("in-the-step.gif");
    h.answer_export(&file);
    assert!(!tick(&h) || file.exists(), "a job running is no file");
    h.run_export_to_end();
    settle(&mut h);
    assert!(tick(&h) && file.exists());

    // From a menu row, standing elsewhere.
    let mut h = shipped(ball(3));
    export_gif(&mut h, Action::ExportGif, &dir.path().join("from-the-menu.gif"));
    assert!(tick(&h));

    // By the queue's old route, which opens the pane as it always did.
    let mut h = shipped(ball(3));
    let ctx = h.ctx();
    let old = dir.path().join("old-row");
    std::fs::create_dir(&old).unwrap();
    h.editor_mut().queue_export(&ctx, ExportKind::Png, &old);
    settle(&mut h);
    assert!(visible_panes(&h.editor().dock).contains(&Pane::RenderQueue));
    h.run_export_to_end();
    settle(&mut h);
    assert!(tick(&h) && file_names(&old).len() == 3);

    // A job that wrote nothing ticks nothing, and leaves Share's row standing.
    let mut h = shipped(ball(3));
    let wall = dir.path().join("wall");
    std::fs::write(&wall, b"a file, not a folder").unwrap();
    export_gif(&mut h, Action::ExportGif, &wall.join("ball.gif"));
    assert!(!tick(&h), "no file");
    assert_eq!(h.rig().config.ui.hints_seen & say::SHARE, 0);
    assert_ne!(lane(&h).as_deref(), Some("Exported ball.gif."));
}

/// Spec 8.3: every slot does its step. The fifth enters Export with its first
/// choice ringed, and leaves the tick to the person's file.
#[test]
fn auexport_show_me_is_five_of_five() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = EditorHarness::as_shipped();
    h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
    sized(&mut h, 1600.0, 900.0);
    for slot in 0..welcome::STEPS.len() {
        assert_eq!(h.editor_mut().show_me(slot), Performed::Done, "slot {slot}");
        settle(&mut h);
    }
    assert_eq!(h.editor().quick_start, [true, true, true, true, false], "no file yet");
    assert_eq!(h.editor().stage, Some(Stage::Export));
    let ring = h.state().render_queue.panel.ring_at.expect("a ring");
    assert!(ring.contains_rect(h.rect(GIF)) && ring != h.rect(GIF), "around Animated GIF: {ring:?}");
    h.wait(30.0);
    assert!(h.state().render_queue.panel.ring_at.is_some(), "no timer takes it");

    h.click(GIF);
    settle(&mut h);
    assert_eq!(h.state().render_queue.panel.ring_at, None, "the press answers it");
    h.answer_export(&dir.path().join("ball.gif"));
    h.run_export_to_end();
    settle(&mut h);
    assert_eq!(h.editor().quick_start, [true; 5], "five of five");

    // Another step takes the ring with it, and its card.
    assert_eq!(h.editor_mut().show_me(EXPORT_STEP), Performed::Done);
    settle(&mut h);
    assert!(h.state().render_queue.panel.ring_at.is_some());
    h.click("Draw");
    settle(&mut h);
    let panel = &h.state().render_queue.panel;
    assert!(!panel.ring && panel.done.is_none());

    // One frame: the ring goes on what leads there.
    let mut h = shipped(ball(1));
    assert_eq!(h.editor_mut().show_me(EXPORT_STEP), Performed::Done);
    settle(&mut h);
    let ring = h.state().render_queue.panel.ring_at.expect("a ring");
    assert!(ring.contains_rect(h.rect(export_step::PNG_IMAGE)));
}

/// Spec 5.5: one layout, with today's whole panel under `More options`: closed
/// on a shipped boot, open on the frozen one (spec 1.5.2).
#[test]
fn auexport_more_options_holds_the_queue_panel() {
    let mut h = shipped(ball(3));
    enter(&mut h);
    assert!(!h.has_text_containing(crate::vocab::EXPORT_QUEUE), "{}", h.describe());
    assert!(!h.has(export_step::TRANSPARENT));
    h.click(export_step::MORE_OPTIONS);
    settle(&mut h);
    assert!(h.has_text_containing(crate::vocab::EXPORT_QUEUE), "{}", h.describe());
    assert!(h.has(export_step::TRANSPARENT));
    h.click(export_step::MORE_OPTIONS);
    settle(&mut h);
    assert!(!h.has_text_containing(crate::vocab::EXPORT_QUEUE), "{}", h.describe());

    let mut h = EditorHarness::with_state(ball(3));
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h);
    assert!(h.has_text_containing(crate::vocab::EXPORT_QUEUE), "{}", h.describe());
}

/// Spec 6.2: `File ▸ Export ▸` leads with the plain rows and Other formats,
/// pressed for real: a row asks for its one dialog where the person stands,
/// and Other formats opens the Export step.
#[test]
fn auexport_file_menu_leads_with_the_plain_rows() {
    fn export_menu(h: &mut EditorHarness) {
        h.click("file");
        settle(h);
        h.click("Export… ⏵");
        settle(h);
    }
    const OTHER: &str = "Other formats…";
    let mut h = shipped(ball(3));
    assert_eq!(h.perform(Action::RoomDrawing), Performed::Done);
    settle(&mut h);
    export_menu(&mut h);
    let tops = [GIF, VIDEO, SEQUENCE, STILL, OTHER].map(|row| h.rect(row).top());
    assert!(tops.is_sorted(), "{tops:?}");

    h.click(GIF);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ExportTo(ExportKind::Gif)]);
    assert_eq!(h.editor().stage, Some(Stage::Drawing), "it stays where it is");
    assert!(!visible_panes(&h.editor().dock).contains(&Pane::RenderQueue), "and opens no pane");

    export_menu(&mut h);
    h.click(OTHER);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Export));
    assert_eq!(asked(&h).len(), 1, "no dialog of its own");
}

/// The queue's panel follows the Export panel in one Ui: it keeps its own
/// rows' rhythm, and stands on the cards' left edge.
#[test]
fn auexport_queue_keeps_its_rhythm_under_the_panel() {
    let p = PanelHarness::new(vec2(340.0, 600.0), ball(3), |ui, state| {
        let rhythm = ui.spacing().item_spacing;
        export_step::ui(ui, state, false);
        assert_eq!(ui.spacing().item_spacing, rhythm, "the panel's gaps are its own");
    });
    assert!(p.has(GIF), "the panel drew");

    let mut h = EditorHarness::with_state(ball(3));
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h);
    assert_eq!(h.rect("preset").left(), h.rect(GIF).left(), "one edge down the pane");
}
