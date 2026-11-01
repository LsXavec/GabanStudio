//! AU20: the lesson table, as the Lessons page reads it (spec 4.5). `ALL` stays
//! empty until a lesson ships: while it is, Start keeps `Open the example` on
//! its second row and the page cannot be reached. AU27: a lesson's paper and
//! tasks, the types the frame in `lesson.rs` reads (spec 5.3). Lesson papers
//! are `Template` consts here, outside `ALL` and `INTENTS`.

// The vocabulary is whole now; the lessons that use all of it come with AU28.
#![allow(dead_code)]

use anim_core::fx::FxKind;
use anim_core::generator::GenKind;
use anim_core::model::LayerRole;

use crate::canvas::CanvasTool;
use crate::config::Action;
use crate::steps::ProjectKind;
use crate::templates::{self, Template};
use crate::workspace::Stage;
use crate::Editor;

/// Who a lesson is for: a column of the page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Rookie,
    Amateur,
    Experienced,
}

impl Class {
    pub const ALL: [Class; 3] = [Class::Rookie, Class::Amateur, Class::Experienced];

    pub fn word(self) -> &'static str {
        match self {
            Class::Rookie => "Rookie",
            Class::Amateur => "Amateur",
            Class::Experienced => "Experienced",
        }
    }
}

/// What a path of a lesson's art is for (spec 5.13). Shown by the guide, which
/// comes after the frame; the table rules read it today.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathKind {
    Guide,
    Landmark,
    StandIn,
    Seed,
    Finish,
}

/// A line of a lesson's art, in paper units, which are the paper's pixels
/// (spec 5.13).
pub struct Path {
    /// `(x, y, pressure)`.
    pub points: Vec<[f32; 3]>,
    pub width: f32,
    pub kind: PathKind,
    /// The guide paints it above an opaque background.
    #[allow(dead_code)]
    pub over: bool,
}

impl Path {
    /// A line of a lesson's finish picture.
    pub fn new(points: Vec<[f32; 3]>, width: f32) -> Path {
        Path { points, width, kind: PathKind::Finish, over: false }
    }

    /// `[left, top, right, bottom]`.
    pub fn bounds(&self) -> [f32; 4] {
        self.points.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| {
            [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]
        })
    }
}

/// What `Show me` does: the demonstration lands on the real document, so a
/// task it answers ticks as any other (spec 5.9).
#[derive(Clone, Copy)]
pub enum Show {
    Trace(&'static [u16]),
    Perform(Action),
    Run(fn(&mut Editor)),
    Fill([f32; 2]),
    Where,
}

/// A control the ring can stand on (spec 5.5, 5.8). Each is found by the
/// `plate::mark` at its own site; `Paper` is found and rings nothing, because
/// the guide is the pointer there.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    Action(Action),
    Rail(crate::rail::Key),
    Float(crate::floats::Float),
    Pane(crate::workspace::Pane),
    Step(Stage),
    Cell { track: u8, frame: u32 },
    Named(&'static str),
    Paper,
}

/// What ticks a task: read each frame on the current task alone, from state
/// against a baseline taken when the task began (spec 5.4).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Done {
    OnStep(Stage),
    Ink(&'static [u16]),
    Strokes(u32),
    KeyArt { track: u8, frame: u32 },
    InkOnLayer,
    InkInPanels(u32),
    BrushChanged,
    Armed(&'static str),
    Smoothing,
    Tool(CanvasTool),
    Undone,
    Erased(u16),
    Filled { seed: [f32; 2], keep: Option<[f32; 2]> },
    Fills(u32),
    FilledOn(u32),
    Drawings(u32),
    /// Inclusive, rows counted from 0.
    Frame(u32, u32),
    Played,
    Speed(u32),
    Hold { frame: u32, len: u32 },
    Shows { frame: u32, drawing: u32 },
    Kinds(u32),
    Loop,
    Layers(u32),
    LayerHidden,
    LayerRole(LayerRole),
    Moved,
    Mirrored,
    Effect(FxKind),
    AnyEffect,
    Strength,
    CameraKeys(u32),
    Eased,
    Particles,
    Panels(u32),
    Pages(u32),
    Gutters,
    Rulers(u32),
    Tone,
    Generator(GenKind),
    Words,
    PageSeen,
    Lipped,
    Inbetweens,
    Chart,
    Exported(u32),
    Preset(&'static str),
    Picked([u8; 3]),
    Locked(bool),
    PanelPicked(u32),
    All(&'static [Done]),
}

#[derive(Clone, Copy)]
pub struct Task {
    /// `{Word}` tokens, resolved by `lesson::word`: no lesson string holds a
    /// literal control name.
    pub say: &'static str,
    /// Required on every `OnStep` task.
    pub why: Option<&'static str>,
    pub step: Stage,
    pub done: Done,
    /// Ancestor first, the control last.
    pub ring: &'static [Target],
    /// Paths shown during this task. The guide reads it.
    pub guide: &'static [u16],
    /// What the gate does for a person's own art: strokes or fill seeds.
    pub stand_in: &'static [u16],
    /// The act landed, the outcome did not (a leak). Replaces `say`.
    pub miss: Option<&'static str>,
    pub show: Show,
}

/// What a lesson's file must be exported with: `Exported` counts a file only
/// with this on (spec 5.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Latch {
    Transparent,
    Mono,
}

#[derive(Clone, Copy)]
pub struct Lesson {
    /// Stored in `app.lesson`, never renumbered.
    pub id: &'static str,
    pub kind: ProjectKind,
    pub class: Class,
    pub title: &'static str,
    /// One sentence, minutes, and what file it ends in.
    pub hover: &'static str,
    /// 4:3, square or page: they lose least to the card.
    pub paper: &'static Template,
    /// Given art, then `clear_history`.
    pub prep: Option<fn(&mut crate::doc::AppState)>,
    /// `CanvasView.fill_gap` for the run.
    pub fill_gap: Option<u32>,
    /// `canvas.brush_color` for the run, restored on leaving.
    pub ink: Option<[u8; 4]>,
    /// The latch its exports stand on.
    pub latch: Option<Latch>,
    /// Everything the lesson draws with.
    pub art: fn() -> Vec<Path>,
    /// The paths of `art` that make its picture.
    pub finish: &'static [u16],
    pub tasks: &'static [Task],
    /// Names what the person now knows.
    pub closing: &'static str,
}

impl Lesson {
    /// A row with nothing to run: what the stand-ins and a page test build on.
    pub const BLANK: Lesson = Lesson {
        id: "",
        kind: ProjectKind::Drawing,
        class: Class::Rookie,
        title: "",
        hover: "",
        paper: &templates::READY,
        prep: None,
        fill_gap: None,
        ink: None,
        latch: None,
        art: stand_in_art,
        finish: &[0],
        tasks: &[],
        closing: "",
    };
}

/// Every lesson that ships.
pub const ALL: &[Lesson] = &[];

/// The rows a page's `WelcomeAction::Lesson(i)` indexes: `ALL`, and in a test
/// build the probe lesson after it.
#[cfg(not(test))]
pub fn table() -> &'static [Lesson] {
    ALL
}

#[cfg(test)]
pub fn table() -> &'static [Lesson] {
    &crate::aulesson_tests::TABLE
}

/// What the table rules run over: every shipping lesson and the probe.
#[cfg(test)]
pub fn for_gate() -> &'static [Lesson] {
    &table()[..ALL.len() + 1]
}

