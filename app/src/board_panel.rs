//! GM15 EDITORIAL (編集): the shinkō board (進行, the cut board) — a card
//! per cut of the scene with its picture, number, length, status,
//! assignee and notes — and the henshū timeline (the episode), the cuts
//! as blocks in order (drag one to move it), with the running time (TRT)
//! against the episode's target; and the retake note window. Every edit
//! goes through AppState as one undo step (D1), and a button naming a
//! verb asks the editor for it, to run through Editor::perform (D6). A
//! card's picture is a CPU render of frame 0 on a worker (D9), keyed on
//! that cut's own frame-0 eval — an edit to another cut leaves it be —
//! and never started mid-stroke (D7).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use anim_core::board::{self, CutStatus, ProdInfo, RetakeNote, TrtMark};
use anim_core::color::ColorLaw;
use anim_core::command::CutRef;
use anim_core::deliver::{self, Purpose, RenderOpts};
use anim_core::ids::{CutId, DrawingId, SceneId};
use anim_core::model::Cut;
use eframe::egui;

use crate::config::Action;
use crate::doc::AppState;
use crate::{plate, vocab};

/// A card's picture width in pixels.
pub const THUMB_W: usize = 128;
/// AU15: the Shots panel over a scene of one shot, above `Add shot`.
pub const ONE_SHOT: &str = "One shot so far.";
const CARD_W: f32 = 150.0;
const CARD_H: f32 = 236.0;
/// The retake note window's text field.
const NOTE_ID: &str = "gaban_retake_note_text";

#[derive(Default)]
pub struct BoardUi {
    /// The board's filters: None = any status / anyone; Some("") = nobody.
    pub status: Option<CutStatus>,
    pub assignee: Option<String>,
    /// An assignee being typed, per cut (lands on Enter or focus out).
    typing: HashMap<CutId, String>,
    /// Card pictures and the key each was drawn at (see [`thumb_key`]).
    thumbs: HashMap<CutId, (u64, egui::TextureHandle)>,
    /// Each cut's picture key now, and the document stamp (scene,
    /// generation, revision, look) it was read at: read again only when
    /// the stamp moves, never mid-stroke.
    keys: HashMap<CutId, u64>,
    keyed: Option<(SceneId, u64, u64, u64)>,
    /// The picture being drawn on the worker (one at a time).
    job: Option<ThumbJob>,
    /// The episode block being dragged (its index in the scene).
    pub drag: Option<usize>,
    /// The target TRT being typed.
    target: String,
    /// GM15: Play episode — at a cut's end playback runs on into the next.
    pub episode_play: bool,
    /// The henshū timeline's points per frame (None = the episode fits).
    zoom: Option<f32>,
    /// The verbs the panes' buttons asked for: the editor runs them
    /// through its one dispatch after the dock, as a menu item (D6).
    pub asked: Vec<Action>,
    /// The retake note being written (the note window shows while Some).
    pub note: Option<NoteForm>,
}

impl BoardUi {
    /// "Add retake note…": the note window opens on cut `at`'s `frame`,
    /// its text field taking the keys, "set Retake" latched.
    pub fn open_note(&mut self, at: CutRef, frame: u32) {
        self.note = Some(NoteForm {
            at,
            frame,
            text: String::new(),
            retake: true,
            focus: true,
        });
    }
}

/// The retake note window's form: the cut and frame the note is about,
/// what to fix, and whether the cut goes to Retake with it.
pub struct NoteForm {
    pub at: CutRef,
    pub frame: u32,
    pub text: String,
    pub retake: bool,
    /// The text field takes the keys on the window's first frame.
    focus: bool,
}

/// A card picture on the worker: its cut, the key it is drawn at, and
/// where the picture arrives.
struct ThumbJob {
    cut: CutId,
    key: u64,
    rx: Receiver<egui::ColorImage>,
}

/// What a card shows, read once a frame (no cut is cloned for it).
struct CardInfo {
    at: CutRef,
    name: String,
    frames: u32,
    prod: ProdInfo,
    /// The cut is an animatic (it has a BOARD column): its card makes
    /// cuts from its panels.
    panels: bool,
}

/// A status row's hover card: where the shot stands, and the studio's word.
fn status_card(word: &str) -> String {
    vocab::with_studio_word(word, &format!("This shot is at: {word}."))
}

/// A cut's length as seconds + koma ("1+12" = a second and 12 frames).
pub fn length_sk(frames: u32, fps: u32) -> String {
    let fps = fps.max(1);
    format!("{}+{:02}", frames / fps, frames % fps)
}

