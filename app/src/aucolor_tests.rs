//! AU13's tests: the Color step stands canvas first, a first fill is three
//! taps, and the strip shows pictures.

use anim_core::ids::DrawingId;
use anim_core::model::LayerRole;
use anim_core::raster::{TILE, TILE_LEN, TileData, f32_to_f16_bits};
use anim_core::xsheet::Exposure;
use eframe::egui;
use egui::{Key, Pos2, Rect, vec2};
use std::sync::Arc;

use crate::canvas::extpaper_tests::paint_texels;
use crate::config::Config;
use crate::doc::{self, AppState};
use crate::measure::{self, Act, Recorder};
use crate::palette::{CharacterPalette, ColorRole};
use crate::palette_panel::{ADDING, DEFAULT_CHARACTER, NO_COLORS};
use crate::strip::{self, Strip};
use crate::testkit::{EditorHarness, PanelHarness};
use crate::workspace::{Pane, Stage};
use crate::{plate, rail, same_shape, templates, vocab};

const SIZES: [(f32, f32); 2] = [(1600.0, 900.0), (1280.0, 800.0)];
const INK: [f32; 4] = [0.1, 0.1, 0.1, 1.0];

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

fn sized(h: &mut EditorHarness, w: f32, tall: f32) {
    h.kit_mut().set_size(vec2(w, tall));
    settle(h);
}

fn enter(h: &mut EditorHarness, stage: Stage) {
    h.editor_mut().enter_room(stage);
    settle(h);
}

fn color_room(w: f32, tall: f32) -> EditorHarness {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, w, tall);
    enter(&mut h, Stage::InkPaint);
    h
}

fn panes(h: &EditorHarness) -> Vec<Pane> {
    h.editor().dock.iter_all_tabs().map(|(_, p)| *p).collect()
}

fn pane_rect(h: &EditorHarness, pane: Pane) -> Rect {
    let rects = &h.editor().pane_rects;
    rects.iter().find(|(p, ..)| *p == pane).map(|(_, r, _)| *r).unwrap_or_else(|| panic!("{pane:?}: {rects:?}"))
}

fn says(h: &EditorHarness, words: &str) -> bool {
    h.painted_text().iter().any(|w| w == words)
}

#[track_caller]
fn in_pane(h: &EditorHarness, pane: Pane, name: &str) {
    let (room, control) = (pane_rect(h, pane), h.rect(name));
    assert!(room.contains_rect(control), "{name}: {control:?} in {room:?}");
}

fn well(name: &str) -> String {
    format!("colour well: {name}")
}

/// A blot of ink at `x`, on the armed layer.
fn blot(state: &mut AppState, x: i32) {
    let texels: Vec<(i32, i32)> = (0..40).flat_map(|dx| (0..40).map(move |dy| (x + dx, 60 + dy))).collect();
    paint_texels(state, &texels, INK);
}

/// Two inked drawings, each held `hold` frames.
fn two_inked(hold: u32) -> AppState {
    let mut s = AppState::new_project("t", 640, 360, 24, 72.0);
    s.set_cut_length(hold * 2);
    for (at, x) in [(0, 60), (hold, 300)] {
        s.goto(at);
        s.new_drawing_at_frame();
        blot(&mut s, x);
    }
    s.goto(0);
    s
}

fn drawing_ids(s: &AppState) -> Vec<DrawingId> {
    let run = |sheet: strip::Sheet| match sheet.shows {
        Exposure::Drawing(id) => Some(id),
        Exposure::Empty => None,
    };
    strip::sheets(s).into_iter().filter_map(run).collect()
}

type Bare = egui_kittest::Harness<'static, (AppState, Strip, bool)>;

/// The strip alone. The bool is `gesture_safe`.
fn bare(state: AppState, safe: bool) -> Bare {
    let size = vec2(1600.0, rail::STRIP_H + 40.0);
    let builder = egui_kittest::Harness::builder().with_size(size).with_step_dt(1.0 / 60.0);
    builder.build_ui_state(
        |ui, (state, strip, safe): &mut (AppState, Strip, bool)| {
            plate::install_fonts(ui.ctx());
            strip.begin();
            strip::show(ui, state, strip, *safe);
        },
        (state, Strip::default(), safe),
    )
}

