//! Grab: hover any element and press Ctrl+Alt+G. The element under the
//! pointer, the panels it sits in, the source lines that draw it and the
//! app's state go to `<exe dir>/ui_dump/pick.md`, so development starts from
//! the exact spot instead of a search.
//!
//! A READ, like the UI dump: nothing here changes the document. The key is a
//! keyboard press, not a click, so the grabbed button never fires.
//!
//! Labels come from egui's AccessKit tree, which egui only builds in frames
//! after it is switched on: the press arms it, the next frame reads it.

use eframe::egui;
use std::path::{Path, PathBuf};

/// One widget under the pointer.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub label: String,
    pub role: String,
    pub rect: egui::Rect,
}

#[derive(Default)]
pub struct Pick {
    /// Armed by the key; read at the end of the next frame.
    armed: bool,
    /// The grab switched AccessKit on, so it switches it off again.
    switched_on: bool,
    pub last: Option<String>,
    /// What was grabbed, outlined on screen for a moment so the grab is seen.
    pub flash: Option<(egui::Rect, String, std::time::Instant)>,
}

/// How long the grabbed outline stays up.
const FLASH: std::time::Duration = std::time::Duration::from_millis(1500);

impl Pick {
    /// The grab key, Ctrl+Alt+G: no F-row needed (60% keyboards), and no
    /// binding uses Ctrl+Alt (bindings match modifiers exactly, so G, Shift+G
    /// and Alt+G keep theirs).
    pub fn pressed(ctx: &egui::Context) -> bool {
        ctx.input(|i| {
            let m = i.modifiers;
            m.command && m.alt && !m.shift && i.key_pressed(egui::Key::G)
        })
    }

    /// The modifiers of the grab key, for tests.
    #[cfg(test)]
    pub const MODS: egui::Modifiers = egui::Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };

    pub fn arm(&mut self, ctx: &egui::Context) {
        if self.armed {
            return;
        }
        // On already (a test harness, a screen reader): leave it on after.
        let on = ctx.accesskit_node_builder(egui::Id::new("pick-probe"), |_| ()).is_some();
        if !on {
            ctx.enable_accesskit();
        }
        self.switched_on = !on;
        self.armed = true;
        ctx.request_repaint();
    }

    /// At the end of a frame: once armed and labels are built, what the
    /// pointer is over, smallest first. `None` while there is nothing to read.
    pub fn take(&mut self, ctx: &egui::Context) -> Option<Vec<Hit>> {
        if !self.armed {
            return None;
        }
        let on = ctx.accesskit_node_builder(egui::Id::new("pick-probe"), |_| ()).is_some();
        if !on {
            ctx.request_repaint();
            return None; // the frame after the press builds them
        }
        self.armed = false;
        if self.switched_on {
            ctx.disable_accesskit();
            self.switched_on = false;
        }
        let hits = hits(ctx);
        let at = ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
        self.flash = Some(match hits.first() {
            Some(h) => (h.rect, format!("Grabbed: {}", h.label), std::time::Instant::now()),
            None => (egui::Rect::from_center_size(at, egui::Vec2::splat(24.0)), "Nothing to grab here".into(), std::time::Instant::now()),
        });
        Some(hits)
    }

    /// The outline and its tag, over everything, until it expires.
    pub fn paint(&mut self, ctx: &egui::Context) {
        let Some((rect, words, at)) = &self.flash else { return };
        if at.elapsed() > FLASH {
            self.flash = None;
            return;
        }
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("pick-flash"));
        let p = ctx.layer_painter(layer);
        let mark = crate::plate::BLUE;
        p.rect_stroke(rect.expand(2.0), 3.0, egui::Stroke::new(2.5, mark), egui::StrokeKind::Outside);
        let font = crate::plate::Role::Chrome.font();
        let galley = p.layout_no_wrap(words.clone(), font, crate::plate::ON_FILL);
        let tag = egui::Rect::from_min_size(rect.left_top() - egui::vec2(2.0, galley.size().y + 10.0), galley.size() + egui::vec2(10.0, 6.0));
        let tag = tag.translate(egui::vec2(0.0, (ctx.content_rect().top() - tag.top()).max(0.0)));
        p.rect_filled(tag, 3.0, mark);
        p.galley(tag.min + egui::vec2(5.0, 3.0), galley, crate::plate::ON_FILL);
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// Every labelled widget that contains the pointer, smallest first.
pub fn hits(ctx: &egui::Context) -> Vec<Hit> {
    let ids = ctx.interaction_snapshot(|s| s.contains_pointer.clone());
    let mut out: Vec<Hit> = ids
        .into_iter()
        .filter_map(|id| {
            let rect = ctx.read_response(id)?.rect;
            let (label, role) = ctx.accesskit_node_builder(id, |n| {
                let label = n.label().or(n.value()).map(str::to_owned);
                (label, format!("{:?}", n.role()))
            })?;
            let label = label.filter(|l| !l.trim().is_empty())?;
            Some(Hit { label, role, rect })
        })
        .collect();
    out.sort_by(|a, b| a.rect.area().total_cmp(&b.rect.area()));
    out.dedup_by(|a, b| a.label == b.label);
    out
}

