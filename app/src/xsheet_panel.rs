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
            // Content origin (top-left of frame 0's row) for handle geometry.
            let mut geom: Option<(f32, f32)> = None;

            for frame in 0..frame_count {
                let (row_rect, resp) = ui.allocate_exact_size(
                    vec2(sheet_w.max(ui.available_width()), ROW_H),
                    Sense::click(),
                );
                if frame == 0 {
                    geom = Some((row_rect.top(), row_rect.left()));
                }
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

            // ---- Continuation-frame-line handles ---------------------------
            // Each held drawing that has room before the next drawing gets a
            // draggable handle to shorten its exposure (placing an Empty key);
            // the frames after it then go blank until the next drawing.
            if let Some((ctop, cleft)) = geom {
                continuation_handles(ui, state, ctop, cleft, frame_count);
            }

            if let Some(f) = clicked_frame {
                state.goto(f);
            }
        });
}

/// One drawing's exposure span and its optional early-end terminator.
struct Cont {
    column: anim_core::ids::ColumnId,
    ci: usize,
    n: u32,           // the drawing key's frame
    max_end: u32,     // next drawing frame, or frame_count (the natural end)
    terminator: Option<u32>, // Empty key ending it early, if edited
}

fn continuation_handles(
    ui: &mut egui::Ui,
    state: &mut AppState,
    ctop: f32,
    cleft: f32,
    frame_count: u32,
) {
    // Gather spans (owned, so we can mutate state afterwards).
    let mut conts: Vec<Cont> = Vec::new();
    for (ci, col) in state.cut().xsheet.columns.iter().enumerate() {
        let keys: Vec<(u32, Exposure)> = col.keys().collect();
        for (i, (n, exp)) in keys.iter().enumerate() {
            if !matches!(exp, Exposure::Drawing(_)) {
                continue;
            }
            let n = *n;
            let mut max_end = frame_count;
            let mut terminator = None;
            for (m, e) in &keys[i + 1..] {
                match e {
                    Exposure::Drawing(_) => {
                        max_end = *m;
                        break;
                    }
                    Exposure::Empty => {
                        if terminator.is_none() {
                            terminator = Some(*m);
                        }
                    }
                }
            }
            // Only when there's a gap to control (not adjacent to a drawing).
            if max_end > n + 1 {
                conts.push(Cont { column: col.id, ci, n, max_end, terminator });
            }
        }
    }

    let painter = ui.painter();
    let hover = ui.input(|i| i.pointer.hover_pos());
    let mut pending: Vec<(anim_core::ids::ColumnId, Option<u32>, Option<u32>)> = Vec::new();

    for c in &conts {
        let col_x = cleft + FRAME_NUM_W + c.ci as f32 * COL_W;
        let x = col_x + COL_W - 10.0;
        let hold_end = c.terminator.unwrap_or(c.max_end);
        let handle_y = ctop + hold_end as f32 * ROW_H;
        let start_y = ctop + (c.n as f32 + 1.0) * ROW_H;
        let handle_center = pos2(x, handle_y);

        // Interact first (so the handle wins over the row click).
        let id = ui.id().with(("cont", c.column.0, c.n));
        let hr = ui.interact(
            Rect::from_center_size(handle_center, vec2(16.0, 16.0)),
            id,
            Sense::drag(),
        );

        // Show if edited, hovered within the span, or being dragged.
        let span_rect = Rect::from_min_max(
            pos2(col_x, ctop + c.n as f32 * ROW_H),
            pos2(col_x + COL_W, ctop + c.max_end as f32 * ROW_H),
        );
        let hovered_span = hover.is_some_and(|p| span_rect.contains(p));
        let edited = c.terminator.is_some();
        let show = edited || hovered_span || hr.dragged();

        if show {
            let col = if hr.dragged() || hr.hovered() {
                Color32::WHITE
            } else if edited {
                Color32::from_rgb(230, 160, 90)
            } else {
                Color32::from_gray(150)
            };
            painter.line_segment(
                [pos2(x, start_y), pos2(x, handle_y)],
                egui::Stroke::new(1.5, col),
            );
            painter.circle_stroke(handle_center, 4.0, egui::Stroke::new(1.5, col));
        }

        if hr.dragged()
            && let Some(p) = hr.interact_pointer_pos() {
                let raw = ((p.y - ctop) / ROW_H).round() as i32;
                let new_end = raw.clamp(c.n as i32 + 1, c.max_end as i32) as u32;
                let new_term = if new_end >= c.max_end {
                    None // dragged to/past the natural end → no terminator
                } else {
                    Some(new_end)
                };
                if new_term != c.terminator {
                    pending.push((c.column, c.terminator, new_term));
                }
            }
    }

    for (column, old, new) in pending {
        state.set_hold_terminator(column, old, new);
    }
}
