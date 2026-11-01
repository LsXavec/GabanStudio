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

/// What a path of a lesson's art is for (spec 5.13).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathKind {
    Guide,
    Landmark,
    StandIn,
    Seed,
    Finish,
    /// Rubbed out, not drawn: an eraser's stroke (Up Late's `wipe`).
    Wipe,
}

/// A line of a lesson's art, in paper units, which are the paper's pixels
/// (spec 5.13).
pub struct Path {
    /// `(x, y, pressure)`.
    pub points: Vec<[f32; 3]>,
    pub width: f32,
    pub kind: PathKind,
    /// The guide paints it above an opaque background.
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

    /// Shorter than it is wide: a press of the pen, not a line (spec 5.14).
    pub fn is_press(&self) -> bool {
        let long: f32 = self.points.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
        long < self.width
    }
}

/// AU28: lines by a hand, and Deckle, from curves in code: no binary assets
/// (spec 5.13). Nothing here is symmetrical and nothing is ruled.
pub mod art {
    use super::{Path, PathKind};

    pub type Pt = [f32; 2];

    /// The samples of one cubic.
    const CUBIC_ROWS: usize = 32;
    /// The gap between a hand line's samples, in paper units.
    pub const HAND_STEP: f32 = 3.0;
    /// A hand's wobble: how far it strays, and over what run: a short line
    /// gets its share of the run too, or it is one bow and reads as a curve.
    const WOBBLE: f32 = 1.0;
    const WOBBLE_RUN: f32 = 34.0;
    const WOBBLE_RUNS: f32 = 5.0;
    /// A line's ends hold still over this run: a joined line stays joined.
    const WOBBLE_PIN: f32 = 12.0;
    const WOBBLE_PINS: f32 = 6.0;
    /// The pen on a straight run, full pressure being the middle of a turn.
    const LIGHT: f32 = 0.62;
    /// What a turn over `TURN_READ` samples takes to reach full pressure.
    const TURN_FULL: f32 = 0.7;
    const TURN_READ: usize = 4;
    /// The share of a line its ends take to swell, and where they start.
    const TIP_SHARE: f32 = 0.12;
    const TIP: f32 = 0.2;
    /// The strip's width, of its length.
    const STRIP_WIDE: f32 = 0.2;
    /// The tear: fibres pulled one way, so a long easy slope, then a short
    /// sharp nick back. A slope runs this far, and starts and ends this far
    /// off the line. Each nick is a corner, so what lies between reads as a
    /// hand's line, and no run is long enough to count as one on its own.
    const TEAR_GAP: [f32; 2] = [26.0, 54.0];
    const TEAR_TOP: [f32; 2] = [7.0, 10.0];
    const TEAR_FOOT: [f32; 2] = [3.0, 6.0];
    const TEAR_NICK: f32 = 3.0;
    /// The wobble's run along the tear: the hand's own would be one bow per
    /// slope, and a slope that bows is a smooth curve.
    const TEAR_RUN: f32 = 14.0;
    /// How far a line runs past a corner.
    const OVERSHOOT: f32 = 3.5;
    /// Where the eye and the two holes stand: along the strip, tail to head,
    /// and across it, cut side to torn. The eye looks up and ahead; the holes
    /// are the punch's, in a row by the straight edge.
    const EYE_AT: [f32; 2] = [0.75, 0.6];
    const HOLES_AT: [[f32; 2]; 2] = [[0.3, 0.2], [0.55, 0.2]];
    /// A dab's width, of the strip's: an eye is a good deal more than a hole.
    const EYE_WIDE: f32 = 0.3;
    const HOLE_WIDE: f32 = 0.13;
    /// The ink line's width, of the strip's.
    const LINE_WIDE: f32 = 0.06;

    /// A cubic, sampled.
    pub fn cubic(p: [Pt; 4]) -> Vec<Pt> {
        let at = |t: f32| {
            let s = 1.0 - t;
            let (a, b, c, d) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
            [0, 1].map(|i| a * p[0][i] + b * p[1][i] + c * p[2][i] + d * p[3][i])
        };
        (0..=CUBIC_ROWS).map(|i| at(i as f32 / CUBIC_ROWS as f32)).collect()
    }

