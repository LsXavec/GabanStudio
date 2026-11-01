//! AU18: project kinds and three bars (research/PSD-approachable-ui-feedback.md
//! 2.1 to 2.10). The prefix is `aukind_`; the fill on a page cel stands beside
//! the fill rig in canvas.rs.

use std::path::{Path, PathBuf};

use anim_core::manga::PAGE_KEY;
use anim_core::store;
use eframe::egui::Key;

use crate::config::Action;
use crate::doc::AppState;
use crate::export_step::Ask;
use crate::newproject::NewProjectForm;
use crate::say::{self, Facts, Verb, Words};
use crate::steps::{self, KIND_KEY, ProjectKind};
use crate::testkit::EditorHarness;
use crate::welcome::{self, WelcomeAction};
use crate::workspace::Stage;
use crate::{Leave, Pane, Performed, same_shape, sample, templates, vocab};

const NO_PICTURE_TIMING: &str = "A single picture has no timing.";
const NO_PAGE_TIMING: &str = "Comic pages have no timing.";
const NO_PAGE_EFFECTS: &str = "Comic pages get their look from tones on the Manga step.";

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

/// One frame, one track, no stamp: the ready paper's document.
fn a_drawing() -> AppState {
    templates::READY.build()
}

/// Forty-eight frames, no stamp: what `new_project` has always made.
fn an_animation() -> AppState {
    let mut state = AppState::new_project("anim", 640, 360, 24, 72.0);
    state.new_drawing_at_frame();
    state
}

/// A book of one page, its stamp gone: a comic an old build saved.
pub(crate) fn a_comic() -> AppState {
    let mut state = templates::ALL[templates::COMIC].build();
    state.engine.project.app_meta.remove(KIND_KEY);
    state
}

/// `a_drawing` on paper a page is laid out for: New page takes it.
fn a_page_sized_drawing() -> AppState {
    let book = &templates::ALL[templates::COMIC];
    let mut state = AppState::new_project("sheet", book.width, book.height, 24, book.dpi());
    state.set_cut_length(1);
    state
}

fn of_kind(kind: ProjectKind) -> AppState {
    match kind {
        ProjectKind::Drawing => a_drawing(),
        ProjectKind::Animation => an_animation(),
        ProjectKind::Comic => a_comic(),
    }
}

fn shipped(state: AppState) -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(state);
    settle(&mut h);
    h
}

fn kind_of(state: &AppState) -> ProjectKind {
    ProjectKind::of(&state.engine.project)
}

fn stored(state: &AppState) -> Option<&str> {
    state.engine.project.app_meta.get(KIND_KEY).map(String::as_str)
}

fn saved(state: &AppState, dir: &Path, name: &str) -> PathBuf {
    let file = dir.join(format!("{name}.animproj"));
    store::save(&state.engine.project, &file).expect("saved");
    file
}

/// The file, opened in this Editor: the state replaced, no boot.
fn open(h: &mut EditorHarness, file: &Path) {
    let ctx = h.ctx();
    h.editor_mut().go(Leave::OpenPath(file.to_path_buf()), &ctx);
    settle(h);
}

fn pills(kind: ProjectKind) -> Vec<&'static str> {
    steps::bar(kind).iter().map(|s| s.label).collect()
}

fn panes(h: &EditorHarness) -> Vec<Pane> {
    h.editor().dock.iter_all_tabs().map(|(_, p)| *p).collect()
}

fn has_sheet(h: &EditorHarness) -> bool {
    panes(h).contains(&Pane::XSheet)
}

/// The step the Done card's `Back to` names.
fn back_word(h: &EditorHarness) -> Option<String> {
    h.editor().export_view().back
}

/// That row, pressed.
fn press_back(h: &mut EditorHarness) {
    h.state_mut().render_queue.panel.asked.push(Ask::Back);
    settle(h);
}

// ---- the kind ----

/// Spec 2.4: every cut a page, then cuts or frames, then a drawing.
#[test]
fn aukind_derive_table() {
    let mut state = an_animation();
    assert_eq!(kind_of(&state), ProjectKind::Animation, "48 frames");
    state.set_cut_length(1);
    assert_eq!(kind_of(&state), ProjectKind::Drawing, "one cut, one frame");
    state.set_cut_length(2);
    assert_eq!(kind_of(&state), ProjectKind::Animation, "over one frame");
    state.set_cut_length(1);
    let scene = state.view.scene;
    state.new_cut(scene);
    assert_eq!(state.engine.project.scenes.iter().flat_map(|s| &s.cuts).count(), 2);
    assert_eq!(kind_of(&state), ProjectKind::Animation, "more than one cut");

    let mut book = a_comic();
    assert_eq!(kind_of(&book), ProjectKind::Comic, "its one cut a page");
    let scene = book.view.scene;
    book.new_page(scene);
    assert_eq!(kind_of(&book), ProjectKind::Comic, "two pages");
    book.new_cut(scene);
    assert_eq!(kind_of(&book), ProjectKind::Animation, "every cut, not any cut");
}

#[test]
fn aukind_stored_wins() {
    for kind in ProjectKind::ALL {
        for mut state in [a_drawing(), an_animation(), a_comic()] {
            kind.stamp(&mut state.engine.project);
            assert_eq!(kind_of(&state), kind);
        }
    }
}

#[test]
fn aukind_unknown_value_derives() {
    for (mut state, derived) in [
        (a_drawing(), ProjectKind::Drawing),
        (an_animation(), ProjectKind::Animation),
        (a_comic(), ProjectKind::Comic),
    ] {
        assert_eq!(kind_of(&state), derived, "absent");
        state.engine.project.app_meta.insert(KIND_KEY.into(), "storyboard".into());
        assert_eq!(kind_of(&state), derived, "unknown");
    }
}

/// Spec 2.3: on disk it is `app.kind`, through `store::save`'s full rewrite.
#[test]
fn aukind_round_trips_as_app_kind() {
    let dir = tempfile::tempdir().unwrap();
    for (kind, word) in [
        (ProjectKind::Drawing, "drawing"),
        (ProjectKind::Animation, "animation"),
        (ProjectKind::Comic, "comic"),
    ] {
        let mut state = an_animation();
        kind.stamp(&mut state.engine.project);
        let file = saved(&state, dir.path(), word);
        let read = store::read_meta(&file).expect("the meta table");
        assert_eq!(read.app_meta.get(KIND_KEY).map(String::as_str), Some(word));
        let bytes = std::fs::read(&file).unwrap();
        assert!(bytes.windows(8).any(|w| w == b"app.kind"), "the key on disk");
        let back = store::load(&file).expect("loaded");
        assert_eq!(ProjectKind::of(&back), kind);
    }
}

