//! THE COLORS PANEL (the swatch board, built 2026-08-17, AU13: made
//! canvas-first). Ten starter swatches, `Add color` and `Edit
//! colors` stand above the scroll, so none of them can leave the screen. A
//! person's own colors are pots under them, a pot's own colour its icon; the
//! armed pot is ringed. Character groups and color names are disclosed. The
//! colors are EDITED only behind the `Edit colors` latch: nothing destructive
//! or renaming can be hit by accident while painting. Removals are held
//! dangers (edits propagate to every frame using the role).
//!
//! Presentation only: `CharacterPalette`/`ColorRole` serde is untouched
//! (the project format is NEVER-DO 1 territory).

use anim_core::color::RecolourScope;
use eframe::egui;
use egui::color_picker::{Alpha, color_picker_color32};
use egui::{Sense, vec2};

use crate::canvas::CanvasView;
use crate::doc::AppState;
use crate::lessons::Target;
use crate::palette::{CharacterPalette, ColorRole};
use crate::plate::{self, Card};
use crate::vocab;

/// AU13: the empty board. `Add color` stands above it.
pub(crate) const NO_COLORS: &str = "No colors yet.";
/// The character `Add color` makes the first time, unasked.
pub(crate) const DEFAULT_CHARACTER: &str = "Colors";
/// A starter swatch's cell stands this wide at the most.
const WIDEST_SWATCH: f32 = 44.0;
/// A swatch's cell stands this wide at the least, for a pen to find.
const NARROWEST_SWATCH: f32 = plate::CTRL_H - 4.0;
/// A pot: the room's most clicked non-canvas target (XL law).
const POT: egui::Vec2 = vec2(44.0, 30.0);
/// The wheel beside a pot in `Edit colors`.
const CHANGE_COLOR: &str = "Change this color everywhere it is used.";
/// The popover's area.
pub(crate) const ADDING: &str = "add color popover";
/// The picker's square: as wide as egui's row of numbers over it, so it leaves
/// no gutter.
pub(crate) const PICKER_W: f32 = 254.0;
/// A kept color's chip in the popover.
const KEPT: egui::Vec2 = vec2(24.0, 20.0);
/// The chips a popover shows, the newest last.
const KEPT_ROW: usize = 8;

/// What the Colors panel reads from the Editor.
#[derive(Clone, Copy, Default)]
pub struct View {
    /// `Result beside`, in the Color step, the one place it means anything.
    pub beside: Option<bool>,
    /// `Result beside` brings the Timing sheet in this document.
    pub beside_sheet: bool,
    /// The frozen boot keeps the fold open (spec 1.5.2).
    pub more_default: bool,
}

/// The handle a starter swatch answers to (feedback spec 5.5).
fn swatch_handle(name: &str) -> Option<Target> {
    match name {
        "Black" => Some(Target::Named("swatch.black")),
        "White" => Some(Target::Named("swatch.white")),
        "Yellow" => Some(Target::Named("swatch.yellow")),
        "Brown" => Some(Target::Named("swatch.brown")),
        "Red" => Some(Target::Named("swatch.red")),
        _ => None,
    }
}

/// The ten starters, five to the row where a pane has the width, and four or
/// three where it does not: a click arms the color and nothing else, so the
/// document does not change.
fn starters(ui: &mut egui::Ui, state: &mut AppState, canvas: &mut CanvasView) {
    // AU15: a pane under its minimum pans across; the starters keep to what
    // it shows, so all ten stand whole at once.
    let shown = ui.clip_rect().width() - 2.0 * ui.visuals().clip_rect_margin;
    let (gap, room) = (ui.spacing().item_spacing.x, ui.available_width().min(shown));
    let needs = |n: usize| n as f32 * NARROWEST_SWATCH + (n - 1) as f32 * gap;
    let columns = [5, 4].into_iter().find(|n| needs(*n) <= room).unwrap_or(3);
    let cell = ((room - (columns - 1) as f32 * gap) / columns as f32).clamp(NARROWEST_SWATCH, WIDEST_SWATCH);
    for row in plate::STARTER_SWATCHES.chunks(columns) {
        ui.horizontal(|ui| {
            for (name, color) in row {
                let held = canvas.brush_color == color.to_array();
                let resp = plate::swatch_cell(ui, vec2(cell, plate::CTRL_H), *color, held, name).card(*name);
                if let Some(handle) = swatch_handle(name) {
                    plate::mark(ui, resp.rect, handle);
                }
                if resp.clicked() {
                    canvas.brush_color = color.to_array();
                    state.active_chip = None;
                }
            }
        });
    }
}

