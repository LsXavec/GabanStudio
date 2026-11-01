//! GM3: THE MENU BAR, built from the one verb registry (D6): every action
//! with a menu path ("Edit/Undo") is an item there, showing its chord, a
//! one-line tooltip, and — when it cannot run now — greyed with the reason
//! (`Editor::can_perform`). A click goes through `Editor::invoke`, the same
//! dispatch a shortcut takes. Also the help windows: F1's shortcut sheet,
//! quick start, about.

use std::sync::Arc;

use eframe::egui;
use egui::containers::menu::{MenuConfig, SubMenuButton};

use crate::config::{Action, Binding, BrushPreset, Group};
use crate::lessons::Target;
use crate::plate;
use crate::steps::ProjectKind;
use crate::vocab;

/// The help windows the Help menu and F1 open.
#[derive(Default)]
pub struct HelpWindows {
    /// F1: every bound action, grouped and searchable.
    pub sheet: bool,
    pub sheet_query: String,
    pub quick_start: bool,
    pub about: bool,
}

fn chords_id() -> egui::Id {
    egui::Id::new("gaban_chords")
}

/// Publish this frame's bindings, so controls drawn deep inside panes (the
/// canvas deck) can name their chords in their tooltips.
pub fn publish(ctx: &egui::Context, binds: &[Binding]) {
    let same = ctx.data(|d| {
        d.get_temp::<Arc<Vec<Binding>>>(chords_id())
            .is_some_and(|b| b.as_slice() == binds)
    });
    if !same {
        ctx.data_mut(|d| d.insert_temp(chords_id(), Arc::new(binds.to_vec())));
    }
}

/// The published chord label of an action ("Ctrl+Y / Ctrl+Shift+Z").
pub fn chord_label(ctx: &egui::Context, action: Action) -> Option<String> {
    let binds = ctx.data(|d| d.get_temp::<Arc<Vec<Binding>>>(chords_id()))?;
    crate::cmdpal::chord_of(&binds, action)
}

/// AU10: the first of them, for a key's name: a key answers to the chord you
/// would press, and its card lists the rest.
pub fn first_chord(ctx: &egui::Context, action: Action) -> Option<String> {
    let binds = ctx.data(|d| d.get_temp::<Arc<Vec<Binding>>>(chords_id()))?;
    Some(binds.iter().find(|b| b.action == action)?.chords().next()?.label())
}

/// "Name (chord) — help": the one tooltip shape for menu items, deck
/// buttons and palette rows.
pub fn tip(ctx: &egui::Context, action: Action) -> String {
    tip_with(action, chord_label(ctx, action))
}

pub fn tip_with(action: Action, chord: Option<String>) -> String {
    match chord {
        Some(c) => format!("{} ({c}) — {}", action.label(), action.help()),
        None => format!("{} — {}", action.label(), action.help()),
    }
}

/// The registry tooltip with `more` on its own line, built only while the
/// control is hovered (never every frame).
pub fn hover_tip(resp: egui::Response, action: Action, more: &str) -> egui::Response {
    hover_tip_with(resp, action, || more.to_string())
}

/// [`hover_tip`] whose extra line is itself built only on hover.
pub fn hover_tip_with(
    resp: egui::Response,
    action: Action,
    more: impl FnOnce() -> String,
) -> egui::Response {
    let ctx = resp.ctx.clone();
    plate::mark_row(&resp, Target::Action(action));
    crate::plate::hover_card(resp, || {
        let mut text = tip(&ctx, action);
        let more = more();
        if !more.is_empty() {
            text.push('\n');
            text.push_str(&more);
        }
        text
    })
}

/// The shortcut sheet's rows for `query`: every action with a chord, in
/// group order, as (action, chord label). Pure.
pub fn sheet_rows(binds: &[Binding], query: &str) -> Vec<(Action, String)> {
    let q = query.trim().to_lowercase();
    let mut rows = Vec::new();
    for g in Group::ALL {
        for a in Action::ALL.iter().filter(|a| a.group() == *g) {
            let Some(chord) = crate::cmdpal::chord_of(binds, *a) else {
                continue;
            };
            let hit = q.is_empty()
                || a.label().to_lowercase().contains(&q)
                || chord.to_lowercase().contains(&q)
                || a.help().to_lowercase().contains(&q);
            if hit {
                rows.push((*a, chord));
            }
        }
    }
    rows
}

/// GM7: the canvas gestures no chord names — a modifier on a tool's click
/// or drag — as (gesture, what it does). F1's sheet lists them under its
/// keys, so a refill is found where every other key is.
pub const GESTURES: [(&str, &str); 5] = [
    ("Space+drag", "move the view (Shift+drag turns it)"),
    ("Alt+click", "pick a colour (a chip's exact colour arms the chip)"),
    ("Shift+click with the pen", "rule on from the last stroke"),
    ("Shift+click with fill", "refill the clicked flat in the brush colour (nuri-naoshi)"),
    ("Ctrl+Shift+click with fill", "replace that colour on the whole layer"),
];

/// The gesture rows for `query`, matched like [`sheet_rows`]. Pure.
pub fn gesture_rows(query: &str) -> Vec<(&'static str, &'static str)> {
    let q = query.trim().to_lowercase();
    GESTURES
        .iter()
        .filter(|(gesture, what)| {
            q.is_empty() || gesture.to_lowercase().contains(&q) || what.to_lowercase().contains(&q)
        })
        .copied()
        .collect()
}

