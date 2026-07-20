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

const ROW_H: f32 = 18.0;
const FRAME_NUM_W: f32 = 34.0;
const COL_W: f32 = 86.0;
/// Parameter (camera) columns are numeric — narrower than drawing columns.
const PARAM_COL_W: f32 = 58.0;
/// The audio waveform column (frames are rows, so the waveform runs DOWN
/// the sheet — the paper timesheet's audio track).
const AUDIO_COL_W: f32 = 46.0;

pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    // Claim the full panel width EVERY frame. egui 0.35 panels are
    // content-sized (the stored panel rect is the content rect, re-measured
    // per frame), so variable-width rows — the layer strip swapping between
    // full rows and the held-frame note during playback, changing text —
    // would otherwise wobble the panel width and nudge the canvas
    // horizontally on every frame of playback.
    ui.set_min_width(ui.available_width());

    // (The cel-layers strip is its own dockable pane now — see workspace::Pane.)
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
                .selectable_label(state.view.active_column == id, name)
                .clicked()
            {
                state.view.active_column = id;
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

    // Scratch audio (C4): its OWN row — the clip name is variable-width and
    // the toolbar row above must stay fixed (UI-STABILITY law). The name is
    // truncated so even this row can't outgrow the panel.
    ui.horizontal(|ui| {
        match state.cut().audio.as_ref().map(|a| (a.name.clone(), a.seconds())) {
            Some((name, secs)) => {
                ui.label(
                    egui::RichText::new(format!("♪ {} ({secs:.1}s)", truncate_name(&name)))
                        .color(Color32::from_rgb(120, 210, 190)),
                );
                if ui.small_button("✕").on_hover_text("remove the audio track").clicked() {
                    state.remove_audio();
                }
            }
            None => {
                if ui
                    .button("♪ wav…")
                    .on_hover_text("import a WAV as this cut's scratch track")
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("WAV audio", &["wav"])
                        .pick_file()
                {
                    state.import_audio(&path);
                }
            }
        }
    });

    param_key_strip(ui, state);

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
                .selectable_label(state.view.selected_drawing == Some(id), name)
                .clicked()
            {
                state.view.selected_drawing = Some(id);
            }
        }
    });
    ui.separator();

    // ---- The sheet itself -------------------------------------------------
    let n_cols = state.cut().xsheet.columns.len();
    let n_params = state.cut().xsheet.params.len();
    let has_audio = state.cut().audio.is_some();
    let sheet_w = FRAME_NUM_W
        + n_cols as f32 * COL_W
        + n_params as f32 * PARAM_COL_W
        + if has_audio { AUDIO_COL_W } else { 0.0 };

    // Header row. Param-column headers are clickable: selecting one opens
    // its keying strip above the sheet.
    let mut clicked_param: Option<anim_core::ids::ParamId> = None;
    ui.horizontal(|ui| {
        ui.allocate_exact_size(vec2(FRAME_NUM_W, ROW_H), Sense::hover());
        for col in &state.cut().xsheet.columns {
            let (rect, _) = ui.allocate_exact_size(vec2(COL_W, ROW_H), Sense::hover());
            let is_active = col.id == state.view.active_column;
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
                if is_sel {
                    Color32::from_rgb(190, 160, 255)
                } else {
                    Color32::from_gray(160)
                },
            );
        }
        if has_audio {
            let (rect, _) = ui.allocate_exact_size(vec2(AUDIO_COL_W, ROW_H), Sense::hover());
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                "♪",
                egui::FontId::proportional(12.0),
                Color32::from_rgb(120, 210, 190),
            );
        }
    });
    if let Some(pid) = clicked_param {
        // Toggle: clicking the selected header closes its strip.
        state.param_sel = if state.param_sel == Some(pid) { None } else { Some(pid) };
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

                // Audio waveform cell: this frame's peak amplitude as a
                // centred horizontal bar — the track reads as a vertical
                // waveform down the sheet. Subsampled (≤32 taps per row) so
                // a long clip never makes the sheet paint heavy.
                if let Some(clip) = &cut.audio {
                    let x0 = row_rect.left()
                        + FRAME_NUM_W
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
                    let a = (((frame as u64 * sr) / f) * ch).min(clip.samples.len() as u64) as usize;
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
                                Color32::from_rgba_unmultiplied(120, 210, 190, 90),
                            ),
                        );
                    }
                }

                // Parameter cells: ◆ value on keys (gold, like drawing
                // keys), the interpolated value dimmed on in-between frames.
                let params_x0 = row_rect.left() + FRAME_NUM_W + n_cols as f32 * COL_W;
                for (pi, pcol) in cut.xsheet.params.iter().enumerate() {
                    let x = params_x0 + pi as f32 * PARAM_COL_W;
                    let cell =
                        Rect::from_min_size(pos2(x, row_rect.top()), vec2(PARAM_COL_W, ROW_H));
                    let (text, color) = match pcol.key_at(frame) {
                        Some(k) => (
                            format!("◆ {:.5}", k.value)
                                .trim_end_matches('0')
                                .trim_end_matches('.')
                                .to_string(),
                            Color32::from_rgb(230, 210, 120),
                        ),
                        None => match pcol.resolve(frame) {
                            Some(v) => (format!("{v:.1}"), Color32::from_gray(95)),
                            None => (String::new(), Color32::TRANSPARENT),
                        },
                    };
                    if !text.is_empty() {
                        painter.text(
                            pos2(cell.left() + 4.0, cell.center().y),
                            egui::Align2::LEFT_CENTER,
                            text,
                            egui::FontId::monospace(10.0),
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
        ui.label(
            egui::RichText::new(format!("{name} @ {}", frame + 1))
                .color(Color32::from_rgb(190, 160, 255)),
        );
        ui.add_enabled(editable, egui::DragValue::new(&mut state.param_buf).speed(0.5));
        for (label, interp, hint) in [
            ("H", ParamInterp::Hold, "hold: step to the next key"),
            ("L", ParamInterp::Linear, "linear: straight line to the next key"),
            ("E", ParamInterp::Ease, "ease: gentle start and stop (camera slide)"),
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
    let mut mark_positioned: Vec<(anim_core::ids::ColumnId, u32)> = Vec::new();

    for c in &conts {
        let col_x = cleft + FRAME_NUM_W + c.ci as f32 * COL_W;
        let x = col_x + COL_W - 10.0;
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
        let hr = ui.interact(
            Rect::from_center_size(pos2(x, rest_y), vec2(16.0, 16.0)),
            id,
            Sense::drag(),
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
            p.x >= col_x
                && p.x <= col_x + COL_W
                && p.y >= rest_y - ROW_H
                && p.y <= rest_y + ROW_H
        });
        let show = positioned || hr.dragged() || near;

        if show {
            let active = hr.dragged() || hr.hovered() || near;
            let col = if active {
                Color32::WHITE
            } else {
                Color32::from_rgb(230, 160, 90) // positioned, resting
            };
            // Draw the line only when the handle is below the frame start (i.e.
            // it has been pulled down); a stowed handle is just the circle.
            if handle_y > start_y + 0.5 {
                painter.line_segment(
                    [pos2(x, start_y), pos2(x, handle_y)],
                    egui::Stroke::new(1.5, col),
                );
            }
            painter.circle_stroke(handle_center, 4.0, egui::Stroke::new(1.5, col));
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

pub fn cel_layers_strip(ui: &mut egui::Ui, state: &mut AppState) {
    // Same width-isolation as the parent panel: the strip's rows must never
    // drive the panel width (they appear/disappear on undo/redo and playback).
    ui.set_min_width(ui.available_width());
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("CEL LAYERS")
                .strong()
                .small()
                .color(Color32::from_rgb(120, 190, 255)),
        );
        let have_cel = state.own_key_drawing().is_some();
        let at_cap = state.strip_layers().len() >= 8;
        ui.menu_button("＋", |ui| {
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

    if rows.is_empty() {
        ui.label(
            egui::RichText::new("held/empty frame — a new cel gets color + line")
                .weak()
                .small(),
        );
        ui.add_space(2.0);
        return;
    }

    let n = rows.len();
    for (slot, (lid, name, visible, opacity)) in rows.iter().enumerate() {
        let is_active = state.view.active_layer_slot.min(n - 1) == slot;
        ui.horizontal(|ui| {
            // Eye: visibility toggle (one click = one undo step).
            let eye = if *visible { "👁" } else { "—" };
            if ui
                .selectable_label(*visible, eye)
                .on_hover_text("show / hide")
                .clicked()
            {
                state.layer_set_props(
                    *lid,
                    LayerProps {
                        name: name.clone(),
                        visible: !visible,
                        opacity: *opacity,
                    },
                );
            }
            // RETAS colour dot.
            ui.label(egui::RichText::new("●").color(layer_chip_color(name)));

            // Name: click = activate, double-click = rename.
            let renaming = matches!(&state.strip_rename, Some((rid, _)) if rid == lid);
            if renaming {
                let (_, mut buf) = state.strip_rename.take().expect("checked above");
                let resp = ui.text_edit_singleline(&mut buf);
                let done = resp.lost_focus();
                if done {
                    let trimmed = buf.trim();
                    if !trimmed.is_empty() && trimmed != name {
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
                // Truncated for display so a long name can't widen the row
                // (and with it the panel); renaming edits the full name.
                let shown = truncate_name(name);
                let text = if is_active {
                    egui::RichText::new(shown).strong().color(Color32::from_rgb(120, 190, 255))
                } else {
                    egui::RichText::new(shown).color(Color32::from_gray(200))
                };
                let resp = ui
                    .selectable_label(is_active, text)
                    .on_hover_text("click: edit this layer — double-click: rename");
                if resp.double_clicked() {
                    state.strip_rename = Some((*lid, name.clone()));
                } else if resp.clicked() {
                    state.view.active_layer_slot = slot;
                }
            }

            // Reorder / remove.
            if ui.small_button("↑").on_hover_text("move up").clicked() {
                state.layer_move(*lid, true);
            }
            if ui.small_button("↓").on_hover_text("move down").clicked() {
                state.layer_move(*lid, false);
            }
            if ui.small_button("–").on_hover_text("remove layer").clicked() {
                state.layer_remove(*lid);
            }

            // Opacity: live value during the drag, committed ONCE at the end
            // (one gesture = one undo step).
            let mut live = match &state.strip_opacity {
                Some((oid, v)) if oid == lid => *v,
                _ => *opacity,
            };
            ui.spacing_mut().slider_width = 60.0;
            let resp = ui.add(
                egui::Slider::new(&mut live, 0.0..=1.0)
                    .show_value(false)
                    .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 }),
            );
            ui.label(
                egui::RichText::new(format!("{:3.0}%", live * 100.0))
                    .monospace()
                    .small(),
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
    ui.add_space(2.0);
}
