//! GM7 (D8) golden tests for the colour law and the ink-and-paint verbs:
//! chips stored exactly under Srgb (every byte through f16, fills and Solid
//! nodes exporting as the chip), Legacy untouched (the v0.3 encoding, the
//! same render bytes), the Legacy→sRGB conversion, recolour by chip at each
//! scope (paint roles only, one undo step), refill / replace-all and the
//! paint check (holes and stray colours with exact bboxes).

use std::collections::BTreeMap;
use std::sync::Arc;

use anim_core::Engine;
use anim_core::color::{self, ColorLaw, Finding, FindingKind, RecolourScope, RefillRefusal};
use anim_core::command::{Command, CutRef};
use anim_core::export::{
    linear_to_srgb, render_frame, render_graph_frame, render_graph_frame_law, srgb_to_linear,
};
use anim_core::fill::{FillOpts, fill_diff, flood_fill_mask};
use anim_core::graph::{BlendMode, NodeKind};
use anim_core::ids::*;
use anim_core::model::{CelLayer, LayerRole};
use anim_core::raster::{
    TILE, TILE_LEN, TileCoord, TileData, TileDiff, f16_bits_to_f32, f32_to_f16_bits,
};
use anim_core::value::fnv1a;
use anim_core::xsheet::{ColumnDef, Exposure};

/// Two tiles wide, so floods and scans cross a tile seam.
const W: u32 = 128;
const H: u32 = 64;

const C1: [u8; 4] = [200, 120, 37, 255];
const C2: [u8; 4] = [17, 99, 250, 255];
const C3: [u8; 4] = [254, 128, 0, 255];
const INK: [u8; 4] = [20, 20, 24, 255];

type Tiles = BTreeMap<TileCoord, Arc<TileData>>;

/// One cut, one column, one drawing (color, shadow, line) at frame 0.
struct Rig {
    engine: Engine,
    at: CutRef,
    col: ColumnId,
    d: DrawingId,
    color: LayerId,
    shadow: LayerId,
    line: LayerId,
}

fn rig(law: ColorLaw) -> Rig {
    let mut engine = Engine::new("color tests");
    engine.project.color_law = law;
    let scene = engine.add_scene("SC");
    let (at, col, d, [color, shadow, line]) = add_cut(&mut engine, scene, "cut");
    Rig { engine, at, col, d, color, shadow, line }
}

/// A cut with one column showing a fresh color / shadow / line drawing.
fn add_cut(
    engine: &mut Engine,
    scene: SceneId,
    name: &str,
) -> (CutRef, ColumnId, DrawingId, [LayerId; 3]) {
    let cut = engine.add_cut(scene, name, 24).unwrap();
    let at = CutRef { scene, cut };
    let col = engine.add_column(at, "A").unwrap();
    let d = engine.alloc_drawing_id();
    let ids = [engine.alloc_layer_id(), engine.alloc_layer_id(), engine.alloc_layer_id()];
    engine
        .apply(
            "rig",
            vec![
                Command::AddDrawing {
                    at,
                    id: d,
                    index: 0,
                    name: "A1".into(),
                    strokes: vec![],
                    layers: vec![
                        CelLayer::with_role(ids[0], "color", LayerRole::Color),
                        CelLayer::with_role(ids[1], "shadow", LayerRole::Shadow),
                        CelLayer::with_role(ids[2], "line", LayerRole::Line),
                    ],
                },
                Command::SetCell { at, column: col, frame: 0, key: Some(Exposure::Drawing(d)) },
            ],
        )
        .unwrap();
    (at, col, d, ids)
}

fn tiles(engine: &Engine, at: CutRef, d: DrawingId, layer: LayerId) -> Tiles {
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    let drawing = cut.drawing(d).unwrap();
    drawing.layers.iter().find(|l| l.id == layer).unwrap().raster.tiles.clone()
}

/// Write `texel` (f16 bits) at every pixel of `pts` on one layer, as one
/// PaintTiles step through the engine.
fn paint(
    engine: &mut Engine,
    at: CutRef,
    d: DrawingId,
    layer: LayerId,
    pts: &[(i32, i32)],
    texel: [u16; 4],
) {
    let cur = tiles(engine, at, d, layer);
    let t = TILE as i32;
    let mut work: BTreeMap<TileCoord, Vec<u16>> = BTreeMap::new();
    for &(x, y) in pts {
        let coord = (x.div_euclid(t), y.div_euclid(t));
        let px = work.entry(coord).or_insert_with(|| match cur.get(&coord) {
            Some(tile) => tile.rgba.to_vec(),
            None => vec![0; TILE_LEN],
        });
        let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4) as usize;
        px[i..i + 4].copy_from_slice(&texel);
    }
    let diff = work
        .into_iter()
        .map(|(c, px)| (c, cur.get(&c).cloned(), Some(Arc::new(TileData::from_vec(px)))))
        .collect();
    engine.apply("paint", vec![Command::PaintTiles { at, id: d, layer, diff }]).unwrap();
}

