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
    /// Commands run (by shortcut, menu or palette), most recent first.
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
/// matches its letters in order ("ndr" finds "New drawing"), though under any
/// alias: an old word typed whole finds its command before a stray spelling
/// does. The first alias is the command's old name and edges out the rest.
/// Case and the romaji macrons fold away ("shinko" finds Shots). Pure.
pub fn score(query: &str, action: Action) -> Option<i32> {
    const LETTERS: i32 = 50;
    let name = fold(action.label());
    let menu = fold(action.menu().unwrap_or(""));
    let help = fold(action.help());
    let query = fold(query);
    let mut total = 0;
    for word in query.split_whitespace() {
        let best = [
            text_score(&name, word, true).map(|s| if s == LETTERS { s + 900 } else { s + 2000 }),
            text_score(&menu, word, false).map(|s| s + 1500),
            action
                .aliases()
                .iter()
                .enumerate()
                .filter_map(|(at, al)| text_score(&fold(al), word, false).map(|s| s + LETTERS * i32::from(at == 0)))
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

/// Lower case, with ā ī ū ē ō read as the plain vowel (typed romaji).
fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'ā' => 'a',
            'ī' => 'i',
            'ū' => 'u',
            'ē' => 'e',
            'ō' => 'o',
            c => c,
        })
        .collect()
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
        // A command that cannot run now is dim and says why on hover.
        let blocked: Vec<Option<&'static str>> =
            rows.iter().map(|a| self.can_perform(*a).err()).collect();
        let mut run = if enter { rows.get(sel).copied() } else { None };
        let id = egui::Id::new("cmdpal");
        let area = egui::Modal::default_area(id.with("area"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0));
        let frame = plate::dialog_frame(&ctx.global_style());
        let resp = egui::Modal::new(id).area(area).frame(frame).show(ctx, |ui| {
            ui.set_width(560.0);
            plate::legend(ui, "command palette");
            let field = plate::text_field(
                ui,
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
            let scroll = egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                if rows.is_empty() {
                    ui.label(egui::RichText::new("no command matches").color(plate::legend_dim()));
                }
                for (i, a) in rows.iter().enumerate() {
                    let why = blocked[i];
                    ui.horizontal(|ui| {
                        let row = match why {
                            Some(_) => plate::detent_dim(ui, i == sel, a.label()),
                            None => plate::detent(ui, i == sel, a.label()),
                        };
                        // Built only while hovered.
                        let row = row.on_hover_ui(|ui| {
                            ui.set_max_width(ui.spacing().tooltip_width);
                            let tip = crate::menus::tip_with(*a, chord_of(binds, *a));
                            ui.label(match why {
                                Some(w) => format!("{tip}\nnot now: {w}"),
                                None => tip,
                            });
                        });
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
            plate::note_viewport(ui, scroll.inner_rect, [false, true]);
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
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use crate::canvas::CanvasTool;
    use crate::config::{self, Config, Ctx, Group, Keymap};
    use crate::doc::AppState;
    use crate::testkit::{CTRL, EditorHarness};
    use crate::{AppRequest, Performed};
    use anim_core::raster::{TILE, TILE_LEN, TileCoord, TileData};
    use eframe::egui;

    /// The menus a well-formed path may name.
    const MENUS: [&str; 6] = ["File", "Edit", "View", "Animation", "Help", "More"];

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
        assert!(score("sukashi", Action::ToggleOnion).is_some());
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
        for (m, _) in crate::menus::MENUS {
            assert!(h.has(m), "no {m} menu\n{}", h.describe());
        }
        let before = h.state().engine.position();
        assert!(before > 0, "the harness drew a drawing first");
        h.click(crate::vocab::MENU_EDIT);
        h.click("Undo");
        assert_eq!(h.state().engine.position(), before - 1, "one step back");
    }

    #[test]
    fn cmdui_redo_with_nothing_to_redo_is_greyed_and_says_why() {
        let mut h = EditorHarness::new();
        assert!(!h.state().engine.can_redo());
        h.click(crate::vocab::MENU_EDIT);
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
        assert!(h.has(Action::ToggleOnion.label()), "{}", h.describe());
    }

    #[test]
    fn genui_the_palette_finds_add_node_by_the_generators_words() {
        // GM16: a manga artist types the trade word, not "add node".
        for q in ["fukidashi", "speed lines", "ryūsen", "focus lines", "shuchusen", "tategaki", "serifu", "balloon"] {
            assert_eq!(ranked(q, &[]).first(), Some(&Action::AddNode), "{q:?} finds Add node first");
        }
        for q in ["particles", "snow", "sakura", "text", "lettering"] {
            assert!(ranked(q, &[]).contains(&Action::AddNode), "{q:?} finds Add node");
        }
        assert!(Action::AddNode.help().contains("generators"), "{}", Action::AddNode.help());
        // In the palette itself: the word lists Add node…, and Enter opens
        // the node graph's add menu.
        let mut h = EditorHarness::new();
        h.perform(Action::CommandPalette);
        h.type_text("fukidashi");
        assert_eq!(ranked(&h.editor().palette.query, &[]).first(), Some(&Action::AddNode));
        assert!(h.has("Add node…"), "{}", h.describe());
        h.key(egui::Key::Enter);
        h.run();
        assert!(!h.editor().palette.open, "Enter runs and closes");
        assert!(h.state().graph_ui.add_menu.is_some(), "the add menu is open\n{}", h.describe());
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
        let digit = 1 + h.rig().config.presets.iter().position(|p| p.name == "genga").unwrap();
        let genga = format!("{} ({digit})", crate::vocab::CLEAN_PENCIL);
        let genga_preset = Action::preset_slot(digit).expect("a pencil-box slot");
        use crate::floats::Float;
        for (button, action, float) in [
            ("Select", Action::SelectTool, None),
            ("Eraser", Action::ToggleEraser, None),
            ("Fill", Action::FillTool, None),
            // AU4: the deck's keys stand on the rail and in the floats.
            (crate::vocab::DRAW_TO_FILL, Action::LassoFillTool, Some(Float::ToolOptions)),
            (crate::vocab::KEEP_INSIDE_STROKES, Action::ToggleAlphaLock, Some(Float::Brushes)),
            ("Miss check", Action::MissCheck, Some(Float::CanvasOptions)),
            (crate::vocab::GHOST_FRAMES, Action::ToggleOnion, Some(Float::CanvasOptions)),
            ("Fit", Action::ZoomFit, Some(Float::CanvasOptions)),
            (genga.as_str(), genga_preset, Some(Float::Brushes)),
        ] {
            if let Some(float) = float {
                // Tool options answer the armed tool: Fill's hold the chooser.
                if float == Float::ToolOptions {
                    h.click("Fill");
                }
                h.open_float(float);
                h.scroll_to(button);
            }
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
            h.hover("File");
            h.wait(0.2);
        }
        // AU4: the composite latch left the deck, and its chord went with it
        // to the View row that took its place.
        let action = Action::ToggleCompositeView;
        let chord = chord_of(&binds, action).expect("bound");
        let line = crate::menus::tip_with(action, Some(chord.clone()));
        assert!(line.contains(&format!("({chord})")));
        h.click(crate::vocab::MENU_VIEW);
        assert!(!h.has_text_containing(&line), "shown before the hover");
        h.hover("Preview final");
        h.wait(1.0);
        assert!(h.has_text_containing(&line), "no {line:?} on its row\n{}", h.describe());
    }

    #[test]
    fn cmdui_menu_and_palette_reach_the_shell_like_shortcuts() {
        let mut h = EditorHarness::new();
        h.click(crate::vocab::MENU_FILE);
        h.click("Settings…");
        assert_eq!(h.rig().requests, vec![AppRequest::Settings]);
        h.perform(Action::CommandPalette);
        h.type_text("quit");
        h.key(egui::Key::Enter);
        assert_eq!(h.rig().requests, vec![AppRequest::Settings, AppRequest::Quit]);
    }

    // ---- review fixes ----

    /// Two inked tiles side by side on this frame's own cel, active layer.
    fn ink_two_tiles(h: &mut EditorHarness) {
        let ink = Arc::new(TileData::from_vec(vec![0x3C00; TILE_LEN]));
        let s = h.state_mut();
        let slot = s.active_slot_resolved().expect("an armed layer");
        s.commit_raster(vec![((0, 0), ink.clone()), ((1, 0), ink)], slot);
    }

    fn layer_tiles(h: &EditorHarness) -> BTreeMap<TileCoord, Arc<TileData>> {
        h.state().active_layer_tiles().cloned().unwrap_or_default()
    }

    #[test]
    fn cmdui_industry_delete_clears_only_the_selection() {
        let mut h = EditorHarness::new();
        h.rig_mut().config.set_keymap(Keymap::Industry);
        ink_two_tiles(&mut h);
        let before = layer_tiles(&h);
        assert!(before.contains_key(&(0, 0)) && before.contains_key(&(1, 0)));
        // A selection over the first tile only; Delete over the canvas.
        let t = TILE as f32;
        let rect = vec![
            egui::pos2(0.0, 0.0),
            egui::pos2(t, 0.0),
            egui::pos2(t, t),
            egui::pos2(0.0, t),
        ];
        h.editor_mut().canvas.set_selection(rect);
        let over_canvas = h.editor().canvas.dbg_rect.center();
        h.point_at(over_canvas);
        h.key(egui::Key::Delete);
        assert_eq!(h.performed().last(), Some(&(Action::ClearCel, Performed::Done)));
        let after = layer_tiles(&h);
        assert!(!after.contains_key(&(0, 0)), "inside the selection: cleared");
        assert_eq!(after.get(&(1, 0)), before.get(&(1, 0)), "outside it: untouched");
        // One step, undone as one.
        h.perform(Action::Undo);
        assert_eq!(layer_tiles(&h), before);
    }

    #[test]
    fn cmdui_lasso_fill_is_a_verb_with_a_chord() {
        for &km in Keymap::ALL {
            let binds = config::default_binds(km);
            let lf = binds.iter().find(|b| b.action == Action::LassoFillTool).unwrap();
            assert_eq!(lf.label().as_deref(), Some("Shift+G"), "{km:?}");
        }
        let mut h = EditorHarness::new();
        h.key_mods(egui::Modifiers::SHIFT, egui::Key::G);
        assert!(h.editor().canvas.tool == CanvasTool::LassoFill, "Shift+G arms the lasso fill");
        h.key_mods(egui::Modifiers::SHIFT, egui::Key::G);
        assert!(h.editor().canvas.tool == CanvasTool::Paint, "and goes back to the brush");
        // "lasso" finds the lasso fill first, not the select tool.
        assert_eq!(ranked("lasso", &[])[0], Action::LassoFillTool);
        assert_eq!(ranked("fill loop", &[])[0], Action::LassoFillTool);
        assert_eq!(ranked("lasso select", &[])[0], Action::SelectTool);
    }

    #[test]
    fn cmdui_room_items_carry_the_room_names() {
        use crate::workspace::Stage;
        // GM8 (D12): five spine rooms — Finishing split into ink & paint
        // and composite.
        let rooms = [
            Action::RoomLayout,
            Action::RoomDrawing,
            Action::RoomFinishing,
            Action::RoomComposite,
            Action::RoomEdit,
        ];
        assert_eq!(rooms.len(), Stage::ALL.len());
        for (a, st) in rooms.iter().zip(Stage::ALL) {
            let item = a.menu().expect("in More ▸ Steps ▸");
            assert!(item.starts_with("More/Steps/") && item.contains(st.name()), "{a:?}: {item}");
            assert!(a.label().contains(st.name()), "{a:?}: {}", a.label());
        }
        // The English words still find them.
        for (word, a) in [
            ("drawing", Action::RoomDrawing),
            ("paint", Action::RoomFinishing),
            ("ink", Action::RoomFinishing),
            ("composite", Action::RoomComposite),
            ("edit", Action::RoomEdit),
        ] {
            assert!(score(word, a).is_some(), "{word:?} finds {a:?}");
        }
        let mut h = EditorHarness::new();
        h.open_path("More/Steps");
        h.click("Color step");
        assert_eq!(h.editor().stage, Some(Stage::InkPaint));
    }

    #[test]
    fn cmdui_old_configs_and_workspaces_still_load() {
        use crate::workspace::Pane;
        // A config from before GM3: no keymap, no extra chords, none of
        // GM3's verbs, and F3 already the artist's own key.
        let dir = tempfile::tempdir().unwrap();
        let mut v = serde_json::to_value(Config::default()).unwrap();
        v.as_object_mut().unwrap().remove("keymap");
        let binds = v["keybinds"].as_array_mut().unwrap();
        binds.retain(|b| {
            !matches!(b["action"].as_str(), Some("CommandPalette" | "History" | "LassoFillTool"))
        });
        for b in binds.iter_mut() {
            b.as_object_mut().unwrap().remove("also");
            if b["action"] == "ToggleLoop" {
                b["chord"] = serde_json::json!({
                    "keys": ["F3"], "ctrl": false, "shift": false, "alt": false
                });
            }
        }
        let path = config::config_path_in(dir.path());
        std::fs::write(&path, serde_json::to_vec_pretty(&v).unwrap()).unwrap();
        let (cfg, notes) = Config::load_from(dir.path());
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(cfg.keymap, Keymap::Gaban, "no keymap reads as gaban");
        assert_eq!(cfg.keybinds.len(), Action::ALL.len(), "the new verbs are filled in");
        let label = |c: &Config, a| c.binding(a).and_then(|b| b.label());
        assert_eq!(label(&cfg, Action::Redo).as_deref(), Some("Ctrl+Y / Ctrl+Shift+Z"));
        assert_eq!(label(&cfg, Action::CommandPalette).as_deref(), Some("Ctrl+K"), "F3 was taken");
        assert_eq!(label(&cfg, Action::ToggleLoop).as_deref(), Some("F3"), "the artist's key stays");
        assert_eq!(config::clashes(&cfg.keybinds), vec![], "no key fires two verbs");
        // A later config where the artist dropped Redo's extra chord: it
        // stays dropped, load after load.
        let mut cfg = cfg;
        cfg.binding_mut(Action::Redo).also.clear();
        cfg.save_to(dir.path()).unwrap();
        let (back, _) = Config::load_from(dir.path());
        assert_eq!(label(&back, Action::Redo).as_deref(), Some("Ctrl+Y"));
        // The History pane is stored by name, beside the old panes.
        assert_eq!(serde_json::to_string(&Pane::History).unwrap(), "\"History\"");
        assert_eq!(serde_json::from_str::<Pane>("\"History\"").unwrap(), Pane::History);
        assert_eq!(serde_json::from_str::<Pane>("\"XSheet\"").unwrap(), Pane::XSheet);
    }

    #[test]
    fn cmdui_history_is_not_written_into_the_project() {
        // No project-file schema change: the history lives in memory only.
        let mut h = EditorHarness::new();
        for i in 0..3 {
            h.state_mut().commit_stroke(stroke(i as f32));
        }
        h.state_mut().undo_to(2);
        let file = h.data_dir().join("cut.gaban");
        let s = h.state_mut();
        s.begin_save(file.clone());
        assert!(s.wait_save(), "the save landed");
        let hash = |s: &AppState| s.cut().drawing(s.current_drawing().unwrap()).unwrap().content_hash();
        let saved = hash(h.state());
        let back = AppState::load_from(file).expect("the file opens");
        assert_eq!(hash(&back), saved, "the drawing as it was at step 2");
        assert_eq!(back.engine.history_len(), 0, "no history came back from the file");
        assert_eq!(back.engine.position(), 0);
    }

    #[test]
    fn cmdui_in_a_session_undo_goes_to_the_host_and_jumps_refuse() {
        for (guest, hosting) in [(true, false), (false, true)] {
            let mut h = EditorHarness::new();
            h.editor_mut().canvas.is_guest = guest;
            h.editor_mut().canvas.session_live = hosting;
            let pos = h.state().engine.position();
            // The menu's Undo is live in a session (the host decides) and
            // asks the host.
            h.click(crate::vocab::MENU_EDIT);
            assert_eq!(h.node_enabled("Undo"), Some(true));
            h.click("Undo");
            assert_eq!(h.editor().canvas.history_request, Some(false), "{guest} {hosting}");
            assert_eq!(h.state().engine.position(), pos, "nothing undone here");
            // The palette's Redo goes the same way.
            h.editor_mut().canvas.history_request = None;
            h.perform(Action::CommandPalette);
            h.type_text("redo");
            h.key(egui::Key::Enter);
            assert_eq!(h.editor().canvas.history_request, Some(true));
            // The History pane refuses a jump and says why.
            h.perform(Action::History);
            h.run();
            h.click("0. start");
            assert_eq!(h.state().engine.position(), pos, "no jump in a session");
            assert!(h.state().refusal.contains("one step at a time"), "{}", h.state().refusal);
        }
    }

    #[test]
    fn cmdui_palette_never_opens_mid_stroke() {
        let mut h = EditorHarness::new();
        let c = h.editor().canvas.dbg_rect.center();
        h.press_and_move(c, c + egui::vec2(40.0, 30.0), 4);
        assert!(h.editor().canvas.stroke_active(), "the pen is down");
        h.key_mods(CTRL, egui::Key::K);
        assert!(!h.editor().palette.open, "nothing modal opens over a live stroke");
        assert_eq!(h.performed().last(), Some(&(Action::CommandPalette, Performed::Ignored)));
        assert!(h.state().refusal.contains("finish the stroke first"));
        assert_eq!(h.editor().can_perform(Action::CommandPalette), Err("finish the stroke first"));
        h.release(c + egui::vec2(40.0, 30.0));
        assert!(!h.editor().canvas.stroke_active());
        h.key_mods(CTRL, egui::Key::K);
        assert!(h.editor().palette.open, "pen up: Ctrl+K opens it");
    }

    #[test]
    fn cmdui_history_marks_the_saved_step_and_dims_undone_ones() {
        let mut h = EditorHarness::new();
        h.state_mut().engine.clear_history();
        for i in 0..5 {
            h.state_mut().commit_stroke(stroke(i as f32));
        }
        // Saved at step 3 (as a landed save leaves it), then back to step 1.
        h.state_mut().undo_to(3);
        let file = h.data_dir().join("cut.gaban");
        let s = h.state_mut();
        s.file_path = Some(file);
        s.saved_rev = s.engine.revision();
        h.state_mut().undo_to(1);
        h.perform(Action::History);
        h.run();
        let (mark, row3) = (h.rect(crate::vocab::SAVED_STATE), h.rect("3. draw"));
        assert!(
            (mark.center().y - row3.center().y).abs() < 2.0,
            "the saved mark sits on row 3: {mark:?} vs {row3:?}"
        );
        // Rows 2 to 5 wait to be redone; row 1 is the current step.
        let e = &h.state().engine;
        assert!(e.history_entry(0).is_some_and(|r| r.applied));
        assert!((1..5).all(|i| e.history_entry(i).is_some_and(|r| !r.applied)));
        // Clicking the saved row brings the saved state back.
        h.click("3. draw");
        assert_eq!(h.state().engine.revision(), h.state().saved_rev, "back at the saved step");
    }

    #[test]
    fn cmdui_delete_removes_a_column_only_over_the_sheet() {
        use crate::workspace::Pane;
        let mut h = EditorHarness::new();
        h.state_mut().add_column();
        let cols = |h: &EditorHarness| h.state().cut().xsheet.columns.len();
        assert_eq!(cols(&h), 2);
        // A painter's Delete over the canvas leaves the sheet alone.
        let over_canvas = h.editor().canvas.dbg_rect.center();
        h.point_at(over_canvas);
        h.key(egui::Key::Delete);
        assert_eq!(cols(&h), 2, "Delete over the canvas kept the column");
        // Over the sheet it removes the active column.
        let sheet = h.editor().pane_rects.iter().find(|(p, ..)| *p == Pane::XSheet).map(|(_, r, _)| *r);
        let sheet = sheet.expect("the drawing room docks the sheet");
        h.point_at(sheet.center());
        h.key(egui::Key::Delete);
        assert_eq!(cols(&h), 1, "Delete over the sheet removed it");
    }

    #[test]
    fn cmdui_items_that_cannot_run_say_why() {
        let mut h = EditorHarness::new();
        let ed = h.editor();
        assert_eq!(ed.can_perform(Action::NextCut), Err("this is the only shot"));
        assert_eq!(ed.can_perform(Action::RemoveColumn), Err("a shot keeps at least one track"));
        assert_eq!(ed.can_perform(Action::ClearFrameKey), Ok(()), "frame 0 holds the drawing");
        h.perform(Action::NextFrame);
        let ed = h.editor();
        assert_eq!(ed.can_perform(Action::ClearFrameKey), Err("no drawing starts on this frame"));
        assert!(ed.can_perform(Action::ClearCel).is_err(), "a held frame has no cel of its own");
        // A second column makes Remove column runnable.
        h.state_mut().add_column();
        assert_eq!(h.editor().can_perform(Action::RemoveColumn), Ok(()));
        // The palette dims what cannot run and says why on hover...
        h.perform(Action::CommandPalette);
        h.type_text("next cut");
        h.hover(Action::NextCut.label());
        h.wait(1.0);
        assert!(h.has_text_containing("not now: this is the only shot"), "{}", h.describe());
        h.key(egui::Key::Escape);
        assert!(!h.editor().palette.open);
        // ...and the menu greys it.
        h.open_path("File/Shots");
        assert_eq!(h.node_enabled("Next shot"), Some(false));
    }

    #[test]
    fn cmdui_shortcuts_count_as_recent_and_toggles_show_their_state() {
        let mut h = EditorHarness::new();
        h.key(egui::Key::O);
        assert_eq!(h.editor().palette.recent.first(), Some(&Action::ToggleOnion));
        assert_eq!(h.editor().is_on(Action::ToggleOnion), Some(h.state().view.onion));
        assert_eq!(h.editor().is_on(Action::Undo), None, "not a toggle");
        // The view menu's toggles say whether they are on.
        let onion = h.state().view.onion;
        h.click(crate::vocab::MENU_VIEW);
        assert_eq!(h.node_toggled("Ghost frames"), Some(onion), "{}", h.describe());
        h.click("Ghost frames");
        assert_eq!(h.state().view.onion, !onion);
        h.click(crate::vocab::MENU_VIEW);
        assert_eq!(h.node_toggled("Ghost frames"), Some(!onion));
        h.key(egui::Key::Escape);
        // The About window says its name in glyphs the embedded font has.
        h.perform(Action::About);
        h.run();
        assert!(h.has(crate::vocab::APP_NAME), "{}", h.describe());
    }
}
