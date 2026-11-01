//! AU26's tests: an animation opens with rows to draw on, and the blank frames
//! left after its last drawing neither play nor export (feedback spec 3.7 to
//! 3.9). The prefix is `autail_`. No test here opens a dialog: a dialog's
//! answer is `h.answer_export`.

use std::path::Path;

use anim_core::command::Command;
use anim_core::graph::NodeKind;
use anim_core::xsheet::{ColumnDef, ColumnKind, Exposure};
use eframe::egui;
use egui::vec2;

use crate::canvas::extpaper_tests::paint_texels;
use crate::config::Action;
use crate::doc::{self, AppState};
use crate::render_queue::{self, ExportKind};
use crate::steps::{self, ProjectKind};
use crate::testkit::EditorHarness;
use crate::welcome::WelcomeAction;
use crate::workspace::Stage;
use crate::{AppRequest, Performed, plate, strip, templates, vocab};

const INK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const TICK: f32 = 1.0 / 12.0;
const GIF: &str = "Animated GIF";

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

fn sized(h: &mut EditorHarness, w: f32, tall: f32) {
    h.kit_mut().set_size(vec2(w, tall));
    h.kit_mut().set_pixels_per_point(1.0);
    settle(h);
}

/// `n` drawings on an animation of `rows` rows, laid down as a person lays
/// them: a stroke, then `Add drawing` and a stroke, each with ink of its own,
/// on paper small enough that a job ends quickly.
fn drawn(rows: u32, n: u32) -> AppState {
    let mut s = AppState::new_project("tail", 64, 36, 12, 72.0);
    ProjectKind::Animation.stamp(&mut s.engine.project);
    s.set_cut_length(rows);
    s.new_drawing_at_frame();
    paint_texels(&mut s, &[(2, 18), (3, 18)], INK);
    for pose in 1..n as i32 {
        assert!(s.add_drawing_after());
        paint_texels(&mut s, &[(2 + 4 * pose, 18), (3 + 4 * pose, 18)], INK);
    }
    s.goto(0);
    s
}

/// A shot whose active track holds one drawing for each of `holds`.
fn timed(holds: &[u32]) -> AppState {
    let mut s = AppState::new_project("t", 64, 36, 12, 72.0);
    ProjectKind::Animation.stamp(&mut s.engine.project);
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

/// `timed`, with `frame` on the active track blanked: a gap, or a tail.
fn gap_at(mut s: AppState, frame: u32) -> AppState {
    let (at, column) = (s.at(), s.view.active_column);
    let cmd = Command::SetCell { at, column, frame, key: Some(Exposure::Empty) };
    s.engine.apply("gap", vec![cmd]).expect("the gap lands");
    s
}

/// A sound on `s`'s cut.
fn sound(s: &mut AppState) {
    let (at, audio) = (s.at(), Some(crate::playback::tests::clip()));
    s.engine.apply("set audio", vec![Command::SetCutAudio { at, audio }]).expect("the clip is set");
}

fn holds(state: &AppState) -> Vec<u32> {
    strip::sheets(state).iter().map(|s| s.len).collect()
}

fn shipped(state: AppState) -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(state);
    h.state_mut().render_queue.ffmpeg = Some((String::new(), None));
    sized(&mut h, 1600.0, 900.0);
    h
}

fn timing(state: AppState) -> EditorHarness {
    let mut h = shipped(state);
    assert_eq!(h.perform(Action::RoomEdit), Performed::Done);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Edit));
    h
}

/// A track laid over `s`, holding one drawing from frame 0 to the cut's end.
fn track(s: &mut AppState, kind: ColumnKind, hidden: bool) {
    let (at, active) = (s.at(), s.view.active_column);
    let column = s.engine.add_column(at, "BG").expect("a column");
    s.view.active_column = column;
    s.goto(0);
    s.new_drawing_at_frame();
    s.view.active_column = active;
    let def = ColumnDef { kind, hidden, ..ColumnDef::default() };
    s.engine.project.cut_mut(at.scene, at.cut).expect("the cut").column_defs.insert(column, def);
}

