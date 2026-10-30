//! GM16: the generators' laws — deterministic analytic particles and their
//! presets, speed and focus lines, horizontal and vertical lettering, speech
//! balloons — and Generator nodes in the graph, the store and the eval hash.
//! Headless and pure: no window, GPU, network or child process; the store
//! test writes only under the temp dir.

use anim_core::Engine;
use anim_core::color::ColorLaw;
use anim_core::command::{Command, CutRef};
use anim_core::export::{graph_composite_pixel, render_node_px, straight_rgba8};
use anim_core::fx::{FxBinds, FxParams, FxValue, Image, ParamKind};
use anim_core::generator::{self, GenKind, GenValues, Particle, Preset, SHAPES, Target, focus_lines, particles_at, speed_lines};
use anim_core::graph::NodeKind;
use anim_core::ids::*;
use anim_core::text::{self, Direction, Face, Form, Style};
use anim_core::xsheet::{ParamInterp, ParamKey};

const LAW: ColorLaw = ColorLaw::Srgb;

fn n(x: f32) -> FxValue {
    FxValue::Float(x)
}

fn pick(s: &str) -> FxValue {
    FxValue::Choice(s.to_string())
}

fn words(s: &str) -> FxValue {
    FxValue::Text { text: s.to_string() }
}

fn params(kind: GenKind, set: &[(&str, FxValue)]) -> FxParams {
    let mut p = kind.defaults();
    for (k, v) in set {
        assert!(kind.spec(k).is_some(), "{} has no '{k}'", kind.name());
        p.set(k, v.clone());
    }
    p
}

fn vals(kind: GenKind, set: &[(&str, FxValue)]) -> GenValues {
    GenValues::of(kind, &params(kind, set))
}

fn draw(v: &GenValues, frame: u32, w: u32, h: u32) -> Image {
    generator::render(v, LAW, None, frame, Target::frame(w, h))
}

fn inked(img: &Image) -> usize {
    img.px.chunks_exact(4).filter(|p| p[3] > 0.0).count()
}

fn rgba8(img: &Image, x: u32, y: u32) -> [u8; 4] {
    let i = (y as usize * img.w as usize + x as usize) * 4;
    straight_rgba8(&img.px[i..i + 4])
}

