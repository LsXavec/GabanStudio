//! AU22: the Comic step: a page is lettered, seen as it will print and sent
//! out without the graph (spec Comic step, 5.5). The prefix is `aupage_`. No
//! test here opens a dialog or a window, and the pages go to a temp folder.

use std::path::{Path, PathBuf};

use anim_core::deliver::PageMode;
use anim_core::generator::{GenKind, GenValues};
use anim_core::graph::NodeKind;
use anim_core::ids::{ColumnId, NodeId};
use anim_core::manga;
use eframe::egui;
use egui::{Key, Pos2, pos2, vec2};

use crate::aurail_tests::window;
use crate::auguide_tests;
use crate::canvas::extpaper_tests::paint_texels;
use crate::config::Action;
use crate::doc;
use crate::export_step;
use crate::lessons::{self, Done, Latch, Lesson, Target, Task};
use crate::manga_tools;
use crate::measure::{self, Scope};
use crate::testkit::EditorHarness;
use crate::welcome::{EXPORT_STEP, WelcomeAction};
use crate::workspace::{Pane, Stage};
use crate::{PROBE_SCALES, Performed, plate, say, templates, vocab};

const INK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const INK_TRACK: &str = "Ink track";
const TONE_TRACK: &str = "Tone track";
const PANE_MONO: &str = "in mono (1-bit)";
const SIZES: [(f32, f32); 4] = [(1600.0, 900.0), (1280.0, 800.0), (1280.0, 720.0), (1024.0, 640.0)];

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

/// A shipped comic with ink on its page, as a person starts one.
fn comic() -> EditorHarness {
    let mut h = EditorHarness::as_shipped();
    h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    h.kit_mut().set_pixels_per_point(1.0);
    settle(&mut h);
    h.answer_start(WelcomeAction::Template(templates::COMIC));
    settle(&mut h);
    paint_texels(h.state_mut(), &[(10, 10), (11, 10)], INK);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Manga));
    h
}

/// The one control named `label`, pressed where it shows in its pane's scroll.
pub(crate) fn press(h: &mut EditorHarness, label: &str) {
    h.scroll_to(label);
    h.click(label);
    settle(h);
}

fn paints(h: &EditorHarness, words: &str) -> bool {
    h.painted_text().iter().any(|w| w.contains(words))
}

/// The page's generators other than its panels, by kind, bottom first.
fn kinds(h: &EditorHarness) -> Vec<GenKind> {
    manga_tools::letters(h.state().cut()).into_iter().map(|(_, kind)| kind).collect()
}

fn the(h: &EditorHarness, kind: GenKind) -> NodeId {
    let all = manga_tools::letters(h.state().cut());
    all.into_iter().find(|(_, k)| *k == kind).unwrap_or_else(|| panic!("no {kind:?} on the page")).0
}

fn values(h: &EditorHarness, id: NodeId) -> GenValues {
    let NodeKind::Generator { generator, params, .. } = &h.state().cut().graph.node(id).expect("the node").kind else {
        panic!("no generator");
    };
    GenValues::of(*generator, params)
}

fn steps(h: &EditorHarness) -> usize {
    h.state().engine.history_len()
}

/// Where page px (`x`, `y`) stands on screen.
fn on_screen(h: &EditorHarness, x: f32, y: f32) -> Pos2 {
    h.editor().canvas.view_xform().paper_to_screen(pos2(x, y))
}

/// The page's two template tracks.
fn tracks(h: &EditorHarness) -> (ColumnId, ColumnId) {
    let column = |word: &str| {
        let columns = &h.state().cut().xsheet.columns;
        columns.iter().find(|c| c.name == word).unwrap_or_else(|| panic!("no {word} track")).id
    };
    (column("INK"), column("TONE"))
}

/// The book's pages into `dir`, past the folder dialog and to the end of the
/// worker, as the App's request runs them.
fn export_pages(h: &mut EditorHarness, dir: &Path) {
    h.state_mut().begin_page_export(dir, None);
    h.state_mut().wait_page_export();
    settle(h);
}

fn the_png(dir: &Path) -> PathBuf {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|f| f.unwrap().path()).collect();
    files.retain(|f| f.extension().is_some_and(|e| e == "png"));
    assert_eq!(files.len(), 1, "one page: {files:?}");
    files.remove(0)
}

/// `file`'s pixels as (red, green, blue) rows, however the writer packed them.
fn pixels(file: &Path) -> Vec<[u8; 3]> {
    let mut decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(file).unwrap()));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    let per = info.color_type.samples();
    let spread = |px: &[u8]| if per >= 3 { [px[0], px[1], px[2]] } else { [px[0]; 3] };
    buf[..info.buffer_size()].chunks_exact(per).map(spread).collect()
}

/// The open book as one page writes it.
fn printed(h: &mut EditorHarness) -> Vec<[u8; 3]> {
    let dir = tempfile::tempdir().unwrap();
    export_pages(h, dir.path());
    pixels(&the_png(dir.path()))
}

fn differing(a: &[[u8; 3]], b: &[[u8; 3]]) -> usize {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// The `Words` field's place among the window's text fields.
fn words_field(h: &EditorHarness) -> usize {
    let pane = h.editor().pane_rects.iter().find(|(pane, ..)| *pane == Pane::Manga).expect("the pane is forward").1;
    let found = (0..h.text_fields().len()).find(|&n| pane.contains_rect(h.text_field_rect(n)));
    found.unwrap_or_else(|| panic!("no Words field\n{}", h.describe()))
}

pub(crate) fn type_words(h: &mut EditorHarness, words: &str) {
    let field = words_field(h);
    h.focus_text_field(field);
    h.type_text(words);
    h.key(Key::Enter);
    settle(h);
}

/// Words typed and left in the field, no Enter and no press.
fn type_held(h: &mut EditorHarness, words: &str) {
    let field = words_field(h);
    h.focus_text_field(field);
    h.type_text(words);
}

fn page_px(h: &EditorHarness) -> (f32, f32) {
    (h.state().engine.project.width as f32, h.state().engine.project.height as f32)
}

/// Page px of `v`, a generator's fractions, `out` of the way across its width.
fn off_centre(h: &EditorHarness, v: &GenValues, out: f32) -> Pos2 {
    let (w, tall) = page_px(h);
    pos2((v.float("x") + v.float("width") * out) * w, v.float("y") * tall)
}

fn screen_of(h: &EditorHarness, p: Pos2) -> Pos2 {
    on_screen(h, p.x, p.y)
}

/// A tap with the mouse: a press that does not move.
fn tap(h: &mut EditorHarness, at: Pos2) {
    h.drag(at, at, 1);
    settle(h);
}

/// The pixels of `file` that lie in `v`'s ellipse, `inset` of its radii, and
/// are not white: `file` is the whole page.
fn marked_in_balloon(h: &EditorHarness, px: &[[u8; 3]], v: &GenValues, inset: f32) -> usize {
    let (w, tall) = page_px(h);
    let (cx, cy) = (v.float("x") * w, v.float("y") * tall);
    let (rx, ry) = (v.float("width") * w / 2.0 * inset, v.float("height") * tall / 2.0 * inset);
    assert_eq!(px.len(), w as usize * tall as usize, "the whole page");
    let (x0, x1, y0, y1) = ((cx - rx) as usize, (cx + rx) as usize, (cy - ry) as usize, (cy + ry) as usize);
    let inside = |x: usize, y: usize| ((x as f32 - cx) / rx).powi(2) + ((y as f32 - cy) / ry).powi(2) <= 1.0;
    let cells = (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y)));
    cells.filter(|&(x, y)| inside(x, y) && px[y * w as usize + x] != [255; 3]).count()
}

