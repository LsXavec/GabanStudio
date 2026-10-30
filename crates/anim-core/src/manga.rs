//! GM18: manga pages. A book is a scene and a page is a cut of one frame
//! (D15: 350 dpi; a webtoon is 800 px wide and exports in 1280 px
//! segments). The page's geometry is the cut's `manga.page` meta and its
//! perspective rulers `manga.rulers` (D3: JSON, written by
//! `Command::SetCutMeta`, so old files never see them). The panels (koma)
//! are a Panels generator's polygons, so every panel edit is one
//! `SetNodeKind`; the page graph cuts each column's art to them, and the
//! Tone effect is the screen tone, an AM halftone (D9: on the CPU here).
//! Pure maths: the app draws the guides, runs the panel and ruler tools and
//! exports the pages through `crate::deliver`.

use std::f32::consts::PI;

use rayon::prelude::*;

use crate::color::ColorLaw;
use crate::command::{Command, CutRef};
use crate::fx::{FxKind, FxValue, FxValues, Image, LUMA};
use crate::generator::{GenKind, GenValues, Target};
use crate::graph::{BlendMode, Node, NodeKind, place_node};
use crate::ids::{ColumnId, NodeId};
use crate::look::{self, Build};
use crate::model::Cut;

/// The manga print resolution (D15).
pub const PAGE_DPI: f32 = 350.0;
/// A webtoon strip's width, px (D15).
pub const WEBTOON_WIDTH: u32 = 800;
/// A webtoon's export segment, px down (D15).
pub const WEBTOON_SEGMENT: u32 = 1280;
/// A new webtoon strip's length: five segments.
pub const WEBTOON_HEIGHT: u32 = 5 * WEBTOON_SEGMENT;
/// The cut meta key of the page's geometry (D3).
pub const PAGE_KEY: &str = "manga.page";
/// The cut meta key of the page's perspective rulers (D3).
pub const RULERS_KEY: &str = "manga.rulers";
/// The panel tool's gutters, mm: across, between panels side by side (a
/// cut down the page), and down, between rows (a cut across it).
pub const GUTTER_SIDE_MM: f32 = 2.0;
pub const GUTTER_ROW_MM: f32 = 5.0;
/// The panel border's width, mm.
pub const BORDER_MM: f32 = 0.5;
/// How far the pen moves before the rulers pick its line, screen px.
pub const SNAP_PICK_PX: f32 = 6.0;

/// `mm` at `dpi` in whole px: round(mm · dpi / 25.4), worked in f64 so
/// every size lands on the same px on every machine.
pub fn mm_px(mm: f32, dpi: f32) -> u32 {
    (mm as f64 * dpi as f64 / 25.4).round().max(0.0) as u32
}

// ---- Pages ---------------------------------------------------------------------------

/// A rectangle in page px: `x, y` its top left, `w × h` its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PxRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl PxRect {
    /// The rect between `mm` edges at `dpi`, each edge rounded on its own
    /// (so rects that share an edge in mm share it in px).
    fn from_mm(x0: f32, y0: f32, x1: f32, y1: f32, dpi: f32) -> Self {
        let (x, y) = (mm_px(x0, dpi), mm_px(y0, dpi));
        Self {
            x,
            y,
            w: mm_px(x1, dpi) - x,
            h: mm_px(y1, dpi) - y,
        }
    }

    pub fn right(&self) -> u32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }
}

/// A page's geometry, as `manga.page` stores it (px at `dpi`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Page {
    /// "B5", "A4" or "webtoon".
    pub format: String,
    pub dpi: f32,
    /// The whole canvas, bleed and all.
    pub width: u32,
    pub height: u32,
    /// Where the printer trims.
    pub trim: PxRect,
    /// The inner frame (the safe area the panels sit in).
    pub inner: PxRect,
    /// A webtoon's export segment, px down (None: one image a page).
    #[serde(default)]
    pub segment: Option<u32>,
}

impl Page {
    /// A print page: `trim` mm with `bleed` mm all round it, the inner
    /// frame `inner` mm centred on the trim.
    pub fn print(format: &str, trim: [f32; 2], bleed: f32, inner: [f32; 2], dpi: f32) -> Self {
        let (ix, iy) = (bleed + (trim[0] - inner[0]) / 2.0, bleed + (trim[1] - inner[1]) / 2.0);
        Self {
            format: format.into(),
            dpi,
            width: mm_px(trim[0] + 2.0 * bleed, dpi),
            height: mm_px(trim[1] + 2.0 * bleed, dpi),
            trim: PxRect::from_mm(bleed, bleed, bleed + trim[0], bleed + trim[1], dpi),
            inner: PxRect::from_mm(ix, iy, ix + inner[0], iy + inner[1], dpi),
            segment: None,
        }
    }

