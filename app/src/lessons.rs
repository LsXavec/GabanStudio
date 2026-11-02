//! AU20: the lesson table, as the Lessons page reads it (spec 4.5). AU27: a
//! lesson's paper and tasks, the types the frame in `lesson.rs` reads (spec 5.3).
//! Lesson papers are `Template` consts here, outside `ALL` and `INTENTS`. AU23:
//! the three Rookie lessons, as `research/LESSONS-approachable-ui.md` gives them.

// The vocabulary is whole now; the lessons that use all of it come with AU28.
#![allow(dead_code)]

use anim_core::camera::IRect;
use anim_core::fill::{FillOpts, flood_fill_mask_in};
use anim_core::fx::FxKind;
use anim_core::generator::GenKind;
use anim_core::graph::NodeKind;
use anim_core::manga::Page;
use anim_core::model::{CelLayer, Drawing, LayerRole};
use anim_core::xsheet::{ColumnKind, Exposure};
use eframe::egui::{Color32, pos2};

use art::Pt;

use crate::canvas::CanvasTool;
use crate::config::Action;
use crate::doc::{self, PathInk};
use crate::manga_tools::{self, PanelOp};
use crate::rail::Key;
use crate::steps::ProjectKind;
use crate::templates::{self, Template};
use crate::workspace::{Pane, Stage};
use crate::{Editor, lesson, plate};

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

// ---- AU23: the three Rookie lessons (`research/LESSONS-approachable-ui.md`) ----

const fn rgb(color: Color32) -> [u8; 3] {
    [color.r(), color.g(), color.b()]
}

/// A swatch of the starter row, by its place in it.
const fn swatch(i: usize) -> Color32 {
    plate::STARTER_SWATCHES[i].1
}

const BLACK_INK: Color32 = swatch(0);
const WHITE_INK: Color32 = swatch(1);
const RED_INK: Color32 = swatch(2);
const YELLOW_INK: Color32 = swatch(4);
const NIGHT_INK: Color32 = plate::LESSON_NIGHT;

/// A task with no line of its own.
const fn act(say: &'static str, step: Stage, done: Done, ring: &'static [Target], show: Show) -> Task {
    Task { say, why: None, step, done, ring, guide: &[], stand_in: &[], miss: None, show }
}

/// A task that draws `paths` over their guides.
const fn trace(say: &'static str, step: Stage, paths: &'static [u16], ring: &'static [Target]) -> Task {
    Task { guide: paths, ..act(say, step, Done::Ink(paths), ring, Show::Trace(paths)) }
}

/// A later step's first task: its pill, and why the step is there.
const fn pill(say: &'static str, why: &'static str, stage: Stage, action: Action) -> Task {
    let ring: &'static [Target] = match stage {
        Stage::Edit => &[Target::Step(Stage::Edit)],
        Stage::InkPaint => &[Target::Step(Stage::InkPaint)],
        Stage::Composite => &[Target::Step(Stage::Composite)],
        Stage::Export => &[Target::Step(Stage::Export)],
        _ => &[],
    };
    Task { why: Some(why), ..act(say, stage, Done::OnStep(stage), ring, Show::Perform(action)) }
}

/// A closed shape's seed: a fill at the lessons' own gap.
const fn fill(say: &'static str, step: Stage, seed: [f32; 2], keep: [f32; 2]) -> Task {
    let done = Done::Filled { seed, keep: Some(keep) };
    act(say, step, done, &[Target::Rail(Key::Fill)], Show::Fill(seed))
}

const ON_PAPER: &[Target] = &[Target::Paper];
const BRUSH_KEY: &[Target] = &[Target::Rail(Key::Brush)];
const FILL_KEY: &[Target] = &[Target::Rail(Key::Fill)];
const GLOW_KEY: &[Target] = &[Target::Named("shelf.glow")];

/// A step's verbs stand on its own screen: from anywhere else the card is
/// pointing back, and nothing reaches past a live pen.
fn at_step(e: &mut Editor, step: Stage) -> bool {
    if e.canvas.stroke_active() {
        e.state.refuse(doc::FINISH_STROKE);
        return false;
    }
    e.stage == Some(step)
}

/// A swatch's own act.
fn pick(e: &mut Editor, color: Color32) {
    if at_step(e, Stage::InkPaint) {
        e.canvas.brush_color = color.to_array();
        e.state.active_chip = None;
    }
}

fn pick_night(e: &mut Editor) {
    pick(e, NIGHT_INK);
}

fn pick_white(e: &mut Editor) {
    pick(e, WHITE_INK);
}

fn pick_red(e: &mut Editor) {
    pick(e, RED_INK);
}

fn pick_yellow(e: &mut Editor) {
    pick(e, YELLOW_INK);
}

fn pick_black(e: &mut Editor) {
    pick(e, BLACK_INK);
}

/// `Tap {Glow}`: the shelf's own add.
fn add_glow(e: &mut Editor) {
    if at_step(e, Stage::Composite) {
        e.state.shelf_add(NodeKind::effect(FxKind::Glow));
    }
}

