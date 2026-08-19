# Editor repaint (Plate & Pencil) — the room

PSD gate passed 2026-08-17 — root: the repaint never changes what the document
contains or how the pen reaches it.

Ratified by the owner 2026-08-17, his words: "yes continue on the development
of the application" — given in direct answer to this drafted room with the
invitation to edit any word. One room covers the whole spec build; phase
boundaries are internal law.

---

## STANZA

**PURPOSE.** Make the editor legible as an instrument — the Plate & Pencil
spec (`DESIGN-SPEC-editor-ui.md`) — without changing what the app does to
artwork.

**ROOT.** This stands on the repaint never changing what the document contains
or how the pen reaches it; if that's weak, this is rubble.

**NEVER-DO.**
1. Never change document format, save/load, or undo semantics. This build
   paints and re-queries; it never rewrites state.
2. Never touch the stroke path or the wgpu hot path. The spec's "Do NOT
   build" list binds as law.
3. Never build past the active phase boundary. Each phase lands, builds
   clean, and is looked at in the running app (F12 dump) before the next
   starts.

**BLAST RADIUS.** The `app` crate's UI code only — `plate.rs`, `icons.rs`,
`runs.rs` (new), plus the panel files the spec names. `crates/anim-core`
untouched. `workspace.rs`'s dock untouched.

**STAGES.** Phases 0–2 first (plate, ink, the Rule); 3–4 after the owner has
seen 0–2 running. Valid in the editor stage, on the owner's machine.

---

## OWNER'S AMENDMENT — 2026-08-17, after the Phase-0 F12 look

His words, binding on all later phases and on the per-room design work:

- "We need to identify what exactly an artist is going to be utilizing in
  the specific 3 workflows … Each workflow should incorporate a very
  efficient way of easily either clicking or understanding that a keypress
  or a button on the UI or a color wheel or color swatch board etc."
- "Genga, Shiage, and Henshū … are really just a drafted layout … focus
  strategically on making each of them uniform, efficient for the design
  and artistic work each of them hold … each core UI element correctly
  sized and solid for a workflow."
- "If the UI is not filled in that much and a lot of spacing, we can make
  things larger to incorporate the ease-of-use design. Making sure to make
  it look professional as well — like what a similar artist would want the
  feel to feel like."
- Genga specifically: "no good icons or tool window for specifically having
  the brushes I want … we have pressure sensitivity and opacity sensitivity
  on our setup. We should make … a standard at least 3 presets for now for
  specifically that workflow."
- "The UI for the genga panel looks pretty bland and doesn't have
  differentiation for the tools, which can lead to blindness or harder to
  find what you're looking for / memorize where things are at."

Read against the ratified spec: differentiation and professional ink are
Phase 1 (proceeds now); correct sizing/larger targets are Phase 3 law with
the added rule "sparse space is spent on larger controls"; the per-room
workflow charters and the three genga presets are NEW design scope —
analysed first, implemented inside this same room's laws.

Second directive, same day: "figure out a way to get real detailed icons
for our buttons and a decent background to incorporate with them … non
copyright, our own generated" — then detailing continues. Ruling: all
icon artwork is authored in-repo as SVG (original work), rendered by our
own tool (`tools/iconforge`) into white-alpha masks tinted at draw time
(so the colour law survives — one texture, every ink); the background is
procedurally generated grain seeded by our own code, derived from
Graphite, whisper-subtle. No third-party asset packs, ever. Painted
primitives remain as the never-tofu fallback.

---

## Build log

- 2026-08-17 — gate passed, Phase 0 (the plate) begun.
- 2026-08-17 — Phase 0 landed minus fonts: `plate.rs` (8 tokens + derived
  alphas, spacing law, snap/device_px, legend/value type roles,
  DETENT/LATCH/DANGER affordances, `visuals()`), wired at the single
  Visuals site in main.rs. 0 warnings, 3 unit tests, delivered to the
  running app via dev-loop handover (PID 27600).
- 2026-08-17 — Phase 0 COMPLETE: IBM Plex installed (owner-approved
  download from github.com/IBM/plex, OFL; Sans Regular/SemiBold, Mono
  Medium/SemiBold, + JP Bold subset cut locally to 210KB — kana, CJK
  punctuation, pipeline kanji; tab glyphs 原画/仕上げ/編集/レイアウト
  verified present). Proportional=Plex Sans, Monospace=Plex Mono Medium,
  named families carry the SemiBold cuts, JP second in every family,
  egui defaults kept at the tail. Type scale set (body/button 11.5,
  small 9.5, mono 11, heading 13 SemiBold). Delivered via handover
  (PID 16360). Owner's F12 look returned the workflow amendment above.
