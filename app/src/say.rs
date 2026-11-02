//! AU7: THE SAY LINE'S TEACHING (spec 8.2 to 8.5). One table, read once a
//! frame: the first row that applies speaks, and it stays until it is
//! answered. An answer lands in `ui.hints_seen`, so a row teaches once. No
//! timer takes a sentence off the lane: the one clock here, `READ_SECS`, only
//! keeps a row nobody had time to read from counting as said. A row's bit is
//! stored: it keeps its number.
//!
//! The lane's order stays `Editor::say_line`'s: a refusal, the machine's last
//! remark, then what this file picks. Quick Start's ticks are read here too,
//! from outcomes only: the stroke signal, `AddDrawingAfter`, playback and a
//! finished export's file on disk.

use anim_core::command::CutRef;
use anim_core::generator::GenKind;
use anim_core::graph::NodeKind;
use anim_core::manga;

use crate::config::{Action, UiConfig};
use crate::render_queue::Landed;
use crate::steps::{self, ProjectKind};
use crate::welcome::{self, PenCheck};
use crate::workspace::Stage;
use crate::{AppRequest, Editor, Pane, Performed, doc, export_step, menus, visible_panes};

pub const SHOW_ME: &str = "Show me";
pub const GO_TO_PLAN: &str = "Go to Plan";
pub const GO_TO_DRAW: &str = "Go to Draw";
pub const GO_TO_MANGA: &str = "Go to Manga";
pub const FIX_IT: &str = "Fix it";

/// AU19: what Plan teaches, once.
pub const PLAN_ROW_WORDS: &str = "Click a row, then draw on it.";

// The stored bits, one for each row of spec 8.2. 15 was AU7's stand-in for the
// Export step and 5 the Plan preview's row: either may stand set in a config,
// so neither is given to another row.
pub const NEW_PAPER: u32 = 1 << 0;
pub const ADD_DRAWING: u32 = 1 << 1;
pub const NEXT_POSE: u32 = 1 << 2;
pub const PLAY: u32 = 1 << 3;
pub const SHARE: u32 = 1 << 4;
pub const REFUSED_TWICE: u32 = 1 << 6;
pub const FLAT_PEN: u32 = 1 << 7;
/// Never stored: a finished export is news each time, not a lesson.
pub const EXPORTED: u32 = 1 << 8;
pub const COLOR: u32 = 1 << 9;
pub const EFFECTS: u32 = 1 << 10;
pub const APPEARS_TWICE: u32 = 1 << 11;
pub const HELD: u32 = 1 << 12;
/// AU10: the return dot's card, not the lane's: the band is off when it speaks.
pub const HIDE_TOOLS: u32 = 1 << 13;
/// The canvas line's bit (spec 8.4).
pub const EMPTY_CELL: u32 = 1 << 14;
pub const TWO_DRAWINGS: u32 = 1 << 16;
/// AU18: Quick Start's slots 1 to 3 on a drawing and on a comic. `ui.quick_start`
/// is one array, an animation's, so these keep theirs here (spec 2.10).
pub const DRAWING_SLOTS: [u32; 3] = [1 << 17, 1 << 18, 1 << 19];
pub const COMIC_SLOTS: [u32; 3] = [1 << 20, 1 << 21, 1 << 22];
pub const PLAN_ROWS: u32 = 1 << 23;
/// AU18: the drawing grew into an animation: a remark, said once.
pub const GREW: u32 = 1 << 24;
pub const GREW_WORDS: &str = "Two drawings make an animation. Plan and Timing are here now.";

/// What a hint's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    /// Quick Start's `Show me` on this slot.
    Show(usize),
    GoToDraw,
    /// The Action that answers a refusal.
    Fix(Action),
    /// Settings, on the pen page.
    PenPage,
    /// The finished export `Say` holds, in the program that opens it.
    OpenFile,
}

impl Verb {
    /// AU18: `GoToDraw` is always the bar's first step, in that step's word.
    pub fn label(self, kind: Option<ProjectKind>) -> &'static str {
        match self {
            Verb::Show(_) => SHOW_ME,
            Verb::GoToDraw => match kind {
                Some(ProjectKind::Animation) => GO_TO_PLAN,
                Some(ProjectKind::Comic) => GO_TO_MANGA,
                Some(ProjectKind::Drawing) | None => GO_TO_DRAW,
            },
            Verb::Fix(_) | Verb::PenPage => FIX_IT,
            Verb::OpenFile => export_step::OPEN_FILE,
        }
    }

    /// AU18: the label follows the kind, and so does what it says of itself.
    pub fn card(self, kind: Option<ProjectKind>) -> &'static str {
        match self {
            Verb::Show(_) => "Do this step for me.",
            Verb::GoToDraw => kind.map_or(Action::RoomDrawing.help(), |kind| steps::bar(kind)[0].purpose),
            Verb::Fix(action) => action.help(),
            Verb::PenPage => "Open Settings on the pen page.",
            Verb::OpenFile => "Open the file you exported.",
        }
    }
}

