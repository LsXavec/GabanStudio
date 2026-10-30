//! GM16: generators — pictures made from parameters, for anime FX and for
//! manga: deterministic particles, speed lines (ryūsen), focus lines
//! (shūchūsen), set lettering (shashoku, with tategaki) and speech balloons
//! (fukidashi). Like the FX (D9) they are drawn once, here, on the CPU on
//! premultiplied f32 frames: the CPU golden renderer's Generator arm calls
//! [`render`], and the GPU compositor shows its upload (NEVER-DO 2).
//!
//! Every kind declares [`ParamSpec`]s, so its inspector, defaults and binds
//! work as an Effect's ([`crate::fx`]). Units: positions and extents are
//! fractions of the frame (x of its width, y of its height: 0.5 is the
//! centre), so a template looks the same at every size; line widths, type
//! sizes, particle sizes and particle motion are px (motion per frame).
//! Angles are degrees, 0 pointing right, turning clockwise on screen (90
//! points down).
//!
//! Laws (pinned by tests/gen_tests.rs — change them together):
//! - **Particles are analytic.** Birth `i` happens at `t_i = i/rate −
//!   preroll`; it lives `L_i = lifetime·(1 + var·(2u − 1))` frames, so at
//!   frame f the alive set is every `i` with `0 ≤ f − t_i < L_i` (with no
//!   variance: the births within `(f − L, f]`). A particle's every random
//!   choice is a hash of (seed, i, stream), and its state at age `a` is
//!   closed form: `p = p0 + v0·a + ½(wind, gravity)·a² + turbulence(a)` —
//!   no history, so any frame renders alone and scrubbing is exact. Size,
//!   colour and opacity mix linearly from their start to their end values
//!   over life. Particles draw in birth order (Normal: src-over; Add: the
//!   colour adds and the coverage unions).
//! - **Speed lines**: `count` lines at `angle`, each at a seeded offset
//!   across the frame (skipping the clear band, widened by half the widest
//!   line), a seeded length (a fraction of the frame's extent along the
//!   lines) and thickness, tapered to a point at both ends
//!   (`w(s) = t·√(4s(1 − s))`).
//! - **Focus lines**: `count` radial lines, evenly spaced with a seeded
//!   wobble, from beyond the frame's farthest corner toward the centre, each
//!   stopping at the clear ellipse times `1 + jitter·u`, tapered to a point
//!   at the inner end (`w(s) = t·(1 − s)`). Nothing inks inside the ellipse.
//! - Lines held, re-seeded every frame, or every second or third frame
//!   ("on twos": frames 2k and 2k + 1 are one layout; "on threes": 3k to
//!   3k + 2).
//! - **Balloons** are signed distances: the body (ellipse, rounded rect,
//!   cloud = ellipse ∪ bumps on it, burst = a spiked polygon) united with
//!   the tail triangle (its base across the body's centre, its tip at the
//!   point); the fill is inside, the stroke the `stroke width` px just
//!   inside the outline, so the tail's tip pixel is the given point.
//! - **Text** sets [`crate::text`]'s block centred on (x, y), its top-left
//!   on whole pixels.

use std::f32::consts::PI;

use rayon::prelude::*;

use crate::color::ColorLaw;
use crate::fx::{self, FxBinds, FxParams, FxValue, Image, ParamKind, ParamSpec, color, flag, float};
use crate::ids::ParamId;
use crate::text::{self, Direction, Face, Style};

/// Particles alive at once at most; past it the oldest are not drawn.
pub const MAX_PARTICLES: u64 = 200_000;

/// What a Generator node draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum GenKind {
    Particles,
    SpeedLines,
    FocusLines,
    Text,
    Balloon,
}

// ---- Parameter specs ---------------------------------------------------------------

const fn choice_at(
    key: &'static str,
    label: &'static str,
    help: &'static str,
    options: &'static [&'static str],
    default: usize,
) -> ParamSpec {
    ParamSpec {
        key,
        label,
        help,
        kind: ParamKind::Choice {
            options,
            default,
            renamed: &[],
        },
    }
}

const fn text_spec(key: &'static str, label: &'static str, help: &'static str, default: &'static str) -> ParamSpec {
    ParamSpec {
        key,
        label,
        help,
        kind: ParamKind::Text { default },
    }
}

/// The particle emitters.
pub const EMITTERS: &[&str] = &["point", "line", "rect", "matte"];
/// The particle shapes.
pub const SHAPES: &[&str] = &["dot", "streak", "soft disc", "petal", "snowflake"];
const BLENDS: &[&str] = &["normal", "add"];
/// How lines move: one layout, a new one every frame, or every second or
/// third frame (on twos, on threes).
pub const MOTIONS: &[&str] = &["held", "every frame", "on twos", "on threes"];
const MOTION_HELP: &str = "Held keeps one layout; every frame, on twos or on threes draws a new one every 1, 2 or 3 frames.";
const DIRECTIONS: &[&str] = &["horizontal", "vertical"];
const FONTS: &[&str] = &["plex sans jp", "plex sans"];
/// The balloon bodies.
pub const BALLOONS: &[&str] = &["ellipse", "rounded rect", "cloud", "burst"];

const SEED_HELP: &str = "Picks the pattern; one seed draws the same way every render.";

const PARTICLE_SPECS: &[ParamSpec] = &[
    choice_at("emitter", "emitter", "Where particles are born: a point, a line, a rectangle, or wherever the matte input is opaque.", EMITTERS, 0),
    float("x", "x", "The emitter's centre across the frame: 0 is the left edge, 1 the right.", -2.0, 3.0, 0.5, 0.005),
    float("y", "y", "The emitter's centre down the frame: 0 is the top edge, 1 the bottom.", -2.0, 3.0, 0.5, 0.005),
    float("width", "width", "The line's length or the rectangle's width, as a fraction of the frame width.", 0.0, 4.0, 0.5, 0.005),
    float("height", "height", "The rectangle's height, as a fraction of the frame height.", 0.0, 4.0, 0.2, 0.005),
    float("rate", "rate", "Particles born per frame.", 0.0, 500.0, 10.0, 0.1),
    float("lifetime", "lifetime", "How many frames a particle lives.", 1.0, 1000.0, 24.0, 0.1),
    float("lifetime_var", "lifetime variance", "How much lifetimes differ: 0.25 lives 75% to 125% as long.", 0.0, 1.0, 0.25, 0.01),
    float("preroll", "preroll", "Frames already run before frame 0, so the frame starts full.", 0.0, 1000.0, 0.0, 1.0),
    float("speed", "speed", "Starting speed in px per frame.", 0.0, 500.0, 6.0, 0.05),
    float("speed_var", "speed variance", "How much starting speeds differ: 0.25 is 75% to 125%.", 0.0, 1.0, 0.25, 0.01),
    float("direction", "direction", "The starting direction in degrees: 0 right, 90 down, -90 up.", -360.0, 360.0, -90.0, 0.5),
    float("spread", "spread", "The cone of starting directions, in degrees (360 is every way).", 0.0, 360.0, 45.0, 0.5),
    float("gravity", "gravity", "Downward pull in px per frame per frame (negative rises).", -10.0, 10.0, 0.2, 0.005),
    float("wind", "wind", "Sideways push in px per frame per frame (negative blows left).", -10.0, 10.0, 0.0, 0.005),
    float("turbulence", "turbulence", "How far a particle wanders off its path, in px (a smooth wobble).", 0.0, 500.0, 0.0, 0.1),
    float("size", "size", "The size at birth, in px.", 0.0, 500.0, 4.0, 0.05),
    float("size_end", "size at death", "The size at the end of life, in px; it changes evenly over life.", 0.0, 500.0, 1.0, 0.05),
    color("color", "colour", "The colour at birth.", [255, 255, 255, 255]),
    color("color_end", "colour at death", "The colour at the end of life; it mixes evenly over life.", [255, 255, 255, 255]),
    float("opacity", "opacity", "The opacity at birth.", 0.0, 1.0, 1.0, 0.01),
    float("opacity_end", "opacity at death", "The opacity at the end of life.", 0.0, 1.0, 0.0, 0.01),
    choice_at("shape", "shape", "Dot, streak (along the motion), soft disc, petal or snowflake.", SHAPES, 2),
    choice_at("blend", "blend", "Normal lays particles over each other; add sums them into light.", BLENDS, 0),
    float("seed", "seed", SEED_HELP, 0.0, 65535.0, 1.0, 1.0),
];