    /// B5 (182 × 257 mm, the tankōbon page) with 3 mm bleed and a
    /// 150 × 220 mm inner frame, at 350 dpi: 2591 × 3624 px.
    pub fn b5() -> Self {
        Self::print("B5", [182.0, 257.0], 3.0, [150.0, 220.0], PAGE_DPI)
    }

    /// A4 (210 × 297 mm, the manuscript sheet) with 3 mm bleed and a
    /// 180 × 270 mm inner frame, at 350 dpi: 2976 × 4175 px.
    pub fn a4() -> Self {
        Self::print("A4", [210.0, 297.0], 3.0, [180.0, 270.0], PAGE_DPI)
    }

    /// A webtoon strip 800 px wide and [`WEBTOON_HEIGHT`] down, no bleed;
    /// it exports in [`WEBTOON_SEGMENT`] px segments (72 dpi: a screen's).
    pub fn webtoon() -> Self {
        let full = PxRect {
            x: 0,
            y: 0,
            w: WEBTOON_WIDTH,
            h: WEBTOON_HEIGHT,
        };
        Self {
            format: "webtoon".into(),
            dpi: 72.0,
            width: WEBTOON_WIDTH,
            height: WEBTOON_HEIGHT,
            trim: full,
            inner: full,
            segment: Some(WEBTOON_SEGMENT),
        }
    }

    /// The page a `w` × `h` px project is laid out for (None: not a
    /// book size, so a new page has nothing to copy).
    pub fn for_size(w: u32, h: u32) -> Option<Self> {
        [Self::b5(), Self::a4(), Self::webtoon()].into_iter().find(|p| (p.width, p.height) == (w, h))
    }

    /// `mm` on this page, px.
    pub fn px(&self, mm: f32) -> f32 {
        mm_px(mm, self.dpi) as f32
    }

    /// The JSON `manga.page` holds.
    pub fn to_meta(&self) -> String {
        serde_json::to_string(self).expect("a page serialises")
    }

    /// Cut `cut`'s page (None: the cut is not a manga page).
    pub fn of(cut: &Cut) -> Option<Self> {
        cut.meta.get(PAGE_KEY).and_then(|s| serde_json::from_str(s).ok())
    }

    /// The panel a new page starts with: its inner frame.
    pub fn first_panel(&self) -> Poly {
        rect_poly(self.inner)
    }
}

/// The name a new page takes after pages named `names`: P and one past
/// the highest P number, two digits at least ("P01", "P02"; a page
/// removed from the middle frees no number, so no two pages share one).
pub fn next_page_name<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let n = names
        .into_iter()
        .filter_map(|s| s.strip_prefix(['P', 'p']))
        .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|d| d.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("P{:02}", n + 1)
}

// ---- Panels --------------------------------------------------------------------------

/// A panel: its outline's corners in page px, in order.
pub type Poly = Vec<[f32; 2]>;

