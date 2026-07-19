//! Golden tests for cut image assets (schema v6) — the B2 milestone. Laws:
//! AddImage/RemoveImage are exact inverses (bytes verbatim, library order
//! restored); bad bytes reject the whole batch atomically; ImageSource is
//! content-addressed in eval recipes and tolerated-as-transparent when the
//! asset is missing; decoded texels keep FILE values (premultiplied, no
//! EOTF); v6 round-trips bit-clean and pre-v6 files load with no images.

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::export::render_graph_frame;
use anim_core::graph::NodeKind;
use anim_core::raster::{TILE, f32_to_f16_bits};
use anim_core::xsheet::Exposure;

/// A 2×2 RGBA8 PNG: red, half-alpha green / blue, fully transparent.
fn tiny_png() -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 2, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[
            255, 0, 0, 255, /* */ 0, 255, 0, 128, //
            0, 0, 255, 255, /* */ 0, 0, 0, 0,
        ])
        .unwrap();
    }
    out
}

struct Rig {
    engine: Engine,
    at: CutRef,
}

fn rig() -> Rig {
    let mut engine = Engine::new("image tests");
    let scene = engine.add_scene("SC");
    let cut = engine.add_cut(scene, "cut", 24).unwrap();
    let at = CutRef { scene, cut };
    // A column + exposed empty-ish drawing so the cut is realistic.
    let col = engine.add_column(at, "A").unwrap();
    let d = engine.alloc_drawing_id();
    engine
        .apply(
            "rig",
            vec![
                Command::AddDrawing {
                    at,
                    id: d,
                    index: 0,
                    name: "art".into(),
                    strokes: vec![],
                    layers: vec![],
                },
                Command::SetCell { at, column: col, frame: 0, key: Some(Exposure::Drawing(d)) },
            ],
        )
        .unwrap();
    Rig { engine, at }
}

#[test]
fn add_remove_image_is_one_exact_undo_step() {
    let mut r = rig();
    let baseline = r.engine.project.clone();
    let id = r.engine.alloc_image_id();
    let png = tiny_png();
    let at = r.at;
    r.engine
        .apply(
            "import image",
            vec![Command::AddImage {
                at,
                id,
                index: 0,
                name: "bg".into(),
                bytes: png.clone(),
            }],
        )
        .unwrap();

    let cut = r.engine.project.cut(at.scene, at.cut).unwrap();
    let img = cut.image(id).expect("asset exists");
    assert_eq!(img.width, 2);
    assert_eq!(img.height, 2);
    assert_eq!(*img.bytes, png, "encoded bytes stored verbatim");
    // Decoded texels: FILE values premultiplied, no EOTF. (0,0) opaque red;
    // (1,0) green at alpha 128/255.
    let t = img.tiles.get(&(0, 0)).unwrap();
    assert_eq!(t.rgba[0], f32_to_f16_bits(1.0));
    assert_eq!(t.rgba[3], f32_to_f16_bits(1.0));
    let a = 128.0 / 255.0;
    let i10 = 4; // texel (1,0)
    assert_eq!(t.rgba[i10 + 1], f32_to_f16_bits(a)); // green premult = 1.0·a
    assert_eq!(t.rgba[i10 + 3], f32_to_f16_bits(a));
    let i11 = (TILE + 1) * 4; // texel (1,1): fully transparent
    assert_eq!(t.rgba[i11 + 3] & 0x7FFF, 0);

    // Remove restores library order; undo of BOTH steps is byte-exact.
    r.engine
        .apply("remove image", vec![Command::RemoveImage { at, id }])
        .unwrap();
    r.engine.undo().unwrap(); // un-remove
    assert!(r.engine.project.cut(at.scene, at.cut).unwrap().image(id).is_some());
    r.engine.undo().unwrap(); // un-import
    // Id allocation is not undoable (monotonic by design) — normalize the
    // counter before comparing, the same law layer_tests pins.
    let mut normalized = baseline;
    normalized.next_id = r.engine.project.next_id;
    assert_eq!(r.engine.project, normalized, "byte-identical after undo");
}

