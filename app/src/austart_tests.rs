//! AU6: the ready paper and the Start float (spec 8.1). A fresh launch draws
//! at once; the float asks nothing. AU20: it stands dead centre (feedback spec
//! 4), and every point of the paper outside the card still takes a stroke.

use eframe::egui;
use egui::{Id, Key, LayerId, Modifiers, Order, Pos2, Rect, pos2, vec2};

use crate::aurail_tests::{paper_share_sweep, window};
use crate::canvas::extpaper_tests::paint_texels;
use crate::config::{Action, Config, Ctx, Keymap, UiConfig};
use crate::floats::{self, Float};
use crate::measure::{self, Census, Reading, Row, Scope};
use crate::testkit::{Boot, CTRL, EditorHarness};
use crate::welcome::{self, HEADING, LESSONS_HEADING, PenCheck, Welcome, WelcomeAction, WelcomeTab};
use crate::workspace::Stage;
use crate::{
    AppRequest, Leave, PROBE_SCALES, Pane, Performed, Startup, lessons, plate, sample, startup_route, templates,
};

const SIZES: [(f32, f32); 4] = [(1600.0, 900.0), (1280.0, 800.0), (1280.0, 720.0), (1024.0, 640.0)];

const RECENT: [&str; 2] = ["Ball bounce", "Walk cycle"];

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

fn sized(harness: &mut EditorHarness, w: f32, tall: f32) {
    harness.kit_mut().set_size(vec2(w, tall));
    harness.kit_mut().set_pixels_per_point(1.0);
    settle(harness);
}

/// A first launch at 1600×900.
fn first_run() -> EditorHarness {
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1600.0, 900.0);
    h
}

fn is_open(h: &EditorHarness) -> bool {
    h.editor().start.is_some()
}

fn start_id() -> Id {
    Id::new(("float", HEADING))
}

fn float_rect(h: &EditorHarness) -> Option<Rect> {
    let id = start_id();
    let shown = |m: &egui::Memory| m.areas().visible_layer_ids().iter().any(|l| l.id == id);
    h.ctx().memory(|m| shown(m).then(|| m.area_rect(id)).flatten())
}

fn float_open(h: &EditorHarness) -> Rect {
    float_rect(h).expect("the Start float on screen")
}

fn paper_middle(h: &EditorHarness) -> Pos2 {
    let xf = h.editor().canvas.view_xform();
    xf.paper_to_screen(pos2(xf.paper.x / 2.0, xf.paper.y / 2.0))
}

fn strokes(h: &EditorHarness) -> usize {
    h.state().cut().drawings.iter().map(|d| d.strokes.len()).sum()
}

/// Paper beside the card, with room for `line_at` to its left: the card stands
/// on the paper's centre.
fn free_paper(h: &EditorHarness) -> Pos2 {
    let card = float_open(h);
    pos2(card.left() - 4.0, card.center().y)
}

/// The dock rect, which the card centres on.
fn dock(h: &EditorHarness) -> Rect {
    plate::dock_rect(&h.ctx()).expect("the dock was drawn")
}

/// A line from `from`, leftward, away from the float.
fn line_at(h: &mut EditorHarness, from: Pos2) {
    h.drag_path(&[from, from - vec2(30.0, 0.0), from - vec2(60.0, 10.0)]);
    settle(h);
}

/// Two files named as a person names them, on the float's recent list.
fn recents(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let project = crate::sample::build_sample().engine.project;
    let path = |name: &&str| dir.join(format!("{name}.animproj"));
    let files: Vec<_> = RECENT.iter().map(path).collect();
    for file in &files {
        anim_core::store::save(&project, file).expect("saved");
    }
    files
}

/// The float at its worst case: two recents.
fn with_recents(h: &mut EditorHarness, dir: &std::path::Path) {
    let list = recents(dir);
    h.editor_mut().start = Some(Welcome::new(&list));
    settle(h);
}

/// The float alone, counted the way the census counts a screen.
fn float_census(h: &mut EditorHarness) -> Census {
    settle(h);
    let (ctx, rect) = (h.ctx(), float_open(h));
    let layer = LayerId::new(Order::Middle, start_id());
    let reading = Reading::of_float(&ctx, &h.kit().output().shapes, layer, rect);
    reading.census(measure::TERMS.len())
}

/// The float as built, on the same count: egui paints nothing wholly outside a
/// clip, so `float_census` misses a row below the fold, and a row it misses
/// cannot break a budget. The accessibility tree has every row. The close
/// key's name is the one word it holds that no eye reads: that paints as `×`.
fn built_census(h: &mut EditorHarness) -> Census {
    use egui::accesskit::Role;
    use egui_kittest::kittest::{NodeT, Queryable};
    settle(h);
    let float = start_id().accesskit_id();
    let mut reading = Reading { area: h.ctx().content_rect().area(), ..Reading::default() };
    for role in [Role::Button, Role::CheckBox, Role::Label] {
        for node in h.kit().query_all_by_role(role) {
            let tree = node.accesskit_node();
            if !std::iter::successors(Some(tree), |n| n.parent()).any(|n| n.locate().0 == float) {
                continue;
            }
            let words = tree.label().or_else(|| tree.value()).unwrap_or_default();
            let at = node.rect();
            if role != Role::Label {
                reading.rows.push(Row { w: at.width(), h: at.height(), focus: true, enabled: true });
            }
            if !words.starts_with("Close ") {
                reading.texts.push((at.height(), words));
            }
        }
    }
    reading.census(measure::TERMS.len())
}

// ---- the ready paper ----

#[test]
fn austart_fresh_launch_opens_on_ready_paper() {
    let h = first_run();
    assert_eq!(h.boot(), Boot::FirstRun);
    // A blank Draw step: a drawing, 1280×720, 12 fps, one track `A`.
    let project = &h.state().engine.project;
    assert_eq!((project.width, project.height), (1280, 720));
    assert_eq!(h.state().fps(), 12);
    assert_eq!(h.state().frame_count(), 1);
    let columns = &h.state().cut().xsheet.columns;
    assert_eq!(columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["A"]);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert!(!h.editor().strip_open, "the ready paper's strip is closed");
    // The float sits over it, on its Start tab, and the paper is live.
    assert!(is_open(&h) && h.has("Close Start"));
    assert_eq!(h.editor().start.as_ref().map(|w| w.tab), Some(WelcomeTab::Start));
    assert!(float_rect(&h).is_some());
    assert_eq!(startup_route(false, false, None, &Config::default()), Startup::Welcome);
    // Zero clicks to the first line.
    let at = free_paper(&h);
    let mut h = h;
    line_at(&mut h, at);
    assert_eq!(h.state().strokes_done, 1);
}

/// The shipped boot is that launch with the float closed (spec 1): the same
/// blank Draw, measured on the same paper.
#[test]
fn austart_shipped_boot_is_the_ready_paper_with_the_float_closed() {
    let stands = |h: &EditorHarness| {
        let p = &h.state().engine.project;
        (p.width, p.height, h.state().fps(), h.state().frame_count(), h.editor().stage, h.editor().strip_open)
    };
    let mut shipped = EditorHarness::as_shipped();
    sized(&mut shipped, 1600.0, 900.0);
    assert_eq!(shipped.boot(), Boot::Shipped);
    assert!(!is_open(&shipped), "the float is closed");
    let mut first = first_run();
    let word = stands(&first);
    assert_eq!(stands(&shipped), word);
    first.click("Close Start");
    settle(&mut first);
    assert_eq!(first.editor().canvas.dbg_rect, shipped.editor().canvas.dbg_rect, "the same pane");
    assert_eq!(stands(&first), word, "closing the float moves nothing");
}

#[test]
fn austart_first_stroke_closes_the_float_and_keeps_the_stroke() {
    let mut h = first_run();
    let group = h.state().untitled_group.clone();
    let at = free_paper(&h);
    line_at(&mut h, at);
    assert!(!is_open(&h), "the first finished stroke ends it");
    assert!(float_rect(&h).is_none(), "and nothing of it is left on screen");
    assert_eq!(strokes(&h), 1, "the stroke stays");
    assert_eq!(h.state().untitled_group, group, "on the same paper: a drawing accepted");
    assert!(!h.editor().strip_open);
    // The float is gone for good: the next line finds nothing to close.
    let again = paper_middle(&h) + vec2(0.0, 60.0);
    line_at(&mut h, again);
    assert_eq!(strokes(&h), 2);
}

