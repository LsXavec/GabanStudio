//! GM11 delivery: the image-sequence writers read back, the ffmpeg
//! arguments and file-name patterns are pinned, burn-ins are pinned and an
//! empty one is the identity, the Final and LineTest purposes keep exactly
//! their layers, and the batch render's command line parses. No process
//! is spawned; every file lands in a temp dir.

use std::path::Path;
use std::sync::Arc;

use anim_core::Engine;
use anim_core::color::ColorLaw;
use anim_core::command::{Command, CutRef};
use anim_core::deliver::{
    self, AudioIn, BurnIns, CliSource, Codec, Format, Names, PRESETS, Preset, Purpose, RenderOpts,
    Slate,
};
use anim_core::export::{render_frame, render_graph_frame_law, straight_rgba8};
use anim_core::fx::Image;
use anim_core::graph::NodeKind;
use anim_core::model::{CelLayer, Cut, LayerRole};
use anim_core::raster::{TILE, TILE_LEN, TileData, f32_to_f16_bits};
use anim_core::xsheet::{ColumnDef, ColumnKind, Exposure};

const W: u32 = 64;
const H: u32 = 36;

/// A cut whose column A shows one drawing on every frame, with one layer
/// per role; each layer inks its own 6×8 block at x = 6 · rank (opaque,
/// premultiplied `rgba`). With `ref_column`, a Ref column B also shows a
/// drawing (a block at x = 56).
fn rig(roles: &[(LayerRole, [f32; 4])], ref_column: bool) -> (Engine, CutRef) {
    let mut engine = Engine::new("delivery tests");
    let scene = engine.add_scene("s01");
    let cut = engine.add_cut(scene, "c012", 3).unwrap();
    let at = CutRef { scene, cut };
    let col = engine.add_column(at, "A").unwrap();
    let d = engine.alloc_drawing_id();
    let mut layers = Vec::new();
    let mut paint = Vec::new();
    for &(role, rgba) in roles {
        let id = engine.alloc_layer_id();
        layers.push(CelLayer::with_role(id, role.default_name(), role));
        let x0 = 6 * role.rank().unwrap_or(9) as usize;
        paint.push(Command::PaintTiles {
            at,
            id: d,
            layer: id,
            diff: vec![((0, 0), None, Some(block(x0, 4, 6, 8, rgba)))],
        });
    }
    let mut cmds = vec![
        Command::AddDrawing {
            at,
            id: d,
            index: 0,
            name: "art".into(),
            strokes: vec![],
            layers,
        },
        Command::SetCell {
            at,
            column: col,
            frame: 0,
            key: Some(Exposure::Drawing(d)),
        },
    ];
    cmds.extend(paint);
    engine.apply("rig", cmds).unwrap();
    if ref_column {
        let rcol = engine.add_column(at, "B").unwrap();
        let (rd, rl) = (engine.alloc_drawing_id(), engine.alloc_layer_id());
        engine
            .apply(
                "ref",
                vec![
                    Command::AddDrawing {
                        at,
                        id: rd,
                        index: 1,
                        name: "lo".into(),
                        strokes: vec![],
                        layers: vec![CelLayer::with_role(rl, "color", LayerRole::Color)],
                    },
                    Command::PaintTiles {
                        at,
                        id: rd,
                        layer: rl,
                        diff: vec![((0, 0), None, Some(block(56, 4, 6, 8, [1.0, 0.0, 0.0, 1.0])))],
                    },
                    Command::SetCell {
                        at,
                        column: rcol,
                        frame: 0,
                        key: Some(Exposure::Drawing(rd)),
                    },
                    Command::SetColumnDef {
                        at,
                        column: rcol,
                        def: ColumnDef {
                            kind: ColumnKind::Ref,
                            hidden: false,
                            locked: false,
                        },
                    },
                ],
            )
            .unwrap();
    }
    (engine, at)
}

/// One tile with a `w × h` block of `rgba` at (x0, y0).
fn block(x0: usize, y0: usize, w: usize, h: usize, rgba: [f32; 4]) -> Arc<TileData> {
    let mut px = vec![0u16; TILE_LEN];
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            for c in 0..4 {
                px[(y * TILE + x) * 4 + c] = f32_to_f16_bits(rgba[c]);
            }
        }
    }
    Arc::new(TileData::from_vec(px))
}

