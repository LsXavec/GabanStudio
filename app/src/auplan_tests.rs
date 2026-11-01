//! AU19's tests: Plan, an animation's first step and a real one (feedback
//! spec 3): the sheet at the left, the live paper clear of the rail, one
//! timeline.

use anim_core::model::{Stroke, StrokePoint};
use eframe::egui;
use egui::epaint::Primitive;
use egui::{Color32, Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};

use crate::aurail_tests::window;
use crate::config::{Action, Config};
use crate::doc::AppState;
use crate::newproject::NewProjectForm;
use crate::say;
use crate::steps::{self, ProjectKind};
use crate::testkit::EditorHarness;
use crate::workspace::{Pane, Stage};
use crate::{PROBE_SCALES, measure, plate, sample, strip, vocab};

const SIZES: [(f32, f32); 4] = [(1600.0, 900.0), (1280.0, 800.0), (1280.0, 720.0), (1024.0, 640.0)];
/// 16:9, 4:3, 1:1 and 9:16 (spec 3.4).
const PAPERS: [(u32, u32); 4] = [(1280, 720), (1440, 1080), (1080, 1080), (1080, 1920)];

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

fn sized(h: &mut EditorHarness, w: f32, tall: f32) {
    h.kit_mut().set_size(vec2(w, tall));
    settle(h);
}

fn stroke(x: f32) -> Stroke {
    let pt = |x: f32, y: f32| StrokePoint { x, y, pressure: 1.0, tilt: [0.0, 0.0] };
    Stroke { points: vec![pt(x, 60.0), pt(x + 60.0, 240.0)], base_width: 12.0, color: [0, 0, 0, 255] }
}

/// Two drawings each held `hold` frames, each with its own line.
fn two_keys(hold: u32) -> AppState {
    let mut s = AppState::new_project("t", 640, 360, 24, 72.0);
    s.set_cut_length(hold * 2);
    for (at, x) in [(0, 100.0), (hold, 400.0)] {
        s.goto(at);
        s.new_drawing_at_frame();
        s.commit_stroke(stroke(x));
    }
    s.goto(0);
    s
}

/// One drawing held `frames`.
fn one_run(frames: u32) -> AppState {
    let mut s = AppState::new_project("t", 640, 360, 24, 72.0);
    s.set_cut_length(frames);
    s.goto(0);
    s.new_drawing_at_frame();
    s.commit_stroke(stroke(100.0));
    s
}

/// `state`, in Plan at 1600×900.
fn plan(state: AppState) -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(state);
    sized(&mut h, 1600.0, 900.0);
    assert!(h.editor().in_plan() && h.editor().sheet_on_screen(), "{:?}", panes(&h));
    h
}

/// A paper of `size`, stamped an animation: no frames need drawing for it.
fn paper(size: (u32, u32)) -> AppState {
    let mut s = NewProjectForm { width: size.0, height: size.1, ..Default::default() }.new_state();
    ProjectKind::Animation.stamp(&mut s.engine.project);
    s
}

fn panes(h: &EditorHarness) -> Vec<Pane> {
    h.editor().dock.iter_all_tabs().map(|(_, p)| *p).collect()
}

fn pane_rect(h: &EditorHarness, pane: Pane) -> Rect {
    let rects = &h.editor().pane_rects;
    rects.iter().find(|(p, ..)| *p == pane).map(|(_, r, _)| *r).unwrap_or_else(|| panic!("{pane:?}: {rects:?}"))
}

/// The middle of `frame`'s row, in column A.
fn row(h: &EditorHarness, frame: u32) -> Pos2 {
    h.state().sheet_ui.cell_center(0, frame).expect("the sheet drew its rows")
}

fn paper_middle(h: &EditorHarness) -> Pos2 {
    let xf = h.editor().canvas.view_xform();
    xf.paper_to_screen((xf.paper / 2.0).to_pos2())
}

/// A pen line on the paper, from just left of its middle.
fn line(h: &EditorHarness) -> [Pos2; 3] {
    let at = paper_middle(h) - vec2(30.0, 0.0);
    [at, at + vec2(30.0, 0.0), at + vec2(60.0, 10.0)]
}

/// Sample spacing of [`paper_pixels`], in points.
const PIXEL: f32 = 2.0;

