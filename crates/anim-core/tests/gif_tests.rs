//! AU8: the GIF writer, read back through a test decoder (`support/gif_decode.rs`):
//! every file the writer makes has to decode whole, with its frame count, its
//! delays and its first and last frame exact.

use anim_core::deliver::{self, BurnIns, Codec, Format, PRESETS, Preset, Purpose};
use anim_core::fx::Image;
use anim_core::gif::{self, GifWriter};

#[path = "support/gif_decode.rs"]
mod gif_decode;
use gif_decode::{Gif, decode};

/// `w × h` straight RGBA, opaque, of one colour.
fn flat(w: u32, h: u32, c: [u8; 3]) -> Vec<u8> {
    [c[0], c[1], c[2], 255].repeat((w * h) as usize)
}

/// The file the writer makes of these frames, every one of them sampled.
fn written(w: u32, h: u32, frames: &[(Vec<u8>, u32)]) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.gif");
    let mut g = GifWriter::new(&path, w, h).unwrap();
    for (rgba, _) in frames {
        g.sample(rgba).unwrap();
    }
    for (rgba, cs) in frames {
        g.push_frame(rgba, *cs).unwrap();
    }
    g.finish().unwrap();
    std::fs::read(&path).unwrap()
}

fn decoded(w: u32, h: u32, frames: &[(Vec<u8>, u32)]) -> Gif {
    decode(&written(w, h, frames)).unwrap()
}

/// What `rgba` looks like as the decoder gives it back, when no colour moved.
fn rgb_of(rgba: &[u8]) -> Vec<[u8; 3]> {
    rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect()
}

/// Delays of a shot of `frames` frames at `fps`, as a job asks for them.
fn delays(frames: u32, fps: u32) -> Vec<u32> {
    (0..frames).map(|i| gif::delay_cs(i, fps)).collect()
}

/// round(frames × 100 / fps), half up.
fn running_time_cs(frames: u32, fps: u32) -> u32 {
    (frames as f64 * 100.0 / fps as f64 + 0.5).floor() as u32
}

/// The images a shot of all-different frames leaves, each its frame and its
/// delay: a new image once the one in hand has had the floor, the first frame
/// always, and a file that ends on the last frame.
fn thinned(delays: &[u32]) -> Vec<(u32, u32)> {
    let floor = gif::MIN_DELAY_CS;
    let mut images: Vec<(u32, u32)> = Vec::new();
    for (i, &cs) in delays.iter().enumerate() {
        match images.last_mut() {
            Some((_, time)) if *time < floor => *time += cs,
            _ => images.push((i as u32, cs)),
        }
    }
    let last = delays.len() as u32 - 1;
    if images.len() == 1 && last > 0 {
        let time = images[0].1;
        images = vec![(0, floor), (last, time.saturating_sub(floor))];
    } else {
        images.last_mut().unwrap().0 = last;
    }
    images.into_iter().map(|(f, cs)| (f, cs.max(floor))).collect()
}

#[test]
fn augif_header_and_loop_block() {
    let bytes = written(4, 3, &[(flat(4, 3, [10, 20, 30]), 5), (flat(4, 3, [200, 100, 50]), 7)]);
    assert_eq!(&bytes[..6], b"GIF89a");
    assert_eq!(&bytes[6..10], &[4, 0, 3, 0], "width and height, little end first");
    assert_eq!(bytes[10] & 0x80, 0x80, "the one global table");
    assert_eq!(&bytes[11..13], &[0, 0], "background 0, no aspect");
    let table = 3usize << ((bytes[10] & 7) + 1);
    // Loops forever, straight after the table, before any image.
    let block = [&[0x21, 0xFF, 11][..], b"NETSCAPE2.0", &[3, 1, 0, 0, 0]].concat();
    assert_eq!(&bytes[13 + table..13 + table + block.len()], &block[..]);
    assert_eq!(bytes.last(), Some(&0x3B), "the trailer ends the file");
    let g = decode(&bytes).unwrap();
    assert_eq!(g.size, (4, 3));
    assert_eq!(g.loops, Some(0), "forever");
    assert_eq!(g.delays(), [5, 7]);
}

