//! GM3: THE COMMAND PALETTE (Ctrl+K, F3) — every action by name, alias or
//! help line, recent first. Enter runs the highlighted row through the one
//! dispatch (D6), Esc closes; while it is open the keys go to its field.

use eframe::egui;

use crate::config::{Action, Binding};
use crate::plate;

/// How many recent commands the palette keeps (this session only).
const RECENT_MAX: usize = 8;

#[derive(Default)]
pub struct Palette {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    /// Commands run from the menus or the palette, most recent first.
    pub recent: Vec<Action>,
    /// Just opened: the field takes focus.
    fresh: bool,
}

impl Palette {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.query.clear();
            self.selected = 0;
            self.fresh = true;
        }
    }

    pub fn note_run(&mut self, a: Action) {
        self.recent.retain(|r| *r != a);
        self.recent.insert(0, a);
        self.recent.truncate(RECENT_MAX);
    }
}

/// How well `query` finds `action` (higher is better; None = not at all).
/// Every word of the query must match somewhere. A word in the NAME (or the
/// menu path) outranks one in an ALIAS, which outranks one in the HELP line;
/// within each, a prefix beats a word start beats anywhere, and a name also
/// matches its letters in order ("ndr" finds "New drawing"). Pure.
pub fn score(query: &str, action: Action) -> Option<i32> {
    let name = action.label().to_lowercase();
    let menu = action.menu().unwrap_or("").to_lowercase();
    let help = action.help().to_lowercase();
    let mut total = 0;
    for word in query.to_lowercase().split_whitespace() {
        let best = [
            text_score(&name, word, true).map(|s| s + 2000),
            text_score(&menu, word, false).map(|s| s + 1500),
            action
                .aliases()
                .iter()
                .filter_map(|al| text_score(&al.to_lowercase(), word, false))
                .max()
                .map(|s| s + 1000),
            help.contains(word).then_some(0),
        ]
        .into_iter()
        .flatten()
        .max()?;
        total += best;
    }
    Some(total)
}

fn text_score(text: &str, word: &str, fuzzy: bool) -> Option<i32> {
    if text.starts_with(word) {
        return Some(400);
    }
    if let Some(i) = text.find(word) {
        let at_word = text[..i].ends_with(|c: char| !c.is_alphanumeric());
        return Some(if at_word { 300 } else { 200 });
    }
    let mut letters = text.chars();
    let in_order = word.chars().all(|w| letters.any(|c| c == w));
    (fuzzy && in_order).then_some(50)
}

