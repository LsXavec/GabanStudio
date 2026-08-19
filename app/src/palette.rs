//! Character color palettes (B5) — named per-character colour sets (normal/
//! shadow/highlight/...), PROJECT-persisted so they travel with the .animproj
//! and everyone drawing that character uses the same numbers.
//!
//! Stored as an opaque JSON blob in `Project.app_meta` (the engine never
//! parses it — see that field's doc comment) under [`META_KEY`]. Not
//! undo-tracked: like project resolution/fps/dpi, this is project-level
//! reference data set up once and occasionally edited, not moment-to-moment
//! artwork; it round-trips through Save/Open like everything else in the
//! file, just outside the Command/undo system.

use anim_core::model::Project;

const META_KEY: &str = "palettes";

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ColorRole {
    pub name: String,
    pub color: [u8; 4],
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct CharacterPalette {
    pub name: String,
    pub roles: Vec<ColorRole>,
}

impl CharacterPalette {
    /// A fresh character with the standard anime color-model triple —
    /// same values as the per-layer defaults (Settings → Layers), so a
    /// character's "normal" tone matches what "color" paints by default.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            roles: vec![
                ColorRole {
                    name: "normal".into(),
                    color: [222, 178, 140, 255],
                },
                ColorRole {
                    name: "shadow".into(),
                    color: [96, 112, 192, 255],
                },
                ColorRole {
                    name: "highlight".into(),
                    color: [255, 233, 168, 255],
                },
            ],
        }
    }
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Palettes {
    pub characters: Vec<CharacterPalette>,
}

impl Palettes {
    /// Load from a project's opaque meta (missing/unparseable key = empty —
    /// every pre-B5 project, and a corrupt hand-edited value, both land here
    /// rather than refusing to open the project).
    pub fn load_from(project: &Project) -> Self {
        project
            .app_meta
            .get(META_KEY)
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }

    /// Write the current palettes into the project's opaque meta — call
    /// right before an actual file save (AppState::save), the same point
    /// project-level data is always this fresh-on-demand.
    pub fn save_into(&self, project: &mut Project) {
        if let Ok(s) = serde_json::to_string(self) {
            project.app_meta.insert(META_KEY.to_string(), s);
        }
    }
}

// ---------------------------------------------------------------------------
// Colour math for THE PAINT DISH (room 2026-08-17). Pure functions — the
// dish's DERIVE and Eye use these; no colour policy lives here.
// ---------------------------------------------------------------------------

pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

/// The pipeline's systematic shadow from a normal tone: hue leans cool
/// (15% of the way toward 240°), saturation up a step, value down a step.
/// A STARTING POINT — the colour designer's taste edits it after; the
/// dish guards every write to the model behind a held DANGER.
pub fn derive_shadow(normal: [u8; 4]) -> [u8; 4] {
    let (h, s, v) = rgb_to_hsv(normal[0], normal[1], normal[2]);
    let mut d = 240.0 - h;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    let (r, g, b) = hsv_to_rgb(
        (h + d * 0.15).rem_euclid(360.0),
        (s * 1.15).min(1.0),
        (v * 0.72).max(0.0),
    );
    [r, g, b, normal[3]]
}

#[cfg(test)]
mod dish_tests {
    use super::*;

    #[test]
    fn hsv_roundtrips() {
        for c in [
            [222u8, 178, 140],
            [96, 112, 192],
            [25, 25, 30],
            [255, 255, 255],
        ] {
            let (h, s, v) = rgb_to_hsv(c[0], c[1], c[2]);
            let (r, g, b) = hsv_to_rgb(h, s, v);
            assert!((r as i16 - c[0] as i16).abs() <= 1, "{c:?} -> {r},{g},{b}");
            assert!((g as i16 - c[1] as i16).abs() <= 1);
            assert!((b as i16 - c[2] as i16).abs() <= 1);
        }
    }

    #[test]
    fn derived_shadow_is_cooler_and_darker() {
        let n = [222u8, 178, 140, 255];
        let sh = derive_shadow(n);
        let (_, _, vn) = rgb_to_hsv(n[0], n[1], n[2]);
        let (hs, _, vs) = rgb_to_hsv(sh[0], sh[1], sh[2]);
        assert!(vs < vn, "shadow must be darker");
        // Cooler: closer to 240° than the warm normal was.
        let (hn, _, _) = rgb_to_hsv(n[0], n[1], n[2]);
        let dist = |h: f32| {
            let mut d = (240.0 - h).abs();
            if d > 180.0 {
                d = 360.0 - d;
            }
            d
        };
        assert!(dist(hs) < dist(hn), "shadow must lean cool");
    }
}