/// The paper as the last frame painted it: every triangle egui would hand a
/// device, filled on a 2 pt grid. A locked preview, a stale paper or a drawing
/// that never reached the canvas reads here; `view.frame` says nothing of them.
fn paper_pixels(h: &EditorHarness) -> Vec<Color32> {
    let paper = h.editor().canvas.view_xform().paper_bounds();
    let (cols, rows) = ((paper.width() / PIXEL) as usize, (paper.height() / PIXEL) as usize);
    let mut pixels = vec![Color32::TRANSPARENT; cols * rows];
    let ctx = h.ctx();
    for clipped in ctx.tessellate(h.kit().output().shapes.clone(), ctx.pixels_per_point()) {
        let Primitive::Mesh(mesh) = clipped.primitive else { continue };
        let clip = clipped.clip_rect.intersect(paper);
        for triangle in mesh.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|i| mesh.vertices[triangle[i] as usize]);
            let area = (b.pos - a.pos).x * (c.pos - a.pos).y - (b.pos - a.pos).y * (c.pos - a.pos).x;
            let hull = Rect::from_points(&[a.pos, b.pos, c.pos]).intersect(clip);
            if area == 0.0 || !hull.is_positive() {
                continue;
            }
            for y in ((hull.top() - paper.top()) / PIXEL) as usize..=((hull.bottom() - paper.top()) / PIXEL) as usize {
                for x in ((hull.left() - paper.left()) / PIXEL) as usize..=((hull.right() - paper.left()) / PIXEL) as usize {
                    let p = paper.min + vec2(x as f32, y as f32) * PIXEL;
                    let side = |from: Pos2, to: Pos2| ((to - from).x * (p - from).y - (to - from).y * (p - from).x) / area;
                    let (wa, wb, wc) = (side(b.pos, c.pos), side(c.pos, a.pos), side(a.pos, b.pos));
                    let alpha = wa * a.color.a() as f32 + wb * b.color.a() as f32 + wc * c.color.a() as f32;
                    if wa >= 0.0 && wb >= 0.0 && wc >= 0.0 && alpha >= 128.0 && x < cols && y < rows {
                        pixels[y * cols + x] = a.color;
                    }
                }
            }
        }
    }
    pixels
}

// ---- the recipe and the door ----

/// Acceptance 1: exactly the two panes, the sheet on the left share.
#[test]
fn auplan_recipe_is_sheet_and_canvas() {
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    let mut tabs = panes(&h);
    tabs.sort_by_key(|p| format!("{p:?}"));
    assert_eq!(tabs, [Pane::Canvas, Pane::XSheet]);
    let (sheet, canvas) = (pane_rect(&h, Pane::XSheet), pane_rect(&h, Pane::Canvas));
    assert!(sheet.right() < canvas.left(), "{sheet:?} left of {canvas:?}");
    assert!(sheet.left() <= 6.0, "from the window's edge, the pane's margin in: {sheet:?}");
    let share = (sheet.right() + canvas.left()) / 2.0 / 1600.0;
    assert!((share - 0.22).abs() < 0.002, "{share}");
    // Every road there reads the one recipe.
    assert!(crate::same_shape(&h.editor().dock, &Stage::Layout.dock()));
    h.perform(Action::RoomFinishing);
    h.perform(Action::RoomDrawing);
    settle(&mut h);
    assert!(crate::same_shape(&h.editor().dock, &Stage::Layout.dock()));
    h.editor_mut().open_pane(Pane::History);
    h.perform(Action::ResetLayout);
    settle(&mut h);
    assert!(crate::same_shape(&h.editor().dock, &Stage::Layout.dock()));
    // A drawing has no such step, whatever a config remembers.
    let mut h = EditorHarness::as_shipped();
    h.editor_mut().sheet_in_draw = true;
    h.perform(Action::RoomFinishing);
    h.perform(Action::RoomDrawing);
    settle(&mut h);
    assert_eq!(panes(&h), [Pane::Canvas]);
}

/// Acceptance 2: nobody stands in `Stage::Layout`, by any road or on any boot.
#[test]
fn auplan_layout_redirects_at_the_door() {
    let shipped = |kind: ProjectKind| {
        let mut s = paper((1280, 720));
        kind.stamp(&mut s.engine.project);
        EditorHarness::with_state_shipped(s)
    };
    for (mut h, want) in [
        (shipped(ProjectKind::Drawing), Stage::Drawing),
        (shipped(ProjectKind::Animation), Stage::Drawing),
        (EditorHarness::with_state_shipped(crate::templates::ALL[crate::templates::COMIC].build()), Stage::Manga),
        (EditorHarness::new(), Stage::Drawing),
    ] {
        for go in 0..3 {
            match go {
                0 => h.editor_mut().enter_room(Stage::Layout),
                1 => {
                    h.perform(Action::RoomLayout);
                }
                _ => h.key(Key::F4),
            }
            settle(&mut h);
            assert_eq!(h.editor().stage, Some(want), "road {go}");
        }
    }
    // And no line of the app sets it but the door's.
    let main = include_str!("main.rs");
    assert_eq!(main.matches("self.stage = Some(st)").count(), 1);
    assert!(!main.contains("self.stage = Some(Stage::Layout)"));
}

/// Acceptance 3: the preview has gone, with its refusal and its pane.
#[test]
fn auplan_no_preview() {
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    h.editor_mut().enter_room(Stage::Layout);
    settle(&mut h);
    assert!(!panes(&h).contains(&Pane::Viewer(u8::MAX)), "{:?}", panes(&h));
    assert!(!h.editor().viewers.contains_key(&u8::MAX));
    assert_eq!(h.state().strokes_done, 0);
    let path = line(&h);
    h.drag_path(&path);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, 1, "{}", h.state().status);
    assert!(h.state().refusal.is_empty(), "{}", h.state().refusal);
    assert!(!h.has_text_containing("preview"), "{}", h.describe());
    assert!(h.state().has_art(), "the line is on the paper");
}

// ---- Fit ----

