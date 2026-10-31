//! The Node Graph pane: a live view + editor over the cut's REAL compositing
//! graph (in the engine since M1 — persisted, undoable, cache-evaluated).
//! Nodes drag; wires connect output→input pins; every edit is an engine
//! command. Honest note: the canvas still renders through the sandwich
//! compositor — the graph drives evaluation/caching today and becomes the
//! render path when the GPU graph compositor lands.

use anim_core::fx::FxKind;
use anim_core::generator::{GenKind, Preset};
use anim_core::graph::{BlendMode, NodeKind};
use anim_core::ids::NodeId;
use anim_core::model::LayerRole;
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
/// the cut's `graph.layout.<id>` keys).
#[derive(Default)]
pub struct GraphUi {
    /// The searchable add-node menu, while open.
    pub add_menu: Option<AddMenu>,
    /// The add-node verb ran (Tab, the menus, the palette): the pane opens
    /// the menu on its next frame.
    ask_menu: bool,
    /// A node drag in progress: shown live, committed once on release.
    pub drag: Option<NodeDrag>,
    /// Each node's screen rect this frame.
    pub node_rects: Vec<(NodeId, Rect)>,
    /// The Effect inspector's gesture in progress.
    pub fx: crate::fxui::FxEdit,
    /// GM13: "load .cube…" was clicked for this node's file parameter —
    /// the editor hands it to the shell, which opens the file picker.
    pub lut_pick: Option<(NodeId, &'static str)>,
    /// GM17: an ImageSequence's "pick a frame…" was clicked — the shell's
    /// picker names the run from the file picked.
    pub sequence_pick: Option<NodeId>,
}

impl GraphUi {
    /// `Action::AddNode`: open the add-node menu at the pointer when it is
    /// over the graph, else in the graph's middle.
    pub fn ask_add_menu(&mut self) {
        self.ask_menu = true;
    }
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
    /// "Image (PNG, JPEG, TIFF)…" asks for the file first.
    Image,
    /// GM17: "3D model from file…" asks for the glTF or OBJ first.
    Model,
}

/// One entry of the add-node menu.
struct Entry {
    category: &'static str,
    name: String,
    /// The studio's own word (romaji), shown beside the name (D7 labels).
    term: Option<&'static str>,
    aliases: &'static [&'static str],
    help: &'static str,
    make: Make,
}

/// GM10: the Camera node's help (add menu and title tooltip).
const CAMERA_HELP: &str = "the shooting camera over the paper: pans, zooms (T.U./T.B.), \
    rotates and shakes; each plane input is a multiplane level whose depth sets how far \
    a pan moves it and how far out of focus it sits";

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
            term: None,
            aliases: &["drawingsource", "cel", "column"],
            help: "the column's drawing at each frame",
            make: Make::Node(NodeKind::source(c.id)),
        })
        .collect();
    v.push(Entry {
        category: "Source",
        name: "Solid".into(),
        term: None,
        aliases: &["colour", "color", "fill", "constant"],
        help: "a constant colour layer",
        make: Make::Node(NodeKind::Solid {
            rgba: [255, 255, 255, 255],
        }),
    });
    v.push(Entry {
        category: "Source",
        name: "Image (PNG, JPEG, TIFF)…".into(),
        term: None,
        aliases: &["imagesource", "png", "jpg", "jpeg", "tif", "tiff", "background", "plate", "reference"],
        help: "import a background plate / reference image into this cut — \
            stored inside the project file, wired as an ImageSource node",
        make: Make::Image,
    });
    // GM17: the frames another app rendered, read from disk at render.
    v.push(Entry {
        category: "Source",
        name: "Image sequence".into(),
        term: Some("renban"),
        aliases: &[
            "imagesequence",
            "sequence",
            "renban (sequence)",
            "ae",
            "blender render",
            "render",
            "frames",
            "exr",
            "shot",
        ],
        help: "frames another app rendered, one file per frame (renders/shot_####.png)",
        make: Make::Node(NodeKind::image_sequence("")),
    });
    v.push(Entry {
        category: "Transform",
        name: "Transform".into(),
        term: None,
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
        category: "Transform",
        name: "Camera".into(),
        term: Some("satsuei kamera"),
        aliases: &[
            "kamera",
            "satsuei",
            "multiplane",
            "pan",
            "zoom",
            "t.u.",
            "t.b.",
            "truck",
            "shake",
            "parallax",
            "depth of field",
            "dof",
            "paper",
        ],
        help: CAMERA_HELP,
        make: Make::Node(NodeKind::camera()),
    });
    v.push(Entry {
        category: "Blend",
        name: "Blend".into(),
        term: None,
        aliases: &["composite", "over", "merge"],
        help: "composites its 'over' input on top of its 'under' input",
        make: Make::Node(NodeKind::Blend {
            mode: BlendMode::Normal,
        }),
    });
    for &fx in FxKind::CORE {
        v.push(Entry {
            category: "FX",
            name: fx.name().into(),
            term: fx.term(),
            aliases: fx.aliases(),
            help: fx.help(),
            make: Make::Node(NodeKind::effect(fx)),
        });
    }
    v.push(Entry {
        category: "Output",
        name: "Output".into(),
        term: None,
        aliases: &["result", "final"],
        help: "the cut's final image",
        make: Make::Node(NodeKind::Output),
    });
    v
}

