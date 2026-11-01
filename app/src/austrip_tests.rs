//! AU5's tests: the simple timeline and the band (spec 7.2, 4.4, 8.2).

use anim_core::command::Command;
use anim_core::xsheet::Exposure;
use eframe::egui;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use egui_kittest::kittest::Queryable;

use crate::config::{self, Action};
use crate::doc::{AppState, BLANK_FRAME, HELD_FRAME};
use crate::say::{Spoken, Verb};
use crate::steps::ProjectKind;
use crate::strip::{self, Sheet, Strip};
use crate::testkit::{EditorHarness, PanelHarness};
use crate::workspace::Stage;
use crate::{Pane, Performed, measure, plate, rail, steps, vocab, visible_panes};

/// The Say line's floor (spec 4.4).
const SAY_FLOOR: f32 = 240.0;

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

/// A shot whose active track holds one drawing for each of `holds`.
fn timed(holds: &[u32]) -> AppState {
    let mut s = AppState::new_project("t", 64, 48, 24, 72.0);
    s.set_cut_length(holds.iter().sum());
    let mut frame = 0;
    for hold in holds {
        s.goto(frame);
        s.new_drawing_at_frame();
        frame += hold;
    }
    s.goto(0);
    s
}

/// `state`, shipped: the sheet off screen and the strip up. AU19: a shipped
/// document of many frames is an animation and opens on its sheet, so this is
/// that sheet hidden by hand: the one state the strip stands in.
fn strip_of(state: AppState) -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(state);
    h.editor_mut().plan_sheet_hidden = true;
    h.editor_mut().dock = Stage::Drawing.dock();
    h.kit_mut().set_size(vec2(1280.0, 800.0));
    settle(&mut h);
    assert!(!h.editor().sheet_on_screen() && h.editor().strip_open, "the strip is up");
    h
}

fn strip_up(holds: &[u32]) -> EditorHarness {
    strip_of(timed(holds))
}

/// The full transport, where it stands now: AU19 keeps the plain band in Plan
/// beside its sheet, so this is Color with the sheet brought up, the band
/// that keeps its sheet key.
fn full_transport(h: &mut EditorHarness) {
    h.perform(Action::RoomFinishing);
    h.editor_mut().show_sheet();
    settle(h);
    assert!(h.editor().sheet_on_screen());
}

fn holds(state: &AppState) -> Vec<u32> {
    strip::sheets(state).iter().map(|s| s.len).collect()
}

fn strip_rect(h: &EditorHarness) -> Rect {
    let rects = &h.editor().pane_rects;
    rects.iter().find(|(p, ..)| *p == Pane::XSheet).map(|(_, r, _)| *r).expect("the strip stands")
}

fn scale(h: &EditorHarness) -> f32 {
    h.editor().strip.scale().expect("the strip has shown")
}

/// A point on frame `f`'s sheet.
fn on_sheet(h: &EditorHarness, f: u32) -> Pos2 {
    let r = strip_rect(h);
    pos2(r.left() + (f as f32 + 0.5) * scale(h), r.top() + strip::BAR_H + 19.0)
}

/// A point on the peg bar over frame `f`.
fn on_bar(h: &EditorHarness, f: u32) -> Pos2 {
    let r = strip_rect(h);
    pos2(r.left() + (f as f32 + 0.5) * scale(h), r.top() + 7.0)
}

/// A point on the right edge of the sheet that ends before frame `end`.
fn on_edge(h: &EditorHarness, end: u32) -> Pos2 {
    let r = strip_rect(h);
    pos2(r.left() + end as f32 * scale(h) - 3.0, r.top() + strip::BAR_H + 19.0)
}

/// The rect of a named widget in points: the harness names them in pixels.
fn points(h: &EditorHarness, name: &str) -> Rect {
    let (r, ppp) = (h.rect(name), h.ctx().pixels_per_point());
    Rect::from_min_max((r.min.to_vec2() / ppp).to_pos2(), (r.max.to_vec2() / ppp).to_pos2())
}

fn steps(h: &EditorHarness) -> usize {
    h.state().engine.position()
}

/// A chord for a rebound key. The Episode pane holds it too by default.
fn free_chord() -> Option<config::Chord> {
    Some(config::Chord { keys: vec!["T".to_string()], ctrl: true, shift: true, alt: false })
}

// ---- one view of the same exposures ----

#[test]
fn austrip_sheets_are_the_column_runs() {
    let mut s = timed(&[3, 2, 4]);
    let column = s.cut().xsheet.column(s.view.active_column).expect("a track");
    let runs = column.runs(0, s.frame_count());
    let sheets = strip::sheets(&s);
    assert_eq!(sheets.iter().map(|x| (x.shows, x.len)).collect::<Vec<_>>(), runs);
    assert_eq!(sheets.iter().map(|x| x.from).collect::<Vec<_>>(), [0, 3, 5]);
    // A track that starts late has an empty sheet at its head.
    s.goto(0);
    s.clear_key_at_frame();
    let sheets = strip::sheets(&s);
    assert_eq!(sheets[0], Sheet { from: 0, len: 3, shows: Exposure::Empty });
    assert_eq!(sheets.iter().map(|x| x.len).sum::<u32>(), s.frame_count());
}

/// A small generator: the property's seed stays in the source.
struct Dice(u64);

impl Dice {
    fn roll(&mut self, sides: u32) -> u32 {
        self.0 = self.0.wrapping_mul(0x5851_F42D_4C95_7F2D).wrapping_add(0x1405_7B7E_F767_814F);
        ((self.0 >> 33) as u32) % sides.max(1)
    }
}

type Bare = egui_kittest::Harness<'static, (AppState, Strip)>;

const FRAME: f32 = 1.0 / 60.0;

/// The strip alone, wide enough that nothing scrolls, on a clock of real
/// frames: the builder's own step is a quarter of a second.
fn bare(state: AppState) -> Bare {
    let size = vec2(7000.0, rail::STRIP_H + 40.0);
    let builder = egui_kittest::Harness::builder().with_size(size).with_step_dt(FRAME);
    let mut h = builder.build_ui_state(
        |ui, (state, strip): &mut (AppState, Strip)| {
            plate::install_fonts(ui.ctx());
            strip.begin();
            strip::show(ui, state, strip, true);
        },
        (state, Strip::default()),
    );
    h.step();
    h
}

fn press(h: &mut Bare, at: Pos2, pressed: bool) {
    let (button, modifiers) = (PointerButton::Primary, Modifiers::NONE);
    h.input_mut().events.push(Event::PointerButton { pos: at, button, pressed, modifiers });
}

fn drag(h: &mut Bare, from: Pos2, to: Pos2) {
    h.input_mut().events.push(Event::PointerMoved(from));
    h.step();
    press(h, from, true);
    h.step();
    h.input_mut().events.push(Event::PointerMoved(to));
    h.step();
    press(h, to, false);
    h.step();
}

/// Pixels a bare strip gives one frame.
fn bare_scale(h: &Bare) -> f32 {
    h.state().1.scale().expect("the strip has shown")
}

/// Where a bare strip stands: the panel's margin puts it off the origin.
fn bare_rect(h: &Bare) -> Rect {
    h.state().1.rect().expect("the strip has shown")
}

/// `dx` past frame `frame`, down the middle of a bare strip's sheets.
fn bare_sheet(h: &Bare, frame: u32, dx: f32) -> Pos2 {
    let r = bare_rect(h);
    pos2(r.left() + frame as f32 * bare_scale(h) + dx, r.top() + strip::BAR_H + 19.0)
}

/// `dx` past frame `frame`, on a bare strip's peg bar.
fn bare_bar(h: &Bare, frame: u32, dx: f32) -> Pos2 {
    let r = bare_rect(h);
    pos2(r.left() + frame as f32 * bare_scale(h) + dx, r.top() + 7.0)
}

/// The same edit through the Timing sheet's own verbs.
fn sheet_edge(twin: &mut AppState, sheet: Sheet, frames: u32) {
    let count = twin.frame_count();
    let last = sheet.from + sheet.len - 1;
    if sheet.from + sheet.len < count {
        let unit = if frames > sheet.len { 1 } else { -1 };
        for step in 0..frames.abs_diff(sheet.len) {
            twin.goto((i64::from(last) + i64::from(unit) * i64::from(step)) as u32);
            twin.hold(unit);
        }
    }
    twin.set_cut_length(count + frames - sheet.len);
}

fn sheet_add(twin: &mut AppState, sheet: Sheet) {
    let after = sheet.from + sheet.len;
    if after == twin.frame_count() {
        twin.set_cut_length(after + sheet.len);
    } else {
        twin.goto(after);
        for _ in 0..sheet.len {
            twin.insert_frame();
        }
    }
    twin.goto(after);
    twin.new_drawing_at_frame();
}

/// The strip's every gesture against the Timing sheet's verbs, seeded. Both
/// write the same cells, the same length and the same cursor, and undo walks
/// the strip back byte for byte. A gesture its sheet cannot take is a tap,
/// and every kind has to run: a dull track would let the property off.
#[test]
fn austrip_two_views_agree() {
    const GESTURES: usize = 240;
    const LONG: u32 = 60;
    let start = timed(&[3, 2, 4, 1]);
    let before = serde_json::to_vec(start.cut()).expect("a cut serialises");
    let mut twin = timed(&[3, 2, 4, 1]);
    let mut h = bare(start);
    let first = h.state().0.engine.position();
    let mut dice = Dice(0x5EED_A05);
    let mut ran = [0; 6];
    for gesture in 0..GESTURES {
        let sheets = strip::sheets(&h.state().0);
        let sheet = sheets[dice.roll(sheets.len() as u32) as usize];
        let count = h.state().0.frame_count();
        let was = h.state().0.engine.position();
        let drawn = matches!(sheet.shows, Exposure::Drawing(_));
        let drawings = sheets.iter().filter(|s| matches!(s.shows, Exposure::Drawing(_))).count();
        let kind = match dice.roll(6) {
            2 if !drawn => 0,
            3 if !drawn || count >= LONG => 0,
            5 if !drawn || drawings < 3 => 0,
            kind => kind,
        };
        ran[kind as usize] += 1;
        match kind {
            0 => {
                let mid = bare_sheet(&h, sheet.from, 8.0);
                drag(&mut h, mid, mid);
                let column = twin.view.active_column;
                twin.select_cells((column, sheet.from), column, sheet.from);
            }
            1 => {
                let frame = dice.roll(count);
                let (from, to) = (bare_bar(&h, 0, 4.0), bare_bar(&h, frame, 12.0));
                drag(&mut h, from, to);
                twin.goto(frame);
            }
            2 => {
                let longer = sheet.len < 2 || (count < LONG && dice.roll(2) == 0);
                let frames = if longer { sheet.len + 1 + dice.roll(3) } else { 1 + dice.roll(sheet.len - 1) };
                let edge = bare_sheet(&h, sheet.from + sheet.len, -3.0);
                let by = (frames as f32 - sheet.len as f32) * bare_scale(&h);
                drag(&mut h, edge, edge + vec2(by, 0.0));
                sheet_edge(&mut twin, sheet, frames);
                let frame = h.state().0.view.frame;
                twin.goto(frame);
            }
            3 => {
                let (state, _) = h.state_mut();
                state.goto(sheet.from);
                assert!(state.add_drawing_after());
                sheet_add(&mut twin, sheet);
                h.step();
            }
            4 => {
                let step = if count < LONG { 1 + dice.roll(3) } else { 1 };
                let (state, _) = h.state_mut();
                state.set_speed(step);
                let (length, frame) = (state.frame_count(), state.view.frame);
                let column = twin.view.active_column;
                twin.select_cells((column, 0), column, count - 1);
                twin.retime(step);
                twin.set_cut_length(length);
                twin.goto(frame);
                h.step();
            }
            _ => {
                let (state, _) = h.state_mut();
                state.goto(sheet.from);
                state.clear_key_at_frame();
                twin.goto(sheet.from);
                twin.clear_key_at_frame();
                h.step();
            }
        }
        let (a, b) = (&h.state().0, &twin);
        let said = format!("gesture {gesture}, kind {kind}: {} / {}", a.refusal, a.status);
        assert_eq!(strip::sheets(a), strip::sheets(b), "{said}");
        assert_eq!(a.frame_count(), b.frame_count(), "{said}");
        assert_eq!(a.view.frame, b.view.frame, "{said}");
        assert!(a.engine.position() <= was + 1, "{said}: one undo at most");
    }
    assert!(GESTURES >= 200);
    assert!(ran.iter().all(|n| *n >= 12), "every kind ran: {ran:?}");
    let state = &mut h.state_mut().0;
    while state.engine.position() > first {
        state.undo();
    }
    assert_eq!(serde_json::to_vec(state.cut()).expect("a cut serialises"), before, "undo is byte exact");
}