/// The ink's bounding box (x0, y0, x1, y1), inclusive.
fn ink_box(img: &Image) -> Option<(u32, u32, u32, u32)> {
    let mut b: Option<(u32, u32, u32, u32)> = None;
    for y in 0..img.h {
        for x in 0..img.w {
            if img.pixel(x, y)[3] > 0.0 {
                b = Some(match b {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
    }
    b
}

/// FNV-1a over the straight RGBA8 bytes and the size.
fn fnv(img: &Image) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    for b in img.w.to_le_bytes().into_iter().chain(img.h.to_le_bytes()) {
        eat(b);
    }
    for p in img.px.chunks_exact(4) {
        for b in straight_rgba8(p) {
            eat(b);
        }
    }
    h
}

/// Births alive at `f` with no lifetime variance: within (f − L, f].
fn alive(f: f32, rate: f32, life: f32, pre: f32) -> usize {
    let hi = ((f + pre) * rate).floor();
    let lo = ((f + pre - life) * rate).floor().max(-1.0);
    (hi - lo) as usize
}

// ---- Particles --------------------------------------------------------------

#[test]
fn particles_draw_alike_for_one_seed_and_frame_and_a_new_seed_moves_them() {
    let set = [("rate", n(6.0)), ("spread", n(360.0)), ("turbulence", n(8.0))];
    let v = vals(GenKind::Particles, &set);
    let a = draw(&v, 30, 160, 90);
    assert!(inked(&a) > 50, "the default emitter draws: {} px", inked(&a));
    assert_eq!(a, draw(&v, 30, 160, 90), "same seed and frame, same pixels");
    let ps = particles_at(&v, LAW, 30, 160, 90, None);
    assert_eq!(ps, particles_at(&v, LAW, 30, 160, 90, None));
    let mut other = set.to_vec();
    other.push(("seed", n(2.0)));
    let w = vals(GenKind::Particles, &other);
    assert_ne!(a, draw(&w, 30, 160, 90), "a new seed is a new pattern");
}

#[test]
fn a_particle_frame_renders_alone_as_it_does_after_its_predecessor() {
    let r = rig(NodeKind::particles(Preset::Snow));
    let cut = r.cut();
    let alone = render_node_px(cut, LAW, r.node, 37, 160, 90);
    let mut played = Vec::new();
    for f in 30..=37 {
        played = render_node_px(cut, LAW, r.node, f, 160, 90);
    }
    assert_eq!(alone, played, "no state carries from frame to frame");
    assert!(alone.chunks_exact(4).any(|p| p[3] > 0.0));
    assert_ne!(alone, render_node_px(cut, LAW, r.node, 36, 160, 90), "snow moves");
    // The particle list too, scrubbing backward.
    let v = GenValues::of(GenKind::Particles, &Preset::Snow.params());
    let at37 = particles_at(&v, LAW, 37, 160, 90, None);
    let _ = particles_at(&v, LAW, 90, 160, 90, None);
    assert_eq!(at37, particles_at(&v, LAW, 37, 160, 90, None));
}

#[test]
fn the_alive_count_is_the_births_within_the_lifetime() {
    for pre in [0.0, 5.0] {
        let v = vals(
            GenKind::Particles,
            &[("rate", n(4.0)), ("lifetime", n(10.0)), ("lifetime_var", n(0.0)), ("preroll", n(pre))],
        );
        for f in 0..40u32 {
            let got = particles_at(&v, LAW, f, 320, 180, None).len();
            assert_eq!(got, alive(f as f32, 4.0, 10.0, pre), "frame {f}, preroll {pre}");
        }
    }
    assert_eq!(alive(0.0, 4.0, 10.0, 0.0), 1, "frame 0 holds the first birth");
    assert_eq!(alive(20.0, 4.0, 10.0, 0.0), 40, "full: rate × lifetime");
    // With variance every live particle is within its own lifetime, and
    // the count lies between the shortest and longest lives' counts.
    let v = vals(GenKind::Particles, &[("rate", n(4.0)), ("lifetime", n(10.0)), ("lifetime_var", n(0.5))]);
    let ps = particles_at(&v, LAW, 30, 320, 180, None);
    assert!(ps.iter().all(|p| 0.0 <= p.age && p.age < p.life && (5.0..=15.0).contains(&p.life)));
    let n_alive = ps.len();
    assert!((alive(30.0, 4.0, 5.0, 0.0)..=alive(30.0, 4.0, 15.0, 0.0)).contains(&n_alive), "{n_alive}");
}

#[test]
fn gravity_moves_the_centroid_down_over_life() {
    let set = |g: f32| {
        vals(
            GenKind::Particles,
            &[
                ("rate", n(5.0)),
                ("lifetime", n(40.0)),
                ("lifetime_var", n(0.0)),
                ("speed", n(1.0)),
                ("speed_var", n(0.0)),
                ("spread", n(360.0)),
                ("gravity", n(g)),
            ],
        )
    };
    let ps = particles_at(&set(0.25), LAW, 100, 400, 400, None);
    assert_eq!(ps.len(), 200);
    let mean_y = |ps: &[Particle], keep: &dyn Fn(&Particle) -> bool| {
        let ys: Vec<f32> = ps.iter().filter(|p| keep(p)).map(|p| p.y).collect();
        ys.iter().sum::<f32>() / ys.len() as f32
    };
    let young = mean_y(&ps, &|p| p.age < 20.0);
    let old = mean_y(&ps, &|p| p.age >= 20.0);
    assert!(old > young + 50.0, "old {old} sits below young {young}");
    // With no gravity the cloud stays centred on the emitter.
    let still = particles_at(&set(0.0), LAW, 100, 400, 400, None);
    let all = mean_y(&still, &|_| true);
    assert!((all - 200.0).abs() < 5.0, "{all}");
}

#[test]
fn every_preset_has_valid_parameters_and_draws_inside_the_frame() {
    for preset in Preset::ALL {
        let p = preset.params();
        for (k, v) in &p.0 {
            let spec = GenKind::Particles.spec(k).unwrap_or_else(|| panic!("{}: no '{k}'", preset.name()));
            match (spec.kind, v) {
                (ParamKind::Float { min, max, .. }, FxValue::Float(x)) => {
                    assert!((min..=max).contains(x), "{}: {k} = {x} outside {min}..{max}", preset.name())
                }
                (ParamKind::Choice { options, .. }, FxValue::Choice(s)) => {
                    assert!(options.contains(&s.as_str()), "{}: {k} = {s}", preset.name())
                }
                (ParamKind::Color { .. }, FxValue::Color(_)) => {}
                (_, v) => panic!("{}: {k} has the wrong type {v:?}", preset.name()),
            }
        }
        let v = GenValues::of(GenKind::Particles, &p);
        let img = draw(&v, 48, 384, 216);
        assert!(inked(&img) > 30, "{} draws {} px", preset.name(), inked(&img));
    }
    assert_eq!(Preset::ALL.len(), 6);
}

#[test]
fn every_shape_draws_and_add_sums_light_where_normal_lays_over() {
    for shape in SHAPES {
        let v = vals(
            GenKind::Particles,
            &[("shape", pick(shape)), ("speed", n(3.0)), ("size", n(10.0)), ("size_end", n(10.0)), ("opacity_end", n(1.0))],
        );
        assert!(inked(&draw(&v, 20, 120, 120)) > 10, "{shape}");
    }
    // Two particles on one spot (births 0 and 1 at frame 1), each 50%.
    let one = |blend: &str| {
        let grey = FxValue::Color([100, 100, 100, 255]);
        vals(
            GenKind::Particles,
            &[
                ("rate", n(1.0)),
                ("lifetime", n(100.0)),
                ("lifetime_var", n(0.0)),
                ("speed", n(0.0)),
                ("gravity", n(0.0)),
                ("size", n(9.0)),
                ("size_end", n(9.0)),
                ("opacity", n(0.5)),
                ("opacity_end", n(0.5)),
                ("color", grey.clone()),
                ("color_end", grey),
                ("shape", pick("dot")),
                ("blend", pick(blend)),
            ],
        )
    };
    let c = 100.0 / 255.0;
    let normal = draw(&one("normal"), 1, 64, 64).pixel(32, 32);
    let add = draw(&one("add"), 1, 64, 64).pixel(32, 32);
    assert!((normal[0] - 0.75 * c).abs() < 1e-5 && (normal[3] - 0.75).abs() < 1e-5, "{normal:?}");
    assert!((add[0] - c).abs() < 1e-5 && (add[3] - 0.75).abs() < 1e-5, "{add:?}");
}

#[test]
fn the_matte_emitter_births_only_where_the_matte_is_opaque() {
    let mut matte = Image::new(200, 100);
    for y in 0..100 {
        for x in 0..100 {
            matte.set_pixel(x, y, [1.0; 4]);
        }
    }
    let v = vals(
        GenKind::Particles,
        &[("emitter", pick("matte")), ("rate", n(10.0)), ("lifetime", n(20.0)), ("lifetime_var", n(0.0)), ("speed", n(0.0)), ("gravity", n(0.0))],
    );
    let ps = particles_at(&v, LAW, 30, 200, 100, Some(&matte));
    assert!(ps.len() > 150, "most births find the matte: {}", ps.len());
    assert!(ps.iter().all(|p| p.x < 100.0), "none born on the clear half");
    assert!(particles_at(&v, LAW, 30, 200, 100, None).is_empty(), "no matte, no births");
    let img = generator::render(&v, LAW, Some(&matte), 30, Target::frame(200, 100));
    let (_, _, x1, _) = ink_box(&img).unwrap();
    assert!(x1 < 104, "ink stays on the matte (plus a particle's reach): {x1}");
}

// ---- Speed and focus lines ----------------------------------------------------

#[test]
fn speed_lines_run_within_half_a_degree_of_the_angle_and_their_count_is_the_density() {
    for angle in [0.0f32, 30.0, -60.0, 90.0, 137.5] {
        let v = vals(GenKind::SpeedLines, &[("angle", n(angle)), ("count", n(57.0))]);
        let lines = speed_lines(&v, 0, 320, 180);
        assert_eq!(lines.len(), 57);
        for l in &lines {
            let d = (l.angle_deg() - angle).rem_euclid(180.0);
            assert!(d.min(180.0 - d) < 0.5, "{angle}: a line at {}", l.angle_deg());
        }
    }
    // Measured from the pixels: a lone line wholly inside the frame (the
    // seeds whose stroke lands there) inks along the angle — its ink's
    // principal axis.
    let (w, h) = (320u32, 320u32);
    let mut measured = 0;
    for angle in [30.0f32, -60.0, 137.5] {
        for seed in 1..400 {
            let set = [
                ("angle", n(angle)),
                ("count", n(1.0)),
                ("length_min", n(0.3)),
                ("length_max", n(0.3)),
                ("thickness_min", n(3.0)),
                ("thickness_max", n(3.0)),
                ("seed", n(seed as f32)),
            ];
            let v = vals(GenKind::SpeedLines, &set);
            let l = speed_lines(&v, 0, w, h)[0];
            let inside = |p: (f32, f32)| p.0 > 4.0 && p.1 > 4.0 && p.0 < w as f32 - 4.0 && p.1 < h as f32 - 4.0;
            if !(inside(l.a) && inside(l.b)) {
                continue;
            }
            let img = draw(&v, 0, w, h);
            let (mut s, mut mx, mut my) = (0.0f64, 0.0f64, 0.0f64);
            for y in 0..h {
                for x in 0..w {
                    let a = img.pixel(x, y)[3] as f64;
                    (s, mx, my) = (s + a, mx + a * x as f64, my + a * y as f64);
                }
            }
            let (mx, my) = (mx / s, my / s);
            let (mut sxx, mut syy, mut sxy) = (0.0f64, 0.0f64, 0.0f64);
            for y in 0..h {
                for x in 0..w {
                    let a = img.pixel(x, y)[3] as f64;
                    let (dx, dy) = (x as f64 - mx, y as f64 - my);
                    (sxx, syy, sxy) = (sxx + a * dx * dx, syy + a * dy * dy, sxy + a * dx * dy);
                }
            }
            let axis = (0.5 * (2.0 * sxy).atan2(sxx - syy)).to_degrees() as f32;
            let d = (axis - angle).rem_euclid(180.0);
            assert!(d.min(180.0 - d) < 0.5, "{angle}, seed {seed}: the ink runs at {axis}°");
            measured += 1;
            break;
        }
    }
    assert_eq!(measured, 3, "a seed puts a whole line in the frame at every angle");
}

#[test]
fn a_new_seed_moves_the_lines_and_on_twos_holds_frame_pairs() {
    let (w, h) = (160, 90);
    let held = vals(GenKind::SpeedLines, &[]);
    let seed2 = vals(GenKind::SpeedLines, &[("seed", n(2.0))]);
    assert_ne!(draw(&held, 0, w, h), draw(&seed2, 0, w, h), "the seed decides the layout");
    assert_eq!(draw(&held, 0, w, h), draw(&held, 7, w, h), "held lines never move");
    for kind in [GenKind::SpeedLines, GenKind::FocusLines] {
        let twos = vals(kind, &[("motion", pick("on twos"))]);
        for k in 0..4 {
            let (a, b, c) = (draw(&twos, 2 * k, w, h), draw(&twos, 2 * k + 1, w, h), draw(&twos, 2 * k + 2, w, h));
            assert_eq!(a, b, "{}: frames {} and {} are one drawing", kind.name(), 2 * k, 2 * k + 1);
            assert_ne!(b, c, "{}: frame {} is a new drawing", kind.name(), 2 * k + 2);
        }
        let every = vals(kind, &[("motion", pick("every frame"))]);
        assert_ne!(draw(&every, 4, w, h), draw(&every, 5, w, h));
        // On threes (the satsuei look of a shūchūsen held on 3s): frames
        // 3k to 3k + 2 are one drawing.
        let threes = vals(kind, &[("motion", pick("on threes"))]);
        for k in 0..3 {
            let first = draw(&threes, 3 * k, w, h);
            for f in 3 * k + 1..3 * k + 3 {
                assert_eq!(draw(&threes, f, w, h), first, "{}: frame {f} holds frame {}", kind.name(), 3 * k);
            }
            assert_ne!(draw(&threes, 3 * k + 3, w, h), first, "{}: frame {} is new", kind.name(), 3 * k + 3);
        }
    }
}

#[test]
fn the_clear_band_keeps_its_rows_bare() {
    let v = vals(
        GenKind::SpeedLines,
        &[("angle", n(0.0)), ("count", n(300.0)), ("band", n(0.3)), ("band_offset", n(0.0)), ("thickness_max", n(6.0))],
    );
    let img = draw(&v, 0, 320, 180);
    // 0.3 of 180 rows about the middle: rows 63..117.
    for y in 63..117 {
        for x in 0..320 {
            assert_eq!(img.pixel(x, y)[3], 0.0, "ink in the band at ({x}, {y})");
        }
    }
    let above = (0..63).flat_map(|y| (0..320).map(move |x| (x, y))).filter(|&(x, y)| img.pixel(x, y)[3] > 0.0).count();
    let below = (117..180).flat_map(|y| (0..320).map(move |x| (x, y))).filter(|&(x, y)| img.pixel(x, y)[3] > 0.0).count();
    assert!(above > 1000 && below > 1000, "lines fill both sides: {above} / {below}");
}

#[test]
fn focus_lines_leave_the_ellipse_bare_and_point_at_its_centre() {
    let (w, h) = (320u32, 180u32);
    let v = vals(
        GenKind::FocusLines,
        &[("x", n(0.45)), ("y", n(0.55)), ("rx", n(0.2)), ("ry", n(0.15)), ("count", n(90.0)), ("jitter", n(0.2))],
    );
    let img = draw(&v, 0, w, h);
    let (cx, cy, rx, ry) = (0.45 * w as f32, 0.55 * h as f32, 0.2 * w as f32, 0.15 * h as f32);
    let (mut near, mut total) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let q = ((x as f32 + 0.5 - cx) / rx).powi(2) + ((y as f32 + 0.5 - cy) / ry).powi(2);
            let a = img.pixel(x, y)[3];
            if q < 1.0 {
                assert_eq!(a, 0.0, "ink inside the clear ellipse at ({x}, {y})");
            } else if a > 0.0 {
                total += 1;
                if q < 2.0 {
                    near += 1;
                }
            }
        }
    }
    assert!(near > 100 && total > 3000, "lines reach in toward the ellipse: {near} near, {total} in all");
    let lines = focus_lines(&v, 0, w, h);
    assert_eq!(lines.len(), 90, "the count is pinned");
    for l in &lines {
        let to_centre = (cy - l.a.1).atan2(cx - l.a.0).to_degrees();
        let d = (l.angle_deg() - to_centre).rem_euclid(360.0);
        assert!(d.min(360.0 - d) < 1.0, "a line {d}° off the centre");
        let (ex, ey) = ((l.b.0 - cx) / rx, (l.b.1 - cy) / ry);
        let q = (ex * ex + ey * ey).sqrt();
        assert!((1.0 - 1e-3..=1.2 + 1e-3).contains(&q), "each stops at the ellipse × (1 + jitter·u): {q}");
    }
}

// ---- Lettering ------------------------------------------------------------------

#[test]
fn horizontal_text_is_pinned_and_grows_with_the_string() {
    let gaban = vals(GenKind::Text, &[("text", words("Gaban")), ("size", n(48.0))]);
    let img = draw(&gaban, 0, 256, 96);
    assert_eq!(fnv(&img), GABAN_48, "\"Gaban\" at 48 px moved: {:#x}", fnv(&img));
    let mut last = 0;
    for s in ["Gab", "Gaban", "Gaban Studio", "Gaban Studio セル"] {
        let b = text::layout(s, &Style::new(Face::SansJp, 48.0, Direction::Horizontal));
        let ink = ink_box(&draw(&vals(GenKind::Text, &[("text", words(s))]), 0, 1024, 96)).unwrap();
        let width = ink.2 - ink.0;
        assert!(width > last, "{s}: {width} px after {last}");
        assert!((b.w - width as f32).abs() < 12.0, "{s}: the block ({}) holds the ink ({width})", b.w);
        last = width;
    }
}

/// The pinned FNV-1a of "Gaban", Plex Sans JP Bold 48 px, black, centred
/// in 256 × 96.
const GABAN_48: u64 = 0x25a5_1e54_eeb1_c340;

#[test]
fn vertical_text_sets_columns_down_then_right_to_left() {
    let em = 40.0;
    let st = Style::new(Face::SansJp, em, Direction::Vertical);
    let b = text::layout("あいう", &st);
    assert_eq!(b.cells.len(), 3);
    for pair in b.cells.windows(2) {
        let (p, q) = (pair[0].centre(), pair[1].centre());
        assert_eq!(p.0, q.0, "one column");
        assert!(q.1 > p.1 + 0.9 * em, "down the column");
        let (pi, qi) = (pair[0].ink.unwrap(), pair[1].ink.unwrap());
        let (pc, qc) = ((pi[0] + pi[2]) / 2.0, (qi[0] + qi[2]) / 2.0);
        assert!((pc - qc).abs() < 0.12 * em, "the glyphs' ink centres share x: {pc} / {qc}");
    }
    // Wrapping starts the next column, to the left, at the top.
    let wrapped = text::layout("あいう", &Style { wrap: 2.5 * em, ..st });
    let c: Vec<_> = wrapped.cells.iter().map(|c| c.centre()).collect();
    assert_eq!(c[0].0, c[1].0);
    assert!(c[2].0 < c[0].0 - 0.9 * em, "う moved left: {:?}", c);
    assert_eq!(c[2].1, c[0].1, "う starts the new column at the top");
    // So does a new line.
    let lines = text::layout("あ\nい", &st);
    assert!(lines.cells[1].centre().0 < lines.cells[0].centre().0);
}

#[test]
fn vertical_forms_turn_dashes_lift_commas_and_set_two_digits_in_one_cell() {
    let em = 40.0;
    let st = Style::new(Face::SansJp, em, Direction::Vertical);
    let b = text::layout("あ、", &st);
    let comma = &b.cells[1];
    assert_eq!(comma.form, Form::Corner);
    let (ink, (cx, cy)) = (comma.ink.unwrap(), comma.centre());
    let (ix, iy) = ((ink[0] + ink[2]) / 2.0, (ink[1] + ink[3]) / 2.0);
    assert!(ix > cx && iy < cy, "、 sits in the top-right quadrant: ink ({ix}, {iy}) in a cell centred ({cx}, {cy})");
    let bar = text::layout("ー", &st);
    assert_eq!(bar.cells[0].form, Form::Rotated);
    let ink = bar.cells[0].ink.unwrap();
    assert!(ink[3] - ink[1] > 2.0 * (ink[2] - ink[0]), "ー runs down the column: {ink:?}");
    let flat = text::layout("ー", &Style::new(Face::SansJp, em, Direction::Horizontal));
    let fi = flat.cells[0].ink.unwrap();
    assert!(fi[2] - fi[0] > fi[3] - fi[1], "and across a line");
    // Tate-chu-yoko: two digits share one cell, three take one each.
    let tcy = text::layout("12", &st);
    assert_eq!(tcy.cells.len(), 1);
    assert_eq!(tcy.cells[0].form, Form::Tcy);
    let ti = tcy.cells[0].ink.unwrap();
    assert!(ti[2] - ti[0] <= em, "set across within the em");
    assert_eq!(text::layout("第12話", &st).cells.len(), 3);
    assert_eq!(text::layout("123", &st).cells.len(), 3);
    // The vertical text draws through the generator.
    let v = vals(GenKind::Text, &[("text", words("あいう、ー12")), ("direction", pick("vertical"))]);
    let (x0, y0, x1, y1) = ink_box(&draw(&v, 0, 200, 400)).unwrap();
    assert!(y1 - y0 > 4 * (x1 - x0), "a tall column: {}×{}", x1 - x0, y1 - y0);
}

#[test]
fn vertical_brackets_and_the_ellipsis_turn_and_the_full_stop_sits_top_right() {
    let em = 40.0;
    let column = Style::new(Face::SansJp, em, Direction::Vertical);
    let line = Style::new(Face::SansJp, em, Direction::Horizontal);
    let size = |r: [f32; 4]| (r[2] - r[0], r[3] - r[1]);
    // The brackets a letterer sees first in every line of dialogue: turned
    // a quarter, so a column's ink is a line's on its side (「 becomes ﹁).
    for mark in ["「", "」", "『", "』", "（", "）"] {
        let set = text::layout(&format!("あ{mark}"), &column);
        assert_eq!(set.cells[1].form, Form::Rotated, "{mark}");
        let (vw, vh) = size(set.cells[1].ink.unwrap());
        let (lw, lh) = size(text::layout(mark, &line).cells[0].ink.unwrap());
        assert!(lh > lw, "{mark} stands taller than wide in a line: {lw}×{lh}");
        assert!(vw > vh, "{mark} lies wider than tall in a column: {vw}×{vh}");
        assert!((vw - lh).abs() <= 1.5 && (vh - lw).abs() <= 1.5, "{mark}: {vw}×{vh} turned from {lw}×{lh}");
    }
    // A bracketed line keeps one column, the brackets at its two ends.
    let said = text::layout("「セリフ」", &column);
    assert!(said.cells.windows(2).all(|p| p[0].centre().0 == p[1].centre().0), "one column");
    assert!(said.cells.windows(2).all(|p| p[1].centre().1 > p[0].centre().1), "down it");
    // … and ― run down the column.
    for mark in ["…", "―"] {
        let set = text::layout(&format!("あ{mark}"), &column);
        assert_eq!(set.cells[1].form, Form::Rotated, "{mark}");
        let (w, h) = size(set.cells[1].ink.unwrap());
        assert!(h > 2.0 * w, "{mark} runs down the column: {w}×{h}");
    }
    // 。 sits in the cell's top-right quadrant, as 、 does.
    let stop = &text::layout("あ。", &column).cells[1];
    assert_eq!(stop.form, Form::Corner);
    let (ink, (cx, cy)) = (stop.ink.unwrap(), stop.centre());
    let (ix, iy) = ((ink[0] + ink[2]) / 2.0, (ink[1] + ink[3]) / 2.0);
    assert!(ix > cx && iy < cy, "。 in the top-right quadrant: ink ({ix}, {iy}) in a cell centred ({cx}, {cy})");
}

#[test]
fn an_outline_rings_the_letters_in_its_own_colour() {
    let b = text::layout("Gaban", &Style::new(Face::Sans, 32.0, Direction::Horizontal));
    let (black, white) = ([0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
    let (plain, p0) = text::raster_block(&b, black, 0.0, white);
    let (ringed, p1) = text::raster_block(&b, black, 3.0, white);
    assert_eq!(p0, 0);
    assert!(p1 >= 4);
    assert_eq!((ringed.w, ringed.h), (plain.w + 2 * p1, plain.h + 2 * p1));
    assert!(inked(&ringed) > inked(&plain) + 200);
    let rings = ringed
        .px
        .chunks_exact(4)
        .filter(|p| p[3] > 0.99 && p[0] > 0.99)
        .count();
    assert!(rings > 200, "the ring is white: {rings} px");
    // The letters stay black over it.
    let cores = ringed.px.chunks_exact(4).filter(|p| p[3] > 0.99 && p[0] < 0.01).count();
    assert!(cores > 100, "{cores}");
}

#[test]
fn lettering_works_only_its_target_however_big_the_type() {
    // A window of a block is that crop of the whole block's raster, outline
    // and all (the outline reaches in from ink outside the window).
    let b = text::layout("Gaban セリフ\n縦書き", &Style::new(Face::SansJp, 40.0, Direction::Horizontal));
    let (black, white) = ([0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
    let (whole, pad) = text::raster_block(&b, black, 5.0, white);
    let win = text::Window { x: 37, y: -4, w: 90, h: 60 };
    let part = text::raster_block_in(&b, black, 5.0, white, win);
    assert_eq!((part.w, part.h), (win.w, win.h));
    assert!(inked(&part) > 500, "the window holds ink and ring");
    for y in 0..win.h {
        for x in 0..win.w {
            let (sx, sy) = (x as i64 + win.x + pad as i64, y as i64 + win.y + pad as i64);
            let want = if sx >= 0 && sy >= 0 && sx < whole.w as i64 && sy < whole.h as i64 {
                whole.pixel(sx as u32, sy as u32)
            } else {
                [0.0; 4]
            };
            assert_eq!(part.pixel(x, y), want, "({x}, {y}) of the window");
        }
    }
    // A window wholly past the block and its outline is clear.
    let away = text::Window { x: whole.w as i64 + 10, y: 0, w: 32, h: 32 };
    assert_eq!(inked(&text::raster_block_in(&b, black, 5.0, white, away)), 0);
    // A long line at the largest size with the widest outline (about
    // 200000 × 2900 px of type) into a small target: only the target is
    // worked, so this costs a 256 × 144 render, not gigabytes. The middle
    // of the 51st square fills it.
    assert!(Face::SansJp.covers('■'));
    let long = "■".repeat(101);
    let v = vals(GenKind::Text, &[("text", words(&long)), ("size", n(2000.0)), ("outline", n(100.0))]);
    let img = draw(&v, 0, 256, 144);
    assert_eq!((img.w, img.h), (256, 144));
    assert!(img.px.chunks_exact(4).all(|p| p[3] > 0.999 && p[0] < 0.001), "the square's middle is solid ink");
}

// ---- Balloons -------------------------------------------------------------------

fn balloon(shape: &str) -> GenValues {
    vals(
        GenKind::Balloon,
        &[
            ("shape", pick(shape)),
            ("x", n(0.5)),
            ("y", n(0.4)),
            ("width", n(0.5)),
            ("height", n(0.3)),
            ("tail_x", n(150.5 / 200.0)),
            ("tail_y", n(180.5 / 200.0)),
            ("tail_width", n(0.06)),
            ("stroke", FxValue::Color([255, 0, 0, 255])),
            ("stroke_width", n(3.0)),
        ],
    )
}

#[test]
fn a_balloon_fills_inside_strokes_its_outline_and_its_tail_reaches_the_point() {
    let img = draw(&balloon("ellipse"), 0, 200, 200);
    assert_eq!(rgba8(&img, 100, 80), [255, 255, 255, 255], "the fill inside");
    assert_eq!(rgba8(&img, 51, 80), [255, 0, 0, 255], "the stroke just inside the left edge");
    assert_eq!(rgba8(&img, 40, 80)[3], 0, "nothing outside");
    assert!(img.pixel(150, 180)[3] >= 0.4, "the tail's tip pixel is the point: {:?}", img.pixel(150, 180));
    for (x, y) in [(151, 181), (150, 181), (151, 180), (152, 182)] {
        assert_eq!(img.pixel(x, y)[3], 0.0, "past the tip at ({x}, {y})");
    }
    let mid = rgba8(&img, 125, 130);
    assert_eq!(mid[3], 255, "the tail's body is filled: {mid:?}");
    let no_tail = {
        let mut p = params(GenKind::Balloon, &[]);
        p.set("tail", FxValue::Bool(false));
        draw(&GenValues::of(GenKind::Balloon, &p), 0, 200, 200)
    };
    assert_eq!(no_tail.pixel(150, 180)[3], 0.0);
}

#[test]
fn cloud_and_burst_balloons_draw_their_own_outlines() {
    let ellipse = draw(&balloon("ellipse"), 0, 200, 200);
    for shape in ["rounded rect", "cloud", "burst"] {
        let img = draw(&balloon(shape), 0, 200, 200);
        assert_eq!(rgba8(&img, 100, 80), [255, 255, 255, 255], "{shape}: filled");
        assert_ne!(img, ellipse, "{shape}: its own outline");
        assert!(inked(&img) > 4000, "{shape}");
    }
    // The burst's spikes reach past the ellipse.
    let burst = draw(&balloon("burst"), 0, 200, 200);
    let past = (0..200u32)
        .flat_map(|y| (0..200u32).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let q = ((x as f32 + 0.5 - 100.0) / 50.0).powi(2) + ((y as f32 + 0.5 - 80.0) / 30.0).powi(2);
            q > 1.2 && y < 100 && burst.pixel(x, y)[3] > 0.0
        })
        .count();
    assert!(past > 50, "{past}");
}

/// The first solid pixel (alpha above 0.99) walking from `at` by `step`.
fn first_solid(img: &Image, mut at: (i32, i32), step: (i32, i32)) -> Option<(u32, u32)> {
    while at.0 >= 0 && at.1 >= 0 && at.0 < img.w as i32 && at.1 < img.h as i32 {
        if img.pixel(at.0 as u32, at.1 as u32)[3] > 0.99 {
            return Some((at.0 as u32, at.1 as u32));
        }
        at = (at.0 + step.0, at.1 + step.1);
    }
    None
}

#[test]
fn every_balloon_shape_keeps_its_whole_outline_at_hd_proportions() {
    // 256 × 144 is 1920 × 1080 scaled: the default balloon, and a 0.3 × 0.3
    // one with 8 bumps, whose bumps reach farthest past the body (a cloud
    // whose box was its radii drew them cut flat at the top and bottom).
    let (w, h) = (256u32, 144u32);
    let tip = (200u32, 128u32);
    for (width, height, bumps) in [(0.4, 0.3, 14.0), (0.3, 0.3, 8.0)] {
        for shape in generator::BALLOONS {
            let v = vals(
                GenKind::Balloon,
                &[
                    ("shape", pick(shape)),
                    ("width", n(width)),
                    ("height", n(height)),
                    ("spikes", n(bumps)),
                    ("tail_x", n((tip.0 as f32 + 0.5) / w as f32)),
                    ("tail_y", n((tip.1 as f32 + 0.5) / h as f32)),
                    ("stroke", FxValue::Color([255, 0, 0, 255])),
                    ("stroke_width", n(3.0)),
                ],
            );
            let img = draw(&v, 0, w, h);
            let what = format!("{shape} {width} × {height}, {bumps} bumps");
            let (cx, cy) = (w / 2, h / 2);
            assert_eq!(rgba8(&img, cx, cy), [255, 255, 255, 255], "{what}: the fill inside");
            // The stroke on the outline: the first solid pixel in from the
            // top and from either side is the stroke's.
            for (from, step) in [((cx as i32, 0), (0, 1)), ((0, cy as i32), (1, 0)), ((w as i32 - 1, cy as i32), (-1, 0))] {
                let (x, y) = first_solid(&img, from, step).unwrap_or_else(|| panic!("{what}: no outline from {from:?}"));
                assert_eq!(rgba8(&img, x, y), [255, 0, 0, 255], "{what}: the stroke at ({x}, {y})");
            }
            // The tail's tip pixel is the point, and nothing lies past it.
            assert!(img.pixel(tip.0, tip.1)[3] >= 0.4, "{what}: the tip {:?}", img.pixel(tip.0, tip.1));
            for (x, y) in [(tip.0 + 1, tip.1 + 1), (tip.0 + 2, tip.1 + 2), (tip.0 + 1, tip.1)] {
                assert_eq!(img.pixel(x, y)[3], 0.0, "{what}: past the tip at ({x}, {y})");
            }
            // Every outermost row and column of ink is the outline's soft
            // edge: a shape cut short by its box ends in solid ink.
            let (x0, y0, x1, y1) = ink_box(&img).unwrap();
            assert!(x0 > 0 && y0 > 0 && x1 < w - 1 && y1 < h - 1, "{what}: inside the frame");
            let edges = [
                ("top", (x0..=x1).map(|x| img.pixel(x, y0)[3]).collect::<Vec<_>>()),
                ("bottom", (x0..=x1).map(|x| img.pixel(x, y1)[3]).collect()),
                ("left", (y0..=y1).map(|y| img.pixel(x0, y)[3]).collect()),
                ("right", (y0..=y1).map(|y| img.pixel(x1, y)[3]).collect()),
            ];
            for (side, alphas) in edges {
                let solid = alphas.iter().filter(|a| **a > 0.999).count();
                assert_eq!(solid, 0, "{what}: the {side} edge is cut ({solid} solid px)");
            }
        }
    }
}

// ---- Nodes, the store and the eval hash -----------------------------------------

struct Rig {
    engine: Engine,
    at: CutRef,
    node: NodeId,
    param: ParamId,
}

fn rig(kind: NodeKind) -> Rig {
    let mut engine = Engine::new("gen tests");
    let scene = engine.add_scene("SC");
    let cut = engine.add_cut(scene, "cut", 24).unwrap();
    let at = CutRef { scene, cut };
    let param = engine.add_param_column(at, "gen.angle").unwrap();
    let (node, out) = (engine.alloc_node_id(), engine.alloc_node_id());
    engine
        .apply(
            "rig",
            vec![
                Command::AddNode { at, id: node, kind },
                Command::AddNode { at, id: out, kind: NodeKind::Output },
                Command::Connect { at, from: node, from_pin: 0, to: out, to_pin: 0 },
                Command::SetOutput { at, node: Some(out) },
            ],
        )
        .unwrap();
    Rig { engine, at, node, param }
}

impl Rig {
    fn cut(&self) -> &anim_core::model::Cut {
        self.engine.project.cut(self.at.scene, self.at.cut).unwrap()
    }

    fn kind(&self) -> NodeKind {
        self.cut().graph.node(self.node).unwrap().kind.clone()
    }

    fn set_kind(&mut self, kind: NodeKind) {
        let (at, id) = (self.at, self.node);
        self.engine.apply("params", vec![Command::SetNodeKind { at, id, kind }]).unwrap();
    }

    fn key(&mut self, frame: u32, value: f32) {
        let (at, column) = (self.at, self.param);
        let key = Some(ParamKey { value, interp: ParamInterp::Linear, handles: None });
        self.engine.apply("key", vec![Command::SetParamKey { at, column, frame, key }]).unwrap();
    }

    fn hash(&mut self, frame: u32) -> u64 {
        self.engine.eval(self.at.scene, self.at.cut, frame).unwrap().hash()
    }
}

#[test]
fn generator_nodes_round_trip_through_the_store_with_binds_and_text() {
    let mut r = rig(NodeKind::generator(GenKind::SpeedLines));
    assert_eq!(r.kind().input_count(), 0);
    assert_eq!(NodeKind::generator(GenKind::Particles).input_count(), 1, "the matte pin");
    let NodeKind::Generator { generator: g, mut params, mut binds } = r.kind() else { unreachable!() };
    assert!(g.spec("angle").unwrap().bindable());
    assert!(!GenKind::Text.spec("text").unwrap().bindable(), "a text is not a column's number");
    params.set("count", n(40.0));
    binds.set("angle", Some(r.param));
    r.set_kind(NodeKind::Generator { generator: g, params, binds });
    assert!(r.kind().references(r.param));
    // Lettering and a particle node with its matte wired from a solid.
    let (at, text, solid, sparks) = (r.at, r.engine.alloc_node_id(), r.engine.alloc_node_id(), r.engine.alloc_node_id());
    let mut lettering = NodeKind::generator(GenKind::Text);
    if let NodeKind::Generator { params, .. } = &mut lettering {
        params.set("text", words("決め台詞！"));
        params.set("direction", pick("vertical"));
    }
    r.engine
        .apply(
            "more",
            vec![
                Command::AddNode { at, id: text, kind: lettering },
                Command::AddNode { at, id: solid, kind: NodeKind::Solid { rgba: [255, 255, 255, 255] } },
                Command::AddNode { at, id: sparks, kind: NodeKind::particles(Preset::Sparks) },
                Command::Connect { at, from: solid, from_pin: 0, to: sparks, to_pin: 0 },
            ],
        )
        .unwrap();
    r.key(0, 0.0);
    r.key(10, 90.0);
    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("gen_roundtrip.animproj");
    r.engine.save(&path).unwrap();
    let back = Engine::load(&path).unwrap();
    std::fs::remove_file(&path).ok();
    let (a, b) = (r.cut(), back.project.cut(at.scene, at.cut).unwrap());
    assert_eq!(a.graph, b.graph, "every generator, param, bind, text and wire survives");
    let Ok(NodeKind::Generator { params, .. }) = b.graph.node(text).map(|n| n.kind.clone()) else { panic!() };
    assert_eq!(params.get("text"), Some(&words("決め台詞！")));
    for f in [0, 5] {
        assert_eq!(graph_composite_pixel(a, f, 64, 36, 20, 10), graph_composite_pixel(b, f, 64, 36, 20, 10));
    }
    // The bound angle resolves per frame.
    let NodeKind::Generator { generator: g, params, binds } = r.kind() else { unreachable!() };
    assert_eq!(r.cut().gen_at(g, &params, &binds, 5).float("angle"), 45.0);
    assert_eq!(r.cut().gen_at(g, &params, &binds, 5).float("count"), 40.0);
    let empty = FxBinds::default();
    assert_eq!(r.cut().gen_at(g, &params, &empty, 5).float("angle"), 0.0, "unbound: the stored value");
}

#[test]
fn the_eval_hash_moves_with_the_generator_only_when_its_picture_does() {
    let mut held = rig(NodeKind::generator(GenKind::SpeedLines));
    assert_eq!(held.hash(0), held.hash(9), "held lines cache-hit across frames");
    let NodeKind::Generator { generator: g, mut params, binds } = held.kind() else { unreachable!() };
    params.set("motion", pick("on twos"));
    held.set_kind(NodeKind::Generator { generator: g, params: params.clone(), binds: binds.clone() });
    assert_eq!(held.hash(4), held.hash(5), "on twos: frames 4 and 5 are one drawing");
    assert_ne!(held.hash(5), held.hash(6));
    params.set("count", n(12.0));
    let before = held.hash(4);
    held.set_kind(NodeKind::Generator { generator: g, params, binds });
    assert_ne!(held.hash(4), before, "a parameter changes the hash");
    let mut text = rig(NodeKind::generator(GenKind::Text));
    assert_eq!(text.hash(0), text.hash(13), "lettering holds");
    let mut sparks = rig(NodeKind::particles(Preset::Sparks));
    assert_ne!(sparks.hash(3), sparks.hash(4), "particles move every frame");
}
