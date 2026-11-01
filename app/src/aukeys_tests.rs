//! AU10: arrows, a key for every step, no chord that waits on the pointer, and
//! Hide tools (spec 6.3 to 6.5, J5, R14). The prefix is `aukeys_`. The goldens
//! are hand-built v0.5.0 shapes: no `api_key`, no `totp_secret`.

use eframe::egui;
use egui::{Key, Modifiers, Rect, Vec2, vec2};
use egui_kittest::kittest::NodeT;
use serde_json::{Value, json};

use crate::audock_tests::null_rects;
use crate::ausay_tests::{line, raster_line};
use crate::config::{self, Action, Chord, Config, Ctx, KEYMAP_REV, Keymap};
use crate::floats::Float;
use crate::measure::{self, Act, Recorder};
use crate::say;
use crate::shortcuts;
use crate::testkit::{CTRL, EditorHarness};
use crate::workspace::{Stage, Workspace, Workspaces};
use crate::{Pane, Performed, plate, templates, visible_panes};

/// v0.5.0's primary chords as `git show ebbc5fc:app/src/config.rs` has them:
/// the frozen list. Ctrl+1 style names are the key names the file stores.
pub(crate) const FROZEN: [(&str, &str); 120] = [
    ("PlayPause", "Space"), ("NextFrame", "F"), ("PrevFrame", "S"), ("FirstFrame", "Home"),
    ("LastFrame", "End"), ("PrevKey", "Q"), ("NextKey", "W"), ("MissCheck", "M"),
    ("PrevCut", "PageUp"), ("NextCut", "PageDown"), ("ToggleLoop", "P"), ("NewDrawing", "E"),
    ("ClearCel", "D"), ("ClearCelAll", "Shift+D"), ("ToggleEraser", "X"), ("ToggleAlphaLock", "L"),
    ("Preset1", "1"), ("Preset2", "2"), ("Preset3", "3"), ("Preset4", "4"), ("Preset5", "5"),
    ("Preset6", "6"), ("Preset7", "7"), ("Preset8", "8"), ("CycleCelLayer", "A"),
    ("CycleCelLayerBack", "Shift+A"), ("ClearFrameKey", "Backspace"), ("RemoveColumn", "Delete"),
    ("ToggleOnion", "O"), ("ToggleCompositeView", "C"), ("SelectTool", "V"), ("BrushTool", "B"),
    ("FillTool", "G"), ("LassoFillTool", "Shift+G"), ("SelectAll", "Ctrl+A"), ("Undo", "Ctrl+Z"),
    ("Redo", "Ctrl+Y"), ("Save", "Ctrl+S"), ("SaveAs", "Ctrl+Shift+S"), ("Open", "Ctrl+O"),
    ("NewProject", "Ctrl+N"), ("CommandPalette", "Ctrl+K"), ("ShortcutSheet", "F1"),
    ("Settings", "Ctrl+Comma"), ("Quit", "Ctrl+Q"), ("ZoomFit", "Ctrl+0"), ("ZoomIn", "Ctrl+Equals"),
    ("ZoomOut", "Ctrl+Minus"), ("Zoom100", "Ctrl+1"), ("RotateViewLeft", "Ctrl+OpenBracket"),
    ("RotateViewRight", "Ctrl+CloseBracket"), ("ResetRotation", "Ctrl+Backslash"), ("MirrorView", "H"),
    ("BrushSmaller", "OpenBracket"), ("BrushBigger", "CloseBracket"), ("InsertFrames", "Insert"),
    ("DeleteFrames", "Shift+Delete"), ("HoldPlus", "Equals"), ("HoldMinus", "Minus"),
    ("RetimeOnes", "Alt+1"), ("RetimeTwos", "Alt+2"), ("RetimeThrees", "Alt+3"), ("Reverse", "Alt+V"),
    ("ColumnKindCel", "Alt+C"), ("ColumnKindBg", "Alt+B"), ("ColumnKindBook", "Alt+K"),
    ("ColumnKindRef", "Alt+G"), ("ToggleColumnHidden", "Alt+H"), ("ToggleColumnLocked", "Alt+L"),
    ("CopyCells", "Ctrl+C"), ("CutCells", "Ctrl+X"), ("PasteCells", "Ctrl+V"),
    ("PasteCellsInsert", "Ctrl+Shift+V"), ("TypeIntoCell", "Enter"), ("FrameCurves", "Shift+F"),
    ("CopyPixels", "Ctrl+C"), ("PastePixels", "Ctrl+V"), ("DuplicateCel", "Alt+D"),
    ("MakeInbetweens", "Alt+M"), ("KindKey", "Alt+K"), ("KindBreakdown", "Alt+B"),
    ("KindInbetween", "Alt+I"), ("KindUnset", "Alt+U"), ("PinToLightTable", "Alt+P"),
    ("ShiftTrace", "Alt+T"), ("ResetShift", "Alt+0"), ("ShowPaintCheck", "Ctrl+Shift+K"),
    ("AddNode", "Tab"), ("ShowRenderQueue", "Ctrl+Shift+E"), ("ToggleFinalLook", "Shift+C"),
    ("LoopIn", "Shift+I"), ("LoopOut", "Shift+O"), ("ClearLoop", "Alt+X"), ("ToggleScrub", "Shift+S"),
    ("ImportSound", "Ctrl+I"), ("AddSoundTrack", "Ctrl+Shift+I"), ("RamPreview", "0"),
    ("ShowCutBoard", "Ctrl+Shift+B"), ("ShowEpisode", "Ctrl+Shift+T"), ("PlayEpisode", "Ctrl+Shift+P"),
    ("ExportEdl", "Ctrl+Shift+L"), ("ExportColorSheet", "Ctrl+Shift+H"), ("ExportCutAsProject", "Ctrl+Shift+F"),
    ("MakeCutsFromPanels", "Ctrl+Shift+M"), ("RenderEpisode", "Alt+E"), ("AddRetakeNote", "Alt+N"),
    ("View3dFront", "1"), ("View3dBack", "Ctrl+1"), ("View3dRight", "3"), ("View3dLeft", "Ctrl+3"),
    ("View3dTop", "7"), ("View3dBottom", "Ctrl+7"), ("View3dCamera", "0"), ("View3dOrtho", "5"),
    ("View3dFrameAll", "Home"), ("SetCutCameraFromView", "Ctrl+Alt+0"), ("NewPage", "Ctrl+Shift+N"),
    ("ExportPages", "Ctrl+E"), ("ToggleRulerSnap", "Ctrl+2"), ("PanelTool", "K"),
];

/// The 34 of v0.5.0's Actions with no chord.
pub(crate) const FROZEN_BARE: [&str; 34] = [
    "QuickStart", "About", "History", "ResetLayout", "RoomLayout", "RoomDrawing", "RoomFinishing",
    "RoomComposite", "RoomEdit", "ShowCurves", "InterpHold", "InterpLinear", "InterpEase", "InterpEaseIn",
    "InterpEaseOut", "InterpBezier", "HandlesAuto", "HandlesSmooth", "HandlesFlat", "HandlesBroken",
    "ConvertColorsToSrgb", "BuildComposite", "ProjectSettings", "WelcomeScreen", "OpenSample", "PaperSize",
    "LipFlap", "BuildSatsuei", "Import3DModel", "Layout3dRoom", "MangaRoom", "ClipToPanels",
    "ReloadSequences", "Underlay3d",
];

/// Industry's seven overrides, v0.5.0.
const INDUSTRY: [(&str, &str); 7] = [
    ("NextFrame", "Period"), ("PrevFrame", "Comma"), ("NewDrawing", "N"), ("ToggleEraser", "E"),
    ("ClearCel", "Delete"), ("ClearCelAll", "Shift+Delete"), ("FrameCurves", "F"),
];

/// `Ctrl+Shift+K` to the shape the file stores.
fn chord_json(spec: &str) -> Value {
    let mut parts: Vec<&str> = spec.split('+').collect();
    let key = parts.pop().expect("a key");
    json!({
        "keys": [key],
        "ctrl": parts.contains(&"Ctrl"),
        "shift": parts.contains(&"Shift"),
        "alt": parts.contains(&"Alt"),
    })
}

fn chord(spec: &str) -> Chord {
    serde_json::from_value(chord_json(spec)).expect("a chord")
}

