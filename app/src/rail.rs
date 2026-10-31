//! THE RAIL (AU4, spec 4.3): the tools over the canvas, an `Area` at
//! `Order::Middle`. It never moves the paper, and a pen on it never reaches
//! the canvas. `overlay` is the one way in for both canvas hosts, the dock's
//! pane and the OS window; it draws the rail and then the floats.

use eframe::egui;
use egui::{Rect, Sense, Vec2, pos2, vec2};

use crate::canvas::{CanvasTool, CanvasView};
use crate::config::{Action, BrushPreset};
use crate::doc::AppState;
use crate::floats::{self, Float, Floats};
use crate::icons::Icon;
use crate::menus::{hover_tip, hover_tip_with};
use crate::{Pane, plate};

/// The rail's left edge, from its host's.
pub const RAIL_X: f32 = 12.0;
/// What the strip takes from the pane when it opens (spec 4.4).
pub const STRIP_H: f32 = 56.0;
/// The rail's inner margin (spec 2.6).
const MARGIN: i8 = 4;
const KEY_W: f32 = 48.0;
/// A drag of this many points runs the Size key's whole range.
const SIZE_TRAVEL: f32 = 160.0;
/// The true-size bubble, however big the brush.
const BUBBLE_MAX: f32 = 160.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Full,
    Compact,
    Pocket,
    Hidden,
}

impl Tier {
    /// One rule, by reference height, in every room that shows a rail.
    pub fn of(reference: f32, pane_w: f32) -> Tier {
        if reference < 232.0 || pane_w < 120.0 {
            Tier::Hidden
        } else if reference < 372.0 {
            Tier::Pocket
        } else if reference < 492.0 {
            Tier::Compact
        } else {
            Tier::Full
        }
    }

    /// Items plus 8 of margin, every item shown.
    pub fn total(self) -> f32 {
        match self {
            Tier::Full => 476.0,
            Tier::Compact => 356.0,
            Tier::Pocket => 224.0,
            Tier::Hidden => 0.0,
        }
    }

    fn key_size(self, key: Key) -> Vec2 {
        match (self, key) {
            (Tier::Full, Key::Size) => vec2(KEY_W, 84.0),
            (Tier::Full, _) => Vec2::splat(KEY_W),
            (_, Key::Size) => vec2(36.0, 60.0),
            _ => vec2(KEY_W, 36.0),
        }
    }
}

/// The height the pane will have with the strip open. So no rail control
/// moves when the strip opens, and the tier does not flip either.
pub fn reference(pane_h: f32, strip_closed: bool) -> f32 {
    if strip_closed { pane_h - STRIP_H } else { pane_h }
}

/// The top of the rail's box: centred on the reference height, by the tier's
/// whole total, so the island grows down and nothing above moves.
pub fn top_of(host: Rect, reference: f32, tier: Tier) -> f32 {
    host.top() + (reference - tier.total()) / 2.0
}

/// Everything the overlay borrows, from whichever host calls it.
pub struct OverlayArgs<'a> {
    pub canvas: &'a mut CanvasView,
    pub state: &'a mut AppState,
    pub presets: &'a [BrushPreset],
    pub floats: &'a mut Floats,
    /// The GPU route exists; the `full_deck` rule reads it.
    pub raster_available: bool,
    /// Manga's slot 9 holds `Panel`.
    pub manga: bool,
    /// 3D and the Brush maker show no rail.
    pub rail_off: bool,
    /// The strip is closed by its latch here, and may open.
    pub strip_closed: bool,
    /// A Layers tab is on screen: the rail's Layers focuses it.
    pub layers_docked: bool,
    /// The dock's list of what is under the pointer. None in the OS window.
    pub pane_rects: Option<&'a mut Vec<(Pane, Rect, egui::LayerId)>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Key {
    Brush,
    Eraser,
    Select,
    Fill,
    Hand,
    Size,
    Color,
    Layers,
    Flip,
    Panel,
    More,
}

/// Slot 9: Manga's panel tool, else Flip from the second drawing.
fn slot_nine(args: &OverlayArgs) -> Option<Key> {
    if args.manga {
        Some(Key::Panel)
    } else {
        args.state.onion_neighbors().iter().any(Option::is_some).then_some(Key::Flip)
    }
}

