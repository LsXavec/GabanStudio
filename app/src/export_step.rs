//! AU9: THE EXPORT PANEL (spec 5.5), the top of `Pane::RenderQueue`: one
//! layout wherever the pane stands. The choices follow the document, a job
//! that finishes in the Export step leaves a Done card made from its file on
//! disk, and the queue's own panel waits under `More options`. A pane cannot
//! reach the Editor, so what this one asks for waits in [`Panel::asked`] for
//! the drain after the dock, as the other panes' verbs do.

use eframe::egui;

use crate::config::Action;
use crate::doc::AppState;
use crate::plate::{self, Card as _};
use crate::render_queue::{self, JobState, Landed};
use crate::{AppRequest, say};

/// A choice's height: a Start card's.
pub const CHOICE_H: f32 = 44.0;
/// Between choices: more than the ring takes.
const CHOICE_GAP: f32 = 8.0;
/// Round the pane's scrolling, inside its clip: Show me's ring stands whole.
pub const RING_ROOM: i8 = 6;

/// The empty state (spec 8.4).
pub const NO_ART: &str = "Nothing to export yet. Draw something first.";
/// The job list with nothing in it (spec 8.4).
pub const NOTHING_WAITING: &str = "Nothing waiting.";
/// The Video row with no ffmpeg on this PC (spec 5.5).
pub const NO_FFMPEG: &str = "Video needs a free add-on on this PC. GIF works now.";
pub const GIF_INSTEAD: &str = "Export GIF instead";
pub const MORE_OPTIONS: &str = "More options";
pub const OTHER_FORMATS: &str = "Other formats";
/// A one-frame document's choice, with `Action::ExportStill` behind it.
pub const PNG_IMAGE: &str = "PNG image";
pub const OPEN_FILE: &str = "Open file";
pub const SHOW_IN_FOLDER: &str = "Show in folder";
pub const STOP_EXPORT: &str = "Stop export";
pub const TRANSPARENT: &str = "Transparent";

/// What the document offers: spec 5.5's table, read fresh each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    NoArt,
    Comic,
    OneFrame,
    Animation,
}

impl Offer {
    pub fn of(state: &AppState) -> Offer {
        if !state.has_art() {
            Offer::NoArt
        } else if anim_core::manga::Page::of(state.cut()).is_some() {
            Offer::Comic
        } else if state.frame_count() > 1 {
            Offer::Animation
        } else {
            Offer::OneFrame
        }
    }
}

/// What the panel asks of the Editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Through `Editor::invoke`, as a menu row runs it.
    Do(Action),
    /// `Back to Draw`: the step it came from, as it was left.
    Back,
    App(AppRequest),
}

/// The panel's own state, on the queue.
#[derive(Default)]
pub struct Panel {
    /// Waits for the drain after the dock.
    pub asked: Vec<Ask>,
    /// The Done card. Only a job that finishes in the Export step leaves one.
    pub done: Option<Landed>,
    /// The name the back button says.
    pub back_to: String,
    /// Show me's ring on the first choice, until something is pressed.
    pub ring: bool,
    /// Where the ring stood this frame.
    pub ring_at: Option<egui::Rect>,
    /// `More options`, once it has been pressed; before that the boot's
    /// default. Stored by the Panels part, with every other fold.
    pub more: Option<bool>,
    /// A still keeps its alpha, through the existing `PNG 8-bit`.
    pub transparent: bool,
}

impl Panel {
    /// Anything pressed answers Show me's ring.
    fn ask(&mut self, ask: Ask) {
        self.ring = false;
        self.asked.push(ask);
    }
}

/// The panel. True when `More options` is open, and the queue's own panel is
/// to follow it. `more_default`: the fold before any press.
pub fn ui(ui: &mut egui::Ui, state: &mut AppState, more_default: bool) -> bool {
    state.render_queue.panel.ring_at = None;
    let wide = ui.available_width();
    // The queue's panel after this keeps its own rhythm.
    let rhythm = std::mem::replace(&mut ui.spacing_mut().item_spacing.y, CHOICE_GAP);
    if let Some(landed) = state.render_queue.panel.done.clone() {
        done_card(ui, state, &landed, wide);
    }
    // The Done card's `Open file` is the screen's one primary while it stands.
    let lead = state.render_queue.panel.done.is_none();
    match Offer::of(state) {
        Offer::NoArt => no_art(ui, state),
        Offer::Comic => {
            lead_choice(ui, state, Action::ExportPages.label(), Action::ExportPages, lead);
            other_formats(ui, state, wide);
        }
        Offer::OneFrame => {
            lead_choice(ui, state, PNG_IMAGE, Action::ExportStill, lead);
            other_formats(ui, state, wide);
        }
        Offer::Animation => {
            lead_choice(ui, state, Action::ExportGif.label(), Action::ExportGif, lead);
            video_row(ui, state);
            choice(ui, state, Action::ExportPngSequence);
            choice(ui, state, Action::ExportStill);
        }
    }
    job_list(ui, state, wide);
    let open = fold(ui, state, more_default, wide);
    ui.spacing_mut().item_spacing.y = rhythm;
    open
}

/// The document's first choice, solid unless the Done card leads, and where
/// Show me's ring goes.
fn lead_choice(ui: &mut egui::Ui, state: &mut AppState, label: &str, action: Action, solid: bool) {
    let resp = if solid {
        plate::primary_card(ui, label, CHOICE_H)
    } else {
        plate::choice_card(ui, label, CHOICE_H)
    };
    let panel = &mut state.render_queue.panel;
    if panel.ring {
        panel.ring_at = Some(plate::ring(ui, resp.rect));
    }
    if resp.card(action.help()).clicked() {
        panel.ask(Ask::Do(action));
    }
}

