# Performance Settings — Krita-derived spec

**Date:** 2026-07-17
**Method:** multi-agent derivation (Krita Performance docs + Display/canvas-accel docs + read of the user's real `kritarc` + our-code mapping; 47 findings, 8/10 verified). Full source under this run.

## Principle: wire what maps, omit what doesn't

Krita's Performance + Display tabs have ~25 knobs. Only **4** genuinely map to our eframe 0.35 / wgpu 29 / engine stack. The rest are Krita-internal (image RAM manager, on-disk swap, animation cache, SIMD/driver workarounds, OpenGL texture-buffer). Faking them as dead controls would mislead, so they're **omitted**.

## Settings window layout (Krita-like)

Split the existing `egui::Window("Settings")` into a **left category sidebar** (`SidePanel::left`, ~150px: "Keyboard Shortcuts", "Performance") + a right content page (`CentralPanel::show_inside`, `match category`). Each page owns its own scroll area + Reset button. Config gains a `#[serde(default)] perf: PerfConfig` so existing shortcuts-only `config.json` still loads.

## Performance tab

**Canvas Acceleration**
| Setting | Control | Default | Wiring |
|---|---|---|---|
| V-Sync | checkbox | off (AutoNoVsync) | present_mode at startup — **applies after restart** (live `set_wgpu_surface_config` is a no-op in eframe 0.35: the Frame's SurfaceConfig clone is never read back by the painter) |
| Frame latency | Low (1) / High-throughput (2) | Low | `desired_maximum_frame_latency` at startup — applies after restart |
| Canvas scaling filter | Nearest / Linear | Linear | **live** via `Renderer::update_egui_texture_from_wgpu_texture(device, view, filter, tex_id)` |
| Renderer | read-only label | — | shows the active wgpu backend (D3D12/Vulkan); informational, not a control |

**Performance**
| Setting | Control | Default | Wiring |
|---|---|---|---|
| Max FPS while painting | slider 30–240 | 100 | **live** — `ctx.request_repaint_after(1/fps)` replacing the unconditional `request_repaint()`; during playback use `max(cap, project_fps)`; input events still repaint immediately |
| Show FPS overlay | checkbox | off | **live** — frame-time counter drawn in the canvas corner |
| Undo history limit | int (0 = unlimited) | 200 | **live** — engine change: `History.undo_stack` → `VecDeque` + `limit`; `Engine::set_undo_limit`; trim oldest in `apply()` (also bounds raster-undo RAM) |

## Concrete APIs (verified)

- **Present mode (startup):** `eframe::egui_wgpu::SurfaceConfig { present_mode: AutoVsync|AutoNoVsync, desired_maximum_frame_latency: Some(1|2) }`. Restrict to Auto* — Immediate/Mailbox need surface support and can fail.
- **FPS cap:** `egui::Context::request_repaint_after(Duration)` (context.rs). Guard: during playback use `max(fps_cap, project_fps)` or playback drops frames.
- **Canvas filter:** `Renderer::register_native_texture(device, view, filter)` at create; `update_egui_texture_from_wgpu_texture(device, view, filter, id)` to switch live. `wgpu::FilterMode::{Nearest, Linear}`.
- **Undo cap:** `VecDeque<Applied>` + `limit`; `while len > limit { pop_front() }` after push in `apply()`; `undo/redo/can_undo` use `pop_back/push_back`. 0 = unlimited. Needs a unit test.

## Omitted Krita-internal settings (do not fake)

RAM Memory Limit (50%), Swap Undo After (2%), Internal Pool (deprecated), Swap file size/location, Multithreading CPU limit, Frame Renderer clones + timeout, Animation Cache backend/size/ROI, Instant Preview / Level of Detail, disable-AVX / AMD / LCMS workarounds, Use Texture Buffer / Large Pixmap Cache, HDR display format, transparency-checker cosmetics, performance logging. Each needs a subsystem we don't have (tile-memory manager, on-disk swap, animation cache, LoD path, HDR surface) or is a Krita OpenGL/CPU internal with no wgpu counterpart.

## Risks
- Live V-Sync toggle is a no-op in eframe 0.35 → must be labeled "applies after restart," not instant.
- FPS cap during playback must not throttle below the project fps.
- Undo-limit is an engine change → unit-test it; use VecDeque (front-drop O(1)).
- All new Config fields need `#[serde(default)]` so old config.json keeps loading.
- Renderer / any unwired row must render visibly disabled with a "not yet wired" hint.