/// `Add color`: the first use makes the default character silently, every
/// use adds one color, holding what the brush holds until the popover says
/// otherwise. The new character starts bare: nobody here asked for the
/// trade's three model tones.
fn add_color(state: &mut AppState, canvas: &CanvasView) {
    if state.palettes.characters.is_empty() {
        let mut first = CharacterPalette::new(DEFAULT_CHARACTER);
        first.roles.clear();
        state.palettes.characters.push(first);
        state.active_character = 0;
    }
    let ci = state.active_character.min(state.palettes.characters.len() - 1);
    let roles = &mut state.palettes.characters[ci].roles;
    let free = |n: &usize| !roles.iter().any(|r| r.name == format!("Color {n}"));
    let n = (roles.len() + 1..).find(free).expect("a free number");
    roles.push(ColorRole { name: format!("Color {n}"), color: canvas.brush_color });
    let ri = roles.len() - 1;
    state.active_chip = Some(ri);
    state.palette_adding = Some((ci, ri));
    state.meta_dirty = true;
}

/// What the popover would otherwise hide on the board under it: the new
/// color's name, then the character's latest colors, the new one ringed. A
/// press on `Add color` shows here at once, whatever color it took.
fn kept_ui(ui: &mut egui::Ui, roles: &[ColorRole], newest: usize) {
    ui.label(egui::RichText::new(&roles[newest].name).font(plate::Role::Chrome.font()).color(plate::INK));
    ui.horizontal(|ui| {
        for (ri, role) in roles.iter().enumerate().skip(roles.len().saturating_sub(KEPT_ROW)) {
            let (at, resp) = ui.allocate_exact_size(KEPT, Sense::hover());
            let new = ri == newest;
            plate::speak(&resp, egui::WidgetType::Label, Some(new), &format!("{}: {}", vocab::KEPT_COLOR, role.name));
            let well = at.shrink(2.0);
            let p = ui.painter();
            p.rect_filled(well, plate::R_SHEET, plate::chip([role.color[0], role.color[1], role.color[2], 255]));
            let (ring, ink) = if new { (2.0, plate::BLUE) } else { (1.0, plate::INK3) };
            p.rect_stroke(well, plate::R_SHEET, egui::Stroke::new(ring, ink), egui::StrokeKind::Outside);
        }
    });
    ui.add_space(2.0);
}

/// The popover over the newest color: egui's own picker, which stays open
/// across picks. Only a press elsewhere or Esc closes it: a press on `Add
/// color` starts the next color under it.
fn adding_ui(ui: &egui::Ui, state: &mut AppState, canvas: &mut CanvasView, add: &egui::Response) {
    let Some((ci, ri)) = state.palette_adding else {
        return;
    };
    let Some(roles) = state.palettes.characters.get(ci).map(|c| &c.roles).filter(|r| ri < r.len()) else {
        state.palette_adding = None;
        return;
    };
    let mut color = plate::chip(roles[ri].color);
    let mut open = true;
    let shown = egui::Popup::from_response(add)
        .id(egui::Id::new(ADDING))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .show(|ui| {
            ui.spacing_mut().slider_width = PICKER_W;
            kept_ui(ui, roles, ri);
            color_picker_color32(ui, &mut color, Alpha::Opaque)
        });
    let Some(shown) = shown else {
        return;
    };
    if shown.inner {
        let chosen = color.to_srgba_unmultiplied();
        state.palettes.characters[ci].roles[ri].color = chosen;
        canvas.brush_color = chosen;
        state.meta_dirty = true;
    }
    let keeps = |at: egui::Pos2| shown.response.rect.contains(at) || add.rect.contains(at);
    let elsewhere = ui.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, egui::Event::PointerButton { pos, pressed: true, .. } if !keeps(*pos))
        })
    });
    if elsewhere || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.palette_adding = None;
    }
}

/// One pot: its color, seated, ringed when armed.
fn pot(ui: &mut egui::Ui, role: &ColorRole, armed: bool) -> egui::Response {
    let (pr, presp) = ui.allocate_exact_size(POT, Sense::click());
    plate::speak(&presp, egui::WidgetType::Button, Some(armed), &format!("paint pot: {}", role.name));
    let p = ui.painter();
    let well = pr.shrink(2.0);
    p.rect_filled(well.expand(2.0), plate::R_SHEET, plate::WELL);
    p.rect_filled(well, 0.0, plate::chip([role.color[0], role.color[1], role.color[2], 255]));
    if armed {
        p.rect_stroke(well.expand(1.0), plate::R_SHEET, egui::Stroke::new(2.0, plate::BLUE), egui::StrokeKind::Outside);
    }
    presp
}