/// AU11: a row with no `Action`, drawn by hand at its place in [`ORDER`]. Each
/// has its home (spec 6.2's table of hand-built rows), so none is lost.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum HandRow {
    OpenRecent,
    ShotNames,
    ShotTree,
    Xdts,
    OtherFormats,
    FrameRange,
    MangaStep,
    OtherPanes,
    AddResult,
    OsWindows,
    SaveArrangement,
    Arrangements,
}

impl HandRow {
    #[cfg(test)]
    pub const ALL: [HandRow; 12] = [
        HandRow::OpenRecent,
        HandRow::ShotNames,
        HandRow::ShotTree,
        HandRow::Xdts,
        HandRow::OtherFormats,
        HandRow::FrameRange,
        HandRow::MangaStep,
        HandRow::OtherPanes,
        HandRow::AddResult,
        HandRow::OsWindows,
        HandRow::SaveArrangement,
        HandRow::Arrangements,
    ];

    /// Where it stands. A path of three parts, as a filed row's: the last
    /// names the row for a reader, because a hand row draws its own words.
    pub const fn path(self) -> &'static str {
        match self {
            HandRow::OpenRecent => "File/Open recent/each recent file",
            HandRow::ShotNames => "File/Shots/Rename shot…",
            HandRow::ShotTree => "File/Shots/the shot tree",
            HandRow::Xdts => "File/Import/XDTS…",
            HandRow::OtherFormats => "File/Export/Other formats…",
            HandRow::FrameRange => "File/Export/Frame range",
            HandRow::MangaStep => "More/Steps/Manga step",
            HandRow::OtherPanes => "More/Panels/each other panel",
            HandRow::AddResult => "More/Panels/Add Result",
            HandRow::OsWindows => "More/Panels/the OS windows",
            HandRow::SaveArrangement => "More/Saved arrangements/Save this arrangement…",
            HandRow::Arrangements => "More/Saved arrangements/each saved arrangement",
        }
    }

    /// It holds a field to type or drag in.
    const fn has_field(self) -> bool {
        matches!(self, HandRow::ShotNames | HandRow::FrameRange | HandRow::SaveArrangement)
    }
}

/// AU11: one row of [`ORDER`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Row {
    A(Action),
    Hand(HandRow),
}

impl Row {
    pub fn path(self) -> &'static str {
        match self {
            Row::A(action) => action.menu().unwrap_or(""),
            Row::Hand(hand) => hand.path(),
        }
    }
}

/// AU11: the menus, in the order they show their rows (spec 6.2): every
/// `Action` with a path once, and the hand rows at their places. A submenu
/// opens where its first row sits.
pub const ORDER: &[Row] = {
    use Action::*;
    use HandRow::*;
    use Row::{A, Hand};
    &[
        A(NewProject), A(Open), Hand(OpenRecent), A(Save), A(SaveAs),
        A(PrevCut), A(NextCut), A(MakeCutsFromPanels), A(AddRetakeNote), A(PaperSize), Hand(ShotNames), Hand(ShotTree),
        A(ImportSound), A(AddSoundTrack), A(Import3DModel), A(ReloadSequences), Hand(Xdts),
        A(ExportGif), A(ExportMp4), A(ExportPngSequence), A(ExportStill), Hand(OtherFormats),
        A(ExportPages), A(ExportEdl), A(ExportColorSheet), A(ExportCutAsProject), A(RenderEpisode), Hand(FrameRange),
        A(ProjectSettings), A(Settings), A(Quit),

        A(Undo), A(Redo), A(EditCut), A(EditCopy), A(EditPaste),
        A(CutCells), A(CopyCells), A(PasteCells), A(PasteCellsInsert), A(CopyPixels), A(PastePixels),
        A(SelectAll),
        A(ClearCel), A(ClearCelAll),
        A(BrushTool), A(ToggleEraser), A(SelectTool), A(FillTool), A(LassoFillTool), A(ToggleAlphaLock),
        A(PanelTool), A(BrushSmaller), A(BrushBigger), A(CycleCelLayer), A(CycleCelLayerBack),

        A(ZoomIn), A(ZoomOut), A(Zoom100),
        A(ZoomFit),
        A(RotateViewLeft), A(RotateViewRight), A(ResetRotation),
        A(MirrorView), A(ToggleOnion), A(ToggleCompositeView), A(ToggleFinalLook), A(CanvasOptions),
        A(MissCheck), A(PinToLightTable), A(ShiftTrace), A(ResetShift),
        A(View3dFront), A(View3dBack), A(View3dRight), A(View3dLeft), A(View3dTop), A(View3dBottom),
        A(View3dCamera), A(View3dOrtho), A(View3dFrameAll), A(SetCutCameraFromView), A(Underlay3d),
        A(HideTools), A(ToggleLockPositions),

        A(AddDrawingAfter),
        A(NextFrame), A(PrevFrame), A(FirstFrame), A(LastFrame), A(NextKey), A(PrevKey), A(GoToFrame),
        A(PlayPause), A(ToggleScrub), A(RamPreview), A(PlayEpisode),
        A(ToggleLoop), A(LoopIn), A(LoopOut), A(ClearLoop),
        A(NewDrawing), A(DuplicateCel), A(MakeInbetweens), A(TypeIntoCell), A(ClearFrameKey), A(InsertFrames),
        A(DeleteFrames),
        A(HoldPlus), A(HoldMinus),
        A(RetimeOnes), A(RetimeTwos), A(RetimeThrees), A(Reverse),
        A(KindKey), A(KindBreakdown), A(KindInbetween), A(KindUnset),
        A(ColumnKindCel), A(ColumnKindBg), A(ColumnKindBook), A(ColumnKindRef), A(ToggleColumnHidden),
        A(ToggleColumnLocked), A(LipFlap), A(RemoveColumn),
        A(InterpHold), A(InterpLinear), A(InterpEase), A(InterpEaseIn), A(InterpEaseOut), A(InterpBezier),
        A(HandlesAuto), A(HandlesSmooth), A(HandlesFlat), A(HandlesBroken), A(FrameCurves),

        A(QuickStart), A(OpenLessons), A(ToggleHints), A(ShortcutSheet), A(CommandPalette), A(WhatsNew),
        A(WelcomeScreen), A(OpenSample), A(About),

        A(MangaRoom), A(Layout3dRoom), A(BrushMakerRoom),
        Hand(MangaStep), A(RoomLayout), A(RoomDrawing), A(RoomEdit), A(RoomFinishing), A(RoomComposite), A(RoomExport),
        A(History), A(ShowCurves), A(ShowPaintCheck), A(ShowRenderQueue), A(ShowCutBoard), A(ShowEpisode),
        Hand(OtherPanes), Hand(AddResult), Hand(OsWindows),
        A(AddNode), A(BuildComposite), A(BuildSatsuei), A(ConvertColorsToSrgb),
        A(NewPage), A(ClipToPanels), A(ToggleRulerSnap),
        Hand(SaveArrangement), Hand(Arrangements),
        A(ResetLayout),
    ]
};

