//! THE DISPLAY WORDS (AU1): one constant per row of the vocabulary table in
//! research/PSD-approachable-ui.md section 3, shared by shipped code and
//! tests, so a rename is one line here and a test that pins a word moves
//! with it. `auseam_vocab_table_is_pinned` holds the table.
//!
//! AU12: the table is final. Stored vocabulary (`Pane`, `Action`, room keys,
//! stack and preset names) never reads these: only what the screen says does.
//! The maps at the foot turn a stored name into its display word, and an old
//! word stays a Search alias in `Action::aliases`.

/// `step`, as the menu and the status line say it.
pub const STEP: &str = "step";
/// The network room, as the lamp says it.
pub const SHARED_SESSION: &str = "shared session";
pub const ARRANGEMENT: &str = "arrangement";

// The six menus on the top row (spec 6.2).
pub const MENU_FILE: &str = "File";
pub const MENU_EDIT: &str = "Edit";
pub const MENU_VIEW: &str = "View";
pub const MENU_ANIMATION: &str = "Animation";
pub const MENU_HELP: &str = "Help";
pub const MENU_MORE: &str = "More";

// The steps. `Stage::name()` and `steps::CATALOG` read these.
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

pub const BRUSH_MAKER: &str = "Brush maker";
pub const LAYOUT_3D: &str = "3D layout";
pub const SHOT: &str = "shot";
pub const DRAWING: &str = "drawing";
pub const TRACK: &str = "track";
pub const TIMING_SHEET: &str = "Timing sheet";
pub const GHOST_FRAMES: &str = "Ghost frames";
/// The old `safe` latch, on the canvas and on the Result view.
pub const MARGINS: &str = "Margins";

// The pencil box, as its legends print the preset names.
pub const SKETCH_PENCIL: &str = "Sketch pencil";
pub const CLEAN_PENCIL: &str = "Clean pencil";
pub const CORRECTION_PENCIL: &str = "Correction pencil";

pub const DRAW_TO_FILL: &str = "Draw to fill";
pub const KEEP_INSIDE_STROKES: &str = "Keep inside strokes";
pub const KEEP_INSIDE_STROKES_SHORT: &str = "Keep inside strokes";
pub const PREVIEW_FINAL: &str = "Preview final";
pub const PREVIEW_FINAL_SHORT: &str = "Preview final";
/// The Result's latch and `ToggleFinalLook`: the picture as the export gives it.
pub const FULL_QUALITY: &str = "Full quality";
pub const EDGE_SOFTNESS: &str = "Edge softness";
pub const PIXEL_EDGES: &str = "Pixel edges";
pub const STEADY_STROKE: &str = "Steady stroke";
pub const TRACKS: &str = "Tracks";
pub const LENGTH: &str = "Length";
pub const NOTES: &str = "Notes";
/// The sound track kind the old `serifu (dialogue)` label named.
pub const DIALOGUE: &str = "Dialogue";
pub const DRAWINGS: &str = "Drawings";
pub const LAYERS: &str = "Layers";
pub const BRUSHES: &str = "Brushes";
// The rail's two left floats that have no older word.
pub const TOOL_OPTIONS: &str = "Tool options";
pub const CANVAS_OPTIONS: &str = "Canvas options";
pub const LAYER_SET: &str = "Layer set";
pub const DRAWING_WITH: &str = "Drawing with";
pub const EXPORT_QUEUE: &str = "Export queue";
pub const EFFECTS_GRAPH: &str = "Effects graph";
pub const FULL_FINISHING_SETUP: &str = "Full finishing setup";
pub const REBUILD_FROM_LAYERS: &str = "Rebuild from layers";
// The Effects shelf's own words (AU14). Its effects read `fx` below.
pub const STRENGTH: &str = "Strength";
pub const ADD_EFFECT: &str = "Add effect";
pub const CAMERA_MOVE: &str = "Camera move";
pub const GO_TO_LAST_DRAWING: &str = "Go to the last drawing";
// The Comic step's own words (AU22).
pub const SEE_THE_PAGE: &str = "See the page";
pub const PAGE_GUIDES: &str = "Page guides";
pub const ADD_BALLOON: &str = "Add balloon";
pub const ADD_TEXT: &str = "Add text";
pub const SPEED_LINES: &str = "Speed lines";
pub const MONO: &str = "Mono";
pub const DETAILS: &str = "Details";
pub const RESULT: &str = "Result";
/// The Layout room's canvas pane: the shot's preview, under the Canvas tab.
pub const PREVIEW: &str = "Preview";
pub const MOTION_CURVES: &str = "Motion curves";
pub const EDIT_COLORS: &str = "Edit colors";
pub const COLOR_NAME: &str = "Color name";
pub const COLOR_WELL: &str = "color well";
pub const KEPT_COLOR: &str = "kept color";
pub const COLORS: &str = "Colors";
pub const ADD_COLOR: &str = "Add color";
pub const COLOR_DETAILS: &str = "Color details";
pub const COLOR_SET: &str = "Color set";
pub const COLOR_SETS: &str = "Color sets";
pub const RESULT_BESIDE: &str = "Result beside";
pub const GO_TO_THIS_DRAWING: &str = "Go to this drawing";
pub const PROTECT_LINE_ART: &str = "Protect line art";
pub const REMOVE_FROM_TIMELINE: &str = "Remove from timeline";
pub const SHOTS: &str = "Shots";
pub const EPISODE_TIMELINE: &str = "Episode timeline";
pub const ANIMATE: &str = "Animate";
pub const ZOOM_IN: &str = "Zoom in";
pub const ZOOM_OUT: &str = "Zoom out";
pub const LONGER: &str = "Longer";
pub const SHORTER: &str = "Shorter";

