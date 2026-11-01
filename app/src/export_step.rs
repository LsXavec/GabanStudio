//! AU9: THE EXPORT PANEL (spec 5.5), the top of `Pane::RenderQueue`: one
//! layout wherever the pane stands. The choices follow the document, a job
//! that finishes in the Export step leaves a Done card made from its file on
//! disk, the jobs and `More options` hold at the pane's foot, outside its
//! scrolling, and the queue's own panel waits behind that fold. A pane cannot
//! reach the Editor, so what this one asks for waits in [`Panel::asked`] for
//! the drain after the dock, as the other panes' verbs do.

use anim_core::graph::NodeKind;
use anim_core::ids::NodeId;
use anim_core::model::Cut;
use eframe::egui;

use crate::config::Action;
use crate::doc::AppState;
use crate::lessons::Target;
use crate::manga_tools;
use crate::plate::{self, Card as _};
use crate::vocab;
use crate::AppRequest;
use crate::render_queue::{self, JobState, Landed};

/// A choice's height: a Start card's.
pub const CHOICE_H: f32 = 44.0;
/// Between choices: more than the ring takes.
const CHOICE_GAP: f32 = 8.0;
/// Round the pane's scrolling, inside its clip: Show me's ring stands whole.
pub const RING_ROOM: i8 = 6;
/// The most of the pane's height the job list takes: a long one scrolls.
const JOBS_SHARE: f32 = 0.3;

/// The empty state (spec 8.4).
pub const NO_ART: &str = "Nothing to export yet. Draw something first.";
/// The job list with nothing in it (spec 8.4).
pub const NOTHING_WAITING: &str = "Nothing waiting.";
/// The Video row with no ffmpeg on this PC (spec 5.5).
pub const NO_FFMPEG: &str = "Video needs a free add-on on this PC. GIF works now.";
pub const GIF_INSTEAD: &str = "Export GIF instead";
pub const MORE_OPTIONS: &str = "More options";
pub const OTHER_FORMATS: &str = "Other formats";
pub const XDTS: &str = "Exposure sheet (XDTS)…";
/// The pane width both of the fold's rows' words fit on one row.
const FOLD_ONE_ROW: f32 = 480.0;
/// A one-frame document's choice, with `Action::ExportStill` behind it.
pub const PNG_IMAGE: &str = "PNG image";
pub const OPEN_FILE: &str = "Open file";
pub const SHOW_IN_FOLDER: &str = "Show in folder";
pub const STOP_EXPORT: &str = "Stop export";
pub const TRANSPARENT: &str = "Transparent background (PNG picture)";
pub const TRY_AGAIN: &str = "Try again";
/// A book's pages on their way: no queue job, so no row of its own to read.
pub const WRITING_PAGES: &str = "Writing the pages…";

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
        if !state.has_art() && !shows_a_source(state.cut()) {
            Offer::NoArt
        } else if anim_core::manga::Page::of(state.cut()).is_some() {
            Offer::Comic
        } else if state.shown_length() > 1 {
            // AU26: one drawing with a blank tail is a picture.
            Offer::Animation
        } else {
            Offer::OneFrame
        }
    }
}

/// Art that no drawing holds, on its way to the Result: an image, a sequence,
/// a generator or a 3D model. A `Solid` is a page's paper, not art.
pub(crate) fn shows_a_source(cut: &Cut) -> bool {
    let graph = &cut.graph;
    let is_output = |id: &NodeId| graph.node(*id).is_ok_and(|n| matches!(n.kind, NodeKind::Output));
    graph.nodes().any(|n| {
        let art = matches!(
            n.kind,
            NodeKind::ImageSource { .. } | NodeKind::ImageSequence { .. } | NodeKind::Generator { .. } | NodeKind::Model3D { .. }
        );
        art && graph.downstream_closure(&[n.id]).iter().any(is_output)
    })
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

/// An export that did not finish. A refusal's four seconds are no report of a
/// file that never came, so this stands in the job list until the next export
/// starts. One at a time: `Try again` is one name, and the queue's own list
/// under `More options` keeps each job's failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failed {
    pub why: String,
    /// The same choice again. None when only the queue's form knows what it was.
    pub again: Option<Ask>,
}

