//! THE SIMPLE TIMELINE (AU5, spec 7.2): a second view of the active track's
//! exposures, along the band. It keeps no data: it reads `Column::runs` and
//! writes through the verbs the Timing sheet shares.
//!
//! One surface takes every gesture, with its own tap and its own long press:
//! a press on anything that also senses clicks loses its capture to egui's
//! long touch at 0.817 s under a pen.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use anim_core::ids::{CutId, DrawingId};
use anim_core::raster::{TILE, TileCoord, TileData, f16_bits_to_f32};
use anim_core::xsheet::Exposure;
use eframe::egui;
use egui::accesskit;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::doc::{AppState, FINISH_STROKE, drawing_has_art};
use crate::lesson::color_layer_art;
use crate::lessons::Target;
use crate::plate::{self, Card};
use crate::rail::STRIP_H;

/// The steel bar across the top of the strip.
pub const BAR_H: f32 = 14.0;
/// The sheets under it; the 4 left of `STRIP_H` are the gap below them.
pub const SHEET_H: f32 = 38.0;
/// A sheet's last points: a drag there sets its hold.
const EDGE: f32 = 8.0;
/// A press held still this long opens the cell menu.
const LONG_PRESS: f64 = 0.6;
/// A press that moves this far is a drag, not a tap.
const SLOP: f32 = 6.0;
/// The widest a sheet's frame ever stands, in points.
const WIDEST: f32 = 48.0;
/// The default scale's floor; Ctrl+wheel goes down to `NARROWEST`.
const DEFAULT_FLOOR: f32 = 8.0;
const NARROWEST: f32 = 6.0;
/// A sheet this wide carries its number.
const NUMBERED: f32 = 16.0;
/// A sheet this wide says `Used twice` beside its number; a narrower one
/// marks it.
const WORDED: f32 = 100.0;
/// The row a sheet's number and badge stand on, clear of the peg hole.
const SHEET_ROW: f32 = 20.0;
/// The census counts at most this many sheets (spec 7.2).
pub const CENSUS_CAP: usize = 12;
/// A sheet this wide shows its picture; a narrower one keeps its number.
const PICTURED: f32 = 28.0;
/// Where a picture stands on its sheet, under the peg hole.
const PICTURE_TOP: f32 = 12.0;
const PICTURE_FOOT: f32 = 2.0;
const PICTURE_SIDE: f32 = 4.0;
/// A picture's largest size in pixels, twice what it stands at on the sheet.
const PICTURE_PX: [usize; 2] = [96, 52];
/// What a covered cell shows of its ink at the least, times the strongest
/// pixel in it: a thin line reads at 3:1 on its sheet, and a pale wash, whose
/// strongest pixel is pale too, stays pale.
const INK_FLOOR: f32 = 0.8;

/// One sheet: a run of the active track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sheet {
    pub from: u32,
    pub len: u32,
    pub shows: Exposure,
}

/// The range the ruler marks: a loop range that is not the whole cut. AU26: a
/// blank tail plays short of the cut, but nobody set that, so it marks nothing.
pub fn marked_range(state: &AppState) -> Option<(u32, u32)> {
    let range = crate::playback::play_range(state.loop_range(), state.frame_count());
    (range != (0, state.frame_count() - 1)).then_some(range)
}

/// The active track's runs, as sheets.
pub fn sheets(state: &AppState) -> Vec<Sheet> {
    let Some(column) = state.cut().xsheet.column(state.view.active_column) else {
        return Vec::new();
    };
    let mut from = 0;
    let runs = column.runs(0, state.frame_count());
    runs.into_iter()
        .map(|(shows, len)| {
            let sheet = Sheet { from, len, shows };
            from += len;
            sheet
        })
        .collect()
}

/// The hold every drawing on the track shares, the one Speed shows lit.
pub fn speed_of(state: &AppState) -> Option<u32> {
    let sheets = sheets(state);
    let mut holds = sheets.iter().filter(|s| matches!(s.shows, Exposure::Drawing(_))).map(|s| s.len);
    let first = holds.next()?;
    holds.all(|h| h == first).then_some(first)
}

/// What a sheet is called on its card and aloud.
fn span(sheet: &Sheet) -> String {
    match sheet.len {
        1 => format!("frame {}", sheet.from + 1),
        n => format!("frames {} to {}", sheet.from + 1, sheet.from + n),
    }
}

/// One drawing on the strip: its number, in the order it first appears, and
/// how many sheets show it.
#[derive(Clone, Copy, Default)]
struct Tally {
    number: u32,
    uses: u32,
}

/// Every drawing on the track, read once a frame: a drawing exposed again
/// keeps its number.
fn tally(sheets: &[Sheet]) -> HashMap<DrawingId, Tally> {
    let mut seen: HashMap<DrawingId, Tally> = HashMap::new();
    for sheet in sheets {
        if let Exposure::Drawing(id) = sheet.shows {
            let next = seen.len() as u32 + 1;
            let entry = seen.entry(id).or_insert(Tally { number: next, uses: 0 });
            entry.uses += 1;
        }
    }
    seen
}