/// GM13: what the add menu lists — [`entries`] with the satsuei look's
/// kinds as their own Look category before the Output; GM16: then the
/// Generators, with a Particles entry per preset.
fn menu_entries(state: &AppState) -> Vec<Entry> {
    let mut v = entries(state);
    let out = v.pop();
    for &fx in FxKind::LOOK {
        v.push(Entry {
            category: "Look",
            name: fx.name().into(),
            term: fx.term(),
            aliases: fx.aliases(),
            help: fx.help(),
            make: Make::Node(NodeKind::effect(fx)),
        });
    }
    // GM18: the screen tone, its own Manga category.
    for &fx in FxKind::MANGA {
        v.push(Entry {
            category: "Manga",
            name: fx.name().into(),
            term: fx.term(),
            aliases: fx.aliases(),
            help: fx.help(),
            make: Make::Node(NodeKind::effect(fx)),
        });
    }
    for &generator in GenKind::ALL {
        v.push(Entry {
            category: "Generators",
            name: generator.name().into(),
            term: generator.term(),
            aliases: generator.aliases(),
            help: generator.help(),
            make: Make::Node(NodeKind::generator(generator)),
        });
    }
    for &preset in Preset::ALL {
        v.push(Entry {
            category: "Generators",
            name: format!("Particles: {}", preset.name()),
            term: Some(preset.term()),
            aliases: preset.aliases(),
            help: GenKind::Particles.help(),
            make: Make::Node(NodeKind::particles(preset)),
        });
    }
    // GM17: a node per model the cut holds, and a model from a file (the
    // frames another app rendered are a Source).
    for (id, m) in &state.cut().models {
        v.push(Entry {
            category: "3D",
            name: format!("3D model: {}", m.name),
            term: None,
            aliases: &["model3d", "3d", "layout", "gltf", "obj"],
            help: "the cut's embedded model, shot by its own camera in the toon look",
            make: Make::Node(NodeKind::model3d(*id)),
        });
    }
    v.push(Entry {
        category: "3D",
        name: "3D model from file…".into(),
        term: None,
        aliases: &["model3d", "3d", "import", "gltf", "glb", "obj", "blender"],
        help: "embeds a glTF or OBJ model in the cut and adds its Model3D node",
        make: Make::Model,
    });
    v.extend(out);
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
    let rows = ranked(menu_entries(state), &menu.query);
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
                let scroll = egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
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
                            // GM9: the romaji term beside the English (D7).
                            if let Some(t) = e.term {
                                ui.label(egui::RichText::new(format!("· {t}")).color(crate::plate::legend_dim()));
                            }
                            ui.label(egui::RichText::new(e.category).color(crate::plate::legend_dim()));
                        });
                    }
                });
                crate::plate::note_viewport(ui, scroll.inner_rect, [false, true]);
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
                // GM17: a JPEG or TIFF is stored as the PNG it decodes to.
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Image (PNG, JPEG, TIFF)", &["png", "jpg", "jpeg", "tif", "tiff"])
                    .pick_file()
                {
                    state.graph_import_image(&path, menu.world);
                }
            }
            Make::Model => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("3D model (glTF, OBJ)", anim_core::r3d::FORMATS)
                    .pick_file()
                {
                    state.import_model(&path, menu.world);
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

/// GM9: input pin `pin`'s name (painted beside it) and what it takes (its
/// tooltip). An Effect's come from its FX kind, so a later kind's pins are
/// named where it is declared.
fn pin_label(kind: &NodeKind, pin: usize) -> (&'static str, &'static str) {
    let (names, takes): (&[&str], &[&str]) = match kind {
        NodeKind::Blend { .. } => (
            &["under", "over"],
            &["the layer underneath", "the layer composited on top of it"],
        ),
        NodeKind::Effect { fx, .. } => (fx.input_names(), fx.input_help()),
        NodeKind::Generator { generator, .. } => (generator.input_names(), generator.input_help()),
        // GM10: one pin per plane, named for its place in the stack.
        NodeKind::Camera { .. } => {
            let name = anim_core::camera::PLANE_NAMES.get(pin).copied().unwrap_or("plane");
            return (name, anim_core::camera::PLANE_HELP);
        }
        NodeKind::Transform { .. } => (&["image"], &["the picture it moves, scales and rotates"]),
        NodeKind::Output => (&["image"], &["the cut's final picture"]),
        NodeKind::DrawingSource { .. }
        | NodeKind::Solid { .. }
        | NodeKind::ImageSource { .. }
        | NodeKind::Model3D { .. }
        | NodeKind::ImageSequence { .. } => (&[], &[]),
    };
    (
        names.get(pin).copied().unwrap_or("input"),
        takes.get(pin).copied().unwrap_or("an image"),
    )
}

fn kind_color(kind: &NodeKind) -> Color32 {
    match kind {
        NodeKind::DrawingSource { .. } => crate::plate::BLUE,
        NodeKind::Solid { .. } => crate::plate::LEGEND,
        NodeKind::ImageSource { .. } => crate::plate::STRUCK,
        // AUDIT [28]: these two hues are in no ratified palette. Node
        // identity comes from tokens now — Legend for an operation, blue
        // for the one that composites (continuity), as elsewhere.
        NodeKind::Transform { .. } | NodeKind::Effect { .. } | NodeKind::Camera { .. } => {
            crate::plate::LEGEND
        }
        NodeKind::Blend { .. } => crate::plate::BLUE,
        // GM16: a generator makes the cut's own picture, as an image does;
        // GM17: so do a 3D model and another app's frames.
        NodeKind::Generator { .. } | NodeKind::Model3D { .. } | NodeKind::ImageSequence { .. } => {
            crate::plate::STRUCK
        }
        // AUDIT [17]: this was Aka, PAINTED AS A TITLE FILL — Aka
        // means refusal or destruction, and the graph's result is
        // neither. Its identity is carried by its Struck edge instead.
        NodeKind::Output => crate::plate::STRUCK,
    }
}

/// A node's title on the graph: its kind's name, and after it a Text
/// node's first words (GM16), so lettering nodes tell apart.
fn node_title(kind: &NodeKind) -> String {
    if let NodeKind::Generator { generator: GenKind::Text, params, .. } = kind
        && let Some(anim_core::fx::FxValue::Text { text }) =
            GenKind::Text.spec("text").map(|s| s.read(params.get("text")))
    {
        // As much of the first line as the title bar holds.
        let line = text.lines().next().unwrap_or_default();
        let mut words: String = line.chars().take(7).collect();
        if words.len() < text.trim_end().len() {
            words.push('…');
        }
        return format!("{}: {words}", kind.name());
    }
    kind.name().to_string()
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState) {
    state.ensure_node_layout();
    toolbar(ui, state);
    inspector_row(ui, state);
    ui.separator();
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, crate::plate::WELL);
    // Painted names stop at `rect`, node controls at the pane's own clip.
    crate::plate::note_pan_view(ui, rect, ui.clip_rect().with_min_y(rect.top()));

    // Pan with a background drag.
    if response.dragged() && state.pending_wire.is_none() {
        state.graph_pan.0 += response.drag_delta().x;
        state.graph_pan.1 += response.drag_delta().y;
    }
    let origin = rect.left_top() + vec2(state.graph_pan.0, state.graph_pan.1);
    let to_screen = |p: (f32, f32)| pos2(origin.x + p.0, origin.y + p.1);

    // GM9: the add-node verb (Tab over the graph — never while typing —
    // or the menus and the palette) opens the add-node menu at the
    // pointer, or in the graph's middle when the pointer is elsewhere; a
    // right-click on empty graph space opens it there.
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));
    let asked = std::mem::take(&mut state.graph_ui.ask_menu);
    let open_at = if response.secondary_clicked() {
        response.interact_pointer_pos()
    } else if asked {
        Some(hover.unwrap_or_else(|| rect.center()))
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
                    crate::plate::LEGEND,
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
            crate::plate::TALLY,
        );
    }

    // Nodes.
    let mut wire_drop: Option<(NodeId, u16)> = None; // input pin under a released wire
    let released = ui.input(|i| i.pointer.any_released());
    for nv in &nodes {
        let selected = state.selected_node == Some(nv.id);
        let is_output = output_node == Some(nv.id);
        let body = crate::plate::GRAPHITE;
        painter.rect_filled(nv.rect, crate::plate::R_CONTROL, body);
        // The kind's ground: its ink at the wash's strength, so the title
        // holds on every kind.
        painter.rect_filled(
            Rect::from_min_size(nv.rect.left_top(), vec2(NODE_W, TITLE_H)),
            egui::CornerRadius { nw: crate::plate::R_CONTROL, ne: crate::plate::R_CONTROL, sw: 0, se: 0 },
            kind_color(&nv.kind).gamma_multiply(0.2),
        );
        painter.rect_stroke(
            nv.rect,
            crate::plate::R_CONTROL,
            Stroke::new(
                if selected { 2.0 } else { 1.0 },
                // Selection is TALLY — the one colour meaning current/armed
                // (Ao is never a selection; the colour law). The output
                // node's identity edge is Struck, not Aka — the result is
                // not a correction.
                if selected {
                    crate::plate::TALLY
                } else if is_output {
                    crate::plate::STRUCK
                } else {
                    crate::plate::legend_muted()
                },
            ),
            egui::StrokeKind::Outside,
        );
        painter.text(
            nv.rect.left_top() + vec2(8.0, TITLE_H * 0.5),
            egui::Align2::LEFT_CENTER,
            if is_output {
                format!("▶ {}", node_title(&nv.kind))
            } else {
                node_title(&nv.kind)
            },
            egui::FontId::proportional(12.0),
            crate::plate::STRUCK,
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
                crate::plate::TALLY
            } else {
                crate::plate::LEGEND
            },
        );

        // Input pins: wire drop target; right-click disconnects. GM9: each
        // is named inside the body, and says what it takes.
        for pin in 0..nv.inputs.len() {
            let ip = in_pin_pos(nv, pin);
            let (name, takes) = pin_label(&nv.kind, pin);
            painter.text(
                ip + vec2(PIN_R + 4.0, 0.0),
                egui::Align2::LEFT_CENTER,
                name,
                egui::FontId::proportional(10.0),
                crate::plate::legend_dim(),
            );
            let ip_rect = Rect::from_center_size(ip, vec2(16.0, 16.0));
            let ip_resp = ui
                .interact(
                    ip_rect,
                    ui.id().with(("inpin", nv.id.0, pin)),
                    Sense::click(),
                )
                .on_hover_text(format!("{name}: {takes} (right-click to disconnect)"));
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
                    crate::plate::TALLY
                } else if nv.inputs[pin].is_some() {
                    crate::plate::STRUCK
                } else {
                    crate::plate::legend_muted()
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

    // Hint when empty (GM9: the add verb by its chord in this keymap).
    if nodes.is_empty() {
        let add = crate::menus::chord_label(ui.ctx(), crate::config::Action::AddNode)
            .map(|c| format!("{c} or right-click"))
            .unwrap_or_else(|| "Right-click".into());
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{add} to add nodes — the cut's compositing graph lives here"),
            egui::FontId::proportional(13.0),
            crate::plate::LEGEND,
        );
    }
}

