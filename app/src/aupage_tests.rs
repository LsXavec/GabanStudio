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
    assert_eq!(kinds(&h), [GenKind::Balloon, GenKind::Text, GenKind::SpeedLines, GenKind::Balloon]);
    assert!(!h.has_text_containing("DANGER"), "{}", h.describe());
    assert!(h.state_mut().clip_to_panels());
    assert_eq!(kinds(&h), [GenKind::Balloon, GenKind::Text, GenKind::SpeedLines, GenKind::Balloon]);
    assert_eq!(h.state().page_hand_built(), None);
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
    assert_eq!(kinds(&h), [GenKind::Text, GenKind::Balloon]);
    assert_eq!(values(&h, the(&h, GenKind::Text)).text("text"), "tink");
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
    ];
    let mut h = comic();
    h.kit_mut().set_size(vec2(1600.0, 1400.0));
    press(&mut h, vocab::ADD_BALLOON);
    for chain in NAMED {
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
/// under its scroll.
#[test]
fn aupage_panel_verbs_stand_clear_of_the_scroll() {
    let mut h = comic();
    window(&mut h, 1280.0, 800.0, 1.0);
    let pane = h.editor().pane_rects.iter().find(|(pane, ..)| *pane == Pane::Manga).expect("the pane is forward").1;
    for verb in ["Split across", "Split down", "Add panel", "Delete panel"] {
        let row = h.rect(verb);
        assert!(pane.contains_rect(row), "{verb} {row:?} of {pane:?}");
    }
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

