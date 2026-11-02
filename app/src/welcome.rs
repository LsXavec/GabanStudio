//! GM8: the Welcome screen — New from template, Open…, recent files with
//! thumbnails (missing ones greyed, with Remove), the sample cut, a "Show
//! at startup" latch — and the Quick start checklist it shares with Help ▸
//! Quick start. Plus the save worker's thumbnail (`app_meta["thumb"]`, a
//! base64 PNG 256 px wide, at most 64 KB) and its reader.

use std::collections::HashMap;
use std::path::PathBuf;

use anim_core::command::CutRef;
use anim_core::model::Project;
use anim_core::store::FileMeta;
use eframe::egui;

use crate::config::Action;
use crate::lessons::{self, Class, Lesson};
use crate::plate;
use crate::plate::Card;
use crate::steps::ProjectKind;
use crate::templates;

/// The project meta key the thumbnail lives under (`app.thumb` on disk;
/// `store::load` leaves it out of the document).
pub const THUMB_KEY: &str = anim_core::store::PREVIEW_KEY;
pub const THUMB_WIDTH: u32 = 256;
pub const THUMB_MAX_BYTES: usize = 64 * 1024;
/// The recent cards shown (the config keeps its own, longer list).
pub const RECENT_CARDS: usize = 12;
/// AU6: the Start float's heading.
pub const HEADING: &str = "Start";
/// AU20: its heading on the Lessons page, and on What's New.
pub const LESSONS_HEADING: &str = "Lessons";
pub const WHATS_NEW_HEADING: &str = concat!("What's new in ", env!("CARGO_PKG_VERSION"));
/// AU20: the latch in its heading row, and that latch's hover.
pub const LATCH: &str = "Show at startup";
pub const LATCH_CARD: &str = "Off: the app opens on the same paper, without this panel";
/// AU6: the Start float's recent rows; the rest are under File ▸ Open recent.
const RECENT_ROWS: usize = 2;

// ---- Quick start -----------------------------------------------------------

/// What a step's "Show me" does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowMe {
    /// An undoable demo circle, on this frame (spec 8.3).
    Circle,
    /// The next circle: on a drawing of its own, past the first frame.
    NextCircle,
    Perform(Action),
    /// Playback, unless it already runs.
    Play,
    /// The Export step, with its first choice ringed (spec 8.3).
    Export,
    /// AU18: the Panel tool, unless it is already armed: its Action turns it
    /// back off.
    Panels,
}

pub struct Step {
    pub label: &'static str,
    /// The Action that ticks the step. The others tick on outcomes, in
    /// `Editor::say_outcomes`: a finished stroke, playback, a finished export.
    pub ticks_on: Option<Action>,
    pub show_me: ShowMe,
}

pub const STEPS: [Step; 5] = [
    Step {
        label: "Draw something",
        ticks_on: None,
        show_me: ShowMe::Circle,
    },
    Step {
        label: "Add a drawing",
        ticks_on: Some(Action::AddDrawingAfter),
        show_me: ShowMe::Perform(Action::AddDrawingAfter),
    },
    Step {
        label: "Draw the next pose",
        ticks_on: None,
        show_me: ShowMe::NextCircle,
    },
    Step {
        label: "Play it",
        ticks_on: None,
        show_me: ShowMe::Play,
    },
    Step {
        label: "Export a file",
        ticks_on: None,
        show_me: ShowMe::Export,
    },
];

/// AU18: a drawing's five. Only slots 0 and 4 share a meaning with `STEPS`, so
/// slots 1 to 3 keep their ticks in `hints_seen` (`Editor::tick_slot`).
pub const DRAWING_STEPS: [Step; 5] = [
    Step {
        label: "Draw something",
        ticks_on: None,
        show_me: ShowMe::Circle,
    },
    Step {
        label: "Color it",
        ticks_on: None,
        show_me: ShowMe::Perform(Action::RoomFinishing),
    },
    Step {
        label: "Add an effect",
        ticks_on: None,
        show_me: ShowMe::Perform(Action::RoomComposite),
    },
    Step {
        label: "Save it",
        ticks_on: Some(Action::Save),
        show_me: ShowMe::Perform(Action::Save),
    },
    Step {
        label: "Export a file",
        ticks_on: None,
        show_me: ShowMe::Export,
    },
];

/// AU18: a comic's five, the same way.
pub const COMIC_STEPS: [Step; 5] = [
    Step {
        label: "Draw on the page",
        ticks_on: None,
        show_me: ShowMe::Circle,
    },
    Step {
        label: "Cut it into panels",
        ticks_on: None,
        show_me: ShowMe::Panels,
    },
    Step {
        label: "Add a page",
        ticks_on: Some(Action::NewPage),
        show_me: ShowMe::Perform(Action::NewPage),
    },
    Step {
        label: "Save it",
        ticks_on: Some(Action::Save),
        show_me: ShowMe::Perform(Action::Save),
    },
    Step {
        label: "Export pages",
        ticks_on: None,
        show_me: ShowMe::Export,
    },
];