#[test]
fn austart_raster_stroke_closes_the_float() {
    let mut h = first_run();
    assert!(is_open(&h));
    // The CPU route's landing: what a raster stroke commits at pen-up.
    paint_texels(h.state_mut(), &[(10, 10), (11, 10), (12, 10)], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(h.state().strokes_done, 1);
    settle(&mut h);
    assert!(!is_open(&h), "a raster stroke ends the float too");
    assert_eq!(h.state().strokes_done, 1, "and stays");
    assert!(h.state().active_layer_tiles().is_some_and(|tiles| !tiles.is_empty()));
}

/// Every point of the paper outside the card takes a first stroke: on each of
/// its four sides, at every size, and the stroke stays.
#[test]
fn austart_stroke_on_free_paper_with_the_float_open() {
    for (w, tall) in SIZES {
        for stored in PROBE_SCALES {
            let sides = [vec2(-4.0, 0.0), vec2(4.0, 0.0), vec2(0.0, -4.0), vec2(0.0, 4.0)];
            for side in sides {
                let mut h = EditorHarness::as_first_run();
                window(&mut h, w, tall, stored);
                let float = float_open(&h);
                let at = float.center() + side.normalized() * (float.size() / 2.0) + side;
                let size = format!("{w}×{tall} at {stored}, {side:?}");
                assert!(!float.contains(at), "{size}: {at:?} {float:?}");
                assert!(h.editor().canvas.dbg_rect.contains(at), "{size}: on the canvas");
                // Along the card's edge, so all of it is on free paper.
                let along = if side.x == 0.0 { vec2(60.0, 0.0) } else { vec2(0.0, 40.0) };
                h.drag_path(&[at, at + along / 2.0, at + along]);
                settle(&mut h);
                assert_eq!(strokes(&h), 1, "{size}");
                assert!(!is_open(&h), "{size}");
            }
        }
    }
}

#[test]
fn austart_largest_float_stands_centred() {
    for ((w, tall), stored) in SIZES.into_iter().flat_map(|size| PROBE_SCALES.map(|stored| (size, stored))) {
        let dir = tempfile::tempdir().unwrap();
        let mut h = EditorHarness::as_first_run();
        window(&mut h, w, tall, stored);
        with_recents(&mut h, dir.path());
        for what in ["recents", "More sizes", "What's new"] {
            match what {
                "More sizes" => h.click("More sizes"),
                "What's new" => h.editor_mut().start.as_mut().expect("open").tab = WelcomeTab::WhatsNew,
                _ => {}
            }
            settle(&mut h);
            let float = float_open(&h);
            let pane = dock(&h);
            let at = format!("{what} at {w}×{tall}, {stored}: {float:?} in {pane:?}");
            assert!((float.center().x - pane.center().x).abs() < 1.0, "centred across: {at}");
            assert!((float.center().y - pane.center().y).abs() < 1.0, "centred down: {at}");
            assert!(pane.contains_rect(float), "inside the dock: {at}");
            assert!(float.left() >= 76.0, "clear of the rail: {at}");
            assert!(float.height() <= 300.5, "{at}");
        }
    }
    // 8.1's number: what the paper keeps with the float at its largest.
    let dir = tempfile::tempdir().unwrap();
    let mut h = first_run();
    with_recents(&mut h, dir.path());
    h.click("More sizes");
    settle(&mut h);
    let share = measure::paper_share(&h);
    println!("paper uncovered at 1600×900, the float at its largest: {share:?}");
    assert!(share.paper >= 58.0, "paper uncovered at 1600×900: {share:?}");
}

#[test]
fn austart_float_geometry_is_the_rule() {
    let pane = |w: f32, tall: f32| Rect::from_min_size(pos2(100.0, 50.0), vec2(w, tall));
    let (pos, size) = floats::start_place(pane(1500.0, 800.0));
    assert_eq!(size, vec2(600.0, 300.0));
    assert_eq!(pos, pos2(100.0 + 750.0 - 300.0, 50.0 + 400.0 - 150.0), "dead centre");
    assert_eq!(floats::start_place(pane(752.0, 800.0)).1.x, 600.0, "the width's cap");
    assert_eq!(floats::start_place(pane(652.0, 800.0)).1.x, 500.0, "76 clear at each side");
    assert_eq!(floats::start_place(pane(300.0, 800.0)).1.x, 240.0, "the floor");
    assert_eq!(floats::start_place(pane(1000.0, 300.0)).1.y, 276.0, "the pane less 24");
}

/// Spec 4.2, whole: window, stored scale, the card's left, and its top over a
/// band of 36 and of 92. `1024×640, 125 or 150` is one row there, and the
/// last row here is the one off the table.
const PLACES: [(f32, f32, f32, f32, f32, f32); 13] = [
    (1600.0, 900.0, 1.0, 500.0, 304.0, 276.0),
    (1600.0, 900.0, 1.25, 340.0, 214.0, 186.0),
    (1600.0, 900.0, 1.5, 233.0, 154.0, 126.0),
    (1280.0, 800.0, 1.0, 340.0, 254.0, 226.0),
    (1280.0, 800.0, 1.25, 212.0, 174.0, 146.0),
    (1280.0, 800.0, 1.5, 126.0, 120.0, 92.0),
    (1280.0, 720.0, 1.0, 340.0, 214.0, 186.0),
    (1280.0, 720.0, 1.25, 212.0, 142.0, 114.0),
    (1280.0, 720.0, 1.5, 162.0, 114.0, 86.0),
    (1024.0, 640.0, 1.0, 212.0, 174.0, 146.0),
    (1024.0, 640.0, 1.25, 116.0, 114.0, 86.0),
    (1024.0, 640.0, 1.5, 116.0, 114.0, 86.0),
    (1024.0, 768.0, 2.0, 80.0, 139.0, 111.0),
];

/// Start at its cap: two recents and `More sizes` overflow any card.
fn at_the_cap(h: &mut EditorHarness, dir: &std::path::Path) {
    let mut w = Welcome::new(&recents(dir));
    w.more_open = true;
    h.editor_mut().open_start(w);
    settle(h);
}

#[test]
fn austart_stands_centred() {
    for (w, tall, stored, left, top, top_strip) in PLACES {
        let dir = tempfile::tempdir().unwrap();
        // Ready paper, band 36; an animation with its sheet hidden, band 92.
        let mut ready = EditorHarness::as_first_run();
        let mut strip = EditorHarness::as_animation();
        strip.editor_mut().plan_sheet_hidden = true;
        strip.editor_mut().dock = Stage::Drawing.dock();
        strip.editor_mut().strip_open = true;
        for (h, want_top) in [(&mut ready, top), (&mut strip, top_strip)] {
            window(h, w, tall, stored);
            at_the_cap(h, dir.path());
            let (card, pane) = (float_open(h), dock(h));
            let at = format!("{w}×{tall} at {stored}: {card:?} in {pane:?}");
            assert!((card.width() - 600.0).abs() < 0.5 && (card.height() - 300.0).abs() < 0.5, "{at}");
            assert!((card.center() - pane.center()).length() <= 1.0, "within 1 pt: {at}");
            assert!((card.left() - left).abs() <= 1.0 && (card.top() - want_top).abs() <= 1.0, "the table: {at}");
            assert!(pane.contains_rect(card), "inside the dock: {at}");
            assert!(card.left() >= if stored == 2.0 { 79.5 } else { 115.5 }, "{at}");
        }
        // A fresh card stands at the same size, so nothing it opens moves it.
        ready.editor_mut().start = Some(Welcome::new(&[]));
        settle(&mut ready);
        let (card, pane) = (float_open(&ready), dock(&ready));
        assert!((card.height() - 300.0).abs() < 0.5, "the fresh card fills to the cap: {card:?}");
        assert!((card.center() - pane.center()).length() <= 1.0, "{w}×{tall}: {card:?} in {pane:?}");
    }
}

/// A returning animator's two files stand whole, and the way to turn the card
/// off needs no scroll, at every size.
#[test]
fn austart_recents_stand_unscrolled() {
    for (w, tall, stored, ..) in PLACES {
        let dir = tempfile::tempdir().unwrap();
        let mut h = EditorHarness::as_first_run();
        window(&mut h, w, tall, stored);
        with_recents(&mut h, dir.path());
        let (card, ppp) = (float_open(&h), h.ctx().pixels_per_point());
        let rows = RECENT.map(|name| h.rect(name) / ppp);
        let at = format!("{w}×{tall} at {stored}: {rows:?} in {card:?}");
        for row in rows {
            assert!(card.contains_rect(row) && row.bottom() <= card.bottom() - 12.0, "{at}");
        }
        assert!(rows[0].top() == rows[1].top() && rows[0].right() < rows[1].left(), "{at}");
        let half = h.rect("More sizes") / ppp;
        assert_eq!((rows[1].left(), rows[1].width()), (half.left(), half.width()), "{at}");
        let (latch, close) = (h.rect("Show at startup") / ppp, h.rect("Close Start") / ppp);
        assert!(card.contains_rect(latch) && latch.bottom() < (h.rect("A drawing") / ppp).top(), "{at}");
        assert!(latch.right() <= close.left() && (latch.center().y - close.center().y).abs() <= 1.0, "{at}");
    }
}

/// Under the cap a body scrolls, and egui keeps a scroll by its id: opened
/// again, Start stands at its head, with the cards in sight.
#[test]
fn austart_opens_at_its_head() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = first_run();
    with_recents(&mut h, dir.path());
    let head = h.rect("A drawing");
    h.scroll_to("What's new");
    settle(&mut h);
    assert_ne!(h.rect("A drawing"), head, "it did scroll");
    h.click("Close Start");
    settle(&mut h);
    h.editor_mut().open_start(Welcome::new(&recents(dir.path())));
    settle(&mut h);
    assert_eq!(h.rect("A drawing"), head);
}

/// `More sizes` scrolls, and the card stands at one size on every page, so
/// neither moves a row under the pointer.
#[test]
fn austart_more_sizes_keeps_it_centred() {
    let mut h = first_run();
    let (shut, row) = (float_open(&h), h.rect("More sizes"));
    assert_eq!(shut.height(), 300.0, "a fresh card fills to the cap");
    h.click("More sizes");
    settle(&mut h);
    assert_eq!(float_open(&h), shut, "the cap, and the rows scroll under it");
    assert_eq!(h.rect("More sizes"), row, "the row the pointer is on stays under it");
    for tab in [WelcomeTab::WhatsNew, WelcomeTab::Lessons] {
        stand_in(&mut h, tab);
        assert_eq!(float_open(&h), shut, "{tab:?}");
    }
}

/// A fresh card fills to its cap, so its foot takes the last line and the gap
/// falls above the foot, not under it.
#[test]
fn austart_foot_takes_the_last_line() {
    for (w, tall, stored, ..) in PLACES {
        let mut h = EditorHarness::as_first_run();
        window(&mut h, w, tall, stored);
        let (card, ppp) = (float_open(&h), h.ctx().pixels_per_point());
        let (above, foot) = (h.rect("Open a project") / ppp, h.rect("Check your pen") / ppp);
        let at = format!("{w}×{tall} at {stored}: {foot:?} {card:?}");
        println!("{at}");
        assert_eq!(foot.top(), (h.rect("What's new") / ppp).top(), "{at}");
        assert!(card.bottom() - foot.bottom() < 16.0, "{at}");
        assert!(foot.top() - above.bottom() > 16.0, "{at}");
    }
}

/// The rail's rect and the card's, at a window of `PLACES`, with `h`'s room.
fn rail_and_card(h: &mut EditorHarness, w: f32, tall: f32, stored: f32) -> (Rect, Rect) {
    window(h, w, tall, stored);
    h.editor_mut().floats.more = false;
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(h);
    let rail = h.ctx().memory(|m| m.area_rect(Id::new("rail"))).expect("the rail");
    (rail, float_open(h))
}

/// Clear of the rail on a dock with no left column, spec 4.2's case. Where the
/// dock has one, the rail and the left floats stand on the canvas beside it,
/// and spec 4.1's centre is the dock's: in Plan only the widest row clears it.
/// Pinned as measured, for the Lead's list: its keys answer again when Start
/// closes.
#[test]
fn austart_clears_the_rail() {
    for (w, tall, stored, ..) in PLACES {
        let (rail, card) = rail_and_card(&mut EditorHarness::as_first_run(), w, tall, stored);
        assert!(rail.right() <= 68.5, "{w}×{tall} at {stored}: {rail:?}");
        assert!(!rail.intersects(card), "{w}×{tall} at {stored}: {rail:?} {card:?}");
    }
    for (w, tall, stored, ..) in PLACES {
        let (rail, card) = rail_and_card(&mut EditorHarness::as_animation(), w, tall, stored);
        let at = format!("Plan at {w}×{tall}, {stored}: {rail:?} {card:?}");
        println!("{at}");
        assert_eq!(rail.intersects(card), (w, stored) != (1600.0, 1.0), "{at}");
    }
    let mut h = EditorHarness::as_animation();
    rail_and_card(&mut h, 1024.0, 640.0, 1.0);
    let held = h.editor().canvas.tool;
    h.click("Select");
    settle(&mut h);
    assert_eq!(h.editor().canvas.tool, held, "the card takes the press");
    h.click("Close Start");
    settle(&mut h);
    h.click("Select");
    settle(&mut h);
    assert_ne!(h.editor().canvas.tool, held, "and with Start closed the rail answers");
}

/// The card lies wholly on the ready paper, so it costs its own area and no
/// more. The worst gated size leaves paper on all four sides: the floors are
/// the first measured values less 0.5. Spec 4.2 reckons 56 and 67 on 735×413,
/// but Fit keeps a ready paper 76 clear of each side: 680×382.
#[test]
fn austart_paper_uncovered() {
    let mut h = first_run();
    let fresh = measure::paper_share(&h);
    println!("paper uncovered at 1600×900, fresh: {fresh:?}, {:?}", float_open(&h));
    let dir = tempfile::tempdir().unwrap();
    at_the_cap(&mut h, dir.path());
    let share = measure::paper_share(&h);
    println!("paper uncovered at 1600×900, at the cap: {share:?}");
    assert!(share.paper >= 58.0, "paper uncovered at 1600×900: {share:?}");
    // The card is one size, and the reading sees it: closed, the paper is back.
    assert!((fresh.paper - share.paper).abs() < 0.05, "{fresh:?} {share:?}");
    h.click("Close Start");
    settle(&mut h);
    let closed = measure::paper_share(&h);
    assert!(closed.paper > share.paper + 10.0, "{closed:?} {share:?}");
    let mut h = EditorHarness::as_first_run();
    window(&mut h, 1024.0, 640.0, 1.5);
    at_the_cap(&mut h, dir.path());
    let xf = h.editor().canvas.view_xform();
    let corners = [pos2(0.0, 0.0), pos2(xf.paper.x, xf.paper.y)].map(|at| xf.paper_to_screen(at));
    let (paper, card) = (Rect::from_two_pos(corners[0], corners[1]), float_open(&h));
    assert!(paper.contains_rect(card), "{paper:?} {card:?}");
    let free = [card.top() - paper.top(), paper.bottom() - card.bottom()];
    let sides = [card.left() - paper.left(), paper.right() - card.right()];
    println!("paper at 832×520: {paper:?}, free {free:?} {sides:?}");
    assert!(free.iter().all(|f| *f >= 40.4) && sides.iter().all(|s| *s >= 39.3), "{free:?} {sides:?}");
}

/// Stored scales past the gated three clip nothing: the same `min` caps them.
#[test]
fn austart_stored_200_clips_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = EditorHarness::as_first_run();
    window(&mut h, 1024.0, 640.0, 2.0);
    let fresh = measure::clip_here(&h, Scope::Chrome);
    assert_eq!(fresh.clipped + fresh.outside, 0, "{:?}", fresh.found);
    at_the_cap(&mut h, dir.path());
    let full = measure::clip_here(&h, Scope::Chrome);
    assert_eq!(full.clipped + full.outside, 0, "{:?}", full.found);
    assert!(dock(&h).contains_rect(float_open(&h)));
}

