//! AU9: the Export step: a beginner ends in a real file (spec 5.5, 8.2, 8.3,
//! 9). The prefix is `auexport_`. No test here opens a dialog or a file: a
//! dialog's answer is `h.answer_export`, and `Open file` stays a logged request.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui::vec2;

use crate::canvas::CanvasTool;
use crate::canvas::extpaper_tests::paint_texels;
use crate::config::{Action, Config};
use crate::doc::{self, AppState};
use crate::export_step::{self, Ask, Offer};
use crate::manga_tools::PageExport;
use crate::measure::{self, Act, Recorder};
use crate::render_queue::{self, ExportKind, JobState, Landed, Progress};
use crate::say::{self, Verb};
use crate::testkit::{EditorHarness, PanelHarness};
use crate::welcome::{self, EXPORT_STEP, WelcomeAction};
use crate::workspace::Stage;
use crate::{AppRequest, Pane, Performed, arming_key, plate, stage_key, templates, visible_panes, vocab};

const INK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const GIF: &str = "Animated GIF";
const VIDEO: &str = "Video (MP4)";
const SEQUENCE: &str = "Picture sequence (PNG)";
const STILL: &str = "This frame (PNG)";
const OPEN: &str = "Open file (say line)";
const BACK: &str = "Back to Plan";

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
pub(crate) fn ball(n: u32) -> AppState {
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
pub(crate) fn shipped(state: AppState) -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(state);
    h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
    sized(&mut h, 1600.0, 900.0);
    h
}

pub(crate) fn enter(h: &mut EditorHarness) {
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(h);
    assert_eq!(h.editor().stage, Some(Stage::Export));
}

fn says(h: &EditorHarness, words: &str) -> bool {
    h.painted_text().iter().any(|w| w == words)
}

pub(crate) fn lane(h: &EditorHarness) -> Option<String> {
    h.editor().say.hint.as_ref().map(|s| s.words.clone())
}

/// The remark gone, as it is after its dwell.
pub(crate) fn quiet(h: &mut EditorHarness) {
    h.wait(4.2);
    settle(h);
}

/// What a row pressed asks of the App.
fn asked(h: &EditorHarness) -> Vec<AppRequest> {
    h.rig().requests.clone()
}