/// Frame 0 of `cut`, small, laid on paper for the card — on the worker
/// (a full-size CPU render, sampled down), relative frames read from the
/// project's folder `dir` (a card has no room for what it could not read:
/// the viewer says it).
fn thumbnail(cut: &Cut, law: ColorLaw, width: u32, height: u32, dir: Option<std::path::PathBuf>) -> egui::ColorImage {
    let opts = RenderOpts {
        purpose: Purpose::View,
        scale: 0.5,
    };
    let env = anim_core::export::RenderEnv::new(dir);
    let img = deliver::render_env(cut, law, 0, width, height, &opts, &env);
    let tw = THUMB_W;
    let th = ((tw as u64 * img.h as u64) / img.w.max(1) as u64).max(1) as usize;
    let paper = [plate::PAPER.r(), plate::PAPER.g(), plate::PAPER.b()].map(|v| v as f32 / 255.0);
    let mut rgb = Vec::with_capacity(tw * th * 3);
    for y in 0..th {
        for x in 0..tw {
            let sx = (x * img.w as usize / tw) as u32;
            let sy = (y * img.h as usize / th) as u32;
            let s = img.pixel(sx.min(img.w - 1), sy.min(img.h - 1));
            for c in 0..3 {
                let v = s[c] + paper[c] * (1.0 - s[3]);
                rgb.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    egui::ColorImage::from_rgb([tw, th], &rgb)
}

/// The frame size and colour law every card picture is drawn under.
fn look_hash(state: &AppState) -> u64 {
    let p = &state.engine.project;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (p.width, p.height, p.color_law).hash(&mut h);
    h.finish()
}

/// Cut `at`'s picture key: its frame-0 eval and the drawings exposed
/// there (the playback cache's frame key, under `look`). An edit to
/// another cut, or to this cut's status, assignee or notes, leaves it be.
fn thumb_key(state: &mut AppState, at: CutRef, look: u64, memo: &mut HashMap<DrawingId, u64>) -> Option<u64> {
    let eval = state.engine.eval(at.scene, at.cut, 0).ok().map(|v| v.hash());
    let cut = state.engine.project.cut(at.scene, at.cut)?;
    Some(crate::playback::frame_hash(cut, eval, 0, look, memo))
}

/// Keep the shown cards' pictures fresh: take a finished picture from the
/// worker; when the document moved (and no stroke is live) read every
/// cut's key again and drop the pictures of cuts no longer in the scene;
/// then, with the worker free, start the first shown card whose picture
/// is missing or behind its key. The worker asks for the repaint.
fn refresh_thumbs(ctx: &egui::Context, state: &mut AppState, scene: SceneId, shown: &[CutId], gesture_safe: bool) {
    if let Some(job) = &state.board.job {
        match job.rx.try_recv() {
            Ok(img) => {
                let (cut, key) = (job.cut, job.key);
                state.board.job = None;
                if state.board.keys.contains_key(&cut) {
                    let tex = ctx.load_texture(format!("board_thumb_{}", cut.0), img, egui::TextureOptions::LINEAR);
                    state.board.thumbs.insert(cut, (key, tex));
                }
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => state.board.job = None,
        }
    }
    if !gesture_safe {
        return;
    }
    let look = look_hash(state);
    let stamp = (scene, state.doc_gen, state.engine.revision(), look);
    if state.board.keyed != Some(stamp) {
        state.board.keyed = Some(stamp);
        let cuts: Vec<CutId> = state
            .engine
            .project
            .scene(scene)
            .map(|s| s.cuts.iter().map(|c| c.id).collect())
            .unwrap_or_default();
        let (mut keys, mut memo) = (HashMap::new(), HashMap::new());
        for cut in cuts {
            if let Some(k) = thumb_key(state, CutRef { scene, cut }, look, &mut memo) {
                keys.insert(cut, k);
            }
        }
        state.board.thumbs.retain(|c, _| keys.contains_key(c));
        state.board.keys = keys;
    }
    let board = &state.board;
    let stale = shown.iter().copied().find(|c| {
        board
            .keys
            .get(c)
            .is_some_and(|k| board.thumbs.get(c).is_none_or(|(held, _)| held != k))
    });
    let Some(id) = stale else {
        return;
    };
    let key = board.keys[&id];
    let p = &state.engine.project;
    let (law, width, height) = (p.color_law, p.width, p.height);
    let Some(cut) = p.cut(scene, id).cloned() else {
        return;
    };
    let dir = state.project_dir();
    let (tx, rx) = channel();
    let repaint = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(thumbnail(&cut, law, width, height, dir));
        repaint.request_repaint();
    });
    state.board.job = Some(ThumbJob { cut: id, key, rx });
}

/// The shinkō board pane. `gesture_safe` is false while a stroke is live:
/// no card picture starts then (D7).
pub fn ui_board(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool) {
    let scene_id = state.at().scene;
    let (status, assignee) = (state.board.status, state.board.assignee.clone());
    let Some(scene) = state.engine.project.scene(scene_id) else {
        ui.label("no scene");
        return;
    };
    let mut assignees: Vec<String> = scene
        .cuts
        .iter()
        .map(|c| ProdInfo::of(c).assignee)
        .filter(|a| !a.is_empty())
        .collect();
    assignees.sort();
    assignees.dedup();
    let total = scene.cuts.len();
    let cards: Vec<CardInfo> = board::filter_cuts(scene, status, assignee.as_deref())
        .into_iter()
        .map(|i| {
            let c = &scene.cuts[i];
            CardInfo {
                at: CutRef {
                    scene: scene_id,
                    cut: c.id,
                },
                name: c.name.clone(),
                frames: c.frame_count,
                prod: ProdInfo::of(c),
                panels: c.xsheet.columns.iter().any(|col| col.name == "BOARD"),
            }
        })
        .collect();
    let shown: Vec<CutId> = cards.iter().map(|c| c.at.cut).collect();
    refresh_thumbs(ui.ctx(), state, scene_id, &shown, gesture_safe);
    plate::legend(ui, vocab::SHOTS).on_hover_text(vocab::with_studio_word(vocab::SHOTS, "Every shot of the scene, by its status."));
    // AU15: a scene of one shot says so, and offers the next (spec 8.4),
    // ahead of the fold.
    if total == 1 {
        ui.label(egui::RichText::new(ONE_SHOT).color(plate::legend_dim()));
        let add = ui.button("Add shot").on_hover_text("Add a shot to this scene.");
        plate::speak(&add, egui::WidgetType::Button, None, "Add shot to this scene");
        if add.clicked() && gesture_safe {
            state.new_cut(scene_id);
        }
    }
    // AU15: the cards and their status stand; the filters and exports fold.
    let more = plate::more_row(ui, &mut state.panel_more, plate::MORE_SHOTS, vocab::MENU_MORE);
    // Filters.
    if more {
    ui.horizontal_wrapped(|ui| {
        ui.label("status");
        let shown = state.board.status.map_or("any status", crate::vocab::cut_status);
        egui::ComboBox::from_id_salt("board_status_filter")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.board.status, None, "any status");
                for s in CutStatus::ALL {
                    let word = crate::vocab::cut_status(s);
                    ui.selectable_value(&mut state.board.status, Some(s), word)
                        .on_hover_text(status_card(word));
                }
            });
        ui.label("assignee");
        let shown = match state.board.assignee.as_deref() {
            None => "anyone".to_string(),
            Some("") => "nobody yet".to_string(),
            Some(a) => a.to_string(),
        };
        egui::ComboBox::from_id_salt("board_assignee_filter")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.board.assignee, None, "anyone");
                ui.selectable_value(&mut state.board.assignee, Some(String::new()), "nobody yet");
                for a in &assignees {
                    ui.selectable_value(&mut state.board.assignee, Some(a.clone()), a);
                }
            });
    });
    }
    // The count, and the exports — verbs the editor runs, as File ▸
    // Export does (D6); above the cards, so they never scroll away.
    let name = state.cut().name.clone();
    ui.horizontal_wrapped(|ui| {
        // AU15: a lone shot has its sentence above.
        if total > 1 {
            ui.label(format!("{} of {} shot(s)", cards.len(), total));
        }
        if !more {
            return;
        }
        if ui
            .button("export colour sheet")
            .on_hover_text(Action::ExportColorSheet.help())
            .clicked()
        {
            state.board.asked.push(Action::ExportColorSheet);
        }
        if ui
            .button(format!("export {name} as project"))
            .on_hover_text(format!(
                "{name}, its palettes and the project's settings as a new .animproj in the render \
                 folder (never over a file there: _2, _3 …)"
            ))
            .clicked()
        {
            state.board.asked.push(Action::ExportCutAsProject);
        }
    });
    ui.separator();
    let fps = state.engine.project.fps;
    let mut open: Option<CutId> = None;
    let scroll = egui::ScrollArea::vertical()
        .id_salt("board_cards")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for info in &cards {
                    let tex = state.board.thumbs.get(&info.at.cut).map(|(_, t)| t.clone());
                    // A card stacks top-down at one size (the row's wrapped,
                    // centred flow would spill a card's widgets past the
                    // board's edge).
                    let down = egui::Layout::top_down(egui::Align::Min);
                    let frame = ui
                        .allocate_ui_with_layout(egui::vec2(CARD_W, CARD_H), down, |ui| {
                            egui::Frame::group(ui.style())
                                .show(ui, |ui| {
                                    ui.set_width(CARD_W - 12.0);
                                    ui.set_min_height(CARD_H - 12.0);
                                    card(ui, state, info, tex, fps, &mut open);
                                })
                                .response
                        })
                        .inner;
                    // The status on the card's edge: Aka for a retake,
                    // Tally once done.
                    let edge = match info.prod.status {
                        Some(CutStatus::Retake) => Some(plate::RED),
                        Some(CutStatus::Done) => Some(plate::BLUE),
                        _ => None,
                    };
                    if let Some(ink) = edge {
                        ui.painter().rect_stroke(
                            frame.rect,
                            2.0,
                            egui::Stroke::new(2.0, ink),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            });
        });
    plate::note_viewport(ui, scroll.inner_rect, [false, true]);
    if let Some(cut) = open {
        state.goto_cut(scene_id, cut);
    }
}

