//! GM8: the sample cut, built procedurally (no binary assets): a BG plate
//! (sky gradient over ground), a ball bounce over 24 frames on twos — 12
//! drawings, the keys marked Key and the rest Inbetween — an empty column B
//! above it for practice, and a palette with line and paint roles. It opens
//! untitled; Save As keeps it.

use std::collections::BTreeMap;
use std::sync::Arc;

use anim_core::color::ColorLaw;
use anim_core::model::{CelLayer, Drawing, DrawingDef, DrawingKind};
use anim_core::raster::{RasterLayer, TILE, TILE_LEN, TileCoord, TileData};
use anim_core::xsheet::{ColumnDef, ColumnKind, Exposure};

use crate::doc::AppState;
use crate::palette::{CharacterPalette, ColorRole, Palettes};

/// Small enough to render in a blink on the CPU (the D0 test limit).
pub const SIZE: (u32, u32) = (640, 360);
pub const FRAMES: u32 = 24;
const GROUND: u32 = 290;
const RADIUS: f32 = 26.0;
const LINE: [u8; 4] = [26, 26, 26, 255];
const BALL: [u8; 4] = [214, 72, 60, 255];
/// The drawings (of 12) that are keys: the start, the contact, the end.
pub const KEYS: [usize; 3] = [0, 6, 11];

/// Rasterise `paint(x, y)` over the tiles covering `[x0, x1) × [y0, y1)`.
fn raster(
    (x0, y0, x1, y1): (i32, i32, i32, i32),
    paint: impl Fn(i32, i32) -> Option<[u8; 4]>,
) -> RasterLayer {
    let t = TILE as i32;
    let mut tiles: BTreeMap<TileCoord, Arc<TileData>> = BTreeMap::new();
    for ty in y0.div_euclid(t)..=(y1 - 1).div_euclid(t) {
        for tx in x0.div_euclid(t)..=(x1 - 1).div_euclid(t) {
            let mut rgba = vec![0u16; TILE_LEN];
            let mut any = false;
            for row in 0..t {
                for col in 0..t {
                    let (x, y) = (tx * t + col, ty * t + row);
                    if x < x0 || x >= x1 || y < y0 || y >= y1 {
                        continue;
                    }
                    if let Some(c) = paint(x, y) {
                        let i = ((row * t + col) * 4) as usize;
                        rgba[i..i + 4].copy_from_slice(&ColorLaw::Srgb.texel(c));
                        any = true;
                    }
                }
            }
            if any {
                tiles.insert((tx, ty), Arc::new(TileData::from_vec(rgba)));
            }
        }
    }
    RasterLayer { tiles }
}

/// The ball's centre on drawing `i` (of 12): falls from the left, meets the
/// ground on drawing 6, rises to the right.
pub fn ball_centre(i: usize) -> (f32, f32) {
    let s = i as f32 / 11.0;
    let x = 90.0 + s * 460.0;
    let top = 70.0;
    let floor = GROUND as f32 - RADIUS;
    let k = (i as f32 - 6.0) / 6.0; // -1 … 0 (contact) … 0.83
    (x, floor - (floor - top) * k * k)
}

/// The sample cut, untitled, history empty.
pub fn build_sample() -> AppState {
    let (w, h) = SIZE;
    let mut s = AppState::new_project("sample cut", w, h, 24, 300.0);
    s.engine.project.color_law = ColorLaw::Srgb;
    let at = s.at();
    let ball_col = s.view.active_column;
    let bg_col = s.engine.add_column(at, "BG").expect("fresh cut");
    // Empty: Quick start's first steps draw here, not over the ball.
    s.engine.add_column(at, "B").expect("fresh cut");
    let mut ids = Vec::new();
    for _ in 0..13 {
        ids.push((
            s.engine.alloc_drawing_id(),
            s.engine.alloc_layer_id(),
            s.engine.alloc_layer_id(),
        ));
    }
    let cut = s
        .engine
        .project
        .cut_mut(at.scene, at.cut)
        .expect("fresh cut");
    cut.frame_count = FRAMES;
    if let Some(c) = cut.xsheet.column_mut(ball_col) {
        c.name = "A".into();
    }
    // BG: a sky gradient over a flat ground.
    let (bg, bg_layer, _) = ids[0];
    let mut plate = Drawing::new(bg, "BG");
    let mut layer = CelLayer::new(bg_layer, "color");
    layer.raster = raster((0, 0, w as i32, h as i32), |_, y| {
        Some(if y as u32 >= GROUND {
            [104, 132, 88, 255]
        } else {
            let k = y as f32 / GROUND as f32;
            let mix = |a: f32, b: f32| (a + (b - a) * k).round() as u8;
            [mix(120.0, 236.0), mix(170.0, 226.0), mix(232.0, 206.0), 255]
        })
    });
    plate.layers.push(layer);
    cut.drawings.push(plate);
    cut.column_defs.insert(
        bg_col,
        ColumnDef {
            kind: ColumnKind::Bg,
            ..ColumnDef::default()
        },
    );
    if let Some(c) = cut.xsheet.column_mut(bg_col) {
        c.set_key(0, Exposure::Drawing(bg));
    }
    // The ball: paint under line, one drawing per two frames.
    for (i, &(id, paint_id, line_id)) in ids[1..].iter().enumerate() {
        let (cx, cy) = ball_centre(i);
        let r = RADIUS;
        let bounds = (
            (cx - r - 2.0) as i32,
            (cy - r - 2.0) as i32,
            (cx + r + 3.0) as i32,
            (cy + r + 3.0) as i32,
        );
        let dist = move |x: i32, y: i32| {
            ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt()
        };
        let mut d = Drawing::new(id, format!("A{}", i + 1));
        let mut paint = CelLayer::new(paint_id, "color");
        paint.raster = raster(bounds, |x, y| (dist(x, y) <= r).then_some(BALL));
        let mut line = CelLayer::new(line_id, "line");
        line.raster = raster(bounds, |x, y| {
            let e = dist(x, y);
            (e <= r + 1.5 && e >= r - 1.5).then_some(LINE)
        });
        d.layers.push(paint);
        d.layers.push(line);
        cut.drawings.push(d);
        let kind = if KEYS.contains(&i) {
            DrawingKind::Key
        } else {
            DrawingKind::Inbetween
        };
        cut.set_drawing_def(id, DrawingDef { kind, chart: None });
        if let Some(c) = cut.xsheet.column_mut(ball_col) {
            c.set_key(2 * i as u32, Exposure::Drawing(id));
        }
    }
    // The ball's column (and B after it) sits above the plate in the sheet.
    cut.xsheet.columns.sort_by_key(|c| c.id != bg_col);
    s.palettes = Palettes {
        characters: vec![CharacterPalette {
            name: "Ball".into(),
            roles: vec![
                ColorRole {
                    name: "line".into(),
                    color: LINE,
                },
                ColorRole {
                    name: "normal".into(),
                    color: BALL,
                },
                ColorRole {
                    name: "shadow".into(),
                    color: [150, 44, 62, 255],
                },
                ColorRole {
                    name: "highlight".into(),
                    color: [246, 160, 128, 255],
                },
            ],
        }],
    };
    s.auto_comp();
    s.engine.clear_history();
    s
}

/// The sample with its empty column B active on frame 0 — where Quick
/// start's "Show me" begins (a new drawing needs an empty cell).
pub fn build_practice() -> AppState {
    let mut s = build_sample();
    let b = s.cut().xsheet.columns.iter().find(|c| c.name == "B").map(|c| c.id);
    if let Some(b) = b {
        s.view.active_column = b;
    }
    s.view.frame = 0;
    s
}
