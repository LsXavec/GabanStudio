//! THE FLOATS (AU4, spec 4.5): the five panels the rail opens over the canvas.
//! Each is an `Area` at `Order::Middle`, kept to its host's rect, so opening
//! one never moves the art. Not saved, and never opened during a stroke: a
//! request waits for pen-up. The bodies stay where their fields are, in
//! canvas.rs and xsheet_panel.rs; this file holds the frame, the size and
//! scroll rules, and who is open.

use eframe::egui;
use egui::{Pos2, Rect, Vec2, pos2, vec2};

use crate::lessons::Target;
use crate::plate::Card;
use crate::rail::OverlayArgs;
use crate::{Pane, plate, vocab, xsheet_panel};

/// Where the left floats stand: beside the rail.
pub const LEFT_X: f32 = 76.0;
/// A float's distance from its host's edges.
const INSET: f32 = 12.0;
/// A float's inner margin (spec 2.6).
const MARGIN: i8 = 12;
/// `auto`: a float as tall as its body, capped by its host.
const AUTO: f32 = f32::INFINITY;
/// The scroll handle at rest, and the gutter it keeps from the body: as wide
/// as the bar a hover widens it to, so it lies on no control.
const HANDLE_W: f32 = 4.0;
pub const GUTTER: f32 = 10.0;
/// The Start float's rule: the peg bar, cut down to a line (spec 2.1).
const PEG_RULE_H: f32 = 8.0;
/// The `Area` of the Pocket tier's `…` popover, which rail.rs draws.
pub const POPOVER: &str = "rail_popover";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Float {
    Layers,
    Brushes,
    Color,
    ToolOptions,
    CanvasOptions,
}

impl Float {
    #[cfg(test)]
    pub const ALL: [Float; 5] =
        [Float::Layers, Float::Brushes, Float::Color, Float::ToolOptions, Float::CanvasOptions];

    pub fn heading(self) -> &'static str {
        match self {
            Float::Layers => vocab::LAYERS,
            Float::Brushes => vocab::BRUSHES,
            Float::Color => vocab::STEP_COLOR,
            Float::ToolOptions => vocab::TOOL_OPTIONS,
            Float::CanvasOptions => vocab::CANVAS_OPTIONS,
        }
    }

    /// The table's size, before the host caps it.
    fn table(self) -> Vec2 {
        match self {
            Float::Layers => vec2(240.0, 320.0),
            Float::Brushes => vec2(280.0, 420.0),
            Float::Color => vec2(280.0, 300.0),
            Float::ToolOptions | Float::CanvasOptions => vec2(240.0, AUTO),
        }
    }

    /// The most this float may take of `host`.
    pub fn size_in(self, host: Rect) -> Vec2 {
        let table = self.table();
        vec2(table.x.min(host.width() - 88.0), table.y.min(host.height() - 24.0))
    }

    /// Where its top left stands. A left float's top is the rail's, kept
    /// inside the host.
    fn pos(self, host: Rect, rail_top: f32, size: Vec2) -> Pos2 {
        if self == Float::Layers {
            return pos2(host.right() - INSET - size.x, host.top() + INSET);
        }
        let last = host.bottom() - INSET - size.y.min(host.height() - 24.0);
        pos2(host.left() + LEFT_X, rail_top.min(last).max(host.top() + INSET))
    }

    fn pane(self) -> Pane {
        if self == Float::Layers { Pane::Layers } else { Pane::Canvas }
    }
}

/// Who is open. One float per side: Layers on the right, the rest on the left.
#[derive(Default)]
pub struct Floats {
    left: Option<Float>,
    layers: bool,
    /// Asked for during a stroke: opens at pen-up.
    waiting: Option<Float>,
    /// The Pocket tier's `…` popover.
    pub more: bool,
    /// Layers was tapped with a Layers tab on screen. The dock is the
    /// Editor's, so it brings the tab forward after the panes are drawn.
    pub focus_layers: bool,
    /// AU20: Start was just opened: a left float under it makes way, where a
    /// left float opened over it closes it.
    pub start_fresh: bool,
    /// AU20: the `Area` of what last stood on the left, drawn.
    left_met: Option<egui::Id>,
}

impl Floats {
    pub fn is_open(&self, f: Float) -> bool {
        match f {
            Float::Layers => self.layers,
            _ => self.left == Some(f),
        }
    }

    /// Open `f`, now or at pen-up: `busy` says a stroke is live.
    pub fn open(&mut self, f: Float, busy: bool) {
        if busy {
            self.waiting = Some(f);
            return;
        }
        match f {
            Float::Layers => self.layers = true,
            _ => {
                self.left = Some(f);
                self.more = false;
            }
        }
    }

