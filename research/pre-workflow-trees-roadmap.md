# Pre-Workflow-Trees Roadmap (2026-07-19)

What must land before "separating and defining the workflow trees" — the
per-stage workspaces of the LENS-DOCK plan (research/windowing-architecture.md)
on the MERGED solo stages he chose: roughly **Story/Layout → Drawing
(genga+douga+sakkan) → Finishing (paint+composite) → Edit**.

Method: gap list drafted from the roadmap docs + memory, then a two-agent
audit (wf_677b3cb4-b26): a codebase census verified every claimed gap with
file:line evidence (9/9 confirmed missing), and a completeness critic found 5
missed gaps. LENS-DOCK status today: Phase 0 (dock shell) ✅, Phase 4 (node
graph pane on the real Cut.graph) ✅ + the graph RENDER path (beyond plan) ✅;
Phases 1, 2, 3 (panel instances, Document/ViewContext split, stage spine) not
started — the trees ARE Phases 2+3.

## Why these gaps matter

A workspace is only a real "room" if its stage's defining verb exists.
LENS-DOCK's own definition: workspace = layout + TOOL/MODE + view state.
Today the only canvas verbs are brush and eraser, so a Layout room or a
Finishing room would just be the Draw room with different panel positions.

## Tier A — structural prerequisites (the trees stand ON these)

| # | Item | Why before trees | Effort |
|---|------|------------------|--------|
| A1 | **Tool abstraction + selection/transform tool** (audit N1): `Tool` enum (Brush/Eraser/Fill/Select…), lasso+rect select, move/scale/rotate of active-layer content (readback → PaintTiles diff = undo already fits) | Layout/sakkan/douga constantly reframe and nudge; and per-stage tool restore needs a Tool to restore | M-L |
| A2 | **Panel instances — LENS-DOCK Phase 1** (audit N3): PanelId registry, multiple canvas instances w/ independent zoom/pan/view, composite as a **Viewer pane kind** (today it's a mode toggle on the ONE canvas) | Finishing wants edit canvas + composite viewer SIDE BY SIDE; Edit wants a viewer next to the X-sheet | M |
| A3 | **Document/ViewContext split — Phase 2-lite** (G7): split AppState's Engine from the cursor (scene/cut/frame/column/onion/playing); workspaces capture+restore tool, view mode, onion, preset | The re-lensing invariant; what makes stages feel distinct without forking data | L (highest churn — do behind "Main" per the design) |

## Tier B — stage-defining tools (each room's verb)

| # | Item | Stage it defines | Effort |
|---|------|------------------|--------|
| B1 | **Flood fill / paint bucket with gap closing** (G1): line-aware fill on the active layer; the industry ink&paint verb | Finishing (shiage) | M |
| B2 | **Image import — `ImageSource` node + reference underlay** (audit N2): bring a BG plate / storyboard / scanned pencil into a cut; engine NodeKind + storage, app decode+upload (tile machinery exists) | Story/Layout (draw against reference) + Finishing (cels over BG — satsuei rows 11-12) | M-L (engine schema) |
| B3 | **Drawing-stage kit** — clone-prev-cel-stack (G3, design banked v1.1), per-layer onion (G4, v1.1), alpha-lock (G5, v1.5 DstAlpha dab variant) | Drawing (douga/sakkan) | S each |
| B4 | **Multi-column raster in EDIT view** (G2): other columns' composites visible while editing (today: vector strokes only; composite view shows all but refuses painting) | Cross-stage (douga against layout column, paint against line column) | M |
| B5 | **Character color models / palette pane** (G6): named per-character normal/shadow/highlight sets, project-persisted | Finishing | M |

## Tier C — can land with or after the trees

| # | Item | Note |
|---|------|------|
| C1 | **Camera / animated params — MODEL DECISION ONLY pre-trees** (audit N4): today no graph param varies with time. Decide WHERE camera lives — recommendation: **X-sheet camera columns** (the law: the exposure sheet is the master timing artifact) driving Transform params — BEFORE the Composite room's panel set freezes. Implementation after trees. | decision S, impl L |
| C2 | **Multi-cut/scene navigation** (G8): model supports it, UI edits one cut. A cut browser can arrive WITH the Edit room. | M |
| C3 | **Export range + progress** (G9): whole-cut + blocking today. QoL, anytime. | S |
| C4 | **Audio scratch track** (audit N5): zero audio in the app. Post-trees; the Edit room design should reserve its spot (waveform strip under the X-sheet). | L |

## Recommended sequence

1. **A1 tools + select/transform** — unblocks everything conceptually; biggest single upgrade to daily drawing too.
2. **B1 flood fill** — fast, huge value, makes Finishing real.
3. **A2 panel instances → A3 ViewContext split** — the structural double-feature, in the design's own order (Phase 1 then 2), each behind behavior-preserving defaults.
4. **B2 image import** — Layout + Finishing become real rooms.
5. **B3 drawing kit + B4 multi-column display** — small items, big daily-use payoff.
6. **B5 palette** + **C1 camera decision** (one AskUserQuestion-scale choice).
7. → **DEFINE THE TREES** (Phase 3 stage spine: merged stages, default layouts, per-stage tool/preset/view restore) with C2/C3/C4 landing inside or after their rooms.

Items 1-2 are also just good standalone milestones if the structural work
needs to breathe between them.
