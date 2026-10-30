//! GM15 EDITORIAL (編集): the cut board — a card per cut of the scene
//! with its picture, number, length, status, assignee and open notes —
//! and the episode timeline, the cuts as blocks in order (drag one to
//! move it), with the running time (TRT) against the episode's target.
//! Every edit goes through AppState as one undo step (D1); a card's
//! picture is a CPU render of frame 0, at most one stale card per frame.

use std::collections::HashMap;

use anim_core::board::{self, CutStatus, ProdInfo, TrtMark};
use anim_core::command::CutRef;
use anim_core::deliver::{self, Purpose, RenderOpts};
use anim_core::ids::CutId;
use anim_core::model::Cut;
use eframe::egui;

use crate::doc::AppState;
use crate::plate;

/// A card's picture width in points.
pub const THUMB_W: usize = 128;
const CARD_W: f32 = 150.0;

#[derive(Default)]
pub struct BoardUi {
    /// The board's filters: None = any status / anyone; Some("") = nobody.
    pub status: Option<CutStatus>,
    pub assignee: Option<String>,
    /// An assignee being typed, per cut (lands on Enter or focus out).
    typing: HashMap<CutId, String>,
    /// Card pictures and the document generation each was drawn at.
    thumbs: HashMap<CutId, (u64, egui::TextureHandle)>,
    /// The episode block being dragged (its index in the scene).
    pub drag: Option<usize>,
    /// The target TRT being typed.
    target: String,
    /// GM15: Play episode — at a cut's end playback runs on into the next.
    pub episode_play: bool,
}

/// A cut's length as seconds + koma ("1+12" = a second and 12 frames).
pub fn length_sk(frames: u32, fps: u32) -> String {
    let fps = fps.max(1);
    format!("{}+{:02}", frames / fps, frames % fps)
}

/// Frame 0 of `cut`, small, laid on paper for the card.
fn thumbnail(state: &AppState, cut: &Cut) -> egui::ColorImage {
    let p = &state.engine.project;
    let opts = RenderOpts {
        purpose: Purpose::View,
        scale: 0.5,
    };
    let img = deliver::render(cut, p.color_law, 0, p.width, p.height, &opts);
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

/// The cut board pane.
pub fn ui_board(ui: &mut egui::Ui, state: &mut AppState) {
    let scene_id = state.at().scene;
    let Some(scene) = state.engine.project.scene(scene_id).cloned() else {
        ui.label("no scene");
        return;
    };
    plate::legend(ui, "cut board");
    // Filters.
    let mut assignees: Vec<String> = scene
        .cuts
        .iter()
        .map(|c| ProdInfo::of(c).assignee)
        .filter(|a| !a.is_empty())
        .collect();
    assignees.sort();
    assignees.dedup();
    ui.horizontal_wrapped(|ui| {
        ui.label("status");
        let shown = state.board.status.map_or("any status", |s| s.label());
        egui::ComboBox::from_id_salt("board_status_filter")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.board.status, None, "any status");
                for s in CutStatus::ALL {
                    ui.selectable_value(&mut state.board.status, Some(s), s.label());
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
    let shown = board::filter_cuts(&scene, state.board.status, state.board.assignee.as_deref());
    ui.label(format!("{} of {} cut(s)", shown.len(), scene.cuts.len()));
    ui.separator();
    let fps = state.engine.project.fps;
    let mut drew_one = false;
    let mut open: Option<CutId> = None;
    egui::ScrollArea::vertical()
        .id_salt("board_cards")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for i in shown {
                    let cut = &scene.cuts[i];
                    let at = CutRef {
                        scene: scene_id,
                        cut: cut.id,
                    };
                    // One stale picture per frame (each is a CPU render).
                    let fresh = state
                        .board
                        .thumbs
                        .get(&cut.id)
                        .is_some_and(|(g, _)| *g == state.doc_gen);
                    if !fresh && !drew_one {
                        drew_one = true;
                        let img = thumbnail(state, cut);
                        let tex = ui.ctx().load_texture(
                            format!("board_thumb_{}", cut.id.0),
                            img,
                            egui::TextureOptions::LINEAR,
                        );
                        state.board.thumbs.insert(cut.id, (state.doc_gen, tex));
                    } else if !fresh {
                        ui.ctx().request_repaint();
                    }
                    let tex = state.board.thumbs.get(&cut.id).map(|(_, t)| t.clone());
                    ui.allocate_ui(egui::vec2(CARD_W, 210.0), |ui| {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_width(CARD_W - 12.0);
                            card(ui, state, at, cut, tex, fps, &mut open);
                        });
                    });
                }
            });
        });
    if let Some(cut) = open {
        state.goto_cut(scene_id, cut);
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("make cuts from panels")
            .on_hover_text(
                "an animatic's BOARD panels become cuts, one per panel cut number, \
                 the pictures on a Ref column — one undo step",
            )
            .clicked()
        {
            state.make_cuts_from_panels();
        }
        if ui
            .button("export colour sheet")
            .on_hover_text("iro shitei: one PNG per character palette, into the render folder")
            .clicked()
        {
            match state.render_queue.out_dir(state.file_path.as_deref()) {
                Some(dir) => {
                    state.export_color_sheets(&dir);
                }
                None => state.refuse("refused — type a render folder first, or save the project"),
            }
        }
        if ui
            .button("export cut as project")
            .on_hover_text("this cut, its palettes and the project's settings as a new .animproj")
            .clicked()
        {
            let at = state.at();
            match state.render_queue.out_dir(state.file_path.as_deref()) {
                Some(dir) => {
                    let name = format!(
                        "{}_{}.animproj",
                        deliver::sanitize(&state.engine.project.name),
                        deliver::sanitize(&state.cut().name)
                    );
                    let _ = std::fs::create_dir_all(&dir);
                    state.export_cut_as_project(at, &dir.join(name));
                }
                None => state.refuse("refused — type a render folder first, or save the project"),
            }
        }
    });
}