/// Returns `Result beside`'s new value, the frame it is pressed: the dock is
/// being drawn, so the Editor swaps it after.
pub fn ui(ui: &mut egui::Ui, state: &mut AppState, canvas: &mut CanvasView, view: View) -> Option<bool> {
    ui.add_space(2.0);
    starters(ui, state, canvas);
    ui.add_space(2.0);
    // Above the scroll: these never fall below the fold (spec AU13 item 4).
    let add = ui.horizontal_wrapped(|ui| {
        let add = ui.button(vocab::ADD_COLOR).card("Keep a color of your own in this project.");
        // The latch is deliberately keyless: model edits are rare and global,
        // and they cost a click.
        let mut em = state.palette_edit;
        plate::latch(ui, &mut em, vocab::EDIT_COLORS).on_hover_text(
            "Open the colors for editing. Renames, removals and new colors reach every frame that uses them.",
        );
        state.palette_edit = em;
        add
    });
    if add.inner.clicked() {
        add_color(state, canvas);
    }
    adding_ui(ui, state, canvas, &add.inner);
    let mut beside = view.beside.unwrap_or(false);
    let beside_row = view.beside.map(|_| plate::latch(ui, &mut beside, vocab::RESULT_BESIDE));
    let beside_card = if view.beside_sheet {
        "Show the Timing sheet, Layers and the Result beside the paper."
    } else {
        "Show Layers and the Result beside the paper."
    };
    let asked = beside_row.is_some_and(|row| row.card(beside_card).clicked());
    ui.add_space(2.0);
    // Above the scroll: an edit far down a long board raises this in view.
    recolour_offer_ui(ui, state);
    let scroll = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| board(ui, state, canvas, view));
    plate::note_viewport(ui, scroll.inner_rect, [false, true]);
    asked.then_some(beside)
}