/// A spine from `tail` to `head`, `long` along, its bend spread round the whole
/// arch: `art::spine`'s sine turns at the peak of so tall a stand as tight as a
/// fold, and Up Late's Deckle is large enough for a fold to read as a kink.
fn arch(tail: Pt, head: Pt, long: f32) -> Vec<Pt> {
    /// How far the shoulders swell past the feet, of the slack.
    const FLARE: f32 = 0.22;
    /// The tail's shoulder is the fuller: he has just come up off it.
    const LEAN: f32 = 0.9;
    let flare = (long - (head[0] - tail[0]).hypot(head[1] - tail[1])) * FLARE;
    let bowed = |rise: f32| {
        art::cubic([tail, [tail[0] - flare, tail[1] - rise], [head[0] + flare * LEAN, head[1] - rise * LEAN], head])
    };
    let (mut low, mut high) = (0.0, long * 2.0);
    for _ in 0..32 {
        let mid = (low + high) * 0.5;
        if art::length(&bowed(mid)) < long {
            low = mid;
        } else {
            high = mid;
        }
    }
    bowed(high)
}

// ---- Up Late ----

const UP_LATE: Template = Template {
    name: "Up Late",
    kind: Some(ProjectKind::Drawing),
    width: 1200,
    height: 1200,
    ..templates::READY
};

// Its art, in the order `up_late_art` lays it down.
const BACK: u16 = 0;
const BELLY: u16 = 1;
const HOLE_1: u16 = 2;
const HOLE_2: u16 = 3;
const HOLE_3: u16 = 4;
const PENCIL: u16 = 5;
const SCRIBBLE: u16 = 6;
const SMUDGE: u16 = 7;
const WIPE: u16 = 8;
const STAR_1: u16 = 9;
const STAR_2: u16 = 10;
const STAR_3: u16 = 11;
const OUTLINE: u16 = 12;
const RIM_1: u16 = 13;
const RIM_2: u16 = 14;
const RIM_3: u16 = 15;

pub(crate) const SKY_SEED: [f32; 2] = [100.0, 100.0];
pub(crate) const BODY_SEED: [f32; 2] = [560.0, 545.0];
pub(crate) const PENCIL_SEED: [f32; 2] = [600.0, 830.0];
pub(crate) const STARS: [Pt; 3] = [[850.0, 240.0], [1000.0, 380.0], [760.0, 130.0]];
/// A star's press: wide enough that Glow's halo still shows 30 px from its
/// middle (the lessons file's file fact).
const STAR_WIDE: f32 = 44.0;
/// Past Glow's reach over the white body: a hole this wide stays dark. The
/// strip's own, 13 px here, comes out of the file as a grey ghost of itself.
const HOLE_WIDE: f32 = 20.0;
/// How far off its middle the finish reads a star. Glow burns the middle nearly
/// white, and a reading of ink takes that for paper; the rim keeps its colour.
const STAR_RIM: f32 = 25.0;
/// The strip, tail foot to front foot: the back foot stands on the pencil, and
/// the front is lifted clear of it by more than the lesson's gap, so a night
/// poured over the sky reaches under the arch. Its peak is the torn side's.
const ARCH: [Pt; 2] = [[420.0, 797.0], [700.0, 778.0]];
const ARCH_LONG: f32 = 500.0;
const SMUDGE_GREY: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
/// `size_up`'s share: a little bigger.
const SIZE_UP: f32 = 1.6;

fn up_late_art() -> Vec<Path> {
    use art::{dab, hand};
    let deckle = art::deckle(&arch(ARCH[0], ARCH[1], ARCH_LONG), 1);
    let hole = |path: &Path| art::dab([path.points[0][0], path.points[0][1]], HOLE_WIDE, PathKind::Guide);
    let [hole_2, hole_3] = [hole(&deckle.holes[0]), hole(&deckle.holes[1])];
    let width = deckle.cut.width;
    let [outline, _] = closed(&arch(ARCH[0], ARCH[1], ARCH_LONG), 1);
    // One closed stroke: its end runs back over its start.
    let pencil = [[184.0, 802.0], [860.0, 801.0], [1018.0, 830.0], [862.0, 860.0], [182.0, 858.0], [183.0, 800.0], [240.0, 802.0]];
    let scribble = [[900.0, 960.0], [1082.0, 985.0], [905.0, 1022.0], [1076.0, 1050.0], [912.0, 1086.0], [1068.0, 1112.0]];
    let smudge = [[176.0, 1006.0], [286.0, 992.0], [182.0, 1038.0], [282.0, 1030.0], [190.0, 1066.0], [270.0, 1062.0]];
    let wipe = [[120.0, 985.0], [340.0, 978.0], [120.0, 1030.0], [340.0, 1026.0], [120.0, 1078.0], [340.0, 1074.0]];
    let star = |at: Pt| dab(at, STAR_WIDE, PathKind::Guide);
    let rim = |at: Pt| dab([at[0] + STAR_RIM, at[1]], STAR_WIDE, PathKind::Finish);
    vec![
        deckle.torn,
        deckle.cut,
        deckle.eye,
        hole_2,
        hole_3,
        hand(&pencil, width, PathKind::Guide, 11),
        hand(&scribble, width, PathKind::Guide, 22),
        hand(&smudge, 22.0, PathKind::Landmark, 37),
        hand(&wipe, 90.0, PathKind::Wipe, 14),
        star(STARS[0]),
        star(STARS[1]),
        star(STARS[2]),
        outline,
        rim(STARS[0]),
        rim(STARS[1]),
        rim(STARS[2]),
    ]
}