#[test]
fn austrip_undo_is_byte_exact() {
    let mut h = strip_up(&[3, 2, 4]);
    let bytes = |h: &EditorHarness| serde_json::to_vec(h.state().cut()).expect("a cut serialises");
    let start = bytes(&h);
    let edge = on_edge(&h, 3);
    let sc = scale(&h);
    h.drag(edge, edge + vec2(2.0 * sc, 0.0), 4);
    let edged = bytes(&h);
    assert_ne!(edged, start);
    h.perform(Action::AddDrawingAfter);
    let added = bytes(&h);
    assert_ne!(added, edged);
    h.state_mut().set_speed(2);
    assert_ne!(bytes(&h), added);
    h.state_mut().undo();
    assert_eq!(bytes(&h), added, "speed");
    h.state_mut().undo();
    assert_eq!(bytes(&h), edged, "add");
    h.state_mut().undo();
    assert_eq!(bytes(&h), start, "edge");
}

// ---- the gestures ----

#[test]
fn austrip_tap_lands_on_the_run_start() {
    let mut h = strip_up(&[4, 4]);
    let at = on_sheet(&h, 6);
    h.click_pos(at);
    assert_eq!(h.state().view.frame, 4, "the run's first frame, not frame 6");
    assert_eq!(h.state().view.active_column, h.state().cut().xsheet.columns[0].id);
    assert_eq!(h.state().no_own_cel_reason(), None, "the tap leaves the pen on a drawing");
}

#[test]
fn austrip_bar_drag_scrubs() {
    let mut h = strip_up(&[4, 4]);
    let was = steps(&h);
    let (from, to) = (on_bar(&h, 1), on_bar(&h, 6));
    h.drag(from, to, 5);
    assert_eq!(h.state().view.frame, 6, "the bar is frame exact");
    assert_eq!(steps(&h), was, "a scrub writes nothing");
}

#[test]
fn austrip_edge_drag_commits_once() {
    let mut h = strip_up(&[3, 2, 4]);
    let was = steps(&h);
    let edge = on_edge(&h, 3);
    let to = edge + vec2(2.0 * scale(&h), 0.0);
    h.press_and_move(edge, to, 4);
    assert_eq!(steps(&h), was, "nothing is written while the hand moves");
    assert_eq!(holds(h.state()), [3, 2, 4]);
    h.release(to);
    assert_eq!(holds(h.state()), [5, 2, 4], "later keys ripple");
    assert_eq!(h.state().frame_count(), 11, "and the cut follows");
    assert_eq!(steps(&h), was + 1, "one undo");
    h.state_mut().undo();
    assert_eq!(holds(h.state()), [3, 2, 4]);
    assert_eq!(h.state().frame_count(), 9);
    // Shorter, on the last sheet: the cut's length is its length.
    settle(&mut h);
    let edge = on_edge(&h, 9);
    h.drag(edge, edge - vec2(2.0 * scale(&h), 0.0), 4);
    assert_eq!(holds(h.state()), [3, 2, 2]);
    assert_eq!(h.state().frame_count(), 7);
}

#[test]
fn austrip_add_drawing_is_one_undo_and_copies_the_hold() {
    let mut h = strip_up(&[3, 2]);
    let (was, library) = (steps(&h), h.state().cut().drawings.len());
    h.key_mods(Modifiers::ALT, Key::A);
    assert_eq!(h.performed().last().map(|p| p.0), Some(Action::AddDrawingAfter));
    assert_eq!(holds(h.state()), [3, 3, 2], "the new sheet holds as long as this one");
    assert_eq!(h.state().view.frame, 3, "and the cursor stands on it");
    assert_eq!(h.state().cut().drawings.len(), library + 1);
    assert_eq!(steps(&h), was + 1, "one undo");
    h.state_mut().undo();
    assert_eq!(holds(h.state()), [3, 2]);
    assert_eq!(h.state().cut().drawings.len(), library);
    // After the last sheet the cut grows by the hold.
    h.state_mut().goto(4);
    settle(&mut h);
    h.click("Add drawing (band)");
    assert_eq!(holds(h.state()), [3, 2, 2]);
    assert_eq!(h.state().frame_count(), 7);
}

#[test]
fn austrip_add_drawing_on_an_empty_run_stays_uniform() {
    let mut s = timed(&[2, 2, 3]);
    // The third drawing's frames become a gap.
    let (at, column) = (s.at(), s.view.active_column);
    let cmd = Command::SetCell { at, column, frame: 4, key: Some(Exposure::Empty) };
    assert!(s.engine.apply("gap", vec![cmd]).is_ok());
    s.goto(5);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [2, 2, 3, 2], "a gap has no hold to copy: the track's rhythm goes on");
    assert_eq!(s.view.frame, 7);
    // With no drawing before it, a gap gives the shortest hold.
    let mut blank = AppState::new_project("t", 64, 48, 24, 72.0);
    let count = blank.frame_count();
    assert!(blank.add_drawing_after());
    assert_eq!(holds(&blank), [count, 1]);
}

#[test]
fn austrip_speed_is_one_undo() {
    let mut h = strip_up(&[1, 1, 1]);
    let was = steps(&h);
    h.click("Speed");
    settle(&mut h);
    h.wait(plate::CARD_DELAY + 0.1);
    assert!(!h.has_text_containing("each drawing holds"), "the card would lie over its popover");
    h.click("Medium");
    settle(&mut h);
    assert_eq!(holds(h.state()), [2, 2, 2], "every drawing on twos");
    assert_eq!(h.state().frame_count(), 6, "and the cut follows");
    assert_eq!(steps(&h), was + 1, "one undo");
    assert_eq!(strip::speed_of(h.state()), Some(2));
    h.state_mut().undo();
    assert_eq!(holds(h.state()), [1, 1, 1]);
    assert_eq!(h.state().frame_count(), 3);
}

#[test]
fn austrip_speed_keeps_the_cursor_on_its_drawing() {
    let mut s = timed(&[2, 2, 2]);
    s.goto(3);
    let shown = s.current_drawing();
    s.set_speed(3);
    assert_eq!(holds(&s), [3, 3, 3]);
    assert_eq!(s.view.frame, 3);
    assert_eq!(s.current_drawing(), shown);
}

/// R15's trap: at 0.817 s egui hands a pen's press to the long touch, and a
/// click-sensing widget loses its drag. The strip senses drags alone.
#[test]
fn austrip_pen_hold_survives_long_touch() {
    let mut h = strip_up(&[4, 4]);
    let (from, to) = (on_bar(&h, 0), on_bar(&h, 6));
    h.pen_down(from);
    h.wait(1.2);
    assert!(h.ctx().input(|i| i.pointer.primary_down()), "the pen is still down");
    h.pen_move(to);
    h.run_frames(2);
    assert_eq!(h.state().view.frame, 6, "the bar still scrubs");
    h.pen_up(to);
    assert_eq!(h.state().view.frame, 6);
}

/// The moment the press becomes a menu, on the frame clock: the editor's
/// harness steps a quarter second at a time.
#[test]
fn austrip_long_press_waits_six_tenths() {
    let mut h = bare(timed(&[4, 4]));
    let at = bare_sheet(&h, 1, 8.0);
    h.input_mut().events.push(Event::PointerMoved(at));
    h.step();
    press(&mut h, at, true);
    let menu = |h: &Bare| h.query_all_by_label("add retake note…").next().is_some();
    for _ in 0..33 {
        h.step();
    }
    assert!(!menu(&h), "0.55 s is still a press");
    for _ in 0..5 {
        h.step();
    }
    assert!(menu(&h), "0.63 s is a menu");
}

#[test]
fn austrip_long_press_opens_the_cell_menu() {
    let mut h = strip_up(&[4, 4]);
    let at = on_sheet(&h, 1);
    assert!(!h.has("add retake note…"));
    h.pen_down(at);
    h.wait(0.7);
    assert!(h.has("add retake note…"), "{}", h.describe());
    let was = h.state().view.frame;
    h.pen_up(at);
    settle(&mut h);
    assert!(h.has("add retake note…"), "the lift leaves it open");
    assert_eq!(h.state().view.frame, was, "and is no tap");
    h.key(Key::Escape);
    settle(&mut h);
    assert!(!h.has("add retake note…"));
    // A right click is the same menu.
    let at = on_sheet(&h, 5);
    h.right_click_pos(at);
    settle(&mut h);
    assert!(h.has("add retake note…"), "{}", h.describe());
    // The menu stands alone: the sheet's card would lie over it.
    h.wait(plate::CARD_DELAY + 0.1);
    let name = h.state().cut().drawings[1].name.clone();
    assert!(!h.has_text_containing(&format!("({name}), ")), "{}", h.describe());
}

// ---- the sheet's chords ----

#[test]
fn austrip_sheet_chords_fire_over_the_strip() {
    let mut h = strip_up(&[4, 4]);
    let at = on_sheet(&h, 1);
    h.point_at(at);
    let ctx = h.ctx();
    assert_eq!(h.editor_mut().shortcut_ctx(&ctx), config::Ctx::Sheet, "the strip speaks for the sheet");
    h.key_mods(Modifiers::COMMAND, Key::C);
    assert_eq!(h.performed().last().map(|p| p.0), Some(Action::CopyCells));
    assert!(h.state().sheet_ui.clip_text.is_some(), "the sheet's clipboard took it");
    // Off the strip the sheet's chords sleep.
    let middle = h.editor().canvas.dbg_rect.center();
    h.point_at(middle);
    let ctx = h.ctx();
    assert_ne!(h.editor_mut().shortcut_ctx(&ctx), config::Ctx::Sheet);
}