fn action(name: &str) -> Action {
    serde_json::from_value(json!(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// A v0.5.0 `keybinds` list for this keymap.
fn stock_keybinds(keymap: &str) -> Vec<Value> {
    let chord_of = |name: &str, frozen: &str| {
        let over = INDUSTRY.iter().find(|(n, _)| keymap == "Industry" && *n == name);
        chord_json(over.map_or(frozen, |(_, c)| c))
    };
    let also = |name: &str| match name {
        "Redo" => vec![chord_json("Ctrl+Shift+Z")],
        "CommandPalette" => vec![chord_json("F3")],
        "ZoomIn" => vec![chord_json("Ctrl+Plus")],
        _ => Vec::new(),
    };
    let mut list: Vec<Value> = FROZEN
        .iter()
        .map(|(name, frozen)| {
            let mut row = json!({ "action": name, "chord": chord_of(name, frozen) });
            if !also(name).is_empty() {
                row["also"] = json!(also(name));
            }
            row
        })
        .collect();
    list.extend(FROZEN_BARE.iter().map(|name| json!({ "action": name, "chord": null })));
    list
}

/// The whole of a v0.5.0 config: no `session` secrets, and a `render` section
/// worth keeping.
pub(crate) fn golden(keymap: &str) -> Value {
    json!({
        "keymap": keymap,
        "keybinds": stock_keybinds(keymap),
        "ui": {
            "scale": 100,
            "show_welcome": false,
            "quick_start": [true, false, false, false, false],
            "whats_new_seen": "0.5.0",
            "lock_positions": false,
            "exact_descriptions": false,
        },
        "render": { "ffmpeg": "C:/tools/ffmpeg.exe", "out_dir": "D:/renders" },
    })
}

/// A golden with one action's row replaced.
fn with_chord(mut config: Value, name: &str, spec: Option<&str>) -> Value {
    let list = config["keybinds"].as_array_mut().expect("a list");
    let row = list.iter_mut().find(|row| row["action"] == name).expect("a row");
    row["chord"] = spec.map_or(Value::Null, chord_json);
    config
}

fn write(dir: &std::path::Path, config: &Value) {
    let text = serde_json::to_string_pretty(config).expect("a golden");
    assert!(!text.contains("api_key") && !text.contains("totp_secret"), "goldens hold no secrets");
    std::fs::write(dir.join("config.json"), text).expect("a golden on disk");
}

/// A v0.5.0 data dir, on its way to this build's first launch.
pub(crate) fn upgraders_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a data dir");
    write(dir.path(), &golden("Gaban"));
    dir
}

/// What that launch says: each chord that moved, in one sentence.
pub(crate) const KEYS_MOVED: &str =
    "Overlay track moved from Alt+K to Alt+O · Background track moved from Alt+B to Alt+W";

/// The file's bytes, read the way the app reads them, once.
pub(crate) fn loaded(config: &Value) -> (tempfile::TempDir, Config) {
    let dir = tempfile::tempdir().expect("a data dir");
    write(dir.path(), config);
    let loaded = Config::load_in(dir.path());
    (dir, loaded)
}

fn chord_of(config: &Config, action: Action) -> Option<&Chord> {
    config.chord_for(action)
}

const SIX_STEPS: [(Action, &str, Stage); 6] = [
    (Action::RoomLayout, "F4", Stage::Layout),
    (Action::RoomDrawing, "F5", Stage::Drawing),
    (Action::RoomEdit, "F6", Stage::Edit),
    (Action::RoomFinishing, "F7", Stage::InkPaint),
    (Action::RoomComposite, "F8", Stage::Composite),
    (Action::RoomExport, "F9", Stage::Export),
];

// ---- the goldens ----

/// Today's stock Gaban config takes the two moves, the steps' keys and the
/// arrows, and its people see no lessons.
#[test]
fn aukeys_stock_gaban_config_migrates() {
    let (_dir, config) = loaded(&golden("Gaban"));
    assert_eq!(config.ui.keymap_rev, KEYMAP_REV);
    assert_eq!(chord_of(&config, Action::ColumnKindBook), Some(&chord("Alt+O")));
    assert_eq!(chord_of(&config, Action::ColumnKindBg), Some(&chord("Alt+W")));
    assert_eq!(chord_of(&config, Action::KindKey), Some(&chord("Alt+K")), "the stayer");
    assert_eq!(chord_of(&config, Action::KindBreakdown), Some(&chord("Alt+B")), "the stayer");
    for (step, key, _) in SIX_STEPS {
        assert_eq!(chord_of(&config, step), Some(&chord(key)), "{step:?}");
    }
    let back = config.binding(Action::PrevFrame).expect("bound");
    assert_eq!(back.chord, Some(chord("S")));
    assert_eq!(back.also, vec![chord("Left"), chord("Up")]);
    let on = config.binding(Action::NextFrame).expect("bound");
    assert_eq!(on.chord, Some(chord("F")));
    assert_eq!(on.also, vec![chord("Right"), chord("Down")]);
    // The 118: all but the two moves, read off the migrated config.
    let same = FROZEN.iter().filter(|(name, spec)| chord_of(&config, action(name)) == Some(&chord(spec))).count();
    assert_eq!(same, 118, "118 of 120 stay");
    assert_eq!(config.ui.hints_seen, u32::MAX, "an upgrader has not come to be taught");
    assert_eq!(config.load_notes, ["Overlay track moved from Alt+K to Alt+O", "Background track moved from Alt+B to Alt+W"]);
}

/// Industry's seven overrides stand: its frame steps stay on Comma and Period,
/// and the arrows stand beside them.
#[test]
fn aukeys_stock_industry_keeps_comma_and_period() {
    let (_dir, config) = loaded(&golden("Industry"));
    assert_eq!(config.keymap, Keymap::Industry);
    for (name, spec) in INDUSTRY {
        assert_eq!(chord_of(&config, action(name)), Some(&chord(spec)), "{name}");
    }
    assert_eq!(config.binding(Action::PrevFrame).expect("bound").also, vec![chord("Left"), chord("Up")]);
    assert_eq!(config.binding(Action::NextFrame).expect("bound").also, vec![chord("Right"), chord("Down")]);
    assert_eq!(chord_of(&config, Action::ColumnKindBook), Some(&chord("Alt+O")));
    assert_eq!(chord_of(&config, Action::ColumnKindBg), Some(&chord("Alt+W")));
    assert_eq!(chord_of(&config, Action::RoomExport), Some(&chord("F9")));
}

/// A chord the artist chose is theirs: Book on a key of their own stays there,
/// and Alt+K keeps whatever they put on it.
#[test]
fn aukeys_users_alt_k_is_left_alone() {
    let mine = with_chord(golden("Gaban"), "ColumnKindBook", Some("Ctrl+Alt+K"));
    let (_dir, config) = loaded(&mine);
    assert_eq!(chord_of(&config, Action::ColumnKindBook), Some(&chord("Ctrl+Alt+K")));
    assert_eq!(chord_of(&config, Action::ColumnKindBg), Some(&chord("Alt+W")), "the other still moves");
    assert_eq!(config.load_notes, ["Background track moved from Alt+B to Alt+W"], "no word about Book");
    // The artist took Alt+K itself for another verb, and gave Book away.
    let mine = with_chord(with_chord(golden("Gaban"), "ColumnKindBook", None), "KindKey", Some("Ctrl+Alt+J"));
    let mine = with_chord(mine, "History", Some("Alt+K"));
    let (_dir, config) = loaded(&mine);
    assert_eq!(chord_of(&config, Action::History), Some(&chord("Alt+K")));
    assert_eq!(chord_of(&config, Action::ColumnKindBook), None, "an action they unbound stays unbound");
    assert_eq!(chord_of(&config, Action::KindKey), Some(&chord("Ctrl+Alt+J")));
}

/// Alt+O is the artist's already: Book stays on Alt+K, one note says so, and
/// the verb on Alt+O keeps it.
#[test]
fn aukeys_users_alt_o_keeps_book_with_one_note() {
    let mine = with_chord(golden("Gaban"), "History", Some("Alt+O"));
    let (_dir, config) = loaded(&mine);
    assert_eq!(chord_of(&config, Action::ColumnKindBook), Some(&chord("Alt+K")), "stays");
    assert_eq!(chord_of(&config, Action::History), Some(&chord("Alt+O")), "theirs");
    assert_eq!(chord_of(&config, Action::ColumnKindBg), Some(&chord("Alt+W")));
    let about_book: Vec<&String> = config.load_notes.iter().filter(|n| n.contains("Overlay")).collect();
    assert_eq!(about_book, ["Overlay track stays on Alt+K: Alt+O is taken"], "one note");
}

/// F5 is the artist's: that verb keeps it, Draw stays bare, and the other
/// steps still arrive.
#[test]
fn aukeys_users_f5_is_kept() {
    let mine = with_chord(golden("Gaban"), "History", Some("F5"));
    let (_dir, config) = loaded(&mine);
    assert_eq!(chord_of(&config, Action::History), Some(&chord("F5")));
    assert_eq!(chord_of(&config, Action::RoomDrawing), None, "no key fought for");
    for (step, key) in [(Action::RoomLayout, "F4"), (Action::RoomEdit, "F6"), (Action::RoomFinishing, "F7"), (Action::RoomComposite, "F8")] {
        assert_eq!(chord_of(&config, step), Some(&chord(key)), "{step:?}");
    }
    // A step the artist gave a key of their own keeps it, and takes no second.
    let mine = with_chord(golden("Gaban"), "RoomDrawing", Some("Ctrl+F5"));
    let (_dir, config) = loaded(&mine);
    assert_eq!(chord_of(&config, Action::RoomDrawing), Some(&chord("Ctrl+F5")));
    assert!(config.binding(Action::RoomDrawing).expect("bound").also.is_empty());
}

/// T is the artist's: that verb keeps it, and Hide tools takes no key.
#[test]
fn aukeys_users_t_is_kept() {
    let mine = with_chord(golden("Gaban"), "History", Some("T"));
    let (_dir, config) = loaded(&mine);
    assert_eq!(chord_of(&config, Action::History), Some(&chord("T")));
    let hide = config.binding(Action::HideTools).expect("a row");
    assert_eq!(hide.chords().count(), 0, "no key fought for");
}

/// An arrow on another verb stays there, and the frame verb takes the rest of
/// its pair. A frame verb the artist unbound takes none, and its arrows stay
/// free.
#[test]
fn aukeys_users_arrows_are_kept() {
    let mine = with_chord(golden("Gaban"), "History", Some("Left"));
    let (_dir, config) = loaded(&mine);
    assert_eq!(chord_of(&config, Action::History), Some(&chord("Left")));
    assert_eq!(config.binding(Action::PrevFrame).expect("a row").also, [chord("Up")], "the free one");
    assert_eq!(config.binding(Action::NextFrame).expect("a row").also, [chord("Right"), chord("Down")]);

    let mine = with_chord(golden("Gaban"), "PrevFrame", None);
    let (_dir, config) = loaded(&mine);
    assert_eq!(config.binding(Action::PrevFrame).expect("a row").chords().count(), 0, "stays unbound");
    assert_eq!(config.binding(Action::NextFrame).expect("a row").also, [chord("Right"), chord("Down")]);
    for free in ["Left", "Up"] {
        assert!(!config.keybinds.iter().any(|b| b.chords().any(|c| *c == chord(free))), "{free}");
    }
}

/// The migration runs once: the file it wrote loads to itself, the notes say
/// nothing, and a chord changed after it stays changed.
#[test]
fn aukeys_second_load_changes_nothing() {
    let (dir, first) = loaded(&golden("Gaban"));
    let bytes = std::fs::read(dir.path().join("config.json")).expect("written back at once");
    assert_eq!(serde_json::from_slice::<Value>(&bytes).expect("json")["ui"]["keymap_rev"], KEYMAP_REV);
    let second = Config::load_in(dir.path());
    assert!(second.load_notes.is_empty(), "{:?}", second.load_notes);
    assert!(!second.migrated);
    assert_eq!(second.keybinds, first.keybinds);
    assert_eq!(std::fs::read(dir.path().join("config.json")).expect("still there"), bytes, "not written again");
    // The artist puts Book back: a later load does not move it again.
    let mut mine = second;
    mine.binding_mut(Action::ColumnKindBook).chord = Some(chord("Alt+K"));
    mine.ui.hints_seen = 0;
    mine.save_to(dir.path()).expect("saved");
    let third = Config::load_in(dir.path());
    assert_eq!(chord_of(&third, Action::ColumnKindBook), Some(&chord("Alt+K")));
    assert_eq!(third.ui.hints_seen, 0, "the mask went on once");
}

/// A file that stands but cannot be read is not an upgrade: the load must not
/// answer it by writing defaults over the artist's keys and session. A folder
/// in the file's place is the failure a test can hold.
#[test]
fn aukeys_unreadable_config_is_not_written_over() {
    let dir = tempfile::tempdir().expect("a data dir");
    let file = dir.path().join("config.json");
    std::fs::create_dir(&file).expect("in the file's place");
    let config = Config::load_in(dir.path());
    assert!(!config.migrated, "nothing in hand to move");
    assert_eq!(config.ui.keymap_rev, KEYMAP_REV);
    assert_eq!(config.ui.hints_seen, 0, "an upgrader is someone whose file was read");
    assert!(config.load_notes.is_empty(), "{:?}", config.load_notes);
    assert!(file.is_dir(), "left where it stood");
}

/// A fresh install has nothing to move: it starts on the new defaults, stamped,
/// with every lesson still to come and nothing written.
#[test]
fn aukeys_fresh_install_stamps_and_moves_nothing() {
    let dir = tempfile::tempdir().expect("a data dir");
    let (config, notes) = Config::load_from(dir.path());
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(config.ui.keymap_rev, KEYMAP_REV);
    assert!(!config.migrated);
    assert_eq!(config.ui.hints_seen, 0, "a new hand is taught");
    assert_eq!(config.keybinds, config::default_binds(Keymap::Gaban));
    let fresh = Config::load_in(dir.path());
    assert!(!dir.path().join("config.json").exists(), "nothing to save yet");
    assert_eq!(fresh.ui.keymap_rev, KEYMAP_REV);
    // `Config::load`'s other road: no data dir at all.
    let bare = Config::fresh();
    assert_eq!(bare.ui.keymap_rev, KEYMAP_REV, "stamped, or the next launch would migrate it");
    assert!(bare.load_notes.is_empty(), "{:?}", bare.load_notes);
    assert!(!bare.migrated);
    assert_eq!(bare.keybinds, config::default_binds(Keymap::Gaban));
}

/// Reset and a keymap switch reach the same chords a migration does, and
/// neither takes the stamp away.
#[test]
fn aukeys_reset_and_set_keymap_agree() {
    let (_dir, migrated) = loaded(&golden("Gaban"));
    let mut reset = migrated.clone();
    reset.reset_keybinds();
    let held = |c: &Config| -> Vec<(Action, Option<Chord>, Vec<Chord>)> {
        let mut all: Vec<_> = c.keybinds.iter().map(|b| (b.action, b.chord.clone(), b.also.clone())).collect();
        all.sort_by_key(|(a, ..)| format!("{a:?}"));
        all
    };
    assert_eq!(held(&reset), held(&migrated), "a migrated stock config is the new default");
    assert_eq!(reset.ui.keymap_rev, KEYMAP_REV);

    let mut switched = migrated.clone();
    switched.set_keymap(Keymap::Industry);
    assert_eq!(chord_of(&switched, Action::NextFrame), Some(&chord("Period")));
    assert_eq!(switched.binding(Action::NextFrame).expect("bound").also, vec![chord("Right"), chord("Down")], "the arrows stay");
    switched.set_keymap(Keymap::Gaban);
    assert_eq!(held(&switched), held(&migrated), "and back");

    let (_dir, industry) = loaded(&golden("Industry"));
    let mut reset = industry.clone();
    reset.reset_keybinds();
    assert_eq!(held(&reset), held(&industry), "Industry too");
}

/// A config from before the steps had rows at all: a file with none of them.
#[test]
fn aukeys_f4_to_f8_arrive_on_an_old_config() {
    let mut old = golden("Gaban");
    let list = old["keybinds"].as_array_mut().expect("a list");
    list.retain(|row| !row["action"].as_str().expect("a name").starts_with("Room"));
    let (_dir, config) = loaded(&old);
    for (step, key, _) in SIX_STEPS {
        assert_eq!(chord_of(&config, step), Some(&chord(key)), "{step:?}");
    }
    // Bare rows and no rows arrive at the same place.
    let (_dir, bare) = loaded(&golden("Gaban"));
    assert_eq!(chord_of(&config, Action::RoomEdit), chord_of(&bare, Action::RoomEdit));
    // Pressed, not just stored.
    let dir = tempfile::tempdir().expect("a data dir");
    write(dir.path(), &old);
    let mut h = EditorHarness::with_dir_shipped(dir);
    h.key(Key::F7);
    assert_eq!(h.editor().stage, Some(Stage::InkPaint));
}

/// The rest of the file comes through untouched: the render section by name.
#[test]
fn aukeys_render_survives_migration() {
    let (dir, config) = loaded(&golden("Gaban"));
    assert_eq!(config.render.ffmpeg, "C:/tools/ffmpeg.exe");
    assert_eq!(config.render.out_dir, "D:/renders");
    let bytes = std::fs::read(dir.path().join("config.json")).expect("written back");
    let file: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(file["render"], json!({ "ffmpeg": "C:/tools/ffmpeg.exe", "out_dir": "D:/renders" }));
    assert_eq!(file["ui"]["whats_new_seen"], "0.5.0", "what it said stands");
    assert_eq!(file["ui"]["quick_start"], json!([true, false, false, false, false]));
    assert_eq!(file["ui"]["lock_positions"], false, "an upgrader keeps their unlocked panels");
}

/// The frozen list over today's defaults, in both keymaps.
#[test]
fn aukeys_only_the_two_named_chords_moved() {
    let gaban = config::default_binds(Keymap::Gaban);
    let primary = |binds: &[config::Binding], name: &str| {
        binds.iter().find(|b| b.action == action(name)).and_then(|b| b.chord.clone())
    };
    let moved: Vec<&str> = FROZEN.iter().filter(|(n, c)| primary(&gaban, n) != Some(chord(c))).map(|(n, _)| *n).collect();
    assert_eq!(moved, ["ColumnKindBg", "ColumnKindBook"], "118 of 120 unchanged");
    assert_eq!(FROZEN.len() - moved.len(), 118);
    let industry = config::default_binds(Keymap::Industry);
    let over = |name: &str| INDUSTRY.iter().find(|(n, _)| *n == name).map(|(_, c)| chord(c));
    let moved: Vec<&str> = FROZEN
        .iter()
        .filter(|(n, c)| primary(&industry, n) != Some(over(n).unwrap_or_else(|| chord(c))))
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(moved, ["ColumnKindBg", "ColumnKindBook"], "Industry's seven stand");
    assert_eq!(config::clashes(&gaban), vec![], "the new keys clash with nothing");
    assert_eq!(config::clashes(&industry), vec![]);
}

// ---- the keys ----

fn stepped() -> EditorHarness {
    let mut state = crate::doc::AppState::new_project("t", 640, 480, 24, 72.0);
    state.set_cut_length(12);
    state.new_drawing_at_frame();
    state.goto(5);
    let mut h = EditorHarness::with_state_shipped(state);
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    h.run();
    h
}

/// All four, in both keymaps, on a shipped install.
#[test]
fn aukeys_arrows_step_frames() {
    let mut h = stepped();
    for (key, frame) in [(Key::ArrowRight, 6), (Key::ArrowDown, 7), (Key::ArrowLeft, 6), (Key::ArrowUp, 5)] {
        h.key(key);
        assert_eq!(h.state().view.frame, frame, "{key:?}");
    }
    let mut h = stepped();
    h.rig_mut().config.set_keymap(Keymap::Industry);
    h.key(Key::ArrowRight);
    assert_eq!(h.state().view.frame, 6, "Industry");
    h.key(Key::ArrowUp);
    assert_eq!(h.state().view.frame, 5, "Industry");
    // The frame steps' own keys still step.
    h.rig_mut().config.set_keymap(Keymap::Gaban);
    h.key(Key::F);
    assert_eq!(h.state().view.frame, 6, "F");
    // A modifier makes it another chord: no step.
    h.key_mods(Modifiers::SHIFT, Key::ArrowRight);
    assert_eq!(h.state().view.frame, 6, "Shift+Right is nobody's");
}

/// The focus rule: a widget that holds the keys keeps its arrows, and the
/// frame stays. After a plain click on a button the keys are the app's again.
#[test]
fn aukeys_arrows_leave_focused_widgets_alone() {
    let mut h = stepped();
    h.perform(Action::GoToFrame);
    h.run_frames(2);
    assert!(h.typing(), "the counter is a field");
    let frame = h.state().view.frame;
    for key in [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown] {
        h.key(key);
        assert_eq!(h.state().view.frame, frame, "{key:?} moved the field's caret");
    }
    h.key(Key::Escape);
    h.run_frames(2);
    assert!(!h.typing());

    // Tab to a key on the band: not a text field, and it holds the arrows.
    let mut h = stepped();
    h.key(Key::Tab);
    h.run_frames(2);
    let held = h.ctx().memory(|m| m.focused());
    assert!(held.is_some(), "Tab gives the first control the keys");
    assert!(!h.typing());
    let frame = h.state().view.frame;
    h.key(Key::ArrowRight);
    assert_eq!(h.state().view.frame, frame, "the arrow walked the focus, and stepped no frame");
    h.key(Key::Escape);
    h.run_frames(2);
    assert!(h.ctx().memory(|m| m.focused()).is_none(), "Escape lets go");
    h.key(Key::ArrowRight);
    assert_eq!(h.state().view.frame, frame + 1, "the keys came back");

    // A click on a button leaves no focus behind it.
    let mut h = stepped();
    h.click(crate::vocab::STEP_PLAN);
    h.run_frames(2);
    h.key(Key::ArrowRight);
    assert_eq!(h.state().view.frame, 6, "a button click takes no arrows");
}

/// Every step answers to its key, pressed, in the trade's order. AU18: F4 is
/// the bar's first step, and each bar reaches all its pills from the keys: J5's
/// 5 of 5, 4 of 4 and 3 of 3.
#[test]
fn aukeys_each_bar_has_its_keys() {
    let mut h = stepped();
    for (step, key, stage) in SIX_STEPS {
        let bound = h.rig().config.chord_for(step).expect("a key");
        assert_eq!(bound, &chord(key), "{step:?}");
        h.key(Key::from_name(key).expect("a key"));
        let stage = if stage == Stage::Layout { Stage::Drawing } else { stage };
        assert_eq!(h.editor().stage, Some(stage), "{key} goes to {step:?}");
    }
    for keymap in Keymap::ALL {
        let binds = config::default_binds(*keymap);
        for (step, key, _) in SIX_STEPS {
            let row = binds.iter().find(|b| b.action == step).expect("a row");
            assert_eq!(row.chord, Some(chord(key)), "{keymap:?} {step:?}");
        }
    }
    for t in [&templates::INTENTS[templates::PICTURE], &templates::INTENTS[templates::ANIMATION], &templates::ALL[templates::COMIC]] {
        let mut h = EditorHarness::with_state_shipped(t.build());
        h.run();
        let mut reached = Vec::new();
        for (_, key, _) in SIX_STEPS {
            h.key(Key::from_name(key).expect("a key"));
            let here = h.editor().stage.expect("on a step");
            if !reached.contains(&here) {
                reached.push(here);
            }
        }
        let bar: Vec<Stage> = h.editor().bar().iter().map(|step| step.stage).collect();
        assert_eq!(reached, bar, "{}", t.name);
    }
}

/// The verbs one chord reaches over one pane.
fn verbs_on(binds: &[config::Binding], c: &Chord, here: Ctx) -> Vec<Action> {
    let mut on: Vec<Action> = binds
        .iter()
        .filter(|b| b.chords().any(|d| d == c) && shortcuts::aimed_here(b.action, c, binds, here))
        .map(|b| b.action)
        .collect();
    on.sort_by_key(|a| format!("{a:?}"));
    on
}

/// The chords that do one thing over one pane and another over the other.
fn pointer_dependent(binds: &[config::Binding], a: Ctx, b: Ctx) -> Vec<String> {
    let mut chords: Vec<Chord> = binds.iter().flat_map(|b| b.chords().cloned()).collect();
    chords.sort_by_key(Chord::label);
    chords.dedup();
    let differ = |c: &&Chord| {
        let (over_a, over_b) = (verbs_on(binds, c, a), verbs_on(binds, c, b));
        !over_a.is_empty() && !over_b.is_empty() && over_a != over_b
    };
    chords.iter().filter(differ).map(Chord::label).collect()
}

/// Spec 6.4's walk over both keymaps. The list is the one allowed list:
/// Copy and Paste mean the sheet's cells or the canvas's pixels by design, and
/// Industry's Delete pair has always read the pane. Alt+K and Alt+B are the
/// two chords this part took off it.
#[test]
fn aukeys_no_chord_depends_on_the_pointer() {
    let gaban = config::default_binds(Keymap::Gaban);
    assert_eq!(pointer_dependent(&gaban, Ctx::Canvas, Ctx::Sheet), ["Ctrl+C", "Ctrl+V"], "Gaban");
    let industry = config::default_binds(Keymap::Industry);
    let walked = pointer_dependent(&industry, Ctx::Canvas, Ctx::Sheet);
    assert_eq!(walked, ["Ctrl+C", "Ctrl+V", "Delete", "Shift+Delete"], "Industry");
    // The two named moves, answered over each pane by name.
    for binds in [&gaban, &industry] {
        for (spec, verb) in [("Alt+K", Action::KindKey), ("Alt+B", Action::KindBreakdown)] {
            for here in [Ctx::Canvas, Ctx::Sheet, Ctx::Global] {
                assert_eq!(verbs_on(binds, &chord(spec), here), [verb], "{spec} over {here:?}");
            }
        }
        for (spec, verb) in [("Alt+O", Action::ColumnKindBook), ("Alt+W", Action::ColumnKindBg)] {
            assert_eq!(verbs_on(binds, &chord(spec), Ctx::Sheet), [verb], "{spec} over the sheet");
            assert!(verbs_on(binds, &chord(spec), Ctx::Canvas).is_empty(), "{spec} is the sheet's: no column changes unseen");
        }
    }
    // The old pair, put back: the walk sees it, so the walk can see.
    let mut old = gaban.clone();
    for (verb, spec) in [(Action::ColumnKindBook, "Alt+K"), (Action::ColumnKindBg, "Alt+B")] {
        old.iter_mut().find(|b| b.action == verb).expect("a row").chord = Some(chord(spec));
    }
    let walked = pointer_dependent(&old, Ctx::Canvas, Ctx::Sheet);
    assert_eq!(walked.iter().filter(|c| ["Alt+K", "Alt+B"].contains(&c.as_str())).count(), 2, "{walked:?}");
    // 3D's numbers read the pane, by design, and only there.
    for binds in [&gaban, &industry] {
        let walked = pointer_dependent(binds, Ctx::Canvas, Ctx::View3D);
        assert_eq!(walked, ["0", "1", "3", "5", "7", "Ctrl+1", "Home"], "6.4's 3D row");
    }
}

// ---- Hide tools ----

fn shipped() -> EditorHarness {
    let mut h = EditorHarness::as_shipped();
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    h.kit_mut().set_pixels_per_point(1.0);
    h.run();
    h.run_frames(4);
    h
}

/// `shipped`, after `An animation`: the place in dots, and the Timing step.
fn animation() -> EditorHarness {
    let mut h = EditorHarness::as_animation();
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    h.kit_mut().set_pixels_per_point(1.0);
    h.run();
    h.run_frames(4);
    h
}

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

fn paper_zoom(h: &EditorHarness) -> f32 {
    h.editor().canvas.view_xform().zoom
}

/// A control of the band's.
const BAND: &str = "play (Space)";

fn chrome_gone(h: &EditorHarness) {
    assert!(!h.has("Search"), "the top row");
    assert!(!h.has(BAND), "the band");
    assert!(!h.has("Brush"), "the rail");
}

fn chrome_here(h: &EditorHarness) {
    assert!(h.has("Search"), "the top row");
    assert!(h.has(BAND), "the band");
    assert!(h.has("Brush"), "the rail");
}

/// T and the dot, both ways: the pane grows, the paper refits once and comes
/// back to the same size, the View row answers, and nothing is stored.
#[test]
fn aukeys_hide_tools_hides_and_returns() {
    let mut h = animation();
    h.hide_sheet();
    chrome_here(&h);
    let (pane, zoom) = (h.editor().canvas.dbg_rect, paper_zoom(&h));
    assert!(!h.has(plate::SHOW_TOOLS));
    assert!(h.editor().say.dots_painted, "the place, in dots");
    h.key(Key::T);
    settle(&mut h);
    chrome_gone(&h);
    assert!(!h.editor().say.dots_painted, "gone with the band");
    assert_eq!(h.editor().is_on(Action::HideTools), Some(true));
    let dot = h.rect(plate::SHOW_TOOLS);
    assert_eq!(dot.min, plate::RETURN_DOT_AT, "the corner");
    assert_eq!(dot.size(), vec2(32.0, 32.0), "32 pt");
    let grown = h.editor().canvas.dbg_rect;
    assert!(grown.height() > pane.height() + 44.0 && grown.width() >= pane.width(), "{pane:?} to {grown:?}");
    let refit = paper_zoom(&h);
    assert!(refit > zoom, "the paper took the room: {zoom} to {refit}");
    h.run_frames(6);
    assert_eq!(paper_zoom(&h), refit, "once");
    assert_eq!(h.editor().canvas.dbg_rect, grown);
    h.click(plate::SHOW_TOOLS);
    settle(&mut h);
    chrome_here(&h);
    assert!(!h.has(plate::SHOW_TOOLS));
    assert_eq!(h.editor().canvas.dbg_rect, pane, "the pane it left");
    assert_eq!(paper_zoom(&h), zoom, "the paper it left");
    // T brings them back as well, and a row in View takes them off.
    assert_eq!(h.editor().is_on(Action::HideTools), Some(false));
    assert_eq!(Action::HideTools.menu(), Some("View/Hide tools"));
    assert_eq!(h.perform(Action::HideTools), Performed::Done);
    settle(&mut h);
    chrome_gone(&h);
    h.key(Key::T);
    settle(&mut h);
    chrome_here(&h);
    // Never stored: the config reads the same with the tools off, and a
    // rebuilt editor has its tools.
    let file = serde_json::to_value(&h.rig().config).expect("json");
    h.perform(Action::HideTools);
    settle(&mut h);
    assert_eq!(serde_json::to_value(&h.rig().config).expect("json"), file);
    h.rebuild_editor(crate::doc::AppState::new_project("t", 640, 480, 24, 72.0));
    settle(&mut h);
    assert_eq!(h.editor().is_on(Action::HideTools), Some(false), "a session opens with its tools");
    chrome_here(&h);
}

/// How many of `frames` moved the pane, and the paper, each read on its own.
fn frames_moved(h: &mut EditorHarness, frames: usize, mut step: impl FnMut(&mut EditorHarness)) -> (usize, usize) {
    let (mut pane, mut zoom) = (h.editor().canvas.dbg_rect, paper_zoom(h));
    let mut moved = (0, 0);
    for _ in 0..frames {
        step(h);
        let (now, refit) = (h.editor().canvas.dbg_rect, paper_zoom(h));
        moved.0 += usize::from(now != pane);
        moved.1 += usize::from(refit != zoom);
        (pane, zoom) = (now, refit);
    }
    moved
}

/// A key held past the typematic delay: the pane grows once and the paper
/// refits once, the first time, card's room and all, and every time after, and
/// the way back is one move too.
#[test]
fn aukeys_hide_tools_refits_once_for_a_held_key() {
    let mut h = shipped();
    for time in ["with the card", "without it"] {
        assert_eq!(frames_moved(&mut h, 8, |h| h.hold_key(Key::T)), (1, 1), "{time}");
        assert_eq!(h.editor().is_on(Action::HideTools), Some(true), "{time}");
        h.release_key(Key::T);
        settle(&mut h);
        assert_eq!(frames_moved(&mut h, 8, |h| h.hold_key(Key::T)), (1, 1), "{time}: the way back");
        assert_eq!(h.editor().is_on(Action::HideTools), Some(false), "{time}");
        h.release_key(Key::T);
        settle(&mut h);
    }
}

/// A held key repeats. What rearranges the screen answers the press, and only
/// the press; a frame step is what a repeat is for, and keeps it.
#[test]
fn aukeys_held_key_runs_a_screen_verb_once() {
    fn runs(h: &EditorHarness, verb: Action) -> usize {
        h.performed().iter().filter(|(a, _)| *a == verb).count()
    }
    // One frame with these keys going down, in this order; egui marks which repeat.
    fn frame_of(h: &mut EditorHarness, keys: [Key; 2]) {
        let modifiers = Modifiers::NONE;
        for key in keys {
            let press = egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers };
            h.kit_mut().input_mut().events.push(press);
        }
        h.kit_mut().step();
    }
    let mut h = shipped();
    for _ in 0..6 {
        h.hold_key(Key::T);
    }
    assert_eq!(runs(&h, Action::HideTools), 1, "one press, five repeats");
    frame_of(&mut h, [Key::Q, Key::T]);
    assert_eq!(runs(&h, Action::HideTools), 1, "another key's press is not its own");
    h.release_key(Key::Q);
    h.release_key(Key::T);
    h.key(Key::T);
    assert_eq!(runs(&h, Action::HideTools), 2, "a new press is a new run");
    for (step, key, _) in SIX_STEPS {
        let key = Key::from_name(key).expect("a key");
        for _ in 0..4 {
            h.hold_key(key);
        }
        h.release_key(key);
        assert_eq!(runs(&h, step), 1, "{step:?}");
    }
    let mut h = stepped();
    for frame in 6..9 {
        h.hold_key(Key::ArrowRight);
        assert_eq!(h.state().view.frame, frame, "a repeat steps");
    }

    // A chord of two: the key pressed while the other repeats is a press,
    // whichever comes first in the frame, and two keys that only repeat are not.
    let mut h = shipped();
    let both = Chord { keys: vec!["Q".into(), "W".into()], ctrl: false, shift: false, alt: false };
    h.rig_mut().config.binding_mut(Action::HideTools).chord = Some(both);
    for (turn, keys) in [[Key::Q, Key::W], [Key::W, Key::Q]].into_iter().enumerate() {
        h.hold_key(Key::Q);
        frame_of(&mut h, keys);
        assert_eq!(runs(&h, Action::HideTools), turn + 1, "{keys:?}: the second key is a press");
        frame_of(&mut h, keys);
        assert_eq!(runs(&h, Action::HideTools), turn + 1, "{keys:?}: and then both repeat");
        h.release_key(Key::W);
        h.release_key(Key::Q);
    }

    // Bare Space answers its release, a frame with no press in it at all.
    let mut h = shipped();
    h.rig_mut().config.binding_mut(Action::HideTools).chord = Some(chord("Space"));
    h.key(Key::Space);
    assert_eq!(runs(&h, Action::HideTools), 1, "a tap is a press");
}

/// Hide tools takes the chrome off and leaves the dock: it draws on every
/// frame, so a step's recipe swapped in under it has rects by its next frame,
/// and nothing saved from it was never drawn.
#[test]
fn aukeys_hide_tools_still_draws_the_dock() {
    let mut h = animation();
    h.key(Key::T);
    settle(&mut h);
    for (step, _, stage) in SIX_STEPS {
        let recipe = serde_json::to_value(stage.dock()).expect("json");
        assert!(null_rects(&recipe) > 0, "{step:?}: a recipe has no rects till it draws");
        h.perform(step);
        settle(&mut h);
        // Plan's recipe stands on `Stage::Drawing`.
        let stage = if stage == Stage::Layout { Stage::Drawing } else { stage };
        assert_eq!(h.editor().stage, Some(stage), "{step:?}");
        assert!(h.has(plate::SHOW_TOOLS), "{step:?}: still hidden");
        let live = serde_json::to_value(&h.editor().dock).expect("json");
        assert_eq!(null_rects(&live), 0, "{step:?}: drawn");
        assert!(!plate::pane_bodies(&h.ctx()).is_empty(), "{step:?}");
    }
    let dock = h.editor().dock.clone();
    let list = vec![Workspace { name: "hidden".into(), dock, preset: None, view: None }];
    let dir = tempfile::tempdir().expect("a data dir");
    Workspaces { list }.save_to(dir.path()).expect("saved");
    let text = std::fs::read_to_string(Workspaces::path_in(dir.path())).expect("the file");
    assert_eq!(null_rects(&serde_json::from_str(&text).expect("json")), 0);
    let (back, notes) = Workspaces::load_from(dir.path());
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(back.list.len(), 1, "it reads back");
}

/// A closed strip's room is the paper's with the rest: Fit runs to the whole
/// pane, not to the rail's reach, and the latch stands for the way back.
#[test]
fn aukeys_hide_tools_fit_takes_the_whole_pane() {
    let mut h = EditorHarness::with_state_shipped(crate::doc::AppState::new_project("t", 1920, 1080, 24, 72.0));
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    h.kit_mut().set_pixels_per_point(1.0);
    settle(&mut h);
    h.hide_sheet();
    h.editor_mut().strip_open = false;
    settle(&mut h);
    assert!(h.editor().strip_closed(), "the latch holds it");
    let closed = paper_zoom(&h);
    h.perform(Action::HideTools);
    settle(&mut h);
    let pane = h.editor().canvas.dbg_rect;
    let whole = (pane.width() / 1920.0).min(pane.height() / 1080.0) * 0.94;
    assert!((paper_zoom(&h) - whole).abs() < 1e-4, "{} of {whole} in {pane:?}", paper_zoom(&h));
    h.perform(Action::HideTools);
    settle(&mut h);
    assert!(h.editor().strip_closed(), "the latch stands");
    assert_eq!(paper_zoom(&h), closed, "the paper it left");
}

/// In every step, and the floats go with the rest and come back as they stood.
#[test]
fn aukeys_hide_tools_works_in_any_step() {
    let mut h = shipped();
    for (step, _, _) in SIX_STEPS {
        h.perform(step);
        settle(&mut h);
        h.perform(Action::HideTools);
        settle(&mut h);
        assert!(h.has(plate::SHOW_TOOLS), "{step:?}");
        assert!(!h.has("Search"), "{step:?}");
        assert!(!h.has(BAND), "{step:?}");
        h.perform(Action::HideTools);
        settle(&mut h);
    }
    let close = format!("Close {}", crate::vocab::CANVAS_OPTIONS);
    let mut h = shipped();
    h.perform(Action::CanvasOptions);
    settle(&mut h);
    assert!(h.editor().floats.is_open(crate::floats::Float::CanvasOptions));
    assert!(h.has(&close), "the float");
    h.key(Key::T);
    settle(&mut h);
    assert!(!h.has(&close), "off the screen");
    assert!(h.editor().floats.is_open(crate::floats::Float::CanvasOptions), "kept");
    h.key(Key::T);
    settle(&mut h);
    assert!(h.has(&close), "back");
}

/// What the dot would cut: every word the last frame painted and every control
/// the tree names that reach under it. A pane it merely stands on holds it
/// whole, and is no finding.
fn cut_by_the_dot(h: &EditorHarness, dot: Rect) -> Vec<String> {
    fn words(s: &egui::Shape, dot: Rect, out: &mut Vec<String>) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|s| words(s, dot, out)),
            egui::Shape::Text(t) if t.visual_bounding_rect().intersects(dot) => out.push(t.galley.text().to_string()),
            _ => {}
        }
    }
    fn names(n: egui_kittest::Node<'_>, dot: Rect, out: &mut Vec<String>) {
        let ak = n.accesskit_node();
        if let (Some(name), Some(_)) = (ak.label(), ak.bounding_box()) {
            let at = n.rect();
            if name != plate::SHOW_TOOLS && at.intersects(dot) && !at.contains_rect(dot) {
                out.push(name.to_string());
            }
        }
        n.children().for_each(|c| names(c, dot, out));
    }
    let mut out = Vec::new();
    h.kit().output().shapes.iter().for_each(|c| words(&c.shape, dot, &mut out));
    names(h.kit().root(), dot, &mut out);
    out
}