/// One grey smudge on the line layer: the paper's first drawing, by the route a
/// stroke takes, and no stroke of the person's.
fn up_late_prep(s: &mut crate::doc::AppState) {
    let art = up_late_art();
    let smudge = &art[usize::from(SMUDGE)];
    let ink = PathInk { points: &smudge.points, width: smudge.width, rgba: SMUDGE_GREY, erase: false, inside: false };
    s.commit_path(&[ink], 0);
    s.strokes_done = 0;
}

fn size_up(e: &mut Editor) {
    if at_step(e, Stage::Drawing) {
        let px = e.canvas.active_px();
        e.canvas.set_brush_px(px * SIZE_UP, &mut e.state);
    }
}

fn arm_brush(e: &mut Editor) {
    if at_step(e, Stage::InkPaint) {
        e.canvas.arm_paint(false, &mut e.state);
    }
}

/// A night poured at the sky would reach Deckle's body: the lines leak.
fn night_leaks(e: &Editor) -> bool {
    let paper = IRect { x: 0, y: 0, w: UP_LATE.width, h: UP_LATE.height };
    let opts = FillOpts { threshold: 0.1, gap_px: e.canvas.fill_gap, grow_px: 0, protect_ink: false };
    let seed = (SKY_SEED[0] as i32, SKY_SEED[1] as i32);
    let lines = e.state.display_cel_flatten().unwrap_or_default();
    flood_fill_mask_in(&lines, seed, paper, &opts, None).is_ok_and(|mask| mask.get(BODY_SEED[0] as i32, BODY_SEED[1] as i32))
}

/// Task 11's `Show me`: the fill, and first the closed outline where the lines
/// leak. A person's fill arms the Fill key on the way: this does too, or
/// `Press {Brush}` would stand answered before it was asked.
fn close_and_fill(e: &mut Editor) {
    if !at_step(e, Stage::InkPaint) || !crate::visible_panes(&e.dock).contains(&Pane::Canvas) {
        return;
    }
    if e.canvas.slot_at_pen_down(true, &mut e.state).is_none() {
        return;
    }
    e.canvas.set_tool(CanvasTool::Fill, &mut e.state);
    if !night_leaks(e) || e.lesson_land(&[OUTLINE]) {
        e.canvas.fill_at(pos2(SKY_SEED[0], SKY_SEED[1]), &mut e.state);
    }
}

const UP_LATE_TASKS: &[Task] = &[
    Task {
        why: Some("Draw is where the picture starts."),
        ..trace(
            "This is Deckle, a strip of animation paper that tore off and got away. Draw over the blue line.",
            Stage::Drawing,
            &[BACK],
            BRUSH_KEY,
        )
    },
    trace("Now the line underneath. Join it to the first line at both ends.", Stage::Drawing, &[BELLY], ON_PAPER),
    act(
        "Drag {Size} to make your brush a little bigger.",
        Stage::Drawing,
        Done::BrushChanged,
        &[Target::Rail(Key::More), Target::Rail(Key::Size)],
        Show::Run(size_up),
    ),
    trace(
        "Press once on each blue dot. These holes held the paper still. The first one is looking at you.",
        Stage::Drawing,
        &[HOLE_1, HOLE_2, HOLE_3],
        ON_PAPER,
    ),
    trace("Draw the pencil Deckle is standing on.", Stage::Drawing, &[PENCIL], ON_PAPER),
    act("Scribble anywhere.", Stage::Drawing, Done::Strokes(1), ON_PAPER, Show::Trace(&[SCRIBBLE])),
    act(
        "Now press {Undo}. It is gone. Nothing here can break.",
        Stage::Drawing,
        Done::Undone,
        &[Target::Action(Action::Undo)],
        Show::Perform(Action::Undo),
    ),
    act(
        "Someone left a smudge on your paper. Pick the {Eraser} and rub it out.",
        Stage::Drawing,
        Done::Erased(SMUDGE),
        &[Target::Rail(Key::Eraser)],
        Show::Trace(&[WIPE]),
    ),
    pill("Deckle is ready. Press {Color}.", "Color is where the picture gets its night.", Stage::InkPaint, Action::RoomFinishing),
    act(
        "Pick {Night} on this card.",
        Stage::InkPaint,
        Done::Picked(rgb(NIGHT_INK)),
        &[Target::Named(lesson::CHIP)],
        Show::Run(pick_night),
    ),
    Task {
        miss: Some("Night got into Deckle. Press {Undo}, then {Show me} closes the gap."),
        ..act(
            "Tap the paper, away from Deckle.",
            Stage::InkPaint,
            Done::Filled { seed: SKY_SEED, keep: Some(BODY_SEED) },
            FILL_KEY,
            Show::Run(close_and_fill),
        )
    },
    act("Pick white.", Stage::InkPaint, Done::Picked(rgb(WHITE_INK)), &[Target::Named("swatch.white")], Show::Run(pick_white)),
    fill("Tap inside Deckle.", Stage::InkPaint, BODY_SEED, SKY_SEED),
    act("Pick red.", Stage::InkPaint, Done::Picked(rgb(RED_INK)), &[Target::Named("swatch.red")], Show::Run(pick_red)),
    fill("Tap the pencil.", Stage::InkPaint, PENCIL_SEED, BODY_SEED),
    act("Pick yellow.", Stage::InkPaint, Done::Picked(rgb(YELLOW_INK)), &[Target::Named("swatch.yellow")], Show::Run(pick_yellow)),
    act("Press {Brush}.", Stage::InkPaint, Done::Tool(CanvasTool::Paint), BRUSH_KEY, Show::Run(arm_brush)),
    trace("Press three stars into the sky.", Stage::InkPaint, &[STAR_1, STAR_2, STAR_3], ON_PAPER),
    pill(
        "Press {Effects}.",
        "Effects is where light gets added, after the picture is finished.",
        Stage::Composite,
        Action::RoomComposite,
    ),
    act("Tap {Glow}.", Stage::Composite, Done::Effect(FxKind::Glow), GLOW_KEY, Show::Run(add_glow)),
    pill("Press {Export}.", "Export turns your project into a file anyone can open.", Stage::Export, Action::RoomExport),
    act(
        "Save your picture.",
        Stage::Export,
        Done::Exported(1),
        &[Target::Action(Action::ExportStill)],
        Show::Perform(Action::ExportStill),
    ),
];