/// The system's Ctrl+C reaches the app as an event, with no key: the strip
/// hands it to the sheet's clipboard, and the text comes back as cells.
#[test]
fn austrip_system_clipboard_events_reach_the_sheet_clipboard() {
    let mut h = strip_up(&[4, 4]);
    let at = on_sheet(&h, 1);
    h.click_pos(at);
    h.point_at(at);
    h.event(Event::Copy);
    h.run();
    let text = h.state().sheet_ui.clip_text.clone().expect("the cells went out as text");
    let was = steps(&h);
    let there = on_sheet(&h, 6);
    h.click_pos(there);
    h.point_at(there);
    h.event(Event::Paste(text));
    h.run();
    assert_eq!(steps(&h), was + 1, "the cells came back, one undo");
    h.event(Event::Paste("a line of text".into()));
    h.run();
    assert!(h.state().refusal.contains("not cells"), "{}", h.state().refusal);
}

#[test]
fn austrip_enter_opens_the_sheet_and_starts_typing() {
    for room in [Action::RoomDrawing, Action::RoomComposite] {
        let mut h = strip_up(&[4, 4]);
        h.perform(room);
        settle(&mut h);
        assert!(!h.editor().sheet_on_screen(), "{room:?} shows no sheet");
        let at = on_sheet(&h, 1);
        h.point_at(at);
        h.key(Key::Enter);
        settle(&mut h);
        assert_eq!(h.performed().last().map(|p| p.0), Some(Action::TypeIntoCell));
        assert!(h.editor().sheet_on_screen(), "{room:?}: the sheet came up");
        assert!(h.ctx().text_edit_focused(), "{room:?}: and it is typing\n{}", h.describe());
    }
}

#[test]
fn austrip_rebound_type_chord_still_works() {
    let mut h = strip_up(&[4, 4]);
    h.rig_mut().config.binding_mut(Action::TypeIntoCell).chord = free_chord();
    let at = on_sheet(&h, 1);
    h.point_at(at);
    h.key(Key::Enter);
    settle(&mut h);
    assert!(!h.editor().sheet_on_screen(), "Enter let go of it");
    h.key_mods(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    settle(&mut h);
    assert!(h.editor().sheet_on_screen());
    assert!(h.ctx().text_edit_focused(), "{}", h.describe());
}

#[test]
fn austrip_delete_removes_the_drawing_not_the_track() {
    let mut h = strip_up(&[4, 4, 4]);
    let columns = h.state().cut().xsheet.columns.len();
    let was = steps(&h);
    let at = on_sheet(&h, 6);
    h.point_at(at);
    h.key(Key::Delete);
    assert_eq!(h.performed().last().map(|p| p.0), Some(Action::ClearFrameKey), "one translation");
    assert_eq!(h.state().cut().xsheet.columns.len(), columns, "never a track");
    assert_eq!(holds(h.state()), [8, 4], "the sheet under the pointer went");
    assert_eq!(steps(&h), was + 1, "one undo");
    h.state_mut().undo();
    assert_eq!(holds(h.state()), [4, 4, 4]);
    // Shift+Delete takes the frame, as over the sheet.
    h.key_mods(Modifiers::SHIFT, Key::Delete);
    assert_eq!(h.performed().last().map(|p| p.0), Some(Action::DeleteFrames));
    assert_eq!(h.state().frame_count(), 11);
}

#[test]
fn austrip_menu_remove_track_still_removes_a_track() {
    let mut h = strip_up(&[4, 4]);
    h.state_mut().add_column();
    settle(&mut h);
    let columns = h.state().cut().xsheet.columns.len();
    let at = on_sheet(&h, 1);
    h.point_at(at);
    // The menu and the palette reach `perform`: no translation there.
    assert_eq!(h.perform(Action::RemoveColumn), Performed::Done);
    assert_eq!(h.state().cut().xsheet.columns.len(), columns - 1);
}

// ---- the band ----

#[test]
fn austrip_band_follows_the_sheet() {
    let mut h = strip_up(&[4, 4]);
    for key in ["Frame 1 of 8", "Add drawing (band)", "Ghost frames toggle", "Speed"] {
        assert!(h.has(key), "{key} on the plain transport\n{}", h.describe());
    }
    let loop_start = vocab::LOOP_START;
    assert!(!h.has("first frame (Home)") && !h.has(loop_start) && !h.has(vocab::SOUND_WHILE_DRAGGING));
    full_transport(&mut h);
    let full = [
        "first frame (Home)",
        "last frame (End)",
        vocab::LOOP,
        loop_start,
        vocab::LOOP_END,
        vocab::SOUND_WHILE_DRAGGING,
    ];
    for key in full {
        assert!(h.has(key), "{key} on the full transport\n{}", h.describe());
    }
    for key in ["Frame 1 of 8", "Add drawing (band)", "Ghost frames toggle", "Speed"] {
        assert!(!h.has(key), "{key} is the plain transport's");
    }
    h.editor_mut().show_sheet();
    settle(&mut h);
    assert!(h.editor().sheet_on_screen(), "show_sheet only ever shows");
}

#[test]
fn austrip_transport_row_stays_on_the_bottom_edge() {
    let mut h = strip_up(&[4, 4]);
    let up = points(&h, "play (Space)");
    let window = h.ctx().content_rect();
    assert_eq!(window.bottom() - up.bottom(), 2.0, "the row is the band's last 36");
    assert_eq!(strip_rect(&h).height(), rail::STRIP_H);
    assert_eq!(up.height(), plate::CHROME_H);
    h.editor_mut().strip_open = false;
    settle(&mut h);
    assert_eq!(points(&h, "play (Space)"), up, "the strip goes above it, and nothing moves");
    h.editor_mut().strip_open = true;
    full_transport(&mut h);
    assert_eq!(points(&h, "play (Space)").bottom(), up.bottom(), "the full row stands on the same edge");
}

/// The transport and the rail's keys, by name.
fn band_keys(h: &EditorHarness) -> Vec<(String, Rect)> {
    let names = ["play (Space)", "Add drawing (band)", vocab::SHOW_TIMING_SHEET, "Brush", "Eraser", "Layers"];
    names.iter().map(|n| (n.to_string(), points(h, n))).collect()
}

#[test]
fn austrip_full_band_latch_moves_nothing() {
    let mut h = EditorHarness::as_animation();
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    settle(&mut h);
    h.hide_sheet();
    h.state_mut().new_drawing_at_frame();
    settle(&mut h);
    for key in ["previous frame (S)", "next frame (F)", "Ghost frames toggle", "Speed"] {
        assert!(!h.has(key), "{key} waits for the second drawing");
    }
    let was = band_keys(&h);
    let count = h.state().frame_count();
    let counter = points(&h, &format!("Frame 1 of {count}"));
    h.state_mut().view.frame = 4;
    h.state_mut().new_drawing_at_frame();
    h.state_mut().view.frame = 0;
    settle(&mut h);
    for key in ["previous frame (S)", "next frame (F)", "Ghost frames toggle", "Speed", "Flip"] {
        assert!(h.has(key), "{key} arrives with the second drawing");
    }
    assert_eq!(band_keys(&h), was, "their slots were reserved");
    assert_eq!(points(&h, &format!("Frame 1 of {count}")), counter);
    let (next, play) = (points(&h, "next frame (F)"), points(&h, "play (Space)"));
    assert!(next.left() >= play.right() && next.left() - play.right() < 12.0, "in its slot");
}

#[test]
fn austrip_strip_opens_with_one_refit_at_pen_up() {
    let mut h = EditorHarness::as_animation();
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    settle(&mut h);
    h.hide_sheet();
    h.editor_mut().strip_open = false;
    settle(&mut h);
    let was = band_keys(&h);
    let middle = h.editor().canvas.dbg_rect.center();
    // Mid-stroke the band stands still.
    h.press_and_move(middle, middle + vec2(30.0, 0.0), 3);
    assert!(h.editor().canvas.stroke_active());
    h.key_mods(Modifiers::ALT, Key::A);
    h.run_frames(2);
    assert!(!h.editor().strip_open, "no refit under a stroke");
    h.release(middle + vec2(30.0, 0.0));
    settle(&mut h);
    let pane = h.editor().canvas.dbg_rect;
    h.perform(Action::AddDrawingAfter);
    settle(&mut h);
    assert!(h.editor().strip_open);
    let open = h.editor().canvas.dbg_rect;
    assert!((pane.height() - open.height() - rail::STRIP_H).abs() < 0.34, "the pane gives the strip its 56");
    assert_eq!(band_keys(&h), was, "no rail or transport control moved");
    // One refit: the frames after it hold still.
    let fit = h.editor().canvas.view_xform();
    h.run_frames(6);
    assert_eq!(h.editor().canvas.view_xform(), fit);
}

#[test]
fn austrip_add_drawing_waits_for_pen_up() {
    let mut h = strip_up(&[4, 4]);
    let at = h.editor().canvas.dbg_rect.center();
    h.press_and_move(at, at + vec2(30.0, 10.0), 3);
    assert!(h.editor().canvas.stroke_active());
    let library = h.state().cut().drawings.len();
    assert_eq!(h.perform(Action::AddDrawingAfter), Performed::Ignored);
    assert_eq!(h.state().cut().drawings.len(), library, "a live stroke keeps its target");
    h.release(at + vec2(30.0, 10.0));
    settle(&mut h);
    assert_eq!(h.perform(Action::AddDrawingAfter), Performed::Done);
    assert_eq!(h.state().cut().drawings.len(), library + 1);
}

#[test]
fn austrip_stop_settles_only_with_the_sheet_off_screen() {
    let mut h = strip_up(&[4, 4]);
    h.state_mut().view.playing = true;
    h.run_frames(1);
    h.state_mut().view.frame = 6;
    h.perform(Action::PlayPause);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 4, "off screen a stop lands on the run's first frame");
    h.editor_mut().show_sheet();
    settle(&mut h);
    h.state_mut().view.playing = true;
    h.run_frames(1);
    h.state_mut().view.frame = 6;
    h.perform(Action::PlayPause);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 6, "with the sheet up a stop stays frame exact");
}

/// `play_tick` runs before any panel: with the sheet and the strip both gone
/// the playhead still moves.
#[test]
fn austrip_hidden_panels_do_not_stop_playback() {
    let mut h = strip_up(&[8, 8]);
    h.editor_mut().strip_open = false;
    settle(&mut h);
    assert!(!visible_panes(&h.editor().dock).contains(&Pane::XSheet) && h.editor().strip_closed());
    h.perform(Action::PlayPause);
    h.run_frames(1);
    let at = h.state().view.frame;
    h.run_frames(1);
    assert!(h.state().view.playing);
    assert_ne!(h.state().view.frame, at, "the clock did not need a panel");
}

#[test]
fn austrip_no_own_cel_reason_is_the_table() {
    let mut s = timed(&[4, 4]);
    assert_eq!(s.no_own_cel_reason(), None, "a key of its own");
    s.goto(2);
    assert_eq!(s.no_own_cel_reason(), Some(HELD_FRAME), "held from frame 0");
    let mut blank = AppState::new_project("t", 64, 48, 24, 72.0);
    assert_eq!(blank.no_own_cel_reason(), Some(BLANK_FRAME), "nothing on it");
    blank.goto(3);
    assert_eq!(blank.no_own_cel_reason(), Some(BLANK_FRAME));
}

#[test]
fn austrip_held_and_blank_refusals_reach_all_four_sites() {
    let sites = include_str!("canvas.rs").matches(".no_own_cel_reason() {").count();
    let own = include_str!("doc.rs").matches("self.no_own_cel_reason() {").count();
    assert_eq!(sites, 3, "transform, draw-to-fill, fill");
    assert_eq!(own, 1, "refill");
}