fn jobs(h: &Bare) -> u32 {
    h.state().1.picture_jobs()
}

// ---- the recipe ----

/// Spec AU13 item 1: the paper, then Colors, Check and Result as tabs. The
/// frozen boot, which owes nobody a new layout, keeps today's split.
#[test]
fn aucolor_recipe_is_canvas_and_right_tabs() {
    for (w, tall) in SIZES {
        let h = color_room(w, tall);
        assert_eq!(panes(&h), [Pane::Canvas, Pane::Palette, Pane::PaintCheck, Pane::Viewer(0)]);
        let (canvas, tabs) = (pane_rect(&h, Pane::Canvas), pane_rect(&h, Pane::Palette));
        assert!(canvas.right() <= tabs.left(), "{canvas:?} {tabs:?}");
        let share = canvas.width() / (tabs.right() - canvas.left());
        assert!((share - 0.78).abs() < 0.02, "{share}");
        assert!(h.has(vocab::ADD_COLOR), "Colors is the tab on show: {}", h.describe());
    }
    let mut h = EditorHarness::new();
    enter(&mut h, Stage::InkPaint);
    assert!(same_shape(&h.editor().dock, &Stage::dock_with_result()));
}

/// One click brings the old split back, and one click takes it away.
#[test]
fn aucolor_result_beside_restores_todays_split() {
    let mut h = color_room(1600.0, 900.0);
    let plain = Stage::InkPaint.dock();
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    assert!(same_shape(&h.editor().dock, &Stage::dock_with_result()), "{:?}", panes(&h));
    for pane in [Pane::XSheet, Pane::Layers, Pane::Viewer(0), Pane::Palette, Pane::PaintCheck] {
        assert!(panes(&h).contains(&pane), "{pane:?}");
    }
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    assert!(same_shape(&h.editor().dock, &plain), "{:?}", panes(&h));
}

/// The choice stands across the steps, and it reaches the disk at the click.
#[test]
fn aucolor_result_beside_is_remembered() {
    let mut h = color_room(1600.0, 900.0);
    assert!(!Config::load_from(h.data_dir()).0.ui.result_in_color);
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    assert!(Config::load_from(h.data_dir()).0.ui.result_in_color, "saved at the click");
    enter(&mut h, Stage::Drawing);
    assert!(!panes(&h).contains(&Pane::PaintCheck), "Draw has its own recipe");
    enter(&mut h, Stage::InkPaint);
    assert!(same_shape(&h.editor().dock, &Stage::dock_with_result()), "{:?}", panes(&h));
}

/// The sheet's restart test, on this choice.
#[test]
fn aucolor_result_choice_survives_a_restart() {
    let mut first = EditorHarness::as_shipped();
    settle(&mut first);
    enter(&mut first, Stage::InkPaint);
    first.click(vocab::RESULT_BESIDE);
    settle(&mut first);
    let mut second = EditorHarness::with_dir_shipped(first.quit());
    settle(&mut second);
    enter(&mut second, Stage::InkPaint);
    assert!(same_shape(&second.editor().dock, &Stage::dock_with_result()), "held over the restart");
    second.click(vocab::RESULT_BESIDE);
    settle(&mut second);
    assert!(!Config::load_from(second.data_dir()).0.ui.result_in_color, "saved at the click");
    let mut third = EditorHarness::with_dir_shipped(second.quit());
    settle(&mut third);
    enter(&mut third, Stage::InkPaint);
    assert!(same_shape(&third.editor().dock, &Stage::InkPaint.dock()), "off over the restart");
}

/// Must not change: neither layout lets a fill reach the line art.
#[test]
fn aucolor_protect_line_art_is_still_forced() {
    let mut h = color_room(1600.0, 900.0);
    assert!(h.state().view.line_guard);
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    assert!(h.state().view.line_guard, "beside");
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    assert!(h.state().view.line_guard, "and back");
    enter(&mut h, Stage::Drawing);
    assert!(!h.state().view.line_guard, "Color's own");
}

/// AU18's review: a comic's Color has no sheet in it.
#[test]
fn aucolor_comic_color_has_no_sheet() {
    let mut h = EditorHarness::with_state_shipped(templates::ALL[templates::COMIC].build());
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h, Stage::InkPaint);
    assert_eq!(panes(&h), [Pane::Canvas, Pane::Palette, Pane::PaintCheck, Pane::Viewer(0)]);
    assert!(h.has(vocab::ADD_COLOR));
}