/// The dot stands where the spec puts it, and on nothing: in every step, at
/// both window sizes, no tab, control or word reaches under it.
#[test]
fn aukeys_hide_tools_dot_cuts_nothing() {
    let dot = Rect::from_min_size(plate::RETURN_DOT_AT, Vec2::splat(plate::PILL_H));
    for size in [vec2(1600.0, 900.0), vec2(1280.0, 800.0)] {
        let mut h = shipped();
        h.kit_mut().set_size(size);
        for (step, _, _) in SIX_STEPS {
            h.perform(step);
            settle(&mut h);
            h.perform(Action::HideTools);
            settle(&mut h);
            assert_eq!(h.rect(plate::SHOW_TOOLS), dot, "{step:?}");
            assert_eq!(cut_by_the_dot(&h, dot), Vec::<String>::new(), "{step:?} at {size:?}");
            h.perform(Action::HideTools);
            settle(&mut h);
        }
    }
}

/// `play_tick` runs before the panels: the frames go on with the band off.
#[test]
fn aukeys_hide_tools_keeps_playback_running() {
    let mut h = stepped();
    h.key(Key::Space);
    assert!(h.state().view.playing);
    h.key(Key::T);
    settle(&mut h);
    assert!(h.has(plate::SHOW_TOOLS));
    let frame = h.state().view.frame;
    h.wait(1.0);
    assert!(h.state().view.playing, "still running");
    assert_ne!(h.state().view.frame, frame, "the frames moved");
    h.key(Key::T);
    settle(&mut h);
    assert!(h.state().view.playing, "and it never stopped");
}