fn card(
    ui: &mut egui::Ui,
    state: &mut AppState,
    at: CutRef,
    cut: &Cut,
    tex: Option<egui::TextureHandle>,
    fps: u32,
    open: &mut Option<CutId>,
) {
    let p = ProdInfo::of(cut);
    match tex {
        Some(t) => {
            let size = t.size_vec2() * (CARD_W - 16.0) / t.size_vec2().x.max(1.0);
            let r = ui.add(egui::Image::new((t.id(), size)).sense(egui::Sense::click()));
            if r.clicked() {
                *open = Some(cut.id);
            }
        }
        None => {
            ui.add_space(72.0);
        }
    }
    if ui
        .button(format!("open {}", cut.name))
        .on_hover_text("open this cut")
        .clicked()
    {
        *open = Some(cut.id);
    }
    ui.label(format!("{} · {}", cut.name, length_sk(cut.frame_count, fps)));
    let shown = p.status.map_or("no status", |s| s.label());
    let mut pick = p.status;
    egui::ComboBox::from_id_salt(("board_status", cut.id.0))
        .selected_text(shown)
        .width(CARD_W - 24.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut pick, None, "no status");
            for s in CutStatus::ALL {
                ui.selectable_value(&mut pick, Some(s), s.label());
            }
        });
    if pick != p.status {
        state.set_cut_status(at, pick);
    }
    let text = state
        .board
        .typing
        .entry(cut.id)
        .or_insert_with(|| p.assignee.clone());
    let r = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text("assignee")
            .desired_width(CARD_W - 24.0),
    );
    if r.lost_focus() {
        let who = state.board.typing.remove(&cut.id).unwrap_or_default();
        state.set_cut_assignee(at, &who);
    } else if !r.has_focus() && state.board.typing.get(&cut.id) != Some(&p.assignee) {
        // Someone else changed it (undo, a guest): show theirs.
        state.board.typing.insert(cut.id, p.assignee.clone());
    }
    let open_notes = p.open_notes();
    let notes = if p.notes.is_empty() {
        "no notes".to_string()
    } else {
        format!("notes {open_notes} open of {}", p.notes.len())
    };
    ui.label(notes);
}

