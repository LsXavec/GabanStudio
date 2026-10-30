//! GM7 (D8): the colour law, and the ink-and-paint verbs built on it —
//! recolour by chip, refill, replace-all and the paint check. Headless.
//!
//! A CHIP is the 8-bit sRGB colour an artist picks (a palette pot, the
//! brush colour, a Solid node). A STORED value is what a tile texel holds
//! (premultiplied f16, see [`crate::raster::TileData`]). The law is the one
//! map between them, kept per project in `meta.color_law`:
//! - **Legacy** (v0.3, and every file without the key): chips are
//!   linearised by the sRGB EOTF, so screen and export agree with each
//!   other but both sit darker than the chip.
//! - **Srgb** (new projects): chips are stored as picked (`c/255`), so the
//!   export bytes equal the chip and compositing happens in the stored
//!   (gamma) space — the AE / Photoshop / CSP default.
//!
//! The stored-to-bytes path (export.rs) is the same under both laws; only
//! [`ColorLaw::encode`] and [`ColorLaw::decode`] differ. Every place that
//! turns a chip into a stored value goes through them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use crate::camera::IRect;
use crate::command::{Command, CutRef};
use crate::export::{linear_to_srgb, linear_to_srgb_f, srgb_to_linear};
use crate::ids::{ColumnId, DrawingId, LayerId, SceneId};
use crate::model::{Cut, Drawing, LayerRole, Project};
use crate::raster::{
    TILE, TileCoord, TileData, TileDiff, f16_bits_to_f32, f32_to_f16_bits,
};
use crate::xsheet::ColumnKind;

/// How a project stores chip colours (D8). A missing `meta.color_law`
/// means Legacy, so every older file renders as it always did.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ColorLaw {
    /// v0.3: chips linearised on the way in (darker than the chip on
    /// screen and in export, the two agreeing).
    #[default]
    Legacy,
    /// Chips stored exactly as picked: export bytes equal the chip.
    Srgb,
}

impl ColorLaw {
    /// The persisted `meta.color_law` value. Never rename these.
    pub fn key(self) -> &'static str {
        match self {
            ColorLaw::Legacy => "legacy",
            ColorLaw::Srgb => "srgb",
        }
    }

    pub fn from_key(key: &str) -> Option<ColorLaw> {
        [ColorLaw::Legacy, ColorLaw::Srgb]
            .into_iter()
            .find(|l| l.key() == key)
    }

    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            ColorLaw::Legacy => "legacy (linear, v0.3)",
            ColorLaw::Srgb => "sRGB (exact chips)",
        }
    }

    /// One chip channel → its stored (straight) value.
    pub fn encode(self, v: u8) -> f32 {
        match self {
            ColorLaw::Legacy => srgb_to_linear(v),
            ColorLaw::Srgb => v as f32 / 255.0,
        }
    }

    /// A stored (straight) channel → the chip byte that paints it. The
    /// exact inverse of [`Self::encode`] on every byte.
    pub fn decode(self, v: f32) -> u8 {
        match self {
            ColorLaw::Legacy => linear_to_srgb(v),
            ColorLaw::Srgb => (v.clamp(0.0, 1.0) * 255.0).round() as u8,
        }
    }

    /// A chip → its straight stored RGBA. Alpha is coverage, `a/255` under
    /// both laws.
    pub fn encode_rgba(self, c: [u8; 4]) -> [f32; 4] {
        [
            self.encode(c[0]),
            self.encode(c[1]),
            self.encode(c[2]),
            c[3] as f32 / 255.0,
        ]
    }

    /// A chip → the premultiplied texel a hard fill of it writes.
    pub fn premultiplied(self, c: [u8; 4]) -> [f32; 4] {
        let s = self.encode_rgba(c);
        [s[0] * s[3], s[1] * s[3], s[2] * s[3], s[3]]
    }

    /// A chip → the exact f16 texel bits a hard fill of it stores — the
    /// key recolour and the paint check match on.
    pub fn texel(self, c: [u8; 4]) -> [u16; 4] {
        self.premultiplied(c).map(f32_to_f16_bits)
    }

    /// A premultiplied stored pixel → the chip that paints it, alpha
    /// included; None where it is bare.
    pub fn decode_pixel(self, px: [f32; 4]) -> Option<[u8; 4]> {
        let a = px[3].clamp(0.0, 1.0);
        if a <= 0.0 {
            return None;
        }
        Some([
            self.decode(px[0] / a),
            self.decode(px[1] / a),
            self.decode(px[2] / a),
            (a * 255.0).round() as u8,
        ])
    }
}