/// Spec Comic step: a line on the composed page has nowhere to go, so the
/// refusal says the way back, and the way back draws. Stage::Manga enters off.
#[test]
fn aupage_see_the_page_refuses_a_stroke_with_the_way_back() {
    let mut h = comic();
    assert!(!h.editor().canvas.composite_view, "the step opens on the drawing");
    let (from, to) = (on_screen(&h, 900.0, 1400.0), on_screen(&h, 1300.0, 1700.0));
    press(&mut h, vocab::SEE_THE_PAGE);
    assert!(h.editor().canvas.composite_view);
    assert_eq!(h.state().status, manga_tools::SEE_PAGE_SAYS);

    let before = steps(&h);
    h.drag(from, to, 6);
    settle(&mut h);
    assert_eq!(steps(&h), before, "nothing landed");
    assert_eq!(h.state().refusal, doc::SEE_PAGE_ON);
    assert_eq!(doc::SEE_PAGE_ON, "Turn off See the page to draw.");
    assert_eq!(say::fix_of(doc::SEE_PAGE_ON), Some(Action::ToggleCompositeView), "Fix it turns it off");

    press(&mut h, vocab::SEE_THE_PAGE);
    assert!(!h.editor().canvas.composite_view);
    h.drag(from, to, 6);
    settle(&mut h);
    assert_eq!(steps(&h), before + 1, "and the same line draws");

    // Stage::Manga enters off, wherever it was left on.
    press(&mut h, vocab::SEE_THE_PAGE);
    h.editor_mut().enter_room(Stage::Composite);
    settle(&mut h);
    h.editor_mut().enter_room(Stage::Manga);
    settle(&mut h);
    assert!(!h.editor().canvas.composite_view);

    // A comic's own bar too: Export comes back by another road, however it is
    // taken, and it is the road a page takes.
    press(&mut h, vocab::SEE_THE_PAGE);
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    assert_eq!(h.perform(Action::MangaRoom), Performed::Done);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Manga));
    assert!(!h.editor().canvas.composite_view, "the step's pill");
    press(&mut h, vocab::SEE_THE_PAGE);
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    h.editor_mut().leave_export();
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Manga));
    assert!(!h.editor().canvas.composite_view, "`Back to Manga`");

    // Off this step the words stay the ones they were.
    h.editor_mut().canvas.composite_view = true;
    h.editor_mut().enter_room(Stage::Composite);
    h.editor_mut().canvas.composite_view = true;
    settle(&mut h);
    assert!(!h.editor().canvas.page_step, "only the Comic step's latch is named");
}

/// Spec acceptance 1: with no GPU the page cannot be composed, and it says so.
#[test]
fn aupage_see_the_page_says_why_with_no_gpu() {
    let mut h = comic();
    assert!(!paints(&h, manga_tools::SEE_PAGE_NO_GPU));
    press(&mut h, vocab::SEE_THE_PAGE);
    assert!(paints(&h, manga_tools::SEE_PAGE_NO_GPU), "{:?}", h.painted_text());
    press(&mut h, vocab::SEE_THE_PAGE);
    assert!(!paints(&h, manga_tools::SEE_PAGE_NO_GPU), "the note goes with the view");
}

/// Spec acceptance 1, with a GPU: the composed page is what stands on the
/// paper, in black where the drawing's lettering is grey, and nothing that will
/// not print stands on it.
#[test]
#[ignore = "needs a GPU"]
fn aupage_see_the_page_shows_the_composed_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = EditorHarness::with_gpu_ready();
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    h.kit_mut().set_pixels_per_point(1.0);
    h.run_frames(4);
    h.answer_start(WelcomeAction::Template(templates::COMIC));
    h.run_frames(4);
    press(&mut h, vocab::ADD_BALLOON);
    type_words(&mut h, "tink");
    let paper = h.editor().canvas.dbg_rect;
    let on_paper = |h: &EditorHarness| h.painted_text_in(paper).iter().any(|w| w.contains("tink"));
    assert!(on_paper(&h) && !h.editor().canvas.composite_shown, "the drawing's own lettering");
    let v = values(&h, the(&h, GenKind::Balloon));
    let (a, b) = (off_centre(&h, &v, -0.5), off_centre(&h, &v, 0.5));
    let span = egui::Rect::from_min_max(screen_of(&h, a), screen_of(&h, b)).expand2(vec2(0.0, 40.0));
    let black = |file: &Path| {
        let mut decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(file).unwrap()));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let info = decoder.read_info().unwrap().info().clone();
        let px = pixels(file);
        let wide = info.width as usize;
        let cells = (span.top() as usize..span.bottom() as usize).flat_map(|y| (span.left() as usize..span.right() as usize).map(move |x| (x, y)));
        cells.filter(|&(x, y)| px[y * wide + x].iter().all(|c| *c < 40)).count()
    };
    let drawn = dir.path().join("drawn.png");
    h.shot(vec2(1600.0, 900.0), &drawn);
    assert_eq!(black(&drawn), 0, "the drawing's lettering is a grey line");
    press(&mut h, vocab::SEE_THE_PAGE);
    // `Ready` is the picture asked for: this size of page takes its time.
    for _ in 0..800 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        h.run_frames(1);
    }
    assert!(h.editor().canvas.composite_shown, "the composed picture stands: {:?}", h.painted_text());
    assert!(!paints(&h, manga_tools::SEE_PAGE_NO_GPU));
    assert!(!on_paper(&h), "the words are the picture's now");
    assert!(h.painted_text_in(paper).iter().any(|w| w == vocab::SEE_THE_PAGE), "and the tag has the latch's name");
    let seen = dir.path().join("seen.png");
    h.shot(vec2(1600.0, 900.0), &seen);
    let printed = black(&seen);
    assert!(printed > 100, "{printed} black pixels of the balloon's line and words");
}

/// See the page says "this is how it prints": the view paints no line of its
/// own but the handle, and that only under the pointer.
#[test]
fn aupage_the_printed_view_carries_no_marks() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    type_words(&mut h, "tink");
    let v = values(&h, the(&h, GenKind::Balloon));
    let inside = off_centre(&h, &v, 0.25);
    let shapes = |h: &EditorHarness, printed: bool, over: Option<[f32; 2]>| {
        let ctx = egui::Context::default();
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let id = egui::LayerId::background();
            let painter = egui::Painter::new(ui.ctx().clone(), id, egui::Rect::EVERYTHING);
            manga_tools::paint_letters(&painter, h.state(), &|p| p, None, printed, over);
        });
        out.shapes.len()
    };
    assert!(shapes(&h, false, None) >= 5, "the drawing: its line, tail, words, box and handle");
    assert_eq!(shapes(&h, true, None), 0, "nothing on the printed page");
    assert_eq!(shapes(&h, true, Some([1.0, 1.0])), 0, "nor with the pointer elsewhere");
    assert_eq!(shapes(&h, true, Some([inside.x, inside.y])), 2, "the handle, to drag it from this view");
}

