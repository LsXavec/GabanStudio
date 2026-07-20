//! The Node Graph pane: a live view + editor over the cut's REAL compositing
//! graph (in the engine since M1 — persisted, undoable, cache-evaluated).
//! Nodes drag; wires connect output→input pins; every edit is an engine
//! command. Honest note: the canvas still renders through the sandwich
//! compositor — the graph drives evaluation/caching today and becomes the
//! render path when the GPU graph compositor lands.

use anim_core::graph::{BlendMode, NodeKind};
use anim_core::ids::NodeId;
use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::doc::AppState;

const NODE_W: f32 = 150.0;
const TITLE_H: f32 = 22.0;
const PIN_STEP: f32 = 18.0;
const PIN_R: f32 = 5.0;

fn node_height(inputs: usize) -> f32 {
    TITLE_H + (inputs.max(1) as f32) * PIN_STEP + 6.0
}

fn kind_color(kind: &NodeKind) -> Color32 {
    match kind {
        NodeKind::DrawingSource { .. } => Color32::from_rgb(90, 140, 200),
        NodeKind::Solid { .. } => Color32::from_rgb(150, 120, 190),
        NodeKind::ImageSource { .. } => Color32::from_rgb(90, 180, 170),
        NodeKind::Transform { .. } => Color32::from_rgb(120, 170, 120),
        NodeKind::Blend { .. } => Color32::from_rgb(200, 150, 90),
        NodeKind::Output => Color32::from_rgb(200, 100, 100),
    }
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    state.ensure_node_layout();
    inspector_row(ui, state);
    ui.separator();

    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, Color32::from_rgb(20, 22, 26));

    // Pan with a background drag.
    if response.dragged() && state.pending_wire.is_none() {
        state.graph_pan.0 += response.drag_delta().x;
        state.graph_pan.1 += response.drag_delta().y;
    }
    let origin = rect.left_top() + vec2(state.graph_pan.0, state.graph_pan.1);
    let to_screen = |p: (f32, f32)| pos2(origin.x + p.0, origin.y + p.1);

    // Snapshot the graph (owned) so state can be mutated by interactions.
    struct NodeView {
        id: NodeId,
        kind: NodeKind,
        inputs: Vec<Option<(NodeId, u16)>>,
        rect: Rect,
    }
    let output_node = state.cut().graph.output;
    let nodes: Vec<NodeView> = state
        .cut()
        .graph
        .nodes()
        .map(|n| {
            let pos = state
                .node_positions
                .get(&n.id.0)
                .copied()
                .unwrap_or((40.0, 40.0));
            NodeView {
                id: n.id,
                kind: n.kind.clone(),
                inputs: n.inputs.clone(),
                rect: Rect::from_min_size(
                    to_screen(pos),
                    vec2(NODE_W, node_height(n.inputs.len())),
                ),
            }
        })
        .collect();

    let in_pin_pos = |nv: &NodeView, pin: usize| -> Pos2 {
        pos2(
            nv.rect.left(),
            nv.rect.top() + TITLE_H + PIN_STEP * (pin as f32 + 0.5),
        )
    };
    let out_pin_pos = |nv: &NodeView| -> Pos2 {
        pos2(nv.rect.right(), nv.rect.top() + TITLE_H * 0.5)
    };
    let find = |id: NodeId| nodes.iter().find(|n| n.id == id);

    // Wires (under nodes).
    let cable = |painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32| {
        let d = ((b.x - a.x).abs() * 0.5).max(24.0);
        painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
            [a, pos2(a.x + d, a.y), pos2(b.x - d, b.y), b],
            false,
            Color32::TRANSPARENT,
            Stroke::new(2.0, color),
        ));
    };
    for nv in &nodes {
        for (pin, input) in nv.inputs.iter().enumerate() {
            if let Some((src, _)) = input
                && let Some(sv) = find(*src)
            {
                cable(
                    &painter,
                    out_pin_pos(sv),
                    in_pin_pos(nv, pin),
                    Color32::from_gray(150),
                );
            }
        }
    }
    // Pending wire follows the pointer.
    if let Some(from) = state.pending_wire
        && let Some(fv) = find(from)
        && let Some(ptr) = ui.input(|i| i.pointer.latest_pos())
    {
        cable(&painter, out_pin_pos(fv), ptr, Color32::from_rgb(120, 190, 255));
    }

    // Nodes.
    let mut wire_drop: Option<(NodeId, u16)> = None; // input pin under a released wire
    let released = ui.input(|i| i.pointer.any_released());
    for nv in &nodes {
        let selected = state.selected_node == Some(nv.id);
        let is_output = output_node == Some(nv.id);
        let body = Color32::from_rgb(38, 41, 48);
        painter.rect_filled(nv.rect, 4, body);
        painter.rect_filled(
            Rect::from_min_size(nv.rect.left_top(), vec2(NODE_W, TITLE_H)),
            4,
            kind_color(&nv.kind).gamma_multiply(0.55),
        );
        painter.rect_stroke(
            nv.rect,
            4,
            Stroke::new(
                if selected { 2.0 } else { 1.0 },
                if selected {
                    Color32::from_rgb(120, 190, 255)
                } else if is_output {
                    Color32::from_rgb(200, 100, 100)
                } else {
                    Color32::from_gray(70)
                },
            ),
            egui::StrokeKind::Outside,
        );
        painter.text(
            nv.rect.left_top() + vec2(8.0, TITLE_H * 0.5),
            egui::Align2::LEFT_CENTER,
            if is_output {
                format!("▶ {}", nv.kind.name())
            } else {
                nv.kind.name().to_string()
            },
            egui::FontId::proportional(12.0),
            Color32::from_gray(230),
        );

        // Body drag = move; click = select; right-click = node menu.
        let body_resp = ui.interact(
            nv.rect,
            ui.id().with(("node", nv.id.0)),
            Sense::click_and_drag(),
        );
        if body_resp.dragged() {
            let e = state.node_positions.entry(nv.id.0).or_insert((40.0, 40.0));
            e.0 += body_resp.drag_delta().x;
            e.1 += body_resp.drag_delta().y;
        }
        if body_resp.clicked() {
            state.selected_node = Some(nv.id);
        }
        body_resp.context_menu(|ui| {
            if ui.button("set as output ▶").clicked() {
                state.graph_set_output(nv.id);
                ui.close();
            }
            if ui.button("remove node").clicked() {
                state.graph_remove_node(nv.id);
                ui.close();
            }
        });

        // Output pin: drag starts a wire.
        let op = out_pin_pos(nv);
        let op_rect = Rect::from_center_size(op, vec2(16.0, 16.0));
        let op_resp = ui.interact(
            op_rect,
            ui.id().with(("outpin", nv.id.0)),
            Sense::click_and_drag(),
        );
        if op_resp.drag_started() {
            state.pending_wire = Some(nv.id);
        }
        painter.circle_filled(
            op,
            PIN_R,
            if op_resp.hovered() {
                Color32::from_rgb(120, 190, 255)
            } else {
                Color32::from_gray(170)
            },
        );

        // Input pins: wire drop target; right-click disconnects.
        for pin in 0..nv.inputs.len() {
            let ip = in_pin_pos(nv, pin);
            let ip_rect = Rect::from_center_size(ip, vec2(16.0, 16.0));
            let ip_resp = ui.interact(
                ip_rect,
                ui.id().with(("inpin", nv.id.0, pin)),
                Sense::click(),
            );
            if state.pending_wire.is_some() && released && ip_resp.hovered() {
                wire_drop = Some((nv.id, pin as u16));
            }
            if ip_resp.secondary_clicked() {
                state.graph_disconnect(nv.id, pin as u16);
            }
            painter.circle_filled(
                ip,
                PIN_R,
                if ip_resp.hovered() {
                    Color32::from_rgb(120, 190, 255)
                } else if nv.inputs[pin].is_some() {
                    Color32::from_gray(200)
                } else {
                    Color32::from_gray(90)
                },
            );
        }
    }

    // Resolve the wire drag.
    if released {
        if let (Some(from), Some((to, to_pin))) = (state.pending_wire, wire_drop)
            && from != to
        {
            state.graph_connect(from, to, to_pin);
        }
        state.pending_wire = None;
    }

    // Background context menu: add nodes at the click position.
    response.context_menu(|ui| {
        let Some(ptr) = ui.ctx().pointer_interact_pos() else {
            return;
        };
        let world = (ptr.x - origin.x - 160.0, ptr.y - origin.y - 60.0);
        ui.label(egui::RichText::new("add node").weak().small());
        let columns: Vec<(anim_core::ids::ColumnId, String)> = state
            .cut()
            .xsheet
            .columns
            .iter()
            .map(|c| (c.id, c.name.clone()))
            .collect();
        for (cid, cname) in columns {
            if ui.button(format!("DrawingSource: column {cname}")).clicked() {
                state.graph_add_node(NodeKind::DrawingSource { column: cid }, world);
                ui.close();
            }
        }
        if ui.button("Solid").clicked() {
            state.graph_add_node(
                NodeKind::Solid {
                    rgba: [255, 255, 255, 255],
                },
                world,
            );
            ui.close();
        }
        if ui
            .button("Image (PNG)…")
            .on_hover_text(
                "import a background plate / reference image into this cut — \
                 stored inside the project file, wired as an ImageSource node",
            )
            .clicked()
        {
            ui.close();
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("PNG image", &["png"])
                .pick_file()
            {
                state.graph_import_image(&path, world);
            }
        }
        if ui.button("Transform").clicked() {
            state.graph_add_node(
                NodeKind::Transform {
                    translate: (0.0, 0.0),
                    scale: 1.0,
                    rotate_deg: 0.0,
                    binds: Default::default(),
                },
                world,
            );
            ui.close();
        }
        if ui.button("Blend").clicked() {
            state.graph_add_node(
                NodeKind::Blend {
                    mode: BlendMode::Normal,
                },
                world,
            );
            ui.close();
        }
        if ui.button("Output").clicked() {
            state.graph_add_node(NodeKind::Output, world);
            ui.close();
        }
    });

    // Hint when empty.
    if nodes.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "right-click to add nodes — the cut's compositing graph lives here",
            egui::FontId::proportional(13.0),
            Color32::from_gray(120),
        );
    }
}

