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
use anim_core::ids::{ColumnId, CutId, NodeId, SceneId};
use anim_core::manga::{self, Page, Poly, RulerHandle, Rulers};
use anim_core::model::Cut;
use eframe::egui;

use crate::config::Action;
use crate::doc::AppState;
use crate::lessons::Target;
use crate::plate;
use crate::vocab;

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
    /// AU22: the `Words` field while it is being typed in, and whose words
    /// they are: the page changes once, when it lets go.
    pub typing: Option<(NodeId, String)>,
    /// AU22: the words it let go, for the pane to write once no stroke is live.
    pub typed: Option<(NodeId, String)>,
    /// AU22: the field stood this frame. Gone, it can no longer let go, so its
    /// words land without it ([`land_words`]).
    pub words_shown: bool,
    /// AU22: `Add balloon` or `Add text` hands the field the keys.
    pub focus_words: bool,
    /// AU22: the mode `Mono` stands over, for when it goes off.
    pub over_mono: PageMode,
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
            typing: None,
            typed: None,
            words_shown: false,
            focus_words: false,
            over_mono: PageMode::Color,
            asked: Vec::new(),
        }
    }
}

/// AU9: what the page worker hands back: how many files, and the report made
/// from them on its side of the channel.
pub type PagesOut = Result<(usize, Option<crate::render_queue::Landed>), String>;

/// GM18: the book's pages on their way out — rendered and written off the
/// UI thread (NEVER-DO 6); the status line says what was written.
pub struct PageExport {
    /// The folder they go into.
    pub dir: PathBuf,
    pub rx: std::sync::mpsc::Receiver<PagesOut>,
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

/// AU22: a drag on the selected generator's handle, in page px: where it was
/// pressed and where the pointer is now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LetterDrag {
    pub from: [f32; 2],
    pub to: [f32; 2],
}

/// A split drawn within this many degrees of level or upright is cut
/// exactly level or upright.
const AXIS_SNAP_DEG: f32 = 4.0;

/// AU22: what `Add text` sets until words are typed: a stand-in the page shows
/// and a person replaces, so the button is seen to have done something.
pub const TEXT_START: &str = "Your words";
/// AU22: `See the page` on a machine that cannot compose it.
pub const SEE_PAGE_NO_GPU: &str = "See the page needs a graphics card: showing the drawing.";
/// AU22: the lane's words as it turns on.
pub const SEE_PAGE_SAYS: &str = "See the page: this is how it prints.";
/// AU22: the drawing view shows no speed lines, so `Speed lines` says where
/// they are. A balloon and words show as they are added.
pub const SPEED_LINES_SAY: &str = "Speed lines added. See the page shows them.";
/// One threshold, read on this pane and on the Export step.
pub const THRESHOLD_CARD: &str = "Mono: a pixel greyer than this prints black, the rest white (0.5 is half grey).";
/// One card for the same latch on the Export step.
pub const MONO_CARD: &str = "Black or white, nothing between. Shows in the exported file.";
/// AU22: the Export step's word for the third mode, which only this pane sets.
pub const GREY_NOTE: &str = "Grey, set on the Manga step";

/// AU22: how far across a balloon its words run before they wrap, and how much
/// of its height they may take there, an ellipse being narrower up and down.
const WRAP_IN_BALLOON: f32 = 0.8;
const TALL_IN_BALLOON: f32 = 0.6;

/// AU22: the field's room from the pane's scrollbar.
const WORDS_MARGIN: f32 = 12.0;

/// AU22: the largest words the canvas draws, points: each size is its own
/// glyphs in the atlas, and a page at full zoom asks for a few hundred.
const WORDS_MAX_PT: f32 = 96.0;

/// AU22: `manga.panel.<n>`: a handle is a `&'static str`, and the lessons cut
/// a page into no more than this.
const PANEL_HANDLES: [&str; 12] = [
    "manga.panel.1", "manga.panel.2", "manga.panel.3", "manga.panel.4", "manga.panel.5", "manga.panel.6",
    "manga.panel.7", "manga.panel.8", "manga.panel.9", "manga.panel.10", "manga.panel.11", "manga.panel.12",
];

/// AU22: the three verbs, each with its handle and card.
const LETTER_VERBS: [(GenKind, &str, &str, &str, &str); 3] = [
    (GenKind::Balloon, vocab::ADD_BALLOON, "balloon", "manga.balloon", "A speech balloon in the selected panel."),
    (GenKind::Text, vocab::ADD_TEXT, "text", "manga.text", "A line of words in the selected panel."),
    (GenKind::SpeedLines, vocab::SPEED_LINES, "speed lines", "manga.speed_lines", "Speed lines across the page."),
];

/// What Clip to panels does to a graph shaped by hand (its button's and
/// menu item's DANGER line).
pub const CLIP_DANGER: &str = "rebuilds the page graph; nodes you added are removed — Ctrl+Z restores";

