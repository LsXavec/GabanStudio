//! Keyboard-shortcut collection (D6), moved out of main.rs: which bound
//! [`Action`]s fired this frame. The TEXT-FOCUS RULE lives here — while an
//! egui text field has keyboard focus the keys belong to the field: chords
//! without Ctrl or Alt never fire (B, G, Space… type into the field) and
//! Ctrl+Z/Y/A/C/V/X stay the field's own undo/redo/select/clipboard.
//! Ctrl+S and the other app chords still reach their actions.
//!
//! GM3: an action fires on any of its chords (Redo on Ctrl+Y and
//! Ctrl+Shift+Z), and a chord several actions share fires only the one
//! whose context is Global or the pane the keys are aimed at (`here`: the
//! pane under the pointer, else the focused pane). A chord only one action
//! uses fires anywhere, as before.

use eframe::egui;

use crate::config::{Action, Binding, Chord, Ctx};

/// Every action whose chord fired this frame in the root viewport, in
/// [`Action::ALL`] order.
pub fn collect(ctx: &egui::Context, binds: &[Binding], here: Ctx) -> Vec<Action> {
    let typing = ctx.text_edit_focused();
    ctx.input(|i| fired(i, binds, typing, here))
}

/// The same over one viewport's input — the popout canvas is its own OS
/// window, so a shortcut pressed there arrives in ITS input state.
pub fn collect_for(
    ctx: &egui::Context,
    viewport: egui::ViewportId,
    binds: &[Binding],
    here: Ctx,
) -> Vec<Action> {
    let typing = ctx.text_edit_focused();
    ctx.input_for(viewport, |i| fired(i, binds, typing, here))
}

fn fired(i: &egui::InputState, binds: &[Binding], typing: bool, here: Ctx) -> Vec<Action> {
    Action::ALL
        .iter()
        .copied()
        .filter(|a| {
            let Some(b) = binds.iter().find(|b| b.action == *a) else {
                return false;
            };
            b.chords().any(|c| {
                (!typing || fires_over_field(c)) && c.matches(i) && aimed_here(*a, c, binds, here)
            })
        })
        .collect()
}

/// A chord another action also uses fires only in its own pane; so does
/// a destructive pane verb's chord (see [`Action::pane_only`]).
fn aimed_here(a: Action, c: &Chord, binds: &[Binding], here: Ctx) -> bool {
    if a.pane_only() {
        return a.context() == here;
    }
    let shared = binds
        .iter()
        .any(|b| b.action != a && b.chords().any(|d| d == c));
    !shared || a.context() == Ctx::Global || a.context() == here
}

/// May this chord fire while a text field has keyboard focus?
fn fires_over_field(c: &Chord) -> bool {
    if !c.ctrl && !c.alt {
        return false;
    }
    let field_key = c.ctrl
        && !c.alt
        && c.keys.len() == 1
        && matches!(c.keys[0].as_str(), "Z" | "Y" | "A" | "C" | "V" | "X");
    !field_key
}