/// The roles a recolour rewrites: the paint flats and Custom (never a
/// line role, never reference, rough or correction).
pub fn is_recolour_role(role: LayerRole) -> bool {
    matches!(
        role,
        LayerRole::Color | LayerRole::Shadow | LayerRole::Highlight | LayerRole::Custom
    )
}

/// The roles the paint check reads as paint: the shiage flats. Custom is
/// left out — a legacy one-layer cel keeps its line art there.
pub fn is_paint_role(role: LayerRole) -> bool {
    matches!(
        role,
        LayerRole::Color | LayerRole::Shadow | LayerRole::Highlight
    )
}

/// One texel's four f16 bit patterns.
fn texel_at(tile: &TileData, i: usize) -> [u16; 4] {
    [
        tile.rgba[i * 4],
        tile.rgba[i * 4 + 1],
        tile.rgba[i * 4 + 2],
        tile.rgba[i * 4 + 3],
    ]
}

/// f16 ±0 alpha: nothing there.
fn bare(t: [u16; 4]) -> bool {
    t[3] & 0x7FFF == 0
}

/// A tile after a texel rewrite, as a diff entry (None = no change).
fn rewritten(
    coord: TileCoord,
    before: &Arc<TileData>,
    px: Vec<u16>,
) -> Option<(TileCoord, Option<Arc<TileData>>, Option<Arc<TileData>>)> {
    let after = TileData::from_vec(px);
    if after.rgba == before.rgba {
        return None;
    }
    let after = (!after.is_empty()).then(|| Arc::new(after));
    Some((coord, Some(before.clone()), after))
}

/// Every texel of `tiles` exactly equal to `from` becomes `to`.
fn swap_texels(
    tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    from: [u16; 4],
    to: [u16; 4],
) -> (TileDiff, usize) {
    let mut diff = Vec::new();
    let mut n = 0;
    for (coord, tile) in tiles {
        let mut px: Option<Vec<u16>> = None;
        for i in 0..TILE * TILE {
            if texel_at(tile, i) == from {
                let p = px.get_or_insert_with(|| tile.rgba.to_vec());
                p[i * 4..i * 4 + 4].copy_from_slice(&to);
                n += 1;
            }
        }
        if let Some(entry) = px.and_then(|p| rewritten(*coord, tile, p)) {
            diff.push(entry);
        }
    }
    (diff, n)
}

// ---- Converting a Legacy project -----------------------------------------

/// D8: the colour-law migration, in place: every cel-layer texel's colour
/// goes through the sRGB OETF (coverage kept), so a Legacy flat of chip `c`
/// (stored `lin(c)`) becomes `c/255` and exports as the chip; the law
/// becomes Srgb. Image assets are left alone — import stores their bytes
/// as they are under both laws. Returns false (and changes nothing) when
/// the project is already Srgb.
///
/// A file-level migration outside the undo system (NEVER-DO 3): the caller
/// writes the backup first and clears the history after.
pub fn convert_legacy_to_srgb(project: &mut Project) -> bool {
    if project.color_law == ColorLaw::Srgb {
        return false;
    }
    // Each byte's Legacy channel bits → its Srgb bits.
    let exact: HashMap<u16, u16> = (0..=255u8)
        .map(|b| {
            let bits = |law: ColorLaw| f32_to_f16_bits(law.encode(b));
            (bits(ColorLaw::Legacy), bits(ColorLaw::Srgb))
        })
        .collect();
    for scene in &mut project.scenes {
        for cut in &mut scene.cuts {
            for d in &mut cut.drawings {
                for layer in &mut d.layers {
                    for tile in layer.raster.tiles.values_mut() {
                        *tile = Arc::new(remap_tile(tile, &exact));
                    }
                }
            }
        }
    }
    project.color_law = ColorLaw::Srgb;
    true
}

