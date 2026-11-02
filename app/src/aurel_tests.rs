//! AU17: Release. What's New, what an upgrader keeps, and the sweep of the
//! comments that outlived the look. Goldens are `aukeys_tests`' hand-built
//! v0.5.0 shapes: no `api_key`, no `totp_secret`.

use eframe::egui::{self, Rect};
use serde_json::{Value, json};

use crate::auexport_tests::{ball, invoke, lane, quiet, shipped};
use crate::aukeys_tests::{chord, golden, loaded, upgraders_dir};
use crate::config::{Action, Chord, Config};
use crate::export_step::job_name;
use crate::testkit::EditorHarness;
use crate::welcome::{self, WelcomeTab};
use crate::{Startup, startup_route, vocab};

fn settle(h: &mut EditorHarness) {
    h.run();
    h.run_frames(4);
}

/// An upgrader's first launch at this size.
fn upgraded(w: f32, tall: f32) -> EditorHarness {
    let mut h = EditorHarness::with_dir_first_run(upgraders_dir());
    h.kit_mut().set_size(egui::vec2(w, tall));
    h.kit_mut().set_pixels_per_point(1.0);
    settle(&mut h);
    h
}

fn float_rect(h: &EditorHarness) -> Rect {
    let id = egui::Id::new(("float", welcome::HEADING));
    h.ctx().memory(|m| m.area_rect(id)).expect("the float on screen")
}

/// The live key names the page reads, a person's own included.
#[test]
fn aurel_whats_new_names_moved_chords_live() {
    let mut h = upgraded(1600.0, 900.0);
    assert_eq!(h.editor().start.as_ref().map(|w| w.tab), Some(WelcomeTab::WhatsNew));
    // The stock keys, as the migration left them.
    let line = "Keys that moved: Overlay track is on Alt+O, Background track is on Alt+W.";
    assert!(h.has(line), "{}", h.describe());
    assert_eq!(h.rig().config.chord_for(Action::ColumnKindBook).map(Chord::label).as_deref(), Some("Alt+O"));
    // A rebound key reads as it stands, the frame it changes.
    h.rig_mut().config.binding_mut(Action::ColumnKindBook).chord = Some(chord("Alt+L"));
    settle(&mut h);
    assert!(h.has("Keys that moved: Overlay track is on Alt+L, Background track is on Alt+W."), "{}", h.describe());
    // A key taken off says so, and never shows the old key.
    let row = h.rig_mut().config.binding_mut(Action::ColumnKindBg);
    (row.chord, row.also) = (None, Vec::new());
    settle(&mut h);
    assert!(h.has("Keys that moved: Overlay track is on Alt+L, Background track has no key."), "{}", h.describe());
    // The two chords the migration may move, and no others.
    assert_eq!(welcome::MOVED_CHORDS, [Action::ColumnKindBook, Action::ColumnKindBg]);
    // A person who kept the old key on purpose reads that key, not the stock.
    let mut old = golden("Gaban");
    let list = old["keybinds"].as_array_mut().expect("a list");
    let row = list.iter_mut().find(|row| row["action"] == "ColumnKindBook").expect("a row");
    row["chord"] = json!({ "keys": ["P"], "ctrl": false, "shift": false, "alt": true });
    let (_dir, config) = loaded(&old);
    let mut h = EditorHarness::with_dir_first_run(_dir);
    assert_eq!(config.chord_for(Action::ColumnKindBook), Some(&chord("Alt+P")));
    settle(&mut h);
    assert!(h.has_text_containing("Overlay track is on Alt+P"), "{}", h.describe());
}

/// Bars and lessons, each said once and each true of the app.
#[test]
fn aurel_whats_new_says_bars_and_lessons() {
    let h = upgraded(1600.0, 900.0);
    assert!(h.has(welcome::WHATS_NEW_BARS) && h.has(welcome::WHATS_NEW_LESSONS), "{}", h.describe());
    assert!(!h.has_text_containing("comes back with one click"), "feedback 1.11 retired the line");
    assert!(welcome::WHATS_NEW_LESSONS.contains("Nine"));
    assert_eq!(crate::lessons::ALL.len(), 9);
    assert_eq!(Action::OpenLessons.menu(), Some("Help/Lessons"));
    assert!(welcome::WHATS_NEW_LESSONS.contains("Help ▸ Lessons"));
    use crate::steps::{ProjectKind, bar};
    assert_eq!(bar(ProjectKind::Animation)[0].label, vocab::STEP_PLAN, "an animation opens on Plan");
    assert_eq!(bar(ProjectKind::Drawing)[0].label, vocab::STEP_DRAW);
}