/// Spec Comic step: Ink and Tone are one choice of the active track, and Tone
/// is the template's own column. A rebuild leaves the choice alone.
#[test]
fn aupage_latch_sets_the_track() {
    let mut h = comic();
    let (ink, tone) = tracks(&h);
    press(&mut h, TONE_TRACK);
    assert_eq!(h.state().view.active_column, tone);
    assert!(manga::is_tone_column(h.state().cut(), h.state().view.active_column), "the template's column");
    press(&mut h, vocab::ADD_BALLOON);
    assert_eq!(h.state().view.active_column, tone, "the page's rebuild keeps the track");
    assert_eq!(tracks(&h), (ink, tone), "and the tracks themselves");
    press(&mut h, INK_TRACK);
    assert_eq!(h.state().view.active_column, ink);
}

/// Spec acceptance 2: the latch is a page's, and a page's with both tracks.
/// The verbs are a page's too.
#[test]
fn aupage_latch_is_absent_off_a_page() {
    let mut h = EditorHarness::new();
    assert_eq!(h.perform(Action::MangaRoom), Performed::Done);
    settle(&mut h);
    assert!(h.has("New page"), "{}", h.describe());
    for word in [INK_TRACK, TONE_TRACK, vocab::SEE_THE_PAGE, vocab::ADD_BALLOON, vocab::ADD_TEXT, vocab::SPEED_LINES] {
        assert!(!h.has(word), "{word} off a page\n{}", h.describe());
    }
    // A page that lost its TONE track has no second half to the choice.
    let mut h = comic();
    assert!(h.has(INK_TRACK) && h.has(TONE_TRACK), "{}", h.describe());
    let (at, id) = (h.state().at(), tracks(&h).1);
    let rename = anim_core::command::Command::RenameColumn { at, id, name: "SKY".into() };
    h.state_mut().engine.apply("rename", vec![rename]).expect("applied");
    settle(&mut h);
    assert!(!h.has(INK_TRACK) && !h.has(TONE_TRACK), "{}", h.describe());
    assert!(h.has(vocab::SEE_THE_PAGE) && h.has(vocab::ADD_BALLOON), "the rest stays the page's");
}

/// `word` adds one `kind`, as one undo step that takes it off again, with the
/// page's stack rebuilt, and the page's file shows it.
fn verb_reaches_the_file(word: &str, kind: GenKind) {
    let mut h = comic();
    let bare = printed(&mut h);
    let before = steps(&h);
    let nodes = h.state().cut().graph.nodes().count();
    press(&mut h, word);
    assert_eq!(kinds(&h), [kind], "{} / {}", h.state().status, h.state().refusal);
    assert_eq!(steps(&h), before + 1, "one undo");
    assert_eq!(h.state().page_hand_built(), None, "the page stays the page's");
    assert_eq!(h.state().selected_node, Some(the(&h, kind)), "selected, for its rows");
    let shown = printed(&mut h);
    let changed = differing(&bare, &shown);
    assert!(changed > 200, "{changed} pixels of the page changed");

    h.state_mut().undo();
    settle(&mut h);
    assert_eq!(kinds(&h), []);
    assert_eq!(h.state().cut().graph.nodes().count(), nodes);
    assert_eq!(h.state().page_hand_built(), None);
    assert_eq!(differing(&bare, &printed(&mut h)), 0, "undo takes it off the file too");
}

#[test]
fn aupage_balloon_is_one_undo_and_reaches_the_file() {
    verb_reaches_the_file(vocab::ADD_BALLOON, GenKind::Balloon);
}

#[test]
fn aupage_text_is_one_undo_and_reaches_the_file() {
    verb_reaches_the_file(vocab::ADD_TEXT, GenKind::Text);
}

#[test]
fn aupage_speed_lines_are_one_undo_and_reach_the_file() {
    verb_reaches_the_file(vocab::SPEED_LINES, GenKind::SpeedLines);
}

/// Spec acceptance 3: what a verb added is the page's: `Clip to panels` asks
/// nothing, and keeps it.
#[test]
fn aupage_verbs_leave_no_danger() {
    let mut h = comic();
    for word in [vocab::ADD_BALLOON, vocab::ADD_TEXT, vocab::SPEED_LINES, vocab::ADD_BALLOON] {
        press(&mut h, word);
        assert_eq!(h.state().page_hand_built(), None, "{word}");
    }
    let printing = [GenKind::SpeedLines, GenKind::Balloon, GenKind::Balloon, GenKind::Text];
    assert_eq!(kinds(&h), printing, "lines under balloons under words");
    assert!(!h.has_text_containing("DANGER"), "{}", h.describe());
    assert!(h.state_mut().clip_to_panels());
    assert_eq!(kinds(&h), printing);
    assert_eq!(h.state().page_hand_built(), None);
}

/// Text under a balloon added after it stays readable: what a person reads
/// stands on top, whatever the order of the presses.
#[test]
fn aupage_words_stay_over_a_balloon_added_after_them() {
    let mut alone = comic();
    press(&mut alone, vocab::ADD_BALLOON);
    let empty = printed(&mut alone);

    let mut h = comic();
    press(&mut h, vocab::ADD_TEXT);
    type_words(&mut h, "tink");
    press(&mut h, vocab::ADD_BALLOON);
    assert_eq!(kinds(&h), [GenKind::Balloon, GenKind::Text]);
    assert_eq!(h.state().selected_node, Some(the(&h, GenKind::Balloon)), "the new one, by its place");
    assert_eq!(h.text_fields()[words_field(&h)], "tink", "and the words the field reads are the words shown");
    assert_eq!(h.state().page_hand_built(), None);
    let changed = differing(&empty, &printed(&mut h));
    assert!(changed > 50, "{changed} pixels of the words print over the balloon");
}

/// Speed lines added last stay behind the balloon they would cross.
#[test]
fn aupage_speed_lines_stay_under_a_balloon() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    let v = values(&h, the(&h, GenKind::Balloon));
    let bare = printed(&mut h);
    press(&mut h, vocab::SPEED_LINES);
    assert_eq!(kinds(&h), [GenKind::SpeedLines, GenKind::Balloon]);
    let shown = printed(&mut h);
    assert!(differing(&bare, &shown) > 200, "the lines print");
    assert_eq!(marked_in_balloon(&h, &shown, &v, 0.9), 0, "and the balloon's white holds");
}

/// Must not change: a page shaped by hand is still shaped by hand. A verb wires
/// over it, drops nothing and leaves the DANGER rule as it stood.
#[test]
fn aupage_a_hand_built_page_keeps_its_nodes() {
    let mut h = comic();
    let at = h.state().at();
    let glow = h.state_mut().engine.alloc_node_id();
    let add = anim_core::command::Command::AddNode {
        at,
        id: glow,
        kind: NodeKind::effect(anim_core::fx::FxKind::Glow),
    };
    h.state_mut().engine.apply("glow", vec![add]).expect("applied");
    settle(&mut h);
    let built = h.state().page_hand_built();
    assert!(built.is_some());
    let before = steps(&h);
    press(&mut h, vocab::ADD_BALLOON);
    assert!(h.state().cut().graph.node(glow).is_ok(), "the hand's node stays");
    assert_eq!(kinds(&h), [GenKind::Balloon]);
    assert_eq!(steps(&h), before + 1);
    assert!(h.state().page_hand_built().is_some(), "and the rule still says so");
}