- 2026-08-17 — Phase 1, first slice, delivered live (PID 17872): `icons.rs`
  (14 painted primitives — brush, eraser, lock, select, lasso, marquee,
  fill, comp, eye, eye-off, chevrons, fit, plus); plate gains tool_button /
  tool_latch / icon_button / swatch. Brush strip rebuilt: 4-way tool DETENT
  group with icons + armed Tally lamp, lock/comp as LATCHes, raster engine
  switch moved to Settings→Performance (defect 2), clear-cel removed from
  the strip. ERASE CEL + DELETE LAYER now DANGER holds in the Cel Layers
  footer (defects 6/7/11), reorder is painted chevrons, delete left the
  row. Swatches are painted wells: ink·ao·aka·white (defect 5). Warning
  Law applied: ⚠ MOUSE red → "input: mouse — no pressure" in Legend;
  hidden layer red → dim. X-sheet colour law: navy slab → tally_well,
  keys/names → Struck, holds → Ao, continuation handle Ao/Tally (absorbs
  the amber), headings → engraved Legend caps, greens dead in main panels.
  Layer identity chips re-inked to the pencil vocabulary (rough=Ao,
  correction=Aka; the green "color" literal is dead). 10 tests green,
  0 warnings. REMAINING Phase 1: nodegraph_panel (21 literals), config
  curve editor (10), floatwin/viewer (8), canvas leftovers (14 — mostly
  paper/selection overlays), library section styling.
- 2026-08-17 — ASSET FORGE delivered live (PID 17872 — NB Windows reused
  the PID; process StartTime proved the handover): `tools/iconforge`
  (resvg) renders our 14 in-repo SVG icon drawings → white-alpha masks
  at 24/48px, tinted at draw time so one mask serves every ink; painted
  primitives remain as never-tofu fallback. Chrome grain generated
  procedurally (256² tileable value noise, Graphite ±4 relief, seeded)
  and laid down via plate::surface() in every dock pane + top bar. All
  assets original work, owned by the repo; fonts OFL with license shipped.
- 2026-08-17 — ROOM CHARTERS landed (9-agent workflow, verified against
  the stanza + owner critique): research/ROOM-CHARTERS.md — shared
  grammar (position law, one-lamp law, material rule, XL law, one verb
  one key), full genga/shiage/henshū element inventories, and the three
  genga pencils as exact BrushPreset numbers: ATARI (ao construction,
  14px, opacity-dynamic, tilt loud), GENGA (ink line, 6px, size-only
  dynamics, tight floor), SHUSEI (aka correction, 8px). AWAITING OWNER'S
  READ before implementation (slots into Phases 2–4).
- 2026-08-17 — owner's directive after seeing the forge live: "the Brush
  toolbox has to be more differentiated and not move around — Select and
  Fill open sub-elements that make the toolbox move… the top bar can use
  a size edit as well, everything seems a little small." Ruling: spec
  defect 4 (fixed slots + always-allocated OPTIONS row) pulled forward
  from Phase 3 on the owner's word; top bar steps up one size class.
  DELIVERED same day (PID 24988): OPTIONS row always allocated at 26pt
  under the strip — select's lasso/rect and fill's gap/under/cel/layer
  moved there, row 1 membership now constant, brush/erase leave the row
  empty but claimed; top bar scoped to 13pt controls, 28pt targets,
  wider spacing, larger check glyphs. 0 warnings.
- 2026-08-17 — owner: "the Brush being a sub window above the canvas seems
  goofy… configure it to a more optimized area in the drawing area; add a
  Settings UI Features sub-menu with UI lock." DELIVERED (handover
  19:36:17): the tool deck now renders INSIDE the Canvas pane's top edge
  (Panel::top within the leaf — part of the instrument, moves with the
  canvas, survives rearrangement); Pane::Brush is now a pointer note (the
  dock recipe itself untouched, close it via its ✕). Settings gains
  UI Features → "Lock UI positions": freezes tab dragging, splits, and
  close buttons via DockArea flags; arrangement kept; persisted in
  config (UiConfig, serde-defaulted). Probe harness updated. 0 warnings.
