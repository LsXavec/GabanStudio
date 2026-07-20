# The Workflow Trees — stage spine definition (2026-07-19)

LENS-DOCK Phase 3, on the merged solo stages chosen for this pipeline.
Everything the pre-workflow-trees roadmap listed as a prerequisite (A1-A3,
B1-B5, C2-C3) shipped first; this document records the two decisions that
define the trees themselves.

## C1 decision — where camera lives (RATIFIED 2026-07-19)

**Camera moves (and every future animated parameter) live on the X-sheet as
parameter columns.** Alongside drawing columns (frame → which cel, hold
semantics), the sheet will grow a numeric column kind (frame → value, hold or
eased interpolation). The node graph's `Transform` params (translate / scale /
rotate) become *bindable* to such columns; a camera move is a Transform at the
top of the graph driven by camera columns.

Why this over the alternatives:
- **Keyframes inside graph nodes** (After Effects style) splits timing across
  two artifacts and breaks the founding law — the exposure sheet is the master
  timing artifact.
- **A single Camera object per cut** can't express multi-plane moves (BG
  panning slower than the cel layer) and would be outgrown immediately.
- Parameter columns generalize for free: opacity fades, blur ramps, any
  animated node param later — same machinery, same sheet, and the Edit room's
  X-sheet shows the WHOLE cut's timing in one place.

Implementation is post-trees (engine: column kind + interpolation + Transform
binding; app: numeric column UI on the sheet). Decided now so the Edit and
Finishing rooms' panel sets don't freeze around the wrong answer.

## The spine — four fixed rooms

A fixed ordered stage spine (DaVinci Resolve pages model): **Layout → Drawing
→ Finishing → Edit**, always present in the top bar, compiled-in defaults.
Users can't add/remove/rename stages, but freely rearrange within one and
Save-as into the ordinary workspace list (which lives in the "ws ▾" menu and
is now purely user-owned — the old seeded "draw"/"timing" defaults are
superseded by the spine).

**The re-lensing invariant is absolute:** switching stage swaps layout +
tool/view state only. It never copies, forks, or converts document data, and
it obeys the same all-or-nothing gesture guard as every other context switch
(a live pen stroke refuses the tool/view swap).

| Stage | Pipeline stages merged | Default layout | Tool/view restored on entry |
|---|---|---|---|
| **layout** | storyboard + layout | Canvas; Brush bar; left rail X-sheet with Node Graph tabbed behind it (reference images are ImageSource nodes), Cel Layers below | Brush, onion off |
| **drawing** | genga + sakkan + douga | Canvas; Brush bar; left rail X-sheet, Cel Layers with Presets tabbed below | Brush, **onion on** |
| **finishing** | paint (shiage) + composite | Canvas and **live composite Viewer side by side**; Palette under the viewer; narrow X-sheet rail; Brush bar | **Fill tool, reference-cel fill on** |
| **edit** | timing + review | **X-sheet takes the left half**; Viewer (Canvas tabbed behind) over the Node Graph on the right | Select tool, onion off |

Startup room: **drawing** (the daily driver). The stage buttons highlight the
current room; applying a custom saved workspace clears the highlight
(off-spine). Rearranging panes does NOT clear it — the highlight means "the
room I'm working from", not "pixel-identical to the default".

Per-stage brush presets: built-in stages don't bind a preset (preset names
are user-defined); a user who wants "finishing always re-arms my fill-check
brush" saves a custom workspace with a bound preset — that flow already
exists.

## What the Edit room is reserving space for

- **Camera parameter columns** (C1 above) — they appear on the Edit room's
  X-sheet when implemented.
- **Audio scratch track** (roadmap C4) — waveform strip under the X-sheet.
- **Cut browser** — multi-cut navigation shipped as the "cut ▾" menu (C2);
  a richer per-cut thumbnail strip can join the Edit room later.
