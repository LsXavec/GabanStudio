//! GM8: the sample cut, built procedurally (no binary assets): a BG plate
//! (sky gradient over ground), a ball bounce over 24 frames on twos — 12
//! drawings, the keys marked Key and the rest Inbetween — an empty column B
//! above it for practice, and a palette with line and paint roles. GM19:
//! the release's showcase on top. The BG plate runs past the frame's left
//! edge (the cut's paper) and a Camera node pans across it, easing onto the
//! frame by the last frame; the ball glows and diffuses, held to the frame
//! off the camera; sakura petals drift over the comp; and a short serifu
//! (dialogue) line generated in code rides the first track, so a delivery
//! has sound to mux and the sheet a waveform to read. It opens untitled;
//! Save As keeps it.

use std::collections::BTreeMap;
use std::sync::Arc;

use anim_core::audio::{TrackKind, TrackProps};
use anim_core::camera::{IRect, PAPER_KEY, paper_to_json};
use anim_core::color::ColorLaw;
use anim_core::command::Command;
use anim_core::fx::{FxKind, FxValue};
use anim_core::generator::Preset;
use anim_core::graph::{BlendMode, NodeKind, layout_key, pos_to_json};
use anim_core::look::effect_with;
use anim_core::model::{AudioClip, CelLayer, Drawing, DrawingDef, DrawingKind};
use anim_core::raster::{RasterLayer, TILE, TILE_LEN, TileCoord, TileData};
use anim_core::xsheet::{ColumnDef, ColumnKind, Exposure, ParamInterp, ParamKey};

use crate::doc::AppState;
use crate::palette::{CharacterPalette, ColorRole, Palettes};

/// Small enough to render in a blink on the CPU (the D0 test limit).
pub const SIZE: (u32, u32) = (640, 360);
pub const FRAMES: u32 = 24;
/// GM19: how far the BG plate — the cut's paper — runs left of the frame:
/// the camera pans this far, from the paper's left end onto the frame.
pub const PAN: u32 = 320;
const GROUND: u32 = 290;
const RADIUS: f32 = 26.0;
const LINE: [u8; 4] = [26, 26, 26, 255];
const BALL: [u8; 4] = [214, 72, 60, 255];
/// The drawings (of 12) that are keys: the start, the contact, the end.
pub const KEYS: [usize; 3] = [0, 6, 11];
/// GM19: the dialogue's sample rate.
pub const VOICE_RATE: u32 = 48_000;
/// GM19: the dialogue's syllables as (first, last) sheet frames — three
/// beats a lip flap opens on, silence between and at both ends.
pub const SYLLABLES: [(u32, u32); 3] = [(3, 8), (10, 15), (17, 21)];

/// GM19: the sample's serifu line, made in code: a 150 Hz voiced buzz (its
/// harmonics falling off as 1/k) under three syllable swells, as a 16-bit
/// mono WAV exactly as long as the cut. Deterministic — no noise, no clock.
pub fn dialogue_wav() -> Vec<u8> {
    use std::f32::consts::{PI, TAU};
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: VOICE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let per_frame = VOICE_RATE / 24;
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut out, spec).expect("a WAV header into memory");
        for i in 0..FRAMES * per_frame {
            let t = i as f32 / VOICE_RATE as f32;
            // The sheet frame this sample sounds on, fractional.
            let f = i as f32 / per_frame as f32;
            let swell = SYLLABLES
                .iter()
                .map(|&(a, b)| {
                    let (a, b) = (a as f32, b as f32 + 1.0);
                    if (a..b).contains(&f) {
                        (PI * (f - a) / (b - a)).sin().powi(2)
                    } else {
                        0.0
                    }
                })
                .fold(0.0, f32::max);
            let buzz: f32 = (1..=8)
                .map(|k| (TAU * 150.0 * k as f32 * t).sin() / k as f32)
                .sum();
            let s = 0.25 * swell * buzz;
            w.write_sample((s * 32767.0).round().clamp(-32768.0, 32767.0) as i16)
                .expect("into memory");
        }
        w.finalize().expect("into memory");
    }
    out.into_inner()
}

