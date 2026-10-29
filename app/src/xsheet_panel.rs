//! The exposure sheet panel — frames as rows (the paper timesheet layout Toei
//! kept in its own digital exposure sheet), columns as layers. Keys show as
//! ● + name; held frames show as │. Click any row to jump the playhead.

use anim_core::ids::{ColumnId, LayerId};
use anim_core::model::LayerProps;
use anim_core::xsheet::Exposure;
use eframe::egui;
use egui::{Color32, Rect, Sense, pos2, vec2};

use crate::canvas::{props_chip_color, role_chip_color};
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
/// GM14: the memo/dialogue column (serifu and action notes per frame).
const MEMO_COL_W: f32 = 96.0;

/// GM14: the typed entry on the cursor's cell, and the toolbar fields.
const ENTRY_ID: &str = "gaban_sheet_cell_entry";
const LENGTH_ID: &str = "gaban_sheet_cut_length";
const MEMO_ID: &str = "gaban_sheet_memo";

/// GM14: the sheet panel's own state — view furniture, never in the file.
#[derive(Default)]
pub struct SheetUi {
    /// The range's fixed corner (column, frame), and where the cursor was
    /// put with it: the range lives while the cursor stays on `head`.
    pub anchor: Option<(ColumnId, u32)>,
    pub head: Option<(ColumnId, u32)>,
    /// What Copy / Cut took (the sheet's own clipboard).
    pub clipboard: Option<anim_core::sheet_ops::Block>,
    /// The typed-entry field on the cursor's cell.
    pub entry: String,
    /// The cut-length field, and the length it was seeded from.
    pub length: String,
    length_at: Option<u32>,
    /// The memo field, and the (cut, frame) it was seeded from.
    pub memo: String,
    memo_at: Option<(anim_core::ids::CutId, u32)>,
    /// The rows laid out last frame: the sheet lays out only these.
    pub rows_shown: std::ops::Range<usize>,
    /// The top-left of frame 0's row on screen last frame (tests aim at
    /// cells with it).
    pub origin: Option<egui::Pos2>,
    /// The clipboard block's text as handed to the system clipboard (None
    /// = not handed over yet): a paste carrying other text is not cells.
    pub clip_text: Option<String>,
    /// The Type-into-cell verb asked for the entry (taken next frame).
    type_asked: bool,
    /// The cursor (cut, frame) the sheet last scrolled to: a new one is
    /// scrolled into view, so the entry stays laid out under the keys.
    followed: Option<(anim_core::ids::CutId, u32)>,
}

impl SheetUi {
    /// GM14: the Type-into-cell verb — the entry on the cursor's cell
    /// takes the keys on the sheet's next frame, its row brought into view.
    pub fn ask_entry(&mut self) {
        self.type_asked = true;
        self.followed = None;
    }

    /// The centre of drawing column `ci`'s cell on `frame`, as last drawn.
    pub fn cell_center(&self, ci: usize, frame: u32) -> Option<egui::Pos2> {
        self.origin.map(|o| {
            o + vec2(
                GUTTER_W + ci as f32 * COL_W + COL_W * 0.5,
                frame as f32 * ROW_H + ROW_H * 0.5,
            )
        })
    }
}

/// GM14: a cut length typed as frames ("60") or seconds+frames ("2+12",
/// "2s+12k", "3s"); None for anything else, or zero.
pub fn parse_length(text: &str, fps: u32) -> Option<u32> {
    let t = text.trim().to_lowercase();
    let num = |s: &str, unit: char| s.trim().trim_end_matches(unit).trim().parse::<u32>().ok();
    let frames = match t.split_once('+') {
        Some((s, k)) => num(s, 's')?.checked_mul(fps)?.checked_add(num(k, 'k')?)?,
        None if t.ends_with('s') => num(&t, 's')?.checked_mul(fps)?,
        None => t.parse::<u32>().ok()?,
    };
    (frames > 0).then_some(frames)
}

/// GM14: frames the way the paper sheet says them — seconds+frames.
pub fn seconds_plus_frames(frames: u32, fps: u32) -> String {
    let fps = fps.max(1);
    format!("{}+{}", frames / fps, frames % fps)
}

/// GM14: the copied cells as text for the system clipboard (a tab per
/// column, a line per frame, "x" for empty) — and so that the system's
/// paste key reaches the app at all (it sends nothing when its clipboard
/// is empty).
fn block_text(state: &AppState, block: &anim_core::sheet_ops::Block) -> String {
    let name = |e: Exposure| match e {
        Exposure::Empty => "x".to_string(),
        Exposure::Drawing(d) => block
            .drawings
            .iter()
            .find(|x| x.id == d)
            .map(|x| x.name.clone())
            .or_else(|| state.cut().drawing(d).map(|x| x.name.clone()))
            .unwrap_or_default(),
    };
    let mut rows = vec![Vec::new(); block.len as usize];
    for runs in &block.columns {
        let mut f = 0usize;
        for &(e, n) in runs {
            for i in 0..n as usize {
                if let Some(row) = rows.get_mut(f + i) {
                    row.push(if i == 0 { name(e) } else { String::new() });
                }
            }
            f += n as usize;
        }
    }
    rows.iter().map(|r| r.join("\t")).collect::<Vec<_>>().join("\n")
}

/// GM14: hand a freshly filled sheet clipboard's text to the system
/// clipboard — however it was filled (these keys, the menu, the palette) —
/// so the paste key sends that text back and a paste can tell the block
/// from text copied elsewhere since.
fn publish_clipboard(ui: &egui::Ui, state: &mut AppState) {
    if let Some(block) = &state.sheet_ui.clipboard
        && state.sheet_ui.clip_text.is_none()
    {
        let text = block_text(state, block);
        ui.ctx().copy_text(text.clone());
        state.sheet_ui.clip_text = Some(text);
    }
}

/// GM14: the system clipboard keys over the sheet. egui-winit turns
/// Ctrl+C / Ctrl+X / Ctrl+V into Copy / Cut / Paste events (no key
/// event), so the sheet answers them here, when the pointer is over it
/// and no field is typing; the same verbs as the Copy/Cut/Paste cells
/// actions. A frame that also carries the chord's key leaves it to the
/// action, so one press never acts twice. A paste lands the cells only
/// when it carries the block's own text; other text is refused.
fn clipboard_events(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool) {
    publish_clipboard(ui, state);
    if !ui.rect_contains_pointer(ui.max_rect()) || ui.ctx().text_edit_focused() {
        return;
    }
    let (copy, cut, paste, shift, keyed) = ui.input(|i| {
        let mut out = (false, false, None, i.modifiers.shift, false);
        for e in &i.events {
            match e {
                egui::Event::Copy => out.0 = true,
                // Shift+Delete is Cut on Windows too: only Ctrl+X cuts cells.
                egui::Event::Cut if i.modifiers.command => out.1 = true,
                egui::Event::Paste(text) => out.2 = Some(text.clone()),
                _ => {}
            }
        }
        out.4 = i.modifiers.command
            && [egui::Key::C, egui::Key::X, egui::Key::V]
                .iter()
                .any(|k| i.key_pressed(*k));
        out
    });
    if keyed {
        return;
    }
    if copy || cut {
        if cut && gesture_safe {
            state.cut_cells();
        } else {
            state.copy_cells();
        }
        publish_clipboard(ui, state);
    } else if let Some(text) = paste
        && gesture_safe
    {
        // The system hands text back with its own line ends.
        let plain = |t: &str| t.replace("\r\n", "\n").trim_end().to_string();
        if state.sheet_ui.clip_text.as_deref().map(plain) == Some(plain(&text)) {
            state.paste_cells(shift);
        } else {
            state.refuse("refused — the clipboard holds text, not cells (copy cells first)");
        }
    }
}