/// Mid-stroke T says why, and the screen stands. The step keys wait for the
/// pen the same way: a dock is never swapped under a stroke.
#[test]
fn aukeys_hide_tools_refused_mid_stroke() {
    let mut h = shipped();
    let at = h.editor().canvas.dbg_rect.center();
    h.press_and_move(at, at + vec2(30.0, 10.0), 4);
    assert!(h.editor().canvas.stroke_active(), "a live stroke");
    assert_eq!(h.editor().can_perform(Action::HideTools), Err("finish the stroke first"));
    let dock = h.editor().canvas.dbg_rect;
    h.key(Key::T);
    assert_eq!(h.state().refusal, "refused — finish the stroke first");
    assert_eq!(h.editor().is_on(Action::HideTools), Some(false));
    assert!(h.has("Search"), "the screen stands");
    assert_eq!(h.editor().canvas.dbg_rect, dock, "no pane regrown under the pen");
    let stage = h.editor().stage;
    for (_, key, _) in SIX_STEPS {
        h.key(Key::from_name(key).expect("a key"));
        assert_eq!(h.editor().stage, stage, "{key} waits for the pen");
    }
    h.release(at + vec2(30.0, 10.0));
    settle(&mut h);
    assert!(!h.editor().canvas.stroke_active());
    h.key(Key::T);
    settle(&mut h);
    assert_eq!(h.editor().is_on(Action::HideTools), Some(true), "and after it, it runs");
    // And on the way back too, with the only words left on the dot's card.
    let at = h.editor().canvas.dbg_rect.center();
    h.press_and_move(at, at + vec2(30.0, 10.0), 4);
    h.key(Key::T);
    assert_eq!(h.editor().is_on(Action::HideTools), Some(true));
    settle_painted(&mut h, "finish the stroke first");
    // Nor does what wants the chrome bring it back under the pen: two of the
    // four are refused, which leaves nothing owed at pen-up.
    let dock = h.editor().canvas.dbg_rect;
    for verb in [Action::GoToFrame, Action::WelcomeScreen] {
        assert_eq!(h.editor().can_perform(verb), Err("finish the stroke first"), "{verb:?}");
        assert_eq!(h.perform(verb), Performed::Ignored, "{verb:?}");
        assert_eq!(h.editor().is_on(Action::HideTools), Some(true), "{verb:?}");
        assert_eq!(h.editor().canvas.dbg_rect, dock, "{verb:?}: no pane regrown under the pen");
    }
    h.release(at + vec2(30.0, 10.0));
    settle(&mut h);
    assert_eq!(h.editor().is_on(Action::HideTools), Some(true), "nothing owed");

    // The other two wait for pen-up (spec 4.5), and the tools wait with them.
    type Opened = fn(&EditorHarness) -> bool;
    let opened: [(Action, Opened); 2] = [
        (Action::CanvasOptions, |h| h.editor().floats.is_open(Float::CanvasOptions)),
        (Action::WhatsNew, |h| h.editor().start.is_some()),
    ];
    for (verb, open) in opened {
        let mut h = shipped();
        h.perform(Action::HideTools);
        settle(&mut h);
        let at = h.editor().canvas.dbg_rect.center();
        h.press_and_move(at, at + vec2(30.0, 10.0), 4);
        let dock = h.editor().canvas.dbg_rect;
        assert_eq!(h.perform(verb), Performed::Done, "{verb:?}");
        h.run_frames(3);
        assert!(!open(&h), "{verb:?} waits for the pen");
        assert_eq!(h.editor().is_on(Action::HideTools), Some(true), "{verb:?}");
        assert_eq!(h.editor().canvas.dbg_rect, dock, "{verb:?}: no pane regrown under the pen");
        h.release(at + vec2(30.0, 10.0));
        settle(&mut h);
        assert!(open(&h), "{verb:?} at pen-up");
        assert_eq!(h.editor().is_on(Action::HideTools), Some(false), "{verb:?}: and what it opened shows");
        assert!(h.has("Search"), "{verb:?}");
        h.perform(Action::HideTools);
        settle(&mut h);
        assert_eq!(h.editor().is_on(Action::HideTools), Some(true), "{verb:?}: owed once, not for good");
    }
}