/// AU11: the six menus, and the `menu.*` handles a word holds (spec 5.5). A
/// submenu's handle stands on its word until the menu is open: a row open
/// inside marks first.
pub const MENUS: [(&str, &[&str]); 6] = [
    (vocab::MENU_FILE, &[]),
    (vocab::MENU_EDIT, &[]),
    (vocab::MENU_VIEW, &["menu.view"]),
    (vocab::MENU_ANIMATION, &["menu.animation", "menu.animation.track"]),
    (vocab::MENU_HELP, &[]),
    (vocab::MENU_MORE, &["menu.more", "menu.more.panels"]),
];

/// The handles a submenu's head holds, for a lesson's chain to walk through.
fn head_handles(menu: &str, sub: &str) -> &'static [&'static str] {
    match (menu, sub) {
        (vocab::MENU_ANIMATION, "Track") => &["menu.animation.track"],
        (vocab::MENU_MORE, "Panels") => &["menu.more.panels"],
        _ => &[],
    }
}

/// AU21: `handles` stand on this menu word, whatever it reads.
pub fn mark_word(resp: &egui::Response, handles: &[&'static str]) {
    for handle in handles {
        plate::mark_on(&resp.ctx, resp.layer_id, resp.interact_rect, Target::Named(handle));
    }
}

/// AU21: the collapsed `Menu` marks every handle. A row open inside it marked
/// first, so this stands only until its row is open.
pub fn mark_narrow(resp: &egui::Response) {
    for (_, handles) in MENUS {
        mark_word(resp, handles);
    }
}

/// The rows of `menu` in [`ORDER`], each with its path under the menu.
fn rows_of(menu: &str) -> impl Iterator<Item = (Row, &'static str)> {
    ORDER.iter().filter_map(move |row| {
        let rest = row.path().strip_prefix(menu)?.strip_prefix('/')?;
        Some((*row, rest))
    })
}

/// The rows `menu` stands at: one for an item, one for a submenu.
pub(crate) fn menu_rows(menu: &str) -> usize {
    let rows: std::collections::BTreeSet<&str> =
        rows_of(menu).map(|(_, rest)| rest.split('/').next().unwrap_or(rest)).collect();
    rows.len()
}

/// Every place a menu opens, by path: the six words, then each submenu.
#[cfg(test)]
pub(crate) fn places() -> Vec<&'static str> {
    let mut places: Vec<&str> = MENUS.iter().map(|(word, _)| *word).collect();
    for row in ORDER {
        if let Some((parent, _)) = row.path().rsplit_once('/')
            && !places.contains(&parent)
        {
            places.push(parent);
        }
    }
    places
}

/// The panes More ▸ Panels ▸ lists by hand. The Brush window is RETIRED (owner
/// 2026-08-17): its settings live on the canvas deck + right rail. The others
/// left out are the registry's own rows.
pub(crate) fn window_panes() -> impl Iterator<Item = &'static crate::Pane> {
    use crate::Pane;
    let filed = |p: &&Pane| {
        matches!(
            **p,
            Pane::Brush | Pane::History | Pane::Curves | Pane::PaintCheck | Pane::RenderQueue | Pane::Board | Pane::Episode
        )
    };
    Pane::ALL.iter().filter(move |p| !filed(p))
}

/// AU21: a menu `tall` high that the window cannot hold scrolls (a shorter one
/// the popup lifts to fit). Under the edge a row has no rect: no pointer reaches
/// it, and neither does a ring. egui keeps a scroll's offset by its id, so the
/// frame a menu opens starts it at its first row.
pub(crate) fn scroll_menu(ui: &mut egui::Ui, tall: f32, body: impl FnOnce(&mut egui::Ui)) {
    let window = ui.ctx().content_rect().height();
    if tall <= window {
        return body(ui);
    }
    let room = window - ui.spacing().menu_margin.sum().y;
    let seen = ui.id().with("menu_seen");
    let frame = ui.ctx().cumulative_frame_nr();
    let last = ui.ctx().data_mut(|d| {
        let last = d.get_temp::<u64>(seen);
        d.insert_temp(seen, frame);
        last
    });
    let mut area = egui::ScrollArea::vertical().max_height(room);
    if last.is_none_or(|at| at + 1 < frame) {
        area = area.vertical_scroll_offset(0.0);
    }
    let shown = area.show(ui, body);
    plate::note_viewport(ui, shown.inner_rect, [false, true]);
}

