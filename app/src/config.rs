//! User configuration: rebindable keyboard shortcuts (and room for more).
//!
//! Krita-style: every command is a named [`Action`] the user can bind to any
//! key + modifiers, edited in the Settings window and persisted to
//! `%APPDATA%/AnimStudio/config.json`. Defaults avoid keys a compact keyboard
//! may lack (frame stepping is `,` / `.`, not the arrow keys).

use std::path::PathBuf;

use eframe::egui;

/// Every rebindable command in the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Action {
    PlayPause,
    NextFrame,
    PrevFrame,
    FirstFrame,
    LastFrame,
    NewDrawing,
    ClearCel,
    ToggleOnion,
    Undo,
    Redo,
    Save,
    SaveAs,
    Open,
    NewProject,
}

impl Action {
    /// All actions, in the order shown in the Settings window.
    pub const ALL: &'static [Action] = &[
        Action::PlayPause,
        Action::NextFrame,
        Action::PrevFrame,
        Action::FirstFrame,
        Action::LastFrame,
        Action::NewDrawing,
        Action::ClearCel,
        Action::ToggleOnion,
        Action::Undo,
        Action::Redo,
        Action::Save,
        Action::SaveAs,
        Action::Open,
        Action::NewProject,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::PlayPause => "Play / Pause",
            Action::NextFrame => "Next frame",
            Action::PrevFrame => "Previous frame",
            Action::FirstFrame => "First frame",
            Action::LastFrame => "Last frame",
            Action::NewDrawing => "New drawing (blank cel)",
            Action::ClearCel => "Clear cel",
            Action::ToggleOnion => "Toggle onion skin",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Save => "Save",
            Action::SaveAs => "Save As…",
            Action::Open => "Open…",
            Action::NewProject => "New project…",
        }
    }

    fn default_chord(self) -> Option<Chord> {
        use Action::*;
        let k = |name: &str| Chord::plain(name);
        let ctrl = |name: &str| Chord {
            key: name.to_string(),
            ctrl: true,
            shift: false,
            alt: false,
        };
        match self {
            PlayPause => Some(k("Space")),
            NextFrame => Some(k("Period")),
            PrevFrame => Some(k("Comma")),
            FirstFrame => Some(k("Home")),
            LastFrame => Some(k("End")),
            NewDrawing => Some(k("N")),
            ClearCel => None,
            ToggleOnion => Some(k("O")),
            Undo => Some(ctrl("Z")),
            Redo => Some(ctrl("Y")),
            Save => Some(ctrl("S")),
            SaveAs => Some(Chord {
                key: "S".to_string(),
                ctrl: true,
                shift: true,
                alt: false,
            }),
            Open => Some(ctrl("O")),
            NewProject => Some(ctrl("N")),
        }
    }
}

/// A key combination. `key` is an egui [`egui::Key`] name (see `Key::name`);
/// `ctrl` means the platform command modifier (Ctrl on Windows/Linux).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Chord {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    fn plain(key: &str) -> Self {
        Self {
            key: key.to_string(),
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    pub fn from_event(key: egui::Key, m: egui::Modifiers) -> Self {
        Self {
            key: key.name().to_string(),
            ctrl: m.command,
            shift: m.shift,
            alt: m.alt,
        }
    }

    fn egui_key(&self) -> Option<egui::Key> {
        egui::Key::from_name(&self.key)
    }

    /// Human-readable label, e.g. "Ctrl+Shift+S" or ",".
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s.push_str(pretty_key(&self.key));
        s
    }

    fn matches(&self, i: &egui::InputState) -> bool {
        let Some(key) = self.egui_key() else {
            return false;
        };
        let m = i.modifiers;
        i.key_pressed(key) && m.command == self.ctrl && m.shift == self.shift && m.alt == self.alt
    }
}

fn pretty_key(name: &str) -> &str {
    match name {
        "Comma" => ",",
        "Period" => ".",
        "Space" => "Space",
        other => other,
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Binding {
    pub action: Action,
    pub chord: Option<Chord>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub keybinds: Vec<Binding>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keybinds: Action::ALL
                .iter()
                .map(|a| Binding {
                    action: *a,
                    chord: a.default_chord(),
                })
                .collect(),
        }
    }
}

