use eframe::egui;
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

const HEADER_H: f32 = 26.0;
const PORT_SPACING: f32 = 20.0;
const PORT_R: f32 = 5.0;
const PORT_HIT_R: f32 = 10.0;

struct Node {
    pos: Pos2, // graph space
    size: Vec2,
    title: String,
    n_in: usize,
    n_out: usize,
    hue: f32,
}

struct Link {
    from_node: usize,
    from_port: usize,
    to_node: usize,
    to_port: usize,
}

enum DragState {
    None,
    Pan,
    Node(usize),
    Wire {
        node: usize,
        port: usize,
        is_output: bool,
    },
}

pub struct NodeGraph {
    nodes: Vec<Node>,
    links: Vec<Link>,
    pan: Vec2,   // screen-space offset
    zoom: f32,   // 1.0 = 100%
    drag: DragState,
    pub cull: bool,
    pub painted_last_frame: usize,
}

/// Deterministic pseudo-random (no rand dep, reproducible layouts).
struct Lcg(u64);
impl Lcg {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32) / (u32::MAX >> 1) as f32
    }
}

// Names evoke the actual pipeline stages, so the stress test doubles as a mock-up.
const STAGE_NAMES: &[&str] = &[
    "XSheet", "Genga", "Sakkan Check", "Douga", "Paint", "ColorModel", "BG", "Camera", "Blur",
    "Glow", "Composite", "CutOut", "Trace", "Layout", "Storyboard",
];

