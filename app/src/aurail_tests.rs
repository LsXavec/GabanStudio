//! AU4's tests: the rail and the floats (spec 4.3, 4.5).

use eframe::egui;
use egui::{Id, LayerId, Order, Pos2, Rect, pos2, vec2};

use crate::canvas::{CanvasTool, CanvasView, SWATCHES, SelShape};
use crate::config::{self, Action, BrushPreset};
use crate::doc::AppState;
use crate::floats::{Float, Floats};
use crate::measure::{self, Scope};
use crate::menus;
use crate::rail::{self, OverlayArgs, Tier};
use crate::testkit::{EditorHarness, PanelHarness};
use crate::workspace::Stage;
use crate::{PROBE_ROOMS, PROBE_SCALES, Pane, ProbeRoom, plate, sample, vocab};

const SIZES: [(f32, f32); 4] = [(1600.0, 900.0), (1280.0, 800.0), (1280.0, 720.0), (1024.0, 640.0)];
/// The floors of spec 4.1, by `SIZES`. They are never lowered.
const PAPER_FLOORS: [f32; 4] = [63.0, 66.5, 57.5, 60.0];
const PEN_FLOORS: [f32; 4] = [82.5, 80.0, 77.5, 74.0];
/// The rail, top down, from the second drawing on.
const KEYS: [&str; 9] = ["Brush", "Eraser", "Select", "Fill", "Hand", "Size", "Brush color", "Layers", "Flip"];

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

/// A window `w`×`h` points at 100%, at a stored scale, as the App zooms it.
fn window(harness: &mut EditorHarness, w: f32, h: f32, stored: f32) {
    let scale = crate::effective_scale(stored, w, h);
    harness.kit_mut().set_size(vec2(w / scale, h / scale));
    harness.kit_mut().set_pixels_per_point(scale);
    settle(harness);
}

fn points(h: &EditorHarness, name: &str) -> Rect {
    let (r, ppp) = (h.rect(name), h.ctx().pixels_per_point());
    Rect::from_min_max((r.min.to_vec2() / ppp).to_pos2(), (r.max.to_vec2() / ppp).to_pos2())
}

fn centre(h: &EditorHarness, name: &str) -> Pos2 {
    points(h, name).center()
}

/// A window of exactly these points: `window` zooms out a short one, so its
/// layout never comes to a small tier.
fn sized(harness: &mut EditorHarness, w: f32, h: f32) {
    harness.kit_mut().set_size(vec2(w, h));
    harness.kit_mut().set_pixels_per_point(1.0);
    settle(harness);
}

/// The sample cut, shipped, at 1600×900: a rail with its ninth key.
fn drawn() -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    window(&mut h, 1600.0, 900.0, 1.0);
    h
}

/// A blank document, shipped, at 1600×900: no drawing yet.
fn blank() -> EditorHarness {
    let mut h = EditorHarness::as_shipped();
    window(&mut h, 1600.0, 900.0, 1.0);
    h
}

/// An `Area`'s rect, while it is on screen.
fn area(h: &EditorHarness, id: Id) -> Option<Rect> {
    let shown = |m: &egui::Memory| m.areas().visible_layer_ids().iter().any(|l| l.id == id);
    h.ctx().memory(|m| shown(m).then(|| m.area_rect(id)).flatten())
}

fn island(h: &EditorHarness) -> Option<Rect> {
    area(h, Id::new("rail"))
}

fn float_rect(h: &EditorHarness, f: Float) -> Option<Rect> {
    area(h, Id::new(("float", f.heading())))
}

/// Every control the rail's layer holds, by its rect. The island's own
/// background, which keeps a pen off the paper, is none: it is the one
/// thing wider than a key.
fn rail_controls(h: &EditorHarness) -> Vec<Rect> {
    let layer = LayerId::new(Order::Middle, Id::new("rail"));
    h.ctx().viewport(|v| {
        let rows = v.prev_pass.widgets.get_layer(layer);
        rows.filter(|r| r.sense.interactive() && r.rect.width() <= 48.5).map(|r| r.rect).collect()
    })
}

/// The hover card on screen, if one is: the one Tooltip that takes no input.
fn card(h: &EditorHarness) -> Option<Rect> {
    let ctx = h.ctx();
    let shown = ctx.memory(|m| m.areas().visible_layer_ids());
    let silent = |l: &&LayerId| egui::AreaState::load(&ctx, l.id).is_some_and(|a| !a.interactable);
    let found = shown.iter().filter(|l| l.order == Order::Tooltip).find(silent);
    found.and_then(|l| ctx.memory(|m| m.area_rect(l.id)))
}

fn strokes(h: &EditorHarness) -> usize {
    h.state().cut().drawings.iter().map(|d| d.strokes.len()).sum()
}

fn is_open(h: &EditorHarness, f: Float) -> bool {
    h.editor().floats.is_open(f)
}

fn close(h: &mut EditorHarness, f: Float) {
    h.editor_mut().floats.close(f);
    settle(h);
}

/// A pencil's tile: its name carries its digit.
fn tile(h: &EditorHarness, pencil: &str) -> String {
    let presets = &h.rig().config.presets;
    let digit = 1 + presets.iter().position(|p| p.name == pencil).expect("a pencil");
    format!("{pencil} pencil ({digit})")
}

fn paper_middle(h: &EditorHarness) -> Pos2 {
    let xf = h.editor().canvas.view_xform();
    xf.paper_to_screen(pos2(xf.paper.x / 2.0, xf.paper.y / 2.0))
}

// ---- the rail ----

