//! AU26's tests: an animation opens with rows to draw on, and the blank frames
//! left after its last drawing neither play nor export (feedback spec 3.7 to
//! 3.9). The prefix is `autail_`. No test here opens a dialog: a dialog's
//! answer is `h.answer_export`.

use std::path::Path;

use anim_core::command::Command;
use anim_core::graph::NodeKind;
use anim_core::model::{Stroke, StrokePoint};
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

/// The two roads a stroke lands by (main spec 1.5(11)), `x` apart on the paper.
const ROADS: [fn(&mut AppState, i32); 2] = [
    |s, x| {
        let pt = |x: i32| StrokePoint { x: x as f32, y: 18.0, pressure: 1.0, tilt: [0.0, 0.0] };
        s.commit_stroke(Stroke { points: vec![pt(x), pt(x + 2)], base_width: 2.0, color: [0, 0, 0, 255] });
    },
    |s, x| paint_texels(s, &[(x, 18), (x + 1, 18)], INK),
];

/// An animation of 12 rows with a stroke on each of `rows`, made the way Plan's
/// Say line teaches: click a row, draw on it.
fn clicked(rows: &[u32], draw: fn(&mut AppState, i32)) -> AppState {
    let mut s = AppState::new_project("tail", 64, 36, 12, 72.0);
    ProjectKind::Animation.stamp(&mut s.engine.project);
    s.set_cut_length(12);
    for row in rows {
        s.goto(*row);
        draw(&mut s, 2 + 4 * *row as i32);
    }
    s.goto(0);
    s
}

/// A shot whose active track holds one drawing for each of `holds`.
pub(crate) fn timed(holds: &[u32]) -> AppState {
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
pub(crate) fn gap_at(mut s: AppState, frame: u32) -> AppState {
    let (at, column) = (s.at(), s.view.active_column);
    let cmd = Command::SetCell { at, column, frame, key: Some(Exposure::Empty) };
    s.engine.apply("gap", vec![cmd]).expect("the gap lands");
    s
}

/// A sound `ms` long on `s`'s cut.
fn sound(s: &mut AppState, ms: usize) {
    let (at, audio) = (s.at(), Some(crate::playback::tests::clip_of(ms)));
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
fn autail_add_drawing_from_a_blank_row() {
    // The band's main button on a new animation: row 1 takes it.
    let mut s = templates::INTENTS[templates::ANIMATION].build();
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [1, 11]);
    assert_eq!((s.frame_count(), s.view.frame, s.shown_length()), (12, 0, 1));
    // Parked on the tail, where Next past the last drawing lands, the tail's
    // first row takes it, or the rows before the cursor would play blank.
    for cursor in [3, 4, 11] {
        let mut s = drawn(12, 3);
        s.goto(cursor);
        assert!(s.add_drawing_after());
        assert_eq!(holds(&s), [1, 1, 1, 1, 8], "from frame {cursor}");
        assert_eq!((s.frame_count(), s.view.frame, s.shown_length()), (12, 3, 4), "from frame {cursor}");
    }
    // A gap gives what it has, what it has over stays blank, and what follows
    // moves only by what it could not give.
    let mut s = gap_at(timed(&[2, 3, 2]), 2);
    s.goto(3);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [2, 2, 1, 2]);
    assert_eq!((s.frame_count(), s.view.frame), (7, 2));
    let mut s = gap_at(timed(&[3, 1, 2]), 3);
    s.goto(3);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [3, 3, 2]);
    assert_eq!(s.frame_count(), 8);
    // A blank drawing's one frame is no row to give: the cut must grow, or
    // the verb could not make an animation of it (spec 2.7).
    let mut s = templates::READY.build();
    assert!(s.add_drawing_after());
    assert_eq!(s.frame_count(), 2);
    // Neither Play nor the GIF reaches past the new drawing.
    let dir = tempfile::tempdir().unwrap();
    let mut s = drawn(12, 3);
    s.goto(8);
    assert!(s.add_drawing_after());
    paint_texels(&mut s, &[(20, 18)], INK);
    let mut h = shipped(s);
    h.state_mut().view.loop_playback = false;
    assert_eq!(run_from(&mut h, 0, 40), [0, 1, 2, 3]);
    let file = dir.path().join("four.gif");
    export(&mut h, ExportKind::Gif, &file);
    let job = h.state().render_queue.queue.jobs.last().expect("the job");
    assert_eq!((job.frames, job.total()), (Some((0, 3)), 4));
    assert_eq!(gif(&file).frames.len(), 4);
}

