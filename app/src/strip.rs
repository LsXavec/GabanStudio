//! THE SIMPLE TIMELINE (AU5, spec 7.2): a second view of the active track's
//! exposures, along the band. It keeps no data: it reads `Column::runs` and
//! writes through the verbs the Timing sheet shares.
//!
//! One surface takes every gesture, with its own tap and its own long press:
//! a press on anything that also senses clicks loses its capture to egui's
//! long touch at 0.817 s under a pen.

use std::collections::HashMap;

use anim_core::ids::{CutId, DrawingId};
use anim_core::xsheet::Exposure;
use eframe::egui;
use egui::accesskit;
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::doc::{AppState, FINISH_STROKE};
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

/// One sheet: a run of the active track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sheet {
    pub from: u32,
    pub len: u32,
    pub shows: Exposure,
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
}

impl Strip {
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
        paint(ui, state, rect, ruler, &sheets, &drawings, self.under);
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
    for sheet in sheets {
        let rect = sheet_rect(ruler, top, sheet);
        if !rect.intersects(clip) {
            continue;
        }
        plate::note_strip_sheet(ui, rect);
        let id = resp.id.with(("sheet", sheet.from));
        let at = rect.intersect(clip);
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

fn paint(
    ui: &Ui,
    state: &AppState,
    rect: Rect,
    ruler: Ruler,
    sheets: &[Sheet],
    drawings: &HashMap<DrawingId, Tally>,
    under: Option<u32>,
) {
    let painter = ui.painter();
    let clip = ui.clip_rect();
    let bar = Rect::from_min_size(rect.min, vec2(rect.width(), BAR_H));
    painter.rect_filled(bar, 0.0, plate::STEEL);
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
            let font = FontId::proportional(12.0);
            let label = painter.text(pos2(x + 3.0, bar.center().y), Align2::LEFT_CENTER, words, font, plate::STEEL_INK);
            free = label.right() + 6.0;
        }
    }
    // The play range, when it is not the whole cut.
    let (lo, hi) = state.play_range();
    if (lo, hi) != (0, ruler.last) {
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
        paint_sheet(painter, at, sheet, drawings, under == Some(sheet.from));
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
    let font = FontId::proportional(12.0);
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

fn paint_sheet(painter: &egui::Painter, at: Rect, sheet: &Sheet, drawings: &HashMap<DrawingId, Tally>, under: bool) {
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
    painter.circle_filled(pos2(at.center().x, at.top() + 6.0), 2.5, plate::DESK);
    let twice = drawings.get(&id).is_some_and(|t| t.uses > 1);
    let worded = twice && at.width() >= WORDED;
    if at.width() >= NUMBERED {
        // Beside the badge's words when they show, in the middle when not.
        let (x, align) = if worded { (at.left() + 8.0, Align2::LEFT_CENTER) } else { (at.center().x, Align2::CENTER_CENTER) };
        let font = FontId::proportional(14.0);
        painter.text(pos2(x, at.top() + SHEET_ROW), align, number_of(drawings, id).to_string(), font, plate::INK2);
    }
    if twice {
        twice_badge(painter, at, worded);
    }
}

/// `Used twice`, on the number's row, or its mark where the words do not fit.
fn twice_badge(painter: &egui::Painter, at: Rect, worded: bool) {
    if worded {
        let (font, end) = (FontId::proportional(12.0), pos2(at.right() - 12.0, at.top() + SHEET_ROW));
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
