//! GM11: one line of horizontal text, rasterised on the CPU — the delivery
//! burn-ins. GM16 extends it into lettering: blocks of lines or of vertical
//! columns ([`layout`], [`raster_block`]).
//!
//! The faces are the app's own Plex cuts, embedded ONCE (D16): the app's
//! font stack (`plate::install_fonts`) takes these same statics. Layout is
//! ab_glyph's advances plus pair kerning; coverage lands in a premultiplied
//! [`Image`] like every other render here.
//!
//! **Vertical setting (tategaki, GM16).** Columns run right to left, each
//! top to bottom, one em cell per character (the cell's baseline sits
//! [`EM_ASCENT`] down it: the ideographic em box). ab_glyph does not apply
//! the fonts' `vert` substitutions, so the vertical forms are made here:
//! brackets, the long-vowel bar, dashes, the wave dash and the ellipsis
//! ([`ROTATED`]) turn 90° clockwise about the cell's centre (「 becomes ﹁);
//! 、 and 。 move half an em right and up, into the cell's top-right
//! quarter, and the small kana a tenth; a run of one or two ASCII digits is
//! set upright across one cell (tate-chu-yoko), condensed to fit. A wrap or
//! a newline starts the next column to the left, and the marks that may
//! not start a line ([`NO_LINE_START`]) hang at the end of the one before
//! (burasage). Horizontal blocks wrap the same way (at the last space when
//! the line has one).
//!
//! **Outlines** are the coverage dilated by a disc of the outline's width
//! (soft-edged), composited under the fill.

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};

use crate::fx::Image;

/// IBM Plex Sans Regular (the app's proportional face).
pub static PLEX_SANS: &[u8] = include_bytes!("../../../app/assets/fonts/IBMPlexSans-Regular.ttf");
/// IBM Plex Mono Medium (the app's monospace face; the burn-ins').
pub static PLEX_MONO: &[u8] = include_bytes!("../../../app/assets/fonts/IBMPlexMono-Medium.ttf");
/// GM16: IBM Plex Sans JP Bold, the full face (kana, the JIS kanji, Latin):
/// lettering's default and the app's JP fallback.
pub static PLEX_SANS_JP: &[u8] = include_bytes!("../../../app/assets/fonts/IBMPlexSansJP-Bold.otf");

/// Which embedded face a line is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Face {
    Sans,
    #[default]
    Mono,
    /// GM16: Plex Sans JP Bold.
    SansJp,
}

impl Face {
    fn font(self) -> FontRef<'static> {
        let bytes = match self {
            Face::Sans => PLEX_SANS,
            Face::Mono => PLEX_MONO,
            Face::SansJp => PLEX_SANS_JP,
        };
        FontRef::try_from_slice(bytes).expect("the embedded Plex face parses")
    }

    /// GM16: does this face draw `ch`?
    pub fn covers(self, ch: char) -> bool {
        self.font().glyph_id(ch).0 != 0
    }
}

/// GM16: the characters of `chars` that no font of `stack` (TrueType or
/// OpenType bytes) has a glyph for, in order, once each — the UI's
/// glyph-coverage test reads the app's font stack through it. A font that
/// does not parse covers nothing.
pub fn uncovered(stack: &[&[u8]], chars: impl IntoIterator<Item = char>) -> Vec<char> {
    let fonts: Vec<FontRef<'_>> = stack
        .iter()
        .filter_map(|b| FontRef::try_from_slice(b).ok())
        .collect();
    let mut out = Vec::new();
    for ch in chars {
        if !out.contains(&ch) && !fonts.iter().any(|f| f.glyph_id(ch).0 != 0) {
            out.push(ch);
        }
    }
    out
}

/// One glyph on the line: its id and its pen x from the line's left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub glyph: GlyphId,
    pub x: f32,
}

/// A laid-out line: glyphs left to right, the advance width, and the
/// line's ascent and height (ascent − descent), all in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub glyphs: Vec<Placed>,
    pub width: f32,
    pub ascent: f32,
    pub height: f32,
}