fn texel(tiles: &Tiles, x: i32, y: i32) -> [u16; 4] {
    let t = TILE as i32;
    match tiles.get(&(x.div_euclid(t), y.div_euclid(t))) {
        Some(tile) => {
            let i = ((y.rem_euclid(t) * t + x.rem_euclid(t)) * 4) as usize;
            [tile.rgba[i], tile.rgba[i + 1], tile.rgba[i + 2], tile.rgba[i + 3]]
        }
        None => [0; 4],
    }
}

/// Every paper pixel of `[x0, x1) × [y0, y1)`.
fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).collect()
}

/// The one-pixel border of the box with corners (x0, y0) and (x1, y1).
fn outline(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for x in x0..=x1 {
        v.push((x, y0));
        v.push((x, y1));
    }
    for y in y0 + 1..y1 {
        v.push((x0, y));
        v.push((x1, y));
    }
    v
}

/// Paper pixels where two layer states differ.
fn changed(a: &Tiles, b: &Tiles) -> Vec<(i32, i32)> {
    rect(0, 0, W as i32, H as i32)
        .into_iter()
        .filter(|&(x, y)| texel(a, x, y) != texel(b, x, y))
        .collect()
}

fn px(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

impl Rig {
    fn cut(&self) -> &anim_core::model::Cut {
        self.engine.project.cut(self.at.scene, self.at.cut).unwrap()
    }

    fn law(&self) -> ColorLaw {
        self.engine.project.color_law
    }

    fn paint(&mut self, layer: LayerId, pts: &[(i32, i32)], chip: [u8; 4]) {
        let t = self.law().texel(chip);
        let (at, d) = (self.at, self.d);
        paint(&mut self.engine, at, d, layer, pts, t);
    }

    fn tiles(&self, layer: LayerId) -> Tiles {
        tiles(&self.engine, self.at, self.d, layer)
    }

    fn node(&mut self, kind: NodeKind) -> NodeId {
        let id = self.engine.alloc_node_id();
        let at = self.at;
        self.engine.apply("node", vec![Command::AddNode { at, id, kind }]).unwrap();
        id
    }

    fn connect(&mut self, from: NodeId, to: NodeId, pin: u16) {
        let at = self.at;
        let c = Command::Connect { at, from, from_pin: 0, to, to_pin: pin };
        self.engine.apply("connect", vec![c]).unwrap();
    }

    fn output(&mut self, node: NodeId) {
        let at = self.at;
        let c = Command::SetOutput { at, node: Some(node) };
        self.engine.apply("output", vec![c]).unwrap();
    }

    /// Solid(chip) under the column's drawing, wired to the output.
    fn solid_under_art(&mut self, chip: [u8; 4]) {
        let s = self.node(NodeKind::Solid { rgba: chip });
        let src = self.node(NodeKind::source(self.col));
        let blend = self.node(NodeKind::Blend { mode: BlendMode::Normal });
        let out = self.node(NodeKind::Output);
        self.connect(s, blend, 0);
        self.connect(src, blend, 1);
        self.connect(blend, out, 0);
        self.output(out);
    }
}

// ---- The law ----------------------------------------------------------------

#[test]
fn srgb_law_round_trips_every_byte_through_f16() {
    let law = ColorLaw::Srgb;
    for v in 0..=255u8 {
        let stored = f16_bits_to_f32(f32_to_f16_bits(law.encode(v)));
        assert_eq!(law.decode(stored), v, "channel byte {v}");
        let chip = [v, 255 - v, v / 2, 255];
        let back = law.decode_pixel(law.texel(chip).map(f16_bits_to_f32));
        assert_eq!(back, Some(chip), "chip {chip:?}");
    }
    // Bare paper decodes to no chip at all.
    assert_eq!(law.decode_pixel([0.0; 4]), None);
}

#[test]
fn legacy_law_is_the_v03_linear_encoding() {
    let law = ColorLaw::Legacy;
    for v in 0..=255u8 {
        assert_eq!(law.encode(v).to_bits(), srgb_to_linear(v).to_bits(), "byte {v}");
        let stored = f16_bits_to_f32(f32_to_f16_bits(law.encode(v)));
        assert_eq!(law.decode(stored), linear_to_srgb(stored));
        assert_eq!(law.decode(stored), v, "legacy byte {v} comes home through f16");
    }
    // The engine default stays Legacy; keys never change.
    assert_eq!(Engine::new("x").project.color_law, ColorLaw::Legacy);
    assert_eq!(ColorLaw::default(), ColorLaw::Legacy);
    for law in [ColorLaw::Legacy, ColorLaw::Srgb] {
        assert_eq!(ColorLaw::from_key(law.key()), Some(law));
    }
    assert_eq!((ColorLaw::Legacy.key(), ColorLaw::Srgb.key()), ("legacy", "srgb"));
    assert_eq!(ColorLaw::from_key("cmyk"), None);
}

#[test]
fn color_law_persists_and_a_file_without_it_loads_legacy() {
    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("color_law_{}.animproj", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let r = rig(ColorLaw::Srgb);
    r.engine.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    assert_eq!(loaded.project.color_law, ColorLaw::Srgb);
    assert_eq!(loaded.project, r.engine.project);

    // A file from before GM7 has no key: Legacy, everything else intact.
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("DELETE FROM meta WHERE key = 'color_law'", ()).unwrap();
    drop(conn);
    let old = Engine::load(&path).unwrap();
    assert_eq!(old.project.color_law, ColorLaw::Legacy);
    assert_eq!(old.project.scenes, r.engine.project.scenes);

    // A law this build does not know refuses to load instead of mis-rendering.
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("INSERT INTO meta(key, value) VALUES ('color_law', 'cmyk')", ())
        .unwrap();
    drop(conn);
    assert!(Engine::load(&path).is_err());
    // Said as a newer file (D2's wording), not as a corrupt one.
    let err = Engine::load(&path).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(err.contains("newer Gaban Studio") && err.contains("cmyk"), "{err}");
    std::fs::remove_file(&path).unwrap();
}

// ---- Exact chips under Srgb, Legacy untouched --------------------------------

#[test]
fn srgb_fill_exports_exactly_the_chip() {
    let mut r = rig(ColorLaw::Srgb);
    // Line art splits the paper into four boxes; each takes a chip through
    // the fill tool's own path (flood mask + fill_diff of the law's ink).
    let mut ink = Vec::new();
    for x in [31, 63, 95] {
        ink.extend((0..H as i32).map(|y| (x, y)));
    }
    r.paint(r.line, &ink, INK);
    let chips = [C1, C2, C3, [1, 2, 3, 255]];
    let line = r.tiles(r.line);
    let opts = FillOpts { threshold: 0.1, gap_px: 0, grow_px: 0, protect_ink: false };
    for (i, chip) in chips.iter().enumerate() {
        let seed = (i as i32 * 32 + 10, 10);
        let mask = flood_fill_mask(&line, seed, W, H, &opts, None).unwrap();
        let diff = fill_diff(&r.tiles(r.color), &mask, ColorLaw::Srgb.premultiplied(*chip));
        let (at, id, layer) = (r.at, r.d, r.color);
        r.engine.apply("fill", vec![Command::PaintTiles { at, id, layer, diff }]).unwrap();
    }
    let img = render_frame(r.cut(), 0, W, H);
    for (i, chip) in chips.iter().enumerate() {
        let x = i as u32 * 32 + 10;
        assert_eq!(px(&img, x, 10), *chip, "box {i}");
        assert_eq!(px(&img, x + 5, 50), *chip, "box {i}");
    }
}

#[test]
fn srgb_solid_exports_the_chip_and_legacy_keeps_its_linear_bytes() {
    for chip in [C1, C2, C3, [128, 64, 32, 255]] {
        let mut s = rig(ColorLaw::Srgb);
        s.solid_under_art(chip);
        let img = render_graph_frame_law(s.cut(), ColorLaw::Srgb, 0, W, H).unwrap();
        assert_eq!(px(&img, 3, 3), chip);

        // Legacy: the v0.3 bytes — each channel through the EOTF.
        let mut l = rig(ColorLaw::Legacy);
        l.solid_under_art(chip);
        let img = render_graph_frame(l.cut(), 0, W, H).unwrap();
        let lin = |c: u8| (srgb_to_linear(c) * 255.0).round() as u8;
        assert_eq!(px(&img, 3, 3), [lin(chip[0]), lin(chip[1]), lin(chip[2]), 255]);
    }
}

#[test]
fn legacy_render_is_unchanged_by_the_law_seam() {
    let mut r = rig(ColorLaw::Legacy);
    r.paint(r.color, &rect(10, 10, 90, 50), C1);
    r.paint(r.line, &outline(9, 9, 90, 50), INK);
    r.solid_under_art([90, 140, 200, 255]);
    let cut = r.cut();
    let old = render_graph_frame(cut, 0, W, H).unwrap();
    assert_eq!(old, render_graph_frame_law(cut, ColorLaw::Legacy, 0, W, H).unwrap());
    // Pinned: this rig's export bytes as 27cbf36 (the last build before the
    // law existed) rendered them, hashed — the graph path (the Solid under
    // the art) and the column path.
    const PRE_GM7_GRAPH: u64 = 0x34bc_5ae0_8cdf_e645;
    const PRE_GM7_COLUMN: u64 = 0x9df0_3433_c410_5ed5;
    assert_eq!(fnv1a(&old), PRE_GM7_GRAPH, "the Legacy export changed");
    assert_eq!(fnv1a(&render_frame(cut, 0, W, H)), PRE_GM7_COLUMN, "the Legacy export changed");
    // A painted Legacy pixel exports as the linear value of its chip.
    let lin = |c: u8| (srgb_to_linear(c) * 255.0).round() as u8;
    assert_eq!(px(&old, 20, 20), [lin(C1[0]), lin(C1[1]), lin(C1[2]), 255]);
    // Stored values export the same under either law; only the Solid's chip
    // goes through it.
    let srgb = render_graph_frame_law(cut, ColorLaw::Srgb, 0, W, H).unwrap();
    assert_eq!(px(&srgb, 20, 20), px(&old, 20, 20));
    assert_eq!(px(&srgb, 120, 60), [90, 140, 200, 255]);
    assert_ne!(px(&old, 120, 60), [90, 140, 200, 255]);
}

// ---- Converting a Legacy project ---------------------------------------------

#[test]
fn convert_brings_legacy_flats_within_one_of_the_chip() {
    let mut r = rig(ColorLaw::Legacy);
    let chips = [C1, C2, C3, [1, 2, 3, 255], [255, 255, 255, 255], [0, 0, 0, 255]];
    for (i, chip) in chips.iter().enumerate() {
        r.paint(r.color, &rect(i as i32 * 20, 0, i as i32 * 20 + 20, 30), *chip);
    }
    // Under Legacy the export sits darker than the chip.
    let before = render_frame(r.cut(), 0, W, H);
    assert_ne!(px(&before, 5, 5), C1);

    assert!(color::convert_legacy_to_srgb(&mut r.engine.project));
    assert_eq!(r.law(), ColorLaw::Srgb);
    let after = render_frame(r.cut(), 0, W, H);
    for (i, chip) in chips.iter().enumerate() {
        let got = px(&after, i as u32 * 20 + 5, 5);
        for c in 0..4 {
            let d = (got[c] as i32 - chip[c] as i32).abs();
            assert!(d <= 1, "chip {chip:?} exported {got:?}");
        }
    }
    // The paint now matches the chip's Srgb texel exactly where it rounds home.
    assert_eq!(texel(&r.tiles(r.color), 5, 5), ColorLaw::Srgb.texel(C1));
    // Already Srgb: a second run changes nothing.
    let snap = r.engine.project.clone();
    assert!(!color::convert_legacy_to_srgb(&mut r.engine.project));
    assert_eq!(r.engine.project, snap);
}

/// A 2×2 RGBA8 PNG: red, half-alpha green / blue, fully transparent.
fn tiny_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 2, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 0, 0, 0, 0])
            .unwrap();
    }
    out
}

