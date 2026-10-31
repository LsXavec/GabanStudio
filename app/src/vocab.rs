//! THE DISPLAY WORDS (AU1): one constant per row of the vocabulary table in
//! research/PSD-approachable-ui.md section 3, shared by shipped code and
//! tests, so a rename is one line here and a test that pins a word moves
//! with it. `auseam_vocab_table_is_pinned` holds the table.
//!
//! A constant is named for the word its row ends as; its value is the word
//! on screen today. Stored vocabulary (`Pane`, `Action`, room keys, stack and
//! preset names) never reads these: only what the screen says does.

// The table is declared whole; panels start reading it as their rows flip.
#![allow(dead_code)]

/// `step`, as the menu and the status line say it.
pub const STEP: &str = "step";
/// The network room: "in a room" is what the lamp says of it.
pub const SHARED_SESSION: &str = "in a room";
pub const ARRANGEMENT: &str = "workspace";

// The steps. `Stage::name()` and `steps::STEPS` read these.
pub const STEP_PLAN: &str = "Plan";
pub const STEP_DRAW: &str = "Draw";
pub const STEP_COLOR: &str = "Color";
pub const STEP_EFFECTS: &str = "Effects";
pub const STEP_TIMING: &str = "Timing";
pub const STEP_MANGA: &str = "Manga";
pub const STEP_EXPORT: &str = "Export";

// The Timing sheet button: its caption, and what it says aloud by what its
// click will do.
pub const SHEET_BUTTON: &str = "Timing sheet";
pub const SHOW_TIMING_SHEET: &str = "Show Timing sheet";
pub const HIDE_TIMING_SHEET: &str = "Hide Timing sheet";

pub const BRUSH_MAKER: &str = "Brush Forge";
pub const LAYOUT_3D: &str = "3D layout";
pub const SHOT: &str = "cut";
pub const DRAWING: &str = "cel";
pub const TRACK: &str = "column";
pub const TIMING_SHEET: &str = "X-Sheet";
pub const GHOST_FRAMES: &str = "onion";

// The pencil box, as its legends print the preset names.
pub const SKETCH_PENCIL: &str = "ATARI";
pub const CLEAN_PENCIL: &str = "GENGA";
pub const CORRECTION_PENCIL: &str = "SHUSEI";

pub const DRAW_TO_FILL: &str = "lasso fill";
pub const KEEP_INSIDE_STROKES: &str = "alpha lock";
pub const KEEP_INSIDE_STROKES_SHORT: &str = "lock";
pub const PREVIEW_FINAL: &str = "composite view";
pub const PREVIEW_FINAL_SHORT: &str = "comp";
pub const EDGE_SOFTNESS: &str = "HARDNESS";
pub const PIXEL_EDGES: &str = "aliased";
pub const STEADY_STROKE: &str = "STABILIZER";
pub const TRACKS: &str = "COLUMNS";
pub const LENGTH: &str = "SHAKU";
pub const NOTES: &str = "MEMO";
pub const DRAWINGS: &str = "library";
pub const LAYERS: &str = "Cel Layers";
pub const BRUSHES: &str = "Presets";
pub const LAYER_SET: &str = "stack";
/// Retired: the row's word leaves the screen and `Drawing with` takes its
/// place in the hover card.
pub const DRAWING_WITH: &str = "ARMED";
pub const EXPORT_QUEUE: &str = "Render Queue";
pub const EFFECTS_GRAPH: &str = "Node Graph";
pub const FULL_FINISHING_SETUP: &str = "Build satsuei graph";
pub const REBUILD_FROM_LAYERS: &str = "Build composite from sheet";
pub const RESULT: &str = "Viewer";
/// The Layout room's canvas pane: the cut's preview, under the Canvas tab.
pub const PREVIEW: &str = "Canvas";
pub const MOTION_CURVES: &str = "Curves";
pub const EDIT_COLORS: &str = "EDIT MODEL";
pub const COLOR_NAME: &str = "role";
pub const PROTECT_LINE_ART: &str = "line guard";
pub const REMOVE_FROM_TIMELINE: &str = "LIFT KEY";
pub const SHOTS: &str = "Shinkō board";
pub const EPISODE_TIMELINE: &str = "Henshū timeline";
pub const ANIMATE: &str = "bind";
pub const ZOOM_IN: &str = "T.U.";
pub const ZOOM_OUT: &str = "T.B.";
pub const LONGER: &str = "Hold +1";
pub const SHORTER: &str = "Hold -1";

