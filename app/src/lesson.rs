//! AU27: the lesson frame (spec 5): a run over a real document, the one reader
//! that ticks its tasks, and the card that carries the sentence. A lesson locks
//! nothing and calls no `enter_room`: it reads state, and the person walks.

use std::path::Path;
use std::sync::Arc;

use anim_core::command::CutRef;
use anim_core::generator::GenKind;
use anim_core::graph::{Graph, NodeKind};
use anim_core::manga::{self, Rulers};
use anim_core::ids::{ColumnId, DrawingId, LayerId, NodeId};
use anim_core::model::{CelLayer, Cut, Drawing, DrawingKind, LayerRole};
use anim_core::raster::TILE;
use anim_core::xsheet::{Column, Exposure, ParamInterp};
use eframe::egui;

use crate::canvas::{self, GuideLine};
use crate::config::BrushPreset;
use crate::doc::{self, AppState, PathInk, drawing_has_art};
use crate::lessons::{self, Class, Done, Latch, Lesson, PathKind, Show, Target, Task};
use crate::plate::{self, Role};
use crate::say::Facts;
use crate::steps;
use crate::workspace::Stage;
use crate::{Action, Editor, RenderState, config, menus, vocab, welcome};

/// Where a run stands, `<id>/<task>`, in `app_meta`: outside undo, and no
/// `meta_dirty`, so an untouched lesson never asks to save.
pub const KEY: &str = "lesson";
/// One finished id a line, in the data dir: a list, not a setting (spec 1.3).
pub const FINISHED_FILE: &str = "lessons.txt";

pub const SHOW_ME: &str = "Show me";
pub const SHOW_WHERE: &str = "Show me where";
pub const HOLD: &str = "Hold to see it";
/// AU28: the card, once a tap the pen route cannot do has been refused.
pub const BY_HAND: &str = "This machine cannot do that by hand. Press Show me.";
pub const LEAVE: &str = "Leave lesson";
pub const NEXT: &str = "Next lesson";
/// The chip's name, in `{Night}`.
pub const NIGHT: &str = "Night";
/// The handle a task's ring names to bring the chip on.
pub const CHIP: &str = "lesson.chip";
/// The finish, before the lesson's own closing line.
pub const MADE: &str = "You made that. It is a real file.";

/// The card's width, spec 5.6.
const CARD_MIN: f32 = 180.0;
const CARD_MAX: f32 = 220.0;
const CARD_PAD: f32 = 8.0;
const CARD_GAP: f32 = 8.0;
/// Between the card's title, strip and path, which read as one heading: the
/// longest sentence at the worst size needs what this and the pad give back.
const HEAD_GAP: f32 = 4.0;
/// A lesson's picture on the card: tall enough for six small ones to read.
const PICTURE_H: f32 = 104.0;
/// The share of the paper a path's box grows by when a stroke is met.
const MEET_GROW: f32 = 0.08;
/// A stroke spans at least this share of the path's longest side.
const MEET_SPAN: f32 = 0.5;
/// Erased: what is left of the smudge.
const ERASED_LEFT: f32 = 0.1;
/// Picked: how far from the asked colour a channel may stand.
const PICK_NEAR: i32 = 8;
/// Show me's hand: the seconds a stroke takes on the paper (spec 5.9), and a
/// press of the pen, which is a touch and no stroke.
pub(crate) const REVEAL_SECS: f64 = 1.5;
pub(crate) const PRESS_SECS: f64 = 0.3;

/// A control's word, one entry each: the `{Token}` table of the lessons, so no
/// lesson string holds a name that a rename would leave behind.
pub(crate) const WORDS: &[(&str, &str)] = &[
    ("Draw", vocab::STEP_DRAW),
    ("Color", vocab::STEP_COLOR),
    ("Effects", vocab::STEP_EFFECTS),
    ("Timing", vocab::STEP_TIMING),
    ("Export", vocab::STEP_EXPORT),
    ("Longer", vocab::LONGER),
    ("Shorter", vocab::SHORTER),
    ("Layers", vocab::LAYERS),
    ("Brushes", vocab::BRUSHES),
    ("Canvas options", vocab::CANVAS_OPTIONS),
    ("Steady stroke", vocab::STEADY_STROKE),
    ("Keep inside strokes", vocab::KEEP_INSIDE_STROKES),
    ("Sketch pencil", vocab::SKETCH_PENCIL),
    ("Clean pencil", vocab::CLEAN_PENCIL),
    ("Brush maker", vocab::BRUSH_MAKER),
    ("Motion curves", vocab::MOTION_CURVES),
    ("Loop", vocab::LOOP),
    ("Loop start", vocab::LOOP_START),
    ("Loop end", vocab::LOOP_END),
    // The rail's faces.
    ("Brush", "Brush"),
    ("Eraser", "Eraser"),
    ("Select", "Select"),
    ("Fill", "Fill"),
    ("Size", "Size"),
    // Each control's own label.
    ("Add drawing", "Add drawing"),
    ("Speed", "Speed"),
    ("Medium", "Medium"),
    ("Play", "Play"),
    ("Undo", "Undo"),
    ("View", "View"),
    ("Mirror view", "Mirror view"),
    ("Lip flap", "Lip flap"),
    ("Custom layer", "Custom layer"),
    ("New page", "New page"),
    ("Panel tool", "Panel tool"),
    ("Export pages", "Export pages"),
    ("Snap to rulers", "Snap to rulers"),
    // The Effects step's.
    ("Glow", "Glow"),
    ("Strength", crate::vocab::STRENGTH),
    ("Add effect", crate::vocab::ADD_EFFECT),
    ("Camera move", crate::vocab::CAMERA_MOVE),
    // The comic's.
    ("Split across", "Split across"),
    ("Split down", "Split down"),
    ("panel 3", "panel 3"),
    ("See the page", vocab::SEE_THE_PAGE),
    ("Speed lines", vocab::SPEED_LINES),
    // This card's.
    ("Night", NIGHT),
    ("Show me", SHOW_ME),
];

/// What `{token}` reads, the run's own words aside.
pub fn word(token: &str) -> Option<&'static str> {
    WORDS.iter().find(|(t, _)| *t == token).map(|(_, w)| *w)
}

