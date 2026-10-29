//! User configuration: rebindable keyboard shortcuts (and room for more).
//!
//! Krita-style: every command is a named [`Action`] the user can bind to any
//! key + modifiers, edited in the Settings window and persisted to
//! `%APPDATA%/AnimStudio/config.json`. Defaults avoid keys a compact keyboard
//! may lack (frame stepping is `,` / `.`, not the arrow keys).

use std::path::{Path, PathBuf};

use eframe::egui;

/// Every rebindable command in the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Action {
    PlayPause,
    NextFrame,
    PrevFrame,
    FirstFrame,
    LastFrame,
    PrevKey,
    NextKey,
    MissCheck,
    PrevCut,
    NextCut,
    ToggleLoop,
    NewDrawing,
    ClearCel,
    ClearCelAll,
    ToggleEraser,
    ToggleAlphaLock,
    Preset1,
    Preset2,
    Preset3,
    Preset4,
    Preset5,
    Preset6,
    Preset7,
    Preset8,
    CycleCelLayer,
    CycleCelLayerBack,
    ClearFrameKey,
    RemoveColumn,
    ToggleOnion,
    ToggleCompositeView,
    SelectTool,
    BrushTool,
    FillTool,
    /// GM3: the deck's lasso fill tool (it had a button but no verb).
    LassoFillTool,
    SelectAll,
    Undo,
    Redo,
    Save,
    SaveAs,
    Open,
    NewProject,
    // GM3: the chrome's own verbs — menus, palette, help, rooms.
    CommandPalette,
    ShortcutSheet,
    QuickStart,
    About,
    History,
    Settings,
    Quit,
    ResetLayout,
    ZoomFit,
    RoomLayout,
    RoomDrawing,
    RoomFinishing,
    RoomEdit,
    // GM4: the canvas view's own verbs and the pen's size steps.
    ZoomIn,
    ZoomOut,
    Zoom100,
    RotateViewLeft,
    RotateViewRight,
    ResetRotation,
    MirrorView,
    BrushSmaller,
    BrushBigger,
    // GM5: the sheet verbs, on the sheet cursor (the current cell, or the
    // active column from the cursor down).
    InsertFrames,
    DeleteFrames,
    HoldPlus,
    HoldMinus,
    RetimeOnes,
    RetimeTwos,
    RetimeThrees,
    Reverse,
    ColumnKindCel,
    ColumnKindBg,
    ColumnKindBook,
    ColumnKindRef,
    ToggleColumnHidden,
    ToggleColumnLocked,
    // GM14: the sheet's cell clipboard (the selected range, else the
    // cursor's cell), over the sheet only.
    CopyCells,
    CutCells,
    PasteCells,
    PasteCellsInsert,
}

/// GM3: what an action is about — the shortcut sheet's sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    File,
    Edit,
    View,
    Sheet,
    Cut,
    Room,
    Window,
    Help,
    Tool,
    Playback,
}

impl Group {
    pub const ALL: &'static [Group] = &[
        Group::File,
        Group::Edit,
        Group::View,
        Group::Playback,
        Group::Sheet,
        Group::Cut,
        Group::Tool,
        Group::Room,
        Group::Window,
        Group::Help,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Group::File => "file",
            Group::Edit => "edit",
            Group::View => "view",
            Group::Sheet => "sheet",
            Group::Cut => "cut",
            Group::Room => "room",
            Group::Window => "window",
            Group::Help => "help",
            Group::Tool => "tools",
            Group::Playback => "playback",
        }
    }
}

/// GM3: where a shortcut applies. Shortcuts dispatch by the pane under
/// the pointer, else the focused pane; a chord may repeat only across
/// different non-Global contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    Global,
    Canvas,
    Sheet,
    NodeGraph,
}

impl Ctx {
    /// May two actions in these contexts share a chord? Never when either
    /// is Global or both are the same pane.
    pub fn overlaps(self, other: Ctx) -> bool {
        self == Ctx::Global || other == Ctx::Global || self == other
    }
}

/// GM3: the shortcut preset the bindings start from. User overrides sit on
/// top: switching presets moves only the bindings still at the old
/// preset's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Keymap {
    /// Gaban's own: one key per verb, frame steps on F / S.
    #[default]
    Gaban,
    /// The letters Clip Studio Paint, Toon Boom and Krita share: B brush,
    /// E eraser, G fill, `,` / `.` frame steps, Delete clears (only the
    /// selection when there is one).
    Industry,
}

impl Keymap {
    pub const ALL: &'static [Keymap] = &[Keymap::Gaban, Keymap::Industry];
    pub fn name(self) -> &'static str {
        match self {
            Keymap::Gaban => "gaban",
            Keymap::Industry => "industry",
        }
    }

    /// The preset detent's tooltip on the Shortcuts page.
    pub fn describe(self) -> &'static str {
        match self {
            Keymap::Gaban => {
                "gaban: one key per verb (F / S step frames, E new drawing); your own changes stay"
            }
            Keymap::Industry => {
                "industry: the letters Clip Studio Paint, Toon Boom and Krita share (B brush, \
                 E eraser, G fill, , / . frame steps); your own changes stay"
            }
        }
    }
}

