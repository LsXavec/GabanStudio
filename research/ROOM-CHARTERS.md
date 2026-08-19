# ROOM CHARTERS — the three workflows (addendum to DESIGN-SPEC-editor-ui.md)

Ratified inputs: the Plate & Pencil spec (tokens §2, colour law §3, DETENT/LATCH/DANGER §6.3, Do-NOT-build §7 — binding), the Owner's Amendment of 2026-08-17, the grounding report, and the live tree (`app/src/config.rs`, `canvas.rs`, `main.rs`, `workspace.rs`, `xsheet_panel.rs`, `palette_panel.rs`, `icons.rs`, `plate.rs`, `nodegraph_panel.rs`). Blast radius honored throughout: app-crate UI only; `crates/anim-core` untouched; `workspace.rs`'s dock untouched; no document format, no save/load, no undo semantics, no stroke path, no wgpu.

## How the rooms share one grammar

1. **Same skeleton.** Every room is written and built in the same six movements: THE WORK, AT THE PEN, NOT IN THIS ROOM, DIFFERENTIATION, KEYS, BUILD DELTA. A room is a lens over one instrument, never a second instrument.
2. **Same chassis.** The Slate (arming line, top) and the Foot (transport, editable frame, loop, fps, refusal/chatter/undo lanes, bottom) are global — never duplicated in a pane, never resized per room. Muscle memory crosses doors intact.
3. **Position law.** Left = time and the queue. Top = the verbs. Right = reference and paint. Bottom = the machine. No zone trades jobs between rooms.
4. **One-lamp law.** Tally means the one armed or current thing — at most one lamp per zone. Three glances read the whole arming state; nothing else glows.
5. **Token law.** Only the ratified roles: Tally, Ao, Aka, Legend, Struck, Well, Paper, Graphite. Well keeps its §2.1 ONLY-list inviolate. No green, anywhere, ever.
6. **Material rule** (ratified with this addendum): the artwork's own colours — swatches, pots, preset specimen strokes — are *material*, not tokens. Material is always seated in a recess so it reads as substance, never as signal, and it never encodes UI state. This is defect-5's shipped precedent (`canvas.rs:31-36`) stated as law.
7. **XL law** (ratified with this addendum): an oversized control doubles its spec size class in the axis the pen approaches; where a fixed row pitch caps that axis, the doubling lands wholly in the free axis. Nothing earns XL without a named top-five frequency verb. Big = frequent, never decorative.
8. **One verb, one key.** `Q` = PrevKey, `W` = NextKey, `R` = Flip (momentary), in every room — one ratified default, ending the split proposals; `Shift+F`/`Shift+S` stay free for the owner's own rebinding. The sole cross-room key exception is the armed-tool lens on `1`–`8` (SHIAGE §KEYS), named and ratified below.
9. **Lensing mechanism.** A room changes pane *content*, never the dock: `EditorTabs::ui` (`main.rs:886-923`) gains the active stage as input; `Stage::dock()` recipes and the dock subsystem stay untouched.
10. **One icon kit.** Every mark is a `Painter` primitive from the shipped `icons.rs` (Brush/Eraser/Lock/Select/Lasso/Marquee/Fill/Comp/Eye/EyeOff/Up/Down/Fit/Plus, `icons.rs:14-42`), drawn in one caller-supplied ink; the rooms extend the kit, never fork it. Nothing reflows, ever; capability is never removed, only unfronted.

---

## GENGA 原画 — the drawing room

### THE WORK

An hour in this room is one loop repeated hundreds of times: lay a burst of short pressure strokes on the active cel — construction in blue, clean line in graphite over it — undo the bad ones, then flip to the neighbouring drawings to judge the motion, and back to the paper. Every twenty to sixty seconds the pencil changes character (rough to clean line to eraser and back), a few times an hour a new cel is exposed to the sheet, and the timing questions — *how long is this hold, am I on ones* — are answered peripherally, because timing pre-exists the drawing: the sheet is the work queue here, not an editing surface. The room's whole job is to keep that loop unbroken: the pencil box under the eyes, the flip under two fingers, the clock at the edge of the paper, and nothing else asking to be read.

### AT THE PEN — ranked element inventory

One new size, derived under the XL law: **SLOT_XL = 96 w × 44 h** — 44 is exactly 2 × CTRL_H (the pen's approach axis in a horizontal bar); 96 of width because a specimen stroke needs run to tell its pressure story, plus seating for the engraved name and hotkey digit. Four slots ≈ 400 pt of bar, inside the existing 0.11 pane (44 + 26 + padding of height). Brush and mode switches are the third most frequent deliberate act in the room after strokes and undo — the owner's single named pain — and this is the only doubled control in the app.

| # | Element | Size | Kind | Colour roles | Where |
|---|---|---|---|---|---|
| 1 | **Paper** | — | — | Paper token | Canvas leaf, centre |
| 2 | **The mirror strip** (run marks, rule hierarchy, Tally notch) | RULE_STRIP_W 14, pitch 18 | readout | key dots Tally, holds Ao, rules Legend α-steps | Canvas left edge (spec §5.4, Phase 2, unchanged) |
| 3 | **THE PENCIL BOX — [ATARI] [GENGA] [SHUSEI] + ERASER** | four SLOT_XL slots, fixed, never reflow | DETENT group (mutually exclusive; arming a preset arms Brush; Eraser is the fourth detent) | slot ground Graphite; specimen stroke painted in the preset's own ink (ao / ink / aka — material rule, seated); engraved caps names, 9.5 pt (the spec's one engraved-caps size); hotkey digit Plex Mono 11 Legend; armed slot = 2-device-px Tally ring, drawn broken (stroke, not closed) when parameters have been nudged off the preset | Brush bar, far left — first thing the eye meets above the paper |
| 4 | **Undo/redo** | keys + Foot tape counter `UNDO 12 · REDO 0` | readout | Legend label, Struck digits | Global Foot, right lane |
| 5 | **Flip / key-step** | keys only | — | felt on Paper + mirror strip | No control surface; the mirror strip is its instrument |
| 6 | **SIZE track + armed-dynamics latches** (`P·SIZE`, `P·OPAC`, `TILT`) | OPTIONS row 26 pt, always allocated; track fixed **200 pt** | slider + three LATCHes | label-before Legend, value Plex Mono Struck at fixed right column; latch on = Tally left-edge bar, off = LEGEND_DIM | Brush bar OPTIONS row — armed-tool-scoped, populated for Brush in this room |
| 7 | **Onion cluster**: latch, back/fwd depth, strength, "ghost active layer only" | latch CTRL_H 22; steppers 22 | LATCH + steppers | ghost BEHIND Ao, ghost AHEAD Legend (spec defect 14); latch on = Tally bar | Lightbox rail, 96 pt, inside Canvas leaf (Phase 4) |
| 8 | **New cel** | `+` detent on X-sheet column head, hit ≥ 22 | DETENT menu (new drawing / expose sel. / LIFT KEY) | `+` Legend; LIFT KEY entry DANGER (Aka edge, 350 ms hold) | X-sheet column head (spec §4.2) |
| 9 | **Cel-layer chips** (line / rough / correction) | ROW_H 18, fixed column grid (defect 10) | rows; footer ERASE CEL is DANGER | rough name in Ao, correction name in Aka (text only, never fill — lawful: it *is* the correction), line in Struck; layer receiving ink = Tally dot in vis column | Cel Layers pane |
| 10 | **Pencil swatch board** `ink · ao · aka · white` | 18×18 rect_filled in a Well recess | swatches | material rule; armed swatch Tally ring (defect 5) | Brush bar, right of the pencil box |
| 11 | **Select detent · alpha-lock latch · comp latch** | CTRL_H 22, small fixed slots | DETENT / LATCH / LATCH | comp active paints 1-px Aka canvas edge (it refuses the pen — defect 3) | Brush bar, right of swatches — deliberately half the pencil box's size |
| 12 | **X-sheet** | left rail (dock 0.24, untouched), ROW_H 18, TALLY_WELL current row | reference / work queue | per spec §5.2–5.3 | Left half of the room |
| 13 | **Arming line** `D4A · col A · line · genga` | Slate row 2 | readout | Legend labels, Struck values; the armed preset's `name` field is displayed verbatim (`main.rs:502`) — the brush finally has a persistent identity | Global Slate (Phase 3) |
| 14 | **INPUT plate field** | rail field | plate fact (click opens pen settings) | Legend label, Struck value; Aka only on contradiction (Warning Law) | Lightbox rail (Phase 4) |

