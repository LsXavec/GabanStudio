//! GM3: the History pane — every undo step as a row (its label and who
//! made it), the current step armed, undone steps dim, the saved point
//! marked. Clicking a row undoes or redoes single steps until that row is
//! current.

use eframe::egui;

use crate::doc::AppState;
use crate::plate;

/// The row's name: "0. start", then "n. label" (the step it applies).
pub fn row_label(n: usize, label: &str) -> String {
    format!("{n}. {label}")
}

/// `gesture_safe`: no stroke is live. `shared`: in a session undo is per
/// artist and travels to the host, so a jump is refused there.
pub fn ui(ui: &mut egui::Ui, state: &mut AppState, gesture_safe: bool, shared: bool) {
    plate::legend(ui, "history");
    let pos = state.engine.position();
    // The saved point means something only once there is a file.
    let saved = state.file_path.is_some().then_some(state.saved_rev);
    let base = state.engine.base_revision();
    let mut jump = None;
    // Row 0 is the start, row n the step n. Only the rows on screen are
    // built: a long session costs nothing per frame.
    let rows = state.engine.history_len() + 1;
    let scroll = egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, plate::CTRL_H, rows, |ui, range| {
            for n in range {
                if n == 0 {
                    let at_saved = saved == Some(base);
                    row(ui, 0, "start", None, true, pos == 0, at_saved, &mut jump);
                    continue;
                }
                let Some(e) = state.engine.history_entry(n - 1) else {
                    continue;
                };
                let who = e.author.as_deref().unwrap_or("you");
                let at_saved = saved == Some(e.revision);
                row(ui, n, &e.label, Some(who), e.applied, pos == n, at_saved, &mut jump);
            }
        });
    plate::note_viewport(ui, scroll.inner_rect);
    let Some(n) = jump.filter(|n| *n != pos) else {
        return;
    };
    if !gesture_safe {
        state.refuse("refused — finish the stroke first");
    } else if shared {
        state.refuse("refused — in a session undo goes one step at a time (yours only)");
    } else {
        state.undo_to(n);
    }
}

#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    n: usize,
    label: &str,
    author: Option<&str>,
    applied: bool,
    current: bool,
    saved: bool,
    jump: &mut Option<usize>,
) {
    ui.horizontal(|ui| {
        let name = row_label(n, label);
        // An undone step waits to be redone: dim, like every paint app's.
        let detent = if applied {
            plate::detent(ui, current, &name)
        } else {
            plate::detent_dim(ui, current, &name)
        };
        if detent.clicked() {
            *jump = Some(n);
        }
        if let Some(a) = author {
            ui.label(egui::RichText::new(a).color(plate::legend_dim()));
        }
        if saved {
            plate::legend(ui, "saved").on_hover_text("the file on disk is this state");
        }
    });
}
