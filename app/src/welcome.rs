//! GM8: the Welcome screen — New from template, Open…, recent files with
//! thumbnails (missing ones greyed, with Remove), the sample cut, a "Show
//! at startup" latch — and the Quick start checklist it shares with Help ▸
//! Quick start. Plus the save worker's thumbnail (`app_meta["thumb"]`, a
//! base64 PNG 256 px wide, at most 64 KB) and its reader.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anim_core::command::CutRef;
use anim_core::model::Project;
use anim_core::store::FileMeta;
use eframe::egui;

use crate::config::Action;
use crate::plate;
use crate::templates;
use crate::workspace::Stage;

/// The project meta key the thumbnail lives under (`app.thumb` on disk;
/// `store::load` leaves it out of the document).
pub const THUMB_KEY: &str = anim_core::store::PREVIEW_KEY;
pub const THUMB_WIDTH: u32 = 256;
pub const THUMB_MAX_BYTES: usize = 64 * 1024;
/// The recent cards shown (the config keeps its own, longer list).
pub const RECENT_CARDS: usize = 12;

// ---- Quick start -----------------------------------------------------------

/// What a step's "Show me" does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowMe {
    Perform(Action),
    /// Do it when the cut allows (a charted key for inbetweens), else open
    /// the room where it is set up.
    Try(Action, Stage),
    /// The editing room with its render queue forward (GM11: every export
    /// is a queue job, File ▸ Export…'s too).
    Export,
}

pub struct Step {
    pub label: &'static str,
    /// The Action whose first run ticks the step (None: ticked elsewhere —
    /// the export when a render queue job starts).
    pub ticks_on: Option<Action>,
    pub show_me: ShowMe,
}

pub const STEPS: [Step; 5] = [
    Step {
        label: "Make a new drawing",
        ticks_on: Some(Action::NewDrawing),
        show_me: ShowMe::Perform(Action::NewDrawing),
    },
    Step {
        label: "Mark it Key",
        ticks_on: Some(Action::KindKey),
        show_me: ShowMe::Perform(Action::KindKey),
    },
    Step {
        label: "Make an inbetween",
        ticks_on: Some(Action::MakeInbetweens),
        show_me: ShowMe::Try(Action::MakeInbetweens, Stage::Drawing),
    },
    Step {
        label: "Play",
        ticks_on: Some(Action::PlayPause),
        show_me: ShowMe::Perform(Action::PlayPause),
    },
    Step {
        label: "Export",
        ticks_on: None,
        show_me: ShowMe::Export,
    },
];

/// The export step (ticked when a render queue job starts).
pub const EXPORT_STEP: usize = 4;

/// Tick the step `action` belongs to; true when a tick was new.
pub fn tick(ticks: &mut [bool; 5], action: Action) -> bool {
    let mut changed = false;
    for (t, step) in ticks.iter_mut().zip(STEPS.iter()) {
        if step.ticks_on == Some(action) && !*t {
            *t = true;
            changed = true;
        }
    }
    changed
}

/// The checklist rows: a tick, the step, its chord (from `binds`, so the
/// keys are learnt with the step) and "Show me". Returns the step whose
/// button was clicked.
pub fn checklist_ui(
    ui: &mut egui::Ui,
    ticks: &[bool; 5],
    binds: &[crate::config::Binding],
) -> Option<usize> {
    let mut clicked = None;
    for (i, step) in STEPS.iter().enumerate() {
        ui.horizontal(|ui| {
            let mark = if ticks[i] { "✔" } else { "○" };
            ui.label(egui::RichText::new(mark).color(if ticks[i] {
                plate::STRUCK
            } else {
                plate::legend_dim()
            }));
            ui.label(format!("{}. {}", i + 1, step.label));
            if let Some(chord) = step.ticks_on.and_then(|a| crate::cmdpal::chord_of(binds, a)) {
                ui.label(egui::RichText::new(chord).color(plate::legend_dim()));
            }
            if ui
                .small_button("Show me")
                .on_hover_text(match step.show_me {
                    ShowMe::Perform(a) => format!("does it: {}", a.label()),
                    ShowMe::Try(a, st) => {
                        format!("does it: {} (or opens the {} room)", a.label(), st.name())
                    }
                    ShowMe::Export => {
                        "opens the edit room's render queue (File ▸ Export… adds a job too)".into()
                    }
                })
                .clicked()
            {
                clicked = Some(i);
            }
        });
    }
    clicked
}

// ---- Thumbnails --------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = B64.iter().position(|&b| b == c)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn encode_png(w: u32, h: u32, rgb: &[u8]) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Best);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(rgb).ok()?;
    }
    Some(buf)
}