- 2026-08-17 — owner directed genga workflow implementation ("solely
  workflow efficiency … what artists utilize in order") + Layout-tab
  preview. GENGA SLICE DELIVERED (handover verified by StartTime
  19:50:37, pending cleared): THE PENCIL BOX live — atari/genga/shusei
  as named factory presets (charter numbers verbatim; merged into
  existing configs at load, user edits respected, hotkeys 1/2/3 via the
  existing preset actions) + eraser as fourth detent; each slot 88×44
  with a specimen stroke painted FROM the preset's real numbers in a
  Well recess, engraved name, hotkey digit, Tally ring solid when armed
  and DASHED once the live brush drifts off-preset (armed_preset
  tracking in apply_preset). Size/flow/opacity + p·size/p·opac/tilt
  latches + dynamics menu moved into the OPTIONS row's brush arm (strip
  membership now truly constant). Q/W = prev/next KEY on the active
  column (new Actions, default chords, load-merged; momentary R flip
  DEFERRED — needs a render-path decision). Layout room's Canvas pane
  now lenses to the animation PREVIEW (viewer content, graph gated as a
  consumer; dock recipes untouched — content lensing per the charters).
  SKILL-FORGE: one seedling born under law — devloop-deliver (project
  level, advisory; evidence = 3-day delivery-cycle record incl. the
  PID-reuse false reading; ledgered at root). 10 tests, 0 warnings.
- 2026-08-17 — OWNER'S HAND EXAM: "the pencils feel good" — the charter
  numbers confirmed at the pen. Directives: (a) proceed to Phase 2 THE
  RULE; (b) side audit: select/fill/lock/comp/chip lack backers vs the
  pencil slots and blend into the chrome — seat them the same way;
  (c) brush editing (size, p·size, all dynamics) moves OFF the deck into
  a RIGHT-of-canvas rail, hidden until the pointer hovers the canvas's
  right edge, categorized to the armed pencil — anti-clutter law.
  BOTH DELIVERED 2026-08-17 (StartTime 20:02:51 then 20:05:48):
  (b) tool_button/tool_latch/icon_button + layer chip all seated on Well
  recesses with edges — no control floats on chrome anymore.
  (c) THE BRUSH RAIL: floats as an overlay at the canvas right edge (the
  paper never rescales), opens on edge-hover, never mid-stroke, stays
  through a slider drag, closes when left; grip ticks mark it when
  hidden; content = armed pencil's name + px/flow/opacity + pressure
  latches + min size + full tilt group, flattened (no more menu). The
  deck's OPTIONS row now carries only select/fill options + a dim hint.
- 2026-08-17 — PHASE 2, THE RULE, DELIVERED (same handover 20:05:48):
  `runs.rs` (column_marks walks keys into runs + empty-terminators;
  rule_tier for the 6/12/24 hierarchy; 4 unit tests incl. the
  islands-become-runs case). Sheet: gutter 34→44 (18pt seconds
  sub-column with "1s/2s" labels + 26pt frame numbers, current steps to
  Mono SemiBold weight-only), row tints replaced by painted boundary
  RULES (sec 2dev px / half / beat Legend alphas), vertical column
  separators, Tally gutter notch, and the per-row cell text (●/○/│
  islands) replaced by ONE mark per run painted post-loop from a
  whole-sheet painter: Tally key dot, Ao hold stroke, tail cross-tick,
  name at run head only, whole run turns Tally on hover. Continuation
  handle ABSORBED: rides the rule's own x, contributes only its grab
  circle + a live drag guide. CANVAS MIRROR: 14pt strip at the paper's
  left edge, same 18pt pitch, active column's dots/holds/rules/notch,
  follows the playhead when the cut outgrows it (current at 40%).
  14 tests, 0 warnings. Still open: momentary R flip; ~60 literals in
  nodegraph/config/floatwin/viewer; Phase 3 Slate/Foot.
- 2026-08-17 — owner's next directives, DELIVERED (StartTime 20:14:07):
  (a) THE RAIL SLIDES: rebuilt as a true toolbar — full-height, the
  deck's own grain material with one seam on its leading edge, controls
  centred on a single axis (the slider column is the spine), 160ms
  animated slide in/out via animate_bool (an owner-ordered exception to
  the no-animation law, recorded here), still an overlay (paper never
  rescales), still stroke-guarded. (b) BRUSH WINDOW RETIRED: any copy in
  a saved arrangement is removed on sight (find_tab/remove_tab in
  main.rs — dock recipes in workspace.rs untouched) and it left the
  panes menu. (c) EXACT UI DESCRIPTIONS shipped: Settings → UI Features
  toggle; plate serves every control's hover with kind+label+source
  ([DETENT tool_button 'select' · plate.rs]); pencil slots name
  themselves and their preset home; the x-sheet gains a pointer
  INSPECTOR naming the element under the cursor (seconds gutter, frame
  number, key dot/run head, hold rule with run span, empty terminator,
  param/audio zones) — so the owner can direct edits by true names.
  14 tests, 0 warnings.
- 2026-08-17 — owner: rail should be TRANSLUCENT (not cover the paper)
  and its options truly centred on the pullout's centre (muscle memory:
  the hand lands mid-tab). DELIVERED (StartTime 20:18:22): grain ground
  now surface_alpha(200) — one number tunes it; root cause of the
  off-centre feel found and fixed — egui sliders hang label+value text
  right of the track, so the TRACK centre sat left of rail centre. Now
  every slider is a bare track (show_value false) with an engraved
  label+value line ABOVE it (Legend caps + Struck mono, live), so track
  centre = rail centre = hover reflex point. 14 tests, 0 warnings.
