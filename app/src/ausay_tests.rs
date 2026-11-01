//! AU7: the Say line teaches, Quick Start ticks on outcomes and no step is
//! silent (spec 8.2 to 8.5). The prefix is `ausay_`.

use anim_core::command::Command;
use anim_core::raster::{TILE, f16_bits_to_f32};
use eframe::egui;
use egui::{Rect, pos2, vec2};

use crate::canvas::EMPTY_CELL_LINE;
use crate::canvas::extpaper_tests::paint_texels;
use crate::config::{Action, UiConfig};
use crate::doc::{self, AppState};
use crate::measure::{self, Scope};
use crate::say::{self, Facts, Hint, Spoken, Verb, Words};
use crate::steps::ProjectKind;
use crate::testkit::EditorHarness;
use crate::welcome::{self, EXPORT_STEP, ShowMe};
use crate::workspace::Stage;
use crate::{Pane, Performed, plate, steps, visible_panes};

const GIF: &str = "Animated GIF";
// The lane's buttons, spoken apart (1.5 rule 9).
const SHOW_ME: &str = "Show me (say line)";
const FIX_IT: &str = "Fix it (say line)";
const LANE_GO_TO_DRAW: &str = "Go to Draw (say line)";

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

fn sized(h: &mut EditorHarness, w: f32, tall: f32) {
    h.kit_mut().set_size(vec2(w, tall));
    h.kit_mut().set_pixels_per_point(1.0);
    settle(h);
}

/// A shipped install at 1600×900, on the ready paper, hints on.
fn ready() -> EditorHarness {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    h
}

/// The same install after `An animation`: the rows a drawing never hears.
fn animation() -> EditorHarness {
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    h
}

/// A cut of six frames with one drawing, on frame `key`.
fn one_drawing_at(key: u32) -> AppState {
    let mut state = AppState::new_project("t", 640, 480, 24, 72.0);
    state.set_cut_length(6);
    state.goto(key);
    state.new_drawing_at_frame();
    paint_texels(&mut state, &[(10, 10)], [0.0, 0.0, 0.0, 1.0]);
    state.goto(0);
    state
}

/// `ready`, with the machine's last remark gone.
fn quiet(h: &mut EditorHarness) {
    h.wait(4.2);
    settle(h);
}

fn paper_middle(h: &EditorHarness) -> egui::Pos2 {
    let xf = h.editor().canvas.view_xform();
    xf.paper_to_screen(pos2(xf.paper.x / 2.0, xf.paper.y / 2.0))
}

/// A line drawn the way a hand draws it.
pub(crate) fn line(h: &mut EditorHarness) {
    let from = paper_middle(h);
    h.drag_path(&[from, from - vec2(30.0, 0.0), from - vec2(60.0, 10.0)]);
    settle(h);
}

/// What a raster stroke lands, at pen-up.
pub(crate) fn raster_line(h: &mut EditorHarness) {
    paint_texels(h.state_mut(), &[(10, 10), (11, 10), (12, 10)], [0.0, 0.0, 0.0, 1.0]);
    settle(h);
}

fn says(h: &EditorHarness, words: &str) -> bool {
    h.painted_text().iter().any(|w| w == words)
}

fn picked(h: &EditorHarness) -> Option<String> {
    h.editor().say.hint.as_ref().map(|s| s.words.clone())
}

fn add_drawing(h: &mut EditorHarness) {
    assert_ne!(h.perform(Action::AddDrawingAfter), Performed::Ignored);
    settle(h);
}

/// The document a person has after three poses, on the last of them.
fn three_poses(h: &mut EditorHarness) {
    raster_line(h);
    add_drawing(h);
    raster_line(h);
    add_drawing(h);
    raster_line(h);
}

fn at_step(stage: Stage, f: Facts) -> Facts {
    Facts { stage: Some(stage), ..f }
}

fn chord(_: Action) -> Option<String> {
    Some("Space".into())
}

/// Spec 8.2: a refusal, then the machine's remark, then the hint. A hint
/// waits behind both, and its action button with it.
#[test]
fn ausay_priority() {
    const HINT: &str = "Draw on the paper to begin.";
    let mut h = ready();
    quiet(&mut h);
    assert!(says(&h, HINT), "{:?}", h.painted_text());
    assert!(h.has(SHOW_ME), "the button stands with its hint");

    h.state_mut().status = "saved".into();
    settle(&mut h);
    assert!(says(&h, "saved") && !says(&h, HINT));
    assert!(!h.has(SHOW_ME), "and goes with it");

    let refusal = "refused — this layer is locked";
    h.state_mut().refuse(refusal);
    settle(&mut h);
    assert!(says(&h, refusal) && !says(&h, "saved") && !says(&h, HINT));

    quiet(&mut h);
    assert!(says(&h, HINT), "both dwelt: the hint is back: {:?}", h.painted_text());
    assert!(!says(&h, refusal) && !says(&h, "saved"));
    assert!(h.has(SHOW_ME));

    // The table's own order, read with no editor.
    let f = at_step(Stage::Layout, Facts { plan_tried: true, flat_pen: true, ..Facts::default() });
    let spoken = say::read(&f, &mut 0, None, chord).expect("a row applies");
    assert_eq!(spoken.words, say::PLAN_WORDS);
    let f = Facts { refused_twice: Some(doc::HELD_FRAME.into()), ..f };
    let f = Facts { plan_tried: false, ..f };
    assert_eq!(say::read(&f, &mut 0, None, chord).expect("a row").bit, Some(say::REFUSED_TWICE));
}

/// R3: the display order is the trade's, and a hint's sentence never sends
/// the person to a step before the one it speaks in.
#[test]
fn ausay_hints_never_point_backward() {
    let bars = [
        (ProjectKind::Drawing, vec!["Draw", "Color", "Effects", "Export"]),
        (ProjectKind::Animation, vec!["Plan", "Timing", "Color", "Effects", "Export"]),
        (ProjectKind::Comic, vec!["Manga", "Color", "Export"]),
    ];
    let (mut total, mut sent_back) = (0, 0);
    for (kind, trade) in bars {
        let bar = steps::bar(kind);
        let place = |word: &str| bar.iter().position(|s| s.label == word);
        let labels: Vec<&str> = bar.iter().map(|s| s.label).collect();
        assert_eq!(labels, trade, "the trade's order");
        let mut checked = 0;
        for hint in say::HINTS.iter().filter(|hint| hint.kinds.contains(&kind)) {
            let Words::Plain(words) = hint.words else { continue };
            let named = words.split(|c: char| !c.is_alphabetic()).filter_map(place).max();
            let speaks = hint.steps.iter().filter_map(|st| bar.iter().position(|s| s.stage == *st));
            for here in speaks {
                if let Some(there) = named {
                    assert!(there >= here, "{words:?} points back from {}", labels[here]);
                    checked += 1;
                }
            }
        }
        assert!(checked >= 1, "{kind:?}: the sentences that name a step: {checked}");
        total += checked;
        // `Go to` speaks on an empty step alone, and the step it names is the
        // first, where the work that step lacks is made.
        for row in say::EMPTY_STEPS.iter().filter(|row| bar.iter().any(|s| s.stage == row.stage)) {
            let f = Facts { stage: Some(row.stage), kind: Some(kind), ..Facts::default() };
            let spoken = say::empty_step(&f, false).expect("an empty step speaks");
            let word = spoken.verb.expect("its button").label(Some(kind)).strip_prefix("Go to ").expect("a step");
            assert_eq!(place(word), Some(0), "{:?} on {kind:?}", row.words);
            sent_back += 1;
        }
    }
    assert!(total >= 4, "the sentences that name a step, on every bar: {total}");
    assert!(sent_back >= 3, "the empty steps on a bar: {sent_back}");
    // The Quick Start chain speaks in its own order: each row needs what the
    // one before it taught.
    let chain = [say::NEW_PAPER, say::ADD_DRAWING, say::NEXT_POSE, say::TWO_DRAWINGS, say::PLAY, say::SHARE];
    let order: Vec<usize> = chain.iter().map(|b| say::HINTS.iter().position(|r| r.bit == *b).expect("a row")).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
}