#[test]
fn augif_delays_follow_holds() {
    // The running remainder: each frame takes round(cum × 100 / fps) - prev.
    assert_eq!(delays(12, 12), [8, 9, 8, 8, 9, 8, 8, 9, 8, 8, 9, 8]);
    assert_eq!(delays(6, 24), [4, 4, 5, 4, 4, 4]);
    assert_eq!(delays(3, 25), [4, 4, 4]);
    assert_eq!(delays(4, 1), [100, 100, 100, 100]);
    for fps in [1, 6, 7, 8, 12, 15, 23, 24, 25, 29, 30, 48, 50] {
        for n in [1, 2, 11, 12, 100, 1001] {
            assert_eq!(
                delays(n, fps).iter().sum::<u32>(),
                running_time_cs(n, fps),
                "{n} frames at {fps} fps never drift"
            );
        }
    }
    // A shot of 12 frames at 12 fps: three drawings, held 3, 2 and 7.
    let (a, b, c) = (flat(5, 5, [255, 0, 0]), flat(5, 5, [0, 255, 0]), flat(5, 5, [0, 0, 255]));
    let shot: Vec<(Vec<u8>, u32)> = [&a, &a, &a, &b, &b, &c, &c, &c, &c, &c, &c, &c]
        .iter()
        .enumerate()
        .map(|(i, px)| ((*px).clone(), gif::delay_cs(i as u32, 12)))
        .collect();
    let g = decoded(5, 5, &shot);
    assert_eq!(g.delays(), [25, 17, 58], "each image lasts its hold");
    assert_eq!(g.duration_cs(), 100, "1.0 s");
    assert_eq!(g.frames[0].rgb, rgb_of(&a), "the first frame, exact");
    assert_eq!(g.frames[1].rgb, rgb_of(&b));
    assert_eq!(g.frames[2].rgb, rgb_of(&c), "the last frame, exact");
}

#[test]
fn augif_merges_identical_neighbours() {
    let (a, b) = (flat(6, 2, [40, 80, 120]), flat(6, 2, [220, 30, 10]));
    let g = decoded(
        6,
        2,
        &[(a.clone(), 4), (a.clone(), 4), (b.clone(), 5), (a.clone(), 4), (a.clone(), 9)],
    );
    assert_eq!(g.delays(), [8, 5, 13], "neighbours merge, an A after a B does not");
    assert_eq!(g.frames[0].rgb, rgb_of(&a));
    assert_eq!(g.frames[1].rgb, rgb_of(&b));
    assert_eq!(g.frames[2].rgb, rgb_of(&a));
    // A still: one image, however long the shot.
    let still = vec![(a.clone(), 4); 50];
    let g = decoded(6, 2, &still);
    assert_eq!(g.delays(), [200]);
}

#[test]
fn augif_flat_colours_decode_exactly() {
    // 256 colours in 256 cells of the histogram, none on a cell's corner, so
    // the palette holds every one and a rounded mean would show.
    let colour = |i: u32| {
        [
            (i & 7) * 36 + 3,
            ((i >> 3) & 7) * 36 + 5,
            ((i >> 6) & 3) * 80 + 7,
        ]
        .map(|v| v as u8)
    };
    let shot: Vec<(Vec<u8>, u32)> = (0..256).map(|i| (flat(3, 2, colour(i)), 4)).collect();
    let g = decoded(3, 2, &shot);
    assert_eq!(g.frames.len(), 256);
    assert_eq!(g.palette.len(), 256);
    for (i, f) in g.frames.iter().enumerate() {
        assert_eq!(f.rgb, vec![colour(i as u32); 6], "frame {i}");
    }
    // Ink and paper are exact too, in a two-colour picture with both.
    let mut page = flat(4, 4, [255, 255, 255]);
    page[20..24].copy_from_slice(&[0, 0, 0, 255]);
    let g = decoded(4, 4, &[(page.clone(), 10)]);
    assert_eq!(g.frames[0].rgb, rgb_of(&page));
}