#[test]
fn sub_8bit_palette_and_oversize_pngs_never_panic() {
    use anim_core::model::{ImageAsset, MAX_IMAGE_DIM};
    use anim_core::ids::ImageId;

    // 1-bit grayscale (scanned line art): EXPAND normalizes it — must decode,
    // not panic (it once sliced past a bit-packed buffer).
    let mut gray1 = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut gray1, 8, 1);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::One);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[0b1010_1010]).unwrap();
    }
    let img = ImageAsset::from_png(ImageId(1), "scan", gray1).expect("1-bit gray decodes");
    let t = img.tiles.get(&(0, 0)).unwrap();
    assert_eq!(t.rgba[3], f32_to_f16_bits(1.0), "bit 1 → white, opaque");

    // Indexed/palette: EXPAND turns it into RGB — must decode.
    let mut indexed = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut indexed, 2, 1);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(vec![255, 0, 0, 0, 0, 255]);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[0, 1]).unwrap();
    }
    let img2 = ImageAsset::from_png(ImageId(2), "indexed", indexed).expect("palette decodes");
    let t2 = img2.tiles.get(&(0, 0)).unwrap();
    assert_eq!(t2.rgba[0], f32_to_f16_bits(1.0), "palette entry 0 = red");
    assert_eq!(t2.rgba[4 + 2], f32_to_f16_bits(1.0), "palette entry 1 = blue");

    // Oversize DIMENSION rejected from the header, before allocation — a
    // 16385×1 file is tiny on disk but past the per-side cap.
    let mut wide = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut wide, MAX_IMAGE_DIM + 1, 1);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&vec![0u8; (MAX_IMAGE_DIM + 1) as usize]).unwrap();
    }
    let err = ImageAsset::from_png(ImageId(3), "wide", wide);
    assert!(err.is_err(), "oversize must be a clean Err, got {err:?}",);
    assert!(err.unwrap_err().contains("too large"));
}

#[test]
fn bad_bytes_reject_the_whole_batch_atomically() {
    let mut r = rig();
    let baseline = r.engine.project.clone();
    let id = r.engine.alloc_image_id();
    let node = r.engine.alloc_node_id();
    let at = r.at;
    let err = r.engine.apply(
        "import image",
        vec![
            Command::AddImage {
                at,
                id,
                index: 0,
                name: "junk".into(),
                bytes: vec![1, 2, 3, 4],
            },
            Command::AddNode {
                at,
                id: node,
                kind: NodeKind::ImageSource { image: id },
            },
        ],
    );
    assert!(err.is_err(), "garbage bytes must be rejected");
    let mut normalized = baseline;
    normalized.next_id = r.engine.project.next_id;
    assert_eq!(r.engine.project, normalized, "nothing applied");
}

#[test]
fn image_source_is_content_addressed_and_tolerates_removal() {
    let mut r = rig();
    let id = r.engine.alloc_image_id();
    let node = r.engine.alloc_node_id();
    let at = r.at;
    r.engine
        .apply(
            "import image",
            vec![
                Command::AddImage { at, id, index: 0, name: "bg".into(), bytes: tiny_png() },
                Command::AddNode { at, id: node, kind: NodeKind::ImageSource { image: id } },
                Command::SetOutput { at, node: Some(node) },
            ],
        )
        .unwrap();

    let v = r.engine.eval(at.scene, at.cut, 0).unwrap();
    assert!(v.recipe().contains("image("), "recipe: {}", v.recipe());
    assert!(v.recipe().contains('#'), "content hash folded");

    // Render: texel (0,0) = opaque red at exactly (0,0); (1,1) transparent.
    let img = render_graph_frame(r.engine.project.cut(at.scene, at.cut).unwrap(), 0, 8, 8)
        .unwrap();
    assert_eq!(&img[0..4], &[255, 0, 0, 255]);
    assert_eq!(img[(8 + 1) * 4 + 3], 0, "transparent texel stays empty");

    // Removing the asset is tolerated: sentinel recipe, transparent render.
    r.engine
        .apply("remove image", vec![Command::RemoveImage { at, id }])
        .unwrap();
    let v2 = r.engine.eval(at.scene, at.cut, 0).unwrap();
    assert!(v2.recipe().contains("missing-image"), "recipe: {}", v2.recipe());
    let img2 = render_graph_frame(r.engine.project.cut(at.scene, at.cut).unwrap(), 0, 8, 8)
        .unwrap();
    assert!(img2.chunks_exact(4).all(|p| p[3] == 0));
}

#[test]
fn store_v6_roundtrips_and_pre_v6_files_load_without_images() {
    let mut r = rig();
    let id = r.engine.alloc_image_id();
    let node = r.engine.alloc_node_id();
    let at = r.at;
    r.engine
        .apply(
            "import image",
            vec![
                Command::AddImage { at, id, index: 0, name: "bg".into(), bytes: tiny_png() },
                Command::AddNode { at, id: node, kind: NodeKind::ImageSource { image: id } },
            ],
        )
        .unwrap();

    let dir = std::env::temp_dir().join(format!("animstudio_image_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("img.animproj");
    r.engine.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    assert_eq!(loaded.project, r.engine.project, "v6 round-trip identical");

    // Downgrade the file to a pre-v6 shape: no cut_images table, version 5.
    // The loader must gate on the version and come back with no images.
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("UPDATE meta SET value = '5' WHERE key = 'schema_version'", ())
            .unwrap();
        conn.execute("DROP TABLE cut_images", ()).unwrap();
        // The ImageSource node would also not exist in a real v5 file.
        conn.execute("DELETE FROM nodes WHERE id = ?1", (node.0 as i64,))
            .unwrap();
    }
    let old = Engine::load(&path).unwrap();
    let cut = old.project.cut(at.scene, at.cut).unwrap();
    assert!(cut.images.is_empty(), "pre-v6: no images, load still succeeds");
    let _ = std::fs::remove_dir_all(&dir);
}
