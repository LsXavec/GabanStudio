# PSD: the Lead's feedback on the approachable UI

To live at `research/PSD-approachable-ui-feedback.md`. **This document wins over `research/PSD-approachable-ui.md` ("the main spec") wherever they differ.** The nine lessons in full live in `research/LESSONS-approachable-ui.md` ("the lessons file").

Base: branch `approachable-ui`, parts 1 to 9 built, head f3bb1dc, which holds the Export review fixes. The Keys and Hide tools part was running, uncommitted, in the same tree when this was reviewed, so it lands first (1.12). Nothing here was compiled, run or timed. Numbers are arithmetic from the source. Functions are cited because lines drift. The last section says what was not verified.

## 0 What the Lead asked

The Lead's words, verbatim:

> The look is friendly yes, Startup menu is up to the right so could be centered maybe that would better fit but find a more central start, The toolbar I very much like. But building each step into your workflow matters. Maybe in the start we have a tutorial that we follow to recreate something like a rookie amature and Experienced class per each user type that would use the application. Have the tutorial be Unique and Set apart from other references. Ill let you decide what they are drawing. Then aswell as for the animation which would be different than both the Comic, Building out the tutorial for that aswell. Overall impressed with the UI and design so far. I will say the open canvas with the left toolbar is pretty much as good as it gets for the current state. on the Plan Side of things I see the Canvas has a locked Composite window which Defeats the purpous of the Left X sheet in the Plan window. My approach would to be if you hare working on a Animatino to have the Plan/Genga be that step and for just drawing having the first step the Draw removing the Plan window in that workspace. That way you dont have 2 workflows that in theory someone would look at and skip the plan window.

| The Lead's sentence | Answer | Where |
|---|---|---|
| Startup menu up to the right, find a more central start | Start stands dead centre, 600×300 at the most, at every gated size | 4, AU20 |
| The toolbar I very much like. The open canvas with the left toolbar is as good as it gets | The rail, the look and the open canvas do not change. A plain drawing gets more paper, because no strip opens on it | 2.5, every part's Must not change |
| Building each step into your workflow matters | The bar depends on the project. Every lesson walks its bar, and the learner presses every pill | 2, 5.2, `aulesson_tasks_walk_the_bar` |
| A tutorial to recreate something, rookie, amateur, experienced, per user type | Nine lessons: three classes in each of drawing, animation, comic. Each ends in a real file | Lessons file, AU20, AU21, AU23 to AU25, AU27, AU28 |
| Unique and set apart. I'll let you decide | Deckle, a torn strip of animation paper that gets away | 5.1 |
| Animation different from comic | Different subject, different craft, no shared art, and finishes that do not look alike across types, proved by a test | `aurookie_kinds_share_no_paths`, `aurookie_kinds_look_different`, `auveteran_classes_differ` |
| Plan's canvas is a locked composite, which defeats the X-sheet | The preview is gone. Plan is a real drawing step with the sheet at left | 3, AU19 |
| Plan/Genga for an animation, Draw first for a drawing, no Plan window there | Three bars | 2 |
| No two workflows someone would skip one of | An animation has no Draw pill. A drawing has no Plan or Timing pill. One route per project | 2, `aukind_bars_are_the_table` |

**The build lead's defaults.** D1 kept. D2 kept with one change: Plan's left column is the sheet alone, not `dock_with_sheet`, because that recipe holds the sheet to about 15 rows against about 33 and `auseam_frozen_boot_is_todays_recipe` pins it. D3 kept with one change: the key is `app.kind` through `project.app_meta`, because `store::save` deletes and rewrites `meta`, so a raw key is gone at the first save. D4 kept and extended: Fit clears the rail while the sheet is on and while a lesson stands.

## 1 Overrides

Each row replaces the main-spec text it names. Nobody needs to read the old rule. Only numbers named here change. No gate is loosened by this document: see 1.10.

### 1.1 Measured promise (main spec section 0)

| Row | Old | New |
|---|---|---|
| Paper share, Draw | 63.7% by arithmetic, gate ≥63.0% | The ready paper is a drawing with the strip closed (2.5), so the band is 36. Paper 1370×771, **73.4% by arithmetic. Gate stays ≥63.0%**. The other floors stand too: `PAPER_FLOORS` 63.0, 66.5, 57.5, 60.0 against about 73.4, 69.8, 69.8, 65.2 |
| Same, with Start at its largest | 58.3%, gate ≥58.0% | The card lies wholly on paper: (1,056,270 − 180,000) ÷ 1,440,000 = **60.9% by arithmetic. Gate stays ≥58.0%**. Fresh, 600×244: 63.2%. The engine study's "cannot hold" assumed the strip open |
| Visible controls and words, Draw and First screen | Gates ≤30, ≤44, ≤40, ≤70 | Gates stand. Counts fall, because the bar has four pills and the band loses `Add drawing` and `Timing sheet` on a drawing |
| Ball-bounce journey | 27 actions | 28 actions, gate ≤28 unchanged. Overhead 5, gate ≤5 unchanged. Both now stand exactly at the gate. Reason: an animation is chosen on Start, because Add drawing leaves a plain drawing's band |
| "six steps" wherever it appears | Six | The bar of the project's kind: 4, 5 or 3 |

### 1.2 Fixed decisions and resolutions (main spec 1.1, 1.2)

| Item | New text |
|---|---|
| 1.1(5) | Saved vocabulary stands. `.animproj` stays schema 11. Two additive app keys are the only new stored data: `app.kind`, `app.lesson`. The sample writes neither and stays byte-stable |
| R2 Step names | Plan, Draw, Timing, Color, Effects, Manga, Export, by bar. Plan is `Stage::Drawing` with the sheet. Its hover card carries the word genga. The Layout recipe is no step |
| R4 | `Stage::Export` stands, last on every bar |
| R12 First run | Ready paper is an unstamped drawing, not the Animation intent. The rest stands. See 2.5 and 4 |
| R13 Step memory | Stands, with one exception, Plan's hidden sheet (3.10) |
| R16 Strip visibility | Stands. Its consequence is now stated: an animation shows the sheet in Plan and Timing and the strip in Color, Effects and Export. Nobody sees both |
| R17 Growing chrome | `strip_open` opens only for an animation. It never opens on a drawing. `Add drawing` on the band and the full-band latch stand for an animation |
| R18 Upgraders | An animation opens on Plan with its sheet. A plain drawing has no sheet button. What's New says so once |
| R20 | Twenty-eight parts. See 1.12 |
| R21 Motion | No chrome motion is added. Show me's reveal draws on the paper, only when pressed, and pen down cancels it. The card's Deckle changes pose per tick as a state change, not a tween |

### 1.3 Data rules (main spec 1.4)

| Row | New text |
|---|---|
| `.animproj` | Add `app.kind` (`drawing`, `animation`, `comic`) and `app.lesson` (`<id>/<task>`) through `project.app_meta` |
| `ui` | Unchanged in shape. `panel_more`, already listed as additive, lands early in the Plan part with one key, `plan_sheet_hidden`. `sheet_in_draw` keeps its field and is read only on the frozen boot |
| New file | `lessons.txt` in `config::data_dir()`: one finished lesson id per line. Not in `ui`, not in any project. A missing or unreadable file means nothing is ticked |

### 1.4 Layout (main spec 4)

| Section | New text |
|---|---|
| 4.2 Top row | One pill per row of `bar(kind)`. `steps_width`, `island_x`, `narrow` and `place_width` read the shorter list |
| 4.3 last line | "The rail is hidden in 3D and in Brush maker." Plan's Preview is gone. Tightest rooms add Plan at an effective 832×520 |
| 4.4 Band | See the table below. `Timing sheet` stands in Plan as show and hide, stays absent in Timing, and is absent on a drawing. `Longer` and `Shorter` never fold |
| 4.5 Start row | Start, 600×≤300, centred on the dock rect. Opening Layers closes it, and so does opening any left float that intersects it. Lessons card: `Panel::right`, no float |
| 4.8 | Stands. "Paper under chrome" is now also gated in Plan and in every step of a lesson |
| 4.9 | Plan row: left 349, canvas 1245×786, paper 1132×637 (50.0%), and left 279, canvas 995×686, paper 890×500 (43.5%), band 36. The "Draw, sheet on" row goes: it was never gated and it overstated (49.8% in the shot) |

| Slot | Plan | Timing | Color, Effects, Export (animation) | Drawing, Comic |
|---|---|---|---|---|
| Left | Previous, Play, Next, `Frame 3 of 12` | Full transport, as today | Plain transport | Plain transport |
| Centre | Say line | Say line | Say line | Say line |
| Right | Add drawing, Ghost frames, Speed, Timing sheet | Longer, Shorter, Sound while dragging, fps | Add drawing, Ghost frames, Speed, Timing sheet | Nothing |

The last column gained Comic in the AU18 review: the table had no Comic column, and a page offering `Add drawing` and `Timing sheet` beside `Comic pages have no timing.` read as an animation tool leaking in. Alt+A and `Window ▸ Timing sheet` stay on both.

### 1.5 The steps (main spec 5)

| Section | New text |
|---|---|
| 5.1 | Replaced by section 2 here |
| 5.2 Manga row | Manga is on the Comic bar. In a drawing or an animation it stays off the spine, as today |
| 5.3 recipe | See 3.2. The **Plan** paragraph goes. No stroke is refused in Plan |
| 5.4 | Plan's sheet starts on for every animation. Hiding it is remembered in `ui.panel_more` (3.10). `ui.sheet_in_draw` no longer drives a shipped recipe |

### 1.6 Keys, menus, Actions (main spec 6)

| Section | New text |
|---|---|
| 6.2 | `More ▸ Steps ▸` shows the bar of the open project. Help gains `Lessons`, so first-level rows go from 59 to 60, inside the gate of ≤60 |
| 6.3 | One new Action, `OpenLessons`, no default chord, `Help ▸ Lessons` and palette. `Action::ALL` tests move by one, with this reason |
| 6.4 | F4 runs the first step of the kind and is never refused. See 2.8 |

### 1.7 Timeline (main spec 7.2)

Add drawing takes blank frames before it adds any (3.8). Play and a plain export stop at the last frame that shows a drawing (3.8). Longer and Shorter are band keys in Timing (3.9).

### 1.8 Learning (main spec 8)

| Section | New text |
|---|---|
| 8.1 Ready paper | Unstamped, 1280×720, 12 fps, one frame, track `A`, strip closed, `Stage::Drawing`, bar Draw, Color, Effects, Export. A first stroke keeps it a drawing |
| 8.1 Start float | Section 4 here replaces geometry, entries and budget. `Open the example` moves to the Lessons page once lessons stand |
| 8.2 Say rows | The Plan row goes. New row `Click a row, then draw on it.` on bit 23, Plan with the sheet on, once. New remark `Two drawings make an animation. Plan and Timing are here now.` on bit 24, once. Bits 5 and 15 stay reserved |
| 8.3 Quick Start | Per kind, see 2.10 |
| 8.4 | `Plan preview` row goes. `Go to Draw` becomes `Go to Plan`, `Go to Draw` or `Go to Manga` by kind, always the first step of the bar. `Timing, under 2 drawings` stands, for an animation only |
| 8.5 | Lessons work with Hints off. Turning Hints off does not touch a lesson |

### 1.9 Journeys (main spec 9)

| Journey | Change |
|---|---|
| J1 | Starts by pressing `An animation`, lands on Plan with the sheet, `Add drawing` is on Plan's band. 28 actions (1, then the 12 strokes, 11 Add drawings, Play and 3 of Export that `auexport_ball_journey` already scripts), 5 overhead, gates ≤28, ≤5, ≤3, ≤2 unchanged. Twelve drawings on 12 rows: a real 12-frame GIF of 1.0 s, which needs 3.8, because each Add drawing must take a blank row and not lengthen the cut |
| J2 | `A drawing`. Unchanged. The bar has no Plan or Timing |
| J3, J4 | Unchanged. Camera move is shown only on an animation |
| J5 | Room chords: 5 of 5 on an animation, 4 of 4 on a drawing, 3 of 3 on a comic. On an animation F4 and F5 land on one pill |

### 1.10 Gate ledger (main spec 10)

**No threshold loosens.** Rows that move:

| Gate | Old | New | Part |
|---|---|---|---|
| Room chords, Step keys | 6 of 6 | Every step of every bar has its key: 5, 4, 3 | Project kinds |
| Paper uncovered with Start at its largest | ≥58.0% at 1600×900 | ≥58.0% at 1600×900, unchanged. If the first measure misses, stop and report. Do not re-base | Central Start |
| Crowded blank room budgets | Plan about 39 | The ceiling ≤45 stands. "About 39" was the old Plan. The first measured value is reported | Panels |
| Clip audits | `PROBE_ROOMS` | Gains Plan on an animation with the sheet on, the lesson card open in every step, and the Lessons page. A `Stage::Layout` row now lands on Plan, so it goes rather than count Plan twice | Panels, Plan, Central Start, Lesson frame |

New rows:

