//! The plate — the app's entire colour, type, spacing and affordance
//! vocabulary, in the one look research/PSD-approachable-ui.md section 2 pins.
//!
//! The direction in one line: a neutral desk, white paper, one steel peg bar,
//! everything else quiet, so the art is the only saturated thing in the
//! window. Tokens are named by role, not by material. This module is the only
//! place a UI colour may be defined; a `Color32::from_rgb` literal anywhere
//! else in the app's chrome is a bug with a name. State is never colour
//! alone: a refusal is words plus a mark, current is a wash plus deep ink,
//! armed is a filled shape.

// Later parts of the look read tokens this one only declares.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use egui::{
    Color32, CornerRadius, FontFamily, FontId, Pos2, Response, Sense, Shape, Stroke, StrokeKind,
    Ui, Vec2,
    text::{LayoutJob, TextFormat},
};

use crate::icons::Icon;
use crate::lessons::Target;

// ---------------------------------------------------------------------------
// Tokens (spec 2.2). Named by role. `aulook_tokens_are_the_table` pins them.
// ---------------------------------------------------------------------------

/// The window, top row, band, tab bars and the canvas's pasteboard.
pub const DESK: Color32 = Color32::from_rgb(0xD3, 0xD5, 0xD8);
/// Docked panels, rail, floats, menus, the step island.
pub const ISLAND: Color32 = Color32::from_rgb(0xF6, 0xF7, 0xF8);
/// Fields, a filled button at rest, a ghost button under the hand.
pub const WELL: Color32 = Color32::from_rgb(0xE8, 0xEA, 0xED);
/// A filled button under the hand, a hovered row.
pub const HOVER: Color32 = Color32::from_rgb(0xE0, 0xE3, 0xE7);
/// A filled button pressed.
pub const PRESSED: Color32 = Color32::from_rgb(0xCD, 0xD1, 0xD6);
/// The paper and the strip's sheets. Screen-only: exports never read it.
pub const PAPER: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
/// Primary text.
pub const INK: Color32 = Color32::from_rgb(0x1E, 0x22, 0x27);
/// Secondary text, icons at rest.
pub const INK2: Color32 = Color32::from_rgb(0x4C, 0x52, 0x5B);
/// The quietest ink the look allows; held to 3.0, not 4.5. Dim words outside
/// a disabled scope, a field's edge, a swatch's ring, a page outline on paper.
pub const INK3: Color32 = Color32::from_rgb(0x70, 0x76, 0x7F);
/// Decorative hairlines only.
pub const RULE: Color32 = Color32::from_rgb(0xBF, 0xC3, 0xC8);
/// Armed tool fill, the one primary, the focus ring, links.
pub const BLUE: Color32 = Color32::from_rgb(0x1F, 0x5F, 0xBF);
/// A primary under the hand, current words, selected text, the playhead.
pub const BLUE_DEEP: Color32 = Color32::from_rgb(0x18, 0x4C, 0x9A);
/// The current step, a selected row.
pub const BLUE_WASH: Color32 = Color32::from_rgb(0xDC, 0xE8, 0xF9);
/// Refusal words, a danger fill.
pub const RED: Color32 = Color32::from_rgb(0xB9, 0x2D, 0x1C);
/// AU27: the night sky a lesson paints: a chip on the lesson card.
pub const LESSON_NIGHT: Color32 = Color32::from_rgb(0x14, 0x1B, 0x34);
/// AU13: the Colors panel's ten starter swatches, as (name, color). UI
/// constants: nothing of them reaches a document until a fill lands.
pub const STARTER_SWATCHES: [(&str, Color32); 10] = [
    ("Black", Color32::from_rgb(0x00, 0x00, 0x00)),
    ("White", Color32::from_rgb(0xFF, 0xFF, 0xFF)),
    ("Red", Color32::from_rgb(0xD9, 0x3A, 0x2B)),
    ("Orange", Color32::from_rgb(0xF2, 0x99, 0x4A)),
    ("Yellow", Color32::from_rgb(0xF2, 0xC9, 0x4C)),
    ("Green", Color32::from_rgb(0x3F, 0xA3, 0x4D)),
    ("Blue", Color32::from_rgb(0x2F, 0x7F, 0xD8)),
    ("Purple", Color32::from_rgb(0x8A, 0x5C, 0xC7)),
    ("Peach", Color32::from_rgb(0xF2, 0xC9, 0xA1)),
    ("Brown", Color32::from_rgb(0x8B, 0x5E, 0x3C)),
];
/// The refusal pill.
pub const RED_WASH: Color32 = Color32::from_rgb(0xFB, 0xE5, 0xE1);
/// The peg bar, and paper turned dark for the miss check.
pub const STEEL: Color32 = Color32::from_rgb(0x2A, 0x2E, 0x34);
/// Peg bar labels.
pub const STEEL_INK: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEB);
/// The surround of the Result pane only, so a colour judgement sees neutral
/// grey.
pub const RESULT: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);
/// The shadow colour, with the alphas of [`visuals`] and [`dialog_frame`].
pub const SHADOW: Color32 = Color32::from_rgb(0x14, 0x18, 0x1F);
/// White on a solid fill: armed, primary, danger.
pub const ON_FILL: Color32 = Color32::WHITE;

// The old names, as aliases until the alias retirement (spec 2.4), so the
// uses across the app do not all move at once. They flip light and dark:
// `GRAPHITE` was a dark ink on pale paper and is a pale surface now, so a
// mark on paper never reads one of these.
pub const GRAPHITE: Color32 = ISLAND;
pub const LEGEND: Color32 = INK2;
pub const STRUCK: Color32 = INK;
pub const AO: Color32 = INK3;
pub const AKA: Color32 = RED;
pub const TALLY: Color32 = BLUE;

// Document colours. The trade's pencils are what a document shows and stores,
// so they keep their values through any change of look: the swatch board, the
// presets' inks, the ghosts' tints and the layer roles read these.
/// The blue pencil.
pub const PENCIL_AO: Color32 = Color32::from_rgb(83, 137, 196);
/// The red pencil.
pub const PENCIL_AKA: Color32 = Color32::from_rgb(228, 82, 47);
/// The grey the ghost AHEAD is tinted from.
pub const PENCIL_LEGEND: Color32 = Color32::from_rgb(139, 141, 131);

// `LayerProps.chip` is stored in the document, and a stored chip is matched
// to its ring by value, so these are frozen: a token moving must not change
// what gets stored, nor stop an old document's ring matching.
pub const CHIP_STRUCK: [u8; 4] = [228, 225, 214, 255];
pub const CHIP_LEGEND: [u8; 4] = [139, 141, 131, 255];
pub const CHIP_PAPER: [u8; 4] = [242, 238, 230, 255];
pub const CHIP_AO: [u8; 4] = [83, 137, 196, 255];
pub const CHIP_AKA: [u8; 4] = [228, 82, 47, 255];
/// AU28: a lesson's blue line (spec 5.7). Non-photo blue, the scaffold's ink:
/// `AO` is grey now. It never reaches a file.
pub const GUIDE: Color32 = Color32::from_rgb(CHIP_AO[0], CHIP_AO[1], CHIP_AO[2]);
/// AU28: `Show me`'s eraser on its way: paper, soft, over the ink it will take.
pub const RUBBING: Color32 = Color32::from_rgba_premultiplied(190, 190, 190, 190);

/// AU28: the document's ink as the UI paints it: `Show me`'s line on its way.
pub fn opaque(rgba: [u8; 4]) -> Color32 {
    Color32::from_rgb(rgba[0], rgba[1], rgba[2])
}

/// What a disabled scope dims to. egui's 0.5 leaves `INK` at 2.87 on `DESK`;
/// at this value `INK` holds 3.0 on all three fills and `INK2` still does
/// not, so content inside a disabled scope is always `INK` (spec 2.3).
pub const DISABLED_ALPHA: f32 = 0.65;

// AU1: what the audits in `measure.rs` read a shape's colours against. The
// plate is the one place a colour may be spelled, and that holds for tests.
/// A text section's "use the shape's fallback colour" sentinel.
#[cfg(test)]
pub const SECTION_PLACEHOLDER: Color32 = Color32::PLACEHOLDER;
/// The seeded fixtures' inks and grounds.
#[cfg(test)]
pub const SEED_WHITE: Color32 = Color32::WHITE;
#[cfg(test)]
pub const SEED_BLACK: Color32 = Color32::BLACK;
#[cfg(test)]
pub const SEED_GREY: Color32 = Color32::from_gray(128);

/// Exact UI descriptions (Settings → UI Features): when set, every plate
/// control's hover names its kind + label + source, so the owner can ask
/// for edits by an element's true name. Set once per frame from config.
static EXACT: AtomicBool = AtomicBool::new(false);

pub fn set_exact(on: bool) {
    EXACT.store(on, Ordering::Relaxed);
}

pub fn exact() -> bool {
    EXACT.load(Ordering::Relaxed)
}

/// The accessible name and state of a painted control (AccessKit, D7):
/// icon-only controls answer to their hover text or icon name instead of
/// an empty label, so screen readers and the testkit can find them. Every
/// control painted outside this file speaks through here too.
pub(crate) fn speak(resp: &Response, typ: egui::WidgetType, selected: Option<bool>, name: &str) {
    let enabled = resp.enabled();
    resp.widget_info(|| match selected {
        Some(on) => egui::WidgetInfo::selected(typ, enabled, on, name),
        None => egui::WidgetInfo::labeled(typ, enabled, name),
    });
}

/// An icon's spoken name, for controls drawn without a label.
fn icon_name(icon: crate::icons::Icon) -> String {
    format!("{icon:?}").to_lowercase()
}

/// What a tool control answers to: its own name, else its caption, else
/// its icon — so a compact (caption-less) deck keeps the full names.
fn tool_name(name: &str, label: &str, icon: crate::icons::Icon) -> String {
    match (name.is_empty(), label.is_empty()) {
        (false, _) => name.to_string(),
        (true, false) => label.to_string(),
        (true, true) => icon_name(icon),
    }
}

/// Append the exact identity line to a response's hover, when enabled: a card
/// of its own above the control, so the pen passes it and the control's own
/// card keeps its place.
fn describe(resp: Response, kind: &str, label: &str) -> Response {
    if exact() { exact_card(resp, kind, label) } else { resp }
}

pub(crate) fn exact_card(resp: Response, kind: &str, label: &str) -> Response {
    let over = resp.rect.left_top() - Vec2::new(0.0, 4.0);
    let words = || format!("[{kind} '{label}' · plate.rs]");
    card_at(resp, "exact_card", |_| (egui::Align2::LEFT_BOTTOM, over), words)
}

// Derived colours. No new hues are permitted: each one is a token, opaque or
// at a set transparency, used for the time rules and the current row's
// ground. Text is opaque, so its contrast never depends on what is behind it.

/// Secondary words: opaque `INK2`. A stroke, frame or grip reads
/// [`hairline`].
#[inline]
pub fn legend_dim() -> Color32 {
    INK2
}
/// A decorative stroke, frame or grip. Carries no information alone.
#[inline]
pub fn hairline() -> Color32 {
    RULE
}
/// Beat rule (every 6 frames), 1 device px. Decorative.
#[inline]
pub fn rule_beat() -> Color32 {
    alpha(INK2, 56)
}
/// Half-second rule (every 12 frames), 1 device px. Decorative.
#[inline]
pub fn rule_half() -> Color32 {
    alpha(INK2, 102)
}
/// Second rule (every 24 frames), 2 device px.
#[inline]
pub fn rule_sec() -> Color32 {
    LEGEND
}
/// The current row's ground, over whatever the row already holds: the blue
/// at the wash's strength, so the cells' marks still read through it.
#[inline]
pub fn tally_well() -> Color32 {
    alpha(BLUE, 36)
}
/// GM20: the veil over paper the GPU does not paint (past a capped
/// painting window): steel through the sheet, so the strip reads as paper
/// out of reach — not desk, and not a warning.
#[inline]
pub fn paper_veil() -> Color32 {
    alpha(STEEL, 110)
}
/// GM22: the tint that draws a picture as it is at `opacity` (0..=1): the
/// canvas's 3D underlay, faded under the drawing.
#[inline]
pub fn faded(opacity: f32) -> Color32 {
    tint((opacity.clamp(0.0, 1.0) * 255.0).round() as u8)
}
/// GM19: a token at a transparency `a` of 255 — the same hue, never a new
/// one (the hold rule's Ao under a selection, a Well veil behind a note).
#[inline]
pub fn alpha(token: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(token.r(), token.g(), token.b(), a)
}
/// GM19: the node graph's idle pins and wires, opaque, which must hide what
/// they cross.
#[inline]
pub fn legend_muted() -> Color32 {
    INK3
}

// Neutral contrast (GM19). Black and white mean nothing here: they are how a
// mark reads on ANY paper or picture (the brush cursor's double ring, the
// scrim behind a modal) and how a texture is drawn as it is.

/// A texture drawn as it is (egui's white tint).
pub const UNTINTED: Color32 = Color32::WHITE;
/// The colour picker's square fades every hue to these two ends.
pub const PICK_WHITE: Color32 = Color32::WHITE;
pub const PICK_BLACK: Color32 = Color32::BLACK;
/// A texture drawn at `a` of 255 ([`faded`]'s byte form).
#[inline]
pub fn tint(a: u8) -> Color32 {
    Color32::from_white_alpha(a)
}
/// The light half of a mark that must read on any paper.
#[inline]
pub fn halo(a: u8) -> Color32 {
    Color32::from_white_alpha(a)
}
/// The dark half of that mark, and the scrim behind a modal.
#[inline]
pub fn shade(a: u8) -> Color32 {
    Color32::from_black_alpha(a)
}

/// GM19: a colour the ANIMATOR made — a chip, an ink, a swatch, a picked
/// hue — shown exactly as stored. Document data passing through the
/// chrome, never a UI colour of its own.
#[inline]
pub fn chip([r, g, b, a]: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

// ---------------------------------------------------------------------------
// Structure (§2.3). The spacing scale is 2·4·6·8·12·16·24 — nothing else.
// Fixed dimensions, in logical points. Later phases consume these; they are
// declared here so every panel snaps to the same skeleton.
// ---------------------------------------------------------------------------

pub const ROW_H: f32 = 18.0;
pub const GUTTER_W: f32 = 44.0;
pub const SLATE_H: f32 = 42.0;
pub const FOOT_H: f32 = 34.0;
/// Every button, toggle, detent in a panel.
pub const CTRL_H: f32 = 28.0;
/// The canvas deck's options row: its tallest control, because a stock
/// button stands a point over `CTRL_H` on `button_padding` (the Body row is
/// 17). The canvas takes what is left below this, so no tool may outgrow it.
pub const OPTIONS_H: f32 = CTRL_H + 2.0;
pub const RULE_STRIP_W: f32 = 14.0;
pub const LIGHTBOX_W: f32 = 96.0;
/// The words a hand-painted control carries: the Panel role.
const CTRL_TEXT: f32 = Role::Panel.size();
/// The icon in a hand-painted control, on the size the masks are cut at.
const ICON: f32 = 16.0;
/// The icons on the top row and the band (spec 2.7).
pub const BAR_ICON: f32 = 20.0;
/// The icon in a rail key (spec 2.6).
pub const RAIL_ICON: f32 = 22.0;
/// A control on the top row or the band (spec 2.6).
pub const CHROME_H: f32 = 32.0;
/// The transport row: the band's last 36 (spec 4.4).
pub const BAND_H: f32 = 36.0;
/// A detent's ring takes this much of the control's left.
const DETENT_W: f32 = 22.0;
/// The gap between the sheet head's rows, down from `item_spacing`: the
/// sheet's density is the one panel density that does not change.
pub const SHEET_GAP: f32 = 2.0;
/// From a text field's edge to its words.
const FIELD_INSET: i8 = 6;
/// A latch's checkbox.
const LATCH_BOX: f32 = 16.0;
/// From a latch's left to its words.
const LATCH_TEXT_X: f32 = LATCH_BOX + 6.0;
/// A docked pane's inner margin. `window_margin` is a float's; a pane keeps
/// the room it always had, because the sheet's rows live in it.
pub const PANE_MARGIN: i8 = 6;

// The radii (spec 2.6). Paper is square; a stadium takes half its height.
pub const R_DIALOG: u8 = 14;
pub const R_FLOAT: u8 = 12;
pub const R_MENU: u8 = 10;
pub const R_CONTROL: u8 = 8;
pub const R_INPUT: u8 = 6;
pub const R_SHEET: u8 = 4;
/// A stadium: egui clamps a radius to half the shape's height.
pub const R_PILL: u8 = 64;
/// AU15: a dock's tab bar, and how far its tabs stand in from the bar's edges,
/// so they read as the pills of the step bar over them.
pub const TAB_BAR_H: f32 = 34.0;
pub const TAB_INSET: i8 = 3;
/// Half a tab's height, the full stadium of 2.6.
pub const TAB_RADIUS: u8 = 14;
/// AU15: how much of a scroll handle stands at rest.
pub const SCROLL_CUE: f32 = 0.5;

/// The pixel-snapping law. Every rule, notch, dot and 1px edge is snapped;
/// unsnapped hairlines shimmer under scroll at ppp 1.25 and this direction
/// has no ornament to hide behind. Rule thickness is specified in DEVICE
/// pixels and converted with `device_px`.
#[inline]
pub fn snap(v: f32, ppp: f32) -> f32 {
    (v * ppp).round() / ppp
}

/// N device pixels expressed in logical points at the given pixels-per-point.
#[inline]
pub fn device_px(n: f32, ppp: f32) -> f32 {
    n / ppp
}

// ---------------------------------------------------------------------------
// Type (spec 2.5). IBM Plex Sans only, sentence case everywhere, no
// letterspacing. Static cuts, installed once at boot; the Japanese face stays
// in every stack because studio words appear in hover cards and aliases.
// ---------------------------------------------------------------------------

/// The SemiBold cut (headings, legends, step names, the current sheet row).
pub fn semibold() -> FontFamily {
    FontFamily::Name("plex-sans-semibold".into())
}

/// The SemiBold mono cut (the frame counter).
pub fn mono_semibold() -> FontFamily {
    FontFamily::Name("plex-mono-semibold".into())
}

/// Plex Mono: file patterns only, at 12 ([`mono_font`], [`pattern_field`]).
pub fn mono() -> FontFamily {
    FontFamily::Name("mono".into())
}

/// A file pattern's face.
pub fn mono_font() -> FontId {
    FontId::new(12.0, mono())
}

/// The type roles, and the only sizes the look has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// The Start float's heading.
    Title,
    /// Start cards, the Done card's heading.
    Card,
    /// Float and dialog headings.
    Heading,
    /// Menu words, band words.
    Chrome,
    /// Step pills.
    Step,
    /// `TextStyle::Body` and `Button`, panel text.
    Panel,
    /// The floor: rail captions, peg bar labels.
    Small,
}

