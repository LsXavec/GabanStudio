//! UI hook — an on-demand dump of what the app is ACTUALLY showing.
//!
//! ROOT (ratified): this stands on the hook observing the same build he
//! actually runs; if that's weak, this is rubble.
//!
//! So it has no feature flag, no debug-only path, no window of its own, and
//! no second renderer. It asks the live viewport for the frame it just
//! presented (`ViewportCommand::Screenshot`) and writes it next to a text
//! record of what the app believes it is showing. Both come from the running
//! editor state — the same values that drew the pixels.
//!
//! It is a READ. Nothing here may mutate the document.
//!
//! NEVER-DO 4 stands over this file: the dump reports what is on screen, it
//! does not certify that anything is correct. A dump is not a test.

use eframe::egui;
use std::fmt::Write as _;
use std::path::PathBuf;

/// Where dumps land: `<exe dir>/ui_dump/`. Beside the binary on purpose —
/// whoever ran the build can find them without knowing a config path.
fn dump_dir() -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("ui_dump")
}

/// A request that has been asked for but whose image has not arrived.
///
/// The reply is not guaranteed: a viewport that never honours the command
/// leaves this set forever, which would silently kill F12 for the session.
/// It expires.
const REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

pub struct UiDump {
    awaiting: Option<std::time::Instant>,
    /// Dump counter. Seeded from what is ALREADY on disk, because the dev loop
    /// restarts the process on every rebuild — a per-process counter starting
    /// at 0 destroyed the "before" dump with the "after" one, which is exactly
    /// the comparison this instrument exists to make.
    seq: u32,
    /// One-shot latch for ANIMSTUDIO_UIDUMP.
    auto_done: bool,
    /// Last thing that happened, shown in the status bar so a dump is never
    /// silent.
    pub last: Option<String>,
}

impl UiDump {
    pub fn new() -> Self {
        Self {
            awaiting: None,
            seq: next_seq(),
            auto_done: false,
            last: None,
        }
    }

    /// True once, when ANIMSTUDIO_UIDUMP asked for an automatic dump.
    pub fn auto_pending(&mut self) -> bool {
        if self.auto_done || std::env::var_os("ANIMSTUDIO_UIDUMP").is_none() {
            return false;
        }
        self.auto_done = true;
        true
    }

    /// Ask the live viewport for the frame it is about to present.
    pub fn request(&mut self, ctx: &egui::Context) {
        if self.awaiting.is_some() {
            return; // one in flight is enough; the reply is one frame away
        }
        self.awaiting = Some(std::time::Instant::now());
        self.last = Some("capturing…".into());
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        ctx.request_repaint();
    }

    /// Collect the screenshot reply, if one arrived this frame.
    ///
    /// The caller describes the app state at THIS moment — the top of the
    /// frame after the captured one, i.e. the state that produced the pixels.
    /// Describing it at request time instead meant the text was written before
    /// that frame's playback tick and pen input had run, so a dump taken
    /// during playback reported a frame number the image did not show.
    pub fn take_image(&mut self, ctx: &egui::Context) -> Option<std::sync::Arc<egui::ColorImage>> {
        let started = self.awaiting?;
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if shot.is_some() {
            self.awaiting = None;
            return shot;
        }
        if started.elapsed() > REPLY_TIMEOUT {
            self.awaiting = None;
            self.last = Some("⚠ ui dump: the viewport never returned a frame".into());
        }
        None
    }

    /// Write the pair. Both files carry the same sequence number, and the PNG
    /// is written FIRST so a failure can never leave a state file claiming to
    /// describe an image that does not exist.
    pub fn write(&mut self, image: std::sync::Arc<egui::ColorImage>, state: String) {
        let dir = dump_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.last = Some(format!("⚠ ui dump failed: {e}"));
            return;
        }
        let seq = self.seq;
        let png_path = dir.join(format!("frame_{seq:03}.png"));
        let txt_path = dir.join(format!("state_{seq:03}.txt"));
        if let Err(e) = write_png(&png_path, &image) {
            self.last = Some(format!("⚠ ui dump png failed: {e}"));
            return;
        }
        // The text record carries the image's real pixel size, so a reader can
        // tell a HiDPI frame from a logical-point layout without guessing.
        let header = format!("frame: {}x{} px\n{}", image.size[0], image.size[1], state);
        if let Err(e) = std::fs::write(&txt_path, header) {
            self.last = Some(format!("⚠ ui dump txt failed: {e}"));
            let _ = std::fs::remove_file(&png_path);
            return;
        }
        self.seq += 1;
        self.last = Some(format!("ui dump → frame_{seq:03}.png"));
    }
}

/// The next free index in the dump directory, so dumps accumulate across
/// restarts instead of overwriting each other.
fn next_seq() -> u32 {
    let dir = dump_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let mut max: Option<u32> = None;
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(rest) = name.strip_prefix("frame_") else {
            continue;
        };
        let Some(num) = rest.strip_suffix(".png") else {
            continue;
        };
        if let Ok(n) = num.parse::<u32>() {
            max = Some(max.map_or(n, |m: u32| m.max(n)));
        }
    }
    max.map_or(0, |m| m + 1)
}