/// A row's bit outlives the moment. Undoing the stroke does not bring the
/// first sentence back, and a held frame says so once.
#[test]
fn ausay_a_hint_shows_once() {
    let mut h = ready();
    quiet(&mut h);
    assert_eq!(picked(&h).as_deref(), Some("Draw on the paper to begin."));
    raster_line(&mut h);
    assert_ne!(picked(&h).as_deref(), Some("Draw on the paper to begin."));
    assert_ne!(h.rig().config.ui.hints_seen & say::NEW_PAPER, 0, "stored");
    h.state_mut().undo();
    settle(&mut h);
    assert!(!h.state().has_art(), "the art went");
    assert_ne!(picked(&h).as_deref(), Some("Draw on the paper to begin."), "the lesson stayed");

    // `passes`: shown, read, then the moment went.
    let mut h = EditorHarness::with_state_shipped(one_drawing_at(0));
    sized(&mut h, 1600.0, 900.0);
    quiet(&mut h);
    h.state_mut().view.frame = 3;
    settle(&mut h);
    let held = "This drawing stays on screen for 6 frames.";
    assert_eq!(picked(&h).as_deref(), Some(held));
    h.wait(say::READ_SECS);
    h.state_mut().view.frame = 0;
    settle(&mut h);
    assert_ne!(h.editor().say.seen & say::HELD, 0);
    h.state_mut().view.frame = 3;
    settle(&mut h);
    assert_ne!(picked(&h).as_deref(), Some(held), "once");
}

/// A held frame, one frame at a time: a frame of the harness is a quarter of
/// a second, so what happens here happens before anyone could read it.
fn on_frame(h: &mut EditorHarness, frame: u32) {
    h.state_mut().view.frame = frame;
    h.run_frames(2);
}

/// Spec 8.2 and AU7's list: a sentence is not used up unread. Playback runs
/// across a hold and says nothing, and a scrub past one leaves it to say.
#[test]
fn ausay_a_passing_row_waits_to_be_read() {
    const HELD: &str = "This drawing stays on screen for 6 frames.";
    let mut h = EditorHarness::with_state_shipped(one_drawing_at(0));
    sized(&mut h, 1600.0, 900.0);
    quiet(&mut h);
    let lane = picked(&h);
    assert_eq!(lane.as_deref(), Some("Next: add a drawing."));

    // With the sheet off a stop lands on the run's first frame (AU5): no hold.
    h.state_mut().view.playing = true;
    on_frame(&mut h, 3);
    h.state_mut().view.playing = false;
    h.run_frames(2);
    assert_eq!(h.state().view.frame, 0);
    assert_eq!(picked(&h), lane);

    // With it up, a stop stays where it lands.
    h.editor_mut().show_sheet();
    settle(&mut h);
    h.state_mut().view.playing = true;
    for frame in [1, 2, 3, 4, 5, 0, 3] {
        on_frame(&mut h, frame);
        assert_eq!(picked(&h), lane, "playback on frame {frame}");
        assert_eq!(h.rig().config.ui.hints_seen & say::HELD, 0, "unspent on frame {frame}");
    }
    h.state_mut().view.playing = false;
    on_frame(&mut h, 3);
    assert_eq!(picked(&h).as_deref(), Some(HELD), "stopped on a hold, it speaks");
    assert_eq!(h.editor().say.shown, Some(say::HELD), "on the lane");

    // Scrubbed past: on the lane, and gone before it could be read.
    on_frame(&mut h, 0);
    h.run_frames(1);
    assert_ne!(picked(&h).as_deref(), Some(HELD));
    assert_eq!(h.rig().config.ui.hints_seen & say::HELD, 0, "unread");
    on_frame(&mut h, 2);
    assert_eq!(picked(&h).as_deref(), Some(HELD), "so it still has it to say");
    // Coming back is a new reading, not the time since it first stood.
    h.wait(say::READ_SECS + 1.0);
    on_frame(&mut h, 0);
    h.run_frames(1);
    assert_ne!(h.rig().config.ui.hints_seen & say::HELD, 0, "it stood, so it was read");
    on_frame(&mut h, 3);
    assert_ne!(picked(&h).as_deref(), Some(HELD), "once");

    // A row back on the lane starts its reading over.
    let mut h = EditorHarness::with_state_shipped(one_drawing_at(0));
    sized(&mut h, 1600.0, 900.0);
    quiet(&mut h);
    on_frame(&mut h, 3);
    assert_eq!(h.editor().say.shown, Some(say::HELD));
    on_frame(&mut h, 0);
    h.wait(say::READ_SECS + 1.0);
    on_frame(&mut h, 3);
    on_frame(&mut h, 0);
    h.run_frames(1);
    assert_eq!(h.rig().config.ui.hints_seen & say::HELD, 0, "the time away was no reading");

    // The table's own word, for both rows that pass on a frame.
    let rows = [(Facts { twice: true, ..Facts::default() }, say::APPEARS_TWICE), (Facts { held: Some(6), ..Facts::default() }, say::HELD)];
    for (on, bit) in rows {
        let on = at_step(Stage::Drawing, on);
        let mut seen = 0;
        let playing = Facts { playing: true, stood: true, ..on.clone() };
        assert_ne!(say::read(&playing, &mut seen, Some(bit), chord).and_then(|s| s.bit), Some(bit), "playback hides it");
        assert_eq!(seen & bit, 0, "and spends nothing");
        let elsewhere = at_step(Stage::Drawing, Facts::default());
        let carried = Facts { playing: true, stood: true, ..elsewhere.clone() };
        say::read(&carried, &mut seen, Some(bit), chord);
        assert_eq!(seen & bit, 0, "playback carried it off");
        say::read(&elsewhere, &mut seen, Some(bit), chord);
        assert_eq!(seen & bit, 0, "it did not stand");
        say::read(&Facts { stood: true, ..elsewhere }, &mut seen, Some(bit), chord);
        assert_eq!(seen & bit, bit, "it stood, and the moment went");
    }
}

/// `N` is how long the person sees it: a cut shortened under the next key
/// ends first.
#[test]
fn ausay_a_held_sentence_counts_to_the_cut_end() {
    let mut state = one_drawing_at(0);
    state.set_cut_length(12);
    state.goto(8);
    state.new_drawing_at_frame();
    state.set_cut_length(6);
    state.goto(3);
    let mut h = EditorHarness::with_state_shipped(state);
    sized(&mut h, 1600.0, 900.0);
    quiet(&mut h);
    assert_eq!(h.state().exposure_here(), Some((0, Some(8))), "the next key stands past the end");
    assert_eq!(picked(&h).as_deref(), Some("This drawing stays on screen for 6 frames."));
}

/// Spec 8.5: off means no hints, no dots and no action button, and nothing
/// else on the screen moves.
#[test]
fn ausay_hints_off_changes_no_chrome() {
    let mut h = animation();
    quiet(&mut h);
    let counter = format!("Frame 1 of {}", h.state().frame_count());
    let keys = ["Add drawing (band)", counter.as_str()];
    let before: Vec<Rect> = keys.iter().map(|k| h.rect(k)).collect();
    let paper = h.editor().canvas.dbg_rect;
    let on = measure::census(&mut h);
    assert!(h.has(SHOW_ME));
    assert!(h.editor().say.dots_painted, "the place shows");

    assert_ne!(h.perform(Action::ToggleHints), Performed::Ignored);
    settle(&mut h);
    assert!(h.rig().config.ui.hints_off, "the shell stored it");
    assert_eq!(h.editor().is_on(Action::ToggleHints), Some(false));
    assert!(!h.has(SHOW_ME), "no action button");
    assert!(!h.editor().say.dots_painted, "no dots");
    assert!(!says(&h, "Draw on the paper to begin."), "no hint");
    let after: Vec<Rect> = keys.iter().map(|k| h.rect(k)).collect();
    assert_eq!(before, after, "the band stands");
    assert_eq!(paper, h.editor().canvas.dbg_rect, "and the paper");
    let off = measure::census(&mut h);
    assert_eq!(on.controls, off.controls + 1, "one button went: {on:?} {off:?}");
}