// The stacks, as the strip lists them. Display only: the stored names stay.
pub const ONE_LAYER: &str = "Solo merged";
pub const ROUGH_OVER_REFERENCE: &str = "Layout (LO)";
pub const CLEAN_LINE_OVER_ROUGH: &str = "Genga";
pub const FULL_PAINT: &str = "Shiage (full)";

// The sheet head's marks and what they say aloud.
pub const HIDE_MARK: &str = "H";
pub const LOCK_MARK: &str = "L";
pub const HIDE_TRACK: &str = "hide column";
pub const LOCK_TRACK: &str = "lock column";

pub const NEXT_DRAWING: &str = "Next key";
pub const PREVIOUS_DRAWING: &str = "Previous key";

// Legends a test reads back, as the plate paints them.
pub const APP_NAME: &str = "Gaban Studio";
pub const SAVED_STATE: &str = "Saved";
pub const HORIZON_Y: &str = "Horizon Y";
pub const MANGA_PANE: &str = "Manga (pages)";
pub const CANVAS_GESTURES: &str = "Canvas gestures";
pub const LIP_FLAP_TITLE: &str = "Kuchipaku (lip flap)";

/// Today's word for every constant above, in the table's order: the pin.
#[cfg(test)]
pub const TABLE: &[(&str, &str, &str)] = &[
    ("STEP", STEP, "step"),
    ("SHARED_SESSION", SHARED_SESSION, "in a room"),
    ("ARRANGEMENT", ARRANGEMENT, "workspace"),
    ("STEP_PLAN", STEP_PLAN, "Plan"),
    ("STEP_DRAW", STEP_DRAW, "Draw"),
    ("STEP_COLOR", STEP_COLOR, "Color"),
    ("STEP_EFFECTS", STEP_EFFECTS, "Effects"),
    ("STEP_TIMING", STEP_TIMING, "Timing"),
    ("STEP_MANGA", STEP_MANGA, "Manga"),
    ("STEP_EXPORT", STEP_EXPORT, "Export"),
    ("SHEET_BUTTON", SHEET_BUTTON, "Timing sheet"),
    ("SHOW_TIMING_SHEET", SHOW_TIMING_SHEET, "Show Timing sheet"),
    ("HIDE_TIMING_SHEET", HIDE_TIMING_SHEET, "Hide Timing sheet"),
    ("BRUSH_MAKER", BRUSH_MAKER, "Brush Forge"),
    ("LAYOUT_3D", LAYOUT_3D, "3D layout"),
    ("SHOT", SHOT, "cut"),
    ("DRAWING", DRAWING, "cel"),
    ("TRACK", TRACK, "column"),
    ("TIMING_SHEET", TIMING_SHEET, "X-Sheet"),
    ("GHOST_FRAMES", GHOST_FRAMES, "onion"),
    ("SKETCH_PENCIL", SKETCH_PENCIL, "ATARI"),
    ("CLEAN_PENCIL", CLEAN_PENCIL, "GENGA"),
    ("CORRECTION_PENCIL", CORRECTION_PENCIL, "SHUSEI"),
    ("DRAW_TO_FILL", DRAW_TO_FILL, "lasso fill"),
    ("KEEP_INSIDE_STROKES", KEEP_INSIDE_STROKES, "alpha lock"),
    ("KEEP_INSIDE_STROKES_SHORT", KEEP_INSIDE_STROKES_SHORT, "lock"),
    ("PREVIEW_FINAL", PREVIEW_FINAL, "composite view"),
    ("PREVIEW_FINAL_SHORT", PREVIEW_FINAL_SHORT, "comp"),
    ("EDGE_SOFTNESS", EDGE_SOFTNESS, "HARDNESS"),
    ("PIXEL_EDGES", PIXEL_EDGES, "aliased"),
    ("STEADY_STROKE", STEADY_STROKE, "STABILIZER"),
    ("TRACKS", TRACKS, "COLUMNS"),
    ("LENGTH", LENGTH, "SHAKU"),
    ("NOTES", NOTES, "MEMO"),
    ("DRAWINGS", DRAWINGS, "library"),
    ("LAYERS", LAYERS, "Cel Layers"),
    ("BRUSHES", BRUSHES, "Presets"),
    ("LAYER_SET", LAYER_SET, "stack"),
    ("DRAWING_WITH", DRAWING_WITH, "ARMED"),
    ("EXPORT_QUEUE", EXPORT_QUEUE, "Render Queue"),
    ("EFFECTS_GRAPH", EFFECTS_GRAPH, "Node Graph"),
    ("FULL_FINISHING_SETUP", FULL_FINISHING_SETUP, "Build satsuei graph"),
    ("REBUILD_FROM_LAYERS", REBUILD_FROM_LAYERS, "Build composite from sheet"),
    ("RESULT", RESULT, "Viewer"),
    ("PREVIEW", PREVIEW, "Canvas"),
    ("MOTION_CURVES", MOTION_CURVES, "Curves"),
    ("EDIT_COLORS", EDIT_COLORS, "EDIT MODEL"),
    ("COLOR_NAME", COLOR_NAME, "role"),
    ("PROTECT_LINE_ART", PROTECT_LINE_ART, "line guard"),
    ("REMOVE_FROM_TIMELINE", REMOVE_FROM_TIMELINE, "LIFT KEY"),
    ("SHOTS", SHOTS, "Shinkō board"),
    ("EPISODE_TIMELINE", EPISODE_TIMELINE, "Henshū timeline"),
    ("ANIMATE", ANIMATE, "bind"),
    ("ZOOM_IN", ZOOM_IN, "T.U."),
    ("ZOOM_OUT", ZOOM_OUT, "T.B."),
    ("LONGER", LONGER, "Hold +1"),
    ("SHORTER", SHORTER, "Hold -1"),
    ("ONE_LAYER", ONE_LAYER, "Solo merged"),
    ("ROUGH_OVER_REFERENCE", ROUGH_OVER_REFERENCE, "Layout (LO)"),
    ("CLEAN_LINE_OVER_ROUGH", CLEAN_LINE_OVER_ROUGH, "Genga"),
    ("FULL_PAINT", FULL_PAINT, "Shiage (full)"),
    ("HIDE_MARK", HIDE_MARK, "H"),
    ("LOCK_MARK", LOCK_MARK, "L"),
    ("HIDE_TRACK", HIDE_TRACK, "hide column"),
    ("LOCK_TRACK", LOCK_TRACK, "lock column"),
    ("NEXT_DRAWING", NEXT_DRAWING, "Next key"),
    ("PREVIOUS_DRAWING", PREVIOUS_DRAWING, "Previous key"),
    ("APP_NAME", APP_NAME, "Gaban Studio"),
    ("SAVED_STATE", SAVED_STATE, "Saved"),
    ("HORIZON_Y", HORIZON_Y, "Horizon Y"),
    ("MANGA_PANE", MANGA_PANE, "Manga (pages)"),
    ("CANVAS_GESTURES", CANVAS_GESTURES, "Canvas gestures"),
    ("LIP_FLAP_TITLE", LIP_FLAP_TITLE, "Kuchipaku (lip flap)"),
];
