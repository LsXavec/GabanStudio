//! The exposure sheet panel — frames as rows (the paper timesheet layout Toei
//! kept in its own digital exposure sheet), columns as layers. Keys show as
//! ● + name; held frames show as │. Click any row to jump the playhead.

use anim_core::xsheet::Exposure;
use eframe::egui;
use egui::{Color32, Rect, Sense, pos2, vec2};

use crate::doc::AppState;

const ROW_H: f32 = 18.0;
const FRAME_NUM_W: f32 = 34.0;
const COL_W: f32 = 86.0;

pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} — X-SHEET", state.cut().name))
                .strong()
                .color(Color32::from_rgb(120, 190, 255)),
        );
        ui.label(format!("{} fps", state.fps()));
    });

    // Column tabs (active column = the one the pen draws into).
    ui.horizontal(|ui| {
        ui.label("cols:");
        let columns: Vec<(anim_core::ids::ColumnId, String)> = state
            .cut()
            .xsheet
            .columns
            .iter()
            .map(|c| (c.id, c.name.clone()))
            .collect();
        for (id, name) in columns {
            if ui
                .selectable_label(state.active_column == id, name)
                .clicked()
            {
                state.active_column = id;
            }
        }
        if ui.button("+").on_hover_text("add column (layer)").clicked() {
            state.add_column();
        }
    });

    ui.horizontal(|ui| {
        if ui.button("new drawing").on_hover_text("create + expose at current frame (N)").clicked() {
            state.new_drawing_at_frame();
        }
        if ui.button("expose sel.").on_hover_text("expose selected library drawing here").clicked() {
            state.expose_selected();
        }
        if ui.button("clear key").on_hover_text("remove key: previous hold extends").clicked() {
            state.clear_key_at_frame();
        }
    });

    // Drawing library.
    ui.separator();
    ui.label("drawings:");
    ui.horizontal_wrapped(|ui| {
        let drawings: Vec<(anim_core::ids::DrawingId, String)> = state
            .cut()
            .drawings
            .iter()
            .map(|d| (d.id, d.name.clone()))
            .collect();
        if drawings.is_empty() {
            ui.label(egui::RichText::new("(none yet — just draw)").weak());
        }
        for (id, name) in drawings {
            if ui
                .selectable_label(state.selected_drawing == Some(id), name)
                .clicked()
            {
                state.selected_drawing = Some(id);
            }
        }
    });
    ui.separator();

    // ---- The sheet itself -------------------------------------------------
    let n_cols = state.cut().xsheet.columns.len();
    let sheet_w = FRAME_NUM_W + n_cols as f32 * COL_W;

    // Header row.
    ui.horizontal(|ui| {
        ui.allocate_exact_size(vec2(FRAME_NUM_W, ROW_H), Sense::hover());
        for col in &state.cut().xsheet.columns {
            let (rect, _) = ui.allocate_exact_size(vec2(COL_W, ROW_H), Sense::hover());
            let is_active = col.id == state.active_column;
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                &col.name,
                egui::FontId::proportional(12.0),
                if is_active {
                    Color32::from_rgb(120, 190, 255)
                } else {
                    Color32::from_gray(160)
                },
            );
        }
    });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let frame_count = state.frame_count();
            let fps = state.fps();
            let mut clicked_frame: Option<u32> = None;

            for frame in 0..frame_count {
                let (row_rect, resp) = ui.allocate_exact_size(
                    vec2(sheet_w.max(ui.available_width()), ROW_H),
                    Sense::click(),
                );
                if resp.clicked() {
                    clicked_frame = Some(frame);
                }
                let painter = ui.painter_at(row_rect);

                // Row background: playhead > second marks > default.
                let is_current = frame == state.frame;
                if is_current {
                    painter.rect_filled(row_rect, 0, Color32::from_rgb(45, 62, 88));
                } else if fps > 0 && frame % fps == 0 {
                    painter.rect_filled(row_rect, 0, Color32::from_gray(38));
                } else if fps > 1 && frame % (fps / 2).max(1) == 0 {
                    painter.rect_filled(row_rect, 0, Color32::from_gray(31));
                }

                // Frame number (1-based like paper sheets).
                painter.text(
                    pos2(row_rect.left() + FRAME_NUM_W - 6.0, row_rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{}", frame + 1),
                    egui::FontId::monospace(11.0),
                    if is_current {
                        Color32::WHITE
                    } else {
                        Color32::from_gray(140)
                    },
                );

                // Cells.
                let cut = state.cut();
                for (ci, col) in cut.xsheet.columns.iter().enumerate() {
                    let x = row_rect.left() + FRAME_NUM_W + ci as f32 * COL_W;
                    let cell = Rect::from_min_size(pos2(x, row_rect.top()), vec2(COL_W, ROW_H));
                    let (text, color) = match col.key_at(frame) {
                        Some(Exposure::Drawing(d)) => {
                            let name = cut
                                .drawing(d)
                                .map(|dr| dr.name.clone())
                                .unwrap_or_else(|| format!("{d}"));
                            (format!("● {name}"), Color32::from_rgb(230, 210, 120))
                        }
                        Some(Exposure::Empty) => ("○".to_string(), Color32::from_gray(120)),
                        None => {
                            if col.resolve(frame).is_some() {
                                ("│".to_string(), Color32::from_gray(95))
                            } else {
                                (String::new(), Color32::TRANSPARENT)
                            }
                        }
                    };
                    if !text.is_empty() {
                        painter.text(
                            pos2(cell.left() + 6.0, cell.center().y),
                            egui::Align2::LEFT_CENTER,
                            text,
                            egui::FontId::proportional(11.0),
                            color,
                        );
                    }
                }
            }

            if let Some(f) = clicked_frame {
                state.goto(f);
            }
        });
}