/// Spec acceptance 4: the words typed are the words on the page.
#[test]
fn aupage_words_reach_the_file() {
    let mut h = comic();
    press(&mut h, vocab::ADD_TEXT);
    let text = the(&h, GenKind::Text);
    assert_eq!(values(&h, text).text("text"), manga_tools::TEXT_START, "a stand-in shows the button did something");
    let stand_in = printed(&mut h);
    let before = steps(&h);
    type_words(&mut h, "tink");
    assert_eq!(values(&h, text).text("text"), "tink");
    assert_eq!(steps(&h), before + 1, "one undo, however many letters");
    assert_eq!(h.text_fields()[words_field(&h)], "tink", "the field reads the page");
    assert_eq!(h.state().page_hand_built(), None);
    let changed = differing(&stand_in, &printed(&mut h));
    assert!(changed > 50, "{changed} pixels of the page changed");

    h.state_mut().undo();
    settle(&mut h);
    assert_eq!(values(&h, text).text("text"), manga_tools::TEXT_START);
}

/// A balloon has no words of its own, and Generator params stay: the words it
/// is given stand in a Text at its centre, and that is the one undo step.
#[test]
fn aupage_words_on_a_balloon_stand_inside_it() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    let balloon = the(&h, GenKind::Balloon);
    let before = steps(&h);
    type_words(&mut h, "tink");
    assert_eq!(steps(&h), before + 1);
    assert_eq!(kinds(&h), [GenKind::Balloon, GenKind::Text]);
    let balloon = if h.state().cut().graph.node(balloon).is_ok() { balloon } else { the(&h, GenKind::Balloon) };
    assert_eq!(h.state().selected_node, Some(balloon), "the balloon stays selected");
    let (b, t) = (values(&h, balloon), values(&h, the(&h, GenKind::Text)));
    assert_eq!(t.text("text"), "tink");
    assert_eq!((t.float("x"), t.float("y")), (b.float("x"), b.float("y")));
    assert_eq!(h.text_fields()[words_field(&h)], "tink", "the balloon's field reads its lettering");

    // New words go to the same Text, not to a second.
    let before = steps(&h);
    let field = words_field(&h);
    h.focus_text_field(field);
    h.key_mods(egui::Modifiers::COMMAND, Key::A);
    h.type_text("clang");
    h.key(Key::Enter);
    settle(&mut h);
    assert_eq!(kinds(&h), [GenKind::Balloon, GenKind::Text]);
    assert_eq!(values(&h, the(&h, GenKind::Text)).text("text"), "clang");
    assert_eq!(steps(&h), before + 1);
}

/// A press that takes the focus from `Words` lands too, after the words.
#[test]
fn aupage_a_press_elsewhere_lets_the_words_go_and_still_lands() {
    let mut h = comic();
    press(&mut h, vocab::ADD_TEXT);
    let field = words_field(&h);
    h.focus_text_field(field);
    h.type_text("tink");
    press(&mut h, vocab::ADD_BALLOON);
    assert_eq!(kinds(&h), [GenKind::Balloon, GenKind::Text]);
    assert_eq!(values(&h, the(&h, GenKind::Text)).text("text"), "tink");
}

/// Spec acceptance 4 on the road the lane suggests: the words left in the field
/// are in the file behind the Export pill, and the field keeps no ghost.
#[test]
fn aupage_leaving_the_step_lands_the_words() {
    for road in ["pill", "chord"] {
        let mut h = comic();
        press(&mut h, vocab::ADD_TEXT);
        let text = the(&h, GenKind::Text);
        let stand_in = printed(&mut h);
        type_held(&mut h, "tink");
        match road {
            "pill" => h.click(vocab::STEP_EXPORT),
            _ => assert_eq!(h.perform(Action::RoomExport), Performed::Done),
        }
        settle(&mut h);
        assert_eq!(h.editor().stage, Some(Stage::Export), "{road}");
        assert_eq!(values(&h, text).text("text"), "tink", "{road}");
        assert_eq!(h.state().manga.typing, None, "{road}: no words left behind");
        let changed = differing(&stand_in, &printed(&mut h));
        assert!(changed > 50, "{road}: {changed} pixels of the page changed");
        assert_eq!(h.perform(Action::MangaRoom), Performed::Done);
        settle(&mut h);
        assert_eq!(h.text_fields()[words_field(&h)], "tink", "{road}: the field reads the page");

        // A balloon has no Text until its words land.
        let mut h = comic();
        press(&mut h, vocab::ADD_BALLOON);
        type_held(&mut h, "tink");
        match road {
            "pill" => h.click(vocab::STEP_EXPORT),
            _ => assert_eq!(h.perform(Action::RoomExport), Performed::Done),
        }
        settle(&mut h);
        assert_eq!(kinds(&h), [GenKind::Balloon, GenKind::Text], "{road}");
        assert_eq!(values(&h, the(&h, GenKind::Text)).text("text"), "tink", "{road}");
    }
}

/// A save and a page export fire over the field: the file has the words.
#[test]
fn aupage_a_save_and_an_export_land_the_words() {
    let mut h = comic();
    press(&mut h, vocab::ADD_TEXT);
    let text = the(&h, GenKind::Text);
    type_held(&mut h, "tink");
    h.key_mods(crate::testkit::CTRL, Key::S);
    settle(&mut h);
    assert!(h.performed().iter().any(|(a, _)| *a == Action::Save), "{:?}", h.performed());
    assert_eq!(values(&h, text).text("text"), "tink", "Ctrl+S reads the page next");

    type_held(&mut h, "!");
    assert!(h.state().manga.typing.is_some());
    assert_eq!(h.perform(Action::ExportPages), Performed::App(crate::AppRequest::ExportPages));
    assert_eq!(values(&h, text).text("text"), "tink!");
}

/// The pane can go by a road no verb walks, and a field not drawn never lets go.
#[test]
fn aupage_a_field_gone_from_the_screen_lands_its_words() {
    let mut h = comic();
    press(&mut h, vocab::ADD_TEXT);
    let text = the(&h, GenKind::Text);
    type_held(&mut h, "tink");
    let dock = h.editor().recipe(Stage::Export);
    h.editor_mut().dock = dock;
    settle(&mut h);
    assert_eq!(values(&h, text).text("text"), "tink");
    assert_eq!(h.state().manga.typing, None);
}

/// Words let go under a live stroke wait for it, rather than go.
#[test]
fn aupage_words_wait_out_a_stroke() {
    let mut h = comic();
    press(&mut h, vocab::ADD_TEXT);
    let text = the(&h, GenKind::Text);
    let (from, to) = (on_screen(&h, 900.0, 1400.0), on_screen(&h, 1300.0, 1700.0));
    h.press_and_move(from, to, 4);
    assert!(h.editor().canvas.stroke_active(), "a stroke stands");
    let refusals = h.state().refusal_seq;
    h.state_mut().manga.typed = Some((text, "tink".into()));
    h.run_frames(3);
    assert_eq!(values(&h, text).text("text"), manga_tools::TEXT_START);
    assert_eq!(h.state().refusal_seq, refusals, "nothing was refused");
    h.release(to);
    settle(&mut h);
    assert_eq!(values(&h, text).text("text"), "tink");
}

