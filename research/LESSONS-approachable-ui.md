# Lessons: Deckle

To live at `research/LESSONS-approachable-ui.md`. The engine, the card and the gates are in `research/PSD-approachable-ui-feedback.md` ("the feedback spec"), section 5. This file is the content. Nothing here has been run, and no person has done a lesson. All times are estimates.

## The nine

| | Rookie: the strip | Amateur: the coat | Experienced: the moth |
|---|---|---|---|
| **A drawing** | Up Late. PNG, about 9 min | A Coat of Shavings. PNG, about 20 min | Plate One. Transparent PNG, about 45 min |
| **An animation** | End of the Desk. GIF, about 12 min | Still in the Coat. Looping GIF, about 20 min | Desk to Lamp. PNG sequence and GIF, about 60 min |
| **A comic** | Not a Pencil. One page PNG, about 15 min | Too Much Coat. Two page PNGs, about 25 min | The Only Word. One mono page, about 60 min |

Read left to right, one life: a strip gets away, wraps itself in shavings, comes out as something the learner designs. Read top to bottom, three crafts: a picture, a motion, a page turn.

**The world.** Deckle is a strip of animation paper, torn off the punched edge of a sheet. It has one soft torn side, one cut side and three peg holes, the front one its eye. No mouth, arms or legs. It never speaks. Only Deckle is alive. It appears on the paper and on the lesson card, never in the chrome. It is never green, never sings, never measures anything, meets no birds and never eats. The word "inchworm" stays out of titles. The moth appears only at Experienced, as the learner's own design. The comics are silent until `tink`, the one word in the course.

**Hover cards** may carry the Japanese behind the idea: an inchworm is *shakutori-mushi*, the shaku taker. A Japanese reader checks these lines before release.

## How to read a task table

Each table is grouped by step, in bar order. The first task of every later step presses that step's pill.

| Column | Meaning |
|---|---|
| On screen | The exact sentence. `{Word}` is a token resolved at run time (end of this file). *Why* is the step's one line, shown under the sentence on arrival |
| Ring | `Task.ring`, ancestor first. A bare dotted name is `Named`. `Paper` puts no ring anywhere: the guide is the pointer |
| Tick | `Task.done`, read from state, never from a click |
| Show me | `Task.show`. Rookie: it does the task. Amateur: it does the controls, and `Where` on their art. Experienced: `Where` only, and the button reads `Show me where` |

`Trace(a)` reveals path `a` on the paper, then lands it through `commit_path`. `Perform(A)` runs Action `A` once. `Run(f)` calls a named function in `lessons.rs`. `Fill(S)` calls `fill_at` at seed `S`. `Where` rings the control and draws nothing. Every pill's Show me is `Perform` of that step's Action, listed once here and left out of the tables as `Press`.

**Stand-ins.** Amateur and Experienced art is the person's own. `Task.stand_in` names what the gate does in its place: strokes, or fill seeds, kept in `art`, kind `StandIn` or `Seed`, never painted on the paper. They are also what the finish picture on the card is built from.

**Pocket tier.** Where a ring says `Rail(Size)` or `Rail(Layers)`, the table's chain starts with `Rail(More)`, because those keys stand behind `More` at the Pocket tier. The ring stands on the last target with a rect, so nothing else changes. It is written out once, in Up Late task 3, and implied after that.

**Menus.** A ring on a menu row lists its menu word first, as main spec 6.2 places it at the end of the Menus part. The ring stands on the word until the menu opens.

---

## 1 Up Late

| | |
|---|---|
| Id, kind, class | `up_late`, Drawing, Rookie |
| Paper | Const `UP_LATE`: 1200×1200, 1 frame, track `A`, stack `Solo merged` |
| Prep | One grey smudge on the line layer, then `clear_history` |
| `fill_gap`, `ink` | 8, none (default black) |
| Card picture | The finish, small |
| Closing | `You made that. It is a real file. You can draw, undo, erase, fill a whole sky and add light.` |
| Next | A Coat of Shavings |

| # | On screen | Ring | Tick | Show me |
|---|---|---|---|---|
| | **Draw** | | | |
| 1 | This is Deckle, a strip of animation paper that tore off and got away. Draw over the blue line. *Why: Draw is where the picture starts.* | `Rail(Brush)` | `Ink(back)` | `Trace(back)` |
| 2 | Now the line underneath. Join it to the first line at both ends. | `Paper` | `Ink(belly)` | `Trace(belly)` |
| 3 | Drag {Size} to make your brush a little bigger. | `Rail(More)`, `Rail(Size)` | `BrushChanged` | `Run(size_up)` |
| 4 | Press once on each blue dot. These holes held the paper still. The first one is looking at you. | `Paper` | `Ink(hole_1, hole_2, hole_3)` | `Trace(hole_1, hole_2, hole_3)` |
| 5 | Draw the pencil Deckle is standing on. | `Paper` | `Ink(pencil)` | `Trace(pencil)` |
| 6 | Scribble anywhere. | `Paper` | `Strokes(1)` | `Trace(scribble)` |
| 7 | Now press {Undo}. It is gone. Nothing here can break. | `Action(Undo)` | `Undone` | `Perform(Undo)` |
| 8 | Someone left a smudge on your paper. Pick the {Eraser} and rub it out. | `Rail(Eraser)` | `Erased(smudge)` | `Trace(wipe)`, an erase path |
| | **Color** | | | |
| 9 | Deckle is ready. Press {Color}. *Why: Color is where the picture gets its night.* | `Step(InkPaint)` | `OnStep(InkPaint)` | Press |
| 10 | Pick {Night} on this card. | `lesson.chip` | `Picked(NIGHT)` | `Run(pick_night)` |
| 11 | Tap the paper, away from Deckle. | `Rail(Fill)` | `Filled { seed: SKY, keep: BODY }` | `Fill(SKY)`. After a leak, `Run(close_and_fill)` |
| 12 | Pick white. | `swatch.white` | `Picked(WHITE)` | `Run(pick_white)` |
| 13 | Tap inside Deckle. | `Rail(Fill)` | `Filled { seed: BODY, keep: SKY }` | `Fill(BODY)` |
| 14 | Pick red. | `swatch.red` | `Picked(RED)` | `Run(pick_red)` |
| 15 | Tap the pencil. | `Rail(Fill)` | `Filled { seed: PENCIL, keep: BODY }` | `Run(fill_pencil)` |
| 16 | Pick yellow. | `swatch.yellow` | `Picked(YELLOW)` | `Run(pick_yellow)` |
| 17 | Press {Brush}. | `Rail(Brush)` | `Tool(Brush)` | `Run(arm_brush)` |
| 18 | Press three stars into the sky. | `Paper` | `Ink(star_1, star_2, star_3)` | `Trace(star_1, star_2, star_3)` |
| | **Effects** | | | |
| 19 | Press {Effects}. *Why: Effects is where light gets added, after the picture is finished.* | `Step(Composite)` | `OnStep(Composite)` | Press |
| 20 | Tap {Glow}. | `shelf.glow` | `Effect(Glow)` | `Run(add_glow)` |
| 21 | Too bright. Drag {Strength} down until the stars are yellow again. | `shelf.strength` | `Strength` | `Run(glow_down)` |
| | **Export** | | | |
| 22 | Press {Export}. *Why: Export turns your project into a file anyone can open.* | `Step(Export)` | `OnStep(Export)` | Press |
| 23 | Save your picture. | `Action(ExportStill)` | `Exported(1)` | `Perform(ExportStill)`, the real save dialog |