/// What the panel reads from the Editor.
pub struct View {
    /// The step the back button names. Some only in the Export step.
    pub back: Option<String>,
    /// AU18: the words that go to the bar's first step.
    pub go_first: &'static str,
    /// AU18: that button's card, in the same step's words.
    pub go_card: &'static str,
    /// `More options` before any press: the boot's default.
    pub more_default: bool,
    /// `File ▸ Export`'s frame fields, once they are moved: its rows and these
    /// choices are one route, so the range holds here too and is said.
    pub range: Option<(u32, u32)>,
}

/// The panel's own state, on the queue.
#[derive(Default)]
pub struct Panel {
    /// Waits for the drain after the dock.
    pub asked: Vec<Ask>,
    /// The Done card. Only a job that finishes in the Export step leaves one.
    pub done: Option<Landed>,
    /// The last export that failed, until the next one starts.
    pub failed: Option<Failed>,
    /// Show me's ring on the first choice, until something is pressed.
    pub ring: bool,
    /// Where the ring stood this frame.
    pub ring_at: Option<egui::Rect>,
    /// `More options`, once it has been pressed; before that the boot's
    /// default. Stored by the Panels part, with every other fold.
    pub more: Option<bool>,
    /// A still keeps its alpha, through the existing `PNG 8-bit`.
    pub transparent: bool,
    /// The fold just opened: the scrolling brings it into view.
    reveal: bool,
    /// The foot's height, last frame.
    foot_high: f32,
}

impl Panel {
    /// Anything pressed answers Show me's ring.
    fn ask(&mut self, ask: Ask) {
        self.ring = false;
        self.asked.push(ask);
    }

    /// `More options` open, on any road to it, and brought into view.
    pub fn unfold(&mut self) {
        self.more = Some(true);
        self.reveal = true;
    }

    /// A new export is on its way: the last one's card or failure goes.
    pub fn clear_outcome(&mut self) {
        (self.done, self.failed) = (None, None);
    }
}

/// The panel's scrolling part: the card and the choices, and the Transparent
/// latch while `open`, with the queue's own panel to follow it.
pub fn ui(ui: &mut egui::Ui, state: &mut AppState, view: &View, open: bool) {
    state.render_queue.panel.ring_at = None;
    let wide = ui.available_width();
    // The queue's panel after this keeps its own rhythm.
    let rhythm = std::mem::replace(&mut ui.spacing_mut().item_spacing.y, CHOICE_GAP);
    if let Some(landed) = state.render_queue.panel.done.clone() {
        done_card(ui, state, &landed, view, wide);
    }
    // The Export step's one primary, while no Done card's `Open file` is.
    // Anywhere else the next step's pill leads the screen.
    let lead = view.back.is_some() && state.render_queue.panel.done.is_none();
    match Offer::of(state) {
        Offer::NoArt => no_art(ui, state, view, lead),
        Offer::Comic => {
            lead_choice(ui, state, Action::ExportPages.label(), Action::ExportPages, lead);
            mono_row(ui, state);
            other_formats(ui, state, wide, open);
        }
        Offer::OneFrame => {
            lead_choice(ui, state, PNG_IMAGE, Action::ExportStill, lead);
            other_formats(ui, state, wide, open);
        }
        Offer::Animation => {
            lead_choice(ui, state, Action::ExportGif.label(), Action::ExportGif, lead);
            video_row(ui, state);
            choice(ui, state, Action::ExportPngSequence);
            choice(ui, state, Action::ExportStill);
            range_note(ui, state, view);
        }
    }
    if open {
        // Before the row below: a row's cursor stands at its middle, not its top.
        if std::mem::take(&mut state.render_queue.panel.reveal) {
            ui.scroll_to_cursor(Some(egui::Align::TOP));
        }
        // AU11: XDTS moved here from the menus; its import is File ▸ Import ▸.
        // It shares the latch's row, so the fold costs a wide pane no height.
        if wide >= FOLD_ONE_ROW {
            ui.horizontal(|ui| {
                transparent(ui, state);
                let left = ui.available_width();
                xdts_row(ui, state, left);
            });
        } else {
            transparent(ui, state);
            xdts_row(ui, state, wide);
        }
    }
    ui.spacing_mut().item_spacing.y = rhythm;
}