/// `text` with its `{tokens}` in words. An unknown token stands as written, so
/// `aulesson_tokens_resolve` has something to find.
pub fn resolve(text: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        let token = &rest[open + 1..open + close];
        out.push_str(&rest[..open]);
        match lookup(token) {
            Some(words) => out.push_str(&words),
            None => out.push_str(&rest[open..=open + close]),
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// `{chord:Name}`'s Action.
pub fn chord_action(token: &str) -> Option<Action> {
    let name = token.strip_prefix("chord:")?;
    Action::ALL.iter().copied().find(|a| format!("{a:?}") == name)
}

/// The card's width in a window this wide (spec 5.6).
pub fn card_width(window_w: f32) -> f32 {
    (window_w * 0.2).clamp(CARD_MIN, CARD_MAX)
}

/// The ids in `lessons.txt`. Missing or unreadable means nothing ticked.
pub fn finished(dir: Option<&Path>) -> Vec<String> {
    let Some(dir) = dir else { return Vec::new() };
    ids_in(&std::fs::read_to_string(dir.join(FINISHED_FILE)).unwrap_or_default())
}

fn ids_in(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// `id` joins `lessons.txt`, once. A write that fails costs a tick, not the
/// person's finish. A file that cannot be read is no empty list: the ids in it
/// are not this write's to lose.
fn write_finished(dir: Option<&Path>, id: &str) {
    let Some(dir) = dir else { return };
    let path = dir.join(FINISHED_FILE);
    let mut ids = match std::fs::read_to_string(&path) {
        Ok(text) => ids_in(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return,
    };
    if ids.iter().any(|have| have == id) {
        return;
    }
    ids.push(id.to_string());
    let _ = config::write_atomic(&path, (ids.join("\n") + "\n").as_bytes());
}

/// What a task is read against: taken when it began.
struct Baseline {
    strokes: u64,
    fills: u64,
    drawings: usize,
    keys: usize,
    moves: u64,
    exported: u32,
    brush: (f32, [u8; 4], Option<String>),
    smoothing: u8,
    gutters: [f32; 2],
    /// The presets of the banks a `Preset` task reads.
    presets: Vec<BrushPreset>,
    /// Each effect's parameters.
    effects: Vec<(NodeId, anim_core::fx::FxParams)>,
    /// The cel under the task: `Erased` and `Filled` read this one by its
    /// ids, so stepping to a blank drawing or another layer erases nothing.
    cel: Option<(DrawingId, LayerId)>,
    /// `Erased`'s inked pixels, by path.
    ink: Vec<(u16, u32)>,
    /// `Filled`'s pixels, seed and keep, on each layer of the drawing: a fill
    /// lands on the armed layer, which may change before it does.
    pixels: Vec<([f32; 2], Vec<(LayerId, [u16; 4])>)>,
    refusals: u32,
}

/// `fill_gap`, the ink and `canvas.raster`, and what a lesson's tasks turn on and
/// off: left on, `Keep inside strokes` or the ruler snap would hold the person's
/// own pen after the run.
struct Borrowed {
    fill_gap: u32,
    ink: [u8; 4],
    raster: bool,
    alpha_lock: bool,
    snap: bool,
}

/// AU28: `Show me`'s lines on their way. Anything that moves what they stand
/// over cancels them: a line landing then would land on something the person
/// never saw it reach for.
struct Reveal {
    paths: &'static [u16],
    began: f64,
    over: Stand,
}

/// AU28: what a reveal stands over: the document, and where in it the ink
/// would land. An entered step changes no revision, and arming a step moves
/// the active layer.
#[derive(PartialEq)]
struct Stand {
    task: usize,
    revision: u64,
    step: Option<Stage>,
    cut: CutRef,
    frame: u32,
    column: ColumnId,
    layer: String,
    /// The paper is on screen.
    paper: bool,
}

/// AU28: what `path` takes on the paper.
fn reveal_secs(path: &lessons::Path) -> f64 {
    if path.is_press() { PRESS_SECS } else { REVEAL_SECS }
}

/// A lesson standing over the open document.
pub(crate) struct LessonRun {
    pub lesson: Lesson,
    art: Arc<Vec<lessons::Path>>,
    /// The current task; `tasks.len()` once finished.
    pub task: usize,
    base: Baseline,
    /// The first reading took the first baseline: the run opens before its
    /// step is entered and its brush armed, and the door sooner still.
    based: bool,
    /// What a run borrows, given back on leaving.
    back: Borrowed,
    /// A hide of Plan's sheet during this run. `plan_sheet_hidden` is the
    /// person's own and stands untouched (spec 3.10).
    pub sheet_off: bool,
    /// `Result beside`, pressed during this run. `result_in_color` is the
    /// person's own and stands untouched, as `plan_sheet_hidden` does.
    pub result_on: bool,
    /// The document this run is over.
    doc: String,
    /// The boxes of the strokes landed since the task began.
    marks: Vec<[f32; 4]>,
    /// The most undo steps since the task began.
    peak: usize,
    /// Files landed since the run began.
    pub exported: u32,
    played: bool,
    /// A pass under way, from the frame it was last at the start of.
    pass: Option<u32>,
    was_playing: bool,
    last_frame: u32,
    mirror_seen: bool,
    composite_seen: bool,
    /// The person pressed the card's button on this task.
    asked: bool,
    /// AU23: this task's fill reached what it was to keep. It stays through the
    /// `Undo` the card's next sentence asks for, until the task is done.
    spilt: bool,
    /// `{rest drawing}`, read when the task opened.
    rest: String,
    picture: welcome::Picture,
    reveal: Option<Reveal>,
    /// `Hold to see it` is down.
    holding: bool,
    /// The task, the hold and the page the guide was last set for.
    guide_for: Option<(usize, bool, usize)>,
}

impl LessonRun {
    pub fn finished(&self) -> bool {
        self.task >= self.lesson.tasks.len()
    }

    fn current(&self) -> Option<&'static Task> {
        self.lesson.tasks.get(self.task)
    }

    /// The guide's lines: the current task's, so the paper clears as you go,
    /// or the finish, full size, while it is held: the finish of `page`, on a
    /// book. A stand-in is the gate's and a wipe the eraser's: neither stands
    /// on the paper.
    fn guide_lines(&self, page: usize) -> Vec<GuideLine> {
        let line = |p: &lessons::Path| GuideLine::of(&p.points, p.width, p.over);
        let path = |i: &u16| self.art.get(usize::from(*i));
        if self.holding {
            return self.lesson.held(page).iter().filter_map(path).map(line).collect();
        }
        let shows = |p: &&lessons::Path| !matches!(p.kind, PathKind::StandIn | PathKind::Seed | PathKind::Wipe);
        let guide = self.current().map_or(&[][..], |t| t.guide);
        guide.iter().filter_map(path).filter(shows).map(line).collect()
    }

    /// AU23: the current task's dot at `at` on the paper, give or take its own
    /// width: a press lands near, and the dot is under the pointer. A dot that
    /// has its mark is done with: three presses of one dot are not three dots.
    /// A mark is the dot's by its middle: an outline's box holds the eye too.
    fn press_under(&self, at: [f32; 2]) -> Option<u16> {
        let guide = self.current().filter(|_| !self.holding).map_or(&[][..], |t| t.guide);
        let under = |p: &lessons::Path| {
            let b = p.bounds();
            let near = |from: f32, to: f32, v: f32| (from - p.width..=to + p.width).contains(&v);
            let on = |at: [f32; 2]| near(b[0], b[2], at[0]) && near(b[1], b[3], at[1]);
            let met = |m: &[f32; 4]| on([(m[0] + m[2]) * 0.5, (m[1] + m[3]) * 0.5]);
            p.is_press() && on(at) && !self.marks.iter().any(met)
        };
        guide.iter().copied().find(|i| self.art.get(usize::from(*i)).is_some_and(under))
    }

    /// The reveal `secs` along: those lines done, and the one the hand is on
    /// as far as it has come, ink first and the eraser's after.
    fn revealed(&self, paths: &[u16], secs: f64) -> (Vec<GuideLine>, Vec<GuideLine>) {
        let (mut ink, mut rubbing, mut began) = (Vec::new(), Vec::new(), 0.0);
        for path in paths.iter().filter_map(|i| self.art.get(usize::from(*i))) {
            let along = ((secs - began) / reveal_secs(path)).clamp(0.0, 1.0);
            let upto = (path.points.len() as f64 * along).ceil() as usize;
            began += reveal_secs(path);
            if upto > 0 {
                let line = GuideLine::of(&path.points[..upto], path.width, false);
                if path.kind == PathKind::Wipe { rubbing.push(line) } else { ink.push(line) }
            }
        }
        (ink, rubbing)
    }

    /// What the whole of a reveal of `paths` takes.
    fn reveal_length(&self, paths: &[u16]) -> f64 {
        paths.iter().filter_map(|i| self.art.get(usize::from(*i))).map(reveal_secs).sum()
    }

    /// Whether `paths` are met, each by its own stroke (spec 5.4): an outline
    /// cannot tick the dab of its eye, nor a dab an outline.
    fn inked(&self, paths: &[u16], paper: [f32; 2]) -> bool {
        let grow = [paper[0] * MEET_GROW, paper[1] * MEET_GROW];
        let answers = |path: u16, mark: &[f32; 4]| {
            self.art.get(usize::from(path)).is_some_and(|p| {
                let b = p.bounds();
                // A press has no length to span: a pen's tap is one point.
                let span = if p.is_press() { 0.0 } else { (b[2] - b[0]).max(b[3] - b[1]) };
                let inside = mark[0] >= b[0] - grow[0]
                    && mark[1] >= b[1] - grow[1]
                    && mark[2] <= b[2] + grow[0]
                    && mark[3] <= b[3] + grow[1];
                inside && (mark[2] - mark[0]).max(mark[3] - mark[1]) >= span * MEET_SPAN
            })
        };
        let mut taken: Vec<Option<usize>> = vec![None; self.marks.len()];
        paths.iter().enumerate().all(|(i, _)| {
            let mut seen = vec![false; self.marks.len()];
            match_path(i, paths, &self.marks, &answers, &mut seen, &mut taken)
        })
    }
}

/// One step of the matching of paths to strokes.
fn match_path(
    i: usize,
    paths: &[u16],
    marks: &[[f32; 4]],
    answers: &impl Fn(u16, &[f32; 4]) -> bool,
    seen: &mut [bool],
    taken: &mut [Option<usize>],
) -> bool {
    for m in 0..marks.len() {
        if seen[m] || !answers(paths[i], &marks[m]) {
            continue;
        }
        seen[m] = true;
        let free = match taken[m] {
            None => true,
            Some(other) => match_path(other, paths, marks, answers, seen, taken),
        };
        if free {
            taken[m] = Some(i);
            return true;
        }
    }
    false
}

/// A goal and each goal under an `All`.
fn each(done: Done, f: &mut impl FnMut(Done)) {
    match done {
        Done::All(list) => list.iter().for_each(|d| each(*d, f)),
        d => f(d),
    }
}

/// AU24: the float a ring's stop opens: its own, or the rail key that opens one.
fn ring_float(target: &Target) -> Option<crate::floats::Float> {
    match target {
        Target::Float(float) => Some(*float),
        Target::Rail(crate::rail::Key::Layers) => Some(crate::floats::Float::Layers),
        _ => None,
    }
}

/// The cel `state` shows and arms.
fn armed_cel(state: &AppState) -> Option<(DrawingId, LayerId)> {
    Some((state.current_drawing()?, state.active_layer_id()?))
}

/// A cel's layer, by its ids: there or not, whatever the view has moved to.
fn cel_layer(state: &AppState, (drawing, layer): (DrawingId, LayerId)) -> Option<&CelLayer> {
    state.cut().drawing(drawing)?.layers.iter().find(|l| l.id == layer)
}

/// `layer`'s pixel at `at`, in paper units.
fn pixel(layer: &CelLayer, at: [f32; 2]) -> [u16; 4] {
    let t = TILE as i32;
    let (x, y) = (at[0].floor() as i32, at[1].floor() as i32);
    let tile = layer.raster.tiles.get(&(x.div_euclid(t), y.div_euclid(t)));
    let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4) as usize;
    tile.map_or([0; 4], |tile| [tile.rgba[i], tile.rgba[i + 1], tile.rgba[i + 2], tile.rgba[i + 3]])
}

/// The pixel at `at` on every layer of the cel's drawing.
fn pixels_at(state: &AppState, cel: Option<(DrawingId, LayerId)>, at: [f32; 2]) -> Vec<(LayerId, [u16; 4])> {
    let drawing = cel.and_then(|(id, _)| state.cut().drawing(id));
    drawing.map_or_else(Vec::new, |d| d.layers.iter().map(|l| (l.id, pixel(l, at))).collect())
}

/// Did a layer's pixel move? A layer added since began empty.
fn pixels_moved(was: &[(LayerId, [u16; 4])], now: &[(LayerId, [u16; 4])]) -> bool {
    now.iter().any(|(id, px)| *px != was.iter().find(|(w, _)| w == id).map_or([0; 4], |(_, p)| *p))
}

/// Inked pixels of `layer` inside `b`, read from its tiles.
fn ink_in(layer: &CelLayer, b: [f32; 4]) -> u32 {
    let t = TILE as i32;
    let tiles = &layer.raster.tiles;
    let (left, top, right, bottom) = (b[0].floor() as i32, b[1].floor() as i32, b[2].ceil() as i32, b[3].ceil() as i32);
    let mut count = 0;
    for (&(tx, ty), tile) in tiles.iter() {
        for y in top.max(ty * t)..bottom.min((ty + 1) * t) {
            for x in left.max(tx * t)..right.min((tx + 1) * t) {
                let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4) as usize;
                count += u32::from(tile.rgba[i + 3] != 0);
            }
        }
    }
    count
}

/// The nodes `graph` draws from its Output.
pub(crate) fn reached(graph: &Graph) -> Vec<&NodeKind> {
    let mut seen = Vec::new();
    let mut walk: Vec<_> = graph.output.into_iter().collect();
    let mut out = Vec::new();
    while let Some(id) = walk.pop() {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        if let Ok(node) = graph.node(id) {
            out.push(&node.kind);
            walk.extend(node.inputs.iter().flatten().map(|pin| pin.0));
        }
    }
    out
}

fn generator_reached(cut: &Cut, kind: GenKind) -> bool {
    reached(&cut.graph).iter().any(|n| matches!(n, NodeKind::Generator { generator, .. } if *generator == kind))
}

/// The last words a person typed into the page. `Add text`'s stand-in is no
/// one's words, and nor is the node's own default.
fn words_on_page(cut: &Cut) -> bool {
    let own = |text: &str| !text.trim().is_empty() && text != crate::manga_tools::TEXT_START && text != "セリフ";
    let typed = |params: &anim_core::fx::FxParams| {
        params.0.values().any(|v| matches!(v, anim_core::fx::FxValue::Text { text, .. } if own(text)))
    };
    reached(&cut.graph).iter().any(|n| {
        matches!(n, NodeKind::Generator { generator: GenKind::Text | GenKind::Balloon, params, .. } if typed(params))
    })
}

fn track(cut: &Cut, track: u8) -> Option<&Column> {
    cut.xsheet.columns.get(usize::from(track))
}

/// The drawing a key of `column` puts down at `frame`, its own.
fn own_key(cut: &Cut, column: &Column, frame: u32) -> Option<bool> {
    match column.key_at(frame)? {
        Exposure::Drawing(id) => cut.drawing(id).map(drawing_has_art),
        Exposure::Empty => None,
    }
}

pub(crate) fn color_layer_art(d: &Drawing) -> bool {
    d.layers.iter().any(|l| l.props.role == LayerRole::Color && !l.raster.tiles.is_empty())
}

impl Editor {
    /// AU27: the run's start, on a paper or at the door: the three left-over
    /// states go (spec 5.3), and the first baseline is taken.
    pub(crate) fn begin_lesson(&mut self, lesson: Lesson, task: usize) {
        self.tools_hidden = false;
        let back = Borrowed {
            fill_gap: self.canvas.fill_gap,
            ink: self.canvas.brush_color,
            raster: self.canvas.raster,
            alpha_lock: self.canvas.alpha_lock,
            snap: self.state.manga.snap,
        };
        if let Some(gap) = lesson.fill_gap {
            self.canvas.fill_gap = gap;
        }
        if let Some(ink) = lesson.ink {
            self.canvas.brush_color = ink;
        }
        let art = Arc::new((lesson.art)());
        let task = task.min(lesson.tasks.len());
        let base = self.lesson_base(lesson.tasks.get(task), &art, 0);
        let state = &self.state;
        self.lesson = Some(LessonRun {
            lesson,
            task,
            base,
            based: false,
            back,
            sheet_off: false,
            result_on: false,
            doc: state.untitled_group.clone(),
            marks: Vec::new(),
            peak: state.engine.undo_depth_by(None),
            exported: 0,
            played: false,
            pass: None,
            was_playing: state.view.playing,
            last_frame: state.view.frame,
            mirror_seen: self.canvas.view_mirrored(),
            composite_seen: self.canvas.composite_view,
            asked: false,
            spilt: false,
            rest: self.lesson_rest(),
            picture: welcome::Picture::of(&lesson),
            art,
            reveal: None,
            holding: false,
            guide_for: None,
        });
    }

    /// AU27: a raster brush for the run, on a machine with the engine: with a
    /// vector brush armed `rail::keys_of` holds only Size and Color, so the rail
    /// has no Fill or Eraser to press or ring. A person's raster switch comes
    /// back on leaving. The Rig has no engine, so an ignored GPU test proves it.
    pub(crate) fn lesson_arm(&mut self) {
        if self.paint.is_some() {
            self.canvas.raster = true;
            self.canvas.arm_paint(false, &mut self.state);
        }
    }

    /// AU27: the name of the drawing on row 1 of track A.
    pub(crate) fn lesson_rest(&self) -> String {
        let cut = self.state.cut();
        let id = track(cut, lessons::TRACK_A).and_then(|c| c.resolve(0));
        id.and_then(|id| cut.drawing(id)).map_or_else(String::new, |d| d.name.clone())
    }

    fn lesson_base(&self, task: Option<&Task>, art: &[lessons::Path], exported: u32) -> Baseline {
        let state = &self.state;
        let cut = state.cut();
        let cel = armed_cel(state);
        let mut ink = Vec::new();
        let mut pixels = Vec::new();
        let mut presets = Vec::new();
        if let Some(task) = task {
            each(task.done, &mut |done| match done {
                Done::Erased(path) => {
                    let layer = cel.and_then(|c| cel_layer(state, c));
                    let inside = art.get(usize::from(path)).zip(layer).map_or(0, |(p, l)| ink_in(l, p.bounds()));
                    ink.push((path, inside));
                }
                Done::Filled { seed, keep } => {
                    pixels.extend(std::iter::once(seed).chain(keep).map(|at| (at, pixels_at(state, cel, at))));
                }
                Done::Preset(bank) => presets.extend(self.presets.iter().filter(|p| p.bank == bank).cloned()),
                _ => {}
            });
        }
        let column = cut.xsheet.column(state.view.active_column);
        let effects = cut.graph.nodes().filter_map(|n| match &n.kind {
            NodeKind::Effect { params, .. } => Some((n.id, params.clone())),
            _ => None,
        });
        Baseline {
            strokes: state.strokes_done,
            fills: state.fills_done,
            drawings: cut.drawings.len(),
            keys: column.map_or(0, |c| c.keys().count()),
            moves: self.canvas.moves_done,
            exported,
            brush: (self.canvas.brush_width, self.canvas.brush_color, self.canvas.armed_preset.clone()),
            smoothing: self.canvas.stabilizer,
            gutters: [state.manga.side_mm, state.manga.row_mm],
            presets,
            effects: effects.collect(),
            cel,
            ink,
            pixels,
            refusals: state.refusal_seq,
        }
    }

    /// AU27: the task the run stands on begins now: its baseline, and what
    /// only a watcher saw of the last one goes.
    fn lesson_rebase(&self, run: &mut LessonRun) {
        run.base = self.lesson_base(run.current(), &run.art, run.exported);
        run.rest = self.lesson_rest();
        run.marks.clear();
        run.peak = self.state.engine.undo_depth_by(None);
        (run.played, run.pass) = (false, None);
        run.mirror_seen = self.canvas.view_mirrored();
        run.composite_seen = self.canvas.composite_view;
        run.asked = false;
        run.spilt = false;
    }

    /// AU23: a fill of this task has reached the pixel it was to keep.
    fn lesson_spilt(&self, run: &LessonRun, done: Done) -> bool {
        let Done::Filled { keep: Some(keep), .. } = done else { return false };
        let cel = run.base.cel.or_else(|| armed_cel(&self.state));
        let was = run.base.pixels.iter().find(|(q, _)| *q == keep).map(|(_, px)| px);
        self.state.fills_done != run.base.fills && was.is_some_and(|px| pixels_moved(px, &pixels_at(&self.state, cel, keep)))
    }

    /// AU27: does `done` hold now? State, never Actions or clicks: a route the
    /// lesson did not name ticks the same.
    fn lesson_holds(&self, run: &LessonRun, done: Done) -> bool {
        let state = &self.state;
        let cut = state.cut();
        let view = &state.view;
        let base = &run.base;
        let strokes = state.strokes_done.saturating_sub(base.strokes);
        let fills = state.fills_done.saturating_sub(base.fills);
        let drawing = state.current_drawing().and_then(|id| cut.drawing(id));
        let layers = || drawing.map_or(&[][..], |d| &d.layers[..]);
        let column = cut.xsheet.column(view.active_column);
        let bezier = |k: &anim_core::xsheet::ParamKey| k.interp != ParamInterp::Linear || k.handles.is_some();
        let panels = || manga::panels_of(cut).map_or_else(Vec::new, |(_, polys)| polys);
        match done {
            Done::OnStep(stage) => self.stage == Some(stage),
            Done::Ink(paths) => {
                let paper = [run.lesson.paper.width as f32, run.lesson.paper.height as f32];
                strokes >= paths.len() as u64 && run.inked(paths, paper)
            }
            Done::Strokes(n) => strokes >= u64::from(n),
            Done::KeyArt { track: t, frame } => {
                track(cut, t).and_then(|c| own_key(cut, c, frame)).unwrap_or(false)
            }
            Done::InkOnLayer => strokes > 0 && state.active_layer_tiles().is_some_and(|t| !t.is_empty()),
            Done::InkInPanels(n) => {
                let polys = panels();
                let mut met: Vec<usize> = run.marks.iter().filter_map(|b| {
                    manga::panel_at(&polys, [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0])
                }).collect();
                met.sort_unstable();
                met.dedup();
                met.len() >= n as usize
            }
            Done::BrushChanged => {
                let (width, ink, preset) = &base.brush;
                *width != self.canvas.brush_width || *ink != self.canvas.brush_color || *preset != self.canvas.armed_preset
            }
            Done::Armed(name) => self.canvas.armed_preset.as_deref() == Some(name),
            Done::Smoothing => self.canvas.stabilizer > base.smoothing,
            Done::Tool(tool) => self.canvas.tool == tool,
            // `history_len` counts the redo side too: an undo does not move it.
            Done::Undone => state.engine.undo_depth_by(None) < run.peak,
            Done::Erased(path) => {
                let began = base.ink.iter().find(|(p, _)| *p == path).map_or(0, |(_, n)| *n);
                let layer = base.cel.and_then(|c| cel_layer(state, c));
                let now = run.art.get(usize::from(path)).zip(layer).map(|(p, l)| ink_in(l, p.bounds()));
                began > 0 && now.is_some_and(|now| now as f32 <= began as f32 * ERASED_LEFT)
            }
            Done::Filled { seed, keep } => {
                // A blank paper has no cel until its first mark makes one.
                let cel = base.cel.or_else(|| armed_cel(state));
                let moved = |p: [f32; 2]| {
                    let was = base.pixels.iter().find(|(q, _)| *q == p).map(|(_, px)| px);
                    was.map(|px| pixels_moved(px, &pixels_at(state, cel, p)))
                };
                fills > 0 && moved(seed) == Some(true) && keep.is_none_or(|k| moved(k) == Some(false))
            }
            Done::Fills(n) => fills >= u64::from(n),
            Done::FilledOn(n) => cut.drawings.iter().filter(|d| color_layer_art(d)).count() >= n as usize,
            Done::Drawings(n) => cut.drawings.len() >= n as usize,
            Done::Frame(from, to) => (from..=to).contains(&view.frame),
            Done::Played => run.played,
            Done::Speed(n) => column.is_some_and(|c| {
                let (from, to) = state.play_range();
                let runs = c.runs(from, to + 1);
                runs.len() >= 2 && runs[..runs.len() - 1].iter().all(|(_, len)| *len == n)
            }),
            Done::Hold { frame, len } => column.and_then(|c| c.exposure_at(frame)).is_some_and(|(key, next)| {
                next.unwrap_or_else(|| state.frame_count()).saturating_sub(key) == len
            }),
            Done::Shows { frame, drawing } => column.is_some_and(|c| {
                let key = c.keys().filter(|(_, e)| *e != Exposure::Empty).nth(drawing as usize);
                key.is_some_and(|(_, shown)| c.resolve(frame).map(Exposure::Drawing) == Some(shown))
            }),
            Done::Kinds(n) => {
                let marked = |d: &&Drawing| cut.drawing_def(d.id).kind != DrawingKind::Unset;
                cut.drawings.iter().filter(marked).count() >= n as usize
            }
            Done::Loop => view.loop_playback && state.loop_range().is_some(),
            Done::Layers(n) => layers().len() >= n as usize,
            Done::LayerHidden => layers().iter().any(|l| !l.props.visible),
            Done::LayerRole(role) => layers().iter().any(|l| l.props.role == role),
            Done::Moved => self.canvas.moves_done > base.moves,
            Done::Mirrored => run.mirror_seen && !self.canvas.view_mirrored(),
            Done::Effect(kind) => {
                reached(&cut.graph).iter().any(|n| matches!(n, NodeKind::Effect { fx, .. } if *fx == kind))
            }
            Done::AnyEffect => reached(&cut.graph).iter().any(|n| matches!(n, NodeKind::Effect { .. })),
            Done::Strength => cut.graph.nodes().any(|n| match &n.kind {
                NodeKind::Effect { params, .. } => base.effects.iter().any(|(id, was)| *id == n.id && was != params),
                _ => false,
            }),
            Done::CameraKeys(n) => cut.xsheet.params.iter().any(|c| c.keys().count() >= n as usize),
            Done::Eased => cut.xsheet.params.iter().any(|c| c.keys().any(|(_, k)| bezier(&k))),
            Done::Particles => generator_reached(cut, GenKind::Particles),
            Done::Panels(n) => panels().len() >= n as usize,
            Done::Pages(n) => pages(state) >= n as usize,
            Done::Gutters => base.gutters != [state.manga.side_mm, state.manga.row_mm],
            Done::Rulers(n) => Rulers::of(cut).vps.len() >= n as usize,
            Done::Tone => {
                let toned = |c: &&Column| manga::is_tone_column(cut, c.id);
                let art = |c: &Column| c.keys().any(|(frame, _)| own_key(cut, c, frame) == Some(true));
                cut.xsheet.columns.iter().filter(toned).any(art)
            }
            Done::Generator(kind) => generator_reached(cut, kind),
            Done::Words => words_on_page(cut),
            Done::PageSeen => run.composite_seen && !self.canvas.composite_view,
            Done::Lipped => column.is_some_and(|c| c.keys().count() > base.keys),
            Done::Inbetweens => {
                cut.drawings.len() > base.drawings && cut.drawings[base.drawings..].iter().all(drawing_has_art)
            }
            Done::Chart => cut.drawings.iter().any(|d| cut.drawing_def(d.id).chart.is_some()),
            Done::Exported(n) => run.exported.saturating_sub(base.exported) >= n,
            // A new name in the bank, or one saved over with new numbers.
            Done::Preset(bank) => self.presets.iter().any(|p| p.bank == bank && !base.presets.contains(p)),
            Done::Picked(rgb) => {
                let ink = self.canvas.brush_color;
                (0..3).all(|c| (i32::from(ink[c]) - i32::from(rgb[c])).abs() <= PICK_NEAR)
            }
            Done::Locked(on) => self.canvas.alpha_lock == on,
            Done::Snap(on) => state.manga.snap == on,
            Done::PanelPicked(n) => state.manga.selected == n as usize,
            Done::All(list) => list.iter().all(|d| self.lesson_holds(run, *d)),
        }
    }

    /// AU27: this frame's reading, right after `say_facts`: the current task
    /// alone, against its baseline. Ticks never un-tick. A learner ahead sails
    /// through in one frame, each next task on a baseline of its own. A
    /// document swapped in this frame is not the run's: the door ends the run
    /// on the next, and until then nothing of it is read or written, or the
    /// old run would tick on the new document and write its key into a file
    /// that never held a lesson.
    pub(crate) fn lesson_read(&mut self, facts: &Facts) {
        if self.lesson.as_ref().is_none_or(|run| run.doc != self.state.untitled_group) {
            return;
        }
        let Some(mut run) = self.lesson.take() else { return };
        if !std::mem::replace(&mut run.based, true) {
            self.lesson_rebase(&mut run);
        }
        self.lesson_note(&mut run, facts);
        // AU24: a piece the Select tool dragged floats until a click away, which
        // no sentence asks for: the drag's end sets it down, the one commit
        // `Moved` reads.
        if run.current().is_some_and(|task| task.done == Done::Moved) && self.canvas.float_moved() {
            self.canvas.finish_gesture(&mut self.state);
        }
        let mut ticked = false;
        while let Some(task) = run.current() {
            if !self.lesson_holds(&run, task.done) {
                break;
            }
            // AU24: a loop plays on past its pass for as long as they watch it.
            // Nothing else ends it, and a paper that is playing takes no pen, so
            // the task after the pass stops it.
            let watched = run.task.checked_sub(1).and_then(|i| run.lesson.tasks.get(i)).is_some_and(|t| t.done == Done::Played);
            if watched && self.state.view.playing && self.state.view.loop_playback {
                self.state.toggle_play();
            }
            run.task += 1;
            self.lesson_rebase(&mut run);
            ticked = true;
            // AU24: what the ring opened for this task would stand over the paper
            // the next one works on. What the person opened is theirs.
            if run.current().is_some_and(|next| matches!(next.ring, [Target::Paper])) {
                task.ring.iter().filter_map(ring_float).for_each(|float| self.floats.close(float));
                self.floats.more &= !task.ring.contains(&Target::Rail(crate::rail::Key::More));
            }
        }
        if ticked {
            self.state.engine.project.app_meta.insert(KEY.to_string(), format!("{}/{}", run.lesson.id, run.task));
            if run.finished() {
                write_finished(self.data_dir.as_deref(), run.lesson.id);
            }
        }
        self.lesson = Some(run);
    }

    /// AU27: what only a watcher sees: the stroke's box, a pass, a view that
    /// was on.
    fn lesson_note(&mut self, run: &mut LessonRun, facts: &Facts) {
        if facts.stroked && let Some(mark) = self.canvas.last_stroke_box.take() {
            run.marks.push(mark);
        }
        let latched = run.lesson.latch.is_none_or(|latch| match latch {
            Latch::Transparent => facts.transparent,
            Latch::Mono => facts.mono,
        });
        run.exported += if latched { facts.landed } else { 0 };
        run.peak = run.peak.max(self.state.engine.undo_depth_by(None));
        run.mirror_seen |= self.canvas.view_mirrored();
        run.composite_seen |= self.canvas.composite_view;
        run.spilt |= run.current().filter(|t| t.miss.is_some()).is_some_and(|t| self.lesson_spilt(run, t.done));
        // `commit_path`'s `painted` is doc.rs's word to a log. The card does the
        // talking here, and the lane must not read like a bug beside it.
        if self.state.status.starts_with("painted") {
            self.state.status.clear();
        }
        let view = &self.state.view;
        let end = self.state.play_range().1;
        // A frame that fell back under a running pass is the wrap. A stop
        // moves the frame back too, onto its run's first frame: no pass.
        let wrapped = view.playing && run.was_playing && view.frame < run.last_frame;
        if view.playing && !run.was_playing {
            run.pass = Some(view.frame);
        }
        if run.pass.is_some_and(|from| from != end && (view.frame == end || wrapped)) {
            run.played = true;
        }
        // A wrap starts the next pass, on a run that was going when the task
        // opened too: that one began after the task did.
        if wrapped {
            run.pass = Some(view.frame);
        }
        if !view.playing {
            run.pass = None;
        }
        (run.was_playing, run.last_frame) = (view.playing, view.frame);
    }

    /// AU27: the one door's reading of `app.lesson` (spec 5.11), before the
    /// door moves anything: `plan_sheet_off` must see the run. Open and Recover
    /// replace the state without a boot, so a boot cannot be the place. An
    /// unknown id is ignored and kept. A finished lesson's key goes: the file
    /// is the person's now, and its card would stand over their work for good.
    /// True: a run began here.
    pub(crate) fn lesson_door(&mut self) -> bool {
        let doc = &self.state.untitled_group;
        if self.lesson.as_ref().is_some_and(|run| run.doc == *doc) {
            return false;
        }
        // The run stood over another document, which has gone.
        self.end_lesson();
        let meta = self.state.engine.project.app_meta.get(KEY);
        let Some((id, task)) = meta.and_then(|m| m.rsplit_once('/')) else { return false };
        let found = lessons::table().iter().find(|l| l.id == id);
        let Some(lesson) = found.filter(|_| !self.canvas.is_guest) else { return false };
        let task = task.parse().unwrap_or(0);
        if task >= lesson.tasks.len() {
            self.drop_lesson_key();
            return false;
        }
        self.begin_lesson(*lesson, task);
        // AU23: no file holds the brush's colour, and the next fill goes on in
        // the one the lesson last had the person pick.
        let picked = |t: &Task| if let Done::Picked([r, g, b]) = t.done { Some([r, g, b, 255]) } else { None };
        if let Some(color) = lesson.tasks[..task].iter().rev().find_map(picked) {
            self.canvas.brush_color = color;
        }
        // AU24: nor does any hold the lock or the snap, and the next task stands
        // on what the last ones turned on or off.
        for before in &lesson.tasks[..task] {
            each(before.done, &mut |done| match done {
                Done::Locked(on) => self.canvas.alpha_lock = on,
                Done::Snap(on) => self.state.manga.snap = on,
                _ => {}
            });
        }
        true
    }

    /// `app.lesson` goes. An untouched paper with no file asks nothing.
    fn drop_lesson_key(&mut self) {
        self.state.engine.project.app_meta.remove(KEY);
        self.state.meta_dirty |= self.state.file_path.is_some();
    }

    /// AU27: the run's end: what it borrowed goes back.
    fn end_lesson(&mut self) -> Option<LessonRun> {
        let run = self.lesson.take()?;
        let Borrowed { fill_gap, ink, raster, alpha_lock, snap } = run.back;
        (self.canvas.fill_gap, self.canvas.brush_color, self.canvas.raster) = (fill_gap, ink, raster);
        (self.canvas.alpha_lock, self.state.manga.snap) = (alpha_lock, snap);
        self.canvas.guide = canvas::Guide::default();
        Some(run)
    }

    /// AU28: the paper's lines for this frame, set as `empty_cell_hint` is,
    /// and Show me's hand. Pen down cancels it, and the pen's own stroke goes
    /// on as it would have (spec 5.9); so does anything that moves what it
    /// stands over, a pill pressed on the way above all.
    pub(crate) fn lesson_guide(&mut self, ctx: &egui::Context) {
        self.lesson_press(ctx);
        let page = open_page(&self.state);
        let Some(run) = self.lesson.as_mut() else {
            self.canvas.guide = canvas::Guide::default();
            self.state.manga.quiet = false;
            return;
        };
        let wanted = Some((run.task, run.holding, page));
        if run.guide_for != wanted {
            run.guide_for = wanted;
            self.canvas.guide.lines = run.guide_lines(page);
        }
        self.state.manga.quiet = !self.canvas.guide.lines.is_empty();
        let task = run.task;
        let reveal = run.reveal.take();
        self.canvas.guide.reveal.clear();
        self.canvas.guide.rubbing.clear();
        let live = reveal.filter(|r| !self.canvas.stroke_active() && r.over == self.lesson_stand(task));
        let (Some(reveal), Some(run)) = (live, self.lesson.as_mut()) else { return };
        let secs = ctx.input(|i| i.time) - reveal.began;
        if secs < run.reveal_length(reveal.paths) {
            (self.canvas.guide.reveal, self.canvas.guide.rubbing) = run.revealed(reveal.paths, secs);
            self.canvas.guide.reveal_ink = plate::opaque(self.canvas.brush_color);
            run.reveal = Some(reveal);
            ctx.request_repaint();
            return;
        }
        self.lesson_land(reveal.paths);
    }

    /// AU23: `Press once on each blue dot` is a press. A pen's tap paints by the
    /// pen route, whose mouse paints nothing for a bare click, and that route
    /// is not this lesson's to move: so a click on a dot lands it as Show me's
    /// ink does. Only what the canvas would have taken counts: a click on
    /// chrome over a dot is the chrome's, and the click a pen's tap leaves
    /// behind is that tap, already painted.
    fn lesson_press(&mut self, ctx: &egui::Context) {
        let click = ctx.input(|i| i.pointer.primary_clicked().then(|| i.pointer.interact_pos()).flatten());
        let (Some(run), Some(at)) = (self.lesson.as_ref(), click) else { return };
        let Some(task) = run.current() else { return };
        let canvas = &self.canvas;
        let armed = canvas.tool == canvas::CanvasTool::Paint && !self.canvas.erasing;
        if canvas.stroke_active() || canvas.pen_has_pointer() || !armed {
            return;
        }
        if !self.lesson_paper() || self.lesson_wandered(task) || !self.lesson_canvas_under(ctx, at) {
            return;
        }
        let paper = canvas.view_xform().screen_to_paper(at);
        if let Some(dot) = run.press_under([paper.x, paper.y]) {
            self.lesson_land(&[dot]);
        }
    }

    /// AU23: the canvas is what stands at `at`: its layer on top, inside its
    /// rect, and no control of that layer over it. `lesson_guide` runs before
    /// the dock, so these are last frame's, which is what the click landed on.
    fn lesson_canvas_under(&self, ctx: &egui::Context, at: egui::Pos2) -> bool {
        let (layer, rect) = (self.canvas.dbg_layer, self.canvas.dbg_rect);
        let control = |r: &egui::WidgetRect| r.rect != rect && r.sense.interactive() && r.rect.contains(at);
        ctx.layer_id_at(at) == Some(layer)
            && rect.contains(at)
            && !ctx.viewport(|v| v.prev_pass.widgets.get_layer(layer).any(control))
    }

    /// AU28: the Canvas pane is on screen: a step may hold none, or have it
    /// behind another tab.
    fn lesson_paper(&self) -> bool {
        crate::visible_panes(&self.dock).contains(&crate::Pane::Canvas)
    }

    /// AU28: what a reveal begun on `task` now would stand over.
    fn lesson_stand(&self, task: usize) -> Stand {
        let state = &self.state;
        Stand {
            task,
            revision: state.engine.revision(),
            step: self.stage,
            cut: state.at(),
            frame: state.view.frame,
            column: state.view.active_column,
            layer: state.view.active_layer.clone(),
            paper: self.lesson_paper(),
        }
    }

    /// AU28: `paths` land as Show me's ink: one undo, and a stroke each to the
    /// signal, by the circle's route: the pen's refusals, then `commit_path`
    /// with `canvas.ink()`, honouring `Keep inside strokes`.
    pub(crate) fn lesson_land(&mut self, paths: &[u16]) -> bool {
        let Some(art) = self.lesson.as_ref().map(|run| run.art.clone()) else { return false };
        let Some(slot) = self.canvas.slot_at_pen_down(true, &mut self.state) else { return false };
        let (rgba, inside) = (self.canvas.ink(), self.canvas.alpha_lock);
        let lines: Vec<&lessons::Path> = paths.iter().filter_map(|i| art.get(usize::from(*i))).collect();
        let erase = |p: &lessons::Path| p.kind == PathKind::Wipe;
        let ink = lines.iter().map(|p| PathInk { points: &p.points, width: p.width, rgba, erase: erase(p), inside });
        if !self.state.commit_path(&ink.collect::<Vec<_>>(), slot) {
            return false;
        }
        if let Some(run) = self.lesson.as_mut() {
            run.marks.extend(lines.iter().map(|p| p.bounds()));
        }
        self.shown_landed();
        true
    }

    /// `Leave lesson`: the card goes, the document stays a real document. The
    /// paper refits once for the room the card gave back, and Plan's sheet and
    /// Color's `Result beside` are the person's own memory's again.
    pub(crate) fn leave_lesson(&mut self) {
        if self.end_lesson().is_none() {
            return;
        }
        self.drop_lesson_key();
        self.plan_sheet_back();
        self.result_back();
        self.canvas.fit_view();
    }

    /// `Show me`: the demonstration lands on the real document, so the task
    /// ticks as any other. AU21: `Show me where` rings the control, and that
    /// is all an Experienced lesson's button does (spec 5.2). AU28: a line is
    /// revealed at a hand's pace first, a fill lands at once, and neither
    /// reaches for a paper the person has left, or that no pane shows: the
    /// card is pointing back. A paper the pen would refuse is refused here,
    /// as the circle's is, before the hand sets out.
    pub(crate) fn lesson_show(&mut self, ctx: &egui::Context) {
        let Some(run) = self.lesson.as_mut() else { return };
        run.asked = true;
        if run.lesson.class == Class::Experienced {
            return;
        }
        let Some(task) = run.current() else { return };
        let on_paper = matches!(task.show, Show::Trace(_) | Show::Fill(_));
        if on_paper && (self.lesson_wandered(task) || !self.lesson_paper()) {
            return;
        }
        if on_paper && self.canvas.stroke_active() {
            self.state.refuse(doc::FINISH_STROKE);
            return;
        }
        match task.show {
            Show::Perform(action) => {
                self.invoke(action, ctx);
            }
            Show::Run(run) => run(self),
            Show::Trace(paths) => {
                if self.canvas.slot_at_pen_down(true, &mut self.state).is_none() {
                    return;
                }
                let began = ctx.input(|i| i.time);
                let over = self.lesson_stand(self.lesson.as_ref().map_or(0, |run| run.task));
                if let Some(run) = self.lesson.as_mut() {
                    run.reveal.get_or_insert(Reveal { paths, began, over });
                }
            }
            Show::Fill(seed) => {
                if self.canvas.slot_at_pen_down(true, &mut self.state).is_some() {
                    self.canvas.fill_at(egui::pos2(seed[0], seed[1]), &mut self.state);
                }
            }
            Show::Where => {}
        }
    }

    /// `Next lesson`: the next class of this type, and after the last class
    /// the Lessons page.
    fn lesson_next(&mut self, ctx: &egui::Context) {
        let Some(run) = self.lesson.as_ref() else { return };
        let (kind, class) = (run.lesson.kind, run.lesson.class);
        let next = match class {
            Class::Rookie => Some(Class::Amateur),
            Class::Amateur => Some(Class::Experienced),
            Class::Experienced => None,
        };
        let table = lessons::table();
        let row = next.and_then(|class| table.iter().position(|l| l.kind == kind && l.class == class));
        match row {
            Some(i) => self.choose_start(welcome::WelcomeAction::Lesson(i), ctx),
            None => {
                let mut w = welcome::Welcome::over_editor(&self.recent);
                w.lessons = table;
                self.open_start(w.on_lessons());
            }
        }
    }

    /// The word a token reads in this run.
    fn lesson_word(&self, run: &LessonRun, token: &str, ctx: &egui::Context) -> Option<String> {
        if token == "rest drawing" {
            return Some(run.rest.clone());
        }
        if let Some(action) = chord_action(token) {
            return Some(menus::chord_label(ctx, action).unwrap_or_else(|| "its key".to_string()));
        }
        word(token).map(str::to_string)
    }

    /// The task's own step has been left. A task that is the pill stays itself,
    /// and a place off the spine stands when the chain itself goes there: Plate
    /// One sends the person into the Brush maker, a pane no step holds.
    fn lesson_wandered(&self, task: &Task) -> bool {
        if matches!(task.done, Done::OnStep(_)) || self.stage == Some(task.step) {
            return false;
        }
        !(self.stage.is_none() && self.lesson_place(task))
    }

    /// The dock shows a pane the task's chain names and its step does not hold.
    fn lesson_place(&self, task: &Task) -> bool {
        let (here, step) = (crate::visible_panes(&self.dock), crate::visible_panes(&task.step.dock()));
        task.ring.iter().any(|t| matches!(t, Target::Pane(p) if here.contains(p) && !step.contains(p)))
    }

    /// The card's button has something to show: the paper rings nothing, so a
    /// task there offers no `Show me where`, and its sentence says the paper.
    fn lesson_rings(task: Option<&Task>) -> bool {
        task.is_some_and(|t| !matches!(t.ring.last(), None | Some(Target::Paper)))
    }

    /// AU21: what the ring looks for this frame, ancestor first: the way back
    /// when the step was left (spec 5.10), else the task's own chain, from its
    /// start, and in an Experienced lesson only once asked (spec 5.2).
    pub(crate) fn lesson_chain(&self) -> Vec<Target> {
        let Some(run) = self.lesson.as_ref() else { return Vec::new() };
        let Some(task) = run.current() else { return Vec::new() };
        if self.lesson_wandered(task) {
            return vec![Target::Step(task.step)];
        }
        if run.lesson.class == Class::Experienced && !run.asked {
            return Vec::new();
        }
        task.ring.to_vec()
    }

    /// AU27: the sentence the card stands on. The wander line takes the
    /// title's place when the task's own step has been left (spec 5.10), and
    /// `miss` the sentence once the act reached what it was to keep.
    pub(crate) fn lesson_lines(&self, run: &LessonRun, ctx: &egui::Context) -> Option<LessonLines> {
        let task = run.current()?;
        let words = |text: &str| resolve(text, |t| self.lesson_word(run, t, ctx));
        let wander = self.lesson_wandered(task).then(|| {
            format!("Go back to {}", self.step_name(task.step))
        });
        let say = task.miss.filter(|_| run.spilt).unwrap_or(task.say);
        // The carrying task, and the one after it in the same step: a step's
        // reason is read while it is young.
        let prior = run.task.checked_sub(1).and_then(|i| run.lesson.tasks.get(i));
        let young = task.why.or_else(|| prior.filter(|p| p.step == task.step).and_then(|p| p.why));
        // AU28: the tap was refused for want of an engine, and the button can
        // still do it (spec 5.9).
        let refused = self.state.refusal_seq != run.base.refusals && self.state.refusal.contains(canvas::NEEDS_ENGINE);
        let answers = run.lesson.class != Class::Experienced && !matches!(task.show, Show::Where);
        let by_hand = (refused && answers && self.paint.is_none()).then_some(BY_HAND);
        Some(LessonLines { wander, say: words(say), why: young.map(words), by_hand })
    }

    /// The card, a panel of its own: the dock, Fit and the rail's host follow.
    pub(crate) fn lesson_panel(&mut self, ui: &mut egui::Ui) {
        if self.lesson.is_none() {
            return;
        }
        let wide = card_width(ui.ctx().content_rect().width());
        let panel = egui::Panel::right("lesson").exact_size(wide).resizable(false);
        // No margin of the frame's: `CARD_PAD` is the card's one inset, or the
        // column loses a tenth of its width to the two together.
        let frame = plate::desk_frame(ui.style()).fill(plate::ISLAND).inner_margin(0);
        let panel = panel.frame(frame).show_separator_line(false);
        panel.show(ui, |ui| self.lesson_card(ui));
    }

    fn lesson_card(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(run) = self.lesson.as_ref() else { return };
        let lesson = run.lesson;
        let (done, total, finished) = (run.task, lesson.tasks.len(), run.finished());
        let lines = self.lesson_lines(run, &ctx);
        let bar: &[steps::Step] = steps::bar(lesson.kind);
        let path: Vec<_> = bar.iter().filter(|s| lesson.tasks.iter().any(|t| t.step == s.stage)).collect();
        let now = run.current().map(|t| t.step);
        let chip = lesson.tasks[..done.min(total)].iter().chain(run.current()).any(|t| {
            t.ring.contains(&Target::Named(CHIP))
        });
        let night = plate::chip_bytes(plate::LESSON_NIGHT);
        let armed = self.canvas.brush_color == night;
        let where_only = lesson.class == Class::Experienced || matches!(run.current().map(|t| t.show), Some(Show::Where));
        let offered = !where_only || Self::lesson_rings(run.current());
        // The coat's landmarks stand on the paper; the finish waits behind
        // this. It paints on the paper, so it stands only where one shows.
        let hold = lesson.class == Class::Amateur && !finished && self.lesson_paper() && !self.canvas.composite_view;

        let area = ui.max_rect().shrink(CARD_PAD);
        let foot_rows = if hold { 3.0 } else { 2.0 };
        let foot_h = foot_rows * plate::CTRL_H + (foot_rows - 1.0) * CARD_GAP;
        let foot = egui::Rect::from_min_max(egui::pos2(area.left(), area.bottom() - foot_h), area.max);
        let top = egui::Rect::from_min_max(area.min, egui::pos2(area.right(), foot.top() - CARD_GAP));
        let width = area.width();

        ui.scope_builder(egui::UiBuilder::new().max_rect(top), |ui| {
            let scroll = egui::ScrollArea::vertical().id_salt("lesson_scroll").auto_shrink([false, false]);
            let shown = scroll.show(ui, |ui| {
                ui.set_width(width);
                // The card's top line (spec 5.10): the way back, in the title's
                // own slot, so a long sentence keeps its room.
                match lines.as_ref().and_then(|l| l.wander.as_deref()) {
                    Some(back) => plate::lesson_words(ui, back, Role::Step, plate::BLUE_DEEP),
                    None => plate::lesson_words(ui, lesson.title, Role::Step, plate::INK),
                };
                ui.add_space(HEAD_GAP);
                plate::lesson_strip(ui, width, done, total);
                ui.add_space(HEAD_GAP);
                let names: Vec<_> = path.iter().map(|s| (s.label, Some(s.stage) == now)).collect();
                plate::lesson_path(ui, &names);
                ui.add_space(CARD_GAP);
                if let Some(lines) = &lines {
                    plate::lesson_words(ui, &lines.say, Role::Heading, plate::INK);
                    if let Some(why) = &lines.why {
                        ui.add_space(CARD_GAP);
                        plate::lesson_words(ui, why, Role::Panel, plate::INK2);
                    }
                    if let Some(by_hand) = lines.by_hand {
                        ui.add_space(CARD_GAP);
                        plate::lesson_words(ui, by_hand, Role::Panel, plate::BLUE_DEEP);
                    }
                } else {
                    plate::lesson_words(ui, MADE, Role::Heading, plate::INK);
                    ui.add_space(CARD_GAP);
                    // A closing in the file's own words opens with the line above.
                    let closing = lesson.closing.strip_prefix(MADE).unwrap_or(lesson.closing).trim_start();
                    plate::lesson_words(ui, closing, Role::Panel, plate::INK2);
                }
                if chip && !finished {
                    ui.add_space(CARD_GAP);
                    let swatch = plate::swatch(ui, plate::LESSON_NIGHT, armed, NIGHT);
                    plate::mark_row(&swatch, Target::Named(CHIP));
                    if swatch.clicked() {
                        self.canvas.brush_color = night;
                    }
                }
                ui.add_space(CARD_GAP);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, PICTURE_H), egui::Sense::hover());
                if let Some(run) = self.lesson.as_ref() {
                    plate::lesson_picture(ui, rect, |p, paper| run.picture.paint(p, paper));
                }
            });
            plate::note_viewport(ui, shown.inner_rect, [false, true]);
        });
        ui.scope_builder(egui::UiBuilder::new().max_rect(foot), |ui| {
            ui.spacing_mut().item_spacing.y = CARD_GAP;
            // AU24: where no `Show me` is offered it is the card's one help, and
            // has that button's look: a ghost row reads as a caption.
            let hold_row = |ui: &mut egui::Ui| if offered { plate::ghost_row(ui, HOLD, width, None) } else { plate::filled_row(ui, HOLD, width) };
            let held = hold && hold_row(ui).is_pointer_button_down_on();
            if let Some(run) = self.lesson.as_mut() {
                run.holding = held;
            }
            if finished {
                if plate::filled_row(ui, NEXT, width).clicked() {
                    self.lesson_next(&ctx);
                }
            } else if offered {
                let word = if where_only { SHOW_WHERE } else { SHOW_ME };
                if plate::filled_row(ui, word, width).clicked() {
                    self.lesson_show(&ctx);
                }
            } else {
                // `Leave lesson` stays under the hand.
                ui.add_space(plate::CTRL_H + CARD_GAP);
            }
            if plate::ghost_row(ui, LEAVE, width, None).clicked() {
                self.leave_lesson();
            }
        });
    }
}