/// Lay `text` out on one line at `px` pixels (the font's em height).
/// Control characters (a newline included) take no room: one line only.
pub fn layout_line(face: Face, text: &str, px: f32) -> Line {
    let font = face.font();
    let scaled = font.as_scaled(PxScale::from(px.max(1.0)));
    let mut glyphs = Vec::new();
    let mut x = 0.0f32;
    let mut prev: Option<GlyphId> = None;
    for ch in text.chars().filter(|c| !c.is_control()) {
        let id = scaled.glyph_id(ch);
        if let Some(p) = prev {
            x += scaled.kern(p, id);
        }
        glyphs.push(Placed { glyph: id, x });
        x += scaled.h_advance(id);
        prev = Some(id);
    }
    Line {
        glyphs,
        width: x,
        ascent: scaled.ascent(),
        height: scaled.ascent() - scaled.descent(),
    }
}

/// `text` as a premultiplied image just big enough for the line (its
/// advance width × its height, at least 1×1), inked in `color` — a straight
/// RGBA chip whose bytes are the stored values (`c/255`; white and black
/// are the same under either colour law). Transparent where no glyph is.
pub fn raster_line(face: Face, text: &str, px: f32, color: [u8; 4]) -> Image {
    let line = layout_line(face, text, px);
    let (w, h) = (
        line.width.ceil().max(1.0) as u32,
        line.height.ceil().max(1.0) as u32,
    );
    let mut img = Image::new(w, h);
    let font = face.font();
    let scale = PxScale::from(px.max(1.0));
    let c = color.map(|v| v as f32 / 255.0);
    for g in &line.glyphs {
        let glyph = g
            .glyph
            .with_scale_and_position(scale, point(g.x, line.ascent));
        let Some(outline) = font.outline_glyph(glyph) else {
            continue; // a space
        };
        let b = outline.px_bounds();
        outline.draw(|gx, gy, cov| {
            let x = b.min.x as i64 + gx as i64;
            let y = b.min.y as i64 + gy as i64;
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                return;
            }
            let a = cov.clamp(0.0, 1.0) * c[3];
            let i = (y as usize * w as usize + x as usize) * 4;
            // Glyphs of one line may touch (kerned pairs): src-over.
            let keep = 1.0 - a;
            for k in 0..3 {
                img.px[i + k] = c[k] * a + img.px[i + k] * keep;
            }
            img.px[i + 3] = a + img.px[i + 3] * keep;
        });
    }
    img
}

// ---- GM16: lettering blocks ----------------------------------------------------------

/// Where the baseline sits down a vertical cell, in ems: the ideographic em
/// box runs from 0.88 above the baseline to 0.12 below it.
pub const EM_ASCENT: f32 = 0.88;

/// What a vertical column turns 90° clockwise: its vertical forms.
pub const ROTATED: &str = "「」『』（）［］｛｝〈〉《》【】〔〕()[]{}<>ー－―‐–—〜～…‥=＝";

/// Marks that never start a line or a column: a wrap leaves them hanging at
/// the end of the one before (burasage).
pub const NO_LINE_START: &str =
    "、。，．,.」』）］｝〉》】〕)]}ー〜～…‥!?！？ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ";

/// The small kana: in a column they sit a tenth of an em up and right.
const SMALL_KANA: &str = "ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ";

/// The comma and full stop and their full-width kin: half an em up and
/// right in a column, into the cell's top-right quarter.
const CORNER_MARKS: &str = "、。，．";

/// GM16: which way a block's lines run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Horizontal,
    /// Tategaki: columns right to left, characters top to bottom.
    Vertical,
}

/// GM16: how a block of lettering is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The face asked for; a character it lacks falls back to Plex Sans JP,
    /// then Plex Sans.
    pub face: Face,
    /// The em size in px.
    pub px: f32,
    pub direction: Direction,
    /// Line (or column) pitch as a multiple of the size.
    pub leading: f32,
    /// Extra px between characters along a line (or down a column).
    pub tracking: f32,
    /// The longest a line (or a column) runs before it wraps, in px; 0 never
    /// wraps.
    pub wrap: f32,
}