/// Acceptance 4: the paper never goes under the rail or the sheet, whatever
/// its shape and whatever the window.
#[test]
fn auplan_paper_clears_the_rail() {
    let mut found = Vec::new();
    for size in PAPERS {
        let mut h = EditorHarness::with_state_shipped(paper(size));
        assert!(h.editor().in_plan());
        for (w, tall) in SIZES {
            for stored in PROBE_SCALES {
                window(&mut h, w, tall, stored);
                h.perform(Action::ZoomFit);
                settle(&mut h);
                let share = measure::paper_share(&h);
                let bounds = h.editor().canvas.view_xform().paper_bounds();
                let pane = h.editor().canvas.dbg_rect;
                let rail_right = pane.left() + crate::floats::LEFT_X;
                if share.paper_under_chrome != 0 || bounds.left() < rail_right - 0.5 || bounds.right() > pane.right() + 0.5 {
                    found.push(format!("{size:?} at {w}×{tall} ×{stored}: {bounds:?} in {pane:?}: {share:?}"));
                }
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
    // Spec 3.4's numbers, at the two sizes it draws.
    let mut h = plan(paper(PAPERS[0]));
    let (pane, bounds) = (h.editor().canvas.dbg_rect, h.editor().canvas.view_xform().paper_bounds());
    println!("Plan at 1600×900: pane {pane:?}, paper {bounds:?}");
    assert_eq!(pane, Rect::from_min_max(pos2(355.0, 78.0), pos2(1600.0, 864.0)));
    assert!((bounds.left() - 431.0).abs() <= 1.0 && (bounds.width() - 1132.0).abs() <= 1.0, "{bounds:?}");
    sized(&mut h, 1280.0, 800.0);
    h.perform(Action::ZoomFit);
    settle(&mut h);
    let (pane, bounds) = (h.editor().canvas.dbg_rect, h.editor().canvas.view_xform().paper_bounds());
    println!("Plan at 1280×800: pane {pane:?}, paper {bounds:?}");
    assert_eq!(pane, Rect::from_min_max(pos2(285.0, 78.0), pos2(1280.0, 764.0)));
    assert!((bounds.left() - 361.0).abs() <= 1.0 && (bounds.width() - 890.0).abs() <= 1.0, "{bounds:?}");
}

/// Acceptance 5, and the sheet hidden is an open canvas again: that Fit is
/// today's, the rail's room gone with the sheet.
#[test]
fn auplan_paper_share() {
    let mut shares = Vec::new();
    // Spec 1.10: the first measured value less 0.5: 49.997 and 43.555.
    for ((w, tall), floor) in [((1600.0, 900.0), 49.5), ((1280.0, 800.0), 43.05)] {
        let mut h = plan(paper(PAPERS[0]));
        sized(&mut h, w, tall);
        h.perform(Action::ZoomFit);
        settle(&mut h);
        let share = measure::paper_share(&h);
        shares.push((w, share.paper));
        assert!(share.paper >= floor, "{w}×{tall}: {share:?}");
        assert_eq!(share.paper_under_chrome, 0, "{w}×{tall}: {share:?}");
        let with_sheet = h.editor().canvas.view_xform().paper_bounds();
        h.click(vocab::HIDE_TIMING_SHEET);
        settle(&mut h);
        assert!(!h.editor().canvas.rail_reserve, "no sheet, no reserve");
        assert!(h.editor().canvas.view_xform().paper_bounds().width() > with_sheet.width());
    }
    println!("Plan paper shares: {shares:?}");
}

/// Acceptance 6: the whole sheet stands in the left column.
#[test]
fn auplan_sheet_rows() {
    let mut rows = Vec::new();
    for ((w, tall), floor) in [((1600.0, 900.0), 32), ((1280.0, 800.0), 27)] {
        let mut h = plan(sample::build_sample());
        sized(&mut h, w, tall);
        let row_h = row(&h, 1).y - row(&h, 0).y;
        let view = plate::viewports(&h.ctx()).into_iter().rev().find(|v| v.rect.contains(row(&h, 0)));
        let whole = (view.expect("the sheet's scroll view").rect.height() / row_h) as usize;
        rows.push((w, whole));
        assert!(whole >= floor, "{w}×{tall}: {whole} rows");
    }
    println!("Plan sheet rows: {rows:?}");
}

/// Spec 3.5: the turn's pivot is the paper's own centre, which Fit moved off
/// the pane's, and a turned Fit keeps the corners off the rail.
#[test]
fn auplan_hand_turn_pivots_on_the_paper() {
    let mut h = plan(paper(PAPERS[0]));
    let c = paper_middle(&h);
    let pane = h.editor().canvas.dbg_rect;
    assert!(c.x - pane.center().x > 10.0, "Fit moved the paper past the rail: {c:?} in {pane:?}");
    let arc: Vec<Pos2> = [0.0f32, 5.0, 15.0, 25.0, 35.0, 45.0]
        .iter()
        .map(|d| c + vec2(d.to_radians().cos(), d.to_radians().sin()) * 150.0)
        .collect();
    h.hold_key(Key::Space);
    h.set_mods(Modifiers::SHIFT);
    h.drag_path(&arc);
    h.set_mods(Modifiers::NONE);
    h.release_key(Key::Space);
    settle(&mut h);
    let turned = h.editor().canvas.view_rotation();
    assert!((turned - 40.0).abs() < 0.5, "the pen went 40° round the paper's centre: {turned}");
    assert!((paper_middle(&h) - c).length() < 1.0, "it stayed put: {:?} from {c:?}", paper_middle(&h));
    h.key_mods(Modifiers::COMMAND, Key::Num0);
    settle(&mut h);
    let (bounds, pane) = (h.editor().canvas.view_xform().paper_bounds(), h.editor().canvas.dbg_rect);
    assert_eq!(h.editor().canvas.view_rotation(), turned, "Fit keeps the turn");
    assert!(bounds.left() >= pane.left() + crate::floats::LEFT_X - 0.5, "{bounds:?} past the rail in {pane:?}");
    assert!(bounds.right() <= pane.right() + 0.5 && bounds.top() >= pane.top() - 0.5, "{bounds:?} in {pane:?}");
    assert!(bounds.bottom() <= pane.bottom() + 0.5, "{bounds:?} in {pane:?}");
}

// ---- rows and paper ----

/// Acceptance 7: the whole workflow, and one undo takes it all back.
#[test]
fn auplan_click_a_row_and_draw() {
    let mut h = plan(one_run(6));
    assert_eq!(strip::sheets(h.state()).iter().map(|s| s.len).collect::<Vec<_>>(), [6]);
    let before = h.state_mut().snapshot_project();
    let at = row(&h, 3);
    h.click_pos(at);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 3);
    let path = line(&h);
    h.drag_path(&path);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, 2, "{}", h.state().status);
    // AU26: one drawing across the cut was a hold nobody chose, so the new one
    // keeps to its row and the row after it stays blank.
    assert_eq!(strip::sheets(h.state()).iter().map(|s| (s.from, s.len)).collect::<Vec<_>>(), [(0, 3), (3, 1), (4, 2)]);
    assert_eq!(h.state().cut().drawings.len(), 2);
    assert_eq!(h.perform(Action::Undo), crate::Performed::Done);
    settle(&mut h);
    let mut after = h.state_mut().snapshot_project();
    // The engine mints ids forward and never reuses one, so a redo stays
    // valid: the mint is the one field undo leaves, and every other byte is back.
    assert!(after.next_id > before.next_id, "the mint ran on");
    after.next_id = before.next_id;
    assert!(after == before, "the document is back");
    let dir = h.data_dir().to_path_buf();
    let (was, now) = (dir.join("was.animproj"), dir.join("now.animproj"));
    anim_core::store::save(&before, &was).expect("saved");
    anim_core::store::save(&after, &now).expect("saved");
    assert_eq!(std::fs::read(&was).unwrap(), std::fs::read(&now).unwrap(), "byte for byte");
}

/// Acceptance 8's first half, on what reaches the screen: the old preview also
/// moved `view.frame`, so the paper's pixels are the only witness.
#[test]
fn auplan_row_moves_the_canvas() {
    let mut h = plan(two_keys(4));
    let first = paper_pixels(&h);
    let inked = |pixels: &[Color32]| pixels.iter().filter(|c| c.r() < 128 && c.a() > 0).count();
    assert!(inked(&first) > 100, "the first drawing's line reads: {}", inked(&first));
    let at = row(&h, 5);
    h.click_pos(at);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 5);
    let second = paper_pixels(&h);
    let moved = first.iter().zip(&second).filter(|(a, b)| a != b).count();
    assert!(moved > 100, "the paper changed with the row: {moved} of {}", first.len());
    // Where the second drawing's line stands, it is now the drawing's own ink.
    let xf = h.editor().canvas.view_xform();
    let paper = xf.paper_bounds();
    let width = (paper.width() / PIXEL) as usize;
    let on = xf.paper_to_screen(pos2(430.0, 150.0)) - paper.min;
    let at = (on.y / PIXEL) as usize * width + (on.x / PIXEL) as usize;
    let full = |c: &Color32| c.r() < 16 && c.g() < 16 && c.b() < 16 && c.a() == 255;
    let near = |pixels: &[Color32]| (0..6).filter_map(|d| pixels.get(at + d)).any(full);
    assert!(near(&second) && !near(&first), "the second line is at full strength on its own row only");
    let at = row(&h, 0);
    h.click_pos(at);
    settle(&mut h);
    assert_eq!(paper_pixels(&h), first, "and back");
}