/// The frame a card's note is about: the playhead's on the open cut, else
/// the cut's first (the note window can move it).
fn note_frame(state: &AppState, at: CutRef) -> u32 {
    if state.at() == at { state.view.frame } else { 0 }
}

/// A card's menu (its picture's and its open button's): a retake note on
/// the cut, and the cut as a project for a freelancer.
fn card_menu(r: &egui::Response, state: &mut AppState, info: &CardInfo) {
    r.context_menu(|ui| {
        plate::legend(ui, &info.name);
        if ui.button("add retake note…").clicked() {
            let frame = note_frame(state, info.at);
            state.board.open_note(info.at, frame);
            ui.close();
        }
        if ui
            .button(format!("export {} as project", info.name))
            .on_hover_text("the shot, its palettes and the project's settings as a new .animproj in the render folder")
            .clicked()
        {
            state.export_cut_project(info.at);
            ui.close();
        }
    });
}

fn card(
    ui: &mut egui::Ui,
    state: &mut AppState,
    info: &CardInfo,
    tex: Option<egui::TextureHandle>,
    fps: u32,
    open: &mut Option<CutId>,
) {
    let (at, name, p) = (info.at, &info.name, &info.prod);
    // The picture, in a slot of one size drawn or not (the cards never
    // jump as pictures land).
    let pw = CARD_W - 16.0;
    let project = &state.engine.project;
    let ph = (pw * project.height as f32 / project.width.max(1) as f32).clamp(24.0, 84.0);
    let (slot, pic) = ui.allocate_exact_size(egui::vec2(pw, ph), egui::Sense::click());
    match &tex {
        Some(t) => {
            let size = t.size_vec2();
            let s = (pw / size.x.max(1.0)).min(ph / size.y.max(1.0));
            egui::Image::new((t.id(), size * s)).paint_at(ui, egui::Rect::from_center_size(slot.center(), size * s));
        }
        None => {
            ui.painter().rect_filled(slot, 2.0, plate::WELL);
        }
    }
    plate::speak(&pic, egui::WidgetType::Button, None, &format!("{name} picture"));
    let pic = pic.on_hover_text("open this shot (right-click: a retake note, export it as a project)");
    if pic.clicked() {
        *open = Some(at.cut);
    }
    card_menu(&pic, state, info);
    let b = ui
        .button(format!("open {name}"))
        .on_hover_text("open this shot (right-click: a retake note, export it as a project)");
    if b.clicked() {
        *open = Some(at.cut);
    }
    card_menu(&b, state, info);
    ui.label(format!("{name} · {}", length_sk(info.frames, fps)));
    let shown = p.status.map_or("no status", crate::vocab::cut_status);
    let mut pick = p.status;
    egui::ComboBox::from_id_salt(("board_status", at.cut.0))
        .selected_text(shown)
        .width(CARD_W - 24.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut pick, None, "no status");
            for s in CutStatus::ALL {
                let word = crate::vocab::cut_status(s);
                ui.selectable_value(&mut pick, Some(s), word).on_hover_text(status_card(word));
            }
        });
    if pick != p.status {
        state.set_cut_status(at, pick);
    }
    let text = state
        .board
        .typing
        .entry(at.cut)
        .or_insert_with(|| p.assignee.clone());
    let r = plate::text_field(
        ui,
        egui::TextEdit::singleline(text)
            .hint_text("assignee")
            .desired_width(CARD_W - 24.0),
    );
    if r.lost_focus() {
        let who = state.board.typing.remove(&at.cut).unwrap_or_default();
        state.set_cut_assignee(at, &who);
    } else if !r.has_focus() && state.board.typing.get(&at.cut) != Some(&p.assignee) {
        // Someone else changed it (undo, a guest): show theirs.
        state.board.typing.insert(at.cut, p.assignee.clone());
    }
    // The notes: open ones in Aka; the menu resolves each and adds one.
    let open_notes = p.open_notes();
    let summary = if p.notes.is_empty() {
        "notes: none".to_string()
    } else {
        format!("notes: {open_notes} open of {}", p.notes.len())
    };
    let ink = if open_notes > 0 { plate::RED } else { plate::INK2 };
    let m = ui.menu_button(egui::RichText::new(&summary).color(ink), |ui| {
        plate::legend(ui, &format!("retake notes on {name}"));
        note_latches(ui, state, at, None);
        ui.separator();
        if ui.button("add retake note…").clicked() {
            let frame = note_frame(state, at);
            state.board.open_note(at, frame);
            ui.close();
        }
    });
    plate::speak(&m.response, egui::WidgetType::Button, None, &format!("{name} {summary}"));
    m.response.on_hover_text("the shot's retake notes: latch one resolved, or add one");
    // An animatic's panels become the cuts after it (the editor's verb,
    // on this cut).
    if info.panels
        && ui
            .button(format!("make shots from {name}'s panels"))
            .on_hover_text(Action::MakeCutsFromPanels.help())
            .clicked()
    {
        state.goto_cut(at.scene, at.cut);
        state.board.asked.push(Action::MakeCutsFromPanels);
    }
}

/// A note as its latch names it: "f6 cheek line breaks — Mika".
fn note_line(n: &RetakeNote) -> String {
    let mut text: String = n.text.chars().take(40).collect();
    if n.text.chars().count() > 40 {
        text.push('…');
    }
    let who = if n.author.is_empty() { "?" } else { &n.author };
    format!("f{} {text} — {who}", n.frame + 1)
}