/// One drawing's tiles and opacity, held by a picture job.
pub(crate) type PictureLayer = (BTreeMap<TileCoord, Arc<TileData>>, f32);

/// The picture being made, off the UI thread.
struct PictureJob {
    id: DrawingId,
    key: u64,
    rx: Receiver<Option<egui::ColorImage>>,
}

/// AU13: the sheets' pictures (spec 7.2): keyed on the drawing's content, for
/// the sheets on screen, one job at a time and none while a stroke is live.
#[derive(Default)]
struct Pictures {
    /// The key each picture was made from: None where there is nothing to
    /// show. A stale one stays until its new one lands.
    held: HashMap<DrawingId, (u64, Option<egui::TextureHandle>)>,
    /// The content keys of the drawings asked about since `keyed` moved.
    keys: HashMap<DrawingId, u64>,
    keyed: Option<(CutId, u64, u64)>,
    job: Option<PictureJob>,
    /// Jobs started, for the tests.
    #[cfg(test)]
    started: u32,
    /// Off the UI thread here too: the suite makes its pictures in line.
    #[cfg(test)]
    threaded: bool,
}

impl Pictures {
    /// Pick up a finished picture and start the next one, for `shown`.
    fn refresh(&mut self, ctx: &egui::Context, state: &AppState, shown: &[DrawingId], gesture_safe: bool) {
        if !gesture_safe {
            return;
        }
        if let Some(job) = &self.job {
            match job.rx.try_recv() {
                Ok(image) => {
                    let (id, key) = (job.id, job.key);
                    self.job = None;
                    self.land(ctx, id, key, image);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => self.job = None,
            }
        }
        let stamp = (state.cut().id, state.doc_gen, state.engine.revision());
        if self.keyed != Some(stamp) {
            self.keyed = Some(stamp);
            self.keys.clear();
            self.held.retain(|id, _| state.cut().drawing(*id).is_some());
        }
        let (keys, held) = (&mut self.keys, &self.held);
        let stale = shown.iter().copied().find(|id| {
            let key = *keys.entry(*id).or_insert_with(|| content_key(state, *id));
            held.get(id).is_none_or(|(made, _)| *made != key)
        });
        let Some(id) = stale else {
            return;
        };
        let key = self.keys[&id];
        let layers = layers_of(state, id);
        let paper = {
            let p = &state.engine.project;
            [p.width as usize, p.height as usize]
        };
        #[cfg(test)]
        {
            self.started += 1;
        }
        if !self.aside() {
            self.land(ctx, id, key, picture(&layers, paper));
            return;
        }
        let (tx, rx) = channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(picture(&layers, paper));
            repaint.request_repaint();
        });
        self.job = Some(PictureJob { id, key, rx });
    }

    fn land(&mut self, ctx: &egui::Context, id: DrawingId, key: u64, image: Option<egui::ColorImage>) {
        let name = format!("strip_picture_{}", id.0);
        let texture = image.map(|image| ctx.load_texture(name, image, egui::TextureOptions::LINEAR));
        self.held.insert(id, (key, texture));
    }

    /// The job runs on its own thread.
    fn aside(&self) -> bool {
        #[cfg(test)]
        return self.threaded;
        #[cfg(not(test))]
        true
    }
}

/// What a drawing's picture is made from, as a key: 0 for nothing to show.
fn content_key(state: &AppState, id: DrawingId) -> u64 {
    state.drawing_composite(id, None, false).map_or(0, |(_, key)| key)
}

fn layers_of(state: &AppState, id: DrawingId) -> Vec<PictureLayer> {
    let slices = state.drawing_composite(id, None, false).map(|(slices, _)| slices);
    slices.unwrap_or_default().into_iter().map(|(tiles, opacity)| (tiles.clone(), opacity)).collect()
}

/// One pixel of `tiles`: premultiplied, and scaled by the layer's opacity.
fn texel(tile: &TileData, x: usize, y: usize, opacity: f32) -> [f32; 4] {
    let at = (y * TILE + x) * 4;
    std::array::from_fn(|c| f16_bits_to_f32(tile.rgba[at + c]) * opacity)
}

