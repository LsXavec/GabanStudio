//! AU7: THE SAY LINE'S TEACHING (spec 8.2 to 8.5). One table, read once a
//! frame: the first row that applies speaks, and it stays until it is
//! answered. An answer lands in `ui.hints_seen`, so a row teaches once and
//! nothing here runs on a timer. A row's bit is stored: it keeps its number.
//!
//! The lane's order stays `Editor::say_line`'s: a refusal, the machine's last
//! remark, then what this file picks. Quick Start's ticks are read here too,
//! from outcomes only: the stroke signal, `AddDrawingAfter`, playback and a
//! queued export.

use crate::config::{Action, UiConfig};
use crate::viewer::ViewerView;
use crate::welcome::{self, PenCheck};
use crate::workspace::Stage;
use crate::{Editor, Pane, Performed, doc, menus, visible_panes};

pub const SHOW_ME: &str = "Show me";
pub const GO_TO_DRAW: &str = "Go to Draw";
pub const FIX_IT: &str = "Fix it";

/// The Plan preview's answer to a stroke, whether or not a hint can say it.
pub const PLAN_WORDS: &str = "This step shows a preview. Drawing happens in Draw.";
/// What the render queue asks of someone Show me sent there.
pub const QUEUE_WORDS: &str = "Pick a preset, press + this cut, then press Start.";

// The stored bits, one for each row of spec 8.2. 8 is AU9's `Exported ball.gif.`
// and 13 is AU10's card on the return dot.
pub const NEW_PAPER: u32 = 1 << 0;
pub const ADD_DRAWING: u32 = 1 << 1;
pub const NEXT_POSE: u32 = 1 << 2;
pub const PLAY: u32 = 1 << 3;
pub const SHARE: u32 = 1 << 4;
pub const PLAN: u32 = 1 << 5;
pub const REFUSED_TWICE: u32 = 1 << 6;
pub const FLAT_PEN: u32 = 1 << 7;
pub const COLOR: u32 = 1 << 9;
pub const EFFECTS: u32 = 1 << 10;
pub const APPEARS_TWICE: u32 = 1 << 11;
pub const HELD: u32 = 1 << 12;
/// The canvas line's bit (spec 8.4).
pub const EMPTY_CELL: u32 = 1 << 14;
pub const QUEUE: u32 = 1 << 15;

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
}

impl Verb {
    pub fn label(self) -> &'static str {
        match self {
            Verb::Show(_) => SHOW_ME,
            Verb::GoToDraw => GO_TO_DRAW,
            Verb::Fix(_) | Verb::PenPage => FIX_IT,
        }
    }

    pub fn card(self) -> &'static str {
        match self {
            Verb::Show(_) => "Do this step for me.",
            Verb::GoToDraw => Action::RoomDrawing.help(),
            Verb::Fix(action) => action.help(),
            Verb::PenPage => "Open Settings on the pen page.",
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
}

/// The document and the step as the table reads them, and what this frame
/// came to. Pure, so a test reaches every row without an editor.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub stage: Option<Stage>,
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
    /// Show me sent the person to the queue.
    pub export_asked: bool,
    /// A stroke was tried on the Plan preview.
    pub plan_tried: bool,
    /// Three pen strokes with no pressure in them.
    pub flat_pen: bool,
    /// A refusal said twice running.
    pub refused_twice: Option<String>,
    /// `ui.hints_seen`, as the frame began.
    pub seen: u32,
    // ---- this frame's outcomes ----
    pub stroked: bool,
    pub added: bool,
    pub filled: bool,
    pub effect_added: bool,
    pub exported: bool,
    pub pen_works: bool,
}

impl Facts {
    fn has_seen(&self, bit: u32) -> bool {
        self.seen & bit != 0
    }
}

/// One row of spec 8.2. `HINTS`' order is its precedence.
pub struct Hint {
    pub bit: u32,
    pub words: Words,
    pub verb: Option<Verb>,
    /// The steps it can speak in; none: any.
    pub steps: &'static [Stage],
    pub shows: fn(&Facts) -> bool,
    pub answered: fn(&Facts) -> bool,
    /// Said for a moment: the moment passing answers it.
    pub passes: bool,
}