/// `action`, as a menu row or the palette runs it.
pub(crate) fn invoke(h: &mut EditorHarness, action: Action) -> Performed {
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

/// The panel, as it reads the Editor from the Export step left from Draw.
fn view() -> export_step::View {
    export_step::View {
        back: Some(Stage::Drawing.name().to_string()),
        go_first: say::GO_TO_DRAW,
        go_card: say::Verb::GoToDraw.card(None),
        more_default: false,
        range: None,
    }
}

/// A shipped comic with ink on its page, from the template a person picks.
fn comic() -> EditorHarness {
    let mut h = EditorHarness::as_shipped();
    h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
    sized(&mut h, 1600.0, 900.0);
    h.answer_start(WelcomeAction::Template(templates::COMIC));
    settle(&mut h);
    paint_texels(h.state_mut(), &[(10, 10), (11, 10)], INK);
    settle(&mut h);
    h
}

/// The book's pages into `dir`: past the folder dialog, on their worker, to
/// the end, as the App's request runs them.
fn export_pages(h: &mut EditorHarness, dir: &Path) {
    h.state_mut().begin_page_export(dir, None);
    h.state_mut().wait_page_export();
    settle(h);
}

/// The pane's whole rect, as the dock gave it this frame.
fn pane_rect(h: &EditorHarness) -> egui::Rect {
    h.editor().pane_rects.iter().find(|(pane, ..)| *pane == Pane::RenderQueue).expect("the pane is forward").1
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
    // The sentence stands on the cards' text edge, and its action reads as one.
    let edge = h.rect(GIF).left() + plate::CARD_PAD;
    assert_eq!(h.rect(export_step::NO_FFMPEG).left(), edge, "one text edge");
    assert!(h.filled_rects(plate::WELL).contains(&h.rect(export_step::GIF_INSTEAD)), "a ground at rest");
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
    assert!(!h.has(export_step::OTHER_FORMATS), "the fold is open: no second way to it");
    h.click(export_step::MORE_OPTIONS);
    settle(&mut h);
    assert!(h.has(export_step::OTHER_FORMATS), "and back, once it is closed");

    // A comic: its pages, from the existing route.
    let mut h = comic();
    enter(&mut h);
    assert_eq!(Offer::of(h.state()), Offer::Comic);
    assert_eq!(Action::ExportPages.label(), "Export pages");
    assert!(h.has("Export pages") && h.has(export_step::OTHER_FORMATS));
    assert!(!h.has(GIF) && !h.has(export_step::PNG_IMAGE));
    h.rig_mut().requests.clear();
    h.click("Export pages");
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ExportPages]);
    h.click(export_step::OTHER_FORMATS);
    settle(&mut h);
    assert!(!h.has(export_step::OTHER_FORMATS), "a comic's fold is the same fold");

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
    assert_eq!(h.state().status, "", "and the machine's remark gave way");
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

    // The failure stays in the list, marked, past any refusal's dwell, with the
    // same choice one press away, until the next export starts.
    let why = h.state().render_queue.panel.failed.clone().expect("it stands").why;
    let words = format!("That export did not finish: {}.", why.trim_end_matches('.'));
    assert!(says(&h, &words), "{words} in {:?}", h.painted_text());
    assert!(!h.filled_rects(plate::RED_WASH).is_empty(), "on the wash");
    assert!(!says(&h, export_step::NOTHING_WAITING), "the list is not empty");
    let refusal = h.state().refusal.clone();
    assert!(says(&h, &refusal), "the lane's four seconds: {:?}", h.painted_text());
    quiet(&mut h);
    assert!(says(&h, &words), "no timer takes it");
    assert!(!says(&h, &refusal), "the lane's has gone, and this has not: {:?}", h.painted_text());
    h.click(export_step::TRY_AGAIN);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ExportTo(ExportKind::Gif)], "the same dialog");
    assert!(says(&h, &words), "a dialog is not an export");
    h.answer_export(&dir.path().join("ball.gif"));
    h.run_export_to_end();
    settle(&mut h);
    assert!(!says(&h, &words), "{:?}", h.painted_text());
    assert_eq!(h.state().render_queue.panel.failed, None);
    assert!(says(&h, "Exported ball.gif"), "{:?}", h.painted_text());

    // A stopped job is no failure: the person asked for that.
    let mut h = shipped(ball(3));
    enter(&mut h);
    let tx = render_queue::held_job(h.state_mut(), &dir.path().join("stopped.gif"), 1);
    settle(&mut h);
    tx.send(Progress::Done(Err(render_queue::CANCELLED.into()), None)).unwrap();
    settle(&mut h);
    assert_eq!(h.state().render_queue.panel.failed, None);
    assert!(says(&h, export_step::NOTHING_WAITING), "{:?}", h.painted_text());

    // A comic's pages end on the same card, made from the files on disk: their
    // count for a name, and the first of them for both presses.
    let mut h = comic();
    enter(&mut h);
    let pages = dir.path().join("pages");
    std::fs::create_dir(&pages).unwrap();
    export_pages(&mut h, &pages);
    let written = file_names(&pages);
    assert_eq!(written.len(), 1, "{written:?}");
    let first = pages.join(&written[0]);
    let bytes = std::fs::metadata(&first).unwrap().len();
    let card = h.state().render_queue.panel.done.clone().expect("a card");
    assert_eq!((card.file.as_path(), card.bytes), (first.as_path(), bytes), "the file itself");
    assert!(says(&h, "Exported 1 page"), "{:?}", h.painted_text());
    assert!(says(&h, &render_queue::size_words(bytes)), "{:?}", h.painted_text());
    assert_eq!(h.state().status, "", "the card reports");
    assert_eq!(lane(&h), None);
    h.rig_mut().requests.clear();
    h.click(export_step::OPEN_FILE);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::OpenFile(first.clone())]);
    h.rig_mut().requests.clear();
    h.click(export_step::SHOW_IN_FOLDER);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ShowInFolder(first.clone())], "its folder, the page picked out");

    // Pages that could not be written fail the same way, and try the same way.
    let mut h = comic();
    enter(&mut h);
    export_pages(&mut h, &wall);
    let failed = h.state().render_queue.panel.failed.clone().expect("it stands");
    assert_eq!(failed.again, Some(Ask::Do(Action::ExportPages)));
    assert_eq!(h.state().render_queue.panel.done, None);
    let words = format!("That export did not finish: {}.", failed.why.trim_end_matches('.'));
    assert!(says(&h, &words), "{words} in {:?}", h.painted_text());
    h.rig_mut().requests.clear();
    h.click(export_step::TRY_AGAIN);
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ExportPages]);
    export_pages(&mut h, &pages);
    assert_eq!(h.state().render_queue.panel.failed, None, "gone as the next export starts");

    // While pages are being written the list says so: they are no queue job.
    let mut h = comic();
    enter(&mut h);
    assert!(says(&h, export_step::NOTHING_WAITING), "{:?}", h.painted_text());
    let (_held, rx) = std::sync::mpsc::channel();
    h.state_mut().manga.export_job = Some(PageExport { dir: pages.clone(), rx });
    settle(&mut h);
    assert!(says(&h, export_step::WRITING_PAGES), "{:?}", h.painted_text());
    assert!(!says(&h, export_step::NOTHING_WAITING), "{:?}", h.painted_text());
}