impl Style {
    pub fn new(face: Face, px: f32, direction: Direction) -> Self {
        Self {
            face,
            px,
            direction,
            leading: 1.25,
            tracking: 0.0,
            wrap: 0.0,
        }
    }
}

/// GM16: how one cell's glyph is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// As in horizontal text (every horizontal glyph is upright).
    Upright,
    /// Turned 90° clockwise about the cell's centre ([`ROTATED`]).
    Rotated,
    /// 、 。: moved half an em right and up.
    Corner,
    /// A small kana: moved a tenth of an em right and up.
    Small,
    /// Tate-chu-yoko: one or two ASCII digits upright across one cell.
    Tcy,
}

/// GM16: one character's cell (two digits' for tate-chu-yoko).
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub text: String,
    /// The cell as (x, y, w, h) in the block's px, origin top-left.
    pub rect: [f32; 4],
    pub form: Form,
    /// The glyphs' pixel bounds as (x0, y0, x1, y1) in the block's px; None
    /// for a space.
    pub ink: Option<[f32; 4]>,
}

impl Cell {
    pub fn centre(&self) -> (f32, f32) {
        (self.rect[0] + self.rect[2] / 2.0, self.rect[1] + self.rect[3] / 2.0)
    }
}

/// One glyph placed in a block.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Piece {
    face: Face,
    glyph: GlyphId,
    scale: PxScale,
    /// The glyph's origin (pen x, baseline y) in the block's px.
    at: (f32, f32),
    /// A turned form: the centre it turns 90° clockwise about.
    turn: Option<(f32, f32)>,
}

/// GM16: a laid-out block of lettering, `w × h` px.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub cells: Vec<Cell>,
    pub w: f32,
    pub h: f32,
    pieces: Vec<Piece>,
}

/// The face that draws `ch`: the one asked for, else Plex Sans JP, else
/// Plex Sans (else the one asked for, drawing its missing glyph).
fn face_for(face: Face, ch: char) -> Face {
    [face, Face::SansJp, Face::Sans]
        .into_iter()
        .find(|f| f.covers(ch))
        .unwrap_or(face)
}

/// One unit of a line: a character, or (vertical) a tate-chu-yoko run.
#[derive(Debug, Clone)]
struct Unit {
    text: String,
    /// Its length along the line, tracking included.
    size: f32,
}

/// Greedy wrap of one paragraph at `limit` (0 = never): a unit that would
/// run past it starts the next line, except a [`NO_LINE_START`] mark, which
/// hangs; a Latin word carries over whole from its line's last space, and
/// the space a line breaks at is dropped.
fn wrap_units(units: Vec<Unit>, limit: f32) -> Vec<Vec<Unit>> {
    if limit <= 0.0 {
        return vec![units];
    }
    let mut lines: Vec<Vec<Unit>> = vec![Vec::new()];
    let first = |u: &Unit| u.text.chars().next().unwrap_or(' ');
    for u in units {
        let line = lines.last_mut().expect("a line");
        let run: f32 = line.iter().map(|x| x.size).sum();
        let c = first(&u);
        if line.is_empty() || run + u.size <= limit + 1e-3 || NO_LINE_START.contains(c) {
            line.push(u);
            continue;
        }
        if c == ' ' {
            // The break itself: the space goes.
            lines.push(Vec::new());
            continue;
        }
        let mut carry = Vec::new();
        if c.is_ascii_graphic()
            && let Some(sp) = line.iter().rposition(|x| first(x) == ' ')
            && line[sp + 1..].iter().all(|x| first(x).is_ascii_graphic())
        {
            carry = line.split_off(sp + 1);
            line.pop();
        }
        carry.push(u);
        lines.push(carry);
    }
    lines
}

/// GM16: lay `text` out as a block (a newline starts the next line or
/// column).
pub fn layout(text: &str, style: &Style) -> Block {
    match style.direction {
        Direction::Horizontal => layout_horizontal(text, style),
        Direction::Vertical => layout_vertical(text, style),
    }
}