/// An old file derives, in memory: the door marks nothing, and only the next
/// real save writes the key.
#[test]
fn aukind_old_file_opens_clean() {
    let dir = tempfile::tempdir().unwrap();
    for (state, word) in [(a_drawing(), "drawing"), (an_animation(), "animation"), (a_comic(), "comic")] {
        let file = saved(&state, dir.path(), word);
        let mut h = EditorHarness::as_shipped();
        settle(&mut h);
        open(&mut h, &file);
        assert_eq!(stored(h.state()), Some(word), "stamped in memory");
        assert!(!h.state().is_dirty(), "{word}");
        assert_eq!(store::read_meta(&file).expect("meta").app_meta.get(KIND_KEY), None, "the file is as it was");
        // The next real save writes it.
        h.state_mut().save(false);
        assert!(h.state_mut().wait_save(), "{}", h.state().status);
        assert_eq!(store::read_meta(&file).expect("meta").app_meta.get(KIND_KEY).map(String::as_str), Some(word));
    }
}

/// Spec 2.5: the sample writes no key, so what it builds stays what it was.
#[test]
fn aukind_sample_stays_byte_stable() {
    let state = sample::build_sample();
    assert_eq!(stored(&state), None);
    assert_eq!(kind_of(&state), ProjectKind::Animation);
    let dir = tempfile::tempdir().unwrap();
    let file = saved(&state, dir.path(), "sample");
    assert!(!std::fs::read(&file).unwrap().windows(8).any(|w| w == b"app.kind"));
    // The door stamps it in memory, and marks nothing: the sample's edits are
    // what it arrives dirty with.
    let was = state.is_dirty();
    let mut h = shipped(state);
    settle(&mut h);
    assert_eq!(stored(h.state()), Some("animation"));
    assert!(!h.state().meta_dirty);
    assert_eq!(h.state().is_dirty(), was);
}

// ---- the bars ----

#[test]
fn aukind_bars_are_the_table() {
    assert_eq!(pills(ProjectKind::Drawing), ["Draw", "Color", "Effects", "Export"]);
    assert_eq!(pills(ProjectKind::Animation), ["Plan", "Timing", "Color", "Effects", "Export"]);
    assert_eq!(pills(ProjectKind::Comic), ["Manga", "Color", "Export"]);
    // `of` searches the catalog, so a stage off a bar still finds its words.
    let of = |stage| steps::of(stage).map(|row| row.label);
    assert_eq!(of(Stage::Layout), Some("Plan"), "not the manga row");
    assert_eq!(of(Stage::Drawing), Some("Draw"));
    assert_eq!(of(Stage::Manga), Some("Manga"));
    assert_eq!(Stage::Layout.card().0, "genga");
    assert_eq!(Stage::Manga.card().0, "manga");
    // The top row draws exactly those pills on each kind.
    for kind in ProjectKind::ALL {
        let h = shipped(of_kind(kind));
        for row in steps::CATALOG {
            let on_bar = pills(kind).contains(&row.label);
            assert_eq!(h.has(row.label), on_bar, "{kind:?}: {}\n{}", row.label, h.describe());
        }
    }
}

/// Spec 2.5, each route. A stamp lands before `saved_rev`: a blank is not dirty.
#[test]
fn aukind_every_route_stamps() {
    use ProjectKind::{Animation, Comic, Drawing};
    let start = [(templates::ANIMATION, Animation), (templates::PICTURE, Drawing)];
    let more = [(templates::PICTURE_SQUARE, Drawing), (templates::PICTURE_TALL, Drawing)];
    for (i, kind) in start.into_iter().chain(more) {
        let mut h = EditorHarness::as_shipped();
        h.answer_start(WelcomeAction::Intent(i));
        settle(&mut h);
        assert_eq!(kind_of(h.state()), kind, "{}", templates::INTENTS[i].name);
        assert!(!h.state().is_dirty());
    }
    let rows = templates::ALL.iter().enumerate();
    let (mut comics, mut animatics) = (0, 0);
    for (i, t) in rows {
        let comic = t.name.contains("Manga") || t.name.contains("Webtoon");
        comics += usize::from(comic);
        let kind = if comic { Comic } else { Animation };
        assert_eq!(t.kind, Some(kind), "{}", t.name);
        let built = t.build();
        assert_eq!(stored(&built).is_some(), true, "{}", t.name);
        assert!(!built.is_dirty(), "{}", t.name);
        let mut h = EditorHarness::as_shipped();
        h.answer_start(WelcomeAction::Template(i));
        settle(&mut h);
        assert_eq!(kind_of(h.state()), kind, "{}", t.name);
        assert!(!h.state().is_dirty(), "{}", t.name);
        if t.name.starts_with("Animatic") {
            assert_eq!(h.editor().stage, Some(Stage::Edit), "it still opens on Timing");
            animatics += 1;
        }
    }
    assert_eq!(comics, 3, "Manga B5, A4, Webtoon");
    assert_eq!(animatics, 1, "the Animatic's row ran");
    // The form's blank and the sample derive: 48 frames, no key.
    let blank = NewProjectForm::default().new_state();
    assert_eq!(stored(&blank), None);
    assert_eq!(kind_of(&blank), Animation);
    assert_eq!(kind_of(&sample::build_sample()), Animation);
}

#[test]
fn aukind_ready_paper_is_an_unstamped_drawing() {
    let built = templates::READY.build();
    assert_eq!(stored(&built), None, "no key");
    assert_eq!(built.frame_count(), 1);
    assert_eq!((built.engine.project.width, built.engine.project.height), (1280, 720));
    assert_eq!(built.engine.project.fps, 12);
    assert_eq!(kind_of(&built), ProjectKind::Drawing);
    assert!(!templates::INTENTS.iter().any(|t| std::ptr::eq(t, &templates::READY)));
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    assert_eq!(h.state().frame_count(), 1, "one frame");
    assert!(h.editor().strip_closed(), "the strip stays closed");
    assert_eq!(h.editor().bar()[0].label, vocab::STEP_DRAW);
    assert_eq!(h.node_toggled(vocab::STEP_DRAW), Some(true));
    assert_eq!(stored(h.state()), Some("drawing"), "the door stamps it in memory");
    assert!(!h.state().is_dirty(), "and leaves it clean");
}

// ---- the door ----

#[test]
fn aukind_door_lands_on_the_first_step() {
    let dir = tempfile::tempdir().unwrap();
    let picture = saved(&a_drawing(), dir.path(), "picture");
    let cut = saved(&an_animation(), dir.path(), "cut");
    // A drawing opened from Timing, which its bar lacks.
    let mut h = EditorHarness::as_animation();
    h.perform(Action::RoomEdit);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Edit));
    open(&mut h, &picture);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert_eq!(h.node_toggled(vocab::STEP_DRAW), Some(true));
    assert!(!h.state().is_dirty());
    assert!(h.editor().strip_closed(), "the animation's strip stays with it");
    // A step on both bars stays.
    h.perform(Action::RoomFinishing);
    settle(&mut h);
    open(&mut h, &cut);
    assert_eq!(h.editor().stage, Some(Stage::InkPaint));
    // A place off the spine stays where it is.
    h.perform(Action::Layout3dRoom);
    settle(&mut h);
    let name = format!("Current place: {}", vocab::LAYOUT_3D);
    assert!(h.has(&name));
    open(&mut h, &picture);
    assert_eq!(h.editor().stage, None);
    assert!(h.has(&name), "{}", h.describe());
}