const fn never(_: &Facts) -> bool {
    false
}

const DRAW: &[Stage] = &[Stage::Drawing];
const DRAWING_ROOMS: &[Stage] = &[Stage::Drawing, Stage::Manga];
const TRANSPORT: &[Stage] = &[Stage::Drawing, Stage::Edit];
const ON_A_FRAME: &[Stage] = &[Stage::Drawing, Stage::Edit, Stage::InkPaint];

pub const HINTS: [Hint; 13] = [
    Hint {
        bit: PLAN,
        words: Words::Plain(PLAN_WORDS),
        verb: Some(Verb::GoToDraw),
        steps: &[Stage::Layout],
        shows: |f| f.plan_tried,
        answered: never,
        passes: true,
    },
    Hint {
        bit: REFUSED_TWICE,
        words: Words::Reason,
        verb: None,
        steps: &[],
        shows: |f| f.refused_twice.is_some(),
        answered: never,
        passes: true,
    },
    Hint {
        bit: FLAT_PEN,
        words: Words::Plain("Your pen is drawing without pressure."),
        verb: Some(Verb::PenPage),
        steps: &[],
        shows: |f| f.flat_pen,
        answered: |f| f.pen_works,
        passes: false,
    },
    Hint {
        bit: APPEARS_TWICE,
        words: Words::Plain("This drawing appears twice. Changes show in both."),
        verb: None,
        steps: ON_A_FRAME,
        shows: |f| f.twice,
        answered: never,
        passes: true,
    },
    Hint {
        bit: HELD,
        words: Words::Held,
        verb: None,
        steps: ON_A_FRAME,
        shows: |f| f.held.is_some(),
        answered: never,
        passes: true,
    },
    Hint {
        bit: COLOR,
        words: Words::Plain("Pick a color, then tap an area."),
        verb: None,
        steps: &[Stage::InkPaint],
        shows: |f| f.art,
        answered: |f| f.filled,
        passes: false,
    },
    Hint {
        bit: EFFECTS,
        words: Words::Plain("Pick an effect to add it to the picture."),
        verb: None,
        steps: &[Stage::Composite],
        shows: |f| f.art,
        answered: |f| f.effect_added,
        passes: true,
    },
    // Quick Start's chain, in its own order.
    Hint {
        bit: NEW_PAPER,
        words: Words::Plain("Draw on the paper to begin."),
        verb: Some(Verb::Show(0)),
        steps: DRAWING_ROOMS,
        shows: |f| !f.art,
        answered: |f| f.stroked,
        passes: false,
    },
    Hint {
        bit: ADD_DRAWING,
        words: Words::Plain("Next: add a drawing."),
        verb: Some(Verb::Show(1)),
        steps: DRAW,
        shows: |f| f.art && f.drawings == 1 && f.strip,
        answered: |f| f.added,
        passes: false,
    },
    Hint {
        bit: NEXT_POSE,
        words: Words::Plain("The ghost is your last drawing. Draw the next pose."),
        verb: Some(Verb::Show(2)),
        steps: DRAW,
        shows: |f| f.art && f.blank && f.frame >= 1 && f.ghost,
        answered: |f| f.stroked && f.frame >= 1,
        passes: false,
    },
    Hint {
        bit: PLAY,
        words: Words::Chord("Press ", Action::PlayPause, " to play."),
        verb: Some(Verb::Show(3)),
        steps: TRANSPORT,
        shows: |f| f.drawings >= 3 && !f.blank && !f.playing,
        answered: |f| f.playing,
        passes: false,
    },
    Hint {
        bit: SHARE,
        words: Words::Plain("Ready to share? Go to Export."),
        verb: Some(Verb::Show(welcome::EXPORT_STEP)),
        steps: TRANSPORT,
        shows: |f| f.has_seen(PLAY) && f.drawings >= 2 && !f.playing && !f.queue_up,
        answered: |f| f.exported,
        passes: false,
    },
    Hint {
        bit: QUEUE,
        words: Words::Plain(QUEUE_WORDS),
        verb: None,
        steps: &[],
        shows: |f| f.queue_up && f.export_asked,
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
        (self.steps.is_empty() || f.stage.is_some_and(|s| self.steps.contains(&s))) && (self.shows)(f)
    }

    fn spoken(&self, f: &Facts, chord: &impl Fn(Action) -> Option<String>) -> Option<Spoken> {
        let refusal = f.refused_twice.as_deref();
        let words = match self.words {
            Words::Plain(words) => words.to_string(),
            Words::Chord(before, verb, after) => format!("{before}{}{after}", chord(verb)?),
            Words::Held => format!("This drawing stays on screen for {} frames.", f.held?),
            Words::Reason => reason(refusal?),
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
        doc::READ_ONLY_VIEW => Some(Action::ToggleCompositeView),
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
        let passed = hint.passes && shown == Some(hint.bit) && !hint.applies(f);
        if (hint.answered)(f) || passed {
            *seen |= hint.bit;
        }
    }
    let due = |hint: &&Hint| *seen & hint.bit == 0 && hint.applies(f);
    HINTS.iter().filter(due).find_map(|hint| hint.spoken(f, &chord))
}

/// The empty state of the step `f` stands in.
pub fn empty_step(f: &Facts, hints_off: bool) -> Option<Spoken> {
    let row = EMPTY_STEPS.iter().find(|e| f.stage == Some(e.stage) && (e.applies)(f))?;
    let verb = (!hints_off).then_some(Verb::GoToDraw);
    Some(Spoken { bit: None, words: row.words.to_string(), verb })
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
    /// `AddDrawingAfter` ran since the last reading.
    pub added: bool,
    /// Show me sent the person to the queue.
    pub export_asked: bool,
    /// The stroke just read was `Check your pen`'s.
    pub check_line: bool,
    /// The five dots painted this frame.
    pub dots_painted: bool,
    /// A sentence a test holds on the lane, whatever the document says.
    #[cfg(test)]
    pub pin: Option<Spoken>,
    strokes: Option<u64>,
    fills: Option<u64>,
    nodes: Option<usize>,
    plan_tried: bool,
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

/// Pen strokes with no pressure before the hint says so (spec 8.2).
const FLAT_STROKES: u32 = 3;
/// The demo circle's places along the paper, one a frame.
const DEMO_STEPS: u32 = 8;

impl Say {
    /// The shell's config, each frame: the switch and the bits.
    pub fn feed(&mut self, ui: &UiConfig) {
        (self.off, self.seen) = (ui.hints_off, ui.hints_seen);
    }

    /// Another document: nothing that differs from the last one is an outcome.
    pub fn new_document(&mut self) {
        (self.strokes, self.fills, self.nodes) = (None, None, None);
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
    /// says it instead.
    pub fn empty_cell_due(&self, state: &doc::AppState) -> bool {
        !self.off && self.seen & EMPTY_CELL == 0 && !state.cut().drawings.is_empty()
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
    /// AU7: what this frame came to: Quick Start's ticks, and the row that
    /// speaks next. Runs after the dock, where a stroke lands.
    pub(crate) fn say_read(&mut self, ctx: &eframe::egui::Context) {
        let (stroked, filled) = self.say_outcomes();
        self.say_plan();
        self.say.count_refusals(&self.state);
        let facts = self.say_facts(stroked, filled);
        let mut seen = self.say.seen;
        let hint = if self.say.off {
            None
        } else {
            seen |= if stroked && self.say.line_shown { EMPTY_CELL } else { 0 };
            read(&facts, &mut seen, self.say.shown, |a| menus::chord_label(ctx, a))
        };
        if seen != self.say.seen {
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

    /// The stroke, fill and playback signals, read once, and what they tick.
    /// A line drawn for `Check your pen` was the check's: it ticks nothing.
    fn say_outcomes(&mut self) -> (bool, bool) {
        let state = &self.state;
        let check_line = std::mem::take(&mut self.say.check_line);
        let stroked = self.say.strokes.is_some_and(|seen| seen != state.strokes_done) && !check_line;
        let filled = self.say.fills.is_some_and(|seen| seen != state.fills_done);
        (self.say.strokes, self.say.fills) = (Some(state.strokes_done), Some(state.fills_done));
        if stroked {
            self.tick_stroke();
            match self.canvas.pen_verdict() {
                PenCheck::NoPressure => self.say.flat_strokes += 1,
                PenCheck::Works => {
                    self.say.flat_strokes = 0;
                    self.say.pen_answered = true;
                }
                _ => {}
            }
        }
        self.quick_start[welcome::PLAY_STEP] |= self.state.view.playing;
        (stroked, filled)
    }

    /// What a finished stroke ticks (spec 8.3).
    fn tick_stroke(&mut self) {
        self.quick_start[welcome::FIRST_STROKE] = true;
        self.quick_start[welcome::NEXT_POSE] |= self.state.view.frame >= 1;
    }

    /// A stroke tried on the Plan preview always gets an answer: a hint the
    /// first time, then the same sentence as a remark.
    fn say_plan(&mut self) {
        let tried = self.viewers.get_mut(&u8::MAX).is_some_and(|vs: &mut ViewerView| std::mem::take(&mut vs.tried));
        if tried && (self.say.off || self.say.seen & PLAN != 0) {
            self.state.status = PLAN_WORDS.into();
            self.status_seen.clear();
        }
        self.say.plan_tried = (self.say.plan_tried || tried) && self.stage == Some(Stage::Layout);
    }

    fn say_facts(&mut self, stroked: bool, filled: bool) -> Facts {
        let state = &self.state;
        let cut = state.cut();
        let nodes = cut.graph.nodes().count();
        let effect_added = self.say.nodes.is_some_and(|before| nodes > before);
        self.say.nodes = Some(nodes);
        let shown = state.current_drawing();
        let held = state.own_key_drawing().is_none().then(|| state.exposure_here()).flatten();
        let held = held.filter(|_| shown.is_some());
        let length = state.frame_count();
        let keys = |d| cut.xsheet.column(state.view.active_column).map_or(0, |c| c.keys_referencing(d).len());
        Facts {
            stage: self.stage,
            drawings: cut.drawings.len(),
            art: state.has_art(),
            frame: state.view.frame,
            blank: state.blank_here(),
            ghost: state.view.onion,
            playing: state.view.playing,
            strip: !self.strip_closed() || self.full_band,
            held: held.map(|(key, next)| next.unwrap_or(length) - key),
            twice: shown.is_some_and(|d| keys(d) > 1),
            queue_up: visible_panes(&self.dock).contains(&Pane::RenderQueue),
            export_asked: self.say.export_asked,
            plan_tried: self.say.plan_tried,
            flat_pen: self.say.flat_strokes >= FLAT_STROKES,
            refused_twice: (self.say.refusals >= 2).then(|| self.say.refusal.clone()),
            seen: self.say.seen,
            stroked,
            added: std::mem::take(&mut self.say.added),
            filled,
            effect_added,
            exported: state.render_queue.busy(),
            pen_works: std::mem::take(&mut self.say.pen_answered),
        }
    }

    /// The action button, pressed. It runs what the Help window's rows run;
    /// the two fixes have no outcome to wait for, so the press answers them.
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
            }
            Verb::PenPage => {
                self.request_settings_pen = true;
                self.say.flat_strokes = 0;
                self.say_answered(FLAT_PEN);
            }
        }
    }

    fn say_answered(&mut self, bit: u32) {
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
        if !visible_panes(&self.dock).contains(&Pane::Canvas) {
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
        self.canvas.engine_changed();
        // The Help window's row asks with no frame to come yet.
        self.tick_stroke();
        Performed::Done
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
