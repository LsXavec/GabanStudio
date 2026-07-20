//! Character color palette pane (B5): named per-character colour roles
//! (normal/shadow/highlight/...), project-persisted. Click a swatch to load
//! it as the brush colour — the same one-click-apply UX as brush presets.

use eframe::egui;

use crate::canvas::CanvasView;
use crate::doc::AppState;
use crate::palette::{CharacterPalette, ColorRole};

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, canvas: &mut CanvasView) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(
            "Per-character colour sets — click a swatch to load it as the \
             brush colour. Saved with the project.",
        )
        .weak(),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.palette_new_name)
                .hint_text("character name…")
                .desired_width(130.0),
        );
        let name = state.palette_new_name.trim().to_string();
        if ui.button("+ character").clicked() && !name.is_empty() {
            state.palettes.characters.push(CharacterPalette::new(name));
            state.palette_new_name.clear();
        }
    });
    ui.separator();

    let mut remove_char: Option<usize> = None;
    let mut add_role: Option<usize> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (ci, ch) in state.palettes.characters.iter_mut().enumerate() {
            ui.push_id(ci, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&ch.name).strong());
                    if ui.small_button("+ role").on_hover_text("add a colour role").clicked() {
                        add_role = Some(ci);
                    }
                    if ui
                        .small_button("✕ character")
                        .on_hover_text("remove this character")
                        .clicked()
                    {
                        remove_char = Some(ci);
                    }
                });
                let mut remove_role: Option<usize> = None;
                for (ri, role) in ch.roles.iter_mut().enumerate() {
                    ui.push_id(ri, |ui| {
                        ui.horizontal(|ui| {
                            let swatch =
                                egui::Color32::from_rgb(role.color[0], role.color[1], role.color[2]);
                            if ui
                                .add(
                                    egui::Button::new("")
                                        .fill(swatch)
                                        .min_size(egui::vec2(26.0, 18.0)),
                                )
                                .on_hover_text(format!("apply '{}' as the brush colour", role.name))
                                .clicked()
                            {
                                canvas.brush_color = role.color;
                            }
                            let mut rgb = [role.color[0], role.color[1], role.color[2]];
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                role.color = [rgb[0], rgb[1], rgb[2], role.color[3]];
                            }
                            ui.add(
                                egui::TextEdit::singleline(&mut role.name).desired_width(80.0),
                            );
                            if ui.small_button("✕").on_hover_text("remove role").clicked() {
                                remove_role = Some(ri);
                            }
                        });
                    });
                }
                if let Some(ri) = remove_role {
                    ch.roles.remove(ri);
                }
                ui.add_space(6.0);
                ui.separator();
            });
        }
    });
    // Deferred: both need &mut on the Vec at the top level, which the loop
    // above is already borrowing element-wise.
    if let Some(ci) = add_role {
        state.palettes.characters[ci].roles.push(ColorRole {
            name: "role".into(),
            color: [200, 200, 200, 255],
        });
    }
    if let Some(ci) = remove_char {
        state.palettes.characters.remove(ci);
    }
    if state.palettes.characters.is_empty() {
        ui.label(egui::RichText::new("no characters yet — add one above").weak());
    }
}
