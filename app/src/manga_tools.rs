//! GM18: the Manga room's tool pane — the book's pages, the panel (koma)
//! tool, screen tone (tōn), the perspective rulers (pāsu jōgi) and the
//! pages' export. Every edit is one undo step through the engine
//! (`AppState::manga_edit`) and is refused while a stroke is live; New
//! page, Clip to panels, the panel tool, the ruler snap and Export pages go
//! through the one dispatch (D6). The rulers' snap is the pen's: the canvas
//! reads it each frame and holds a stroke to it before the stabilizer. The
//! canvas draws a page's guides, panels and rulers ([`paint_page`]) and
//! its panel tool's gestures end here ([`koma_gesture`]).

use std::path::PathBuf;

use anim_core::command::{Command, CutRef};
use anim_core::deliver::{PageFormat, PageMode, PageOpts};
use anim_core::fx::{FxKind, FxValue, FxValues};
use anim_core::generator::{GenKind, GenValues};
use anim_core::graph::NodeKind;
use anim_core::ids::{CutId, NodeId, SceneId};
use anim_core::manga::{self, Page, Poly, RulerHandle, Rulers};
use anim_core::model::Cut;
use eframe::egui;

use crate::config::Action;
use crate::doc::AppState;
use crate::plate;

/// The pane's session state (the view's, not the file's): the panel the
/// tools act on, the panel tool's gutters, the rulers' snap, the canvas's
/// page guides and how the pages export.
pub struct MangaState {
    /// The selected panel, an index into the page's panels.
    pub selected: usize,
    /// The gutter between panels side by side, and between rows, mm.
    pub side_mm: f32,
    pub row_mm: f32,
    /// The pen holds to the page's rulers (when it has any).
    pub snap: bool,
    /// The canvas draws the page's guides: the bleed edge, trim and inner
    /// frame, the panels' outlines and the rulers.
    pub guides: bool,
    /// How Export pages writes the book: PNG or TIFF, colour, grey or mono
    /// (its threshold), trim marks.
    pub export: PageOpts,
    /// The pages being written on a worker (one export at a time).
    pub export_job: Option<PageExport>,
    /// A field's value while its drag runs (the field's key, the value):
    /// the page changes once, when the drag ends.
    pub held: Option<(&'static str, f32)>,
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
            guides: true,
            export: PageOpts::default(),
            export_job: None,
            held: None,
            asked: Vec::new(),
        }
    }
}

/// GM18: the book's pages on their way out — rendered and written off the
/// UI thread (NEVER-DO 6); the status line says what was written.
pub struct PageExport {
    /// The folder they go into.
    pub dir: PathBuf,
    pub rx: std::sync::mpsc::Receiver<Result<Vec<PathBuf>, String>>,
}

/// GM18: the panel tool's drag, in page px: where it was pressed, where
/// the pointer is now, the ruler handle it holds (None: it draws or
/// splits a panel) and [`HANDLE_REACH`] at the view's zoom (a drag shorter
/// than twice it only selects).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KomaDrag {
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub handle: Option<RulerHandle>,
    pub reach: f32,
}

/// How near a press must land to a vanishing point or the horizon for the
/// panel tool to hold it, screen px.
pub const HANDLE_REACH: f32 = 10.0;

/// A split drawn within this many degrees of level or upright is cut
/// exactly level or upright.
const AXIS_SNAP_DEG: f32 = 4.0;

/// What Clip to panels does to a graph shaped by hand (its button's and
/// menu item's DANGER line).
pub const CLIP_DANGER: &str = "rebuilds the page graph; nodes you added are removed — Ctrl+Z restores";

/// How a tone column is told: by its name.
const TONE_NAMING: &str = "A column whose name starts TONE (or トーン) is a tone column: Clip to panels \
                           puts its art through a Tone, the screen dots; every other column's art stays as drawn.";

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
            Self::SplitAcross => {
                "A cut left to right through the selected panel's middle: an upper and a lower panel, \
                 the row gutter between them."
            }
            Self::SplitDown => {
                "A cut top to bottom through the selected panel's middle: a left and a right panel, \
                 the side gutter between them."
            }
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

/// The line from `a` to `b`, held level or upright on a whole px when it
/// runs within [`AXIS_SNAP_DEG`] of one (a split then leaves the gutter
/// exactly); else as drawn.
fn level(a: [f32; 2], b: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    let deg = (b[1] - a[1]).abs().atan2((b[0] - a[0]).abs()).to_degrees();
    if deg <= AXIS_SNAP_DEG {
        let y = ((a[1] + b[1]) / 2.0).round();
        ([a[0], y], [b[0], y])
    } else if deg >= 90.0 - AXIS_SNAP_DEG {
        let x = ((a[0] + b[0]) / 2.0).round();
        ([x, a[1]], [x, b[1]])
    } else {
        (a, b)
    }
}

/// What a panel-tool drag does to the page's panels.
#[derive(Debug, Clone, PartialEq)]
pub enum KomaOp {
    /// Panel `i`, split along the line through the two points.
    Split(usize, [f32; 2], [f32; 2]),
    /// A new panel, drawn on bare paper.
    Draw(Poly),
}

/// The drag from `from` to `to` over `polys`: the panel under its middle
/// (else under its start) split along its line, held level or upright;
/// else, on bare paper, the box it spans drawn (None: too thin a box).
pub fn koma_op(polys: &[Poly], from: [f32; 2], to: [f32; 2]) -> Option<KomaOp> {
    let mid = [(from[0] + to[0]) / 2.0, (from[1] + to[1]) / 2.0];
    match manga::panel_at(polys, mid).or_else(|| manga::panel_at(polys, from)) {
        Some(i) => {
            let (a, b) = level(from, to);
            Some(KomaOp::Split(i, a, b))
        }
        None => manga::drag_panel(from, to).map(KomaOp::Draw),
    }
}

/// A drag too short to draw or split: under twice the handle reach.
fn koma_short(k: &KomaDrag) -> bool {
    (k.to[0] - k.from[0]).hypot(k.to[1] - k.from[1]) < 2.0 * k.reach
}