/// Spec 8.5: on again, the hints start again.
#[test]
fn ausay_hints_on_starts_again() {
    let mut h = animation();
    quiet(&mut h);
    raster_line(&mut h);
    assert_ne!(h.rig().config.ui.hints_seen, 0);
    h.perform(Action::ToggleHints);
    settle(&mut h);
    assert_ne!(h.rig().config.ui.hints_seen, 0, "off keeps what was taught");
    assert_eq!(picked(&h), None);
    h.perform(Action::ToggleHints);
    settle(&mut h);
    assert!(!h.rig().config.ui.hints_off);
    assert_eq!(h.rig().config.ui.hints_seen, 0, "on clears it");
    assert_eq!(picked(&h).as_deref(), Some("Next: add a drawing."));

    // The Settings row goes through the same switch.
    let mut ui = UiConfig { hints_seen: 7, hints_off: true, ..UiConfig::default() };
    ui.set_hints(true);
    assert_eq!((ui.hints_off, ui.hints_seen), (false, 0));
    ui.hints_seen = 7;
    ui.set_hints(true);
    assert_eq!(ui.hints_seen, 7, "already on: nothing starts again");

    // A row that passes: the one last on the lane is not said already.
    let mut h = EditorHarness::with_state_shipped(one_drawing_at(0));
    sized(&mut h, 1600.0, 900.0);
    quiet(&mut h);
    h.state_mut().view.frame = 3;
    settle(&mut h);
    h.wait(say::READ_SECS);
    assert_eq!(h.editor().say.shown, Some(say::HELD));
    h.perform(Action::ToggleHints);
    settle(&mut h);
    h.state_mut().view.frame = 0;
    settle(&mut h);
    h.perform(Action::ToggleHints);
    settle(&mut h);
    assert_eq!(h.rig().config.ui.hints_seen & say::HELD, 0, "back on, it was never read");
    h.state_mut().view.frame = 3;
    settle(&mut h);
    assert_eq!(picked(&h).as_deref(), Some("This drawing stays on screen for 6 frames."));

    // The canvas line: a stroke answers it only once it stood again.
    let mut h = empty_cell();
    assert!(h.editor().canvas.dbg_empty_line);
    h.perform(Action::ToggleHints);
    settle(&mut h);
    assert!(!h.editor().canvas.dbg_empty_line, "off, it goes");
    h.state_mut().goto(3);
    settle(&mut h);
    h.perform(Action::ToggleHints);
    settle(&mut h);
    raster_line(&mut h);
    assert_eq!(h.rig().config.ui.hints_seen & say::EMPTY_CELL, 0, "it has not stood since");
    h.state_mut().goto(0);
    settle(&mut h);
    assert!(h.editor().canvas.dbg_empty_line, "so it comes back");
}

/// Spec 8.5: one switch, in Help ▸ Hints and in Settings. A frozen boot has it
/// off, so a suite's chrome holds still; a shipped one has it on.
#[test]
fn ausay_hints_switch_stands_in_both_places() {
    assert_eq!(Action::ToggleHints.label(), "Hints");
    let mut h = ready();
    quiet(&mut h);
    assert!(h.editor().say.hint.is_some());
    h.click("help");
    h.click("Hints");
    settle(&mut h);
    assert!(h.rig().config.ui.hints_off, "the menu row turned it off");
    assert_eq!(picked(&h), None);
    h.click("help");
    h.click("Hints");
    settle(&mut h);
    assert!(!h.rig().config.ui.hints_off, "and on");
    assert!(picked(&h).is_some());

    // The row Settings draws: the page saves, and a test has no data dir.
    use egui_kittest::kittest::Queryable;
    let start = UiConfig { hints_seen: 7, ..UiConfig::default() };
    let mut kit = egui_kittest::Harness::new_ui_state(
        |ui, config: &mut UiConfig| {
            crate::config::hints_row(ui, config);
        },
        start,
    );
    kit.run();
    kit.get_by_label("Hints").click();
    kit.run();
    assert!(kit.state().hints_off, "the row turned it off");
    assert_eq!(kit.state().hints_seen, 7, "off keeps what was taught");
    kit.get_by_label("Hints").click();
    kit.run();
    assert_eq!((kit.state().hints_off, kit.state().hints_seen), (false, 0), "on clears it");

    // Every frozen boot holds them off.
    let frozen = [
        EditorHarness::new(),
        EditorHarness::with_state(AppState::new_project("t", 64, 48, 24, 72.0)),
        EditorHarness::with_dir(AppState::new_project("t", 64, 48, 24, 72.0), tempfile::tempdir().unwrap()),
    ];
    for h in &frozen {
        assert!(h.rig().config.ui.hints_off);
        assert!(h.editor().say.off, "the editor reads it");
        assert_eq!(picked(h), None);
    }
    assert!(!ready().rig().config.ui.hints_off, "a shipped boot has them on");
}

/// Spec 8.2: a hint whose verb has no binding is not shown.
#[test]
fn ausay_hint_with_no_binding_is_not_shown() {
    let mut h = ready();
    three_poses(&mut h);
    quiet(&mut h);
    let words = picked(&h).expect("a sentence");
    assert!(words.starts_with("Press ") && words.ends_with(" to play."), "{words}");
    let chord_words = crate::menus::chord_label(&h.ctx(), Action::PlayPause).expect("bound");
    assert_eq!(words, format!("Press {chord_words} to play."), "the live chord");

    let binds = &mut h.rig_mut().config.keybinds;
    binds.iter_mut().filter(|b| b.action == Action::PlayPause).for_each(|b| {
        b.chord = None;
        b.also.clear();
    });
    settle(&mut h);
    assert_eq!(crate::menus::chord_label(&h.ctx(), Action::PlayPause), None);
    assert_eq!(picked(&h), None, "unbound: the row does not show");
    assert!(!h.has(SHOW_ME), "nor its button");
}

/// Spec 8.3: ticks are outcomes. Looking at a step, asking for Help or
/// opening a window ticks nothing.
#[test]
fn ausay_quick_start_ticks_on_outcomes() {
    let mut h = ready();
    h.perform(Action::QuickStart);
    settle(&mut h);
    h.perform(Action::QuickStart);
    h.editor_mut().enter_room(Stage::Edit);
    settle(&mut h);
    h.editor_mut().enter_room(Stage::Drawing);
    settle(&mut h);
    assert_eq!(h.editor().quick_start, [false; 5], "nothing was done yet");

    raster_line(&mut h);
    assert_eq!(h.editor().quick_start, [true, false, false, false, false]);
    add_drawing(&mut h);
    assert_eq!(h.editor().quick_start, [true, true, false, false, false]);
    assert!(h.state().view.frame >= 1);
    raster_line(&mut h);
    assert_eq!(h.editor().quick_start, [true, true, true, false, false]);
    h.state_mut().view.playing = true;
    settle(&mut h);
    h.state_mut().view.playing = false;
    settle(&mut h);
    assert_eq!(h.editor().quick_start, [true, true, true, true, false]);
    // A line drawn for `Check your pen` is that check's.
    let mut h = ready();
    h.editor_mut().say.check_line = true;
    raster_line(&mut h);
    assert_eq!(h.editor().quick_start, [false; 5]);
    assert_eq!(EXPORT_STEP, 4, "the fifth slot");
    assert_eq!(EXPORT_STEP, h.editor().quick_start.len() - 1);
}

/// Spec 8.3: the stroke signal is one signal.
#[test]
fn ausay_vector_and_raster_strokes_tick_alike() {
    let (mut vector, mut raster) = (ready(), ready());
    line(&mut vector);
    raster_line(&mut raster);
    assert_eq!(vector.state().strokes_done, 1);
    assert_eq!(raster.state().strokes_done, 1);
    assert_eq!(vector.editor().quick_start, raster.editor().quick_start);
    assert!(vector.editor().quick_start[0]);
    add_drawing(&mut vector);
    add_drawing(&mut raster);
    line(&mut vector);
    raster_line(&mut raster);
    assert_eq!(vector.editor().quick_start, raster.editor().quick_start);
    assert!(vector.editor().quick_start[2], "a finished stroke at frame 1 or later");
    assert_eq!(vector.rig().config.ui.hints_seen, raster.rig().config.ui.hints_seen);
}

fn texel_alpha(state: &AppState, x: i32, y: i32) -> f32 {
    let t = TILE as i32;
    let tiles = state.active_layer_tiles().expect("a layer");
    let Some(tile) = tiles.get(&(x.div_euclid(t), y.div_euclid(t))) else { return 0.0 };
    let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4 + 3) as usize;
    f16_bits_to_f32(tile.rgba[i])
}