/// AU18: the five of this kind. Unset, an animation's.
pub fn steps_of(kind: Option<ProjectKind>) -> &'static [Step; 5] {
    match kind {
        Some(ProjectKind::Drawing) => &DRAWING_STEPS,
        Some(ProjectKind::Comic) => &COMIC_STEPS,
        Some(ProjectKind::Animation) | None => &STEPS,
    }
}

/// The drawing steps' ticks: a finished stroke ticks the first, and one past
/// the first frame ticks the third as well (spec 8.3).
pub const FIRST_STROKE: usize = 0;
pub const NEXT_POSE: usize = 2;
/// Ticked when playback runs.
pub const PLAY_STEP: usize = 3;
/// The export step (ticked when a finished export's file is on disk).
pub const EXPORT_STEP: usize = 4;

/// The checklist rows: a tick, the step, its chord (from `binds`, so the
/// keys are learnt with the step) and "Show me". Returns the step whose
/// button was clicked.
pub fn checklist_ui(
    ui: &mut egui::Ui,
    rows: &[Step; 5],
    ticks: &[bool; 5],
    binds: &[crate::config::Binding],
) -> Option<usize> {
    let mut clicked = None;
    for (i, step) in rows.iter().enumerate() {
        ui.horizontal(|ui| {
            let mark = if ticks[i] { "✔" } else { "○" };
            ui.label(egui::RichText::new(mark).color(if ticks[i] {
                plate::INK
            } else {
                plate::legend_dim()
            }));
            ui.label(format!("{}. {}", i + 1, step.label));
            let verb = match step.show_me {
                ShowMe::Perform(a) => Some(a),
                ShowMe::Play => Some(Action::PlayPause),
                ShowMe::Panels => Some(Action::PanelTool),
                _ => None,
            };
            if let Some(chord) = verb.and_then(|a| crate::cmdpal::chord_of(binds, a)) {
                ui.label(egui::RichText::new(chord).color(plate::legend_dim()));
            }
            if ui
                .small_button("Show me")
                .on_hover_text(match step.show_me {
                    ShowMe::Circle => "draws a circle on this frame; Undo takes it back".into(),
                    ShowMe::NextCircle => "draws the next circle on a drawing of its own".into(),
                    ShowMe::Perform(a) => format!("does it: {}", a.label()),
                    ShowMe::Play => format!("does it: {}", Action::PlayPause.label()),
                    ShowMe::Export => "opens the Export step, with its first choice ringed".to_string(),
                    ShowMe::Panels => format!("does it: {}", Action::PanelTool.label()),
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WelcomeAction {
    Template(usize),
    /// AU6: a card on the Start float: an entry of `templates::INTENTS`.
    Intent(usize),
    /// AU20: a card on the Lessons page: an entry of the page's table.
    Lesson(usize),
    Custom,
    Open,
    OpenRecent(PathBuf),
    Remove(PathBuf),
    Sample,
    /// "Show me" on a Quick start step: open the sample, then do the step.
    ShowMe(usize),
    /// Settings… (the shell's window, over the screen).
    Settings,
    /// Close the screen shown over an open cut (Help ▸ Start screen…).
    Back,
}

impl WelcomeAction {
    /// This choice puts another document in place of the open one, so the
    /// unsaved guard asks first. The rest ask for themselves, or drop nothing.
    pub fn replaces_document(&self) -> bool {
        use WelcomeAction::*;
        matches!(self, Template(_) | Intent(_) | Lesson(_) | OpenRecent(_) | Sample | ShowMe(_))
    }
}

// ---- What's New (GM19) -----------------------------------------------------

/// This build's version, as the What's New tab and Help ▸ About say it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The screen's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WelcomeTab {
    #[default]
    Start,
    /// What this release brings, on one page — the tab the first launch of a
    /// new version opens on.
    WhatsNew,
    /// AU20: the grid of lessons, on its own scroll.
    Lessons,
}

/// Whether the What's New tab is due: `seen` (the config's last shown
/// version) is not this build's.
pub fn whats_new_due(seen: &str) -> bool {
    seen != VERSION
}

/// GM19: the Welcome screen the start-up route opens — on What's New while
/// it is due (a config an earlier version wrote; a fresh install's starts
/// seen, see `Config::load_from`), recording this version as seen; on Start
/// after that. True when `ui` changed: the caller saves the config, or the
/// tab would open on every launch.
pub fn open_at_startup(ui: &mut crate::config::UiConfig, recent: &[PathBuf]) -> (Welcome, bool) {
    let mut w = Welcome::new(recent);
    let due = whats_new_due(&ui.whats_new_seen);
    if due {
        w.tab = WelcomeTab::WhatsNew;
        ui.whats_new_seen = VERSION.into();
    }
    (w, due)
}

/// AU17: the two chords this release moved (spec 6.5), said by their live
/// keys, so a person's own binding reads as it stands.
pub const MOVED_CHORDS: [Action; 2] = [Action::ColumnKindBook, Action::ColumnKindBg];
/// The keys they left, by `MOVED_CHORDS`' order: the ones a hand still reaches
/// for, the keys `Config::migrate_keymap` moved them from.
const LEFT_CHORDS: [&str; 2] = ["Alt+K", "Alt+B"];
/// AU17: one line on the bars and one on the lessons (feedback 1.11).
pub const WHATS_NEW_BARS: &str = "An animation opens on Plan with its sheet, a drawing on Draw, a comic on Manga.";
pub const WHATS_NEW_LESSONS: &str = "Nine lessons make something real, rookie to experienced: Help ▸ Lessons.";
/// The word list's columns, a row's height (one line of panel text), and the
/// gap between columns.
pub(crate) const WORD_COLUMNS: usize = 3;
const WORD_ROW: f32 = 20.0;
const WORD_GAP: f32 = plate::CARD_PAD;

/// AU17: the key a moved chord left, and where it stands now.
fn moved_chord(ctx: &egui::Context, action: Action, left: &str) -> String {
    let name = action.label();
    match crate::menus::first_chord(ctx, action) {
        Some(chord) if chord == left => format!("{name} stays on {chord}"),
        Some(chord) => format!("{name} was {left}, now {chord}"),
        None => format!("{name} was {left}, now no key"),
    }
}

/// AU17: the What's New page under its heading, one screen: the bars, the
/// lessons, the moved chords, and the old words beside the new. It stands
/// unscrolled in the float's cap.
fn whats_new_ui(ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(WHATS_NEW_BARS).color(plate::INK));
    ui.label(egui::RichText::new(WHATS_NEW_LESSONS).color(plate::INK));
    let moved = MOVED_CHORDS.iter().zip(LEFT_CHORDS);
    let moved: Vec<String> = moved.map(|(action, left)| moved_chord(ui.ctx(), *action, left)).collect();
    ui.label(egui::RichText::new(format!("Keys moved: {}.", moved.join("; "))).color(plate::INK));
    ui.add_space(GROUP_GAP);
    // The rows are text, not controls: a grid's default row is a control's
    // height, which would push the last row off the card.
    let grid = egui::Grid::new("whats_new_words").num_columns(WORD_COLUMNS);
    grid.min_row_height(WORD_ROW).spacing([WORD_GAP, 0.0]).show(ui, |ui| {
        for row in crate::vocab::OLD_WORDS.chunks(WORD_COLUMNS) {
            for (old, new) in row {
                let font = plate::Role::Panel.font();
                let mut job = egui::text::LayoutJob::default();
                job.append(&format!("{old} → "), 0.0, egui::TextFormat::simple(font.clone(), plate::INK2));
                job.append(new, 0.0, egui::TextFormat::simple(font, plate::INK));
                ui.label(job);
            }
            ui.end_row();
        }
    });
}

#[derive(Default)]
pub struct Welcome {
    pub cards: Vec<RecentCard>,
    textures: HashMap<PathBuf, Option<egui::TextureHandle>>,
    /// Shown again over an open cut (Help ▸ Start screen…): a modal with
    /// a way back, and the cut stays open until a card replaces it.
    pub over_editor: bool,
    /// GM19: the tab shown (What's New on a new version's first launch).
    pub tab: WelcomeTab,
    /// AU6: `More sizes` is open.
    pub more_open: bool,
    /// AU6: how `Check your pen` stands.
    pub pen: PenCheck,
    /// AU6: the answer just came in: the body scrolls down to it, once.
    pub pen_said: bool,
    /// AU20: the Lessons page's table. Empty, the page cannot be reached.
    pub lessons: &'static [Lesson],
    /// AU20: each lesson's picture, built once by its id.
    pictures: HashMap<&'static str, Picture>,
    /// AU27: the ids in `lessons.txt`.
    pub finished: Vec<String>,
    /// AU27: the ticks the page drew last: painted, no control.
    pub ticks_painted: usize,
}

/// AU6: `Check your pen`: the next line drawn is read, and the row below says
/// what it found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PenCheck {
    #[default]
    Idle,
    /// Asked: the next finished stroke answers, and the float stays up for it.
    Waiting,
    Works,
    /// A tablet's stream, moving, that never carried force.
    NoPressure,
    /// The line came in as a mouse. A tablet with Windows Ink off comes in
    /// the same way, and nothing tells the two apart.
    Mouse,
}

impl PenCheck {
    /// What the row says now. Pressing it asks again.
    pub fn words(self) -> &'static str {
        match self {
            PenCheck::Waiting => "Draw one line",
            _ => "Check your pen",
        }
    }

    /// The answer shown under the row, in words and with its fix.
    pub fn verdict(self) -> Option<(bool, &'static str, Option<&'static str>)> {
        match self {
            PenCheck::Idle | PenCheck::Waiting => None,
            PenCheck::Works => Some((true, "Pressure works", None)),
            PenCheck::NoPressure => Some((false, "No pressure", Some(PEN_FIX))),
            PenCheck::Mouse => Some((false, "No pressure", Some(MOUSE_FIX))),
        }
    }

    /// What a finished stroke does to the check, `heard` being what
    /// `pen_verdict` read off it. None: the stroke was no check line; it
    /// stands, and ends the float. A check that found no pressure keeps
    /// listening for the fixed pen, but a mouse goes on drawing: a second
    /// mouse line is a real one.
    pub fn read(self, heard: PenCheck) -> Option<PenCheck> {
        match (self, heard) {
            (PenCheck::Idle | PenCheck::Works, _) | (PenCheck::Mouse, PenCheck::Mouse) => None,
            // A tap has nothing to read: keep asking.
            (_, PenCheck::Waiting) => Some(self),
            _ => Some(heard),
        }
    }
}

/// AU6: the plain fix under `No pressure`, for a tablet that reports flat.
pub const PEN_FIX: &str = "Turn on Windows Ink in your tablet's settings, then draw one line again.";
/// The same answer to a mouse, for the tablet that may be one.
pub const MOUSE_FIX: &str = "A mouse has no pressure. You can still draw. For a tablet pen, turn on \
                             Windows Ink in the tablet's settings, then draw one line again.";

impl Welcome {
    pub fn new(recent: &[PathBuf]) -> Self {
        Self {
            cards: recent_cards(recent),
            textures: HashMap::new(),
            over_editor: false,
            tab: WelcomeTab::Start,
            more_open: false,
            pen: PenCheck::Idle,
            pen_said: false,
            lessons: lessons::ALL,
            pictures: HashMap::new(),
            finished: Vec::new(),
            ticks_painted: 0,
        }
    }

    /// AU20: the float's heading on this page.
    pub fn heading(&self) -> &'static str {
        match self.tab {
            WelcomeTab::Start => HEADING,
            WelcomeTab::WhatsNew => WHATS_NEW_HEADING,
            WelcomeTab::Lessons => LESSONS_HEADING,
        }
    }

    /// The screen again over an open cut.
    pub fn over_editor(recent: &[PathBuf]) -> Self {
        Self {
            over_editor: true,
            ..Self::new(recent)
        }
    }

    /// AU20: on the Lessons page, while there are lessons to show.
    pub fn on_lessons(mut self) -> Self {
        if !self.lessons.is_empty() {
            self.tab = WelcomeTab::Lessons;
        }
        self
    }

    /// Drop a card (Remove) without touching the disk.
    #[cfg(test)]
    pub fn forget(&mut self, path: &std::path::Path) {
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
    ui.label(egui::RichText::new(text).strong().color(plate::INK2));
}

/// A card on the float, and the gap between cards.
const CARD_H: f32 = 44.0;
const CARD_GAP: f32 = 8.0;
/// The chips under `A drawing`: from the card, and from each other.
const CHIP_GAP: f32 = 4.0;
/// The float's rows stand this far apart, and a group this much further from
/// the next.
const ROW_GAP: f32 = 2.0;
const GROUP_GAP: f32 = 4.0;
/// `Open the example`'s hover, on either page.
const EXAMPLE_CARD: &str = "A ball bounce to look through and draw on";
/// The Lessons page (spec 4.5): the legends over its columns, the words of its
/// gutter of kinds, its rows and the gap between them. Under a cap of 300 the
/// head and the margins leave the body 229: rows of 56, not 60, keep the grid
/// unscrolled.
const PAGE_LEGEND_H: f32 = 16.0;
const PAGE_GUTTER: f32 = 84.0;
pub const PAGE_ROW_H: f32 = 56.0;
const PAGE_GAP: f32 = 6.0;

/// What is no card or row on the float, on the one edge their words start at.
fn on_edge<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let edge = egui::Margin::symmetric(plate::CARD_PAD as i8, 0);
    egui::Frame::new().inner_margin(edge).show(ui, add).inner
}

/// AU6: the Start float's body, inside the frame's scroll: spec 8.1's entries
/// in its budget's order. AU20: the latch stands in the heading row, so it
/// never needs a scroll, and the recents stand as a half row, so a returning
/// animator's files stand whole under the cap.
pub fn float_body(ui: &mut egui::Ui, w: &mut Welcome) -> Option<WelcomeAction> {
    let wide = ui.available_width();
    ui.spacing_mut().item_spacing.y = ROW_GAP;
    if w.tab == WelcomeTab::WhatsNew {
        // AU17: the way back leads the page, as it does on the Lessons page;
        // the float's heading is the page's title.
        let half = (wide - ui.spacing().item_spacing.x) / 2.0;
        if plate::filled_row(ui, "Back to Start", half).clicked() {
            w.tab = WelcomeTab::Start;
        }
        ui.add_space(GROUP_GAP);
        on_edge(ui, whats_new_ui);
        return None;
    }
    if w.tab == WelcomeTab::Lessons {
        return lessons_page(ui, w, wide);
    }
    let mut out = None;
    intent_cards(ui, wide, &mut out);
    ui.add_space(GROUP_GAP);
    if w.lessons.is_empty() {
        let sample = plate::choice_card(ui, "Open the example", CARD_H);
        if sample.card(EXAMPLE_CARD).clicked() {
            out = Some(WelcomeAction::Sample);
        }
    } else {
        let lessons = plate::choice_card(ui, "Follow a lesson", CARD_H);
        if lessons.card("Learn by making something, step by step").clicked() {
            w.tab = WelcomeTab::Lessons;
        }
    }
    ui.add_space(GROUP_GAP);
    let half = (wide - ui.spacing().item_spacing.x) / 2.0;
    ui.horizontal(|ui| {
        let open = plate::ghost_row(ui, "Open a project", half, None);
        if open.card("Open a project saved on this computer").clicked() {
            out = Some(WelcomeAction::Open);
        }
        if plate::fold_row(ui, "More sizes", half, w.more_open).clicked() {
            w.more_open = !w.more_open;
        }
    });
    if w.more_open {
        more_sizes(ui, wide, &mut out);
    }
    recent_rows(ui, w, half, &mut out);
    // The card stands at its cap, so the foot takes the card's last line while
    // the body leaves room, and an answer takes that room from it.
    let last = ui.clip_rect().bottom() - ui.visuals().clip_rect_margin - plate::CTRL_H;
    let free = if w.pen.verdict().is_none() { last - ui.cursor().top() } else { 0.0 };
    ui.add_space(free.max(GROUP_GAP));
    ui.horizontal(|ui| {
        let check = if w.pen == PenCheck::Waiting {
            plate::current_row(ui, w.pen.words(), half)
        } else {
            plate::ghost_row(ui, w.pen.words(), half, None)
        };
        if check.card("Draw one line and see whether your pen reports pressure").clicked() {
            w.pen = PenCheck::Waiting;
        }
        if plate::ghost_row(ui, "What's new", half, None).clicked() {
            w.tab = WelcomeTab::WhatsNew;
        }
    });
    if let Some((yes, said, fix)) = w.pen.verdict() {
        on_edge(ui, |ui| {
            plate::answer(ui, yes, said);
            if let Some(fix) = fix {
                ui.label(egui::RichText::new(fix).color(plate::INK2));
            }
        });
    }
    // To the body's end, not the answer's: that leaves its last line in the fade.
    if std::mem::take(&mut w.pen_said) {
        ui.scroll_to_cursor(Some(egui::Align::Max));
    }
    out
}

/// The float's three cards, across. `A drawing`'s other two sizes stand under
/// it: it and its chips miss one card's width (spec 4.3).
fn intent_cards(ui: &mut egui::Ui, wide: f32, out: &mut Option<WelcomeAction>) {
    let across = (wide - 2.0 * CARD_GAP) / 3.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(CARD_GAP, CHIP_GAP);
        ui.vertical(|ui| {
            ui.set_width(across);
            let t = &templates::INTENTS[templates::PICTURE];
            let drawing = plate::choice_card(ui, "A drawing", CARD_H);
            if drawing.card(format!("One picture, {}×{}", t.width, t.height)).clicked() {
                *out = Some(WelcomeAction::Intent(templates::PICTURE));
            }
            let (row, _) = ui.allocate_exact_size(egui::vec2(across, plate::CHIP_H), egui::Sense::hover());
            let half = egui::vec2((across - CHIP_GAP) / 2.0, row.height());
            let sizes = [(row.left(), "Square", templates::PICTURE_SQUARE), (row.right() - half.x, "Tall", templates::PICTURE_TALL)];
            for (left, word, intent) in sizes {
                let t = &templates::INTENTS[intent];
                let at = egui::Rect::from_min_size(egui::pos2(left, row.top()), half);
                if plate::card_chip(ui, at, word).card(format!("{}×{}", t.width, t.height)).clicked() {
                    *out = Some(WelcomeAction::Intent(intent));
                }
            }
        });
        ui.vertical(|ui| {
            ui.set_width(across);
            let animation = plate::choice_card(ui, "An animation", CARD_H);
            if animation.card("Drawings that move, on new paper").clicked() {
                *out = Some(WelcomeAction::Intent(templates::ANIMATION));
            }
        });
        ui.vertical(|ui| {
            ui.set_width(across);
            let t = &templates::ALL[templates::COMIC];
            let comic = plate::choice_card(ui, "A comic", CARD_H);
            if comic.card(format!("A page: {}", t.name)).clicked() {
                *out = Some(WelcomeAction::Template(templates::COMIC));
            }
        });
    });
}