/// Spec 2.6: one stage on two bars with two recipes, and no boot between.
#[test]
fn aukind_door_swaps_draw_and_plan_docks() {
    let dir = tempfile::tempdir().unwrap();
    let picture = saved(&a_drawing(), dir.path(), "picture");
    let cut = saved(&an_animation(), dir.path(), "cut");
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    assert!(!has_sheet(&h), "a drawing has no button for it");
    open(&mut h, &cut);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert!(has_sheet(&h), "an animation from Draw: {:?}", panes(&h));
    open(&mut h, &picture);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert!(!has_sheet(&h), "a drawing from Plan: {:?}", panes(&h));
    // Plan's sheet is no drawing's: it has no button to undo it.
    h.perform(Action::RoomFinishing);
    h.perform(Action::RoomDrawing);
    settle(&mut h);
    assert!(!has_sheet(&h), "back in Draw: {:?}", panes(&h));
    h.perform(Action::ResetLayout);
    settle(&mut h);
    assert!(!has_sheet(&h), "through Reset: {:?}", panes(&h));
    // And it waits: nothing moves at the promotion, and the next Plan has it.
    h.perform(Action::AddDrawingAfter);
    settle(&mut h);
    assert_eq!(stored(h.state()), Some("animation"));
    assert!(!has_sheet(&h), "no dock swap: {:?}", panes(&h));
    h.perform(Action::RoomFinishing);
    h.perform(Action::RoomDrawing);
    settle(&mut h);
    assert!(has_sheet(&h), "the next time they enter Plan: {:?}", panes(&h));
}