/// NEVER-DO 6: a sequence's stats would hold the frame. The worker reads the
/// files and its last message carries them, so `poll` reads no disk, a file
/// gone by then is still reported, and no frame's `poll` splits the report
/// from the end of the job.
#[test]
fn auexport_poll_reads_a_message_not_the_disk() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(ball(3));
    let gone = dir.path().join("never-written.gif");
    let tx = render_queue::held_job(h.state_mut(), &gone, 2);
    let name = "never-written.gif".to_string();
    let landed = Landed { file: gone, name, drawings: 3, frames: 3, fps: 12, bytes: 2048, plain: true };
    let ctx = h.ctx();
    tx.send(Progress::Frame).unwrap();
    render_queue::poll(h.state_mut(), &ctx);
    assert!(h.state().render_queue.busy(), "a frame is not the end");
    tx.send(Progress::Done(Ok(3), Some(landed.clone()))).unwrap();
    render_queue::poll(h.state_mut(), &ctx);
    assert!(!h.state().render_queue.busy());
    assert_eq!(h.state().render_queue.landed, [landed]);
}

/// A book's report is its files on disk, however many: their count for a name
/// and their sizes summed. A page that is missing leaves no report at all.
#[test]
fn auexport_a_book_reports_every_page() {
    let dir = tempfile::tempdir().unwrap();
    let pages: Vec<_> = [10, 20, 30].iter().enumerate().map(|(i, bytes)| {
        let page = dir.path().join(format!("P0{}.png", i + 1));
        std::fs::write(&page, vec![0u8; *bytes]).unwrap();
        page
    }).collect();
    let book = Landed::pages(&pages).expect("all three are there");
    assert_eq!(book.name, "3 pages");
    assert_eq!(book.bytes, 60);
    assert_eq!(book.file, pages[0], "the first page is what `Open file` opens");
    assert_eq!(book.line(), render_queue::size_words(60), "no drawings and no running time to give");
    assert_eq!(Landed::pages(&[]), None, "no page, no report");
    std::fs::remove_file(&pages[1]).unwrap();
    assert_eq!(Landed::pages(&pages), None, "a file that is not there cannot be reported");
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
    assert!(matches!(invoke(&mut h, Action::ExportGif), Performed::App(_)));
    h.answer_export(&first);
    assert_eq!(h.state().status, "Exporting ball.gif…", "plain words while it runs");
    h.run_frames(1);
    assert!(says(&h, "Exporting ball.gif…"), "in the lane: {:?}", h.painted_text());
    h.run_export_to_end();
    settle(&mut h);
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
    let count = h.state().frame_count();
    assert_eq!(h.state().status, format!("Exporting {count} pictures…"), "a folder is no name to say");
    h.run_export_to_end();
    settle(&mut h);
    assert_eq!(file_names(&pictures).len(), count as usize);
    assert_eq!(lane(&h), Some(format!("Exported {count} pictures.")));

    // A comic's pages, standing in another step, end in the same lane.
    let mut h = comic();
    let pages = dir.path().join("pages");
    std::fs::create_dir(&pages).unwrap();
    export_pages(&mut h, &pages);
    assert_eq!(lane(&h).as_deref(), Some("Exported 1 page."));
    assert!(h.has(OPEN), "{}", h.describe());

    // The step's dock lost the pane: no card could stand, so the lane reports.
    let mut h = shipped(ball(3));
    enter(&mut h);
    h.editor_mut().dock = Stage::Drawing.dock();
    settle(&mut h);
    export_gif(&mut h, Action::ExportGif, &dir.path().join("no-pane.gif"));
    assert_eq!(h.state().render_queue.panel.done, None);
    assert_eq!(lane(&h).as_deref(), Some("Exported no-pane.gif."));

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
    assert!(!says(&h, export_step::NOTHING_WAITING), "one empty sentence is enough: {:?}", h.painted_text());
    for gone in [GIF, VIDEO, SEQUENCE, STILL, export_step::PNG_IMAGE, export_step::OTHER_FORMATS] {
        assert!(!h.has(gone), "{gone}");
    }
    assert!(h.filled_rects(plate::BLUE).contains(&h.rect(say::GO_TO_DRAW)), "the step's one way on");
    h.click(say::GO_TO_DRAW);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
}