impl Role {
    pub const fn size(self) -> f32 {
        match self {
            Role::Title => 24.0,
            Role::Card => 20.0,
            Role::Heading => 16.0,
            // Panel was 13 (spec 2.5): the frozen arrangement read 9 px at it,
            // and a miss raises `Body` to 14.
            Role::Chrome | Role::Step | Role::Panel => 14.0,
            Role::Small => 12.0,
        }
    }

    pub fn font(self) -> FontId {
        let face = match self {
            Role::Title | Role::Card | Role::Heading | Role::Step => semibold(),
            Role::Chrome | Role::Panel | Role::Small => FontFamily::Proportional,
        };
        FontId::new(self.size(), face)
    }
}

/// Install the Plex stack and the type scale. Proportional = Plex Sans
/// Regular; Monospace = Plex Mono Medium; named families carry the SemiBold
/// cuts and the file patterns' face. IBM Plex Sans JP Bold (GM16: the whole
/// face, the engine's lettering static, no longer a subset) rides second in
/// every family so 原画 falls through per-glyph automatically; egui's default
/// fonts stay at the tail so an unexpected glyph degrades to a drawn shape,
/// never tofu.
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

/// The Plex stack as egui font definitions (what [`install_fonts`] sets;
/// GM16: the glyph-coverage test reads every family's stack from here).
pub fn font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    // GM11 (D16): Plex Sans Regular and Mono Medium are the engine's own
    // statics (the burn-ins rasterise them too) — the bytes live once.
    for (name, bytes) in [
        ("plex-sans", anim_core::text::PLEX_SANS),
        (
            "plex-sans-sb",
            &include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf")[..],
        ),
        ("plex-mono", anim_core::text::PLEX_MONO),
        (
            "plex-mono-sb",
            &include_bytes!("../assets/fonts/IBMPlexMono-SemiBold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), egui::FontData::from_static(bytes).into());
    }
    // GM16: the whole JP Bold, the generators' lettering face — one copy of
    // its bytes for the UI and the burn-in alike (D16). Half of its 2 em
    // height is line gap, and epaint centres a fallback's height on the
    // row's 1.3 em: unseated, 原画 rides 0.495 em over Plex Sans's baseline.
    let seated = egui::FontTweak { y_offset_factor: 0.495, ..Default::default() };
    let jp = egui::FontData::from_static(anim_core::text::PLEX_SANS_JP).tweak(seated);
    fonts.font_data.insert("plex-jp".to_owned(), jp.into());

    // GM16: egui's Hack closes the proportional tail too — of the whole
    // stack only it draws ▾ and ▸ (the menus' marks; genui_glyph_coverage).
    // Last, so it draws nothing another face has.
    if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
        prop.push("Hack".to_owned());
    }
    // Capture egui's default stacks BEFORE prepending, so the named families
    // inherit the same symbol/emoji tail the built-ins keep.
    let default_prop = fonts.families[&FontFamily::Proportional].clone();
    let default_mono = fonts.families[&FontFamily::Monospace].clone();
    let prop = fonts.families.get_mut(&FontFamily::Proportional).unwrap();
    prop.insert(0, "plex-sans".to_owned());
    prop.insert(1, "plex-jp".to_owned());
    let tail = fonts.families.get_mut(&FontFamily::Monospace).unwrap();
    tail.insert(0, "plex-mono".to_owned());
    tail.insert(1, "plex-jp".to_owned());
    let mut sb = vec!["plex-sans-sb".to_owned(), "plex-jp".to_owned()];
    sb.extend(default_prop);
    fonts.families.insert(semibold(), sb);
    let mut msb = vec!["plex-mono-sb".to_owned(), "plex-jp".to_owned()];
    msb.extend(default_mono.clone());
    fonts.families.insert(mono_semibold(), msb);
    let mut pattern = vec!["plex-mono".to_owned(), "plex-jp".to_owned()];
    pattern.extend(default_mono);
    fonts.families.insert(mono(), pattern);
    fonts
}

/// The app's one `egui::Style`: the visuals, the type scale and the spacing.
/// `dress` writes it into both theme slots, so Windows Light mode cannot fall
/// back to stock egui.
pub fn style() -> egui::Style {
    use egui::TextStyle;
    let mut style = egui::Style { visuals: visuals(), ..Default::default() };
    // Debug-build red boxes on unaligned text read as refusals. Off.
    // egui's debug field exists only in debug builds.
    #[cfg(debug_assertions)]
    {
        style.debug.show_unaligned = false;
    }
    let scale = &mut style.text_styles;
    scale.insert(TextStyle::Body, Role::Panel.font());
    scale.insert(TextStyle::Button, Role::Panel.font());
    scale.insert(TextStyle::Small, Role::Small.font());
    scale.insert(TextStyle::Monospace, Role::Panel.font());
    scale.insert(TextStyle::Heading, Role::Heading.font());
    style.drag_value_text_style = TextStyle::Body;

    let s = &mut style.spacing;
    s.button_padding = Vec2::new(10.0, 6.0);
    s.item_spacing = Vec2::new(6.0, 4.0);
    s.window_margin = egui::Margin::same(12);
    s.menu_margin = egui::Margin::same(6);
    s.indent = 16.0;
    s.interact_size.y = CTRL_H;
    // Eleven rows at a control's height: a combo opens whole, not to a fold.
    s.combo_height = 352.0;
    // AU15: a list that runs past its edge shows it at rest, not only under a
    // hover: the handle is what says the word cut there can be reached.
    s.scroll.dormant_handle_opacity = SCROLL_CUE;

    style.interaction.selectable_labels = false;
    style.interaction.tooltip_delay = 0.35;
    style
}

/// The top row's and the band's frame: the desk itself, with no rule.
pub fn desk_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).fill(DESK)
}

/// A dialog's frame. Floats and the rail take [`visuals`]'s window shadow.
pub fn dialog_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style).corner_radius(R_DIALOG).shadow(egui::epaint::Shadow {
        offset: [0, 12],
        blur: 40,
        spread: 0,
        color: alpha(SHADOW, 71),
    })
}

/// The paper's own shadow on the pasteboard (spec 2.6).
pub fn paper_shadow(rect: egui::Rect) -> Shape {
    let shadow = egui::epaint::Shadow { offset: [0, 2], blur: 10, spread: 0, color: alpha(SHADOW, 41) };
    Shape::from(shadow.as_shape(rect, CornerRadius::ZERO))
}

/// A hand-painted float's ground (a fold-out over the canvas): the island,
/// with the window's corners and shadow.
pub fn float_ground(ui: &Ui, rect: egui::Rect) {
    let painter = ui.painter();
    painter.add(ui.visuals().window_shadow.as_shape(rect, CornerRadius::same(R_FLOAT)));
    painter.rect_filled(rect, R_FLOAT, ISLAND);
}

/// The rail's and a float's frame: the island, the float's corners, the
/// window's shadow, `margin` inside.
pub fn island_frame(style: &egui::Style, margin: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(ISLAND)
        .corner_radius(R_FLOAT)
        .shadow(style.visuals.window_shadow)
        .inner_margin(margin)
}

/// How long a pointer rests on a control before its card shows (spec 4.5).
pub const CARD_DELAY: f64 = 0.35;

/// A hover card: at `Order::Tooltip`, and never interactable, so the pen
/// passes through it while the floats below still block. `text` is built only
/// once the card shows. It stands beside its control. Against the window's
/// right edge it stands over or under instead: pushed back onto its control
/// it would take the pointer from it, and blink.
pub fn hover_card(resp: Response, text: impl FnOnce() -> String) -> Response {
    let (rect, room) = (resp.rect, resp.ctx.content_rect());
    let place = move |wide: f32| {
        let beside = rect.right_top() + Vec2::new(8.0, 0.0);
        if beside.x + wide <= room.right() {
            (egui::Align2::LEFT_TOP, beside)
        } else if rect.center().y > room.center().y {
            (egui::Align2::RIGHT_BOTTOM, rect.right_top() - Vec2::new(0.0, 8.0))
        } else {
            (egui::Align2::RIGHT_TOP, rect.right_bottom() + Vec2::new(0.0, 8.0))
        }
    };
    card_at(resp, "hover_card", place, text)
}

/// A [`hover_card`] that stands on `at` and reads upward: the strip's, because
/// a card beside one surface would cover the band it describes.
pub fn card_above(resp: Response, at: Pos2, text: impl FnOnce() -> String) -> Response {
    card_at(resp, "card_above", |_| (egui::Align2::LEFT_BOTTOM, at), text)
}

/// [`hover_card`] with its words ready, for a chain: what a float's controls
/// wear where a stock tooltip would stop the pen (R15).
pub trait Card {
    fn card(self, words: impl Into<String>) -> Self;
}

impl Card for Response {
    fn card(self, words: impl Into<String>) -> Self {
        hover_card(self, || words.into())
    }
}

/// One card, by `salt`, so one control can wear two. `place` is told how wide
/// its words came out, and names the corner of it that stands on which point.
fn card_at(
    resp: Response,
    salt: &str,
    place: impl FnOnce(f32) -> (egui::Align2, Pos2),
    text: impl FnOnce() -> String,
) -> Response {
    let ctx = resp.ctx.clone();
    let id = resp.id.with(salt);
    // A click is no rest, as with a stock tooltip: a card rising beside the
    // key just clicked would lie over the next one and take its press, since
    // a Tooltip layer stands in egui's hit test. The pointer moves on first.
    let clicked = ctx.input(|i| i.pointer.time_since_last_click() < i.pointer.time_since_last_movement() + 0.1);
    // egui never calls a disabled widget hovered, and that is the key with
    // something to explain.
    let rests = resp.hovered() || (!resp.enabled() && resp.contains_pointer());
    if !rests || resp.is_pointer_button_down_on() || clicked {
        ctx.data_mut(|d| d.remove_temp::<f64>(id));
        return resp;
    }
    let now = ctx.input(|i| i.time);
    let since = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || now));
    if now - since < CARD_DELAY {
        ctx.request_repaint_after_secs((CARD_DELAY - (now - since)) as f32);
        return resp;
    }
    let words = text();
    let style = ctx.global_style();
    let frame = card_frame(&style);
    let (font, limit) = (egui::TextStyle::Body.resolve(&style), style.spacing.tooltip_width);
    let galley = ctx.fonts_mut(|f| f.layout(words.clone(), font, INK, limit));
    let (pivot, at) = place(galley.size().x + frame.total_margin().sum().x);
    card_area(&ctx, id, frame, pivot, at, |ui| {
        ui.label(words);
    });
    resp
}

fn card_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::popup(style).fill(ISLAND)
}

/// A card's own layer: over everything, and the pen passes through it.
fn card_area(
    ctx: &egui::Context,
    id: egui::Id,
    frame: egui::Frame,
    pivot: egui::Align2,
    at: Pos2,
    add: impl FnOnce(&mut Ui),
) {
    egui::Area::new(id)
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fade_in(false)
        .constrain_to(ctx.content_rect())
        .pivot(pivot)
        .fixed_pos(at)
        .show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_max_width(ui.spacing().tooltip_width);
                add(ui);
            });
        });
}

/// AU10: the layer of what stands beside the return dot.
pub fn dot_card_id() -> egui::Id {
    egui::Id::new("return_dot").with("card")
}

/// AU10: a card that needs no resting, clear of the dot as any card is, and
/// level with it however many lines its words take.
fn dot_card_in(dot: &Response, frame: egui::Frame, add: impl FnOnce(&mut Ui)) {
    let at = dot.rect.right_center() + Vec2::new(8.0, 0.0);
    card_area(&dot.ctx, dot_card_id(), frame, egui::Align2::LEFT_CENTER, at, add);
}

/// AU10: what stands beside the return dot with no pointer on it.
pub fn dot_card(dot: &Response, words: &str) {
    dot_card_in(dot, card_frame(&dot.ctx.global_style()), |ui| {
        ui.label(words);
    });
}

/// AU10: a refusal beside the return dot: the card is its wash, so the words
/// stand in one outline, not a pill in a card.
pub fn dot_refusal(dot: &Response, words: &str) {
    let frame = card_frame(&dot.ctx.global_style()).fill(RED_WASH);
    dot_card_in(dot, frame, |ui| {
        ui.horizontal(|ui| refusal_words(ui, words));
    });
}

/// The ink a rail key stands in: solid blue armed, the wash while it holds a
/// float open, a ghost at rest.
fn rail_inks(ui: &Ui, resp: &Response, armed: bool, opened: bool) -> (Color32, Color32) {
    if armed {
        (TALLY, ON_FILL)
    } else if opened {
        (BLUE_WASH, BLUE_DEEP)
    } else {
        ghost(ui, resp)
    }
}

/// A rail key: `size`, its icon at [`RAIL_ICON`], and its caption at the Small
/// role, under the icon (`None` in the shorter tiers). `sense` is `click`, or
/// `drag` for a key that acts while it is held. `options` draws the small
/// mark that says a click on this armed tool opens its options.
#[allow(clippy::too_many_arguments)] // a key's whole face; a struct would be noise
pub fn rail_key(
    ui: &mut Ui,
    size: Vec2,
    sense: egui::Sense,
    icon: crate::icons::Icon,
    caption: Option<&str>,
    name: &str,
    armed: bool,
    opened: bool,
    options: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let (fill, ink) = rail_inks(ui, &resp, armed, opened);
        let p = ui.painter();
        p.rect_filled(rect, R_CONTROL, fill);
        let centre = match caption {
            Some(_) => Pos2::new(rect.center().x, rect.top() + 4.0 + RAIL_ICON * 0.5),
            None => rect.center(),
        };
        crate::icons::paint(p, egui::Rect::from_center_size(centre, Vec2::splat(RAIL_ICON)), ink, icon);
        if let Some(words) = caption {
            let font = Role::Small.font();
            p.text(Pos2::new(rect.center().x, rect.bottom() - 11.0), egui::Align2::CENTER_CENTER, words, font, ink);
        }
        // In the top corner: the caption runs out to the bottom one.
        if options {
            let (r, t) = (rect.right() - 4.0, rect.top() + 4.0);
            let mark = vec![Pos2::new(r, t), Pos2::new(r, t + 6.0), Pos2::new(r - 6.0, t)];
            p.add(Shape::convex_polygon(mark, ink, Stroke::NONE));
        }
    }
    speak(&resp, egui::WidgetType::SelectableLabel, Some(armed), name);
    describe(resp, "RAIL KEY rail_key", name)
}

/// The rail's Size key, drag only: a line of dots growing from small to big,
/// marked at `t` (0 to 1) along its length, big end up.
pub fn size_key(ui: &mut Ui, size: Vec2, caption: Option<&str>, t: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::drag());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = ghost(ui, &resp);
        let p = ui.painter();
        p.rect_filled(rect, R_CONTROL, fill);
        let bottom = rect.bottom() - if caption.is_some() { 22.0 } else { 6.0 };
        let (top, cx) = (rect.top() + 6.0, rect.center().x);
        const DOTS: [f32; 5] = [1.5, 2.5, 3.5, 4.5, 6.0];
        let gap = (bottom - top - 2.0 * DOTS.iter().sum::<f32>()) / (DOTS.len() - 1) as f32;
        let mut foot = bottom;
        for r in DOTS {
            p.circle_filled(Pos2::new(cx, foot - r), r, at_rest(ui, INK3));
            foot -= 2.0 * r + gap;
        }
        let y = bottom + (top - bottom) * t.clamp(0.0, 1.0);
        let bar = egui::Rect::from_center_size(Pos2::new(cx, y), Vec2::new(22.0, 3.0));
        p.rect_filled(bar, 1, ink);
        if let Some(words) = caption {
            let font = Role::Small.font();
            p.text(Pos2::new(cx, rect.bottom() - 11.0), egui::Align2::CENTER_CENTER, words, font, ink);
        }
    }
    speak(&resp, egui::WidgetType::Slider, None, "Size");
    describe(resp, "RAIL KEY size_key", "Size")
}