/// Spec 2.6 step 4: the document replaced on Export.
#[test]
fn aukind_door_rewrites_the_way_back() {
    let dir = tempfile::tempdir().unwrap();
    let picture = saved(&a_drawing(), dir.path(), "picture");
    let cut = saved(&an_animation(), dir.path(), "cut");
    let mut h = EditorHarness::as_animation();
    settle(&mut h);
    h.perform(Action::RoomEdit);
    settle(&mut h);
    h.perform(Action::RoomExport);
    settle(&mut h);
    assert_eq!(back_word(&h).as_deref(), Some(vocab::STEP_TIMING));
    open(&mut h, &picture);
    assert_eq!(h.editor().stage, Some(Stage::Export));
    assert_eq!(back_word(&h).as_deref(), Some(vocab::STEP_DRAW));
    press_back(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert!(same_shape(&h.editor().dock, &Stage::Drawing.dock()), "{:?}", panes(&h));
    assert_eq!(h.node_toggled(vocab::STEP_DRAW), Some(true));
    // Back the other way: Plan's own recipe, the way it reads for this project.
    h.perform(Action::RoomExport);
    settle(&mut h);
    open(&mut h, &cut);
    assert_eq!(back_word(&h).as_deref(), Some(vocab::STEP_PLAN));
    press_back(&mut h);
    assert!(same_shape(&h.editor().dock, &Stage::Layout.dock()), "{:?}", panes(&h));
    // A guest's snapshot opens nothing: the way back stands while its pill does.
    h.perform(Action::RoomEdit);
    settle(&mut h);
    h.perform(Action::RoomExport);
    settle(&mut h);
    h.editor_mut().canvas.is_guest = true;
    h.editor_mut().state = an_animation();
    settle(&mut h);
    assert_eq!(back_word(&h).as_deref(), Some(vocab::STEP_TIMING));
    h.editor_mut().state = a_drawing();
    settle(&mut h);
    assert_eq!(back_word(&h).as_deref(), Some(vocab::STEP_DRAW));
}

#[test]
fn aukind_export_back_names_a_pill() {
    for kind in ProjectKind::ALL {
        for from_first in [true, false] {
            let mut h = shipped(of_kind(kind));
            let first = h.editor().bar()[0].label;
            if !from_first {
                // A step every bar has.
                h.perform(Action::RoomFinishing);
                settle(&mut h);
            }
            h.perform(Action::RoomExport);
            settle(&mut h);
            let back = if from_first { first } else { vocab::STEP_COLOR };
            assert!(pills(kind).contains(&back));
            assert_eq!(back_word(&h).as_deref(), Some(back), "{kind:?}");
            press_back(&mut h);
            assert_eq!(h.node_toggled(back), Some(true), "{kind:?}");
        }
        // With no way back on record it is still a pill, and the first one.
        let mut h = shipped(of_kind(kind));
        h.perform(Action::RoomExport);
        settle(&mut h);
        h.editor_mut().export_from = None;
        settle(&mut h);
        assert_eq!(back_word(&h).as_deref(), Some(h.editor().bar()[0].label), "{kind:?}");
        assert_eq!(h.editor().stage_kept().map(|st| h.editor().step_name(st)), Some(h.editor().bar()[0].label));
    }
}

#[test]
fn aukind_comic_opens_on_manga() {
    let dir = tempfile::tempdir().unwrap();
    let book = saved(&a_comic(), dir.path(), "book");
    for step in [
        Action::RoomDrawing,
        Action::RoomEdit,
        Action::RoomFinishing,
        Action::RoomComposite,
        Action::RoomExport,
    ] {
        let mut h = EditorHarness::as_animation();
        h.perform(step);
        settle(&mut h);
        open(&mut h, &book);
        assert_eq!(kind_of(h.state()), ProjectKind::Comic);
        assert_eq!(h.editor().stage, Some(Stage::Manga), "from {step:?}");
        assert_eq!(h.node_toggled(vocab::STEP_MANGA), Some(true), "lit on its bar");
        let name = format!("Current place: {}", vocab::STEP_MANGA);
        assert!(!h.has(&name), "Manga is on its bar: {}", h.describe());
    }
}

/// Spec 2.8: every caller that names `Stage::Drawing` meets one redirect.
#[test]
fn aukind_no_route_puts_a_comic_on_draw() {
    let mut h = shipped(a_comic());
    let manga = Some(Stage::Manga);
    assert_eq!(h.editor().stage, manga);
    for action in [Action::RoomLayout, Action::RoomDrawing] {
        h.perform(Action::RoomFinishing);
        settle(&mut h);
        h.perform(action);
        settle(&mut h);
        assert_eq!(h.editor().stage, manga, "{action:?}");
    }
    for key in [Key::F4, Key::F5] {
        h.perform(Action::RoomFinishing);
        settle(&mut h);
        h.key(key);
        settle(&mut h);
        assert_eq!(h.editor().stage, manga, "{key:?}");
    }
    // ResetLayout's arm with no step and no place.
    h.editor_mut().stage = None;
    h.editor_mut().place_dock = None;
    h.perform(Action::ResetLayout);
    settle(&mut h);
    assert_eq!(h.editor().stage, manga);
    // leave_export's arm with no way back.
    h.perform(Action::RoomExport);
    settle(&mut h);
    h.editor_mut().export_from = None;
    h.editor_mut().leave_export();
    settle(&mut h);
    assert_eq!(h.editor().stage, manga);
    // demo_circle's fallback, with no canvas on screen.
    h.perform(Action::RoomExport);
    settle(&mut h);
    h.editor_mut().export_from = None;
    h.editor_mut().demo_circle();
    settle(&mut h);
    assert_eq!(h.editor().stage, manga);
}

/// Spec 2.4: an old animation with one page among its cuts keeps its Plan and
/// Timing, or it would open on a bar that refuses its own work.
#[test]
fn aukind_one_page_among_cuts_stays_an_animation() {
    let mut state = a_page_sized_drawing();
    state.set_cut_length(12);
    let scene = state.view.scene;
    state.new_page(scene);
    let cuts: Vec<_> = state.engine.project.scenes.iter().flat_map(|s| &s.cuts).collect();
    assert_eq!(cuts.iter().filter(|c| c.meta.contains_key(PAGE_KEY)).count(), 1, "one page of two");
    assert_eq!(cuts.len(), 2);
    let dir = tempfile::tempdir().unwrap();
    let file = saved(&state, dir.path(), "mixed");
    let mut h = EditorHarness::as_animation();
    open(&mut h, &file);
    assert_eq!(kind_of(h.state()), ProjectKind::Animation);
    assert!(h.has(vocab::STEP_PLAN) && h.has(vocab::STEP_TIMING), "{}", h.describe());
    assert_eq!(h.editor().can_perform(Action::RoomEdit), Ok(()));
    h.key(Key::F6);
    assert_eq!(h.editor().stage, Some(Stage::Edit));
}

#[test]
fn aukind_new_page_lands_on_manga() {
    for step in [Action::RoomDrawing, Action::RoomComposite, Action::RoomFinishing] {
        let mut h = shipped(a_page_sized_drawing());
        h.perform(step);
        settle(&mut h);
        let stage = h.editor().stage;
        assert_eq!(h.perform(Action::NewPage), Performed::Done);
        settle(&mut h);
        assert_eq!(kind_of(h.state()), ProjectKind::Comic, "{step:?}");
        // Draw and Effects are off the Comic bar; Color is on it, and it stays.
        let want = if stage == Some(Stage::InkPaint) { stage } else { Some(Stage::Manga) };
        assert_eq!(h.editor().stage, want, "{step:?}");
    }
    // Export is on the bar, so it stays; the way back was a step that is not,
    // and no comic reaches Draw or Effects by it (acceptance 6).
    for step in [Action::RoomDrawing, Action::RoomComposite] {
        let mut h = shipped(a_page_sized_drawing());
        h.perform(step);
        settle(&mut h);
        h.perform(Action::RoomExport);
        settle(&mut h);
        assert_eq!(h.perform(Action::NewPage), Performed::Done);
        settle(&mut h);
        assert_eq!(kind_of(h.state()), ProjectKind::Comic, "{step:?}");
        assert_eq!(h.editor().stage, Some(Stage::Export), "{step:?}");
        assert_eq!(back_word(&h).as_deref(), Some(vocab::STEP_MANGA), "{step:?}");
        assert_eq!(h.editor().stage_kept(), Some(Stage::Manga), "{step:?}");
        press_back(&mut h);
        assert_eq!(h.editor().stage, Some(Stage::Manga), "{step:?}");
        assert_eq!(h.node_toggled(vocab::STEP_MANGA), Some(true), "{step:?}");
        assert!(same_shape(&h.editor().dock, &Stage::Manga.dock()), "{step:?}: {:?}", panes(&h));
    }
    // From a step the Comic bar has, the way back stands.
    let mut h = shipped(a_page_sized_drawing());
    h.perform(Action::RoomFinishing);
    settle(&mut h);
    h.perform(Action::RoomExport);
    settle(&mut h);
    h.perform(Action::NewPage);
    settle(&mut h);
    assert_eq!(back_word(&h).as_deref(), Some(vocab::STEP_COLOR));
}

// ---- the hidden steps ----

/// Spec 2.8, whole: what each step key does on each kind, or the sentence it
/// says. None is the bar's first step.
fn table(kind: ProjectKind) -> [(Action, Key, Result<Option<Stage>, &'static str>); 6] {
    use ProjectKind::{Animation, Comic, Drawing};
    let timing = match kind {
        Drawing => Err(NO_PICTURE_TIMING),
        Animation => Ok(Some(Stage::Edit)),
        Comic => Err(NO_PAGE_TIMING),
    };
    let effects = if kind == Comic { Err(NO_PAGE_EFFECTS) } else { Ok(Some(Stage::Composite)) };
    [
        (Action::RoomLayout, Key::F4, Ok(None)),
        (Action::RoomDrawing, Key::F5, Ok(None)),
        (Action::RoomEdit, Key::F6, timing),
        (Action::RoomFinishing, Key::F7, Ok(Some(Stage::InkPaint))),
        (Action::RoomComposite, Key::F8, effects),
        (Action::RoomExport, Key::F9, Ok(Some(Stage::Export))),
    ]
}

#[test]
fn aukind_hidden_steps_refuse_through_a_chord() {
    for kind in ProjectKind::ALL {
        let first = steps::bar(kind)[0].stage;
        for (action, key, row) in table(kind) {
            let mut h = shipped(of_kind(kind));
            h.perform(Action::RoomFinishing);
            settle(&mut h);
            let seq = h.state().refusal_seq;
            h.key(key);
            settle(&mut h);
            match row {
                Ok(stage) => {
                    assert_eq!(h.editor().stage, stage.or(Some(first)), "{kind:?} {action:?}");
                    assert_eq!(h.state().refusal_seq, seq, "{kind:?} {action:?}");
                }
                Err(why) => {
                    assert_eq!(h.editor().stage, Some(Stage::InkPaint), "{kind:?} {action:?}");
                    assert_eq!(h.state().refusal_seq, seq + 1, "{kind:?} {action:?}");
                    assert_eq!(h.state().refusal, why, "{kind:?} {action:?}");
                }
            }
        }
    }
    // The verb itself says Ignored, as every refusal does.
    let mut h = shipped(a_drawing());
    assert_eq!(h.perform(Action::RoomEdit), Performed::Ignored);
    // Manga is a place off the spine on the two bars that lack it.
    for kind in [ProjectKind::Drawing, ProjectKind::Animation] {
        let mut h = shipped(of_kind(kind));
        h.perform(Action::MangaRoom);
        settle(&mut h);
        assert_eq!(h.editor().stage, Some(Stage::Manga), "{kind:?}");
        assert!(h.has(&format!("Current place: {}", vocab::STEP_MANGA)), "{kind:?}: {}", h.describe());
    }
}

/// One sentence, from one function: the menu's grey and the palette's dim say
/// what the chord says.
#[test]
fn aukind_can_perform_agrees() {
    for kind in ProjectKind::ALL {
        let h = shipped(of_kind(kind));
        for (action, _, row) in table(kind) {
            match row {
                Ok(_) => assert_eq!(h.editor().can_perform(action), Ok(()), "{kind:?} {action:?}"),
                Err(why) => {
                    assert_eq!(h.editor().can_perform(action), Err(why), "{kind:?} {action:?}");
                    assert_eq!(h.editor().hidden_step(action), Some(why));
                }
            }
        }
    }
    // Through `invoke`, as a menu row or the palette runs it.
    let mut h = shipped(a_comic());
    let ctx = h.ctx();
    assert_eq!(h.editor_mut().invoke(Action::RoomComposite, &ctx), Performed::Ignored);
    settle(&mut h);
    assert_eq!(h.state().refusal, format!("refused — {NO_PAGE_EFFECTS}"));
    assert_eq!(h.editor().stage, Some(Stage::Manga));
}

/// The frozen boot's seam: it reads no kind, so nothing there refuses.
#[test]
fn aukind_frozen_boot_reads_no_kind() {
    let mut h = EditorHarness::with_state(a_drawing());
    settle(&mut h);
    assert_eq!(h.editor().kind(), None);
    assert_eq!(stored(h.state()), None, "no stamp");
    assert_eq!(h.editor().can_perform(Action::RoomEdit), Ok(()));
    h.perform(Action::RoomLayout);
    settle(&mut h);
    // AU19: the door turns everyone from `Stage::Layout`; no kind asked.
    assert_eq!(h.editor().stage, Some(Stage::Drawing), "no refusal");
    assert_eq!(h.state().refusal, "");
    h.state_mut().set_cut_length(4);
    settle(&mut h);
    assert_eq!(stored(h.state()), None, "no promotion");
}

#[test]
fn aukind_f4_runs_the_first_step() {
    for kind in ProjectKind::ALL {
        let mut h = shipped(of_kind(kind));
        h.perform(Action::RoomFinishing);
        settle(&mut h);
        let seq = h.state().refusal_seq;
        h.key(Key::F4);
        settle(&mut h);
        let first = &steps::bar(kind)[0];
        assert_eq!(h.editor().stage, Some(first.stage), "{kind:?}");
        assert_eq!(h.node_toggled(first.label), Some(true), "{kind:?}");
        assert_eq!(h.state().refusal_seq, seq, "never refused");
        assert_eq!(h.state().status, format!("step: {}", first.label));
    }
}

// ---- promotion ----

/// The ways a drawing grows, each by its own verb. None marks the document
/// through the kind alone, so each route is asserted the same way.
const GROWN: [(&str, fn(&mut EditorHarness)); 5] = [
    ("Add drawing", |h| {
        h.perform(Action::AddDrawingAfter);
    }),
    ("Insert frames", |h| {
        h.perform(Action::InsertFrames);
    }),
    ("cut length", |h| h.state_mut().set_cut_length(6)),
    ("a new cut", |h| {
        let scene = h.state().view.scene;
        h.state_mut().new_cut(scene);
    }),
    ("paste", |h| {
        h.state_mut().new_drawing_at_frame();
        assert!(h.state_mut().copy_cells(), "{}", h.state().status);
        h.perform(Action::PasteCellsInsert);
    }),
];

#[test]
fn aukind_promotes_by_frames() {
    for (route, grow) in GROWN {
        let mut h = EditorHarness::as_shipped();
        settle(&mut h);
        let (dock, stage) = (panes(&h), h.editor().stage);
        assert_eq!(stored(h.state()), Some("drawing"), "{route}");
        grow(&mut h);
        settle(&mut h);
        assert_eq!(stored(h.state()), Some("animation"), "{route}");
        assert!(h.state().is_dirty(), "{route}");
        assert!(h.has(vocab::STEP_PLAN) && h.has(vocab::STEP_TIMING), "{route}: {}", h.describe());
        assert!(!h.has(vocab::STEP_DRAW), "{route}");
        assert_eq!(h.editor().stage, stage, "{route}: nothing moves under the pen");
        assert_eq!(panes(&h), dock, "{route}: no dock swap");
    }
}

/// A drawing has no rows past its one frame, so a paste running past it brings
/// them, in its own undo step, by either route. What fits in the frame stays a
/// drawing, and an animation's cells past the end are as they were.
#[test]
fn aukind_paste_past_the_frame_makes_frames() {
    let paste: [fn(&mut EditorHarness); 2] = [
        |h| {
            h.perform(Action::PasteCellsInsert);
        },
        // The sheet's own Ctrl+Shift+V, which comes in with no Editor.
        |h| h.state_mut().paste_cells(true),
    ];
    for route in paste {
        let mut h = EditorHarness::as_shipped();
        settle(&mut h);
        h.state_mut().new_drawing_at_frame();
        assert!(h.state_mut().copy_cells(), "{}", h.state().status);
        let last_key = |h: &EditorHarness| h.state().cut().xsheet.columns[0].last_key();
        let before = last_key(&h);
        route(&mut h);
        settle(&mut h);
        assert_eq!(h.state().frame_count(), 2, "the pushed cell is a frame");
        assert_ne!(last_key(&h), before, "it pasted");
        assert_eq!(stored(h.state()), Some("animation"));
        h.state_mut().undo();
        settle(&mut h);
        assert_eq!(h.state().frame_count(), 1, "one undo step: the frame");
        assert_eq!(last_key(&h), before, "one undo step: the cells");
        assert_eq!(stored(h.state()), Some("animation"), "and it does not demote");
    }

    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    h.state_mut().new_drawing_at_frame();
    assert!(h.state_mut().copy_cells(), "{}", h.state().status);
    h.perform(Action::PasteCells);
    settle(&mut h);
    assert_eq!(h.state().frame_count(), 1, "over its own cell");
    assert_eq!(stored(h.state()), Some("drawing"));

    let mut h = shipped(an_animation());
    let frames = h.state().frame_count();
    assert!(h.state_mut().copy_cells(), "{}", h.state().status);
    h.perform(Action::PasteCellsInsert);
    settle(&mut h);
    assert_eq!(h.state().frame_count(), frames, "an animation's cut stands");
}

/// A verb that grows a drawing is a row of what it grew into: the row ticks.
#[test]
fn aukind_growing_ticks_the_row_it_grew_into() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    assert_eq!(h.perform(Action::AddDrawingAfter), Performed::Done);
    assert_eq!(h.editor().kind(), Some(ProjectKind::Animation), "by the frame it ran in");
    assert_eq!(h.editor().ticks(), [false, true, false, false, false]);
    let mut h = shipped(a_page_sized_drawing());
    assert_eq!(h.perform(Action::NewPage), Performed::Done);
    settle(&mut h);
    assert_eq!(h.editor().ticks(), [false, false, true, false, false]);
}

/// Spec 1.8: said once, to a person with hints on.
#[test]
fn aukind_remark_is_said_once() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    h.perform(Action::AddDrawingAfter);
    settle(&mut h);
    assert_eq!(h.state().status, say::GREW_WORDS);
    assert_ne!(h.editor().say.seen & say::GREW, 0);
    assert_ne!(h.rig().config.ui.hints_seen & say::GREW, 0, "stored");
    // Back on a new drawing: not said again.
    let ctx = h.ctx();
    h.editor_mut().go(Leave::New, &ctx);
    settle(&mut h);
    h.rebuild_editor(a_drawing());
    settle(&mut h);
    h.perform(Action::AddDrawingAfter);
    settle(&mut h);
    assert_eq!(stored(h.state()), Some("animation"));
    assert_ne!(h.state().status, say::GREW_WORDS);
}