/// The rect's outline, clockwise from the top left.
pub fn rect_poly(r: PxRect) -> Poly {
    let (x0, y0, x1, y1) = (r.x as f32, r.y as f32, r.right() as f32, r.bottom() as f32);
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

/// The panel a drag from `a` to `b` draws, on whole px (None: too thin).
pub fn drag_panel(a: [f32; 2], b: [f32; 2]) -> Option<Poly> {
    let (x0, x1) = (a[0].min(b[0]).round(), a[0].max(b[0]).round());
    let (y0, y1) = (a[1].min(b[1]).round(), a[1].max(b[1]).round());
    (x1 - x0 >= 2.0 && y1 - y0 >= 2.0).then(|| vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}

/// The outline's signed area (shoelace; positive clockwise on a page).
pub fn area(poly: &[[f32; 2]]) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (p, q) = (poly[i], poly[(i + 1) % n]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f32>()
        / 2.0
}

/// Is `p` inside `poly` (even-odd)?
pub fn contains(poly: &[[f32; 2]], p: [f32; 2]) -> bool {
    let n = poly.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    inside
}

/// `p`'s distance to `poly`'s outline, negative inside.
pub fn signed_distance(poly: &[[f32; 2]], p: [f32; 2]) -> f32 {
    let n = poly.len();
    let mut d2 = f32::MAX;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
        let (wx, wy) = (p[0] - a[0], p[1] - a[1]);
        let t = ((wx * ex + wy * ey) / (ex * ex + ey * ey).max(1e-12)).clamp(0.0, 1.0);
        let (dx, dy) = (wx - ex * t, wy - ey * t);
        d2 = d2.min(dx * dx + dy * dy);
    }
    let d = d2.sqrt();
    if contains(poly, p) { -d } else { d }
}

/// The panel under `p` (the last drawn wins where they overlap).
pub fn panel_at(polys: &[Poly], p: [f32; 2]) -> Option<usize> {
    polys.iter().rposition(|q| q.len() >= 3 && contains(q, p))
}

/// The gutter a cut from `a` to `b` leaves on `page`, px: a cut more
/// across than down makes rows, [`GUTTER_ROW_MM`] apart; one down the page
/// sets panels side by side, [`GUTTER_SIDE_MM`] apart (the tool's mm).
pub fn gutter_for(a: [f32; 2], b: [f32; 2], side_mm: f32, row_mm: f32, dpi: f32) -> f32 {
    let across = (b[0] - a[0]).abs() >= (b[1] - a[1]).abs();
    mm_px(if across { row_mm } else { side_mm }, dpi) as f32
}

/// The part of `poly` where `n · p <= k` (Sutherland–Hodgman, one edge).
/// A cut along an axis lands exactly on `k`.
fn clip_half(poly: &[[f32; 2]], n: [f32; 2], k: f32) -> Poly {
    let side = |p: [f32; 2]| n[0] * p[0] + n[1] * p[1] - k;
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let (sp, sq) = (side(p), side(q));
        if sp <= 0.0 {
            out.push(p);
        }
        if (sp < 0.0 && sq > 0.0) || (sp > 0.0 && sq < 0.0) {
            let t = sp / (sp - sq);
            let mut x = [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
            if n[0] == 0.0 {
                x[1] = k * n[1];
            } else if n[1] == 0.0 {
                x[0] = k * n[0];
            }
            out.push(x);
        }
    }
    out
}

/// Split `poly` along the line through `a` and `b`, leaving a `gutter` px
/// gap centred on it: (the half above or left of it, the other). A cut
/// along an axis lands on whole px, so the gap is exactly the gutter.
/// None when the line misses the panel or a half would be empty.
pub fn split_panel(poly: &[[f32; 2]], a: [f32; 2], b: [f32; 2], gutter: f32) -> Option<(Poly, Poly)> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if !(len > 1e-3) || poly.len() < 3 {
        return None;
    }
    // The unit normal, pointing down the page (else right): the first half
    // lies on its negative side.
    let mut n = [-dy / len, dx / len];
    if n[1] < 0.0 || (n[1] == 0.0 && n[0] < 0.0) {
        n = [-n[0], -n[1]];
    }
    if n[0] == 0.0 {
        n = [0.0, 1.0];
    } else if n[1] == 0.0 {
        n = [1.0, 0.0];
    }
    let c = n[0] * a[0] + n[1] * a[1];
    let g = gutter.max(0.0);
    let (lo, hi) = if n[0] == 0.0 || n[1] == 0.0 {
        let lo = (c - g / 2.0).round();
        (lo, lo + g.round())
    } else {
        (c - g / 2.0, c + g / 2.0)
    };
    let first = clip_half(poly, n, lo);
    let second = clip_half(poly, [-n[0], -n[1]], -hi);
    let whole = |p: &Poly| p.len() >= 3 && area(p).abs() >= 1.0;
    (whole(&first) && whole(&second)).then_some((first, second))
}

/// The page's Panels node and its panels: the graph's first Panels
/// generator.
pub fn panels_of(cut: &Cut) -> Option<(NodeId, Vec<Poly>)> {
    cut.graph.nodes().find_map(|n| match &n.kind {
        NodeKind::Generator {
            generator: GenKind::Panels,
            params,
            binds,
        } => {
            let v = GenValues::resolve(GenKind::Panels, params, binds, |_| None);
            Some((n.id, v.polys("panels").to_vec()))
        }
        _ => None,
    })
}

/// Node `id`'s panels become `polys`: one `SetNodeKind`, one undo step.
pub fn set_panels(cut: &Cut, at: CutRef, id: NodeId, polys: Vec<Poly>) -> Option<Command> {
    let mut kind = cut.graph.node(id).ok()?.kind.clone();
    let NodeKind::Generator { params, .. } = &mut kind else {
        return None;
    };
    params.set("panels", FxValue::Polys { polys });
    Some(Command::SetNodeKind { at, id, kind })
}

