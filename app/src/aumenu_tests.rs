//! AU11: six plain menus, every command in one known place (spec 6.2, 1.6,
//! 1.11 and AU11). `TABLE` is cut from spec 6.2's own rows, with 1.11's
//! `Help ▸ Lessons`, and the code answers to it.

use anim_core::store;
use eframe::egui;
use egui::{Modifiers, vec2};

use crate::auguide_tests::stand;
use crate::aukeys_tests::{FROZEN, FROZEN_BARE, golden, loaded};
use crate::aulesson_tests::{along, pen};
use crate::aurail_tests::window;
use crate::austrip_tests::strip_of;
use crate::autail_tests::{gap_at, timed};
use crate::cmdpal;
use crate::config::{self, Action, Group};
use crate::doc::AppState;
use crate::lessons::Target;
use crate::measure::{self, Scope};
use crate::menus::{self, HandRow, MENUS, ORDER, Row};
use crate::steps::{self, ProjectKind};
use crate::testkit::EditorHarness;
use crate::welcome::{EXPORT_STEP, WelcomeAction};
use crate::workspace::{Pane, Workspace};
use crate::{AppRequest, Performed, export_step, plate, sample, templates, vocab};

const TABLE: &[(&str, Action)] = &[
    ("File/New…", Action::NewProject),
    ("File/Open…", Action::Open),
    ("File/Save", Action::Save),
    ("File/Save as…", Action::SaveAs),
    ("File/Shots/Previous shot", Action::PrevCut),
    ("File/Shots/Next shot", Action::NextCut),
    ("File/Shots/Make shots from panels", Action::MakeCutsFromPanels),
    ("File/Shots/Add retake note…", Action::AddRetakeNote),
    ("File/Shots/Paper size…", Action::PaperSize),
    ("File/Import/Import sound…", Action::ImportSound),
    ("File/Import/Add sound track…", Action::AddSoundTrack),
    ("File/Import/Import 3D model…", Action::Import3DModel),
    ("File/Import/Reload image sequences", Action::ReloadSequences),
    ("File/Export/Animated GIF", Action::ExportGif),
    ("File/Export/Video (MP4)", Action::ExportMp4),
    ("File/Export/Picture sequence (PNG)", Action::ExportPngSequence),
    ("File/Export/This frame (PNG)", Action::ExportStill),
    ("File/Export/Manga pages (PNG, TIFF)…", Action::ExportPages),
    ("File/Export/EDL (CMX3600)", Action::ExportEdl),
    ("File/Export/Color sheet", Action::ExportColorSheet),
    ("File/Export/Shot as project (for a freelancer)", Action::ExportCutAsProject),
    ("File/Export/Episode", Action::RenderEpisode),
    ("File/Project settings…", Action::ProjectSettings),
    ("File/Settings…", Action::Settings),
    ("File/Quit", Action::Quit),
    ("Edit/Undo", Action::Undo),
    ("Edit/Redo", Action::Redo),
    ("Edit/Cut", Action::EditCut),
    ("Edit/Copy", Action::EditCopy),
    ("Edit/Paste", Action::EditPaste),
    ("Edit/Clipboard/Cut cells", Action::CutCells),
    ("Edit/Clipboard/Copy cells", Action::CopyCells),
    ("Edit/Clipboard/Paste cells", Action::PasteCells),
    ("Edit/Clipboard/Paste cells (insert)", Action::PasteCellsInsert),
    ("Edit/Clipboard/Copy pixels", Action::CopyPixels),
    ("Edit/Clipboard/Paste pixels", Action::PastePixels),
    ("Edit/Select all", Action::SelectAll),
    ("Edit/Clear/Clear layer", Action::ClearCel),
    ("Edit/Clear/Clear whole drawing", Action::ClearCelAll),
    ("Edit/Tools/Brush tool", Action::BrushTool),
    ("Edit/Tools/Eraser tool", Action::ToggleEraser),
    ("Edit/Tools/Select tool", Action::SelectTool),
    ("Edit/Tools/Fill tool", Action::FillTool),
    ("Edit/Tools/Draw to fill tool", Action::LassoFillTool),
    ("Edit/Tools/Keep inside strokes", Action::ToggleAlphaLock),
    ("Edit/Tools/Panel tool", Action::PanelTool),
    ("Edit/Tools/Smaller brush", Action::BrushSmaller),
    ("Edit/Tools/Bigger brush", Action::BrushBigger),
    ("Edit/Tools/Next layer", Action::CycleCelLayer),
    ("Edit/Tools/Previous layer", Action::CycleCelLayerBack),
    ("View/Zoom/Zoom in", Action::ZoomIn),
    ("View/Zoom/Zoom out", Action::ZoomOut),
    ("View/Zoom/Actual size (100%)", Action::Zoom100),
    ("View/Fit", Action::ZoomFit),
    ("View/Rotate/Rotate left 15°", Action::RotateViewLeft),
    ("View/Rotate/Rotate right 15°", Action::RotateViewRight),
    ("View/Rotate/Reset rotation", Action::ResetRotation),
    ("View/Mirror", Action::MirrorView),
    ("View/Ghost frames", Action::ToggleOnion),
    ("View/Preview final", Action::ToggleCompositeView),
    ("View/Full quality", Action::ToggleFinalLook),
    ("View/Canvas options…", Action::CanvasOptions),
    ("View/Check/Miss check", Action::MissCheck),
    ("View/Check/Pin this drawing", Action::PinToLightTable),
    ("View/Check/Shift & trace", Action::ShiftTrace),
    ("View/Check/Reset shift", Action::ResetShift),
    ("View/3D view/Front", Action::View3dFront),
    ("View/3D view/Back", Action::View3dBack),
    ("View/3D view/Right", Action::View3dRight),
    ("View/3D view/Left", Action::View3dLeft),
    ("View/3D view/Top", Action::View3dTop),
    ("View/3D view/Bottom", Action::View3dBottom),
    ("View/3D view/Camera", Action::View3dCamera),
    ("View/3D view/Ortho or perspective", Action::View3dOrtho),
    ("View/3D view/Frame all", Action::View3dFrameAll),
    ("View/3D view/Set shot camera from view", Action::SetCutCameraFromView),
    ("View/3D view/3D underlay", Action::Underlay3d),
    ("View/Hide tools", Action::HideTools),
    ("View/Lock UI positions", Action::ToggleLockPositions),
    ("Animation/Add drawing", Action::AddDrawingAfter),
    ("Animation/Go to/Next frame", Action::NextFrame),
    ("Animation/Go to/Previous frame", Action::PrevFrame),
    ("Animation/Go to/First frame", Action::FirstFrame),
    ("Animation/Go to/Last frame", Action::LastFrame),
    ("Animation/Go to/Next drawing", Action::NextKey),
    ("Animation/Go to/Previous drawing", Action::PrevKey),
    ("Animation/Go to/Go to frame…", Action::GoToFrame),
    ("Animation/Play/Play or pause", Action::PlayPause),
    ("Animation/Play/Sound while dragging", Action::ToggleScrub),
    ("Animation/Play/Render preview to memory", Action::RamPreview),
    ("Animation/Play/Play episode", Action::PlayEpisode),
    ("Animation/Loop/Loop playback", Action::ToggleLoop),
    ("Animation/Loop/Loop start", Action::LoopIn),
    ("Animation/Loop/Loop end", Action::LoopOut),
    ("Animation/Loop/Clear loop range", Action::ClearLoop),
    ("Animation/Insert or delete/New drawing here", Action::NewDrawing),
    ("Animation/Insert or delete/Duplicate drawing", Action::DuplicateCel),
    ("Animation/Insert or delete/Make inbetweens", Action::MakeInbetweens),
    ("Animation/Insert or delete/Type into cell", Action::TypeIntoCell),
    ("Animation/Insert or delete/Remove from timeline", Action::ClearFrameKey),
    ("Animation/Insert or delete/Insert frame", Action::InsertFrames),
    ("Animation/Insert or delete/Delete frame", Action::DeleteFrames),
    ("Animation/Hold/Longer", Action::HoldPlus),
    ("Animation/Hold/Shorter", Action::HoldMinus),
    ("Animation/Retime/On ones", Action::RetimeOnes),
    ("Animation/Retime/On twos", Action::RetimeTwos),
    ("Animation/Retime/On threes", Action::RetimeThrees),
    ("Animation/Retime/Reverse timing", Action::Reverse),
    ("Animation/Mark as/Key pose", Action::KindKey),
    ("Animation/Mark as/Breakdown", Action::KindBreakdown),
    ("Animation/Mark as/Inbetween", Action::KindInbetween),
    ("Animation/Mark as/Unmarked", Action::KindUnset),
    ("Animation/Track/Drawing track", Action::ColumnKindCel),
    ("Animation/Track/Background track", Action::ColumnKindBg),
    ("Animation/Track/Overlay track", Action::ColumnKindBook),
    ("Animation/Track/Reference track", Action::ColumnKindRef),
    ("Animation/Track/Hide track", Action::ToggleColumnHidden),
    ("Animation/Track/Lock track", Action::ToggleColumnLocked),
    ("Animation/Track/Lip flap…", Action::LipFlap),
    ("Animation/Track/Remove track", Action::RemoveColumn),
    ("Animation/Curves/Hold key", Action::InterpHold),
    ("Animation/Curves/Linear", Action::InterpLinear),
    ("Animation/Curves/Ease", Action::InterpEase),
    ("Animation/Curves/Ease in", Action::InterpEaseIn),
    ("Animation/Curves/Ease out", Action::InterpEaseOut),
    ("Animation/Curves/Bezier", Action::InterpBezier),
    ("Animation/Curves/Auto handles", Action::HandlesAuto),
    ("Animation/Curves/Smooth handles", Action::HandlesSmooth),
    ("Animation/Curves/Flat handles", Action::HandlesFlat),
    ("Animation/Curves/Broken handles", Action::HandlesBroken),
    ("Animation/Curves/Frame curves", Action::FrameCurves),
    ("Help/Quick Start", Action::QuickStart),
    ("Help/Lessons", Action::OpenLessons),
    ("Help/Hints", Action::ToggleHints),
    ("Help/Keyboard shortcuts", Action::ShortcutSheet),
    ("Help/Search commands", Action::CommandPalette),
    ("Help/What's New", Action::WhatsNew),
    ("Help/Start screen…", Action::WelcomeScreen),
    ("Help/Example project", Action::OpenSample),
    ("Help/About", Action::About),
    ("More/Manga", Action::MangaRoom),
    ("More/3D layout", Action::Layout3dRoom),
    ("More/Brush maker", Action::BrushMakerRoom),
    ("More/Steps/Plan step", Action::RoomLayout),
    ("More/Steps/Draw step", Action::RoomDrawing),
    ("More/Steps/Timing step", Action::RoomEdit),
    ("More/Steps/Color step", Action::RoomFinishing),
    ("More/Steps/Effects step", Action::RoomComposite),
    ("More/Steps/Export step", Action::RoomExport),
    ("More/Panels/History", Action::History),
    ("More/Panels/Motion curves", Action::ShowCurves),
    ("More/Panels/Check paint", Action::ShowPaintCheck),
    ("More/Panels/Export queue", Action::ShowRenderQueue),
    ("More/Panels/Shots", Action::ShowCutBoard),
    ("More/Panels/Episode timeline", Action::ShowEpisode),
    ("More/Effects setup/Add node…", Action::AddNode),
    ("More/Effects setup/Rebuild from layers", Action::BuildComposite),
    ("More/Effects setup/Full finishing setup", Action::BuildSatsuei),
    ("More/Effects setup/Convert colors to sRGB…", Action::ConvertColorsToSrgb),
    ("More/Manga tools/New page", Action::NewPage),
    ("More/Manga tools/Clip to panels", Action::ClipToPanels),
    ("More/Manga tools/Snap to rulers", Action::ToggleRulerSnap),
    ("More/Reset this step", Action::ResetLayout),
];

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

