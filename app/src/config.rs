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
    PrevKey,
    NextKey,
    MissCheck,
    PrevCut,
    NextCut,
    ToggleLoop,
    NewDrawing,
    ClearCel,
    ClearCelAll,
    ToggleEraser,
    ToggleAlphaLock,
    Preset1,
    Preset2,
    Preset3,
    Preset4,
    Preset5,
    Preset6,
    Preset7,
    Preset8,
    CycleCelLayer,
    CycleCelLayerBack,
    ClearFrameKey,
    RemoveColumn,
    ToggleOnion,
    ToggleCompositeView,
    SelectTool,
    BrushTool,
    FillTool,
    SelectAll,
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
        Action::PrevKey,
        Action::NextKey,
        Action::MissCheck,
        Action::PrevCut,
        Action::NextCut,
        Action::ToggleLoop,
        Action::NewDrawing,
        Action::ClearCel,
        Action::ClearCelAll,
        Action::ToggleEraser,
        Action::ToggleAlphaLock,
        Action::Preset1,
        Action::Preset2,
        Action::Preset3,
        Action::Preset4,
        Action::Preset5,
        Action::Preset6,
        Action::Preset7,
        Action::Preset8,
        Action::CycleCelLayer,
        Action::CycleCelLayerBack,
        Action::ClearFrameKey,
        Action::RemoveColumn,
        Action::ToggleOnion,
        Action::ToggleCompositeView,
        Action::SelectTool,
        Action::BrushTool,
        Action::FillTool,
        Action::SelectAll,
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
            Action::ClearCel => "Clear active layer",
            Action::ClearCelAll => "Clear whole cel (all layers)",
            Action::PrevKey => "Jump to previous key (active column)",
            Action::NextKey => "Jump to next key (active column)",
            Action::MissCheck => "Miss check: dark-ground hole-hunt (shiage)",
            Action::PrevCut => "Previous cut",
            Action::NextCut => "Next cut",
            Action::ToggleLoop => "Loop playback latch",
            Action::ToggleEraser => "Brush / eraser toggle",
            Action::ToggleAlphaLock => "Alpha lock toggle (recolor within existing ink)",
            Action::Preset1 => "Brush preset 1",
            Action::Preset2 => "Brush preset 2",
            Action::Preset3 => "Brush preset 3",
            Action::Preset4 => "Brush preset 4",
            Action::Preset5 => "Brush preset 5",
            Action::Preset6 => "Brush preset 6",
            Action::Preset7 => "Brush preset 7",
            Action::Preset8 => "Brush preset 8",
            Action::CycleCelLayer => "Next cel layer",
            Action::CycleCelLayerBack => "Previous cel layer",
            Action::ClearFrameKey => "Lift key (hold extends)",
            Action::RemoveColumn => "Remove selected column",
            Action::ToggleOnion => "Toggle onion skin",
            Action::ToggleCompositeView => "Composite view (node-graph output)",
            Action::SelectTool => "Select / transform tool",
            Action::BrushTool => "Paint tool (brush)",
            Action::FillTool => "Flood fill tool",
            Action::SelectAll => "Select all",
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
            keys: vec![name.to_string()],
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
            // One verb, one key (room charters): Q/W step the exposure.
            PrevKey => Some(k("Q")),
            NextKey => Some(k("W")),
            MissCheck => Some(k("M")),
            PrevCut => Some(k("PageUp")),
            NextCut => Some(k("PageDown")),
            ToggleLoop => Some(k("P")),
            NewDrawing => Some(k("E")),
            ClearCel => Some(k("D")),
            ClearCelAll => Some(Chord {
                keys: vec!["D".to_string()],
                ctrl: false,
                shift: true,
                alt: false,
            }),
            ToggleEraser => Some(k("X")),
            ToggleAlphaLock => Some(k("L")),
            Preset1 => Some(k("1")),
            Preset2 => Some(k("2")),
            Preset3 => Some(k("3")),
            Preset4 => Some(k("4")),
            Preset5 => Some(k("5")),
            Preset6 => Some(k("6")),
            Preset7 => Some(k("7")),
            Preset8 => Some(k("8")),
            CycleCelLayer => Some(k("A")),
            CycleCelLayerBack => Some(Chord {
                keys: vec!["A".to_string()],
                ctrl: false,
                shift: true,
                alt: false,
            }),
            ClearFrameKey => Some(k("Backspace")),
            RemoveColumn => Some(k("Delete")),
            ToggleOnion => Some(k("O")),
            ToggleCompositeView => Some(k("C")),
            SelectTool => Some(k("V")),
            BrushTool => Some(k("B")),
            FillTool => Some(k("G")),
            SelectAll => Some(ctrl("A")),
            Undo => Some(ctrl("Z")),
            Redo => Some(ctrl("Y")),
            Save => Some(ctrl("S")),
            SaveAs => Some(Chord {
                keys: vec!["S".to_string()],
                ctrl: true,
                shift: true,
                alt: false,
            }),
            Open => Some(ctrl("O")),
            NewProject => Some(ctrl("N")),
        }
    }
}