/// How a tone column is told: by its name.
const TONE_NAMING: &str = "A track whose name starts TONE is a tone track: Clip to panels \
                           puts its art through a Tone, the screen dots; every other track's art stays as drawn.";

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
    /// AU22: each verb's handle, in that order.
    const HANDLES: [&str; 4] = ["manga.split_across", "manga.split_down", "manga.add_panel", "manga.delete_panel"];

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
                "A split left to right through the selected panel's middle: an upper and a lower panel, \
                 the row gutter between them."
            }
            Self::SplitDown => {
                "A split top to bottom through the selected panel's middle: a left and a right panel, \
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
        state.refuse("refused — this shot is no page (New page makes one)");
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
                state.refuse("refused — split panel: a part would be empty (split nearer the middle)");
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

/// AU22: the page's generators other than its Panels, bottom of the stack
/// first: what the pane lists and a rebuild keeps.
pub fn letters(cut: &Cut) -> Vec<(NodeId, GenKind)> {
    let kind = |n: &anim_core::graph::Node| match n.kind {
        NodeKind::Generator { generator, .. } => Some((n.id, generator)),
        _ => None,
    };
    cut.graph.nodes().filter(|n| manga::is_free_generator(n)).filter_map(kind).collect()
}

/// `kind`'s generator values (None: it is no generator).
fn values_of(kind: &NodeKind) -> Option<GenValues> {
    match kind {
        NodeKind::Generator { generator, params, binds } => Some(GenValues::resolve(*generator, params, binds, |_| None)),
        _ => None,
    }
}

/// Node `id`'s generator values (None: it is no generator).
fn values(cut: &Cut, id: NodeId) -> Option<GenValues> {
    values_of(&cut.graph.node(id).ok()?.kind)
}

/// AU22: the generator the pane's rows and the canvas's handle act on: the
/// graph's selection, while it is one of this page's.
fn selected_letter(state: &AppState) -> Option<(NodeId, GenValues)> {
    let id = state.selected_node?;
    let cut = state.cut();
    let page = cut.meta.contains_key(manga::PAGE_KEY);
    let free = cut.graph.node(id).is_ok_and(manga::is_free_generator);
    (page && free).then(|| values(cut, id).map(|v| (id, v))).flatten()
}

/// Does `kind` stand at a place on the page, with a handle to move it?
fn placed(kind: GenKind) -> bool {
    matches!(kind, GenKind::Balloon | GenKind::Text)
}

/// The page's size in px.
fn page_size(state: &AppState) -> (f32, f32) {
    (state.engine.project.width as f32, state.engine.project.height as f32)
}

/// AU22: the selected generator's handle in page px, on the top edge of its
/// box, clear of the words (None: nothing to hold, or the page's guides are
/// off).
pub fn letter_handle(state: &AppState) -> Option<[f32; 2]> {
    if !state.manga.guides {
        return None;
    }
    let (_, v) = selected_letter(state)?;
    let [x0, y0, x1, _] = letter_box(page_size(state), &v)?;
    Some([(x0 + x1) / 2.0, y0])
}

/// AU22: what a press at page px `p` takes hold of. The selected generator by
/// its handle, within `reach`; with `whole` (the Brush, which has no handle of
/// its own to find) the selected one anywhere in its box, else the topmost box
/// there, a balloon standing for the words in it.
pub fn letter_under(state: &AppState, p: [f32; 2], reach: f32, whole: bool) -> Option<NodeId> {
    if !state.manga.guides {
        return None;
    }
    let size = page_size(state);
    let cut = state.cut();
    let holds = |v: &GenValues| {
        letter_box(size, v).is_some_and(|[x0, y0, x1, y1]| (x0..=x1).contains(&p[0]) && (y0..=y1).contains(&p[1]))
    };
    let near = letter_handle(state).is_some_and(|h| (h[0] - p[0]).hypot(h[1] - p[1]) <= reach);
    if let Some((id, v)) = selected_letter(state)
        && (near || (whole && holds(&v)))
    {
        return Some(id);
    }
    if !whole {
        return None;
    }
    let top = letters(cut).into_iter().rev().find(|(id, _)| {
        values(cut, *id).is_some_and(|v| holds(&v) && (v.kind != GenKind::Text || owner_of(cut, &v).is_none()))
    });
    top.map(|(id, _)| id)
}

/// The balloon Text `t` belongs to: of those it lies in, the one whose centre
/// is nearest, so a balloon set down over another's words does not take them.
fn owner_of(cut: &Cut, t: &GenValues) -> Option<NodeId> {
    let balloons = letters(cut).into_iter().filter(|(_, kind)| *kind == GenKind::Balloon);
    let lying = balloons.filter_map(|(id, _)| {
        let b = values(cut, id)?;
        let (dx, dy) = (t.float("x") - b.float("x"), t.float("y") - b.float("y"));
        (dx.abs() <= b.float("width") / 2.0 && dy.abs() <= b.float("height") / 2.0).then_some((dx.hypot(dy), id))
    });
    lying.min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, id)| id)
}

/// The Text generators belonging to `balloon`, nearest its centre first: a
/// balloon has no words of its own, so its lettering is the words given to it.
fn lettering_of(cut: &Cut, balloon: NodeId) -> Vec<NodeId> {
    let Some(b) = values(cut, balloon).filter(|b| b.kind == GenKind::Balloon) else {
        return Vec::new();
    };
    let mut found: Vec<(f32, NodeId)> = letters(cut)
        .iter()
        .filter(|(_, kind)| *kind == GenKind::Text)
        .filter_map(|(id, _)| {
            let t = values(cut, *id)?;
            let (dx, dy) = (t.float("x") - b.float("x"), t.float("y") - b.float("y"));
            (owner_of(cut, &t) == Some(balloon)).then_some((dx.hypot(dy), *id))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, id)| id).collect()
}

/// `id`'s fractions moved by (`dx`, `dy`), a balloon's tail with them, each
/// held to its parameter's range: one `SetNodeKind`.
fn shifted(cut: &Cut, at: CutRef, id: NodeId, dx: f32, dy: f32) -> Option<Command> {
    let v = values(cut, id)?;
    let mut kind = cut.graph.node(id).ok()?.kind.clone();
    let NodeKind::Generator { params, .. } = &mut kind else {
        return None;
    };
    for (key, d) in [("x", dx), ("y", dy)] {
        params.set(key, FxValue::Float((v.float(key) + d).clamp(-2.0, 3.0)));
    }
    if v.kind == GenKind::Balloon {
        for (key, d) in [("tail_x", dx), ("tail_y", dy)] {
            params.set(key, FxValue::Float((v.float(key) + d).clamp(-2.0, 3.0)));
        }
    }
    Some(Command::SetNodeKind { at, id, kind })
}

