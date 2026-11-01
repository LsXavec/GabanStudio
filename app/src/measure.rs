//! THE MEASURES (AU1): the numbers the approachable-UI parts are held to, read
//! off the real editor, headless. `census` counts what a screen asks of a
//! newcomer, `paper_share` what it leaves of the paper, `clip_audit` and
//! `text_contrast_audit` what it cuts or washes out, and `Recorder` scripts a
//! journey that fails on a refused step. Each one fails a seeded fault in its
//! own tests below before anything depends on it. The gate ledger (section 10
//! of research/PSD-approachable-ui.md) says which part turns each into a gate.

// Later parts drive most of this; not every verb has a test caller yet.
#![allow(dead_code)]

use eframe::egui;
use egui::epaint::{ClippedShape, RectShape, Shape, TextShape};
use egui::{Color32, Context, LayerId, Pos2, Rect, Sense};

use crate::config::Action;
use crate::plate::{self, Viewport};
use crate::testkit::EditorHarness;
use crate::workspace::Pane;
use crate::{LESSON_ROOMS, PROBE_ROOMS, ProbeRoom};

/// A frame's worth of settling, as the journeys were measured.
fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

/// Anything that clicks, drags or takes focus. A hover sense is not a
/// control: every `Ui` registers one.
fn interactive(sense: Sense) -> bool {
    sense.senses_click() || sense.senses_drag() || sense.is_focusable()
}

/// The text shapes of a frame in paint order, each with its clip.
fn walk_texts<'s>(shape: &'s Shape, clip: Rect, out: &mut Vec<(&'s TextShape, Rect)>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| walk_texts(s, clip, out)),
        Shape::Text(t) => out.push((t, clip)),
        _ => {}
    }
}

fn texts_of(shapes: &[ClippedShape]) -> Vec<(&TextShape, Rect)> {
    let mut out = Vec::new();
    for c in shapes {
        walk_texts(&c.shape, c.clip_rect, &mut out);
    }
    out
}

/// AU3: the words painted inside `area` this frame: the top row's, say.
pub fn words_in(h: &EditorHarness, area: Rect) -> Vec<String> {
    let shapes = &h.kit().output().shapes;
    let inside = texts_of(shapes).into_iter().filter(|(t, _)| area.contains_rect(t.visual_bounding_rect()));
    inside.map(|(t, _)| t.galley.text().trim().to_string()).collect()
}

/// The name a report or a picture gives a room.
pub fn room_tag(room: ProbeRoom) -> String {
    match room {
        ProbeRoom::Stage(st) => format!("{st:?}"),
        other => format!("{other:?}"),
    }
}

// ---- census (section 10's definitions, ported from the journeys probe) ----

/// One interactive rect on screen, rounded the way the probe's dumps were.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub w: f32,
    pub h: f32,
    pub focus: bool,
    pub enabled: bool,
}

/// What a row of the census counts as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Control,
    Label,
    Seam,
    Backdrop,
}

impl Row {
    pub fn kind(self, area: f32) -> Kind {
        if self.w * self.h >= area * 0.05 {
            Kind::Backdrop
        } else if self.w.min(self.h) <= 6.0 {
            Kind::Seam
        } else if self.focus {
            Kind::Control
        } else {
            Kind::Label
        }
    }
}

