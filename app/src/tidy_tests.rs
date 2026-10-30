//! GM21: menus, dialogs and Shift+Delete — the tidy-up part's tests.

use anim_core::xsheet::ColumnKind;
use eframe::egui;
use egui_kittest::kittest::{NodeT, Queryable};

use crate::config::{self, Action, Group, Keymap};
use crate::doc::AppState;
use crate::newproject::{NewProjectForm, form_ui};
use crate::render_queue::{NEEDS_FFMPEG, RenderQueue};
use crate::testkit::{CTRL, EditorHarness, PanelHarness};
use crate::{Performed, sample, templates};

const SHIFT: egui::Modifiers = egui::Modifiers {
    shift: true,
    ..egui::Modifiers::NONE
};

fn sheet_centre(h: &EditorHarness) -> egui::Pos2 {
    let r = h.editor().pane_rects.iter().find(|(p, _)| *p == crate::workspace::Pane::XSheet);
    r.map(|(_, r)| r.center()).expect("the drawing room docks the sheet")
}

/// The actions fired since `from` (an index into `performed`).
fn fired_since(h: &EditorHarness, from: usize) -> Vec<Action> {
    h.performed()[from..].iter().map(|(a, _)| *a).collect()
}

/// Shift+Delete as egui-winit sends it on Windows: a Cut event with Shift
/// held and no key event.
fn windows_shift_delete(h: &mut EditorHarness) {
    h.set_mods(SHIFT);
    h.event(egui::Event::Cut);
    h.set_mods(egui::Modifiers::NONE);
}

#[test]
fn tidy_shift_delete_cut_event_deletes_a_frame_over_the_sheet() {
    let mut h = EditorHarness::new();
    let frames = h.state().frame_count();
    h.point_at(sheet_centre(&h));
    let from = h.performed().len();
    windows_shift_delete(&mut h);
    assert_eq!(fired_since(&h, from), [Action::DeleteFrames], "once per press");
    assert_eq!(h.performed().last().map(|p| p.1), Some(Performed::Done));
    assert_eq!(h.state().frame_count(), frames - 1);
}

#[test]
fn tidy_a_shift_delete_key_event_still_fires_once() {
    let mut h = EditorHarness::new();
    let frames = h.state().frame_count();
    h.point_at(sheet_centre(&h));
    let from = h.performed().len();
    h.key_mods(SHIFT, egui::Key::Delete);
    assert_eq!(fired_since(&h, from), [Action::DeleteFrames]);
    assert_eq!(h.state().frame_count(), frames - 1);
}

#[test]
fn tidy_industry_shift_delete_cut_event_clears_the_cel_over_the_canvas() {
    let mut h = EditorHarness::new();
    h.rig_mut().config.set_keymap(Keymap::Industry);
    let frames = h.state().frame_count();
    h.point_at(h.editor().canvas.dbg_rect.center());
    let from = h.performed().len();
    windows_shift_delete(&mut h);
    assert_eq!(fired_since(&h, from), [Action::ClearCelAll]);
    assert_eq!(h.state().frame_count(), frames, "no frame closed over the canvas");
}

#[test]
fn tidy_ctrl_x_cut_event_cuts_cells_and_fires_neither() {
    let mut h = EditorHarness::new();
    let col = h.state().view.active_column;
    assert!(h.state().resolve_at(col, 0).is_some(), "the harness draws frame 0");
    h.state_mut().select_cells((col, 0), col, 0);
    let frames = h.state().frame_count();
    h.point_at(sheet_centre(&h));
    let from = h.performed().len();
    h.set_mods(CTRL);
    h.event(egui::Event::Cut);
    h.set_mods(egui::Modifiers::NONE);
    assert_eq!(fired_since(&h, from), [], "Ctrl+X fires no verb");
    assert_eq!(h.state().frame_count(), frames);
    assert!(h.state().sheet_ui.clipboard.is_some(), "{}", h.state().status);
    assert_eq!(h.state().resolve_at(col, 0), None, "the cell was cut");
    // Industry over the canvas: Ctrl+X clears no cel either.
    h.rig_mut().config.set_keymap(Keymap::Industry);
    h.point_at(h.editor().canvas.dbg_rect.center());
    let from = h.performed().len();
    h.set_mods(CTRL);
    h.event(egui::Event::Cut);
    h.set_mods(egui::Modifiers::NONE);
    assert_eq!(fired_since(&h, from), []);
}

#[test]
fn tidy_cut_event_does_nothing_while_typing() {
    let mut h = EditorHarness::new();
    let frames = h.state().frame_count();
    h.point_at(sheet_centre(&h));
    h.focus_text_field(0);
    let from = h.performed().len();
    windows_shift_delete(&mut h);
    assert_eq!(fired_since(&h, from), [], "a text field has focus");
    assert_eq!(h.state().frame_count(), frames);
}