/// A key combination: a set of (non-modifier) keys plus modifier flags. Held
/// together, they fire the action. Multi-key chords (e.g. A+S) are supported.
/// `keys` are egui [`egui::Key`] names; `ctrl` is the platform command modifier.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Chord {
    pub keys: Vec<String>,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    fn plain(key: &str) -> Self {
        Self {
            keys: vec![key.to_string()],
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    /// Build from a captured set of keys + the modifiers held at capture.
    pub fn from_capture(keys: Vec<String>, m: egui::Modifiers) -> Self {
        Self {
            keys,
            ctrl: m.command,
            shift: m.shift,
            alt: m.alt,
        }
    }

    /// Human-readable label, e.g. "Ctrl+Shift+S" or "A+S".
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
        let keys: Vec<&str> = self.keys.iter().map(|k| pretty_key(k)).collect();
        s.push_str(&keys.join("+"));
        s
    }

    /// Fires when every key is held, at least one was pressed THIS frame, and
    /// the modifiers match exactly.
    fn matches(&self, i: &egui::InputState) -> bool {
        if self.keys.is_empty() {
            return false;
        }
        let m = i.modifiers;
        if m.command != self.ctrl || m.shift != self.shift || m.alt != self.alt {
            return false;
        }
        let mut any_pressed = false;
        for name in &self.keys {
            let Some(k) = egui::Key::from_name(name) else {
                return false;
            };
            if !i.keys_down.contains(&k) {
                return false;
            }
            if i.key_pressed(k) {
                any_pressed = true;
            }
        }
        any_pressed
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

// ---- Pen / Tablet (Krita-derived; see research below) ----

/// Pressure response curve: remaps raw pen pressure 0..1 before it drives line
/// width. Points sorted ascending by x, endpoints pinned at (0,0)/(1,1).
/// Piecewise-linear (never overshoots [0,1], unlike a cubic spline).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PressureCurve {
    pub points: Vec<[f32; 2]>,
}

impl PressureCurve {
    pub fn linear() -> Self {
        Self {
            points: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        }
    }
    /// Above the diagonal: light touches produce more width (boosts a weak pen).
    pub fn soft() -> Self {
        Self {
            points: vec![[0.0, 0.0], [0.5, 0.7], [1.0, 1.0]],
        }
    }
    /// Below the diagonal: must press harder for width (thinner light strokes).
    pub fn hard() -> Self {
        Self {
            points: vec![[0.0, 0.0], [0.5, 0.3], [1.0, 1.0]],
        }
    }

    /// Remap `x` (0..1) through the curve.
    pub fn apply(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let p = &self.points;
        if p.len() < 2 {
            return x;
        }
        if x <= p[0][0] {
            return p[0][1];
        }
        if x >= p[p.len() - 1][0] {
            return p[p.len() - 1][1];
        }
        let (mut lo, mut hi) = (0usize, p.len() - 1);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if p[mid][0] <= x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (x0, y0, x1, y1) = (p[lo][0], p[lo][1], p[hi][0], p[hi][1]);
        let dx = x1 - x0;
        if dx <= 1e-6 {
            return y0;
        }
        (y0 + (y1 - y0) * (x - x0) / dx).clamp(0.0, 1.0)
    }
}

impl Default for PressureCurve {
    fn default() -> Self {
        Self::linear()
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PenConfig {
    #[serde(default)]
    pub pressure_curve: PressureCurve,
    /// Native tablet backend (Windows Ink RealTimeStylus via octotablet) on
    /// its dedicated input thread. Proven on the XP-Pen Artist 22 Pro —
    /// default ON. Off = egui Touch events; ANIMSTUDIO_NO_TABLET=1
    /// force-disables if a driver ever misbehaves.
    #[serde(default = "default_true")]
    pub native_tablet: bool,
}

fn default_true() -> bool {
    true
}

/// In-progress rebind: accumulates the keys held (release confirms the chord,
/// so multi-key combos like Ctrl+Shift+S or A+S can be built).
#[derive(Debug, Clone)]
pub struct RebindCapture {
    pub action: Action,
    keys: Vec<String>,
    modifiers: egui::Modifiers,
}

impl RebindCapture {
    pub fn new(action: Action) -> Self {
        Self {
            action,
            keys: Vec::new(),
            modifiers: egui::Modifiers::NONE,
        }
    }
}

// ---- Brush presets ----------------------------------------------------------

/// A named brush snapshot — applied whole from a keybind (1–8), the Presets
/// pane, or automatically when entering a workspace bound to it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushPreset {
    pub name: String,
    pub size_px: f32,
    pub flow: f32,
    pub opacity: f32,
    pub dyn_size: bool,
    pub dyn_opacity: bool,
    pub min_size: f32,
    /// None = keep the current brush colour when applying.
    pub color: Option<[u8; 4]>,
    /// Tilt dynamics (native tablet backend only — egui pen events carry no
    /// tilt). Serde defaults keep pre-tilt configs loading.
    #[serde(default)]
    pub tilt_size: bool,
    #[serde(default)]
    pub tilt_opacity: bool,
    /// Tilting flattens + rotates the dab along the lean (the stamp itself
    /// shows the tilt, like Krita's tip masks following the pen).
    #[serde(default)]
    pub tilt_shape: bool,
    /// How strongly tilt drives the enabled dynamics (0 = off, 1 = full:
    /// size up to 2×, opacity down to ¼ at a flat pen).
    #[serde(default = "default_tilt_strength")]
    pub tilt_strength: f32,
}

pub fn default_tilt_strength() -> f32 {
    0.5
}

impl Default for BrushPreset {
    fn default() -> Self {
        Self {
            name: "preset".into(),
            size_px: 14.0,
            flow: 1.0,
            opacity: 1.0,
            dyn_size: true,
            dyn_opacity: false,
            min_size: 0.0,
            color: None,
            tilt_size: false,
            tilt_opacity: false,
            tilt_shape: false,
            tilt_strength: default_tilt_strength(),
        }
    }
}

/// Starter presets mapped to the anime pipeline stages.
/// THE PENCIL BOX (genga room charter): the three named pencils, hotkeys
/// 1/2/3. Numbers ratified 2026-08-17 (research/ROOM-CHARTERS.md) — each is
/// justified against the engine's pressure pipeline, not taste.
pub fn pencil_box_presets() -> [BrushPreset; 3] {
    [
        // The ao construction pencil: builds tone by hatching, never solid —
        // "does not photograph" encoded as an opacity ceiling.
        BrushPreset {
            name: "atari".into(),
            size_px: 14.0,
            flow: 0.45,
            opacity: 0.8,
            dyn_size: true,
            dyn_opacity: true,
            min_size: 0.15,
            color: Some([83, 137, 196, 255]), // ao
            tilt_size: true,
            tilt_opacity: true,
            tilt_shape: true,
            tilt_strength: 0.7,
        },
        // The committed key line: pressure drives SIZE only, tight floor —
        // a light touch is a thin black line, never a grey one.
        BrushPreset {
            name: "genga".into(),
            size_px: 6.0,
            flow: 1.0,
            opacity: 1.0,
            dyn_size: true,
            dyn_opacity: false,
            min_size: 0.3,
            color: Some([25, 25, 30, 255]), // ink
            tilt_size: false,
            tilt_opacity: false,
            tilt_shape: false,
            tilt_strength: 0.5,
        },
        // The sakkan's correction: faint trial stroke at light pressure,
        // unmistakably loud over both other inks at full press.
        BrushPreset {
            name: "shusei".into(),
            size_px: 8.0,
            flow: 0.7,
            opacity: 1.0,
            dyn_size: true,
            dyn_opacity: true,
            min_size: 0.2,
            color: Some([228, 82, 47, 255]), // aka
            tilt_size: true,
            tilt_opacity: false,
            tilt_shape: true,
            tilt_strength: 0.5,
        },
    ]
}

pub fn default_presets() -> Vec<BrushPreset> {
    let mut v: Vec<BrushPreset> = pencil_box_presets().to_vec();
    v.extend([
        BrushPreset {
            name: "shiage fill".into(),
            size_px: 60.0,
            dyn_size: false,
            ..Default::default()
        },
        BrushPreset {
            name: "shadow airbrush".into(),
            size_px: 120.0,
            flow: 0.25,
            dyn_size: false,
            dyn_opacity: true,
            ..Default::default()
        },
    ]);
    v
}

// ---- Cel-layer defaults ----------------------------------------------------

/// Default brush colour per cel-layer NAME. Switching the active layer loads
/// its colour (a colour picked during the session overrides the default for
/// that layer name until the app closes). Editable in Settings → Layers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayersConfig {
    #[serde(default = "default_layer_colors")]
    pub colors: std::collections::BTreeMap<String, [u8; 4]>,
}

impl Default for LayersConfig {
    fn default() -> Self {
        Self {
            colors: default_layer_colors(),
        }
    }
}

/// Anime-pipeline conventions: ink line, blue-pencil rough, cool shadow,
/// warm highlight, sakkan-red correction.
pub fn default_layer_colors() -> std::collections::BTreeMap<String, [u8; 4]> {
    [
        ("line", [26, 26, 26, 255]),
        ("color", [222, 178, 140, 255]),
        ("shadow", [96, 112, 192, 255]),
        ("highlight", [255, 233, 168, 255]),
        ("correction", [224, 72, 72, 255]),
        ("rough", [116, 168, 232, 255]),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Fixed display order for the Layers settings page (pipeline order, top-first).
pub const LAYER_COLOR_ORDER: [&str; 6] = [
    "correction",
    "line",
    "rough",
    "highlight",
    "shadow",
    "color",
];

/// Which Settings page is showing (Krita-style category sidebar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsCategory {
    #[default]
    Shortcuts,
    Performance,
    Pen,
    Layers,
    Brushes,
    UiFeatures,
    Session,
}

/// THE SESSION's identity + connection config (PSD-session-room).
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct SessionConfig {
    /// The name other artists see on your cursor.
    #[serde(default = "default_username")]
    pub username: String,
    /// The room's API key (base32). The HOST generates it and shares it
    /// with invited artists; a guest pastes the host's key here.
    #[serde(default)]
    pub api_key: String,
    /// The host's TOTP secret (base32) — enrolled into Authy by manual
    /// entry. Guests never need this; they need the host's CODE.
    #[serde(default)]
    pub totp_secret: String,
    #[serde(default = "default_port")]
    pub host_port: u16,
    /// The last room address joined (host:port).
    #[serde(default)]
    pub last_addr: String,
}

fn default_username() -> String {
    "artist".into()
}
fn default_port() -> u16 {
    41100
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            username: default_username(),
            api_key: String::new(),
            totp_secret: String::new(),
            host_port: default_port(),
            last_addr: String::new(),
        }
    }
}

