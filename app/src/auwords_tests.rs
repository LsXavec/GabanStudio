//! AU12: plain words at reading size. Every label says section 3's word, the
//! old word still finds its command in Search, and nothing stored moves.

use std::collections::BTreeSet;

use anim_core::board::CutStatus;
use anim_core::fx::FxKind;
use anim_core::model::{DrawingKind, LayerRole};
use anim_core::xsheet::ColumnKind;
use eframe::egui;

use crate::config::{self, Action};
use crate::measure::{self, Tier};
use crate::testkit::EditorHarness;
use crate::workspace::{Pane, Stage};
use crate::{cmdpal, plate, stage_key, steps, vocab};

/// Spec section 3, the words as it writes them. `vocab::TABLE` is the code's
/// copy; this is the spec's, so neither can drift alone.
const SECTION_3: &[(&str, &str)] = &[
    ("room", "step"),
    ("network room", "shared session"),
    ("workspace", "arrangement"),
    ("layout", "Plan"),
    ("genga (drawing)", "Draw"),
    ("shiage (ink & paint)", "Color"),
    ("satsuei (composite)", "Effects"),
    ("henshū (edit)", "Timing"),
    ("manga (pages)", "Manga"),
    ("Brush Forge", "Brush maker"),
    ("cut", "shot"),
    ("cel", "drawing"),
    ("column", "track"),
    ("X-Sheet", "Timing sheet"),
    ("onion", "Ghost frames"),
    ("ATARI", "Sketch pencil"),
    ("GENGA", "Clean pencil"),
    ("SHUSEI", "Correction pencil"),
    ("lasso fill", "Draw to fill"),
    ("alpha lock", "Keep inside strokes"),
    ("comp", "Preview final"),
    ("HARDNESS", "Edge softness"),
    ("aliased", "Pixel edges"),
    ("STABILIZER", "Steady stroke"),
    ("COLUMNS", "Tracks"),
    ("SHAKU", "Length"),
    ("MEMO", "Notes"),
    ("library", "Drawings"),
    ("Cel Layers", "Layers"),
    ("Presets", "Brushes"),
    ("stack", "Layer set"),
    ("ARMED", "Drawing with"),
    ("Render Queue", "Export queue"),
    ("Node Graph", "Effects graph"),
    ("Build satsuei graph", "Full finishing setup"),
    ("Build composite from sheet", "Rebuild from layers"),
    ("Viewer", "Result"),
    ("Curves", "Motion curves"),
    ("EDIT MODEL", "Edit colors"),
    ("role", "Color name"),
    ("line guard", "Protect line art"),
    ("LIFT KEY", "Remove from timeline"),
    ("Shinkō board", "Shots"),
    ("Henshū timeline", "Episode timeline"),
    ("bind", "Animate"),
    ("T.U.", "Zoom in"),
    ("T.B.", "Zoom out"),
    ("hold +", "Longer"),
    ("hold −", "Shorter"),
    ("Solo merged", "One layer"),
    ("Layout (LO)", "Rough over reference"),
    ("Genga stack", "Clean line over rough"),
    ("Shiage (full)", "Full paint"),
    ("H", "Hide track"),
    ("L", "Lock track"),
    ("Next key", "Next drawing"),
    ("Previous key", "Previous drawing"),
    ("scrub", "Sound while dragging"),
    ("in", "Loop start"),
    ("out", "Loop end"),
];

#[test]
fn auwords_vocab_table_is_final() {
    let words: Vec<String> = vocab::TABLE.iter().map(|(_, value, _)| value.to_lowercase()).collect();
    for (old, new) in SECTION_3 {
        assert!(words.contains(&new.to_lowercase()), "{old} became {new}, which is not in the table");
    }
    for (name, value, word) in vocab::TABLE {
        assert_eq!(value, word, "{name}");
        let met = measure::jargon_in(&[value.to_string()]);
        assert!(!met.iter().any(|t| is_japanese(t)), "{name}: {value} is a studio word");
    }
    // AU26 flipped its two by name and left the words for here.
    assert_eq!((vocab::LONGER, vocab::SHORTER), ("Longer", "Shorter"));
}

/// Every old word that names a command, and the command it still finds.
const OLD_WORDS: &[(&str, Action)] = &[
    ("onion", Action::ToggleOnion),
    ("hold +", Action::HoldPlus),
    ("hold −", Action::HoldMinus),
    ("next key", Action::NextKey),
    ("previous key", Action::PrevKey),
    ("next cut", Action::NextCut),
    ("previous cut", Action::PrevCut),
    ("alpha lock", Action::ToggleAlphaLock),
    ("scrub", Action::ToggleScrub),
    ("lift key", Action::ClearFrameKey),
    ("render queue", Action::ShowRenderQueue),
    ("satsuei", Action::BuildSatsuei),
    ("composite", Action::BuildComposite),
    ("genga", Action::RoomDrawing),
    ("shiage", Action::RoomFinishing),
];

#[test]
fn auwords_every_old_word_is_an_alias() {
    for (old, action) in OLD_WORDS {
        assert!(
            action.aliases().iter().any(|a| a.eq_ignore_ascii_case(old)),
            "{action:?} lost {old}: {:?}",
            action.aliases()
        );
        assert!(cmdpal::ranked(old, &[]).contains(action), "Search no longer finds {action:?} by {old}");
    }
    // Driven from the table: a row whose new word names a command keeps its old one.
    let mut rows = 0;
    for (old, new) in SECTION_3 {
        for action in Action::ALL.iter().filter(|a| a.label().trim_end_matches('…').eq_ignore_ascii_case(new)) {
            rows += 1;
            let found = cmdpal::ranked(old, &[]);
            assert!(found.contains(action), "Search lost {old} for {action:?}: {found:?}");
        }
    }
    assert!(rows >= 6, "the table named {rows} commands: a walk that reads nothing proves nothing");
}