/// Spec 8.3: a ring, by the raster route, as one undo, and it fires the
/// stroke signal. On an empty cell the drawing is made first.
#[test]
fn ausay_show_me_draws_one_undoable_circle() {
    let mut h = ready();
    quiet(&mut h);
    assert!(!h.state().has_art());
    let drawings = h.state().cut().drawings.len();
    let depth = h.state().engine.undo_depth_by(None);
    h.click(SHOW_ME);
    settle(&mut h);
    assert!(h.state().has_art(), "the circle landed");
    assert_eq!(h.state().strokes_done, 1, "as a stroke");
    assert!(h.editor().quick_start[welcome::FIRST_STROKE]);
    assert_eq!(h.state().engine.undo_depth_by(None), depth + 1, "one undo");

    let project = &h.state().engine.project;
    let (w, tall) = (project.width as f32, project.height as f32);
    let radius = w.min(tall) / 6.0;
    let (cx, cy) = (w * 0.3, tall * 0.5);
    assert!(texel_alpha(h.state(), (cx + radius) as i32, cy as i32) > 0.5, "on the ring");
    assert!(texel_alpha(h.state(), (cx - radius) as i32, cy as i32) > 0.5, "all the way round");
    assert_eq!(texel_alpha(h.state(), cx as i32, cy as i32), 0.0, "and hollow");

    h.state_mut().undo();
    settle(&mut h);
    assert!(!h.state().has_art(), "undo takes it all");
    assert_eq!(h.state().cut().drawings.len(), drawings, "and the drawing it made");
}

/// Show me's ring stands around the first choice. It takes the place of AU7's
/// sentence, which named two buttons in the queue.
fn ringed(h: &EditorHarness) -> bool {
    let ring = h.state().render_queue.panel.ring_at;
    ring.is_some_and(|ring| ring.contains_rect(h.rect(GIF)) && h.rect(GIF).contains_rect(ring.shrink(5.0)))
}

/// Show me does slots 0 to 3, AU7's four, and slot 4 rings the choice to
/// press, which stays until a file answers it. The fifth has its own test,
/// `auexport_show_me_is_five_of_five`; this name stands in AU7's list.
#[test]
fn ausay_show_me_is_four_of_five() {
    let kinds: Vec<ShowMe> = welcome::STEPS.iter().map(|s| s.show_me).collect();
    assert_eq!(kinds.len(), 5);
    assert!(matches!(kinds[0], ShowMe::Circle));
    assert!(matches!(kinds[1], ShowMe::Perform(Action::AddDrawingAfter)));
    assert!(matches!(kinds[2], ShowMe::NextCircle));
    assert!(matches!(kinds[3], ShowMe::Play));
    assert!(matches!(kinds[EXPORT_STEP], ShowMe::Export));

    let mut h = animation();
    assert_ne!(h.editor_mut().show_me(0), Performed::Ignored);
    settle(&mut h);
    assert_eq!(h.editor().quick_start, [true, false, false, false, false]);
    assert_ne!(h.editor_mut().show_me(1), Performed::Ignored);
    settle(&mut h);
    assert_eq!(h.editor().quick_start, [true, true, false, false, false]);
    assert_ne!(h.editor_mut().show_me(2), Performed::Ignored);
    settle(&mut h);
    assert_eq!(h.editor().quick_start, [true, true, true, false, false]);
    assert_eq!(h.state().strokes_done, 2, "two circles");
    assert_eq!(h.state().cut().drawings.len(), 2, "two drawings already play");
    assert_ne!(h.editor_mut().show_me(3), Performed::Ignored);
    settle(&mut h);
    assert!(h.state().view.playing, "it plays");
    assert_eq!(h.editor().quick_start, [true, true, true, true, false]);
    h.state_mut().view.playing = false;
    settle(&mut h);

    assert_ne!(h.editor_mut().show_me(EXPORT_STEP), Performed::Ignored);
    settle(&mut h);
    assert!(visible_panes(&h.editor().dock).contains(&Pane::RenderQueue));
    assert!(!h.editor().quick_start[EXPORT_STEP], "no file yet");
    quiet(&mut h);
    assert!(ringed(&h), "the ring holds the place");
    h.wait(30.0);
    assert!(ringed(&h), "and keeps it");

    // Next circle from the first frame goes through Add drawing.
    let mut h = animation();
    assert_ne!(h.editor_mut().show_me(2), Performed::Ignored);
    settle(&mut h);
    assert!(h.state().view.frame >= 1);
    assert!(h.editor().quick_start[2]);
}

/// One viewport, no dialog asked of the App and no float over the paper.
#[track_caller]
fn no_window(h: &EditorHarness) {
    let windows = &h.kit().output().viewport_output;
    assert!(windows.len() == 1 && windows.contains_key(&egui::ViewportId::ROOT));
    assert!(h.rig().requests.is_empty(), "{:?}", h.rig().requests);
    assert!(!h.editor().float_canvas.open);
}

/// A queue that can render here, with no encoder and into `dir`.
fn queue_ready(h: &mut EditorHarness, dir: &std::path::Path) {
    h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
    h.state_mut().render_queue.draft.out = dir.display().to_string();
}

/// Spec 8.2 and 8.3: Show me alone walks a new document to the Export step,
/// and the lane has the next thing to say at every step until the fifth tick.
/// The ring takes it from there, on the choice to press. Nothing on the way
/// opens a window.
#[test]
fn ausay_show_me_walks_the_whole_chain() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = animation();
    queue_ready(&mut h, dir.path());
    let play = crate::menus::chord_label(&h.ctx(), Action::PlayPause).expect("bound");
    let walk = [
        ("Draw on the paper to begin.".to_string(), [true, false, false, false, false]),
        ("Next: add a drawing.".to_string(), [true, true, false, false, false]),
        ("The ghost is your last drawing. Draw the next pose.".to_string(), [true, true, true, false, false]),
        (format!("Add another drawing, or press {play} to play."), [true, true, true, true, false]),
    ];
    for (words, ticks) in walk {
        quiet(&mut h);
        assert_eq!(picked(&h).as_deref(), Some(words.as_str()));
        assert!(says(&h, &words), "{:?}", h.painted_text());
        h.click(SHOW_ME);
        settle(&mut h);
        no_window(&h);
        assert_eq!(h.editor().quick_start, ticks, "after {words}");
        assert!(picked(&h).is_some(), "the lane goes on from {words}");
    }
    assert!(h.state().view.playing, "the two-drawing row plays");
    quiet(&mut h);
    assert_eq!(picked(&h).as_deref(), Some("Ready to share? Go to Export."), "while it plays");
    h.state_mut().view.playing = false;
    settle(&mut h);
    quiet(&mut h);
    assert_eq!(picked(&h).as_deref(), Some("Ready to share? Go to Export."), "and after");
    h.click(SHOW_ME);
    settle(&mut h);
    no_window(&h);
    quiet(&mut h);
    assert!(ringed(&h));
    assert!(says(&h, GIF), "{:?}", h.painted_text());
    assert!(!h.editor().quick_start[EXPORT_STEP], "no file yet");
    h.click(GIF);
    settle(&mut h);
    quiet(&mut h);
    assert!(!h.editor().quick_start[EXPORT_STEP], "a dialog waiting is no file");
    h.answer_export(&dir.path().join("ball.gif"));
    h.run_export_to_end();
    settle(&mut h);
    assert!(h.editor().quick_start[EXPORT_STEP], "the fifth tick");
    assert_ne!(h.rig().config.ui.hints_seen & say::SHARE, 0, "the file answered Share's row");
    assert!(!ringed(&h));
    no_window(&h);
}

/// Show me's place on Export stands until a press answers it, whatever else
/// waits to be said; asked again after a file, it comes again.
#[test]
fn ausay_export_ring_stays_until_a_press() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = ready();
    queue_ready(&mut h, dir.path());
    three_poses(&mut h);
    quiet(&mut h);
    let press = picked(&h).expect("a sentence");
    assert!(press.starts_with("Press ") && press.ends_with(" to play."), "never played: {press}");
    assert_ne!(h.editor_mut().show_me(EXPORT_STEP), Performed::Ignored);
    settle(&mut h);
    quiet(&mut h);
    assert!(ringed(&h), "over the Play row");
    h.state_mut().set_cut_length(6);
    h.state_mut().view.frame = 4;
    assert!(h.state().own_key_drawing().is_none(), "a held frame");
    h.wait(30.0);
    settle(&mut h);
    assert!(ringed(&h), "over a frame's rows, and no timer takes it");

    h.click(GIF);
    settle(&mut h);
    h.answer_export(&dir.path().join("ball.gif"));
    h.run_export_to_end();
    settle(&mut h);
    assert_ne!(h.rig().config.ui.hints_seen & say::SHARE, 0);
    quiet(&mut h);
    assert!(!h.state().render_queue.busy());
    assert!(!ringed(&h), "answered");

    assert_ne!(h.editor_mut().show_me(EXPORT_STEP), Performed::Ignored);
    settle(&mut h);
    assert!(ringed(&h), "again: {:?}", h.painted_text());

    // It stands around what is on screen, whole, with the pane's room for it.
    for (w, tall) in [(1600.0, 900.0), (1280.0, 800.0)] {
        let mut h = ready();
        sized(&mut h, w, tall);
        three_poses(&mut h);
        assert_ne!(h.editor_mut().show_me(EXPORT_STEP), Performed::Ignored);
        settle(&mut h);
        let pane = h.editor().pane_rects.iter().find(|(p, ..)| *p == Pane::RenderQueue).map(|(_, r, _)| *r);
        let pane = pane.expect("the queue");
        assert!(pane.contains_rect(h.rect(GIF)), "{w}: {:?} in {pane:?}", h.rect(GIF));
        let ring = h.state().render_queue.panel.ring_at.expect("a ring");
        assert!(pane.contains_rect(ring), "{w}: {ring:?} in {pane:?}");
    }
}