/// AU22: the handle's drag lands: the selected generator moves by the paper
/// delta, a balloon with the words in it, as one undo step.
pub fn letter_moved(state: &mut AppState, k: LetterDrag) {
    let Some((id, v)) = selected_letter(state) else {
        return;
    };
    let (w, h) = (state.engine.project.width as f32, state.engine.project.height as f32);
    let (dx, dy) = ((k.to[0] - k.from[0]) / w, (k.to[1] - k.from[1]) / h);
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    let mut moved = vec![id];
    if v.kind == GenKind::Balloon {
        moved.extend(lettering_of(state.cut(), id));
    }
    state.manga_edit("move lettering", |cut, at| moved.iter().filter_map(|id| shifted(cut, at, *id, dx, dy)).collect());
}

/// The words generator `id` shows in the `Words` field: none while it holds
/// the stand-in.
fn words_of(cut: &Cut, id: NodeId, v: &GenValues) -> String {
    let own = |t: &GenValues| Some(t.text("text").to_string()).filter(|w| w != TEXT_START);
    match v.kind {
        GenKind::Text => own(v).unwrap_or_default(),
        GenKind::Balloon => lettering_of(cut, id).first().and_then(|id| own(&values(cut, *id)?)).unwrap_or_default(),
        _ => String::new(),
    }
}

/// A new Text on `page`, big enough to read at Fit.
fn text_start(page: &Page, x: f32, y: f32, words: &str) -> NodeKind {
    let mut node = NodeKind::generator(GenKind::Text);
    if let NodeKind::Generator { params, .. } = &mut node {
        params.set("text", FxValue::Text { text: words.into() });
        params.set("x", FxValue::Float(x));
        params.set("y", FxValue::Float(y));
        params.set("size", FxValue::Float(page.px(5.0).max(28.0)));
    }
    node
}

/// AU22: a new `kind` where a person is working: in the selected panel, each
/// one a step down from the last of its kind, at this page's scale. The kinds'
/// stored defaults are a video frame's, too thin to see on a 350 dpi page.
fn letter_start(cut: &Cut, kind: GenKind, panel: usize, page: &Page) -> NodeKind {
    const STEP: f32 = 0.04;
    let (w, h) = (page.width as f32, page.height as f32);
    let polys = manga::panels_of(cut).map(|(_, p)| p).unwrap_or_default();
    let [x0, y0, x1, y1] = polys.get(panel).map_or_else(|| bounds(&page.first_panel()), |p| bounds(p));
    let nth = letters(cut).iter().filter(|(_, k)| *k == kind).count() as f32;
    let (x, y) = ((x0 + x1) / 2.0 / w + nth * STEP, (y0 + y1) / 2.0 / h + nth * STEP);
    if kind == GenKind::Text {
        return text_start(page, x, y, TEXT_START);
    }
    let mut node = NodeKind::generator(kind);
    if let NodeKind::Generator { params, .. } = &mut node {
        let mut set = |key: &str, v: f32| params.set(key, FxValue::Float(v));
        if kind == GenKind::Balloon {
            let (bw, bh) = (((x1 - x0) / w * 0.6).min(0.3), ((y1 - y0) / h * 0.5).min(0.16));
            set("x", x);
            set("y", y);
            set("width", bw);
            set("height", bh);
            set("tail_x", x + bw * 0.2);
            set("tail_y", y + bh * 0.9);
            set("stroke_width", page.px(manga::BORDER_MM).max(3.0));
        } else {
            set("thickness_min", page.px(0.2).max(1.0));
            set("thickness_max", page.px(0.8).max(4.0));
        }
    }
    node
}

/// AU22: a verb of the pane: `kind` on the page, one undo step, selected.
fn add_letter(state: &mut AppState, kind: GenKind, label: &str) {
    let Some(page) = Page::of(state.cut()) else {
        state.refuse("refused — this shot is no page (New page makes one)");
        return;
    };
    let node = letter_start(state.cut(), kind, state.manga.selected, &page);
    let added = state.add_to_page(label, node, false, None).is_some();
    if added && kind == GenKind::SpeedLines {
        state.status = SPEED_LINES_SAY.into();
    }
    state.manga.focus_words = added && placed(kind);
}

/// `words` in a Text of kind `text`, wrapped to `balloon` if it has one.
fn with_words(text: &NodeKind, words: &str, balloon: Option<&GenValues>) -> NodeKind {
    let mut kind = text.clone();
    if let NodeKind::Generator { params, .. } = &mut kind {
        params.set("text", FxValue::Text { text: words.into() });
        if let Some(b) = balloon {
            params.set("wrap", FxValue::Float(b.float("width") * WRAP_IN_BALLOON));
        }
    }
    kind
}

/// `balloon`'s kind grown down to hold `text` (None: it already does). Words
/// wrapped to [`WRAP_IN_BALLOON`] of its width stand in an ellipse's middle,
/// where it is [`TALL_IN_BALLOON`] of its height. The tail stays as far out.
fn grown_for(size: (f32, f32), kind: &NodeKind, text: &NodeKind) -> Option<NodeKind> {
    let (b, t) = (values_of(kind)?, values_of(text)?);
    let (w, h) = size;
    let block = anim_core::text::layout(t.text("text"), &anim_core::generator::text_style(&t, w as u32, h as u32));
    let need = (block.h / h / TALL_IN_BALLOON).min(4.0);
    let (height, y) = (b.float("height"), b.float("y"));
    if need <= height {
        return None;
    }
    let mut kind = kind.clone();
    if let NodeKind::Generator { params, .. } = &mut kind {
        let out = (b.float("tail_y") - y).signum() * (need - height) / 2.0;
        params.set("height", FxValue::Float(need));
        params.set("tail_y", FxValue::Float((b.float("tail_y") + out).clamp(-2.0, 3.0)));
    }
    Some(kind)
}

