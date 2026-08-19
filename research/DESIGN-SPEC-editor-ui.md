<!-- RATIFIED for implementation 2026-08-17 under research/PSD-editor-repaint.md.
     Root: the repaint never changes what the document contains or how the
     pen reaches it. Phase boundaries and the Do-NOT-build list bind as law. -->

# AnimStudio — FINAL DESIGN SPEC (editor UI)

Target: Rust / egui 0.35 / epaint (harfrust + skrifa), 2560×1351 @ ppp 1.25.
Source of truth for geometry: `app/src/xsheet_panel.rs` (`ROW_H = 18.0`, `FRAME_NUM_W = 34.0`, `COL_W = 86.0`), `app/src/canvas.rs:1314`, `app/src/main.rs:1564` (`egui::Panel::top`), `main.rs:2090` (`Panel::bottom`), `main.rs:922` (`closeable`), `main.rs:2215` (`separator.extra = 26.0`).

---

## 1. DIRECTION

A printed X-sheet is a plate carrying permanent engraved legend — rules, column heads, second marks, all in one fixed printing ink — over which the animator lays a second, different ink in pencil, and the entire value of the arrangement is that you can always tell which ink is which. Everything the *application* says (labels, units, headings, rules, tab names) is PLATE: engraved, quiet, permanently present, never bright, never hidden behind a hover; everything the *animator* has made or armed (a key, a hold, a stroke, the current frame, the live tool) is PENCIL: brighter, in the trade's own pencil colours, carried on the plate. The app's present failure is exactly the collapse of those two inks into one — a heading and a selection share a blue, a broken glyph and a checkbox share a square, a destructive command and a settings menu share a grey — so the whole budget goes to typographic exactness, a snapped spatial grid, and a colour system in which no colour is permitted to mean "important," only to mean one specific thing.

---

## 2. TOKENS

### 2.1 Palette (`app/src/plate.rs`, the only colour module; `main.rs:84` remains the only `Visuals` site)