/// Rasterise `paint(x, y)` over the tiles covering `[x0, x1) × [y0, y1)`.
pub(crate) fn raster(
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

/// GM19: the BG plate at paper point (x, y): sky over far hills over a
/// furrowed ground — the hills and furrows change along x, so the pan
/// shows.
fn bg_texel(x: i32, y: i32) -> [u8; 4] {
    let (xf, yf) = (x as f32, y as f32);
    let hills = GROUND as f32 - 50.0 - 16.0 * (xf / 83.0).sin() - 9.0 * (xf / 31.0 + 1.0).sin();
    if y as u32 >= GROUND {
        if x.rem_euclid(40) < 3 {
            [92, 118, 78, 255]
        } else {
            [104, 132, 88, 255]
        }
    } else if yf >= hills {
        [128, 158, 136, 255]
    } else {
        let k = yf / GROUND as f32;
        let mix = |a: f32, b: f32| (a + (b - a) * k).round() as u8;
        [mix(120.0, 236.0), mix(170.0, 226.0), mix(232.0, 206.0), 255]
    }
}

/// GM19: the showcase graph's nodes, by part (the ship tests read them).
#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Showcase {
    pub camera: anim_core::ids::NodeId,
    pub glow: anim_core::ids::NodeId,
    pub diffusion: anim_core::ids::NodeId,
    pub particles: anim_core::ids::NodeId,
}

/// The sample cut, untitled, history empty.
pub fn build_sample() -> AppState {
    build_showcase().0
}