/// A left float opened on the card ends it, as Layers does, whichever float;
/// one beside it shares the screen. On a dock with no left column none reaches
/// it at 1600×900. Plan's left column puts the float further in: there it does,
/// pinned as measured, for the Lead's list.
#[test]
fn austart_left_float_closes_it() {
    let lefts = [Float::Brushes, Float::Color, Float::ToolOptions, Float::CanvasOptions];
    let mut closed = 0;
    for f in lefts {
        let mut h = first_run();
        let card = float_open(&h);
        h.open_float(f);
        settle(&mut h);
        let area = Id::new(("float", f.heading()));
        let float = h.ctx().memory(|m| m.area_rect(area)).expect("the float");
        assert!(!float.intersects(card) && is_open(&h), "{f:?} at 1600×900: {float:?} {card:?}");
        for (w, tall, stored) in [(1280.0, 800.0, 1.0), (1024.0, 640.0, 1.0), (1024.0, 640.0, 1.5)] {
            let mut h = EditorHarness::as_first_run();
            window(&mut h, w, tall, stored);
            let card = float_open(&h);
            h.open_float(f);
            settle(&mut h);
            let float = h.ctx().memory(|m| m.area_rect(area)).expect("the float");
            let at = format!("{f:?} at {w}×{tall}, {stored}: {float:?} {card:?}");
            assert_eq!(is_open(&h), !float.intersects(card), "{at}");
            assert!(h.editor().floats.is_open(f), "the float stays: {at}");
            closed += usize::from(!is_open(&h));
        }
    }
    assert!(closed >= 4, "{closed}");
    // Start opened over one is the newer: the float makes way, and it stands.
    let mut h = EditorHarness::as_shipped();
    window(&mut h, 1024.0, 640.0, 1.5);
    h.open_float(Float::Brushes);
    settle(&mut h);
    h.perform(Action::WelcomeScreen);
    settle(&mut h);
    assert!(is_open(&h), "Start stands");
    assert!(!h.editor().floats.is_open(Float::Brushes), "the float made way");
    // Beside it, the two share the screen, and that stands too.
    let mut h = first_run();
    h.click("Close Start");
    h.open_float(Float::Brushes);
    settle(&mut h);
    h.perform(Action::WelcomeScreen);
    settle(&mut h);
    assert!(is_open(&h) && h.editor().floats.is_open(Float::Brushes));
    // Plan at 1600×900: the sheet's column carries the float onto the card.
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    assert!(is_open(&h));
    h.open_float(Float::Brushes);
    settle(&mut h);
    assert!(!is_open(&h), "opened over the card, the float ends it");
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    assert!(is_open(&h), "Start stands");
    assert!(!h.editor().floats.is_open(Float::Brushes), "opened over the float, it makes way");
}

/// The popover's rect, if this frame's layers hold it.
fn popover(h: &EditorHarness) -> Option<Rect> {
    let id = Id::new(floats::POPOVER);
    let shown = |m: &egui::Memory| m.areas().visible_layer_ids().iter().any(|l| l.id == id);
    h.ctx().memory(|m| shown(m).then(|| m.area_rect(id)).flatten())
}

/// The Pocket tier's `…` stands where a left float does, and trades places
/// with the card as one. That tier needs a short window: 420 tall is
/// `aurail_pocket_reaches_every_verb_in_two_taps`'s.
#[test]
fn austart_pocket_popover_closes_it() {
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 832.0, 420.0);
    let card = float_open(&h);
    h.editor_mut().floats.toggle_more();
    settle(&mut h);
    let shown = popover(&h).expect("the Pocket tier's popover");
    assert!(shown.intersects(card), "{shown:?} {card:?}");
    assert!(!is_open(&h), "opened over the card, it ends it");
    assert!(h.editor().floats.more, "and stands");
    // Start opened over it is the newer of the two.
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    assert!(is_open(&h), "Start stands");
    assert!(!h.editor().floats.more && popover(&h).is_none(), "the popover made way");
}

/// Only an opening closes the card. The two can come to overlap with nothing
/// opened, and then the card stays, as a stale rect does not end it.
#[test]
fn austart_only_an_opening_closes_it() {
    // A narrower window brings a float that stood beside the card onto it.
    let mut h = first_run();
    h.open_float(Float::Brushes);
    settle(&mut h);
    assert!(is_open(&h));
    sized(&mut h, 1024.0, 640.0);
    let area = Id::new(("float", Float::Brushes.heading()));
    let float = h.ctx().memory(|m| m.area_rect(area)).expect("the float");
    assert!(float.intersects(float_open(&h)), "{float:?} {:?}", float_open(&h));
    assert!(is_open(&h) && h.editor().floats.is_open(Float::Brushes), "both stand");
    // Hide tools takes the float off and brings it back: nothing opened.
    h.editor_mut().tools_hidden = true;
    settle(&mut h);
    h.editor_mut().tools_hidden = false;
    settle(&mut h);
    assert!(is_open(&h) && h.editor().floats.is_open(Float::Brushes), "both come back");
    // Pressing a row grows nothing into a float at this size.
    h.click("More sizes");
    settle(&mut h);
    assert!(is_open(&h));
    // The popover keeps its area rect, and `more`, past the Pocket tier.
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1280.0, 420.0);
    h.editor_mut().floats.toggle_more();
    settle(&mut h);
    let last = popover(&h).expect("the Pocket tier's popover");
    assert!(is_open(&h), "beside the card: {last:?} {:?}", float_open(&h));
    sized(&mut h, 832.0, 570.0);
    assert!(h.editor().floats.more && popover(&h).is_none(), "Compact draws none");
    assert!(last.intersects(float_open(&h)), "{last:?} {:?}", float_open(&h));
    assert!(is_open(&h), "a popover not drawn closes nothing");
    // A float opened under Hide tools is an opening, the frame it first stands.
    // Its rect is still in memory from last time, and that is no such frame.
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1024.0, 640.0);
    h.open_float(Float::Brushes);
    settle(&mut h);
    h.editor_mut().floats.close_left();
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    assert!(is_open(&h));
    h.editor_mut().tools_hidden = true;
    settle(&mut h);
    h.open_float(Float::Brushes);
    settle(&mut h);
    h.editor_mut().tools_hidden = false;
    settle(&mut h);
    assert!(h.editor().floats.is_open(Float::Brushes) && !is_open(&h), "it ends the card on showing");
}

/// The nine rows on `h`'s float, on `tab`.
fn stand_in(h: &mut EditorHarness, tab: WelcomeTab) {
    let mut w = Welcome::new(&[]);
    w.lessons = lessons::STAND_IN;
    w.tab = tab;
    h.editor_mut().start = Some(w);
    settle(h);
}

/// With no lesson on the table row two is `Open the example`, and the page has
/// no way in. AU23: three lessons ship, so a first run reads `Follow a lesson`,
/// one control for the other, and `Help ▸ Lessons` opens the page; the empty
/// table stays the row's other state, set here by hand.
#[test]
fn austart_row_two_waits_for_lessons() {
    assert!(!lessons::ALL.is_empty(), "the Rookie lessons ship");
    let empty = || {
        let mut w = Welcome::new(&[]);
        w.lessons = &[];
        w
    };
    let mut h = first_run();
    h.editor_mut().start = Some(empty());
    settle(&mut h);
    assert!(h.has("Open the example") && !h.has("Follow a lesson"));
    assert_eq!(Action::OpenLessons.menu(), Some("Help/Lessons"));
    assert_eq!(empty().on_lessons().tab, WelcomeTab::Start, "no way in by the verb");
    let (row, card) = (h.rect("Open the example"), float_open(&h));
    h.click("Open the example");
    settle(&mut h);
    assert_eq!(h.rig().requests, [AppRequest::Start(WelcomeAction::Sample)]);
    // The same row, the same place, now there are lessons.
    let mut h = first_run();
    assert_eq!(h.editor().can_perform(Action::OpenLessons), Ok(()));
    assert!(h.has("Follow a lesson") && !h.has("Open the example"));
    assert_eq!((h.rect("Follow a lesson"), float_open(&h)), (row, card), "one control for one");
    h.click("Follow a lesson");
    settle(&mut h);
    assert_eq!(h.editor().start.as_ref().map(|w| w.tab), Some(WelcomeTab::Lessons));
    assert!(h.has(LESSONS_HEADING) && h.has("Close Lessons") && !h.has("Close Start"));
    assert_eq!(float_open(&h).width(), card.width(), "the same card");
    assert!(float_open(&h).height() <= 300.5);
    assert!(h.has("Open the example"), "the example moves here");
    h.click("Back to Start");
    settle(&mut h);
    assert!(h.has("Follow a lesson") && h.has("Close Start"));
    let mut w = Welcome::new(&[]);
    w.lessons = lessons::STAND_IN;
    assert_eq!(w.on_lessons().tab, WelcomeTab::Lessons, "the verb's way in");
}

#[test]
fn austart_page_budget() {
    let mut h = first_run();
    stand_in(&mut h, WelcomeTab::Lessons);
    let page = built_census(&mut h);
    println!("Lessons page: {} controls, {} words", page.controls, page.words);
    assert!(page.controls <= 12 && page.words <= 48, "{page:?}");
    let shown = float_census(&mut h);
    assert_eq!((shown.controls, shown.words), (page.controls, page.words), "unscrolled: all of it shows");
    assert!(!h.has("Show at startup"), "the latch is the Start page's: this page's budget has no room");
    // `Follow a lesson` takes the place of `Open the example`, one for one.
    let dir = tempfile::tempdir().unwrap();
    stand_in(&mut h, WelcomeTab::Start);
    let fresh = built_census(&mut h);
    println!("Start with lessons: {} controls, {} words", fresh.controls, fresh.words);
    assert!(fresh.controls <= 12 && fresh.words <= 25, "{fresh:?}");
    let mut w = Welcome::new(&recents(dir.path()));
    w.lessons = lessons::STAND_IN;
    h.editor_mut().start = Some(w);
    let worst = built_census(&mut h);
    println!("Start with lessons, worst case: {} controls, {} words", worst.controls, worst.words);
    assert!(worst.controls <= 14 && worst.words <= 30, "{worst:?}");
    assert!(worst.controls > fresh.controls && worst.words > fresh.words, "{fresh:?} {worst:?}");
}

/// The words the last frame painted as `word`, by the lines it took.
fn lines_of(h: &EditorHarness, word: &str) -> usize {
    fn walk(s: &egui::Shape, word: &str) -> Option<usize> {
        match s {
            egui::Shape::Vec(v) => v.iter().find_map(|s| walk(s, word)),
            egui::Shape::Text(t) if t.galley.text() == word => Some(t.galley.rows.len()),
            _ => None,
        }
    }
    let found = h.kit().output().shapes.iter().find_map(|c| walk(&c.shape, word));
    found.unwrap_or_else(|| panic!("{word:?} was not painted"))
}