/// Spec 8.3: the circle goes where a stroke's commit goes, and in a session
/// that is the mirror a fill rides: its batch lands the same art on a guest's
/// copy of the document. This reads the log the host streams; no two machines
/// ran it. A guest with no file yet gets the pen's own refusal.
#[test]
fn ausay_a_circle_in_a_session_rides_the_mirror_a_fill_rides() {
    fn paints(batches: &[Vec<Command>]) -> usize {
        batches.iter().flatten().filter(|c| matches!(c, Command::PaintTiles { .. })).count()
    }
    let mut h = ready();
    quiet(&mut h);
    let mut guest = anim_core::Engine::new("guest");
    guest.project = h.state().engine.project.clone();
    h.state_mut().engine.mirror_log = true;
    assert_ne!(h.editor_mut().show_me(0), Performed::Ignored);
    settle(&mut h);
    let circle = h.state_mut().engine.drain_applied();
    assert_eq!(paints(&circle), 1, "one PaintTiles");
    for batch in &circle {
        guest.apply_mirror(batch).expect("the batch fits the guest's copy");
    }
    assert!(guest.project.scenes == h.state().engine.project.scenes, "the guest has what the host has");
    assert!(h.state().has_art(), "and it is the circle");

    let drawing = h.state().current_drawing().expect("the circle's drawing");
    let layer = h.state().active_layer_id().expect("an armed layer");
    h.state_mut().commit_fill("fill", drawing, layer, Vec::new());
    assert_eq!(paints(&h.state_mut().engine.drain_applied()), 1, "the fill's log is the same one");

    let mut h = ready();
    quiet(&mut h);
    h.editor_mut().canvas.is_guest = true;
    assert_eq!(h.editor_mut().show_me(0), Performed::Ignored);
    assert_eq!(h.state().refusal, "refused — the host's file is still arriving");
    assert!(!h.state().has_art());
}

/// A cut with one drawing, and the cursor on a cell before it.
fn empty_cell() -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(one_drawing_at(3));
    sized(&mut h, 1600.0, 900.0);
    h
}

/// Spec 8.4: the gate never reads `raster`, so a real raster canvas shows the
/// line. CPU: the frame turns `raster` off with no paint engine, so the gate
/// is asked with it on.
#[test]
fn ausay_empty_cell_line_paints_on_a_raster_canvas() {
    let mut h = empty_cell();
    assert!(h.editor().canvas.dbg_empty_line, "the frame painted it");
    assert!(says(&h, EMPTY_CELL_LINE));
    h.editor_mut().canvas.raster = true;
    let e = h.editor();
    assert!(e.canvas.empty_line_due(&e.state), "a raster canvas reads the same gate");
    let source = include_str!("canvas.rs");
    let gate = source.split("fn empty_line_due").nth(1).expect("the gate");
    let body = gate.split("\n    }").next().expect("its body");
    assert!(!body.contains("raster"), "{body}");
    // Text is INK2 on PAPER, 15 pt.
    assert_eq!(crate::canvas::EMPTY_CELL_TEXT, 15.0);
}

/// Spec 8.4: on a blank document the Say line says it instead, so the first
/// screen does not carry the sentence twice. The first stroke on an empty
/// cell sets its bit.
#[test]
fn ausay_empty_cell_line_waits_for_a_first_drawing() {
    let mut h = ready();
    quiet(&mut h);
    assert!(h.state().cut().drawings.is_empty(), "a blank document");
    assert!(h.state().current_drawing().is_none());
    assert!(!h.editor().canvas.dbg_empty_line);
    assert!(!says(&h, EMPTY_CELL_LINE));

    let mut h = empty_cell();
    assert!(h.editor().canvas.dbg_empty_line);
    assert_eq!(h.editor().say.seen & say::EMPTY_CELL, 0);
    raster_line(&mut h);
    assert_ne!(h.editor().say.seen & say::EMPTY_CELL, 0, "a finished stroke sets it");
    assert!(!says(&h, EMPTY_CELL_LINE));
    assert!(!h.editor().canvas.dbg_empty_line);

    // A drawing with no art yet: the lane is saying it, so the paper does not
    // (spec 8.4: a screen carries a sentence once). Taught, the paper has it.
    let mut state = AppState::new_project("t", 640, 480, 24, 72.0);
    state.set_cut_length(6);
    state.goto(3);
    state.new_drawing_at_frame();
    state.goto(0);
    let mut h = EditorHarness::with_state_shipped(state);
    sized(&mut h, 1600.0, 900.0);
    quiet(&mut h);
    assert!(!h.state().has_art() && h.state().current_drawing().is_none());
    assert!(says(&h, "Draw on the paper to begin."));
    assert!(!h.editor().canvas.dbg_empty_line && !says(&h, EMPTY_CELL_LINE), "{:?}", h.painted_text());
    h.rig_mut().config.ui.hints_seen |= say::NEW_PAPER;
    settle(&mut h);
    assert!(h.editor().canvas.dbg_empty_line && says(&h, EMPTY_CELL_LINE), "{:?}", h.painted_text());
    assert!(!says(&h, "Draw on the paper to begin."));
}

/// Spec 8.3: an outcome happens in a document. Another one's counts are no
/// stroke, whichever way they differ from the last one's.
#[test]
fn ausay_another_document_is_no_outcome() {
    let mut h = empty_cell();
    assert!(h.editor().canvas.dbg_empty_line);
    let ctx = h.kit_mut().ctx.clone();
    h.editor_mut().go(crate::Leave::Sample, &ctx);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, 0, "the document was replaced");
    assert_eq!(h.editor().say.seen & say::EMPTY_CELL, 0, "opening is no stroke");
    raster_line(&mut h);
    assert_ne!(h.editor().say.seen & say::EMPTY_CELL, 0, "a stroke in it counts");
}

/// A blank document says something in every step, hints on or off; and a
/// stroke tried on the Plan preview always gets an answer.
#[test]
fn ausay_no_step_is_silent() {
    let empty = [
        (Stage::Layout, "Nothing here yet."),
        (Stage::Drawing, "Draw on the paper to begin."),
        (Stage::Edit, "Add a second drawing to set timing."),
        (Stage::InkPaint, "Nothing to color yet."),
        (Stage::Composite, "Nothing to add effects to yet."),
        (Stage::Export, crate::export_step::NO_ART),
    ];
    for off in [false, true] {
        for (stage, words) in empty {
            if off && stage == Stage::Drawing {
                continue; // A hint, and the only sentence off silences.
            }
            let mut h = ready();
            // Today's Plan preview, which only the seam reaches until Plan's
            // own part replaces it.
            h.editor_mut().unset_kind();
            if off {
                h.perform(Action::ToggleHints);
                settle(&mut h);
            }
            h.editor_mut().enter_room(stage);
            settle(&mut h);
            quiet(&mut h);
            assert!(says(&h, words), "{stage:?} off={off}: {:?}", h.painted_text());
            // The Plan preview's and the Export panel's button are the pane's
            // own, and off has no action button.
            let button = match stage {
                Stage::Layout | Stage::Export => h.has(say::GO_TO_DRAW),
                Stage::Drawing => !h.has(LANE_GO_TO_DRAW),
                _ => h.has(LANE_GO_TO_DRAW) != off,
            };
            assert!(button, "{stage:?} off={off}");
        }
    }

    // Plan, a stroke attempt: the hint first, then the same sentence as a
    // remark, every time.
    for off in [false, true] {
        let mut h = in_plan(off);
        let at = plan_preview(&h);
        for _ in 0..3 {
            h.drag_path(&[at, at + vec2(30.0, 0.0), at + vec2(60.0, 10.0)]);
            settle(&mut h);
            assert!(says(&h, say::PLAN_WORDS), "off={off}: {:?}", h.painted_text());
            quiet(&mut h);
        }
    }
}

