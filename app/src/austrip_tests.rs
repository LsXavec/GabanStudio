//! AU5's tests: the simple timeline and the band (spec 7.2, 4.4, 8.2).

use anim_core::command::Command;
use anim_core::xsheet::Exposure;
use eframe::egui;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use egui_kittest::kittest::Queryable;

use crate::config::{self, Action};
use crate::doc::{AppState, BLANK_FRAME, HELD_FRAME};
use crate::strip::{self, Sheet, Strip};
use crate::testkit::{EditorHarness, PanelHarness};
use crate::workspace::Stage;
use crate::{Pane, Performed, plate, rail, vocab, visible_panes};

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

/// `timed`, shipped: the sheet off screen and the strip up.
fn strip_up(holds: &[u32]) -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(timed(holds));
    h.kit_mut().set_size(vec2(1280.0, 800.0));
    settle(&mut h);
    assert!(!h.editor().sheet_on_screen() && h.editor().strip_open, "the strip is up");
    h
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

/// A chord the defaults leave free.
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
    assert!(!h.has_text_containing(&format!("{name}, ")), "{}", h.describe());
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
    assert!(!h.has("first frame (Home)") && !h.has("loop in") && !h.has("scrub"));
    h.editor_mut().show_sheet();
    settle(&mut h);
    for key in ["first frame (Home)", "last frame (End)", "loop", "loop in", "loop out", "scrub"] {
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
    h.editor_mut().show_sheet();
    settle(&mut h);
    assert_eq!(points(&h, "play (Space)").bottom(), up.bottom(), "the full row stands on the same edge");
}

/// The transport and the rail's keys, by name.
fn band_keys(h: &EditorHarness) -> Vec<(String, Rect)> {
    let names = ["play (Space)", "Add drawing (band)", vocab::SHOW_TIMING_SHEET, "Brush", "Eraser", "Layers"];
    names.iter().map(|n| (n.to_string(), points(h, n))).collect()
}

#[test]
fn austrip_full_band_latch_moves_nothing() {
    let mut h = EditorHarness::as_shipped();
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    settle(&mut h);
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
    let mut h = EditorHarness::as_shipped();
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    settle(&mut h);
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
    let mut h = EditorHarness::with_state_shipped(timed(&[1; 20]));
    h.kit_mut().set_size(vec2(1280.0, 800.0));
    settle(&mut h);
    assert_eq!(plate::strip_sheets(&h.ctx()).len(), 20, "every sheet on screen is one");
    let shapes = h.kit().output().shapes.clone();
    let reading = crate::measure::Reading::of_frame(&h.ctx(), &shapes);
    let rows = |r: &crate::measure::Reading| r.rows.iter().filter(|row| row.h == strip::SHEET_H).count();
    assert_eq!(rows(&reading), strip::CENSUS_CAP, "the census stops at 12");
    let mut h = EditorHarness::as_shipped();
    h.kit_mut().set_size(vec2(1280.0, 800.0));
    settle(&mut h);
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
    let words = "How long each drawing holds, and whether the shot loops.";
    h.hover("Speed");
    h.wait(plate::CARD_DELAY + 0.1);
    h.run_frames(3);
    assert!(h.has(words), "Speed has no card\n{}", h.describe());
    assert!(!points(&h, words).intersects(points(&h, "Speed")), "it covers its own key");
    h.editor_mut().show_sheet();
    settle(&mut h);
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
    assert!(h.has_text_containing(&format!("{name}, ")), "{name} on the card\n{}", h.describe());
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
    assert!(lane >= 240.0, "the Say line's floor: {lane}");
    h.click("…");
    settle(&mut h);
    assert!(h.has("Ghost frames toggle") && h.has("Speed"), "{}", h.describe());
}

#[test]
fn austrip_show_sheet_is_draws_switch_and_a_pane_elsewhere() {
    let mut h = strip_up(&[4, 4]);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    h.editor_mut().show_sheet();
    assert!(h.editor().sheet_in_draw, "Draw remembers");
    h.perform(Action::RoomFinishing);
    settle(&mut h);
    let remembered = h.editor().sheet_in_draw;
    h.editor_mut().show_sheet();
    settle(&mut h);
    assert!(visible_panes(&h.editor().dock).contains(&Pane::XSheet));
    assert_eq!(h.editor().sheet_in_draw, remembered, "elsewhere it is a pane");
}