/// Spec 8.4: what the Result shows counts, not only what a drawing holds: an
/// empty state beside a real picture would be a lie. A page's paper is no art,
/// and nor is a source the Result never reads.
#[test]
fn auexport_art_no_drawing_holds_is_art() {
    use anim_core::graph::NodeKind;
    let mut state = AppState::new_project("plate", 64, 36, 12, 72.0);
    assert!(state.auto_comp());
    state.set_cut_length(3);
    assert_eq!(Offer::of(&state), Offer::NoArt);
    let output = state.cut().graph.nodes().find(|n| n.kind == NodeKind::Output).expect("the composite").id;

    state.graph_add_node(NodeKind::Solid { rgba: [255; 4] }, (0.0, 0.0));
    let paper = state.selected_node.expect("the new node");
    state.graph_connect(paper, output, 0);
    assert_eq!(Offer::of(&state), Offer::NoArt, "paper");

    state.graph_add_node(NodeKind::image_sequence("renders/shot_####.png"), (0.0, 90.0));
    let frames = state.selected_node.expect("the new node");
    assert_eq!(Offer::of(&state), Offer::NoArt, "reaching nothing, it shows nothing");
    state.graph_connect(frames, output, 0);
    assert_eq!(Offer::of(&state), Offer::Animation, "the Result shows it");
    assert!(!state.has_art(), "and the Say table's reading stands");
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

/// What the lane puts in front of a person on a journey: the hint, and the
/// machine's remark that stands over it for its dwell. In the Export step, the
/// words the panel paints as well: the journey's whole point is that screen.
fn read_lane(h: &EditorHarness, read: &mut Vec<String>) {
    read.extend(h.editor().say.hint.as_ref().map(|s| s.words.clone()));
    read.extend(Some(h.state().status.clone()).filter(|remark| !remark.is_empty()));
    if h.editor().stage == Some(Stage::Export) {
        read.extend(h.painted_text());
    }
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
    let mut read: Vec<String> = Vec::new();
    let mut concepts = Vec::new();
    let mut r = Recorder::new(&mut h, "ball");
    r.act("say what it is", Act::Click("An animation"));
    assert_eq!(
        r.harness().rig().requests,
        [AppRequest::Start(WelcomeAction::Intent(templates::ANIMATION))],
        "the card's own request"
    );
    r.harness().rig_mut().requests.clear();
    r.harness().answer_start(WelcomeAction::Intent(templates::ANIMATION));
    settle(r.harness());
    let c = r.harness().editor().canvas.dbg_rect.center();
    let path = [c, c + vec2(30.0, 0.0), c + vec2(60.0, 10.0)];
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
    read_lane(r.harness(), &mut read);
    r.act("name it", Act::Answer(Box::new(|h| h.answer_export(&file))));
    read_lane(r.harness(), &mut read);
    r.harness().run_export_to_end();
    settle(r.harness());
    read_lane(r.harness(), &mut read);

    let tally = r.tally();
    assert_eq!(tally.actions(), 28, "{tally:?}");
    assert!(tally.actions() <= 28);
    assert_eq!(tally.strokes, 12);
    let overhead = tally.actions() - tally.strokes - 11;
    assert!(overhead <= 5, "{overhead}: {tally:?}");
    assert!(concepts.len() <= 3, "{concepts:?}");
    read.extend(["Add drawing", "Export", GIF, "Save"].map(String::from));
    let jargon = measure::jargon_in(&read);
    println!("J1: {} actions, {overhead} overhead, {} concepts, {} jargon: {tally:?}", tally.actions(), concepts.len(), jargon.len());
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
    read_lane(r.harness(), &mut read);
    r.act("name it", Act::Answer(Box::new(|h| h.answer_export(&file))));
    read_lane(r.harness(), &mut read);
    r.harness().run_export_to_end();
    settle(r.harness());
    read_lane(r.harness(), &mut read);

    let tally = r.tally();
    let overhead = tally.actions() - tally.strokes;
    assert!(overhead <= 4, "{overhead}: {tally:?}");
    assert!(concepts.len() <= 2, "{concepts:?}");
    read.extend(["A drawing", "Export", export_step::PNG_IMAGE, "Save"].map(String::from));
    let jargon = measure::jargon_in(&read);
    println!("J2: {} actions, {overhead} overhead, {} concepts, {} jargon: {tally:?}", tally.actions(), concepts.len(), jargon.len());
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

    // With the queue standing open: the job is the queue's as well.
    let mut h = shipped(ball(3));
    assert_eq!(h.perform(Action::ShowRenderQueue), Performed::Done);
    settle(&mut h);
    let ctx = h.ctx();
    let old = dir.path().join("old-row");
    std::fs::create_dir(&old).unwrap();
    h.editor_mut().export_to(&ctx, ExportKind::Png, &old);
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

    // A comic's primary ends the same way: the tick is its pages on disk.
    let mut h = comic();
    enter(&mut h);
    assert!(!tick(&h), "a step entered is no file");
    h.click("Export pages");
    settle(&mut h);
    assert!(!tick(&h), "a dialog is no file");
    export_pages(&mut h, dir.path());
    assert!(tick(&h), "{:?}", h.painted_text());

    // And pages that were never written tick nothing.
    let mut h = comic();
    enter(&mut h);
    export_pages(&mut h, &wall);
    assert!(h.state().render_queue.panel.failed.is_some(), "it failed");
    assert!(!tick(&h), "no file");
}

/// Spec 8.3: every slot does its step. The fifth enters Export with its first
/// choice ringed, and leaves the tick to the person's file.
#[test]
fn auexport_show_me_is_five_of_five() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = EditorHarness::as_animation();
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
    h.click(vocab::STEP_PLAN);
    settle(&mut h);
    let panel = &h.state().render_queue.panel;
    assert!(!panel.ring && panel.done.is_none());

    // One frame: the ring goes on what leads there.
    let mut h = shipped(ball(1));
    assert_eq!(h.editor_mut().show_me(EXPORT_STEP), Performed::Done);
    settle(&mut h);
    let ring = h.state().render_queue.panel.ring_at.expect("a ring");
    assert!(ring.contains_rect(h.rect(export_step::PNG_IMAGE)));

    // Whatever is pressed in the panel answers it: the fold, its latch, Stop.
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(ball(3));
    let ring = |h: &mut EditorHarness| {
        h.state_mut().render_queue.panel.ring = true;
        settle(h);
        assert!(h.state().render_queue.panel.ring_at.is_some(), "a ring");
    };
    enter(&mut h);
    ring(&mut h);
    h.click(export_step::MORE_OPTIONS);
    settle(&mut h);
    assert_eq!(h.state().render_queue.panel.ring_at, None, "the fold");
    ring(&mut h);
    h.click(export_step::TRANSPARENT);
    settle(&mut h);
    assert_eq!(h.state().render_queue.panel.ring_at, None, "the latch");
    let _held = render_queue::held_job(h.state_mut(), &dir.path().join("ball.gif"), 1);
    ring(&mut h);
    h.click(export_step::STOP_EXPORT);
    settle(&mut h);
    assert_eq!(h.state().render_queue.panel.ring_at, None, "Stop export");

    // On a comic it rings the same choice, and that choice ends the same way.
    let mut h = comic();
    assert_eq!(h.editor_mut().show_me(EXPORT_STEP), Performed::Done);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Export));
    let ring = h.state().render_queue.panel.ring_at.expect("a ring");
    assert!(ring.contains_rect(h.rect("Export pages")), "{ring:?}");
    assert!(!h.editor().quick_start[EXPORT_STEP], "no file yet");
    h.rig_mut().requests.clear();
    h.click("Export pages");
    settle(&mut h);
    assert_eq!(asked(&h), [AppRequest::ExportPages]);
    assert_eq!(h.state().render_queue.panel.ring_at, None, "the press answers it");
    export_pages(&mut h, dir.path());
    assert!(h.editor().quick_start[EXPORT_STEP], "the tick is its pages on disk");
    assert!(says(&h, "Exported 1 page"), "{:?}", h.painted_text());
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
        h.open_path("File/Export");
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
    assert!(h.has_text_containing(vocab::EXPORT_QUEUE), "the other formats are what it says: {}", h.describe());
}