/// A Panels generator of `polys` with a `border` px border.
pub fn panels_kind(polys: Vec<Poly>, border: f32) -> NodeKind {
    let mut kind = NodeKind::generator(GenKind::Panels);
    if let NodeKind::Generator { params, .. } = &mut kind {
        params.set("panels", FxValue::Polys { polys });
        params.set("border", FxValue::Float(border));
    }
    kind
}

/// The Panels generator: each panel's inside `fill`, its border `border`
/// px inside the outline in `color`, clear between the panels (so a
/// multiply leaves the art under an inside as it is and the gutters bare).
pub fn draw_panels(v: &GenValues, law: ColorLaw, t: Target) -> Image {
    let polys = v.polys("panels");
    let (fill, ink) = (law.premultiplied(v.color("fill")), law.premultiplied(v.color("color")));
    let bw = v.float("border");
    let mut img = Image::new(t.w, t.h);
    let w = t.w as usize;
    if w == 0 || polys.is_empty() {
        return img;
    }
    let boxes: Vec<[f32; 4]> = polys
        .iter()
        .map(|p| {
            p.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, q| {
                [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])]
            })
        })
        .collect();
    img.px.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let fy = t.y as f32 + y as f32 + 0.5;
        for (poly, b) in polys.iter().zip(&boxes) {
            if poly.len() < 3 || fy < b[1] - 1.0 || fy > b[3] + 1.0 {
                continue;
            }
            let x0 = (b[0] - 1.0 - t.x as f32).floor().max(0.0) as usize;
            let x1 = (b[2] + 1.0 - t.x as f32).ceil();
            if x1 < 0.0 {
                continue;
            }
            for x in x0..=(x1 as usize).min(w - 1) {
                let d = signed_distance(poly, [t.x as f32 + x as f32 + 0.5, fy]);
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                // 1 within the border's width inside the outline, 0 deeper.
                let edge = if bw > 0.0 { (d + bw + 0.5).clamp(0.0, 1.0) } else { 0.0 };
                let p = &mut row[x * 4..x * 4 + 4];
                let c: [f32; 4] = std::array::from_fn(|k| cov * (fill[k] * (1.0 - edge) + ink[k] * edge));
                for k in 0..4 {
                    p[k] = c[k] + p[k] * (1.0 - c[3]);
                }
            }
        }
    });
    img
}

// ---- The page graph ------------------------------------------------------------------

/// Is `column` a tone column (its name starts TONE, or トーン)?
pub fn is_tone_column(cut: &Cut, column: ColumnId) -> bool {
    cut.xsheet.column(column).is_some_and(|c| {
        let name = c.name.trim().to_uppercase();
        name.starts_with("TONE") || name.starts_with("トーン")
    })
}

/// The Tone a column's source already feeds (its values carry over when
/// the graph is rebuilt).
fn tone_of(cut: &Cut, column: ColumnId) -> Option<NodeKind> {
    cut.graph.nodes().find_map(|n| {
        let fed = n.inputs.first().copied().flatten().and_then(|(src, _)| cut.graph.node(src).ok());
        match (&n.kind, fed.map(|f| &f.kind)) {
            (NodeKind::Effect { fx: FxKind::Tone, .. }, Some(NodeKind::DrawingSource { column: c, .. })) if *c == column => {
                Some(n.kind.clone())
            }
            _ => None,
        }
    })
}

/// A generator nothing feeds, other than the Panels — a balloon, a line of
/// text, speed lines added by hand: the page graph keeps it, on top.
fn is_free_generator(n: &Node) -> bool {
    matches!(&n.kind, NodeKind::Generator { generator, .. } if !matches!(generator, GenKind::Panels))
        && n.inputs.iter().all(Option::is_none)
}

