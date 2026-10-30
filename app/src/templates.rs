//! GM8: New from template — the Welcome cards' data. Every template makes
//! an Srgb project (D8) with 72- or 120-frame cuts (D13), its starting
//! columns, an auto-comp graph (`doc::build_auto_comp`) and a first room;
//! the room's cel-stack preset arms when the editor enters it.

use anim_core::color::ColorLaw;
use anim_core::manga::Page;
use anim_core::xsheet::{ColumnDef, ColumnKind};

use crate::doc::AppState;
use crate::workspace::Stage;

pub struct Template {
    pub name: &'static str,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub cut_frames: u32,
    pub room: Stage,
    /// Sheet order, left to right, with each column's kind.
    pub columns: &'static [(&'static str, ColumnKind)],
    /// GM18: a manga template's page (its geometry in the cut meta, the
    /// page graph instead of the auto-comp); None for an animation.
    pub page: Option<fn() -> Page>,
}

/// GM18: a page's columns — the ink, and the tone painted in greys.
const PAGE_COLUMNS: &[(&str, ColumnKind)] = &[("INK", ColumnKind::Cel), ("TONE", ColumnKind::Cel)];

const BG: (&str, ColumnKind) = ("BG", ColumnKind::Bg);
const A: (&str, ColumnKind) = ("A", ColumnKind::Cel);
const B: (&str, ColumnKind) = ("B", ColumnKind::Cel);
const C: (&str, ColumnKind) = ("C", ColumnKind::Cel);
const D: (&str, ColumnKind) = ("D", ColumnKind::Cel);

pub const ALL: &[Template] = &[
    Template {
        name: "TV anime HD",
        width: 1920,
        height: 1080,
        fps: 24,
        cut_frames: 72,
        room: Stage::Layout,
        columns: &[BG, A, B, C],
        page: None,
    },
    Template {
        name: "Web 720p",
        width: 1280,
        height: 720,
        fps: 24,
        cut_frames: 72,
        room: Stage::Drawing,
        columns: &[BG, A, B],
        page: None,
    },
    Template {
        name: "Feature UHD",
        width: 3840,
        height: 2160,
        fps: 24,
        cut_frames: 72,
        room: Stage::Layout,
        columns: &[BG, A, B, C, D],
        page: None,
    },
    Template {
        name: "Animatic / storyboard",
        width: 1920,
        height: 1080,
        fps: 24,
        cut_frames: 120,
        room: Stage::Edit,
        // The boards are the picture (a BG-band column, so they render and
        // export), the A column draws over them.
        columns: &[("BOARD", ColumnKind::Bg), A],
        page: None,
    },
    // GM18 (D15): a book of one-frame pages at 350 dpi, in the manga room.
    Template {
        name: "Manga B5",
        width: 2591,
        height: 3624,
        fps: 24,
        cut_frames: 1,
        room: Stage::Manga,
        columns: PAGE_COLUMNS,
        page: Some(Page::b5),
    },
    Template {
        name: "Manga A4",
        width: 2976,
        height: 4175,
        fps: 24,
        cut_frames: 1,
        room: Stage::Manga,
        columns: PAGE_COLUMNS,
        page: Some(Page::a4),
    },
    Template {
        name: "Webtoon",
        width: 800,
        height: 6400,
        fps: 24,
        cut_frames: 1,
        room: Stage::Manga,
        columns: PAGE_COLUMNS,
        page: Some(Page::webtoon),
    },
];

/// GM21: the sizes the New Project dialog offers — every template's under
/// its name, then two no template has. The dialog keeps no table of its own.
pub const SIZES: &[(&str, u32, u32)] = &{
    let mut out = [("", 0, 0); ALL.len() + 2];
    let mut i = 0;
    while i < ALL.len() {
        out[i] = (ALL[i].name, ALL[i].width, ALL[i].height);
        i += 1;
    }
    out[ALL.len()] = ("Square 1080", 1080, 1080);
    out[ALL.len() + 1] = ("Vertical 1080×1920", 1080, 1920);
    out
};

impl Template {
    /// GM21: the print resolution — the page's (D15), else 300 dpi.
    pub fn dpi(&self) -> f32 {
        self.page.map_or(300.0, |p| p().dpi)
    }

    /// The card's second line: size, fps and cut length.
    pub fn summary(&self) -> String {
        if let Some(page) = self.page.map(|p| p()) {
            return format!("{}×{} · {} at {} dpi", self.width, self.height, page.format, page.dpi);
        }
        format!(
            "{}×{} · {} fps · {} frames",
            self.width, self.height, self.fps, self.cut_frames
        )
    }

    /// The card's hover: the room it opens in and the columns it starts with.
    pub fn tooltip(&self) -> String {
        let columns: Vec<&str> = self.columns.iter().map(|(name, _)| *name).collect();
        format!(
            "opens in the {} room · columns {}",
            self.room.name(),
            columns.join(", ")
        )
    }

    /// A fresh untitled project to this template (history empty).
    pub fn build(&self) -> AppState {
        let mut s = AppState::new_project("untitled", self.width, self.height, self.fps, self.dpi());
        s.engine.project.color_law = ColorLaw::Srgb;
        s.engine.project.default_cut_frames = Some(self.cut_frames);
        let at = s.at();
        let first = s.view.active_column;
        let mut active = first;
        for (i, (name, kind)) in self.columns.iter().enumerate() {
            let id = if i == 0 {
                first
            } else {
                s.engine.add_column(at, *name).expect("fresh cut")
            };
            let cut = s
                .engine
                .project
                .cut_mut(at.scene, at.cut)
                .expect("fresh cut");
            cut.frame_count = self.cut_frames;
            if let Some(col) = cut.xsheet.column_mut(id) {
                col.name = name.to_string();
            }
            if *kind != ColumnKind::Cel {
                cut.column_defs.insert(
                    id,
                    ColumnDef {
                        kind: *kind,
                        ..ColumnDef::default()
                    },
                );
            } else if active == first && self.columns[0].1 != ColumnKind::Cel {
                // The pen starts on the first cel column, not the plate.
                active = id;
            }
        }
        s.view.active_column = active;
        match self.page.map(|p| p()) {
            Some(page) => {
                s.engine.project.dpi = page.dpi;
                if let Some(cut) = s.engine.project.cut_mut(at.scene, at.cut) {
                    cut.name = "P01".into();
                }
                s.make_page(&page);
            }
            None => {
                s.auto_comp();
            }
        }
        s.engine.clear_history();
        s
    }
}