/// RGBA8 PNG via the `png` crate (already a dependency for export).
fn write_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    let (w, h) = (image.size[0] as u32, image.size[1] as u32);
    let mut bytes = Vec::with_capacity(image.pixels.len() * 4);
    for px in &image.pixels {
        // egui stores PREMULTIPLIED alpha; PNG expects unassociated. Writing
        // `to_array()` straight through looks correct for an opaque window
        // (alpha 255 is a no-op) and silently darkens everything the moment
        // any pixel is translucent — exactly the kind of wrong-but-plausible
        // image an instrument must never produce.
        let [r, g, b, a] = px.to_srgba_unmultiplied();
        bytes.extend_from_slice(&[r, g, b, a]);
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&bytes).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The PNG writer is the one part of this file a `cargo check` cannot see
    /// through: a wrong channel order or a bad row stride still compiles and
    /// produces a file, and the failure only shows up as a garbled image on
    /// someone's screen. Encode a known 2x2 and decode it back.
    #[test]
    fn png_roundtrips_pixels_in_the_right_order() {
        let px = vec![
            egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255),
            egui::Color32::from_rgba_unmultiplied(0, 255, 0, 255),
            egui::Color32::from_rgba_unmultiplied(0, 0, 255, 255),
            // Translucent on purpose: this is the case that catches a
            // premultiplied write. 128 alpha survives the round trip within
            // rounding.
            egui::Color32::from_rgba_unmultiplied(200, 100, 50, 128),
        ];
        let img = egui::ColorImage::new([2, 2], px);
        let dir = std::env::temp_dir().join("animstudio_uidump_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.png");
        write_png(&path, &img).expect("encode");
        let decoder = png::Decoder::new(std::fs::File::open(&path).unwrap());
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 2));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        // Row-major, top row first, RGBA — the order egui hands us.
        assert_eq!(&buf[0..4], &[255, 0, 0, 255], "first pixel");
        assert_eq!(&buf[4..8], &[0, 255, 0, 255], "second pixel");
        assert_eq!(&buf[8..12], &[0, 0, 255, 255], "third pixel");
        // Premultiply/unpremultiply is lossy by a rounding step; the point is
        // that the colour is RECOVERED, not darkened toward black.
        for (got, want) in buf[12..16].iter().zip([200u8, 100, 50, 128]) {
            assert!(
                got.abs_diff(want) <= 2,
                "translucent pixel came back wrong (premultiplied write?): \
        got {:?} want [200,100,50,128]",
                &buf[12..16]
            );
        }
        let _ = std::fs::remove_file(&path);
    }

    /// The record is read by eye and by diff, so its shape is part of the
    /// contract: an unsaved project must SAY it is unsaved rather than showing
    /// a blank where a path goes.
    #[test]
    fn unsaved_project_is_labelled_not_blank() {
        let d = Describe {
            project: "untitled",
            width: 1920,
            height: 1080,
            fps: 24,
            frame: 3,
            frame_count: 12,
            scenes: 1,
            stage: Some("Layout"),
            panes: vec!["Canvas".into(), "XSheet".into()],
            tool: "brush",
            brush: "-",
            playing: false,
            has_file: false,
            file_path: None,
            pixels_per_point: 1.25,
            screen: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0)),
        };
        let out = d.render();
        assert!(
            out.contains("no file yet"),
            "a document with no file must say so, and must NOT claim to know \
        whether it was modified:\n{out}"
        );
        assert!(
            !out.contains("modified"),
            "the app does not track modification; the record must not imply it does:\n{out}"
        );
        assert!(
            out.contains("frame 3 of 12"),
            "playhead must be present:\n{out}"
        );
        assert!(
            out.contains("panes (2)"),
            "pane count must be present:\n{out}"
        );
    }
}

/// A line-oriented record of what the app believes it is showing.
///
/// Deliberately plain text: it is read by a human or by Claude, and a diff of
/// two dumps should be legible. Every value here is read from the SAME state
/// that produced the frame — nothing is recomputed for the dump.
pub struct Describe<'a> {
    pub project: &'a str,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frame: usize,
    pub frame_count: usize,
    pub scenes: usize,
    pub stage: Option<&'a str>,
    pub panes: Vec<String>,
    pub tool: &'a str,
    pub brush: &'a str,
    pub playing: bool,
    /// Whether the document has a file yet. Deliberately NOT called "dirty":
    /// the app does not track modification, and a record that says "(modified)"
    /// when it only knows "has no path" is asserting something it cannot see.
    pub has_file: bool,
    pub file_path: Option<String>,
    pub pixels_per_point: f32,
    pub screen: egui::Rect,
}

impl Describe<'_> {
    pub fn render(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "project: {}", self.project);
        let _ = writeln!(
            s,
            "file: {}",
            match (&self.file_path, self.has_file) {
                (Some(p), _) => p.as_str(),
                (None, _) => "<no file yet — Save will prompt>",
            }
        );
        let _ = writeln!(
            s,
            "resolution: {}x{}  fps: {}",
            self.width, self.height, self.fps
        );
        let _ = writeln!(
            s,
            "playhead: frame {} of {}  {}",
            self.frame,
            self.frame_count,
            if self.playing { "PLAYING" } else { "stopped" }
        );
        let _ = writeln!(s, "scenes: {}", self.scenes);
        let _ = writeln!(s, "tool: {}  brush: {}", self.tool, self.brush);
        let _ = writeln!(s, "stage: {}", self.stage.unwrap_or("<custom>"));
        let _ = writeln!(
            s,
            "screen: {:.0}x{:.0} pt  @ {:.2} px/pt",
            self.screen.width(),
            self.screen.height(),
            self.pixels_per_point
        );
        let _ = writeln!(s, "panes ({}):", self.panes.len());
        for name in &self.panes {
            let _ = writeln!(s, "  {name}");
        }
        s
    }
}
