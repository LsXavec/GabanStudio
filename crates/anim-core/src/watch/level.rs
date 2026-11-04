//! Workflow Watch levels and their thresholds, as plain numbers.

/// How finely the checks judge. The app maps these onto the lessons' tiers.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Level {
    #[default]
    Rookie,
    Amateur,
}

impl Level {
    pub const ALL: [Level; 2] = [Level::Rookie, Level::Amateur];

    /// The level a full retirement suggests; the top level has none.
    pub fn next(self) -> Option<Level> {
        match self {
            Level::Rookie => Some(Level::Amateur),
            Level::Amateur => None,
        }
    }

    pub fn thresholds(self) -> Thresholds {
        match self {
            Level::Rookie => Thresholds {
                undo_count: 6,
                key_ratio: 0.5,
                polish_secs: 180.0,
                allowed_per_cut: 2,
                retire_after: 3,
            },
            Level::Amateur => Thresholds {
                undo_count: 4,
                key_ratio: 1.0,
                polish_secs: 90.0,
                allowed_per_cut: 1,
                retire_after: 3,
            },
        }
    }
}

/// The numbers that differ by level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    /// Undo loop: this many overlapping undos or erases fire the check.
    pub undo_count: usize,
    /// Key check: a key fires under this share of its in-betweens' mean.
    pub key_ratio: f64,
    /// Over-polishing: this much active time fires the check.
    pub polish_secs: f64,
    /// Fires of one check a cut may hold and still count as clean.
    pub allowed_per_cut: u32,
    /// Clean cuts in a row that retire a check.
    pub retire_after: u32,
}

/// Undo loop: the run must fit in this many real seconds.
pub const UNDO_WINDOW_SECS: f64 = 60.0;
/// Undo loop: overlap needed, as a share of the smaller box.
pub const UNDO_OVERLAP: f64 = 0.5;
/// Over-polishing: the zoom the work must be at or above (4.0 = 400%).
pub const POLISH_ZOOM: f32 = 4.0;
/// Over-polishing: the worked region must stay under this share of the paper.
pub const POLISH_REGION: f64 = 0.05;
/// Over-polishing: real seconds without a stroke that end "continuous" work.
pub const POLISH_GAP_SECS: f64 = 30.0;
/// One frame adds at most this much active time: frames come on demand, so
/// one gap between them can be hours.
pub const MAX_FRAME_SECS: f64 = 1.0;