impl Action {
    /// All actions, in the order shown in the Settings window.
    pub const ALL: &'static [Action] = &[
        Action::PlayPause,
        Action::NextFrame,
        Action::PrevFrame,
        Action::FirstFrame,
        Action::LastFrame,
        Action::PrevKey,
        Action::NextKey,
        Action::MissCheck,
        Action::PrevCut,
        Action::NextCut,
        Action::ToggleLoop,
        Action::NewDrawing,
        Action::ClearCel,
        Action::ClearCelAll,
        Action::ToggleEraser,
        Action::ToggleAlphaLock,
        Action::Preset1,
        Action::Preset2,
        Action::Preset3,
        Action::Preset4,
        Action::Preset5,
        Action::Preset6,
        Action::Preset7,
        Action::Preset8,
        Action::CycleCelLayer,
        Action::CycleCelLayerBack,
        Action::ClearFrameKey,
        Action::RemoveColumn,
        Action::ToggleOnion,
        Action::ToggleCompositeView,
        Action::SelectTool,
        Action::BrushTool,
        Action::FillTool,
        Action::LassoFillTool,
        Action::SelectAll,
        Action::Undo,
        Action::Redo,
        Action::Save,
        Action::SaveAs,
        Action::Open,
        Action::NewProject,
        Action::CommandPalette,
        Action::ShortcutSheet,
        Action::QuickStart,
        Action::About,
        Action::History,
        Action::Settings,
        Action::Quit,
        Action::ResetLayout,
        Action::ZoomFit,
        Action::RoomLayout,
        Action::RoomDrawing,
        Action::RoomFinishing,
        Action::RoomEdit,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::Zoom100,
        Action::RotateViewLeft,
        Action::RotateViewRight,
        Action::ResetRotation,
        Action::MirrorView,
        Action::BrushSmaller,
        Action::BrushBigger,
        Action::InsertFrames,
        Action::DeleteFrames,
        Action::HoldPlus,
        Action::HoldMinus,
        Action::RetimeOnes,
        Action::RetimeTwos,
        Action::RetimeThrees,
        Action::Reverse,
        Action::ColumnKindCel,
        Action::ColumnKindBg,
        Action::ColumnKindBook,
        Action::ColumnKindRef,
        Action::ToggleColumnHidden,
        Action::ToggleColumnLocked,
        Action::CopyCells,
        Action::CutCells,
        Action::PasteCells,
        Action::PasteCellsInsert,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Action::PlayPause => "Play / Pause",
            Action::NextFrame => "Next frame",
            Action::PrevFrame => "Previous frame",
            Action::FirstFrame => "First frame",
            Action::LastFrame => "Last frame",
            Action::NewDrawing => "New drawing (blank cel)",
            Action::ClearCel => "Clear active layer",
            Action::ClearCelAll => "Clear whole cel (all layers)",
            Action::PrevKey => "Jump to previous key (active column)",
            Action::NextKey => "Jump to next key (active column)",
            Action::MissCheck => "Miss check: dark-ground hole-hunt (shiage)",
            Action::PrevCut => "Previous cut",
            Action::NextCut => "Next cut",
            Action::ToggleLoop => "Loop playback latch",
            Action::ToggleEraser => "Brush / eraser toggle",
            Action::ToggleAlphaLock => "Alpha lock toggle (recolor within existing ink)",
            Action::Preset1 => "Brush preset 1",
            Action::Preset2 => "Brush preset 2",
            Action::Preset3 => "Brush preset 3",
            Action::Preset4 => "Brush preset 4",
            Action::Preset5 => "Brush preset 5",
            Action::Preset6 => "Brush preset 6",
            Action::Preset7 => "Brush preset 7",
            Action::Preset8 => "Brush preset 8",
            Action::CycleCelLayer => "Next cel layer",
            Action::CycleCelLayerBack => "Previous cel layer",
            Action::ClearFrameKey => "Lift key (hold extends)",
            Action::RemoveColumn => "Remove selected column",
            Action::ToggleOnion => "Toggle onion skin",
            Action::ToggleCompositeView => "Composite view (node-graph output)",
            Action::SelectTool => "Select / transform tool",
            Action::BrushTool => "Paint tool (brush)",
            Action::FillTool => "Flood fill tool",
            Action::LassoFillTool => "Lasso fill tool",
            Action::SelectAll => "Select all",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Save => "Save",
            Action::SaveAs => "Save As…",
            Action::Open => "Open…",
            Action::NewProject => "New project…",
            Action::CommandPalette => "Command palette…",
            Action::ShortcutSheet => "Keyboard shortcuts",
            Action::QuickStart => "Quick start",
            Action::About => "About Gaban Studio",
            Action::History => "History",
            Action::Settings => "Settings…",
            Action::Quit => "Quit",
            Action::ResetLayout => "Reset layout",
            Action::ZoomFit => "Fit view (reset zoom & pan)",
            // The room detents' own names (Stage::name, D7/D12: romaji
            // first, then English), so one room has one name on the bar.
            Action::RoomLayout => "layout room",
            Action::RoomDrawing => "genga (drawing) room",
            Action::RoomFinishing => "shiage (paint) room",
            Action::RoomEdit => "henshū (edit) room",
            Action::ZoomIn => "Zoom in",
            Action::ZoomOut => "Zoom out",
            Action::Zoom100 => "Actual size (100%)",
            Action::RotateViewLeft => "Rotate view left 15°",
            Action::RotateViewRight => "Rotate view right 15°",
            Action::ResetRotation => "Reset view rotation",
            Action::MirrorView => "Mirror view (display only)",
            Action::BrushSmaller => "Brush smaller",
            Action::BrushBigger => "Brush bigger",
            Action::InsertFrames => "Insert frame",
            Action::DeleteFrames => "Delete frame",
            Action::HoldPlus => "Hold +1",
            Action::HoldMinus => "Hold -1",
            Action::RetimeOnes => "Retime to 1s (on ones)",
            Action::RetimeTwos => "Retime to 2s (on twos)",
            Action::RetimeThrees => "Retime to 3s (on threes)",
            Action::Reverse => "Reverse timing",
            Action::ColumnKindCel => "Column kind: Cel",
            Action::ColumnKindBg => "Column kind: BG (haikei)",
            Action::ColumnKindBook => "Column kind: Book (overlay)",
            Action::ColumnKindRef => "Column kind: Ref (guide)",
            Action::ToggleColumnHidden => "Hide column",
            Action::ToggleColumnLocked => "Lock column",
            Action::CopyCells => "Copy cells",
            Action::CutCells => "Cut cells",
            Action::PasteCells => "Paste cells",
            Action::PasteCellsInsert => "Paste cells (insert)",
        }
    }

    /// GM3: one line of help — the tooltip under every menu item, deck
    /// button and palette row.
    pub fn help(self) -> &'static str {
        use Action::*;
        match self {
            PlayPause => "Start or stop playback of the cut. Held on the canvas, Space drags the view (Shift+drag turns it).",
            NextFrame => "Step one frame forward.",
            PrevFrame => "Step one frame back.",
            FirstFrame => "Jump to the cut's first frame.",
            LastFrame => "Jump to the cut's last frame.",
            PrevKey => "Jump to the previous key on the active column.",
            NextKey => "Jump to the next key on the active column.",
            MissCheck => "Flip to a dark ground to hunt unpainted holes.",
            PrevCut => "Open the previous cut in the scene list.",
            NextCut => "Open the next cut in the scene list.",
            ToggleLoop => "Latch looping playback on or off.",
            NewDrawing => "Put a new blank drawing on this frame.",
            ClearCel => "Erase the active layer of this drawing, or only the selection (undoable).",
            ClearCelAll => "Erase every layer of this drawing (undoable).",
            ToggleEraser => "Switch the pen between brush and eraser.",
            ToggleAlphaLock => "Paint only where ink already is (recolour).",
            Preset1 | Preset2 | Preset3 | Preset4 | Preset5 | Preset6 | Preset7 | Preset8 => {
                "Arm this brush preset slot (with fill: a palette pot)."
            }
            CycleCelLayer => "Make the next cel layer active.",
            CycleCelLayerBack => "Make the previous cel layer active.",
            ClearFrameKey => "Lift the key on this frame; the hold before it extends.",
            RemoveColumn => "Remove the selected x-sheet column.",
            ToggleOnion => "Show neighbouring drawings faintly under this one.",
            ToggleCompositeView => "Show the node graph's composite instead of the edit view.",
            SelectTool => "Select, move, scale and rotate ink.",
            BrushTool => "Back to the brush.",
            FillTool => "Flood-fill closed areas with the brush colour.",
            LassoFillTool => "Draw a loop and it fills with the brush colour.",
            SelectAll => "Select everything on the active layer.",
            Undo => "Take back the last step.",
            Redo => "Re-apply the step you took back last.",
            Save => "Write the project to its file.",
            SaveAs => "Write the project to a new file.",
            Open => "Open a project file (asks first about unsaved work).",
            NewProject => "Start a new project (asks first about unsaved work).",
            CommandPalette => "Search every command by name and run it.",
            ShortcutSheet => "List every shortcut, grouped and searchable.",
            QuickStart => "The first five minutes: draw, time, play, save.",
            About => "Version and credits.",
            History => "Every step so far; click one to go back or forward to it.",
            Settings => "Shortcuts, pen, performance and the rest.",
            Quit => "Close the app (asks first about unsaved work).",
            ResetLayout => "Put this room's panes back where they started.",
            ZoomFit => "Fit the whole paper back in its pane, centred; a turned view keeps its turn.",
            RoomLayout => "Open the layout room: the cut's preview and timing.",
            RoomDrawing => "Open the genga (drawing) room: canvas and sheet.",
            RoomFinishing => "Open the shiage (paint) room: palettes and fill.",
            RoomEdit => "Open the henshū (edit) room: cuts and the sheet head.",
            ZoomIn => "Zoom the canvas in one step on the zoom ladder.",
            ZoomOut => "Zoom the canvas out one step on the zoom ladder.",
            Zoom100 => "One paper pixel to one screen point.",
            RotateViewLeft => "Turn the view 15° anticlockwise (Shift+Space+drag turns freely).",
            RotateViewRight => "Turn the view 15° clockwise (Shift+Space+drag turns freely).",
            ResetRotation => "Turn the view back upright.",
            MirrorView => "Flip the view left to right; the drawing itself is untouched.",
            BrushSmaller => "Step the brush size down the size ladder.",
            BrushBigger => "Step the brush size up the size ladder.",
            InsertFrames => "Insert a frame at the cursor in every column; the cut grows by one frame.",
            DeleteFrames => "Delete the cursor's frame from every column; the cut is one frame shorter.",
            HoldPlus => "Hold +1: the exposure under the cursor holds one frame longer; the column ripples.",
            HoldMinus => "Hold -1: the exposure under the cursor holds one frame less; a 1-frame one goes.",
            RetimeOnes => "Retime to 1s (on ones): each drawing in the column from the cursor down holds 1 frame.",
            RetimeTwos => "Retime to 2s (on twos): each drawing in the column from the cursor down holds 2 frames.",
            RetimeThrees => {
                "Retime to 3s (on threes): each drawing in the column from the cursor down holds 3 frames."
            }
            Reverse => "Reverse the drawing order from the cursor down; each exposure keeps its length.",
            ColumnKindCel => "Make the column a cel: a drawing column, composited in sheet order.",
            ColumnKindBg => "Make the column haikei (background): drawn under every cel.",
            ColumnKindBook => "Make the column a book: an overlay held above the cels.",
            ColumnKindRef => "Make the column a ref (guide): shown for reference, never rendered.",
            ToggleColumnHidden => "Hide or show the column: a hidden column is left out of every composite.",
            ToggleColumnLocked => "Lock or unlock the column: a locked column refuses paint and sheet edits.",
            CopyCells => "Copy the selected sheet cells (or the cursor's cell) to the sheet clipboard.",
            CutCells => "Copy the selected sheet cells, then empty them.",
            PasteCells => "Paste the copied cells at the cursor, over the cells there.",
            PasteCellsInsert => "Paste the copied cells at the cursor, pushing the cells there later.",
        }
    }

    /// GM3: the shortcut sheet's section.
    pub fn group(self) -> Group {
        use Action::*;
        match self {
            NewProject | Open | Save | SaveAs | Settings | Quit => Group::File,
            Undo | Redo | History | SelectAll | ClearCel | ClearCelAll => Group::Edit,
            ZoomFit | ToggleOnion | ToggleCompositeView | MissCheck | ZoomIn | ZoomOut | Zoom100
            | RotateViewLeft | RotateViewRight | ResetRotation | MirrorView => Group::View,
            PlayPause | NextFrame | PrevFrame | FirstFrame | LastFrame | ToggleLoop => {
                Group::Playback
            }
            PrevKey | NextKey | NewDrawing | ClearFrameKey | RemoveColumn | InsertFrames
            | DeleteFrames | HoldPlus | HoldMinus | RetimeOnes | RetimeTwos | RetimeThrees
            | Reverse | ColumnKindCel | ColumnKindBg | ColumnKindBook | ColumnKindRef
            | ToggleColumnHidden | ToggleColumnLocked | CopyCells | CutCells | PasteCells
            | PasteCellsInsert => Group::Sheet,
            PrevCut | NextCut => Group::Cut,
            SelectTool | BrushTool | FillTool | LassoFillTool | ToggleEraser | ToggleAlphaLock
            | Preset1 | Preset2 | Preset3 | Preset4 | Preset5 | Preset6 | Preset7 | Preset8
            | CycleCelLayer | CycleCelLayerBack | BrushSmaller | BrushBigger => Group::Tool,
            RoomLayout | RoomDrawing | RoomFinishing | RoomEdit => Group::Room,
            ResetLayout => Group::Window,
            CommandPalette | ShortcutSheet | QuickStart | About => Group::Help,
        }
    }

    /// GM3: "Menu/Item" in the menu bar. None = PALETTE-ONLY: the tools
    /// live on the canvas deck, so the menus do not repeat them.
    pub fn menu(self) -> Option<&'static str> {
        use Action::*;
        Some(match self {
            NewProject => "File/New…",
            Open => "File/Open…",
            Save => "File/Save",
            SaveAs => "File/Save As…",
            Settings => "File/Settings…",
            Quit => "File/Quit",
            Undo => "Edit/Undo",
            Redo => "Edit/Redo",
            History => "Edit/History",
            SelectAll => "Edit/Select All",
            ClearCel => "Edit/Clear active layer",
            ClearCelAll => "Edit/Clear whole cel",
            ZoomFit => "View/Fit view",
            ZoomIn => "View/Zoom in",
            ZoomOut => "View/Zoom out",
            Zoom100 => "View/Actual size (100%)",
            RotateViewLeft => "View/Rotate left 15°",
            RotateViewRight => "View/Rotate right 15°",
            ResetRotation => "View/Reset rotation",
            MirrorView => "View/Mirror view",
            ToggleOnion => "View/Onion skin",
            ToggleCompositeView => "View/Composite view",
            MissCheck => "View/Miss check",
            PlayPause => "Sheet/Play / Pause",
            ToggleLoop => "Sheet/Loop playback",
            FirstFrame => "Sheet/First frame",
            PrevFrame => "Sheet/Previous frame",
            NextFrame => "Sheet/Next frame",
            LastFrame => "Sheet/Last frame",
            PrevKey => "Sheet/Previous key",
            NextKey => "Sheet/Next key",
            NewDrawing => "Sheet/New drawing",
            ClearFrameKey => "Sheet/Lift key",
            RemoveColumn => "Sheet/Remove column",
            InsertFrames => "Sheet/Insert frame",
            DeleteFrames => "Sheet/Delete frame",
            HoldPlus => "Sheet/Hold +1",
            HoldMinus => "Sheet/Hold -1",
            RetimeOnes => "Sheet/Retime to 1s (on ones)",
            RetimeTwos => "Sheet/Retime to 2s (on twos)",
            RetimeThrees => "Sheet/Retime to 3s (on threes)",
            Reverse => "Sheet/Reverse timing",
            ColumnKindCel => "Sheet/Column kind: Cel",
            ColumnKindBg => "Sheet/Column kind: BG (haikei)",
            ColumnKindBook => "Sheet/Column kind: Book (overlay)",
            ColumnKindRef => "Sheet/Column kind: Ref (guide)",
            ToggleColumnHidden => "Sheet/Hide column",
            ToggleColumnLocked => "Sheet/Lock column",
            CopyCells => "Sheet/Copy cells",
            CutCells => "Sheet/Cut cells",
            PasteCells => "Sheet/Paste cells",
            PasteCellsInsert => "Sheet/Paste cells (insert)",
            PrevCut => "Cut/Previous cut",
            NextCut => "Cut/Next cut",
            // Each item is its room detent's Stage::name (a test holds them
            // together).
            RoomLayout => "Room/layout",
            RoomDrawing => "Room/genga (drawing)",
            RoomFinishing => "Room/shiage (paint)",
            RoomEdit => "Room/henshū (edit)",
            ResetLayout => "Window/Reset layout",
            QuickStart => "Help/Quick start",
            ShortcutSheet => "Help/Keyboard shortcuts",
            CommandPalette => "Help/Command palette…",
            About => "Help/About",
            SelectTool | BrushTool | FillTool | LassoFillTool | ToggleEraser | ToggleAlphaLock
            | Preset1 | Preset2 | Preset3 | Preset4 | Preset5 | Preset6 | Preset7 | Preset8
            | CycleCelLayer | CycleCelLayerBack | BrushSmaller | BrushBigger => return None,
        })
    }

    /// GM3: other words the palette finds this action by.
    pub fn aliases(self) -> &'static [&'static str] {
        use Action::*;
        match self {
            PlayPause => &["preview", "run"],
            MissCheck => &["holes", "gaps"],
            // "light table" stays free for the light table's own verb (GM6).
            ToggleOnion => &["onion", "ghost", "sukashi"],
            ToggleCompositeView => &["render", "output", "comp"],
            ToggleEraser => &["rubber", "erase"],
            ToggleAlphaLock => &["preserve alpha", "lock transparency"],
            SelectTool => &["move", "transform", "lasso select"],
            FillTool => &["bucket", "paint"],
            LassoFillTool => &["lasso fill", "fill loop"],
            NewDrawing => &["new cel", "blank"],
            ClearFrameKey => &["hold", "extend"],
            RemoveColumn => &["delete column"],
            ClearCel => &["wipe", "erase layer"],
            ClearCelAll => &["wipe all", "erase cel"],
            CycleCelLayer | CycleCelLayerBack => &["line", "colour", "shadow"],
            Undo => &["back", "revert"],
            Redo => &["again"],
            History => &["undo list", "steps"],
            Save => &["write", "store"],
            Open => &["load"],
            NewProject => &["create"],
            Settings => &["preferences", "options", "config"],
            Quit => &["exit", "close"],
            CommandPalette => &["search", "commands"],
            ShortcutSheet => &["keys", "hotkeys", "cheat sheet"],
            ZoomFit => &["zoom", "reset view"],
            ZoomIn | ZoomOut => &["magnify", "scale view"],
            Zoom100 => &["100%", "pixel size", "one to one"],
            RotateViewLeft | RotateViewRight | ResetRotation => &["turn", "tilt canvas", "spin"],
            MirrorView => &["flop", "reverse view", "flip horizontal", "hanten", "flip canvas"],
            BrushSmaller | BrushBigger => &["size", "brush size", "bracket"],
            // The English words still find the rooms by their romaji names.
            RoomLayout => &["lo"],
            RoomDrawing => &["drawing", "douga"],
            RoomFinishing => &["paint", "ink", "colour"],
            RoomEdit => &["edit", "henshu", "editing"],
            // GM5: the sheet verbs by the words a timing sheet uses.
            InsertFrames => &["insert frames", "add frame", "open a frame"],
            DeleteFrames => &["delete frames", "remove frame", "close a frame"],
            HoldPlus => &["hold", "extend", "longer"],
            HoldMinus => &["hold", "shorten", "shorter"],
            RetimeOnes => &["ones", "1s", "retime"],
            RetimeTwos => &["twos", "2s", "retime"],
            RetimeThrees => &["threes", "3s", "retime"],
            Reverse => &["reverse", "backwards", "play backwards"],
            ColumnKindCel => &["column kind", "cel column"],
            ColumnKindBg => &["column kind", "background", "haikei"],
            ColumnKindBook => &["column kind", "overlay", "held cel"],
            ColumnKindRef => &["column kind", "guide", "reference"],
            ToggleColumnHidden => &["hide", "show column", "visibility"],
            ToggleColumnLocked => &["lock", "unlock", "protect"],
            CopyCells => &["copy", "cells", "clipboard"],
            CutCells => &["cut", "cells", "clipboard"],
            PasteCells => &["paste", "cells", "clipboard"],
            PasteCellsInsert => &["paste insert", "cells", "clipboard"],
            _ => &[],
        }
    }

    /// GM3: where this action's shortcut applies (see [`Ctx`]).
    pub fn context(self) -> Ctx {
        use Action::*;
        match self {
            SelectTool | BrushTool | FillTool | LassoFillTool | ToggleEraser | ToggleAlphaLock
            | Preset1 | Preset2 | Preset3 | Preset4 | Preset5 | Preset6 | Preset7 | Preset8
            | CycleCelLayer | CycleCelLayerBack | SelectAll | ClearCel | ClearCelAll
            | BrushSmaller | BrushBigger => Ctx::Canvas,
            NewDrawing | ClearFrameKey | RemoveColumn | InsertFrames | DeleteFrames | HoldPlus
            | HoldMinus | RetimeOnes | RetimeTwos | RetimeThrees | Reverse | ColumnKindCel
            | ColumnKindBg | ColumnKindBook | ColumnKindRef | ToggleColumnHidden
            | ToggleColumnLocked | CopyCells | CutCells | PasteCells | PasteCellsInsert => {
                Ctx::Sheet
            }
            _ => Ctx::Global,
        }
    }

    /// GM3: a destructive verb whose chord fires only over its own pane,
    /// even when no other action shares the chord (a painter's Delete over
    /// the canvas must not remove the sheet's column). E (new drawing)
    /// still fires anywhere: genga artists press it over the canvas, and so
    /// do GM5's timing keys (= / - hold, Insert, Alt+1/2/3). GM5: Shift+
    /// Delete deletes a frame only over the sheet (Industry's painter clears
    /// the whole cel with it over the canvas).
    pub fn pane_only(self) -> bool {
        // GM14: the cell clipboard too — Ctrl+V over the canvas must not
        // paste cells into the sheet.
        matches!(
            self,
            Action::RemoveColumn
                | Action::DeleteFrames
                | Action::CopyCells
                | Action::CutCells
                | Action::PasteCells
                | Action::PasteCellsInsert
        )
    }

    /// GM3: the verb that arms pencil-box slot `digit` (1-based), if any.
    pub fn preset_slot(digit: usize) -> Option<Action> {
        use Action::*;
        [Preset1, Preset2, Preset3, Preset4, Preset5, Preset6, Preset7, Preset8]
            .get(digit.checked_sub(1)?)
            .copied()
    }

    /// GM3: the preset's chord for this action.
    fn default_chord(self, keymap: Keymap) -> Option<Chord> {
        use Action::*;
        if keymap == Keymap::Industry {
            let shift = |name: &str| Chord {
                shift: true,
                ..Chord::plain(name)
            };
            match self {
                NextFrame => return Some(Chord::plain("Period")),
                PrevFrame => return Some(Chord::plain("Comma")),
                NewDrawing => return Some(Chord::plain("N")),
                ToggleEraser => return Some(Chord::plain("E")),
                // Delete clears on the canvas and removes in the sheet:
                // one chord, two panes.
                ClearCel => return Some(Chord::plain("Delete")),
                ClearCelAll => return Some(shift("Delete")),
                _ => {}
            }
        }
        self.gaban_chord()
    }

    /// GM3: extra chords that fire the action in every preset.
    fn default_also(self) -> Vec<Chord> {
        match self {
            // Ctrl+Shift+Z redoes beside Ctrl+Y (the other studios' habit).
            Action::Redo => vec![Chord {
                shift: true,
                ..Chord::ctrl("Z")
            }],
            Action::CommandPalette => vec![Chord::plain("F3")],
            // GM4: the keypad's plus zooms in beside Ctrl+=.
            Action::ZoomIn => vec![Chord::ctrl("Plus")],
            _ => Vec::new(),
        }
    }

    fn gaban_chord(self) -> Option<Chord> {
        use Action::*;
        let k = |name: &str| Chord::plain(name);
        let ctrl = |name: &str| Chord::ctrl(name);
        let alt = |name: &str| Chord {
            alt: true,
            ..Chord::plain(name)
        };
        match self {
            PlayPause => Some(k("Space")),
            NextFrame => Some(k("F")),
            PrevFrame => Some(k("S")),
            FirstFrame => Some(k("Home")),
            LastFrame => Some(k("End")),
            // One verb, one key (room charters): Q/W step the exposure.
            PrevKey => Some(k("Q")),
            NextKey => Some(k("W")),
            MissCheck => Some(k("M")),
            PrevCut => Some(k("PageUp")),
            NextCut => Some(k("PageDown")),
            ToggleLoop => Some(k("P")),
            NewDrawing => Some(k("E")),
            ClearCel => Some(k("D")),
            ClearCelAll => Some(Chord {
                keys: vec!["D".to_string()],
                ctrl: false,
                shift: true,
                alt: false,
            }),
            ToggleEraser => Some(k("X")),
            ToggleAlphaLock => Some(k("L")),
            Preset1 => Some(k("1")),
            Preset2 => Some(k("2")),
            Preset3 => Some(k("3")),
            Preset4 => Some(k("4")),
            Preset5 => Some(k("5")),
            Preset6 => Some(k("6")),
            Preset7 => Some(k("7")),
            Preset8 => Some(k("8")),
            CycleCelLayer => Some(k("A")),
            CycleCelLayerBack => Some(Chord {
                keys: vec!["A".to_string()],
                ctrl: false,
                shift: true,
                alt: false,
            }),
            ClearFrameKey => Some(k("Backspace")),
            RemoveColumn => Some(k("Delete")),
            ToggleOnion => Some(k("O")),
            ToggleCompositeView => Some(k("C")),
            SelectTool => Some(k("V")),
            BrushTool => Some(k("B")),
            FillTool => Some(k("G")),
            // The other fill: Shift on the fill key.
            LassoFillTool => Some(Chord {
                keys: vec!["G".to_string()],
                ctrl: false,
                shift: true,
                alt: false,
            }),
            SelectAll => Some(ctrl("A")),
            Undo => Some(ctrl("Z")),
            Redo => Some(ctrl("Y")),
            Save => Some(ctrl("S")),
            SaveAs => Some(Chord {
                keys: vec!["S".to_string()],
                ctrl: true,
                shift: true,
                alt: false,
            }),
            Open => Some(ctrl("O")),
            NewProject => Some(ctrl("N")),
            CommandPalette => Some(ctrl("K")),
            ShortcutSheet => Some(k("F1")),
            Settings => Some(ctrl("Comma")),
            Quit => Some(ctrl("Q")),
            ZoomFit => Some(ctrl("0")),
            // GM4: the view keys sit on Ctrl so the bare keys stay the
            // painter's; R is not used (holding R is the flip).
            ZoomIn => Some(ctrl("Equals")),
            ZoomOut => Some(ctrl("Minus")),
            Zoom100 => Some(ctrl("1")),
            RotateViewLeft => Some(ctrl("OpenBracket")),
            RotateViewRight => Some(ctrl("CloseBracket")),
            ResetRotation => Some(ctrl("Backslash")),
            MirrorView => Some(k("H")),
            BrushSmaller => Some(k("OpenBracket")),
            BrushBigger => Some(k("CloseBracket")),
            // GM5: the sheet verbs, the same in both presets. Timing on bare
            // keys (= / - hold, Insert), the frame's delete on Shift+Delete
            // (over the sheet only), the rest on Alt — never R (the flip).
            InsertFrames => Some(k("Insert")),
            DeleteFrames => Some(Chord {
                shift: true,
                ..k("Delete")
            }),
            HoldPlus => Some(k("Equals")),
            HoldMinus => Some(k("Minus")),
            RetimeOnes => Some(alt("1")),
            RetimeTwos => Some(alt("2")),
            RetimeThrees => Some(alt("3")),
            Reverse => Some(alt("V")),
            ColumnKindCel => Some(alt("C")),
            ColumnKindBg => Some(alt("B")),
            ColumnKindBook => Some(alt("K")),
            ColumnKindRef => Some(alt("G")),
            ToggleColumnHidden => Some(alt("H")),
            ToggleColumnLocked => Some(alt("L")),
            // GM14: the clipboard keys every studio uses; Shift inserts.
            CopyCells => Some(ctrl("C")),
            CutCells => Some(ctrl("X")),
            PasteCells => Some(ctrl("V")),
            PasteCellsInsert => Some(Chord {
                shift: true,
                ..ctrl("V")
            }),
            QuickStart | About | History | ResetLayout | RoomLayout | RoomDrawing
            | RoomFinishing | RoomEdit => None,
        }
    }
}

