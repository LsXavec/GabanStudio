# Tech Stack Decision — Node-Graph 2D Animation Suite

> **VALIDATED 2026-07-17 — M0 spike passed every gate.**
> UI build time **0.15 ms/frame** with the 500-node graph + pen canvas + plots on screen
> (~3.6% of the 240fps budget; ≈6,000+ fps uncapped — the observed 236fps was purely the
> user's NVIDIA Max Frame Rate driver cap). Pen pressure arrives natively as Windows Ink
> touch+force on the XP-Pen Artist 22 Pro 2nd Gen — no native-input workaround needed,
> sub-frame pen event rates confirmed. Stack is locked in; fallbacks below are historical.
> Hardware note: dev machine pairs a high-refresh main monitor (node graph/timeline) with a
> 60Hz pen display (drawing surface) — latency work targets the pen display, fps targets the
> main monitor. Spike code lives in `../m0_spike/` (eframe 0.35, wgpu 29); its custom node
> editor (~350 lines) is the seed of the production graph UI.

**Date:** 2026-07-15
**Requirements driving this decision:**
- Node-graph-centric UI (ComfyUI-grade feel: bezier wires, groups, minimap, fast pan/zoom)
- Blender-style workspaces, one per pipeline stage (Script → Ekonte → Layout → Genga → Sakkan → Douga → Shiage → BG/3DCG → Satsuei → Edit → Sound → Export)
- 240+ fps UI on high-refresh monitors; low input-to-photon latency (the real requirement)
- Pro-grade tablet input on Windows (pressure, tilt, high-frequency pen events)
- Solo-developer maintainable, AI-assisted development
- GPU-accelerated canvas, real-time playback

---

## Verdict: Rust + wgpu + egui (immediate-mode), custom node editor

### The stack

| Layer | Choice | Why |
|---|---|---|
| Language | **Rust** | Native performance, no GC pauses (GC = latency spikes mid-stroke), memory-safe, single-exe shipping. Compiler catches an entire class of bugs — the right property for AI-assisted solo dev where Claude writes most code and `cargo check` verifies it. |
| GPU | **wgpu** | Rust-native WebGPU implementation over D3D12/Vulkan/Metal. One API, all platforms. Compute shaders available for brush stamping, fills, and future AI inference interop. |
| UI | **egui** (immediate mode) + **egui_dock** | Immediate mode redraws the whole UI every frame in <1–2 ms on any modern GPU → 240Hz saturated by default, no dirty-rect bookkeeping, no retained-tree invalidation bugs. egui_dock gives Blender-style dockable/tabbed workspaces. |
| Node graph | **Custom widget** (start from `egui_snarl` as reference) | A node editor is ~2–4k lines. Owning it is the difference between "ComfyUI-like" and "compromise": custom wire rendering, node previews (thumbnail of each stage's output ON the node), groups/frames, reroute dots, minimap. This widget IS the product's identity — don't rent it. |
| Tablet input | **Windows Ink via `octotablet` / raw WM_POINTER** | Pressure + tilt + pen history events (coalesced high-frequency points via GetPointerPenInfoHistory) → smooth strokes even between frames. |
| Media/export | **ffmpeg (sidecar or lib)** | MP4/ProRes/PNG-sequence export, audio decode for scrub. |
| Project format | **SQLite file + content-addressed frame blobs** | Autosave = transactions. Crash recovery free. Diffable, inspectable, never one corrupt monolith. |

### Core architectural law: the document IS a node graph

One underlying graph per project. **Workspaces are views onto regions of that graph**, not separate modes:

- A **Cut** is the atomic unit (matching the anime pipeline) — internally a subgraph: `xsheet → drawing layers → paint → composite`.
- Pipeline stages = node categories. The Genga workspace is the graph filtered to drawing nodes with a canvas front-and-center; Satsuei workspace is the full compositing graph; Edit workspace is a sequence of cut-output nodes on a timeline.
- "Seamless transitions" becomes literal: switching workspace never converts data, it re-lenses the same graph.
- Evaluation is **lazy + cached**: each node caches its output texture; edits dirty only downstream nodes; playback pulls frames through the graph. This is how Nuke/Fusion stay interactive and it's what makes 240fps possible with heavy content — the UI never re-renders what didn't change.
- Engine/UI split: the document model + graph evaluator is a **headless Rust library crate** (unit-testable, scriptable, future CLI renderer), the egui app is a thin shell over it. This is the single highest-leverage architecture decision for long-term velocity.

### Threading model

- UI thread: input + egui paint only, never blocks.
- Worker pool: graph evaluation, brush stroke tessellation/stamping, ffmpeg encode.
- GPU: one queue for UI, compute for canvas ops; frame pacing on the UI thread targets the monitor's refresh, uncapped mode for benchmarking.

---

## Alternatives considered and why they lost

**Godot 4 (runner-up).** Genuinely strong: GraphEdit node UI built in, pen pressure/tilt on Windows Ink, GPU 2D renderer, and Material Maker + Pixelorama prove the "creative tool built in Godot" pattern. Fastest path to something visible on screen. Lost on: ceiling, not floor — engine-shaped constraints (text/IME quirks, GraphEdit is customizable but you're skinning someone else's widget), heavier runtime, and less precise control over frame pacing and input latency than owning the loop. If Rust progress ever stalls badly, this is the fallback and it's a good one.

**C++ + Qt (the industry incumbent).** Krita, OpenToonz, and Harmony are all C++/Qt — it's proven for exactly this app category, with the best-in-class tablet stack. Lost on: solo iteration speed (build times, CMake, memory bugs), retained-mode painting model fights the 240fps-everywhere goal, and licensing friction. It's what you'd pick with a 10-person team in 2015.

**Web stack (Electron/Tauri + React Flow + WebGPU canvas).** ComfyUI itself is web (litegraph.js), React Flow is superb, iteration is fastest, and Figma proved WASM+GPU web apps can be pro-grade. Lost on the two hard requirements: browser compositor adds 1–2 frames of latency between pen event and pixel (pros feel it in a drawing tool even when it's fine in a diagram tool), GC pauses cause stroke hitching, and sustained 240fps with a heavy DOM around the canvas is fighting the platform. Right choice for a collaborative/cloud version later; wrong choice for the core drawing engine.

**Flutter.** Impeller is fast, but desktop pen pressure support is immature and node-graph tooling is absent. Not built for this.

---

## Design process (how we build, not just what with)

1. **M0 — Stack validation spike (~1 week equivalent).** One window: egui + wgpu, uncapped fps counter, a node graph with 500 dummy nodes (pan/zoom/wire-drag), and a pen canvas drawing raw pressure strokes. Measure: fps on your monitor, pen-to-pixel latency. **Gate: if this doesn't hit the numbers, we change stack before writing feature code.** Sunk cost: days, not months.
2. **M1 — Engine skeleton.** Headless crate: document model (project → scenes → cuts → nodes), graph evaluator with caching, undo (command-based), SQLite persistence, golden tests.
3. **M2 — The X-sheet + drawing loop.** One cut end-to-end: draw frames on canvas, time them on an exposure sheet, onion skin, playback. This is the soul of the app — everything else decorates it.
4. **M3 — Paint + composite.** Color models, gap-closing fill, the satsuei graph (blend/blur/glow/camera), per-cut render.
5. **M4 — Workspaces + pipeline flow.** Dock layouts per stage, cut status board, the seamless stage-to-stage walk.
6. **M5 — I/O.** ffmpeg export, audio scrub, XDTS import/export (industry interop from the research), scan/trace import.

Each milestone ships something usable; features from the research's 15-row table slot into M2–M5.

## Open items

- Node editor visual language (ComfyUI-style rounded nodes vs Nuke-style compact) — decide during M0 spike with real mockups.
- Script/ekonte workspaces (text + storyboard panels) are UI-heavy, graph-light — design them as special node types (a Cut's storyboard frame is just an input node upstream of layout).
- AI-assist (inbetween gen, line correction, auto-color) — architecture reserves compute-shader / ONNX-runtime room, but no AI work before M5.
