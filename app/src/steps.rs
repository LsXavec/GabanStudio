//! AU3: THE STEPS, in the order a person works: the top row's pills. A step
//! is a lens on the one document; switching swaps the arrangement and the
//! tool, and never forks data. `Stage::ALL` keeps the stored order and its
//! tests; the display order lives here (Timing before Color: the line test
//! comes before paint). AU18: each kind of project has its own bar, cut from
//! one catalog of rows.

use anim_core::manga::PAGE_KEY;
use anim_core::model::Project;

use crate::config::Action;
use crate::vocab;
use crate::workspace::Stage;

/// `project.app_meta`'s key for the kind (`app.kind` on disk).
pub const KIND_KEY: &str = "kind";

/// AU18: what a project is. The app's word: anim-core holds no app types and
/// never reads `app_meta`. Nothing caches it, because four routes replace the
/// document with no boot: the project is the one truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectKind {
    Drawing,
    Animation,
    Comic,
}

impl ProjectKind {
    pub const ALL: [ProjectKind; 3] = [ProjectKind::Drawing, ProjectKind::Animation, ProjectKind::Comic];

    /// The stored value, unknown or absent as None.
    fn stored(p: &Project) -> Option<ProjectKind> {
        match p.app_meta.get(KIND_KEY).map(String::as_str) {
            Some("drawing") => Some(ProjectKind::Drawing),
            Some("animation") => Some(ProjectKind::Animation),
            Some("comic") => Some(ProjectKind::Comic),
            _ => None,
        }
    }

    /// The stored kind wins. An old file, and an unknown value, derive.
    pub fn of(p: &Project) -> ProjectKind {
        Self::stored(p).unwrap_or_else(|| Self::derive(p))
    }

    /// Every cut a page: a comic. "Every", not "any": an old animation with
    /// one page among its cuts keeps its Plan and Timing. This runs per frame,
    /// so it asks for the key and never parses the page.
    fn derive(p: &Project) -> ProjectKind {
        let mut cuts = p.scenes.iter().flat_map(|scene| &scene.cuts);
        if cuts.clone().next().is_some() && cuts.clone().all(|cut| cut.meta.contains_key(PAGE_KEY)) {
            ProjectKind::Comic
        } else if cuts.clone().count() > 1 || cuts.any(|cut| cut.frame_count > 1) {
            ProjectKind::Animation
        } else {
            ProjectKind::Drawing
        }
    }

    /// What a drawing has grown into, if it has (spec 2.7). The page check
    /// comes first, or a New page would promote to Animation by cut count.
    /// Frames, not drawings: two tracks on one frame are still a picture.
    pub fn grown(p: &Project) -> Option<ProjectKind> {
        let mut cuts = p.scenes.iter().flat_map(|scene| &scene.cuts);
        if cuts.clone().any(|cut| cut.meta.contains_key(PAGE_KEY)) {
            Some(ProjectKind::Comic)
        } else if cuts.clone().count() > 1 || cuts.any(|cut| cut.frame_count > 1) {
            Some(ProjectKind::Animation)
        } else {
            None
        }
    }

    /// Into `app_meta`. This alone does not mark the document dirty.
    pub fn stamp(self, p: &mut Project) {
        let word = match self {
            ProjectKind::Drawing => "drawing",
            ProjectKind::Animation => "animation",
            ProjectKind::Comic => "comic",
        };
        p.app_meta.insert(KIND_KEY.to_string(), word.to_string());
    }
}

pub struct Step {
    /// The stage it lenses. AU18: Plan and Draw both lens `Stage::Drawing`, on
    /// two bars, so a stage is not a row's name.
    pub stage: Stage,
    pub label: &'static str,
    /// The hover card's first line.
    pub purpose: &'static str,
    /// The hover card's second line; "" where a row has none.
    pub studio_words: &'static str,
    pub action: Action,
}