// ---- the swatches ----

/// A swatch arms a color. Nothing in the document moves, for all ten.
#[test]
fn aucolor_swatch_changes_no_document() {
    let mut h = color_room(1600.0, 900.0);
    let mark = h.state().engine.position();
    assert!(!h.state().is_dirty());
    assert_eq!(plate::STARTER_SWATCHES.len(), 10);
    for (name, color) in plate::STARTER_SWATCHES {
        h.click(&well(name));
        settle(&mut h);
        assert_eq!(h.editor().canvas.brush_color, color.to_array(), "{name}");
        assert!(!h.state().is_dirty(), "{name}");
    }
    assert_eq!(h.state().engine.position(), mark);
    assert!(h.state().palettes.characters.is_empty(), "no character came of it");
}

/// The spec's ten, to the digit.
#[test]
fn aucolor_swatches_are_the_ten_in_the_spec() {
    let hex = plate::STARTER_SWATCHES.map(|(_, c)| format!("{:02X}{:02X}{:02X}", c.r(), c.g(), c.b()));
    let want = ["000000", "FFFFFF", "D93A2B", "F2994A", "F2C94C", "3FA34D", "2F7FD8", "8A5CC7", "F2C9A1", "8B5E3C"];
    assert_eq!(hex, want);
}

// ---- Add color ----

fn popover(h: &EditorHarness) -> Option<Rect> {
    let id = egui::Id::new(ADDING);
    h.ctx().memory(|m| {
        let shown = m.areas().visible_layer_ids().iter().any(|l| l.id == id);
        m.area_rect(id).filter(|_| shown)
    })
}

/// Three picks on the picker's square, in the popover's lower half.
const PICKS: [(f32, f32); 3] = [(0.3, 0.55), (0.7, 0.65), (0.5, 0.48)];

/// Press inside the open popover, at this fraction of it.
fn pick(h: &mut EditorHarness, x: f32, y: f32) {
    let at = popover(h).expect("the popover");
    h.click_pos(at.min + vec2(at.width() * x, at.height() * y));
    settle(h);
}

fn role_names(h: &EditorHarness) -> Vec<String> {
    h.state().palettes.characters.iter().flat_map(|c| c.roles.iter().map(|r| r.name.clone())).collect()
}

/// The first press makes the character nobody was asked to name. The empty
/// board says so, with the button standing by.
#[test]
fn aucolor_add_color_makes_the_character_silently() {
    let mut h = color_room(1600.0, 900.0);
    assert!(says(&h, NO_COLORS), "{:?}", h.painted_text());
    in_pane(&h, Pane::Palette, vocab::ADD_COLOR);
    let depth = h.state().engine.history_len();
    h.editor_mut().canvas.brush_color = [10, 20, 30, 255];
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    let characters = &h.state().palettes.characters;
    assert_eq!(characters.len(), 1);
    assert_eq!(characters[0].name, DEFAULT_CHARACTER);
    assert_eq!(role_names(&h), ["Color 1"], "one color, none it did not ask for");
    assert_eq!(characters[0].roles[0].color, [10, 20, 30, 255], "what the brush held");
    assert!(!h.state().palette_edit, "and no latch was needed");
    assert!(h.state().meta_dirty, "it is the person's work");
    assert_eq!(h.state().engine.history_len(), depth, "the palette stays out of undo");
    assert!(!says(&h, NO_COLORS));
    // The next one joins it.
    h.key(Key::Escape);
    settle(&mut h);
    h.click(vocab::ADD_COLOR);
    h.key(Key::Escape);
    settle(&mut h);
    assert_eq!(h.state().palettes.characters.len(), 1);
    assert_eq!(role_names(&h), ["Color 1", "Color 2"]);
    // Its pot stands in the closed board and arms what it holds.
    h.state_mut().palettes.characters[0].roles[1].color = [200, 10, 10, 255];
    settle(&mut h);
    h.click("paint pot: Color 2");
    settle(&mut h);
    assert_eq!(h.editor().canvas.brush_color, [200, 10, 10, 255]);
    assert_eq!(h.state().active_chip, Some(1));
}