/// The rail's Color key: a swatch of `colour`, its caption under it. `opened`
/// says the Color float is up.
pub fn color_key(
    ui: &mut Ui,
    size: Vec2,
    caption: Option<&str>,
    colour: Color32,
    opened: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = rail_inks(ui, &resp, false, opened);
        let p = ui.painter();
        p.rect_filled(rect, R_CONTROL, fill);
        let centre = match caption {
            Some(_) => Pos2::new(rect.center().x, rect.top() + 4.0 + RAIL_ICON * 0.5),
            None => rect.center(),
        };
        let well = egui::Rect::from_center_size(centre, Vec2::splat(RAIL_ICON));
        p.rect_filled(well, R_INPUT, colour);
        p.rect_stroke(well, R_INPUT, Stroke::new(1.0, INK3), egui::StrokeKind::Outside);
        if let Some(words) = caption {
            let font = Role::Small.font();
            p.text(Pos2::new(rect.center().x, rect.bottom() - 11.0), egui::Align2::CENTER_CENTER, words, font, ink);
        }
    }
    speak(&resp, egui::WidgetType::Button, None, "Brush color");
    describe(resp, "RAIL KEY color_key", "Brush color")
}

/// A float's close key: a ghost at the heading's height.
pub fn close_key(ui: &mut Ui, name: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(CHROME_H - 4.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = ghost(ui, &resp);
        let p = ui.painter();
        p.rect_filled(rect, R_CONTROL, fill);
        let cross = egui::Rect::from_center_size(rect.center(), Vec2::splat(ICON));
        crate::icons::paint(p, cross, ink, crate::icons::Icon::Close);
    }
    speak(&resp, egui::WidgetType::Button, None, name);
    describe(resp, "ACTION close_key", name)
}

/// How far apart two opaque colours read: WCAG's ratio, 1 to 21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let lum = |c: Color32| {
        let l = egui::Rgba::from(c);
        0.2126 * l.r() + 0.7152 * l.g() + 0.0722 * l.b()
    };
    let (a, b) = (lum(a), lum(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// A layer's identity dot, shown on an island or in a field. One too close
/// to either ground takes an `INK3` ring: stored chips stay what they are, so
/// the dot is what has to stay findable.
pub fn identity_dot(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    painter.circle_filled(center, radius, color);
    if [ISLAND, WELL].iter().any(|ground| contrast(color, *ground) < 1.5) {
        painter.circle_stroke(center, radius, Stroke::new(1.0, INK3));
    }
}

/// The sound track's note, drawn: the JP face's ♪ hangs under the line.
pub fn note_mark(painter: &egui::Painter, center: Pos2, ink: Color32) {
    let foot = Pos2::new(center.x + 1.5, center.y + 3.0);
    let top = Pos2::new(foot.x, center.y - 5.0);
    painter.circle_filled(Pos2::new(center.x - 0.5, foot.y), 2.5, ink);
    painter.line_segment([foot, top], Stroke::new(1.0, ink));
    painter.line_segment([top, Pos2::new(center.x + 5.0, center.y - 2.5)], Stroke::new(1.0, ink));
}

/// The peg bar's steel: the one dark surface in the chrome (spec 2.1), over
/// the strip's sheets and as the rule on the Start float.
pub fn peg_bar(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, 0.0, STEEL);
}

/// One round peg hole.
pub fn peg_hole(painter: &egui::Painter, center: Pos2) {
    painter.circle_filled(center, 2.5, DESK);
}

/// A word over the picture, on an island of its own: no ink holds on every
/// painted ground. Returns the word's rect.
pub fn picture_tag(painter: &egui::Painter, at: Pos2, word: &str) -> egui::Rect {
    let galley = painter.layout_no_wrap(word.to_owned(), Role::Small.font(), INK2);
    let rect = egui::Rect::from_min_size(at, galley.size());
    painter.rect_filled(rect.expand2(Vec2::new(6.0, 2.0)), R_CONTROL, ISLAND);
    painter.galley(at, galley, INK2);
    rect
}

/// [`picture_tag`] centred on `at`, in panel type: a note the hand reads
/// while it works.
pub fn picture_note(painter: &egui::Painter, at: Pos2, word: &str) -> egui::Rect {
    let galley = painter.layout_no_wrap(word.to_owned(), Role::Panel.font(), INK2);
    let rect = egui::Align2::CENTER_CENTER.anchor_size(at, galley.size());
    painter.rect_filled(rect.expand2(Vec2::new(8.0, 3.0)), R_CONTROL, ISLAND);
    painter.galley(rect.min, galley, INK2);
    rect
}

/// A row of exactly `size`, whatever goes in it. Its contents can neither
/// push what sits under it nor paint out of it: `allocate_ui_with_layout`
/// takes the height of what it holds, so a row that shares its space with a
/// canvas moves the canvas by whatever was armed.
pub fn fixed_row<R>(ui: &mut Ui, size: Vec2, add: impl FnOnce(&mut Ui) -> R) -> R {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
    row.shrink_clip_rect(rect);
    add(&mut row)
}

/// A row off the window is a row out of reach. A menu of `rows` too long for
/// the window at a panel's height stands at a sheet row's, a point of gap
/// under what every menu stood at before the new look. Answers how tall the
/// menu stands, so one still too tall can scroll.
pub fn fit_menu(ui: &mut Ui, rows: usize) -> f32 {
    let spacing = ui.spacing();
    let tall = rows as f32 * (CTRL_H + spacing.item_spacing.y) + spacing.menu_margin.sum().y;
    if tall <= ui.ctx().content_rect().height() {
        return tall;
    }
    ui.spacing_mut().interact_size.y = ROW_H;
    ui.spacing_mut().item_spacing.y = SHEET_GAP;
    rows as f32 * (ROW_H + SHEET_GAP) + ui.spacing().menu_margin.sum().y
}

/// The dock in the look: tab bars on the desk, and AU15 pill tabs, the current
/// one in the wash (spec 2.8), as the steps on the top row are.
pub fn dock_style(style: &egui::Style) -> egui_dock::Style {
    let mut dock = egui_dock::Style::from_egui(style);
    dock.tab.tab_body.inner_margin = egui::Margin::same(PANE_MARGIN);
    dock.tab_bar.bg_fill = DESK;
    dock.tab_bar.hline_color = RULE;
    // Desk on the desk: rounding it paints nothing, at a price.
    dock.tab_bar.corner_radius = CornerRadius::ZERO;
    // A ghost at rest has no fill and an outline none would see: an outline in
    // a tab's own fill costs a room hundreds of vertices for nothing.
    let quiet = [&mut dock.tab.inactive, &mut dock.tab.inactive_with_kb_focus];
    for tab in quiet {
        tab.bg_fill = Color32::TRANSPARENT;
        tab.text_color = INK2;
        tab.outline_color = Color32::TRANSPARENT;
    }
    dock.tab.hovered.bg_fill = WELL;
    dock.tab.hovered.text_color = INK;
    dock.tab.hovered.outline_color = Color32::TRANSPARENT;
    let lit = [
        &mut dock.tab.active,
        &mut dock.tab.focused,
        &mut dock.tab.active_with_kb_focus,
        &mut dock.tab.focused_with_kb_focus,
    ];
    for tab in lit {
        tab.bg_fill = BLUE_WASH;
        tab.text_color = BLUE_DEEP;
        tab.outline_color = Color32::TRANSPARENT;
    }
    // A stadium the bar's height less its inset, exactly: egui_dock joins a tab
    // to its body along the bottom between the corners it is told of.
    let stadium = CornerRadius::same(TAB_RADIUS);
    for tab in [
        &mut dock.tab.active,
        &mut dock.tab.focused,
        &mut dock.tab.active_with_kb_focus,
        &mut dock.tab.focused_with_kb_focus,
        &mut dock.tab.inactive,
        &mut dock.tab.inactive_with_kb_focus,
        &mut dock.tab.hovered,
    ] {
        tab.corner_radius = stadium;
    }
    dock.tab.tab_body.stroke = Stroke::NONE;
    // Its top corners stand square under the bar, which rounds them: a corner
    // nobody sees costs a room's panes their vertices all the same.
    dock.tab.tab_body.corner_radius = CornerRadius { nw: 0, ne: 0, sw: R_CONTROL, se: R_CONTROL };
    dock
}

// ---------------------------------------------------------------------------
// Words: the two inks as text helpers.
// ---------------------------------------------------------------------------

/// A legend's words in sentence case: the first letter up and the rest as
/// written, so a name or an abbreviation inside keeps its capitals.
pub fn sentence(text: &str) -> String {
    let mut letters = text.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

/// A word field's size (see [`text_field`]).
pub const FIELD_TEXT: f32 = 13.0;

/// A legend's size. Spec 2.5 gives it 12 or 13; at 13 its capitals paint 9 px,
/// which keeps the small-text share under the AU12 target.
pub const LEGEND_SIZE: f32 = 13.0;

/// The legend's font, for a head painted by hand.
pub fn legend_font() -> FontId {
    FontId::new(LEGEND_SIZE, semibold())
}

/// Panel titles, column heads, group legends: sentence case, 13 pt SemiBold,
/// `INK2`.
pub fn legend(ui: &mut Ui, text: &str) -> Response {
    let mut job = LayoutJob::default();
    job.append(
        &sentence(text),
        0.0,
        TextFormat {
            font_id: legend_font(),
            color: LEGEND,
            // A wrapped row stretches a label to its own height: unset, the
            // letters sit on that floor, under the middle of what they name.
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    ui.label(job)
}

/// A value the animator authored or is reading: Panel type, `INK`.
pub fn value(ui: &mut Ui, text: &str) -> Response {
    let mut job = LayoutJob::default();
    job.append(
        text,
        0.0,
        TextFormat {
            font_id: Role::Panel.font(),
            color: STRUCK,
            ..Default::default()
        },
    );
    ui.label(job)
}

/// The Say line's one size, and the smaller one its second row asks for.
pub const SAY_TEXT: f32 = 14.0;
pub const SAY_SMALL: f32 = 12.0;
/// The room held for a hint's action button (spec 8.2): the longest of its
/// words at control size, its pad and the gap before it.
pub const SAY_ACTION: f32 = 105.0;
/// Spec 8.3's five dots: the room they take, and the narrowest lane that
/// keeps the longest sentence on its two rows beside them.
pub const SAY_DOTS: f32 = 56.0;
pub const SAY_DOTS_LANE: f32 = 410.0;

/// The Say line's words (spec 4.4): one line, or two smaller ones inside the
/// row when one will not hold them. What two cannot hold stops on a `…`; its
/// card has the rest.
pub fn say(ui: &mut Ui, words: &str, ink: Color32) -> Response {
    let wide = ui.available_width();
    let size = if text_width(ui, words, SAY_TEXT) <= wide { SAY_TEXT } else { SAY_SMALL };
    let format = egui::TextFormat::simple(FontId::proportional(size), ink);
    let mut job = egui::text::LayoutJob::single_section(words.to_string(), format);
    job.wrap = egui::text::TextWrapping {
        max_width: wide,
        max_rows: 2,
        ..egui::text::TextWrapping::default()
    };
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let elided = galley.elided;
    let resp = ui.add(egui::Label::new(galley).show_tooltip_when_elided(false));
    if elided { resp.card(words) } else { resp }
}

/// A refusal: the wash, red words and a mark, so it never rests on colour.
/// Long words stop at the end of the lane; the card has the rest.
pub fn refusal(ui: &mut Ui, words: &str) -> Response {
    egui::Frame::new()
        .fill(RED_WASH)
        .corner_radius(R_PILL)
        .inner_margin(egui::Margin::symmetric(8, 1))
        .show(ui, |ui| refusal_words(ui, words))
        .inner
}

fn refusal_words(ui: &mut Ui, words: &str) -> Response {
    ui.spacing_mut().item_spacing.x = 4.0;
    refusal_mark(ui);
    say(ui, words, RED)
}

fn refusal_mark(ui: &mut Ui) {
    let (mark, _) = ui.allocate_exact_size(Vec2::new(10.0, 16.0), Sense::hover());
    let mark = egui::Rect::from_center_size(mark.center(), Vec2::splat(7.0));
    let ink = Stroke::new(1.5, RED);
    ui.painter().line_segment([mark.left_top(), mark.right_bottom()], ink);
    ui.painter().line_segment([mark.right_top(), mark.left_bottom()], ink);
}

/// AU9: a failure that stays on screen: a [`refusal`]'s wash, mark and red
/// words, each word of them however many lines that takes, and what can be
/// done about it under them.
pub fn failure(ui: &mut Ui, words: &str, then: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(RED_WASH)
        .corner_radius(R_CONTROL)
        .inner_margin(egui::Margin::symmetric(CARD_PAD as i8, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                refusal_mark(ui);
                ui.add(egui::Label::new(egui::RichText::new(words).color(RED)).wrap());
            });
            then(ui);
        });
}

/// How wide the Say line's words stand at their full size.
pub fn say_width(ui: &Ui, words: &str) -> f32 {
    text_width(ui, words, SAY_TEXT)
}

/// How wide a hint's action button stands.
pub fn say_action_width(ui: &Ui, words: &str) -> f32 {
    text_width(ui, words, CTRL_TEXT) + 2.0 * ui.spacing().button_padding.x
}

/// The action button of a Say line hint, ending at the right end of `row`: a
/// filled control, never solid blue, because a hint is not the screen's main
/// action. It is spoken apart from the Help window's rows and a pane's own
/// button.
pub fn say_action(ui: &mut Ui, row: egui::Rect, words: &str) -> Response {
    let wide = say_action_width(ui, words);
    let rect = egui::Rect::from_center_size(row.center(), Vec2::new(wide, CTRL_H));
    let rect = rect.translate(Vec2::X * (row.right() - rect.right()));
    let resp = ui.allocate_rect(rect, Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, _) = filled(&resp);
        ui.painter().rect_filled(rect, R_CONTROL, fill);
        label_at(ui, rect.left() + ui.spacing().button_padding.x, rect.center().y, words, CTRL_TEXT, INK);
    }
    speak(&resp, egui::WidgetType::Button, None, &format!("{words} (say line)"));
    describe(resp, "ACTION say_action", words)
}

/// The five dots (spec 8.3), at the right end of `row`: filled to where the
/// person has reached, and the next one ringed. What comes back only hovers,
/// and takes their whole slot, so a 6 pt dot needs no aim to wear a card.
pub fn say_dots(ui: &Ui, row: egui::Rect, ticks: &[bool; 5]) -> Response {
    let (step, radius) = (9.0, 3.0);
    let first = row.right() - SAY_DOTS + radius + 4.0;
    let next = ticks.iter().position(|done| !done);
    for (i, done) in ticks.iter().enumerate() {
        let at = Pos2::new(first + step * i as f32, row.center().y);
        let ink = if *done || next == Some(i) { TALLY } else { INK3 };
        if *done {
            ui.painter().circle_filled(at, radius, ink);
        } else {
            ui.painter().circle_stroke(at, radius - 0.5, Stroke::new(1.0, ink));
        }
    }
    ui.interact(row.with_min_x(row.right() - SAY_DOTS), ui.id().with("say dots"), Sense::hover())
}

/// A painted control's content at rest: `rest`, or `INK` inside a disabled
/// scope, where the scope does the dimming and a dimmer ink would fall under
/// its floor (spec 2.3).
fn at_rest(ui: &Ui, rest: Color32) -> Color32 {
    if ui.is_enabled() { rest } else { INK }
}

/// A painted control's ground: `fill`, or none inside a disabled scope.
fn ground_of(ui: &Ui, fill: Color32) -> Color32 {
    if ui.is_enabled() { fill } else { Color32::TRANSPARENT }
}

// ---------------------------------------------------------------------------
// Affordances (spec 2.8). Fills and inks by state; small controls carry no
// strokes, because a stroke per chip costs vertices the performance gate
// counts. Nothing else in the app may present a control.
// ---------------------------------------------------------------------------

/// The ring's weight, and how far it stands off the control.
const FOCUS_RING: f32 = 2.0;

/// Keyboard focus, on whatever holds it. egui lends a focused widget its
/// pressed look and paints nothing around a hand-painted one, so the ring
/// (spec 2.8) goes round the focused widget once per pass, from here: no
/// control can forget it. `dress` installs this. A field's own edge keeps
/// `selection.stroke` (spec 2.9); the ring stands outside it.
pub struct FocusRing;

impl egui::Plugin for FocusRing {
    fn debug_name(&self) -> &'static str {
        "plate::FocusRing"
    }

    fn on_end_pass(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let Some(id) = ctx.memory(|m| m.focused()) else {
            return;
        };
        // The rect, not `read_response`: that replays the widget's response,
        // and a click elsewhere this pass surrenders focus just given to it.
        let Some(widget) = ctx.viewport(|v| v.this_pass.widgets.get(id).copied()) else {
            return;
        };
        // What the clip leaves of it: a focused widget may be scrolled away,
        // or laid out off screen to keep its keys.
        if !widget.interact_rect.is_positive() {
            return;
        }
        let radius = R_CONTROL + (2.0 * FOCUS_RING) as u8;
        ctx.layer_painter(widget.layer_id).rect_stroke(
            widget.interact_rect.expand(FOCUS_RING),
            radius,
            Stroke::new(FOCUS_RING, BLUE),
            StrokeKind::Outside,
        );
    }
}

