//! Headless pictures of the real editor: `cargo test -p animstudio uishot --
//! --ignored` with UISHOT_DIR set. No window opens; a GPU adapter draws the
//! same egui output the app presents, so a redesign is looked at last, on
//! real pixels, without anything surfacing on the desktop.

use std::path::PathBuf;

use eframe::egui;

use crate::testkit::EditorHarness;
use crate::workspace::Stage;

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(std::env::var("UISHOT_DIR").expect("set UISHOT_DIR to the output folder"));
    std::fs::create_dir_all(&dir).expect("make UISHOT_DIR");
    dir
}

#[test]
#[ignore = "needs a GPU adapter and UISHOT_DIR; writes PNGs"]
fn uishot_rooms() {
    let dir = out_dir();
    let size = egui::vec2(1600.0, 900.0);
    let mut h = EditorHarness::with_state(crate::sample::build_sample());
    h.run_frames(4);
    for (i, stage) in Stage::ALL.iter().chain([Stage::Manga].iter()).enumerate() {
        h.editor_mut().enter_room(*stage);
        h.run_frames(4);
        h.shot(size, &dir.join(format!("room-{i}-{:?}.png", stage)));
    }
}