/// The notes of cut `at` (only `frame`'s when Some), a latch each:
/// latched = resolved; a click resolves it or opens it again (one undo
/// step).
pub fn note_latches(ui: &mut egui::Ui, state: &mut AppState, at: CutRef, frame: Option<u32>) {
    let notes = state
        .engine
        .project
        .cut(at.scene, at.cut)
        .map(board::notes_of)
        .unwrap_or_default();
    let mut any = false;
    for (i, n) in notes.iter().enumerate() {
        if frame.is_some_and(|f| f != n.frame) {
            continue;
        }
        any = true;
        let mut done = n.resolved;
        if plate::latch(ui, &mut done, &note_line(n))
            .on_hover_text(format!("{} — latched: resolved; a click resolves it or opens it again", n.text))
            .changed()
        {
            state.set_note_resolved(at, i, done);
        }
    }
    if !any {
        ui.label(egui::RichText::new("no notes yet").color(plate::INK2));
    }
}

/// The retake note window, while a note is being written: the frame, what
/// to fix, and whether the cut goes to Retake with it. "add note" (or
/// Enter) adds it as one undo step; a refusal keeps the window and the
/// text.
pub fn note_window(ctx: &egui::Context, state: &mut AppState) {
    let Some(at) = state.board.note.as_ref().map(|f| f.at) else {
        return;
    };
    let Some((name, frames)) = state
        .engine
        .project
        .cut(at.scene, at.cut)
        .map(|c| (c.name.clone(), c.frame_count))
    else {
        // The cut is gone (undo, a guest): nothing to note.
        state.board.note = None;
        return;
    };
    let (mut open, mut add, mut cancel) = (true, false, false);
    let form = state.board.note.as_mut().expect("checked above");
    egui::Window::new("Retake note")
        .frame(plate::dialog_frame(&ctx.global_style()))
        .open(&mut open)
        .default_width(300.0)
        .collapsible(false)
        .show(ctx, |ui| {
            plate::surface(ui);
            plate::legend(ui, &format!("retake note on {name}"));
            ui.horizontal(|ui| {
                ui.label("frame");
                let mut shown = form.frame + 1;
                if plate::field(ui, egui::DragValue::new(&mut shown).range(1..=frames.max(1)))
                    .on_hover_text("the frame the note is about")
                    .changed()
                {
                    form.frame = shown.saturating_sub(1);
                }
            });
            let r = plate::text_field(
                ui,
                egui::TextEdit::singleline(&mut form.text)
                    .id(egui::Id::new(NOTE_ID))
                    .hint_text("what to fix")
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut form.focus) {
                r.request_focus();
            }
            let entered = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            plate::latch(ui, &mut form.retake, "set Retake")
                .on_hover_text("the shot's status goes to Retake with the note, in the same undo step");
            ui.horizontal(|ui| {
                let typed = !form.text.trim().is_empty();
                add = ui
                    .add_enabled(typed, egui::Button::new("add note"))
                    .on_hover_text("the note lands on the frame (one undo step); the sheet marks its row")
                    .on_disabled_hover_text("type what to fix first")
                    .clicked()
                    || (entered && typed);
                cancel = ui.button("cancel").clicked();
            });
        });
    if add {
        let f = state.board.note.take().expect("open");
        if !state.add_retake_note(f.at, f.frame, &f.text, f.retake) {
            // Refused: the window stays, with what was typed.
            state.board.note = Some(f);
        }
    } else if cancel || !open {
        state.board.note = None;
    }
}

