//! AU17: Release. What's New, what an upgrader keeps, and the sweep of the
//! comments that outlived the look. Goldens are `aukeys_tests`' hand-built
//! v0.5.0 shapes: no `api_key`, no `totp_secret`.

use eframe::egui::{self, Rect};
use serde_json::{Value, json};

use crate::auexport_tests::{ball, enter, invoke, lane, quiet, shipped};
use crate::aukeys_tests::{chord, golden, loaded, upgraders_dir};
use crate::config::{Action, Chord, Config};
use crate::export_step::job_name;
use crate::testkit::EditorHarness;
use crate::welcome::{self, WelcomeTab};
use crate::workspace::Workspaces;
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
    let line = "Keys moved: Overlay track was Alt+K, now Alt+O; Background track was Alt+B, now Alt+W.";
    assert!(h.has(line), "{}", h.describe());
    assert_eq!(h.rig().config.chord_for(Action::ColumnKindBook).map(Chord::label).as_deref(), Some("Alt+O"));
    // A rebound key reads as it stands, the frame it changes.
    h.rig_mut().config.binding_mut(Action::ColumnKindBook).chord = Some(chord("Alt+L"));
    settle(&mut h);
    let line = "Keys moved: Overlay track was Alt+K, now Alt+L; Background track was Alt+B, now Alt+W.";
    assert!(h.has(line), "{}", h.describe());
    // A key taken off says so, and never offers the old key as live.
    let row = h.rig_mut().config.binding_mut(Action::ColumnKindBg);
    (row.chord, row.also) = (None, Vec::new());
    settle(&mut h);
    let line = "Keys moved: Overlay track was Alt+K, now Alt+L; Background track was Alt+B, now no key.";
    assert!(h.has(line), "{}", h.describe());
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
    assert!(h.has_text_containing("Overlay track was Alt+K, now Alt+P"), "{}", h.describe());
    // A key that stayed where it was is no move.
    h.rig_mut().config.binding_mut(Action::ColumnKindBook).chord = Some(chord("Alt+K"));
    settle(&mut h);
    assert!(h.has_text_containing("Overlay track stays on Alt+K"), "{}", h.describe());
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
    // Each kind's first step, and the three in the one line.
    for (kind, word) in [(ProjectKind::Animation, "An animation opens"), (ProjectKind::Drawing, "a drawing"), (ProjectKind::Comic, "a comic")] {
        let opening = format!("{word} on {}", bar(kind)[0].label);
        assert!(welcome::WHATS_NEW_BARS.contains(&opening), "{opening}: {}", welcome::WHATS_NEW_BARS);
    }
    assert!(welcome::WHATS_NEW_BARS.contains("with its sheet"));
}