/// A character the person made keeps taking colors, and a removed one does
/// not reuse a number still in use.
#[test]
fn aucolor_add_color_joins_the_active_character() {
    let mut h = color_room(1600.0, 900.0);
    let mut ana = CharacterPalette::new("Ana");
    ana.roles = vec![ColorRole { name: "Color 2".into(), color: [1, 2, 3, 255] }];
    h.state_mut().palettes.characters.push(ana);
    settle(&mut h);
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    assert_eq!(h.state().palettes.characters.len(), 1);
    assert_eq!(role_names(&h), ["Color 2", "Color 3"]);
}

/// Spec AU13 item 3: it stays open across picks, the picks land on the new
/// color and the brush, and only a press elsewhere or Esc closes it.
#[test]
fn aucolor_popover_stays_open_across_picks() {
    let mut h = color_room(1600.0, 900.0);
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    assert!(popover(&h).is_some(), "{}", h.describe());
    let mut seen = Vec::new();
    for (x, y) in PICKS {
        pick(&mut h, x, y);
        assert!(popover(&h).is_some(), "open after the pick");
        let color = h.state().palettes.characters[0].roles[0].color;
        assert_eq!(h.editor().canvas.brush_color, color, "the brush holds the pick");
        assert!(!seen.contains(&color), "{color:?} in {seen:?}");
        seen.push(color);
    }
    // A press on the button starts the next color, under the same popover.
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    assert!(popover(&h).is_some(), "still open");
    assert_eq!(h.state().palette_adding, Some((0, 1)));
    pick(&mut h, 0.2, 0.6);
    let roles = &h.state().palettes.characters[0].roles;
    assert_eq!(roles[0].color, *seen.last().unwrap(), "the first color stands");
    assert_ne!(roles[1].color, roles[0].color);
    h.key(Key::Escape);
    settle(&mut h);
    assert!(popover(&h).is_none(), "Esc");
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    assert!(popover(&h).is_some());
    h.click(&well("Blue"));
    settle(&mut h);
    assert!(popover(&h).is_none(), "a press elsewhere");
    assert_eq!(h.editor().canvas.brush_color, plate::STARTER_SWATCHES[6].1.to_array(), "and it did its own work");
}

/// Spec AU13 item 4: the header is above the scroll, however long the board.
#[test]
fn aucolor_edit_colors_is_never_below_the_fold() {
    fn offer(state: &mut AppState) {
        state.recolour_offer = Some(crate::paintcheck::RecolourOffer {
            character: 0,
            role: 0,
            name: "skin".into(),
            from: [1, 2, 3, 255],
            to: [4, 5, 6, 255],
        });
    }
    let long = || {
        let mut ana = CharacterPalette::new("Ana");
        ana.roles = (0..40).map(|n| ColorRole { name: format!("Color {n}"), color: [n, 40, 90, 255] }).collect();
        ana
    };
    for (w, tall) in SIZES {
        let mut h = color_room(w, tall);
        for name in [vocab::ADD_COLOR, vocab::EDIT_COLORS, vocab::RESULT_BESIDE] {
            in_pane(&h, Pane::Palette, name);
        }
        h.state_mut().palettes.characters.push(long());
        offer(h.state_mut());
        settle(&mut h);
        h.click(vocab::COLOR_DETAILS);
        settle(&mut h);
        h.click(vocab::EDIT_COLORS);
        settle(&mut h);
        for name in [vocab::ADD_COLOR, vocab::EDIT_COLORS, vocab::RESULT_BESIDE] {
            in_pane(&h, Pane::Palette, name);
        }
        h.scroll_to("REMOVE CHARACTER");
        settle(&mut h);
        in_pane(&h, Pane::Palette, vocab::EDIT_COLORS);
        in_pane(&h, Pane::Palette, "REMOVE CHARACTER");
    }
    // The narrowest, shortest pane the dock allows.
    let mut s = AppState::new_project("t", 64, 48, 24, 72.0);
    s.palettes.characters.push(long());
    offer(&mut s);
    let size = vec2(160.0, 140.0);
    let p = PanelHarness::new(size, (s, crate::canvas::CanvasView::new()), |ui, (st, cv): &mut (AppState, _)| {
        crate::palette_panel::ui(ui, st, cv, Default::default());
    });
    let (add, edit) = (p.rect(vocab::ADD_COLOR), p.rect(vocab::EDIT_COLORS));
    assert!(edit.bottom() <= size.y && add.bottom() <= size.y, "{add:?} {edit:?}");
}

