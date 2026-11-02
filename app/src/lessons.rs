//! AU20: the lesson table, as the Lessons page reads it (spec 4.5). AU27: a
//! lesson's paper and tasks, the types the frame in `lesson.rs` reads (spec 5.3).
//! Lesson papers are `Template` consts here, outside `ALL` and `INTENTS`. AU23:
//! the three Rookie lessons, as `research/LESSONS-approachable-ui.md` gives them.

// The vocabulary is whole now; the lessons that use all of it come with AU28.
#![allow(dead_code)]

use anim_core::camera::IRect;
use anim_core::fill::{FillOpts, flood_fill_mask_in};
use anim_core::fx::{FxKind, FxValue};
use anim_core::generator::GenKind;
use anim_core::graph::{Node, NodeKind};
use anim_core::manga::{Page, Rulers};
use anim_core::model::{CelLayer, Drawing, DrawingKind, LayerRole};
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

    /// A short pull of the pen, `long` px: an eye small enough that a press would
    /// not make a stroke of it.
    pub fn dash(at: Pt, long: f32, width: f32, kind: PathKind) -> Path {
        const ROWS: usize = 6;
        let point = |i: usize| {
            let t = i as f32 / ROWS as f32;
            [at[0] + long * 0.85 * t, at[1] + long * 0.5 * t, 0.9 + 0.1 * t]
        };
        Path { points: (0..=ROWS).map(point).collect(), width, kind, over: false }
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
        strip(spine, seed, false)
    }

    /// Deckle stood up on his two feet, the one picture of him: the width turns
    /// with the bend, so an arch this tall comes down to the pencil as wide as it
    /// went up. Moved one way, its legs would close to nothing where they stand.
    pub fn upright(spine: &[Pt], seed: u32) -> Deckle {
        strip(spine, seed, true)
    }

    fn strip(spine: &[Pt], seed: u32, turning: bool) -> Deckle {
        let long = length(spine);
        let wide = long * STRIP_WIDE;
        let spine = spaced(spine, HAND_STEP);
        let last = spine.len() - 1;
        // Up off the paper, whichever way the strip is headed.
        let chord = [spine[last][0] - spine[0][0], spine[last][1] - spine[0][1]];
        let reach = chord[0].hypot(chord[1]).max(1e-3);
        let up = if chord[0] < 0.0 { -1.0 } else { 1.0 };
        let moved = |i: usize, by: f32| {
            let across = if turning { side(&spine, i) } else { [chord[1] / reach, -chord[0] / reach] };
            [spine[i][0] + across[0] * up * by, spine[i][1] + across[1] * up * by]
        };
        let ribbon: Vec<Pt> = (0..=last).map(|i| moved(i, wide)).collect();
        // A turning strip's torn side runs longer than its spine: sampled by its
        // own length, a nick keeps a nick's width and stays a corner.
        let (ribbon, along) = if turning {
            let walked = spaced(&ribbon, HAND_STEP);
            let along = length(&walked);
            (walked, along)
        } else {
            (ribbon, long)
        };
        let tear = tear(along, seed);
        let back = pushed(&ribbon, |i| up * tear[i.min(tear.len() - 1)]);
        let line = wide * LINE_WIDE;
        // On the strip: `[along it, tail to head; across it, cut side to torn]`.
        let stood = |[along, over]: [f32; 2]| moved((along * last as f32).round() as usize, wide * over);
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

    /// AU24: a ruffle, one size at any reach: how long and how high, the samples
    /// of each, and how far round a reach of edge goes. Short and steep: each is
    /// a stroke of its own, as a pen ruffles.
    const RUFFLE_LONG: f32 = 36.0;
    const RUFFLE_RISE: f32 = 16.0;
    const RUFFLE_ROWS: usize = 14;
    const EDGE_ROUND: f32 = 5.0;
    /// How much smaller the seed can make one ruffle.
    const RUFFLE_SPREAD: f32 = 0.15;
    /// The long side stays plain this far either way: the one long pull of the
    /// edge, of the way round.
    const PLAIN_SIDE: f32 = 0.125;
    /// It sits this far in from the ruffles' feet: the pen stops and turns there.
    const PLAIN_STEP: f32 = 8.0;
    /// The curl's bend and end: of the way round and of the way out. It turns
    /// back under the edge at a corner, and stays wide: a hand's long curve.
    const CURL_BEND: [f32; 2] = [-0.16, 0.62];
    const CURL_END: [f32; 2] = [-0.4, 0.28];
    const CURL_ROWS: usize = 64;
    /// A stroke that is the edge too ends in a flick, back under the last ruffle
    /// at a corner: how long, and how far in of that.
    const FLICK_LONG: f32 = 36.0;
    const FLICK_IN: f32 = 0.3;
    /// A shaving lies flatter than it is long.
    pub const FLAT: f32 = 0.76;

    /// AU24: a pencil shaving, by a seed: a ruffled edge round `at`, in the same
    /// hand as Deckle's. `wide` and `tall` squeeze it and keep it the same wood.
    #[derive(Clone, Copy)]
    pub struct Shaving {
        pub seed: u32,
        pub at: Pt,
        pub reach: f32,
        pub wide: f32,
        pub tall: f32,
    }

    /// A shaving lying flat at `at`.
    pub fn shaving(seed: u32, at: Pt, reach: f32) -> Shaving {
        Shaving { seed, at, reach, wide: 1.0, tall: FLAT }
    }

    impl Shaving {
        fn ruffles(self) -> u32 {
            (self.reach * EDGE_ROUND / RUFFLE_LONG).round() as u32 + self.seed % 3
        }

        /// How far out the edge stands at `lobe`'s foot: `rise` of the way up it.
        fn out(self, lobe: i32, rise: f32) -> f32 {
            let side = if self.wide >= self.tall { 0.25 } else { 0.5 };
            let plain = ((lobe as f32 + 0.5) / self.ruffles() as f32 - side).abs() < PLAIN_SIDE;
            let big = 1.0 - RUFFLE_SPREAD * (lattice(self.seed, 8 + lobe) + 1.0) * 0.5;
            self.reach - RUFFLE_RISE + if plain { -PLAIN_STEP } else { RUFFLE_RISE * rise * big }
        }

        /// The turn of the long side: no two lie the same way.
        fn lean(self) -> f32 {
            lattice(self.seed, 1) * std::f32::consts::PI * 0.5
        }

        /// `from_middle` of the way to the edge, `t` of the way round.
        pub fn point(self, t: f32, from_middle: f32) -> Pt {
            let round = self.ruffles() as f32;
            let lobe = (t.rem_euclid(1.0) * round).floor().min(round - 1.0) as i32;
            let rise = (t * round * std::f32::consts::PI).sin().abs().sqrt();
            self.placed(t, self.out(lobe, rise) * from_middle)
        }

        fn placed(self, t: f32, far: f32) -> Pt {
            let (sin, cos) = (t * std::f32::consts::TAU).sin_cos();
            let (x, y) = (cos * far * self.wide, sin * far * self.tall);
            let (lean_sin, lean_cos) = self.lean().sin_cos();
            [self.at[0] + x * lean_cos - y * lean_sin, self.at[1] + x * lean_sin + y * lean_cos]
        }

        /// Its line, `from_middle` of the way out, round and back to the start.
        pub fn edge(self, from_middle: f32) -> Vec<Pt> {
            let rows = self.ruffles() as usize * RUFFLE_ROWS;
            // Each row twice, a hair apart: the step on and off the plain side
            // goes straight out, a corner, where one row would lean it over.
            let row = |i: usize| {
                let t = i as f32 / rows as f32;
                [self.point(t - 1e-5, from_middle), self.point(t, from_middle)]
            };
            (0..=rows).flat_map(row).collect()
        }

        /// The curl alone, running in from the foot where the edge shuts.
        pub fn swirl(self) -> Vec<Pt> {
            let foot = self.placed(0.0, self.out(0, 0.0));
            let bend = self.placed(CURL_BEND[0], self.reach * CURL_BEND[1]);
            let end = self.placed(CURL_END[0], self.reach * CURL_END[1]);
            let at = |c: usize, u: f32| foot[c] * (1.0 - u) * (1.0 - u) + bend[c] * 2.0 * u * (1.0 - u) + end[c] * u * u;
            let turn = |i: usize| {
                let u = i as f32 / CURL_ROWS as f32;
                [at(0, u), at(1, u)]
            };
            (0..=CURL_ROWS).map(turn).collect()
        }

        /// The edge, then its flick: one stroke. It ends in the wood, so one tap
        /// still fills the whole shaving.
        pub fn curl(self) -> Vec<Pt> {
            let mut line = self.edge(1.0);
            let foot = self.out(0, 0.0);
            let back = FLICK_LONG / (std::f32::consts::TAU * foot);
            line.push(self.placed(-back, foot - FLICK_LONG * FLICK_IN));
            line
        }

        /// A spot of bare wood: as far from `line` as the middle of the shaving
        /// goes, among those `keep` allows. A tap here cannot land on the ink.
        pub fn bare(self, line: &Path, keep: impl Fn(Pt) -> bool) -> Pt {
            const SPOKES: usize = 48;
            const RINGS: usize = 6;
            let clear = |q: Pt| line.points.iter().fold(f32::MAX, |d, p| d.min((p[0] - q[0]).hypot(p[1] - q[1])));
            let spots = (1..=RINGS).flat_map(|r| (0..SPOKES).map(move |s| (r, s)));
            let spot = |(r, s): (usize, usize)| self.point(s as f32 / SPOKES as f32, 0.7 * r as f32 / RINGS as f32);
            spots.map(spot).filter(|q| keep(*q)).fold(self.at, |best, q| if clear(q) > clear(best) { q } else { best })
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
    /// AU24: what the gate does for a person's hand where no path can say it: a
    /// right-click, a typed cell, a page's cuts, three drawings' two fills.
    pub hand: Option<fn(&mut Editor)>,
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
    /// AU23: what the Lessons page's row shows, where `finish` cannot read at
    /// that height. Empty: `finish`.
    pub thumb: &'static [u16],
    /// AU24: what the card's picture shows, where it is less than the whole
    /// finish `Hold to see it` gives. Empty: `finish`.
    pub card: &'static [u16],
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
        thumb: &[],
        card: &[],
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
const BROWN_INK: Color32 = swatch(9);
const NIGHT_INK: Color32 = plate::LESSON_NIGHT;

/// A task with no line of its own.
const fn act(say: &'static str, step: Stage, done: Done, ring: &'static [Target], show: Show) -> Task {
    Task { say, why: None, step, done, ring, guide: &[], stand_in: &[], hand: None, miss: None, show }
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
/// The strip, tail foot to front foot: the back foot stands on the pencil, and
/// the front is lifted just clear of it, past the lesson's gap, so a night
/// poured over the sky reaches under the arch. Its peak is the torn side's.
const ARCH: [Pt; 2] = [[420.0, 785.0], [700.0, 774.0]];
const ARCH_LONG: f32 = 500.0;
/// `Strength`, where `glow_down` leaves it. The shelf's first Glow triples what
/// it reads, which burns a yellow to white and a line beside white to nothing.
const GLOW_DOWN: f32 = 0.18;
const SMUDGE_GREY: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
/// `size_up`'s share: a little bigger.
const SIZE_UP: f32 = 1.6;

fn up_late_art() -> Vec<Path> {
    use art::{dab, hand};
    let spine = arch(ARCH[0], ARCH[1], ARCH_LONG);
    let deckle = art::upright(&spine, 1);
    let hole = |path: &Path| art::dab([path.points[0][0], path.points[0][1]], HOLE_WIDE, PathKind::Guide);
    let [hole_2, hole_3] = [hole(&deckle.holes[0]), hole(&deckle.holes[1])];
    let width = deckle.cut.width;
    let [outline, _] = joined(art::upright(&spine, 1));
    // One closed stroke: its end runs back over its start. The point is blunt, a
    // shade wider than the lesson's gap, so a fill reaches the end of it.
    let pencil = [[184.0, 802.0], [860.0, 801.0], [1012.0, 820.0], [1016.0, 840.0], [862.0, 860.0], [182.0, 858.0], [183.0, 800.0], [240.0, 802.0]];
    let scribble = [[900.0, 960.0], [1082.0, 985.0], [905.0, 1022.0], [1076.0, 1050.0], [912.0, 1086.0], [1068.0, 1112.0]];
    let smudge = [[176.0, 1006.0], [286.0, 992.0], [182.0, 1038.0], [282.0, 1030.0], [190.0, 1066.0], [270.0, 1062.0]];
    let wipe = [[120.0, 985.0], [340.0, 978.0], [120.0, 1030.0], [340.0, 1026.0], [120.0, 1078.0], [340.0, 1074.0]];
    let star = |at: Pt| dab(at, STAR_WIDE, PathKind::Guide);
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

const UP_LATE_PAPER: IRect = IRect { x: 0, y: 0, w: UP_LATE.width, h: UP_LATE.height };

/// Whether a night poured at the sky would reach each of `points`: the lines
/// leak there, or are gone.
fn night_reaches(e: &Editor, points: [[f32; 2]; 2]) -> [bool; 2] {
    let opts = FillOpts { threshold: 0.1, gap_px: e.canvas.fill_gap, grow_px: 0, protect_ink: false };
    let seed = (SKY_SEED[0] as i32, SKY_SEED[1] as i32);
    let lines = e.state.display_cel_flatten().unwrap_or_default();
    let mask = flood_fill_mask_in(&lines, seed, UP_LATE_PAPER, &opts, None);
    points.map(|p| mask.as_ref().is_ok_and(|mask| mask.get(p[0] as i32, p[1] as i32)))
}

/// A paper task's `Show me` that runs: the pen's own gates, the card's reach.
fn on_the_paper(e: &mut Editor) -> bool {
    at_step(e, Stage::InkPaint)
        && crate::visible_panes(&e.dock).contains(&Pane::Canvas)
        && e.canvas.slot_at_pen_down(true, &mut e.state).is_some()
}

/// The night's `Show me`: the fill, and first the line that lets it in. That is
/// the closed outline where Deckle leaks, and the pencil where a second `Undo`
/// took it. A person's fill arms the Fill key on the way: this does too, or
/// `Press {Brush}` would stand answered before it was asked.
fn close_and_fill(e: &mut Editor) {
    if !on_the_paper(e) {
        return;
    }
    e.canvas.set_tool(CanvasTool::Fill, &mut e.state);
    let [body, pencil] = night_reaches(e, [BODY_SEED, PENCIL_SEED]);
    let gone = [(body, OUTLINE), (pencil, PENCIL)].into_iter().filter(|(leaks, _)| *leaks).map(|(_, line)| line);
    let gone: Vec<u16> = gone.collect();
    if gone.is_empty() || e.lesson_land(&gone) {
        e.canvas.fill_at(pos2(SKY_SEED[0], SKY_SEED[1]), &mut e.state);
    }
}

/// `Tap the pencil`'s `Show me`. A person's night poured with the pencil's line
/// gone stands where the pencil goes, and a fill flows into empty paper only: the
/// line lands again, and the night inside it is refilled.
fn fill_pencil(e: &mut Editor) {
    if !on_the_paper(e) {
        return;
    }
    let (seed, fills) = (pos2(PENCIL_SEED[0], PENCIL_SEED[1]), e.state.fills_done);
    e.canvas.fill_at(seed, &mut e.state);
    if e.state.fills_done == fills && e.lesson_land(&[PENCIL]) {
        let chip = e.canvas.brush_color;
        e.state.refill_at((seed.x as i32, seed.y as i32), chip, false, 0, UP_LATE_PAPER, None);
    }
}

/// `Strength`, down by the shelf's own road: one undo, on the cut's Glow.
fn glow_down(e: &mut Editor) {
    if !at_step(e, Stage::Composite) {
        return;
    }
    let glow = |n: &Node| matches!(n.kind, NodeKind::Effect { fx: FxKind::Glow, .. }).then(|| (n.id, n.kind.clone()));
    let Some((id, mut kind)) = e.state.cut().graph.nodes().find_map(glow) else { return };
    if let NodeKind::Effect { params, .. } = &mut kind {
        params.set("intensity", FxValue::Float(GLOW_DOWN));
    }
    e.state.graph_set_kind(id, kind);
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
    act(
        "Tap the pencil.",
        Stage::InkPaint,
        Done::Filled { seed: PENCIL_SEED, keep: Some(BODY_SEED) },
        FILL_KEY,
        Show::Run(fill_pencil),
    ),
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
    act(
        "Too bright. Drag {Strength} down until the stars are yellow again.",
        Stage::Composite,
        Done::Strength,
        &[Target::Named("shelf.strength")],
        Show::Run(glow_down),
    ),
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
/// The first of the card's six small pictures, and the page's one after them.
const ROW: u16 = 10;
const ROW_PATHS: usize = 16;
const THUMB: u16 = ROW + ROW_PATHS as u16;
/// The arch at the edge, where the wait is.
const THUMB_POSE: usize = 3;

pub(crate) const DESK_Y: f32 = 520.0;
pub(crate) const DESK_EDGE: f32 = 720.0;
/// Long enough for the hand: on a shorter strip an arch is all bend, and its
/// line stands off any smooth curve the wobble check can lay along it.
pub(crate) const STRIP_LONG: f32 = 360.0;
/// Back foot and front foot: each move plants one. The second arch's front foot
/// stands on the edge, and the last back foot about half a strip short of it. An
/// arch's chord stays 190: at 180 it is all bend, as a shorter strip's is.
pub(crate) const FEET: [[f32; 2]; 5] = [[190.0, 550.0], [360.0, 550.0], [360.0, 720.0], [530.0, 720.0], [530.0, 890.0]];
const POSES: usize = FEET.len();
const DECKLE_SEED: u32 = 30;
/// The bar ends short of the first pose's back foot.
pub(crate) const PEG_BAR: [f32; 2] = [20.0, 160.0];
const PEG_TOP: f32 = 492.0;
/// Chalk, a light grey: white fill leaves its eye dark, and Glow's first
/// strength lifts it to white on a line and no further than grey on an eye.
const CHALK_INK: [u8; 4] = [100, 100, 112, 255];
/// The desk's face, the bar and the torn stub left in its first peg.
const DESK_FACE: [u8; 4] = [46, 34, 40, 255];
const DESK_TOP: [u8; 4] = [78, 60, 58, 255];
const PEG_BAR_INK: [u8; 4] = [70, 76, 96, 255];
const STUB_INK: [u8; 4] = [120, 118, 112, 255];
/// The card's six: a pose's share of its size, so the desk is a cell's width,
/// where the first stands, how far on the next does, and three to a row.
const ROW_SCALE: f32 = 0.085;
const ROW_AT: Pt = [150.0, 120.0];
const ROW_STEP: Pt = [66.0, 44.0];
const ROW_ACROSS: usize = 3;
/// The page's small picture: one line and a pen wide enough to hold there.
const THUMB_WIDE: f32 = 12.0;

/// The strip along the desk and over the edge. `spine` arches between two feet;
/// this one is laid on the desk and bends over its end, and stops at the strip's
/// length, so the droop is a strip's and not a stretch.
fn drooped() -> Vec<Pt> {
    let [from, _] = FEET[4];
    let hang = art::curve([from, DESK_Y], &[
        [[from + 40.0, DESK_Y], [DESK_EDGE - 30.0, DESK_Y], [DESK_EDGE, DESK_Y + 1.0]],
        [[DESK_EDGE + 32.0, DESK_Y + 4.0], [DESK_EDGE + 84.0, DESK_Y + 26.0], [DESK_EDGE + 130.0, DESK_Y + 140.0]],
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
fn joined(deckle: art::Deckle) -> [Path; 2] {
    let (width, mut points) = (deckle.cut.width, deckle.cut.points);
    points.extend(deckle.torn.points);
    [Path { points, width, kind: PathKind::Guide, over: false }, deckle.eye]
}

fn closed(spine: &[Pt], seed: u32) -> [Path; 2] {
    joined(art::deckle(spine, seed))
}

/// Pose `i` of the Desk: stood up, as at Up Late, or an arch's torn side closes
/// on its cut side down both legs.
fn pose(i: usize) -> [Path; 2] {
    joined(art::upright(&pose_spine(i), DECKLE_SEED))
}


/// Inside pose `i`, where a tap fills it: under the peak's torn side, midway.
pub(crate) fn pose_seed(i: usize) -> [f32; 2] {
    let spine = pose_spine(i);
    let top = spine.iter().copied().fold([0.0, f32::MAX], |a: Pt, b| if b[1] < a[1] { b } else { a });
    let mid = if FEET[i][1] - FEET[i][0] < STRIP_LONG { top } else { spine[spine.len() / 2] };
    [mid[0], mid[1] - STRIP_LONG * 0.1]
}

/// The six pictures, tiny: the desk and each pose in its place, three over
/// three. In one row a card's width makes each a dash. The sixth is the desk
/// alone, which is the joke.
fn card_row() -> Vec<Path> {
    let [left, _] = FEET[0];
    let mut row = Vec::with_capacity(ROW_PATHS);
    for i in 0..=POSES {
        let at = ROW_AT[0] + (i % ROW_ACROSS) as f32 * ROW_STEP[0];
        let y = ROW_AT[1] + (i / ROW_ACROSS) as f32 * ROW_STEP[1];
        let place = |p: &[f32; 3]| [at + (p[0] - left) * ROW_SCALE, y + (p[1] - DESK_Y) * ROW_SCALE, p[2]];
        let desk = [[at, y], [at + (DESK_EDGE - left) * ROW_SCALE, y]];
        row.push(art::hand(&desk, 1.0, PathKind::Finish, 20 + i as u32));
        if i < POSES {
            // Two halves: at this size one outline is a long line with no room
            // for a hand in it.
            let [line, _] = pose(i);
            let points: Vec<[f32; 3]> = line.points.iter().map(place).collect();
            let half = points.len() / 2;
            for part in [&points[..=half], &points[half..]] {
                row.push(Path { points: part.to_vec(), width: 1.0, kind: PathKind::Finish, over: false });
            }
        }
    }
    row
}

/// The page's picture: six poses cannot read at its height, so the last arch and
/// the desk's end stand for them.
fn card_thumb() -> [Path; 2] {
    let [line, _] = pose(THUMB_POSE);
    let [left, _] = FEET[THUMB_POSE];
    let desk = [[left - 40.0, DESK_Y], [DESK_EDGE, DESK_Y + 1.0], [DESK_EDGE - 2.0, DESK_Y + 90.0]];
    [Path { width: THUMB_WIDE, kind: PathKind::Finish, ..line }, art::hand(&desk, THUMB_WIDE, PathKind::Finish, 26)]
}

fn edge_art() -> Vec<Path> {
    let mut art: Vec<Path> = (0..POSES).flat_map(pose).collect();
    art.iter_mut().for_each(|path| path.over = true);
    art.extend(card_row());
    art.extend(card_thumb());
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
    bg_plate(s, EDGE.width, EDGE.height, desk_texel);
}

/// `texel` as the BG track's one plate, held for the shot.
fn bg_plate(s: &mut crate::doc::AppState, width: u32, height: u32, texel: fn(i32, i32) -> [u8; 4]) {
    // One pass and stop: `Press {Play}` comes twice, and a loop running would
    // turn the second press into a stop. The GIF loops all the same.
    s.view.loop_playback = false;
    let at = s.at();
    let id = s.engine.alloc_drawing_id();
    let layer = s.engine.alloc_layer_id();
    let mut plate = Drawing::new(id, "BG");
    let mut desk = CelLayer::new(layer, "desk");
    desk.raster = crate::sample::raster((0, 0, width as i32, height as i32), |x, y| Some(texel(x, y)));
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
pub(crate) const TRACK_A: u8 = 1;

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
    act("Pick white.", Stage::InkPaint, Done::Picked(rgb(WHITE_INK)), &[Target::Named("swatch.white")], Show::Run(pick_white)),
    act(
        "Click the first picture along the bottom, and tap inside Deckle.",
        Stage::InkPaint,
        Done::FilledOn(1),
        &[Target::Named("strip.next")],
        Show::Run(fill_here),
    ),
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
    // Each ends blunt, a shade wider than the lesson's gap, so the fill reaches it.
    let shadow = [
        [548.0, 104.0], [568.0, 348.0], [584.0, 350.0], [600.0, 262.0], [622.0, 376.0], [638.0, 377.0], [654.0, 262.0],
        [678.0, 342.0], [694.0, 343.0], [716.0, 104.0], [540.0, 103.0],
    ];
    let cup = [[170.0, 931.0], [350.0, 929.0], [330.0, 1098.0], [260.0, 1104.0], [192.0, 1099.0], [168.0, 926.0]];
    // Each with the small step where its paint stops and its point starts.
    let pencils = [
        [186.0, 930.0], [188.0, 700.0], [195.0, 699.0], [201.0, 668.0], [207.0, 699.0], [214.0, 702.0], [215.0, 930.0],
        [222.0, 929.0], [224.0, 740.0], [231.0, 739.0], [236.0, 708.0], [243.0, 739.0], [250.0, 742.0], [250.0, 931.0],
        [316.0, 930.0], [318.0, 690.0], [325.0, 689.0], [330.0, 660.0], [337.0, 689.0], [344.0, 692.0], [344.0, 930.0],
    ];
    // So close he overruns the panel: his sides leave by its top, and he stands
    // on its bottom line, with the eye hard against his far edge.
    let edges = [[542.0, 637.0], [536.0, 1138.0], [714.0, 1138.0], [718.0, 637.0]];
    let seeded = |points: &[Pt], seed| hand(points, PAGE_LINE, PathKind::Guide, seed);
    vec![
        seeded(&desk, 30),
        line,
        eye,
        line_2,
        eye_2,
        seeded(&shadow, 58),
        seeded(&cup, 32),
        seeded(&pencils, 35),
        line_3,
        seeded(&edges, 34),
        dab([692.0, 860.0], SIDE_EYE, PathKind::Guide),
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

// ---- A Coat of Shavings ----

const COAT: Template = Template {
    name: "A Coat of Shavings",
    kind: Some(ProjectKind::Drawing),
    width: 1400,
    height: 1050,
    ..templates::READY
};

// Its art. Six ticks, then what the gate draws, then what the picture holds.
const COAT_TICK: u16 = 0;
const S1_SKETCH: u16 = 6;
const S1_INK: u16 = 7;
const S2_INK: u16 = 8;
const S3_INK: u16 = 9;
const COAT_LASSO: u16 = 10;
const COAT_DRAG: u16 = 11;
const S1_SEED: u16 = 12;
const S2_SEED: u16 = 13;
const S3_SEED: u16 = 14;
const RIM_1: u16 = 15;
const RIM_2: u16 = 16;
const RIM_3: u16 = 17;
const S1_MOVED: u16 = 18;
const COAT_DECKLE: u16 = 19;
const COAT_EYE: u16 = 20;
const COAT_HOLE_1: u16 = 21;
const COAT_HOLE_2: u16 = 22;
const REFERENCE: u16 = 23;
const REFERENCE_RIM: u16 = 24;
const OVERLAP_SEED: u16 = 25;

/// Where the night is poured.
pub(crate) const CORNER: [f32; 2] = [36.0, 36.0];
/// Each shaving's middle, and how far its edge reaches. They stand clear of Deckle.
pub(crate) const SHAVINGS: [Pt; 3] = [[260.0, 280.0], [1140.0, 330.0], [300.0, 790.0]];
const SHAVING_REACH: f32 = 196.0;
/// `Select`'s drag: the first shaving comes down over the third.
pub(crate) const COAT_MOVE: Pt = [24.0, 266.0];
const SHAVING_LINE: f32 = 6.0;
/// A rim's brush: half of it runs off the wood, 15 px past the edge.
const RIM_WIDE: f32 = 30.0;
/// The Deckle a coat is for: a third of the paper's height, on its two feet.
const COAT_ARCH: [Pt; 2] = [[570.0, 700.0], [834.0, 694.0]];
const COAT_ARCH_LONG: f32 = 560.0;
/// `Strength`, where `nudge_strength` leaves it: yellow lifts, brown does not.
const COAT_GLOW: f32 = 0.5;

fn coat_shavings() -> [art::Shaving; 3] {
    [0, 1, 2].map(|i| art::shaving(3 + 5 * i as u32, SHAVINGS[i], SHAVING_REACH))
}

fn moved_by(path: &Path, by: Pt) -> Path {
    let points = path.points.iter().map(|p| [p[0] + by[0], p[1] + by[1], p[2]]).collect();
    Path { points, width: path.width, kind: PathKind::Finish, over: false }
}

fn seed_at(at: Pt) -> Path {
    Path { points: vec![[at[0], at[1], 1.0]], width: 1.0, kind: PathKind::Seed, over: false }
}

/// The strip's middle, as a wide white line: both sides by their share of the way.
fn strip_body(deckle: &art::Deckle) -> Vec<Pt> {
    const ROWS: usize = 120;
    let (cut, torn) = (&deckle.cut.points[..deckle.sides[0]], &deckle.torn.points[..deckle.sides[1]]);
    let at = |line: &[[f32; 3]], t: f32| line[((line.len() - 1) as f32 * t).round() as usize];
    let mid = |i: usize| {
        let t = i as f32 / ROWS as f32;
        let (a, b) = (at(cut, t), at(torn, 1.0 - t));
        [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
    };
    (0..=ROWS).map(mid).collect()
}

fn coat_art() -> Vec<Path> {
    use art::hand;
    let [one, two, three] = coat_shavings();
    let tick = |at: Pt, seed| hand(&[[at[0] - 7.0, at[1] - 15.0], [at[0] + 7.0, at[1] + 15.0]], 4.0, PathKind::Landmark, seed);
    let ticks = SHAVINGS.iter().enumerate().flat_map(|(i, at)| {
        let seed = i as u32 * 2;
        [tick([at[0] - SHAVING_REACH * 1.08, at[1]], seed + 1), tick([at[0] + SHAVING_REACH * 1.08, at[1]], seed + 2)]
    });
    let mut art: Vec<Path> = ticks.collect();
    let ink = |shaving: art::Shaving, seed| hand(&shaving.curl(), SHAVING_LINE, PathKind::StandIn, seed);
    let sketch = art::Shaving { at: [SHAVINGS[0][0] + 12.0, SHAVINGS[0][1] - 10.0], ..one };
    let [ink_1, ink_2, ink_3] = [ink(one, 41), ink(two, 42), ink(three, 43)];
    // The lasso goes round the first shaving and no other.
    let round = |i: usize| {
        let t = i as f32 / 60.0 * std::f32::consts::TAU;
        [SHAVINGS[0][0] + t.cos() * SHAVING_REACH * 1.2, SHAVINGS[0][1] + t.sin() * SHAVING_REACH * 1.02]
    };
    let lasso: Vec<Pt> = (0..=60).map(round).collect();
    let from = SHAVINGS[0];
    let drag = [from, [from[0] + COAT_MOVE[0], from[1] + COAT_MOVE[1]]];
    let moved = art::Shaving { at: drag[1], ..one };
    // Each tap on the side away from the overlap, past the other shaving's edge.
    let seed_1 = moved.bare(&moved_by(&ink_1, COAT_MOVE), |q| q[1] < drag[1][1]);
    let seed_2 = two.bare(&ink_2, |_| true);
    let seed_3 = three.bare(&ink_3, |q| q[1] > SHAVINGS[2][1]);
    let rim = |shaving: art::Shaving, seed| hand(&shaving.edge(1.0), RIM_WIDE, PathKind::StandIn, seed);
    let moved_ink = moved_by(&ink_1, COAT_MOVE);
    art.extend([
        hand(&sketch.curl(), 3.0, PathKind::StandIn, 40),
        ink_1,
        ink_2,
        ink_3,
        hand(&lasso, 2.0, PathKind::StandIn, 44),
        hand(&drag, 2.0, PathKind::StandIn, 45),
        seed_at(seed_1),
        seed_at(seed_2),
        seed_at(seed_3),
        rim(moved, 54),
        rim(two, 55),
        rim(three, 56),
        moved_ink,
    ]);
    let stand = arch(COAT_ARCH[0], COAT_ARCH[1], COAT_ARCH_LONG);
    let art::Deckle { eye, holes: [hole_1, hole_2], .. } = art::upright(&stand, 2);
    let [line, _] = joined(art::upright(&stand, 2));
    art.extend([Path { kind: PathKind::Finish, ..line }, eye, hole_1, hole_2]);
    let reference = art::shaving(5, [700.0, 525.0], SHAVING_REACH);
    art.push(hand(&reference.curl(), SHAVING_LINE, PathKind::Finish, 50));
    art.push(hand(&reference.edge(0.86), SHAVING_LINE, PathKind::Finish, 51));
    // Where two shavings lie over each other the lines shut a space of its own.
    art.push(seed_at([(moved.at[0] + three.at[0]) * 0.5, (moved.at[1] + three.at[1]) * 0.5]));
    art
}

/// Deckle on the color layer, line and white body both, so that hiding the
/// sketch cannot hide him. The color layer is the slot under the line.
fn coat_prep(s: &mut crate::doc::AppState) {
    const COLOR_SLOT: usize = 1;
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    let art = coat_art();
    let ink = |i: u16, rgba| {
        let path = &art[usize::from(i)];
        PathInk { points: &path.points, width: path.width, rgba, erase: false, inside: false }
    };
    let deckle = art::upright(&arch(COAT_ARCH[0], COAT_ARCH[1], COAT_ARCH_LONG), 2);
    let body = strip_body(&deckle).iter().map(|p| [p[0], p[1], 1.0]).collect::<Vec<_>>();
    let body = PathInk { points: &body, width: COAT_ARCH_LONG * 0.17, rgba: WHITE, erase: false, inside: false };
    let lines = [body, ink(COAT_DECKLE, BLACK), ink(COAT_EYE, BLACK), ink(COAT_HOLE_1, BLACK), ink(COAT_HOLE_2, BLACK)];
    s.commit_path(&lines, COLOR_SLOT);
    s.strokes_done = 0;
}

fn add_layer(e: &mut Editor) {
    if at_step(e, Stage::Drawing) {
        e.state.layer_add_role(LayerRole::Custom);
    }
}

/// `Hide your sketch`: the layer it was drawn on, under the new one.
fn hide_sketch(e: &mut Editor) {
    if !at_step(e, Stage::Drawing) {
        return;
    }
    let drawing = e.state.current_drawing().and_then(|id| e.state.cut().drawing(id));
    let sketch = drawing.and_then(|d| d.layers.iter().find(|l| l.props.role == LayerRole::Line));
    let Some((id, mut props)) = sketch.map(|l| (l.id, l.props.clone())) else { return };
    props.visible = false;
    e.state.layer_set_props(id, props);
}

/// `Select` is a toggle: the chord would put it down again.
fn arm_select(e: &mut Editor) {
    if at_step(e, Stage::Drawing) {
        e.canvas.set_tool(CanvasTool::Select, &mut e.state);
    }
}

fn arm_fill(e: &mut Editor) {
    e.canvas.set_tool(CanvasTool::Fill, &mut e.state);
}

fn pick_brown(e: &mut Editor) {
    pick(e, BROWN_INK);
    if at_step(e, Stage::InkPaint) {
        arm_fill(e);
    }
}

fn pick_yellow_and_brush(e: &mut Editor) {
    pick(e, YELLOW_INK);
    arm_brush(e);
}

/// The ground comes last: with it down the whole paper counts as inside.
fn night_at_the_corner(e: &mut Editor) {
    if on_the_paper(e) {
        pick(e, NIGHT_INK);
        arm_fill(e);
        e.canvas.fill_at(pos2(CORNER[0], CORNER[1]), &mut e.state);
    }
}

/// `Strength`, by the shelf's own road, to `to`.
fn set_glow(e: &mut Editor, to: f32) {
    if !at_step(e, Stage::Composite) {
        return;
    }
    let glow = |n: &Node| matches!(n.kind, NodeKind::Effect { fx: FxKind::Glow, .. }).then(|| (n.id, n.kind.clone()));
    let Some((id, mut kind)) = e.state.cut().graph.nodes().find_map(glow) else { return };
    if let NodeKind::Effect { params, .. } = &mut kind {
        params.set("intensity", FxValue::Float(to));
    }
    e.state.graph_set_kind(id, kind);
}

fn nudge_strength(e: &mut Editor) {
    set_glow(e, COAT_GLOW);
}

const LAYERS_KEY: Target = Target::Rail(Key::Layers);
const MORE_KEY: Target = Target::Rail(Key::More);
const KEEP_INSIDE: &[Target] = &[Target::Rail(Key::Brush), Target::Float(crate::floats::Float::Brushes), Target::Named("keep.row")];

const COAT_TASKS: &[Task] = &[
    Task {
        why: Some("A sketch is for finding the shape, so it can be ugly."),
        guide: &[COAT_TICK, COAT_TICK + 1],
        stand_in: &[S1_SKETCH],
        ..act(
            "Deckle is cold, and pencil shavings make a good coat. Sketch one big shaving between the first two ticks. A curl with a ruffled edge. Keep it loose.",
            Stage::Drawing,
            Done::Ink(&[S1_SKETCH]),
            BRUSH_KEY,
            Show::Where,
        )
    },
    act(
        "Open {Layers}, press + and pick {Custom layer}. Your sketch stays under it.",
        Stage::Drawing,
        Done::Layers(3),
        &[MORE_KEY, LAYERS_KEY, Target::Named("layers.add"), Target::Named("layers.custom")],
        Show::Run(add_layer),
    ),
    Task {
        guide: &[COAT_TICK, COAT_TICK + 1],
        stand_in: &[S1_INK],
        ..act(
            "Ink the shaving on the new layer. One clean line, thicker on the turns.",
            Stage::Drawing,
            Done::All(&[Done::InkOnLayer, Done::Ink(&[S1_INK])]),
            ON_PAPER,
            Show::Where,
        )
    },
    act(
        "Hide your sketch with its eye.",
        Stage::Drawing,
        Done::LayerHidden,
        &[MORE_KEY, LAYERS_KEY, Target::Named("layers.eye")],
        Show::Run(hide_sketch),
    ),
    Task {
        guide: &[COAT_TICK + 2, COAT_TICK + 3, COAT_TICK + 4, COAT_TICK + 5],
        stand_in: &[S2_INK, S3_INK],
        ..act(
            "Draw two more shavings at the other ticks. No two alike.",
            Stage::Drawing,
            Done::Ink(&[S2_INK, S3_INK]),
            ON_PAPER,
            Show::Where,
        )
    },
    Task {
        stand_in: &[COAT_LASSO, COAT_DRAG],
        ..act(
            "Pick {Select}, circle one shaving and drag it over another. A coat overlaps.",
            Stage::Drawing,
            Done::Moved,
            &[Target::Rail(Key::Select)],
            Show::Run(arm_select),
        )
    },
    act(
        "Open {View} and press {Mirror view}. A wrong curve shows up at once. Press it again when you have looked.",
        Stage::Drawing,
        Done::Mirrored,
        &[Target::Named("menu.view"), Target::Action(Action::MirrorView)],
        Show::Perform(Action::MirrorView),
    ),
    pill("Press {Color}.", "Color is where the wood gets warm, and your lines stay safe.", Stage::InkPaint, Action::RoomFinishing),
    Task {
        stand_in: &[S1_SEED, S2_SEED, S3_SEED, OVERLAP_SEED],
        ..act(
            "Pick brown and fill each shaving.",
            Stage::InkPaint,
            Done::Fills(3),
            &[Target::Named("swatch.brown"), Target::Rail(Key::Fill)],
            Show::Run(pick_brown),
        )
    },
    act(
        "Press {Brush}, open {Brushes} and turn on {Keep inside strokes}.",
        Stage::InkPaint,
        Done::Locked(true),
        KEEP_INSIDE,
        Show::Perform(Action::ToggleAlphaLock),
    ),
    Task {
        stand_in: &[RIM_1, RIM_2, RIM_3],
        ..act(
            "Pick yellow and paint a rim along each shaving. Paint it rough: it stays inside.",
            Stage::InkPaint,
            Done::Strokes(3),
            &[Target::Named("swatch.yellow"), Target::Rail(Key::Brush)],
            Show::Run(pick_yellow_and_brush),
        )
    },
    act(
        "Turn {Keep inside strokes} off.",
        Stage::InkPaint,
        Done::Locked(false),
        KEEP_INSIDE,
        Show::Perform(Action::ToggleAlphaLock),
    ),
    act(
        "Pick {Night} on this card and fill the paper around the coat.",
        Stage::InkPaint,
        Done::Filled { seed: CORNER, keep: None },
        &[Target::Named(lesson::CHIP), Target::Rail(Key::Fill)],
        Show::Run(night_at_the_corner),
    ),
    pill("Press {Effects}.", "Effects is where the rims catch the light.", Stage::Composite, Action::RoomComposite),
    act("Tap {Glow}.", Stage::Composite, Done::Effect(FxKind::Glow), GLOW_KEY, Show::Run(add_glow)),
    act(
        "Drag {Strength} until only the rims catch the light.",
        Stage::Composite,
        Done::Strength,
        &[Target::Named("shelf.strength")],
        Show::Run(nudge_strength),
    ),
    pill("Press {Export}.", "Export turns your project into a file anyone can open.", Stage::Export, Action::RoomExport),
    act(
        "Save your picture.",
        Stage::Export,
        Done::Exported(1),
        &[Target::Action(Action::ExportStill)],
        Show::Perform(Action::ExportStill),
    ),
];

// ---- Still in the Coat ----

const STILL: Template = Template {
    name: "Still in the Coat",
    kind: Some(ProjectKind::Animation),
    width: 1024,
    height: 768,
    cut_frames: 24,
    sheet_on: true,
    columns: &[("BG", ColumnKind::Bg), ("A", ColumnKind::Cel)],
    ..templates::READY
};

// Its art: each pose's outline, rim line and eye, then the two ticks and the card.
const STILL_POSES: usize = 3;
const COAT_REST: u16 = 0;
const RIM_REST: u16 = 1;
const EYE_REST: u16 = 2;
const COAT_LEFT: u16 = 3;
const RIM_LEFT: u16 = 4;
const EYE_LEFT: u16 = 5;
const COAT_RIGHT: u16 = 6;
const RIM_RIGHT: u16 = 7;
const EYE_RIGHT: u16 = 8;
const FLOOR_TICK: u16 = 9;
const CENTRE_TICK: u16 = 10;
const STILL_CARD: u16 = 11;
const STILL_CARD_PATHS: usize = 9;

/// The desk's top, and the coat's middle over it, at rest.
pub(crate) const STILL_FLOOR: f32 = 610.0;
const STILL_MIDDLE: f32 = 512.0;
const COAT_REACH: f32 = 170.0;
/// The rim's inner line, of the way out.
const COAT_RIM: f32 = 0.84;
/// Each pose's squeeze across, and where its middle goes. Its height is the rest
/// of the same wood: `wide` × `tall` is one.
const COAT_POSES: [(f32, f32); STILL_POSES] = [(1.0, 0.0), (0.78, -110.0), (0.78, 110.0)];
const COAT_SEED: u32 = 6;
const COAT_EYE_WIDE: f32 = 16.0;
/// How long a coat's eye is pulled: a stroke at any fit.
const EYE_PULL: f32 = 26.0;
const STILL_NIGHT: [u8; 4] = [0x14, 0x1B, 0x34, 0xFF];
const STILL_DESK: [u8; 4] = [0x2C, 0x30, 0x4A, 0xFF];
const STILL_EDGE: [u8; 4] = [0x4A, 0x4F, 0x6E, 0xFF];

/// The coat in pose `i`: a blob of shavings by the same generator.
pub(crate) fn coat(i: usize) -> art::Shaving {
    let (wide, by) = COAT_POSES[i];
    let tall = art::FLAT / wide;
    let flat = art::Shaving { wide, tall, ..art::shaving(COAT_SEED, [0.0, 0.0], COAT_REACH) };
    // Stood on the desk: the squeeze goes up, and the feet stay on the floor.
    let feet = flat.edge(1.0).iter().fold(0.0f32, |low, p| low.max(p[1]));
    art::Shaving { at: [STILL_MIDDLE + by, STILL_FLOOR - feet], ..flat }
}

/// Its wood, under the eye; and its rim, between the two lines.
pub(crate) fn coat_seeds(i: usize) -> [Pt; 2] {
    let coat = coat(i);
    [[coat.at[0], coat.at[1] + COAT_REACH * 0.3], coat.point(0.5 + 0.5 / 7.0, (1.0 + COAT_RIM) * 0.5)]
}

fn coat_lines(i: usize, at: Pt, kind: PathKind) -> [Path; 3] {
    let coat = coat(i);
    let moved = art::Shaving { at, ..coat };
    let seed = 61 + i as u32 * 5;
    let eye = art::dash([at[0] - 30.0, at[1] - 34.0], EYE_PULL, COAT_EYE_WIDE, kind);
    [art::hand(&moved.edge(1.0), 5.0, kind, seed), art::hand(&moved.edge(COAT_RIM), 4.0, kind, seed + 1), eye]
}

fn still_art() -> Vec<Path> {
    let mut art: Vec<Path> = (0..STILL_POSES).flat_map(|i| coat_lines(i, coat(i).at, PathKind::StandIn)).collect();
    let tick = |at: Pt, run: Pt, seed| Path { over: true, ..art::hand(&[at, [at[0] + run[0], at[1] + run[1]]], 4.0, PathKind::Landmark, seed) };
    art.push(tick([STILL_MIDDLE - 20.0, STILL_FLOOR + 6.0], [40.0, 1.0], 70));
    art.push(tick([STILL_MIDDLE, 128.0], [1.0, 40.0], 71));
    // The card's: a row of the three, each its own place in it.
    for i in 0..STILL_POSES {
        let at = [300.0 + i as f32 * 420.0, 300.0];
        art.extend(coat_lines(i, at, PathKind::Finish));
    }
    art
}

/// The desk's top under the coat, and the night over it.
fn still_texel(_: i32, y: i32) -> [u8; 4] {
    match y as f32 {
        y if y < STILL_FLOOR => STILL_NIGHT,
        y if y < STILL_FLOOR + 10.0 => STILL_EDGE,
        _ => STILL_DESK,
    }
}

fn still_prep(s: &mut crate::doc::AppState) {
    bg_plate(s, STILL.width, STILL.height, still_texel);
}

/// The Plan keys' three drawings, each its kind: what the sheet's menu writes.
fn mark_keys(e: &mut Editor) {
    let column = e.state.view.active_column;
    let shown = runs_here(e);
    let drawn = shown.iter().filter_map(|(_, shows)| if let Exposure::Drawing(id) = shows { Some(*id) } else { None });
    let drawn: Vec<_> = drawn.collect();
    drawn.into_iter().for_each(|id| e.state.set_drawing_kind(id, DrawingKind::Key));
    e.state.view.active_column = column;
}

fn goto_first(e: &mut Editor) {
    if at_step(e, Stage::Edit) {
        e.state.goto(0);
    }
}

/// Row 16: the blank row past the right pose. A drawing on a blank tail keeps to
/// its row, so the pose's run is one, and the rest takes the row after it.
const REST_AGAIN: u32 = 15;

fn goto_rest_row(e: &mut Editor) {
    if at_step(e, Stage::Edit) {
        e.state.goto(REST_AGAIN);
    }
}

/// `Click row 16 twice, type {rest drawing}`: the name the sheet's first row
/// shows, typed into its cell.
fn the_rest_comes_back(e: &mut Editor) {
    goto_rest_row(e);
    let name = e.lesson_rest();
    e.state.enter_cell(&name);
}

fn turn_on_loop(e: &mut Editor) {
    if at_step(e, Stage::Edit) {
        e.state.view.loop_playback = true;
        let (here, last) = (e.state.view.frame, e.state.shown_length() - 1);
        e.state.goto(0);
        e.state.set_loop_in();
        e.state.goto(last);
        e.state.set_loop_out();
        e.state.goto(here);
    }
}

/// Task 12's person: on each drawing, brown for the wood and yellow for the rim.
fn fill_the_coats(e: &mut Editor) {
    if !at_step(e, Stage::InkPaint) {
        return;
    }
    arm_fill(e);
    let column = e.state.view.active_column;
    for (pose, (from, _)) in runs_here(e).into_iter().take(STILL_POSES).enumerate() {
        e.state.select_cells((column, from), column, from);
        let [wood, rim] = coat_seeds(pose);
        for (ink, seed) in [(BROWN_INK, wood), (YELLOW_INK, rim)] {
            e.canvas.brush_color = ink.to_array();
            if e.canvas.slot_at_pen_down(true, &mut e.state).is_some() {
                e.canvas.fill_at(pos2(seed[0], seed[1]), &mut e.state);
            }
        }
    }
}

const fn cell(frame: u32) -> &'static [Target] {
    match frame {
        0 => &[Target::Named("band.sheet"), Target::Cell { track: TRACK_A, frame: 0 }],
        4 => &[Target::Named("band.sheet"), Target::Cell { track: TRACK_A, frame: 4 }],
        _ => &[Target::Named("band.sheet"), Target::Cell { track: TRACK_A, frame: 6 }],
    }
}

const STILL_TASKS: &[Task] = &[
    Task {
        why: Some("Plan is where the main poses land on the rows you choose."),
        guide: &[FLOOR_TICK, CENTRE_TICK],
        stand_in: &[COAT_REST, RIM_REST, EYE_REST],
        ..act(
            "Deckle has wrapped itself in a coat of shavings. Click row 1 on track A and draw the coat at rest.",
            Stage::Drawing,
            Done::KeyArt { track: TRACK_A, frame: 0 },
            cell(0),
            Show::Where,
        )
    },
    Task {
        guide: &[FLOOR_TICK, CENTRE_TICK],
        stand_in: &[COAT_LEFT, RIM_LEFT, EYE_LEFT],
        ..act(
            "Click row 5. Draw the coat squeezed to the left.",
            Stage::Drawing,
            Done::KeyArt { track: TRACK_A, frame: 4 },
            cell(4),
            Show::Where,
        )
    },
    Task {
        guide: &[FLOOR_TICK, CENTRE_TICK],
        stand_in: &[COAT_RIGHT, RIM_RIGHT, EYE_RIGHT],
        ..act(
            "Click row 7. Draw it squeezed to the right.",
            Stage::Drawing,
            Done::KeyArt { track: TRACK_A, frame: 6 },
            cell(6),
            Show::Where,
        )
    },
    act("Press {Play}. Is it twitchy? Good.", Stage::Drawing, Done::Played, PLAY_KEY, Show::Perform(Action::PlayPause)),
    pill("Press {Timing}.", "This is where nothing gets to happen.", Stage::Edit, Action::RoomEdit),
    Task {
        hand: Some(mark_keys),
        ..act(
            "Right-click each drawing on the sheet and mark all three as keys.",
            Stage::Edit,
            Done::Kinds(3),
            &[Target::Cell { track: TRACK_A, frame: 0 }],
            Show::Run(mark_keys),
        )
    },
    act(
        "Click row 1 and press {Longer} until the rest lasts 12 rows. Wait longer than feels right.",
        Stage::Edit,
        Done::Hold { frame: 0, len: 12 },
        &[Target::Cell { track: TRACK_A, frame: 0 }, Target::Named("band.longer")],
        Show::Run(goto_first),
    ),
    Task {
        hand: Some(the_rest_comes_back),
        ..act(
            "Click row 16 twice, type {rest drawing} and press Enter. The rest drawing comes back after the shiver.",
            Stage::Edit,
            Done::All(&[Done::Shows { frame: REST_AGAIN - 1, drawing: 2 }, Done::Shows { frame: REST_AGAIN, drawing: 0 }]),
            &[Target::Cell { track: TRACK_A, frame: REST_AGAIN }],
            Show::Run(goto_rest_row),
        )
    },
    Task {
        hand: Some(turn_on_loop),
        ..act(
            "Turn on {Loop}, and set {Loop start} and {Loop end} around the shot.",
            Stage::Edit,
            Done::Loop,
            &[Target::Named("band.loop"), Target::Named("band.fold"), Target::Action(Action::LoopIn), Target::Action(Action::LoopOut)],
            Show::Run(turn_on_loop),
        )
    },
    act(
        "Press {Play}. If the wait is not funny yet, change it with {Longer} or {Shorter}.",
        Stage::Edit,
        Done::Played,
        PLAY_KEY,
        Show::Perform(Action::PlayPause),
    ),
    pill("Press {Color}.", "Your lines stay safe here.", Stage::InkPaint, Action::RoomFinishing),
    Task {
        hand: Some(fill_the_coats),
        ..act(
            "Fill all three drawings: brown for the wood, yellow for the rims.",
            Stage::InkPaint,
            Done::FilledOn(3),
            &[Target::Rail(Key::Fill), Target::Named("swatch.brown"), Target::Named("swatch.yellow")],
            Show::Run(pick_brown),
        )
    },
    pill("Press {Effects}.", "Effects is where the rims catch the light.", Stage::Composite, Action::RoomComposite),
    act("Tap {Glow}.", Stage::Composite, Done::Effect(FxKind::Glow), GLOW_KEY, Show::Run(add_glow)),
    act(
        "Drag {Strength} until only the rims catch the light.",
        Stage::Composite,
        Done::Strength,
        &[Target::Named("shelf.strength")],
        Show::Run(nudge_strength),
    ),
    pill("Press {Export}.", "Export turns your drawings into a file that loops forever.", Stage::Export, Action::RoomExport),
    act(
        "Save a GIF.",
        Stage::Export,
        Done::Exported(1),
        &[Target::Action(Action::ExportGif)],
        Show::Perform(Action::ExportGif),
    ),
];

// ---- Too Much Coat ----

// Its art: page one's six, page two's five, its coat, the ticks and the card.
const T1: u16 = 0;
const TREE_1: u16 = 6;
const COAT_BIG: u16 = 11;
const COAT_BIG_CURL: u16 = 12;
const COAT_BIG_EYE: u16 = 13;
const COAT_BIG_SEED: u16 = 14;
const HORIZON_TICK: u16 = 15;
const POINT_TICK: u16 = 16;
const COMIC_CARD: u16 = 17;
const COMIC_CARD_PATHS: usize = 14;

/// Where each panel of `split_into_six` stands: two tall, then four small.
pub(crate) const PANEL_MIDDLES: [Pt; 6] =
    [[262.0, 350.0], [626.0, 350.0], [262.0, 757.0], [626.0, 757.0], [262.0, 1029.0], [626.0, 1029.0]];
/// The 1-point ruler's point: `Rulers::preset`'s on this page.
const VANISHING: Pt = [444.0, 497.0];
pub(crate) const BIG_COAT_AT: Pt = [444.0, 900.0];
const BIG_COAT_REACH: f32 = 300.0;
const PANEL_LINE: f32 = 4.0;

fn big_coat() -> art::Shaving {
    art::shaving(COAT_SEED, BIG_COAT_AT, BIG_COAT_REACH)
}

/// Deckle flat, dragging a shaving: panel `i` of page one, the coat growing.
fn dragged(i: usize, at: Pt, scale: f32, kind: PathKind) -> [Path; 2] {
    const LONG: f32 = 150.0;
    let (seed, reach) = (20 + i as u32, (8.0 + i as f32 * 0.9) * scale);
    let tail = [at[0] - LONG * 0.2 * scale, at[1] + 30.0 * scale];
    let spine = art::spine(tail, [tail[0] + LONG * scale, tail[1]], LONG * scale);
    let [line, _] = closed(&spine, seed);
    // Small on the page: a shaving is one quick curl of the pen, under a long line.
    let from = [tail[0] - reach * 1.1, tail[1] + reach * 0.2];
    let up = |x: f32, y: f32| [from[0] + reach * x, from[1] - reach * y];
    let wood = art::cubic([from, up(0.2, 1.4), up(1.8, 1.4), up(2.0, 0.0)]);
    [Path { kind, ..line }, art::hand(&wood, PANEL_LINE, kind, seed)]
}

/// A pencil going away, as a tree does: its near foot and how tall it stands.
const TREES: [(Pt, f32); 5] = [
    ([116.0, 590.0], 430.0),
    ([226.0, 562.0], 300.0),
    ([316.0, 536.0], 170.0),
    ([716.0, 590.0], 430.0),
    ([612.0, 562.0], 300.0),
];

fn tree(i: usize, kind: PathKind) -> Path {
    let (foot, tall) = TREES[i];
    let thick = tall * 0.12;
    let top = [foot[0] + (VANISHING[0] - foot[0]) * 0.1, foot[1] - tall];
    let along = |x: f32, up: f32| [top[0] + thick * x, top[1] - thick * up];
    // The barrel stops square and the point stands on it: a shoulder either side.
    let point = [along(0.2, 0.0), along(0.5, 0.64), along(0.8, 0.0)];
    let mut line = vec![foot, top];
    line.extend(point);
    line.extend([along(1.0, 0.0), [foot[0] + thick, foot[1]], foot]);
    art::hand(&line, PANEL_LINE, kind, 80 + (i * 5 / 4) as u32)
}

fn comic_art() -> Vec<Path> {
    let mut art: Vec<Path> = (0..6).map(|i| { let [line, _] = dragged(i, PANEL_MIDDLES[i], 1.0, PathKind::StandIn); line }).collect();
    art.extend((0..TREES.len()).map(|i| tree(i, PathKind::StandIn)));
    let coat = big_coat();
    let line = art::hand(&coat.edge(1.0), 8.0, PathKind::StandIn, 90);
    let curl = art::hand(&coat.swirl(), 6.0, PathKind::StandIn, 91);
    let eye = art::dash([BIG_COAT_AT[0] + 60.0, BIG_COAT_AT[1] - 60.0], EYE_PULL * 1.7, 22.0, PathKind::StandIn);
    let seed = coat.bare(&line, |q| q[1] > BIG_COAT_AT[1] + 40.0);
    art.extend([line, curl, eye, seed_at(seed)]);
    let tick = |at: Pt, run: Pt, seed| art::hand(&[at, [at[0] + run[0], at[1] + run[1]]], 4.0, PathKind::Landmark, seed);
    art.push(tick([40.0, VANISHING[1]], [40.0, 1.0], 92));
    art.push(tick([VANISHING[0], VANISHING[1] - 20.0], [1.0, 40.0], 93));
    // The card: six in two rows of three, and page two beside them.
    for i in 0..6 {
        let at = [160.0 + (i % 3) as f32 * 330.0, 120.0 + (i / 3) as f32 * 270.0];
        art.extend(dragged(i, at, 1.4, PathKind::Finish));
    }
    let small = art::shaving(COAT_SEED, [1400.0, 370.0], 170.0);
    art.push(art::hand(&small.edge(1.0), 5.0, PathKind::Finish, 94));
    art.push(art::hand(&[[1160.0, 150.0], [1640.0, 150.0]], 5.0, PathKind::Finish, 95));
    art
}

/// Two rows of the first panel's half, then each row across: two tall, four
/// small. A page's pace, cut by the same `Split` a person's Panel tool makes.
fn split_into_six(e: &mut Editor) {
    if !at_step(e, Stage::Manga) {
        return;
    }
    const CUTS: [(usize, PanelOp); 5] = [
        (0, PanelOp::SplitAcross),
        (1, PanelOp::SplitAcross),
        (2, PanelOp::SplitDown),
        (1, PanelOp::SplitDown),
        (0, PanelOp::SplitDown),
    ];
    for (panel, op) in CUTS {
        e.state.manga.selected = panel;
        manga_tools::panels(&mut e.state, op);
    }
}

fn open_gutters(e: &mut Editor) {
    if at_step(e, Stage::Manga) {
        e.state.manga.side_mm += 1.0;
        e.state.manga.row_mm += 1.0;
    }
}

/// `manga.rulers`' one point, along the desk, and the snap on.
fn open_rulers(e: &mut Editor) {
    if !at_step(e, Stage::Manga) {
        return;
    }
    let (w, h) = (e.state.engine.project.width as f32, e.state.engine.project.height as f32);
    let rulers = Rulers::preset(1, w, h);
    e.state.manga_edit("perspective rulers", |cut, at| rulers.set(cut, at).into_iter().collect());
    e.state.manga.snap = true;
}

const TOO_MUCH_COAT_TASKS: &[Task] = &[
    Task {
        why: Some("A comic's pace is its panels. Six small ones read fast."),
        hand: Some(split_into_six),
        ..act(
            "Deckle wants more coat. Page one: pick the {Panel tool} and split it into six panels.",
            Stage::Manga,
            Done::Panels(6),
            &[Target::Action(Action::PanelTool)],
            Show::Perform(Action::PanelTool),
        )
    },
    Task {
        hand: Some(open_gutters),
        ..act(
            "Open the gutters a little. They are the silence between panels.",
            Stage::Manga,
            Done::Gutters,
            &[Target::Pane(Pane::Manga), Target::Named("manga.gutter.side"), Target::Named("manga.gutter.row")],
            Show::Run(open_gutters),
        )
    },
    Task {
        stand_in: &[T1, T1 + 1, T1 + 2, T1 + 3, T1 + 4, T1 + 5],
        ..act(
            "In each panel, draw Deckle dragging one shaving home. The thumbnails are on this card.",
            Stage::Manga,
            Done::InkInPanels(6),
            BRUSH_KEY,
            Show::Where,
        )
    },
    act("Press {New page}.", Stage::Manga, Done::Pages(2), &[Target::Action(Action::NewPage)], Show::Perform(Action::NewPage)),
    Task {
        hand: Some(open_rulers),
        ..act(
            "Set a 1-point ruler along the desk, and turn snap on.",
            Stage::Manga,
            Done::Rulers(1),
            &[Target::Pane(Pane::Manga), Target::Named("manga.rulers"), Target::Action(Action::ToggleRulerSnap)],
            Show::Run(open_rulers),
        )
    },
    Task {
        guide: &[HORIZON_TICK, POINT_TICK],
        stand_in: &[TREE_1, TREE_1 + 1, TREE_1 + 2, TREE_1 + 3, TREE_1 + 4],
        ..act("Page two is one big picture: pencils going away like trees.", Stage::Manga, Done::Strokes(5), ON_PAPER, Show::Where)
    },
    Task {
        guide: &[HORIZON_TICK, POINT_TICK],
        stand_in: &[COAT_BIG, COAT_BIG_CURL, COAT_BIG_EYE],
        ..act("Put the surprise here, after the page turn: one giant coat, one small eye.", Stage::Manga, Done::Strokes(3), ON_PAPER, Show::Where)
    },
    pill("Press {Color}.", "One flat color is this page's whole budget. Spend it on the coat.", Stage::InkPaint, Action::RoomFinishing),
    Task {
        stand_in: &[COAT_BIG_SEED],
        ..act(
            "Fill the coat with one color.",
            Stage::InkPaint,
            Done::Fills(1),
            &[Target::Named("swatch.brown"), Target::Rail(Key::Fill)],
            Show::Run(pick_brown),
        )
    },
    pill("Press {Export}.", "Two pages, two files.", Stage::Export, Action::RoomExport),
    act(
        "Press {Export pages}.",
        Stage::Export,
        Done::Exported(1),
        &[Target::Action(Action::ExportPages)],
        Show::Perform(Action::ExportPages),
    ),
];

/// The `u16`s from `first`, `N` of them: a run of art, as a table's list.
const fn run_of<const N: usize>(first: u16) -> [u16; N] {
    let mut row = [first; N];
    let mut i = 0;
    while i < N {
        row[i] = first + i as u16;
        i += 1;
    }
    row
}

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
        finish: &[BACK, BELLY, HOLE_1, HOLE_2, HOLE_3, PENCIL, STAR_1, STAR_2, STAR_3],
        thumb: &[],
        card: &[],
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
        ink: Some(CHALK_INK),
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
        thumb: &[THUMB, THUMB + 1],
        card: &[],
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
        thumb: &[],
        card: &[],
        tasks: NOT_A_PENCIL_TASKS,
        closing: "You made that. It is a real file. You can split a page into panels and make a reader wait between them.",
    },
    Lesson {
        id: "coat_of_shavings",
        kind: ProjectKind::Drawing,
        class: Class::Amateur,
        title: "A Coat of Shavings",
        hover: "Deckle is cold: sketch, ink, move and paint him a coat of pencil shavings. About 25 minutes. Ends in a PNG.",
        paper: &COAT,
        prep: Some(coat_prep),
        fill_gap: None,
        ink: None,
        latch: None,
        art: coat_art,
        finish: &[S1_MOVED, S2_INK, S3_INK, RIM_1, RIM_2, RIM_3, COAT_DECKLE, COAT_EYE, COAT_HOLE_1, COAT_HOLE_2],
        thumb: &[REFERENCE, REFERENCE_RIM],
        card: &[REFERENCE, REFERENCE_RIM],
        tasks: COAT_TASKS,
        closing: "You made that. It is a real file. You can sketch under your ink, move what you drew, check it mirrored and paint inside your own shapes.",
    },
    Lesson {
        id: "still_in_the_coat",
        kind: ProjectKind::Animation,
        class: Class::Amateur,
        title: "Still in the Coat",
        hover: "Three poses on the rows you choose, a long wait, then a shiver in a coat of shavings. About 25 minutes. Ends in a GIF.",
        paper: &STILL,
        prep: Some(still_prep),
        fill_gap: None,
        ink: Some([255, 255, 255, 255]),
        latch: None,
        art: still_art,
        finish: &[COAT_REST, RIM_REST, EYE_REST],
        thumb: &run_of::<STILL_CARD_PATHS>(STILL_CARD),
        card: &run_of::<STILL_CARD_PATHS>(STILL_CARD),
        tasks: STILL_TASKS,
        closing: "You made that. It is a real file. You can plant keys on the rows you choose, wait on purpose and reuse a drawing.",
    },
    Lesson {
        id: "too_much_coat",
        kind: ProjectKind::Comic,
        class: Class::Amateur,
        title: "Too Much Coat",
        hover: "Six fast panels of a coat growing, then a page turn and the surprise. About 30 minutes. Ends in two page PNGs.",
        paper: &PAGE_SMALL,
        prep: None,
        fill_gap: None,
        ink: None,
        latch: None,
        art: comic_art,
        finish: &[TREE_1, TREE_1 + 1, TREE_1 + 2, TREE_1 + 3, TREE_1 + 4, COAT_BIG, COAT_BIG_CURL, COAT_BIG_EYE],
        thumb: &run_of::<COMIC_CARD_PATHS>(COMIC_CARD),
        card: &run_of::<COMIC_CARD_PATHS>(COMIC_CARD),
        tasks: TOO_MUCH_COAT_TASKS,
        closing: "You made that. It is real files. You can pace a page and put the surprise where the reader has to turn.",
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
        hand: None,
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