/// A strip of paper, arched: the page's picture until a lesson brings its own.
fn stand_in_art() -> Vec<Path> {
    let arch = |x: f32| [x, 40.0 - 30.0 * (x / 100.0 * std::f32::consts::PI).sin(), 1.0];
    let points = (0..=20).map(|i| arch(i as f32 * 5.0)).collect();
    vec![Path::new(points, 2.0)]
}

const fn stand_in(id: &'static str, kind: ProjectKind, class: Class, title: &'static str) -> Lesson {
    let hover = "One sentence on what it makes. About 20 minutes. Ends in a file.";
    Lesson { id, kind, class, title, hover, ..Lesson::BLANK }
}

/// The card with its fullest task: what the audits' lesson rooms stand on until
/// `ALL` holds a lesson. Its words are no lesson's; the longest in the lessons
/// file is what `aulesson_card_fits_every_sentence` reads.
pub const PROBED: Lesson = Lesson {
    id: "stand-in-card",
    title: "Up Late",
    tasks: &[Task {
        say: "Draw one line, thicker on the turns, from the edge of the paper to the middle.",
        why: Some("A step's reason stands under its sentence while the step is young."),
        step: Stage::Drawing,
        done: Done::Strokes(1),
        ring: &[Target::Paper],
        guide: &[],
        stand_in: &[],
        miss: None,
        show: Show::Trace(&[]),
    }],
    closing: "",
    ..Lesson::BLANK
};

/// The nine rows the page is built for, with the titles the lessons will have
/// (`research/LESSONS-approachable-ui.md`): what its audits and its picture
/// stand on until `ALL` holds them. No lesson opens from here.
pub const STAND_IN: &[Lesson] = &[
    stand_in("stand-in-1", ProjectKind::Drawing, Class::Rookie, "Up Late"),
    stand_in("stand-in-2", ProjectKind::Drawing, Class::Amateur, "A Coat of Shavings"),
    stand_in("stand-in-3", ProjectKind::Drawing, Class::Experienced, "Plate One"),
    stand_in("stand-in-4", ProjectKind::Animation, Class::Rookie, "End of the Desk"),
    stand_in("stand-in-5", ProjectKind::Animation, Class::Amateur, "Still in the Coat"),
    stand_in("stand-in-6", ProjectKind::Animation, Class::Experienced, "Desk to Lamp"),
    stand_in("stand-in-7", ProjectKind::Comic, Class::Rookie, "Not a Pencil"),
    stand_in("stand-in-8", ProjectKind::Comic, Class::Amateur, "Too Much Coat"),
    stand_in("stand-in-9", ProjectKind::Comic, Class::Experienced, "The Only Word"),
];