/// A ruler handle's name, as the canvas speaks it ("VP1", "horizon").
fn handle_name(h: RulerHandle) -> String {
    match h {
        RulerHandle::Vp(k) => format!("VP{}", k + 1),
        RulerHandle::Horizon => "horizon".into(),
    }
}

/// GM18: the panel tool's drag `k` lands — one undo step: the ruler handle
/// it held moved there; else the panel it crosses split along it (the
/// gutter by its direction, [`manga::gutter_for`]); else a panel drawn on
/// the paper. A drag shorter than twice the reach only selects the panel
/// under it.
pub fn koma_gesture(state: &mut AppState, k: KomaDrag) {
    let Some(page) = Page::of(state.cut()) else {
        state.refuse("refused — this cut is no page (New page makes one)");
        return;
    };
    if let Some(handle) = k.handle {
        let rulers = Rulers::of(state.cut()).moved(handle, k.to);
        let label = format!("move {}", handle_name(handle));
        state.manga_edit(&label, |cut, at| rulers.set(cut, at).into_iter().collect());
        return;
    }
    let Some((id, polys)) = manga::panels_of(state.cut()) else {
        state.refuse("refused — the page has no panels (Clip to panels makes them)");
        return;
    };
    if koma_short(&k) {
        if let Some(i) = manga::panel_at(&polys, k.from) {
            state.manga.selected = i;
        }
        return;
    }
    let (next, sel, label) = match koma_op(&polys, k.from, k.to) {
        Some(KomaOp::Split(i, a, b)) => {
            let gutter = manga::gutter_for(a, b, state.manga.side_mm, state.manga.row_mm, page.dpi);
            let Some((first, second)) = manga::split_panel(&polys[i], a, b, gutter) else {
                state.refuse("refused — split panel: a part would be empty (cut nearer the middle)");
                return;
            };
            let mut next = polys;
            next[i] = first;
            next.insert(i + 1, second);
            (next, i, "split panel")
        }
        Some(KomaOp::Draw(poly)) => {
            let mut next = polys;
            next.push(poly);
            let last = next.len() - 1;
            (next, last, "draw panel")
        }
        None => {
            state.refuse("refused — draw panel: drag a box at least 2 px each way");
            return;
        }
    };
    if state.manga_edit(label, |cut, at| manga::set_panels(cut, at, id, next).into_iter().collect()) {
        state.manga.selected = sel;
    }
}

/// The tone presets — line counts, screen angles and dot shapes — with
/// what each is for.
const LPIS: [(f32, &str, &str); 3] = [
    (42.5, "42.5 lpi", "A coarse screen: big dots that show as dots (a bold sky, an effect)."),
    (60.0, "60 lpi", "The common manga screen: dots that print clean on manga paper."),
    (85.0, "85 lpi", "A fine screen: dots that read as a grey (soft shading, a mist)."),
];
const ANGLES: [(f32, &str, &str); 2] = [
    (0.0, "screen 0°", "Dot rows level and upright: the pattern shows (for effects)."),
    (45.0, "screen 45°", "Dot rows on the diagonal, where the eye sees them least: the usual angle."),
];
const SHAPES: [(&str, &str, &str); 3] = [
    ("round", "round dots", "Round dots: the usual tone."),
    ("square", "square dots", "Square dots: they touch like a checkerboard at half grey."),
    ("line", "lines", "Parallel lines along the screen angle: a line tone."),
];
/// The ruler presets, by their vanishing points' count, with what each
/// draws.
const RULERS: [(&str, &str); 4] = [
    ("no rulers", "No vanishing point: the pen draws free."),
    ("1-point", "One vanishing point at the horizon's middle: a street seen head on."),
    ("2-point", "Two vanishing points at the horizon's ends: a building seen at its corner."),
    ("3-point", "Two on the horizon and a third far below: looking down from high up."),
];
/// The pane's fields for each vanishing point, as the held value names
/// them (x, y).
const VP_KEYS: [[&str; 2]; 3] = [["vp1 x", "vp1 y"], ["vp2 x", "vp2 y"], ["vp3 x", "vp3 y"]];

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

/// The page's Panels node and its border, px (the page graph's first
/// Panels generator, as [`manga::panels_of`] finds it).
fn border_of(cut: &Cut) -> Option<(NodeId, f32)> {
    cut.graph.nodes().find_map(|n| match &n.kind {
        NodeKind::Generator {
            generator: GenKind::Panels,
            params,
            binds,
        } => Some((n.id, GenValues::resolve(GenKind::Panels, params, binds, |_| None).float("border"))),
        _ => None,
    })
}

/// Node `id`'s panel border becomes `px`: one `SetNodeKind` (none: it
/// already is, or `id` is no generator).
fn set_border(cut: &Cut, at: CutRef, id: NodeId, px: f32) -> Vec<Command> {
    let Some(now) = border_of(cut).filter(|(n, _)| *n == id).map(|(_, b)| b) else {
        return Vec::new();
    };
    let Ok(node) = cut.graph.node(id) else {
        return Vec::new();
    };
    let mut kind = node.kind.clone();
    let NodeKind::Generator { params, .. } = &mut kind else {
        return Vec::new();
    };
    if now == px {
        return Vec::new();
    }
    params.set("border", FxValue::Float(px));
    vec![Command::SetNodeKind { at, id, kind }]
}

