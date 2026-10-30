//! GM18: the Manga room's tool pane — the book's pages, the panel (koma)
//! tool, screen tone (tōn) and the perspective rulers (pāsu jōgi). Every
//! edit is one undo step through the engine (`AppState::manga_edit`) and is
//! refused while a stroke is live; New page and Clip to panels go through
//! the one dispatch (D6). The rulers' snap is the pen's: the canvas reads
//! it each frame and holds a stroke to it before the stabilizer.

use anim_core::command::{Command, CutRef};
use anim_core::fx::{FxKind, FxValue, FxValues};
use anim_core::graph::NodeKind;
use anim_core::ids::{CutId, SceneId};
use anim_core::manga::{self, Page, Poly, Rulers};
use anim_core::model::Cut;
use eframe::egui;

use crate::config::Action;
use crate::doc::AppState;
use crate::plate;

/// The pane's session state (the view's, not the file's): the panel the
/// tools act on, the panel tool's gutters and the rulers' snap.
pub struct MangaState {
    /// The selected panel, an index into the page's panels.
    pub selected: usize,
    /// The gutter between panels side by side, and between rows, mm.
    pub side_mm: f32,
    pub row_mm: f32,
    /// The pen holds to the page's rulers (when it has any).
    pub snap: bool,
    /// Verbs the pane asked for, run through the one dispatch (D6).
    pub asked: Vec<Action>,
}

impl Default for MangaState {
    fn default() -> Self {
        Self {
            selected: 0,
            side_mm: manga::GUTTER_SIDE_MM,
            row_mm: manga::GUTTER_ROW_MM,
            snap: true,
            asked: Vec::new(),
        }
    }
}

impl MangaState {
    /// The rulers the pen holds to on `cut`: its own while the snap is on
    /// (None: the snap is off, or the cut has no vanishing point).
    pub fn snap_rulers(&self, cut: &Cut) -> Option<Rulers> {
        if !self.snap {
            return None;
        }
        let rulers = Rulers::of(cut);
        (!rulers.vps.is_empty()).then_some(rulers)
    }
}

/// The panel tool's edits of the selected panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelOp {
    /// Cut it through its middle into two rows, the row gutter between.
    SplitAcross,
    /// Cut it through its middle into two side by side, the side gutter
    /// between.
    SplitDown,
    /// A new panel: the page's inner frame.
    Add,
    Delete,
}

impl PanelOp {
    pub const ALL: [PanelOp; 4] = [Self::SplitAcross, Self::SplitDown, Self::Add, Self::Delete];

    pub fn label(self) -> &'static str {
        match self {
            Self::SplitAcross => "Split across",
            Self::SplitDown => "Split down",
            Self::Add => "Add panel",
            Self::Delete => "Delete panel",
        }
    }

    fn help(self) -> &'static str {
        match self {
            Self::SplitAcross => "Cut the selected panel into two rows, the row gutter between them.",
            Self::SplitDown => "Cut the selected panel into two side by side, the side gutter between them.",
            Self::Add => "Add a panel the size of the page's inner frame.",
            Self::Delete => "Delete the selected panel (its art is clipped away, not erased).",
        }
    }
}

/// `op` on panel `i` of `polys` on `page`, gutters `side_mm` and `row_mm`:
/// the panels after it and the one then selected (None: nothing to do —
/// no such panel, or one too small to split).
pub fn panel_op(
    polys: &[Poly],
    i: usize,
    op: PanelOp,
    page: &Page,
    side_mm: f32,
    row_mm: f32,
) -> Option<(Vec<Poly>, usize)> {
    let mut out = polys.to_vec();
    match op {
        PanelOp::SplitAcross | PanelOp::SplitDown => {
            let poly = polys.get(i)?;
            let (a, b) = middle(poly, op == PanelOp::SplitAcross);
            let gutter = manga::gutter_for(a, b, side_mm, row_mm, page.dpi);
            let (first, second) = manga::split_panel(poly, a, b, gutter)?;
            out[i] = first;
            out.insert(i + 1, second);
            Some((out, i))
        }
        PanelOp::Add => {
            out.push(page.first_panel());
            let last = out.len() - 1;
            Some((out, last))
        }
        PanelOp::Delete => {
            (i < out.len()).then_some(())?;
            out.remove(i);
            let sel = i.min(out.len().saturating_sub(1));
            Some((out, sel))
        }
    }
}