/// Two balloons at the same place on the same panel each hold their own words,
/// and a drag takes its own words and no others.
#[test]
fn aupage_a_second_balloon_keeps_its_own_words() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    type_words(&mut h, "Hello");
    press(&mut h, vocab::ADD_BALLOON);
    assert_eq!(h.text_fields()[words_field(&h)], "", "a new balloon has no one's words");
    type_words(&mut h, "World");

    let words = |h: &EditorHarness| {
        let texts = manga_tools::letters(h.state().cut()).into_iter().filter(|(_, k)| *k == GenKind::Text);
        texts.map(|(id, _)| (id, values(h, id))).collect::<Vec<_>>()
    };
    let said: Vec<String> = words(&h).iter().map(|(_, t)| t.text("text").to_string()).collect();
    assert_eq!(said, ["Hello", "World"]);

    let balloons: Vec<NodeId> = manga_tools::letters(h.state().cut())
        .into_iter()
        .filter(|(_, k)| *k == GenKind::Balloon)
        .map(|(id, _)| id)
        .collect();
    let before = words(&h);
    let (first, second) = (values(&h, balloons[0]), values(&h, balloons[1]));
    assert_eq!(h.state().selected_node, Some(balloons[1]));
    assert_eq!((before[1].1.float("x"), before[1].1.float("y")), (second.float("x"), second.float("y")));
    let at = screen_of(&h, off_centre(&h, &second, 0.3));
    h.drag(at, at + vec2(60.0, 40.0), 6);
    settle(&mut h);
    let after = words(&h);
    assert_eq!(after[0].1.float("x"), before[0].1.float("x"), "the first balloon's words stay");
    assert_ne!(after[1].1.float("x"), before[1].1.float("x"), "the second's go with it");
    assert_eq!(values(&h, balloons[0]).float("x"), first.float("x"), "and so does the first balloon");

    // The first balloon reads its own, picked on the page.
    let at = screen_of(&h, off_centre(&h, &first, -0.3));
    tap(&mut h, at);
    assert_eq!(h.state().selected_node, Some(balloons[0]));
    assert_eq!(h.text_fields()[words_field(&h)], "Hello");
}

/// Long words wrap in their balloon and it grows round them, as one undo.
#[test]
fn aupage_a_balloon_grows_to_its_words() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    let tall = values(&h, the(&h, GenKind::Balloon)).float("height");
    let long = "Every word of this sentence has to stand inside the balloon that was drawn for it ".repeat(3);
    let before = steps(&h);
    type_words(&mut h, long.trim());
    assert_eq!(steps(&h), before + 1, "one undo");
    let (b, t) = (values(&h, the(&h, GenKind::Balloon)), values(&h, the(&h, GenKind::Text)));
    assert!(b.float("height") > tall, "{} from {tall}", b.float("height"));
    assert!(t.float("wrap") > 0.0, "wrapped to it");
    let (page_w, page_h) = page_px(&h);
    let style = anim_core::generator::text_style(&t, page_w as u32, page_h as u32);
    let block = anim_core::text::layout(t.text("text"), &style);
    assert!(block.h / page_h <= b.float("height") + 1e-3, "{} of {}", block.h / page_h, b.float("height"));
    assert!(b.float("tail_y") > b.float("y") + b.float("height") / 2.0, "the tail stays outside");

    // Bigger words in the same Text grow it the same way.
    let grown = b.float("height");
    let field = words_field(&h);
    h.focus_text_field(field);
    h.key_mods(egui::Modifiers::COMMAND, Key::A);
    h.type_text("Short");
    h.key(Key::Enter);
    settle(&mut h);
    assert_eq!(values(&h, the(&h, GenKind::Balloon)).float("height"), grown, "a balloon never shrinks under a person");
}

/// Add balloon and Add text leave the keys in the field: the next thing a
/// person does is type, and a press on the paper hands them back.
#[test]
fn aupage_a_verb_hands_the_field_the_keys() {
    for word in [vocab::ADD_BALLOON, vocab::ADD_TEXT] {
        let mut h = comic();
        press(&mut h, word);
        assert!(h.typing(), "{word}");
        h.type_text("tink");
        h.key(Key::Enter);
        settle(&mut h);
        assert_eq!(values(&h, the(&h, GenKind::Text)).text("text"), "tink", "{word}");

        let (from, to) = (on_screen(&h, 900.0, 3000.0), on_screen(&h, 1300.0, 3100.0));
        let mut h = comic();
        press(&mut h, word);
        h.drag(from, to, 6);
        settle(&mut h);
        assert!(!h.typing(), "{word}: a stroke lets the field go");
    }
    let mut h = comic();
    press(&mut h, vocab::SPEED_LINES);
    assert!(!h.typing(), "speed lines have no words");
}

/// A field's right edge stands clear of the pane's scrollbar.
#[test]
fn aupage_the_words_field_clears_the_scrollbar() {
    let mut h = comic();
    press(&mut h, vocab::ADD_TEXT);
    for (w, tall) in [(1600.0, 900.0), (1280.0, 800.0)] {
        window(&mut h, w, tall, 1.0);
        let pane = h.editor().pane_rects.iter().find(|(pane, ..)| *pane == Pane::Manga).expect("the pane is forward").1;
        let field = h.text_field_rect(words_field(&h));
        assert!(field.right() <= pane.right() - 10.0, "{field:?} of {pane:?}");
    }
}

/// The rows grow past the fold with the page; the field a verb hands the keys
/// to comes into view, not under the scroll.
#[test]
fn aupage_a_full_page_still_shows_the_field_it_types_in() {
    let mut h = comic();
    window(&mut h, 1280.0, 800.0, 1.0);
    for _ in 0..3 {
        press(&mut h, vocab::ADD_BALLOON);
        type_words(&mut h, "tink");
    }
    // From the head of the pane, where the verbs show and the field does not.
    h.scroll_to(vocab::SEE_THE_PAGE);
    h.click(vocab::ADD_BALLOON);
    settle(&mut h);
    let pane = h.editor().pane_rects.iter().find(|(pane, ..)| *pane == Pane::Manga).expect("the pane is forward").1;
    assert!(h.typing(), "the keys are the field's");
    let field = h.text_field_rect(words_field(&h));
    assert!(pane.contains_rect(field), "Words {field:?} of {pane:?}");
}

/// Nothing added has to stay: the selected row comes off in one undo, a
/// balloon's words with it, and the page stays the page's.
#[test]
fn aupage_remove_takes_it_off_the_page() {
    const REMOVE: &str = "Remove lettering";
    let mut h = comic();
    assert!(!h.has(REMOVE), "nothing selected, nothing to remove");
    press(&mut h, vocab::ADD_BALLOON);
    type_words(&mut h, "tink");
    press(&mut h, vocab::SPEED_LINES);
    let both = printed(&mut h);
    let at = screen_of(&h, off_centre(&h, &values(&h, the(&h, GenKind::Balloon)), 0.3));
    tap(&mut h, at);
    assert_eq!(h.state().selected_node, Some(the(&h, GenKind::Balloon)));
    let before = steps(&h);
    press(&mut h, REMOVE);
    assert_eq!(kinds(&h), [GenKind::SpeedLines], "the balloon and its words");
    assert_eq!(steps(&h), before + 1, "one undo");
    assert_eq!(h.state().page_hand_built(), None, "no DANGER edge");
    assert_eq!(h.state().selected_node, None);
    assert!(differing(&both, &printed(&mut h)) > 200, "off the file too");
    h.state_mut().undo();
    settle(&mut h);
    assert_eq!(kinds(&h), [GenKind::SpeedLines, GenKind::Balloon, GenKind::Text]);
    assert_eq!(differing(&both, &printed(&mut h)), 0);
}