/// A row's sentence.
#[derive(Clone, Copy)]
pub enum Words {
    Plain(&'static str),
    /// Before, the verb's live chord, after. A verb with no chord has no
    /// sentence, so its row does not show.
    Chord(&'static str, Action, &'static str),
    /// Spec 8.2's `N`: how long the held drawing stays.
    Held,
    /// The refusal said twice, held.
    Reason,
    /// Spec 8.2's `ball.gif`: the file that landed.
    Exported,
}

/// The document and the step as the table reads them, and what this frame
/// came to. Pure, so a test reaches every row without an editor.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub stage: Option<Stage>,
    /// AU18: what the project is. None: no row is filtered.
    pub kind: Option<ProjectKind>,
    /// Drawings in this cut.
    pub drawings: usize,
    /// One of them holds art.
    pub art: bool,
    pub frame: u32,
    /// This frame has a drawing of its own, with nothing on it.
    pub blank: bool,
    /// Ghost frames are on.
    pub ghost: bool,
    pub playing: bool,
    /// The strip is out: this document is being animated.
    pub strip: bool,
    /// A held frame: how many frames its drawing stays.
    pub held: Option<u32>,
    /// The drawing here is exposed in two runs or more.
    pub twice: bool,
    /// The render queue is on screen.
    pub queue_up: bool,
    /// A finished export's name, until something else has happened.
    pub report: Option<String>,
    /// AU19: this is Plan with the sheet on.
    pub plan_sheet: bool,
    /// Three pen strokes with no pressure in them.
    pub flat_pen: bool,
    /// A refusal said twice running.
    pub refused_twice: Option<String>,
    /// The dock left room for the dot's first-time card (see [`Say::card_room`]).
    pub card_room: bool,
    /// `ui.hints_seen`, as the frame began.
    pub seen: u32,
    /// The row last on the lane stood long enough to read.
    pub stood: bool,
    // ---- this frame's outcomes ----
    pub stroked: bool,
    /// A row of the Timing sheet took a click.
    pub row_clicked: bool,
    pub added: bool,
    pub filled: bool,
    pub effect_added: bool,
    /// A finished export's file reached the disk.
    pub exported: bool,
    pub pen_works: bool,
    /// The tools just came back.
    pub tools_back: bool,
    /// AU27: a lesson stands: its card carries the sentence.
    pub lesson: bool,
    /// AU27: the files that landed this frame, and the latches they left under.
    pub landed: u32,
    pub transparent: bool,
    pub mono: bool,
}

impl Facts {
    fn has_seen(&self, bit: u32) -> bool {
        self.seen & bit != 0
    }

    fn animation(&self) -> bool {
        self.kind.is_none_or(|kind| kind == ProjectKind::Animation)
    }
}

/// One row of spec 8.2. `HINTS`' order is its precedence.
pub struct Hint {
    pub bit: u32,
    pub words: Words,
    pub verb: Option<Verb>,
    /// The steps it can speak in; none: any.
    pub steps: &'static [Stage],
    /// AU18: the kinds it can speak on.
    pub kinds: &'static [ProjectKind],
    pub shows: fn(&Facts) -> bool,
    pub answered: fn(&Facts) -> bool,
    /// Said for a moment: the moment passing answers it, if it stood to be
    /// read and playback did not carry it off.
    pub passes: bool,
}

const fn never(_: &Facts) -> bool {
    false
}

const DRAW: &[Stage] = &[Stage::Drawing];
const DRAWING_ROOMS: &[Stage] = &[Stage::Drawing, Stage::Manga];
const DRAWING_ROOMS_AND_TIMING: &[Stage] = &[Stage::Drawing, Stage::Manga, Stage::Edit];
const TRANSPORT: &[Stage] = &[Stage::Drawing, Stage::Edit];
const ON_A_FRAME: &[Stage] = &[Stage::Drawing, Stage::Edit, Stage::InkPaint];
const ANY_KIND: &[ProjectKind] = &ProjectKind::ALL;
/// A row that names a frame, a drawing's order or Play.
const ANIMATION: &[ProjectKind] = &[ProjectKind::Animation];
const ON_PAPER: &[ProjectKind] = &[ProjectKind::Drawing, ProjectKind::Animation];