/// Acceptance 8's second half: in the frame the pen lifts, the new key stands
/// on the cursor's row, and nothing else on either side moved.
#[test]
fn auplan_stroke_shows_on_the_cursor_row() {
    let mut h = plan(one_run(8));
    let at = row(&h, 5);
    h.click_pos(at);
    settle(&mut h);
    let (frame, column) = (h.state().view.frame, h.state().view.active_column);
    let (top, xf) = (row(&h, 0), h.editor().canvas.view_xform());
    let [from, .., to] = line(&h);
    h.press_and_move(from, to, 4);
    assert!(h.painted_text().iter().filter(|t| *t == "2").count() <= 1, "no key yet");
    h.kit_mut().input_mut().events.push(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.kit_mut().step();
    assert_eq!(strip::sheets(h.state()).iter().map(|s| s.from).collect::<Vec<_>>(), [0, 5, 6], "the run split");
    let name = h.state().cut().drawings[1].name.clone();
    let shown = h.painted_at(&name);
    assert!((shown.y - row(&h, 5).y).abs() <= 12.0, "{name} at {shown:?}, row 6 at {:?}", row(&h, 5));
    assert_eq!((h.state().view.frame, h.state().view.active_column), (frame, column), "the cursor stayed");
    assert_eq!(row(&h, 0), top, "the sheet did not scroll");
    assert_eq!(h.editor().canvas.view_xform(), xf, "nor the paper");
    assert!(!h.editor().canvas.stroke_active());
}

// ---- one timeline ----

/// Acceptance 9.
#[test]
fn auplan_one_timeline() {
    let mirror = |h: &EditorHarness| {
        let host = h.editor().canvas.dbg_rect;
        let wells = h.filled_rects(plate::WELL).into_iter();
        wells.filter(|r| r.left() == host.left() && r.height() > host.height() / 2.0).next()
    };
    for mut h in [plan(sample::build_sample()), plan(two_keys(4))] {
        assert!(h.editor().strip.rect().is_none(), "no strip with the sheet up");
        assert!(mirror(&h).is_none(), "no mirror");
        assert!(!h.editor().canvas.sheet_shown);
        assert!(!panes(&h).iter().any(|p| matches!(p, Pane::Viewer(_))), "{:?}", panes(&h));
        assert_eq!(900.0 - pane_rect(&h, Pane::Canvas).bottom(), 36.0, "the band");
        // Timing keeps the mirror: the sheet stands alone there no more than
        // it did.
        h.click(vocab::HIDE_TIMING_SHEET);
        settle(&mut h);
        assert!(h.editor().strip.rect().is_some(), "the sheet hidden, the strip comes back");
        h.click(vocab::SHOW_TIMING_SHEET);
        settle(&mut h);
        assert!(h.editor().strip.rect().is_none());
    }
}

/// Acceptance 10: the band's keys, in their order, and only those.
#[test]
fn auplan_band_keeps_the_drawing_keys() {
    let mut h = plan(two_keys(4));
    h.wait(4.2);
    settle(&mut h);
    let lane = ["previous frame (S)", "play (Space)", "next frame (F)", "Frame 1 of 8"];
    let right = ["Add drawing (band)", "Ghost frames toggle", "Speed", vocab::HIDE_TIMING_SHEET];
    let mut last = f32::MIN;
    for key in lane.iter().chain(&right) {
        assert!(h.has(key), "{key}\n{}", h.describe());
        let at = h.rect(key);
        assert!(at.left() >= last, "{key} stands in order: {at:?} after {last}");
        assert!((at.center().y - 882.0).abs() < 10.0, "{key} on the band: {at:?}");
        last = at.right();
    }
    // Say stands between them, in the middle of the band.
    let say = h.painted_at(say::PLAN_ROW_WORDS).x;
    let (counter, add) = (h.rect("Frame 1 of 8").right(), h.rect("Add drawing (band)").left());
    assert!(counter < say && say < add, "the counter ends at {counter}, Say starts at {say}, Add drawing at {add}");
    for gone in ["first frame (Home)", "last frame (End)", "clear loop range", vocab::SOUND_WHILE_DRAGGING, vocab::LOOP] {
        assert!(!h.has(gone), "{gone}");
    }
    // From the first frame of a new animation too: the sheet's up, so nothing
    // waits for a second drawing.
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    assert!(h.state().cut().drawings.is_empty());
    assert!(h.has("next frame (F)") && h.has("Speed") && h.has("Ghost frames toggle"), "{}", h.describe());
}

/// Acceptance 11: the file holds it, over a real restart, both ways.
#[test]
fn auplan_hidden_sheet_survives_a_restart() {
    let mut first = EditorHarness::as_animation();
    settle(&mut first);
    assert!(!first.rig().config.ui.plan_sheet_hidden());
    first.click(vocab::HIDE_TIMING_SHEET);
    settle(&mut first);
    assert!(Config::load_from(first.data_dir()).0.ui.plan_sheet_hidden(), "saved at the click");
    let mut second = EditorHarness::with_dir_animation(first.quit());
    settle(&mut second);
    assert_eq!(panes(&second), [Pane::Canvas], "hidden over the restart");
    assert!(second.has(vocab::SHOW_TIMING_SHEET), "{}", second.describe());
    second.click(vocab::SHOW_TIMING_SHEET);
    settle(&mut second);
    assert!(!Config::load_from(second.data_dir()).0.ui.plan_sheet_hidden(), "saved at the click");
    let mut third = EditorHarness::with_dir_animation(second.quit());
    settle(&mut third);
    assert!(panes(&third).contains(&Pane::XSheet), "reopened over the restart");
    assert!(third.has(vocab::HIDE_TIMING_SHEET));
}

/// Acceptance 11's last line: the lock's list reads the switch at once, not on
/// the next frame, or a close in between would pass.
#[test]
fn auplan_panes_follow_the_recipe() {
    let mut h = EditorHarness::as_animation();
    settle(&mut h);
    assert!(h.editor_mut().locked_panes().contains(&Pane::XSheet));
    h.editor_mut().switch_sheet(true);
    assert_eq!(h.editor_mut().locked_panes(), [Pane::Canvas], "the same frame");
    h.editor_mut().switch_sheet(false);
    assert!(h.editor_mut().locked_panes().contains(&Pane::XSheet), "the same frame");
    // A reopening by a verb is the same recipe, and stores nothing.
    h.editor_mut().switch_sheet(true);
    settle(&mut h);
    h.editor_mut().show_sheet();
    assert!(h.editor().sheet_on_screen());
    assert!(crate::same_shape(&h.editor().dock, &Stage::Layout.dock()), "the sheet at the left, not a split on the right");
    settle(&mut h);
    assert!(h.rig().config.ui.plan_sheet_hidden(), "the verb remembered nothing");
    // It holds under the lock as the button's does: one sheet, one close key.
    assert!(h.editor_mut().locked_panes().contains(&Pane::XSheet), "a verb's sheet is Plan's");
    h.rig_mut().config.ui.lock_positions = true;
    settle(&mut h);
    crate::audock_tests::middle_click_tab(&mut h, Pane::XSheet);
    assert!(h.editor().sheet_on_screen(), "the tab has no live close under the lock");
}

/// A pane opened in Plan keeps both halves in view: Window's Node Graph, which
/// spec 3.3 keeps, or a chord's. A tab on the sheet's leaf would cover the one
/// timeline, bring the strip back and refit the paper.
#[test]
fn auplan_a_new_pane_keeps_the_sheet() {
    let mut h = plan(two_keys(4));
    let (paper, rows) = (h.editor().canvas.view_xform().paper_bounds(), row(&h, 0));
    h.editor_mut().open_pane(Pane::History);
    settle(&mut h);
    let shown = crate::visible_panes(&h.editor().dock);
    assert!(shown.contains(&Pane::XSheet) && shown.contains(&Pane::Canvas) && shown.contains(&Pane::History), "{shown:?}");
    assert!(pane_rect(&h, Pane::History).top() > pane_rect(&h, Pane::XSheet).bottom(), "under the sheet");
    assert_eq!(h.editor().canvas.view_xform().paper_bounds(), paper, "the paper did not move");
    assert_eq!(row(&h, 0), rows, "nor the sheet's first row");
    assert!(h.editor().canvas.rail_reserve && h.editor().strip.rect().is_none());
    assert!(h.has(vocab::HIDE_TIMING_SHEET));
    // The next one joins it, on no recipe's leaf, wherever the focus stands.
    let at = paper_middle(&h);
    h.pen_tap(at);
    settle(&mut h);
    for pane in [Pane::NodeGraph, Pane::Layers] {
        h.editor_mut().open_pane(pane);
        settle(&mut h);
        let shown = crate::visible_panes(&h.editor().dock);
        assert!(shown.contains(&Pane::XSheet) && shown.contains(&Pane::Canvas) && shown.contains(&pane), "{pane:?}: {shown:?}");
    }
    assert_eq!(h.editor().dock.main_surface().iter().filter(|n| n.is_leaf()).count(), 3, "one leaf for them all");
    assert_eq!(h.editor().canvas.view_xform().paper_bounds(), paper);
    // Reset is the recipe again.
    h.perform(Action::ResetLayout);
    settle(&mut h);
    assert!(crate::same_shape(&h.editor().dock, &Stage::Layout.dock()));
}

/// Spec 1.11 and 3.5: Hide tools gives the paper the rail's room, with the
/// sheet on. Without this the drop is one word in `rail_shows`.
#[test]
fn auplan_hide_tools_drops_the_reserve() {
    let mut h = plan(paper(PAPERS[0]));
    h.perform(Action::ZoomFit);
    settle(&mut h);
    assert!(h.editor().canvas.rail_reserve, "the sheet is on and the rail shows");
    let with_rail = h.editor().canvas.view_xform().paper_bounds();
    let pane = h.editor().canvas.dbg_rect;
    h.key(Key::T);
    settle(&mut h);
    assert!(h.editor().tools_hidden && h.editor().sheet_on_screen(), "the sheet stays");
    assert!(!h.editor().canvas.rail_reserve, "no rail, nothing to clear");
    let bare = h.editor().canvas.view_xform().paper_bounds();
    let pane_now = h.editor().canvas.dbg_rect;
    assert!(bare.width() > with_rail.width(), "the paper grew: {bare:?} from {with_rail:?}");
    assert!((bare.center().x - pane_now.center().x).abs() < 1.0, "it fits the whole pane: {bare:?} in {pane_now:?}");
    h.key(Key::T);
    settle(&mut h);
    assert!(h.editor().canvas.rail_reserve, "the reserve is back with the rail");
    assert_eq!(h.editor().canvas.view_xform().paper_bounds(), with_rail, "the paper it left");
    assert_eq!(h.editor().canvas.dbg_rect, pane);
}

/// The floated canvas hosts its rail too, so spec 3.5's formula has no third
/// case: the paper clears the rail there as well.
#[test]
fn auplan_float_keeps_the_reserve() {
    let mut h = plan(paper(PAPERS[0]));
    h.editor_mut().float_canvas.open = true;
    settle(&mut h);
    assert!(h.editor().sheet_on_screen());
    assert!(h.editor().canvas.rail_reserve);
}

/// The walks' own premise: `window` reaches the room it names, whatever room
/// it comes from. The frame a zoom lands on rewrites the size it was given.
#[test]
fn auplan_window_reaches_every_room() {
    let mut h = EditorHarness::with_state_shipped(paper(PAPERS[0]));
    let mut found = Vec::new();
    for (w, tall) in SIZES {
        for stored in PROBE_SCALES {
            window(&mut h, w, tall, stored);
            let scale = crate::effective_scale(stored, w, tall);
            let (room, ppp) = (h.ctx().content_rect().size(), h.ctx().pixels_per_point());
            found.push(format!("{w}×{tall} at {stored}: room {room:?} at {ppp}"));
            assert_eq!(ppp, scale, "{}", found.join("\n"));
            assert!((room - vec2(w, tall) / scale).length() < 0.5, "{}", found.join("\n"));
        }
    }
    println!("{}", found.join("\n"));
}

/// The size a person with a small screen sees at 150%: no test reads more
/// than the clip; this one reads the paper (spec 3.4).
#[test]
fn auplan_tightest_paper_reads() {
    let mut h = EditorHarness::with_state_shipped(paper(PAPERS[0]));
    window(&mut h, 1024.0, 640.0, 1.5);
    h.perform(Action::ZoomFit);
    settle(&mut h);
    assert!(h.editor().sheet_on_screen());
    let (pane, bounds) = (h.editor().canvas.dbg_rect, h.editor().canvas.view_xform().paper_bounds());
    let (room, ppp) = (h.ctx().content_rect(), h.ctx().pixels_per_point());
    println!("Plan at 832×520: room {room:?} at {ppp}, pane {pane:?}, paper {bounds:?}");
    assert!((room.size() - vec2(832.0, 520.0)).length() < 0.5, "{room:?}");
    assert_eq!(measure::paper_share(&h).paper_under_chrome, 0);
    assert!(bounds.width() >= 400.0 && bounds.height() >= 225.0, "{bounds:?} in {pane:?}");
}

// ---- the Say row ----

/// Spec 8.2's new row: Plan with the sheet on, once. No Show me: its circle
/// never shows a row being clicked.
#[test]
fn auplan_row_hint_once() {
    let says = |h: &EditorHarness| h.editor().say.hint.as_ref().map(|s| s.words.clone());
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    h.wait(4.2);
    settle(&mut h);
    let spoken = h.editor().say.hint.clone().expect("a row");
    assert_eq!(spoken.words, say::PLAN_ROW_WORDS);
    assert_eq!(spoken.bit, Some(say::PLAN_ROWS));
    assert_eq!(spoken.bit, Some(1 << 23), "a stored bit keeps its number");
    assert_eq!(spoken.verb, None);
    assert!(!h.has("Show me (say line)"), "{}", h.describe());
    // The sheet hidden, there are no rows to click.
    h.click(vocab::HIDE_TIMING_SHEET);
    settle(&mut h);
    assert_ne!(says(&h).as_deref(), Some(say::PLAN_ROW_WORDS));
    h.click(vocab::SHOW_TIMING_SHEET);
    settle(&mut h);
    assert_eq!(says(&h).as_deref(), Some(say::PLAN_ROW_WORDS), "not answered yet");
    let path = line(&h);
    h.drag_path(&path);
    settle(&mut h);
    assert_ne!(h.rig().config.ui.hints_seen & say::PLAN_ROWS, 0, "a stroke answers it");
    assert_ne!(says(&h).as_deref(), Some(say::PLAN_ROW_WORDS));
    h.perform(Action::RoomFinishing);
    h.perform(Action::RoomDrawing);
    settle(&mut h);
    assert_ne!(says(&h).as_deref(), Some(say::PLAN_ROW_WORDS), "once");
    // Over art it speaks the same.
    let mut h = plan(two_keys(4));
    h.wait(4.2);
    settle(&mut h);
    let spoken = h.editor().say.hint.clone().expect("a row");
    assert_eq!((spoken.words.as_str(), spoken.verb), (say::PLAN_ROW_WORDS, None));
    // A drawing has no rows.
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    h.wait(4.2);
    settle(&mut h);
    assert_ne!(says(&h).as_deref(), Some(say::PLAN_ROW_WORDS));
    // The bits the spec keeps out of use stay out of it.
    for hint in &say::HINTS {
        assert_ne!(hint.bit, 1 << 5);
        assert_ne!(hint.bit, 1 << 15);
    }
}

/// The AU10 trap: a row answered by a common act counts only where it stood. A
/// person who draws first, the most common route, still reads it in Plan.
#[test]
fn auplan_row_hint_outlives_a_stroke_on_the_ready_paper() {
    let says = |h: &EditorHarness| h.editor().say.hint.as_ref().map(|s| s.words.clone());
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    assert_eq!(h.editor().kind(), Some(ProjectKind::Drawing));
    let path = line(&h);
    h.drag_path(&path);
    settle(&mut h);
    assert_eq!(h.rig().config.ui.hints_seen & say::PLAN_ROWS, 0, "no row stood to answer");
    // It grows into an animation; the stage stays, and the sheet is a press away.
    h.perform(Action::AddDrawingAfter);
    settle(&mut h);
    assert_eq!(h.editor().kind(), Some(ProjectKind::Animation));
    assert_eq!(h.rig().config.ui.hints_seen & say::PLAN_ROWS, 0, "no sheet, no row yet");
    h.perform(Action::RoomFinishing);
    h.perform(Action::RoomDrawing);
    h.wait(4.2);
    settle(&mut h);
    assert!(h.editor().in_plan() && h.editor().sheet_on_screen());
    assert_eq!(says(&h).as_deref(), Some(say::PLAN_ROW_WORDS), "it speaks after all");
}

/// Over art the row must not hold the lane against a working animator: a click
/// on a row or Play answers it, and the next row takes the lane.
#[test]
fn auplan_row_hint_answers_a_click_or_play() {
    let three = || {
        let mut s = two_keys(4);
        s.set_cut_length(12);
        s.goto(8);
        s.new_drawing_at_frame();
        s.commit_stroke(stroke(250.0));
        s.goto(0);
        s
    };
    for act in 0..2 {
        let mut h = plan(three());
        h.wait(4.2);
        settle(&mut h);
        assert_eq!(h.editor().say.hint.as_ref().map(|s| s.words.as_str()), Some(say::PLAN_ROW_WORDS));
        if act == 0 {
            let at = row(&h, 5);
            h.click_pos(at);
        } else {
            h.perform(Action::PlayPause);
            h.run_frames(2);
            h.perform(Action::PlayPause);
        }
        settle(&mut h);
        assert_ne!(h.rig().config.ui.hints_seen & say::PLAN_ROWS, 0, "act {act} answers it");
        let spoken = h.editor().say.hint.as_ref().map(|s| s.words.clone()).unwrap_or_default();
        assert_ne!(spoken, say::PLAN_ROW_WORDS, "act {act}");
    }
}

/// The first instruction's own row: a new animation has one, and the cursor
/// rests on it. A click there chooses it and takes no typing; the second types.
#[test]
fn auplan_click_on_the_resting_row_selects() {
    let mut h = plan(one_run(3));
    let at = row(&h, 0);
    h.click_pos(at);
    settle(&mut h);
    assert!(!h.typing(), "a click on the resting cell is no typing\n{}", h.describe());
    assert_ne!(h.rig().config.ui.hints_seen & say::PLAN_ROWS, 0, "and it answers the row");
    h.key(Key::Space);
    settle(&mut h);
    assert!(h.state().view.playing, "Space still plays");
    h.key(Key::Space);
    settle(&mut h);
    let strokes = h.state().strokes_done;
    let path = line(&h);
    h.drag_path(&path);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, strokes + 1, "and the next stroke lands");
    // Spec 3.7's second click still types.
    let at = row(&h, h.state().view.frame);
    h.click_pos(at);
    settle(&mut h);
    h.click_pos(at);
    settle(&mut h);
    assert!(h.typing(), "{}", h.describe());
}