/// The pixel bounds of a placed glyph (turned when it turns).
fn piece_ink(p: &Piece) -> Option<[f32; 4]> {
    let glyph = p.glyph.with_scale_and_position(p.scale, point(p.at.0, p.at.1));
    let b = p.face.font().outline_glyph(glyph)?.px_bounds();
    let r = [b.min.x, b.min.y, b.max.x, b.max.y];
    Some(match p.turn {
        None => r,
        Some((cx, cy)) => [cx - (r[3] - cy), cy + (r[0] - cx), cx - (r[1] - cy), cy + (r[2] - cx)],
    })
}

fn union(a: Option<[f32; 4]>, b: Option<[f32; 4]>) -> Option<[f32; 4]> {
    match (a, b) {
        (Some(a), Some(b)) => Some([a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]),
        (a, None) => a,
        (None, b) => b,
    }
}

fn layout_horizontal(text: &str, st: &Style) -> Block {
    let px = st.px.max(1.0);
    let scale = PxScale::from(px);
    let primary = st.face.font();
    let ps = primary.as_scaled(scale);
    let (ascent, height) = (ps.ascent(), ps.ascent() - ps.descent());
    let pitch = px * st.leading.max(0.1);
    let advance = |ch: char| {
        let font = face_for(st.face, ch).font();
        let f = font.as_scaled(scale);
        f.h_advance(f.glyph_id(ch))
    };
    let mut lines: Vec<Vec<Unit>> = Vec::new();
    for para in text.split('\n') {
        let units = para
            .chars()
            .filter(|c| !c.is_control())
            .map(|c| Unit {
                text: c.to_string(),
                size: advance(c) + st.tracking,
            })
            .collect();
        lines.extend(wrap_units(units, st.wrap));
    }
    let mut cells = Vec::new();
    let mut pieces = Vec::new();
    let mut w = 0.0f32;
    for (row, line) in lines.iter().enumerate() {
        let top = row as f32 * pitch;
        let mut x = 0.0f32;
        let mut prev: Option<(Face, GlyphId)> = None;
        for u in line {
            let ch = u.text.chars().next().unwrap_or(' ');
            let face = face_for(st.face, ch);
            let font = face.font();
            let f = font.as_scaled(scale);
            let id = f.glyph_id(ch);
            if let Some((pf, pid)) = prev
                && pf == face
            {
                x += f.kern(pid, id);
            }
            let piece = Piece {
                face,
                glyph: id,
                scale,
                at: (x, top + ascent),
                turn: None,
            };
            let adv = f.h_advance(id);
            cells.push(Cell {
                text: u.text.clone(),
                rect: [x, top, adv, height],
                form: Form::Upright,
                ink: piece_ink(&piece),
            });
            pieces.push(piece);
            x += adv + st.tracking;
            prev = Some((face, id));
        }
        let line_w = if line.is_empty() { 0.0 } else { x - st.tracking };
        w = w.max(line_w);
    }
    let h = (lines.len().max(1) - 1) as f32 * pitch + height;
    Block { cells, w, h, pieces }
}