/// GM18 "Clip to panels": the page graph — white paper; each column's art
/// (a tone column through a Tone) cut to the panels by a Matte, stacked in
/// band order; the Panels multiplied over the top, so the art inside a
/// panel shows as it is, the gutters stay paper and the borders print.
/// One batch (the old nodes and their places go first, as the satsuei
/// template's do): one undo step. The graph's Panels keep their panels
/// (else `polys`), a column's Tone its values, and a generator added by
/// hand (a balloon, text) stays, stacked over the borders.
pub fn page_graph(cut: &Cut, at: CutRef, alloc: impl FnMut() -> NodeId, polys: Vec<Poly>, border: f32) -> Vec<Command> {
    let panels_kind = match cut.graph.nodes().find(|n| matches!(n.kind, NodeKind::Generator { generator: GenKind::Panels, .. })) {
        Some(n) => n.kind.clone(),
        None => panels_kind(polys, border),
    };
    let mut b = Build {
        cut,
        at,
        alloc,
        cmds: Vec::new(),
    };
    for n in cut.graph.nodes() {
        b.cmds.push(Command::RemoveNode { at, id: n.id });
    }
    for n in cut.graph.nodes() {
        b.cmds.extend(place_node(cut, at, n.id, None));
    }
    const COL: f32 = 220.0;
    const ROW: f32 = 90.0;
    let paper = b.add(NodeKind::Solid { rgba: [255; 4] }, (3.0 * COL, 0.0));
    let panels = b.add(panels_kind, (COL, 0.0));
    let mut page = paper;
    let mut y = 0.0;
    for column in look::template_columns(cut) {
        y += ROW;
        let src = b.add(NodeKind::source(column), (0.0, y));
        let art = if is_tone_column(cut, column) {
            let tone = tone_of(cut, column).unwrap_or_else(|| NodeKind::effect(FxKind::Tone));
            b.then(src, tone, (COL, y))
        } else {
            src
        };
        let clip = b.then(art, NodeKind::effect(FxKind::Matte), (2.0 * COL, y));
        b.wire(panels, clip, 1);
        page = b.stack(Some(page), clip, BlendMode::Normal, (3.0 * COL, y));
    }
    let mut top = b.stack(Some(page), panels, BlendMode::Multiply, (4.0 * COL, y));
    for n in cut.graph.nodes().filter(|n| is_free_generator(n)) {
        y += ROW;
        let over = b.add(n.kind.clone(), (3.0 * COL, y));
        top = b.stack(Some(top), over, BlendMode::Normal, (4.0 * COL, y));
    }
    let out = b.then(top, NodeKind::Output, (5.0 * COL, y));
    b.cmds.push(Command::SetOutput { at, node: Some(out) });
    b.cmds
}

/// GM18: the node count when "Clip to panels" would remove nodes a hand
/// added to the cut's graph or undo its rewiring (None: the graph is empty
/// or already the page graph — a free generator on top or unwired is kept,
/// a column added since only joins — so the rebuild loses nothing).
pub fn page_graph_hand_built(cut: &Cut) -> Option<usize> {
    let n = cut.graph.nodes().count();
    (n > 0 && !is_page_stack(cut)).then_some(n)
}

/// Is the cut's graph what [`page_graph`] builds: the Output over free
/// generators stacked Normal, over the Panels' Multiply, over the page —
/// each column's default source (a tone column's through a Tone) clipped
/// by a default Matte to those Panels and stacked Normal, bottom up in
/// band order, on white paper — and no other node but free generators?
fn is_page_stack(cut: &Cut) -> bool {
    let g = &cut.graph;
    let node = |id: Option<NodeId>| id.and_then(|id| g.node(id).ok());
    let input = |n: &Node, pin: usize| n.inputs.get(pin).copied().flatten().filter(|(_, p)| *p == 0).map(|(id, _)| id);
    // Its first `fed` pins wired and the rest bare.
    let exact = |n: &Node, fed: usize| {
        n.inputs.len() >= fed && n.inputs[..fed].iter().all(Option::is_some) && n.inputs[fed..].iter().all(Option::is_none)
    };
    let normal = |n: &Node| n.kind == NodeKind::Blend { mode: BlendMode::Normal } && exact(n, 2);
    let mut seen = std::collections::BTreeSet::new();
    let Some(out) = node(g.output).filter(|o| o.kind == NodeKind::Output && exact(o, 1)) else {
        return false;
    };
    seen.insert(out.id);
    // The free generators on top, down to the Panels' Multiply.
    let mut at = node(input(out, 0));
    let top = loop {
        let Some(n) = at else {
            return false;
        };
        match node(input(n, 1)) {
            Some(over) if normal(n) && is_free_generator(over) => {
                if !seen.insert(n.id) {
                    return false;
                }
                seen.insert(over.id);
                at = node(input(n, 0));
            }
            _ => break n,
        }
    };
    let Some(panels) = node(input(top, 1)) else {
        return false;
    };
    if top.kind != (NodeKind::Blend { mode: BlendMode::Multiply })
        || !exact(top, 2)
        || !matches!(panels.kind, NodeKind::Generator { generator: GenKind::Panels, .. })
        || !exact(panels, 0)
    {
        return false;
    }
    seen.extend([top.id, panels.id]);
    // The page, top down: Normal blends of clipped art, then the paper.
    let mut columns = Vec::new();
    let mut at = node(input(top, 0));
    let paper = loop {
        let Some(n) = at else {
            return false;
        };
        if !matches!(n.kind, NodeKind::Blend { .. }) {
            break n;
        }
        let clip = node(input(n, 1)).filter(|c| {
            c.kind == NodeKind::effect(FxKind::Matte) && exact(c, 2) && input(c, 1) == Some(panels.id)
        });
        let (Some(clip), true) = (clip, normal(n)) else {
            return false;
        };
        let Some(mut art) = node(input(clip, 0)) else {
            return false;
        };
        if !seen.insert(n.id) {
            return false;
        }
        seen.insert(clip.id);
        if matches!(art.kind, NodeKind::Effect { fx: FxKind::Tone, .. }) && exact(art, 1) {
            seen.insert(art.id);
            match node(input(art, 0)) {
                // A Tone over a column that is no tone column is a hand's.
                Some(src) if matches!(src.kind, NodeKind::DrawingSource { column, .. } if is_tone_column(cut, column)) => {
                    art = src
                }
                _ => return false,
            }
        }
        let NodeKind::DrawingSource { column, .. } = art.kind else {
            return false;
        };
        if art.kind != NodeKind::source(column) || !exact(art, 0) {
            return false;
        }
        seen.insert(art.id);
        columns.push(column);
        at = node(input(n, 0));
    };
    if paper.kind != (NodeKind::Solid { rgba: [255; 4] }) || !exact(paper, 0) {
        return false;
    }
    seen.insert(paper.id);
    columns.reverse();
    let mut order = look::template_columns(cut).into_iter();
    columns.iter().all(|c| order.any(|o| o == *c)) && g.nodes().all(|n| seen.contains(&n.id) || is_free_generator(n))
}