/// GM13: the pane's toolbar. "Build satsuei graph" (the BuildSatsuei verb:
/// its label, its help) replaces the graph with the satsuei chain in one
/// undo step; over a graph shaped by hand it has the DANGER edge and asks
/// first; without a drawing column it is disabled and its tooltip says why.
fn toolbar(ui: &mut egui::Ui, state: &mut AppState) {
    use crate::config::Action;
    ui.horizontal(|ui| {
        let why = state.satsuei_refusal();
        let hand = state.satsuei_hand_built();
        let button = ui.add_enabled(why.is_none(), egui::Button::new(Action::BuildSatsuei.label()));
        if hand.is_some() && why.is_none() {
            // DANGER's Aka edge, painted over the button: a stroke of its
            // own would grow it, and the graph below would shift.
            ui.painter().rect_stroke(
                button.rect,
                crate::plate::R_CONTROL,
                Stroke::new(1.0, crate::plate::AKA),
                egui::StrokeKind::Inside,
            );
        }
        let help = Action::BuildSatsuei.help();
        let button = match (why, hand) {
            (Some(why), _) => button.on_disabled_hover_text(why),
            (None, Some(n)) => button.on_hover_text(format!(
                "{help} It replaces this hand-built graph ({n} nodes): it asks first, and \
                 undo brings the graph back."
            )),
            (None, None) => button.on_hover_text(format!(
                "{help} The BG goes through the camera (the characters are held to the \
                 frame); one undo step, every node editable."
            )),
        };
        if button.clicked() {
            state.request_satsuei();
        }
    });
}