/// The drawing's visible layers on white paper, small: every pixel on the
/// paper is composited, then averaged into its cell. A thin line must still
/// read at a twentieth of its size, so what covers a cell is lifted, and on a
/// shrunk paper it takes the cells round it too: drawn at half the texture's
/// size, a line one cell wide falls between the screen's pixels. A paper
/// smaller than a picture keeps its own pixels, each with its own cell. None
/// when nothing is drawn on the paper. Runs on the job's thread.
pub(crate) fn picture(layers: &[PictureLayer], paper: [usize; 2]) -> Option<egui::ColorImage> {
    let [width, height] = paper.map(|side| side.max(1));
    let fit = (PICTURE_PX[0] as f32 / width as f32).min(PICTURE_PX[1] as f32 / height as f32);
    let cells = [width, height].map(|side| ((side as f32 * fit).round() as usize).clamp(1, side));
    let mut sums = vec![[0.0f32; 4]; cells[0] * cells[1]];
    let mut peaks = vec![0.0f32; cells[0] * cells[1]];
    let mut coords: Vec<TileCoord> = layers.iter().flat_map(|(tiles, _)| tiles.keys().copied()).collect();
    coords.sort_unstable();
    coords.dedup();
    let t = TILE as i64;
    for (tx, ty) in coords {
        let found: Vec<_> = layers.iter().filter_map(|(tiles, o)| Some((tiles.get(&(tx, ty))?, *o))).collect();
        for y in 0..TILE {
            let py = ty as i64 * t + y as i64;
            if !(0..height as i64).contains(&py) {
                continue;
            }
            let row = py as usize * cells[1] / height;
            for x in 0..TILE {
                let px = tx as i64 * t + x as i64;
                if !(0..width as i64).contains(&px) {
                    continue;
                }
                let mut over = [0.0f32; 4];
                for (tile, opacity) in &found {
                    let s = texel(tile, x, y, *opacity);
                    over = std::array::from_fn(|c| s[c] + over[c] * (1.0 - s[3]));
                }
                let cell = row * cells[0] + px as usize * cells[0] / width;
                for c in 0..4 {
                    sums[cell][c] += over[c];
                }
                peaks[cell] = peaks[cell].max(over[3]);
            }
        }
    }
    // Cells take a whole number of pixels, so they do not all take the same.
    let spans = |side: usize, cells: usize| {
        let mut spans = vec![0.0f32; cells];
        (0..side).for_each(|at| spans[at * cells / side] += 1.0);
        spans
    };
    let (wide, tall) = (spans(width, cells[0]), spans(height, cells[1]));
    // Each cell's ink, unmixed, and how much of it shows.
    let inks: Vec<[f32; 4]> = sums
        .iter()
        .zip(&peaks)
        .enumerate()
        .map(|(at, (sum, peak))| {
            let cover = (sum[3] / (wide[at % cells[0]] * tall[at / cells[0]])).clamp(0.0, 1.0);
            let a = sum[3].max(f32::EPSILON);
            [sum[0] / a, sum[1] / a, sum[2] / a, cover.sqrt().max(INK_FLOOR * peak.min(1.0))]
        })
        .collect();
    let reach = usize::from(fit < 1.0);
    let strongest = |at: usize| {
        let (x, y) = (at % cells[0], at / cells[0]);
        let around = |here: usize, side: usize| here.saturating_sub(reach)..=(here + reach).min(side - 1);
        let near = around(y, cells[1]).flat_map(|y| around(x, cells[0]).map(move |x| y * cells[0] + x));
        near.fold(inks[at], |best, cell| if inks[cell][3] > best[3] { inks[cell] } else { best })
    };
    let mut drawn = false;
    let pixels = (0..inks.len())
        .map(|at| {
            let [r, g, b, shown] = strongest(at);
            if shown <= 0.0 {
                return plate::ON_FILL;
            }
            drawn = true;
            let shade = |c: f32| ((c.clamp(0.0, 1.0) * shown + 1.0 - shown) * 255.0).round() as u8;
            plate::chip([shade(r), shade(g), shade(b), 255])
        })
        .collect();
    drawn.then_some(egui::ColorImage { size: cells, source_size: vec2(cells[0] as f32, cells[1] as f32), pixels })
}

/// Frames and points, across one surface.
#[derive(Clone, Copy)]
struct Ruler {
    left: f32,
    scale: f32,
    last: u32,
}

impl Ruler {
    fn x(self, frame: u32) -> f32 {
        self.left + frame as f32 * self.scale
    }

    fn frame(self, x: f32) -> u32 {
        (((x - self.left) / self.scale).floor().max(0.0) as u32).min(self.last)
    }
}

/// What a press took hold of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grip {
    /// The peg bar: a drag scrubs.
    Bar,
    /// A sheet: its first frame.
    Sheet { from: u32 },
    /// A sheet's right edge: a drag sets its length.
    Edge { from: u32, len: u32 },
}

impl Grip {
    /// The run this grip holds is still there: an undo or a paste under the
    /// hand can take it away.
    fn stands(self, sheets: &[Sheet]) -> bool {
        match self {
            Grip::Bar => true,
            Grip::Sheet { from } => sheets.iter().any(|s| s.from == from),
            Grip::Edge { from, len } => sheets.iter().any(|s| s.from == from && s.len == len),
        }
    }
}

/// A press in progress: egui's own tap would need a click sense.
#[derive(Clone, Copy, Debug)]
struct Press {
    at: Pos2,
    since: f64,
    grip: Grip,
    moved: bool,
    /// The long press fired: the release does nothing.
    held: bool,
}

/// The strip's session state: its scale and the gesture in hand. Nothing here
/// is saved.
#[derive(Default)]
pub struct Strip {
    /// Points per frame: set when a document opens, held for the session.
    scale: Option<f32>,
    press: Option<Press>,
    /// The cursor last scrolled into view.
    followed: Option<(CutId, u32)>,
    /// The open cell menu: its run's first frame, and where it opened.
    menu: Option<(u32, Pos2)>,
    /// A long press ended this frame.
    lifted: bool,
    /// Where the strip stands this frame; None when it is not shown.
    rect: Option<Rect>,
    /// The first frame of the sheet under the pointer.
    under: Option<u32>,
    pictures: Pictures,
}