/// The outline's bounds: [x0, y0, x1, y1].
fn bounds(poly: &[[f32; 2]]) -> [f32; 4] {
    poly.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| {
        [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]
    })
}

/// The line through the middle of `poly`'s bounds, on a whole px: across
/// the page (a row cut) or down it.
fn middle(poly: &[[f32; 2]], across: bool) -> ([f32; 2], [f32; 2]) {
    let [x0, y0, x1, y1] = bounds(poly);
    let (cx, cy) = (((x0 + x1) / 2.0).round(), ((y0 + y1) / 2.0).round());
    if across {
        ([x0 - 1.0, cy], [x1 + 1.0, cy])
    } else {
        ([cx, y0 - 1.0], [cx, y1 + 1.0])
    }
}

/// The tone presets: line counts, screen angles and dot shapes.
const LPIS: [(f32, &str); 3] = [(42.5, "42.5 lpi"), (60.0, "60 lpi"), (85.0, "85 lpi")];
const ANGLES: [(f32, &str); 2] = [(0.0, "screen 0°"), (45.0, "screen 45°")];
const SHAPES: [(&str, &str); 3] = [("round", "round dots"), ("square", "square dots"), ("line", "lines")];
/// The ruler presets, by their vanishing points' count.
const RULERS: [&str; 4] = ["no rulers", "1-point", "2-point", "3-point"];

/// The page's first Tone as it resolves (None: the page has no tone).
fn tone_now(cut: &Cut) -> Option<FxValues> {
    cut.graph.nodes().find_map(|n| match &n.kind {
        NodeKind::Effect {
            fx: FxKind::Tone,
            params,
            binds,
        } => Some(FxValues::resolve(FxKind::Tone, params, binds, |_| None)),
        _ => None,
    })
}

/// Every Tone on the page takes `key` = `v`: a `SetNodeKind` for each one
/// that holds another value, all in one step (none: they all hold it).
pub fn set_tone(cut: &Cut, at: CutRef, key: &str, v: &FxValue) -> Vec<Command> {
    let Some(i) = FxKind::Tone.specs().iter().position(|s| s.key == key) else {
        return Vec::new();
    };
    cut.graph
        .nodes()
        .filter_map(|n| {
            let NodeKind::Effect {
                fx: FxKind::Tone,
                params,
                binds,
            } = &n.kind
            else {
                return None;
            };
            let now = FxValues::resolve(FxKind::Tone, params, binds, |_| None);
            if now.values.get(i) == Some(v) {
                return None;
            }
            let mut kind = n.kind.clone();
            if let NodeKind::Effect { params, .. } = &mut kind {
                params.set(key, v.clone());
            }
            Some(Command::SetNodeKind { at, id: n.id, kind })
        })
        .collect()
}

/// What the pane's pass asked for, applied after it.
enum Edit {
    Goto(SceneId, CutId),
    Panels(PanelOp),
    Tone(&'static str, FxValue),
    Rulers(usize),
}

/// `gesture_safe`: no stroke is live (an edit is refused while one is).
pub fn ui(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool) {
    let mut edit = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| pane(ui, state, &mut edit));
    let Some(edit) = edit else {
        return;
    };
    if !gesture_safe {
        state.refuse("refused — finish the stroke first");
        return;
    }
    match edit {
        Edit::Goto(scene, cut) => state.goto_cut(scene, cut),
        Edit::Panels(op) => panels(state, op),
        Edit::Tone(key, v) => {
            if tone_now(state.cut()).is_none() {
                state.refuse("refused — the page has no tone (a TONE column, then Clip to panels)");
            } else {
                state.manga_edit("tone", |cut, at| set_tone(cut, at, key, &v));
            }
        }
        Edit::Rulers(points) => {
            let (w, h) = (state.engine.project.width as f32, state.engine.project.height as f32);
            let rulers = Rulers::preset(points, w, h);
            state.manga_edit("perspective rulers", |cut, at| rulers.set(cut, at).into_iter().collect());
        }
    }
}