/// One frame more until `words` paint, and a look at them.
#[track_caller]
fn settle_painted(h: &mut EditorHarness, words: &str) {
    h.run_frames(2);
    let text = h.painted_text();
    assert!(text.iter().any(|t| t.to_lowercase().contains(words)), "{words:?} in {text:?}");
}

const ROW: &str = "Tap the dot, or press T, to bring the tools back.";

/// Spec 8.2's row: on the dot's card, once, naming the live binding, and with
/// no pointer on anything. Its card takes no press.
#[test]
fn aukeys_hide_tools_card_teaches_once() {
    let mut h = shipped();
    h.key(Key::T);
    settle(&mut h);
    assert_eq!(h.editor().say.hint.as_ref().and_then(|s| s.bit), Some(say::HIDE_TOOLS));
    assert!(h.painted_text().iter().any(|t| t == ROW), "{:?}", h.painted_text());
    assert_eq!(h.rig().config.ui.hints_seen & say::HIDE_TOOLS, 0, "still due");
    h.wait(6.0);
    assert!(h.painted_text().iter().any(|t| t == ROW), "no timer takes it off");
    h.click(plate::SHOW_TOOLS);
    settle(&mut h);
    assert_ne!(h.rig().config.ui.hints_seen & say::HIDE_TOOLS, 0, "answered");
    assert!(!h.painted_text().iter().any(|t| t == ROW), "gone, and not onto the lane");
    h.key(Key::T);
    settle(&mut h);
    assert!(!h.painted_text().iter().any(|t| t == ROW), "once");

    // It names the binding the artist has.
    let mut h = shipped();
    let mine = chord("Ctrl+Shift+J");
    h.rig_mut().config.binding_mut(Action::HideTools).chord = Some(mine);
    h.run();
    h.perform(Action::HideTools);
    settle(&mut h);
    let said = "Tap the dot, or press Ctrl+Shift+J, to bring the tools back.";
    assert!(h.painted_text().iter().any(|t| t == said), "{:?}", h.painted_text());

    // No chord, no sentence (spec 6.5): the dot still answers.
    let mut h = shipped();
    h.rig_mut().config.binding_mut(Action::HideTools).chord = None;
    h.run();
    h.perform(Action::HideTools);
    settle(&mut h);
    assert!(!h.painted_text().iter().any(|t| t.contains("Tap the dot")));
    assert_eq!(h.editor().canvas.dbg_rect.top(), 0.0, "and no band kept for a card that will not stand");
    h.click(plate::SHOW_TOOLS);
    settle(&mut h);
    assert!(h.has("Search"));

    // Hints off: none. The upgrader's mask: none.
    let mut h = shipped();
    h.perform(Action::ToggleHints);
    settle(&mut h);
    h.perform(Action::HideTools);
    settle(&mut h);
    assert!(!h.painted_text().iter().any(|t| t == ROW), "hints off");
    let mut h = EditorHarness::with_dir_shipped(upgraders_dir());
    h.perform(Action::HideTools);
    settle(&mut h);
    assert!(h.has(plate::SHOW_TOOLS));
    assert!(!h.painted_text().iter().any(|t| t == ROW), "an upgrader's mask");

    // It never reaches the lane, not on the frame the band comes back, by
    // the dot's verb or by one that wants the chrome.
    for back in [Action::HideTools, Action::GoToFrame] {
        let mut h = shipped();
        h.perform(Action::HideTools);
        settle(&mut h);
        assert!(h.painted_text().iter().any(|t| t == ROW), "{back:?}");
        h.perform(back);
        assert!(h.has(BAND), "{back:?}");
        assert!(!h.painted_text().iter().any(|t| t == ROW), "{back:?}");
    }
}