#[test]
fn auwords_palette_finds_the_six_studio_words() {
    for (word, action) in [
        ("genga", Action::RoomDrawing),
        ("douga", Action::MakeInbetweens),
        ("shiage", Action::RoomFinishing),
        ("satsuei", Action::RoomComposite),
        ("henshū", Action::RoomEdit),
        ("shaku", Action::InsertFrames),
    ] {
        let found = cmdpal::ranked(word, &[]);
        assert!(found.contains(&action), "{word} does not find {action:?}: {found:?}");
    }
}

/// The steps a person can stand in, by project kind, as the screen says them.
fn step_visits() -> Vec<(EditorHarness, Stage)> {
    let stages = [Stage::Layout, Stage::Drawing, Stage::InkPaint, Stage::Composite, Stage::Edit];
    let mut visits = Vec::new();
    for stage in stages {
        let mut h = EditorHarness::as_animation();
        h.editor_mut().enter_room(stage);
        crate::aurail_tests::window(&mut h, 1600.0, 900.0, 1.0);
        visits.push((h, stage));
    }
    visits
}

fn is_japanese(term: &str) -> bool {
    measure::TERMS.iter().any(|(name, _, tier)| *name == term && *tier == Tier::Japanese)
}

fn japanese_in(c: &measure::Census) -> Vec<&'static str> {
    c.terms.iter().filter(|t| is_japanese(t)).copied().collect()
}

#[test]
fn auwords_no_japanese_in_any_step() {
    for step in steps::CATALOG {
        let said = [step.label.to_string(), step.purpose.to_string()];
        let met = measure::jargon_in(&said);
        assert!(!met.iter().any(|t| is_japanese(t)), "{} in the catalog: {met:?}", step.label);
    }
    for (mut h, stage) in step_visits() {
        let c = measure::census(&mut h);
        assert_eq!(c.japanese, 0, "{stage:?}: {:?}", japanese_in(&c));
    }
}

/// The hand the walks start from: a project in a step, at the census' window.
fn standing(state: crate::doc::AppState, stage: Stage) -> EditorHarness {
    let mut h = EditorHarness::with_state_shipped(state);
    h.editor_mut().enter_room(stage);
    crate::aurail_tests::window(&mut h, 1600.0, 900.0, 1.0);
    h
}

fn blank() -> crate::doc::AppState {
    crate::newproject::NewProjectForm::default().new_state()
}

/// The sample, with a dialogue clip and a music track on the sheet.
fn sample_with_sound() -> crate::doc::AppState {
    use anim_core::audio::{AudioTrack, TrackKind, TrackProps};
    use anim_core::command::Command;
    let tone = |ext: &str| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/anim-core/tests/fixtures")
            .join(format!("tone.{ext}"));
        let bytes = std::fs::read(path).expect("the fixture");
        anim_core::model::AudioClip::from_bytes(format!("tone.{ext}"), bytes).expect("a clip")
    };
    let mut s = crate::sample::build_sample();
    let at = s.at();
    let track = AudioTrack { props: TrackProps::new("bgm", TrackKind::Music), clip: tone("flac") };
    let sound = vec![
        Command::SetCutAudio { at, audio: Some(tone("wav")) },
        Command::AddTrack { at, index: 1, track },
    ];
    s.engine.apply("sound", sound).expect("the sound lands");
    s
}

/// A hand-built graph, so Full finishing setup stops to ask.
fn hand_built() -> crate::doc::AppState {
    let mut s = crate::sample::build_sample();
    let fx = anim_core::graph::NodeKind::effect(FxKind::Blur);
    s.graph_add_node(fx, (0.0, 0.0));
    s
}

/// A surface of the app, by name, as it stands on screen.
type Surface = (String, EditorHarness);

/// Every step of both project kinds, each mode, every float and every tab a
/// person can reach, each read at the census' window.
fn surfaces() -> Vec<Surface> {
    let mut all: Vec<Surface> = step_visits().into_iter().map(|(h, s)| (format!("{s:?}"), h)).collect();
    for stage in [Stage::Manga, Stage::InkPaint, Stage::Export] {
        all.push((format!("comic {stage:?}"), standing(crate::aukind_tests::a_comic(), stage)));
    }
    all.push(("animation Export".into(), standing(sample_with_sound(), Stage::Export)));
    all.push(("Manga on a blank".into(), standing(blank(), Stage::Manga)));
    for (name, action) in [("Brush maker", Action::BrushMakerRoom), ("3D", Action::Layout3dRoom)] {
        let mut h = standing(blank(), Stage::Drawing);
        h.perform(action);
        h.run_frames(3);
        all.push((name.to_string(), h));
    }
    for pane in Pane::ALL {
        let mut h = standing(sample_with_sound(), Stage::Drawing);
        h.editor_mut().dock = egui_dock::DockState::new(vec![*pane]);
        h.run_frames(3);
        all.push((format!("{pane:?} tab, with sound"), h));
    }
    for float in crate::floats::Float::ALL {
        for (name, state) in [("blank", blank()), ("sample", crate::sample::build_sample())] {
            let mut h = standing(state, Stage::Drawing);
            h.open_float(float);
            all.push((format!("{float:?} float, {name}"), h));
        }
    }
    for mode in crate::canvas::OnionMode::ALL {
        let mut h = standing(blank(), Stage::Drawing);
        h.editor_mut().canvas.onion.mode = mode;
        h.open_float(crate::floats::Float::CanvasOptions);
        all.push((format!("Ghost frames, {mode:?}"), h));
    }
    for pencil in ["atari", "genga", "shusei"] {
        let mut h = standing(blank(), Stage::Drawing);
        h.editor_mut().canvas.armed_preset = Some(pencil.to_string());
        h.open_float(crate::floats::Float::Brushes);
        all.push((format!("{pencil} pencil armed"), h));
    }
    let mut h = standing(crate::sample::build_sample(), Stage::Composite);
    h.perform(Action::ToggleCompositeView);
    h.run_frames(3);
    all.push(("the Result screen".into(), h));
    for (name, action) in [("rebuild", Action::BuildComposite), ("full finishing", Action::BuildSatsuei)] {
        let mut h = standing(hand_built(), Stage::Composite);
        h.perform(action);
        h.run_frames(3);
        all.push((format!("{name} modal"), h));
    }
    let mut h = standing(hand_built(), Stage::Composite);
    h.perform(Action::AddNode);
    h.run_frames(3);
    all.push(("the add menu".into(), h));
    all
}