/// A shipped editor on the Plan preview, its remarks gone.
fn in_plan(off: bool) -> EditorHarness {
    let mut h = ready();
    h.editor_mut().unset_kind();
    if off {
        h.perform(Action::ToggleHints);
        settle(&mut h);
    }
    h.editor_mut().enter_room(Stage::Layout);
    settle(&mut h);
    quiet(&mut h);
    h
}

/// A place on the preview clear of its empty state's button.
fn plan_preview(h: &EditorHarness) -> egui::Pos2 {
    let preview = h.editor().pane_rects.iter().find(|(p, ..)| *p == Pane::Canvas);
    preview.map(|(_, r, _)| r.center() - vec2(0.0, 80.0)).expect("the preview")
}

/// A stroke tried is the drawing hand's drag. A tap, a double-click or a drag
/// with another button is no attempt to draw, and asks nothing.
#[test]
fn ausay_plan_hears_only_a_stroke() {
    let mut h = in_plan(false);
    let at = plan_preview(&h);
    let quiet_lane = picked(&h);
    h.click_pos(at);
    settle(&mut h);
    h.click_pos(at);
    h.click_pos(at);
    settle(&mut h);
    h.right_click_pos(at);
    settle(&mut h);
    h.middle_click_pos(at);
    settle(&mut h);
    for button in [egui::PointerButton::Secondary, egui::PointerButton::Middle] {
        let event = |pos, pressed| egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE };
        h.event(egui::Event::PointerMoved(at));
        h.event(event(at, true));
        h.run_frames(2);
        h.event(egui::Event::PointerMoved(at + vec2(60.0, 10.0)));
        h.run_frames(2);
        h.event(event(at + vec2(60.0, 10.0), false));
        settle(&mut h);
    }
    assert_eq!(picked(&h), quiet_lane, "no attempt");
    assert!(!says(&h, say::PLAN_WORDS), "{:?}", h.painted_text());

    h.drag_path(&[at, at + vec2(30.0, 0.0), at + vec2(60.0, 10.0)]);
    settle(&mut h);
    assert_eq!(picked(&h).as_deref(), Some(say::PLAN_WORDS), "the drawing hand's drag is one");
    // Pressed at once, the row was read: it needs no time on the lane.
    h.click(LANE_GO_TO_DRAW);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert_ne!(h.rig().config.ui.hints_seen & say::PLAN, 0, "the press answered it");
}

/// Spec 8.4's Result: its sentence, and the button that answers it.
#[test]
fn ausay_result_empty_state_builds_the_composite() {
    let mut h = EditorHarness::with_state_shipped(one_drawing_at(0));
    sized(&mut h, 1600.0, 900.0);
    h.editor_mut().enter_room(Stage::Composite);
    settle(&mut h);
    assert!(h.state().cut().graph.output.is_none(), "no output yet");
    assert!(says(&h, crate::viewer::NOTHING_TO_SHOW), "{:?}", h.painted_text());
    h.click("Set up standard composite");
    settle(&mut h);
    assert!(h.state().cut().graph.output.is_some(), "the button built it");
    assert!(!says(&h, crate::viewer::NOTHING_TO_SHOW), "{:?}", h.painted_text());
}

/// Spec 8.4's Plan preview: its sentence, and the button that takes the
/// person to the paper.
#[test]
fn ausay_plan_empty_state_goes_to_draw() {
    let mut h = in_plan(false);
    assert!(says(&h, crate::viewer::NOTHING_HERE), "{:?}", h.painted_text());
    h.click(say::GO_TO_DRAW);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
}

/// Spec 8.4's Motion curves, with nothing keyed.
#[test]
fn ausay_curves_empty_state_says_what_fills_it() {
    let mut h = ready();
    h.perform(Action::ShowCurves);
    settle(&mut h);
    assert!(says(&h, "Animate a value to see its curve here"), "{:?}", h.painted_text());
}

/// Spec 8.4's Check paint, before a check has run.
#[test]
fn ausay_check_paint_empty_state_says_what_fills_it() {
    let mut h = ready();
    h.perform(Action::ShowPaintCheck);
    settle(&mut h);
    assert!(says(&h, "Run a check to find unpainted gaps."), "{:?}", h.painted_text());
}

/// Spec 8.4's Recent, on an install that has opened nothing.
#[test]
fn ausay_recent_empty_state_says_nothing_yet() {
    let mut h = ready();
    h.click("file");
    h.hover("Open Recent ⏵");
    settle(&mut h);
    assert!(says(&h, "Nothing yet"), "{:?}", h.painted_text());
}

/// Spec AU7's list: 4 false sentences to 0, and the two stale ones.
#[test]
fn ausay_false_copy_is_gone() {
    for (file, source) in sources() {
        for gone in [
            "theme, UI scale, pen and shortcuts",
            "fixed once the cut is created",
            "wire an Output node",
            "latch EDIT MODEL",
            "new project — draw on the canvas",
        ] {
            assert!(!shipped(&source).contains(gone), "{file}: {gone}");
        }
    }
    // The sentence that stands in names a control on screen.
    assert!(crate::palette_panel::NO_CHARACTERS.contains("edit model"));
    assert!(crate::palette_panel::NO_CHARACTERS.contains("+ character"));
    // And the status a new project starts with is no teaching at all.
    assert_eq!(AppState::new_project("t", 64, 48, 24, 72.0).status, "");
}

/// AU7's list: the empty board's sentence is true where it stands: the latch
/// it names is in the pane with it, then the button, and the board that comes
/// of it keeps both on the pane's own screen.
#[test]
fn ausay_empty_palette_names_what_is_on_screen() {
    fn palette(h: &EditorHarness) -> Rect {
        h.editor().pane_rects.iter().find(|(p, ..)| *p == Pane::Palette).map(|(_, r, _)| *r).expect("the board")
    }
    #[track_caller]
    fn on_screen(h: &EditorHarness, name: &str) {
        let (pane, control) = (palette(h), h.rect(name));
        assert!(pane.contains_rect(control), "{name}: {control:?} in {pane:?}");
    }
    for (w, tall) in [(1600.0, 900.0), (1280.0, 800.0)] {
        let mut h = ready();
        sized(&mut h, w, tall);
        h.editor_mut().enter_room(Stage::InkPaint);
        settle(&mut h);
        assert!(h.state().palettes.characters.is_empty());
        assert!(says(&h, crate::palette_panel::NO_CHARACTERS), "{:?}", h.painted_text());
        on_screen(&h, "edit model");
        h.click("edit model");
        settle(&mut h);
        on_screen(&h, "+ character");
        h.state_mut().palette_new_name = "Ana".into();
        h.click("+ character");
        settle(&mut h);
        assert_eq!(h.state().palettes.characters.len(), 1, "the sentence's two steps made one");
        assert!(!says(&h, crate::palette_panel::NO_CHARACTERS));
        on_screen(&h, "edit model");
        on_screen(&h, "+ character");
        on_screen(&h, "REMOVE CHARACTER");
    }
}

/// Spec 8.2 and AU7's list: a teaching sentence stays until it is answered,
/// and then stops.
#[test]
fn ausay_teaching_sentence_stays_until_answered() {
    const HINT: &str = "Draw on the paper to begin.";
    let mut h = animation();
    quiet(&mut h);
    for _ in 0..3 {
        h.wait(120.0);
        assert!(says(&h, HINT), "no timer takes it");
    }
    raster_line(&mut h);
    quiet(&mut h);
    assert!(!says(&h, HINT), "the stroke answered it: {:?}", h.painted_text());
    assert!(says(&h, "Next: add a drawing."));
    add_drawing(&mut h);
    quiet(&mut h);
    assert!(says(&h, "The ghost is your last drawing. Draw the next pose."), "{:?}", h.painted_text());
    h.wait(120.0);
    assert!(says(&h, "The ghost is your last drawing. Draw the next pose."));
}

