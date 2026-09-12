# PSD — definable cel layers + modern-flow stack presets

Room opened 2026-09-12. Extends research/layers-per-cel-design.md
(ratified 2026-07-18 — its laws stand: id-addressed layers, one gesture
= one undo step, hidden-layer guard, soft cap 8, clone-the-held-stack).
**PSD gate passed 2026-09-12 — root: an explicit, persisted,
wire-carried layer ROLE, separate from its name.** Owner: "ratified
push till completed". Calls taken as recommended: Stage 5 (rendered
Multiply) OUT — its own room later; held-stack clone keeps precedence
over the preset.

---

## 1. PREMORTEM (candidate story, for the owner to ratify or rewrite)

It is December 2026. The layer presets shipped and the files rotted.
The story of how: a layer's MEANING lived in its free-text name — the
line guard, the chip colour, the brush colour, the pipeline insert
position, and the frame-to-frame layer follow all matched the literal
string "line" or "shadow". The presets introduced new names ("shadow
line", "genga", "LO ref"), artists renamed layers to taste, and every
rename quietly stripped a layer of its job: the line guard stopped
guarding a line layer called "genga L", shadows inserted above the
line, and a stroke armed on "line" at frame 12 landed nowhere at frame
13 because that cel's line was named "clean". Then a guest on an older
build dropped the new layer fields on the wire, and the room forked on
layer definitions nobody could see.

**Root line: this stands on a layer's ROLE being explicit, persisted,
and carried on the wire — separate from its display name; if function
stays inferred from free text, this is rubble.**

## 2. STANZA — the owner's words, verbatim (2026-09-12)

> Make The Cel Layers more Definable and have a preset for the layers
> that match the modernized animation flow

- **PURPOSE:** Cel layers an artist can define (what the layer IS, how
  it looks, whether it's editable, whether it ghosts) and one-click
  stacks that match the modern digital anime pipeline — layout → genga
  → sakkan → douga → shiage — instead of rebuilding them by hand.
- Root line: stands on an explicit, persisted, wire-carried ROLE per
  layer, separate from its name.
- **NEVER-DO:**
  1. **Never let a layer's function depend on its display name.** Role
     drives every behaviour (guard, chip, brush colour, stack position,
     follow, onion filter); the name is a label. Old files infer role
     ONCE at load from the name, then persist it.
  2. **Never let a preset touch pixels.** Applying a preset only ADDS
     missing layers at their pipeline positions (one undo step); it
     never deletes, renames, merges, or reorders painted layers.
  3. **Never show a property the compositor doesn't render.** (The
     2026-07-18 law, restated: no blend-mode picker until multiply
     actually composites on canvas, onion, graph, and export alike.)
  4. **Never let mismatched builds share layer definitions silently.**
     The host refuses a join from a build older than the layer wire,
     with an update hint (version sync then fetches it).
- **BLAST RADIUS:** crates/anim-core model.rs (LayerRole + LayerProps
  fields) and store.rs (schema v9 + role inference); app doc.rs (layer
  ops, new-cel template, follow), xsheet_panel.rs (the strip),
  canvas.rs (chip colour, pen guards, onion filter), config.rs (presets
  + Settings ▸ Layers), workspace.rs (stage → preset binding), net.rs
  (join floor only). NOT touched: paint.rs / the compositor (unless
  Stage 5 is ratified), the brush engine, the session transport.
- **STAGES:** 1 engine (role + fields + schema v9) → 2 role-driven
  behaviour → 3 the definition UI → 4 stack presets → 5 *(optional,
  separately costed)* rendered Multiply for shadow roles.

## 3. WHAT "DEFINABLE" MEANS — the layer card

Every cel layer gains four definitions beside today's name / visible /
opacity (all in `LayerProps`, so SetCelLayerProps keeps its exact
wholesale inverse — no new commands):

| field | meaning | pixels? |
|---|---|---|
| `role` | what the layer IS (table below) | no |
| `chip` | identity colour override (else the role's RETAS colour) | no |
| `locked` | edit-lock — pen/eraser/fill refuse, like the hidden guard | no |
| `onion` | participates in onion ghosts (line-only ghost = line roles) | no |

None of them change composited pixels, so `content_hash` stays
exactly as it is (the same exclusion rename already has) — no eval
invalidation, no GPU resync, no hash-law break.

**Roles** — each with a pipeline rank (stack position, bottom → top),
a RETAS trace colour, and a default brush ink:

| rank | role | anime term | chip / ink |
|---|---|---|---|
| 0 | Reference | LO / BG ref (layout plate to draw against) | grey |
| 1 | Color | shiage base paint | green chip / skin tone ink |
| 2 | Shadow | kage (1-kage) | blue |
| 3 | Highlight | hi | red |
| 4 | Rough | blue-pencil rough | light blue |
| 5 | ShadowLine | kage-sen — shadow boundary trace | blue line |
| 6 | HighlightLine | hi-sen — highlight boundary trace | red line |
| 7 | Line | main trace (genga / douga clean line) | near-black |
| 8 | Correction | sakkan overlay | orange / Aka |
| — | Custom | anything else (no rank: inserts above active) | set by chip |

The shadow/highlight BOUNDARY LINES as their own roles come straight
from the verified research (§2: "shadow markup is isolated onto its own
layer"; §5 row 5: "trace lines / shadow lines / color-boundary lines as
distinct channels per drawing"). Behaviours that become role-driven:
line guard protects every *line* role (Line, ShadowLine,
HighlightLine) — renamed or not; the + menu and preset application
insert by rank; layer switch loads the role's ink; frame-to-frame
follow matches by NAME first, then by ROLE when the name is absent and
exactly one layer holds that role (a rename no longer unarms the pen);
the fresh-cel slot resolution reads the real template, not a hardcoded
[color, line].

## 4. THE PRESETS — built-in modern-flow stacks (bottom → top)

| preset | stack | active | for |
|---|---|---|---|
| **Solo merged** *(today's default)* | Color, Line | Line | one artist, draw + paint |
| **Layout (LO)** | Reference 40% 🔒, Rough | Rough | blocking a cut against the plate |
| **Genga** | Rough 50%, Line, Correction | Line | key animation + sakkan overlay |
| **Douga** | ShadowLine, HighlightLine, Line | Line | clean in-between + colour-trace lines |
| **Shiage (full)** | Color, Shadow, Highlight, ShadowLine 🔒, HighlightLine 🔒, Line 🔒, Correction | Color | modern digital paint stage (7 ≤ cap 8) |

Plus **user presets**: "save this cel's stack as a preset" (structure
only — names, roles, chips, locks, opacity; never pixels), editable in
Settings ▸ Layers. Presets are per-user config (the personalization
law) — only a cel's own structure ever crosses the wire, inside the
AddDrawing that creates it.

**Where presets act:**
- **New cels** — the strip header's "new cel: [preset ▾]" names the
  template a stroke on an empty frame will create (replacing today's
  fixed "a new cel gets color + line"). Precedence is the ratified
  2026-07-18 law: a HELD cel's stack is cloned first (per-scene
  consistency); the preset applies at column starts / after Empty.
- **Rooms** — each stage binds a default preset: layout → Layout (LO),
  genga → Solo merged, shiage → Shiage (full). Switching rooms swaps
  the new-cel preset like it swaps tools; your pick overrides.
- **This cel** — "apply preset to this cel" adds only the missing
  roles at their ranks, one undo step (NEVER-DO 2).
- **Conform cut** — the same additive apply to every drawing in the
  cut, one undo step: pro stacks stay per-scene consistent.

## 5. THE UI (strip + settings)

- **Row** gains a role glyph in the chip (click: role / colour menu)
  and a lock toggle beside the eye. Right-click a row: the **layer
  card** — name, role, identity colour, lock, onion, opacity — every
  change one undo step through SetCelLayerProps.
- **Header:** `+ ▾` lists ROLES (inserted at rank), and `stack ▾`:
  new-cel preset · apply to this cel · conform cut · save as preset.
- **Settings ▸ Layers:** preset list (built-ins read-only, duplicable;
  user presets editable: add/remove/reorder specs, role, chip, lock,
  opacity), and the per-role default inks (the existing name-keyed map
  migrates to role keys, names kept as fallback).

## 6. STAGE 5 (optional — the owner decides)

Rendered **Multiply** for Shadow roles — the one blend mode modern
shiage actually leans on. NEVER-DO 3 means it only ships if it renders
identically in the canvas projections, onion ghosts, the graph
compositor, and export flatten: a new blend pipeline variant in
paint.rs + graphcomp, a blend field in LayerProps that DOES enter
content_hash, schema bump, and cross-GPU audit behaviour in sessions.
Roughly as large as Stages 1–4 combined. Recommended: ship 1–4, live
with them, then gate Stage 5 in its own room.

## 7. TESTING SPINE

Engine: save/load round-trip with every new field; a v8 fixture loads
with roles inferred from names ("line" → Line, "color" → Color, …,
unknown → Custom) and re-saves stable; the exact-inverse fuzz extends
to role/chip/lock/onion props; content_hash unchanged by role, chip,
lock, onion (pinned). App: preset application is additive (painted
layers byte-identical, one history entry); follow-by-role resolves a
renamed line and refuses on ambiguity; lock guard refuses pen, eraser,
fill, and lasso; the join floor refuses an older build.

---
*PSD gate passed 2026-09-12 — root: explicit persisted role. Stages
1–4 build; Stage 5 deferred to its own room.*