#[test]
fn austrip_strip_predicate_table() {
    // (latched, sheet on screen, 3D or Brush maker) → stands.
    let table = [
        (true, false, false, true),
        (false, false, false, false),
        (true, true, false, false),
        (true, false, true, false),
        (false, true, false, false),
    ];
    for (latched, sheet, off, stands) in table {
        let mut h = strip_up(&[4, 4]);
        h.editor_mut().strip_open = latched;
        if sheet {
            h.editor_mut().show_sheet();
        }
        if off {
            h.perform(Action::Layout3dRoom);
        }
        settle(&mut h);
        let tall = |(p, r, _): &(Pane, Rect, _)| *p == Pane::XSheet && r.height() == rail::STRIP_H;
        let up = h.editor().pane_rects.iter().any(tall);
        assert_eq!(up, stands, "latched {latched}, sheet {sheet}, off {off}");
    }
    // A frame added later opens nothing: only a document does.
    let mut closed = EditorHarness::as_shipped();
    closed.editor_mut().strip_open = false;
    settle(&mut closed);
    closed.state_mut().set_cut_length(96);
    settle(&mut closed);
    assert!(!closed.editor().strip_open, "the latch has one writer at open");
    // It never closes by itself.
    let mut h = strip_up(&[4, 4]);
    h.state_mut().set_cut_length(1);
    settle(&mut h);
    assert!(h.editor().strip_open, "and it never closes by itself");
}

#[test]
fn austrip_census_counts_strip_cells() {
    let h = strip_up(&[1; 20]);
    assert_eq!(plate::strip_sheets(&h.ctx()).len(), 20, "every sheet on screen is one");
    let shapes = h.kit().output().shapes.clone();
    let reading = crate::measure::Reading::of_frame(&h.ctx(), &shapes);
    let rows = |r: &crate::measure::Reading| r.rows.iter().filter(|row| row.h == strip::SHEET_H).count();
    assert_eq!(rows(&reading), strip::CENSUS_CAP, "the census stops at 12");
    let mut h = EditorHarness::as_animation();
    h.kit_mut().set_size(vec2(1280.0, 800.0));
    settle(&mut h);
    h.hide_sheet();
    let shapes = h.kit().output().shapes.clone();
    let reading = crate::measure::Reading::of_frame(&h.ctx(), &shapes);
    assert_eq!(rows(&reading), 1, "a blank document adds one");
}

#[test]
fn austrip_every_icon_has_a_card_and_a_name() {
    let mut h = strip_up(&[4, 4]);
    let plain = [
        ("previous frame (S)", Action::PrevFrame),
        ("play (Space)", Action::PlayPause),
        ("next frame (F)", Action::NextFrame),
    ];
    for (key, action) in plain {
        h.hover(key);
        h.wait(plate::CARD_DELAY + 0.1);
        let words = crate::menus::tip(&h.ctx(), action);
        assert!(h.has(&words), "{key} has no card\n{}", h.describe());
    }
    // Against the window's edge the card stands off its key, and stays: one
    // pushed back onto its key takes the pointer from it, and blinks.
    let words = format!("How long each drawing holds, and whether the {} loops.", vocab::SHOT);
    h.hover("Speed");
    h.wait(plate::CARD_DELAY + 0.1);
    h.run_frames(3);
    assert!(h.has(&words), "Speed has no card\n{}", h.describe());
    assert!(!points(&h, &words).intersects(points(&h, "Speed")), "it covers its own key");
    full_transport(&mut h);
    for (key, action) in [("first frame (Home)", Action::FirstFrame), ("last frame (End)", Action::LastFrame)] {
        h.hover(key);
        h.wait(plate::CARD_DELAY + 0.1);
        let words = crate::menus::tip(&h.ctx(), action);
        assert!(h.has(&words), "{key} has no card\n{}", h.describe());
    }
    // A sheet has a name for a screen reader, and its drawing's name on its
    // card: it carries no words of its own.
    let mut h = strip_up(&[4, 4]);
    assert!(h.has("Drawing 1, frames 1 to 4"), "{}", h.describe());
    assert!(h.has("Drawing 2, frames 5 to 8"));
    let at = on_sheet(&h, 1);
    h.point_at(at);
    h.wait(plate::CARD_DELAY + 0.1);
    let name = h.state().cut().drawings[0].name.clone();
    let card = format!("Drawing 1 ({name}), frames 1 to 4");
    assert!(h.has_text_containing(&card), "{name} on the card\n{}", h.describe());
    assert!(h.has_text_containing("Drag the right edge"), "and how its edge works\n{}", h.describe());
    // One sheet, one name: an empty run's card says what its name says.
    let mut s = timed(&[8]);
    s.goto(0);
    assert!(s.add_drawing_after());
    let mut h = strip_of(s);
    let name = "Empty sheet, frames 3 to 8";
    assert!(h.has(name), "{}", h.describe());
    let at = on_sheet(&h, 4);
    h.point_at(at);
    h.wait(plate::CARD_DELAY + 0.1);
    let card = format!("{name}\nDrag the right edge");
    assert!(h.has_text_containing(&card), "the card's name\n{}", h.describe());
}

/// Against the window's right edge a card stands under a key in the upper
/// half and over one in the lower half, never on it.
#[test]
fn austrip_cards_stand_off_the_window_edge() {
    use egui::{Align, Layout, Order};
    for (align, under) in [(Align::Min, true), (Align::Max, false)] {
        let go = std::rc::Rc::new(std::cell::Cell::new(Rect::NOTHING));
        let held = go.clone();
        let mut p = PanelHarness::new(vec2(400.0, 200.0), (), move |ui, ()| {
            ui.with_layout(Layout::right_to_left(align), |ui| {
                let key = ui.button("Go");
                held.set(key.rect);
                plate::hover_card(key, || "Go — too many words to stand beside a key this far over".into());
            });
        });
        p.rest_on("Go", plate::CARD_DELAY + 0.05);
        let ctx = p.ctx();
        let shown = ctx.memory(|m| m.areas().visible_layer_ids());
        let cards = shown.iter().filter(|l| l.order == Order::Tooltip);
        let rect = cards.filter_map(|l| ctx.memory(|m| m.area_rect(l.id))).next();
        let (card, key) = (rect.expect("a card"), go.get());
        assert!(!card.intersects(key), "{card:?} on {key:?}");
        assert_eq!(card.top() >= key.bottom(), under, "{card:?} by {key:?}");
    }
}

/// The transport's cards read the chord the key has now.
#[test]
fn austrip_transport_reads_the_live_chord() {
    let mut h = strip_up(&[4, 4]);
    h.rig_mut().config.binding_mut(Action::NextFrame).chord = free_chord();
    settle(&mut h);
    assert!(!h.has("next frame (F)"), "{}", h.describe());
    assert!(h.has("next frame (Ctrl+Shift+T)"), "{}", h.describe());
}

#[test]
fn austrip_counter_tap_types_a_frame() {
    let mut h = strip_up(&[4, 4]);
    h.click("Frame 1 of 8");
    settle(&mut h);
    assert!(h.ctx().text_edit_focused(), "the counter is a field\n{}", h.describe());
    h.type_text("6");
    h.key(Key::Enter);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 5);
    assert!(!h.ctx().text_edit_focused());
    assert!(h.has("Frame 6 of 8"), "{}", h.describe());
    // Anything else leaves the frame where it was.
    h.click("Frame 6 of 8");
    settle(&mut h);
    h.type_text("2");
    h.key(Key::Escape);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 5);
    assert!(h.has("Frame 6 of 8"));
    // With the pointer on the strip the Enter that ends the number is still
    // the field's: the strip's chord does not hear it and open the sheet.
    h.click("Frame 6 of 8");
    settle(&mut h);
    h.type_text("3");
    let at = on_sheet(&h, 1);
    h.point_at(at);
    h.key(Key::Enter);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 2);
    assert!(h.performed().iter().all(|p| p.0 != Action::TypeIntoCell), "{:?}", h.performed());
    assert!(!h.editor().sheet_on_screen(), "the sheet stayed off");
}

#[test]
fn austrip_dropped_shows_only_above_zero() {
    let mut h = strip_up(&[4, 4]);
    assert!(!h.has_text_containing("dropped"));
    let keys = points(&h, "Add drawing (band)");
    h.editor_mut().play_dropped = 3;
    settle(&mut h);
    assert!(h.has("dropped 3"), "{}", h.describe());
    assert_eq!(points(&h, "Add drawing (band)"), keys, "the slot opens inside the lane");
}

#[test]
fn austrip_say_line_is_one_lane_and_wraps_to_two_rows() {
    const LONG: &str = "refused — this refusal is long enough to run past the end of a narrow lane, and so it wraps";
    let mut h = strip_up(&[4, 4]);
    h.kit_mut().set_size(vec2(832.0, 640.0));
    settle(&mut h);
    h.state_mut().status = "a word of chatter".into();
    settle(&mut h);
    assert!(h.has("a word of chatter"), "the remark speaks");
    h.state_mut().refuse(LONG);
    settle(&mut h);
    assert!(!h.has("a word of chatter"), "a refusal takes the lane");
    let words = points(&h, LONG);
    assert!(words.height() > 20.0, "two rows: {words:?}");
    let row = points(&h, "play (Space)");
    assert!(words.top() >= row.top() - 2.0 && words.bottom() <= row.bottom() + 2.0, "{words:?} in {row:?}");
    assert!(words.right() <= points(&h, "Add drawing (band)").left(), "{words:?}");
    h.wait(4.1);
    assert!(!h.has(LONG), "it dwells 4 s");
}

/// The Say lane: from the left lane's last key to the right lane's first.
fn say_lane(h: &EditorHarness) -> f32 {
    // AU19: Plan's sheet keeps the drawing keys.
    let full = h.editor().sheet_on_screen() && !h.editor().in_plan();
    let wide = h.ctx().content_rect().width() >= steps::FOLD;
    let counter = format!("Frame 1 of {}", h.state().frame_count());
    let (last, first) = match (full, wide) {
        (true, true) => ("clear loop range", vocab::SOUND_WHILE_DRAGGING),
        (true, false) => ("…", vocab::HIDE_TIMING_SHEET),
        (false, true) => (counter.as_str(), "Add drawing (band)"),
        (false, false) => ("…", "Add drawing (band)"),
    };
    points(h, first).left() - points(h, last).right()
}

/// Spec 4.4's floor on both transports, at every narrow size it names and
/// either side of the fold. The action slot is inside the lane.
#[test]
fn austrip_say_lane_holds_its_floor_at_every_size() {
    let sizes = [
        (832.0, 520.0),
        (853.0, 533.0),
        (924.0, 520.0),
        (1039.0, 640.0),
        (steps::FOLD, 640.0),
        (1092.0, 614.0),
        (steps::SOUND_WORDS - 1.0, 640.0),
        (steps::SOUND_WORDS, 640.0),
        (1280.0, 800.0),
        (1600.0, 900.0),
    ];
    let mut lanes = Vec::new();
    for (w, tall) in sizes {
        for sheet in [false, true] {
            let mut h = strip_up(&[4, 4]);
            h.kit_mut().set_size(vec2(w, tall));
            settle(&mut h);
            if sheet {
                h.editor_mut().show_sheet();
                settle(&mut h);
            }
            assert_eq!(h.editor().sheet_on_screen(), sheet);
            lanes.push((w, sheet, say_lane(&h)));
        }
    }
    println!("Say lanes (window, sheet up, lane): {lanes:?}");
    assert!(lanes.iter().all(|(.., lane)| *lane >= SAY_FLOOR), "{lanes:?}");
}