#[test]
fn tidy_rename_cut_is_one_undo_step() {
    let mut s = AppState::new_project("rename", 640, 360, 24, 300.0);
    s.new_drawing_at_frame();
    let hashes = |s: &AppState| -> Vec<_> { s.cut().drawings.iter().map(|d| d.content_hash()).collect() };
    let (old, before, steps) = (s.cut().name.clone(), hashes(&s), s.engine.position());
    assert_ne!(old, "CUT02");
    s.rename_current_cut("CUT02");
    assert_eq!(s.cut().name, "CUT02");
    assert_eq!(s.engine.position(), steps + 1, "one undo step");
    assert!(s.is_dirty());
    s.undo();
    assert_eq!(s.cut().name, old);
    assert_eq!((hashes(&s), s.engine.position()), (before.clone(), steps));
    s.redo();
    assert_eq!(s.cut().name, "CUT02");
    assert_eq!(hashes(&s), before);
    // The same name again adds no step.
    s.rename_current_cut("CUT02");
    assert_eq!(s.engine.position(), steps + 1);
}

#[test]
fn tidy_sizes_come_from_the_templates() {
    let n = templates::ALL.len();
    assert_eq!(templates::SIZES.len(), n + 2);
    for (t, s) in templates::ALL.iter().zip(templates::SIZES) {
        assert_eq!(*s, (t.name, t.width, t.height));
    }
    assert_eq!(templates::SIZES[n], ("Square 1080", 1080, 1080));
    assert_eq!(templates::SIZES[n + 1], ("Vertical 1080×1920", 1080, 1920));
    // The dialog keeps no size table of its own.
    let src = include_str!("newproject.rs");
    assert!(!src.contains("PRESETS"));
    for (name, w, h) in templates::SIZES {
        assert!(!src.contains(&format!("{w}, {h}")), "{name}");
        assert!(!src.contains(&format!("\"{name}\"")), "{name}");
    }
}

/// Each column's name and kind, left to right.
fn sheet_of(s: &AppState) -> Vec<(String, ColumnKind)> {
    let cut = s.cut();
    let kind = |id| cut.column_defs.get(&id).map_or(ColumnKind::Cel, |d| d.kind);
    cut.xsheet.columns.iter().map(|c| (c.name.clone(), kind(c.id))).collect()
}

#[test]
fn tidy_form_project_equals_the_welcome_card() {
    for (i, t) in templates::ALL.iter().enumerate() {
        let mut form = NewProjectForm::default();
        form.pick_template(Some(i));
        let (s, card) = (form.new_state(), t.build());
        let (p, q) = (&s.engine.project, &card.engine.project);
        assert_eq!(
            (p.width, p.height, p.fps, p.dpi),
            (q.width, q.height, q.fps, q.dpi),
            "{}",
            t.name
        );
        assert_eq!(sheet_of(&s), sheet_of(&card), "{}", t.name);
        let page = |s: &AppState| s.cut().meta.get(anim_core::manga::PAGE_KEY).cloned();
        assert_eq!(page(&s), page(&card), "{}", t.name);
        assert_eq!(p.color_law, anim_core::color::ColorLaw::Srgb, "{}", t.name);
        assert_eq!(form.page_template(), t.page.is_some());
    }
    let b5 = templates::ALL.iter().position(|t| t.name == "Manga B5").expect("B5");
    let mut form = NewProjectForm::default();
    form.pick_template(Some(b5));
    let p = form.new_state().engine.project;
    assert_eq!((p.width, p.height, p.dpi), (2591, 3624, 350.0));
    // A page keeps its paper even when the fields were typed over first.
    form.width = 100;
    form.dpi = 72;
    let p = form.new_state().engine.project;
    assert_eq!((p.width, p.height, p.dpi), (2591, 3624, 350.0));
}

/// Is each DragValue on the form (width, height, frame rate, dpi) enabled?
fn form_fields(template: Option<usize>) -> (Vec<bool>, bool) {
    let mut form = NewProjectForm::default();
    form.pick_template(template);
    let mut dressed = false;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(700.0, 700.0))
        .build_ui(|ui| {
            if !std::mem::replace(&mut dressed, true) {
                crate::dress(ui.ctx());
                ui.ctx().request_repaint();
                return;
            }
            form_ui(ui, &mut form, true);
        });
    h.run();
    let fields = h
        .query_all_by_role(egui::accesskit::Role::SpinButton)
        .map(|n| !n.accesskit_node().is_disabled())
        .collect();
    let said = h.query_all_by_label("set by the page template").next().is_some();
    (fields, said)
}