/// UI behaviour toggles (Settings -> UI Features).
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct UiConfig {
    /// Freeze the pane layout: no dragging, splitting, or closing panes
    /// until unlocked. The arrangement itself is untouched.
    #[serde(default)]
    pub lock_positions: bool,
    /// Exact UI descriptions: every control's hover names its kind, label
    /// and source precisely, and the x-sheet gains a pointer inspector —
    /// so edits can be asked for by an element's true name.
    #[serde(default)]
    pub exact_descriptions: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub keybinds: Vec<Binding>,
    #[serde(default)]
    pub perf: PerfConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default)]
    pub pen: PenConfig,
    #[serde(default)]
    pub layers: LayersConfig,
    #[serde(default = "default_presets")]
    pub presets: Vec<BrushPreset>,
    /// Transient: last Krita-import result line (not persisted).
    #[serde(skip)]
    pub last_import_note: String,
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
            ui: UiConfig::default(),
            session: SessionConfig::default(),
            pen: PenConfig::default(),
            layers: LayersConfig::default(),
            presets: default_presets(),
            last_import_note: String::new(),
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
        // The pencil box must exist even in configs saved before it
        // shipped; a user's own edits to same-named presets are kept.
        for p in pencil_box_presets().into_iter().rev() {
            if !cfg.presets.iter().any(|q| q.name == p.name) {
                cfg.presets.insert(0, p);
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

    /// Rebuild ONLY the key bindings from their defaults — the rest of
    /// the config (presets, palettes, session, perf) is untouched.
    /// AUDIT [1]: the old whole-config reset lived behind a plain button
    /// on the Shortcuts page and destroyed everything but shortcuts.
    pub fn reset_keybinds(&mut self) {
        self.keybinds = Action::ALL
            .iter()
            .map(|a| Binding {
                action: *a,
                chord: a.default_chord(),
            })
            .collect();
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
    capturing: &mut Option<RebindCapture>,
    category: &mut SettingsCategory,
    backend: &str,
    raster: Option<&mut bool>,
    session_action: &mut Option<SessionAction>,
    session_status: &str,
    hosting: bool,
    joined: bool,
    peer_names: &[String],
) {
    if !*open {
        *capturing = None;
        return;
    }
    // Only the Shortcuts page captures keys.
    if *category != SettingsCategory::Shortcuts {
        *capturing = None;
    } else if let Some(cap) = capturing.as_mut() {
        // Accumulate keys as they go down; commit on the first key release, so
        // the whole held combination (Ctrl+Shift+S, A+S, …) is captured.
        let (cancel, released) = ctx.input(|i| {
            let mut cancel = false;
            let mut released = false;
            for e in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } = e
                {
                    if *key == egui::Key::Escape {
                        cancel = true;
                    } else if *pressed {
                        let name = key.name().to_string();
                        if !cap.keys.contains(&name) {
                            cap.keys.push(name);
                        }
                        cap.modifiers = *modifiers;
                    } else {
                        released = true;
                    }
                }
            }
            (cancel, released)
        });
        if cancel {
            *capturing = None;
        } else if released && !cap.keys.is_empty() {
            config.binding_mut(cap.action).chord =
                Some(Chord::from_capture(cap.keys.clone(), cap.modifiers));
            config.save();
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
                    ui.selectable_value(category, SettingsCategory::Pen, "Pen / Tablet");
                    ui.selectable_value(category, SettingsCategory::Layers, "Layers");
                    ui.selectable_value(category, SettingsCategory::Brushes, "Brushes");
                    ui.selectable_value(category, SettingsCategory::UiFeatures, "UI Features");
                    ui.selectable_value(category, SettingsCategory::Session, "Session");
                });
                ui.separator();
                // Right: the selected page.
                ui.vertical(|ui| match category {
                    SettingsCategory::Shortcuts => shortcuts_page(ui, config, capturing),
                    SettingsCategory::Performance => performance_page(ui, config, backend, raster),
                    SettingsCategory::Pen => pen_page(ui, config),
                    SettingsCategory::Layers => layers_page(ui, config),
                    SettingsCategory::Brushes => brushes_page(ui, config),
                    SettingsCategory::UiFeatures => ui_features_page(ui, config),
                    SettingsCategory::Session => session_page(
                        ui,
                        config,
                        session_action,
                        session_status,
                        hosting,
                        joined,
                        peer_names,
                    ),
                });
            });
        });
}