/// The ink lines the last frame painted, by their bounds.
fn ink_lines(h: &EditorHarness) -> Vec<Rect> {
    fn walk(s: &egui::Shape, out: &mut Vec<Rect>) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            egui::Shape::Path(p) if p.stroke.color == egui::epaint::ColorMode::Solid(plate::INK) => {
                out.push(s.visual_bounding_rect());
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.kit().output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

static ART_BUILT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn counted_art() -> Vec<lessons::Path> {
    ART_BUILT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let points = vec![[0.0, 0.0, 1.0], [10.0, 5.0, 1.0], [20.0, 10.0, 1.0]];
    vec![lessons::Path::new(points, 2.0)]
}

static COUNTED: [lessons::Lesson; 1] = [lessons::Lesson {
    id: "counted",
    title: "Counted",
    art: counted_art,
    ..lessons::Lesson::BLANK
}];

/// Each card's picture is drawn from its lesson's paths, on its paper square,
/// and the paths are built once however long the page stands: spec 5 makes
/// `art` a lesson's whole set.
#[test]
fn austart_page_cards_hold_their_pictures() {
    for lesson in lessons::ALL.iter().chain(lessons::STAND_IN) {
        let art = (lesson.art)();
        assert!(!lesson.finish.is_empty(), "{}", lesson.title);
        assert!(lesson.finish.iter().all(|i| usize::from(*i) < art.len()), "{}", lesson.title);
    }
    let mut h = first_run();
    stand_in(&mut h, WelcomeTab::Lessons);
    let lines = ink_lines(&h);
    for lesson in lessons::STAND_IN {
        let card = h.rect(lesson.title);
        let side = card.height() - 2.0 * plate::PICTURE_INSET;
        let at = card.left_top() + vec2(plate::PICTURE_INSET, plate::PICTURE_INSET);
        let paper = Rect::from_min_size(at, vec2(side, side));
        let drawn = lines.iter().find(|line| paper.contains_rect(**line));
        let drawn = drawn.unwrap_or_else(|| panic!("{}: no ink on {paper:?} in {lines:?}", lesson.title));
        assert!(drawn.width() >= side / 2.0, "{}: {drawn:?} on {paper:?}", lesson.title);
    }
    let mut w = Welcome::new(&[]);
    w.lessons = &COUNTED;
    w.tab = WelcomeTab::Lessons;
    h.editor_mut().start = Some(w);
    h.run_frames(6);
    settle(&mut h);
    assert_eq!(ART_BUILT.load(std::sync::atomic::Ordering::SeqCst), 1, "once, not once a frame");
}

/// The page's edges line up: a class's word on its column's thumbnails, the
/// kinds on one line each in the gutter of 72, and the two ways out read as
/// keys over the bold legends.
#[test]
fn austart_page_lines_up() {
    let mut h = first_run();
    stand_in(&mut h, WelcomeTab::Lessons);
    for (class, lesson) in lessons::Class::ALL.into_iter().zip(lessons::STAND_IN) {
        let picture = h.rect(lesson.title).left() + plate::PICTURE_INSET;
        assert_eq!(h.painted_at(class.word()).x, picture, "{class:?}");
    }
    let edge = float_open(&h).left() + 12.0 + plate::CARD_PAD;
    for word in ["A drawing", "An animation", "A comic"] {
        assert_eq!(lines_of(&h, word), 1, "{word}");
        assert_eq!(h.painted_at(word).x, edge, "{word}");
    }
    let kind = h.rect(lessons::STAND_IN[0].title).left() - plate::CARD_PAD - edge + 12.0 + plate::CARD_PAD;
    assert!(kind >= 72.0, "the gutter's text box: {kind}");
    let keys = h.filled_rects(plate::WELL);
    for row in ["Back to Start", "Open the example"] {
        assert!(keys.contains(&h.rect(row)), "{row} has a ground at rest: {keys:?}");
    }
}

/// The nine cards stand whole in the card at every size: nothing to scroll,
/// and nothing cut.
#[test]
fn austart_grid_stands_unscrolled() {
    for (w, tall, stored, ..) in PLACES {
        let mut h = EditorHarness::as_first_run();
        window(&mut h, w, tall, stored);
        stand_in(&mut h, WelcomeTab::Lessons);
        let card = float_open(&h);
        let at = format!("{w}×{tall} at {stored}: {card:?}");
        assert!(card.height() <= 300.5, "{at}");
        assert!((card.center() - dock(&h).center()).length() <= 1.0, "{at}");
        let ppp = h.ctx().pixels_per_point();
        for lesson in lessons::STAND_IN {
            // The harness reads a widget in pixels; the card is in points.
            let row = h.rect(lesson.title) / ppp;
            let at = format!("{} {row:?} in {at}", lesson.title);
            assert!((row.height() - welcome::PAGE_ROW_H).abs() < 0.1, "{at}");
            assert!(row.bottom() <= card.bottom() - 12.0, "inside the body's viewport: {at}");
            assert!(card.contains_rect(row), "{at}");
        }
        let clip = measure::clip_here(&h, Scope::All);
        let inside: Vec<_> = clip.found.iter().filter(|f| lessons::STAND_IN.iter().any(|l| f.contains(l.title))).collect();
        assert!(inside.is_empty(), "{inside:?} in {at}");
    }
}

/// A card sends its own row, and comes through the door every document does:
/// the unsaved guard, the guest's refusal, an untouched blank's silence.
#[test]
fn austart_page_cards_go_through_leave() {
    assert!(WelcomeAction::Lesson(0).replaces_document());
    for (i, lesson) in lessons::STAND_IN.iter().enumerate() {
        let mut h = first_run();
        stand_in(&mut h, WelcomeTab::Lessons);
        h.click(lesson.title);
        settle(&mut h);
        assert_eq!(h.rig().requests, [AppRequest::Start(WelcomeAction::Lesson(i))], "{}", lesson.title);
        assert_eq!(h.editor().leave, None, "a blank asks nothing");
    }
    // The grid reads columns of classes, rows of kinds.
    let mut h = first_run();
    stand_in(&mut h, WelcomeTab::Lessons);
    for row in lessons::STAND_IN.chunks(3) {
        let at: Vec<_> = row.iter().map(|l| h.rect(l.title)).collect();
        assert!(at[0].top() == at[2].top() && at[0].right() < at[1].left() && at[1].right() < at[2].left());
    }
    for (class, head) in lessons::Class::ALL.into_iter().zip(lessons::STAND_IN) {
        let word = h.painted_at(class.word());
        assert!(word.x >= h.rect(head.title).left(), "{class:?} over its column");
    }
    let mut h = first_run();
    h.click("Close Start");
    settle(&mut h);
    let at = paper_middle(&h);
    line_at(&mut h, at);
    stand_in(&mut h, WelcomeTab::Lessons);
    h.click(lessons::STAND_IN[0].title);
    settle(&mut h);
    assert_eq!(h.editor().leave, Some(Leave::Start(WelcomeAction::Lesson(0))), "asked first");
    assert!(h.rig().requests.is_empty());
    let mut h = first_run();
    h.editor_mut().canvas.is_guest = true;
    stand_in(&mut h, WelcomeTab::Lessons);
    let seq = h.state().refusal_seq;
    h.click(lessons::STAND_IN[0].title);
    settle(&mut h);
    assert_eq!(h.state().refusal_seq, seq + 1, "said no to");
    assert!(h.rig().requests.is_empty());
}

/// A chord over the card acts as it does with the card closed: the pane under
/// the pointer answers, whichever it is.
#[test]
fn austart_chords_stay_live() {
    // Over paper.
    let mut h = first_run();
    let on = float_open(&h).center();
    h.point_at(on);
    settle(&mut h);
    let ctx = h.ctx();
    assert_eq!(h.editor_mut().shortcut_ctx(&ctx), Ctx::Canvas);
    let from = h.performed().len();
    h.key_mods(CTRL, Key::V);
    let fired: Vec<_> = h.performed()[from..].iter().map(|(a, _)| *a).collect();
    assert!(fired.contains(&Action::PastePixels), "{fired:?}");
    assert!(is_open(&h), "it stands");
    // Over the sheet: Plan on a window where the card reaches it.
    let mut h = EditorHarness::as_animation();
    window(&mut h, 1024.0, 640.0, 1.5);
    let sheet = h.editor().pane_rects.iter().find(|(p, ..)| *p == Pane::XSheet);
    let over = sheet.expect("Plan's sheet").1;
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    let card = float_open(&h);
    assert!(card.intersects(over), "{card:?} {over:?}");
    let on = pos2(card.left() + 10.0, card.center().y);
    assert!(over.contains(on), "{on:?} in {over:?}");
    h.point_at(on);
    settle(&mut h);
    let ctx = h.ctx();
    assert_eq!(h.editor_mut().shortcut_ctx(&ctx), Ctx::Sheet, "as with the card closed");
    let from = h.performed().len();
    h.key_mods(CTRL, Key::V);
    let fired: Vec<_> = h.performed()[from..].iter().map(|(a, _)| *a).collect();
    assert!(fired.contains(&Action::PasteCells), "{fired:?}");
    assert!(!fired.contains(&Action::PastePixels), "{fired:?}");
    h.click("Close Start");
    settle(&mut h);
    h.point_at(on);
    settle(&mut h);
    let ctx = h.ctx();
    assert_eq!(h.editor_mut().shortcut_ctx(&ctx), Ctx::Sheet);
}

#[test]
fn austart_float_blocks_only_its_own_rect() {
    let mut h = first_run();
    let float = float_open(&h);
    // A pen coming down on the float paints nothing, and leaves it up.
    let on = float.left_bottom() + vec2(40.0, -6.0);
    h.drag_path(&[on, on - vec2(20.0, 0.0), on - vec2(30.0, 6.0)]);
    settle(&mut h);
    assert_eq!(strokes(&h), 0, "the float took it");
    assert!(is_open(&h));
    // The rail and the menus stand beside it, live.
    h.click("Brush");
    settle(&mut h);
    assert!(h.editor().floats.is_open(Float::Brushes), "a left float opens beside it");
    assert!(is_open(&h), "and the two share the screen");
    h.editor_mut().floats.close(Float::Brushes);
    settle(&mut h);
    // One point past its edge is paper.
    let beside = pos2(float.left() - 2.0, float.center().y);
    line_at(&mut h, beside);
    assert_eq!(strokes(&h), 1, "the point beside it paints");
    assert!(!is_open(&h));
}

// ---- what closes it ----

#[test]
fn austart_esc_closes_the_float() {
    let mut h = first_run();
    h.key(Key::Escape);
    settle(&mut h);
    assert!(!is_open(&h), "Esc closes it");
    // Nearer things have the key first: the palette, then a menu.
    let mut h = first_run();
    h.perform(Action::CommandPalette);
    settle(&mut h);
    h.key(Key::Escape);
    settle(&mut h);
    assert!(!h.editor().palette.open, "the palette takes it");
    assert!(is_open(&h), "and the float stays");
    h.click(crate::vocab::MENU_FILE);
    settle(&mut h);
    assert!(egui::Popup::is_any_open(&h.ctx()));
    h.key(Key::Escape);
    settle(&mut h);
    assert!(!egui::Popup::is_any_open(&h.ctx()), "the menu takes it");
    assert!(is_open(&h));
    // A stroke under way, and the unsaved question.
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    let at = free_paper(&h);
    h.pen_down(at);
    h.pen_move(at + vec2(-30.0, 0.0));
    assert!(h.editor().canvas.stroke_active());
    h.key(Key::Escape);
    assert!(is_open(&h), "the stroke's end decides, and that is the next line");
    h.pen_up(at + vec2(-30.0, 0.0));
    settle(&mut h);
    assert!(!is_open(&h));
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    assert!(h.state().is_dirty());
    h.click("A drawing");
    settle(&mut h);
    assert!(h.has("Save changes?"));
    h.key(Key::Escape);
    settle(&mut h);
    assert!(!h.has("Save changes?"), "the question takes it");
    assert!(is_open(&h));
}

#[test]
fn austart_close_control_closes_the_float() {
    let mut h = first_run();
    let (before, group) = (strokes(&h), h.state().untitled_group.clone());
    h.click("Close Start");
    settle(&mut h);
    assert!(!is_open(&h));
    assert_eq!((strokes(&h), h.state().untitled_group.clone()), (before, group), "the paper stays");
    assert!(h.state().frame_count() == 1 && !h.editor().strip_open);
    // A click anywhere else is no dismissal.
    let mut h = first_run();
    h.click("Brush");
    settle(&mut h);
    assert!(is_open(&h), "no click-outside dismissal");
}

#[test]
fn austart_opening_layers_closes_start() {
    let mut h = first_run();
    h.click("Layers");
    settle(&mut h);
    assert!(h.editor().floats.is_open(Float::Layers), "Layers opened");
    assert!(!is_open(&h), "one float per side");
    // And the other way round.
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    assert!(is_open(&h));
    assert!(!h.editor().floats.is_open(Float::Layers), "Layers made way");
    assert!(float_rect(&h).is_some());
}

#[test]
fn austart_float_never_opens_during_a_stroke() {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    let at = paper_middle(&h);
    h.pen_down(at);
    h.pen_move(at + vec2(30.0, 0.0));
    assert!(h.editor().canvas.stroke_active());
    h.perform(Action::WhatsNew);
    h.run_frames(2);
    assert!(!is_open(&h), "it waits for pen-up");
    h.pen_move(at + vec2(60.0, 5.0));
    h.pen_up(at + vec2(60.0, 5.0));
    settle(&mut h);
    assert!(is_open(&h), "it opens at pen-up, and that stroke does not end it");
    assert_eq!(h.editor().start.as_ref().map(|w| w.tab), Some(WelcomeTab::WhatsNew));
}

#[test]
fn austart_shortcuts_stay_live_under_the_float() {
    let mut h = first_run();
    assert!(h.ctx().memory(|m| m.top_modal_layer()).is_none(), "nothing is modal");
    h.point_at(float_open(&h).center());
    settle(&mut h);
    h.key_mods(Modifiers::COMMAND, Key::K);
    settle(&mut h);
    assert!(h.editor().palette.open, "the chord fired with the float open: {:?}", h.performed());
    assert!(is_open(&h));
    assert_eq!(h.editor().leave, None);
}

#[test]
fn austart_show_at_startup_off_keeps_the_paper() {
    let mut h = first_run();
    assert!(h.rig().config.ui.show_welcome);
    h.click("Show at startup");
    settle(&mut h);
    assert!(!h.rig().config.ui.show_welcome);
    let (stored, _) = Config::load_from(h.data_dir());
    assert!(!stored.ui.show_welcome, "the choice is saved");
    assert!(is_open(&h), "it takes hold at the next launch");
    // The next launch: the same paper, with no float.
    assert_eq!(startup_route(false, false, None, &stored), Startup::ReadyPaper);
    let paper = crate::open_ready(None, Some(h.data_dir()), &stored.ui);
    assert!(paper.start.is_none());
    assert_eq!(paper.stage, Some(Stage::Drawing));
    assert!(!paper.strip_open);
    let project = &paper.state.engine.project;
    assert_eq!((project.width, project.height, paper.state.fps()), (1280, 720, 12));
}

#[test]
fn austart_untouched_blank_never_asks_to_save() {
    let mut h = first_run();
    assert!(!h.state().is_dirty(), "an untouched blank is clean");
    h.editor_mut().autosave.minutes = 1;
    h.wait(90.0);
    assert!(h.editor().autosave_job.is_none(), "no autosave of nothing");
    assert!(h.editor().last_autosave.is_none());
    h.perform(Action::Open);
    settle(&mut h);
    assert_eq!(h.editor().leave, None, "the guard has nothing to ask");
    let cmds = h.request_close();
    assert!(!cmds.contains(&egui::ViewportCommand::CancelClose), "{cmds:?}");
    assert!(!h.has("Save changes?"));
    // The float's own choices leave it clean too.
    let mut h = first_run();
    h.click("More sizes");
    h.click("Close Start");
    settle(&mut h);
    assert!(!h.state().is_dirty());
}

// ---- its choices ----

#[test]
fn austart_every_entry_lands_a_stroke() {
    use templates::{ANIMATION, PICTURE, PICTURE_SQUARE, PICTURE_TALL};
    let dir = tempfile::tempdir().unwrap();
    let recent = recents(dir.path()).remove(0);
    let cards = [ANIMATION, PICTURE, PICTURE_SQUARE, PICTURE_TALL].map(WelcomeAction::Intent);
    let sizes = (0..templates::ALL.len()).map(WelcomeAction::Template);
    let others = [WelcomeAction::Sample, WelcomeAction::ShowMe(0), WelcomeAction::OpenRecent(recent)];
    let entries = cards.len() + templates::ALL.len() + others.len();
    let mut landed = 0;
    for action in cards.into_iter().chain(sizes).chain(others) {
        let mut h = first_run();
        h.answer_start(action.clone());
        settle(&mut h);
        assert!(!is_open(&h), "{action:?}: a document chosen ends the float");
        if let WelcomeAction::OpenRecent(file) = &action {
            assert_eq!(h.state().file_path.as_ref(), Some(file), "the file is open");
        }
        let before = h.state().strokes_done;
        let at = paper_middle(&h);
        assert!(h.editor().canvas.dbg_rect.contains(at), "{action:?}: the paper is on screen");
        line_at(&mut h, at);
        let (room, said) = (h.editor().stage, h.state().status.clone());
        assert_eq!(h.state().strokes_done, before + 1, "{action:?} in {room:?}: {said}");
        assert!(h.state().is_dirty(), "{action:?}");
        landed += 1;
    }
    assert_eq!(landed, entries);
    assert!(landed >= 8);
    // The two that need the shell go out as requests.
    let mut h = first_run();
    h.answer_start(WelcomeAction::Open);
    h.answer_start(WelcomeAction::Custom);
    assert_eq!(h.rig().requests, [AppRequest::Open, AppRequest::NewProject]);
    assert!(is_open(&h), "and the float waits for what they do");
}

/// A project stands on a column a stroke lands on when it opens: past a plate
/// with no armed layer, past a locked column. With none it stands on its
/// first. Where it stands is the view's: the file saves back byte for byte.
#[test]
fn austart_a_project_opens_on_a_column_that_draws() {
    let dir = tempfile::tempdir().unwrap();
    let (file, again) = (dir.path().join("shot.animproj"), dir.path().join("again.animproj"));
    let opened = |locked: &[&str]| {
        let mut state = crate::sample::build_sample();
        let (scene, cut) = (state.view.scene, state.view.cut);
        let sheet = state.engine.project.cut_mut(scene, cut).expect("the cut");
        let ids: Vec<_> = sheet.xsheet.columns.iter().filter(|c| locked.contains(&c.name.as_str())).map(|c| c.id).collect();
        for id in ids {
            let def = sheet.column_def(id);
            sheet.set_column_def(id, anim_core::xsheet::ColumnDef { locked: true, ..def });
        }
        anim_core::store::save(&state.engine.project, &file).expect("saved");
        let opened = crate::doc::AppState::load_from(file.clone()).expect("opened");
        anim_core::store::save(&opened.engine.project, &again).expect("saved");
        assert_eq!(std::fs::read(&file).unwrap(), std::fs::read(&again).unwrap());
        let on = opened.cut().xsheet.columns.iter().find(|c| c.id == opened.view.active_column);
        (on.expect("a column").name.clone(), opened.active_slot_resolved().is_some())
    };
    assert_eq!(opened(&[]), ("A".to_string(), true), "past the plate");
    assert_eq!(opened(&["A"]), ("B".to_string(), true), "past the lock");
    assert_eq!(opened(&["A", "B"]), ("BG".to_string(), false), "none draws: the first");
}

#[test]
fn austart_cards_make_what_they_say() {
    let size = |h: &EditorHarness| {
        let p = &h.state().engine.project;
        (p.width, p.height)
    };
    for (card, want) in [("A drawing", (1920, 1080)), ("Square", (2048, 2048)), ("Tall", (1080, 1920))] {
        let mut h = first_run();
        let was = h.state().untitled_group.clone();
        h.click(card);
        settle(&mut h);
        let asked = h.rig().requests.last().cloned();
        let Some(AppRequest::Start(action)) = asked else { panic!("{card}: {asked:?}") };
        assert_ne!(action, WelcomeAction::Intent(templates::ANIMATION));
        h.answer_start(action);
        assert_ne!(h.state().untitled_group, was, "{card}");
        assert_eq!(size(&h), want, "{card}");
        assert_eq!(h.editor().stage, Some(Stage::Drawing), "{card}");
        assert!(!h.editor().strip_open, "{card}: a picture closes the strip");
    }
    let mut h = first_run();
    h.click("An animation");
    settle(&mut h);
    assert_eq!(
        h.rig().requests.last(),
        Some(&AppRequest::Start(WelcomeAction::Intent(templates::ANIMATION)))
    );
    h.answer_start(WelcomeAction::Intent(templates::ANIMATION));
    assert!(h.editor().strip_open);
    let mut h = first_run();
    h.click("A comic");
    settle(&mut h);
    let asked = h.rig().requests.last().cloned();
    let Some(AppRequest::Start(action)) = asked else { panic!("{asked:?}") };
    h.answer_start(action);
    assert_eq!(size(&h), (2591, 3624), "Manga B5");
    assert_eq!(h.editor().stage, Some(Stage::Manga));
    assert_eq!(h.state().fps(), 24, "a studio preset stays at 24");
}

#[test]
fn austart_animatic_lands_with_the_brush_on_the_canvas() {
    let animatic = templates::ALL.iter().position(|t| t.room == Stage::Edit).expect("the Animatic");
    let mut h = first_run();
    h.answer_start(WelcomeAction::Template(animatic));
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Edit));
    assert!(crate::visible_panes(&h.editor().dock).contains(&Pane::Canvas), "the canvas tab is front");
    assert_eq!(h.editor().canvas.tool, crate::canvas::CanvasTool::Paint, "the brush, not Timing's Select");
    assert_eq!(h.state().fps(), 24);
    // TV anime and Feature land in Draw with the sheet on.
    for name in ["TV anime HD", "Feature UHD"] {
        let i = templates::ALL.iter().position(|t| t.name == name).expect(name);
        let mut h = first_run();
        h.answer_start(WelcomeAction::Template(i));
        settle(&mut h);
        assert_eq!(h.editor().stage, Some(Stage::Drawing), "{name}");
        assert!(!h.editor().plan_sheet_hidden && h.editor().sheet_on_screen(), "{name}");
    }
}