#[test]
fn aurail_order_and_sizes_are_the_table() {
    let mut h = drawn();
    h.editor_mut().strip_open = true;
    settle(&mut h);
    let keys: Vec<Rect> = KEYS.iter().map(|k| points(&h, k)).collect();
    for ((name, key), next) in KEYS.iter().zip(&keys).zip(keys.iter().skip(1)) {
        assert_eq!(key.bottom(), next.top(), "{name} touches the key below it");
    }
    for (name, key) in KEYS.iter().zip(&keys) {
        let tall = if *name == "Size" { 84.0 } else { 48.0 };
        assert_eq!(key.size(), vec2(48.0, tall), "{name}");
    }
    let rail = island(&h).expect("the rail");
    let host = h.editor().canvas.dbg_rect;
    assert_eq!(rail.min, pos2(host.left() + 12.0, 188.0), "spec 4.1's rail top");
    assert_eq!(rail.size(), vec2(56.0, Tier::Full.total()));
    assert_eq!(keys[0].min, rail.min + vec2(4.0, 4.0), "4 of margin");
    // Spec 4.1's other window.
    window(&mut h, 1280.0, 800.0, 1.0);
    assert_eq!(island(&h).expect("the rail").top(), 138.0);
}

#[test]
fn aurail_nothing_moves_when_the_island_grows() {
    let mut h = blank();
    h.state_mut().new_drawing_at_frame();
    settle(&mut h);
    let tops: Vec<Rect> = KEYS[..8].iter().map(|k| points(&h, k)).collect();
    let rail = island(&h).expect("the rail");
    assert!(!h.has("Flip"), "one drawing has nothing to flip to");
    assert_eq!(rail.height(), 428.0, "eight keys");
    let state = h.state_mut();
    state.view.frame = 4;
    state.new_drawing_at_frame();
    settle(&mut h);
    assert!(h.has("Flip"), "the second drawing brings it");
    assert_eq!(island(&h).expect("the rail").height(), 476.0, "nine keys");
    assert_eq!(island(&h).expect("the rail").min, rail.min, "the island grows down");
    for (name, was) in KEYS.iter().zip(&tops) {
        assert_eq!(points(&h, name), *was, "{name} moved");
    }
}

#[test]
fn aurail_reference_height_ignores_the_strip_latch() {
    assert_eq!(rail::reference(640.0, true), 584.0);
    assert_eq!(rail::reference(584.0, false), 584.0);
    for tall in [900.0, 600.0, 420.0] {
        let mut h = drawn();
        window(&mut h, 1280.0, tall, 1.0);
        let closed = rail_controls(&h);
        let pane = h.editor().canvas.dbg_rect;
        h.editor_mut().strip_open = true;
        settle(&mut h);
        let open = h.editor().canvas.dbg_rect;
        // Under 1:1 the panels stand on whole pixels, so this is the strip
        // to a third of a point.
        let took = pane.height() - open.height();
        assert!((took - rail::STRIP_H).abs() < 0.34, "the latch took {took} at {tall}");
        assert_eq!(rail_controls(&h), closed, "a rail control moved at {tall}");
    }
}

#[test]
fn aurail_armed_tool_opens_its_options() {
    let mut h = blank();
    assert!(h.editor().canvas.tool == CanvasTool::Paint, "the brush is armed");
    h.click("Brush");
    assert!(is_open(&h, Float::Brushes), "a click on the armed tool opens its options");
    h.click("Brush");
    assert!(is_open(&h, Float::Brushes), "the armed tool only opens");
    // A tool that is not armed arms, and leaves the floats alone.
    h.click("Eraser");
    assert!(h.editor().canvas.erasing && is_open(&h, Float::Brushes));
    h.click("Eraser");
    assert!(is_open(&h, Float::ToolOptions) && !is_open(&h, Float::Brushes));
    let shut = format!("Close {}", vocab::TOOL_OPTIONS);
    h.click(&shut);
    assert!(!is_open(&h, Float::ToolOptions));
    for (key, tool) in [("Select", CanvasTool::Select), ("Fill", CanvasTool::Fill)] {
        h.click(key);
        assert!(h.editor().canvas.tool == tool && !is_open(&h, Float::ToolOptions), "{key} arms");
        h.click(key);
        assert!(is_open(&h, Float::ToolOptions), "{key} opens");
        h.click(&shut);
    }
}

#[test]
fn aurail_hand_is_held_and_cleared_by_any_tool() {
    let mut h = blank();
    let c = paper_middle(&h);
    h.click("Hand");
    assert!(h.editor().canvas.hand_held);
    assert_eq!(h.node_toggled("Brush"), Some(false), "the tool waits behind the hand");
    let (pan0, strokes0) = (h.editor().canvas.view_xform().pan, strokes(&h));
    h.drag_path(&[c, c + vec2(30.0, 0.0), c + vec2(60.0, 20.0), c + vec2(90.0, 40.0)]);
    let pan1 = h.editor().canvas.view_xform().pan;
    assert!((pan1 - pan0).length() > 30.0, "the hand panned: {pan0:?} → {pan1:?}");
    assert!(h.editor().canvas.stroke_tap.is_empty() && strokes(&h) == strokes0, "the hand inks nothing");
    assert!(h.editor().canvas.hand_held, "a drag does not let it go");
    // Any tool lets go, by the rail, by a chord or by a pencil.
    h.click("Eraser");
    assert!(!h.editor().canvas.hand_held && h.editor().canvas.erasing);
    for tool in [Action::SelectTool, Action::ToggleEraser, Action::Preset1] {
        h.click("Hand");
        assert!(h.editor().canvas.hand_held, "{tool:?}");
        h.perform(tool);
        assert!(!h.editor().canvas.hand_held, "{tool:?} lets go");
    }
    h.click("Hand");
    h.click("Hand");
    assert!(!h.editor().canvas.hand_held, "its own key too");
}