fn shortcuts_page(ui: &mut egui::Ui, config: &mut Config, capturing: &mut Option<RebindCapture>) {
    ui.horizontal(|ui| {
        ui.heading("Keyboard Shortcuts");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // AUDIT [1]: this called reset_to_defaults(), which is
            // `*self = Self::default()` — it wiped every brush preset,
            // layer colour, perf setting, UI toggle AND the session's
            // room key + TOTP secret, then saved. One stray click.
            // Now: shortcuts ONLY, and held, and it says what it resets.
            if crate::plate::danger(ui, "RESET SHORTCUTS") {
                config.reset_keybinds();
                config.save();
                *capturing = None;
            }
        });
    });
    ui.label(
        egui::RichText::new(
            "Click a shortcut, hold the new combination, then release to set it (Esc cancels).",
        )
        .weak(),
    );
    ui.separator();
    let capturing_action = capturing.as_ref().map(|c| c.action);
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("keybind_grid")
            .num_columns(3)
            .striped(true)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for action in Action::ALL {
                    let action = *action;
                    ui.label(action.label());
                    let is_capturing = capturing_action == Some(action);
                    let text = if is_capturing {
                        "hold keys, release to set…".to_string()
                    } else {
                        config
                            .chord_for(action)
                            .map(|c| c.label())
                            .unwrap_or_else(|| "—".to_string())
                    };
                    let clash = config
                        .chord_for(action)
                        .and_then(|c| config.conflict(action, c));
                    let btn = egui::Button::new(text).min_size(egui::vec2(160.0, 0.0));
                    let btn = if is_capturing {
                        btn.fill(crate::plate::tally_well())
                    } else if clash.is_some() {
                        btn.fill(crate::plate::WELL)
                            .stroke(egui::Stroke::new(1.0, crate::plate::AKA))
                    } else {
                        btn
                    };
                    let mut resp = ui.add(btn);
                    if let Some(other) = clash {
                        resp = resp.on_hover_text(format!("⚠ also bound to “{}”", other.label()));
                    }
                    if resp.clicked() {
                        *capturing = if is_capturing {
                            None
                        } else {
                            Some(RebindCapture::new(action))
                        };
                    }
                    if ui.button("✕").on_hover_text("unbind").clicked() {
                        config.binding_mut(action).chord = None;
                        config.save();
                        if capturing_action == Some(action) {
                            *capturing = None;
                        }
                    }
                    ui.end_row();
                }
            });
    });
}