/// The page is one screen: no scroll at the cap of every gated size, nothing
/// clipped or elided, every old word beside its new one.
#[test]
fn aurel_whats_new_is_one_screen() {
    let sizes = [(1600.0, 900.0, 1.0), (1280.0, 800.0, 1.0), (1024.0, 640.0, 1.0), (832.0, 520.0, 1.0)];
    // A stored scale on the smallest windows the app opens: the points shrink
    // while the type does not, down to the 760×520 of room the scale leaves.
    let scaled = [1.25, 1.5, 2.0].into_iter().flat_map(|scale| {
        [(1024.0, 640.0), (1024.0, 768.0), (1280.0, 720.0)].map(|(w, tall)| (w, tall, scale))
    });
    for (w, tall, stored) in sizes.into_iter().chain(scaled) {
        let ppp = crate::effective_scale(stored, w, tall);
        let mut h = upgraded(w, tall);
        h.kit_mut().set_size(egui::vec2(w / ppp, tall / ppp));
        h.kit_mut().set_pixels_per_point(ppp);
        settle(&mut h);
        let card = float_rect(&h);
        let last = vocab::OLD_WORDS.last().expect("a row");
        // The tree's rects are the screen's pixels, the card's its points.
        let foot = h.rect(&format!("{} → {}", last.0, last.1)) / ppp;
        // The body's scroll ends 8 pt above the card's edge, and a last row
        // that reaches it is cut through its descenders.
        assert!(foot.bottom() <= card.bottom() - 12.0, "{w} at {ppp}: {foot:?} under {card:?}");
        // One line on the bars and one on the lessons (feedback 1.11).
        for line in [welcome::WHATS_NEW_BARS, welcome::WHATS_NEW_LESSONS] {
            assert!(h.rect(line).height() / ppp <= 22.0, "{w} at {ppp}: {line} wraps");
        }
        let mut rows = Vec::new();
        for (old, new) in vocab::OLD_WORDS {
            let word = format!("{old} → {new}");
            assert!(h.has(&word), "{old}\n{}", h.describe());
            let rect = h.rect(&word) / ppp;
            assert!(rect.right() <= card.right() - 8.0, "{w} at {ppp}: {word} leaves the card: {rect:?} {card:?}");
            rows.push(rect);
        }
        // The columns keep the card's gap: no word runs into its neighbour.
        for pair in rows.chunks(welcome::WORD_COLUMNS) {
            for (left, right) in pair.iter().zip(pair.iter().skip(1)) {
                assert!(right.left() - left.right() >= crate::plate::CARD_PAD - 0.5, "{w} at {ppp}: {left:?} {right:?}");
            }
        }
        let clip = crate::measure::clip_here(&h, crate::measure::Scope::All);
        assert_eq!((clip.clipped, clip.elided), (0, 0), "{w} at {ppp}: {:?}", clip.found);
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
/// The same goes for a brush's grain and the Aka pencil lane: those are the
/// tool's words, so the plate's grain and "never Aka" are phrases here.
#[test]
fn aurel_no_stale_room_law_in_comments() {
    const LAWS: &[&str] = &[
        "THE RULE", "charter", "charters", "Charter", "Slate", "slate", "Tally", "Struck", "Graphite", "engraved",
        "Engraved", "ENGRAVED", "plate look", "two inks", "Legend", "plate law", "plate's law", "plate fact",
        "plate's grain", "plate grain", "owner's amendment", "never Aka",
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
/// reason of our own stands. No number survives, no path, and a failure that
/// is not about the folder does not blame the folder.
#[test]
fn aurel_failure_row_has_no_os_words() {
    use crate::render_queue::plain_why;
    let denied = "this folder cannot be written to. Pick another, or close what is using the file";
    let gone = "that folder is not there any more. Pick another place";
    let taken = "something with that name is already in the folder. Pick another place";
    let cases = [
        // The two heads a place's failure carries, the app's and the core's.
        ("create C:\\out\\f: Access is denied. (os error 5)", denied),
        ("write C:\\out\\a.gif: Access is denied. (os error 5)", denied),
        ("write a.gif: The process cannot access the file. (os error 32)", denied),
        ("create C:\\out: The system cannot find the path specified. (os error 3)", gone),
        ("create C:\\out: The system cannot find the file specified. (os error 2)", gone),
        ("create C:\\out\\f: Cannot create a file when that file already exists. (os error 183)", taken),
        ("write C:\\out\\a.png: The file exists. (os error 80)", taken),
        ("write C:\\out\\a.png: There is not enough space on the disk. (os error 112)", "the disk is full"),
        // An unknown number on a place keeps the sentence and drops the place.
        ("create C:\\out\\f: The device is not ready. (os error 21)", "The device is not ready."),
        ("write C:\\out\\a.png: Unknown: strange (os error 1117)", "strange"),
        // What is not a place says what it was doing.
        ("running ffmpeg: The system cannot find the file specified. (os error 2)", "running ffmpeg: The system cannot find the file specified."),
        ("temp dir: Access is denied. (os error 5)", "temp dir: Access is denied."),
        ("audio temp: The system cannot find the path specified. (os error 3)", "audio temp: The system cannot find the path specified."),
        ("ffmpeg stopped reading: The pipe has been ended. (os error 109)", "ffmpeg stopped reading: The pipe has been ended."),
        // The full disk is the disk's, whichever operation found it.
        ("temp dir: There is not enough space on the disk. (os error 112)", "the disk is full"),
        // Bare, and ours.
        ("The device is not ready. (os error 21)", "The device is not ready."),
        ("disk full", "disk full"),
        ("", ""),
        (" (os error 5)", ""),
    ];
    for (raw, plain) in cases {
        let said = plain_why(raw);
        assert_eq!(said, plain, "{raw}");
        assert!(!said.contains("os error") && !said.contains(":\\"), "{raw}: {said}");
    }
    // What the OS never prints still comes back, and never panics.
    for odd in ["(os error", " (os error ", "create ", "write : (os error )", "(os error 5)", "：: (os error 183)"] {
        let _ = plain_why(odd);
    }
}

/// The failure as it comes off the worker, in every place it lands: the row
/// under the panel's button, the refusal on the lane, the queue's own row, the
/// job's stored state. The whole sentence each time.
#[test]
fn aurel_failure_is_plain_wherever_it_lands() {
    use crate::export_step::{TRY_AGAIN, TRY_AGAIN_CARD};
    use crate::render_queue::{ExportKind, JobState, Progress, held_job};
    let raw = "create C:\\Users\\me\\out\\ball: Cannot create a file when that file already exists. (os error 183)";
    let plain = "something with that name is already in the folder. Pick another place";
    let dir = tempfile::tempdir().expect("a folder");
    let mut h = shipped(ball(3));
    enter(&mut h);
    let tx = held_job(h.state_mut(), &dir.path().join("ball.gif"), 1);
    h.state_mut().render_queue.queue.jobs.last_mut().expect("the job").asked = Some(ExportKind::Gif);
    settle(&mut h);
    tx.send(Progress::Done(Err(raw.to_string()), None)).expect("the app holds the job");
    settle(&mut h);
    let job = h.state().render_queue.queue.jobs.last().expect("the job");
    assert_eq!(job.state, JobState::Failed(plain.to_string()));
    let lane = h.state().refusal.clone();
    assert!(lane.ends_with(&format!(" did not finish — {plain}")), "{lane}");
    let failed = h.state().render_queue.panel.failed.clone().expect("the row");
    assert_eq!(failed.why, plain);
    let panel = h.painted_text().join(" ");
    assert!(panel.contains(&format!("That export did not finish: {plain}.")), "{panel}");
    assert!(h.has(TRY_AGAIN));
    assert!(TRY_AGAIN_CARD.contains("pick another place"), "the way to another folder is said");
    h.click("More options");
    // Tall enough for the queue's list under the form.
    h.kit_mut().set_size(egui::vec2(1600.0, 2400.0));
    settle(&mut h);
    let open = h.painted_text().join(" ");
    assert!(open.contains(&format!("failed: {plain}")), "the queue's own row: {open}");
    for word in ["os error", "C:\\", "already exists"] {
        assert!(!open.contains(word) && !lane.contains(word), "{word}: {open}");
    }
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

/// A job that lands while the shot plays still reports: only pressing play
/// again overtakes it, not the playing that was already going.
#[test]
fn aurel_export_report_lands_during_play() {
    let dir = tempfile::tempdir().expect("a folder");
    let mut h = shipped(ball(3));
    assert!(h.perform(Action::PlayPause) == crate::Performed::Done);
    settle(&mut h);
    assert!(h.state().view.playing, "the shot plays");
    assert!(matches!(invoke(&mut h, Action::ExportGif), crate::Performed::App(_)));
    h.answer_export(&dir.path().join("ball.gif"));
    h.run_export_to_end();
    settle(&mut h);
    quiet(&mut h);
    assert!(h.state().view.playing, "it still plays");
    assert!(h.editor().say.exported.is_some(), "the report landed");
    assert_eq!(lane(&h).as_deref(), Some("Exported ball.gif."));
    // Stopping is not a press of play; a new one is.
    assert!(h.perform(Action::PlayPause) == crate::Performed::Done);
    settle(&mut h);
    assert_eq!(lane(&h).as_deref(), Some("Exported ball.gif."), "stopping overtakes nothing");
    assert!(h.perform(Action::PlayPause) == crate::Performed::Done);
    settle(&mut h);
    assert!(h.editor().say.exported.is_none(), "{:?}", lane(&h));
}

/// A key that moved is news: it never rings as a refusal, it holds as long as
/// a notice does, and it gives way to the first thing the artist does. A fault
/// on the same launch keeps the red row and carries the keys with it.
#[test]
fn aurel_moved_keys_are_news_not_a_refusal() {
    use crate::aukeys_tests::KEYS_MOVED;
    let mut h = EditorHarness::with_dir_shipped(upgraders_dir());
    h.kit_mut().set_size(egui::vec2(1600.0, 900.0));
    settle(&mut h);
    assert_eq!(h.state().status, KEYS_MOVED);
    assert_eq!((h.state().refusal.as_str(), h.state().refusal_seq), ("", 0), "no refusal rang");
    h.wait(60.0);
    assert_eq!(h.state().status, KEYS_MOVED, "a minute later, still there");
    assert!(h.painted_text().iter().any(|t| t == KEYS_MOVED));
    h.key(egui::Key::Space);
    settle(&mut h);
    h.wait(5.0);
    assert_ne!(h.state().status, KEYS_MOVED, "it dwells, then goes");
    // With a fault beside it.
    let dir = upgraders_dir();
    std::fs::write(Workspaces::path_in(dir.path()), br#"{ "list": [ { "name": "broken", "dock": 7 } ] }"#)
        .expect("a broken arrangement");
    let mut h = EditorHarness::with_dir_shipped(dir);
    settle(&mut h);
    let said = h.state().refusal.clone();
    assert!(said.contains("saved arrangement") && said.contains(KEYS_MOVED), "{said}");
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
    // The app's own table, not only the folder: a cut on disk that no table
    // names falls back to 48 and blurs. Every icon, at every cut.
    let ctx = egui::Context::default();
    for icon in crate::icons::every_mask() {
        for size in [24.0, 48.0, 96.0] {
            let texture = crate::icons::mask_texture(&ctx, icon, size).expect("a mask");
            assert_eq!(texture.size(), [size as usize; 2], "{icon:?} at {size}");
        }
    }
}

/// What a 0.5.0 peer reads off the wire, and a 0.5.0 build off the disk. The
/// three files stand letter for letter as the `v0.5.0` tag has them, by digest
/// of `git show v0.5.0:<file>`; the host's layout is the shape it always was,
/// with no step in it. A live session against a 0.5.0 build is on the list.
#[test]
fn aurel_wire_is_what_050_speaks() {
    let digest = |text: &str| {
        let bytes = text.bytes().filter(|b| *b != b'\r');
        bytes.fold(0xcbf29ce484222325, |h: u64, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
    };
    let net = include_str!("net.rs");
    // BF3c (Lead 2026-11-15): `Dab::cap` rides the wire; moved from 0x8c3bb56c813db4e1.
    assert_eq!(digest(net), 0xf3d4bbbcc4247f2a, "net.rs left v0.5.0");
    assert_eq!(digest(include_str!("../../crates/anim-core/src/model.rs")), 0x0b24d09e9839c24f, "model.rs left v0.5.0");
    assert_eq!(digest(include_str!("../../crates/anim-core/src/store.rs")), 0xff09ba1fd4cf2794, "store.rs left v0.5.0");
    assert_eq!(crate::net::MIN_PEER_VERSION, "0.5.0");
    let dock = include_str!("../Cargo.toml").lines().find(|l| l.starts_with("egui_dock = ")).expect("the dock");
    assert!(dock.contains("\"=0.20.1\""), "the saved dock's shape is this version's: {dock}");
    assert!(!net.contains("Stage::") && !net.contains("stage_key"), "no step rides the wire");
    let mut h = shipped(ball(3));
    enter(&mut h);
    let layout = h.editor().host_layout().expect("a layout to send");
    let sent: Value = serde_json::from_str(&layout).expect("json");
    let keys: Vec<&str> = sent.as_object().expect("an object").keys().map(String::as_str).collect();
    assert!(keys.iter().all(|key| ["name", "dock", "preset", "view"].contains(key)), "{keys:?}");
    assert!(sent["preset"].is_null(), "the Export step names no stack");
    assert!(!layout.contains("\"export\""), "the Export key stays home: {layout}");
    // The bytes a 0.5.0 peer loads.
    serde_json::from_str::<crate::workspace::Workspace>(&layout).expect("it loads");
}

/// The two old design documents are history, and say so on their first line.
#[test]
fn aurel_old_specs_point_at_this_one() {
    for text in [include_str!("../../research/DESIGN-SPEC-editor-ui.md"), include_str!("../../research/ROOM-CHARTERS.md")] {
        let first = text.lines().next().expect("a first line");
        assert!(first.starts_with("> History: the app's "), "{first}");
        assert!(first.contains("follow `research/PSD-approachable-ui.md`"), "{first}");
        assert!(first.ends_with("that document wins."), "{first}");
    }
}

/// The body of `fn <name>(` in `text`, to the line that closes it.
fn body_of<'a>(text: &'a str, name: &str) -> &'a str {
    let at = text.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no {name}"));
    let end = text[at..].find("\n}").unwrap_or_else(|| panic!("{name} never closes"));
    &text[at..at + end]
}

/// The notes under the spec's final tally.
fn tally_notes() -> &'static str {
    let spec = include_str!("../../research/PSD-approachable-ui.md");
    let notes = spec.split("**AU17 notes: the final tally.**").nth(1).expect("the tally");
    notes.split("\n---").next().expect("its end")
}

/// A journey reads more words than it did and the limits on them did not move,
/// the floors only rose, and each test this part moved says why.
#[test]
fn aurel_moves_kept_their_teeth() {
    let export = include_str!("auexport_tests.rs");
    let read = body_of(export, "read_lane");
    for source in ["say.hint", "h.state().status", "h.painted_text()"] {
        assert!(read.contains(source), "the lane reader lost {source}: {read}");
    }
    let limit = |text: &str, name: &str, most: &str| {
        let line = format!("assert!(jargon.len() <= {most},");
        assert!(body_of(text, name).contains(&line), "{name}: {line}");
    };
    limit(export, "auexport_ball_journey", "2");
    limit(export, "auexport_still_journey", "1");
    limit(include_str!("aucolor_tests.rs"), "aucolor_fill_journey", "1");
    limit(include_str!("aufx_tests.rs"), "aufx_glow_and_pan_journey", "2");
    let kit = include_str!("testkit.rs");
    let floor = |name: &str| -> usize {
        let head = format!("const {name}: usize = ");
        let line = kit.lines().find_map(|l| l.trim().strip_prefix(head.as_str()).map(str::to_string));
        line.expect("the floor").trim_end_matches(';').parse().expect("a number")
    };
    assert!(floor("TEST_FLOOR") > 1369 && floor("ASSERT_FLOOR") > 8078, "the floors the review saw only rise");
    let moves = tally_notes().split("Test moves.").nth(1).expect("the moves");
    let moved = [
        ("label", "ship_whats_new_is_one_page_and_opens_first"),
        ("label", "aurel_whats_new_names_moved_chords_live"),
        ("label", "austart_"),
        ("reach", "aukeys_chords_that_moved_fit_the_lane"),
        ("reach", "aukeys_"),
        ("recipe", "ship_readme_covers_the_release"),
        ("recipe", "audress_the_ignored_pass_owes_every_shot"),
    ];
    for (reason, test) in moved {
        let says = moves.lines().any(|line| line.contains(test) && line.contains(&format!("({reason})")));
        assert!(says, "{test} moved for {reason}, and the notes do not say so");
    }
}

/// The release's new paths at the corners of what a person can open: the
/// smallest room, a big scale, a 4K window. Nothing here may panic.
#[test]
fn aurel_new_paths_stand_odd_windows() {
    for (w, tall, stored) in [(760.0, 520.0, 1.0), (1024.0, 640.0, 3.0), (3840.0, 2160.0, 2.0)] {
        let ppp = crate::effective_scale(stored, w, tall);
        let mut h = upgraded(w / ppp, tall / ppp);
        h.kit_mut().set_pixels_per_point(ppp);
        settle(&mut h);
        assert_eq!(h.editor().start.as_ref().map(|w| w.tab), Some(WelcomeTab::WhatsNew));
        h.wait(30.0);
    }
}

/// News lives in memory: a moved key's sentence reaches no file, the stored
/// words are the words they were, and a stroke over the held notice lands and
/// undoes as any stroke does.
#[test]
fn aurel_news_reaches_no_file() {
    use crate::aukeys_tests::KEYS_MOVED;
    use crate::workspace::Stage;
    let (dir, config) = loaded(&golden("Gaban"));
    assert_eq!(config.keymap_notes.len(), 2);
    config.save_to(dir.path()).expect("saved");
    let bytes = std::fs::read_to_string(dir.path().join("config.json")).expect("the file");
    assert!(!bytes.contains("keymap_notes") && !bytes.contains("moved from"), "{bytes}");
    let keys = Stage::ALL.iter().map(|room| crate::stage_key(Some(*room))).collect::<Vec<_>>();
    assert_eq!(keys, ["layout", "drawing", "finishing", "composite", "edit"]);
    let mut h = EditorHarness::with_dir_shipped(upgraders_dir());
    settle(&mut h);
    assert_eq!(h.state().status, KEYS_MOVED);
    h.kit_mut().set_size(egui::vec2(1600.0, 900.0));
    settle(&mut h);
    let before = (h.state().strokes_done, h.state().engine.revision());
    crate::ausay_tests::line(&mut h);
    assert_eq!(h.state().strokes_done, before.0 + 1, "the stroke landed under the notice");
    assert_ne!(h.state().engine.revision(), before.1);
    assert!(h.perform(Action::Undo) == crate::Performed::Done);
    settle(&mut h);
    assert_eq!(h.state().engine.revision(), before.1, "and undoes whole");
}

/// The rows of the Markdown table under `head`: each row's cells, the header
/// and its rule left out.
fn table<'a>(text: &'a str, head: &str) -> Vec<Vec<&'a str>> {
    let rest = text.split_once(head).unwrap_or_else(|| panic!("no {head}")).1;
    let lines = rest.lines().skip_while(|l| !l.starts_with('|'));
    let rows = lines.take_while(|l| l.starts_with('|')).skip(2);
    rows.map(|l| l.trim().trim_matches('|').split(" | ").map(str::trim).collect()).collect()
}

/// Spec 10 and feedback 1.10, row by row: every gate has today's number, its
/// target and what was measured, and one with nothing measured is a named miss
/// and not a tick. The three named misses stay named, with their numbers.
#[test]
fn aurel_tally_answers_every_gate() {
    let spec = include_str!("../../research/PSD-approachable-ui.md");
    let feedback = include_str!("../../research/PSD-approachable-ui-feedback.md");
    let notes = tally_notes();
    let rows = table(notes, "\n");
    assert!(rows.len() >= 100, "{}", rows.len());
    let ledger = table(spec.split_once("## 10 Gate ledger").expect("the ledger").1, "\n");
    let moved = table(feedback.split_once("### 1.10 Gate ledger").expect("feedback").1, "**No threshold loosens.**");
    let new = table(feedback.split_once("### 1.10 Gate ledger").expect("feedback").1, "New rows:");
    assert_eq!((ledger.len(), moved.len(), new.len()), (90, 4, 10), "the ledgers this tally answers");
    for gate in ledger.iter().chain(&moved).chain(&new) {
        assert!(rows.iter().any(|row| row[0] == gate[0]), "no tally row for {:?}", gate[0]);
    }
    for row in &rows {
        assert_eq!(row.len(), 4, "{row:?}");
        let (gate, today, target, measured) = (row[0], row[1], row[2], row[3]);
        assert!(!today.is_empty() && !target.is_empty(), "{gate}");
        let counted = measured.chars().any(|c| c.is_ascii_digit());
        assert!(counted || measured.starts_with("**Miss"), "{gate}: nothing measured: {measured}");
        assert!(!measured.to_lowercase().contains("green"), "{gate}: {measured}");
    }
    let miss = |gate: &str, words: &[&str]| {
        let row = rows.iter().find(|row| row[0].starts_with(gate)).unwrap_or_else(|| panic!("no {gate}"));
        assert!(row[3].starts_with("**Miss"), "{gate} is a named miss: {}", row[3]);
        for word in words {
            assert!(row[3].contains(word), "{gate}: {word} in {}", row[3]);
        }
    };
    miss("MP4 with nothing installed", &["not met"]);
    miss("Idle repaint", &["not measured"]);
    // AU12's first reading, 9 px and 12.9%, is history: the gate reads today's.
    let reads = |gate: &str, word: &str| {
        let row = rows.iter().find(|row| row[0] == gate).unwrap_or_else(|| panic!("no {gate}"));
        assert!(row[3].contains(word), "{gate}: {word} in {}", row[3]);
    };
    reads("Painted text median", "10 px");
    reads("Painted text at ≤8 px", "8.5%");
}

/// The list the Lead gets, numbered without a gap, through this part's items.
#[test]
fn aurel_list_is_handed_over() {
    let spec = include_str!("../../research/PSD-approachable-ui.md");
    let list = spec.split("## 13 Batched list for the Lead").nth(1).expect("the list");
    let list = list.split("\n---").next().expect("its end");
    let items: Vec<&str> = list.lines().filter(|l| l.starts_with(|c: char| c.is_ascii_digit())).collect();
    assert!(items.len() >= 29, "{}", items.len());
    for (i, item) in items.iter().enumerate() {
        assert!(item.starts_with(&format!("{}. ", i + 1)), "item {}: {item}", i + 1);
    }
    let said = |n: usize, word: &str| assert!(items[n - 1].contains(word), "{n}: {}", items[n - 1]);
    said(28, "painted text");
    said(29, "0.5.0");
}