/// `Exposure sheet (XDTS)…`, the app's dialog behind it.
fn xdts_row(ui: &mut egui::Ui, state: &mut AppState, wide: f32) {
    let row = plate::ghost_row(ui, XDTS, wide, None);
    if row.card("This shot's timing as an exposure sheet (.xdts).").clicked() {
        state.render_queue.panel.ask(Ask::App(AppRequest::ExportXdts));
    }
}

/// The panel's foot: the jobs, `Stop export` and `More options`, outside the
/// pane's scrolling and first into the pane, so they hold down to the smallest
/// pane. True while `More options` is open.
pub fn foot(ui: &mut egui::Ui, state: &mut AppState, view: &View) -> bool {
    let room = ui.available_height();
    let shown = egui::Panel::bottom(ui.id().with("export foot"))
        .resizable(false)
        .frame(egui::Frame::new().inner_margin(RING_ROOM))
        .show(ui, |ui| {
            let wide = ui.available_width();
            ui.spacing_mut().item_spacing.y = CHOICE_GAP;
            job_list(ui, state, wide, room);
            fold(ui, state, view.more_default, wide)
        });
    // The panel stands at last frame's height, so a list that changed length
    // needs one more frame to stand right.
    let high = shown.response.rect.height();
    if std::mem::replace(&mut state.render_queue.panel.foot_high, high) != high {
        ui.ctx().request_repaint();
    }
    shown.inner
}

/// Plain words stand on the cards' text edge, not their rim.
fn on_text_edge<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let edge = egui::Margin::symmetric(plate::CARD_PAD as i8, 0);
    egui::Frame::new().inner_margin(edge).show(ui, add).inner
}

fn plain_words(ui: &mut egui::Ui, words: &str, ink: egui::Color32) {
    on_text_edge(ui, |ui| ui.label(egui::RichText::new(words).color(ink)));
}

/// The document's first choice, solid while it leads the screen, and where
/// Show me's ring goes.
fn lead_choice(ui: &mut egui::Ui, state: &mut AppState, label: &str, action: Action, solid: bool) {
    let resp = if solid {
        plate::primary_card(ui, label, CHOICE_H, plate::Role::Card)
    } else {
        plate::choice_card(ui, label, CHOICE_H)
    };
    let panel = &mut state.render_queue.panel;
    if panel.ring {
        panel.ring_at = Some(plate::ring(ui, resp.rect));
    }
    plate::mark_row(&resp, Target::Action(action));
    if resp.card(action.help()).clicked() {
        panel.ask(Ask::Do(action));
    }
}