/// Must not change: a page shaped by hand keeps its nodes, so `Remove` points
/// at the graph rather than guess at its wires.
#[test]
fn aupage_remove_leaves_a_hand_built_page_to_the_graph() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    let at = h.state().at();
    let id = h.state_mut().engine.alloc_node_id();
    let add = anim_core::command::Command::AddNode { at, id, kind: NodeKind::effect(anim_core::fx::FxKind::Glow) };
    h.state_mut().engine.apply("glow", vec![add]).expect("applied");
    settle(&mut h);
    assert!(h.state().page_hand_built().is_some());
    let balloon = the(&h, GenKind::Balloon);
    assert!(!h.state_mut().remove_from_page("balloon", &[balloon]));
    assert_eq!(h.state().refusal, doc::REMOVE_BY_HAND);
    assert_eq!(kinds(&h), [GenKind::Balloon]);
}

/// Spec acceptance 5: a drag moves the generator by the paper delta, under
/// zoom and turn, as one undo step; a balloon takes its tail and its words.
#[test]
fn aupage_handle_writes_the_fractions() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    type_words(&mut h, "tink");
    let (w, tall) = (h.state().engine.project.width as f32, h.state().engine.project.height as f32);
    let close = |a: f32, b: f32, what: &str| assert!((a - b).abs() < 2e-3, "{what}: {a} against {b}");

    for (zoom, turn) in [(false, 0.0), (true, 30.0), (true, -75.0)] {
        h.editor_mut().canvas.fit_view();
        if zoom {
            h.editor_mut().canvas.zoom_in();
            h.editor_mut().canvas.zoom_in();
        }
        h.editor_mut().canvas.set_view_rotation(turn);
        settle(&mut h);
        let id = h.state().selected_node.expect("the balloon");
        let b = values(&h, id);
        let t = values(&h, the(&h, GenKind::Text));
        let (x, y) = (b.float("x") * w, b.float("y") * tall);
        let moved = (120.0, -80.0);
        let (from, to) = (on_screen(&h, x, y), on_screen(&h, x + moved.0, y + moved.1));
        let before = steps(&h);
        h.drag(from, to, 8);
        settle(&mut h);

        assert_eq!(steps(&h), before + 1, "one undo per drag, at {turn}");
        let after = values(&h, id);
        close(after.float("x"), b.float("x") + moved.0 / w, "x");
        close(after.float("y"), b.float("y") + moved.1 / tall, "y");
        close(after.float("tail_x"), b.float("tail_x") + moved.0 / w, "the tail");
        close(after.float("tail_y"), b.float("tail_y") + moved.1 / tall, "the tail");
        let words = values(&h, the(&h, GenKind::Text));
        close(words.float("x"), t.float("x") + moved.0 / w, "its words");
        close(words.float("y"), t.float("y") + moved.1 / tall, "its words");
    }

    // Undo puts it back.
    let id = h.state().selected_node.expect("the balloon");
    let at = values(&h, id).float("x");
    h.state_mut().undo();
    settle(&mut h);
    assert_ne!(values(&h, id).float("x"), at);
}

/// With the Brush a press anywhere on the balloon moves it and leaves no line;
/// a click selects what it lands on; and the rest of the paper still draws.
#[test]
fn aupage_a_press_in_the_box_moves_it_and_draws_nothing() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    type_words(&mut h, "tink");
    let id = the(&h, GenKind::Balloon);
    let v = values(&h, id);
    let (w, tall) = page_px(&h);
    let from = off_centre(&h, &v, 0.3);
    let handle = manga_tools::letter_handle(h.state()).expect("a handle");
    let far = screen_of(&h, from).distance(on_screen(&h, handle[0], handle[1]));
    assert!(far > manga_tools::HANDLE_REACH, "{far} from the handle");

    let (strokes, before) = (h.state().strokes_done, steps(&h));
    let (a, b) = (screen_of(&h, from), screen_of(&h, from + vec2(240.0, -160.0)));
    h.drag(a, b, 8);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, strokes, "no stray line");
    assert_eq!(steps(&h), before + 1, "one undo");
    let moved = values(&h, id);
    assert!((moved.float("x") - (v.float("x") + 240.0 / w)).abs() < 2e-3, "{}", moved.float("x"));
    assert!((moved.float("y") - (v.float("y") - 160.0 / tall)).abs() < 2e-3, "{}", moved.float("y"));

    // Nothing selected: a click selects, a balloon for the words in it.
    h.state_mut().selected_node = None;
    settle(&mut h);
    let before = steps(&h);
    let at = pos2(moved.float("x") * w, moved.float("y") * tall);
    let at = screen_of(&h, at);
    tap(&mut h, at);
    assert_eq!(h.state().selected_node, Some(id), "the words in it answer for the balloon");
    assert_eq!((h.state().strokes_done, steps(&h)), (strokes, before), "and it draws nothing");

    // A drag on an unselected balloon takes it along with the selection.
    h.state_mut().selected_node = None;
    settle(&mut h);
    let at = screen_of(&h, off_centre(&h, &moved, -0.3));
    h.drag(at, at + vec2(30.0, 0.0), 6);
    settle(&mut h);
    assert_eq!(h.state().selected_node, Some(id));
    assert_eq!(h.state().strokes_done, strokes, "no stray line");
    assert_ne!(values(&h, id).float("x"), moved.float("x"));

    // Off the box the Brush is the Brush.
    let (from, to) = (on_screen(&h, 300.0, 3000.0), on_screen(&h, 900.0, 3200.0));
    h.drag(from, to, 6);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, strokes + 1, "the paper still draws");
    assert_eq!(h.state().selected_node, Some(id));
}

/// Another tool keeps the paper: only the handle is the step's, and the Panel
/// tool keeps even that. With the guides off there is nothing to take hold of.
#[test]
fn aupage_another_tool_keeps_the_paper() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    let id = the(&h, GenKind::Balloon);
    let v = values(&h, id);
    assert_eq!(h.perform(Action::SelectTool), Performed::Done);
    settle(&mut h);
    let at = screen_of(&h, off_centre(&h, &v, 0.3));
    h.drag(at, at + vec2(40.0, 40.0), 6);
    settle(&mut h);
    assert_eq!(values(&h, id).float("x"), v.float("x"), "a selection, not a move");
    let handle = manga_tools::letter_handle(h.state()).expect("a handle");
    let at = on_screen(&h, handle[0], handle[1]);
    h.drag(at, at + vec2(40.0, 40.0), 6);
    settle(&mut h);
    assert_ne!(values(&h, id).float("x"), v.float("x"), "the handle moves it");

    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    let v = values(&h, the(&h, GenKind::Balloon));
    press(&mut h, vocab::PAGE_GUIDES);
    assert_eq!(manga_tools::letter_handle(h.state()), None);
    let strokes = h.state().strokes_done;
    let at = screen_of(&h, off_centre(&h, &v, 0.3));
    h.drag(at, at + vec2(40.0, 40.0), 6);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, strokes + 1, "guides off, the page is paper");
}

/// The lettering is the Manga step's: on a comic's Color step a tap on a balloon
/// is the Color step's own, and See the page shows nothing to take hold of but
/// what is under the pointer.
#[test]
fn aupage_lettering_stays_on_its_step() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    let id = the(&h, GenKind::Balloon);
    let v = values(&h, id);
    h.state_mut().selected_node = None;
    h.editor_mut().enter_room(Stage::InkPaint);
    settle(&mut h);
    let at = screen_of(&h, off_centre(&h, &v, 0.0));
    tap(&mut h, at);
    assert_eq!(h.state().selected_node, None, "no selection through another step's paper");
    h.drag(at, at + vec2(40.0, 0.0), 6);
    settle(&mut h);
    assert_eq!(values(&h, id).float("x"), v.float("x"));
}