**The leak.** Task 11 has a `miss`: if a fill reached `keep`, the sentence becomes `Night got into Deckle. Press {Undo}, then {Show me} closes the gap.`, and stays through that `Undo`. `close_and_fill` lands the path `outline`, the back and belly closed, then fills. The task cannot tick until Deckle is dry.

**A second Undo.** At task 7 a second `Undo` takes the pencil of task 5. `close_and_fill` lands `pencil` too when the sky would reach it. A night a person pours there stands where the pencil goes, so task 15's `Run(fill_pencil)` lands the line again and refills the night inside it.

**Task 21.** The shelf's first Glow triples what it reads, so at that strength a star burns to white and the line beside white paper goes with it. The lesson shows that and has the person bring it down. `glow_down` leaves `Strength` at 0.18, where a star keeps its yellow (blue well under green), the pencil keeps its red within the file's 8 levels, and the halo still shows 30 px off. Added by the review of AU23: Glow's own start stands.

**The moments.** The first fill anyone does here is the size of the sky. Three presses, and there are stars. One tap, and Deckle shines.

**Guide art.** Pencil lying across the lower third, x 180 to 1020, y 800 to 860, tip to the right and blunt, wider than the gap, so a fill reaches its end, one closed stroke. Deckle stands on it arched: feet at x 420 and x 700, peak y 520, strip 100 wide, its width turning with the bend so the legs reach the pencil, built by `upright(arch, 1)`. The front foot is lifted just past the gap, so the sky reaches under the arch. `back` is the torn side, `belly` the cut side. Holes along the strip, front hole near the front foot. Stars upper right, in an uneven triangle, (850, 240), (1000, 380), (760, 130). Smudge lower left at (230, 1030), about 120 across, clear of everything. `scribble` and `wipe` are never shown as guides. Seeds: SKY (100, 100), BODY (560, 545), PENCIL (600, 830). Trace load: 6 strokes.

**File facts.** PNG, 1200×1200. Night at SKY within 8 levels, white at BODY, red at PENCIL. Each star's centre yellow, and a point 30 px off it brighter than SKY, by Glow's halo. The smudge's box holds no grey. No pixel in guide colour.

---

## 2 A Coat of Shavings

| | |
|---|---|
| Id, kind, class | `coat_of_shavings`, Drawing, Amateur |
| Paper | Const `COAT`: 1400×1050, 1 frame, track `A`, stack `Solo merged` |
| Prep | A small Deckle, centre, line and white body both on the color layer, so hiding the sketch in task 4 cannot hide Deckle. Then `clear_history` |
| `fill_gap`, `ink` | None, none |
| On the paper | Landmarks: three pairs of ticks around Deckle, one pair per shaving |
| Card picture | One reference shaving, ruffled curl and painted rim. `Hold to see it` shows the full finish while held |
| Closing | `You made that. It is a real file. You can sketch under your ink, move what you drew, check it mirrored and paint inside your own shapes.` |
| Next | Plate One |

| # | On screen | Ring | Tick | Show me |
|---|---|---|---|---|
| | **Draw** | | | |
| 1 | Deckle is cold, and pencil shavings make a good coat. Sketch one big shaving between the first two ticks. A curl with a ruffled edge. Keep it loose. *Why: A sketch is for finding the shape, so it can be ugly.* | `Rail(Brush)` | `Ink(tick_1)` | `Where` |
| 2 | Open {Layers}, press + and pick {Custom layer}. Your sketch stays under it. | `Rail(Layers)`, `layers.add`, `layers.custom` | `Layers(3)` | `Run(add_layer)` |
| 3 | Ink the shaving on the new layer. One clean line, thicker on the turns. | `Paper` | `All([InkOnLayer, Ink(tick_1)])` | `Where` |
| 4 | Hide your sketch with its eye. | `Rail(Layers)`, `layers.eye` | `LayerHidden` | `Run(hide_sketch)` |
| 5 | Draw two more shavings at the other ticks. No two alike. | `Paper` | `Ink(tick_2, tick_3)` | `Where` |
| 6 | Pick {Select}, circle one shaving and drag it over another. A coat overlaps. | `Rail(Select)` | `Moved`: the frame sets a moved piece down when the drag ends, because a person has no other gesture that commits it | `Run(arm_select)`. `SelectTool` is a toggle, so `Perform` would disarm a learner who had already picked it |
| 7 | Open {View} and press {Mirror view}. A wrong curve shows up at once. Press it again when you have looked. | `menu.view`, `Action(MirrorView)` | `Mirrored` | `Perform(MirrorView)` |
| | **Color** | | | |
| 8 | Press {Color}. *Why: Color is where the wood gets warm, and your lines stay safe.* | `Step(InkPaint)` | `OnStep(InkPaint)` | Press |
| 9 | Pick brown and fill each shaving, and the overlap too. | `swatch.brown`, `Rail(Fill)` | `Fills(3)`, not 4: a tap on a line raises no fill, so 4 could strand a person who has three regions brown | `Run(pick_brown)` |
| 10 | Press {Brush}, open {Brushes} and turn on {Keep inside strokes}. | `Rail(Brush)`, `Float(Brushes)`, `keep.row` | `Locked(true)` | `Perform(ToggleAlphaLock)` |
| 11 | Pick yellow and paint a rim along each shaving. Paint it rough: it stays inside. | `swatch.yellow`, `Rail(Brush)` | `Strokes(3)` | `Run(pick_yellow_and_brush)` |
| 12 | Turn {Keep inside strokes} off. | `Rail(Brush)`, `Float(Brushes)`, `keep.row` | `Locked(false)` | `Perform(ToggleAlphaLock)` |
| 13 | Pick {Night} on this card and fill the paper around the coat. | `lesson.chip`, `Rail(Fill)` | `Filled { seed: CORNER, keep: none }` | `Run(night_at_the_corner)` |
| | **Effects** | | | |
| 14 | Press {Effects}. *Why: Effects is where the rims catch the light.* | `Step(Composite)` | `OnStep(Composite)` | Press |
| 15 | Tap {Glow}. | `shelf.glow` | `Effect(Glow)` | `Run(add_glow)` |
| 16 | Drag {Strength} until only the rims catch the light. | `shelf.strength` | `Strength` | `Run(nudge_strength)` |
| | **Export** | | | |
| 17 | Press {Export}. *Why: Export turns your project into a file anyone can open.* | `Step(Export)` | `OnStep(Export)` | Press |
| 18 | Save your picture. | `Action(ExportStill)` | `Exported(1)` | `Perform(ExportStill)` |