/// GM14: the cut's length field — frames or seconds+frames, Enter sets
/// it (one undo step); beside it the length as seconds+frames.
fn length_field(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool) {
    let (frames, fps) = (state.frame_count(), state.fps());
    let id = egui::Id::new(LENGTH_ID);
    let typing = ui.memory(|m| m.has_focus(id));
    if !typing && state.sheet_ui.length_at != Some(frames) {
        state.sheet_ui.length = frames.to_string();
        state.sheet_ui.length_at = Some(frames);
    }
    plate::legend(ui, "shaku (length)");
    let resp = ui
        .add(
            egui::TextEdit::singleline(&mut state.sheet_ui.length)
                .id(id)
                .desired_width(44.0)
                .font(egui::FontId::monospace(11.0)),
        )
        .on_hover_text(
            "the cut's length: frames (60) or seconds+frames (2+12) — Enter sets it, one undo step",
        );
    ui.label(
        egui::RichText::new(seconds_plus_frames(frames, fps))
            .monospace()
            .size(11.0)
            .color(plate::LEGEND),
    );
    if resp.lost_focus() {
        // The Enter that sets the length is the field's, not the sheet's
        // Type-into-cell key.
        let enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        if enter && gesture_safe {
            match parse_length(&state.sheet_ui.length, fps) {
                Some(n) => state.set_cut_length(n),
                None => state.refuse("cut length: type frames (60) or seconds+frames (2+12)"),
            }
        }
        // Reseed from the cut (the new length, or back to the old one).
        state.sheet_ui.length_at = None;
    }
}

/// GM14: the memo/dialogue field for the current frame (serifu, action
/// notes): the frame's memo, set when the field lets go (Escape cancels).
fn memo_field(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool) {
    let here = (state.view.cut, state.view.frame);
    let id = egui::Id::new(MEMO_ID);
    let typing = ui.memory(|m| m.has_focus(id));
    if !typing && state.sheet_ui.memo_at != Some(here) {
        state.sheet_ui.memo = state.cut().memos.get(&here.1).cloned().unwrap_or_default();
        state.sheet_ui.memo_at = Some(here);
    }
    plate::legend(ui, "memo (serifu)");
    let resp = ui
        .add(
            egui::TextEdit::singleline(&mut state.sheet_ui.memo)
                .id(id)
                .desired_width(130.0)
                .hint_text("serifu / action"),
        )
        .on_hover_text("this frame's memo: dialogue (serifu) and action notes — one undo step");
    if resp.lost_focus() {
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        // The field's own Enter, not the sheet's Type-into-cell key.
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        // The frame the field was seeded for, even if the playhead moved.
        if let Some((cut, frame)) = state.sheet_ui.memo_at
            && !escaped
            && gesture_safe
            && cut == state.view.cut
        {
            let text = state.sheet_ui.memo.clone();
            state.set_memo(frame, &text);
        }
        state.sheet_ui.memo_at = None;
    }
}

/// GM14: typing starts on the cursor's cell before the entry draws — when
/// the Type-into-cell verb asked (Enter over the sheet, the menu), or on a
/// bare digit typed with the pointer over the cells: a drawing number, the
/// entry's first character (3 then Enter exposes drawing 3). Over the
/// sheet the digit is the entry's, not a brush preset's.
fn start_typing(ui: &mut egui::Ui, state: &mut AppState, cells: Rect) {
    let id = egui::Id::new(ENTRY_ID);
    let asked = std::mem::take(&mut state.sheet_ui.type_asked);
    let mut first = String::new();
    if !asked {
        if ui.ctx().text_edit_focused() || !ui.rect_contains_pointer(cells) {
            return;
        }
        let digit = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.is_none() && i.modifiers.is_none() => {
                    let name = key.name();
                    (name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()))
                        .then(|| (*key, name.to_string()))
                }
                _ => None,
            })
        });
        let Some((key, text)) = digit else { return };
        // The key never reaches the presets; its character is the entry's.
        ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, key);
            i.events
                .retain(|e| !matches!(e, egui::Event::Text(t) if *t == text));
        });
        first = text;
    }
    let caret = first.chars().count();
    state.sheet_ui.entry = first;
    ui.memory_mut(|m| m.request_focus(id));
    let mut edit = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
    edit.cursor
        .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(caret))));
    edit.store(ui.ctx(), id);
}