/// A number the page holds, as a field: its value while a drag runs is
/// held under `key` in `held` (the page changes once, when the drag ends
/// or a typed value lands). Some(the value) when it lands.
fn held_field(
    ui: &mut egui::Ui,
    held: &mut Option<(&'static str, f32)>,
    key: &'static str,
    now: f32,
    range: std::ops::RangeInclusive<f32>,
    speed: f32,
    suffix: &str,
) -> (egui::Response, Option<f32>) {
    let mut v = match *held {
        Some((k, v)) if k == key => v,
        _ => now,
    };
    let r = plate::field(
        ui,
        egui::DragValue::new(&mut v)
            .range(range)
            .speed(speed)
            .suffix(suffix)
            .max_decimals(2)
            .update_while_editing(false),
    );
    if r.dragged() {
        *held = Some((key, v));
    }
    let landed = r.drag_stopped() || (r.changed() && !r.dragged());
    if landed && held.is_some_and(|(k, _)| k == key) {
        *held = None;
    }
    (r, landed.then_some(v))
}

/// What the pane's pass asked for, applied after it.
enum Edit {
    Goto(SceneId, CutId),
    Panels(PanelOp),
    Tone(&'static str, FxValue),
    Rulers(usize),
    /// A vanishing point or the horizon, typed or dragged to a place.
    Ruler(RulerHandle, [f32; 2]),
    /// The panels' border, mm.
    Border(f32),
}

/// `gesture_safe`: no stroke is live (an edit is refused while one is).
/// `koma_armed`: the canvas holds the panel tool (its latch shows it).
pub fn ui(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool, koma_armed: bool) {
    let mut edit = None;
    let scroll = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| pane(ui, state, koma_armed, &mut edit));
    plate::note_viewport(ui, scroll.inner_rect);
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
        Edit::Ruler(handle, to) => {
            let rulers = Rulers::of(state.cut()).moved(handle, to);
            let label = format!("move {}", handle_name(handle));
            state.manga_edit(&label, |cut, at| rulers.set(cut, at).into_iter().collect());
        }
        Edit::Border(mm) => match (Page::of(state.cut()), border_of(state.cut())) {
            (Some(page), Some((id, _))) => {
                let px = mm / 25.4 * page.dpi;
                state.manga_edit("panel border", |cut, at| set_border(cut, at, id, px));
            }
            (None, _) => state.refuse("refused — this cut is no page (New page makes one)"),
            (_, None) => state.refuse("refused — the page has no panels (Clip to panels makes them)"),
        },
    }
}