/// The current cut's frame 0 over paper white, box-filtered to 256 px wide,
/// as a base64 PNG of at most 64 KB (posterised once if the first try is
/// too big; None if even that is). CPU only — the save worker calls it.
pub fn render_thumb(project: &Project, at: CutRef) -> Option<String> {
    let cut = project.cut(at.scene, at.cut)?;
    let (w, h) = (project.width.max(1), project.height.max(1));
    let full = anim_core::export::render_frame_over(cut, 0, w, h, [255, 255, 255]);
    let tw = THUMB_WIDTH;
    let th = ((h as u64 * tw as u64 + w as u64 / 2) / w as u64).max(1) as u32;
    let mut rgb = Vec::with_capacity((tw * th * 3) as usize);
    for oy in 0..th {
        let y0 = (oy as u64 * h as u64 / th as u64) as u32;
        let y1 = (((oy + 1) as u64 * h as u64 / th as u64) as u32)
            .max(y0 + 1)
            .min(h);
        for ox in 0..tw {
            let x0 = (ox as u64 * w as u64 / tw as u64) as u32;
            let x1 = (((ox + 1) as u64 * w as u64 / tw as u64) as u32)
                .max(x0 + 1)
                .min(w);
            let mut sum = [0u32; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = &full[((y * w + x) * 4) as usize..][..3];
                    for c in 0..3 {
                        sum[c] += p[c] as u32;
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            rgb.extend(sum.map(|s| ((s + n / 2) / n) as u8));
        }
    }
    let fits = |png: Vec<u8>| Some(base64_encode(&png)).filter(|s| s.len() <= THUMB_MAX_BYTES);
    if let Some(s) = encode_png(tw, th, &rgb).and_then(fits) {
        return Some(s);
    }
    let poster: Vec<u8> = rgb.iter().map(|v| (v & 0xF0) | (v >> 4)).collect();
    encode_png(tw, th, &poster).and_then(fits)
}

/// A thumbnail back to (width, height, RGBA8).
pub fn decode_thumb(b64: &str) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = base64_decode(b64)?;
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let px = (info.width * info.height) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgb => buf[..px * 3]
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::Rgba => buf[..px * 4].to_vec(),
        _ => return None,
    };
    Some((info.width, info.height, rgba))
}

// ---- The screen ---------------------------------------------------------------

/// One recent file: its path, whether it is still there, and its meta.
pub struct RecentCard {
    pub path: PathBuf,
    pub missing: bool,
    pub meta: Option<FileMeta>,
}

/// The cards for the config's recent list (newest first, at most 12) —
/// `store::read_meta` only, never a whole project.
pub fn recent_cards(recent: &[PathBuf]) -> Vec<RecentCard> {
    recent
        .iter()
        .take(RECENT_CARDS)
        .map(|p| {
            let missing = !p.is_file();
            RecentCard {
                path: p.clone(),
                missing,
                meta: if missing {
                    None
                } else {
                    anim_core::store::read_meta(p).ok()
                },
            }
        })
        .collect()
}

/// What the screen asks of the App.
#[derive(Debug, Clone, PartialEq)]
pub enum WelcomeAction {
    Template(usize),
    Custom,
    Open,
    OpenRecent(PathBuf),
    Remove(PathBuf),
    Sample,
    /// "Show me" on a Quick start step: open the sample, then do the step.
    ShowMe(usize),
    /// Settings… (the shell's window, over the screen).
    Settings,
    /// Close the screen shown over an open cut (File ▸ Welcome screen…).
    Back,
}

#[derive(Default)]
pub struct Welcome {
    pub cards: Vec<RecentCard>,
    textures: HashMap<PathBuf, Option<egui::TextureHandle>>,
    /// Shown again over an open cut (File ▸ Welcome screen…): a modal with
    /// a way back, and the cut stays open until a card replaces it.
    pub over_editor: bool,
}

impl Welcome {
    pub fn new(recent: &[PathBuf]) -> Self {
        Self {
            cards: recent_cards(recent),
            textures: HashMap::new(),
            over_editor: false,
        }
    }

    /// The screen again over an open cut.
    pub fn over_editor(recent: &[PathBuf]) -> Self {
        Self {
            over_editor: true,
            ..Self::new(recent)
        }
    }

    /// Drop a card (Remove) without touching the disk.
    pub fn forget(&mut self, path: &Path) {
        self.cards.retain(|c| c.path != path);
    }

    fn thumb(&mut self, ctx: &egui::Context, card: usize) -> Option<egui::TextureHandle> {
        let c = &self.cards[card];
        self.textures
            .entry(c.path.clone())
            .or_insert_with(|| {
                let (w, h, rgba) = decode_thumb(c.meta.as_ref()?.app_meta.get(THUMB_KEY)?)?;
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                Some(ctx.load_texture(
                    format!("thumb {}", c.path.display()),
                    img,
                    egui::TextureOptions::LINEAR,
                ))
            })
            .clone()
    }
}

/// A section's heading — a real label, so the screen reader (and the
/// harness) finds it.
fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong().color(plate::LEGEND));
}