/// The source tree this build came from, when it is still on this machine.
pub fn src_root() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    p.is_dir().then_some(p)
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Tests name every label too; the drawing code is what is wanted.
            if !name.ends_with("_tests.rs") && name != "testkit.rs" {
                out.push(p);
            }
        }
    }
}

/// `file:line: code` for where `label` is written in the source: the quoted
/// text itself, the uses of a `const` that holds it, or, for a label built
/// with numbers (`One picture, 1280×720`), its words before the numbers.
pub fn sources(root: &Path, label: &str) -> Vec<String> {
    const CAP: usize = 6;
    let mut files = Vec::new();
    rs_files(root, &mut files);
    files.sort();
    let texts: Vec<(String, String)> = files
        .iter()
        .filter_map(|p| {
            let name = p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/");
            Some((name, std::fs::read_to_string(p).ok()?))
        })
        .collect();
    let find = |needle: &str, out: &mut Vec<String>, skip: Option<&str>| {
        for (name, text) in &texts {
            for (i, line) in text.lines().enumerate() {
                if out.len() >= CAP {
                    return;
                }
                if line.contains(needle) && skip.is_none_or(|s| !line.contains(s)) {
                    out.push(format!("{name}:{}: {}", i + 1, line.trim()));
                }
            }
        }
    };
    let mut out = Vec::new();
    find(&format!("\"{label}\""), &mut out, None);
    if out.is_empty() {
        let head: String = label.chars().take_while(|c| !c.is_ascii_digit() && *c != '{').collect();
        let head = head.trim_end_matches([' ', ',', ':', '·']).trim();
        if head.split_whitespace().count() >= 2 {
            find(&format!("\"{head}"), &mut out, None);
        }
    }
    // A const holding the label: where it is used is where it is drawn.
    let consts: Vec<String> = out
        .iter()
        .filter_map(|hit| {
            let code = hit.splitn(3, ':').nth(2)?;
            let rest = code.split("const ").nth(1)?;
            Some(rest.split(':').next()?.trim().to_owned())
        })
        .collect();
    for name in consts {
        let mut uses = Vec::new();
        find(&name, &mut uses, Some("const "));
        for u in uses {
            if out.len() < CAP + 4 && !out.contains(&u) {
                out.push(u);
            }
        }
    }
    out
}

