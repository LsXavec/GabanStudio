//! The Node Graph pane: a live view + editor over the cut's REAL compositing
//! graph (in the engine since M1 — persisted, undoable, cache-evaluated).
//! Nodes drag; wires connect output→input pins; every edit is an engine
//! command. Honest note: the canvas still renders through the sandwich
//! compositor — the graph drives evaluation/caching today and becomes the
//! render path when the GPU graph compositor lands.

use anim_core::fx::FxKind;
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

/// GM9: the pane's session-only view state (positions themselves live in
/// the cut's `graph.layout`).
#[derive(Default)]
pub struct GraphUi {
    /// The searchable add-node menu, while open.
    pub add_menu: Option<AddMenu>,
    /// A node drag in progress: shown live, committed once on release.
    pub drag: Option<NodeDrag>,
    /// Each node's screen rect this frame.
    pub node_rects: Vec<(NodeId, Rect)>,
    /// The Effect inspector's gesture in progress.
    pub fx: crate::fxui::FxEdit,
}

pub struct AddMenu {
    /// Where it opened: the screen point, and the graph point a new node
    /// takes (its top-left corner at the pointer).
    pub screen: Pos2,
    pub world: (f32, f32),
    pub query: String,
    pub sel: usize,
    /// The field has not taken focus yet.
    fresh: bool,
    /// The menu's rect last frame: a press outside it closes the menu.
    rect: Option<Rect>,
}

pub struct NodeDrag {
    pub node: NodeId,
    /// Where the node is shown now (graph space).
    pub pos: (f32, f32),
    start: (f32, f32),
    press: Pos2,
}

/// What an add-menu entry makes.
enum Make {
    Node(NodeKind),
    /// "Image (PNG)…" asks for the file first.
    Image,
}

/// One entry of the add-node menu.
struct Entry {
    category: &'static str,
    name: String,
    aliases: &'static [&'static str],
    help: &'static str,
    make: Make,
}

/// Every node the add menu offers, in category order: Source, Transform,
/// Blend, FX, Output (later parts add Generators and 3D).
fn entries(state: &AppState) -> Vec<Entry> {
    let mut v: Vec<Entry> = state
        .cut()
        .xsheet
        .columns
        .iter()
        .map(|c| Entry {
            category: "Source",
            name: format!("Drawing: column {}", c.name),
            aliases: &["drawingsource", "cel", "column"],
            help: "the column's drawing at each frame",
            make: Make::Node(NodeKind::DrawingSource { column: c.id }),
        })
        .collect();
    v.push(Entry {
        category: "Source",
        name: "Solid".into(),
        aliases: &["colour", "color", "fill", "constant"],
        help: "a constant colour layer",
        make: Make::Node(NodeKind::Solid {
            rgba: [255, 255, 255, 255],
        }),
    });
    v.push(Entry {
        category: "Source",
        name: "Image (PNG)…".into(),
        aliases: &["imagesource", "png", "background", "plate", "reference"],
        help: "import a background plate / reference image into this cut — \
            stored inside the project file, wired as an ImageSource node",
        make: Make::Image,
    });
    v.push(Entry {
        category: "Transform",
        name: "Transform".into(),
        aliases: &["move", "scale", "rotate", "camera"],
        help: "moves, scales and rotates its input",
        make: Make::Node(NodeKind::Transform {
            translate: (0.0, 0.0),
            scale: 1.0,
            rotate_deg: 0.0,
            binds: Default::default(),
        }),
    });
    v.push(Entry {
        category: "Blend",
        name: "Blend".into(),
        aliases: &["composite", "over", "merge"],
        help: "composites input 1 over input 0",
        make: Make::Node(NodeKind::Blend {
            mode: BlendMode::Normal,
        }),
    });
    for &fx in FxKind::ALL {
        v.push(Entry {
            category: "FX",
            name: fx.name().into(),
            aliases: fx.aliases(),
            help: fx.help(),
            make: Make::Node(NodeKind::effect(fx)),
        });
    }
    v.push(Entry {
        category: "Output",
        name: "Output".into(),
        aliases: &["result", "final"],
        help: "the cut's final image",
        make: Make::Node(NodeKind::Output),
    });
    v
}

