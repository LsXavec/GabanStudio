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
    ("Layout canvas", "Preview"),
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

/// The walk the 24 was counted by, `help()` and FX help included.
fn tooltip_words() -> Vec<String> {
    let mut words: Vec<String> = Action::ALL.iter().map(|a| a.help().to_string()).collect();
    words.extend(FxKind::ALL.iter().map(|k| k.help().to_string()));
    for mut h in [EditorHarness::as_shipped(), blank_drawing()] {
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
    // of 98 were read on. It stands at 9 px and 12.9%: short of the target the
    // shipped screens meet, so this only holds what AU12 won, and says so.
    let (median, small) = read(blank_drawing(), "the frozen arrangement");
    assert!(median >= 9.0, "the frozen arrangement: median {median}");
    assert!(small <= 0.13, "the frozen arrangement: {:.1}% at 8 px or less", small * 100.0);
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
        // What is under a file's own test module drives no screen.
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