#[test]
fn aukind_two_tracks_on_one_frame_stay_a_drawing() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    h.state_mut().add_column();
    settle(&mut h);
    assert_eq!(h.state().cut().xsheet.columns.len(), 2, "two tracks");
    // Each with a drawing of its own: a picture, so the band's keys stay away.
    h.state_mut().new_drawing_at_frame();
    settle(&mut h);
    let first = h.state().cut().xsheet.columns[0].id;
    h.state_mut().view.active_column = first;
    h.state_mut().new_drawing_at_frame();
    settle(&mut h);
    assert_eq!(h.state().cut().drawings.len(), 2, "two drawings");
    assert_eq!(h.state().frame_count(), 1);
    assert_eq!(stored(h.state()), Some("drawing"));
    assert_eq!(kind_of(h.state()), ProjectKind::Drawing);
    assert!(h.has(vocab::STEP_DRAW) && !h.has(vocab::STEP_TIMING));
    assert_no_frame_keys(&h);
    // They arrive with the frame that makes it an animation.
    h.perform(Action::InsertFrames);
    settle(&mut h);
    assert!(h.has("Speed") && h.has("Flip"), "{}", h.describe());
}

/// The chrome the second drawing brings, gone.
fn assert_no_frame_keys(h: &EditorHarness) {
    assert!(!h.editor().full_band);
    for key in ["previous frame (S)", "next frame (F)", "Ghost frames toggle", "Speed", "Flip"] {
        assert!(!h.has(key), "{key}\n{}", h.describe());
    }
}