/// What a Settings page says, read off a window of its own.
fn settings_page(page: config::SettingsCategory) -> Vec<String> {
    struct Page {
        config: config::Config,
        category: config::SettingsCategory,
    }
    let state = Page { config: config::Config::default(), category: page };
    let size = egui::vec2(1100.0, 1400.0);
    let h = crate::testkit::PanelHarness::new(size, state, |ui, s: &mut Page| {
        let (mut open, mut capturing, mut action, mut check) = (true, None, None, false);
        let (mut join, mut name, mut act, mut autosaves) = (None, String::new(), None, false);
        config::settings_window(
            ui.ctx(),
            &mut open,
            &mut s.config,
            &mut capturing,
            &mut s.category,
            "test",
            None,
            &mut action,
            "offline",
            false,
            false,
            &[],
            "",
            &mut check,
            &[],
            &mut join,
            &["mine".to_string()],
            false,
            &mut name,
            &mut act,
            None,
            &mut autosaves,
            None,
        );
    });
    h.describe().lines().map(str::to_string).collect()
}

const SETTINGS_PAGES: [config::SettingsCategory; 10] = [
    config::SettingsCategory::Shortcuts,
    config::SettingsCategory::General,
    config::SettingsCategory::Performance,
    config::SettingsCategory::Pen,
    config::SettingsCategory::Layers,
    config::SettingsCategory::Brushes,
    config::SettingsCategory::UiFeatures,
    config::SettingsCategory::Layout,
    config::SettingsCategory::Session,
    config::SettingsCategory::Plugins,
];

/// What the New Project form says.
fn new_project_words() -> Vec<String> {
    let form = crate::newproject::NewProjectForm::default();
    let size = egui::vec2(900.0, 900.0);
    let p = crate::testkit::PanelHarness::new(size, form, |ui, f| {
        crate::newproject::form_ui(ui, f, true);
    });
    p.describe().lines().map(str::to_string).collect()
}