/// The frames a run shows from `cursor`, Play pressed as a person presses it,
/// on the UI clock (there is no sound here), for at most `ticks` ticks. No
/// frame runs between the press and the first tick, or that frame's own tick
/// would pass unseen.
fn run_from(h: &mut EditorHarness, cursor: u32, ticks: u32) -> Vec<u32> {
    h.state_mut().goto(cursor);
    h.run_frames(1);
    assert_eq!(h.editor_mut().perform(Action::PlayPause), Performed::Done);
    let mut seen = vec![h.state().view.frame];
    for _ in 0..ticks {
        if !h.state().view.playing {
            break;
        }
        h.editor_mut().play_tick(TICK);
        let now = h.state().view.frame;
        if seen.last() != Some(&now) {
            seen.push(now);
        }
    }
    if h.state().view.playing {
        assert_eq!(h.perform(Action::PlayPause), Performed::Done);
    }
    seen
}

fn export(h: &mut EditorHarness, kind: ExportKind, path: &Path) {
    let ctx = h.ctx();
    h.editor_mut().export_to(&ctx, kind, path);
    h.run_export_to_end();
    settle(h);
}

fn gif(file: &Path) -> crate::gif_decode::Gif {
    crate::gif_decode::decode(&std::fs::read(file).unwrap()).unwrap()
}

fn longer() -> String {
    format!("{} (band)", vocab::LONGER)
}

fn shorter() -> String {
    format!("{} (band)", vocab::SHORTER)
}

// ---- twelve rows (spec 3.7) ----

#[test]
fn autail_new_animation_has_twelve_rows() {
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1600.0, 900.0);
    h.answer_start(WelcomeAction::Intent(templates::ANIMATION));
    settle(&mut h);
    assert_eq!(h.state().frame_count(), 12, "one second at 12 fps");
    assert_eq!(h.state().fps(), 12);
    assert!(h.has("Frame 1 of 12"), "{}", h.describe());
    assert!(h.editor().in_plan() && h.editor().sheet_on_screen(), "the rows are on screen to draw on");
    assert!(!h.state().is_dirty(), "a fresh blank is not dirty");
    assert_eq!(h.state().engine.project.default_cut_frames, Some(12), "the next cut too");
    // A key pose lands on any row, and a held row splits on the stroke's route.
    h.state_mut().goto(7);
    h.state_mut().new_drawing_at_frame();
    assert_eq!(holds(h.state()), [7, 5]);
    assert_eq!(h.state().frame_count(), 12);
    // The other cards and the ready paper stay at one frame.
    for i in [templates::PICTURE, templates::PICTURE_SQUARE, templates::PICTURE_TALL] {
        assert_eq!(templates::INTENTS[i].cut_frames, 1, "{}", templates::INTENTS[i].name);
    }
    assert_eq!(templates::READY.cut_frames, 1);
}

// ---- rule one: Add drawing (spec 3.8) ----

#[test]
fn autail_add_drawing_takes_the_blank_tail() {
    // The spec's own row: the old test read 46 and 49.
    let mut s = timed(&[48]);
    assert!(s.add_drawing_after());
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [1, 1, 1, 45]);
    assert_eq!(s.frame_count(), 48);
    assert_eq!(s.view.frame, 2);
    // Twelve poses on twelve rows: nothing is left and nothing was added.
    let s = drawn(12, 12);
    assert_eq!(holds(&s), [1; 12]);
    assert_eq!(s.frame_count(), 12);
    // More blank than the hold: an empty key stands after the new drawing.
    let mut s = gap_at(timed(&[2, 2, 5]), 4);
    s.goto(2);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [2, 2, 2, 3]);
    assert_eq!(s.frame_count(), 9);
    let last = *strip::sheets(&s).last().unwrap();
    assert_eq!((last.from, last.shows), (6, Exposure::Empty));
    assert_eq!(s.view.frame, 4, "the cursor stands on the new drawing");
    // As much blank as the hold: taken whole, with no empty key after it.
    let mut s = gap_at(timed(&[2, 2, 2]), 4);
    s.goto(2);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [2, 2, 2]);
    assert_eq!(s.frame_count(), 6);
    // Less blank than the hold: taken whole, and the rest is inserted.
    let mut s = gap_at(timed(&[3, 3, 1]), 6);
    s.goto(3);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [3, 3, 3]);
    assert_eq!(s.frame_count(), 9);
}