/// GM3: every action bound as the preset binds it.
pub fn default_binds(keymap: Keymap) -> Vec<Binding> {
    Action::ALL
        .iter()
        .map(|a| Binding {
            action: *a,
            chord: a.default_chord(keymap),
            also: a.default_also(),
        })
        .collect()
}

/// GM3: every pair of actions that share a chord where both could fire
/// (see [`Ctx::overlaps`]) — the conflicts the Shortcuts page marks.
pub fn clashes(binds: &[Binding]) -> Vec<(Action, Action, Chord)> {
    let mut out = Vec::new();
    for (i, a) in binds.iter().enumerate() {
        for b in &binds[i + 1..] {
            if a.action == b.action || !a.action.context().overlaps(b.action.context()) {
                continue;
            }
            if let Some(c) = a.chords().find(|c| b.chords().any(|d| d == *c)) {
                out.push((a.action, b.action, c.clone()));
            }
        }
    }
    out
}

/// A key combination: a set of (non-modifier) keys plus modifier flags. Held
/// together, they fire the action. Multi-key chords (e.g. A+S) are supported.
/// `keys` are egui [`egui::Key`] names; `ctrl` is the platform command modifier.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Chord {
    pub keys: Vec<String>,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    fn plain(key: &str) -> Self {
        Self {
            keys: vec![key.to_string()],
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    fn ctrl(key: &str) -> Self {
        Self {
            ctrl: true,
            ..Self::plain(key)
        }
    }

    /// Build from a captured set of keys + the modifiers held at capture.
    pub fn from_capture(keys: Vec<String>, m: egui::Modifiers) -> Self {
        Self {
            keys,
            ctrl: m.command,
            shift: m.shift,
            alt: m.alt,
        }
    }

    /// Human-readable label, e.g. "Ctrl+Shift+S" or "A+S".
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        let keys: Vec<&str> = self.keys.iter().map(|k| pretty_key(k)).collect();
        s.push_str(&keys.join("+"));
        s
    }

    /// GM4: bare Space — the canvas's hand while held, so its verb fires
    /// on the release (a tap), not the press.
    pub(crate) fn is_space_tap(&self) -> bool {
        self.keys.len() == 1 && self.keys[0] == "Space" && !self.ctrl && !self.shift && !self.alt
    }

    /// Fires when every key is held, at least one was pressed THIS frame, and
    /// the modifiers match exactly. Bare Space fires on its release instead
    /// (GM4: a hold that drags pans; see `shortcuts::collect`).
    pub(crate) fn matches(&self, i: &egui::InputState) -> bool {
        if self.keys.is_empty() {
            return false;
        }
        let m = i.modifiers;
        if m.command != self.ctrl || m.shift != self.shift || m.alt != self.alt {
            return false;
        }
        if self.is_space_tap() {
            return i.key_released(egui::Key::Space);
        }
        let mut any_pressed = false;
        for name in &self.keys {
            let Some(k) = egui::Key::from_name(name) else {
                return false;
            };
            if !i.keys_down.contains(&k) {
                return false;
            }
            if i.key_pressed(k) {
                any_pressed = true;
            }
        }
        any_pressed
    }
}