/// The page is one screen: no scroll at the cap of every gated size, nothing
/// clipped or elided, every old word beside its new one.
#[test]
fn aurel_whats_new_is_one_screen() {
    for (w, tall) in [(1600.0, 900.0), (1280.0, 800.0), (1024.0, 640.0), (832.0, 520.0)] {
        let h = upgraded(w, tall);
        let card = float_rect(&h);
        let last = vocab::OLD_WORDS.last().expect("a row");
        let foot = h.rect(&format!("{} → {}", last.0, last.1));
        // The body's scroll ends 8 pt above the card's edge, and a last row
        // that reaches it is cut through its descenders.
        assert!(foot.bottom() <= card.bottom() - 12.0, "{w}: {foot:?} under {card:?}");
        for (old, new) in vocab::OLD_WORDS {
            assert!(h.has(&format!("{old} → {new}")), "{old}\n{}", h.describe());
        }
        let clip = crate::measure::clip_here(&h, crate::measure::Scope::All);
        assert_eq!((clip.clipped, clip.elided), (0, 0), "{w}: {:?}", clip.found);
        // The size follows the rule the other two pages keep.
        assert!(card.height() <= 300.5 && card.width() <= 600.5, "{card:?}");
    }
}

/// Each new word is the screen's own, so the list cannot drift from the table,
/// and each old word stays a Search alias, so it still finds its verb.
#[test]
fn aurel_old_words_are_the_tables_words() {
    let table: Vec<&str> = vocab::TABLE.iter().map(|(_, word, _)| *word).collect();
    for (old, new) in vocab::OLD_WORDS {
        for word in new.split(", ") {
            assert!(table.contains(&word), "{old} → {word} is no row of the table");
        }
    }
    let aliases: Vec<String> = Action::ALL
        .iter()
        .flat_map(|a| a.aliases().iter().map(|word| word.to_lowercase()))
        .collect();
    for (old, _) in vocab::OLD_WORDS {
        let old = old.to_lowercase();
        assert!(aliases.iter().any(|alias| alias.contains(&old)), "{old} is no Search alias");
    }
}

/// Once: the latch cannot hide it, the version is on disk before the second
/// launch, and that launch opens as the person left it. A fresh install has
/// nothing old to compare.
#[test]
fn aurel_upgrader_sees_whats_new_once() {
    for latch in [true, false] {
        let mut old = golden("Gaban");
        old["ui"]["show_welcome"] = json!(latch);
        let (dir, mut config) = loaded(&old);
        assert_eq!(config.ui.whats_new_seen, "0.5.0");
        assert_eq!(startup_route(false, false, None, &config), Startup::Welcome, "latch {latch}");
        let mut h = EditorHarness::with_dir_first_run(dir);
        settle(&mut h);
        assert_eq!(h.editor().start.as_ref().map(|w| w.tab), Some(WelcomeTab::WhatsNew), "latch {latch}");
        // What the App saves after that launch.
        let (w, changed) = welcome::open_at_startup(&mut config.ui, &[]);
        assert_eq!((w.tab, changed), (WelcomeTab::WhatsNew, true));
        let dir = tempfile::tempdir().expect("a data dir");
        config.save_to(dir.path()).expect("saved");
        let back = Config::load_in(dir.path());
        assert_eq!(back.ui.whats_new_seen, welcome::VERSION);
        assert_eq!(back.ui.show_welcome, latch, "What's New leaves the latch alone");
        let second = if latch { Startup::Welcome } else { Startup::ReadyPaper };
        assert_eq!(startup_route(false, false, None, &back), second, "the second launch");
        let mut ui = back.ui.clone();
        let (w, changed) = welcome::open_at_startup(&mut ui, &[]);
        assert_eq!((w.tab, changed), (WelcomeTab::Start, false));
    }
    // A config older than the field reads "" and is due, the latch off too.
    let mut older = Config::default();
    older.ui.show_welcome = false;
    assert_eq!(older.ui.whats_new_seen, "");
    assert_eq!(startup_route(false, false, None, &older), Startup::Welcome);
    let fresh = tempfile::tempdir().expect("a data dir");
    let config = Config::load_in(fresh.path());
    let mut ui = config.ui.clone();
    let (w, changed) = welcome::open_at_startup(&mut ui, &[]);
    assert_eq!((w.tab, changed), (WelcomeTab::Start, false));
}

