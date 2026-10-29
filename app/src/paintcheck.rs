//! GM7 THE PAINT CHECK (Window ▸ Paint check): the shiage room's last pass
//! before a cut goes out — unpainted holes (bare paper the art encloses) and
//! stray colours (paint that is no chip of the palette), across the cut or a
//! frame range. The scan runs on a worker thread over a clone of the cut
//! (Arc-tiled, cheap), so the pane never blocks the pen; a finding jumps the
//! canvas to it — its frame, its column, its box.
//!
//! Also home to the swatch board's recolour offer (D8's chip-edit verb).

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use anim_core::color::{Finding, FindingKind};
use eframe::egui;

use crate::canvas::CanvasView;
use crate::doc::AppState;
use crate::plate;

/// GM7 RECOLOUR: a chip that changed, and the colour its paint still has.
#[derive(Debug, Clone, PartialEq)]
pub struct RecolourOffer {
    pub character: usize,
    pub role: usize,
    pub name: String,
    pub from: [u8; 4],
    pub to: [u8; 4],
}

/// The paint check's settings, its job on the worker and its last result.
pub struct PaintCheck {
    /// Check only `range` (inclusive frames) instead of the whole cut.
    pub use_range: bool,
    pub range: (u32, u32),
    /// Holes smaller than this many pixels are specks, not findings.
    pub min_px: usize,
    /// Whose chips count as paint: None = every character's.
    pub character: Option<usize>,
    job: Option<Receiver<Vec<Finding>>>,
    /// The last finished check (None = not run yet).
    pub findings: Option<Vec<Finding>>,
    /// Tests: the worker waits on this before it scans, so a test can hold
    /// the job pending and watch the pane keep running.
    #[cfg(test)]
    pub gate: Option<Receiver<()>>,
}

impl Default for PaintCheck {
    fn default() -> Self {
        Self {
            use_range: false,
            range: (0, 0),
            min_px: 4,
            character: None,
            job: None,
            findings: None,
            #[cfg(test)]
            gate: None,
        }
    }
}

impl PaintCheck {
    /// A check is running on the worker.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Take the worker's result if it is in. Never waits.
    pub fn poll(&mut self) {
        let Some(rx) = &self.job else { return };
        match rx.try_recv() {
            Ok(found) => {
                self.findings = Some(found);
                self.job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.job = None,
        }
    }
}

/// Start a check of the current cut on a worker thread.
pub fn start(state: &mut AppState, ctx: &egui::Context) {
    let cut = state.cut().clone();
    let law = state.engine.project.color_law;
    let size = (state.engine.project.width, state.engine.project.height);
    let pc = &mut state.paintcheck;
    let frames = if pc.use_range {
        pc.range.0..pc.range.1.saturating_add(1)
    } else {
        0..cut.frame_count
    };
    let chips: Vec<[u8; 4]> = state
        .palettes
        .characters
        .iter()
        .enumerate()
        .filter(|(i, _)| pc.character.is_none_or(|c| c == *i))
        .flat_map(|(_, ch)| ch.roles.iter().map(|r| r.color))
        .collect();
    let min_px = pc.min_px.max(1);
    let (tx, rx) = channel();
    #[cfg(test)]
    let gate = pc.gate.take();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        #[cfg(test)]
        if let Some(g) = gate {
            let _ = g.recv();
        }
        let found = anim_core::color::paint_check(&cut, law, size, frames, &chips, min_px);
        let _ = tx.send(found);
        ctx.request_repaint();
    });
    pc.job = Some(rx);
    pc.findings = None;
}