impl Config {
    pub fn config_path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(base).join("AnimStudio").join("config.json"))
    }

    /// Load config, falling back to defaults on any error. Missing actions
    /// (e.g. added in a newer build) are filled in with their defaults.
    pub fn load() -> Self {
        let mut cfg = Self::config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Config>(&s).ok())
            .unwrap_or_default();
        for a in Action::ALL {
            if !cfg.keybinds.iter().any(|b| b.action == *a) {
                cfg.keybinds.push(Binding {
                    action: *a,
                    chord: a.default_chord(),
                });
            }
        }
        cfg
    }

    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn reset_to_defaults(&mut self) {
        *self = Self::default();
    }

    pub fn binding_mut(&mut self, action: Action) -> &mut Binding {
        // Guaranteed present after `load()`.
        let idx = self
            .keybinds
            .iter()
            .position(|b| b.action == action)
            .expect("action present");
        &mut self.keybinds[idx]
    }

    pub fn chord_for(&self, action: Action) -> Option<&Chord> {
        self.keybinds
            .iter()
            .find(|b| b.action == action)
            .and_then(|b| b.chord.as_ref())
    }

    /// Did this action's bound chord fire this frame?
    pub fn triggered(&self, action: Action, i: &egui::InputState) -> bool {
        self.chord_for(action).is_some_and(|c| c.matches(i))
    }

    /// True if some OTHER action already uses this chord (conflict warning).
    pub fn conflict(&self, action: Action, chord: &Chord) -> Option<Action> {
        self.keybinds
            .iter()
            .find(|b| b.action != action && b.chord.as_ref() == Some(chord))
            .map(|b| b.action)
    }
}

/// The Settings window (currently: keyboard shortcuts). `capturing` holds the
/// action whose new key we're waiting for. Returns nothing; mutates config +
/// persists on change.
pub fn settings_window(
    ctx: &egui::Context,
    open: &mut bool,
    config: &mut Config,
    capturing: &mut Option<Action>,
) {
    if !*open {
        *capturing = None;
        return;
    }

    // Capture the next key press for a pending rebind.
    if let Some(action) = *capturing {
        let result: Option<Option<Chord>> = ctx.input(|i| {
            for e in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = e
                {
                    if *key == egui::Key::Escape {
                        return Some(None); // cancel
                    }
                    return Some(Some(Chord::from_event(*key, *modifiers)));
                }
            }
            None
        });
        if let Some(outcome) = result {
            if let Some(chord) = outcome {
                config.binding_mut(action).chord = Some(chord);
                config.save();
            }
            *capturing = None;
        }
    }

    egui::Window::new("Settings")
        .open(open)
        .resizable(true)
        .default_size([460.0, 520.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Keyboard Shortcuts");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Reset to defaults").clicked() {
                        config.reset_to_defaults();
                        config.save();
                        *capturing = None;
                    }
                });
            });
            ui.label(
                egui::RichText::new("Click a shortcut to rebind it, then press the new keys (Esc cancels).")
                    .weak(),
            );
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("keybind_grid")
                    .num_columns(3)
                    .striped(true)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        for action in Action::ALL {
                            let action = *action;
                            ui.label(action.label());

                            let is_capturing = *capturing == Some(action);
                            let text = if is_capturing {
                                "press keys…".to_string()
                            } else {
                                config
                                    .chord_for(action)
                                    .map(|c| c.label())
                                    .unwrap_or_else(|| "—".to_string())
                            };
                            // Warn if this binding collides with another action.
                            let clash = config
                                .chord_for(action)
                                .and_then(|c| config.conflict(action, c));
                            let btn = egui::Button::new(text).min_size(egui::vec2(140.0, 0.0));
                            let btn = if is_capturing {
                                btn.fill(egui::Color32::from_rgb(70, 55, 30))
                            } else if clash.is_some() {
                                btn.fill(egui::Color32::from_rgb(70, 30, 30))
                            } else {
                                btn
                            };
                            let mut resp = ui.add(btn);
                            if let Some(other) = clash {
                                resp = resp.on_hover_text(format!(
                                    "⚠ also bound to “{}”",
                                    other.label()
                                ));
                            }
                            if resp.clicked() {
                                *capturing = if is_capturing { None } else { Some(action) };
                            }

                            if ui.button("✕").on_hover_text("unbind").clicked() {
                                config.binding_mut(action).chord = None;
                                config.save();
                                if *capturing == Some(action) {
                                    *capturing = None;
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        });
}