/// AU9's Must not change: the routes into the queue from the old menu rows.
/// They ask for the queue and they land on it, on the boot people run, not
/// only on the frozen one with the fold open.
#[test]
fn auexport_old_rows_land_on_the_queue_itself() {
    fn on_the_queue(h: &EditorHarness, how: &str) {
        assert!(visible_panes(&h.editor().dock).contains(&Pane::RenderQueue), "{how}");
        assert!(h.has_text_containing(vocab::EXPORT_QUEUE), "{how}: {}", h.describe());
        assert!(h.has("preset"), "{how}: every preset is there: {}", h.describe());
    }
    // The Window row, the chord and the palette: one verb.
    let mut h = shipped(ball(3));
    assert!(!h.has_text_containing(vocab::EXPORT_QUEUE));
    assert_eq!(h.perform(Action::ShowRenderQueue), Performed::Done);
    settle(&mut h);
    on_the_queue(&h, "the verb");

    // The menu's own row.
    let mut h = shipped(ball(3));
    assert_eq!(h.perform(Action::RoomDrawing), Performed::Done);
    settle(&mut h);
    h.open_path("More/Panels");
    h.click("Export queue");
    settle(&mut h);
    on_the_queue(&h, "the menu's row");

    // A job from `File ▸ Export ▸` runs with the queue open, its job on view.
    let dir = tempfile::tempdir().unwrap();
    let ctx = h.ctx();
    h.editor_mut().export_to(&ctx, ExportKind::Png, dir.path());
    settle(&mut h);
    on_the_queue(&h, "the export");
    h.run_export_to_end();
    settle(&mut h);
    assert_eq!(h.state().render_queue.queue.jobs[0].asked, Some(ExportKind::Png));
}