/// The lane's clock runs only while the lane stands: a row that went with the
/// band starts its reading again (`Say::stands`).
#[test]
fn aukeys_hide_tools_stops_the_read_clock() {
    let mut h = shipped();
    h.wait(4.2);
    let row = say::Spoken { bit: Some(say::HELD), words: "A row that passes.".into(), verb: None };
    h.editor_mut().say.pin = Some(row);
    settle_painted(&mut h, "a row that passes.");
    h.perform(Action::HideTools);
    h.wait(say::READ_SECS + 1.0);
    h.perform(Action::HideTools);
    h.run_frames(2);
    assert_eq!(h.rig().config.ui.hints_seen & say::HELD, 0, "the time away was no reading");
    h.wait(say::READ_SECS + 1.0);
    assert_ne!(h.rig().config.ui.hints_seen & say::HELD, 0, "the time back was");
}

/// Where the card on the dot stands.
fn dot_card(h: &EditorHarness) -> Rect {
    h.ctx().memory(|m| m.area_rect(plate::dot_card_id())).expect("a card on the dot")
}

/// The card stands off the dot, and the pen draws through it, here above the
/// pane and, for a refusal once the row is answered, over it.
#[test]
fn aukeys_hide_tools_card_stops_no_pen() {
    let mut h = shipped();
    h.key(Key::T);
    settle(&mut h);
    let dot = h.rect(plate::SHOW_TOOLS);
    let words = h.painted_at(ROW);
    assert!(words.x >= dot.right() + 8.0, "clear of the dot: {words:?} {dot:?}");
    let canvas = h.editor().canvas.dbg_rect;
    let host = h.ctx().layer_id_at(canvas.center()).expect("the canvas's layer");
    assert_eq!(h.ctx().layer_id_at(words), Some(host), "the pen sees through it");
    assert_ne!(h.ctx().layer_id_at(dot.center()), Some(host), "the dot is no hole in the pane");

    h.key(Key::T);
    settle(&mut h);
    h.key(Key::T);
    settle(&mut h);
    h.state_mut().refuse("refused — a test's reason");
    h.run_frames(3);
    let words = h.painted_at("refused — a test's reason");
    let canvas = h.editor().canvas.dbg_rect;
    assert!(canvas.contains(words), "no room left for it: {words:?} in {canvas:?}");
    let host = h.ctx().layer_id_at(canvas.center()).expect("the canvas's layer");
    assert_eq!(h.ctx().layer_id_at(words), Some(host), "the pen sees paper through it");
}