#[test]
fn austart_a_choice_over_unsaved_work_goes_through_leave() {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    let at = paper_middle(&h);
    line_at(&mut h, at);
    assert!(h.state().is_dirty());
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    h.click("A drawing");
    settle(&mut h);
    let held = WelcomeAction::Intent(templates::PICTURE);
    assert_eq!(h.editor().leave, Some(Leave::Start(held.clone())), "asked first");
    assert!(h.has("Save changes?"));
    assert!(h.rig().requests.is_empty(), "nothing goes out yet");
    h.hold("Don't Save");
    settle(&mut h);
    assert_eq!(h.rig().requests, [AppRequest::Start(held)], "the answer carries the choice on");
    // A clean document goes straight out.
    let mut h = first_run();
    h.click("A drawing");
    settle(&mut h);
    assert_eq!(h.rig().requests, [AppRequest::Start(WelcomeAction::Intent(templates::PICTURE))]);
    assert_eq!(h.editor().leave, None);
}

#[test]
fn austart_a_guest_stays_in_the_room() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Dropped.animproj");
    anim_core::store::save(&sample::build_sample().engine.project, &file).expect("saved");
    let mut h = first_run();
    h.editor_mut().canvas.is_guest = true;
    let seq = h.state().refusal_seq;
    h.click("A drawing");
    settle(&mut h);
    h.click("Open a project");
    settle(&mut h);
    assert!(h.rig().requests.is_empty(), "{:?}", h.rig().requests);
    assert_eq!(h.state().refusal_seq, seq + 2, "each is said no to");
    drop_file(&mut h, &file);
    assert_eq!(h.state().refusal_seq, seq + 3);
    assert_eq!(h.editor().leave, None);
    assert!(h.state().file_path.is_none() && is_open(&h));
}

#[test]
fn austart_open_recent_loads_a_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let files = recents(dir.path());
    let mut h = first_run();
    with_recents(&mut h, dir.path());
    assert!(h.has("Recent") && h.has(RECENT[0]) && h.has(RECENT[1]));
    h.click(RECENT[1]);
    settle(&mut h);
    let asked = h.rig().requests.last().cloned();
    assert_eq!(asked, Some(AppRequest::Start(WelcomeAction::OpenRecent(files[1].clone()))));
    h.answer_start(WelcomeAction::OpenRecent(files[1].clone()));
    settle(&mut h);
    assert_eq!(h.state().file_path.as_ref(), Some(&files[1]), "the file is open");
    assert!(!h.state().cut().drawings.is_empty(), "with its drawings");
    assert!(!is_open(&h));
    // One that has gone: said, and nothing else moves.
    let mut h = first_run();
    let group = h.state().untitled_group.clone();
    h.answer_start(WelcomeAction::OpenRecent(dir.path().join("gone.animproj")));
    settle(&mut h);
    assert_eq!(h.state().untitled_group, group, "the paper stands");
    assert!(is_open(&h));
    assert!(h.has_text_containing("could not open that project"));
    // More than two stay in the File menu; fewer show what there is.
    let many: Vec<_> = (0..5).map(|i| dir.path().join(format!("{i}.animproj"))).collect();
    for file in &many {
        anim_core::store::save(&sample::build_sample().engine.project, file).expect("saved");
    }
    let mut h = first_run();
    h.editor_mut().start = Some(Welcome::new(&many));
    settle(&mut h);
    assert!(h.has("0") && h.has("1") && !h.has("2"), "two rows");
    // Fresh install: hidden.
    let h = first_run();
    assert!(!h.has("Recent"));
}

#[test]
fn austart_more_sizes_holds_every_template_and_the_form() {
    let mut h = first_run();
    assert!(!h.has(templates::ALL[0].name), "folded");
    h.click("More sizes");
    settle(&mut h);
    for t in templates::ALL {
        assert!(h.has(t.name), "{}", t.name);
    }
    assert!(h.has("Custom size"));
    h.scroll_to("Custom size");
    h.click("Custom size");
    settle(&mut h);
    assert_eq!(h.rig().requests, [AppRequest::Start(WelcomeAction::Custom)]);
    let (stored, shown) = (h.editor().start.as_ref().map(|w| w.more_open), float_open(&h));
    assert_eq!(stored, Some(true));
    assert!(shown.height() <= 460.5, "its rows scroll inside the float");
}

#[test]
fn austart_more_sizes_rows_say_their_size() {
    let mut h = first_run();
    h.click("More sizes");
    settle(&mut h);
    let wide = h.rect("More sizes").right() - h.rect("Open a project").left();
    for t in &templates::ALL[..3] {
        h.scroll_to(t.name);
        settle(&mut h);
        let painted = h.painted_text();
        let size = format!("{}×{}", t.width, t.height);
        assert!(painted.contains(&size), "{}: {size} in {painted:?}", t.name);
        assert!(h.has(t.name) && !h.has(&size), "it speaks the name alone");
        assert_eq!(h.rect(t.name).width(), wide, "{}", t.name);
    }
    // Its name keeps the row's left, its note the right edge, in the dim ink.
    let name = &templates::ALL[0];
    let (row, at) = (h.rect(name.name), h.painted_at(name.name));
    let note = h.painted_at(&format!("{}×{}", name.width, name.height));
    assert_eq!(at.x, row.left() + plate::CARD_PAD);
    assert!(note.x > at.x + 40.0 && note.x < row.right() - plate::CARD_PAD, "{note:?} in {row:?}");
}