#[test]
fn auwords_no_japanese_on_any_surface() {
    let surfaces = surfaces();
    assert!(surfaces.len() >= 40, "the walk read {} surfaces: it proves nothing", surfaces.len());
    let mut faults = Vec::new();
    for (name, mut h) in surfaces {
        let c = measure::census(&mut h);
        println!("{name}: japanese {}, jargon {} {:?}", c.japanese, c.jargon, c.terms);
        if c.japanese != 0 {
            faults.push(format!("{name}: {:?}", japanese_in(&c)));
        }
    }
    let spoken_elsewhere = |name: String, words: Vec<String>| {
        let met = measure::jargon_in(&words);
        println!("{name}: jargon {met:?}");
        met.iter().any(|t| is_japanese(t)).then(|| format!("{name}: {met:?}"))
    };
    faults.extend(spoken_elsewhere("New Project".into(), new_project_words()));
    for page in SETTINGS_PAGES {
        faults.extend(spoken_elsewhere(format!("Settings {page:?}"), settings_page(page)));
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

/// The float states the budget was broken on, and blank Draw under each.
#[test]
fn auwords_jargon_budget_holds_on_each_float() {
    let mut faults = Vec::new();
    for float in crate::floats::Float::ALL {
        let mut h = standing(blank(), Stage::Drawing);
        h.open_float(float);
        let c = measure::census(&mut h);
        println!("{float:?}: jargon {} {:?}", c.jargon, c.terms);
        if c.jargon > 3 {
            faults.push(format!("{float:?}: {:?}", c.terms));
        }
    }
    for mode in crate::canvas::OnionMode::ALL {
        let mut h = standing(blank(), Stage::Drawing);
        h.editor_mut().canvas.onion.mode = mode;
        h.open_float(crate::floats::Float::CanvasOptions);
        let c = measure::census(&mut h);
        println!("Ghost frames {mode:?}: jargon {} {:?}", c.jargon, c.terms);
        if c.jargon > 3 {
            faults.push(format!("Ghost frames {mode:?}: {:?}", c.terms));
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}

/// Picking a pencil names it by its display word; the state keeps the stored one.
#[test]
fn auwords_armed_pencil_says_its_display_word() {
    for (stored, shown) in [("atari", "Sketch pencil"), ("genga", "Clean pencil"), ("shusei", "Correction pencil")] {
        let mut h = standing(blank(), Stage::Drawing);
        h.editor_mut().canvas.armed_preset = Some(stored.to_string());
        h.open_float(crate::floats::Float::Brushes);
        let line = format!("{}: {shown}", vocab::DRAWING_WITH);
        assert!(h.has_text_containing(&line), "{}", h.describe());
        assert_eq!(h.editor().canvas.armed_preset.as_deref(), Some(stored), "the stored name stays");
        assert_eq!(measure::census(&mut h).japanese, 0, "{stored}");
    }
}

/// The Effects screen, read: the display word on the painted graph and in the
/// add menu, the stored kind still what the file keeps.
#[test]
fn auwords_effects_screen_says_the_display_words() {
    use anim_core::graph::NodeKind;
    let mut h = standing(crate::sample::build_sample(), Stage::Composite);
    assert!(h.state_mut().build_satsuei(), "{}", h.state().refusal);
    h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::NodeGraph]);
    h.run_frames(3);
    let said: Vec<String> = measure::reading(&h).texts.into_iter().map(|t| t.1).collect();
    assert!(said.iter().any(|t| t == vocab::fx(FxKind::Grade.name())), "{said:?}");
    assert!(said.iter().any(|t| t == vocab::DRAWINGS), "{said:?}");
    for stored in ["Grade", "Matte", "DrawingSource", "Diffusion"] {
        assert!(!said.iter().any(|t| t == stored), "the stored {stored} reached the graph: {said:?}");
    }
    assert!(h.state().cut().graph.nodes().any(|n| matches!(n.kind, NodeKind::DrawingSource { .. })));
    h.perform(Action::AddNode);
    h.run_frames(3);
    // The menu scrolls: each word is typed, and read from the row it finds.
    for word in ["Soft focus", "Color look", "Color wash", "Color table", "Cut-out", "Blur", "Camera"] {
        h.state_mut().graph_ui.add_menu.as_mut().expect("the menu is open").query = word.to_string();
        h.run_frames(3);
        let said: Vec<String> = measure::reading(&h).texts.into_iter().map(|t| t.1).collect();
        assert!(said.iter().filter(|t| t.contains(word)).count() >= 1, "the add menu lacks {word}: {said:?}");
        for romaji in ["bokashi", "difyūjon", "tōkakō", "renban", "kamera"] {
            assert!(!said.iter().any(|t| t.contains(romaji)), "{romaji} printed in the menu: {said:?}");
        }
    }
    // Its studio word waits in the row's card.
    h.state_mut().graph_ui.add_menu.as_mut().expect("the menu is open").query = "Blur".to_string();
    h.run_frames(3);
    let row = h.rects("Blur").into_iter().next().expect("the Blur row").center();
    let said = card_at(&mut h, row);
    assert!(said.iter().any(|t| t.contains("Studio word: bokashi")), "{said:?}");
}

/// T.U. and T.B. name their direction; the stored key stays `zoom`.
#[test]
fn auwords_camera_moves_say_zoom_in_and_zoom_out() {
    use anim_core::fx::FxValue;
    let name = |value: f32| crate::fxui::param_legend("Camera", "zoom", "zoom", &FxValue::Float(value as _));
    assert_eq!(name(1.5), vocab::ZOOM_IN);
    assert_eq!(name(0.5), vocab::ZOOM_OUT);
    assert_eq!((vocab::ZOOM_IN, vocab::ZOOM_OUT), ("Zoom in", "Zoom out"));
}

/// Canvas options and the Settings nav start each name with a capital.
#[test]
fn auwords_sentence_case_in_canvas_options_and_settings() {
    let mut h = standing(blank(), Stage::Drawing);
    h.open_float(crate::floats::Float::CanvasOptions);
    let names = ["Miss check", "Light table", "Fit", vocab::GHOST_FRAMES];
    for name in names {
        assert!(h.has_text_containing(name), "{name}: {}", h.describe());
    }
    for lower in ["onion", "safe", "peg", "fit", "upright", "mirror", "light table", "miss check"] {
        assert!(!h.has(lower), "{lower} is still a lower-case label: {}", h.describe());
    }
    let page = settings_page(config::SettingsCategory::General);
    for name in ["General", "Performance", "Layers", "Brushes", "Plugins", "Arrangements"] {
        assert!(page.iter().any(|l| l.contains(name)), "{name}: {page:?}");
    }
}

#[test]
fn auwords_jargon_budget_blank_draw() {
    let mut h = EditorHarness::as_shipped();
    crate::aurail_tests::window(&mut h, 1600.0, 900.0, 1.0);
    let c = measure::census(&mut h);
    println!("blank Draw: jargon {} ({:?}), japanese {}", c.jargon, c.terms, c.japanese);
    assert!(c.jargon <= 3, "{} kinds: {:?}", c.jargon, c.terms);
    assert_eq!(c.japanese, 0, "{:?}", c.terms);
}

/// A card's words, without the line that is allowed to teach the trade word.
fn plain_part(text: &str) -> &str {
    text.split("Studio word:").next().unwrap_or(text)
}

/// Every control of the frame, rested on until its card shows: the strings
/// that were not on screen before.
fn hover_sweep(h: &mut EditorHarness) -> Vec<String> {
    let layers = h.ctx().viewport(|v| {
        let w = &v.prev_pass.widgets;
        let mut at = Vec::new();
        for (_, rects) in w.layers() {
            at.extend(rects.iter().filter(|r| r.sense.interactive()).map(|r| r.interact_rect));
        }
        at
    });
    let window = h.ctx().content_rect();
    let mut centres: Vec<egui::Pos2> = Vec::new();
    for rect in layers {
        let rect = rect.intersect(window);
        // The paper, the strip's ground and the dock are not controls.
        if rect.is_positive() && rect.width() < 300.0 && rect.height() < 200.0 {
            let c = rect.center();
            if !centres.iter().any(|o| o.distance(c) < 2.0) {
                centres.push(c);
            }
        }
    }
    let before: BTreeSet<String> = measure::reading(h).texts.into_iter().map(|t| t.1).collect();
    let mut seen = BTreeSet::new();
    for at in centres {
        h.kit_mut().input_mut().events.push(egui::Event::PointerMoved(at));
        h.kit_mut().step();
        h.wait(plate::CARD_DELAY + 0.05);
        h.run_frames(2);
        for (_, text) in measure::reading(h).texts {
            if !before.contains(&text) {
                seen.insert(plain_part(&text).to_string());
            }
        }
    }
    seen.into_iter().collect()
}

/// The walk the 24 was counted by, `help()` and FX help included, then the
/// hover sweep over every step of an animation and of a comic.
fn tooltip_words() -> Vec<String> {
    let mut words: Vec<String> = Action::ALL.iter().map(|a| a.help().to_string()).collect();
    words.extend(FxKind::ALL.iter().map(|k| k.help().to_string()));
    let mut hands = vec![EditorHarness::as_shipped(), blank_drawing()];
    hands.extend(step_visits().into_iter().map(|(h, _)| h));
    hands.push(standing(crate::aukind_tests::a_comic(), Stage::Manga));
    hands.push(standing(blank(), Stage::Export));
    for mut h in hands {
        crate::aurail_tests::window(&mut h, 1600.0, 900.0, 1.0);
        measure::census(&mut h);
        words.extend(hover_sweep(&mut h));
    }
    words
}

/// The screen the baseline's 58 of 98 was read from: a blank file in the Draw
/// step, every panel of the frozen arrangement open.
fn blank_drawing() -> EditorHarness {
    let mut h = EditorHarness::with_state(crate::newproject::NewProjectForm::default().new_state());
    h.editor_mut().enter_room(Stage::Drawing);
    h
}

#[test]
fn auwords_tooltip_jargon_budget() {
    let words = tooltip_words();
    let kinds = measure::jargon_in(&words);
    for kind in &kinds {
        for w in &words {
            if measure::jargon_in(std::slice::from_ref(w)).contains(kind) {
                println!("{kind}: {w}");
            }
        }
    }
    println!("tooltip jargon: {} kinds {kinds:?} over {} strings", kinds.len(), words.len());
    assert!(kinds.len() <= 3, "{} kinds: {kinds:?}", kinds.len());
    assert!(!kinds.iter().any(|k| is_japanese(k)), "{kinds:?}");
    let cards = words.len() - Action::ALL.len() - FxKind::ALL.len();
    assert!(cards >= 20, "the sweep read {cards} cards: a walk that reads nothing proves nothing");
}

#[test]
fn auwords_painted_text_size() {
    let read = |mut h: EditorHarness, name: &str| {
        crate::aurail_tests::window(&mut h, 1600.0, 900.0, 1.0);
        let c = measure::census(&mut h);
        let small = c.texts_at_most(8.0) as f32 / c.text_heights.len().max(1) as f32;
        println!("{name}: median {} px, {:.1}% at 8 px or less", c.median_text_height(), small * 100.0);
        (c.median_text_height(), small)
    };
    for (name, h) in [("blank Draw", EditorHarness::as_shipped()), ("first run", EditorHarness::as_first_run())] {
        let (median, small) = read(h, name);
        assert!(median >= 10.0, "{name}: median {median}");
        assert!(small <= 0.10, "{name}: {:.1}% at 8 px or less", small * 100.0);
    }
    // The frozen arrangement no boot gives a user, the screen the 8 px and 58
    // of 98 were read on: the same target, not a softer one.
    let (median, small) = read(blank_drawing(), "the frozen arrangement");
    assert!(median >= 10.0, "the frozen arrangement: median {median}");
    assert!(small <= 0.10, "the frozen arrangement: {:.1}% at 8 px or less", small * 100.0);
}

#[test]
fn auwords_stored_names_are_unchanged() {
    let keys: Vec<&str> = [Stage::Layout, Stage::Drawing, Stage::InkPaint, Stage::Composite, Stage::Edit]
        .map(|s| stage_key(Some(s)))
        .to_vec();
    assert_eq!(keys, ["layout", "drawing", "finishing", "composite", "edit"]);
    let stacks: Vec<String> = config::builtin_stacks().into_iter().map(|s| s.name).collect();
    for stored in ["Solo merged", "Layout (LO)", "Genga", "Shiage (full)"] {
        assert!(stacks.iter().any(|s| s == stored), "{stored}: {stacks:?}");
    }
    assert_eq!(
        [FxKind::Diffusion.name(), FxKind::Grade.name(), FxKind::Para.name(), FxKind::Lut.name(), FxKind::Matte.name()],
        ["Diffusion", "Grade", "Para", "LUT", "Matte"]
    );
    let statuses: Vec<&str> = CutStatus::ALL.iter().map(|s| s.key()).collect();
    assert_eq!(statuses, ["lo", "genga", "sakkan", "douga", "paint", "composite", "done", "retake"]);
    assert_eq!(LayerRole::Line.key(), "line");
    assert_eq!(ColumnKind::Cel.key(), "cel");
    assert_eq!(DrawingKind::Key.key(), "key");
    // Serde writes the variant names: a display word never reaches a file.
    assert_eq!(serde_json::to_string(&Pane::XSheet).unwrap(), "\"XSheet\"");
    assert_eq!(serde_json::to_string(&Pane::RenderQueue).unwrap(), "\"RenderQueue\"");
    assert_eq!(serde_json::to_string(&Action::HoldPlus).unwrap(), "\"HoldPlus\"");
}

#[test]
fn auwords_display_maps_are_complete() {
    let plain = |word: &str, of: String| {
        assert!(!word.is_empty(), "{of}");
        // Spec 3 keeps Key pose and Inbetween inside `Mark as ▸`, and a color
        // name may be a line.
        let kept = ["line (layer role)", "inbetween", "key"];
        let met = measure::jargon_in(&[word.to_string()]);
        assert_eq!(met.iter().filter(|t| !kept.contains(t)).count(), 0, "{of}: {word}: {met:?}");
    };
    for k in ColumnKind::ALL {
        plain(vocab::column_kind(k), format!("{k:?}"));
        plain(vocab::column_kind_chip(k), format!("{k:?}"));
    }
    for k in LayerRole::ALL {
        plain(vocab::layer_role(k), format!("{k:?}"));
    }
    for k in CutStatus::ALL {
        plain(vocab::cut_status(k), format!("{k:?}"));
    }
    for k in DrawingKind::ALL {
        plain(vocab::drawing_kind(k), format!("{k:?}"));
    }
    for k in FxKind::ALL.iter().filter(|k| **k != FxKind::Matte) {
        plain(vocab::fx(k.name()), format!("{k:?}"));
    }
    // Section 3's own word, which the census reads as a `cut`.
    assert_eq!(vocab::fx(FxKind::Matte.name()), "Cut-out");
    for s in config::builtin_stacks() {
        plain(vocab::stack(&s.name), s.name.clone());
    }
    for name in ["atari", "genga", "shusei"] {
        plain(vocab::preset(name), name.to_string());
    }
    assert_eq!(vocab::stack("my set"), "my set", "a typed name stays typed");
    assert_eq!(vocab::preset("my pen"), "my pen", "a typed name stays typed");
}

#[test]
fn auwords_hover_card_gives_the_studio_word() {
    for step in steps::CATALOG {
        if !step.studio_words.is_empty() {
            assert!(step.card().ends_with(&format!("\nStudio word: {}", step.studio_words)), "{}", step.label);
        }
    }
    // Beyond the steps: a label whose row has a studio term.
    for (label, word) in [
        (vocab::cut_status(CutStatus::Genga), "genga"),
        (vocab::cut_status(CutStatus::Sakkan), "sakkan"),
        (vocab::cut_status(CutStatus::Douga), "douga"),
    ] {
        assert!(vocab::studio_word(label) == Some(word), "{label}");
    }
    let mut h = EditorHarness::as_animation();
    crate::aurail_tests::window(&mut h, 1600.0, 900.0, 1.0);
    measure::census(&mut h);
    h.hover(vocab::STEP_COLOR);
    h.wait(plate::CARD_DELAY + 0.05);
    h.run_frames(3);
    let words = measure::reading(&h).texts;
    assert!(words.iter().any(|(_, t)| t.contains("Studio word: shiage")), "{words:?}");
    assert!(crate::menus::tip_with(Action::LipFlap, None).ends_with("Studio word: kuchipaku"));
}

/// Rest the pointer here until the card shows, and read the card.
fn card_at(h: &mut EditorHarness, at: egui::Pos2) -> Vec<String> {
    h.kit_mut().input_mut().events.push(egui::Event::PointerMoved(at));
    h.kit_mut().step();
    h.wait(plate::CARD_DELAY + 0.05);
    h.run_frames(3);
    measure::reading(h).texts.into_iter().map(|t| t.1).collect()
}

#[test]
fn auwords_real_controls_show_the_studio_word_line() {
    let mut h = standing(crate::sample::build_sample(), Stage::Drawing);
    h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::XSheet]);
    h.run_frames(3);
    h.click(crate::plate::more_name(crate::plate::MORE_SHEET));
    h.run_frames(3);
    for (legend, word) in [(vocab::LENGTH, "shaku"), (vocab::NOTES, "serifu")] {
        let at = h.rect(legend).right_center() + egui::vec2(18.0, 0.0);
        let said = card_at(&mut h, at);
        let line = format!("Studio word: {word}");
        assert!(said.iter().any(|t| t.contains(&line)), "{legend}: {said:?}");
    }
    // A status row in the Shots tab's filter.
    let mut h = standing(crate::sample::build_sample(), Stage::Drawing);
    h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::Board]);
    h.run_frames(3);
    h.click(crate::plate::more_name(crate::plate::MORE_SHOTS));
    h.run_frames(2);
    h.click_combo("any status");
    let row = vocab::cut_status(CutStatus::Sakkan);
    let at = h.rect(row).center();
    let said = card_at(&mut h, at);
    assert!(said.iter().any(|t| t.contains("Studio word: sakkan")), "{said:?}");
    // A stack row in the sheet's Layer set menu.
    let mut h = standing(crate::sample::build_sample(), Stage::Drawing);
    h.open_float(crate::floats::Float::Layers);
    h.click(crate::plate::more_name(crate::plate::MORE_LAYERS));
    h.run_frames(2);
    h.click(vocab::LAYER_SET);
    h.run_frames(2);
    let row = vocab::stack("Genga");
    let at = h.rect(row).center();
    let said = card_at(&mut h, at);
    assert!(said.iter().any(|t| t.contains("Studio word:")), "{row}: {said:?}");
}