/// The rows for `query`, best first; recent commands first among equals
/// (and first of all on an empty query). Pure.
pub fn ranked(query: &str, recent: &[Action]) -> Vec<Action> {
    let mut rows: Vec<(i32, usize, usize, Action)> = Action::ALL
        .iter()
        .enumerate()
        .filter_map(|(order, a)| {
            let s = score(query, *a)?;
            let r = recent.iter().position(|x| x == a).unwrap_or(usize::MAX);
            Some((s, r, order, *a))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    rows.into_iter().map(|r| r.3).collect()
}

/// The chord label of an action in these bindings.
pub fn chord_of(binds: &[Binding], a: Action) -> Option<String> {
    binds.iter().find(|b| b.action == a).and_then(|b| b.label())
}

impl crate::Editor {
    /// The palette, when open: a modal near the top, its field focused.
    pub(crate) fn palette_ui(&mut self, ctx: &egui::Context, binds: &[Binding]) {
        if !self.palette.open {
            return;
        }
        // The list's own keys, taken before the field can see them.
        let none = egui::Modifiers::NONE;
        let (up, down, enter, esc, again) = ctx.input_mut(|i| {
            (
                i.consume_key(none, egui::Key::ArrowUp),
                i.consume_key(none, egui::Key::ArrowDown),
                i.consume_key(none, egui::Key::Enter),
                i.consume_key(none, egui::Key::Escape),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::K),
            )
        });
        if esc || again {
            self.palette.open = false;
            return;
        }
        let rows = ranked(&self.palette.query, &self.palette.recent);
        let last = rows.len().saturating_sub(1);
        let mut sel = self.palette.selected.min(last);
        if up {
            sel = sel.saturating_sub(1);
        }
        if down {
            sel = (sel + 1).min(last);
        }
        self.palette.selected = sel;
        let mut run = if enter { rows.get(sel).copied() } else { None };
        let id = egui::Id::new("cmdpal");
        let area = egui::Modal::default_area(id.with("area"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0));
        let resp = egui::Modal::new(id).area(area).show(ctx, |ui| {
            ui.set_width(560.0);
            plate::legend(ui, "command palette");
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.palette.query)
                    .hint_text("type a command — name, alias or what it does")
                    .desired_width(f32::INFINITY),
            );
            if self.palette.fresh || !field.has_focus() {
                field.request_focus();
                self.palette.fresh = false;
            }
            if field.changed() {
                self.palette.selected = 0;
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(egui::RichText::new("no command matches").color(plate::legend_dim()));
                }
                for (i, a) in rows.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let row = plate::detent(ui, i == sel, a.label())
                            .on_hover_text(crate::menus::tip_with(*a, chord_of(binds, *a)));
                        if i == sel && (up || down) {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            run = Some(*a);
                        }
                        if let Some(m) = a.menu() {
                            ui.label(
                                egui::RichText::new(m.replace('/', " ▸ ")).color(plate::legend_dim()),
                            );
                        }
                        if let Some(c) = chord_of(binds, *a) {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                plate::value(ui, &c);
                            });
                        }
                    });
                }
            });
        });
        if resp.should_close() {
            self.palette.open = false;
        }
        if let Some(a) = run {
            self.palette.open = false;
            self.invoke(a, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::CanvasTool;
    use crate::config::{self, Config, Ctx, Group, Keymap};
    use crate::testkit::{CTRL, EditorHarness};
    use crate::{AppRequest, Performed};
    use eframe::egui;

    /// The menus a well-formed path may name.
    const MENUS: [&str; 8] = ["File", "Edit", "View", "Sheet", "Cut", "Room", "Window", "Help"];

    #[test]
    fn cmdui_every_action_has_help_a_group_and_a_place() {
        for &a in Action::ALL {
            let help = a.help();
            assert!((1..=120).contains(&help.chars().count()), "{a:?} help: {help:?}");
            assert!(Group::ALL.contains(&a.group()), "{a:?}");
            match a.menu() {
                Some(path) => {
                    let (menu, item) = path.split_once('/').expect("Menu/Item");
                    assert!(MENUS.contains(&menu) && !item.is_empty(), "{a:?}: {path}");
                }
                // Palette-only is explicit: the deck's tools, nothing else.
                None => assert_eq!(a.group(), Group::Tool, "{a:?} is in no menu"),
            }
        }
    }

    #[test]
    fn cmdui_neither_keymap_has_a_conflict() {
        for &km in Keymap::ALL {
            let binds = config::default_binds(km);
            assert_eq!(binds.len(), Action::ALL.len());
            assert_eq!(config::clashes(&binds), vec![], "{km:?}");
        }
        // Industry shares Delete across panes — allowed, not a clash.
        let ind = config::default_binds(Keymap::Industry);
        let del = |a: Action| ind.iter().find(|b| b.action == a).unwrap().chord.clone();
        assert_eq!(del(Action::ClearCel), del(Action::RemoveColumn));
        assert!(Action::ClearCel.context() == Ctx::Canvas);
        assert!(Action::RemoveColumn.context() == Ctx::Sheet);
        // Space plays in both.
        for &km in Keymap::ALL {
            let b = config::default_binds(km);
            let play = b.iter().find(|b| b.action == Action::PlayPause).unwrap();
            assert_eq!(play.label().as_deref(), Some("Space"), "{km:?}");
        }
    }

    #[test]
    fn cmdui_industry_then_gaban_restores_exactly() {
        let mut cfg = Config::default();
        // A user override sits on top of either preset.
        cfg.binding_mut(Action::ToggleOnion).chord = cfg.binding(Action::MissCheck).unwrap().chord.clone();
        cfg.binding_mut(Action::MissCheck).chord = None;
        let gaban = cfg.keybinds.clone();
        cfg.set_keymap(Keymap::Industry);
        let eraser = cfg.binding(Action::ToggleEraser).unwrap().label();
        assert_eq!(eraser.as_deref(), Some("E"), "Industry: E erases");
        assert_eq!(cfg.binding(Action::BrushTool).unwrap().label().as_deref(), Some("B"));
        assert_eq!(cfg.binding(Action::FillTool).unwrap().label().as_deref(), Some("G"));
        assert!(cfg.binding(Action::MissCheck).unwrap().chord.is_none(), "override kept");
        cfg.set_keymap(Keymap::Gaban);
        assert_eq!(cfg.keybinds, gaban, "Industry then Gaban gives back every binding");
        assert_eq!(cfg.keymap, Keymap::Gaban);
    }

    #[test]
    fn cmdui_score_ranks_names_over_aliases_over_help() {
        assert_eq!(ranked("und", &[])[0], Action::Undo);
        assert_eq!(ranked("onion", &[])[0], Action::ToggleOnion);
        // An alias alone finds it ("ghost" is nowhere in the name).
        assert!(score("ghost", Action::ToggleOnion).is_some());
        assert!(score("light table", Action::ToggleOnion).is_some());
        assert_eq!(ranked("ghost", &[])[0], Action::ToggleOnion);
        // Every word must match; nonsense finds nothing.
        assert!(ranked("zzqx", &[]).is_empty());
        // An empty query lists everything, recent first.
        let rows = ranked("", &[Action::Save, Action::Undo]);
        assert_eq!(rows.len(), Action::ALL.len());
        assert_eq!(&rows[..2], &[Action::Save, Action::Undo]);
        // A recent command never jumps a better match.
        assert_eq!(ranked("und", &[Action::Redo])[0], Action::Undo);
    }

    // ---- the menus ----

    #[test]
    fn cmdui_all_menus_present_and_edit_undo_undoes() {
        let mut h = EditorHarness::new();
        for m in ["file", "edit", "view", "sheet", "cut", "room", "window", "help"] {
            assert!(h.has(m), "no {m} menu\n{}", h.describe());
        }
        let before = h.state().engine.position();
        assert!(before > 0, "the harness drew a drawing first");
        h.click("edit");
        h.click("Undo");
        assert_eq!(h.state().engine.position(), before - 1, "one step back");
    }

    #[test]
    fn cmdui_redo_with_nothing_to_redo_is_greyed_and_says_why() {
        let mut h = EditorHarness::new();
        assert!(!h.state().engine.can_redo());
        h.click("edit");
        let redo = h.node_enabled("Redo");
        assert_eq!(redo, Some(false), "Redo is greyed\n{}", h.describe());
        assert!(!h.has_text_containing("nothing to redo"), "said only on hover");
        h.hover("Redo");
        h.wait(1.0);
        assert!(
            h.has_text_containing("nothing to redo"),
            "the tooltip says why\n{}",
            h.describe()
        );
        // Undo, which can run, is not greyed beside it.
        assert_eq!(h.node_enabled("Undo"), Some(true));
    }

    // ---- the palette ----

    #[test]
    fn cmdui_ctrl_k_opens_ranks_runs_and_esc_closes() {
        let mut h = EditorHarness::new();
        h.key_mods(CTRL, egui::Key::K);
        assert!(h.editor().palette.open, "Ctrl+K opens the palette");
        assert!(h.typing(), "its field has the keys");
        h.type_text("und");
        assert_eq!(h.editor().palette.query, "und");
        assert_eq!(ranked(&h.editor().palette.query, &[])[0], Action::Undo);
        let before = h.state().engine.position();
        h.key(egui::Key::Enter);
        assert!(!h.editor().palette.open, "Enter runs and closes");
        assert_eq!(h.state().engine.position(), before - 1, "Undo ran");
        assert_eq!(h.editor().palette.recent.first(), Some(&Action::Undo));
        // F3 opens it too; Esc closes without running anything.
        h.key(egui::Key::F3);
        assert!(h.editor().palette.open);
        h.type_text("save");
        h.key(egui::Key::Escape);
        assert!(!h.editor().palette.open);
        assert!(h.rig().requests.is_empty(), "Esc ran nothing");
    }

    #[test]
    fn cmdui_palette_field_keeps_the_keys() {
        let mut h = EditorHarness::new();
        let ed = h.editor_mut();
        ed.canvas.set_tool(CanvasTool::Fill, &mut ed.state);
        h.key_mods(CTRL, egui::Key::K);
        h.key(egui::Key::B);
        assert_eq!(h.editor().palette.query, "b", "B typed into the field");
        assert!(h.editor().canvas.tool == CanvasTool::Fill, "B did not switch tool");
        assert!(
            h.performed().iter().all(|(a, _)| *a == Action::CommandPalette),
            "{:?}",
            h.performed()
        );
    }

    #[test]
    fn cmdui_alias_search_finds_the_onion_toggle() {
        let mut h = EditorHarness::new();
        let onion = h.state().view.onion;
        h.perform(Action::CommandPalette);
        h.type_text("ghost");
        h.key(egui::Key::Enter);
        assert_eq!(h.state().view.onion, !onion, "the onion toggle ran");
        h.perform(Action::CommandPalette);
        h.type_text("onion");
        assert!(h.has("Toggle onion skin"), "{}", h.describe());
    }

    // ---- history, help, chords ----

    #[test]
    fn cmdui_history_rows_jump_back_and_forth() {
        let mut h = EditorHarness::new();
        h.state_mut().engine.clear_history();
        let mut hashes = Vec::new();
        for i in 0..5 {
            let s = h.state_mut();
            s.commit_stroke(stroke(i as f32));
            let d = s.current_drawing().expect("a drawing on this frame");
            hashes.push(s.cut().drawing(d).unwrap().content_hash());
        }
        assert_eq!(h.state().engine.position(), 5);
        assert_eq!(h.perform(Action::History), Performed::Done);
        h.run();
        let hash = |h: &EditorHarness| {
            let s = h.state();
            s.cut().drawing(s.current_drawing().unwrap()).unwrap().content_hash()
        };
        h.click("2. draw");
        assert_eq!(h.state().engine.position(), 2, "two steps applied");
        assert_eq!(hash(&h), hashes[1], "exactly the drawing after step 2");
        h.click("5. draw");
        assert_eq!(h.state().engine.position(), 5);
        assert_eq!(hash(&h), hashes[4], "all five restored");
    }

    fn stroke(seed: f32) -> anim_core::model::Stroke {
        let pt = |x: f32, y: f32| anim_core::model::StrokePoint {
            x,
            y,
            pressure: 1.0,
            tilt: [0.0, 0.0],
        };
        anim_core::model::Stroke {
            points: vec![pt(10.0 + seed, 10.0), pt(40.0 + seed, 30.0 + seed)],
            base_width: 3.0,
            color: [0, 0, 0, 255],
        }
    }

    #[test]
    fn cmdui_f1_lists_every_bound_action() {
        let mut h = EditorHarness::new();
        h.key(egui::Key::F1);
        assert!(h.editor().help.sheet, "F1 opens the shortcut sheet");
        let binds = h.rig().config.keybinds.clone();
        let listed = crate::menus::sheet_rows(&binds, "");
        for b in &binds {
            if b.chord.is_some() {
                assert!(listed.iter().any(|(a, _)| *a == b.action), "{:?} missing", b.action);
            }
        }
        // The window shows them, grouped and searchable.
        assert!(h.has("Keyboard shortcuts"), "{}", h.describe());
        assert!(h.has("Undo"), "{}", h.describe());
        let found = crate::menus::sheet_rows(&binds, "onion");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, Action::ToggleOnion);
    }

    #[test]
    fn cmdui_ctrl_shift_z_redoes() {
        let mut h = EditorHarness::new();
        h.perform(Action::Undo);
        let undone = h.state().engine.position();
        let shift_ctrl = egui::Modifiers { shift: true, ..CTRL };
        h.key_mods(shift_ctrl, egui::Key::Z);
        assert_eq!(h.performed().last(), Some(&(Action::Redo, Performed::Done)));
        assert_eq!(h.state().engine.position(), undone + 1);
        // Ctrl+Y still redoes.
        h.perform(Action::Undo);
        h.key_mods(CTRL, egui::Key::Y);
        assert_eq!(h.performed().last(), Some(&(Action::Redo, Performed::Done)));
    }

    #[test]
    fn cmdui_deck_tool_tooltips_name_their_chords() {
        let mut h = EditorHarness::new();
        let binds = h.rig().config.keybinds.clone();
        for (button, action) in [
            ("select", Action::SelectTool),
            ("fill", Action::FillTool),
            ("alpha lock", Action::ToggleAlphaLock),
            ("composite view", Action::ToggleCompositeView),
            ("eraser", Action::ToggleEraser),
        ] {
            let chord = chord_of(&binds, action).expect("bound");
            // "Name (chord) — help", the registry's own line.
            let line = crate::menus::tip_with(action, Some(chord.clone()));
            assert!(line.contains(&format!("({chord})")));
            assert!(!h.has_text_containing(&line), "{button}: shown before the hover");
            h.hover(button);
            h.wait(1.0);
            assert!(
                h.has_text_containing(&line),
                "{button}: no {line:?} in its tooltip\n{}",
                h.describe()
            );
            h.hover("file");
            h.wait(0.2);
        }
    }

    #[test]
    fn cmdui_menu_and_palette_reach_the_shell_like_shortcuts() {
        let mut h = EditorHarness::new();
        h.click("file");
        h.click("Settings…");
        assert_eq!(h.rig().requests, vec![AppRequest::Settings]);
        h.perform(Action::CommandPalette);
        h.type_text("quit");
        h.key(egui::Key::Enter);
        assert_eq!(h.rig().requests, vec![AppRequest::Settings, AppRequest::Quit]);
    }
}
