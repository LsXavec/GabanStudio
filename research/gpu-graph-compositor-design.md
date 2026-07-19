# GPU Graph-Compositor Render Path — design (2026-07-19)

The convergence milestone: the canvas can display the cut's frame **as computed
by `cut.graph`** — DrawingSource/Solid/Transform/Blend/Output actually execute —
instead of only the active drawing's layer sandwich. The node-graph pane stops
being an editor for a graph nothing renders.

## What this fixes today

- **Non-active columns' raster art is invisible** on canvas (only their legacy
  vector strokes draw). The graph path composites every wired column.
- **Export ignores the graph entirely** (plain column-stack composite). With a
  wired graph, export now renders what the graph says.
- Transform/Blend/Solid nodes had **no defined pixel semantics** (evaluation was
  symbolic). This design defines them, in testable CPU math first.

## Architecture (two halves, same law as the sandwich)

1. **Engine (headless, `anim-core/src/export.rs`)** — a pure-CPU recursive graph
   renderer: `render_graph_frame(cut, frame, w, h) -> Option<RGBA8>`.
   - The GOLDEN REFERENCE for the GPU path (mirrors `flatten()` vs the GPU
     sandwich) and the export truth.
   - `None` when the graph has no wired output → callers fall back to the
     column-stack composite (old behavior; never silently blank).
2. **App (`app/src/graphcomp.rs`)** — `GraphCompositor`: executes the graph on
   the GPU into a displayable texture.
   - **Dirty key = the engine evaluator's output Value hash** (`engine.eval`),
     exactly as value.rs promised in M1. Same hash → cached texture, zero GPU
     work. Command-driven invalidation comes free.
   - Post-order walk of the real graph (not the recipe string) with a small
     texture pool; DrawingSource composites reuse PaintLayer's existing
     `composite_layers_into` (scratch + opacity LAW stays single-owner).
   - LAW: uniform writes follow the opacity_buf rule — one write_buffer + one
     submit per pass (graphs are tiny; submits only happen on hash change).

## Pixel semantics (defined here, pinned by CPU golden tests)

- **DrawingSource**: resolve column at frame (holds apply) → `Drawing::flatten()`
  → full-frame premultiplied f32. Identical math to the column composite, so
  `[DrawingSource → Output]` is bit-exact with the legacy export path. A
  MISSING column (deleted while a node still reads it) renders transparent
  everywhere — evaluator (distinct `missing-column` sentinel value, hash stays
  honest), CPU renderer, and GPU compositor all agree.
- **Solid{rgba}**: full-frame premultiplied fill. Color goes through the SAME
  sRGB→linear conversion the brush dab path uses (`linear_rgba`) so a Solid
  matches painting that color; alpha stays straight then premultiplies.
- **Transform{translate, scale, rotate_deg}**: forward map
  `p_out = C + R_θ · s · (p_in − C) + t`, **C = canvas centre**, t in pixels.
  CPU inverse-maps with bilinear sampling (outside the source rect =
  transparent); GPU draws the forward-transformed quad (self-clipping, no
  border-color feature needed). Integer translates are exact. |s| < 1e-6 →
  transparent. KNOWN DEVIATION (review-confirmed, accepted): on FRACTIONAL
  transforms the GPU's ClampToEdge sampler repeats the border texel inside
  the outer half-texel band where the CPU fades to transparent — a sub-pixel
  display-only fringe; the CPU law is the export truth. Revisit with a
  bounds-checked shader if it ever becomes visible in practice.
- **Blend{mode}**: input 1 (top) over input 0 (bottom), premultiplied:
  - Normal: `out = b + a·(1−b.a)`
  - Multiply: `rgb = b.rgb·a.rgb + b.rgb·(1−a.a) + a.rgb·(1−b.a)`, `a = over`
  - Add: `rgb = a.rgb + b.rgb` (clamped at output), `a = over`
  - Screen: `rgb = a.rgb + b.rgb − a.rgb·b.rgb`, `a = over`
- **Output**: pass-through. Unconnected input pins = transparent.

## UI (v1)

- **Composite view toggle**: canvas toolbar button + rebindable key `C`.
  Composite view shows ONLY the graph output (no sandwich, no onion, no vector
  overlays — it is the final-frame truth, a review mode). Painting is refused
  in composite view with a status hint (same refuse+hint pattern as the
  hidden-layer guard) — a stroke must never silently land somewhere the view
  doesn't show.
- Composite view with no wired graph output → status hint + normal edit view.
- Playback follows the toggle (no hidden mode switching).
- Export uses the graph automatically when an output is wired (status says
  which path exported); falls back to the column composite otherwise.

## Phases

1. **CPU reference + export** (engine): renderer, blend/transform math, golden
   tests (solid, all 4 blend modes hand-computed, integer translate, rotate-90
  centre-origin pin, identity, hold resolution, graph≡column equivalence,
  missing output → None).
2. **GPU executor + composite view** (app): graphcomp.rs, toggle/keybind/guard,
   export switch. Verified against the CPU reference.
3. *(later, needs his input)* **Live-edit substitution**: the active drawing's
   DrawingSource slot takes the live sandwich (active + wet) so strokes appear
   through transforms/blends mid-stroke. Until then composite view is a review
   mode and editing happens in edit view.
4. *(later)* per-node preview thumbnails in the graph pane; frame-range texture
   cache for scrub-heavy cuts; sRGB/linear audit if the color pipeline is ever
   revisited.

## Out of scope v1

Masks/mattes, per-node caching of intermediate textures (only the output is
cached), motion blur, camera nodes, vector-stroke rasterization in DrawingSource
(same honest limitation as export today).