/// The rail's keys, top down, and what the Pocket tier's `…` holds. A vector
/// brush on a GPU has no tools of its own: only its width and its colour.
fn keys_of(tier: Tier, args: &OverlayArgs) -> (Vec<Key>, Vec<Key>) {
    use Key::*;
    if !(args.canvas.raster || !args.raster_available) {
        return (vec![Size, Color], Vec::new());
    }
    let nine = slot_nine(args);
    match tier {
        Tier::Pocket => {
            let mut held = vec![Hand, Size, Layers];
            held.extend(nine);
            (vec![Brush, Eraser, Select, Fill, Color, More], held)
        }
        _ => {
            let mut keys = vec![Brush, Eraser, Select, Fill, Hand, Size, Color, Layers];
            keys.extend(nine);
            (keys, Vec::new())
        }
    }
}

/// What a tool key shows and does.
struct ToolFace {
    icon: Icon,
    caption: &'static str,
    /// The spoken name. The rail says `Fill`, the menu row `Fill tool`.
    name: &'static str,
    action: Action,
    /// What a click on it opens while it is armed.
    options: Float,
}

fn tool_face(key: Key) -> ToolFace {
    let face = |icon, caption, name, action, options| ToolFace { icon, caption, name, action, options };
    match key {
        Key::Brush => face(Icon::Brush, "Brush", "Brush", Action::BrushTool, Float::Brushes),
        Key::Eraser => face(Icon::Eraser, "Eraser", "Eraser", Action::ToggleEraser, Float::ToolOptions),
        Key::Select => face(Icon::Select, "Select", "Select", Action::SelectTool, Float::ToolOptions),
        Key::Fill => face(Icon::Fill, "Fill", "Fill", Action::FillTool, Float::ToolOptions),
        _ => face(Icon::Panel, "Panel", "Panel tool", Action::PanelTool, Float::ToolOptions),
    }
}

fn tool_armed(canvas: &CanvasView, key: Key) -> bool {
    !canvas.hand_held
        && match key {
            Key::Brush => canvas.tool == CanvasTool::Paint && !canvas.erasing,
            Key::Eraser => canvas.tool == CanvasTool::Paint && canvas.erasing,
            Key::Select => canvas.tool == CanvasTool::Select,
            Key::Fill => matches!(canvas.tool, CanvasTool::Fill | CanvasTool::LassoFill),
            _ => canvas.tool == CanvasTool::Panel,
        }
}

/// The rail calls `set_tool` directly: the matching Actions toggle.
fn arm(args: &mut OverlayArgs, key: Key) {
    let (canvas, state) = (&mut *args.canvas, &mut *args.state);
    match key {
        Key::Brush => canvas.arm_paint(false, state),
        Key::Eraser => canvas.arm_paint(true, state),
        Key::Select => canvas.set_tool(CanvasTool::Select, state),
        Key::Fill => canvas.set_tool(CanvasTool::Fill, state),
        _ => canvas.set_tool(CanvasTool::Panel, state),
    }
}

fn tool_key(ui: &mut egui::Ui, args: &mut OverlayArgs, key: Key, size: Vec2, captioned: bool) {
    let face = tool_face(key);
    let armed = tool_armed(args.canvas, key);
    let caption = captioned.then_some(face.caption);
    let opened = armed && args.floats.is_open(face.options);
    let resp = plate::rail_key(ui, size, Sense::click(), face.icon, caption, face.name, armed, opened, armed);
    let resp = if key == Key::Brush {
        hover_tip_with(resp, face.action, || args.canvas.drawing_with(args.state))
    } else {
        hover_tip(resp, face.action, "")
    };
    if resp.clicked() {
        if armed {
            let busy = args.canvas.stroke_active();
            args.floats.open(face.options, busy);
        } else {
            arm(args, key);
        }
    }
}

/// How far along the wedge `px` stands: its range reads evenly by ratio.
fn size_t(px: f32, (lo, hi): (f32, f32)) -> f32 {
    (px / lo).ln() / (hi / lo).ln()
}

