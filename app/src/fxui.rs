//! GM9: the Effect node's inspector, generated from its FX kind's
//! parameter specs (`anim_core::fx`) — a Float is a drag value with its
//! range, a Color a colour button, a Bool a latch, a Choice a combo — and
//! every parameter has a bind picker beside it (a bound colour shows its
//! second chip: the column's 0–1 mixes the two). Later FX kinds get an
//! inspector by declaring specs; nothing here names a kind.
//!
//! One gesture is one undo step: a drag edits a live copy held here, and
//! only the release commits it through SetNodeKind (the curves pane's law).

use anim_core::camera::{CAMERA_SPECS, MAX_PLANES, PLANE_HELP, PLANE_NAMES};
use anim_core::fx::{self, FxBinds, FxKind, FxParams, FxValue, ParamKind, ParamSpec};
use anim_core::generator::GenKind;
use anim_core::graph::{NodeKind, PlaneSpec};
use anim_core::ids::{NodeId, ParamId};
use eframe::egui;

use crate::doc::AppState;

/// The inspector's session-only state.
#[derive(Default)]
pub struct FxEdit {
    /// A value mid-gesture (node, stored key, value): shown, not committed.
    live: Option<(NodeId, String, FxValue)>,
    /// Where each parameter's control sat this frame, by spec key.
    pub fields: Vec<(&'static str, egui::Rect)>,
}

/// One colour chip stored under `key`: a drag in its picker shows live and
/// lands as one commit, on release.
fn chip(
    ui: &mut egui::Ui,
    edit: &mut FxEdit,
    id: NodeId,
    key: String,
    stored: FxValue,
    help: &str,
    commit: &mut Option<(String, FxValue)>,
) -> egui::Rect {
    let live = match &edit.live {
        Some((n, k, FxValue::Color(c))) if *n == id && *k == key => Some(*c),
        _ => None,
    };
    let FxValue::Color(c) = live.map(FxValue::Color).unwrap_or(stored) else {
        return egui::Rect::NOTHING;
    };
    let mut rgb = [c[0], c[1], c[2]];
    let r = ui.color_edit_button_srgb(&mut rgb).on_hover_text(help);
    let c = [rgb[0], rgb[1], rgb[2], c[3]];
    if r.changed() {
        edit.live = Some((id, key, FxValue::Color(c)));
    } else if live.is_some() && !ui.input(|i| i.pointer.any_down()) {
        edit.live = None;
        *commit = Some((key, FxValue::Color(c)));
    }
    r.rect
}

/// A new column's name for `spec`: "Glow radius", or "Glow radius 2"… when
/// a column already has it.
#[cfg(test)]
fn fresh_column_name(fx: FxKind, spec: &ParamSpec, columns: &[(ParamId, String)]) -> String {
    column_name(fx.name(), spec, columns)
}

/// `fresh_column_name` for any node named `node` (GM10: "Camera pan x").
fn column_name(node: &str, spec: &ParamSpec, columns: &[(ParamId, String)]) -> String {
    let label = spec.label.split(" (").next().unwrap_or(spec.label);
    let base = format!("{node} {label}");
    let taken = |n: &str| columns.iter().any(|(_, c)| c == n);
    if !taken(&base) {
        return base;
    }
    let mut n = 2;
    while taken(&format!("{base} {n}")) {
        n += 1;
    }
    format!("{base} {n}")
}

/// The selected Effect node's parameters (`params`/`binds` are its stored
/// ones). Static values edit only while unbound; a bound parameter shows
/// its column's value, disabled, not hidden (UI-STABILITY law).
pub fn inspector(
    ui: &mut egui::Ui,
    state: &mut AppState,
    id: NodeId,
    fx: FxKind,
    params: &FxParams,
    binds: &FxBinds,
) {
    let resolved = state.cut().fx_at(fx, params, binds, state.view.frame).values;
    if let Some((params, binds)) = specs_ui(ui, state, id, fx.name(), fx.specs(), &resolved, params, binds) {
        commit_kind(state, id, NodeKind::Effect { fx, params, binds });
    }
}

/// GM10: the selected Camera node's parameters (the specs' controls, as
/// an Effect's), then its planes: a depth each, "+ plane" and "− plane".
/// One gesture is one undo step here too.
pub fn camera_inspector(
    ui: &mut egui::Ui,
    state: &mut AppState,
    id: NodeId,
    planes: &[PlaneSpec],
    params: &FxParams,
    binds: &FxBinds,
) {
    let frame = state.view.frame;
    let resolved = fx::resolve_specs(CAMERA_SPECS, params, binds, |p| {
        state.cut().param_value(p, frame)
    });
    let specs = specs_ui(ui, state, id, "Camera", CAMERA_SPECS, &resolved, params, binds);
    let mut next = planes.to_vec();
    let mut changed = false;
    ui.separator();
    for (i, plane) in planes.iter().enumerate() {
        let name = PLANE_NAMES.get(i).copied().unwrap_or("plane");
        let key = format!("{name} depth");
        let mut z = match &state.graph_ui.fx.live {
            Some((n, k, FxValue::Float(v))) if *n == id && *k == key => *v,
            _ => plane.depth,
        };
        crate::plate::legend(ui, name).on_hover_text(PLANE_HELP);
        let r = crate::plate::field(
            ui,
            egui::DragValue::new(&mut z)
                .range(-100.0..=100.0)
                .speed(0.05)
                .prefix("z ")
                .update_while_editing(false),
        )
        .on_hover_text(PLANE_HELP);
        if r.dragged() {
            state.graph_ui.fx.live = Some((id, key, FxValue::Float(z)));
        }
        if r.drag_stopped() || (r.changed() && !r.dragged()) {
            state.graph_ui.fx.live = None;
            next[i].depth = z;
            changed = true;
        }
        state.graph_ui.fx.fields.push((name, r.rect));
    }
    ui.horizontal(|ui| {
        let add = ui
            .add_enabled(planes.len() < MAX_PLANES, egui::Button::new("+ plane"))
            .on_hover_text("add a multiplane level behind the last one (one more input pin)");
        if add.clicked() {
            let z = planes.last().map_or(0.0, |p| p.depth + 1.0);
            next.push(PlaneSpec { depth: z });
            changed = true;
        }
        let remove = ui
            .add_enabled(planes.len() > 1, egui::Button::new("− plane"))
            .on_hover_text("remove the last plane and its input pin (undo restores its wire)");
        if remove.clicked() {
            next.pop();
            changed = true;
        }
    });
    if specs.is_none() && !changed {
        return;
    }
    let (params, binds) = specs.unwrap_or_else(|| (params.clone(), binds.clone()));
    commit_kind(
        state,
        id,
        NodeKind::Camera {
            planes: next,
            params,
            binds,
        },
    );
}

/// GM16: the selected Generator node's parameters — its kind's specs'
/// controls, as an Effect's (a Text parameter is a text field that commits
/// when it loses focus).
pub fn gen_inspector(
    ui: &mut egui::Ui,
    state: &mut AppState,
    id: NodeId,
    generator: GenKind,
    params: &FxParams,
    binds: &FxBinds,
) {
    let resolved = state.cut().gen_at(generator, params, binds, state.view.frame).values;
    let specs = generator.specs();
    if let Some((params, binds)) = specs_ui(ui, state, id, generator.name(), specs, &resolved, params, binds) {
        commit_kind(
            state,
            id,
            NodeKind::Generator {
                generator,
                params,
                binds,
            },
        );
    }
}

/// Writes `kind` to node `id` as one undo step, unless it is what the node
/// already holds (a gesture that ends where it began writes nothing).
fn commit_kind(state: &mut AppState, id: NodeId, kind: NodeKind) {
    if state.cut().graph.node(id).is_ok_and(|n| n.kind != kind) {
        state.graph_set_kind(id, kind);
    }
}

/// The controls for `specs` of the node `id` called `node_name` (`resolved` is
/// this frame's value of each spec, `params`/`binds` the stored ones), and
/// the node's new params and binds when a gesture ended or a bind changed.
#[allow(clippy::too_many_arguments)]
fn specs_ui(
    ui: &mut egui::Ui,
    state: &mut AppState,
    id: NodeId,
    node_name: &str,
    specs: &'static [ParamSpec],
    resolved: &[FxValue],
    params: &FxParams,
    binds: &FxBinds,
) -> Option<(FxParams, FxBinds)> {
    let columns: Vec<(ParamId, String)> = state
        .cut()
        .xsheet
        .params
        .iter()
        .map(|p| (p.id, p.name.clone()))
        .collect();
    state.graph_ui.fx.fields.clear();
    // A drag's live value outlives no drag: one this inspector never saw
    // end (it was hidden mid-drag) is dropped.
    let held = ui.input(|i| i.pointer.any_down() || i.pointer.any_released());
    if !held && matches!(state.graph_ui.fx.live, Some((_, _, FxValue::Float(_)))) {
        state.graph_ui.fx.live = None;
    }
    let mut commit: Option<(String, FxValue)> = None;
    let mut rebind: Option<(&'static str, Option<ParamId>)> = None;
    for (spec, now) in specs.iter().zip(resolved) {
        let key = spec.key;
        let bound = binds.get(key).filter(|_| spec.bindable());
        let live = match &state.graph_ui.fx.live {
            Some((n, k, v)) if *n == id && k == key => Some(v.clone()),
            _ => None,
        };
        let shown = live.clone().unwrap_or_else(|| now.clone());
        if !matches!(spec.kind, ParamKind::Bool { .. }) {
            crate::plate::legend(ui, spec.label).on_hover_text(spec.help);
        }
        let rect = match (spec.kind, shown) {
            (ParamKind::Float { min, max, step, .. }, FxValue::Float(mut v)) => {
                let r = ui
                    .add_enabled_ui(bound.is_none(), |ui| {
                        crate::plate::field(
                            ui,
                            egui::DragValue::new(&mut v)
                                .range(min..=max)
                                .speed(step)
                                .update_while_editing(false),
                        )
                    })
                    .inner
                    .on_hover_text(spec.help);
                if r.dragged() {
                    state.graph_ui.fx.live = Some((id, key.to_string(), FxValue::Float(v)));
                }
                // The release (or a typed value) is the one commit.
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    state.graph_ui.fx.live = None;
                    commit = Some((key.to_string(), FxValue::Float(v)));
                }
                r.rect
            }
            (ParamKind::Color { .. }, FxValue::Color(_)) => {
                // The stored chips, not this frame's mix: bound, they are
                // the column's 0 and 1, and both stay editable.
                let edit = &mut state.graph_ui.fx;
                let first = spec.read(params.get(key));
                let to = spec.to_key();
                let second = spec.read(params.get(&to));
                ui.horizontal(|ui| {
                    if bound.is_none() {
                        return chip(ui, edit, id, key.to_string(), first, spec.help, &mut commit);
                    }
                    let help = "The colour at the column's 0: the column mixes toward the second chip.";
                    let rect = chip(ui, edit, id, key.to_string(), first, help, &mut commit);
                    // The second chip shows only while a column drives the
                    // colour: unbound, nothing reads it (NEVER-DO 1).
                    ui.label("→");
                    let help = "The colour at the column's 1: between, the chips mix per channel.";
                    chip(ui, edit, id, to, second, help, &mut commit);
                    rect
                })
                .inner
            }
            (ParamKind::Bool { .. }, FxValue::Bool(mut b)) => {
                let r = ui
                    .add_enabled_ui(bound.is_none(), |ui| {
                        crate::plate::latch(ui, &mut b, spec.label)
                    })
                    .inner
                    .on_hover_text(spec.help);
                if r.changed() {
                    commit = Some((key.to_string(), FxValue::Bool(b)));
                }
                r.rect
            }
            (ParamKind::Choice { options, .. }, FxValue::Choice(s)) => {
                let mut i = options.iter().position(|o| *o == s).unwrap_or(0);
                let before = i;
                let r = ui
                    .add_enabled_ui(bound.is_none(), |ui| {
                        egui::ComboBox::from_id_salt(("fx param", id.0, key))
                            .selected_text(options[i])
                            .show_ui(ui, |ui| {
                                for (j, o) in options.iter().enumerate() {
                                    ui.selectable_value(&mut i, j, *o);
                                }
                            })
                            .response
                    })
                    .inner
                    .on_hover_text(spec.help);
                if i != before {
                    commit = Some((key.to_string(), FxValue::Choice(options[i].to_string())));
                }
                r.rect
            }
            // GM13: a file parameter — the file it holds, and a button that
            // embeds a .cube in the cut (one undo step with the node). The
            // file picker is a window: the shell opens it
            // (AppRequest::LoadLut), only on a real click.
            (ParamKind::Blob, FxValue::Blob { blob }) => {
                let name = blob.strip_prefix("lut.").unwrap_or(&blob);
                ui.horizontal(|ui| {
                    ui.label(if name.is_empty() { "no file" } else { name });
                    let load = ui.button("load .cube…").on_hover_text(spec.help);
                    if load.clicked() {
                        state.graph_ui.lut_pick = Some((id, key));
                    }
                })
                .response
                .rect
            }
            // GM13: a colour map — the palette variant it was taken from;
            // picking one sets the map (one undo step), and the palette's
            // variant edits refresh it. A variant since removed is said in
            // Aka (the node keeps its last colours).
            (ParamKind::ColorMap, FxValue::Map { from, .. }) => {
                let choices = state.palette_variant_names();
                let shown = if from.is_empty() {
                    egui::RichText::new("none")
                } else if !choices.contains(&from) {
                    egui::RichText::new(format!("{from} (gone)")).color(crate::plate::AKA)
                } else {
                    egui::RichText::new(from.clone())
                };
                let mut pick: Option<String> = None;
                let r = egui::ComboBox::from_id_salt(("fx map", id.0, key))
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        if choices.is_empty() {
                            ui.label("no palette has variants yet — add one in the palette panel");
                        }
                        for name in &choices {
                            if ui.selectable_label(*name == from, name).clicked() {
                                pick = Some(name.clone());
                            }
                        }
                    })
                    .response
                    .on_hover_text(spec.help);
                if let Some(name) = pick {
                    commit = state
                        .palette_variant_map(&name)
                        .map(|map| (key.to_string(), FxValue::Map { map, from: name }));
                }
                r.rect
            }
            // GM16: a text parameter — the words as typed (Enter starts a
            // new line), shown live and committed once, when the field lets
            // go of the keyboard (one undo step per edit).
            (ParamKind::Text { .. }, FxValue::Text { mut text }) => {
                let r = ui
                    .add(egui::TextEdit::multiline(&mut text).desired_rows(2).desired_width(f32::INFINITY))
                    .on_hover_text(spec.help);
                if r.changed() {
                    state.graph_ui.fx.live = Some((id, key.to_string(), FxValue::Text { text: text.clone() }));
                }
                if r.lost_focus() {
                    state.graph_ui.fx.live = None;
                    commit = Some((key.to_string(), FxValue::Text { text }));
                }
                r.rect
            }
            // A value that does not fit its spec cannot come out of
            // `resolve` (it reads through the spec); draw nothing.
            _ => continue,
        };
        state.graph_ui.fx.fields.push((key, rect));
        if spec.bindable() {
            // The C1 camera model, per parameter: "static" unbinds; "+ new"
            // makes a column and binds it.
            let name = bound
                .and_then(|b| columns.iter().find(|(p, _)| *p == b))
                .map(|(_, n)| format!("{n} ▾"))
                .unwrap_or_else(|| "bind ▾".into());
            let help = if matches!(spec.kind, ParamKind::Color { .. }) {
                "Drive this colour from an X-sheet param column: its 0–1 mixes the first chip \
                 toward the second. Static = not animated."
            } else {
                "Drive this parameter from an X-sheet param column; static = not animated."
            };
            ui.menu_button(name, |ui| {
                if ui.selectable_label(bound.is_none(), "static").clicked() {
                    rebind = Some((key, None));
                    ui.close();
                }
                for (pid, pname) in &columns {
                    if ui.selectable_label(bound == Some(*pid), pname).clicked() {
                        rebind = Some((key, Some(*pid)));
                        ui.close();
                    }
                }
                let fresh = column_name(node_name, spec, &columns);
                if ui
                    .button(format!("+ new column '{fresh}'"))
                    .on_hover_text("add a parameter column to the X-sheet and bind it")
                    .clicked()
                {
                    if let Some(pid) = state.add_param_column(&fresh) {
                        rebind = Some((key, Some(pid)));
                    }
                    ui.close();
                }
            })
            .response
            .on_hover_text(help);
        }
    }
    if commit.is_none() && rebind.is_none() {
        return None;
    }
    let mut params = params.clone();
    let mut binds = binds.clone();
    if let Some((key, v)) = commit {
        params.set(&key, v);
    }
    if let Some((key, b)) = rebind {
        binds.set(key, b);
    }
    Some((params, binds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fxnode_a_new_column_is_named_for_its_kind_and_parameter() {
        let columns = |names: &[&str]| -> Vec<(ParamId, String)> {
            names
                .iter()
                .enumerate()
                .map(|(i, n)| (ParamId(i as u64), n.to_string()))
                .collect()
        };
        let radius = FxKind::Glow.spec("radius").unwrap();
        assert_eq!(fresh_column_name(FxKind::Glow, radius, &[]), "Glow radius");
        let one = columns(&["Glow radius"]);
        assert_eq!(fresh_column_name(FxKind::Glow, radius, &one), "Glow radius 2");
        let two = columns(&["Glow radius", "Glow radius 2"]);
        assert_eq!(fresh_column_name(FxKind::Glow, radius, &two), "Glow radius 3");
        // A label's gloss stays out of the name.
        let tint = FxKind::Grade.spec("tint").unwrap();
        assert_eq!(fresh_column_name(FxKind::Grade, tint, &[]), "Grade tint");
    }
}