/// One tile through the OETF, colour only, premultiplication kept. An
/// opaque channel holding exactly a Legacy chip byte becomes exactly that
/// byte's Srgb value (the float OETF can land one f16 step off), so
/// converted flats stay exact chips that recolour and the paint check
/// still match.
fn remap_tile(tile: &TileData, exact: &HashMap<u16, u16>) -> TileData {
    let opaque = f32_to_f16_bits(1.0);
    let mut px = tile.rgba.to_vec();
    for t in px.chunks_exact_mut(4) {
        let a = f16_bits_to_f32(t[3]);
        if a <= 0.0 {
            continue;
        }
        let snap = t[3] == opaque;
        for c in t.iter_mut().take(3) {
            if snap && let Some(&s) = exact.get(c) {
                *c = s;
                continue;
            }
            let s = (f16_bits_to_f32(*c) / a).clamp(0.0, 1.0);
            *c = f32_to_f16_bits(linear_to_srgb_f(s) * a);
        }
    }
    TileData::from_vec(px)
}

// ---- Recolour by chip ----------------------------------------------------

/// Where a recolour reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecolourScope {
    Cut(CutRef),
    Scene(SceneId),
    Project,
}

impl RecolourScope {
    fn holds(self, scene: SceneId, cut: crate::ids::CutId) -> bool {
        match self {
            RecolourScope::Cut(at) => at.scene == scene && at.cut == cut,
            RecolourScope::Scene(s) => s == scene,
            RecolourScope::Project => true,
        }
    }
}

/// Does a locked column expose this drawing? Its art is then refused by
/// the engine (GM5), so a recolour leaves it alone.
fn locked_art(cut: &Cut, id: DrawingId) -> bool {
    cut.xsheet
        .columns
        .iter()
        .any(|c| cut.column_def(c.id).locked && !c.keys_referencing(id).is_empty())
}

/// Recolour by chip edit: every opaque texel exactly equal to chip `from`
/// (as the project's law stores it) on a Color, Shadow, Highlight or Custom
/// layer in `scope` becomes chip `to`. Line roles are never touched, nor
/// are locked layers or the art of locked columns. One PaintTiles per
/// layer that changes — apply them as ONE step. `from`'s alpha is ignored
/// (the match is opaque); `to` keeps its own.
pub fn recolour(
    project: &Project,
    scope: RecolourScope,
    from: [u8; 4],
    to: [u8; 4],
) -> Vec<Command> {
    let law = project.color_law;
    let old = law.texel([from[0], from[1], from[2], 255]);
    let new = law.texel(to);
    if old == new {
        return Vec::new();
    }
    let mut out = Vec::new();
    for scene in &project.scenes {
        for cut in scene.cuts.iter().filter(|c| scope.holds(scene.id, c.id)) {
            let at = CutRef {
                scene: scene.id,
                cut: cut.id,
            };
            for d in cut.drawings.iter().filter(|d| !locked_art(cut, d.id)) {
                for layer in &d.layers {
                    if !is_recolour_role(layer.props.role) || layer.props.locked {
                        continue;
                    }
                    let (diff, _) = swap_texels(&layer.raster.tiles, old, new);
                    if !diff.is_empty() {
                        out.push(Command::PaintTiles {
                            at,
                            id: d.id,
                            layer: layer.id,
                            diff,
                        });
                    }
                }
            }
        }
    }
    out
}

// ---- Refill and replace-all ----------------------------------------------