#[test]
fn austrip_narrow_fold_keeps_the_four() {
    let mut h = strip_up(&[4, 4]);
    h.kit_mut().set_size(vec2(832.0, 640.0));
    settle(&mut h);
    for key in ["play (Space)", "Frame 1 of 8", "Add drawing (band)", vocab::SHOW_TIMING_SHEET] {
        assert!(h.has(key), "{key} never folds\n{}", h.describe());
    }
    assert!(!h.has("Ghost frames toggle") && !h.has("Speed"));
    let lane = points(&h, "Add drawing (band)").left() - points(&h, "…").right();
    assert!(lane >= SAY_FLOOR, "the Say line's floor: {lane}");
    h.click("…");
    settle(&mut h);
    assert!(h.has("Ghost frames toggle") && h.has("Speed"), "{}", h.describe());
}

/// The fold's Speed works at every narrow size: a popover opened from inside
/// the `…` would shut both on its first click.
#[test]
fn austrip_speed_works_inside_the_fold() {
    for (w, tall) in [(832.0, 520.0), (853.0, 533.0), (924.0, 520.0)] {
        let mut h = strip_up(&[4, 4]);
        h.kit_mut().set_size(vec2(w, tall));
        settle(&mut h);
        let was = steps(&h);
        h.click("…");
        settle(&mut h);
        h.click("Medium");
        settle(&mut h);
        assert_eq!(holds(h.state()), [2, 2], "{w}: every drawing on twos\n{}", h.describe());
        assert_eq!(h.state().frame_count(), 4, "{w}: and the cut follows");
        assert_eq!(steps(&h), was + 1, "{w}: one undo");
        assert_eq!(h.state().status, "All drawings set to 2 frames each.");
        // The plain band's Loop row stands there too.
        let looped = h.state().view.loop_playback;
        if !h.has(vocab::LOOP) {
            h.click("…");
            settle(&mut h);
        }
        h.click(vocab::LOOP);
        settle(&mut h);
        assert_eq!(h.state().view.loop_playback, !looped, "{w}: the Loop row works");
        // And Ghost frames.
        let ghosts = h.state().view.onion;
        if !h.has("Ghost frames toggle") {
            h.click("…");
            settle(&mut h);
        }
        h.click("Ghost frames toggle");
        settle(&mut h);
        assert_eq!(h.state().view.onion, !ghosts, "{w}: Ghost frames works");
    }
}

#[test]
fn austrip_show_sheet_is_draws_switch_and_a_pane_elsewhere() {
    // The frozen boot's Draw keeps the switch that remembers.
    let mut h = EditorHarness::with_state(timed(&[4, 4]));
    h.editor_mut().sheet_in_draw = false;
    h.editor_mut().dock = Stage::Drawing.dock();
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    h.editor_mut().show_sheet();
    assert!(h.editor().sheet_in_draw, "Draw remembers");
    // Plan's sheet comes back to its recipe, and a verb saves nothing.
    let mut h = strip_up(&[4, 4]);
    h.editor_mut().show_sheet();
    settle(&mut h);
    assert!(crate::same_shape(&h.editor().dock, &Stage::Layout.dock()), "its own place at the left");
    assert!(h.editor().plan_sheet_hidden, "only the button is a choice");
    h.perform(Action::RoomFinishing);
    settle(&mut h);
    let remembered = h.editor().sheet_in_draw;
    h.editor_mut().show_sheet();
    settle(&mut h);
    assert!(visible_panes(&h.editor().dock).contains(&Pane::XSheet));
    assert_eq!(h.editor().sheet_in_draw, remembered, "elsewhere it is a pane");
}

// ---- gestures that must take nothing ----

/// The scrollbar stands on the strip's foot, over the sheets' last row. What
/// it takes is no press on a sheet: the view scrolls, no undo step is taken
/// and the cursor stays.
#[test]
fn austrip_scrollbar_is_no_sheet_press() {
    for (cut, head) in [(vec![1; 300], "Drawing 1, frame 1"), (vec![8; 40], "Drawing 1, frames 1 to 8")] {
        let mut h = strip_up(&cut);
        let (was, count) = (steps(&h), h.state().frame_count());
        let r = h.editor().strip.rect().expect("the strip stands");
        assert!(r.bottom() - 6.0 < r.top() + strip::BAR_H + strip::SHEET_H, "on the sheets' last row");
        assert!(h.has(head), "the shot's head on screen\n{}", h.describe());
        let grip = pos2(r.left() + 40.0, r.bottom() - 6.0);
        h.point_at(grip);
        h.drag(grip, grip + vec2(240.0, 0.0), 8);
        settle(&mut h);
        // By name: on ones at 8 points the sheets' rects can land just where
        // others stood.
        assert!(!h.has(head), "the bar took the drag");
        assert_eq!(steps(&h), was, "no undo step from the bar");
        assert_eq!(h.state().frame_count(), count, "and the cut's length stands");
        assert_eq!(h.state().view.frame, 0, "the cursor stays");
        // A click on its track pages the view, and no more.
        h.click_pos(pos2(r.right() - 12.0, r.bottom() - 6.0));
        settle(&mut h);
        assert_eq!(steps(&h), was, "no undo step from the track");
        assert_eq!(h.state().view.frame, 0, "the cursor stays");
    }
    // A short cut raises no bar: the gap under the sheets is still nothing.
    let mut h = strip_up(&[4, 4]);
    h.state_mut().goto(0);
    settle(&mut h);
    let r = h.editor().strip.rect().expect("the strip stands");
    let gap = pos2(on_sheet(&h, 5).x, r.bottom() - 2.0);
    assert!(gap.y >= r.top() + strip::BAR_H + strip::SHEET_H, "under the sheets");
    h.click_pos(gap);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 0, "the gap takes no press");
}

#[test]
fn austrip_locked_track_refuses_every_edit() {
    const LOCKED: &str = "refused — this column is locked";
    let mut h = strip_up(&[3, 4]);
    h.state_mut().toggle_column_locked();
    settle(&mut h);
    let was = steps(&h);
    // The last run's edge has no later key for the lock to catch.
    for end in [7, 3] {
        h.state_mut().refusal.clear();
        let edge = on_edge(&h, end);
        h.drag(edge, edge - vec2(2.0 * scale(&h), 0.0), 4);
        settle(&mut h);
        assert_eq!(holds(h.state()), [3, 4], "the edge at {end}");
        assert_eq!(h.state().refusal, LOCKED, "the edge at {end}");
    }
    h.state_mut().refusal.clear();
    assert!(!h.state_mut().add_drawing_after());
    assert_eq!(h.state().refusal, LOCKED);
    h.state_mut().refusal.clear();
    h.state_mut().set_speed(2);
    assert_eq!(h.state().refusal, LOCKED);
    assert_eq!(holds(h.state()), [3, 4]);
    assert_eq!(h.state().frame_count(), 7, "the cut stands");
    assert_eq!(steps(&h), was, "no undo step");
}

/// An undo can take the run an edge drag holds. The strip lets go: no panic
/// on the way, and the release writes nothing onto the run that came back.
#[test]
fn austrip_undo_under_an_edge_drag_lets_go() {
    // The release a frame after the undo, and in the very next one.
    for waits in [true, false] {
        let mut s = timed(&[3, 4, 4]);
        s.set_run_length(0, 10);
        assert_eq!(holds(&s), [10, 4, 4]);
        let mut h = bare(s);
        let edge = bare_sheet(&h, 10, -3.0);
        h.input_mut().events.push(Event::PointerMoved(edge));
        h.step();
        press(&mut h, edge, true);
        h.step();
        let pull = edge - vec2(8.0 * bare_scale(&h), 0.0);
        h.input_mut().events.push(Event::PointerMoved(pull));
        h.step();
        h.state_mut().0.undo();
        let at = h.state().0.engine.position();
        if waits {
            h.step();
        }
        press(&mut h, pull, false);
        h.step();
        assert_eq!(holds(&h.state().0), [3, 4, 4], "as the undo left it, waits {waits}");
        assert_eq!(h.state().0.engine.position(), at, "the release wrote nothing, waits {waits}");
    }
}

/// A paste lands after the strip has read its input, in the same frame, so it
/// can take the run under an edge drag where no press check has seen it. The
/// strip draws the run as the paste left it, and the release writes nothing.
#[test]
fn austrip_paste_under_an_edge_drag_lets_go() {
    let mut s = timed(&[2, 4, 4]);
    let column = s.view.active_column;
    s.select_cells((column, 0), column, 0);
    assert!(s.copy_cells());
    // The first drawing's cell lands in the second run, and splits it.
    s.select_cells((column, 3), column, 3);
    let mut h = bare(s);
    let text = h.state().0.sheet_ui.clip_text.clone().expect("the block, as text");
    let edge = bare_sheet(&h, 6, -3.0);
    h.input_mut().events.push(Event::PointerMoved(edge));
    h.step();
    press(&mut h, edge, true);
    h.step();
    let pull = edge + vec2(8.0 * bare_scale(&h), 0.0);
    h.input_mut().events.push(Event::PointerMoved(pull));
    h.step();
    let before = h.state().0.engine.position();
    h.input_mut().events.push(Event::Paste(text));
    h.step();
    let at = h.state().0.engine.position();
    assert_ne!(at, before, "the paste landed");
    let stored = holds(&h.state().0);
    assert_ne!(stored, [2, 4, 4], "and took the run");
    let scale = bare_scale(&h);
    let drawn: Vec<_> = plate::strip_sheets(&h.ctx).iter().map(|s| ((s.width() + 2.0) / scale).round() as u32).collect();
    assert_eq!(drawn, stored, "the strip draws what is stored");
    press(&mut h, pull, false);
    h.step();
    assert_eq!(h.state().0.engine.position(), at, "the release wrote nothing");
    assert_eq!(holds(&h.state().0), stored);
}

/// A cut another track holds longer: the last sheet's edge ends the run
/// inside the cut, and the cut stands. Nothing beside it, and the run's end
/// is the cut's end (`austrip_edge_drag_commits_once`).
#[test]
fn austrip_tail_edge_with_a_longer_track_beside_it() {
    let mut s = timed(&[4, 4]);
    let first = s.view.active_column;
    s.add_column();
    s.goto(7);
    s.new_drawing_at_frame();
    s.view.active_column = first;
    s.goto(0);
    let mut h = strip_of(s);
    let was = steps(&h);
    let edge = on_edge(&h, 8);
    h.drag(edge, edge - vec2(2.0 * scale(&h), 0.0), 4);
    settle(&mut h);
    assert_eq!(holds(h.state()), [4, 2, 2], "the rest of the run goes empty");
    assert_eq!(strip::sheets(h.state())[2].shows, Exposure::Empty);
    assert_eq!(h.state().frame_count(), 8, "the other track's key stays inside the cut");
    assert_eq!(steps(&h), was + 1, "one undo");
}