fn pretty_key(name: &str) -> &str {
    match name {
        "Comma" => ",",
        "Period" => ".",
        "Space" => "Space",
        other => other,
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Binding {
    pub action: Action,
    pub chord: Option<Chord>,
    /// GM3: more chords that fire the same action (Redo's Ctrl+Shift+Z
    /// beside Ctrl+Y). Rebinding or unbinding clears them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also: Vec<Chord>,
}

impl Binding {
    /// The chord, then the extra ones.
    pub fn chords(&self) -> impl Iterator<Item = &Chord> {
        self.chord.iter().chain(self.also.iter())
    }

    /// "Ctrl+Y / Ctrl+Shift+Z", or None when unbound.
    pub fn label(&self) -> Option<String> {
        let all: Vec<String> = self.chords().map(|c| c.label()).collect();
        (!all.is_empty()).then(|| all.join(" / "))
    }
}

// ---- Performance settings (Krita-derived; see research/performance-settings.md) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FrameLatency {
    #[default]
    Low, // 1 frame — lowest pen latency
    Throughput, // 2 frames — smoother under heavy GPU load
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum CanvasFilter {
    Nearest,
    #[default]
    Linear,
}

impl CanvasFilter {
    pub fn wgpu(self) -> wgpu::FilterMode {
        match self {
            CanvasFilter::Nearest => wgpu::FilterMode::Nearest,
            CanvasFilter::Linear => wgpu::FilterMode::Linear,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PerfConfig {
    /// Sync to vertical refresh. Applied at startup (see note in the UI).
    #[serde(default)]
    pub vsync: bool,
    #[serde(default)]
    pub frame_latency: FrameLatency,
    #[serde(default)]
    pub canvas_filter: CanvasFilter,
    /// Max redraws/sec while idle/painting (playback overrides upward).
    #[serde(default = "default_fps_cap")]
    pub fps_cap: u32,
    #[serde(default)]
    pub show_fps: bool,
    /// Undo steps kept (0 = unlimited).
    #[serde(default = "default_undo_limit")]
    pub undo_limit: usize,
}

fn default_fps_cap() -> u32 {
    100
}
fn default_undo_limit() -> usize {
    200
}

impl Default for PerfConfig {
    fn default() -> Self {
        Self {
            vsync: false,
            frame_latency: FrameLatency::Low,
            canvas_filter: CanvasFilter::Linear,
            fps_cap: default_fps_cap(),
            show_fps: false,
            undo_limit: default_undo_limit(),
        }
    }
}

// ---- Pen / Tablet (Krita-derived; see research below) ----

/// Pressure response curve: remaps raw pen pressure 0..1 before it drives line
/// width. Points sorted ascending by x, endpoints pinned at (0,0)/(1,1).
/// Piecewise-linear (never overshoots [0,1], unlike a cubic spline).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PressureCurve {
    pub points: Vec<[f32; 2]>,
}

impl PressureCurve {
    pub fn linear() -> Self {
        Self {
            points: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        }
    }
    /// Above the diagonal: light touches produce more width (boosts a weak pen).
    pub fn soft() -> Self {
        Self {
            points: vec![[0.0, 0.0], [0.5, 0.7], [1.0, 1.0]],
        }
    }
    /// Below the diagonal: must press harder for width (thinner light strokes).
    pub fn hard() -> Self {
        Self {
            points: vec![[0.0, 0.0], [0.5, 0.3], [1.0, 1.0]],
        }
    }

    /// Remap `x` (0..1) through the curve.
    pub fn apply(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let p = &self.points;
        if p.len() < 2 {
            return x;
        }
        if x <= p[0][0] {
            return p[0][1];
        }
        if x >= p[p.len() - 1][0] {
            return p[p.len() - 1][1];
        }
        let (mut lo, mut hi) = (0usize, p.len() - 1);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if p[mid][0] <= x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let (x0, y0, x1, y1) = (p[lo][0], p[lo][1], p[hi][0], p[hi][1]);
        let dx = x1 - x0;
        if dx <= 1e-6 {
            return y0;
        }
        (y0 + (y1 - y0) * (x - x0) / dx).clamp(0.0, 1.0)
    }
}

impl Default for PressureCurve {
    fn default() -> Self {
        Self::linear()
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PenConfig {
    #[serde(default)]
    pub pressure_curve: PressureCurve,
    /// Native tablet backend (Windows Ink RealTimeStylus via octotablet) on
    /// its dedicated input thread. Proven on the XP-Pen Artist 22 Pro —
    /// default ON. Off = egui Touch events; ANIMSTUDIO_NO_TABLET=1
    /// force-disables if a driver ever misbehaves.
    #[serde(default = "default_true")]
    pub native_tablet: bool,
    /// GM4: the stroke stabilizer, 0 (raw) to 10 — how long the pulled
    /// string between pen and ink is.
    #[serde(default)]
    pub stabilizer: u8,
}

fn default_true() -> bool {
    true
}

/// In-progress rebind: accumulates the keys held (release confirms the chord,
/// so multi-key combos like Ctrl+Shift+S or A+S can be built).
#[derive(Debug, Clone)]
pub struct RebindCapture {
    pub action: Action,
    keys: Vec<String>,
    modifiers: egui::Modifiers,
}

impl RebindCapture {
    pub fn new(action: Action) -> Self {
        Self {
            action,
            keys: Vec::new(),
            modifiers: egui::Modifiers::NONE,
        }
    }
}

// ---- Brush presets ----------------------------------------------------------

/// A named brush snapshot — applied whole from a keybind (1–8), the Presets
/// pane, or automatically when entering a workspace bound to it.
/// PSD-brush-engine: everything an imported Krita preset carries that
/// our dab pipeline can honour. ABSENT on the pencil box and every
/// hand-made preset — and when absent, the stroke path is byte-identical
/// to before this room existed (NEVER-DO 1).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
pub struct EngineDef {
    /// Krita paintopid ("paintbrush", "spraybrush", …). Engines outside
    /// the honest set paint as the plain dab and say so in the hover.
    #[serde(default)]
    pub engine: String,
    /// Dab spacing as a fraction of diameter (Krita Brush@spacing).
    #[serde(default = "d_spacing")]
    pub spacing: f32,
    /// Generated tip (MaskGenerator) — rasterized to a mask at arm.
    #[serde(default)]
    pub auto: Option<AutoTip>,
    /// Stamp tip: key into the brush_tips cache (extracted at import).
    #[serde(default)]
    pub tip_key: Option<String>,
    /// Sensor curves per target, applied CPU-side at dab building.
    #[serde(default)]
    pub curves: Vec<CurveDef>,
    /// Scatter as a fraction of diameter (spray engines only — the
    /// paintbrush gate for it is not reliably recoverable; logged).
    #[serde(default)]
    pub scatter: f32,
    /// Fraction of dabs kept (spray density). 0 = keep all (legacy).
    #[serde(default)]
    pub density: f32,
    /// Random tip rotation amount 0..1 (Brush@randomness).
    #[serde(default)]
    pub randomness: f32,
    /// Base tip angle, degrees (Brush@angle).
    #[serde(default)]
    pub angle_deg: f32,
    /// THE SMUDGE GATE (PSD-smudge): dulling colorsmudge. How fast the
    /// held colour picks up the canvas (0 = engine off)…
    #[serde(default)]
    pub smudge_rate: f32,
    /// …and how much brush colour re-inks each dab (blenders: 0).
    #[serde(default)]
    pub color_rate: f32,
    /// Krita EraserMode — the preset ERASES. Never imported (our eraser
    /// is a tool); the flag exists so the classifier can see it.
    #[serde(default)]
    pub eraser: bool,
    /// Paper grain: key into the brush_grains cache + scale + strength.
    #[serde(default)]
    pub grain_key: Option<String>,
    #[serde(default = "d_one")]
    pub grain_scale: f32,
    #[serde(default)]
    pub grain_strength: f32,
}

fn d_spacing() -> f32 {
    0.1
}
fn d_one() -> f32 {
    1.0
}

/// Krita MaskGenerator, the generated-tip recipe.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
pub struct AutoTip {
    /// "circle" or "rect".
    #[serde(default)]
    pub shape: String,
    /// width/height of the stamp (1 = round).
    #[serde(default = "d_one")]
    pub ratio: f32,
    /// Fade fractions 0..1 per axis (1 = hard to the rim).
    #[serde(default = "d_one")]
    pub hfade: f32,
    #[serde(default = "d_one")]
    pub vfade: f32,
    /// Star points; 2 = plain.
    #[serde(default = "d_two")]
    pub spikes: u32,
    /// The "soft" MaskGenerator (gaussian-ish falloff).
    #[serde(default)]
    pub soft: bool,
}

fn d_two() -> u32 {
    2
}

/// One sensor curve: `target` ∈ size|opacity|flow|rotation, `sensor` ∈
/// pressure|fuzzy|fade|distance|xtilt|ytilt|ascension|declination.
/// Empty points = the sensor's raw value (identity curve).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
pub struct CurveDef {
    pub target: String,
    pub sensor: String,
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushPreset {
    pub name: String,
    pub size_px: f32,
    pub flow: f32,
    pub opacity: f32,
    pub dyn_size: bool,
    pub dyn_opacity: bool,
    pub min_size: f32,
    /// None = keep the current brush colour when applying.
    pub color: Option<[u8; 4]>,
    /// Tilt dynamics (native tablet backend only — egui pen events carry no
    /// tilt). Serde defaults keep pre-tilt configs loading.
    /// PSD-brush-engine: the imported Krita machinery. None = the
    /// procedural dab, byte-identical to before.
    #[serde(default)]
    pub engine: Option<EngineDef>,
    /// BRUSH BANK: the source file this preset arrived in ("" = made
    /// here / unsorted). Banks list and remove as one in Settings →
    /// Plugins.
    #[serde(default)]
    pub bank: String,
    #[serde(default)]
    pub tilt_size: bool,
    #[serde(default)]
    pub tilt_opacity: bool,
    /// Tilting flattens + rotates the dab along the lean (the stamp itself
    /// shows the tilt, like Krita's tip masks following the pen).
    #[serde(default)]
    pub tilt_shape: bool,
    /// How strongly tilt drives the enabled dynamics (0 = off, 1 = full:
    /// size up to 2×, opacity down to ¼ at a flat pen).
    #[serde(default = "default_tilt_strength")]
    pub tilt_strength: f32,
    /// GM4: the dab's edge, MyPaint's falloff — toward 1 hard, toward 0
    /// soft. 0.85 is the edge every brush had before it was a setting.
    #[serde(default = "default_hardness")]
    pub hardness: f32,
    /// GM4: aliased — every dab covers fully or not at all (hard 0/255
    /// lines, no edge smoothing), for pixel-exact fill boundaries.
    #[serde(default)]
    pub aliased: bool,
}

pub fn default_tilt_strength() -> f32 {
    0.5
}

pub fn default_hardness() -> f32 {
    0.85
}

impl Default for BrushPreset {
    fn default() -> Self {
        Self {
            name: "preset".into(),
            engine: None,
            bank: String::new(),
            size_px: 14.0,
            flow: 1.0,
            opacity: 1.0,
            dyn_size: true,
            dyn_opacity: false,
            min_size: 0.0,
            color: None,
            tilt_size: false,
            tilt_opacity: false,
            tilt_shape: false,
            tilt_strength: default_tilt_strength(),
            hardness: default_hardness(),
            aliased: false,
        }
    }
}

/// Starter presets mapped to the anime pipeline stages.
/// THE PENCIL BOX (genga room charter): the three named pencils, hotkeys
/// 1/2/3. Numbers ratified 2026-08-17 (research/ROOM-CHARTERS.md) — each is
/// justified against the engine's pressure pipeline, not taste.
pub fn pencil_box_presets() -> [BrushPreset; 3] {
    [
        // The ao construction pencil: builds tone by hatching, never solid —
        // "does not photograph" encoded as an opacity ceiling.
        BrushPreset {
            name: "atari".into(),
            engine: None,
            bank: String::new(),
            size_px: 14.0,
            flow: 0.45,
            opacity: 0.8,
            dyn_size: true,
            dyn_opacity: true,
            min_size: 0.15,
            color: Some([83, 137, 196, 255]), // ao
            tilt_size: true,
            tilt_opacity: true,
            tilt_shape: true,
            tilt_strength: 0.7,
            hardness: default_hardness(),
            aliased: false,
        },
        // The committed key line: pressure drives SIZE only, tight floor —
        // a light touch is a thin black line, never a grey one.
        BrushPreset {
            name: "genga".into(),
            engine: None,
            bank: String::new(),
            size_px: 6.0,
            flow: 1.0,
            opacity: 1.0,
            dyn_size: true,
            dyn_opacity: false,
            min_size: 0.3,
            color: Some([25, 25, 30, 255]), // ink
            tilt_size: false,
            tilt_opacity: false,
            tilt_shape: false,
            tilt_strength: 0.5,
            hardness: default_hardness(),
            aliased: false,
        },
        // The sakkan's correction: faint trial stroke at light pressure,
        // unmistakably loud over both other inks at full press.
        BrushPreset {
            name: "shusei".into(),
            engine: None,
            bank: String::new(),
            size_px: 8.0,
            flow: 0.7,
            opacity: 1.0,
            dyn_size: true,
            dyn_opacity: true,
            min_size: 0.2,
            color: Some([228, 82, 47, 255]), // aka
            tilt_size: true,
            tilt_opacity: false,
            tilt_shape: true,
            tilt_strength: 0.5,
            hardness: default_hardness(),
            aliased: false,
        },
    ]
}

pub fn default_presets() -> Vec<BrushPreset> {
    let mut v: Vec<BrushPreset> = pencil_box_presets().to_vec();
    v.extend([
        BrushPreset {
            name: "shiage fill".into(),
            size_px: 60.0,
            dyn_size: false,
            ..Default::default()
        },
        BrushPreset {
            name: "shadow airbrush".into(),
            size_px: 120.0,
            flow: 0.25,
            dyn_size: false,
            dyn_opacity: true,
            ..Default::default()
        },
    ]);
    v
}

// ---- Cel-layer defaults ----------------------------------------------------

/// Default brush colour per cel-layer NAME. Switching the active layer loads
/// its colour (a colour picked during the session overrides the default for
/// that layer name until the app closes). Editable in Settings → Layers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayersConfig {
    #[serde(default = "default_layer_colors")]
    pub colors: std::collections::BTreeMap<String, [u8; 4]>,
    /// PSD-cel-layers: the artist's OWN stack presets (built-ins are
    /// compiled in and never stored). Per-user — never on the wire.
    #[serde(default)]
    pub stacks: Vec<StackPreset>,
    /// Which stack each room's new cels get (room key → preset name).
    /// Picking a stack in the strip rebinds the CURRENT room.
    #[serde(default = "default_room_stacks")]
    pub room_stack: std::collections::BTreeMap<String, String>,
}

impl Default for LayersConfig {
    fn default() -> Self {
        Self {
            colors: default_layer_colors(),
            stacks: Vec::new(),
            room_stack: default_room_stacks(),
        }
    }
}

/// PSD-cel-layers: a layer-stack preset — STRUCTURE only (names, roles,
/// identity, lock, onion, opacity, visibility; never pixels).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StackPreset {
    pub name: String,
    /// Bottom → top.
    pub layers: Vec<anim_core::model::LayerProps>,
    /// The layer (by name) a fresh cel from this stack arms.
    pub active: String,
}

impl StackPreset {
    /// "color · shadow · line" — top-first, like the strip.
    pub fn summary(&self) -> String {
        self.layers
            .iter()
            .rev()
            .map(|l| l.name.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// The armed layer's (name, role) — the named one, else the top.
    pub fn armed(&self) -> (String, anim_core::model::LayerRole) {
        self.layers
            .iter()
            .find(|l| l.name == self.active)
            .or(self.layers.last())
            .map(|l| (l.name.clone(), l.role))
            .unwrap_or(("line".into(), anim_core::model::LayerRole::Line))
    }
}

/// The built-in stacks for the modern digital anime pipeline (research/
/// PSD-cel-layers.md §4). Compiled in, read-only (duplicate to edit).
pub fn builtin_stacks() -> Vec<StackPreset> {
    use anim_core::model::{LayerProps, LayerRole as R};
    let l = |role: R, opacity: f32, locked: bool| LayerProps {
        name: role.default_name().into(),
        role,
        opacity,
        locked,
        ..LayerProps::default()
    };
    vec![
        StackPreset {
            name: "Solo merged".into(),
            layers: vec![l(R::Color, 1.0, false), l(R::Line, 1.0, false)],
            active: "line".into(),
        },
        StackPreset {
            name: "Layout (LO)".into(),
            layers: vec![l(R::Reference, 0.4, true), l(R::Rough, 1.0, false)],
            active: "rough".into(),
        },
        StackPreset {
            name: "Genga".into(),
            layers: vec![
                l(R::Rough, 0.5, false),
                l(R::Line, 1.0, false),
                l(R::Correction, 1.0, false),
            ],
            active: "line".into(),
        },
        StackPreset {
            name: "Douga".into(),
            layers: vec![
                l(R::ShadowLine, 1.0, false),
                l(R::HighlightLine, 1.0, false),
                l(R::Line, 1.0, false),
            ],
            active: "line".into(),
        },
        StackPreset {
            name: "Shiage (full)".into(),
            layers: vec![
                l(R::Color, 1.0, false),
                l(R::Shadow, 1.0, false),
                l(R::Highlight, 1.0, false),
                l(R::ShadowLine, 1.0, true),
                l(R::HighlightLine, 1.0, true),
                l(R::Line, 1.0, true),
                l(R::Correction, 1.0, false),
            ],
            active: "color".into(),
        },
    ]
}

/// Room → stack defaults: layout rooms block against the plate, the
/// drawing room keeps the merged solo pair, shiage gets the full stack.
pub fn default_room_stacks() -> std::collections::BTreeMap<String, String> {
    [
        ("layout", "Layout (LO)"),
        ("drawing", "Solo merged"),
        ("finishing", "Shiage (full)"),
        ("edit", "Solo merged"),
        ("custom", "Solo merged"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

impl LayersConfig {
    /// Every stack the artist can pick: built-ins first, then their own
    /// (a user stack sharing a built-in's name shadows it).
    pub fn all_stacks(&self) -> Vec<StackPreset> {
        let mut out: Vec<StackPreset> = builtin_stacks()
            .into_iter()
            .filter(|b| !self.stacks.iter().any(|u| u.name == b.name))
            .collect();
        out.extend(self.stacks.iter().cloned());
        out
    }

    /// The stack bound to `room` (falls back to Solo merged if the bound
    /// name no longer exists — a deleted user stack never strands a room).
    pub fn stack_for_room(&self, room: &str) -> StackPreset {
        let all = self.all_stacks();
        let want = self
            .room_stack
            .get(room)
            .cloned()
            .unwrap_or_else(|| "Solo merged".into());
        all.iter()
            .find(|s| s.name == want)
            .cloned()
            .or_else(|| all.into_iter().next())
            .expect("built-ins are never empty")
    }
}

/// Anime-pipeline conventions: ink line, blue-pencil rough, cool shadow,
/// warm highlight, sakkan-red correction.
pub fn default_layer_colors() -> std::collections::BTreeMap<String, [u8; 4]> {
    [
        ("line", [26, 26, 26, 255]),
        ("color", [222, 178, 140, 255]),
        ("shadow", [96, 112, 192, 255]),
        ("highlight", [255, 233, 168, 255]),
        ("correction", [224, 72, 72, 255]),
        ("rough", [116, 168, 232, 255]),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Fixed display order for the Layers settings page (pipeline order, top-first).
pub const LAYER_COLOR_ORDER: [&str; 6] = [
    "correction",
    "line",
    "rough",
    "highlight",
    "shadow",
    "color",
];

/// Which Settings page is showing (Krita-style category sidebar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsCategory {
    #[default]
    Shortcuts,
    /// GM2: everyday behaviour — autosave interval.
    General,
    Performance,
    Pen,
    Layers,
    Brushes,
    UiFeatures,
    /// STAGE 4 (PSD-multiplayer-rescope): application-layout presets —
    /// per-user by default; the host's layout is loadable in a session.
    Layout,
    Session,
    Plugins,
}

/// STAGE 4: what the Layout page asked for (handled by the App, which
/// owns the dock + the workspace list).
pub enum LayoutAct {
    Save(String),
    Load(String),
    Delete(String),
    LoadHost,
}

/// THE SESSION's identity + connection config (PSD-session-room).
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct SessionConfig {
    /// The name other artists see on your cursor.
    #[serde(default = "default_username")]
    pub username: String,
    /// The room's API key (base32). The HOST generates it and shares it
    /// with invited artists; a guest pastes the host's key here.
    #[serde(default)]
    pub api_key: String,
    /// The host's TOTP secret (base32) — enrolled into Authy by manual
    /// entry. Guests never need this; they need the host's CODE.
    #[serde(default)]
    pub totp_secret: String,
    #[serde(default = "default_port")]
    pub host_port: u16,
    /// The last room address joined (host:port).
    #[serde(default)]
    pub last_addr: String,
    /// Require the room key to join (OFF while the testing phase lasts —
    /// LAN-open rooms; the owner's amendment 2026-08-19).
    #[serde(default)]
    pub require_key: bool,
    /// GM2: the most recent project files, newest first (deduped, at most
    /// `RECENT_MAX`). Missing files stay listed, greyed, until removed.
    #[serde(default)]
    pub recent: Vec<PathBuf>,
    /// GM2: autosave interval in minutes (0 = off; Settings ▸ General).
    #[serde(default = "default_autosave_minutes")]
    pub autosave_minutes: u32,
}

/// GM2: how many recent files the Open menu keeps.
pub const RECENT_MAX: usize = 12;

fn default_autosave_minutes() -> u32 {
    5
}

impl SessionConfig {
    /// GM2: put `path` at the front of the recent list (deduped, capped).
    pub fn push_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT_MAX);
    }

    /// GM2: the recent list with each entry's "still on disk" flag (the
    /// Open Recent menu checks them the same way, once as it opens).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn recent_entries(&self) -> Vec<(PathBuf, bool)> {
        crate::worksafe::on_disk(&self.recent)
    }
}

fn default_update_repo() -> String {
    "LsXavec/GabanStudio".into()
}

fn default_username() -> String {
    "artist".into()
}
fn default_port() -> u16 {
    41100
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            username: default_username(),
            api_key: String::new(),
            totp_secret: String::new(),
            host_port: default_port(),
            last_addr: String::new(),
            require_key: false,
            recent: Vec::new(),
            autosave_minutes: default_autosave_minutes(),
        }
    }
}

/// UI behaviour toggles (Settings -> UI Features).
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct UiConfig {
    /// Freeze the pane layout: no dragging, splitting, or closing panes
    /// until unlocked. The arrangement itself is untouched.
    #[serde(default)]
    pub lock_positions: bool,
    /// Exact UI descriptions: every control's hover names its kind, label
    /// and source precisely, and the x-sheet gains a pointer inspector —
    /// so edits can be asked for by an element's true name.
    #[serde(default)]
    pub exact_descriptions: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub keybinds: Vec<Binding>,
    /// GM3: the preset under the keybinds (Settings › Shortcuts).
    #[serde(default)]
    pub keymap: Keymap,
    #[serde(default)]
    pub perf: PerfConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub session: SessionConfig,
    /// PSD-shipping: the GitHub repo ("owner/repo") whose latest release
    /// is the published-build channel. Defaults to the app's own repo so
    /// testers update with zero setup; empty turns checks off.
    #[serde(default = "default_update_repo")]
    pub update_repo: String,
    #[serde(default)]
    pub pen: PenConfig,
    #[serde(default)]
    pub layers: LayersConfig,
    #[serde(default = "default_presets")]
    pub presets: Vec<BrushPreset>,
    /// Transient: last Krita-import result line (not persisted).
    #[serde(skip)]
    pub last_import_note: String,
    /// Transient: what the tolerant load reset (shown once on the status
    /// lane, then cleared; not persisted).
    #[serde(skip)]
    pub load_notes: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keybinds: default_binds(Keymap::Gaban),
            keymap: Keymap::Gaban,
            perf: PerfConfig::default(),
            ui: UiConfig::default(),
            session: SessionConfig::default(),
            update_repo: default_update_repo(),
            pen: PenConfig::default(),
            layers: LayersConfig::default(),
            presets: default_presets(),
            last_import_note: String::new(),
            load_notes: Vec::new(),
        }
    }
}

/// The real data dir, `%APPDATA%/AnimStudio` (D11). It holds config.json,
/// workspaces.json, autosave/, crash.log and running-*.json. Resolved by
/// main() and the real-dir `load()`/`save()` wrappers only; everything else
/// takes a dir. Under `cfg(test)` it refuses outright: tests pass their own
/// temp dir and never reach the user's files (NEVER-DO 5).
#[cfg(not(test))]
pub fn data_dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("AnimStudio"))
}
#[cfg(test)]
pub fn data_dir() -> Option<PathBuf> {
    panic!("tests never compute the real data dir — pass a temp dir (GM1)")
}

/// `config.json` inside a data dir.
pub fn config_path_in(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// The autosave folder inside a data dir (GM2's `worksafe` writes there;
/// the testkit pins the path).
pub fn autosave_dir_in(dir: &Path) -> PathBuf {
    dir.join("autosave")
}

/// Write `bytes` to `path` through `<file>.tmp` + rename (D11): a reader
/// only ever sees the old file or the whole new one, never a torn write.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        let mut f = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut f, bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// What a tolerant config load gave up: whole sections back at their
/// defaults, and single list entries skipped — in the artist's words.
#[derive(Default)]
struct Dropped {
    /// Sections reset to defaults ("interface", "shortcuts", ...).
    reset: Vec<&'static str>,
    /// Entries skipped ("1 shortcut", "2 brush presets").
    skipped: Vec<String>,
}

/// A config section's name as the artist knows it (the Settings pages).
fn section_name(key: &str) -> &'static str {
    match key {
        "keybinds" | "keymap" => "shortcuts",
        "perf" => "performance",
        "ui" => "interface",
        "session" => "session",
        "update_repo" => "update source",
        "pen" => "pen",
        "layers" => "layer settings",
        "presets" => "brush presets",
        _ => "settings",
    }
}

/// Read one section of a config object: absent keeps the default
/// silently; present but of the wrong shape keeps the default and names
/// the section in `dropped`.
fn read_section<T: serde::de::DeserializeOwned>(
    map: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    slot: &mut T,
    dropped: &mut Dropped,
) {
    if let Some(v) = map.get(key) {
        match serde_json::from_value::<T>(v.clone()) {
            Ok(t) => *slot = t,
            Err(_) => dropped.reset.push(section_name(key)),
        }
    }
}

/// Read a list element by element: one bad entry is skipped (and counted
/// in `dropped` as `what`, singular and plural), every readable entry
/// survives. None = not a list.
fn read_list<T: serde::de::DeserializeOwned>(
    v: &serde_json::Value,
    what: (&str, &str),
    dropped: &mut Dropped,
) -> Option<Vec<T>> {
    let items = v.as_array()?;
    let mut out = Vec::with_capacity(items.len());
    let mut bad = 0usize;
    for it in items {
        match serde_json::from_value::<T>(it.clone()) {
            Ok(t) => out.push(t),
            Err(_) => bad += 1,
        }
    }
    if bad > 0 {
        let noun = if bad == 1 { what.0 } else { what.1 };
        dropped.skipped.push(format!("{bad} {noun}"));
    }
    Some(out)
}

/// Keep the unreadable original beside the file (`<file>.bad`) and say,
/// in ONE note, what was reset. Nothing dropped = no note, no copy.
fn note_dropped(path: &Path, bytes: &[u8], dropped: &Dropped, notes: &mut Vec<String>) {
    let mut said = Vec::new();
    if !dropped.reset.is_empty() {
        said.push(format!("{} reset to defaults", dropped.reset.join(", ")));
    }
    if !dropped.skipped.is_empty() {
        said.push(format!("{} dropped", dropped.skipped.join(", ")));
    }
    if said.is_empty() {
        return;
    }
    let mut bad = path.as_os_str().to_owned();
    bad.push(".bad");
    let _ = std::fs::write(PathBuf::from(&bad), bytes);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    notes.push(format!(
        "Settings: {} — your old file is kept as {name}.bad",
        said.join(", ")
    ));
}

impl Config {
    /// Load config from the real data dir (see [`Config::load_from`]); the
    /// notes ride along in `load_notes` for the status lane.
    pub fn load() -> Self {
        let Some(dir) = data_dir() else {
            let mut cfg = Self::default();
            cfg.fill_missing(false);
            return cfg;
        };
        let (mut cfg, notes) = Self::load_from(&dir);
        cfg.load_notes = notes;
        cfg
    }

    /// Load `dir/config.json`, TOLERANT section by section (D11): a section
    /// of the wrong shape falls back to its default, keybinds and presets
    /// are read element by element, and every readable part survives. When
    /// anything was reset the original bytes are kept as `config.json.bad`
    /// and exactly one note says what. A leftover `config.json.tmp` (a save
    /// cut short) is never read. Missing actions (e.g. added in a newer
    /// build) are filled in with their defaults.
    pub fn load_from(dir: &Path) -> (Self, Vec<String>) {
        let path = config_path_in(dir);
        let mut cfg = Self::default();
        let mut notes = Vec::new();
        // GM3: a file with no keymap was saved before the presets (and
        // their extra chords) existed.
        let mut before_keymaps = false;
        if let Ok(bytes) = std::fs::read(&path) {
            let mut dropped = Dropped::default();
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(serde_json::Value::Object(map)) => {
                    before_keymaps = !map.contains_key("keymap");
                    cfg.read_sections(&map, &mut dropped);
                }
                _ => dropped.reset.push("everything"),
            }
            note_dropped(&path, &bytes, &dropped, &mut notes);
        }
        cfg.fill_missing(before_keymaps);
        (cfg, notes)
    }

    fn read_sections(
        &mut self,
        map: &serde_json::Map<String, serde_json::Value>,
        dropped: &mut Dropped,
    ) {
        if let Some(v) = map.get("keybinds") {
            match read_list::<Binding>(v, ("shortcut", "shortcuts"), dropped) {
                Some(list) => self.keybinds = list,
                None => dropped.reset.push(section_name("keybinds")),
            }
        }
        read_section(map, "keymap", &mut self.keymap, dropped);
        read_section(map, "perf", &mut self.perf, dropped);
        read_section(map, "ui", &mut self.ui, dropped);
        read_section(map, "session", &mut self.session, dropped);
        read_section(map, "update_repo", &mut self.update_repo, dropped);
        read_section(map, "pen", &mut self.pen, dropped);
        read_section(map, "layers", &mut self.layers, dropped);
        if let Some(v) = map.get("presets") {
            match read_list::<BrushPreset>(v, ("brush preset", "brush presets"), dropped) {
                Some(list) => self.presets = list,
                None => dropped.reset.push(section_name("presets")),
            }
        }
    }

    /// Save to the real data dir (see [`Config::save_to`]).
    pub fn save(&self) {
        if let Some(dir) = data_dir() {
            let _ = self.save_to(&dir);
        }
    }

    /// Save to `dir/config.json` atomically (`config.json.tmp` + rename).
    pub fn save_to(&self, dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(&config_path_in(dir), json.as_bytes())
    }

    /// Every action bound (missing ones get their default chords where no
    /// other binding already uses them) and the pencil box present.
    /// `before_keymaps`: the file predates GM3's presets, so a binding
    /// still at its preset chord gains the preset's extra chords that are
    /// free. In a later file an empty `also` is the artist's choice and
    /// stays empty.
    fn fill_missing(&mut self, before_keymaps: bool) {
        let cfg = self;
        for a in Action::ALL {
            if !cfg.keybinds.iter().any(|b| b.action == *a) {
                let chord = a.default_chord(cfg.keymap).filter(|c| cfg.conflict(*a, c).is_none());
                let also = cfg.free_chords(*a, a.default_also());
                cfg.keybinds.push(Binding {
                    action: *a,
                    chord,
                    also,
                });
            }
        }
        // GM3: configs saved before Ctrl+Shift+Z redid and F3 searched.
        if before_keymaps {
            for i in 0..cfg.keybinds.len() {
                let b = &cfg.keybinds[i];
                if b.also.is_empty() && b.chord == b.action.default_chord(cfg.keymap) {
                    let a = b.action;
                    cfg.keybinds[i].also = cfg.free_chords(a, a.default_also());
                }
            }
        }
        // The pencil box must exist even in configs saved before it
        // shipped; a user's own edits to same-named presets are kept.
        for p in pencil_box_presets().into_iter().rev() {
            if !cfg.presets.iter().any(|q| q.name == p.name) {
                cfg.presets.insert(0, p);
            }
        }
    }

    /// Rebuild ONLY the key bindings from their defaults — the rest of
    /// the config (presets, palettes, session, perf) is untouched.
    /// AUDIT [1]: the old whole-config reset lived behind a plain button
    /// on the Shortcuts page and destroyed everything but shortcuts.
    pub fn reset_keybinds(&mut self) {
        self.keybinds = default_binds(self.keymap);
    }

    /// GM3: switch the preset. A binding still at the old preset's chord
    /// takes the new preset's; one the user changed is theirs and stays.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        let old = self.keymap;
        for b in &mut self.keybinds {
            if b.chord == b.action.default_chord(old) {
                b.chord = b.action.default_chord(keymap);
            }
        }
        self.keymap = keymap;
    }

    /// The whole binding of an action (chord and extra chords).
    pub fn binding(&self, action: Action) -> Option<&Binding> {
        self.keybinds.iter().find(|b| b.action == action)
    }
    pub fn binding_mut(&mut self, action: Action) -> &mut Binding {
        // Guaranteed present after `load()`.
        let idx = self
            .keybinds
            .iter()
            .position(|b| b.action == action)
            .expect("action present");
        &mut self.keybinds[idx]
    }
    pub fn chord_for(&self, action: Action) -> Option<&Chord> {
        self.keybinds
            .iter()
            .find(|b| b.action == action)
            .and_then(|b| b.chord.as_ref())
    }

    /// True if some OTHER action already uses this chord where both could
    /// fire (conflict warning; GM3: a chord may repeat across panes).
    pub fn conflict(&self, action: Action, chord: &Chord) -> Option<Action> {
        self.keybinds
            .iter()
            .find(|b| {
                b.action != action
                    && b.action.context().overlaps(action.context())
                    && b.chords().any(|c| c == chord)
            })
            .map(|b| b.action)
    }

    /// GM3: the chords of `chords` no other action uses (see [`conflict`](Self::conflict)).
    fn free_chords(&self, action: Action, chords: Vec<Chord>) -> Vec<Chord> {
        chords.into_iter().filter(|c| self.conflict(action, c).is_none()).collect()
    }
}