/// Settings -> UI Features: behaviour of the shell itself.
/// THE CONNECT WINDOW (owner's brief): the 2FA code is entered HERE, in
/// its own window, and nowhere else. Returns Some((addr, code)) once the
/// artist commits — the App performs the join.
pub fn connect_window(
    ctx: &egui::Context,
    open: &mut bool,
    addr: &mut String,
    code: &mut String,
    error: &str,
    busy: bool,
) -> Option<(String, String)> {
    if !*open {
        return None;
    }
    let mut out = None;
    let mut close = false;
    egui::Window::new("Join a room")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
        ui.set_min_width(320.0);
        ui.label(
        egui::RichText::new(
        "Enter the host's address and the 6-digit code from their authenticator. Codes expire every 30 seconds and \
        each one works once.",
                )
                .weak(),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
            ui.label("Room address");
            ui.add(
            egui::TextEdit::singleline(addr)
                        .hint_text("host:port")
                        .desired_width(180.0),
                );
            });
            ui.horizontal(|ui| {
            ui.label("2FA code");
            let r = ui.add(
            egui::TextEdit::singleline(code)
                        .hint_text("000000")
                        .desired_width(90.0)
                        .font(egui::TextStyle::Monospace),
                );
                code.retain(|c| c.is_ascii_digit());
                code.truncate(6);
                if r.lost_focus() {
                    // Committing with Enter is handled by the button below.
                    }
                    });
                    if !error.is_empty() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(error).color(crate::plate::AKA));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
            let ready = code.len() == 6 && !addr.trim().is_empty() && !busy;
            if ui
                    .add_enabled(ready, egui::Button::new(if busy { "Joining…" } else { "Join" }))
                    .clicked()
                {
                out = Some((addr.trim().to_string(), code.clone()));
                }
                if ui.button("Cancel").clicked() {
                close = true;
                }
            });
        });
    if close {
        *open = false;
        code.clear();
    }
    out
}

/// What the Session page asks the App to do (executed where the App owns
/// the net state — the settings dialog only edits config + shows status).
#[derive(Clone)]
pub enum SessionAction {
    StartHost,
    StopHost,
    /// Open the 2FA connect window (a guest joining).
    OpenConnect,
    Leave,
}