/// How a term reads a string (lower-cased first; the probe's regexes were
/// all case-blind).
#[derive(Clone, Copy, Debug)]
pub enum Hit {
    /// `\bword\b`.
    Word(&'static str),
    /// Anywhere.
    Part(&'static str),
    /// The whole string.
    Whole(&'static str),
    /// Any kana or kanji.
    Glyphs,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// A Japanese craft word.
    Japanese,
    /// A trade term.
    Trade,
    /// A UI word a newcomer has to guess at.
    Unclear,
}

use Hit::{Glyphs, Part, Whole, Word};
use Tier::{Japanese as J, Trade as T, Unclear as U};

/// The terms a beginner must already know, with today's 49 first. The list
/// may only be extended (past `TODAY_TERMS`, which the baseline counts by).
pub const TERMS: &[(&str, &[Hit], Tier)] = &[
    ("genga", &[Word("genga")], J),
    ("douga/dōga", &[Word("douga"), Word("doga"), Word("dōga")], J),
    ("shiage", &[Word("shiage")], J),
    ("satsuei", &[Word("satsuei")], J),
    ("henshū", &[Word("henshu"), Word("henshū")], J),
    ("ekonte", &[Word("ekonte")], J),
    ("sakkan", &[Word("sakkan")], J),
    ("shaku", &[Word("shaku")], J),
    ("serifu", &[Word("serifu")], J),
    ("atari", &[Word("atari")], J),
    ("shusei", &[Word("shusei")], J),
    ("aka", &[Word("aka")], J),
    ("ao", &[Word("ao")], J),
    ("nuri", &[Word("nuri"), Part("nuri-naoshi")], J),
    ("kuchipaku", &[Word("kuchipaku")], J),
    ("shinkō", &[Word("shinko"), Word("shinkō")], J),
    ("iro shitei", &[Word("iro shitei")], J),
    ("tategaki", &[Word("tategaki")], J),
    ("tebure hosei", &[Part("tebure")], J),
    ("kanji glyphs", &[Glyphs], J),
    ("cel", &[Word("cel"), Word("cels")], T),
    ("x-sheet", &[Word("x-sheet"), Word("xsheet")], T),
    ("armed", &[Word("arm"), Word("armed"), Word("arms")], T),
    ("col", &[Word("col")], T),
    ("line (layer role)", &[Word("line")], T),
    ("hold / held", &[Word("hold"), Word("held")], T),
    ("key", &[Word("key"), Word("keys")], T),
    ("comp", &[Word("comp")], T),
    ("onion", &[Word("onion")], T),
    ("BG", &[Word("bg")], T),
    ("hardness", &[Word("hardness")], U),
    ("aliased", &[Word("aliased")], U),
    ("stabilizer", &[Word("stabilizer")], U),
    ("lasso fill", &[Word("lasso")], U),
    ("lock", &[Word("lock")], U),
    ("stack", &[Word("stack")], U),
    ("scrub", &[Word("scrub")], U),
    ("dropped", &[Word("dropped")], U),
    ("in/out", &[Whole("in"), Whole("out")], U),
    ("H / L", &[Whole("h"), Whole("l")], U),
    ("Brush Forge", &[Word("forge")], U),
    ("safe", &[Word("safe")], U),
    ("final look", &[Word("final look")], U),
    ("node", &[Word("node"), Word("nodes")], T),
    ("graph", &[Word("graph")], T),
    ("render queue", &[Word("render queue")], T),
    ("cut", &[Word("cut"), Word("cuts")], T),
    ("inbetween", &[Word("inbetween"), Word("inbetweens")], T),
    ("column", &[Word("column"), Word("columns")], T),
    // AU12, past the baseline's count.
    ("composite", &[Word("composite")], T),
    ("layout", &[Word("layout")], T),
    ("queue", &[Word("queue")], T),
    // AU12: the words the add menu and the manga strip printed beside a name.
    ("koma", &[Word("koma")], J),
    ("peji", &[Word("peji")], J),
    ("bokashi", &[Word("bokashi")], J),
    ("tōkakō", &[Word("tōkakō")], J),
    ("difyūjon", &[Word("difyūjon")], J),
    ("raito rappu", &[Word("raito rappu")], J),
    ("rimu raito", &[Word("rimu raito")], J),
    ("noizu", &[Word("noizu")], J),
    ("nagashi", &[Word("nagashi")], J),
    ("tōn", &[Word("tōn"), Word("amiten")], J),
    ("renban", &[Word("renban")], J),
    ("kamera", &[Word("kamera")], J),
    ("ryūsen", &[Word("ryūsen")], J),
    ("shūchūsen", &[Word("shūchūsen")], J),
    ("shashoku", &[Word("shashoku")], J),
    ("fukidashi", &[Word("fukidashi")], J),
    ("hibana", &[Word("hibana")], J),
    ("hokori", &[Word("hokori")], J),
    ("hinoko", &[Word("hinoko")], J),
    ("haikei", &[Word("haikei")], J),
    // AU13: egui's picker heads its row of numbers with this.
    ("U8", &[Whole("u8")], U),
];

/// How many of `TERMS` the journeys counted by. The baseline stays read
/// against these when the list grows.
pub const TODAY_TERMS: usize = 49;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn word_in(hay: &str, word: &str) -> bool {
    hay.match_indices(word).any(|(at, _)| {
        let before = hay[..at].chars().next_back();
        let after = hay[at + word.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    })
}

fn hits(hits: &[Hit], hay: &str) -> bool {
    hits.iter().any(|h| match *h {
        Word(w) => word_in(hay, w),
        Part(p) => hay.contains(p),
        Whole(w) => hay == w,
        // In numbers: `genui_glyph_coverage` reads a char literal as a label.
        Glyphs => hay.chars().any(|c| matches!(u32::from(c), 0x3040..=0x30ff | 0x3400..=0x9fff)),
    })
}

/// AU9: the distinct `TERMS` in the words a person read: a journey's jargon
/// counts what was on its path, not all that the screen holds.
pub fn jargon_in(read: &[String]) -> Vec<&'static str> {
    let read: Vec<String> = read.iter().map(|words| words.to_lowercase()).collect();
    let met = TERMS.iter().filter(|(_, terms, _)| read.iter().any(|words| hits(terms, words)));
    met.map(|(term, ..)| *term).collect()
}

/// A count of the screen. The first eleven fields are the baseline's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Census {
    pub controls: usize,
    pub controls_on: usize,
    pub labels: usize,
    pub seams: usize,
    pub backdrops: usize,
    pub unique: usize,
    pub words: usize,
    pub jargon: usize,
    pub japanese: usize,
    pub trade: usize,
    pub unclear: usize,
    /// The terms that hit, by name.
    pub terms: Vec<&'static str>,
    /// Every control's height, sorted.
    pub control_heights: Vec<f32>,
    /// Every painted string's height, sorted.
    pub text_heights: Vec<f32>,
    /// The largest backdrop's share of the window, in percent (the journeys'
    /// canvas share).
    pub largest_backdrop: f32,
}

fn median(sorted: &[f32]) -> f32 {
    match sorted.len() {
        0 => 0.0,
        n if n % 2 == 1 => sorted[n / 2],
        n => (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0,
    }
}

impl Census {
    /// The baseline's row for this count.
    pub fn row(&self) -> [usize; 11] {
        [
            self.controls,
            self.controls_on,
            self.labels,
            self.seams,
            self.backdrops,
            self.unique,
            self.words,
            self.jargon,
            self.japanese,
            self.trade,
            self.unclear,
        ]
    }

    pub fn median_control_height(&self) -> f32 {
        median(&self.control_heights)
    }

    pub fn controls_under(&self, height: f32) -> usize {
        self.control_heights.iter().filter(|h| **h < height).count()
    }

    pub fn median_text_height(&self) -> f32 {
        median(&self.text_heights)
    }

    pub fn texts_at_most(&self, height: f32) -> usize {
        self.text_heights.iter().filter(|h| **h <= height).count()
    }
}

/// One screen as the census reads it, before any counting: live from a
/// frame, or recorded (a fixture stays countable after the screen moves on).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub area: f32,
    pub rows: Vec<Row>,
    /// (height, string), one per painted string.
    pub texts: Vec<(f32, String)>,
}

impl Reading {
    /// The frame `ctx` last finished, with `shapes` as it painted them.
    pub fn of_frame(ctx: &Context, shapes: &[ClippedShape]) -> Self {
        Self::read(ctx, shapes, None)
    }

    /// AU6: one float of that frame: the rows on its `layer`, the words
    /// inside its `rect`.
    pub fn of_float(ctx: &Context, shapes: &[ClippedShape], layer: LayerId, rect: Rect) -> Self {
        Self::read(ctx, shapes, Some((layer, rect)))
    }

    fn read(ctx: &Context, shapes: &[ClippedShape], only: Option<(LayerId, Rect)>) -> Self {
        let window = ctx.content_rect();
        let rows = ctx.viewport(|v| {
            let w = &v.prev_pass.widgets;
            let mut rows = Vec::new();
            for (layer, rects) in w.layers() {
                if only.is_some_and(|(o, _)| o != *layer) {
                    continue;
                }
                for r in rects.iter().filter(|r| interactive(r.sense)) {
                    let at = r.interact_rect.intersect(window);
                    if at.width() > 0.0 && at.height() > 0.0 {
                        rows.push(Row {
                            w: at.width().round_ties_even(),
                            h: at.height().round_ties_even(),
                            focus: r.sense.is_focusable(),
                            enabled: r.enabled,
                        });
                    }
                }
            }
            rows
        });
        let mut rows = rows;
        // AU5: the strip is one surface, and each sheet on it is a control
        // (spec 7.2), up to the cap.
        let sheets = crate::plate::strip_sheets(ctx);
        let cap = if only.is_some() { 0 } else { crate::strip::CENSUS_CAP };
        rows.extend(sheets.iter().take(cap).map(|at| Row {
            w: at.width().round_ties_even(),
            h: at.height().round_ties_even(),
            focus: true,
            enabled: true,
        }));
        let mut texts = Vec::new();
        for (t, clip) in texts_of(shapes) {
            let at = t.visual_bounding_rect().intersect(clip.intersect(window));
            let text = t.galley.text();
            let inside = only.is_none_or(|(_, rect)| rect.contains_rect(at));
            if inside && at.width() > 0.0 && at.height() > 0.0 && !text.trim().is_empty() {
                let one_line = text.replace(['\n', '\t'], " ").replace('\r', "");
                texts.push((at.height().round_ties_even(), one_line));
            }
        }
        Self { area: window.area(), rows, texts }
    }

    /// The reading as lines of text, the fixture's format.
    pub fn dump(&self) -> String {
        let mut out = format!("area\t{}\n", self.area);
        for r in &self.rows {
            let (focus, on) = (u8::from(r.focus), u8::from(r.enabled));
            out.push_str(&format!("row\t{}\t{}\t{focus}\t{on}\n", r.w, r.h));
        }
        for (h, text) in &self.texts {
            out.push_str(&format!("text\t{h}\t{text}\n"));
        }
        out
    }

    pub fn parse(dump: &str) -> Self {
        let mut out = Self::default();
        for line in dump.lines() {
            let mut cells = line.split('\t');
            let tag = cells.next().unwrap_or_default();
            let mut number = || cells.next().and_then(|c| c.parse::<f32>().ok()).expect("a number");
            match tag {
                "area" => out.area = number(),
                "row" => {
                    let (w, h, focus, enabled) = (number(), number(), number(), number());
                    out.rows.push(Row { w, h, focus: focus > 0.0, enabled: enabled > 0.0 });
                }
                "text" => {
                    let h = number();
                    out.texts.push((h, cells.next().unwrap_or_default().to_string()));
                }
                _ => panic!("not a census line: {line:?}"),
            }
        }
        out
    }

    /// Count the reading against the first `terms` of `TERMS`.
    pub fn census(&self, terms: usize) -> Census {
        let mut c = Census::default();
        for r in &self.rows {
            match r.kind(self.area) {
                Kind::Control => {
                    c.controls += 1;
                    c.controls_on += usize::from(r.enabled);
                    c.control_heights.push(r.h);
                }
                Kind::Label => c.labels += 1,
                Kind::Seam => c.seams += 1,
                Kind::Backdrop => {
                    c.backdrops += 1;
                    c.largest_backdrop = c.largest_backdrop.max(100.0 * r.w * r.h / self.area);
                }
            }
        }
        c.control_heights.sort_by(f32::total_cmp);
        c.text_heights = self.texts.iter().map(|(h, _)| *h).collect();
        c.text_heights.sort_by(f32::total_cmp);
        let mut unique: Vec<String> = self.texts.iter().map(|(_, s)| s.clone()).collect();
        unique.sort();
        unique.dedup();
        c.unique = unique.len();
        let token = |t: &str| t.chars().any(char::is_alphabetic);
        c.words = unique.iter().map(|s| s.split_whitespace().filter(|t| token(t)).count()).sum();
        let lower: Vec<String> = unique.iter().map(|s| s.to_lowercase()).collect();
        for (name, read, tier) in &TERMS[..terms] {
            if lower.iter().any(|s| hits(read, s)) {
                c.terms.push(name);
                c.jargon += 1;
                match tier {
                    Tier::Japanese => c.japanese += 1,
                    Tier::Trade => c.trade += 1,
                    Tier::Unclear => c.unclear += 1,
                }
            }
        }
        c
    }
}

/// The rects a frame paints, walked out of their `Vec`s.
pub fn rects_of(shapes: &[ClippedShape]) -> Vec<RectShape> {
    fn walk(shape: &Shape, out: &mut Vec<RectShape>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            Shape::Rect(r) => out.push(r.clone()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// Where a frame's images stand: an icon cut from its masks is one.
pub fn images_of(shapes: &[ClippedShape]) -> Vec<Rect> {
    fn walk(shape: &Shape, out: &mut Vec<Rect>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            Shape::Mesh(m) if !m.is_empty() => out.push(m.calc_bounds()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// The frame as it stands, read.
pub fn reading(h: &EditorHarness) -> Reading {
    Reading::of_frame(&h.ctx(), &h.kit().output().shapes)
}

/// Count the screen after it settles.
pub fn census(h: &mut EditorHarness) -> Census {
    settle(h);
    reading(h).census(TERMS.len())
}

/// Today's rooms at 1600×900 (document, room, then `Census::row`), from
/// `auseam_census_records_todays_rooms`. Blank Drawing is the journeys'
/// 72 controls, 100 words, 30 jargon. The screen moves on from here; these
/// numbers do not.
pub const BASELINE: [(&str, &str, [usize; 11]); 16] = [
    ("blank", "Layout", [54, 53, 15, 2, 2, 77, 84, 25, 7, 11, 7]),
    ("blank", "Drawing", [72, 71, 18, 2, 2, 92, 100, 30, 9, 10, 11]),
    ("blank", "InkPaint", [80, 79, 21, 4, 3, 92, 121, 31, 10, 12, 9]),
    ("blank", "Composite", [46, 45, 11, 2, 2, 50, 100, 17, 5, 7, 5]),
    ("blank", "Edit", [69, 68, 18, 3, 3, 90, 128, 27, 8, 12, 7]),
    ("blank", "Manga", [87, 85, 24, 2, 1, 102, 180, 26, 7, 10, 9]),
    ("blank", "Forge", [84, 83, 34, 2, 1, 94, 115, 21, 7, 5, 9]),
    ("blank", "Layout3d", [50, 47, 9, 2, 3, 57, 116, 16, 5, 6, 5]),
    ("sample", "Layout", [95, 94, 18, 2, 2, 115, 102, 25, 7, 11, 7]),
    ("sample", "Drawing", [113, 112, 21, 2, 2, 126, 114, 30, 9, 10, 11]),
    ("sample", "InkPaint", [121, 120, 25, 7, 2, 111, 118, 29, 10, 10, 9]),
    ("sample", "Composite", [77, 76, 14, 2, 2, 64, 91, 17, 5, 7, 5]),
    ("sample", "Edit", [132, 131, 20, 3, 3, 146, 150, 28, 8, 13, 7]),
    ("sample", "Manga", [104, 102, 25, 2, 1, 105, 172, 25, 7, 9, 9]),
    ("sample", "Forge", [84, 83, 34, 2, 1, 93, 108, 21, 7, 5, 9]),
    ("sample", "Layout3d", [68, 65, 9, 4, 3, 65, 109, 16, 5, 6, 5]),
];

// ---- paper share (section 4.8) ----

/// What a screen leaves of the paper.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Share {
    /// Percent of the window that is bare paper.
    pub paper: f32,
    /// Percent of the window a pen can land on: bare canvas, paper or not.
    pub pen_land: f32,
    /// 4 pt samples on the paper that chrome covers.
    pub paper_under_chrome: usize,
}

const GRID: f32 = 4.0;

/// The 4 pt grid of 4.8 over the window. Never a pane's share: the window is
/// what the person has. A room with no canvas pane has no paper and scores
/// nothing.
pub fn paper_share(h: &EditorHarness) -> Share {
    let editor = h.editor();
    if !editor.pane_rects.iter().any(|(p, ..)| *p == Pane::Canvas) {
        return Share::default();
    }
    let ctx = h.ctx();
    let (rect, layer) = (editor.canvas.dbg_rect, editor.canvas.dbg_layer);
    let xf = editor.canvas.view_xform();
    let paper = Rect::from_min_size(Pos2::ZERO, xf.paper);
    // `layer_id_at` sees layers, never the widgets on one. The canvas's own
    // widget is the one holding its whole rect.
    let others: Vec<Rect> = ctx.viewport(|v| {
        let rows = v.prev_pass.widgets.get_layer(layer);
        rows.filter(|r| interactive(r.sense) && r.rect != rect).map(|r| r.interact_rect).collect()
    });
    let window = ctx.content_rect();
    let (mut total, mut bare, mut land, mut under) = (0usize, 0usize, 0usize, 0usize);
    let mut y = window.top() + GRID / 2.0;
    while y < window.bottom() {
        let mut x = window.left() + GRID / 2.0;
        while x < window.right() {
            let p = egui::pos2(x, y);
            total += 1;
            let clear = ctx.layer_id_at(p) == Some(layer) && !others.iter().any(|r| r.contains(p));
            let on_paper = paper.contains(xf.screen_to_paper(p));
            if rect.contains(p) {
                land += usize::from(clear);
                bare += usize::from(clear && on_paper);
                under += usize::from(!clear && on_paper);
            }
            x += GRID;
        }
        y += GRID;
    }
    let percent = |n: usize| 100.0 * n as f32 / total.max(1) as f32;
    Share { paper: percent(bare), pen_land: percent(land), paper_under_chrome: under }
}

// ---- clip audit ----

/// What an audit looks at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// Only what sits outside the dock: top row, band, rail, floats. Its tab
    /// bars go with its panels, which `All` gates (AU15).
    Chrome,
    All,
}

/// What a clip audit found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clip {
    /// Painted text cut by its clip by over 1 pt, not by a scroll viewport.
    pub clipped: usize,
    /// Controls past the window's edge.
    pub outside: usize,
    /// Text the layout elided.
    pub elided: usize,
    /// One line per finding.
    pub found: Vec<String>,
}

const SLACK: f32 = 1.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

const SIDES: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];

impl Side {
    fn of(self, r: Rect) -> f32 {
        match self {
            Side::Left => r.left(),
            Side::Right => r.right(),
            Side::Top => r.top(),
            Side::Bottom => r.bottom(),
        }
    }

    /// `inner` runs past `outer` on this side by over the slack.
    fn past(self, inner: Rect, outer: Rect) -> bool {
        match self {
            Side::Left | Side::Top => self.of(inner) < self.of(outer) - SLACK,
            Side::Right | Side::Bottom => self.of(inner) > self.of(outer) + SLACK,
        }
    }

    /// The axis that moves content past this side, as `Viewport::axes` has it.
    fn axis(self) -> usize {
        match self {
            Side::Left | Side::Right => 0,
            Side::Top | Side::Bottom => 1,
        }
    }

    /// The band this side of a viewport's content clip can fall in: its
    /// inner edge out to its reach, a parent clip landing between.
    fn band(self, v: &Viewport) -> (f32, f32) {
        let (edge, far) = (self.of(v.rect), self.of(v.reach));
        (edge.min(far), edge.max(far))
    }

    /// `a` and `b` share a stretch along this side.
    fn alongside(self, a: Rect, b: Rect) -> bool {
        match self {
            Side::Left | Side::Right => a.top() < b.bottom() && b.top() < a.bottom(),
            Side::Top | Side::Bottom => a.left() < b.right() && b.left() < a.right(),
        }
    }
}

/// A registered scroll viewport owns the clip edge `edge` on this side. Only
/// on an axis it scrolls: across one, the clip it sits under is a parent's.
fn owned(views: &[Viewport], side: Side, edge: f32, along: Rect) -> bool {
    views.iter().any(|v| {
        let (lo, hi) = side.band(v);
        v.axes[side.axis()] && side.alongside(v.rect, along) && (lo - SLACK..=hi + SLACK).contains(&edge)
    })
}

/// `cut` lies inside `body`. One-sided, so a control its clip took whole
/// reads too: its `interact_rect` is inverted, and still ends inside.
fn inside(cut: Rect, body: Rect) -> bool {
    let b = body.expand(SLACK);
    cut.min.x >= b.min.x && cut.min.y >= b.min.y && cut.max.x <= b.max.x && cut.max.y <= b.max.y
}

/// Every `Area` on screen, a hover card included: `layer_id_at` passes over
/// the ones that take no input. egui files the background layer as a
/// window-sized area of its own.
fn floats(ctx: &Context) -> Vec<Rect> {
    ctx.memory(|m| {
        let shown = m.areas().visible_layer_ids();
        shown.iter().filter(|l| **l != LayerId::background()).filter_map(|l| m.area_rect(l.id)).collect()
    })
}

/// One frame's findings. `Scope::Chrome` goes by what cut a shape: a dock
/// pane's clip means pane content, wherever panning left its middle. A float
/// over a pane stays chrome, even one kept to the pane's rect.
pub fn clip_frame(ctx: &Context, shapes: &[ClippedShape], scope: Scope) -> Clip {
    let views = plate::viewports(ctx);
    let (mut bodies, floats) = (plate::pane_bodies(ctx), floats(ctx));
    bodies.extend(plate::dock_rect(ctx));
    let window = ctx.content_rect();
    let in_body = |cut: Rect| scope == Scope::Chrome && bodies.iter().any(|b| inside(cut, *b));
    let mut out = Clip::default();
    for (t, clip) in texts_of(shapes) {
        let text = t.galley.text().trim();
        let bound = t.visual_bounding_rect();
        // A text shape keeps no layer, so a float is told by where it stands.
        // Text its clip took whole stands nowhere: the rail is a float over
        // every canvas, and the empty rect's middle can land on it.
        let shown = bound.intersect(clip);
        let on_float = shown.is_positive() && floats.iter().any(|f| f.contains(shown.center()));
        if text.is_empty() || (in_body(clip) && !on_float) {
            continue;
        }
        if t.galley.elided {
            out.elided += 1;
            out.found.push(format!("elided {text:?}"));
        }
        let clip = clip.intersect(window);
        // AU15: an empty clip paints nothing at all: a tab bar folded to no
        // height, a row scrolled clean away. No word stands cut on the screen.
        if !clip.is_positive() {
            continue;
        }
        let cut = |&side: &Side| side.past(bound, clip) && !owned(&views, side, side.of(clip), clip);
        if SIDES.iter().any(cut) {
            out.clipped += 1;
            out.found.push(format!("clipped {text:?} at {bound:?} by {clip:?}"));
        }
    }
    let area = window.area();
    let off = ctx.viewport(|v| {
        let mut off = Vec::new();
        let w = &v.prev_pass.widgets;
        for (_, rects) in w.layers() {
            for r in rects {
                let row = Row {
                    w: r.rect.width(),
                    h: r.rect.height(),
                    focus: r.sense.is_focusable(),
                    enabled: r.enabled,
                };
                if row.kind(area) != Kind::Control {
                    continue;
                }
                // `interact_rect` is the rect cut by its clip, so its edge
                // on a side the rect runs past IS the clip's.
                let at = r.interact_rect;
                let past = |&side: &Side| {
                    side.past(r.rect, window) && !owned(&views, side, side.of(at), r.rect)
                };
                let pane = r.layer_id == LayerId::background() && in_body(at);
                // Panned off past a corner, no side of the view runs along it.
                let panned = views.iter().any(|v| v.axes == [true, true] && inside(at, v.reach));
                if !pane && !panned && SIDES.iter().any(past) {
                    let name = w.info(r.id).and_then(|i| i.label.clone()).unwrap_or_default();
                    off.push((r.rect, name));
                }
            }
        }
        off
    });
    for (rect, name) in off {
        out.outside += 1;
        out.found.push(format!("outside {name:?} at {rect:?}"));
    }
    out
}

/// This frame as it stands.
pub fn clip_here(h: &EditorHarness, scope: Scope) -> Clip {
    clip_frame(&h.ctx(), &h.kit().output().shapes, scope)
}

/// Every room in `PROBE_ROOMS`, entered as its button enters it, added up,
/// and AU27 each step with the lesson card beside it. The harness stays in
/// the last room.
pub fn clip_audit(h: &mut EditorHarness, scope: Scope) -> Clip {
    let mut total = Clip::default();
    for room in PROBE_ROOMS.into_iter().chain(LESSON_ROOMS) {
        room.enter(h.editor_mut());
        settle(h);
        let here = clip_here(h, scope);
        total.clipped += here.clipped;
        total.outside += here.outside;
        total.elided += here.elided;
        let tag = room_tag(room);
        total.found.extend(here.found.into_iter().map(|f| format!("{tag}: {f}")));
    }
    total
}

// ---- text contrast audit ----

/// A colour as painted: gamma-space rgb, and the coverage alpha.
type Rgba = [f32; 4];

fn unmultiplied(c: Color32) -> Rgba {
    c.to_srgba_unmultiplied().map(|v| f32::from(v) / 255.0)
}

/// egui blends in gamma space.
fn over(top: Rgba, under: [f32; 3]) -> [f32; 3] {
    let a = top[3];
    [0, 1, 2].map(|i| top[i] * a + under[i] * (1.0 - a))
}

fn luminance(c: [f32; 3]) -> f32 {
    let linear = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2])
}

/// The WCAG contrast ratio.
pub fn ratio(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `ink` at `opacity`, its own alpha included, over `ground`: what the eye
/// gets from it.
pub fn effective(ink: Color32, opacity: f32, ground: [f32; 3]) -> [f32; 3] {
    let mut c = unmultiplied(ink);
    c[3] *= opacity;
    over(c, ground)
}

/// What lies under a point of the frame so far.
#[derive(Clone, Copy)]
enum Ground {
    Solid(Rect, Rgba),
    /// A mesh, an image or a shape the audit cannot read a colour from.
    Unknown(Rect),
}

impl Ground {
    fn rect(&self) -> Rect {
        match self {
            Ground::Solid(r, _) | Ground::Unknown(r) => *r,
        }
    }
}

/// The filled shapes under `at`, top down to the first opaque one, blended
/// back up. None is a ground the audit cannot vouch for.
fn backdrop(grounds: &[Ground], at: Pos2) -> Option<[f32; 3]> {
    let mut glazes: Vec<Rgba> = Vec::new();
    for g in grounds.iter().rev().filter(|g| g.rect().contains(at)) {
        match g {
            Ground::Unknown(_) => return None,
            Ground::Solid(_, c) if c[3] >= 1.0 => {
                let base = [c[0], c[1], c[2]];
                return Some(glazes.iter().rev().fold(base, |under, g| over(*g, under)));
            }
            Ground::Solid(_, c) => glazes.push(*c),
        }
    }
    None
}

/// What a contrast audit found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Contrast {
    /// Text sections it could judge.
    pub checked: usize,
    /// Under their floor: the words, the ratio, the floor.
    pub failures: Vec<String>,
    /// Over a mesh, an image or no opaque ground: listed, not passed.
    pub unknown: Vec<String>,
}

fn judge_text(t: &TextShape, clip: Rect, grounds: &[Ground], out: &mut Contrast) {
    let bound = t.visual_bounding_rect().intersect(clip);
    if !bound.is_positive() {
        return;
    }
    let at = bound.center();
    let job = &t.galley.job;
    for section in &job.sections {
        let words = job.text[section.byte_range.start.0..section.byte_range.end.0].trim();
        if words.is_empty() {
            continue;
        }
        let colour = section.format.color;
        let colour = if colour == plate::SECTION_PLACEHOLDER { t.fallback_color } else { colour };
        let ink = t.override_text_color.unwrap_or(colour);
        let floor = if t.opacity_factor < 1.0 || ink == plate::INK3 { 3.0 } else { 4.5 };
        let Some(mut ground) = backdrop(grounds, at) else {
            out.unknown.push(format!("{words:?} at {at:?}"));
            continue;
        };
        if section.format.background.a() > 0 {
            ground = over(unmultiplied(section.format.background), ground);
        }
        out.checked += 1;
        let got = ratio(effective(ink, t.opacity_factor, ground), ground);
        if got < floor {
            out.failures.push(format!("{words:?} {got:.2} under {floor:.1} at {at:?}"));
        }
    }
}

fn walk_contrast(shape: &Shape, clip: Rect, grounds: &mut Vec<Ground>, out: &mut Contrast) {
    let unknown = |fill: Color32, grounds: &mut Vec<Ground>| {
        if fill.a() > 0 {
            grounds.push(Ground::Unknown(shape.visual_bounding_rect().intersect(clip)));
        }
    };
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| walk_contrast(s, clip, grounds, out)),
        Shape::Text(t) => judge_text(t, clip, grounds, out),
        Shape::Rect(r) if r.fill_texture_id() == egui::TextureId::default() => {
            if r.fill.a() > 0 {
                grounds.push(Ground::Solid(r.rect.intersect(clip), unmultiplied(r.fill)));
            }
        }
        Shape::Rect(r) => unknown(r.fill, grounds),
        Shape::Mesh(m) => {
            if !m.is_empty() {
                grounds.push(Ground::Unknown(m.calc_bounds().intersect(clip)));
            }
        }
        Shape::Circle(c) => unknown(c.fill, grounds),
        Shape::Ellipse(e) => unknown(e.fill, grounds),
        Shape::Path(p) => unknown(p.fill, grounds),
        Shape::QuadraticBezier(b) => unknown(b.fill, grounds),
        Shape::CubicBezier(b) => unknown(b.fill, grounds),
        Shape::Callback(c) => grounds.push(Ground::Unknown(c.rect.intersect(clip))),
        Shape::Noop | Shape::LineSegment { .. } => {}
    }
}