// ---- End of the Desk ----

const EDGE: Template = Template {
    name: "End of the Desk",
    kind: Some(ProjectKind::Animation),
    width: 1024,
    height: 768,
    columns: &[("BG", ColumnKind::Bg), ("A", ColumnKind::Cel)],
    ..templates::READY
};

// Its art: a pose's line and its eye, pose by pose, then the card's row.
const FLAT_1: u16 = 0;
const EYE_1: u16 = 1;
const ARCH_1: u16 = 2;
const EYE_2: u16 = 3;
const FLAT_2: u16 = 4;
const EYE_3: u16 = 5;
const ARCH_2: u16 = 6;
const EYE_4: u16 = 7;
const FLAT_3: u16 = 8;
const EYE_5: u16 = 9;
/// The first of the card's six small pictures.
const ROW: u16 = 10;
const ROW_PATHS: usize = 11;

pub(crate) const DESK_Y: f32 = 520.0;
pub(crate) const DESK_EDGE: f32 = 720.0;
/// Long enough for the hand: on a shorter strip an arch is all bend, and its
/// line stands off any smooth curve the wobble check can lay along it.
pub(crate) const STRIP_LONG: f32 = 360.0;
/// Back foot and front foot: each move plants one. The second arch's front foot
/// stands on the edge.
pub(crate) const FEET: [[f32; 2]; 5] = [[240.0, 600.0], [360.0, 600.0], [360.0, 720.0], [480.0, 720.0], [480.0, 840.0]];
const POSES: usize = FEET.len();
const DECKLE_SEED: u32 = 2;
const PEG_BAR: [f32; 2] = [60.0, 220.0];
const PEG_TOP: f32 = 492.0;
/// The desk's face, the bar and the torn stub left in its first peg.
const DESK_FACE: [u8; 4] = [46, 34, 40, 255];
const DESK_TOP: [u8; 4] = [78, 60, 58, 255];
const PEG_BAR_INK: [u8; 4] = [70, 76, 96, 255];
const STUB_INK: [u8; 4] = [120, 118, 112, 255];
/// The card's row: a pose's share of its size, where the first stands, and how
/// far on the next does. Small enough that a pose is short lines.
const ROW_SCALE: f32 = 0.06;
const ROW_AT: Pt = [150.0, 120.0];
const ROW_STEP: f32 = 60.0;

/// The strip along the desk and over the edge. `spine` arches between two feet;
/// this one is laid on the desk and bends over its end, and stops at the strip's
/// length, so the droop is a strip's and not a stretch.
fn drooped() -> Vec<Pt> {
    let [from, _] = FEET[4];
    let hang = art::curve([from, DESK_Y], &[
        [[from + 40.0, DESK_Y], [DESK_EDGE - 30.0, DESK_Y], [DESK_EDGE, DESK_Y + 1.0]],
        [[DESK_EDGE + 32.0, DESK_Y + 4.0], [DESK_EDGE + 78.0, DESK_Y + 22.0], [DESK_EDGE + 118.0, DESK_Y + 84.0]],
    ]);
    let mut walked = 0.0;
    let mut kept = vec![hang[0]];
    for w in hang.windows(2) {
        let step = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if walked + step > STRIP_LONG {
            let t = (STRIP_LONG - walked) / step;
            kept.push([w[0][0] + (w[1][0] - w[0][0]) * t, w[0][1] + (w[1][1] - w[0][1]) * t]);
            break;
        }
        walked += step;
        kept.push(w[1]);
    }
    kept
}

/// Pose `i`'s spine, on the desk.
pub(crate) fn pose_spine(i: usize) -> Vec<Pt> {
    let [back, front] = FEET[i];
    if front > DESK_EDGE { drooped() } else { art::spine([back, DESK_Y], [front, DESK_Y], STRIP_LONG) }
}

