//! GM8: New from template — the Welcome cards' data. Every template makes
//! an Srgb project (D8) with 72- or 120-frame cuts (D13), its starting
//! columns, an auto-comp graph (`doc::build_auto_comp`) and a first room;
//! the room's cel-stack preset arms when the editor enters it.

use anim_core::color::ColorLaw;
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
}

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
    },
    Template {
        name: "Web 720p",
        width: 1280,
        height: 720,
        fps: 24,
        cut_frames: 72,
        room: Stage::Drawing,
        columns: &[BG, A, B],
    },
    Template {
        name: "Feature UHD",
        width: 3840,
        height: 2160,
        fps: 24,
        cut_frames: 72,
        room: Stage::Layout,
        columns: &[BG, A, B, C, D],
    },
    Template {
        name: "Animatic / storyboard",
        width: 1920,
        height: 1080,
        fps: 24,
        cut_frames: 120,
        room: Stage::Edit,
        columns: &[("BOARD", ColumnKind::Ref), A],
    },
];

impl Template {
    /// The card's second line: size, fps and cut length.
    pub fn summary(&self) -> String {
        format!(
            "{}×{} · {} fps · {} frames",
            self.width, self.height, self.fps, self.cut_frames
        )
    }

    /// A fresh untitled project to this template (history empty).
    pub fn build(&self) -> AppState {
        let mut s = AppState::new_project("untitled", self.width, self.height, self.fps, 300.0);
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
        s.auto_comp();
        s.engine.clear_history();
        s
    }
}