/// What a person made in 0.5.0 comes through the first launch of this build:
/// nothing they chose moves, and no section loses a field.
#[test]
fn aurel_upgrader_keeps_their_config() {
    let mut old = golden("Gaban");
    old["ui"]["scale"] = json!(125);
    old["ui"]["show_welcome"] = json!(false);
    let list = old["keybinds"].as_array_mut().expect("a list");
    let row = list.iter_mut().find(|row| row["action"] == "BrushTool").expect("a row");
    row["chord"] = json!({ "keys": ["Y"], "ctrl": false, "shift": false, "alt": false });
    let before = old.clone();
    let (dir, mut config) = loaded(&old);
    let (_, changed) = welcome::open_at_startup(&mut config.ui, &[]);
    assert!(changed);
    config.save_to(dir.path()).expect("saved");
    let bytes = std::fs::read(dir.path().join("config.json")).expect("the file");
    let file: Value = serde_json::from_slice(&bytes).expect("json");
    let back = Config::load_in(dir.path());
    assert_eq!(back.chord_for(Action::BrushTool), Some(&chord("Y")), "the artist's chord stays");
    assert_eq!(back.ui.scale, 125);
    assert!(!back.ui.show_welcome && !back.ui.lock_positions, "an upgrader keeps their unlocked panels");
    assert_eq!(back.ui.quick_start, [true, false, false, false, false]);
    assert_eq!(back.render.ffmpeg, "C:/tools/ffmpeg.exe");
    assert_eq!(file["render"]["out_dir"], "D:/renders");
    assert_eq!(file["keymap"], before["keymap"]);
    for key in before["ui"].as_object().expect("an object").keys() {
        assert!(file["ui"].get(key).is_some(), "{key} left the ui section");
    }
    assert_eq!(file["ui"]["whats_new_seen"], welcome::VERSION, "the one word this launch changes");
    assert!(file.get("session").is_none_or(|s| s["api_key"] == "" && s["totp_secret"] == ""));
    // The wire floor did not move with the version.
    assert_eq!(crate::net::MIN_PEER_VERSION, "0.5.0");
    // The migration ran once: the second load moves nothing.
    let again = Config::load_in(dir.path());
    assert_eq!(again.keybinds, back.keybinds);
}

/// The laws section 1.3 retired, as comments would still state them. A word
/// here is one the new look took off the screen, so a comment naming it reads
/// as today's rule when it is not. Colour tokens that stay (`Ao`, `Aka`) and
/// `Tally` the struct in `measure.rs` and `strip.rs` are code, not comments.
#[test]
fn aurel_no_stale_room_law_in_comments() {
    const LAWS: &[&str] = &[
        "THE RULE", "charter", "charters", "Charter", "Slate", "slate", "Tally", "Struck", "Graphite", "engraved",
        "Engraved", "ENGRAVED", "plate look", "two inks", "Legend",
    ];
    let word_at = |hay: &str, word: &str| {
        let inside = |c: char| c.is_alphanumeric() || c == '_';
        hay.match_indices(word).any(|(at, _)| {
            !hay[..at].chars().next_back().is_some_and(inside)
                && !hay[at + word.len()..].chars().next().is_some_and(inside)
        })
    };
    let mut hits = Vec::new();
    let forge = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/iconforge/src/main.rs");
    let forge = std::fs::read_to_string(forge).expect("the forge");
    let mut sources = crate::ausay_tests::sources();
    sources.push(("tools/iconforge".to_string(), forge));
    for (name, text) in sources {
        for (at, line) in text.lines().enumerate() {
            let Some((_, comment)) = line.split_once("//") else { continue };
            for law in LAWS {
                if word_at(comment, law) {
                    hits.push(format!("{name}:{}: {law}: {}", at + 1, line.trim()));
                }
            }
        }
    }
    assert!(hits.is_empty(), "{} stale laws in comments:\n{}", hits.len(), hits.join("\n"));
    // The two the spec names by content.
    let templates = include_str!("templates.rs");
    assert!(!templates.contains("72- or 120-frame cuts"));
}