/// R17: the strip and the band's keys are an animation's, and four routes
/// replace the document in this Editor with no boot.
#[test]
fn aukind_strip_and_keys_stay_with_the_animation() {
    let dir = tempfile::tempdir().unwrap();
    let picture = saved(&a_drawing(), dir.path(), "picture");
    let book = saved(&a_comic(), dir.path(), "book");
    let mut long = a_drawing();
    long.set_cut_length(6);
    ProjectKind::Drawing.stamp(&mut long.engine.project);
    let outgrown = saved(&long, dir.path(), "outgrown");
    for file in [&picture, &book] {
        let mut h = EditorHarness::as_animation();
        settle(&mut h);
        h.state_mut().new_drawing_at_frame();
        h.perform(Action::AddDrawingAfter);
        settle(&mut h);
        assert!(!h.editor().strip_closed() && h.editor().full_band);
        assert!(h.has("Speed") && h.has("Flip"), "{}", h.describe());
        open(&mut h, file);
        assert!(h.editor().strip_closed(), "{file:?}");
        assert_no_frame_keys(&h);
        assert!(!h.has("Add drawing (band)"), "{}", h.describe());
    }
    // A drawing stamped with frames in it opens as what it has grown into.
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    open(&mut h, &outgrown);
    assert_eq!(stored(h.state()), Some("animation"));
    assert!(!h.editor().strip_closed(), "it opens with its frames");
    // A comic with Alt+A: frames, and still no strip.
    let mut h = shipped(a_comic());
    assert_eq!(h.perform(Action::AddDrawingAfter), Performed::Done);
    settle(&mut h);
    assert_eq!(stored(h.state()), Some("comic"));
    assert!(h.state().frame_count() > 1);
    assert!(h.editor().strip_closed());
}

#[test]
fn aukind_new_page_makes_a_comic() {
    let mut h = shipped(a_page_sized_drawing());
    assert_eq!(kind_of(h.state()), ProjectKind::Drawing);
    assert_eq!(h.perform(Action::NewPage), Performed::Done);
    settle(&mut h);
    assert_eq!(stored(h.state()), Some("comic"), "by the page, not the count of cuts");
    assert!(h.state().is_dirty());
    assert_ne!(h.state().status, say::GREW_WORDS, "no Plan or Timing arrived");
    // An animation adding a page stays one.
    let mut state = a_page_sized_drawing();
    state.set_cut_length(4);
    let mut h = shipped(state);
    assert_eq!(h.perform(Action::NewPage), Performed::Done);
    settle(&mut h);
    assert_eq!(stored(h.state()), Some("animation"));
    // A comic with frames stays a comic.
    let mut h = shipped(a_comic());
    h.state_mut().set_cut_length(6);
    settle(&mut h);
    assert_eq!(stored(h.state()), Some("comic"));
    assert_eq!(pills(kind_of(h.state())), ["Manga", "Color", "Export"]);
}

/// One-way: a bar that changes on Ctrl+Z is worse than one extra pill.
#[test]
fn aukind_undo_does_not_demote() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    h.perform(Action::AddDrawingAfter);
    settle(&mut h);
    assert_eq!(h.state().frame_count(), 2);
    assert_eq!(stored(h.state()), Some("animation"));
    h.state_mut().undo();
    settle(&mut h);
    assert_eq!(h.state().frame_count(), 1, "undone");
    assert_eq!(stored(h.state()), Some("animation"));
    assert!(h.has(vocab::STEP_PLAN));
    // An animation cut to one frame stays one.
    let mut h = shipped(an_animation());
    h.state_mut().set_cut_length(1);
    settle(&mut h);
    assert_eq!(stored(h.state()), Some("animation"));
}

/// Nothing caches the kind: `Leave::Sample` replaces the state with no boot.
#[test]
fn aukind_leave_sample_resets() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    assert_eq!(pills(kind_of(h.state())), pills(ProjectKind::Drawing));
    let ctx = h.ctx();
    h.editor_mut().go(Leave::Sample, &ctx);
    settle(&mut h);
    assert_eq!(stored(h.state()), Some("animation"));
    assert!(!h.state().meta_dirty, "the door marks nothing");
    assert!(h.has(vocab::STEP_PLAN) && h.has(vocab::STEP_TIMING), "{}", h.describe());
    assert_eq!(h.editor().stage, Some(Stage::Drawing));
    assert_eq!(h.editor().step_name(Stage::Drawing), vocab::STEP_PLAN);
}

/// A guest's edit promotes on its own screen: the rule reads the project, and
/// nothing goes on the wire: no command, no revision.
#[test]
fn aukind_guest_promotes_with_no_wire_message() {
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    h.editor_mut().canvas.is_guest = true;
    h.state_mut().set_cut_length(4);
    let revision = h.state().engine.revision();
    settle(&mut h);
    assert_eq!(stored(h.state()), Some("animation"));
    assert_eq!(h.state().engine.revision(), revision, "no command of its own");
    assert!(h.has(vocab::STEP_PLAN), "{}", h.describe());
    // A snapshot that makes a guest's document a comic does not move them off
    // the step they stand on: the host's truth is not a document opened.
    let mut h = shipped(an_animation());
    h.editor_mut().canvas.is_guest = true;
    h.perform(Action::RoomFinishing);
    settle(&mut h);
    h.editor_mut().state = a_comic();
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::InkPaint));
    // Nor does it rearrange the step: a sheet opened by hand stands.
    let mut h = shipped(a_drawing());
    h.editor_mut().canvas.is_guest = true;
    h.editor_mut().open_pane(Pane::XSheet);
    settle(&mut h);
    let dock = panes(&h);
    assert!(has_sheet(&h), "{dock:?}");
    h.editor_mut().state = a_drawing();
    settle(&mut h);
    assert_eq!(panes(&h), dock);
    // A person's own open does.
    let mut h = shipped(a_drawing());
    h.editor_mut().open_pane(Pane::XSheet);
    settle(&mut h);
    h.editor_mut().state = a_drawing();
    settle(&mut h);
    assert!(!has_sheet(&h), "{:?}", panes(&h));
}