/// Spec 5.5's pinned list and AU15's smallest pane: the jobs, Stop and the fold
/// stand at the pane's foot, outside its scrolling, however many choices it
/// has and whichever step holds it.
#[test]
fn auexport_the_job_list_holds_at_the_foot() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ball.gif");
    let mut h = EditorHarness::with_state_shipped(ball(3));
    h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
    sized(&mut h, 1280.0, 880.0);
    h.editor_mut().enter_room(Stage::Composite);
    // AU15: the lower leaf a person's Effects step no longer has, any drag
    // can still make one, at its 220 of height: under that the pane pans.
    h.editor_mut().dock = Stage::Composite.dock();
    settle(&mut h);
    assert_eq!(h.perform(Action::ShowRenderQueue), Performed::Done);
    settle(&mut h);
    h.click(export_step::MORE_OPTIONS);
    settle(&mut h);
    let _held = render_queue::held_job(h.state_mut(), &file, 1);
    settle(&mut h);
    let pane = pane_rect(&h);
    assert!(pane.height() < 4.0 * export_step::CHOICE_H + 80.0, "a short pane, or this proves nothing: {pane:?}");
    for row in ["ball.gif", "1 of 3", export_step::STOP_EXPORT, export_step::MORE_OPTIONS] {
        let at = h.rect(row);
        assert!(pane.contains_rect(at), "{row}: {at:?} outside {pane:?}");
    }
    assert!(h.rect("ball.gif").bottom() < h.rect(export_step::MORE_OPTIONS).top());
    assert!(h.rect(export_step::MORE_OPTIONS).bottom() > pane.bottom() - 40.0, "at the foot");

    // The choices scroll above the foot, and scroll away under nothing.
    h.scroll_to(STILL);
    settle(&mut h);
    let foot = h.rect("ball.gif").top();
    assert!(h.rect(STILL).bottom() <= foot, "{:?} over {foot}", h.rect(STILL));
    for row in ["ball.gif", export_step::STOP_EXPORT, export_step::MORE_OPTIONS] {
        assert!(pane.contains_rect(h.rect(row)), "{row} stays when the choices scroll");
    }

    // Pressed in a short pane with the choices at their top, the fold brings
    // what it opened into view: it opens under the choices, past the foot.
    h.scroll_to(GIF);
    settle(&mut h);
    h.click(export_step::MORE_OPTIONS);
    settle(&mut h);
    let opened = h.rect(export_step::TRANSPARENT);
    assert!(opened.top() >= pane.top() && opened.bottom() <= foot, "{opened:?} over {foot} in {pane:?}");

    // A failure's words and its one press hold there too, at the smallest
    // pane, and a list longer than its share scrolls in it.
    let mut state = ball(3);
    let _running = render_queue::held_job(&mut state, &file, 1);
    let long = "the disk is full, and the folder it was writing into has gone";
    state.render_queue.panel.failed = Some(export_step::Failed { why: long.into(), again: Some(Ask::Do(Action::ExportGif)) });
    let mut p = PanelHarness::new(vec2(280.0, 220.0), state, |ui, state| {
        render_queue::ui(ui, state, &view());
    });
    p.frames(3);
    let whole = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(280.0, 220.0));
    for row in ["ball.gif", export_step::STOP_EXPORT, export_step::MORE_OPTIONS] {
        assert!(whole.contains_rect(p.rect(row)), "{row}: {:?}\n{}", p.rect(row), p.describe());
    }
    assert!(p.rect(export_step::STOP_EXPORT).bottom() <= p.rect(export_step::MORE_OPTIONS).top());
    p.scroll_to(export_step::TRY_AGAIN);
    p.frames(3);
    let again = p.rect(export_step::TRY_AGAIN);
    assert!(again.bottom() <= p.rect(export_step::STOP_EXPORT).top() + 1.0, "{again:?}\n{}", p.describe());
    assert!(whole.contains_rect(p.rect(export_step::MORE_OPTIONS)), "the list scrolled, not the foot");
}