// ---- Screen tone ---------------------------------------------------------------------

/// A tone's dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotShape {
    Round,
    Square,
    Line,
}

/// The screen a Tone lays: dot centres `cell` px apart along rows at the
/// angle (an AM halftone: the dots grow with the grey, the grid stays).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub cell: f32,
    cos: f32,
    sin: f32,
    pub shape: DotShape,
}

impl Screen {
    /// `lpi` lines an inch on a `dpi` page, the rows at `angle` degrees.
    pub fn new(dpi: f32, lpi: f32, angle: f32, shape: DotShape) -> Self {
        let a = angle.to_radians();
        Self {
            cell: (dpi / lpi.max(1e-3)).max(1e-3),
            cos: a.cos(),
            sin: a.sin(),
            shape,
        }
    }

    /// The screen of a Tone's values.
    pub fn of(v: &FxValues) -> Self {
        let shape = match v.choice("shape") {
            1 => DotShape::Square,
            2 => DotShape::Line,
            _ => DotShape::Round,
        };
        Self::new(v.float("dpi"), v.float("lpi"), v.float("angle"), shape)
    }

    /// Dot (i, j)'s centre, px: the grid turned by the angle.
    pub fn dot_centre(&self, i: i32, j: i32) -> [f32; 2] {
        let (i, j) = (i as f32, j as f32);
        [
            self.cell * (i * self.cos - j * self.sin),
            self.cell * (i * self.sin + j * self.cos),
        ]
    }

    /// Is the point (px) ink at density `d` (0 bare, 1 solid)? A round dot
    /// grows to half the cell, then the white between the dots shrinks; a
    /// square grows to the cell; a line thickens across its row.
    pub fn ink(&self, x: f32, y: f32, d: f32) -> bool {
        if d <= 0.0 {
            return false;
        }
        if d >= 1.0 {
            return true;
        }
        let u = (x * self.cos + y * self.sin) / self.cell;
        let v = (-x * self.sin + y * self.cos) / self.cell;
        let (fu, fv) = (u - u.round(), v - v.round());
        match self.shape {
            DotShape::Round if d <= 0.5 => fu * fu + fv * fv <= d / PI,
            DotShape::Round => {
                let (a, b) = (0.5 - fu.abs(), 0.5 - fv.abs());
                a * a + b * b > (1.0 - d) / PI
            }
            DotShape::Square => fu.abs().max(fv.abs()) <= d.sqrt() / 2.0,
            DotShape::Line => fv.abs() <= d / 2.0,
        }
    }
}