#[test]
fn augif_palette_stays_under_257() {
    // Far more colours than a GIF can hold: every cell of a colour cube.
    let (w, h) = (256u32, 256u32);
    let mut rich = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rich.extend_from_slice(&[x as u8, y as u8, (x ^ y) as u8, 255]);
        }
    }
    let g = decoded(w, h, &[(rich.clone(), 10)]);
    assert!(g.palette.len() <= 256, "{}", g.palette.len());
    assert!(g.palette.len() > 128, "the room was used: {}", g.palette.len());
    assert!(g.palette.len().is_power_of_two(), "{}", g.palette.len());
    let used = g.frames[0].indices.iter().map(|&i| i as usize).max().unwrap() + 1;
    assert!(used <= g.palette.len());
    assert!(used > 200, "median cut spent the palette: {used}");
    // It is the picture still: each pixel lands near itself.
    let want = rgb_of(&rich);
    let errs: Vec<u32> = g.frames[0]
        .rgb
        .iter()
        .zip(&want)
        .flat_map(|(a, b)| (0..3).map(move |c| (a[c] as i32 - b[c] as i32).unsigned_abs()))
        .collect();
    let mean = errs.iter().sum::<u32>() as f64 / errs.len() as f64;
    assert!(mean < 24.0, "mean channel error {mean}");
    // Few colours use a small table: a one-colour film is not 768 bytes of it.
    assert_eq!(decoded(2, 2, &[(flat(2, 2, [9, 9, 9]), 4)]).palette.len(), 2);
    let two = decoded(2, 2, &[(flat(2, 2, [9, 9, 9]), 4), (flat(2, 2, [200, 9, 9]), 4)]);
    assert_eq!(two.palette.len(), 2);
    let five: Vec<(Vec<u8>, u32)> =
        (0..5u8).map(|i| (flat(2, 2, [i * 50, 0, 0]), 4)).collect();
    assert_eq!(decoded(2, 2, &five).palette.len(), 8, "a power of two");
}

#[test]
fn augif_sub_blocks_stay_under_256() {
    // Noise, so the LZW data runs long.
    let (w, h) = (128u32, 128u32);
    let mut seed = 0x2F6E_2B1Du32;
    let mut noise = Vec::new();
    for _ in 0..w * h {
        seed = seed.wrapping_mul(0x9E37_79B1).wrapping_add(0x7F4A_7C15);
        noise.extend_from_slice(&[(seed >> 24) as u8, (seed >> 16) as u8, (seed >> 8) as u8, 255]);
    }
    let g = decoded(w, h, &[(noise, 10)]);
    let blocks = &g.sub_blocks[0];
    assert!(blocks.len() > 3, "long enough to test: {} blocks", blocks.len());
    assert!(blocks.iter().all(|&n| (1..=255).contains(&n)), "{blocks:?}");
    let (last, full) = blocks.split_last().unwrap();
    assert!(full.iter().all(|&n| n == 255), "only the last block runs short");
    assert!(*last <= 255);
}

#[test]
fn augif_presets_are_append_only() {
    let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
    // Jobs hold an index: everything before the two stays where it was.
    assert_eq!(
        names[..11],
        [
            "Master PNG 16-bit",
            "TIFF 16-bit",
            "EXR half (linear)",
            "PNG 8-bit",
            "Line test PNG",
            "H.264 review",
            "H.265",
            "ProRes 422 HQ",
            "ProRes 4444 (alpha)",
            "DNxHR HQ",
            "Line test",
        ]
    );
    assert_eq!(names[11..], ["GIF", "Still PNG"]);
    let gif = Preset::named("GIF").unwrap();
    assert_eq!(
        (gif.format, gif.alpha, gif.burn_ins, gif.purpose, gif.one_file),
        (Format::Gif, false, BurnIns::NONE, Purpose::View, false),
        "flattened on white, nothing burnt in"
    );
    let still = Preset::named("Still PNG").unwrap();
    assert_eq!(
        (still.format, still.alpha, still.burn_ins, still.purpose, still.one_file),
        (Format::Png8, false, BurnIns::NONE, Purpose::View, true)
    );
    let one_file: Vec<&str> = PRESETS.iter().filter(|p| p.one_file).map(|p| p.name).collect();
    assert_eq!(one_file, ["Still PNG"], "the builder's default is off");
    assert!(PRESETS.iter().all(|p| p.scale == 1.0), "the builder's scale stands");
}