const SPEED_LINE_SPECS: &[ParamSpec] = &[
    float("angle", "angle", "The lines' direction in degrees: 0 runs across, 90 runs down.", -360.0, 360.0, 0.0, 0.1),
    float("count", "lines", "How many lines.", 0.0, 2000.0, 80.0, 1.0),
    float("length_min", "length min", "The shortest line, as a fraction of the frame's extent along the lines.", 0.0, 4.0, 0.25, 0.005),
    float("length_max", "length max", "The longest line, as a fraction of the frame's extent along the lines.", 0.0, 4.0, 0.8, 0.005),
    float("thickness_min", "thickness min", "The thinnest line at its middle, in px.", 0.0, 200.0, 1.0, 0.05),
    float("thickness_max", "thickness max", "The thickest line at its middle, in px.", 0.0, 200.0, 4.0, 0.05),
    float("band", "clear band", "A band kept clear of lines, as a fraction of the frame across the lines (0 = none).", 0.0, 1.0, 0.0, 0.005),
    float("band_offset", "band offset", "Where the clear band sits across the lines: 0 is the middle, -1 and 1 the edges.", -1.0, 1.0, 0.0, 0.005),
    choice_at("motion", "motion", MOTION_HELP, MOTIONS, 0),
    color("color", "colour", "The lines' colour.", [0, 0, 0, 255]),
    float("seed", "seed", SEED_HELP, 0.0, 65535.0, 1.0, 1.0),
];

const FOCUS_LINE_SPECS: &[ParamSpec] = &[
    float("x", "centre x", "Where the lines point, across the frame: 0 left, 1 right.", -2.0, 3.0, 0.5, 0.005),
    float("y", "centre y", "Where the lines point, down the frame: 0 top, 1 bottom.", -2.0, 3.0, 0.5, 0.005),
    float("rx", "clear width", "The clear ellipse's half-width, as a fraction of the frame width.", 0.0, 4.0, 0.25, 0.005),
    float("ry", "clear height", "The clear ellipse's half-height, as a fraction of the frame height.", 0.0, 4.0, 0.25, 0.005),
    float("count", "lines", "How many lines around the centre.", 0.0, 2000.0, 120.0, 1.0),
    float("thickness_min", "thickness min", "The thinnest line at the frame's edge, in px.", 0.0, 200.0, 2.0, 0.05),
    float("thickness_max", "thickness max", "The thickest line at the frame's edge, in px.", 0.0, 200.0, 8.0, 0.05),
    float("jitter", "jitter", "How far past the ellipse a line may stop: 0.3 stops up to 30% farther out.", 0.0, 4.0, 0.15, 0.005),
    choice_at("motion", "motion", MOTION_HELP, MOTIONS, 0),
    color("color", "colour", "The lines' colour.", [0, 0, 0, 255]),
    float("seed", "seed", SEED_HELP, 0.0, 65535.0, 1.0, 1.0),
];

const TEXT_SPECS: &[ParamSpec] = &[
    text_spec("text", "text", "The words. A new line starts the next line, or in vertical text the next column to the left.", "セリフ"),
    choice_at("direction", "direction", "Horizontal lines, or vertical columns read right to left (tategaki).", DIRECTIONS, 0),
    choice_at("font", "font", "IBM Plex Sans JP Bold (kana, kanji and Latin) or Plex Sans; a missing glyph falls back to the other.", FONTS, 0),
    float("size", "size", "The type size (em) in px.", 1.0, 2000.0, 48.0, 0.5),
    float("leading", "leading", "Line pitch as a multiple of the size (column pitch in vertical text).", 0.5, 4.0, 1.3, 0.01),
    float("tracking", "tracking", "Extra space between characters, in px.", -200.0, 500.0, 0.0, 0.1),
    color("color", "colour", "The letters' colour.", [0, 0, 0, 255]),
    float("outline", "outline", "An outline around the letters, in px (0 = none).", 0.0, 100.0, 0.0, 0.05),
    color("outline_color", "outline colour", "The outline's colour.", [255, 255, 255, 255]),
    float("x", "x", "The block's centre across the frame: 0 left, 1 right.", -2.0, 3.0, 0.5, 0.005),
    float("y", "y", "The block's centre down the frame: 0 top, 1 bottom.", -2.0, 3.0, 0.5, 0.005),
    float("wrap", "wrap", "Wrap lines at this fraction of the frame width (columns at this fraction of its height); 0 never wraps.", 0.0, 4.0, 0.0, 0.005),
];

const BALLOON_SPECS: &[ParamSpec] = &[
    choice_at("shape", "shape", "Ellipse, rounded rect, cloud (thought) or burst (shout).", BALLOONS, 0),
    float("x", "x", "The balloon's centre across the frame: 0 left, 1 right.", -2.0, 3.0, 0.5, 0.005),
    // Centred where a new Text node's block is, so the first lettering sits
    // in its balloon.
    float("y", "y", "The balloon's centre down the frame: 0 top, 1 bottom.", -2.0, 3.0, 0.5, 0.005),
    float("width", "width", "The balloon's width, as a fraction of the frame width.", 0.0, 4.0, 0.4, 0.005),
    float("height", "height", "The balloon's height, as a fraction of the frame height.", 0.0, 4.0, 0.3, 0.005),
    flag("tail", "tail", "Draw a tail from the balloon to the tail point.", true),
    float("tail_x", "tail x", "The tail's tip across the frame: 0 left, 1 right.", -2.0, 3.0, 0.62, 0.005),
    float("tail_y", "tail y", "The tail's tip down the frame: 0 top, 1 bottom.", -2.0, 3.0, 0.82, 0.005),
    float("tail_width", "tail width", "The tail's width where it leaves the balloon, as a fraction of the frame width.", 0.0, 1.0, 0.05, 0.001),
    color("fill", "fill", "The inside.", [255, 255, 255, 255]),
    color("stroke", "stroke", "The outline.", [0, 0, 0, 255]),
    float("stroke_width", "stroke width", "The outline's width in px, inside the shape (0 = none).", 0.0, 100.0, 3.0, 0.05),
    float("spikes", "bumps / spikes", "The cloud's bumps or the burst's spikes.", 3.0, 64.0, 14.0, 1.0),
    float("seed", "seed", "Picks the burst's spike lengths; one seed draws the same way every render.", 0.0, 65535.0, 1.0, 1.0),
];