/// Selected node's parameters, edited through SetNodeKind (undoable).
fn inspector_row(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal_wrapped(|ui| {
        // A button's height with or without a selection: the graph under the
        // row stays put when a node's controls come in.
        let button = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
        ui.set_min_height(button);
        let Some(id) = state.selected_node else {
            // GM9: the add verb said by its chord in this keymap.
            let add = crate::menus::chord_label(ui.ctx(), crate::config::Action::AddNode)
                .map(|c| format!("{c} or right-click"))
                .unwrap_or_else(|| "right-click".into());
            ui.label(
                egui::RichText::new(format!(
                    "no node selected — click one; {add} adds, right-click a node removes it"
                ))
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
        let title = ui.label(
            egui::RichText::new(kind.name())
                .strong()
                .color(kind_color(&kind)),
        );
        if let NodeKind::Effect { fx, .. } = &kind {
            title.on_hover_text(fx.help());
        } else if let NodeKind::Camera { .. } = &kind {
            title.on_hover_text(CAMERA_HELP);
        } else if let NodeKind::Generator { generator, .. } = &kind {
            title.on_hover_text(generator.help());
        }
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
                            .color(crate::plate::AKA),
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
            NodeKind::DrawingSource {
                column,
                roles,
                absorb_trace,
            } => {
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
                // GM13: the role pass (every layer, or the ticked roles')
                // and the trace-line absorb — each click one undo step.
                let shown = match roles {
                    None => "every layer".to_string(),
                    Some(r) => r.iter().map(|x| x.label()).collect::<Vec<_>>().join(" + "),
                };
                ui.menu_button(format!("pass: {shown} ▾"), |ui| {
                    if ui.selectable_label(roles.is_none(), "every layer").clicked() {
                        *roles = None;
                        changed = true;
                    }
                    for role in LayerRole::ALL {
                        let on = roles.as_ref().is_some_and(|r| r.contains(&role));
                        if ui.selectable_label(on, role.label()).clicked() {
                            let mut r = roles.take().unwrap_or_default();
                            if on {
                                r.retain(|x| *x != role);
                            } else {
                                r.push(role);
                            }
                            *roles = (!r.is_empty()).then_some(r);
                            changed = true;
                        }
                    }
                })
                .response
                .on_hover_text("only these layer roles render: the shadow pass, the highlight pass, paint only, lines only");
                changed |= ui
                    .checkbox(absorb_trace, "iro-tore (absorb trace lines)")
                    .on_hover_text(
                        "the shadow and highlight trace lines take the paint colour under them \
                         (or the nearest within 3 px)",
                    )
                    .changed();
            }
            NodeKind::Output => {
                ui.label(egui::RichText::new("the cut's final image").color(crate::plate::legend_dim()));
            }
            // GM9: generated from the kind's specs; it commits its own
            // gestures, one undo step each.
            NodeKind::Effect { fx, params, binds } => {
                crate::fxui::inspector(ui, state, id, *fx, params, binds);
            }
            // GM10: the specs' controls, then the planes' depths.
            NodeKind::Camera {
                planes,
                params,
                binds,
            } => {
                crate::fxui::camera_inspector(ui, state, id, planes, params, binds);
            }
            // GM16: the generator's specs, as an Effect's.
            NodeKind::Generator {
                generator,
                params,
                binds,
            } => {
                crate::fxui::gen_inspector(ui, state, id, *generator, params, binds);
            }
            // GM17: the model's placement, camera and look.
            NodeKind::Model3D { model, params, binds } => {
                crate::fxui::model_inspector(ui, state, id, *model, params, binds);
            }
            // GM17: the frames another app rendered — `#` is the frame
            // number, zero-padded; relative names read from the project's
            // folder.
            NodeKind::ImageSequence {
                pattern,
                first,
                offset,
                hold_last,
                fit,
            } => {
                // The name as typed lives in a draft and commits once, when
                // the field lets go (Enter or a click away): one undo step.
                let draft = ui.id().with(("sequence pattern", id));
                let mut text = ui.data(|d| d.get_temp::<String>(draft)).unwrap_or_else(|| pattern.clone());
                let r = ui
                    .add(egui::TextEdit::singleline(&mut text).hint_text("renders/shot_####.png"))
                    .on_hover_text("the files' name, # for the frame number (zero-padded to the #s)");
                if r.changed() {
                    ui.data_mut(|d| d.insert_temp(draft, text.clone()));
                }
                if r.lost_focus() {
                    ui.data_mut(|d| d.remove::<String>(draft));
                    if text != *pattern {
                        *pattern = text;
                        changed = true;
                    }
                }
                if ui
                    .button("pick a frame…")
                    .on_hover_text(
                        "pick one file of the render: shot_0001.png names the run shot_####.png from 1, \
                         relative to the project's folder",
                    )
                    .clicked()
                {
                    state.graph_ui.sequence_pick = Some(id);
                }
                changed |= ui
                    .add(egui::DragValue::new(first).prefix("first file "))
                    .on_hover_text("the number of the file shown at the 'at frame' frame")
                    .changed();
                // The sheet counts frames from 1; the node stores from 0.
                let mut at = *offset as i64 + 1;
                if ui
                    .add(egui::DragValue::new(&mut at).prefix("at frame ").range(-99_999..=99_999))
                    .on_hover_text("the sheet frame (from 1) that shows the first file")
                    .changed()
                {
                    *offset = (at - 1) as i32;
                    changed = true;
                }
                changed |= ui
                    .checkbox(hold_last, "hold last")
                    .on_hover_text("a missing file shows the nearest earlier one (the last frame holds)")
                    .changed();
                changed |= ui
                    .checkbox(fit, "fit to frame")
                    .on_hover_text(
                        "a file of another size than the frame is scaled to fit it, centred; off, it sits \
                         at its own size from the top-left",
                    )
                    .changed();
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
    fn genui_typing_speed_and_enter_adds_speed_lines_in_one_undo_step() {
        let mut h = graph_pane();
        h.point_at(pos2(700.0, 520.0));
        h.key(egui::Key::Tab);
        assert!(h.typing(), "the menu's field has the keys\n{}", h.describe());
        h.type_text("speed");
        let depth = h.state().engine.history_len();
        let count = h.state().cut().graph.nodes().count();
        h.key(egui::Key::Enter);
        let s = h.state();
        assert!(s.graph_ui.add_menu.is_none(), "Enter adds and closes");
        assert_eq!(s.engine.history_len(), depth + 1, "one history entry");
        assert_eq!(s.cut().graph.nodes().count(), count + 1);
        let id = s.selected_node.expect("the new node is selected");
        let kind = &s.cut().graph.node(id).unwrap().kind;
        assert!(
            matches!(kind, NodeKind::Generator { generator: GenKind::SpeedLines, .. }),
            "{kind:?}"
        );
        assert!(crate::graphcomp::needs_cpu(kind), "drawn on the CPU in every view (D9)");
        // The inspector shows every parameter of the kind's specs.
        h.run();
        let keys: Vec<&str> = h.state().graph_ui.fx.fields.iter().map(|(k, _)| *k).collect();
        for spec in GenKind::SpeedLines.specs() {
            assert!(keys.contains(&spec.key), "no '{}' field in {keys:?}\n{}", spec.key, h.describe());
        }
        h.perform(Action::Undo);
        assert_eq!(h.state().cut().graph.nodes().count(), count);
    }

    #[test]
    fn genui_the_text_field_commits_once_on_a_click_away_or_ctrl_enter() {
        let mut h = graph_pane();
        // A second node to click, right of the sample cut's nodes.
        h.state_mut()
            .graph_add_node(NodeKind::generator(GenKind::Balloon), (900.0, 40.0));
        let other = h.state().selected_node.unwrap();
        h.state_mut()
            .graph_add_node(NodeKind::generator(GenKind::Text), (700.0, 40.0));
        let id = h.state().selected_node.unwrap();
        h.run();
        let words = |h: &EditorHarness| {
            let NodeKind::Generator { params, .. } = &h.state().cut().graph.node(id).unwrap().kind else {
                panic!("still a Generator");
            };
            match GenKind::Text.spec("text").unwrap().read(params.get("text")) {
                FxValue::Text { text } => text,
                v => panic!("the words are text: {v:?}"),
            }
        };
        let field = |h: &EditorHarness| {
            let fields = &h.state().graph_ui.fx.fields;
            fields
                .iter()
                .find(|(k, _)| *k == "text")
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("no 'text' field\n{}", h.describe()))
        };
        // Empty graph: under every node (and so every wire) and the inspector.
        let rects = h.state().graph_ui.node_rects.clone();
        let low = rects.iter().map(|(_, r)| r.bottom()).fold(field(&h).bottom(), f32::max);
        let empty = pos2(field(&h).left() + 40.0, low + 40.0);
        assert!(empty.y < 760.0 && !rects.iter().any(|(_, r)| r.contains(empty)), "{empty:?} {rects:?}");
        let depth = h.state().engine.history_len();
        // Into the field, everything selected, new words typed: not stored yet.
        h.click_pos(field(&h).center());
        assert!(h.typing(), "the click gives the field the keys\n{}", h.describe());
        h.key_mods(crate::testkit::CTRL, egui::Key::A);
        h.type_text("ドン");
        assert_eq!(words(&h), "セリフ", "typing alone stores nothing");
        assert_eq!(h.state().engine.history_len(), depth);
        // A click on empty graph lets go of the keys and stores the words once.
        h.click_pos(empty);
        assert!(!h.typing(), "the click away takes the keys");
        assert_eq!(h.state().engine.history_len(), depth + 1, "one history entry");
        assert_eq!(words(&h), "ドン");
        assert_eq!(h.state().selected_node, Some(id), "a background click keeps the node");
        // A bare Enter is the field's new line; Ctrl+Enter stores and lets go.
        h.click_pos(field(&h).center());
        h.key_mods(crate::testkit::CTRL, egui::Key::A);
        h.type_text("ドン");
        h.key(egui::Key::Enter);
        h.type_text("バン");
        assert_eq!(h.state().engine.history_len(), depth + 1, "Enter stores nothing");
        h.key_mods(crate::testkit::CTRL, egui::Key::Enter);
        assert!(!h.typing(), "Ctrl+Enter lets go of the keys");
        assert_eq!(h.state().engine.history_len(), depth + 2, "one history entry");
        assert_eq!(words(&h), "ドン\nバン");
        // A click on another node stores the words in the node they were
        // typed for, then selects the other.
        h.click_pos(field(&h).center());
        h.key_mods(crate::testkit::CTRL, egui::Key::A);
        h.type_text("ズキューン");
        h.click_pos(node_rect(&h, other).center());
        assert_eq!(h.state().selected_node, Some(other), "the click selects the other node");
        assert_eq!(h.state().engine.history_len(), depth + 3, "one history entry");
        assert_eq!(words(&h), "ズキューン");
        // Every edit is one undo step, and none of it touched a GPU.
        h.perform(Action::Undo);
        assert_eq!(words(&h), "ドン\nバン");
        // The graph titles the node by its first words.
        let kind = &h.state().cut().graph.node(id).unwrap().kind;
        assert_eq!(node_title(kind), "Text: ドン…");
        assert_eq!(node_title(&NodeKind::generator(GenKind::Text)), "Text: セリフ");
        assert_eq!(node_title(&NodeKind::generator(GenKind::Balloon)), "Balloon");
        assert!(h.editor().paint.is_none() && h.editor().graph.is_none(), "a GPU resource was created");
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
        // Two nodes nobody placed by hand (an older file's): they take
        // their column's default slots.
        let at = h.state().at();
        let raw: Vec<NodeId> = (0..2).map(|_| h.state_mut().engine.alloc_node_id()).collect();
        let cmds = raw
            .iter()
            .map(|&id| anim_core::command::Command::AddNode {
                at,
                id,
                kind: NodeKind::effect(FxKind::Grade),
            })
            .collect();
        h.state_mut().engine.apply("unplaced nodes", cmds).expect("added");
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
        h.state_mut().graph_move_node(raw[0], (500.0, 500.0));
        h.run();
        // Saved with the cut, each hand-placed node's place under its own
        // key; after a load those come back and every other node finds the
        // same default slot.
        let file = h.data_dir().join("layout.gaban");
        let s = h.state_mut();
        s.begin_save(file.clone());
        assert!(s.wait_save(), "the save landed");
        let saved = h.state().graph_layout();
        assert_eq!(saved.len(), 2, "the dragged Blur and the moved Grade: {saved:?}");
        let mut back = AppState::load_from(file).expect("the file opens");
        assert_eq!(back.graph_layout(), saved);
        back.ensure_node_layout();
        for n in h.state().cut().graph.nodes() {
            assert_eq!(back.node_pos(n.id), h.state().node_pos(n.id), "{:?} kept its spot", n.id);
        }
        assert_eq!(back.node_pos(id), (420.0, 340.0));
        assert_eq!(back.node_pos(raw[0]), (500.0, 500.0));
    }

    #[test]
    fn fxnode_add_node_is_a_verb_that_opens_the_menu() {
        assert!(Action::ALL.contains(&Action::AddNode), "in the registry");
        assert_eq!(Action::AddNode.label(), "Add node…");
        let mut h = graph_pane();
        assert!(h.state().graph_ui.add_menu.is_none());
        h.perform(Action::AddNode);
        h.run();
        assert!(h.state().graph_ui.add_menu.is_some(), "the verb opens the menu\n{}", h.describe());
        assert!(h.typing(), "its field has the keys");
        // Escape closes it, and the verb opens it again.
        h.key(egui::Key::Escape);
        assert!(h.state().graph_ui.add_menu.is_none(), "Escape closes\n{}", h.describe());
        h.perform(Action::AddNode);
        h.run();
        assert!(h.state().graph_ui.add_menu.is_some());
    }

    #[test]
    fn fxnode_input_pins_say_what_they_take() {
        let glow = NodeKind::effect(FxKind::Glow);
        assert_eq!(pin_label(&glow, 0).0, "image");
        assert_eq!(pin_label(&glow, 1).0, "matte");
        assert!(pin_label(&glow, 1).1.contains("opaque"), "the matte says what it does");
        let blend = NodeKind::Blend {
            mode: BlendMode::Normal,
        };
        assert_eq!([pin_label(&blend, 0).0, pin_label(&blend, 1).0], ["under", "over"]);
        // Every FX kind names each of its inputs.
        for fx in FxKind::ALL {
            assert_eq!(fx.input_names().len(), fx.inputs(), "{fx:?}");
            assert_eq!(fx.input_help().len(), fx.inputs(), "{fx:?}");
        }
    }

    // ---- GM13: the satsuei (composite) room's look --------------------------

    /// The graph under `id`, spelled: kinds, passes and pins (`-` unwired).
    fn spell(c: &anim_core::model::Cut, id: NodeId) -> String {
        let n = c.graph.node(id).unwrap();
        let name = match &n.kind {
            NodeKind::DrawingSource {
                column,
                roles,
                absorb_trace,
            } => {
                let col = c.xsheet.column(*column).unwrap().name.clone();
                match roles {
                    None => col,
                    Some(r) => format!(
                        "{col}[{}{}]",
                        r.iter().map(|r| r.key()).collect::<Vec<_>>().join(","),
                        if *absorb_trace { ";absorb" } else { "" }
                    ),
                }
            }
            NodeKind::Blend { mode } => format!("{mode}"),
            k => k.name().to_string(),
        };
        let ins: Vec<String> = n
            .inputs
            .iter()
            .map(|i| i.map_or_else(|| "-".to_string(), |(src, _)| spell(c, src)))
            .collect();
        if ins.is_empty() { name } else { format!("{name}({})", ins.join(", ")) }
    }

    /// The sample cut (BG, the ball column A, the empty column B) under its
    /// auto-comp (GM8's graph; GM19's showcase is hand-built) in the graph
    /// pane alone.
    fn sample_graph_pane() -> EditorHarness {
        let mut h = EditorHarness::with_state(crate::sample::build_auto_comp());
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::NodeGraph]);
        h.run();
        h.run();
        h
    }

    /// The sample under its auto-comp in the satsuei (composite) room, its
    /// node graph brought forward as a click on its tab would (egui_dock's
    /// tab titles are not in the tree).
    fn sample_in_the_composite_room() -> EditorHarness {
        let mut h = EditorHarness::with_state(crate::sample::build_auto_comp());
        h.perform(Action::RoomComposite);
        h.run_frames(3);
        let tab = h
            .editor()
            .dock
            .find_tab(&Pane::NodeGraph)
            .expect("the room has a node graph");
        let _ = h.editor_mut().dock.set_active_tab(tab);
        h.run_frames(2);
        h
    }

    /// A node of `kind` added to the current cut (setup, one step).
    fn add_node(s: &mut AppState, kind: NodeKind) -> NodeId {
        let at = s.at();
        let id = NodeId(s.engine.project.alloc_id());
        s.engine
            .apply("add node", vec![anim_core::command::Command::AddNode { at, id, kind }])
            .unwrap();
        id
    }

    #[test]
    fn comproom_build_satsuei_graph_is_one_undo_step_and_builds_the_template() {
        let mut h = sample_in_the_composite_room();
        let before = h.state().cut().graph.clone();
        let depth = h.state().engine.history_len();
        assert_eq!(h.node_enabled("Build satsuei graph"), Some(true), "{}", h.describe());
        h.click("Build satsuei graph");
        let s = h.state();
        assert_eq!(s.engine.history_len(), depth + 1, "one undo step");
        let c = s.cut();
        let character = |col: &str| {
            format!(
                "Normal(Screen(Multiply({col}[color,shadow,highlight,shadow_line,highlight_line;absorb], \
                 Grade(Matte({col}[shadow], -))), Glow({col}[highlight], -)), {col}[line])"
            )
        };
        let fg = format!("Normal({}, {})", character("A"), character("B"));
        let want = format!(
            "Output(Vignette(Grain(Grade(Light wrap(Para(Diffusion(Normal(Camera(BG), {fg})), -), Camera(BG), {fg})))))"
        );
        assert_eq!(spell(c, c.graph.output.unwrap()), want);
        let layout = s.graph_layout();
        assert!(c.graph.nodes().all(|n| layout.contains_key(&n.id)), "every node has its place");
        assert!(s.status.contains("satsuei graph built"), "{}", s.status);
        // Undo takes the whole graph back, nodes and places.
        h.perform(Action::Undo);
        assert_eq!(h.state().cut().graph, before, "undo restores the graph exactly");
    }

    #[test]
    fn comproom_build_satsuei_is_disabled_and_says_why_without_a_drawing_column() {
        let mut s = AppState::new_project("bare cut", 64, 36, 24, 300.0);
        let at = s.at();
        let column = s.cut().xsheet.columns[0].id;
        s.engine
            .apply("bare", vec![anim_core::command::Command::RemoveColumn { at, id: column }])
            .unwrap();
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::NodeGraph]);
        h.run();
        h.run();
        assert_eq!(h.node_enabled("Build satsuei graph"), Some(false), "{}", h.describe());
        // The registry greys the verb with the same reason (menus, palette).
        assert_eq!(h.editor().can_perform(Action::BuildSatsuei), Err(h.state().satsuei_refusal().unwrap()));
        let depth = h.state().engine.history_len();
        let graph = h.state().cut().graph.clone();
        assert!(!h.state_mut().build_satsuei(), "the verb refuses too");
        let s = h.state();
        assert_eq!(s.engine.history_len(), depth);
        assert_eq!(s.cut().graph, graph);
        assert!(s.refusal.contains("no drawing column"), "the reason is said: {}", s.refusal);
    }

    #[test]
    fn comproom_the_satsuei_look_renders_on_the_cpu_worker_and_a_lut_is_one_step() {
        let mut s = crate::sample::build_sample();
        assert!(s.build_satsuei());
        let c = s.cut();
        let out = c.graph.output.unwrap();
        let vignette = c.graph.node(out).unwrap().inputs[0].unwrap().0;
        // D9: the look's maths live once, on the CPU — its whole subtree
        // goes to the worker; the passes are CPU nodes, a plain source not.
        assert_eq!(crate::graphcomp::cpu_roots(c), vec![vignette]);
        let passes: Vec<&NodeKind> = c
            .graph
            .nodes()
            .map(|n| &n.kind)
            .filter(|k| k.is_filtered_source())
            .collect();
        assert_eq!(passes.len(), 8, "four passes for each cel column");
        assert!(passes.iter().all(|k| crate::graphcomp::needs_cpu(k)));
        let column = c.xsheet.columns[0].id;
        assert!(!crate::graphcomp::needs_cpu(&NodeKind::source(column)));
        // A LUT node: loading a file is one step that brings the file into
        // the cut; a file that is not a .cube is refused with the reason.
        let at = s.at();
        let lut = s.engine.project.next_id;
        s.engine.project.next_id += 1;
        s.engine
            .apply(
                "add node",
                vec![anim_core::command::Command::AddNode {
                    at,
                    id: NodeId(lut),
                    kind: NodeKind::effect(FxKind::Lut),
                }],
            )
            .unwrap();
        let depth = s.engine.history_len();
        assert!(!s.set_lut(NodeId(lut), "lut", "junk", b"LUT_3D_SIZE 2\n0 0 0\n".to_vec()));
        assert_eq!(s.engine.history_len(), depth, "refused: nothing changes");
        assert!(s.refusal.contains("not a 3D .cube"), "{}", s.refusal);
        let mut cube = String::from("LUT_3D_SIZE 2\n");
        for i in 0..8 {
            cube.push_str(&format!("{} {} {}\n", i & 1, (i >> 1) & 1, (i >> 2) & 1));
        }
        assert!(s.set_lut(NodeId(lut), "lut", "identity", cube.clone().into_bytes()));
        assert_eq!(s.engine.history_len(), depth + 1, "one step: the file and the node");
        assert_eq!(s.cut().blobs.get("lut.identity").map(|b| b.as_slice()), Some(cube.as_bytes()));
        let NodeKind::Effect { params, .. } = &s.cut().graph.node(NodeId(lut)).unwrap().kind else {
            panic!("still an effect");
        };
        assert_eq!(params.0.get("lut"), Some(&FxValue::Blob { blob: "lut.identity".into() }));
        s.engine.undo().unwrap();
        assert!(s.cut().blobs.is_empty(), "undo takes the file back out");
    }

    #[test]
    fn comproom_build_satsuei_over_a_hand_built_graph_asks_first() {
        let mut h = sample_graph_pane();
        // A node placed by hand: the graph is no longer an auto-comp.
        add_node(h.state_mut(), NodeKind::effect(FxKind::Blur));
        h.run();
        let n = h.state().cut().graph.nodes().count();
        assert_eq!(h.state().satsuei_hand_built(), Some(n));
        let graph = h.state().cut().graph.clone();
        let depth = h.state().engine.history_len();
        let ask = format!(
            "Replace this hand-built graph ({n} nodes) with the satsuei chain? Undo brings it back."
        );
        h.click("Build satsuei graph");
        assert!(h.has(&ask), "{}", h.describe());
        assert_eq!(h.state().engine.history_len(), depth, "asked: nothing replaced yet");
        h.click("Cancel");
        assert!(!h.has(&ask));
        assert_eq!(h.state().engine.history_len(), depth, "cancel leaves the history as it was");
        assert_eq!(h.state().cut().graph, graph);
        // The verb asks the same way; the held DANGER builds, in one step.
        h.perform(Action::BuildSatsuei);
        h.run();
        assert!(h.has(&ask), "{}", h.describe());
        h.hold("Replace");
        assert!(!h.has(&ask));
        assert_eq!(h.state().engine.history_len(), depth + 1, "{}", h.state().refusal);
        assert!(
            h.state()
                .cut()
                .graph
                .nodes()
                .any(|n| matches!(n.kind, NodeKind::Effect { fx: FxKind::Diffusion, .. })),
            "the satsuei chain"
        );
        h.perform(Action::Undo);
        assert_eq!(h.state().cut().graph, graph, "undo brings the hand-built graph back");
    }

    #[test]
    fn comproom_the_build_satsuei_verb_is_one_step_and_names_the_button() {
        let mut h = sample_graph_pane();
        // The registry's words are the toolbar button's: its label, and its
        // help on hover (with where the camera sits).
        assert!(h.has(Action::BuildSatsuei.label()), "{}", h.describe());
        h.hover(Action::BuildSatsuei.label());
        h.wait(1.0);
        assert!(h.has_text_containing(Action::BuildSatsuei.help()), "{}", h.describe());
        assert!(h.has_text_containing("The BG goes through the camera"), "{}", h.describe());
        assert_eq!(Action::BuildSatsuei.menu(), Some("Project/Build satsuei graph"));
        // A hidden column still counts: the template wires it.
        let s = h.state_mut();
        let at = s.at();
        let hide: Vec<anim_core::command::Command> = s
            .cut()
            .xsheet
            .columns
            .iter()
            .map(|c| {
                let def = s.cut().column_def(c.id);
                anim_core::command::Command::SetColumnDef {
                    at,
                    column: c.id,
                    def: anim_core::xsheet::ColumnDef {
                        kind: def.kind,
                        hidden: true,
                        locked: def.locked,
                    },
                }
            })
            .collect();
        s.engine.apply("hide", hide).unwrap();
        h.run();
        assert!(h.state().cut().render_order().is_empty(), "every column hidden");
        assert_eq!(h.editor().can_perform(Action::BuildSatsuei), Ok(()));
        // An auto-comp graph is replaced at once: one undo step.
        let depth = h.state().engine.history_len();
        assert_eq!(h.perform(Action::BuildSatsuei), crate::Performed::Done);
        h.run();
        let s = h.state();
        assert_eq!(s.engine.history_len(), depth + 1, "one undo step: {}", s.refusal);
        assert_eq!(s.engine.history().last().unwrap().label, "build satsuei graph");
        assert!(s.cut().graph.nodes().any(|n| matches!(n.kind, NodeKind::Effect { fx: FxKind::Vignette, .. })));
    }

    /// A 2×2×2 .cube whose entry at corner (r, g, b) is `f` of it.
    fn cube(f: impl Fn([u8; 3]) -> [u8; 3]) -> Vec<u8> {
        let mut text = String::from("LUT_3D_SIZE 2\n");
        for i in 0..8u8 {
            let [r, g, b] = f([i & 1, (i >> 1) & 1, (i >> 2) & 1]);
            text.push_str(&format!("{r} {g} {b}\n"));
        }
        text.into_bytes()
    }

    #[test]
    fn comproom_two_luts_of_one_name_keep_their_own_files_and_a_replaced_file_leaves() {
        use anim_core::fx::{FxCtx, FxValues, Image};
        let mut s = crate::sample::build_sample();
        let a = add_node(&mut s, NodeKind::effect(FxKind::Lut));
        let b = add_node(&mut s, NodeKind::effect(FxKind::Lut));
        let ident = cube(|c| c);
        let invert = cube(|c| c.map(|v| 1 - v));
        // The file a node holds, and what it makes of one colour.
        let file = |s: &AppState, id: NodeId| match &s.cut().graph.node(id).unwrap().kind {
            NodeKind::Effect { params, .. } => match params.get("lut") {
                Some(FxValue::Blob { blob }) => blob.clone(),
                v => panic!("not a file: {v:?}"),
            },
            k => panic!("not an effect: {k:?}"),
        };
        let look = |s: &AppState, id: NodeId| {
            let NodeKind::Effect { fx, params, binds } = &s.cut().graph.node(id).unwrap().kind else {
                panic!("not an effect");
            };
            let v = FxValues::resolve(*fx, params, binds, |_| None);
            let src = Image::from_px(1, 1, vec![0.25, 0.5, 0.75, 1.0]);
            let ctx = FxCtx {
                frame: 0,
                blobs: Some(&s.cut().blobs),
            };
            fx.apply_in(&v, anim_core::color::ColorLaw::Srgb, &[Some(&src)], 1, 1, ctx).px
        };
        let near = |got: Vec<f32>, want: [f32; 4]| {
            got.len() == 4 && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-4)
        };
        // Two files of one name: each node keeps its own.
        assert!(s.set_lut(a, "lut", "film", ident.clone()));
        assert!(s.set_lut(b, "lut", "film", invert.clone()));
        assert_eq!(file(&s, a), "lut.film");
        assert_eq!(file(&s, b), "lut.film~2");
        assert_eq!(s.cut().blobs["lut.film"].as_slice(), ident.as_slice());
        assert_eq!(s.cut().blobs["lut.film~2"].as_slice(), invert.as_slice());
        assert!(near(look(&s, a), [0.25, 0.5, 0.75, 1.0]), "{:?}", look(&s, a));
        assert!(near(look(&s, b), [0.75, 0.5, 0.25, 1.0]), "{:?}", look(&s, b));
        // A new file on `a` takes its old one out of the cut in the same
        // step; `b`'s stays.
        let depth = s.engine.history_len();
        assert!(s.set_lut(a, "lut", "warm", invert.clone()));
        assert_eq!(s.engine.history_len(), depth + 1, "one step");
        assert_eq!(file(&s, a), "lut.warm");
        let keys: Vec<&str> = s.cut().blobs.keys().map(String::as_str).collect();
        assert_eq!(keys, ["lut.film~2", "lut.warm"]);
        assert!(near(look(&s, b), [0.75, 0.5, 0.25, 1.0]));
        s.engine.undo().unwrap();
        assert_eq!(file(&s, a), "lut.film");
        assert_eq!(s.cut().blobs["lut.film"].as_slice(), ident.as_slice(), "undo brings it back");
    }

    /// The sample with the palette panel and the node graph side by side,
    /// a Palette map node selected.
    fn palette_and_graph() -> (EditorHarness, NodeId) {
        let mut s = crate::sample::build_sample();
        let id = add_node(&mut s, NodeKind::effect(FxKind::PaletteMap));
        s.selected_node = Some(id);
        let mut h = EditorHarness::with_state(s);
        let mut dock = egui_dock::DockState::new(vec![Pane::Palette]);
        dock.main_surface_mut()
            .split_right(egui_dock::NodeIndex::root(), 0.4, vec![Pane::NodeGraph]);
        h.editor_mut().dock = dock;
        h.run();
        h.run();
        (h, id)
    }

    /// Node `id`'s colour map and the variant it was taken from.
    fn map_of(
        s: &AppState,
        at: anim_core::command::CutRef,
        id: NodeId,
    ) -> (Vec<([u8; 4], [u8; 4])>, String) {
        let cut = s.engine.project.cut(at.scene, at.cut).unwrap();
        match &cut.graph.node(id).unwrap().kind {
            NodeKind::Effect { params, .. } => match params.get("map") {
                Some(FxValue::Map { map, from }) => (map.clone(), from.clone()),
                v => panic!("not a map: {v:?}"),
            },
            k => panic!("not an effect: {k:?}"),
        }
    }

    #[test]
    fn comproom_a_palette_variant_is_picked_in_the_graph_and_followed_in_one_step() {
        let (mut h, id) = palette_and_graph();
        let at = h.state().at();
        // A variant named in the palette panel ...
        assert!(!h.text_fields().is_empty(), "{}", h.describe());
        h.focus_text_field(0);
        h.type_text("night");
        assert_eq!(h.text_fields()[0], "night", "{}", h.describe());
        // (The panel's foot sits below the fold of the test window.)
        h.scroll_to("+ variant");
        h.click("+ variant");
        assert_eq!(h.state().palette_variant_names(), ["Ball/night"], "{}", h.describe());
        // ... is a choice of the Palette map picker, as palette/variant.
        h.click_combo("none");
        h.click("Ball/night");
        assert!(h.describe().contains("ComboBox = \"Ball/night\""), "{}", h.describe());
        assert_eq!(map_of(h.state(), at, id), (Vec::new(), "Ball/night".into()), "it starts as the model");
        // A variant chip changes under the picker's drag: nothing lands
        // mid-gesture ...
        let depth = h.state().engine.history_len();
        let ball = h.state().palettes.characters[0].roles[1].color;
        let blue = [30, 60, 160, 255];
        h.scroll_to("night");
        let held = h.rect("night").center();
        h.press_and_move(held, held + egui::vec2(1.0, 0.0), 2);
        h.state_mut().palettes.characters[0].variants[0].colors[1] = blue;
        h.run_frames(3);
        assert_eq!(h.state().engine.history_len(), depth, "nothing lands mid-drag");
        // ... and on release the node follows its variant, one undo step.
        h.release(held);
        let s = h.state();
        assert_eq!(s.engine.history_len(), depth + 1, "one step: {}", s.refusal);
        assert_eq!(s.engine.history().last().unwrap().label, "palette variant");
        assert_eq!(Some(map_of(s, at, id).0), s.palette_variant_map("Ball/night"));
        assert_eq!(map_of(s, at, id).0, vec![(ball, blue)]);
        // The variant removed: the node keeps its last colours, and the
        // picker says the variant is gone (in Aka).
        let depth = h.state().engine.history_len();
        h.scroll_to("×");
        h.click("×");
        assert!(h.state().palette_variant_names().is_empty());
        assert!(h.describe().contains("ComboBox = \"Ball/night (gone)\""), "{}", h.describe());
        assert_eq!(h.state().engine.history_len(), depth);
        assert_eq!(map_of(h.state(), at, id).0, vec![(ball, blue)]);
    }

    #[test]
    fn comproom_palette_edits_refresh_the_palette_maps_of_every_cut_in_one_step() {
        use anim_core::command::{Command, CutRef};
        let mut s = crate::sample::build_sample();
        s.palettes.characters[0].add_variant("night");
        let here = s.at();
        let other = CutRef {
            scene: here.scene,
            cut: s.engine.add_cut(here.scene, "C002", 24).unwrap(),
        };
        let kind = anim_core::look::effect_with(
            FxKind::PaletteMap,
            &[("map", FxValue::Map { map: Vec::new(), from: "Ball/night".into() })],
        );
        let (a, b) = (NodeId(s.engine.project.alloc_id()), NodeId(s.engine.project.alloc_id()));
        s.engine
            .apply(
                "add nodes",
                vec![
                    Command::AddNode { at: here, id: a, kind: kind.clone() },
                    Command::AddNode { at: other, id: b, kind },
                ],
            )
            .unwrap();
        let mut h = EditorHarness::with_state(s);
        h.run();
        let fresh = |h: &EditorHarness| h.state().palette_variant_map("Ball/night").unwrap();
        // The night's ball goes blue: both cuts' nodes follow, one step.
        let depth = h.state().engine.history_len();
        h.state_mut().palettes.characters[0].variants[0].colors[1] = [30, 60, 160, 255];
        h.run();
        assert_eq!(h.state().engine.history_len(), depth + 1, "every cut, one step");
        assert!(!fresh(&h).is_empty());
        for (at, id) in [(here, a), (other, b)] {
            assert_eq!(map_of(h.state(), at, id), (fresh(&h), "Ball/night".into()));
        }
        // A model chip edit is followed too (the model's chip is the key).
        let depth = h.state().engine.history_len();
        h.state_mut().palettes.characters[0].roles[1].color = [200, 30, 40, 255];
        h.run();
        assert_eq!(h.state().engine.history_len(), depth + 1);
        assert_eq!(fresh(&h)[0].0, [200, 30, 40, 255]);
        for (at, id) in [(here, a), (other, b)] {
            assert_eq!(map_of(h.state(), at, id).0, fresh(&h));
        }
        // An edit that leaves every map as it was adds no step.
        let depth = h.state().engine.history_len();
        h.state_mut().palettes.characters[0].roles[0].name = "ink".into();
        h.run();
        assert_eq!(h.state().engine.history_len(), depth);
        // A rename re-points the nodes in one step: they keep following.
        let map = fresh(&h);
        assert!(h.state_mut().rename_palette_variant(0, 0, "dusk"));
        h.run();
        assert_eq!(h.state().engine.history_len(), depth + 1);
        assert_eq!(h.state().palette_variant_names(), ["Ball/dusk"]);
        for (at, id) in [(here, a), (other, b)] {
            assert_eq!(map_of(h.state(), at, id), (map.clone(), "Ball/dusk".into()));
        }
        assert!(!h.state_mut().rename_palette_variant(0, 0, "  "), "an empty name is refused");
    }

    #[test]
    fn comproom_the_cube_picker_is_the_shells_and_the_look_adds_no_shader() {
        let mut s = crate::sample::build_sample();
        let id = add_node(&mut s, NodeKind::effect(FxKind::Lut));
        s.selected_node = Some(id);
        let mut h = EditorHarness::with_state(s);
        h.editor_mut().dock = egui_dock::DockState::new(vec![Pane::NodeGraph]);
        h.run();
        h.run();
        // "load .cube…" asks the shell for its file picker (a window): the
        // rig logs the request and never opens one.
        let depth = h.state().engine.history_len();
        h.click("load .cube…");
        assert_eq!(h.rig().requests, vec![crate::AppRequest::LoadLut { node: id, key: "lut" }]);
        assert_eq!(h.state().engine.history_len(), depth, "nothing changes until a file is picked");
        assert!(!include_str!("fxui.rs").contains(concat!("rfd", "::")), "no picker in the inspector");
        // D9: the look is CPU maths — no shader module, entry point or
        // .wgsl file in the core, and the app's no more than before (the
        // needles are split so this file never counts itself).
        let count = |dir: &str| {
            let (mut modules, mut stages, mut wgsl) = (0, 0, 0);
            for entry in std::fs::read_dir(dir).expect("the sources") {
                let path = entry.expect("a source entry").path();
                match path.extension().and_then(|x| x.to_str()) {
                    Some("rs") => {
                        let text = std::fs::read_to_string(&path).expect("a source file");
                        modules += text.matches(concat!("create_shader", "_module(")).count();
                        for stage in ["vertex", "fragment", "compute"] {
                            stages += text.matches(&format!("@{stage}")).count();
                        }
                    }
                    Some("wgsl") => wgsl += 1,
                    _ => {}
                }
            }
            (modules, stages, wgsl)
        };
        let core = concat!(env!("CARGO_MANIFEST_DIR"), "/../crates/anim-core/src");
        assert_eq!(count(core), (0, 0, 0), "the core holds no shader");
        let (modules, stages, wgsl) = count(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        assert!(modules <= 4 && stages <= 8 && wgsl == 0, "{modules} modules, {stages} entry points, {wgsl} .wgsl");
    }
}