/// A small painted toggle's face (the sheet's column and track chips): no
/// edge, the `WELL` ground, and a wash with its own ink while on. `on` is
/// that wash and ink; `None` is a chip with no on state, or one that is off.
pub fn chip_face(
    ui: &Ui,
    r: egui::Rect,
    resp: &Response,
    words: &str,
    on: Option<(Color32, Color32)>,
) {
    let (fill, ink) = match on {
        Some(lit) => lit,
        None if resp.hovered() => (HOVER, INK),
        None => (WELL, INK2),
    };
    let p = ui.painter();
    p.rect_filled(r.shrink(1.0), R_SHEET, ground_of(ui, fill));
    p.text(r.center(), egui::Align2::CENTER_CENTER, words, Role::Small.font(), at_rest(ui, ink));
}

/// The sheet head's hide mark on its chip: an open eye, shut while the track
/// is hidden.
pub fn eye_mark(ui: &Ui, center: Pos2, hidden: bool) {
    let icon = if hidden { Icon::EyeOff } else { Icon::Eye };
    mark_icon(ui, center, icon, hidden);
}

/// The sheet head's lock mark on its chip.
pub fn lock_mark(ui: &Ui, center: Pos2, locked: bool) {
    mark_icon(ui, center, Icon::Lock, locked);
}

fn mark_icon(ui: &Ui, center: Pos2, icon: Icon, on: bool) {
    let ink = at_rest(ui, if on { INK } else { INK2 });
    let rect = egui::Rect::from_center_size(center, Vec2::splat(Role::Chrome.size()));
    crate::icons::paint(ui.painter(), rect, ink, icon);
}

/// DETENT — a radio position. An `INK3` ring; a filled blue disc when this
/// position is armed. Mutually exclusive with its neighbours; clicking an
/// armed detent does nothing (the caller simply sets the same value again).
pub fn detent(ui: &mut Ui, armed: bool, label: &str) -> Response {
    detent_inked(ui, armed, label, false)
}

/// GM3: a detent for a position out of reach now (an undone history step,
/// a command that cannot run) — the same control in dim ink. It still
/// answers a click; the caller says why when it refuses.
pub fn detent_dim(ui: &mut Ui, armed: bool, label: &str) -> Response {
    detent_inked(ui, armed, label, true)
}

/// A [`detent`] with its fact on the row, in the quieter ink: `word` says
/// what it is and `fact` what that means. The row is one control, named
/// `word`, and `widest`, the group's longest word, holds the facts in line.
pub fn detent_fact(ui: &mut Ui, armed: bool, word: &str, fact: &str, widest: &str) -> Response {
    let fact_x = DETENT_W + 4.0 + text_width(ui, widest, CTRL_TEXT) + 12.0;
    let width = fact_x + text_width(ui, fact, CTRL_TEXT) + 8.0;
    let (rect, resp) = detent_sized(ui, armed, word, false, width);
    if ui.is_rect_visible(rect) {
        label_at(ui, rect.left() + fact_x, rect.center().y, fact, CTRL_TEXT, at_rest(ui, INK3));
    }
    resp
}

fn detent_inked(ui: &mut Ui, armed: bool, label: &str, dim: bool) -> Response {
    let width = DETENT_W + 4.0 + text_width(ui, label, CTRL_TEXT) + 8.0;
    detent_sized(ui, armed, label, dim, width).1
}

/// A detent `width` wide: the rect its face took, and the control.
fn detent_sized(ui: &mut Ui, armed: bool, label: &str, dim: bool, width: f32) -> (egui::Rect, Response) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, CTRL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let ppp = ui.ctx().pixels_per_point();
        let c = Pos2::new(
            snap(rect.left() + DETENT_W * 0.5, ppp),
            snap(rect.center().y, ppp),
        );
        p.circle_stroke(c, 6.0, Stroke::new(1.0, at_rest(ui, INK3)));
        if armed {
            p.circle_filled(c, 3.5, at_rest(ui, TALLY));
        }
        let rest = at_rest(ui, if dim { INK3 } else { INK2 });
        let ink = if armed || resp.hovered() { STRUCK } else { rest };
        label_at(
            ui,
            rect.left() + DETENT_W + 4.0,
            rect.center().y,
            label,
            CTRL_TEXT,
            ink,
        );
    }
    speak(&resp, egui::WidgetType::SelectableLabel, Some(armed), label);
    (rect, describe(resp, "DETENT detent", label))
}

/// The width a [`latch`] takes, for a row that places latches by hand.
pub fn latch_width(ui: &Ui, label: &str) -> f32 {
    LATCH_TEXT_X + text_width(ui, label, CTRL_TEXT) + 8.0
}

/// LATCH — an independent on/off, drawn as the checkbox it is: a 16 pt box
/// with an `INK3` edge, blue with a white tick while latched. Clicking
/// toggles.
pub fn latch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    latch_drawn(ui, on, label, true)
}

/// A [`latch`] with no solid blue: an ink tick in the neutral box. For a
/// screen whose two solid blue things are spoken for (spec 2.1).
pub fn quiet_latch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    latch_drawn(ui, on, label, false)
}

/// The tick of a latch's 16 pt box.
fn paint_tick(p: &egui::Painter, tick_box: egui::Rect, ink: Color32) {
    let at = |x: f32, y: f32| tick_box.min + Vec2::new(x, y);
    let tick = vec![at(3.5, 8.0), at(6.5, 11.0), at(12.5, 5.0)];
    p.add(Shape::line(tick, Stroke::new(2.0, ink)));
}

fn latch_drawn(ui: &mut Ui, on: &mut bool, label: &str, loud: bool) -> Response {
    let (rect, mut resp) =
        ui.allocate_exact_size(Vec2::new(latch_width(ui, label), CTRL_H), Sense::click());
    if resp.clicked() {
        *on = !*on;
        // Callers gate saves on .changed(), as they do for checkboxes.
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let ppp = ui.ctx().pixels_per_point();
        let tick_box = egui::Rect::from_min_size(
            Pos2::new(snap(rect.left(), ppp), snap(rect.center().y - LATCH_BOX * 0.5, ppp)),
            Vec2::splat(LATCH_BOX),
        );
        let solid = loud && *on && ui.is_enabled();
        if solid {
            p.rect_filled(tick_box, R_SHEET, TALLY);
        } else {
            let edge = if resp.hovered() { LEGEND } else { at_rest(ui, INK3) };
            p.rect_stroke(tick_box, R_SHEET, Stroke::new(1.0, edge), StrokeKind::Inside);
        }
        if *on {
            paint_tick(p, tick_box, if solid { ON_FILL } else { INK });
        }
        let ink = if *on || resp.hovered() {
            STRUCK
        } else {
            at_rest(ui, LEGEND)
        };
        label_at(ui, rect.left() + LATCH_TEXT_X, rect.center().y, label, CTRL_TEXT, ink);
    }
    speak(&resp, egui::WidgetType::Checkbox, Some(*on), label);
    describe(resp, "LATCH latch", label)
}

/// A filled control's ground and content by state: `WELL` at rest, `HOVER`
/// under the hand, `PRESSED` held. The icon rests in `INK2`. Nothing filled
/// here reads a disabled scope; the painter dims it.
pub fn filled(resp: &Response) -> (Color32, Color32) {
    if resp.is_pointer_button_down_on() {
        (PRESSED, INK)
    } else if resp.hovered() {
        (HOVER, INK)
    } else {
        (WELL, INK2)
    }
}

/// DETENT with a painted icon — a tool position. The armed position is the
/// solid blue shape with white content. Larger targets than bare text
/// (owner's amendment: sparse space is spent on bigger controls). `label`
/// is the painted caption (empty = icon only); `name` is what the control
/// answers to (empty = the caption).
pub fn tool_button(
    ui: &mut Ui,
    armed: bool,
    icon: crate::icons::Icon,
    label: &str,
    name: &str,
) -> Response {
    let text_w = if label.is_empty() {
        0.0
    } else {
        text_width(ui, label, CTRL_TEXT) + 5.0
    };
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(8.0 + ICON + text_w + 8.0, CTRL_H),
        Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let (ground, rest) = filled(&resp);
        let (fill, ink, words) = if armed && ui.is_enabled() {
            (TALLY, ON_FILL, ON_FILL)
        } else {
            (ground_of(ui, ground), at_rest(ui, rest), STRUCK)
        };
        p.rect_filled(rect, R_CONTROL, fill);
        let icon_rect = egui::Rect::from_center_size(
            Pos2::new(rect.left() + 8.0 + ICON * 0.5, rect.center().y),
            Vec2::splat(ICON),
        );
        crate::icons::paint(p, icon_rect, ink, icon);
        if !label.is_empty() {
            label_at(
                ui,
                rect.left() + 8.0 + ICON + 5.0,
                rect.center().y,
                label,
                CTRL_TEXT,
                words,
            );
        }
    }
    let name = tool_name(name, label, icon);
    speak(&resp, egui::WidgetType::SelectableLabel, Some(armed), &name);
    describe(resp, "DETENT tool_button", label)
}

/// LATCH with a painted icon — independent on/off. Latched takes the wash
/// with deep ink: at most one tool is solid blue on screen, and any number
/// of latches can be on. `label` and `name` as for [`tool_button`].
pub fn tool_latch(
    ui: &mut Ui,
    on: &mut bool,
    icon: crate::icons::Icon,
    label: &str,
    name: &str,
) -> Response {
    latch_sized(ui, on, icon, label, name, ICON)
}

/// A [`tool_latch`] on the band, at [`BAR_ICON`].
pub fn band_latch(
    ui: &mut Ui,
    on: &mut bool,
    icon: crate::icons::Icon,
    label: &str,
    name: &str,
) -> Response {
    latch_sized(ui, on, icon, label, name, BAR_ICON)
}

/// The room a [`band_action`] or a [`band_latch`] called `label` stands in,
/// held empty until it arrives (spec 4.4).
pub fn band_slot(ui: &mut Ui, label: &str) {
    ui.allocate_exact_size(latch_size(ui, label, BAR_ICON), Sense::hover());
}

/// The room a [`band_key`] stands in, held empty until it arrives.
pub fn band_key_slot(ui: &mut Ui) {
    ui.allocate_exact_size(Vec2::new(6.0 + BAR_ICON + 6.0, CHROME_H), Sense::hover());
}

/// The band's counter: its words in a ghost key as wide as `widest` reads,
/// so a digit rolling over moves nothing (D7).
pub fn counter_key(ui: &mut Ui, words: &str, widest: &str) -> Response {
    let w = text_width(ui, widest, CTRL_TEXT) + 16.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, CHROME_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = ghost(ui, &resp);
        ui.painter().rect_filled(rect, R_CONTROL, fill);
        label_at(ui, rect.left() + 8.0, rect.center().y, words, CTRL_TEXT, ink);
    }
    speak(&resp, egui::WidgetType::Button, None, words);
    describe(resp, "ACTION counter_key", words)
}

/// [`band_latch`]'s face on a plain action: as wide, never lit, and it
/// answers as a button, because there is nothing to latch.
pub fn band_action(ui: &mut Ui, icon: crate::icons::Icon, label: &str, name: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(latch_size(ui, label, BAR_ICON), Sense::click());
    if ui.is_rect_visible(rect) {
        latch_face(ui, rect, &resp, false, icon, label, BAR_ICON);
    }
    let name = tool_name(name, label, icon);
    speak(&resp, egui::WidgetType::Button, None, &name);
    describe(resp, "ACTION band_action", label)
}

fn latch_sized(
    ui: &mut Ui,
    on: &mut bool,
    icon: crate::icons::Icon,
    label: &str,
    name: &str,
    size: f32,
) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(latch_size(ui, label, size), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        latch_face(ui, rect, &resp, *on, icon, label, size);
    }
    let name = tool_name(name, label, icon);
    speak(&resp, egui::WidgetType::Checkbox, Some(*on), &name);
    describe(resp, "LATCH tool_latch", label)
}

/// A latch stands its icon at `size`, its caption and the pad either side.
fn latch_size(ui: &Ui, label: &str, size: f32) -> Vec2 {
    let text_w = if label.is_empty() {
        0.0
    } else {
        text_width(ui, label, CTRL_TEXT) + 5.0
    };
    Vec2::new(8.0 + size + text_w + 8.0, height_for(size))
}

/// A band control stands with the top row (spec 2.6), a panel's at `CTRL_H`.
fn height_for(icon: f32) -> f32 {
    if icon == BAR_ICON { CHROME_H } else { CTRL_H }
}

fn latch_face(
    ui: &Ui,
    rect: egui::Rect,
    resp: &Response,
    on: bool,
    icon: crate::icons::Icon,
    label: &str,
    size: f32,
) {
    let p = ui.painter();
    let (ground, rest) = filled(resp);
    let (fill, ink, words) = if on && ui.is_enabled() {
        (BLUE_WASH, BLUE_DEEP, BLUE_DEEP)
    } else {
        (ground_of(ui, ground), at_rest(ui, rest), STRUCK)
    };
    p.rect_filled(rect, R_CONTROL, fill);
    let icon_rect = egui::Rect::from_center_size(
        Pos2::new(rect.left() + 8.0 + size * 0.5, rect.center().y),
        Vec2::splat(size),
    );
    crate::icons::paint(p, icon_rect, ink, icon);
    if !label.is_empty() {
        label_at(
            ui,
            rect.left() + 8.0 + size + 5.0,
            rect.center().y,
            label,
            CTRL_TEXT,
            words,
        );
    }
}

/// A momentary action with a painted icon (no armed state — fires on click):
/// a ghost, no fill at rest, `WELL` under the hand, the wash while pressed.
pub fn icon_button(ui: &mut Ui, icon: crate::icons::Icon, label: &str, hover: &str) -> Response {
    let resp = icon_button_sized(ui, icon, label, hover, ICON);
    if hover.is_empty() { resp } else { resp.card(hover) }
}

/// A transport key: an [`icon_button`] at [`BAR_ICON`], with no caption.
pub fn band_key(ui: &mut Ui, icon: crate::icons::Icon, hover: &str) -> Response {
    let resp = icon_button_sized(ui, icon, "", hover, BAR_ICON);
    if hover.is_empty() { resp } else { resp.card(hover) }
}

/// A [`band_key`] that says `name` and leaves its card to the caller, whose
/// card reads the live chord.
pub fn band_key_named(ui: &mut Ui, icon: crate::icons::Icon, name: &str) -> Response {
    icon_button_sized(ui, icon, "", name, BAR_ICON)
}

fn icon_button_sized(
    ui: &mut Ui,
    icon: crate::icons::Icon,
    label: &str,
    hover: &str,
    size: f32,
) -> Response {
    let text_w = if label.is_empty() {
        0.0
    } else {
        text_width(ui, label, CTRL_TEXT) + 5.0
    };
    let (rect, resp) = ui
        .allocate_exact_size(Vec2::new(6.0 + size + text_w + 6.0, height_for(size)), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let (fill, ink) = ghost(ui, &resp);
        p.rect_filled(rect, R_CONTROL, fill);
        let icon_rect = egui::Rect::from_center_size(
            Pos2::new(rect.left() + 6.0 + size * 0.5, rect.center().y),
            Vec2::splat(size),
        );
        crate::icons::paint(p, icon_rect, ink, icon);
        if !label.is_empty() {
            label_at(
                ui,
                rect.left() + 6.0 + size + 5.0,
                rect.center().y,
                label,
                CTRL_TEXT,
                ink,
            );
        }
    }
    let name = match (label.is_empty(), hover.is_empty()) {
        (false, _) => label.to_string(),
        (true, false) => hover.to_string(),
        (true, true) => icon_name(icon),
    };
    speak(&resp, egui::WidgetType::Button, None, &name);
    describe(resp, "ACTION icon_button", label)
}

/// The top row's pills, islands and island keys stand this tall.
pub const PILL_H: f32 = 32.0;
/// Search, styled as an input.
pub const SEARCH_W: f32 = 141.0;
const PILL_PAD: f32 = 14.0;
/// A choice card's words, from its left edge: the Start float's one text edge.
pub const CARD_PAD: f32 = 12.0;
/// AU9: between a control and [`ring`]'s line.
pub const RING_GAP: f32 = 2.0;
/// A card chip's height.
pub const CHIP_H: f32 = CHROME_H - 4.0;
/// A picture card's picture, from its edges, and its words from the picture.
pub const PICTURE_INSET: f32 = 6.0;
/// A ghost row's words, from its ends: a card's edge, so the two line up.
const ROW_PAD: f32 = CARD_PAD;

/// A pill's width: its words at the Step role, and the pad either side.
pub fn pill_width(ui: &Ui, words: &str) -> f32 {
    let font = Role::Step.font();
    let width = ui.fonts_mut(|f| f.layout_no_wrap(words.to_string(), font, INK).size().x);
    width + 2.0 * PILL_PAD
}