/// The card's sentences, resolved.
#[derive(Debug, PartialEq)]
pub(crate) struct LessonLines {
    pub wander: Option<String>,
    pub say: String,
    pub why: Option<String>,
    pub by_hand: Option<&'static str>,
}

/// The book's pages.
fn pages(state: &AppState) -> usize {
    let scenes = state.engine.project.scenes.iter();
    scenes.flat_map(|s| s.cuts.iter()).filter(|c| manga::Page::of(c).is_some()).count()
}

/// AU24: which of the scene's cuts is open: a book's page, counted from 0.
fn open_page(state: &AppState) -> usize {
    let at = state.at();
    let scene = state.engine.project.scene(at.scene);
    scene.and_then(|s| s.cuts.iter().position(|c| c.id == at.cut)).unwrap_or(0)
}

/// AU27: a lesson's document: its paper, given its art with no history to
/// undo into. `app.lesson` is no edit: the document opens clean. The run
/// starts before the step is entered: `plan_sheet_off` reads it.
pub(crate) fn open_lesson(
    lesson: &Lesson,
    rs: Option<&RenderState>,
    dir: Option<&Path>,
    ui: &config::UiConfig,
) -> Editor {
    let mut state = lesson.paper.build();
    if let Some(prep) = lesson.prep {
        prep(&mut state);
        state.engine.clear_history();
        state.saved_rev = state.engine.revision();
    }
    state.engine.project.app_meta.insert(KEY.to_string(), format!("{}/0", lesson.id));
    let mut editor = Editor::boot(state, rs, dir, ui);
    editor.begin_lesson(*lesson, 0);
    editor.enter_template(lesson.paper);
    editor.lesson_arm();
    editor
}