/// A number handed straight to a font: the ratchet's one question.
fn bare_sizes(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (n, line) in source.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        let literal = |at: usize| code[at..].trim_start().starts_with(|c: char| c.is_ascii_digit());
        for call in ["FontId::new(", "FontId::proportional(", "FontId::monospace("] {
            if let Some(at) = code.find(call) {
                if literal(at + call.len()) {
                    found.push(format!("{}: {}", n + 1, line.trim()));
                }
            }
        }
        if let Some(at) = code.find(".size(") {
            let is_font_chain = code.contains("RichText") || code.trim_start().starts_with(".size(");
            if is_font_chain && literal(at + ".size(".len()) {
                found.push(format!("{}: {}", n + 1, line.trim()));
            }
        }
        if code.contains(".monospace()") {
            found.push(format!("{}: {}", n + 1, line.trim()));
        }
    }
    found
}

#[test]
fn auwords_no_bare_font_sizes() {
    assert_eq!(bare_sizes("egui::FontId::proportional(10.0)\n").len(), 1, "the ratchet itself");
    assert_eq!(bare_sizes("  .size(10.5)\n").len(), 1, "the ratchet itself");
    assert_eq!(bare_sizes("x.monospace()\n").len(), 1, "the ratchet itself");
    assert_eq!(bare_sizes("// FontId::proportional(10.0)\nplate::Role::Small.font()\n").len(), 0);
    let mut hits = Vec::new();
    for (name, text) in crate::ausay_tests::sources() {
        let test_file = name.ends_with("_tests.rs") || name == "testkit.rs" || name == "measure.rs";
        if test_file {
            continue;
        }
        // What is under a file's own test module drives no screen. The tree
        // is CRLF, so the mark is looked for across either line ending.
        let text = text.replace("\r\n", "\n");
        let shipped = text.split("#[cfg(test)]\nmod tests").next().unwrap_or(&text);
        for hit in bare_sizes(shipped) {
            // The one literal the look keeps: file patterns' face, at 12.
            if !(name == "plate.rs" && hit.contains("FontId::new(12.0, mono())")) {
                hits.push(format!("{name}:{hit}"));
            }
        }
    }
    assert!(hits.is_empty(), "{} bare sizes:\n{}", hits.len(), hits.join("\n"));
}

