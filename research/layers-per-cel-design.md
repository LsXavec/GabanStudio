# Layers-per-Cel Design (v1 scope)

Derived 2026-07-18 by a 4-agent design workflow (engine / GPU-app / UX angles + synthesis),
grounded in the codebase at commit 2f9a6dd. Status: awaiting user decisions, then Phase 1.

## Summary

Layers-per-cel ships as an ordered `Vec<CelLayer>` inside `Drawing` — the anime separation (douga line / shiage color / shadow / sakkan correction) WITHIN one cel, orthogonal to X-sheet columns which stay untouched as layers-across-time. Engine model and command semantics come from Design 0 (id-addressed layers, exact-inverse primitives, no merge/duplicate commands — they're batches), renamed with Design 2's `CelLayer` vocabulary to prevent column conflation, plus Design 2's headless-legal CPU `flatten()` as a golden-reference/export tool (Design 0's "no CPU flatten" objection loses: plain bytes don't violate the headless law, and it makes GPU compositing testable). Persistence is schema v5 with NEW table names `cel_layers`/`cel_tiles` (Design 2's PK argument kills Design 1's add-a-column approach); v4 files load as exactly one migrated layer, bit-for-bit. The app side adopts Design 1's Krita-style sandwich projections (below/active/wet/above + scratch: fixed VRAM at any layer count, eraser correctness by construction, onion reuses the projection builder) over Design 2's per-layer CelSlots. The v1 UI is Design 2's: a cel-layers strip at the bottom of the X-sheet panel, a toolbar active-layer chip, A/Shift+A cycling, CSP-style hidden-layer paint guard, RETAS chip colors. Default new cel = ["color","line"] with line active (app-side template; core stays dumb). Four phases, each independently shippable and testable on the XP-Pen rig, engine phase headless-first per project law; ~5–7 focused days total. Every law holds: headless anim-core, exact inverses (verified per-command), v4→v5 forward migration, content_hash drives sync/invalidation (name excluded so rename never invalidates), one gesture = one undo step (batches), minimal solo UI.

## Data model (anim-core)

RECONCILED MODEL (winner per conflict noted). All in crates/anim-core/src/.

ids.rs: `id_type!(LayerId)` — allocated from project.next_id like every entity. Design 0's id-addressing wins unanimously over indices: stale-UI panels fail loudly (UnknownLayer) instead of silently painting the wrong layer; id-addressed PaintTiles stays correct inside mixed structural+paint batches under Engine::apply's rollback; undo across reorder+paint interleaves is trivially exact.

raster.rs: `RasterLayer` becomes a PURE tile surface — `{ tiles: BTreeMap<TileCoord, Arc<TileData>> }`, Default = empty. Opacity MOVES OUT (Design 0 wins over Design 2's "reuse RasterLayer.opacity"): one opacity per layer, one home for it; two opacities on one surface is a footgun. `RasterLayer::content_hash()` drops the opacity bytes (folds (coord, tile.hash) sorted). TILE/TileData/TileDiff unchanged.

model.rs:
```rust
pub struct LayerProps {            // whole struct = SetCelLayerProps payload → trivially exact inverse via mem::replace (Design 0's grouping wins)
    pub name: String,              // "line", "color", "shadow", "correction" — free text
    pub visible: bool,             // default true
    pub opacity: f32,              // 0..=1, applied when compositing this layer into the cel
}                                  // NO `locked` in v1 (Design 2 wins over Design 0): hidden-layer guard + separate layers cover the solo case; alpha-lock is the v1.5 item; add via cheap v6 bump if ever wanted
pub struct CelLayer {              // Design 2's name wins (avoids X-sheet-column conflation); Design 0's structure wins
    pub id: LayerId,
    pub props: LayerProps,
    pub raster: RasterLayer,
}
pub struct Drawing {
    pub id: DrawingId,
    pub name: String,
    pub strokes: Vec<Stroke>,      // UNCHANGED, renders UNDER the whole stack (all 3 designs agree); LayerKind::Vector is a possible v6
    pub layers: Vec<CelLayer>,     // index 0 = BOTTOM. Empty vec = vector-only cel (v4 raster:None case). App policy keeps >=1 on raster cels; ENGINE allows zero (matches remove-column precedent)
}
```
Hashes (Design 0's formulas win): `CelLayer::content_hash()` folds visible(u8) + opacity bits + raster.content_hash() — NAME EXCLUDED so renaming never invalidates the eval cache or forces GPU re-sync. `Drawing::content_hash()` folds strokes exactly as today, then `b"layers"` + each layer hash IN STACK ORDER (reorder changes it, rename doesn't). Verified safe: eval.rs:84 consumes only Drawing::content_hash(), hashes are never persisted, Engine::load starts a fresh Evaluator — only literal-hash tests need updating.

`Drawing::flatten() -> BTreeMap<TileCoord, Arc<TileData>>` (Design 2 wins, scoped down): CPU premult-over of visible layers bottom→top honoring opacity, in u16 — plain bytes, headless law intact. It is the GOLDEN REFERENCE for GPU-compositing tests and the future export/eval path. It is NOT the display or onion path (that's GPU — Design 1 wins there), so Design 2's flatten-hitch risk never materializes.

Accessors: `Drawing::{layer, layer_mut, layer_index}` by LayerId. Cheap duplication for free: tiles are Arc'd and PaintTiles replaces whole entries — cloning a CelLayer is copy-on-write by construction.

lib.rs: `Engine::alloc_layer_id()` beside alloc_drawing_id. History/apply/undo/redo/set_undo_limit untouched; per-layer PaintTiles diffs are the same size as today's so undo RAM bounds hold. error.rs: `EngineError::UnknownLayer(LayerId)`.

## Commands

COMMAND LIST (command.rs) — Design 0's semantics (validate-don't-clamp, remove-then-insert reorder, full-payload prop replace, NO Merge/Duplicate commands) with Design 2's Cel-prefixed names. `stroke_invalidation_roots` (rename to `artwork_invalidation_roots`) is reused by every arm: roots = DrawingSource nodes of columns exposing the drawing. `cut_ref()` gains four arms.

CHANGED:
1. `AddDrawing { at, id, name, strokes, layers: Vec<CelLayer> }` — replaces `raster: Option<RasterLayer>`. Usually the app's default template for user-created cels; carries the ENTIRE stack (ids+props+tiles) when it is the inverse of RemoveDrawing, so redo-stack commands referencing those LayerIds stay valid after undo. Validate: drawing id unused, layer ids distinct. Inverse: RemoveDrawing (unchanged).
2. `PaintTiles { at, id: DrawingId, layer: LayerId, diff: TileDiff }` — inverse still swaps before/after per entry (mechanics untouched). Errors: UnknownDrawing, UnknownLayer (replaces today's "has no raster layer" InvalidCommand at command.rs:188-190).
3. `RemoveDrawing { at, id }` — inverse now `[AddDrawing{..., layers: drawing.layers.clone()}, SetCell×N]` (Arc clone = cheap); restores stack order, LayerIds, tiles, and X-sheet keys exactly.

NEW (all with exact inverses):
4. `AddCelLayer { at, drawing: DrawingId, index: usize, layer: CelLayer }` — insert at index (0=bottom, may equal len). Validate index<=len && layer.id unused. Inverse: RemoveCelLayer{layer.id}. Tiles usually empty; full when inverting RemoveCelLayer.
5. `RemoveCelLayer { at, drawing, layer: LayerId }` — remove by id (UnknownLayer if absent). Inverse: AddCelLayer{index: old_idx, layer: removed_full_clone}. Engine ALLOWS removing the last layer; "keep >=1" is app policy.
6. `MoveCelLayer { at, drawing, layer: LayerId, to_index: usize }` — from=layer_index(layer); require to_index<len; remove-then-insert (so move(i→j) then move(j→i) is an exact identity). Inverse: MoveCelLayer{to_index: from}. VALIDATE, don't clamp — a clamped command's recorded params would differ from its effect and the inverse would not be self-evidently exact.
7. `SetCelLayerProps { at, drawing, layer: LayerId, props: LayerProps }` — apply = mem::replace; inverse = same command with prior props. Covers rename/visibility/opacity in one arm; the app coalesces an opacity drag into ONE apply at gesture end (one gesture = one undo step). Invalidation: emit roots uniformly (rename over-invalidates the eval cache; cheap, refine only if it shows — GPU sync is protected separately because the sync keys use content_hash, which excludes name).

UNCHANGED: AddStroke / PopStroke / SetCell / all graph commands.

DELIBERATELY ABSENT: MergeLayers, DuplicateLayer. Merge-down = `apply("merge down", [PaintTiles{into lower, diff=flatten of the pair minus lower}, RemoveCelLayer{upper}])`; duplicate = AddCelLayer with a cloned layer + fresh id. Both inherit exact inverses and mid-batch rollback from Engine::apply (verified: lib.rs:142-187 rolls back applied prefixes on failure). Not exposed in v1 UI.

App-side composite gestures (each ONE engine.apply = one undo step): paint on empty frame = [AddDrawing(template), SetCell, PaintTiles{active layer}]; first paint on a legacy zero-layer vector cel = [AddCelLayer×template, PaintTiles] — this REMOVES today's "this cel is vector-only" dead end (doc.rs:270-274).

## Schema v5 + migration

SCHEMA_VERSION = 5; accept 1..=5 (existing gate at store.rs:187 unchanged in shape).

NEW TABLES — new NAMES win (Design 2's argument, which also invalidates Design 1's "add layer_id column to tiles"): the legacy `tiles` PK is (drawing_id,tx,ty) and saves run CREATE TABLE IF NOT EXISTS, which would silently keep the old shape on existing files.
```sql
CREATE TABLE IF NOT EXISTS cel_layers(
    layer_id INTEGER PRIMARY KEY, drawing_id INTEGER NOT NULL,
    ord INTEGER NOT NULL,               -- 0 = bottom of stack
    name TEXT NOT NULL, visible INTEGER NOT NULL, opacity REAL NOT NULL);
CREATE TABLE IF NOT EXISTS cel_tiles(
    layer_id INTEGER NOT NULL, tx INTEGER NOT NULL, ty INTEGER NOT NULL,
    bytes BLOB NOT NULL, PRIMARY KEY(layer_id, tx, ty));
```
Legacy `raster_layers`/`tiles` STAY in the SCHEMA constant and their DELETE FROM lines stay in the full-rewrite batch (Design 0 wins over Design 2's DROP — it preserves the uniform CREATE+DELETE pattern with zero schema-object churn, and achieves the same zombie-row purge: re-saving an opened v4 file empties them in the same transaction). Nothing is ever written to them.

SAVE: per drawing, per layer in stack order — one cel_layers row (ord = position) + tiles into cel_tiles. drawings.payload stays strokes-JSON.

LOAD — explicit version branch per drawing:
- version >= 5: SELECT layers ordered by ord; per layer, tiles from cel_tiles.
- version <= 4: keep the existing .ok() tolerance (v1/v2 files may lack raster tables). If a raster_layers row exists → exactly ONE CelLayer { id: LayerId(project.alloc_id()), props { name: <user decision, recommend "paint">, visible: true, opacity: <old row's opacity> }, raster: all legacy tiles rows }. VERIFIED SAFE: next_id is parsed at store.rs:195, before the scene loop at 210, so load-time minting is unique and deterministic. If no raster row → layers = [] (vector-only drawing stays layerless; first paint gesture adds the default stack undoably).
Result: a v4 single-raster drawing opens pixel-identical with its opacity preserved; re-save writes v5. Old builds correctly reject v5 files via the existing SchemaVersion gate — the upgrade is one-way, so show a one-time status-bar note on first save of an upgraded file. Behavioral first: loading a v4 file advances next_id (minted LayerIds) — harmless, the counter only guarantees uniqueness.

CODE-SEAM MIGRATION (no persisted state app-side): every `drawing.raster` touchpoint becomes layer-aware — command.rs (3 changed arms), doc.rs commit_raster/clear_current_raster/drawing_raster/current_raster_tiles/current_raster_key → active-layer targeting + stack providers, canvas.rs `synced` key → three keys, paint.rs per gpuStrategy. Commands and histories are never persisted → no compatibility work there. RasterLayer::content_hash drops opacity bytes; CelLayer::content_hash picks them up; session-only hashes make this invisible outside literal-value tests.

## GPU strategy (sandwich projections)

SANDWICH PROJECTIONS (Design 1 wins over Design 2's per-layer CelSlots). Fixed set of full-canvas Rgba16Float textures regardless of layer count: `below` (composite of visible layers under the active), `active` (the active layer alone, bit-exact — today's cel texture role, machinery untouched), `wet` (unchanged), `above` (visible layers over the active), plus a non-displayed `scratch` staging texture and the existing 2 onion slots. Why it wins: (a) eraser destination-out renders into `active` only — correct by construction, the one thing today's single merged texture cannot do; (b) VRAM is constant in layer count (~111 MiB total at 1080p vs today's ~63; per-layer textures would hit ~205 MiB at 8 layers, ~820 MiB at 4K) so the soft cap of 8 layers is a UI choice, not a VRAM one; (c) constant per-frame render cost (3 quads + wet + 2 onion); (d) the projection builder is exactly the onion-composite builder. The two strategies tie on frame flip (the hot path — all visible layers upload either way, sparse-tile bounded); sandwich wins everywhere else. Cost accepted: switching active layer rebuilds both projections — a click-frequency event, ~N fullscreen blends.

paint.rs: extract `Target { texture, view, tex_id }`; fields become active/wet/below/above + scratch(+bind). `sync_from`→`sync_active`, `clear`→`clear_active` (bodies unchanged). NEW `build_projection(which, layers: &[LayerSlice])`: clear target; per visible layer bottom→top: fill_texture(scratch, tiles) then one fullscreen blend scratch→target via the existing composite_pipeline with opacity_buf = layer opacity. LAW: opacity_buf is ONE uniform buffer (paint.rs:337-342) — issue one write_buffer + one submit PER LAYER; batching passes into one encoder would make every pass read the last-written opacity (switch to dynamic offsets first if ever optimized). `set_onion(slot, layers: Option<&[LayerSlice]>, hash)` — whole-cel composite via the same builder; hash-skip logic unchanged. paint/paint_wet/composite_wet/read_tiles are UNCHANGED beyond renames: eraser targets active only; composite_wet already merges wet into what is now the active texture; read_tiles reads back ONLY the active layer — commits get smaller. ensure_size also recreates below/above/scratch; set_filter also updates below/above registrations.

canvas.rs sync: replace `synced:(u64,u64)` with three keys — `synced_active:(drawing.0, layer.0, layer_hash)` (blank sentinel (u64::MAX, frame, 0)), `synced_below:u64`, `synced_above:u64` (fnv1a fold over visible layers' (id, content_hash) + drawing id; MUST fold the blank/frame sentinel too). One mechanism covers everything: pen-up commit touches only synced_active; undo into a buried layer, visibility/opacity toggles, and reorders change stack hashes; active-layer switch changes all three; frame flip changes the drawing id everywhere. Abandoned-stroke handling extends today's cel_touched reset (canvas.rs:464-467): eraser dabs invalidate synced_active; the raster_new_cel first-dab clear (canvas.rs:481-484) now clears active AND both projections and invalidates all three keys.

Render order (canvas.rs step 3b replacement, bottom→top): paper → other columns' vectors → vector onion ghosts → active column's vectors (raster-over-vector rule kept) → raster onion prev (warm tint, unchanged) → onion next (cool) → below projection (WHITE — per-layer opacities baked in) → active layer tinted `from_white_alpha(active_opacity)` (texture holds full-strength pixels so readback stays bit-exact; same raw o×texel multiply already verified exact at canvas.rs:614-619) → wet buffer tinted `brush_opacity × active_layer_opacity` (both factors or the stroke pops at pen-up) → above projection → brush cursor. Live shiage for free: painting "color" previews under the "line" layer.

doc.rs: `active_layer_slot: usize` counted FROM THE TOP (Design 2 wins over Design 1's bottom index — keeps "line stays active" while flipping under the line-on-top convention), clamped in sanitize(); resolved to a concrete LayerId per operation. `commit_raster` returns Option<(DrawingId, LayerId)>, diffs against the active layer's tiles only. New providers: active_layer_key/tiles, below/above_stack_key, below/above_layers, drawing_composite(id) for onion. New-cel template lives app-side in config (`Vec<LayerTemplate{name, opacity}>`) — core stays dumb; AddDrawing carries whatever the app instantiated via engine.alloc_layer_id().

Budgets: 1080p ~111 MiB (7 full-canvas f16); 4K ~443 MiB — acceptable on the discrete-GPU rig; escape hatch is dropping below/above/onion (display-only) to Rgba8Unorm without touching the bit-exact active/wet path. Frame flip: sparse tile uploads (proportional to inked area, as today) + ≤N blends per projection — well inside the M0-validated 0.15 ms/frame class budget.

## UI v1

Design 2's UI wins nearly wholesale (it's the researched, solo-minimal one); Design 1 contributes the soft cap.

CEL LAYERS STRIP — bottom of the left X-sheet panel (time axis above, inside-of-one-cel below; placement is user decision #7). Rows top-to-bottom = stack top-first (Krita/CSP convention). Row = [eye] [color chip] [name] [opacity %]. Click row = activate (accent highlight + bold — CSP's sync-highlight answer to "which layer am I on"); double-click = rename; opacity drag coalesces to ONE SetCelLayerProps at gesture end. Buttons: [+ ▾] menu (shadow / highlight / correction / empty — inserted at the anime-correct position: shadow+highlight above color and below line, correction above line), [–] (app refuses on last layer, like column removal), [↑][↓] (MoveCelLayer; no drag-reorder in v1). Chips use RETAS trace-line colors as an orientation aid (cosmetic only): line=near-black, color=green, shadow=blue, highlight=red, correction=orange. Held/empty frame shows a grayed "new cel will get color + line". Soft cap 8 (add disables).

CANVAS TOOLBAR CHIP: `▣ line` in the layer's chip color, next to the brush controls where the pen already looks; click = cycle. Turns red `▣ line (hidden)` when the guard would fire.

KEYBINDS (rebindable like everything else): `A` = CycleCelLayer, `Shift+A` = cycle back (sits by the S/F step cluster, left-hand safe). `D` (ClearCel) now clears the ACTIVE layer only; "clear all layers" moves to the toolbar/menu.

GUARD (CSP behavior, not Krita's): pen-down while the active layer is hidden → refuse the stroke, status "active layer 'line' is hidden — press A to switch or click its eye". Never silently paint into an invisible layer (and never silently un-hide — user decision #5).

DEFAULTS: new cel = ["color","line"], active = line (draw douga on top, paint shiage under it without touching the line — the merged solo pipeline). No blend modes, no masks, no groups, no lock column — the strip is one row per layer and five controls total.

## Phases

### Phase 1 — Engine: CelLayer model, commands, hashes, flatten, schema v5 (headless-first) + mechanical app parity

anim-core: ids.rs LayerId; raster.rs opacity removal; model.rs CelLayer/LayerProps/Drawing.layers + content_hash formulas + flatten(); command.rs 3 changed + 4 new arms + UnknownLayer; lib.rs alloc_layer_id; store.rs v5 + v<=4 one-layer loader. Headless tests FIRST per project law: 3-layer save/load round-trip (PartialEq end-to-end); v4 fixture built in-test via rusqlite loads as one-layer drawings with opacity preserved, vector-only loads layerless, re-save/re-load stable; exact-inverse fuzz over random Add/Remove/Move/SetProps/PaintTiles interleavings (undo-all == initial, redo-all == final); RemoveDrawing with 3 layers + exposures restores stack order, LayerIds, tiles, X-sheet keys; hash law (paint/reorder/visible/opacity change Drawing::content_hash, rename does NOT); merge-down batch with a bad id mid-batch leaves the document untouched; flatten golden values (opacity, overlap, missing tiles). Then the MINIMAL app adaptation so the workspace compiles with behavior identical to today: new cels get ONE default layer, commit/clear/sync/onion target layers.last(), no new UI.

- **Effort:** 1.5–2 days (low uncertainty — every mechanism follows an in-repo precedent)
- **Testable:** cargo test green with zero GPU (engine law). Then on the rig: open an existing v4 .animproj — everything pixel-identical; paint, erase, undo, save; reload the now-v5 file — identical again.

### Phase 2 — Sandwich compositor + per-layer editing (2-layer template, keybind + chip, no strip yet)

paint.rs: Target extraction; below/above/scratch; build_projection with the one-submit-per-layer opacity law; sync_active/clear_active renames; set_onion re-targeted to whole-cel layer lists; ensure_size/set_filter extended. canvas.rs: three sync keys with the blank-frame sentinel folded into ALL of them; render order per gpuStrategy (active tinted by layer opacity, wet by brush×layer); raster_new_cel first-dab clear extended to all three targets/keys; commit resync from the (DrawingId, LayerId) return. doc.rs: active_layer_slot (from top, clamped in sanitize); layer-aware providers; commit_raster re-target; new-cel ["color","line"] template as one batch; first-paint-on-legacy-vector-cel auto-adds the stack in one batch (removes the vector-only dead end). Minimal switching UI so it's drivable: A/Shift+A cycle + toolbar chip.

- **Effort:** 2–2.5 days (the paint.rs refactor is the largest single piece of the feature)
- **Testable:** On the rig: draw line art, press A, paint color — it previews UNDER the lines live; eraser on color never touches line; pen-up commit is ONE undo step (Ctrl+Z on an empty-frame stroke leaves no orphan); flip frames — active layer stays 'line'; undo into a buried layer updates the display; onion ghosts show the whole-cel composite; abandoned mid-playback stroke leaves no phantom pixels.

### Phase 3 — Cel Layers strip + management + guard

xsheet_panel.rs strip per uiV1 (rows, eye, chip, rename, coalesced opacity drag, + ▾ menu with anime-correct insert positions, –, ↑/↓, soft cap 8, held-frame preview text). Hidden-layer paint guard with the actionable status message. D clears active layer only; 'clear all layers' toolbar item. All mutations through the new commands so undo covers every strip action.

- **Effort:** 1–1.5 days
- **Testable:** On the rig: add a shadow layer from the menu (lands above color/below line), reorder with ↑/↓, rename, toggle eyes, drag opacity — every action undoes as one step; pen-down on a hidden layer refuses with the hint; where-am-I is answered by chip + highlight while actually coloring a scene.

### Phase 4 — Migration soak, edge cases, perf

Open every real v4 project on disk: visual identity, then save→v5→reload identity; one-time status note on first save of an upgraded file. Edge cases: navigating between two blank held frames (sentinel in all three keys — the historical stale-pixel bug's descendant), held-frame display rule over a whole stack, zero-layer legacy vector cels in the strip and commit paths (no panics), uneven-stack clamping. Perf: frame-flip with 3 fully-painted layers at project res, layer-switch rebuild feel, VRAM check. Fix what the soak finds.

- **Effort:** 0.5–1 day
- **Testable:** His own old project files round-trip; a full animate-then-color session (rough → line → color under → shadow) on the XP-Pen with no stale pixels, no hitches on S/F stepping, and undo behaving at every step.

## Risks

- Sync-key staleness regressions — the historical stale-pixel area (u64::MAX sentinel exists because of a real bug) now has THREE keys, and the blank-frame sentinel must appear in all of them or a stale projection survives navigation between blank frames. Mitigation: keys computed only by doc.rs providers (single source of truth), cel_touched/new-cel invalidation extended to all three, dedicated Phase 4 checklist (frame switch, undo, layer cycle, hide/show, blank↔blank navigation).
- opacity_buf uniform aliasing in build_projection — one uniform buffer means batching N layer blends into one encoder makes every pass read the last-written opacity. Mitigation: one write_buffer+submit per layer, documented as law at the method; any future batching must switch to dynamic uniform offsets or per-layer bind groups first. Layer-switch frequency makes per-layer submits cheap.
- Wrong-layer footgun (frame × layer = two editing targets; erasing line while fixing color). Mitigation shipped in v1: toolbar chip in the pen's sightline, strip highlight, hidden-layer guard, eraser physically scoped to the active texture. Dim-others/isolate is the v1.1 answer if confusion shows in real use — not more chrome now.
- VRAM at 4K: ~443 MiB of full-canvas f16 (7 textures). Fine on the discrete rig at 1080p (~111 MiB); mitigation path if a 4K project pinches: below/above/onion are display-only and can drop to Rgba8Unorm without touching the bit-exact active/wet readback path.
- Hash-formula change: any test asserting literal v4 hash values breaks (update them); visible/opacity toggles now correctly invalidate eval + GPU sync — verify no resync feedback loop when toggling visibility mid-stroke (the sync block only runs between strokes, but the interaction deserves a test).
- Display-tint exactness now has TWO consumers (wet preview AND active-layer opacity) relying on egui-wgpu 0.35 sampling native textures raw so from_white_alpha is a pure multiply (verified today at canvas.rs:614-619). An egui upgrade changing egui.wgsl breaks both silently — re-verify on every egui bump (add to the upgrade checklist).
- One-gesture undo across bigger batches (AddDrawing 2-layer template + SetCell + PaintTiles; AddCelLayer×2 + PaintTiles on legacy vector cels) — inverse ordering must restore exactly. Mitigation: Engine::apply's prepend-inverses rollback is already proven (lib.rs:142-187); add a history round-trip test per batch shape in Phase 1.
- Migration one-way-ness and zombie rows: v4 files opened and saved become v5 (old builds reject by design) — acceptable, surfaced via status note; legacy-row purge rides the existing single save transaction so a crash mid-save can't leave a mixed file. Keep the .ok() tolerance so v1/v2 files (no raster tables) still load.
- Index-based active-layer follow breaks silently on uneven stacks (a 4-layer cel can map 'slot 1 from top' to shadow instead of color). Clamping prevents crashes, not surprise. Acceptable at v1 defaults where stacks are parallel; by-name matching is the recorded upgrade if it bites.
- RemoveCelLayer/RemoveDrawing inverses pin full tile maps in history via Arc — same profile as today's RemoveDrawing, bounded only by set_undo_limit; a user deleting large layers repeatedly with unlimited undo holds all pixels alive. Existing knob covers it; no new mechanism.

## User decisions (with recommendations)

- **Default layers for a brand-new cel?** Options: (a) two layers: 'line' on top (active) + 'color' under it; (b) single 'paint' layer, add more manually; (c) three: + 'shadow' -> Recommended: (a) — matches the merged solo douga/shiage workflow (draw on top, color under, never touch the line); shadow stays one click away in the + menu, and 'new cel clones previous cel's stack' (CSP behavior) is the v1.1 path once his stacks stabilize
- **What is the migrated single layer from your old v4 files called?** Options: 'paint' / 'line' / 'Layer 1' (renameable either way) -> Recommended: 'paint' — the old surface holds line AND color merged; naming it 'line' would lie about its contents
- **Layer-cycle key: A (and Shift+A backward)?** Options: A/Shift+A next to the S/F step cluster, or another key -> Recommended: A/Shift+A — left-hand reachable on the rig, rebindable regardless
- **D currently clears the whole cel. Change D to clear only the ACTIVE layer, with 'clear all layers' as a toolbar item?** Options: yes (safer) / keep D = whole cel -> Recommended: yes — with layers, whole-cel clear silently destroying the line art while coloring is the worst accident available
- **Pen-down while the active layer is hidden: refuse with a hint, or silently un-hide and paint?** Options: refuse + status message (CSP style) / auto-unhide / paint invisibly -> Recommended: refuse + hint — never paint where you can't see; auto-unhide surprises during visibility-juggling
- **When stepping frames, the active layer follows by POSITION from the top (on 'line' at frame 5 → on 'line' at frame 6). Position or name matching?** Options: position from top (v1, simple) / match by layer name -> Recommended: position from top — correct whenever stacks are parallel (the v1 default guarantees it); by-name is the recorded upgrade if uneven stacks appear
- **Cel Layers strip placement?** Options: bottom of the left X-sheet panel (time above, within-cel below) / dropdown in the canvas toolbar -> Recommended: X-sheet panel bottom — persistent visibility answers where-am-I without a click, and it keeps the canvas toolbar to pen-adjacent controls (the chip covers the glance case there)

## Out of scope v1

- Blend modes (multiply for shadow is the classic want) — v6, only when the compositor renders them; storing an ignored mode would lie in the UI. Everything composites premult-over.
- Alpha-lock (Krita lock-alpha — the single most shiage-relevant feature) — v1.5: one DstAlpha-masked dab blend-state variant + a toggle; deliberately not v1.
- Edit-lock per layer (Design 0's advisory `locked`) — cut from v1; hidden-layer guard + separate layers cover the solo coloring case.
- Masks / clipping / alpha-inheritance / layer groups — out.
- Per-layer onion ('line-only ghost', classic douga practice) — v1.1; it's the same set_onion call with a filtered layer list, pulled in on request. v1 onion = whole-cel composite.
- Merge-down and duplicate-layer UI — the command primitives already support both as exact-inverse batches; buttons are v1.1.
- Drag-to-reorder in the strip — ↑/↓ buttons only.
- 'New cel clones the previous cel's layer stack' (CSP behavior) — v1.1; v1 always instantiates the fixed template.
- Isolate/solo view, dim-others, RETAS click-a-trace-line-to-select-its-layer — v1.1 candidates only if where-am-I confusion shows in real use.
- Per-layer vector strokes (LayerKind::Vector) — strokes stay drawing-level under the raster stack; clean v6 migration exists (synthesize a bottom vector layer) if per-layer vector work ever matters.
- 8-bit (Rgba8Unorm) projection/onion fallback for 4K VRAM — designed escape hatch, not built.
- Incremental/dirty-row saves and per-drawing projection caching (Krita-style) — existing M4+ items; both fit behind the current APIs without change.

## User decisions — ratified 2026-07-18

1. **Default stack / presets: build for the real mangaka & pre-anim workflow.** Direct quote: 'Try to implement anything a real mangaka/pre anim workflow would accomplish in this area as if we build this strong enough and appealing to that usecase they will appreciate the work put into that area.' Scope adjustment for Phase 2/3: ground the layer template and + menu in actual production practice, not just line+color —
   - **rough** (blue-pencil convention: roughs drawn in blue, cleaned line over top — real douga practice; ships in the + menu and as an optional template member),
   - **line** (douga clean line, near-black chip), **color** (shiage, under line), **shadow / highlight** (between color and line, blue/red trace-line chips per RETAS convention), **correction** (sakkan overlay, above line, orange).
   - New-cel default remains [color, line] with line active (the merged solo pipeline), but the + menu carries the full professional set at anime-correct stack positions, and the v1.1 'clone previous cel stack' item is promoted to a firm follow-up since pro stacks are per-scene consistent.
2. **Clear keybinds: BOTH.** Two rebindable actions — clear ACTIVE layer and clear WHOLE cel (D defaults to active-layer; whole-cel gets its own chord, e.g. Shift+D). No toolbar-only fallback needed.
3. **Strip placement: whichever is most ergonomic for an animator** — that is the researched recommendation: X-sheet panel bottom (persistent, glanceable while the pen is on the canvas) + the colored active-layer chip in the canvas toolbar. Adopted.
4. **Build start: HELD** until he reads this document. Phase 1 begins on his go.