impl GenKind {
    pub const ALL: &'static [GenKind] = &[
        GenKind::Particles,
        GenKind::SpeedLines,
        GenKind::FocusLines,
        GenKind::Text,
        GenKind::Balloon,
    ];

    /// The node's name in the graph and the add-node menu.
    pub fn name(self) -> &'static str {
        match self {
            Self::Particles => "Particles",
            Self::SpeedLines => "Speed lines",
            Self::FocusLines => "Focus lines",
            Self::Text => "Text",
            Self::Balloon => "Balloon",
        }
    }

    /// The studio's own word (romaji), shown beside the name.
    pub fn term(self) -> Option<&'static str> {
        match self {
            Self::Particles => None,
            Self::SpeedLines => Some("ryūsen"),
            Self::FocusLines => Some("shūchūsen"),
            // Set type for dialogue (kakimoji is the hand-drawn kind).
            Self::Text => Some("shashoku"),
            Self::Balloon => Some("fukidashi"),
        }
    }

    /// One plain sentence for menus and tooltips.
    pub fn help(self) -> &'static str {
        match self {
            Self::Particles => "Particles from an emitter: sparks, snow, rain, dust, petals, embers. The same on every render, frame by frame.",
            Self::SpeedLines => "Speed lines (ryūsen) along one direction, tapered at both ends, with an optional clear band.",
            Self::FocusLines => "Focus lines (shūchūsen) from beyond the frame toward a centre, stopping at a clear ellipse.",
            Self::Text => "Set lettering (shashoku) for dialogue and captions, horizontal or vertical (tategaki), with an outline; put it over a balloon to letter it.",
            Self::Balloon => "A speech balloon (fukidashi): ellipse, rounded rect, cloud or burst, with a tail to a point.",
        }
    }

    /// Search words for the add-node menu.
    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Particles => &["particle", "emitter", "sparks", "snow", "rain", "dust", "sakura", "petals", "embers", "generator"],
            Self::SpeedLines => &["speed", "ryusen", "ryūsen", "motion lines", "action lines", "manga", "generator"],
            Self::FocusLines => &["focus", "shuchusen", "shūchūsen", "radial", "concentration lines", "zoom lines", "manga", "generator"],
            Self::Text => &[
                "lettering", "shashoku", "serifu", "dialogue", "telop", "caption", "subtitle", "type", "title",
                "tategaki", "vertical", "words", "kakimoji", "generator",
            ],
            Self::Balloon => &["fukidashi", "speech", "bubble", "word balloon", "shout", "thought", "manga", "generator"],
        }
    }

    /// Input pins: Particles take an optional matte (the matte emitter's
    /// region); the others draw from their parameters alone.
    pub fn inputs(self) -> usize {
        self.input_names().len()
    }

    pub fn input_names(self) -> &'static [&'static str] {
        match self {
            Self::Particles => &["matte"],
            _ => &[],
        }
    }

    pub fn input_help(self) -> &'static [&'static str] {
        match self {
            Self::Particles => &["optional; with the matte emitter, particles are born where this is opaque"],
            _ => &[],
        }
    }

    pub fn specs(self) -> &'static [ParamSpec] {
        match self {
            Self::Particles => PARTICLE_SPECS,
            Self::SpeedLines => SPEED_LINE_SPECS,
            Self::FocusLines => FOCUS_LINE_SPECS,
            Self::Text => TEXT_SPECS,
            Self::Balloon => BALLOON_SPECS,
        }
    }

    pub fn spec(self, key: &str) -> Option<&'static ParamSpec> {
        self.specs().iter().find(|s| s.key == key)
    }

    /// Every spec written out at its default (a new node).
    pub fn defaults(self) -> FxParams {
        FxParams(
            self.specs()
                .iter()
                .map(|s| (s.key.to_string(), s.default_value()))
                .collect(),
        )
    }
}

/// A Generator's parameters resolved at one frame, in spec order (bound
/// columns sampled there) — what [`render`] draws and the eval hash spells.
#[derive(Debug, Clone, PartialEq)]
pub struct GenValues {
    pub kind: GenKind,
    pub values: Vec<FxValue>,
}

impl GenValues {
    /// A bound column with a value at the frame wins, else the stored
    /// value, else the spec default (the Effect's law, [`fx::resolve_specs`]).
    pub fn resolve(
        kind: GenKind,
        params: &FxParams,
        binds: &FxBinds,
        column: impl Fn(ParamId) -> Option<f32>,
    ) -> Self {
        Self {
            kind,
            values: fx::resolve_specs(kind.specs(), params, binds, column),
        }
    }

    /// The stored values alone (no binds).
    pub fn of(kind: GenKind, params: &FxParams) -> Self {
        Self::resolve(kind, params, &FxBinds::default(), |_| None)
    }

    pub fn defaults(kind: GenKind) -> Self {
        Self::of(kind, &FxParams::default())
    }

    fn value(&self, key: &str) -> &FxValue {
        let i = self
            .kind
            .specs()
            .iter()
            .position(|s| s.key == key)
            .unwrap_or_else(|| panic!("{} has no parameter '{key}'", self.kind.name()));
        &self.values[i]
    }

    pub fn float(&self, key: &str) -> f32 {
        match self.value(key) {
            FxValue::Float(x) => *x,
            v => panic!("'{key}' is not a number: {v:?}"),
        }
    }

    pub fn color(&self, key: &str) -> [u8; 4] {
        match self.value(key) {
            FxValue::Color(c) => *c,
            v => panic!("'{key}' is not a colour: {v:?}"),
        }
    }

    pub fn flag(&self, key: &str) -> bool {
        match self.value(key) {
            FxValue::Bool(b) => *b,
            v => panic!("'{key}' is not a flag: {v:?}"),
        }
    }

    /// A choice's index into its spec's options.
    pub fn choice(&self, key: &str) -> usize {
        let (FxValue::Choice(s), Some(spec)) = (self.value(key), self.kind.spec(key)) else {
            panic!("'{key}' is not a choice");
        };
        match spec.kind {
            ParamKind::Choice { options, default, .. } => options.iter().position(|o| o == s).unwrap_or(default),
            _ => 0,
        }
    }

    pub fn text(&self, key: &str) -> &str {
        match self.value(key) {
            FxValue::Text { text } => text,
            v => panic!("'{key}' is not a text: {v:?}"),
        }
    }

    fn seed(&self) -> u32 {
        self.float("seed").round().max(0.0) as u32
    }

    /// The frame the picture changes with: every frame for particles, the
    /// motion's frame for lines (None held; on twos, f/2; on threes, f/3),
    /// None for text and balloons — the eval recipe spells it, so a still
    /// generator cache-hits across frames.
    pub fn motion(&self, frame: u32) -> Option<u32> {
        match self.kind {
            GenKind::Particles => Some(frame),
            GenKind::SpeedLines | GenKind::FocusLines => match self.choice("motion") {
                1 => Some(frame),
                2 => Some(frame / 2),
                3 => Some(frame / 3),
                _ => None,
            },
            GenKind::Text | GenKind::Balloon => None,
        }
    }

    /// `key=value,...` in spec order: the eval hash's part of the recipe.
    pub fn recipe(&self) -> String {
        self.kind
            .specs()
            .iter()
            .zip(&self.values)
            .map(|(s, v)| format!("{}={v}", s.key))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Where a generator draws: `rect` (x, y, w, h) in the frame's px of a
/// `fw × fh` frame — the frame itself, or a camera plane's paper around it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub fw: u32,
    pub fh: u32,
}

impl Target {
    pub fn frame(w: u32, h: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            w,
            h,
            fw: w,
            fh: h,
        }
    }
}