- 2026-08-17 — owner: "looks worse" — wanted VERTICAL centring (the
  stack clustered on the grip's line), the slide was clipping outside
  bounds, and the opacitised backdrop should go entirely. DELIVERED
  (StartTime 20:20:55): backdrop removed — the controls float bare;
  Area clipped to the canvas rect (the slide now emerges from the edge
  and can never spill over neighbouring panes); the stack centres
  vertically via last-frame content measurement (settles in one frame).
  Horizontal track-centring from the previous fix retained.
- 2026-08-17 — owner's F12 screenshot (frame_004 — first real drawing
  with all three pencils on it) caught the rail STILL top-anchored +
  unreadable text. Root cause: set_min_size inflated min_rect, so the
  content measurement returned the leftover column height and the
  centring pad decayed to zero — a feedback loop. Repair under this
  room (PSD: repair-under-existing-stanza exemption, cited): measure by
  allocation cursor before/after the stack (immune to inflation), and a
  CONTENT-SIZED backing restored — near-opaque Well pill behind just
  the stack, Legend edge, paper stays open above and below. DELIVERED
  StartTime 20:24:35.
  Owner confirmed centred.
- 2026-08-17 — PHASE 3, GEOMETRY, first delivery (StartTime 21:03:35):
  THE SLATE — wordmark deleted; file commands in ONE ≡ detent (New/
  Open/Save/Export▸/Import WAV/Settings, ellipsis convention, WAV moved
  in from the sheet); slate identity fields (filename + ● dirty mark,
  cut · length · fps(.3) · resolution as engraved mono); stage spine =
  four two-line slate tabs, kanji over romaji caps (レイアウト/原画/
  仕上げ/編集 via the JP subset), Well-framed, Tally lamp on the active
  room; row 2 = THE ARMING LINE (drawing · column · layer · pencil,
  live from canvas.arming_pencil()). THE FOOT — painted transport
  (skip/step/play-pause icons as primitives), the frame number is
  finally TYPEABLE (DragValue, gesture-guarded), loop latch, fps
  readout, CHATTER lane with 4s decay (status_seen/status_since on
  Editor — no doc.rs API change), PERSISTENT lane = UNDO·REDO tape
  lamps + pen diag. X-SHEET EVICTION — title row deleted, three
  buttons → one "key ▾" menu with LIFT KEY as an in-menu DANGER hold,
  audio row gone (lives in ≡), library = collapsed drawer. CEL LAYERS
  defect 10 — name in a FIXED elided cell; chevrons/opacity/pct at
  constant x forever. Transport/undo/redo/loop/res all left the top
  bar. 14 tests, 0 warnings. Phase 3 remainder: REFUSAL lane wiring,
  defect 16 layer-by-name + colour-swap deletion (DEFERRED: touches
  stroke-target semantics — wants its own careful pass), fill/select
  OPTIONS polish.
- 2026-08-17 — owner's frame_005 look: "some top heavy elements" +
  egui's debug red flagged the Foot's right lane clipping at the screen
  edge. FIXED (StartTime 21:08:12): tofu purge round 2 — ≡ became
  "file", dirty ● → •, all ▾/▸/⇥/⇤ glyphs became words (our fonts
  never carried them; words never tofu); fps overlay ducked under the
  taller Slate (y 34→96); Slate paddings trimmed one notch (interact 24,
  padding 8×4, tabs 84×34 — type sizes kept); Foot tape counter rebuilt
  as ONE two-tone LayoutJob (REDO · UNDO) seated 8pt off the screen
  edge — no more truncation. frame_005 also carries the first real
  multi-pencil drawing over a 20-frame hold with the run mark + mirror
  strip visibly working.
- 2026-08-17 — PHASE 4, FURNITURE, DELIVERED (StartTime 21:12:20): THE
  LIGHTBOX RAIL — 96pt Panel::left inside the Canvas leaf (dies only
  with what it serves): onion latch + "line only" + ghost-strength
  slider (wired: scales BOTH the raster tint alphas and the vector
  ghost alphas — paint-time only, hot path untouched); paper furniture
  latches (field 90% / safe 80% + centre cross / registration peg bar)
  drawn in Ao under the drawing; fit view + live zoom % moved here from
  the deck; THE INPUT PLATE FIELD completes the Warning Law — Legend
  label, Struck value ("mouse" / "pen · ink"), never red, click opens
  Settings→Pen (canvas → Editor → App plumbing). Onion/line-only left
  the Slate. VECTOR GHOST RE-INK: onion_ghosts' (215,70,70) red and
  (70,175,90) GREEN — the kill-list's last two literals in the canvas —
  are dead; behind = Ao, ahead = Legend, depth alphas 100/55 × strength.
  Paper fill/edge now plate::PAPER + legend_dim. egui 0.35 note: the
  unified Panel sizes via exact_size(), not exact_width. 14 tests,
  0 warnings. ALL FIVE SPEC PHASES NOW HAVE THEIR CORE LIVE. Remaining
  room-scope: Phase-1 tail literals (nodegraph/config/floatwin/viewer),
  REFUSAL lane wiring, defect 16 (own pass), R-flip, shiage/henshū
  charter deltas.