/// Deckle as a Rookie traces it: one closed outline and an eye.
fn closed(spine: &[Pt], seed: u32) -> [Path; 2] {
    let deckle = art::deckle(spine, seed);
    let (width, mut points) = (deckle.cut.width, deckle.cut.points);
    points.extend(deckle.torn.points);
    [Path { points, width, kind: PathKind::Guide, over: false }, deckle.eye]
}

/// Inside pose `i`, where a tap fills it: under the peak's torn side, midway.
pub(crate) fn pose_seed(i: usize) -> [f32; 2] {
    let spine = pose_spine(i);
    let top = spine.iter().copied().fold([0.0, f32::MAX], |a: Pt, b| if b[1] < a[1] { b } else { a });
    let mid = if FEET[i][1] - FEET[i][0] < STRIP_LONG { top } else { spine[spine.len() / 2] };
    [mid[0], mid[1] - STRIP_LONG * 0.1]
}

/// The six pictures, tiny: the desk and each pose in its place. The sixth is the
/// desk alone, which is the joke.
fn card_row() -> Vec<Path> {
    let [left, _] = FEET[0];
    let place = |at: f32| move |p: &[f32; 3]| [at + (p[0] - left) * ROW_SCALE, ROW_AT[1] + (p[1] - DESK_Y) * ROW_SCALE, p[2]];
    let mut row = Vec::with_capacity(ROW_PATHS);
    for i in 0..=POSES {
        let at = ROW_AT[0] + i as f32 * ROW_STEP;
        let desk = [[at, ROW_AT[1]], [at + 46.0, ROW_AT[1]]];
        row.push(art::hand(&desk, 1.0, PathKind::Finish, 20 + i as u32));
        if i < POSES {
            let [line, _] = closed(&pose_spine(i), DECKLE_SEED);
            row.push(Path { points: line.points.iter().map(place(at)).collect(), width: 1.0, kind: PathKind::Finish, over: false });
        }
    }
    row
}

fn edge_art() -> Vec<Path> {
    let mut art: Vec<Path> = (0..POSES).flat_map(|i| closed(&pose_spine(i), DECKLE_SEED)).collect();
    art.iter_mut().for_each(|path| path.over = true);
    art.extend(card_row());
    art
}

/// The desk at night: its top along `DESK_Y` up to its edge, an empty peg bar
/// standing on it, and the torn stub Deckle left in the first peg.
pub(crate) fn desk_texel(x: i32, y: i32) -> [u8; 4] {
    let (px, py) = (x as f32, y as f32);
    let on_bar = (PEG_BAR[0]..PEG_BAR[1]).contains(&px) && (PEG_TOP..DESK_Y).contains(&py);
    let stub = (PEG_BAR[0]..PEG_BAR[0] + 34.0).contains(&px) && (PEG_TOP - 16.0 - (px * 0.7).sin().abs() * 7.0..PEG_TOP).contains(&py);
    if on_bar {
        PEG_BAR_INK
    } else if stub {
        STUB_INK
    } else if px >= DESK_EDGE || py < DESK_Y {
        NIGHT_INK.to_array()
    } else if py < DESK_Y + 12.0 {
        DESK_TOP
    } else {
        DESK_FACE
    }
}

/// The BG track's one plate, under the armed `A`. Its layer has no colour role:
/// `FilledOn` counts Deckle's drawings.
fn desk_prep(s: &mut crate::doc::AppState) {
    // One pass and stop: `Press {Play}` comes twice, and a loop running would
    // turn the second press into a stop. The GIF loops all the same.
    s.view.loop_playback = false;
    let at = s.at();
    let id = s.engine.alloc_drawing_id();
    let layer = s.engine.alloc_layer_id();
    let mut plate = Drawing::new(id, "BG");
    let mut desk = CelLayer::new(layer, "desk");
    desk.raster = crate::sample::raster((0, 0, EDGE.width as i32, EDGE.height as i32), |x, y| Some(desk_texel(x, y)));
    plate.layers.push(desk);
    let cut = s.engine.project.cut_mut(at.scene, at.cut).expect("the lesson's paper");
    cut.drawings.push(plate);
    if let Some(bg) = cut.xsheet.columns.first_mut() {
        bg.set_key(0, Exposure::Drawing(id));
    }
}

/// The track in hand's runs, in order: where each starts and what it shows.
fn runs_here(e: &Editor) -> Vec<(u32, Exposure)> {
    let state = &e.state;
    let Some(column) = state.cut().xsheet.column(state.view.active_column) else { return Vec::new() };
    let mut from = 0;
    let row = |(shows, len): (Exposure, u32)| {
        let start = from;
        from += len;
        (start, shows)
    };
    column.runs(0, state.frame_count()).into_iter().map(row).collect()
}

/// Pose `i`, on its drawing's first row: the cursor may stand on a held one, and
/// Fill refuses there. A click on the strip lands the same way.
fn fill_pose(e: &mut Editor, i: usize) {
    let Some((from, _)) = runs_here(e).get(i).copied() else { return };
    let column = e.state.view.active_column;
    e.state.select_cells((column, from), column, from);
    let seed = pose_seed(i);
    if e.canvas.slot_at_pen_down(true, &mut e.state).is_some() {
        e.canvas.fill_at(pos2(seed[0], seed[1]), &mut e.state);
    }
}