/// Draw the generator at `frame` into `t` (premultiplied f32). `matte` is
/// the Particles' matte input rendered over the same target.
pub fn render(v: &GenValues, law: ColorLaw, matte: Option<&Image>, frame: u32, t: Target) -> Image {
    match v.kind {
        GenKind::Particles => {
            let ps = particles_in(v, law, frame, t.fw, t.fh, matte.map(|m| (m, t.x, t.y)));
            draw_particles(&ps, v.choice("shape"), v.choice("blend") == 1, t)
        }
        GenKind::SpeedLines => {
            let lines = speed_lines(v, frame, t.fw, t.fh);
            draw_strokes(&lines, law.premultiplied(v.color("color")), t, None)
        }
        GenKind::FocusLines => {
            let lines = focus_lines(v, frame, t.fw, t.fh);
            let clear = clear_ellipse(v, t.fw, t.fh);
            draw_strokes(&lines, law.premultiplied(v.color("color")), t, Some(clear))
        }
        GenKind::Text => lettering(v, law, t),
        GenKind::Balloon => balloon(v, law, t),
    }
}

// ---- Seeded randomness -------------------------------------------------------------

/// splitmix64's finaliser over (seed, index, stream): every random choice
/// is a pure function of where it is used, never of a running state.
fn hash(seed: u32, i: u64, k: u32) -> u64 {
    let mut x = ((seed as u64) << 32 | k as u64) ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// A seeded uniform in [0, 1).
fn rnd(seed: u32, i: u64, k: u32) -> f32 {
    (hash(seed, i, k) >> 40) as f32 / (1u64 << 24) as f32
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// The lines' layout seed at `frame` under their motion choice.
fn motion_seed(v: &GenValues, frame: u32) -> u32 {
    match v.motion(frame) {
        None => v.seed(),
        Some(f) => (hash(v.seed(), f as u64, 0xF0F0) >> 32) as u32,
    }
}

// ---- Particles ---------------------------------------------------------------------

/// One live particle at a frame (frame px, y down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    /// Its birth index.
    pub index: u64,
    pub x: f32,
    pub y: f32,
    /// Its velocity now, px per frame.
    pub vx: f32,
    pub vy: f32,
    pub age: f32,
    pub life: f32,
    pub size: f32,
    /// Straight stored colour (the chips through the colour law, mixed).
    pub rgb: [f32; 3],
    pub opacity: f32,
    /// Its turn in radians (petals and snowflakes spin).
    pub turn: f32,
}

/// The particles alive at `frame` (pure: the seed, the birth index and the
/// frame decide everything; see the module law), in birth order. `matte`
/// is frame-sized; with the matte emitter and no matte nothing is born.
pub fn particles_at(
    v: &GenValues,
    law: ColorLaw,
    frame: u32,
    fw: u32,
    fh: u32,
    matte: Option<&Image>,
) -> Vec<Particle> {
    particles_in(v, law, frame, fw, fh, matte.map(|m| (m, 0, 0)))
}

/// [`particles_at`] with the matte given over a rect starting at (x, y) of
/// the frame.
fn particles_in(
    v: &GenValues,
    law: ColorLaw,
    frame: u32,
    fw: u32,
    fh: u32,
    matte: Option<(&Image, i32, i32)>,
) -> Vec<Particle> {
    let rate = v.float("rate");
    if rate <= 0.0 {
        return Vec::new();
    }
    let (life, var, pre) = (v.float("lifetime"), v.float("lifetime_var"), v.float("preroll"));
    let f = frame as f32;
    let hi = ((f + pre) * rate).floor();
    if hi < 0.0 {
        return Vec::new();
    }
    let longest = life * (1.0 + var);
    let lo = (((f + pre - longest) * rate).floor() + 1.0).max(0.0);
    let (hi, mut lo) = (hi as u64, lo as u64);
    if hi + 1 - lo.min(hi + 1) > MAX_PARTICLES {
        lo = hi + 1 - MAX_PARTICLES;
    }
    let seed = v.seed();
    let (fwf, fhf) = (fw as f32, fh as f32);
    let (ex, ey) = (v.float("x") * fwf, v.float("y") * fhf);
    let (ew, eh) = (v.float("width") * fwf, v.float("height") * fhf);
    let emitter = v.choice("emitter");
    let (dir, spread) = (v.float("direction"), v.float("spread"));
    let (speed, speed_var) = (v.float("speed"), v.float("speed_var"));
    let (gx, gy) = (v.float("wind"), v.float("gravity"));
    let turb = v.float("turbulence");
    let (s0, s1) = (v.float("size"), v.float("size_end"));
    let (c0, c1) = (law.encode_rgba(v.color("color")), law.encode_rgba(v.color("color_end")));
    let (o0, o1) = (v.float("opacity"), v.float("opacity_end"));
    let alpha_at = |x: f32, y: f32| -> f32 {
        let Some((m, mx, my)) = matte else {
            return 0.0;
        };
        let (ix, iy) = ((x.floor() as i64) - mx as i64, (y.floor() as i64) - my as i64);
        if ix < 0 || iy < 0 || ix >= m.w as i64 || iy >= m.h as i64 {
            return 0.0;
        }
        m.pixel(ix as u32, iy as u32)[3]
    };
    (lo..=hi)
        .into_par_iter()
        .filter_map(|i| {
            let r = |k: u32| rnd(seed, i, k);
            let born = i as f32 / rate - pre;
            let age = f - born;
            let l = (life * (1.0 + var * (2.0 * r(0) - 1.0))).max(1e-3);
            if !(0.0..l).contains(&age) {
                return None;
            }
            let (px, py) = match emitter {
                1 => (ex + (r(1) - 0.5) * ew, ey),
                2 => (ex + (r(1) - 0.5) * ew, ey + (r(2) - 0.5) * eh),
                3 => (0..8u32).find_map(|j| {
                    let (x, y) = (r(20 + 2 * j) * fwf, r(21 + 2 * j) * fhf);
                    (alpha_at(x, y) >= 0.5).then_some((x, y))
                })?,
                _ => (ex, ey),
            };
            let th = (dir + spread * (r(3) - 0.5)).to_radians();
            let sp = speed * (1.0 + speed_var * (2.0 * r(4) - 1.0));
            let (vx0, vy0) = (sp * th.cos(), sp * th.sin());
            // The wobble: two seeded sines per axis, zero at birth.
            let (mut wx, mut wy, mut dwx, mut dwy) = (0.0, 0.0, 0.0, 0.0);
            if turb > 0.0 {
                for (j, (pos, vel)) in [(&mut wx, &mut dwx), (&mut wy, &mut dwy)].into_iter().enumerate() {
                    let k = 5 + 4 * j as u32;
                    let (w1, w2) = (0.04 + 0.16 * r(k), 0.1 + 0.3 * r(k + 1));
                    let (p1, p2) = (2.0 * PI * r(k + 2), 2.0 * PI * r(k + 3));
                    *pos = turb * ((w1 * age + p1).sin() - p1.sin() + 0.5 * ((w2 * age + p2).sin() - p2.sin())) / 1.5;
                    *vel = turb * (w1 * (w1 * age + p1).cos() + 0.5 * w2 * (w2 * age + p2).cos()) / 1.5;
                }
            }
            let s = age / l;
            let c = [mix(c0[0], c1[0], s), mix(c0[1], c1[1], s), mix(c0[2], c1[2], s)];
            let opacity = mix(o0, o1, s).clamp(0.0, 1.0) * mix(c0[3], c1[3], s);
            let spin = (r(14) - 0.5) * 0.2;
            Some(Particle {
                index: i,
                x: px + vx0 * age + 0.5 * gx * age * age + wx,
                y: py + vy0 * age + 0.5 * gy * age * age + wy,
                vx: vx0 + gx * age + dwx,
                vy: vy0 + gy * age + dwy,
                age,
                life: l,
                size: mix(s0, s1, s).max(0.0),
                rgb: c,
                opacity,
                turn: 2.0 * PI * r(13) + spin * age,
            })
        })
        .collect()
}

/// Distance from `p` to the segment a→b, and where along it (0–1) the
/// nearest point lies.
fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let s = if len2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (qx, qy) = (a.0 + s * dx - p.0, a.1 + s * dy - p.1);
    ((qx * qx + qy * qy).sqrt(), s)
}

