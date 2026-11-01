//! AU13's tests: the Color step stands canvas first, a first fill is three
//! taps, and the strip shows pictures.

use anim_core::ids::DrawingId;
use anim_core::model::LayerRole;
use anim_core::raster::{TILE, TILE_LEN, TileCoord, TileData, f16_bits_to_f32, f32_to_f16_bits};
use anim_core::xsheet::Exposure;
use eframe::egui;
use egui::{Key, Pos2, Rect, vec2};
use std::sync::Arc;

use crate::canvas::extpaper_tests::paint_texels;
use crate::config::Config;
use crate::doc::{self, AppState};
use crate::measure::{self, Act, Recorder};
use crate::palette::{CharacterPalette, ColorRole};
use crate::palette_panel::{ADDING, DEFAULT_CHARACTER, NO_COLORS, PICKER_W};
use crate::strip::{self, Strip};
use crate::testkit::{EditorHarness, PanelHarness};
use crate::workspace::{Pane, Stage};
use crate::{auguide_tests, aulesson_tests, plate, rail, same_shape, say, templates, vocab};

const SIZES: [(f32, f32); 2] = [(1600.0, 900.0), (1280.0, 800.0)];
const INK: [f32; 4] = [0.1, 0.1, 0.1, 1.0];
/// The sentence Color's Say line opens on (spec AU13 item 7).
const PICK_THEN_TAP: &str = "Pick a color, then tap an area.";

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

/// The Color step of a new animation.
fn film_color_room(w: f32, tall: f32) -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(auguide_tests::film().build());
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
    format!("{}: {name}", vocab::COLOR_WELL)
}