/// Section 3's old words as a string would say them.
const RETIRED: &[&str] = &[
    "cel", "cels", "cut", "cuts", "column", "columns", "onion", "room", "rooms", "workspace", "workspaces",
    "x-sheet", "xsheet", "genga", "douga", "shiage", "satsuei", "henshū", "henshu", "koma", "peji", "serifu",
    "atari", "shusei", "nuri", "kuchipaku", "shinkō", "shinko", "bokashi", "difyūjon", "tōkakō", "renban",
    "ekonte", "sakkan", "shaku", "aka", "ao", "node graph", "render queue", "stabilizer", "lasso", "armed",
];

/// The ceiling on `auwords_allow.txt`. It only ever comes down.
const ALLOWED_AT_MOST: usize = 99;

fn word_at(hay: &str, word: &str) -> bool {
    let inside = |c: char| c.is_alphanumeric() || c == '_';
    hay.match_indices(word).any(|(at, _)| {
        !hay[..at].chars().next_back().is_some_and(inside) && !hay[at + word.len()..].chars().next().is_some_and(inside)
    })
}

/// The string literals of a line of code.
fn literals(line: &str) -> Vec<String> {
    let line = line.replace("'\"'", "");
    let (mut found, mut open, mut escaped) = (Vec::new(), None::<String>, false);
    for c in line.chars() {
        match &mut open {
            None if c == '"' => open = Some(String::new()),
            None => {}
            Some(text) if escaped => {
                escaped = false;
                text.push(c);
            }
            Some(_) if c == '\\' => escaped = true,
            Some(_) if c == '"' => found.push(open.take().expect("open")),
            Some(text) => text.push(c),
        }
    }
    found
}