/// A step pill on the step island: the current step on the wash, the
/// `primary` one solid blue until it is current, the rest ghosts.
pub fn step_pill(ui: &mut Ui, label: &str, current: bool, primary: bool) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(pill_width(ui, label), PILL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = if current || (!primary && resp.is_pointer_button_down_on()) {
            (BLUE_WASH, BLUE_DEEP)
        } else if primary {
            (if resp.hovered() { BLUE_DEEP } else { BLUE }, ON_FILL)
        } else if resp.hovered() {
            (WELL, INK)
        } else {
            (Color32::TRANSPARENT, INK)
        };
        let p = ui.painter();
        p.rect_filled(rect, R_PILL, fill);
        p.text(rect.center(), egui::Align2::CENTER_CENTER, label, Role::Step.font(), ink);
    }
    speak(&resp, egui::WidgetType::Button, Some(current), label);
    describe(resp, "DETENT step_pill", label)
}

/// AU15: a card's words on one line in `role`, one size down where a narrow
/// pane cannot hold them, and on two lines in the panel size past that.
fn card_words(p: &egui::Painter, rect: egui::Rect, label: &str, role: Role, ink: Color32) {
    let room = rect.width() - 2.0 * CARD_PAD;
    let one_line = |role: Role| p.layout_no_wrap(label.to_owned(), role.font(), ink);
    let words = [role, Role::Heading]
        .into_iter()
        .map(one_line)
        .find(|words| words.size().x <= room)
        .unwrap_or_else(|| p.layout(label.to_owned(), Role::Panel.font(), ink, room));
    let at = Pos2::new(rect.left() + CARD_PAD, rect.center().y - words.size().y / 2.0);
    p.galley(at, words, ink);
}

/// AU6: a big choice on the Start float: a `WELL` card the float's width,
/// `tall` tall, its words on the left in the Card role.
pub fn choice_card(ui: &mut Ui, label: &str, tall: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), tall), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, _) = filled(&resp);
        let p = ui.painter();
        p.rect_filled(rect, R_CONTROL, fill);
        card_words(p, rect, label, Role::Card, INK);
    }
    speak(&resp, egui::WidgetType::Button, None, label);
    describe(resp, "CARD choice_card", label)
}

/// AU9: the one choice a screen leads with: a [`choice_card`] in solid blue,
/// white words on it, in `role`'s size.
pub fn primary_card(ui: &mut Ui, label: &str, tall: f32, role: Role) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), tall), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if resp.hovered() { BLUE_DEEP } else { BLUE };
        let p = ui.painter();
        p.rect_filled(rect, R_CONTROL, fill);
        card_words(p, rect, label, role, ON_FILL);
    }
    speak(&resp, egui::WidgetType::Button, None, label);
    describe(resp, "CARD primary_card", label)
}

/// AU9: Show me's mark: a ring round a control, clear of its edge.
pub fn ring(ui: &Ui, around: egui::Rect) -> egui::Rect {
    ring_on(ui.painter(), around)
}

fn ring_on(painter: &egui::Painter, around: egui::Rect) -> egui::Rect {
    let ring = around.expand(RING_GAP);
    painter.rect_stroke(ring, R_CONTROL + RING_GAP as u8, Stroke::new(2.0, BLUE_DEEP), StrokeKind::Outside);
    ring.expand(2.0)
}

/// AU21: a task's chain this frame (spec 5.5): the wish, ancestor first, the
/// first place each target found, and where the ring stood last.
#[derive(Clone, Default)]
struct Marks {
    wish: Vec<Target>,
    found: Vec<Option<(egui::Rect, egui::LayerId)>>,
    stands: Option<(Target, egui::Rect)>,
    /// Rows drawn this frame, and those already met since they came to be drawn.
    laid: Vec<Target>,
    met: Vec<Target>,
    /// The rect the ring was stroked round: none under a menu.
    #[cfg(test)]
    painted: Option<egui::Rect>,
}

fn marks_id() -> egui::Id {
    egui::Id::new("lesson_marks")
}

/// AU21: the chain this frame's marks look for. Called once, before anything
/// that can mark is drawn.
pub fn wish(ctx: &egui::Context, chain: Vec<Target>) {
    ctx.data_mut(|d| {
        let marks = d.get_temp_mut_or_default::<Marks>(marks_id());
        let laid = std::mem::take(&mut marks.laid);
        marks.met.retain(|t| laid.contains(t) && chain.contains(t));
        marks.found = vec![None; chain.len()];
        marks.wish = chain;
    });
}

/// AU21: does the wish ask for `target`? A scan that only a lesson reads asks
/// this once a frame first, so the working animator's frame pays nothing.
pub fn wishes(ctx: &egui::Context, target: Target) -> bool {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Marks>(marks_id()).wish.contains(&target))
}

/// AU21: the sheet's cells the wish asks for, the same one look.
pub fn wished_cells(ctx: &egui::Context) -> Vec<(u8, u32)> {
    let cell = |t: &Target| if let Target::Cell { track, frame } = *t { Some((track, frame)) } else { None };
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Marks>(marks_id()).wish.iter().filter_map(cell).collect())
}

/// AU21: `target` stands at `rect` in the `ui` it is drawn in, if the wish asks
/// for it. Nothing here reads a word. The first rect of a frame holds, so a row
/// of an open menu stands before the word that opened it, however late that
/// word is drawn. A control scrolled away has no size, and the canvas in its
/// own OS window is not on this screen.
pub fn mark(ui: &Ui, rect: egui::Rect, target: Target) {
    mark_on(ui.ctx(), ui.layer_id(), rect, target);
}

/// AU21: [`mark`] for a control known by its layer.
pub fn mark_on(ctx: &egui::Context, layer: egui::LayerId, rect: egui::Rect, target: Target) {
    #[cfg(test)]
    MARK_COST.with(|c| c.set(c.get() + 1));
    if !rect.is_positive() || ctx.viewport_id() != egui::ViewportId::ROOT {
        return;
    }
    ctx.data_mut(|d| {
        let marks = d.get_temp_mut_or_default::<Marks>(marks_id());
        if let Some(i) = marks.wish.iter().position(|t| *t == target) {
            marks.found[i].get_or_insert((rect, layer));
        }
    });
}

/// AU21: the frame a wished control comes to be drawn, once, however it is
/// laid out: what brings a row into view leaves the person's own scroll alone
/// after that.
pub fn first_sight(ctx: &egui::Context, target: Target) -> bool {
    ctx.data_mut(|d| {
        let marks = d.get_temp_mut_or_default::<Marks>(marks_id());
        if !marks.wish.contains(&target) {
            return false;
        }
        marks.laid.push(target);
        let first = !marks.met.contains(&target);
        if first {
            marks.met.push(target);
        }
        first
    })
}

/// AU21: [`mark`] for a control in a body that scrolls. Scrolled away it has no
/// rect, so a wished one that arrives hidden is brought into view: once, each
/// time it comes to be drawn, and the person's own scroll stands after that.
pub fn mark_row(resp: &Response, target: Target) {
    let ctx = &resp.ctx;
    let hidden = !resp.interact_rect.contains_rect(resp.rect) && ctx.viewport_id() == egui::ViewportId::ROOT;
    if first_sight(ctx, target) && hidden {
        resp.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
    }
    mark_on(ctx, resp.layer_id, resp.interact_rect, target);
}

/// The lesson ring's halo, outside its stroke: `BLUE_DEEP` alone is lost on a
/// `BLUE` pill and on the strip's `STEEL`.
const RING_HALO: f32 = 1.0;

/// AU21: something in front of the control stands over its centre: an open menu
/// or a dialog, and the ring would cross its rows. A float over a pane leaves
/// the rest of the pane to ring.
fn ring_covered(ctx: &egui::Context, target: Target, rect: egui::Rect, own: egui::LayerId) -> bool {
    let beside = |top: egui::LayerId| matches!(target, Target::Pane(_)) && top.order < egui::Order::Foreground;
    ctx.layer_id_at(rect.center()).is_some_and(|top| top != own && !beside(top))
}

/// AU21: the ring on the chain's last target with a rect (spec 5.8), on a bare
/// painter: no `Area`, so `layer_id_at` skips it and the pen passes (R15).
/// `Target::Paper` is found and rings nothing. Kept inside the window, so a
/// pane at its edge keeps every side, with a halo of its own: `panel.ring`'s
/// mark stands on calm ground and has none. A pill's ring is a stadium.
pub fn stroke_ring(ctx: &egui::Context) {
    let stands = ctx.data_mut(|d| {
        let marks = d.get_temp_mut_or_default::<Marks>(marks_id());
        let last = marks.found.iter().rposition(Option::is_some);
        let found = last.and_then(|i| Some((marks.wish[i], marks.found[i]?)));
        marks.stands = found.map(|(target, (rect, _))| (target, rect));
        found
    });
    let room = ctx.content_rect().shrink(RING_GAP + 2.0 + RING_HALO);
    let shown = stands.filter(|(target, (rect, layer))| {
        *target != Target::Paper && !ring_covered(ctx, *target, *rect, *layer)
    });
    let painted = shown.map(|(target, (rect, _))| {
        let rect = rect.intersect(room);
        let ring = rect.expand(RING_GAP);
        let round = match target {
            Target::Step(_) => (ring.height() / 2.0) as u8,
            _ => R_CONTROL + RING_GAP as u8,
        };
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("lesson_ring"));
        let painter = ctx.layer_painter(layer);
        painter.rect_stroke(ring, round, Stroke::new(2.0, BLUE_DEEP), StrokeKind::Outside);
        painter.rect_stroke(ring.expand(2.0), round + 2, Stroke::new(RING_HALO, ISLAND), StrokeKind::Outside);
        rect
    });
    #[cfg(test)]
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Marks>(marks_id()).painted = painted);
    #[cfg(not(test))]
    let _ = painted;
}

/// AU21: where the ring stood at the end of the last frame.
#[cfg(test)]
pub fn ring_stands(ctx: &egui::Context) -> Option<(Target, egui::Rect)> {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Marks>(marks_id()).stands)
}

/// AU21: the rect the ring was stroked round at the end of the last frame.
#[cfg(test)]
pub fn ring_painted(ctx: &egui::Context) -> Option<egui::Rect> {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Marks>(marks_id()).painted)
}

/// AU21: what of the wish was marked in the last frame.
#[cfg(test)]
pub fn ring_found(ctx: &egui::Context) -> Vec<Target> {
    ctx.data_mut(|d| {
        let marks = d.get_temp_mut_or_default::<Marks>(marks_id());
        marks.wish.iter().zip(&marks.found).filter(|(_, f)| f.is_some()).map(|(t, _)| *t).collect()
    })
}

#[cfg(test)]
thread_local! {
    /// AU21: `mark_on`'s calls on this thread: a frame's bill for its handles.
    pub static MARK_COST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// AU21: the walks over drawings or layers that only a lesson reads.
    pub static SCAN_COST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// AU20: a lesson on the Lessons page: a `WELL` card at `rect`, its `picture`
/// on a square of paper at the left, its title beside it, on up to two lines.
pub fn picture_card(
    ui: &Ui,
    rect: egui::Rect,
    title: &str,
    picture: impl FnOnce(&egui::Painter, egui::Rect),
) -> Response {
    let resp = ui.interact(rect, ui.id().with(("picture_card", title)), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, _) = filled(&resp);
        let p = ui.painter();
        p.rect_filled(rect, R_CONTROL, fill);
        let side = rect.height() - 2.0 * PICTURE_INSET;
        let at = rect.left_top() + Vec2::splat(PICTURE_INSET);
        let paper = egui::Rect::from_min_size(at, Vec2::splat(side));
        p.rect_filled(paper, R_CONTROL, PAPER);
        picture(p, paper);
        let wrap = rect.right() - PICTURE_INSET - paper.right() - PICTURE_INSET;
        let words = ui.fonts_mut(|f| f.layout(title.to_string(), Role::Panel.font(), INK, wrap));
        let top = rect.center().y - words.size().y / 2.0;
        p.galley(Pos2::new(paper.right() + PICTURE_INSET, top), words, INK);
    }
    speak(&resp, egui::WidgetType::Button, None, title);
    describe(resp, "CARD picture_card", title)
}

/// AU27: a finished lesson's tick, in the top right of its card.
pub fn finished_tick(painter: &egui::Painter, card: egui::Rect) {
    let at = card.right_top() + Vec2::new(-PICTURE_INSET - 12.0, PICTURE_INSET + 6.0);
    let line = vec![at + Vec2::new(0.0, 1.0), at + Vec2::new(4.0, 5.0), at + Vec2::new(12.0, -4.0)];
    painter.add(egui::Shape::line(line, Stroke::new(2.0, BLUE)));
}

/// AU27: a run of the lesson card's words, wrapped to the card.
pub fn lesson_words(ui: &mut Ui, words: &str, role: Role, ink: Color32) -> Response {
    ui.add(egui::Label::new(egui::RichText::new(words).font(role.font()).color(ink)).wrap())
}

/// AU27: the lesson's steps in bar order, on a line of their own; the open one
/// stands in deep ink on the wash, as the bar's pill does: never colour alone.
/// AU23: a card too narrow for five steps at panel size takes the floor's size
/// and one space, then tightens that a hair, not a second line.
pub fn lesson_path(ui: &mut Ui, steps: &[(&str, bool)]) -> Response {
    /// Off each letter on the narrowest card: five steps at the floor's size
    /// want 173 pt of its 164.
    const TIGHT: f32 = -0.3;
    let wide = ui.available_width();
    let path = |role: Role, between: &str, letters: f32| {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = wide;
        for (i, (name, open)) in steps.iter().enumerate() {
            let format = |ink| egui::TextFormat {
                extra_letter_spacing: letters,
                ..egui::TextFormat::simple(role.font(), ink)
            };
            if i > 0 {
                job.append(between, 0.0, format(INK3));
            }
            let word = if *open {
                egui::TextFormat { background: BLUE_WASH, ..format(BLUE_DEEP) }
            } else {
                format(INK3)
            };
            job.append(name, 0.0, word);
        }
        job
    };
    let fits = |job: &egui::text::LayoutJob| ui.painter().layout_job(job.clone()).rows.len() == 1;
    let job = [path(Role::Panel, "  ", 0.0), path(Role::Small, " ", 0.0)].into_iter().find(fits);
    ui.add(egui::Label::new(job.unwrap_or_else(|| path(Role::Small, " ", TIGHT))))
}

/// AU27: the strip's height: room for Deckle stood up over the ticks.
const STRIP_TALL: f32 = 36.0;
/// From the strip's foot up to its line of ticks.
const STRIP_FOOT: f32 = 3.0;
/// Deckle stands this far over the ticks' line.
const DECKLE_LIFT: f32 = 4.0;
/// Below this pitch a dot would touch the next: short upright ticks instead.
pub(crate) const TICK_PITCH: f32 = 7.0;
/// Deckle, in every pose: paper does not stretch.
pub(crate) const DECKLE_LONG: f32 = 28.0;
const DECKLE_WIDE: f32 = 10.0;
/// The arch's half angle: how far round the strip bends.
const DECKLE_BEND: f32 = 1.2;

/// AU27: where the strip's stops stand across `width`, the first before the
/// first tick, then one per task: its inset, and what a tick has to itself.
pub(crate) fn strip_stops(width: f32, total: usize) -> (f32, f32) {
    let inset = DECKLE_LONG / 2.0;
    (inset, (width - 2.0 * inset) / total.max(1) as f32)
}

/// AU27: one tick a task, lit once it is finished, and Deckle on the strip,
/// who starts before the first and moves a tick for each finished task, flat
/// and arched by turns, and stands up on the last. An off-white strip, one eye,
/// two holes (spec 5.1, 5.6).
pub fn lesson_strip(ui: &mut Ui, width: f32, done: usize, total: usize) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, STRIP_TALL), Sense::hover());
    speak(&resp, egui::WidgetType::Image, None, &format!("{done} of {total} done"));
    let p = ui.painter();
    let base = rect.bottom() - STRIP_FOOT;
    let (inset, pitch) = strip_stops(rect.width(), total);
    let at = |i: usize| Pos2::new(rect.left() + inset + pitch * i as f32, base);
    p.line_segment([at(0), at(total)], Stroke::new(1.0, RULE));
    for i in 1..=total {
        let ink = if i <= done { BLUE } else { INK3 };
        if pitch < TICK_PITCH {
            p.line_segment([at(i) - Vec2::new(0.0, 3.0), at(i) + Vec2::new(0.0, 3.0)], Stroke::new(1.5, ink));
        } else {
            p.circle_filled(at(i), 2.0, ink);
        }
    }
    let foot = at(done.min(total)) - Vec2::new(0.0, DECKLE_LIFT);
    paint_deckle(p, foot, done, done >= total);
    resp
}

/// Deckle's spine, up from its foot at the origin, head last. Every pose is
/// `DECKLE_LONG` along the spine and keeps the strip's underside on the foot.
pub(crate) fn deckle_spine(pose: usize, standing: bool) -> Vec<Vec2> {
    const ROWS: usize = 12;
    let half = DECKLE_LONG / 2.0;
    let share = |i: usize| i as f32 / ROWS as f32;
    let row = |i: usize| {
        if standing {
            // It leans a little: nothing here is ruled.
            let lean = 2.0;
            let rise = (DECKLE_LONG * DECKLE_LONG - lean * lean).sqrt();
            Vec2::new(lean * share(i), -rise * share(i))
        } else if pose % 2 == 1 {
            let radius = half / DECKLE_BEND;
            let turn = DECKLE_BEND * (2.0 * share(i) - 1.0);
            let rise = radius * (turn.cos() - DECKLE_BEND.cos());
            Vec2::new(radius * turn.sin(), -rise - DECKLE_WIDE / 2.0)
        } else {
            Vec2::new(DECKLE_LONG * share(i) - half, -DECKLE_WIDE / 2.0)
        }
    };
    (0..=ROWS).map(row).collect()
}