/// Spec 8.5, nothing over art. Until something answers it, the card has a band
/// above the pane: at Fit the paper starts under it at every size, and no pan
/// or zoom can bring art beneath it. The first stroke answers it on either
/// road, and takes no room back, so the paper does not refit under the pen.
#[test]
fn aukeys_hide_tools_card_stands_on_no_art() {
    let roads: [fn(&mut EditorHarness); 2] = [line, raster_line];
    for size in [vec2(1600.0, 900.0), vec2(1720.0, 960.0), vec2(1280.0, 800.0)] {
        for road in roads {
            let mut h = shipped();
            h.kit_mut().set_size(size);
            settle(&mut h);
            let open = h.editor().canvas.dbg_rect;
            h.key(Key::T);
            settle(&mut h);
            assert!(h.painted_text().iter().any(|t| t == ROW), "{size:?}: {:?}", h.painted_text());
            let (card, canvas, zoom) = (dot_card(&h), h.editor().canvas.dbg_rect, paper_zoom(&h));
            let xf = h.editor().canvas.view_xform();
            assert!(!card.intersects(xf.paper_bounds()), "{card:?} on {:?} at {size:?}", xf.paper_bounds());
            assert!(card.bottom() <= canvas.top(), "{card:?} above {canvas:?} at {size:?}");
            assert!(h.filled_rects(plate::DESK).iter().any(|r| r.contains_rect(card)), "the band is desk");
            let fit = (canvas.width() / xf.paper.x).min(canvas.height() / xf.paper.y) * 0.94;
            assert!((zoom - fit).abs() < 1e-4, "at Fit: {zoom} of {fit} in {canvas:?}");
            assert!(canvas.height() > open.height(), "and the pane still grew: {open:?} to {canvas:?}");

            road(&mut h);
            assert!(!h.painted_text().iter().any(|t| t == ROW), "{size:?}: a stroke answers it");
            assert_ne!(h.rig().config.ui.hints_seen & say::HIDE_TOOLS, 0, "{size:?}: and it stays answered");
            assert_eq!(h.editor().canvas.dbg_rect, canvas, "{size:?}: the room stays while the tools are off");
            assert_eq!(paper_zoom(&h), zoom, "{size:?}: no refit under the pen");

            h.key(Key::T);
            settle(&mut h);
            h.key(Key::T);
            settle(&mut h);
            assert!(!h.painted_text().iter().any(|t| t == ROW), "{size:?}: once");
            let whole = h.editor().canvas.dbg_rect;
            assert_eq!(whole.top(), canvas.top() - f32::from(plate::RETURN_DOT_ROOM), "{size:?}: the room has gone");
        }
    }

    // A stroke with the tools showing answers nothing: the card has not stood.
    let mut h = shipped();
    line(&mut h);
    raster_line(&mut h);
    assert_eq!(h.rig().config.ui.hints_seen & say::HIDE_TOOLS, 0, "{:?}", h.rig().config.ui.hints_seen);
    h.key(Key::T);
    settle(&mut h);
    assert!(h.painted_text().iter().any(|t| t == ROW), "still to teach");

    // Hints switched on with the tools off: no room was left, so no card.
    let mut h = shipped();
    h.perform(Action::ToggleHints);
    settle(&mut h);
    h.perform(Action::HideTools);
    settle(&mut h);
    let pane = h.editor().canvas.dbg_rect;
    h.perform(Action::ToggleHints);
    settle(&mut h);
    assert!(!h.painted_text().iter().any(|t| t == ROW), "it waits for the room");
    assert_eq!(h.editor().canvas.dbg_rect, pane, "and none opens under the pen");

    // In every step: what a dock shows starts below the card.
    for (step, _, _) in SIX_STEPS {
        let mut h = shipped();
        h.perform(step);
        settle(&mut h);
        h.perform(Action::HideTools);
        settle(&mut h);
        assert!(h.painted_text().iter().any(|t| t == ROW), "{step:?}");
        let cut = cut_by_the_dot(&h, dot_card(&h));
        assert_eq!(cut.iter().filter(|w| *w != ROW).count(), 0, "{step:?}: {cut:?}");
    }
}

/// A refusal under Hide tools still says why, on the dot's card, for as long
/// as the lane would have held it, and it does not stand behind the dot's row.
/// It is one red card, not a pill in a card, level with the dot as the row is.
#[test]
fn aukeys_hide_tools_refusals_have_a_voice() {
    let mut h = shipped();
    h.key(Key::T);
    settle(&mut h);
    let dot = h.rect(plate::SHOW_TOOLS);
    assert!((dot_card(&h).center().y - dot.center().y).abs() < 1.0, "{:?} by {dot:?}", dot_card(&h));
    h.state_mut().refuse("refused — a test's reason");
    h.run_frames(3);
    assert!(h.painted_text().iter().any(|t| t == "refused — a test's reason"), "{:?}", h.painted_text());
    assert!(!h.painted_text().iter().any(|t| t == ROW), "one voice");
    assert!((dot_card(&h).center().y - dot.center().y).abs() < 1.0, "{:?} by {dot:?}", dot_card(&h));
    let wash = h.filled_rects(plate::RED_WASH);
    assert_eq!(wash.len(), 1, "one outline: {wash:?}");
    assert!((wash[0].height() - dot_card(&h).height()).abs() < 1.0, "the card is the wash: {wash:?}");
    assert!(measure::clip_here(&h, measure::Scope::Chrome).clipped == 0, "the refusal fits");
    // Two lines are level too, and a long reason stands inside the window.
    let long = "refused — a reason long enough to run past one line of a card, however wide the card may stand on \
                this window, and still have words left for the second line of it";
    h.state_mut().refuse(long);
    h.run_frames(3);
    let card = dot_card(&h);
    assert!((card.center().y - dot.center().y).abs() < 1.0, "{card:?} by {dot:?}");
    assert!(card.height() > dot.height(), "two lines: {card:?}");
    assert!(card.top() >= 0.0, "{card:?}");
    h.wait(4.2);
    settle(&mut h);
    assert!(!h.painted_text().iter().any(|t| t == long), "its dwell ended");
    assert!(h.painted_text().iter().any(|t| t == ROW), "the row stood by");
    assert!(measure::clip_here(&h, measure::Scope::Chrome).clipped == 0, "the card fits");
}

/// AU19: Plan has no preview to refuse a stroke: with the lane gone a stroke
/// in it simply lands, and the dot's card has nothing to answer.
#[test]
fn aukeys_hide_tools_draws_in_plan() {
    let mut h = EditorHarness::as_animation();
    settle(&mut h);
    h.key(Key::T);
    settle(&mut h);
    let canvas = h.editor().pane_rects.iter().find(|(p, ..)| *p == Pane::Canvas);
    let at = canvas.map(|(_, r, _)| r.center()).expect("the canvas");
    let before = h.state().strokes_done;
    for done in 1..=2 {
        h.drag_path(&[at, at + vec2(30.0, 0.0), at + vec2(60.0, 10.0)]);
        settle(&mut h);
        assert_eq!(h.state().strokes_done, before + done, "it lands");
        assert!(h.state().refusal.is_empty(), "{}", h.state().refusal);
    }
}

/// A key takes a Layers rail away mid-drag. Its release will land nowhere, so
/// nothing waits for one and nothing is committed: the tools come back on the
/// document's own value. The same goes for a step that takes the docked pane.
#[test]
fn aukeys_hide_tools_drops_an_opacity_drag() {
    fn layered() -> EditorHarness {
        let mut state = crate::doc::AppState::new_project("t", 640, 480, 24, 72.0);
        state.new_drawing_at_frame();
        let mut h = EditorHarness::with_state_shipped(state);
        h.kit_mut().set_size(vec2(1600.0, 900.0));
        settle(&mut h);
        h
    }
    /// A drag begun on the row's rail: the percentage stands just past it.
    fn drag(h: &mut EditorHarness) -> egui::Pos2 {
        let at = h.painted_at("100%") + vec2(-24.0, 8.0);
        let to = at - vec2(20.0, 0.0);
        h.press_and_move(at, to, 4);
        assert!(h.state().strip_opacity.is_some(), "a drag in hand");
        h.run_frames(2);
        assert!(h.state().strip_opacity.is_some(), "a held pointer holds it");
        to
    }
    let mut h = layered();
    h.open_float(Float::Layers);
    let to = drag(&mut h);
    h.key(Key::T);
    assert!(h.has(plate::SHOW_TOOLS));
    assert_eq!(h.state().strip_opacity, None, "gone with its rail");
    h.release(to);
    h.key(Key::T);
    settle(&mut h);
    assert_eq!(h.state().strip_opacity, None);
    assert_eq!(h.state().active_layer_props().map(|p| p.opacity), Some(1.0), "nothing committed");
    assert!(h.painted_text().iter().any(|t| t == "100%"), "{:?}", h.painted_text());

    let mut h = layered();
    h.editor_mut().open_pane(Pane::Layers);
    settle(&mut h);
    let to = drag(&mut h);
    h.key(Key::from_name(SIX_STEPS[1].1).expect("a key"));
    assert_eq!(visible_panes(&h.editor().dock), [Pane::XSheet, Pane::Canvas], "Plan's recipe, and no rail");
    assert_eq!(h.state().strip_opacity, None, "gone with its rail");
    h.release(to);
    settle(&mut h);
    assert_eq!(h.state().strip_opacity, None);
    assert_eq!(h.state().active_layer_props().map(|p| p.opacity), Some(1.0), "nothing committed");
}