/// The Welcome screen, drawn into the central area (or a modal over an
/// open cut). `binds` names the chords beside Open… and the checklist.
pub fn ui(
    ui: &mut egui::Ui,
    w: &mut Welcome,
    show_at_startup: &mut bool,
    ticks: &[bool; 5],
    binds: &[crate::config::Binding],
) -> Option<WelcomeAction> {
    let mut out = None;
    plate::surface(ui);
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading("Gaban Studio");
            if w.over_editor
                && ui
                    .button("Back to the cut")
                    .on_hover_text("closes this screen; the open cut is untouched — Esc")
                    .clicked()
            {
                out = Some(WelcomeAction::Back);
            }
        });
        ui.add_space(8.0);
        section(ui, "New from template");
        ui.horizontal_wrapped(|ui| {
            for (i, t) in templates::ALL.iter().enumerate() {
                ui.vertical(|ui| {
                    if ui.button(t.name).on_hover_text(t.tooltip()).clicked() {
                        out = Some(WelcomeAction::Template(i));
                    }
                    ui.label(
                        egui::RichText::new(t.summary())
                            .size(11.0)
                            .color(plate::legend_dim()),
                    );
                });
                ui.add_space(10.0);
            }
            if ui
                .button("Custom size…")
                .on_hover_text("name, size, fps and dpi of your own")
                .clicked()
            {
                out = Some(WelcomeAction::Custom);
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let open_hint = match crate::cmdpal::chord_of(binds, Action::Open) {
                Some(chord) => format!("Open a saved project (.animproj) — {chord}"),
                None => "Open a saved project (.animproj)".into(),
            };
            if ui.button("Open…").on_hover_text(open_hint).clicked() {
                out = Some(WelcomeAction::Open);
            }
            if ui
                .button("Open the sample cut")
                .on_hover_text("a ball bounce on twos over a BG plate — opens untitled")
                .clicked()
            {
                out = Some(WelcomeAction::Sample);
            }
            if ui
                .button("Settings…")
                .on_hover_text("theme, UI scale, pen and shortcuts — before the first cut")
                .clicked()
            {
                out = Some(WelcomeAction::Settings);
            }
        });
        ui.add_space(8.0);
        section(ui, "Recent files");
        if w.cards.is_empty() {
            ui.label(egui::RichText::new("nothing yet").color(plate::legend_dim()));
        }
        ui.horizontal_wrapped(|ui| {
            for i in 0..w.cards.len() {
                let tex = if w.cards[i].missing {
                    None
                } else {
                    w.thumb(ui.ctx(), i)
                };
                let c = &w.cards[i];
                let name = c
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| c.path.display().to_string());
                ui.vertical(|ui| {
                    ui.set_width(150.0);
                    if let Some(tex) = &tex {
                        let size = tex.size_vec2() * (140.0 / tex.size_vec2().x.max(1.0));
                        ui.add(egui::Image::new((tex.id(), size)));
                    }
                    if c.missing {
                        ui.add_enabled(false, egui::Button::new(&name))
                            .on_disabled_hover_text(format!("missing: {}", c.path.display()));
                        ui.label(egui::RichText::new("missing").color(plate::legend_dim()));
                        if ui.small_button("Remove").clicked() {
                            out = Some(WelcomeAction::Remove(c.path.clone()));
                        }
                    } else {
                        if ui
                            .button(&name)
                            .on_hover_text(c.path.display().to_string())
                            .clicked()
                        {
                            out = Some(WelcomeAction::OpenRecent(c.path.clone()));
                        }
                        if let Some(m) = &c.meta {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}×{} · {} fps",
                                    m.width, m.height, m.fps
                                ))
                                .size(11.0)
                                .color(plate::legend_dim()),
                            );
                        }
                    }
                });
            }
        });
        ui.add_space(8.0);
        section(ui, "Quick start");
        if let Some(step) = checklist_ui(ui, ticks, binds) {
            out = Some(WelcomeAction::ShowMe(step));
        }
        ui.add_space(8.0);
        plate::latch(ui, show_at_startup, "Show at startup").on_hover_text(
            "off: start-up opens the New Project form; Settings ▸ General turns it back on",
        );
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use anim_core::export::{render_frame, render_graph_frame_law};
    use egui_kittest::kittest::Queryable;

    #[test]
    fn welcome_base64_round_trips() {
        for n in 0..8usize {
            let bytes: Vec<u8> = (0..n as u8).map(|b| b.wrapping_mul(37)).collect();
            assert_eq!(
                base64_decode(&base64_encode(&bytes)).as_deref(),
                Some(&bytes[..])
            );
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }

    #[test]
    fn welcome_sample_has_keys_and_a_frame() {
        let s = crate::sample::build_sample();
        let cut = s.engine.project.cut(s.at().scene, s.at().cut).unwrap();
        assert!(cut.drawings.len() >= 12, "{} drawings", cut.drawings.len());
        let keys = cut
            .drawings
            .iter()
            .filter(|d| cut.drawing_def(d.id).kind == anim_core::model::DrawingKind::Key)
            .count();
        assert_eq!(keys, crate::sample::KEYS.len());
        assert_eq!(cut.frame_count, crate::sample::FRAMES);
        assert!(s.file_path.is_none(), "the sample opens untitled");
        assert!(!s.engine.can_undo(), "the sample opens with no history");
        let (w, h) = crate::sample::SIZE;
        let px = render_frame(cut, 0, w, h);
        assert!(px.chunks_exact(4).any(|p| p[3] > 0), "frame 0 is empty");
        // The ball itself reaches frame 0 (its paint, not just the plate).
        let (cx, cy) = crate::sample::ball_centre(0);
        let i = ((cy as u32 * w + cx as u32) * 4) as usize;
        assert_eq!(&px[i..i + 3], &[214, 72, 60]);
        assert!(
            !crate::palette::Palettes::load_from(&s.engine.project)
                .characters
                .is_empty()
                || !s.palettes.characters.is_empty()
        );
    }

    #[test]
    fn welcome_auto_comp_matches_the_column_render() {
        let s = crate::sample::build_sample();
        let cut = s.engine.project.cut(s.at().scene, s.at().cut).unwrap();
        let (w, h) = crate::sample::SIZE;
        for frame in [0, 7, 23] {
            let graph = render_graph_frame_law(cut, s.engine.project.color_law, frame, w, h)
                .expect("the auto-comp wires the output");
            assert!(
                graph == render_frame(cut, frame, w, h),
                "frame {frame} differs"
            );
        }
    }

    #[test]
    fn welcome_auto_comp_is_one_undo_step() {
        let mut s = crate::sample::build_sample();
        let at = s.at();
        let before = s
            .engine
            .project
            .cut(at.scene, at.cut)
            .unwrap()
            .graph
            .clone();
        assert!(s.auto_comp());
        assert_eq!(s.engine.history_len(), 1);
        s.engine.undo().unwrap();
        assert_eq!(
            s.engine.project.cut(at.scene, at.cut).unwrap().graph,
            before
        );
    }

    #[test]
    fn welcome_templates_make_their_projects() {
        use anim_core::xsheet::ColumnKind;
        for t in templates::ALL {
            let s = t.build();
            let p = &s.engine.project;
            assert_eq!(
                (p.width, p.height, p.fps),
                (t.width, t.height, t.fps),
                "{}",
                t.name
            );
            assert_eq!(p.default_cut_frames, Some(t.cut_frames));
            assert_eq!(p.color_law, anim_core::color::ColorLaw::Srgb);
            let cut = p.cut(s.at().scene, s.at().cut).unwrap();
            assert_eq!(cut.frame_count, t.cut_frames, "{}", t.name);
            let names: Vec<&str> = cut.xsheet.columns.iter().map(|c| c.name.as_str()).collect();
            let want: Vec<&str> = t.columns.iter().map(|c| c.0).collect();
            assert_eq!(names, want, "{}", t.name);
            for (col, (_, kind)) in cut.xsheet.columns.iter().zip(t.columns) {
                assert_eq!(
                    cut.column_def(col.id).kind,
                    *kind,
                    "{} {}",
                    t.name,
                    col.name
                );
            }
            let active = cut.xsheet.column(s.view.active_column).unwrap();
            assert_eq!(
                cut.column_def(active.id).kind,
                ColumnKind::Cel,
                "the pen starts on a cel"
            );
            // The graph's Output is wired (the auto-comp).
            let out = cut.graph.output.expect("an output");
            let node = cut.graph.nodes().find(|n| n.id == out).unwrap();
            assert!(
                node.inputs[0].is_some(),
                "{}: the output is unwired",
                t.name
            );
            assert!(!s.engine.can_undo(), "a new project has no history");
            // A new cut takes the template's length.
            let mut s = s;
            let scene = s.at().scene;
            s.new_cut(scene);
            assert_eq!(s.frame_count(), t.cut_frames);
        }
        // Each template's card opens its first room.
        let mut h = crate::testkit::EditorHarness::new();
        for (i, t) in templates::ALL.iter().enumerate() {
            *h.editor_mut() = crate::open_template(i, None, None);
            h.run();
            assert_eq!(h.editor().stage, Some(t.room), "{}", t.name);
        }
    }

    #[test]
    fn welcome_screen_shows_new_open_sample_and_templates() {
        let mut w = Welcome::default();
        let mut show = true;
        let mut got = None;
        let mut dressed = false;
        let binds = crate::config::Config::default().keybinds;
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_ui(|ui| {
                // The app's own fonts go live on the next pass.
                if !std::mem::replace(&mut dressed, true) {
                    crate::dress(ui.ctx());
                    ui.ctx().request_repaint();
                    return;
                }
                if let Some(a) = super::ui(ui, &mut w, &mut show, &[false; 5], &binds) {
                    got = Some(a);
                }
            });
        h.run();
        for label in [
            "New from template",
            "Open…",
            "Open the sample cut",
            "Show at startup",
        ] {
            assert!(h.query_by_label(label).is_some(), "no {label:?}");
        }
        for t in templates::ALL {
            assert!(h.query_by_label(t.name).is_some(), "no {:?}", t.name);
        }
        h.get_by_label("Web 720p").click();
        h.run();
        drop(h);
        assert_eq!(got, Some(WelcomeAction::Template(1)));
        // The start-up route: the Welcome screen unless the latch is off.
        let mut c = crate::config::Config::default();
        assert!(crate::startup_shows_welcome(&c));
        c.ui.show_welcome = false;
        assert!(!crate::startup_shows_welcome(&c));
    }

    #[test]
    fn welcome_recent_cards_flag_missing_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let there = dir.path().join("there.animproj");
        anim_core::store::save(&crate::sample::build_sample().engine.project, &there).unwrap();
        let gone = dir.path().join("gone.animproj");
        let mut config = crate::config::Config::default();
        config.session.recent = vec![there.clone(), gone.clone()];
        let mut w = Welcome::new(&config.session.recent);
        assert!(!w.cards[0].missing && w.cards[0].meta.is_some());
        assert!(w.cards[1].missing && w.cards[1].meta.is_none());
        let mut show = true;
        let mut got = None;
        let mut dressed = false;
        let binds = config.keybinds.clone();
        {
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(1280.0, 800.0))
                .build_ui(|ui| {
                    if !std::mem::replace(&mut dressed, true) {
                        crate::dress(ui.ctx());
                        ui.ctx().request_repaint();
                        return;
                    }
                    if let Some(a) = super::ui(ui, &mut w, &mut show, &[false; 5], &binds) {
                        got = Some(a);
                    }
                });
            h.run();
            assert!(h.query_by_label("missing").is_some());
            assert!(h.query_by_label("there").is_some());
            h.get_by_label("Remove").click();
            h.run();
        }
        assert_eq!(got, Some(WelcomeAction::Remove(gone.clone())));
        crate::forget_recent(&mut config, &mut w, &gone);
        assert_eq!(config.session.recent, vec![there]);
        assert_eq!(w.cards.len(), 1);
    }

    #[test]
    fn welcome_thumbnail_is_written_on_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thumb.animproj");
        let mut s = crate::sample::build_sample();
        s.begin_save(path.clone());
        s.wait_save();
        assert!(!s.is_dirty());
        // The preview is the file's, never the document's.
        let loaded = anim_core::store::load(&path).unwrap();
        assert!(!loaded.app_meta.contains_key(THUMB_KEY));
        // Break every drawing row: read_meta must not care.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("DROP TABLE drawings;").unwrap();
        }
        let meta = anim_core::store::read_meta(&path).unwrap();
        let thumb = meta.app_meta.get(THUMB_KEY).expect("a thumbnail");
        assert!(thumb.len() <= THUMB_MAX_BYTES, "{} bytes", thumb.len());
        let (w, h, rgba) = decode_thumb(thumb).unwrap();
        assert_eq!((w, h), (256, 144));
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        assert_eq!(meta.width, crate::sample::SIZE.0);
    }

    #[test]
    fn welcome_project_props_undo_exactly() {
        use anim_core::command::Command;
        let mut s = crate::sample::build_sample();
        let before = s.engine.project.props();
        let after = anim_core::model::ProjectProps {
            name: "renamed".into(),
            width: 800,
            height: 450,
            fps: 12,
            dpi: 144.0,
            default_cut_frames: Some(96),
        };
        let tiles_before = s.engine.project.clone();
        assert!(s.set_project_props(after.clone()));
        assert_eq!(s.engine.project.props(), after);
        s.engine.undo().unwrap();
        assert_eq!(s.engine.project.props(), before);
        assert_eq!(s.engine.project, tiles_before, "the inverse is exact");
        s.engine.redo().unwrap();
        assert_eq!(s.engine.project.props(), after);
        // A stale edit refuses rather than drift.
        let stale = Command::SetProjectProps {
            before: before.clone(),
            after: before.clone(),
        };
        assert!(s.engine.apply("stale", vec![stale]).is_err());
        // The field survives a save.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.animproj");
        anim_core::store::save(&s.engine.project, &path).unwrap();
        assert_eq!(
            anim_core::store::load(&path).unwrap().default_cut_frames,
            Some(96)
        );
    }

    #[test]
    fn welcome_settings_window_edits_fps_in_one_step() {
        let mut h = crate::testkit::EditorHarness::new();
        h.perform(Action::ProjectSettings);
        h.run();
        assert!(h.has("Project settings"));
        let undo0 = h.state().engine.history_len();
        h.editor_mut().project_form.as_mut().unwrap().fps = 12;
        h.click("Apply");
        h.run();
        assert_eq!(h.state().engine.project.fps, 12);
        assert_eq!(h.state().engine.history_len(), undo0 + 1);
        h.perform(Action::Undo);
        h.run();
        assert_eq!(h.state().engine.project.fps, 24);
    }

    #[test]
    fn welcome_rooms_split_finishing_and_add_composite() {
        use crate::config::{Config, LayersConfig};
        assert_eq!(
            Stage::ALL,
            &[
                Stage::Layout,
                Stage::Drawing,
                Stage::InkPaint,
                Stage::Composite,
                Stage::Edit
            ]
        );
        // An older config binds "finishing" only: ink & paint keeps it,
        // composite gets the full shiage stack.
        let mut layers = LayersConfig::default();
        layers.room_stack.clear();
        layers.room_stack.insert("finishing".into(), "Douga".into());
        let json = serde_json::to_string(&layers).unwrap();
        let back: LayersConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.stack_for_room(crate::stage_key(Some(Stage::InkPaint)))
                .name,
            "Douga"
        );
        assert_eq!(
            back.stack_for_room(crate::stage_key(Some(Stage::Composite)))
                .name,
            "Shiage (full)"
        );
        assert_eq!(
            Config::default().layers.stack_for_room("composite").name,
            "Shiage (full)"
        );
        // Entering Composite shows the node graph and the viewer, with
        // composite view on.
        let mut h = crate::testkit::EditorHarness::new();
        h.perform(Action::RoomComposite);
        h.run_frames(3);
        assert_eq!(h.editor().stage, Some(Stage::Composite));
        let panes: Vec<_> = h.editor().dock.iter_all_tabs().map(|(_, p)| *p).collect();
        assert!(panes.contains(&crate::workspace::Pane::NodeGraph));
        assert!(panes.contains(&crate::workspace::Pane::Viewer(0)));
        assert!(panes.contains(&crate::workspace::Pane::Curves));
        assert!(h.has("room satsuei (composite)"));
        assert!(crate::workspace::Stage::Composite.view().composite_view);
        // The layout probe passes in the new room, at every display scale.
        for ppp in crate::PROBE_SCALES {
            let run = crate::probe_run(Stage::Composite, ppp);
            let lines = run.lines.join("\n");
            assert_eq!(
                run.unmeasured, 0,
                "composite at {ppp}: no canvas rect\n{lines}"
            );
            assert_eq!(
                run.moved, 0,
                "composite at {ppp}: the canvas moved\n{lines}"
            );
        }
    }

    #[test]
    fn welcome_ui_scale_sets_the_zoom_and_the_probe_passes() {
        let mut c = crate::config::Config::default();
        assert_eq!(c.ui.zoom(), 1.0);
        c.ui.scale = 150;
        let dir = tempfile::tempdir().unwrap();
        c.save_to(dir.path()).unwrap();
        let (back, _) = crate::config::Config::load_from(dir.path());
        assert_eq!(back.ui.scale, 150);
        let ctx = egui::Context::default();
        crate::apply_ui_scale(&ctx, &back);
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        assert_eq!(ctx.zoom_factor(), 1.5);
        assert_eq!(ctx.pixels_per_point(), 1.5);
        // The layout probe passes at that scale.
        let run = crate::probe_run_zoom(Stage::Drawing, 1.0, 150);
        let lines = run.lines.join("\n");
        assert_eq!(run.unmeasured, 0, "150%: no canvas rect\n{lines}");
        assert_eq!(run.moved, 0, "150%: the canvas moved\n{lines}");
    }

    #[test]
    fn welcome_quick_start_ticks_and_show_me() {
        // Frame 0 empty, so the drawing verb runs.
        let state = crate::doc::AppState::new_project("qs", 640, 360, 24, 300.0);
        let mut h = crate::testkit::EditorHarness::with_state(state);
        assert_eq!(h.perform(Action::NewDrawing), crate::Performed::Done);
        h.run();
        assert_eq!(h.editor().quick_start, [true, false, false, false, false]);
        // The ticks survive a config round trip.
        let mut c = crate::config::Config::default();
        crate::merge_quick_start(&mut c, &h.editor().quick_start);
        let dir = tempfile::tempdir().unwrap();
        c.save_to(dir.path()).unwrap();
        let (back, _) = crate::config::Config::load_from(dir.path());
        assert_eq!(back.ui.quick_start, [true, false, false, false, false]);
        // "Show me" on step 4 performs PlayPause.
        assert!(!h.state().view.playing);
        h.editor_mut().show_me(3);
        h.run();
        assert!(h.state().view.playing);
        assert!(h.editor().quick_start[3]);
        h.perform(Action::PlayPause);
        h.run();
    }

    #[test]
    fn welcome_resize_keeps_the_pixels_and_the_layers() {
        let mut h = crate::testkit::EditorHarness::with_state(crate::sample::build_sample());
        let (w0, h0) = crate::sample::SIZE;
        let cut0 = h.state().cut().clone();
        let before = render_frame(&cut0, 0, w0, h0);
        let layers = (h.editor().paint.is_some(), h.editor().graph.is_some());
        h.perform(Action::ProjectSettings);
        h.run();
        let form = h.editor_mut().project_form.as_mut().unwrap();
        (form.width, form.height) = (800, 450);
        h.click("Apply");
        h.run();
        let p = &h.state().engine.project;
        assert_eq!((p.width, p.height), (800, 450));
        // The GPU paint layer and compositor are kept: they follow the
        // size on their own (`ensure_size`), never dropped for good.
        assert_eq!(
            (h.editor().paint.is_some(), h.editor().graph.is_some()),
            layers
        );
        // No tile moved: the old picture sits at the top-left of the
        // bigger paper, the rest is empty.
        let cut1 = h.state().cut();
        assert_eq!(cut1.drawings, cut0.drawings);
        let after = render_frame(cut1, 0, 800, 450);
        for y in 0..450usize {
            for x in 0..800usize {
                let px = &after[(y * 800 + x) * 4..][..4];
                if x < w0 as usize && y < h0 as usize {
                    assert_eq!(px, &before[(y * w0 as usize + x) * 4..][..4], "({x}, {y})");
                } else {
                    assert_eq!(px[3], 0, "({x}, {y}) is past the old paper");
                }
            }
        }
        h.perform(Action::Undo);
        h.run();
        let p = &h.state().engine.project;
        assert_eq!((p.width, p.height), (w0, h0));
    }

    #[test]
    fn welcome_new_columns_reach_the_auto_comp() {
        use anim_core::raster::{RasterLayer, TILE_LEN, TileData};
        use anim_core::xsheet::ColumnKind;
        let mut s = templates::ALL[1].build(); // BG, A, B
        s.add_column();
        assert_eq!(s.engine.history_len(), 1, "the column and its chain: one step");
        s.new_drawing_at_frame();
        let id = s.current_drawing().expect("a drawing in the new column");
        let at = s.at();
        let texel = anim_core::color::ColorLaw::Srgb.texel([200, 30, 30, 255]);
        let rgba: Vec<u16> = texel.iter().copied().cycle().take(TILE_LEN).collect();
        let cut = s.engine.project.cut_mut(at.scene, at.cut).unwrap();
        let d = cut.drawings.iter_mut().find(|d| d.id == id).unwrap();
        d.layers[0].raster = RasterLayer {
            tiles: std::collections::BTreeMap::from([(
                (0, 0),
                std::sync::Arc::new(TileData::from_vec(rgba)),
            )]),
        };
        // The graph follows the sheet: its render is the column render,
        // the new column's pixels included.
        let cut = s.cut();
        assert_eq!(crate::doc::auto_chain(&cut.graph), Some(cut.render_order()));
        let column = render_frame(cut, 0, 320, 180);
        assert!(column.chunks(4).any(|p| p[3] > 0), "the new column draws");
        let law = s.engine.project.color_law;
        assert!(render_graph_frame_law(cut, law, 0, 320, 180) == Some(column));
        // A kind change and a removal re-steer it too, one step each.
        let a = cut.xsheet.columns[1].id;
        let added = s.view.active_column;
        s.view.active_column = a;
        let n = s.engine.history_len();
        s.set_column_kind(ColumnKind::Ref);
        assert_eq!(s.engine.history_len(), n + 1);
        assert!(!s.cut().render_order().contains(&a));
        assert_eq!(crate::doc::auto_chain(&s.cut().graph), Some(s.cut().render_order()));
        s.view.active_column = added;
        s.remove_active_column();
        assert_eq!(crate::doc::auto_chain(&s.cut().graph), Some(s.cut().render_order()));
        // Undo brings the column and its chain back together.
        s.engine.undo().unwrap();
        assert!(s.cut().render_order().contains(&added));
        assert_eq!(crate::doc::auto_chain(&s.cut().graph), Some(s.cut().render_order()));
        // A graph shaped by hand is never touched.
        s.graph_add_node(anim_core::graph::NodeKind::Output, (40.0, 40.0));
        let shaped = s.cut().graph.clone();
        s.add_column();
        assert_eq!(s.cut().graph, shaped);
    }

    #[test]
    fn welcome_build_composite_asks_over_a_hand_graph() {
        let mut h = crate::testkit::EditorHarness::with_state(crate::sample::build_sample());
        // Over the auto-comp it runs at once, one step.
        let n0 = h.state().engine.history_len();
        assert_eq!(h.perform(Action::BuildComposite), crate::Performed::Done);
        assert_eq!(h.state().engine.history_len(), n0 + 1);
        assert_eq!(h.state().auto_comp_confirm, None);
        // A node added by hand: the verb asks first and changes nothing.
        h.state_mut()
            .graph_add_node(anim_core::graph::NodeKind::Output, (40.0, 40.0));
        let shaped = h.state().cut().graph.clone();
        let n = shaped.nodes().count();
        let steps = h.state().engine.history_len();
        h.perform(Action::BuildComposite);
        h.run();
        assert!(h.has(&format!(
            "Replace the node graph ({n} nodes)? Undo brings it back."
        )));
        assert_eq!(h.state().cut().graph, shaped);
        h.click("Cancel");
        h.run();
        assert_eq!(h.state().auto_comp_confirm, None);
        assert_eq!(h.state().cut().graph, shaped);
        // Held DANGER: replaced in one step, and undo brings it back.
        h.perform(Action::BuildComposite);
        h.run();
        h.hold("Replace");
        assert_eq!(h.state().engine.history_len(), steps + 1);
        assert!(crate::doc::auto_chain(&h.state().cut().graph).is_some());
        h.perform(Action::Undo);
        h.run();
        assert_eq!(h.state().cut().graph, shaped);
        assert!(Action::BuildComposite.help().starts_with("Replace the node graph"));
    }

    #[test]
    fn welcome_screen_comes_back_after_startup() {
        // File ▸ Welcome screen…: a clean cut goes at once.
        let mut s = crate::sample::build_sample();
        s.saved_rev = s.engine.revision();
        let mut h = crate::testkit::EditorHarness::with_state(s);
        assert_eq!(Action::WelcomeScreen.menu(), Some("File/Welcome screen…"));
        assert_eq!(h.perform(Action::WelcomeScreen), crate::Performed::Done);
        assert!(h.editor().request_welcome);
        // Unsaved work is asked about first.
        h.editor_mut().request_welcome = false;
        h.state_mut().add_column();
        assert!(h.state().is_dirty());
        h.perform(Action::WelcomeScreen);
        h.run();
        assert!(h.editor().leave_pending() && !h.editor().request_welcome);
        h.hold("Don't Save");
        assert!(h.editor().request_welcome);
        // The palette finds it by the words a newcomer types.
        assert!(Action::WelcomeScreen.aliases().contains(&"templates"));
        // Over a cut the screen stays until a card or Back ends it.
        let c = crate::config::Config::default();
        let again = Welcome::over_editor(&[]);
        assert_eq!(
            crate::startup_route(true, false, Some(&again), &c),
            crate::Startup::Keep
        );
        // Settings ▸ General carries the start-up latch, so it can be
        // turned back on.
        let mut config = c.clone();
        config.ui.show_welcome = false;
        let mut flag = false;
        let mut dressed = false;
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(900.0, 900.0))
            .build_ui(|ui| {
                // The plate's fonts go live on the next pass.
                if !std::mem::replace(&mut dressed, true) {
                    crate::dress(ui.ctx());
                    ui.ctx().request_repaint();
                    return;
                }
                crate::config::general_page(ui, &mut config, None, &mut flag, None);
            });
        h.run();
        assert!(
            h.query_by_label("Show the Welcome screen at startup")
                .is_some()
        );
    }

    #[test]
    fn welcome_startup_route_picks_the_screen() {
        use crate::{Startup, startup_route};
        let mut c = crate::config::Config::default();
        // Nothing open: the Welcome screen.
        assert_eq!(startup_route(false, false, None, &c), Startup::Welcome);
        // The latch off: the New Project form, as before GM8.
        c.ui.show_welcome = false;
        assert_eq!(startup_route(false, false, None, &c), Startup::NewForm);
        // A cut restored under the start-up screen (recovery, a handover)
        // drops it; a form over the screen keeps it for its Cancel.
        c.ui.show_welcome = true;
        let start = Welcome::default();
        assert_eq!(
            startup_route(true, false, Some(&start), &c),
            Startup::DropWelcome
        );
        assert_eq!(startup_route(false, true, Some(&start), &c), Startup::Keep);
        assert_eq!(startup_route(true, false, None, &c), Startup::Keep);
        assert_eq!(startup_route(false, true, None, &c), Startup::Keep);
    }

    #[test]
    fn welcome_show_me_works_from_the_screen() {
        // Step 1 on the sample: its empty column B takes the new drawing.
        let (ed, did) = crate::welcome_show_me(0, None, None);
        assert_eq!(did, crate::Performed::Done);
        assert!(ed.quick_start[0], "step 1 ticked");
        let b = ed.state.cut().xsheet.column(ed.state.view.active_column).unwrap();
        assert_eq!(b.name, "B");
        assert!(ed.state.current_drawing().is_some());
        // Every step does something from the screen it is offered on.
        for (i, step) in STEPS.iter().enumerate() {
            let (ed, did) = crate::welcome_show_me(i, None, None);
            assert_eq!(did, crate::Performed::Done, "step {}", i + 1);
            if let Some(a) = step.ticks_on.filter(|a| *a != Action::MakeInbetweens) {
                assert!(ed.quick_start[i], "step {} ({a:?}) ticked", i + 1);
            }
        }
        // The sample still renders as before: B is empty.
        let s = crate::sample::build_practice();
        let (w, h) = crate::sample::SIZE;
        let cut = s.cut();
        assert!(
            render_graph_frame_law(cut, s.engine.project.color_law, 0, w, h)
                == Some(render_frame(cut, 0, w, h))
        );
    }

    #[test]
    fn welcome_animatic_board_renders_under_a() {
        let t = templates::ALL
            .iter()
            .find(|t| t.name.starts_with("Animatic"))
            .unwrap();
        let s = t.build();
        let cut = s.cut();
        let names: Vec<&str> = cut
            .render_order()
            .iter()
            .map(|id| cut.xsheet.column(*id).unwrap().name.as_str())
            .collect();
        assert_eq!(names, ["BOARD", "A"], "the boards render, A above them");
        assert!(t.tooltip().contains("BOARD, A"));
    }

    #[test]
    fn welcome_new_project_form_offers_the_templates() {
        use crate::newproject::{NewProjectForm, form_ui};
        // The blank form is unchanged: one column, a 48-frame cut.
        let blank = NewProjectForm::default().new_state();
        assert_eq!(blank.cut().xsheet.columns.len(), 1);
        assert_eq!(blank.frame_count(), 48);
        // Picked in the form, a template brings its sheet and cut length.
        let mut form = NewProjectForm::default();
        let mut dressed = false;
        {
            let mut h = egui_kittest::Harness::builder()
                .with_size(egui::vec2(700.0, 700.0))
                .build_ui(|ui| {
                    if !std::mem::replace(&mut dressed, true) {
                        crate::dress(ui.ctx());
                        ui.ctx().request_repaint();
                        return;
                    }
                    form_ui(ui, &mut form, true);
                });
            h.run();
            // The first combo on the form is "start from".
            h.query_all_by_role(egui::accesskit::Role::ComboBox)
                .next()
                .expect("the template combo")
                .click();
            h.run();
            h.get_by_label("Animatic / storyboard").click();
            h.run();
        }
        assert_eq!(form.template, Some(3));
        assert_eq!((form.width, form.height, form.fps), (1920, 1080, 24));
        form.name = "boards".into();
        let s = form.new_state();
        let p = &s.engine.project;
        assert_eq!(p.name, "boards");
        assert_eq!(p.default_cut_frames, Some(120));
        assert_eq!(p.color_law, anim_core::color::ColorLaw::Srgb);
        assert_eq!(s.frame_count(), 120);
        let names: Vec<&str> = s.cut().xsheet.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["BOARD", "A"]);
    }
}