#[test]
fn autail_add_drawing_keeps_what_follows_a_gap() {
    // A gap between two drawings is blank frames too: what follows it moves
    // only by what the gap could not give.
    let mut s = gap_at(timed(&[2, 3, 2]), 2);
    let after = strip::sheets(&s)[2].shows;
    s.goto(0);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [2, 2, 1, 2]);
    assert_eq!(s.frame_count(), 7);
    assert_eq!(strip::sheets(&s)[3].shows, after);
    let mut s = gap_at(timed(&[3, 1, 2]), 3);
    let after = strip::sheets(&s)[2].shows;
    s.goto(0);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [3, 3, 2]);
    assert_eq!(s.frame_count(), 8);
    assert_eq!(strip::sheets(&s)[2].shows, after);
}

#[test]
fn autail_add_drawing_is_one_undo_and_rides_the_loop_only_when_it_inserts() {
    let mut s = gap_at(timed(&[2, 2, 5]), 4);
    s.goto(6);
    s.set_loop_in();
    s.goto(8);
    s.set_loop_out();
    let (before, position) = (s.cut().clone(), s.engine.position());
    s.goto(2);
    assert!(s.add_drawing_after());
    assert_eq!(s.engine.position(), position + 1, "one undo");
    assert_eq!(s.loop_range(), Some((6, 8)), "no frame moved, so the range stays");
    s.undo();
    assert_eq!(s.cut().xsheet, before.xsheet, "and it comes back whole");
    assert_eq!(s.frame_count(), 9);
    // Inserted frames ripple the range from where they go in.
    let mut s = gap_at(timed(&[3, 1, 2]), 3);
    s.goto(4);
    s.set_loop_in();
    s.goto(5);
    s.set_loop_out();
    s.goto(0);
    assert!(s.add_drawing_after());
    assert_eq!(s.loop_range(), Some((6, 7)));
}

// ---- rule two: Play (spec 3.8) ----

#[test]
fn autail_blank_tail_never_plays() {
    let mut h = shipped(drawn(12, 3));
    assert_eq!(holds(h.state()), [1, 1, 1, 9]);
    assert_eq!(h.state().shown_length(), 3);
    assert_eq!(h.state().play_range(), (0, 2));
    h.state_mut().view.loop_playback = false;
    assert_eq!(run_from(&mut h, 0, 40), [0, 1, 2]);
    assert!(!h.state().view.playing, "it stops on the last drawing");
    assert_eq!(h.state().view.frame, 2);
    // Pressed again where it stopped, it rewinds and runs them again.
    assert_eq!(run_from(&mut h, 2, 40), [0, 1, 2]);
    // Looping wraps at the last drawing, on either clock.
    h.state_mut().view.loop_playback = true;
    assert_eq!(run_from(&mut h, 0, 7), [0, 1, 2, 0, 1, 2, 0, 1]);
    let mut s = drawn(12, 3);
    s.view.playing = true;
    s.view.loop_playback = true;
    for _ in 0..7 {
        s.tick(TICK);
        assert!(s.view.frame < 3, "{}", s.view.frame);
    }
    s.view.loop_playback = false;
    for _ in 0..7 {
        s.tick(TICK);
    }
    assert_eq!((s.view.frame, s.view.playing), (2, false));
    // The tail stays: the counter, the sheet and the keys still reach it.
    assert_eq!(h.state().frame_count(), 12);
    h.state_mut().goto(11);
    settle(&mut h);
    assert!(h.has("Frame 12 of 12"), "{}", h.describe());
}