impl Strip {
    /// Picture jobs started so far.
    #[cfg(test)]
    pub(crate) fn picture_jobs(&self) -> u32 {
        self.pictures.started
    }

    /// `id` has a picture to show.
    #[cfg(test)]
    pub(crate) fn pictured(&self, id: DrawingId) -> bool {
        self.pictures.held.get(&id).is_some_and(|(_, texture)| texture.is_some())
    }

    /// The content key `id`'s picture was made from.
    #[cfg(test)]
    pub(crate) fn picture_key(&self, id: DrawingId) -> Option<u64> {
        self.pictures.held.get(&id).map(|(key, _)| *key)
    }

    /// Pictures run on their own thread, as they do outside the suite.
    #[cfg(test)]
    pub(crate) fn pictures_aside(&mut self) {
        self.pictures.threaded = true;
    }

    /// A picture job is out.
    #[cfg(test)]
    pub(crate) fn picture_out(&self) -> bool {
        self.pictures.job.is_some()
    }

    /// A new frame: the strip has not shown yet.
    pub fn begin(&mut self) {
        self.rect = None;
        self.under = None;
    }

    /// A document opened: its scale is worked out afresh.
    pub fn opened(&mut self) {
        *self = Self::default();
    }

    /// `pos` is over the strip this frame.
    pub fn holds(&self, pos: Pos2) -> bool {
        self.rect.is_some_and(|r| r.contains(pos))
    }

    /// The first frame of the sheet under the pointer.
    pub fn under(&self) -> Option<u32> {
        self.under
    }

    #[cfg(test)]
    pub fn scale(&self) -> Option<f32> {
        self.scale
    }

    #[cfg(test)]
    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    /// What a press at `at` takes hold of. The gap under the sheets is
    /// nothing: the scrollbar stands over it.
    fn grip_at(sheets: &[Sheet], ruler: Ruler, top: f32, at: Pos2) -> Option<Grip> {
        if at.y < top + BAR_H {
            return Some(Grip::Bar);
        }
        if at.y >= top + BAR_H + SHEET_H {
            return None;
        }
        let frame = ruler.frame(at.x);
        Some(match sheets.iter().find(|s| frame < s.from + s.len) {
            Some(s) if at.x >= ruler.x(s.from + s.len) - EDGE => Grip::Edge { from: s.from, len: s.len },
            Some(s) => Grip::Sheet { from: s.from },
            None => Grip::Bar,
        })
    }

    /// The length an edge drag asks for, with the pointer at `here`.
    fn pulled(press: &Press, len: u32, here: Pos2, scale: f32) -> u32 {
        (len as f32 + (here.x - press.at.x) / scale).round().max(1.0) as u32
    }