/// Selected node's parameters, edited through SetNodeKind (undoable).
fn inspector_row(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal_wrapped(|ui| {
        let Some(id) = state.selected_node else {
            ui.label(
                egui::RichText::new("no node selected — click one; right-click adds/removes")
                    .weak(),
            );
            return;
        };
        let Ok(node) = state.cut().graph.node(id) else {
            state.selected_node = None;
            return;
        };
        let mut kind = node.kind.clone();
        let mut changed = false;
        ui.label(egui::RichText::new(kind.name()).strong().color(kind_color(&kind)));
        match &mut kind {
            NodeKind::Solid { rgba } => {
                let mut rgb = [rgba[0], rgba[1], rgba[2]];
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    *rgba = [rgb[0], rgb[1], rgb[2], rgba[3]];
                    changed = true;
                }
                let mut a = rgba[3];
                if ui
                    .add(egui::DragValue::new(&mut a).range(0..=255).prefix("a "))
                    .changed()
                {
                    rgba[3] = a;
                    changed = true;
                }
            }
            NodeKind::Transform {
                translate,
                scale,
                rotate_deg,
                binds,
            } => {
                // Static values edit only while UNBOUND — a bound param is
                // driven by its X-sheet column (shown disabled, not hidden:
                // UI-STABILITY law).
                changed |= ui
                    .add_enabled(
                        binds.tx.is_none(),
                        egui::DragValue::new(&mut translate.0).prefix("x "),
                    )
                    .changed();
                changed |= ui
                    .add_enabled(
                        binds.ty.is_none(),
                        egui::DragValue::new(&mut translate.1).prefix("y "),
                    )
                    .changed();
                changed |= ui
                    .add_enabled(
                        binds.scale.is_none(),
                        egui::DragValue::new(scale)
                            .range(0.01..=20.0)
                            .speed(0.01)
                            .prefix("s "),
                    )
                    .changed();
                changed |= ui
                    .add_enabled(
                        binds.rotate.is_none(),
                        egui::DragValue::new(rotate_deg).suffix("°"),
                    )
                    .changed();
                // Bind each param to an X-sheet parameter column (the C1
                // camera model): "static" unbinds; "+ new" creates a fresh
                // column and binds it in one click.
                let params: Vec<(anim_core::ids::ParamId, String)> = state
                    .cut()
                    .xsheet
                    .params
                    .iter()
                    .map(|p| (p.id, p.name.clone()))
                    .collect();
                ui.menu_button("binds ▾", |ui| {
                    for (label, slot, fresh) in [
                        ("x", &mut binds.tx, "cam.x"),
                        ("y", &mut binds.ty, "cam.y"),
                        ("scale", &mut binds.scale, "cam.scale"),
                        ("rotate", &mut binds.rotate, "cam.rot"),
                    ] {
                        ui.menu_button(
                            match slot
                                .and_then(|id| params.iter().find(|(pid, _)| *pid == id))
                            {
                                Some((_, name)) => format!("{label}: {name}"),
                                None => format!("{label}: static"),
                            },
                            |ui| {
                                if ui.selectable_label(slot.is_none(), "static").clicked() {
                                    *slot = None;
                                    changed = true;
                                    ui.close();
                                }
                                for (pid, pname) in &params {
                                    if ui
                                        .selectable_label(*slot == Some(*pid), pname)
                                        .clicked()
                                    {
                                        *slot = Some(*pid);
                                        changed = true;
                                        ui.close();
                                    }
                                }
                                if ui
                                    .button(format!("+ new column '{fresh}'"))
                                    .on_hover_text(
                                        "add a parameter column to the X-sheet and bind it",
                                    )
                                    .clicked()
                                {
                                    if let Some(pid) = state.add_param_column(fresh) {
                                        *slot = Some(pid);
                                        changed = true;
                                    }
                                    ui.close();
                                }
                            },
                        );
                    }
                });
            }
            NodeKind::ImageSource { image } => {
                match state.cut().image(*image) {
                    Some(img) => {
                        ui.label(format!("{} — {}×{}", img.name, img.width, img.height));
                    }
                    None => {
                        ui.label(
                            egui::RichText::new("missing image (renders transparent)")
                                .color(egui::Color32::from_rgb(235, 160, 90)),
                        );
                    }
                }
            }
            NodeKind::Blend { mode } => {
                for m in [
                    BlendMode::Normal,
                    BlendMode::Multiply,
                    BlendMode::Add,
                    BlendMode::Screen,
                ] {
                    if ui
                        .selectable_label(*mode == m, format!("{m}"))
                        .clicked()
                    {
                        *mode = m;
                        changed = true;
                    }
                }
            }
            NodeKind::DrawingSource { column } => {
                let columns: Vec<(anim_core::ids::ColumnId, String)> = state
                    .cut()
                    .xsheet
                    .columns
                    .iter()
                    .map(|c| (c.id, c.name.clone()))
                    .collect();
                for (cid, cname) in columns {
                    if ui
                        .selectable_label(*column == cid, format!("col {cname}"))
                        .clicked()
                    {
                        *column = cid;
                        changed = true;
                    }
                }
            }
            NodeKind::Output => {
                ui.label(egui::RichText::new("the cut's final image").weak());
            }
        }
        if changed {
            state.graph_set_kind(id, kind);
        }
    });
}