/// Why a refill or replace-all changed nothing — callers word the refusal
/// from the cause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefillRefusal {
    /// The click was off the paper.
    OffPaper,
    /// No paint under the click (a plain fill floods bare paper).
    Bare,
    /// The click is on the line art bounding the refill.
    OnLine,
    /// That paint already is the new colour.
    Unchanged,
    /// The click is outside the active selection (refills respect it,
    /// as fills do).
    OutsideSelection,
}

/// The paper pixels inside `clip` (a paper-space polygon: the select tool's
/// active selection), a grid over `area` row-major; None when there is no
/// selection.
fn clip_grid(clip: Option<&[(f32, f32)]>, area: IRect) -> Option<Vec<bool>> {
    let poly = clip.filter(|p| p.len() >= 3)?;
    let (w, h) = (area.w as usize, area.h as usize);
    let (ax, ay) = (area.x as i64, area.y as i64);
    let mut grid = vec![false; w * h];
    // Only the polygon's bbox can hold inside pixels.
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in poly {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    let span = |a: f32, b: f32, o: i64, n: usize| {
        let lo = ((a.floor() as i64).clamp(o, o + n as i64) - o) as usize;
        let hi = ((b.ceil() as i64).clamp(o, o + n as i64) - o) as usize;
        lo..hi
    };
    for y in span(y0, y1, ay, h) {
        for x in span(x0, x1, ax, w) {
            let (fx, fy) = ((x as i64 + ax) as f32, (y as i64 + ay) as f32);
            grid[y * w + x] = crate::edit::contains(poly, fx + 0.5, fy + 0.5);
        }
    }
    Some(grid)
}

/// The texel under paper pixel (x, y) of a tile map (bare where no tile).
fn texel_of(tiles: &BTreeMap<TileCoord, Arc<TileData>>, x: i32, y: i32) -> [u16; 4] {
    let t = TILE as i32;
    match tiles.get(&(x.div_euclid(t), y.div_euclid(t))) {
        Some(tile) => texel_at(tile, (y.rem_euclid(t) * t + x.rem_euclid(t)) as usize),
        None => [0; 4],
    }
}

/// Paper grid (w×h, row-major) of `pred` over every paper texel of
/// `tiles`; pixels with no tile are false.
fn paper_grid(
    tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    w: usize,
    h: usize,
    pred: impl FnMut([u16; 4]) -> bool,
) -> Vec<bool> {
    paper_grid_in(tiles, IRect::frame(w as u32, h as u32), pred)
}

/// [`paper_grid`] over any paper `area` (its grid starts at the area's
/// top-left, which may lie left of or above the frame).
fn paper_grid_in(
    tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    area: IRect,
    pred: impl FnMut([u16; 4]) -> bool,
) -> Vec<bool> {
    let mut grid = vec![false; area.w as usize * area.h as usize];
    mark_grid_in(&mut grid, tiles, area, pred);
    grid
}

/// Set (never clear) the cells of a w×h paper `grid` where `pred` holds
/// for the texel of `tiles`.
fn mark_grid(
    grid: &mut [bool],
    tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    w: usize,
    h: usize,
    pred: impl FnMut([u16; 4]) -> bool,
) {
    mark_grid_in(grid, tiles, IRect::frame(w as u32, h as u32), pred);
}

/// [`mark_grid`] over a grid covering paper `area`.
fn mark_grid_in(
    grid: &mut [bool],
    tiles: &BTreeMap<TileCoord, Arc<TileData>>,
    area: IRect,
    mut pred: impl FnMut([u16; 4]) -> bool,
) {
    let (w, h) = (area.w as usize, area.h as usize);
    let (ax, ay) = (area.x as i64, area.y as i64);
    for ((tx, ty), tile) in tiles {
        for row in 0..TILE {
            let y = *ty as i64 * TILE as i64 + row as i64 - ay;
            if y < 0 || y >= h as i64 {
                continue;
            }
            for cx in 0..TILE {
                let x = *tx as i64 * TILE as i64 + cx as i64 - ax;
                if x < 0 || x >= w as i64 {
                    continue;
                }
                if pred(texel_at(tile, row * TILE + cx)) {
                    grid[y as usize * w + x as usize] = true;
                }
            }
        }
    }
}

/// 4-connected flood from `seed` over the cells where `open` holds.
fn flood4(open: &[bool], w: usize, h: usize, seed: usize) -> Vec<bool> {
    let mut bits = vec![false; w * h];
    if !open[seed] {
        return bits;
    }
    let mut stack = vec![seed];
    bits[seed] = true;
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        let mut push = |j: usize| {
            if open[j] && !bits[j] {
                bits[j] = true;
                stack.push(j);
            }
        };
        if x > 0 {
            push(i - 1);
        }
        if x + 1 < w {
            push(i + 1);
        }
        if y > 0 {
            push(i - w);
        }
        if y + 1 < h {
            push(i + w);
        }
    }
    bits
}