/// The app's and the core's shipped sources, their test modules gone.
fn shipped_text() -> Vec<(String, String)> {
    let mut all = crate::ausay_tests::sources();
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/anim-core/src");
    for file in std::fs::read_dir(core).expect("the core").flatten() {
        let name = file.file_name().to_string_lossy().to_string();
        if name.ends_with(".rs") {
            all.push((format!("core/{name}"), std::fs::read_to_string(file.path()).expect("readable")));
        }
    }
    all.into_iter().map(|(n, t)| (n, before_test_modules(&t))).collect()
}

/// A source up to its first `#[cfg(test)] mod x {`, whatever the module's name.
fn before_test_modules(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let opens = |n: usize| {
        lines[n].trim() == "#[cfg(test)]"
            && lines.get(n + 1).is_some_and(|l| l.trim_start().starts_with("mod ") && l.contains('{'))
    };
    let end = (0..lines.len()).find(|&n| opens(n)).unwrap_or(lines.len());
    lines[..end].join("\n")
}

/// Code that is not screen words: logs, panics, SQL, and the structural homes
/// of the aliases and the studio-word line (the rule's two allowed places).
fn not_screen_words(line: &str) -> bool {
    let code = line.trim_start();
    const MARKS: [&str; 12] = [
        "expect(", "panic!", "println!", "eprintln!", "unreachable!", "slog", "SELECT ", "INSERT ", "studio_words:",
        "term: Some(", "assert", "Studio word:",
    ];
    code.starts_with("//") || code.starts_with("#[") || MARKS.iter().any(|mark| code.contains(mark))
}