fn pane(ui: &mut egui::Ui, state: &mut AppState, edit: &mut Option<Edit>) {
    plate::legend(ui, "manga (pages)");
    let page = Page::of(state.cut());
    match &page {
        Some(p) => {
            plate::value(ui, &format!("{} · {}×{} at {} dpi", p.format, p.width, p.height, p.dpi));
        }
        None => {
            ui.label(egui::RichText::new("this cut is no page — New page makes one").color(plate::legend_dim()));
        }
    }

    ui.add_space(6.0);
    plate::legend(ui, "pages (pēji)");
    let scene = state.at().scene;
    let pages: Vec<(CutId, String)> = state
        .engine
        .project
        .scenes
        .iter()
        .filter(|s| s.id == scene)
        .flat_map(|s| s.cuts.iter())
        .filter(|c| Page::of(c).is_some())
        .map(|c| (c.id, c.name.clone()))
        .collect();
    ui.horizontal_wrapped(|ui| {
        for (id, name) in &pages {
            if plate::detent(ui, *id == state.view.cut, name).clicked() && *id != state.view.cut {
                *edit = Some(Edit::Goto(scene, *id));
            }
        }
    });
    if ui.button("New page").on_hover_text(Action::NewPage.help()).clicked() {
        state.manga.asked.push(Action::NewPage);
    }

    ui.add_space(6.0);
    plate::legend(ui, "panels (koma)");
    let panels = manga::panels_of(state.cut()).map(|(_, p)| p).unwrap_or_default();
    if state.manga.selected >= panels.len() {
        state.manga.selected = panels.len().saturating_sub(1);
    }
    if let Some(p) = &page {
        thumbnail(ui, p, &panels, &mut state.manga.selected);
    }
    ui.horizontal_wrapped(|ui| {
        for (i, poly) in panels.iter().enumerate() {
            let [x0, y0, x1, y1] = bounds(poly);
            let name = format!("panel {}", i + 1);
            let r = plate::detent(ui, i == state.manga.selected, &name)
                .on_hover_text(format!("{:.0}×{:.0} px", x1 - x0, y1 - y0));
            if r.clicked() {
                state.manga.selected = i;
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for op in PanelOp::ALL {
            if ui.button(op.label()).on_hover_text(op.help()).clicked() {
                *edit = Some(Edit::Panels(op));
            }
        }
    });
    ui.horizontal(|ui| {
        plate::legend(ui, "gutter side");
        plate::field(ui, egui::DragValue::new(&mut state.manga.side_mm).range(0.0..=20.0).speed(0.1).suffix(" mm"));
        plate::legend(ui, "rows");
        plate::field(ui, egui::DragValue::new(&mut state.manga.row_mm).range(0.0..=20.0).speed(0.1).suffix(" mm"));
    });
    if ui.button("Clip to panels").on_hover_text(Action::ClipToPanels.help()).clicked() {
        state.manga.asked.push(Action::ClipToPanels);
    }

    ui.add_space(6.0);
    plate::legend(ui, "tone (tōn)");
    let tone = tone_now(state.cut());
    if tone.is_none() {
        ui.label(egui::RichText::new("no tone yet — a TONE column, then Clip to panels").color(plate::legend_dim()));
    }
    let lpi = tone.as_ref().map(|v| v.float("lpi"));
    let angle = tone.as_ref().map(|v| v.float("angle"));
    let shape = tone.as_ref().map(|v| v.choice("shape"));
    ui.horizontal_wrapped(|ui| {
        for (x, label) in LPIS {
            if plate::detent(ui, lpi == Some(x), label).clicked() {
                *edit = Some(Edit::Tone("lpi", FxValue::Float(x)));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (x, label) in ANGLES {
            if plate::detent(ui, angle == Some(x), label).clicked() {
                *edit = Some(Edit::Tone("angle", FxValue::Float(x)));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (i, (option, label)) in SHAPES.iter().enumerate() {
            if plate::detent(ui, shape == Some(i), label).clicked() {
                *edit = Some(Edit::Tone("shape", FxValue::Choice((*option).into())));
            }
        }
    });

    ui.add_space(6.0);
    plate::legend(ui, "rulers (pāsu jōgi)");
    let rulers = Rulers::of(state.cut());
    ui.horizontal_wrapped(|ui| {
        for (points, label) in RULERS.iter().enumerate() {
            if plate::detent(ui, rulers.vps.len() == points, label).clicked() {
                *edit = Some(Edit::Rulers(points));
            }
        }
    });
    plate::latch(ui, &mut state.manga.snap, "snap to rulers")
        .on_hover_text("The pen holds to the nearest ruler line once it has moved 6 px.");
    for (k, vp) in rulers.vps.iter().enumerate() {
        plate::value(ui, &format!("VP{} {:.0}, {:.0}", k + 1, vp[0], vp[1]));
    }
    if let Some(y) = rulers.horizon {
        plate::value(ui, &format!("horizon {y:.0}"));
    }
}

/// The page drawn small: the paper, its trim and inner frame, the panels
/// (the selected one in tally ink). A click selects the panel under it.
fn thumbnail(ui: &mut egui::Ui, page: &Page, panels: &[Poly], selected: &mut usize) {
    let w = ui.available_width().clamp(60.0, 160.0);
    let scale = w / page.width.max(1) as f32;
    let h = page.height as f32 * scale;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h.min(400.0)), egui::Sense::click());
    let to_screen = |p: [f32; 2]| rect.min + egui::vec2(p[0], p[1]) * scale;
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, plate::PAPER);
    for r in [page.trim, page.inner] {
        let pts = manga::rect_poly(r).into_iter().map(to_screen).collect();
        p.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.0, plate::LEGEND)));
    }
    for (i, poly) in panels.iter().enumerate() {
        let pts = poly.iter().copied().map(to_screen).collect();
        let stroke = if i == *selected {
            egui::Stroke::new(2.0, plate::TALLY)
        } else {
            egui::Stroke::new(1.0, plate::GRAPHITE)
        };
        p.add(egui::Shape::closed_line(pts, stroke));
    }
    if let Some(at) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
        let q = (at - rect.min) / scale;
        if let Some(i) = manga::panel_at(panels, [q.x, q.y]) {
            *selected = i;
        }
    }
}