/// The size a pull of `dy` points lands on from `px`, up for bigger.
fn dragged_px(px: f32, dy: f32, (lo, hi): (f32, f32)) -> f32 {
    px * ((hi / lo).ln() * -dy / SIZE_TRAVEL).exp()
}

fn size_key(ui: &mut egui::Ui, args: &mut OverlayArgs, size: Vec2, captioned: bool) {
    let (px, range) = (args.canvas.active_px(), args.canvas.px_range());
    let caption = captioned.then_some("Size");
    let resp = plate::size_key(ui, size, caption, size_t(px, range));
    // The whole pull, from the size at the press: a frame's delta would carry
    // a pointer's jump onto the key, and a press alone changes nothing.
    let from = resp.id.with("from");
    if resp.drag_started() {
        ui.data_mut(|d| d.insert_temp(from, px));
    }
    if let Some(pull) = resp.total_drag_delta().filter(|pull| pull.y != 0.0) {
        let start = ui.data(|d| d.get_temp(from)).unwrap_or(px);
        args.canvas.set_brush_px(dragged_px(start, pull.y, range), args.state);
    }
    if resp.is_pointer_button_down_on() {
        bubble(ui.ctx(), &resp, args.canvas);
    }
    plate::hover_card(resp, || {
        format!("Size: {px:.1} px. Drag up for bigger, down for smaller.")
    });
}

/// The true-size bubble beside the Size key: a dab as the paper will show it.
fn bubble(ctx: &egui::Context, key: &egui::Response, canvas: &CanvasView) {
    let px = canvas.active_px();
    let across = (px * canvas.view_scale()).clamp(2.0, BUBBLE_MAX);
    let side = (across + 16.0).max(56.0);
    let at = pos2(key.rect.right() + 12.0, key.rect.center().y - side * 0.5);
    egui::Area::new(egui::Id::new("rail_size_bubble"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fade_in(false)
        .constrain_to(ctx.content_rect())
        .fixed_pos(at)
        .show(ctx, |ui| {
            plate::island_frame(ui.style(), 4).show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
                ui.painter().circle_filled(rect.center(), across * 0.5, plate::INK);
                let below = pos2(rect.center().x, rect.bottom() - 2.0);
                let font = plate::Role::Small.font();
                let words = format!("{px:.0} px");
                let color = plate::BLUE_DEEP;
                ui.painter().text(below, egui::Align2::CENTER_BOTTOM, words, font, color);
            });
        });
}

fn color_key(ui: &mut egui::Ui, args: &mut OverlayArgs, size: Vec2, captioned: bool) {
    let opened = args.floats.is_open(Float::Color);
    let caption = captioned.then_some("Color");
    let ink = plate::chip(args.canvas.brush_color);
    let resp = plate::color_key(ui, size, caption, ink, opened);
    let resp = plate::hover_card(resp, || "Brush color. Opens the colors.".to_string());
    if resp.clicked() {
        let busy = args.canvas.stroke_active();
        args.floats.toggle(Float::Color, busy);
    }
}

fn layers_key(ui: &mut egui::Ui, args: &mut OverlayArgs, size: Vec2, captioned: bool) {
    let opened = args.floats.is_open(Float::Layers);
    let caption = captioned.then_some("Layers");
    let resp =
        plate::rail_key(ui, size, Sense::click(), Icon::Layers, caption, "Layers", false, opened, false);
    let resp = plate::hover_card(resp, || args.canvas.drawing_with(args.state));
    if resp.clicked() {
        if args.layers_docked {
            args.floats.focus_layers = true;
        } else {
            let busy = args.canvas.stroke_active();
            args.floats.toggle(Float::Layers, busy);
        }
    }
}

fn hand_key(ui: &mut egui::Ui, args: &mut OverlayArgs, size: Vec2, captioned: bool) {
    let held = args.canvas.hand_held;
    let caption = captioned.then_some("Hand");
    let resp = plate::rail_key(ui, size, Sense::click(), Icon::Hand, caption, "Hand", held, false, false);
    let resp = plate::hover_card(resp, || {
        "Hand: drag the canvas to move it, as with Space held. Any tool lets go.".to_string()
    });
    if resp.clicked() && !args.canvas.pen_busy() {
        args.canvas.hand_held = !held;
    }
}