// The stacks, as the strip lists them. Display only: the stored names stay.
pub const ONE_LAYER: &str = "One layer";
pub const ROUGH_OVER_REFERENCE: &str = "Rough over reference";
pub const CLEAN_LINE_OVER_ROUGH: &str = "Clean line over rough";
pub const FULL_PAINT: &str = "Full paint";

// What the sheet head's eye and lock marks say aloud.
pub const HIDE_TRACK: &str = "Hide track";
pub const LOCK_TRACK: &str = "Lock track";

pub const NEXT_DRAWING: &str = "Next drawing";
pub const PREVIOUS_DRAWING: &str = "Previous drawing";

// The full transport's words (spec 4.4). They arrived with the band: the old
// `scrub`, `in` and `out` stay Search aliases.
pub const SOUND_WHILE_DRAGGING: &str = "Sound while dragging";
pub const FPS: &str = "fps";
pub const LOOP: &str = "Loop";
pub const LOOP_START: &str = "Loop start";
pub const LOOP_END: &str = "Loop end";

// Legends a test reads back, as the plate paints them.
pub const APP_NAME: &str = "Gaban Studio";
pub const SAVED_STATE: &str = "Saved";
pub const HORIZON_Y: &str = "Horizon Y";
pub const MANGA_PANE: &str = "Manga (pages)";
pub const CANVAS_GESTURES: &str = "Canvas gestures";
pub const LIP_FLAP_TITLE: &str = "Lip flap";

/// A stack's stored name as the screen says it. Stored names stay
/// (`config.rs` matches them by string); a name the user gave shows as typed.
pub fn stack(stored: &str) -> &str {
    match stored {
        "Solo merged" => ONE_LAYER,
        "Layout (LO)" => ROUGH_OVER_REFERENCE,
        "Genga" => CLEAN_LINE_OVER_ROUGH,
        "Shiage (full)" => FULL_PAINT,
        // Section 3 leaves this one out: it says what the set holds.
        "Douga" => "Clean line",
        other => other,
    }
}

/// A brush preset's stored name as the screen says it. Presets are re-inserted
/// by name, so the stored name stays; a preset the user named shows as typed.
pub fn preset(stored: &str) -> &str {
    match stored {
        "atari" => SKETCH_PENCIL,
        "genga" => CLEAN_PENCIL,
        "shusei" => CORRECTION_PENCIL,
        "shiage fill" => "Color fill",
        "shadow airbrush" => "Shadow airbrush",
        other => other,
    }
}