fn sized(h: &mut EditorHarness, w: f32, tall: f32) {
    h.kit_mut().set_size(vec2(w, tall));
    settle(h);
}

fn centre_of(h: &EditorHarness, pane: Pane) -> egui::Pos2 {
    let found = h.editor().pane_rects.iter().find(|(p, ..)| *p == pane);
    found.unwrap_or_else(|| panic!("{pane:?} stands nowhere")).1.center()
}

/// The pointer over `pane`, and the frames that read it.
fn over(h: &mut EditorHarness, pane: Pane) {
    let at = centre_of(h, pane);
    h.point_at(at);
    settle(h);
}

/// The words a row speaks: `Hold key` from `Animation/Curves/Hold key`.
fn word(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[test]
fn aumenu_six_menus_within_budget() {
    let words: Vec<&str> = MENUS.iter().map(|(word, _)| *word).collect();
    assert_eq!(words, ["File", "Edit", "View", "Animation", "Help", "More"]);
    let rows: Vec<usize> = words.iter().map(|word| menus::menu_rows(word)).collect();
    println!("first-level rows {rows:?}: {}", rows.iter().sum::<usize>());
    assert!(rows.iter().sum::<usize>() <= 60, "{rows:?}");
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    for word in words {
        assert!(h.has(word), "{word}\n{}", h.describe());
    }
    for gone in ["file", "edit", "view", "sheet", "cut", "project", "window", "help"] {
        assert!(!h.has(gone), "{gone} is still a menu");
    }
}

#[test]
fn aumenu_every_action_has_its_table_path() {
    for (path, action) in TABLE {
        assert_eq!(action.menu(), Some(*path), "{action:?}");
    }
    for action in Action::ALL {
        match action.menu() {
            Some(path) => assert!(TABLE.contains(&(path, *action)), "{action:?} at {path} is no row of the table"),
            None => assert_eq!(action.group(), Group::Tool, "{action:?} has no row"),
        }
    }
    let off: Vec<_> = Action::ALL.iter().filter(|a| a.menu().is_none()).collect();
    assert_eq!(off.len(), 8, "Preset1 to Preset8 alone: {off:?}");
    // The canvas window's line and the Steps rows name what is really there.
    assert_eq!(steps::row_of(Action::RoomLayout).map(|s| s.label), Some(vocab::STEP_PLAN));
    assert_eq!(steps::row_of(Action::RoomDrawing).map(|s| s.label), Some(vocab::STEP_DRAW));
}

#[test]
fn aumenu_order_is_the_table() {
    let walked: Vec<Action> = ORDER
        .iter()
        .filter_map(|row| match row {
            Row::A(action) => Some(*action),
            Row::Hand(_) => None,
        })
        .collect();
    let table: Vec<Action> = TABLE.iter().map(|(_, action)| *action).collect();
    assert_eq!(walked, table, "ORDER walks the tables, row for row");
    for action in Action::ALL.iter().filter(|a| a.menu().is_some()) {
        assert_eq!(walked.iter().filter(|a| *a == action).count(), 1, "{action:?}");
    }
    // A path has at most three parts, so nothing nests under a submenu.
    assert!(ORDER.iter().all(|row| (2..=3).contains(&row.path().split('/').count())), "{ORDER:?}");
    // Each menu's rows stand together, in the order of MENUS.
    let mut seen: Vec<&str> = Vec::new();
    for row in ORDER {
        let menu = row.path().split('/').next().unwrap_or("");
        if seen.last() != Some(&menu) {
            assert!(!seen.contains(&menu), "{menu} comes back");
            seen.push(menu);
        }
    }
    let words: Vec<&str> = MENUS.iter().map(|(word, _)| *word).collect();
    assert_eq!(seen, words);
}

#[test]
fn aumenu_no_duplicate_rows_or_spoken_names() {
    let mut paths: Vec<&str> = ORDER.iter().map(|row| row.path()).collect();
    paths.sort_unstable();
    assert!(paths.windows(2).all(|w| w[0] != w[1]), "{paths:?}");
    let mut spoken: Vec<&str> = TABLE.iter().map(|(path, _)| word(path)).collect();
    spoken.sort_unstable();
    assert!(spoken.windows(2).all(|w| w[0] != w[1]), "{spoken:?}");
    // And on the screen: each row answers to one widget where it stands.
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    sized(&mut h, 1600.0, 900.0);
    let mut read = 0;
    for place in menus::places() {
        h.open_path(place);
        settle(&mut h);
        for (path, _) in TABLE.iter().filter(|(path, _)| path.rsplit_once('/').is_some_and(|(p, _)| p == place)) {
            if place == "More/Steps" && !h.has(word(path)) {
                continue;
            }
            assert_eq!(h.query_count(word(path)), 1, "{path}\n{}", h.describe());
            read += 1;
        }
        h.close_menus();
    }
    assert!(read >= TABLE.len() - 1, "{read} of {} rows read", TABLE.len());
}

#[test]
fn aumenu_every_action_is_in_the_palette() {
    let reach = cmdpal::ranked("", &[]);
    assert_eq!(reach.len(), Action::ALL.len(), "palette reach is the registry");
    for action in Action::ALL {
        assert!(reach.contains(action), "{action:?}");
        assert!(cmdpal::ranked(action.label(), &[]).contains(action), "{action:?} by its own name");
    }
    // Spec 6.2: the LAN session lives in Settings, and Search says where.
    for word in ["lan", "host", "join", "network", "room"] {
        assert!(cmdpal::ranked(word, &[]).contains(&Action::Settings), "{word}");
    }
}

#[test]
fn aumenu_every_hand_row_has_a_home() {
    for hand in HandRow::ALL {
        assert_eq!(ORDER.iter().filter(|row| **row == Row::Hand(hand)).count(), 1, "{hand:?}");
        assert!(hand.path().split('/').count() == 3, "{hand:?}");
    }
    let mut state = sample::build_sample();
    state.saved_rev = state.engine.revision();
    let mut h = EditorHarness::with_state_shipped(state);
    sized(&mut h, 1600.0, 900.0);
    let list = Workspace { name: "mine".into(), dock: h.editor().dock.clone(), preset: None, view: None };
    h.editor_mut().workspaces.list.push(list);
    let homes: [(&str, &str); 12] = [
        ("File/Open recent", "Nothing yet"),
        ("File/Shots", "Rename shot…"),
        ("File/Shots", "Add scene"),
        ("File/Import", "XDTS…"),
        ("File/Export", "Other formats…"),
        ("File/Export", "Whole shot"),
        ("More/Steps", "Manga step"),
        ("More/Panels", "Layers panel"),
        ("More/Panels", "Add Result"),
        ("More/Panels", "Canvas in an OS window"),
        ("More/Saved arrangements", "Save this arrangement…"),
        ("More/Saved arrangements", "mine"),
    ];
    for (hand, (place, row)) in HandRow::ALL.into_iter().zip(homes) {
        assert!(hand.path().starts_with(place), "{hand:?} stands at {}", hand.path());
        if hand == HandRow::MangaStep {
            continue; // a comic's alone: `aumenu_steps_submenu_reads_the_bar`
        }
        h.open_path(place);
        settle(&mut h);
        assert!(h.has_text_containing(row), "{place} holds {row}\n{}", h.describe());
        h.close_menus();
    }
    // Every panel opens from a row: a pane is never lost with its old menu.
    let filed = TABLE.iter().filter(|(path, _)| path.starts_with("More/Panels/")).count();
    assert_eq!(filed + menus::window_panes().count() + 1, Pane::ALL.len(), "the Brush pane alone is retired");
    // The old rows' words stayed reachable: export XDTS under the Export step.
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    h.click(export_step::MORE_OPTIONS);
    settle(&mut h);
    h.scroll_to(export_step::XDTS);
    assert!(h.has(export_step::XDTS), "{}", h.describe());
    // On the queue's wide pane it costs the fold no height: it shares a row.
    h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::RenderQueue]);
    settle(&mut h);
    h.scroll_to(export_step::XDTS);
    let (row, latch) = (h.rect(export_step::XDTS), h.rect(export_step::TRANSPARENT));
    assert_eq!(row.center().y, latch.center().y, "{row:?} under {latch:?}");
}