| Gate | Target | Measured by | Part |
|---|---|---|---|
| Paper under chrome in Plan | 0, 4 sizes × 3 scales, 4 aspects | `measure::paper_share` | Plan |
| Plan paper share, 16:9 at Fit | ≥49.5% at 1600×900, ≥43.0% at 1280×800. Raise to the first measured value less 0.5 | `measure::paper_share` | Plan |
| Plan whole sheet rows | ≥32 at 1600×900, ≥27 at 1280×800 | `auplan_sheet_rows` | Plan |
| Lessons page | ≤12 controls, ≤48 words, grid unscrolled | Census | Central Start |
| Paper under chrome, lesson card | 0, every task of every lesson's paper, 4 sizes × 3 scales | `aulesson_card_covers_no_paper` | Lesson frame |
| Every lesson, every task, to a real file | 9 of 9 | `auguide_every_lesson_ends_in_its_file` | Guide and Show me, then each lesson part |
| Every stroke task ticks from a pen route | 100% | `auguide_pen_ticks_every_stroke_task` | Guide and Show me |
| Ring on its real control | Every task, every size, Pocket tier included, and every target of a chain reached by opening what the ring stands on | `auguide_ring_stands_on_its_control`, `auguide_ring_reaches_the_last_target` | Handles and ring |
| Guide reaches no file | 0 guide pixels, 0 saved keys | `auguide_guide_never_exports` | Guide and Show me |
| Lessons walk their bars | 9 of 9, every pill pressed, bar order | `aulesson_tasks_walk_the_bar` | Lesson frame |

### 1.11 The remaining parts

| Part | Change |
|---|---|
| Keys and Hide tools | Runs as planned on six steps, because it was already running (1.12). Three things it cannot do yet move to the part that makes them possible. Project kinds moves `aukeys_six_steps_have_keys` to `aukeys_each_bar_has_its_keys`. Plan makes `rail_reserve` drop while tools are hidden. Lesson frame refuses `T` while a lesson stands, in the words `Leave the lesson to hide the tools.`, tested by `aukeys_hide_tools_waits_for_a_lesson`, and `open_lesson` shows hidden tools (5.3). F-key defaults stand. `migrate_keymap` still sets `hints_seen` to all ones: an upgrader's new rows start ticked, which is accepted |
| Menus | `More ▸ Steps ▸` reads the bar. `Help ▸ Lessons`. `BrushMakerRoom` as specified: the Plate One lesson needs it. Every menu word and submenu head keeps its `menu.*` mark (5.5) wherever this part moves it |
| Plain words | Runs over every new string. `vocab::LONGER`, `SHORTER` get their final words here. `aulesson_no_old_words` starts to bite after this part |
| Color step | Each starter swatch registers a handle (5.5). "Armed on entry" stands |
| Effects step | Shelf entries, `Strength` and `Add effect` register handles. Camera move is hidden on a drawing and on a comic. **New:** `wire_over`, a generator with no through input laid over the feed with a Blend node in one undo, test `aufx_generator_blends_over_the_output`. Say's `effect_added` reads an effect reaching Output, not a node count |
| Panels | Runs after the Comic step, so its audit covers the lesson card, the Lessons page and the Manga pane's new controls. `panel_more` already exists. A control moved behind a fold keeps its handle |
| Alias retirement | After the last lesson part, so it sweeps every new surface and its pixel diff covers them |
| Release | What's New: replace "the Timing sheet comes back with one click" with one line on bars and one on lessons. Final tally includes every row of 1.10 |

### 1.12 Part map (main spec 14)

Twenty-eight parts. New parts run about 120 minutes each including two reviews. The last three built parts ran about 132, 75 and 137 minutes from one review commit to the next, by the commit times, with 32 to 40 tests, so a test count near 25 fits. What does not fit is three separate systems in one part, which is why the plan's first draft of AU19, AU20 and AU21 each split into two. The remaining order and why:

- **Keys and Hide tools first,** because it was already running in the tree when this was reviewed. What it would need from the bars moves to the later part (1.11).
- **Structure next** (AU18, AU19, AU26, AU20, AU27, AU21, AU28), so Menus and Words build on the three bars, not on six steps they would have to undo.
- **Then the planned parts** in their planned order (AU11 to AU14). Their lessons-facing work is small and is listed in 1.11.
- **The Comic step before Panels,** so Panels' clip audit covers every new surface: the card, the Lessons page and the Manga pane's new controls.
- **Lesson content after Plain words, Color, Effects and Panels,** because its tasks need those steps and its words must settle first.
- **Alias retirement after the last lesson,** so its sweep and pixel diff cover the new surfaces. **Release stays last.**

| Part | Title | The Lead sees | Prefix | Min tests | Depends on |
|---|---|---|---:|---|---|
| AU10 | Keys and Hide tools | As specified | `aukeys_` | 14 | AU9 |
| AU18 | Project kinds and three bars | Each project has its own bar. A plain drawing has no Plan | `aukind_` | 16 | AU10 |
| AU19 | Plan step | Plan is a real drawing step with the sheet at left, paper clear of the rail | `auplan_` | 12 | AU18 |
| AU26 | Twelve rows and the blank tail | A new animation has rows to draw on. Blank frames no longer play or export. Longer and Shorter on Timing's band | `autail_` | 8 | AU19 |
| AU20 | Central Start | Start in the middle | `austart_` | 6 | AU26 |
| AU27 | Lesson frame | Nothing in the app yet. A probe lesson runs in the tests | `aulesson_` | 16 | AU20 |
| AU21 | Handles and ring | Nothing in the app yet | `auguide_` | 6 | AU27 |
| AU28 | Guide and Show me | Nothing in the app yet. A contact sheet of Deckle's poses | `auguide_` | 10 | AU21 |
| AU11 | Menus | As specified | `aumenu_` | 8 | AU28 |
| AU12 | Plain words | As specified | `auwords_` | 10 | AU11 |
| AU13 | Color step and strip pictures | As specified | `aucolor_` | 12 | AU12 |
| AU14 | Effects step | As specified | `aufx_` | 12 | AU12 |
| AU22 | Comic step | See the page, Ink and Tone, Add balloon, Add text, Speed lines, Mono on the Export step | `aupage_` | 10 | AU14, AU21 |
| AU15 | Panels | As specified | `audress_` | 8 | AU13, AU14, AU22 |
| AU23 | Rookie lessons | `Follow a lesson` on Start. Three lessons a newcomer can finish | `aurookie_` | 8 | AU15, AU22 |
| AU24 | Amateur lessons | Three more | `auamateur_` | 6 | AU23 |
| AU25 | Experienced lessons | All nine | `auveteran_` | 6 | AU24 |
| AU16 | Alias retirement | Nothing, by design | `autoken_` | 3 | AU25 |
| AU17 | Release | What's New, 0.6.0, the Lead's list | `aurel_` | 4 | AU16 |

**Cut order,** first to last: finished ticks on the grid, `Hold to see it`, the drag handle on page lettering, Hide tools, drop to open, strip pictures, `Check your pen`, folded sections on the rarest panels. A cut is named in the report. Nothing on a journey, and no step of a lesson, is cut.

### 1.13 Tests that move

Each moves with its reason. None is weakened.

| Test or group | Why |
|---|---|
| `austart_largest_float_leaves_the_paper_centre_clear` | L1. Asserts "past the centre", "12 in from right", "12 in from top" become "centred within 1 pt", "inside the dock", "clear of the rail", "≤300 tall". Test renamed `austart_largest_float_stands_centred`. **The ≥58.0 assert stays unedited** |
| `austart_stroke_at_the_paper_centre_with_the_float_open` | L1. The stroke lands on paper outside the card at every size. Renamed `austart_stroke_on_free_paper_with_the_float_open` |
| `austart_float_geometry_is_the_rule`, `austart_words_stand_on_one_edge` | L1. Rewritten to section 4, one edge per column |
| `austart_fresh_launch_opens_on_ready_paper`, `austart_intents_are_the_table` | Ready paper split from the Animation intent. Strip closed. `INTENTS[ANIMATION].cut_frames` is 12 from the blank tail part on |
| `austrip_tests.rs`, `holds(&s) == [1, 1, 1, 46]`, `frame_count() == 49` | 45 and 48. Add drawing takes the blank tail |
| `strip_up`, 45 uses | A multi-frame shipped document is now an animation. The seam: `strip_of` puts `Stage::Drawing.dock()` in place, a real state (sheet hidden by hand) |
| `audock_sheet_*`, `audock_sheet_choice_survives_a_restart` | Polarity flips: the sheet starts on and a click hides it. The restart gate keeps its shape over `plan_sheet_hidden` |
| `aurail_mirror_stands_with_the_sheet` | Exception for Plan. The mirror goes there, because the Lead's point is one timeline. Named on the end list |
| `ausay_` Plan tests, `measure.rs` preview tests | The preview is gone |
| `audock_` step order and cards, `testkit` pills, `welcome` Quick Start tests | Assert per `bar(kind)` |
| `auexport_ball_journey` | One more action, 28 at a ceiling of 28, unchanged |

**Frozen boot.** Its kind reads unset through one seam field set beside `more_default`, so `recipe` falls to `sheet_in_draw`, `full` stays true and `rail_reserve` stays false. `auseam_frozen_boot_is_todays_recipe` does not move.

## 2 Steps by project

### 2.1 Three bars

| Kind | Bar | First step |
|---|---|---|
| Drawing | Draw, Color, Effects, Export | Draw |
| Animation | Plan, Timing, Color, Effects, Export | Plan |
| Comic | Manga, Color, Export | Manga |

`steps.rs` keeps one flat catalog of rows, and `pub fn bar(kind) -> &'static [Step]` slices it. `of(stage)` searches the catalog, so a stage off a bar still finds its words.

| Row | `Stage` | Stored key | Purpose | Studio word | Action |
|---|---|---|---|---|---|
| Plan | `Drawing` | `drawing` | Draw your key poses on the frames you choose. | genga | `RoomDrawing` |
| Draw | `Drawing` | `drawing` | Draw on the paper. | none | `RoomDrawing` |
| Timing | `Edit` | `edit` | As today | henshū | `RoomEdit` |
| Color | `InkPaint` | `finishing` | As today | shiage | `RoomFinishing` |
| Effects | `Composite` | `composite` | As today | satsuei | `RoomComposite` |
| Manga | `Manga` | `manga` | The words `Stage::card()`'s None arm holds today | manga | `MangaRoom` |
| Export | `Export` | none | As today | none | `RoomExport` |

`Stage::ALL` stays five, `stage_key` stays, `Stage::name()` stays. New `Editor::step_name(stage)` gives the kind's word for `ResetLayout`, `enter_room`'s status, `place_here` and `export_view`'s way back. New `Editor::first_step()` gives the bar's first row. It replaces the bare `Stage::Drawing` in `export_view` and `stage_kept`, so an animation never reads `Back to Draw` and a comic never lands on a step its bar lacks. Trap: `Stage::card()`'s None arm returns the manga row, so Layout must find the catalog, not fall through. `place_name` returns None for Manga in a comic, because Manga is on its bar.

### 2.2 ProjectKind

```rust
// app/src/steps.rs. anim-core holds no app types and never reads app_meta.
pub enum ProjectKind { Drawing, Animation, Comic }
impl ProjectKind {
    pub fn of(p: &Project) -> ProjectKind   // stored wins, else derive
    fn derive(p: &Project) -> ProjectKind
}
```

Nothing caches the kind in `Editor`, because `Leave::Sample`, `OpenPath`, `Open` and `Recover` replace `self.state` without a boot. The project is the one truth. One seam field for the frozen boot reads "unset" and stores no kind. Under that seam the door (2.6) and promotion (2.7) do nothing.

### 2.3 Storage

`project.app_meta["kind"]`, on disk `app.kind`, values `drawing`, `animation`, `comic`. An unknown value derives. Engine Undo never touches it. `store::save`'s full rewrite carries it. Builds that know `app_meta` round-trip it. Builds older than `app_meta` drop it on re-save, which the comment in `store::save` already accepts. No schema bump, no wire change, no `MIN_PEER_VERSION` bump.

### 2.4 Derivation

Every cut has `PAGE_KEY`: Comic. Otherwise more than one cut, or any cut over one frame: Animation. Otherwise Drawing. "Every", not "any": an old animation with one page among its cuts must keep Plan and Timing, or it would open on a bar that refuses its own work with no Kind row to fix it. Use `cut.meta.contains_key(PAGE_KEY)`, not `Page::of`, which parses JSON and this runs per frame.

### 2.5 How each route sets it

| Route | Kind |
|---|---|
| Start `An animation` | Animation, 12 frames, stamped in `Template::build` before `saved_rev`, so a fresh blank is not dirty |
| Start `A drawing`, `Square`, `Tall` | Drawing |
| Start `A comic`, Manga B5, A4, Webtoon | Comic |
| TV anime, Web 720p, Feature UHD, Animatic | Animation. Animatic still opens on Timing (`room: Stage::Edit`) |
| **Ready paper** | New const `templates::READY`: 1280×720, 12 fps, 1 frame, track `A`, no stamp, strip closed. `READY.build()` holds no key. Derives Drawing, and the door stamps that in memory |
| New form, blank | Derives Animation (`new_project` makes 48 frames) |
| Sample | `build_sample()` writes no key, so its built bytes stay. Derives Animation, and the door stamps that in memory |
| Open an old file | Derives |
| A lesson | `Template.kind` of its paper |