/// A frame's painted text in paint order, against what lay under it.
pub fn contrast_of(shapes: &[ClippedShape]) -> Contrast {
    let mut out = Contrast::default();
    let mut grounds = Vec::new();
    for c in shapes {
        walk_contrast(&c.shape, c.clip_rect, &mut grounds, &mut out);
    }
    out
}

/// This screen after it settles. Menus and modals: open them first.
pub fn text_contrast_audit(h: &mut EditorHarness) -> Contrast {
    settle(h);
    contrast_of(&h.kit().output().shapes)
}

// ---- journeys ----

/// One scripted step, as a person does it.
pub enum Act<'a> {
    /// Click the one widget with this name.
    Click(&'a str),
    /// A verb run from its button or menu row: one click.
    Perform(Action),
    Key(egui::Key),
    Chord(egui::Modifiers, egui::Key),
    /// A pen stroke along these points.
    Stroke(&'a [Pos2]),
    /// AU14: one drag along these points that lays no stroke: a slider, a box.
    Drag(&'a [Pos2]),
    /// A dialog answered: `h.click` on a button, or the seam behind an OS
    /// dialog. One action, whatever it takes to answer it.
    Answer(Box<dyn FnOnce(&mut EditorHarness) + 'a>),
}

/// What a journey cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub clicks: usize,
    pub keys: usize,
    pub strokes: usize,
    pub dialogs: usize,
}

impl Tally {
    pub fn actions(self) -> usize {
        self.clicks + self.keys + self.strokes + self.dialogs
    }
}

/// A named script over the harness. Every step is counted, and a step that
/// was refused, or that ran a verb which answered `Performed::Ignored`,
/// fails the journey by name: a refused step never scores as done. A stroke
/// that left no stroke behind fails the same way.
pub struct Recorder<'h> {
    h: &'h mut EditorHarness,
    journey: &'static str,
    tally: Tally,
}

impl<'h> Recorder<'h> {
    pub fn new(h: &'h mut EditorHarness, journey: &'static str) -> Self {
        Self { h, journey, tally: Tally::default() }
    }

    /// One step, or the journey ends here.
    #[track_caller]
    pub fn act(&mut self, step: &str, act: Act) {
        if let Err(why) = self.try_act(step, act) {
            panic!("{why}");
        }
    }

    /// One step, with the failure as a value.
    pub fn try_act(&mut self, step: &str, act: Act) -> Result<(), String> {
        let refusals = self.h.state().refusal_seq;
        let ignored = self.h.editor().ignored.len();
        let strokes = self.h.state().strokes_done;
        let stroke = matches!(act, Act::Stroke(_));
        match act {
            Act::Click(label) => {
                self.h.click(label);
                self.tally.clicks += 1;
            }
            Act::Perform(action) => {
                self.h.perform(action);
                self.tally.clicks += 1;
            }
            Act::Key(key) => {
                self.h.key(key);
                self.tally.keys += 1;
            }
            Act::Chord(mods, key) => {
                self.h.key_mods(mods, key);
                self.tally.keys += 1;
            }
            Act::Stroke(path) => {
                self.h.drag_path(path);
                self.tally.strokes += 1;
            }
            Act::Drag(path) => {
                self.h.drag_path(path);
                self.tally.clicks += 1;
            }
            Act::Answer(answer) => {
                answer(self.h);
                self.tally.dialogs += 1;
            }
        }
        let at = format!("journey {:?}, step {step:?}", self.journey);
        let state = self.h.state();
        if state.refusal_seq != refusals {
            return Err(format!("{at}: refused: {}", state.refusal));
        }
        if let Some(action) = self.h.editor().ignored.get(ignored) {
            return Err(format!("{at}: {action:?} was ignored"));
        }
        if stroke && state.strokes_done == strokes {
            return Err(format!("{at}: no stroke landed"));
        }
        Ok(())
    }

    pub fn tally(&self) -> Tally {
        self.tally
    }

    /// The harness back, for what the journey asserts at its end.
    pub fn harness(&mut self) -> &mut EditorHarness {
        self.h
    }
}

#[cfg(test)]
mod tests {
    use egui::epaint::ColorMode;
    use egui::{Align2, FontId, Label, pos2, vec2};
    use egui_kittest::Harness;

    use super::*;
    use crate::floats::Float;
    use crate::newproject::NewProjectForm;
    use crate::workspace::Stage;

    const SEVERAL_WORDS: &str = "A label far too long for the room it was given";

    /// A bare frame with only what `paint` puts there.
    fn fixture(mut paint: impl FnMut(&mut egui::Ui) + 'static) -> Harness<'static> {
        let mut h = Harness::builder()
            .with_size(vec2(400.0, 300.0))
            .with_pixels_per_point(1.0)
            .build_ui(move |ui| paint(ui));
        h.run_steps(3);
        h
    }

    fn clip_of(h: &Harness<'static>) -> Clip {
        clip_frame(&h.ctx, &h.output().shapes, Scope::All)
    }

    fn chrome_of(h: &Harness<'static>) -> Clip {
        clip_frame(&h.ctx, &h.output().shapes, Scope::Chrome)
    }

    /// A dock pane's body as `EditorTabs` reports one: its clip on `body`.
    fn pane_body(ui: &mut egui::Ui, body: Rect, paint: impl FnOnce(&mut egui::Ui)) {
        let mut pane = ui.new_child(egui::UiBuilder::new().max_rect(body));
        pane.set_clip_rect(body);
        plate::note_pane_body(&pane);
        paint(&mut pane);
    }

    /// `SEVERAL_WORDS` cut by `clip`.
    fn cut_words(painter: &egui::Painter, clip: Rect, at: Pos2) {
        let font = FontId::proportional(14.0);
        painter.with_clip_rect(clip).text(at, Align2::LEFT_TOP, SEVERAL_WORDS, font, plate::SEED_WHITE);
    }

    fn ink(ui: &egui::Ui, at: Pos2, colour: Color32) {
        let font = FontId::proportional(14.0);
        ui.painter().text(at, Align2::LEFT_TOP, "Clear words", font, colour);
    }

    fn ground(ui: &egui::Ui, colour: Color32) {
        ui.painter().rect_filled(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0)), 0.0, colour);
    }

    fn contrast_fixture(mut paint: impl FnMut(&mut egui::Ui) + 'static) -> Contrast {
        let h = fixture(move |ui| paint(ui));
        contrast_of(&h.output().shapes)
    }

    /// Blank Drawing, as the journeys measured it.
    fn blank_drawing() -> EditorHarness {
        let mut h = EditorHarness::with_state(NewProjectForm::default().new_state());
        h.editor_mut().enter_room(Stage::Drawing);
        h.kit_mut().set_size(vec2(1600.0, 900.0));
        settle(&mut h);
        h
    }

    // ---- census ----

    /// A recording of Blank Drawing from the day the baseline was taken: the
    /// port is held to the journeys' numbers however the screen moves on.
    const BLANK_DRAWING: &str = include_str!("census-blank-drawing.tsv");

    #[test]
    fn auseam_census_reads_the_baseline() {
        let c = Reading::parse(BLANK_DRAWING).census(TODAY_TERMS);
        assert_eq!((c.controls, c.words, c.jargon), (72, 100, 30), "journeys.md: {:?}", c.terms);
        assert_eq!(c.row(), [72, 71, 18, 2, 2, 92, 100, 30, 9, 10, 11], "{:?}", c.terms);
        let row = BASELINE.iter().find(|(doc, room, _)| *doc == "blank" && *room == "Drawing");
        assert_eq!(row.map(|r| r.2), Some(c.row()), "the const table agrees");
        // journeys.md's other readings of the same screen.
        assert_eq!(c.median_control_height(), 24.0);
        assert_eq!((c.controls_under(24.0), c.controls_under(20.0)), (30, 14));
        assert_eq!(c.median_text_height(), 8.0);
        assert_eq!((c.text_heights.len(), c.texts_at_most(8.0), c.texts_at_most(7.0)), (98, 58, 23));
        assert!((c.largest_backdrop - 54.5).abs() < 0.05, "{}", c.largest_backdrop);
    }

    #[test]
    fn auseam_census_holds_the_49_terms_and_their_edges() {
        assert_eq!(TODAY_TERMS, 49);
        assert_eq!(&TERMS[..TODAY_TERMS].iter().filter(|t| t.2 == Tier::Japanese).count(), &20);
        let kinds = |s: &str| {
            let reading = Reading { area: 1.0, rows: vec![], texts: vec![(8.0, s.into())] };
            reading.census(TODAY_TERMS).terms
        };
        assert_eq!(kinds("Genga pencil"), vec!["genga"]);
        assert_eq!(kinds("SHUSEI"), vec!["shusei"], "case-blind");
        assert_eq!(kinds("cels"), vec!["cel"]);
        assert_eq!(kinds("cancel"), Vec::<&str>::new(), "a word, not a part");
        assert_eq!(kinds("in"), vec!["in/out"]);
        assert_eq!(kinds("in time"), Vec::<&str>::new(), "the whole string or nothing");
        assert_eq!(kinds("X-Sheet"), vec!["x-sheet"]);
        assert_eq!(kinds("henshu"), vec!["henshū"]);
        assert_eq!(kinds("原画"), vec!["kanji glyphs"]);
        assert_eq!(kinds("tebureish"), vec!["tebure hosei"], "this one is a part");
    }

    #[test]
    fn auseam_census_row_kinds_are_the_definitions() {
        let area = 1600.0 * 900.0;
        let row = |w, h, focus| Row { w, h, focus, enabled: true }.kind(area);
        assert_eq!(row(100.0, 24.0, true), Kind::Control);
        assert_eq!(row(100.0, 24.0, false), Kind::Label, "not focusable");
        assert_eq!(row(100.0, 6.0, true), Kind::Seam, "short side at 6");
        assert_eq!(row(100.0, 7.0, true), Kind::Control, "short side over 6");
        assert_eq!(row(800.0, 90.0, true), Kind::Backdrop, "at 5% of the window");
        assert_eq!(row(800.0, 89.0, true), Kind::Control, "under 5%");
    }

    /// Records today's rooms: with BASELINE_JSON set (the way UISHOT_DIR
    /// works), the file gets them, a fixture dump lands beside it, and the
    /// const table must say the same. Without it nothing runs, so this stays
    /// a recording of its day and never a check on a later screen.
    #[test]
    #[ignore = "records the baseline; set BASELINE_JSON to the output file"]
    fn auseam_census_records_todays_rooms() {
        let Ok(path) = std::env::var("BASELINE_JSON") else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        let mut rows = Vec::new();
        for (doc, state) in [
            ("blank", NewProjectForm::default().new_state()),
            ("sample", crate::sample::build_sample()),
        ] {
            let mut h = EditorHarness::with_state(state);
            if doc == "blank" {
                h.editor_mut().enter_room(Stage::Drawing);
            }
            h.kit_mut().set_size(vec2(1600.0, 900.0));
            settle(&mut h);
            if doc == "blank" {
                let dump = reading(&h).dump();
                std::fs::write(path.with_file_name("census-blank-drawing.tsv"), dump).unwrap();
            }
            for room in PROBE_ROOMS {
                room.enter(h.editor_mut());
                settle(&mut h);
                let c = reading(&h).census(TODAY_TERMS);
                let share = paper_share(&h);
                let clip = clip_here(&h, Scope::All);
                let chrome = clip_here(&h, Scope::Chrome);
                let contrast = contrast_of(&h.kit().output().shapes);
                rows.push(serde_json::json!({
                    "doc": doc,
                    "room": room_tag(room),
                    "census": c.row(),
                    "terms": c.terms,
                    "median_control_height": c.median_control_height(),
                    "controls_under_24": c.controls_under(24.0),
                    "median_text_height": c.median_text_height(),
                    "texts_at_most_8": c.texts_at_most(8.0),
                    "texts": c.text_heights.len(),
                    "largest_backdrop": c.largest_backdrop,
                    "paper": share.paper,
                    "pen_land": share.pen_land,
                    "paper_under_chrome": share.paper_under_chrome,
                    "clip_all": [clip.clipped, clip.outside, clip.elided],
                    "clip_chrome": [chrome.clipped, chrome.outside, chrome.elided],
                    "contrast": [contrast.checked, contrast.failures.len(), contrast.unknown.len()],
                    "canvas": format!("{:?}", h.editor().canvas.dbg_rect),
                    "zoom": h.editor().canvas.view_xform().zoom,
                    "clip_found": clip.found,
                    "contrast_failures": contrast.failures,
                }));
                println!("{doc} {} {:?}", room_tag(room), c.row());
            }
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&rows).unwrap()).expect("write the baseline");
        for (at, (doc, room, want)) in BASELINE.iter().enumerate() {
            assert_eq!(rows[at]["census"], serde_json::json!(want), "{doc} {room}");
        }
        // The rows above sit in the harness's 8 pt inset, as the journeys
        // did. The same room on the person's window, beside them.
        let mut flush = EditorHarness::as_shipped();
        flush.editor_mut().enter_room(Stage::Drawing);
        flush.kit_mut().set_size(vec2(1600.0, 900.0));
        settle(&mut flush);
        let share = paper_share(&flush);
        let row = serde_json::json!({
            "paper": share.paper,
            "pen_land": share.pen_land,
            "paper_under_chrome": share.paper_under_chrome,
            "canvas": format!("{:?}", flush.editor().canvas.dbg_rect),
            "zoom": flush.editor().canvas.view_xform().zoom,
        });
        println!("flush blank Drawing {row}");
        std::fs::write(path.with_file_name("flush-blank-drawing.json"), row.to_string()).expect("write it");
    }

    // ---- paper share ----

    /// The middle of the paper on screen.
    fn over_the_paper(h: &EditorHarness) -> Pos2 {
        h.editor().canvas.dbg_rect.center() - vec2(60.0, 60.0)
    }

    #[test]
    fn auseam_paper_share_counts_a_float_as_chrome() {
        let mut h = blank_drawing();
        let clean = paper_share(&h);
        assert!(clean.paper > 20.0 && clean.pen_land >= clean.paper, "{clean:?}");
        let at = over_the_paper(&h);
        h.rig_mut().seed = Some(Box::new(move |ui| {
            let float = egui::Area::new(egui::Id::new("seed_float")).fixed_pos(at);
            float.show(ui.ctx(), |ui| {
                ui.allocate_space(vec2(120.0, 120.0));
            });
        }));
        h.run_frames(3);
        let seeded = paper_share(&h);
        let covered = seeded.paper_under_chrome - clean.paper_under_chrome;
        // 120 pt square over a 4 pt grid.
        assert!((840..=960).contains(&covered), "{covered} samples: {seeded:?}");
        assert!(seeded.paper < clean.paper && seeded.pen_land < clean.pen_land, "{seeded:?}");
        // The clean fixture again.
        h.rig_mut().seed = None;
        h.run_frames(3);
        assert_eq!(paper_share(&h), clean);
    }

    #[test]
    fn auseam_paper_share_counts_a_same_layer_widget() {
        let mut h = blank_drawing();
        let clean = paper_share(&h);
        let rect = Rect::from_min_size(over_the_paper(&h), vec2(120.0, 120.0));
        h.rig_mut().seed = Some(Box::new(move |ui| {
            ui.interact(rect, egui::Id::new("seed_widget"), Sense::click());
        }));
        h.run_frames(3);
        let seeded = paper_share(&h);
        // `layer_id_at` cannot see this one, the widget list can.
        let canvas = h.editor().canvas.dbg_layer;
        assert_eq!(h.ctx().layer_id_at(rect.center()), Some(canvas));
        let covered = seeded.paper_under_chrome - clean.paper_under_chrome;
        assert!((840..=960).contains(&covered), "{covered} samples: {seeded:?}");
        assert!(seeded.paper < clean.paper && seeded.pen_land < clean.pen_land, "{seeded:?}");
    }

    #[test]
    fn auseam_paper_share_is_nothing_without_a_canvas_pane() {
        let mut h = blank_drawing();
        h.editor_mut().enter_room(Stage::Composite);
        settle(&mut h);
        let tabs = &h.editor().pane_rects;
        assert!(!tabs.is_empty() && !tabs.iter().any(|(p, ..)| *p == Pane::Canvas), "{tabs:?}");
        assert_eq!(paper_share(&h), Share::default());
    }

    /// 4.1's arithmetic on the screen the person has: the App lays out from
    /// the window's edge, and the share is of the whole window.
    #[test]
    fn auseam_paper_share_reads_the_persons_window() {
        let mut h = EditorHarness::as_shipped();
        // The frozen boot keeps the sheet: like for like (AU3). AU19: only with
        // no kind stamped, so that goes too.
        h.editor_mut().sheet_in_draw = true;
        h.editor_mut().unset_kind();
        h.editor_mut().enter_room(Stage::Drawing);
        h.kit_mut().set_size(vec2(1600.0, 900.0));
        settle(&mut h);
        let pane = h.editor().canvas.dbg_rect;
        let inset = blank_drawing();
        let framed = inset.editor().canvas.dbg_rect;
        assert_eq!(pane.height() - framed.height(), 16.0, "{pane:?} against {framed:?}");
        assert!(pane.width() > framed.width(), "{pane:?} against {framed:?}");
        let xf = h.editor().canvas.view_xform();
        let corners = [xf.paper_to_screen(Pos2::ZERO), xf.paper_to_screen(xf.paper.to_pos2())];
        let on_screen = Rect::from_two_pos(corners[0], corners[1]).intersect(pane);
        let share = paper_share(&h);
        let window = 1600.0 * 900.0;
        let by_area = 100.0 * on_screen.area() / window;
        let by_grid = share.paper + 100.0 * (share.paper_under_chrome as f32 * GRID * GRID) / window;
        assert!((by_grid - by_area).abs() < 0.6, "{by_grid} on the grid, {by_area} by area: {share:?}");
        assert!(share.paper > paper_share(&inset).paper, "the 8 pt the harness took are back");
    }

    // ---- clip audit ----

    #[test]
    fn auseam_clip_audit_fails_a_cut_label() {
        let cut = fixture(|ui| {
            let rect = Rect::from_min_size(pos2(20.0, 20.0), vec2(40.0, 20.0));
            let font = FontId::proportional(14.0);
            ui.painter_at(rect).text(rect.min, Align2::LEFT_TOP, SEVERAL_WORDS, font, plate::SEED_WHITE);
        });
        let clean = fixture(|ui| {
            let rect = Rect::from_min_size(pos2(20.0, 20.0), vec2(360.0, 20.0));
            let font = FontId::proportional(12.0);
            ui.painter_at(rect).text(rect.min, Align2::LEFT_TOP, "Fits", font, plate::SEED_WHITE);
        });
        assert_eq!(clip_of(&clean), Clip::default());
        let got = clip_of(&cut);
        assert_eq!((got.clipped, got.outside, got.elided), (1, 0, 0), "{:?}", got.found);
        assert!(got.found[0].contains("far too long"), "{:?}", got.found);
    }

    #[test]
    fn auseam_clip_audit_walks_the_rooms_and_keeps_its_scope() {
        let mut h = EditorHarness::with_state(NewProjectForm::default().new_state());
        let (all, chrome) = (clip_audit(&mut h, Scope::All), clip_audit(&mut h, Scope::Chrome));
        assert!(chrome.clipped <= all.clipped && chrome.elided <= all.elided, "{chrome:?} {all:?}");
        let cut_label = |at: Rect| -> Box<dyn FnMut(&mut egui::Ui)> {
            Box::new(move |ui| {
                let font = FontId::proportional(14.0);
                ui.painter_at(at).text(at.min, Align2::LEFT_TOP, SEVERAL_WORDS, font, plate::SEED_WHITE);
            })
        };
        // A cut label on the top row: every room, one finding each.
        h.rig_mut().seed = Some(cut_label(Rect::from_min_size(pos2(1.0, 1.0), vec2(40.0, 12.0))));
        let seeded = clip_audit(&mut h, Scope::Chrome);
        let rooms = PROBE_ROOMS.len() + LESSON_ROOMS.len();
        assert_eq!(seeded.clipped, chrome.clipped + rooms, "{:?}", seeded.found);
        assert!(seeded.found.iter().any(|f| f.starts_with("Forge:")), "named by room");
        // One inside a pane body: `All` has it, `Chrome` leaves it out. The
        // last room's float would stand on it: a text is told by where it stands.
        h.rig_mut().seed = None;
        h.editor_mut().start = None;
        h.editor_mut().enter_room(Stage::Drawing);
        settle(&mut h);
        let (all, chrome) = (clip_here(&h, Scope::All), clip_here(&h, Scope::Chrome));
        let body = h.editor().canvas.dbg_rect.center();
        h.rig_mut().seed = Some(cut_label(Rect::from_min_size(body, vec2(40.0, 12.0))));
        settle(&mut h);
        assert_eq!(clip_here(&h, Scope::All).clipped, all.clipped + 1);
        assert_eq!(clip_here(&h, Scope::Chrome).clipped, chrome.clipped);
        // Hanging over a pane's foot, its middle past the pane: it is the
        // pane's clip that cuts it, so it is still the pane's.
        h.rig_mut().seed = None;
        settle(&mut h);
        let body = plate::pane_bodies(&h.ctx()).into_iter().find(|b| b.contains(body)).expect("a pane");
        let at = pos2(body.left() + 20.0, body.bottom() - 6.0);
        h.rig_mut().seed = Some(Box::new(move |ui| cut_words(ui.painter(), body, at)));
        settle(&mut h);
        let shapes = &h.kit().output().shapes;
        let (seeded, _) = texts_of(shapes).into_iter().find(|(t, _)| t.galley.text() == SEVERAL_WORDS).unwrap();
        let middle = seeded.visual_bounding_rect().center();
        assert!(!plate::pane_bodies(&h.ctx()).iter().any(|b| b.contains(middle)), "past the pane");
        assert_eq!(clip_here(&h, Scope::All).clipped, all.clipped + 1);
        assert_eq!(clip_here(&h, Scope::Chrome).clipped, chrome.clipped);
    }

    #[test]
    fn auseam_clip_audit_keeps_a_float_over_a_pane() {
        let body = Rect::from_min_max(pos2(20.0, 20.0), pos2(380.0, 280.0));
        let words = Rect::from_min_size(pos2(100.0, 100.0), vec2(40.0, 12.0));
        // The hover card's shape takes no input: `layer_id_at` never sees it.
        for (order, interactable) in [(egui::Order::Middle, true), (egui::Order::Tooltip, false)] {
            let float = fixture(move |ui| {
                pane_body(ui, body, |_| {});
                let area = egui::Area::new(egui::Id::new("seed_card")).order(order).interactable(interactable);
                area.fixed_pos(words.min).constrain_to(body).show(ui.ctx(), |ui| {
                    ui.allocate_space(vec2(120.0, 40.0));
                    cut_words(ui.painter(), words, words.min);
                });
            });
            let got = chrome_of(&float);
            assert_eq!(got.clipped, 1, "{order:?}: {:?}", got.found);
            let top = float.ctx.layer_id_at(words.center()).is_none_or(|l| l == LayerId::background());
            assert_eq!(top, !interactable, "{order:?}: the layer lookup sees only the first");
        }
        // The same words on the pane, with no float: its own, so left out.
        let plain = fixture(move |ui| pane_body(ui, body, |pane| cut_words(pane.painter(), words, words.min)));
        assert_eq!(clip_of(&plain).clipped, 1);
        assert_eq!(chrome_of(&plain).clipped, 0);
    }

    #[test]
    fn auseam_clip_audit_reads_a_control_by_its_clip() {
        let body = Rect::from_min_max(pos2(200.0, 40.0), pos2(400.0, 260.0));
        let button = |x: f32| Rect::from_min_size(pos2(x, 60.0), vec2(80.0, 24.0));
        // A list scrolled sideways in a pane on the window's edge: one half
        // gone, its middle out of the window; one gone whole.
        for x in [380.0, 420.0] {
            let panned = fixture(move |ui| {
                pane_body(ui, body, |pane| {
                    pane.put(button(x), egui::Button::new("Off"));
                });
            });
            assert_eq!(clip_of(&panned).outside, 1, "{:?}", clip_of(&panned).found);
            assert_eq!(chrome_of(&panned).outside, 0, "{:?}", chrome_of(&panned).found);
        }
        // Off the window beside the pane: nothing of a pane's cut it.
        let chrome = fixture(move |ui| {
            pane_body(ui, body, |_| {});
            ui.put(Rect::from_min_size(pos2(380.0, 10.0), vec2(80.0, 24.0)), egui::Button::new("Off"));
        });
        assert_eq!(chrome_of(&chrome).outside, 1, "{:?}", chrome_of(&chrome).found);
        // On a float over the pane: the widget's layer says it is no pane's.
        let float = fixture(move |ui| {
            pane_body(ui, body, |_| {});
            let area = egui::Area::new(egui::Id::new("seed_float")).fixed_pos(pos2(210.0, 60.0));
            area.constrain_to(body).show(ui.ctx(), |ui| {
                ui.put(button(380.0), egui::Button::new("Off"));
            });
        });
        assert_eq!(chrome_of(&float).outside, 1, "{:?}", chrome_of(&float).found);
    }

    #[test]
    fn auseam_clip_audit_skips_a_scroll_viewport() {
        let rows = |ui: &mut egui::Ui| {
            for i in 0..80 {
                ui.label(format!("row {i}"));
            }
        };
        // 132 tall lands the viewport's edge across a row: 137 to 146 at 18
        // apart, the clip at 140.
        let registered = fixture(move |ui| {
            let out = egui::ScrollArea::vertical().max_height(132.0).show(ui, rows);
            plate::note_viewport(ui, out.inner_rect, [false, true]);
        });
        let got = clip_of(&registered);
        assert_eq!((got.clipped, got.outside), (0, 0), "{:?}", got.found);
        // The same list with no note: its cut row is a finding. The audit
        // skips only what was registered.
        let unregistered = fixture(move |ui| {
            egui::ScrollArea::vertical().max_height(132.0).show(ui, rows);
        });
        let got = clip_of(&unregistered);
        assert_eq!(got.clipped, 1, "{:?}", got.found);
        assert!(got.found[0].contains("row 7"), "{:?}", got.found);
        // A label cut across the scroll, not along it, is still a finding.
        let across = fixture(|ui| {
            let out = egui::ScrollArea::vertical().max_width(60.0).max_height(120.0).show(ui, |ui| {
                let font = FontId::proportional(14.0);
                let at = ui.next_widget_position();
                ui.painter_at(Rect::from_min_size(at, vec2(50.0, 20.0))).text(
                    at,
                    Align2::LEFT_TOP,
                    SEVERAL_WORDS,
                    font,
                    plate::SEED_WHITE,
                );
                ui.allocate_space(vec2(50.0, 20.0));
            });
            plate::note_viewport(ui, out.inner_rect, [false, true]);
        });
        assert_eq!(clip_of(&across).clipped, 1);
    }

    /// A list filling a host whose clip is the host, so the parent's clip
    /// sits on the viewport's own edges: one line too long, one row cut by
    /// the foot. `noted` is what the note claims scrolls.
    fn hosted(axes: [bool; 2], noted: [bool; 2]) -> Harness<'static> {
        fixture(move |ui| {
            let host = Rect::from_min_size(pos2(20.0, 20.0), vec2(120.0, 116.0));
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(host));
            inner.set_clip_rect(host);
            let out = egui::ScrollArea::new(axes).show(&mut inner, |ui| {
                ui.add(Label::new(SEVERAL_WORDS).extend());
                for i in 0..20 {
                    ui.label(format!("row {i}"));
                }
            });
            plate::note_viewport(&inner, out.inner_rect, noted);
        })
    }

    #[test]
    fn auseam_clip_audit_owns_only_the_scrolled_axis() {
        let edges = |h: &Harness<'static>| {
            let view = plate::viewports(&h.ctx).first().expect("noted").rect;
            (view.right(), view.bottom())
        };
        // Up and down: the foot is the scroll's, the side is the parent's.
        let list = hosted([false, true], [false, true]);
        assert_eq!(edges(&list), (140.0, 136.0), "the parent's clip on the viewport's edges");
        let got = clip_of(&list);
        assert_eq!(got.clipped, 1, "{:?}", got.found);
        assert!(got.found[0].contains("far too long"), "{:?}", got.found);
        // Left and right: the other way about.
        let strip = hosted([true, false], [true, false]);
        assert_eq!(edges(&strip), (140.0, 136.0), "the parent's clip on the viewport's edges");
        let got = clip_of(&strip);
        assert_eq!(got.clipped, 1, "{:?}", got.found);
        assert!(got.found[0].contains("row"), "{:?}", got.found);
        // Only the note's axes changed: they are what decides.
        let both = hosted([false, true], [true, true]);
        assert_eq!(clip_of(&both).clipped, 0, "{:?}", clip_of(&both).found);
    }

    /// A view panned by hand owns what it cuts on every side, out to `reach`.
    #[test]
    fn auseam_clip_audit_owns_a_pan_view() {
        let rect = Rect::from_min_size(pos2(40.0, 40.0), vec2(120.0, 80.0));
        let findings = |reach: Option<Rect>| {
            let panned = fixture(move |ui| {
                if let Some(reach) = reach {
                    plate::note_pan_view(ui, rect, reach);
                }
                cut_words(ui.painter(), rect, rect.left_bottom() - vec2(0.0, 6.0));
                ui.put(Rect::from_min_size(pos2(370.0, 60.0), vec2(80.0, 24.0)), egui::Button::new(""));
            });
            let got = clip_of(&panned);
            (got.clipped, got.outside)
        };
        assert_eq!(findings(None), (1, 1));
        assert_eq!(findings(Some(rect)), (0, 1), "its words stop at the rect, the control runs on");
        assert_eq!(findings(Some(rect.with_max_x(400.0))), (0, 0), "the control's clip is in reach");
    }

    /// The sample's node graph, panned about: nodes leave the view past its
    /// sides and corners, and none of that is a cut label or a lost control.
    /// Panned up over the pane's toolbar is another matter: a node's pins
    /// stay live there, so past the window's edge they stay findings.
    #[test]
    fn auseam_clip_audit_reads_the_node_graph_as_a_pan_view() {
        let mut h = EditorHarness::with_state(crate::sample::build_sample());
        h.editor_mut().enter_room(Stage::Edit);
        h.kit_mut().set_size(vec2(1600.0, 900.0));
        for pan in [(0.0, 0.0), (900.0, 250.0), (-300.0, -120.0), (500.0, 60.0), (-60.0, 400.0)] {
            h.state_mut().graph_pan = pan;
            settle(&mut h);
            let got = clip_here(&h, Scope::All);
            assert_eq!((got.clipped, got.outside), (0, 0), "{pan:?}: {:?}", got.found);
        }
    }

    #[test]
    fn auseam_clip_audit_counts_elision() {
        let elided = fixture(|ui| {
            ui.set_max_width(60.0);
            ui.add(Label::new(SEVERAL_WORDS).truncate());
        });
        let got = clip_of(&elided);
        assert_eq!((got.elided, got.clipped), (1, 0), "{:?}", got.found);
        let whole = fixture(|ui| {
            ui.add(Label::new("Fits").truncate());
        });
        assert_eq!(clip_of(&whole), Clip::default());
    }

    #[test]
    fn auseam_clip_audit_counts_a_control_off_the_window() {
        let off = fixture(|ui| {
            let rect = Rect::from_min_size(pos2(380.0, 40.0), vec2(80.0, 24.0));
            ui.put(rect, egui::Button::new("Off"));
        });
        assert_eq!(clip_of(&off).outside, 1, "{:?}", clip_of(&off).found);
        let on = fixture(|ui| {
            let rect = Rect::from_min_size(pos2(280.0, 40.0), vec2(80.0, 24.0));
            ui.put(rect, egui::Button::new("On"));
        });
        assert_eq!(clip_of(&on).outside, 0);
    }

    // ---- contrast audit ----

    #[test]
    fn auseam_contrast_audit_multiplies_opacity_and_alpha() {
        let black = [0.0, 0.0, 0.0];
        // White on black is 21:1; half of it is the mid grey it blends to.
        let whole = ratio(effective(plate::SEED_WHITE, 1.0, black), black);
        assert!((whole - 21.0).abs() < 0.01, "{whole}");
        let by_alpha = effective(plate::SEED_WHITE.gamma_multiply(0.5), 1.0, black);
        let by_opacity = effective(plate::SEED_WHITE, 0.5, black);
        assert!((by_alpha[0] - 0.5).abs() < 0.01 && (by_opacity[0] - 0.5).abs() < 0.01);
        let both = effective(plate::SEED_WHITE.gamma_multiply(0.5), 0.5, black);
        assert!((both[0] - 0.25).abs() < 0.01, "alpha times opacity: {both:?}");
        // A grey passing at 4.5 whole fails once faded through either door.
        // At 0.4 it reads 3.66: an opacity factor's fade is held to 3, a
        // colour's own alpha is not.
        let cases: [(&str, f32, f32, usize); 5] = [
            ("whole", 1.0, 1.0, 0),
            ("its own alpha", 0.3, 1.0, 1),
            ("opacity factor", 1.0, 0.3, 1),
            ("opacity factor over 3", 1.0, 0.4, 0),
            ("its own alpha under 4.5", 0.4, 1.0, 1),
        ];
        for (name, alpha, opacity, failures) in cases {
            let got = contrast_fixture(move |ui| {
                ground(ui, plate::SEED_BLACK);
                let font = FontId::proportional(14.0);
                let colour = plate::SEED_WHITE.gamma_multiply(alpha);
                let galley = ui.painter().layout_no_wrap("Words".to_string(), font, colour);
                let text = TextShape::new(pos2(20.0, 20.0), galley, colour).with_opacity_factor(opacity);
                ui.painter().add(text);
            });
            assert_eq!((got.checked, got.failures.len()), (1, failures), "{name}: {got:?}");
        }
    }

    #[test]
    fn auseam_contrast_audit_fails_a_seeded_low_ink() {
        let clean = contrast_fixture(|ui| {
            ground(ui, plate::SEED_BLACK);
            ink(ui, pos2(20.0, 20.0), plate::SEED_WHITE);
        });
        assert_eq!((clean.checked, clean.failures.len(), clean.unknown.len()), (1, 0, 0), "{clean:?}");
        let low = contrast_fixture(|ui| {
            ground(ui, plate::SEED_GREY);
            ink(ui, pos2(20.0, 20.0), plate::SEED_GREY);
        });
        assert_eq!(low.failures.len(), 1, "{low:?}");
        assert!(low.failures[0].contains("Clear words"), "{low:?}");
    }

    #[test]
    fn auseam_contrast_audit_blends_a_glaze_and_holds_ink3_to_three() {
        // White ink on white is 1:1. A black glaze at 0.6 leaves a ground of
        // 0.4 grey, about 5.7:1: the verdict turns on blending it.
        let unglazed = contrast_fixture(|ui| {
            ground(ui, plate::SEED_WHITE);
            ink(ui, pos2(20.0, 20.0), plate::SEED_WHITE);
        });
        assert_eq!((unglazed.checked, unglazed.failures.len()), (1, 1), "{unglazed:?}");
        let glazed = contrast_fixture(|ui| {
            ground(ui, plate::SEED_WHITE);
            ground(ui, plate::SEED_BLACK.gamma_multiply(0.6));
            ink(ui, pos2(20.0, 20.0), plate::SEED_WHITE);
        });
        assert_eq!((glazed.checked, glazed.failures.len()), (1, 0), "{glazed:?}");
        // INK3 on white is about 4.6; on this 0.85 grey about 3.2, over its
        // floor of 3 and under the 4.5 a colour one shade off it is held to.
        let ink3 = plate::INK3;
        let on_grey = contrast_fixture(move |ui| {
            ground(ui, plate::SEED_WHITE);
            ground(ui, plate::SEED_BLACK.gamma_multiply(0.15));
            ink(ui, pos2(20.0, 20.0), ink3);
        });
        assert_eq!((on_grey.checked, on_grey.failures.len()), (1, 0), "{on_grey:?}");
        let plain = contrast_fixture(move |ui| {
            ground(ui, plate::SEED_WHITE);
            ground(ui, plate::SEED_BLACK.gamma_multiply(0.15));
            ink(ui, pos2(20.0, 20.0), ink3.gamma_multiply(0.99));
        });
        assert_eq!(plain.failures.len(), 1, "a colour off INK3 is held to 4.5: {plain:?}");
    }

    #[test]
    fn auseam_contrast_audit_lists_a_mesh_ground_as_unknown() {
        let over_mesh = contrast_fixture(|ui| {
            ground(ui, plate::SEED_BLACK);
            let mut mesh = egui::Mesh::default();
            mesh.add_colored_rect(Rect::from_min_size(Pos2::ZERO, vec2(200.0, 100.0)), plate::SEED_GREY);
            ui.painter().add(Shape::mesh(mesh));
            ink(ui, pos2(20.0, 20.0), plate::SEED_WHITE);
        });
        assert_eq!((over_mesh.checked, over_mesh.unknown.len()), (0, 1), "{over_mesh:?}");
        // Text with no opaque ground at all is unknown too, not passed.
        let bare = contrast_fixture(|ui| ink(ui, pos2(20.0, 20.0), plate::SEED_WHITE));
        assert_eq!((bare.checked, bare.unknown.len()), (0, 1), "{bare:?}");
    }

    // ---- the recorder ----

    fn stroke_path(h: &EditorHarness) -> Vec<Pos2> {
        let c = h.editor().canvas.dbg_rect.center();
        vec![c, c + vec2(30.0, 0.0), c + vec2(60.0, 10.0)]
    }

    #[test]
    fn auseam_recorder_counts_a_clean_journey() {
        let mut h = EditorHarness::new();
        let path = stroke_path(&h);
        let mut r = Recorder::new(&mut h, "clean");
        r.act("draw", Act::Stroke(&path));
        r.act("undo it", Act::Chord(crate::testkit::CTRL, egui::Key::Z));
        r.act("redo it", Act::Perform(Action::Redo));
        r.act("step on", Act::Key(egui::Key::ArrowRight));
        r.act("open a dialog of sorts", Act::Answer(Box::new(|h| h.run())));
        assert_eq!(r.tally(), Tally { clicks: 1, keys: 2, strokes: 1, dialogs: 1 });
        assert_eq!(r.tally().actions(), 5);
    }

    #[test]
    fn auseam_recorder_fails_on_a_refusal() {
        let mut h = EditorHarness::new();
        let path = stroke_path(&h);
        assert_eq!(h.perform(Action::ToggleColumnLocked), crate::Performed::Done);
        let mut r = Recorder::new(&mut h, "bounce");
        let why = r.try_act("the first stroke", Act::Stroke(&path)).unwrap_err();
        assert!(why.contains("bounce") && why.contains("the first stroke"), "{why}");
        assert!(why.contains("refused") && why.contains("locked"), "{why}");
        // A refused stroke still cost the person the action.
        assert_eq!(r.tally().strokes, 1);
    }

    #[test]
    #[should_panic(expected = "journey \"bounce\", step \"paste\": refused")]
    fn auseam_recorder_act_ends_the_journey() {
        let mut h = EditorHarness::new();
        Recorder::new(&mut h, "bounce").act("paste", Act::Perform(Action::PastePixels));
    }

    #[test]
    fn auseam_recorder_fails_on_an_ignored_action() {
        let mut h = EditorHarness::new();
        // Fill armed: a number picks a pot, and this document has none. No
        // refusal is said; the verb just answers Ignored.
        h.perform(Action::FillTool);
        let seq = h.state().refusal_seq;
        let mut r = Recorder::new(&mut h, "colour");
        let why = r.try_act("pick the eighth pot", Act::Perform(Action::Preset8)).unwrap_err();
        assert!(why.contains("colour") && why.contains("pick the eighth pot"), "{why}");
        assert!(why.contains("Preset8") && why.contains("ignored"), "{why}");
        assert_eq!(h.state().refusal_seq, seq, "not a refusal: the Ignored door alone");
    }

    #[test]
    fn auseam_recorder_sees_an_ignored_key_and_a_stroke_that_never_landed() {
        let mut h = EditorHarness::new();
        h.perform(Action::FillTool);
        let path = [pos2(2.0, 2.0), pos2(4.0, 2.0)];
        let mut r = Recorder::new(&mut h, "keys");
        let why = r.try_act("press 8", Act::Key(egui::Key::Num8)).unwrap_err();
        assert!(why.contains("Preset8") && why.contains("ignored"), "{why}");
        let why = r.try_act("miss the canvas", Act::Stroke(&path)).unwrap_err();
        assert!(why.contains("no stroke landed"), "{why}");
    }

    #[test]
    fn auseam_measures_read_the_real_rooms() {
        let mut h = blank_drawing();
        let c = census(&mut h);
        assert!(c.controls > 0 && c.words > 0, "{c:?}");
        let contrast = text_contrast_audit(&mut h);
        assert!(contrast.checked + contrast.unknown.len() > 0, "{contrast:?}");
    }

    // ---- the look (spec section 2) ----

    /// What a style paints with. A `Style` holds a formatter that compares by
    /// pointer, so two built apart are never `==`.
    fn look_of(s: &egui::Style) -> impl PartialEq + std::fmt::Debug + use<> {
        (s.visuals.clone(), s.text_styles.clone(), s.spacing.clone(), s.drag_value_text_style.clone())
    }

    /// A fixture with the app's fonts, which go live on the next pass.
    fn dressed(mut paint: impl FnMut(&mut egui::Ui) + 'static) -> Harness<'static> {
        let mut first = true;
        fixture(move |ui| {
            if std::mem::take(&mut first) {
                plate::install_fonts(ui.ctx());
            } else {
                ui.set_style(plate::style());
                paint(ui);
            }
        })
    }

    #[test]
    fn aulook_theme_is_pinned_in_both_slots() {
        let ctx = egui::Context::default();
        crate::dress(&ctx);
        assert_eq!(ctx.options(|o| o.theme_preference), egui::ThemePreference::Light);
        let (light, dark) = (ctx.style_of(egui::Theme::Light), ctx.style_of(egui::Theme::Dark));
        assert_eq!(light.visuals.panel_fill, plate::ISLAND);
        assert_eq!(look_of(&light), look_of(&dark), "one style in both slots");
        assert_eq!(look_of(&light), look_of(&plate::style()), "and it is the plate's");
    }

    /// The boot pins Light, and kittest's own window says Dark: whatever the
    /// system says, the editor comes up in the one style.
    #[test]
    fn aulook_system_light_gives_the_same_style() {
        let under = |theme| {
            let mut h = blank_drawing();
            h.kit_mut().input_mut().system_theme = Some(theme);
            h.run_frames(3);
            h.ctx().global_style()
        };
        let (light, dark) = (under(egui::Theme::Light), under(egui::Theme::Dark));
        assert_eq!(light.visuals.panel_fill, plate::ISLAND);
        assert_eq!(look_of(&light), look_of(&dark), "the system theme changes nothing");
    }

    /// Light and Dark system themes paint the same pixels (148,629 of them
    /// differed before the pin).
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn aulook_system_theme_paints_the_same_pixels() {
        let shot = |theme| {
            let mut h = EditorHarness::with_gpu(crate::sample::build_sample());
            h.kit_mut().input_mut().system_theme = Some(theme);
            h.kit_mut().set_size(vec2(1600.0, 900.0));
            h.run_frames(30);
            h.kit_mut().render().expect("headless render")
        };
        let (light, dark) = (shot(egui::Theme::Light), shot(egui::Theme::Dark));
        assert_eq!(light.dimensions(), dark.dimensions());
        let diff = light.pixels().zip(dark.pixels()).filter(|(a, b)| a != b).count();
        assert_eq!(diff, 0, "pixels that differ between the system themes");
    }

    /// The table in spec 2.3: ink, the opacity it is painted at, ground, the
    /// ratio the table gives, the floor (0 for a decorative pair).
    #[test]
    fn aulook_contrast_pairs_hold() {
        use plate::{
            BLUE, BLUE_DEEP, BLUE_WASH, DESK, HOVER, INK, INK2, INK3, ISLAND, ON_FILL, PAPER, PRESSED, RED,
            RED_WASH, RULE, STEEL, STEEL_INK, WELL,
        };
        let rows = [
            (INK, 1.0, DESK, 10.87, 4.5),
            (INK2, 1.0, DESK, 5.36, 4.5),
            (INK3, 1.0, DESK, 3.11, 3.0),
            (INK, 1.0, ISLAND, 14.91, 4.5),
            (INK2, 1.0, ISLAND, 7.35, 4.5),
            (INK3, 1.0, ISLAND, 4.27, 3.0),
            (INK, 1.0, WELL, 13.27, 4.5),
            (INK2, 1.0, WELL, 6.54, 4.5),
            (INK3, 1.0, WELL, 3.80, 3.0),
            (INK, 1.0, HOVER, 12.42, 4.5),
            (INK2, 1.0, HOVER, 6.12, 4.5),
            (INK, 1.0, PRESSED, 10.42, 4.5),
            (INK2, 1.0, PRESSED, 5.14, 4.5),
            (INK, 1.0, PAPER, 15.99, 4.5),
            (INK2, 1.0, PAPER, 7.88, 4.5),
            (ON_FILL, 1.0, BLUE, 6.09, 4.5),
            (ON_FILL, 1.0, BLUE_DEEP, 8.27, 4.5),
            (BLUE, 1.0, ISLAND, 5.68, 4.5),
            (BLUE, 1.0, PAPER, 6.09, 3.0),
            (BLUE, 1.0, DESK, 4.14, 3.0),
            (BLUE_DEEP, 1.0, BLUE_WASH, 6.68, 4.5),
            (INK, 1.0, BLUE_WASH, 12.91, 4.5),
            (BLUE_DEEP, 1.0, DESK, 5.62, 3.0),
            (BLUE_DEEP, 1.0, PAPER, 8.27, 3.0),
            // AU21: the lesson ring's stroke where it crosses a pill or the
            // strip's peg bar. Alone it is lost there, so its floor is none:
            // the ISLAND halo round it carries the edge, at the next two rows.
            (BLUE_DEEP, 1.0, BLUE, 1.36, 0.0),
            (BLUE_DEEP, 1.0, STEEL, 1.65, 0.0),
            (ISLAND, 1.0, BLUE, 5.68, 3.0),
            (ISLAND, 1.0, STEEL, 12.72, 3.0),
            (plate::RED, 1.0, ISLAND, 5.67, 4.5),
            (RED, 1.0, DESK, 4.13, 3.0),
            (RED, 1.0, RED_WASH, 5.04, 4.5),
            (ON_FILL, 1.0, RED, 6.08, 4.5),
            (STEEL_INK, 1.0, STEEL, 11.12, 4.5),
            (ON_FILL, 1.0, STEEL, 13.65, 3.0),
            (STEEL, 1.0, DESK, 9.28, 3.0),
            // Non-text: the input's edge, the dashed sheet, the manga outline.
            (INK3, 1.0, ISLAND, 4.27, 3.0),
            (INK3, 1.0, WELL, 3.80, 3.0),
            (INK3, 1.0, PAPER, 4.58, 3.0),
            (INK3, 1.0, DESK, 3.11, 3.0),
            (INK3, 1.0, PAPER, 4.58, 3.0),
            // Disabled content, at `disabled_alpha`.
            (INK, plate::DISABLED_ALPHA, ISLAND, 4.88, 3.0),
            (INK, plate::DISABLED_ALPHA, WELL, 4.65, 3.0),
            (INK, plate::DISABLED_ALPHA, DESK, 4.26, 3.0),
            // Decorative: they carry no information alone.
            (RULE, 1.0, ISLAND, 1.65, 0.0),
            (WELL, 1.0, ISLAND, 1.12, 0.0),
            (ISLAND, 1.0, DESK, 1.37, 0.0),
            (PAPER, 1.0, DESK, 1.47, 0.0),
            (HOVER, 1.0, ISLAND, 1.20, 0.0),
        ];
        assert_eq!(rows.len(), 48);
        for (i, (ink, opacity, ground, table, floor)) in rows.into_iter().enumerate() {
            let under = effective(ground, 1.0, [0.0; 3]);
            let got = ratio(effective(ink, opacity, under), under);
            assert!(got >= floor, "row {i}: {got:.2} under {floor}");
            assert!((got - table).abs() < 0.01, "row {i}: {got:.3}, the table says {table}");
        }
    }

    /// A blank document and the sample, in every room. AU19: the frozen boot
    /// has no kind, so the shipped sample walks with them: its Plan is the real
    /// one, the sheet on.
    fn in_every_room(mut look: impl FnMut(&str, &mut EditorHarness)) {
        let shipped = EditorHarness::with_state_shipped(crate::sample::build_sample());
        let docs = [
            ("blank", EditorHarness::with_state(NewProjectForm::default().new_state())),
            ("sample", EditorHarness::with_state(crate::sample::build_sample())),
            ("shipped", shipped),
        ];
        for (doc, mut h) in docs {
            h.kit_mut().set_size(vec2(1600.0, 900.0));
            for room in PROBE_ROOMS {
                room.enter(h.editor_mut());
                settle(&mut h);
                look(&format!("{doc} {}", room_tag(room)), &mut h);
            }
        }
    }

    /// The recovery dialog in its frame: it hangs off the App, which no
    /// harness holds.
    fn recovery_contrast() -> Contrast {
        use crate::worksafe::{Autosave, Recovery, RunningRecord, recovery_ui};
        let now = 1_000_000;
        let mut offer = Recovery {
            record_path: "running-7-1.json".into(),
            record: RunningRecord { pid: 7, open: Some("Cut 12.animproj".into()), autosave: None, started: 1 },
            autosaves: vec![Autosave { path: "a.animproj".into(), at: now - 30 }],
            selected: 0,
        };
        let h = dressed(move |ui| {
            ground(ui, plate::DESK);
            plate::dialog_frame(ui.style()).show(ui, |ui| recovery_ui(ui, &mut offer, now, 0));
        });
        contrast_of(&h.output().shapes)
    }

    #[test]
    fn aulook_painted_text_contrast_holds_everywhere() {
        let mut failed = Vec::new();
        let (mut checked, mut unread) = (0, 0);
        let mut add = |at: &str, c: Contrast| {
            checked += c.checked;
            unread += c.unknown.len();
            failed.extend(c.failures.into_iter().map(|f| format!("{at}: {f}")));
        };
        in_every_room(|at, h| add(at, text_contrast_audit(h)));
        let mut h = EditorHarness::with_state(crate::sample::build_sample());
        h.kit_mut().set_size(vec2(1600.0, 900.0));
        h.editor_mut().enter_room(Stage::Drawing);
        settle(&mut h);
        for menu in crate::menus::places() {
            h.open_path(menu);
            add(menu, text_contrast_audit(&mut h));
            h.close_menus();
        }
        h.perform(Action::CommandPalette);
        add("the palette", text_contrast_audit(&mut h));
        h.perform(Action::CommandPalette);
        h.perform(Action::ShortcutSheet);
        add("the F1 sheet", text_contrast_audit(&mut h));
        h.perform(Action::ShortcutSheet);
        h.editor_mut().leave = Some(crate::Leave::Open);
        add("the save prompt", text_contrast_audit(&mut h));
        add("recovery", recovery_contrast());
        println!("audit: {checked} sections read, {unread} on no known ground");
        assert!(checked > 2000, "the audit read {checked} sections");
        assert_eq!(unread, 0, "a word the audit cannot read is a word it passed unread");
        assert!(failed.is_empty(), "{} under their floor:\n{}", failed.len(), failed.join("\n"));
    }

    /// A real `Button`, disabled on `fill`, with the look at `alpha`.
    fn disabled_on(fill: Color32, alpha: f32) -> Contrast {
        contrast_fixture(move |ui| {
            let mut style = plate::style();
            style.visuals.disabled_alpha = alpha;
            ui.set_style(style);
            ground(ui, fill);
            ui.add_enabled(false, egui::Button::new("Not now"));
        })
    }

    #[test]
    fn aulook_disabled_button_holds_on_each_fill() {
        for fill in [plate::ISLAND, plate::WELL, plate::DESK] {
            let held = disabled_on(fill, plate::DISABLED_ALPHA);
            assert_eq!(held.checked, 1, "the button's words: {held:?}");
            assert!(held.failures.is_empty(), "{:?}: {held:?}", fill.to_array());
            let stock = disabled_on(fill, 0.5);
            assert_eq!(stock.checked, 1, "the button's words: {stock:?}");
            assert_eq!(stock.failures.len(), 1, "egui's 0.5 on {:?}: {stock:?}", fill.to_array());
        }
    }

    #[test]
    fn aulook_no_translucent_ink_on_text() {
        assert_eq!((plate::legend_dim().a(), plate::legend_muted().a()), (255, 255));
        let mut read = 0;
        in_every_room(|at, h| {
            // AU4: the deck's words stand in the floats, and are read there.
            for float in [None, Some(Float::Brushes), Some(Float::Color), Some(Float::CanvasOptions)] {
                if let Some(float) = float {
                    h.open_float(float);
                }
                for (t, _) in texts_of(&h.kit().output().shapes) {
                    let job = &t.galley.job;
                    for s in &job.sections {
                        let colour = s.format.color;
                        let colour = if colour == plate::SECTION_PLACEHOLDER { t.fallback_color } else { colour };
                        let ink = t.override_text_color.unwrap_or(colour);
                        let words = &job.text[s.byte_range.start.0..s.byte_range.end.0];
                        // The one transparency text takes is a disabled scope's.
                        let dimmed = plate::INK.gamma_multiply(plate::DISABLED_ALPHA);
                        assert!(ink.a() == 255 || ink == dimmed, "{at}: {words:?} in {ink:?}");
                        read += 1;
                    }
                }
            }
            h.editor_mut().floats = Default::default();
        });
        assert!(read > 1500, "read {read} sections");
    }

    #[test]
    fn aulook_type_roles_are_the_table() {
        use egui::{FontFamily, TextStyle};
        use plate::{Role, semibold};
        let regular = FontFamily::Proportional;
        let roles = [
            (Role::Title, 24.0, semibold()),
            (Role::Card, 20.0, semibold()),
            (Role::Heading, 16.0, semibold()),
            (Role::Chrome, 14.0, regular.clone()),
            (Role::Step, 14.0, semibold()),
            (Role::Panel, 14.0, regular.clone()),
            (Role::Small, 12.0, regular),
        ];
        for (role, size, face) in roles {
            assert_eq!(role.font(), FontId::new(size, face), "{role:?}");
        }
        let style = plate::style();
        let of = |s: TextStyle| style.text_styles[&s].clone();
        assert_eq!(of(TextStyle::Body), Role::Panel.font());
        assert_eq!(of(TextStyle::Button), Role::Panel.font());
        assert_eq!(of(TextStyle::Small), Role::Small.font());
        assert_eq!(of(TextStyle::Heading), Role::Heading.font());
        // The old mono call sites read Plex Sans; only a pattern keeps a mono.
        assert_eq!(of(TextStyle::Monospace), Role::Panel.font());
        assert_eq!(style.drag_value_text_style, TextStyle::Body);
        assert_eq!(plate::mono_font(), FontId::new(12.0, plate::mono()));
    }

    #[test]
    fn aulook_legend_is_sentence_case() {
        let h = dressed(|ui| {
            for legend in ["render queue", "VP 1", "horizon Y"] {
                plate::legend(ui, legend);
            }
        });
        let shapes = &h.output().shapes;
        let words: Vec<&str> = texts_of(shapes).iter().map(|(t, _)| t.galley.job.text.as_str()).collect();
        assert_eq!(words, ["Render queue", "VP 1", "Horizon Y"]);
        assert_eq!(plate::sentence(""), "", "an empty legend stays empty");
    }

    #[test]
    fn aulook_kanji_sit_on_the_latin_line() {
        let h = dressed(|ui| {
            ui.label("H原");
        });
        let shapes = &h.output().shapes;
        let texts = texts_of(shapes);
        let (text, _) = texts.iter().find(|(t, _)| t.galley.job.text == "H原").expect("the label");
        let glyphs = &text.galley.rows[0].glyphs;
        let middle = |c: char| {
            let g = glyphs.iter().find(|g| g.chr == c).expect("the glyph");
            g.pos.y + g.uv_rect.offset.y + g.uv_rect.size.y * 0.5
        };
        let apart = (middle('H') - middle('原')).abs();
        assert!(apart < 1.5, "H and 原 sit {apart} pt apart");
    }

    #[test]
    fn aulook_top_row_fits_at_1280() {
        let mut h = EditorHarness::with_state(crate::sample::build_sample());
        h.kit_mut().set_size(vec2(1280.0, 800.0));
        settle(&mut h);
        let right = h.rect(crate::vocab::MENU_MORE).right();
        assert!(right <= 1280.0, "the last menu ends at {right}");
    }

    /// Every row built this pass, painted or not: a row wholly off the window
    /// is never painted, so painted text cannot tell it is gone.
    fn built_rows(h: &EditorHarness) -> Vec<(String, egui::Rect)> {
        use egui_kittest::kittest::{NodeT, Queryable};
        let label = |n: &egui_kittest::Node<'_>| n.accesskit_node().label().unwrap_or_default().to_string();
        let rows = h.kit().query_all_by_role(egui::accesskit::Role::Button);
        rows.map(|n| (label(&n), n.rect())).collect()
    }

    /// Every menu's every row is reached in every window the rooms are measured
    /// at: on the window as it opens, or by the wheel over its body. A row off
    /// the window is a row a person cannot reach. Closed and opened again, a
    /// menu that scrolled starts at its first row.
    #[test]
    fn aulook_menus_stay_on_the_window() {
        const WHEEL: f32 = -120.0;
        let sizes = [(1600.0, 900.0), (1280.0, 800.0), (1280.0, 720.0), (1280.0, 640.0), (1280.0, 520.0), (1280.0, 480.0)];
        for size in sizes.map(|(w, tall)| vec2(w, tall)) {
            let mut h = EditorHarness::with_state(crate::sample::build_sample());
            h.kit_mut().set_size(size);
            h.editor_mut().enter_room(Stage::Drawing);
            settle(&mut h);
            h.editor_mut().set_recent(&[std::path::PathBuf::from("C:/gone/c012.animproj")]);
            let bare = built_rows(&h);
            for menu in crate::menus::places() {
                // A submenu's rows are what its opening adds to its parent's.
                let closed = match menu.rsplit_once('/') {
                    Some((parent, _)) => {
                        h.open_path(parent);
                        settle(&mut h);
                        let rows = built_rows(&h);
                        h.close_menus();
                        rows
                    }
                    None => bare.clone(),
                };
                h.open_path(menu);
                settle(&mut h);
                assert!(egui::Popup::is_any_open(&h.ctx()), "{menu} at {size:?} opens");
                let open_rows = |h: &EditorHarness| -> Vec<(String, egui::Rect)> {
                    built_rows(h).into_iter().filter(|r| !closed.contains(r)).collect()
                };
                let rows = open_rows(&h);
                assert!(!rows.is_empty(), "{menu} at {size:?} builds its rows");
                let first = rows.iter().map(|r| r.1.top()).fold(f32::MAX, f32::min);
                let on = |r: &(String, egui::Rect)| r.1.top() >= 0.0 && r.1.bottom() <= size.y;
                let mut reached: Vec<String> = rows.iter().filter(|r| on(r)).map(|r| r.0.clone()).collect();
                let over = rows.iter().find(|r| on(r)).map(|r| r.1.center());
                let mut scrolled = false;
                if let Some(over) = over.filter(|_| rows.iter().any(|r| !on(r))) {
                    for _ in 0..24 {
                        h.wheel_at(over, WHEEL);
                        reached.extend(open_rows(&h).iter().filter(|r| on(r)).map(|r| r.0.clone()));
                    }
                    scrolled = true;
                }
                let off: Vec<&str> =
                    rows.iter().filter(|r| !reached.contains(&r.0)).map(|r| r.0.as_str()).collect();
                let bottom = rows.iter().map(|r| r.1.bottom()).fold(0.0, f32::max);
                println!("{menu} at {size:?}: {} rows to {bottom}, {} lost: {off:?}", rows.len(), off.len());
                assert!(off.is_empty(), "{menu} at {size:?} loses {off:?}");
                if scrolled {
                    h.close_menus();
                    h.open_path(menu);
                    settle(&mut h);
                    let top = open_rows(&h).iter().map(|r| r.1.top()).fold(f32::MAX, f32::min);
                    assert_eq!(top, first, "{menu} at {size:?} opens at its first row");
                }
                match menu {
                    "More/Panels" => assert!(rows.len() >= 16, "the whole menu was read: {}", rows.len()),
                    "Edit" => assert!(rows.iter().all(|r| r.1.height() == plate::CTRL_H), "{rows:?}"),
                    _ => {}
                }
                h.close_menus();
            }
        }
    }

    #[test]
    fn aulook_render_section_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.render.ffmpeg = "C:/tools/ffmpeg.exe".into();
        config.render.out_dir = "D:/renders".into();
        config.save_to(dir.path()).unwrap();
        let (back, notes) = crate::config::Config::load_from(dir.path());
        assert!(notes.is_empty(), "nothing reset: {notes:?}");
        assert_eq!(back.render, config.render);
    }

    /// What 0.5.0 wrote, built by hand: a golden is never a real config, so
    /// it holds no `api_key` and no `totp_secret`.
    #[test]
    fn aulook_v050_config_golden_loads() {
        use crate::config::{self, Config};
        let dir = tempfile::tempdir().unwrap();
        let golden = serde_json::json!({
            "perf": {
                "vsync": true, "frame_latency": "Throughput", "canvas_filter": "Nearest",
                "fps_cap": 60, "show_fps": true, "undo_limit": 200
            },
            "ui": {
                "scale": 125, "show_welcome": false, "quick_start": [true, true, false, false, false],
                "whats_new_seen": "0.5.0", "lock_positions": true, "exact_descriptions": true
            },
            "session": { "username": "lancer", "host_port": 7878, "last_addr": "", "require_key": false },
            "update_repo": "someone/else",
            "render": { "ffmpeg": "C:/tools/ffmpeg.exe", "out_dir": "D:/renders" }
        });
        assert!(!golden.to_string().contains("api_key") && !golden.to_string().contains("totp_secret"));
        std::fs::write(config::config_path_in(dir.path()), golden.to_string()).unwrap();
        let (got, notes) = Config::load_from(dir.path());
        assert!(notes.is_empty(), "nothing reset: {notes:?}");
        assert_eq!(got.ui.scale, 125);
        assert_eq!(got.ui.quick_start, [true, true, false, false, false]);
        assert_eq!(got.ui.whats_new_seen, "0.5.0");
        assert!(!got.ui.show_welcome && got.ui.lock_positions && got.ui.exact_descriptions);
        assert_eq!((got.session.api_key.as_str(), got.session.totp_secret.as_str()), ("", ""));
        assert_eq!((got.render.ffmpeg.as_str(), got.render.out_dir.as_str()), ("C:/tools/ffmpeg.exe", "D:/renders"));
    }

    // ---- the review of the look: what a person sees of it ----

    /// Every colour a frame lays down outside its words, fills and strokes.
    fn marks_of(shapes: &[ClippedShape]) -> Vec<Color32> {
        fn walk(shape: &Shape, out: &mut Vec<Color32>) {
            match shape {
                Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                Shape::Rect(r) => out.extend([r.fill, r.stroke.color]),
                Shape::Circle(c) => out.extend([c.fill, c.stroke.color]),
                Shape::LineSegment { stroke, .. } => out.push(stroke.color),
                Shape::Path(p) => {
                    out.push(p.fill);
                    if let ColorMode::Solid(colour) = p.stroke.color {
                        out.push(colour);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out.retain(|c| c.a() > 0);
        out
    }

    /// Arming any tool leaves the paper where it was: the options row is one
    /// height whatever it holds (the rows moved it by up to 3 pt).
    #[test]
    fn aulook_arming_a_tool_leaves_the_paper_still() {
        use crate::canvas::CanvasTool;
        for size in [vec2(1600.0, 900.0), vec2(1280.0, 800.0)] {
            let mut h = EditorHarness::new();
            h.kit_mut().set_size(size);
            settle(&mut h);
            let where_it_is = |h: &EditorHarness| {
                let c = &h.editor().canvas;
                (c.dbg_rect, c.view_xform().zoom, c.view_xform().paper_bounds())
            };
            let at_rest = where_it_is(&h);
            for tool in [CanvasTool::Select, CanvasTool::Fill, CanvasTool::LassoFill, CanvasTool::Panel, CanvasTool::Paint] {
                h.editor_mut().canvas.tool = tool;
                settle(&mut h);
                assert_eq!(where_it_is(&h), at_rest, "{tool:?} at {size:?}");
            }
        }
    }

    /// The whole rows the sheet's view holds of the sample, booted as a person
    /// boots: an animator reads the sheet row by row.
    fn sheet_rows(room: Stage, size: egui::Vec2) -> usize {
        let mut h = EditorHarness::with_state_shipped(crate::sample::build_sample());
        h.kit_mut().set_size(size);
        h.editor_mut().sheet_in_draw = true;
        // AU13: Color's sheet stands behind `Result beside`.
        h.editor_mut().result_in_color = true;
        h.editor_mut().enter_room(room);
        settle(&mut h);
        let sheet = &h.state().sheet_ui;
        let row_of = |frame| sheet.cell_center(0, frame).expect("the sheet drew its rows");
        let row_h = row_of(1).y - row_of(0).y;
        let view = plate::viewports(&h.ctx()).into_iter().rev().find(|v| v.rect.contains(row_of(0)));
        println!("{room:?} at {size:?}: {:?}", view.map(|v| v.rect));
        (view.expect("the sheet's scroll view").rect.height() / row_h) as usize
    }

    /// Spec 2.6: row density unchanged. These are the rows each room held on
    /// the commit before the new look (the review found 10, 14, 9 and 7 at
    /// 1600x900). AU3 takes the sheet out of the shipped Draw recipe, so
    /// Draw's rows are its sheet-on recipe's.
    #[test]
    fn aulook_sheet_keeps_its_rows() {
        let rooms = [Stage::Drawing, Stage::Edit, Stage::Layout, Stage::InkPaint];
        for (size, floors) in [(vec2(1600.0, 900.0), [15, 19, 14, 12]), (vec2(1280.0, 720.0), [9, 12, 8, 7])] {
            for (room, floor) in rooms.into_iter().zip(floors) {
                let rows = sheet_rows(room, size);
                assert!(rows >= floor, "{room:?} at {size:?} holds {rows} rows, {floor} before the new look");
            }
        }
    }

    /// A word field and a number field, as `plate` hands them out.
    #[test]
    fn aulook_fields_wear_the_well() {
        let h = dressed(|ui| {
            let mut words = String::from("Cut 12");
            plate::text_field(ui, egui::TextEdit::singleline(&mut words));
        });
        let shapes = &h.output().shapes;
        let rects = rects_of(shapes);
        let edge = rects.iter().find(|r| r.stroke.width > 0.0).expect("the field's edge");
        assert_eq!(edge.stroke, egui::Stroke::new(1.0, plate::INK3));
        assert_eq!(edge.fill, plate::WELL);
        assert_eq!(edge.corner_radius, egui::CornerRadius::same(plate::R_INPUT));
        assert_eq!(edge.rect.height(), plate::CTRL_H, "a word field is a panel control");

        let h = dressed(|ui| {
            let mut value = 12;
            plate::field(ui, egui::DragValue::new(&mut value));
        });
        let shapes = &h.output().shapes;
        let rects = rects_of(shapes);
        let edge = rects.iter().find(|r| r.stroke.width > 0.0).expect("the field's edge");
        assert_eq!(edge.stroke, egui::Stroke::new(1.0, plate::INK3));
        assert_eq!(edge.fill, plate::WELL);
        assert_eq!(edge.corner_radius, egui::CornerRadius::same(plate::R_INPUT));
    }

    /// Stock controls stay edgeless: the field's edge is `field`'s alone.
    #[test]
    fn aulook_a_button_has_no_edge() {
        let h = dressed(|ui| {
            let _ = ui.button("Save");
        });
        let shapes = &h.output().shapes;
        let rects = rects_of(shapes);
        assert!(rects.iter().any(|r| r.fill == plate::WELL), "the button's ground: {rects:?}");
        assert!(rects.iter().all(|r| r.stroke.width == 0.0), "{rects:?}");
    }

    #[test]
    fn aulook_latch_is_a_checkbox() {
        let painted = |on: bool| {
            let h = dressed(move |ui| {
                let mut on = on;
                plate::latch(ui, &mut on, "Snap");
            });
            let shapes = &h.output().shapes;
            let rects = rects_of(shapes);
            let square = rects.iter().find(|r| r.rect.size() == vec2(16.0, 16.0));
            (square.expect("the 16 pt box").clone(), marks_of(shapes))
        };
        let (off, marks) = painted(false);
        assert_eq!(off.corner_radius, egui::CornerRadius::same(4));
        assert_eq!(off.stroke, egui::Stroke::new(1.0, plate::INK3));
        assert_eq!(off.fill, Color32::TRANSPARENT);
        assert!(!marks.contains(&plate::ON_FILL), "no tick at rest");
        let (on, marks) = painted(true);
        assert_eq!(on.corner_radius, egui::CornerRadius::same(4));
        assert_eq!(on.fill, plate::BLUE);
        assert!(marks.contains(&plate::ON_FILL), "the white tick");
        // The box is a control's edge: 3.0 on either ground a latch stands on.
        for ground in [plate::ISLAND, plate::DESK] {
            assert!(plate::contrast(plate::INK3, ground) >= 3.0, "{:?}", ground.to_array());
        }
    }

    /// Spec 2.8: in a disabled scope a painted control has no ground and every
    /// mark and word is `INK` through `disabled_alpha`, never a dimmer ink.
    #[test]
    fn aulook_disabled_controls_ink_with_ink() {
        use crate::icons::Icon;
        let mut h = dressed(|ui| {
            ui.add_enabled_ui(false, |ui| {
                let (mut on, mut off, mut value) = (true, false, 0.5);
                plate::detent(ui, true, "Armed");
                plate::detent(ui, false, "Free");
                plate::detent_dim(ui, false, "Dim");
                plate::latch(ui, &mut on, "Latched");
                plate::latch(ui, &mut off, "Free");
                plate::tool_button(ui, true, Icon::Brush, "Armed", "");
                plate::tool_button(ui, false, Icon::Brush, "Free", "");
                plate::tool_latch(ui, &mut on, Icon::Lock, "Latched", "");
                plate::tool_latch(ui, &mut off, Icon::Lock, "Free", "");
                plate::icon_button(ui, Icon::Eraser, "Action", "");
                plate::swatch(ui, plate::INK, true, "ink");
                plate::rail(ui, &mut value, 0.0..=1.0, 80.0);
                plate::meter(ui, Some(0.5), 80.0);
                plate::danger(ui, "ERASE CEL");
            });
        });
        h.set_size(vec2(400.0, 800.0));
        h.run_steps(2);
        let panel = h.ctx.global_style().visuals.panel_fill;
        let shapes = &h.output().shapes;
        let dimmed = plate::INK.gamma_multiply(plate::DISABLED_ALPHA);
        let marks: Vec<Color32> = marks_of(shapes).into_iter().filter(|c| *c != panel).collect();
        assert!(marks.len() >= 16, "read {} marks", marks.len());
        for mark in marks {
            assert_eq!(mark, dimmed, "{:?}", mark.to_array());
        }
        // A scope dims painted words in their glyphs, not in the job.
        let texts = texts_of(shapes);
        assert!(texts.len() >= 10, "read {} words", texts.len());
        for (t, _) in texts {
            let glyphs = t.galley.rows.iter().flat_map(|row| &row.row.visuals.mesh.vertices);
            assert!(glyphs.clone().count() > 0, "{:?} has glyphs", t.galley.job.text);
            for glyph in glyphs {
                assert_eq!(glyph.color, dimmed, "{:?}", t.galley.job.text);
            }
        }
    }

    /// A disabled field keeps its well (spec 2.3, "disabled content in a
    /// field"), and its edge is `INK` through the scope.
    #[test]
    fn aulook_a_disabled_field_keeps_its_well() {
        let h = dressed(|ui| {
            ui.add_enabled_ui(false, |ui| {
                let mut value = 12;
                plate::field(ui, egui::DragValue::new(&mut value));
            });
        });
        let shapes = &h.output().shapes;
        let rects = rects_of(shapes);
        let edge = rects.iter().find(|r| r.stroke.width > 0.0).expect("the field's edge");
        assert_eq!(edge.stroke.color, plate::INK.gamma_multiply(plate::DISABLED_ALPHA));
        assert_eq!(edge.fill, plate::WELL.gamma_multiply(plate::DISABLED_ALPHA));
    }

    /// What `paint` returns, given focus, and the frame `plate::FocusRing`
    /// leaves.
    fn focused(mut paint: impl FnMut(&mut egui::Ui) -> egui::Response + 'static) -> (Rect, Vec<RectShape>) {
        let seen = std::rc::Rc::new(std::cell::Cell::new(Rect::NOTHING));
        let held = seen.clone();
        let mut h = dressed(move |ui| {
            ui.ctx().add_plugin(plate::FocusRing);
            let r = paint(ui);
            r.request_focus();
            held.set(r.interact_rect);
        });
        h.run_steps(2);
        (seen.get(), rects_of(&h.output().shapes))
    }

    /// Spec 2.8: a 2 pt blue ring, 2 pt off, on what took focus by the Tab
    /// ring, whoever painted it; and none on a frame with nothing focused.
    #[test]
    fn aulook_focus_wears_the_ring() {
        let held = |(at, rects): (Rect, Vec<RectShape>)| {
            let ring = rects.iter().find(|r| r.stroke == egui::Stroke::new(2.0, plate::BLUE));
            let ring = ring.unwrap_or_else(|| panic!("no ring in {rects:?}"));
            assert_eq!(ring.rect, at.expand(2.0));
            assert_eq!(ring.stroke_kind, egui::StrokeKind::Outside);
            assert_eq!(ring.fill, Color32::TRANSPARENT);
        };
        held(focused(|ui| ui.button("Save")));
        held(focused(|ui| {
            let mut on = false;
            plate::latch(ui, &mut on, "Snap")
        }));
        held(focused(|ui| plate::tool_button(ui, false, crate::icons::Icon::Brush, "Brush", "")));
        held(focused(|ui| plate::swatch(ui, plate::RED, false, "red")));
        held(focused(|ui| {
            let mut words = String::from("Cut 12");
            plate::text_field(ui, egui::TextEdit::singleline(&mut words))
        }));
        let h = dressed(|ui| {
            ui.ctx().add_plugin(plate::FocusRing);
            let _ = ui.button("Save");
        });
        let rects = rects_of(&h.output().shapes);
        assert!(rects.iter().all(|r| r.stroke.width != 2.0), "nothing has focus: {rects:?}");
    }

    /// The boot installs the ring.
    #[test]
    fn aulook_the_boot_wears_the_ring() {
        let mut h = blank_drawing();
        let rings = |h: &EditorHarness| {
            let rects = rects_of(&h.kit().output().shapes);
            let round = egui::CornerRadius::same(plate::R_CONTROL + 4);
            rects.iter().filter(|r| r.stroke == egui::Stroke::new(2.0, plate::BLUE) && r.corner_radius == round).count()
        };
        assert_eq!(rings(&h), 0, "nothing has focus");
        h.perform(Action::CommandPalette);
        settle(&mut h);
        assert_eq!(rings(&h), 1, "the palette's field took focus");
    }

    #[test]
    fn aulook_spacing_radii_and_shadows_are_the_table() {
        use egui::epaint::Shadow;
        let style = plate::style();
        let s = &style.spacing;
        assert_eq!(s.button_padding, vec2(10.0, 6.0));
        assert_eq!(s.item_spacing, vec2(6.0, 4.0));
        assert_eq!(s.window_margin, egui::Margin::same(12));
        assert_eq!(s.menu_margin, egui::Margin::same(6));
        assert_eq!(s.indent, 16.0);
        assert_eq!(s.interact_size.y, 28.0);
        let v = &style.visuals;
        let radius = egui::CornerRadius::same;
        assert_eq!(v.window_corner_radius, radius(12), "floats");
        assert_eq!(v.menu_corner_radius, radius(10), "menus");
        let w = &v.widgets;
        for state in [&w.noninteractive, &w.inactive, &w.hovered, &w.active, &w.open] {
            assert_eq!(state.corner_radius, radius(8), "controls");
        }
        assert_eq!(plate::dialog_frame(&style).corner_radius, radius(14), "dialogs");
        assert_eq!(
            [plate::R_DIALOG, plate::R_FLOAT, plate::R_MENU, plate::R_CONTROL, plate::R_INPUT, plate::R_SHEET],
            [14, 12, 10, 8, 6, 4]
        );
        let shadow = |offset, blur, a| Shadow { offset, blur, spread: 0, color: plate::alpha(plate::SHADOW, a) };
        assert_eq!(v.window_shadow, shadow([0, 6], 24, 41), "floats and the rail");
        assert_eq!(v.popup_shadow, shadow([0, 4], 16, 46), "menus");
        assert_eq!(plate::dialog_frame(&style).shadow, shadow([0, 12], 40, 71), "dialogs");
        // The dock's pane margin stays what it was, or the sheet pays for it.
        assert_eq!(plate::dock_style(&style).tab.tab_body.inner_margin, egui::Margin::same(6));
    }

    /// Spec 2.6: chips carry no stroke, on or off, and stand in a sheet's
    /// corners: a stroke a chip costs vertices the performance gate counts.
    #[test]
    fn aulook_small_controls_carry_no_strokes() {
        let mut h = EditorHarness::with_state(crate::sample::build_sample());
        h.kit_mut().set_size(vec2(1600.0, 900.0));
        h.editor_mut().enter_room(Stage::Drawing);
        settle(&mut h);
        let inside = |h: &EditorHarness, chip: Rect| -> Vec<RectShape> {
            let rects = rects_of(&h.kit().output().shapes);
            rects.into_iter().filter(|r| chip.contains_rect(r.rect) && r.rect.area() > 0.0).collect()
        };
        for name in ["Hide track A", "Lock track A"] {
            let chip = h.rect(name);
            let faces = inside(&h, chip);
            assert!(!faces.is_empty(), "{name} paints a face");
            for face in faces {
                assert_eq!(face.stroke.width, 0.0, "{name}: {face:?}");
                assert_eq!(face.fill, plate::WELL, "{name} at rest");
                assert_eq!(face.corner_radius, egui::CornerRadius::same(plate::R_SHEET));
            }
        }
        h.click("Hide track A");
        settle(&mut h);
        let chip = h.rect("Hide track A");
        let lit = inside(&h, chip);
        assert!(lit.iter().any(|r| r.fill == plate::RED_WASH), "lit takes the wash: {lit:?}");
        assert!(lit.iter().all(|r| r.stroke.width == 0.0), "and still no stroke: {lit:?}");
    }

    /// Spec 2.5: a DANGER's words are sentence case on screen, and its stored
    /// name, what a test and a screen reader find it by, stays.
    #[test]
    fn aulook_danger_words_are_sentence_case() {
        use egui_kittest::kittest::Queryable;
        let names = [
            "ERASE CEL", "DELETE LAYER", "REMOVE DRAWING", "REMOVE TRACK", "REMOVE NODE", "DELETE BRUSH",
            "DELETE ROOM", "RESET SHORTCUTS", "REMOVE BANK", "OVERWRITE", "LIFT KEY", "REMOVE CHARACTER",
            "Don't Save",
        ];
        for name in names {
            let h = dressed(move |ui| {
                plate::danger(ui, name);
            });
            let shapes = &h.output().shapes;
            let words: Vec<String> = texts_of(shapes).iter().map(|(t, _)| t.galley.job.text.clone()).collect();
            let want = plate::sentence(&name.to_lowercase());
            assert_eq!(words, [want.clone()]);
            assert!(!want[1..].contains(char::is_uppercase), "{want}");
            assert!(h.query_by_label(name).is_some(), "{name} still answers to its name");
        }
    }

    /// A character's dot stands out of the ground it sits on: a colour too
    /// close to that ground takes a ring.
    #[test]
    fn aulook_identity_dot_keeps_an_edge() {
        let dot = |colour: Color32| {
            let h = dressed(move |ui| {
                plate::identity_dot(ui.painter(), pos2(40.0, 40.0), 6.0, colour);
            });
            marks_of(&h.output().shapes).contains(&plate::INK3)
        };
        assert!(dot(plate::ISLAND), "the island's own colour needs its ring");
        assert!(dot(plate::WELL), "as does the well's");
        assert!(!dot(plate::RED), "a colour that stands out needs none");
    }
}