/// Write `to` into every texel of `target` the `mask` (a grid over paper
/// `area`) selects.
fn paint_mask(
    target: &BTreeMap<TileCoord, Arc<TileData>>,
    mask: &[bool],
    area: IRect,
    to: [u16; 4],
) -> TileDiff {
    let t = TILE as i32;
    let (w, h) = (area.w as i64, area.h as i64);
    let (ax, ay) = (area.x as i64, area.y as i64);
    let mut diff = Vec::new();
    if w == 0 || h == 0 {
        return diff;
    }
    for ty in area.y.div_euclid(t)..=(area.y + area.h as i32 - 1).div_euclid(t) {
        for tx in area.x.div_euclid(t)..=(area.x + area.w as i32 - 1).div_euclid(t) {
            let Some(before) = target.get(&(tx, ty)) else {
                continue; // a refill only rewrites paint, which has a tile
            };
            let mut px: Option<Vec<u16>> = None;
            for row in 0..TILE {
                let y = (ty * t) as i64 + row as i64 - ay;
                if y < 0 || y >= h {
                    continue;
                }
                for cx in 0..TILE {
                    let x = (tx * t) as i64 + cx as i64 - ax;
                    if x < 0 || x >= w || !mask[(y * w + x) as usize] {
                        continue;
                    }
                    let i = (row * TILE + cx) * 4;
                    px.get_or_insert_with(|| before.rgba.to_vec())[i..i + 4]
                        .copy_from_slice(&to);
                }
            }
            if let Some(entry) = px.and_then(|p| rewritten((tx, ty), before, p)) {
                diff.push(entry);
            }
        }
    }
    diff
}

/// The paint value under the click, checked: on the paper and not bare.
fn seed_texel(
    target: &BTreeMap<TileCoord, Arc<TileData>>,
    seed: (i32, i32),
    area: IRect,
) -> Result<[u16; 4], RefillRefusal> {
    let (sx, sy) = (seed.0 as i64 - area.x as i64, seed.1 as i64 - area.y as i64);
    if sx < 0 || sy < 0 || sx >= area.w as i64 || sy >= area.h as i64 {
        return Err(RefillRefusal::OffPaper);
    }
    let v = texel_of(target, seed.0, seed.1);
    if bare(v) {
        return Err(RefillRefusal::Bare);
    }
    Ok(v)
}

/// REFILL (Shift+click with Fill): the 4-connected region of exactly the
/// clicked texel value on `target`, walled by `line` (texels at or above
/// 0.1 alpha — the drawing's line art), becomes `color` (a premultiplied
/// stored texel). The same value tucked under the line within `under_px`
/// of the region goes too, so a recoloured flat leaves no old halo at the
/// line's soft edge. `clip` (the active selection) walls the region like
/// the line does, and a click outside it is refused. Paper pixels only;
/// one diff for one PaintTiles.
#[allow(clippy::too_many_arguments)] // the fill tool's own knobs, as flood_fill_mask
pub fn refill(
    target: &BTreeMap<TileCoord, Arc<TileData>>,
    line: &BTreeMap<TileCoord, Arc<TileData>>,
    seed: (i32, i32),
    width: u32,
    height: u32,
    under_px: u32,
    color: [f32; 4],
    clip: Option<&[(f32, f32)]>,
) -> Result<TileDiff, RefillRefusal> {
    let area = IRect::frame(width, height);
    refill_in(target, line, seed, area, under_px, color, clip)
}