`Template` gains `kind`. `open_intent` reads `READY`. `enter_intent` keeps `strip_open = i == ANIMATION`.

### 2.6 One door

The opened-document block in `Editor::ui` (`opened_doc`, `untitled_group`) is the one point every route passes. There:

1. Insert the derived kind into `app_meta` **without** `meta_dirty`. An old file does not turn dirty on open, and a derived kind cannot flip mid-session. It is written at the next real save.
2. If the current stage is not on the bar, go to the bar's first step. `Stage::Drawing` is on two bars with two recipes, and the block replaces the state in the same Editor, so it does not change the dock. So when the stage is `Stage::Drawing` and the dock's sheet disagrees with `recipe`, it runs `enter_room(Stage::Drawing)` as well. Otherwise a drawing opened from Plan keeps a sheet it has no button for, and an animation opened from Draw loses R18. A place off the spine stays where it is.
3. A comic goes to `Stage::Manga`.
4. `export_from` is rewritten to `(first_step(), recipe(first_step()))` if the document was replaced while on Export, so the way back and its dock are this document's. This is what `export_view`'s comment asks for and today's stored dock does not give.
5. Read `app.lesson` (5.11).

### 2.7 Transitions

**Drawing to Animation or Comic.** One per-frame rule beside the `full_band` latch:

```
if kind == Drawing and no lesson stands:
    any cut has PAGE_KEY                 -> store "comic"
    any cut over one frame, or cuts > 1  -> store "animation"
```

It sets `meta_dirty` and says its remark once. Frames, not drawings: two tracks on one frame are still a picture. It catches Add drawing, Insert frames, cut length, paste, a new cut and a guest's edit with no wire message. The page check comes first, or a New page on a page-sized drawing would promote to Animation by cut count.

**Paste.** `sheet_ops::paste_block` never changes `frame_count`, so `AppState::paste_cells` does it, on a stamped drawing alone: a paste that runs past the one frame, Insert always, adds `SetFrameCount` to its own undo step. The cells are frames then, and the rule catches them. An animation's cells past the end stay as they are, and the frozen boot, which stamps nothing, is untouched.

**What the person sees.** The pill reads Plan instead of Draw and Timing appears. The stage stays `Stage::Drawing`, so nothing moves under the pen: no dock swap, no refit. The strip opens as `AddDrawingAfter` already makes it. The sheet arrives the next time they enter Plan or press `Timing sheet`.

**A drawing that becomes a comic moves.** The Comic bar has neither Draw nor Effects, so staying put would leave no pill lit and no pill for the step they stand on. When the stage is not on the Comic bar, the promotion runs `enter_room(Stage::Manga)`, after any stroke in progress has ended.

**One-way.** An animation cut to one frame stays an animation. Undo of the Add drawing does not demote, because a bar that changes on Ctrl+Z is worse than one extra pill. An animation adding a page stays an animation. A comic with frames stays a comic.

### 2.8 Hidden step keys

| Action | Drawing | Animation | Comic |
|---|---|---|---|
| `RoomLayout` (F4) | Draw | Plan | Manga |
| `RoomDrawing` (F5) | Draw | Plan | Manga |
| `RoomEdit` (F6) | Refused: `A single picture has no timing.` | Timing | Refused: `Comic pages have no timing.` |
| `RoomFinishing` (F7) | Color | Color | Color |
| `RoomComposite` (F8) | Effects | Effects | Refused: `Comic pages get their look from tones on the Manga step.` |
| `RoomExport` (F9) | Export | Export | Export |
| `MangaRoom` | Manga, off the spine | Manga, off the spine | Manga |

The two studies differed on F4. It runs the first step and is never refused: a person pressing F4 in a drawing wants the start of their work, and Draw is that. F4 and F5 are not refusals, so `hidden_step` does not carry them. One redirect at the head of `enter_room` does: `Stage::Layout` becomes `Stage::Drawing`, and on a comic `Stage::Drawing` becomes `Stage::Manga`. That same line serves `leave_export`'s `None` arm, `ResetLayout`'s `(None, None)` arm and `demo_circle`'s fallback, which all name `Stage::Drawing` today and must not put a comic on a step its bar lacks. `App::perform` calls `Editor::perform` directly, so chords skip `can_perform`. The rule goes in both, one sentence from one function, `Editor::hidden_step(action) -> Option<&'static str>`. `state.refuse` turns it into `Performed::Ignored`. Menus grey with the reason, the palette dims the row and keeps it (`rows.len() == Action::ALL.len()` stands). `Timing sheet` stays in Window and View for every kind.

### 2.9 What else reads the kind

- `recipe` (3.2), `canvas_shown`, `viewers_open`, `sheet_button`, `band_right`, `strip_room` as it stands.
- `locked_panes`: its cache key is `(stage, sheet_in_draw)` and it reads `recipe`, so the key gains the kind and `plan_sheet_hidden`, or Plan's panes stay held against the wrong recipe after a document changes. `switch_sheet` tests `same_shape` against `Stage::Drawing.dock()` and `dock_with_sheet()`, and writes `sheet_in_draw`. In Plan it compares against `Stage::Layout.dock()` and writes `plan_sheet_hidden`.
- `band_right`: a Drawing drops `Add drawing` and `Timing sheet`. Alt+A and the menu remain, which is how a person promotes on purpose.
- `Verb::GoToDraw`: label by kind, runs `RoomDrawing`. `demo_circle`'s fallback enters the kind's drawing step. `Template::tooltip` reads by kind. The Help window's numbered lines read by kind.
- `satsuei_refusal` also refuses on a page, because `Set up standard composite` would write over the page's graph.

### 2.10 Quick Start per kind

`quick_start: [bool; 5]`, the dots and `EXPORT_STEP = 4` stay. `ui.quick_start` is one array, so only slots 0 and 4 share a meaning. Drawing and Comic slots 1 to 3 use free `hints_seen` bits 17 to 22. The dots paint on a drawing and on a comic, in their own rows: the main spec hid them on a closed strip so a Picture start was not counted through an animation, and these rows remove that reason. An animation with its strip closed still hides them.

| Slot | Animation | Drawing | Comic |
|---|---|---|---|
| 0 | Draw something | Draw something | Draw on the page |
| 1 | Add a drawing | Color it (`filled`, Show me enters Color) | Cut it into panels (a panel-count Fact, Show me arms the Panel tool) |
| 2 | Draw the next pose | Add an effect (`effect_added`, Show me enters Effects) | Add a page (`NewPage`) |
| 3 | Play it | Save it (`Save`) | Save it |
| 4 | Export a file | Export a file | Export pages |

`Facts.kind` and a `kinds` filter on `Hint`: `ADD_DRAWING`, `NEXT_POSE`, `TWO_DRAWINGS`, `PLAY`, `APPEARS_TWICE`, `HELD`, `EMPTY_CELL` are animation only. `SHARE` must be reachable without `PLAY`. `ausay_hints_never_point_backward` runs over `bar(kind)`. Turning hints on clears `hints_seen` but not `quick_start`, so Drawing and Comic rows reset and Animation's do not. Accepted.

## 3 Plan

### 3.1 What it is

Plan is `Stage::Drawing` on an animation: stored key `drawing`, onion on, stack `Solo merged`, label Plan, hover card "genga". A click on a row and a stroke on the paper are the whole workflow. The stack armed changes from `Layout (LO)` to `Solo merged`, a behaviour change for anyone used to Plan's layers. `Layout (LO)` stays pickable from the Layers `stack` button.

`Stage::Layout`, `Stage::ALL`, `stage_key`, `Action::RoomLayout`, `default_room_stacks` stay, and `self.stage == Some(Stage::Layout)` is never true: `enter_room` redirects at the door, so `ProbeRoom` and tests cannot keep a preview alive. `.view()`, `.card()`, `.name()` stay for tests.

### 3.2 Recipe

```rust
// workspace.rs, Stage::dock. Layout's arm becomes Plan's one source.
Stage::Layout => {
    let mut ds = DockState::new(vec![Pane::Canvas]);
    ds.main_surface_mut().split_left(NodeIndex::root(), 0.22, vec![Pane::XSheet]);
    ds
}
// main.rs, Editor::recipe
fn recipe(&self, st: Stage) -> DockState<Pane> {
    match st {
        Stage::Drawing if self.is_animation() && !self.plan_sheet_hidden => Stage::Layout.dock(),
        Stage::Drawing if self.frozen_kind && self.sheet_in_draw => Stage::dock_with_sheet(),
        Stage::InkPaint if self.result_in_color => st.dock_with_result(),
        _ => st.dock(),
    }
}
```

`is_animation()` and `in_plan()` are new helpers. A saved `sheet_in_draw` does nothing on a drawing, which has no button to undo it. In `recipe`, `plan_sheet_hidden` reads false while a lesson stands (3.10).

### 3.3 Panes

| Pane | Decision | Why |
|---|---|---|
| Canvas | Live | The step's purpose |
| X-Sheet | Whole left column | One timeline, about 33 rows |
| Cel Layers | Leaves the dock | The rail's Layers key opens the same strip as a float |
| Node Graph | Leaves | It belongs to Effects. The Window menu still adds it |
| Presets, Forge | Not added | Brushes float, Brush maker place |

### 3.4 Regions, 100% scale, 16:9 document at Fit

| Region | 1600×900 | 1280×800 |
|---|---|---|
| Top row | y 0–44, five pills | same |
| Left leaf | x 0–349 | x 0–279 |
| Canvas pane | x 355–1600, y 78–864, 1245×786 | x 285–1280, y 78–764, 995×686 |
| Band | y 864–900, 36, no strip | y 764–800, 36 |
| Rail | Full tier, x 367–423 | Full tier, x 297–353 |
| Paper at Fit | 1132×637, left 431, gap to rail 8, right margin 37 | 890×500, left 361, gap 8, right margin 29 |
| Paper share | 50.0% | 43.5% |
| Paper under chrome | 0 | 0 |
| Whole sheet rows | about 33 | about 28 |

Other shapes, share at 1600×900, 1280×800, 1280×720, 1024×640: 16:9 50.0, 43.5, 48.3, 41.6. 4:3 50.5, 54.1, 46.9, 49.7. 1:1 37.9, 40.6, 35.2, 37.3. 9:16 21.3, 22.8, 19.8, 21.0.

The canvas keeps its 34 pt tab bar. `tab_style_override` with `Margin::ZERO` for `Pane::Canvas` in this recipe returns 12 wide and 12 tall.

### 3.5 Fit clears the rail

```rust
// CanvasView::ui, the Fit block. The host sets rail_reserve each frame.
let fit = if self.rail_reserve {
    ((rect.width() * 0.97 - crate::floats::LEFT_X) / paper_w)
        .min(rect.height() * 0.94 / paper_h).max(0.01)
} else {
    clear.clamp(strip_open, fit_in(rect.height())).max(0.01) // today's, untouched
};
self.last_fit = fit;
let cx = rect.center().x.max(rect.left() + crate::floats::LEFT_X + fit * paper_w / 2.0);
self.xf.center = pos2(cx, rect.center().y);
```

`rail_reserve = (sheet on screen or a lesson stands) and the rail shows and tools are not hidden`. The hand-turn pivot, `let c = rect.center()` in the Space hand block, moves to `self.xf.center`, and `turned_fit` fits the reduced width. Today's Fit for an open canvas does not move: L4.

### 3.6 Band

```rust
let rows = self.sheet_on_screen();      // the frames are all on screen
let full = rows && !self.in_plan();     // Plan keeps the drawing keys
let arrived = rows || self.full_band;   // Previous and Next from the first frame
```

The Plan column of 1.4 is the result. `sheet_button` adds `in_plan()`. The 12 pt mirror goes in Plan. Loop keys stay in Timing and on `Animation ▸ Play`. Say's new row is `Click a row, then draw on it.`

### 3.7 Twelve rows

`INTENTS[ANIMATION].cut_frames` is 12, one second at 12 fps, because rows stop at `frame_count` and a key pose needs a row to land on. Other intents stay at 1. Cell selection, held cells, `raster_new_cel` and `land_raster` need no new rule: drawing on a held or empty row makes a key, splits the run, and is one undo. A second click on the cursor's own cell focuses `entry_field`, which is why lesson text says click once. `entry_field` takes a drawing's name, and `next_drawing_name` makes names like `D1A`, so a bare `1` makes a new blank drawing called `1` through `expose_by_name`. Lesson text never tells anyone to type a number.

`show_sheet` calls `open_pane` in an animation, so reopening a closed sheet saves nothing that means nothing. The `RoomLayout` help text is rewritten. Palette aliases add `genga` and keep `lo`, `layout`.

### 3.8 Add drawing and the blank tail

The twelve rows create a problem no study closed. Three Add drawings on a fresh animation give `[1, 1, 1, 9 empty]`, and today Play and the GIF run through nine blank frames. `set_speed` keeps `Exposure::Empty` at full length, so Speed adds to it. Two rules close it.

**Rule one.** `add_drawing_after`: when the run after the cursor's run is `Exposure::Empty`, the new drawing takes its frames before any are inserted. If the empty run has more frames than the new hold, a new `Exposure::Empty` key stands after it. If fewer, it takes them all and `InsertFrames` adds the rest. `hold_to_copy` is untouched.

**Rule two.** New `AppState::shown_length()`: one past the last frame on which any rendering track shows a drawing, at least 1, at most `frame_count()`. A track's last key being `Exposure::Empty` ends it there.

- `play_range`: a loop range inside the cut wins, as today. Otherwise `(0, shown_length() - 1)`.
- `run_range`: its fallback for a cursor past the range's end reads `(0, frame_count() - 1)` today, which would play the tail, so it takes `shown_length()` too. `AppState::tick`'s `frame_count()` check moves to the same end.
- `Editor::export_to` and the Export step's cards pass `export_range`, or else `(0, shown_length() - 1)`, to `render_queue::add_job`.
- The queue's own form keeps the whole cut. A deliberate blank is made there, or by any track showing art.
- The counter, the sheet and `frame_count()` do not change.

A background track that holds to the end means no tail, so a character may leave the shot and the shot goes on. This is what the End of the Desk lesson needs.

### 3.9 Longer and Shorter

`Action::HoldPlus` has no button, and it refuses on the last run with `the last drawing already holds to the end of the cut`. The sheet's edge drag belongs to the strip, which is hidden here. So Timing's band gains two keys, `vocab::LONGER` and `vocab::SHORTER`, calling `state.set_run_length(frame, len ± 1)`, the function that already grows the cut for the last run. Their hover cards name the chords. They act on the active track at the cursor. The Actions and their refusal are unchanged.

### 3.10 Sheet memory

`ui.sheet_in_draw` defaults false and upgraders hold it false, so it cannot mean "on". A new `ui` bool would change the section's shape. `ui.panel_more`, already listed in main spec 1.4 and not yet built, lands here with one key, `plan_sheet_hidden`. `Editor.plan_sheet_hidden` mirrors it through `apply_ui`, as `sheet_in_draw` does. The Panels part inherits the field.

The memory is global, so it must not reach a lesson: a learner who once hid the sheet would open Still in the Coat's first task with no row to click. While a lesson stands `recipe` reads the sheet as on, and `open_lesson` does not touch the stored value. If the learner hides the sheet during a lesson, the task's chain rings `Timing sheet` (5.5).

### 3.11 What goes with the preview

The arm in `EditorTabs::ui`, viewer `u8::MAX`, `viewer::ui`'s `plan` parameter, `ViewerView.tried`, `bare_plan`, `NOTHING_HERE`, `CanvasView.dbg_preview` and its read in `measure.rs`, `say_plan`, `plan_tried`, the `PLAN` row in `HINTS`, `say::PLAN_WORDS`, and the `Stage::Layout` clauses in `canvas_shown`, `viewers_open`, `sheet_button`. Seeing the composed cut play moves to Timing's Result. `View ▸ Preview final` stays a toggle on the live canvas. Side gain: Plan stops evaluating the graph every frame.

## 4 Central Start

### 4.1 Rule

```rust
pub fn start_place(pane: Rect) -> (Pos2, Vec2) {
    let w = (pane.width() - 2.0 * LEFT_X).clamp(240.0, 600.0);
    let h = (pane.height() - 2.0 * INSET).min(300.0);
    (pane.center() - vec2(w, h) / 2.0, vec2(w, h))
}
```

`pane` is always the dock rect, never `canvas.dbg_rect`, which removes `start_ui`'s `host` branch. The size comes from the cap, not from last frame, so `More sizes` cannot move the card: it scrolls. `auto_shrink` lets a fresh card end short of the cap, and the card stays centred on its own height.

**This departs from the engine study,** which kept the bottom edge above the paper's centre. That left a 168-tall card at the smallest size and, at 1600×900, a card about 160 pt above the middle of the window. The Lead said centred. The cost is that a pen pressed on the card itself draws nothing, and every other point of the paper still takes a first stroke.

### 4.2 Geometry at every gated size and scale

The effective scale is `min(stored, w/760, h/520)`. Dock top is 44. Band is 36 on ready paper, 92 with the strip.

| Window, stored scale | Points | Card | x | y, band 36 | y, band 92 |
|---|---|---:|---|---|---|
| 1600×900, 100 | 1600×900 | 600×300 | 500–1100 | 304–604 | 276–576 |
| 1600×900, 125 | 1280×720 | 600×300 | 340–940 | 214–514 | 186–486 |
| 1600×900, 150 | 1067×600 | 600×300 | 233–833 | 154–454 | 126–426 |
| 1280×800, 100 | 1280×800 | 600×300 | 340–940 | 254–554 | 226–526 |
| 1280×800, 125 | 1024×640 | 600×300 | 212–812 | 174–474 | 146–446 |
| 1280×800, 150 | 853×533 | 600×300 | 126–726 | 120–420 | 92–392 |
| 1280×720, 100 | 1280×720 | 600×300 | 340–940 | 214–514 | 186–486 |
| 1280×720, 125 | 1024×576 | 600×300 | 212–812 | 142–442 | 114–414 |
| 1280×720, 150 | 924×520 | 600×300 | 162–762 | 114–414 | 86–386 |
| 1024×640, 100 | 1024×640 | 600×300 | 212–812 | 174–474 | 146–446 |
| 1024×640, 125 or 150 | 832×520 | 600×300 | 116–716 | 114–414 | 86–386 |
| 1024×768, 200, off the table | 760×570 | 600×300 | 80–680 | 139–439 | 111–411 |

Stored scales above the gated three clip nothing, because the same `min` caps them: 200 reads 925×520 at 1600×900, 832×520 at 1280×800 and at 1024×640. The left edge is never under 116 on a gated row, and never under 80 anywhere, because `effective_scale` keeps the width at 760 or more. The rail (right edge 68) is clear everywhere. Paper left free at the worst gated size, 735×413 ready paper at 832×520: 56 above, 56 below, 67 at each side. At 1600×900 the card covers 17% of the paper.

**New closing rule.** A left float spans x 76 to 356, so it can overlap the card on a dock under 1312 wide. Opening one that intersects the card closes Start, as Layers does.

### 4.3 Body

| Row | Height | Contents |
|---|---|---|
| Heading | outside the scroll | `Start`, `×` |
| 1 | 44 | `A drawing` (with `Square`, `Tall`), `An animation`, `A comic`, three across, about 188 wide each |
| 2 | 44 | `Follow a lesson`, full width. Until `lessons::ALL` holds a lesson, `Open the example` stands here |
| 3 | 32 | `Open a project`, `More sizes` |
| Recent | 28 each | As today |
| Foot | 32, 24 | `Check your pen`, `What's new`, then `Show at startup` |