/// A drag starts from See the page too: it is where the fit is judged.
#[test]
fn aupage_see_the_page_still_moves_a_balloon() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    let id = the(&h, GenKind::Balloon);
    let v = values(&h, id);
    press(&mut h, vocab::SEE_THE_PAGE);
    let at = screen_of(&h, off_centre(&h, &v, 0.3));
    h.drag(at, at + vec2(40.0, 30.0), 6);
    settle(&mut h);
    assert_ne!(values(&h, id).float("x"), v.float("x"));
    assert_eq!(h.state().refusal, "", "no refusal: it was no stroke");
}

/// While the lettering is on the drawing it shows as a person made it: the
/// words themselves, not a box, and no more with the guides off.
#[test]
fn aupage_the_drawing_shows_the_words_typed() {
    let mut h = comic();
    press(&mut h, vocab::ADD_TEXT);
    let paper = h.editor().canvas.dbg_rect;
    let shown = |h: &EditorHarness, w: &str| h.painted_text_in(paper).iter().any(|t| t.contains(w));
    assert!(shown(&h, manga_tools::TEXT_START), "{:?}", h.painted_text_in(paper));
    type_words(&mut h, "tink");
    assert!(shown(&h, "tink"), "{:?}", h.painted_text_in(paper));
    press(&mut h, vocab::PAGE_GUIDES);
    assert!(!shown(&h, "tink"));
}

/// Speed lines stay in the graph until See the page, so the verb says so, and
/// which way and how many is a field apiece, one undo each.
#[test]
fn aupage_speed_lines_say_where_they_are_and_take_two_fields() {
    let mut h = comic();
    press(&mut h, vocab::SPEED_LINES);
    assert_eq!(h.state().status, manga_tools::SPEED_LINES_SAY);
    assert_eq!(manga_tools::SPEED_LINES_SAY, "Speed lines added. See the page shows them.");
    let id = the(&h, GenKind::SpeedLines);
    assert_eq!(manga_tools::letter_handle(h.state()), None, "they stand nowhere to hold");
    let bare = printed(&mut h);

    for (name, key, value) in [("Lines direction", "angle", 90.0), ("Lines count", "count", 12.0)] {
        let before = steps(&h);
        h.scroll_to(name);
        h.click(name);
        h.key_mods(egui::Modifiers::COMMAND, Key::A);
        h.type_text(&format!("{value}"));
        h.key(Key::Enter);
        settle(&mut h);
        assert_eq!(values(&h, id).float(key), value, "{name}");
        assert_eq!(steps(&h), before + 1, "{name}: one undo");
        assert_eq!(h.state().page_hand_built(), None, "{name}");
    }
    assert!(differing(&bare, &printed(&mut h)) > 200, "they reach the file");
}

/// The Comic step's view has one name, wherever a tool remarks on it.
#[test]
fn aupage_the_view_keeps_one_name_on_the_step() {
    let mut h = comic();
    let (from, to) = (on_screen(&h, 900.0, 1400.0), on_screen(&h, 1300.0, 1700.0));
    press(&mut h, vocab::SEE_THE_PAGE);
    assert_eq!(h.perform(Action::SelectTool), Performed::Done);
    settle(&mut h);
    h.state_mut().status.clear();
    h.drag(from, to, 6);
    settle(&mut h);
    assert_eq!(h.state().status, doc::SEE_PAGE_ON);
    assert_eq!(h.perform(Action::FillTool), Performed::Done);
    settle(&mut h);
    h.state_mut().status.clear();
    h.drag(from, to, 6);
    settle(&mut h);
    assert_eq!(h.state().status, doc::SEE_PAGE_ON);
}

/// The fractions are refused past the page's reach, not stored past their range.
#[test]
fn aupage_a_drag_past_the_edge_stays_in_range() {
    let mut h = comic();
    press(&mut h, vocab::ADD_TEXT);
    let id = the(&h, GenKind::Text);
    let k = manga_tools::LetterDrag { from: [0.0, 0.0], to: [9.0e6, -9.0e6] };
    manga_tools::letter_moved(h.state_mut(), k);
    settle(&mut h);
    assert_eq!((values(&h, id).float("x"), values(&h, id).float("y")), (3.0, -2.0));
}

/// Spec 5.5: each handle a lesson rings stands on its control.
#[test]
fn aupage_every_handle_registers() {
    const NAMED: &[&[Target]] = &[
        &[Target::Named("manga.split_across")],
        &[Target::Named("manga.split_down")],
        &[Target::Named("manga.add_panel")],
        &[Target::Named("manga.delete_panel")],
        &[Target::Named("manga.panel.1")],
        &[Target::Named("manga.gutter.side")],
        &[Target::Named("manga.gutter.row")],
        &[Target::Named("manga.rulers")],
        &[Target::Named("manga.see_page")],
        &[Target::Named("manga.ink_tone")],
        &[Target::Named("manga.tone_preset")],
        &[Target::Named("manga.balloon")],
        &[Target::Named("manga.text")],
        &[Target::Named("manga.speed_lines")],
        &[Target::Named("manga.words")],
        &[Target::Named("manga.remove")],
    ];
    let mut h = comic();
    h.kit_mut().set_size(vec2(1600.0, 1400.0));
    press(&mut h, vocab::ADD_BALLOON);
    for chain in NAMED {
        auguide_tests::stand(&mut h, chain, Stage::Manga);
        assert!(plate::ring_stands(&h.ctx()).is_some(), "{chain:?}\n{}", h.describe());
    }
    press(&mut h, vocab::SPEED_LINES);
    for chain in [&[Target::Named("manga.speed.direction")], &[Target::Named("manga.speed.count")]] {
        auguide_tests::stand(&mut h, chain, Stage::Manga);
        assert!(plate::ring_stands(&h.ctx()).is_some(), "{chain:?}\n{}", h.describe());
    }
    auguide_tests::stand(&mut h, &[Target::Named("export.page.mono")], Stage::Export);
    assert!(plate::ring_stands(&h.ctx()).is_some(), "{}", h.describe());
}

/// Spec acceptance 6: a page's file ends on the Done card, and it ticks Quick
/// Start's last slot.
#[test]
fn aupage_export_reaches_the_done_card() {
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    assert!(!h.editor().quick_start[EXPORT_STEP], "no file yet");
    assert!(!h.has(export_step::SHOW_IN_FOLDER));
    let dir = tempfile::tempdir().unwrap();
    export_pages(&mut h, dir.path());
    assert!(h.has(export_step::SHOW_IN_FOLDER), "{}", h.describe());
    assert!(h.has_text_containing("Exported 1 page"), "{}", h.describe());
    assert!(h.editor().quick_start[EXPORT_STEP], "the tick is its pages on disk");
    assert!(the_png(dir.path()).exists());
}

