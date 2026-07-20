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
                ColorRole { name: "normal".into(), color: [222, 178, 140, 255] },
                ColorRole { name: "shadow".into(), color: [96, 112, 192, 255] },
                ColorRole { name: "highlight".into(), color: [255, 233, 168, 255] },
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