fn cut(e: &Engine, at: CutRef) -> &Cut {
    e.project.cut(at.scene, at.cut).unwrap()
}

/// Wire column A → Output, so the graph path renders.
fn wire_graph(e: &mut Engine, at: CutRef) {
    let column = cut(e, at).xsheet.columns[0].id;
    let (src, out) = (e.alloc_node_id(), e.alloc_node_id());
    e.apply(
        "graph",
        vec![
            Command::AddNode {
                at,
                id: src,
                kind: NodeKind::DrawingSource { column },
            },
            Command::AddNode {
                at,
                id: out,
                kind: NodeKind::Output,
            },
            Command::Connect {
                at,
                from: src,
                from_pin: 0,
                to: out,
                to_pin: 0,
            },
            Command::SetOutput {
                at,
                node: Some(out),
            },
        ],
    )
    .unwrap();
}

fn opts(purpose: Purpose) -> RenderOpts {
    RenderOpts {
        purpose,
        scale: 1.0,
    }
}

fn fnv(img: &Image) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in &img.px {
        for b in v.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// A 7×5 premultiplied test card: gradients in colour and alpha, with a
/// transparent corner.
fn card() -> Image {
    let mut img = Image::new(7, 5);
    for y in 0..5 {
        for x in 0..7 {
            let a = if x == 0 && y == 0 {
                0.0
            } else {
                (x + 1) as f32 / 7.0
            };
            let c = [x as f32 / 6.0, y as f32 / 4.0, 0.3];
            img.set_pixel(x, y, [c[0] * a, c[1] * a, c[2] * a, a]);
        }
    }
    img
}

fn has_pair(args: &[String], a: &str, b: &str) -> bool {
    args.windows(2).any(|w| w[0] == a && w[1] == b)
}

#[test]
fn png16_reads_back_as_the_buffer_quantised_to_16_bits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.png");
    let img = card();
    deliver::write_png16(&path, &img).unwrap();
    let back = image::open(&path).unwrap();
    assert_eq!((back.width(), back.height()), (7, 5));
    assert_eq!(back.color(), image::ColorType::Rgba16);
    assert_eq!(back.to_rgba16().into_raw(), deliver::quantize16(&img));
    // The law itself, on one pixel: straight colour, then 16 bits.
    let mut one = Image::new(1, 1);
    one.set_pixel(0, 0, [0.25, 0.1, 0.0, 0.5]);
    assert_eq!(deliver::quantize16(&one), vec![32768, 13107, 0, 32768]);
    assert_eq!(deliver::quantize16(&Image::new(1, 1)), vec![0, 0, 0, 0]);
}

#[test]
fn tiff16_and_exr_read_back_with_their_size_and_values() {
    let dir = tempfile::tempdir().unwrap();
    let img = card();
    let tif = dir.path().join("f.tif");
    deliver::write_tiff16(&tif, &img).unwrap();
    let back = image::open(&tif).unwrap();
    assert_eq!((back.width(), back.height()), (7, 5));
    assert_eq!(back.to_rgba16().into_raw(), deliver::quantize16(&img));
    // EXR half: the premultiplied values, within half-float precision.
    let exr = dir.path().join("f.exr");
    deliver::write_exr(&exr, &img, ColorLaw::Legacy).unwrap();
    let back = image::open(&exr).unwrap();
    assert_eq!((back.width(), back.height()), (7, 5));
    let px = back.to_rgba32f().into_raw();
    assert_eq!(px.len(), img.px.len());
    for (got, want) in px.iter().zip(&img.px) {
        assert!(
            (got - want).abs() <= want.abs() / 1024.0 + 1e-4,
            "{got} vs {want}"
        );
    }
}

