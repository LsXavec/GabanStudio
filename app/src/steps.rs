//! AU3: THE STEPS, in the order a person works: the top row's pills. A step
//! is a lens on the one document; switching swaps the arrangement and the
//! tool, and never forks data. `Stage::ALL` keeps the stored order and its
//! tests; the display order lives here (Timing before Color: the line test
//! comes before paint).

use crate::config::Action;
use crate::vocab;
use crate::workspace::Stage;

pub struct Step {
    /// The stage it lenses. None for Export, a pane until the Export step
    /// has a stage of its own.
    pub stage: Option<Stage>,
    pub label: &'static str,
    /// The hover card's first line.
    pub purpose: &'static str,
    /// The hover card's second line; "" for Export.
    pub studio_words: &'static str,
    pub action: Action,
}

pub const STEPS: [Step; 6] = [
    Step {
        stage: Some(Stage::Layout),
        label: vocab::STEP_PLAN,
        purpose: "Set up shots, timing and camera before drawing.",
        studio_words: "layout",
        action: Action::RoomLayout,
    },
    Step {
        stage: Some(Stage::Drawing),
        label: vocab::STEP_DRAW,
        purpose: "Draw on the paper, one drawing at a time.",
        studio_words: "genga",
        action: Action::RoomDrawing,
    },
    Step {
        stage: Some(Stage::Edit),
        label: vocab::STEP_TIMING,
        purpose: "Arrange when each drawing appears.",
        studio_words: "henshū",
        action: Action::RoomEdit,
    },
    Step {
        stage: Some(Stage::InkPaint),
        label: vocab::STEP_COLOR,
        purpose: "Fill your drawings with color.",
        studio_words: "shiage",
        action: Action::RoomFinishing,
    },
    Step {
        stage: Some(Stage::Composite),
        label: vocab::STEP_EFFECTS,
        purpose: "Add glow, blur and camera moves.",
        studio_words: "satsuei",
        action: Action::RoomComposite,
    },
    Step {
        stage: None,
        label: vocab::STEP_EXPORT,
        purpose: "Save a file to share.",
        studio_words: "",
        action: Action::ShowRenderQueue,
    },
];

/// The step that lenses `stage`, if it is on the spine.
pub fn of(stage: Stage) -> Option<&'static Step> {
    STEPS.iter().find(|s| s.stage == Some(stage))
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
