//! GM3: THE MENU BAR, built from the one verb registry (D6): every action
//! with a menu path ("Edit/Undo") is an item there, showing its chord, a
//! one-line tooltip, and — when it cannot run now — greyed with the reason
//! (`Editor::can_perform`). A click goes through `Editor::invoke`, the same
//! dispatch a shortcut takes. Also the help windows: F1's shortcut sheet,
//! quick start, about.

use std::sync::Arc;

use eframe::egui;

use crate::config::{Action, Binding, Group};
use crate::plate;

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
    resp.on_hover_ui(|ui| {
        // As on_hover_text does: a tooltip keeps its width.
        ui.set_max_width(ui.spacing().tooltip_width);
        let mut text = tip(ui.ctx(), action);
        let more = more();
        if !more.is_empty() {
            text.push('\n');
            text.push_str(&more);
        }
        ui.label(text);
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

impl crate::Editor {
    /// One action as a menu item: the last segment of its menu path, its
    /// chord at the right, a one-line tooltip; greyed with the reason when
    /// it cannot run now.
    pub(crate) fn menu_item(&mut self, ui: &mut egui::Ui, action: Action) {
        let Some(path) = action.menu() else {
            return;
        };
        let name = path.rsplit('/').next().unwrap_or(path);
        let chord = chord_label(ui.ctx(), action);
        let can = self.can_perform(action);
        // A toggle shows whether it is on.
        let on = self.is_on(action);
        let mut button = egui::Button::new(name).selected(on.unwrap_or(false));
        if let Some(c) = &chord {
            button = button.shortcut_text(c.as_str());
        }
        let resp = ui.add_enabled(can.is_ok(), button);
        // The chord is drawn, not spoken: the item answers to its name.
        plate::speak(&resp, egui::WidgetType::Button, on, name);
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

    /// Every action filed under `menu`, in registry order. GM14: a
    /// three-part path ("Sheet/Key handles/flat") files the item in a
    /// submenu, opened where its first item sits.
    pub(crate) fn menu_items(&mut self, ui: &mut egui::Ui, menu: &str) {
        let mut subs: Vec<&str> = Vec::new();
        for a in Action::ALL {
            let Some(path) = a.menu() else { continue };
            let mut parts = path.split('/');
            if parts.next() != Some(menu) {
                continue;
            }
            match (parts.next(), parts.next()) {
                (Some(sub), Some(_)) if !subs.contains(&sub) => {
                    subs.push(sub);
                    ui.menu_button(sub, |ui| {
                        for b in Action::ALL {
                            let mut p = b.menu().unwrap_or("").split('/');
                            if p.next() == Some(menu) && p.next() == Some(sub) && p.next().is_some() {
                                self.menu_item(ui, *b);
                            }
                        }
                    });
                }
                (Some(_), Some(_)) => {}
                _ => self.menu_item(ui, *a),
            }
        }
    }

    /// GM7: a line of the document's own state under an item's tooltip —
    /// the colour law beside the migration that changes it.
    fn menu_note(&self, action: Action) -> Option<String> {
        match action {
            Action::ConvertColorsToSrgb => Some(format!(
                "this project: {}",
                self.state.engine.project.color_law.label()
            )),
            _ => None,
        }
    }

    /// The registry-only menus: edit, view, sheet.
    pub(crate) fn plain_menus(&mut self, ui: &mut egui::Ui) {
        for (label, menu) in [("edit", "Edit"), ("view", "View"), ("sheet", "Sheet")] {
            ui.menu_button(label, |ui| self.menu_items(ui, menu));
        }
    }

    /// GM7: the project's own menu — its colour law (D8), said where the
    /// migration that changes it sits.
    pub(crate) fn project_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("project", |ui| {
            let law = self.state.engine.project.color_law.label();
            ui.label(egui::RichText::new(format!("colour law: {law}")).color(plate::legend_dim()));
            ui.separator();
            self.menu_items(ui, "Project");
        });
    }

    pub(crate) fn help_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("help", |ui| self.menu_items(ui, "Help"));
    }

    /// The help windows, each when open.
    pub(crate) fn help_windows(&mut self, ctx: &egui::Context, binds: &[Binding]) {
        let help = &mut self.help;
        egui::Window::new("Keyboard shortcuts")
            .open(&mut help.sheet)
            .default_size([460.0, 560.0])
            .show(ctx, |ui| {
                plate::surface(ui);
                ui.horizontal(|ui| {
                    plate::legend(ui, "find");
                    ui.text_edit_singleline(&mut help.sheet_query);
                });
                ui.separator();
                let rows = sheet_rows(binds, &help.sheet_query);
                egui::ScrollArea::vertical().show(ui, |ui| {
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
                        plate::legend(ui, "canvas gestures");
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
            });
        let ticks = self.quick_start;
        let mut show_me = None;
        egui::Window::new("Quick start")
            .open(&mut help.quick_start)
            .default_width(420.0)
            .show(ctx, |ui| {
                plate::surface(ui);
                // GM8: the five first steps, each ticked the first time its
                // verb runs, each with its chord and "Show me".
                show_me = crate::welcome::checklist_ui(ui, &ticks, binds);
                ui.separator();
                // Said in THIS keymap's chords, so it never lies.
                let k = |a| crate::cmdpal::chord_of(binds, a).unwrap_or_else(|| "—".into());
                use Action::*;
                for line in [
                    format!(
                        "1. Draw on the canvas with the pen ({} brush, {} eraser).",
                        k(BrushTool),
                        k(ToggleEraser)
                    ),
                    format!(
                        "2. {} puts a new drawing on this frame; {} / {} step frames.",
                        k(NewDrawing),
                        k(NextFrame),
                        k(PrevFrame)
                    ),
                    "3. The x-sheet times the drawings: a key holds until the next one.".into(),
                    format!("4. {} plays the cut; {} latches looping.", k(PlayPause), k(ToggleLoop)),
                    format!(
                        "5. {} saves. {} finds any command; {} lists every key.",
                        k(Save),
                        k(CommandPalette),
                        k(ShortcutSheet)
                    ),
                    // GM4: the canvas gestures no menu shows.
                    "6. Hold Space and drag the canvas to move the view (Shift+drag turns it); \
                     Alt+click picks a colour; Shift+click rules on from the last stroke."
                        .into(),
                    // GM7: the refill gestures, on the fill tool.
                    format!(
                        "7. With fill ({}), Shift+click refills a painted flat in the brush colour; \
                         Ctrl+Shift+click replaces that colour on the whole layer.",
                        k(FillTool)
                    ),
                ] {
                    ui.label(line);
                }
            });
        egui::Window::new("About Gaban Studio")
            .open(&mut help.about)
            .default_width(320.0)
            .show(ctx, |ui| {
                plate::surface(ui);
                // Latin only: the embedded JP subset has 画 but not 版.
                plate::legend(ui, "gaban studio");
                plate::value(ui, &format!("version {}", env!("CARGO_PKG_VERSION")));
                ui.label("A node-graph 2D animation studio: draw, time, paint, edit.");
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
    }
}