/// AU22: `words` on generator `id`, one undo step, a balloon grown to hold
/// them in it. A balloon has no words of its own, so the first it is given
/// stand in a new Text at its centre, wrapped to it, and the balloon stays
/// selected. Words the page already holds are no step.
fn set_words(state: &mut AppState, id: NodeId, words: &str) {
    let size = page_size(state);
    let cut = state.cut();
    let Some(v) = values(cut, id).filter(|v| placed(v.kind)) else {
        return;
    };
    if words == words_of(cut, id, &v) {
        return;
    }
    let balloon = (v.kind == GenKind::Balloon).then_some(&v);
    let kind_of = |id: NodeId| cut.graph.node(id).ok().map(|n| n.kind.clone());
    let text = if balloon.is_some() { lettering_of(cut, id).first().copied() } else { Some(id) };
    if let Some(text) = text {
        let Some(worded) = kind_of(text).map(|kind| with_words(&kind, words, balloon)) else {
            return;
        };
        let grown = balloon.and_then(|_| grown_for(size, &kind_of(id)?, &worded));
        state.manga_edit("words", |_, at| {
            let mut cmds = vec![Command::SetNodeKind { at, id: text, kind: worded }];
            cmds.extend(grown.map(|kind| Command::SetNodeKind { at, id, kind }));
            cmds
        });
    } else if !words.trim().is_empty() {
        let Some(page) = Page::of(cut) else {
            return;
        };
        let node = with_words(&text_start(&page, v.float("x"), v.float("y"), words), words, balloon);
        let grown = kind_of(id).and_then(|kind| grown_for(size, &kind, &node)).map(|kind| (id, kind));
        state.add_to_page("words", node, true, grown);
    }
}

/// AU22: words still in the field land before anything reads the page or takes
/// the pane away: a step's pill runs ahead of the dock, so the field is gone
/// that frame and never lets go, and a save fires over it.
pub fn land_words(state: &mut AppState) {
    let (typed, typing) = (state.manga.typed.take(), state.manga.typing.take());
    for (id, words) in typed.into_iter().chain(typing) {
        set_words(state, id, &words);
    }
}

/// `key`, a number generator `id` holds, becomes `v`: one `SetNodeKind`.
fn set_float(cut: &Cut, at: CutRef, id: NodeId, key: &str, v: f32) -> Vec<Command> {
    let Ok(node) = cut.graph.node(id) else {
        return Vec::new();
    };
    let mut kind = node.kind.clone();
    if let NodeKind::Generator { params, .. } = &mut kind {
        params.set(key, FxValue::Float(v));
    }
    vec![Command::SetNodeKind { at, id, kind }]
}