- 2026-08-17 — owner's canvas law ratified: "if you're gonna make
  anything in the canvas window besides the top UI bar, make it fold in
  like the right one." DELIVERED (StartTime 21:18:10): the fixed
  lightbox Panel::left is dead; the lightbox now FOLDS from the left
  edge with the right rail's exact mechanics (hover zone, 160ms slide,
  clip to paper, vertical centring, content-sized backing, left grip
  clear of the mirror strip). Onion investigation: two causes named —
  (1) the strength slider had NO value readout (a silent dial reads as
  a bug when dragged low) → STRENGTH value line added, matching the
  right rail's grammar; (2) semantics: ghosts are of the neighbouring
  DRAWING, not frame — inside a hold of one drawing there is no
  neighbour to ghost (hover text now says so). If ghosts still fail
  with two distinct drawings + strength high, that's a real layering
  bug — awaiting owner's retest.
- 2026-08-17 — owner's library + onion report (frame_006 dump, lightbox
  fold-out held open). DELIVERED (StartTime below): (1) DRAWING NAMES
  NEVER REPEAT — next_drawing_name now bumps until unused library-wide
  (the old count-distinct-on-column scheme repeated D1A whenever keys
  were lifted or columns switched — his library had 11 entries, 3
  named D1A); (2) LIBRARY REMOVAL exists — right-click a drawing →
  held DANGER "REMOVE DRAWING": lifts every key exposing it + removes
  from library in ONE undoable engine apply built from EXISTING
  commands (SetCell None + RemoveDrawing — no new semantics); (3) the
  Foot's red corner boxes were egui's DEBUG-BUILD unaligned-text
  markers — red that is not Aka violates the colour law →
  style.debug.show_unaligned = false. frame_006 also CONFIRMED the
  silent-dial diagnosis: STRENGTH sat at 40%. Owner's feature ask
  (onion-optimized ghost layering that does not populate the sheet)
  = NEW SCOPE — room drafted and presented for ratification, not built.

## GHOST PIN ROOM — ratified 2026-08-17 ("ok continue")

ROOT: the ghost is pure display — never exported, never saved as
exposure, never in the sheet. NEVER-DO: (1) never store the pin in the
document or let it export; (2) never let a pinned ghost receive ink;
(3) the pin dies with the session unless persistence is later ratified.
BLAST RADIUS: view-state (ViewContext) + canvas paint + lightbox rail +
library menu. No anim-core changes.

- 2026-08-17 — GHOST PIN DELIVERED (StartTime below): right-click a
  library drawing → "pin as ghost". Raster path: the pin rides the
  FORWARD onion slot (outranking the forward neighbour, disclosed in
  hover) and re-tints that slot Ao; vector path: strokes drawn in Ao at
  onion strength; works with onion on OR off; removal of a pinned
  drawing clears the pin; the lightbox rail shows GHOST <name> + unpin.
  Same delivery: hold-handle helper tag (hover names drag + right-click)
  and RIGHT-CLICK RESET on the continuation handle — clears the
  terminator (hold runs to the next key again) and re-stows the handle
  (positioned_holds removal). 14 tests, 0 warnings.
- 2026-08-17 — owner: ghost caps at ~75% at full strength; the hold
  handle's stow/pulled states need the tape-measure read. DELIVERED
  (StartTime 21:37:00): STRENGTH now spans the full range — the ghost
  alphas' old hard-coded ~43% ceiling (110/255) rescaled so 100% = fully
  opaque (raster 255·s, vector 255·s/140·s depth falloff, pin 255·s);
  default strength 0.45 keeps the previous look until the dial moves.
  TAPE-MEASURE LAW on the sheet: a hold running to the sheet's END draws
  OPEN (no tail tick) = stowed; only a pulled-out (terminated) hold gets
  the cross-tick, which is also its drag target; the handle's hover now
  says drag-out/stow in the owner's own metaphor. 14 tests, 0 warnings.
