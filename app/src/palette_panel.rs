//! THE SWATCH BOARD (shiage room charter, built 2026-08-17): the painter's
//! primary instrument. One character at a time behind DETENT tabs; pots are
//! 44×30 seated wells — a pot's own colour is its icon, grid position its
//! address — with the armed pot ringed in Tally. The model is EDITED only
//! behind the EDIT MODEL latch: mid-metronome the board is pure pots, and
//! nothing destructive or renaming can be hit by accident. Removals are
//! held DANGERs (model edits propagate to every frame using the role).
//!
//! Presentation only: `CharacterPalette`/`ColorRole` serde is untouched
//! (the project format is NEVER-DO 1 territory).

use anim_core::color::RecolourScope;
use eframe::egui;
use egui::{Color32, Sense, vec2};

use crate::canvas::CanvasView;
use crate::doc::AppState;
use crate::palette::{CharacterPalette, ColorRole};
use crate::plate;

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, canvas: &mut CanvasView) {
    ui.add_space(4.0);

    // Character DETENT tabs — one board at a time (today's all-characters
    // stack made every pick a scroll hunt).
    let n_chars = state.palettes.characters.len();
    if n_chars > 0 && state.active_character >= n_chars {
        state.active_character = n_chars - 1;
    }
    ui.horizontal_wrapped(|ui| {
        for ci in 0..n_chars {
            let name = state.palettes.characters[ci].name.clone();
            if plate::detent(ui, state.active_character == ci, &name).clicked() {
                if state.active_character != ci {
                    // GM7: the active chip is a pot of the board on show.
                    state.active_chip = None;
                }
                state.active_character = ci;
            }
        }
    });
    if n_chars == 0 {
        ui.label(egui::RichText::new("no characters yet — latch EDIT MODEL and add one").color(plate::legend_dim()));
    }
    recolour_offer_ui(ui, state);
    ui.add_space(4.0);
    let edit = state.palette_edit;
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
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(ch) = state.palettes.characters.get_mut(ci) {
                for (ri, role) in ch.roles.iter_mut().enumerate() {
                    ui.push_id(ri, |ui| {
                        ui.horizontal(|ui| {
                            // THE POT: 44×30, seated — the room's most
                            // clicked non-canvas target (XL law).
                            let (pr, presp) =
                                ui.allocate_exact_size(vec2(44.0, 30.0), Sense::click());
                            let armed = ring == Some(ri);
                            plate::speak(
                                &presp,
                                egui::WidgetType::Button,
                                Some(armed),
                                &format!("paint pot: {}", role.name),
                            );
                            let p = ui.painter();
                            let well = pr.shrink(2.0);
                            p.rect_filled(well.expand(2.0), 0.0, plate::WELL);
                            p.rect_filled(
                                well,
                                0.0,
                                Color32::from_rgba_unmultiplied(
                                    role.color[0],
                                    role.color[1],
                                    role.color[2],
                                    255,
                                ),
                            );
                            if armed {
                                p.rect_stroke(
                                    well.expand(1.0),
                                    0.0,
                                    egui::Stroke::new(2.0, plate::TALLY),
                                    egui::StrokeKind::Outside,
                                );
                            } else {
                                p.rect_stroke(
                                    well,
                                    0.0,
                                    egui::Stroke::new(1.0, plate::LEGEND),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if presp.clicked() {
                                canvas.brush_color = role.color;
                                armed_pot = Some(ri);
                            }
                            let key_hint = if ri < 8 {
                                format!(" · key {} with fill", ri + 1)
                            } else {
                                String::new()
                            };
                            presp.on_hover_text(format!("arm '{}'{key_hint}", role.name));
                            if edit {
                                // EDIT MODEL: the wheel lives here and only
                                // here; edits propagate everywhere.
                                let mut rgb = [role.color[0], role.color[1], role.color[2]];
                                if ui.color_edit_button_srgb(&mut rgb).changed() {
                                    let old = role.color;
                                    role.color = [rgb[0], rgb[1], rgb[2], role.color[3]];
                                    chip_changed = Some((ri, old, role.color));
                                    model_changed = true;
                                }
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut role.name)
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
                                // Mid-metronome: the name, Struck (the
                                // colour designer authored it), nothing else.
                                ui.label(
                                    egui::RichText::new(role.name.clone())
                                        .size(11.5)
                                        .color(plate::STRUCK),
                                );
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
                if edit && ui.button("+ role").clicked() {
                    ch.roles.push(ColorRole {
                        name: "role".into(),
                        color: [200, 200, 200, 255],
                    });
                    model_changed = true;
                }
            }
        });
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
        state.offer_recolour(ci, ri, old, new);
    }
    if armed_pot.is_some() {
        state.active_chip = armed_pot;
    }
    if !state.palettes.characters.is_empty() {
        variants_ui(ui, state, ci);
    }

    // Footer: the EDIT MODEL latch — deliberately keyless (model edits are
    // rare and global; they cost a click).
    ui.separator();
    let mut em = state.palette_edit;
    plate::latch(ui, &mut em, "edit model").on_hover_text(
        "open the model for editing — renames, removals and new roles \
    propagate to every frame using them",
    );
    state.palette_edit = em;
    if state.palette_edit {
        ui.horizontal(|ui| {
            ui.add(
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
/// row of chips each, aligned to the model's roles. A Palette map node set
/// to a variant follows it: once a chip edit ends (the pointer is up), the
/// stale nodes refresh in one undo step.
fn variants_ui(ui: &mut egui::Ui, state: &mut AppState, ci: usize) {
    let stale_id = egui::Id::new("palette variants stale");
    let name_id = egui::Id::new("palette variant name");
    ui.separator();
    plate::legend(ui, "variants").on_hover_text(
        "time-of-day colours of this character, one per chip: a Palette map node in the \
         graph swaps the model's chips for a variant's",
    );
    let mut stale = false;
    let mut remove: Option<usize> = None;
    let ch = &mut state.palettes.characters[ci];
    let roles: Vec<[u8; 4]> = ch.roles.iter().map(|r| r.color).collect();
    for (vi, v) in ch.variants.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(&v.name);
            for (ri, model) in roles.iter().enumerate() {
                let mut c = v.colors.get(ri).copied().unwrap_or(*model);
                if ui.color_edit_button_srgba_unmultiplied(&mut c).changed() {
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
    }
    let mut name: String = ui.data_mut(|d| d.get_temp(name_id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text("variant name… (evening)")
                .desired_width(110.0),
        );
        let trimmed = name.trim().to_string();
        if ui.button("+ variant").clicked() && !trimmed.is_empty() {
            ch.add_variant(trimmed);
            name.clear();
            stale = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(name_id, name));
    if stale || remove.is_some() {
        state.meta_dirty = true;
        ui.data_mut(|d| d.insert_temp(stale_id, true));
    }
    let pending = ui.data(|d| d.get_temp::<bool>(stale_id).unwrap_or(false));
    if pending && !ui.input(|i| i.pointer.any_down()) {
        ui.data_mut(|d| d.insert_temp(stale_id, false));
        state.refresh_palette_maps();
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
    plate::legend(ui, "recolour paint");
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
        "paint layers only (Color/Shadow/Highlight/Custom); line art is never touched; one undo step";
    let (at, scene) = (state.at(), state.view.scene);
    ui.horizontal_wrapped(|ui| {
        if ui.button("Recolour this cut").on_hover_text(TOUCHES).clicked() {
            state.recolour(RecolourScope::Cut(at));
        }
        if ui.button("Recolour this scene").on_hover_text(TOUCHES).clicked() {
            state.recolour(RecolourScope::Scene(scene));
        }
        if ui.button("Recolour project").on_hover_text(TOUCHES).clicked() {
            state.recolour(RecolourScope::Project);
        }
        if ui.button("Keep paint").on_hover_text("leave the paint as it is").clicked() {
            state.recolour_offer = None;
        }
    });
}