/// GM14: the typed entry on the cursor's cell — a text field, so a
/// single-key shortcut never fires while it types. A drawing name and
/// Enter exposes it ("x" = empty) and moves down; Tab enters and moves to
/// the next column; Escape or a click away drops the text. Resting, a
/// dim caret marks the cell that takes typing; typing, a frame rings it.
fn entry_field(ui: &mut egui::Ui, state: &mut AppState, cell: Rect, gesture_safe: bool) {
    let id = egui::Id::new(ENTRY_ID);
    if ui.memory(|m| m.has_focus(id)) {
        ui.painter().rect_filled(cell, 0.0, plate::WELL);
        ui.painter().rect_stroke(
            cell,
            0.0,
            egui::Stroke::new(1.0, plate::LEGEND),
            egui::StrokeKind::Inside,
        );
    } else if state.sheet_ui.entry.is_empty() {
        let x = cell.left() + 2.0;
        ui.painter().line_segment(
            [pos2(x, cell.top() + 4.0), pos2(x, cell.bottom() - 4.0)],
            egui::Stroke::new(1.0, plate::legend_dim()),
        );
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cell));
    let resp = child
        .add(
            egui::TextEdit::singleline(&mut state.sheet_ui.entry)
                .id(id)
                .frame(egui::Frame::NONE)
                .lock_focus(true)
                .desired_width(cell.width())
                .font(egui::FontId::monospace(11.0))
                .text_color(plate::STRUCK),
        )
        .on_hover_text(
            "type a drawing name (x = empty; a digit over the sheet starts it): Enter enters it \
             and moves down, Tab moves right",
        );
    let (enter, tab) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Tab)));
    let tabbed = tab && resp.has_focus();
    if !(resp.lost_focus() || tabbed) {
        return;
    }
    if !(enter || tabbed) || !gesture_safe {
        state.sheet_ui.entry.clear();
        return;
    }
    let text = std::mem::take(&mut state.sheet_ui.entry);
    state.enter_cell(&text);
    if tabbed {
        let cols: Vec<ColumnId> = state.cut().xsheet.columns.iter().map(|c| c.id).collect();
        if let Some(i) = cols.iter().position(|c| *c == state.view.active_column)
            && let Some(next) = cols.get(i + 1)
        {
            state.view.active_column = *next;
        }
    } else if state.view.frame + 1 < state.frame_count() {
        state.goto(state.view.frame + 1);
    }
    // The cursor moved: the range collapses to it, and the field follows.
    state.sheet_ui.anchor = None;
    state.sheet_ui.head = None;
    ui.memory_mut(|m| m.request_focus(id));
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, edit_room: bool, gesture_safe: bool) {
    // Claim the full panel width EVERY frame. egui 0.35 panels are
    // content-sized (the stored panel rect is the content rect, re-measured
    // per frame), so variable-width rows — the layer strip swapping between
    // full rows and the held-frame note during playback, changing text —
    // would otherwise wobble the panel width and nudge the canvas
    // horizontally on every frame of playback.
    ui.set_min_width(ui.available_width());
    clipboard_events(ui, state, gesture_safe);

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
                .button("new drawing")
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
            // (LIFT KEY is the standing control in the Edit-room head —
            // AUDIT [47]: it existed here too, two identical guarded
            // controls for one verb in one panel.)
        });
        ui.add_space(6.0);
        length_field(ui, state, gesture_safe);
        ui.add_space(6.0);
        memo_field(ui, state, gesture_safe);
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
            ui.label(egui::RichText::new("(none yet — just draw)").color(plate::legend_dim()));
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
                        "ghost this drawing on the light box on every frame \
                         (Ao, onion strength) — display only, never exported, \
                         never on the sheet",
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
    let memo_x = GUTTER_W
        + n_cols as f32 * COL_W
        + n_params as f32 * PARAM_COL_W
        + if has_audio { AUDIO_COL_W } else { 0.0 };
    let sheet_w = memo_x + MEMO_COL_W;

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
        // GM14: the memo/dialogue row beside the sheet.
        let (rect, resp) = ui.allocate_exact_size(vec2(MEMO_COL_W, ROW_H), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            "memo",
            egui::FontId::proportional(12.0),
            plate::LEGEND,
        );
        resp.on_hover_text("serifu (dialogue) and action notes — edit in the memo field above");
    });
    // GM14: each column's kind badge (a click steps Cel → BG → Book →
    // Ref) and its hide and lock toggles — the GM5 verbs, per column.
    let mut header_edit: Option<(ColumnId, u8)> = None;
    ui.horizontal(|ui| {
        ui.allocate_exact_size(vec2(GUTTER_W, ROW_H), Sense::hover());
        for col in &state.cut().xsheet.columns {
            let def = state.cut().column_def(col.id);
            let (rect, _) = ui.allocate_exact_size(vec2(COL_W, ROW_H), Sense::hover());
            let parts = [
                (Rect::from_min_size(rect.min, vec2(40.0, ROW_H)), 0u8),
                (Rect::from_min_size(rect.min + vec2(42.0, 0.0), vec2(20.0, ROW_H)), 1),
                (Rect::from_min_size(rect.min + vec2(64.0, 0.0), vec2(20.0, ROW_H)), 2),
            ];
            for (r, what) in parts {
                let resp = ui.interact(r, ui.id().with(("column_head", col.id, what)), Sense::click());
                let (text, on, name) = match what {
                    0 => (
                        def.kind.label().to_string(),
                        false,
                        format!("kind of column {}: {}", col.name, def.kind.label()),
                    ),
                    1 => ("H".to_string(), def.hidden, format!("hide column {}", col.name)),
                    _ => ("L".to_string(), def.locked, format!("lock column {}", col.name)),
                };
                let ink = match (what, on) {
                    (1, true) => plate::AKA,
                    (2, true) => plate::TALLY,
                    _ if resp.hovered() => plate::STRUCK,
                    _ => plate::legend_dim(),
                };
                let p = ui.painter();
                p.rect_stroke(
                    r.shrink(1.0),
                    0.0,
                    egui::Stroke::new(1.0, if on { ink } else { plate::legend_dim() }),
                    egui::StrokeKind::Inside,
                );
                p.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::monospace(10.0),
                    ink,
                );
                let typ = if what == 0 {
                    egui::WidgetType::Button
                } else {
                    egui::WidgetType::Checkbox
                };
                plate::speak(&resp, typ, (what > 0).then_some(on), &name);
                let hover = match what {
                    0 => "the column's kind: Cel, BG (haikei), Book (over the cels) or Ref (never rendered) — click for the next",
                    1 => "hide or show the column: a hidden column is left out of every composite",
                    _ => "lock or unlock the column: a locked column refuses paint and sheet edits",
                };
                if resp.on_hover_text(hover).clicked() && gesture_safe {
                    header_edit = Some((col.id, what));
                }
            }
        }
    });
    match header_edit {
        Some((col, 0)) => state.cycle_kind_of(col),
        Some((col, 1)) => state.toggle_hidden_of(col),
        Some((col, _)) => state.toggle_locked_of(col),
        None => {}
    }
    if let Some(pid) = clicked_param {
        // Toggle: clicking the selected header closes its strip.
        state.param_sel = if state.param_sel == Some(pid) {
            None
        } else {
            Some(pid)
        };
        state.param_buf_at = None;
    }
    // Frame rows must abut: egui's default item_spacing.y would insert a
    // few px between each allocated row, so ROW_H would NOT be the true
    // row pitch and the continuation handles (which step by ROW_H) would
    // drift farther down the more frames there are. Zero it here so the
    // sheet is contiguous and ctop + frame*ROW_H lands exactly on a row.
    // GM14: zeroed OUTSIDE the scroll area too — show_rows reads the row
    // pitch from the outer spacing.
    let col_ids: Vec<ColumnId> = state.cut().xsheet.columns.iter().map(|c| c.id).collect();
    let range = state.sheet_range();
    let frame_count = state.frame_count();
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        // GM14: following the cursor jumps at once — the next frame lays
        // out the cursor's row, so the typed entry never drops out.
        .animated(false)
        // GM14: only the visible rows are laid out (a 600-frame cut).
        .show_rows(ui, ROW_H, frame_count as usize, |ui, rows| {
            ui.spacing_mut().item_spacing.y = 0.0;
            state.sheet_ui.rows_shown = rows.clone();
            // THE RULE paints in ONE pass after the rows (spec §5.2) — from
            // a whole-sheet painter, never from painter_at(row_rect), which
            // would clip every stroke to one row.
            let sheet_painter = ui.painter().clone();
            let ppp = ui.ctx().pixels_per_point();
            let fps = state.fps();
            let mut clicked_frame: Option<u32> = None;
            let mut type_here = false;
            // Content origin (top-left of frame 0's row) for handle geometry.
            let mut geom: Option<(f32, f32)> = None;
            let row_w = sheet_w.max(ui.available_width());
            for frame in rows.start as u32..rows.end as u32 {
                let (row_rect, _) = ui.allocate_exact_size(vec2(row_w, ROW_H), Sense::hover());
                if geom.is_none() {
                    geom = Some((row_rect.top() - frame as f32 * ROW_H, row_rect.left()));
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

                // GM14: the memo/dialogue cell.
                if let Some(memo) = cut.memos.get(&frame) {
                    let cell = Rect::from_min_size(
                        pos2(row_rect.left() + memo_x, row_rect.top()),
                        vec2(MEMO_COL_W, ROW_H),
                    );
                    painter.with_clip_rect(cell).text(
                        pos2(cell.left() + 4.0, cell.center().y),
                        egui::Align2::LEFT_CENTER,
                        memo,
                        egui::FontId::proportional(11.0),
                        plate::STRUCK,
                    );
                }

                // GM14: the selected range of cells (a wash), else the
                // cursor's cell (a ring).
                let cell_of = |ci: usize| {
                    Rect::from_min_size(
                        pos2(row_rect.left() + GUTTER_W + ci as f32 * COL_W, row_rect.top()),
                        vec2(COL_W, ROW_H),
                    )
                };
                match &range {
                    Some((cols, f0, f1)) if (*f0..*f1).contains(&frame) => {
                        for c in cols {
                            if let Some(ci) = col_ids.iter().position(|x| x == c) {
                                painter.rect_filled(cell_of(ci), 0.0, plate::AO.gamma_multiply(0.22));
                            }
                        }
                    }
                    None if is_current => {
                        if let Some(ci) = col_ids.iter().position(|x| *x == state.view.active_column) {
                            painter.rect_stroke(
                                cell_of(ci),
                                0.0,
                                egui::Stroke::new(1.0, plate::TALLY),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                    _ => {}
                }
            }

            // GM14: the cells answer clicks and drags. A click selects the
            // cell (Shift+click runs the range from the anchor), a drag
            // sweeps a range across columns and frames; a click outside
            // the drawing columns jumps the playhead as before. Registered
            // under the scrub lane, the handles and the entry field.
            state.sheet_ui.origin = geom.map(|(ctop, cleft)| pos2(cleft, ctop));
            if let Some((ctop, cleft)) = geom {
                let shown = Rect::from_min_max(
                    pos2(cleft, ctop + rows.start as f32 * ROW_H),
                    pos2(cleft + row_w, ctop + rows.end as f32 * ROW_H),
                );
                let cresp = ui.interact(shown, ui.id().with("sheet_cells"), Sense::click_and_drag());
                let cell_at = |p: egui::Pos2| {
                    let f = (((p.y - ctop) / ROW_H).floor() as i64)
                        .clamp(0, frame_count as i64 - 1) as u32;
                    let ci = ((p.x - cleft - GUTTER_W) / COL_W).floor();
                    let col = if ci >= 0.0 {
                        col_ids.get(ci as usize).copied()
                    } else {
                        None
                    };
                    (col, f, p.x - cleft >= memo_x)
                };
                if cresp.clicked()
                    && let Some(p) = cresp.interact_pointer_pos()
                {
                    match cell_at(p) {
                        (Some(col), f, _) => {
                            let shift = ui.input(|i| i.modifiers.shift);
                            let cursor = (state.view.active_column, state.view.frame);
                            // A click on the cursor's own cell — anywhere
                            // on it — types into it (focused once the entry
                            // has drawn: it gives up focus to a click off it).
                            type_here |= !shift && (col, f) == cursor;
                            let live = state.sheet_ui.head == Some(cursor);
                            let anchor = match state.sheet_ui.anchor {
                                Some(a) if shift && live => a,
                                _ if shift => cursor,
                                _ => (col, f),
                            };
                            state.select_cells(anchor, col, f);
                        }
                        (None, f, memo) => {
                            clicked_frame = Some(f);
                            if memo {
                                ui.memory_mut(|m| m.request_focus(egui::Id::new(MEMO_ID)));
                            }
                        }
                    }
                }
                if cresp.drag_started()
                    && let Some(p) = ui.input(|i| i.pointer.press_origin())
                    && let (Some(col), f, _) = cell_at(p)
                {
                    state.select_cells((col, f), col, f);
                }
                if cresp.dragged()
                    && let Some(p) = cresp.interact_pointer_pos()
                    && let (Some(col), f, _) = cell_at(p)
                    && let Some(anchor) = state.sheet_ui.anchor
                    && (col, f) != (state.view.active_column, state.view.frame)
                {
                    state.select_cells(anchor, col, f);
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
                // The rules of the rows laid out (and the last one's foot).
                for f in rows.start as u32..=(rows.end as u32).min(frame_count) {
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
            // GM14: the typed entry rides on the cursor's cell (right of
            // the key dot), on top of everything else in the sheet. While it
            // types it is laid out even off the shown rows (the follow below
            // scrolls them to it), so Enter past the last row keeps the keys.
            if let Some((ctop, cleft)) = geom
                && let Some(ci) = col_ids.iter().position(|c| *c == state.view.active_column)
            {
                let cells = Rect::from_min_max(
                    pos2(cleft + GUTTER_W, ctop + rows.start as f32 * ROW_H),
                    pos2(
                        cleft + GUTTER_W + col_ids.len() as f32 * COL_W,
                        ctop + rows.end as f32 * ROW_H,
                    ),
                );
                start_typing(ui, state, cells);
                let id = egui::Id::new(ENTRY_ID);
                if ui.memory(|m| m.has_focus(id)) || rows.contains(&(state.view.frame as usize)) {
                    let cell = Rect::from_min_size(
                        pos2(
                            cleft + GUTTER_W + ci as f32 * COL_W + 30.0,
                            ctop + state.view.frame as f32 * ROW_H,
                        ),
                        vec2(COL_W - 32.0, ROW_H),
                    );
                    entry_field(ui, state, cell, gesture_safe);
                    if type_here {
                        ui.memory_mut(|m| m.request_focus(id));
                    }
                }
            } else {
                // No drawing column under the cursor: nothing takes typing.
                state.sheet_ui.type_asked = false;
            }
            if let Some(f) = clicked_frame {
                state.goto(f);
            }
            // GM14: the sheet follows the cursor — a cursor moved by a key,
            // by Enter in the entry or by a jump is scrolled into view (the
            // next frame lays out its row); a scroll by hand stays put.
            let at = (state.view.cut, state.view.frame);
            if state.sheet_ui.followed != Some(at)
                && let Some((ctop, cleft)) = geom
            {
                state.sheet_ui.followed = Some(at);
                ui.scroll_to_rect(
                    Rect::from_min_size(pos2(cleft, ctop + at.1 as f32 * ROW_H), vec2(1.0, ROW_H)),
                    None,
                );
                ui.ctx().request_repaint();
            }
        });
}

/// The parameter keying strip (C1 camera model): shown when a param-column
/// header is selected. Workflow: click a frame row, type the value, "key" —
/// interpolation (hold/linear/ease, GM14: ease in/out, bezier) applies from
/// that key to the next.
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
            (
                "In",
                ParamInterp::EaseIn,
                "ease in: slow start — leaves this key slowly, speeds up to the next",
            ),
            (
                "Out",
                ParamInterp::EaseOut,
                "ease out: slow stop — leaves at speed, slows into the next key",
            ),
            (
                "B",
                ParamInterp::Bezier,
                "bezier: shape the curve to the next key with handles in the Curves pane",
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
            // GM14: a Bezier key keeps its handles (the curves pane's);
            // any other interp has none to keep.
            let handles = existing
                .filter(|_| state.param_interp == ParamInterp::Bezier)
                .and_then(|k| k.handles);
            let key = ParamKey {
                value: state.param_buf,
                interp: state.param_interp,
                handles,
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
                "hold handle — drag out: the hold ends HERE · right-click: \
                 stow (retracts like a tape measure — the hold runs on to the \
                 next key or the sheet's end)",
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

// ---- The layer card (PSD-cel-layers Stage 3) ---------------------------------
// Right-click a layer's name or colour dot: what the layer IS (role), how
// it's known (identity ink), whether it's editable (lock) and whether it
// ghosts (onion). Every change is ONE SetCelLayerProps — one undo step —
// and always clone-and-modify (NEVER-DO 1: a definition is never lost).

/// Identity inks the card offers: the plate's own tokens (spec §3 — no
/// hue outside them; Tally is the armed lamp, never identity).
fn identity_swatches() -> [(&'static str, [u8; 4]); 7] {
    let c = |col: egui::Color32| col.to_array();
    let dim = |col: egui::Color32| {
        let [r, g, b, _] = col.to_array();
        [r, g, b, 150]
    };
    [
        ("struck", c(plate::STRUCK)),
        ("legend", c(plate::LEGEND)),
        ("paper", c(plate::PAPER)),
        ("ao", c(plate::AO)),
        ("ao, dim", dim(plate::AO)),
        ("aka", c(plate::AKA)),
        ("aka, dim", dim(plate::AKA)),
    ]
}

fn layer_card(ui: &mut egui::Ui, state: &mut AppState, lid: LayerId, props: &LayerProps, armed: bool) {
    use anim_core::model::LayerRole;
    ui.set_min_width(220.0);
    plate::legend(ui, "layer card");
    ui.label(
        egui::RichText::new(&props.name)
            .size(12.5)
            .strong()
            .color(props_chip_color(props)),
    );
    ui.label(
        egui::RichText::new("double-click the name in the strip to rename")
            .size(10.0)
            .color(plate::legend_dim()),
    );
    ui.add_space(4.0);

    // ROLE — what drives behaviour. Changing it re-labels only a layer
    // still wearing its old role's default name (a custom name is the
    // artist's; never overwritten).
    plate::legend(ui, "role");
    for role in LayerRole::ALL.iter().rev() {
        let on = props.role == *role;
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
            ui.painter()
                .circle_filled(dot.center(), 3.5, role_chip_color(*role));
            if plate::detent(ui, on, role.label()).clicked() && !on {
                let name = if props.name == props.role.default_name() {
                    role.default_name().to_string()
                } else {
                    props.name.clone()
                };
                let next = LayerProps {
                    role: *role,
                    name,
                    ..props.clone()
                };
                state.layer_set_props(lid, next.clone());
                if armed {
                    state.arm_layer(&next);
                }
            }
        });
    }
    ui.add_space(4.0);

    // IDENTITY INK — the chip; "role" returns to the convention colour.
    plate::legend(ui, "identity ink");
    ui.horizontal_wrapped(|ui| {
        let role_pick = props.chip.is_none();
        if plate::detent(ui, role_pick, "role").clicked() && !role_pick {
            state.layer_set_props(
                lid,
                LayerProps {
                    chip: None,
                    ..props.clone()
                },
            );
        }
        for (label, rgba) in identity_swatches() {
            let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
            let [cr, cg, cb, ca] = rgba;
            ui.painter().circle_filled(
                r.center(),
                6.0,
                egui::Color32::from_rgba_unmultiplied(cr, cg, cb, ca),
            );
            if props.chip == Some(rgba) {
                ui.painter().circle_stroke(
                    r.center(),
                    7.5,
                    egui::Stroke::new(1.0, plate::STRUCK),
                );
            }
            if resp.on_hover_text(label).clicked() && props.chip != Some(rgba) {
                state.layer_set_props(
                    lid,
                    LayerProps {
                        chip: Some(rgba),
                        ..props.clone()
                    },
                );
            }
        }
    });
    ui.add_space(4.0);

    // LOCK + ONION — latches, one undo step each.
    let mut locked = props.locked;
    plate::latch(ui, &mut locked, "locked").on_hover_text(
        "every painting tool refuses on a locked layer (pen, eraser, fill, lasso, select)",
    );
    if locked != props.locked {
        state.layer_set_props(
            lid,
            LayerProps {
                locked,
                ..props.clone()
            },
        );
    }
    let mut onion = props.onion;
    plate::latch(ui, &mut onion, "in onion ghosts")
        .on_hover_text("off: this layer never appears in onion-skin ghosts (the flip still shows it)");
    if onion != props.onion {
        state.layer_set_props(
            lid,
            LayerProps {
                onion,
                ..props.clone()
            },
        );
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
                ui.label(egui::RichText::new("layer cap (8) reached").color(plate::legend_dim()).small());
                return;
            }
            if !have_cel {
                ui.label(
                    egui::RichText::new("no cel on this frame (draw or press E)")
                        .color(plate::legend_dim())
                        .small(),
                );
                return;
            }
            // PSD-cel-layers: the menu lists ROLES, top of the pipeline
            // first; each lands at its role's rank (custom: above active).
            for role in anim_core::model::LayerRole::ALL.iter().rev().skip(1) {
                let text = egui::RichText::new(role.label()).color(role_chip_color(*role));
                if ui.button(text).clicked() {
                    state.layer_add_role(*role);
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .button(egui::RichText::new("custom layer").color(plate::LEGEND))
                .clicked()
            {
                state.layer_add_role(anim_core::model::LayerRole::Custom);
                ui.close();
            }
        })
        .response
        .on_hover_text("add a layer — it lands at its role's pipeline position");

        // PSD-cel-layers Stage 4: the stack presets. Picking binds the
        // CURRENT room's new cels; apply/conform only ever ADD layers.
        ui.menu_button("stack", |ui| {
            ui.set_min_width(240.0);
            plate::legend(ui, "new cels in this room get");
            let stacks = state.stacks.clone();
            for s in &stacks {
                let on = s.name == state.stack_current;
                ui.horizontal(|ui| {
                    if plate::detent(ui, on, &s.name).clicked() && !on {
                        state.stack_request = Some(crate::doc::StackRequest::Pick(s.name.clone()));
                    }
                });
                ui.label(
                    egui::RichText::new(s.summary())
                        .size(10.0)
                        .color(plate::legend_dim()),
                );
            }
            ui.separator();
            let cur = state.stack_current.clone();
            let can = have_cel;
            if ui
                .add_enabled(can, egui::Button::new(format!("add missing '{cur}' layers to this cel")))
                .on_hover_text("adds only the layers this cel lacks — never deletes, renames, or moves painted layers")
                .clicked()
            {
                state.apply_stack_to_cel(&cur);
                ui.close();
            }
            if ui
                .button(format!("conform the whole cut to '{cur}'"))
                .on_hover_text("the same additive apply for every layered cel in this cut — one undo step")
                .clicked()
            {
                state.conform_cut(&cur);
                ui.close();
            }
            ui.separator();
            plate::legend(ui, "save this cel's stack");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut state.stack_name_buf).desired_width(130.0));
                let name = state.stack_name_buf.trim().to_string();
                if ui
                    .add_enabled(can && !name.is_empty(), egui::Button::new("save"))
                    .on_hover_text("structure only — names, roles, inks, locks, opacity; never pixels")
                    .clicked()
                {
                    state.stack_request = Some(crate::doc::StackRequest::Save(name));
                    state.stack_name_buf.clear();
                    ui.close();
                }
            });
        })
        .response
        .on_hover_text("layer-stack presets for the modern pipeline — LO, genga, douga, shiage");
    });

    // Owned snapshot so row actions can mutate state freely. The FULL props
    // ride along: every row action clones-and-modifies, so a rename or an
    // eye click can never strip a layer's definition (PSD-cel-layers
    // NEVER-DO 1).
    let full: Vec<LayerProps> = state
        .strip_layers()
        .iter()
        .map(|(_, l)| l.props.clone())
        .collect();
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
        // Say what a stroke here will ACTUALLY create (the held cel's
        // cloned stack, else the chosen preset) — never a fixed pair.
        let names: Vec<String> = state
            .new_cel_specs()
            .iter()
            .rev()
            .map(|p| p.name.clone())
            .collect();
        ui.label(
            egui::RichText::new(format!("held/empty frame — a new cel gets {}", names.join(" · ")))
                .color(plate::legend_dim())
                .small(),
        );
        ui.add_space(2.0);
        return;
    }
    let armed_id = state.active_layer_id();
    for (row_i, (lid, name, visible, opacity)) in rows.iter().enumerate() {
        let is_active = armed_id == Some(*lid);
        let props = full[row_i].clone();
        ui.horizontal(|ui| {
            // Eye: visibility toggle (one click = one undo step). Hidden is a
            // configuration, so the struck eye dims — it does not go red.
            let eye_icon = if *visible { Icon::Eye } else { Icon::EyeOff };
            if plate::icon_button(ui, eye_icon, "", "show / hide").clicked() {
                state.layer_set_props(
                    *lid,
                    LayerProps {
                        visible: !visible,
                        ..props.clone()
                    },
                );
            }
            // PSD-cel-layers: the lock, beside the eye — one click = one
            // undo step. Unlocked is the quiet state (dim shackle).
            let lock_color = if props.locked {
                plate::STRUCK
            } else {
                plate::legend_dim()
            };
            let (lock_rect, lock_resp) = ui.allocate_exact_size(vec2(16.0, 18.0), Sense::click());
            plate::speak(
                &lock_resp,
                egui::WidgetType::Checkbox,
                Some(props.locked),
                &format!("lock layer {name}"),
            );
            crate::icons::paint(
                ui.painter(),
                Rect::from_center_size(lock_rect.center(), vec2(11.0, 11.0)),
                lock_color,
                Icon::Lock,
            );
            if lock_resp
                .on_hover_text(if props.locked {
                    "locked — click to unlock"
                } else {
                    "lock this layer (every painting tool refuses)"
                })
                .clicked()
            {
                state.layer_set_props(
                    *lid,
                    LayerProps {
                        locked: !props.locked,
                        ..props.clone()
                    },
                );
            }
            // RETAS colour dot — the layer's identity ink. Click or
            // right-click: the layer card.
            let (dot_rect, dot_resp) = ui.allocate_exact_size(vec2(12.0, 18.0), Sense::click());
            plate::speak(
                &dot_resp,
                egui::WidgetType::Button,
                None,
                &format!("layer colour {name}"),
            );
            ui.painter()
                .circle_filled(dot_rect.center(), 4.0, props_chip_color(&props));
            let dot_resp = dot_resp.on_hover_text(format!(
                "{} — click: define this layer",
                props.role.label()
            ));
            egui::Popup::menu(&dot_resp).show(|ui| {
                layer_card(ui, state, *lid, &props, is_active);
            });

            // Name: click = activate, double-click = rename — in a FIXED
            // cell (spec defect 10): the columns after it never drift with
            // name length; the text elides inside.
            // Reserve what the row's fixed furniture actually needs —
            // eye 27 + dot 12 + two chevrons 54 + slider 60 + "100%" 34 +
            // spacing — or the percentage clips off the panel's edge.
            let name_w = (ui.available_width() - 190.0).max(48.0);
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
                                // A rename is a LABEL change — the role
                                // (and with it every behaviour) stays.
                                let renamed = LayerProps {
                                    name: trimmed.to_string(),
                                    ..props.clone()
                                };
                                state.layer_set_props(*lid, renamed.clone());
                                // The armed layer stays armed under its
                                // new label.
                                if is_active {
                                    state.arm_layer(&renamed);
                                }
                            }
                        } else {
                            resp.request_focus();
                            state.strip_rename = Some((*lid, buf));
                        }
                    } else {
                        // LINE GUARD (shiage): the guarded line row wears the
                        // shackle; tapping it refuses instead of arming.
                        if state.view.line_guard && props.role.is_line() {
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
                        let resp = ui.selectable_label(is_active, text).on_hover_text(
                            "click: edit this layer — double-click: rename — right-click: define",
                        );
                        resp.context_menu(|ui| {
                            layer_card(ui, state, *lid, &props, is_active);
                        });
                        if resp.double_clicked() {
                            state.strip_rename = Some((*lid, name.clone()));
                        } else if resp.clicked() {
                            if state.view.line_guard && props.role.is_line() {
                                state.refuse(
                                    "refused — line guard is on (unlatch it to arm a line layer)",
                                );
                            } else {
                                state.arm_layer(&props);
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
                            opacity: live,
                            ..props.clone()
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
                        opacity: live,
                        ..props.clone()
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
        let active_lid = state.active_layer_id();
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

/// GM14: the sheet panel driven through the harness — range select, the
/// typed entry, the clipboard, the length field, the header toggles and a
/// long cut.
#[cfg(test)]
mod sheetui_tests {
    use anim_core::ids::ColumnId;
    use anim_core::xsheet::Exposure;
    use eframe::egui;

    use super::{parse_length, seconds_plus_frames};
    use crate::config::Action;
    use crate::testkit::{CTRL, EditorHarness};
    use crate::workspace::Pane;

    /// Only the sheet on screen; column A shows drawings keyed at frames
    /// 0, 4 and 8 of 48, the cursor on frame 0.
    fn sheet() -> EditorHarness {
        let mut h = EditorHarness::new();
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::XSheet]);
        let s = h.state_mut();
        for f in [4, 8] {
            s.goto(f);
            s.new_drawing_at_frame();
        }
        s.goto(0);
        h.run();
        h.run();
        h
    }

    fn cell(h: &EditorHarness, ci: usize, frame: u32) -> egui::Pos2 {
        h.state().sheet_ui.cell_center(ci, frame).expect("the sheet drew its rows")
    }

    fn column(h: &EditorHarness, ci: usize) -> ColumnId {
        h.state().cut().xsheet.columns[ci].id
    }

    fn shown(h: &EditorHarness, ci: usize, frame: u32) -> Exposure {
        h.state().cut().xsheet.columns[ci].shown(frame)
    }

    #[test]
    fn sheetui_shift_click_selects_the_range_across_columns() {
        let mut h = sheet();
        h.state_mut().add_column();
        h.run();
        let (a, b) = (column(&h, 0), column(&h, 1));
        h.click_pos(cell(&h, 0, 2));
        assert_eq!(h.state().view.active_column, a);
        assert_eq!(h.state().view.frame, 2);
        assert_eq!(h.state().sheet_range(), None, "a click is just the cursor's cell");
        h.set_mods(egui::Modifiers::SHIFT);
        h.click_pos(cell(&h, 1, 5));
        h.set_mods(egui::Modifiers::NONE);
        assert_eq!(h.state().sheet_range(), Some((vec![a, b], 2, 6)));
        // A drag sweeps a range too.
        h.drag(cell(&h, 1, 10), cell(&h, 0, 12), 4);
        assert_eq!(h.state().sheet_range(), Some((vec![a, b], 10, 13)));
        // Stepping the cursor away collapses the range.
        h.state_mut().goto(20);
        assert_eq!(h.state().sheet_range(), None);
    }

    #[test]
    fn sheetui_typed_entry_exposes_by_name_in_one_step_and_fires_no_tool() {
        let mut h = sheet();
        let c = cell(&h, 0, 2);
        h.click_pos(c);
        // The entry rides on the cursor's cell: a second click types.
        h.click_pos(c);
        assert!(h.typing(), "the entry took focus\n{}", h.describe());
        let steps = h.state().engine.position();
        h.key(egui::Key::Num3);
        h.key(egui::Key::Enter);
        let s = h.state();
        let three = s.cut().drawings.iter().find(|d| d.name == "3").expect("drawing 3 made");
        assert_eq!(s.cut().xsheet.columns[0].key_at(2), Some(Exposure::Drawing(three.id)));
        assert_eq!(s.engine.position(), steps + 1, "one entry, one undo step");
        assert_eq!(s.view.frame, 3, "Enter moves down");
        assert!(h.typing(), "the entry follows the cursor");
        // B, 1 and 2 are the brush and two presets — typed, they are text.
        h.editor_mut().canvas.tool = crate::canvas::CanvasTool::Select;
        h.key_mods(egui::Modifiers::SHIFT, egui::Key::B);
        h.key(egui::Key::Num1);
        h.key(egui::Key::Num2);
        assert_eq!(h.state().sheet_ui.entry, "B12");
        assert_eq!(h.editor().canvas.tool, crate::canvas::CanvasTool::Select, "no tool changed");
        let fired: Vec<Action> = h.performed().iter().map(|(a, _)| *a).collect();
        assert!(
            !fired.iter().any(|a| matches!(
                a,
                Action::BrushTool | Action::Preset1 | Action::Preset2 | Action::Preset3
            )),
            "{fired:?}"
        );
        // "x" empties the cell.
        h.key_mods(CTRL, egui::Key::A);
        h.type_text("x");
        h.key(egui::Key::Enter);
        assert_eq!(h.state().cut().xsheet.columns[0].key_at(3), Some(Exposure::Empty));
    }

    #[test]
    fn sheetui_copy_then_paste_on_another_column_reproduces_the_block() {
        let mut h = sheet();
        h.state_mut().add_column();
        h.run();
        h.click_pos(cell(&h, 0, 2));
        h.set_mods(egui::Modifiers::SHIFT);
        h.click_pos(cell(&h, 0, 5));
        h.set_mods(egui::Modifiers::NONE);
        h.key_mods(CTRL, egui::Key::C);
        assert_eq!(h.state().sheet_ui.clipboard.as_ref().map(|b| b.len), Some(4));
        h.click_pos(cell(&h, 1, 10));
        let steps = h.state().engine.position();
        h.key_mods(CTRL, egui::Key::V);
        for i in 0..4 {
            assert_eq!(shown(&h, 1, 10 + i), shown(&h, 0, 2 + i), "frame {}", 10 + i);
        }
        assert_eq!(shown(&h, 1, 14), Exposure::Empty, "the block's length only");
        assert_eq!(h.state().engine.position(), steps + 1, "one paste, one undo step");
        // The system clipboard's Paste event (what Ctrl+V sends in the
        // real window) lands the same block: it carries the text the copy
        // handed to the system clipboard.
        h.click_pos(cell(&h, 1, 20));
        let text = h.state().sheet_ui.clip_text.clone().expect("the copy handed its text over");
        h.event(egui::Event::Paste(text));
        for i in 0..4 {
            assert_eq!(shown(&h, 1, 20 + i), shown(&h, 0, 2 + i), "frame {}", 20 + i);
        }
        // Text copied from elsewhere since is not cells: refused, untouched.
        h.click_pos(cell(&h, 1, 0));
        let steps = h.state().engine.position();
        h.event(egui::Event::Paste("1".into()));
        assert_eq!(h.state().engine.position(), steps, "foreign text pastes nothing");
        assert_eq!(shown(&h, 1, 0), Exposure::Empty);
        assert!(h.state().refusal.contains("clipboard holds text"), "{}", h.state().refusal);
    }

    #[test]
    fn sheetui_a_digit_types_into_the_selected_cell_without_a_second_click() {
        let mut h = sheet();
        h.click_pos(cell(&h, 0, 2));
        assert!(!h.typing(), "one click selects the cell");
        let steps = h.state().engine.position();
        // Over the sheet a bare digit is a drawing number, not a preset.
        h.key(egui::Key::Num3);
        assert!(h.typing(), "the digit started the entry\n{}", h.describe());
        assert_eq!(h.state().sheet_ui.entry, "3");
        h.key(egui::Key::Enter);
        let s = h.state();
        let three = s.cut().drawings.iter().find(|d| d.name == "3").expect("drawing 3 made");
        assert_eq!(s.cut().xsheet.columns[0].key_at(2), Some(Exposure::Drawing(three.id)));
        assert_eq!(s.engine.position(), steps + 1, "one entry, one undo step");
        let fired: Vec<Action> = h.performed().iter().map(|(a, _)| *a).collect();
        assert!(!fired.contains(&Action::Preset3), "{fired:?}");
        // Escape drops the entry; Enter over the sheet (Type into cell,
        // Enter in both keymaps) takes it up again.
        h.key(egui::Key::Escape);
        assert!(!h.typing());
        h.key(egui::Key::Enter);
        assert!(h.typing(), "Enter over the sheet types\n{}", h.describe());
        assert!(h.performed().iter().any(|(a, _)| *a == Action::TypeIntoCell));
        for km in crate::config::Keymap::ALL {
            let binds = crate::config::default_binds(*km);
            let chord = crate::cmdpal::chord_of(&binds, Action::TypeIntoCell);
            assert_eq!(chord.as_deref(), Some("Enter"), "{km:?}");
        }
        // Anywhere on the cursor's cell — the key dot's side too — types.
        h.key(egui::Key::Escape);
        h.click_pos(cell(&h, 0, 5));
        let left = cell(&h, 0, 5) - egui::vec2(super::COL_W * 0.4, 0.0);
        h.click_pos(left);
        assert!(h.typing(), "a click on the cell's left side types\n{}", h.describe());
    }

    #[test]
    fn sheetui_typing_down_past_the_shown_rows_keeps_the_keys() {
        let mut h = sheet();
        h.state_mut().set_cut_length(600);
        h.run();
        let rows = h.state().sheet_ui.rows_shown.clone();
        let low = rows.end as u32 - 3;
        h.click_pos(cell(&h, 0, low));
        for i in 0..6 {
            h.key(egui::Key::Num1);
            assert!(h.typing(), "entry {i} has the keys\n{}", h.describe());
            h.key(egui::Key::Enter);
        }
        let frame = h.state().view.frame;
        assert_eq!(frame, low + 6);
        assert!(frame as usize >= rows.end, "the cursor left the first screen: {rows:?}");
        assert!(h.typing(), "the entry kept the keys past the first screen");
        let now = h.state().sheet_ui.rows_shown.clone();
        assert!(now.contains(&(frame as usize)), "the sheet followed: {now:?}");
        // A digit there is the entry's text, never a brush preset.
        h.key(egui::Key::Num2);
        assert_eq!(h.state().sheet_ui.entry, "2");
        let fired: Vec<Action> = h.performed().iter().map(|(a, _)| *a).collect();
        assert!(
            !fired.iter().any(|a| matches!(a, Action::Preset1 | Action::Preset2)),
            "{fired:?}"
        );
    }

    #[test]
    fn sheetui_range_verbs_retime_and_reverse_both_columns_of_a_shift_click_range() {
        let mut h = sheet();
        h.state_mut().add_column();
        h.state_mut().add_column();
        h.run();
        let (a, b) = (column(&h, 0), column(&h, 1));
        // Column A's first 12 frames into B and C.
        h.click_pos(cell(&h, 0, 11));
        h.set_mods(egui::Modifiers::SHIFT);
        h.click_pos(cell(&h, 0, 0));
        h.set_mods(egui::Modifiers::NONE);
        h.key_mods(CTRL, egui::Key::C);
        for ci in [1, 2] {
            h.click_pos(cell(&h, ci, 0));
            h.key_mods(CTRL, egui::Key::V);
        }
        let (d0, d4, d8) = (shown(&h, 0, 0), shown(&h, 0, 4), shown(&h, 0, 8));
        assert!(d0 != d4 && d4 != d8, "three drawings: {d0:?} {d4:?} {d8:?}");
        for ci in [1, 2] {
            assert_eq!((shown(&h, ci, 0), shown(&h, ci, 4), shown(&h, ci, 8)), (d0, d4, d8));
        }
        // Shift+click: columns A and B, frames 1 to 8.
        h.click_pos(cell(&h, 0, 0));
        h.set_mods(egui::Modifiers::SHIFT);
        h.click_pos(cell(&h, 1, 7));
        h.set_mods(egui::Modifiers::NONE);
        assert_eq!(h.state().sheet_range(), Some((vec![a, b], 0, 8)));
        let steps = h.state().engine.position();
        h.perform(Action::Reverse);
        for ci in [0, 1] {
            assert_eq!((shown(&h, ci, 0), shown(&h, ci, 3)), (d4, d4), "column {ci} reversed");
            assert_eq!((shown(&h, ci, 4), shown(&h, ci, 7)), (d0, d0), "column {ci} reversed");
            assert_eq!(shown(&h, ci, 8), d8, "column {ci}: past the range untouched");
        }
        assert_eq!((shown(&h, 2, 0), shown(&h, 2, 4)), (d0, d4), "column C is outside the range");
        assert_eq!(h.state().engine.position(), steps + 1, "one verb, one undo step");
        // On twos: each drawing in the range holds 2, what follows ripples
        // up by the same amount in both columns.
        h.perform(Action::RetimeTwos);
        for ci in [0, 1] {
            let got: Vec<_> = (0..5).map(|f| shown(&h, ci, f)).collect();
            assert_eq!(got, vec![d4, d4, d0, d0, d8], "column {ci} on twos");
        }
        assert_eq!((shown(&h, 2, 0), shown(&h, 2, 4)), (d0, d4), "column C untouched");
        assert_eq!(h.state().engine.position(), steps + 2);
    }

    #[test]
    fn sheetui_length_field_sets_sixty_frames_in_one_step() {
        let mut h = sheet();
        let fields = h.text_fields();
        let i = fields.iter().position(|t| t == "48").expect("the length field holds 48");
        assert_eq!(fields.iter().filter(|t| *t == "48").count(), 1, "{fields:?}");
        let steps = h.state().engine.position();
        h.focus_text_field(i);
        h.key_mods(CTRL, egui::Key::A);
        h.type_text("60");
        h.key(egui::Key::Enter);
        assert_eq!(h.state().frame_count(), 60);
        assert_eq!(h.state().engine.position(), steps + 1);
        assert!(h.text_fields().iter().any(|t| t == "60"), "the field reseeds");
        // Seconds+frames: 2+12 at 24 fps is 60 too — nothing to do.
        h.focus_text_field(i);
        h.key_mods(CTRL, egui::Key::A);
        h.type_text("2+12");
        h.key(egui::Key::Enter);
        assert_eq!(h.state().frame_count(), 60);
        assert_eq!(h.state().engine.position(), steps + 1);
    }

    #[test]
    fn sheetui_header_hide_toggle_drops_the_column_from_the_render() {
        let mut h = sheet();
        let a = column(&h, 0);
        assert!(h.state().cut().render_order().contains(&a));
        assert_eq!(h.node_toggled("hide column A"), Some(false));
        let steps = h.state().engine.position();
        h.click("hide column A");
        assert!(!h.state().cut().render_order().contains(&a));
        assert_eq!(h.node_toggled("hide column A"), Some(true));
        assert_eq!(h.state().engine.position(), steps + 1);
        h.click("lock column A");
        assert!(h.state().cut().column_def(a).locked);
        h.click("kind of column A: Cel");
        assert_ne!(h.state().cut().column_def(a).kind, anim_core::xsheet::ColumnKind::Cel);
    }

    #[test]
    fn sheetui_a_600_frame_cut_lays_out_only_the_visible_rows() {
        let mut h = sheet();
        h.state_mut().set_cut_length(600);
        h.run();
        let rows = h.state().sheet_ui.rows_shown.clone();
        assert!(!rows.is_empty() && rows.len() < 80, "{rows:?}");
        h.state_mut().goto(599);
        h.run();
        let rows = h.state().sheet_ui.rows_shown.clone();
        assert!(rows.len() < 80, "{rows:?}");
        assert!(rows.contains(&599), "the sheet scrolled to the cursor: {rows:?}");
        assert_eq!(h.state().frame_count(), 600);
    }

    #[test]
    fn sheetui_length_text_reads_frames_and_seconds_plus_frames() {
        assert_eq!(parse_length("60", 24), Some(60));
        assert_eq!(parse_length(" 2+12 ", 24), Some(60));
        assert_eq!(parse_length("2s+12k", 24), Some(60));
        assert_eq!(parse_length("3s", 24), Some(72));
        assert_eq!(parse_length("0", 24), None);
        assert_eq!(parse_length("two", 24), None);
        assert_eq!(parse_length("1+x", 24), None);
        assert_eq!(seconds_plus_frames(60, 24), "2+12");
        assert_eq!(seconds_plus_frames(48, 24), "2+0");
    }
}

/// GM14's test minimums, counted from the source (a name filter also
/// counts any test whose module path merely holds the word): 10 tests in
/// anim-core's curve_tests.rs, 5 curves_* and 5 sheetui_* app tests.
#[cfg(test)]
mod gm14_minimums {
    /// The test fns named `prefix…` (each right under its #[test] line).
    fn tests_named(src: &str, prefix: &str) -> usize {
        let head = format!("fn {prefix}");
        let lines: Vec<&str> = src.lines().map(str::trim).collect();
        lines
            .windows(2)
            .filter(|w| w[0] == "#[test]" && w[1].starts_with(&head))
            .count()
    }

    #[test]
    fn test_counts_hold_in_source() {
        let core = tests_named(include_str!("../../crates/anim-core/tests/curve_tests.rs"), "");
        assert!(core >= 10, "curve_tests.rs holds {core} tests");
        let curves = tests_named(include_str!("curves_panel.rs"), "curves_");
        assert!(curves >= 5, "{curves} curves_* tests");
        let sheet = tests_named(include_str!("xsheet_panel.rs"), "sheetui_");
        assert!(sheet >= 5, "{sheet} sheetui_* tests");
    }
}