/// Deckle at `foot`: a filled strip with a thin edge, one eye by the head and
/// two punched holes along the spine.
fn paint_deckle(p: &egui::Painter, foot: Pos2, pose: usize, standing: bool) {
    let spine: Vec<Pos2> = deckle_spine(pose, standing).iter().map(|v| foot + *v).collect();
    let last = spine.len() - 1;
    let side = |i: usize| {
        let along = (spine[(i + 1).min(last)] - spine[i.saturating_sub(1)]).normalized();
        Vec2::new(-along.y, along.x) * (DECKLE_WIDE / 2.0)
    };
    let mut body = egui::Mesh::default();
    for i in 0..=last {
        body.colored_vertex(spine[i] + side(i), PAPER);
        body.colored_vertex(spine[i] - side(i), PAPER);
        if i > 0 {
            let at = 2 * i as u32;
            body.add_triangle(at - 2, at - 1, at);
            body.add_triangle(at - 1, at, at + 1);
        }
    }
    p.add(body);
    let near = (0..=last).map(|i| spine[i] + side(i));
    let far = (0..=last).rev().map(|i| spine[i] - side(i));
    p.add(egui::Shape::closed_line(near.chain(far).collect(), Stroke::new(1.0, INK3)));
    p.circle_filled(spine[last - 2], 2.0, INK);
    for hole in [spine[3], spine[6]] {
        p.circle_filled(hole, 1.25, INK3);
    }
}

/// AU27: the finish picture on a paper as wide as the card gives it: a row of
/// six has no use for a square. AU23.
pub fn lesson_picture(ui: &Ui, rect: egui::Rect, picture: impl FnOnce(&egui::Painter, egui::Rect)) {
    let p = ui.painter();
    p.rect_filled(rect, R_CONTROL, PAPER);
    picture(p, rect);
}

/// The card's chip as the brush holds its colour.
pub fn chip_bytes(color: Color32) -> [u8; 4] {
    color.to_srgba_unmultiplied()
}

/// AU6: a small key under a [`choice_card`], at `rect`: another size of the
/// card's choice. AU20: three cards across leave it no room on the card.
pub fn card_chip(ui: &mut Ui, rect: egui::Rect, label: &str) -> Response {
    let resp = ui.interact(rect, ui.id().with(("card_chip", label)), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = if resp.is_pointer_button_down_on() {
            (BLUE_WASH, BLUE_DEEP)
        } else if resp.hovered() {
            (HOVER, STRUCK)
        } else {
            (WELL, LEGEND)
        };
        let p = ui.painter();
        p.rect_filled(rect, R_CONTROL, fill);
        p.text(rect.center(), egui::Align2::CENTER_CENTER, label, Role::Panel.font(), ink);
    }
    speak(&resp, egui::WidgetType::Button, None, label);
    describe(resp, "CHIP card_chip", label)
}

/// AU6: one line of words that acts, a ghost `CTRL_H` tall and `width` wide.
/// A longer name elides. A row that opens something below it speaks whether
/// it is open, and shows it with a chevron.
pub fn ghost_row(ui: &mut Ui, label: &str, width: f32, open: Option<bool>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, CTRL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = ghost(ui, &resp);
        let chevron = if open.is_some() { ICON + 4.0 } else { 0.0 };
        row_words(ui, rect, label, chevron, fill, ink);
        if let Some(open) = open {
            let centre = Pos2::new(rect.right() - ROW_PAD - ICON / 2.0, rect.center().y);
            let mark = egui::Rect::from_center_size(centre, Vec2::splat(ICON));
            let icon = if open { crate::icons::Icon::Up } else { crate::icons::Icon::Down };
            crate::icons::paint(ui.painter(), mark, ink, icon);
        }
    }
    speak(&resp, egui::WidgetType::Button, open, label);
    describe(resp, "ACTION ghost_row", label)
}

/// AU20: a [`ghost_row`] that opens something, where the row's far end is too
/// far from its words to read as theirs: the chevron follows the words.
pub fn fold_row(ui: &mut Ui, label: &str, width: f32, open: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, CTRL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = ghost(ui, &resp);
        row_words(ui, rect, label, ICON + 4.0, fill, ink);
        let words = ui.painter().layout_no_wrap(label.to_string(), Role::Chrome.font(), ink).size().x;
        let centre = Pos2::new(rect.left() + ROW_PAD + words + 4.0 + ICON / 2.0, rect.center().y);
        let mark = egui::Rect::from_center_size(centre, Vec2::splat(ICON));
        let icon = if open { crate::icons::Icon::Up } else { crate::icons::Icon::Down };
        crate::icons::paint(ui.painter(), mark, ink, icon);
    }
    speak(&resp, egui::WidgetType::Button, Some(open), label);
    describe(resp, "ACTION fold_row", label)
}

/// AU15: the panels' `More` folds, one key each (spec 7.1): `ui.panel_more` as
/// this session reads it. A panel changes one here and the `Editor` saves it.
#[derive(Clone, Debug)]
pub struct PanelMore {
    /// What the person has chosen, per panel.
    pub open: std::collections::BTreeMap<String, bool>,
    /// The frozen boot's seam: every fold nobody chose stands open. A panel
    /// drawn alone in a test has no `Editor`, and stands the same way.
    pub all_open: bool,
    /// Folds changed this frame, for the `Editor` to save.
    pub changed: Vec<(String, bool)>,
}

/// `ui.panel_more`'s keys, one per fold. Stored: never renamed.
pub const MORE_SHEET: &str = "sheet_details";
pub const MORE_LAYERS: &str = "layers_more";
pub const MORE_FORGE: &str = "forge_more";
pub const MORE_COLORS: &str = "colors_more";
pub const MORE_GRAPH: &str = "graph_more";
pub const MORE_RESULT: &str = "result_more";
pub const MORE_CHECK: &str = "check_more";
pub const MORE_EXPORT: &str = "export_more";
pub const MORE_SHOTS: &str = "shots_more";
pub const MORE_EPISODE: &str = "episode_more";
pub const MORE_3D: &str = "view3d_more";
pub const MORE_MANGA: &str = "manga_more";
pub const MORE_BRUSHES: &str = "brushes_more";

/// The name a panel's fold answers to: `More` alone is the menu bar's.
pub fn more_name(key: &str) -> &'static str {
    match key {
        MORE_SHEET => "Timing sheet details",
        MORE_LAYERS => "More in Layers",
        MORE_FORGE => "More in Brush maker",
        MORE_COLORS => "More in Colors",
        MORE_GRAPH => "More in Effects graph",
        MORE_RESULT => "More in Result",
        MORE_CHECK => "More in Check paint",
        MORE_SHOTS => "More in Shots",
        MORE_EPISODE => "More in Episode",
        MORE_3D => "More in 3D",
        MORE_MANGA => "More in Manga",
        MORE_BRUSHES => "More settings",
        _ => "More options",
    }
}

/// AU15: egui_dock scrolls a tab bar too full for its leaf, over the body it
/// tops, so the bar's words are a viewport's to cut: that bar and no wider.
pub fn note_tab_bar(ui: &Ui) {
    let body = ui.clip_rect();
    let bar = egui::Rect::from_min_max(body.left_top() - egui::vec2(0.0, TAB_BAR_H), body.right_top());
    note_viewport(ui, bar.shrink2(egui::vec2(f32::from(TAB_INSET) + 1.0, 0.0)), [true, false]);
}

impl Default for PanelMore {
    fn default() -> Self {
        Self { open: Default::default(), all_open: cfg!(test), changed: Vec::new() }
    }
}

impl PanelMore {
    pub fn is_open(&self, key: &str) -> bool {
        self.open.get(key).copied().unwrap_or(self.all_open)
    }

    pub fn set(&mut self, key: &str, open: bool) {
        self.open.insert(key.to_string(), open);
        self.changed.push((key.to_string(), open));
    }
}

/// AU15: the width a [`fold_row`] needs for `label`, whole.
pub fn fold_row_width(ui: &Ui, label: &str) -> f32 {
    let words = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), Role::Chrome.font(), INK));
    words.size().x + 2.0 * ROW_PAD + ICON + 4.0
}

/// AU15: a panel in its slot, by its policy. Below its minimum it keeps the
/// minimum and scrolls as a whole, and a panel of words also scrolls whatever
/// outgrows a slot above it. Its own scrolls inside stand at the slot's size,
/// so only true overflow ever reaches this one. A view that pans costs nothing
/// in a slot it fits.
pub fn keep_minimum<R>(
    ui: &mut Ui,
    min: Vec2,
    policy: crate::workspace::PaneScroll,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    use egui::scroll_area::ScrollSource;
    let slot = ui.available_size();
    let axes = match policy {
        crate::workspace::PaneScroll::Pans => [slot.x < min.x, slot.y < min.y],
        _ => [true, true],
    };
    if axes == [false, false] {
        return add(ui);
    }
    let size = slot.max(min);
    let scroll = egui::ScrollArea::new(axes)
        .id_salt("pane_minimum")
        .auto_shrink([false, false])
        .scroll_source(ScrollSource::SCROLL_BAR | ScrollSource::MOUSE_WHEEL)
        .show(ui, |ui| {
            let layout = *ui.layout();
            ui.allocate_ui_with_layout(size, layout, |ui| {
                ui.set_min_size(size);
                add(ui)
            })
            .inner
        });
    // Only the axes that really run over: the audit excuses a viewport's edge
    // on those alone.
    let over = [0, 1].map(|d| axes[d] && scroll.content_size[d] > scroll.inner_rect.size()[d] + 0.5);
    if over != [false, false] {
        note_viewport(ui, scroll.inner_rect, over);
    }
    scroll.inner
}

/// AU15: a panel's disclosure row, the width of the panel, remembered under
/// `key`. It answers whether the fold stands open.
pub fn more_row(ui: &mut Ui, more: &mut PanelMore, key: &'static str, label: &str) -> bool {
    let width = ui.available_width();
    more_at(ui, more, key, label, width)
}

/// AU15: a [`more_row`] the width of its words, for a panel's head row.
pub fn more_tag(ui: &mut Ui, more: &mut PanelMore, key: &'static str, label: &str) -> bool {
    let width = fold_row_width(ui, label);
    more_at(ui, more, key, label, width)
}

fn more_at(ui: &mut Ui, more: &mut PanelMore, key: &'static str, label: &str, width: f32) -> bool {
    let open = more.is_open(key);
    let row = fold_row(ui, label, width, open);
    speak(&row, egui::WidgetType::Button, Some(open), more_name(key));
    mark_row(&row, Target::Named(key));
    let row = row.card(if open { "Fold this section away." } else { "Show the rest of this panel." });
    if row.clicked() {
        more.set(key, !open);
    }
    open != row.clicked()
}

/// AU9: a [`ghost_row`] with a ground at rest: one action among plain words,
/// which a ghost would leave reading as one more of them.
pub fn filled_row(ui: &mut Ui, label: &str, width: f32) -> Response {
    filled_named(ui, label, label, width)
}

/// AU14: the width a [`filled_row`] needs for `label`, whole.
pub fn filled_row_width(ui: &Ui, label: &str) -> f32 {
    let words = ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), Role::Chrome.font(), INK));
    words.size().x + 2.0 * ROW_PAD
}

/// AU14: a [`filled_row`] that answers to `name`, where the word it paints is
/// another control's on the same screen.
pub fn filled_named(ui: &mut Ui, label: &str, name: &str, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, CTRL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, _) = filled(&resp);
        row_words(ui, rect, label, 0.0, fill, INK);
    }
    speak(&resp, egui::WidgetType::Button, None, name);
    describe(resp, "ACTION filled_row", name)
}

/// AU14: a [`filled_row`] that stays on: armed, it is the armed tool's solid
/// `BLUE` with white words (spec 2.8), so it reads as a thing that is on, not
/// as the ring that means "press this next". A disabled one is never armed.
pub fn armed_row(ui: &mut Ui, label: &str, width: f32, armed: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, CTRL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = if armed && ui.is_enabled() { (TALLY, ON_FILL) } else { (filled(&resp).0, INK) };
        row_words(ui, rect, label, 0.0, fill, ink);
    }
    speak(&resp, egui::WidgetType::Button, Some(armed), label);
    describe(resp, "ACTION armed_row", label)
}

/// AU6: a [`ghost_row`] with a dim note at its right end, which the row's
/// name, its spoken label, leaves room for.
pub fn noted_row(ui: &mut Ui, label: &str, note: &str, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, CTRL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = ghost(ui, &resp);
        let dim = ui.painter().layout_no_wrap(note.to_string(), Role::Small.font(), INK3);
        row_words(ui, rect, label, dim.size().x + 8.0, fill, ink);
        let at = Pos2::new(rect.right() - ROW_PAD - dim.size().x, rect.center().y - dim.size().y / 2.0);
        ui.painter().galley(at, dim, INK3);
    }
    speak(&resp, egui::WidgetType::Button, None, label);
    describe(resp, "ACTION noted_row", label)
}

/// AU6: a [`ghost_row`] that says a state the app is in: on the wash, in the
/// current words' blue. Pressing it does what the row does at rest.
pub fn current_row(ui: &mut Ui, label: &str, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, CTRL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        row_words(ui, rect, label, 0.0, BLUE_WASH, BLUE_DEEP);
    }
    speak(&resp, egui::WidgetType::Button, Some(true), label);
    describe(resp, "DETENT current_row", label)
}

/// A row's ground and its one elided line, with `kept` held free at its
/// right end.
fn row_words(ui: &Ui, rect: egui::Rect, label: &str, kept: f32, fill: Color32, ink: Color32) {
    let mut job = LayoutJob::simple_singleline(label.to_string(), Role::Chrome.font(), ink);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 2.0 * ROW_PAD - kept);
    let words = ui.fonts_mut(|f| f.layout_job(job));
    let p = ui.painter();
    p.rect_filled(rect, R_CONTROL, fill);
    p.galley(Pos2::new(rect.left() + ROW_PAD, rect.center().y - words.size().y / 2.0), words, ink);
}

/// AU6: a check's answer: a mark and plain words, standing in the latch's
/// columns, and no control. A tick says yes, a cross no, so colour says
/// neither.
pub fn answer(ui: &mut Ui, yes: bool, words: &str) -> Response {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = LATCH_TEXT_X - LATCH_BOX;
        let (slot, _) = ui.allocate_exact_size(Vec2::splat(LATCH_BOX), Sense::hover());
        let p = ui.painter();
        if yes {
            paint_tick(p, slot, INK);
        } else {
            let mark = egui::Rect::from_center_size(slot.center(), Vec2::splat(8.0));
            let ink = Stroke::new(2.0, INK);
            p.line_segment([mark.left_top(), mark.right_bottom()], ink);
            p.line_segment([mark.right_top(), mark.left_bottom()], ink);
        }
        ui.label(egui::RichText::new(words).color(INK))
    })
    .inner
}

/// The off-spine pill: read-only, on the wash, `width` wide. A longer name
/// elides; it answers to `Current place: <name>` either way.
pub fn place_pill(ui: &mut Ui, name: &str, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, PILL_H), Sense::hover());
    if ui.is_rect_visible(rect) {
        let mut job = LayoutJob::simple_singleline(name.to_string(), Role::Step.font(), BLUE_DEEP);
        job.wrap = egui::text::TextWrapping::truncate_at_width(width - 2.0 * PILL_PAD);
        let words = ui.fonts_mut(|f| f.layout_job(job));
        let p = ui.painter();
        p.rect_filled(rect, R_PILL, BLUE_WASH);
        p.galley(rect.center() - words.size() * 0.5, words, BLUE_DEEP);
    }
    speak(&resp, egui::WidgetType::Label, None, &format!("Current place: {name}"));
    resp
}

/// The top row's menu words (specs 2.3 and 4.2): bare on the desk at rest,
/// `WELL` under the hand, the wash while pressed. For the row's own style; a
/// menu's popup reads the app's.
pub fn bare_words(style: &mut egui::Style) {
    let w = &mut style.visuals.widgets;
    w.inactive.bg_fill = Color32::TRANSPARENT;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.hovered.bg_fill = WELL;
    w.hovered.weak_bg_fill = WELL;
    w.active.bg_fill = BLUE_WASH;
    w.active.weak_bg_fill = BLUE_WASH;
    w.active.fg_stroke = Stroke::new(1.0, BLUE_DEEP);
}

/// A menu on the top row, a word under [`bare_words`] that keeps the wash
/// while its menu is open. Inside a menu (the narrow `Menu`) it is the
/// ordinary row.
pub fn menu_word<R>(ui: &mut Ui, word: &str, contents: impl FnOnce(&mut Ui) -> R) -> Response {
    if egui::containers::menu::is_in_menu(ui) {
        return ui.menu_button(word, contents).response;
    }
    let open = egui::Popup::is_id_open(ui.ctx(), ui.next_auto_id().with("popup"));
    let word = egui::Button::new(word).selected(open);
    egui::containers::menu::MenuButton::from_button(word).ui(ui, contents).0
}