fn pane(ui: &mut egui::Ui, state: &mut AppState, koma_armed: bool, edit: &mut Option<Edit>) {
    plate::legend(ui, "manga (pages)");
    let page = Page::of(state.cut());
    let not_a_book = state.new_page_refusal();
    match &page {
        Some(p) => {
            plate::value(ui, &format!("{} · {}×{} at {} dpi", p.format, p.width, p.height, p.dpi));
        }
        None => {
            let why = not_a_book.unwrap_or("this cut is no page — New page makes one");
            ui.label(egui::RichText::new(why).color(plate::legend_dim()));
        }
    }
    plate::latch(ui, &mut state.manga.guides, "page guides").on_hover_text(
        "The canvas draws the page's bleed edge, trim and inner frame, its panels and its rulers \
         (the panel tool shows them while it is armed, whatever this says).",
    );

    ui.add_space(6.0);
    plate::legend(ui, "pages (pēji)");
    let scene = state.at().scene;
    let pages: Vec<(CutId, String, Page, Vec<Poly>)> = state
        .engine
        .project
        .scenes
        .iter()
        .filter(|s| s.id == scene)
        .flat_map(|s| s.cuts.iter())
        .filter_map(|c| {
            let polys = manga::panels_of(c).map(|(_, p)| p).unwrap_or_default();
            Page::of(c).map(|p| (c.id, c.name.clone(), p, polys))
        })
        .collect();
    // One row a page: its thumbnail, its number in the book, its name.
    for (n, (id, name, p, polys)) in pages.iter().enumerate() {
        let here = *id == state.view.cut;
        ui.horizontal(|ui| {
            let (thumb, ..) = page_thumb(ui, p, polys, None, egui::vec2(30.0, 40.0));
            plate::speak(&thumb, egui::WidgetType::Button, Some(here), &format!("page {}: {name}", n + 1));
            let thumb = thumb.on_hover_text(format!("page {} of the book — click to open it", n + 1));
            plate::value(ui, &format!("{:>2}", n + 1));
            let row = plate::detent(ui, here, name);
            if (thumb.clicked() || row.clicked()) && !here {
                *edit = Some(Edit::Goto(scene, *id));
            }
        });
    }
    let new = ui.add_enabled(not_a_book.is_none(), egui::Button::new("New page"));
    let new = match not_a_book {
        Some(why) => new.on_disabled_hover_text(format!("{}\nnot now: {why}", Action::NewPage.help())),
        None => crate::menus::hover_tip(new, Action::NewPage, ""),
    };
    if new.clicked() {
        state.manga.asked.push(Action::NewPage);
    }

    ui.add_space(6.0);
    plate::legend(ui, "panels (koma)");
    let panels = manga::panels_of(state.cut()).map(|(_, p)| p).unwrap_or_default();
    if state.manga.selected >= panels.len() {
        state.manga.selected = panels.len().saturating_sub(1);
    }
    if let Some(p) = &page {
        // The page drawn small; a click selects the panel under it.
        let w = ui.available_width().clamp(60.0, 160.0);
        let (resp, rect, scale) = page_thumb(ui, p, &panels, Some(state.manga.selected), egui::vec2(w, 400.0));
        if let Some(at) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
            let q = (at - rect.min) / scale;
            if let Some(i) = manga::panel_at(&panels, [q.x, q.y]) {
                state.manga.selected = i;
            }
        }
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
    // The canvas's panel tool: a drag draws, splits or moves a ruler.
    let mut armed = koma_armed;
    let latch = plate::latch(ui, &mut armed, "koma tool");
    if crate::menus::hover_tip(latch, Action::PanelTool, "").clicked() {
        state.manga.asked.push(Action::PanelTool);
    }
    const SIDE: &str = "The gutter between panels side by side — Split down, or a panel-tool cut more down \
                        the page than across: 2 mm is 28 px at 350 dpi.";
    const ROW: &str = "The gutter between rows — Split across, or a panel-tool cut more across the page \
                       than down: 5 mm is 69 px at 350 dpi.";
    ui.horizontal(|ui| {
        plate::legend(ui, "side gutter").on_hover_text(SIDE);
        plate::field(ui, egui::DragValue::new(&mut state.manga.side_mm).range(0.0..=20.0).speed(0.1).suffix(" mm"))
            .on_hover_text(SIDE);
    });
    ui.horizontal(|ui| {
        plate::legend(ui, "row gutter").on_hover_text(ROW);
        plate::field(ui, egui::DragValue::new(&mut state.manga.row_mm).range(0.0..=20.0).speed(0.1).suffix(" mm"))
            .on_hover_text(ROW);
    });
    if let (Some(p), Some((_, px))) = (&page, border_of(state.cut())) {
        const BORDER: &str = "The panels' border, inside each outline (0.5 mm is the common pen line).";
        ui.horizontal(|ui| {
            plate::legend(ui, "border").on_hover_text(BORDER);
            let now = px / p.dpi * 25.4;
            let (r, mm) = held_field(ui, &mut state.manga.held, "border mm", now, 0.0..=5.0, 0.01, " mm");
            r.on_hover_text(BORDER);
            if let Some(mm) = mm {
                *edit = Some(Edit::Border(mm));
            }
        });
    }
    // DANGER over a graph shaped by hand: the rebuild drops what it does
    // not know (the Aka edge; its tooltip says what goes).
    let hand = state.page_hand_built();
    let clip = ui.button("Clip to panels");
    if hand.is_some() {
        ui.painter().rect_stroke(
            clip.rect,
            egui::CornerRadius::ZERO,
            egui::Stroke::new(1.0, plate::AKA),
            egui::StrokeKind::Inside,
        );
    }
    let more = hand.map_or_else(String::new, |n| format!("DANGER: it {CLIP_DANGER} ({n} nodes now)"));
    if crate::menus::hover_tip(clip, Action::ClipToPanels, &more).clicked() {
        state.manga.asked.push(Action::ClipToPanels);
    }

    ui.add_space(6.0);
    plate::legend(ui, "tone (tōn)");
    let tone = tone_now(state.cut());
    let hint = match &tone {
        None => "no tone yet — a column named TONE (or トーン), then Clip to panels",
        Some(_) => "the screen of the TONE (トーン) columns",
    };
    ui.label(egui::RichText::new(hint).color(plate::legend_dim())).on_hover_text(TONE_NAMING);
    let lpi = tone.as_ref().map(|v| v.float("lpi"));
    let angle = tone.as_ref().map(|v| v.float("angle"));
    let shape = tone.as_ref().map(|v| v.choice("shape"));
    ui.horizontal_wrapped(|ui| {
        for (x, label, help) in LPIS {
            if plate::detent(ui, lpi == Some(x), label).on_hover_text(help).clicked() {
                *edit = Some(Edit::Tone("lpi", FxValue::Float(x)));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (x, label, help) in ANGLES {
            if plate::detent(ui, angle == Some(x), label).on_hover_text(help).clicked() {
                *edit = Some(Edit::Tone("angle", FxValue::Float(x)));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (i, (option, label, help)) in SHAPES.iter().enumerate() {
            if plate::detent(ui, shape == Some(i), label).on_hover_text(*help).clicked() {
                *edit = Some(Edit::Tone("shape", FxValue::Choice((*option).into())));
            }
        }
    });
    if let (Some(lpi), Some(angle)) = (lpi, angle) {
        const LINES: &str = "The screen's line count, lines per inch along its angle (20 to 120).";
        const TURN: &str = "The dot rows' angle, degrees (0 to 180).";
        ui.horizontal(|ui| {
            plate::legend(ui, "lpi").on_hover_text(LINES);
            let (r, x) = held_field(ui, &mut state.manga.held, "tone lpi", lpi, 20.0..=120.0, 0.5, "");
            r.on_hover_text(LINES);
            if let Some(x) = x {
                *edit = Some(Edit::Tone("lpi", FxValue::Float(x)));
            }
        });
        ui.horizontal(|ui| {
            plate::legend(ui, "angle").on_hover_text(TURN);
            let (r, x) = held_field(ui, &mut state.manga.held, "tone angle", angle, 0.0..=180.0, 0.5, "°");
            r.on_hover_text(TURN);
            if let Some(x) = x {
                *edit = Some(Edit::Tone("angle", FxValue::Float(x)));
            }
        });
    }

    ui.add_space(6.0);
    plate::legend(ui, "rulers (pāsu jōgi)");
    let rulers = Rulers::of(state.cut());
    ui.horizontal_wrapped(|ui| {
        for (points, (label, help)) in RULERS.iter().enumerate() {
            if plate::detent(ui, rulers.vps.len() == points, label).on_hover_text(*help).clicked() {
                *edit = Some(Edit::Rulers(points));
            }
        }
    });
    // The snap is a verb (View ▸ Snap to rulers, its chord): the latch asks
    // the one dispatch for it.
    let mut snap = state.manga.snap;
    let latch = plate::latch(ui, &mut snap, "snap to rulers");
    let more = "The pen holds to the nearest ruler line once it has moved 6 px.";
    if crate::menus::hover_tip(latch, Action::ToggleRulerSnap, more).clicked() {
        state.manga.asked.push(Action::ToggleRulerSnap);
    }
    // Each point and the horizon placed by number (the panel tool drags
    // them on the canvas).
    const PLACE: &str = "Page px from the top left; the panel tool drags it on the canvas. A point on the \
                         horizon moves the horizon with it.";
    for (k, vp) in rulers.vps.iter().enumerate().take(VP_KEYS.len()) {
        ui.horizontal(|ui| {
            plate::legend(ui, &format!("vp {}", k + 1)).on_hover_text(PLACE);
            let [kx, ky] = VP_KEYS[k];
            let (r, x) = held_field(ui, &mut state.manga.held, kx, vp[0], -1.0e6..=1.0e6, 1.0, "");
            r.on_hover_text(PLACE);
            let (r, y) = held_field(ui, &mut state.manga.held, ky, vp[1], -1.0e6..=1.0e6, 1.0, "");
            r.on_hover_text(PLACE);
            if let Some(x) = x {
                *edit = Some(Edit::Ruler(RulerHandle::Vp(k), [x, vp[1]]));
            }
            if let Some(y) = y {
                *edit = Some(Edit::Ruler(RulerHandle::Vp(k), [vp[0], y]));
            }
        });
    }
    if let Some(h) = rulers.horizon {
        const LEVEL: &str = "The horizon's height, page px from the top; the points on it move with it.";
        ui.horizontal(|ui| {
            plate::legend(ui, "horizon y").on_hover_text(LEVEL);
            let (r, y) = held_field(ui, &mut state.manga.held, "horizon y", h, -1.0e6..=1.0e6, 1.0, "");
            r.on_hover_text(LEVEL);
            if let Some(y) = y {
                *edit = Some(Edit::Ruler(RulerHandle::Horizon, [0.0, y]));
            }
        });
    }

    ui.add_space(6.0);
    plate::legend(ui, "export (nyūkō)");
    let o = &mut state.manga.export;
    ui.horizontal_wrapped(|ui| {
        for (f, label, help) in [
            (PageFormat::Png, "PNG files", "One PNG a page, its dpi in the file."),
            (PageFormat::Tiff, "TIFF files", "One TIFF a page, its dpi in the file (a print shop's usual ask)."),
        ] {
            if plate::detent(ui, o.format == f, label).on_hover_text(help).clicked() {
                o.format = f;
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (m, label, help) in [
            (PageMode::Color, "in colour", "The page as it composes, in colour."),
            (PageMode::Grey, "in grey", "The page's grey (its luma): an 8-bit grey file."),
            (PageMode::Mono, "in mono (1-bit)", "Black or white, nothing between: the print plate, below the threshold."),
        ] {
            if plate::detent(ui, o.mode == m, label).on_hover_text(help).clicked() {
                o.mode = m;
            }
        }
    });
    ui.horizontal(|ui| {
        const CUT: &str = "Mono: a pixel greyer than this prints black, the rest white (0.5 is half grey).";
        plate::legend(ui, "threshold").on_hover_text(CUT);
        let mono = o.mode == PageMode::Mono;
        ui.add_enabled_ui(mono, |ui| {
            plate::field(ui, egui::DragValue::new(&mut o.threshold).range(0.0..=1.0).speed(0.01).max_decimals(2))
        })
        .inner
        .on_hover_text(CUT)
        .on_disabled_hover_text("Mono only: colour and grey keep every tone.");
    });
    plate::latch(ui, &mut o.trim_marks, "trim marks")
        .on_hover_text("Tombo: the printer's corner and centre marks in a margin round each print page.");
    let why = state.page_export_refusal();
    let go = ui.add_enabled(why.is_none(), egui::Button::new("Export pages…"));
    let go = match why {
        Some(why) => go.on_disabled_hover_text(format!("{}\nnot now: {why}", Action::ExportPages.help())),
        None => crate::menus::hover_tip(go, Action::ExportPages, "The folder is asked for next."),
    };
    if go.clicked() {
        state.manga.asked.push(Action::ExportPages);
    }
    if let Some(job) = &state.manga.export_job {
        let text = format!("writing the pages into {}…", job.dir.display());
        ui.label(egui::RichText::new(text).color(plate::legend_dim()));
    }
}

/// The page drawn small within `max`: the paper, its trim and inner frame,
/// the panels (`selected` in tally ink). The response, its rect and its
/// scale (screen points a page px) — the whole page always fits.
fn page_thumb(
    ui: &mut egui::Ui,
    page: &Page,
    panels: &[Poly],
    selected: Option<usize>,
    max: egui::Vec2,
) -> (egui::Response, egui::Rect, f32) {
    let scale = (max.x / page.width.max(1) as f32).min(max.y / page.height.max(1) as f32);
    let size = egui::vec2(page.width as f32, page.height as f32) * scale;
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let to_screen = |p: [f32; 2]| rect.min + egui::vec2(p[0], p[1]) * scale;
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, plate::PAPER);
    for r in [page.trim, page.inner] {
        let pts = manga::rect_poly(r).into_iter().map(to_screen).collect();
        p.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.0, plate::LEGEND)));
    }
    for (i, poly) in panels.iter().enumerate() {
        let pts = poly.iter().copied().map(to_screen).collect();
        let stroke = if Some(i) == selected {
            egui::Stroke::new(2.0, plate::TALLY)
        } else {
            egui::Stroke::new(1.0, plate::GRAPHITE)
        };
        p.add(egui::Shape::closed_line(pts, stroke));
    }
    (resp, rect, scale)
}

/// The page's three guide outlines on screen — the bleed edge (the whole
/// canvas), the trim and the inner frame — their corners through
/// `to_screen` (page px to screen), clockwise from the top left.
pub fn guide_outlines(page: &Page, to_screen: &impl Fn(egui::Pos2) -> egui::Pos2) -> [[egui::Pos2; 4]; 3] {
    let full = manga::PxRect {
        x: 0,
        y: 0,
        w: page.width,
        h: page.height,
    };
    [full, page.trim, page.inner].map(|r| {
        let q = manga::rect_poly(r);
        [0, 1, 2, 3].map(|i| to_screen(egui::pos2(q[i][0], q[i][1])))
    })
}

/// GM18: the page's guides over the canvas while the page guides latch is
/// on or the panel tool is armed: the bleed edge (dim, dashed), the trim
/// (Ao) and the inner frame (Ao, dashed); the panels (the selected one in
/// tally ink); the horizon and a fan of rays from each vanishing point
/// (where the drag holding one has it); and the panel tool's drag as it
/// would land. The vanishing points and the horizon answer to "VP1"… and
/// "horizon" where they show (the panel tool drags them).
pub fn paint_page(
    ui: &egui::Ui,
    painter: &egui::Painter,
    state: &AppState,
    page: &Page,
    to_screen: &impl Fn(egui::Pos2) -> egui::Pos2,
    armed: bool,
    drag: Option<&KomaDrag>,
) {
    if !(state.manga.guides || armed) {
        return;
    }
    let pt = |p: [f32; 2]| to_screen(egui::pos2(p[0], p[1]));
    let closed = |q: &[egui::Pos2]| q.iter().chain(q.first()).copied().collect::<Vec<_>>();
    let dim = egui::Stroke::new(1.0, plate::legend_dim());
    let ao = egui::Stroke::new(1.0, plate::AO);
    let tally = egui::Stroke::new(1.0, plate::TALLY);
    let [bleed, trim, inner] = guide_outlines(page, to_screen);
    painter.extend(egui::Shape::dashed_line(&closed(&bleed), dim, 6.0, 4.0));
    painter.add(egui::Shape::closed_line(trim.to_vec(), ao));
    painter.extend(egui::Shape::dashed_line(&closed(&inner), ao, 4.0, 4.0));
    let panels = manga::panels_of(state.cut()).map(|(_, p)| p).unwrap_or_default();
    for (i, poly) in panels.iter().enumerate() {
        let pts = poly.iter().map(|p| pt(*p)).collect();
        let stroke = if i == state.manga.selected {
            egui::Stroke::new(2.0, plate::TALLY)
        } else {
            dim
        };
        painter.add(egui::Shape::closed_line(pts, stroke));
    }
    // The rulers, as the drag holding a handle has them.
    let held = drag.and_then(|k| k.handle.map(|h| (h, k.to)));
    let rulers = match held {
        Some((h, to)) => Rulers::of(state.cut()).moved(h, to),
        None => Rulers::of(state.cut()),
    };
    let ink = |h: RulerHandle| if held.is_some_and(|(g, _)| g == h) { tally } else { ao };
    let (w, h) = (page.width as f32, page.height as f32);
    let far = w.hypot(h);
    let clip = painter.clip_rect();
    let reach = egui::Vec2::splat(2.0 * HANDLE_REACH);
    if let Some(y) = rulers.horizon {
        let x0 = rulers.vps.iter().fold(0.0f32, |m, v| m.min(v[0])) - far;
        let x1 = rulers.vps.iter().fold(w, |m, v| m.max(v[0])) + far;
        painter.line_segment([pt([x0, y]), pt([x1, y])], ink(RulerHandle::Horizon));
        let c = pt([page.inner.x as f32, y]);
        painter.circle_filled(c, 3.0, ink(RulerHandle::Horizon).color);
        if clip.contains(c) {
            let resp = ui.interact(egui::Rect::from_center_size(c, reach), ui.id().with("manga_horizon"), egui::Sense::hover());
            plate::speak(&resp, egui::WidgetType::Label, None, "horizon");
        }
    }
    let faint = egui::Stroke::new(1.0, plate::AO.gamma_multiply(0.35));
    let aims = [[0.0, 0.0], [w / 2.0, 0.0], [w, 0.0], [w, h / 2.0], [w, h], [w / 2.0, h], [0.0, h], [0.0, h / 2.0]];
    for (k, vp) in rulers.vps.iter().enumerate() {
        for a in aims {
            let (dx, dy) = (a[0] - vp[0], a[1] - vp[1]);
            let len = dx.hypot(dy).max(1e-3);
            painter.line_segment([pt(*vp), pt([a[0] + dx / len * far, a[1] + dy / len * far])], faint);
        }
        let c = pt(*vp);
        let stroke = ink(RulerHandle::Vp(k));
        painter.circle_stroke(c, 5.0, stroke);
        painter.line_segment([c - egui::vec2(9.0, 0.0), c + egui::vec2(9.0, 0.0)], stroke);
        painter.line_segment([c - egui::vec2(0.0, 9.0), c + egui::vec2(0.0, 9.0)], stroke);
        if clip.contains(c) {
            let resp = ui.interact(egui::Rect::from_center_size(c, reach), ui.id().with(("manga_vp", k)), egui::Sense::hover());
            plate::speak(&resp, egui::WidgetType::Label, None, &handle_name(RulerHandle::Vp(k)));
        }
    }
    // The panel tool's drag, as it would land.
    let Some(k) = drag.filter(|k| k.handle.is_none() && !koma_short(k)) else {
        return;
    };
    match koma_op(&panels, k.from, k.to) {
        Some(KomaOp::Split(i, a, b)) => {
            painter.line_segment([pt(a), pt(b)], tally);
            let gutter = manga::gutter_for(a, b, state.manga.side_mm, state.manga.row_mm, page.dpi);
            for half in manga::split_panel(&panels[i], a, b, gutter).into_iter().flat_map(|(p, q)| [p, q]) {
                painter.add(egui::Shape::closed_line(half.iter().map(|p| pt(*p)).collect(), tally));
            }
        }
        Some(KomaOp::Draw(poly)) => {
            painter.add(egui::Shape::closed_line(poly.iter().map(|p| pt(*p)).collect(), tally));
        }
        None => {}
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

    #[test]
    fn mangaui_page_guides_sit_on_the_page_through_the_view() {
        let page = Page::b5();
        let xf = crate::canvas::ViewXform {
            zoom: 0.5,
            pan: egui::Vec2::ZERO,
            rotation_deg: 0.0,
            mirror: false,
            center: egui::pos2(640.0, 400.0),
            paper: egui::vec2(2591.0, 3624.0),
        };
        let [bleed, trim, inner] = guide_outlines(&page, &|p| xf.paper_to_screen(p));
        // The canvas's corners, the trim's 41 px in (3 mm), the inner
        // frame's 262 × 296 px in, at half size about the pane's centre.
        assert_eq!((bleed[0], bleed[2]), (egui::pos2(-7.75, -506.0), egui::pos2(1287.75, 1306.0)));
        assert_eq!(trim[0], egui::pos2(12.75, -485.5));
        assert_eq!((inner[0], inner[2]), (egui::pos2(123.25, -358.0), egui::pos2(1156.75, 1158.0)));
        // Mirrored, the trim's first corner flips about the paper's centre.
        let m = crate::canvas::ViewXform { mirror: true, ..xf };
        assert_eq!(guide_outlines(&page, &|p| m.paper_to_screen(p))[1][0], egui::pos2(1267.25, -485.5));
    }

    #[test]
    fn mangaui_page_list_has_a_row_a_page() {
        let mut h = b5();
        assert!(h.has("page 1: P01"), "{}", h.describe());
        h.scroll_to("New page");
        h.click("New page");
        assert!(h.has("page 1: P01") && h.has("page 2: P02"), "{}", h.describe());
        assert!(!h.has_text_containing("page 3"), "two pages, two rows\n{}", h.describe());
        assert_eq!(h.node_toggled("page 2: P02"), Some(true), "the open page");
        // A row's thumbnail opens its page.
        h.scroll_to("page 1: P01");
        h.click("page 1: P01");
        assert_eq!(h.state().cut().name, "P01");
        assert_eq!(h.node_toggled("page 1: P01"), Some(true));
    }

    #[test]
    fn mangaui_koma_drag_splits_a_panel_and_draws_one_on_bare_paper() {
        let mut h = b5();
        h.perform(Action::PanelTool);
        assert_eq!(h.editor().canvas.tool, crate::canvas::CanvasTool::Panel);
        assert_eq!(h.editor().is_on(Action::PanelTool), Some(true));
        let xf = h.editor().canvas.view_xform();
        let at = |x: f32, y: f32| xf.paper_to_screen(egui::pos2(x, y));
        let inner = Page::b5().inner;
        let (x0, x1, y) = (inner.x as f32, inner.right() as f32, (inner.y + inner.h / 2) as f32);
        // From the margin across the panel, a little off level: one cut,
        // held level, the 5 mm row gutter between the halves.
        h.drag(at(x0 - 30.0, y + 10.0), at(x1 + 30.0, y - 10.0), 8);
        let (_, polys) = manga::panels_of(h.state().cut()).expect("panels");
        assert_eq!(polys.len(), 2);
        assert_eq!(top(&polys[1]) - bottom(&polys[0]), 69.0, "5 mm at 350 dpi");
        assert_eq!(h.state().engine.history_len(), 1, "the drag is one step");
        // Down through the top row: the 2 mm side gutter, 28 px.
        let cx = (x0 + x1) / 2.0;
        h.drag(at(cx, inner.y as f32 - 40.0), at(cx, y), 8);
        let (_, polys) = manga::panels_of(h.state().cut()).expect("panels");
        assert_eq!(polys.len(), 3);
        assert_eq!(bounds(&polys[1])[0] - bounds(&polys[0])[2], 28.0, "2 mm at 350 dpi");
        assert_eq!(h.state().engine.history_len(), 2);
        // On bare paper (the margin): a new panel, the box the drag spans.
        h.drag(at(40.0, 400.0), at(220.0, 900.0), 6);
        let (_, polys) = manga::panels_of(h.state().cut()).expect("panels");
        assert_eq!(polys.len(), 4);
        assert_eq!(polys[3], vec![[40.0, 400.0], [220.0, 400.0], [220.0, 900.0], [40.0, 900.0]]);
        assert_eq!(h.state().manga.selected, 3, "the new panel is selected");
        assert_eq!(h.state().engine.history_len(), 3);
        assert!(!h.editor().canvas.stroke_active(), "the drag has landed");
    }

    #[test]
    fn mangaui_vp1_drags_on_the_canvas_as_one_step() {
        let mut h = b5();
        h.scroll_to("1-point");
        h.click("1-point");
        let before = Rulers::of(h.state().cut());
        assert_eq!(before.vps.len(), 1);
        assert!(h.has("VP1") && h.has("horizon"), "the page guides name the handles\n{}", h.describe());
        assert!(h.has("VP 1") && h.has("HORIZON Y"), "and the pane places them by number");
        // The guides latch hides them; the panel tool shows them again.
        h.scroll_to("page guides");
        h.click("page guides");
        assert!(!h.has("VP1"), "guides off");
        h.perform(Action::PanelTool);
        assert!(h.has("VP1"), "the armed panel tool shows the guides");
        let steps = h.state().engine.history_len();
        let xf = h.editor().canvas.view_xform();
        let c = h.rect("VP1").center();
        let to = c + egui::vec2(60.0, 30.0);
        h.drag(c, to, 6);
        let after = Rulers::of(h.state().cut());
        let want = xf.screen_to_paper(to);
        assert!(
            (after.vps[0][0] - want.x).abs() < 0.05 && (after.vps[0][1] - want.y).abs() < 0.05,
            "{:?} is not {want:?}",
            after.vps
        );
        assert_eq!(after.horizon, Some(after.vps[0][1]), "VP1 on the horizon carries it");
        assert_eq!(h.state().engine.history_len(), steps + 1, "the drag is one step");
        h.state_mut().undo();
        assert_eq!(Rulers::of(h.state().cut()), before);
    }

    #[test]
    fn mangaui_clip_to_panels_says_danger_over_a_graph_shaped_by_hand() {
        let mut h = b5();
        assert_eq!(h.state().page_hand_built(), None, "the template's page graph");
        let at = h.state().at();
        let glow = h.state_mut().engine.alloc_node_id();
        let add = Command::AddNode {
            at,
            id: glow,
            kind: NodeKind::effect(FxKind::Glow),
        };
        h.state_mut().engine.apply("glow", vec![add]).expect("applied");
        let n = h.state().cut().graph.nodes().count();
        assert_eq!(h.state().page_hand_built(), Some(n));
        h.run();
        h.scroll_to("Clip to panels");
        h.hover("Clip to panels");
        h.wait(1.0);
        assert!(h.has_text_containing(CLIP_DANGER), "{}", h.describe());
        // The Cut menu's entry says it too (the pane out of the dock).
        h.editor_mut().dock = egui_dock::DockState::new(vec![crate::workspace::Pane::Canvas]);
        h.run();
        assert!(!h.has_text_containing(CLIP_DANGER), "the pane's tooltip went with it");
        h.click("cut");
        h.hover("Clip to panels");
        h.wait(1.0);
        assert!(h.has_text_containing(CLIP_DANGER), "{}", h.describe());
        // The rebuild drops the glow; Ctrl+Z brings it back.
        assert!(h.state_mut().clip_to_panels());
        assert_eq!(h.state().page_hand_built(), None);
        h.state_mut().undo();
        assert!(h.state().cut().graph.node(glow).is_ok(), "undo restores the glow");
    }

    #[test]
    fn mangaui_snap_to_rulers_is_a_verb_with_a_chord() {
        use crate::config::{self, Keymap};
        for &km in Keymap::ALL {
            let binds = config::default_binds(km);
            let chord = |a: Action| crate::cmdpal::chord_of(&binds, a);
            assert_eq!(chord(Action::ToggleRulerSnap).as_deref(), Some("Ctrl+2"), "{km:?}");
            assert_eq!(chord(Action::PanelTool).as_deref(), Some("K"), "{km:?}");
            assert_eq!(chord(Action::ExportPages).as_deref(), Some("Ctrl+E"), "{km:?}");
            assert_eq!(chord(Action::NewPage).as_deref(), Some("Ctrl+Shift+N"), "{km:?}");
        }
        assert_eq!(Action::ToggleRulerSnap.menu(), Some("View/Snap to rulers"));
        let mut h = b5();
        assert_eq!(h.editor().is_on(Action::ToggleRulerSnap), Some(true));
        // The pane's latch runs the verb (the status line is the verb's).
        h.scroll_to("snap to rulers");
        h.click("snap to rulers");
        assert!(!h.state().manga.snap);
        assert!(h.state().status.starts_with("snap to rulers off"), "{}", h.state().status);
        // And so does its chord, over the canvas.
        let over_canvas = h.editor().canvas.dbg_rect.center();
        h.point_at(over_canvas);
        h.key_mods(crate::testkit::CTRL, egui::Key::Num2);
        assert_eq!(h.performed().last(), Some(&(Action::ToggleRulerSnap, crate::Performed::Done)));
        assert!(h.state().manga.snap);
    }

    #[test]
    fn mangaui_export_pages_writes_the_book_into_a_folder_at_its_dpi() {
        // A small 350 dpi book: 165 × 193 px pages (a 10 × 12 mm trim with
        // 1 mm bleed), in the manga room.
        let mut s = AppState::new_project("book", 165, 193, 24, 350.0);
        assert!(s.make_page(&Page::print("test", [10.0, 12.0], 1.0, [8.0, 10.0], 350.0)));
        let mut h = EditorHarness::with_state(s);
        h.perform(Action::MangaRoom);
        h.scroll_to("New page");
        h.click("New page");
        assert_eq!(h.state().cut().name, "P01");
        h.scroll_to("in grey");
        h.click("in grey");
        assert_eq!(h.state().manga.export.mode, PageMode::Grey);
        // The button asks the shell for the folder (its dialog).
        h.scroll_to("Export pages…");
        h.click("Export pages…");
        assert!(h.rig().requests.contains(&crate::AppRequest::ExportPages), "{:?}", h.rig().requests);
        // The shell's folder, then the worker.
        let dir = h.data_dir().join("pages");
        std::fs::create_dir_all(&dir).expect("the folder");
        h.state_mut().begin_page_export(&dir, None);
        assert!(h.editor().can_perform(Action::ExportPages).is_err(), "one export at a time");
        h.state_mut().wait_page_export();
        assert_eq!(h.state().status, format!("2 page file(s) written into {}", dir.display()));
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("the folder")
            .map(|f| f.expect("an entry").path())
            .collect();
        files.sort();
        assert_eq!(files.len(), 2);
        assert!(files.contains(&dir.join("P01.png")), "{files:?}");
        for path in &files {
            let png = png::Decoder::new(std::fs::File::open(path).expect("written"));
            let reader = png.read_info().expect("a PNG");
            let info = reader.info();
            assert_eq!((info.width, info.height), (165, 193), "{path:?}: the page's size");
            assert_eq!(info.color_type, png::ColorType::Grayscale, "{path:?}");
            let dims = info.pixel_dims.expect("pHYs");
            assert_eq!((dims.xppu, dims.yppu, dims.unit), (13780, 13780, png::Unit::Meter), "350 dpi");
        }
    }

    #[test]
    fn mangaui_new_page_refuses_a_project_that_is_no_book() {
        let mut h = EditorHarness::new();
        let cuts = |h: &EditorHarness| h.state().engine.project.scenes.iter().map(|s| s.cuts.len()).sum::<usize>();
        let n = cuts(&h);
        assert!(h.editor().can_perform(Action::NewPage).is_err(), "1920 × 1080 is no book size");
        assert_eq!(h.perform(Action::NewPage), crate::Performed::Ignored);
        assert!(h.state().refusal.contains("not a manga book"), "{}", h.state().refusal);
        assert_eq!(cuts(&h), n);
        assert!(h.editor().can_perform(Action::ExportPages).is_err(), "no page to export");
    }

    #[test]
    fn mangaui_border_is_one_step_in_page_px() {
        let mut h = b5();
        let (id, px) = border_of(h.state().cut()).expect("the page's panels");
        assert_eq!(px, 7.0, "0.5 mm at 350 dpi");
        assert!(h.state_mut().manga_edit("panel border", |cut, at| set_border(cut, at, id, 14.0)));
        assert_eq!(border_of(h.state().cut()).map(|b| b.1), Some(14.0));
        assert_eq!(h.state().engine.history_len(), 1);
        assert!(!h.state_mut().manga_edit("panel border", |cut, at| set_border(cut, at, id, 14.0)), "no change, no step");
        assert_eq!(h.state().engine.history_len(), 1);
    }
}
