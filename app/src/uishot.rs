//! Headless pictures of the real editor: `cargo test -p animstudio uishot --
//! --ignored` with UISHOT_DIR set. No window opens; a GPU adapter draws the
//! same egui output the app presents, so a redesign is looked at last, on
//! real pixels, without anything surfacing on the desktop.

use std::path::PathBuf;
use std::time::Duration;

use eframe::egui;

use crate::measure::room_tag;
use crate::testkit::EditorHarness;
use crate::viewer::GraphView;
use crate::PROBE_ROOMS;

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(std::env::var("UISHOT_DIR").expect("set UISHOT_DIR to the output folder"));
    std::fs::create_dir_all(&dir).expect("make UISHOT_DIR");
    dir
}

/// The room, arrived: thumbnails decoded, the compositor's picture back.
/// Its render runs on its own thread, so this one waits in real time, bounded
/// so a graph that never answers still gets its picture.
fn arrive(h: &mut EditorHarness) {
    h.run_frames(30);
    for _ in 0..200 {
        if !matches!(h.editor().dbg_graph, GraphView::Rendering(_)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
        h.run_frames(1);
    }
}

fn walk(h: &mut EditorHarness, size: egui::Vec2, suffix: &str) {
    let dir = out_dir();
    h.kit_mut().set_size(size);
    for (i, room) in PROBE_ROOMS.into_iter().enumerate() {
        room.enter(h.editor_mut());
        arrive(h);
        h.shot(size, &dir.join(format!("room-{i}-{}{suffix}.png", room_tag(room))));
    }
}

#[test]
#[ignore = "needs a GPU adapter and UISHOT_DIR; writes PNGs"]
fn uishot_rooms() {
    let mut h = EditorHarness::with_gpu(crate::sample::build_sample());
    h.run_frames(4);
    walk(&mut h, egui::vec2(1600.0, 900.0), "");
    walk(&mut h, egui::vec2(1280.0, 800.0), "-1280");
}