#[test]
fn aumenu_edit_cut_copy_paste_follow_the_last_pane() {
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    over(&mut h, Pane::XSheet);
    let row = |h: &EditorHarness, a| h.editor().edit_row(a);
    assert_eq!(row(&h, Action::EditCut), Some(Action::CutCells));
    assert_eq!(row(&h, Action::EditCopy), Some(Action::CopyCells));
    assert_eq!(row(&h, Action::EditPaste), Some(Action::PasteCells));
    over(&mut h, Pane::Canvas);
    assert_eq!(row(&h, Action::EditCut), None, "there is no CutPixels");
    assert_eq!(row(&h, Action::EditCopy), Some(Action::CopyPixels));
    assert_eq!(row(&h, Action::EditPaste), Some(Action::PastePixels));

    // The row presses for the pane the person left, and stands still while
    // the menu is open over another.
    let at = centre_of(&h, Pane::XSheet);
    h.click_pos(at);
    settle(&mut h);
    h.click(vocab::MENU_EDIT);
    settle(&mut h);
    assert_eq!(h.editor().last_ctx, config::Ctx::Sheet, "{}", h.describe());
    let paper = centre_of(&h, Pane::Canvas);
    h.point_at(paper);
    settle(&mut h);
    assert_eq!(h.editor().last_ctx, config::Ctx::Sheet, "the open menu holds it");
    assert!(h.state().sheet_ui.clip_text.is_none());
    h.click("Copy");
    settle(&mut h);
    assert!(h.state().sheet_ui.clip_text.is_some(), "the sheet's clipboard took it");
    assert_eq!(h.editor().can_perform(Action::EditPaste), Ok(()), "and Paste follows");

    // The chords keep to the pane under the pointer.
    over(&mut h, Pane::XSheet);
    h.key_mods(Modifiers::COMMAND, egui::Key::C);
    assert_eq!(h.performed().last().map(|p| p.0), Some(Action::CopyCells));
    over(&mut h, Pane::Canvas);
    let fired = h.performed().len();
    h.key_mods(Modifiers::COMMAND, egui::Key::C);
    let cells = h.performed()[fired..].iter().filter(|p| p.0 == Action::CopyCells).count();
    assert_eq!(cells, 0, "Ctrl+C over the canvas leaves the sheet's alone");
}