/// Delete aims at the sheet under the pointer (spec 7.2), and the hover says
/// so: it lights the one that goes, wherever the cursor stands.
#[test]
fn austrip_delete_takes_the_sheet_the_hover_lights() {
    let mut h = strip_up(&[4, 4, 4]);
    let near = on_sheet(&h, 1);
    h.click_pos(near);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 0, "the cursor on the first sheet");
    let at = on_sheet(&h, 9);
    h.point_at(at);
    settle(&mut h);
    let rects = measure::rects_of(&h.kit().output().shapes);
    let lit = rects.iter().filter(|r| r.fill == plate::HOVER && r.rect.contains(at)).count();
    assert_eq!(lit, 1, "the third sheet lights");
    let near = rects.iter().filter(|r| r.fill == plate::HOVER && r.rect.contains(on_sheet(&h, 1))).count();
    assert_eq!(near, 0, "the cursor's does not");
    h.key(Key::Delete);
    assert_eq!(holds(h.state()), [4, 8], "the lit sheet went, not the cursor's");
    assert_eq!(h.state().view.frame, 8, "and the cursor followed it");
}

// ---- stopping and starting ----

/// R19 at the end of a run: it settles like any stop, and the next Play still
/// rewinds. Parked on the last run's first frame the press is no dead one.
#[test]
fn austrip_natural_end_still_rewinds() {
    fn run_out(h: &mut EditorHarness) {
        h.state_mut().goto(90);
        h.perform(Action::PlayPause);
        for _ in 0..40 {
            if !h.state().view.playing {
                break;
            }
            h.run_frames(1);
        }
        assert!(!h.state().view.playing, "the run reached the end");
        settle(h);
        assert_eq!(h.state().view.frame, 48, "with the sheet off it settles on the run's start");
    }
    let mut h = strip_up(&[48, 48]);
    h.state_mut().view.loop_playback = false;
    run_out(&mut h);
    h.perform(Action::PlayPause);
    assert!(h.state().view.playing);
    assert!(h.state().view.frame < 48, "Play rewinds: frame {}", h.state().view.frame);
    // A stop by hand is a place to go on from.
    h.state_mut().goto(50);
    h.perform(Action::PlayPause);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 48);
    h.perform(Action::PlayPause);
    assert!(h.state().view.frame >= 48, "no rewind: frame {}", h.state().view.frame);
    h.perform(Action::PlayPause);
    settle(&mut h);
    // A cursor put there by hand, after an end, is one too.
    run_out(&mut h);
    h.state_mut().goto(48);
    h.perform(Action::PlayPause);
    assert!(h.state().view.frame >= 48, "no rewind: frame {}", h.state().view.frame);
}

// ---- Add drawing on a hold nobody chose ----

/// The first stroke on a blank shot holds across the whole cut. Add drawing
/// takes the next frame, for one frame: the cut stands and nothing doubles
/// (spec, Section 13).
#[test]
fn austrip_add_drawing_after_one_drawing_across_the_shot() {
    let shot = crate::newproject::NewProjectForm::default().new_state();
    let mut h = EditorHarness::with_state_shipped(shot);
    h.kit_mut().set_size(vec2(1280.0, 800.0));
    settle(&mut h);
    let count = h.state().frame_count();
    let at = h.editor().canvas.dbg_rect.center();
    h.drag(at, at + vec2(30.0, 10.0), 3);
    settle(&mut h);
    assert_eq!(holds(h.state()), [count], "one stroke, one drawing, the whole shot");
    let was = steps(&h);
    h.key_mods(Modifiers::ALT, Key::A);
    settle(&mut h);
    assert_eq!(holds(h.state()), [1, 1, count - 2]);
    assert_eq!(strip::sheets(h.state())[2].shows, Exposure::Empty, "the rest goes empty");
    assert_eq!(h.state().frame_count(), count, "the cut stands");
    assert_eq!(h.state().view.frame, 1, "the cursor on the new drawing");
    assert_eq!(steps(&h), was + 1, "one undo");
    assert!(h.has(&format!("Frame 2 of {count}")), "{}", h.describe());
    h.state_mut().undo();
    assert_eq!(holds(h.state()), [count]);
    assert_eq!(h.state().frame_count(), count);
    // From a frame inside it, the drawing keeps what the cursor has passed.
    let mut s = timed(&[48]);
    s.goto(9);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [10, 1, 37]);
    assert_eq!(s.view.frame, 10);
    // From its last frame the cut takes one more.
    let mut s = timed(&[48]);
    s.goto(47);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [48, 1]);
    assert_eq!(s.frame_count(), 49);
    // After that the track has a rhythm, and the hold copies again.
    let mut s = timed(&[48]);
    assert!(s.add_drawing_after());
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [1, 1, 1, 46]);
    assert_eq!(s.frame_count(), 49);
}

// ---- the band's words and cards ----

/// A card on screen: silent, as no stock tooltip is.
fn card(h: &EditorHarness) -> Option<Rect> {
    let ctx = h.ctx();
    let shown = ctx.memory(|m| m.areas().visible_layer_ids());
    let silent = |l: &&egui::LayerId| egui::AreaState::load(&ctx, l.id).is_some_and(|a| !a.interactable);
    let found = shown.iter().filter(|l| l.order == egui::Order::Tooltip).find(silent);
    found.and_then(|l| ctx.memory(|m| m.area_rect(l.id)))
}

/// Every control and readout under `top`, named, with a point on it. Words
/// that only read are no control: the frame field's `/ 8` and the Say line.
fn band_census(h: &EditorHarness, top: f32) -> Vec<(String, Pos2)> {
    use egui::accesskit::Role;
    use egui_kittest::kittest::NodeT;
    fn walk(n: egui_kittest::Node<'_>, top: f32, skip: [&str; 2], ppp: f32, out: &mut Vec<(String, Pos2)>) {
        let ak = n.accesskit_node();
        let keys = [Role::Button, Role::CheckBox, Role::RadioButton, Role::SpinButton, Role::Label];
        let name = ak.label().or_else(|| ak.value()).unwrap_or_default();
        let reads = ak.role() == Role::Label && (name.starts_with("/ ") || skip.contains(&name.as_str()));
        if let Some(at) = ak.bounding_box().filter(|_| keys.contains(&ak.role()) && !reads) {
            let at = pos2((at.x0 + at.x1) as f32 * 0.5 / ppp, (at.y0 + at.y1) as f32 * 0.5 / ppp);
            if at.y >= top {
                out.push((name, at));
            }
        }
        n.children().for_each(|c| walk(c, top, skip, ppp, out));
    }
    let mut out = Vec::new();
    let hint = h.editor().say.hint.as_ref().map_or("", |s| s.words.as_str());
    walk(h.kit().root(), top, [&h.state().status, hint], h.ctx().pixels_per_point(), &mut out);
    out
}

/// Which of `keys` stand bare: the pointer rests, and no card rises.
fn bare_keys(h: &mut EditorHarness, keys: &[(String, Pos2)]) -> Vec<String> {
    let mut none = Vec::new();
    for (name, at) in keys {
        h.point_at(*at);
        h.wait(plate::CARD_DELAY + 0.1);
        if card(h).is_none() {
            none.push(name.clone());
        }
    }
    none
}

/// Spec 4.5 and the gate `Icon-only with no card 0`: nothing in the band or
/// on the strip goes without a card, in either transport, on the row or in
/// the `…`. Two tracks, so the strip's chip stands.
#[test]
fn austrip_no_band_control_goes_without_a_card() {
    let tracks = || {
        let mut s = timed(&[4, 4]);
        let first = s.view.active_column;
        s.add_column();
        s.view.active_column = first;
        s.goto(0);
        s
    };
    let plain = ["play (Space)", "Frame 1 of 8", "Add drawing (band)", vocab::SHOW_TIMING_SHEET, "Track A"];
    let full = ["clear loop range", "24 fps", "—", vocab::SOUND_WHILE_DRAGGING, vocab::SHOW_TIMING_SHEET];
    let forms: [(f32, bool, &[&str]); 7] = [
        (1280.0, false, &plain),
        (832.0, false, &plain),
        (1280.0, true, &full),
        (steps::SOUND_WORDS - 1.0, true, &full),
        (steps::FOLD, true, &full),
        (832.0, true, &["…", vocab::LOOP, vocab::SHOW_TIMING_SHEET]),
        (1280.0, false, &["Speed", "Ghost frames toggle", "Drawing 2, frames 5 to 8"]),
    ];
    let mut tally = Vec::new();
    for (w, sheet, must) in forms {
        let mut h = strip_of(tracks());
        h.kit_mut().set_size(vec2(w, 800.0));
        settle(&mut h);
        if sheet {
            full_transport(&mut h);
        }
        let row = points(&h, "play (Space)").top() - 2.0;
        let top = if sheet { row } else { strip_rect(&h).top() };
        let keys = band_census(&h, top);
        for name in must {
            assert!(keys.iter().any(|(k, _)| k == name), "{name} at {w} in {keys:?}");
        }
        let none = bare_keys(&mut h, &keys);
        assert!(none.is_empty(), "at {w}, sheet {sheet}, {none:?} of {keys:?}");
        tally.push(keys.len());
    }
    println!("band controls and readouts, each form: {tally:?}, with no card: 0");
    // Plan's band beside its sheet: the plain keys, and the latch that hides it.
    let mut h = EditorHarness::with_state_shipped(tracks());
    h.kit_mut().set_size(vec2(1280.0, 800.0));
    settle(&mut h);
    let keys = band_census(&h, points(&h, "play (Space)").top() - 2.0);
    for name in ["Speed", "Ghost frames toggle", "Add drawing (band)", vocab::HIDE_TIMING_SHEET] {
        assert!(keys.iter().any(|(k, _)| k == name), "{name} in {keys:?}");
    }
    let none = bare_keys(&mut h, &keys);
    assert!(none.is_empty(), "in Plan: {none:?} of {keys:?}");
    // What the `…` holds, in both transports.
    for (sheet, inside) in [
        (false, &["Ghost frames toggle", vocab::LOOP][..]),
        (true, &["first frame (Home)", "last frame (End)", vocab::LOOP_START, vocab::LOOP_END, "clear loop range", "24 fps"][..]),
    ] {
        let mut h = strip_of(tracks());
        h.kit_mut().set_size(vec2(832.0, 800.0));
        settle(&mut h);
        if sheet {
            full_transport(&mut h);
        }
        h.click("…");
        settle(&mut h);
        let keys: Vec<_> = inside.iter().map(|k| (k.to_string(), points(&h, k).center())).collect();
        let none = bare_keys(&mut h, &keys);
        assert!(none.is_empty(), "in the fold, sheet {sheet}: {none:?}");
    }
}

/// Spec 4.5: the card replaces the stock tooltip on the band, and on the
/// sheet's own controls, which waited for this part.
#[test]
fn austrip_band_wears_no_stock_tooltip() {
    let main = include_str!("main.rs");
    let between = |from: &str, to: &str| -> Vec<&str> {
        let mut lines = main.lines().skip_while(|l| !l.contains(from));
        let first = lines.next();
        first.into_iter().chain(lines.take_while(|l| !l.contains(to))).collect()
    };
    let (keys, band) = (between("fn loop_card(", "fn ui("), between("fn transport_row(", "fn ui("));
    assert!(keys.iter().any(|l| l.contains("fn chord_card(")), "the slice reaches the keys");
    assert!(band.iter().any(|l| l.contains("fn sound_keys(")), "the slice reaches the band's last");
    let strip: Vec<_> = shipped(include_str!("strip.rs")).collect();
    let sheet: Vec<_> = shipped(include_str!("xsheet_panel.rs")).collect();
    assert!(sheet.len() > 1000, "the sheet's source: {}", sheet.len());
    for line in keys.iter().chain(&band).chain(&strip).chain(&sheet) {
        for stock in ["on_hover_text", "on_disabled_hover_text", "on_hover_ui", "on_disabled_hover_ui"] {
            assert!(!line.contains(stock), "{stock}: {line}");
        }
    }
}