/// AU21: a menu laid by hand, of about `rows` rows: its fields and tree keep
/// their own heights, so one past the window at the full row height scrolls
/// whatever the registry's rows compact to.
pub(crate) fn scroll_laid_menu(ui: &mut egui::Ui, rows: usize, body: impl FnOnce(&mut egui::Ui)) {
    let spacing = ui.spacing();
    let full = rows as f32 * (plate::CTRL_H + spacing.item_spacing.y) + spacing.menu_margin.sum().y;
    plate::fit_menu(ui, rows);
    scroll_menu(ui, full, body);
}

impl crate::Editor {
    /// One action as a menu item: the last segment of its menu path, its
    /// chord at the right, a one-line tooltip; greyed with the reason when
    /// it cannot run now.
    pub(crate) fn menu_item(&mut self, ui: &mut egui::Ui, action: Action) {
        let Some(path) = action.menu() else {
            return;
        };
        self.action_row(ui, action, path.rsplit('/').next().unwrap_or(path));
    }

    /// [`Self::menu_item`] under `name`.
    fn action_row(&mut self, ui: &mut egui::Ui, action: Action, name: &str) {
        // AU11: a merged Edit row shows the chord of what it would perform.
        let chord = chord_label(ui.ctx(), self.edit_row(action).unwrap_or(action));
        let can = self.can_perform(action);
        // A toggle shows whether it is on.
        let on = self.is_on(action);
        let mut button = egui::Button::new(name).selected(on.unwrap_or(false));
        if let Some(c) = &chord {
            button = button.shortcut_text(c.as_str());
        }
        let resp = ui.add_enabled(can.is_ok(), button);
        // GM18: over a graph shaped by hand, Clip to panels is DANGER (the
        // Aka edge; its note says what goes).
        if action == Action::ClipToPanels && self.state.page_hand_built().is_some() {
            ui.painter().rect_stroke(
                resp.rect,
                plate::R_CONTROL,
                egui::Stroke::new(1.0, plate::AKA),
                egui::StrokeKind::Inside,
            );
        }
        // The chord is drawn, not spoken: the item answers to its name.
        plate::speak(&resp, egui::WidgetType::Button, on, name);
        plate::mark_row(&resp, Target::Action(action));
        let mut tip = tip_with(action, chord);
        if let Some(note) = self.menu_note(action) {
            tip.push('\n');
            tip.push_str(&note);
        }
        let resp = match can {
            Ok(()) => resp.on_hover_text(tip),
            Err(why) => resp.on_disabled_hover_text(format!("{tip}\nnot now: {why}")),
        };
        if resp.clicked() {
            ui.close();
            self.invoke(action, ui.ctx());
        }
    }