fn set_medium(e: &mut Editor) {
    if at_step(e, Stage::Drawing) {
        e.state.set_speed(2);
    }
}

fn goto_last_arch(e: &mut Editor) {
    if at_step(e, Stage::Edit) {
        e.state.goto(LAST_ARCH);
    }
}

fn longer_three(e: &mut Editor) {
    if at_step(e, Stage::Edit) {
        e.state.set_run_length(LAST_ARCH, LAST_ARCH_LEN);
    }
}

fn fill_here(e: &mut Editor) {
    if at_step(e, Stage::InkPaint) {
        e.canvas.set_tool(CanvasTool::Fill, &mut e.state);
        let frame = e.state.view.frame;
        let here = runs_here(e).iter().rposition(|(from, _)| *from <= frame).unwrap_or(0);
        fill_pose(e, here.min(POSES - 1));
    }
}

fn fill_the_rest(e: &mut Editor) {
    if at_step(e, Stage::InkPaint) {
        e.canvas.set_tool(CanvasTool::Fill, &mut e.state);
        for (i, (_, shows)) in runs_here(e).into_iter().take(POSES).enumerate() {
            let bare = |id| e.state.cut().drawing(id).is_some_and(|d| !lesson::color_layer_art(d));
            if matches!(shows, Exposure::Drawing(id) if bare(id)) {
                fill_pose(e, i);
            }
        }
    }
}

/// The fourth drawing's first row at `Medium`, and its run after `Longer`.
const LAST_ARCH: u32 = 6;
const LAST_ARCH_LEN: u32 = 5;
/// The track the sheet's rows read: `BG` stands left of it.
const TRACK_A: u8 = 1;

const ADD_KEY: &[Target] = &[Target::Action(Action::AddDrawingAfter)];
const PLAY_KEY: &[Target] = &[Target::Action(Action::PlayPause)];

/// `Press {Add drawing}`. The BG track's plate is a drawing of the cut too, so
/// `count` of them, `Done::Drawings`' reading, is one past the person's.
const fn add(say: &'static str, count: u32) -> Task {
    act(say, Stage::Drawing, Done::Drawings(count + 1), ADD_KEY, Show::Perform(Action::AddDrawingAfter))
}

const END_OF_THE_DESK_TASKS: &[Task] = &[
    Task {
        why: Some("Plan is where your drawings land, one row after another."),
        ..trace(
            "Deckle has reached the desk. Draw over the blue line, then press the blue dot for its eye.",
            Stage::Drawing,
            &[FLAT_1, EYE_1],
            BRUSH_KEY,
        )
    },
    add("Press {Add drawing}.", 2),
    trace(
        "Deckle arches. The front stays put and the back end catches up. Draw it over the blue line. The faint one is your last drawing.",
        Stage::Drawing,
        &[ARCH_1, EYE_2],
        ON_PAPER,
    ),
    add("Press {Add drawing}.", 3),
    trace("Flat again. Now the back stays put and the front reaches out.", Stage::Drawing, &[FLAT_2, EYE_3], ON_PAPER),
    add("Press {Add drawing}.", 4),
    trace("Arch again. Deckle has reached the edge.", Stage::Drawing, &[ARCH_2, EYE_4], ON_PAPER),
    add("Press {Add drawing}.", 5),
    trace("One more. Half of Deckle is past the edge.", Stage::Drawing, &[FLAT_3, EYE_5], ON_PAPER),
    add("Press {Add drawing} and leave this one empty. Deckle is gone.", 6),
    act("Press {Play}.", Stage::Drawing, Done::Played, PLAY_KEY, Show::Perform(Action::PlayPause)),
    act(
        "Too fast. Press {Speed} and pick {Medium}.",
        Stage::Drawing,
        Done::Speed(2),
        &[Target::Named("band.fold"), Target::Named("band.speed"), Target::Named("speed.medium")],
        Show::Run(set_medium),
    ),
    pill("Press {Timing}.", "Timing is where you choose how long each picture stays.", Stage::Edit, Action::RoomEdit),
    act(
        "Click the last arch on the sheet.",
        Stage::Edit,
        Done::Frame(LAST_ARCH, LAST_ARCH + 1),
        &[Target::Cell { track: TRACK_A, frame: LAST_ARCH }],
        Show::Run(goto_last_arch),
    ),
    act(
        "Press {Longer} three times. Deckle stops at the edge and looks down.",
        Stage::Edit,
        Done::Hold { frame: LAST_ARCH, len: LAST_ARCH_LEN },
        &[Target::Named("band.longer")],
        Show::Run(longer_three),
    ),
    act("Press {Play}. The wait is the joke.", Stage::Edit, Done::Played, PLAY_KEY, Show::Perform(Action::PlayPause)),
    pill("Press {Color}.", "Color is where each picture gets filled. Your lines stay safe.", Stage::InkPaint, Action::RoomFinishing),
    act("Tap inside Deckle.", Stage::InkPaint, Done::FilledOn(1), FILL_KEY, Show::Run(fill_here)),
    act(
        "Click each picture along the bottom, and tap inside Deckle.",
        Stage::InkPaint,
        Done::FilledOn(5),
        &[Target::Named("strip.next")],
        Show::Run(fill_the_rest),
    ),
    pill("Press {Effects}.", "Effects is where the chalk starts to shine.", Stage::Composite, Action::RoomComposite),
    act("Tap {Glow}.", Stage::Composite, Done::Effect(FxKind::Glow), GLOW_KEY, Show::Run(add_glow)),
    pill("Press {Export}.", "Export turns your drawings into a file anyone can watch.", Stage::Export, Action::RoomExport),
    act(
        "Save your animation.",
        Stage::Export,
        Done::Exported(1),
        &[Target::Action(Action::ExportGif)],
        Show::Perform(Action::ExportGif),
    ),
];

