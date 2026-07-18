//! Phase 4 migration soak: round-trip REAL project files. Ignored by default —
//! run explicitly with the ANIMSTUDIO_SOAK env var pointing at files or
//! directories (`;`-separated):
//!
//! ```text
//! ANIMSTUDIO_SOAK="C:\path\to\projects" \
//!     cargo test -p anim-core --test migration_soak -- --ignored --nocapture
//! ```
//!
//! Originals are NEVER written: each file is loaded read-only, saved to a
//! temp copy (upgrading it to the current schema), reloaded, and compared.

use std::path::PathBuf;

use anim_core::Engine;

fn collect(path: &std::path::Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().is_some_and(|e| e == "animproj") {
            out.push(path.to_path_buf());
        }
        return;
    }
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            collect(&entry.path(), out);
        }
    }
}

#[test]
#[ignore = "runs against real files via ANIMSTUDIO_SOAK"]
fn soak_real_projects() {
    let spec = std::env::var("ANIMSTUDIO_SOAK")
        .expect("set ANIMSTUDIO_SOAK to ;-separated files/dirs of .animproj files");
    let mut files = Vec::new();
    for part in spec.split(';').filter(|s| !s.trim().is_empty()) {
        collect(std::path::Path::new(part.trim()), &mut files);
    }
    assert!(!files.is_empty(), "no .animproj files found under {spec}");

    let tmp = std::env::temp_dir().join("anim_soak");
    std::fs::create_dir_all(&tmp).unwrap();

    let mut failures = 0usize;
    for (i, file) in files.iter().enumerate() {
        print!("[{}/{}] {} … ", i + 1, files.len(), file.display());
        let engine = match Engine::load(file) {
            Ok(e) => e,
            Err(e) => {
                println!("LOAD FAILED: {e}");
                failures += 1;
                continue;
            }
        };
        // Inventory: drawings / layers / tiles.
        let mut drawings = 0usize;
        let mut layers = 0usize;
        let mut tiles = 0usize;
        for scene in &engine.project.scenes {
            for cut in &scene.cuts {
                drawings += cut.drawings.len();
                for d in &cut.drawings {
                    layers += d.layers.len();
                    tiles += d.layers.iter().map(|l| l.raster.tiles.len()).sum::<usize>();
                }
            }
        }

        // Round-trip via a temp copy (the original is never written).
        let copy = tmp.join(format!("soak_{i}.animproj"));
        let _ = std::fs::remove_file(&copy);
        if let Err(e) = engine.save(&copy) {
            println!("SAVE FAILED: {e}");
            failures += 1;
            continue;
        }
        let reloaded = match Engine::load(&copy) {
            Ok(e) => e,
            Err(e) => {
                println!("RELOAD FAILED: {e}");
                failures += 1;
                continue;
            }
        };
        if reloaded.project != engine.project {
            println!("MISMATCH after save/reload");
            failures += 1;
            continue;
        }
        // Second generation: v5 → v5 must be stable too.
        reloaded.save(&copy).unwrap();
        let again = Engine::load(&copy).unwrap();
        if again.project != reloaded.project {
            println!("MISMATCH after second save/reload");
            failures += 1;
            continue;
        }
        let _ = std::fs::remove_file(&copy);
        println!("OK — {drawings} drawings, {layers} layers, {tiles} tiles");
    }

    assert_eq!(failures, 0, "{failures} of {} files failed the soak", files.len());
}