- 2026-08-17 — owner: "still not retracting like a tape on right click."
  Diagnosis: the DATA stowed correctly (terminator lifted), but THE RULE
  drew the open hold as a full line to the sheet floor — the opposite
  of retraction, visually. REFINED (StartTime 21:41-ish): an open hold
  (running to the sheet's end) now draws a SHORT STUB fading over ~5
  rows — tape in the reel; only a pulled-out hold draws its full length
  + drag tick. Strength confirmed in working order by the owner.
- 2026-08-17 — owner: the stub needs a head. DELIVERED (StartTime
  21:40:34): the stowed hold's stub now ENDS IN A DOWNWARD ARROWHEAD
  (convex triangle, rule ink at 55%) — it reads as "continues", never
  as a hanging line; fade softened (max 60%) so the stub stays legible
  into the head.
- 2026-08-17 — owner: close frames still collided with the stub/arrow →
  "maybe fully retract to the drag area and signal it continues".
  DELIVERED (StartTime 21:43-ish): stowed holds are now FULLY
  RETRACTED — the key dot wears a small downward arrowhead directly
  beneath it ("continues until pulled"), nothing extends into other
  rows; pulled holds keep full line + drag tick. Collision class
  eliminated structurally.
- 2026-08-17 — "move on": REFUSAL LANE WIRED + PHASE 1 CLOSED
  (StartTime 21:48:17). Refusals: AppState.refuse() channel (seq bumps
  so repeats re-flash) — sources wired: composite-view stroke attempt,
  hidden-layer stroke attempt, the central gesture gate ("refused —
  finish the stroke first"); the Foot's Aka lane dwells 4s; a fresh
  refusal FLASHES THE CANVAS EDGE in Aka for 0.9s (seen from the pen
  tip, per the Foot law). LITERAL TAIL SWEPT: nodegraph 21 (node kinds
  to token identities — drawings=Ao, output/assets=Struck, rest=Legend
  steps; cables=Ao; invalid pins=Aka; missing-image warning=Aka; bg/
  body=Well/Graphite; CAVEAT logged: node selection currently Ao, may
  want Tally in a polish pass), config curve editor 10 (Well ground,
  beat-rule grid, Ao curve, Struck endpoints; capture-armed=tally_well,
  clash=WELL+Aka EDGE — never a red fill), floatwin 4, viewer 4.
  Phase 1's kill list is now EMPTY outside the canvas's functional
  overlays (brush cursor rings etc., exempt as instruments). 14 tests,
  0 warnings.
- 2026-08-17 — owner's NEXT directive recorded: COLOUR gets a BOTTOM
  canvas fold-out ("the paint dish") — uniform palette pot area, a
  SPLOTCH/mixing area, an accurate colour element for artists, one
  designed uncluttered colour workflow. Completes the four edges: top
  deck fixed, right brush, left lightbox, bottom colour. To be designed
  against the shiage charter's pot rules (material rule, armed-pot
  Tally ring) before building.

## THE PAINT DISH — bottom fold-out design (drafted 2026-08-17, awaiting go)

Owner's brief: colour lives in a bottom-canvas pop-out — uniform palette
area, a splotch area, an accurate colour element, uncluttered.

MECHANICS (the fold law, inherited whole): grip ticks at the canvas's
bottom-centre; hover the bottom edge → slides UP 160ms; clipped to the
paper; cannot open mid-stroke; stays while dragging; content-sized
backing; contents CENTRED on the grip's vertical line (the rails' law,
rotated 90°).

FOUR ZONES, one row, ~150pt tall, left→right:
1. THE ARMED WELL — the current brush colour as one large seated well
   (44×44), hex beneath in Struck mono, matching pot's name in Legend.
2. THE POTS — the character palette's roles as a uniform pot grid
   (36×26 wells, names under, from palette.rs — the SAME colours shiage
   fill fetches by name; no second truth). Armed pot = Tally ring.
   Click arms; double-click loads the pot into THE EYE for editing;
   "set" writes back to the role (project-persisted). Character detent
   when >1.
3. THE DISH (the signature) — a Well mixing tray ~200×110. Click empty:
   lay a splotch of the current colour. Click a splotch: pick it up.
   LAY OVER a splotch: the new splotch MIXES 50/50 with what's beneath
   — real intermediate tones, like a physical dish. Right-click removes
   a splotch; "rinse" clears. Session-lived (ghost-pin law) unless
   persistence is later ratified.
4. THE EYE — the accurate element: the full SV-square + hue picker,
   inline and always open (never a popup), hex line beneath; edits arm
   the brush live.

COLOUR LAW: paint is MATERIAL — pots/splotches/armed well seated in
Well recesses, never encoding UI state; armed = Tally ring only; labels
Legend caps; values Struck mono. The chrome stays in the eight tokens;
the paint inside the wells may be any colour (material exemption).