    /// The `…` popover stands where a left float does, so the two trade
    /// places.
    pub fn toggle_more(&mut self) {
        self.more = !self.more;
        if self.more {
            self.left = None;
        }
    }

    pub fn close(&mut self, f: Float) {
        match f {
            Float::Layers => self.layers = false,
            _ if self.left == Some(f) => self.left = None,
            _ => {}
        }
    }

    pub fn toggle(&mut self, f: Float, busy: bool) {
        if self.is_open(f) {
            self.close(f);
        } else {
            self.open(f, busy);
        }
    }

    /// The pen is up: a float that waited for it opens.
    pub fn settle(&mut self, busy: bool) {
        if !busy && let Some(f) = self.waiting.take() {
            self.open(f, false);
        }
    }

    /// AU20: what stands on the left, as this frame drew it, and whether that
    /// is news: asked once a frame, after the panes. An `Area` keeps its rect
    /// when it stops being drawn, and `more` outlives the Pocket tier, so only
    /// a layer with widgets this frame counts. What has not been drawn yet
    /// stays news until it is.
    pub fn left_drawn(&mut self, ctx: &egui::Context) -> (Option<Rect>, bool) {
        let area = match self.left {
            Some(f) => Some(area_id(f.heading())),
            None if self.more => Some(egui::Id::new(POPOVER)),
            None => None,
        };
        let drawn = |id: &egui::Id| {
            let layer = egui::LayerId::new(egui::Order::Middle, *id);
            ctx.viewport(|v| v.this_pass.widgets.get_layer(layer).next().is_some())
        };
        let rect = area.filter(drawn).and_then(|id| ctx.memory(|m| m.area_rect(id)));
        let news = rect.is_some() && area != self.left_met;
        if rect.is_some() || area.is_none() {
            self.left_met = area;
        }
        (rect, news)
    }

    /// AU20: the left float, or the `…` popover in its place, makes way.
    pub fn close_left(&mut self) {
        self.left = None;
        self.more = false;
    }
}

/// The body of `f`, inside its scroll.
fn body(ui: &mut egui::Ui, f: Float, args: &mut OverlayArgs) {
    match f {
        Float::Layers => {
            let gesture_safe = !args.canvas.stroke_active();
            xsheet_panel::cel_layers_strip(ui, args.state, gesture_safe);
        }
        Float::Brushes => args.canvas.brush_rail_ui(ui, args.presets, args.state),
        Float::Color => args.canvas.color_ui(ui, args.state),
        Float::ToolOptions => args.canvas.tool_options_ui(ui, args.state),
        Float::CanvasOptions => args.canvas.lightbox_rail_ui(ui, args.state),
    }
}

/// One float: its heading stays put, its body scrolls under it.
fn show_float(ctx: &egui::Context, args: &mut OverlayArgs, f: Float, host: Rect, rail_top: f32) {
    let size = f.size_in(host);
    if size.x < 120.0 || size.y < 80.0 {
        return;
    }
    // The first frame of an `auto` float knows nothing of its height: the
    // host's cap stands in until the Area has one.
    let id = area_id(f.heading());
    let tall = ctx.memory(|m| m.area_rect(id)).map_or(size.y, |r| r.height().min(size.y));
    let pos = f.pos(host, rail_top, vec2(size.x, tall));
    let head = Head { id: f.heading(), words: f.heading(), role: plate::Role::Heading, start: false, page: 0, top: false, latch: None };
    let (shown, close) = frame(ctx, head, pos, size, host, |ui| body(ui, f, args));
    if close {
        args.floats.close(f);
    }
    plate::mark_on(ctx, shown.layer_id, shown.rect, Target::Float(f));
    if let Some(rects) = args.pane_rects.as_deref_mut() {
        rects.push((f.pane(), shown.rect, shown.layer_id));
    }
}

/// AU20: where the Start float stands, dead centre on the dock rect `pane`, and
/// its size (spec 4.1). Each page fills it, so no page and no `More sizes`
/// moves a row under the pointer.
pub fn start_place(pane: Rect) -> (Pos2, Vec2) {
    let w = (pane.width() - 2.0 * LEFT_X).clamp(240.0, 600.0);
    let h = (pane.height() - 2.0 * INSET).min(300.0);
    (pane.center() - vec2(w, h) / 2.0, vec2(w, h))
}

/// A float's `Area`.
fn area_id(name: &str) -> egui::Id {
    egui::Id::new(("float", name))
}