/// Spec 6.2's column `Over the sheet or strip`, by the road a person takes: a
/// press on the strip, out across the paper, up to a top row that holds no pane.
#[test]
fn aumenu_edit_rows_answer_over_the_strip() {
    let mut h = strip_of(timed(&[2, 2, 2]));
    let rects = &h.editor().pane_rects;
    assert_eq!(rects.iter().filter(|(p, ..)| *p == Pane::XSheet).count(), 1, "the strip alone");
    let strip = centre_of(&h, Pane::XSheet);
    h.click_pos(strip);
    settle(&mut h);
    over(&mut h, Pane::Canvas);
    h.point_at(egui::pos2(640.0, 4.0));
    settle(&mut h);
    let ctx = h.ctx();
    let copy = menus::chord_label(&ctx, Action::CopyCells).expect("Copy cells has a chord");
    h.click(vocab::MENU_EDIT);
    settle(&mut h);
    assert_eq!(h.editor().last_ctx, config::Ctx::Sheet, "{}", h.describe());
    assert_eq!(h.node_enabled("Cut"), Some(true), "{}", h.describe());
    assert!(h.painted_text().iter().any(|t| *t == copy), "{:?}", h.painted_text());
    h.click("Copy");
    settle(&mut h);
    assert!(h.state().sheet_ui.clip_text.is_some(), "the cells were copied");
    assert!(h.state().pixel_clip.is_none(), "not the pixels");
}