**Why the ground comes last.** `Keep inside strokes` holds paint to pixels already on the layer, and the color layer holds every fill. With Night down first the whole paper counts as inside, and a yellow rim would land anywhere. So wood first, rims, lock off, then Night. This is what a working painter does and worth feeling once. It rests on a read of the brush, not a run.

**Fallback for task 6.** If `commit_floating` cannot carry the `Moved` signal, or the raster rig cannot drive it: `Draw a fourth shaving over the others. A coat overlaps.`, ring `Paper`, tick `Strokes(1)`, and the report says so.

**Stand-ins.** `s1_sketch` (1), `s1_ink` (3), `s2_ink`, `s3_ink` (5), a lasso and drag for 6, seeds `s1`, `s2`, `s3` (9), `rim_1` to `rim_3` (11). The rims overrun their wood by at least 10 px on purpose, and stay clear of Deckle, so the file proves the lock held them.

**Guide art.** Landmarks only: six short ticks. Deckle by `deckle(rest, 2)`, small, at the centre, a third of the paper's height. The reference shaving is the path `shaving(seed)`, a generator of the same line as Deckle's hand: a spiral with a ruffled outer edge, its rim a darker band. No shaving outline ever stands on the paper.

**File facts.** PNG, 1400×1050. Three brown regions. Yellow only inside the brown mask. Night at CORNER. No pixel from `s1_sketch` alone. Deckle's line and white body present. A rim pixel brighter than the same pixel with Glow off, and the night a shaving-width from a rim within 20 levels of pure night: only the rims catch the light.

---

## 3 Plate One

| | |
|---|---|
| Id, kind, class | `plate_one`, Drawing, Experienced |
| Paper | Const `PLATE`: 1500×2000, 1 frame, track `A`, stack `Solo merged` |
| Prep, `fill_gap`, `ink` | None |
| On the paper | Nothing |
| Card picture | None until the end. A learner's plate is not compared with ours: the end card and the Lessons page's tile show three neutral stage marks, never the stand-in's boxes |
| Closing | `You made that. It is a real file. You can rough, ink, keep your roughs out of the file and deliver on a clear background.` |
| Next | The Lessons page |

| # | On screen | Ring | Tick | Show me where |
|---|---|---|---|---|
| | **Draw** | | | |
| 1 | Plate One of a field guide. Three stages of one life: the strip, the coat, and what comes out. What comes out is yours. Rough all three with {Sketch pencil}. *Why: Roughs are for deciding. Decide here.* | `Rail(Brush)`, `Float(Brushes)`, `brush.sketch` | `All([Armed(sketch), Strokes(6)])` | `Where` |
| 2 | Add a {Custom layer} for your ink. | `Rail(Layers)`, `layers.add`, `layers.custom` | `Layers(3)` | `Where` |
| 3 | Pick {Clean pencil} and raise {Steady stroke} until your hand stops showing in the long lines. | `Rail(Brush)`, `Float(Brushes)`, `brush.clean`, `steady.row` | `All([Armed(clean), Smoothing])` | `Where` |
| 4 | Ink all three stages. | `Paper` | `All([InkOnLayer, Strokes(6)])` | `Where` |
| 5 | Set the rough layer's role to Rough, so it never reaches a file. | `Rail(Layers)`, `layers.role` | `LayerRole(Rough)` | `Where` |
| 6 | Make a brush in {Brush maker}: under {More}, raise its scatter, then save it. | `menu.more`, `Action(BrushMakerRoom)`, `Pane(Forge)`, `forge_more`, `forge.scatter`, `forge.save` | `Scattered(my_brushes)` | `Where` |
| 7 | Press {Draw} to come back, pick your ink layer in {Layers}, and shade one stage with your new brush. | `Rail(Layers)`, `layers.pick` | `Strokes(1)` | `Where` |
| | **Color** | | | |
| 8 | Press {Color}. *Why: Flat colors only. The paper stays bare, so this plate sits on any page.* | `Step(InkPaint)` | `OnStep(InkPaint)` | `Where` |
| 9 | Flat-color all three stages, and leave the paper bare. | `Rail(Fill)`, `Pane(Palette)` | `Fills(3)` | `Where` |
| | **Effects** | | | |
| 10 | Press {Effects}. *Why: One effect, quietly. If someone can name it, it is too much.* | `Step(Composite)` | `OnStep(Composite)` | `Where` |
| 11 | Add one effect from the shelf and keep it subtle. | `shelf.strength` | `All([AnyEffect, Strength])` | `Where` |
| | **Export** | | | |
| 12 | Press {Export}. *Why: A plate is only finished when it can leave the app.* | `Step(Export)` | `OnStep(Export)` | `Where` |
| 13 | Export a PNG with a transparent background. If the file lands with the latch off: `That file kept its white paper. Open {More options} and turn on {Transparent background}.` | `export.more`, `export.transparent`, `Action(ExportStill)` | `Exported(1)`, latch on | `Where` |

**Fallback, task 5.** If a plain still does not leave out a Rough layer: `Hide the rough layer, so it never reaches a file.`, ring `Rail(Layers)`, `layers.eye`, tick `LayerHidden`. The purpose of a layer role was not read.

**Why scatter, not a paper grain (task 6, AU25 review).** A grain needs a texture brought in through an OS file dialog, and a new install has none, so a paper grain cannot be made on a first run and the gate could only fake one. Scatter is on the Brush maker's own fold, and the tick reads the value in the saved brush.

**Why a task of its own (task 7).** Brush maker hides the rail, so the shading stroke cannot land there. The door back arms the stack's working layer, which is the one task 5 hid, and a hidden layer refuses the pen, so the person picks their ink layer first. The door itself does not change: `arm_room` is free work's too.

**The wander line** holds off in task 6, because Brush maker is off the spine and the ring leads there. In task 7 it points back to Draw, which is the way back from an off-spine place.

**Stand-ins.** `rough_1` to `rough_6` (1), `ink_1` to `ink_6` (4), a grain stroke (7), seeds `body_1` to `body_3` (9). The gate's stand-in moth is a plain shape nobody sees, so it sets no example.