/// What a job makes, in a newcomer's words: a row of `CUT01, PNG 8-bit` was the
/// queue's name for a choice the panel calls something else.
#[test]
fn aurel_running_row_says_what_a_job_makes() {
    use crate::render_queue::{ExportKind, export_job};
    let mut state = ball(3);
    let dir = tempfile::tempdir().expect("a folder");
    let file = dir.path().join("ball.mp4");
    for (kind, dest, words) in
        [(ExportKind::Png, dir.path(), "a picture sequence"), (ExportKind::Mp4, file.as_path(), "an MP4 video")]
    {
        let i = export_job(&mut state, kind, dest, None).expect("a job");
        state.render_queue.queue.jobs[i].asked = Some(kind);
        let name = job_name(&state.render_queue.queue.jobs[i]);
        assert!(name.ends_with(&format!(", {words}")), "{name}");
        assert!(!name.contains("8-bit") && !name.contains("H.264"), "{name}");
    }
    // One set up in the queue keeps the preset its maker named.
    let i = export_job(&mut state, ExportKind::Png, dir.path(), None).expect("a job");
    assert!(job_name(&state.render_queue.queue.jobs[i]).ends_with(", PNG 8-bit"));
}

/// An OS failure reaches the row as something a person can act on, and a
/// reason of our own stands. No number survives.
#[test]
fn aurel_failure_row_has_no_os_words() {
    use crate::render_queue::plain_why;
    for raw in ["Access is denied. (os error 5)", "write a.gif: Permission denied (os error 13)"] {
        assert!(!plain_why(raw).contains("os error"), "{raw}");
    }
    assert!(plain_why("Access is denied. (os error 5)").contains("cannot be written"));
    assert!(plain_why("The process cannot access the file. (os error 32)").contains("using the file"));
    assert_eq!(plain_why("There is not enough space on the disk. (os error 112)"), "the disk is full");
    assert_eq!(plain_why("The system cannot find the path specified. (os error 3)"), "that folder is not there any more");
    assert_eq!(plain_why("The device is not ready. (os error 21)"), "The device is not ready.");
    assert_eq!(plain_why("disk full"), "disk full");
}

/// `Exported ball.gif.` stands until something else happens, not until a stroke
/// happens: a fill or a play overtakes it too, so the next hint gets its row.
#[test]
fn aurel_export_report_gives_way_to_work() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut h = shipped(ball(3));
    assert!(matches!(invoke(&mut h, Action::ExportGif), crate::Performed::App(_)));
    h.answer_export(&dir.path().join("ball.gif"));
    h.run_export_to_end();
    settle(&mut h);
    quiet(&mut h);
    assert_eq!(lane(&h).as_deref(), Some("Exported ball.gif."));
    assert!(h.perform(Action::PlayPause) == crate::Performed::Done);
    settle(&mut h);
    assert!(h.editor().say.exported.is_none(), "{:?}", lane(&h));
    assert_ne!(lane(&h).as_deref(), Some("Exported ball.gif."));
}

/// Spec 2.7: every icon has its 96 px cut beside the 24 and the 48, square, so
/// a mask stays sharp on any display the app scales to.
#[test]
fn aurel_every_icon_has_a_96_cut() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
    let svgs = std::fs::read_dir(root.join("svg")).expect("the masks").flatten();
    let names: Vec<String> = svgs
        .filter(|f| f.path().extension().is_some_and(|x| x == "svg"))
        .map(|f| f.path().file_stem().expect("a stem").to_string_lossy().to_string())
        .collect();
    assert!(names.len() >= 24, "{names:?}");
    for name in names {
        for size in [24, 48, 96] {
            let path = root.join("png").join(format!("{name}_{size}.png"));
            let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("no {}", path.display()));
            let reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().expect("a png");
            let info = reader.info();
            assert_eq!((info.width, info.height), (size, size), "{}", path.display());
        }
    }
    // Under the old 28 the cut stands, so no key at 100% changes a pixel.
    assert_eq!(
        [22.0, 28.0, 28.5, 44.0, 56.0, 57.0, 88.0].map(crate::icons::cut_for),
        [24, 24, 48, 48, 48, 96, 96]
    );
}