/// egui_dock moves its focus on a click alone, so a drag on the paper leaves it
/// on the sheet. The press is what counts.
#[test]
fn aumenu_edit_rows_follow_a_drag_on_the_paper() {
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    let sheet = centre_of(&h, Pane::XSheet);
    h.click_pos(sheet);
    settle(&mut h);
    assert_eq!(h.perform(Action::SelectTool), Performed::Done);
    let paper = h.editor().canvas.dbg_rect.center();
    h.drag(paper - vec2(60.0, 40.0), paper + vec2(60.0, 40.0), 6);
    settle(&mut h);
    let focus = h.editor_mut().dock.find_active_focused().map(|(_, pane)| *pane);
    assert_eq!(focus, Some(Pane::XSheet), "the premise: the drag left the focus there");
    h.click(vocab::MENU_EDIT);
    settle(&mut h);
    assert_eq!(h.editor().last_ctx, config::Ctx::Canvas, "{}", h.describe());
    assert_eq!(h.node_enabled("Cut"), Some(false), "{}", h.describe());
    h.click("Copy");
    settle(&mut h);
    assert!(h.state().sheet_ui.clip_text.is_none(), "the sheet's clipboard is left alone");
    assert_eq!(h.editor().edit_row(Action::EditPaste), Some(Action::PastePixels));
}

/// The way to a menu crosses panes, and passing over one is no press. A press
/// on a pane replaces the last, and a popup answers for none.
#[test]
fn aumenu_only_a_press_moves_the_rows() {
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    let sheet = centre_of(&h, Pane::XSheet);
    let paper = h.editor().canvas.dbg_rect.center();
    h.click_pos(sheet);
    settle(&mut h);
    over(&mut h, Pane::Canvas);
    assert_eq!(h.editor().last_ctx, config::Ctx::Sheet, "the way to a menu crosses the paper");
    h.click_pos(paper);
    settle(&mut h);
    assert_eq!(h.editor().last_ctx, config::Ctx::Canvas);
    over(&mut h, Pane::XSheet);
    assert_eq!(h.editor().last_ctx, config::Ctx::Canvas, "and back again");
    h.click_pos(egui::pos2(800.0, 4.0));
    settle(&mut h);
    assert_eq!(h.editor().last_ctx, config::Ctx::Canvas, "the top row is no pane");
}

/// A pressed pane is forgotten once it has left the screen, so a drawing opened
/// over an animation aims no row at a strip it does not have.
#[test]
fn aumenu_a_pressed_pane_goes_with_the_screen() {
    let mut h = strip_of(timed(&[2, 2, 2]));
    let strip = centre_of(&h, Pane::XSheet);
    h.click_pos(strip);
    settle(&mut h);
    assert_eq!(h.editor().last_ctx, config::Ctx::Sheet);
    h.answer_start(WelcomeAction::Intent(templates::PICTURE));
    settle(&mut h);
    assert_eq!(h.editor().kind(), Some(ProjectKind::Drawing));
    assert!(!h.editor().pane_rects.iter().any(|(p, ..)| *p == Pane::XSheet), "{}", h.describe());
    over(&mut h, Pane::Canvas);
    assert_eq!(h.editor().last_ctx, config::Ctx::Canvas, "nothing to answer for the old strip");
    assert_eq!(h.editor().edit_row(Action::EditCopy), Some(Action::CopyPixels));

    // The same when the project stays and only the pane goes. (Not the sheet:
    // it stands on as the strip wherever the dock has no room for it.)
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    let paper = centre_of(&h, Pane::Canvas);
    h.click_pos(paper);
    settle(&mut h);
    assert_eq!(h.editor().last_ctx, config::Ctx::Canvas);
    h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::RenderQueue]);
    settle(&mut h);
    over(&mut h, Pane::RenderQueue);
    assert_eq!(h.editor().last_ctx, config::Ctx::Global, "the paper left the screen");
}

#[test]
fn aumenu_edit_cut_says_why_on_the_canvas() {
    const WHY: &str = "pixels have no Cut. Use Copy, then Clear.";
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    over(&mut h, Pane::Canvas);
    assert_eq!(h.editor().can_perform(Action::EditCut), Err(WHY));
    let at = centre_of(&h, Pane::Canvas);
    h.click_pos(at);
    settle(&mut h);
    h.click(vocab::MENU_EDIT);
    settle(&mut h);
    assert_eq!(h.node_enabled("Cut"), Some(false), "{}", h.describe());
    h.hover("Cut");
    h.wait(1.0);
    assert!(h.has_text_containing(&format!("not now: {WHY}")), "{}", h.describe());
    h.close_menus();
    // The palette's road and a stray call are refused in the same words.
    over(&mut h, Pane::Canvas);
    let frames = h.state().frame_count();
    let ctx = h.ctx();
    assert_eq!(h.editor_mut().invoke(Action::EditCut, &ctx), Performed::Ignored);
    assert_eq!(h.perform(Action::EditCut), Performed::Ignored);
    assert!(h.state().refusal.contains(WHY), "{}", h.state().refusal);
    assert_eq!(h.state().frame_count(), frames);
}