/// Settings -> Session (PSD-session-room): username, the room key, host
/// controls, and the connect button. The 2FA CODE is entered in a separate
/// window (see `connect_window`) — never on this page.
fn session_page(
    ui: &mut egui::Ui,
    config: &mut Config,
    action: &mut Option<SessionAction>,
    status: &str,
    hosting: bool,
    joined: bool,
    peer_names: &[String],
) {
    ui.heading("Session");
    ui.label(
        egui::RichText::new(
            "Draw together over a direct connection. Joining a room needs \
             its key AND a fresh 6-digit code from the host's authenticator.",
        )
        .weak(),
    );
    ui.separator();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Your name");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut config.session.username).desired_width(180.0))
            .on_hover_text("the name other artists see on your cursor")
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label("Room key");
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut config.session.api_key)
                    .desired_width(280.0)
                    .password(false),
            )
            .on_hover_text("the room's API key — the host generates it; a guest pastes it here")
            .changed();
    });
    // ENROLLMENT — shown whenever this machine holds a room secret,
    // hosting or not: you enroll in Authy BEFORE you open the room, and
    // the live code is how you check the enrollment took.
    if !config.session.totp_secret.trim().is_empty() {
        ui.separator();
        ui.label(
            egui::RichText::new("Authenticator (host only — guests never need this)")
                .color(crate::plate::LEGEND),
        );
        ui.horizontal(|ui| {
            ui.label("Secret");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut config.session.totp_secret)
                        .desired_width(280.0)
                        .font(egui::TextStyle::Monospace),
                )
                .on_hover_text("type this into Authy as a manual-entry account")
                .changed();
        });
        if let Some((code, secs)) = crate::net::current_code(config.session.totp_secret.trim()) {
            ui.horizontal(|ui| {
                ui.label("Code now");
                ui.label(
                    egui::RichText::new(code)
                        .monospace()
                        .size(20.0)
                        .color(crate::plate::STRUCK),
                );
                ui.label(
                    egui::RichText::new(format!("{secs}s left"))
                        .monospace()
                        .color(crate::plate::LEGEND),
                );
            });
            ui.label(
                egui::RichText::new(
                    "Authy should show this same number. If it does, enrollment took.",
                )
                .weak(),
            );
        } else {
            ui.label(
                egui::RichText::new("that secret is not valid base32 — regenerate it")
                    .color(crate::plate::AKA),
            );
        }
    }
    ui.separator();
    if joined {
        ui.label(egui::RichText::new("Connected to a room.").color(crate::plate::STRUCK));
        if !peer_names.is_empty() {
            ui.label(format!("With: {}", peer_names.join(", ")));
        }
        if ui.button("Leave room").clicked() {
            *action = Some(SessionAction::Leave);
        }
    } else if hosting {
        ui.label(
            egui::RichText::new("Hosting — share the room key and read codes from your Authy.")
                .color(crate::plate::STRUCK),
        );
        ui.horizontal(|ui| {
            ui.label("Port");
            ui.add(egui::DragValue::new(&mut config.session.host_port).range(1024..=65535));
        });
        ui.label(format!(
            "Artists here: {}",
            if peer_names.is_empty() {
                "just you".to_string()
            } else {
                peer_names.join(", ")
            }
        ));
        if ui.button("Stop hosting").clicked() {
            *action = Some(SessionAction::StopHost);
        }
    } else {
        ui.label("You are offline.");
        ui.horizontal(|ui| {
            if ui
                .button("Host a room")
                .on_hover_text("open your file to invited artists")
                .clicked()
            {
                *action = Some(SessionAction::StartHost);
            }
            if ui
                .button("Connect to a room…")
                .on_hover_text("join a host with the key + their 2FA code")
                .clicked()
            {
                *action = Some(SessionAction::OpenConnect);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Generate a key + secret for a new room:");
            if ui
                .button("Generate")
                .on_hover_text("a fresh room key and authenticator secret")
                .clicked()
            {
                config.session.api_key = crate::net::generate_key();
                config.session.totp_secret = crate::net::base32_encode(&{
                    let b: [u8; 20] = rand::random();
                    b
                });
                changed = true;
            }
        });
    }
    ui.separator();
    ui.label(
        egui::RichText::new(status)
            .italics()
            .color(crate::plate::LEGEND),
    );
    if changed {
        config.save();
    }
}

fn ui_features_page(ui: &mut egui::Ui, config: &mut Config) {
    ui.heading("UI Features");
    ui.separator();
    let mut changed = false;
    changed |= ui
        .checkbox(&mut config.ui.lock_positions, "Lock UI positions")
        .on_hover_text(
            "freeze the pane layout: no dragging, splitting, or closing \
             windows until unlocked. The arrangement itself is kept.",
        )
        .changed();
    changed |= ui
        .checkbox(&mut config.ui.exact_descriptions, "Exact UI descriptions")
        .on_hover_text(
            "every control's hover names its kind, label and source, and the \
             x-sheet gains a pointer inspector — useful for asking for edits \
             by an element's true name.",
        )
        .changed();
    if changed {
        config.save();
    }
}