fn choice(ui: &mut egui::Ui, state: &mut AppState, action: Action) {
    if plate::choice_card(ui, action.label(), CHOICE_H).card(action.help()).clicked() {
        state.render_queue.panel.ask(Ask::Do(action));
    }
}

/// Video needs ffmpeg, which is not bundled, so without it the row says so and
/// offers what works now.
fn video_row(ui: &mut egui::Ui, state: &mut AppState) {
    if state.render_queue.ffmpeg_path().is_some() {
        choice(ui, state, Action::ExportMp4);
        return;
    }
    ui.label(egui::RichText::new(NO_FFMPEG).color(plate::LEGEND));
    let wide = ui.available_width();
    if plate::ghost_row(ui, GIF_INSTEAD, wide, None).card(Action::ExportGif.help()).clicked() {
        state.render_queue.panel.ask(Ask::Do(Action::ExportGif));
    }
}

/// A short document's other choices live with the queue's.
fn other_formats(ui: &mut egui::Ui, state: &mut AppState, wide: f32) {
    let card = "Every other format, under More options.";
    if plate::ghost_row(ui, OTHER_FORMATS, wide, None).card(card).clicked() {
        let panel = &mut state.render_queue.panel;
        panel.ring = false;
        panel.more = Some(true);
    }
}

fn no_art(ui: &mut egui::Ui, state: &mut AppState) {
    ui.label(egui::RichText::new(NO_ART).color(plate::LEGEND));
    if plate::choice_card(ui, say::GO_TO_DRAW, CHOICE_H).card(Action::RoomDrawing.help()).clicked() {
        state.render_queue.panel.ask(Ask::Do(Action::RoomDrawing));
    }
}

/// `Exported ball.gif`, what it holds, and where to go from it. Every word
/// comes from the finished file and the job.
fn done_card(ui: &mut egui::Ui, state: &mut AppState, landed: &Landed, wide: f32) {
    let frame = egui::Frame::new().fill(plate::WELL).corner_radius(plate::R_CONTROL).inner_margin(plate::CARD_PAD);
    let mut ask = None;
    frame.show(ui, |ui| {
        ui.set_width(wide - 2.0 * plate::CARD_PAD);
        ui.spacing_mut().item_spacing.y = 6.0;
        let heading = format!("Exported {}", landed.name);
        ui.label(egui::RichText::new(heading).font(plate::Role::Card.font()).color(plate::INK));
        ui.label(egui::RichText::new(landed.line()).color(plate::LEGEND));
        let inner = ui.available_width();
        if plate::primary_card(ui, OPEN_FILE, plate::CTRL_H + 8.0).clicked() {
            ask = Some(Ask::App(AppRequest::OpenFile(landed.file.clone())));
        }
        if plate::ghost_row(ui, SHOW_IN_FOLDER, inner, None).clicked() {
            ask = Some(Ask::App(AppRequest::ShowInFolder(landed.file.clone())));
        }
        let back = format!("Back to {}", state.render_queue.panel.back_to);
        if plate::ghost_row(ui, &back, inner, None).clicked() {
            ask = Some(Ask::Back);
        }
    });
    if let Some(ask) = ask {
        state.render_queue.panel.ask(ask);
    }
}

/// A job as this list names it: the file a person chose, or the cut and what
/// it makes, in words a screen reader tells from the queue's own row.
fn job_name(job: &render_queue::Job) -> String {
    if job.exact_name {
        format!("{}.{}", job.pattern, job.preset().format.ext())
    } else {
        format!("{}, {}", job.label, job.preset().name)
    }
}

/// The jobs still to finish, above the fold: a person sees an export go
/// whether or not `More options` is open.
fn job_list(ui: &mut egui::Ui, state: &mut AppState, wide: f32) {
    let rq = &state.render_queue;
    let unfinished = rq.queue.jobs.iter().filter(|j| matches!(j.state, JobState::Queued | JobState::Running));
    let rows: Vec<(String, usize, usize)> = unfinished.map(|j| (job_name(j), j.done, j.total())).collect();
    if rows.is_empty() {
        ui.label(egui::RichText::new(NOTHING_WAITING).color(plate::LEGEND));
        return;
    }
    for (name, done, total) in rows {
        ui.label(egui::RichText::new(name).color(plate::INK));
        ui.horizontal(|ui| {
            let count = format!("{done} of {total}");
            let width = ui.fonts_mut(|f| {
                let font = egui::TextStyle::Body.resolve(ui.style());
                f.layout_no_wrap(count.clone(), font, plate::LEGEND).size().x
            });
            plate::meter(ui, Some(done as f32 / total.max(1) as f32), (wide - width - 8.0).max(40.0));
            ui.label(egui::RichText::new(count).color(plate::LEGEND));
        });
    }
    if state.render_queue.busy() && plate::ghost_row(ui, STOP_EXPORT, wide, None).clicked() {
        render_queue::stop(state);
    }
}

/// `More options`: the Transparent latch, and the queue's panel after it.
fn fold(ui: &mut egui::Ui, state: &mut AppState, more_default: bool, wide: f32) -> bool {
    let panel = &mut state.render_queue.panel;
    let open = panel.more.unwrap_or(more_default);
    let pressed = plate::ghost_row(ui, MORE_OPTIONS, wide, Some(open)).clicked();
    let open = open != pressed;
    if pressed {
        panel.more = Some(open);
    }
    if open {
        let card = "A still keeps its see-through background: a PNG with no white behind it.";
        plate::latch(ui, &mut panel.transparent, TRANSPARENT).card(card);
    }
    open
}