/// Disclosed: names and groups stay out of the way until they are asked for.
#[test]
fn aucolor_details_disclose_names_and_groups() {
    let mut h = color_room(1600.0, 900.0);
    let mut ana = CharacterPalette::new("Ana");
    ana.roles = vec![ColorRole { name: "skin".into(), color: [240, 200, 160, 255] }];
    h.state_mut().palettes.characters.push(ana);
    settle(&mut h);
    assert!(h.has("paint pot: skin"));
    assert!(!says(&h, "skin"), "the pot alone");
    assert!(!h.has("Ana"));
    h.click(vocab::COLOR_DETAILS);
    settle(&mut h);
    assert!(says(&h, "skin"));
    assert!(h.has("Ana"));
    assert!(h.has(&format!("+ {}", vocab::COLOR_SET)));
    assert!(!h.has("remove"), "the wheel and the holds stay behind the latch");
}

// ---- a held frame ----

fn beside_on(h: &mut EditorHarness) {
    enter(h, Stage::InkPaint);
    h.click(vocab::RESULT_BESIDE);
    settle(h);
}

/// Spec AU13 item 6: on a held frame the Layers pane has one row, which
/// changes nothing, and the Say lane reads the hold.
#[test]
fn aucolor_held_frame_row_is_read_only() {
    let mut h = EditorHarness::with_state_shipped(two_inked(3));
    sized(&mut h, 1600.0, 900.0);
    beside_on(&mut h);
    assert!(!h.has(vocab::GO_TO_THIS_DRAWING), "a key has its layers");
    h.state_mut().goto(1);
    settle(&mut h);
    assert!(h.state().strip_layers().is_empty());
    assert!(h.has(vocab::GO_TO_THIS_DRAWING), "{}", h.describe());
    assert!(h.painted_text().iter().any(|w| w.starts_with("Held from frame 1")), "{:?}", h.painted_text());
    let mark = h.state().engine.position();
    settle(&mut h);
    assert_eq!(h.state().engine.position(), mark);
}

/// The row's button goes to the drawing's first frame, and waits for a pen.
#[test]
fn aucolor_go_to_this_drawing() {
    let mut h = EditorHarness::with_state_shipped(two_inked(3));
    sized(&mut h, 1600.0, 900.0);
    beside_on(&mut h);
    h.state_mut().goto(4);
    settle(&mut h);
    let panel = crate::manga_tools::KomaDrag { from: [0.0; 2], to: [1.0; 2], handle: None, reach: 0.0 };
    h.editor_mut().canvas.koma = Some(panel);
    h.click(vocab::GO_TO_THIS_DRAWING);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 4, "not under a live stroke");
    assert_eq!(h.state().refusal, doc::FINISH_STROKE);
    h.editor_mut().canvas.koma = None;
    settle(&mut h);
    h.click(vocab::GO_TO_THIS_DRAWING);
    settle(&mut h);
    assert_eq!(h.state().view.frame, 3, "the drawing's own frame");
    assert!(!h.has(vocab::GO_TO_THIS_DRAWING), "its layers now");
}

// ---- the strip's pictures ----

/// Spec 7.2: the key is the content's. Time changes no picture; ink does, and
/// only that drawing's.
#[test]
fn aucolor_thumbs_key_on_content() {
    let s = two_inked(3);
    let ids = drawing_ids(&s);
    let mut h = bare(s, true);
    for _ in 0..5 {
        h.step();
    }
    assert_eq!(jobs(&h), 2, "one each");
    assert!(ids.iter().all(|id| h.state().1.pictured(*id)));
    let key = |h: &Bare, n: usize| h.state().0.drawing_composite(ids[n], None, false).unwrap().1;
    let first = key(&h, 0);
    assert_eq!(h.state().1.picture_key(ids[0]), Some(first));
    h.state_mut().0.set_cut_length(12);
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(jobs(&h), 2, "a longer cut holds the same ink");
    h.state_mut().0.goto(0);
    blot(&mut h.state_mut().0, 200);
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(jobs(&h), 3, "the changed drawing alone");
    assert_ne!(key(&h, 0), first);
    assert_eq!(h.state().1.picture_key(ids[0]), Some(key(&h, 0)));
    h.state_mut().0.undo();
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(h.state().1.picture_key(ids[0]), Some(first), "undo is content too");
}