/// The Settings window — Krita-style: a left category sidebar and a content
/// page. `capturing` holds the action whose new key we're waiting for.
pub fn settings_window(
    ctx: &egui::Context,
    open: &mut bool,
    config: &mut Config,
    capturing: &mut Option<RebindCapture>,
    category: &mut SettingsCategory,
    backend: &str,
    raster: Option<&mut bool>,
    session_action: &mut Option<SessionAction>,
    session_status: &str,
    hosting: bool,
    joined: bool,
    peer_names: &[String],
    update_note: &str,
    check_now: &mut bool,
    lan_rooms: &[crate::net::Beacon],
    join_room: &mut Option<String>,
    layout_names: &[String],
    host_layout: bool,
    layout_name: &mut String,
    layout_act: &mut Option<LayoutAct>,
    autosave_dir: Option<&Path>,
    show_autosaves: &mut bool,
) {
    if !*open {
        *capturing = None;
        return;
    }
    // Only the Shortcuts page captures keys.
    if *category != SettingsCategory::Shortcuts {
        *capturing = None;
    } else if let Some(cap) = capturing.as_mut() {
        // Accumulate keys as they go down; commit on the first key release, so
        // the whole held combination (Ctrl+Shift+S, A+S, …) is captured.
        let (cancel, released) = ctx.input(|i| {
            let mut cancel = false;
            let mut released = false;
            for e in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } = e
                {
                    if *key == egui::Key::Escape {
                        cancel = true;
                    } else if *pressed {
                        let name = key.name().to_string();
                        if !cap.keys.contains(&name) {
                            cap.keys.push(name);
                        }
                        cap.modifiers = *modifiers;
                    } else {
                        released = true;
                    }
                }
            }
            (cancel, released)
        });
        if cancel {
            *capturing = None;
        } else if released && !cap.keys.is_empty() {
            let b = config.binding_mut(cap.action);
            b.chord = Some(Chord::from_capture(cap.keys.clone(), cap.modifiers));
            b.also.clear();
            config.save();
            *capturing = None;
        }
    }
    egui::Window::new("Settings")
        .open(open)
        .resizable(true)
        .default_size([640.0, 560.0])
        .show(ctx, |ui| {
            crate::plate::surface(ui);
            ui.horizontal_top(|ui| {
                // Left: category sidebar (Krita's Configure-dialog nav list).
                ui.vertical(|ui| {
                    ui.set_min_width(150.0);
                    ui.add_space(4.0);
                    ui.selectable_value(
                        category,
                        SettingsCategory::Shortcuts,
                        "Keyboard Shortcuts",
                    );
                    // AUDIT [18]: a mutually-exclusive pick is a DETENT.
                    // As selectables these rendered their labels in Tally,
                    // which is the armed LAMP, never text.
                    for (cat, label) in [
                        (SettingsCategory::General, "general"),
                        (SettingsCategory::Performance, "performance"),
                        (SettingsCategory::Pen, "pen / tablet"),
                        (SettingsCategory::Layers, "layers"),
                        (SettingsCategory::Brushes, "brushes"),
                        (SettingsCategory::UiFeatures, "ui features"),
                        (SettingsCategory::Layout, "layout"),
                        (SettingsCategory::Session, "session"),
                        (SettingsCategory::Plugins, "plugins"),
                    ] {
                        if crate::plate::detent(ui, *category == cat, label).clicked() {
                            *category = cat;
                        }
                    }
                });
                ui.separator();
                // Right: the selected page.
                ui.vertical(|ui| match category {
                    SettingsCategory::Shortcuts => shortcuts_page(ui, config, capturing),
                    SettingsCategory::General => {
                        general_page(ui, config, autosave_dir, show_autosaves)
                    }
                    SettingsCategory::Performance => performance_page(ui, config, backend, raster),
                    SettingsCategory::Pen => pen_page(ui, config),
                    SettingsCategory::Layers => layers_page(ui, config),
                    SettingsCategory::Brushes => brushes_page(ui, config),
                    SettingsCategory::UiFeatures => ui_features_page(ui, config),
                    SettingsCategory::Layout => {
                        layout_page(ui, layout_names, host_layout, layout_name, layout_act)
                    }
                    SettingsCategory::Plugins => {
                        plugins_page(ui, config, update_note, check_now)
                    }
                    SettingsCategory::Session => session_page(
                        ui,
                        config,
                        lan_rooms,
                        join_room,
                        session_action,
                        session_status,
                        hosting,
                        joined,
                        peer_names,
                    ),
                });
            });
        });
}

