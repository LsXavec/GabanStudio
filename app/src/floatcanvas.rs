//! LENS-DOCK Phase 5 step 2: the EDITABLE canvas as a real OS window.
//!
//! Deliberately the simplest correct architecture, not the design doc's
//! full detach/re-dock model: an IMMEDIATE viewport (not deferred), so the
//! callback runs INLINE in the same call as everything else this frame and
//! can borrow `&mut CanvasView` / `&mut AppState` / `&mut PaintLayer`
//! directly — no `'static` bound, no Arc<RwLock> snapshot, no command
//! channel. `CanvasView::ui` is called VERBATIM, the exact call the dock's
//! Canvas tab already makes, so every tool, guard, and the wet-stroke pen-up
//! commit law work completely unchanged. There is exactly one CanvasView;
//! it just renders into a different window some frames.
//!
//! Trade-off, disclosed rather than hidden: an immediate viewport's repaint
//! is tied to the main window's frame (the design's deferred-viewport
//! rationale — independent repaint cadence for a pen display running at a
//! different refresh rate than the main monitor — does not apply here).
//! If real-rig testing shows a latency problem, upgrading this one window
//! to deferred + snapshot is the next step; escalate only on evidence, per
//! this project's law.
//!
//! Native tablet input (octotablet/RealTimeStylus) is bound to the MAIN
//! window's HWND at startup (see main.rs's tablet-thread spawn) — pen
//! strokes on THIS window fall back to egui's own Touch-event path (the
//! original, still-tuned M0 pipeline; real pressure, just not the enhanced
//! native tilt data). Worth confirming on the rig: does the brush still
//! feel right when drawn on the pen-display window specifically?

use eframe::egui;

/// Whether the canvas is currently undocked into its own OS window. Plain
/// bool (not Arc'd) — an immediate viewport needs no data to outlive this
/// frame's call.
#[derive(Default)]
pub struct FloatCanvas {
    pub open: bool,
}

impl FloatCanvas {
    pub const TITLE: &'static str = "AnimStudio — Canvas";
    pub fn viewport_id() -> egui::ViewportId {
        egui::ViewportId::from_hash_of("animstudio_float_canvas")
    }
}