#[test]
fn aumenu_edit_rows_show_the_chord_they_perform() {
    let mut h = EditorHarness::as_animation();
    sized(&mut h, 1600.0, 900.0);
    let ctx = h.ctx();
    let cut = menus::chord_label(&ctx, Action::CutCells).expect("Cut cells has a chord");
    let copy = menus::chord_label(&ctx, Action::CopyCells).expect("Copy cells has a chord");
    let shown = |h: &EditorHarness, chord: &str| h.painted_text().iter().any(|t| t == chord);
    // Over the sheet each row wears its cells' chord.
    let sheet = centre_of(&h, Pane::XSheet);
    h.click_pos(sheet);
    settle(&mut h);
    h.click(vocab::MENU_EDIT);
    settle(&mut h);
    assert!(shown(&h, &cut), "{:?}", h.painted_text());
    assert!(shown(&h, &copy), "{:?}", h.painted_text());
    h.close_menus();
    // Over the canvas, Cut stands for nothing and shows no chord.
    let paper = centre_of(&h, Pane::Canvas);
    h.click_pos(paper);
    settle(&mut h);
    h.click(vocab::MENU_EDIT);
    settle(&mut h);
    assert!(!shown(&h, &cut), "{:?}", h.painted_text());
    let pixels = menus::chord_label(&ctx, Action::CopyPixels).expect("Copy pixels has a chord");
    assert!(shown(&h, &pixels), "{:?}", h.painted_text());
    // None of the three took a chord of its own.
    for action in [Action::EditCut, Action::EditCopy, Action::EditPaste, Action::BrushMakerRoom] {
        assert_eq!(menus::chord_label(&ctx, action), None, "{action:?}");
    }
}

#[test]
fn aumenu_blank_draw_budget() {
    let mut h = EditorHarness::as_shipped();
    window(&mut h, 1600.0, 900.0, 1.0);
    let blank = measure::census(&mut h);
    println!("blank Draw: {} controls, {} words", blank.controls, blank.words);
    assert!(blank.controls <= 30, "{blank:?}");
    assert!(blank.words <= 40, "{blank:?}");
    pen(&mut h, &along([300.0, 300.0], [500.0, 400.0]));
    settle(&mut h);
    let stroked = measure::census(&mut h);
    println!("after a stroke: {} controls, {} words", stroked.controls, stroked.words);
    assert!(h.state().has_art(), "the stroke landed");
    assert!(stroked.controls <= 36, "{stroked:?}");
    assert!(stroked.words <= 40, "{stroked:?}");
}

/// Spec 1.6: the Steps submenu is the bar of the open project.
#[test]
fn aumenu_steps_submenu_reads_the_bar() {
    let kinds: [(ProjectKind, AppState); 3] = [
        (ProjectKind::Drawing, AppState::new_project("one", 640, 360, 24, 300.0)),
        (ProjectKind::Animation, sample::build_sample()),
        (ProjectKind::Comic, templates::ALL[templates::COMIC].build()),
    ];
    for (kind, mut state) in kinds {
        state.set_cut_length(if kind == ProjectKind::Drawing { 1 } else { state.frame_count() });
        let mut h = EditorHarness::with_state_shipped(state);
        sized(&mut h, 1600.0, 900.0);
        assert_eq!(h.editor().kind(), Some(kind));
        h.open_path("More/Steps");
        let mut on: Vec<&str> =
            steps::CATALOG.iter().map(|step| step.label).filter(|label| h.has(&format!("{label} step"))).collect();
        let bar: Vec<&str> = steps::bar(kind).iter().map(|step| step.label).collect();
        on.sort_by_key(|label| bar.iter().position(|step| step == label));
        assert_eq!(on, bar, "{kind:?}\n{}", h.describe());
        if kind == ProjectKind::Comic {
            assert_eq!(on, [vocab::STEP_MANGA, vocab::STEP_COLOR, vocab::STEP_EXPORT]);
            assert!(h.rect("Manga step").top() < h.rect("Color step").top(), "in the bar's order");
            h.close_menus();
            h.editor_mut().enter_room(steps::bar(kind)[1].stage);
            settle(&mut h);
            h.open_path("More/Steps");
            h.click("Manga step");
            settle(&mut h);
            assert_eq!(h.editor().stage, Some(steps::bar(kind)[0].stage), "the row takes the step");
        }
    }
}

/// `BrushMakerRoom` takes the hand row's place, with its stroke guard.
#[test]
fn aumenu_brush_maker_is_a_verb() {
    assert_eq!(Action::BrushMakerRoom.menu(), Some("More/Brush maker"));
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    h.open_path(vocab::MENU_MORE);
    h.click("Brush maker");
    settle(&mut h);
    assert_eq!(h.editor().stage, None, "off the spine");
    assert!(h.editor().dock.find_tab(&Pane::Forge).is_some(), "{}", h.describe());
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    let at = h.editor().canvas.dbg_rect.center();
    h.pen_down(at);
    assert_eq!(h.editor().can_perform(Action::BrushMakerRoom), Err("finish the stroke first"));
    assert_eq!(h.perform(Action::BrushMakerRoom), Performed::Ignored);
    h.pen_up(at);
}