fn choice(ui: &mut egui::Ui, state: &mut AppState, action: Action) {
    let resp = plate::choice_card(ui, action.label(), CHOICE_H);
    plate::mark_row(&resp, Target::Action(action));
    if resp.card(action.help()).clicked() {
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
    plain_words(ui, NO_FFMPEG, plate::LEGEND);
    let wide = ui.available_width();
    if plate::filled_row(ui, GIF_INSTEAD, wide).card(Action::ExportGif.help()).clicked() {
        state.render_queue.panel.ask(Ask::Do(Action::ExportGif));
    }
}

/// AU22: `Mono` and its threshold, where a comic's pages go out: the very
/// options the Manga pane's export rows read and write, so the two show one
/// state. Off is the colour page: `in grey` stays the pane's.
fn mono_row(ui: &mut egui::Ui, state: &mut AppState) {
    use anim_core::deliver::PageMode;
    let o = &mut state.manga.export;
    let mut mono = o.mode == PageMode::Mono;
    let panel = &mut state.render_queue.panel;
    on_text_edge(ui, |ui| {
        ui.horizontal(|ui| {
            let latch = plate::latch(ui, &mut mono, vocab::MONO);
            plate::mark_row(&latch, Target::Named("export.page.mono"));
            if latch.card(manga_tools::MONO_CARD).changed() {
                o.mode = if mono { PageMode::Mono } else { PageMode::Color };
                panel.ring = false;
            }
            plate::legend(ui, "threshold").on_hover_text(manga_tools::THRESHOLD_CARD);
            plate::field_if(ui, mono, egui::DragValue::new(&mut o.threshold).range(0.0..=1.0).speed(0.01).max_decimals(2))
                .on_hover_text(manga_tools::THRESHOLD_CARD)
                .on_disabled_hover_text("Mono only: colour keeps every tone.");
        });
    });
}

/// A short document's other choices live with the queue's, so with the fold
/// open there is nothing left for this row to open.
fn other_formats(ui: &mut egui::Ui, state: &mut AppState, wide: f32, open: bool) {
    let card = "Every other format, under More options.";
    if !open && plate::ghost_row(ui, OTHER_FORMATS, wide, None).card(card).clicked() {
        let panel = &mut state.render_queue.panel;
        panel.ring = false;
        panel.unfold();
    }
}

/// The frames `File ▸ Export` narrowed these choices to, said where they are
/// chosen. A range nobody moved is the whole animation and says nothing.
fn range_note(ui: &mut egui::Ui, state: &AppState, view: &View) {
    if view.range.is_some() {
        let (first, last) = render_queue::resolve(state.frame_count(), view.range);
        let words = format!("Frames {} to {} only, as File ▸ Export says.", first + 1, last + 1);
        plain_words(ui, &words, plate::LEGEND);
    }
}

/// What only a still takes, first behind the fold.
fn transparent(ui: &mut egui::Ui, state: &mut AppState) {
    let panel = &mut state.render_queue.panel;
    let card = "A still keeps its see-through background: a PNG with no white behind it.";
    let latch = plate::latch(ui, &mut panel.transparent, TRANSPARENT);
    plate::mark_row(&latch, Target::Named("export.transparent"));
    if latch.card(card).changed() {
        panel.ring = false;
    }
}

fn no_art(ui: &mut egui::Ui, state: &mut AppState, view: &View, lead: bool) {
    plain_words(ui, NO_ART, plate::LEGEND);
    let resp = if lead {
        plate::primary_card(ui, view.go_first, CHOICE_H, plate::Role::Card)
    } else {
        plate::choice_card(ui, view.go_first, CHOICE_H)
    };
    if resp.card(view.go_card).clicked() {
        state.render_queue.panel.ask(Ask::Do(Action::RoomDrawing));
    }
}

/// `Exported ball.gif`, what it holds, and where to go from it. Every word
/// comes from the finished file and the job. A paler ground than the choices
/// and a hairline: the card says what happened, and its rows are not choices.
fn done_card(ui: &mut egui::Ui, state: &mut AppState, landed: &Landed, view: &View, wide: f32) {
    const EDGE: f32 = 1.0;
    let frame = egui::Frame::new()
        .fill(plate::ISLAND)
        .stroke(egui::Stroke::new(EDGE, plate::RULE))
        .corner_radius(plate::R_CONTROL)
        .inner_margin(plate::CARD_PAD);
    let mut ask = None;
    frame.show(ui, |ui| {
        ui.set_width(wide - 2.0 * (plate::CARD_PAD + EDGE));
        ui.spacing_mut().item_spacing.y = 6.0;
        let heading = format!("Exported {}", landed.name);
        ui.label(egui::RichText::new(heading).font(plate::Role::Card.font()).color(plate::INK));
        ui.label(egui::RichText::new(landed.line()).color(plate::LEGEND));
        let inner = ui.available_width();
        if plate::primary_card(ui, OPEN_FILE, plate::CTRL_H + 8.0, plate::Role::Step).clicked() {
            ask = Some(Ask::App(AppRequest::OpenFile(landed.file.clone())));
        }
        if plate::ghost_row(ui, SHOW_IN_FOLDER, inner, None).clicked() {
            ask = Some(Ask::App(AppRequest::ShowInFolder(landed.file.clone())));
        }
        if let Some(back) = &view.back {
            if plate::ghost_row(ui, &format!("Back to {back}"), inner, None).clicked() {
                ask = Some(Ask::Back);
            }
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

/// One job on its way, in words and a meter.
fn job_row(ui: &mut egui::Ui, name: &str, done: usize, total: usize) {
    on_text_edge(ui, |ui| {
        let wide = ui.available_width();
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
    });
}

/// `That export did not finish`, and the same choice again. The sentence ends
/// here, whatever the reason's own stop.
fn failed_row(ui: &mut egui::Ui, state: &mut AppState, failed: &Failed) {
    let words = format!("That export did not finish: {}.", failed.why.trim_end_matches('.'));
    let mut pressed = false;
    plate::failure(ui, &words, |ui| {
        if failed.again.is_some() {
            let wide = ui.available_width();
            pressed = plate::filled_row(ui, TRY_AGAIN, wide).clicked();
        }
    });
    if let Some(again) = failed.again.clone().filter(|_| pressed) {
        state.render_queue.panel.ask(again);
    }
}

/// What this export is doing, pinned: a person sees one go whether or not
/// `More options` is open, and one that failed does not vanish. With no art
/// and nothing going the panel has said all it has to say (spec 8.4).
fn job_list(ui: &mut egui::Ui, state: &mut AppState, wide: f32, room: f32) {
    let rq = &state.render_queue;
    let unfinished = rq.queue.jobs.iter().filter(|j| matches!(j.state, JobState::Queued | JobState::Running));
    let rows: Vec<(String, usize, usize)> = unfinished.map(|j| (job_name(j), j.done, j.total())).collect();
    let failed = rq.panel.failed.clone();
    let pages = state.manga.export_job.is_some();
    if rows.is_empty() && failed.is_none() && !pages {
        if Offer::of(state) != Offer::NoArt {
            plain_words(ui, NOTHING_WAITING, plate::LEGEND);
        }
        return;
    }
    let scroll = egui::ScrollArea::vertical()
        .id_salt("export jobs")
        .max_height(room * JOBS_SHARE)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (name, done, total) in rows {
                job_row(ui, &name, done, total);
            }
            if pages {
                plain_words(ui, WRITING_PAGES, plate::INK);
            }
            if let Some(failed) = &failed {
                failed_row(ui, state, failed);
            }
        });
    plate::note_viewport(ui, scroll.inner_rect, [false, true]);
    if state.render_queue.busy() && plate::ghost_row(ui, STOP_EXPORT, wide, None).clicked() {
        state.render_queue.panel.ring = false;
        render_queue::stop(state);
    }
}

/// `More options`: the Transparent latch and the queue's panel, in the
/// scrolling above it.
fn fold(ui: &mut egui::Ui, state: &mut AppState, more_default: bool, wide: f32) -> bool {
    let panel = &mut state.render_queue.panel;
    let open = panel.more.unwrap_or(more_default);
    let row = plate::ghost_row(ui, MORE_OPTIONS, wide, Some(open));
    plate::mark_row(&row, Target::Named("export.more"));
    if !row.clicked() {
        return open;
    }
    panel.ring = false;
    if open {
        panel.more = Some(false);
    } else {
        panel.unfold();
    }
    !open
}