#[test]
fn aurail_size_drag_sets_px_and_refuses_mid_stroke() {
    let mut h = blank();
    let px = |h: &EditorHarness| h.editor().canvas.active_px();
    let (start, at) = (px(&h), centre(&h, "Size"));
    h.drag(at, at - vec2(0.0, 40.0), 8);
    let up = px(&h);
    assert!(up > start, "up is bigger: {start} → {up}");
    h.drag(at, at + vec2(0.0, 40.0), 8);
    assert!((px(&h) - start).abs() / start < 0.02, "and down comes back: {start} → {}", px(&h));
    // The range holds: a long pull stops at its end.
    h.drag(at, at + vec2(0.0, 400.0), 8);
    assert_eq!(px(&h), h.editor().canvas.px_range().0);
    assert!(h.editor().canvas.stroke_tap.is_empty(), "the key is no paper");
    // A stroke's width is fixed at pen-down.
    let c = paper_middle(&h);
    h.press_and_move(c, c + vec2(40.0, 0.0), 4);
    assert!(h.editor().canvas.stroke_active());
    let held = px(&h);
    let editor = h.editor_mut();
    assert!(!editor.canvas.set_brush_px(held * 2.0, &mut editor.state), "refused");
    assert_eq!(px(&h), held);
    assert!(h.state().refusal.contains("refused"), "{}", h.state().refusal);
    h.release(c + vec2(40.0, 0.0));
}

#[test]
fn aurail_flip_is_momentary() {
    let mut h = drawn();
    let at = centre(&h, "Flip");
    h.press_and_move(at, at, 2);
    assert!(h.editor().canvas.rail_flip, "held");
    h.run();
    assert!(h.editor().canvas.deck_reads().flip_held, "the canvas flips, as under R");
    h.wait(1.2);
    assert!(h.editor().canvas.deck_reads().flip_held, "a long hold stays");
    h.release(at);
    settle(&mut h);
    assert!(!h.editor().canvas.rail_flip && !h.editor().canvas.deck_reads().flip_held, "it lets go");
    assert!(h.editor().canvas.stroke_tap.is_empty());
}

#[test]
fn aurail_pen_down_on_rail_never_paints() {
    let mut h = drawn();
    let before = strokes(&h);
    for key in KEYS {
        let r = points(&h, key);
        for at in [r.center(), r.min + vec2(1.0, 1.0), r.max - vec2(1.0, 1.0)] {
            h.pen_down(at);
            h.pen_move(at + vec2(3.0, 3.0));
            h.pen_up(at + vec2(3.0, 3.0));
            assert!(h.editor().canvas.stroke_tap.is_empty(), "{key} at {at:?} painted");
            // What the tap set stands aside, so the next one has a brush.
            let editor = h.editor_mut();
            editor.canvas.arm_paint(false, &mut editor.state);
            editor.canvas.hand_held = false;
            editor.floats = Floats::default();
            h.run();
        }
    }
    let rail = island(&h).expect("the rail");
    h.pen_tap(rail.min + vec2(1.0, 1.0));
    assert!(h.editor().canvas.stroke_tap.is_empty(), "nor its margin");
    assert_eq!(strokes(&h), before);
}

#[test]
fn aurail_pen_one_point_beside_paints() {
    let mut h = blank();
    let rail = island(&h).expect("the rail");
    let at = pos2(rail.right() + 1.0, rail.center().y);
    let before = strokes(&h);
    h.pen_down(at);
    h.pen_move(at + vec2(12.0, 0.0));
    h.pen_move(at + vec2(24.0, 4.0));
    h.pen_up(at + vec2(24.0, 4.0));
    assert!(!h.editor().canvas.stroke_tap.is_empty(), "1 pt beside the rail is paper's");
    assert_eq!(strokes(&h), before + 1);
}

/// The critic's replay (longtouch.rs): press, hold still, pull 80 pt at 1.2 s,
/// release. A control that also takes clicks loses its capture at 0.817 s.
#[test]
fn aurail_touch_replay_on_size_is_harmless() {
    let mut h = blank();
    let px = |h: &EditorHarness| h.editor().canvas.active_px();
    let (start, at) = (px(&h), centre(&h, "Size"));
    let (lo, hi) = h.editor().canvas.px_range();
    let before = strokes(&h);
    h.pen_down(at);
    h.wait(1.2);
    assert_eq!(px(&h), start, "a held press changes nothing");
    h.pen_move(at - vec2(0.0, 80.0));
    h.pen_up(at - vec2(0.0, 80.0));
    let want = (start * ((hi / lo).ln() * 80.0 / 160.0).exp()).min(hi);
    assert!((px(&h) - want).abs() / want < 0.001, "the pull landed: {} of {want}", px(&h));
    assert!(h.editor().canvas.stroke_tap.is_empty() && strokes(&h) == before, "no ink");
    assert!(h.state().refusal.is_empty(), "{}", h.state().refusal);
}

#[test]
fn aurail_tiers_are_the_table() {
    use Tier::{Compact, Full, Hidden, Pocket};
    let cases = [
        (492.0, 800.0, Full),
        (491.9, 800.0, Compact),
        (372.0, 800.0, Compact),
        (371.9, 800.0, Pocket),
        (232.0, 800.0, Pocket),
        (231.9, 800.0, Hidden),
        (900.0, 120.0, Full),
        (900.0, 119.9, Hidden),
    ];
    for (reference, wide, tier) in cases {
        assert_eq!(Tier::of(reference, wide), tier, "{reference} over {wide}");
    }
    assert_eq!((Full.total(), Compact.total(), Pocket.total(), Hidden.total()), (476.0, 356.0, 224.0, 0.0));
    // The live rail, by window height: the strip's latch is closed here.
    let mut h = drawn();
    sized(&mut h, 1280.0, 900.0);
    assert_eq!(island(&h).expect("the rail").height(), 476.0);
    assert!(h.has("Brush") && points(&h, "Size").size() == vec2(48.0, 84.0));
    sized(&mut h, 1280.0, 600.0);
    assert_eq!(island(&h).expect("the rail").height(), 356.0);
    assert_eq!(points(&h, "Size").size(), vec2(36.0, 60.0));
    assert_eq!(points(&h, "Brush").size(), vec2(48.0, 36.0));
    assert_eq!(rail_controls(&h).len(), 9, "the same nine");
    sized(&mut h, 1280.0, 420.0);
    assert_eq!(island(&h).expect("the rail").height(), 224.0);
    assert_eq!(rail_controls(&h).len(), 6);
    for held in ["Hand", "Size", "Layers", "Flip"] {
        assert!(!h.has(held), "{held} waits behind the dots");
    }
    h.click("More tools");
    for held in ["Hand", "Size", "Layers", "Flip"] {
        assert!(h.has(held), "{held} is one tap away");
    }
    h.click("More tools");
    sized(&mut h, 1280.0, 300.0);
    assert!(island(&h).is_none(), "no room for a rail");
}