pub const HINTS: [Hint; 16] = [
    // First, or a row with nowhere to stand would keep it from the dot. The
    // first stroke answers it too: whoever draws has read it, or has no need
    // to. Only one made under the card: `read` asks every row, shown or not.
    Hint {
        bit: HIDE_TOOLS,
        words: Words::Chord("Tap the dot, or press ", Action::HideTools, ", to bring the tools back."),
        verb: None,
        steps: &[],
        kinds: ANY_KIND,
        shows: |f| f.card_room,
        answered: |f| f.tools_back || (f.card_room && f.stroked),
        passes: false,
    },
    Hint {
        bit: REFUSED_TWICE,
        words: Words::Reason,
        verb: None,
        steps: &[],
        kinds: ANY_KIND,
        shows: |f| f.refused_twice.is_some(),
        answered: never,
        passes: true,
    },
    Hint {
        bit: FLAT_PEN,
        words: Words::Plain("Your pen is drawing without pressure."),
        verb: Some(Verb::PenPage),
        steps: &[],
        kinds: ANY_KIND,
        shows: |f| f.flat_pen,
        answered: |f| f.pen_works,
        passes: false,
    },
    // Nothing answers it, so its bit is never stored: every finished export
    // says so, for as long as nothing else has happened (`Say::exported`).
    Hint {
        bit: EXPORTED,
        words: Words::Exported,
        verb: Some(Verb::OpenFile),
        steps: &[],
        kinds: ANY_KIND,
        shows: |f| f.report.is_some(),
        answered: never,
        passes: false,
    },
    Hint {
        bit: APPEARS_TWICE,
        words: Words::Plain("This drawing appears twice. Changes show in both."),
        verb: None,
        steps: ON_A_FRAME,
        kinds: ANIMATION,
        shows: |f| f.twice && !f.playing,
        answered: never,
        passes: true,
    },
    Hint {
        bit: HELD,
        words: Words::Held,
        verb: None,
        steps: ON_A_FRAME,
        kinds: ANIMATION,
        shows: |f| f.held.is_some() && !f.playing,
        answered: never,
        passes: true,
    },
    Hint {
        bit: COLOR,
        words: Words::Plain("Pick a color, then tap an area."),
        verb: None,
        steps: &[Stage::InkPaint],
        kinds: ANY_KIND,
        shows: |f| f.art,
        answered: |f| f.filled,
        passes: false,
    },
    Hint {
        bit: EFFECTS,
        words: Words::Plain("Pick an effect to add it to the picture."),
        verb: None,
        steps: &[Stage::Composite],
        kinds: ANY_KIND,
        shows: |f| f.art,
        answered: |f| f.effect_added,
        passes: true,
    },
    // AU19: Plan's lesson stands where the paper's would. No Show me: its
    // circle teaches the paper, and the row after this one offers it. A click,
    // Play or a stroke in Plan answers it; a stroke anywhere else, on the ready
    // paper say, was made before it stood (`read` asks every row).
    Hint {
        bit: PLAN_ROWS,
        words: Words::Plain(PLAN_ROW_WORDS),
        verb: None,
        steps: DRAW,
        kinds: ANIMATION,
        shows: |f| f.plan_sheet,
        answered: |f| f.plan_sheet && (f.stroked || f.row_clicked || f.playing),
        passes: false,
    },
    // Quick Start's chain, in its own order.
    Hint {
        bit: NEW_PAPER,
        words: Words::Plain("Draw on the paper to begin."),
        verb: Some(Verb::Show(0)),
        steps: DRAWING_ROOMS,
        kinds: ON_PAPER,
        shows: |f| !f.art,
        answered: |f| f.stroked,
        passes: false,
    },
    // AU18: the same lesson, in a comic's word: one bit.
    Hint {
        bit: NEW_PAPER,
        words: Words::Plain("Draw on the page to begin."),
        verb: Some(Verb::Show(0)),
        steps: DRAWING_ROOMS,
        kinds: &[ProjectKind::Comic],
        shows: |f| !f.art,
        answered: |f| f.stroked,
        passes: false,
    },
    Hint {
        bit: ADD_DRAWING,
        words: Words::Plain("Next: add a drawing."),
        verb: Some(Verb::Show(1)),
        steps: DRAW,
        kinds: ANIMATION,
        shows: |f| f.art && f.drawings == 1 && f.strip,
        answered: |f| f.added,
        passes: false,
    },
    Hint {
        bit: NEXT_POSE,
        words: Words::Plain("The ghost is your last drawing. Draw the next pose."),
        verb: Some(Verb::Show(2)),
        steps: DRAW,
        kinds: ANIMATION,
        shows: |f| f.art && f.blank && f.frame >= 1 && f.ghost,
        answered: |f| f.stroked && f.frame >= 1,
        passes: false,
    },
    // Spec 8.2's Play row waits for a third drawing, and Show me's own chain
    // stops at two: without this one it would end here, with nothing said.
    Hint {
        bit: TWO_DRAWINGS,
        words: Words::Chord("Add another drawing, or press ", Action::PlayPause, " to play."),
        verb: Some(Verb::Show(welcome::PLAY_STEP)),
        steps: TRANSPORT,
        kinds: ANIMATION,
        shows: |f| f.art && f.drawings == 2 && !f.blank && !f.playing,
        answered: |f| f.playing,
        passes: false,
    },
    Hint {
        bit: PLAY,
        words: Words::Chord("Press ", Action::PlayPause, " to play."),
        verb: Some(Verb::Show(3)),
        steps: TRANSPORT,
        kinds: ANIMATION,
        shows: |f| f.drawings >= 3 && !f.blank && !f.playing,
        answered: |f| f.playing,
        passes: false,
    },
    // AU18: Play is an animation's: a drawing or a comic gets here with art.
    Hint {
        bit: SHARE,
        words: Words::Plain("Ready to share? Go to Export."),
        verb: Some(Verb::Show(welcome::EXPORT_STEP)),
        steps: DRAWING_ROOMS_AND_TIMING,
        kinds: ANY_KIND,
        shows: |f| !f.queue_up && if f.animation() { f.has_seen(PLAY) && f.drawings >= 2 } else { f.art },
        answered: |f| f.exported,
        passes: false,
    },
];