fn shortcuts_page(ui: &mut egui::Ui, config: &mut Config, capturing: &mut Option<RebindCapture>) {
    ui.horizontal(|ui| {
        crate::plate::legend(ui, "keyboard shortcuts");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // AUDIT [1]: this called reset_to_defaults(), which is
            // `*self = Self::default()` — it wiped every brush preset,
            // layer colour, perf setting, UI toggle AND the session's
            // room key + TOTP secret, then saved. One stray click.
            // Now: shortcuts ONLY, and held, and it says what it resets.
            if crate::plate::danger(ui, "RESET SHORTCUTS") {
                config.reset_keybinds();
                config.save();
                *capturing = None;
            }
        });
    });
    ui.label(
        egui::RichText::new(
            "Click a shortcut, hold the new combination, then release to set it (Esc cancels).",
        )
        .color(crate::plate::legend_dim()),
    );
    // GM3: the preset under the bindings; your own changes stay on top.
    ui.horizontal(|ui| {
        crate::plate::legend(ui, "preset");
        for km in Keymap::ALL {
            let detent = crate::plate::detent(ui, config.keymap == *km, km.name())
                .on_hover_text(km.describe());
            if detent.clicked() && config.keymap != *km {
                config.set_keymap(*km);
                config.save();
                *capturing = None;
            }
        }
    });
    ui.separator();
    let capturing_action = capturing.as_ref().map(|c| c.action);
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("keybind_grid")
            .num_columns(4)
            .striped(true)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for action in Action::ALL {
                    let action = *action;
                    ui.label(action.label()).on_hover_text(action.help());
                    let is_capturing = capturing_action == Some(action);
                    let text = if is_capturing {
                        "hold keys, release to set…".to_string()
                    } else {
                        config
                            .binding(action)
                            .and_then(|b| b.label())
                            .unwrap_or_else(|| "—".to_string())
                    };
                    let clash = config
                        .binding(action)
                        .and_then(|b| b.chords().find_map(|c| config.conflict(action, c)));
                    let btn = egui::Button::new(text).min_size(egui::vec2(160.0, 0.0));
                    let btn = if is_capturing {
                        btn.fill(crate::plate::tally_well())
                    } else if clash.is_some() {
                        btn.fill(crate::plate::WELL)
                            .stroke(egui::Stroke::new(1.0, crate::plate::AKA))
                    } else {
                        btn
                    };
                    let mut resp = ui.add(btn);
                    if let Some(other) = clash {
                        resp = resp.on_hover_text(format!("⚠ also bound to “{}”", other.label()));
                    }
                    if resp.clicked() {
                        *capturing = if is_capturing {
                            None
                        } else {
                            Some(RebindCapture::new(action))
                        };
                    }
                    if ui.button("unbind").clicked() {
                        let b = config.binding_mut(action);
                        b.chord = None;
                        b.also.clear();
                        config.save();
                        if capturing_action == Some(action) {
                            *capturing = None;
                        }
                    }
                    // GM3: conflicts said inline, not only on hover.
                    match clash {
                        Some(other) => {
                            ui.label(
                                egui::RichText::new(format!("⚠ also {}", other.label()))
                                    .color(crate::plate::AKA),
                            );
                        }
                        None => {
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
    });
}

/// Settings -> UI Features: behaviour of the shell itself.
/// THE CONNECT WINDOW (owner's brief): the 2FA code is entered HERE, in
/// its own window, and nowhere else. Returns Some((addr, code)) once the
/// artist commits — the App performs the join.
#[allow(dead_code)] // dormant with the 2FA flow (owner 2026-08-19)
pub fn connect_window(
    ctx: &egui::Context,
    open: &mut bool,
    addr: &mut String,
    code: &mut String,
    error: &str,
    busy: bool,
) -> Option<(String, String)> {
    if !*open {
        return None;
    }
    let mut out = None;
    let mut close = false;
    egui::Window::new("Join a room")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
        ui.set_min_width(320.0);
        ui.label(
        egui::RichText::new(
        "Enter the host's address and the 6-digit code from their authenticator. Codes expire every 30 seconds and \
        each one works once.",
                )
                .color(crate::plate::legend_dim()),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
            ui.label("Room address");
            ui.add(
            egui::TextEdit::singleline(addr)
                        .hint_text("host:port")
                        .desired_width(180.0),
                );
            });
            ui.horizontal(|ui| {
            ui.label("2FA code");
            let r = ui.add(
            egui::TextEdit::singleline(code)
                        .hint_text("000000")
                        .desired_width(90.0)
                        .font(egui::TextStyle::Monospace),
                );
                code.retain(|c| c.is_ascii_digit());
                code.truncate(6);
                if r.lost_focus() {
                    // Committing with Enter is handled by the button below.
                    }
                    });
                    if !error.is_empty() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(error).color(crate::plate::AKA));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
            let ready = code.len() == 6 && !addr.trim().is_empty() && !busy;
            if ui
                    // AUDIT [46]: the label swapped width mid-request and
                    // shoved Cancel sideways under a reaching hand. The
                    // button keeps ONE width; the wait shows as a lamp.
                    .add_enabled(
                        ready,
                        egui::Button::new(if busy { "joining…" } else { "join" })
                            .min_size(egui::vec2(96.0, 24.0)),
                    )
                    .clicked()
                {
                out = Some((addr.trim().to_string(), code.clone()));
                }
                if ui.button("Cancel").clicked() {
                close = true;
                }
            });
        });
    if close {
        *open = false;
        code.clear();
    }
    out
}

/// What the Session page asks the App to do (executed where the App owns
/// the net state — the settings dialog only edits config + shows status).
#[derive(Clone)]
pub enum SessionAction {
    StartHost,
    StopHost,
    /// Open the 2FA connect window (a guest joining).
    OpenConnect,
    Leave,
}

/// Settings -> Session (PSD-session-room): username, the room key, host
/// controls, and the connect button. The 2FA CODE is entered in a separate
/// window (see `connect_window`) — never on this page.
/// STAGE 4 (PSD-multiplayer-rescope): application-layout presets. Every
/// user's default stays their own installed/configured arrangement; the
/// host's layout is LOADABLE here during a session, never forced.
fn layout_page(
    ui: &mut egui::Ui,
    names: &[String],
    host_layout: bool,
    name_buf: &mut String,
    act: &mut Option<LayoutAct>,
) {
    crate::plate::legend(ui, "layout presets");
    ui.label(
        egui::RichText::new(
            "save the current arrangement (panes + tool/view) under a \
             name, and load any preset — the same rooms as the ws menu, \
             kept here so they persist across hosted sessions.",
        )
        .size(11.0)
        .color(crate::plate::legend_dim()),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.text_edit_singleline(name_buf);
        let name = name_buf.trim().to_string();
        if ui.button("save current as").clicked() && !name.is_empty() {
            *act = Some(LayoutAct::Save(name));
            name_buf.clear();
        }
    });
    ui.add_space(6.0);
    if host_layout {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("host's layout")
                    .size(11.5)
                    .color(crate::plate::STRUCK),
            );
            ui.label(
                egui::RichText::new("from this session")
                    .size(10.0)
                    .color(crate::plate::LEGEND),
            );
            if ui.button("LOAD").clicked() {
                *act = Some(LayoutAct::LoadHost);
            }
        });
        ui.add_space(4.0);
    }
    if names.is_empty() {
        ui.label(
            egui::RichText::new("no presets saved yet")
                .size(11.0)
                .color(crate::plate::legend_dim()),
        );
    } else {
        for n in names {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(n)
                        .size(11.5)
                        .color(crate::plate::STRUCK),
                );
                if ui.button("LOAD").clicked() {
                    *act = Some(LayoutAct::Load(n.clone()));
                }
                if crate::plate::danger(ui, "DELETE") {
                    *act = Some(LayoutAct::Delete(n.clone()));
                }
            });
        }
    }
}