/// [`refill`] over any paper `area` (frame pixels; its edges wall the
/// region, a click outside it is `OffPaper`). Over the frame rect it is
/// the frame refill bit for bit.
pub fn refill_in(
    target: &BTreeMap<TileCoord, Arc<TileData>>,
    line: &BTreeMap<TileCoord, Arc<TileData>>,
    seed: (i32, i32),
    area: IRect,
    under_px: u32,
    color: [f32; 4],
    clip: Option<&[(f32, f32)]>,
) -> Result<TileDiff, RefillRefusal> {
    let value = seed_texel(target, seed, area)?;
    let to = color.map(f32_to_f16_bits);
    if to == value {
        return Err(RefillRefusal::Unchanged);
    }
    let (w, h) = (area.w as usize, area.h as usize);
    let inside = clip_grid(clip, area);
    let si = (seed.1 - area.y) as usize * w + (seed.0 - area.x) as usize;
    if inside.as_ref().is_some_and(|g| !g[si]) {
        return Err(RefillRefusal::OutsideSelection);
    }
    let mut same = paper_grid_in(target, area, |t| t == value);
    if let Some(g) = &inside {
        same.iter_mut().zip(g).for_each(|(s, i)| *s &= *i);
    }
    let ink = paper_grid_in(line, area, |t| f16_bits_to_f32(t[3]) >= 0.1);
    if ink[si] {
        return Err(RefillRefusal::OnLine);
    }
    let open: Vec<bool> = same.iter().zip(&ink).map(|(s, i)| *s && !*i).collect();
    let mut mask = flood4(&open, w, h, si);
    // Under the line: the same value within under_px of the region.
    let mut grown = mask.clone();
    dilate(&mut grown, w, h, under_px as usize);
    for i in 0..w * h {
        mask[i] |= grown[i] && same[i] && ink[i];
    }
    Ok(paint_mask(target, &mask, area, to))
}

/// REPLACE-ALL (Ctrl+Shift+click with Fill): every texel of `target`
/// exactly equal to the clicked one becomes `color`, on and off the
/// paper — or, with `clip` (the active selection), on the paper inside it
/// only. Returns the diff and the pixel count.
pub fn replace_all(
    target: &BTreeMap<TileCoord, Arc<TileData>>,
    seed: (i32, i32),
    width: u32,
    height: u32,
    color: [f32; 4],
    clip: Option<&[(f32, f32)]>,
) -> Result<(TileDiff, usize), RefillRefusal> {
    replace_all_in(target, seed, IRect::frame(width, height), color, clip)
}

/// [`replace_all`] with the click checked against any paper `area`, and a
/// selection read over it. Over the frame rect it is the frame form bit
/// for bit.
pub fn replace_all_in(
    target: &BTreeMap<TileCoord, Arc<TileData>>,
    seed: (i32, i32),
    area: IRect,
    color: [f32; 4],
    clip: Option<&[(f32, f32)]>,
) -> Result<(TileDiff, usize), RefillRefusal> {
    let value = seed_texel(target, seed, area)?;
    let to = color.map(f32_to_f16_bits);
    if to == value {
        return Err(RefillRefusal::Unchanged);
    }
    let w = area.w as usize;
    let Some(inside) = clip_grid(clip, area) else {
        return Ok(swap_texels(target, value, to));
    };
    if !inside[(seed.1 - area.y) as usize * w + (seed.0 - area.x) as usize] {
        return Err(RefillRefusal::OutsideSelection);
    }
    let mut mask = paper_grid_in(target, area, |t| t == value);
    mask.iter_mut().zip(&inside).for_each(|(m, i)| *m &= *i);
    let n = mask.iter().filter(|m| **m).count();
    Ok((paint_mask(target, &mask, area, to), n))
}