/// A pixel's tone density: the ink its grey asks for (black 1, white or
/// clear 0), on the chip scale under either law.
pub fn density(law: ColorLaw, p: &[f32]) -> f32 {
    let a = p[3];
    if a <= 0.0 {
        return 0.0;
    }
    let y = (LUMA[0] * p[0] + LUMA[1] * p[1] + LUMA[2] * p[2]) / a;
    let shown = match law {
        ColorLaw::Srgb => y.clamp(0.0, 1.0),
        ColorLaw::Legacy => law.decode(y) as f32 / 255.0,
    };
    (a * (1.0 - shown)).clamp(0.0, 1.0)
}

/// The Tone effect: the image's grey as the screen's dots of the ink on
/// clear. 1-bit: each pixel is ink or nothing (its centre decides); else
/// 4 × 4 samples smooth the dots' edges.
pub(crate) fn tone(v: &FxValues, law: ColorLaw, src: &Image) -> Image {
    let screen = Screen::of(v);
    let ink = law.premultiplied(v.color("color"));
    let mono = v.flag("mono");
    let mut out = Image::new(src.w, src.h);
    let w = src.w as usize;
    if w == 0 {
        return out;
    }
    out.px
        .par_chunks_mut(w * 4)
        .zip(src.px.par_chunks(w * 4))
        .enumerate()
        .for_each(|(y, (row, from))| {
            for (x, (p, q)) in row.chunks_exact_mut(4).zip(from.chunks_exact(4)).enumerate() {
                let d = density(law, q);
                if d <= 0.0 {
                    continue;
                }
                let (fx, fy) = (x as f32, y as f32);
                let cov = if mono {
                    if screen.ink(fx + 0.5, fy + 0.5, d) { 1.0 } else { 0.0 }
                } else {
                    let hits = (0..16)
                        .filter(|s| screen.ink(fx + (s % 4) as f32 * 0.25 + 0.125, fy + (s / 4) as f32 * 0.25 + 0.125, d))
                        .count();
                    hits as f32 / 16.0
                };
                for k in 0..4 {
                    p[k] = ink[k] * cov;
                }
            }
        });
    out
}

// ---- Perspective rulers --------------------------------------------------------------

/// A page's perspective rulers, as `manga.rulers` stores them: one to
/// three vanishing points (page px) and the horizon's height.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rulers {
    pub vps: Vec<[f32; 2]>,
    #[serde(default)]
    pub horizon: Option<f32>,
}

impl Rulers {
    /// Cut `cut`'s rulers (none set: no vanishing points).
    pub fn of(cut: &Cut) -> Self {
        cut.meta
            .get(RULERS_KEY)
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }

    /// The JSON `manga.rulers` holds.
    pub fn to_meta(&self) -> String {
        serde_json::to_string(self).expect("rulers serialise")
    }

    /// `points` vanishing points (0 to 3) laid out on a `w × h` page: one
    /// at the horizon's middle, two at its ends, a third far below.
    pub fn preset(points: usize, w: f32, h: f32) -> Self {
        let horizon = h * 0.4;
        let vps = match points {
            0 => Vec::new(),
            1 => vec![[w * 0.5, horizon]],
            2 => vec![[-w * 0.25, horizon], [w * 1.25, horizon]],
            _ => vec![[-w * 0.25, horizon], [w * 1.25, horizon], [w * 0.5, h * 2.5]],
        };
        Self {
            horizon: (!vps.is_empty()).then_some(horizon),
            vps,
        }
    }

    /// The `manga.rulers` edit to `self`: one `SetCutMeta` (None: no change).
    pub fn set(&self, cut: &Cut, at: CutRef) -> Option<Command> {
        let before = cut.meta.get(RULERS_KEY).cloned();
        let after = (!self.vps.is_empty()).then(|| self.to_meta());
        (before != after).then(|| Command::SetCutMeta {
            at,
            key: RULERS_KEY.into(),
            before,
            after,
        })
    }

    /// The handle within `reach` px of `p` (None: none): the nearest
    /// vanishing point, else the horizon line.
    pub fn handle_at(&self, p: [f32; 2], reach: f32) -> Option<RulerHandle> {
        let near = self
            .vps
            .iter()
            .enumerate()
            .map(|(k, v)| (k, (v[0] - p[0]).hypot(v[1] - p[1])))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match near {
            Some((k, _)) => Some(RulerHandle::Vp(k)),
            None => self.horizon.filter(|h| (p[1] - h).abs() <= reach).map(|_| RulerHandle::Horizon),
        }
    }