/// An FX node's stored name as the screen says it (spec 3, FX display map).
pub fn fx(stored: &str) -> &str {
    match stored {
        "Diffusion" => "Soft focus",
        "Grade" => "Color look",
        "Para" => "Color wash",
        "LUT" => "Color table",
        "Matte" => "Cut-out",
        "DrawingSource" => DRAWINGS,
        "ImageSource" => "Image",
        "ImageSequence" => "Image sequence",
        "Model3D" => "3D model",
        other => other,
    }
}

/// A sound track's kind. `TrackKind::key` is what the file stores.
pub fn track_kind(kind: anim_core::audio::TrackKind) -> &'static str {
    use anim_core::audio::TrackKind::*;
    match kind {
        Dialogue => DIALOGUE,
        Effects => "Sound effects",
        Music => "Music",
        Other => "Other",
    }
}

/// A track's kind as its head says it. `ColumnKind` is stored by name.
pub fn column_kind(kind: anim_core::xsheet::ColumnKind) -> &'static str {
    use anim_core::xsheet::ColumnKind::*;
    match kind {
        Cel => "Drawing",
        Bg => "Background",
        Book => "Overlay",
        Ref => "Reference",
    }
}

/// The same in the sheet head's 40 pt chip; the card and the spoken name say
/// the whole word.
pub fn column_kind_chip(kind: anim_core::xsheet::ColumnKind) -> &'static str {
    use anim_core::xsheet::ColumnKind::*;
    match kind {
        Cel => "Draw",
        Bg => "Back",
        Book => "Over",
        Ref => "Ref",
    }
}

/// A layer's color name (the old "role"). `LayerRole::key` is what is stored.
pub fn layer_role(role: anim_core::model::LayerRole) -> &'static str {
    use anim_core::model::LayerRole::*;
    match role {
        Reference => "Reference",
        Color => "Color",
        Shadow => "Shadow",
        Highlight => "Highlight",
        Rough => "Rough",
        ShadowLine => "Shadow line",
        HighlightLine => "Highlight line",
        Line => "Line art",
        Correction => "Correction",
        Custom => "Custom",
    }
}

/// A shot's production status. `CutStatus::key` is what the file stores.
pub fn cut_status(status: anim_core::board::CutStatus) -> &'static str {
    use anim_core::board::CutStatus::*;
    match status {
        Lo => "Plan",
        Genga => "Key poses",
        Sakkan => "Art check",
        Douga => "Inbetweens",
        Paint => "Color",
        Composite => "Effects",
        Done => "Done",
        Retake => "Retake",
    }
}

/// What a drawing is marked as (`Mark as ▸`). Key pose, Breakdown and
/// Inbetween stay, by spec 3.
pub fn drawing_kind(kind: anim_core::model::DrawingKind) -> &'static str {
    use anim_core::model::DrawingKind::*;
    match kind {
        Unset => "Unmarked",
        Key => "Key pose",
        Breakdown => "Breakdown",
        Inbetween => "Inbetween",
    }
}

/// The studio's own word for a label, for its hover card's second line.
pub fn studio_word(label: &str) -> Option<&'static str> {
    // A menu row's "…" is not part of the word (`Lip flap…`).
    Some(match label.trim_end_matches('…') {
        LENGTH => "shaku",
        NOTES | DIALOGUE => "serifu",
        SKETCH_PENCIL => "atari",
        CLEAN_PENCIL | CLEAN_LINE_OVER_ROUGH | "Key poses" => "genga",
        CORRECTION_PENCIL => "shusei",
        FULL_PAINT => "shiage",
        FULL_FINISHING_SETUP => "satsuei",
        SHOTS => "shinkō board",
        EPISODE_TIMELINE => "henshū",
        LIP_FLAP_TITLE => "kuchipaku",
        "Art check" => "sakkan",
        "Inbetweens" => "douga",
        _ => return None,
    })
}

/// A hover card's words with that second line, where the label has one.
pub fn with_studio_word(label: &str, card: &str) -> String {
    match studio_word(label) {
        Some(word) => format!("{card}\nStudio word: {word}"),
        None => card.to_string(),
    }
}