/// Spec 2.8: at most one primary and two solid blue things. Outside the step
/// the next step's pill leads, so the pane adds no blue of its own there. The
/// Timing step's transport latches and sliders are older than this pane and
/// are counted apart: the screen holds no more with the pane than without it.
#[test]
fn auexport_one_primary_wherever_the_pane_stands() {
    let primary = |h: &EditorHarness, row: &str| h.filled_rects(plate::BLUE).contains(&h.rect(row));
    let in_pane = |h: &EditorHarness| {
        let pane = pane_rect(h);
        h.filled_rects(plate::BLUE).into_iter().filter(|blue| pane.intersects(*blue)).collect::<Vec<_>>()
    };

    let mut h = shipped(ball(3));
    h.editor_mut().enter_room(Stage::Edit);
    settle(&mut h);
    let without = h.filled_rects(plate::BLUE).len();
    assert_eq!(h.perform(Action::ShowRenderQueue), Performed::Done);
    settle(&mut h);
    h.click(export_step::MORE_OPTIONS);
    settle(&mut h);
    assert!(h.has(GIF) && !primary(&h, GIF), "a plain choice here: {:?}", h.filled_rects(plate::BLUE));
    assert_eq!(in_pane(&h), [], "the pill leads");
    assert!(h.filled_rects(plate::BLUE).len() <= without, "{:?}", h.filled_rects(plate::BLUE));

    // With the fold open the queue's own armed chip is the pane's one.
    h.click(export_step::MORE_OPTIONS);
    settle(&mut h);
    assert!(in_pane(&h).len() <= 1, "{:?}", in_pane(&h));

    // In the step it leads, until a Done card's `Open file` does.
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(ball(3));
    enter(&mut h);
    assert!(primary(&h, GIF), "{:?}", h.filled_rects(plate::BLUE));
    assert!(h.filled_rects(plate::BLUE).len() <= 2, "{:?}", h.filled_rects(plate::BLUE));
    export_gif(&mut h, Action::ExportGif, &dir.path().join("ball.gif"));
    assert!(primary(&h, export_step::OPEN_FILE) && !primary(&h, GIF), "{:?}", h.filled_rects(plate::BLUE));
    assert!(h.filled_rects(plate::BLUE).len() <= 2, "{:?}", h.filled_rects(plate::BLUE));
}

/// Spec 8.2 and AU10: an upgrader's mask marks every lesson as learned, and
/// the Exported row is no lesson.
#[test]
fn auexport_news_reaches_an_upgrader() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(ball(3));
    h.rig_mut().config.ui.hints_seen = u32::MAX;
    settle(&mut h);
    assert_eq!(lane(&h), None, "every lesson is seen");
    export_gif(&mut h, Action::ExportGif, &dir.path().join("ball.gif"));
    assert_eq!(lane(&h).as_deref(), Some("Exported ball.gif."));
    assert!(h.has(OPEN), "{}", h.describe());
}

/// AU9's item 1: F9 runs a new Action, not a room key: pressed for real it
/// enters the step, and a config saved before the action existed gets it.
#[test]
fn auexport_f9_is_a_new_action_old_configs_receive() {
    let mut h = shipped(ball(3));
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    h.key(egui::Key::F9);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Export), "the key enters the step");

    let dir = tempfile::tempdir().unwrap();
    let mut old = Config::default();
    old.keybinds.retain(|b| b.action != Action::RoomExport);
    assert_eq!(old.binding(Action::RoomExport), None, "a file from before the step");
    old.save_to(dir.path()).unwrap();
    let (loaded, _) = Config::load_from(dir.path());
    let chord = loaded.chord_for(Action::RoomExport).expect("delivered on load");
    assert_eq!(chord.keys, ["F9"]);
    assert!(!chord.ctrl && !chord.shift && !chord.alt, "{chord:?}");
}

/// A palette opened before any menu has not searched yet. The search answers
/// before the dialog, so no dialog leads to a job that could only fail.
#[test]
fn auexport_video_searches_before_its_dialog() {
    let mut h = shipped(ball(3));
    h.state_mut().render_queue.ffmpeg = None;
    let searches = h.state().render_queue.searches;
    let done = h.perform(Action::ExportMp4);
    assert_eq!(h.state().render_queue.searches, searches + 1, "the one search");
    match h.state().render_queue.ffmpeg_cached().expect("the search answered") {
        Some(_) => assert_eq!(done, Performed::App(AppRequest::ExportTo(ExportKind::Mp4))),
        None => {
            assert_eq!(done, Performed::Ignored);
            assert!(h.state().refusal.ends_with(export_step::NO_FFMPEG), "{}", h.state().refusal);
        }
    }
}