/// [`build_sample`], with the nodes its showcase graph placed.
pub fn build_showcase() -> (AppState, Showcase) {
    let (w, h) = SIZE;
    let mut s = AppState::new_project("sample cut", w, h, 24, 300.0);
    s.engine.project.color_law = ColorLaw::Srgb;
    let at = s.at();
    let ball_col = s.view.active_column;
    let bg_col = s.engine.add_column(at, "BG").expect("fresh cut");
    // Empty: Quick start's first steps draw here, not over the ball.
    let b_col = s.engine.add_column(at, "B").expect("fresh cut");
    // GM19: the camera's pan rides a param column on the sheet.
    let pan = s.engine.add_param_column(at, "Camera pan x").expect("fresh cut");
    let [bg_src, camera, ball_src, glow, diffusion, over_bg, b_src, over_b, petals, over_petals, out] =
        std::array::from_fn(|_| s.engine.alloc_node_id());
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
    // BG: sky, hills and ground over the whole paper — PAN px left of the
    // frame to its right edge (GM19).
    let (bg, bg_layer, _) = ids[0];
    let mut plate = Drawing::new(bg, "BG");
    let mut layer = CelLayer::new(bg_layer, "color");
    layer.raster = raster((-(PAN as i32), 0, w as i32, h as i32), |x, y| Some(bg_texel(x, y)));
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
        cut.set_drawing_def(
            id,
            DrawingDef {
                kind,
                chart: None,
                cut: None,
            },
        );
        if let Some(c) = cut.xsheet.column_mut(ball_col) {
            c.set_key(2 * i as u32, Exposure::Drawing(id));
        }
    }
    // The ball's column (and B after it) sits above the plate in the sheet.
    cut.xsheet.columns.sort_by_key(|c| c.id != bg_col);
    // GM19: the serifu line on the first track, from frame 0.
    cut.audio = Some(AudioClip::from_wav("serifu.wav", dialogue_wav()).expect("the WAV made above reads"));
    cut.audio_props = TrackProps::new("dialogue", TrackKind::Dialogue);
    // GM19: the satsuei showcase, one batch of ordinary, editable nodes,
    // each placed in the graph pane. The BG plate is the paper; the camera
    // films it (plane 0) and pans from its left end onto the frame, eased;
    // the ball — glow, then diffusion — and B ride over it, held to the
    // frame; the petals fall over everything.
    let paper = IRect {
        x: -(PAN as i32),
        y: 0,
        w: w + PAN,
        h,
    };
    let mut camera_kind = NodeKind::camera();
    if let NodeKind::Camera { binds, .. } = &mut camera_kind {
        binds.set("pan_x", Some(pan));
    }
    let mut petal_kind = NodeKind::particles(Preset::Sakura);
    if let NodeKind::Generator { params, .. } = &mut petal_kind {
        // The preset is tuned at 1080 lines; this frame has 360.
        params.set("size", FxValue::Float(7.0));
        params.set("size_end", FxValue::Float(6.0));
    }
    const COL: f32 = 220.0;
    const ROW: f32 = 90.0;
    let nodes = [
        (bg_src, NodeKind::source(bg_col), (0.0, 0.0)),
        (camera, camera_kind, (COL, 0.0)),
        (ball_src, NodeKind::source(ball_col), (0.0, ROW)),
        (
            glow,
            effect_with(
                FxKind::Glow,
                &[
                    ("threshold", FxValue::Float(0.25)),
                    ("radius", FxValue::Float(10.0)),
                    ("intensity", FxValue::Float(0.8)),
                ],
            ),
            (COL, ROW),
        ),
        (
            diffusion,
            effect_with(FxKind::Diffusion, &[("radius", FxValue::Float(8.0))]),
            (2.0 * COL, ROW),
        ),
        (over_bg, NodeKind::Blend { mode: BlendMode::Normal }, (3.0 * COL, ROW)),
        (b_src, NodeKind::source(b_col), (0.0, 2.0 * ROW)),
        (over_b, NodeKind::Blend { mode: BlendMode::Normal }, (4.0 * COL, 2.0 * ROW)),
        (petals, petal_kind, (0.0, 3.0 * ROW)),
        (over_petals, NodeKind::Blend { mode: BlendMode::Normal }, (5.0 * COL, 3.0 * ROW)),
        (out, NodeKind::Output, (6.0 * COL, 3.0 * ROW)),
    ];
    let wires = [
        (bg_src, camera, 0),
        (ball_src, glow, 0),
        (glow, diffusion, 0),
        (camera, over_bg, 0),
        (diffusion, over_bg, 1),
        (over_bg, over_b, 0),
        (b_src, over_b, 1),
        (over_b, over_petals, 0),
        (petals, over_petals, 1),
        (over_petals, out, 0),
    ];
    let key = |frame: u32, value: f32| Command::SetParamKey {
        at,
        column: pan,
        frame,
        key: Some(ParamKey {
            value,
            interp: ParamInterp::Ease,
            handles: None,
        }),
    };
    let mut cmds = vec![
        Command::SetCutMeta {
            at,
            key: PAPER_KEY.into(),
            before: None,
            after: Some(paper_to_json(paper)),
        },
        key(0, -(PAN as f32)),
        key(FRAMES - 1, 0.0),
    ];
    for (id, kind, pos) in nodes {
        cmds.push(Command::AddNode { at, id, kind });
        cmds.push(Command::SetCutMeta {
            at,
            key: layout_key(id),
            before: None,
            after: Some(pos_to_json(pos)),
        });
    }
    for (from, to, to_pin) in wires {
        cmds.push(Command::Connect {
            at,
            from,
            from_pin: 0,
            to,
            to_pin,
        });
    }
    cmds.push(Command::SetOutput { at, node: Some(out) });
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
            variants: Vec::new(),
        }],
    };
    s.engine.apply("sample showcase", cmds).expect("the showcase graph wires");
    s.engine.clear_history();
    let parts = Showcase {
        camera,
        glow,
        diffusion,
        particles: petals,
    };
    (s, parts)
}

/// The sample's sheet under GM8's plain graph — the auto-comp of its
/// columns, no camera or FX — history empty: where the tests of the
/// auto-comp, and of the satsuei template built over one, start.
#[cfg(test)]
pub fn build_auto_comp() -> AppState {
    let mut s = build_sample();
    assert!(s.auto_comp());
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