/// The episode timeline pane.
pub fn ui_episode(ui: &mut egui::Ui, state: &mut AppState) {
    let scene_id = state.at().scene;
    let Some(scene) = state.engine.project.scene(scene_id).cloned() else {
        ui.label("no scene");
        return;
    };
    let fps = state.engine.project.fps;
    let trt = board::trt_frames(&scene);
    plate::legend(ui, "episode");
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("TRT {}", board::format_trt(trt, fps)));
        match board::episode_target(&state.engine.project) {
            Some(target) => {
                let mark = board::trt_mark(trt, target);
                let (word, color) = match mark {
                    TrtMark::Over => ("over", plate::AKA),
                    TrtMark::Under => ("under", plate::AO),
                    TrtMark::OnTarget => ("on target", plate::TALLY),
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
        let r = ui.add(
            egui::TextEdit::singleline(&mut state.board.target)
                .hint_text("target MM:SS+FF")
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
    ui.horizontal_wrapped(|ui| {
        let playing = state.board.episode_play && state.view.playing;
        if ui
            .button(if playing { "stop episode" } else { "play episode" })
            .on_hover_text("play every cut of the scene back to back, with its sound")
            .clicked()
        {
            if playing {
                state.view.playing = false;
                state.board.episode_play = false;
            } else if let Some(first) = scene.cuts.first() {
                state.goto_cut(scene_id, first.id);
                state.board.episode_play = true;
                state.view.playing = true;
            }
        }
        if ui
            .button("export EDL")
            .on_hover_text("CMX3600, non-drop, record from 01:00:00:00, into the render folder")
            .clicked()
        {
            match state.render_queue.out_dir(state.file_path.as_deref()) {
                Some(dir) => {
                    let _ = std::fs::create_dir_all(&dir);
                    let name = format!(
                        "{}_{}.edl",
                        deliver::sanitize(&state.engine.project.name),
                        deliver::sanitize(&scene.name)
                    );
                    state.export_edl(scene_id, &dir.join(name));
                }
                None => state.refuse("refused — type a render folder first, or save the project"),
            }
        }
        if ui
            .button("render episode")
            .on_hover_text("queue the scene's cuts as one job in the render queue")
            .clicked()
        {
            crate::render_queue::add_episode_job(state, scene_id);
        }
    });
    ui.separator();
    // The blocks: widths in proportion to each cut's length.
    let width = ui.available_width().max(60.0);
    let per_frame = width / trt.max(1) as f32;
    let mut rects = Vec::with_capacity(scene.cuts.len());
    let mut open: Option<CutId> = None;
    let mut dropped: Option<(usize, usize)> = None;
    let current = state.at().cut;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for (i, c) in scene.cuts.iter().enumerate() {
            let w = (c.frame_count as f32 * per_frame).max(18.0);
            let (rect, r) = ui.allocate_exact_size(egui::vec2(w, 44.0), egui::Sense::click_and_drag());
            let r = r.on_hover_text(format!(
                "{} · {} — drag to move it",
                c.name,
                length_sk(c.frame_count, fps)
            ));
            let name = c.name.clone();
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
            let fill = if c.id == current { plate::AO } else { plate::WELL };
            ui.painter().rect_filled(rect, 2.0, fill);
            if state.board.drag == Some(i) {
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(2.0, plate::TALLY),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().text(
                rect.left_top() + egui::vec2(4.0, 4.0),
                egui::Align2::LEFT_TOP,
                &c.name,
                egui::FontId::proportional(11.0),
                plate::STRUCK,
            );
            if r.clicked() {
                open = Some(c.id);
            }
            if r.drag_started() {
                state.board.drag = Some(i);
            }
            if r.drag_stopped()
                && let Some(from) = state.board.drag.take()
                && let Some(p) = r.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())
            {
                dropped = Some((from, p.x as usize));
            }
            rects.push(rect);
        }
    });
    if let Some((from, x)) = dropped {
        let x = x as f32;
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
        assert_eq!(Pane::Board.title(), "Cut board");
        assert_eq!(Pane::Episode.title(), "Episode timeline");
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
        h.click_combo("Genga");
        h.click("Done");
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
        assert!(h.has_text_containing("1 of 3 cut(s)"));
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
}