/// Square (Chebyshev) dilation by `r`, in place — the fill's grow law.
fn dilate(bits: &mut [bool], w: usize, h: usize, r: usize) {
    if r == 0 {
        return;
    }
    let src = bits.to_vec();
    let mut rows = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r).min(w - 1));
            rows[y * w + x] = src[y * w + x0..=y * w + x1].iter().any(|b| *b);
        }
    }
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r).min(h - 1));
        for x in 0..w {
            bits[y * w + x] = (y0..=y1).any(|yy| rows[yy * w + x]);
        }
    }
}

// ---- The paint check -----------------------------------------------------

/// What a paint check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingKind {
    /// Bare paper enclosed by the drawing's art (an unpainted region).
    Hole,
    /// Opaque paint that is not a chip of the palette.
    Stray,
    /// Part-covered paint on a paint layer — a feathered, grown or
    /// anti-aliased edge, whatever its colour: one finding per layer.
    /// Refill, replace-all and recolour match opaque chips only, so it is
    /// worth knowing, but it is an edge, not a wrong colour.
    SoftEdge,
}

/// One finding: where it is and how big.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub kind: FindingKind,
    /// The first checked frame that shows the drawing.
    pub frame: u32,
    pub column: ColumnId,
    pub drawing: DrawingId,
    /// A stray or soft edge: its layer. A hole: the drawing's color layer
    /// (where its paint belongs), if it has one.
    pub layer: Option<LayerId>,
    /// Paper pixels `[x0, y0, x1, y1)`.
    pub bbox: [u32; 4],
    pub pixels: usize,
    /// A stray: its colour, decoded as a chip. A soft edge: its first
    /// texel's, alpha being that texel's coverage.
    pub color: Option<[u8; 4]>,
}

/// One 4-connected component of a paper grid.
struct Component {
    bbox: [u32; 4],
    pixels: usize,
    first: usize,
    edge: bool,
}