/// Spec acceptance 7: one state, whichever side sets it, and the page out of
/// it holds black and white and nothing else.
#[test]
fn aupage_mono_on_export_is_the_pane_state() {
    let mut h = comic();
    press(&mut h, vocab::SPEED_LINES);
    assert_eq!(h.state().manga.export.mode, PageMode::Color);
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    press(&mut h, vocab::MONO);
    assert_eq!(h.state().manga.export.mode, PageMode::Mono, "the Export step's latch is the pane's mode");

    assert!(h.drag_values().contains(&0.5), "the threshold stands beside it: {:?}", h.drag_values());
    h.state_mut().manga.export.threshold = 0.25;
    settle(&mut h);
    assert!(h.drag_values().contains(&0.25), "and reads the very number: {:?}", h.drag_values());

    let pixels = printed(&mut h);
    let grey = pixels.iter().filter(|p| !matches!(p, [0, 0, 0] | [255, 255, 255])).count();
    assert_eq!(grey, 0, "of {} pixels", pixels.len());
    assert!(pixels.iter().any(|p| *p == [0, 0, 0]) && pixels.iter().any(|p| *p == [255, 255, 255]));

    // Off is the colour page, and the pane's row is the same one state.
    press(&mut h, vocab::MONO);
    assert_eq!(h.state().manga.export.mode, PageMode::Color, "off is the colour page");
    h.editor_mut().enter_room(Stage::Manga);
    settle(&mut h);
    h.scroll_to(PANE_MONO);
    h.click(PANE_MONO);
    settle(&mut h);
    assert_eq!(h.state().manga.export.mode, PageMode::Mono, "the Manga pane's rows stay");
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    press(&mut h, vocab::MONO);
    assert_eq!(h.state().manga.export.mode, PageMode::Color, "the latch read it on");
}

/// A latch has two faces and the pane three modes: on Export grey says so, and
/// Mono on and off comes back to it, whichever side turned Mono on.
#[test]
fn aupage_grey_stands_through_the_mono_latch() {
    const PANE_GREY: &str = "in grey";
    let mut h = comic();
    h.scroll_to(PANE_GREY);
    h.click(PANE_GREY);
    settle(&mut h);
    assert_eq!(h.state().manga.export.mode, PageMode::Grey);
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    assert!(paints(&h, manga_tools::GREY_NOTE), "{:?}", h.painted_text());
    press(&mut h, vocab::MONO);
    assert_eq!(h.state().manga.export.mode, PageMode::Mono);
    assert!(!paints(&h, manga_tools::GREY_NOTE), "no longer grey");
    press(&mut h, vocab::MONO);
    assert_eq!(h.state().manga.export.mode, PageMode::Grey, "back to what it stood over");
    assert!(paints(&h, manga_tools::GREY_NOTE));

    // Set on the pane, undone on Export.
    h.editor_mut().enter_room(Stage::Manga);
    settle(&mut h);
    h.scroll_to(PANE_MONO);
    h.click(PANE_MONO);
    settle(&mut h);
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    press(&mut h, vocab::MONO);
    assert_eq!(h.state().manga.export.mode, PageMode::Grey, "the pane's own road");

    // The row stands in the window at the widest it reads.
    for (w, tall) in SIZES {
        window(&mut h, w, tall, 1.0);
        let clip = measure::clip_audit(&mut h, Scope::Chrome);
        assert!(clip.found.is_empty(), "{w}×{tall}: {:?}", clip.found);
    }
}

/// Spec acceptance 8: the step's new rows stand in the window at every size,
/// and on Export for a comic.
#[test]
fn aupage_clip_audit_passes_on_the_comic_step() {
    let mut found = Vec::new();
    let mut h = comic();
    press(&mut h, vocab::ADD_BALLOON);
    for (w, tall) in SIZES {
        for stored in PROBE_SCALES {
            window(&mut h, w, tall, stored);
            let clip = measure::clip_audit(&mut h, Scope::Chrome);
            found.extend(clip.found.into_iter().map(|f| format!("{w}×{tall} at {stored}: {f}")));
        }
    }
    assert!(found.is_empty(), "{} findings:\n{}", found.len(), found.join("\n"));
}

/// AU18's left item: at 1280×800 the panel verbs stand in the pane's view, not
/// under its scroll, and so does the step's own job, with the field it gives.
#[test]
fn aupage_panel_verbs_stand_clear_of_the_scroll() {
    let mut h = comic();
    window(&mut h, 1280.0, 800.0, 1.0);
    let pane = h.editor().pane_rects.iter().find(|(pane, ..)| *pane == Pane::Manga).expect("the pane is forward").1;
    for verb in ["Split across", "Split down", "Add panel", "Delete panel"] {
        let row = h.rect(verb);
        assert!(pane.contains_rect(row), "{verb} {row:?} of {pane:?}");
    }
    for verb in [vocab::ADD_BALLOON, vocab::ADD_TEXT, vocab::SPEED_LINES] {
        let row = h.rect(verb);
        assert!(pane.contains_rect(row), "{verb} {row:?} of {pane:?}");
    }
    // A press leaves everything where it was, the row and its field under it.
    h.click(vocab::ADD_BALLOON);
    settle(&mut h);
    let field = h.text_field_rect(words_field(&h));
    assert!(pane.contains_rect(field), "Words {field:?} of {pane:?}");
    assert!(pane.contains_rect(h.rect("Remove lettering")), "{pane:?}");
}

/// AU27's left item: `Done::Exported` reads the Mono latch through the facts,
/// the same state the Export step's latch writes.
#[test]
fn aupage_exported_reads_the_mono_latch() {
    let first = lessons::PROBED.tasks[0];
    let tasks = [Task { done: Done::Exported(1), step: Stage::Export, ..first }, Task { done: Done::Strokes(u32::MAX), ..first }];
    let tasks: &'static [Task] = Box::leak(Box::new(tasks));
    let mut h = comic();
    h.editor_mut().begin_lesson(Lesson { tasks, latch: Some(Latch::Mono), ..lessons::PROBED }, 0);
    settle(&mut h);
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    let task = |h: &EditorHarness| h.editor().lesson.as_ref().expect("a lesson stands").task;

    let dir = tempfile::tempdir().unwrap();
    export_pages(&mut h, dir.path());
    assert_eq!(task(&h), 0, "a colour page is off the latch");
    press(&mut h, vocab::MONO);
    let dir = tempfile::tempdir().unwrap();
    export_pages(&mut h, dir.path());
    assert_eq!(task(&h), 1, "the Export step's latch is the lesson's");
}

/// The starting values are this page's, not a video frame's: each verb stands
/// in the selected panel, and a second one a step on from the first.
#[test]
fn aupage_a_verb_lands_in_the_selected_panel() {
    let mut h = comic();
    let (_, panels) = manga::panels_of(h.state().cut()).expect("the template's panels");
    let (w, tall) = (h.state().engine.project.width as f32, h.state().engine.project.height as f32);
    let last = panels.len() - 1;
    h.state_mut().manga.selected = last;
    settle(&mut h);
    press(&mut h, vocab::ADD_BALLOON);
    let first = values(&h, the(&h, GenKind::Balloon));
    assert_eq!(manga::panel_at(&panels, [first.float("x") * w, first.float("y") * tall]), Some(last));
    press(&mut h, vocab::ADD_BALLOON);
    let ids = manga_tools::letters(h.state().cut());
    let second = values(&h, ids[1].0);
    assert!(second.float("x") > first.float("x") && second.float("y") > first.float("y"), "not on top of the first");
    let stroke = first.float("stroke_width");
    assert!(stroke >= 3.0, "{stroke} reads at Fit");
}