/// Momentary, so drag only: a held press keeps the capture a long touch
/// would take from a click control. Nothing is clicked here.
fn flip_key(ui: &mut egui::Ui, args: &mut OverlayArgs, size: Vec2, captioned: bool) {
    let held = args.canvas.rail_flip;
    let caption = captioned.then_some("Flip");
    let resp = plate::rail_key(ui, size, Sense::drag(), Icon::Flip, caption, "Flip", held, false, false);
    args.canvas.rail_flip = resp.is_pointer_button_down_on();
    plate::hover_card(resp, || "Flip: hold to see the drawing before this one (R).".to_string());
}

fn draw_key(ui: &mut egui::Ui, args: &mut OverlayArgs, key: Key, tier: Tier, captioned: bool) {
    let size = tier.key_size(key);
    match key {
        Key::Brush | Key::Eraser | Key::Select | Key::Fill | Key::Panel => {
            tool_key(ui, args, key, size, captioned)
        }
        Key::Hand => hand_key(ui, args, size, captioned),
        Key::Size => size_key(ui, args, size, captioned),
        Key::Color => color_key(ui, args, size, captioned),
        Key::Layers => layers_key(ui, args, size, captioned),
        Key::Flip => flip_key(ui, args, size, captioned),
        Key::More => {
            let open = args.floats.more;
            let resp =
                plate::rail_key(ui, size, Sense::click(), Icon::Dots, None, "More tools", false, open, false);
            if resp.clicked() {
                args.floats.more = !open;
            }
        }
    }
}

/// The Pocket tier's `…`: a row beside the rail, its keys at their full size.
fn popover(ctx: &egui::Context, args: &mut OverlayArgs, keys: &[Key], host: Rect, top: f32) -> Rect {
    let shown = egui::Area::new(egui::Id::new("rail_popover"))
        .order(egui::Order::Middle)
        .fixed_pos(pos2(host.left() + floats::LEFT_X, top))
        .constrain_to(host)
        .fade_in(false)
        .show(ctx, |ui| {
            plate::island_frame(ui.style(), MARGIN).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.horizontal(|ui| {
                    for key in keys {
                        draw_key(ui, args, *key, Tier::Full, true);
                    }
                });
            });
        });
    if let Some(rects) = args.pane_rects.as_deref_mut() {
        rects.push((Pane::Canvas, shown.response.rect, shown.response.layer_id));
    }
    shown.response.rect
}

/// The rail and the floats over `host`, the rect the canvas drew in this
/// frame. Called once per host, after `canvas.ui`.
pub fn overlay(ui: &egui::Ui, args: &mut OverlayArgs, host: Rect) {
    let ctx = ui.ctx();
    args.floats.settle(args.canvas.stroke_active());
    args.canvas.rail_flip = false;
    let reference = reference(host.height(), args.strip_closed);
    let tier = if args.rail_off { Tier::Hidden } else { Tier::of(reference, host.width()) };
    let mut top = host.top() + 12.0;
    if tier != Tier::Hidden {
        top = top_of(host, reference, tier);
        let (keys, held) = keys_of(tier, args);
        let shown = egui::Area::new(egui::Id::new("rail"))
            .order(egui::Order::Middle)
            .fixed_pos(pos2(host.left() + RAIL_X, top))
            .constrain_to(host)
            .fade_in(false)
            .show(ctx, |ui| {
                plate::island_frame(ui.style(), MARGIN).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    ui.set_width(KEY_W);
                    ui.vertical_centered(|ui| {
                        for key in &keys {
                            draw_key(ui, args, *key, tier, tier == Tier::Full);
                        }
                    });
                });
            });
        if let Some(rects) = args.pane_rects.as_deref_mut() {
            rects.push((Pane::Canvas, shown.response.rect, shown.response.layer_id));
        }
        if args.floats.more && !held.is_empty() {
            popover(ctx, args, &held, host, top);
        }
    }
    floats::show(ctx, args, host, top);
}