/// Every retired word still standing in a shipped literal: `file|literal`.
fn retired_sites() -> BTreeSet<String> {
    let mut sites = BTreeSet::new();
    for (name, text) in shipped_text() {
        if name == "vocab.rs" {
            continue; // The map itself: stored keys and studio words.
        }
        let mut until: Option<(usize, bool)> = None;
        for line in text.lines() {
            let indent = line.len() - line.trim_start().len();
            if let Some((depth, list)) = until {
                let end = format!("{}}}", " ".repeat(depth));
                if if list { line.trim_start().starts_with(']') } else { line.trim_end() == end } {
                    until = None;
                }
                continue;
            }
            if line.contains("fn aliases(") || line.contains("fn term(") {
                until = Some((indent, false));
                continue;
            }
            if line.trim_end().ends_with("aliases: &[") {
                until = Some((indent, true));
                continue;
            }
            if line.contains("aliases: &[") || not_screen_words(line) {
                continue;
            }
            for literal in literals(line) {
                let lower = literal.to_lowercase();
                if RETIRED.iter().any(|word| word_at(&lower, word)) {
                    sites.insert(format!("{name}|{literal}"));
                }
            }
        }
    }
    sites
}

/// The list can only fall: a new old word fails, and so does an entry whose
/// string has gone, until the list and its ceiling come down.
#[test]
fn auwords_retired_words_only_fall() {
    assert_eq!(literals(r#"a("one \"two\"", "b");"#), ["one \"two\"", "b"]);
    assert!(word_at("a cel here", "cel") && !word_at("cancel", "cel"));
    let allowed: BTreeSet<String> = include_str!("auwords_allow.txt")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    let found = retired_sites();
    let new: Vec<&str> = found.difference(&allowed).map(|s| s.as_str()).collect();
    let gone: Vec<&str> = allowed.difference(&found).map(|s| s.as_str()).collect();
    assert!(new.is_empty(), "{} retired words in shipped strings:\n{}", new.len(), new.join("\n"));
    assert!(gone.is_empty(), "take these off the list and lower the ceiling:\n{}", gone.join("\n"));
    assert!(allowed.len() <= ALLOWED_AT_MOST, "{} entries: the list only falls", allowed.len());
}

/// Every row of the table is read by shipped code: an unused row is a word
/// that never reached the screen.
#[test]
fn auwords_every_display_row_has_a_shipped_use() {
    let sources = crate::ausay_tests::sources();
    let vocab_text = sources.iter().find(|(n, _)| n == "vocab.rs").expect("the table").1.clone();
    let code = vocab_text.split("pub const TABLE").next().expect("a first half");
    let mut unused = Vec::new();
    for (name, ..) in vocab::TABLE {
        let path = format!("vocab::{name}");
        let by_panel = sources.iter().any(|(n, t)| n != "vocab.rs" && word_at(crate::ausay_tests::shipped(t), &path));
        let by_map = code.lines().any(|l| !l.starts_with("pub const") && word_at(l, name));
        if !by_panel && !by_map {
            unused.push(*name);
        }
    }
    assert!(unused.is_empty(), "rows with no shipped reader: {unused:?}");
    assert!(vocab::TABLE.len() >= 80);
}

/// The sentences a person reads when stuck, each read from its source.
#[test]
fn auwords_refusals_speak_plain() {
    let mut said = Vec::new();
    for (name, text) in crate::ausay_tests::sources() {
        let text = crate::ausay_tests::shipped(&text).to_string();
        let lines: Vec<&str> = text.lines().collect();
        for (n, line) in lines.iter().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || !(line.contains("refuse(") || line.contains("refused —")) {
                continue;
            }
            for near in lines.iter().skip(n).take(3) {
                said.extend(literals(near).into_iter().map(|l| (name.clone(), l)));
            }
        }
    }
    assert!(said.len() >= 60, "the sweep read {} refusals: it proves nothing", said.len());
    let mut faults = Vec::new();
    for (file, words) in &said {
        let lower = words.to_lowercase();
        let met = measure::jargon_in(std::slice::from_ref(words));
        let old = ["cel", "cels", "column", "columns", "room", "composite view", "cel layers"];
        if met.iter().any(|t| is_japanese(t)) || old.iter().any(|w| word_at(&lower, w)) {
            faults.push(format!("{file}: {words}"));
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
    for sentence in [crate::doc::READ_ONLY_VIEW, crate::doc::LONGER_SAYS] {
        let met = measure::jargon_in(&[sentence.to_string()]);
        assert_eq!(met.iter().filter(|t| is_japanese(t)).count(), 0, "{sentence}");
    }
}

#[test]
fn auwords_action_help_stays_one_sentence() {
    for action in Action::ALL {
        let help = action.help();
        assert!(!help.is_empty(), "{action:?}");
        assert!(help.chars().count() <= 120, "{action:?}: {} chars", help.chars().count());
        assert_eq!(help.matches(". ").count(), 0, "{action:?}: {help}");
        assert!(help.ends_with('.'), "{action:?}: {help}");
    }
    for kind in FxKind::ALL {
        let help = kind.help();
        assert!(help.chars().count() <= 120, "{kind:?}: {} chars", help.chars().count());
        assert_eq!(help.matches(". ").count(), 0, "{kind:?}: {help}");
    }
}