/// A finding's row in the list (also its accessible name).
pub fn finding_label(state: &AppState, f: &Finding) -> String {
    let cut = state.cut();
    let column = cut.xsheet.column(f.column).map(|c| c.name.as_str()).unwrap_or("?");
    let drawing = cut.drawing(f.drawing).map(|d| d.name.as_str()).unwrap_or("?");
    let what = match f.kind {
        FindingKind::Hole => "hole".to_string(),
        FindingKind::Stray => match f.color {
            Some(c) => format!("stray #{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
            None => "stray".to_string(),
        },
    };
    format!(
        "frame {} · {column} {drawing} · {what} · {} px at {}, {}",
        f.frame + 1,
        f.pixels,
        f.bbox[0],
        f.bbox[1]
    )
}

/// Jump to a finding: its frame and column, the canvas framed on its box.
pub fn jump(state: &mut AppState, canvas: &mut CanvasView, f: &Finding) {
    state.goto(f.frame);
    state.view.active_column = f.column;
    canvas.look_at_paper(f.bbox);
    state.status = format!("paint check: {} px at {}, {}", f.pixels, f.bbox[0], f.bbox[1]);
}

pub fn ui(ui: &mut egui::Ui, state: &mut AppState, canvas: &mut CanvasView) {
    state.paintcheck.poll();
    let frames = state.frame_count().max(1);
    plate::legend(ui, "paint check");
    ui.horizontal_wrapped(|ui| {
        let pc = &mut state.paintcheck;
        if plate::detent(ui, !pc.use_range, "whole cut").clicked() {
            pc.use_range = false;
        }
        if plate::detent(ui, pc.use_range, "frames").clicked() {
            pc.use_range = true;
        }
        if pc.use_range {
            // Shown 1-based, like the sheet.
            let (mut a, mut b) = (pc.range.0 + 1, pc.range.1 + 1);
            ui.add(egui::DragValue::new(&mut a).range(1..=frames));
            ui.label("to");
            ui.add(egui::DragValue::new(&mut b).range(a..=frames));
            pc.range = (a - 1, b.max(a) - 1);
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("holes from").color(plate::legend_dim()));
        let mut px = state.paintcheck.min_px;
        ui.add(egui::DragValue::new(&mut px).range(1..=10_000).suffix(" px"));
        state.paintcheck.min_px = px;
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("chips").color(plate::legend_dim()));
        let all = state.paintcheck.character.is_none();
        if plate::detent(ui, all, "every character").clicked() {
            state.paintcheck.character = None;
        }
        for (i, ch) in state.palettes.characters.iter().enumerate() {
            let on = state.paintcheck.character == Some(i);
            if plate::detent(ui, on, &ch.name).clicked() {
                state.paintcheck.character = Some(i);
            }
        }
    });
    ui.add_space(4.0);
    if state.paintcheck.busy() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("checking…");
        });
    } else if ui
        .button("Check paint")
        .on_hover_text("find unpainted holes and colours that are no chip of the palette")
        .clicked()
    {
        start(state, ui.ctx());
    }
    let Some(found) = state.paintcheck.findings.clone() else {
        return;
    };
    let holes = found.iter().filter(|f| f.kind == FindingKind::Hole).count();
    let strays = found.len() - holes;
    ui.label(format!("{holes} hole(s) · {strays} stray colour(s)"));
    if found.is_empty() {
        ui.label(egui::RichText::new("clean — every region is painted in chips").color(plate::legend_dim()));
        return;
    }
    let mut go = None;
    egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
        for (i, f) in found.iter().enumerate() {
            if ui.button(finding_label(state, f)).clicked() {
                go = Some(i);
            }
        }
    });
    if let Some(i) = go {
        jump(state, canvas, &found[i]);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use anim_core::color::ColorLaw;
    use anim_core::model::LayerRole;
    use anim_core::raster::{TILE, TILE_LEN, TileData, f16_bits_to_f32, f32_to_f16_bits};
    use egui::{Modifiers, Pos2, pos2};
    use egui_dock::{DockState, NodeIndex};

    use super::*;
    use crate::config::Action;
    use crate::doc::AppState;
    use crate::newproject::NewProjectForm;
    use crate::palette::{CharacterPalette, ColorRole};
    use crate::testkit::EditorHarness;
    use crate::workspace::Pane;

    const SKIN: [u8; 4] = [236, 188, 150, 255];
    const HAIR: [u8; 4] = [60, 40, 120, 255];

    fn palette(h: &mut EditorHarness) {
        let mut ch = CharacterPalette::new("Mika".to_string());
        ch.roles = vec![
            ColorRole { name: "skin".into(), color: SKIN },
            ColorRole { name: "hair".into(), color: HAIR },
        ];
        h.state_mut().palettes.characters = vec![ch];
        h.state_mut().active_character = 0;
    }

    /// Commit one tile as the armed layer's whole raster (`commit_raster`
    /// takes the layer's full tile set): `px(x, y)` per texel (stored
    /// premultiplied f16 bits).
    fn ink_tile(h: &mut EditorHarness, coord: (i32, i32), px: impl Fn(usize, usize) -> [u16; 4]) {
        let slot = h.state().active_slot_resolved().expect("an armed layer");
        let mut v = vec![0u16; TILE_LEN];
        for y in 0..TILE {
            for x in 0..TILE {
                v[(y * TILE + x) * 4..][..4].copy_from_slice(&px(x, y));
            }
        }
        h.state_mut()
            .commit_raster(vec![(coord, Arc::new(TileData::from_vec(v)))], slot)
            .expect("the tile committed");
    }

    fn texel(h: &EditorHarness, x: i32, y: i32) -> [u16; 4] {
        let t = TILE as i32;
        let tiles = h.state().active_layer_tiles().expect("an armed layer");
        let tile = &tiles[&(x.div_euclid(t), y.div_euclid(t))];
        let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4) as usize;
        [tile.rgba[i], tile.rgba[i + 1], tile.rgba[i + 2], tile.rgba[i + 3]]
    }

    fn alt_click(h: &mut EditorHarness, p: Pos2) {
        // The canvas fits the paper on its first frames; aim once it settled.
        h.run_frames(2);
        let s = h.editor().canvas.view_xform().paper_to_screen(p);
        h.set_mods(Modifiers::ALT);
        h.click_pos(s);
        h.set_mods(Modifiers::NONE);
    }

    #[test]
    fn inkpaint_new_projects_store_exact_chips_and_the_canvas_inks_by_the_law() {
        let form = NewProjectForm::default();
        assert_eq!(form.new_state().engine.project.color_law, ColorLaw::Srgb);
        // The engine default and the test projects stay Legacy (old files).
        let legacy = AppState::new_project("t", 64, 48, 24, 72.0);
        assert_eq!(legacy.engine.project.color_law, ColorLaw::Legacy);

        let mut h = EditorHarness::with_state(form.new_state());
        h.state_mut().new_drawing_at_frame();
        h.editor_mut().canvas.brush_color = HAIR;
        h.run();
        assert_eq!(h.editor().canvas.ink(), ColorLaw::Srgb.encode_rgba(HAIR));
        // The brush's stored value comes home as the chip through the law.
        let stored = h.editor().canvas.ink().map(|v| f16_bits_to_f32(f32_to_f16_bits(v)));
        let law = h.state().engine.project.color_law;
        assert_eq!([law.decode(stored[0]), law.decode(stored[1]), law.decode(stored[2])], [60, 40, 120]);
    }

    /// D8: one encode — the canvas (brush, fills) and the GPU graph
    /// compositor's Solid both go through ColorLaw, never their own curve.
    #[test]
    fn inkpaint_canvas_and_graphcomp_share_one_encode() {
        let canvas = include_str!("canvas.rs");
        let graphcomp = include_str!("graphcomp.rs");
        for (name, src) in [("canvas.rs", canvas), ("graphcomp.rs", graphcomp)] {
            assert!(!src.contains("srgb_to_linear"), "{name} has its own chip curve");
        }
        assert!(canvas.contains("self.color_law.encode_rgba(self.brush_color)"));
        for site in ["let base = self.ink();", "let c = self.ink();"] {
            assert!(canvas.contains(site), "canvas site {site}");
        }
        assert_eq!(canvas.matches("self.ink()").count(), 3, "dab, lasso fill, flood fill");
        assert!(canvas.contains("law.decode("), "the eyedropper decodes by the law");
        assert!(graphcomp.contains("self.law.premultiplied(*rgba)"));
    }

    #[test]
    fn inkpaint_convert_backs_up_clears_history_and_dirties() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("shot.animproj");
        let mut state = AppState::new_project("shot", 1920, 1080, 24, 300.0);
        state.new_drawing_at_frame();
        state.engine.save(&file).unwrap();
        state.file_path = Some(file.clone());
        let mut h = EditorHarness::with_state(state);
        let chip = ColorLaw::Legacy.texel(SKIN);
        ink_tile(&mut h, (15, 8), |_, _| chip);
        assert!(h.state().engine.can_undo());
        let on_disk = std::fs::read(&file).unwrap();

        h.perform(Action::ConvertColorsToSrgb);
        h.run();
        assert!(h.has("Convert"), "the DANGER confirm is up");
        h.hold("Convert");
        h.run();

        let backup = dir.path().join("shot.pre-srgb.animproj");
        let saved = anim_core::Engine::load(&backup).expect("the backup loads");
        assert_eq!(saved.project.color_law, ColorLaw::Legacy);
        let s = h.state();
        assert_eq!(s.engine.project.color_law, ColorLaw::Srgb);
        assert!(!s.engine.can_undo() && s.engine.history_len() == 0, "history cleared");
        assert!(s.is_dirty(), "the converted document is unsaved");
        assert_eq!(std::fs::read(&file).unwrap(), on_disk, "the user's file is untouched");
        // The flat now exports as the chip.
        let img = anim_core::export::render_frame(s.cut(), 0, 1920, 1080);
        let i = ((530 * 1920 + 980) * 4) as usize;
        for c in 0..4 {
            assert!((img[i + c] as i32 - SKIN[c] as i32).abs() <= 1, "{:?}", &img[i..i + 4]);
        }
        // Already Srgb: the command refuses.
        assert!(h.editor().can_perform(Action::ConvertColorsToSrgb).is_err());
    }

    #[test]
    fn inkpaint_alt_click_picks_the_matching_chip_only() {
        let mut h = EditorHarness::new();
        palette(&mut h);
        let law = h.state().engine.project.color_law;
        // Paper x 960..992 is the hair chip, 992..1024 a colour no chip has.
        ink_tile(&mut h, (15, 8), |x, _| law.texel(if x < 32 { HAIR } else { [10, 200, 90, 255] }));
        h.run();
        alt_click(&mut h, pos2(975.5, 530.5));
        let s = h.state();
        assert_eq!(s.active_chip, Some(1), "status '{}' refusal '{}'", s.status, s.refusal);
        assert_eq!(h.editor().canvas.brush_color[..3], HAIR[..3]);
        alt_click(&mut h, pos2(1010.5, 530.5));
        assert_eq!(h.editor().canvas.brush_color[..3], [10, 200, 90]);
        assert_eq!(h.state().active_chip, None, "a non-chip colour selects no chip");
    }

    #[test]
    fn inkpaint_recolour_offer_repaints_in_one_history_step() {
        let mut h = EditorHarness::new();
        palette(&mut h);
        // Paint lives on the color layer (recolour never touches line art).
        h.state_mut().view.active_layer = "color".into();
        h.state_mut().view.active_role = LayerRole::Color;
        let law = h.state().engine.project.color_law;
        ink_tile(&mut h, (15, 8), |x, _| law.texel(if x < 32 { SKIN } else { HAIR }));
        h.editor_mut().dock = DockState::new(vec![Pane::Palette]);
        let new_skin = [250, 200, 170, 255];
        h.state_mut().palettes_mut().characters[0].roles[0].color = new_skin;
        h.state_mut().offer_recolour(0, 0, SKIN, new_skin);
        h.run();
        let depth = h.state().engine.history_len();
        h.click("Recolour this cut");
        h.run();
        assert_eq!(h.state().engine.history_len(), depth + 1, "{}", h.state().refusal);
        let last = h.state().engine.history().last().unwrap().label.clone();
        assert_eq!(last, "recolour paint 'skin'");
        assert_eq!(texel(&h, 960 + 5, 520), law.texel(new_skin));
        assert_eq!(texel(&h, 960 + 40, 520), law.texel(HAIR), "other paint untouched");
        assert!(h.state().recolour_offer.is_none());
        h.state_mut().undo();
        assert_eq!(texel(&h, 960 + 5, 520), law.texel(SKIN));
    }

    #[test]
    fn inkpaint_paint_check_lists_and_jumps_and_never_blocks() {
        let mut h = EditorHarness::new();
        palette(&mut h);
        // A closed box of ink on the armed layer, its inside left bare.
        let ink = ColorLaw::Legacy.texel([20, 20, 24, 255]);
        ink_tile(&mut h, (15, 8), |x, y| {
            let ring = (10..=40).contains(&x) && (10..=30).contains(&y);
            let inside = (11..40).contains(&x) && (11..30).contains(&y);
            if ring && !inside { ink } else { [0; 4] }
        });
        let mut dock = DockState::new(vec![Pane::Canvas]);
        dock.main_surface_mut().split_right(NodeIndex::root(), 0.65, vec![Pane::PaintCheck]);
        h.editor_mut().dock = dock;
        h.state_mut().goto(5);
        h.run();

        let (tx, rx) = std::sync::mpsc::channel();
        h.state_mut().paintcheck.gate = Some(rx);
        h.click("Check paint");
        h.run_frames(4);
        assert!(h.state().paintcheck.busy(), "the scan is still on its worker");
        assert!(h.has_text_containing("checking"));
        tx.send(()).unwrap();
        for _ in 0..400 {
            h.run();
            if !h.state().paintcheck.busy() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let found = h.state().paintcheck.findings.clone().expect("the check finished");
        let holes: Vec<_> = found.iter().filter(|f| f.kind == FindingKind::Hole).collect();
        assert_eq!(holes.len(), 1, "{found:?}");
        assert_eq!(holes[0].bbox, [960 + 11, 512 + 11, 960 + 40, 512 + 30]);
        assert_eq!(holes[0].pixels, 29 * 19);
        assert!(h.has_text_containing("1 hole(s)"));

        let label = finding_label(h.state(), holes[0]);
        h.click(&label);
        h.run();
        assert_eq!(h.state().view.frame, holes[0].frame);
        let xf = h.editor().canvas.view_xform();
        let centre = xf.paper_to_screen(pos2(960.0 + 25.5, 512.0 + 20.5));
        assert!((centre - xf.center).length() < 1.0, "{centre:?} vs {:?}", xf.center);
        assert!(h.editor().canvas.abs_zoom() > 2.0, "zoomed onto the box");
    }
}
