# THE SMUDGE GATE — colorsmudge, honestly

PSD gate 2026-08-19. Owner: "lets test the smudge engine gate."
(Excluded from PSD-brush-engine by its NEVER-DO 4 until it could be
real.)

PREMORTEM. A month on, smudge damaged trust in the pen. The story: the
first build sampled the GPU wet buffer per dab — a feedback loop that
stalled the stroke path and produced different pixels on every replay,
so undo stopped being byte-stable. The rewrite sampled "the canvas"
through a readback that froze the pen for 40ms per flush. And artists
expected finger-smearing — dragging their OWN fresh stroke — got
colour-pickup instead, and called it broken because nothing said so.

ROOT: this stands on the sample source being the PRE-STROKE truth (the
committed CPU tiles, which cannot change while the wet stroke is open)
and on the mix running CPU-side at dab building — deterministic in
(canvas state at stroke start, dab index), no GPU feedback, no
readbacks. If either is weak, this is rubble.

NEVER-DO.
1. No GPU feedback and no readbacks anywhere in the stroke path. The
   sample source is an Arc snapshot of the active layer's committed
   tiles, taken once at stroke start.
2. This is DULLING smudge (pick colour up, lay it down, blend along the
   stroke). Within-stroke SELF-smearing is absent by architecture and
   the rail hover SAYS SO — never fakes it, never promises it.
3. Determinism: the held-colour chain folds over dab index during dab
   building (prefix-stable, like every dab). Same stroke = same pixels.
4. Activates only from parsed rates on colorsmudge presets; anything
   unparseable keeps painting as the plain dab, exactly as today.

BLAST RADIUS: kpp.rs (SmudgeRate/ColorRate values + gated curves),
config.rs EngineDef two serde-default fields, canvas.rs (stroke-start
tile snapshot + per-dab held-colour mix + hover honesty). paint.rs and
anim-core UNTOUCHED.

## Build log

- 2026-08-19 — DELIVERED (40 tests, 0 warnings). Parse: SmudgeRateValue
  + ColorRateValue with their gated pressure curves (targets "smudge" /
  "color_rate" riding the existing curve machinery). Canvas: an Arc
  snapshot of the active layer's committed tiles at BOTH stroke-latch
  sites (pen + mouse); a held-colour Cell folded over dab order inside
  build_stroke_dabs — pickup at smudge_rate, deposit re-inked by
  color_rate, alpha riding held coverage so blenders fade to nothing
  over empty paper. sample_tiles un-premultiplies; misses are
  transparent. paint.rs and anim-core UNTOUCHED — no GPU feedback, no
  readbacks (NEVER-DO 1). Tests: unpremultiply + miss transparency;
  the held chain deterministic and convergent. Hover: colorsmudge now
  says "picks up committed colour (no self-smear within a stroke)"
  (NEVER-DO 2). Re-import refreshes any preset whose parse improved —
  ONE MORE CLICK of the Krita import migrates all 39 smudge presets.