**Guide art.** None. Nothing stands on the paper in this lesson.

**File facts.** PNG, 1500×2000. All four corners alpha 0. No pixel where only a rough stroke was. Pixels with alpha 255 inside each stage. An effect reaches Output.

---

## 4 End of the Desk

| | |
|---|---|
| Id, kind, class | `end_of_the_desk`, Animation, Rookie. Straight ahead: one drawing after another |
| Paper | Const `EDGE`: 1024×768, 12 fps, 1 frame, tracks `BG`, `A`, stack `Solo merged` |
| Prep | BG: a night desk with its edge at x 720, an empty peg bar with a torn stub at the left. `A` armed. `clear_history` |
| `fill_gap`, `ink` | 8, chalk grey (100, 100, 112). Chalk on a night desk: the body is filled white in Color, so a white line would lose the eye |
| Guide | `over: true`, because BG is opaque |
| Card picture | The six pictures, tiny, three over three. The Lessons page's row shows one arch at the desk's end: six cannot read at its height |
| Closing | `You made that. It is a real file. You can add drawings, play them, set their speed and choose how long each one stays.` |
| Next | Still in the Coat |

| # | On screen | Ring | Tick | Show me |
|---|---|---|---|---|
| | **Plan** | | | |
| 1 | Deckle has reached the desk. Draw over the blue line, then press the blue dot for its eye. *Why: Plan is where your drawings land, one row after another.* | `Rail(Brush)` | `Ink(flat_1, eye_1)` | `Trace(flat_1, eye_1)` |
| 2 | Press {Add drawing}. | `Action(AddDrawingAfter)` | `Drawings(2)` | `Perform(AddDrawingAfter)` |
| 3 | Deckle arches. The front stays put and the back end catches up. Draw it over the blue line. The faint one is your last drawing. | `Paper` | `Ink(arch_1, eye_2)` | `Trace(arch_1, eye_2)` |
| 4 | Press {Add drawing}. | `Action(AddDrawingAfter)` | `Drawings(3)` | `Perform(AddDrawingAfter)` |
| 5 | Flat again. Now the back stays put and the front reaches out. | `Paper` | `Ink(flat_2, eye_3)` | `Trace(flat_2, eye_3)` |
| 6 | Press {Add drawing}. | `Action(AddDrawingAfter)` | `Drawings(4)` | `Perform(AddDrawingAfter)` |
| 7 | Arch again. Deckle has reached the edge. | `Paper` | `Ink(arch_2, eye_4)` | `Trace(arch_2, eye_4)` |
| 8 | Press {Add drawing}. | `Action(AddDrawingAfter)` | `Drawings(5)` | `Perform(AddDrawingAfter)` |
| 9 | One more. Half of Deckle is past the edge. | `Paper` | `Ink(flat_3, eye_5)` | `Trace(flat_3, eye_5)` |
| 10 | Press {Add drawing} and leave this one empty. Deckle is gone. | `Action(AddDrawingAfter)` | `Drawings(6)` | `Perform(AddDrawingAfter)` |
| 11 | Press {Play}. | `Action(PlayPause)` | `Played` | `Perform(PlayPause)` |
| 12 | Too fast. Press {Speed} and pick {Medium}. | `band.fold`, `band.speed`, `speed.medium` | `Speed(2)` | `Run(set_medium)` |
| | **Timing** | | | |
| 13 | Press {Timing}. *Why: Timing is where you choose how long each picture stays.* | `Step(Edit)` | `OnStep(Edit)` | Press |
| 14 | Click the last arch on the sheet. | `Cell { A, 6 }` | `Frame(6..=7)` | `Run(goto_last_arch)` |
| 15 | Press {Longer} three times. Deckle stops at the edge and looks down. | `band.longer` | `Hold { frame: 6, len: 5 }` | `Run(longer_three)` |
| 16 | Press {Play}. The wait is the joke. | `Action(PlayPause)` | `Played` | `Perform(PlayPause)` |
| | **Color** | | | |
| 17 | Press {Color}. *Why: Color is where each picture gets filled. Your lines stay safe.* | `Step(InkPaint)` | `OnStep(InkPaint)` | Press |
| 18 | Pick white. | `swatch.white` | `Picked(WHITE)` | `Run(pick_white)` |
| 19 | Click the first picture along the bottom, and tap inside Deckle. | `strip.next` | `FilledOn(1)` | `Run(fill_here)` |
| 20 | Click each picture along the bottom, and tap inside Deckle. | `strip.next` | `FilledOn(5)` | `Run(fill_the_rest)` |
| | **Effects** | | | |
| 21 | Press {Effects}. *Why: Effects is where the chalk starts to shine.* | `Step(Composite)` | `OnStep(Composite)` | Press |
| 22 | Tap {Glow}. | `shelf.glow` | `Effect(Glow)` | `Run(add_glow)` |
| | **Export** | | | |
| 23 | Press {Export}. *Why: Export turns your drawings into a file anyone can watch.* | `Step(Export)` | `OnStep(Export)` | Press |
| 24 | Save your animation. | `Action(ExportGif)` | `Exported(1)` | `Perform(ExportGif)` |

**Why the numbers work.** Six drawings at one frame each. `Medium` gives `[2, 2, 2, 2, 2, 2]`. `Longer` three times on the fourth, through `set_run_length`, which ripples later keys and takes the cut with it, gives `[2, 2, 2, 5, 2, 2]`: 15 frames, 1.25 s. BG holds to the end, so `shown_length` keeps the empty sixth picture. The empty desk is the punchline.

**Task 12.** Below a band 1040 wide, Speed stands behind `…`, so the 1024×640 gate rings `…` first. The word "strip" stays out of tasks 19 and 20, because Deckle is one.

**Tasks 19 and 20.** The sheet is gone in Color and the band's Next steps one frame, so it stops on held frames, where Fill refuses. The second `Play` stops on the empty sixth picture, so task 19 cannot be a bare tap either. A click on the bottom row lands on the picture's own frame (`strip::tap`), and `strip.next` rings the next one still white. The cursor after Timing does not matter. `Run(fill_here)` in task 19 goes to its drawing's first frame, because task 14 also accepts a held row.

**Guide art.** Six pictures on the desk line y 520. Strip length 360, 72 wide, built by `upright(spine, 30)`, so the length holds and an arch's legs keep their width. A shorter strip is all bend and fails the hand-wobble check, and so does an arch on a chord under 190. Each pose one closed outline and one dab: 2 strokes.