#[test]
fn autail_add_drawing_after_a_key_pose_doubles_nothing() {
    // A first drawing that holds to the cut's end never chose that hold,
    // wherever it starts.
    for draw in ROADS {
        let mut s = clicked(&[4], draw);
        assert_eq!(holds(&s), [4, 8]);
        s.goto(4);
        assert!(s.add_drawing_after());
        assert_eq!(holds(&s), [4, 1, 1, 6]);
        assert_eq!(s.frame_count(), 12);
    }
    // A second drawing's hold is the rhythm, so it copies as it did.
    let mut s = timed(&[4, 8]);
    s.goto(4);
    assert!(s.add_drawing_after());
    assert_eq!(holds(&s), [4, 8, 8]);
}

#[test]
fn autail_click_a_row_and_draw_keeps_the_tail() {
    for draw in ROADS {
        // Rows 1, 2 and 3, as the Say line teaches: what Add drawing leaves.
        let s = clicked(&[0, 1, 2], draw);
        assert_eq!(holds(&s), [1, 1, 1, 9]);
        assert_eq!(s.shown_length(), 3);
        assert_eq!(strip::sheets(&s)[3].shows, Exposure::Empty);
        // A key pose: what stands before it holds up to it, and the last
        // keeps to its row. No white frame plays between two poses.
        assert_eq!(holds(&clicked(&[0, 4, 8], draw)), [4, 4, 1, 3]);
        assert_eq!(holds(&clicked(&[4, 8], draw)), [4, 4, 1, 3]);
        // The last row has nothing after it to blank.
        assert_eq!(holds(&clicked(&[0, 11], draw)), [11, 1]);
        // One undo, the blank key with it.
        let mut s = clicked(&[0, 1], draw);
        let (before, position) = (s.cut().xsheet.clone(), s.engine.position());
        s.goto(2);
        draw(&mut s, 30);
        assert_eq!(s.engine.position(), position + 1);
        s.undo();
        assert_eq!(s.cut().xsheet, before);
        // The track's first drawing runs on: nothing came before it to end on.
        assert_eq!(holds(&clicked(&[0], draw)), [12]);
        // A hold a person set splits as it always has, and a blank a person
        // left stays blank: a stroke rewrites no row before its own.
        let mut s = timed(&[2, 3]);
        s.goto(3);
        draw(&mut s, 30);
        assert_eq!(holds(&s), [2, 1, 2]);
        let mut s = gap_at(timed(&[6, 6]), 6);
        s.goto(9);
        draw(&mut s, 30);
        assert_eq!(holds(&s), [6, 3, 1, 2]);
        // Play and the GIF stop on the third drawing, as they do by Add drawing.
        let dir = tempfile::tempdir().unwrap();
        let mut h = shipped(clicked(&[0, 1, 2], draw));
        h.state_mut().view.loop_playback = false;
        assert_eq!(run_from(&mut h, 0, 40), [0, 1, 2]);
        let file = dir.path().join("three.gif");
        export(&mut h, ExportKind::Gif, &file);
        assert_eq!(gif(&file).duration_cs(), 25, "a quarter of a second at 12 fps");
    }
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
    // A sound is there to be heard before anything is drawn on it, to its end.
    let mut s = AppState::new_project("voice", 64, 36, 12, 72.0);
    s.set_cut_length(12);
    assert_eq!(s.shown_length(), 1);
    sound(&mut s, 1000);
    assert_eq!(s.shown_length(), 12);
    sound(&mut s, 250);
    assert_eq!(s.shown_length(), 3);
}