/// Opening `More sizes` scrolls the body, and moves nothing sideways: the
/// handle lives in the float's margin, and it still answers there.
#[test]
fn austart_more_sizes_moves_nothing_sideways() {
    let mut h = first_run();
    let names = ["A drawing", "Square", "Tall", "An animation", "A comic", "Open a project", "More sizes"];
    let before = names.map(|name| h.rect(name));
    let float = float_open(&h);
    h.click("More sizes");
    settle(&mut h);
    for (name, was) in names.into_iter().zip(before) {
        let now = h.rect(name);
        assert_eq!((now.left(), now.right()), (was.left(), was.right()), "{name}");
    }
    assert_eq!(float_open(&h).width(), float.width());
    let rest = plate::INK3.gamma_multiply(0.6);
    let handles = h.filled_rects(rest);
    let handle = *handles.iter().find(|r| float.contains_rect(**r)).expect("the handle at rest");
    assert_eq!(handle.width(), 4.0, "{handle:?}");
    assert!(handle.left() >= before[4].right() + 6.0, "clear of the cards: {handle:?}");
    assert!(handle.right() < float.right(), "inside the float: {handle:?}");
    let comic = h.rect("A comic");
    h.drag_path(&[handle.center(), handle.center() + vec2(0.0, 30.0), handle.center() + vec2(0.0, 60.0)]);
    settle(&mut h);
    assert!(h.rect("A comic").top() < comic.top(), "the handle scrolls the body");
}

#[test]
fn austart_intents_are_the_table() {
    let table: Vec<_> = templates::INTENTS.iter().map(|t| (t.name, t.width, t.height)).collect();
    let want = [
        ("Animation", 1280, 720),
        ("Picture", 1920, 1080),
        ("Square picture", 2048, 2048),
        ("Tall picture", 1080, 1920),
    ];
    assert_eq!(table, want);
    assert!(templates::INTENTS.len() >= 2);
    for t in templates::INTENTS {
        let rows = if t.name == "Animation" { 12 } else { 1 };
        assert_eq!((t.fps, t.cut_frames, t.room), (12, rows, Stage::Drawing), "{}", t.name);
        assert!(!t.sheet_on && t.page.is_none(), "{}", t.name);
        assert_eq!(t.columns.iter().map(|c| c.0).collect::<Vec<_>>(), ["A"], "{}", t.name);
    }
    assert_eq!(templates::INTENTS[templates::ANIMATION].name, "Animation", "the first card");
    // The ready paper is the first card's paper, and its own row: no stamp.
    let ready = &templates::READY;
    let card = &templates::INTENTS[templates::ANIMATION];
    assert_eq!((ready.width, ready.height, ready.fps, ready.cut_frames), (card.width, card.height, card.fps, 1));
    assert_eq!(ready.columns.iter().map(|c| c.0).collect::<Vec<_>>(), ["A"]);
    assert!(ready.kind.is_none() && card.kind.is_some());
    // Outside `ALL`, which stays what the New form lists.
    let names: Vec<_> = templates::ALL.iter().map(|t| t.name).collect();
    assert_eq!(names.len(), 7);
    assert!(templates::INTENTS.iter().all(|t| !names.contains(&t.name)));
    assert!(!names.contains(&ready.name) && templates::INTENTS.iter().all(|t| t.name != ready.name));
    assert_eq!(templates::SIZES.len(), 9);
    assert!(templates::ALL.iter().all(|t| t.fps == 24), "studio presets stay at 24");
    assert_eq!(templates::ALL[templates::COMIC].name, "Manga B5");
}

// ---- the budgets ----

#[test]
fn austart_float_budget_at_worst_case() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = first_run();
    let fresh = built_census(&mut h);
    let shown = float_census(&mut h);
    let tall = float_open(&h).height();
    println!("fresh install: {} controls, {} words, {tall} tall", fresh.controls, fresh.words);
    assert!(tall <= 300.5, "fresh install: {tall}");
    assert!(fresh.controls <= 12 && fresh.words <= 25, "fresh install: {fresh:?}");
    assert_eq!((shown.controls, shown.words), (fresh.controls, fresh.words), "all of it shows, on one count");
    // A fresh card's body fits: its last row stands in the float unscrolled.
    let last = h.rect("What's new");
    assert!(last.bottom() <= float_open(&h).bottom() - 12.0, "{last:?} in {:?}", float_open(&h));
    with_recents(&mut h, dir.path());
    let worst = built_census(&mut h);
    let shown = float_census(&mut h);
    assert!(worst.controls <= 14, "controls: {worst:?}");
    assert!(worst.words <= 30, "words: {worst:?}");
    assert!(worst.controls > fresh.controls && worst.words > fresh.words, "{fresh:?} {worst:?}");
    let tall = float_open(&h).height();
    println!("worst case: {} controls, {} words, {tall} tall", worst.controls, worst.words);
    println!("worst case, painted: {} controls, {} words", shown.controls, shown.words);
    assert!(tall <= 300.5, "worst case: {tall}");
    // Two recents pass the cap of spec 4.1: the foot is a scroll away, and the
    // head, with the way to close it and the latch, stands.
    h.scroll_to("What's new");
    settle(&mut h);
    let last = h.rect("What's new");
    assert!(last.bottom() <= float_open(&h).bottom() - 12.0, "{last:?} in {:?}", float_open(&h));
    assert!(float_open(&h).contains_rect(h.rect("Close Start")));
    assert!(float_open(&h).contains_rect(h.rect("Show at startup")));
}

#[test]
fn austart_first_screen_budget() {
    let mut h = first_run();
    let screen = measure::census(&mut h);
    println!("first screen: {} controls, {} words", screen.controls, screen.words);
    assert!(screen.controls <= 44, "controls: {} {screen:?}", screen.controls);
    assert!(screen.words <= 70, "words: {} {screen:?}", screen.words);
    // At most two solid blue things: the armed tool and the one primary.
    let blues = h.filled_rects(plate::BLUE);
    assert!(blues.len() <= 2, "solid blue: {blues:?}");
}

/// The first screen's text keeps one left edge: heading, cards, rows, the
/// Recent legend, and the latch's box.
#[test]
fn austart_words_stand_on_one_edge() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = first_run();
    with_recents(&mut h, dir.path());
    let edge = float_open(&h).left() + 12.0 + plate::CARD_PAD;
    for word in ["Start", "A drawing", "Follow a lesson", "Open a project", "Recent", RECENT[0]] {
        assert_eq!(h.painted_at(word).x, edge, "{word}");
    }
    // Each column keeps its own: a card's, a half row's.
    for card in ["An animation", "A comic"] {
        assert_eq!(h.painted_at(card).x, h.rect(card).left() + plate::CARD_PAD, "{card}");
    }
    let half = h.rect("More sizes").left() + plate::CARD_PAD;
    assert_eq!(h.painted_at("More sizes").x, half);
    assert_eq!(h.painted_at(RECENT[1]).x, half, "the recents stand on the half rows' edges");
    h.scroll_to("Check your pen");
    settle(&mut h);
    assert_eq!(h.painted_at("Check your pen").x, edge);
    assert_eq!(h.painted_at("What's new").x, half, "the two half rows share their edges");
    // On the other tab too, and in the answer to a check.
    let w = h.editor_mut().start.as_mut().expect("open");
    (w.pen, w.pen_said) = (PenCheck::Mouse, true);
    settle(&mut h);
    let said = h.rect(welcome::MOUSE_FIX);
    assert_eq!(said.left(), edge, "{said:?}");
    assert_eq!(h.painted_at(welcome::MOUSE_FIX).x, edge, "{said:?}");
    // That answer scrolled the body: What's New still opens at its own top.
    h.editor_mut().start.as_mut().expect("open").tab = WelcomeTab::WhatsNew;
    settle(&mut h);
    assert_eq!(h.painted_at(welcome::WHATS_NEW_BARS).x, edge);
}

/// The peg bar is the float's rule (spec 2.1): steel, one round peg hole, in
/// the head's 72 pt. No other float wears it.
#[test]
fn austart_rule_is_the_peg_bar() {
    let mut h = first_run();
    let float = float_open(&h);
    let inside = |rects: Vec<Rect>| rects.into_iter().filter(|r| float.contains_rect(*r)).collect::<Vec<_>>();
    let bars = inside(h.filled_rects(plate::STEEL));
    assert_eq!(bars.len(), 1, "{bars:?}");
    let bar = bars[0];
    assert_eq!(bar.width(), float.width() - 24.0, "margin to margin");
    let holes = h.filled_circles(plate::DESK);
    let holes: Vec<_> = holes.iter().filter(|(at, _)| bar.contains(*at)).collect();
    assert_eq!(holes.len(), 1, "one round peg hole: {holes:?}");
    assert_eq!(holes[0], &(bar.center(), 2.5));
    let head = h.rect("A drawing").top() - float.top() + 12.0;
    assert!(head <= 72.0, "spec 8.1's head block: {head}");
    assert!(float.top() + 12.0 < bar.top() && bar.bottom() <= h.rect("A drawing").top());
    // Any other float stands as it stood.
    h.click("Close Start");
    h.open_float(Float::Brushes);
    settle(&mut h);
    let area = h.ctx().memory(|m| m.area_rect(Id::new(("float", Float::Brushes.heading()))));
    let area = area.expect("the Brushes float");
    let steel = h.filled_rects(plate::STEEL);
    assert!(!steel.iter().any(|r| area.contains_rect(*r)), "{steel:?} in {area:?}");
}

#[test]
fn austart_paper_share_holds_once_the_float_closes() {
    const PAPER_FLOORS: [f32; 4] = [63.0, 66.5, 57.5, 60.0];
    const PEN_FLOORS: [f32; 4] = [82.5, 80.0, 77.5, 74.0];
    for (at, (w, tall)) in SIZES.into_iter().enumerate() {
        let mut h = EditorHarness::as_first_run();
        sized(&mut h, w, tall);
        h.click("Close Start");
        h.perform(Action::ZoomFit);
        settle(&mut h);
        let share = measure::paper_share(&h);
        assert!(share.paper >= PAPER_FLOORS[at], "paper at {w}×{tall}: {share:?}");
        assert!(share.pen_land >= PEN_FLOORS[at], "pen at {w}×{tall}: {share:?}");
        assert_eq!(share.paper_under_chrome, 0, "{w}×{tall}: {share:?}");
    }
    // The rail's whole sweep, from a first launch: strip closed and open,
    // every aspect, every stored scale, on the same floors.
    paper_share_sweep(|state| {
        let mut h = match state {
            Some(state) => EditorHarness::with_state_first_run(state),
            None => EditorHarness::as_first_run(),
        };
        h.click("Close Start");
        h
    });
}

// ---- the lock ----

#[test]
fn austart_fresh_install_locks_positions() {
    let (fresh, _) = Config::load_from(tempfile::tempdir().unwrap().path());
    assert!(fresh.ui.lock_positions, "a fresh install starts locked");
    assert!(!UiConfig::default().lock_positions, "the type's default stays");
    // A working animator's config keeps its value.
    let dir = tempfile::tempdir().unwrap();
    let mut kept = Config::default();
    kept.ui.lock_positions = false;
    kept.save_to(dir.path()).expect("saved");
    assert!(!Config::load_from(dir.path()).0.ui.lock_positions);
    // The frozen boot holds still.
    assert!(!EditorHarness::new().rig().config.ui.lock_positions);
    // The verb undoes it, and the shell stores it.
    assert_eq!(Action::ToggleLockPositions.menu(), Some("View/Lock UI positions"));
    let mut h = first_run();
    assert!(h.rig().config.ui.lock_positions);
    assert_eq!(h.editor().is_on(Action::ToggleLockPositions), Some(true));
    assert_eq!(h.perform(Action::ToggleLockPositions), Performed::Done);
    settle(&mut h);
    assert!(!h.rig().config.ui.lock_positions);
    assert_eq!(h.editor().is_on(Action::ToggleLockPositions), Some(false));
    assert!(!Config::load_from(h.data_dir()).0.ui.lock_positions, "saved");
}

#[test]
fn austart_locked_keeps_close_on_opened_panels() {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    assert!(h.rig().config.ui.lock_positions, "the fresh install's lock");
    h.editor_mut().open_pane(Pane::History);
    settle(&mut h);
    let panes = |h: &EditorHarness| -> Vec<Pane> { h.editor().dock.iter_all_tabs().map(|(_, p)| *p).collect() };
    assert_eq!(panes(&h), [Pane::Canvas, Pane::History]);
    crate::audock_tests::middle_click_tab(&mut h, Pane::Canvas);
    assert_eq!(panes(&h), [Pane::Canvas, Pane::History], "the Canvas never closes");
    crate::audock_tests::middle_click_tab(&mut h, Pane::History);
    assert_eq!(panes(&h), [Pane::Canvas], "a panel opened by hand still closes");
}

