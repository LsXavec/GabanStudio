# Raster Brush Engine — Architecture

**Date:** 2026-07-17
**Method:** multi-agent derivation — 5 research areas (76 findings on GPU dab rendering, tile storage/undo, brush dynamics, Krita/MyPaint/Photoshop formats, and the anim-core pivot), 12 load-bearing claims adversarially verified against wgpu/egui/Krita/MyPaint sources.
**Decision context:** user committed to a raster (Krita-parity) drawing model replacing M2 vector strokes, with Krita brush import eventually.

## Core decision: hybrid, headless-preserving

Pivot to raster **without breaking the engine's invariants** by keeping the split the codebase already anticipates:
- **anim-core stays the headless data/plan/hash authority** — pixels live in CPU RAM as plain bytes (`Arc<TileData>`), never wgpu types. Headless CI + golden tests untouched.
- **A new `anim-gpu` module/crate (app-side) owns all wgpu** — Device/Queue, the brush rasterizer, tile textures, and a graph-executing compositor keyed by the engine's existing `u64` content hash.
- **Hybrid `Drawing`, not pure raster:** `Drawing { id, name, layers: Vec<Layer> }` with `enum Layer { Raster(RasterLayer), Vector(VectorLayer) }`. New drawings default to one RasterLayer (raster IS the drawing model), but vector isn't ripped out — it stays resolution-independent for retiming/cleanup and reuses the validated pen pipeline. Old `strokes` migrate to one `Layer::Vector`; schema v3→4, old files still load.

## The load-bearing seams

**Content hash.** Keep `Drawing::content_hash() -> u64`, but fold per-layer: vector = existing stroke byte-fold; raster = fold over sorted `(tile_coord, tile.hash)` where each tile's `hash` is fnv1a over its premultiplied bytes computed **at paint-commit time on only the dirty tiles** (O(dirty), not O(canvas)). Deterministic + content-addressable → identical repaints dedupe, caches survive save/load, and the `recipe→hash→cache-invalidation` chain stays intact. (Not a monotonic counter — that isn't content-addressable and cold-starts caches after load.)

**Undo.** New `Command::PaintTiles { at, id, layer, diff: Vec<(coord, before: Option<Arc<TileData>>, after: Option<Arc<TileData>>)> }`. Capture each tile's `before` Arc (refcount bump, O(1)) the first time a stroke touches it, `after` at commit. Its exact inverse is the same diff with before/after swapped. **Drops into the existing apply/undo/redo with ZERO Engine change** — it's just another Command with an exact inverse, like AddStroke. One undo step costs memory ∝ painted area (~16–32 KiB/tile), independent of canvas size. Reuses `stroke_invalidation_roots` verbatim.

## GPU pipeline (wgpu 29 + egui 0.35)

