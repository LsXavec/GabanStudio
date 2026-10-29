//! GM3: the History pane — every undo step as a row (its label and who
//! made it), the current step armed, the saved point marked. Clicking a
//! row undoes or redoes single steps until that row is current.

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
    let rows = state.engine.history();
    let pos = state.engine.position();
    // The saved point means something only once there is a file.
    let saved = state.file_path.is_some().then_some(state.saved_rev);
    let base = state.engine.base_revision();
    let mut jump = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            row(ui, 0, "start", None, pos == 0, saved == Some(base), &mut jump);
            for (i, e) in rows.iter().enumerate() {
                let who = e.author.as_deref().unwrap_or("you");
                let at_saved = saved == Some(e.revision);
                row(ui, i + 1, &e.label, Some(who), pos == i + 1, at_saved, &mut jump);
            }
        });
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

fn row(
    ui: &mut egui::Ui,
    n: usize,
    label: &str,
    author: Option<&str>,
    current: bool,
    saved: bool,
    jump: &mut Option<usize>,
) {
    ui.horizontal(|ui| {
        if plate::detent(ui, current, &row_label(n, label)).clicked() {
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