// ---- drop to open, the pen, the menus ----

/// A project file dropped on the window, in the next frame's input.
fn drop_file(h: &mut EditorHarness, path: &std::path::Path) {
    let file = egui::DroppedFile { path: Some(path.to_path_buf()), ..Default::default() };
    h.kit_mut().input_mut().dropped_files.push(file);
    settle(h);
}

#[test]
fn austart_drop_to_open_goes_through_leave() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Dropped.animproj");
    anim_core::store::save(&sample::build_sample().engine.project, &file).expect("saved");
    // Nothing to lose: it opens, and the float's job is done.
    let mut h = first_run();
    drop_file(&mut h, &file);
    assert_eq!(h.state().file_path.as_ref(), Some(&file));
    assert!(!is_open(&h));
    // A name in capitals is the same project.
    let shouted = dir.path().join("LOUD.ANIMPROJ");
    std::fs::copy(&file, &shouted).expect("copied");
    let mut h = first_run();
    drop_file(&mut h, &shouted);
    assert_eq!(h.state().file_path.as_ref(), Some(&shouted));
    // Work in the way: the guard asks, and nothing has moved.
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    let at = paper_middle(&h);
    line_at(&mut h, at);
    drop_file(&mut h, &file);
    assert_eq!(h.editor().leave, Some(Leave::OpenPath(file.clone())));
    assert!(h.state().file_path.is_none());
    // That question stands: a second drop waits for its answer.
    let seq = h.state().refusal_seq;
    drop_file(&mut h, &shouted);
    assert_eq!(h.editor().leave, Some(Leave::OpenPath(file.clone())), "the first one's");
    assert_eq!(h.state().refusal_seq, seq + 1, "said no to");
    // What is no project is refused in plain words.
    let mut h = first_run();
    let seq = h.state().refusal_seq;
    drop_file(&mut h, &dir.path().join("picture.png"));
    assert_ne!(h.state().refusal_seq, seq);
    assert!(h.state().file_path.is_none() && is_open(&h));
    // The shell's own questions, the recovery offer and the New form, stand
    // the same way. The App feeds this; the Rig has neither.
    let mut h = first_run();
    h.editor_mut().shell_asks = true;
    let seq = h.state().refusal_seq;
    drop_file(&mut h, &file);
    assert!(h.state().file_path.is_none() && is_open(&h), "nothing opened behind it");
    assert_eq!(h.state().refusal_seq, seq + 1);
    h.editor_mut().shell_asks = false;
    drop_file(&mut h, &file);
    assert_eq!(h.state().file_path.as_ref(), Some(&file), "and it opens once answered");
}

fn pen_of(h: &EditorHarness) -> Option<PenCheck> {
    h.editor().start.as_ref().map(|w| w.pen)
}

/// A pen line with force, leftward from `from`.
fn pen_line(h: &mut EditorHarness, from: Pos2) {
    h.pen_down(from);
    h.pen_move(from + vec2(-30.0, 0.0));
    h.pen_move(from + vec2(-60.0, 5.0));
    h.pen_up(from + vec2(-60.0, 5.0));
    settle(h);
}

/// A tablet's line whose samples never carry force.
fn flat_line(h: &mut EditorHarness, from: Pos2) {
    h.pen_down(from);
    h.pen_move_flat(from + vec2(-30.0, 0.0));
    h.pen_move_flat(from + vec2(-60.0, 5.0));
    h.pen_up(from + vec2(-60.0, 5.0));
    settle(h);
}

#[test]
fn austart_check_your_pen_reads_every_transition() {
    use PenCheck::{Idle, Mouse, NoPressure, Waiting, Works};
    // A stroke with no check under way, or after a good answer, ends the float.
    for stood in [Idle, Works] {
        for heard in [Waiting, Works, NoPressure, Mouse] {
            assert_eq!(stood.read(heard), None, "{stood:?} heard {heard:?}");
        }
    }
    // Asked, or still without pressure: the line is read, and a tap says nothing.
    for stood in [Waiting, NoPressure, Mouse] {
        assert_eq!(stood.read(Works), Some(Works), "{stood:?}");
        assert_eq!(stood.read(NoPressure), Some(NoPressure), "{stood:?}");
        assert_eq!(stood.read(Waiting), Some(stood), "{stood:?}");
    }
    assert_eq!(Waiting.read(Mouse), Some(Mouse));
    assert_eq!(NoPressure.read(Mouse), Some(Mouse));
    assert_eq!(Mouse.read(Mouse), None, "you can still draw: the second mouse line is a real one");
    // Every answer is words and a mark, and every one of them names the fix.
    assert_eq!(Works.verdict(), Some((true, "Pressure works", None)));
    for found in [NoPressure, Mouse] {
        let (yes, said, fix) = found.verdict().expect("an answer");
        assert!(!yes && said == "No pressure" && fix.is_some_and(|f| f.contains("Windows Ink")), "{found:?}");
    }
    assert!(welcome::MOUSE_FIX.starts_with("A mouse has no pressure. You can still draw."));
}

#[test]
fn austart_check_your_pen_reports() {
    let mut h = first_run();
    h.click("Check your pen");
    settle(&mut h);
    assert!(h.has("Draw one line"), "it asks for the line");
    let at = free_paper(&h);
    pen_line(&mut h, at);
    assert_eq!(h.state().strokes_done, 1);
    assert_eq!(pen_of(&h), Some(PenCheck::Works));
    assert!(is_open(&h) && h.has("Pressure works"), "the answer stays to be read");
    assert_eq!(strokes(&h), 0, "the check's line came off the paper");
    // The check is over: the next line is the first one that counts. It is
    // the pen's: the canvas takes the mouse for the pen's echo from here on.
    pen_line(&mut h, at + vec2(-60.0, 60.0));
    assert_eq!(h.state().strokes_done, 2);
    assert!(!is_open(&h));
    assert_eq!(strokes(&h), 1, "that one stays");
    // A mouse carries no force: the fix, which names Windows Ink for the
    // tablet a mouse may be, and the word that a mouse can still draw.
    let mut h = first_run();
    h.click("Check your pen");
    settle(&mut h);
    let at = free_paper(&h);
    line_at(&mut h, at);
    assert_eq!(pen_of(&h), Some(PenCheck::Mouse));
    assert!(h.has("No pressure") && h.has_text_containing("Windows Ink"));
    assert!(h.has(welcome::MOUSE_FIX), "{}", h.describe());
    assert!(float_open(&h).height() <= 300.5);
    // Asked again, it reads the next line.
    h.click("Check your pen");
    settle(&mut h);
    assert_eq!(pen_of(&h), Some(PenCheck::Waiting));
    // At the worst case the fix lands below the fold: the body goes to it.
    let dir = tempfile::tempdir().unwrap();
    let mut h = EditorHarness::as_first_run();
    sized(&mut h, 1280.0, 800.0);
    with_recents(&mut h, dir.path());
    h.scroll_to("Check your pen");
    h.click("Check your pen");
    settle(&mut h);
    let at = free_paper(&h);
    line_at(&mut h, at);
    let (fix, float) = (h.rect(welcome::MOUSE_FIX), float_open(&h));
    assert!(fix.bottom() <= float.bottom(), "the fix is read whole: {fix:?} in {float:?}");
    // A tablet that reports flat is told the same fix, without the mouse.
    let mut h = first_run();
    h.click("Check your pen");
    settle(&mut h);
    let at = free_paper(&h);
    flat_line(&mut h, at);
    assert_eq!(pen_of(&h), Some(PenCheck::NoPressure));
    assert!(h.has("No pressure") && h.has(welcome::PEN_FIX), "{}", h.describe());
    assert!(welcome::PEN_FIX.contains("Windows Ink"));
}

/// `No pressure` keeps listening: the pen, fixed, answers without another
/// press, and the line after a good answer is the one that ends the float.
#[test]
fn austart_check_your_pen_keeps_listening_after_no_pressure() {
    for flat in [false, true] {
        let mut h = first_run();
        h.click("Check your pen");
        settle(&mut h);
        let at = free_paper(&h);
        if flat {
            flat_line(&mut h, at);
            flat_line(&mut h, at + vec2(0.0, 30.0));
            assert_eq!(pen_of(&h), Some(PenCheck::NoPressure), "still flat: it says so");
        } else {
            line_at(&mut h, at);
        }
        assert!(h.has("No pressure") && is_open(&h));
        pen_line(&mut h, at + vec2(0.0, 60.0));
        assert_eq!(pen_of(&h), Some(PenCheck::Works), "flat {flat}: read without a press");
        assert!(is_open(&h) && h.has("Pressure works") && !h.has("No pressure"));
        assert_eq!(strokes(&h), 0, "flat {flat}: every line was the check's");
        pen_line(&mut h, at + vec2(0.0, 90.0));
        assert!(!is_open(&h), "flat {flat}");
        assert_eq!(strokes(&h), 1, "flat {flat}");
    }
    // A mouse that goes on drawing is drawing: it was told it could.
    let mut h = first_run();
    h.click("Check your pen");
    settle(&mut h);
    let at = free_paper(&h);
    line_at(&mut h, at);
    assert_eq!((pen_of(&h), strokes(&h)), (Some(PenCheck::Mouse), 0));
    line_at(&mut h, at + vec2(0.0, 40.0));
    assert!(!is_open(&h), "the float makes way");
    assert_eq!(strokes(&h), 1, "and the line stays");
}

/// A tap gives no Move sample, so it says nothing of the pen. The Rig's paint
/// has no raster layer: the CPU route lands the dab a tap commits there.
#[test]
fn austart_a_pen_tap_keeps_asking() {
    let mut h = first_run();
    h.click("Check your pen");
    settle(&mut h);
    let at = free_paper(&h);
    h.pen_tap(at);
    settle(&mut h);
    assert_eq!(h.editor().canvas.pen_verdict(), PenCheck::Waiting);
    assert_eq!(pen_of(&h), Some(PenCheck::Waiting), "a vector tap commits nothing");
    paint_texels(h.state_mut(), &[(10, 10)], [1.0, 0.0, 0.0, 1.0]);
    settle(&mut h);
    assert_eq!(h.state().strokes_done, 1);
    assert_eq!(pen_of(&h), Some(PenCheck::Waiting), "a dab is a stroke, and says nothing either");
    assert!(h.has("Draw one line") && !h.has("No pressure"));
    assert!(!h.state().is_dirty(), "and it leaves nothing behind");
    // The line after it answers.
    pen_line(&mut h, at);
    assert_eq!(pen_of(&h), Some(PenCheck::Works));
}

/// Both stroke routes answer (spec 1.5 rule 11): the raster tap and the raster
/// line, on a real device.
#[test]
#[ignore = "needs a GPU adapter"]
fn austart_check_your_pen_reads_the_raster_route() {
    let mut h = EditorHarness::with_gpu_first_run();
    h.run_frames(4);
    assert!(h.editor().canvas.raster, "the paint layer is live");
    h.click("Check your pen");
    h.run_frames(4);
    let at = free_paper(&h);
    h.pen_tap(at);
    h.run_frames(8);
    assert_eq!(h.state().strokes_done, 1, "{}", h.state().status);
    assert_eq!(pen_of(&h), Some(PenCheck::Waiting), "the tap keeps asking");
    assert!(!h.state().is_dirty(), "and its dab came off");
    pen_line(&mut h, at);
    h.run_frames(8);
    assert_eq!(h.state().strokes_done, 2, "{}", h.state().status);
    assert_eq!(pen_of(&h), Some(PenCheck::Works));
    assert!(h.has("Pressure works") && is_open(&h));
    assert!(!h.state().is_dirty(), "the check's line came off");
    // The answer is in, so the next line is drawing.
    pen_line(&mut h, at + vec2(0.0, 60.0));
    h.run_frames(8);
    assert_eq!(h.state().strokes_done, 3, "{}", h.state().status);
    assert!(!is_open(&h) && h.state().is_dirty(), "a real line stays");
}