    /// One frame's input. A press and its release can land in one frame on
    /// a slow one, so the raw events drive it, in order. A press counts only
    /// when the surface is what the pointer is on: `hovered` and not
    /// `contains_pointer`, which is also true over the scrollbar on top of it.
    fn input(
        &mut self,
        ui: &mut Ui,
        state: &mut AppState,
        resp: &egui::Response,
        sheets: &[Sheet],
        ruler: Ruler,
        gesture_safe: bool,
    ) {
        let (time, pointer, stayed) = ui.input(|i| (i.time, i.pointer.latest_pos(), i.pointer.primary_down()));
        let buttons: Vec<(Pos2, bool)> = ui.input(|i| {
            let events = i.events.iter().filter_map(|e| match e {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } => {
                    Some((*pos, *pressed))
                }
                _ => None,
            });
            events.collect()
        });
        // Before the buttons: a release this frame must not write onto a run
        // an undo or a paste has taken since the last one.
        if self.press.is_some_and(|p| !p.grip.stands(sheets)) {
            self.press = None;
        }
        for (at, pressed) in buttons {
            if pressed {
                if resp.hovered()
                    && let Some(grip) = Self::grip_at(sheets, ruler, resp.rect.top(), at)
                {
                    self.press = Some(Press { at, since: time, grip, moved: false, held: false });
                }
            } else if let Some(press) = self.press.take() {
                self.release(state, press, at, ruler, gesture_safe);
            }
        }
        if !stayed {
            self.press = None;
        }
        if let Some(press) = &mut self.press {
            let here = pointer.unwrap_or(press.at);
            press.moved |= (here - press.at).length() > SLOP;
            if press.moved {
                if press.grip == Grip::Bar && gesture_safe {
                    state.goto(ruler.frame(here.x));
                }
            } else if !press.held && press.grip != Grip::Bar {
                let left = LONG_PRESS - (time - press.since);
                if left <= 0.0 {
                    press.held = true;
                    let from = match press.grip {
                        Grip::Sheet { from } | Grip::Edge { from, .. } => from,
                        Grip::Bar => 0,
                    };
                    self.open_menu(ui, resp, from, press_at(self.press.as_ref()));
                } else {
                    ui.ctx().request_repaint_after_secs(left as f32);
                }
            }
        }
        // Right click, as the sheet's cells take it.
        if resp.hovered()
            && ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Secondary))
            && let Some(at) = pointer
            && let Some(Grip::Sheet { from } | Grip::Edge { from, .. }) =
                Self::grip_at(sheets, ruler, resp.rect.top(), at)
        {
            self.open_menu(ui, resp, from, at);
        }
        if resp.contains_pointer() {
            let zoom = ui.input(|i| i.zoom_delta());
            if zoom != 1.0 {
                self.scale = Some((ruler.scale * zoom).clamp(NARROWEST, WIDEST));
            }
        }
    }

    fn open_menu(&mut self, ui: &Ui, resp: &egui::Response, from: u32, at: Pos2) {
        self.menu = Some((from, at));
        egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(resp));
    }

    /// The press ended at `at`: a tap lands, an edge commits, once.
    fn release(&mut self, state: &mut AppState, press: Press, at: Pos2, ruler: Ruler, gesture_safe: bool) {
        let moved = press.moved || (at - press.at).length() > SLOP;
        self.lifted = press.held;
        if press.held || (moved && matches!(press.grip, Grip::Bar | Grip::Sheet { .. })) {
            return;
        }
        if !gesture_safe {
            state.refuse(FINISH_STROKE);
            return;
        }
        match press.grip {
            Grip::Edge { from, len } if moved => {
                state.set_run_length(from, Self::pulled(&press, len, at, ruler.scale));
            }
            Grip::Sheet { from } | Grip::Edge { from, .. } => tap(state, from),
            Grip::Bar => {
                let frame = ruler.frame(press.at.x);
                let from = sheets(state).iter().find(|s| frame < s.from + s.len).map_or(frame, |s| s.from);
                tap(state, from);
            }
        }
    }

    /// The sheets as they stand under an edge drag: the strip follows the
    /// hand, and nothing is written until the release.
    fn live(&self, mut sheets: Vec<Sheet>, here: Option<Pos2>, scale: f32) -> Vec<Sheet> {
        let Some(press) = self.press.filter(|p| p.moved) else {
            return sheets;
        };
        let (Grip::Edge { from, len }, Some(here)) = (press.grip, here) else {
            return sheets;
        };
        // A paste in the same frame can take the run, as an undo can.
        if !press.grip.stands(&sheets) {
            return sheets;
        }
        let to = Self::pulled(&press, len, here, scale);
        let shift = to as i64 - len as i64;
        for sheet in &mut sheets {
            if sheet.from == from {
                sheet.len = to;
            } else if sheet.from > from {
                sheet.from = (sheet.from as i64 + shift).max(0) as u32;
            }
        }
        sheets
    }

    fn surface(&mut self, ui: &mut Ui, state: &mut AppState, scale: f32, gesture_safe: bool) {
        let count = state.frame_count();
        let wide = (count as f32 * scale).max(ui.available_width());
        let (rect, resp) = ui.allocate_exact_size(vec2(wide, STRIP_H), Sense::drag());
        let ruler = Ruler { left: rect.left(), scale, last: count - 1 };
        let (runs, made) = (sheets(state), state.engine.position());
        self.input(ui, state, &resp, &runs, ruler, gesture_safe);
        crate::xsheet_panel::clipboard_events(ui, state, gesture_safe);
        // An edit above may have changed the runs and the cut's length.
        let runs = if state.engine.position() == made { runs } else { sheets(state) };
        let ruler = Ruler { last: state.frame_count() - 1, ..ruler };
        let here = ui.input(|i| i.pointer.latest_pos());
        let sheets = self.live(runs, here, scale);
        let hovered = here.filter(|_| resp.hovered());
        let top = rect.top() + BAR_H;
        let under = hovered.filter(|p| (top..top + SHEET_H).contains(&p.y)).map(|p| ruler.frame(p.x));
        let sheet = under.and_then(|f| sheets.iter().find(|s| f < s.from + s.len).copied());
        self.under = sheet.map(|s| s.from);
        if let (Some(s), Some(p)) = (sheet, hovered)
            && p.x >= ruler.x(s.from + s.len) - EDGE
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        self.follow(ui, state, rect, ruler);
        let drawings = tally(&sheets);
        let clip = ui.clip_rect();
        let shows = |s: &Sheet| match s.shows {
            Exposure::Drawing(id) => {
                let at = sheet_rect(ruler, top, s);
                (at.width() >= PICTURED && at.intersects(clip)).then_some(id)
            }
            Exposure::Empty => None,
        };
        let shown: Vec<DrawingId> = sheets.iter().filter_map(shows).collect();
        self.pictures.refresh(ui.ctx(), state, &shown, gesture_safe);
        paint(ui, state, rect, ruler, &sheets, &drawings, self.under, &self.pictures);
        speak_sheets(ui, state, &resp, ruler, top, &sheets, &drawings);
        if self.press.is_none()
            && self.menu.is_none()
            && let Some(s) = sheet
        {
            let words = card(state, &drawings, &s);
            plate::card_above(resp.clone(), pos2(ruler.x(s.from), rect.top()), || words);
        }
        self.menu_ui(state, &resp);
    }

    /// GM14's rule: a cursor moved by a key, a jump or playback is scrolled
    /// into view; a scroll by hand stays put.
    fn follow(&mut self, ui: &mut Ui, state: &AppState, rect: Rect, ruler: Ruler) {
        let at = (state.view.cut, state.view.frame);
        if self.followed != Some(at) {
            self.followed = Some(at);
            let x = ruler.x(at.1);
            ui.scroll_to_rect(Rect::from_min_size(pos2(x, rect.top()), vec2(ruler.scale, rect.height())), None);
        }
    }

    fn menu_ui(&mut self, state: &mut AppState, resp: &egui::Response) {
        let lifted = std::mem::take(&mut self.lifted);
        let Some((frame, at)) = self.menu else {
            return;
        };
        let column = state.view.active_column;
        let popup = egui::Popup::menu(resp)
            .at_position(at)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        let shown = popup.show(|ui| crate::xsheet_panel::cell_menu(ui, state, Some(column), frame));
        // A long press lifted inside egui's click time reads as a click
        // outside the menu it just opened.
        if lifted {
            egui::Popup::open_id(&resp.ctx, egui::Popup::default_response_id(resp));
        } else if shown.is_none() {
            self.menu = None;
        }
    }
}