/// The record: the grabbed element, what holds it, where its code is, and
/// the app's own description of what it shows.
pub fn render(hits: &[Hit], src: &[String], where_: &str, state: &str) -> String {
    let mut s = String::from("# Pick\n\n");
    match hits.first() {
        None => s.push_str("Nothing labelled under the pointer.\n"),
        Some(h) => {
            let r = h.rect;
            s.push_str(&format!(
                "element: \"{}\" ({}) at x {:.0} y {:.0}, {:.0}×{:.0} pt\n",
                h.label,
                h.role,
                r.left(),
                r.top(),
                r.width(),
                r.height()
            ));
            let inside: Vec<String> = hits[1..].iter().take(4).map(|h| format!("\"{}\"", h.label)).collect();
            if !inside.is_empty() {
                s.push_str(&format!("inside: {}\n", inside.join(" < ")));
            }
        }
    }
    if !where_.is_empty() {
        s.push_str(&format!("where: {where_}\n"));
    }
    s.push_str("\n## Source (app/src)\n");
    if src.is_empty() {
        s.push_str("not found\n");
    }
    for line in src {
        s.push_str(&format!("{line}\n"));
    }
    s.push_str("\n## State\n");
    s.push_str(state.trim_end());
    s.push('\n');
    s
}

/// Write `ui_dump/pick.md` (each grab replaces the last); the status line says it.
pub fn write(record: &str) -> Result<PathBuf, String> {
    let dir = crate::uidump::dump_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("pick.md");
    std::fs::write(&path, record).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Hover each button, press the key, and the grab names that button
    /// and the frame around it; the button never fires.
    #[test]
    fn pick_grabs_the_element_under_the_pointer() {
        let words = ["Host a new drawing", "Join a session", "Back to Start", "Open the example", "Settings…"];
        for word in words {
            let got: Rc<RefCell<Option<Vec<Hit>>>> = Rc::default();
            let fired = Rc::new(RefCell::new(false));
            let (g, f) = (got.clone(), fired.clone());
            let mut pick = Pick::default();
            let mut harness = Harness::new_ui(move |ui| {
                let frame = egui::Frame::group(ui.style()).show(ui, |ui| {
                    for w in words {
                        if ui.button(w).clicked() {
                            *f.borrow_mut() = true;
                        }
                    }
                });
                let r = ui.interact(frame.response.rect, egui::Id::new("tray"), egui::Sense::hover());
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Tray"));
                if Pick::pressed(ui.ctx()) {
                    pick.arm(ui.ctx());
                }
                if let Some(h) = pick.take(ui.ctx()) {
                    *g.borrow_mut() = Some(h);
                }
            });
            harness.get_by_label(word).hover();
            harness.run();
            harness.key_press_modifiers(Pick::MODS, egui::Key::G);
            harness.run();
            let hits = got.borrow().clone().unwrap_or_else(|| panic!("{word}: nothing grabbed"));
            assert_eq!(hits.first().map(|h| h.label.as_str()), Some(word), "{word}: {hits:?}");
            assert!(hits.iter().any(|h| h.label == "Tray"), "{word}: the frame around it");
            assert!(!*fired.borrow(), "{word}: the grab is a key, not a click");
        }
    }

    /// The app runs with AccessKit off: the press switches it on, the next
    /// frame reads the labels, and it goes off again.
    #[test]
    fn pick_works_with_labels_off_and_leaves_them_off() {
        // A bare context: kittest refuses to run with AccessKit off.
        let ctx = egui::Context::default();
        let mut pick = Pick::default();
        let mut got = None;
        let mut at = egui::Pos2::ZERO;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        let frame = |events: Vec<egui::Event>, modifiers: egui::Modifiers, pick: &mut Pick| {
            let input = egui::RawInput { screen_rect: Some(screen), events, modifiers, ..Default::default() };
            let mut rect = egui::Rect::NOTHING;
            let mut hits = None;
            let _ = ctx.run_ui(input, |ui| {
                rect = ui.button("Join a session").rect;
                if Pick::pressed(ui.ctx()) {
                    pick.arm(ui.ctx());
                }
                hits = pick.take(ui.ctx());
            });
            (rect, hits)
        };
        let probe = |ctx: &egui::Context| ctx.accesskit_node_builder(egui::Id::new("x"), |_| ()).is_some();
        let none = egui::Modifiers::NONE;
        let (rect, _) = frame(vec![], none, &mut pick);
        at = at.max(rect.center());
        frame(vec![egui::Event::PointerMoved(at)], none, &mut pick);
        frame(vec![], none, &mut pick);
        assert!(!probe(&ctx), "labels off, as in the app");
        let key = egui::Event::Key {
            key: egui::Key::G,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Pick::MODS,
        };
        let (_, first) = frame(vec![key], Pick::MODS, &mut pick);
        assert!(first.is_none(), "the press frame has no labels yet");
        for _ in 0..3 {
            if let (_, Some(h)) = frame(vec![], none, &mut pick) {
                got = Some(h);
                break;
            }
        }
        let hits = got.expect("grabbed with labels off");
        assert_eq!(hits.first().map(|h| h.label.as_str()), Some("Join a session"));
        // The grab is seen: the element's outline and its name.
        let (r, words, _) = pick.flash.clone().expect("a flash");
        assert_eq!((r, words.as_str()), (hits[0].rect, "Grabbed: Join a session"));
        pick.paint(&ctx);
        assert!(pick.flash.is_some(), "still up right after");
        pick.flash.as_mut().unwrap().2 -= FLASH * 2;
        pick.paint(&ctx);
        assert!(pick.flash.is_none(), "gone once it expires");
        frame(vec![], none, &mut pick);
        assert!(!probe(&ctx), "switched off again");
    }

    /// The source search finds where a label is written, a const's uses,
    /// and a label built with numbers by its words.
    #[test]
    fn pick_sources_find_the_drawing_code() {
        let dir = std::env::temp_dir().join(format!("pick_src_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(
            dir.join("page.rs"),
            "pub const HOST: &str = \"Host a saved project\";\nfn page() {\n    row(HOST);\n    card(format!(\"One picture, {}×{}\", w, h));\n    row(\"Join a session\");\n}\n",
        )
        .unwrap();
        std::fs::write(dir.join("page_tests.rs"), "click(\"Join a session\");\n").unwrap();
        std::fs::write(dir.join("sub/more.rs"), "fn f() { row(\"Join a session\"); }\n").unwrap();
        let join = sources(&dir, "Join a session");
        assert_eq!(join, ["page.rs:5: row(\"Join a session\");", "sub/more.rs:1: fn f() { row(\"Join a session\"); }"]);
        let host = sources(&dir, "Host a saved project");
        assert_eq!(host, ["page.rs:1: pub const HOST: &str = \"Host a saved project\";", "page.rs:3: row(HOST);"]);
        let built = sources(&dir, "One picture, 1280×720");
        assert_eq!(built, ["page.rs:4: card(format!(\"One picture, {}×{}\", w, h));"]);
        assert!(sources(&dir, "Nowhere").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        // This build's own tree: the Host row is found where welcome.rs draws it.
        let root = src_root().expect("the source tree of this build");
        let real = sources(&root, crate::welcome::NewKind::Drawing.label());
        assert!(real.iter().any(|l| l.starts_with("welcome.rs:")), "{real:?}");
    }

    #[test]
    fn pick_record_reads_short() {
        let hits = [
            Hit { label: "Join a session".into(), role: "Button".into(), rect: egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(120.0, 28.0)) },
            Hit { label: "Start".into(), role: "Window".into(), rect: egui::Rect::EVERYTHING },
        ];
        let rec = render(&hits, &["welcome.rs:9: row(\"Join a session\");".into()], "Start, page Together", "project: x\n");
        assert!(rec.contains("element: \"Join a session\" (Button) at x 10 y 20, 120×28 pt"), "{rec}");
        assert!(rec.contains("inside: \"Start\""), "{rec}");
        assert!(rec.contains("where: Start, page Together"), "{rec}");
        assert!(rec.contains("welcome.rs:9:"), "{rec}");
        assert!(rec.len() < 2000, "a pick is small: {} bytes", rec.len());
        assert!(render(&[], &[], "", "s").contains("Nothing labelled"));
    }
}