#[test]
fn autail_cursor_on_the_tail_plays_no_blanks() {
    let mut h = shipped(drawn(12, 3));
    h.state_mut().view.loop_playback = false;
    for cursor in 2..12 {
        assert_eq!(run_from(&mut h, cursor, 40), [0, 1, 2], "from frame {cursor}");
    }
    assert_eq!(run_from(&mut h, 1, 40), [1, 2], "play from the cursor never jumps back");
    assert_eq!(h.state().run_range(9), (0, 2));
    // Looping from the tail stays off it too.
    h.state_mut().view.loop_playback = true;
    assert_eq!(run_from(&mut h, 9, 4), [0, 1, 2, 0, 1]);
}

#[test]
fn autail_blank_cut_with_a_sound_or_a_source_plays_whole() {
    // A blank cut has no last drawing, so nothing marks a tail on it.
    let blank = templates::INTENTS[templates::ANIMATION].build();
    assert_eq!(blank.shown_length(), 1, "at least 1");
    // A source the Result reads is on every frame with no drawing to end it.
    let mut s = AppState::new_project("plate", 64, 36, 12, 72.0);
    assert!(s.auto_comp());
    s.set_cut_length(12);
    assert_eq!(s.shown_length(), 1);
    let output = s.cut().graph.nodes().find(|n| n.kind == NodeKind::Output).expect("the composite").id;
    s.graph_add_node(NodeKind::image_sequence("renders/shot_####.png"), (0.0, 90.0));
    let frames = s.selected_node.expect("the new node");
    assert_eq!(s.shown_length(), 1, "reaching nothing, it shows nothing");
    s.graph_connect(frames, output, 0);
    assert_eq!(s.shown_length(), 12, "the Result shows it on every frame");
    // A sound is there to be heard before anything is drawn on it.
    let mut s = AppState::new_project("voice", 64, 36, 12, 72.0);
    s.set_cut_length(12);
    assert_eq!(s.shown_length(), 1);
    sound(&mut s);
    assert_eq!(s.shown_length(), 12);
}

#[test]
fn autail_episode_plays_each_cut_whole() {
    let mut s = drawn(12, 3);
    s.view.loop_playback = false;
    s.board.episode_play = true;
    s.view.playing = true;
    for _ in 0..20 {
        s.tick(TICK);
    }
    assert_eq!(s.view.frame, 11, "the edit reads the cut's length, not its drawings");
}

#[test]
fn autail_lip_form_keeps_the_whole_cut() {
    // Lip flap writes over the sound, not over the frames that play.
    let mut s = drawn(48, 3);
    sound(&mut s);
    let form = crate::playback::LipForm::of(&s).expect("a track");
    assert_eq!((form.from, form.to), (0, 47));
}

// ---- the tracks that count (spec 3.8) ----

#[test]
fn autail_background_keeps_the_tail() {
    let mut s = drawn(12, 3);
    track(&mut s, ColumnKind::Bg, false);
    assert_eq!(s.shown_length(), 12, "a background holding to the end means no tail");
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("shot.gif");
    let mut h = shipped(s);
    h.state_mut().view.loop_playback = false;
    assert_eq!(run_from(&mut h, 0, 40), (0..12).collect::<Vec<_>>());
    export(&mut h, ExportKind::Gif, &file);
    assert_eq!(gif(&file).duration_cs(), 100, "the whole second");
    // A track that does not render counts for nothing.
    let mut s = drawn(12, 3);
    track(&mut s, ColumnKind::Bg, true);
    assert_eq!(s.shown_length(), 3, "hidden");
    let mut s = drawn(12, 3);
    track(&mut s, ColumnKind::Ref, false);
    assert_eq!(s.shown_length(), 3, "a reference");
    // A track that ends on an empty key ends there.
    let mut s = drawn(12, 3);
    track(&mut s, ColumnKind::Cel, false);
    let (at, column) = (s.at(), s.cut().xsheet.columns[1].id);
    let cmd = Command::SetCell { at, column, frame: 6, key: Some(Exposure::Empty) };
    s.engine.apply("end", vec![cmd]).expect("the key lands");
    assert_eq!(s.shown_length(), 6);
    // Keys past the cut stay inside it.
    let mut s = drawn(12, 3);
    s.set_cut_length(2);
    assert_eq!(s.shown_length(), 2);
}

