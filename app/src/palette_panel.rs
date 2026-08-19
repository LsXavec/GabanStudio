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
                state.active_character = ci;
            }
        }
    });
    if n_chars == 0 {
        ui.label(egui::RichText::new("no characters yet — latch EDIT MODEL and add one").color(plate::legend_dim()));
    }
    ui.add_space(4.0);
    let edit = state.palette_edit;
    let ci = state.active_character.min(n_chars.saturating_sub(1));
    let mut remove_role: Option<usize> = None;
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
                            let armed = canvas.brush_color[..3] == role.color[..3];
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
                                    role.color = [rgb[0], rgb[1], rgb[2], role.color[3]];
                                }
                                ui.add(
                                    egui::TextEdit::singleline(&mut role.name).desired_width(72.0),
                                );
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
                }
            }
        });
    if let Some(ri) = remove_role {
        state.palettes.characters[ci].roles.remove(ri);
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
                state.palette_new_name.clear();
            }
        });
        if !state.palettes.characters.is_empty() && plate::danger(ui, "REMOVE CHARACTER") {
            state.palettes.characters.remove(ci);
            state.active_character = 0;
        }
    }
}
