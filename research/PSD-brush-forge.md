# THE BRUSH FORGE — making brushes, not just importing them

PSD gate 2026-08-19. Owner's order: "Then after that Ill queue a new
project for Creating brushes." / "go ahead with the brush creation
project."

PREMORTEM. A month on, the forge damaged the library. The story: it
grew its own preset format "for flexibility", and forge brushes started
painting differently from imported ones as the two truths drifted. Its
live preview lied — CPU preview said soft, GPU stroke said hard — so
every brush was designed twice. A slider edited the ARMED brush
mid-stroke and dabs changed width halfway through a line. And saving
over a name silently destroyed a brush the artist had tuned for a week.

ROOT: this stands on ONE SCHEMA — the forge edits the same
BrushPreset/EngineDef every import produces, through the same
rasterizer and curve math the stroke uses — and on the DRAFT being
separate from the armed brush until an explicit apply. If either is
weak, this is rubble.

NEVER-DO.
1. One schema, one math: no forge-only fields, no second tip
   rasterizer, no private curve evaluator. The preview calls the SAME
   functions the stroke path calls (made pub(crate), not copied).
2. The forge edits a DRAFT. The armed brush changes only on "arm" —
   the same apply_preset door the rail uses. Never mid-stroke.
3. Saving over an existing name is a held DANGER that names the brush
   it replaces. Forge brushes carry bank "my brushes"; the unreal purge
   can never classify them (engine tag "forge" is not, and never joins,
   the utility set).
4. The preview approximates the GPU stroke at preview scale and SAYS SO
   in its hover; it never becomes a reason to fork the math.

BLAST RADIUS: new app/src/forge.rs (the pane: draft, sections, curve
editor, CPU specimen preview), the existing Presets pane becomes the
forge's home (tab name unchanged — muscle memory), canvas.rs visibility
only (hash01/curve_eval/rasterize_auto_tip → pub(crate)), main.rs pane
arm plumbing. anim-core untouched; import paths untouched.

STAGES: G1 forge state + sections + save/arm. G2 curve editor. G3 CPU
specimen preview. Ship together; each compiles.

## Build log

- 2026-08-19 — G1+G2+G3 DELIVERED (38 tests, 0 warnings). forge.rs in
  the Presets pane behind a BRUSH FORGE latch (the quick list below it
  never moves). Foundation: name / from-armed / blank. TIP: shaped
  (circle|rect detents, soft latch, ratio/fade/spikes rails — the
  stroke path's own rasterize_auto_tip) or STAMP from any png/gbr/gih
  (cached like every import). MOTION: size, spacing, rotation jitter,
  scatter. PRESSURE: latch-per-target curve rows (→size, →opacity)
  with a draggable 3-point Well editor whose polyline is drawn by
  canvas::curve_eval — the same evaluator the pen uses; flow/opacity
  rails. GRAIN: none / pick from the grain cache + strength/scale.
  DOORS: "arm draft" through apply_preset (the rail's own door);
  "save to library" for new names; OVERWRITE is a held DANGER when the
  name exists. SPECIMEN: a CPU-composited S-stroke with pressure
  ramping left→right, spaced/curved/jittered by the shared math
  (hash01/curve_eval), Graphite ink on Paper — deterministic, tested.
  Tests pin: a forged brush is a real EngineDef preset in bank
  "my brushes" that the unreal purge can never classify; the specimen
  is deterministic and non-empty. Shared math went pub(crate) — never
  copied (NEVER-DO 1).
- 2026-08-19 — OWNER AMENDMENT, verbatim: "make the Brush Forge a bar
  where theres Cel layers and Presets that way its distinct Or a tab
  rather." The forge is now its OWN DOCK TAB ("Brush Forge") beside Cel
  Layers and Presets — in Pane::ALL (panes menu), in the default
  layout's bottom-left stack, scrollable, no latch. The Presets pane
  returns to exactly its pre-forge shape. Saved workspace layouts are
  untouched (the new tab joins via the default layout or the panes
  menu). 38 tests, 0 warnings.