/// AU20: the Lessons page: its two ways out, then a card for each kind and
/// class, in the same float at the same size (spec 4.5).
fn lessons_page(ui: &mut egui::Ui, w: &mut Welcome, wide: f32) -> Option<WelcomeAction> {
    let mut out = None;
    let half = (wide - ui.spacing().item_spacing.x) / 2.0;
    ui.horizontal(|ui| {
        if plate::filled_row(ui, "Back to Start", half).clicked() {
            w.tab = WelcomeTab::Start;
        }
        let sample = plate::filled_row(ui, "Open the example", half);
        if sample.card(EXAMPLE_CARD).clicked() {
            out = Some(WelcomeAction::Sample);
        }
    });
    let lessons = w.lessons;
    w.ticks_painted = 0;
    let gutter_right = plate::CARD_PAD + PAGE_GUTTER;
    let across = (wide - gutter_right - 3.0 * PAGE_GAP) / 3.0;
    let tall = PAGE_LEGEND_H + ROW_GAP + 3.0 * PAGE_ROW_H + 2.0 * PAGE_GAP;
    let (grid, _) = ui.allocate_exact_size(egui::vec2(wide, tall), egui::Sense::hover());
    let column = |c: usize| grid.left() + gutter_right + PAGE_GAP + c as f32 * (across + PAGE_GAP);
    for (c, class) in Class::ALL.into_iter().enumerate() {
        let at = egui::pos2(column(c) + plate::PICTURE_INSET, grid.top());
        let cell = egui::Rect::from_min_size(at, egui::vec2(across, PAGE_LEGEND_H));
        ui.scope_builder(egui::UiBuilder::new().max_rect(cell), |ui| plate::legend(ui, class.word()));
    }
    let kinds = [(ProjectKind::Drawing, "A drawing"), (ProjectKind::Animation, "An animation"), (ProjectKind::Comic, "A comic")];
    for (r, (kind, word)) in kinds.into_iter().enumerate() {
        let top = grid.top() + PAGE_LEGEND_H + ROW_GAP + r as f32 * (PAGE_ROW_H + PAGE_GAP);
        let gutter = egui::Rect::from_min_size(
            egui::pos2(grid.left() + plate::CARD_PAD, top),
            egui::vec2(PAGE_GUTTER, PAGE_ROW_H),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(gutter).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            plate::legend(ui, word);
        });
        for (c, class) in Class::ALL.into_iter().enumerate() {
            let Some(i) = lessons.iter().position(|l| l.kind == kind && l.class == class) else { continue };
            let lesson = &lessons[i];
            let at = egui::pos2(column(c), top);
            let rect = egui::Rect::from_min_size(at, egui::vec2(across, PAGE_ROW_H));
            let picture = w.pictures.entry(lesson.id).or_insert_with(|| Picture::small(lesson));
            let card = plate::picture_card(ui, rect, lesson.title, |p, paper| picture.paint(p, paper));
            if w.finished.iter().any(|id| id == lesson.id) {
                plate::finished_tick(ui.painter(), rect);
                w.ticks_painted += 1;
            }
            if card.card(lesson.hover).clicked() {
                out = Some(WelcomeAction::Lesson(i));
            }
        }
    }
    out
}

