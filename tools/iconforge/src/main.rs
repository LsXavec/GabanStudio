//! iconforge — the project's asset forge. Two jobs, both producing ORIGINAL
//! work owned by this repo:
//!
//! 1. ICONS: render every `app/assets/icons/svg/*.svg` (authored in-repo,
//!    drawn in white + greys) at 24px and 48px into
//!    `app/assets/icons/png/<name>_<size>.png`. The app embeds the PNGs and
//!    tints them at draw time, so ONE mask serves every ink the colour law
//!    allows (Legend at rest, Struck armed, dim disabled).
//!
//! 2. GRAIN: generate the chrome's material textures procedurally —
//!    a 256x256 tileable value-noise grain with a faint horizontal brush,
//!    seeded, deterministic. `plate_grain.png` IS the plate: opaque
//!    Graphite with +-4 levels of relief, hue preserved — never a new hue.
//!
//! Deterministic: same inputs → byte-identical outputs. Re-run after adding
//! or editing an SVG, then rebuild the app.

use std::path::{Path, PathBuf};

use resvg::tiny_skia;
use resvg::usvg;

fn main() {
    let root = repo_root();
    let svg_dir = root.join("app/assets/icons/svg");
    let png_dir = root.join("app/assets/icons/png");
    let tex_dir = root.join("app/assets/tex");
    std::fs::create_dir_all(&png_dir).expect("create png dir");
    std::fs::create_dir_all(&tex_dir).expect("create tex dir");

    // ---- 1. Icons ---------------------------------------------------------
    let mut count = 0usize;
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&svg_dir)
        .expect("read svg dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "svg"))
        .collect();
    entries.sort();
    for path in &entries {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let data = std::fs::read(path).expect("read svg");
        let tree = usvg::Tree::from_data(&data, &usvg::Options::default())
            .unwrap_or_else(|e| panic!("{name}.svg: {e}"));
        for size in [24u32, 48u32] {
            let mut pixmap = tiny_skia::Pixmap::new(size, size).unwrap();
            let scale = size as f32 / tree.size().width();
            resvg::render(
                &tree,
                tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            let out = png_dir.join(format!("{name}_{size}.png"));
            pixmap.save_png(&out).expect("write png");
        }
        count += 1;
    }
    println!("icons: {count} SVGs → {} PNGs", count * 2);

    // ---- 2. Grain ---------------------------------------------------------
    // Tileable value noise, two octaves, plus a faint horizontal brushed
    // streak. Emitted as a NEUTRAL grain centred on 128: the app blends it
    // over Graphite (or any panel fill) at low alpha, so the texture adds
    // material, never colour.
    let n = 256u32;
    let mut pixmap = tiny_skia::Pixmap::new(n, n).unwrap();
    {
        let px = pixmap.pixels_mut();
        for y in 0..n {
            for x in 0..n {
                // Two octaves of lattice value noise (tileable: lattice
                // wraps at 16 and 32 cells).
                let v1 = value_noise(x as f32 / 16.0, y as f32 / 16.0, 16, 1);
                let v2 = value_noise(x as f32 / 8.0, y as f32 / 8.0, 32, 7);
                // Horizontal brush: a slow noise sampled only on y, stretched.
                let streak = value_noise(y as f32 / 4.0, 0.0, 64, 13);
                let g = 0.55 * v1 + 0.30 * v2 + 0.15 * streak; // 0..1
                // The plate itself: Graphite (26,27,24) with ±4 levels of
                // relief, hue offset preserved — opaque, laid down as the
                // panel surface. Material, never a new colour.
                let rel = ((g - 0.5) * 8.0).round();
                let ch = |base: f32| (base + rel).clamp(0.0, 255.0) as u8;
                px[(y * n + x) as usize] =
                    tiny_skia::PremultipliedColorU8::from_rgba(ch(26.0), ch(27.0), ch(24.0), 255)
                        .unwrap();
            }
        }
    }
    pixmap
        .save_png(tex_dir.join("plate_grain.png"))
        .expect("write grain");
    println!("grain: plate_grain.png (256×256, seeded, tileable)");
}

/// Tileable lattice value noise in [0,1]. `period` is the lattice size the
/// coordinates wrap at; `seed` decorrelates octaves. Deterministic.
fn value_noise(x: f32, y: f32, period: u32, seed: u32) -> f32 {
    let xi = x.floor() as i64;
    let yi = y.floor() as i64;
    let xf = x - x.floor();
    let yf = y - y.floor();
    // Smoothstep fade.
    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let p = period as i64;
    let h = |ix: i64, iy: i64| -> f32 {
        let ix = ix.rem_euclid(p) as u32;
        let iy = iy.rem_euclid(p) as u32;
        // A small integer hash (xorshift-flavoured), stable across runs.
        let mut k = ix.wrapping_mul(374761393)
            ^ iy.wrapping_mul(668265263)
            ^ seed.wrapping_mul(2246822519);
        k ^= k >> 13;
        k = k.wrapping_mul(1274126177);
        k ^= k >> 16;
        (k & 0xFFFF) as f32 / 65535.0
    };
    let a = h(xi, yi) + u * (h(xi + 1, yi) - h(xi, yi));
    let b = h(xi, yi + 1) + u * (h(xi + 1, yi + 1) - h(xi, yi + 1));
    a + v * (b - a)
}

fn repo_root() -> PathBuf {
    // tools/iconforge → repo root is two levels up from CARGO_MANIFEST_DIR.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}