const PLAN: Step = Step {
    stage: Stage::Drawing,
    label: vocab::STEP_PLAN,
    purpose: "Draw your key poses on the frames you choose.",
    studio_words: "genga",
    action: Action::RoomDrawing,
};
const DRAW: Step = Step {
    stage: Stage::Drawing,
    label: vocab::STEP_DRAW,
    purpose: "Draw on the paper.",
    studio_words: "",
    action: Action::RoomDrawing,
};
const TIMING: Step = Step {
    stage: Stage::Edit,
    label: vocab::STEP_TIMING,
    purpose: "Arrange when each drawing appears.",
    studio_words: "henshū",
    action: Action::RoomEdit,
};
const COLOR: Step = Step {
    stage: Stage::InkPaint,
    label: vocab::STEP_COLOR,
    purpose: "Fill your drawings with color.",
    studio_words: "shiage",
    action: Action::RoomFinishing,
};
const EFFECTS: Step = Step {
    stage: Stage::Composite,
    label: vocab::STEP_EFFECTS,
    purpose: "Add glow, blur and camera moves.",
    studio_words: "satsuei",
    action: Action::RoomComposite,
};
const MANGA: Step = Step {
    stage: Stage::Manga,
    label: vocab::STEP_MANGA,
    purpose: "Draw the book's pages, panels and screen tone.",
    studio_words: "manga",
    action: Action::MangaRoom,
};
const EXPORT: Step = Step {
    stage: Stage::Export,
    label: vocab::STEP_EXPORT,
    purpose: "Save a file to share.",
    studio_words: "",
    action: Action::RoomExport,
};

/// Every row, on a bar or off it.
pub const CATALOG: [Step; 7] = [PLAN, DRAW, TIMING, COLOR, EFFECTS, MANGA, EXPORT];

static DRAWING_BAR: [Step; 4] = [DRAW, COLOR, EFFECTS, EXPORT];
static ANIMATION_BAR: [Step; 5] = [PLAN, TIMING, COLOR, EFFECTS, EXPORT];
static COMIC_BAR: [Step; 3] = [MANGA, COLOR, EXPORT];

/// The top row of a project of this kind.
pub fn bar(kind: ProjectKind) -> &'static [Step] {
    match kind {
        ProjectKind::Drawing => &DRAWING_BAR,
        ProjectKind::Animation => &ANIMATION_BAR,
        ProjectKind::Comic => &COMIC_BAR,
    }
}

/// The catalog's row for `stage`, so a stage off a bar still finds its words.
/// By the stage's own name: `Stage::Drawing` is two rows, and it is Draw's, and
/// no row lenses `Stage::Layout`, which is Plan's.
pub fn of(stage: Stage) -> Option<&'static Step> {
    CATALOG.iter().find(|s| s.label == stage.name())
}

impl Step {
    /// The hover card: the purpose, then the studio word where it has one.
    pub fn card(&self) -> String {
        if self.studio_words.is_empty() {
            self.purpose.to_string()
        } else {
            format!("{}\nStudio word: {}", self.purpose, self.studio_words)
        }
    }
}

/// Row widths, in points. The row fits nine menus from `WIDE`, and the
/// off-spine pill as well from `WIDE_PLACE`; under that the menus collapse
/// into one `Menu` button and Search becomes an icon.
pub const WIDE: f32 = 1120.0;
pub const WIDE_PLACE: f32 = 1180.0;
/// The off-spine pill: its width is the name's, between these.
pub const PLACE_MIN: f32 = 72.0;
pub const PLACE_MAX: f32 = 140.0;
/// Space between the row's groups.
pub const GROUP_GAP: f32 = 16.0;
/// The foot: under this the rarer transport folds behind one `…` (spec 4.4).
pub const FOLD: f32 = 1040.0;
/// From here the full transport's Sound key has room for its words beside
/// the Say line's floor; from the fold to here it is its icon.
pub const SOUND_WORDS: f32 = 1160.0;

/// The menus collapse at this window width.
pub fn narrow(width: f32, off_spine: bool) -> bool {
    width < if off_spine { WIDE_PLACE } else { WIDE }
}

/// The off-spine pill's width: the name's, at least [`PLACE_MIN`] and at most
/// `min(PLACE_MAX, room)`. Only the minimum is firm: under it the row has
/// failed its narrow rule, and the pill keeps its minimum rather than vanish.
pub fn place_width(name: f32, room: f32) -> f32 {
    name.max(PLACE_MIN).min(PLACE_MAX.min(room)).max(PLACE_MIN)
}

/// Where the step island starts: centred, unless the menus reach past that.
/// The island may run to `limit`: the steps hold still until `tail`, the
/// off-spine pill, would pass it.
pub fn island_x(window: f32, menus_end: f32, steps_width: f32, tail: f32, limit: f32) -> f32 {
    let centred = (window - steps_width) / 2.0;
    (menus_end + GROUP_GAP).max(centred.min(limit - steps_width - tail))
}