/// GM16's test minimums and its headless rule, read from the source (the
/// gate's name filter also counts any test whose path merely holds the
/// word): 14 tests in anim-core's gen_tests.rs and 2 genui_* app tests,
/// none of which opens a window or a device, and whose files are written
/// only under the temp dir.
#[cfg(test)]
mod gm16_minimums {
    const GEN_TESTS: &str = include_str!("../../crates/anim-core/tests/gen_tests.rs");
    /// The app files holding genui_* tests.
    const APP: [&str; 3] = [include_str!("cmdpal.rs"), include_str!("nodegraph_panel.rs"), include_str!("plate.rs")];

    /// The test fns named `prefix…` (each right under its #[test] line),
    /// each as its source up to the next test.
    fn tests_named<'a>(src: &'a str, prefix: &str) -> Vec<&'a str> {
        let head = format!("fn {prefix}");
        src.split("#[test]").skip(1).filter(|t| t.trim_start().starts_with(&head)).collect()
    }

    #[test]
    fn test_counts_hold_in_source() {
        let core = tests_named(GEN_TESTS, "").len();
        assert!(core >= 14, "gen_tests.rs holds {core} tests");
        let app: usize = APP.iter().map(|src| tests_named(src, "genui_").len()).sum();
        assert!(app >= 2, "{app} genui_* tests");
    }

    #[test]
    fn genui_tests_stay_headless_and_write_only_under_the_temp_dir() {
        // A window, the GPU, a process, the network, the clock, a sleep or
        // an audio device (spelled in halves so this file never matches).
        let never = [
            concat!("run_", "native"),
            concat!("winit", "::"),
            concat!("wgpu", "::"),
            concat!("std::", "process"),
            concat!("std::", "net"),
            concat!("Instant::", "now"),
            concat!("SystemTime::", "now"),
            concat!("thread::", "sleep"),
            concat!("cpal", "::"),
            concat!("rodio", "::"),
        ];
        let writes = [concat!(".save", "("), concat!("fs::", "write("), concat!("File::", "create("), concat!("create_dir", "_all(")];
        let app = APP.iter().flat_map(|src| tests_named(src, "genui_"));
        let tests: Vec<&str> = tests_named(GEN_TESTS, "").into_iter().chain(app).collect();
        assert!(tests.len() >= 16, "{} tests read", tests.len());
        for t in tests {
            let name = t.trim_start().split('(').next().unwrap_or(t);
            for n in never {
                assert!(!t.contains(n), "{name} reaches for {n}");
            }
            if writes.iter().any(|w| t.contains(w)) {
                assert!(t.contains("std::env::temp_dir()"), "{name} writes outside the temp dir");
            }
        }
        // gen_tests' helpers sit between its tests, so the whole file is read.
        for n in never {
            assert!(!GEN_TESTS.contains(n), "gen_tests.rs reaches for {n}");
        }
        assert!(GEN_TESTS.contains(writes[0]), "the round trip writes a file");
    }
}