/// The entries matching `query`, best first: every word must appear in
/// the name, the category or an alias; a name starting with the query
/// ranks above a name containing it, above an alias or category match.
fn ranked(entries: Vec<Entry>, query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let mut rows: Vec<(u8, usize, Entry)> = entries
        .into_iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let name = e.name.to_lowercase();
            let hay = format!("{name} {} {}", e.category.to_lowercase(), e.aliases.join(" "));
            if !q.split_whitespace().all(|w| hay.contains(w)) {
                return None;
            }
            let rank = if name.starts_with(&q) {
                0
            } else if name.contains(&q) {
                1
            } else {
                2
            };
            Some((rank, i, e))
        })
        .collect();
    rows.sort_by_key(|r| (r.0, r.1));
    rows.into_iter().map(|r| r.2).collect()
}

/// GM9: the add-node menu at the pointer. Typing filters; the arrows move;
/// Enter or a click adds the node where the menu opened, in one undo step;
/// Esc or a press outside closes it.
fn add_menu_ui(ui: &mut egui::Ui, state: &mut AppState) {
    let Some(mut menu) = state.graph_ui.add_menu.take() else {
        return;
    };
    let ctx = ui.ctx().clone();
    // The list's own keys, taken before the field can see them.
    let none = egui::Modifiers::NONE;
    let (up, down, enter, esc) = ctx.input_mut(|i| {
        (
            i.consume_key(none, egui::Key::ArrowUp),
            i.consume_key(none, egui::Key::ArrowDown),
            i.consume_key(none, egui::Key::Enter),
            i.consume_key(none, egui::Key::Escape),
        )
    });
    let outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i
                .pointer
                .interact_pos()
                .is_some_and(|p| menu.rect.is_some_and(|r| !r.contains(p)))
    });
    if esc || outside {
        return;
    }
    let rows = ranked(entries(state), &menu.query);
    let last = rows.len().saturating_sub(1);
    if up {
        menu.sel = menu.sel.saturating_sub(1);
    }
    if down {
        menu.sel += 1;
    }
    menu.sel = menu.sel.min(last);
    let mut pick = if enter && !rows.is_empty() {
        Some(menu.sel)
    } else {
        None
    };
    let mut build = false;
    let area = egui::Area::new(ui.id().with("add node menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(menu.screen)
        .show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(260.0);
                crate::plate::legend(ui, "add node");
                let field = ui.add(
                    egui::TextEdit::singleline(&mut menu.query)
                        .hint_text("type a node — name, category or alias")
                        .desired_width(f32::INFINITY),
                );
                if menu.fresh || !field.has_focus() {
                    field.request_focus();
                    menu.fresh = false;
                }
                if field.changed() {
                    menu.sel = 0;
                }
                ui.separator();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    if rows.is_empty() {
                        ui.label(egui::RichText::new("no node matches").color(crate::plate::legend_dim()));
                    }
                    for (i, e) in rows.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let row = crate::plate::detent(ui, i == menu.sel, &e.name).on_hover_text(e.help);
                            if i == menu.sel && (up || down) {
                                row.scroll_to_me(None);
                            }
                            if row.clicked() {
                                pick = Some(i);
                            }
                            ui.label(egui::RichText::new(e.category).color(crate::plate::legend_dim()));
                        });
                    }
                });
                // GM8: the whole graph from the sheet, where graphs are made
                // (a graph shaped by hand is asked about first, DANGER).
                ui.separator();
                let action = crate::config::Action::BuildComposite;
                let can = !state.cut().render_order().is_empty();
                let resp = ui.add_enabled(can, egui::Button::new(action.label()));
                let resp = if can {
                    resp.on_hover_text(action.help())
                } else {
                    resp.on_disabled_hover_text("no column reaches the composite")
                };
                build = resp.clicked();
            });
        });
    menu.rect = Some(area.response.rect);
    if let Some(e) = pick.and_then(|i| rows.into_iter().nth(i)) {
        match e.make {
            Make::Node(kind) => state.graph_add_node(kind, menu.world),
            Make::Image => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG image", &["png"])
                    .pick_file()
                {
                    state.graph_import_image(&path, menu.world);
                }
            }
        }
        return;
    }
    if build {
        state.request_auto_comp();
        return;
    }
    state.graph_ui.add_menu = Some(menu);
}