/// What the last frame painted on `key`: its masks by name, and its dots.
fn icon_on(h: &EditorHarness, key: Rect) -> (Vec<String>, usize) {
    fn walk(s: &egui::Shape, key: Rect, masks: &mut Vec<egui::TextureId>, dots: &mut usize) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, key, masks, dots)),
            egui::Shape::Mesh(m) if m.texture_id != egui::TextureId::default() && key.contains_rect(m.calc_bounds()) => {
                masks.push(m.texture_id);
            }
            egui::Shape::Circle(c) if key.contains(c.center) => *dots += 1,
            _ => {}
        }
    }
    let (mut masks, mut dots) = (Vec::new(), 0);
    h.kit().output().shapes.iter().for_each(|c| walk(&c.shape, key, &mut masks, &mut dots));
    let textures = h.ctx().tex_manager();
    let textures = textures.read();
    (masks.iter().filter_map(|t| textures.meta(*t)).map(|m| m.name.clone()).collect(), dots)
}

/// Spec 2.7: the dot has no word on it, so its shape is all it has, and that
/// shape is its own: the hide mask, not the three dots of More tools.
#[test]
fn aukeys_hide_tools_dot_wears_its_own_icon() {
    let mut h = shipped();
    h.key(Key::T);
    settle(&mut h);
    assert_eq!(icon_on(&h, h.rect(plate::SHOW_TOOLS)), (vec!["icon:hide:24".to_string()], 0));
    let mut h = shipped();
    h.kit_mut().set_size(vec2(1280.0, 480.0));
    settle(&mut h);
    assert_eq!(icon_on(&h, h.rect("More tools")), (Vec::new(), 3), "the dots stay More tools'");
}

/// The chords that moved, joined, on every window from the Lead's largest to
/// the smallest the app opens: the sentence stands whole, on the refusal's
/// lane, and waits for the artist as any notice does.
#[test]
fn aukeys_chords_that_moved_fit_the_lane() {
    for size in [vec2(1600.0, 900.0), vec2(1280.0, 800.0), vec2(1280.0, 720.0), vec2(1024.0, 640.0)] {
        let mut h = EditorHarness::with_dir_shipped(upgraders_dir());
        h.kit_mut().set_size(size);
        h.kit_mut().set_pixels_per_point(1.0);
        settle(&mut h);
        h.wait(6.0);
        assert_eq!(h.state().refusal, KEYS_MOVED, "{size:?}");
        assert!(h.painted_text().iter().any(|t| t == KEYS_MOVED), "{size:?}: {:?}", h.painted_text());
        let clip = measure::clip_here(&h, measure::Scope::Chrome);
        assert_eq!((clip.clipped, clip.elided), (0, 0), "{size:?}: {:?}", clip.found);
    }
}

/// The Start float goes with the tools and comes back with them, and what opens
/// in the chrome brings the chrome back.
#[test]
fn aukeys_hide_tools_gives_way_to_what_needs_the_chrome() {
    let mut h = EditorHarness::as_first_run();
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    settle(&mut h);
    assert!(h.editor().start.is_some(), "the float");
    let heading = crate::welcome::HEADING;
    assert!(h.painted_text().iter().any(|t| t == heading));
    h.key(Key::T);
    settle(&mut h);
    assert!(!h.painted_text().iter().any(|t| t == heading), "off the screen");
    assert!(h.editor().start.is_some(), "kept");
    h.key(Key::T);
    settle(&mut h);
    assert!(h.painted_text().iter().any(|t| t == heading), "back");

    for verb in [Action::GoToFrame, Action::CanvasOptions, Action::WelcomeScreen, Action::WhatsNew] {
        let mut h = shipped();
        h.perform(Action::HideTools);
        settle(&mut h);
        h.perform(verb);
        settle(&mut h);
        assert!(h.has("Search"), "{verb:?} needs the chrome");
        assert_eq!(h.editor().is_on(Action::HideTools), Some(false), "{verb:?}");
    }
}

/// J5: a whole edit without the mouse. Six steps, four arrows and the example
/// edit: five presses, no clicks.
#[test]
fn aukeys_keyboard_journey() {
    let mut state = crate::doc::AppState::new_project("t", 640, 480, 24, 72.0);
    state.set_cut_length(24);
    for key in [0, 6, 12] {
        state.goto(key);
        state.new_drawing_at_frame();
    }
    state.goto(0);
    let mut h = EditorHarness::with_state_shipped(state);
    h.kit_mut().set_size(vec2(1600.0, 900.0));
    settle(&mut h);
    let mut steps = Recorder::new(&mut h, "every step by key");
    for (_, key, _) in SIX_STEPS {
        steps.act(key, Act::Key(Key::from_name(key).expect("a key")));
    }
    assert_eq!(steps.tally().keys, 6, "steps 6 of 6");
    assert_eq!(steps.tally().clicks, 0);
    let h = steps.harness();
    assert_eq!(h.editor().stage, Some(Stage::Export));

    let mut arrows = Recorder::new(h, "arrows");
    arrows.act("to Draw", Act::Key(Key::F5));
    for key in [Key::ArrowRight, Key::ArrowDown, Key::ArrowLeft, Key::ArrowUp] {
        arrows.act("step", Act::Key(key));
    }
    assert_eq!(arrows.tally().keys, 5, "arrows 4 of 4, and the way there");
    let h = arrows.harness();
    assert_eq!(h.state().view.frame, 0, "there and back");

    // The example edit, as J5 spells it: W, =, =, Alt+2, Space.
    h.rig_mut().performed.clear();
    let mut edit = Recorder::new(h, "example edit");
    edit.act("next key", Act::Key(Key::W));
    edit.act("hold", Act::Key(Key::Equals));
    edit.act("hold", Act::Key(Key::Equals));
    edit.act("on twos", Act::Chord(Modifiers::ALT, Key::Num2));
    edit.act("play", Act::Key(Key::Space));
    let tally = edit.tally();
    assert_eq!(tally.keys, 5, "5 presses");
    assert_eq!(tally.clicks, 0, "0 clicks");
    let h = edit.harness();
    let ran: Vec<Action> = h.rig().performed.iter().map(|(a, _)| *a).collect();
    assert_eq!(ran, [Action::NextKey, Action::HoldPlus, Action::HoldPlus, Action::RetimeTwos, Action::PlayPause]);
    assert!(h.state().view.playing, "and it plays");
}

/// Keys belong to the column under the sheet's pane: Book and BG answer there,
/// by their new keys, and nowhere else.
#[test]
fn aukeys_book_and_bg_answer_over_the_sheet_only() {
    let mut h = animation();
    h.perform(Action::RoomEdit);
    settle(&mut h);
    assert!(visible_panes(&h.editor().dock).contains(&Pane::XSheet));
    let sheet = h.editor().pane_rects.iter().find(|(p, ..)| *p == Pane::XSheet).expect("the sheet").1;
    h.point_at(sheet.center());
    h.run();
    let ctx = h.ctx();
    assert_eq!(h.editor_mut().shortcut_ctx(&ctx), Ctx::Sheet);
    h.key_mods(Modifiers::ALT, Key::O);
    assert!(h.rig().performed.iter().any(|(a, _)| *a == Action::ColumnKindBook), "Alt+O over the sheet");
    h.key_mods(Modifiers::ALT, Key::W);
    assert!(h.rig().performed.iter().any(|(a, _)| *a == Action::ColumnKindBg), "Alt+W over the sheet");

    let mut h = shipped();
    let at = h.editor().canvas.dbg_rect.center();
    h.point_at(at);
    h.run();
    h.key_mods(Modifiers::ALT, Key::O);
    h.key_mods(Modifiers::ALT, Key::W);
    assert!(h.rig().performed.is_empty(), "over the canvas: {:?}", h.rig().performed);
}

/// A focused widget takes the arrows and nothing else: a chord still fires.
#[test]
fn aukeys_focus_takes_only_the_arrows() {
    let mut h = stepped();
    h.key(Key::Tab);
    h.run_frames(2);
    assert!(h.ctx().memory(|m| m.focused()).is_some());
    h.key_mods(CTRL, Key::K);
    assert!(h.editor().palette.open, "a chord that is not an arrow still fires");
}

/// A row bound to an arrow by hand is the widget's too: the rule is the key's.
#[test]
fn aukeys_chord_knows_an_arrow() {
    assert!(chord("Left").has_arrow());
    assert!(chord("Ctrl+Down").has_arrow());
    assert!(!chord("F").has_arrow());
    assert!(!chord("Ctrl+Comma").has_arrow());
}

/// AU27, spec 1.11: `T` is refused while a lesson stands, in words that name
/// the way out, and works again after Leave.
#[test]
fn aukeys_hide_tools_waits_for_a_lesson() {
    let mut h = crate::testkit::EditorHarness::as_shipped();
    h.editor_mut().begin_lesson(crate::lessons::PROBED, 0);
    h.run();
    assert_eq!(h.editor().can_perform(Action::HideTools), Err("Leave the lesson to hide the tools."));
    h.perform(Action::HideTools);
    h.run();
    assert!(!h.editor().tools_hidden);
    h.click(crate::lesson::LEAVE);
    h.run();
    h.perform(Action::HideTools);
    h.run();
    assert!(h.editor().tools_hidden);
}