/// An icon key on an island: a `PILL_H` square ghost. `name` is what it
/// answers to; the caller gives it its tip.
pub fn island_key(ui: &mut Ui, icon: crate::icons::Icon, name: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(PILL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, ink) = ghost(ui, &resp);
        let p = ui.painter();
        p.rect_filled(rect, R_PILL, fill);
        crate::icons::paint(p, egui::Rect::from_center_size(rect.center(), Vec2::splat(BAR_ICON)), ink, icon);
    }
    speak(&resp, egui::WidgetType::Button, None, name);
    describe(resp, "ACTION island_key", name)
}

/// AU10: where the return dot stands (spec section AU10), and what it answers
/// to.
pub const RETURN_DOT_AT: Pos2 = Pos2::new(8.0, 8.0);
/// The width the dock leaves it: the dot and its margin on each side.
pub const RETURN_DOT_ROOM: i8 = (RETURN_DOT_AT.x * 2.0 + PILL_H) as i8;
pub const SHOW_TOOLS: &str = "Show tools";
/// The window with its tools (spec 2.7), never More tools' dots: this key
/// wears no word to tell the two apart.
pub const RETURN_DOT_ICON: crate::icons::Icon = crate::icons::Icon::Hide;

/// AU10: the one thing Hide tools leaves on the screen: a `PILL_H` round key
/// on its own island, so it reads on paper and on the desk alike.
pub fn return_dot(ui: &mut Ui) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(PILL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.add(ui.visuals().window_shadow.as_shape(rect, CornerRadius::same(R_PILL)));
        p.rect_filled(rect, R_PILL, ISLAND);
        let (fill, ink) = ghost(ui, &resp);
        p.rect_filled(rect, R_PILL, fill);
        crate::icons::paint(p, egui::Rect::from_center_size(rect.center(), Vec2::splat(BAR_ICON)), ink, RETURN_DOT_ICON);
    }
    speak(&resp, egui::WidgetType::Button, None, SHOW_TOOLS);
    describe(resp, "ACTION return_dot", SHOW_TOOLS)
}

/// Search: a button in an input's clothes, or the bare lens when the row is
/// narrow. It answers to `Search` both ways, and the caller gives it its tip.
pub fn search_key(ui: &mut Ui, wide: bool) -> Response {
    if !wide {
        return island_key(ui, crate::icons::Icon::Search, "Search");
    }
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(SEARCH_W, PILL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        // A field's corners are square enough to need the room: this keeps
        // them inside the island's round end.
        let well = rect.with_max_x(rect.right() - 4.0).shrink(2.0);
        let (fill, ink) = filled(&resp);
        let p = ui.painter();
        p.rect(well, R_INPUT, fill, Stroke::new(1.0, INK3), StrokeKind::Inside);
        let lens = Pos2::new(well.left() + 10.0 + BAR_ICON * 0.5, well.center().y);
        crate::icons::paint(p, egui::Rect::from_center_size(lens, Vec2::splat(BAR_ICON)), ink, crate::icons::Icon::Search);
        label_at(ui, lens.x + BAR_ICON * 0.5 + 6.0, well.center().y, "Search", CTRL_TEXT, ink);
    }
    speak(&resp, egui::WidgetType::Button, None, "Search");
    describe(resp, "ACTION search_key", "Search")
}

/// A ghost control's ground and content by state, as [`icon_button`] paints
/// them.
fn ghost(ui: &Ui, resp: &Response) -> (Color32, Color32) {
    if resp.is_pointer_button_down_on() {
        (BLUE_WASH, BLUE_DEEP)
    } else if resp.hovered() {
        (WELL, STRUCK)
    } else {
        (Color32::TRANSPARENT, at_rest(ui, LEGEND))
    }
}

/// A colour swatch: an 18×18 fill with its `INK3` ring, so a pale colour
/// still has an edge on an island; the armed swatch is ringed in blue. The
/// hit target is larger than the fill (the owner's amendment). It answers to
/// "color well: <name>" (the role or color name; empty = the hex value), so a
/// palette's wells are told apart by name, not position.
pub fn swatch(ui: &mut Ui, color: Color32, armed: bool, name: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(CTRL_H), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let well = egui::Rect::from_center_size(rect.center(), Vec2::splat(18.0));
        p.rect_filled(well, R_SHEET, color);
        p.rect_stroke(
            well,
            R_SHEET,
            Stroke::new(1.0, at_rest(ui, INK3)),
            StrokeKind::Outside,
        );
        if armed {
            p.rect_stroke(
                well.expand(3.0),
                R_INPUT,
                Stroke::new(2.0, at_rest(ui, TALLY)),
                StrokeKind::Outside,
            );
        } else if resp.hovered() {
            p.rect_stroke(
                well.expand(3.0),
                R_INPUT,
                Stroke::new(1.0, STRUCK),
                StrokeKind::Outside,
            );
        }
    }
    let well = crate::vocab::COLOR_WELL;
    let name = if name.is_empty() {
        format!("{well}: {}", color.to_hex())
    } else {
        format!("{well}: {name}")
    };
    speak(&resp, egui::WidgetType::Button, Some(armed), &name);
    describe(resp, "SWATCH swatch", well)
}

/// AU13: a [`swatch`] that fills a cell of `size`, for a pen to find: the
/// Colors panel's starters.
pub fn swatch_cell(ui: &mut Ui, size: Vec2, color: Color32, armed: bool, name: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let well = rect.shrink(3.0);
        // One shape, its edge drawn inside the rect a point larger, as it stood
        // outside the well: a row of ten as two shapes each costs 1,000 vertices.
        let edge = Stroke::new(1.0, at_rest(ui, INK3));
        p.add(egui::epaint::RectShape::new(well.expand(1.0), R_SHEET + 1, color, edge, StrokeKind::Inside));
        if armed {
            p.rect_stroke(well.expand(2.0), R_INPUT, Stroke::new(2.0, at_rest(ui, BLUE)), StrokeKind::Outside);
        } else if resp.hovered() {
            p.rect_stroke(well.expand(2.0), R_INPUT, Stroke::new(1.0, INK), StrokeKind::Outside);
        }
    }
    let well = crate::vocab::COLOR_WELL;
    speak(&resp, egui::WidgetType::Button, Some(armed), &format!("{well}: {name}"));
    describe(resp, "SWATCH swatch_cell", well)
}

/// RAIL — a bounded value the hand drags (the plate's fourth and last
/// control kind, added 2026-08-18 under the sweetening room). A Well
/// track with its `INK3` edge, filled blue from the left to the handle, so
/// 0 and full are always visible. egui's own slider rail is invisible under
/// our Visuals (its track is a bare Well on an island) — this replaces it.
/// Returns true while the value is being changed.
pub fn rail(
    ui: &mut Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    w: f32,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, CTRL_H), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let span = (hi - lo).max(f32::EPSILON);
    if let Some(p) = resp.interact_pointer_pos() {
        let t = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        *value = lo + t * span;
    }
    if ui.is_rect_visible(rect) {
        let ppp = ui.ctx().pixels_per_point();
        let p = ui.painter();
        let track = egui::Rect::from_min_max(
            Pos2::new(snap(rect.left(), ppp), snap(rect.center().y - 4.0, ppp)),
            Pos2::new(snap(rect.right(), ppp), snap(rect.center().y + 4.0, ppp)),
        );
        p.rect_filled(track, 4, ground_of(ui, WELL));
        let t = ((*value - lo) / span).clamp(0.0, 1.0);
        if t > 0.0 {
            let mut fill = track;
            fill.max.x = snap(track.left() + track.width() * t, ppp);
            p.rect_filled(fill.shrink(1.0), 3, at_rest(ui, TALLY));
        }
        p.rect_stroke(
            track,
            4,
            Stroke::new(1.0, if resp.hovered() { LEGEND } else { at_rest(ui, INK3) }),
            StrokeKind::Inside,
        );
        // The handle: an ink bar the eye can find at any fill level.
        let hx = snap(track.left() + track.width() * t, ppp);
        p.rect_filled(
            egui::Rect::from_min_max(
                Pos2::new(hx - 2.0, rect.center().y - 7.0),
                Pos2::new(hx + 2.0, rect.center().y + 7.0),
            ),
            2,
            STRUCK,
        );
    }
    resp
}

/// FIELD — a value the animator sets and reads: a `DragValue` keeps its
/// behaviour and takes the field's `WELL` and its `INK3` edge, with the
/// corners of an input. The FACE comes from `drag_value_text_style`. Words go
/// through [`text_field`], which also makes the height. The edge lives here
/// and not on `widgets.inactive`, which would stroke every button.
pub fn field(ui: &mut Ui, widget: impl egui::Widget) -> Response {
    let edge = Stroke::new(1.0, at_rest(ui, INK3));
    let saved = ui.visuals().widgets.clone();
    let w = &mut ui.visuals_mut().widgets;
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
        state.bg_stroke = edge;
        state.corner_radius = CornerRadius::same(R_INPUT);
    }
    w.inactive.weak_bg_fill = WELL;
    w.hovered.weak_bg_fill = WELL;
    w.active.weak_bg_fill = WELL;
    let r = ui.add(widget);
    ui.visuals_mut().widgets = saved;
    r
}

/// [`field`] that takes no input while `enabled` is false.
pub fn field_if(ui: &mut Ui, enabled: bool, widget: impl egui::Widget) -> Response {
    ui.add_enabled_ui(enabled, |ui| field(ui, widget)).inner
}

/// FIELD for words: a `TextEdit` at its row's control height (the panel's 28,
/// or the sheet head's row), text centred. egui takes a `TextEdit`'s height
/// from its font and margin (`min_size` is width only), so the margin is what
/// makes it. Margins are whole points: 13 pt's row is whole and lands on the
/// 28, 14 pt's is 18.19 and cannot, so the field keeps 13 under the 14 pt body.
pub fn text_field(ui: &mut Ui, edit: egui::TextEdit<'_>) -> Response {
    fitted_field(ui, edit, FontId::proportional(FIELD_TEXT))
}

/// [`text_field`] for a file pattern, in the one face that lines its `####`
/// up (spec 2.5).
pub fn pattern_field(ui: &mut Ui, edit: egui::TextEdit<'_>) -> Response {
    fitted_field(ui, edit, mono_font())
}

fn fitted_field(ui: &mut Ui, edit: egui::TextEdit<'_>, font: FontId) -> Response {
    let height = ui.spacing().interact_size.y;
    let spare = (height - ui.fonts_mut(|f| f.row_height(&font))).max(0.0);
    let margin = egui::Margin {
        left: FIELD_INSET,
        right: FIELD_INSET,
        top: (spare * 0.5).ceil() as i8,
        bottom: (spare * 0.5).floor() as i8,
    };
    field(ui, edit.font(font).margin(margin))
}

/// METER — a read-only fill (export progress, and anything else that
/// reports how far along a machine is). Same material as RAIL: a Well
/// track with its `INK3` edge and a blue fill; the count rides BESIDE it
/// in ink, never inside the bar where the fill swallows it. Indeterminate
/// work passes `None`, which sweeps a short blue band instead of faking a
/// percentage.
pub fn meter(ui: &mut Ui, frac: Option<f32>, w: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 14.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let ppp = ui.ctx().pixels_per_point();
    let p = ui.painter();
    let track = egui::Rect::from_min_max(
        Pos2::new(snap(rect.left(), ppp), snap(rect.center().y - 5.0, ppp)),
        Pos2::new(snap(rect.right(), ppp), snap(rect.center().y + 5.0, ppp)),
    );
    p.rect_filled(track, 5, ground_of(ui, WELL));
    match frac {
        Some(f) => {
            let t = f.clamp(0.0, 1.0);
            if t > 0.0 {
                let mut fill = track;
                fill.max.x = snap(track.left() + track.width() * t, ppp);
                p.rect_filled(fill.shrink(1.0), 4, at_rest(ui, TALLY));
            }
        }
        None => {
            // Indeterminate: a band that sweeps, so the window reads as
            // working rather than stalled — without claiming a number.
            let phase = (ui.input(|i| i.time) * 0.6).fract() as f32;
            let bw = track.width() * 0.25;
            let x = track.left() + (track.width() + bw) * phase - bw;
            let band = egui::Rect::from_min_max(
                Pos2::new(x.max(track.left()), track.top() + 1.0),
                Pos2::new((x + bw).min(track.right()), track.bottom() - 1.0),
            );
            if band.width() > 0.0 {
                p.rect_filled(band, 4, at_rest(ui, TALLY));
            }
            ui.ctx().request_repaint();
        }
    }
    p.rect_stroke(track, 5, Stroke::new(1.0, at_rest(ui, INK3)), StrokeKind::Inside);
}

/// How long DANGER must be held before it fires.
const DANGER_HOLD_S: f64 = 0.35;

/// DANGER — a destructive command. A red edge and red words, and it does not
/// fire on a click: it fires after a ~350ms press-and-hold, during which the
/// red fills and a white ring closes beside the label. Releasing early
/// cancels. Returns true exactly once, at the moment the hold completes.
pub fn danger(ui: &mut Ui, label: &str) -> bool {
    danger_hint(ui, label, "")
}

/// DANGER with what it destroys said in plain words on hover. Every DANGER
/// says "hold" there: a first-time hand clicks, and a click does nothing.
/// The words paint in sentence case; `label` stays the control's spoken name.
pub fn danger_hint(ui: &mut Ui, label: &str, hint: &str) -> bool {
    let words = sentence(&label.to_lowercase());
    let text_w = text_width(ui, &words, CTRL_TEXT);
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(8.0 + text_w + 6.0 + CTRL_H, CTRL_H),
        Sense::click_and_drag(),
    );
    // GM2: a held DANGER is still a named button (screen readers, testkit).
    speak(&resp, egui::WidgetType::Button, None, label);
    let id = resp.id;
    let fired_id = id.with("fired");
    let now = ui.input(|i| i.time);
    let held = resp.is_pointer_button_down_on();
    let start: Option<f64> = ui.data_mut(|d| d.get_temp(id));
    let already_fired: bool = ui.data_mut(|d| d.get_temp(fired_id).unwrap_or(false));
    let mut fired = false;
    let mut progress = 0.0_f32;
    if held && !already_fired {
        let t0 = match start {
            Some(t) => t,
            None => {
                ui.data_mut(|d| d.insert_temp(id, now));
                now
            }
        };
        progress = (((now - t0) / DANGER_HOLD_S).min(1.0)) as f32;
        if progress >= 1.0 {
            fired = true;
            ui.data_mut(|d| {
                d.remove_temp::<f64>(id);
                d.insert_temp(fired_id, true);
            });
        } else {
            ui.ctx().request_repaint();
        }
    } else if !held {
        ui.data_mut(|d| {
            d.remove_temp::<f64>(id);
            d.remove_temp::<bool>(fired_id);
        });
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let ink = if held {
            p.rect_filled(rect, R_CONTROL, RED);
            ON_FILL
        } else {
            let rest = at_rest(ui, RED);
            if resp.hovered() {
                p.rect_filled(rect, R_CONTROL, RED_WASH);
            }
            p.rect_stroke(rect, R_CONTROL, Stroke::new(1.0, rest), StrokeKind::Inside);
            rest
        };
        label_at(ui, rect.left() + 8.0, rect.center().y, &words, CTRL_TEXT, ink);
        // The filling ring, seated in the square right slot.
        let c = Pos2::new(rect.right() - CTRL_H * 0.5, rect.center().y);
        if progress > 0.0 {
            let n = 24usize;
            let pts: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2
                        + std::f32::consts::TAU * progress * (i as f32 / n as f32);
                    Pos2::new(c.x + 6.0 * a.cos(), c.y + 6.0 * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new(2.0, ink)));
        } else {
            p.circle_stroke(c, 6.0, Stroke::new(1.0, ink));
        }
    }
    let _ = if hint.is_empty() {
        resp.card("keep it pressed to fire — a click does nothing")
    } else {
        resp.card(format!("keep it pressed — {hint}"))
    };
    fired
}

// ---------------------------------------------------------------------------
// The base Visuals — `style` carries them into both theme slots.
// ---------------------------------------------------------------------------

/// The app's egui Visuals, derived entirely from the tokens above (spec 2.9).
pub fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    v.panel_fill = ISLAND;
    v.window_fill = ISLAND;
    v.extreme_bg_color = WELL;
    v.faint_bg_color = WELL;
    v.code_bg_color = WELL;
    v.hyperlink_color = BLUE;
    v.warn_fg_color = RED;
    v.error_fg_color = RED;

    // selection.stroke.color is what egui paints a selected widget's text
    // with, so a selection reads as deep ink on the wash.
    v.selection.bg_fill = BLUE_WASH;
    v.selection.stroke = Stroke::new(1.0, BLUE_DEEP);
    v.window_stroke = Stroke::NONE;
    v.disabled_alpha = DISABLED_ALPHA;
    v.weak_text_color = Some(INK3);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = ISLAND;
    w.noninteractive.weak_bg_fill = ISLAND;
    w.noninteractive.fg_stroke = Stroke::new(1.0, INK);
    w.noninteractive.bg_stroke = Stroke::new(1.0, RULE);
    w.inactive.bg_fill = WELL;
    w.inactive.weak_bg_fill = WELL;
    w.inactive.fg_stroke = Stroke::new(1.0, INK);
    w.inactive.bg_stroke = Stroke::NONE;
    w.hovered.bg_fill = HOVER;
    w.hovered.weak_bg_fill = HOVER;
    w.hovered.fg_stroke = Stroke::new(1.0, INK);
    w.hovered.bg_stroke = Stroke::NONE;
    w.active.bg_fill = PRESSED;
    w.active.weak_bg_fill = PRESSED;
    w.active.fg_stroke = Stroke::new(1.0, INK);
    w.active.bg_stroke = Stroke::NONE;
    w.open.bg_fill = BLUE_WASH;
    w.open.weak_bg_fill = BLUE_WASH;
    w.open.fg_stroke = Stroke::new(1.0, BLUE_DEEP);
    w.open.bg_stroke = Stroke::NONE;

    let control = CornerRadius::same(R_CONTROL);
    w.noninteractive.corner_radius = control;
    w.inactive.corner_radius = control;
    w.hovered.corner_radius = control;
    w.active.corner_radius = control;
    w.open.corner_radius = control;
    v.window_corner_radius = CornerRadius::same(R_FLOAT);
    v.menu_corner_radius = CornerRadius::same(R_MENU);
    v.window_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 24, spread: 0, color: alpha(SHADOW, 41) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: alpha(SHADOW, 46) };
    v
}