// ---- the words ----

#[test]
fn aukind_go_to_word_follows_the_kind() {
    assert_eq!(Verb::GoToDraw.label(Some(ProjectKind::Drawing)), "Go to Draw");
    assert_eq!(Verb::GoToDraw.label(Some(ProjectKind::Animation)), "Go to Plan");
    assert_eq!(Verb::GoToDraw.label(Some(ProjectKind::Comic)), "Go to Manga");
    assert_eq!(Verb::GoToDraw.label(None), "Go to Draw", "the frozen boot");
    for kind in ProjectKind::ALL {
        let mut h = shipped(of_kind(kind));
        h.perform(Action::RoomFinishing);
        settle(&mut h);
        let ctx = h.ctx();
        h.editor_mut().say_act(Verb::GoToDraw, &ctx);
        settle(&mut h);
        assert_eq!(h.editor().stage, Some(steps::bar(kind)[0].stage), "{kind:?}");
        // The Export step's way to the first step, with no art to send.
        h.perform(Action::RoomExport);
        settle(&mut h);
        let word = Verb::GoToDraw.label(Some(kind));
        assert_eq!(h.editor().export_view().go_first, word);
        // Its card speaks of that step too, under either button.
        let card = steps::bar(kind)[0].purpose;
        assert_eq!(Verb::GoToDraw.card(Some(kind)), card);
        assert_eq!(h.editor().export_view().go_card, card);
    }
    assert_eq!(Verb::GoToDraw.card(None), Action::RoomDrawing.help(), "the frozen boot");
    // The tooltip of a template names the step it opens on, by kind.
    assert!(templates::ALL[templates::COMIC].tooltip().contains("opens in the Manga step"));
    assert!(templates::ALL[0].tooltip().contains("opens in the Plan step"));
}

#[test]
fn aukind_quick_start_rows_per_kind() {
    let rows = |kind| welcome::steps_of(kind).iter().map(|s| s.label).collect::<Vec<_>>();
    let animation = ["Draw something", "Add a drawing", "Draw the next pose", "Play it", "Export a file"];
    assert_eq!(rows(Some(ProjectKind::Animation)), animation);
    assert_eq!(rows(None), animation, "the frozen boot");
    assert_eq!(rows(Some(ProjectKind::Drawing)), ["Draw something", "Color it", "Add an effect", "Save it", "Export a file"]);
    assert_eq!(rows(Some(ProjectKind::Comic)), ["Draw on the page", "Cut it into panels", "Add a page", "Save it", "Export pages"]);

    // A drawing's slots tick by their own outcomes, and leave an animation's.
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    h.state_mut().fills_done += 1;
    settle(&mut h);
    assert_eq!(h.editor().ticks(), [false, true, false, false, false]);
    assert_eq!(h.perform(Action::BuildSatsuei), Performed::Done);
    settle(&mut h);
    assert_eq!(h.editor().ticks(), [false, true, true, false, false]);
    h.perform(Action::Save);
    settle(&mut h);
    assert_eq!(h.editor().ticks(), [false, true, true, true, false]);
    assert_eq!(h.editor().quick_start[1..4], [false; 3], "one array, an animation's");
    assert_ne!(h.editor().say.seen & say::DRAWING_SLOTS[0], 0);
    // Show me on a drawing enters the step the row names.
    h.editor_mut().show_me(1);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::InkPaint));
    h.editor_mut().show_me(2);
    settle(&mut h);
    assert_eq!(h.editor().stage, Some(Stage::Composite));

    // A comic's: a panel, a page, a save.
    let mut h = shipped(templates::ALL[templates::COMIC].build());
    settle(&mut h);
    let panels = |h: &EditorHarness| anim_core::manga::panels_of(h.state().cut()).map(|(_, polys)| polys.len());
    assert_eq!(panels(&h), Some(1), "the page's inner frame");
    let add = crate::manga_tools::PanelOp::Add.label();
    h.scroll_to(add);
    h.click(add);
    settle(&mut h);
    assert_eq!(panels(&h), Some(2), "{}", h.state().status);
    assert_eq!(h.editor().ticks(), [false, true, false, false, false]);
    assert_eq!(h.perform(Action::NewPage), Performed::Done);
    settle(&mut h);
    assert_eq!(h.editor().ticks(), [false, true, true, false, false]);
    h.perform(Action::Save);
    settle(&mut h);
    assert_eq!(h.editor().ticks(), [false, true, true, true, false]);
    assert_eq!(h.editor().quick_start[1..4], [false; 3]);
    // Show me arms the Panel tool, and Save it reaches the shell.
    h.editor_mut().show_me(1);
    settle(&mut h);
    assert!(h.editor().canvas.tool == crate::canvas::CanvasTool::Panel);
    h.editor_mut().show_me(3);
    settle(&mut h);
    assert!(h.rig().requests.contains(&crate::AppRequest::Save), "{:?}", h.rig().requests);
}

/// Feedback 2.10: the dots stay, on every kind, and count that kind's own five.
#[test]
fn aukind_dots_count_each_kinds_own_rows() {
    let rows = [
        (ProjectKind::Drawing, "Quick Start: 0 of 5. Next: draw something."),
        (ProjectKind::Comic, "Quick Start: 0 of 5. Next: draw on the page."),
    ];
    for (kind, words) in rows {
        let mut h = shipped(of_kind(kind));
        h.kit_mut().set_size(eframe::egui::vec2(1600.0, 900.0));
        settle(&mut h);
        h.wait(4.2);
        settle(&mut h);
        assert!(h.editor().say.dots_painted, "{kind:?}");
        let dots = h.editor().say.dots_at.expect("where they stand");
        h.event(eframe::egui::Event::PointerMoved(dots.center()));
        h.run_frames(1);
        h.wait(1.5);
        assert!(h.painted_text().iter().any(|w| w == words), "{kind:?}: {:?}", h.painted_text());
    }
    // The ready paper's first screen, and a tick on the drawing's own slot.
    let mut h = EditorHarness::as_shipped();
    settle(&mut h);
    assert!(h.editor().say.dots_painted, "a fresh launch");
    h.state_mut().fills_done += 1;
    settle(&mut h);
    assert_eq!(h.editor().ticks(), [false, true, false, false, false]);
    assert!(h.editor().say.dots_painted);
    let place = say::place_words(h.editor().quick_rows(), &h.editor().ticks());
    assert_eq!(place, "Quick Start: 1 of 5. Next: draw something.");
    // All five in, they are done.
    let bits = say::DRAWING_SLOTS.iter().fold(0, |all, bit| all | bit);
    h.rig_mut().config.ui.hints_seen |= bits;
    h.editor_mut().quick_start = [true; 5];
    settle(&mut h);
    assert_eq!(h.editor().ticks(), [true; 5]);
    assert!(!h.editor().say.dots_painted, "finished");
}