/// AU9 left this: the File rows export with no art, and that earns no tick.
#[test]
fn aumenu_no_art_export_earns_no_tick() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    assert!(!h.state().has_art());
    let ctx = h.ctx();
    let done = h.editor_mut().invoke(Action::ExportStill, &ctx);
    assert_ne!(done, Performed::Ignored, "the row stays live");
    settle(&mut h);
    let file = dir.path().join("blank.png");
    h.answer_export(&file);
    h.run_export_to_end();
    settle(&mut h);
    assert!(file.exists(), "the file was written");
    assert!(!h.editor().quick_start[EXPORT_STEP], "a blank page taught nothing");
    // The same road with a drawing on the paper ticks.
    pen(&mut h, &along([300.0, 300.0], [500.0, 400.0]));
    settle(&mut h);
    assert!(h.state().has_art());
    let ctx = h.ctx();
    h.editor_mut().invoke(Action::ExportStill, &ctx);
    settle(&mut h);
    h.answer_export(&dir.path().join("line.png"));
    h.run_export_to_end();
    settle(&mut h);
    assert!(h.editor().quick_start[EXPORT_STEP], "a real file ticks");
}

/// Spec 6.2: each menu's clip audit passes with the menu open, at both shot
/// sizes and under the narrow `Menu`, on blank paper and on a full project. The
/// full one stands on the strip: the audit tells a float by where text stands,
/// so a sheet's own cut cells under an open menu would read as the menu's.
#[test]
fn aumenu_open_menus_clip_nothing() {
    for (w, tall) in [(1600.0, 900.0), (1280.0, 800.0), (1039.0, 800.0)] {
        for mut h in [EditorHarness::as_shipped(), strip_of(sample::build_sample())] {
            sized(&mut h, w, tall);
            for place in menus::places() {
                h.open_path(place);
                settle(&mut h);
                let clip = measure::clip_here(&h, Scope::Chrome);
                assert_eq!((clip.clipped, clip.outside), (0, 0), "{place} at {w}: {clip:?}");
                h.close_menus();
            }
        }
    }
}

/// Spec 6.2: the narrow row at its thresholds. The feedback spec's 5.5: a
/// `menu.*` mark stands on its word, or on the collapsed `Menu`, until the row
/// that holds it is open, and then on the submenu's head.
#[test]
fn aumenu_narrow_falls_to_1040() {
    assert_eq!((steps::WIDE, steps::WIDE_PLACE), (1040.0, 1100.0));
    assert!(!steps::narrow(1040.0, false) && steps::narrow(1039.0, false));
    assert!(!steps::narrow(1100.0, true) && steps::narrow(1099.0, true));
    static PANELS: [Target; 1] = [Target::Named("menu.more.panels")];
    for (w, word) in [(1040.0, vocab::MENU_MORE), (1039.0, "Menu")] {
        let mut h = EditorHarness::with_state_shipped(sample::build_sample());
        sized(&mut h, w, 800.0);
        let first = h.editor().bar()[0].stage;
        stand(&mut h, &PANELS, first);
        let stands = |h: &EditorHarness| plate::ring_stands(&h.ctx()).expect("the ring stands").1;
        assert!(stands(&h).contains(h.rect(word).center()), "{word} at {w}: {:?}", stands(&h));
        h.open_path("More/Panels");
        assert!(h.has("Motion curves"), "{}", h.describe());
        assert!(stands(&h).contains(h.rect("Panels ⏵").center()), "the head at {w}: {:?}", stands(&h));
    }
}

/// AU26 left this: with no range set the fields read as far as the plain rows
/// above them export, and a person may still reach into the tail.
#[test]
fn aumenu_frame_fields_stop_at_the_last_drawing() {
    let dir = tempfile::tempdir().unwrap();
    let state = gap_at(timed(&[1, 1, 1, 9]), 3);
    assert_eq!((state.frame_count(), state.shown_length()), (12, 3), "12 rows and 3 drawings");
    let mut h = EditorHarness::with_state_shipped(state);
    sized(&mut h, 1600.0, 900.0);
    h.open_path("File/Export");
    settle(&mut h);
    assert_eq!(h.drag_values(), [1.0, 3.0], "{}", h.describe());
    h.close_menus();
    let ctx = h.ctx();
    h.editor_mut().invoke(Action::ExportGif, &ctx);
    settle(&mut h);
    h.answer_export(&dir.path().join("three.gif"));
    let job = &h.state().render_queue.queue.jobs[0];
    assert_eq!((job.frames, job.total()), (Some((0, 2)), 3), "the fields told the truth");
    h.run_export_to_end();
    // A chosen range still runs past the last drawing, and `Whole shot` drops it.
    h.editor_mut().export_range = Some((0, 11));
    h.open_path("File/Export");
    settle(&mut h);
    assert_eq!(h.drag_values(), [1.0, 12.0]);
    h.click("Whole shot");
    settle(&mut h);
    assert_eq!(h.editor().export_range, None);
    assert!(!egui::Popup::is_any_open(&h.ctx()), "a row closes its menu");
}

/// egui shuts a menu on any click inside it, which took a field away before a
/// word was typed. By position, as a pen presses: `focus_text_field` goes
/// through accesskit and never saw it.
#[test]
fn aumenu_a_press_on_a_field_keeps_its_menu() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    sized(&mut h, 1600.0, 900.0);
    h.open_path("File/Shots");
    let field = h.text_field_rect(0).center();
    h.click_pos(field);
    settle(&mut h);
    assert!(egui::Popup::is_any_open(&h.ctx()), "{}", h.describe());
    h.type_text("Ball");
    settle(&mut h);
    assert_eq!(h.editor().rename_buf, "Ball");
    h.click("Rename shot…");
    settle(&mut h);
    assert_eq!(h.state().cut().name, "Ball");
    assert!(!egui::Popup::is_any_open(&h.ctx()), "a row still closes it");

    h.open_path("File/Export");
    h.click_pos(h.rect("Frames").right_center() + vec2(24.0, 0.0));
    settle(&mut h);
    assert!(egui::Popup::is_any_open(&h.ctx()), "{}", h.describe());
    h.close_menus();

    h.open_path("More/Saved arrangements");
    assert_eq!(h.node_enabled("Save"), Some(false), "no name, no Save\n{}", h.describe());
    assert!(h.painted_text().iter().any(|t| t == "Name"), "{:?}", h.painted_text());
    let field = h.text_field_rect(0).center();
    h.click_pos(field);
    settle(&mut h);
    assert!(egui::Popup::is_any_open(&h.ctx()), "{}", h.describe());
    h.type_text("Mine");
    settle(&mut h);
    assert_eq!(h.node_enabled("Save"), Some(true));
    h.click("Save");
    settle(&mut h);
    assert_eq!(h.editor().workspaces.list.last().map(|w| w.name.as_str()), Some("Mine"));
    assert!(!egui::Popup::is_any_open(&h.ctx()));
}