fn session_page(
    ui: &mut egui::Ui,
    config: &mut Config,
    lan_rooms: &[crate::net::Beacon],
    join_room: &mut Option<String>,
    action: &mut Option<SessionAction>,
    status: &str,
    hosting: bool,
    joined: bool,
    peer_names: &[String],
) {
    crate::plate::legend(ui, "session");
    ui.label(
        egui::RichText::new(
            "Draw together over a direct connection. Joining a room needs \
             its key AND a fresh 6-digit code from the host's authenticator.",
        )
        .color(crate::plate::legend_dim()),
    );
    ui.separator();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Your name");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut config.session.username).desired_width(180.0))
            .on_hover_text("the name other artists see on your cursor")
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label("Room key");
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut config.session.api_key)
                    .desired_width(280.0)
                    .password(false),
            )
            .on_hover_text("the room's API key — the host generates it; a guest pastes it here")
            .changed();
    });
    // ENROLLMENT — shown whenever this machine holds a room secret,
    // hosting or not: you enroll in Authy BEFORE you open the room, and
    // the live code is how you check the enrollment took.
    // OWNER AMENDMENT 2026-08-19: Authy is dormant — the key alone
    // gates. The enrollment UI stays in the tree for the day it returns.
    if false && !config.session.totp_secret.trim().is_empty() {
        ui.separator();
        ui.label(
            egui::RichText::new("Authenticator (host only — guests never need this)")
                .color(crate::plate::LEGEND),
        );
        ui.horizontal(|ui| {
            ui.label("Secret");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut config.session.totp_secret)
                        .desired_width(280.0)
                        .font(egui::TextStyle::Monospace),
                )
                .on_hover_text("type this into Authy as a manual-entry account")
                .changed();
        });
        if let Some((code, secs)) = crate::net::current_code(config.session.totp_secret.trim()) {
            ui.horizontal(|ui| {
                ui.label("Code now");
                ui.label(
                    egui::RichText::new(code)
                        .monospace()
                        .size(20.0)
                        .color(crate::plate::STRUCK),
                );
                ui.label(
                    egui::RichText::new(format!("{secs}s left"))
                        .monospace()
                        .color(crate::plate::LEGEND),
                );
            });
            ui.label(
                egui::RichText::new(
                    "Authy should show this same number. If it does, enrollment took.",
                )
                .color(crate::plate::legend_dim()),
            );
        } else {
            ui.label(
                egui::RichText::new("that secret is not valid base32 — regenerate it")
                    .color(crate::plate::AKA),
            );
        }
    }
    ui.separator();
    if joined {
        ui.label(egui::RichText::new("Connected to a room.").color(crate::plate::STRUCK));
        if !peer_names.is_empty() {
            ui.label(format!("With: {}", peer_names.join(", ")));
        }
        if ui.button("Leave room").clicked() {
            *action = Some(SessionAction::Leave);
        }
    } else if hosting {
        ui.label(
            egui::RichText::new("Hosting — share the room key and read codes from your Authy.")
                .color(crate::plate::STRUCK),
        );
        ui.horizontal(|ui| {
            ui.label("Port");
            crate::plate::field(
                ui,
                egui::DragValue::new(&mut config.session.host_port).range(1024..=65535),
            );
        });
        ui.label(format!(
            "Artists here: {}",
            if peer_names.is_empty() {
                "just you".to_string()
            } else {
                peer_names.join(", ")
            }
        ));
        if ui.button("Stop hosting").clicked() {
            *action = Some(SessionAction::StopHost);
        }
    } else {
        ui.label("You are offline.");
        // LAN DISCOVERY (testing phase): rooms on this network, one
        // click to join — no key, no address.
        if lan_rooms.is_empty() {
            ui.label(
                egui::RichText::new("no rooms on this network yet")
                    .size(11.0)
                    .color(crate::plate::legend_dim()),
            );
        } else {
            crate::plate::legend(ui, "rooms on this network");
            for b in lan_rooms {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&b.name)
                            .size(11.5)
                            .color(crate::plate::STRUCK),
                    );
                    if !b.open {
                        ui.label(
                            egui::RichText::new("key")
                                .size(9.5)
                                .color(crate::plate::LEGEND),
                        )
                        .on_hover_text("this room requires its key");
                    }
                    ui.label(
                        egui::RichText::new(format!("v{} · {}", b.version, b.addr))
                            .monospace()
                            .size(10.0)
                            .color(crate::plate::LEGEND),
                    );
                    if ui.button("JOIN").clicked() {
                        *join_room = Some(b.addr.clone());
                    }
                });
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .button("Host a room")
                .on_hover_text(
                    "open your file to this network — the room appears in                      everyone's rooms list",
                )
                .clicked()
            {
                *action = Some(SessionAction::StartHost);
            }
            let mut req = config.session.require_key;
            if crate::plate::latch(ui, &mut req, "require key")
                .on_hover_text(
                    "off (testing): anyone on this network joins with one                      click · on: joiners must hold the room key",
                )
                .changed()
            {
                config.session.require_key = req;
                config.save();
            }
        });
        // The host's address (owner's report: the field lived inside the
        // dormant 2FA window, so removing Authy orphaned it — join needs
        // it HERE, beside the button that uses it).
        ui.horizontal(|ui| {
            ui.label("Host address");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut config.session.last_addr)
                        .desired_width(180.0)
                        .hint_text("192.168.1.23 or host:port"),
                )
                .on_hover_text(
                    "the host's IP (they run ipconfig) — port 41100 is                      assumed unless you add :port",
                )
                .changed()
            {
                config.save();
            }
            if ui
                .button("Connect")
                .on_hover_text("join with this address and the room key above")
                .clicked()
            {
                *action = Some(SessionAction::OpenConnect);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Generate a key + secret for a new room:");
            if ui
                .button("Generate")
                .on_hover_text("a fresh room key and authenticator secret")
                .clicked()
            {
                config.session.api_key = crate::net::generate_key();
                config.session.totp_secret = crate::net::base32_encode(&{
                    let b: [u8; 20] = rand::random();
                    b
                });
                changed = true;
            }
        });
    }
    ui.separator();
    ui.label(
        egui::RichText::new(status)
            .italics()
            .color(crate::plate::LEGEND),
    );
    if changed {
        config.save();
    }
}

/// PLUGINS (PSD-brush-library, owner amendment 2026-08-19): brush
/// banks — every imported preset carries its source file as its BANK;
/// banks remove as one; the import door takes the modern formats too.
fn plugins_page(ui: &mut egui::Ui, config: &mut Config, update_note: &str, check_now: &mut bool) {
    crate::plate::legend(ui, "plugins");
    ui.separator();
    // PSD-shipping: the published-build channel.
    crate::plate::legend(ui, "updates");
    ui.horizontal(|ui| {
        ui.label("GitHub repo");
        if ui
            .add(
                egui::TextEdit::singleline(&mut config.update_repo)
                    .desired_width(220.0)
                    .hint_text("owner/repo"),
            )
            .on_hover_text(
                "the repo whose LATEST RELEASE is the published build — \
                 empty turns update checks off",
            )
            .changed()
        {
            config.save();
        }
        if ui.button("check now").clicked() {
            *check_now = true;
        }
    });
    ui.label(
        egui::RichText::new(format!(
            "this build: v{}{}",
            env!("CARGO_PKG_VERSION"),
            if update_note.is_empty() {
                String::new()
            } else {
                format!(" · {update_note}")
            }
        ))
        .size(11.0)
        .color(crate::plate::legend_dim()),
    );
    ui.add_space(8.0);
    ui.separator();
    crate::plate::legend(ui, "brush banks");
    ui.label(
        egui::RichText::new(
            "Krita .kpp / .bundle · Photoshop .abr · Procreate .brush / \
             .brushset · bare .gbr / .gih / .png as stamps. Photoshop and \
             Procreate brushes bring their tip shapes and grains; their \
             engine parameters stay with their apps — they paint with OUR \
             brush, honestly.",
        )
        .size(11.5)
        .color(crate::plate::LEGEND),
    );
    ui.add_space(6.0);
    if ui
        .button("import brush files…")
        .clicked()
        && let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "brush files",
                &["kpp", "bundle", "abr", "brush", "brushset", "gbr", "gih", "png"],
            )
            .pick_files()
    {
        let r = crate::brushbank::import_any(&paths, &mut config.presets);
        let purged = crate::brushbank::purge_unreal(&mut config.presets);
        if r.ok > 0 || purged > 0 {
            config.save();
        }
        config.last_import_note = format!(
            "imported {} · {} duplicate(s) · {} failed",
            r.ok, r.dup, r.failed
        );
    }
    if !config.last_import_note.is_empty() {
        ui.label(
            egui::RichText::new(&config.last_import_note)
                .color(crate::plate::legend_dim())
                .small(),
        );
    }
    ui.add_space(8.0);
    // The banks, each with its count and a held REMOVE.
    let bank_list = crate::brushbank::banks(&config.presets);
    let mut remove: Option<String> = None;
    for (bank, count) in &bank_list {
        ui.horizontal(|ui| {
            let shown = if bank.is_empty() { "unsorted" } else { bank };
            ui.label(
                egui::RichText::new(shown)
                    .size(11.5)
                    .color(crate::plate::STRUCK),
            );
            ui.label(
                egui::RichText::new(format!("{count} brush(es)"))
                    .monospace()
                    .size(11.0)
                    .color(crate::plate::LEGEND),
            );
            if !bank.is_empty() && crate::plate::danger(ui, "REMOVE BANK") {
                remove = Some(bank.clone());
            }
        });
    }
    if let Some(bank) = remove {
        config.presets.retain(|p| p.bank != bank);
        config.save();
        config.last_import_note = format!("removed the '{bank}' bank");
    }
    ui.add_space(8.0);
    // Dependencies (stage F2): self-contained or NAMED.
    let (no_tip, no_grain) = crate::brushbank::audit_deps(&config.presets);
    if no_tip.is_empty() && no_grain.is_empty() {
        ui.label(
            egui::RichText::new(
                "every brush is self-contained — all tips and grains cached",
            )
            .size(11.0)
            .color(crate::plate::legend_dim()),
        );
    } else {
        ui.label(
            egui::RichText::new(format!(
                "{} brush(es) missing their tip, {} missing grain — they \
                 paint as the plain dab; re-import their source to restore",
                no_tip.len(),
                no_grain.len()
            ))
            .color(crate::plate::AKA),
        );
        for n in no_tip.iter().take(8) {
            ui.label(
                egui::RichText::new(format!("  · {n}"))
                    .size(10.5)
                    .color(crate::plate::legend_dim()),
            );
        }
    }
}

/// GM2: Settings ▸ General — the autosave interval (writes only to the
/// data folder's autosave/, never the user's file) and where that folder
/// is: after a mistaken Don't Save its autosaves are the only copy.
/// "show autosave folder" only raises the flag — the shell opens it.
pub(crate) fn general_page(
    ui: &mut egui::Ui,
    config: &mut Config,
    autosave_dir: Option<&Path>,
    show_autosaves: &mut bool,
) {
    crate::plate::legend(ui, "general");
    ui.separator();
    ui.label("autosave every");
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for m in crate::worksafe::AUTOSAVE_CHOICES {
            let label = if m == 0 { "off".to_string() } else { format!("{m} min") };
            let on = config.session.autosave_minutes == m;
            if crate::plate::detent(ui, on, &label).clicked() && !on {
                config.session.autosave_minutes = m;
                changed = true;
            }
        }
    });
    ui.label(
        egui::RichText::new(
            "autosaves go to the data folder (newest 3 per project) — your file \
             changes only when you save",
        )
        .color(crate::plate::legend_dim()),
    );
    if let Some(dir) = autosave_dir {
        ui.add(
            egui::Label::new(
                egui::RichText::new(dir.display().to_string())
                    .monospace()
                    .size(10.0)
                    .color(crate::plate::LEGEND),
            )
            .selectable(true),
        );
        if ui.button("show autosave folder").clicked() {
            *show_autosaves = true;
        }
    }
    if changed {
        config.save();
    }
}

fn ui_features_page(ui: &mut egui::Ui, config: &mut Config) {
    crate::plate::legend(ui, "ui features");
    ui.separator();
    let mut changed = false;
    changed |= ui
        .checkbox(&mut config.ui.lock_positions, "Lock UI positions")
        .on_hover_text(
            "freeze the pane layout: no dragging, splitting, or closing \
             windows until unlocked. The arrangement itself is kept.",
        )
        .changed();
    changed |= ui
        .checkbox(&mut config.ui.exact_descriptions, "Exact UI descriptions")
        .on_hover_text(
            "every control's hover names its kind, label and source, and the \
             x-sheet gains a pointer inspector — useful for asking for edits \
             by an element's true name.",
        )
        .changed();
    if changed {
        config.save();
    }
}

fn performance_page(
    ui: &mut egui::Ui,
    config: &mut Config,
    backend: &str,
    raster: Option<&mut bool>,
) {
    ui.horizontal(|ui| {
        crate::plate::legend(ui, "performance");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Reset to defaults").clicked() {
                config.perf = PerfConfig::default();
                config.save();
            }
        });
    });
    ui.separator();

    // The brush ENGINE switch lives here, not in the tool strip: it is a
    // GPU backend choice, not a tool (spec §6 defect 2). None = no wgpu.
    match raster {
        Some(r) => {
            crate::plate::latch(ui, r, "GPU raster brush engine")
                .on_hover_text("off falls back to the vector brush");
        }
        None => {
            ui.label("GPU raster brush engine: unavailable (no wgpu surface)");
        }
    }
    ui.separator();
    let mut changed = false;
    egui::ScrollArea::vertical().show(ui, |ui| {
    let p = &mut config.perf;
    ui.strong("Canvas Acceleration");
    ui.add_space(4.0);
    egui::Grid::new("perf_accel")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
            ui.label("V-Sync")
                    .on_hover_text("Sync to the monitor's refresh. Off = lowest latency.");
                    ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut p.vsync, "").changed();
                    ui.label(egui::RichText::new("(applies after restart)").color(crate::plate::legend_dim()));
                });
                ui.end_row();
                ui.label("Frame latency");
                egui::ComboBox::from_id_salt("frame_latency")
                    .selected_text(match p.frame_latency {
                    FrameLatency::Low => "Low (1 frame)",
                    FrameLatency::Throughput => "Throughput (2 frames)",
                    })
                    .show_ui(ui, |ui| {
                    changed |= ui
                            .selectable_value(&mut p.frame_latency, FrameLatency::Low, "Low (1 frame)")
                            .changed();
                            changed |= ui
                            .selectable_value(
                                &mut p.frame_latency,
                                FrameLatency::Throughput,
                                "Throughput (2 frames)",
                            )
                            .changed();
                    });
                    ui.end_row();
                    ui.label("Canvas scaling filter")
                    .on_hover_text("How the painted layer is filtered when zoomed. Nearest = crisp pixels, Linear = smooth.");
                    egui::ComboBox::from_id_salt("canvas_filter")
                    .selected_text(match p.canvas_filter {
                    CanvasFilter::Nearest => "Nearest",
                    CanvasFilter::Linear => "Linear",
                    })
                    .show_ui(ui, |ui| {
                    changed |= ui
                            .selectable_value(&mut p.canvas_filter, CanvasFilter::Nearest, "Nearest")
                            .changed();
                            changed |= ui
                            .selectable_value(&mut p.canvas_filter, CanvasFilter::Linear, "Linear")
                            .changed();
                    });
                    ui.end_row();
                    ui.label("Renderer");
                    ui.add_enabled(false, egui::Label::new(backend))
                    .on_hover_text("Active GPU backend (chosen automatically by wgpu).");
                    ui.end_row();
            });
            ui.add_space(10.0);
            ui.strong("Performance");
            ui.add_space(4.0);
            egui::Grid::new("perf_general")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
            ui.label("Max FPS while painting")
                    .on_hover_text("Caps idle redraws. Playback still runs at the project frame rate.");
                    changed |= ui
                    .add(egui::Slider::new(&mut p.fps_cap, 30..=240).suffix(" fps"))
                    .changed();
                    ui.end_row();
                    ui.label("Show FPS overlay");
                    changed |= ui.checkbox(&mut p.show_fps, "").changed();
                    ui.end_row();
                    ui.label("Undo history limit")
                    .on_hover_text("Max undo steps kept in memory. 0 = unlimited.");
                    changed |= ui
                    .add(egui::DragValue::new(&mut p.undo_limit).range(0..=100_000).speed(5))
                    .changed();
                    ui.end_row();
            });
    });
    if changed {
        config.save();
    }
}