/// Spec AU13's gate: no job while a stroke is live, and one job at a time.
#[test]
fn aucolor_thumbs_wait_for_a_gesture() {
    let s = two_inked(3);
    let ids = drawing_ids(&s);
    let mut h = bare(s, false);
    for _ in 0..6 {
        h.step();
    }
    assert_eq!(jobs(&h), 0);
    h.state_mut().2 = true;
    h.step();
    assert_eq!(jobs(&h), 1, "one a frame");
    h.step();
    assert_eq!(jobs(&h), 2);
    // Ink landing under a held pen leaves the old picture where it is.
    h.state_mut().2 = false;
    let old = h.state().1.picture_key(ids[0]);
    blot(&mut h.state_mut().0, 200);
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(jobs(&h), 2);
    assert_eq!(h.state().1.picture_key(ids[0]), old);
    assert!(h.state().1.pictured(ids[0]));
    h.state_mut().2 = true;
    h.step();
    assert_eq!(jobs(&h), 3);
}

/// The real route: the job on its own thread, one out at a time.
#[test]
fn aucolor_thumbs_run_off_the_ui_thread() {
    let s = two_inked(3);
    let ids = drawing_ids(&s);
    let mut h = bare(s, true);
    h.state_mut().1.pictures_aside();
    let mut frames = 0;
    while !ids.iter().all(|id| h.state().1.pictured(*id)) {
        h.step();
        let strip = &h.state().1;
        let landed = ids.iter().filter(|id| strip.pictured(**id)).count() as u32;
        assert!(strip.picture_jobs() <= landed + 1, "one out at a time");
        frames += 1;
        assert!(frames < 2000, "the pictures never came");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!h.state().1.picture_out());
    assert_eq!(jobs(&h), 2);
}

/// A sheet off the strip's screen costs nothing.
#[test]
fn aucolor_thumbs_are_for_visible_sheets() {
    let mut s = AppState::new_project("t", 640, 360, 24, 72.0);
    s.set_cut_length(400);
    for at in [0, 390] {
        s.goto(at);
        s.new_drawing_at_frame();
        blot(&mut s, 60);
    }
    s.goto(0);
    let ids = drawing_ids(&s);
    let mut h = bare(s, true);
    for _ in 0..5 {
        h.step();
    }
    assert_eq!(jobs(&h), 1, "the last sheet stands past the window");
    assert!(h.state().1.pictured(ids[0]) && !h.state().1.pictured(ids[1]));
}

/// Spec AU13 item 8: a narrow sheet keeps its number, a wide one has both,
/// and a drawing with nothing to show keeps its number too.
#[test]
fn aucolor_narrow_sheet_keeps_its_number() {
    let mut s = AppState::new_project("t", 640, 360, 24, 72.0);
    s.set_cut_length(152);
    for at in [0, 2] {
        s.goto(at);
        s.new_drawing_at_frame();
        blot(&mut s, 60);
    }
    s.goto(0);
    let ids = drawing_ids(&s);
    let mut h = bare(s, true);
    for _ in 0..3 {
        h.step();
    }
    assert!(!h.state().1.pictured(ids[0]), "{:?}", strip::sheets(&h.state().0));
    assert!(h.state().1.pictured(ids[1]));
    assert_eq!(jobs(&h), 1, "the narrow sheet asked for none");
    let painted: Vec<String> = h.output().shapes.iter().filter_map(|c| match &c.shape {
        egui::Shape::Text(t) => Some(t.galley.text().to_string()),
        _ => None,
    }).collect();
    assert!(["1", "2"].iter().all(|n| painted.iter().any(|w| w == n)), "{painted:?}");
    // Nothing raster on a sheet: no picture, and its number.
    let mut blank = AppState::new_project("t", 640, 360, 24, 72.0);
    blank.new_drawing_at_frame();
    let ids = drawing_ids(&blank);
    let mut h = bare(blank, true);
    for _ in 0..3 {
        h.step();
    }
    assert!(!h.state().1.pictured(ids[0]));
}