/// A step with nothing to work on yet (spec 8.4). No bit: an empty state is
/// no lesson, so it stays as long as the room is empty, hints on or off. Off,
/// it has no button.
pub struct EmptyStep {
    pub stage: Stage,
    pub words: &'static str,
    pub applies: fn(&Facts) -> bool,
}

pub const EMPTY_STEPS: [EmptyStep; 3] = [
    EmptyStep { stage: Stage::Edit, words: "Add a second drawing to set timing.", applies: |f| f.drawings < 2 },
    EmptyStep { stage: Stage::InkPaint, words: "Nothing to color yet.", applies: |f| !f.art },
    EmptyStep { stage: Stage::Composite, words: "Nothing to add effects to yet.", applies: |f| !f.art },
];

/// What the lane says when nothing outranks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spoken {
    /// None for an empty state.
    pub bit: Option<u32>,
    pub words: String,
    pub verb: Option<Verb>,
}

impl Hint {
    fn applies(&self, f: &Facts) -> bool {
        (self.steps.is_empty() || f.stage.is_some_and(|s| self.steps.contains(&s)))
            && f.kind.is_none_or(|kind| self.kinds.contains(&kind))
            && (self.shows)(f)
    }

    fn spoken(&self, f: &Facts, chord: &impl Fn(Action) -> Option<String>) -> Option<Spoken> {
        let refusal = f.refused_twice.as_deref();
        let words = match self.words {
            Words::Plain(words) => words.to_string(),
            Words::Chord(before, verb, after) => format!("{before}{}{after}", chord(verb)?),
            Words::Held => format!("This drawing stays on screen for {} frames.", f.held?),
            Words::Reason => reason(refusal?),
            Words::Exported => format!("Exported {}.", f.report.as_deref()?),
        };
        let verb = match self.words {
            Words::Reason => refusal.and_then(fix_of).map(Verb::Fix),
            _ => self.verb,
        };
        Some(Spoken { bit: Some(self.bit), words, verb })
    }
}

/// A refusal to hold: its machine prefix off, its first letter raised. The
/// refusals already say the reason and the fix (spec 8.2).
fn reason(refusal: &str) -> String {
    let words = refusal.strip_prefix("refused — ").unwrap_or(refusal);
    let mut letters = words.chars();
    letters.next().map_or_else(String::new, |c| c.to_uppercase().chain(letters).collect())
}

/// The refusals one Action answers.
pub fn fix_of(refusal: &str) -> Option<Action> {
    match refusal {
        doc::HELD_FRAME | doc::BLANK_FRAME => Some(Action::NewDrawing),
        doc::READ_ONLY_VIEW | doc::SEE_PAGE_ON => Some(Action::ToggleCompositeView),
        _ => None,
    }
}

/// One frame's reading: the rows `f` answers join `seen`, then the first row
/// that still applies speaks. `shown`: the row last on the lane.
pub fn read(
    f: &Facts,
    seen: &mut u32,
    shown: Option<u32>,
    chord: impl Fn(Action) -> Option<String>,
) -> Option<Spoken> {
    for hint in &HINTS {
        let passed = hint.passes && shown == Some(hint.bit) && f.stood && !f.playing && !hint.applies(f);
        if (hint.answered)(f) || passed {
            *seen |= hint.bit;
        }
    }
    // The Exported row is news, not a lesson: no seen bit silences it, not
    // even an upgrader's whole mask.
    let due = |hint: &&Hint| (hint.bit == EXPORTED || *seen & hint.bit == 0) && hint.applies(f);
    // AU27: during a lesson the lane keeps what is no lesson's to say.
    let kept = |hint: &&Hint| !f.lesson || matches!(hint.bit, EXPORTED | REFUSED_TWICE | FLAT_PEN);
    HINTS.iter().filter(due).filter(kept).find_map(|hint| hint.spoken(f, &chord))
}

/// The empty state of the step `f` stands in.
pub fn empty_step(f: &Facts, hints_off: bool) -> Option<Spoken> {
    if f.lesson {
        return None;
    }
    let row = EMPTY_STEPS.iter().find(|e| f.stage == Some(e.stage) && (e.applies)(f))?;
    let verb = (!hints_off).then_some(Verb::GoToDraw);
    Some(Spoken { bit: None, words: row.words.to_string(), verb })
}