/// A line of half-width `hw` at distance `d`: the edge anti-aliased over a
/// pixel; thinner than a pixel, it fades by its width instead.
fn line_cov(hw: f32, d: f32) -> f32 {
    if hw >= 0.5 {
        (hw + 0.5 - d).clamp(0.0, 1.0)
    } else {
        (1.0 - d).clamp(0.0, 1.0) * 2.0 * hw
    }
}

/// How far a particle's picture reaches from its centre, px.
fn particle_reach(p: &Particle, shape: usize) -> f32 {
    let r = (p.size / 2.0).max(0.5);
    match shape {
        1 => r + (p.vx * p.vx + p.vy * p.vy).sqrt().max(p.size) + 1.0,
        _ => r + 1.5,
    }
}

/// One particle's coverage at the point (frame px).
fn particle_cov(p: &Particle, shape: usize, x: f32, y: f32) -> f32 {
    let (dx, dy) = (x - p.x, y - p.y);
    let r = (p.size / 2.0).max(0.5);
    match shape {
        // Streak: from the particle back along its motion, fading to the tail.
        1 => {
            let speed = (p.vx * p.vx + p.vy * p.vy).sqrt();
            let len = speed.max(p.size);
            let (ux, uy) = if speed > 1e-6 { (p.vx / speed, p.vy / speed) } else { (1.0, 0.0) };
            let tail = (p.x - ux * len, p.y - uy * len);
            let (d, s) = seg_dist((x, y), (p.x, p.y), tail);
            line_cov(p.size / 2.0, d) * (1.0 - s)
        }
        // Soft disc: (1 − (d/r)²)².
        2 => {
            let q = (dx * dx + dy * dy) / (r * r);
            if q < 1.0 { (1.0 - q) * (1.0 - q) } else { 0.0 }
        }
        // Petal: an ellipse twice as long as wide, turned.
        3 => {
            let (c, s) = (p.turn.cos(), p.turn.sin());
            let (u, w) = (c * dx + s * dy, -s * dx + c * dy);
            let (rx, ry) = (r, (r * 0.5).max(0.5));
            let q = ((u / rx).powi(2) + (w / ry).powi(2)).sqrt();
            (0.5 - (q - 1.0) * ry).clamp(0.0, 1.0)
        }
        // Snowflake: three bars through the centre, 60° apart, turned.
        4 => {
            let hw = (p.size / 12.0).max(0.35);
            let d = (0..3)
                .map(|k| {
                    let a = p.turn + k as f32 * PI / 3.0;
                    let (ux, uy) = (a.cos() * r, a.sin() * r);
                    seg_dist((x, y), (p.x - ux, p.y - uy), (p.x + ux, p.y + uy)).0
                })
                .fold(f32::INFINITY, f32::min);
            line_cov(hw, d).max(line_cov(r * 0.25, (dx * dx + dy * dy).sqrt()))
        }
        // Dot: a hard disc, a pixel of anti-aliasing.
        _ => (r + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0) * p.size.min(1.0),
    }
}

/// Rows per band when particles are binned for drawing.
const BAND: usize = 16;

fn draw_particles(ps: &[Particle], shape: usize, add: bool, t: Target) -> Image {
    let mut img = Image::new(t.w, t.h);
    if ps.is_empty() || t.w == 0 || t.h == 0 {
        return img;
    }
    let (w, h) = (t.w as usize, t.h as usize);
    let bands = h.div_ceil(BAND);
    // Bin by band in birth order, so every pixel composites in that order.
    let mut bins: Vec<Vec<usize>> = vec![Vec::new(); bands];
    for (n, p) in ps.iter().enumerate() {
        if p.opacity <= 0.0 {
            continue;
        }
        let reach = particle_reach(p, shape);
        let y0 = (p.y - reach - t.y as f32).floor().max(0.0) as usize;
        let y1 = (p.y + reach - t.y as f32).ceil();
        if y1 < 0.0 || y0 >= h {
            continue;
        }
        let y1 = (y1 as usize).min(h - 1);
        for b in (y0 / BAND)..=(y1 / BAND) {
            bins[b].push(n);
        }
    }
    img.px
        .par_chunks_mut(w * 4 * BAND)
        .zip(bins.par_iter())
        .enumerate()
        .for_each(|(b, (rows, bin))| {
            for &n in bin {
                let p = &ps[n];
                let reach = particle_reach(p, shape);
                let (px, py) = (p.x - t.x as f32, p.y - t.y as f32);
                let x0 = (px - reach).floor().max(0.0) as usize;
                let x1 = (px + reach).ceil().min(w as f32 - 1.0);
                if x1 < 0.0 {
                    continue;
                }
                let x1 = x1 as usize;
                let row0 = b * BAND;
                for (ry, row) in rows.chunks_exact_mut(w * 4).enumerate() {
                    let y = row0 + ry;
                    if (y as f32 + 0.5 - py).abs() > reach {
                        continue;
                    }
                    for x in x0..=x1 {
                        let fx = t.x as f32 + x as f32 + 0.5;
                        let fy = t.y as f32 + y as f32 + 0.5;
                        let a = particle_cov(p, shape, fx, fy) * p.opacity;
                        if a <= 0.0 {
                            continue;
                        }
                        let d = &mut row[x * 4..x * 4 + 4];
                        if add {
                            for k in 0..3 {
                                d[k] += p.rgb[k] * a;
                            }
                            d[3] = d[3] + a - d[3] * a;
                        } else {
                            let keep = 1.0 - a;
                            for k in 0..3 {
                                d[k] = p.rgb[k] * a + d[k] * keep;
                            }
                            d[3] = a + d[3] * keep;
                        }
                    }
                }
            }
        });
    img
}

// ---- Presets -------------------------------------------------------------------------

/// The particle presets: a Particles node's parameters set for a look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Sparks,
    Snow,
    Rain,
    Dust,
    Sakura,
    Embers,
}