NOT BUILT (named): canvas eyedropper (wants canvas sampling — its own
piece), swatch import, splotch persistence.

REFINED per owner (2026-08-17): "think of it as a colour-picking
workflow for PAINTING — the pencils carry drawing-stage colour up top;
this is fundamentally the colouring/rendering stage." Changes:
- THE POTS becomes THE MODEL: not a flat grid but the colour-model
  TABLE anime pipelines actually use — characters as ROWS (Legend
  names), roles as COLUMNS (NORMAL | SHADOW | HILITE, Legend caps),
  pots at intersections. The painter's most frequent move — fill flats
  in normal, then hop to that SAME character's shadow — becomes one
  adjacent-pot step, always in the same direction. Armed pot = Tally
  ring; click arms (and arms FILL's colour in shiage).
- THE ARMED WELL gains ROW CONTEXT: beneath the big current-colour
  well, small chips of the same character's other roles — the model
  around your colour at a glance, clickable.
- Workflow accelerator (offered): DERIVE — with a normal pot armed,
  one action derives/updates its shadow role by the standard transform
  (hue toward cool, value down) into an editable result; the pipeline's
  systematic shadow-derivation as one detent. Optional; owner decides.
- Home room: the dish serves everywhere (genga can tweak pencil inks)
  but its grammar is shiage's; it is that room's charter pot-board,
  materialised as the bottom fold-out.
DISH + EYE unchanged. Build order when ratified: dish shell + MODEL
table → armed well/context → EYE → splotch mixing → (derive, if taken).

PSD gate passed 2026-08-17 — PAINT DISH room (owner: "go ahead with
derive included"). PREMORTEM: the dish damaged the studio by silently
rewriting colour models — DERIVE overwrote a hand-tuned shadow, and
on-model colours drifted mid-production (palettes are NOT undo-tracked).
ROOT: the colour MODEL is the single deliberate truth — the dish picks
from it freely but writes to it only by explicit act; if that's weak,
rubble. NEVER-DO: (1) arming/mixing/splotching never write the model —
only the Eye's explicit "set" and DERIVE's HELD action write, both
DANGER-guarded (no undo exists there); (2) splotches are session
material, never saved — no second colour truth; (3) fold law inherited
whole (overlay, never mid-stroke, paper never rescales). BLAST RADIUS:
canvas.rs fold-out + palette.rs pure helpers; palette data via the
existing Palettes API; no anim-core.
- 2026-08-17 — THE PAINT DISH DELIVERED under its room (StartTime
  below): bottom fold-out, fold law whole (grip at bottom-centre, 160ms,
  clipped, content centred on the grip's vertical line). ARMED WELL
  (46px current colour + hex + matching role name + row-context chips) ·
  THE MODEL (characters × roles colour-model table from state.palettes —
  the single truth shiage fetches by name; click arms, Tally ring;
  double-click → the Eye; per-row DERIVE as a held DANGER: shadow from
  normal via palette::derive_shadow — hue 15% toward 240°, sat +15%,
  value ×0.72; refuses via the Aka lane when no normal exists) ·
  THE DISH (180×100 Well tray; click empty = lay, dead-centre = pick,
  overlap-lay = 50/50 MIX which also arms the mix; right-click removes;
  rinse clears; session-only per NEVER-DO 2) · THE EYE (custom compact
  SV mesh square + hue strip, always open, sticky hue through greys;
  edits arm live; "set" writes the targeted role as a held DANGER).
  palette.rs gained pure rgb/hsv/derive_shadow with 2 tests (16 total).
  0 warnings.
- 2026-08-17 — DEFECT 16 DELIVERED, the deferred careful pass
  (StartTime below): the active layer is tracked BY NAME
  (ViewContext.active_layer: String; the silent slot-clamp
  active_layer_slot.min(n-1) is DEAD). Strokes resolve name→slot at
  latch time on BOTH input paths (pen stroke_start + mouse fallback);
  an unresolvable name = UNARMED: the pen REFUSES through the Aka lane
  + canvas edge flash ("no 'X' layer on this cel") instead of
  misrouting ink — the one failure mode that could put strokes on the
  wrong layer is now structurally impossible. cycle_layer cycles names;
  adding a layer arms its name; strip selects/arms by name; DELETE
  LAYER resolves by name and refuses when absent. The SILENT PER-LAYER
  COLOUR SWAP IS DEAD (canvas layer_colors/last_layer_name removed):
  the brush colour never changes without a gesture — the pencil box
  and the paint dish are the arming gestures. Template law kept: blank
  frames resolve line/color; other names refuse until a cel exists.
  16 tests, 0 warnings.
- 2026-08-17 — R-FLIP DELIVERED (StartTime below) — the queue's last
  render item, resolved WITHOUT touching the engine or stroke path: the
  flip rides the onion slot machinery. Hold R (polled, momentary;
  guarded against typing focus, playback, composite view, and live
  strokes) → slot 0 uploads the previous drawing's FULL composite (no
  line-only filter) and the sandwich draws ONLY that, untinted, while
  pin/ghosts/current step aside; vector path swaps the shown drawing
  the same way; release R → everything returns. Legend overlay names
  the state while held. No previous drawing = flip shows nothing new
  (honest no-op). Not an Action (fires on key-down state, not press) —
  noted as not rebindable v1. 16 tests, 0 warnings.
- 2026-08-17 — node-selection Tally polish (StartTime below): the
  selected node's ring is TALLY (Ao is never a selection — the law),
  and the output node's identity edge corrected Aka → Struck (the
  result is not a correction). Pin hovers stay Ao deliberately — a
  hovered pin previews making an Ao cable (continuity, lawful). THE
  QUEUE IS EMPTY: every item from the ratified spec, the charters'
  genga workflow, and every owner amendment of 2026-08-17 is live.
- 2026-08-17 — SHIAGE CHARTER BUILT (app closed; live on next launch;
  16 tests, 0 warnings). Much substrate was already live (OPTIONS row,
  fixed slots, Q/W, layers grid, eviction, dish); the deltas delivered:
  THE SWATCH BOARD — palette_panel rebuilt whole: character DETENT tabs
  (one board at a time), 44×30 seated pots (armed = Tally ring, hover
  names the pot + its fill-key), names Struck, and the EDIT MODEL latch
  gating every editing control (wheel/rename/remove/add; removals are
  held DANGERs; + character/REMOVE CHARACTER inside; keyless by
  design). THE QUEUE — 44×40 prev/next-CEL chevrons (ChevL/ChevR icons)
  on the lightbox rail's top block wired to goto_adjacent_key (moved
  into doc.rs; Q/W and the Foot arms share it; stroke-guarded). MISS
  CHECK — Action::MissCheck on M + rail latch with the HOLE icon: the
  composite ground flips Paper→Graphite (pure UI paint) so unpainted
  pixels read as pits; health is a meter, never a lamp. LINE GUARD —
  ViewContext.line_guard: arming-only protection for the line layer
  (cycle skips it; row-tap fires the defect-16 refusal; SHACKLE icon on
  the guarded row; latch in the Cel Layers footer; defaults ON entering
  Finishing, OFF elsewhere — set at the stage switch). THE POT LENS —
  the grammar's ratified key exception: with FILL armed, 1–8 arm the
  active character's pots (status echoes the pot; Slate arming line now
  appends the armed pot's name with fill); with the brush, presets as
  everywhere. Fill's gap/under became engraved-label 64pt fields.
  Deferred per charter item 13, named: alt-click role-pick and the
  variant-grid palette model (each needs its own gate).