/// The check leaves nothing to save: its line is off the paper, so a card
/// after it replaces the blank without a question.
#[test]
fn austart_a_card_after_a_pen_check_asks_nothing() {
    let mut h = first_run();
    h.click("Check your pen");
    settle(&mut h);
    let at = free_paper(&h);
    pen_line(&mut h, at);
    assert_eq!(pen_of(&h), Some(PenCheck::Works));
    assert!(!h.state().is_dirty(), "the blank is clean again");
    h.scroll_to("A drawing");
    h.click("A drawing");
    settle(&mut h);
    assert_eq!(h.editor().leave, None, "nothing to ask about");
    assert!(!h.has("Save changes?"));
    assert_eq!(h.rig().requests, [AppRequest::Start(WelcomeAction::Intent(templates::PICTURE))]);
    // Work that was there before the check stays, and is still asked about.
    let mut h = first_run();
    h.click("Close Start");
    settle(&mut h);
    let at = paper_middle(&h);
    line_at(&mut h, at);
    assert_eq!(strokes(&h), 1);
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    h.click("Check your pen");
    settle(&mut h);
    let at = free_paper(&h);
    line_at(&mut h, at);
    assert_eq!(strokes(&h), 1, "the check's line alone came off");
    h.scroll_to("A drawing");
    h.click("A drawing");
    settle(&mut h);
    assert!(h.has("Save changes?"), "the work before the check is unsaved");
}

/// The check says where it stands: the row on the wash while it listens, and
/// an answer that is a mark and words, no control.
#[test]
fn austart_check_your_pen_shows_its_state() {
    let mut h = first_run();
    let idle = float_census(&mut h);
    let row = h.rect("Check your pen");
    assert!(!h.filled_rects(plate::BLUE_WASH).contains(&row));
    h.click("Check your pen");
    settle(&mut h);
    assert_eq!(h.rect("Draw one line"), row);
    assert!(h.filled_rects(plate::BLUE_WASH).contains(&row), "the current row's wash");
    let at = free_paper(&h);
    pen_line(&mut h, at);
    // The answer's line takes the body past the cap: the same row, scrolled by it.
    let back = h.rect("Check your pen");
    assert_eq!((back.left(), back.size()), (row.left(), row.size()), "the row is back, and asks again");
    assert!(float_open(&h).contains_rect(back), "{back:?}");
    assert!(h.painted_text().contains(&"Pressure works".to_string()));
    let answered = float_census(&mut h);
    assert_eq!(answered.controls, idle.controls, "an answer is no control");
    h.click("Pressure works");
    settle(&mut h);
    assert_eq!(pen_of(&h), Some(PenCheck::Works), "and pressing it does nothing");
    h.click("Check your pen");
    settle(&mut h);
    assert_eq!(pen_of(&h), Some(PenCheck::Waiting));
}

/// With no canvas on screen the float claims no pane: the keys are the
/// focused pane's, under the pointer's float as anywhere else.
#[test]
fn austart_over_no_canvas_the_focused_pane_has_the_keys() {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    h.editor_mut().enter_room(Stage::Edit);
    h.editor_mut().open_pane(Pane::XSheet);
    settle(&mut h);
    assert!(!crate::visible_panes(&h.editor().dock).contains(&Pane::Canvas), "Timing's Viewer is front");
    h.editor_mut().open_start(Welcome::new(&[]));
    settle(&mut h);
    let on = float_open(&h).center();
    h.point_at(on);
    settle(&mut h);
    let ctx = h.ctx();
    assert_eq!(h.editor_mut().shortcut_ctx(&ctx), Ctx::Sheet);
    let from = h.performed().len();
    h.key_mods(CTRL, Key::V);
    let fired: Vec<_> = h.performed()[from..].iter().map(|(a, _)| *a).collect();
    assert!(fired.contains(&Action::PasteCells), "{fired:?}");
    assert!(!fired.contains(&Action::PastePixels), "{fired:?}");
    h.rig_mut().config.set_keymap(Keymap::Industry);
    settle(&mut h);
    let from = h.performed().len();
    h.key(Key::Delete);
    let fired: Vec<_> = h.performed()[from..].iter().map(|(a, _)| *a).collect();
    assert!(!fired.contains(&Action::ClearCel), "Delete clears no cel behind the float: {fired:?}");
    assert!(is_open(&h));
    // Over the canvas it is the canvas's, as it was.
    let mut h = first_run();
    let on = float_open(&h).center();
    h.point_at(on);
    settle(&mut h);
    let ctx = h.ctx();
    assert_eq!(h.editor_mut().shortcut_ctx(&ctx), Ctx::Canvas);
}

/// What this part stores: nothing. One Action joins, with no chord; the stored
/// words stand; and going through every page moves neither the project, its
/// file nor the config. The latch alone writes, the one key it always wrote.
#[test]
fn austart_saved_data_stands() {
    let rooms = [Stage::Layout, Stage::Drawing, Stage::InkPaint, Stage::Composite, Stage::Edit, Stage::Manga];
    let keys = rooms.map(|room| crate::stage_key(Some(room)));
    assert_eq!(keys, ["layout", "drawing", "finishing", "composite", "edit", "manga"]);
    let ui = serde_json::to_value(UiConfig::default()).unwrap();
    let mut keys: Vec<_> = ui.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    let shape = [
        "exact_descriptions", "hints_off", "hints_seen", "keymap_rev", "lock_positions", "quick_start",
        "result_in_color", "scale", "sheet_in_draw", "show_welcome", "whats_new_seen",
    ];
    assert_eq!(keys, shape, "the ui section's shape");
    let store = include_str!("../../crates/anim-core/src/store.rs");
    assert!(store.contains("const SCHEMA_VERSION: i64 = 11;"), "the .animproj format stands");
    assert!(Action::ALL.contains(&Action::OpenLessons));
    for keymap in [Keymap::Gaban, Keymap::Industry] {
        let binds = crate::config::default_binds(keymap);
        assert_eq!(crate::cmdpal::chord_of(&binds, Action::OpenLessons), None, "no chord, so no keymap moves");
    }
    let mut h = first_run();
    let config = serde_json::to_value(&h.rig().config).unwrap();
    let project = h.state_mut().snapshot_project();
    stand_in(&mut h, WelcomeTab::Start);
    h.click("More sizes");
    h.click("More sizes");
    h.click("Follow a lesson");
    settle(&mut h);
    h.click("Back to Start");
    settle(&mut h);
    h.click("What's new");
    settle(&mut h);
    h.click("Back to Start");
    settle(&mut h);
    let after = h.state_mut().snapshot_project();
    assert!(after == project, "the project did not move");
    assert!(after.app_meta.keys().all(|key| !key.contains("lesson")), "{:?}", after.app_meta);
    assert_eq!(serde_json::to_value(&h.rig().config).unwrap(), config, "nor the config");
    let dir = h.data_dir().to_path_buf();
    let (was, now) = (dir.join("was.animproj"), dir.join("now.animproj"));
    anim_core::store::save(&project, &was).expect("saved");
    anim_core::store::save(&after, &now).expect("saved");
    assert_eq!(std::fs::read(&was).unwrap(), std::fs::read(&now).unwrap(), "the same file");
    h.click("Show at startup");
    settle(&mut h);
    let mut want = config;
    want["ui"]["show_welcome"] = false.into();
    assert_eq!(serde_json::to_value(&h.rig().config).unwrap(), want, "the latch's one key");
}

/// The float's cost: no window, no shader, one branch while it is closed, and
/// while open no look at the disk: its rows are what it read when it opened.
#[test]
fn austart_float_opens_no_window_and_adds_no_shader() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = first_run();
    with_recents(&mut h, dir.path());
    for tab in [WelcomeTab::Start, WelcomeTab::Lessons, WelcomeTab::WhatsNew] {
        let windows = &h.kit().output().viewport_output;
        assert!(windows.len() == 1 && windows.contains_key(&egui::ViewportId::ROOT), "{tab:?}");
        stand_in(&mut h, tab);
    }
    with_recents(&mut h, dir.path());
    let doors = [
        concat!("show_", "viewport"),
        concat!("Viewport", "Builder"),
        concat!("rfd", "::"),
        concat!("create_shader_", "module"),
        concat!(".w", "gsl"),
    ];
    let (body, frame) = (include_str!("welcome.rs"), include_str!("floats.rs"));
    for text in [body, frame, include_str!("lessons.rs")] {
        let found: Vec<_> = doors.iter().filter(|door| text.contains(**door)).collect();
        assert!(found.is_empty(), "{found:?}");
    }
    let main = include_str!("main.rs");
    let from = main.find("fn start_ui(").expect("the float's frame");
    let closed = main[from..].find("self.start.take() else { return }").expect("its early way out");
    let dock = main[from..].find("visible_panes(").expect("what it reads when open");
    assert!(closed < dock, "closed, it costs a branch");
    let from = body.find("pub fn float_body(").expect("its body");
    let to = body.find("/// The Welcome screen, drawn into").expect("the end of its rows");
    assert!(from < to);
    for disk in [concat!("std::", "fs"), concat!(".is_", "file"), concat!(".exi", "sts("), "read_meta"] {
        assert!(!body[from..to].contains(disk), "{disk}");
    }
    std::fs::remove_file(dir.path().join(format!("{}.animproj", RECENT[0]))).expect("removed");
    settle(&mut h);
    assert!(h.has(RECENT[0]), "the rows stand as they were read");
}

#[test]
fn austart_menu_rows_open_the_float_with_no_modal() {
    let mut s = sample::build_sample();
    s.saved_rev = s.engine.revision();
    let mut h = EditorHarness::with_state_shipped(s);
    sized(&mut h, 1600.0, 900.0);
    assert!(!is_open(&h), "the shipped boot is a fresh install with it closed");
    assert_eq!(Action::WelcomeScreen.menu(), Some("Help/Start screen…"));
    h.perform(Action::WelcomeScreen);
    settle(&mut h);
    assert!(is_open(&h) && float_rect(&h).is_some());
    assert!(h.ctx().memory(|m| m.top_modal_layer()).is_none(), "no Modal");
    let shown = float_open(&h);
    assert!((shown.center().x - dock(&h).center().x).abs() <= 1.0, "{shown:?}");
    h.click("Close Start");
    settle(&mut h);
    // What's New: the same float, on its tab.
    assert_eq!(Action::WhatsNew.menu(), Some("Help/What's New"));
    assert_eq!(h.perform(Action::WhatsNew), Performed::Done);
    settle(&mut h);
    assert_eq!(h.editor().start.as_ref().map(|w| w.tab), Some(WelcomeTab::WhatsNew));
    h.click("Back to Start");
    settle(&mut h);
    assert!(h.has("A drawing"));
    h.click("What's new");
    settle(&mut h);
    assert_eq!(h.editor().start.as_ref().map(|w| w.tab), Some(WelcomeTab::WhatsNew));
    assert!(h.has(welcome::WHATS_NEW_HEADING) && !h.has("Close Start"), "the page says what it is");
    assert!(!h.has("Show at startup"), "the latch is the Start page's");
    h.click(&format!("Close {}", welcome::WHATS_NEW_HEADING));
    settle(&mut h);
    // Over unsaved work the row still asks nothing. A card that replaces the
    // cut does, once.
    h.state_mut().add_column();
    assert!(h.state().is_dirty());
    assert_eq!(h.perform(Action::WelcomeScreen), Performed::Done);
    settle(&mut h);
    assert!(is_open(&h), "it opens at once");
    assert_eq!(h.editor().leave, None);
    assert!(h.ctx().memory(|m| m.top_modal_layer()).is_none(), "no Modal over dirty work");
    h.click("A drawing");
    settle(&mut h);
    assert!(h.has("Save changes?"), "the card asks");
    assert_eq!(h.editor().leave, Some(Leave::Start(WelcomeAction::Intent(templates::PICTURE))));
    assert!(h.ctx().memory(|m| m.top_modal_layer()).is_some());
    h.hold("Don't Save");
    settle(&mut h);
    assert!(!h.has("Save changes?") && h.editor().leave.is_none(), "once");
    // An open dialog is the shell's to answer; the App's gate reads the old
    // screen's field, which nothing sets now.
    let source = include_str!("main.rs");
    assert!(!source.contains("self.welcome = Some("), "the old screen never comes back");
}

#[test]
fn austart_upgrader_opens_once_on_whats_new() {
    let dir = tempfile::tempdir().unwrap();
    let mut old = Config::default();
    old.ui.whats_new_seen = "0.1".into();
    old.save_to(dir.path()).expect("saved");
    let mut h = EditorHarness::with_dir_first_run(dir);
    sized(&mut h, 1600.0, 900.0);
    assert_eq!(h.editor().start.as_ref().map(|w| w.tab), Some(WelcomeTab::WhatsNew));
    assert_eq!(h.rig().config.ui.whats_new_seen, welcome::VERSION, "once");
    assert!((float_open(&h).center().x - paper_middle(&h).x).abs() <= 1.0, "the same paper and float");
    assert_eq!(h.state().fps(), 12);
    // A fresh install has nothing new to say.
    let h = first_run();
    assert_eq!(h.editor().start.as_ref().map(|w| w.tab), Some(WelcomeTab::Start));
}