/// The least a press stands at, as a radius, on a picture this small.
const PRESS_SMALLEST: f32 = 1.5;

/// A lesson's finish, read once: `art` is the whole lesson's paths, too much
/// to build on each repaint.
pub(crate) struct Picture {
    /// Each line, its width, and whether it is a press: a dab drawn as a line
    /// is a tick.
    lines: Vec<(Vec<egui::Pos2>, f32, bool)>,
    bounds: egui::Rect,
}

impl Picture {
    pub(crate) fn of(lesson: &Lesson) -> Self {
        Self::from_paths(lesson, if lesson.card.is_empty() { lesson.finish } else { lesson.card })
    }

    /// AU23: the Lessons page's, at a row's height.
    pub(crate) fn small(lesson: &Lesson) -> Self {
        Self::from_paths(lesson, if lesson.thumb.is_empty() { lesson.finish } else { lesson.thumb })
    }

    fn from_paths(lesson: &Lesson, paths: &[u16]) -> Self {
        let art = (lesson.art)();
        let at = |q: &[f32; 3]| egui::pos2(q[0], q[1]);
        let paths = paths.iter().filter_map(|i| art.get(*i as usize));
        let lines: Vec<_> = paths.map(|path| (path.points.iter().map(at).collect::<Vec<_>>(), path.width, path.is_press())).collect();
        let mut bounds = egui::Rect::NOTHING;
        lines.iter().flat_map(|(line, _, _)| line).for_each(|q| bounds.extend_with(*q));
        Self { lines, bounds }
    }

