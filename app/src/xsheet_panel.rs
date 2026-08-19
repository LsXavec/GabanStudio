//! The exposure sheet panel — frames as rows (the paper timesheet layout Toei
//! kept in its own digital exposure sheet), columns as layers. Keys show as
//! ● + name; held frames show as │. Click any row to jump the playhead.

use anim_core::ids::LayerId;
use anim_core::model::LayerProps;
use anim_core::xsheet::Exposure;
use eframe::egui;
use egui::{Color32, Rect, Sense, pos2, vec2};

use crate::canvas::layer_chip_color;
use crate::doc::AppState;
use crate::icons::Icon;
use crate::plate;

const ROW_H: f32 = 18.0;
/// The gutter: an 18pt seconds sub-column + 26pt frame numbers (spec §2.3).
const GUTTER_W: f32 = 44.0;
const COL_W: f32 = 86.0;
/// Parameter (camera) columns are numeric — narrower than drawing columns.
const PARAM_COL_W: f32 = 58.0;
/// The audio waveform column (frames are rows, so the waveform runs DOWN
/// the sheet — the paper timesheet's audio track).
const AUDIO_COL_W: f32 = 46.0;

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, edit_room: bool, gesture_safe: bool) {
    // Claim the full panel width EVERY frame. egui 0.35 panels are
    // content-sized (the stored panel rect is the content rect, re-measured
    // per frame), so variable-width rows — the layer strip swapping between
    // full rows and the held-frame note during playback, changing text —
    // would otherwise wobble the panel width and nudge the canvas
    // horizontally on every frame of playback.
    ui.set_min_width(ui.available_width());

    // (The title row is EVICTED — the dock tab and the Slate already name
    // this sheet; ~145pt of header chrome becomes grid rows. Spec §4.2.)
    ui.add_space(2.0);

    // ---- THE HENSHU HEAD (charter §B): ONE fixed 26pt row, Edit room
    // only — cut nav, the cut's identity, standing LIFT KEY, and the
    // HOLD readout. The editor of timing stays the tail drag; this row
    // answers the questions around it.
    if edit_room {
        ui.horizontal(|ui| {
            for (icon, fwd, hover) in [
                (Icon::ChevL, false, "previous cut (PageUp)"),
                (Icon::ChevR, true, "next cut (PageDown)"),
            ] {
                let (cr, cresp) = ui.allocate_exact_size(vec2(44.0, 26.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(cr, 0.0, plate::WELL);
                p.rect_stroke(
                    cr,
                    0.0,
                    egui::Stroke::new(
                        1.0,
                        if cresp.hovered() {
                            plate::LEGEND
                        } else {
                            plate::legend_dim()
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                crate::icons::paint(
                    p,
                    Rect::from_center_size(cr.center(), vec2(16.0, 16.0)),
                    if cresp.hovered() {
                        plate::STRUCK
                    } else {
                        plate::LEGEND
                    },
                    icon,
                );
                if cresp.clicked() && gesture_safe {
                    state.step_cut(fwd);
                }
                cresp.on_hover_text(hover);
                if !fwd {
                    // The cut's identity field sits between the chevrons.
                    let cname = state.cut().name.clone();
                    ui.menu_button(
                        egui::RichText::new(cname.chars().take(10).collect::<String>())
                            .monospace()
                            .color(plate::STRUCK),
                        |ui| {
                            let scenes: Vec<(
                                anim_core::ids::SceneId,
                                Vec<(anim_core::ids::CutId, String)>,
                            )> = state
                                .engine
                                .project
                                .scenes
                                .iter()
                                .map(|sc| {
                                    (
                                        sc.id,
                                        sc.cuts.iter().map(|c| (c.id, c.name.clone())).collect(),
                                    )
                                })
                                .collect();
                            for (sid, cuts) in scenes {
                                for (cid, name) in cuts {
                                    let here = state.view.scene == sid && state.view.cut == cid;
                                    if ui.selectable_label(here, name).clicked()
                                        && !here
                                        && gesture_safe
                                    {
                                        state.goto_cut(sid, cid);
                                        ui.close();
                                    }
                                }
                            }
                        },
                    );
                }
            }
            ui.add_space(8.0);
            // Standing LIFT KEY (charter element 12): the big target is
            // safe BECAUSE the guard is temporal — the hold, not the size.
            if plate::danger(ui, "LIFT KEY") && gesture_safe {
                state.clear_key_at_frame();
            }
            ui.add_space(8.0);
            // HOLD readout: "am I on twos" without counting rows.
            plate::legend(ui, "hold");
            let hold_txt = {
                let count = state.frame_count();
                let frame = state.view.frame;
                let active = state.view.active_column;
                let cut = state.cut();
                cut.xsheet
                    .columns
                    .iter()
                    .find(|c| c.id == active)
                    .and_then(|col| {
                        let marks = crate::runs::column_marks(count, |f| col.key_at(f));
                        marks.runs.iter().find_map(|r| {
                            (frame >= r.start && frame <= r.end).then(|| {
                                let len = r.end - r.start + 1;
                                format!("{len}f · on {len}s")
                            })
                        })
                    })
                    .unwrap_or_else(|| "—".into())
            };
            ui.label(
                egui::RichText::new(hold_txt)
                    .monospace()
                    .size(11.0)
                    .color(plate::STRUCK),
            );
        });
        ui.add_space(2.0);
    }

    // Column tabs (active column = the one the pen draws into).
    ui.horizontal(|ui| {
        plate::legend(ui, "columns");
        let columns: Vec<(anim_core::ids::ColumnId, String)> = state
            .cut()
            .xsheet
            .columns
            .iter()
            .map(|c| (c.id, c.name.clone()))
            .collect();
        // AUDIT [4]: a mutually-exclusive pick is a DETENT — the Legend
        // ring + Tally disc — not a selectable whose text turns amber.
        for (id, name) in columns {
            if plate::detent(ui, state.view.active_column == id, &name).clicked() {
                state.view.active_column = id;
            }
        }
        if plate::icon_button(ui, Icon::Plus, "", "add column (layer)").clicked() {
            state.add_column();
        }
    });
    ui.horizontal(|ui| {
        ui.menu_button("key", |ui| {
            if ui
                .button("new drawing (E)")
                .on_hover_text("create + expose at the current frame")
                .clicked()
            {
                state.new_drawing_at_frame();
                ui.close();
            }
            if ui
                .button("expose selected")
                .on_hover_text("expose the selected library drawing here")
                .clicked()
            {
                state.expose_selected();
                ui.close();
            }
            // LIFT KEY (spec defect 7): you lift a key off a sheet and the
            // previous hold extends. Exposure, not pixels — and guarded.
            if plate::danger(ui, "LIFT KEY") {
                state.clear_key_at_frame();
                ui.close();
            }
        });
    });

    // (Scratch audio import/remove is EVICTED to the Slate's file detent —
    // the sheet still draws the waveform column when a clip exists.)
    param_key_strip(ui, state);

    // Drawing library — a DRAWER (spec §4.2): collapsed until reached for.
    ui.separator();
    egui::CollapsingHeader::new("library")
        .default_open(false)
        .show(ui, |ui| {
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
        // Defensive: if the drawing under rename vanished (undo past its
        // creation, removal from another pane), drop the buffer — otherwise
        // a redo reviving the SAME id would reopen a stale editor.
        if let Some((rid, _)) = &state.drawing_rename
        && !drawings.iter().any(|(id, _)| id == rid)
        {
        state.drawing_rename = None;
        }
        for (id, name) in drawings {
            // Double-click = rename (same idiom as the cel-layer strip);
            // the commit is one undoable RenameDrawing. Escape CANCELS
            // (egui surrenders focus on Escape, so bare lost_focus would
            // commit the aborted text).
            let renaming = matches!(&state.drawing_rename, Some((rid, _)) if *rid == id);
            if renaming {
            let (_, mut buf) = state.drawing_rename.take().expect("checked above");
            let resp = ui.add(
            egui::TextEdit::singleline(&mut buf).desired_width(90.0),
                );
                let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
                if resp.lost_focus() {
                let trimmed = buf.trim();
                if !escaped && !trimmed.is_empty() && trimmed != name {
                state.rename_drawing(id, trimmed);
                    }
                } else {
                resp.request_focus();
                state.drawing_rename = Some((id, buf));
                }
            } else {
            let resp = ui
                    .selectable_label(state.view.selected_drawing == Some(id), &name)
                    .on_hover_text(
                    "click: select — double-click: rename — right-click: remove",
                    );
                    resp.context_menu(|ui| {
                    // THE GHOST PIN (room ratified 2026-08-17): display
                    // only — Ao on the light box, every frame, no sheet
                    // entry, no export, dies with the session.
                    let pinned = state.view.ghost_pin == Some(id);
                    if ui
                        .button(if pinned { "unpin ghost" } else { "pin as ghost" })
                        .on_hover_text(
                        "ghost
                        this
                        drawing on the light box on every frame (Ao, onion strength) — display only, \
                        never exported, never on the sheet",
                        )
                        .clicked()
                    {
                    state.view.ghost_pin = if pinned { None } else { Some(id) };
                    ui.close();
                    }
                    // Held DANGER: removal lifts the drawing's keys and
                    // deletes it from the library, one undoable step.
                    if plate::danger(ui, "REMOVE DRAWING") {
                    state.remove_drawing_from_library(id);
                    ui.close();
                    }
                });
                if resp.double_clicked() {
                state.drawing_rename = Some((id, name));
                } else if resp.clicked() {
                state.view.selected_drawing = Some(id);
                }
            }
        }
    });
        });
    ui.separator();

    // ---- The sheet itself -------------------------------------------------
    let n_cols = state.cut().xsheet.columns.len();
    let n_params = state.cut().xsheet.params.len();
    let has_audio = state.cut().audio.is_some();
    let sheet_w = GUTTER_W
        + n_cols as f32 * COL_W
        + n_params as f32 * PARAM_COL_W
        + if has_audio { AUDIO_COL_W } else { 0.0 };

    // Header row. Param-column headers are clickable: selecting one opens
    // its keying strip above the sheet.
    let mut clicked_param: Option<anim_core::ids::ParamId> = None;
    ui.horizontal(|ui| {
        ui.allocate_exact_size(vec2(GUTTER_W, ROW_H), Sense::hover());
        for col in &state.cut().xsheet.columns {
            let (rect, _) = ui.allocate_exact_size(vec2(COL_W, ROW_H), Sense::hover());
            let is_active = col.id == state.view.active_column;
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                &col.name,
                egui::FontId::proportional(12.0),
                if is_active {
                    plate::STRUCK
                } else {
                    plate::LEGEND
                },
            );
        }
        for pcol in &state.cut().xsheet.params {
            let (rect, resp) = ui.allocate_exact_size(vec2(PARAM_COL_W, ROW_H), Sense::click());
            if resp.clicked() {
                clicked_param = Some(pcol.id);
            }
            let is_sel = state.param_sel == Some(pcol.id);
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                &pcol.name,
                egui::FontId::proportional(12.0),
                if is_sel { plate::STRUCK } else { plate::LEGEND },
            );
        }
        if has_audio {
            let (rect, _) = ui.allocate_exact_size(vec2(AUDIO_COL_W, ROW_H), Sense::hover());
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                "♪",
                egui::FontId::proportional(12.0),
                plate::LEGEND,
            );
        }
    });
    if let Some(pid) = clicked_param {
        // Toggle: clicking the selected header closes its strip.
        state.param_sel = if state.param_sel == Some(pid) {
            None
        } else {
            Some(pid)
        };
        state.param_buf_at = None;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Frame rows must abut: egui's default item_spacing.y would insert a
            // few px between each allocated row, so ROW_H would NOT be the true
            // row pitch and the continuation handles (which step by ROW_H) would
            // drift farther down the more frames there are. Zero it here so the
            // sheet is contiguous and ctop + frame*ROW_H lands exactly on a row.
            ui.spacing_mut().item_spacing.y = 0.0;
            // THE RULE paints in ONE pass after the rows (spec §5.2) — from
            // a whole-sheet painter, never from painter_at(row_rect), which
            // would clip every stroke to one row.
            let sheet_painter = ui.painter().clone();
            let ppp = ui.ctx().pixels_per_point();
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
                let is_current = frame == state.view.frame;
                // ONE current, amber ground; time boundaries are drawn
                // as RULES in the post-pass, not as tints on rows (a tint
                // sits a row off from sixty years of paper habit).
                if is_current {
                    painter.rect_filled(row_rect, 0, plate::tally_well());
                }

                // Frame number (1-based like paper sheets).
                painter.text(
                    pos2(row_rect.left() + GUTTER_W - 4.0, row_rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{}", frame + 1),
                    if is_current {
                        // Weight only — never size; the row must not reflow
                        // as the playhead passes (spec §5.3).
                        egui::FontId::new(11.0, plate::mono_semibold())
                    } else {
                        egui::FontId::monospace(11.0)
                    },
                    if is_current {
                        plate::STRUCK
                    } else {
                        plate::LEGEND
                    },
                );

                // (Drawing-column marks are painted by THE RULE's
                // post-pass below — identity and duration as one stroke,
                // not per-row text islands.)
                let cut = state.cut();

                // Audio waveform cell: this frame's peak amplitude as a
                // centred horizontal bar — the track reads as a vertical
                // waveform down the sheet. Subsampled (≤32 taps per row) so
                // a long clip never makes the sheet paint heavy.
                if let Some(clip) = &cut.audio {
                    let x0 = row_rect.left()
                        + GUTTER_W
                        + n_cols as f32 * COL_W
                        + cut.xsheet.params.len() as f32 * PARAM_COL_W;
                    let cell =
                        Rect::from_min_size(pos2(x0, row_rect.top()), vec2(AUDIO_COL_W, ROW_H));
                    // Same integer math as playback's start offset (whole
                    // sample-frames, then interleave) so the drawn row and
                    // the sound never drift apart over a long clip.
                    let ch = clip.channels as u64;
                    let sr = clip.sample_rate as u64;
                    let f = fps.max(1) as u64;
                    let a =
                        (((frame as u64 * sr) / f) * ch).min(clip.samples.len() as u64) as usize;
                    let b = ((((frame as u64 + 1) * sr) / f) * ch).min(clip.samples.len() as u64)
                        as usize;
                    if a < b {
                        let stride = ((b - a) / 32).max(1);
                        let mut peak = 0.0f32;
                        let mut i = a;
                        while i < b {
                            peak = peak.max(clip.samples[i].abs());
                            i += stride;
                        }
                        let half = peak.min(1.0) * (AUDIO_COL_W - 8.0) * 0.5;
                        let cy = cell.center();
                        painter.line_segment(
                            [pos2(cy.x - half, cy.y), pos2(cy.x + half, cy.y)],
                            egui::Stroke::new(
                                (ROW_H - 4.0).max(1.0),
                                Color32::from_rgba_unmultiplied(139, 141, 131, 90),
                            ),
                        );
                    }
                }

                // Parameter cells: ◆ value on keys (gold, like drawing
                // keys), the interpolated value dimmed on in-between frames.
                let params_x0 = row_rect.left() + GUTTER_W + n_cols as f32 * COL_W;
                for (pi, pcol) in cut.xsheet.params.iter().enumerate() {
                    let x = params_x0 + pi as f32 * PARAM_COL_W;
                    let cell =
                        Rect::from_min_size(pos2(x, row_rect.top()), vec2(PARAM_COL_W, ROW_H));
                    let keyed = pcol.key_at(frame).is_some();
                    if keyed {
                        painter.circle_filled(
                            pos2(cell.left() + 5.0, cell.center().y),
                            2.5,
                            plate::TALLY,
                        );
                    }
                    let (text, color) = match pcol.key_at(frame) {
                        Some(k) => (
                            format!("{:.5}", k.value)
                                .trim_end_matches('0')
                                .trim_end_matches('.')
                                .to_string(),
                            plate::STRUCK,
                        ),
                        None => match pcol.resolve(frame) {
                            Some(v) => (format!("{v:.1}"), plate::legend_dim()),
                            None => (String::new(), Color32::TRANSPARENT),
                        },
                    };
                    if !text.is_empty() {
                        painter.text(
                            pos2(cell.left() + 10.0, cell.center().y),
                            egui::Align2::LEFT_CENTER,
                            text,
                            egui::FontId::monospace(11.0),
                            color,
                        );
                    }
                }
            }

            // THE GUTTER SCRUB LANE (henshu delta 9): dragging in the
            // 44pt gutter scrubs the playhead — a pen-sized time lane on
            // the same view-state write the row click performs.
            if let Some((ctop, cleft)) = geom {
                let lane = Rect::from_min_max(
                    pos2(cleft, ctop),
                    pos2(cleft + GUTTER_W, ctop + frame_count as f32 * ROW_H),
                );
                let lresp = ui.interact(lane, ui.id().with("gutter_scrub"), Sense::drag());
                if lresp.dragged()
                    && let Some(hp) = lresp.interact_pointer_pos()
                {
                    let f =
                        (((hp.y - ctop) / ROW_H) as i64).clamp(0, frame_count as i64 - 1) as u32;
                    if f != state.view.frame {
                        state.goto(f);
                    }
                }
            }

            // ---- THE RULE (spec §5.2–5.3): rules of time, then one
            // continuous mark per run — key dot, Ao hold stroke, tail
            // cross-tick, name at the head only. Hover turns a run Tally.
            if let Some((ctop, cleft)) = geom {
                let cut = state.cut();
                let right = cleft + sheet_w;
                for f in 0..=frame_count {
                    if let Some(tier) = crate::runs::rule_tier(f, fps) {
                        let y = plate::snap(ctop + f as f32 * ROW_H, ppp);
                        let (w, ink) = match tier {
                            2 => (plate::device_px(2.0, ppp), plate::rule_sec()),
                            1 => (plate::device_px(1.0, ppp), plate::rule_half()),
                            _ => (plate::device_px(1.0, ppp), plate::rule_beat()),
                        };
                        sheet_painter.line_segment(
                            [pos2(cleft, y), pos2(right, y)],
                            egui::Stroke::new(w, ink),
                        );
                        if tier == 2 && f > 0 && f < frame_count {
                            sheet_painter.text(
                                pos2(cleft + 2.0, y + ROW_H * 0.5),
                                egui::Align2::LEFT_CENTER,
                                format!("{}s", f / fps.max(1)),
                                egui::FontId::monospace(11.0),
                                plate::LEGEND,
                            );
                        }
                    }
                }
                // Vertical separators mean SPACE; horizontal rules mean time.
                for ci in 0..=n_cols {
                    let x = plate::snap(cleft + GUTTER_W + ci as f32 * COL_W, ppp);
                    sheet_painter.line_segment(
                        [pos2(x, ctop), pos2(x, ctop + frame_count as f32 * ROW_H)],
                        egui::Stroke::new(plate::device_px(1.0, ppp), plate::rule_beat()),
                    );
                }
                // The current frame's gutter notch.
                let ncy = ctop + (state.view.frame as f32 + 0.5) * ROW_H;
                sheet_painter.add(egui::Shape::convex_polygon(
                    vec![
                        pos2(cleft, ncy - 4.0),
                        pos2(cleft + 5.0, ncy),
                        pos2(cleft, ncy + 4.0),
                    ],
                    plate::TALLY,
                    egui::Stroke::NONE,
                ));
                let hover_pos = ui.input(|i| i.pointer.hover_pos());
                for (ci, col) in cut.xsheet.columns.iter().enumerate() {
                    let marks = crate::runs::column_marks(frame_count, |f| col.key_at(f));
                    let cx = plate::snap(cleft + GUTTER_W + ci as f32 * COL_W + 22.0, ppp);
                    for r in &marks.runs {
                        let y_dot = plate::snap(ctop + r.start as f32 * ROW_H + 9.0, ppp);
                        let y_end = plate::snap(ctop + (r.end + 1) as f32 * ROW_H, ppp) - 1.0;
                        let hot = hover_pos.is_some_and(|hp| {
                            hp.x >= cx - 22.0
                                && hp.x <= cx - 22.0 + COL_W
                                && hp.y >= y_dot - 9.0
                                && hp.y <= y_end
                        });
                        let rule_ink = if hot { plate::TALLY } else { plate::AO };
                        sheet_painter.circle_filled(pos2(cx, y_dot), 3.5, plate::TALLY);
                        if y_end > y_dot + 3.5 {
                            // Tape-measure law (owner 2026-08-17, refined):
                            // a hold running to the sheet's END is RETRACTED
                            // — a short stub fading out, tape in the reel.
                            // Only a pulled-out hold draws its full length,
                            // ending in the tick that is its drag target.
                            let open_ended = r.end + 1 >= frame_count;
                            if open_ended {
                                // FULLY RETRACTED (owner, final form): no
                                // stub at all — the key dot wears a small
                                // downward head, "continues until pulled".
                                // Nothing extends into neighbouring rows,
                                // so nothing can ever collide.
                                sheet_painter.add(egui::Shape::convex_polygon(
                                    vec![
                                        pos2(cx - 4.0, y_dot + 5.5),
                                        pos2(cx + 4.0, y_dot + 5.5),
                                        pos2(cx, y_dot + 10.5),
                                    ],
                                    rule_ink.gamma_multiply(0.85),
                                    egui::Stroke::NONE,
                                ));
                            } else {
                                sheet_painter.line_segment(
                                    [pos2(cx, y_dot + 3.5), pos2(cx, y_end)],
                                    egui::Stroke::new(plate::device_px(2.0, ppp), rule_ink),
                                );
                                sheet_painter.line_segment(
                                    [pos2(cx - 4.5, y_end), pos2(cx + 4.5, y_end)],
                                    egui::Stroke::new(plate::device_px(2.0, ppp), rule_ink),
                                );
                            }
                        }
                        let name = cut
                            .drawing(r.drawing)
                            .map(|d| d.name.clone())
                            .unwrap_or_else(|| format!("{}", r.drawing));
                        sheet_painter.text(
                            pos2(cx + 10.0, ctop + r.start as f32 * ROW_H + ROW_H * 0.5),
                            egui::Align2::LEFT_CENTER,
                            name,
                            egui::FontId::monospace(11.0),
                            plate::STRUCK,
                        );
                    }
                    for f in &marks.empties {
                        let y = plate::snap(ctop + *f as f32 * ROW_H + 9.0, ppp);
                        sheet_painter.circle_stroke(
                            pos2(cx, y),
                            3.5,
                            egui::Stroke::new(1.0, plate::LEGEND),
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

/// The parameter keying strip (C1 camera model): shown when a param-column
/// header is selected. Workflow: click a frame row, type the value, "key" —
/// interpolation (hold/linear/ease) applies from that key to the next.
fn param_key_strip(ui: &mut egui::Ui, state: &mut AppState) {
    use anim_core::xsheet::{ParamInterp, ParamKey};
    let Some(pid) = state.param_sel else { return };
    let Some(pcol) = state.cut().xsheet.param(pid) else {
        state.param_sel = None;
        return;
    };
    let name = pcol.name.clone();
    let frame = state.view.frame;
    let existing = pcol.key_at(frame);
    let resolved = pcol.resolve(frame);

    // Reseed the value buffer whenever the (column, frame) target moves:
    // an existing key's value, else the interpolated value here, else 0.
    if state.param_buf_at != Some((pid, frame)) {
        state.param_buf = existing.map(|k| k.value).or(resolved).unwrap_or(0.0);
        if let Some(k) = existing {
            state.param_interp = k.interp;
        }
        state.param_buf_at = Some((pid, frame));
    }

    // During playback the playhead (and with it this strip's target frame)
    // moves every UI frame — an edit would land on whatever frame the
    // playhead happened to reach. Disabled, not hidden (UI-STABILITY law).
    let editable = !state.view.playing;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{name} @ {}", frame + 1)).color(plate::LEGEND));
        ui.add_enabled(
            editable,
            egui::DragValue::new(&mut state.param_buf).speed(0.5),
        );
        for (label, interp, hint) in [
            ("H", ParamInterp::Hold, "hold: step to the next key"),
            (
                "L",
                ParamInterp::Linear,
                "linear: straight line to the next key",
            ),
            (
                "E",
                ParamInterp::Ease,
                "ease: gentle start and stop (camera slide)",
            ),
        ] {
            if ui
                .add_enabled(
                    editable,
                    egui::Button::selectable(state.param_interp == interp, label),
                )
                .on_hover_text(hint)
                .clicked()
            {
                state.param_interp = interp;
            }
        }
        let key_label = if existing.is_some() { "set key" } else { "key" };
        if ui
            .add_enabled(editable, egui::Button::new(key_label))
            .on_hover_text("one undo step")
            .clicked()
        {
            let key = ParamKey {
                value: state.param_buf,
                interp: state.param_interp,
            };
            state.set_param_key(pid, Some(key));
        }
        if ui
            .add_enabled(editable && existing.is_some(), egui::Button::new("clear"))
            .on_hover_text("remove this key")
            .clicked()
        {
            state.set_param_key(pid, None);
            state.param_buf_at = None;
        }
    });
}

/// One drawing's exposure span and its optional early-end terminator.
struct Cont {
    column: anim_core::ids::ColumnId,
    ci: usize,
    n: u32,                  // the drawing key's frame
    max_end: u32,            // next drawing frame, or frame_count (the natural end)
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
                conts.push(Cont {
                    column: col.id,
                    ci,
                    n,
                    max_end,
                    terminator,
                });
            }
        }
    }
    let painter = ui.painter();
    let hover = ui.input(|i| i.pointer.hover_pos());
    let mut pending: Vec<(anim_core::ids::ColumnId, Option<u32>, Option<u32>)> = Vec::new();
    let mut mark_positioned: Vec<(anim_core::ids::ColumnId, u32)> = Vec::new();
    let mut unposition: Vec<(anim_core::ids::ColumnId, u32)> = Vec::new();
    for c in &conts {
        let col_x = cleft + GUTTER_W + c.ci as f32 * COL_W;
        // The handle rides ON the run's rule (spec §5.2 absorbed it): same
        // x as the painted hold stroke; the circle is the drag target.
        let x = col_x + 22.0;
        // Exposure top = just below the drawing's own key cell.
        let start_y = ctop + (c.n as f32 + 1.0) * ROW_H;
        let max_y = ctop + c.max_end as f32 * ROW_H;

        // A hold is "positioned" once the user has actually pulled the handle
        // (it has an Empty terminator, or was dragged to its natural end and
        // recorded). Positioned = the exposure line is drawn down the sheet.
        // Un-positioned = STOWED: the handle rests at the drawing's frame and no
        // line is drawn; the drawing still holds normally down the sheet.
        let positioned =
            c.terminator.is_some() || state.positioned_holds.contains(&(c.column, c.n));
        let rest_y = if positioned {
            ctop + c.terminator.unwrap_or(c.max_end) as f32 * ROW_H
        } else {
            // Stowed: centred on the drawing's own cell, no line drawn.
            ctop + (c.n as f32 + 0.5) * ROW_H
        };

        // Interact first (so the handle wins over the row click).
        let id = ui.id().with(("cont", c.column.0, c.n));
        let hr = ui
            .interact(
                Rect::from_center_size(pos2(x, rest_y), vec2(22.0, 22.0)),
                id,
                Sense::click_and_drag(),
            )
            .on_hover_text(
                "hold handle — drag out: the hold ends HERE · right-click: stow (retracts like
            a
            tape measure — the
            hold runs on to the next key or the sheet's end)",
            );

        // While dragging, the handle (and line) follow the pointer live, clamped
        // to the valid span; otherwise it sits at its resting row.
        let handle_y = if hr.dragged() {
            hr.interact_pointer_pos()
                .map(|p| p.y)
                .unwrap_or(rest_y)
                .clamp(start_y, max_y)
        } else {
            rest_y
        };
        let handle_center = pos2(x, handle_y);

        // Reveal: positioned, dragging, or hovering near the (stowed or set)
        // handle — a small grab zone, so a fresh drawing shows nothing until you
        // reach for it at the frame.
        let near = hover.is_some_and(|p| {
            p.x >= col_x && p.x <= col_x + COL_W && p.y >= rest_y - ROW_H && p.y <= rest_y + ROW_H
        });
        let show = positioned || hr.dragged() || near;
        if show {
            let active = hr.dragged() || hr.hovered() || near;
            let col = if active { plate::TALLY } else { plate::AO };
            // The rule's post-pass already paints the hold stroke —
            // the handle contributes only its grab circle. While dragging,
            // a live guide shows where the terminator will land.
            if hr.dragged() && handle_y > start_y + 0.5 {
                painter.line_segment(
                    [pos2(x, start_y), pos2(x, handle_y)],
                    egui::Stroke::new(1.5, col),
                );
            }
            painter.circle_stroke(handle_center, 4.0, egui::Stroke::new(1.5, col));
        }

        // Right-click resets the hold whole: terminator off (it runs to
        // the next key / sheet end again) and the handle re-stows.
        if hr.secondary_clicked() {
            if c.terminator.is_some() {
                pending.push((c.column, c.terminator, None));
            }
            unposition.push((c.column, c.n));
        }
        if hr.dragged() {
            // Any deliberate drag counts as positioning it, so it stays visible.
            mark_positioned.push((c.column, c.n));
            if let Some(p) = hr.interact_pointer_pos() {
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
    }
    for key in mark_positioned {
        state.positioned_holds.insert(key);
    }
    for key in unposition {
        state.positioned_holds.remove(&key);
    }
    for (column, old, new) in pending {
        state.set_hold_terminator(column, old, new);
    }
}

/// Cap a layer name for strip display (full name stays in the model/rename).
fn truncate_name(name: &str) -> String {
    const MAX: usize = 16;
    if name.chars().count() > MAX {
        let mut s: String = name.chars().take(MAX - 1).collect();
        s.push('…');
        s
    } else {
        name.to_string()
    }
}

// ---- Cel Layers strip -------------------------------------------------------
// The layers INSIDE this frame's own cel, top-first (Krita/CSP convention).
// Row: [eye] [colour dot] [name] [↑][↓][–] [opacity]. Click = activate,
// double-click = rename, opacity drag commits ONCE at gesture end. The + menu
// inserts presets at their anime-correct stack positions.

pub fn cel_layers_strip(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool) {
    // Same width-isolation as the parent panel: the strip's rows must never
    // drive the panel width (they appear/disappear on undo/redo and playback).
    ui.set_min_width(ui.available_width());
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        plate::legend(ui, "cel layers");
        let have_cel = state.own_key_drawing().is_some();
        let at_cap = state.strip_layers().len() >= 8;
        ui.menu_button("+", |ui| {
            if at_cap {
                ui.label(egui::RichText::new("layer cap (8) reached").weak().small());
                return;
            }
            if !have_cel {
                ui.label(
                    egui::RichText::new("no cel on this frame (draw or press E)")
                        .weak()
                        .small(),
                );
                return;
            }
            for preset in ["rough", "shadow", "highlight", "correction", "empty"] {
                let text = egui::RichText::new(preset).color(layer_chip_color(preset));
                if ui.button(text).clicked() {
                    state.layer_add_preset(preset);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("add a layer at its pipeline position");
    });

    // Owned snapshot so row actions can mutate state freely.
    let rows: Vec<(LayerId, String, bool, f32)> = state
        .strip_layers()
        .iter()
        .map(|(_, l)| (l.id, l.props.name.clone(), l.props.visible, l.props.opacity))
        .collect();

    // Defensive: drop a rename buffer whose layer is gone (undo/redo can
    // revive the same LayerId later and would reopen a stale editor).
    if let Some((rid, _)) = &state.strip_rename
        && !rows.iter().any(|(id, ..)| id == rid)
    {
        state.strip_rename = None;
    }
    if rows.is_empty() {
        ui.label(
            egui::RichText::new("held/empty frame — a new cel gets color + line")
                .weak()
                .small(),
        );
        ui.add_space(2.0);
        return;
    }
    for (lid, name, visible, opacity) in rows.iter() {
        let is_active = state.view.active_layer == *name;
        ui.horizontal(|ui| {
            // Eye: visibility toggle (one click = one undo step). Hidden is a
            // configuration, so the struck eye dims — it does not go red.
            let eye_icon = if *visible { Icon::Eye } else { Icon::EyeOff };
            if plate::icon_button(ui, eye_icon, "", "show / hide").clicked() {
                state.layer_set_props(
                    *lid,
                    LayerProps {
                        name: name.clone(),
                        visible: !visible,
                        opacity: *opacity,
                    },
                );
            }
            // RETAS colour dot — painted, in the layer's identity ink.
            let (dot_rect, _) = ui.allocate_exact_size(vec2(12.0, 18.0), Sense::hover());
            ui.painter()
                .circle_filled(dot_rect.center(), 4.0, layer_chip_color(name));

            // Name: click = activate, double-click = rename — in a FIXED
            // cell (spec defect 10): the columns after it never drift with
            // name length; the text elides inside.
            // Reserve what the row's fixed furniture actually needs —
            // eye 27 + dot 12 + two chevrons 54 + slider 60 + "100%" 34 +
            // spacing — or the percentage clips off the panel's edge.
            let name_w = (ui.available_width() - 205.0).max(48.0);
            ui.allocate_ui_with_layout(
                vec2(name_w, 20.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(name_w);
                    ui.set_max_width(name_w);
                    let renaming = matches!(&state.strip_rename, Some((rid, _)) if rid == lid);
                    if renaming {
                        let (_, mut buf) = state.strip_rename.take().expect("checked above");
                        let resp = ui.text_edit_singleline(&mut buf);
                        // Escape cancels (same fix as the drawing-library rename —
                        // bare lost_focus committed the aborted text).
                        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        let done = resp.lost_focus();
                        if done {
                            let trimmed = buf.trim();
                            if !escaped && !trimmed.is_empty() && trimmed != name {
                                state.layer_set_props(
                                    *lid,
                                    LayerProps {
                                        name: trimmed.to_string(),
                                        visible: *visible,
                                        opacity: *opacity,
                                    },
                                );
                            }
                        } else {
                            resp.request_focus();
                            state.strip_rename = Some((*lid, buf));
                        }
                    } else {
                        // LINE GUARD (shiage): the guarded line row wears the
                        // shackle; tapping it refuses instead of arming.
                        if state.view.line_guard && name == "line" {
                            let (sr, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
                            crate::icons::paint(
                                ui.painter(),
                                Rect::from_center_size(sr.center(), vec2(12.0, 12.0)),
                                plate::legend_dim(),
                                Icon::Shackle,
                            );
                        }
                        // Truncated for display so a long name can't widen the row
                        // (and with it the panel); renaming edits the full name.
                        let shown = truncate_name(name);
                        let text = if is_active {
                            egui::RichText::new(shown).strong().color(plate::STRUCK)
                        } else {
                            egui::RichText::new(shown).color(plate::LEGEND)
                        };
                        let resp = ui
                            .selectable_label(is_active, text)
                            .on_hover_text("click: edit this layer — double-click: rename");
                        if resp.double_clicked() {
                            state.strip_rename = Some((*lid, name.clone()));
                        } else if resp.clicked() {
                            if state.view.line_guard && name == "line" {
                                state.refuse(
                                    "refused — line guard is on (unlatch it to arm the line layer)",
                                );
                            } else {
                                state.view.active_layer = name.clone();
                            }
                        }
                    }
                },
            );

            // Reorder: painted chevrons at fixed columns. Delete has LEFT
            // the row (spec §6 defect 11) — Fitts's law stops pointing at
            // the irreversible control; it lives in the footer as DANGER.
            if plate::icon_button(ui, Icon::Up, "", "move up").clicked() {
                state.layer_move(*lid, true);
            }
            if plate::icon_button(ui, Icon::Down, "", "move down").clicked() {
                state.layer_move(*lid, false);
            }

            // Opacity: live value during the drag, committed ONCE at the end
            // (one gesture = one undo step).
            let mut live = match &state.strip_opacity {
                Some((oid, v)) if oid == lid => *v,
                _ => *opacity,
            };
            // AUDIT [3]: this was an egui Slider whose track is invisible
            // under our Visuals — a bare handle on the plate with no 0 or
            // full. The rail shows its range; the readout is Mono 11 Struck
            // (the old .monospace().small() pair resolved to 9.5 PROP).
            let resp = plate::rail(ui, &mut live, 0.0..=1.0, 60.0);
            ui.label(
                egui::RichText::new(format!("{:3.0}%", live * 100.0))
                    .monospace()
                    .size(11.0)
                    .color(plate::STRUCK),
            );
            if resp.drag_stopped() {
                state.strip_opacity = None;
                if (live - *opacity).abs() > 0.001 {
                    state.layer_set_props(
                        *lid,
                        LayerProps {
                            name: name.clone(),
                            visible: *visible,
                            opacity: live,
                        },
                    );
                }
            } else if resp.dragged() {
                state.strip_opacity = Some((*lid, live));
            } else if resp.changed() {
                // Click-to-set (no drag): apply immediately.
                state.layer_set_props(
                    *lid,
                    LayerProps {
                        name: name.clone(),
                        visible: *visible,
                        opacity: live,
                    },
                );
            }
        });
    }

    // ---- The destructive commands: apart from the rows, Aka-edged, and
    // fired only by a held press (spec §6 defects 6/7/11). Both act on the
    // gesture-safe guard the keybinds use — never under a live stroke.
    ui.add_space(4.0);
    ui.separator();
    ui.horizontal(|ui| {
        // LINE GUARD: arming-only protection for the line layer (shiage;
        // defaults ON in the Finishing room). A guard, not a lock.
        let mut lg = state.view.line_guard;
        plate::latch(ui, &mut lg, "line guard").on_hover_text(
        "while on, the line layer cannot be ARMED as the ink target — cycling skips it, tapping refuses",
        );
        state.view.line_guard = lg;
        let
        active_lid =
        rows
            .iter()
            .find(|(_, nm, ..)| *nm == state.view.active_layer)
            .map(|(id, ..)| *id);
            if plate::danger(ui, "ERASE CEL") && gesture_safe {
            // Pixels, not exposure: clears this cel's raster (undoable).
            state.clear_current_raster();
        }
        if plate::danger(ui, "DELETE LAYER") && gesture_safe {
        match active_lid {
        Some(lid) => state.layer_remove(lid),
        None => state.refuse(format!(
        "refused — no '{}' layer on this cel to delete",
        state.view.active_layer
                )),
            }
        }
    });
    ui.add_space(2.0);
}