#[test]
fn autail_loop_range_wins_over_the_tail() {
    let mut h = shipped(drawn(12, 3));
    h.state_mut().view.loop_playback = true;
    h.state_mut().goto(4);
    h.state_mut().set_loop_in();
    h.state_mut().goto(7);
    h.state_mut().set_loop_out();
    assert_eq!(h.state().play_range(), (4, 7), "inside the cut it wins, blanks and all");
    assert_eq!(run_from(&mut h, 4, 6), [4, 5, 6, 7, 4, 5, 6]);
    h.state_mut().view.loop_playback = false;
    assert_eq!(run_from(&mut h, 4, 20), [4, 5, 6, 7]);
    assert!(!h.state().view.playing);
    // Past the cut it never counted, and the end is the last drawing again.
    h.state_mut().set_cut_length(6);
    assert_eq!(h.state().play_range(), (0, 2));
    // A range inside the drawings wins as well.
    let mut h = shipped(drawn(12, 3));
    h.state_mut().goto(1);
    h.state_mut().set_loop_in();
    h.state_mut().goto(1);
    h.state_mut().set_loop_out();
    assert_eq!(h.state().play_range(), (1, 1));
}

// ---- rule two: plain exports (spec 3.8) ----

#[test]
fn autail_blank_tail_never_exports() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = shipped(drawn(12, 3));
    let file = dir.path().join("three.gif");
    export(&mut h, ExportKind::Gif, &file);
    let three = gif(&file);
    assert_eq!(three.frames.len(), 3);
    assert_eq!(three.duration_cs(), 25, "a quarter of a second at 12 fps");
    assert_ne!(three.frames[0].rgb, three.frames[2].rgb, "the last drawing is in it");
    // A sequence on the same route: three files.
    let pictures = dir.path().join("pictures");
    std::fs::create_dir(&pictures).unwrap();
    export(&mut h, ExportKind::Png, &pictures);
    assert_eq!(std::fs::read_dir(&pictures).unwrap().count(), 3);
    // The Export step's card takes the same route.
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    h.click(GIF);
    settle(&mut h);
    assert!(h.rig().requests.iter().any(|r| *r == AppRequest::ExportTo(ExportKind::Gif)));
    let card = dir.path().join("card.gif");
    h.answer_export(&card);
    h.run_export_to_end();
    assert_eq!(gif(&card).frames.len(), 3);
    // `File ▸ Export`'s moved range holds over the tail, past it or short of it.
    h.editor_mut().export_range = Some((1, 9));
    let range = dir.path().join("range.gif");
    export(&mut h, ExportKind::Gif, &range);
    let job = h.state().render_queue.queue.jobs.last().expect("the job");
    assert_eq!((job.frames, job.total()), (Some((1, 9)), 9));
    assert_eq!(gif(&range).duration_cs(), 75, "nine frames, blanks and all");
    // A shot with no tail asks for no range: its job stays the whole cut.
    let mut h = shipped(drawn(3, 3));
    let whole = dir.path().join("whole.gif");
    export(&mut h, ExportKind::Gif, &whole);
    assert_eq!(h.state().render_queue.queue.jobs[0].frames, None);
    assert_eq!(gif(&whole).frames.len(), 3);
}