/// The units of one vertical paragraph: a run of one or two ASCII digits
/// is one tate-chu-yoko unit; every other character is its own (a longer
/// number sets one upright digit per cell).
fn vertical_units(para: &str) -> Vec<String> {
    let chars: Vec<char> = para.chars().filter(|c| !c.is_control()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let run = chars[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if (1..=2).contains(&run) {
            out.push(chars[i..i + run].iter().collect());
            i += run;
        } else if run > 2 {
            out.extend(chars[i..i + run].iter().map(|c| c.to_string()));
            i += run;
        } else {
            out.push(chars[i].to_string());
            i += 1;
        }
    }
    out
}

fn layout_vertical(text: &str, st: &Style) -> Block {
    let em = st.px.max(1.0);
    let scale = PxScale::from(em);
    let pitch = em * st.leading.max(0.1);
    // How far down the column a unit runs.
    let along = |u: &str| -> f32 {
        let mut cs = u.chars();
        let ch = cs.next().unwrap_or(' ');
        if cs.next().is_some() || ch.is_ascii_digit() {
            return em;
        }
        match ch {
            ' ' => em * 0.5,
            c if ROTATED.contains(c) => {
                let font = face_for(st.face, c).font();
                let f = font.as_scaled(scale);
                f.h_advance(f.glyph_id(c)).max(1.0)
            }
            _ => em,
        }
    };
    let mut cols: Vec<Vec<Unit>> = Vec::new();
    for para in text.split('\n') {
        let units = vertical_units(para)
            .into_iter()
            .map(|t| {
                let size = along(&t) + st.tracking;
                Unit { text: t, size }
            })
            .collect();
        cols.extend(wrap_units(units, st.wrap));
    }
    let n = cols.len().max(1);
    let w = (n - 1) as f32 * pitch + em;
    let mut cells = Vec::new();
    let mut pieces = Vec::new();
    let mut h = 0.0f32;
    for (k, col) in cols.iter().enumerate() {
        // Right to left: the first column is the rightmost.
        let x = w - em - k as f32 * pitch;
        let mut y = 0.0f32;
        for u in col {
            let size = u.size - st.tracking;
            let (cx, cy) = (x + em / 2.0, y + size / 2.0);
            let baseline = y + EM_ASCENT * em;
            let chars: Vec<char> = u.text.chars().collect();
            let ch = chars[0];
            let face = face_for(st.face, ch);
            let font = face.font();
            let f = font.as_scaled(scale);
            let mut placed: Vec<Piece> = Vec::new();
            let form = if chars.len() > 1 || ch.is_ascii_digit() {
                // Tate-chu-yoko: the run set across the cell, condensed to
                // fit 0.95 em.
                let ids: Vec<GlyphId> = chars.iter().map(|c| f.glyph_id(*c)).collect();
                let mut starts = Vec::with_capacity(ids.len());
                let mut run = 0.0f32;
                for (j, id) in ids.iter().enumerate() {
                    if j > 0 {
                        run += f.kern(ids[j - 1], *id);
                    }
                    starts.push(run);
                    run += f.h_advance(*id);
                }
                let k = if run > 0.95 * em { 0.95 * em / run } else { 1.0 };
                let condensed = PxScale { x: em * k, y: em };
                let left = cx - run * k / 2.0;
                for (id, s) in ids.iter().zip(&starts) {
                    placed.push(Piece {
                        face,
                        glyph: *id,
                        scale: condensed,
                        at: (left + s * k, baseline),
                        turn: None,
                    });
                }
                Form::Tcy
            } else {
                let id = f.glyph_id(ch);
                let adv = f.h_advance(id);
                let left = x + (em - adv) / 2.0;
                let (form, at, turn) = if ROTATED.contains(ch) {
                    // Set as in a line, its em box centred on the cell's
                    // centre, then turned.
                    let at = (cx - adv / 2.0, cy - em / 2.0 + EM_ASCENT * em);
                    (Form::Rotated, at, Some((cx, cy)))
                } else if CORNER_MARKS.contains(ch) {
                    (Form::Corner, (left + em * 0.5, baseline - em * 0.5), None)
                } else if SMALL_KANA.contains(ch) {
                    (Form::Small, (left + em * 0.1, baseline - em * 0.1), None)
                } else {
                    (Form::Upright, (left, baseline), None)
                };
                placed.push(Piece {
                    face,
                    glyph: id,
                    scale,
                    at,
                    turn,
                });
                form
            };
            let ink = placed.iter().map(piece_ink).fold(None, union);
            cells.push(Cell {
                text: u.text.clone(),
                rect: [x, y, em, size],
                form,
                ink,
            });
            pieces.extend(placed);
            y += u.size;
        }
        if !col.is_empty() {
            h = h.max(y - st.tracking);
        }
    }
    Block { cells, w, h, pieces }
}

/// GM16: the block's coverage (0–1, `w × h`, row-major) with `pad` px of
/// clear margin on every side. Overlapping glyphs union.
pub fn block_coverage(block: &Block, pad: u32) -> (u32, u32, Vec<f32>) {
    let w = block.w.ceil().max(1.0) as u32 + 2 * pad;
    let h = block.h.ceil().max(1.0) as u32 + 2 * pad;
    let mut cov = vec![0.0f32; w as usize * h as usize];
    let off = pad as f32;
    for p in &block.pieces {
        let font = p.face.font();
        let glyph = p
            .glyph
            .with_scale_and_position(p.scale, point(p.at.0 + off, p.at.1 + off));
        let Some(outline) = font.outline_glyph(glyph) else {
            continue; // a space
        };
        let b = outline.px_bounds();
        let turn = p.turn.map(|(cx, cy)| (cx + off, cy + off));
        outline.draw(|gx, gy, c| {
            let (x, y) = (b.min.x + gx as f32, b.min.y + gy as f32);
            // A turned form: the pixel's centre turned 90° clockwise about
            // the cell's centre (exact when the centre sits on the grid).
            let (x, y) = match turn {
                None => (x, y),
                Some((cx, cy)) => (cx - (y + 0.5 - cy) - 0.5, cy + (x + 0.5 - cx) - 0.5),
            };
            let (ix, iy) = (x.round() as i64, y.round() as i64);
            if ix < 0 || iy < 0 || ix >= w as i64 || iy >= h as i64 {
                return;
            }
            let i = iy as usize * w as usize + ix as usize;
            let c = c.clamp(0.0, 1.0);
            cov[i] = cov[i] + c - cov[i] * c;
        });
    }
    (w, h, cov)
}

/// Coverage dilated by a disc of radius `r` px, soft-edged: each pixel takes
/// the most coverage within reach, weighted by `clamp(r + 0.5 − d, 0, 1)`.
fn dilate(cov: &[f32], w: u32, h: u32, r: f32) -> Vec<f32> {
    use rayon::prelude::*;
    let reach = r.ceil() as i64 + 1;
    let offsets: Vec<(i64, i64, f32)> = (-reach..=reach)
        .flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)))
        .filter_map(|(dx, dy)| {
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            let k = (r + 0.5 - d).clamp(0.0, 1.0);
            (k > 0.0).then_some((dx, dy, k))
        })
        .collect();
    let (wi, hi) = (w as i64, h as i64);
    let mut out = vec![0.0f32; cov.len()];
    out.par_chunks_mut(w as usize).enumerate().for_each(|(y, row)| {
        let y = y as i64;
        for (x, o) in row.iter_mut().enumerate() {
            let x = x as i64;
            let mut m = 0.0f32;
            for &(dx, dy, k) in &offsets {
                let (sx, sy) = (x + dx, y + dy);
                if sx >= 0 && sy >= 0 && sx < wi && sy < hi {
                    m = m.max(cov[(sy * wi + sx) as usize] * k);
                }
            }
            *o = m;
        }
    });
    out
}

/// GM16: a block rasterised: `fill` (premultiplied) over an outline of
/// `outline` px in `outline_px` (premultiplied; 0 px = none). Returns the
/// image and the clear margin it was given on every side (room for the
/// outline): the block's own origin sits at (pad, pad).
pub fn raster_block(block: &Block, fill: [f32; 4], outline: f32, outline_px: [f32; 4]) -> (Image, u32) {
    let ring = outline > 0.0;
    let pad = if ring { outline.ceil() as u32 + 1 } else { 0 };
    let (w, h, cov) = block_coverage(block, pad);
    let halo = if ring { dilate(&cov, w, h, outline) } else { Vec::new() };
    let mut img = Image::new(w, h);
    for (i, c) in cov.iter().enumerate() {
        let o = if ring { halo[i] } else { 0.0 };
        let keep = 1.0 - fill[3] * c;
        for k in 0..4 {
            img.px[i * 4 + k] = fill[k] * c + outline_px[k] * o * keep;
        }
    }
    (img, pad)
}