/// The panel tool's `op` on the open page as one undo step.
fn panels(state: &mut AppState, op: PanelOp) {
    let Some(page) = Page::of(state.cut()) else {
        state.refuse("refused — this cut is no page (New page makes one)");
        return;
    };
    let Some((id, polys)) = manga::panels_of(state.cut()) else {
        state.refuse("refused — the page has no panels (Clip to panels makes them)");
        return;
    };
    let (i, side, row) = (state.manga.selected, state.manga.side_mm, state.manga.row_mm);
    let Some((next, sel)) = panel_op(&polys, i, op, &page, side, row) else {
        state.refuse(format!("refused — {}: the panel is too small, or there is none", op.label().to_lowercase()));
        return;
    };
    let label = op.label().to_lowercase();
    if state.manga_edit(&label, |cut, at| manga::set_panels(cut, at, id, next).into_iter().collect()) {
        state.manga.selected = sel;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates;
    use crate::testkit::EditorHarness;
    use crate::workspace::Stage;

    fn template(name: &str) -> usize {
        templates::ALL.iter().position(|t| t.name == name).expect("the template")
    }

    /// The editor on a fresh Manga B5 book, in its room.
    fn b5() -> EditorHarness {
        let mut h = EditorHarness::new();
        *h.editor_mut() = crate::open_template(template("Manga B5"), None, None);
        h.run();
        h
    }

    fn top(p: &Poly) -> f32 {
        bounds(p)[1]
    }

    fn bottom(p: &Poly) -> f32 {
        bounds(p)[3]
    }

    #[test]
    fn mangaui_room_shows_panel_tone_and_ruler_tools() {
        let h = b5();
        assert_eq!(h.editor().stage, Some(Stage::Manga));
        for label in [
            "MANGA (PAGES)",
            "New page",
            "panel 1",
            "Split across",
            "Split down",
            "Add panel",
            "Delete panel",
            "Clip to panels",
            "42.5 lpi",
            "60 lpi",
            "85 lpi",
            "screen 45°",
            "round dots",
            "no rulers",
            "1-point",
            "2-point",
            "3-point",
            "snap to rulers",
        ] {
            assert!(h.has(label), "the manga room has no {label:?}\n{}", h.describe());
        }
        assert_eq!(Action::MangaRoom.menu(), Some("Room/manga (pages)"));
        // The page's tone starts at the common screen: 60 lpi at 45°, round.
        assert_eq!(h.node_toggled("60 lpi"), Some(true));
        assert_eq!(h.node_toggled("screen 45°"), Some(true));
        assert_eq!(h.node_toggled("round dots"), Some(true));
        assert_eq!(h.node_toggled("no rulers"), Some(true));
    }

    #[test]
    fn mangaui_b5_template_makes_a_2591_by_3624_page() {
        let s = templates::ALL[template("Manga B5")].build();
        let p = &s.engine.project;
        assert_eq!((p.width, p.height, p.dpi), (2591, 3624, 350.0));
        let page = Page::of(s.cut()).expect("the cut is a page");
        assert_eq!(page, Page::b5());
        assert_eq!(s.cut().name, "P01");
        let (_, panels) = manga::panels_of(s.cut()).expect("the page graph's panels");
        assert_eq!(panels, vec![page.first_panel()]);
        assert!(tone_now(s.cut()).is_some(), "the TONE column goes through a Tone");
        assert!(!s.engine.can_undo(), "a new book has no history");
        let a4 = templates::ALL[template("Manga A4")].build();
        assert_eq!((a4.engine.project.width, a4.engine.project.height), (2976, 4175));
        assert_eq!(Page::of(a4.cut()), Some(Page::a4()));
        let web = templates::ALL[template("Webtoon")].build();
        assert_eq!(web.engine.project.width, 800);
        assert_eq!(Page::of(web.cut()).and_then(|p| p.segment), Some(1280));
    }

    #[test]
    fn mangaui_new_page_is_one_undo_step() {
        let mut h = b5();
        let scene = h.state().at().scene;
        let cuts = |h: &EditorHarness| {
            h.state().engine.project.scenes.iter().find(|s| s.id == scene).map_or(0, |s| s.cuts.len())
        };
        h.scroll_to("New page");
        h.click("New page");
        assert_eq!(cuts(&h), 2);
        let s = h.state();
        assert_eq!((s.cut().name.as_str(), s.cut().frame_count), ("P02", 1));
        assert_eq!(Page::of(s.cut()), Some(Page::b5()));
        assert_eq!(manga::panels_of(s.cut()).map(|p| p.1), Some(vec![Page::b5().first_panel()]));
        assert_eq!(s.engine.history_len(), 1, "the page and its graph are one step");
        assert!(h.has("P01") && h.has("P02"), "both pages are listed");
        h.state_mut().undo();
        assert_eq!(cuts(&h), 1);
        assert_eq!(h.state().cut().name, "P01");
    }

    #[test]
    fn mangaui_split_across_is_one_undo_step_with_a_69_px_gutter() {
        let mut h = b5();
        h.scroll_to("Split across");
        h.click("Split across");
        let (_, polys) = manga::panels_of(h.state().cut()).expect("panels");
        assert_eq!(polys.len(), 2);
        assert_eq!(top(&polys[1]) - bottom(&polys[0]), 69.0, "5 mm at 350 dpi");
        assert_eq!(h.state().engine.history_len(), 1);
        // The top row, split down: the 2 mm side gutter, 28 px.
        h.click("Split down");
        let (_, polys) = manga::panels_of(h.state().cut()).expect("panels");
        assert_eq!(polys.len(), 3);
        assert_eq!(bounds(&polys[1])[0] - bounds(&polys[0])[2], 28.0, "2 mm at 350 dpi");
        assert_eq!(h.state().engine.history_len(), 2);
        h.state_mut().undo();
        h.state_mut().undo();
        let (_, polys) = manga::panels_of(h.state().cut()).expect("panels");
        assert_eq!(polys, vec![Page::b5().first_panel()]);
        // Deleting the last panel, then adding one back: a step each.
        h.run();
        h.click("Delete panel");
        assert_eq!(manga::panels_of(h.state().cut()).map(|p| p.1.len()), Some(0));
        h.click("Add panel");
        assert_eq!(manga::panels_of(h.state().cut()).map(|p| p.1), Some(vec![Page::b5().first_panel()]));
    }

    #[test]
    fn mangaui_tone_and_ruler_presets_are_one_step_each() {
        let mut h = b5();
        h.scroll_to("85 lpi");
        h.click("85 lpi");
        assert_eq!(tone_now(h.state().cut()).map(|v| v.float("lpi")), Some(85.0));
        assert_eq!(h.state().engine.history_len(), 1);
        // The armed detent again changes nothing.
        h.click("85 lpi");
        assert_eq!(h.state().engine.history_len(), 1);
        h.scroll_to("square dots");
        h.click("square dots");
        assert_eq!(tone_now(h.state().cut()).map(|v| v.choice("shape")), Some(1));
        h.scroll_to("2-point");
        h.click("2-point");
        let rulers = Rulers::of(h.state().cut());
        assert_eq!(rulers.vps.len(), 2);
        assert_eq!(rulers.horizon, Some(3624.0 * 0.4));
        assert_eq!(h.state().engine.history_len(), 3);
        // The canvas reads the snap on its next frame.
        h.run_frames(2);
        assert_eq!(h.editor().canvas.snap_rulers, Some(rulers), "the pen holds to them");
        h.scroll_to("snap to rulers");
        h.click("snap to rulers");
        h.run_frames(2);
        assert_eq!(h.editor().canvas.snap_rulers, None, "snap off: the pen is free");
        h.click("no rulers");
        assert!(Rulers::of(h.state().cut()).vps.is_empty());
        assert_eq!(h.state().engine.history_len(), 4);
    }

    /// The perpendicular distance from `p` to the line through `a` toward `b`.
    fn off_line(p: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
        let d = (b - a).normalized();
        let v = p - a;
        (v.x * d.y - v.y * d.x).abs()
    }

    #[test]
    fn mangaui_ruler_snap_holds_the_pen_to_the_vanishing_point() {
        let vp = egui::pos2(960.0, 540.0);
        let stroke = |snap: bool| {
            let mut h = EditorHarness::new();
            h.rig_mut().config.pen.stabilizer = 0;
            let rulers = Rulers {
                vps: vec![[vp.x, vp.y]],
                horizon: Some(vp.y),
            };
            let at = h.state().at();
            let set = rulers.set(h.state().cut(), at).expect("a change");
            h.state_mut().engine.apply("rulers", vec![set]).expect("applied");
            h.state_mut().manga.snap = snap;
            h.run_frames(2);
            assert_eq!(h.editor().canvas.snap_rulers.is_some(), snap);
            let xf = h.editor().canvas.view_xform();
            // A stroke that sets off toward the vanishing point (the snap
            // picks its line from that), then wobbles ±6 px across it.
            let paper: Vec<egui::Pos2> = (0..12)
                .map(|k| {
                    let t = k as f32 / 11.0;
                    let wob = match k {
                        0..3 => 0.0,
                        _ if k % 2 == 0 => 6.0,
                        _ => -6.0,
                    };
                    egui::pos2(560.0 + 300.0 * t + wob * 0.447, 340.0 + 150.0 * t - wob * 0.894)
                })
                .collect();
            let screen: Vec<egui::Pos2> = paper.iter().map(|p| xf.paper_to_screen(*p)).collect();
            h.drag_path(&screen);
            let tap = h.editor().canvas.stroke_tap.clone();
            assert!(tap.len() >= 6, "the stroke was tapped: {} samples", tap.len());
            let first = egui::pos2(tap[0].x, tap[0].y);
            tap.iter()
                .map(|s| off_line(egui::pos2(s.x, s.y), first, vp))
                .fold(0.0f32, f32::max)
        };
        let held = stroke(true);
        assert!(held < 0.5, "the snapped stroke strays {held} px off its line");
        let free = stroke(false);
        assert!(free > 3.0, "the free stroke keeps its wobble: {free} px");
    }
}