#[test]
fn autail_six_drawings_at_medium_make_a_second() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = drawn(12, 6);
    state.set_speed(2);
    assert_eq!(holds(&state), [2, 2, 2, 2, 2, 2, 6], "Speed keeps the blank at full length");
    assert_eq!(state.frame_count(), 18);
    assert_eq!(state.shown_length(), 12);
    let mut h = shipped(state);
    let file = dir.path().join("six.gif");
    export(&mut h, ExportKind::Gif, &file);
    let job = h.state().render_queue.queue.jobs.last().expect("the job");
    assert_eq!((job.frames, job.total()), (Some((0, 11)), 12));
    // The file keeps a picture that stays as one image with a longer delay
    // (`gif.rs`), so 12 frames of six drawings are six images, two frames each.
    let six = gif(&file);
    assert_eq!(six.frames.len(), 6);
    assert!(six.frames.iter().all(|f| (16..=17).contains(&f.delay_cs)));
    assert_eq!(six.duration_cs(), 100, "1.0 s");
}

#[test]
fn autail_queue_keeps_the_whole_cut() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = drawn(12, 3);
    s.render_queue.ffmpeg = Some((String::new(), None));
    s.render_queue.draft.out = dir.path().display().to_string();
    let at = s.at();
    assert!(render_queue::add_job(&mut s, at, None), "the queue's form");
    let job = &s.render_queue.queue.jobs[0];
    assert_eq!((job.frames, job.span), (None, (0, 11)), "a deliberate blank is made here");
    let mut h = shipped(s);
    h.state_mut().render_queue.queue.jobs.clear();
    // The menu's queue rows read that menu's range, as the sheet of it says.
    let ctx = h.ctx();
    h.editor_mut().queue_export(&ctx, ExportKind::Png, dir.path());
    let job = &h.state().render_queue.queue.jobs[0];
    assert_eq!((job.frames, job.total()), (None, 12));
}

// ---- Longer and Shorter (spec 3.9) ----

#[test]
fn autail_longer_grows_the_last_run() {
    let mut h = timing(timed(&[2, 3]));
    h.state_mut().goto(2);
    settle(&mut h);
    let was = h.state().engine.position();
    h.click(&longer());
    settle(&mut h);
    assert_eq!(holds(h.state()), [2, 4]);
    assert_eq!(h.state().frame_count(), 6, "the cut grows by one");
    assert_eq!(h.state().engine.position(), was + 1, "one undo");
    assert_eq!(h.state().view.frame, 2, "the cursor stays");
    // The Action and its refusal are unchanged.
    let why = "the last drawing already holds to the end of the cut";
    assert_eq!(h.editor().can_perform(Action::HoldPlus), Err(why));
    assert_eq!(h.perform(Action::HoldPlus), Performed::Ignored);
    h.state_mut().undo();
    assert_eq!(holds(h.state()), [2, 3]);
    assert_eq!(h.state().frame_count(), 5);
    // On an earlier run what follows ripples, and it acts at the cursor.
    h.state_mut().goto(1);
    settle(&mut h);
    h.click(&longer());
    settle(&mut h);
    assert_eq!(holds(h.state()), [3, 3]);
    assert_eq!(h.state().frame_count(), 6);
}

#[test]
fn autail_shorter_stops_at_one() {
    let mut h = timing(timed(&[1, 3]));
    h.state_mut().goto(1);
    settle(&mut h);
    for len in [2, 1] {
        let was = h.state().engine.position();
        h.click(&shorter());
        settle(&mut h);
        assert_eq!(holds(h.state()), [1, len]);
        assert_eq!(h.state().frame_count(), 1 + len);
        assert_eq!(h.state().engine.position(), was + 1, "one undo");
    }
    let was = h.state().engine.position();
    let refused = h.state().refusal_seq;
    h.click(&shorter());
    settle(&mut h);
    assert_eq!(holds(h.state()), [1, 1], "one frame is the shortest");
    assert_eq!(h.state().frame_count(), 2);
    assert_eq!(h.state().engine.position(), was, "nothing changed, so nothing to undo");
    assert_eq!(h.state().refusal_seq, refused, "and nothing was refused");
    assert_eq!(h.state().status, doc::SHORTEST_HOLD, "but the press is answered");
    // A cut of one drawing goes down to one frame, and no further.
    let mut h = timing(timed(&[2]));
    h.click(&shorter());
    settle(&mut h);
    h.click(&shorter());
    settle(&mut h);
    assert_eq!(holds(h.state()), [1]);
    assert_eq!(h.state().frame_count(), 1);
}