/// The henshū timeline pane.
pub fn ui_episode(ui: &mut egui::Ui, state: &mut AppState) {
    let scene_id = state.at().scene;
    let Some(scene) = state.engine.project.scene(scene_id) else {
        ui.label("no scene");
        return;
    };
    // The blocks, read once (no cut is cloned for them).
    let blocks: Vec<(CutId, String, u32)> = scene
        .cuts
        .iter()
        .map(|c| (c.id, c.name.clone(), c.frame_count))
        .collect();
    let fps = state.engine.project.fps;
    let trt = board::trt_frames(scene);
    let more = state.panel_more.is_open(plate::MORE_EPISODE);
    plate::legend(ui, vocab::EPISODE_TIMELINE)
        .on_hover_text(vocab::with_studio_word(vocab::EPISODE_TIMELINE, "The scene's shots, end to end."));
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("TRT {}", board::format_trt(trt, fps)));
        match board::episode_target(&state.engine.project) {
            Some(target) => {
                let mark = board::trt_mark(trt, target);
                let (word, color) = match mark {
                    TrtMark::Over => ("over", plate::RED),
                    TrtMark::Under => ("under", plate::INK3),
                    TrtMark::OnTarget => ("on target", plate::BLUE),
                };
                ui.label(format!("target {}", board::format_trt(target, fps)));
                ui.colored_label(
                    color,
                    format!("{word} {}", board::format_trt_diff(trt, target, fps)),
                );
            }
            None => {
                ui.label("no target");
            }
        }
        let r = plate::text_field(
            ui,
            egui::TextEdit::singleline(&mut state.board.target)
                .hint_text("MM:SS+FF")
                .desired_width(110.0),
        );
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let typed = std::mem::take(&mut state.board.target);
            if typed.trim().is_empty() {
                state.set_episode_target(None);
            } else {
                match board::parse_trt(&typed, fps) {
                    Some(f) => state.set_episode_target(Some(f)),
                    None => state.refuse("refused — type the target as MM:SS+FF"),
                }
            }
        }
    });
    // The verbs the editor runs (D6); the play button keeps one width
    // as it turns to "stop", so the row never jumps under the pointer.
    let width = ui.available_width().max(60.0);
    let fit = ((width - blocks.len() as f32) / trt.max(1) as f32).max(0.01);
    ui.horizontal_wrapped(|ui| {
        let playing = state.board.episode_play && state.view.playing;
        if ui
            .add_sized([108.0, 20.0], egui::Button::new(if playing { "stop episode" } else { "play episode" }))
            .on_hover_text(Action::PlayEpisode.help())
            .clicked()
        {
            state.board.asked.push(Action::PlayEpisode);
        }
        // AU15: the EDL folds, per spec 7.1: Timeline and Play stand.
        plate::more_tag(ui, &mut state.panel_more, plate::MORE_EPISODE, vocab::MENU_MORE);
        let ext = state.edl_ext();
        if more
            && ui
                .button("export EDL")
                .on_hover_text(format!(
                    "CMX3600, non-drop, record from 01:00:00:00, into the render folder; each event \
                     names its cut's .{ext} render (a frame-sequence preset falls back to .mov)"
                ))
                .clicked()
        {
            state.board.asked.push(Action::ExportEdl);
        }
        if ui
            .button("render episode")
            .on_hover_text(Action::RenderEpisode.help())
            .clicked()
        {
            state.board.asked.push(Action::RenderEpisode);
        }
        ui.separator();
        // Zoom: points per frame; "fit" shows the whole episode.
        let now = state.board.zoom.unwrap_or(fit);
        for (label, spoken, to) in [
            ("−", "timeline zoom out", Some(now / 1.5)),
            ("+", "timeline zoom in", Some(now * 1.5)),
            ("fit", "timeline zoom to fit", None),
        ] {
            let r = ui.button(label);
            plate::speak(&r, egui::WidgetType::Button, None, spoken);
            if r.on_hover_text(spoken).clicked() {
                state.board.zoom = to.map(|z| z.clamp(0.05, 48.0));
            }
        }
    });
    ui.separator();
    // The blocks: widths in proportion to each cut's length; the open
    // cut's carries the playhead.
    let per_frame = state.board.zoom.unwrap_or(fit);
    let mut rects = Vec::with_capacity(blocks.len());
    let mut open: Option<CutId> = None;
    let mut dropped: Option<(usize, f32)> = None;
    let (current, frame) = (state.at().cut, state.view.frame);
    let scroll = egui::ScrollArea::horizontal().id_salt("episode_blocks").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            for (i, (id, name, frames)) in blocks.iter().enumerate() {
                let w = (*frames as f32 * per_frame).max(18.0);
                let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 44.0), egui::Sense::click_and_drag());
                let r = r.on_hover_text(format!("{name} · {} — drag to move it", length_sk(*frames, fps)));
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
                let painter = ui.painter();
                let (fill, ink) = if *id == current {
                    (plate::BLUE_WASH, plate::BLUE_DEEP)
                } else {
                    (plate::WELL, plate::INK)
                };
                painter.rect_filled(rect, plate::R_SHEET, fill);
                if state.board.drag == Some(i) {
                    painter.rect_stroke(
                        rect,
                        plate::R_SHEET,
                        egui::Stroke::new(2.0, plate::BLUE),
                        egui::StrokeKind::Inside,
                    );
                }
                // The name, clipped to its block (a short cut's never
                // runs over the next).
                painter.with_clip_rect(rect.shrink(2.0)).text(
                    rect.left_top() + egui::vec2(4.0, 4.0),
                    egui::Align2::LEFT_TOP,
                    name,
                    crate::plate::Role::Small.font(),
                    ink,
                );
                if *id == current && *frames > 0 {
                    let x = rect.left() + rect.width() * frame.min(*frames) as f32 / *frames as f32;
                    let x = x.clamp(rect.left() + 1.0, rect.right() - 1.0);
                    painter.line_segment(
                        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                        egui::Stroke::new(2.0, plate::BLUE),
                    );
                }
                if r.clicked() {
                    open = Some(*id);
                }
                if r.drag_started() {
                    state.board.drag = Some(i);
                }
                if r.drag_stopped()
                    && let Some(from) = state.board.drag.take()
                    && let Some(p) = r.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())
                {
                    dropped = Some((from, p.x));
                }
                rects.push(rect);
            }
        });
    });
    plate::note_viewport(ui, scroll.inner_rect, [true, false]);
    if let Some((from, x)) = dropped {
        let to = rects
            .iter()
            .position(|r| x < r.center().x)
            .map_or(rects.len().saturating_sub(1), |i| if i > from { i - 1 } else { i });
        state.move_cut(scene_id, from, to.min(rects.len().saturating_sub(1)));
    }
    if let Some(cut) = open {
        state.goto_cut(scene_id, cut);
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;

    use anim_core::command::{Command, CutRef};
    use anim_core::deliver::{self, Preset};

    use super::*;
    use crate::render_queue::{self, JobSpec};
    use crate::testkit::EditorHarness;
    use crate::workspace::{Pane, Stage};

    /// A 64×36 project whose scene has CUT01, CUT02 and CUT03 of 24, 36
    /// and 48 frames, CUT01 current.
    fn three() -> (AppState, Vec<CutRef>) {
        let mut s = AppState::new_project("show", 64, 36, 24, 72.0);
        let scene = s.at().scene;
        s.new_cut(scene);
        s.new_cut(scene);
        let cuts: Vec<CutRef> = s
            .engine
            .project
            .scene(scene)
            .unwrap()
            .cuts
            .iter()
            .map(|c| CutRef { scene, cut: c.id })
            .collect();
        for (at, frames) in cuts.iter().zip([24, 36, 48]) {
            s.engine
                .apply("len", vec![Command::SetFrameCount { at: *at, frames }])
                .unwrap();
        }
        s.goto_cut(scene, cuts[0].cut);
        s.engine.clear_history();
        (s, cuts)
    }

    fn names(s: &AppState) -> Vec<String> {
        let scene = s.at().scene;
        s.engine.project.scene(scene).unwrap().cuts.iter().map(|c| c.name.clone()).collect()
    }

    /// A solid tile of `v` (f16 bits, every channel) over frame 0 of cut
    /// `at`, on the cel its first column shows there (made if blank).
    fn ink(s: &mut AppState, at: CutRef, v: u16) {
        use anim_core::raster::{TILE_LEN, TileData};
        s.goto_cut(at.scene, at.cut);
        s.goto(0);
        let slot = s.active_slot_resolved().expect("an armed layer");
        let tile = std::sync::Arc::new(TileData::from_vec(vec![v; TILE_LEN]));
        s.commit_raster(vec![((0, 0), tile)], slot).expect("inked");
    }

    /// CUT01 as an animatic: 108 frames and a BOARD column (a Ref column,
    /// the active one) with panels keyed at frames 0, 36 and 60.
    fn animatic(s: &mut AppState) -> CutRef {
        let at = s.at();
        s.engine
            .apply("len", vec![Command::SetFrameCount { at, frames: 108 }])
            .unwrap();
        let board = s.engine.add_column(at, "BOARD").unwrap();
        s.view.active_column = board;
        s.set_column_kind(anim_core::xsheet::ColumnKind::Ref);
        for f in [0, 36, 60] {
            s.goto(f);
            s.new_drawing_at_frame();
        }
        s.goto(0);
        assert_eq!(anim_core::board::panels(s.cut(), board).len(), 3);
        at
    }

    /// Run `refresh_thumbs` for every cut of the scene until each card's
    /// picture is the one its key asks for (the worker is a thread).
    fn settle(ctx: &egui::Context, s: &mut AppState) {
        let scene = s.at().scene;
        let shown: Vec<CutId> = s.engine.project.scene(scene).unwrap().cuts.iter().map(|c| c.id).collect();
        for _ in 0..400 {
            refresh_thumbs(ctx, s, scene, &shown, true);
            let b = &s.board;
            let fresh = shown
                .iter()
                .all(|c| b.thumbs.get(c).map(|(k, _)| *k) == b.keys.get(c).copied());
            if b.job.is_none() && fresh {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the card pictures never landed");
    }

    fn pngs(dir: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "png"))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn cutboard_panes_are_tabs_of_the_edit_room() {
        let tabs: Vec<Pane> = Stage::Edit.dock().iter_all_tabs().map(|(_, p)| *p).collect();
        assert!(tabs.contains(&Pane::Board) && tabs.contains(&Pane::Episode), "{tabs:?}");
        assert!(Pane::ALL.contains(&Pane::Board) && Pane::ALL.contains(&Pane::Episode));
        assert_eq!(Pane::Board.title(), crate::vocab::SHOTS);
        assert_eq!(Pane::Episode.title(), crate::vocab::EPISODE_TIMELINE);
        let mut h = EditorHarness::new();
        h.perform(crate::config::Action::RoomEdit);
        h.run_frames(2);
        assert!(h.editor().dock.find_tab(&Pane::Board).is_some());
        assert!(h.editor().dock.find_tab(&Pane::Episode).is_some());
    }

    #[test]
    fn cutboard_shows_a_card_per_cut_and_a_click_opens_it() {
        let (s, cuts) = three();
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Board]);
        h.run_frames(6);
        for n in ["CUT01", "CUT02", "CUT03"] {
            assert!(h.has(&format!("open {n}")), "{n}: {}", h.describe());
        }
        assert!(h.has_text_containing("CUT02 · 1+12"), "the length in s+k");
        h.click("open CUT02");
        h.run_frames(2);
        assert_eq!(h.state().at().cut, cuts[1].cut, "the card opened its cut");
    }

    #[test]
    fn cutboard_status_and_assignee_are_each_one_undo_step_and_show_on_the_card() {
        let (s, cuts) = three();
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Board]);
        h.state_mut().set_cut_status(cuts[1], Some(CutStatus::Genga));
        h.state_mut().set_cut_assignee(cuts[1], "Mori");
        h.run_frames(3);
        let cut = |h: &EditorHarness| {
            ProdInfo::of(h.state().engine.project.cut(cuts[1].scene, cuts[1].cut).unwrap())
        };
        assert_eq!((cut(&h).status, cut(&h).assignee), (Some(CutStatus::Genga), "Mori".to_string()));
        // The card's combo shows it and edits it: one more undo step.
        h.click_combo(crate::vocab::cut_status(CutStatus::Genga));
        h.click(crate::vocab::cut_status(CutStatus::Done));
        h.run_frames(2);
        assert_eq!(cut(&h).status, Some(CutStatus::Done));
        h.state_mut().engine.undo().unwrap();
        assert_eq!(cut(&h).status, Some(CutStatus::Genga));
        h.state_mut().engine.undo().unwrap();
        assert_eq!((cut(&h).status, cut(&h).assignee), (Some(CutStatus::Genga), String::new()));
        h.state_mut().engine.undo().unwrap();
        assert_eq!(cut(&h), ProdInfo::default());
    }

    #[test]
    fn cutboard_filter_shows_exactly_the_matching_cards() {
        let (mut s, cuts) = three();
        s.set_cut_status(cuts[1], Some(CutStatus::Paint));
        s.set_cut_status(cuts[2], Some(CutStatus::Done));
        s.board.status = Some(CutStatus::Paint);
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Board]);
        h.run_frames(4);
        assert!(h.has("open CUT02"));
        assert!(!h.has("open CUT01") && !h.has("open CUT03"), "{}", h.describe());
        assert!(h.has_text_containing("1 of 3 shot(s)"));
    }

    #[test]
    fn cutboard_episode_drag_reorders_as_one_undo_step_and_shows_the_trt() {
        let (mut s, _) = three();
        s.set_episode_target(Some(120));
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Episode]);
        h.run_frames(3);
        assert!(h.has_text_containing("TRT 00:04+12"), "{}", h.describe());
        assert!(h.has_text_containing("under -00:00+12"));
        let from = h.rect("CUT01").center();
        let to = h.rect("CUT03").right_center() + egui::vec2(-2.0, 0.0);
        h.drag(from, to, 8);
        h.run_frames(2);
        assert_eq!(names(h.state()), ["CUT02", "CUT03", "CUT01"]);
        h.state_mut().engine.undo().unwrap();
        assert_eq!(names(h.state()), ["CUT01", "CUT02", "CUT03"], "one undo puts it back");
    }

    #[test]
    fn cutboard_play_episode_runs_on_into_the_next_cut_and_stops_after_the_last() {
        let (s, cuts) = three();
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Episode]);
        h.run_frames(2);
        h.click("play episode");
        assert!(h.state().board.episode_play && h.state().view.playing);
        let mut seen = vec![h.state().at().cut];
        for _ in 0..400 {
            h.editor_mut().play_tick(1.0 / 24.0);
            let c = h.state().at().cut;
            if seen.last() != Some(&c) {
                seen.push(c);
            }
            if !h.state().view.playing {
                break;
            }
        }
        assert_eq!(seen, cuts.iter().map(|c| c.cut).collect::<Vec<_>>());
        assert!(!h.state().view.playing && !h.state().board.episode_play);
    }

    #[test]
    fn cutboard_episode_job_writes_every_cut_frame_for_frame() {
        let (mut s, cuts) = three();
        // Each cut inked its own way: a frame from the wrong cut shows.
        for (at, v) in cuts.iter().zip([0x3C00, 0x3800, 0x3400]) {
            ink(&mut s, *at, v);
        }
        s.goto_cut(cuts[0].scene, cuts[0].cut);
        let dir = tempfile::tempdir().unwrap();
        s.render_queue.draft.out = dir.path().join("q").display().to_string();
        assert!(render_queue::add_episode_job(&mut s, cuts[0].scene));
        let job = s.render_queue.queue.jobs.last().unwrap().clone();
        assert!(job.episode && job.label.contains("108 frames"), "{}", job.label);

        let p = Preset::named("PNG 8-bit").unwrap();
        let cancel = AtomicBool::new(false);
        let ep_dir = dir.path().join("ep");
        let spec = JobSpec::episode(
            &s.engine.project,
            cuts[0].scene,
            p,
            p.burn_ins,
            ep_dir.clone(),
            deliver::DEFAULT_PATTERN.into(),
            "mika".into(),
        )
        .unwrap();
        assert_eq!(render_queue::render_job(&spec, None, &cancel, &mut |_| {}).unwrap(), 108);
        let ours = pngs(&ep_dir);
        assert_eq!(ours.len(), 108);
        assert!(ours[0].to_string_lossy().contains("episode_0001"), "{}", ours[0].display());
        // The cuts' first frames (episode frames 0, 24 and 60) differ.
        let first = |i: usize| std::fs::read(&ours[i]).unwrap();
        assert_ne!(first(0), first(24), "CUT01 and CUT02");
        assert_ne!(first(24), first(60), "CUT02 and CUT03");
        assert_ne!(first(0), first(60), "CUT01 and CUT03");
        // The same frames, cut by cut.
        let mut theirs = Vec::new();
        for at in &cuts {
            let out = dir.path().join(format!("cut{}", at.cut.0));
            let spec = JobSpec::new(
                &s.engine.project,
                *at,
                None,
                p,
                p.burn_ins,
                out.clone(),
                deliver::DEFAULT_PATTERN.into(),
                "mika".into(),
            )
            .unwrap();
            render_queue::render_job(&spec, None, &cancel, &mut |_| {}).unwrap();
            theirs.extend(pngs(&out));
        }
        assert_eq!(theirs.len(), 108);
        for (a, b) in ours.iter().zip(&theirs) {
            assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap(), "{}", a.display());
        }
    }

    #[test]
    fn cutboard_exports_write_an_edl_colour_sheets_and_a_cut_project() {
        let (mut s, cuts) = three();
        let dir = tempfile::tempdir().unwrap();
        let edl = dir.path().join("ep.edl");
        assert!(s.export_edl(cuts[0].scene, &edl));
        let text = std::fs::read_to_string(&edl).unwrap();
        assert!(text.contains("FCM: NON-DROP FRAME") && text.contains("\n003  AX"), "{text}");
        s.palettes.characters.push(crate::palette::CharacterPalette {
            name: "Hana".into(),
            roles: vec![crate::palette::ColorRole {
                name: "hada kage".into(),
                color: [200, 150, 140, 255],
            }],
            variants: vec![],
        });
        assert_eq!(s.export_color_sheets(&dir.path().join("iro")), 1);
        let file = std::fs::File::open(dir.path().join("iro").join("iro_Hana.png")).unwrap();
        let mut reader = png::Decoder::new(std::io::BufReader::new(file)).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        let l = board::color_sheet_layout(&crate::palette::sheet_palette(&s.palettes.characters[0]));
        let (x, y) = l.rows[0].swatches[0].center();
        let ch = info.line_size / info.width as usize;
        let i = y as usize * info.line_size + x as usize * ch;
        assert_eq!(&buf[i..i + 3], &[200, 150, 140], "the chip as stored");
        let proj = dir.path().join("c2.animproj");
        assert!(s.export_cut_as_project(cuts[1], &proj));
        let back = anim_core::store::load(&proj).unwrap();
        assert_eq!(back.scenes.len(), 1);
        assert_eq!(back.scenes[0].cuts.len(), 1);
        assert_eq!(back.scenes[0].cuts[0].name, "CUT02");
        assert_eq!(names(&s).len(), 3, "the source keeps its cuts");
    }

    #[test]
    fn cutboard_verbs_live_in_the_registry_with_menus_aliases_and_chords() {
        use crate::config::{Keymap, default_binds};
        let verbs = [
            Action::ShowCutBoard,
            Action::ShowEpisode,
            Action::PlayEpisode,
            Action::ExportEdl,
            Action::ExportColorSheet,
            Action::ExportCutAsProject,
            Action::MakeCutsFromPanels,
            Action::RenderEpisode,
            Action::AddRetakeNote,
        ];
        for a in verbs {
            assert!(Action::ALL.contains(&a), "{a:?}");
            assert!((1..=120).contains(&a.help().len()), "{a:?}: {}", a.help());
            assert!(a.menu().is_some() && !a.aliases().is_empty(), "{a:?}");
            for km in Keymap::ALL {
                let binds = default_binds(*km);
                assert!(crate::cmdpal::chord_of(&binds, a).is_some(), "{a:?} in {km:?}");
            }
        }
        // The palette finds them by the studio's words, typed without the
        // macrons ("henshu" alone is the edit room's too).
        for (q, a) in [
            ("shinko", Action::ShowCutBoard),
            ("henshu timeline", Action::ShowEpisode),
            ("cmx3600", Action::ExportEdl),
            ("iro shitei", Action::ExportColorSheet),
            ("freelancer", Action::ExportCutAsProject),
            ("add retake note", Action::AddRetakeNote),
            ("render episode", Action::RenderEpisode),
        ] {
            assert_eq!(crate::cmdpal::ranked(q, &[]).first(), Some(&a), "{q}");
        }
        // Window › the two panes dock where they are not.
        let mut h = EditorHarness::new();
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::XSheet]);
        h.run();
        h.perform(Action::ShowCutBoard);
        h.perform(Action::ShowEpisode);
        assert!(h.editor().dock.find_tab(&Pane::Board).is_some());
        assert!(h.editor().dock.find_tab(&Pane::Episode).is_some());
    }

    #[test]
    fn cutboard_pane_buttons_run_their_verbs_through_the_editor() {
        let (mut s, cuts) = three();
        let dir = tempfile::tempdir().unwrap();
        s.render_queue.draft.out = dir.path().display().to_string();
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Episode]);
        h.run();
        // Render episode queues the job and brings the queue forward.
        h.click("render episode");
        let s = h.state();
        assert!(s.render_queue.queue.jobs.last().is_some_and(|j| j.episode));
        assert_eq!(s.status, "episode SC01 queued: 108 frames — see the Export queue");
        assert!(h.editor().dock.find_tab(&Pane::RenderQueue).is_some());
        // Export EDL writes <project>_<scene>.edl into the render folder.
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Episode]);
        h.run();
        h.click("export EDL");
        let edl = std::fs::read_to_string(dir.path().join("show_SC01.edl")).unwrap();
        assert!(edl.contains("\n003  AX"), "{edl}");
        // Play episode is the verb's toggle; the button keeps its width.
        let w = h.rect("play episode").width();
        h.click("play episode");
        assert!(h.state().board.episode_play && h.state().view.playing);
        assert_eq!(h.rect("stop episode").width(), w);
        h.click("stop episode");
        assert!(!h.state().view.playing && !h.state().board.episode_play);
        assert_eq!(h.state().at().cut, cuts[0].cut);
    }

    #[test]
    fn cutboard_card_pictures_are_keyed_on_their_own_cut_and_drawn_on_a_worker() {
        let ctx = egui::Context::default();
        let (mut s, cuts) = three();
        ink(&mut s, cuts[1], 0x3C00);
        s.goto_cut(cuts[0].scene, cuts[0].cut);
        let scene = cuts[0].scene;
        let shown: Vec<CutId> = cuts.iter().map(|c| c.cut).collect();
        // Mid-stroke nothing is read or started (D7).
        refresh_thumbs(&ctx, &mut s, scene, &shown, false);
        assert!(s.board.job.is_none() && s.board.keyed.is_none());
        // Otherwise the picture is drawn off the UI thread: it lands later.
        refresh_thumbs(&ctx, &mut s, scene, &shown, true);
        assert!(s.board.job.is_some() && s.board.thumbs.is_empty());
        settle(&ctx, &mut s);
        let held = |s: &AppState| cuts.iter().map(|c| s.board.thumbs[&c.cut].0).collect::<Vec<u64>>();
        let before = held(&s);
        // A status is no picture change; ink on CUT03 redraws only CUT03.
        s.set_cut_status(cuts[0], Some(CutStatus::Genga));
        ink(&mut s, cuts[2], 0x3800);
        s.goto_cut(scene, cuts[0].cut);
        settle(&ctx, &mut s);
        let after = held(&s);
        assert_eq!((after[0], after[1]), (before[0], before[1]));
        assert_ne!(after[2], before[2]);
        // A removed cut's picture goes with it.
        s.engine
            .apply("rm", vec![Command::RemoveCut { at: cuts[2] }])
            .unwrap();
        settle(&ctx, &mut s);
        assert!(!s.board.thumbs.contains_key(&cuts[2].cut));
        assert_eq!(s.board.thumbs.len(), 2);
    }

    #[test]
    fn cutboard_card_notes_resolve_from_its_menu_and_the_note_window_adds_one() {
        let (mut s, cuts) = three();
        s.render_queue.artist = "Mika".into();
        assert!(s.add_retake_note(cuts[1], 4, "cheek line breaks", true));
        assert!(s.add_retake_note(cuts[1], 9, "hair colour", false));
        let mut h = EditorHarness::with_state(s);
        // The shell hands the session's user name on as the author.
        h.rig_mut().config.session.username = "Mika".into();
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Board]);
        h.run();
        assert!(h.has("CUT01 notes: none"), "{}", h.describe());
        // The card's notes: a latch each; one click resolves one (one step).
        h.click("CUT02 notes: 2 open of 2");
        assert_eq!(h.node_toggled("f5 cheek line breaks — Mika"), Some(false));
        let steps = h.state().engine.position();
        h.click("f5 cheek line breaks — Mika");
        let notes = |h: &EditorHarness| {
            board::notes_of(h.state().engine.project.cut(cuts[1].scene, cuts[1].cut).unwrap())
        };
        assert!(notes(&h)[0].resolved && !notes(&h)[1].resolved);
        assert_eq!(h.state().engine.position(), steps + 1);
        h.run();
        // "add retake note…" (the menu again) opens the window on the
        // card's cut.
        h.click("CUT02 notes: 1 open of 2");
        h.click("add retake note…");
        let form = h.state().board.note.as_ref().expect("the note window");
        assert_eq!((form.at, form.frame, form.retake), (cuts[1], 0, true));
        h.type_text("timing is off");
        h.click("add note");
        assert!(h.state().board.note.is_none(), "added: the window closes");
        let n = notes(&h);
        assert_eq!(n.len(), 3);
        assert_eq!((n[0].frame, n[0].text.as_str(), n[0].author.as_str()), (0, "timing is off", "Mika"));
        let prod = ProdInfo::of(h.state().engine.project.cut(cuts[1].scene, cuts[1].cut).unwrap());
        assert_eq!(prod.status, Some(CutStatus::Retake));
        h.run();
        assert!(h.has("CUT02 notes: 2 open of 3"), "{}", h.describe());
    }

    #[test]
    fn cutboard_make_cuts_lives_on_the_animatic_card_and_is_refused_elsewhere() {
        // No animatic: no card offers it, and the verb is refused.
        let (s, _) = three();
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Board]);
        h.run();
        assert!(!h.has_text_containing("make shots from"), "{}", h.describe());
        h.perform(Action::MakeCutsFromPanels);
        assert_eq!(h.state().refusal, "refused — open the animatic (a shot with a BOARD track) first");
        assert_eq!(names(h.state()).len(), 3);
        // CUT01 an animatic: its card names the source, and makes the cuts
        // after it under the free numbers (CUT04 on) as one undo step.
        let at = animatic(h.state_mut());
        let second = h.state().engine.project.scene(at.scene).unwrap().cuts[1].id;
        h.state_mut().goto_cut(at.scene, second);
        h.run();
        h.click("make shots from CUT01's panels");
        assert_eq!(h.state().status, "made 3 shot(s) from CUT01's panels");
        assert_eq!(names(h.state()), ["CUT01", "CUT04", "CUT05", "CUT06", "CUT02", "CUT03"]);
        h.state_mut().engine.undo().unwrap();
        assert_eq!(names(h.state()), ["CUT01", "CUT02", "CUT03"]);
    }

    #[test]
    fn cutboard_export_as_project_from_a_cards_menu_never_writes_over_a_file() {
        let (mut s, cuts) = three();
        let dir = tempfile::tempdir().unwrap();
        s.render_queue.draft.out = dir.path().display().to_string();
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Board]);
        h.run();
        // The board's own button names the open cut.
        assert!(h.has("export CUT01 as project"), "{}", h.describe());
        // A card's menu exports its cut; again, beside the first.
        for _ in 0..2 {
            h.right_click_pos(h.rect("open CUT02").center());
            h.click("export CUT02 as project");
        }
        let first = anim_core::store::load(&dir.path().join("show_CUT02.animproj")).unwrap();
        let second = anim_core::store::load(&dir.path().join("show_CUT02_2.animproj")).unwrap();
        assert_eq!(first.scenes[0].cuts[0].name, "CUT02");
        assert_eq!(second.scenes[0].cuts[0].name, "CUT02");
        assert_eq!(h.state().at().cut, cuts[0].cut, "the open cut stays");
    }

    #[test]
    fn cutboard_play_episode_passes_a_cut_with_no_columns_and_never_loops_on_it() {
        let (mut s, cuts) = three();
        // CUT03 has no columns yet: nothing to show, so it is passed.
        let col = s.engine.project.cut(cuts[2].scene, cuts[2].cut).unwrap().xsheet.columns[0].id;
        s.engine
            .apply("rm", vec![Command::RemoveColumn { at: cuts[2], id: col }])
            .unwrap();
        let mut h = EditorHarness::with_state(s);
        h.perform(Action::PlayEpisode);
        let mut seen = vec![h.state().at().cut];
        let mut ticks = 0;
        while h.state().view.playing && ticks < 400 {
            h.editor_mut().play_tick(1.0 / 24.0);
            ticks += 1;
            let c = h.state().at().cut;
            if seen.last() != Some(&c) {
                seen.push(c);
            }
        }
        assert_eq!(seen, [cuts[0].cut, cuts[1].cut], "CUT03 is passed over");
        assert!(ticks < 100, "it stops after CUT02 ({ticks} ticks)");
        assert!(!h.state().board.episode_play);
    }
}

/// GM15's test minimums, counted from the source (the gate's name filter
/// also counts any test whose path merely holds the word): 10 tests in
/// anim-core's board_tests.rs and 6 cutboard_* app tests.
#[cfg(test)]
mod gm15_minimums {
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
        let core = tests_named(include_str!("../../crates/anim-core/tests/board_tests.rs"), "");
        assert!(core >= 10, "board_tests.rs holds {core} tests");
        let app = tests_named(include_str!("board_panel.rs"), "cutboard_")
            + tests_named(include_str!("xsheet_panel.rs"), "cutboard_");
        assert!(app >= 6, "{app} cutboard_* tests");
    }
}