/// Every 4-connected component of the set cells, in scan order.
fn components(grid: &[bool], w: usize, h: usize) -> Vec<Component> {
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !grid[start] || seen[start] {
            continue;
        }
        let mut c = Component {
            bbox: [u32::MAX, u32::MAX, 0, 0],
            pixels: 0,
            first: start,
            edge: false,
        };
        seen[start] = true;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            c.pixels += 1;
            c.bbox = [
                c.bbox[0].min(x as u32),
                c.bbox[1].min(y as u32),
                c.bbox[2].max(x as u32 + 1),
                c.bbox[3].max(y as u32 + 1),
            ];
            c.edge |= x == 0 || y == 0 || x + 1 == w || y + 1 == h;
            let mut visit = |j: usize| {
                if grid[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        out.push(c);
    }
    out
}

/// THE PAINT CHECK over `frames` of a cut (paper `size`): every drawing a
/// rendering Cel column shows in the range, once each, at the first frame
/// that shows it.
/// - **Holes**: 4-connected bare regions of the drawing's art (its visible
///   layers but reference, rough and correction — those are not the cel)
///   that do not touch the paper's edge (so they are enclosed by its art),
///   at least `min_px` pixels.
/// - **Stray colours**: opaque texels on Color / Shadow / Highlight layers
///   that are not exactly a chip of `chips` stored under `law`, grouped in
///   4-connected components per layer.
/// - **Soft edges**: part-covered texels on those layers, one finding per
///   layer (their union box and count).
///
/// Pure and deterministic (frame, then sheet order, holes before strays,
/// a layer's strays before its soft edge, scan order), so it runs on any
/// thread.
pub fn paint_check(
    cut: &Cut,
    law: ColorLaw,
    size: (u32, u32),
    frames: Range<u32>,
    chips: &[[u8; 4]],
    min_px: usize,
) -> Vec<Finding> {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let mut out = Vec::new();
    if w == 0 || h == 0 {
        return out;
    }
    let chip_texels: HashSet<[u16; 4]> = chips
        .iter()
        .map(|c| law.texel([c[0], c[1], c[2], 255]))
        .collect();
    let mut checked: HashSet<DrawingId> = HashSet::new();
    for frame in frames.start..frames.end.min(cut.frame_count) {
        for col in cut.render_order().into_iter().filter_map(|id| cut.xsheet.column(id)) {
            if cut.column_def(col.id).kind != ColumnKind::Cel {
                continue; // BG / Book art is not palette paint
            }
            let Some(d) = col.resolve(frame).and_then(|id| cut.drawing(id)) else {
                continue;
            };
            if !checked.insert(d.id) {
                continue;
            }
            check_drawing(d, law, (w, h), &chip_texels, min_px, &mut |kind, layer, c, color| {
                out.push(Finding {
                    kind,
                    frame,
                    column: col.id,
                    drawing: d.id,
                    layer,
                    bbox: c.bbox,
                    pixels: c.pixels,
                    color,
                });
            });
        }
    }
    out
}

type Found<'a> = dyn FnMut(FindingKind, Option<LayerId>, &Component, Option<[u8; 4]>) + 'a;

fn check_drawing(
    d: &Drawing,
    law: ColorLaw,
    (w, h): (usize, usize),
    chips: &HashSet<[u16; 4]>,
    min_px: usize,
    found: &mut Found<'_>,
) {
    // The cel's art walls the holes: the coverage of every layer its
    // flatten shows (visible, opacity above 0) but the reference, rough and
    // correction layers — a visible rough over a bare region must not hide
    // the hole.
    let mut inked = vec![false; w * h];
    for layer in &d.layers {
        let p = &layer.props;
        if !p.visible
            || p.opacity <= 0.0
            || matches!(p.role, LayerRole::Reference | LayerRole::Rough | LayerRole::Correction)
        {
            continue;
        }
        mark_grid(&mut inked, &layer.raster.tiles, w, h, |t| !bare(t));
    }
    let bare_grid: Vec<bool> = inked.iter().map(|b| !*b).collect();
    let home = d
        .layers
        .iter()
        .find(|l| l.props.role == LayerRole::Color)
        .map(|l| l.id);
    for c in components(&bare_grid, w, h) {
        if !c.edge && c.pixels >= min_px {
            found(FindingKind::Hole, home, &c, None);
        }
    }
    let opaque = f32_to_f16_bits(1.0);
    for layer in d.layers.iter().filter(|l| is_paint_role(l.props.role)) {
        let tiles = &layer.raster.tiles;
        let colour_at = |i: usize| {
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            law.decode_pixel(texel_of(tiles, x, y).map(f16_bits_to_f32))
        };
        let stray = paper_grid(tiles, w, h, |t| t[3] == opaque && !chips.contains(&t));
        for c in components(&stray, w, h) {
            found(FindingKind::Stray, Some(layer.id), &c, colour_at(c.first));
        }
        // Soft texels: one finding for the layer, however many edges.
        let soft = paper_grid(tiles, w, h, |t| !bare(t) && t[3] != opaque);
        let mut rim = Component {
            bbox: [u32::MAX, u32::MAX, 0, 0],
            pixels: 0,
            first: 0,
            edge: false,
        };
        for (i, _) in soft.iter().enumerate().filter(|(_, s)| **s) {
            let (x, y) = ((i % w) as u32, (i / w) as u32);
            if rim.pixels == 0 {
                rim.first = i;
            }
            rim.pixels += 1;
            rim.bbox = [
                rim.bbox[0].min(x),
                rim.bbox[1].min(y),
                rim.bbox[2].max(x + 1),
                rim.bbox[3].max(y + 1),
            ];
        }
        if rim.pixels > 0 {
            found(FindingKind::SoftEdge, Some(layer.id), &rim, colour_at(rim.first));
        }
    }
}