/// `File ▸ Export`'s frame fields and the panel's choices are one route, so the
/// range holds for both. It is said where the choice is made, and the words
/// behind every choice allow for it.
#[test]
fn auexport_a_file_menu_range_is_said_where_it_holds() {
    const RANGED: &str = "Frames 5 to 8 only, as File ▸ Export says.";
    for action in [Action::ExportGif, Action::ExportMp4, Action::ExportPngSequence] {
        assert!(action.help().contains("frame range"), "{}", action.help());
        assert!(!action.help().contains("whole animation"), "{}", action.help());
    }
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(ball(12));
    enter(&mut h);
    assert!(!says(&h, RANGED), "{:?}", h.painted_text());
    h.editor_mut().export_range = Some((4, 7));
    settle(&mut h);
    assert!(says(&h, RANGED), "{:?}", h.painted_text());
    export_gif(&mut h, Action::ExportGif, &dir.path().join("clip.gif"));
    assert_eq!(h.state().render_queue.queue.jobs[0].span, (4, 7), "what it says is what ran");
}

/// A session's join swaps the document under the step: the way back is the
/// Editor's, so its name holds. A new Editor carries a step it can stand in.
#[test]
fn auexport_the_way_back_outlives_the_document() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(ball(3));
    assert_eq!(h.editor().export_view().back, None, "only in the step");
    h.editor_mut().enter_room(Stage::Edit);
    settle(&mut h);
    enter(&mut h);
    h.editor_mut().canvas.is_guest = true;
    h.editor_mut().state = ball(3);
    h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
    settle(&mut h);
    export_gif(&mut h, Action::ExportGif, &dir.path().join("ball.gif"));
    assert!(says(&h, "Back to Timing"), "{:?}", h.painted_text());
    assert_eq!(h.editor().stage_kept(), Some(Stage::Edit), "the step it came from");

    // From off the steps there is no step to carry, and the first step is the
    // way back of an Export step no dock swap entered.
    h.editor_mut().export_from = Some((None, Stage::Drawing.dock()));
    assert_eq!(h.editor().stage_kept(), None);
    h.editor_mut().export_from = None;
    assert_eq!(h.editor().stage_kept(), Some(Stage::Drawing));
    assert_eq!(h.editor().export_view().back.as_deref(), Some(vocab::STEP_PLAN));
    h.editor_mut().stage = Some(Stage::Edit);
    assert_eq!(h.editor().stage_kept(), Some(Stage::Edit), "any other step is carried as it was");
}

/// Spec 5.3's peek: pressing the step it came from is `Back to`, whichever
/// button says so: the dock comes back as it was left, not as the recipe has it.
#[test]
fn auexport_a_look_at_the_result_costs_nothing() {
    let mut h = shipped(ball(3));
    h.editor_mut().enter_room(Stage::Edit);
    settle(&mut h);
    h.editor_mut().open_pane(Pane::History);
    settle(&mut h);
    let dock = visible_panes(&h.editor().dock);
    assert!(dock.contains(&Pane::History), "{dock:?}");
    assert_ne!(dock, visible_panes(&h.editor().recipe(Stage::Edit)), "a dock of the person's own");
    enter(&mut h);
    h.click(vocab::STEP_TIMING);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Edit));
    assert_eq!(visible_panes(&h.editor().dock), dock, "as it was left");
    assert!(h.editor().export_from.is_none(), "nothing left to come back to");

    // Any other step is entered as ever, with what the step left behind gone.
    enter(&mut h);
    h.click(vocab::STEP_PLAN);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert_eq!(visible_panes(&h.editor().dock), visible_panes(&h.editor().recipe(Stage::Drawing)));
    assert!(h.editor().export_from.is_none());
}

/// The queue's panel follows the Export panel in one Ui: it keeps its own
/// rows' rhythm, and stands on the cards' left edge.
#[test]
fn auexport_queue_keeps_its_rhythm_under_the_panel() {
    let p = PanelHarness::new(vec2(340.0, 600.0), ball(3), |ui, state| {
        let rhythm = ui.spacing().item_spacing;
        export_step::ui(ui, state, &view(), false);
        assert_eq!(ui.spacing().item_spacing, rhythm, "the panel's gaps are its own");
    });
    assert!(p.has(GIF), "the panel drew");

    let mut h = EditorHarness::with_state(ball(3));
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h);
    assert_eq!(h.rect("preset").left(), h.rect(GIF).left(), "one edge down the pane");
}