#[test]
fn convert_keeps_coverage_and_leaves_images_alone() {
    let mut r = rig(ColorLaw::Legacy);
    // A soft (half-covered) Legacy texel of C1.
    let soft = ColorLaw::Legacy.texel([C1[0], C1[1], C1[2], 128]);
    let (at, d, layer) = (r.at, r.d, r.color);
    paint(&mut r.engine, at, d, layer, &[(40, 40)], soft);
    let img = r.engine.alloc_image_id();
    let add = Command::AddImage { at, id: img, index: 0, name: "bg".into(), bytes: tiny_png() };
    r.engine.apply("image", vec![add]).unwrap();
    let image_before = r.cut().image(img).unwrap().clone();

    assert!(color::convert_legacy_to_srgb(&mut r.engine.project));
    let t = texel(&r.tiles(r.color), 40, 40);
    assert_eq!(t[3], soft[3], "coverage is kept bit-exact");
    let chip = ColorLaw::Srgb.decode_pixel(t.map(f16_bits_to_f32)).unwrap();
    for c in 0..3 {
        assert!((chip[c] as i32 - C1[c] as i32).abs() <= 1, "{chip:?}");
    }
    assert_eq!(r.cut().image(img).unwrap(), &image_before);
}

// ---- Recolour by chip ----------------------------------------------------------