/// The transport's row, read: the words painted along it.
fn row_words(h: &EditorHarness) -> Vec<String> {
    let window = h.ctx().content_rect();
    let row = Rect::from_min_max(pos2(window.left(), points(h, "play (Space)").top() - 4.0), window.max);
    measure::words_in(h, row)
}

/// Spec 4.4's words, through `vocab` so AU12's flip reaches them, the old
/// words still finding the keys in the palette, and a readout that says what
/// it counts.
#[test]
fn austrip_full_transport_reads_the_spec_words() {
    let mut h = strip_up(&[4, 4]);
    full_transport(&mut h);
    let words = row_words(&h);
    for word in [vocab::LOOP, vocab::LOOP_START, vocab::LOOP_END, vocab::SOUND_WHILE_DRAGGING, "24 fps"] {
        assert!(words.iter().any(|w| w == word), "{word} in {words:?}");
    }
    for old in ["loop", "in", "out", "scrub", "24.000"] {
        assert!(!words.iter().any(|w| w == old), "{old} in {words:?}");
    }
    // The palette finds each key by the old word and by the one on screen.
    let found = [
        (Action::LoopIn, ["in", "loop start"]),
        (Action::LoopOut, ["out", "loop end"]),
        (Action::ToggleScrub, ["scrub", "sound while dragging"]),
    ];
    for (action, words) in found {
        for word in words {
            assert!(action.aliases().contains(&word), "{word} still finds {action:?}");
        }
    }
    // With a range set a stopped run ends on it: Loop's card says both ends.
    h.hover(vocab::LOOP);
    h.wait(plate::CARD_DELAY + 0.1);
    assert!(h.has_text_containing("the end of the loop range or the"), "{}", h.describe());
    // Spec 4.5's one shape, in the band's word and with the chord it has now.
    let word = vocab::LOOP_START;
    let shape = match crate::menus::chord_label(&h.ctx(), Action::LoopIn) {
        Some(chord) => format!("{word} ({chord}) — "),
        None => format!("{word} — "),
    };
    h.hover(word);
    h.wait(plate::CARD_DELAY + 0.1);
    assert!(h.has_text_containing(&shape), "{shape}\n{}", h.describe());
    // Just over the fold the key is its icon: the name and the card keep the
    // words, and the Say line keeps its floor.
    let word = vocab::SOUND_WHILE_DRAGGING;
    let mut h = strip_up(&[4, 4]);
    h.kit_mut().set_size(vec2(steps::FOLD, 800.0));
    settle(&mut h);
    full_transport(&mut h);
    assert!(!row_words(&h).iter().any(|w| w == word), "{:?}", row_words(&h));
    h.hover(word);
    h.wait(plate::CARD_DELAY + 0.1);
    let painted = measure::reading(&h).texts;
    assert!(painted.iter().any(|(_, t)| t.contains(word)), "{word} on its card");
}

#[test]
fn austrip_speed_rows_say_what_they_set() {
    let mut h = strip_up(&[1, 1, 1]);
    h.click("Speed");
    settle(&mut h);
    let words = painted_words(&h);
    let at = |text: &str| {
        let found = words.iter().find(|(t, ..)| t == text);
        found.unwrap_or_else(|| panic!("{text} on its row: {words:?}")).1
    };
    // Each fact beside its word on one line, the facts in one column.
    let rows = [("Fast", "1 frame each"), ("Medium", "2 frames each"), ("Slow", "3 frames each")];
    for (word, fact) in rows {
        assert!((at(word).center().y - at(fact).center().y).abs() < 2.0, "{word}, {fact}: {words:?}");
        assert!(at(word).right() + 8.0 < at(fact).left(), "{word}, {fact}: {words:?}");
        assert!((at(fact).left() - at(rows[1].1).left()).abs() < 2.0, "{fact} in the column: {words:?}");
    }
    assert!(at("Fast").bottom() < at("Medium").top() && at("Medium").bottom() < at("Slow").top());
    let fact = at("3 frames each").center();
    // One of three, the one in force marked. Asked with the popover open: a
    // row gone with it reads None too.
    let marked = |h: &EditorHarness| ["Fast", "Medium", "Slow"].map(|row| h.node_toggled(row));
    assert_eq!(marked(&h), [Some(true), Some(false), Some(false)]);
    h.click("Medium");
    settle(&mut h);
    assert_eq!(h.state().status, "All drawings set to 2 frames each.");
    h.click("Speed");
    settle(&mut h);
    assert_eq!(marked(&h), [Some(false), Some(true), Some(false)]);
    // The row is one control: its fact answers too.
    h.click_pos(fact);
    settle(&mut h);
    assert_eq!(h.state().status, "All drawings set to 3 frames each.");
}

/// Spec 8.2: the action button has its end of the lane: a remark stops short
/// of the slot, and a hint's words stop short of the button that stands there.
/// AU7: no button stands beside a refusal, which outranks every hint, and its
/// gate needs the slot for the longest ones at 832: a refusal stops at the
/// lane's end, with no button in it.
#[test]
fn austrip_say_words_leave_the_action_slot_free() {
    const LONG: &str = "this remark is long enough to run the whole lane and on past where a button would stand";
    let mut h = strip_up(&[4, 4]);
    h.kit_mut().set_size(vec2(832.0, 640.0));
    settle(&mut h);
    // The slot holds the longest of 8.2's words as a band key: the 10 pt pad
    // of every button either side, and the gap before it.
    let font = plate::Role::Panel.font();
    let width = |h: &EditorHarness, word: &str| {
        h.ctx().fonts_mut(|f| f.layout_no_wrap(word.to_string(), font.clone(), plate::ON_FILL).size().x)
    };
    let verb = Verb::GoToDraw;
    for kind in ProjectKind::ALL {
        let word = width(&h, verb.label(Some(kind)));
        assert!(plate::SAY_ACTION >= word + 20.0 + 6.0, "{} for {kind:?}'s word of {word}", plate::SAY_ACTION);
    }
    let word = width(&h, verb.label(h.editor().kind()));
    let slot = points(&h, "Add drawing (band)").left() - plate::SAY_ACTION;
    h.state_mut().status = LONG.into();
    settle(&mut h);
    assert!(points(&h, LONG).right() <= slot, "{:?} past {slot}", points(&h, LONG));
    let name = format!("{} (say line)", verb.label(h.editor().kind()));
    let spoken = Spoken { bit: None, words: LONG.into(), verb: Some(verb) };
    h.editor_mut().say.pin = Some(spoken);
    h.wait(4.2);
    settle(&mut h);
    let button = points(&h, &name);
    assert!((button.width() - (word + 20.0)).abs() < 1.0, "{button:?} for a word of {word}");
    assert!(points(&h, LONG).right() <= button.left(), "{:?} past {button:?}", points(&h, LONG));
    assert!(button.right() <= points(&h, "Add drawing (band)").left(), "{button:?}");
    let words = format!("refused — {LONG}");
    h.state_mut().refuse(words.clone());
    settle(&mut h);
    assert!(!h.has(&name), "a refusal outranks the hint, and its button");
    let lane = points(&h, "Add drawing (band)").left();
    assert!(points(&h, &words).right() <= lane, "{:?} past {lane}", points(&h, &words));
}

/// The rule beside the Say line stands only with words beside it, and takes
/// no slot away from anything when it goes.
#[test]
fn austrip_say_rule_stands_only_beside_words() {
    let mut h = strip_up(&[4, 4]);
    h.state_mut().status.clear();
    h.rig_mut().config.ui.hints_off = true;
    settle(&mut h);
    let (left, right) = (points(&h, "Frame 1 of 8").right(), points(&h, "Add drawing (band)").left());
    let row = points(&h, "play (Space)").y_range();
    let rules = |h: &EditorHarness| {
        let lane = |[a, b]: [Pos2; 2]| a.x == b.x && (left..right).contains(&a.x) && row.contains(a.y);
        let lines = painted(h).into_iter().filter_map(|s| match s {
            egui::Shape::LineSegment { points, .. } => Some(points),
            _ => None,
        });
        lines.filter(|l| lane(*l)).count()
    };
    let still = band_keys(&h);
    assert_eq!(rules(&h), 0, "a quiet lane has no rule");
    h.state_mut().status = "Hold set.".into();
    settle(&mut h);
    assert_eq!(rules(&h), 1, "words have one");
    assert_eq!(band_keys(&h), still, "and nothing moved");
}

/// Every shape the frame painted, out of its `Vec`s.
fn painted(h: &EditorHarness) -> Vec<egui::Shape> {
    fn walk(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            s => out.push(s.clone()),
        }
    }
    let mut out = Vec::new();
    h.kit().output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// The texts the frame painted, with their ink.
fn painted_words(h: &EditorHarness) -> Vec<(String, Rect, egui::Color32)> {
    let ink = |t: &egui::epaint::TextShape| {
        let colour = t.galley.job.sections.first().map_or(t.fallback_color, |s| s.format.color);
        let colour = if colour == plate::SECTION_PLACEHOLDER { t.fallback_color } else { colour };
        t.override_text_color.unwrap_or(colour)
    };
    let texts = painted(h).into_iter().filter_map(|s| match s {
        egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.visual_bounding_rect(), ink(&t))),
        _ => None,
    });
    texts.collect()
}

/// The peg bar's labels read on its steel. Its ticks stay white (spec 7.2).
#[test]
fn austrip_peg_labels_wear_the_bars_own_ink() {
    let h = strip_up(&[48, 48]);
    let r = strip_rect(&h);
    let bar = Rect::from_min_size(r.min, vec2(r.width(), strip::BAR_H));
    let labels: Vec<_> = painted_words(&h).into_iter().filter(|(t, at, _)| t.ends_with(" s") && bar.intersects(*at)).collect();
    assert!(!labels.is_empty(), "the strip marks its seconds");
    assert!(labels.iter().all(|(.., ink)| *ink == plate::STEEL_INK), "{labels:?}");
    let ticks = strokes(&h, plate::ON_FILL, 1.0);
    assert!(ticks.iter().any(|[a, b]| bar.contains(*a) && bar.contains(*b)), "white: {ticks:?}");
}

/// The number and the badge stand on a row of their own, 2 pt clear of the
/// peg hole, on a wide sheet and a narrow one.
#[test]
fn austrip_used_twice_stands_clear_of_the_peg_hole() {
    for hold in [24, 6] {
        let mut s = timed(&[hold, hold, hold]);
        s.goto(0);
        s.view.selected_drawing = s.current_drawing();
        s.goto(2 * hold);
        s.expose_selected();
        assert_eq!(holds(&s), [hold, hold, hold]);
        s.goto(0);
        let h = strip_of(s);
        let r = strip_rect(&h);
        let holes: Vec<_> = painted(&h).into_iter().filter_map(|s| match s {
            egui::Shape::Circle(c) if c.fill == plate::DESK && r.contains(c.center) => Some(c),
            _ => None,
        }).collect();
        assert_eq!(holes.len(), 3, "a hole a sheet");
        let words: Vec<_> = painted_words(&h).into_iter().filter(|(t, at, _)| {
            r.intersects(*at) && (t == "Used twice" || t.parse::<u32>().is_ok())
        }).collect();
        assert!(words.iter().filter(|(t, ..)| t.parse::<u32>().is_ok()).count() >= 3, "{words:?}");
        if hold == 24 {
            assert_eq!(words.iter().filter(|(t, ..)| t == "Used twice").count(), 2, "{words:?}");
        }
        for (t, at, _) in &words {
            for hole in &holes {
                let gap = at.distance_to_pos(hole.center) - hole.radius;
                assert!(gap >= 2.0, "{t} stands {gap} from the hole at {:?}", hole.center);
            }
        }
    }
}