impl Preset {
    pub const ALL: &'static [Preset] = &[
        Preset::Sparks,
        Preset::Snow,
        Preset::Rain,
        Preset::Dust,
        Preset::Sakura,
        Preset::Embers,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Sparks => "Sparks",
            Self::Snow => "Snow",
            Self::Rain => "Rain",
            Self::Dust => "Dust",
            Self::Sakura => "Sakura petals",
            Self::Embers => "Embers",
        }
    }

    /// The studio's word (romaji).
    pub fn term(self) -> &'static str {
        match self {
            Self::Sparks => "hibana",
            Self::Snow => "yuki",
            Self::Rain => "ame",
            Self::Dust => "hokori",
            Self::Sakura => "sakura",
            Self::Embers => "hinoko",
        }
    }

    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Self::Sparks => &["hibana", "spark", "particles", "impact", "fire"],
            Self::Snow => &["yuki", "snowfall", "particles", "winter"],
            Self::Rain => &["ame", "rainfall", "drops", "particles", "weather"],
            Self::Dust => &["hokori", "motes", "particles", "floating"],
            Self::Sakura => &["sakura", "cherry blossom", "petals", "hanabira", "particles"],
            Self::Embers => &["hinoko", "ember", "ash", "fire", "particles"],
        }
    }

    /// The Particles parameters of this look (tuned at 1920×1080).
    pub fn params(self) -> FxParams {
        let mut p = GenKind::Particles.defaults();
        let mut set = |k: &str, v: FxValue| p.set(k, v);
        let n = FxValue::Float;
        let pick = |s: &str| FxValue::Choice(s.to_string());
        match self {
            Self::Sparks => {
                for (k, x) in [("x", 0.5), ("y", 0.6), ("rate", 30.0), ("lifetime", 18.0), ("lifetime_var", 0.4), ("speed", 14.0), ("speed_var", 0.5), ("direction", -90.0), ("spread", 120.0), ("gravity", 0.6), ("size", 3.0), ("size_end", 1.0), ("opacity", 1.0), ("opacity_end", 0.0)] {
                    set(k, n(x));
                }
                set("color", FxValue::Color([255, 228, 150, 255]));
                set("color_end", FxValue::Color([255, 96, 24, 255]));
                set("shape", pick("streak"));
                set("blend", pick("add"));
            }
            Self::Snow => {
                for (k, x) in [("x", 0.5), ("y", -0.05), ("width", 1.3), ("rate", 6.0), ("lifetime", 240.0), ("lifetime_var", 0.2), ("preroll", 240.0), ("speed", 2.0), ("speed_var", 0.4), ("direction", 90.0), ("spread", 30.0), ("gravity", 0.0), ("wind", 0.002), ("turbulence", 14.0), ("size", 6.0), ("size_end", 6.0), ("opacity", 0.95), ("opacity_end", 0.85)] {
                    set(k, n(x));
                }
                set("emitter", pick("line"));
                set("shape", pick("snowflake"));
            }
            Self::Rain => {
                for (k, x) in [("x", 0.5), ("y", -0.05), ("width", 1.4), ("rate", 40.0), ("lifetime", 30.0), ("lifetime_var", 0.1), ("preroll", 30.0), ("speed", 45.0), ("speed_var", 0.15), ("direction", 100.0), ("spread", 2.0), ("gravity", 0.0), ("size", 1.5), ("size_end", 1.5), ("opacity", 0.55), ("opacity_end", 0.55)] {
                    set(k, n(x));
                }
                set("emitter", pick("line"));
                set("color", FxValue::Color([205, 214, 232, 255]));
                set("color_end", FxValue::Color([205, 214, 232, 255]));
                set("shape", pick("streak"));
            }
            Self::Dust => {
                for (k, x) in [("x", 0.5), ("y", 0.5), ("width", 1.0), ("height", 1.0), ("rate", 3.0), ("lifetime", 120.0), ("lifetime_var", 0.5), ("preroll", 120.0), ("speed", 0.3), ("speed_var", 0.5), ("spread", 360.0), ("gravity", 0.0), ("turbulence", 10.0), ("size", 3.0), ("size_end", 2.0), ("opacity", 0.6), ("opacity_end", 0.0)] {
                    set(k, n(x));
                }
                set("emitter", pick("rect"));
                set("color", FxValue::Color([255, 240, 214, 255]));
                set("color_end", FxValue::Color([255, 240, 214, 255]));
                set("blend", pick("add"));
            }
            Self::Sakura => {
                for (k, x) in [("x", 0.5), ("y", -0.05), ("width", 1.3), ("rate", 2.0), ("lifetime", 300.0), ("lifetime_var", 0.2), ("preroll", 300.0), ("speed", 1.6), ("speed_var", 0.3), ("direction", 75.0), ("spread", 40.0), ("gravity", 0.002), ("wind", 0.004), ("turbulence", 18.0), ("size", 16.0), ("size_end", 14.0), ("opacity", 1.0), ("opacity_end", 0.9)] {
                    set(k, n(x));
                }
                set("emitter", pick("line"));
                set("color", FxValue::Color([255, 183, 197, 255]));
                set("color_end", FxValue::Color([255, 208, 218, 255]));
                set("shape", pick("petal"));
            }
            Self::Embers => {
                for (k, x) in [("x", 0.5), ("y", 1.0), ("width", 1.0), ("height", 0.1), ("rate", 4.0), ("lifetime", 90.0), ("lifetime_var", 0.3), ("preroll", 90.0), ("speed", 2.0), ("speed_var", 0.4), ("direction", -90.0), ("spread", 40.0), ("gravity", -0.02), ("turbulence", 12.0), ("size", 4.0), ("size_end", 1.0), ("opacity", 1.0), ("opacity_end", 0.0)] {
                    set(k, n(x));
                }
                set("emitter", pick("rect"));
                set("color", FxValue::Color([255, 170, 70, 255]));
                set("color_end", FxValue::Color([255, 64, 0, 255]));
                set("blend", pick("add"));
            }
        }
        p
    }
}

// ---- Lines -----------------------------------------------------------------------------

/// How a stroke narrows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taper {
    /// To a point at both ends, widest in the middle (speed lines).
    Both,
    /// Widest at `a`, a point at `b` (focus lines, toward the centre).
    ToEnd,
}

/// One line: from `a` to `b` (frame px), `width` px at its widest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub a: (f32, f32),
    pub b: (f32, f32),
    pub width: f32,
    pub taper: Taper,
}

impl Stroke {
    /// The width at `s` along it (0 at `a`, 1 at `b`).
    pub fn width_at(&self, s: f32) -> f32 {
        let s = s.clamp(0.0, 1.0);
        match self.taper {
            Taper::Both => self.width * (4.0 * s * (1.0 - s)).sqrt(),
            Taper::ToEnd => self.width * (1.0 - s),
        }
    }

    /// Its direction in degrees, a to b.
    pub fn angle_deg(&self) -> f32 {
        (self.b.1 - self.a.1).atan2(self.b.0 - self.a.0).to_degrees()
    }
}