/// A comic is pages, and its first sentence says so. One lesson: one bit.
#[test]
fn aukind_a_comic_hears_of_the_page() {
    for (kind, words) in [
        (ProjectKind::Drawing, "Draw on the paper to begin."),
        (ProjectKind::Animation, "Draw on the paper to begin."),
        (ProjectKind::Comic, "Draw on the page to begin."),
    ] {
        let stage = steps::bar(kind)[0].stage;
        let f = Facts { stage: Some(stage), kind: Some(kind), ..Facts::default() };
        let mut seen = 0;
        let spoken = say::read(&f, &mut seen, None, |_| None).expect("a row applies");
        assert_eq!((spoken.bit, spoken.words.as_str()), (Some(say::NEW_PAPER), words), "{kind:?}");
        seen |= say::NEW_PAPER;
        assert_ne!(say::read(&f, &mut seen, None, |_| None).and_then(|s| s.bit), Some(say::NEW_PAPER), "{kind:?}");
    }
    let h = shipped(templates::ALL[templates::COMIC].build());
    assert_eq!(h.editor().say.hint.as_ref().map(|s| s.words.as_str()), Some("Draw on the page to begin."));
}

/// A panel counts when one is cut. A page that has more than the last one does
/// not tick the row by being turned to.
#[test]
fn aukind_turning_a_page_cuts_no_panel() {
    let mut state = templates::ALL[templates::COMIC].build();
    let (scene, first) = (state.view.scene, state.at().cut);
    state.new_page(scene);
    let second = state.at().cut;
    assert_ne!(first, second);
    let (node, mut polys) = anim_core::manga::panels_of(state.cut()).expect("the page's panels");
    polys.push(polys[0].clone());
    let cut_at = state.at();
    let command = anim_core::manga::set_panels(state.cut(), cut_at, node, polys).expect("a Panels node");
    state.engine.apply("panel", vec![command]).expect("applied");
    state.goto_cut(scene, first);
    let mut h = shipped(state);
    h.state_mut().goto_cut(scene, second);
    settle(&mut h);
    assert_eq!(h.editor().ticks()[1], false, "turned to, not cut");
}

/// A hint's button names a pill of the bar it speaks on, and never one behind
/// it; so does a sentence. `Go to` speaks on an empty step, and names the first.
#[test]
fn aukind_no_hint_points_backward_on_any_bar() {
    let (mut named, mut compared, mut empty) = (0, 0, 0);
    for kind in ProjectKind::ALL {
        let bar = steps::bar(kind);
        let place = |word: &str| bar.iter().position(|s| s.label == word);
        for hint in say::HINTS.iter().filter(|hint| hint.kinds.contains(&kind)) {
            let said = match hint.words {
                Words::Plain(words) => words.split_once("Go to ").map(|(_, word)| word.trim_end_matches('.')),
                _ => None,
            };
            let button = hint.verb.and_then(|v| v.label(Some(kind)).strip_prefix("Go to "));
            let Some(word) = button.or(said) else { continue };
            let there = place(word).unwrap_or_else(|| panic!("{word} is not on {kind:?}'s bar"));
            for here in hint.steps.iter().filter_map(|st| bar.iter().position(|s| s.stage == *st)) {
                assert!(there >= here, "{word} points back on {kind:?}");
                compared += 1;
            }
            named += 1;
        }
        for row in say::EMPTY_STEPS.iter().filter(|row| bar.iter().any(|s| s.stage == row.stage)) {
            let f = Facts { stage: Some(row.stage), kind: Some(kind), ..Facts::default() };
            let verb = say::empty_step(&f, false).and_then(|spoken| spoken.verb).expect("its button");
            assert_eq!(verb.label(Some(kind)).strip_prefix("Go to "), Some(bar[0].label), "{kind:?}");
            empty += 1;
        }
    }
    assert!(named >= 3, "{named}");
    assert!(compared >= 4, "{compared}");
    assert!(empty >= 3, "{empty}");
    // SHARE has a way in with no Play.
    let share = say::HINTS.iter().find(|hint| hint.bit == say::SHARE).expect("a row");
    for (kind, stage) in [(ProjectKind::Drawing, Stage::Drawing), (ProjectKind::Comic, Stage::Manga)] {
        let f = Facts { stage: Some(stage), kind: Some(kind), art: true, ..Facts::default() };
        let spoken = say::read(&f, &mut 0, None, |_| None).expect("a row applies");
        assert_eq!(spoken.bit, Some(share.bit), "{kind:?}");
    }
}

/// Acceptance 9: a drawing's say line never names a frame or Play, and its band
/// holds neither of the animation's keys.
#[test]
fn aukind_a_drawing_never_hears_of_frames() {
    for hint in say::HINTS.iter().filter(|hint| hint.kinds.contains(&ProjectKind::Drawing)) {
        let words = match hint.words {
            Words::Plain(words) => words.to_string(),
            Words::Chord(before, _, after) => format!("{before}{after}"),
            Words::Held | Words::Reason | Words::Exported => continue,
        };
        let lower = words.to_lowercase();
        assert!(!lower.contains("frame") && !lower.contains("play"), "{words:?}");
    }
    // A comic has neither either: F6 there says it has no timing. Alt+A and the
    // menu remain: that is how a person promotes a drawing on purpose.
    for kind in [ProjectKind::Drawing, ProjectKind::Comic] {
        let mut h = shipped(of_kind(kind));
        settle(&mut h);
        assert!(!h.has("Add drawing (band)"), "{kind:?}: {}", h.describe());
        assert!(!h.has(vocab::SHOW_TIMING_SHEET), "{kind:?}: {}", h.describe());
        assert_eq!(h.editor().can_perform(Action::AddDrawingAfter), Ok(()), "{kind:?}");
    }
    // An animation's stand.
    let mut h = EditorHarness::as_animation();
    settle(&mut h);
    assert!(h.has("Add drawing (band)") && h.has(vocab::HIDE_TIMING_SHEET), "{}", h.describe());
}

#[test]
fn aukind_satsuei_refused_on_a_page() {
    let mut h = shipped(a_comic());
    let why = h.state().satsuei_refusal().expect("a page keeps its graph");
    assert_eq!(h.editor().can_perform(Action::BuildSatsuei), Err(why));
    let before = h.state().cut().graph.nodes().count();
    let seq = h.state().refusal_seq;
    h.perform(Action::BuildSatsuei);
    settle(&mut h);
    assert_eq!(h.state().refusal_seq, seq + 1);
    assert_eq!(h.state().refusal, format!("refused — {why}"));
    assert_eq!(h.state().cut().graph.nodes().count(), before, "the page's graph stands");
    // An ordinary cut is not one.
    let h = shipped(an_animation());
    assert_eq!(h.state().satsuei_refusal(), None);
    assert_eq!(h.editor().can_perform(Action::BuildSatsuei), Ok(()));
}