// ---- Not a Pencil ----

fn page_small() -> Page {
    Page::print("B5", [182.0, 257.0], 3.0, [150.0, 220.0], 120.0)
}

const PAGE_SMALL: Template = Template {
    name: "Not a Pencil",
    kind: Some(ProjectKind::Comic),
    width: 888,
    height: 1243,
    fps: 24,
    room: Stage::Manga,
    columns: &[("INK", ColumnKind::Cel), ("TONE", ColumnKind::Cel)],
    page: Some(page_small),
    ..templates::READY
};

// Its art, panel by panel.
const P1_DESK: u16 = 0;
const P1_DECKLE: u16 = 1;
const P1_EYE: u16 = 2;
const P2_DECKLE: u16 = 3;
const P2_EYE: u16 = 4;
const P2_SHADOW: u16 = 5;
const P3_CUP: u16 = 6;
const P3_PENCILS: u16 = 7;
const P3_DECKLE: u16 = 8;
const P4_EDGES: u16 = 9;
const P4_EYE: u16 = 10;

pub(crate) const SHADOW_SEED: [f32; 2] = [625.0, 210.0];
pub(crate) const P2_PAPER: [f32; 2] = [478.0, 300.0];
/// The first panel's strip, asleep; the second's is the same strip, where the
/// split moved the panel to.
const ASLEEP: [Pt; 2] = [[158.0, 484.0], [376.0, 484.0]];
const NEXT_PANEL: f32 = 359.0;
/// How the page's strip stands up in the third panel.
const UPRIGHT: [Pt; 2] = [[304.0, 930.0], [304.0, 712.0]];
const PAGE_LINE: f32 = 4.0;
const SIDE_EYE: f32 = 40.0;

fn not_a_pencil_art() -> Vec<Path> {
    use art::{dab, hand};
    const LONG: f32 = 218.0;
    const SEED: u32 = 7;
    let [line, eye] = closed(&art::spine(ASLEEP[0], ASLEEP[1], LONG), SEED);
    let moved = [[ASLEEP[0][0] + NEXT_PANEL, ASLEEP[0][1]], [ASLEEP[1][0] + NEXT_PANEL, ASLEEP[1][1]]];
    let [line_2, eye_2] = closed(&art::spine(moved[0], moved[1], LONG), SEED);
    let [line_3, _] = closed(&art::spine(UPRIGHT[0], UPRIGHT[1], LONG), SEED);
    let desk = [[106.0, 488.0], [262.0, 487.0], [424.0, 490.0]];
    // A hand's shadow, three claws coming down out of the top of the panel.
    let shadow = [
        [548.0, 118.0], [576.0, 348.0], [600.0, 262.0], [630.0, 376.0], [654.0, 262.0],
        [686.0, 342.0], [716.0, 118.0], [540.0, 117.0],
    ];
    let cup = [[170.0, 931.0], [350.0, 929.0], [330.0, 1098.0], [260.0, 1104.0], [192.0, 1099.0], [168.0, 926.0]];
    // Each with the small step where its paint stops and its point starts.
    let pencils = [
        [186.0, 930.0], [188.0, 700.0], [195.0, 699.0], [201.0, 668.0], [207.0, 699.0], [214.0, 702.0], [215.0, 930.0],
        [222.0, 929.0], [224.0, 740.0], [231.0, 739.0], [236.0, 708.0], [243.0, 739.0], [250.0, 742.0], [250.0, 931.0],
        [316.0, 930.0], [318.0, 690.0], [325.0, 689.0], [330.0, 660.0], [337.0, 689.0], [344.0, 692.0], [344.0, 930.0],
    ];
    let edges = [[542.0, 652.0], [536.0, 1122.0], [714.0, 1125.0], [718.0, 650.0]];
    let seeded = |points: &[Pt], seed| hand(points, PAGE_LINE, PathKind::Guide, seed);
    vec![
        seeded(&desk, 30),
        line,
        eye,
        line_2,
        eye_2,
        seeded(&shadow, 57),
        seeded(&cup, 32),
        seeded(&pencils, 35),
        line_3,
        seeded(&edges, 34),
        dab([664.0, 860.0], SIDE_EYE, PathKind::Guide),
    ]
}

fn split_across(e: &mut Editor) {
    if at_step(e, Stage::Manga) {
        manga_tools::panels(&mut e.state, PanelOp::SplitAcross);
    }
}

fn split_down(e: &mut Editor) {
    if at_step(e, Stage::Manga) {
        manga_tools::panels(&mut e.state, PanelOp::SplitDown);
    }
}