/// AU22: the page's two template tracks, INK and TONE (None: it lacks one).
fn ink_tone(cut: &Cut) -> Option<(ColumnId, ColumnId)> {
    let ink = cut.xsheet.columns.iter().find(|c| c.name.trim().eq_ignore_ascii_case("INK"))?;
    let tone = cut.xsheet.columns.iter().find(|c| manga::is_tone_column(cut, c.id))?;
    Some((ink.id, tone.id))
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
    /// AU22: the track a stroke lands on.
    Track(ColumnId),
    /// AU22: one of the three verbs, and its word for the undo list.
    Add(GenKind, &'static str),
    /// AU22: the generator the rows and the handle act on.
    Pick(NodeId),
    /// AU22: a number of the selected generator, and its word for the undo list.
    Float(NodeId, &'static str, f32),
    /// AU22: the selected generator off the page.
    Remove(NodeId, GenKind),
}

/// `gesture_safe`: no stroke is live (an edit is refused while one is).
/// `koma_armed`: the canvas holds the panel tool (its latch shows it).
/// `see_page`: the canvas shows the composed page (its latch shows it).
pub fn ui(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool, koma_armed: bool, see_page: bool) {
    let mut edit = None;
    let scroll = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| pane(ui, state, koma_armed, see_page, &mut edit));
    plate::note_viewport(ui, scroll.inner_rect, [false, true]);
    if !gesture_safe {
        // The words wait out the stroke: only the verb is refused.
        if edit.is_some() {
            state.refuse("refused — finish the stroke first");
        }
        return;
    }
    // AU22: a press elsewhere is what lets `Words` go: both land, the words first.
    if let Some((id, words)) = state.manga.typed.take() {
        set_words(state, id, &words);
    }
    let Some(edit) = edit else {
        return;
    };
    match edit {
        Edit::Goto(scene, cut) => state.goto_cut(scene, cut),
        Edit::Panels(op) => panels(state, op),
        Edit::Tone(key, v) => {
            if tone_now(state.cut()).is_none() {
                state.refuse("refused — the page has no tone (a TONE track, then Clip to panels)");
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
            (None, _) => state.refuse("refused — this shot is no page (New page makes one)"),
            (_, None) => state.refuse("refused — the page has no panels (Clip to panels makes them)"),
        },
        Edit::Track(column) => state.view.active_column = column,
        Edit::Add(kind, label) => add_letter(state, kind, label),
        Edit::Pick(id) => state.selected_node = Some(id),
        Edit::Float(id, key, v) => {
            state.manga_edit("speed lines", |cut, at| set_float(cut, at, id, key, v));
        }
        Edit::Remove(id, kind) => {
            let mut gone = vec![id];
            if kind == GenKind::Balloon {
                gone.extend(lettering_of(state.cut(), id));
            }
            state.remove_from_page(&kind.name().to_lowercase(), &gone);
        }
    }
}

/// AU22: the page's generators, one row each, and under the selected one what
/// it is given here: `Remove`, its `Words`, or its direction and count. The
/// rest of a generator stays the Effects graph's.
fn letter_rows(ui: &mut egui::Ui, state: &mut AppState, edit: &mut Option<Edit>) {
    let rows = letters(state.cut());
    let selected = selected_letter(state);
    let by_hand = state.page_hand_built().is_some();
    ui.horizontal_wrapped(|ui| {
        for (i, (id, kind)) in rows.iter().enumerate() {
            let nth = rows[..=i].iter().filter(|(_, k)| k == kind).count();
            if plate::detent(ui, state.selected_node == Some(*id), &format!("{} {nth}", kind.name())).clicked() {
                *edit = Some(Edit::Pick(*id));
            }
        }
        if let Some((id, v)) = &selected {
            let r = ui.add_enabled(!by_hand, egui::Button::new("Remove"));
            plate::speak(&r, egui::WidgetType::Button, None, "Remove lettering");
            plate::mark_row(&r, Target::Named("manga.remove"));
            let more = "Takes it off the page. A balloon's words go with it.";
            if r.on_hover_text(more).on_disabled_hover_text(crate::doc::REMOVE_BY_HAND).clicked() {
                *edit = Some(Edit::Remove(*id, v.kind));
            }
        }
    });
    if let Some((id, v)) = selected.as_ref().filter(|(_, v)| v.kind == GenKind::SpeedLines) {
        speed_rows(ui, state, *id, v, edit);
    }
    let Some((id, v)) = selected.filter(|(_, v)| placed(v.kind)) else {
        if let Some(typing) = state.manga.typing.take() {
            state.manga.typed = Some(typing);
        }
        return;
    };
    // The selection left words behind in the field: they are the last letter's.
    if state.manga.typing.as_ref().is_some_and(|(of, _)| *of != id) {
        state.manga.typed = state.manga.typing.take();
    }
    let now = words_of(state.cut(), id, &v);
    state.manga.words_shown = true;
    ui.horizontal(|ui| {
        plate::legend(ui, "words");
        let waiting = state.manga.typing.as_ref().or(state.manga.typed.as_ref()).filter(|(of, _)| *of == id);
        let mut text = waiting.map_or_else(|| now.clone(), |(_, words)| words.clone());
        let r = plate::text_field(
            ui,
            egui::TextEdit::singleline(&mut text)
                .id_salt("manga words")
                .hint_text(TEXT_START)
                .desired_width((ui.available_width() - WORDS_MARGIN).max(0.0)),
        );
        plate::speak(&r, egui::WidgetType::TextEdit, None, "Words");
        plate::mark_row(&r, Target::Named("manga.words"));
        // The rows above grow with the page: keys need a field that shows.
        if std::mem::take(&mut state.manga.focus_words) {
            r.request_focus();
            r.scroll_to_me(None);
        }
        // egui lets a field go on a click, and a stroke on the paper is no
        // click: the next key would type here. Any press elsewhere lets go.
        if r.has_focus() && !r.hovered() && ui.input(|i| i.pointer.any_pressed()) {
            r.surrender_focus();
        }
        if r.has_focus() {
            state.manga.typing = Some((id, text.clone()));
        }
        if r.lost_focus() {
            state.manga.typing = None;
            state.manga.typed = (text != now).then_some((id, text));
        }
    });
}

/// AU22: the two things a person asks of speed lines on a page: which way, and
/// how many. One undo each, when the field lands.
fn speed_rows(ui: &mut egui::Ui, state: &mut AppState, id: NodeId, v: &GenValues, edit: &mut Option<Edit>) {
    const WAY: &str = "The lines' direction in degrees: 0 runs across, 90 runs down.";
    const MANY: &str = "How many lines.";
    let rows = [
        ("direction", "Lines direction", "angle", -360.0..=360.0, "°", "manga.speed.direction", WAY),
        ("lines", "Lines count", "count", 0.0..=2000.0, "", "manga.speed.count", MANY),
    ];
    for (word, name, key, range, suffix, handle, card) in rows {
        ui.horizontal(|ui| {
            plate::legend(ui, word).on_hover_text(card);
            let (r, x) = held_field(ui, &mut state.manga.held, key, v.float(key), range, 1.0, suffix);
            plate::speak(&r, egui::WidgetType::DragValue, None, name);
            plate::mark_row(&r, Target::Named(handle));
            r.on_hover_text(card);
            // A press that lands where it stood is no edit.
            if let Some(x) = x.filter(|x| *x != v.float(key)) {
                *edit = Some(Edit::Float(id, key, x));
            }
        });
    }
}

fn pane(ui: &mut egui::Ui, state: &mut AppState, koma_armed: bool, see_page: bool, edit: &mut Option<Edit>) {
    plate::legend(ui, vocab::MANGA_PANE);
    let page = Page::of(state.cut());
    let not_a_book = state.new_page_refusal();
    match &page {
        Some(p) => {
            plate::value(ui, &format!("{} · {}×{} at {} dpi", p.format, p.width, p.height, p.dpi));
        }
        None => {
            let why = not_a_book.unwrap_or("this shot is no page — New page makes one");
            ui.label(egui::RichText::new(why).color(plate::legend_dim()));
        }
    }
    ui.horizontal(|ui| {
        plate::latch(ui, &mut state.manga.guides, vocab::PAGE_GUIDES).on_hover_text(
            "The canvas draws the page's bleed edge, trim and inner frame, its panels and its rulers \
             (the Panel tool shows them while it is on, whatever this says).",
        );
        if page.is_some() {
            let mut on = see_page;
            let latch = plate::latch(ui, &mut on, vocab::SEE_THE_PAGE);
            plate::mark_row(&latch, Target::Named("manga.see_page"));
            let more = "The whole page as it will print. Turn it off to draw.";
            if crate::menus::hover_tip(latch, Action::ToggleCompositeView, more).clicked() {
                state.manga.asked.push(Action::ToggleCompositeView);
            }
        }
    });
    // The track a stroke lands on, in a comic's two words.
    if let Some((ink, tone)) = page.as_ref().and_then(|_| ink_tone(state.cut())) {
        let active = state.view.active_column;
        let tracks = [
            (ink, "Ink", "Ink track", "Draw on the INK track: the lines print as drawn."),
            (tone, "Tone", "Tone track", "Draw on the TONE track: its greys print as screen dots."),
        ];
        let row = ui.horizontal(|ui| {
            plate::legend(ui, "Draw on");
            for (column, word, name, help) in tracks {
                let r = plate::detent(ui, active == column, word);
                plate::speak(&r, egui::WidgetType::RadioButton, Some(active == column), name);
                if r.on_hover_text(help).clicked() {
                    *edit = Some(Edit::Track(column));
                }
            }
        });
        plate::mark_row(&row.response, Target::Named("manga.ink_tone"));
    }

    ui.add_space(6.0);
    plate::legend(ui, "panels");
    let panels = manga::panels_of(state.cut()).map(|(_, p)| p).unwrap_or_default();
    if state.manga.selected >= panels.len() {
        state.manga.selected = panels.len().saturating_sub(1);
    }
    // AU22: the page beside its verbs, so they stand clear of the scroll in the
    // pane's short height at 1280×800.
    ui.horizontal_top(|ui| {
        if let Some(p) = &page {
            // The page drawn small; a click selects the panel under it.
            let w = ui.available_width().clamp(60.0, 110.0);
            let (resp, rect, scale) = page_thumb(ui, p, &panels, Some(state.manga.selected), egui::vec2(w, 154.0));
            if let Some(at) = resp.interact_pointer_pos().filter(|_| resp.clicked()) {
                let q = (at - rect.min) / scale;
                if let Some(i) = manga::panel_at(&panels, [q.x, q.y]) {
                    state.manga.selected = i;
                }
            }
        }
        ui.vertical(|ui| {
            for (op, handle) in PanelOp::ALL.into_iter().zip(PanelOp::HANDLES) {
                let r = ui.button(op.label());
                plate::mark_row(&r, Target::Named(handle));
                if r.on_hover_text(op.help()).clicked() {
                    *edit = Some(Edit::Panels(op));
                }
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        for (i, poly) in panels.iter().enumerate() {
            let [x0, y0, x1, y1] = bounds(poly);
            let name = format!("panel {}", i + 1);
            let r = plate::detent(ui, i == state.manga.selected, &name);
            if let Some(handle) = PANEL_HANDLES.get(i) {
                plate::mark_row(&r, Target::Named(handle));
            }
            if r.on_hover_text(format!("{:.0}×{:.0} px", x1 - x0, y1 - y0)).clicked() {
                state.manga.selected = i;
            }
        }
    });
    // AU22: the step's job stands before the book's pages, which grow with it,
    // and the tools that cut a page: it is what a person comes back to for
    // every balloon.
    if page.is_some() {
        ui.add_space(6.0);
        plate::legend(ui, "lettering");
        ui.horizontal_wrapped(|ui| {
            for (kind, word, label, handle, help) in LETTER_VERBS {
                let r = ui.button(word);
                plate::mark_row(&r, Target::Named(handle));
                if r.on_hover_text(help).clicked() {
                    *edit = Some(Edit::Add(kind, label));
                }
            }
        });
        letter_rows(ui, state, edit);
    }

    ui.add_space(6.0);
    plate::legend(ui, "pages");
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

    // The canvas's panel tool: a drag draws, splits or moves a ruler.
    let mut armed = koma_armed;
    let latch = plate::latch(ui, &mut armed, "Panel tool");
    // The rail's key is spoken `Panel tool`; one name, one control.
    plate::speak(&latch, egui::WidgetType::SelectableLabel, Some(armed), "Panel tool on the page");
    if crate::menus::hover_tip(latch, Action::PanelTool, "").clicked() {
        state.manga.asked.push(Action::PanelTool);
    }
    const SIDE: &str = "The gutter between panels side by side — Split down, or a Panel tool drag more down \
                        the page than across: 2 mm is 28 px at 350 dpi.";
    const ROW: &str = "The gutter between rows — Split across, or a Panel tool drag more across the page \
                       than down: 5 mm is 69 px at 350 dpi.";
    ui.horizontal(|ui| {
        plate::legend(ui, "side gutter").on_hover_text(SIDE);
        let r = plate::field(ui, egui::DragValue::new(&mut state.manga.side_mm).range(0.0..=20.0).speed(0.1).suffix(" mm"));
        plate::mark_row(&r, Target::Named("manga.gutter.side"));
        r.on_hover_text(SIDE);
    });
    ui.horizontal(|ui| {
        plate::legend(ui, "row gutter").on_hover_text(ROW);
        let r = plate::field(ui, egui::DragValue::new(&mut state.manga.row_mm).range(0.0..=20.0).speed(0.1).suffix(" mm"));
        plate::mark_row(&r, Target::Named("manga.gutter.row"));
        r.on_hover_text(ROW);
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
            plate::R_CONTROL,
            egui::Stroke::new(1.0, plate::AKA),
            egui::StrokeKind::Inside,
        );
    }
    let more = hand.map_or_else(String::new, |n| format!("DANGER: it {CLIP_DANGER} ({n} nodes now)"));
    if crate::menus::hover_tip(clip, Action::ClipToPanels, &more).clicked() {
        state.manga.asked.push(Action::ClipToPanels);
    }

    ui.add_space(6.0);
    plate::legend(ui, "tone");
    let tone = tone_now(state.cut());
    let hint = match &tone {
        None => "no tone yet — a track named TONE, then Clip to panels",
        Some(_) => "the screen of the TONE tracks",
    };
    ui.label(egui::RichText::new(hint).color(plate::legend_dim())).on_hover_text(TONE_NAMING);
    let lpi = tone.as_ref().map(|v| v.float("lpi"));
    let angle = tone.as_ref().map(|v| v.float("angle"));
    let shape = tone.as_ref().map(|v| v.choice("shape"));
    let presets = ui.horizontal_wrapped(|ui| {
        for (x, label, help) in LPIS {
            if plate::detent(ui, lpi == Some(x), label).on_hover_text(help).clicked() {
                *edit = Some(Edit::Tone("lpi", FxValue::Float(x)));
            }
        }
    });
    plate::mark_row(&presets.response, Target::Named("manga.tone_preset"));
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
            plate::legend(ui, "LPI").on_hover_text(LINES);
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
    let presets = ui.horizontal_wrapped(|ui| {
        for (points, (label, help)) in RULERS.iter().enumerate() {
            if plate::detent(ui, rulers.vps.len() == points, label).on_hover_text(*help).clicked() {
                *edit = Some(Edit::Rulers(points));
            }
        }
    });
    plate::mark_row(&presets.response, Target::Named("manga.rulers"));
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
            plate::legend(ui, &format!("VP {}", k + 1)).on_hover_text(PLACE);
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
            plate::legend(ui, crate::vocab::HORIZON_Y).on_hover_text(LEVEL);
            let (r, y) = held_field(ui, &mut state.manga.held, "horizon y", h, -1.0e6..=1.0e6, 1.0, "");
            r.on_hover_text(LEVEL);
            if let Some(y) = y {
                *edit = Some(Edit::Ruler(RulerHandle::Horizon, [0.0, y]));
            }
        });
    }

    ui.add_space(6.0);
    plate::legend(ui, "export (nyūkō)");
    let MangaState { export: o, over_mono, .. } = &mut state.manga;
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
                *over_mono = if o.mode == PageMode::Mono { *over_mono } else { o.mode };
                o.mode = m;
            }
        }
    });
    ui.horizontal(|ui| {
        plate::legend(ui, "threshold").on_hover_text(THRESHOLD_CARD);
        let mono = o.mode == PageMode::Mono;
        ui.add_enabled_ui(mono, |ui| {
            plate::field(ui, egui::DragValue::new(&mut o.threshold).range(0.0..=1.0).speed(0.01).max_decimals(2))
        })
        .inner
        .on_hover_text(THRESHOLD_CARD)
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
            egui::Stroke::new(1.0, plate::INK3)
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
    let dim = egui::Stroke::new(1.0, plate::INK3);
    let ao = egui::Stroke::new(1.0, plate::PENCIL_AO);
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
    let faint = egui::Stroke::new(1.0, plate::PENCIL_AO.gamma_multiply(0.35));
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

/// AU22: what a balloon, its tail with it, or a block of words takes up, in
/// page px. The Brush's press lands in it and the selected one is drawn round.
fn letter_box(project: (f32, f32), v: &GenValues) -> Option<[f32; 4]> {
    if !placed(v.kind) {
        return None;
    }
    let (w, h) = project;
    let (x, y) = (v.float("x") * w, v.float("y") * h);
    match v.kind {
        GenKind::Balloon => {
            let (half_w, half_h) = (v.float("width") * w / 2.0, v.float("height") * h / 2.0);
            let body = [x - half_w, y - half_h, x + half_w, y + half_h];
            let (tx, ty) = (v.float("tail_x") * w, v.float("tail_y") * h);
            Some(if v.flag("tail") { [body[0].min(tx), body[1].min(ty), body[2].max(tx), body[3].max(ty)] } else { body })
        }
        GenKind::Text => {
            let style = anim_core::generator::text_style(v, w as u32, h as u32);
            let block = anim_core::text::layout(v.text("text"), &style);
            let (half_w, half_h) = (block.w.max(v.float("size")) / 2.0, block.h.max(v.float("size")) / 2.0);
            Some([x - half_w, y - half_h, x + half_w, y + half_h])
        }
        _ => None,
    }
}

/// `v`, a balloon, as a thin line through `pt` (page px to screen): its body
/// and its tail from the body's edge to the tip. What prints is the graph's;
/// this says where it stands.
fn paint_balloon(painter: &egui::Painter, v: &GenValues, size: (f32, f32), pt: &impl Fn(f32, f32) -> egui::Pos2, line: egui::Stroke) {
    let (w, h) = size;
    let (cx, cy) = (v.float("x") * w, v.float("y") * h);
    let (rx, ry) = ((v.float("width") * w / 2.0).max(1.0), (v.float("height") * h / 2.0).max(1.0));
    let round = (0..=64).map(|n| {
        let at = n as f32 / 64.0 * std::f32::consts::TAU;
        pt(cx + rx * at.cos(), cy + ry * at.sin())
    });
    painter.add(egui::Shape::line(round.collect(), line));
    if !v.flag("tail") {
        return;
    }
    let (tx, ty) = (v.float("tail_x") * w, v.float("tail_y") * h);
    let (dx, dy) = (tx - cx, ty - cy);
    let long = dx.hypot(dy).max(1e-3);
    let half = v.float("tail_width") * w / 2.0;
    // Where the side from the tail's root to its tip leaves the ellipse.
    let edge = |side: f32| {
        let (bx, by) = (cx - dy / long * half * side, cy + dx / long * half * side);
        let (ax, ay) = ((bx - cx) / rx, (by - cy) / ry);
        let (ux, uy) = ((tx - bx) / rx, (ty - by) / ry);
        let (a, b, c) = (ux * ux + uy * uy, ax * ux + ay * uy, ax * ax + ay * ay - 1.0);
        let t = ((-b + (b * b - a * c).max(0.0).sqrt()) / a.max(1e-6)).clamp(0.0, 1.0);
        pt(bx + (tx - bx) * t, by + (ty - by) * t)
    };
    painter.add(egui::Shape::line(vec![edge(1.0), pt(tx, ty), edge(-1.0)], line));
}

/// `v`, a Text, as its words where it stands, sized and turned with the view:
/// the file's letters are the graph's and a screen's are near enough to read.
fn paint_words(painter: &egui::Painter, v: &GenValues, width: f32, at: egui::Pos2, turn: egui::Vec2) {
    let zoom = turn.length();
    let px = (v.float("size") * zoom).round().clamp(6.0, WORDS_MAX_PT);
    // The wrap as far across as the letters are, when they are held small.
    let wrap = v.float("wrap") * width * px / v.float("size").max(1.0);
    let wrap = if wrap > 0.0 { wrap } else { f32::INFINITY };
    let job = egui::text::LayoutJob::simple(v.text("text").to_string(), egui::FontId::proportional(px), plate::INK2, wrap);
    let galley = painter.layout_job(job);
    let top_left = at - galley.size() / 2.0;
    let shape = egui::epaint::TextShape::new(top_left, galley, plate::INK2);
    painter.add(shape.with_angle_and_anchor(turn.y.atan2(turn.x), egui::Align2::CENTER_CENTER));
}

/// AU22: the page's lettering over the drawing, which shows no graph: each
/// balloon as a thin line and each block of words as its words, the selected
/// one in blue in a dashed box with its handle, where the drag holding it has
/// it (a balloon's words go with it). `printed`: See the page, which shows them
/// as they print, so only the handle stands, and only under the pointer
/// (`over`, page px).
pub fn paint_letters(
    painter: &egui::Painter,
    state: &AppState,
    to_screen: &impl Fn(egui::Pos2) -> egui::Pos2,
    drag: Option<&LetterDrag>,
    printed: bool,
    over: Option<[f32; 2]>,
) {
    if !state.manga.guides {
        return;
    }
    let cut = state.cut();
    let size = page_size(state);
    let moved = drag.map_or([0.0; 2], |k| [k.to[0] - k.from[0], k.to[1] - k.from[1]]);
    let picked = selected_letter(state).map(|(id, _)| id);
    let carried = picked.map(|id| lettering_of(cut, id)).unwrap_or_default();
    let turn = to_screen(egui::pos2(1.0, 0.0)) - to_screen(egui::pos2(0.0, 0.0));
    for (id, _) in letters(cut) {
        let Some(v) = values(cut, id) else {
            continue;
        };
        let Some([x0, y0, x1, y1]) = letter_box(size, &v) else {
            continue;
        };
        let on = picked == Some(id);
        let [dx, dy] = if on || carried.contains(&id) { moved } else { [0.0; 2] };
        let pt = |x: f32, y: f32| to_screen(egui::pos2(x + dx, y + dy));
        let line = egui::Stroke::new(1.0, if on { plate::BLUE } else { plate::INK3 });
        if !printed {
            match v.kind {
                GenKind::Balloon => paint_balloon(painter, &v, size, &pt, line),
                _ => paint_words(painter, &v, size.0, pt((x0 + x1) / 2.0, (y0 + y1) / 2.0), turn),
            }
        }
        let under = over.is_some_and(|p| (x0..=x1).contains(&p[0]) && (y0..=y1).contains(&p[1]));
        if on && (!printed || under || drag.is_some()) {
            if !printed {
                let corners = [pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1), pt(x0, y0)];
                painter.extend(egui::Shape::dashed_line(&corners, line, 5.0, 4.0));
            }
            let c = pt((x0 + x1) / 2.0, y0);
            painter.circle_filled(c, 6.0, plate::PAPER);
            painter.circle_stroke(c, 6.0, egui::Stroke::new(2.0, plate::BLUE));
        }
    }
}

/// The panel tool's `op` on the open page as one undo step.
fn panels(state: &mut AppState, op: PanelOp) {
    let Some(page) = Page::of(state.cut()) else {
        state.refuse("refused — this shot is no page (New page makes one)");
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

    /// The panel outline is a mark on paper: a surface token would vanish on
    /// the white sheet, and this one holds the non-text floor.
    #[test]
    fn aulook_manga_page_outline_shows_on_paper() {
        use crate::measure::{effective, ratio};
        use egui::epaint::ColorMode;
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(300.0, 400.0)).build_ui(|ui| {
            let page = Page::b5();
            let panels = vec![page.first_panel()];
            page_thumb(ui, &page, &panels, None, egui::vec2(240.0, 320.0));
        });
        h.run_steps(2);
        let outlines: Vec<egui::Color32> = (h.output().shapes.iter())
            .filter_map(|c| match &c.shape {
                egui::Shape::Path(p) if p.closed => Some(p.stroke.color.clone()),
                _ => None,
            })
            .filter_map(|c| if let ColorMode::Solid(c) = c { Some(c) } else { None })
            .collect();
        assert_eq!(outlines, [plate::LEGEND, plate::LEGEND, plate::INK3], "the trim, the inner frame, the panel");
        let paper = effective(plate::PAPER, 1.0, [0.0; 3]);
        let got = ratio(effective(plate::INK3, 1.0, paper), paper);
        assert!(got >= 3.0, "INK3 on PAPER: {got:.2}");
    }

    /// The editor on a fresh Manga B5 book, in its room.
    fn b5() -> EditorHarness {
        let mut h = EditorHarness::new();
        *h.editor_mut() = crate::open_template(template("Manga B5"), None, None, &Default::default());
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
            crate::vocab::MANGA_PANE,
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
        assert_eq!(Action::MangaRoom.menu(), Some("More/Manga"));
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
        assert!(h.has("VP 1") && h.has(crate::vocab::HORIZON_Y), "and the pane places them by number");
        // The guides latch hides them; the panel tool shows them again.
        h.scroll_to(crate::vocab::PAGE_GUIDES);
        h.click(crate::vocab::PAGE_GUIDES);
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
        // The menu's entry says it too (the pane out of the dock).
        h.editor_mut().dock = egui_dock::DockState::new(vec![crate::workspace::Pane::Canvas]);
        h.run();
        assert!(!h.has_text_containing(CLIP_DANGER), "the pane's tooltip went with it");
        h.open_path("More/Manga tools");
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
        assert_eq!(Action::ToggleRulerSnap.menu(), Some("More/Manga tools/Snap to rulers"));
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