/// Lay the island down as this pane's ground. Call FIRST in a pane's ui,
/// before any content; everything else paints over it.
pub fn surface(ui: &mut Ui) {
    surface_alpha(ui, 255);
}

/// The island at partial opacity — the brush rail's ground (owner
/// 2026-08-17): the toolbar is present without covering the paper.
pub fn surface_alpha(ui: &mut Ui, opacity: u8) {
    ui.painter().rect_filled(ui.max_rect(), CornerRadius::ZERO, alpha(ISLAND, opacity));
}

/// AU1: a `ScrollArea` reports its viewport here, because egui gives no
/// other way to find one. `axes` are the ones that scroll, `[horizontal,
/// vertical]` as `ScrollArea::new` takes them. The clip audit reads the list
/// (`note_viewport` sits after every scroll area, a rule test holds that);
/// it exists only under test. AU15: `cue` reads whether a handle stands at
/// rest here, and a scroll that shows nothing excuses no cut word.
#[cfg_attr(not(test), allow(unused_variables))]
pub fn note_viewport(ui: &Ui, inner: egui::Rect, axes: [bool; 2]) {
    #[cfg(test)]
    {
        let reach = inner.expand(ui.visuals().clip_rect_margin);
        note_view(ui, Viewport { rect: inner, reach, axes, cue: ui.spacing().scroll.dormant_handle_opacity > 0.0 });
    }
}

/// AU1: a view panned by hand is a scroll viewport on both axes, with no
/// `ScrollArea` to report it. What pans in it is cut anywhere from `rect`
/// out to `reach`.
#[cfg_attr(not(test), allow(unused_variables))]
pub fn note_pan_view(ui: &Ui, rect: egui::Rect, reach: egui::Rect) {
    #[cfg(test)]
    note_view(ui, Viewport { rect, reach, axes: [true, true], cue: true });
}

/// AU1: a dock pane reports the clip its body draws under, so the clip audit
/// can tell a pane's content from chrome by what cuts it. Only under test.
#[cfg_attr(not(test), allow(unused_variables))]
pub fn note_pane_body(ui: &Ui) {
    #[cfg(test)]
    {
        let body = ui.clip_rect();
        this_pass(ui.ctx(), |seen| seen.bodies.push(body));
    }
}

/// AU3: the dock reports the rect it is handed, the centre of the window: a
/// tab bar is its panel's, not chrome. Only under test.
#[cfg_attr(not(test), allow(unused_variables))]
pub fn note_dock(ui: &Ui, rect: egui::Rect) {
    #[cfg(test)]
    this_pass(ui.ctx(), |seen| seen.dock = Some(rect));
}

/// AU15: a dock seam across `rect` at `fraction`, its painted 6 pt: what the
/// census calls a seam, though its grab runs wider. Only under test.
#[cfg(test)]
pub fn note_dock_seam(ui: &Ui, rect: egui::Rect, fraction: f32, across: bool) {
    let mut seam = rect;
    if across {
        let x = rect.left() + rect.width() * fraction;
        (seam.min.x, seam.max.x) = (x - 3.0, x + 3.0);
    } else {
        let y = rect.top() + rect.height() * fraction;
        (seam.min.y, seam.max.y) = (y - 3.0, y + 3.0);
    }
    this_pass(ui.ctx(), |seen| seen.seams.push(seam));
}

/// AU5: the strip is one surface, and each sheet on it counts as a control
/// (spec 7.2), so it reports them to the census. Only under test.
#[cfg_attr(not(test), allow(unused_variables))]
pub fn note_strip_sheet(ui: &Ui, rect: egui::Rect) {
    #[cfg(test)]
    this_pass(ui.ctx(), |seen| seen.sheets.push(rect));
}

/// A scroll viewport: its inner rect, the farthest out a clip can sit on the
/// sides that scroll (`clip_rect_margin` past the inner rect, for a
/// `ScrollArea`), and which axes those are.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub rect: egui::Rect,
    pub reach: egui::Rect,
    pub axes: [bool; 2],
    /// Something at rest says this view scrolls: a handle, or the view's own pan.
    pub cue: bool,
}

/// One pass's registered viewports, pane bodies and dock.
#[cfg(test)]
#[derive(Clone, Default)]
struct Viewports {
    pass: u64,
    views: Vec<Viewport>,
    bodies: Vec<egui::Rect>,
    dock: Option<egui::Rect>,
    sheets: Vec<egui::Rect>,
    seams: Vec<egui::Rect>,
}

#[cfg(test)]
fn note_view(ui: &Ui, view: Viewport) {
    this_pass(ui.ctx(), |seen| seen.views.push(view));
}

/// This pass's list, fresh on the pass's first note.
#[cfg(test)]
fn this_pass(ctx: &egui::Context, note: impl FnOnce(&mut Viewports)) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let seen = d.get_temp_mut_or_default::<Viewports>(egui::Id::new("plate_viewports"));
        if seen.pass != pass {
            *seen = Viewports { pass, ..Viewports::default() };
        }
        note(seen);
    });
}

/// What the last finished pass registered.
#[cfg(test)]
fn last_pass(ctx: &egui::Context) -> Viewports {
    let last = ctx.cumulative_pass_nr().wrapping_sub(1);
    ctx.data_mut(|d| d.get_temp::<Viewports>(egui::Id::new("plate_viewports")))
        .filter(|seen| seen.pass == last)
        .unwrap_or_default()
}

/// The viewports the last finished pass registered.
#[cfg(test)]
pub fn viewports(ctx: &egui::Context) -> Vec<Viewport> {
    last_pass(ctx).views
}

/// The pane bodies the last finished pass registered.
#[cfg(test)]
pub fn pane_bodies(ctx: &egui::Context) -> Vec<egui::Rect> {
    last_pass(ctx).bodies
}

/// The dock's rect in the last finished pass.
#[cfg(test)]
pub fn dock_rect(ctx: &egui::Context) -> Option<egui::Rect> {
    last_pass(ctx).dock
}

/// The strip's sheets on screen in the last finished pass.
#[cfg(test)]
pub fn strip_sheets(ctx: &egui::Context) -> Vec<egui::Rect> {
    last_pass(ctx).sheets
}

/// The dock's seams, as painted, in the last finished pass.
#[cfg(test)]
pub fn dock_seams(ctx: &egui::Context) -> Vec<egui::Rect> {
    last_pass(ctx).seams
}

// ---------------------------------------------------------------------------
// Small internals
// ---------------------------------------------------------------------------

fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
    let font = FontId::new(size, FontFamily::Proportional);
    ui.fonts_mut(|f| {
        f.layout_no_wrap(text.to_string(), font, Color32::WHITE)
            .size()
            .x
    })
}

fn label_at(ui: &Ui, x: f32, cy: f32, text: &str, size: f32, ink: Color32) {
    ui.painter().text(
        Pos2::new(x, cy),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::new(size, FontFamily::Proportional),
        ink,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_lands_on_device_pixels() {
        // At ppp 1.25, logical 10.1 is 12.625 device px; the nearest device
        // pixel is 13, which is logical 10.4.
        let s = snap(10.1, 1.25);
        assert!((s - 10.4).abs() < 1e-6, "got {s}");
        // Snapped values are fixed points of snap.
        assert_eq!(snap(s, 1.25), s);
        // At ppp 1.0 snapping is plain rounding.
        assert_eq!(snap(3.4, 1.0), 3.0);
    }

    #[test]
    fn device_px_converts_rule_weights() {
        // A 1-device-px rule at ppp 1.25 is 0.8 logical points, and snapping
        // preserves it: 0.8 * 1.25 = 1.0 exactly.
        let w = device_px(1.0, 1.25);
        assert!((w - 0.8).abs() < 1e-6);
        assert_eq!(w * 1.25, 1.0);
    }

    #[test]
    fn the_palette_is_exactly_the_ratified_one() {
        // The spec's hex values, transcribed independently of the constants.
        assert_eq!(GRAPHITE, Color32::from_rgb(0xF6, 0xF7, 0xF8));
        assert_eq!(WELL, Color32::from_rgb(0xE8, 0xEA, 0xED));
        assert_eq!(LEGEND, Color32::from_rgb(0x4C, 0x52, 0x5B));
        assert_eq!(STRUCK, Color32::from_rgb(0x1E, 0x22, 0x27));
        assert_eq!(AO, Color32::from_rgb(0x70, 0x76, 0x7F));
        assert_eq!(AKA, Color32::from_rgb(0xB9, 0x2D, 0x1C));
        assert_eq!(TALLY, Color32::from_rgb(0x1F, 0x5F, 0xBF));
        assert_eq!(PAPER, Color32::from_rgb(0xFF, 0xFF, 0xFF));
    }

    #[test]
    fn aulook_tokens_are_the_table() {
        let table: [(Color32, [u8; 3]); 19] = [
            (DESK, [0xD3, 0xD5, 0xD8]),
            (ISLAND, [0xF6, 0xF7, 0xF8]),
            (WELL, [0xE8, 0xEA, 0xED]),
            (HOVER, [0xE0, 0xE3, 0xE7]),
            (PRESSED, [0xCD, 0xD1, 0xD6]),
            (PAPER, [0xFF, 0xFF, 0xFF]),
            (INK, [0x1E, 0x22, 0x27]),
            (INK2, [0x4C, 0x52, 0x5B]),
            (INK3, [0x70, 0x76, 0x7F]),
            (RULE, [0xBF, 0xC3, 0xC8]),
            (BLUE, [0x1F, 0x5F, 0xBF]),
            (BLUE_DEEP, [0x18, 0x4C, 0x9A]),
            (BLUE_WASH, [0xDC, 0xE8, 0xF9]),
            (RED, [0xB9, 0x2D, 0x1C]),
            (RED_WASH, [0xFB, 0xE5, 0xE1]),
            (STEEL, [0x2A, 0x2E, 0x34]),
            (STEEL_INK, [0xE6, 0xE8, 0xEB]),
            (RESULT, [0x80, 0x80, 0x80]),
            (SHADOW, [0x14, 0x18, 0x1F]),
        ];
        for (token, [r, g, b]) in table {
            assert_eq!(token.to_array(), [r, g, b, 255]);
        }
        // The old names read the new roles until the alias retirement.
        assert_eq!([GRAPHITE, LEGEND, STRUCK, AO, AKA, TALLY], [ISLAND, INK2, INK, INK3, RED, BLUE]);
        // The derived colours, each a token opaque or at a set transparency.
        assert_eq!([legend_dim(), hairline(), rule_sec(), legend_muted()], [INK2, RULE, INK2, INK3]);
        assert_eq!([rule_beat(), rule_half()], [alpha(INK2, 56), alpha(INK2, 102)]);
        assert_eq!([tally_well(), paper_veil()], [alpha(BLUE, 36), alpha(STEEL, 110)]);
        // The radii and the disabled scope's dimming.
        assert_eq!([R_DIALOG, R_FLOAT, R_MENU, R_CONTROL, R_INPUT, R_SHEET], [14, 12, 10, 8, 6, 4]);
        assert_eq!(visuals().disabled_alpha, 0.65);
    }

    /// GM16: the characters of every string and char literal in `src`
    /// (comments skipped; `\u{…}` escapes decoded).
    fn literal_chars(src: &str) -> Vec<char> {
        let c: Vec<char> = src.chars().collect();
        let mut out = Vec::new();
        // An escape at `i` (just past the backslash): the char it spells
        // when it is `\u{…}`, and where the literal goes on.
        let escape = |i: usize| -> (Option<char>, usize) {
            if c.get(i) == Some(&'u') && c.get(i + 1) == Some(&'{') {
                let end = (i + 2..c.len()).find(|&j| c[j] == '}').unwrap_or(c.len() - 1);
                let hex: String = c[i + 2..end].iter().filter(|ch| **ch != '_').collect();
                (u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32), end + 1)
            } else {
                (None, i + 1)
            }
        };
        let mut i = 0;
        while i < c.len() {
            match c[i] {
                '/' if c.get(i + 1) == Some(&'/') => {
                    while i < c.len() && c[i] != '\n' {
                        i += 1;
                    }
                }
                '/' if c.get(i + 1) == Some(&'*') => {
                    let mut depth = 0;
                    while i < c.len() {
                        if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                            depth += 1;
                            i += 2;
                        } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                            depth -= 1;
                            i += 2;
                            if depth == 0 {
                                break;
                            }
                        } else {
                            i += 1;
                        }
                    }
                }
                'r' if matches!(c.get(i + 1), Some('"') | Some('#')) => {
                    let mut j = i + 1;
                    while c.get(j) == Some(&'#') {
                        j += 1;
                    }
                    let hashes = j - i - 1;
                    if c.get(j) != Some(&'"') {
                        i = j; // a raw identifier
                        continue;
                    }
                    j += 1;
                    while j < c.len() && !(c[j] == '"' && (1..=hashes).all(|k| c.get(j + k) == Some(&'#'))) {
                        out.push(c[j]);
                        j += 1;
                    }
                    i = j + 1 + hashes;
                }
                '"' => {
                    i += 1;
                    while i < c.len() && c[i] != '"' {
                        if c[i] == '\\' {
                            let (ch, next) = escape(i + 1);
                            out.extend(ch);
                            i = next;
                        } else {
                            out.push(c[i]);
                            i += 1;
                        }
                    }
                    i += 1;
                }
                '\'' if c.get(i + 1) == Some(&'\\') => {
                    let (ch, next) = escape(i + 2);
                    out.extend(ch);
                    i = (next..c.len()).find(|&j| c[j] == '\'').map_or(c.len(), |j| j + 1);
                }
                // A char literal, not a lifetime.
                '\'' if c.get(i + 2) == Some(&'\'') => {
                    out.push(c[i + 1]);
                    i += 3;
                }
                _ => i += 1,
            }
        }
        out
    }

    #[test]
    fn genui_literal_scanner_reads_strings_and_skips_comments() {
        let src = "// 原画 in a comment\n/* 版 /* nested 版 */ */ let a = \"仕上\"; let b = r#\"撮\"影\"#;\n\
                   let c = '…'; let d = '\\u{2014}'; fn f<'a>(x: &'a str) {} let e = \"\\u{30BB}\\\"\";";
        let got: String = literal_chars(src).into_iter().filter(|c| !c.is_ascii()).collect();
        assert_eq!(got, "仕上撮影…\u{2014}セ");
    }

    /// Every non-ASCII character in the app's string literals, and in the
    /// engine's that the app draws, draws in every family of the embedded
    /// stack (D7: no tofu; GM16: the JP face is the whole Bold, so kanji in
    /// labels and lettering defaults render).
    #[test]
    fn genui_glyph_coverage() {
        fn rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    rs_files(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        rs_files(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        assert!(files.len() > 20, "the app's sources: {}", files.len());
        // The engine's words the app draws too: node and generator names,
        // romaji terms, labels, help, options and default text.
        let core = [
            include_str!("../../crates/anim-core/src/generator.rs"),
            include_str!("../../crates/anim-core/src/fx.rs"),
            include_str!("../../crates/anim-core/src/camera.rs"),
            include_str!("../../crates/anim-core/src/graph.rs"),
        ];
        let core_chars: Vec<char> = core.iter().flat_map(|src| literal_chars(src)).collect();
        assert!(core_chars.contains(&'ū') && core_chars.contains(&'セ'), "the scan reads ryūsen and セリフ");
        let mut chars: Vec<char> = files
            .iter()
            .flat_map(|f| literal_chars(&std::fs::read_to_string(f).unwrap()))
            .chain(core_chars)
            .filter(|c| !c.is_ascii() && !c.is_whitespace())
            .collect();
        chars.sort_unstable();
        chars.dedup();
        assert!(chars.contains(&'→') && chars.contains(&'▾'), "the scan finds the labels' symbols");
        let defs = font_definitions();
        for (family, names) in &defs.families {
            let stack: Vec<&[u8]> = names.iter().map(|n| &defs.font_data[n].font[..]).collect();
            let missing = anim_core::text::uncovered(&stack, chars.iter().copied());
            assert!(missing.is_empty(), "{family:?} has no glyph for {missing:?}");
        }
        // The lettering's default words are in the engine's JP face itself.
        assert!(anim_core::text::uncovered(&[anim_core::text::PLEX_SANS_JP], "セリフ".chars()).is_empty());
    }
}