- 2026-08-17 — HENSHU CHARTER BUILT (app closed; live on next launch;
  16 tests, 0 warnings). Its heavy dependencies had already landed
  (Foot, Q/W, R-flip, LIFT KEY danger, node-graph ink, param Struck) —
  the deltas delivered: THE HENSHU HEAD (Edit room only, via the stage
  lens through EditorTabs — dock recipes untouched): one fixed 26pt row
  with 44×26 CUT ◀/▶ chevrons (gesture-guarded, wired to the new
  step_cut view move), the cut's identity as a Struck mono detent
  opening the cut list (jump-only — C2's browser seed, not built past
  that), STANDING LIFT KEY at DANGER width (the temporal guard makes
  the big target safe), and the HOLD readout ("6f · on 6s" — am I on
  twos without counting rows). NEW ACTIONS: PrevCut/NextCut on
  PageUp/PageDown (added to the gesture gate — a cut switch must never
  orphan a stroke commit) and ToggleLoop on P; ClearFrameKey's display
  string honestied to "Lift key (hold extends)"; the key menu's stale
  (N) hover corrected to (E). GUTTER SCRUB LANE: pen-drag in the 44pt
  seconds gutter scrubs the playhead (same view write as row-click;
  drag falls through rows' click-only sense). Param columns finished:
  keyed cells wear a painted Tally DOT (the ◆ text glyph is gone —
  Tally is never text), values Mono 11. Retiming hit zone: the hold
  handle's grab grew 16→22 (paint unchanged). Charter delta 10 was
  already satisfied structurally (the deck lives inside the Canvas
  leaf, so every room carries the tool strip); delta 11 (audio ♪
  column-head detent) noted as satisfied via the Slate ≡ detent, the
  column-head variant left unbuilt.
  ALL THREE ROOM CHARTERS ARE NOW BUILT. The ratified spec, the
  charters, and every 2026-08-17 owner amendment are live in full.