/// Everything under the header, in its scroll.
fn board(ui: &mut egui::Ui, state: &mut AppState, canvas: &mut CanvasView, view: View) {
    let n_chars = state.palettes.characters.len();
    if n_chars > 0 && state.active_character >= n_chars {
        state.active_character = n_chars - 1;
    }
    let edit = state.palette_edit;
    let mut more = false;
    if n_chars == 0 {
        ui.label(egui::RichText::new(NO_COLORS).color(plate::legend_dim()));
    } else {
        more = state.panel_more.open.get(plate::MORE_COLORS).copied().unwrap_or(view.more_default);
        // `Edit colors` shows it all, so the fold would do nothing.
        if !edit && plate::fold_row(ui, vocab::COLOR_DETAILS, ui.available_width(), more).clicked() {
            more = !more;
            state.panel_more.set(plate::MORE_COLORS, more);
        }
    }
    let shown = more || edit;
    if shown {
        // Character DETENT tabs — one board at a time (today's all-characters
        // stack made every pick a scroll hunt).
        ui.horizontal_wrapped(|ui| {
            for ci in 0..n_chars {
                let name = state.palettes.characters[ci].name.clone();
                // Under the fold's `DETENT`s, a name of its own.
                if plate::detent(ui, state.active_character == ci, &name).clicked() {
                    if state.active_character != ci {
                        // GM7: the active chip is a pot of the board on show.
                        state.active_chip = None;
                    }
                    state.active_character = ci;
                }
            }
        });
    }
    ui.add_space(2.0);
    let ci = state.active_character.min(n_chars.saturating_sub(1));
    let mut remove_role: Option<usize> = None;
    // GM2: palette edits live outside the undo system — they mark the
    // document dirty directly.
    let mut model_changed = false;
    // GM7: a chip the wheel changed (role, old, new) — offers a recolour —
    // and the pot a click armed (the active chip).
    let mut chip_changed: Option<(usize, [u8; 4], [u8; 4])> = None;
    let mut armed_pot: Option<usize> = None;
    // GM7 CHIP PICK: one ringed pot — the active chip, else the first pot
    // of the brush colour (two pots sharing a colour never both ring).
    let ring = state.ringed_chip(canvas.brush_color);
    if let Some(ch) = state.palettes.characters.get_mut(ci) {
        if !shown {
            // The pots alone: a color is its own name.
            ui.horizontal_wrapped(|ui| {
                for (ri, role) in ch.roles.iter().enumerate() {
                    // No `push_id` here: a scope's child never wraps.
                    let presp = pot(ui, role, ring == Some(ri));
                    if presp.clicked() {
                        canvas.brush_color = role.color;
                        armed_pot = Some(ri);
                    }
                    presp.on_hover_text(format!("Use '{}'", role.name));
                }
            });
        }
        for (ri, role) in ch.roles.iter_mut().enumerate().filter(|_| shown) {
            ui.push_id(ri, |ui| {
                ui.horizontal(|ui| {
                    let presp = pot(ui, role, ring == Some(ri));
                    if presp.clicked() {
                        canvas.brush_color = role.color;
                        armed_pot = Some(ri);
                    }
                    let key_hint = if ri < 8 {
                        format!(" · press {} with Fill", ri + 1)
                    } else {
                        String::new()
                    };
                    presp.on_hover_text(format!("Use '{}'{key_hint}", role.name));
                    if edit {
                        // Edit colors: the wheel lives here and only here;
                        // edits propagate everywhere.
                        let mut rgb = [role.color[0], role.color[1], role.color[2]];
                        if ui.color_edit_button_srgb(&mut rgb).card(CHANGE_COLOR).changed() {
                            let old = role.color;
                            role.color = [rgb[0], rgb[1], rgb[2], role.color[3]];
                            chip_changed = Some((ri, old, role.color));
                            model_changed = true;
                        }
                        if plate::text_field(
                            ui,
                            egui::TextEdit::singleline(&mut role.name)
                                .hint_text(vocab::COLOR_NAME)
                                .desired_width(72.0),
                        )
                        .changed()
                        {
                            model_changed = true;
                        }
                        if plate::danger(ui, "remove") {
                            remove_role = Some(ri);
                        }
                    } else {
                        // Mid-metronome: the name, in full ink (the colour
                        // designer authored it), nothing else.
                        ui.label(
                            egui::RichText::new(role.name.clone())
                                .font(plate::Role::Small.font())
                                .color(plate::INK),
                        );
                    }
                });
            });
            ui.add_space(4.0);
        }
    }
    if let Some(ri) = remove_role {
        state.palettes.characters[ci].roles.remove(ri);
        // GM13: the variants stay aligned to the model.
        for v in &mut state.palettes.characters[ci].variants {
            if ri < v.colors.len() {
                v.colors.remove(ri);
            }
        }
        // GM7: the pots after it moved up one; the ring finds its colour.
        state.active_chip = None;
        model_changed = true;
    }
    if model_changed {
        state.meta_dirty = true;
    }
    if let Some((ri, old, new)) = chip_changed {
        // GM13: a variant chip left as the model's follows the model.
        for v in &mut state.palettes.characters[ci].variants {
            if v.colors.get(ri) == Some(&old) {
                v.colors[ri] = new;
            }
        }
        state.offer_recolour(ci, ri, old, new);
    }
    if armed_pot.is_some() {
        state.active_chip = armed_pot;
    }
    if shown && !state.palettes.characters.is_empty() {
        variants_ui(ui, state, ci);
    }
    if state.palette_edit {
        ui.separator();
        ui.horizontal(|ui| {
            plate::text_field(
                ui,
                egui::TextEdit::singleline(&mut state.palette_new_name)
                    .hint_text("character name…")
                    .desired_width(110.0),
            );
            let name = state.palette_new_name.trim().to_string();
            if ui.button("+ character").clicked() && !name.is_empty() {
                state.palettes.characters.push(CharacterPalette::new(name));
                state.active_character = state.palettes.characters.len() - 1;
                state.active_chip = None;
                state.palette_new_name.clear();
                state.meta_dirty = true;
            }
        });
        if !state.palettes.characters.is_empty() && plate::danger(ui, "REMOVE CHARACTER") {
            state.palettes.characters.remove(ci);
            state.active_character = 0;
            state.active_chip = None;
            state.meta_dirty = true;
        }
    }
}