/// A template's sheet is its project's: the next document in the same Editor
/// reads the memory again, without a restart.
#[test]
fn auplan_template_sheet_stays_with_its_project() {
    let mut hidden = crate::config::UiConfig::default();
    hidden.set_plan_sheet_hidden(true);
    let t = crate::templates::ALL.iter().position(|t| t.sheet_on).expect("a template with its sheet on");
    let mut h = EditorHarness::as_animation();
    h.click(vocab::HIDE_TIMING_SHEET);
    settle(&mut h);
    *h.editor_mut() = crate::open_template(t, None, Some(h.data_dir()), &hidden);
    settle(&mut h);
    assert!(h.editor().sheet_on_screen(), "the template's project");
    let next = paper(PAPERS[0]);
    h.editor_mut().state = next;
    settle(&mut h);
    assert!(h.editor().in_plan());
    assert!(!h.editor().sheet_on_screen(), "the next one reads the memory: {:?}", panes(&h));
    assert!(h.editor().plan_sheet_hidden);
}

/// F4 is the bar's first step, F5 Draw's old key, and on an animation they are
/// the one pill.
#[test]
fn auplan_f4_and_f5_land_on_one_pill() {
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    let bar = steps::bar(ProjectKind::Animation);
    for key in [Key::F4, Key::F5] {
        h.perform(Action::RoomFinishing);
        settle(&mut h);
        assert_eq!(h.node_toggled("Plan"), Some(false));
        h.key(key);
        settle(&mut h);
        assert_eq!(h.editor().stage, Some(Stage::Drawing), "{key:?}");
        assert!(crate::same_shape(&h.editor().dock, &Stage::Layout.dock()), "{key:?}");
        let lit: Vec<&str> = bar.iter().map(|s| s.label).filter(|l| h.node_toggled(l) == Some(true)).collect();
        assert_eq!(lit, ["Plan"], "{key:?}");
    }
}