/// A parameter track's stored name (found by name in old files), as said.
pub fn param_track(stored: &str) -> String {
    stored.replacen("cut camera", "shot camera", 1)
}

/// The wire's sentences keep their word (net.rs is pinned whole, and the
/// host's refusal crosses the network); the screen says the plain one.
pub fn session_words(why: &str) -> String {
    why.replace("room key", "session key").replace("room closed", "session closed")
}

/// The final word of every constant above, in the table's order: the pin.
#[cfg(test)]
pub const TABLE: &[(&str, &str, &str)] = &[
    ("STEP", STEP, "step"),
    ("SHARED_SESSION", SHARED_SESSION, "shared session"),
    ("ARRANGEMENT", ARRANGEMENT, "arrangement"),
    ("MENU_FILE", MENU_FILE, "File"),
    ("MENU_EDIT", MENU_EDIT, "Edit"),
    ("MENU_VIEW", MENU_VIEW, "View"),
    ("MENU_ANIMATION", MENU_ANIMATION, "Animation"),
    ("MENU_HELP", MENU_HELP, "Help"),
    ("MENU_MORE", MENU_MORE, "More"),
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
    ("BRUSH_MAKER", BRUSH_MAKER, "Brush maker"),
    ("LAYOUT_3D", LAYOUT_3D, "3D layout"),
    ("SHOT", SHOT, "shot"),
    ("DRAWING", DRAWING, "drawing"),
    ("TRACK", TRACK, "track"),
    ("TIMING_SHEET", TIMING_SHEET, "Timing sheet"),
    ("GHOST_FRAMES", GHOST_FRAMES, "Ghost frames"),
    ("MARGINS", MARGINS, "Margins"),
    ("SKETCH_PENCIL", SKETCH_PENCIL, "Sketch pencil"),
    ("CLEAN_PENCIL", CLEAN_PENCIL, "Clean pencil"),
    ("CORRECTION_PENCIL", CORRECTION_PENCIL, "Correction pencil"),
    ("DRAW_TO_FILL", DRAW_TO_FILL, "Draw to fill"),
    ("KEEP_INSIDE_STROKES", KEEP_INSIDE_STROKES, "Keep inside strokes"),
    ("KEEP_INSIDE_STROKES_SHORT", KEEP_INSIDE_STROKES_SHORT, "Keep inside strokes"),
    ("PREVIEW_FINAL", PREVIEW_FINAL, "Preview final"),
    ("PREVIEW_FINAL_SHORT", PREVIEW_FINAL_SHORT, "Preview final"),
    ("FULL_QUALITY", FULL_QUALITY, "Full quality"),
    ("EDGE_SOFTNESS", EDGE_SOFTNESS, "Edge softness"),
    ("PIXEL_EDGES", PIXEL_EDGES, "Pixel edges"),
    ("STEADY_STROKE", STEADY_STROKE, "Steady stroke"),
    ("TRACKS", TRACKS, "Tracks"),
    ("LENGTH", LENGTH, "Length"),
    ("NOTES", NOTES, "Notes"),
    ("DIALOGUE", DIALOGUE, "Dialogue"),
    ("DRAWINGS", DRAWINGS, "Drawings"),
    ("LAYERS", LAYERS, "Layers"),
    ("BRUSHES", BRUSHES, "Brushes"),
    ("TOOL_OPTIONS", TOOL_OPTIONS, "Tool options"),
    ("CANVAS_OPTIONS", CANVAS_OPTIONS, "Canvas options"),
    ("LAYER_SET", LAYER_SET, "Layer set"),
    ("DRAWING_WITH", DRAWING_WITH, "Drawing with"),
    ("EXPORT_QUEUE", EXPORT_QUEUE, "Export queue"),
    ("EFFECTS_GRAPH", EFFECTS_GRAPH, "Effects graph"),
    ("FULL_FINISHING_SETUP", FULL_FINISHING_SETUP, "Full finishing setup"),
    ("STRENGTH", STRENGTH, "Strength"),
    ("ADD_EFFECT", ADD_EFFECT, "Add effect"),
    ("CAMERA_MOVE", CAMERA_MOVE, "Camera move"),
    ("GO_TO_LAST_DRAWING", GO_TO_LAST_DRAWING, "Go to the last drawing"),
    ("SEE_THE_PAGE", SEE_THE_PAGE, "See the page"),
    ("PAGE_GUIDES", PAGE_GUIDES, "Page guides"),
    ("ADD_BALLOON", ADD_BALLOON, "Add balloon"),
    ("ADD_TEXT", ADD_TEXT, "Add text"),
    ("SPEED_LINES", SPEED_LINES, "Speed lines"),
    ("MONO", MONO, "Mono"),
    ("DETAILS", DETAILS, "Details"),
    ("REBUILD_FROM_LAYERS", REBUILD_FROM_LAYERS, "Rebuild from layers"),
    ("RESULT", RESULT, "Result"),
    ("PREVIEW", PREVIEW, "Preview"),
    ("MOTION_CURVES", MOTION_CURVES, "Motion curves"),
    ("EDIT_COLORS", EDIT_COLORS, "Edit colors"),
    ("COLOR_NAME", COLOR_NAME, "Color name"),
    ("COLOR_WELL", COLOR_WELL, "color well"),
    ("KEPT_COLOR", KEPT_COLOR, "kept color"),
    ("COLORS", COLORS, "Colors"),
    ("ADD_COLOR", ADD_COLOR, "Add color"),
    ("COLOR_DETAILS", COLOR_DETAILS, "Color details"),
    ("COLOR_SET", COLOR_SET, "Color set"),
    ("COLOR_SETS", COLOR_SETS, "Color sets"),
    ("RESULT_BESIDE", RESULT_BESIDE, "Result beside"),
    ("GO_TO_THIS_DRAWING", GO_TO_THIS_DRAWING, "Go to this drawing"),
    ("PROTECT_LINE_ART", PROTECT_LINE_ART, "Protect line art"),
    ("REMOVE_FROM_TIMELINE", REMOVE_FROM_TIMELINE, "Remove from timeline"),
    ("SHOTS", SHOTS, "Shots"),
    ("EPISODE_TIMELINE", EPISODE_TIMELINE, "Episode timeline"),
    ("ANIMATE", ANIMATE, "Animate"),
    ("ZOOM_IN", ZOOM_IN, "Zoom in"),
    ("ZOOM_OUT", ZOOM_OUT, "Zoom out"),
    ("LONGER", LONGER, "Longer"),
    ("SHORTER", SHORTER, "Shorter"),
    ("ONE_LAYER", ONE_LAYER, "One layer"),
    ("ROUGH_OVER_REFERENCE", ROUGH_OVER_REFERENCE, "Rough over reference"),
    ("CLEAN_LINE_OVER_ROUGH", CLEAN_LINE_OVER_ROUGH, "Clean line over rough"),
    ("FULL_PAINT", FULL_PAINT, "Full paint"),
    ("HIDE_TRACK", HIDE_TRACK, "Hide track"),
    ("LOCK_TRACK", LOCK_TRACK, "Lock track"),
    ("NEXT_DRAWING", NEXT_DRAWING, "Next drawing"),
    ("PREVIOUS_DRAWING", PREVIOUS_DRAWING, "Previous drawing"),
    ("SOUND_WHILE_DRAGGING", SOUND_WHILE_DRAGGING, "Sound while dragging"),
    ("FPS", FPS, "fps"),
    ("LOOP", LOOP, "Loop"),
    ("LOOP_START", LOOP_START, "Loop start"),
    ("LOOP_END", LOOP_END, "Loop end"),
    ("APP_NAME", APP_NAME, "Gaban Studio"),
    ("SAVED_STATE", SAVED_STATE, "Saved"),
    ("HORIZON_Y", HORIZON_Y, "Horizon Y"),
    ("MANGA_PANE", MANGA_PANE, "Manga (pages)"),
    ("CANVAS_GESTURES", CANVAS_GESTURES, "Canvas gestures"),
    ("LIP_FLAP_TITLE", LIP_FLAP_TITLE, "Lip flap"),
];