#[test]
fn tidy_page_template_greys_size_and_dpi() {
    let b5 = templates::ALL.iter().position(|t| t.name == "Manga B5").expect("B5");
    // width, height, frame rate, dpi
    assert_eq!(form_fields(Some(b5)), (vec![false, false, true, false], true));
    assert_eq!(form_fields(Some(0)), (vec![true; 4], false));
    assert_eq!(form_fields(None), (vec![true; 4], false));
}

#[test]
fn tidy_open_sample_is_a_file_verb_with_no_chord() {
    let a = Action::OpenSample;
    assert!(Action::ALL.contains(&a));
    assert_eq!(a.label(), "Open the sample cut");
    assert!((1..=120).contains(&a.help().chars().count()), "{}", a.help());
    assert_eq!(a.group(), Group::File);
    assert_eq!(a.menu(), Some("File/Open the sample cut"));
    for word in ["sample", "demo", "example", "tutorial"] {
        assert!(a.aliases().contains(&word), "{word}");
    }
    for &km in Keymap::ALL {
        let binds = config::default_binds(km);
        let b = binds.iter().find(|b| b.action == a).expect("bound");
        assert!(b.chord.is_none() && b.also.is_empty(), "{km:?}");
        assert_eq!(config::clashes(&binds), vec![], "{km:?}");
    }
    assert_eq!(crate::cmdpal::ranked("sample", &[]).first(), Some(&a));
}

#[test]
fn tidy_open_sample_over_a_clean_document_opens_it() {
    let mut h = EditorHarness::new();
    let rev = h.state().engine.revision();
    h.state_mut().saved_rev = rev;
    assert!(!h.state().is_dirty());
    h.click("file");
    h.click("Open the sample cut");
    assert!(!h.editor().leave_pending(), "nothing to ask about");
    let p = &h.state().engine.project;
    assert_eq!((p.width, p.height), sample::SIZE);
    assert_eq!(h.state().frame_count(), sample::FRAMES);
}

#[test]
fn tidy_open_sample_over_unsaved_work_asks_first() {
    let mut h = EditorHarness::new();
    assert!(h.state().is_dirty(), "the harness drew a frame");
    h.click("file");
    h.click("Open the sample cut");
    assert!(h.editor().leave_pending());
    assert!(h.has("Save changes?"), "{}", h.describe());
    assert_eq!(h.state().engine.project.width, 1920, "still the old document");
    h.hold("Don't Save");
    assert!(!h.editor().leave_pending());
    let p = &h.state().engine.project;
    assert_eq!((p.width, p.height), sample::SIZE);
    assert_eq!(h.state().frame_count(), sample::FRAMES);
}

#[test]
fn tidy_open_sample_refuses_a_guest() {
    let mut h = EditorHarness::new();
    h.editor_mut().canvas.is_guest = true;
    assert!(h.editor().can_perform(Action::OpenSample).is_err());
    assert_eq!(h.perform(Action::OpenSample), Performed::Ignored);
    assert_eq!(h.state().engine.project.width, 1920, "the host's document stays");
    assert!(h.state().refusal.contains("leave the room first"), "{}", h.state().refusal);
}

type General = (config::Config, bool, RenderQueue);

#[test]
fn tidy_general_page_says_whether_ffmpeg_was_found() {
    // A stand-in ffmpeg at the configured location: the search finds the
    // file first and scans nothing else.
    let dir = tempfile::tempdir().expect("temp dir");
    let exe = dir.path().join("ffmpeg.exe");
    std::fs::write(&exe, b"").expect("stand-in");
    let typed = exe.display().to_string();
    let mut cfg = config::Config::default();
    cfg.render.ffmpeg = typed.clone();
    let mut queue = RenderQueue::default();
    queue.configure(&cfg.render, "");
    queue.ffmpeg = Some((typed.clone(), Some(exe.clone())));
    let mut p = PanelHarness::new(
        egui::vec2(520.0, 420.0),
        (cfg, false, queue),
        |ui, (cfg, show, q): &mut General| {
            config::general_page(ui, cfg, None, show, Some(q));
        },
    );
    let found = format!("found: {}", exe.display());
    assert!(p.has(&found), "{}", p.describe());
    assert_eq!(p.state().2.searches, 0, "the cached answer is read, not searched");
    // None found: the queue's message and "look again".
    p.state_mut().2.ffmpeg = Some((typed.clone(), None));
    p.frames(2);
    assert!(p.has(NEEDS_FFMPEG), "{}", p.describe());
    assert!(!p.has(&found));
    p.click("look again");
    assert_eq!(p.state().2.searches, 1, "the click emptied the cache");
    assert!(p.has(&found), "{}", p.describe());
    p.frames(30);
    assert_eq!(p.state().2.searches, 1, "thirty frames search at most once");
}