#[test]
fn autail_hold_keys_stand_only_in_timing_and_never_fold() {
    let mut h = timing(timed(&[4, 4]));
    let (long, short) = (longer(), shorter());
    assert!(h.has(&long) && h.has(&short), "{}", h.describe());
    // The spec's row, left to right.
    let (a, b) = (h.rect(&long), h.rect(&short));
    let sound = h.rect(vocab::SOUND_WHILE_DRAGGING);
    assert!(a.right() <= b.left() && b.right() <= sound.left(), "{a:?} {b:?} {sound:?}");
    // Their cards name the chords.
    for (key, action) in [(&long, Action::HoldPlus), (&short, Action::HoldMinus)] {
        h.hover(key);
        h.wait(plate::CARD_DELAY + 0.1);
        let words = crate::menus::tip(&h.ctx(), action);
        assert!(h.has(&words), "{key} has no card\n{}", h.describe());
    }
    // Under the fold the sound goes behind `…`, and these stay on the row.
    let narrow = steps::FOLD - 40.0;
    sized(&mut h, narrow, 640.0);
    assert!(h.has(&long) && h.has(&short), "{}", h.describe());
    assert!(!h.has(vocab::SOUND_WHILE_DRAGGING), "{}", h.describe());
    let (a, b) = (h.rect(&long), h.rect(&short));
    assert!(a.left() >= 0.0 && b.right() <= narrow, "{a:?} {b:?}");
    let window = h.ctx().content_rect();
    assert!(window.contains_rect(a) && window.contains_rect(b));
    // No other step has them, the full band of Color's sheet among them.
    sized(&mut h, 1600.0, 900.0);
    for action in [Action::RoomDrawing, Action::RoomFinishing, Action::RoomComposite, Action::RoomExport] {
        assert_eq!(h.perform(action), Performed::Done);
        settle(&mut h);
        assert!(!h.has(&long) && !h.has(&short), "{action:?}\n{}", h.describe());
    }
    h.perform(Action::RoomFinishing);
    h.editor_mut().show_sheet();
    settle(&mut h);
    assert!(h.editor().sheet_on_screen());
    assert!(!h.has(&long) && !h.has(&short), "{}", h.describe());
}

#[test]
fn autail_hold_keys_refuse_as_the_sheet_does() {
    // A locked track, in the sheet's words.
    let mut s = timed(&[2, 3]);
    let (at, column) = (s.at(), s.view.active_column);
    let locked = ColumnDef { locked: true, ..ColumnDef::default() };
    s.engine.project.cut_mut(at.scene, at.cut).unwrap().column_defs.insert(column, locked);
    let mut h = timing(s);
    let was = holds(h.state());
    h.click(&longer());
    settle(&mut h);
    assert_eq!(holds(h.state()), was);
    assert_eq!(h.state().refusal, "refused — this column is locked");
    // Nothing exposed under the cursor: nothing to hold.
    let mut h = timing(AppState::new_project("t", 64, 36, 12, 72.0));
    let count = h.state().frame_count();
    h.click(&shorter());
    settle(&mut h);
    assert_eq!(h.state().frame_count(), count);
    assert_eq!(h.state().refusal, "refused — nothing is exposed on this frame");
    // The sheet never changes under a live stroke.
    let mut h = timing(timed(&[2, 3]));
    let panel = crate::manga_tools::KomaDrag { from: [0.0; 2], to: [1.0; 2], handle: None, reach: 0.0 };
    h.editor_mut().canvas.koma = Some(panel);
    assert!(h.editor().canvas.stroke_active());
    h.click(&longer());
    settle(&mut h);
    assert_eq!(holds(h.state()), [2, 3]);
    assert_eq!(h.state().refusal, doc::FINISH_STROKE);
}