/// The five dots' card: the place they show, in words.
pub fn place_words(rows: &[welcome::Step; 5], ticks: &[bool; 5]) -> String {
    let done = ticks.iter().filter(|done| **done).count();
    let next = ticks.iter().position(|done| !done).map_or("", |i| rows[i].label);
    format!("Quick Start: {done} of 5. Next: {}.", next.to_lowercase())
}

/// What the Say line keeps between frames.
#[derive(Default)]
pub struct Say {
    /// `ui.hints_off` and `ui.hints_seen`, as the shell feeds them.
    pub off: bool,
    pub seen: u32,
    /// `ToggleHints` asked for this; the shell stores it.
    pub hints_request: Option<bool>,
    /// New bits; the shell stores them.
    pub seen_request: Option<u32>,
    /// What the lane says next, when nothing outranks it.
    pub hint: Option<Spoken>,
    /// The row last on the lane.
    pub shown: Option<u32>,
    /// The row on the lane this frame, and when it came up.
    standing: Option<u32>,
    since: f64,
    /// `shown` stood `READ_SECS`.
    stood: bool,
    /// `AddDrawingAfter` ran since the last reading.
    pub added: bool,
    /// An export that finished outside the Export step, until something else
    /// happens: `Open file` opens this one.
    pub exported: Option<Landed>,
    /// Playing, last frame: a press of play overtakes the report, and a report
    /// that lands while it plays still has to be read.
    pub was_playing: bool,
    /// The stroke just read was `Check your pen`'s.
    pub check_line: bool,
    /// The stroke just read was Show me's circle: no pen drew it.
    circle: bool,
    /// The five dots painted this frame.
    pub dots_painted: bool,
    /// Where they last stood, for a test to rest on.
    #[cfg(test)]
    pub dots_at: Option<eframe::egui::Rect>,
    /// Hide tools was on at the last reading.
    pub tools_were_hidden: bool,
    /// AU10: the dock leaves the dot's first-time card its room (spec 8.5:
    /// nothing over art). Settled as the tools go and kept until they are back,
    /// or the stroke that answers the card would refit the paper under the pen.
    /// `None`: the tools are showing.
    pub card_room: Option<bool>,
    /// A sentence a test holds on the lane, whatever the document says.
    #[cfg(test)]
    pub pin: Option<Spoken>,
    strokes: Option<u64>,
    fills: Option<u64>,
    /// Effects reaching the Output, and the cut and the revision that count is of.
    effects: Option<usize>,
    effects_at: Option<(CutRef, u64)>,
    /// A comic's panels, and the page and the revision that count is of.
    panels: Option<usize>,
    panels_at: Option<(CutRef, u64)>,
    flat_strokes: u32,
    pen_answered: bool,
    /// The canvas line has painted (spec 8.4).
    line_shown: bool,
    refusal_seq: u32,
    refusal: String,
    refusals: u32,
    revision: u64,
    frame: u32,
}

/// AU14: what `effect_added` counts: what changes the picture. The nodes that
/// make it (a source, a Blend, a comic's panels) are no effect.
fn is_an_effect(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::Effect { .. } | NodeKind::Camera { .. } => true,
        NodeKind::Generator { generator, .. } => *generator != GenKind::Panels,
        _ => false,
    }
}

/// Pen strokes with no pressure before the hint says so (spec 8.2).
const FLAT_STROKES: u32 = 3;
/// The demo circle's places along the paper, one a frame.
const DEMO_STEPS: u32 = 8;
/// How long a row that passes stands before its passing counts as read: long
/// enough for its longest sentence. A floor on being used up, on the clock
/// because egui paints no frames while nothing happens, and never a reason
/// for a row to leave the lane.
pub const READ_SECS: f64 = 2.0;

impl Say {
    /// The shell's config, each frame: the switch and the bits. On again,
    /// nothing the lane or the paper said before counts (spec 8.5).
    pub fn feed(&mut self, ui: &UiConfig) {
        if self.off && !ui.hints_off {
            (self.shown, self.standing, self.stood, self.line_shown) = (None, None, false, false);
        }
        // An answer from outside the frame waits in the request: `perform` ticks
        // a row before the shell has stored it.
        (self.off, self.seen) = (ui.hints_off, ui.hints_seen | self.seen_request.unwrap_or(0));
    }

    /// What the lane shows this frame, from the shell, with the time. A row
    /// that leaves and comes back starts its reading again.
    pub fn stands(&mut self, bit: Option<u32>, now: f64) {
        if bit != self.standing {
            (self.standing, self.since) = (bit, now);
        }
        if bit.is_some() {
            (self.shown, self.stood) = (bit, now - self.since >= READ_SECS);
        }
    }

    /// Another document: nothing that differs from the last one is an outcome.
    pub fn new_document(&mut self) {
        (self.strokes, self.fills, self.effects, self.panels) = (None, None, None, None);
        (self.effects_at, self.panels_at) = (None, None);
        self.exported = None;
    }