If `A drawing` and its two chips miss one 188-wide card, the chips move under the card.

### 4.4 Budget

| Page | Controls | Words |
|---|---|---|
| Start, fresh | 12 | 25 |
| Start, worst case | ≤14 | ≤30 |
| Lessons page | ≤12 | ≤48 |
| First screen with Start open | ≤44 | ≤70 |

`Follow a lesson` takes the place of `Open the example`, one control and three words for one control and three words, so the old ceilings hold unchanged. A lesson's guide stands on the paper only after Start closes, so Start never covers a Rookie's first line.

### 4.5 Lessons in one click

`Follow a lesson` opens `WelcomeTab::Lessons` in the same card at the same size, built like What's New. `Head.page = w.tab as usize` already gives it its own scroll.

| Row | Height | Contents |
|---|---|---|
| Heading | outside the scroll | `Lessons`, `×` |
| A | 32 | `Back to Start`, `Open the example` |
| B | 16 | `Rookie`, `Amateur`, `Experienced` over the columns |
| C, D, E | 60 each, gap 6 | One row per type, three cards, each a vector thumbnail from the lesson's art and its title, with `A drawing`, `An animation`, `A comic` in a 72-wide gutter |

That is 12 controls and 38 words, and it stands unscrolled in 300. Hover card: one sentence, minutes, and what file it ends in. A finished lesson shows a small tick from `lessons.txt`. From anywhere else: `Help ▸ Lessons`, or the palette.

A click sends `WelcomeAction::Lesson(i)`, listed in `replaces_document`, so the Leave guard, the guest refusal and an untouched blank's silence all come free.

### 4.6 What it keeps

Non-modal `Area`, `Order::Middle`. Shortcuts live. 0 clicks to a first stroke on free paper, which closes Start and keeps the stroke. Esc closes. Never opens during a stroke. Choices run behind the Leave guard, then `apply_start`. Heading outside the scroll. `austart_float_blocks_only_its_own_rect` stands.

## 5 Lessons

### 5.1 The subject

**Deckle.** Every sheet of animation paper has a punched strip along one edge. One night a strip tears off and gets away. It moves the only way a strip can: it arches, then flattens. Rookie lessons star the strip. Amateur lessons follow it as it wraps itself in a coat of pencil shavings. Experienced lessons are about the moth it becomes, which the learner designs. Hover cards may carry the Japanese behind it: an inchworm is *shakutori-mushi*, the shaku taker, and shaku is this app's own word for a cut's length. A Rookie just sees a funny strip of paper.

**Rules of the world.** An off-white strip with one soft torn edge, one eye and two holes. No mouth, arms or legs. It never speaks, in the art or to the user. It appears on the paper and on the lesson card, never in the chrome. Only Deckle is alive. It is never green, never sings, never measures anything in the plot, and meets no birds. The word "inchworm" stays out of titles. The moth appears only at Experienced, as the learner's own design, never a guided cute moth and with no symmetry trick. Comics are silent until the one word in the course, `tink`.

**Palette.** Night, a `plate` constant `LESSON_NIGHT` (`#141B34`), offered as one chip on the card. White, yellow (`#F2C94C`), brown (`#8B5E3C`), red (`#D93A2B`) and black are starter swatches. Glow's threshold is 0.7 of luma, so white and yellow shine and red and brown do not, and every Glow lesson has a dark ground.

### 5.2 The class rule

What stands on the paper decides the class.

| | Rookie: strip | Amateur: coat | Experienced: moth |
|---|---|---|---|
| On the paper | Every line, as a blue guide | Landmarks only. `Hold to see it` shows the finish full size while the button is held | Nothing |
| A task is | One act, one sentence | One intent, naming the concept | A goal with a checked outcome. The route is theirs |
| Button | `Show me`: does it for real | `Show me`: does the controls, never draws their art | `Show me where`: rings the control |
| Ring | From the task's start | From the task's start | Only when pressed |
| Can fail? | No. Show me alone reaches a file | Needs their art | Needs their art and the outcome |
| Words | No frame, layer, key, hold, loop, onion, cel, node, composite, exposure, fps outside a control's own name | Plain words, concepts named | Plain words, chords from `chord_label` |

Across a type the three classes share no art. Across types no lesson shares a path. Rookie animation adds drawings one after another, Amateur animation clicks rows and plants keys, Experienced animation uses keys, a spacing chart and in-betweens, which is the trade's own order.

### 5.3 Engine

New `app/src/lesson.rs` (engine, card) and `app/src/lessons.rs` (tables). Lesson papers are new `Template` consts in `lessons.rs`, outside `ALL` and `INTENTS`, so `tidy_sizes_come_from_the_templates` and `austart_intents_are_the_table` do not move for them.

```rust
pub struct Lesson {
    pub id: &'static str,           // stored in app.lesson, never renumbered
    pub kind: ProjectKind, pub class: Class, pub title: &'static str,
    pub hover: &'static str,
    pub paper: &'static Template,   // 4:3, square or page: they lose least to the card
    pub prep: Option<fn(&mut AppState)>, // given art, then clear_history
    pub fill_gap: Option<u32>,      // CanvasView.fill_gap's type; that field becomes pub(crate)
    pub ink: Option<[u8; 4]>,       // canvas.brush_color for the run, restored on leaving
    pub art: fn() -> Vec<Path>,     // guide, landmarks, stand-ins, seeds, finish picture
    pub finish: &'static [u16],     // paths of the card's picture
    pub tasks: &'static [Task],
    pub closing: &'static str,      // names what the person now knows
}
pub struct Task {
    pub say: &'static str,          // {Word} tokens, see below
    pub why: Option<&'static str>,  // required on every OnStep task
    pub step: Stage,
    pub done: Done,
    pub ring: &'static [Target],    // ancestor first, the control last
    pub guide: &'static [u16],      // paths shown during this task
    pub stand_in: &'static [u16],   // what the gate does for a person's own art: strokes or fill seeds
    pub miss: Option<&'static str>, // the act landed, the outcome did not (a leak); replaces `say`
    pub show: Show,
}
pub enum Show { Trace(&'static [u16]), Perform(Action), Run(fn(&mut Editor)), Fill([f32; 2]), Where }
```

**Words.** `say` holds `{Token}`s resolved at run time from `vocab` and `chord_label`, so no lesson string holds a literal control name. `aulesson_tokens_resolve` proves each one. Lesson text never uses `Action::label()`.