/// Spec 1.5 rule 10: both routes reach the App's dialogs, read off the log.
#[test]
fn aumenu_xdts_rows_ask_the_app() {
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    sized(&mut h, 1600.0, 900.0);
    h.open_path("File/Import");
    h.click("XDTS…");
    settle(&mut h);
    assert!(h.rig().requests.contains(&AppRequest::ImportXdts), "{:?}", h.rig().requests);
    assert_eq!(h.perform(Action::RoomExport), Performed::Done);
    settle(&mut h);
    h.click(export_step::MORE_OPTIONS);
    settle(&mut h);
    h.scroll_to(export_step::XDTS);
    h.click(export_step::XDTS);
    settle(&mut h);
    assert!(h.rig().requests.contains(&AppRequest::ExportXdts), "{:?}", h.rig().requests);
}

/// Spec AU11 Build 3: the words where the docked canvas stood.
#[test]
fn aumenu_canvas_window_says_where_it_went() {
    const LINE: &str = "Canvas is open in its own window. More ▸ Panels ▸ brings it back.";
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    assert!(!h.has_text_containing(LINE));
    h.editor_mut().float_canvas.open = true;
    settle(&mut h);
    assert!(h.has_text_containing(LINE), "{}", h.describe());
    // The row it names is `aumenu_every_hand_row_has_a_home`'s OS windows row.
    assert!(HandRow::OsWindows.path().starts_with("More/Panels/"));
}

/// A menu row wears what is printed on the key. The stored chord keeps egui's
/// name, which is the file's vocabulary (`aukeys_tests::FROZEN`).
#[test]
fn aumenu_chords_read_as_the_keys_read() {
    let mut h = EditorHarness::as_shipped();
    sized(&mut h, 1600.0, 900.0);
    let ctx = h.ctx();
    for (action, chord) in [
        (Action::BrushSmaller, "["),
        (Action::BrushBigger, "]"),
        (Action::RotateViewLeft, "Ctrl+["),
        (Action::RotateViewRight, "Ctrl+]"),
        (Action::ResetRotation, "Ctrl+\\"),
        (Action::ZoomOut, "Ctrl+-"),
        (Action::ZoomIn, "Ctrl+= / Ctrl+Plus"),
    ] {
        assert_eq!(menus::chord_label(&ctx, action).as_deref(), Some(chord), "{action:?}");
    }
    h.open_path("Edit/Tools");
    let painted = h.painted_text();
    assert!(painted.iter().any(|t| t == "["), "{painted:?}");
    assert!(!painted.iter().any(|t| t.contains("Bracket")), "{painted:?}");
    let config = config::Config::default();
    let stored = serde_json::to_string(config.binding(Action::BrushSmaller).unwrap()).unwrap();
    assert!(stored.contains("\"OpenBracket\""), "{stored}");
}

/// Spec AU11's `Must not change`: Actions only grow, every chord and config a
/// person has reads as it did, and the menus add nothing to a project.
/// `aulook_config_shape_stands` holds the config's keys, and `aukeys_tests`
/// each chord.
#[test]
fn aumenu_stored_words_stand() {
    const NEW: [Action; 4] = [Action::BrushMakerRoom, Action::EditCut, Action::EditCopy, Action::EditPaste];
    let old: Vec<&str> = FROZEN.iter().map(|(name, _)| *name).chain(FROZEN_BARE).collect();
    for name in &old {
        let action: Result<Action, _> = serde_json::from_value(serde_json::json!(name));
        assert!(action.is_ok(), "{name} is a stored word and still reads");
    }
    for action in NEW {
        let word = serde_json::to_value(action).unwrap();
        assert_eq!(word, format!("{action:?}"), "a variant's own name");
        assert!(!old.contains(&word.as_str().unwrap()), "{action:?} is new");
    }
    // An old config takes the new verbs unbound, and keeps them that way.
    let (_dir, config) = loaded(&golden("Gaban"));
    for action in NEW {
        let binding = config.binding(action).expect("fill_missing brought it in");
        assert_eq!(binding.chords().count(), 0, "{action:?} took a chord");
        assert_eq!(config::Config::default().binding(action).map(|b| b.chords().count()), Some(0));
    }
    // A project saved after the new rows is the project saved before them.
    assert!(include_str!("../../crates/anim-core/src/store.rs").contains("const SCHEMA_VERSION: i64 = 11;"));
    let dir = tempfile::tempdir().unwrap();
    let (before, after) = (dir.path().join("before.animproj"), dir.path().join("after.animproj"));
    let mut h = EditorHarness::with_state_shipped(sample::build_sample());
    sized(&mut h, 1600.0, 900.0);
    store::save(&h.state().engine.project, &before).expect("saved");
    for action in [Action::EditCopy, Action::BrushMakerRoom] {
        let ctx = h.ctx();
        h.editor_mut().invoke(action, &ctx);
        settle(&mut h);
    }
    store::save(&h.state().engine.project, &after).expect("saved");
    assert_eq!(std::fs::read(&before).unwrap(), std::fs::read(&after).unwrap());
}