/// The straight strokes the frame painted in this ink and weight.
fn strokes(h: &EditorHarness, ink: egui::Color32, width: f32) -> Vec<[Pos2; 2]> {
    let lines = painted(h).into_iter().filter_map(|s| match s {
        egui::Shape::LineSegment { points, stroke } if stroke.color == ink && stroke.width == width => Some(points),
        _ => None,
    });
    lines.collect()
}

/// The current sheet says its edge pulls: two upright ticks inside the 8 pt
/// that take the drag, where a sheet has room for them beside its number.
#[test]
fn austrip_current_sheet_shows_its_edge() {
    let mut h = strip_up(&[4, 4]);
    h.state_mut().goto(0);
    settle(&mut h);
    let sheet = plate::strip_sheets(&h.ctx())[0];
    let ticks = strokes(&h, plate::BLUE_DEEP, 1.0);
    assert_eq!(ticks.len(), 2, "{ticks:?}");
    for [a, b] in ticks {
        assert_eq!(a.x, b.x, "upright");
        assert!((sheet.right() - 8.0..sheet.right()).contains(&a.x), "{a:?} on the edge of {sheet:?}");
    }
    let mut h = strip_up(&[1; 300]);
    h.state_mut().goto(0);
    settle(&mut h);
    assert!(plate::strip_sheets(&h.ctx())[0].width() < 32.0, "a narrow sheet is current");
    assert!(strokes(&h, plate::BLUE_DEEP, 1.0).is_empty(), "its number keeps the room");
}

/// An empty run's dashes stand inside the current sheet's 2 pt outline, so
/// the outline never hides them.
#[test]
fn austrip_dashes_stand_inside_the_outline() {
    let mut s = timed(&[8]);
    s.goto(0);
    assert!(s.add_drawing_after());
    s.goto(4);
    let h = strip_of(s);
    let empty = plate::strip_sheets(&h.ctx())[2];
    let dashes = strokes(&h, plate::INK3, 1.0);
    let dashes: Vec<_> = dashes.iter().filter(|[a, _]| empty.contains(*a)).collect();
    assert!(dashes.len() > 8, "the outline is dashed: {dashes:?}");
    let inside = empty.shrink(2.0);
    assert!(dashes.iter().all(|[a, b]| inside.contains(*a) && inside.contains(*b)), "{dashes:?} in {inside:?}");
}

// ---- icons ----

/// What `icon` paints: the kind of each shape, and whether it fills.
fn icon_shapes(icon: crate::icons::Icon) -> Vec<(&'static str, bool, usize)> {
    let box_at = Rect::from_min_size(pos2(40.0, 40.0), vec2(24.0, 24.0));
    let mut kit = egui_kittest::Harness::new_ui(move |ui| {
        crate::icons::paint(ui.painter(), box_at, plate::INK, icon);
    });
    kit.run();
    let mut out = Vec::new();
    for clipped in &kit.output().shapes {
        match &clipped.shape {
            egui::Shape::Path(p) => out.push(("path", p.fill.a() > 0, p.points.len())),
            // The panel's own ground is no part of the icon.
            egui::Shape::Rect(r) if box_at.contains_rect(r.rect) => out.push(("rect", true, 4)),
            _ => {}
        }
    }
    out
}

/// Play is one solid triangle. Next frame is a bar and a thin chevron, so the
/// two keys beside each other never read as the same one.
#[test]
fn austrip_play_and_next_wear_different_icons() {
    use crate::icons::Icon;
    let (play, next, back) = (icon_shapes(Icon::Play), icon_shapes(Icon::StepFwd), icon_shapes(Icon::StepBack));
    assert_eq!(play, [("path", true, 3)], "the one solid shape");
    let solid = |shapes: &[(&str, bool, usize)]| shapes.iter().any(|s| s.0 == "path" && s.1);
    assert!(!solid(&next) && !solid(&back), "{next:?} {back:?}");
    assert_eq!(next, back, "the two steps are one shape, mirrored");
    assert_ne!(play, next);
}

// ---- what this part stored, and what it left out ----

/// Saved data is sacred. AU5 added two verbs and nothing else: an old config
/// takes them in and loses nothing, and the words the files hold have not
/// moved.
#[test]
fn austrip_saved_vocabulary_is_additive() {
    use crate::config::{Config, UiConfig};
    // A config from before this part, Alt+A already the artist's own key.
    let dir = tempfile::tempdir().unwrap();
    let mut v = serde_json::to_value(Config::default()).unwrap();
    let binds = v["keybinds"].as_array_mut().unwrap();
    binds.retain(|b| !matches!(b["action"].as_str(), Some("AddDrawingAfter" | "GoToFrame")));
    for b in binds.iter_mut().filter(|b| b["action"] == "ToggleLoop") {
        b["chord"] = serde_json::json!({ "keys": ["A"], "ctrl": false, "shift": false, "alt": true });
    }
    std::fs::write(config::config_path_in(dir.path()), serde_json::to_vec_pretty(&v).unwrap()).unwrap();
    let (cfg, notes) = Config::load_from(dir.path());
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(cfg.keybinds.len(), Action::ALL.len(), "both verbs are filled in");
    let chord = |a| cfg.binding(a).and_then(|b| b.chord.clone());
    assert_eq!(chord(Action::AddDrawingAfter), None, "Alt+A was taken");
    assert_eq!(chord(Action::GoToFrame), None);
    let label = cfg.binding(Action::ToggleLoop).and_then(|b| b.label());
    assert_eq!(label.as_deref(), Some("Alt+A"), "the artist's key stays");
    assert_eq!(config::clashes(&cfg.keybinds), vec![], "no key fires two verbs");
    // The stored names.
    let name = |a| serde_json::to_string(&a).unwrap();
    assert_eq!(name(Action::AddDrawingAfter), "\"AddDrawingAfter\"");
    assert_eq!(name(Action::GoToFrame), "\"GoToFrame\"");
    assert_eq!(serde_json::to_string(&Pane::XSheet).unwrap(), "\"XSheet\"");
    let rooms = [
        (Some(Stage::Layout), "layout"),
        (Some(Stage::Drawing), "drawing"),
        (Some(Stage::InkPaint), "finishing"),
        (Some(Stage::Composite), "composite"),
        (Some(Stage::Edit), "edit"),
        (Some(Stage::Manga), "manga"),
        (None, "custom"),
    ];
    for (room, key) in rooms {
        assert_eq!(crate::stage_key(room), key);
    }
    // The ui section's shape.
    let ui = serde_json::to_value(UiConfig::default()).unwrap();
    let mut keys: Vec<_> = ui.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    let shape = [
        "exact_descriptions", "hints_off", "hints_seen", "keymap_rev", "lock_positions", "quick_start", "scale",
        "sheet_in_draw", "show_welcome", "whats_new_seen",
    ];
    assert_eq!(keys, shape);
    // A strip's edits are plain exposures: the file takes them as it stands.
    let mut s = timed(&[3, 2, 4]);
    s.set_run_length(0, 5);
    s.goto(5);
    assert!(s.add_drawing_after());
    s.set_speed(2);
    let file = dir.path().join("cut.gaban");
    s.begin_save(file.clone());
    assert!(s.wait_save(), "the save landed");
    let back = AppState::load_from(file).expect("the file opens");
    let cells = |s: &AppState| serde_json::to_value(&s.cut().xsheet).unwrap();
    assert_eq!(cells(&back), cells(&s), "the exposures, as they were");
    assert_eq!(back.frame_count(), s.frame_count());
}

/// Lines of `text` above its test module.
fn shipped(text: &'static str) -> impl Iterator<Item = &'static str> {
    text.lines().take_while(|l| !l.starts_with("#[cfg(test)]"))
}

/// The Lead's rule, by the source: the strip and the band open no window, and
/// the pictures of them are test code. Everything else here drives them
/// headless.
#[test]
fn austrip_strip_and_band_open_no_window() {
    let main = include_str!("main.rs");
    let mut band = main.lines().skip_while(|l| !l.contains("fn transport_row(")).peekable();
    assert!(band.peek().is_some(), "the band's source");
    let band = band.take_while(|l| !l.contains("        pen: &PenConfig,"));
    // Cut in two, as `auseam_nothing_under_test_opens_a_window` has them: that
    // scan reads this file too.
    let doors = [
        concat!("show_", "viewport"),
        concat!("Viewport", "Builder"),
        concat!("run_", "native"),
        "rfd::",
        "process::Command",
        "open::that",
    ];
    for line in band.chain(shipped(include_str!("strip.rs"))) {
        for door in doors {
            assert!(!line.contains(door), "{door}: {line}");
        }
    }
    let lines: Vec<_> = main.lines().collect();
    let at = lines.iter().position(|l| *l == "mod uishot;").expect("the shots' module");
    assert_eq!(lines[at - 1], "#[cfg(test)]", "the pictures never ship");
}

/// Spec, AU5's Leave out: no thumbnails, no drag reorder, one active track,
/// no hints, no menu moves.
#[test]
fn austrip_left_out_stays_out() {
    let mut s = timed(&[4, 2, 6]);
    let first = s.view.active_column;
    s.add_column();
    s.goto(3);
    s.new_drawing_at_frame();
    s.view.active_column = first;
    s.goto(0);
    let mut h = strip_of(s);
    let r = strip_rect(&h);
    // Thumbnails: a sheet paints no image.
    let images = measure::images_of(&h.kit().output().shapes);
    assert!(!images.iter().any(|at| at.intersects(r)), "{images:?}");
    // One active track: one row of sheets, its runs and no others.
    let sheets = plate::strip_sheets(&h.ctx());
    assert_eq!(sheets.len(), 3, "{sheets:?}");
    assert!(sheets.iter().all(|at| at.top() == sheets[0].top()), "{sheets:?}");
    // Drag reorder: a sheet's body dragged over the next one moves nothing.
    let was = steps(&h);
    let (from, to) = (on_sheet(&h, 1), on_sheet(&h, 9));
    h.drag(from, to, 6);
    settle(&mut h);
    assert_eq!(holds(h.state()), [4, 2, 6]);
    assert_eq!(steps(&h), was, "a sheet's body is no handle");
    // Hints off: at rest the Say lane is empty.
    h.state_mut().status.clear();
    h.state_mut().refusal.clear();
    h.rig_mut().config.ui.hints_off = true;
    settle(&mut h);
    let (left, right) = (points(&h, "Frame 1 of 12").right(), points(&h, "Add drawing (band)").left());
    let row = points(&h, "play (Space)");
    let lane = Rect::from_x_y_ranges(left..=right, row.y_range());
    assert_eq!(measure::words_in(&h, lane), Vec::<String>::new(), "a quiet lane");
    // Menu moves: the new verb stands where the spec left it for now.
    assert_eq!(Action::AddDrawingAfter.menu(), Some("Sheet/Add drawing"));
}