    /// Drawn small: fitted to `paper` and kept to its own shape.
    pub(crate) fn paint(&self, p: &egui::Painter, paper: egui::Rect) {
        if !self.bounds.is_positive() {
            return;
        }
        let room = paper.shrink(6.0);
        let scale = (room.width() / self.bounds.width()).min(room.height() / self.bounds.height());
        let placed = |q: &egui::Pos2| room.center() + (*q - self.bounds.center()) * scale;
        for (line, width, press) in &self.lines {
            match line.first() {
                Some(dot) if *press => {
                    p.circle_filled(placed(dot), (width * scale / 2.0).max(PRESS_SMALLEST), plate::INK);
                }
                _ => {
                    let points = line.iter().map(placed).collect();
                    p.add(egui::Shape::line(points, egui::Stroke::new((width * scale).max(1.0), plate::INK)));
                }
            }
        }
    }
}

/// `More sizes`: every template, then the form, under their own legend: the
/// list opens under `Open a project`, and it is none of those.
fn more_sizes(ui: &mut egui::Ui, wide: f32, out: &mut Option<WelcomeAction>) {
    on_edge(ui, |ui| plate::legend(ui, "Sizes"));
    for (i, t) in templates::ALL.iter().enumerate() {
        let row = plate::noted_row(ui, t.name, &format!("{}×{}", t.width, t.height), wide);
        if row.card(format!("{}\n{}", t.summary(), t.tooltip())).clicked() {
            *out = Some(WelcomeAction::Template(i));
        }
    }
    let custom = plate::ghost_row(ui, "Custom size", wide, None);
    if custom.card("Name, size and frame rate of your own").clicked() {
        *out = Some(WelcomeAction::Custom);
    }
}