/// The table's total, less the ninth key before the second drawing. Keys, not
/// layout: this is what the rail is meant to come to.
fn rail_total(tier: Tier, keys: usize) -> f32 {
    match (tier, keys) {
        (Tier::Full, 8) => 428.0,
        (Tier::Compact, 8) => 320.0,
        _ => tier.total(),
    }
}

#[test]
fn aurail_rail_fits_every_room() {
    let mut found = Vec::new();
    for doc in [sample::build_sample(), crate::newproject::NewProjectForm::default().new_state()] {
        let mut h = EditorHarness::with_state_shipped(doc);
        for (w, tall) in SIZES {
            for stored in PROBE_SCALES {
                window(&mut h, w, tall, stored);
                for room in PROBE_ROOMS {
                    room.enter(h.editor_mut());
                    settle(&mut h);
                    let at = format!("{w}×{tall} at {stored} in {}", room.name());
                    // Plan's Canvas is a preview, the Brush maker and 3D
                    // switch the rail off.
                    let off = matches!(
                        room,
                        ProbeRoom::Forge | ProbeRoom::Layout3d | ProbeRoom::Stage(Stage::Layout)
                    );
                    let hosted = !off && crate::visible_panes(&h.editor().dock).contains(&Pane::Canvas);
                    let host = h.editor().canvas.dbg_rect;
                    let reference = rail::reference(host.height(), h.editor().strip_closed());
                    let tier = Tier::of(reference, host.width());
                    let Some(rail) = island(&h) else {
                        if hosted && tier != Tier::Hidden {
                            found.push(format!("{at}: no rail in {host:?}"));
                        }
                        continue;
                    };
                    if !hosted {
                        found.push(format!("{at}: a rail where none stands"));
                        continue;
                    }
                    if !host.contains_rect(rail) {
                        found.push(format!("{at}: {rail:?} out of {host:?}"));
                    }
                    let keys = rail_controls(&h);
                    let total = rail_total(tier, keys.len());
                    if (rail.height() - total).abs() > 0.5 {
                        found.push(format!("{at}: {tier:?} is {}, not {total}", rail.height()));
                    }
                    if keys.iter().any(|c| c.height() < 24.0) {
                        found.push(format!("{at}: a key under 24"));
                    }
                    // Every verb in at most two taps.
                    if tier == Tier::Pocket {
                        h.editor_mut().floats.more = true;
                        settle(&mut h);
                        for held in ["Hand", "Size"] {
                            if !h.has(held) {
                                found.push(format!("{at}: {held} is not behind the dots"));
                            }
                        }
                        h.editor_mut().floats.more = false;
                        settle(&mut h);
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "{} findings:\n{}", found.len(), found.join("\n"));
}

#[test]
fn aurail_manga_rail_holds_the_panel_tool() {
    let mut h = drawn();
    ProbeRoom::Stage(Stage::Manga).enter(h.editor_mut());
    settle(&mut h);
    assert!(h.has("Panel tool") && !h.has("Flip"), "slot 9 is Manga's\n{}", h.describe());
    assert_eq!(points(&h, "Panel tool").height(), 48.0);
    h.click("Panel tool");
    assert!(h.editor().canvas.tool == CanvasTool::Panel);
    h.click("Panel tool");
    assert!(is_open(&h, Float::ToolOptions), "its options hold the note");
    assert!(h.has_text_containing("drag on paper"), "{}", h.describe());
}

// ---- the floats ----

#[test]
fn aurail_float_during_a_stroke_waits_for_pen_up() {
    let mut h = blank();
    let c = paper_middle(&h);
    h.press_and_move(c, c + vec2(40.0, 0.0), 4);
    assert!(h.editor().canvas.stroke_active());
    let paper = h.editor().canvas.view_xform();
    h.perform(Action::CanvasOptions);
    settle(&mut h);
    assert!(!is_open(&h, Float::CanvasOptions), "nothing opens under a live pen");
    assert!(float_rect(&h, Float::CanvasOptions).is_none());
    h.release(c + vec2(40.0, 0.0));
    settle(&mut h);
    assert!(is_open(&h, Float::CanvasOptions), "pen-up lets it in");
    assert!(float_rect(&h, Float::CanvasOptions).is_some());
    assert_eq!(h.editor().canvas.view_xform().pan, paper.pan);
}

#[test]
fn aurail_float_never_moves_art() {
    for (w, tall) in SIZES {
        let mut h = EditorHarness::as_shipped();
        window(&mut h, w, tall, 1.0);
        let where_it_is = |h: &EditorHarness| {
            let c = &h.editor().canvas;
            (c.dbg_rect, c.view_xform().zoom, c.view_xform().pan, c.view_xform().paper_bounds())
        };
        let at_rest = where_it_is(&h);
        for f in Float::ALL {
            h.open_float(f);
            settle(&mut h);
            assert!(float_rect(&h, f).is_some(), "{f:?} at {w}×{tall}");
            assert_eq!(where_it_is(&h), at_rest, "{f:?} at {w}×{tall}");
            close(&mut h, f);
            assert_eq!(where_it_is(&h), at_rest, "closing {f:?} at {w}×{tall}");
        }
    }
}

#[test]
fn aurail_only_one_float_per_side() {
    let mut h = blank();
    h.open_float(Float::Brushes);
    h.open_float(Float::Color);
    assert!(is_open(&h, Float::Color) && !is_open(&h, Float::Brushes), "the left has one");
    assert!(float_rect(&h, Float::Brushes).is_none());
    h.open_float(Float::Layers);
    assert!(is_open(&h, Float::Color) && is_open(&h, Float::Layers), "a side each");
    h.open_float(Float::ToolOptions);
    assert!(is_open(&h, Float::ToolOptions) && !is_open(&h, Float::Color) && is_open(&h, Float::Layers));
    let (left, right) = (float_rect(&h, Float::ToolOptions).unwrap(), float_rect(&h, Float::Layers).unwrap());
    let host = h.editor().canvas.dbg_rect;
    assert_eq!(left.left(), host.left() + 76.0, "beside the rail");
    assert_eq!(right.right(), host.right() - 12.0, "the right edge");
    assert!(left.right() < right.left());
}

#[test]
fn aurail_every_float_fits_and_scrolls() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    let mut found = Vec::new();
    for (w, tall) in SIZES {
        for stored in PROBE_SCALES {
            window(&mut h, w, tall, stored);
            for f in Float::ALL {
                h.open_float(f);
                settle(&mut h);
                let at = format!("{f:?} at {w}×{tall}, {stored}");
                let host = h.editor().canvas.dbg_rect;
                let rect = float_rect(&h, f).unwrap_or_else(|| panic!("{at}: not shown"));
                if !host.contains_rect(rect) {
                    found.push(format!("{at}: {rect:?} out of {host:?}"));
                }
                if rect.width() > f.size_in(host).x + 0.5 || rect.height() > f.size_in(host).y + 0.5 {
                    found.push(format!("{at}: {:?} over {:?}", rect.size(), f.size_in(host)));
                }
                // A short body's viewport stands past the float's foot: egui
                // lays a scroll out at 64 before it shrinks to its content.
                let views = plate::viewports(&h.ctx());
                if !views.iter().any(|v| rect.contains(v.rect.min) && v.axes == [false, true]) {
                    found.push(format!("{at}: nothing scrolls in {rect:?}"));
                }
                let clip = measure::clip_here(&h, Scope::Chrome);
                found.extend(clip.found.into_iter().map(|c| format!("{at}: {c}")));
                close(&mut h, f);
            }
        }
    }
    assert!(found.is_empty(), "{} findings:\n{}", found.len(), found.join("\n"));
}

/// A body longer than its float fills the spec's `min`, in both directions:
/// an `Area` left to itself keeps the size it had, and starts from egui's 400.
#[test]
fn aurail_a_long_float_stands_its_whole_height() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    window(&mut h, 1600.0, 900.0, 1.0);
    h.open_float(Float::Brushes);
    settle(&mut h);
    assert_eq!(float_rect(&h, Float::Brushes).expect("shown").height(), 420.0, "the table");
    close(&mut h, Float::Brushes);

    window(&mut h, 1280.0, 640.0, 1.0);
    h.open_float(Float::CanvasOptions);
    settle(&mut h);
    let capped = h.editor().canvas.dbg_rect.height() - 24.0;
    let tall = float_rect(&h, Float::CanvasOptions).expect("shown").height();
    assert_eq!(tall, capped, "`auto`, past the host's cap");

    window(&mut h, 1600.0, 900.0, 1.0);
    let grown = float_rect(&h, Float::CanvasOptions).expect("shown").height();
    assert!(grown > tall, "more host, more float: {grown} against {tall}");
}

/// The handle stays in sight, so it has a gutter: it lies on no control.
#[test]
fn aurail_a_float_handle_lies_on_no_control() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    window(&mut h, 1280.0, 800.0, 1.0);
    h.open_float(Float::Brushes);
    settle(&mut h);
    let float = float_rect(&h, Float::Brushes).expect("shown");
    let views = plate::viewports(&h.ctx());
    let view = views.iter().find(|v| float.contains(v.rect.min)).expect("its scroll").rect;
    // The handle's left edge, dormant, and the bar that is wider under a hover.
    let handle = float.right() - 12.0 - 2.0;
    let bar = float.right() - 12.0 - 10.0;
    let layer = LayerId::new(Order::Middle, Id::new(("float", Float::Brushes.heading())));
    let rows: Vec<Rect> = h.ctx().viewport(|v| {
        let rows = v.prev_pass.widgets.get_layer(layer).filter(|r| r.sense.interactive());
        rows.map(|r| r.rect).collect()
    });
    let body = rows.iter().filter(|r| view.contains(r.min) && r.left() < bar);
    let over: Vec<&Rect> = body.filter(|r| r.width() < float.width() && r.right() > handle).collect();
    assert!(over.is_empty(), "under the handle at {handle}: {over:?}");
}

/// The body rides with a pen: nothing else scrolls a float under one.
#[test]
fn aurail_a_pen_drag_scrolls_a_long_float() {
    let mut h = EditorHarness::as_shipped();
    window(&mut h, 1024.0, 640.0, 1.5);
    h.open_float(Float::Brushes);
    settle(&mut h);
    // Off a tile, as a hand would: the press takes no tile with it.
    let mark = tile(&h, "shusei");
    let (from, was) = (centre(&h, &mark), points(&h, &mark).top());
    let armed = h.editor().canvas.arming_pencil();
    h.pen_down(from);
    h.pen_move(from - vec2(0.0, 12.0));
    h.pen_move(from - vec2(0.0, 24.0));
    h.pen_up(from - vec2(0.0, 24.0));
    settle(&mut h);
    assert!(points(&h, &mark).top() < was, "the body rode up with the pen");
    assert_eq!(h.editor().canvas.arming_pencil(), armed, "the tile stayed unpressed");
}

#[test]
fn aurail_overlay_takes_the_host_rect() {
    struct Bed {
        canvas: CanvasView,
        state: AppState,
        floats: Floats,
        presets: Vec<BrushPreset>,
    }
    let host = Rect::from_min_size(pos2(300.0, 150.0), vec2(620.0, 520.0));
    let bed = Bed {
        canvas: CanvasView::new(),
        state: AppState::new_project("host", 1920, 1080, 24, 300.0),
        floats: Floats::default(),
        presets: config::Config::default().presets,
    };
    let mut p = PanelHarness::new(vec2(1280.0, 800.0), bed, move |ui, bed| {
        let args = &mut OverlayArgs {
            canvas: &mut bed.canvas,
            state: &mut bed.state,
            presets: &bed.presets,
            floats: &mut bed.floats,
            raster_available: false,
            manga: false,
            rail_off: false,
            strip_closed: false,
            layers_docked: false,
            pane_rects: None,
        };
        rail::overlay(ui, args, host);
    });
    p.frames(3);
    let ctx = p.ctx();
    let rect_of = |id: Id| ctx.memory(|m| m.area_rect(id));
    let rail = rect_of(Id::new("rail")).expect("the rail");
    assert!(host.contains_rect(rail), "{rail:?}");
    assert_eq!(rail.left(), host.left() + 12.0);
    for f in Float::ALL {
        p.state_mut().floats = Floats::default();
        p.state_mut().floats.open(f, false);
        p.frames(3);
        let rect = rect_of(Id::new(("float", f.heading()))).expect("a float");
        assert!(host.contains_rect(rect), "{f:?}: {rect:?} out of {host:?}");
    }
}

// ---- the hover card and the chords ----

#[test]
fn aurail_hover_card_passes_the_pen() {
    let mut h = blank();
    // One frame under the pointer: `hover` runs past the delay.
    let at = centre(&h, "Eraser");
    h.kit_mut().input_mut().events.push(egui::Event::PointerMoved(at));
    h.kit_mut().step();
    assert!(card(&h).is_none(), "not at once");
    h.wait(plate::CARD_DELAY + 0.05);
    h.run();
    let rect = card(&h).unwrap_or_else(|| panic!("no card\n{}", h.describe()));
    let binds = h.rig().config.keybinds.clone();
    let chord = crate::cmdpal::chord_of(&binds, Action::ToggleEraser).expect("bound");
    let line = menus::tip_with(Action::ToggleEraser, Some(chord));
    assert!(h.has_text_containing(&line), "the card is the tip's line: {line:?}\n{}", h.describe());
    let middle = rect.center();
    let canvas = h.editor().canvas.dbg_layer;
    assert_eq!(h.ctx().layer_id_at(middle), Some(canvas), "the pen sees paper through it");
    let before = strokes(&h);
    h.pen_down(middle);
    h.pen_move(middle + vec2(20.0, 0.0));
    h.pen_up(middle + vec2(20.0, 0.0));
    assert_eq!(strokes(&h), before + 1, "and draws there");
    // A float below it still blocks.
    h.open_float(Float::Brushes);
    h.hover("Eraser");
    h.wait(plate::CARD_DELAY + 0.05);
    h.run();
    let over = card(&h).expect("a card").intersect(float_rect(&h, Float::Brushes).expect("a float"));
    assert!(over.is_positive(), "the card stands over the float");
    h.editor_mut().canvas.stroke_tap.clear();
    h.pen_tap(over.center());
    assert!(h.editor().canvas.stroke_tap.is_empty(), "the float took the pen");
}

#[test]
fn aurail_chord_over_a_float_goes_to_the_float() {
    let mut h = drawn();
    h.open_float(Float::Brushes);
    h.open_float(Float::Layers);
    settle(&mut h);
    let ctx = h.ctx();
    let layers = float_rect(&h, Float::Layers).expect("a float").center();
    let brushes = float_rect(&h, Float::Brushes).expect("a float").center();
    let rail = island(&h).expect("the rail").center();
    let under = |h: &mut EditorHarness, p: Pos2| {
        h.point_at(p);
        h.editor_mut().shortcut_ctx(&ctx)
    };
    assert_eq!(under(&mut h, layers), config::Ctx::Global, "Layers answers for itself");
    assert_eq!(under(&mut h, brushes), config::Ctx::Canvas, "the tools' floats speak for the canvas");
    assert_eq!(under(&mut h, rail), config::Ctx::Canvas);
    let paper = pos2(layers.x, h.editor().canvas.dbg_rect.bottom() - 8.0);
    assert_eq!(under(&mut h, paper), config::Ctx::Canvas, "beneath the float it is paper again");
}

#[test]
fn aurail_stray_touch_does_not_set_seen_pen() {
    let mut h = blank();
    let c = paper_middle(&h);
    let path = [c, c + vec2(30.0, 0.0), c + vec2(60.0, 10.0)];
    // A finger on the rail: no pen has met the canvas.
    let (hand, eraser, brush) = (centre(&h, "Hand"), centre(&h, "Eraser"), centre(&h, "Brush"));
    h.pen_tap(hand);
    h.pen_tap(hand);
    h.pen_tap(eraser);
    h.pen_tap(brush);
    assert!(!h.editor().canvas.erasing && !h.editor().canvas.hand_held);
    settle(&mut h);
    let before = strokes(&h);
    h.drag_path(&path);
    assert_eq!(strokes(&h), before + 1, "the mouse still draws");
    // A pen on the canvas does latch: the mouse is its echo from here on.
    h.pen_down(c);
    h.pen_move(c + vec2(20.0, 0.0));
    h.pen_up(c + vec2(20.0, 0.0));
    assert_eq!(strokes(&h), before + 2, "the pen drew");
    h.run_frames(4);
    h.drag_path(&path);
    assert_eq!(strokes(&h), before + 2, "the latch stands");
}

// ---- the deck's homes ----

/// One step of a pen-only path.
#[derive(Clone, Copy)]
enum Tap {
    /// The control that speaks this name.
    On(&'static str),
    /// A pencil's tile: its name carries its digit.
    Pencil(&'static str),
    /// A drag up by this much.
    Pull(&'static str, f32),
}

/// A row of spec 4.5's table: what the deck held, where it lives now, the
/// taps that reach it from a fresh boot and the state they set.
struct DeckRow {
    today: &'static str,
    home: &'static str,
    /// Armed first, so a key's first tap arms and its second opens.
    from: CanvasTool,
    room: Stage,
    path: &'static [Tap],
    read: fn(&EditorHarness) -> bool,
}

/// Spec 4.5, frozen. Every control of the retired surfaces has a row.
const TODAY_DECK: [DeckRow; 15] = {
    use CanvasTool::{Fill, Paint, Select};
    use Stage::{Drawing, Manga};
    use Tap::{On, Pencil, Pull};
    [
        DeckRow {
            today: "Pencil slots",
            home: "Brushes tiles",
            from: Select,
            room: Drawing,
            path: &[On("Brush"), On("Brush"), Pencil("shusei")],
            read: |h| h.editor().canvas.arming_pencil() == "shusei",
        },
        DeckRow {
            today: "Eraser slot",
            home: "Rail",
            from: Paint,
            room: Drawing,
            path: &[On("Eraser")],
            read: |h| h.editor().canvas.erasing,
        },
        DeckRow {
            today: "Select, shape buttons",
            home: "Rail, Tool options",
            from: Paint,
            room: Drawing,
            path: &[On("Select"), On("Select"), On("rect selection")],
            read: |h| h.editor().canvas.deck_reads().sel_shape == SelShape::Rect,
        },
        DeckRow {
            today: "Fill, gap, under, cel or layer",
            home: "Rail, Tool options",
            from: Paint,
            room: Drawing,
            path: &[On("Fill"), On("Fill"), On("layer")],
            read: |h| !h.editor().canvas.deck_reads().fill_ref_cel,
        },
        DeckRow {
            today: "Lasso fill, softness, grow",
            home: "Fill chooser in Tool options",
            from: Paint,
            room: Drawing,
            path: &[On("Fill"), On("Fill"), On(vocab::DRAW_TO_FILL)],
            read: |h| h.editor().canvas.tool == CanvasTool::LassoFill,
        },
        DeckRow {
            today: "Alpha lock latch",
            home: "Keep inside strokes, Brushes and Tool options",
            from: Paint,
            room: Drawing,
            path: &[On("Eraser"), On("Eraser"), On(vocab::KEEP_INSIDE_STROKES)],
            read: |h| h.editor().canvas.deck_reads().alpha_lock,
        },
        DeckRow {
            today: "Comp latch",
            home: "View ▸ Preview final",
            from: Paint,
            room: Drawing,
            path: &[On("view"), On("Composite view")],
            read: |h| h.editor().canvas.composite_view,
        },
        DeckRow {
            today: "Layer chip",
            home: "Layers float",
            from: Paint,
            room: Drawing,
            path: &[On("Layers"), On("color")],
            read: |h| h.state().view.active_layer == "color",
        },
        DeckRow {
            today: "Vector width",
            home: "Rail Size",
            from: Paint,
            room: Drawing,
            path: &[Pull("Size", 40.0)],
            // The harness has no GPU, so this is the vector width, 3 at boot.
            read: |h| h.editor().canvas.active_px() > 3.0,
        },
        DeckRow {
            today: "Color button, swatches",
            home: "Color float",
            from: Paint,
            room: Drawing,
            path: &[On("Brush color"), On("colour well: aka — correction")],
            read: |h| h.editor().canvas.brush_color == SWATCHES[2],
        },
        DeckRow {
            today: "Panel tool and note",
            home: "Manga's rail slot 9, note in Tool options",
            from: Paint,
            room: Manga,
            path: &[On("Panel tool")],
            read: |h| h.editor().canvas.tool == CanvasTool::Panel,
        },
        DeckRow {
            today: "Hardness, aliased, stabilizer",
            home: "Brushes",
            from: Select,
            room: Drawing,
            path: &[On("Brush"), On("Brush"), On("More"), On(vocab::PIXEL_EDGES)],
            read: |h| h.editor().canvas.brush_aliased,
        },
        DeckRow {
            today: "brush_rail_ui",
            home: "Brushes float",
            from: Select,
            room: Drawing,
            path: &[On("Brush"), On("Brush"), On("More"), On("p·size")],
            read: |h| !h.editor().canvas.deck_reads().dyn_size,
        },
        DeckRow {
            today: "lightbox_rail_ui",
            home: "Canvas options float",
            from: Paint,
            room: Drawing,
            path: &[On("view"), On("Canvas options…"), On("miss check")],
            read: |h| h.editor().canvas.miss_check,
        },
        DeckRow {
            today: "dish_ui",
            home: "Color float",
            from: Fill,
            room: Drawing,
            path: &[On("Brush color")],
            read: |h| h.editor().floats.is_open(Float::Color),
        },
    ]
};

fn walk(h: &mut EditorHarness, path: &[Tap]) {
    for tap in path {
        match *tap {
            Tap::On(name) => {
                h.scroll_to(name);
                let at = centre(h, name);
                h.pen_tap(at);
            }
            Tap::Pencil(pencil) => {
                let at = centre(h, &tile(h, pencil));
                h.pen_tap(at);
            }
            Tap::Pull(name, up) => {
                let at = centre(h, name);
                h.pen_down(at);
                h.pen_move(at - vec2(0.0, up / 2.0));
                h.pen_move(at - vec2(0.0, up));
                h.pen_up(at - vec2(0.0, up));
            }
        }
        settle(h);
    }
}

#[test]
fn aurail_every_deck_verb_has_a_home() {
    for row in &TODAY_DECK {
        let mut h = drawn();
        if row.room != Stage::Drawing {
            ProbeRoom::Stage(row.room).enter(h.editor_mut());
        }
        let editor = h.editor_mut();
        editor.canvas.set_tool(row.from, &mut editor.state);
        settle(&mut h);
        assert!(!(row.read)(&h), "{}: already so, so the walk would prove nothing", row.today);
        walk(&mut h, row.path);
        let tree = h.describe();
        assert!((row.read)(&h), "{} → {}: not reached by a pen\n{tree}", row.today, row.home);
    }
}

#[test]
fn aurail_fill_chooser_reaches_draw_to_fill() {
    let mut h = blank();
    h.click("Fill");
    h.click("Fill");
    assert!(is_open(&h, Float::ToolOptions));
    h.click(vocab::DRAW_TO_FILL);
    assert!(h.editor().canvas.tool == CanvasTool::LassoFill);
    assert_eq!(h.node_toggled("Fill"), Some(true), "the rail's Fill stays lit");
    assert!(h.has_text_containing("softness") && h.has_text_containing("grow"), "its rows below");
    h.click("Fill a region");
    assert!(h.editor().canvas.tool == CanvasTool::Fill);
    assert!(h.has_text_containing("gap") && h.has_text_containing("under"), "and back");
    // The rail's key goes on opening the same chooser from either.
    h.click(vocab::DRAW_TO_FILL);
    h.click(&format!("Close {}", vocab::TOOL_OPTIONS));
    h.click("Fill");
    assert!(is_open(&h, Float::ToolOptions) && h.editor().canvas.tool == CanvasTool::LassoFill);
}

#[test]
fn aurail_keep_inside_strokes_serves_brush_and_eraser() {
    let lock = |h: &EditorHarness| h.editor().canvas.deck_reads().alpha_lock;
    let mut h = blank();
    h.open_float(Float::Brushes);
    h.scroll_to(vocab::KEEP_INSIDE_STROKES);
    h.click(vocab::KEEP_INSIDE_STROKES);
    assert!(lock(&h), "from Brushes");
    h.click("Eraser");
    h.click("Eraser");
    assert!(is_open(&h, Float::ToolOptions));
    assert_eq!(h.node_toggled(vocab::KEEP_INSIDE_STROKES), Some(true), "one function, one state");
    h.click(vocab::KEEP_INSIDE_STROKES);
    assert!(!lock(&h), "from Tool options, under the eraser");
    let canvas = include_str!("canvas.rs");
    assert_eq!(canvas.matches("self.keep_inside_row(ui);").count(), 2, "one row, two homes");
}

// ---- the rules ----

#[test]
fn aurail_both_canvas_hosts_call_one_overlay() {
    let main = include_str!("main.rs");
    assert_eq!(main.matches("rail::overlay(ui, args, host);").count(), 2, "the pane and the OS window");
    assert_eq!(main.matches("pane_rects: None,").count(), 1, "the OS window pushes none");
    assert_eq!(main.matches("pane_rects: Some(self.pane_rects),").count(), 1);
    let canvas = include_str!("canvas.rs");
    for gone in ["fn canvas_tool_deck", "fn brush_ui", "rail_open:", "light_open:", "dish_open:"] {
        assert!(!canvas.contains(gone), "{gone} is retired");
    }
    assert!(!canvas.contains("rail::overlay"), "the host calls it, after `canvas.ui`");
}

/// The floors of 4.1, the band at 92, and the paper clear of the chrome at
/// Fit. 4.2's accepted case is wider than any aspect here.
#[test]
fn aurail_paper_share_holds_at_every_size_and_aspect() {
    for (at, (w, tall)) in SIZES.into_iter().enumerate() {
        let mut h = EditorHarness::as_shipped();
        h.editor_mut().strip_open = true;
        window(&mut h, w, tall, 1.0);
        h.perform(Action::ZoomFit);
        settle(&mut h);
        let share = measure::paper_share(&h);
        assert!(share.paper >= PAPER_FLOORS[at], "paper at {w}×{tall}: {share:?}");
        assert!(share.pen_land >= PEN_FLOORS[at], "pen at {w}×{tall}: {share:?}");
    }
    for (pw, ph) in [(1920, 1080), (1600, 1200), (1080, 1080), (1080, 1920)] {
        for (w, tall) in SIZES {
            let mut state = AppState::new_project("aspect", pw, ph, 24, 300.0);
            state.new_drawing_at_frame();
            let mut h = EditorHarness::with_state_shipped(state);
            h.editor_mut().strip_open = true;
            window(&mut h, w, tall, 1.0);
            h.perform(Action::ZoomFit);
            settle(&mut h);
            let share = measure::paper_share(&h);
            assert_eq!(share.paper_under_chrome, 0, "{pw}:{ph} at {w}×{tall}: {share:?}");
        }
    }
}

/// Median ≥32, none under 24, essentials on screen; and the interim tally
/// the part reports, printed for the record.
#[test]
fn aurail_census_of_shipped_draw() {
    let mut h = EditorHarness::as_shipped();
    h.editor_mut().strip_open = true;
    window(&mut h, 1600.0, 900.0, 1.0);
    let c = measure::census(&mut h);
    println!("blank Draw, interim: {} controls, {} words", c.controls, c.words);
    assert!(c.median_control_height() >= 32.0, "median {}", c.median_control_height());
    assert_eq!(c.controls_under(24.0), 0, "{:?}", c.control_heights);
    let screen = h.ctx().content_rect();
    for essential in ["Size", "Undo (toolbar)", "Redo (toolbar)"] {
        assert!(screen.contains_rect(points(&h, essential)), "{essential} is off screen");
    }
}