impl NodeGraph {
    pub fn generate(count: usize) -> Self {
        let mut rng = Lcg(0xA11CE ^ count as u64);
        let cols = (((count as f32) * 16.0 / 9.0).sqrt().ceil() as usize).max(1);
        let mut nodes = Vec::with_capacity(count);
        for i in 0..count {
            let col = i % cols;
            let row = i / cols;
            let jx = rng.next_f32() * 60.0 - 30.0;
            let jy = rng.next_f32() * 50.0 - 25.0;
            let n_in = 1 + (rng.next_f32() * 2.0) as usize;
            let n_out = 1 + (rng.next_f32() * 2.0) as usize;
            let ports = n_in.max(n_out) as f32;
            nodes.push(Node {
                pos: pos2(col as f32 * 250.0 + jx, row as f32 * 160.0 + jy),
                size: vec2(170.0, HEADER_H + 12.0 + ports * PORT_SPACING),
                title: format!(
                    "{} {}",
                    STAGE_NAMES[(rng.next_f32() * STAGE_NAMES.len() as f32) as usize
                        % STAGE_NAMES.len()],
                    i
                ),
                n_in,
                n_out,
                hue: rng.next_f32(),
            });
        }

        let mut links = Vec::new();
        for i in 1..count {
            let from = i - 1;
            links.push(Link {
                from_node: from,
                from_port: 0,
                to_node: i,
                to_port: 0,
            });
            // Cross-links every 7th node for visual density
            if i % 7 == 0 && i >= 5 {
                links.push(Link {
                    from_node: i - 5,
                    from_port: nodes[i - 5].n_out - 1,
                    to_node: i,
                    to_port: nodes[i].n_in - 1,
                });
            }
        }

        Self {
            nodes,
            links,
            pan: vec2(40.0, 40.0),
            zoom: 0.8,
            drag: DragState::None,
            cull: true,
            painted_last_frame: 0,
        }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn to_screen(&self, origin: Pos2, p: Pos2) -> Pos2 {
        origin + self.pan + p.to_vec2() * self.zoom
    }

    fn from_screen(&self, origin: Pos2, s: Pos2) -> Pos2 {
        ((s - origin - self.pan) / self.zoom).to_pos2()
    }

    fn input_port_screen(&self, origin: Pos2, node: &Node, port: usize) -> Pos2 {
        self.to_screen(
            origin,
            node.pos + vec2(0.0, HEADER_H + 12.0 + port as f32 * PORT_SPACING),
        )
    }

    fn output_port_screen(&self, origin: Pos2, node: &Node, port: usize) -> Pos2 {
        self.to_screen(
            origin,
            node.pos + vec2(node.size.x, HEADER_H + 12.0 + port as f32 * PORT_SPACING),
        )
    }

    /// Returns (node_idx, port_idx, is_output) for a port near the given screen pos.
    fn hit_port(&self, origin: Pos2, s: Pos2) -> Option<(usize, usize, bool)> {
        for (i, node) in self.nodes.iter().enumerate().rev() {
            for p in 0..node.n_in {
                if self.input_port_screen(origin, node, p).distance(s) <= PORT_HIT_R {
                    return Some((i, p, false));
                }
            }
            for p in 0..node.n_out {
                if self.output_port_screen(origin, node, p).distance(s) <= PORT_HIT_R {
                    return Some((i, p, true));
                }
            }
        }
        None
    }

    fn hit_node(&self, origin: Pos2, s: Pos2) -> Option<usize> {
        for (i, node) in self.nodes.iter().enumerate().rev() {
            let rect = Rect::from_min_size(self.to_screen(origin, node.pos), node.size * self.zoom);
            if rect.contains(s) {
                return Some(i);
            }
        }
        None
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let origin = rect.min;
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);

        // ---- Zoom at cursor ----
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                if let Some(mouse) = response.hover_pos() {
                    let graph_at_mouse = self.from_screen(origin, mouse);
                    self.zoom = (self.zoom * (scroll * 0.0015).exp()).clamp(0.08, 3.0);
                    // Keep the graph point under the cursor fixed
                    self.pan = mouse - origin - graph_at_mouse.to_vec2() * self.zoom;
                }
            }
        }

        // ---- Drag state machine ----
        let pointer = response.interact_pointer_pos();
        if response.drag_started_by(egui::PointerButton::Primary) {
            if let Some(p) = pointer {
                self.drag = if let Some((node, port, is_output)) = self.hit_port(origin, p) {
                    DragState::Wire {
                        node,
                        port,
                        is_output,
                    }
                } else if let Some(i) = self.hit_node(origin, p) {
                    // Raise to top: move to end of vec (cheap enough at this scale? No -
                    // indices are stable references for links, so just track without reorder.)
                    DragState::Node(i)
                } else {
                    DragState::Pan
                };
            }
        }
        if response.drag_started_by(egui::PointerButton::Middle) {
            self.drag = DragState::Pan;
        }

        if response.dragged() {
            let delta = response.drag_delta();
            match self.drag {
                DragState::Pan => self.pan += delta,
                DragState::Node(i) => {
                    let d = delta / self.zoom;
                    self.nodes[i].pos += d;
                }
                _ => {}
            }
        }

        if response.drag_stopped() {
            if let DragState::Wire {
                node,
                port,
                is_output,
            } = self.drag
            {
                if let Some(p) = pointer {
                    if let Some((other, other_port, other_is_out)) = self.hit_port(origin, p) {
                        if other != node && other_is_out != is_output {
                            let (from_node, from_port, to_node, to_port) = if is_output {
                                (node, port, other, other_port)
                            } else {
                                (other, other_port, node, port)
                            };
                            self.links.push(Link {
                                from_node,
                                from_port,
                                to_node,
                                to_port,
                            });
                        }
                    }
                }
            }
            self.drag = DragState::None;
        }

        // ---- Paint: wires ----
        let wire_stroke_w = (2.0 * self.zoom).max(1.0);
        let expanded = rect.expand(200.0);
        for link in &self.links {
            let a = self.output_port_screen(origin, &self.nodes[link.from_node], link.from_port);
            let b = self.input_port_screen(origin, &self.nodes[link.to_node], link.to_port);
            if self.cull && !expanded.contains(a) && !expanded.contains(b) {
                continue;
            }
            draw_wire(&painter, a, b, wire_stroke_w, Color32::from_rgb(160, 170, 190));
        }

        // Pending wire preview
        if let DragState::Wire {
            node,
            port,
            is_output,
        } = self.drag
        {
            if let Some(p) = ui.input(|i| i.pointer.hover_pos()).or(pointer) {
                let anchor = if is_output {
                    self.output_port_screen(origin, &self.nodes[node], port)
                } else {
                    self.input_port_screen(origin, &self.nodes[node], port)
                };
                let (a, b) = if is_output { (anchor, p) } else { (p, anchor) };
                draw_wire(&painter, a, b, wire_stroke_w, Color32::from_rgb(255, 210, 90));
            }
        }

        // ---- Paint: nodes ----
        self.painted_last_frame = 0;
        let hover = ui.input(|i| i.pointer.hover_pos());
        // Quantize font size so the galley cache stays effective while zooming
        let title_font = FontId::proportional(((12.0 * self.zoom) * 2.0).round() / 2.0);
        let show_text = self.zoom > 0.35;

        for node in &self.nodes {
            let srect = Rect::from_min_size(self.to_screen(origin, node.pos), node.size * self.zoom);
            if self.cull && !rect.intersects(srect) {
                continue;
            }
            self.painted_last_frame += 1;

            let radius = (6.0 * self.zoom).clamp(1.0, 8.0) as u8;
            // Body
            painter.rect_filled(srect, radius, Color32::from_rgb(43, 46, 56));
            // Header with per-node hue (ComfyUI-style category color)
            let header_color: Color32 =
                egui::ecolor::Hsva::new(node.hue, 0.45, 0.32, 1.0).into();
            let header_rect = Rect::from_min_max(
                srect.min,
                pos2(srect.max.x, srect.min.y + HEADER_H * self.zoom),
            );
            painter.rect_filled(header_rect, radius, header_color);
            painter.rect_stroke(
                srect,
                radius,
                Stroke::new(1.0, Color32::from_rgb(70, 75, 90)),
                egui::StrokeKind::Outside,
            );

            if show_text {
                painter.text(
                    header_rect.left_center() + vec2(6.0 * self.zoom, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &node.title,
                    title_font.clone(),
                    Color32::from_gray(225),
                );
            }

            // Ports
            let port_r = (PORT_R * self.zoom).max(1.5);
            for p in 0..node.n_in {
                let c = self.input_port_screen(origin, node, p);
                let hovered = hover.is_some_and(|h| c.distance(h) <= PORT_HIT_R);
                painter.circle_filled(
                    c,
                    if hovered { port_r * 1.5 } else { port_r },
                    Color32::from_rgb(90, 200, 220),
                );
            }
            for p in 0..node.n_out {
                let c = self.output_port_screen(origin, node, p);
                let hovered = hover.is_some_and(|h| c.distance(h) <= PORT_HIT_R);
                painter.circle_filled(
                    c,
                    if hovered { port_r * 1.5 } else { port_r },
                    Color32::from_rgb(240, 170, 80),
                );
            }
        }

        // Help overlay
        painter.text(
            rect.left_bottom() + vec2(8.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            "drag empty = pan | wheel = zoom | drag node = move | drag port = wire",
            FontId::proportional(12.0),
            Color32::from_gray(120),
        );
    }
}

fn draw_wire(painter: &egui::Painter, a: Pos2, b: Pos2, width: f32, color: Color32) {
    let dx = ((b.x - a.x) * 0.5).abs().max(30.0);
    let shape = egui::epaint::CubicBezierShape::from_points_stroke(
        [a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b],
        false,
        Color32::TRANSPARENT,
        Stroke::new(width, color),
    );
    painter.add(shape);
}