/// The speed lines at `frame` of a `fw × fh` frame (see the module law).
pub fn speed_lines(v: &GenValues, frame: u32, fw: u32, fh: u32) -> Vec<Stroke> {
    let n = v.float("count").round().max(0.0) as u64;
    let seed = motion_seed(v, frame);
    let th = v.float("angle").to_radians();
    let (dx, dy) = (th.cos(), th.sin());
    let (nx, ny) = (-dy, dx);
    let (fwf, fhf) = (fw as f32, fh as f32);
    let (cx, cy) = (fwf / 2.0, fhf / 2.0);
    // The frame's extents across and along the lines (its projections).
    let across = fwf * nx.abs() + fhf * ny.abs();
    let along = fwf * dx.abs() + fhf * dy.abs();
    let (lmin, lmax) = (v.float("length_min"), v.float("length_max"));
    let (tmin, tmax) = (v.float("thickness_min"), v.float("thickness_max"));
    // The clear band, widened by half the widest line so no ink enters it.
    let band = v.float("band") * across;
    let (lo, skip) = if band > 0.0 {
        let skip = (band + tmax.max(tmin) + 2.0).min(across);
        let mid = v.float("band_offset") * across / 2.0;
        let lo = (mid - skip / 2.0).clamp(-across / 2.0, across / 2.0 - skip);
        (lo, skip)
    } else {
        (0.0, 0.0)
    };
    (0..n)
        .map(|i| {
            let r = |k: u32| rnd(seed, i, k);
            let mut o = -across / 2.0 + r(0) * (across - skip);
            if skip > 0.0 && o >= lo {
                o += skip;
            }
            let len = mix(lmin, lmax, r(1)) * along;
            let mid = (r(2) - 0.5) * (along + len);
            let width = mix(tmin, tmax, r(3));
            let (mx, my) = (cx + nx * o + dx * mid, cy + ny * o + dy * mid);
            Stroke {
                a: (mx - dx * len / 2.0, my - dy * len / 2.0),
                b: (mx + dx * len / 2.0, my + dy * len / 2.0),
                width,
                taper: Taper::Both,
            }
        })
        .collect()
}

/// The focus lines' clear ellipse: centre and radii in frame px.
pub fn clear_ellipse(v: &GenValues, fw: u32, fh: u32) -> (f32, f32, f32, f32) {
    let (fwf, fhf) = (fw as f32, fh as f32);
    (
        v.float("x") * fwf,
        v.float("y") * fhf,
        (v.float("rx") * fwf).max(1e-3),
        (v.float("ry") * fhf).max(1e-3),
    )
}

/// The focus lines at `frame` of a `fw × fh` frame (see the module law).
pub fn focus_lines(v: &GenValues, frame: u32, fw: u32, fh: u32) -> Vec<Stroke> {
    let n = v.float("count").round().max(0.0) as u64;
    let seed = motion_seed(v, frame);
    let (cx, cy, rx, ry) = clear_ellipse(v, fw, fh);
    let (tmin, tmax) = (v.float("thickness_min"), v.float("thickness_max"));
    let jitter = v.float("jitter");
    // From beyond the farthest corner, so every line enters from outside.
    let reach = [(0.0, 0.0), (fw as f32, 0.0), (0.0, fh as f32), (fw as f32, fh as f32)]
        .iter()
        .map(|(x, y)| ((x - cx).powi(2) + (y - cy).powi(2)).sqrt())
        .fold(0.0f32, f32::max)
        + tmax.max(tmin)
        + 2.0;
    (0..n)
        .map(|i| {
            let r = |k: u32| rnd(seed, i, k);
            let th = (i as f32 + 0.8 * (r(0) - 0.5)) / n as f32 * 2.0 * PI;
            let (ux, uy) = (th.cos(), th.sin());
            let edge = 1.0 / ((ux / rx).powi(2) + (uy / ry).powi(2)).sqrt();
            let inner = edge * (1.0 + jitter * r(1));
            Stroke {
                a: (cx + ux * reach, cy + uy * reach),
                b: (cx + ux * inner, cy + uy * inner),
                width: mix(tmin, tmax, r(2)),
                taper: Taper::ToEnd,
            }
        })
        .collect()
}

/// Draw `strokes` in one premultiplied colour (their coverage unions);
/// nothing inks inside `clear` (centre and radii, frame px).
fn draw_strokes(strokes: &[Stroke], rgba: [f32; 4], t: Target, clear: Option<(f32, f32, f32, f32)>) -> Image {
    let mut img = Image::new(t.w, t.h);
    let w = t.w as usize;
    if w == 0 || strokes.is_empty() {
        return img;
    }
    img.px.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let fy = t.y as f32 + y as f32 + 0.5;
        let mut cov = vec![0.0f32; w];
        for st in strokes {
            let reach = st.width / 2.0 + 1.0;
            let (a, b) = (st.a, st.b);
            if fy < a.1.min(b.1) - reach || fy > a.1.max(b.1) + reach {
                continue;
            }
            // The stretch of the line within reach of this row, in x.
            let dy = b.1 - a.1;
            let (s0, s1) = if dy.abs() < 1e-6 {
                (0.0, 1.0)
            } else {
                let p = ((fy - reach - a.1) / dy).clamp(0.0, 1.0);
                let q = ((fy + reach - a.1) / dy).clamp(0.0, 1.0);
                (p.min(q), p.max(q))
            };
            let (xa, xb) = (a.0 + s0 * (b.0 - a.0), a.0 + s1 * (b.0 - a.0));
            let x0 = (xa.min(xb) - reach - t.x as f32).floor().max(0.0) as usize;
            let x1 = (xa.max(xb) + reach - t.x as f32).ceil();
            if x1 < 0.0 {
                continue;
            }
            let x1 = (x1 as usize).min(w - 1);
            for (x, c) in cov.iter_mut().enumerate().take(x1 + 1).skip(x0) {
                let fx = t.x as f32 + x as f32 + 0.5;
                let (d, s) = seg_dist((fx, fy), a, b);
                let k = line_cov(st.width_at(s) / 2.0, d);
                if k > 0.0 {
                    *c = *c + k - *c * k;
                }
            }
        }
        for (x, c) in cov.iter().enumerate() {
            let mut c = *c;
            if let Some((ex, ey, rx, ry)) = clear {
                let fx = t.x as f32 + x as f32 + 0.5;
                if ((fx - ex) / rx).powi(2) + ((fy - ey) / ry).powi(2) < 1.0 {
                    c = 0.0;
                }
            }
            if c > 0.0 {
                for k in 0..4 {
                    row[x * 4 + k] = rgba[k] * c;
                }
            }
        }
    });
    img
}

// ---- Balloons --------------------------------------------------------------------------

/// The approximate signed distance to an ellipse of radii (a, b) about the
/// origin (inside negative).
fn sd_ellipse(p: (f32, f32), a: f32, b: f32) -> f32 {
    let k0 = ((p.0 / a).powi(2) + (p.1 / b).powi(2)).sqrt();
    let k1 = ((p.0 / (a * a)).powi(2) + (p.1 / (b * b)).powi(2)).sqrt();
    if k1 <= 0.0 {
        return -a.min(b);
    }
    k0 * (k0 - 1.0) / k1
}

