//! THE FLOATS (AU4, spec 4.5): the five panels the rail opens over the canvas.
//! Each is an `Area` at `Order::Middle`, kept to its host's rect, so opening
//! one never moves the art. Not saved, and never opened during a stroke: a
//! request waits for pen-up. The bodies stay where their fields are, in
//! canvas.rs and xsheet_panel.rs; this file holds the frame, the size and
//! scroll rules, and who is open.

use eframe::egui;
use egui::{Pos2, Rect, Vec2, pos2, vec2};

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
    let id = egui::Id::new(("float", f.heading()));
    let tall = ctx.memory(|m| m.area_rect(id)).map_or(size.y, |r| r.height().min(size.y));
    let pos = f.pos(host, rail_top, vec2(size.x, tall));
    let head = Head { words: f.heading(), role: plate::Role::Heading, rule: false };
    let (shown, close) = frame(ctx, head, pos, size, host, |ui| body(ui, f, args));
    if close {
        args.floats.close(f);
    }
    if let Some(rects) = args.pane_rects.as_deref_mut() {
        rects.push((f.pane(), shown.rect, shown.layer_id));
    }
}

/// AU6: where the Start float stands in a pane of `pane`, and its size (spec
/// 4.5): top right, and wide enough only to stay past the pane's centre.
pub fn start_place(pane: Rect) -> (Pos2, Vec2) {
    let w = (pane.width() / 2.0 - 20.0).clamp(240.0, 320.0);
    let size = vec2(w, (pane.height() - 24.0).min(460.0));
    (pos2(pane.right() - INSET - size.x, pane.top() + INSET), size)
}

/// A float's heading: its words, their type role, and a rule under them.
pub struct Head<'a> {
    pub words: &'a str,
    pub role: plate::Role,
    pub rule: bool,
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
    let mut close = false;
    let heading = head.words;
    let shown = egui::Area::new(egui::Id::new(("float", heading)))
        .order(egui::Order::Middle)
        .fixed_pos(pos)
        .constrain_to(host)
        .fade_in(false)
        .show(ctx, |ui| {
            // An `Area` holds its ui to the size it had last frame, and the
            // first of those is egui's 400: the height is the float's to say.
            ui.set_max_height(size.y);
            plate::island_frame(ui.style(), MARGIN).show(ui, |ui| {
                ui.set_width(inner);
                ui.horizontal(|ui| {
                    let words = egui::RichText::new(heading).font(head.role.font());
                    ui.label(words.color(plate::INK));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = plate::close_key(ui, &format!("Close {heading}")).clicked();
                    });
                });
                if head.rule {
                    ui.add(egui::Separator::default().spacing(2.0));
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
                bar.floating_allocated_width = GUTTER;
                let widgets = &mut ui.visuals_mut().widgets;
                let ink = std::mem::replace(&mut widgets.inactive.fg_stroke.color, plate::INK3);
                let scroll = egui::ScrollArea::vertical()
                    .id_salt(("float_scroll", heading))
                    .scroll_source(egui::scroll_area::ScrollSource::ALL)
                    .max_height((size.y - spent).max(40.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.visuals_mut().widgets.inactive.fg_stroke.color = ink;
                        ui.set_width(ui.available_width());
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