fn performance_page(
    ui: &mut egui::Ui,
    config: &mut Config,
    backend: &str,
    raster: Option<&mut bool>,
) {
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

    // The brush ENGINE switch lives here, not in the tool strip: it is a
    // GPU backend choice, not a tool (spec §6 defect 2). None = no wgpu.
    match raster {
        Some(r) => {
            ui.checkbox(r, "GPU raster brush engine")
                .on_hover_text("uncheck to fall back to the vector brush");
        }
        None => {
            ui.label("GPU raster brush engine: unavailable (no wgpu surface)");
        }
    }
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

fn pen_page(ui: &mut egui::Ui, config: &mut Config) {
    {
        let mut changed = false;
        changed |= ui
            .checkbox(
                &mut config.pen.native_tablet,
                "Native tablet backend (Windows Ink) — applies after restart",
            )
            .on_hover_text(
                "Direct RealTimeStylus pen input (Krita-grade). If the app \
                 crashes with this on, launch with ANIMSTUDIO_NO_TABLET=1 to \
                 force it off, then untick here. Fallback = the standard \
                 pen-touch path.",
            )
            .changed();
        if changed {
            config.save();
        }
        ui.add_space(6.0);
    }
    ui.horizontal(|ui| {
        ui.heading("Pen / Tablet");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Reset to defaults").clicked() {
                config.pen = PenConfig::default();
                config.save();
            }
        });
    });
    ui.separator();
    let mut changed = false;
    ui.strong("Input Pressure Curve");
    ui.label(
    egui::RichText::new(
    "Remap pen pressure before it sets line width. Drag the curve up to make light touches thicker.",
        )
        .weak(),
    );
    ui.add_space(6.0);
    changed |= pressure_curve_editor(ui, &mut config.pen.pressure_curve);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Linear").clicked() {
            config.pen.pressure_curve = PressureCurve::linear();
            changed = true;
        }
        if ui
            .button("Soft")
            .on_hover_text("Light touches produce more width — boosts a pen that feels weak")
            .clicked()
        {
            config.pen.pressure_curve = PressureCurve::soft();
            changed = true;
        }
        if ui
            .button("Hard")
            .on_hover_text("Must press harder for the same width — thinner light strokes")
            .clicked()
        {
            config.pen.pressure_curve = PressureCurve::hard();
            changed = true;
        }
    });
    ui.add_space(10.0);
    ui.separator();
    ui.label(
        egui::RichText::new("Not available on this stack:")
            .weak()
            .italics(),
    );
    ui.label(
        egui::RichText::new(
            "• Tablet Input API (WinTab / Windows Ink) — the window toolkit delivers Windows Ink \
    pointer events only.\n• Use-mouse-events-for-right/middle-click — managed by the \
    window toolkit.",
        )
        .weak()
        .size(11.0),
    );
    if changed {
        config.save();
    }
}

/// Layers page: default brush colour per cel-layer name. Switching the active
/// layer loads its colour; a colour picked during the session overrides the
/// default for that layer name until the app closes.
fn layers_page(ui: &mut egui::Ui, config: &mut Config) {
    ui.heading("Layers");
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Default brush colour per layer. Switching to a layer loads its colour \
    (picking a colour while on a layer remembers it for this session).",
        )
        .weak(),
    );
    ui.add_space(8.0);
    let mut changed = false;
    for name in LAYER_COLOR_ORDER {
        let entry = config
            .layers
            .colors
            .entry(name.to_string())
            .or_insert([128, 128, 128, 255]);
        ui.horizontal(|ui| {
            let mut rgb = [entry[0], entry[1], entry[2]];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                *entry = [rgb[0], rgb[1], rgb[2], 255];
                changed = true;
            }
            ui.label(name);
        });
    }
    ui.add_space(8.0);
    if ui
        .button("Reset to defaults")
        .on_hover_text(
            "ink line, blue-pencil rough, cool shadow, warm highlight, sakkan-red correction",
        )
        .clicked()
    {
        config.layers.colors = default_layer_colors();
        changed = true;
    }
    if changed {
        config.save();
    }
}