fn kind_color(kind: &NodeKind) -> Color32 {
    match kind {
        NodeKind::DrawingSource { .. } => Color32::from_rgb(83, 137, 196),
        NodeKind::Solid { .. } => Color32::from_rgb(139, 141, 131),
        NodeKind::ImageSource { .. } => Color32::from_rgb(228, 225, 214),
        // AUDIT [28]: these two hues are in no ratified palette. Node
        // identity comes from tokens now — Legend for an operation, Ao
        // for the one that composites (continuity), as elsewhere.
        NodeKind::Transform { .. } | NodeKind::Effect { .. } => crate::plate::LEGEND,
        NodeKind::Blend { .. } => crate::plate::AO,
        // AUDIT [17]: this was Aka, PAINTED AS A TITLE FILL — Aka
        // means refusal or destruction, and the graph's result is
        // neither. Its identity is carried by its Struck edge instead.
        NodeKind::Output => Color32::from_rgb(228, 225, 214),
    }
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    state.ensure_node_layout();
    inspector_row(ui, state);
    ui.separator();
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, Color32::from_rgb(14, 15, 13));

    // Pan with a background drag.
    if response.dragged() && state.pending_wire.is_none() {
        state.graph_pan.0 += response.drag_delta().x;
        state.graph_pan.1 += response.drag_delta().y;
    }
    let origin = rect.left_top() + vec2(state.graph_pan.0, state.graph_pan.1);
    let to_screen = |p: (f32, f32)| pos2(origin.x + p.0, origin.y + p.1);

    // GM9: Tab with the pointer over the graph, or a right-click on empty
    // graph space, opens the add-node menu there (never while typing).
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));
    let tab = state.graph_ui.add_menu.is_none()
        && hover.is_some()
        && !ui.ctx().text_edit_focused()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
    let open_at = if response.secondary_clicked() {
        response.interact_pointer_pos()
    } else if tab {
        hover
    } else {
        None
    };
    // A node drag the pane never saw end (it closed mid-drag) is dropped.
    if state.graph_ui.drag.is_some()
        && !ui.input(|i| i.pointer.any_down() || i.pointer.any_released())
    {
        state.graph_ui.drag = None;
    }
    if let Some(p) = open_at {
        state.graph_ui.add_menu = Some(AddMenu {
            screen: p,
            world: (p.x - origin.x, p.y - origin.y),
            query: String::new(),
            sel: 0,
            fresh: true,
            rect: None,
        });
    }
    state.graph_ui.node_rects.clear();

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
            let pos = state.node_pos(n.id);
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
    let out_pin_pos =
        |nv: &NodeView| -> Pos2 { pos2(nv.rect.right(), nv.rect.top() + TITLE_H * 0.5) };
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
                    Color32::from_rgb(139, 141, 131),
                );
            }
        }
    }
    // Pending wire follows the pointer.
    if let Some(from) = state.pending_wire
        && let Some(fv) = find(from)
        && let Some(ptr) = ui.input(|i| i.pointer.latest_pos())
    {
        cable(
            &painter,
            out_pin_pos(fv),
            ptr,
            Color32::from_rgb(83, 137, 196),
        );
    }

    // Nodes.
    let mut wire_drop: Option<(NodeId, u16)> = None; // input pin under a released wire
    let released = ui.input(|i| i.pointer.any_released());
    for nv in &nodes {
        let selected = state.selected_node == Some(nv.id);
        let is_output = output_node == Some(nv.id);
        let body = Color32::from_rgb(26, 27, 24);
        // AUDIT [39]: square, like every other surface in the app.
        painter.rect_filled(nv.rect, 0, body);
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
                // Selection is TALLY — the one colour meaning current/armed
                // (Ao is never a selection; the colour law). The output
                // node's identity edge is Struck, not Aka — the result is
                // not a correction.
                if selected {
                    Color32::from_rgb(239, 194, 74)
                } else if is_output {
                    Color32::from_rgb(228, 225, 214)
                } else {
                    Color32::from_rgb(100, 102, 94)
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
            Color32::from_rgb(228, 225, 214),
        );

        // Body drag = move (GM9: shown live, saved once on release — one
        // undo step); click = select; right-click = node menu.
        state.graph_ui.node_rects.push((nv.id, nv.rect));
        let body_resp = ui.interact(
            nv.rect,
            ui.id().with(("node", nv.id.0)),
            Sense::click_and_drag(),
        );
        if body_resp.drag_started() {
            let start = state.node_pos(nv.id);
            let press = ui
                .input(|i| i.pointer.press_origin())
                .unwrap_or(nv.rect.left_top());
            state.graph_ui.drag = Some(NodeDrag {
                node: nv.id,
                pos: start,
                start,
                press,
            });
        }
        if body_resp.dragged()
            && let Some(d) = state.graph_ui.drag.as_mut()
            && d.node == nv.id
            && let Some(p) = ui.input(|i| i.pointer.interact_pos())
        {
            d.pos = (d.start.0 + p.x - d.press.x, d.start.1 + p.y - d.press.y);
        }
        if body_resp.drag_stopped()
            && let Some(d) = state.graph_ui.drag.take()
            && d.node == nv.id
            && d.pos != d.start
        {
            state.graph_move_node(d.node, d.pos);
        }
        if body_resp.clicked() {
            state.selected_node = Some(nv.id);
        }
        body_resp.context_menu(|ui| {
            if ui.button("set as output ▶").clicked() {
                state.graph_set_output(nv.id);
                ui.close();
            }
            // AUDIT [35]: destruction is found by feel — a held guard —
            // before it is read, even when undoable.
            if crate::plate::danger(ui, "REMOVE NODE") {
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
                Color32::from_rgb(83, 137, 196)
            } else {
                Color32::from_rgb(139, 141, 131)
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
                    Color32::from_rgb(83, 137, 196)
                } else if nv.inputs[pin].is_some() {
                    Color32::from_rgb(228, 225, 214)
                } else {
                    Color32::from_rgb(100, 102, 94)
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

    // GM9: the searchable add-node menu (opened above).
    add_menu_ui(ui, state);

    // Hint when empty.
    if nodes.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Tab or right-click to add nodes — the cut's compositing graph lives here",
            egui::FontId::proportional(13.0),
            Color32::from_rgb(139, 141, 131),
        );
    }
}

/// Selected node's parameters, edited through SetNodeKind (undoable).
fn inspector_row(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal_wrapped(|ui| {
        let Some(id) = state.selected_node else {
            ui.label(
                egui::RichText::new("no node selected — click one; right-click adds/removes")
                    .color(crate::plate::legend_dim()),
            );
            return;
        };
        let Ok(node) = state.cut().graph.node(id) else {
            state.selected_node = None;
            return;
        };
        let mut kind = node.kind.clone();
        let mut changed = false;
        ui.label(
            egui::RichText::new(kind.name())
                .strong()
                .color(kind_color(&kind)),
        );
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
                            match slot.and_then(|id| params.iter().find(|(pid, _)| *pid == id)) {
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
                                    if ui.selectable_label(*slot == Some(*pid), pname).clicked() {
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
            NodeKind::ImageSource { image } => match state.cut().image(*image) {
                Some(img) => {
                    ui.label(format!("{} — {}×{}", img.name, img.width, img.height));
                }
                None => {
                    ui.label(
                        egui::RichText::new("missing image (renders transparent)")
                            .color(egui::Color32::from_rgb(228, 82, 47)),
                    );
                }
            },
            NodeKind::Blend { mode } => {
                for &m in BlendMode::ALL {
                    if ui.selectable_label(*mode == m, format!("{m}")).clicked() {
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
                ui.label(egui::RichText::new("the cut's final image").color(crate::plate::legend_dim()));
            }
            // GM9: generated from the kind's specs; it commits its own
            // gestures, one undo step each.
            NodeKind::Effect { fx, params, binds } => {
                crate::fxui::inspector(ui, state, id, *fx, params, binds);
            }
        }
        if changed {
            state.graph_set_kind(id, kind);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Action;
    use crate::testkit::EditorHarness;
    use crate::workspace::Pane;
    use anim_core::fx::FxValue;

    /// A harness showing only the Node Graph pane.
    fn graph_pane() -> EditorHarness {
        let mut h = EditorHarness::new();
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::NodeGraph]);
        h.run();
        h.run();
        h
    }

    fn node_rect(h: &EditorHarness, id: NodeId) -> Rect {
        let rects = &h.state().graph_ui.node_rects;
        rects
            .iter()
            .find(|(n, _)| *n == id)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("node {id:?} not drawn\n{}", h.describe()))
    }

    #[test]
    fn fxnode_add_menu_search_ranks_names_then_aliases() {
        let h = graph_pane();
        let names = |q: &str| -> Vec<String> {
            ranked(entries(h.state()), q)
                .into_iter()
                .map(|e| e.name)
                .collect()
        };
        assert_eq!(names("glo").first().map(String::as_str), Some("Glow"));
        assert_eq!(names("bokashi"), ["Blur"], "a trade word finds its kind");
        assert_eq!(names("fx"), ["Blur", "Glow", "Diffusion", "Grade"]);
        assert!(names("no such node").is_empty());
        // Empty: everything, in category order.
        let cats: Vec<&str> = ranked(entries(h.state()), "")
            .iter()
            .map(|e| e.category)
            .collect();
        let mut order = cats.clone();
        order.dedup();
        assert_eq!(order, ["Source", "Transform", "Blend", "FX", "Output"]);
    }

    #[test]
    fn fxnode_typing_glo_and_enter_adds_a_glow_in_one_undo_step() {
        let mut h = graph_pane();
        let at = pos2(700.0, 520.0);
        h.point_at(at);
        h.key(egui::Key::Tab);
        assert!(h.state().graph_ui.add_menu.is_some(), "Tab opens the menu\n{}", h.describe());
        assert!(h.typing(), "its field has the keys");
        h.type_text("glo");
        let depth = h.state().engine.history_len();
        let count = h.state().cut().graph.nodes().count();
        h.key(egui::Key::Enter);
        let s = h.state();
        assert!(s.graph_ui.add_menu.is_none(), "Enter adds and closes");
        assert_eq!(s.engine.history_len(), depth + 1, "one undo step");
        assert_eq!(s.cut().graph.nodes().count(), count + 1);
        let id = s.selected_node.expect("the new node is selected");
        assert!(matches!(
            s.cut().graph.node(id).unwrap().kind,
            NodeKind::Effect { fx: FxKind::Glow, .. }
        ));
        let r = node_rect(&h, id);
        assert!((r.left_top() - at).length() < 0.01, "placed at the pointer: {r:?}");
        // Undo takes the node and its place together.
        h.perform(Action::Undo);
        assert_eq!(h.state().cut().graph.nodes().count(), count);
        assert!(!h.state().graph_layout().contains_key(&id));
    }

    #[test]
    fn fxnode_one_drag_on_a_float_parameter_is_one_history_entry() {
        let mut h = graph_pane();
        h.state_mut()
            .graph_add_node(NodeKind::effect(FxKind::Glow), (300.0, 300.0));
        let id = h.state().selected_node.unwrap();
        h.run();
        let field = |h: &EditorHarness, key: &str| {
            let fields = &h.state().graph_ui.fx.fields;
            fields
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("no '{key}' field\n{}", h.describe()))
        };
        let r = field(&h, "radius");
        let depth = h.state().engine.history_len();
        h.drag(r.center(), r.center() + vec2(60.0, 0.0), 6);
        let s = h.state();
        assert_eq!(s.engine.history_len(), depth + 1, "one drag, one entry");
        let NodeKind::Effect { params, .. } = &s.cut().graph.node(id).unwrap().kind else {
            panic!("still an Effect");
        };
        let spec = FxKind::Glow.spec("radius").unwrap();
        let FxValue::Float(v) = spec.read(params.get("radius")) else {
            panic!("radius is a number");
        };
        assert!(v > 16.0, "the drag raised it from its default: {v}");
        // Undo puts it back in one step.
        h.perform(Action::Undo);
        let NodeKind::Effect { params, .. } = &h.state().cut().graph.node(id).unwrap().kind else {
            panic!("still an Effect");
        };
        assert_eq!(spec.read(params.get("radius")), FxValue::Float(16.0));
    }

    #[test]
    fn fxnode_node_drag_is_one_entry_and_positions_survive_save_and_load() {
        let mut h = graph_pane();
        h.state_mut()
            .graph_add_node(NodeKind::effect(FxKind::Blur), (300.0, 300.0));
        let id = h.state().selected_node.unwrap();
        h.run();
        let r0 = node_rect(&h, id);
        let depth = h.state().engine.history_len();
        let grab = r0.left_top() + vec2(40.0, TITLE_H * 0.5);
        h.drag(grab, grab + vec2(120.0, 40.0), 8);
        assert_eq!(h.state().engine.history_len(), depth + 1, "one drag, one entry");
        assert_eq!(h.state().graph_layout().get(&id), Some(&(420.0, 340.0)));
        let r1 = node_rect(&h, id);
        assert!((r1.left_top() - (r0.left_top() + vec2(120.0, 40.0))).length() < 0.01);
        // Saved with the cut, every node's place; back after a load.
        let file = h.data_dir().join("layout.gaban");
        let s = h.state_mut();
        s.begin_save(file.clone());
        assert!(s.wait_save(), "the save landed");
        let saved = h.state().graph_layout();
        assert_eq!(saved.len(), h.state().cut().graph.nodes().count());
        let back = AppState::load_from(file).expect("the file opens");
        assert_eq!(back.graph_layout(), saved);
        assert_eq!(back.node_pos(id), (420.0, 340.0));
    }
}