fn kept(name: &str) -> String {
    format!("{}: {name}", vocab::KEPT_COLOR)
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

/// One click brings the old split back on an animation, and one click takes
/// it away.
#[test]
fn aucolor_result_beside_restores_todays_split() {
    let mut h = film_color_room(1600.0, 900.0);
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
    let mut h = film_color_room(1600.0, 900.0);
    assert!(!Config::load_from(h.data_dir()).0.ui.result_in_color);
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    assert!(Config::load_from(h.data_dir()).0.ui.result_in_color, "saved at the click");
    enter(&mut h, Stage::Drawing);
    assert!(!panes(&h).contains(&Pane::PaintCheck), "Draw has its own recipe");
    enter(&mut h, Stage::InkPaint);
    assert!(same_shape(&h.editor().dock, &Stage::dock_with_result()), "{:?}", panes(&h));
}

/// The sheet's restart test, on this choice. The ready paper is a drawing, so
/// the choice comes back without a sheet.
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
    assert!(same_shape(&second.editor().dock, &Stage::dock_with_result_no_sheet()), "held over the restart");
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

fn comic() -> AppState {
    templates::ALL[templates::COMIC].build()
}

#[track_caller]
fn beside_with_no_sheet(h: &EditorHarness) {
    assert!(same_shape(&h.editor().dock, &Stage::dock_with_result_no_sheet()), "{:?}", panes(h));
    assert!(!panes(h).contains(&Pane::XSheet), "{:?}", panes(h));
    for pane in [Pane::Layers, Pane::Viewer(0), Pane::Palette, Pane::PaintCheck] {
        assert!(panes(h).contains(&pane), "{pane:?}");
    }
}

/// AU18's review: a comic's Color has no sheet in it, and neither has a
/// drawing's, with `Result beside` on or off.
#[test]
fn aucolor_comic_color_has_no_sheet() {
    for mut h in [EditorHarness::with_state_shipped(comic()), EditorHarness::as_shipped()] {
        sized(&mut h, 1600.0, 900.0);
        enter(&mut h, Stage::InkPaint);
        assert_eq!(panes(&h), [Pane::Canvas, Pane::Palette, Pane::PaintCheck, Pane::Viewer(0)]);
        assert!(h.has(vocab::ADD_COLOR));
        h.click(vocab::RESULT_BESIDE);
        settle(&mut h);
        beside_with_no_sheet(&h);
        h.click(vocab::RESULT_BESIDE);
        settle(&mut h);
        assert_eq!(panes(&h), [Pane::Canvas, Pane::Palette, Pane::PaintCheck, Pane::Viewer(0)]);
    }
}

/// The memory is global: one press on an animation reaches every later
/// document, and it brings a sheet only where a sheet belongs.
#[test]
fn aucolor_animation_memory_puts_no_sheet_on_a_page() {
    let mut film = film_color_room(1600.0, 900.0);
    film.click(vocab::RESULT_BESIDE);
    settle(&mut film);
    assert!(panes(&film).contains(&Pane::XSheet));
    let mut h = EditorHarness::with_dir_shipped(film.quit());
    sized(&mut h, 1600.0, 900.0);
    assert!(h.rig().config.ui.result_in_color, "the memory stands");
    enter(&mut h, Stage::InkPaint);
    beside_with_no_sheet(&h);
    h.rebuild_editor(comic());
    settle(&mut h);
    enter(&mut h, Stage::InkPaint);
    beside_with_no_sheet(&h);
    h.rebuild_editor(auguide_tests::film().build());
    settle(&mut h);
    enter(&mut h, Stage::InkPaint);
    assert!(same_shape(&h.editor().dock, &Stage::dock_with_result()), "an animation's own: {:?}", panes(&h));
}

/// The card says what the latch brings, on each kind.
#[test]
fn aucolor_result_card_says_what_it_brings() {
    const SHEET: &str = "Show the Timing sheet, Layers and the Result beside the paper.";
    const NO_SHEET: &str = "Show Layers and the Result beside the paper.";
    for (mut h, words, not) in
        [(film_color_room(1600.0, 900.0), SHEET, NO_SHEET), (color_room(1600.0, 900.0), NO_SHEET, SHEET)]
    {
        h.hover(vocab::RESULT_BESIDE);
        settle(&mut h);
        assert!(says(&h, words), "{:?}", h.painted_text());
        assert!(!says(&h, not));
    }
}

/// Feedback spec 3.10's rule on this memory: it stops at a lesson's door, the
/// strip a Color task walks stands, a press holds for the run alone, and Leave
/// gives the person's memory back.
#[test]
fn aucolor_result_memory_stays_out_of_a_lesson() {
    let mut h = film_color_room(1600.0, 900.0);
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    assert!(h.editor().sheet_on_screen(), "the person's own");
    enter(&mut h, Stage::Drawing);
    // This asserts the step's own recipe, and raises the strip.
    auguide_tests::strip_up_for_color(&mut h);
    assert!(same_shape(&h.editor().dock, &Stage::InkPaint.dock()), "{:?}", panes(&h));
    let ring = h.ring_rect().expect("strip.next stands on the strip");
    assert!(plate::strip_sheets(&h.ctx()).iter().any(|sheet| sheet.intersects(ring)), "{ring:?}");
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    assert!(h.editor().sheet_on_screen(), "the key works");
    assert!(h.rig().config.ui.result_in_color && h.editor().result_in_color, "and the memory stands");
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    assert!(!h.editor().sheet_on_screen(), "and back");
    assert!(h.rig().config.ui.result_in_color && h.editor().result_in_color, "the press was the run's");
    assert!(Config::load_from(h.data_dir()).0.ui.result_in_color, "on the disk too");
    h.editor_mut().leave_lesson();
    settle(&mut h);
    assert!(same_shape(&h.editor().dock, &Stage::dock_with_result()), "Leave: {:?}", panes(&h));
}

/// The other way round: with the memory off a press in a run stores nothing.
#[test]
fn aucolor_a_press_in_a_lesson_is_not_remembered() {
    let mut h = film_color_room(1600.0, 900.0);
    auguide_tests::strip_up_for_color(&mut h);
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    assert!(h.editor().sheet_on_screen());
    assert!(!h.rig().config.ui.result_in_color && !h.editor().result_in_color);
    assert!(!Config::load_from(h.data_dir()).0.ui.result_in_color);
    h.editor_mut().leave_lesson();
    settle(&mut h);
    assert!(same_shape(&h.editor().dock, &Stage::InkPaint.dock()), "Leave: {:?}", panes(&h));
}

/// Color's memory stays Color's: Draw's paper and the pen's canvas read the
/// same with it on, on the screen a person draws on.
#[test]
fn aucolor_draw_shares_do_not_move() {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    let before = measure::paper_share(&h);
    assert!(before.paper >= 63.0 && before.pen_land >= 82.5, "{before:?}");
    enter(&mut h, Stage::InkPaint);
    h.click(vocab::RESULT_BESIDE);
    settle(&mut h);
    enter(&mut h, Stage::Drawing);
    assert_eq!(panes(&h), [Pane::Canvas]);
    assert_eq!(measure::paper_share(&h), before);
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

/// The tightest room a swatch stands in: Color beside the lesson card at
/// 832×520. Every well, lesson swatches among them, is whole in the pane.
#[test]
fn aucolor_swatches_stand_whole_in_the_tightest_pane() {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 832.0, 520.0);
    aulesson_tests::open(&mut h, "probe-film");
    enter(&mut h, Stage::InkPaint);
    let room = pane_rect(&h, Pane::Palette);
    assert!(room.width() < 160.0, "the fixture's premise: {room:?}");
    for (name, _) in plate::STARTER_SWATCHES {
        in_pane(&h, Pane::Palette, &well(name));
    }
    let side = |name: &str| h.rect(&well(name)).width();
    assert!(side("Red") >= plate::CTRL_H - 4.0, "the pen's floor: {}", side("Red"));
    h.click(&well("Brown"));
    settle(&mut h);
    assert_eq!(h.editor().canvas.brush_color, plate::STARTER_SWATCHES[9].1.to_array());
}

/// Each starter says its name on a pointer's rest.
#[test]
fn aucolor_swatch_says_its_name() {
    let mut h = color_room(1600.0, 900.0);
    h.hover(&well("Peach"));
    settle(&mut h);
    assert!(says(&h, "Peach"), "{:?}", h.painted_text());
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

/// The popover stands over the board, so it shows what the board would: the
/// new color's name and the character's colors, the newest ringed. A second
/// press in the color the brush already holds changes the bar over them not
/// at all, so this is all that says it happened.
#[test]
fn aucolor_popover_shows_what_it_keeps() {
    let mut h = color_room(1600.0, 900.0);
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    let at = popover(&h).expect("the popover");
    assert!(at.contains_rect(h.rect(&kept("Color 1"))), "{at:?}");
    assert!(h.painted_text_in(at).iter().any(|w| w == "Color 1"), "{:?}", h.painted_text_in(at));
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    let at = popover(&h).expect("the popover");
    assert!(h.painted_text_in(at).iter().any(|w| w == "Color 2"), "{:?}", h.painted_text_in(at));
    assert!(!h.painted_text_in(at).iter().any(|w| w == "Color 1"), "one heading");
    let (old, new) = (h.rect(&kept("Color 1")), h.rect(&kept("Color 2")));
    assert!(at.contains_rect(old) && at.contains_rect(new), "{at:?} {old:?} {new:?}");
    assert!(old.right() <= new.left(), "the newest is last");
    assert_eq!(h.node_toggled(&kept("Color 2")), Some(true), "and ringed");
    assert_eq!(h.node_toggled(&kept("Color 1")), Some(false));
    for _ in 0..8 {
        h.click(vocab::ADD_COLOR);
        settle(&mut h);
    }
    let at = popover(&h).expect("the popover");
    assert!(at.contains_rect(h.rect(&kept("Color 10"))), "the newest of many");
    assert!(!h.has(&kept("Color 1")), "the row holds the latest");
}

/// The square meets the row of numbers over it, and the taller popover stands
/// on the smallest window.
#[test]
fn aucolor_popover_has_no_gutter_and_fits() {
    let mut h = color_room(1600.0, 900.0);
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    let wide = popover(&h).expect("the popover").width();
    assert!(wide - PICKER_W <= 16.0, "{wide} over {PICKER_W}");
    let mut h = color_room(832.0, 520.0);
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    let at = popover(&h).expect("the popover");
    assert!(h.ctx().content_rect().contains_rect(at), "{at:?}");
}

/// A step key leaves no press behind, so the popover goes with the step, and
/// it does not come back on its own over a color that may be painted by then.
#[test]
fn aucolor_popover_goes_with_its_step() {
    let mut h = color_room(1600.0, 900.0);
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    assert!(popover(&h).is_some());
    enter(&mut h, Stage::Drawing);
    assert_eq!(h.state().palette_adding, None);
    enter(&mut h, Stage::InkPaint);
    assert!(popover(&h).is_none(), "not unasked");
    // Colors gone with no step change, as a dock swap does it.
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    assert!(h.state().palette_adding.is_some());
    h.editor_mut().dock = Stage::Drawing.dock();
    settle(&mut h);
    assert_eq!(h.state().palette_adding, None);
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
        // An edit far down the board raises its offer where it can be seen.
        for name in ["Recolor this shot", "Keep paint"] {
            in_pane(&h, Pane::Palette, name);
        }
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

/// Edit colors holds one way to add a color and no fold that does nothing, and
/// the wheel beside a pot says what it is.
#[test]
fn aucolor_edit_mode_has_one_add_and_no_dead_fold() {
    let mut h = color_room(1600.0, 900.0);
    let mut ana = CharacterPalette::new("Ana");
    ana.roles = vec![ColorRole { name: "skin".into(), color: [240, 200, 160, 255] }];
    h.state_mut().palettes.characters.push(ana);
    settle(&mut h);
    assert!(h.has(vocab::COLOR_DETAILS));
    h.click(vocab::EDIT_COLORS);
    settle(&mut h);
    assert!(!h.has(vocab::COLOR_DETAILS), "Edit colors shows it all");
    assert!(!h.has(&format!("+ {}", vocab::COLOR_NAME)), "{}", h.describe());
    assert!(h.has("remove"));
    h.click(vocab::ADD_COLOR);
    settle(&mut h);
    assert_eq!(role_names(&h), ["skin", "Color 2"], "named as Add color names it");
    h.key(Key::Escape);
    settle(&mut h);
    h.click(vocab::EDIT_COLORS);
    settle(&mut h);
    assert!(h.has(vocab::COLOR_DETAILS), "and it comes back");
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

fn hint(h: &EditorHarness) -> Option<String> {
    h.editor().say.hint.as_ref().map(|s| s.words.clone())
}

/// Spec AU13 item 6: on a held frame the Layers pane has one row, which
/// changes nothing, and the Say line reads the hold, in Color.
#[test]
fn aucolor_held_frame_row_is_read_only() {
    let mut h = EditorHarness::with_state_shipped(two_inked(3));
    sized(&mut h, 1600.0, 900.0);
    beside_on(&mut h);
    assert!(!h.has(vocab::GO_TO_THIS_DRAWING), "a key has its layers");
    h.state_mut().goto(1);
    h.state_mut().status.clear();
    settle(&mut h);
    assert!(h.state().strip_layers().is_empty());
    assert!(h.has(vocab::GO_TO_THIS_DRAWING), "{}", h.describe());
    let words = h.painted_text().into_iter().find(|w| w.starts_with("Held from frame 1"));
    let words = words.unwrap_or_else(|| panic!("{:?}", h.painted_text()));
    const HELD: &str = "This drawing stays on screen for 3 frames.";
    assert_eq!(hint(&h).as_deref(), Some(HELD));
    assert!(says(&h, HELD), "{:?}", h.painted_text());
    // A press on the row itself changes nothing.
    let (mark, row) = (h.state().engine.position(), h.painted_block(&words).0);
    h.click_pos(row.center());
    settle(&mut h);
    assert_eq!(h.state().engine.position(), mark);
    assert_eq!(h.state().view.frame, 1, "it is a row, not a button");
    assert!(h.has(vocab::GO_TO_THIS_DRAWING));
}

/// A drawing with vector ink alone lists no layers on its own key frame. That
/// frame is held by nothing, so the row does not stand there.
#[test]
fn aucolor_held_row_is_for_a_held_frame() {
    let mut s = two_inked(3);
    s.goto(0);
    let id = s.own_key_drawing().expect("a key");
    let at = s.at();
    let cut = s.engine.project.cut_mut(at.scene, at.cut).expect("the cut");
    cut.drawings.iter_mut().find(|d| d.id == id).expect("in the cut").layers.clear();
    assert!(s.strip_layers().is_empty() && s.exposure_here().is_some());
    let mut h = EditorHarness::with_state_shipped(s);
    sized(&mut h, 1600.0, 900.0);
    beside_on(&mut h);
    assert!(!h.has(vocab::GO_TO_THIS_DRAWING), "{}", h.describe());
}

/// Spec AU13 item 7: Color opens on the sentence that teaches it, with art on
/// the paper, and with none it stays quiet.
#[test]
fn aucolor_hint_says_pick_then_tap() {
    let h = color_room(1600.0, 900.0);
    assert_ne!(hint(&h).as_deref(), Some(PICK_THEN_TAP), "nothing to fill yet");
    let mut h = EditorHarness::with_state_shipped(two_inked(3));
    sized(&mut h, 1600.0, 900.0);
    enter(&mut h, Stage::InkPaint);
    assert_eq!(hint(&h).as_deref(), Some(PICK_THEN_TAP));
    h.state_mut().status.clear();
    settle(&mut h);
    assert!(says(&h, PICK_THEN_TAP), "{:?}", h.painted_text());
    assert_eq!(h.editor().say.hint.as_ref().and_then(|s| s.bit), Some(say::COLOR));
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

/// The commonest state of a working animator: one frame a drawing, and no room
/// beside the picture. The number stands on it.
#[test]
fn aucolor_a_sheet_of_one_frame_keeps_picture_and_number() {
    let mut s = two_inked(1);
    s.set_cut_length(12);
    let ids = drawing_ids(&s);
    let mut h = bare(s, true);
    for _ in 0..3 {
        h.step();
    }
    assert!(ids.iter().all(|id| h.state().1.pictured(*id)));
    let wide = plate::strip_sheets(&h.ctx)[0].width();
    assert!(wide < 56.0, "the fixture's premise: {wide}");
    let painted: Vec<String> = h.output().shapes.iter().filter_map(|c| match &c.shape {
        egui::Shape::Text(t) => Some(t.galley.text().to_string()),
        _ => None,
    }).collect();
    assert!(["1", "2"].iter().all(|n| painted.iter().any(|w| w == n)), "{painted:?}");
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
    assert_eq!(at(70, 10).to_array(), [255; 4], "the paper");
    // Half a layer over it is half-way there.
    let over = (std::iter::once(((0, 0), opaque([0.0, 0.0, 0.5, 0.5]))).collect(), 1.0);
    let image = strip::picture(&[red.clone(), over], [TILE * 2, TILE]).expect("a picture");
    let mix = image.pixels[10 * 96 + 10];
    assert!(mix.r() < 200 && mix.b() > 80 && mix.g() == 0, "{mix:?}");
    // Off the frame, nothing shows.
    let off = (std::iter::once(((5, 5), opaque([1.0, 0.0, 0.0, 1.0]))).collect(), 1.0);
    assert!(strip::picture(&[off], [TILE, TILE]).is_none());
    assert!(strip::picture(&[], [TILE, TILE]).is_none());
    // A pale wash stays pale: it is no line to be lifted.
    let wash = (std::iter::once(((0, 0), opaque([0.05, 0.0, 0.0, 0.05]))).collect(), 1.0);
    let image = strip::picture(&[wash], [TILE, TILE]).expect("a picture");
    assert!(image.pixels.iter().all(|p| p.g() >= 180), "{:?}", image.pixels[0]);
}

/// Every paper the New Project form allows, from 16 on a side: more picture
/// than paper leaves no cell empty, so a blank one has no picture and an inked
/// one has its ink and only that.
#[test]
fn aucolor_small_paper_has_a_true_picture() {
    let red: strip::PictureLayer = (std::iter::once(((0, 0), opaque([1.0, 0.0, 0.0, 1.0]))).collect(), 1.0);
    for paper in [[16, 16], [64, 48], [95, 51], [96, 52], [16, 16384], [16384, 16], [40, 300], [300, 40]] {
        assert!(strip::picture(&[], paper).is_none(), "{paper:?}: blank");
        let empty = (std::iter::once(((0, 0), opaque([0.0; 4]))).collect(), 1.0);
        assert!(strip::picture(&[empty], paper).is_none(), "{paper:?}: a tile of nothing");
        let image = strip::picture(std::slice::from_ref(&red), paper).expect("a picture");
        assert!(image.size[0] <= paper[0] && image.size[1] <= paper[1], "{paper:?}: {:?}", image.size);
        let black = image.pixels.iter().filter(|p| p.r() == 0).count();
        assert_eq!(black, 0, "{paper:?}: red or the paper, never a dark cell");
    }
    // At its own size a picture is the paper, pixel for pixel.
    let image = strip::picture(std::slice::from_ref(&red), [TILE / 2, TILE / 2]).expect("a picture");
    assert_eq!(image.size, [TILE / 2, TILE / 2]);
    assert!(image.pixels.iter().all(|p| p.to_array() == [255, 0, 0, 255]));
}

/// How far one pixel of `image` stands from white, against black's 21:1.
fn contrast_on_white(luminance: f32) -> f32 {
    1.05 / (luminance + 0.05)
}

fn linear(byte: u8) -> f32 {
    let c = byte as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The darkest a picture stands at the size a sheet draws it, read as the
/// renderer reads it: bilinear, in linear light, with no smaller copies of the
/// texture to read, from `shift` of a screen pixel along.
fn darkest_drawn(image: &egui::ColorImage, drawn: egui::Vec2, shift: f32) -> f32 {
    let [w, h] = image.size.map(|n| n as isize);
    let at = |x: isize, y: isize| linear(image.pixels[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize].g());
    let across = |n: f32, cells: f32, side: f32| (n + 0.5 + shift) * cells / side - 0.5;
    let mut darkest = 1.0f32;
    for py in 0..drawn.y as usize {
        for px in 0..drawn.x as usize {
            let (u, v) = (across(px as f32, w as f32, drawn.x), across(py as f32, h as f32, drawn.y));
            let (x, y, fx, fy) = (u.floor() as isize, v.floor() as isize, u - u.floor(), v - v.floor());
            let top = at(x, y) * (1.0 - fx) + at(x + 1, y) * fx;
            let foot = at(x, y + 1) * (1.0 - fx) + at(x + 1, y + 1) * fx;
            darkest = darkest.min(top * (1.0 - fy) + foot * fy);
        }
    }
    darkest
}

/// A one-pixel line of the default ink on full HD reads at 3:1 or better on a
/// sheet of one frame, wherever it falls and whichever way it runs.
#[test]
fn aucolor_a_thin_line_reads_on_its_sheet() {
    let ink = canvas_ink();
    let sheet = Rect::from_min_size(Pos2::ZERO, vec2(46.0, strip::SHEET_H));
    for offset in [0, 1, 3, 7, 11, 13, 17, 19] {
        let (flat, upright) = (TILE * 8 + offset, TILE * 12 + offset);
        let rows = (0..1080).map(|y| (y, upright)).collect::<Vec<_>>();
        let cols = (0..1920).map(|x| (flat, x)).collect::<Vec<_>>();
        for line in [line_layer(&cols, ink), line_layer(&rows, ink)] {
            let image = strip::picture(&[line], [1920, 1080]).expect("a picture");
            let drawn = strip::picture_rect(sheet, vec2(image.size[0] as f32, image.size[1] as f32)).size();
            assert!(drawn.x < 42.0, "the fixture's premise: {drawn:?}");
            for shift in [0.0, 0.25, 0.5, 0.75] {
                let contrast = contrast_on_white(darkest_drawn(&image, drawn, shift));
                assert!(contrast >= 3.0, "{contrast:.2} at offset {offset}, shift {shift}");
            }
        }
    }
}

/// The ink a new canvas holds, stored as a new project stores it.
fn canvas_ink() -> [f32; 4] {
    crate::canvas::SWATCHES[0].map(|c| c as f32 / 255.0)
}

/// One pixel of `ink` at each (row, column).
fn line_layer(pixels: &[(usize, usize)], ink: [f32; 4]) -> strip::PictureLayer {
    let bits = ink.map(f32_to_f16_bits);
    let mut tiles: std::collections::BTreeMap<TileCoord, Vec<u16>> = Default::default();
    for (y, x) in pixels {
        let tile = tiles.entry(((x / TILE) as i32, (y / TILE) as i32)).or_insert_with(|| vec![0; TILE_LEN]);
        tile[((y % TILE) * TILE + x % TILE) * 4..][..4].copy_from_slice(&bits);
    }
    (tiles.into_iter().map(|(at, tile)| (at, Arc::new(TileData::from_vec(tile)))).collect(), 1.0)
}

// ---- the journey ----

/// A closed ring of ink round the paper's middle, drawn with the pen. The
/// paper point it hands back is the ring's middle: a step's panes move the
/// paper on the screen, so a tap is worked out from it when it happens.
fn ringed_paper() -> (EditorHarness, Pos2) {
    let mut h = EditorHarness::with_gpu_ready();
    sized(&mut h, 1600.0, 900.0);
    assert!(h.editor().canvas.raster, "a paint layer is up");
    let c = h.editor().canvas.dbg_rect.center();
    // Past its start: a stroke tapers at both ends, and a ring that only
    // meets itself leaks.
    let points: Vec<Pos2> = (0..=44)
        .map(|n| {
            let at = n as f32 / 40.0 * std::f32::consts::TAU;
            c + 120.0 * vec2(at.cos(), at.sin())
        })
        .collect();
    h.drag_path(&points);
    h.run_frames(8);
    assert_eq!(h.state().strokes_done, 1, "{}", h.state().status);
    let middle = h.editor().canvas.view_xform().screen_to_paper(c);
    (h, middle)
}

fn color_tiles(h: &EditorHarness) -> usize {
    let at = h.state().current_drawing().expect("the drawing");
    let d = h.state().cut().drawing(at).expect("in the cut");
    d.layers.iter().filter(|l| l.props.role == LayerRole::Color).map(|l| l.raster.tiles.len()).sum()
}

/// The ring's middle took the color and the paper's corner, far outside it,
/// holds none: the fill stayed inside its lines.
#[track_caller]
fn stayed_in_the_ring(h: &EditorHarness, middle: Pos2) {
    let at = h.state().current_drawing().expect("the drawing");
    let d = h.state().cut().drawing(at).expect("in the cut");
    let colored = |tile: TileCoord| {
        let colors = d.layers.iter().filter(|l| l.props.role == LayerRole::Color);
        let solid = |t: &Arc<TileData>| t.rgba.chunks(4).any(|px| f16_bits_to_f32(px[3]) > 0.0);
        colors.filter_map(|l| l.raster.tiles.get(&tile)).any(solid)
    };
    let size = anim_core::raster::TILE as f32;
    assert!(colored(((middle.x / size) as i32, (middle.y / size) as i32)), "the ring's inside");
    assert!(!colored((0, 0)), "the fill leaked out of the ring");
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
    fn tap<'a>(paper: Pos2) -> Act<'a> {
        Act::Answer(Box::new(move |h: &mut EditorHarness| {
            let at = h.editor().canvas.view_xform().paper_to_screen(paper);
            h.click_pos(at)
        }))
    }
    fn lane(h: &EditorHarness, read: &mut Vec<String>) {
        read.extend(h.editor().say.hint.as_ref().map(|s| s.words.clone()));
    }
    // What the picker itself paints, the row of numbers over its square.
    fn popover_words(h: &EditorHarness, read: &mut Vec<String>) {
        read.extend(popover(h).map(|at| h.painted_text_in(at)).unwrap_or_default());
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
    r.act("pick red", Act::Click("color well: Red"));
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
    stayed_in_the_ring(&h, c);
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
        popover_words(r.harness(), &mut read);
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
    stayed_in_the_ring(&h, c);
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
        let controls = [vocab::ADD_COLOR.into(), vocab::EDIT_COLORS.into(), well("Red"), well("Black")];
        for name in controls {
            let at = h.rect(&name);
            assert!(window.contains_rect(at), "{name}: {at:?}");
            let scrolled = views.iter().find(|v| v.rect.contains(at.center()));
            assert!(scrolled.is_none(), "{name} at {at:?} in {:?}", scrolled.map(|v| v.rect));
        }
        assert!(window.contains_rect(h.rect("Color")), "the step");
    }
}