fn opaque(rgba: [f32; 4]) -> Arc<TileData> {
    let bits = rgba.map(f32_to_f16_bits);
    let tile: Vec<u16> = (0..TILE_LEN).map(|i| bits[i % 4]).collect();
    Arc::new(TileData::from_vec(tile))
}

/// The picture's arithmetic: the frame on white, top layer over bottom.
#[test]
fn aucolor_picture_is_the_drawing_on_white() {
    let red: strip::PictureLayer = (std::iter::once(((0, 0), opaque([1.0, 0.0, 0.0, 1.0]))).collect(), 1.0);
    let image = strip::picture(std::slice::from_ref(&red), [TILE * 2, TILE]).expect("a picture");
    assert_eq!(image.size, [96, 48]);
    let at = |x: usize, y: usize| image.pixels[y * image.size[0] + x];
    assert_eq!(at(10, 10).to_array(), [255, 0, 0, 255]);
    assert_eq!(at(60, 10).to_array(), [255; 4], "the paper");
    // Half a layer over it is half-way there.
    let over = (std::iter::once(((0, 0), opaque([0.0, 0.0, 0.5, 0.5]))).collect(), 1.0);
    let image = strip::picture(&[red.clone(), over], [TILE * 2, TILE]).expect("a picture");
    let mix = image.pixels[10 * 96 + 10];
    assert!(mix.r() < 200 && mix.b() > 80 && mix.g() == 0, "{mix:?}");
    // Off the frame, nothing shows.
    let off = (std::iter::once(((5, 5), opaque([1.0, 0.0, 0.0, 1.0]))).collect(), 1.0);
    assert!(strip::picture(&[off], [TILE, TILE]).is_none());
    assert!(strip::picture(&[], [TILE, TILE]).is_none());
    // A one-pixel line on a big frame still shows.
    let bits = [0.0f32, 0.0, 0.0, 1.0].map(f32_to_f16_bits);
    let mut tile = vec![0u16; TILE_LEN];
    for x in 0..TILE {
        tile[(30 * TILE + x) * 4..][..4].copy_from_slice(&bits);
    }
    let tile = Arc::new(TileData::from_vec(tile));
    let line = ((0..30).map(|tx| ((tx, 8), tile.clone())).collect(), 1.0);
    let image = strip::picture(&[line], [1920, 1080]).expect("a picture");
    let darkest = image.pixels.iter().map(|p| p.r()).min().unwrap();
    assert!(darkest < 200, "{darkest}");
}

// ---- the journey ----

/// A closed ring of ink round the paper's middle, drawn with the pen.
fn ringed_paper() -> (EditorHarness, Pos2) {
    let mut h = EditorHarness::with_gpu_ready();
    sized(&mut h, 1600.0, 900.0);
    assert!(h.editor().canvas.raster, "a paint layer is up");
    let c = h.editor().canvas.dbg_rect.center();
    let points: Vec<Pos2> = (0..=40)
        .map(|n| {
            let at = n as f32 / 40.0 * std::f32::consts::TAU;
            c + 120.0 * vec2(at.cos(), at.sin())
        })
        .collect();
    h.drag_path(&points);
    h.run_frames(8);
    assert_eq!(h.state().strokes_done, 1, "{}", h.state().status);
    (h, c)
}

fn color_tiles(h: &EditorHarness) -> usize {
    let at = h.state().current_drawing().expect("the drawing");
    let d = h.state().cut().drawing(at).expect("in the cut");
    d.layers.iter().filter(|l| l.props.role == LayerRole::Color).map(|l| l.raster.tiles.len()).sum()
}

fn line_tiles(h: &EditorHarness) -> Vec<u64> {
    let at = h.state().current_drawing().expect("the drawing");
    let d = h.state().cut().drawing(at).expect("in the cut");
    let lines = d.layers.iter().filter(|l| l.props.role != LayerRole::Color);
    lines.flat_map(|l| l.raster.tiles.values().map(|t| t.hash)).collect()
}