    /// These rulers with `handle` dragged to `to`. The horizon carries the
    /// vanishing points on it (a 1- or 2-point view stays level); a point
    /// on the horizon carries the horizon and the other points on it; the
    /// third point of a 3-point view, below, moves alone.
    pub fn moved(&self, handle: RulerHandle, to: [f32; 2]) -> Self {
        let on = |v: &[f32; 2]| self.horizon.is_some_and(|h| (v[1] - h).abs() <= 0.5);
        let mut r = self.clone();
        let level = match handle {
            RulerHandle::Vp(k) => {
                let Some(v) = self.vps.get(k) else {
                    return r;
                };
                let level = on(v);
                r.vps[k] = to;
                level
            }
            RulerHandle::Horizon => self.horizon.is_some(),
        };
        if level {
            for (k, v) in self.vps.iter().enumerate() {
                if on(v) && handle != RulerHandle::Vp(k) {
                    r.vps[k][1] = to[1];
                }
            }
            r.horizon = Some(to[1]);
        }
        r
    }
}

/// GM18: what a canvas drag holds of a page's rulers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulerHandle {
    /// Vanishing point `k` (0 is VP1).
    Vp(usize),
    /// The horizon line (it moves up and down).
    Horizon,
}

/// A line the pen is held to: through `origin` along the unit `dir`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub origin: [f32; 2],
    pub dir: [f32; 2],
}

impl Line {
    /// The point on the line nearest `p`.
    pub fn project(&self, p: [f32; 2]) -> [f32; 2] {
        let t = (p[0] - self.origin[0]) * self.dir[0] + (p[1] - self.origin[1]) * self.dir[1];
        [self.origin[0] + t * self.dir[0], self.origin[1] + t * self.dir[1]]
    }
}

/// The ruler line a stroke from `start` is held to once the pen at
/// `current` is `min_move` px away (None before then, or with no rulers):
/// of the lines from `start` toward each vanishing point, the horizontal
/// and the vertical (a third point stands in for the vertical), the one
/// nearest the stroke's direction.
pub fn snap_direction(start: [f32; 2], current: [f32; 2], rulers: &Rulers, min_move: f32) -> Option<Line> {
    if rulers.vps.is_empty() {
        return None;
    }
    let (dx, dy) = (current[0] - start[0], current[1] - start[1]);
    let len = dx.hypot(dy);
    if len < min_move.max(1e-3) {
        return None;
    }
    let d = [dx / len, dy / len];
    let mut lines: Vec<[f32; 2]> = rulers
        .vps
        .iter()
        .filter_map(|vp| {
            let (x, y) = (vp[0] - start[0], vp[1] - start[1]);
            let l = x.hypot(y);
            (l > 1e-3).then(|| [x / l, y / l])
        })
        .collect();
    lines.push([1.0, 0.0]);
    if rulers.vps.len() < 3 {
        lines.push([0.0, 1.0]);
    }
    let fit = |m: &[f32; 2]| (m[0] * d[0] + m[1] * d[1]).abs();
    let dir = lines.into_iter().max_by(|a, b| fit(a).total_cmp(&fit(b)))?;
    Some(Line { origin: start, dir })
}

/// A stroke held to the rulers point by point (the canvas's stroke tap
/// feeds it before the stabilizer): the first point as it is, then the
/// start until the line is picked, then each point on the line.
#[derive(Debug, Clone)]
pub struct Snap {
    rulers: Rulers,
    min_move: f32,
    start: Option<[f32; 2]>,
    line: Option<Line>,
}

impl Snap {
    pub fn new(rulers: Rulers, min_move: f32) -> Self {
        Self {
            rulers,
            min_move,
            start: None,
            line: None,
        }
    }

    /// The line picked so far.
    pub fn line(&self) -> Option<Line> {
        self.line
    }

    /// Where the pen at `p` draws.
    pub fn feed(&mut self, p: [f32; 2]) -> [f32; 2] {
        if self.rulers.vps.is_empty() {
            return p;
        }
        let Some(start) = self.start else {
            self.start = Some(p);
            return p;
        };
        if self.line.is_none() {
            self.line = snap_direction(start, p, &self.rulers, self.min_move);
        }
        match self.line {
            Some(l) => l.project(p),
            None => start,
        }
    }
}

// ---- Export --------------------------------------------------------------------------

/// A webtoon strip's export segments, (top, height) down `height` px:
/// `segment` px each, the last what is left.
pub fn slice_webtoon(height: u32, segment: u32) -> Vec<(u32, u32)> {
    let seg = segment.max(1);
    (0..height.div_ceil(seg)).map(|i| (i * seg, seg.min(height - i * seg))).collect()
}