#[test]
fn autail_play_with_nothing_on_the_cut_says_so() {
    let mut h = shipped(templates::INTENTS[templates::ANIMATION].build());
    let refused = h.state().refusal_seq;
    assert_eq!(h.editor_mut().perform(Action::PlayPause), Performed::Done);
    assert!(!h.state().view.playing, "the key stays on Play");
    assert_eq!(h.state().status, doc::NOTHING_TO_PLAY);
    assert_eq!(h.state().refusal_seq, refused, "a press answered, not a refusal");
    h.run_frames(2);
    assert!(h.has(doc::NOTHING_TO_PLAY), "{}", h.describe());
    // Anything on the cut is something to run: a drawing, even with no ink yet,
    // or a sound.
    let mut s = templates::INTENTS[templates::ANIMATION].build();
    assert!(s.add_drawing_after());
    s.toggle_play();
    assert!(s.view.playing);
    let mut s = templates::INTENTS[templates::ANIMATION].build();
    sound(&mut s, 1000);
    s.toggle_play();
    assert!(s.view.playing);
    // A stop is always a stop, whatever went from the cut meanwhile.
    s.undo();
    s.toggle_play();
    assert!(!s.view.playing);
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
fn autail_episode_runs_a_tail_out_on_the_band() {
    // The app's own road: the band's tick, into the scene's next cut.
    let mut s = drawn(12, 3);
    let (scene, first) = (s.view.scene, s.view.cut);
    s.new_cut(scene);
    s.goto_cut(scene, first);
    let mut h = shipped(s);
    h.state_mut().view.loop_playback = false;
    h.state_mut().board.episode_play = true;
    h.state_mut().view.playing = true;
    let mut last = 0;
    for _ in 0..40 {
        h.editor_mut().play_tick(TICK);
        if h.state().view.cut != first {
            break;
        }
        last = h.state().view.frame;
    }
    assert_ne!(h.state().view.cut, first, "it ran on");
    assert_eq!(last, 11, "the edit reads the cut's length, not its drawings");
}

#[test]
fn autail_lip_form_keeps_the_whole_cut() {
    // Lip flap writes over the sound, not over the frames that play.
    let mut s = drawn(48, 3);
    sound(&mut s, 100);
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
fn autail_sound_and_sources_count_beside_a_drawing() {
    // A voice line under a few drawings runs to its end, on keys or not.
    let mut s = drawn(12, 3);
    sound(&mut s, 1000);
    assert_eq!(s.shown_length(), 12);
    let mut h = shipped(s);
    h.state_mut().view.loop_playback = false;
    assert_eq!(run_from(&mut h, 0, 40), (0..12).collect::<Vec<_>>());
    // A short one ends where it ends, and past the last drawing only.
    let mut s = drawn(12, 3);
    sound(&mut s, 500);
    assert_eq!(s.shown_length(), 6);
    sound(&mut s, 100);
    assert_eq!(s.shown_length(), 3, "the last drawing outlasts it");
    // Started late, it ends late. Muted, it ends nothing, and past the cut it
    // stays inside it.
    let mut s = drawn(12, 3);
    sound(&mut s, 250);
    let at = s.at();
    s.engine.project.cut_mut(at.scene, at.cut).unwrap().audio_props.offset = 6;
    assert_eq!(s.shown_length(), 9);
    s.engine.project.cut_mut(at.scene, at.cut).unwrap().audio_props.offset = 60;
    assert_eq!(s.shown_length(), 12);
    s.engine.project.cut_mut(at.scene, at.cut).unwrap().audio_props.mute = true;
    assert_eq!(s.shown_length(), 3);
    // A source the Result reads is on every frame, a drawing or no drawing.
    let mut s = drawn(12, 3);
    assert!(s.auto_comp());
    assert_eq!(s.shown_length(), 3);
    let output = s.cut().graph.nodes().find(|n| n.kind == NodeKind::Output).expect("the composite").id;
    s.graph_add_node(NodeKind::image_sequence("renders/shot_####.png"), (0.0, 90.0));
    let frames = s.selected_node.expect("the new node");
    assert_eq!(s.shown_length(), 3, "reaching nothing, it shows nothing");
    s.graph_connect(frames, output, 0);
    assert_eq!(s.shown_length(), 12);
    assert_eq!(s.play_range(), (0, 11));
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

#[test]
fn autail_loop_start_alone_runs_to_the_cut_end() {
    // A tail is frames, and only a run stops short of them.
    let mut s = drawn(12, 3);
    s.goto(4);
    s.set_loop_in();
    assert_eq!(s.loop_range(), Some((4, 11)));
    assert_eq!(s.status, "loop range 5–12");
    // A range that stands keeps its end.
    s.goto(6);
    s.set_loop_out();
    s.goto(2);
    s.set_loop_in();
    assert_eq!(s.loop_range(), Some((2, 6)));
    // On a blank cut too.
    let mut s = AppState::new_project("t", 64, 36, 12, 72.0);
    s.set_cut_length(48);
    s.goto(10);
    s.set_loop_in();
    assert_eq!(s.loop_range(), Some((10, 47)));
}

#[test]
fn autail_ruler_marks_only_a_range_somebody_set() {
    let mut s = drawn(12, 3);
    assert_eq!(s.play_range(), (0, 2), "the run stops short");
    assert_eq!(strip::marked_range(&s), None, "and that is no range");
    s.goto(4);
    s.set_loop_in();
    s.goto(7);
    s.set_loop_out();
    assert_eq!(strip::marked_range(&s), Some((4, 7)));
    s.goto(0);
    s.set_loop_in();
    s.goto(11);
    s.set_loop_out();
    assert_eq!(strip::marked_range(&s), None, "the whole cut marks nothing");
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
fn autail_one_drawing_with_a_tail_is_a_picture() {
    use crate::export_step::Offer;
    let mut s = templates::INTENTS[templates::ANIMATION].build();
    assert!(s.add_drawing_after());
    paint_texels(&mut s, &[(2, 18)], INK);
    assert_eq!(holds(&s), [1, 11]);
    assert_eq!(Offer::of(&s), Offer::OneFrame);
    assert!(s.add_drawing_after());
    assert_eq!(Offer::of(&s), Offer::Animation);
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
    // Off the queue, the blank tail is a range a person sets in the menu.
    h.editor_mut().export_range = Some((0, 11));
    let ctx = h.ctx();
    h.editor_mut().export_to(&ctx, ExportKind::Png, dir.path());
    let job = &h.state().render_queue.queue.jobs[0];
    assert_eq!((job.frames, job.total()), (Some((0, 11)), 12));
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
    // Their cards say what these keys do, and name the chords.
    let cards = [
        (&long, vocab::LONGER, Action::HoldPlus, doc::LONGER_SAYS),
        (&short, vocab::SHORTER, Action::HoldMinus, doc::SHORTER_SAYS),
    ];
    for (key, word, action, says) in cards {
        h.hover(key);
        h.wait(plate::CARD_DELAY + 0.1);
        let chord = crate::menus::chord_label(&h.ctx(), action).expect("a chord");
        let words = format!("{word} ({chord}) — {says}");
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
    sized(&mut h, 1600.0, 900.0);
    // Timing keeps them with its sheet closed: nothing else there holds a run.
    let dock = h.editor().dock.clone();
    h.editor_mut().dock = egui_dock::DockState::new(vec![crate::workspace::Pane::Canvas]);
    settle(&mut h);
    assert!(!h.editor().sheet_on_screen());
    assert_eq!(h.editor().stage, Some(Stage::Edit));
    assert!(h.has(&long) && h.has(&short), "{}", h.describe());
    h.editor_mut().dock = dock;
    settle(&mut h);
    // No other step has them, the full band of Color's sheet among them.
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
fn autail_hold_keys_leave_the_place_on_the_band() {
    // At 1280 the keys take the lane under `SAY_DOTS_LANE`, with room to spare.
    let mut h = timing(timed(&[4, 4]));
    sized(&mut h, 1280.0, 800.0);
    assert!(h.editor().say.dots_painted, "{}", h.describe());
    let dots = h.editor().say.dots_at.expect("the dots");
    assert!(dots.right() <= h.rect(&longer()).left(), "{dots:?}");
    // A refusal still outranks them.
    h.editor_mut().state.refuse("refused — nothing is exposed on this frame");
    h.run_frames(2);
    assert!(!h.editor().say.dots_painted);
}

#[test]
fn autail_hold_handle_rests_out_of_sight_on_one_frame() {
    let seen = |h: &EditorHarness| {
        let id = crate::xsheet_panel::handles_seen();
        h.ctx().data(|d| d.get_temp::<Vec<u32>>(id)).unwrap_or_default()
    };
    // What Add drawing leaves on every animation shows no ring.
    let h = shipped(drawn(12, 3));
    assert!(h.editor().sheet_on_screen(), "{}", h.describe());
    assert_eq!(strip::sheets(h.state())[3].shows, Exposure::Empty);
    assert_eq!(seen(&h), [0u32; 0]);
    // A hold that ends early still does, and so does one somebody pulled.
    let h = shipped(gap_at(timed(&[6, 6]), 3));
    assert!(h.editor().sheet_on_screen(), "{}", h.describe());
    assert_eq!(seen(&h), [0]);
    let mut h = shipped(drawn(12, 3));
    let column = h.state().view.active_column;
    h.state_mut().positioned_holds.insert((column, 2));
    settle(&mut h);
    assert_eq!(seen(&h), [2]);
}

/// The part's fence (feedback AU26, Must not change): what it could not be
/// seen to leave alone anywhere else. `aulook_nothing_opens_a_window`,
/// `aulook_the_seams_stand_as_they_stood` (shaders, wire) and
/// `aulook_what_was_left_out_stays_out` (Actions, panes) stand behind this one.
#[test]
fn autail_stores_nothing_new_and_adds_by_one_apply() {
    let doc = include_str!("doc.rs");
    let body = doc.split("pub fn add_drawing_after").nth(1).expect("the verb");
    let body: String = body.lines().take_while(|l| *l != "    }").collect();
    assert_eq!(body.matches("engine.apply(").count(), 1, "one undo, by construction");
    let mut used: Vec<&str> = body.split("Command::").skip(1).map(|c| c.split(' ').next().unwrap()).collect();
    used.sort();
    used.dedup();
    assert_eq!(used, ["AddDrawing", "InsertFrames", "SetCell"], "no new command, so no new wire");
    // Each road through it, as the engine counts them.
    let roads = [gap_at(timed(&[2, 2, 5]), 4), gap_at(timed(&[3, 1, 2]), 3), timed(&[2, 2])];
    for mut s in roads {
        let position = s.engine.position();
        assert!(s.add_drawing_after());
        assert_eq!(s.engine.position(), position + 1);
    }
    let mut blank = templates::INTENTS[templates::ANIMATION].build();
    let position = blank.engine.position();
    assert!(blank.add_drawing_after());
    assert_eq!(blank.engine.position(), position + 1);
    // A tail is cells, in the file's own words: no new stored key, and the
    // schema stands.
    let store = include_str!("../../crates/anim-core/src/store.rs");
    assert!(store.contains("const SCHEMA_VERSION: i64 = 11;"));
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tail.animproj");
    let project = drawn(12, 3).engine.project.clone();
    anim_core::store::save(&project, &file).expect("saved");
    let read = anim_core::store::load(&file).expect("read");
    assert!(read == project, "a tail survives the file whole");
    assert_eq!(read.app_meta.keys().collect::<Vec<_>>(), ["kind"], "`app.kind`, as AU18 left it");
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