#[test]
fn recolour_changes_exactly_the_matching_paint_pixels_in_one_step() {
    for law in [ColorLaw::Legacy, ColorLaw::Srgb] {
        let mut r = rig(law);
        r.paint(r.color, &rect(0, 0, 70, 10), C1); // crosses the tile seam
        r.paint(r.color, &rect(0, 10, 20, 20), C3);
        r.paint(r.shadow, &rect(0, 20, 5, 25), C1);
        r.paint(r.line, &rect(30, 30, 40, 40), C1); // line art in the chip
        let soft = law.texel([C1[0], C1[1], C1[2], 128]);
        let (at, d, color) = (r.at, r.d, r.color);
        paint(&mut r.engine, at, d, color, &[(100, 50)], soft);
        let before = r.engine.project.clone();
        let (c0, s0, l0) = (r.tiles(r.color), r.tiles(r.shadow), r.tiles(r.line));
        let depth = r.engine.history_len();

        let cmds = color::recolour(&r.engine.project, RecolourScope::Cut(r.at), C1, C2);
        assert_eq!(cmds.len(), 2, "one PaintTiles per changed layer");
        r.engine.apply("recolour paint", cmds).unwrap();
        assert_eq!(r.engine.history_len(), depth + 1, "one undo step");

        let (c1, s1) = (r.tiles(r.color), r.tiles(r.shadow));
        let mut want = rect(0, 0, 70, 10);
        want.sort_by_key(|&(x, y)| (y, x));
        assert_eq!(changed(&c0, &c1), want);
        assert_eq!(changed(&s0, &s1), rect(0, 20, 5, 25));
        assert_eq!(texel(&c1, 5, 5), law.texel(C2));
        assert_eq!(texel(&c1, 5, 15), law.texel(C3));
        assert_eq!(texel(&c1, 100, 50), soft, "soft paint is not the chip");
        assert_eq!(r.tiles(r.line), l0, "line roles are never touched");

        r.engine.undo().unwrap();
        assert_eq!(r.engine.project, before);
    }
}