/// Every literal in the shipped sources that a refusal can say.
fn refusals() -> Vec<String> {
    let mut found = vec![
        doc::HELD_FRAME.to_string(),
        doc::BLANK_FRAME.to_string(),
        doc::FINISH_STROKE.to_string(),
        doc::OUTSIDE_PAPER.to_string(),
    ];
    for (_, source) in sources() {
        let code = shipped(&source);
        let bare = code.match_indices("refuse(\"").map(|(at, _)| at + "refuse(\"".len());
        let named = code.match_indices("\"refused — ").map(|(at, _)| at + 1);
        for at in bare.chain(named) {
            let rest = &code[at..];
            let words = &rest[..rest.find('"').expect("closed")];
            if !words.contains('\\') {
                found.push(filled_in(words));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// A format string as it reads with a short name in it.
fn filled_in(words: &str) -> String {
    let mut out = String::new();
    let mut rest = words;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        out.push_str("Line 2");
        let close = rest[open..].find('}').expect("closed");
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// The app's sources the person meets: every file past the test-only ones.
pub(crate) fn sources() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = std::fs::read_dir(dir).expect("the sources").flatten();
    let named = files.map(|f| (f.file_name().to_string_lossy().to_string(), f.path()));
    let ships = |(name, path): &(String, std::path::PathBuf)| {
        let test_only = name.ends_with("_tests.rs") || ["testkit.rs", "measure.rs", "uishot.rs"].contains(&name.as_str());
        path.extension().is_some_and(|e| e == "rs") && !test_only
    };
    let read = |(name, path): (String, std::path::PathBuf)| {
        let source = std::fs::read_to_string(path).expect("readable");
        (name, source)
    };
    named.filter(ships).map(read).collect()
}

/// A source without its test module.
fn shipped(source: &str) -> &str {
    source.split("mod tests {").next().expect("a first half")
}

/// The table's sentences, each with the button its row stands with.
fn every_hint() -> Vec<Spoken> {
    let full = Facts { held: Some(120), refused_twice: Some(doc::HELD_FRAME.into()), ..Facts::default() };
    let spoken = |hint: &Hint| {
        let words = match hint.words {
            Words::Plain(words) => words.to_string(),
            Words::Chord(before, _, after) => format!("{before}Ctrl+Shift+Space{after}"),
            Words::Held => format!("This drawing stays on screen for {} frames.", full.held.expect("set")),
            Words::Reason => doc::HELD_FRAME.to_string(),
            Words::Exported => "Exported ball.gif.".to_string(),
        };
        let verb = if matches!(hint.words, Words::Reason) { Some(Verb::Fix(Action::NewDrawing)) } else { hint.verb };
        Spoken { bit: Some(hint.bit), words, verb }
    };
    let mut all: Vec<Spoken> = say::HINTS.iter().map(spoken).collect();
    let rows = say::EMPTY_STEPS.iter().map(|e| Spoken { bit: None, words: e.words.into(), verb: Some(Verb::GoToDraw) });
    all.extend(rows);
    all
}

/// Spec 4.4 and AU7's gate: at the two smallest sizes no sentence and no
/// refusal elides, and the action button stands whole inside the band. The
/// sizes past them hold too, the dots in the lane from `SAY_DOTS_LANE`. One
/// is the lane's floor: at the fold with the sheet up the longest words stop
/// on a `…` beside a whole button, and their card has the rest.
#[test]
fn ausay_every_hint_fits_at_the_smallest_sizes() {
    let refusals = refusals();
    assert!(refusals.len() >= 90, "the scan reads them: {}", refusals.len());
    for (w, tall) in [(832.0, 520.0), (853.0, 533.0), (steps::FOLD, 640.0), (1092.0, 614.0), (1280.0, 800.0)] {
        for sheet in [false, true] {
            let floor = w == steps::FOLD && sheet;
            let mut h = ready();
            add_drawing(&mut h);
            sized(&mut h, w, tall);
            if sheet {
                h.editor_mut().show_sheet();
                settle(&mut h);
            }
            h.perform(Action::ZoomFit);
            settle(&mut h);
            // The paper as it stands with nothing on the lane. The window's
            // own rail lands over a sheet-up paper at the smallest sizes.
            let bare = measure::paper_share(&h).paper_under_chrome;
            assert!(sheet || bare == 0, "{w}: {bare}");
            for hint in every_hint() {
                h.editor_mut().say.pin = Some(hint.clone());
                h.run_frames(3);
                quiet(&mut h);
                let clip = measure::clip_here(&h, Scope::Chrome);
                assert!(floor || clip.elided == 0, "{w} sheet={sheet} {:?}: {:?}", hint.words, clip.found);
                assert_eq!(clip.clipped, 0, "{w} sheet={sheet} {:?}: {:?}", hint.words, clip.found);
                let word_at = h.painted_at(&hint.words);
                if let Some(verb) = hint.verb {
                    let button = h.rect(&format!("{} (say line)", verb.label(h.editor().kind())));
                    let inside = button.bottom() <= tall && button.top() >= tall - plate::BAND_H;
                    assert!(inside && button.right() <= w, "{button:?} at {w}");
                    assert!(word_at.x < button.left(), "{:?} under the button", hint.words);
                    assert_eq!(button.height(), plate::CTRL_H, "it never shrinks");
                }
                assert_eq!(measure::paper_share(&h).paper_under_chrome, bare, "{:?}", hint.words);
            }
            h.editor_mut().say.pin = None;
            for refusal in &refusals {
                h.state_mut().refuse(refusal.clone());
                h.run_frames(3);
                let clip = measure::clip_here(&h, Scope::Chrome);
                assert!(floor || clip.elided == 0, "{w} sheet={sheet} {refusal:?}: {:?}", clip.found);
            }
        }
    }
}

/// AU7's gate: a hint and the canvas line cover no paper.
#[test]
fn ausay_nothing_teaches_over_the_paper() {
    let mut h = ready();
    quiet(&mut h);
    assert!(says(&h, "Draw on the paper to begin."));
    assert_eq!(measure::paper_share(&h).paper_under_chrome, 0, "with a hint");
    let h = empty_cell();
    assert!(h.editor().canvas.dbg_empty_line);
    assert_eq!(measure::paper_share(&h).paper_under_chrome, 0, "with the canvas line");
}

/// Spec 8.2: the same refusal twice holds its reason on the lane, with the
/// Action that answers it when there is one.
#[test]
fn ausay_a_refusal_said_twice_stays_with_its_fix() {
    let mut h = ready();
    raster_line(&mut h);
    quiet(&mut h);
    h.state_mut().refuse(doc::HELD_FRAME);
    settle(&mut h);
    quiet(&mut h);
    assert_ne!(picked(&h).as_deref(), Some(doc::HELD_FRAME), "once is no pattern");
    h.state_mut().refuse(doc::HELD_FRAME);
    settle(&mut h);
    assert_eq!(picked(&h).as_deref(), Some(doc::HELD_FRAME), "twice is");
    quiet(&mut h);
    assert!(h.has(FIX_IT));
    assert_eq!(say::fix_of(doc::HELD_FRAME), Some(Action::NewDrawing));
    assert_eq!(say::fix_of(doc::READ_ONLY_VIEW), Some(Action::ToggleCompositeView));
    assert!(include_str!("canvas.rs").contains(doc::READ_ONLY_VIEW), "the pen route says these very words");
    assert_eq!(say::fix_of("refused — the layer is locked"), None);
    h.click(FIX_IT);
    settle(&mut h);
    assert_ne!(picked(&h).as_deref(), Some(doc::HELD_FRAME), "answered");
    assert_ne!(h.rig().config.ui.hints_seen & say::REFUSED_TWICE, 0, "the press read it");
}

/// Spec 8.2: three pen strokes with no pressure, and `Fix it` opens the pen
/// page. A pen that works answers it, and a mouse never asks.
#[test]
fn ausay_a_flat_pen_is_said_after_three_strokes() {
    let f = Facts { flat_pen: true, stage: Some(Stage::Drawing), ..Facts::default() };
    let spoken = say::read(&f, &mut 0, None, chord).expect("the row");
    assert_eq!(spoken.words, "Your pen is drawing without pressure.");
    assert_eq!(spoken.verb, Some(Verb::PenPage));
    let mut seen = 0;
    let works = Facts { pen_works: true, ..f.clone() };
    assert_ne!(say::read(&works, &mut seen, None, chord).map(|s| s.bit), Some(Some(say::FLAT_PEN)));
    assert_ne!(seen & say::FLAT_PEN, 0);

    let mut h = ready();
    h.editor_mut().say.pin = Some(Spoken { bit: Some(say::FLAT_PEN), words: "pin".into(), verb: Some(Verb::PenPage) });
    settle(&mut h);
    quiet(&mut h);
    h.click(FIX_IT);
    settle(&mut h);
    assert!(h.editor().request_settings_pen, "the pen page");
    assert_ne!(h.editor().say.seen & say::FLAT_PEN, 0, "and it is answered");

    // Real strokes, through the pen's own route.
    const FLAT: &str = "Your pen is drawing without pressure.";
    let mut h = ready();
    quiet(&mut h);
    let from = paper_middle(&h);
    flat_line(&mut h, from);
    flat_line(&mut h, from + vec2(0.0, 20.0));
    assert_eq!(h.state().strokes_done, 2, "two strokes landed");
    quiet(&mut h);
    assert_ne!(picked(&h).as_deref(), Some(FLAT), "two are no pattern");
    flat_line(&mut h, from + vec2(0.0, 40.0));
    quiet(&mut h);
    assert_eq!(picked(&h).as_deref(), Some(FLAT), "three are");

    // Show me's circle is nobody's pen: two flat lines and a circle are two.
    let mut h = ready();
    quiet(&mut h);
    let from = paper_middle(&h);
    flat_line(&mut h, from);
    flat_line(&mut h, from + vec2(0.0, 20.0));
    assert_ne!(h.editor_mut().show_me(0), Performed::Ignored);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, 3, "the circle is a stroke to the document");
    quiet(&mut h);
    assert_ne!(picked(&h).as_deref(), Some(FLAT));

    // A mouse never asks.
    let mut h = ready();
    quiet(&mut h);
    for _ in 0..3 {
        line(&mut h);
    }
    assert_eq!(h.state().strokes_done, 3);
    quiet(&mut h);
    assert_ne!(picked(&h).as_deref(), Some(FLAT));
}

/// A tablet's line whose samples never carry force.
fn flat_line(h: &mut EditorHarness, from: egui::Pos2) {
    h.pen_down(from);
    h.pen_move_flat(from + vec2(-30.0, 0.0));
    h.pen_move_flat(from + vec2(-60.0, 5.0));
    h.pen_up(from + vec2(-60.0, 5.0));
    settle(h);
}

/// `Check your pen` listens for the person's line. Show me's circle is not
/// that line: it stands, its tick stands, and the check goes on listening.
/// With no check asked, the circle ends the float as any stroke does.
#[test]
fn ausay_show_me_leaves_the_pen_check_listening() {
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1600.0, 900.0);
    h.click("Check your pen");
    settle(&mut h);
    assert_eq!(h.editor().start.as_ref().map(|w| w.pen), Some(welcome::PenCheck::Waiting));
    assert_ne!(h.editor_mut().show_me(0), Performed::Ignored);
    settle(&mut h);
    assert!(h.state().has_art(), "the circle stands");
    assert!(h.editor().quick_start[welcome::FIRST_STROKE], "and so does its tick");
    assert_eq!(h.editor().start.as_ref().map(|w| w.pen), Some(welcome::PenCheck::Waiting), "still listening");

    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1600.0, 900.0);
    assert!(h.editor().start.is_some());
    assert_ne!(h.editor_mut().show_me(0), Performed::Ignored);
    settle(&mut h);
    assert!(h.editor().start.is_none(), "a finished stroke accepts the paper");
    assert!(h.state().has_art());
}

/// Spec 8.3: five dots, no controls and no words; under the fold they hide,
/// and the last tick ends them.
#[test]
fn ausay_dots_show_the_place() {
    let mut h = ready();
    quiet(&mut h);
    assert!(h.editor().say.dots_painted);
    let before = measure::census(&mut h);
    h.editor_mut().quick_start = [true; 5];
    h.rig_mut().config.ui.hints_seen |= say::DRAWING_SLOTS.iter().fold(0, |all, bit| all | bit);
    settle(&mut h);
    assert!(!h.editor().say.dots_painted, "finished");
    let after = measure::census(&mut h);
    assert_eq!((before.controls, before.words), (after.controls, after.words), "they count as nothing");
    h.editor_mut().quick_start = [false; 5];
    sized(&mut h, steps::FOLD - 1.0, 640.0);
    assert!(!h.editor().say.dots_painted, "under the fold");
    sized(&mut h, 1600.0, 900.0);
    assert!(h.editor().say.dots_painted);
}

/// The dots wear a card for whoever rests on them, in words, and an animation
/// with its strip closed has nothing yet for them to count.
#[test]
fn ausay_dots_card_says_the_place() {
    let rows = &welcome::STEPS;
    assert_eq!(say::place_words(rows, &[true, true, false, false, false]), "Quick Start: 2 of 5. Next: draw the next pose.");
    assert_eq!(say::place_words(rows, &[false; 5]), "Quick Start: 0 of 5. Next: draw something.");
    let mut h = animation();
    quiet(&mut h);
    let band_left = h.rect("Add drawing (band)").left();
    h.event(egui::Event::PointerMoved(egui::pos2(band_left - 24.0, h.rect("Add drawing (band)").center().y)));
    h.run_frames(1);
    h.wait(1.5);
    assert!(says(&h, "Quick Start: 0 of 5. Next: draw something."), "{:?}", h.painted_text());

    let mut h = animation();
    h.editor_mut().strip_open = false;
    h.editor_mut().full_band = false;
    settle(&mut h);
    assert!(h.editor().strip_closed(), "an animation with its strip closed");
    assert!(!h.editor().say.dots_painted, "the strip is closed");
    h.editor_mut().strip_open = true;
    settle(&mut h);
    assert!(h.editor().say.dots_painted, "the strip came out");
}

/// Spec 8.2: a hint's button stands straight after its words, where the eye
/// already is, and the dots keep the lane's far end clear of both.
#[test]
fn ausay_the_action_button_stands_after_its_words() {
    let mut h = animation();
    quiet(&mut h);
    let words = h.rect("Draw on the paper to begin.");
    let button = h.rect(SHOW_ME);
    assert!((button.left() - words.right() - 6.0).abs() < 1.0, "{words:?} then {button:?}");
    assert!(button.right() < h.rect("Add drawing (band)").left() - plate::SAY_DOTS, "{button:?}");
}

/// Spec 8.2's Effects row, in the words that are true until AU14's shelf: the
/// graph is on screen, and the chord is the live one.
#[test]
fn ausay_effects_hint_names_what_the_step_has() {
    let mut h = EditorHarness::with_state_shipped(one_drawing_at(0));
    sized(&mut h, 1600.0, 900.0);
    h.editor_mut().enter_room(Stage::Composite);
    settle(&mut h);
    quiet(&mut h);
    assert!(visible_panes(&h.editor().dock).contains(&Pane::NodeGraph));
    let chord_words = crate::menus::chord_label(&h.ctx(), Action::AddNode).expect("bound");
    let words = format!("Press {chord_words} over the graph to add an effect.");
    assert_eq!(picked(&h).as_deref(), Some(words.as_str()));
    assert!(says(&h, &words), "{:?}", h.painted_text());
}

/// AU7's 16 named tests and its shots stand: the spec's list, read from the
/// spec, against this file; and the shots' own list against what it owes.
#[test]
fn ausay_the_named_tests_exist() {
    let spec = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../research/PSD-approachable-ui.md");
    let spec = std::fs::read_to_string(spec).expect("the spec");
    let part = spec.split("## AU7 Say line and Quick Start").nth(1).expect("the part");
    let list = part.split("### Tests").nth(1).expect("its tests").split("### Gates").next().expect("a list");
    let is_name = |word: &&str| word.starts_with("ausay_") && word.len() > 6 && !word.contains(' ');
    let named: Vec<&str> = list.split('`').filter(is_name).collect();
    assert_eq!(named.len(), 16, "{named:?}");
    let lines: Vec<&str> = include_str!("ausay_tests.rs").lines().collect();
    for name in named {
        let test = format!("fn {name}() {{");
        assert!(lines.windows(2).any(|pair| pair[0] == "#[test]" && pair[1] == test), "{name}");
    }
    let shots = include_str!("uishot.rs");
    for shot in ["say-hint", "say-sheet", "say-fix-fold", "say-plan", "say-timing", "say-color", "say-effects", "say-empty-cell", "say-export"] {
        assert!(shots.contains(&format!("(\"{shot}\", ")), "{shot}");
    }
}