#[test]
fn exr_is_scene_linear_under_either_colour_law() {
    // D8: Legacy stores linear values (written as they are); Srgb stores
    // the chip's gamma values, so a 50 % grey linearises to 0.214.
    let p = [0.25, 0.25, 0.25, 0.5];
    assert_eq!(deliver::scene_linear(p, ColorLaw::Legacy), p);
    let lin = deliver::scene_linear(p, ColorLaw::Srgb);
    assert!((lin[0] - 0.214_041_14 * 0.5).abs() < 1e-5, "{lin:?}");
    assert_eq!(lin[3], 0.5);
    assert_eq!(deliver::scene_linear([0.0; 4], ColorLaw::Srgb), [0.0; 4]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grey.exr");
    let mut img = Image::new(1, 1);
    img.set_pixel(0, 0, [0.5, 0.5, 0.5, 1.0]);
    deliver::write_exr(&path, &img, ColorLaw::Srgb).unwrap();
    let v = image::open(&path).unwrap().to_rgba32f().into_raw();
    assert!((v[0] - 0.214_041_14).abs() < 2e-4 && v[3] == 1.0, "{v:?}");
}

#[test]
fn ffmpeg_args_are_pinned_for_each_video_preset() {
    let out = Path::new("out.mov");
    let args = |c: Codec, alpha: bool| deliver::ffmpeg_args(c, 24, 1920, 1080, alpha, None, out);
    // Frames arrive as rawvideo RGBA on stdin.
    let head: Vec<String> = [
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-video_size",
        "1920x1080",
        "-framerate",
        "24",
        "-i",
        "-",
    ]
    .map(String::from)
    .to_vec();
    let p4444 = args(Codec::ProRes4444, true);
    assert_eq!(p4444[..head.len()], head[..]);
    assert!(has_pair(&p4444, "-c:v", "prores_ks"));
    assert!(has_pair(&p4444, "-profile:v", "4"));
    assert!(has_pair(&p4444, "-pix_fmt", "yuva444p10le"));
    assert!(has_pair(
        &args(Codec::ProRes4444, false),
        "-pix_fmt",
        "yuv444p10le"
    ));
    let p422 = args(Codec::ProRes422Hq, false);
    assert!(has_pair(&p422, "-profile:v", "3") && has_pair(&p422, "-pix_fmt", "yuv422p10le"));
    let dnx = args(Codec::DnxhrHq, false);
    assert!(has_pair(&dnx, "-c:v", "dnxhd"));
    assert!(has_pair(&dnx, "-profile:v", "dnxhr_hq"));
    assert!(has_pair(&dnx, "-pix_fmt", "yuv422p"));
    let h265 = args(Codec::H265, false);
    assert!(has_pair(&h265, "-c:v", "libx265") && has_pair(&h265, "-tag:v", "hvc1"));
    // H.264 is v0.3's MP4, argument for argument after the input.
    let h264 = deliver::ffmpeg_args(Codec::H264, 24, 64, 36, false, None, Path::new("a.mp4"));
    let tail: Vec<String> = [
        "-c:v",
        "libx264",
        "-profile:v",
        "high",
        "-level:v",
        "4.1",
        "-pix_fmt",
        "yuv420p",
        "-crf",
        "18",
        "-preset",
        "medium",
        "-movflags",
        "+faststart",
        "-vf",
        "crop=trunc(iw/2)*2:trunc(ih/2)*2",
        "a.mp4",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(h264[11..], tail[..]);
    assert_eq!(Codec::H264.ext(), "mp4");
    assert_eq!(Codec::ProRes4444.ext(), "mov");
}

#[test]
fn ffmpeg_audio_rides_along_as_v03_muxed_it_and_is_absent_without_audio() {
    let wav = Path::new("audio.wav");
    let au = Some(AudioIn {
        path: wav,
        offset: 0.5,
        secs: 3.0,
    });
    let h264 = deliver::ffmpeg_args(Codec::H264, 24, 64, 36, false, au, Path::new("a.mp4"));
    let mux: Vec<String> = [
        "-ss",
        "0.500000",
        "-i",
        "audio.wav",
        "-map",
        "0:v",
        "-map",
        "1:a",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-t",
        "3.000000",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(h264[11..11 + mux.len()], mux[..]);
    for c in [Codec::ProRes422Hq, Codec::ProRes4444, Codec::DnxhrHq] {
        let a = deliver::ffmpeg_args(c, 24, 64, 36, false, au, Path::new("a.mov"));
        assert!(has_pair(&a, "-c:a", "pcm_s24le"), "{c:?}");
        assert!(!a.iter().any(|s| s == "aac"), "{c:?}");
    }
    let h265 = deliver::ffmpeg_args(Codec::H265, 24, 64, 36, false, au, Path::new("a.mp4"));
    assert!(has_pair(&h265, "-c:a", "aac") && has_pair(&h265, "-b:a", "192k"));
    for c in [
        Codec::H264,
        Codec::H265,
        Codec::ProRes422Hq,
        Codec::ProRes4444,
        Codec::DnxhrHq,
    ] {
        let a = deliver::ffmpeg_args(c, 24, 64, 36, false, None, Path::new("a.mov"));
        for flag in ["-c:a", "-b:a", "-map", "-ss", "-t"] {
            assert!(
                !a.iter().any(|s| s == flag),
                "{c:?} carries {flag} without audio"
            );
        }
        assert_eq!(a.iter().filter(|s| *s == "-i").count(), 1, "{c:?}");
    }
}

#[test]
fn expand_pattern_is_pinned_and_sanitised() {
    let names = Names {
        project: "show".into(),
        scene: "s01".into(),
        cut: "c012".into(),
    };
    assert_eq!(
        deliver::expand_pattern("{project}_{cut}_{frame:04}", &names, 7, "png"),
        "show_c012_0007.png"
    );
    assert_eq!(
        deliver::expand_pattern(deliver::DEFAULT_PATTERN, &names, 12, "exr"),
        "show_s01_c012_0012.exr"
    );
    assert_eq!(
        deliver::expand_pattern("{cut}-{frame}", &names, 3, "tif"),
        "c012-3.tif"
    );
    // Unknown tokens stay as written; illegal characters become '_'.
    let odd = Names {
        project: "a/b:c*?".into(),
        scene: "s<1>".into(),
        cut: "c|\"2\\".into(),
    };
    assert_eq!(
        deliver::expand_pattern("{project}_{scene}_{cut}_{take}_{frame:03}", &odd, 1, "png"),
        "a_b_c___s_1__c__2__{take}_001.png"
    );
    assert_eq!(
        deliver::expand_pattern("{cut}. ", &names, 1, "png"),
        "c012.png"
    );
    // A video takes the pattern without its frame.
    assert_eq!(
        deliver::video_name(deliver::DEFAULT_PATTERN, &names, "mp4"),
        "show_s01_c012.mp4"
    );
    assert_eq!(deliver::video_name("{frame}", &names, "mov"), "render.mov");
}

#[test]
fn burn_ins_are_pinned_and_an_empty_burn_in_is_the_identity() {
    let slate = Slate {
        names: Names {
            project: "show".into(),
            scene: "s01".into(),
            cut: "c012".into(),
        },
        fps: 24,
        date: "2026-10-29".into(),
        artist: "mika".into(),
    };
    let base = deliver::over(&card(), [40, 40, 40]);
    let mut plain = Image::new(256, 144);
    for p in plain.px.chunks_exact_mut(4) {
        p.copy_from_slice(&[0.5, 0.5, 0.5, 1.0]);
    }
    // No burn-ins: untouched, bit for bit.
    let mut same = base.clone();
    deliver::burn_in(&mut same, BurnIns::NONE, &slate, 30);
    assert_eq!(same, base);
    // Every corner on a 256×144 grey frame: pinned pixels.
    let mut img = plain.clone();
    deliver::burn_in(&mut img, BurnIns::REVIEW, &slate, 30);
    assert_ne!(img, plain);
    // Text in each corner, the middle left alone.
    let marked = |x0: u32, y0: u32| {
        (y0..y0 + 20).any(|y| (x0..x0 + 100).any(|x| img.pixel(x, y) != plain.pixel(x, y)))
    };
    for (x0, y0) in [(0, 0), (156, 0), (0, 124), (156, 124)] {
        assert!(marked(x0, y0), "no burn-in near ({x0}, {y0})");
    }
    assert!(!marked(78, 62));
    assert_eq!(
        fnv(&img),
        BURN_IN_FNV,
        "burn-in pixels moved: {:#x}",
        fnv(&img)
    );
    // Only the frame: the top rows stay the plate.
    let mut frame_only = plain.clone();
    let which = BurnIns {
        frame: true,
        ..BurnIns::NONE
    };
    deliver::burn_in(&mut frame_only, which, &slate, 30);
    assert_eq!(frame_only.px[..256 * 60 * 4], plain.px[..256 * 60 * 4]);
    assert_ne!(frame_only, plain);
    // The text it states.
    assert_eq!(deliver::seconds_koma(30, 24), "1+07");
    assert_eq!(deliver::seconds_koma(0, 24), "0+01");
    assert_eq!(deliver::timecode(30, 24), "00:00:01:06");
    assert_eq!(deliver::timecode(24 * 3725 + 5, 24), "01:02:05:05");
}

/// The pinned FNV of `BurnIns::REVIEW` on the 256×144 grey frame.
const BURN_IN_FNV: u64 = 0x8241_a451_49fe_c4bc;

#[test]
fn final_leaves_out_correction_rough_reference_and_ref_columns() {
    let ink = [0.0, 0.0, 0.5, 1.0];
    let all = [
        (LayerRole::Reference, [0.0, 0.5, 0.0, 1.0]),
        (LayerRole::Color, [0.8, 0.6, 0.4, 1.0]),
        (LayerRole::Rough, [0.2, 0.2, 0.9, 1.0]),
        (LayerRole::Line, ink),
        (LayerRole::Correction, [1.0, 0.0, 0.0, 1.0]),
    ];
    let kept = [
        (LayerRole::Color, [0.8, 0.6, 0.4, 1.0]),
        (LayerRole::Line, ink),
    ];
    for graph in [false, true] {
        let (mut full, at) = rig(&all, true);
        let (mut bare, bat) = rig(&kept, false);
        if graph {
            wire_graph(&mut full, at);
            wire_graph(&mut bare, bat);
        }
        let law = ColorLaw::Legacy;
        let fin = deliver::render(cut(&full, at), law, 0, W, H, &opts(Purpose::Final));
        let want = deliver::render(cut(&bare, bat), law, 0, W, H, &opts(Purpose::Final));
        assert_eq!(fin, want, "graph {graph}: Final shows a left-out layer");
        // Under View the correction (x 48..54) and the rough (24..30) show.
        let view = deliver::render(cut(&full, at), law, 0, W, H, &opts(Purpose::View));
        assert_eq!(view.pixel(50, 6), [1.0, 0.0, 0.0, 1.0], "graph {graph}");
        assert!(view.pixel(26, 6)[3] > 0.0, "graph {graph}");
        assert_eq!(fin.pixel(50, 6), [0.0; 4], "graph {graph}");
        // The Ref column never renders, View or Final.
        assert_eq!(view.pixel(58, 6), [0.0; 4], "graph {graph}");
        // The column filter is the drawing's own: flatten_for(Final).
        let d = &cut(&full, at).drawings[0];
        assert_eq!(d.flatten_for(Purpose::View), d.flatten());
        assert_ne!(d.flatten_for(Purpose::Final), d.flatten());
    }
}

#[test]
fn line_test_renders_only_the_line_roles_black_on_white() {
    let (e, at) = rig(
        &[
            (LayerRole::Color, [0.8, 0.6, 0.4, 1.0]),
            (LayerRole::ShadowLine, [0.0, 0.3, 0.6, 1.0]),
            (LayerRole::Line, [0.0, 0.0, 0.5, 1.0]),
        ],
        false,
    );
    let img = deliver::render(
        cut(&e, at),
        ColorLaw::Legacy,
        0,
        W,
        H,
        &opts(Purpose::LineTest),
    );
    let (white, black) = ([1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 1.0]);
    let line_x = |x: u32| (30..36).contains(&x) || (42..48).contains(&x);
    for y in 0..H {
        for x in 0..W {
            let want = if line_x(x) && (4..12).contains(&y) {
                black
            } else {
                white
            };
            assert_eq!(img.pixel(x, y), want, "({x}, {y})");
        }
    }
}

#[test]
fn view_is_todays_render_on_both_paths() {
    let (mut e, at) = rig(
        &[
            (LayerRole::Color, [0.8, 0.6, 0.4, 1.0]),
            (LayerRole::Line, [0.0, 0.0, 0.5, 0.5]),
        ],
        true,
    );
    let to8 =
        |img: &Image| -> Vec<u8> { img.px.chunks_exact(4).flat_map(straight_rgba8).collect() };
    let view = deliver::render(cut(&e, at), ColorLaw::Srgb, 0, W, H, &RenderOpts::default());
    assert_eq!(to8(&view), render_frame(cut(&e, at), 0, W, H));
    wire_graph(&mut e, at);
    let view = deliver::render(cut(&e, at), ColorLaw::Srgb, 0, W, H, &RenderOpts::default());
    assert_eq!(
        Some(to8(&view)),
        render_graph_frame_law(cut(&e, at), ColorLaw::Srgb, 0, W, H)
    );
}

#[test]
fn a_half_scale_proxy_averages_each_2x2_block() {
    let (e, at) = rig(&[(LayerRole::Color, [0.8, 0.6, 0.4, 1.0])], false);
    let full = deliver::render(
        cut(&e, at),
        ColorLaw::Legacy,
        0,
        W,
        H,
        &RenderOpts::default(),
    );
    let proxy = RenderOpts {
        purpose: Purpose::View,
        scale: 0.5,
    };
    let half = deliver::render(cut(&e, at), ColorLaw::Legacy, 0, W, H, &proxy);
    assert_eq!((half.w, half.h), (32, 18));
    for (x, y) in [(3, 2), (4, 3), (0, 0), (31, 17)] {
        let mut want = [0.0f32; 4];
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let p = full.pixel(2 * x + dx, 2 * y + dy);
            for c in 0..4 {
                want[c] += p[c] / 4.0;
            }
        }
        assert_eq!(half.pixel(x, y), want, "({x}, {y})");
    }
    assert_eq!(
        Preset::named("Master PNG 16-bit").unwrap().out_size(W, H),
        (W, H)
    );
}

#[test]
fn presets_cover_the_delivery_list() {
    let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
    assert_eq!(
        names,
        [
            "Master PNG 16-bit",
            "TIFF 16-bit",
            "EXR half (linear)",
            "PNG 8-bit",
            "H.264 review",
            "H.265",
            "ProRes 422 HQ",
            "ProRes 4444 (alpha)",
            "DNxHR HQ",
            "Line test",
        ]
    );
    let p = |n: &str| Preset::named(n).unwrap();
    assert_eq!(p("master png 16-bit").format, Format::Png16);
    assert_eq!(p("EXR half (linear)").format.ext(), "exr");
    assert!(p("ProRes 4444 (alpha)").alpha && !p("ProRes 422 HQ").alpha);
    assert!(!p("H.264 review").burn_ins.is_empty());
    assert_eq!(p("Line test").purpose, Purpose::LineTest);
    assert_eq!(p("Master PNG 16-bit").purpose, Purpose::Final);
    assert!(PRESETS.iter().all(|p| p.scale == 1.0 || p.scale == 0.5));
    assert_eq!(PRESETS.iter().filter(|p| !p.format.is_video()).count(), 4);
    assert!(Preset::named("Master PNG 8-bit").is_none());
}

#[test]
fn parse_cli_maps_the_job_and_refuses_bad_arguments() {
    let args = |s: &[&str]| s.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    let job = deliver::parse_cli(&args(&[
        "--render",
        "x.animproj",
        "--preset",
        "Master PNG 16-bit",
        "--out",
        "d",
    ]))
    .unwrap();
    assert_eq!(job.source, CliSource::File("x.animproj".into()));
    assert_eq!(job.preset.name, "Master PNG 16-bit");
    assert_eq!(job.out, Path::new("d"));
    let sample = deliver::parse_cli(&args(&[
        "--render",
        "--sample",
        "--out",
        "o",
        "--preset",
        "PNG 8-bit",
    ]))
    .unwrap();
    assert_eq!(sample.source, CliSource::Sample);
    assert!(deliver::is_cli(&args(&["--render", "--sample"])));
    assert!(!deliver::is_cli(&args(&["x.animproj"])));
    let unknown = deliver::parse_cli(&args(&[
        "--render",
        "x.animproj",
        "--preset",
        "Betamax",
        "--out",
        "d",
    ]));
    assert!(unknown.unwrap_err().contains("unknown preset \"Betamax\""));
    let no_out = deliver::parse_cli(&args(&["--render", "x.animproj", "--preset", "PNG 8-bit"]));
    assert!(no_out.unwrap_err().contains("--out is missing"));
    assert!(deliver::parse_cli(&args(&["--render"])).is_err());
    assert!(
        deliver::parse_cli(&args(&[
            "--render",
            "x",
            "--preset",
            "PNG 8-bit",
            "--out",
            "d",
            "--fast"
        ]))
        .is_err()
    );
    assert_eq!(
        (deliver::EXIT_OK, deliver::EXIT_FAILED, deliver::EXIT_USAGE),
        (0, 1, 2)
    );
}

#[test]
fn a_delivered_frame_lies_on_white_without_alpha_and_keeps_it_with() {
    let (e, at) = rig(&[(LayerRole::Line, [0.0, 0.0, 0.5, 1.0])], false);
    let slate = Slate {
        fps: 24,
        ..Slate::default()
    };
    let c = cut(&e, at);
    let master = Preset::named("Master PNG 16-bit").unwrap();
    let a = deliver::deliver_frame(
        c,
        ColorLaw::Legacy,
        0,
        (W, H),
        master,
        BurnIns::NONE,
        &slate,
    );
    assert_eq!(a.pixel(0, 0), [0.0; 4]);
    let review = Preset::named("ProRes 422 HQ").unwrap();
    let b = deliver::deliver_frame(
        c,
        ColorLaw::Legacy,
        0,
        (W, H),
        review,
        BurnIns::NONE,
        &slate,
    );
    assert_eq!(b.pixel(0, 0), [1.0; 4]);
    assert_eq!(b.pixel(44, 6), [0.0, 0.0, 0.5, 1.0]);
    // Every image format writes; a video frame is refused.
    let dir = tempfile::tempdir().unwrap();
    for f in [Format::Png16, Format::Tiff16, Format::Exr, Format::Png8] {
        let path = dir.path().join(format!("f.{}", f.ext()));
        deliver::write_frame(&path, &a, f, ColorLaw::Legacy).unwrap();
        assert!(path.is_file(), "{f:?}");
    }
    let png8 = image::open(dir.path().join("f.png")).unwrap().to_rgba8();
    assert_eq!(png8.get_pixel(44, 6).0, [0, 0, 128, 255]);
    assert!(
        deliver::write_frame(
            &dir.path().join("v"),
            &a,
            Format::Video(Codec::H264),
            ColorLaw::Legacy
        )
        .is_err()
    );
}

#[test]
fn text_lines_lay_out_left_to_right_in_the_embedded_faces() {
    use anim_core::text::{self, Face};
    let line = text::layout_line(Face::Mono, "c012", 20.0);
    assert_eq!(line.glyphs.len(), 4);
    // A monospace face: equal advances.
    let step = line.glyphs[1].x - line.glyphs[0].x;
    assert!(
        step > 5.0 && (line.width - 4.0 * step).abs() < 1e-3,
        "{line:?}"
    );
    let sans = text::layout_line(Face::Sans, "il", 20.0);
    let mono = text::layout_line(Face::Mono, "il", 20.0);
    assert!(sans.width < mono.width);
    let img = text::raster_line(Face::Mono, "8", 24.0, [255, 255, 255, 255]);
    let eight = text::layout_line(Face::Mono, "8", 24.0);
    assert_eq!(
        (img.w, img.h),
        (eight.width.ceil() as u32, eight.height.ceil() as u32)
    );
    let inked = img.px.chunks_exact(4).filter(|p| p[3] > 0.5).count();
    assert!(inked > 20, "the glyph inks: {inked}");
    assert!(img.px.chunks_exact(4).all(|p| p[0] == p[3] && p[1] == p[3]));
    assert!(text::PLEX_SANS.len() > 100_000 && text::PLEX_MONO.len() > 100_000);
}