#[test]
fn recolour_scopes_reach_exactly_their_cuts() {
    let mut engine = Engine::new("scopes");
    engine.project.color_law = ColorLaw::Srgb;
    let (s1, s2) = (engine.add_scene("S1"), engine.add_scene("S2"));
    let mut cuts = Vec::new();
    for (scene, name) in [(s1, "K1"), (s1, "K2"), (s2, "K3")] {
        let (at, _, d, [color, ..]) = add_cut(&mut engine, scene, name);
        paint(&mut engine, at, d, color, &rect(0, 0, 8, 8), ColorLaw::Srgb.texel(C1));
        cuts.push((at, d, color));
    }
    let reach = |scope| -> Vec<CutRef> {
        color::recolour(&engine.project, scope, C1, C2)
            .into_iter()
            .map(|c| match c {
                Command::PaintTiles { at, .. } => at,
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    };
    let at = |i: usize| cuts[i].0;
    assert_eq!(reach(RecolourScope::Cut(at(1))), vec![at(1)]);
    assert_eq!(reach(RecolourScope::Scene(s1)), vec![at(0), at(1)]);
    assert_eq!(reach(RecolourScope::Scene(s2)), vec![at(2)]);
    assert_eq!(reach(RecolourScope::Project), vec![at(0), at(1), at(2)]);

    let cmds = color::recolour(&engine.project, RecolourScope::Scene(s1), C1, C2);
    engine.apply("recolour paint", cmds).unwrap();
    for (i, &(at, d, layer)) in cuts.iter().enumerate() {
        let want = if i < 2 { C2 } else { C1 };
        assert_eq!(texel(&tiles(&engine, at, d, layer), 3, 3), ColorLaw::Srgb.texel(want));
    }
    // Nothing to change: no commands (the caller refuses instead).
    assert!(color::recolour(&engine.project, RecolourScope::Project, C3, C2).is_empty());
    assert!(color::recolour(&engine.project, RecolourScope::Project, C1, C1).is_empty());
}

#[test]
fn recolour_leaves_locked_layers_and_locked_columns_alone() {
    let mut r = rig(ColorLaw::Srgb);
    r.paint(r.color, &rect(0, 0, 8, 8), C1);
    r.paint(r.shadow, &rect(0, 0, 8, 8), C1);
    let (at, col, d) = (r.at, r.col, r.d);
    let mut props = r.cut().drawing(d).unwrap().layers[1].props.clone();
    props.locked = true;
    let lock = Command::SetCelLayerProps { at, drawing: d, layer: r.shadow, props };
    r.engine.apply("lock", vec![lock]).unwrap();
    let cmds = color::recolour(&r.engine.project, RecolourScope::Project, C1, C2);
    assert_eq!(cmds.len(), 1, "the locked shadow layer is skipped");

    let def = ColumnDef { locked: true, ..ColumnDef::default() };
    r.engine.apply("lock col", vec![Command::SetColumnDef { at, column: col, def }]).unwrap();
    assert!(color::recolour(&r.engine.project, RecolourScope::Project, C1, C2).is_empty());
}

// ---- Refill and replace-all ------------------------------------------------------

/// C1 paint over x 0..100 (tucked under a line at x 70), C3 over 100..120,
/// bare beyond.
fn refill_rig() -> Rig {
    let mut r = rig(ColorLaw::Srgb);
    r.paint(r.color, &rect(0, 0, 100, H as i32), C1);
    r.paint(r.color, &rect(100, 0, 120, H as i32), C3);
    r.paint(r.line, &rect(70, 0, 71, H as i32), INK);
    r
}

#[test]
fn refill_changes_only_the_clicked_connected_region() {
    let r = refill_rig();
    let (before, line) = (r.tiles(r.color), r.tiles(r.line));
    let to = ColorLaw::Srgb.premultiplied(C2);
    let diff = color::refill(&before, &line, (10, 10), W, H, 2, to, None).unwrap();
    let mut after = before.clone();
    for (coord, _, a) in diff {
        match a {
            Some(t) => after.insert(coord, t),
            None => after.remove(&coord),
        };
    }
    // The left region (crossing the tile seam) and the paint under its line.
    let mut want = rect(0, 0, 71, H as i32);
    want.sort_by_key(|&(x, y)| (y, x));
    assert_eq!(changed(&before, &after), want);
    assert_eq!(texel(&after, 69, 30), ColorLaw::Srgb.texel(C2));
    assert_eq!(texel(&after, 80, 30), ColorLaw::Srgb.texel(C1), "across the line");
    assert_eq!(texel(&after, 110, 30), ColorLaw::Srgb.texel(C3));
}

#[test]
fn replace_all_changes_every_exact_match_in_the_layer_and_nothing_else() {
    let r = refill_rig();
    let before = r.tiles(r.color);
    let to = ColorLaw::Srgb.premultiplied(C2);
    let (diff, n) = color::replace_all(&before, (80, 5), W, H, to, None).unwrap();
    assert_eq!(n, 100 * H as usize);
    let mut after = before.clone();
    for (coord, _, a) in diff {
        after.insert(coord, a.unwrap());
    }
    let mut want = rect(0, 0, 100, H as i32);
    want.sort_by_key(|&(x, y)| (y, x));
    assert_eq!(changed(&before, &after), want);
    assert_eq!(texel(&after, 110, 9), ColorLaw::Srgb.texel(C3));
}

#[test]
fn refill_refuses_off_paper_bare_line_and_same_colour() {
    let r = refill_rig();
    let (color, line) = (r.tiles(r.color), r.tiles(r.line));
    let to = ColorLaw::Srgb.premultiplied(C2);
    let same = ColorLaw::Srgb.premultiplied(C1);
    let refill = |seed, c| color::refill(&color, &line, seed, W, H, 2, c, None).err();
    assert_eq!(refill((-1, 4), to), Some(RefillRefusal::OffPaper));
    assert_eq!(refill((4, H as i32), to), Some(RefillRefusal::OffPaper));
    assert_eq!(refill((125, 4), to), Some(RefillRefusal::Bare));
    assert_eq!(refill((70, 4), to), Some(RefillRefusal::OnLine));
    assert_eq!(refill((4, 4), same), Some(RefillRefusal::Unchanged));
    let all = |seed| color::replace_all(&color, seed, W, H, to, None).err();
    assert_eq!(all((125, 4)), Some(RefillRefusal::Bare));
    assert_eq!(all((200, 4)), Some(RefillRefusal::OffPaper));
}

/// A layer's tiles after a diff.
fn applied(before: &Tiles, diff: TileDiff) -> Tiles {
    let mut after = before.clone();
    for (coord, _, a) in diff {
        match a {
            Some(t) => after.insert(coord, t),
            None => after.remove(&coord),
        };
    }
    after
}

#[test]
fn refill_and_replace_all_keep_inside_the_selection() {
    let r = refill_rig();
    let (color, line) = (r.tiles(r.color), r.tiles(r.line));
    let to = ColorLaw::Srgb.premultiplied(C2);
    // The select tool's box over x 20..40, y 0..32 of the C1 flat.
    let sel = [(20.0, 0.0), (40.0, 0.0), (40.0, 32.0), (20.0, 32.0)];
    let mut want = rect(20, 0, 40, 32);
    want.sort_by_key(|&(x, y)| (y, x));
    let diff = color::refill(&color, &line, (25, 10), W, H, 2, to, Some(&sel)).unwrap();
    assert_eq!(changed(&color, &applied(&color, diff)), want, "refill stays in the selection");
    let (diff, n) = color::replace_all(&color, (25, 10), W, H, to, Some(&sel)).unwrap();
    assert_eq!(n, 20 * 32);
    assert_eq!(changed(&color, &applied(&color, diff)), want, "replace-all too");
    // A click outside the selection is refused, as a plain fill's is.
    let outside = Some(RefillRefusal::OutsideSelection);
    assert_eq!(color::refill(&color, &line, (5, 10), W, H, 2, to, Some(&sel)).err(), outside);
    assert_eq!(color::replace_all(&color, (25, 40), W, H, to, Some(&sel)).err(), outside);
}

// ---- The paint check --------------------------------------------------------------

const STRAY: [u8; 4] = [200, 40, 160, 255];

/// Four boxes of line art: two left bare (holes), one painted a stray
/// colour, one painted the chip C1; plus a one-pixel enclosed pinhole.
fn check_rig(paint_all: bool) -> Rig {
    let mut r = rig(ColorLaw::Srgb);
    let mut ink = outline(4, 4, 20, 20);
    ink.extend(outline(56, 4, 80, 20)); // crosses the tile seam
    ink.extend(outline(4, 30, 20, 46));
    ink.extend(outline(30, 30, 50, 46));
    r.paint(r.line, &ink, INK);
    r.paint(r.color, &rect(31, 31, 50, 46), C1);
    if paint_all {
        r.paint(r.color, &rect(5, 5, 20, 20), C1);
        r.paint(r.color, &rect(57, 5, 80, 20), C1);
        r.paint(r.color, &rect(5, 31, 20, 46), C1);
    } else {
        r.paint(r.color, &rect(5, 31, 20, 46), STRAY);
        r.paint(r.line, &outline(100, 40, 102, 42), INK);
    }
    r
}

#[test]
fn paint_check_finds_two_holes_and_one_stray_with_exact_bboxes() {
    let r = check_rig(false);
    let found = color::paint_check(r.cut(), r.law(), (W, H), 0..24, &[C1], 4);
    let hole = |bbox, pixels| Finding {
        kind: FindingKind::Hole,
        frame: 0,
        column: r.col,
        drawing: r.d,
        layer: Some(r.color),
        bbox,
        pixels,
        color: None,
    };
    let stray = Finding {
        kind: FindingKind::Stray,
        bbox: [5, 31, 20, 46],
        pixels: 15 * 15,
        color: Some(STRAY),
        ..hole([0; 4], 0)
    };
    assert_eq!(
        found,
        vec![hole([5, 5, 20, 20], 15 * 15), hole([57, 5, 80, 20], 23 * 15), stray],
    );
    // The one-pixel pinhole shows once min_px allows it.
    let fine = color::paint_check(r.cut(), r.law(), (W, H), 0..24, &[C1], 1);
    assert_eq!(fine.len(), 4);
    assert_eq!(fine[2].bbox, [101, 41, 102, 42]);
}

#[test]
fn paint_check_on_a_clean_drawing_finds_nothing() {
    let r = check_rig(true);
    assert_eq!(color::paint_check(r.cut(), r.law(), (W, H), 0..24, &[C1], 4), vec![]);
    // Without its chip in the palette, the same paint is all stray.
    let strays = color::paint_check(r.cut(), r.law(), (W, H), 0..24, &[C2], 4);
    assert_eq!(strays.len(), 4);
    assert!(strays.iter().all(|f| f.kind == FindingKind::Stray && f.color == Some(C1)));
}

#[test]
fn paint_check_reads_each_drawing_once_at_its_first_frame_in_range() {
    let mut r = check_rig(false);
    let (at, col, d) = (r.at, r.col, r.d);
    let hold = Command::SetCell { at, column: col, frame: 0, key: None };
    let late = Command::SetCell { at, column: col, frame: 6, key: Some(Exposure::Drawing(d)) };
    r.engine.apply("retime", vec![hold, late]).unwrap();
    let found = color::paint_check(r.cut(), r.law(), (W, H), 0..24, &[C1], 4);
    assert_eq!(found.len(), 3);
    assert!(found.iter().all(|f| f.frame == 6));
    // A range that never shows the drawing checks nothing.
    assert!(color::paint_check(r.cut(), r.law(), (W, H), 0..6, &[C1], 4).is_empty());
}

#[test]
fn paint_check_walls_holes_with_the_cel_not_its_rough_or_reference() {
    let mut r = check_rig(false);
    let want = color::paint_check(r.cut(), r.law(), (W, H), 0..24, &[C1], 4);
    assert_eq!(want.len(), 3);
    // A visible rough, LO reference and correction layer over the whole
    // paper: they are not the cel, so the holes under them still show.
    let (at, d) = (r.at, r.d);
    for (name, role) in [
        ("rough", LayerRole::Rough),
        ("lo", LayerRole::Reference),
        ("fix", LayerRole::Correction),
    ] {
        let id = r.engine.alloc_layer_id();
        let layer = CelLayer::with_role(id, name, role);
        let add = Command::AddCelLayer { at, drawing: d, index: 0, layer };
        r.engine.apply("layer", vec![add]).unwrap();
        let plate = ColorLaw::Srgb.texel([90, 90, 200, 255]);
        paint(&mut r.engine, at, d, id, &rect(0, 0, W as i32, H as i32), plate);
    }
    let found = color::paint_check(r.cut(), r.law(), (W, H), 0..24, &[C1], 4);
    assert_eq!(found, want, "the same holes and stray as before");
    // Paint does close a hole, on any paint layer.
    r.paint(r.shadow, &rect(5, 5, 20, 20), C1);
    let found = color::paint_check(r.cut(), r.law(), (W, H), 0..24, &[C1], 4);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].bbox, [57, 5, 80, 20]);
}