/// The signed distance to a box of half-size (bx, by) with corners rounded
/// by `r`, about the origin.
fn sd_round_box(p: (f32, f32), bx: f32, by: f32, r: f32) -> f32 {
    let (qx, qy) = (p.0.abs() - bx + r, p.1.abs() - by + r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// The signed distance to a simple polygon (inside negative).
fn sd_polygon(p: (f32, f32), v: &[(f32, f32)]) -> f32 {
    let mut d = f32::INFINITY;
    let mut inside = false;
    let mut j = v.len() - 1;
    for i in 0..v.len() {
        let (a, b) = (v[i], v[j]);
        d = d.min(seg_dist(p, a, b).0);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
        j = i;
    }
    if inside { -d } else { d }
}

/// The balloon's body and tail as one signed distance field, frame px.
struct BalloonShape {
    shape: usize,
    c: (f32, f32),
    rx: f32,
    ry: f32,
    /// Cloud bumps (centre, radius) or burst vertices.
    bumps: Vec<(f32, f32)>,
    bump_r: f32,
    tail: Option<[(f32, f32); 3]>,
}

impl BalloonShape {
    fn new(v: &GenValues, fw: u32, fh: u32) -> Self {
        let (fwf, fhf) = (fw as f32, fh as f32);
        let c = (v.float("x") * fwf, v.float("y") * fhf);
        let (rx, ry) = ((v.float("width") * fwf / 2.0).max(1.0), (v.float("height") * fhf / 2.0).max(1.0));
        let shape = v.choice("shape");
        let n = v.float("spikes").round().max(3.0) as usize;
        let seed = v.seed();
        let mut bumps = Vec::new();
        let mut bump_r = 0.0;
        match shape {
            2 => {
                // Cloud: bumps around an ellipse a little inside the body.
                let (a, b) = (rx * 0.82, ry * 0.82);
                let perimeter = PI * (3.0 * (a + b) - ((3.0 * a + b) * (a + 3.0 * b)).sqrt());
                bump_r = perimeter / n as f32 * 0.62;
                bumps = (0..n)
                    .map(|k| {
                        let t = k as f32 / n as f32 * 2.0 * PI;
                        (c.0 + a * t.cos(), c.1 + b * t.sin())
                    })
                    .collect();
            }
            3 => {
                // Burst: spikes out to 1.0–1.35 of the ellipse, valleys at 0.72.
                bumps = (0..2 * n)
                    .map(|k| {
                        let t = k as f32 / (2 * n) as f32 * 2.0 * PI;
                        let s = if k % 2 == 0 { 1.0 + 0.35 * rnd(seed, k as u64, 7) } else { 0.72 };
                        (c.0 + rx * s * t.cos(), c.1 + ry * s * t.sin())
                    })
                    .collect();
            }
            _ => {}
        }
        let tail = v.flag("tail").then(|| {
            let tip = (v.float("tail_x") * fwf, v.float("tail_y") * fhf);
            let (dx, dy) = (tip.0 - c.0, tip.1 - c.1);
            let len = (dx * dx + dy * dy).sqrt().max(1e-3);
            let half = v.float("tail_width") * fwf / 2.0;
            let (nx, ny) = (-dy / len * half, dx / len * half);
            [(c.0 + nx, c.1 + ny), (c.0 - nx, c.1 - ny), tip]
        });
        Self {
            shape,
            c,
            rx,
            ry,
            bumps,
            bump_r,
            tail,
        }
    }

    fn sd(&self, p: (f32, f32)) -> f32 {
        let q = (p.0 - self.c.0, p.1 - self.c.1);
        let body = match self.shape {
            1 => sd_round_box(q, self.rx, self.ry, 0.35 * self.rx.min(self.ry)),
            2 => {
                let core = sd_ellipse(q, self.rx * 0.82, self.ry * 0.82);
                self.bumps
                    .iter()
                    .map(|b| ((p.0 - b.0).powi(2) + (p.1 - b.1).powi(2)).sqrt() - self.bump_r)
                    .fold(core, f32::min)
            }
            3 => sd_polygon(p, &self.bumps),
            _ => sd_ellipse(q, self.rx, self.ry),
        };
        match &self.tail {
            Some(tri) => body.min(sd_polygon(p, tri)),
            None => body,
        }
    }

    /// The frame-px box that holds the whole shape: the body's radii, every
    /// cloud bump's disc and burst spike's tip (both reach past the radii,
    /// the bumps by up to their own radius), and the tail.
    fn bounds(&self) -> (f32, f32, f32, f32) {
        let (mut x0, mut y0) = (self.c.0 - self.rx, self.c.1 - self.ry);
        let (mut x1, mut y1) = (self.c.0 + self.rx, self.c.1 + self.ry);
        // A burst's vertices have no radius (bump_r is 0).
        let r = self.bump_r;
        for b in &self.bumps {
            (x0, y0, x1, y1) = (x0.min(b.0 - r), y0.min(b.1 - r), x1.max(b.0 + r), y1.max(b.1 + r));
        }
        if let Some(tri) = &self.tail {
            for p in tri {
                (x0, y0, x1, y1) = (x0.min(p.0), y0.min(p.1), x1.max(p.0), y1.max(p.1));
            }
        }
        (x0 - 2.0, y0 - 2.0, x1 + 2.0, y1 + 2.0)
    }
}

/// The balloon drawn into `t` (see the module law).
pub fn balloon(v: &GenValues, law: ColorLaw, t: Target) -> Image {
    let shape = BalloonShape::new(v, t.fw, t.fh);
    let (fill, stroke) = (law.premultiplied(v.color("fill")), law.premultiplied(v.color("stroke")));
    let sw = v.float("stroke_width");
    let mut img = Image::new(t.w, t.h);
    let w = t.w as usize;
    if w == 0 {
        return img;
    }
    let (bx0, by0, bx1, by1) = shape.bounds();
    img.px.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let fy = t.y as f32 + y as f32 + 0.5;
        if fy < by0 || fy > by1 {
            return;
        }
        let x0 = (bx0 - t.x as f32).floor().max(0.0) as usize;
        let x1 = (bx1 - t.x as f32).ceil();
        if x1 < 0.0 {
            return;
        }
        for x in x0..=(x1 as usize).min(w - 1) {
            let fx = t.x as f32 + x as f32 + 0.5;
            let d = shape.sd((fx, fy));
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov <= 0.0 {
                continue;
            }
            // 1 within the stroke's width inside the outline, 0 deeper in.
            let edge = if sw > 0.0 { (d + sw + 0.5).clamp(0.0, 1.0) } else { 0.0 };
            for k in 0..4 {
                row[x * 4 + k] = cov * (fill[k] * (1.0 - edge) + stroke[k] * edge);
            }
        }
    });
    img
}

// ---- Text ---------------------------------------------------------------------------------

/// The text's style from its values at a `fw × fh` frame.
pub fn text_style(v: &GenValues, fw: u32, fh: u32) -> Style {
    let direction = if v.choice("direction") == 1 { Direction::Vertical } else { Direction::Horizontal };
    let face = if v.choice("font") == 1 { Face::Sans } else { Face::SansJp };
    let span = match direction {
        Direction::Horizontal => fw as f32,
        Direction::Vertical => fh as f32,
    };
    Style {
        face,
        px: v.float("size"),
        direction,
        leading: v.float("leading"),
        tracking: v.float("tracking"),
        wrap: v.float("wrap") * span,
    }
}

/// The lettering drawn into `t`: the block centred on (x, y), its top-left
/// on whole pixels. Only the target's own pixels are rasterised (and the
/// outline's reach around them), so type far bigger than the target costs
/// no more than the target.
fn lettering(v: &GenValues, law: ColorLaw, t: Target) -> Image {
    let words = v.text("text");
    if words.trim().is_empty() {
        return Image::new(t.w, t.h);
    }
    let block = text::layout(words, &text_style(v, t.fw, t.fh));
    let fill = law.premultiplied(v.color("color"));
    let ring = law.premultiplied(v.color("outline_color"));
    // The block's top-left in the target's px.
    let left = (v.float("x") * t.fw as f32 - block.w / 2.0).round() as i64 - t.x as i64;
    let top = (v.float("y") * t.fh as f32 - block.h / 2.0).round() as i64 - t.y as i64;
    let win = text::Window {
        x: -left,
        y: -top,
        w: t.w,
        h: t.h,
    };
    text::raster_block_in(&block, fill, v.float("outline"), ring, win)
}