fn pen_page(ui: &mut egui::Ui, config: &mut Config) {
    // AUDIT [36]: this block sat ABOVE the page's own heading, so the
    // page opened on an orphaned control.
    {
        let mut changed = false;
        changed |= ui
            .checkbox(
                &mut config.pen.native_tablet,
                "Native tablet backend (Windows Ink) — applies after restart",
            )
            .on_hover_text(
                "Direct RealTimeStylus pen input (Krita-grade). If the app \
                 crashes with this on, launch with ANIMSTUDIO_NO_TABLET=1 to \
                 force it off, then untick here. Fallback = the standard \
                 pen-touch path.",
            )
            .changed();
        if changed {
            config.save();
        }
        ui.add_space(6.0);
    }
    ui.horizontal(|ui| {
        crate::plate::legend(ui, "pen / tablet");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Reset to defaults").clicked() {
                config.pen = PenConfig::default();
                config.save();
            }
        });
    });
    ui.separator();
    let mut changed = false;
    ui.strong("Input Pressure Curve");
    ui.label(
    egui::RichText::new(
    "Remap pen pressure before it sets line width. Drag the curve up to make light touches thicker.",
        )
        .color(crate::plate::legend_dim()),
    );
    ui.add_space(6.0);
    changed |= pressure_curve_editor(ui, &mut config.pen.pressure_curve);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("Linear").clicked() {
            config.pen.pressure_curve = PressureCurve::linear();
            changed = true;
        }
        if ui
            .button("Soft")
            .on_hover_text("Light touches produce more width — boosts a pen that feels weak")
            .clicked()
        {
            config.pen.pressure_curve = PressureCurve::soft();
            changed = true;
        }
        if ui
            .button("Hard")
            .on_hover_text("Must press harder for the same width — thinner light strokes")
            .clicked()
        {
            config.pen.pressure_curve = PressureCurve::hard();
            changed = true;
        }
    });
    ui.add_space(10.0);
    ui.separator();
    ui.label(
        egui::RichText::new("Not available on this stack:")
            .color(crate::plate::legend_dim())
            .italics(),
    );
    ui.label(
        egui::RichText::new(
            "• Tablet Input API (WinTab / Windows Ink) — the window toolkit delivers Windows Ink \
    pointer events only.\n• Use-mouse-events-for-right/middle-click — managed by the \
    window toolkit.",
        )
        .color(crate::plate::legend_dim())
        .size(11.0),
    );
    if changed {
        config.save();
    }
}

/// Layers page: default brush colour per cel-layer name. Switching the active
/// layer loads its colour; a colour picked during the session overrides the
/// default for that layer name until the app closes.
fn layers_page(ui: &mut egui::Ui, config: &mut Config) {
    let mut changed = false;
    // ---- PSD-cel-layers Stage 4: stack presets + room bindings ----------
    crate::plate::legend(ui, "layer stacks");
    ui.label(
        egui::RichText::new(
            "The layer stack a new cel gets in each room (a held cel's stack is \
             cloned first). Built-ins follow the modern pipeline; build your own \
             in the Cel Layers strip and save it from its stack menu.",
        )
        .size(11.0)
        .color(crate::plate::legend_dim()),
    );
    ui.add_space(4.0);
    let all = config.layers.all_stacks();
    for (room, label) in [
        ("layout", "layout room"),
        ("drawing", "genga room"),
        ("finishing", "shiage room"),
        ("edit", "henshū room"),
        ("custom", "custom rooms"),
    ] {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(11.5)
                    .color(crate::plate::STRUCK),
            );
            let current = config.layers.stack_for_room(room).name;
            ui.menu_button(current.clone(), |ui| {
                for s in &all {
                    if crate::plate::detent(ui, s.name == current, &s.name).clicked() {
                        config.layers.room_stack.insert(room.to_string(), s.name.clone());
                        changed = true;
                        ui.close();
                    }
                }
            });
        });
    }
    ui.add_space(6.0);
    let builtins = builtin_stacks();
    let mut duplicate: Option<StackPreset> = None;
    let mut delete: Option<String> = None;
    for s in &all {
        let is_builtin = builtins.iter().any(|b| b.name == s.name)
            && !config.layers.stacks.iter().any(|u| u.name == s.name);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&s.name)
                    .size(11.5)
                    .color(crate::plate::STRUCK),
            );
            if is_builtin {
                ui.label(
                    egui::RichText::new("built-in")
                        .size(10.0)
                        .color(crate::plate::LEGEND),
                );
                if ui
                    .small_button("duplicate")
                    .on_hover_text("make an editable copy under your own stacks")
                    .clicked()
                {
                    duplicate = Some(s.clone());
                }
            } else if crate::plate::danger(ui, "DELETE") {
                delete = Some(s.name.clone());
            }
        });
        ui.label(
            egui::RichText::new(s.summary())
                .monospace()
                .size(10.0)
                .color(crate::plate::legend_dim()),
        );
    }
    if let Some(mut s) = duplicate {
        let base = format!("{} copy", s.name);
        let mut name = base.clone();
        let mut n = 2;
        while config.layers.all_stacks().iter().any(|x| x.name == name) {
            name = format!("{base} {n}");
            n += 1;
        }
        s.name = name;
        config.layers.stacks.push(s);
        changed = true;
    }
    if let Some(name) = delete {
        config.layers.stacks.retain(|s| s.name != name);
        changed = true;
    }
    ui.add_space(10.0);

    // The name-keyed colour map predates roles. Its automatic per-layer
    // brush swap was retired (spec defect 16 — the brush colour never
    // changes without a gesture), so these are REFERENCE inks only.
    crate::plate::legend(ui, "reference inks");
    ui.label(
        egui::RichText::new(
            "Reference colours per layer name (the automatic brush swap on layer \
             change is retired — pick ink with the paint dish or pencil box).",
        )
        .size(11.0)
        .color(crate::plate::legend_dim()),
    );
    ui.add_space(8.0);
    for name in LAYER_COLOR_ORDER {
        let entry = config
            .layers
            .colors
            .entry(name.to_string())
            .or_insert([128, 128, 128, 255]);
        ui.horizontal(|ui| {
            let mut rgb = [entry[0], entry[1], entry[2]];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                *entry = [rgb[0], rgb[1], rgb[2], 255];
                changed = true;
            }
            ui.label(name);
        });
    }
    ui.add_space(8.0);
    if ui
        .button("Reset to defaults")
        .on_hover_text(
            "ink line, blue-pencil rough, cool shadow, warm highlight, sakkan-red correction",
        )
        .clicked()
    {
        config.layers.colors = default_layer_colors();
        changed = true;
    }
    if changed {
        config.save();
    }
}

/// Brushes page: edit the preset list (name, size, strength, dynamics, an
/// optional colour), reorder implicitly by position (slots 1–8 map to the
/// number keybinds), and import Krita community brushes (.kpp / .bundle).
fn brushes_page(ui: &mut egui::Ui, config: &mut Config) {
    crate::plate::legend(ui, "brush presets");
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Slots 1–8 fire from the number keys (rebindable in Shortcuts). \
    Bind a preset to a workspace in the ws menu to auto-load it per \
    workflow stage.",
        )
        .color(crate::plate::legend_dim()),
    );
    ui.add_space(8.0);
    let mut changed = false;
    let mut remove: Option<usize> = None;
    egui::ScrollArea::vertical()
        .max_height(340.0)
        .show(ui, |ui| {
            for (i, p) in config.presets.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(if i < 8 {
                                format!("[{}]", i + 1)
                            } else {
                                "[·]".into()
                            })
                            .monospace()
                            .color(crate::plate::legend_dim()),
                        );
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut p.name).desired_width(130.0))
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut p.size_px)
                                    .range(1.0..=300.0)
                                    .suffix(" px"),
                            )
                            .changed();
                        // AUDIT [12]: the smallest target in the row,
                        // beside the size field being dragged.
                        if crate::plate::danger(ui, "DELETE PRESET") {
                            remove = Some(i);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(34.0);
                        ui.spacing_mut().slider_width = 70.0;
                        changed |= ui
                            .add(egui::Slider::new(&mut p.flow, 0.05..=1.0).text("flow"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut p.opacity, 0.05..=1.0).text("opacity"))
                            .changed();
                        // AUDIT [24]: these named struct fields, not
                        // what the hand controls.
                        changed |= crate::plate::latch(ui, &mut p.dyn_size, "pressure → size")
                            .changed();
                        changed |=
                            crate::plate::latch(ui, &mut p.dyn_opacity, "pressure → opacity")
                                .changed();
                        changed |= ui
                            .add(|ui: &mut egui::Ui| {
                                crate::plate::latch(ui, &mut p.tilt_size, "tilt → size")
                            })
                            .on_hover_text("tilting the pen broadens the stroke (native ink pen)")
                            .changed();
                        changed |= ui
                            .add(|ui: &mut egui::Ui| {
                                crate::plate::latch(ui, &mut p.tilt_opacity, "tilt → opacity")
                            })
                            .on_hover_text("tilting the pen lightens the stroke (native ink pen)")
                            .changed();
                        changed |= ui
                            .checkbox(&mut p.tilt_shape, "tilt shape")
                            .on_hover_text(
                                "the stamp flattens and turns with the pen's lean (native ink pen)",
                            )
                            .changed();
                        // Optional fixed colour.
                        let mut has_color = p.color.is_some();
                        if ui
                            .checkbox(&mut has_color, "colour")
                            .on_hover_text("preset sets the brush colour when applied")
                            .changed()
                        {
                            p.color = has_color.then_some([26, 26, 26, 255]);
                            changed = true;
                        }
                        if let Some(c) = &mut p.color {
                            let mut rgb = [c[0], c[1], c[2]];
                            if ui.color_edit_button_srgb(&mut rgb).changed() {
                                *c = [rgb[0], rgb[1], rgb[2], 255];
                                changed = true;
                            }
                        }
                    });
                    ui.add_space(4.0);
                });
            }
        });
    if let Some(i) = remove {
        config.presets.remove(i);
        changed = true;
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("new preset").clicked() {
            config.presets.push(BrushPreset::default());
            changed = true;
        }
        if ui
            .button("import Krita brushes…")
            .on_hover_text(
                "community .kpp presets / .bundle packs — maps name, size, \
            opacity and flow onto our round brush (textured tips need the \
            fuller brush engine, later)",
            )
            .clicked()
            && let Some(paths) = rfd::FileDialog::new()
                .add_filter("Krita brushes", &["kpp", "bundle"])
                .pick_files()
        {
            let (ok, dup, failed) = crate::kpp::import_files(&paths, &mut config.presets);
            changed |= ok > 0;
            config.last_import_note =
                format!("imported {ok}, skipped {dup} duplicate(s), {failed} failed");
        }
    });
    if !config.last_import_note.is_empty() {
        ui.label(egui::RichText::new(&config.last_import_note).color(crate::plate::legend_dim()).small());
    }
    if changed {
        config.save();
    }
}

/// Draggable pressure-curve editor in a 0..1 box. Returns whether it changed.
fn pressure_curve_editor(ui: &mut egui::Ui, curve: &mut PressureCurve) -> bool {
    use egui::{Color32, Pos2, Rect, pos2, vec2};
    let mut changed = false;
    let (rect, _r) = ui.allocate_exact_size(vec2(220.0, 220.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4, crate::plate::WELL);
    painter.rect_stroke(
        rect,
        4,
        egui::Stroke::new(1.0, crate::plate::legend_dim()),
        egui::StrokeKind::Inside,
    );
    for i in 1..4 {
        let f = i as f32 / 4.0;
        painter.line_segment(
            [
                pos2(rect.left() + rect.width() * f, rect.top()),
                pos2(rect.left() + rect.width() * f, rect.bottom()),
            ],
            egui::Stroke::new(1.0, crate::plate::rule_beat()),
        );
        painter.line_segment(
            [
                pos2(rect.left(), rect.top() + rect.height() * f),
                pos2(rect.right(), rect.top() + rect.height() * f),
            ],
            egui::Stroke::new(1.0, crate::plate::rule_beat()),
        );
    }
    let to_screen = |p: [f32; 2]| {
        pos2(
            rect.left() + p[0] * rect.width(),
            rect.bottom() - p[1] * rect.height(),
        )
    };
    let to_curve = |s: Pos2| {
        [
            ((s.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            ((rect.bottom() - s.y) / rect.height()).clamp(0.0, 1.0),
        ]
    };
    // Plot the true (clamped) transfer function by sampling apply().
    let line: Vec<Pos2> = (0..=64)
        .map(|i| {
            let x = i as f32 / 64.0;
            to_screen([x, curve.apply(x)])
        })
        .collect();
    painter.add(egui::Shape::line(
        line,
        egui::Stroke::new(2.0, crate::plate::AO),
    ));
    let n = curve.points.len();
    for i in 1..n.saturating_sub(1) {
        let center = to_screen(curve.points[i]);
        let hr = ui.interact(
            Rect::from_center_size(center, vec2(16.0, 16.0)),
            ui.id().with(("pcurve", i)),
            egui::Sense::drag(),
        );
        if hr.dragged() {
            let np = to_curve(center + hr.drag_delta());
            let xmin = curve.points[i - 1][0] + 0.02;
            let xmax = curve.points[i + 1][0] - 0.02;
            curve.points[i] = [np[0].clamp(xmin, xmax), np[1].clamp(0.0, 1.0)];
            changed = true;
        }
        let col = if hr.hovered() || hr.dragged() {
            Color32::WHITE
        } else {
            crate::plate::AO
        };
        painter.circle_filled(center, 5.0, col);
    }
    painter.circle_filled(to_screen(curve.points[0]), 4.0, crate::plate::STRUCK);
    painter.circle_filled(to_screen(curve.points[n - 1]), 4.0, crate::plate::STRUCK);
    changed
}