#[test]
fn paint_check_reports_soft_paint_as_one_soft_edge_per_layer() {
    let mut r = check_rig(true);
    // Half-covered chip paint — two feathered specks on the color layer,
    // one on the shadow layer: an edge, not a wrong colour.
    let soft = ColorLaw::Srgb.texel([C1[0], C1[1], C1[2], 128]);
    let (at, d, color, shadow) = (r.at, r.d, r.color, r.shadow);
    let mut specks = rect(100, 50, 104, 54);
    specks.extend(rect(110, 10, 112, 12));
    paint(&mut r.engine, at, d, color, &specks, soft);
    paint(&mut r.engine, at, d, shadow, &rect(120, 50, 122, 52), soft);
    let found = color::paint_check(r.cut(), r.law(), (W, H), 0..24, &[C1], 4);
    assert_eq!(found.len(), 2, "one per layer: {found:?}");
    assert!(found.iter().all(|f| f.kind == FindingKind::SoftEdge), "{found:?}");
    let what = |f: &Finding| (f.layer, f.bbox, f.pixels);
    assert_eq!(what(&found[0]), (Some(color), [100, 10, 112, 54], 16 + 4));
    assert_eq!(what(&found[1]), (Some(shadow), [120, 50, 122, 52], 4));
    assert_eq!(found[0].color.map(|c| c[3]), Some(128), "its coverage");
    // Opaque paint of a non-chip colour is still a stray, not an edge.
    r.paint(r.color, &rect(100, 30, 102, 32), STRAY);
    let found = color::paint_check(r.cut(), r.law(), (W, H), 0..24, &[C1], 4);
    let kinds: Vec<_> = found.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, [FindingKind::Stray, FindingKind::SoftEdge, FindingKind::SoftEdge]);
}