**What gets BIGGER than spec default:** only the pencil box (44 vs CTRL_H 22 — the XL law's doubling, bought by switch frequency and the owner's core complaint) and the size track (fixed 200 pt vs today's 48–110 px clamp at `canvas.rs:1028`, the "controls shrink" defect the Amendment names). Everything else sits exactly on spec scale; the room reads larger by *subtraction*, not inflation.

### NOT IN THIS ROOM

- **Fill tool, its options, and the fill slot.** Shiage's verb. No fill detent in the genga bar; `G` still arms it for the rare leak-fix, at which point the OPTIONS row lenses to fill options (armed-tool-scoped, nothing reflows).
- **The Palette pane / character colour models.** Shiage's asset board. Genga colour is the pencil triad plus white, period.
- **Colour wheel / free picker.** Behind the swatch board's `⋯` detent; never resident.
- **Viewer panes, Node Graph, composite furniture.** Finishing and Edit own them. The comp *latch* remains — checking the composite is a glance, not a workspace.
- **Transport as room furniture, export, camera columns, waveform.** Foot and Henshū own them.
- **X-sheet editing ceremony.** Retiming lives in Henshū; here the sheet is read and appended to (`+` detent), and LIFT KEY stays guarded behind it.
- **A separate Presets pane as the room's instrument.** Still reachable via the panes menu, but the room stops depending on a pane the dock recipe tabs behind Layers (`workspace.rs:134`, unfixable inside blast radius) — the pencil box replaces it as the fronted surface.

### DIFFERENTIATION — how the hand finds tools without reading

**By position (the room's compass):** left edge = time (sheet, then mirror strip, then paper); top of paper = the pencil box, always the same four slots in work order rough→line→correction→eraser, matching keys 1·2·3·X; right = the layer stack; bottom = the machine. Muscle memory is possible because nothing ever reflows.

**By size:** exactly one oversized group exists. Big = "changes what the pencil is." Small = "changes how the room behaves." That single contrast is the anti-tool-blindness mechanism.

**By painted mark** (shipped kit + two additions, all `Painter` primitives):
- **Specimen stroke** (new, each preset slot): a tapered stroke built as one `convex_polygon` strip, thin→thick, drawn in the preset's ink — ATARI a soft wide ao taper, GENGA a moderate ink taper, SHUSEI a thin aka taper. The icon *is* the pressure behaviour; the three slots differ in colour AND silhouette, unconfusable at a glance.
- **Eraser** = shipped slab (`Icon::Eraser`). **Select** = shipped corner brackets. **Alpha lock** = shipped padlock. **Depth steppers** = shipped Up/Down chevrons. **New cel** = shipped Plus.
- **Onion latch** (new): two offset `circle_stroke`s — rear Ao, front Legend — teaching the behind/ahead ghost colours in the icon itself.
- **ERASE CEL / LIFT KEY**: text in DANGER dress (Aka 1-px edge, filling Tally ring on hold).

**By colour role:** Tally alone means armed — one ring in the pencil box, one dot in the layer stack, one notch in the mirror; the eye counts three lamps and knows the whole arming state. Ao always means scaffold. Aka appears only as correction identity, danger edges, and refusals — never ambient.

### KEYS

Verified against `config.rs`'s Action table:

| Act | Key | Status |
|---|---|---|
| Preset ATARI / GENGA / SHUSEI | `1` `2` `3` | exists (`Preset1–3`) — requires the triad at list positions 0–2 (slot = position, `config.rs:1110-1118`) |
| Eraser | `X` | exists |
| Undo / Redo | `Ctrl+Z` / `Ctrl+Y` | exists |
| Brush / Select | `B` / `V` | exists |
| Onion latch | `O` | exists |
| Layer cycle | `A` / `Shift+A` | exists |
| New cel | `E` | exists (`NewDrawing`) |
| Frame step / play | `S` `F` `Space` | exists |
| Alpha lock | `L` | exists |
| Erase cel (guarded) | `D` / `Shift+D` | exists — gains DANGER hold per defect 6 |
| PrevKey / NextKey | **`Q` / `W`** | new — spec Phase 2 mandate; the ratified cross-room default |
| Flip (momentary: previous drawing while held) | **`R`** (hold) | new — same Phase 2 mandate |
| Brush size down / up | **`[` / `]`** | new UI-only actions `BrushSizeDown`/`BrushSizeUp` — frequency rank 4 |
| Onion depth back/fwd nudge | `,` / `.` | new UI-only, optional — rail steppers may suffice |

All preset dispatch keeps its `!stroke_active()` guard (`main.rs:497-505`); Flip and key-step must carry the same guard.

### BUILD DELTA

Phase-2 items (run model, mirror strip, `NextKey`/`PrevKey`/`Flip` actions) are already chartered by spec §5/§7 and are dependencies here, not re-chartered.

**Phase 1 — INK**
1. Extend the shipped `icons.rs` kit: specimen-stroke painter (tapered `convex_polygon` strip in the caller's ink) and onion twin-circles. (Nib, slab, corners, padlock, droplet, plus, marquee, lasso, comp are already shipped; the remaining tofu sweep completes under the spec's existing defect-1 item.)
2. Layer-identity inks in Cel Layers chips: rough = Ao text, correction = Aka text (retires `layers-per-cel-design.md`'s RETAS orange — the ratified palette has no token for it).
3. ERASE CEL → DANGER dress and eviction to the Layers pane footer (defects 6–7).
4. Finish defect 5 on the shipped rect_filled swatches (`canvas.rs:31-36`): 18×18, Well-recess seat, armed-swatch Tally ring.
5. Pen-curve editor off-law blue (`config.rs:1287,1307`) → plate tokens; the brush-settings dialog is this room's furniture.

**Phase 3 — GEOMETRY**
6. **Stage-lensed brush bar**: pass the active stage into the pane body (`EditorTabs::ui`, `main.rs:886-923`). Genga composition: pencil box (4 × SLOT_XL, fixed) · select/lock/comp (CTRL_H) · swatch board · OPTIONS row. No fill slot in genga. (The raster switch already left the bar for Settings — `canvas.rs:1030-1031`, shipped.)
7. OPTIONS row populated for brush: 200 pt fixed size track, min-size, three dynamics latches with at-a-glance armed state. Row height 26, never reflows, content scoped to the armed tool.
8. **Default preset list**: the three genga pencils (table below) installed at positions 0–2 of `default_presets()` (`config.rs:495-532`); the existing `shiage fill` and `shadow airbrush` move to positions 3–4 (keys 4·5); slots 6–8 free for the owner. Retires the dyn_size-only "genga pen" (`config.rs:497-502`) against the Amendment's ≥3 pressure+opacity presets.
9. **Active-preset tracking** (UI state only — no such field exists in `canvas.rs` today): set by `apply_preset`, cleared/marked-modified on parameter change; drives the Tally ring, its broken-ring modified state, and the Slate arming line's preset name.
10. Mid-stroke guard on the Presets-pane click path (`main.rs:988-991`) to match the hotkey guard.
11. Slider geometry: labels-before, mono values at fixed right column, fixed track widths; retire the 48–110 px clamp and the sub-190 px bare-glyph compact collapse (`canvas.rs:1027-1028`) for the genga bar.
12. X-sheet header eviction + `+` column-head detent + LIFT KEY naming (global Phase 3; genga's new-cel target).
13. New actions `BrushSizeDown`/`BrushSizeUp` (+ optional onion-depth nudges) in the Action table with the defaults above.

**Phase 4 — FURNITURE**
14. Lightbox rail (96 pt, `Panel::left` inside the Canvas leaf): onion latch/back/fwd/strength, "ghost active layer only" (renamed from "line only"), INPUT plate field per the Warning Law, field/safe/peg guides in Ao.
15. Onion re-ink on the **vector** path only — ghost BEHIND Ao, AHEAD Legend, at real saturation (defect 14; raster recolour stays on the Do-NOT-build list).

**Explicitly not built**, restated from the binding list: pen-down score dim, live pressure trace, Aka forward ghost, transport in the X-sheet pane, de-docking, animated transitions, any green.

---

## SHIAGE 仕上げ — the finishing room (ink & paint + composite)

### THE WORK

The painter's hour is a metronome: open the character's colour model, arm a pot, click a closed region, click the next, and the next — hundreds of fills, dozens of regions per cel, dozens to hundreds of cels — pausing only to close a leak with a two-second pencil stroke, to hop the target layer (colr → shadow → hilite, never line), and to hit one key that advances to the next cel in the queue. Colour is never *chosen* here; it is *fetched by name* from a model someone already designed (skin-normal, skin-shadow, hair-highlight). The room's entire economy is that no fill ever costs a second click: the tool stays armed (Fill is re-armed on entry, `workspace.rs:177-181`), the pot stays armed across cels, the advance is one key, and the only glances are peripheral — the composite Viewer, the slim X-sheet rail, and at the end of each cel a hole-hunt before calling it done. Everything below protects that metronome or shortens those glances.

### AT THE PEN — ranked element inventory

Room-specific sizes under the XL law:

```
POT_W 44 · POT_H 30      swatch pot   (2× the shipped 26×18, palette_panel.rs:63, snapped)
CHEV_W 44 · CHEV_H 40    next/prev-cel chevron
OPT_VAL_W 64             gap/under numeric field width
```

| # | Element | Size class | Affordance | Colour roles | Where |
|---|---|---|---|---|---|
| 1 | **THE SWATCH BOARD** — the room's primary instrument. One character at a time: a character DETENT tab row (engraved caps, active tab Tally fill), then the pot grid: fixed rows via `allocate_exact_size` — `[pot 44×30][role name, flex, elided]`, 4 pt gaps, 36 pt row pitch, vertical scroll | Pot **44×30** (XL: hundreds of picks/hr against today's 26×18 sub-Fitts button — the single highest click-frequency × target-size mismatch in the app) | Pot = DETENT (one armed pot, ever) | Pot fill = the paint itself — material rule, seated in a Well recess with 1-px Legend edge; armed pot ringed Tally; role and character names Struck (the colour designer authored them — two-ink law) | Palette pane, right rail below Viewer |
| 2 | **EDIT MODEL latch** — OFF (default): the board is pure pots — no rename fields, no remove, no colour-edit, no add-row can be hit mid-metronome. ON: each row grows its colour-edit detent (the wheel lives here and only here), rename, remove (DANGER), plus ADD ROLE / ADD CHARACTER. Model edits propagate to every frame using the role — that power is why it is gated | CTRL_H 22, full board width | LATCH; removals inside are DANGER | Label Legend, armed Struck + Tally edge bar | Swatch board footer |
| 3 | **OPTIONS row** — armed-tool-scoped like every room. Fill armed (the room's resting state): `GAP` 0–8, `UNDER` 0–4, and the boundary DETENT pair `CEL / LAYER`. Brush armed (the B-bounce): the size track and dynamics latches, exactly as genga defines them. Select armed: the shape pair | OPTIONS_H **26**, always allocated (defect 4); numeric fields **64 pt** Plex Mono (leak-chasing nudges these several times per cel; today's DragValues at `canvas.rs:1102-1105` are thumb-sized) | DragValues; CEL/LAYER = DETENT pair | Labels Legend, values Struck Plex Mono 11, armed boundary detent Tally disc | Brush strip, the 26 pt row under the fixed tool slots |
| 4 | **NEXT / PREV CEL chevrons** — the queue advance, wired to Phase-2 `NextKey`/`PrevKey`. Mounted on the **Canvas rail**, top block — *not* the X-sheet pane: spec §7's ban on transport inside that closable leaf is verbatim law and this charter obeys it. The readout stays where it belongs: pressing a chevron moves the Tally notch in the sheet one zone left, and the rail sits at the paper's edge, where the pen already lives | **44×40 each**, side by side (XL: dozens–hundreds of advances/hr — the room's second verb) | Momentary buttons | Painted Legend chevrons, Tally flash on press | Canvas rail (Phase-4 rail inside the Canvas leaf), top block |
| 5 | **THE QUEUE RAIL** — the 0.18 X-sheet as read-mostly work queue: runs, key dots, TALLY_WELL current row, seconds gutter; header chrome evicted per spec §4.2; this room's lens keeps the library drawer collapsed | ROW_H 18 grid, unchanged | — | Per spec §5.2 | Left rail, 0.18 |
| 6 | **TOOL SLOTS** — fixed slots (defect 4): fill (armed on entry), brush (the retouch pencil), eraser, lock, select, comp. Painted icons, no tofu | CTRL_H 22 per slot, fixed x forever | DETENTs / LATCHes; comp keeps its Aka canvas edge while active (defect 3) | Icons Legend; armed = Tally disc | Brush strip, 0.11 above canvas |
| 7 | **CEL LAYERS pane** — fixed column grid (defect 10); the paint-target rows (colr / shadow / hilite) are what `A`/`Shift+A` cycles; the `line` row displays the SHACKLE mark. Footer: ERASE CEL (DANGER) and the LINE GUARD latch | ROW_H rows, fixed grid | rows; ERASE CEL = DANGER | Active layer Tally; hidden LEGEND_DIM strike-through; guard refusal Aka | Below X-sheet rail |
| 8 | **LINE GUARD latch** — default ON in this room. While latched, the `line`-named layer **cannot be armed as ink target**: `A`/`Shift+A` skips it, and tapping its row fires the defect-16 refusal (Aka lane in the Foot + canvas-edge flash). The pen path itself is untouched — the pen always reaches whichever layer is armed; the guard governs *arming only*, UI state of the same family as tool arming, keeping ROOT ("never changes how the pen reaches the document") whole. A guard, not a lock: unlatch for deliberate line surgery. Its ratification rides this addendum, not silent inheritance | CTRL_H 22 | LATCH | Label Legend; the refusal Aka (armed pen vs forbidden target — Warning Law compliant) | Cel Layers footer |
| 9 | **MISS CHECK latch** — the hole-hunt. Latched: the ground the cel composites over flips **Paper → Graphite** (the chassis dark — Well's §2.1 ONLY-list stays inviolate); every unpainted pixel reads instantly as a dark pit in flat colour, lines untouched. No lamp, no new hue — health shown by a meter (the ground), the spec's instrument philosophy. The ground is UI paint (`canvas.rs:1382`), verified: NEVER-DO 2 never comes into contact | CTRL_H 22 | LATCH | Label Legend; ground Graphite | Canvas rail, below the chevrons. The rail's onion block sits dormant at LEGEND_DIM in this room (onion off, `workspace.rs:177-185`) |
| 10 | **VIEWER (composite)** | unchanged pane | — | — | Right of canvas, 0.58 — the continuous peripheral glance |
| 11 | **Arming line (Slate)** gains the armed pot: `D4A · col A · colr · skin-shadow` — kills the "what colour am I holding" saccade | Slate row, global | readout | Value Struck | Slate, global |
| 12 | **Foot** — transport, editable frame, loop, refusal/chatter/undo lanes | FOOT_H 34, global | — | per spec §4.2 | Bottom |

### NOT IN THIS ROOM

- **The genga pencil box and Presets pane** — ATARI, GENGA, SHUSEI are the Drawing room's instrument. (When the painter bounces to the retouch brush with `B`, the number keys still reach them — GENGA on key 2 is the natural leak-closing pencil.)
- **Brush dynamics ⚙** — the fill hand never opens it; stays behind the brush's own slot, unfronted.
- **Onion controls and ghost colours** — onion is off here; the rail block sits LEGEND_DIM.
- **The colour wheel as furniture** — colours are assets, not choices; the wheel exists only inside EDIT MODEL.
- **The drawings library** — collapsed drawer; this room's lens keeps it shut.
- **Retiming decoration** — hold-tail drag and LIFT KEY remain functional (capability is never removed) but undecorated; the sheet here is a queue, and timing surgery is henshū's charter.
- **NodeGraph** — not in the Finishing dock, stays out.
- **Transport duplicates, export buttons** — Foot and `≡` menu own them; nothing transport-shaped enters the X-sheet leaf.

### DIFFERENTIATION — how the hand finds each tool blind

**Position:** the three zones never trade jobs — left rail = the queue (sheet + layers), top strip = the verbs (fixed slots + options), right rail = the paint (viewer over pots), with the advance chevrons at the paper's own edge. The armed pot is the only Tally ring on the right; the armed tool the only Tally disc up top; the current cel the only Tally notch on the left. One lamp per zone, three glances, zero reading.

**Size:** the two things touched most are the two biggest targets — 44×30 pots, 44×40 chevrons. Nothing destructive is large: ERASE CEL and the EDIT MODEL removals are deliberately ordinary-sized, edge-marked, and hold-guarded (Fitts's law stops pointing at the irreversible, defect 11).

**Painted marks** — droplet, nib, slab, and corner brackets are already shipped in `icons.rs`; this room adds three:
- **CHEVRON ◂ / ▸** (prev/next cel): three-point `convex_polygon`, apex left/right (siblings of the shipped Up/Down).
- **HOLE** (miss check): small `rect_filled` in Legend with one Paper `circle_filled` punched through — the defect the latch hunts, drawn as itself.
- **SHACKLE** (line guard, on the line layer's row): `rect_filled` base + upper `circle_stroke` arc.
- The pots need no icon — **a pot's own colour is its icon**, and grid position is its address; the hand learns "skin is row two" within a session.

**Colour:** Tally only for armed/current; Ao untouched by this room beyond the sheet's own marks; Aka only refusals and destruction; no green; artwork colour confined to material seated in Well.

### KEYS

Existing and already correct (`config.rs:139-188`): `G` Fill (re-armed on entry anyway), `B` retouch brush, `X` eraser, `L` alpha lock, `A`/`Shift+A` the colr→shadow→hilite hop, `F`/`S`/`Space`/`Home`/`End` transport, `C` composite, `Ctrl+Z`/`Ctrl+Y`, `D`/`Shift+D` (DANGER-guarded).

- **`Q` / `W` = PrevKey / NextKey** — the ratified cross-room default (uniformity contract §8); in this room they *are* prev/next cel, the chevrons' keys, and the single most valuable binding in the room. `Shift+F`/`Shift+S` remain free for the owner's own rebind.
- **`M` = MISS CHECK latch** (M is unbound today).
- **Pot keys — the armed-tool lens, ratified with this addendum as the grammar's sole key exception:** `1`–`8` arm pots 1–8 of the active character's board *while Fill is armed*; with Brush armed they arm brush presets exactly as everywhere else, so the B-bounce keeps GENGA on key 2. A tool-scoped lens of the existing `Preset1–8` dispatch (`main.rs:485-505`), stage- and tool-checked at dispatch; the Action table untouched. Mode-dependence is disclosed and mitigated: the Slate names the armed pot, the status line echoes it, and the lens follows the armed tool the hand just chose.
- **EDIT MODEL gets no key — deliberately.** Model edits are rare and propagate everywhere; they cost a click.

### BUILD DELTA

**Phase 1 — INK**
1. `palette_panel.rs`: pots restyled — `rect_filled` seated in a Well recess, 1-px Legend edge, armed pot Tally ring (today *nothing* marks the armed pot); character/role names to Struck; helper paragraph deleted; remove buttons restyled DANGER pending their Phase-3 move into EDIT MODEL.
2. Fill options (`canvas.rs:1102-1105`): `prefix("gap ")` label-inside-value → engraved Legend label before, Struck Plex Mono value (part of the global slider medium fix).
3. Extend the shipped icon kit with CHEVRON-L/R, HOLE, SHACKLE (droplet/nib/slab/corners already landed); DETENT/LATCH helpers applied where the strip still lacks them.

**Phase 3 — GEOMETRY**
4. `palette_panel.rs` rebuilt as the SWATCH BOARD: character DETENT tabs (one character at a time — today all characters stack vertically); fixed pot grid via `allocate_exact_size` `[pot 44×30][name flex]`; EDIT MODEL latch gating colour-edit / rename / remove / add-role / add-character (all currently exposed permanently, `palette_panel.rs:40-80`). Presentation only — `CharacterPalette`/`ColorRole` serde untouched; restructuring roles into a variant grid would change project format, forbidden under NEVER-DO 1 and deferred to its own gate.
5. Brush strip fixed slots + always-allocated OPTIONS row (defect 4); fill's gap/under/boundary land there with the 64 pt fields; brush's and select's row content scoped per the armed tool.
6. X-sheet header eviction (spec §4.2) turns the 0.18 rail into the queue; the Finishing pane lens (stage input to `EditorTabs::ui`) keeps the library drawer collapsed.
7. Slate arming line extended with the armed pot name (one string, Struck).
8. Cel Layers fixed grid + ERASE CEL to DANGER footer (defects 6/10/11) — inherited, not room-specific.

**Phase 4 — FURNITURE**
9. Canvas-rail top block: NEXT/PREV CEL chevron pair (44×40), wired to Phase-2 `NextKey`/`PrevKey`; `Q`/`W` are their keys. (Nothing mounts in the X-sheet leaf — spec §7 verbatim.)
10. MISS CHECK latch on the canvas rail: flips the composite ground Paper ↔ Graphite in UI paint (`canvas.rs:1382`, verified UI-side; NEVER-DO 2 untouched).
11. LINE GUARD latch (default ON in Finishing): arming-only guard on the `line`-named layer through the defect-16 refusal machinery — cycle skips it, row-tap refuses. Zero stroke-path code.
12. Pot-key lens at the `Preset1–8` dispatch (Fill → pots, Brush → presets); `M` bound to MISS CHECK.
13. Deferred, unscheduled, named so nobody rediscovers them as "quick wins": alt-click role-pick (a re-query, but a new interaction surface) and the variant-grid palette model (a format change) — each needs its own gate.

**What got BIGGER and the number that justifies it:** pots 26×18 → 44×30 (hundreds of picks/hr — the room's most-clicked non-canvas target); cel advance from no control at all → 44×40 chevrons + `Q`/`W` (dozens–hundreds/hr); gap/under fields → 64 pt (several adjustments per cel while leak-chasing). Everything else holds spec scale — the Amendment spends sparse space on the frequent, not on everything.

---

## HENSHU 編集 — the edit room (timing + review)

### THE WORK

The henshū artist does not draw; he *listens to the cut breathe*. The hour is a loop: play, watch, stop, jog three frames back, stare at the sheet, drag one hold's tail two frames longer, play again — dozens of cycles, each ending at the exposure sheet, because in this pipeline the sheet is the master timing artifact and drawings are only references it exposes. Retiming never touches artwork: a hold extended is a reference moved; a key lifted is a hold flowing forward into the gap. His eyes live in two places — the looping Viewer and the sheet's vertical marks — and his hand lives on three verbs: transport, the hold-tail drag, and LIFT KEY. Camera is not a second timeline; pan/zoom/ease are numeric columns *on this same sheet* (C1, ratified). Everything else in the room is furniture and knows it.

### AT THE PEN — ranked element inventory

The dock arrangement (`Stage::dock()` Edit: X-sheet left 0.45, Viewer∥Canvas tabbed right, Layers under sheet, NodeGraph under viewer) is not touched.

**A. The sheet itself** (Well field, left 0.45 — the instrument)

| # | Element | Size | Kind | Colour | Where |
|---|---|---|---|---|---|
| 1 | **Run marks** — key dot, unbroken hold rule, tail cross-tick (spec §5.2) | dot 3.5r; rule 2 dev px; pitch ROW_H 18, sacred | passive → drag | dot Tally; rule + tick Ao; whole run Tally only while hovered/dragged | the grid |
| 2 | **Tail cross-tick drag** — THE retiming verb | tick 9 pt visual, hit zone 22×22 | drag handle | Ao → Tally while engaged | run's last row |
| 3 | **Key drag / row click** (jump playhead; move key) | full 18 pt cell hit | drag/click | — | any cell |
| 4 | **Time rules 6/12/24 + seconds gutter** (spec §5.3) | 1–2 dev px; gutter 44 pt | passive | Legend α-steps; labels Mono 11 Legend | grid + gutter |
| 5 | **Current row** | weight change only, never size | passive | TALLY_WELL ground + Tally gutter notch; Mono SemiBold Struck | playhead row |
| 6 | **Gutter scrub lane** — pen-drag in the 44 pt seconds gutter scrubs the playhead; an extension of the jump the row-click already performs, on the same view-state write | full gutter width (a 44 pt pen lane) | drag | notch Tally | left gutter |
| 7 | **Camera/param columns** (C1) | PARAM_COL_W 58 | click-to-key via existing strip | heads Legend caps 9.5; values Mono 11 Struck; keyed cell = Tally dot | right of drawing cols |
| 8 | **Audio waveform column** | AUDIO_COL_W 46 | passive | Legend 1-px strokes (app-rendered reference; never Ao — Ao is scaffold, not sound) | rightmost |
| 9 | Column heads + `+` detent (new drawing / expose sel. / **LIFT KEY**) | ROW_H 18; `+` 22×22 | DETENT menu | Legend caps; active column = Tally underbar notch, not blue text | column head row |

**B. The sheet head** (henshū lens — ONE fixed 26 pt row replacing today's ~145 pt header pile). This is where the Amendment's sparse space is spent; the row never reflows. Sizes follow the XL law: the 26 pt row pitch caps height, so the doubling lands wholly in width.

| # | Element | Size | Kind | Colour | Justification |
|---|---|---|---|---|---|
| 10 | **CUT ◀ / ▶** prev/next | **44×26 each** (XL: 2 × CTRL_H in the free axis) | momentary | Legend chevrons; Tally fill on press | verb #4, many/hr, executed pen-in-hand between loops; the sheet *displays* the cut, so cut nav mounts on the sheet head — the channel-strip law, and no transport enters the leaf (these change *which document section*, not the playhead) |
| 11 | **Cut name** | **96×26**, pinned, Well recess (longer names elide) | DETENT menu (existing cut ▾ content: cut list, rename, XDTS in/out; the seed of C2's browser, not built past that) | Mono Struck on Well | the room's identity field; the Slate's cut field stays a passive readout |
| 12 | **LIFT KEY** | **84×26** (XL, 2× its text-button width) | **DANGER** (Aka 1-px edge + 350 ms hold, filling Tally ring) | Aka edge, Legend label | verb #3's destructive half, promoted to standing furniture in this room only, under the Amendment: the big target is safe *because* the guard is temporal, not spatial — Fitts's law works for us, the hold works against slips. Acts on active column × current frame. Genga keeps LIFT KEY behind the `+` menu (defect 7); the two placements are one law read at two frequencies |
| 13 | **HOLD readout** | plate field, 26 pt row | passive | `HOLD` Legend caps · `6f · on 2s` Mono Struck | answers "am I on twos" for the hovered/current run without counting rows; the *editor* of that fact remains the tail drag |

**C. The Foot** (global chassis — this room's console; inherited Phase 3): painted transport `|◀ ◀ ▶ ▶ ▶|` at CTRL_H 22 · editable frame DragValue, Mono SemiBold 17 — the only large type in the app, and it belongs to this room's verb #2 · loop LATCH (Tally left-edge bar) · fps Legend · three status lanes. No room-local resize: the Foot is muscle memory across rooms.

**D. The right half:** Viewer (fronted tab) — the composite frame, clean, zero controls added. Canvas (rear tab) gains a fixed 26 pt tool strip carrying the Select shape DETENT pair (rect/lasso) — the armed tool's options, today unreachable in rooms whose dock lacks a Brush pane. NodeGraph below viewer: review furniture; its *behaviour* untouched, its off-law ink re-dressed (BUILD DELTA 1). Layers under the sheet: readout of the current cel, untouched.

### NOT IN THIS ROOM

- **Brush pane, the pencil box (ATARI/GENGA/SHUSEI), eraser, dynamics ⚙** — genga's. Hidden with the pane; hotkeys 1–8 still work but the room offers no targets.
- **Palette/swatch board, Fill and its options, miss-check** — shiage's.
- **Onion controls and the lightbox rail** — genga's; onion is armed off here (`workspace.rs:184`) and the rail hides with the rear Canvas tab.
- **Drawing library as standing furniture** — reachable via the `+` detent's "expose…", never spread across the header (that real estate is the grid's: ~53 visible rows ≈ 2.2 seconds).
- **Any clip/trim/filmstrip metaphor** — structurally banned; a drawing is a duration, the run mark is its only truthful body.

### DIFFERENTIATION — the hand finds without reading

**Position law:** bottom edge = time (Foot, identical in all rooms); sheet top-left = document verbs; grid = the work itself; right = the picture. **The room's only Aka-edged control and only press-hold is LIFT KEY** — destruction is findable by feel (the hold) before it is readable. **The only chevron pair is cut nav; the only filled triangles are transport** — same family, different form, different edge of the screen. Tally has one meaning everywhere the eye lands: playhead notch, current row ground, armed detent, pressed button.

Painted marks (`icons.rs` doctrine; Plus, rect-select, and lasso already shipped — this room's additions):
1. **CHEVRON-L/R** — shared with shiage (cut nav here)
2. **PLAY** — filled 3-pt `convex_polygon`; fills Tally while playing (armed)
3. **STOP** — `rect_filled`
4. **STEP-BACK/FWD** — triangle + 1 dev px bar
5. **TO-START/END** — triangle + heavier bar
6. **LOOP RING** — `circle_stroke` with gap + 3-pt arrowhead (LATCH)
7. **LIFT-KEY** — `circle_stroke` (the key dot) + apex-up chevron above it (the lift)
8. **NOTE** — `circle_filled` + vertical ascender (♪ head)
9. **CARET** — apex-down 3-pt polygon (all detent menus)
10. **CROSS-TICK** — 9 pt `line_segment` (spec §5.2, existing)

### KEYS

Verified against the Action table (`config.rs:129-189`); free-key audit confirmed Q W R P PageUp/PageDown clear:

| Verb | Binding | Status |
|---|---|---|
| Play/pause · step · home/end | `Space` · `F`/`S` · `Home`/`End` | exists |
| Lift key | `Backspace` | exists as `ClearFrameKey`; display string "Remove frame from X-sheet" → **"Lift key (hold extends)"** |
| Prev/next KEY (jump by exposure) | **`Q` / `W`** | new — spec Phase 2; the ratified cross-room default |
| Flip | **`R`** (hold) | new — spec Phase 2 |
| Prev/next CUT | **`PageUp` / `PageDown`** | new UI-only `PrevCut`/`NextCut` |
| Loop latch | **`P`** | new UI-only `ToggleLoop` (today a mouse-only checkbox, `main.rs:1758-1765`) |
| Undo/redo · save | `Ctrl+Z`/`Y` · `Ctrl+S` | exists |

### BUILD DELTA

1. **[1 ink]** Re-dress the room's actual remaining off-law ink: the `xsheet_panel.rs` heads are already plate-tokenized (Phase 1 landed there), so the purge target is **NodeGraph**, which this room fronts — `from_rgb(120,190,255)` at `nodegraph_panel.rs:123,145,206,231` → plate tokens; selected/active = Tally notch fill per the One-Current law. (The pen-curve editor's twin blue at `config.rs:1287,1307` lands with genga's Phase-1 pass.)
2. **[1 ink]** `clear key` → **LIFT KEY**, DANGER-styled (Aka edge + 350 ms hold) wherever it appears; config display string updated; stale "(N)" hover at `xsheet_panel.rs:65` corrected — the bind is `E`.
3. **[1 ink]** Export progress window + cut ▾ menu restyled to plate tokens; export completion reports via the decaying CHATTER lane, never a lamp.
4. **[3 geometry]** Per-stage pane-body lensing: the x-sheet panel renders the henshū sheet head (elements 10–13, one fixed 26 pt row) in Edit and the spec's evicted minimal head elsewhere — pane *content* lensing via the stage input to `EditorTabs::ui` (`main.rs:886-923`); `Stage::dock()` untouched.
5. **[3 geometry]** Sheet-head geometry per §B: chevrons 44×26, cut detent 96×26, LIFT KEY 84×26, HOLD plate field; `allocate_exact_size` slots, zero reflow.
6. **[3 geometry]** Enlarged hit zones on the run marks: tail cross-tick 22×22, key-drag full cell (paint unchanged from Phase 2's pass; only hit-testing grows).
7. **[3 geometry]** *(inherited dependency, global Phase 3)* the Foot — painted transport, editable frame DragValue, loop LATCH — replaces the top-bar text transport (`main.rs:1726-1765`). This room's verbs #1–2 land with it.
8. **[3 geometry]** New actions `PrevCut`/`NextCut`/`ToggleLoop` in the Action table (UI-only dispatch, same pattern as PlayPause) — lands with delta 7.
9. **[4 furniture]** Gutter scrub lane: pen-drag in the 44 pt seconds gutter drives `view.frame` — pure view-state write, the same path row-click already uses.
10. **[4 furniture]** Canvas-tab tool strip (fixed 26 pt) hosting the Select shape DETENT pair in rooms whose dock lacks a Brush pane; the Slate arming line shows `select · rect` so the armed tool is never invisible.
11. **[4 furniture]** ♪ column-head detent (import/remove WAV) replacing the header audio row (`xsheet_panel.rs:81-105`); waveform column restyled to Legend strokes.
12. **[4 furniture]** Param columns finished: Mono 11 values, Tally key dots, existing keying strip (`xsheet_panel.rs:402-469`) restyled to plate tokens.

Nothing here duplicates art, edits exposure semantics, or approaches the wgpu path; every delta is paint, hit-testing, lensing, or a keybind on an existing state write.

---

## The three genga pencils

The triad installs at `default_presets()` positions 0–2 (`config.rs:495-532`); slot = position = key (`config.rs:1110-1118`), so keys `1`·`2`·`3` are ATARI·GENGA·SHUSEI forever. The `name` fields below are canonical — they are what the slots engrave and what the Slate and status line display verbatim (`main.rs:502`); the earlier working labels ("P1 ao construction" etc.) are retired. Every field of `BrushPreset` (`config.rs:445-468`), per preset; colours are byte-identical to SWATCHES (`canvas.rs:31-36`) so preset and swatch read as one system.

| field | **ATARI** (key 1) | **GENGA** (key 2) | **SHUSEI** (key 3) |
|---|---|---|---|
| `name` | `atari` | `genga` | `shusei` |
| `size_px` | 14 | 6 | 8 |
| `flow` | 0.45 | 1.0 | 0.7 |
| `opacity` | 0.8 | 1.0 | 1.0 |
| `dyn_size` | true | true | true |
| `dyn_opacity` | true | false | true |
| `min_size` | 0.15 | 0.3 | **0.325** |
| `color` | [83, 137, 196, 255] — SWATCHES ao | [25, 25, 30, 255] — SWATCHES ink | [228, 82, 47, 255] — SWATCHES aka |
| `tilt_size` | true | false | true |
| `tilt_opacity` | true | false | **false** |
| `tilt_shape` | true | false | true |
| `tilt_strength` | 0.7 | 0.5 (stock default) | 0.5 |
| pressure sweep | 2.1 px → 14 px | 1.8 px → 6 px | 2.6 px → 8 px |

**ATARI — why.** `size_px` 14: the construction pass works in whole volumes and arcs, not contour — double the genga line's weight so rough marks read as scaffold at a glance, and the size gap alone differentiates the slot. `flow` 0.45: dab alpha = flow × curve(pressure) (`canvas.rs:1942-1945`), so with `dyn_opacity` full-range the flow value IS the opacity-dynamic's ceiling — 0.45 makes even a full-pressure dab semi-transparent, forcing the buildable, hatch-to-darken behaviour of a soft blue pencil. `opacity` 0.8: the wet-buffer whole-stroke cap (`canvas.rs:239-242`) guarantees no ao stroke ever reaches solid — the "ao does not photograph" law, visually encoded; construction stays subordinate to ink on the same cel. `dyn_opacity` true is the headline — pressure drives opacity strongly, and the median+EMA prefilter (`canvas.rs:201-203, 55`) makes the full-range mapping stable rather than flickery. `min_size` 0.15: a wide pressure→size sweep needs a low floor (2.1 px at feather → 14 px at full, `canvas.rs:1936`) but not zero — 0.15 keeps exploratory ghost passes visible above the 0.5 px absolute radius floor, and the 5-point end taper (`canvas.rs:56-58`) still lifts to a ~1 px tip. Tilt all-on at strength 0.7: construction is the pass where side-of-the-lead pays — a flat pen broadens ~1.7×, lightens up to ~52%, and the stamp flattens to ~2:1 along the lean (`canvas.rs:1839, 1943, 1951`), turning the tilt gesture into volume shading; 0.7 over the 0.5 default because this is the only preset that wants tilt loud.
*Feel:* light — a ghost-blue hairline that barely marks; you build tone by hatching. Medium — ~8 px soft translucent blue, visibly darkening where strokes cross. Heavy — full 14 px building toward the 80% stroke cap: assertive but never solid, always under-drawing. Lay the pen over and the mark goes wide, pale, and elliptical along the lean.

**GENGA — why.** `size_px` 6: continuity — the owner has been drawing with the existing 6 px pen (`config.rs:497-502`), so the committed line's maximum weight stays exactly what his hand knows; only the floor changes. `min_size` 0.3: the tight floor — pressure sweeps 1.8 px to 6 px (a controlled 3.3:1) so the line modulates expressively without collapsing to invisibility mid-contour, and the end taper still exits at 0.9 px radius — a clean pencil-lift tip, neither needle nor stub. `dyn_opacity` false is a deliberate engine-fact call: dyn_opacity has no strength knob — it maps alpha across the FULL curve range (`canvas.rs:1942`) — so enabling it would grey out every light-pressure passage, and the committed key line must scan and photograph as uniform ink. The Amendment's pressure+opacity exploitation lives in ATARI and SHUSEI; GENGA's pressure channel is size, crisply. `flow` 1.0 + `opacity` 1.0: every dab lands full density, the stroke ceiling is solid — no buildup behaviour on the line douga will trace. Tilt all-off: the committed line is immune to incidental hand lean — predictability is the feature; `tilt_strength` left at the 0.5 stock default so it behaves like stock if ever toggled on.
*Feel:* light — a fully-black 1.8 px hairline: never grey, just thin; ticks and darts stay crisp. Medium — ~4 px living graphite contour, width breathing with the hand. Heavy — the full 6 px accent weight. The same stroke at any speed and any lean produces the same ink — the dependable line.

**SHUSEI — why.** `size_px` 8: a step heavier than the 6 px line it corrects over — the aka pass must win the eye against dense linework without shouting like the 14 px rough. `flow` 0.7: with `dyn_opacity` on, flow is the opacity ceiling (`canvas.rs:1945`) — 0.7 gives the sakkan the real working gesture: a light trial pass floats a faint red suggestion, then bearing down firms the same line toward solid, in one preset. `opacity` 1.0: unlike ao's 0.8 cap, a committed correction may build to fully solid red — the overrule must dominate the sandwich (the correction layer sits above line per `layers-per-cel-design.md`). `min_size` 0.325: the floor lands at 2.6 px (8 × 0.325) — above GENGA's own 1.8 px floor, so even feather-weight annotation marks stay legible over busy line art; the sweep is still an expressive 2.6–8 px. `tilt_size` + `tilt_shape` true at stock 0.5: corrections include form and shading indications, so leaning the pen broadens and flattens the mark (up to 1.5× wider, ~1.75:1 ellipse) for quick volume notes — but `tilt_opacity` false, because a correction must NEVER fade from hand posture; its legibility over two other passes is its entire job.
*Feel:* light — a faint 2.6 px red whisper, the trial stroke the sakkan floats before committing: visible but retractable. Medium — ~5 px confident red at building density. Heavy — full 8 px near-solid red that sits unmistakably on top of both the ao scaffold and the ink line. Leaning the pen widens and flattens the mark for shading callouts, but the red never pales — corrections stay loud at any posture.

---

## Phase absorption

Every delta lands in an existing spec phase. **No new phase is required.**

**Phase 1 — INK**
- Icon-kit extensions to shipped `icons.rs`: specimen-stroke painter, onion twin-circles (genga); CHEVRON-L/R, HOLE, SHACKLE (shiage); PLAY/STOP/STEP/TO-END/LOOP RING/LIFT-KEY/NOTE/CARET (henshū/Foot).
- Genga: layer-identity inks (Ao/Aka text); ERASE CEL DANGER + eviction; defect-5 completion on the shipped swatches; pen-curve editor blue (`config.rs:1287,1307`) to plate tokens.
- Shiage: pot restyle (Well seat, Legend edge, Tally ring); fill-option label mediums (`canvas.rs:1102-1105`); remove buttons to DANGER.
- Henshū: NodeGraph blue purge (`nodegraph_panel.rs:123,145,206,231`); LIFT KEY rename + DANGER dress + "(N)"→E hover fix (`xsheet_panel.rs:65`); export/cut-menu restyle, completion via CHATTER.

**Phase 2 — dependencies only (already chartered by spec §5/§7, not re-chartered here)**
- Run model, mirror strip, run marks; `NextKey`/`PrevKey`/`Flip` actions with the ratified `Q`/`W`/`R` defaults. Genga's flip, shiage's cel advance, and henshū's key-jump all ride these.

**Phase 3 — GEOMETRY**
- Global: stage input to `EditorTabs::ui` (`main.rs:886-923`); X-sheet header eviction + `+` detent; the Foot (transport, frame, loop) replacing the top-bar text transport; slider medium fixes.
- Genga: pencil box + OPTIONS row; preset defaults swap (triad at 0–2); active-preset tracking; mid-stroke guard (`main.rs:988-991`); clamp/compact retirement (`canvas.rs:1027-1028`); new actions `BrushSizeDown`/`BrushSizeUp`.
- Shiage: swatch-board rebuild + EDIT MODEL latch; brush-strip fixed slots + options-row content; Slate pot name; Cel Layers fixed grid.
- Henshū: sheet-head row (chevrons, cut detent, LIFT KEY, HOLD); run-mark hit zones; new actions `PrevCut`/`NextCut`/`ToggleLoop`.

**Phase 4 — FURNITURE**
- Genga: lightbox rail; onion vector-only re-ink.
- Shiage: canvas-rail chevron pair + MISS CHECK (Graphite ground, UI paint verified) + LINE GUARD (arming-only) + pot-key lens + `M` binding.
- Henshū: gutter scrub lane; canvas-tab tool strip; ♪ detent + waveform restyle; param-column finish.

**Outside all phases — flagged, each needs its own future gate, not a new phase of this build:**
- Shiage alt-click role-pick (a re-query, but a new interaction surface).
- Shiage variant-grid palette model (a document-format change — forbidden under NEVER-DO 1 until its own gate opens).