fn press_at(press: Option<&Press>) -> Pos2 {
    press.map_or(Pos2::ZERO, |p| p.at)
}

/// A tap: the cursor lands on the run's first frame, never on a held frame.
fn tap(state: &mut AppState, from: u32) {
    let column = state.view.active_column;
    state.select_cells((column, from), column, from);
}

/// What the strip's sheet is called: its number and its frames.
fn sheet_name(drawings: &HashMap<DrawingId, Tally>, sheet: &Sheet) -> String {
    let frames = span(sheet);
    match sheet.shows {
        Exposure::Empty => format!("Empty sheet, {frames}"),
        Exposure::Drawing(id) => format!("Drawing {}, {frames}", number_of(drawings, id)),
    }
}

fn number_of(drawings: &HashMap<DrawingId, Tally>, id: DrawingId) -> u32 {
    drawings.get(&id).map_or(0, |t| t.number)
}

/// A sheet's hover card: the number and the frames it shows, its drawing's
/// own name in the middle, and how a sheet's edge works.
fn card(state: &AppState, drawings: &HashMap<DrawingId, Tally>, sheet: &Sheet) -> String {
    let frames = span(sheet);
    let hint = "Drag the right edge to make it longer or shorter.";
    match sheet.shows {
        Exposure::Empty => format!("{}\n{hint}", sheet_name(drawings, sheet)),
        Exposure::Drawing(id) => {
            let number = number_of(drawings, id);
            let name = state.cut().drawing(id).map_or("", |d| d.name.as_str());
            let used = drawings.get(&id).is_some_and(|t| t.uses > 1);
            let twice = if used { "\nUsed twice" } else { "" };
            format!("Drawing {number} ({name}), {frames}{twice}\n{hint}")
        }
    }
}

/// The strip in the band's room. Answers where it stands, for the routing.
pub fn show(ui: &mut Ui, state: &mut AppState, strip: &mut Strip, gesture_safe: bool) -> Rect {
    let wide = ui.available_width();
    let frames = state.frame_count();
    let scale = *strip.scale.get_or_insert((wide / frames as f32).clamp(DEFAULT_FLOOR, WIDEST));
    let scroll = egui::ScrollArea::horizontal()
        .id_salt("strip")
        .auto_shrink([false, false])
        .show(ui, |ui| strip.surface(ui, state, scale, gesture_safe));
    plate::note_viewport(ui, scroll.inner_rect, [true, false]);
    strip.rect = Some(scroll.inner_rect);
    scroll.inner_rect
}

fn sheet_rect(ruler: Ruler, top: f32, sheet: &Sheet) -> Rect {
    let width = (sheet.len as f32 * ruler.scale - 2.0).max(1.0);
    Rect::from_min_size(pos2(ruler.x(sheet.from), top), vec2(width, SHEET_H))
}

const STRIP_NEXT: Target = Target::Named("strip.next");

/// AU21: `strip.next`: where the first sheet stands whose drawing has ink and
/// no color (spec 5.5). `tap` lands on a run's first frame, so Fill never meets
/// a held one.
fn next_to_color(state: &AppState, sheets: &[Sheet]) -> Option<u32> {
    #[cfg(test)]
    plate::SCAN_COST.with(|c| c.set(c.get() + 1));
    let wants = |id| state.cut().drawing(id).is_some_and(|d| drawing_has_art(d) && !color_layer_art(d));
    let bare = |s: &&Sheet| matches!(s.shows, Exposure::Drawing(id) if wants(id));
    sheets.iter().find(bare).map(|s| s.from)
}