    /// AU11: the top row's six words, each walking [`ORDER`].
    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui, presets: &[BrushPreset]) {
        let file_menu_was_open = std::mem::take(&mut self.file_menu_open);
        for (menu, handles) in MENUS {
            let word = plate::menu_word(ui, menu, |ui| {
                if menu == vocab::MENU_FILE {
                    // GM2: the recent files are checked on disk as the menu
                    // opens, not on every frame it stays open.
                    self.file_menu_open = true;
                    if !file_menu_was_open {
                        self.recent_shown = crate::worksafe::on_disk(&self.recent);
                    }
                }
                self.menu_items(ui, menu, presets);
            });
            mark_word(&word, handles);
        }
    }

    /// `menu`'s rows in [`ORDER`]. GM14: a three-part path files the row in a
    /// submenu, opened where its first row sits.
    fn menu_items(&mut self, ui: &mut egui::Ui, menu: &'static str, presets: &[BrushPreset]) {
        let tall = plate::fit_menu(ui, menu_rows(menu));
        scroll_menu(ui, tall, |ui| {
            let mut subs: Vec<&str> = Vec::new();
            for (row, rest) in rows_of(menu) {
                match rest.split_once('/') {
                    Some((sub, _)) if !subs.contains(&sub) => {
                        subs.push(sub);
                        self.submenu(ui, menu, sub, presets);
                    }
                    Some(_) => {}
                    None => self.row(ui, row, presets),
                }
            }
        });
    }

    fn submenu(&mut self, ui: &mut egui::Ui, menu: &str, sub: &str, presets: &[BrushPreset]) {
        let rows: Vec<Row> = self.sub_rows(menu, sub);
        // egui shuts a menu on any click inside it, so a click on a field
        // would take the field away. These close from their rows instead.
        let mut config = MenuConfig::find(ui);
        if rows.iter().any(|row| matches!(row, Row::Hand(hand) if hand.has_field())) {
            config = config.close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        }
        let (head, _) = SubMenuButton::new(sub).config(config).ui(ui, |ui| {
            // AU9: MP4's row reads the add-on, so it is looked for here.
            if sub == "Export" {
                self.state.render_queue.search_ffmpeg();
            }
            let tall: usize = rows.iter().map(|row| self.row_height(*row)).sum();
            scroll_laid_menu(ui, tall, |ui| {
                for row in rows {
                    self.row(ui, row, presets);
                }
            });
        });
        mark_word(&head, head_handles(menu, sub));
    }

    /// A submenu's rows. AU18: `More ▸ Steps ▸` is the bar of the open project.
    fn sub_rows(&self, menu: &str, sub: &str) -> Vec<Row> {
        let on_bar = |label: &str| self.bar().iter().any(|on| on.label == label);
        let shown = |row: &Row| match row {
            Row::A(action) => crate::steps::row_of(*action).is_none_or(|step| on_bar(step.label)),
            Row::Hand(HandRow::MangaStep) => on_bar(vocab::STEP_MANGA),
            Row::Hand(_) => true,
        };
        let filed = rows_of(menu).filter(|(_, rest)| rest.split_once('/').is_some_and(|(s, _)| s == sub));
        filed.map(|(row, _)| row).filter(shown).collect()
    }

    /// How many rows' height `row` stands, for a menu laid by hand.
    fn row_height(&self, row: Row) -> usize {
        match row {
            Row::A(_)
            | Row::Hand(HandRow::Xdts | HandRow::OtherFormats | HandRow::MangaStep | HandRow::AddResult) => 1,
            Row::Hand(HandRow::SaveArrangement | HandRow::OsWindows) => 2,
            Row::Hand(HandRow::ShotNames) => 3,
            Row::Hand(HandRow::FrameRange) => 3,
            Row::Hand(HandRow::OpenRecent) => self.recent_shown.len().max(1) + 2,
            Row::Hand(HandRow::OtherPanes) => window_panes().count(),
            Row::Hand(HandRow::Arrangements) => self.workspaces.list.len(),
            Row::Hand(HandRow::ShotTree) => {
                let scenes = &self.state.engine.project.scenes;
                scenes.iter().map(|s| s.cuts.len() + 2).sum::<usize>() + 2
            }
        }
    }

    fn row(&mut self, ui: &mut egui::Ui, row: Row, presets: &[BrushPreset]) {
        match row {
            Row::A(action) => self.menu_item(ui, action),
            Row::Hand(HandRow::OpenRecent) => self.recent_rows(ui),
            Row::Hand(HandRow::ShotNames) => self.shot_names(ui),
            Row::Hand(HandRow::ShotTree) => self.shot_tree(ui),
            Row::Hand(HandRow::Xdts) => self.xdts_row(ui),
            Row::Hand(HandRow::OtherFormats) => self.other_formats_row(ui),
            Row::Hand(HandRow::FrameRange) => self.frame_range(ui),
            // A comic's bar opens on Manga, and the verb's own row stays at
            // `More ▸ Manga` for the projects whose bar lacks it.
            Row::Hand(HandRow::MangaStep) => self.action_row(ui, Action::MangaRoom, "Manga step"),
            Row::Hand(HandRow::OtherPanes) => self.pane_rows(ui),
            Row::Hand(HandRow::AddResult) => self.add_result(ui),
            Row::Hand(HandRow::OsWindows) => self.os_windows(ui),
            Row::Hand(HandRow::SaveArrangement) => self.save_arrangement(ui),
            Row::Hand(HandRow::Arrangements) => self.arrangements(ui, presets),
        }
    }

    /// `File ▸ Open recent ▸`: New, Open and these go through the unsaved
    /// guard (GM2).
    fn recent_rows(&mut self, ui: &mut egui::Ui) {
        if self.recent_shown.is_empty() {
            ui.label(egui::RichText::new("Nothing yet").color(plate::legend_dim()));
        }
        for (p, present) in self.recent_shown.clone() {
            // Cut files repeat across episode folders (ep01/c012, ep02/c012):
            // the folder tells them apart at a glance.
            let name = crate::recent_label(&p);
            if present {
                if ui.button(&name).on_hover_text(p.display().to_string()).clicked() {
                    self.leave(crate::Leave::OpenPath(p), ui.ctx());
                    ui.close();
                }
            } else {
                // Missing: greyed, with its own Remove.
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{name} (missing)")).color(plate::legend_dim()))
                        .on_hover_text(p.display().to_string());
                    if ui.small_button("Remove").clicked() {
                        self.recent_edit = Some(crate::RecentEdit::Remove(p.clone()));
                    }
                });
            }
        }
        if !self.recent_shown.is_empty() {
            ui.separator();
            // Forgets the list only; no file is touched.
            let clear = ui.button("Clear recent").on_hover_text("Empties this list. No file is removed.");
            if clear.clicked() {
                self.recent_edit = Some(crate::RecentEdit::Clear);
                ui.close();
            }
        }
    }

    /// Rename the open shot (one undo step, GM21) or scene (scaffolding, not
    /// undoable). Empty-buffer pattern: type a new name and confirm; no
    /// pre-seeding to fight.
    fn shot_names(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(
            egui::RichText::new(format!("Now editing: {}", self.state.cut().name)).color(plate::legend_dim()),
        );
        ui.horizontal(|ui| {
            plate::text_field(
                ui,
                egui::TextEdit::singleline(&mut self.rename_buf).hint_text("New name…").desired_width(110.0),
            );
            let name = self.rename_buf.trim().to_string();
            if ui.add_enabled(!name.is_empty(), egui::Button::new("Rename shot…")).clicked() {
                self.state.rename_current_cut(&name);
                self.rename_buf.clear();
                ui.close();
            }
            if ui.add_enabled(!name.is_empty(), egui::Button::new("Rename scene…")).clicked() {
                self.state.rename_current_scene(&name);
                self.rename_buf.clear();
                ui.close();
            }
        });
    }

    /// Scene and shot navigation (C2). PageUp and PageDown walk the same list.
    fn shot_tree(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        let scenes: crate::SceneCutTree = self
            .state
            .engine
            .project
            .scenes
            .iter()
            .map(|s| (s.id, s.name.clone(), s.cuts.iter().map(|c| (c.id, c.name.clone())).collect()))
            .collect();
        for (scene_id, scene_name, cuts) in &scenes {
            ui.label(egui::RichText::new(scene_name).strong());
            for (cut_id, cut_name) in cuts {
                let current = *scene_id == self.state.view.scene && *cut_id == self.state.view.cut;
                if ui.selectable_label(current, format!("    {cut_name}")).clicked() {
                    ui.close();
                    if !current && crate::guard_gesture(&mut self.canvas, &mut self.state) {
                        self.state.goto_cut(*scene_id, *cut_id);
                    }
                }
            }
            let add = ui.button("Add shot").on_hover_text("Add a shot to this scene.");
            plate::speak(&add, egui::WidgetType::Button, None, &format!("Add shot to {scene_name}"));
            if add.clicked() {
                ui.close();
                if crate::guard_gesture(&mut self.canvas, &mut self.state) {
                    self.state.new_cut(*scene_id);
                }
            }
            ui.add_space(4.0);
        }
        ui.separator();
        let add = ui.button("Add scene").on_hover_text("A new scene with its first shot.");
        if add.clicked() {
            ui.close();
            if crate::guard_gesture(&mut self.canvas, &mut self.state) {
                self.state.new_scene();
            }
        }
    }

    /// XDTS, the industry exposure-sheet exchange format (Toei, OpenToonz,
    /// CSP). Import builds a NEW shot; the Export step's Other formats writes
    /// this one's timing.
    fn xdts_row(&mut self, ui: &mut egui::Ui) {
        let row = ui.button("XDTS…").on_hover_text("Import an exposure sheet (.xdts) as a new shot.");
        if row.clicked() {
            ui.close();
            self.requests.push(crate::AppRequest::ImportXdts);
        }
    }

    /// `Other formats…`: the Export step, with its fold open.
    fn other_formats_row(&mut self, ui: &mut egui::Ui) {
        let row = ui.button("Other formats…").on_hover_text(Action::RoomExport.help());
        plate::mark_row(&row, Target::Action(Action::RoomExport));
        if row.clicked() {
            ui.close();
            self.invoke(Action::RoomExport, ui.ctx());
            self.state.render_queue.panel.unfold();
        }
    }

    /// Frame range (C3): 1-based in the UI (matches the "N / total" frame
    /// readout elsewhere), 0-based internally. None = up to the last drawing,
    /// as the plain rows above it stop (read when the job starts), until a
    /// field is moved, so it can never point past a shot that has since grown
    /// shorter. A field may still reach into the blank tail: that is a choice.
    fn frame_range(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        let n = self.state.frame_count();
        let reach = if self.export_range.is_some() { n } else { self.state.shown_length() };
        let (mut a, mut b) = crate::render_queue::resolve(reach, self.export_range);
        let shown = (a, b);
        ui.horizontal(|ui| {
            ui.label("Frames");
            let mut a1 = a + 1;
            let mut b1 = b + 1;
            if plate::field(ui, egui::DragValue::new(&mut a1).range(1..=b1)).changed() {
                a = a1 - 1;
            }
            ui.label("–");
            if plate::field(ui, egui::DragValue::new(&mut b1).range(a1..=n)).changed() {
                b = b1 - 1;
            }
        });
        if (a, b) != shown {
            self.export_range = Some((a, b));
        }
        let whole = ui.button("Whole shot").on_hover_text("Reset the range to the last drawing.");
        if whole.clicked() {
            self.export_range = None;
            ui.close();
        }
    }

    /// Open or bring forward each pane (a pane lives only while it's in the
    /// dock tree). A row says `<title> panel`.
    fn pane_rows(&mut self, ui: &mut egui::Ui) {
        for pane in window_panes() {
            let present = self.dock.iter_all_tabs().any(|(_, t)| t == pane);
            let hover = if present { "Bring it forward." } else { "Open it." };
            if ui.button(format!("{} panel", pane.title())).on_hover_text(hover).clicked() {
                self.open_pane(*pane);
                ui.close();
            }
        }
    }

    /// Viewers are multi-instance: add up to `MAX_VIEWERS`.
    fn add_result(&mut self, ui: &mut egui::Ui) {
        use crate::Pane;
        let next_id =
            (0..Pane::MAX_VIEWERS).find(|id| !self.dock.iter_all_tabs().any(|(_, t)| *t == Pane::Viewer(*id)));
        let row = ui
            .add_enabled(next_id.is_some(), egui::Button::new("Add Result"))
            .on_hover_text("A read-only view of the finished picture. Dock one beside the canvas to watch it live.");
        if row.clicked()
            && let Some(id) = next_id
        {
            self.add_pane(Pane::Viewer(id));
            ui.close();
        }
    }

    fn os_windows(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        // Phase 5 step 1: the viewer as a REAL OS window — drag it onto the
        // second monitor / pen display.
        let mut floating = self.float_viewer.is_open();
        if ui
            .add(|ui: &mut egui::Ui| plate::latch(ui, &mut floating, "Result in an OS window"))
            .on_hover_text("Open the result as a separate real window. Drag it to another monitor.")
            .changed()
        {
            self.float_viewer.set_open(floating);
            ui.close();
        }
        // Phase 5 step 2: the EDITABLE canvas as a real OS window — draw
        // directly on the pen display. Docking it back goes through the SAME
        // guard as Save/Open/workspace switching: a live pen stroke refuses
        // the transition rather than being silently orphaned (mid-stroke this
        // would otherwise strand touch_active forever — nothing else ever
        // resets it).
        let mut floating = self.float_canvas.open;
        if ui
            .add(|ui: &mut egui::Ui| plate::latch(ui, &mut floating, "Canvas in an OS window"))
            .on_hover_text(
                "Open the drawing canvas as a separate real window. Drag it onto the pen display. \
                 Every tool works there exactly as it does docked.",
            )
            .changed()
        {
            if floating {
                self.float_canvas.open = true;
                ui.close();
            } else if crate::guard_gesture(&mut self.canvas, &mut self.state) {
                self.float_canvas.open = false;
                ui.close();
            }
            // Refused: leave float_canvas.open at its prior value (still
            // true) — the checkbox reflects it again next frame; the status
            // line already explains why.
        }
    }

    /// `Save this arrangement…`: captures the CURRENT tool and view state
    /// along with the layout.
    fn save_arrangement(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Save this arrangement…").color(plate::legend_dim()));
        ui.horizontal(|ui| {
            plate::text_field(ui, egui::TextEdit::singleline(&mut self.ws_name).hint_text("Name"));
            let name = self.ws_name.trim().to_string();
            if ui.add_enabled(!name.is_empty(), egui::Button::new("Save")).clicked() {
                let view = Some(self.canvas.snapshot_view(&self.state));
                if let Some(w) = self.workspaces.list.iter_mut().find(|w| w.name == name) {
                    w.dock = self.dock.clone();
                    w.view = view;
                } else {
                    self.workspaces.list.push(crate::Workspace { name, dock: self.dock.clone(), preset: None, view });
                }
                self.save_workspaces();
                self.ws_name.clear();
                ui.close();
            }
        });
        if !self.workspaces.list.is_empty() {
            ui.separator();
        }
    }

    /// Apply, bind and delete for each saved arrangement.
    fn arrangements(&mut self, ui: &mut egui::Ui, presets: &[BrushPreset]) {
        let mut apply: Option<usize> = None;
        let mut remove: Option<usize> = None;
        let mut assign: Option<(usize, Option<String>)> = None;
        for (i, w) in self.workspaces.list.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.button(&w.name).on_hover_text("Apply this saved arrangement.").clicked() {
                    apply = Some(i);
                    ui.close();
                }
                // Bound brush preset for this arrangement (a step keeps its
                // own brush).
                let bound = w.preset.clone().unwrap_or_else(|| "no brush".into());
                // GM16: a word, not 🖌, which no embedded face draws.
                ui.menu_button(format!("brush: {bound}"), |ui| {
                    if ui.button("no brush").clicked() {
                        assign = Some((i, None));
                        ui.close();
                    }
                    for p in presets {
                        if ui.button(&p.name).clicked() {
                            assign = Some((i, Some(p.name.clone())));
                            ui.close();
                        }
                    }
                });
                // AUDIT [11]: this was the smallest target in the row, one
                // item from "apply". Destruction must be findable by feel,
                // not by precision.
                if plate::danger(ui, "Delete") {
                    remove = Some(i);
                    ui.close();
                }
            });
        }
        if let Some(i) = apply {
            let name = self.workspaces.list[i].name.clone();
            self.dock = self.workspaces.list[i].dock.clone();
            // A custom room is off-spine — no stage highlighted.
            self.go_off_spine(&name);
            // Same all-or-nothing tool/view + brush gate as the stage buttons.
            let view_ok = match self.workspaces.list[i].view {
                Some(v) => self.canvas.apply_view(&v, &mut self.state),
                None => !self.canvas.stroke_active(),
            };
            self.state.status = format!("workspace: {name}");
            if view_ok
                && let Some(pname) = self.workspaces.list[i].preset.clone()
                && let Some(p) = presets.iter().find(|p| p.name == pname)
            {
                self.canvas.apply_preset(p);
                self.state.status = format!("workspace {name} — brush: {pname}");
            }
        }
        if let Some((i, pname)) = assign {
            self.workspaces.list[i].preset = pname;
            self.save_workspaces();
        }
        if let Some(i) = remove {
            self.workspaces.list.remove(i);
            self.save_workspaces();
        }
    }

    /// GM7: a line of the document's own state under an item's tooltip —
    /// the colour law beside the migration that changes it.
    pub(crate) fn menu_note(&self, action: Action) -> Option<String> {
        match action {
            Action::ConvertColorsToSrgb => Some(format!(
                "this project: {}",
                self.state.engine.project.color_law.label()
            )),
            // GM18: what Clip to panels takes from a graph shaped by hand.
            Action::ClipToPanels => self
                .state
                .page_hand_built()
                .map(|n| format!("DANGER: it {} ({n} nodes now)", crate::manga_tools::CLIP_DANGER)),
            // GM22: why a ticked 3D underlay shows nothing.
            Action::Underlay3d => self.canvas.underlay_note(&self.state).map(String::from),
            _ => None,
        }
    }

    /// The help windows, each when open.
    pub(crate) fn help_windows(&mut self, ctx: &egui::Context, binds: &[Binding]) {
        let ticks = self.ticks();
        let rows = self.quick_rows();
        let kind = self.kind();
        let help = &mut self.help;
        egui::Window::new("Keyboard shortcuts")
            .frame(plate::dialog_frame(&ctx.global_style()))
            .open(&mut help.sheet)
            .default_size([460.0, 560.0])
            .show(ctx, |ui| {
                plate::surface(ui);
                ui.horizontal(|ui| {
                    plate::legend(ui, "find");
                    plate::text_field(ui, egui::TextEdit::singleline(&mut help.sheet_query));
                });
                ui.separator();
                let rows = sheet_rows(binds, &help.sheet_query);
                let scroll = egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut group = None;
                    for (a, chord) in rows {
                        if group != Some(a.group()) {
                            group = Some(a.group());
                            ui.add_space(4.0);
                            plate::legend(ui, a.group().name());
                        }
                        ui.horizontal(|ui| {
                            ui.label(a.label()).on_hover_text(a.help());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                plate::value(ui, &chord);
                            });
                        });
                    }
                    // GM7: then the gestures, under the same find.
                    let gestures = gesture_rows(&help.sheet_query);
                    if !gestures.is_empty() {
                        ui.add_space(4.0);
                        plate::legend(ui, crate::vocab::CANVAS_GESTURES);
                    }
                    for (gesture, what) in gestures {
                        ui.horizontal(|ui| {
                            ui.label(what);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                plate::value(ui, gesture);
                            });
                        });
                    }
                });
                plate::note_viewport(ui, scroll.inner_rect, [false, true]);
            });
        let mut show_me = None;
        egui::Window::new("Quick start")
            .frame(plate::dialog_frame(&ctx.global_style()))
            .open(&mut help.quick_start)
            .default_width(420.0)
            .show(ctx, |ui| {
                plate::surface(ui);
                // GM8: the five first steps, each ticked the first time its
                // verb runs, each with its chord and "Show me".
                show_me = crate::welcome::checklist_ui(ui, rows, &ticks, binds);
                ui.separator();
                // Said in THIS keymap's chords, so it never lies.
                let k = |a| crate::cmdpal::chord_of(binds, a).unwrap_or_else(|| "—".into());
                use Action::*;
                let first = format!(
                    "Draw on the canvas with the pen ({} brush, {} eraser).",
                    k(BrushTool),
                    k(ToggleEraser)
                );
                // AU18: the middle lines by kind: frames, the sheet and Play
                // are an animation's.
                let middle = match kind {
                    Some(ProjectKind::Drawing) => vec![
                        "Color fills your drawing; Effects adds a look to it.".to_string(),
                    ],
                    Some(ProjectKind::Comic) => vec![
                        format!("{} cuts the page into panels.", k(PanelTool)),
                        format!("{} adds the next page.", k(NewPage)),
                    ],
                    _ => vec![
                        format!(
                            "{} puts a new drawing on this frame; {} / {} step frames.",
                            k(NewDrawing),
                            k(NextFrame),
                            k(PrevFrame)
                        ),
                        "The x-sheet times the drawings: a key holds until the next one.".to_string(),
                        format!("{} plays the cut; {} latches looping.", k(PlayPause), k(ToggleLoop)),
                    ],
                };
                let saves = format!(
                    "{} saves. {} finds any command; {} lists every key.",
                    k(Save),
                    k(CommandPalette),
                    k(ShortcutSheet)
                );
                // GM4: the canvas gestures no menu shows.
                let view = "Hold Space and drag the canvas to move the view (Shift+drag turns it); \
                            Alt+click picks a colour; Shift+click rules on from the last stroke.";
                // GM7: the refill gestures, on the fill tool.
                let fill = format!(
                    "With fill ({}), Shift+click refills a painted flat in the brush colour; \
                     Ctrl+Shift+click replaces that colour on the whole layer.",
                    k(FillTool)
                );
                let last = [saves, view.to_string(), fill];
                let lines = std::iter::once(first).chain(middle).chain(last);
                for (i, line) in lines.enumerate() {
                    ui.label(format!("{}. {line}", i + 1));
                }
            });
        egui::Window::new("About Gaban Studio")
            .frame(plate::dialog_frame(&ctx.global_style()))
            .open(&mut help.about)
            .default_width(320.0)
            .show(ctx, |ui| {
                plate::surface(ui);
                // Latin, as the legends are (GM16: the embedded JP face is
                // whole now, so 画版 would draw too).
                plate::legend(ui, crate::vocab::APP_NAME);
                plate::value(ui, &format!("version {}", env!("CARGO_PKG_VERSION")));
                ui.label(
                    "A node-graph 2D animation studio, layout to delivery: draw, time, paint, \
                     composite, edit.",
                );
                // GM19: where this release's changes are listed.
                ui.label(
                    egui::RichText::new("this release's changes: Help ▸ What's New")
                        .color(plate::legend_dim()),
                );
            });
        if let Some(step) = show_me {
            self.show_me(step);
        }
        // GM8: the project's own settings window, while open.
        self.project_settings_window(ctx);
        // GM10: the cut's paper window, while open.
        self.paper_size_window(ctx);
        // GM12: the lip-flap window, while open.
        self.lip_flap_window(ctx);
        // GM15: the retake note window, while open.
        crate::board_panel::note_window(ctx, &mut self.state);
    }
}