#[test]
fn augif_format_is_one_file_with_no_ffmpeg() {
    assert_eq!(Format::Gif.ext(), "gif");
    assert!(Format::Gif.is_video(), "one file: delivery counts do not move");
    assert!(!Format::Gif.needs_ffmpeg());
    assert!(Format::Video(Codec::H264).is_video() && Format::Video(Codec::H264).needs_ffmpeg());
    for f in [Format::Png16, Format::Tiff16, Format::Exr, Format::Png8] {
        assert!(!f.is_video() && !f.needs_ffmpeg(), "{f:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.gif");
    let img = Image::new(2, 2);
    assert_eq!(
        deliver::write_frame(&path, &img, Format::Gif, anim_core::color::ColorLaw::Srgb),
        Err("a GIF frame goes to the GIF writer".to_string())
    );
    assert!(!path.exists());
}

#[test]
fn augif_samples_are_evenly_spaced() {
    assert_eq!(gif::sample_frames(0, 0), [0]);
    assert_eq!(gif::sample_frames(7, 7), [7], "a one-frame range");
    assert_eq!(gif::sample_frames(3, 6), [3, 4, 5, 6], "a short shot, every frame");
    assert_eq!(gif::sample_frames(0, 15).len(), 16);
    let long = gif::sample_frames(10, 4809);
    assert_eq!(long.len(), gif::PALETTE_FRAMES as usize, "a long shot stops at 16");
    assert_eq!(gif::PALETTE_FRAMES, 16);
    assert_eq!((long[0], long[15]), (10, 4809), "the first and last frames are in it");
    let steps: Vec<u32> = long.windows(2).map(|w| w[1] - w[0]).collect();
    let (lo, hi) = (*steps.iter().min().unwrap(), *steps.iter().max().unwrap());
    assert!(hi - lo <= 1, "even: {steps:?}");
    // No overflow at the top of the range.
    assert_eq!(gif::sample_frames(0, u32::MAX)[15], u32::MAX);
}

#[test]
fn augif_a_frame_outside_the_sample_goes_to_its_nearest_colour() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.gif");
    let (red, blue) = ([250, 10, 10], [10, 10, 250]);
    let mut g = GifWriter::new(&path, 2, 1).unwrap();
    g.sample(&flat(2, 1, red)).unwrap();
    g.sample(&flat(2, 1, blue)).unwrap();
    g.push_frame(&flat(2, 1, red), 10).unwrap();
    g.push_frame(&flat(2, 1, [230, 40, 30]), 10).unwrap(); // reddish, not sampled
    g.push_frame(&flat(2, 1, [30, 40, 230]), 10).unwrap(); // bluish, not sampled
    g.finish().unwrap();
    let got = decode(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(got.delays(), [20, 10], "the reddish frame reads as the red one");
    assert_eq!(got.frames[0].rgb, [red; 2]);
    assert_eq!(got.frames[1].rgb, [blue; 2]);
    // With no sample the first frame is the sample: the writer still works.
    let bare = dir.path().join("bare.gif");
    let mut g = GifWriter::new(&bare, 2, 1).unwrap();
    g.push_frame(&flat(2, 1, red), 10).unwrap();
    g.finish().unwrap();
    let got = decode(&std::fs::read(&bare).unwrap()).unwrap();
    assert_eq!(got.frames[0].rgb, [red; 2]);
}

#[test]
fn augif_a_long_hold_fits_the_delay_field() {
    // A delay is 16 bits of hundredths: 70,000 of them need two images.
    let a = flat(3, 3, [12, 34, 56]);
    let g = decoded(3, 3, &[(a.clone(), 70_000)]);
    assert_eq!(g.delays(), [65_535, 4_465]);
    assert_eq!(g.frames[0].rgb, g.frames[1].rgb, "the same picture held on");
    assert_eq!(g.duration_cs(), 70_000);
}

#[test]
fn augif_a_fast_shot_keeps_its_running_time() {
    // The formula alone gives delays of 0 and 1 past 50 fps, and viewers run
    // those at 10 cs. So a picture stays up at least 2 cs, the first frame is
    // still the first, the last still the last, and the shot's running time
    // stands: only the last picture can have banked less than the floor, so
    // at most one floor over.
    let colour = |i: u32| [(i * 37 % 256) as u8, (i * 91 % 256) as u8, (i * 53 % 256) as u8];
    let fast = |fps: u32| -> Gif {
        let n = fps * 2;
        let shot: Vec<(Vec<u8>, u32)> =
            (0..n).map(|i| (flat(2, 2, colour(i)), gif::delay_cs(i, fps))).collect();
        decoded(2, 2, &shot)
    };
    for fps in [51, 60, 100, 101, 120, 240] {
        let n = fps * 2;
        let g = fast(fps);
        let over = g.duration_cs() - 200;
        assert!(over <= gif::MIN_DELAY_CS, "2.0 s at {fps} fps ran {over} cs over");
        assert!(g.delays().iter().all(|&d| d >= gif::MIN_DELAY_CS as u16), "{fps}: {:?}", g.delays());
        assert_eq!(g.frames[0].rgb[0], colour(0), "{fps}");
        assert_eq!(g.frames.last().unwrap().rgb[0], colour(n - 1), "{fps}: the last frame, exact");
    }
    assert_eq!(gif::MIN_DELAY_CS, 2);
    // A GIF holds at most 50 pictures a second, so past 50 fps the count is
    // the floor's and not the shot's. It is exact all the same.
    assert_eq!(fast(60).delays(), [2, 3].repeat(40), "80 pictures of 120 frames");
    assert_eq!(fast(100).delays(), [2; 100], "100 of 200");
    assert_eq!(fast(120).delays(), [2; 100], "100 of 240");
    // At 50 fps and under nothing is thinned: every drawing shows, and the
    // running time is exact.
    let shot: Vec<(Vec<u8>, u32)> = (0..100).map(|i| (flat(2, 2, colour(i)), gif::delay_cs(i, 50))).collect();
    let g = decoded(2, 2, &shot);
    assert_eq!(g.frames.len(), 100);
    assert_eq!(g.duration_cs(), 200);
    assert_eq!(g.frames[99].rgb[0], colour(99), "the last frame, exact");
    // One frame too fast to have a delay at all is still a whole file.
    assert_eq!(gif::delay_cs(0, 240), 0);
    let g = decoded(2, 2, &[(flat(2, 2, [1, 2, 3]), 0)]);
    assert_eq!(g.delays(), [2]);
}

#[test]
fn augif_the_last_frame_is_never_thinned_away() {
    let (a, b, c) = (flat(2, 2, [200, 0, 0]), flat(2, 2, [0, 200, 0]), flat(2, 2, [0, 0, 200]));
    let shown = |g: &Gif| -> Vec<[u8; 3]> { g.frames.iter().map(|f| f.rgb[0]).collect() };
    let (ra, rb, rc) = ([200, 0, 0], [0, 200, 0], [0, 0, 200]);
    // 60 fps: B has 1 cs, so C comes too soon. C takes B's place and its time.
    let g = decoded(2, 2, &[(a.clone(), 2), (b.clone(), 1), (c.clone(), 2)]);
    assert_eq!((shown(&g), g.delays()), (vec![ra, rc], vec![2, 3]));
    // The picture before it is the same one: one image, no two alike side by side.
    let g = decoded(2, 2, &[(a.clone(), 2), (b.clone(), 1), (a.clone(), 2)]);
    assert_eq!((shown(&g), g.delays()), (vec![ra], vec![5]));
    // The picture in hand is the first frame, so both stay. Two pictures cannot
    // run under twice the floor, however short the shot.
    let g = decoded(2, 2, &[(a.clone(), 1), (b.clone(), 1)]);
    assert_eq!((shown(&g), g.delays()), (vec![ra, rb], vec![2, 2]));
    let g = decoded(2, 2, &[(a.clone(), 0), (b.clone(), 1)]);
    assert_eq!((shown(&g), g.delays()), (vec![ra, rb], vec![2, 2]), "two frames at 240 fps");
    let g = decoded(2, 2, &[(a.clone(), 1), (b.clone(), 4)]);
    assert_eq!((shown(&g), g.delays()), (vec![ra, rb], vec![2, 3]), "the time stands");
    // It came back to the picture in hand: that is the last frame, and nothing
    // is owed. Too soon in the middle of a shot, a frame just has no image.
    let g = decoded(2, 2, &[(a.clone(), 1), (b.clone(), 1), (a.clone(), 1)]);
    assert_eq!((shown(&g), g.delays()), (vec![ra], vec![3]));
    let g = decoded(2, 2, &[(a.clone(), 2), (b.clone(), 1), (c.clone(), 2), (a.clone(), 2)]);
    assert_eq!((shown(&g), g.delays()), (vec![ra, rb, ra], vec![2, 3, 2]));
    // A hold stays a hold past 50 fps: 120 frames at 120 fps, three drawings.
    let drawings: Vec<(Vec<u8>, u32)> = (0..120)
        .map(|i| ([&a, &b, &c][i as usize / 40].clone(), gif::delay_cs(i, 120)))
        .collect();
    let g = decoded(2, 2, &drawings);
    assert_eq!((shown(&g), g.delays()), (vec![ra, rb, rc], vec![33, 34, 33]));
}

/// Every rate the New Project form offers, at the lengths that catch a shot's
/// two ends. At 50 fps and under every frame gets its image and its delay. Past
/// that the file holds what the floor lets it, and always both ends.
#[test]
fn augif_every_speed_keeps_its_first_and_last_frame() {
    let colour = |i: u32| {
        [
            (i & 7) * 36 + 3,
            ((i >> 3) & 7) * 36 + 5,
            ((i >> 6) & 3) * 80 + 7,
        ]
        .map(|v| v as u8)
    };
    let check = |n: u32, fps: u32| {
        let times = delays(n, fps);
        let shot: Vec<(Vec<u8>, u32)> = (0..n).map(|i| (flat(1, 1, colour(i)), times[i as usize])).collect();
        let g = decoded(1, 1, &shot);
        let at = format!("{n} frames at {fps} fps");
        let want = thinned(&times);
        let shown: Vec<[u8; 3]> = g.frames.iter().map(|f| f.rgb[0]).collect();
        assert_eq!(shown, want.iter().map(|w| colour(w.0)).collect::<Vec<_>>(), "{at}");
        assert_eq!(g.delays(), want.iter().map(|w| w.1 as u16).collect::<Vec<_>>(), "{at}");
        assert_eq!((shown[0], shown[shown.len() - 1]), (colour(0), colour(n - 1)), "{at}");
        if fps <= 50 {
            assert_eq!(g.frames.len(), n as usize, "{at}: nothing thinned");
            assert_eq!(g.delays(), times.iter().map(|&t| t as u16).collect::<Vec<_>>(), "{at}");
            assert_eq!(g.duration_cs(), running_time_cs(n, fps), "{at}");
        }
        let (floor, images) = (gif::MIN_DELAY_CS, g.frames.len() as u32);
        assert!(g.delays().iter().all(|&d| d as u32 >= floor), "{at}: {:?}", g.delays());
        let ceiling = (running_time_cs(n, fps) + floor).max(images.min(2) * floor);
        assert!(g.duration_cs() <= ceiling, "{at} ran {}, ceiling {ceiling}", g.duration_cs());
    };
    for fps in 1..=240 {
        for n in 1..=7 {
            check(n, fps);
        }
    }
    let rates = [12, 24, 30, 48, 50, 51, 59, 60, 61, 66, 67, 75, 99, 100, 101, 119, 120, 121, 144, 199, 200, 201, 240];
    for fps in rates {
        for n in [fps - 1, fps, fps + 1, fps * 2 + 1] {
            check(n, fps);
        }
    }
}

#[test]
fn augif_flattens_on_white() {
    // Half ink, no ink, whole ink: a straight RGBA frame of each.
    let px = [
        [0, 0, 0, 128],
        [90, 10, 200, 0],
        [40, 50, 60, 255],
        [255, 0, 0, 51],
    ];
    let rgba: Vec<u8> = px.iter().flatten().copied().collect();
    let g = decoded(4, 1, &[(rgba, 10)]);
    assert_eq!(
        g.frames[0].rgb,
        [[127, 127, 127], [255, 255, 255], [40, 50, 60], [255, 204, 204]],
    );
}

#[test]
fn augif_refuses_what_a_gif_cannot_hold() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.gif");
    for (w, h) in [(0, 10), (10, 0), (65_536, 10), (10, 65_536)] {
        let e = GifWriter::new(&path, w, h).err().unwrap_or_else(|| panic!("{w}×{h} refused"));
        assert!(e.contains(&format!("{w}×{h}")), "{e}");
    }
    let mut g = GifWriter::new(&path, 2, 2).unwrap();
    assert!(g.sample(&[0; 8]).is_err(), "a frame of the wrong size");
    assert!(g.push_frame(&[0; 12], 4).is_err(), "a frame of the wrong size");
    let e = g.finish().unwrap_err();
    assert!(e.contains("no frame"), "{e}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "and no file");
}

#[test]
fn augif_the_file_lands_whole_or_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.gif");
    let names = || -> Vec<String> {
        std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    };
    // Stopped half way: an older export of that name stays as it was.
    std::fs::write(&path, b"yesterday").unwrap();
    let mut g = GifWriter::new(&path, 2, 2).unwrap();
    g.push_frame(&flat(2, 2, [1, 2, 3]), 4).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"yesterday", "nothing lands early");
    drop(g);
    assert_eq!(std::fs::read(&path).unwrap(), b"yesterday");
    assert_eq!(names(), ["t.gif"], "and the part file is gone");
    // Finished: the new file replaces it, with nothing left beside it.
    let mut g = GifWriter::new(&path, 2, 2).unwrap();
    g.push_frame(&flat(2, 2, [1, 2, 3]), 4).unwrap();
    g.finish().unwrap();
    assert_eq!(decode(&std::fs::read(&path).unwrap()).unwrap().frames.len(), 1);
    assert_eq!(names(), ["t.gif"]);
    // A folder that does not exist is said, not panicked.
    let e = GifWriter::new(&dir.path().join("no/such/t.gif"), 2, 2).err().unwrap();
    assert!(e.contains("t.gif"), "{e}");
}

/// The decoder has to be able to fail, or none of the above means much.
#[test]
fn augif_the_test_decoder_refuses_a_damaged_file() {
    let good = written(4, 4, &[(flat(4, 4, [10, 20, 30]), 5), (flat(4, 4, [200, 20, 30]), 5)]);
    assert!(decode(&good).is_ok());
    let mut extra = good.clone();
    extra.push(0);
    assert!(decode(&extra).is_err(), "bytes after the trailer");
    assert!(decode(&good[..good.len() - 1]).is_err(), "no trailer");
    let mut signature = good.clone();
    signature[4] = b'7';
    assert!(decode(&signature).is_err(), "not GIF89a");
    // Each block in turn: cut one short and it must be noticed.
    for n in 1..good.len() {
        assert!(decode(&good[..n]).is_err(), "cut at byte {n}");
    }
}