/// The last files still on disk, by name, side by side, `half` wide each;
/// hidden when there are none.
fn recent_rows(ui: &mut egui::Ui, w: &Welcome, half: f32, out: &mut Option<WelcomeAction>) {
    let mut rows = w.cards.iter().filter(|c| !c.missing).take(RECENT_ROWS).peekable();
    if rows.peek().is_none() {
        return;
    }
    ui.add_space(GROUP_GAP);
    on_edge(ui, |ui| plate::legend(ui, "Recent"));
    ui.horizontal(|ui| {
        for c in rows {
            let name = c.path.file_stem().map(|s| s.to_string_lossy().into_owned());
            let name = name.unwrap_or_else(|| c.path.display().to_string());
            let row = plate::ghost_row(ui, &name, half, None);
            if row.card(c.path.display().to_string()).clicked() {
                *out = Some(WelcomeAction::OpenRecent(c.path.clone()));
            }
        }
    });
}

/// The Welcome screen, drawn into the central area (or a modal over an
/// open cut). `binds` names the chords beside Open… and the checklist. AU6:
/// the Start float took its place on screen; its tests still read it here.
#[cfg_attr(not(test), allow(dead_code))]
pub fn ui(
    ui: &mut egui::Ui,
    w: &mut Welcome,
    show_at_startup: &mut bool,
    ticks: &[bool; 5],
    binds: &[crate::config::Binding],
) -> Option<WelcomeAction> {
    let mut out = None;
    plate::surface(ui);
    let scroll = egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.heading("Gaban Studio");
            // GM19: the tabs sit in the heading's row, so the Start tab's
            // cards stay where they were.
            if plate::detent(ui, w.tab == WelcomeTab::Start, "Start")
                .on_hover_text("templates, recent shots and the sample")
                .clicked()
            {
                w.tab = WelcomeTab::Start;
            }
            if plate::detent(ui, w.tab == WelcomeTab::WhatsNew, "What's new")
                .on_hover_text("What this release brings, on one page")
                .clicked()
            {
                w.tab = WelcomeTab::WhatsNew;
            }
            if w.over_editor
                && ui
                    .button("Back to the shot")
                    .on_hover_text("closes this screen; the open shot is untouched — Esc")
                    .clicked()
            {
                out = Some(WelcomeAction::Back);
            }
        });
        ui.add_space(8.0);
        if w.tab == WelcomeTab::WhatsNew {
            section(ui, WHATS_NEW_HEADING);
            whats_new_ui(ui);
            return;
        }
        section(ui, "New from template");
        ui.horizontal_wrapped(|ui| {
            for (i, t) in templates::ALL.iter().enumerate() {
                ui.vertical(|ui| {
                    if ui.button(t.name).on_hover_text(t.tooltip()).clicked() {
                        out = Some(WelcomeAction::Template(i));
                    }
                    ui.label(
                        egui::RichText::new(t.summary())
                            .font(plate::Role::Small.font())
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
                .button("Open the sample shot")
                .on_hover_text("a ball bounce on twos over a background plate — opens untitled")
                .clicked()
            {
                out = Some(WelcomeAction::Sample);
            }
            if ui
                .button("Settings…")
                .on_hover_text("UI scale, pen and shortcuts")
                .clicked()
            {
                out = Some(WelcomeAction::Settings);
            }
        });
        ui.add_space(8.0);
        section(ui, "Recent files");
        if w.cards.is_empty() {
            ui.label(egui::RichText::new("Nothing yet").color(plate::legend_dim()));
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
                                .font(plate::Role::Small.font())
                                .color(plate::legend_dim()),
                            );
                        }
                    }
                });
            }
        });
        ui.add_space(8.0);
        section(ui, "Quick start");
        if let Some(step) = checklist_ui(ui, &STEPS, ticks, binds) {
            out = Some(WelcomeAction::ShowMe(step));
        }
        ui.add_space(8.0);
        plate::latch(ui, show_at_startup, "Show at startup").on_hover_text(
            "off: start-up opens the New Project form; Settings ▸ General turns it back on",
        );
    });
    plate::note_viewport(ui, scroll.inner_rect, [false, true]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Stage;
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
        let s = crate::sample::build_auto_comp();
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
            *h.editor_mut() = crate::open_template(i, None, None, &Default::default());
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
            "Open the sample shot",
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
        assert!(h.has(crate::vocab::STEP_EFFECTS));
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
        assert_eq!(h.editor().quick_start, [false; 5], "AU7: a drawing is no stroke yet");
        crate::canvas::extpaper_tests::paint_texels(h.state_mut(), &[(9, 9)], [0.0, 0.0, 0.0, 1.0]);
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
        let mut h = crate::testkit::EditorHarness::with_state(crate::sample::build_auto_comp());
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
            "Replace the Effects graph ({n} nodes)? Undo brings it back."
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
        assert!(Action::BuildComposite.help().starts_with("Replace the Effects graph"));
    }

    #[test]
    fn welcome_screen_comes_back_after_startup() {
        // Help ▸ Start screen…: the float opens at once, clean or not, and
        // the open cut stands under it: only a card that replaces it asks.
        let mut s = crate::sample::build_sample();
        s.saved_rev = s.engine.revision();
        let mut h = crate::testkit::EditorHarness::with_state(s);
        assert_eq!(Action::WelcomeScreen.menu(), Some("Help/Start screen…"));
        assert_eq!(h.perform(Action::WelcomeScreen), crate::Performed::Done);
        assert!(h.editor().start.as_ref().is_some_and(|w| w.over_editor));
        h.editor_mut().start = None;
        h.state_mut().add_column();
        assert!(h.state().is_dirty());
        let (rev, group) = (h.state().engine.revision(), h.state().untitled_group.clone());
        assert_eq!(h.perform(Action::WelcomeScreen), crate::Performed::Done);
        h.run();
        assert!(h.editor().start.is_some() && !h.editor().leave_pending());
        assert_eq!((h.state().engine.revision(), h.state().untitled_group.clone()), (rev, group));
        assert!(h.state().is_dirty());
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
        assert!(h.query_by_label("Show Start at startup").is_some());
    }

    #[test]
    fn welcome_startup_route_picks_the_screen() {
        use crate::{Startup, startup_route};
        let mut c = crate::config::Config::default();
        // Someone who has read this version's What's New: while it is due it
        // stands whatever the latch says (`aurel_upgrader_sees_whats_new_once`).
        c.ui.whats_new_seen = VERSION.into();
        // Nothing open: the Welcome screen.
        assert_eq!(startup_route(false, false, None, &c), Startup::Welcome);
        // The latch off: the same paper with no float (AU6; a form before).
        c.ui.show_welcome = false;
        assert_eq!(startup_route(false, false, None, &c), Startup::ReadyPaper);
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
        let (ed, did) = crate::welcome_show_me(0, None, None, &Default::default());
        assert_eq!(did, crate::Performed::Done);
        assert!(ed.quick_start[0], "step 1 ticked");
        let b = ed.state.cut().xsheet.column(ed.state.view.active_column).unwrap();
        assert_eq!(b.name, "B");
        assert!(ed.state.current_drawing().is_some());
        // Every step does something from the screen it is offered on.
        for (i, step) in STEPS.iter().enumerate() {
            let (ed, did) = crate::welcome_show_me(i, None, None, &Default::default());
            assert_eq!(did, crate::Performed::Done, "step {}", i + 1);
            if let Some(a) = step.ticks_on.filter(|a| *a != Action::MakeInbetweens) {
                assert!(ed.quick_start[i], "step {} ({a:?}) ticked", i + 1);
            }
        }
        // The sample still renders as before: B is empty. GM19: the
        // showcase draws the practice cut as it draws the sample, and under
        // the auto-comp its graph is its sheet.
        let mut s = crate::sample::build_practice();
        let (w, h) = crate::sample::SIZE;
        let law = s.engine.project.color_law;
        let sample = crate::sample::build_sample();
        assert!(
            render_graph_frame_law(s.cut(), law, 0, w, h)
                == render_graph_frame_law(sample.cut(), law, 0, w, h)
        );
        assert!(s.auto_comp());
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
