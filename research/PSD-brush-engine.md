# THE BRUSH ENGINE — tips, textures, engines, sensors

PSD gate 2026-08-19. Owner's order, verbatim: "I want all of the brush
Real tip stamps textures multi engine support and sensor curves."

PREMORTEM. A month on, this damaged the studio. The story: the dab
shader grew four features in one pass and the PEN changed under the
hand — the pencil box's atari/genga/shusei strokes rendered a hair
different after the "unification", and the muscle memory built over
weeks died quietly. Undo replay stopped being byte-stable because
scatter jitter seeded from the clock. VRAM crept: 268 tips + patterns
resident. And smudge — promised as "engine support" — shipped as a fake
that sampled stale canvas, so strokes smeared wrongly and the artist
stopped trusting every imported brush, including the honest ones.

ROOT: this stands on the procedural dab staying BYTE-IDENTICAL when a
preset carries no tip/texture/curves (opt-in per preset; the pencil box
never opts in), and on DETERMINISM (same stroke = same pixels: all
jitter seeded from dab index + geometry, never clock or RNG). If either
is weak, this is rubble.

NEVER-DO.
1. The pencil box and every existing preset keep the procedural dab.
   New machinery activates only from fields an imported preset actually
   carries. Old strokes, undo, replay: byte-identical.
2. Deterministic stroke path. No SystemTime, no thread RNG anywhere in
   dab building or shading. Scatter/rotation jitter = hash(dab index,
   position).
3. Resources are CACHE: tips and patterns live beside brush_thumbs on
   disk, load as textures ON ARM (the armed preset only + small LRU),
   never all-resident. The .animproj format never carries them.
4. HONEST ENGINES ONLY: an engine ships when its core behaviour is
   real. paintbrush (pixel: tips+spacing+scatter), spray (deterministic
   scatter), auto-brush shapes (generated tips) are in scope. SMUDGE
   and wet mixing are NOT in this room — they need canvas feedback
   architecture and get their own gate. An unsupported engine imports
   as the honest dab and SAYS SO in its hover, never fakes.
5. Sensor curves: pressure/speed/fade curves apply CPU-side at dab
   building (size/opacity/flow/rotation), sampled from the preset's own
   curve points. No invented defaults — absent curve = linear.

BLAST RADIUS: kpp.rs (deep parse: paintopid, auto/predefined tip,
spacing, scatter, curves, texture ref; bundle resource extraction:
brushes/*.png|gbr|gih, patterns/*), config.rs BrushPreset additive
serde-default fields, paint.rs (dab pipeline: tip mask sampling,
generated-shape params, pattern grain, per-dab rotation/spacing),
canvas.rs (dab building applies curves; arm loads resources), rail
hover honesty. anim-core UNTOUCHED.

STAGES: A parse+extract inventory-driven. B tips (predefined + auto
shapes) + spacing/rotation in the dab pipeline. C sensor curves at dab
build. D texture grain. E spray scatter. Each stage compiles, tests,
delivers before the next.