| Token | Hex | RGB | On Graphite | Role |
|---|---|---|---|---|
| **Graphite** | `#1A1B18` | 26,27,24 | — | The plate. Every panel, bar, rail, chrome surface. Warm-neutral (olive, not blue-black) so it reads as anodized surround and does not fight Paper. |
| **Well** | `#0E0F0D` | 14,15,13 | — | The one darker step. Recessed data fields ONLY: the x-sheet grid, the layer list, the drawing library, the frame `DragValue`. A sunken field is how a panel says "the data lives here." There is no third grey step — raised/hover is made with Legend rule or Tally, never with more grey. |
| **Legend** | `#8B8D83` | 139,141,131 | 5.2:1 | The engraved printing ink. Every word and rule the APP authors: panel titles, column heads, unit labels, second/half/beat rules, non-current frame numbers, inactive controls, the onion ghost AHEAD. |
| **Struck** | `#E4E1D6` | 228,225,214 | 13.9:1 | The bright fill of the engraving. Facts the ANIMATOR authored or is reading: drawing names in cells, numeric values, frame counter, layer name, filename. |
| **Ao** | `#5389C4` | 83,137,196 | 5.0:1 | The blue pencil (ao-enpitsu — does not photograph). ONE meaning: CONTINUITY AND SCAFFOLD. The hold rule, the onion ghost BEHIND, the roughs/construction layer identity, field & peg guides on the paper. **Never a selection.** |
| **Aka** | `#E4522F` | 228,82,47 | 4.75:1 | The red pencil (the sakkan's correction). ONE meaning: THIS OVERRULES YOUR HAND. Correction marks, destructive commands, refusals, armed-vs-actual contradictions. Always text or edge, never a fill. |
| **Tally** | `#EFC24A` | 239,194,74 | 10.6:1 | The lamp. The ONLY colour meaning CURRENT / ARMED: current-frame notch, key dot at a hold's head, armed tool detent, active room, active column, layer receiving ink. Always a fill, notch or dot, never text. |
| **Paper** | `#F2EEE6` | 242,238,230 | — | The canvas material. Not a UI token. Unchanged. |

Derived, no new hues permitted:
- `LEGEND_DIM` = Legend @ α115 — disabled controls.
- `RULE_BEAT` = Legend @ α56 (22%), `RULE_HALF` = Legend @ α102 (40%), `RULE_SEC` = Legend @ α255.
- `TALLY_WELL` = Tally @ α36 — the current row's ground inside the Well. This **replaces** the saturated navy `from_rgb(45,62,88)` slab entirely; there is one "current" and it is amber.

Deleted from the codebase, by grep, in Phase 1: `from_gray(38)`, `from_gray(31)`, `from_rgb(45,62,88)`, `(230,160,90)` (positioned-handle amber), `(235,90,80)`, `(120,200,140)`, `(120,200,120)`, `(215,70,70)`, `(70,175,90)`.

### 2.2 Type

**Family: IBM Plex.** Four static instances plus one subset JP. **Static, not variable** — epaint 0.35 does support variations (`VariationCoords`, `ShaperInstance::from_variations`, HVAR advances), but IBM Plex Mono has no official variable release, and determinism at 9.5pt matters more than 3 MB. **Tabular figures are unavailable** — not because features are unimplemented, but because `shape(buffer, &[])` passes an empty user-feature list — so columns of digits must come from a monospace, structurally, and never from `tnum`.

Ship: `IBMPlexSans-Regular`, `IBMPlexSans-SemiBold`, `IBMPlexMono-Medium`, `IBMPlexMono-SemiBold`, `IBMPlexSansJP-Bold` **subset to ~200 glyphs** (kana + 原画仕上編集撮影動画色指定演出中割背景). Keep `default_fonts` installed as the last fallback entry so an unexpected Windows path glyph degrades to a shape rather than tofu.

| Role | Face | Size (pt) | Colour | Notes |
|---|---|---|---|---|
| Engraved caps — panel titles, column heads, group legends | Plex Sans SemiBold | **9.5** | Legend | UPPERCASE, `TextFormat::extra_letter_spacing = 0.9` (exists at `epaint::text_layout_types.rs:477`, applied at cluster boundaries). One helper `plate::legend(ui, &str)`. |
| Control labels | Plex Sans Regular | **11.5** | Legend; Struck when armed | |
| Sheet cells, frame numbers, all numeric fields | Plex Mono Medium | **11** | Legend (numbers), Struck (drawing names) | Current row steps to SemiBold + Struck. **Weight only — never size.** The row must not reflow as the playhead passes. |
| Frame counter (Foot) | Plex Mono SemiBold | **17** | Struck | `/48` beside it at 11pt Legend. The only large type in the app. |
| Stage tabs | Plex Sans JP Bold **13** over Plex Sans SemiBold **8.5** caps +0.9 | | Legend / Struck+Tally when active | All four rooms get identical two-line structure. |
| Foot status lanes | Plex Sans Regular | **10.5** | per lane | |

No italics. No third family. Emphasis comes from colour role or a painted rule — never from a new size.

### 2.3 Spacing scale (logical points)

`2 · 4 · 6 · 8 · 12 · 16 · 24`. Nothing else. Fixed structural dimensions:

```
ROW_H          18.0   (unchanged — 22.5 device px at ppp 1.25, a real click target)
GUTTER_W       44.0   = SEC_W 18 + FR_W 26     (was FRAME_NUM_W 34)
COL_W          86.0   (unchanged)
SLATE_H        42.0   global Panel::top
FOOT_H         34.0   global Panel::bottom
OPTIONS_H      26.0   always allocated, always present, in the brush bar
RULE_STRIP_W   14.0   canvas left edge, the mirror
LIGHTBOX_W     96.0   canvas left rail (Phase 4)
CTRL_H         22.0   every button, toggle, detent
```

**Pixel-snapping law.** Every rule, notch, dot and 1px edge is snapped: `fn snap(v: f32, ppp: f32) -> f32 { (v * ppp).round() / ppp }`. Rule thickness is specified in *device* px and converted: beat/half = 1 device px, second = 2 device px. Unsnapped hairlines shimmer under scroll at ppp 1.25 and this direction has no ornament to hide behind.

---

## 3. COLOUR SEMANTICS — the law

Each colour has exactly the meanings in its row. Any other use is a bug with a name.

| Colour | ALLOWED to mean | FORBIDDEN |
|---|---|---|
| **Graphite** | chrome surface | data ground |
| **Well** | "data lives here" (recessed field) | a hover or pressed state |
| **Legend** | the app authored this: labels, units, headings, rules, inactive controls, the onion ghost AHEAD | anything the animator authored; any selection |
| **Struck** | the animator authored this / is reading this value | labels, headings, rules |
| **Ao** | continuity & scaffold: hold rule, ghost BEHIND, roughs layer identity, field/peg guides | selection, active tab, active tool, headings, "visible" |
| **Aka** | refusal, destruction, correction, contradiction | any configuration, capability, absence, or informational state |
| **Tally** | current / armed — as a fill, notch or dot | any text; anything not currently current |
| **green** | *nothing. There is no green in this application.* | health, success, "OK" |

**Four enforceable laws.**

1. **Two-ink test.** If the app wrote it → Legend. If the animator wrote it or is about to act on it → Struck. Checkable against the running app; not a matter of taste.
2. **One current.** Tally is the only "current," and it is always a painted fill/notch/dot. Aka is always text or a 1px edge. Two warm colours, never confusable at 11pt, because they never take the same form.
3. **A horizontal rule means TIME and nothing else.** Painted horizontal lines exist only in the x-sheet grid and the canvas-edge strip. Everywhere else, separation is a vertical rule or the Graphite→Well plane step. Grep-enforceable.
4. **THE WARNING LAW — Aka marks a CONTRADICTION, never a CONFIGURATION.**
   > Test: *could this be true for the whole session while the animator is working correctly?*
   > **Yes → it is a plate fact.** Legend label + Struck value. Aka is forbidden.
   > **No → it is a contradiction between what is armed and what is happening.** Aka is required.

   Worked cases, binding:
   - `INPUT · MOUSE — NO PRESSURE` — true all session, animator working fine. **Legend label, Struck value, no colour.** It is a statement of the instrument's configuration, printed on the canvas rail's plate. The current build's red here is the exact defect this law abolishes.
   - Pressure dynamics ARMED, device delivering flat pressure → armed state contradicted → **Aka**, at the canvas edge.
   - Tablet delivered pressure earlier this session and stopped → contradiction with the session's own history → **Aka**.
   - "layer hidden" — a configuration you chose. **Not Aka.** A struck-through row at `LEGEND_DIM`.
   - "refused — finish the stroke first" — the machine overruled your hand. **Aka**, and it flashes the canvas edge.
   - "erase cel", "delete layer", "lift key" — destruction. **Aka** edge + guarded press-hold.
   - "painted", "exporting…", "room: genga" — chatter. Legend, decaying.

   There is **no green OK lamp anywhere**. On an instrument, health is shown by a meter moving, not by a lamp saying fine. Pressure health is shown by the pen's line varying — which is the work itself.

---

## 4. LAYOUT

### 4.1 The four top-bar jobs, resolved

The top bar currently does four unrelated jobs. Three are evicted onto the thing they control (channel-strip principle: *a control lives on the section it controls*); one is evicted downward to the chassis.

1. **Document identity + room switching → THE SLATE** (global `Panel::top`, 42pt, two rows). Left: film-slate fields — filename, dirty mark, cut, length, fps, resolution — and beneath them the arming line (`D4A · col A · line · ink`). This puts filename and dirty flag in the top-left where they belong and converts `1920×1080 24fps` from dead chrome into a legitimate slate field. Right: the four rooms as engraved two-line tabs, all four with identical structure (kanji over romaji), all four framed. The `AnimStudio` wordmark is **deleted** — the operator knows what he opened.
2. **File commands → one detent.** `≡` at the far left opens new/open/save/export/settings. Ellipsis convention applied uniformly: every entry that opens a dialog takes one, so the convention finally teaches something.
3. **Transport + time → THE FOOT** (global `Panel::bottom`, 34pt), **not the x-sheet.** Transport is global state (`playing`, `view.frame`), and the x-sheet is a dock leaf that `main.rs:2215` deliberately allows the animator to collapse to a slim rail and `main.rs:928` allows him to close outright. A tape deck's transport is bolted to the chassis, not to a removable reel. The Foot carries: `|◀ ◀ ▶ ▶ ▶|` (painted, not ASCII), an **editable frame `DragValue`** (`  7`/48) — the number an animator most wants to type into stops being the one number that cannot be typed into — loop latch, fps readout, then the three status lanes.
4. **Onion / display → THE CANVAS LEFT RAIL** (96pt `Panel::left` *inside* the Canvas leaf; Canvas is non-closable per `main.rs:922`, so the rail dies only with the thing it describes). Onion is a property of the light box, not the file. Carries: onion latch, back depth, forward depth, strength (the dial the code currently hard-codes at 2/2 with alphas 100/55), the input-state field, and paper furniture (field / safe / peg). "line only" is indented **under** onion as its child and renamed **"ghost active layer only."** Ships in Phase 4, not before.

**The dock is not touched.** `workspace.rs`'s LENS-DOCK subsystem — nine pane kinds, the panes menu, serde-persisted `workspaces.json`, per-stage re-lensing — survives intact. Only the Slate and the Foot are global panels; every rail is a panel *inside* a leaf.

### 4.2 Wireframe

```
┌ SLATE ── global Panel::top, 42pt ────────────────────────────────────────────────────────────────────────────┐
│ ≡  ktr_cut01.anim ●   CUT01 · 48f · 24.000 · 1920×1080                    レイアウト│ 原画 │仕上げ│ 編集     │
│    D4A · col A · line · ink                                                LAYOUT │GENGA │SHIAGE│HENSHŪ     │
├──────────────────────┬───────────────────────────────────────────────────────────┬───────────────────────────┤
│ X-SHEET          360 │ BRUSH   12 fixed slots · 34pt · NEVER reflows              │ CEL LAYERS            300 │
│┌sec┬ fr ┬─── A ─────┐│ ◉brush ○erase │ ○select ○fill │ ▤lock ▤comp │ ■ ■ ■ ■ │14px│┌vis┬i┬name───┬opac┬%┬⋯┐│
││   │  1 │ ●  D1A    ││───────────────────────────────────────────────────────────││ ◉ │▮│line   │▮▮▮▮│90│⋯││
││1s ├────┼───────────┤│ OPTIONS  always present · always 26pt · empty for brush    ││ ◉ │▮│colr   │▮▮▯▯│55│⋯││
││═══│  2 │ ┃         │├────────┬──────────────────────────────────────────────────┬┤│ ○ │▮│r̶o̶u̶g̶h̶ │▮▮▯▯│40│⋯││
││   │  3 │ ●  D2A    ││LIGHTBOX│R┌──────────────────────────────────────────────┐ ││└───┴─┴───────┴────┴──┴─┘│
││   │  4 │ ┃         ││ ◉onion │U│                                              │ ││ ADD LAYER      ▌ERASE ⌫ │
││   │  5 │ ┃         ││ back 2 │L│                                              │ │├─ PALETTE ───────────────┤
││   │  6 │ ┃         ││ fwd  1 │E│                                              │ ││ ink   ao    aka   white │
││2s ├────┼───────────┤│ str 60%│ │            P A P E R                         │ ││ ██    ██    ██    ██    │
││═══│  7 │◂●  D4A    ││────────│▮│                                              │ ││                         │
││   │  8 │ ┃         ││ field ▢│▮│                                              │ │├─ LIBRARY ─(drawer)──────┤
││   │  9 │ ┃         ││ safe  ▢│▮│                                              │ ││ D1A D2A D3A D4A         │
││   │ 10 │ ┃         ││ peg   ▢│▯└──────────────────────────────────────────────┘ ││                         │
││   │ 11 │ ○         ││────────│▯   100%     D4A · fr 7 · line                    ││                         │
││   │ 12 │           ││INPUT   │▯                                                 ││                         │
││3s ├────┼───────────┤│ MOUSE  │▯                                                 ││                         │
││═══│ 13 │ ●  D5A    ││ NO PRES│▯                                                 ││                         │
│└───┴────┴───────────┘└────────┴──────────────────────────────────────────────────┘└─────────────────────────┘
├ FOOT ── global Panel::bottom, 34pt ──────────────────────────────────────────────────────────────────────────┤
│ |◀ ◀ ▶ ▶ ▶|  [   7]/48  ⟳loop  24.000 │▌REFUSED — finish the stroke first│ painted      │ UNDO 12 · REDO 0  │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────┘

  ┃ = hold rule (Ao, continuous)      ● = key dot (Tally)      ○ = empty (Legend hairline ring)
  ◂ = current-frame notch (Tally)     ═══ = second rule (2 device px, Legend @100%)
  ├───┤ = half-second rule (1 device px, Legend @40%)     beat rule every 6f (1 device px, Legend @22%)
  R U L E ▮▯ = the mirrored strip at the canvas's left edge — see §5
```

**The Foot's three lanes** are not one channel: a left **REFUSAL** lane (Aka, dwells 4s, and simultaneously flashes the canvas edge so it is seen from the paper), a centre **CHATTER** lane (Legend, decays to nothing after 2s so a stale "painted" can never sit there training the eye to ignore the line), and a right **PERSISTENT** lane carrying undo depth like a tape counter.

**X-sheet geometry.** The ~145pt of header chrome (title, cols row, three buttons, wav, library) is evicted: the library becomes a collapsed drawer in the right pane, `new drawing / expose sel. / clear key` become a single `+` detent menu on the column head, `♪ wav…` moves into the `≡` menu. The panel title "CUT01 — X-SHEET" is deleted (the tab already says it). Result: ~965pt of grid at ROW_H 18 = **53 rows visible ≈ 2.2 seconds**, up from 25 rows.

---

## 5. SIGNATURE — THE RULE

**A drawing's life is one vertical stroke, and that stroke is repeated at the pen.**

### 5.1 The data model (build this first; everything else is paint)

Replace the per-row `key_at(frame)` lookup — the wrong query against a sparse `BTreeMap` — with a run model computed once per visible viewport:

```rust
pub struct Run { pub col: usize, pub start: u32, pub end: u32, pub drawing: DrawingId }
pub fn runs_in(doc: &Doc, col: usize, lo: u32, hi: u32) -> Vec<Run>
```

Runs are adjacent entries in an already-sorted map — roughly 10 lines. The core crate already stores timing as sparse keys with hold semantics, so runs are the model's *native* shape and the current 47-disconnected-islands rendering is a mis-query. The same pass hands **key-step** and **flip** their target frames for free; both get bound in `config.rs` (`NextKey`/`PrevKey`/`Flip`) in the same phase.

### 5.2 The sheet mark

Painted in **one pass, after** the `show_rows` loop, from `ui.painter()` obtained before the loop — never inside `painter_at(row_rect)`, which clips every stroke to one row. Runs beginning above the viewport are clamped to the visible band; runs ending below are clamped and drawn without a tail cap.

For each run, at column x-centre `cx = gutter + ci*COL_W + 22.0`:
- **Key dot**: `circle_filled(pos2(cx, snap(ctop + start*ROW_H + 9.0)), 3.5, TALLY)`.
- **Hold rule**: `line_segment` from the dot's bottom edge to `snap(ctop + (end+1)*ROW_H) - 1.0`, **Ao**, 2 device px. Unbroken. Ends flush where the next key begins.
- **Tail cap**: a 9pt horizontal cross-tick, Ao, at the run's last row.
- **Empty**: `circle_stroke(…, 3.5, 1px Legend)`.
- Drawing name: Plex Mono Medium 11, Struck, at the run's first row only, `cx + 10`.

**The hold rule absorbs the existing continuation handle** (`xsheet_panel.rs:521-600`). That code already computes runs (`Cont { n, max_end, terminator }`) and already drags a vertical line to set a hold's end — but stows it until hover and paints it in `(230,160,90)` amber, which the new law reserves for current/armed. The rule **is** the handle: always drawn, always in Ao; the tail cross-tick is the drag target; the whole run turns **Tally** only while hovered or dragged, which is exactly what Tally means. Two competing vertical duration marks collapse into one object.

### 5.3 The rules of time (grafted from THE SECOND RULE)

One ink, three weights, drawn as **boundaries above** the frame they begin — not as tints sitting on a row (the current tint is a row off from sixty years of paper habit):

| Every | Weight | Colour | Label |
|---|---|---|---|
| 6 frames (beat) | 1 device px | Legend @ 22% | — |
| 12 frames (half-second) | 1 device px | Legend @ 40% | — |
| 24 frames (second) | 2 device px | Legend @ 100% | `1s`, `2s`, … in the 18pt seconds gutter, Plex Mono Medium 11 |

The 6/12 hierarchy is mandatory and is not decoration: at 24 fps and ROW_H 18, a second rule falls every **432pt**, so on a 965pt grid you see two of them. The signature's claim — "three rows against the rule and it *looks* like a 3s from across the room" — needs a reference mark within a few rows, and the beat rule is it. Also add a 1px Legend @22% **vertical** column separator at each `COL_W` boundary (vertical rules are permitted; they mean space, not time).

The current row gets `TALLY_WELL` ground plus a solid Tally notch in the gutter's left edge, and steps to Plex Mono SemiBold + Struck. **No size change, no full-width slab.**

### 5.4 The mirror — why this is the signature and not a bug fix

The same run-painting function is factored out and called a second time with a different rect: a **14pt strip along the canvas's left edge, at exactly the same 18pt pitch**, immediately beside the paper. Same key dots, same Ao holds, same rule hierarchy (beat/half/second, ticks only, no labels), current frame as a Tally notch. For a 48-frame cut on a ~900pt canvas the entire cut fits the strip; for longer cuts it follows the playhead, current frame held at 40% of strip height.

The animator never looks at it directly and never needs to — it lives in peripheral vision at the pen tip, the way a VU meter does at a mixing desk. *"How long does this hold?"*, *"am I on ones here?"*, *"which frame am I on?"* — the three questions that currently cost a ~750pt saccade to the top bar or a hunt through 25 near-identical rows — are answered by a shape at the edge of the paper without the eye leaving the drawing.

What it encodes is true and is not decoration: **in animation a drawing is not an object, it is a duration.** A cel held for eight frames is a different thing from the same cel held for two, and every other representation in this app — a name in a cell, a row in a list, a thumbnail — hides that. The Rule states identity and duration as one mark because on a real sheet they are one mark.

**Explicitly not built:** the pen-down score dim. A genga artist lays hundreds of short strokes per drawing; an un-eased alpha toggle on the largest structured object in peripheral vision is a ~1 Hz strobe for hours, and eased it is a forced continuous repaint. Cut, not deferred.

---

## 6. DEFECT RESOLUTION (high severity)

| # | Defect | Fix |
|---|---|---|
| 1 | Tofu everywhere — ▱ ⬚ 🪣 ⬤ ● ○ ↑ ↓ ▾ ＋ ▣ │ ◆; one square means six things | **`icons.rs`**: every mark becomes a `Painter` primitive — `circle_filled`, `circle_stroke`, `line_segment`, `rect_filled`, three-point `add(Shape::convex_polygon)` for chevrons/carets. Font stack installed alongside, with `default_fonts` retained as final fallback. Resolution-independent, immune to font coverage forever. ~44 call sites. |
| 2 | Tool row reads as checkboxes ("raster on, eraser off") | `raster` **leaves the tool group entirely** → settings dialog (it is a GPU backend switch, not a tool). Remaining controls get painted detents (filled Tally disc in a Legend ring), which cannot be confused with a checkbox because there are no checkboxes left in the row. |
| 3 | Six controls, three behaviours, one presentation | Three explicit helpers in `plate.rs`: **DETENT** (radio — Tally disc, brush/erase/select/fill, mutually exclusive, second click does nothing), **LATCH** (independent — Tally left-edge bar, lock/comp/onion/loop), **DANGER** (Aka 1px edge + press-and-hold ~350 ms with a filling Tally ring). `comp` — which refuses painting — additionally paints a 1px Aka edge around the canvas while active, because it contradicts the armed pen. |
| 4 | Toolbar reflows on tool change; violates `canvas.rs:1314` | 12 **fixed slots** via `allocate_exact_size` per slot, plus an **always-allocated 26pt OPTIONS row** beneath. Select's 2 shape buttons and Fill's 4 options land in that row; it is empty for brush. Nothing downstream ever moves — vertically or horizontally. `fit view` leaves for the canvas rail. |
| 5 | Brush colour invisible — ink (25,25,30) on (26,26,26); presets are hollow outlines | Swatches become painted `rect_filled` at 18×18, seated in a **Well** recess with a 1px Legend edge, armed swatch ringed in Tally. Palette order is the pencil triad by name: `ink · ao · aka · white`. The blue/red-pencil vernacular becomes readable by colour alone, peripherally, without stopping the stroke. |
| 6 | `clear cel` identical to `dynamics`, adjacent, no confirmation | Renamed **ERASE CEL**, moved out of the brush strip to the Cel Layers pane footer, styled DANGER (Aka edge + 350 ms hold). |
| 7 | `clear cel` / `clear key` naming collision | Split by verb and by domain: **ERASE CEL** (pixels, brush/layers) vs **LIFT KEY** (exposure — you lift a key off a sheet and the hold extends; x-sheet column menu). Different first letter, different verb, different panel. |
| 8 | Sheet has no timing metric — second at 1.14:1, playhead is the loudest object | §5.3 rule hierarchy in Legend ink + seconds gutter labels; playhead demoted from a full-width saturated slab to `TALLY_WELL` ground + a gutter notch. |
| 9 | Exposure grammar collapses; hold drawn as per-row text | §5.2 — one continuous painted mark per run, key dot / rule / cross-tick, name only at the run head. |
| 10 | Cel Layers rows drift 10–11px with name length | Fixed column grid via `allocate_exact_size`: `[vis 22][ident 10][name flex, elided][opacity 64][pct 34][⋯ 22]`. Every control in every row at the same x, forever. |
| 11 | Reorder = two identical squares; delete is the smallest target beside them | Reorder becomes painted chevrons (`convex_polygon`, apex up/down) at fixed columns, 26pt targets. **Delete leaves the row entirely** → the pane footer, DANGER-styled, acting on the active layer. Fitts's law stops pointing at the irreversible control. |
| 12 | One blue means seven things; a second blue means "current frame" | The palette law (§3). Ao is demoted to one meaning (scaffold) and is banned from every selection. Headings become Legend. Selection/current becomes Tally-as-fill. The navy is deleted. |
| 13 | Pen warning is a red label 2,300px from the pen, names no location | Two changes. (a) **The Warning Law** — `MOUSE · NO PRESSURE` is a configuration, so it is printed as a plate field (`INPUT` in Legend, value in Struck) on the canvas rail, ~120pt from the pen. Red here becomes *structurally impossible*. (b) The rail's field is a control: clicking it opens this app's settings dialog at the pen page. Aka appears at the canvas edge only on contradiction (dynamics armed, no pressure delivered; or pressure lost mid-session). |
| 14 | Onion tint doesn't survive — multiply on black ink yields grey | Onion becomes **replacement ink**, not a multiply: `draw_strokes` already takes a colour, so pass the ghost colour at real saturation. **Ghost BEHIND = Ao. Ghost AHEAD = Legend.** Aka is *removed* from the forward ghost (grafted cut, flagged twice by judges): a large red drawing sitting on the paper for every hour of inbetweening is exactly the dilution that makes Aka unreadable, and the neighbouring key is not the sakkan's correction. Direction is additionally unambiguous from the canvas-edge strip, where ghost frames are marked around the Tally notch. Vector path only; see §7. |
| 15 | Stage spine — three of four rooms have no affordance; `layout` reads as a heading | Four identical two-line tabs in the Slate (kanji over romaji caps), all framed, all hovering, `layout` gains its gloss. Active room = Tally fill. |
| 16 | Active layer slot silently clamps on frame step, and swaps brush colour | The active layer is tracked **by name**, not by index. `active_layer_slot.min(n-1)` is deleted. If the named layer is absent on the cel you stepped onto, the slot goes **UNARMED**: the Tally goes dark, no layer receives ink, the pen refuses with an Aka refusal in the Foot and a canvas-edge flash. **The brush colour never changes without a gesture** — `canvas.rs:1332-1346`'s colour swap is deleted. Silence is never an acceptable response to state changing under the hand. |

*Mediums closed by the same changes:* slider label-after-control (labels move before, values in Plex Mono at a fixed right column, all three tracks the same width); layer chip reading as a text field (becomes a DETENT menu with a painted caret); x-sheet header pile and duplication (evicted, title deleted); no key-step/flip (falls out of the run model); onion depth/strength hard-coding (rail dials); status channel (three lanes with decay); dock double-✕ (the leaf-level ✕ is removed; the tab ✕ stays, the pane menu moves to the top-right where a menu is expected); ellipsis/case convention (uniform in the `≡` menu); `1920×1080 24fps` in prime space (becomes a slate field); missing filename/dirty flag (slate, top-left).

---

## 7. BUILD ORDER

**Phase 0 — the plate (½ day).** `plate.rs`: the eight tokens, the derived alphas, `snap()`, `legend()`, `value()`, and the three affordance helpers (DETENT/LATCH/DANGER). Font stack: `FontDefinitions::default()` → insert four `FontData` blobs + subset JP appended as a fallback entry in both family vectors (egui resolves per-glyph down the vector, so 原画 falls through automatically). Fonts are greenfield — zero `FontDefinitions`/`font_data` in the tree — so there is nothing to unwind. This alone converts the app from "default egui" to "this was designed."

**Phase 1 — INK (1½ days).** `icons.rs` and the glyph purge (~44 call sites). Palette law applied: kill the seven listed literals, headings to Legend, selections to Tally. Swatches. Three affordance kinds wired. ERASE CEL / LIFT KEY split and guarded. Highest legibility return per line changed, zero blast radius outside styling.

**Phase 2 — THE RULE (1½ days).** `runs.rs` + `runs_in()`. Rule hierarchy 6/12/24 with the seconds gutter (`GUTTER_W` 34→44). Continuous hold stroke painted in one post-loop pass. The continuation handle absorbed. The canvas-edge mirror as the second call site. `NextKey`/`PrevKey`/`Flip` bound. This is the signature and the most expensive defect in the app; it depends on nothing in Phase 3 or 4.

**Phase 3 — GEOMETRY (2 days).** Cel Layers fixed column grid. Brush strip fixed slots + always-allocated OPTIONS row. X-sheet chrome eviction (library → drawer, buttons → column detent menu). The Slate. The Foot with the editable frame `DragValue` and three decaying status lanes. Layer-by-name + unarmed state.

**Phase 4 — FURNITURE (2 days).** The 96pt canvas lightbox rail: onion latch, back/forward depth, strength, the INPUT plate field, field/safe/peg guides in Ao. Onion re-ink on the **vector** path (two colour constants — `draw_strokes` already accepts a colour). Paper furniture. `fit view` and zoom % readout.

Sequencing law: **ink, then geometry, then furniture.** Phases 0–2 deliver most of the legibility gain and touch almost nothing structural.

### Do NOT build

- **Transport inside the X-Sheet pane.** It is a collapsible, closable dock leaf; losing it loses the instrument's motor. Transport is global state and lives in the Foot.
- **De-docking any pane** into a fixed `Panel::left/right`. `workspace.rs`'s LENS-DOCK subsystem (nine pane kinds, persisted layouts, per-stage re-lensing) is shipped architecture and stays.
- **The pen-down score dim.** Strobe in peripheral vision; unfixable without easing, and easing is banned.
- **The live pen-pressure trace polyline.** A forced per-frame repaint on the one surface where latency is the product, answering a once-per-session question. The static INPUT field carries the whole real signal for free.
- **Aka on the forward onion ghost.** A signal that is always on is not a signal, and it would collide with the Aka canvas edge inside a single gaze.
- **The raster onion recolour** (`doc.rs:776-801`). Wants a colour-matrix multiply in the wgpu blit shader; it touches the hot path validated at 0.15 ms/frame. Ship the vector path, revisit later as its own piece of work.
- **Animated transitions** on room switches, panel slides, tool changes. Immediate mode turns each into a per-frame state machine forcing continuous repaints, and a detent should arrive instantly. The only motion in this app is the playhead and ink.
- **Variable fonts** (supported, but Plex Mono has no official variable release — determinism wins), **`tnum`** (`shape()` passes an empty user-feature list; solve it with a monospace instead), **full IBM Plex Sans JP** (~7 MB/weight for ~200 used glyphs).
- **Faux-engraved bevels.** At ppp 1.25 the half-pixel rounding shimmers under scroll and it doubles paint calls per widget. The engraved feel comes from type, rules and the Well step.
- **Vertical Japanese (tategaki) and CJK line-breaking.** Irrelevant at four two-character strings.
- **Any green state, anywhere, ever.**

**Accepted risk, stated plainly.** This photographs as a devtool. There is no gradient, no card, no shadow, no friendly radius — so precision is the *only* thing carrying the aesthetic, and a 1pt drift in the layer strip will read as "unfinished" rather than "imperfect." That is the correct trade for one operator at hour six: air and roundness are paid for in saccades hundreds of times a session, and unlike taste, "is the pitch exactly 18pt, is the baseline seated, is the column snapped" is checkable against the running app.