| Pose | Back foot x | Front foot x |
|---|---:|---:|
| `flat_1` | 190 | 550 |
| `arch_1` | 360 | 550 |
| `flat_2` | 360 | 720 |
| `arch_2` | 530 | 720 |
| `flat_3` | 530 | 890, drooping past the edge at x 720, 170 of its 360 past |

Each move plants one foot, so nothing skates. Peg bar x 20 to 160, on the desk. The room right of the edge stays empty, for a fall we never show.

**File facts.** GIF, 1024×768, 15 frames, 1.25 s, endless (`gif.rs`'s `LOOP_BLOCK`). 6 distinct pictures. Deckle's mass moves right. The last picture equals BG with Glow. No pixel in guide colour.

---

## 5 Still in the Coat

| | |
|---|---|
| Id, kind, class | `still_in_the_coat`, Animation, Amateur. Pose to pose: keys first, timing after |
| Paper | Const `STILL`: 1024×768, 12 fps, 24 frames, tracks `BG`, `A`, stack `Solo merged` |
| Prep | BG: a dark desk edge held for the shot. `clear_history` |
| `fill_gap`, `ink` | None, white |
| On the paper | Landmarks: a floor tick and a centre tick |
| Card picture | Three small poses. `Hold to see it` shows the rest pose full size |
| Closing | `You made that. It is a real file. You can plant keys on the rows you choose, wait on purpose and reuse a drawing.` |
| Next | Desk to Lamp |

| # | On screen | Ring | Tick | Show me |
|---|---|---|---|---|
| | **Plan** | | | |
| 1 | Deckle has wrapped itself in a coat of shavings. Click row 1 on track A and draw the coat at rest. *Why: Plan is where the main poses land on the rows you choose.* | `band.sheet`, `Cell { A, 0 }` | `KeyArt { A, 0 }` | `Where` |
| 2 | Click row 5. Draw the coat squeezed to the left. | `band.sheet`, `Cell { A, 4 }` | `KeyArt { A, 4 }` | `Where` |
| 3 | Click row 7. Draw it squeezed to the right. | `band.sheet`, `Cell { A, 6 }` | `KeyArt { A, 6 }` | `Where` |
| 4 | Press {Play}. Is it twitchy? Good. | `Action(PlayPause)` | `Played` | `Perform(PlayPause)` |
| | **Timing** | | | |
| 5 | Press {Timing}. *Why: This is where nothing gets to happen.* | `Step(Edit)` | `OnStep(Edit)` | Press |
| 6 | Right-click each drawing on the sheet and mark all three as keys. | `Cell { A, 0 }` | `Kinds(3)` | `Run(mark_keys)` |
| 7 | Click row 1 and press {Longer} until the rest lasts 12 rows. Wait longer than feels right. | `Cell { A, 0 }`, `band.longer` | `Hold { frame: 0, len: 12 }` | `Run(goto_first)` |
| 8 | Click row 16 twice, type {rest drawing} and press Enter. The rest drawing comes back after the shiver. | `Cell { A, 15 }` | `All([Shows { frame: 14, drawing: 2 }, Shows { frame: 15, drawing: 0 }])` | `Run(goto_rest_row)` |
| 9 | Turn on {Loop}, and set {Loop start} and {Loop end} around the shot. | `band.loop`, `band.fold`, `Action(LoopIn)`, `Action(LoopOut)` | `Loop` | `Run(turn_on_loop)` |
| 10 | Press {Play}. If the wait is not funny yet, change it with {Longer} or {Shorter}. | `Action(PlayPause)` | `Played` | `Perform(PlayPause)` |
| | **Color** | | | |
| 11 | Press {Color}. *Why: Your lines stay safe here.* | `Step(InkPaint)` | `OnStep(InkPaint)` | Press |
| 12 | Fill all three drawings: brown for the wood, yellow for the rims. | `Rail(Fill)`, `swatch.brown`. The sentence takes them to yellow | `FilledOn(3)` | `Run(pick_brown)` |
| | **Effects** | | | |
| 13 | Press {Effects}. *Why: Effects is where the rims catch the light.* | `Step(Composite)` | `OnStep(Composite)` | Press |
| 14 | Tap {Glow}. | `shelf.glow` | `Effect(Glow)` | `Run(add_glow)` |
| 15 | Drag {Strength} until only the rims catch the light. | `shelf.strength` | `Strength` | `Run(nudge_strength)` |
| | **Export** | | | |
| 16 | Press {Export}. *Why: Export turns your drawings into a file that loops forever.* | `Step(Export)` | `OnStep(Export)` | Press |
| 17 | Save a GIF. | `Action(ExportGif)` | `Exported(1)` | `Perform(ExportGif)` |

**Tasks 1 to 3 are the Lead's Plan.** Sheet and canvas on one screen, linked by a click. The text says "click" once, because a second click on the cursor's own cell opens cell entry. Task 8 is the one place that says twice. Every ring on a Plan cell starts with `band.sheet`, because the sheet can be hidden there mid-lesson, and then the ring stands on the button that brings it back. Timing's sheet cannot be hidden.

**Task 8.** The cell takes a drawing's name, and strokes name drawings `D1A`, `D2A`, so `{rest drawing}` is read at run time from row 1. Typing a bare number would make a new blank drawing and lose the pose. `Shows`' 0 is the first key on the track.

**Why the numbers work.** Three keys on 24 rows give `[4, 2, 18]`. Eight presses of `Longer` on the first ripple later keys and take the cut along: `[12, 2, 18]`, 32 frames. A drawing on a blank tail keeps to its row, so the right pose stops at row 15, and a key at row 17 would flash the blank row 16 between right and rest. A key at row 16 gives `[12, 2, 1, 17]`: rest, left, right, rest. 32 frames at 12 fps is about 2.67 s.

**Stand-ins.** `coat_rest`, `coat_left`, `coat_right`, each a closed outline with a rim band. Seeds for each, two colours.

**Guide art.** Landmarks only. The three poses on the card are `coat(pose)`, a blob of shavings by the same generator, with one small eye in a gap. The rest is round. The squeezes share its area, within 5%, so the coat keeps its weight, and that is checked with the same on-model test.

**File facts.** GIF, 1024×768, 32 frames, about 2.67 s, endless. First and last pictures equal. 3 distinct pictures. Frame 16 shows the same picture as frame 0. The loop range does not change the file. BG present in every frame.

---

## 6 Desk to Lamp

| | |
|---|---|
| Id, kind, class | `desk_to_lamp`, Animation, Experienced |
| Paper | Const `LAMP`: 1024×768, 12 fps, 48 frames, tracks `BG`, `A`, `W`, stack `Solo merged` |
| Prep | BG: the desk and a lit lamp. `A`: blank keys marked at rows 1, 21 and 41. A code-made hum on a sound track, made as `sample.rs` makes `dialogue_wav`, never a file dialog. `clear_history` |
| `fill_gap`, `ink` | None, white |
| On the paper | Nothing |
| Card picture | None until the end |
| Closing | `You made that. It is a real file. You can key a shot, chart it, time it to sound and move a camera through it.` |
| Next | The Lessons page |

| # | On screen | Ring | Tick | Show me where |
|---|---|---|---|---|
| | **Plan** | | | |
| 1 | The moth wakes. Its first flight is from the desk to the lamp, and the hum is already on the sheet. Design it. On track W, rows 41 to 43, draw the wings closed, half and open, at the lamp where it lands. {Lip flap} reads those three, in that order. *Why: Plan is where a design becomes keys.* | `band.sheet`, `Cell { W, 40 }` | `All([KeyArt { W, 40 }, KeyArt { W, 41 }, KeyArt { W, 42 }])` | `Where` |
| 2 | On track A, draw the body on the three marked keys: take-off, the turn, the landing. In flight, its wings are part of the body. | `band.sheet`, `Cell { A, 0 }` | `All([KeyArt { A, 0 }, KeyArt { A, 20 }, KeyArt { A, 40 }])` | `Where` |
| 3 | Open {Canvas options}, chart the spacing between two keys, make the in-betweens and draw them. | `menu.view`, `Action(CanvasOptions)`, `options.chart`, `Action(MakeInbetweens)` | `All([Chart, Inbetweens])` | `Where` |
| | **Timing** | | | |
| 4 | Press {Timing}. *Why: Timing is where a moth gets its weight.* | `Step(Edit)` | `OnStep(Edit)` | `Where` |
| 5 | Let the moth hang at the lamp for at least a second. | `Cell { A, 40 }`, `band.longer` | `Hold { frame: 40, len: 12 }` | `Where` |
| 6 | Run {Lip flap} on track W from row 41, so the wings beat with the hum while it hangs. | `menu.animation`, `menu.animation.track`, `Action(LipFlap)`, `lip.in` | `Lipped(W)` | `Where` |
| | **Color** | | | |
| 7 | Press {Color}. *Why: Two colors. A small thing needs a small palette.* | `Step(InkPaint)` | `OnStep(InkPaint)` | `Where` |
| 8 | Fill every drawing, in two colors. | `Rail(Fill)`, `Pane(Palette)` | `FilledOn(9)` | `Where` |
| | **Effects** | | | |
| 9 | Press {Effects}. *Why: Effects is where the camera gets its nerve.* | `Step(Composite)` | `OnStep(Composite)` | `Where` |
| 10 | Add a {Camera move} from the desk to the lamp. | `shelf.camera` | `CameraKeys(2)` | `Where` |
| 11 | Add dust to the air: {Add effect}, Particles, Dust. | `shelf.add` | `Particles` | `Where` |
| 12 | Ease the camera in {Motion curves}, so it arrives late. | `menu.more`, `menu.more.panels`, `Action(ShowCurves)`, `Pane(Curves)` | `Eased` | `Where` |
| | **Export** | | | |
| 13 | Press {Export}. *Why: Two files: frames for an editor, a GIF for everyone else.* | `Step(Export)` | `OnStep(Export)` | `Where` |
| 14 | Export a PNG sequence. | `Action(ExportPngSequence)` | `Exported(1)` | `Where` |
| 15 | Export a GIF. | `Action(ExportGif)` | `Exported(1)` | `Where` |

**Why the last key is at row 41.** Its run is 8 rows, so "at least a second" is a real act, 4 presses of `Longer`, and the cut grows to 52 frames.

**No MP4.** It needs ffmpeg, and a lesson must not.

**Lip flap on wings.** It reads the first three drawings as closed, half and open, which is why task 1 orders them. The gate runs it on drawn wings over the code-made hum. **Fallback:** if it fails, task 6 reads `Set the wings by hand, closed, half, open, for a beat of 3 rows.`, tick `Lipped(W)` unchanged (cells on W past row 3).

**Where the wings are (AU25 review, the Lead's to confirm).** Track W is a plain cel with no road of its own, so wings beating across the whole cut stand still at the desk while the body flies off, and the moth comes apart. The file left that open, and the part took the smaller of the reviewer's two roads, recorded here and on the spec's list for the Lead. The flying keys and their in-betweens carry the wings in the body's own drawing, W stays blank until row 41, and Lip flap beats W's three drawings only while the moth hangs at the lamp. The other road, a keyed Transform on W's plane, needs a person's route to a node's parameters that no step offers yet. So the hang comes before Lip flap, whose window takes the cut as it stands, and the fold's `in` field is the ring's last stop.

**The hum** is made to suit this: a beat of 8 rows, each of Lip flap's three bands held 2 rows, closed, half, open, half, so all three wings land and the 12 rows of the hang hold 6 beats. It runs 60 rows, past the cut, so the hang never meets silence.

**Stand-ins.** `wing_0` to `wing_2`, `body_1` to `body_3`, in-between drawings, seeds, two camera keys.

**Guide art.** None on the paper. BG and the hum are made by code. The lamp's cone is one soft shape, so the dust has something to catch.

**File facts.** A folder of frames and one GIF, the same count, at least 52. W cells past row 3, none before row 41, all three wings among them. At the first frame W is blank; at the last, W's ink stands within reach of the landed body. First and last pictures differ by the camera. Dust present: pixels changed against the same frame with Particles off. No `.mp4`.

---

## 7 Not a Pencil

| | |
|---|---|
| Id, kind, class | `not_a_pencil`, Comic, Rookie. Four panels, no words |
| Paper | Const `PAGE_SMALL`: `Page::print("B5", [182, 257], 3, [150, 220], 120.0)`, about 888×1243, tracks `INK`, `TONE`, one panel |
| Prep | None |
| `fill_gap`, `ink` | 8, default black |
| Card picture | The finished page, small |
| Closing | `You made that. It is a real file. You can split a page into panels and make a reader wait between them.` |
| Next | Too Much Coat |

| # | On screen | Ring | Tick | Show me |
|---|---|---|---|---|
| | **Manga** | | | |
| 1 | Four panels, no words. Press {Split across}. *Why: Manga is where a page is split into boxes, and the reader's eye goes box to box.* | `Pane(Manga)`, `manga.split_across` | `Panels(2)` | `Run(split_across)` |
| 2 | Press {Split down}. | `manga.split_down` | `Panels(3)` | `Run(split_down)` |
| 3 | Click {panel 3}. | `manga.panel.3` | `PanelPicked(2)` | `Run(pick_panel_3)` |
| 4 | Press {Split down} again. Four panels. | `manga.split_down` | `Panels(4)` | `Run(split_down)` |
| 5 | Panel 1. Deckle is asleep on the desk. Draw over the blue lines. | `Rail(Brush)` | `Ink(p1_desk, p1_deckle, p1_eye)` | `Trace(p1_desk, p1_deckle, p1_eye)` |
| 6 | Panel 2. Something is coming. Draw over the blue lines. | `Paper` | `Ink(p2_deckle, p2_eye, p2_shadow)` | `Trace(p2_deckle, p2_eye, p2_shadow)` |
| 7 | Panel 3. Deckle hides among the pencils. Stand very still. | `Paper` | `Ink(p3_cup, p3_pencils, p3_deckle)` | `Trace(p3_cup, p3_pencils, p3_deckle)` |
| 8 | Panel 4. Close up. One eye looks sideways. | `Paper` | `Ink(p4_edges, p4_eye)` | `Trace(p4_edges, p4_eye)` |
| | **Color** | | | |
| 9 | Press {Color}. *Why: One black is enough. It is the heaviest thing on a page.* | `Step(InkPaint)` | `OnStep(InkPaint)` | Press |
| 10 | Pick black. | `swatch.black` | `Picked(BLACK)` | `Run(pick_black)` |
| 11 | Tap the shadow in panel 2. | `Rail(Fill)` | `Filled { seed: SHADOW, keep: P2_PAPER }` | `Fill(SHADOW)` |
| | **Export** | | | |
| 12 | Press {Export}. *Why: Export turns your page into a file you can print.* | `Step(Export)` | `OnStep(Export)` | Press |
| 13 | Press {Export pages}. | `Action(ExportPages)` | `Exported(1)` | `Perform(ExportPages)` |

**The gag.** A hand's shadow only, so nothing is hard to draw. The fourth panel is why the page turns out funny: it is the same still strip, and it knows.

**If Color leaves the Comic bar** (the kinds part's first check): tasks 10 and 11 move to the Manga step after task 8, and task 9 goes. Fill stays on Manga's rail. The same holds for tasks 8 and 9 of Too Much Coat and 8 and 9 of The Only Word.

**Guide art.** Four panels, each ≤6 strokes, from `deckle(spine, 7)`. Panel 1: a desk line, Deckle flat, an eye dab. Panel 2: the same Deckle, the shadow one closed shape coming in from the top edge of the panel, its claws blunt and wider than the gap, so the fill reaches their ends. Panel 3: a cup, three pencils as one stroke each joined to the cup's rim, and Deckle upright among them. Panel 4: two straight edges that leave by the panel's top, standing on its bottom line, and one eye looking right, hard against the far edge. Panel rectangles come from the same fractions as `panel_op`, so guides stay inside. If the numbering of `panel 3` differs when built, this lesson follows it: `manga.panel.<n>` is by index and the tick reads `selected`.

**File facts.** One PNG, about 888×1243. 4 panels with the page border. One black region at SHADOW. No pixel in guide colour. Ink present in each panel.

---

## 8 Too Much Coat

| | |
|---|---|
| Id, kind, class | `too_much_coat`, Comic, Amateur. Two pages, the second a surprise |
| Paper | Const `PAGE_SMALL`, as Not a Pencil |
| Prep, `fill_gap`, `ink` | None |
| On the paper | Page 2: a horizon tick and a vanishing-point tick |
| Card picture | Six thumbnails for page 1, one for page 2 |
| Closing | `You made that. It is real files. You can pace a page and put the surprise where the reader has to turn.` |
| Next | The Only Word |

| # | On screen | Ring | Tick | Show me |
|---|---|---|---|---|
| | **Manga** | | | |
| 1 | Deckle wants more coat. Page one: pick the {Panel tool} and split it into six panels. *Why: A comic's pace is its panels. Six small ones read fast.* | `Action(PanelTool)` | `Panels(6)` | `Perform(PanelTool)` |
| 2 | Open the gutters a little. They are the silence between panels. | `manga.gutter.side`, `manga.gutter.row` | `Gutters` | `Run(open_gutters)` |
| 3 | In each panel, draw Deckle dragging one shaving home. The thumbnails are on this card. | `Rail(Brush)` | `InkInPanels(6)` | `Where` |
| 4 | Press {New page}. | `Action(NewPage)` | `Pages(2)` | `Perform(NewPage)` |
| 5 | Set a 1-point ruler along the desk, and turn snap on. | `manga.rulers`, `Action(ToggleRulerSnap)` | `All([Rulers(1), Snap(true)])` | `Run(open_rulers)` |
| 6 | Page two is one big picture: pencils going away like trees. | `Paper` | `Strokes(5)` | `Where` |
| 7 | Turn {Snap to rulers} off. | `manga.rulers`, `Action(ToggleRulerSnap)` | `Snap(false)` | `Perform(ToggleRulerSnap)` |
| 8 | Put the surprise here, after the page turn: one giant coat, one small eye. | `Paper` | `Strokes(3)` | `Where` |
| | **Color** | | | |
| 9 | Press {Color}. *Why: One flat color is this page's whole budget. Spend it on the coat.* | `Step(InkPaint)` | `OnStep(InkPaint)` | Press |
| 10 | Fill the coat with one color. | `swatch.brown`, `Rail(Fill)` | `Fills(1)` | `Run(pick_brown)` |
| | **Export** | | | |
| 11 | Press {Export}. *Why: Two pages, two files.* | `Step(Export)` | `OnStep(Export)` | Press |
| 12 | Press {Export pages}. | `Action(ExportPages)` | `Exported(1)` | `Perform(ExportPages)` |

**Task 7.** The snap holds every stroke to a ruler's line. Left on, task 8's coat would come out straight. Neither the lock nor the snap is in the file, so a resumed run takes both from the tasks before it, and `Leave lesson` gives back what was there.

**Stand-ins.** Cuts from the table `split_into_six` (1), `t1` to `t6` and each one's shaving (3), `tree_1` to `tree_5` (6), `coat_big` and an eye (8), seed `coat` (10).

**Guide art.** Landmarks and thumbnails only. Thumbnails are quarter-scale paths by `deckle` and `shaving`, a small one each. Page 2's thumbnail shows the desk going away to one point, and the giant coat filling the lower half.

**File facts.** Two PNGs, about 888×1243 each. At least 6 panels on page 1. Page 2 has a 1-point ruler in its meta and one brown region. Rulers do not reach the file.

---

## 9 The Only Word

| | |
|---|---|
| Id, kind, class | `only_word`, Comic, Experienced. One serious page and one word |
| Paper | `Manga B5` from the real table: 2591×3624, tracks `INK`, `TONE`, one panel |
| Prep, `fill_gap`, `ink` | None |
| On the paper | Nothing |
| Card picture | None until the end |
| Closing | `You made that. It is a real file. You can rule a perspective, lay tone, letter a page and send it to print.` |
| Next | The Lessons page |

| # | On screen | Ring | Tick | Show me where |
|---|---|---|---|---|
| | **Manga** | | | |
| 1 | The moth has seen the bulb. Draw the most serious page ever made about something very small. Split it into at least five panels, one tall for the dive. *Why: Save the only word in this course for the end.* | `Action(PanelTool)` | `Panels(5)` | `Where` |
| 2 | Set a 3-point ruler, snap on. Build a canyon of book spines. | `manga.rulers`, `Action(ToggleRulerSnap)` | `All([Rulers(3), Strokes(6)])` | `Where` |
| 3 | Turn {Snap to rulers} off, then ink the dive. Small moth, huge canyon. | `manga.rulers`, `Action(ToggleRulerSnap)` | `All([InkInPanels(3), Snap(false)])` | `Where` |
| 4 | Lay tone on the sky only. | `manga.ink_tone`, `manga.tone_preset` | `Tone` | `Where` |
| 5 | Set Draw on back to Ink. | `manga.ink_tone` | `OnTrack(INK)` | `Where` |
| 6 | Add {Speed lines}. They cross the whole page, so set their direction to 90, down the dive, and thin them out. | `manga.speed_lines`, `manga.speed.direction` | `All([Generator(SpeedLines), LinesDown])` | `Where` |
| 7 | Last panel: one balloon, one word: tink | `manga.panel.5`, `manga.balloon`, `manga.words` | `All([Generator(Balloon), Words])` | `Where` |
| 8 | Press {See the page}, check it as it will print, then come back. | `manga.see_page` | `PageSeen` | `Where` |
| | **Color** | | | |
| 9 | Press {Color}. *Why: A black wall makes a small bulb glare.* | `Step(InkPaint)` | `OnStep(InkPaint)` | `Where` |
| 10 | Flood the wall behind the bulb to solid black. | `swatch.black`, `Rail(Fill)` | `Fills(1)` | `Where` |
| | **Export** | | | |
| 11 | Press {Export}. *Why: Mono is how a page goes to print.* | `Step(Export)` | `OnStep(Export)` | `Where` |
| 12 | Export in mono, with a threshold that keeps the tone clean. If the pages land with the latch off: `That page kept its greys. Set {Mono}, then export again.` | `export.page.mono`, `Action(ExportPages)` | `Exported(1)`, mono on | `Where` |

**This lesson needs the Comic step.** Tasks 4 to 8 use its verbs, so no one meets the Tab menu, typed fractions or a DANGER edge. `PageSeen` passes with no GPU, as the Comic step specifies. Task 12's Mono latch stands on the Export step only because that part puts it there: today it is a row in the Manga pane.

**Why the pen goes back to Ink (task 5).** Task 4 leaves `Draw on` on Tone, and a flood lands on the track in hand. On TONE the wall would print as dots.

**Speed lines cross the whole page (task 6).** The generator has no panel to stay in and the Comic step's pane offers only direction and count, so the lines cannot be confined to the dive without an engine change, which AU25 may not make. The sentence says what the control does, and the tick reads the direction. Confining them is on the spec's list for the Lead.

**Stand-ins.** Cuts, `spine_1` to `spine_6`, `dive_1` to `dive_3`, a tone patch, the word `tink`, seed `wall`.

**Guide art.** None on the paper.

**File facts.** One PNG, 2591×3624, every pixel black or white. Panels ≥5. Tone on the `TONE` track. The wall solid black, on the `INK` track. A balloon whose centre stands in the last panel, and the text `tink` in the page graph. Speed lines running down the page, at a thinned count. No DANGER edge: `page_hand_built` is None.

---

## Shared art plan

| Piece | Rule |
|---|---|
| Data | No binary assets. Each lesson's `art` builds `Path`s in paper units from cubic curves, sampled to `(x, y, pressure)` |
| `deckle(spine, seed)` | Outline from a spine curve. Constant arc length, so every pose is on model. Planted feet. One long side torn from seeded noise, the other cut straight |
| Line | Tapers like a pen, thicker on the turn. Slight overshoot at corners. Nothing symmetrical. A hand wobble along the curve |
| `shaving(seed)` | A spiral with a ruffled edge and a band for its rim |
| Seeds | Each lesson its own, so no two lessons share a path. `aurookie_kinds_share_no_paths`, `aurookie_kinds_look_different`, `auveteran_classes_differ` |
| Finish pictures | Built from guides and stand-ins, drawn by the driver's paths on the card as vector lines, so they cannot drift from the lesson |

**Contact sheet.** An ignored test writes all finishes to one PNG for the Lead's eye, with no window. Every art number is a first estimate, set from the first measure and never tuned to pass. The checks are in the feedback spec, 5.14. One first-time user doing Up Late by hand is on the Lead's list and is not a code gate.

## Tokens

Resolved at run time, so a word changed by Plain words changes here too. A token the build cannot resolve fails `aulesson_tokens_resolve`.

| Token | Source |
|---|---|
| `{Draw}`, `{Color}`, `{Effects}`, `{Timing}`, `{Export}` | `vocab::STEP_*` |
| `{Longer}`, `{Shorter}` | `vocab::LONGER`, `vocab::SHORTER` |
| `{Layers}`, `{Brushes}`, `{Canvas options}`, `{Steady stroke}`, `{Keep inside strokes}`, `{Sketch pencil}`, `{Clean pencil}`, `{Brush maker}`, `{Loop}`, `{Loop start}`, `{Loop end}` | `vocab::` constants of those names |
| `{Brush}`, `{Eraser}`, `{Select}`, `{Fill}`, `{Size}` | The rail's faces |
| `{Add drawing}`, `{Speed}`, `{Medium}`, `{Play}`, `{Undo}`, `{View}`, `{Mirror view}`, `{Motion curves}`, `{Lip flap}`, `{Custom layer}`, `{New page}`, `{Panel tool}`, `{Export pages}`, `{More}`, `{More options}`, `{Transparent background}`, `{Mono}` | The control's own label, one const each, shared with it |
| `{Glow}`, `{Strength}`, `{Add effect}`, `{Camera move}` | `Effects` step constants |
| `{Split across}`, `{Split down}`, `{panel 3}`, `{See the page}`, `{Speed lines}`, `{Snap to rulers}` | `manga_tools` constants, `op.label()` |
| `{Night}`, `{Show me}` | `lesson.rs` |
| `{rest drawing}` | `lesson.rs`: the name of the drawing on row 1 of track A, read from the sheet when the task opens, for example `D1A` |
| `{chord:Action}` | `chord_label`, for Experienced text if the build adds a chord to a goal |