    /// What this frame asked of the config. True when it changed (the shell
    /// saves).
    pub fn store(&mut self, ui: &mut UiConfig) -> bool {
        let seen = self.seen_request.take();
        if let Some(on) = self.hints_request.take() {
            ui.set_hints(on);
            return true;
        }
        if let Some(bits) = seen {
            ui.hints_seen = bits;
        }
        seen.is_some()
    }

    /// `canvas.empty_cell_hint` (spec 8.4): on a blank document the Say line
    /// says it instead, and on a drawing with no art it is still saying
    /// `NEW_PAPER`: a screen carries a sentence once.
    pub fn empty_cell_due(&self, state: &doc::AppState) -> bool {
        let said = state.has_art() || self.seen & NEW_PAPER != 0;
        !self.off && self.seen & EMPTY_CELL == 0 && !state.cut().drawings.is_empty() && said
    }

    /// A refusal said twice running: a new word, an edit or another frame
    /// starts the count again.
    fn count_refusals(&mut self, state: &doc::AppState) {
        let (revision, frame) = (state.engine.revision(), state.view.frame);
        if (revision, frame) != (self.revision, self.frame) {
            (self.revision, self.frame, self.refusals) = (revision, frame, 0);
        }
        if state.refusal_seq != self.refusal_seq {
            self.refusal_seq = state.refusal_seq;
            let same = self.refusal == state.refusal;
            self.refusals = if same { self.refusals + 1 } else { 1 };
            self.refusal.clone_from(&state.refusal);
        }
    }
}

impl Editor {
    /// AU18: an animation's rules stand here, as they do on the frozen boot.
    pub(crate) fn animated(&self) -> bool {
        self.kind().is_none_or(|kind| kind == ProjectKind::Animation)
    }