**Install.** `open_lesson` does `paper.build()`, runs `prep`, seeds `Editor.lesson: Option<LessonRun>`, writes `app.lesson` without `meta_dirty`, and sets `fill_gap`. It goes through `apply_start`. Three states left over from free work would break a task, so it also clears them. `tools_hidden` goes false, or no rail key has a rect. On a machine with the paint engine it arms the raster brush, because `rail::keys_of` shows only Size and Color while a vector brush is armed, so `Rail(Fill)` and `Rail(Eraser)` would have no rect and Fill would refuse. The Rig never sees this, because `raster_available` is false there, so only the ignored `with_gpu` mode can prove it. `plan_sheet_hidden` reads false for the run (3.10).

### 5.4 What ticks a task

`Done` is read each frame on the current task only, right after `say_facts`, by one reader, `lesson_read(&facts)`. It reads **state, never Actions or clicks**, against a baseline taken when the task began. A learner who is ahead sails through. Ticks never un-tick. A stroke elsewhere stays, and no sentence scolds it.

| `Done` | Reads |
|---|---|
| `OnStep(Stage)` | `self.stage` |
| `Ink(&[u16])` | Every listed path was met by its own stroke landed since the task began: its box, grown by 8% of the paper, holds that stroke's box, the stroke spans at least half the path's longest side, and no stroke answers two paths. So an outline cannot tick the dab of its eye, a dab cannot tick an outline, and `strokes_done` rose by at least the list's length. New `canvas.last_stroke_box`, set in `finish_stroke` and `commit_path`, so vector and raster routes both count |
| `Strokes(n)`, `KeyArt { track, frame }`, `InkOnLayer`, `InkInPanels(n)` | `strokes_done`, the own key there has art, the armed layer, the panels met |
| `BrushChanged`, `Armed(preset)`, `Smoothing`, `Tool(CanvasTool)` | Canvas state against baseline |
| `Undone`, `Erased(path)` | `history_len` fell after a stroke; the inked pixels inside the path's box fell to 10% of what the task began with, read from the cel's tiles, so a stroke that misses the smudge does not tick |
| `Filled { seed, keep }` | `fills_done` rose, the seed's pixel changed, `keep`'s pixel did not |
| `Fills(n)`, `FilledOn(n)` | `fills_done`; drawings with pixels on the color layer |
| `Drawings(n)`, `Frame(range)`, `Played`, `Speed(n)`, `Hold { frame, len }`, `Shows { frame, drawing }`, `Kinds(n)`, `Loop` | `cut.drawings.len()`, `view.frame`, a pass that began after the task did and reached the last frame of `play_range`, runs of the active track, cell contents, drawing kinds, `view.loop_playback` and the range. `Played` is not `view.playing`: pressing Play and stopping at once has not shown anyone the wait. `Shows`' `drawing` is an index into the track's keys, because drawing names are made at run time |
| `Layers(n)`, `LayerHidden`, `LayerRole(role)` | The drawing's layers |
| `Moved` | New counter raised in `commit_floating` |
| `Mirrored` | `view_mirrored()` was on, is off |
| `Effect(kind)`, `AnyEffect`, `Strength`, `CameraKeys(n)`, `Eased`, `Particles` | The graph reaches Output; the shelf's parameter moved; a param column has n keys; a camera key's `ParamInterp` or handles are no longer the default |
| `Panels(n)`, `Pages(n)`, `Gutters`, `Rulers(n)`, `Tone`, `Generator(kind)`, `Words`, `PageSeen` | `panels_of`, page count, page params, `Rulers::of`, `Tone` params, the page graph. `PageSeen` passes with no GPU |
| `Lipped`, `Inbetweens`, `Chart` | Cells on the track, drawings grew and have art, the chart exists |
| `Exported(n)` | `facts.exported` rose n times since the task began, with this lesson's latch state (transparent, mono). A landed file is the proof, so a cancelled dialog does not tick. Desk to Lamp's two exports each read their own task's baseline |
| `Preset(bank)` | A new preset in the bank |
| `Picked(rgb)` | `canvas.brush_color`, within 8 levels a channel |
| `Locked(bool)` | The armed layer's `Keep inside strokes` state |
| `PanelPicked(n)` | `state.manga.selected` |
| `All(&[Done])` | Every listed state holds, read together. A goal with two checks needs no second reader |

`render_queue.landed` keeps one consumer, `say_landed`. `doc.rs`'s `pages_written` already feeds `landed` at f3bb1dc, so a comic's finish needs no new counter. `aupage_export_reaches_the_done_card` holds it there.

### 5.5 Handles

Lessons find controls by typed handle, never by text.

```rust
pub enum Target {
    Action(Action), Rail(rail::Key), Float(Float), Pane(Pane), Step(Stage),
    Cell { track: u8, frame: u32 }, Named(&'static str), Paper,
}
```