/// Spec 9's J3 on the real device: a first fill in three, and three colors of
/// one's own in eight, on the color layer. The journeys' `New concept` column
/// is by hand, so it stands by the step that meets it.
#[test]
#[ignore = "needs a GPU adapter"]
fn aucolor_fill_journey() {
    fn tap<'a>(at: Pos2) -> Act<'a> {
        Act::Answer(Box::new(move |h: &mut EditorHarness| h.click_pos(at)))
    }
    fn lane(h: &EditorHarness, read: &mut Vec<String>) {
        read.extend(h.editor().say.hint.as_ref().map(|s| s.words.clone()));
    }
    let mut read: Vec<String> = ["Color", "Red", vocab::ADD_COLOR].map(String::from).to_vec();
    let mut concepts = Vec::new();

    // The first fill.
    let (mut h, c) = ringed_paper();
    let lines = line_tiles(&h);
    let mut r = Recorder::new(&mut h, "first fill");
    r.act("go to Color", Act::Click("Color"));
    settle(r.harness());
    lane(r.harness(), &mut read);
    concepts.push("a color fills up to the lines");
    r.act("pick red", Act::Click("colour well: Red"));
    settle(r.harness());
    lane(r.harness(), &mut read);
    r.act("tap the ring's inside", tap(c));
    settle(r.harness());
    r.harness().run_frames(8);
    lane(r.harness(), &mut read);
    let tally = r.tally();
    assert!(tally.actions() <= 3, "{tally:?}");
    assert!(h.state().fills_done >= 1, "{} / {}", h.state().refusal, h.state().status);
    assert!(color_tiles(&h) > 0, "pixels on the color layer");
    assert_eq!(line_tiles(&h), lines, "and none on the line art");
    drop(h);

    // Three colors of one's own.
    let (mut h, c) = ringed_paper();
    let lines = line_tiles(&h);
    let mut r = Recorder::new(&mut h, "own colors");
    r.act("go to Color", Act::Click("Color"));
    settle(r.harness());
    concepts.push("a color can be kept in the project");
    for (n, (x, y)) in PICKS.into_iter().enumerate() {
        r.act("add a color", Act::Click(vocab::ADD_COLOR));
        settle(r.harness());
        lane(r.harness(), &mut read);
        r.act("pick it", Act::Answer(Box::new(move |h: &mut EditorHarness| pick(h, x, y))));
        lane(r.harness(), &mut read);
        assert_eq!(r.harness().state().palettes.characters[0].roles.len(), n + 1);
    }
    r.act("tap the ring's inside", tap(c));
    settle(r.harness());
    r.harness().run_frames(8);
    let tally = r.tally();
    assert_eq!(tally.actions(), 8, "{tally:?}");
    assert!(tally.actions() <= 9);
    let own = h.state().palettes.characters[0].roles.iter().map(|r| r.color).collect::<Vec<_>>();
    assert!(own[0] != own[1] && own[1] != own[2], "{own:?}");
    assert!(h.state().fills_done >= 1, "{} / {}", h.state().refusal, h.state().status);
    assert!(color_tiles(&h) > 0);
    assert_eq!(line_tiles(&h), lines);
    assert!(concepts.len() <= 2, "{concepts:?}");
    let jargon = measure::jargon_in(&read);
    assert!(jargon.len() <= 1, "{jargon:?} in {read:?}");
}

/// The census's row for J3: 3 today, 0 here. Each journey control stands in
/// the window with no pointer over anything, outside every scroll.
#[test]
fn aucolor_no_journey_control_behind_hover_or_scroll() {
    for (w, tall) in SIZES {
        let h = color_room(w, tall);
        let window = h.ctx().content_rect();
        let views = plate::viewports(&h.ctx());
        let controls = [vocab::ADD_COLOR, vocab::EDIT_COLORS, "colour well: Red", "colour well: Black"];
        for name in controls {
            let at = h.rect(name);
            assert!(window.contains_rect(at), "{name}: {at:?}");
            let scrolled = views.iter().find(|v| v.rect.contains(at.center()));
            assert!(scrolled.is_none(), "{name} at {at:?} in {:?}", scrolled.map(|v| v.rect));
        }
        assert!(window.contains_rect(h.rect("Color")), "the step");
    }
}
