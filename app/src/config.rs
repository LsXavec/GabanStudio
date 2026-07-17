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
            NextFrame => Some(k("F")),
            PrevFrame => Some(k("S")),
            FirstFrame => Some(k("Home")),
            LastFrame => Some(k("End")),
            NewDrawing => Some(k("E")),
            ClearCel => Some(k("D")),
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

// ---- Performance settings (Krita-derived; see research/performance-settings.md) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FrameLatency {
    #[default]
    Low, // 1 frame — lowest pen latency
    Throughput, // 2 frames — smoother under heavy GPU load
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum CanvasFilter {
    Nearest,
    #[default]
    Linear,
}

impl CanvasFilter {
    pub fn wgpu(self) -> wgpu::FilterMode {
        match self {
            CanvasFilter::Nearest => wgpu::FilterMode::Nearest,
            CanvasFilter::Linear => wgpu::FilterMode::Linear,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PerfConfig {
    /// Sync to vertical refresh. Applied at startup (see note in the UI).
    #[serde(default)]
    pub vsync: bool,
    #[serde(default)]
    pub frame_latency: FrameLatency,
    #[serde(default)]
    pub canvas_filter: CanvasFilter,
    /// Max redraws/sec while idle/painting (playback overrides upward).
    #[serde(default = "default_fps_cap")]
    pub fps_cap: u32,
    #[serde(default)]
    pub show_fps: bool,
    /// Undo steps kept (0 = unlimited).
    #[serde(default = "default_undo_limit")]
    pub undo_limit: usize,
}

fn default_fps_cap() -> u32 {
    100
}
fn default_undo_limit() -> usize {
    200
}

impl Default for PerfConfig {
    fn default() -> Self {
        Self {
            vsync: false,
            frame_latency: FrameLatency::Low,
            canvas_filter: CanvasFilter::Linear,
            fps_cap: default_fps_cap(),
            show_fps: false,
            undo_limit: default_undo_limit(),
        }
    }
}

/// Which Settings page is showing (Krita-style category sidebar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsCategory {
    #[default]
    Shortcuts,
    Performance,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub keybinds: Vec<Binding>,
    #[serde(default)]
    pub perf: PerfConfig,
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
            perf: PerfConfig::default(),
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

/// The Settings window — Krita-style: a left category sidebar and a content
/// page. `capturing` holds the action whose new key we're waiting for.
pub fn settings_window(
    ctx: &egui::Context,
    open: &mut bool,
    config: &mut Config,
    capturing: &mut Option<Action>,
    category: &mut SettingsCategory,
    backend: &str,
) {
    if !*open {
        *capturing = None;
        return;
    }
    // Only the Shortcuts page captures keys.
    if *category != SettingsCategory::Shortcuts {
        *capturing = None;
    } else if let Some(action) = *capturing {
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
                        return Some(None);
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
        .default_size([640.0, 560.0])
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                // Left: category sidebar (Krita's Configure-dialog nav list).
                ui.vertical(|ui| {
                    ui.set_min_width(150.0);
                    ui.add_space(4.0);
                    ui.selectable_value(
                        category,
                        SettingsCategory::Shortcuts,
                        "Keyboard Shortcuts",
                    );
                    ui.selectable_value(category, SettingsCategory::Performance, "Performance");
                });
                ui.separator();
                // Right: the selected page.
                ui.vertical(|ui| match category {
                    SettingsCategory::Shortcuts => shortcuts_page(ui, config, capturing),
                    SettingsCategory::Performance => performance_page(ui, config, backend),
                });
            });
        });
}

fn shortcuts_page(ui: &mut egui::Ui, config: &mut Config, capturing: &mut Option<Action>) {
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
                        resp = resp.on_hover_text(format!("⚠ also bound to “{}”", other.label()));
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
}

fn performance_page(ui: &mut egui::Ui, config: &mut Config, backend: &str) {
    ui.horizontal(|ui| {
        ui.heading("Performance");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Reset to defaults").clicked() {
                config.perf = PerfConfig::default();
                config.save();
            }
        });
    });
    ui.separator();

    let mut changed = false;
    egui::ScrollArea::vertical().show(ui, |ui| {
        let p = &mut config.perf;

        ui.strong("Canvas Acceleration");
        ui.add_space(4.0);
        egui::Grid::new("perf_accel")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("V-Sync")
                    .on_hover_text("Sync to the monitor's refresh. Off = lowest latency.");
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut p.vsync, "").changed();
                    ui.label(egui::RichText::new("(applies after restart)").weak());
                });
                ui.end_row();

                ui.label("Frame latency");
                egui::ComboBox::from_id_salt("frame_latency")
                    .selected_text(match p.frame_latency {
                        FrameLatency::Low => "Low (1 frame)",
                        FrameLatency::Throughput => "Throughput (2 frames)",
                    })
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(&mut p.frame_latency, FrameLatency::Low, "Low (1 frame)")
                            .changed();
                        changed |= ui
                            .selectable_value(
                                &mut p.frame_latency,
                                FrameLatency::Throughput,
                                "Throughput (2 frames)",
                            )
                            .changed();
                    });
                ui.end_row();

                ui.label("Canvas scaling filter")
                    .on_hover_text("How the painted layer is filtered when zoomed. Nearest = crisp pixels, Linear = smooth.");
                egui::ComboBox::from_id_salt("canvas_filter")
                    .selected_text(match p.canvas_filter {
                        CanvasFilter::Nearest => "Nearest",
                        CanvasFilter::Linear => "Linear",
                    })
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(&mut p.canvas_filter, CanvasFilter::Nearest, "Nearest")
                            .changed();
                        changed |= ui
                            .selectable_value(&mut p.canvas_filter, CanvasFilter::Linear, "Linear")
                            .changed();
                    });
                ui.end_row();

                ui.label("Renderer");
                ui.add_enabled(false, egui::Label::new(backend))
                    .on_hover_text("Active GPU backend (chosen automatically by wgpu).");
                ui.end_row();
            });

        ui.add_space(10.0);
        ui.strong("Performance");
        ui.add_space(4.0);
        egui::Grid::new("perf_general")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("Max FPS while painting")
                    .on_hover_text("Caps idle redraws. Playback still runs at the project frame rate.");
                changed |= ui
                    .add(egui::Slider::new(&mut p.fps_cap, 30..=240).suffix(" fps"))
                    .changed();
                ui.end_row();

                ui.label("Show FPS overlay");
                changed |= ui.checkbox(&mut p.show_fps, "").changed();
                ui.end_row();

                ui.label("Undo history limit")
                    .on_hover_text("Max undo steps kept in memory. 0 = unlimited.");
                changed |= ui
                    .add(egui::DragValue::new(&mut p.undo_limit).range(0..=100_000).speed(5))
                    .changed();
                ui.end_row();
            });
    });

    if changed {
        config.save();
    }
}