    /// AU18: Quick Start's rows for this kind.
    pub(crate) fn quick_rows(&self) -> &'static [welcome::Step; 5] {
        welcome::steps_of(self.kind())
    }

    /// AU18: where a drawing or a comic keeps its slots 1 to 3.
    fn slot_bits(&self) -> Option<[u32; 3]> {
        match self.kind() {
            Some(ProjectKind::Drawing) => Some(DRAWING_SLOTS),
            Some(ProjectKind::Comic) => Some(COMIC_SLOTS),
            _ => None,
        }
    }

    /// AU18: Quick Start's five, as this kind reads them.
    pub(crate) fn ticks(&self) -> [bool; 5] {
        let mut ticks = self.quick_start;
        if let Some(bits) = self.slot_bits() {
            for (tick, bit) in ticks[1..4].iter_mut().zip(bits) {
                *tick = self.say.seen & bit != 0;
            }
        }
        ticks
    }

    /// AU18: slot `i` is done, where this kind keeps it.
    pub(crate) fn tick_slot(&mut self, i: usize) {
        // AU27: a lesson is not a free Quick Start.
        if self.lesson.is_some() {
            return;
        }
        match self.slot_bits() {
            Some(bits) if (1..4).contains(&i) => {
                if self.say.seen & bits[i - 1] == 0 {
                    self.say_answered(bits[i - 1]);
                }
            }
            _ => self.quick_start[i] = true,
        }
    }

    /// AU7: what this frame came to: Quick Start's ticks, and the row that
    /// speaks next. Runs after the dock, where a stroke lands.
    pub(crate) fn say_read(&mut self, ctx: &eframe::egui::Context) {
        let (stroked, filled, landed) = self.say_outcomes();
        self.say.count_refusals(&self.state);
        let facts = self.say_facts(stroked, filled, landed);
        self.lesson_read(&facts);
        let mut seen = self.say.seen;
        let hint = if self.say.off {
            None
        } else {
            seen |= if stroked && self.say.line_shown { EMPTY_CELL } else { 0 };
            read(&facts, &mut seen, self.say.shown, |a| menus::chord_label(ctx, a))
        };
        if seen != self.say.seen && !facts.lesson {
            self.say.seen = seen;
            self.say.seen_request = Some(seen);
        }
        let hint = hint.or_else(|| empty_step(&facts, self.say.off));
        #[cfg(test)]
        let hint = self.say.pin.clone().or(hint);
        // The lane stands before the dock: a change shows on the next frame.
        if hint != self.say.hint {
            ctx.request_repaint();
        }
        self.say.hint = hint;
        self.say.line_shown |= self.canvas.dbg_empty_line;
    }

    /// The stroke, fill, playback and export signals, read once, and what they
    /// tick. A line drawn for `Check your pen` was the check's: it ticks
    /// nothing.
    fn say_outcomes(&mut self) -> (bool, bool, u32) {
        let state = &self.state;
        let check_line = std::mem::take(&mut self.say.check_line);
        let circle = std::mem::take(&mut self.say.circle);
        let stroked = self.say.strokes.is_some_and(|seen| seen != state.strokes_done) && !check_line;
        let filled = self.say.fills.is_some_and(|seen| seen != state.fills_done);
        (self.say.strokes, self.say.fills) = (Some(state.strokes_done), Some(state.fills_done));
        if stroked {
            self.tick_stroke();
            match self.canvas.pen_verdict() {
                // The verdict is the last real stroke's, and the circle no pen's.
                _ if circle => {}
                PenCheck::NoPressure => self.say.flat_strokes += 1,
                PenCheck::Works => {
                    self.say.flat_strokes = 0;
                    self.say.pen_answered = true;
                }
                _ => {}
            }
        }
        if self.animated() {
            self.quick_start[welcome::PLAY_STEP] |= self.state.view.playing && self.lesson.is_none();
        } else if filled && self.kind() == Some(ProjectKind::Drawing) {
            self.tick_slot(1);
        }
        let landed = self.say_landed();
        (stroked, filled, landed)
    }

    /// The files that reached the disk since the last reading (spec 8.3 ticks
    /// only on one). In the Export step the Done card reports, if the step's
    /// dock still has the pane to hold one; elsewhere the lane does, and at
    /// once: a plain finish has nothing to say that the report does not, so
    /// the remark gives way. With hints off the lane has no report and no
    /// button, and the remark stays.
    fn say_landed(&mut self) -> u32 {
        let landed = std::mem::take(&mut self.state.render_queue.landed);
        let count = landed.len() as u32;
        // A blank graph still exports (spec AU8), and it earns no tick.
        let art = || self.state.has_art() || crate::export_step::shows_a_source(self.state.cut());
        self.quick_start[welcome::EXPORT_STEP] |= count > 0 && self.lesson.is_none() && art();
        let card = self.stage == Some(Stage::Export) && visible_panes(&self.dock).contains(&Pane::RenderQueue);
        for file in landed {
            if !card && self.say.off {
                continue;
            }
            if file.plain {
                self.state.status.clear();
            }
            if card {
                self.state.render_queue.panel.done = Some(file);
            } else {
                self.say.exported = Some(file);
            }
        }
        count
    }

    /// What a finished stroke ticks (spec 8.3).
    fn tick_stroke(&mut self) {
        if self.lesson.is_some() {
            return;
        }
        self.quick_start[welcome::FIRST_STROKE] = true;
        if self.animated() {
            self.quick_start[welcome::NEXT_POSE] |= self.state.view.frame >= 1;
        }
    }

    /// AU10: will the dot wear its first-time card? Asked once each time the
    /// tools go. With no chord to name the row has no sentence.
    pub(crate) fn dot_card_due(&self, ctx: &eframe::egui::Context) -> bool {
        !self.say.off && self.say.seen & HIDE_TOOLS == 0 && menus::chord_label(ctx, Action::HideTools).is_some()
    }

    fn say_facts(&mut self, stroked: bool, filled: bool, landed: u32) -> Facts {
        let exported = landed > 0;
        let added = std::mem::take(&mut self.say.added);
        let row_clicked = std::mem::take(&mut self.state.sheet_ui.row_clicked);
        // Work goes on, and the report has been overtaken: it must not hold
        // the row of what comes next over a fill or a play. Only the press of
        // play counts: a job that lands while it plays still reports.
        let playing = self.state.view.playing;
        let was_playing = std::mem::replace(&mut self.say.was_playing, playing);
        if stroked || filled || added || (playing && !was_playing) {
            self.say.exported = None;
        }
        let state = &self.state;
        let cut = state.cut();
        // AU14: an effect reaching the Output, not a node count: a wire or a
        // source adds no effect, and the walk runs only after an edit.
        let here = (state.at(), state.engine.revision());
        let mut effect_added = false;
        if self.say.effects_at != Some(here) {
            let now = crate::lesson::reached(&cut.graph).iter().filter(|n| is_an_effect(n)).count();
            let same_cut = self.say.effects_at.is_some_and(|(at, _)| at == here.0);
            effect_added = same_cut && self.say.effects.is_some_and(|before| now > before);
            (self.say.effects, self.say.effects_at) = (Some(now), Some(here));
        }
        // AU18: a comic's cut into panels by the count, read on a comic alone
        // and only after an edit: `panels_of` clones them, and this runs per
        // frame. A page with more panels than the last one is no outcome.
        let mut panel_added = false;
        if self.kind() == Some(ProjectKind::Comic) && self.say.panels_at != Some(here) {
            let now = manga::panels_of(cut).map_or(0, |(_, polys)| polys.len());
            let same_page = self.say.panels_at.is_some_and(|(page, _)| page == here.0);
            panel_added = same_page && self.say.panels.is_some_and(|before| now > before);
            (self.say.panels, self.say.panels_at) = (Some(now), Some(here));
        }
        let shown = state.current_drawing();
        let held = state.own_key_drawing().is_none().then(|| state.exposure_here()).flatten();
        let held = held.filter(|_| shown.is_some());
        let length = state.frame_count();
        let keys = |d| cut.xsheet.column(state.view.active_column).map_or(0, |c| c.keys_referencing(d).len());
        let tools_back = std::mem::replace(&mut self.say.tools_were_hidden, self.tools_hidden) && !self.tools_hidden;
        let facts = Facts {
            stage: self.stage,
            kind: self.kind(),
            drawings: cut.drawings.len(),
            art: state.has_art(),
            frame: state.view.frame,
            blank: state.blank_here(),
            ghost: state.view.onion,
            playing: state.view.playing,
            strip: !self.strip_closed() || self.full_band,
            // A cut shortened under its next key ends first.
            held: held.map(|(key, next)| next.unwrap_or(length).min(length).saturating_sub(key)),
            twice: shown.is_some_and(|d| keys(d) > 1),
            queue_up: visible_panes(&self.dock).contains(&Pane::RenderQueue),
            report: self.say.exported.as_ref().map(|file| file.name.clone()),
            plan_sheet: self.in_plan() && self.sheet_on_screen(),
            flat_pen: self.say.flat_strokes >= FLAT_STROKES,
            refused_twice: (self.say.refusals >= 2).then(|| self.say.refusal.clone()),
            card_room: self.say.card_room == Some(true),
            seen: self.say.seen,
            stood: self.say.stood,
            stroked,
            row_clicked,
            added,
            filled,
            effect_added,
            exported,
            pen_works: std::mem::take(&mut self.say.pen_answered),
            tools_back,
            lesson: self.lesson.is_some(),
            landed,
            transparent: state.render_queue.panel.transparent,
            mono: state.manga.export.mode == anim_core::deliver::PageMode::Mono,
        };
        if effect_added && self.kind() == Some(ProjectKind::Drawing) {
            self.tick_slot(2);
        }
        if panel_added {
            self.tick_slot(1);
        }
        facts
    }

    /// The action button, pressed. It runs what the Help window's rows run;
    /// only a Show me has an outcome to wait for, so the press answers the
    /// rest, however quick it was: whoever pressed it read the row.
    pub(crate) fn say_act(&mut self, verb: Verb, ctx: &eframe::egui::Context) {
        match verb {
            Verb::Show(slot) => {
                self.show_me(slot);
            }
            Verb::GoToDraw => {
                self.invoke(Action::RoomDrawing, ctx);
            }
            Verb::Fix(action) => {
                self.invoke(action, ctx);
                self.say.refusals = 0;
                self.say_answered(REFUSED_TWICE);
            }
            Verb::PenPage => {
                self.request_settings_pen = true;
                self.say.flat_strokes = 0;
                self.say_answered(FLAT_PEN);
            }
            Verb::OpenFile => {
                if let Some(file) = self.say.exported.take() {
                    self.requests.push(AppRequest::OpenFile(file.file));
                }
            }
        }
    }

    pub(crate) fn say_answered(&mut self, bit: u32) {
        if self.lesson.is_some() {
            return;
        }
        self.say.seen |= bit;
        self.say.seen_request = Some(self.say.seen);
    }

    /// Show me's circle (spec 8.3): the pen's own refusals first, then a ring
    /// by the route a raster stroke takes. Each frame's sits further along,
    /// so playing the drawings shows it move.
    pub(crate) fn demo_circle(&mut self) -> Performed {
        if self.canvas.stroke_active() {
            self.state.refuse(doc::FINISH_STROKE);
            return Performed::Ignored;
        }
        // AU28: no lesson calls `enter_room` (spec 5.10): the person walks.
        if !visible_panes(&self.dock).contains(&Pane::Canvas) {
            if self.lesson.is_some() {
                return Performed::Ignored;
            }
            self.enter_room(Stage::Drawing);
        }
        let Some(slot) = self.canvas.slot_at_pen_down(true, &mut self.state) else {
            return Performed::Ignored;
        };
        let project = &self.state.engine.project;
        let (w, h) = (project.width as f32, project.height as f32);
        let (radius, stroke) = (w.min(h) / 6.0, (w.min(h) / 120.0).max(2.0));
        let step = (self.state.view.frame % DEMO_STEPS) as f32 / DEMO_STEPS as f32;
        let center = [w * (0.3 + 0.4 * step), h * 0.5];
        let ink = self.canvas.ink();
        if !self.state.commit_ring(center, radius, stroke, ink, slot) {
            return Performed::Ignored;
        }
        self.shown_landed();
        Performed::Done
    }

    /// What follows a line Show me landed, the circle's or a lesson's.
    pub(crate) fn shown_landed(&mut self) {
        self.canvas.engine_changed();
        self.say.circle = true;
        // A check listening for a line is listening for the person's. This
        // one stands, and the check goes on listening.
        let heard = self.canvas.pen_verdict();
        if self.start.as_ref().is_some_and(|w| w.pen.read(heard).is_some()) {
            self.strokes_seen = self.state.strokes_done;
        }
        // The Help window's row asks with no frame to come yet.
        self.tick_stroke();
    }

    /// The next circle: on a drawing past the first frame, where the ticks
    /// read it.
    pub(crate) fn next_circle(&mut self) -> Performed {
        if self.state.view.frame == 0 && self.perform(Action::AddDrawingAfter) == Performed::Ignored {
            return Performed::Ignored;
        }
        self.demo_circle()
    }
}
