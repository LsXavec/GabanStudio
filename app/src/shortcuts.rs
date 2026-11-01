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
//!
//! AU10: while any widget has keyboard focus the arrows are its own: a
//! slider steps, and egui walks the focus on from a button.

use eframe::egui;

use crate::config::{Action, Binding, Chord, Ctx};

/// Every action whose chord fired this frame in the root viewport, in
/// [`Action::ALL`] order.
pub fn collect(ctx: &egui::Context, binds: &[Binding], here: Ctx) -> Vec<Action> {
    let typing = ctx.text_edit_focused();
    let focused = ctx.memory(|m| m.focused()).is_some();
    let released = ctx.input(|i| i.key_released(egui::Key::Space));
    let hand = take_hand_drag(ctx, released);
    ctx.input(|i| fired(i, binds, typing, focused, here, hand))
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
    let focused = ctx.memory(|m| m.focused()).is_some();
    let released = ctx.input_for(viewport, |i| i.key_released(egui::Key::Space));
    let hand = take_hand_drag(ctx, released);
    ctx.input_for(viewport, |i| fired(i, binds, typing, focused, here, hand))
}

/// GM4: the canvas notes that the Space hold dragged — anything moved the
/// view while Space was down (the hand's drag or turn, a wheel zoom, a
/// middle drag) — so the release that ends the hold is not a tap.
pub fn note_hand_drag(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(hand_id(), true));
}

/// GM4: forget a stale note when a new Space hold starts.
pub fn clear_hand_drag(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<bool>(hand_id()));
}

fn hand_id() -> egui::Id {
    egui::Id::new("gm4-space-hand-dragged")
}

/// Did the Space hold drag? Space's release ends the hold, so it also
/// clears the note.
fn take_hand_drag(ctx: &egui::Context, released: bool) -> bool {
    let dragged = ctx.data(|d| d.get_temp::<bool>(hand_id()).unwrap_or(false));
    if released && dragged {
        clear_hand_drag(ctx);
    }
    dragged
}

fn fired(
    i: &egui::InputState,
    binds: &[Binding],
    typing: bool,
    focused: bool,
    here: Ctx,
    hand_dragged: bool,
) -> Vec<Action> {
    Action::ALL
        .iter()
        .copied()
        .filter(|a| {
            let Some(b) = binds.iter().find(|b| b.action == *a) else {
                return false;
            };
            b.chords().any(|c| {
                (!typing || fires_over_field(c))
                    && !(focused && c.has_arrow())
                    && c.matches(i)
                    && !(hand_dragged && c.is_space_tap())
                    && aimed_here(*a, c, binds, here)
            })
        })
        .collect()
}

/// A chord another action also uses fires only in its own pane; so does
/// a destructive pane verb's chord (see [`Action::pane_only`]).
pub(crate) fn aimed_here(a: Action, c: &Chord, binds: &[Binding], here: Ctx) -> bool {
    if a.pane_only() {
        return a.context() == here;
    }
    // GM17: View3D's verbs fire over View3D only (pane-only), so elsewhere
    // they are no rival; over View3D they shadow any verb on their chord
    // (0 is the cut camera there, RAM preview elsewhere).
    let rivals = |ctx: Option<Ctx>| {
        binds.iter().any(|b| {
            b.action != a
                && (b.action.context() != Ctx::View3D || here == Ctx::View3D)
                && ctx.is_none_or(|x| b.action.context() == x)
                && b.chords().any(|d| d == c)
        })
    };
    if here == Ctx::View3D && rivals(Some(Ctx::View3D)) {
        return false;
    }
    !rivals(None) || a.context() == Ctx::Global || a.context() == here
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