/// Brushes page: edit the preset list (name, size, strength, dynamics, an
/// optional colour), reorder implicitly by position (slots 1–8 map to the
/// number keybinds), and import Krita community brushes (.kpp / .bundle).
fn brushes_page(ui: &mut egui::Ui, config: &mut Config) {
    ui.heading("Brush presets");
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Slots 1–8 fire from the number keys (rebindable in Shortcuts). \
    Bind a preset to a workspace in the ws menu to auto-load it per \
    workflow stage.",
        )
        .weak(),
    );
    ui.add_space(8.0);
    let mut changed = false;
    let mut remove: Option<usize> = None;
    egui::ScrollArea::vertical()
        .max_height(340.0)
        .show(ui, |ui| {
            for (i, p) in config.presets.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(if i < 8 {
                                format!("[{}]", i + 1)
                            } else {
                                "[·]".into()
                            })
                            .monospace()
                            .weak(),
                        );
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut p.name).desired_width(130.0))
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut p.size_px)
                                    .range(1.0..=300.0)
                                    .suffix(" px"),
                            )
                            .changed();
                        if ui
                            .small_button("✕")
                            .on_hover_text("delete preset")
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(34.0);
                        ui.spacing_mut().slider_width = 70.0;
                        changed |= ui
                            .add(egui::Slider::new(&mut p.flow, 0.05..=1.0).text("flow"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut p.opacity, 0.05..=1.0).text("op"))
                            .changed();
                        changed |= ui.checkbox(&mut p.dyn_size, "size dyn").changed();
                        changed |= ui.checkbox(&mut p.dyn_opacity, "op dyn").changed();
                        changed |= ui
                            .checkbox(&mut p.tilt_size, "tilt sz")
                            .on_hover_text("tilting the pen broadens the stroke (native ink pen)")
                            .changed();
                        changed |= ui
                            .checkbox(&mut p.tilt_opacity, "tilt op")
                            .on_hover_text("tilting the pen lightens the stroke (native ink pen)")
                            .changed();
                        changed |= ui
                            .checkbox(&mut p.tilt_shape, "tilt shape")
                            .on_hover_text(
                                "the stamp flattens and turns with the pen's lean (native ink pen)",
                            )
                            .changed();
                        // Optional fixed colour.
                        let mut has_color = p.color.is_some();
                        if ui
                            .checkbox(&mut has_color, "colour")
                            .on_hover_text("preset sets the brush colour when applied")
                            .changed()
                        {
                            p.color = has_color.then_some([26, 26, 26, 255]);
                            changed = true;
                        }
                        if let Some(c) = &mut p.color {
                            let mut rgb = [c[0], c[1], c[2]];
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                *c = [rgb[0], rgb[1], rgb[2], 255];
                                changed = true;
                            }
                        }
                    });
                    ui.add_space(4.0);
                });
            }
        });
    if let Some(i) = remove {
        config.presets.remove(i);
        changed = true;
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("＋ new preset").clicked() {
            config.presets.push(BrushPreset::default());
            changed = true;
        }
        if ui
            .button("import Krita brushes…")
            .on_hover_text(
                "community .kpp presets / .bundle packs — maps name, size, \
            opacity and flow onto our round brush (textured tips need the \
            fuller brush engine, later)",
            )
            .clicked()
            && let Some(paths) = rfd::FileDialog::new()
                .add_filter("Krita brushes", &["kpp", "bundle"])
                .pick_files()
        {
            let (ok, dup, failed) = crate::kpp::import_files(&paths, &mut config.presets);
            changed |= ok > 0;
            config.last_import_note =
                format!("imported {ok}, skipped {dup} duplicate(s), {failed} failed");
        }
    });
    if !config.last_import_note.is_empty() {
        ui.label(egui::RichText::new(&config.last_import_note).weak().small());
    }
    if changed {
        config.save();
    }
}

/// Draggable pressure-curve editor in a 0..1 box. Returns whether it changed.
fn pressure_curve_editor(ui: &mut egui::Ui, curve: &mut PressureCurve) -> bool {
    use egui::{Color32, Pos2, Rect, pos2, vec2};
    let mut changed = false;
    let (rect, _r) = ui.allocate_exact_size(vec2(220.0, 220.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4, crate::plate::WELL);
    painter.rect_stroke(
        rect,
        4,
        egui::Stroke::new(1.0, crate::plate::legend_dim()),
        egui::StrokeKind::Inside,
    );
    for i in 1..4 {
        let f = i as f32 / 4.0;
        painter.line_segment(
            [
                pos2(rect.left() + rect.width() * f, rect.top()),
                pos2(rect.left() + rect.width() * f, rect.bottom()),
            ],
            egui::Stroke::new(1.0, crate::plate::rule_beat()),
        );
        painter.line_segment(
            [
                pos2(rect.left(), rect.top() + rect.height() * f),
                pos2(rect.right(), rect.top() + rect.height() * f),
            ],
            egui::Stroke::new(1.0, crate::plate::rule_beat()),
        );
    }
    let to_screen = |p: [f32; 2]| {
        pos2(
            rect.left() + p[0] * rect.width(),
            rect.bottom() - p[1] * rect.height(),
        )
    };
    let to_curve = |s: Pos2| {
        [
            ((s.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            ((rect.bottom() - s.y) / rect.height()).clamp(0.0, 1.0),
        ]
    };
    // Plot the true (clamped) transfer function by sampling apply().
    let line: Vec<Pos2> = (0..=64)
        .map(|i| {
            let x = i as f32 / 64.0;
            to_screen([x, curve.apply(x)])
        })
        .collect();
    painter.add(egui::Shape::line(
        line,
        egui::Stroke::new(2.0, crate::plate::AO),
    ));
    let n = curve.points.len();
    for i in 1..n.saturating_sub(1) {
        let center = to_screen(curve.points[i]);
        let hr = ui.interact(
            Rect::from_center_size(center, vec2(16.0, 16.0)),
            ui.id().with(("pcurve", i)),
            egui::Sense::drag(),
        );
        if hr.dragged() {
            let np = to_curve(center + hr.drag_delta());
            let xmin = curve.points[i - 1][0] + 0.02;
            let xmax = curve.points[i + 1][0] - 0.02;
            curve.points[i] = [np[0].clamp(xmin, xmax), np[1].clamp(0.0, 1.0)];
            changed = true;
        }
        let col = if hr.hovered() || hr.dragged() {
            Color32::WHITE
        } else {
            crate::plate::AO
        };
        painter.circle_filled(center, 5.0, col);
    }
    painter.circle_filled(to_screen(curve.points[0]), 4.0, crate::plate::STRUCK);
    painter.circle_filled(to_screen(curve.points[n - 1]), 4.0, crate::plate::STRUCK);
    changed
}