fn pick_panel_3(e: &mut Editor) {
    if at_step(e, Stage::Manga) {
        e.state.manga.selected = 2;
    }
}

const NOT_A_PENCIL_TASKS: &[Task] = &[
    Task {
        why: Some("Manga is where a page is split into boxes, and the reader's eye goes box to box."),
        ..act(
            "Four panels, no words. Press {Split across}.",
            Stage::Manga,
            Done::Panels(2),
            &[Target::Pane(Pane::Manga), Target::Named("manga.split_across")],
            Show::Run(split_across),
        )
    },
    act("Press {Split down}.", Stage::Manga, Done::Panels(3), &[Target::Named("manga.split_down")], Show::Run(split_down)),
    act("Click {panel 3}.", Stage::Manga, Done::PanelPicked(2), &[Target::Named("manga.panel.3")], Show::Run(pick_panel_3)),
    act("Press {Split down} again. Four panels.", Stage::Manga, Done::Panels(4), &[Target::Named("manga.split_down")], Show::Run(split_down)),
    trace("Panel 1. Deckle is asleep on the desk. Draw over the blue lines.", Stage::Manga, &[P1_DESK, P1_DECKLE, P1_EYE], BRUSH_KEY),
    trace("Panel 2. Something is coming. Draw over the blue lines.", Stage::Manga, &[P2_DECKLE, P2_EYE, P2_SHADOW], ON_PAPER),
    trace("Panel 3. Deckle hides among the pencils. Stand very still.", Stage::Manga, &[P3_CUP, P3_PENCILS, P3_DECKLE], ON_PAPER),
    trace("Panel 4. Close up. One eye looks sideways.", Stage::Manga, &[P4_EDGES, P4_EYE], ON_PAPER),
    pill("Press {Color}.", "One black is enough. It is the heaviest thing on a page.", Stage::InkPaint, Action::RoomFinishing),
    act("Pick black.", Stage::InkPaint, Done::Picked(rgb(BLACK_INK)), &[Target::Named("swatch.black")], Show::Run(pick_black)),
    fill("Tap the shadow in panel 2.", Stage::InkPaint, SHADOW_SEED, P2_PAPER),
    pill("Press {Export}.", "Export turns your page into a file you can print.", Stage::Export, Action::RoomExport),
    act(
        "Press {Export pages}.",
        Stage::Export,
        Done::Exported(1),
        &[Target::Action(Action::ExportPages)],
        Show::Perform(Action::ExportPages),
    ),
];

/// Every lesson that ships.
pub const ALL: &[Lesson] = &[
    Lesson {
        id: "up_late",
        kind: ProjectKind::Drawing,
        class: Class::Rookie,
        title: "Up Late",
        hover: "Deckle, a strip of paper that got away, stands on a pencil under the stars. About 9 minutes. Ends in a PNG.",
        paper: &UP_LATE,
        prep: Some(up_late_prep),
        fill_gap: Some(LESSON_GAP),
        ink: None,
        latch: None,
        art: up_late_art,
        finish: &[BACK, BELLY, HOLE_1, HOLE_2, HOLE_3, PENCIL, RIM_1, RIM_2, RIM_3],
        tasks: UP_LATE_TASKS,
        closing: "You made that. It is a real file. You can draw, undo, erase, fill a whole sky and add light.",
    },
    Lesson {
        id: "end_of_the_desk",
        kind: ProjectKind::Animation,
        class: Class::Rookie,
        title: "End of the Desk",
        hover: "One drawing after another, Deckle walks to the edge of the desk and stops. About 12 minutes. Ends in a GIF.",
        paper: &EDGE,
        prep: Some(desk_prep),
        fill_gap: Some(LESSON_GAP),
        ink: Some([255, 255, 255, 255]),
        latch: None,
        art: edge_art,
        finish: &{
            let mut row = [ROW; ROW_PATHS];
            let mut i = 0;
            while i < ROW_PATHS {
                row[i] = ROW + i as u16;
                i += 1;
            }
            row
        },
        tasks: END_OF_THE_DESK_TASKS,
        closing: "You made that. It is a real file. You can add drawings, play them, set their speed and choose how long each one stays.",
    },
    Lesson {
        id: "not_a_pencil",
        kind: ProjectKind::Comic,
        class: Class::Rookie,
        title: "Not a Pencil",
        hover: "Four panels and no words: something is coming, and Deckle hides among the pencils. About 15 minutes. Ends in a page PNG.",
        paper: &PAGE_SMALL,
        prep: None,
        fill_gap: Some(LESSON_GAP),
        ink: None,
        latch: None,
        art: not_a_pencil_art,
        finish: &[P1_DESK, P1_DECKLE, P1_EYE, P2_DECKLE, P2_EYE, P2_SHADOW, P3_CUP, P3_PENCILS, P3_DECKLE, P4_EDGES, P4_EYE],
        tasks: NOT_A_PENCIL_TASKS,
        closing: "You made that. It is a real file. You can split a page into panels and make a reader wait between them.",
    },
];

/// A Rookie's lines are shaky, and a gap this wide still holds a fill.
const LESSON_GAP: u32 = 8;

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