    /// Cubics in a chain, from `start`, each `[control, control, end]`.
    pub fn curve(start: Pt, runs: &[[Pt; 3]]) -> Vec<Pt> {
        let mut out = vec![start];
        for [a, b, end] in runs {
            let from = out[out.len() - 1];
            out.extend_from_slice(&cubic([from, *a, *b, *end])[1..]);
        }
        out
    }

    pub fn length(points: &[Pt]) -> f32 {
        points.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum()
    }

    /// `points` again, `step` apart along the line, both ends kept.
    pub fn spaced(points: &[Pt], step: f32) -> Vec<Pt> {
        let total = length(points);
        let rows = ((total / step).round() as usize).max(1);
        let gap = total / rows as f32;
        let (mut out, mut walked, mut next) = (Vec::with_capacity(rows + 1), 0.0, 0.0);
        for w in points.windows(2) {
            let long = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
            while out.len() < rows && next <= walked + long && long > 0.0 {
                let t = (next - walked) / long;
                out.push([w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t]);
                next += gap;
            }
            walked += long;
        }
        out.extend(points.last());
        out
    }

    /// A seeded value at each whole `i`, in -1..1.
    fn lattice(seed: u32, i: i32) -> f32 {
        let mut h = (i as u32).wrapping_mul(0x9E37_79B1) ^ seed.wrapping_mul(0x85EB_CA77);
        h ^= h >> 15;
        h = h.wrapping_mul(0xC2B2_AE3D);
        h ^= h >> 13;
        h as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// Smooth seeded noise: repeatable, so a pose is the same every run.
    fn noise(seed: u32, x: f32) -> f32 {
        let (i, t) = (x.floor(), x - x.floor());
        let t = t * t * (3.0 - 2.0 * t);
        let (a, b) = (lattice(seed, i as i32), lattice(seed, i as i32 + 1));
        a + (b - a) * t
    }

    /// The way out of a line at sample `i`, a quarter turn from its run.
    fn side(points: &[Pt], i: usize) -> Pt {
        let (a, b) = (points[i.saturating_sub(1)], points[(i + 1).min(points.len() - 1)]);
        let (run, rise) = (b[0] - a[0], b[1] - a[1]);
        let long = run.hypot(rise).max(1e-3);
        [rise / long, -run / long]
    }

    /// `points` pushed out along their sides by `by(i)`.
    fn pushed(points: &[Pt], by: impl Fn(usize) -> f32) -> Vec<Pt> {
        let at = |i: usize| {
            let (s, d) = (side(points, i), by(i));
            [points[i][0] + s[0] * d, points[i][1] + s[1] * d]
        };
        (0..points.len()).map(at).collect()
    }

    /// A hand's wobble along `points`: `WOBBLE` at its widest, gone at the ends.
    pub fn wobble(points: &[Pt], seed: u32) -> Vec<Pt> {
        wobbled(points, seed, WOBBLE_RUN)
    }

    /// The wobble at a run of at most `most`.
    fn wobbled(points: &[Pt], seed: u32, most: f32) -> Vec<Pt> {
        let line = spaced(points, HAND_STEP);
        let last = line.len() - 1;
        let long = last as f32 * HAND_STEP;
        let (run, pin) = (most.min(long / WOBBLE_RUNS), WOBBLE_PIN.min(long / WOBBLE_PINS));
        let raw: Vec<f32> = (0..=last)
            .map(|i| {
                let (from, to) = (i as f32 * HAND_STEP, (last - i) as f32 * HAND_STEP);
                noise(seed, from / run) * (from.min(to) / pin).min(1.0)
            })
            .collect();
        let peak = raw.iter().fold(0.0f32, |p, v| p.max(v.abs())).max(1e-3);
        pushed(&line, |i| raw[i] / peak * WOBBLE)
    }

    /// A pen's pressure along `points`: light at the tips, heavier on the turn.
    pub fn taper(points: &[Pt]) -> Vec<[f32; 3]> {
        let last = points.len().saturating_sub(1);
        let heading = |a: Pt, b: Pt| (b[1] - a[1]).atan2(b[0] - a[0]);
        let turn = |i: usize| {
            let (before, after) = (i.saturating_sub(TURN_READ), (i + TURN_READ).min(last));
            if before == i || after == i {
                return 0.0;
            }
            let bend = heading(points[i], points[after]) - heading(points[before], points[i]);
            bend.sin().atan2(bend.cos()).abs()
        };
        let press = |i: usize| {
            let t = i as f32 / last.max(1) as f32;
            let tip = (t.min(1.0 - t) / TIP_SHARE).min(1.0);
            let body = LIGHT + (1.0 - LIGHT) * (turn(i) / TURN_FULL).min(1.0);
            let p = points[i];
            [p[0], p[1], body * (TIP + (1.0 - TIP) * tip.sqrt())]
        };
        (0..=last).map(press).collect()
    }

    /// `points` as a person's line.
    pub fn hand(points: &[Pt], width: f32, kind: PathKind, seed: u32) -> Path {
        Path { points: taper(&wobble(points, seed)), width, kind, over: false }
    }

    /// A press of the pen: a dot, a shade off round.
    pub fn dab(at: Pt, width: f32, kind: PathKind) -> Path {
        let points = vec![[at[0], at[1], 0.9], [at[0] + width * 0.08, at[1] + width * 0.05, 1.0]];
        Path { points, width, kind, over: false }
    }

    /// A spine from `tail` to `head`, `long` along the line wherever they
    /// stand: nearer than `long`, it arches up off the paper, and more to the
    /// head than the tail, because the strip is being pushed from behind.
    pub fn spine(tail: Pt, head: Pt, long: f32) -> Vec<Pt> {
        const ROWS: usize = 96;
        const LEAN: f32 = 0.8;
        let arched = |rise: f32| -> Vec<Pt> {
            let at = |i: usize| {
                let t = i as f32 / ROWS as f32;
                let up = (std::f32::consts::PI * t.powf(LEAN)).sin() * rise;
                [tail[0] + (head[0] - tail[0]) * t, tail[1] + (head[1] - tail[1]) * t - up]
            };
            (0..=ROWS).map(at).collect()
        };
        let (mut low, mut high) = (0.0, long);
        for _ in 0..32 {
            let mid = (low + high) * 0.5;
            if length(&arched(mid)) < long {
                low = mid;
            } else {
                high = mid;
            }
        }
        spaced(&arched(low), long / ROWS as f32)
    }

    /// The walk: flat, then the tail catches up with a head that stays put,
    /// then flat with the head reaching on and a tail that stays put. Each
    /// pose keeps one foot of the last. `reach` is how far a stride goes.
    pub fn walk(tail: Pt, long: f32, reach: f32, poses: usize) -> Vec<Vec<Pt>> {
        let stride = |i: usize| {
            let hops = ((i + 1) / 2) as f32 * reach;
            let from = [tail[0] + hops, tail[1]];
            let to = [tail[0] + (i / 2) as f32 * reach + long, tail[1]];
            spine(from, to, long)
        };
        (0..poses).map(stride).collect()
    }

    /// Deckle: two lines and three dots.
    pub struct Deckle {
        /// The cut side, the spine itself, tail to head, then round the head.
        pub cut: Path,
        /// The torn side, head to tail, then round the tail, which shuts it.
        pub torn: Path,
        /// The samples of `cut` and `torn` before the turn round their end.
        pub sides: [usize; 2],
        pub eye: Path,
        pub holes: [Path; 2],
    }

    impl Deckle {
        pub fn paths(self) -> Vec<Path> {
            let [first, second] = self.holes;
            vec![self.cut, self.torn, self.eye, first, second]
        }
    }

    /// The tear's level at each whole `HAND_STEP` along a strip this `long`:
    /// the noise runs along the strip, not across the paper, so the seed tears
    /// the same edge on every pose (spec 5.13).
    fn tear(long: f32, seed: u32) -> Vec<f32> {
        let mix = |[low, high]: [f32; 2], i: i32| low + (high - low) * (lattice(seed, i) + 1.0) * 0.5;
        let mut knots = vec![(0.0, mix(TEAR_TOP, 0))];
        let mut tooth = 0;
        while let Some(&(from, _)) = knots.last() {
            let to = from + mix(TEAR_GAP, 3 * tooth + 1);
            if to + TEAR_NICK >= long {
                break;
            }
            knots.push((to, -mix(TEAR_FOOT, 3 * tooth + 2)));
            knots.push((to + TEAR_NICK, mix(TEAR_TOP, 3 * tooth + 3)));
            tooth += 1;
        }
        knots.push((long, -mix(TEAR_FOOT, 3 * tooth + 2)));
        let at = |s: f32| {
            let next = knots.iter().position(|k| k.0 >= s).unwrap_or(knots.len() - 1).max(1);
            let ((a, from), (b, to)) = (knots[next - 1], knots[next]);
            from + (to - from) * ((s - a) / (b - a).max(1e-3)).clamp(0.0, 1.0)
        };
        (0..=(long / HAND_STEP).round() as usize).map(|i| at(i as f32 * HAND_STEP)).collect()
    }

    /// Deckle along `spine`, its cut side. Paper does not stretch, so the
    /// strip's width is one way across the whole pose, as a ribbon's is: the
    /// torn side is the spine moved, and keeps its length through any bend.
    /// Pushed out along the bend's normal, the arch's outer side would run
    /// half as long again (spec 5.13).
    pub fn deckle(spine: &[Pt], seed: u32) -> Deckle {
        let long = length(spine);
        let wide = long * STRIP_WIDE;
        let spine = spaced(spine, HAND_STEP);
        let last = spine.len() - 1;
        // Up off the paper, whichever way the strip is headed.
        let chord = [spine[last][0] - spine[0][0], spine[last][1] - spine[0][1]];
        let reach = chord[0].hypot(chord[1]).max(1e-3);
        let up = if chord[0] < 0.0 { -1.0 } else { 1.0 };
        let across = [chord[1] / reach * up, -chord[0] / reach * up];
        let moved = |p: Pt, by: f32| [p[0] + across[0] * by, p[1] + across[1] * by];
        let ribbon: Vec<Pt> = spine.iter().map(|p| moved(*p, wide)).collect();
        let tear = tear(long, seed);
        let back = pushed(&ribbon, |i| up * tear[i.min(tear.len() - 1)]);
        let line = wide * LINE_WIDE;
        // On the strip: `[along it, tail to head; across it, cut side to torn]`.
        let stood = |[along, over]: [f32; 2]| moved(spine[(along * last as f32).round() as usize], wide * over);
        let past = |from: Pt, to: Pt| {
            let long = (to[0] - from[0]).hypot(to[1] - from[1]).max(1e-3);
            [to[0] + (to[0] - from[0]) / long * OVERSHOOT, to[1] + (to[1] - from[1]) / long * OVERSHOOT]
        };
        // A corner is a hand's line too, and sampled as one: `taper` reads
        // samples, and one long last segment would all be the pen's thin tip,
        // and leak. Its first sample is the corner, which holds still.
        let round = |from: Pt, to: Pt, seed: u32| wobbled(&[from, past(from, to)], seed, TEAR_RUN).split_off(1);
        let mut cut = wobble(&spine, seed);
        let mut torn = wobbled(&back.iter().rev().copied().collect::<Vec<Pt>>(), seed.wrapping_add(1), TEAR_RUN);
        let sides = [cut.len(), torn.len()];
        cut.extend(round(spine[last], back[back.len() - 1], seed.wrapping_add(2)));
        torn.extend(round(back[0], spine[0], seed.wrapping_add(3)));
        Deckle {
            cut: Path { points: taper(&cut), width: line, kind: PathKind::Guide, over: false },
            torn: Path { points: taper(&torn), width: line, kind: PathKind::Guide, over: false },
            sides,
            eye: dab(stood(EYE_AT), wide * EYE_WIDE, PathKind::Guide),
            holes: HOLES_AT.map(|at| dab(stood(at), wide * HOLE_WIDE, PathKind::Guide)),
        }
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