/// A float's heading: the name of its `Area`, its words, their type role, and
/// whether it is the Start float's. That one has the peg rule under its words,
/// which stand on the body's text edge, and a scroll reaching into the right
/// margin: the handle lives there, so a body that starts to scroll moves
/// nothing sideways. `page` is the body's tab: each keeps its own scroll, or a
/// tab opens part way down, with its way back out of sight. `top` puts the
/// body back at its head, for a float that has just opened. `latch`, its value,
/// words and hover, stands beside the close key, in reach whatever the body's
/// scroll.
pub struct Head<'a> {
    pub id: &'a str,
    pub words: &'a str,
    pub role: plate::Role,
    pub start: bool,
    pub page: usize,
    pub top: bool,
    pub latch: Option<(&'a mut bool, &'a str, &'a str)>,
}

/// A float's frame, each float's and the Start float's: an `Area` at `Middle`,
/// its heading and close key outside the scroll. Says the area's response,
/// and whether the close key was pressed.
pub fn frame(
    ctx: &egui::Context,
    head: Head,
    pos: Pos2,
    size: Vec2,
    host: Rect,
    body: impl FnOnce(&mut egui::Ui),
) -> (egui::Response, bool) {
    let inner = size.x - 2.0 * f32::from(MARGIN);
    let reach = if head.start { GUTTER } else { 0.0 };
    let margin = egui::Margin { right: MARGIN - reach as i8, ..egui::Margin::same(MARGIN) };
    let mut close = false;
    let heading = head.words;
    let shown = egui::Area::new(area_id(head.id))
        .order(egui::Order::Middle)
        .fixed_pos(pos)
        .constrain_to(host)
        .fade_in(false)
        .show(ctx, |ui| {
            // An `Area` holds its ui to the size it had last frame, and the
            // first of those is egui's 400: the height is the float's to say.
            ui.set_max_height(size.y);
            plate::island_frame(ui.style(), MARGIN).inner_margin(margin).show(ui, |ui| {
                ui.set_width(inner + reach);
                ui.horizontal(|ui| {
                    if head.start {
                        ui.add_space(plate::CARD_PAD);
                    }
                    let words = egui::RichText::new(heading).font(head.role.font());
                    ui.label(words.color(plate::INK));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(reach);
                        close = plate::close_key(ui, &format!("Close {heading}")).clicked();
                        if let Some((on, words, card)) = head.latch {
                            plate::quiet_latch(ui, on, words).card(card);
                        }
                    });
                });
                if head.start {
                    let (rule, _) = ui.allocate_exact_size(vec2(inner, PEG_RULE_H), egui::Sense::hover());
                    plate::peg_bar(ui.painter(), rule);
                    plate::peg_hole(ui.painter(), rule.center());
                } else {
                    ui.add_space(4.0);
                }
                let spent = ui.cursor().top() - ui.max_rect().top() + 2.0 * f32::from(MARGIN);
                // Drag too: a pen has no wheel, and egui's default only starts
                // at the first touch, past the press of the first pull. It has
                // no hover to wake the bar either, so the handle stays in sight,
                // in a gutter of its own: it would lie on the body's right edge.
                // Rounded, and in a lighter ink than the words, or it reads
                // as a stray rule.
                let bar = &mut ui.spacing_mut().scroll;
                bar.dormant_handle_opacity = 0.6;
                bar.floating_width = HANDLE_W;
                bar.floating_allocated_width = GUTTER - reach;
                let widgets = &mut ui.visuals_mut().widgets;
                let ink = std::mem::replace(&mut widgets.inactive.fg_stroke.color, plate::INK3);
                let mut area = egui::ScrollArea::vertical()
                    .id_salt(("float_scroll", heading, head.page))
                    .scroll_source(egui::scroll_area::ScrollSource::ALL)
                    .max_height((size.y - spent).max(40.0))
                    .auto_shrink([false, !head.start]);
                if head.top {
                    area = area.vertical_scroll_offset(0.0);
                }
                let scroll = area.show(ui, |ui| {
                    ui.visuals_mut().widgets.inactive.fg_stroke.color = ink;
                    ui.set_width(ui.available_width() - reach);
                    body(ui);
                });
                plate::note_viewport(ui, scroll.inner_rect, [false, true]);
            });
        });
    (shown.response, close)
}

/// The open floats, over `host`.
pub fn show(ctx: &egui::Context, args: &mut OverlayArgs, host: Rect, rail_top: f32) {
    if let Some(f) = args.floats.left {
        show_float(ctx, args, f, host, rail_top);
    }
    if args.floats.layers {
        show_float(ctx, args, Float::Layers, host, rail_top);
    }
}
