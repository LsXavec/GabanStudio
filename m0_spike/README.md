# M0 Spike — Stack Validation

Proves (or kills) the chosen stack — **Rust + wgpu + egui, immediate mode, uncapped** — against
the two hard requirements before any feature code gets written:

1. **240+ fps UI** with a heavy node graph on screen
2. **Low input-to-photon latency** with real pen pressure (Windows Ink)

## Run

```
cargo run --release
```

(Debug builds are fine for iterating but measure fps ONLY in release.)

## What's in the window

- **Top bar** — live FPS (green ≥240), avg / p99 / worst frame ms, node-count buttons
  (100/500/1000/2000), offscreen-culling toggle, painted-node counter.
- **Center** — custom node editor (the ComfyUI-style proof): drag empty space to pan,
  wheel to zoom at cursor, drag nodes to move them, drag from a port to wire nodes together.
  Node names are mock pipeline stages (Genga, Douga, Paint, Composite…).
- **Right** — pen canvas. Draw with the tablet pen. The PRESSURE line flips to
  green "REAL" the moment a Windows Ink pressure value arrives. Event counters show
  touch-events/s vs pointer-events/s (a pen typically reports 133–266 events/s —
  more than the frame rate = sub-frame input data arriving correctly).
  The red crosshair tracks the raw pointer: the visible gap between crosshair and
  stroke head while drawing fast = perceived lag.
- **Bottom** — per-frame time bars with 240fps / 120fps threshold lines.
  Green bars = frame under 4.17ms.

## Pass/fail gates

| Gate | Pass | Meaning |
|---|---|---|
| FPS @ 500 nodes | ≥ 240 sustained (green bars) | immediate-mode + wgpu holds the budget |
| FPS @ 2000 nodes, culling ON | ≥ 240 | culling strategy works; graph scale is a non-issue |
| Pen pressure | "PRESSURE: REAL" appears | Windows Ink reaches egui as Touch+force — no native-input detour needed |
| Pen feel | stroke head hugs the crosshair | input-to-photon latency acceptable at this layer |

**Note on fps numbers:** with vsync off (AutoNoVsync) the fps counter can read far above
your monitor's refresh — that headroom number is the real result (e.g. 800 fps rendered =
~1.2ms frame budget used of the 4.17ms available). Your monitor caps what you *see*.

## If a gate fails

- FPS low → check GPU actually in use (integrated vs discrete), try culling on, then
  profile tessellation; worst case the fallback stack is documented in
  `../research/tech-stack-decision.md`.
- No pressure → winit is not delivering pen as Touch on this machine; M0.1 adds the
  `octotablet` crate (raw Windows Ink / RealTimeStylus) — a known, bounded fix, not a
  stack change.

## Files

- `src/main.rs` — app shell, panels, uncapped repaint loop, AutoNoVsync surface config
- `src/node_graph.rs` — custom node editor (~350 lines): pan/zoom/drag/wire, culling,
  deterministic layout. This grows into the real graph UI.
- `src/fps.rs` — frame-time ring buffer + bar plot
- `src/canvas.rs` — pen strokes with pressure + input diagnostics