/// Each sheet on screen is one accessible control (spec 7.2): the surface
/// alone would be one large widget.
fn speak_sheets(
    ui: &mut Ui,
    state: &mut AppState,
    resp: &egui::Response,
    ruler: Ruler,
    top: f32,
    sheets: &[Sheet],
    drawings: &HashMap<DrawingId, Tally>,
) {
    let ppp = ui.ctx().pixels_per_point();
    let clip = ui.clip_rect();
    let next = if plate::wishes(ui.ctx(), STRIP_NEXT) { next_to_color(state, sheets) } else { None };
    for sheet in sheets {
        let rect = sheet_rect(ruler, top, sheet);
        if !rect.intersects(clip) {
            continue;
        }
        plate::note_strip_sheet(ui, rect);
        let id = resp.id.with(("sheet", sheet.from));
        let at = rect.intersect(clip);
        if next == Some(sheet.from) {
            plate::mark(ui, at, STRIP_NEXT);
        }
        let name = sheet_name(drawings, sheet);
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(accesskit::Role::Button);
            node.set_label(name);
            node.add_action(accesskit::Action::Click);
            node.set_bounds(accesskit::Rect {
                x0: (at.min.x * ppp) as f64,
                y0: (at.min.y * ppp) as f64,
                x1: (at.max.x * ppp) as f64,
                y1: (at.max.y * ppp) as f64,
            });
        });
        if ui.input(|i| i.has_accesskit_action_request(id, accesskit::Action::Click)) {
            tap(state, sheet.from);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint(
    ui: &Ui,
    state: &AppState,
    rect: Rect,
    ruler: Ruler,
    sheets: &[Sheet],
    drawings: &HashMap<DrawingId, Tally>,
    under: Option<u32>,
    pictures: &Pictures,
) {
    let painter = ui.painter();
    let clip = ui.clip_rect();
    let bar = Rect::from_min_size(rect.min, vec2(rect.width(), BAR_H));
    plate::peg_bar(painter, bar);
    let fps = state.fps();
    let seen = ruler.frame(clip.left())..=ruler.frame(clip.right()).saturating_add(1);
    // The second ticks, white; a label in the bar's own ink wherever the
    // last one leaves it room.
    let mut free = f32::MIN;
    for frame in seen {
        if crate::runs::rule_tier(frame, fps) != Some(2) || frame == 0 {
            continue;
        }
        let x = ruler.x(frame);
        painter.line_segment([pos2(x, bar.top()), pos2(x, bar.bottom())], Stroke::new(1.0, plate::ON_FILL));
        if x >= free {
            let words = format!("{} s", frame / fps.max(1));
            let font = plate::Role::Small.font();
            let label = painter.text(pos2(x + 3.0, bar.center().y), Align2::LEFT_CENTER, words, font, plate::STEEL_INK);
            free = label.right() + 6.0;
        }
    }
    if let Some((lo, hi)) = marked_range(state) {
        let band = Rect::from_min_max(pos2(ruler.x(lo), bar.bottom() - 3.0), pos2(ruler.x(hi + 1), bar.bottom()));
        painter.rect_filled(band, 0.0, plate::BLUE);
    }
    let top = bar.bottom();
    let frame = state.view.frame;
    for sheet in sheets {
        let at = sheet_rect(ruler, top, sheet);
        if !at.intersects(clip) {
            continue;
        }
        paint_sheet(painter, at, sheet, drawings, under == Some(sheet.from), pictures);
        if frame >= sheet.from && frame < sheet.from + sheet.len {
            let round = CornerRadius { nw: 0, ne: 0, sw: 4, se: 4 };
            painter.rect_stroke(at, round, Stroke::new(2.0, plate::BLUE_DEEP), StrokeKind::Inside);
            edge_grip(painter, at);
        }
    }
    let cut = state.cut();
    if cut.xsheet.columns.len() > 1
        && let Some(column) = cut.xsheet.column(state.view.active_column)
    {
        track_chip(ui, pos2(clip.right() - 4.0, bar.center().y), &column.name);
    }
    // Over everything: the playhead, its cap in a white ring.
    let x = ruler.x(frame) + ruler.scale * 0.5;
    let foot = top + SHEET_H;
    painter.line_segment([pos2(x, bar.top()), pos2(x, foot)], Stroke::new(2.0, plate::BLUE_DEEP));
    let cap = pos2(x, bar.top() + 6.0);
    painter.circle(cap, 4.5, plate::BLUE_DEEP, Stroke::new(1.5, plate::ON_FILL));
}

/// From the second track, the strip says which one it shows: at the visible
/// bar's right end, where the playhead's cap does not start. `at` is the
/// chip's right end.
fn track_chip(ui: &Ui, at: Pos2, name: &str) {
    let painter = ui.painter();
    let font = plate::Role::Small.font();
    let galley = painter.layout_no_wrap(name.to_string(), font, plate::INK);
    let size = vec2(galley.size().x + 10.0, 14.0);
    let chip = Rect::from_min_size(at - vec2(size.x, 7.0), size);
    painter.rect_filled(chip, plate::R_SHEET, plate::PAPER);
    painter.galley(pos2(chip.left() + 5.0, chip.center().y - galley.size().y * 0.5), galley, plate::INK);
    // Hover only: a press here is still the bar's.
    let spoken = format!("Track {name}");
    let resp = ui.interact(chip, ui.id().with("track_chip"), Sense::hover());
    plate::speak(&resp, egui::WidgetType::Label, None, &spoken);
    resp.card(format!("The strip shows {spoken}."));
}

/// The current sheet's edge says it can be pulled: two short ticks, where
/// they leave its number room.
fn edge_grip(painter: &egui::Painter, at: Rect) {
    if at.width() < 2.0 * NUMBERED {
        return;
    }
    let ink = Stroke::new(1.0, plate::BLUE_DEEP);
    let (mid, right) = (at.center().y, at.right() - 5.0);
    for x in [right, right - 3.0] {
        painter.line_segment([pos2(x, mid - 5.0), pos2(x, mid + 5.0)], ink);
    }
}

/// Where a picture of `size` stands on a sheet: under the peg hole, at its own
/// shape.
pub(crate) fn picture_rect(at: Rect, size: egui::Vec2) -> Rect {
    let room = vec2(at.width() - 2.0 * PICTURE_SIDE, SHEET_H - PICTURE_TOP - PICTURE_FOOT);
    let fit = (room.x / size.x).min(room.y / size.y);
    Rect::from_min_size(pos2(at.left() + PICTURE_SIDE, at.top() + PICTURE_TOP), size * fit)
}

/// A sheet's number on its picture, where the sheet has no room beside it: on
/// the left, clear of `Used twice`'s mark and the current sheet's grip.
fn number_chip(painter: &egui::Painter, picture: Rect, number: u32) {
    let galley = painter.layout_no_wrap(number.to_string(), plate::Role::Small.font(), plate::INK2);
    let size = galley.size() + vec2(4.0, 0.0);
    let chip = Rect::from_min_size(picture.left_bottom() - vec2(0.0, size.y), size);
    painter.rect_filled(chip, plate::R_SHEET, plate::PAPER);
    painter.galley(chip.min + vec2(2.0, 0.0), galley, plate::INK2);
}

fn paint_sheet(
    painter: &egui::Painter,
    at: Rect,
    sheet: &Sheet,
    drawings: &HashMap<DrawingId, Tally>,
    under: bool,
    pictures: &Pictures,
) {
    let round = CornerRadius { nw: 0, ne: 0, sw: 4, se: 4 };
    let Exposure::Drawing(id) = sheet.shows else {
        if under {
            painter.rect_filled(at, round, plate::HOVER);
        }
        dashed(painter, at);
        return;
    };
    // The sheet under the pointer is the one Delete takes: it shows.
    painter.rect_filled(at, round, if under { plate::HOVER } else { plate::PAPER });
    plate::peg_hole(painter, pos2(at.center().x, at.top() + 6.0));
    let twice = drawings.get(&id).is_some_and(|t| t.uses > 1);
    let worded = twice && at.width() >= WORDED;
    let texture = pictures.held.get(&id).and_then(|(_, texture)| texture.as_ref()).filter(|_| at.width() >= PICTURED);
    let shown = texture.map(|texture| {
        let shown = picture_rect(at, texture.size_vec2());
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(texture.id(), shown, uv, plate::ON_FILL);
        painter.rect_stroke(shown, 0.0, Stroke::new(1.0, plate::INK2), StrokeKind::Outside);
        shown
    });
    if at.width() >= NUMBERED {
        // Beside the badge's words when they show, in the middle when not,
        // and on the picture when the sheet holds no room for both.
        let number = number_of(drawings, id);
        let at_rest = match shown {
            Some(picture) if at.right() - picture.right() < NUMBERED => {
                number_chip(painter, picture, number);
                None
            }
            Some(picture) => Some((picture.right() + 6.0, Align2::LEFT_CENTER)),
            None if worded => Some((at.left() + 8.0, Align2::LEFT_CENTER)),
            None => Some((at.center().x, Align2::CENTER_CENTER)),
        };
        if let Some((x, align)) = at_rest {
            let font = plate::Role::Chrome.font();
            painter.text(pos2(x, at.top() + SHEET_ROW), align, number.to_string(), font, plate::INK2);
        }
    }
    if twice {
        twice_badge(painter, at, worded);
    }
}

/// `Used twice`, on the number's row, or its mark where the words do not fit.
fn twice_badge(painter: &egui::Painter, at: Rect, worded: bool) {
    if worded {
        let (font, end) = (plate::Role::Small.font(), pos2(at.right() - 12.0, at.top() + SHEET_ROW));
        painter.text(end, Align2::RIGHT_CENTER, "Used twice", font, plate::BLUE_DEEP);
    } else {
        painter.circle_filled(at.right_bottom() - vec2(6.0, 5.0), 2.5, plate::BLUE_DEEP);
    }
}

/// An empty run's outline, dashed so it never reads as a sheet of paper, and
/// inside the current sheet's 2 pt outline so it still shows there.
fn dashed(painter: &egui::Painter, at: Rect) {
    let at = at.shrink(3.5);
    let path = [at.left_top(), at.right_top(), at.right_bottom(), at.left_bottom(), at.left_top()];
    let ink: Color32 = plate::INK3;
    painter.extend(egui::Shape::dashed_line(&path, Stroke::new(1.0, ink), 4.0, 3.0));
}