/// GM13: the character's time-of-day variants (day, evening, night ...): a
/// row of chips each, aligned to the model's roles; double-click a name to
/// rename it. A Palette map node set to a variant follows it (and the
/// model): once an edit ends (the pointer is up), the stale nodes refresh
/// in one undo step ([`AppState::follow_palette_variants`]).
fn variants_ui(ui: &mut egui::Ui, state: &mut AppState, ci: usize) {
    let name_id = egui::Id::new("palette variant name");
    let rename_id = egui::Id::new(("palette variant rename", ci));
    ui.separator();
    plate::legend(ui, vocab::COLOR_SETS).on_hover_text(
        "time-of-day colours of this character, one per chip: a Palette map node in the \
         graph swaps the model's chips for a variant's",
    );
    let mut stale = false;
    let mut remove: Option<usize> = None;
    let mut renaming: Option<(usize, String)> =
        ui.data_mut(|d| d.get_temp(rename_id).unwrap_or_default());
    let mut renamed: Option<(usize, String)> = None;
    let ch = &mut state.palettes.characters[ci];
    let roles: Vec<(String, [u8; 4])> =
        ch.roles.iter().map(|r| (r.name.clone(), r.color)).collect();
    for (vi, v) in ch.variants.iter_mut().enumerate() {
        ui.horizontal_wrapped(|ui| {
            let mut close = false;
            match renaming.as_mut().filter(|(at, _)| *at == vi) {
                Some((_, text)) => {
                    let r = plate::text_field(ui, egui::TextEdit::singleline(text).desired_width(72.0));
                    if r.lost_focus() {
                        // Enter or a click away renames; Esc keeps the name.
                        if !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            renamed = Some((vi, text.clone()));
                        }
                        close = true;
                    } else if !r.has_focus() {
                        r.request_focus();
                    }
                }
                None => {
                    if ui
                        .add(egui::Label::new(&v.name).sense(egui::Sense::click()))
                        .on_hover_text("double-click to rename")
                        .double_clicked()
                    {
                        renaming = Some((vi, v.name.clone()));
                    }
                }
            }
            if close {
                renaming = None;
            }
            for (ri, (role, model)) in roles.iter().enumerate() {
                let mut c = v.colors.get(ri).copied().unwrap_or(*model);
                if ui
                    .color_edit_button_srgba_unmultiplied(&mut c)
                    .on_hover_text(format!("{role}: original → {}", v.name))
                    .changed()
                {
                    v.colors.resize(ri.max(v.colors.len()), *model);
                    if ri < v.colors.len() {
                        v.colors[ri] = c;
                    } else {
                        v.colors.push(c);
                    }
                    stale = true;
                }
            }
            if ui.small_button("×").on_hover_text("remove this variant").clicked() {
                remove = Some(vi);
            }
        });
    }
    if let Some(vi) = remove {
        ch.variants.remove(vi);
        renaming = None;
    }
    let mut name: String = ui.data_mut(|d| d.get_temp(name_id).unwrap_or_default());
    ui.horizontal(|ui| {
        plate::text_field(
            ui,
            egui::TextEdit::singleline(&mut name)
                .hint_text("Set name")
                .desired_width(110.0),
        );
        let trimmed = name.trim().to_string();
        if ui.button(format!("+ {}", vocab::COLOR_SET)).clicked() && !trimmed.is_empty() {
            ch.add_variant(trimmed);
            name.clear();
            stale = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(name_id, name));
    ui.data_mut(|d| d.insert_temp(rename_id, renaming));
    if stale || remove.is_some() {
        state.meta_dirty = true;
    }
    if let Some((vi, text)) = renamed {
        state.rename_palette_variant(ci, vi, &text);
    }
}

/// GM7 RECOLOUR: after a chip changed, offer to repaint the paint of its
/// old colour — this cut, this scene or the whole project, one undo step
/// each (paint roles only; line art is never touched).
fn recolour_offer_ui(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(o) = state.recolour_offer.clone() else {
        return;
    };
    let hex = |c: [u8; 4]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
    plate::legend(ui, "Recolor paint");
    ui.label(
        egui::RichText::new(format!("'{}' {} → {}", o.name, hex(o.from), hex(o.to)))
            .color(plate::legend_dim()),
    );
    // The repaint matches the old colour, not the chip: another character
    // whose chip shares it (skin tones often do) is repainted too.
    let shared: Vec<&str> = state
        .palettes
        .characters
        .iter()
        .enumerate()
        .filter(|(ci, c)| *ci != o.character && c.roles.iter().any(|r| r.color[..3] == o.from[..3]))
        .map(|(_, c)| c.name.as_str())
        .collect();
    if !shared.is_empty() {
        ui.label(
            egui::RichText::new(format!("…also used by {} — its paint changes too", shared.join(", ")))
                .color(plate::legend_dim()),
        );
    }
    const TOUCHES: &str =
        "Paint layers only (Color, Shadow, Highlight, Custom). Line art is never touched. One undo step.";
    let (at, scene) = (state.at(), state.view.scene);
    ui.horizontal_wrapped(|ui| {
        if ui.button("Recolor this shot").on_hover_text(TOUCHES).clicked() {
            state.recolour(RecolourScope::Cut(at));
        }
        if ui.button("Recolor this scene").on_hover_text(TOUCHES).clicked() {
            state.recolour(RecolourScope::Scene(scene));
        }
        if ui.button("Recolor project").on_hover_text(TOUCHES).clicked() {
            state.recolour(RecolourScope::Project);
        }
        if ui.button("Keep paint").on_hover_text("Leave the paint as it is.").clicked() {
            state.recolour_offer = None;
        }
    });
}