- The app is **already** the wgpu renderer (eframe `features=["wgpu"]`); grab `cc.wgpu_render_state` in `App::new`, clone the shared `device`/`queue` — never create a second Device.
- **Layer texture:** `Rgba16Float` (linear, no 8-bit banding on soft gradients), usage `RENDER_ATTACHMENT | TEXTURE_BINDING` (+`COPY_SRC/DST` for tile readback/undo). wgpu-29 note: `RenderPassColorAttachment.depth_slice: None`.
- **Dab = instanced premultiplied soft-round quad** (`VertexStepMode::Instance`), per-instance `{center, radius, angle, aspect, color, flow, hardness}`. Fragment: `rr = dot(local,local)/radius²`, MyPaint two-segment hardness falloff, `fwidth` rim AA. `draw(0..4, 0..dab_count)` → thousands of dabs/stroke on fixed-function blend.
- **Anti-darkening (the core correctness rule):** dabs within one stroke accumulate coverage by **MAX**, not summed 'over'. Wet buffer = coverage-only texture, blend `alpha: {One, One, Max}`, `write_mask: ALPHA`; composite the wet coverage **once** at stroke opacity on pen-up via `PREMULTIPLIED_ALPHA_BLENDING`. Ink (opacity=1) can skip the wet buffer and composite 'over' directly.
- **Display:** render into the layer texture on **input events** (not every UI frame) via own encoder + `queue`; show it in the canvas with `painter.image(texture_id, paper_rect, …)` where `texture_id` comes from `Renderer::register_native_texture`. Premultiplied alpha end-to-end; Nearest at 100%, Linear when scaling.
- **Compositor (biggest structural change, Phase 2+):** replace canvas.rs's ad-hoc per-column stroke loop with a GPU compositor that **executes the node graph** — DrawingSource→tiles-as-texture (cache key = the engine's `ImageDesc.hash`), Transform→sampling pass, Blend→blend shader, Output→frame. This is where engine-plan and real-pixels converge (today they're two decoupled worlds).

## Tiles & brush model

- **Tile = 64×64.** RGBA16 premultiplied `TileData { rgba: Box<[u16; 64*64*4]>, hash }` (Krita/MyPaint precision parity; RGBA8 halves RAM but bands on repeated wash). `Arc<TileData>` + copy-on-write via `Arc::make_mut`. Empty layer ≈ free (empty map + one shared empty tile). Crates: `rustc-hash`, `bytemuck`, `lz4_flex`.
- **CPU-master tiles** (authoritative in RAM; upload dirty tiles for display; undo needs no readback). If painting on GPU, read back dirty rects **once** at stroke commit through a 3–4 buffer `MAP_READ` staging ring polled a frame later — never `poll(Wait)` inline (stalls).
- **Brush preset** (serde, mirrors MyPaint base+inputs / Krita target+sensor+curve): `BrushPreset { name, base: BrushBase{size, opacity, flow, hardness, spacing=0.1, roundness, angle, scatter, …}, dynamics: Vec<Dynamic{target: Param, input: Sensor, curve, combine}> }`. Sensors: Pressure/Speed/Direction/Tilt/Random/Fade/Distance/Time. One combine convention (multiply gains, add offsets). Three shipped feels via a sensor-routing table: **PENCIL** = Pressure→Opacity, hardness ~0.9; **INK** = Pressure→Size, opacity=1, tight spacing; **SOFT PAINT** = Pressure→Size+Flow, low hardness, wash on. Spacing: `step = spacing_frac*diameter`; default 0.1 ⇒ ~5 dabs/radius.

## Brush import (honest fidelity)

Normalize every format into one `ImportedBrush { tip: Option<Image>, diameter, spacing_ratio, angle, roundness, hardness, flow, opacity, pressure_curves… }`, then into `BrushPreset`. Sequence import **behind the engine features it needs**: MyPaint `.myb` + Krita `auto_brush` (procedural → our round dab) first; then GBR/GIH (needs a sampled-mask tip primitive); then ABR sampled (PackBits + partial ActionDescriptor, port GIMP's abr.c); then Krita `.bundle` (ZIP of real tip files — prefer over fragile standalone `.kpp`). Centralize unit/polarity normalization (GIMP spacing %, Krita fraction, ABR %; Krita angle radians, ABR degrees; big-endian headers; mask polarity). **Fidelity ceiling to state in-product:** "imports the brush tip + basic size/spacing/pressure dynamics as an approximation, not a byte-for-byte behavioral clone." GBR/GIH near-lossless; ABR faithful tip + approximate dynamics (~7 categories unrecoverable); KPP/bundle = tip + scalars only; MyPaint approximate unless libmypaint FFI.

## Phased implementation

| Phase | Deliverable | Effort |
|---|---|---|
| **0 — GPU scaffold** | anim-gpu module; grab `cc.wgpu_render_state`, one instanced-quad pipeline + one Rgba16Float texture, render a quad, display via `register_native_texture`. Proves the shared-context + native-texture path. | S |
| **1 — Paint pressure pixels** | Reuse existing pen capture; walk polyline → soft-round dabs at spacing with pressure→radius; instanced premultiplied dabs (MyPaint falloff + fwidth AA) into one Rgba16Float layer; display in canvas. Ink 'over' at opacity=1, no wet buffer, no tiles, no engine change, no undo. **Result: GPU pressure ink on screen.** | M |
| **2 — Tiles in engine + undo + graph display** | Hybrid Drawing + Layer enum; RasterLayer tiles + per-tile hash in content_hash; PaintTiles command with tile-diff inverse (no Engine change); readback dirty tiles at commit; GPU compositor executes the graph; SQLite tiles table, schema v4. **Raster strokes undoable, persist, flow through the graph.** | L |
| **3 — Wet buffer / wash + flow + eraser** | Coverage wet buffer (Max blend), composite-once; flow<1; eraser. Kills intra-stroke darkening. | M |
| **4 — Brush dynamics model** | BrushPreset serde + curve eval + sensors + 3 built-in presets + curve editor. | L |
| **5 — Brush import** | ImportedBrush + normalization; MyPaint→GBR/GIH→ABR→bundle, in that order. | XL |
| **6 — Memory/scale hardening** | lz4 cold tiles, content-addressed dedup, undo depth cap, mipmaps, incremental SQLite saves, optional rayon. | L |

## Key risks
- **Precision:** RGBA16 working layer round-tripped through RGBA8 tiles bands on wash — use RGBA16 tiles.
- **Readback stalls:** never inline `poll(Wait)`; staging ring polled a frame later, batched per stroke.
- **Hash determinism:** per-tile hash must be byte-identical across save/load/platforms or golden tests + caches silently corrupt.
- **Compositor convergence** (canvas currently bypasses the graph entirely) is the largest hidden change — don't attempt before Phase 2 is stable.
- **Undo blow-up:** long strokes touch many tiles; bound history RAM (depth cap + compression) from Phase 2.
- **Headless-law discipline:** no wgpu type may leak into anim-core.
- **Resolution-independence tension:** raster tiles are pixel-bound but the app's paper is resolution-independent — needs a policy (see open questions).
- **Sensor gap:** egui only exposes Touch force; tilt/barrel/rotation need raw winit tablet events upstream.

## Open questions for the user
1. **Pure raster vs hybrid** — recommendation is KEEP vector as an optional non-default layer (resolution-independent retiming is the app's core value). Hard-delete vector, or keep alongside?
2. **Tile precision** — RGBA16 (parity, clean wash, 32 KiB/tile) vs RGBA8 (half RAM, bands on low-flow layering)?
3. **Content identity** — content-addressed per-tile hash (recommended) vs monotonic version counter?
4. **Tile master** — CPU-master (recommended, no readback stalls) vs GPU-master?
5. **Resolution-independence** — should paint resolution == project resolution, and how should zoom past 100% behave (upscale tiles vs repaint at higher res)? Directly conflicts with the app's resolution-independence.
6. **Import priority + fidelity bar** — which source app first (MyPaint maps cleanest; ABR most-requested but hardest)? Is "tip + basic dynamics approximation" acceptable, or is libmypaint FFI needed for exact MyPaint parity?
7. **Dab richness** — need textured/multi-color/stamped dabs (needs a compute AlphaDarken path), or is procedural soft-round + one sampled tip enough (cheap fixed-function Max)?
8. **Threading** — single-threaded MVP OK, or rayon worker rendering from the start?