One primitive, `plate::mark(ctx, rect, target)`, compares against the wish in temp data and records the rect. Sites: `menus::hover_tip` (Action, so rail, band and deck keys come free), `menus::menu_item` (Action, because it uses `on_hover_text`, not `hover_tip`), `export_step::lead_choice` and `export_step::choice` (Action, because the Export step's cards go through `resp.card`, not `hover_tip`, and `ExportPngSequence` is a plain `choice`), `chord_card` (Action, because `loop_keys` uses it), `floats::show_float` for `Float` (it pushes only the pane to `pane_rects`, so it needs its own line), the rail's key painter (`rail::Key` becomes `pub(crate)`), the top row's pill loop, `pane_rects`, `xsheet_panel` for `Cell`, and one line on each `Named` control. No site compares words.

A pane in a hidden tab has no rect. A key inside `fold_menu`'s `…` has no rect until the fold opens. A row inside a menu has none until the menu opens. Each of those needs an ancestor in the chain (5.8), which is why `menu.*` and `band.fold` exist.

| `Named` handle | Registered by |
|---|---|
| `lesson.chip` | Lesson frame |
| `band.fold`, `band.speed`, `speed.medium`, `band.loop`, `band.longer`, `band.sheet`, `strip.next`, `layers.add`, `layers.custom`, `layers.eye`, `layers.role`, `brush.sketch`, `brush.clean`, `steady.row`, `keep.row`, `export.more`, `export.transparent`, `options.chart`, `lip.generate` | Handles and ring |
| `menu.view`, `menu.animation`, `menu.animation.track`, `menu.more`, `menu.more.panels` | Handles and ring. Each marks its menu word or submenu head. At `narrow`, the collapsed `Menu` word (`plate::menu_word`) marks every `menu.*` until its row is open. Menus keeps the marks when it moves rows, and `auguide_ring_reaches_the_last_target` fails by name if a row left a chain behind |
| `swatch.black`, `.white`, `.yellow`, `.brown`, `.red` | Color step |
| `shelf.glow`, `shelf.strength`, `shelf.camera`, `shelf.add` | Effects step |
| `manga.split_across`, `manga.split_down`, `manga.panel.<n>`, `manga.gutter.side`, `manga.gutter.row`, `manga.rulers`, `manga.see_page`, `manga.ink_tone`, `manga.tone_preset`, `manga.balloon`, `manga.text`, `manga.speed_lines`, `manga.words`, `export.page.mono` | Comic step |

`strip.next` marks the first sheet on the strip whose drawing has ink and no color. `strip::tap` lands on a run's first frame, never a held one, which is why the Color step can walk drawings through it: `Next frame` on the band stops on held frames, where Fill refuses.

**What may land first.** The engine parts bind only to these handles and to `Action`, `Pane`, `Stage`, which are saved vocabulary, so Menus, Plain words, Color, Effects and Panels cannot break them. The lesson content parts still come **after** Plain words, Color step, Effects step, Comic step and Panels, for three reasons: their tasks need those steps to exist, `aulesson_no_old_words` needs the final words, and the Rig's picture of each control needs its final fold.

### 5.6 The card

`egui::Panel::right("lesson").exact_size(c)`, inserted straight after the band's `Panel::bottom`, before `dock_rect` is taken. `c = clamp(window_w * 0.2, 180, 220)`. The dock, Fit and the rail's host all follow. Nothing is saved, no `Pane` is added, and it stands in every step.

Top to bottom: title. The strip, one tick per task, with a small vector Deckle standing on it, which moves one tick per finished task, swapping flat and arch poses. The path, this lesson's steps in bar order, current one open. The task sentence at 16 pt. The step's `why` at 13 pt, while its step is young. The finish picture. Then, outside the scroll, `Hold to see it` (Amateur), `Show me` or `Show me where`, `Leave lesson`.

All papers are 4:3, square or page, because a wide paper would pay for the card. On the open canvas a 4:3 paper is limited by height, so the card costs it nothing. In Plan it costs a little: at 1600×900 about 966×724 against 985×739. At the worst size, 832×520, Plan's 4:3 paper is about 412×309 if the sheet takes its 22%, and smaller if the sheet's own minimum width wins, which was not verified.

### 5.7 The guide

`canvas.guide`, set by the Editor each frame as `empty_cell_hint` is, painted with `to_screen` after step 0 and before the render, so it follows zoom, turn and mirror and stays under the art. `Path.over` paints it above an opaque background. No GPU and no paint layer are needed, so the Rig sees it. Colour: `plate::GUIDE`, from `CHIP_AO`, because `plate::AO` is now grey. Only the current task's lines stay blue, so the paper clears as you go.

It cannot export, because `render_frame`, `render_thumb` and `store::save` read `Project` and the guide is never there. `ColumnKind::Ref`, `cut.images` and `LayerRole::Reference` were rejected: `render_split` drops non-rendering columns from the canvas, and the others are stored in the file and count as art.

### 5.8 The ring

Recorded by `plate::mark`, stroked at frame end on a bare `LayerId::new(Order::Tooltip, ..)` painter. A painter layer is no `Area`, so `layer_id_at` skips it and the pen passes (R15). `plate::ring` takes a `Ui` and is clipped to its panel, so it does not serve here.

**The chain.** `Task.ring` lists ancestor first. The ring stands on the last target with a rect this frame. A float's rail key, a closed fold's header and the Pocket tier's `More` need no `reveal` step: list them, and the ring walks in as the person opens each.

Two rules follow. A target that is always on screen hides everything listed before it, so a rail key or a menu word never follows a control that still needs opening: two controls in two places are two tasks. And a chain that stops on an ancestor still "has a rect", so that alone proves nothing: `auguide_ring_reaches_the_last_target` clicks where the ring stands until the last target has a rect, at each size, and fails by name.

### 5.9 Show me

First build: `Show::Trace` reveals the guide path as ink, on the paper, at a hand's pace, about 1.5 s a stroke, then lands it with `commit_path`. Pen down cancels, and nothing lands. The same route runs on the Rig and on a GPU, so the end-to-end gate proves what ships.

`commit_path` generalises `doc::commit_ring`, and `path_tiles(points, width, rgba, erase)` generalises `ring_tiles`, walking segment boxes, not the whole bounding box. `commit_ring` ignores `Keep inside strokes`, because the lock lives in the GPU's `PaintMode::AlphaLock`. `commit_path` honours it on the CPU: with the lock on, a texel with no alpha stays empty. Without this the Rig could not prove A Coat of Shavings' rims, and a stand-in drawn inside the wood would pass with no lock at all. Everything else is `demo_circle`: the `stroke_active` refusal, `slot_at_pen_down`, `canvas.ink()`, `commit_raster`, `engine_changed`, `say.circle`, one undo, the stroke signal. `demo_circle`'s `enter_room` fallback must not run in a lesson.

`Show::Fill` calls `fill_at(paper_pos, state)`, the body of `fill_input` past its gate, pulled out and shared. `Show::Perform` acts once with the ring on. `Show::Run` covers controls with no Action. Export's Show me opens the real dialog: the person presses Save.

With no paint engine, a tap with Fill or Select refuses as today. The card adds `This machine cannot do that by hand. Press Show me.` `fill_at` is CPU work, so a Rookie still finishes.

Replay through the real brush (`stroke_start`, `push_point`, `stroke_end`) is left for later: those are private, tied to `mouse_lockout`, `stab` and `slot_at_pen_down`, and have no route on the Rig.

### 5.10 Walking the bar

The first task of every step after the first is pressing that pill: `Done::OnStep`, ring on the pill, with a `why` sentence. **No lesson calls `enter_room`.** Nothing is locked or refused. If the learner wanders off, the card's top line becomes `Go back to {Step}`, ringed. Tools stay live throughout.

### 5.11 Leaving, resuming, finishing

- `Leave lesson`: guide and card go, `fill_gap` and the starting ink are restored, the paper refits once, the document stays a real document. `app.lesson` is removed. `meta_dirty` is set only if the file already has a path, so an untouched lesson never asks to save.
- Progress: `app.lesson = id/task`, outside undo, updated on each tick without `meta_dirty`.
- Resume is read at the one door (2.6), not in `Editor::boot`, because `Open` and `Recover` replace the state without a boot. An unknown id is ignored and kept. A guest never runs a lesson.
- While `app.lesson` stands, promotion does not run (`aulesson_holds_promotion`). It runs on Leave.
- Finish: the Export step's real Done card with `Open file` or `Show in folder`. The card's Deckle reaches the last tick and stands up. `You made that. It is a real file.`, then the lesson's `closing` line, then `Next lesson`. The document stays open. `lessons.txt` gains the id. No confetti, score or badge.
- `Next lesson` goes to the next class in the same type, and after Experienced back to the Lessons page.

### 5.12 Say during a lesson

The card carries the task sentence, so the lane must not. `Facts.lesson` suppresses the Quick Start chain rows and `EMPTY_STEPS`, and keeps refusals, `REFUSED_TWICE` and `FLAT_PEN`. The five dots hide. Quick Start ticks stop for the lesson's run, because a lesson is not a free Quick Start.

### 5.13 Art pipeline

**Stroke data in code, no binary assets.** Each lesson's `art` builds `Path`s in paper units from cubic curves, sampled to `(x, y, pressure)`. `Path { points, width, kind, over }`, where `kind` is `Guide`, `Landmark`, `StandIn` or `Finish`.

**Deckle is a generator.** `deckle(spine, seed)` builds the outline from a spine curve. Constant arc length: paper does not stretch, so every pose stays on model. Planted feet: contact points stay fixed between poses, so nothing skates. One long side is torn from seeded noise, the other cut straight.

**Why charming, not clip-art.** Every stroke tapers like a pen, thicker on the turn. Lines overshoot slightly at corners. Nothing is symmetrical. A hand wobble runs along the curve. Deckle is two lines and three dots.

### 5.14 How the art is checked

Thresholds are first estimates, set from the first measured value, never tuned to pass.

| Check | Measure | Runs |
|---|---|---|
| Contact sheet | All finishes in one PNG for the Lead's eye, written by an ignored test, no window | Ordinary shots |
| On model | Arc length within 1% across poses | Ordinary |
| No skating | Planted points within 2 px between poses | Ordinary |
| Closed shapes | Outside flood never reaches the body seed at gap 2 | Ordinary |
| `guide_closes_where_filled` | Every `Show::Fill` seed bounds a region at the default gap | Ordinary |
| Trace load | Rookie ≤6 strokes a drawing, inside the paper's margin | Ordinary |
| Hand wobble | 0.4 to 1.6 px from the smooth curve, on every path longer than 60 px | Ordinary |
| No stock shape | No path longer than 60 px fits a circle within 3%. A dab, a star or a peg hole is a press and is exempt | Ordinary |
| Classes and types differ | 64×64 finishes, all 36 pairs of the nine, IoU ≤0.45, so a comic cannot be an animation's drawing again | Ordinary |
| Lands with people | One first-time user does Up Late | By hand, on the Lead's list, **not a code gate** |

### 5.15 The one gate

`auguide_every_lesson_ends_in_its_file` drives **every lesson, every task, through the real editor Rig on a first-run boot, to a real finished file.** One loop on `Recorder`, reading only the lesson's own tables, so a new lesson joins with no new test code. Any refusal or ignored verb fails it by name (main spec 1.5 rule 12).

Each pass: assert the ring has a rect, click where the ring stands until the chain's last target has one (the driver never names a word), or press `Show me`, assert this act ticked this task and no other. A `Played` task steps the Rig's clock until the pass ends. It ends on the real Export step, through `h.answer_export` or a new `h.answer_pages(dir)`, then decodes the file: PNG, `gif_decode` or page files. It asserts size, frame count, ink where the paths are, each lesson's own file facts from the lessons file, then saves the `.animproj`, reopens it and asserts it is equal.

| Mode | Route | Proves |
|---|---|---|
| Show me, ordinary, all lessons | `commit_path`, `fill_at`, real Export. `stand_in` paths draw a person's own art | The finished file |
| Pen, ordinary, all lessons | `Act::Stroke` along paths through `paper_to_screen`, with `paint_texels` for the pixels and `commit_path`'s `erase` for an eraser stroke, because a Rig stroke takes the vector route | Every stroke task ticks from a real pen route at a real window size |
| Real, ignored `with_gpu`, one per type | Real raster strokes and fill | Main spec 1.5 rules 5 and 11 |

The gate needs `h.ring_rect()` and `h.click_at(rect)`. `lessons::for_gate()` is `ALL` plus the probe lesson. Each lesson part grows it. The last part pins `ALL.len() == 9`.

---

## AU18 Project kinds and three bars

After this part each project has its own bar, and a plain drawing has no Plan.

### Start from

`steps.rs`. `workspace.rs`: `Stage::name`, `Stage::card`. `templates.rs`: `Template`, `intent`, `Template::build`, `INTENTS`. `main.rs`: `open_intent`, `enter_template`, `enter_intent`, the opened-document block in `Editor::ui`, the top row's pill loop and `steps_width`, `place_name`, `can_perform`, `perform`, `band_right`, `recipe`, `enter_room`, `export_view`, `stage_kept`, `leave_export`, `locked_panes`, the `ResetLayout` arm. `say.rs`: `HINTS`, `Facts`, `say_facts`, `say_act`. `welcome.rs`: `STEPS`, `tick`, `checklist_ui`. `menus.rs`: `help_windows`. `testkit.rs`: the line beside `more_default`.

### Build

1. `ProjectKind`, `of`, `derive` (2.2 to 2.4), the frozen seam (1.13).
2. Catalog, `bar(kind)`, `of(stage)`, `Stage::card` through the catalog (2.1). `Editor::step_name`. The top row reads `bar`.
3. `Template.kind`, `templates::READY`, the route table (2.5). Ready paper is no longer `INTENTS[ANIMATION]`, and its strip stays closed.
4. The door (2.6, steps 1 to 4). `first_step()`, and the redirect at the head of `enter_room` (2.8).
5. `hidden_step`, in both `can_perform` and `perform` (2.8).
6. Promotion (2.7), the move to Manga, its remark on bit 24.
7. Everything in 2.9.
8. Quick Start per kind, `Facts.kind`, the `kinds` filter (2.10).
9. **A first-step check of the Comic bar:** a fill on a page cel, through the `fill_rig` pattern on an armed Color step. If it does not land, Color leaves the Comic bar, the bar is Manga, Export, fill stays on Manga's rail, and each comic's Color tasks move to the Manga step. The report says which.
10. Move the tests in 1.13 that belong here, and `aukeys_six_steps_have_keys` to `aukeys_each_bar_has_its_keys`. Plan's pill is `Stage::Drawing` with today's dock until the next part.

### Acceptance

1. `bar(Drawing)` reads Draw, Color, Effects, Export. `bar(Animation)` reads Plan, Timing, Color, Effects, Export. `bar(Comic)` reads Manga, Color, Export (or Manga, Export under item 9). Timing stands before Color.
2. The top row draws exactly those pills on each kind.
3. Stored kind wins. An unknown or absent value derives by 2.4.
4. `app.kind` round-trips through save and load. An old file opens not dirty. Schema is 11. The sample's bytes are unchanged.
5. Each route of 2.5 gives its kind. `READY.build()` holds no key. Ready paper shows one frame, strip closed, and first pill Draw, and the door leaves it not dirty.
6. Every open route lands on a step of the bar, a comic on Manga. Opening an animation from Draw shows the sheet. Opening a drawing from Plan shows no sheet. Replacing the document on Export leaves a way back that names a pill of the new bar and restores that step's recipe. No comic reaches `Stage::Drawing` by `leave_export`, `ResetLayout`, F4 or F5. An old file with one page among several cuts opens as an animation.
7. Each row of 2.8 holds through a chord and through `can_perform`, with the exact sentence.
8. Promotion holds for Add drawing, Insert frames, a new cut, paste, and a guest's edit. Two tracks on one frame stay a drawing. New page on a drawing makes a comic and lands on Manga from Draw and from Effects. Undo does not demote.
9. A drawing's band has no `Add drawing` or `Timing sheet`. Its Say chain never names a frame or Play.
10. J1 ≤28, ≤5. J2 unchanged. Draw gates and `PAPER_FLOORS`, `PEN_FLOORS` pass unedited.
11. Every `auseam_` test passes unedited.

### Tests (`aukind_`, at least 16)

`aukind_derive_table`, `aukind_stored_wins`, `aukind_unknown_value_derives`, `aukind_round_trips_as_app_kind`, `aukind_old_file_opens_clean`, `aukind_sample_stays_byte_stable`, `aukind_bars_are_the_table`, `aukind_every_route_stamps`, `aukind_ready_paper_is_an_unstamped_drawing`, `aukind_door_lands_on_the_first_step`, `aukind_door_swaps_draw_and_plan_docks`, `aukind_door_rewrites_the_way_back`, `aukind_comic_opens_on_manga`, `aukind_no_route_puts_a_comic_on_draw`, `aukind_one_page_among_cuts_stays_an_animation`, `aukind_new_page_lands_on_manga`, `aukind_export_back_names_a_pill`, `aukind_hidden_steps_refuse_through_a_chord`, `aukind_can_perform_agrees`, `aukind_f4_runs_the_first_step`, `aukind_promotes_by_frames`, `aukind_two_tracks_on_one_frame_stay_a_drawing`, `aukind_new_page_makes_a_comic`, `aukind_undo_does_not_demote`, `aukind_leave_sample_resets`, `aukind_guest_promotes_with_no_wire_message`, `aukind_go_to_word_follows_the_kind`, `aukind_quick_start_rows_per_kind`, `aukind_no_hint_points_backward_on_any_bar`, `aukind_fill_lands_on_a_page_cel`, `aukind_satsuei_refused_on_a_page`.

### Must not change

`Stage::ALL`, `stage_key`, every `Action`, `Pane`, stack name, the `ui` section's shape, schema 11, `rail.rs`, the look, the frozen boot's behaviour.

### Leave out

Plan's recipe, a Kind row in Project settings, Play's presence on a one-frame drawing.

## AU19 Plan step

After this part an animation opens on a real drawing step, sheet at left, paper clear of the rail.

### Start from

`workspace.rs`: `Stage::dock`, `dock_with_sheet`. `main.rs`: `recipe`, `enter_room`, `band_left`, `band_right`, `sheet_button`, `switch_sheet`, `locked_panes`, `show_sheet`, `sheet_on_screen`, `strip_room`, `speed_rows`, the `EditorTabs::ui` preview arm. `canvas.rs`: the Fit block in `CanvasView::ui`, the Space hand block, `turned_fit`. `viewer.rs`. `say.rs`: `say_plan`. `config.rs`: `RoomLayout` help, `UiConfig`.

### Build

1. Recipe, redirect, panes (3.1 to 3.3). `tab_style_override` for the canvas.
2. Remove the preview and all of 3.11.
3. `rail_reserve`, its drop while tools are hidden, Fit, the hand pivot, `turned_fit` (3.5).
4. The band split, `in_plan()`, the mirror out of Plan (3.6).
5. `show_sheet`, help text and aliases (3.7).
6. `ui.panel_more`, `plan_sheet_hidden`, `locked_panes`, `switch_sheet` (3.10, 2.9).
7. The Say row on bit 23.
8. Move the tests in 1.13 that belong here. `strip_of`'s seam first, because it unblocks 45 uses.

### Acceptance

1. Entering Plan on an animation shows exactly Canvas and X-Sheet, left share 0.22.
2. `enter_room(Stage::Layout)` lands on `Stage::Drawing`. No code path leaves `self.stage == Some(Stage::Layout)`.
3. No stroke is refused in Plan. No `Viewer(u8::MAX)` exists.
4. `paper_under_chrome == 0` in Plan at the 4 sizes × 3 scales, for 16:9, 4:3, 1:1, 9:16.
5. Paper share at Fit, 16:9: ≥49.5% at 1600×900, ≥43.0% at 1280×800.
6. Whole sheet rows ≥32 and ≥27.
7. Clicking a row and drawing puts a key on that row and splits the run. One undo restores the document byte for byte.
8. The link holds both ways. Clicking a row changes the drawing the live canvas shows. A stroke on the canvas shows on the sheet's cursor row in the same frame, and moves nothing else.
9. One timeline: in Plan with the sheet on, `Strip::rect()` is None, no mirror paints, no `Viewer` pane exists, band 36.
10. Plan's band reads Previous, Play, Next, counter, Say, Add drawing, Ghost frames, Speed, Timing sheet.
11. Hiding the sheet survives a real restart. A reopened sheet stays reopened. Each changes `locked_panes` in the same frame.
12. `aurail_rail_fits_every_room` includes Plan. Open-canvas Fit is pixel-equal: `PAPER_FLOORS`, `PEN_FLOORS` and every `aurail_` test but the mirror's pass unedited.
13. J1 ≤28, ≤5, ≤3, ≤2, real GIF, cut still growing as today until the next part.

### Tests (`auplan_`, at least 12)

`auplan_recipe_is_sheet_and_canvas`, `auplan_layout_redirects_at_the_door`, `auplan_no_preview`, `auplan_paper_clears_the_rail`, `auplan_paper_share`, `auplan_sheet_rows`, `auplan_click_a_row_and_draw`, `auplan_row_moves_the_canvas`, `auplan_stroke_shows_on_the_cursor_row`, `auplan_one_timeline`, `auplan_band_keeps_the_drawing_keys`, `auplan_hidden_sheet_survives_a_restart`, `auplan_panes_follow_the_recipe`, `auplan_hand_turn_pivots_on_the_paper`, `auplan_row_hint_once`, `auplan_f4_and_f5_land_on_one_pill`.

`auplan_row_moves_the_canvas` compares the paper's pixels at two rows. A test that reads only `view.frame` would pass with the old locked preview in place.

### Must not change

`rail.rs`. Open-canvas Fit. `dock_with_sheet`. `stage_key`. The sheet's density. Fill, ink, onion. `ui.sheet_in_draw`'s field.

### Leave out

A per-leaf tab bar height, the `Details` fold on the sheet head, Play on a one-frame drawing, the twelve rows and the blank tail.

## AU26 Twelve rows and the blank tail

After this part a new animation has rows to draw on, and blank frames neither play nor export.

### Start from

`templates.rs`: `INTENTS`. `doc.rs`: `add_drawing_after`, `set_speed`, `set_run_length`, `play_range`, `run_range`, `tick`. `render_queue.rs`: `resolve`, `add_job`. `main.rs`: `band_right`, `export_to`. `export_step.rs`. The 3.8 note on `austrip_tests.rs`.

### Build

1. Twelve rows (3.7).
2. Rule one and rule two (3.8), `run_range` and `tick` with them.
3. `Longer` and `Shorter` (3.9).
4. Move the tests in 1.13 that belong here.

### Acceptance

1. `An animation` opens with 12 rows, `Frame 1 of 12`.
2. Add drawing takes blank frames first: `[1, 1, 1, 45]`, `frame_count() == 48` where the old test read 46 and 49.
3. Three drawings on a fresh animation: Play runs three frames from any cursor, and the GIF holds three pictures. Six drawings at Medium: a 12-frame GIF of 1.0 s.
4. A background track holding to the end keeps the tail. A loop range inside the cut wins. A job from the queue's form renders the whole cut.
5. `Longer` on the last run grows the cut by one. `Shorter` stops at one frame. Both stand only in Timing, never fold, and each change is one undo.
6. J1 ≤28, ≤5, ≤3, ≤2, a real GIF of 12 frames.

### Tests (`autail_`, at least 8)

`autail_new_animation_has_twelve_rows`, `autail_add_drawing_takes_the_blank_tail`, `autail_blank_tail_never_plays`, `autail_cursor_on_the_tail_plays_no_blanks`, `autail_blank_tail_never_exports`, `autail_background_keeps_the_tail`, `autail_loop_range_wins_over_the_tail`, `autail_queue_keeps_the_whole_cut`, `autail_longer_grows_the_last_run`, `autail_shorter_stops_at_one`.

### Must not change

`hold_to_copy`. `Action::HoldPlus`'s refusal. The queue's own range. `frame_count()`, the counter and the sheet's rows.

### Leave out

Trimming the cut itself.

## AU20 Central Start

After this part Start stands in the middle. The Lessons page is built and waits, unreachable, until a lesson ships.

### Start from

`floats.rs`: `start_place`, `Head`, `frame`. `main.rs`: `start_ui`, `open_start`, `choose_start`, `apply_start`, `dock_rect`. `welcome.rs`: `float_body`, `intent_cards`, `WelcomeTab`, `WelcomeAction`. `plate.rs`: `choice_card`, `card_chip`.

### Build

1. `start_place`, `pane` as the dock rect, the new closing rule (4.1, 4.2).
2. The body, three across, row two (4.3). Rewrite the tests in 1.13.
3. `WelcomeTab::Lessons`, the grid, `WelcomeAction::Lesson`, `replaces_document`, `Action::OpenLessons` (4.5). The page reads `lessons::ALL`. While it is empty, row two stays `Open the example` and the page cannot be reached.

### Acceptance

1. Start is centred on the dock rect within 1 pt and 600×300 at the most at all 12 rows of 4.2, with the left edge at least 116 on the 11 gated rows, at least 80 at 760×570, and the card inside the dock.
2. With Start at its largest at 1600×900, paper uncovered ≥58.0%, the assert unedited. Start ≤14 controls, ≤30 words. First screen ≤44, ≤70.
3. A stroke on free paper lands at every size, closes Start and keeps the stroke. `austart_float_blocks_only_its_own_rect` passes unedited. Stored 200 clips nothing at 1024×640.
4. Opening a left float that intersects the card closes it. At 1600×900 it does not intersect.
5. The Lessons page is ≤12 controls, ≤48 words, and its grid stands unscrolled at all sizes, on a stand-in table of nine rows that a test supplies.
6. Shortcuts stay live while Start stands: a chord over the card acts as it does with Start closed.

### Tests (`austart_`, at least 6)

`austart_stands_centred`, `austart_clears_the_rail`, `austart_paper_uncovered`, `austart_left_float_closes_it`, `austart_row_two_waits_for_lessons`, `austart_page_budget`, `austart_grid_stands_unscrolled`, `austart_chords_stay_live`.

### Must not change

Start's non-modal rules (4.6). `rail.rs`. The look. The `ui` section. `austart_float_blocks_only_its_own_rect`.

### Leave out

The card, a lesson of any kind.

## AU27 Lesson frame

After this part a lesson can run from start to finish, proved on a probe lesson. No lesson ships yet.

### Start from

`main.rs`: `apply_start`, the band's `Panel::bottom`, `dock_rect`, the door. `say.rs`: `say_read`, `read`, `say_line`. `welcome.rs`: `WelcomeAction`. `config.rs`: `data_dir`.

### Build

1. `lesson.rs`, `lessons.rs`: types (5.3), tokens, `open_lesson` with its three clears, `LessonRun`, `fill_gap`.
2. The card (5.6), `lesson.chip`. `rail_reserve` also while a lesson stands (3.5).
3. `lesson_read`, `Done` (5.4) for everything except art, the wander line (5.10), step `why`.
4. Leave, resume at the door, finish, `Next lesson`, `lessons.txt`, the promotion hold (5.11). `config::data_dir()` is a seam in tests.
5. Say during a lesson (5.12). `T` refused while a lesson stands (1.11).
6. A probe lesson in test code, Draw, Color, Export, four tasks, through `WelcomeAction::Lesson`.
7. Table rules that every later lesson must pass: walks the bar, class rule, words, tokens, reasons. They run over `lessons::for_gate()`.

### Acceptance

1. `Follow a lesson` opens the Lessons page in one click on a test table.
2. Opening a lesson over an untouched blank asks nothing. Over unsaved work it goes through the Leave guard. A lesson never made dirty never asks to save.
3. Opening a lesson clears the three left-over states: with tools hidden the rail shows, with `plan_sheet_hidden` set an animation lesson's Plan shows its sheet and the stored value is unchanged after Leave.
4. The card covers no paper: `paper_under_chrome == 0` for 4:3, 1:1 and page paper in every step, 4 sizes × 3 scales.
5. Every sentence fits the card at 180 wide without moving `Show me` or `Leave lesson` out of view.
6. Tasks tick on state: reaching the state by a route the lesson did not name ticks the task. A learner ahead sails through. Ticks do not un-tick on Undo.
7. An act that misses does not tick: a press of Play stopped at once, a stroke beside the smudge, one stroke across two listed paths, a cancelled export.
8. Wandering off shows `Go back to {Step}`. No control is locked. Tools work during a lesson.
9. Refusals, `REFUSED_TWICE` and `FLAT_PEN` still speak in the lane. The Quick Start chain and the dots do not. `hints_seen` and `quick_start` are unchanged by a lesson's run.
10. Leave keeps the document, restores `fill_gap`, removes the key. Saving mid-lesson and reopening through `Choice::OpenRecent` resumes on the same task.
11. No promotion during a lesson. It runs on Leave.
12. Finishing writes the id to `lessons.txt`, and the page shows the tick. A missing or broken file shows no ticks and no error.

### Tests (`aulesson_`, at least 16)

`aulesson_page_is_one_click`, `aulesson_open_goes_through_the_leave_guard`, `aulesson_untouched_lesson_asks_nothing`, `aulesson_shows_hidden_tools`, `aulesson_ignores_a_hidden_sheet`, `aulesson_card_covers_no_paper`, `aulesson_card_fits_every_sentence`, `aulesson_done_reads_the_state` (each `Done`, both directions), `aulesson_misses_do_not_tick`, `aulesson_ahead_sails_through`, `aulesson_wander_points_back`, `aulesson_tools_stay_live`, `aukeys_hide_tools_waits_for_a_lesson`, `aulesson_say_keeps_refusals`, `aulesson_hints_bits_untouched`, `aulesson_leave_keeps_the_document`, `aulesson_leave_then_resume`, `aulesson_holds_promotion`, `aulesson_finish_ticks_the_grid`, `aulesson_tasks_walk_the_bar`, `aulesson_step_reason_on_every_change`, `aulesson_class_is_honest`, `aulesson_rookie_words`, `aulesson_no_old_words`, `aulesson_tokens_resolve`.

`aulesson_class_is_honest`: every Rookie task has a `Show` other than `Where`. No Amateur task carries a `Guide` path. No Experienced task has any path on the paper. `aulesson_no_old_words` starts as a table test and takes the section 3 old-word list.

### Must not change

`rail.rs`. The look. The `ui` section. Fit outside `rail_reserve`.

### Leave out

The guide, the ring, Show me's art, a real lesson.

## AU21 Handles and ring

After this part a lesson can point at any control, and a test proves every chain reaches it.

### Start from

`menus.rs`: `hover_tip`, `menu_item`. `export_step.rs`: `lead_choice`, `choice`. `floats.rs`: `show_float`. `main.rs`: `chord_card`, `fold_menu`, the menu row, `plate::menu_word`. `strip.rs`: `tap`, `sheet_rect`. `rail.rs`: `Key`, `keys_of`. `xsheet_panel.rs`. `testkit.rs`: `Recorder`, `EditorHarness`. `measure.rs`.

### Build

1. `Target`, `plate::mark`, the wish, every site, the `Named` handles this part owns (5.5).
2. The ring painter, the chain (5.8).
3. The driver's `h.ring_rect()`, `h.click_at(rect)`.

### Acceptance

1. The ring stands on the control for every task at every size, and the driver clicks only where it stands. At the Pocket tier a held-back key rings `More` until it opens. At 1024×640 a key behind the band's `…` rings `…` until it opens, and a menu row rings its menu word until it opens.
2. Every chain in the probe lesson reaches its last target. A chain whose last target never gets a rect fails by name.
3. A pen pressed under the ring lands: the ring takes no input.

### Tests (`auguide_`, at least 6)

`auguide_ring_stands_on_its_control`, `auguide_ring_reaches_the_last_target`, `auguide_ring_passes_the_pen`, `auguide_ring_walks_the_chain`, `auguide_pocket_rings_more`, `auguide_fold_rings_the_fold`, `auguide_menu_rings_the_menu`, `auguide_cell_ring_follows_the_scroll`.

### Must not change

`panel.ring`. `rail.rs` beyond `pub(crate) Key` and `mark` lines.

### Leave out

The guide, Show me, any art.

## AU28 Guide and Show me

After this part a lesson can show and do, and one gate walks it to a real file. The Lead gets a contact sheet of Deckle, and nothing new in the app.

### Start from

`canvas.rs`: paint order around step 0, `empty_cell_hint`, `fill_input`, `finish_stroke`, `ink`, `paper_to_screen`. `doc.rs`: `ring_tiles`, `commit_ring`, `commit_raster`, `tile_over`. `say.rs`: `demo_circle`. `plate.rs`: `speak`, the token block. `testkit.rs`: `answer_export`.

### Build

1. `Path`, `canvas.guide`, `over`, `plate::GUIDE` (5.7). `Hold to see it`.
2. `lessons::art`: curves to samples, hand wobble, taper, `deckle(spine, seed)` (5.13).
3. `path_tiles`, `commit_path`, `erase`, keep inside, `fill_at`, `canvas.last_stroke_box` (5.9, 5.4).
4. `Show` dispatch, the reveal, cancel on pen down, the no-engine line. The art `Done`s.
5. The rest of the driver: `h.answer_pages(dir)`, both ordinary modes, one ignored real mode (5.15).
6. The contact sheet writer.
7. The art checks of 5.14 that run on tables.

### Acceptance

1. `auguide_every_lesson_ends_in_its_file` passes on the probe lesson in both ordinary modes.
2. The guide follows zoom, turn and mirror. Its colour appears in no file and no saved key. A rule test over `sources()` keeps `guide` out of the render and save modules.
3. `Show::Trace` is one undo and ticks like a stroke. Pen down during the reveal lands nothing and keeps the pen's own stroke.
4. `commit_path` over a full-page path tests only pixels near its segments: a cost test with a stated ceiling, set from the first measure.
5. With `Keep inside strokes` on, `commit_path` changes no texel that had no alpha. With it off, the same path does.
6. `fill_at` and `fill_input` give identical pixels on one seed.
7. With no paint engine a Rookie task still ticks through Show me, and the card says so.
8. `deckle`: arc length within 1%, planted points within 2 px.

### Tests (`auguide_`, at least 10)

`auguide_every_lesson_ends_in_its_file`, `auguide_pen_ticks_every_stroke_task`, `auguide_guide_never_exports`, `auguide_guide_stays_out_of_render_and_save`, `auguide_guide_follows_the_view`, `auguide_show_me_is_one_undo`, `auguide_pen_down_cancels_the_reveal`, `auguide_commit_path_walks_segment_boxes`, `auguide_commit_path_keeps_inside`, `auguide_fill_at_matches_fill_input`, `auguide_refused_fill_points_to_show_me`, `auguide_deckle_keeps_its_length`, `auguide_deckle_feet_stay_planted`, `auguide_real_route_with_gpu` (ignored, and it asserts `open_lesson` arms the raster brush and every rail key a lesson rings has a rect).

### Must not change

`demo_circle`'s behaviour outside a lesson. `fill_input`'s results. `.animproj` contents: the guide is never stored.

### Leave out

Real-brush replay, any lesson's art, lesson text.

## AU22 Comic step

After this part a comic can see its page, reach tone, and letter without the node graph.

### Start from

`manga_tools.rs`: the pane, `panel_op`, `paint_page`, the page export options. `export_step.rs`: `Offer::Comic`. `doc.rs`: `clip_to_panels`, `page_hand_built`, the two `page_graph` call sites. `crates/anim-core/src/manga.rs`: `page_graph`, `is_page_stack`, `is_free_generator`, `is_tone_column`. `generator.rs`. `wire_over` from the Effects step. `canvas.rs`: the paint cursor gate.

### Build

1. `See the page`, a latch on the Manga pane running `ToggleCompositeView`. A stroke tried while on is refused in the words `Turn off {See the page} to draw.` Stage::Manga enters with it off.
2. `Ink` and `Tone`, one latch on the Manga pane, on a page with both template tracks. It sets `view.active_column`. Tone stays on the template column.
3. `Add balloon`, `Add text`, `Speed lines`. Each adds a generator, and rebuilds the page stack through `manga::page_graph`, in one undo, so `page_hand_built` stays None.
4. A one-line `Words` field for the selected balloon or text.
5. A drag handle on the canvas for the selected generator, writing its fractions. One undo per drag.
6. `Mono`, a latch with its threshold on the Export step's `Offer::Comic`, reading and writing the same page options the Manga pane's export rows use (`o.mode`, `o.threshold`). Today mono exists only on the Manga pane, so the last step of a comic has no such control. The Manga pane's rows stay.
7. Register every `manga.*` handle and `export.page.mono`. The stock `ui.button` rows get `plate::mark`.
8. Confirm the page export's `Landed` and Done card (5.4).

### Acceptance

1. `See the page` shows the composed page with a GPU. With none it says why. A stroke while on is refused with the way back.
2. The latch moves the active track between INK and TONE and is absent off a page.
3. Each of the three verbs is one undo, reaches the page's export, and leaves `page_hand_built` at None, so no DANGER edge appears.
4. Typed words reach the file.
5. A drag moves the generator by the paper delta, under zoom and turn.
6. A page export reaches the Done card with `Show in folder`, and Quick Start's last slot ticks.
7. `Mono` on the Export step and the Manga pane show one state, and a page exported with it holds only black and white pixels.
8. Clip audit `Scope::Chrome` passes on Stage::Manga at all sizes, and on Export for a comic.

### Tests (`aupage_`, at least 10)

`aupage_see_the_page_refuses_a_stroke_with_the_way_back`, `aupage_see_the_page_says_why_with_no_gpu`, `aupage_latch_sets_the_track`, `aupage_latch_is_absent_off_a_page`, `aupage_balloon_is_one_undo_and_reaches_the_file`, `aupage_text_is_one_undo_and_reaches_the_file`, `aupage_speed_lines_are_one_undo_and_reach_the_file`, `aupage_verbs_leave_no_danger`, `aupage_words_reach_the_file`, `aupage_handle_writes_the_fractions`, `aupage_every_handle_registers`, `aupage_export_reaches_the_done_card`, `aupage_mono_on_export_is_the_pane_state`.

### Must not change

`page_graph`'s stack for a hand-built page. `Clip to panels`' DANGER rule. Generator params. `is_tone_column`. Page export formats.

### Leave out

Balloon tails by hand, fonts, vertical text, effects on pages.

## AU23 Rookie lessons

After this part Start offers `Follow a lesson`, and a newcomer can finish three real files.

### Start from

The lessons file: Up Late, End of the Desk, Not a Pencil. `lessons.rs`. **No engine change.** A missing `Done`, handle or engine verb stops the part and goes back to the part that owns it.

### Build

1. Three `Template` consts, `prep` for each, `art` tables through `deckle`.
2. The tables, every sentence as the lessons file gives it.
3. `Open the example` moves to the Lessons page. Row two becomes `Follow a lesson`.
4. Each lesson's file facts.
5. Contact sheet, thumbnails.

### Acceptance

1. `auguide_every_lesson_ends_in_its_file` passes on the three in both ordinary modes, on `as_first_run()`, at 1280×800 and 1024×640.
2. Show me alone, one press a task plus the save dialog, reaches each file.
3. Up Late's PNG: 1200×1200, night at the sky seed, white at the body seed, a yellow star brighter than its neighbours' sky by Glow's halo, the smudge's box clean, no guide colour.
4. End of the Desk's GIF: 15 frames, 1.25 s, 6 distinct pictures, Deckle's mass moves right, the last picture is the desk alone.
5. Not a Pencil's page: 888×1243, 4 panels, one black region, no guide colour.
6. Night's leak path: a leak is seen, the sentence shows, Undo and Show me recover.
7. Rookie words, trace load ≤6 strokes a drawing, closed shapes, `guide_closes_where_filled` all pass.
8. No path is shared across the three.
9. Start's budget holds with `Follow a lesson`.

### Tests (`aurookie_`, at least 8)

`aurookie_three_stand`, `aurookie_up_late_file`, `aurookie_night_leak_recovers`, `aurookie_end_of_the_desk_file`, `aurookie_not_a_pencil_file`, `aurookie_show_me_alone_reaches_each_file`, `aurookie_trace_load`, `aurookie_shapes_close`, `aurookie_kinds_share_no_paths`, `aurookie_kinds_look_different`, `aurookie_poses_stay_on_model`, `aurookie_contact_sheet_writes`.

`aurookie_kinds_share_no_paths` alone would pass on the same drawing with another seed, so `aurookie_kinds_look_different` compares the three finishes at 64×64, IoU ≤0.45.

### Must not change

Any engine file. Any gate. Any main-spec number.

### Leave out

Amateur and Experienced lessons.

## AU24 Amateur lessons

After this part six lessons stand, and each type has a second, harder piece of work.

### Start from

The lessons file: A Coat of Shavings, Still in the Coat, Too Much Coat. No engine change.

### Build

1. Papers, `prep`, landmarks, stand-ins, finish pictures, the coat generator `shaving(seed)`.
2. Tables as the lessons file gives them.
3. File facts.

### Acceptance

1. The gate passes on all six in both ordinary modes.
2. No Amateur task carries a `Guide` path. `Hold to see it` shows the finish only while held.
3. A Coat of Shavings' PNG: 1400×1050, no yellow outside the fill's mask, the hidden sketch absent, Deckle's own line still present.
4. Still in the Coat's GIF: 32 frames with the stand-ins, the first and last pictures equal, the file loops forever.
5. Too Much Coat: 2 page files, 888×1243, at least 6 panels on page 1.
6. Select's move ticks through a raster rig. If `commit_floating` cannot carry the signal, say so and take the fallback in the lessons file.

### Tests (`auamateur_`, at least 6)

`auamateur_six_stand`, `auamateur_coat_file`, `auamateur_rims_stay_inside`, `auamateur_still_in_the_coat_file`, `auamateur_loop_closes`, `auamateur_too_much_coat_files`, `auamateur_hold_to_see_it_is_held`.

`auamateur_rims_stay_inside` runs on stand-in rims that overrun the wood by design, so it fails if the lock does nothing.

### Must not change

Any engine file, any Rookie lesson's sentence, any gate.

### Leave out

Experienced lessons.

## AU25 Experienced lessons

After this part all nine lessons stand.

### Start from

The lessons file: Plate One, Desk to Lamp, The Only Word. No engine change, apart from a code-made hum in `prep`, as `sample.rs` makes `dialogue_wav`.

### Build

1. Papers, `prep`, seeds, stand-ins. Desk to Lamp's seed: marked keys and the hum.
2. Tables as the lessons file gives them, with `Show me where` only.
3. File facts.
4. `lessons::ALL.len() == 9`, and the classes-differ check across all nine.

### Acceptance

1. The gate passes on all nine in both ordinary modes. The ignored real mode passes one per type.
2. No Experienced task has any path on the paper.
3. Plate One's PNG: 1500×2000, four corners alpha 0, no pixel where only the rough was.
4. Desk to Lamp: a PNG sequence and a GIF, a camera change between first and last, dust present, wing cells past row 3.
5. The Only Word: one mono page, 2591×3624, panels ≥5, tone on the TONE track, a balloon and the word `tink` in the graph.
6. No lesson needs ffmpeg, a file dialog for sound, or the Tab key. `Add effect` is the door to Particles.
7. Classes and types differ: IoU ≤0.45 on all 36 pairs of the nine, against the demo circle and against the sample's ball.
8. If `BrushMakerRoom` has no Rig route, take the fallback in the lessons file and say so.

### Tests (`auveteran_`, at least 6)

`auveteran_nine_stand`, `auveteran_plate_one_file`, `auveteran_desk_to_lamp_files`, `auveteran_only_word_file`, `auveteran_no_lesson_needs_ffmpeg`, `auveteran_classes_differ`, `auveteran_no_ring_until_asked`.

### Must not change

Any engine file, any earlier lesson's sentence, any gate.

### Leave out

A tenth lesson, a score, a certificate.

---

## Not verified

- **Nothing was compiled, run or timed.** All geometry is arithmetic from source. Real fonts may move row heights.
- The 60.9% in 1.1 assumes `as_first_run()` shows band 36 once ready paper's strip stays closed. The Fit block was read and worked through. It was not measured.
- Read at f3bb1dc and in the running tree of Keys and Hide tools, whose line numbers and `tools_hidden` code may still move. `pages_written` feeding `landed` is committed at f3bb1dc.
- `Panel::right` beside `DockArea` in this `ui` is assumed to behave as `Panel::top` and `Panel::bottom` do.
- `A drawing` plus two chips in one 188-wide card.
- `tab_style_override`'s margin in this egui_dock, read in the crate by the plan study, not tried. A white band left of the paper in `shipped-sheet-on.png` was not explained, and if it is art past the paper's edge it stays under the rail.
- Fill on page cels and on a Color step over a page. Read: `fill_input` has no page check, `Solo merged` has a `color` layer under `line`, and Color arms `color` by name. Not run. AU18 item 9 decides it.
- Whether panel borders reach a page file without `Clip to panels`. Read only: the template and `new_page` call `page_graph`.
- Glow over unpainted paper (`deliver` lays frames on white after the graph). Read, not run. Lessons are built around it.
- `commit_floating` as the place for the `Moved` signal. Whether a hidden layer and a `Rough` layer stay out of a plain PNG.
- `Keep inside strokes` on the color layer. Read, not run. Night shares that layer, which is why A Coat of Shavings lays its ground last.
- `make_inbetweens` and Lip flap on seeded data with drawn wings. Whether Dust reads as wing dust.
- Stroke, flood and composite cost on full Manga B5, and flood cost at `fill_gap` 8.
- Bits 17 to 24 of `hints_seen` are free in `say.rs`'s constants (0 to 14 and 16 are used), read in the running tree.
- `BrushMakerRoom` and `OpenLessons` do not exist yet. The `menu.*` chains in the lessons file follow main spec 6.2's final paths, not today's menus.
- `commit_path`'s keep-inside rule is a design from reading `commit_ring` and `PaintMode::AlphaLock`. The CPU result was not compared with the GPU's.
- `Eased` assumes the Motion curves pane writes `ParamInterp` or handles on a camera column. Read in `anim-core`'s curve tests, not through the pane.
- `strip.next` assumes the strip stands in Color on an animation with its sheets clickable, as R16 and `strip::tap` read. Not run with the new Color recipe.
- A `Played` pass on the Rig: `testkit` has a clock seam, and the live route runs on `PlayClock` when sound plays. The Done reads the frame only, which should serve both. Not run.
- The Rig's Select route for `Moved`. The lessons file keeps its fallback.
- Row counts assume the sheet head ends where the shot shows it. The sheet leaf is 170 to 210 wide by share at small windows, against a 240 floor read in the sheet. If the floor wins, Plan's paper at those sizes is narrower than 3.4 and 5.6 say. The gates still decide.
- Lessons under a real pen, with a guest connected, and with `gesture_safe` false.
- Uniqueness is what searches found, not proof. Four first-draft titles were real works or too close to one and were replaced: First Flight (DreamWorks short, 2006), Nothing Happens (the Kranots' short, 2017), TINK (Mr. Kaplin's short, 2018) and Over the Edge (one letter from Over the Hedge, and First Flight screened with it). The replacements returned no match of that exact title in four searches. "Deckle" returned no character. It is also a scavenger-hunt platform's name and a listing for a Mac app, in other fields, and a paper-making word. That is a web check, not a trademark search. The Japanese wordplay in hover cards needs a Japanese reader, and "shaku" as an anime term is confirmed only by the app itself.
- All lesson times are estimates. No person has done a lesson.
