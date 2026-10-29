//! Keyboard-shortcut collection (D6), moved out of main.rs: which bound
//! [`Action`]s fired this frame. The TEXT-FOCUS RULE lives here — while an
//! egui text field has keyboard focus the keys belong to the field: chords
//! without Ctrl or Alt never fire (B, G, Space… type into the field) and
//! Ctrl+Z/Y/A/C/V/X stay the field's own undo/redo/select/clipboard.
//! Ctrl+S and the other app chords still reach their actions.

use eframe::egui;

use crate::config::{Action, Binding, Chord};

/// Every action whose chord fired this frame in the root viewport, in
/// [`Action::ALL`] order (an action's first binding is its chord).
pub fn collect(ctx: &egui::Context, binds: &[Binding]) -> Vec<Action> {
    let typing = ctx.text_edit_focused();
    ctx.input(|i| fired(i, binds, typing))
}

/// The same over one viewport's input — the popout canvas is its own OS
/// window, so a shortcut pressed there arrives in ITS input state.
pub fn collect_for(
    ctx: &egui::Context,
    viewport: egui::ViewportId,
    binds: &[Binding],
) -> Vec<Action> {
    let typing = ctx.text_edit_focused();
    ctx.input_for(viewport, |i| fired(i, binds, typing))
}

fn fired(i: &egui::InputState, binds: &[Binding], typing: bool) -> Vec<Action> {
    Action::ALL
        .iter()
        .copied()
        .filter(|a| {
            binds
                .iter()
                .find(|b| b.action == *a)
                .and_then(|b| b.chord.as_ref())
                .is_some_and(|c| (!typing || fires_over_field(c)) && c.matches(i))
        })
        .collect()
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
