# PSD: Approachable UI

| | |
|---|---|
| Status | Build document, written 2026-10-30, revised after review the same day. Nothing in it has been built yet. |
| Project | Gaban Studio (exe `animstudio`), Rust, eframe/egui 0.35, wgpu 29, egui_dock 0.20 |
| Branch | `approachable-ui`, local commits only, never pushed |
| From | v0.5.0, 828 headless tests |
| To | v0.6.0, 17 build parts, each buildable, reviewed and fixed in about 80 minutes. The aim was about ten; R20 says why it grew |
| The request | "I want the UI to be designed for a more approachable Software like autodesk sketchbook where the UI is substantially easier to navigate. Depricate the current themes in the software now. Optimize the User areas and workflow steps so someone can learn the software but also have it already optimized in the areas needed." (the Lead, 2026-10-30) |

**How to read this.** Sections 0 to 14 are shared. Every builder reads them, then reads its own `## AU<n>` section and nothing else. If a part section and a shared section disagree, the shared section wins and the builder notes the disagreement in its report. The last section, Rejected review notes, records the options the review weighed and did not take.

**Words used in this document.** `pt` is an egui point at 100% UI scale. The *top row* is the single row at the top of the window. The *rail* is the floating tool column on the left of the canvas. A *float* is a non-modal panel anchored over the canvas. The *band* is the bottom bar: the transport row plus, when shown, the strip. The *strip* is the simple timeline. The *peg bar* is the steel ruler at the top of the strip. The *Say line* is the one-sentence lane in the band's transport row. A *step* is one of the six workflow places that used to be called rooms. *Frozen boot*, *shipped boot* and *first-run boot* are the three test starts in 1.5. "Today" always means v0.5.0, commit `ebbc5fc`.

Code anchors are `file:line` into `app/src` on this branch as it stands, unless another path is given: commit `ebbc5fc` plus the test-only uishot harness, which adds two lines after `main.rs:50` and eleven after `testkit.rs:187`. Lines will drift as parts land; search by the quoted symbol.

---

## 0 The design on one page

Gaban Studio opens on white paper on a calm grey desk. A person can draw at once. Nothing has to be chosen, named or set up first.

- **One new look.** Light neutral desk, white paper, sentence-case IBM Plex Sans, rounded controls, one blue for "this is on". The memorable element is one steel peg bar over the timeline. The plate look (Graphite, Well, Legend, Struck, Tally, square corners, engraved caps, mono type, grain) and the Japanese-first room dressing are retired. There is no theme switcher and no classic option.
- **Six plain steps on the top row:** Plan, Draw, Timing, Color, Effects, Export. Each is the same document seen through a different arrangement. They map onto the existing stored rooms, so nothing saved changes.
- **Draw is canvas-first.** One flush canvas, a tool rail at the left, a simple timeline at the bottom, and floats that open on demand. Paper grows from 48.7% of the window to 63.7% at 1600×900. Tools float, so pen targets on the paper are measured against what really covers them, not against the biggest rectangle.
- **The simple timeline is a second view of the same exposures.** It reads the same `Column::runs` as the Timing sheet and writes through the same commands. Nothing is forked. The Timing sheet stays one visible click away.
- **Learning is built in, then gets out of the way.** A Start float over ready paper, one Say line that teaches one thing at a time and only when it applies, a five-dot Quick Start that ends in a real file, plain empty states, and one switch that turns it all off. Hints never cover art and never fade on a skill guess.
- **A beginner can finish.** New in-house GIF export, one-click PNG of this frame, plain Export step with a Done card. MP4 stays an ffmpeg add-on with a visible plain reason and a one-click route to GIF.
- **Nothing is lost.** Every verb stays reachable. The keyboard path of a working animator costs no extra keystroke: two chords move, press count unchanged, and they move only through a version-gated migration that never touches a user's own binding. The node graph, curves, render queue, board, episode, 3D, manga and brush forge are re-dressed and made easier to reach, not rebuilt.

### Measured promise

Two screens are measured. *Draw* is blank Draw with the Start float closed (`as_shipped()`), the screen a person works on. *First screen* is what a fresh install shows at launch, Start float open (`as_first_run()`). The first screen has more controls than today's form, because the whole editor is live behind the float. That is the design: a person can draw at once.

| Measure at 1600×900, blank project | Screen | Today | Target |
|---|---|---:|---:|
| True unoccluded paper share (16:9 document) | Draw | about 48.7% | 63.7% by arithmetic, gate at ≥63.0% |
| Same, with Start at its largest | First screen | n/a | 58.3% by arithmetic, gate at ≥58.0% |
| Canvas the pen can reach (`journeys.md` canvas share, target ≥80%) | Draw | 54.5% | 83.0%, gate at ≥82.5% |
| Visible controls | Draw | 72 | 29, gate at ≤30 |
| Visible controls | First screen | 19 | 41, gate at ≤44 |
| Words on screen | Draw | 100 | 37, gate at ≤40 |
| Words on screen | First screen | 74 | 62, gate at ≤70 |
| Jargon terms | Draw | 30 (9 Japanese) | ≤3, Japanese 0 |
| First-level menu rows | Any | 143 in 9 menus | 59 in 6 menus, gate at ≤60 and ≤6 |
| Clicks from launch to first stroke | First screen | 1, on a right guess | 0 |
| Start entries that land on a working stroke | First screen | 5 of 8 | all |
| Ball-bounce journey | Whole journey | 33 actions, no shareable file without ffmpeg | 27 actions, real GIF |
| Still journey overhead | Whole journey | 11 actions, 48 PNGs | 4 actions, 1 named PNG |
| Glow and pan journey | Whole journey | 24 actions, 4 wires, 1 room switch | 7 actions, 0 wires, 0 room switches |

Every target is by construction until its gate runs. See section 12.

The Today column was taken in the test harness, which frames its content 8 pt in from the window. The App has no such frame, and the targets are flush, as 4.1's arithmetic is. On AU1's grid, today's blank Draw reads 47.4% paper and 54.6% canvas in the harness's frame, and 49.5% and 56.5% flush to the window (`as_shipped()`). The 54.5% is the framed reading, and the 48.7% matches neither.

---

## 1 Shared decisions

### 1.1 The nine fixed decisions

These were decided before this document and were told to the Lead. Do not reopen them.

1. There is no theme switcher. "Deprecate the current themes" means retiring the plate visual language and the Japanese-first room dressing, replaced by ONE new pinned look. No "classic" option. Trade terms survive only as secondary hints and command palette aliases.
2. The Lead's request supersedes the ratified design laws in `research/DESIGN-SPEC-editor-ui.md` and `research/ROOM-CHARTERS.md` wherever they conflict. Section 1.3 lists what is superseded.
3. Aim for about 10 build parts. The biggest visible changes land first. Advanced panels are re-dressed and made easier to reach, not rebuilt. Every part leaves a working app with a green suite.
4. Nothing is lost: every existing verb stays reachable, and a working animator's keyboard path costs no extra keystroke. Sheet, graph and queue stay one visible click away. No forked Simple Mode.
5. Saved data is sacred: `Pane`, `CanvasTool`, `Action`, `stage_key` room keys, stack names and the `ui` config section's shape are stored vocabulary. Only display labels change. Never save an undrawn dock. `.animproj` files do not change.
6. Pin one egui theme and dress both style slots, so Windows Light mode cannot fall back to stock egui.
7. Tests are migrated deliberately through a shared vocabulary and test seams, never loosened. A pinned label may follow a rename the Lead asked for. A behavioural assertion may not weaken.
8. Tests and tools never open a window. Real-pen and real-GPU checks that cannot run headlessly are queued for the Lead as one batched list at the very end (target rig: XP-Pen Artist 22 Pro). No question goes to the Lead mid-plan: decide, record, continue.
9. Numbers before pictures: every design claim gets a measurable gate where possible.

### 1.2 Resolutions

Where the proposals disagreed, this is the answer and why. The chassis comes from the risk proposal (best judged), the learner spine from the steps proposal, the ready paper and pen ideas from the canvas proposal.

| # | Question | Decision | Why |
|---|---|---|---|
| R1 | Chassis | One always-drawn `DockArea`. A one-tab dock draws flush (no tab bar, no margin). Rail, floats and band sit around it. | A dock is never swapped out, so saved docks and floats keep working and an undrawn dock can never be saved. Fallback to a bare host only if the kill test in AU3 fails. |
| R2 | Step names | Plan, Draw, Timing, Color, Effects, Export | Decision 1 forbids keeping trade words as the primary name. "Layout" collides with Reset layout. The pro substance stays: Plan keeps the Layout recipe, Effects keeps the graph large and at the front (tab named `Effects graph`, so the depth is not undersold). |
| R3 | Step order | Timing before Color | Real trade order: line test before paint. A gate pins the display order and that hints never point backward. |
| R4 | Export as a step | New `Stage::Export`, off `Stage::ALL`, with no stored key | Export changes the arrangement, never the tool, layer or view. The other steps need no new stored room. |
| R5 | A Next pill | None. The Say line's single action button appears only while a hint shows. No numerals on steps. | One voice, no extra chrome, nothing to count. |
| R6 | Radial tool ring | Not built. Its contract is recorded in 6.1 and offered in the end list. | The rail and keys meet every journey gate. Press-and-hold behaviour on Windows pens is unmeasured. |
| R7 | Menus | Five text menus plus More: File, Edit, View, Animation, Help, More | 59 first-level rows against a ceiling of 60. |
| R8 | Arrows | One meaning everywhere: Left/Up previous frame, Right/Down next frame | Today 0 of 4 arrows do anything. They become `default_also` chords, so nothing is displaced. |
| R9 | Pointer-dependent chords | `ColumnKindBook` moves to Alt+O, `ColumnKindBg` to Alt+W, both become `pane_only` | Alt+K and Alt+B each mean two things depending on the pointer. One verb, one key. Press count is unchanged. |
| R10 | Add drawing chord | Alt+A, new `AddDrawingAfter` | Free in both keymaps, same in both, A for Add. E and N stay `NewDrawing`. |
| R11 | Beginner export | Build a GIF writer in-house over `weezl`, a one-click still PNG, and a plain MP4 reason | See 5.5. `weezl` is already compiled into the app through `tiff`; no `gif` crate is available offline. |
| R12 | First run | Ready paper plus a non-modal Start float, owned by `Editor`. Its entries act through one seam, `apply_start` | The Rig drives `Editor::ui`, so the float is testable, and `apply_start` means a test lands a stroke on the document an entry really builds. Shortcuts and the pen stay live. See 8.1. |
| R13 | Step memory | Left out. Entering a step resets to its recipe as today. | Saved arrangements already cover the durable case. Less state to lose. |
| R14 | Hide tools | T hides top row, band, rail and floats in any step, leaving one return dot. Never persisted. Refits the paper once. | Sketchbook's cleanest idea and the first thing cut if time runs out. |
| R15 | Tooltips near the pen | Own hover card built on a non-interactable `Area` | Stock tooltips block pen-down (448 of 15,000 samples measured). `Area::interactable(false)` passes the pen and is skipped by `layer_id_at`. |
| R16 | Strip visibility | Hidden if and only if the sheet is on screen, or `!strip_open`, or in 3D and Brush maker | A single predicate. No auto-hiding by path or skill. |
| R17 | Growing chrome | Two latches that only ever widen in a session: `strip_open`, and the full-band latch from the second drawing. The strip sits above the transport row, which is always the last 36 pt of the window | The full-band latch fills reserved slots, so nothing moves. `strip_open` grows the band from 36 to 92: one accepted refit of the paper, at open or on Add drawing, never during a stroke. No rail or transport control moves. Adaptive bars are rejected. |
| R18 | Upgraders | They get canvas-first Draw too. One click on `Timing sheet` brings the sheet back and it stays. What's New says so once. | The Lead asked for this and must judge the real thing on his own config. |
| R19 | Holds in Draw | Stop while the sheet is off screen settles the playhead on the run start. With the sheet on screen, stop stays frame-exact. | A stroke on a held frame is not refused: it makes a new drawing and splits the run (`doc.rs:859-861`), which is right for frame-by-frame work. The trap is a stop in the middle of a hold: the frame looks blank with onion on, Layers is empty, and fill, draw to fill, transform and refill refuse there. Held and blank frames each get their own refusal sentence through one seam (8.2). |
| R20 | Number of parts | Seventeen, not ten | Five parts of the eleven-part map were each several deliveries, too much for an 80-minute part with review, and scope is not cut. The look lands in the second part, canvas-first Draw by the fourth, the timeline in the fifth, the guided start in the sixth and seventh. See 14. |
| R21 | Motion | None added. `reduce_motion` does not exist. | Less to test, less to distract. Floats use `fade_in(false)`. |
| R22 | `Lock UI positions` | On for fresh installs only, with a View row (`ToggleLockPositions`) and a Settings row to undo | Protects a learner from dragging a dock apart. A panel opened by hand keeps its close control (4.6). A working animator's existing config keeps its value. |

### 1.3 What this supersedes

**`research/DESIGN-SPEC-editor-ui.md`**

| Line | Law | Now |
|---|---|---|
| 1 | Ratified status | Superseded where listed here, in force elsewhere |
| 14, 289 | Labels never hide behind hover | Depth moves into floats and disclosures. The rail keeps visible captions. |
| 25-26 | Colour tokens | Section 2 |
| 48 | Caps engraved legends | Sentence case |
| 94 | Two-ink test | One blue, one red, by role |
| 110 | Owner's identity amendment | Replaced by the new look |
| 133 | Slate tabs | Step pills |
| 138 | "dock is not touched" | Style changes. The host stays. |
| 224-230 | THE RULE | Replaced by the one-look rule in section 2 |
| 277-283 | No transport in a pane, no de-docking, no animation | Transport stays in the foot and no animation is added. De-docking is allowed for the rail and floats. |

**`research/ROOM-CHARTERS.md`**

| Line | Law | Now |
|---|---|---|
| 3 | anim-core untouched | GIF writer and delivery additions are added to it. Nothing else changes in it. |
| 8 | Japanese first | Plain English first, studio word second |
| 9 | Left = time | Time runs along the bottom in Draw |
| 11 | Colour law | Section 2 |
| 15 | Chassis law | Section 4 |
| 20, 125, 215 | Genga, Shiage, Henshū charters | Section 5 |
| 108 | Stage-lensed bar | The idea survives in the band |

**Kept from the charters:** "capability is never removed, only unfronted", "one verb one key", "a room is a lens".

**Code comments that become false** and change in the part that makes them false: `plate.rs:1-13, 348-350, 931-933, 954, 962, 980-988, 1001`; `workspace.rs:114-115`; `canvas.rs:5148`; `xsheet_panel.rs:1627` where it conflicts; `templates.rs` module doc D13 ("72- or 120-frame cuts"). Stale today and fixed anyway: `welcome.rs:542`, `config.rs:4130-4134`.

### 1.4 Data rules

These apply to every part.

| Thing | Rule |
|---|---|
| `.animproj` | No change. Schema stays 11. It stores only `app.palettes`, `app.thumb`, `app.episode.target` and `prod.*` beyond the engine data. |
| `Pane` | No new variant. `Pane::Brush` stays as a tombstone and the runtime removal loop `main.rs:8286-8291` stays. |
| `CanvasTool`, `SelShape` | All variants stay. Hand is a held state, not a tool. |
| `Action` | Additions only. No rename, no removal. New actions reach old configs through `fill_missing`. Old actions change default only through the migration in 6.5. |
| `stage_key` strings | `layout`, `drawing`, `finishing`, `composite`, `edit`, `manga`, `custom` are unchanged. `Stage::Export` has an `export` arm for exhaustiveness and the string is never written or looked up. |
| Stack names | Matched by string at `config.rs:2364-2367`. Display map only. |
| Preset names | Re-inserted by name at `config.rs:2896-2899`. Display map only. |
| `CutStatus::key`, FX names `crates/anim-core/src/fx.rs:746-761`, `AO`/`AKA` document uses | Never renamed or recoloured. Display map only where a label changes. |
| `LayerProps.chip` | Stored in the document as `[u8; 4]`. `identity_swatches` (`xsheet_panel.rs:2019-2034`) builds its rings from tokens today, so a token change would silently change what gets stored and stop old documents' rings matching (`:2100`). New `CHIP_STRUCK`, `CHIP_LEGEND`, `CHIP_PAPER`, `CHIP_AO` and `CHIP_AKA` in `plate.rs` hold today's literal values. |
| `ui` config section | Additive fields only, each with field-level `#[serde(default)]`: `sheet_in_draw: bool`, `result_in_color: bool`, `panel_more: BTreeMap<String, bool>`, `keymap_rev: u32`, `hints_seen: u32`, `hints_off: bool`. `quick_start` stays `[bool; 5]`. |
| `workspaces.json` | Format unchanged. Any new `WorkspaceView` field needs `#[serde(default)]` (`workspace.rs:320-330`). |
| Dock saves | `Workspace::normalised()` runs in `Workspaces::save_to` (`workspace.rs:403`) and at the host send (`main.rs:2085-2097`). A dock that has never been drawn has `null` rects and fails to load, so no save path may write one. |
| Wire | No `MIN_PEER_VERSION` bump (`net.rs:231`). No new `Pane`, so a 0.5.0 peer can read the host layout. Pin `egui_dock` with `=0.20.1` so the serialised dock cannot drift. |
| Render presets | `PRESETS` (`crates/anim-core/src/deliver.rs:1000`) is append-only, because jobs hold an index. Tokens keep `{cut}` (`deliver.rs:1116, 1133`); `{shot}` becomes an alias. |
| Secrets | The `session` config section holds `api_key` and `totp_secret`. Goldens and fixtures are built without them, never copied from a real config. |
| Downgrade | Not promised. An older build is expected to skip unknown bindings, but that was read, not run. |

### 1.5 Test rules

1. **The suite stays green at the end of every part** and the count never falls: 828 plus everything added.
2. **Three boots, one funnel.** Every `EditorHarness` constructor ends in one funnel (AU1), so no test can start outside this rule. The *frozen boot* is `new()`, `with_state()`, `with_dir()`, `with_ppp()` and `with_gpu()`: Timing sheet on in Draw, Result beside in Color, disclosures open, hints off, positions unlocked, no room arming. Everything frozen is set in one function, `EditorHarness::frozen()`, so the list is readable in one place. The *shipped boot* is `as_shipped()`, `with_state_shipped()` and `with_dir_shipped()`: a fresh install with the Start float closed. The *first-run boot* is `as_first_run()`: what a fresh install's first launch shows, Start float open. Every new default gets at least one test under a shipped or first-run boot. `ship_` persona tests move to `as_shipped()` in the part that changes their path. The frozen boot exists so that a change of default does not disturb tests about something else. It never hides a behaviour change.
3. **Labels go through `vocab`.** A test that pins a display word uses `vocab::X`. The flip is three steps: move the test to `vocab::X` with the value unchanged, change one line in `vocab.rs`, and `auseam_vocab_table_is_pinned` asserts the table. 15 trade-word tests stay unedited, because their alias still has to work.
4. **Behavioural assertions never weaken.** A moved test keeps every assertion. Each moved test is listed in the part's commit body with one reason: `label`, `reach` or `recipe`. Thresholds and gates are never lowered. If a gate misses, find the cause.
5. **No window, ever.** GPU work runs only in ignored tests, so the 828 stay GPU-free. Pixel checks live in the ignored pass. `render` needs a device, so style equality is what the normal suite checks. The Rig has no paint layer, so `canvas.raster` is false and a fill is refused there (`canvas.rs:6619-6625`). An ordinary test never scripts a fill through that refusal: it uses the CPU route (`fill_rig`, `canvas.rs:9688`), or it is an ignored `with_gpu` test that section 10 marks `(ignored)`. Each part's ignored run includes its prefix.
6. **Every part ends with the ignored shot run** with `UISHOT_DIR` set to `C:/Users/Lancer/Desktop/LsXavecs AGI/.work/gaban-ui/shots-after/`, under the same names as `shots-before/`.
7. **Ratchets stay on:** `COLOUR_LITERALS_PINNED = 0`, the numeric chip check, `genui_glyph_coverage`, `ship_every_action_is_described_placed_and_clash_free`, `tidy_sizes_come_from_the_templates`, `test_counts_hold_in_source` (additions are safe, it asserts ≥). The assert-count ratchet from AU1 applies to every part.
8. **One test prefix per part.** Tests a part adds start with its prefix, so `cargo test <prefix>` runs that part's work. In the `animstudio` crate new tests live next to what they test or in `testkit.rs`; in `anim-core` they live in `crates/anim-core/tests/`.
9. **Spoken names are unique.** `click` and `hover` panic on duplicates (`testkit.rs:234-241, 248-249`), and `get_by_label` too. Use `plate::speak` to separate a toolbar control from a menu row: `Undo (toolbar)`, `Redo (toolbar)`, `Show Timing sheet` and `Hide Timing sheet`, `Ghost frames toggle`, `Brush color`, `Export queue`, `Add drawing (band)`, `Close Start`.
10. **Dialogs and OS calls go through `AppRequest`.** The App runs them, the Rig logs them, tests assert the log. A test writes real output by calling the seam behind the dialog with a temp path: `h.answer_export(path)` calls `Editor::export_to`, and `h.answer_start(action)` calls `apply_start` (8.1). So the Rig ends with a real document and a real file.
11. **One stroke signal.** The real app never calls `commit_stroke` for a brush stroke: `canvas.rs:7378-7381` returns first and the raster commit is `commit_raster` (`canvas.rs:4410`), while the harness forces `raster = false` (`canvas.rs:3820`) and only ever runs the vector route. So anything that reacts to a finished stroke reads one counter, `AppState.strokes_done`, which `Editor` compares once a frame. `commit_stroke` and `commit_raster` bump it on success. A paste is not a stroke: `paste_pixels` calls the private `land_raster` that `commit_raster` wraps. A fill is not a stroke. Every test of a stroke outcome runs both routes: vector through the harness, raster through `paint_texels` (`canvas.rs:9646`). One ignored `with_gpu` test runs a real stroke.
12. **A journey with a refusal is a failed journey.** The `Recorder` fails the script, naming the step, if `refusal_seq` moves or a perform returns `Performed::Ignored`. Room arming runs in `Editor::arm_room` (5.3), so a Rig under a shipped boot arms exactly what the App arms.

---

## 2 Visual system

### 2.1 Concept

**A neutral desk, white paper, one steel peg bar, everything else quiet.** The whole window is one desk surface. The top row and band have the `DESK` fill with no rule under them. Controls sit on it in pale islands. The art is the only saturated thing in the window, so a colour judgement is never polluted by the chrome. The peg bar is the one thing someone remembers: a dark steel ruler above sheets of paper with one round peg hole each, the animator's real tool drawn plainly. It also appears as the rule on the Start float. It is the only dark surface in the chrome.

### 2.2 Colour tokens

All tokens are `Color32` constants in `plate.rs`. Named by role, not by material. No other colour constant may appear outside `plate.rs`; `COLOUR_LITERALS_PINNED = 0` enforces this.

| Token | Hex | Use |
|---|---|---|
| `DESK` | `#D3D5D8` | Window, top row, band, tab bars, canvas pasteboard |
| `ISLAND` | `#F6F7F8` | Docked panels, rail, floats, menus, step island |
| `WELL` | `#E8EAED` | Fields, filled button rest, ghost button hover |
| `HOVER` | `#E0E3E7` | Filled button hover, hovered row |
| `PRESSED` | `#CDD1D6` | Filled button pressed |
| `PAPER` | `#FFFFFF` | The paper, strip sheets. Screen-only (see 2.4) |
| `INK` | `#1E2227` | Primary text |
| `INK2` | `#4C525B` | Secondary text, icons at rest |
| `INK3` | `#70767F` | Dim words outside a disabled scope, field edge, swatch ring, dashed empty sheet, page outline on paper |
| `RULE` | `#BFC3C8` | Decorative hairlines only |
| `BLUE` | `#1F5FBF` | Armed tool fill, the one primary, focus ring, links |
| `BLUE_DEEP` | `#184C9A` | Primary hover, current words, selected text, playhead |
| `BLUE_WASH` | `#DCE8F9` | Current step, selected row |
| `RED` | `#B92D1C` | Refusal words, danger fill |
| `RED_WASH` | `#FBE5E1` | Refusal pill |
| `STEEL` | `#2A2E34` | The peg bar |
| `STEEL_INK` | `#E6E8EB` | Peg bar labels |
| `RESULT` | `#808080` | Surround of the Result pane only, so a colour judgement sees neutral grey |
| `SHADOW` | `#14181F` | Shadow colour, with the alphas in 2.6 |

Rejected along the way: sage `#B4C2B9` and `#BBBFBC` as the desk (`INK2` fails on them).

**Blue and red are equal in luminance** (0.1224 and 0.1227, 1.0:1 apart). So state is never colour alone. A refusal is always words plus a mark. Current is wash plus deep ink. Armed is a filled shape.

### 2.3 Contrast check

Computed with WCAG 2 relative luminance over the tokens above: 44 pairs, 0 failures. `aulook_contrast_pairs_hold` repeats this exact table in Rust. Text needs 4.5, disabled content, dim words and non-text controls need 3.0. A row `X at 0.65` is `X` blended over the background at `disabled_alpha`. Rows marked decorative carry no information alone.

| Foreground | Background | Ratio | Need | Use |
|---|---|---:|---:|---|
| `INK` | `DESK` | 10.87 | 4.5 | Menu words and step names on the top row |
| `INK2` | `DESK` | 5.36 | 4.5 | Counter and Say line on the band |
| `INK3` | `DESK` | 3.11 | 3.0 | Dim words on the desk |
| `INK` | `ISLAND` | 14.91 | 4.5 | Tool captions, float text, panel text |
| `INK2` | `ISLAND` | 7.35 | 4.5 | Icons at rest, secondary float text |
| `INK3` | `ISLAND` | 4.27 | 3.0 | Dim words on an island |
| `INK` | `WELL` | 13.27 | 4.5 | Field text |
| `INK2` | `WELL` | 6.54 | 4.5 | Placeholder text |
| `INK3` | `WELL` | 3.80 | 3.0 | Dim words in a field |
| `INK` | `HOVER` | 12.42 | 4.5 | Words on a hovered button |
| `INK2` | `HOVER` | 6.12 | 4.5 | Icon on a hovered button |
| `INK` | `PRESSED` | 10.42 | 4.5 | Words on a pressed button |
| `INK2` | `PRESSED` | 5.14 | 4.5 | Icon on a pressed button |
| `INK` | `PAPER` | 15.99 | 4.5 | Strip sheet number |
| `INK2` | `PAPER` | 7.88 | 4.5 | Secondary on a sheet |
| white | `BLUE` | 6.09 | 4.5 | Armed tool, Export primary |
| white | `BLUE_DEEP` | 8.27 | 4.5 | Hovered primary |
| `BLUE` | `ISLAND` | 5.68 | 4.5 | Links, focus ring on islands |
| `BLUE` | `PAPER` | 6.09 | 3.0 | Selection outline on paper |
| `BLUE` | `DESK` | 4.14 | 3.0 | Focus ring on the desk |
| `BLUE_DEEP` | `BLUE_WASH` | 6.68 | 4.5 | Current step words, selected row |
| `INK` | `BLUE_WASH` | 12.91 | 4.5 | Selected row body words |
| `BLUE_DEEP` | `DESK` | 5.62 | 3.0 | Playhead on the desk |
| `BLUE_DEEP` | `PAPER` | 8.27 | 3.0 | Current sheet outline |
| `RED` | `ISLAND` | 5.67 | 4.5 | Refusal words on islands |
| `RED` | `DESK` | 4.13 | 3.0 | Refusal mark on the desk |
| `RED` | `RED_WASH` | 5.04 | 4.5 | Refusal pill words |
| white | `RED` | 6.08 | 4.5 | Danger fill |
| `STEEL_INK` | `STEEL` | 11.12 | 4.5 | Ruler seconds on the peg bar |
| white | `STEEL` | 13.65 | 3.0 | Ruler ticks |
| `STEEL` | `DESK` | 9.28 | 3.0 | Peg bar against the desk |
| `INK3` | `ISLAND` | 4.27 | 3.0 | Input edge, swatch ring on an island |
| `INK3` | `WELL` | 3.80 | 3.0 | Input edge on a field fill |
| `INK3` | `PAPER` | 4.58 | 3.0 | Dashed empty sheet on paper |
| `INK3` | `DESK` | 3.11 | 3.0 | Dashed empty sheet on the desk |
| `INK3` | `PAPER` | 4.58 | 3.0 | Manga page outline |
| `INK` at 0.65 | `ISLAND` | 4.88 | 3.0 | Disabled content on an island |
| `INK` at 0.65 | `WELL` | 4.65 | 3.0 | Disabled content in a field |
| `INK` at 0.65 | `DESK` | 4.26 | 3.0 | Disabled content on the desk |
| `RULE` | `ISLAND` | 1.65 | decorative | Hairline |
| `WELL` | `ISLAND` | 1.12 | decorative | Field fill, always with its `INK3` edge |
| `ISLAND` | `DESK` | 1.37 | decorative | Island edge, the shadow does the work |
| `PAPER` | `DESK` | 1.47 | decorative | Paper edge, the shadow does the work |
| `HOVER` | `ISLAND` | 1.20 | decorative | Hover fill |

**Disabled content.** egui dims a disabled scope by `disabled_alpha`, through `Ui::disable` and `multiply_opacity` (`ui.rs:496-502`), and the default 0.5 leaves `INK` at 2.87 on `DESK`. So 2.9 sets 0.65. At 0.65 `INK` holds on all three fills, while `INK2` (2.71 on `DESK`) and `INK3` (1.99) do not. So content inside a disabled scope is always `INK`. `INK3` is only for words painted dim with no disabled scope around them.

### 2.4 Old tokens to new

Old names stay as aliases from AU2 to AU16, so 870 references in 23 files do not all move at once. AU16 deletes the aliases.

| Old | New alias target | Note |
|---|---|---|
| `GRAPHITE` | `ISLAND` | |
| `WELL` | `WELL` | Value changes |
| `LEGEND` | `INK2` | |
| `STRUCK` | `INK` | |
| `AO` | `INK3` for UI | Document uses get new `PENCIL_AO` and `PENCIL_AKA` holding the old values, at `canvas.rs:35-36` and `config.rs:2134, 2174`, so `doc.rs:6613 lighttable_onion_tints_are_unchanged` passes unedited |
| `AKA` | `RED` | |
| `TALLY` | `BLUE` | Fix every `STRUCK`-on-`TALLY` pair, which would become `INK` on `BLUE` (use white) |
| `PAPER` | `PAPER` | Value changes. Screen-only: `canvas.rs:3972`, `viewer.rs:261`, `floatwin.rs:138`. There is no `plate` reference in `export.rs`, `paint.rs`, `graphcomp.rs` or `floatcanvas.rs`, so exported pixels do not change. |

`role_chip_color` Highlight maps to `PAPER` (`canvas.rs:7429`) and needs an `INK3` ring now that paper and island are close. The stray colour arrays at `forge.rs:512` and `canvas.rs:2135` come under the ratchet.

**Marks on paper.** The aliases flip light and dark. `GRAPHITE` was a dark ink on pale paper, and as an alias it is `ISLAND`, which is 1.07:1 on `PAPER`. So AU2 greps every `GRAPHITE`, `WELL`, `STRUCK` and `LEGEND` use painted on paper or used as a stroke and reviews it once. The sites known now:

| Site | Today | Now | Why |
|---|---|---|---|
| `canvas.rs:3970` miss check paper | `GRAPHITE` | `STEEL` | Miss check needs dark paper, or the gaps it looks for vanish |
| `canvas.rs:3996` veil in miss check | `WELL` | `STEEL` | Same |
| `plate.rs:142` `paper_veil` | Graphite at α110 | `STEEL` at α110 | It must stay a dark veil. Tests read the token (`canvas.rs:9974-9999`) |
| `manga_tools.rs:834` page thumbnail outline | `GRAPHITE`, 1 pt | `INK3` | 4.58:1, pinned in 2.3 and by `aulook_manga_page_outline_shows_on_paper` |
| `manga_tools.rs:878` `paint_page` guides | `legend_dim()` | `INK3` | Translucent ink goes, see below |
| `manga_tools.rs:827` trim | `LEGEND` | Alias, `INK2` | 7.88:1 |
| `canvas.rs:3974, 4001` paper edge, frame edge | `legend_dim()`, `LEGEND` | `INK2` | 7.88:1 |
| `nodegraph_panel.rs:652` node body | `GRAPHITE` | Alias, `ISLAND` | Not on paper |
| `canvas.rs:4944` shift-trace chip | `WELL` | Alias, `WELL` | A chip with its own fill |

**Translucent ink.** `legend_dim()` is `LEGEND` at alpha 115 (`plate.rs:114`) and is a text colour at 90 sites, so its real contrast depends on what is behind it. It becomes opaque `INK2`. Its roughly 61 non-text uses (strokes, frames, grips) move to new `plate::hairline()`, which is `RULE`. `legend_muted()` (`plate.rs:160`, node graph idle pins and wires) becomes opaque `INK3`. `rule_beat` and `rule_half` stay decorative, computed from `INK2`.

### 2.5 Type

IBM Plex Sans only, sentence case everywhere. No letterspacing. `plate::legend` stops calling `to_uppercase`. The Japanese fallback in `font_definitions` stays, because studio words appear in hover cards and aliases.

| Role | Size | Weight | Used for |
|---|---:|---|---|
| Title | 24 | SemiBold | Start float heading |
| Card | 20 | SemiBold | Start cards, Done card heading |
| Heading | 16 | SemiBold | Float and dialog headings |
| Chrome | 14 | Regular | Menu words, band words |
| Step | 14 | SemiBold | Step pills |
| Panel | 13 | Regular | `TextStyle::Body` and `Button`, panel text |
| Small | 12 | Regular | The floor. Rail captions, peg bar labels |

- `TextStyle::Monospace` becomes Plex Sans 13 Proportional. `drag_value_text_style` becomes `Body`.
- `FontFamily::Name("mono")` keeps Plex Mono at 12, used only for file patterns.
- `plate::legend` moves to 12 or 13 pt. Direct `FontId::monospace` calls and the 132 hard-coded sizes move to roles in AU12.
- If the painted median is under 10 px after AU12, `Body` goes to 14. The gate is never loosened.

### 2.6 Shape, shadow, spacing

**Radii.** Dialogs 14, floats and rail 12, menus 10, controls 8, inputs 6, strip sheets bottom corners 4 and top 0, paper 0, pills full stadium.

**Shadows** are for floating things only. All use `SHADOW`.

| What | Offset | Blur | Alpha |
|---|---|---:|---:|
| Rail and floats | [0, 6] | 24 | 16% (41) |
| Menus | [0, 4] | 16 | 18% (46) |
| Dialogs | [0, 12] | 40 | 28% (71) |
| Paper | [0, 2] | 10 | 16% (41) |

**Spacing,** base 4.

| Item | Value |
|---|---|
| `button_padding` | [10, 6] |
| `item_spacing` | [6, 4] |
| `window_margin` | 12 |
| `menu_margin` | 6 |
| `indent` | 16 |
| Top row padding | 6 vertical, 8 horizontal |
| Gap between groups | 16 |
| Float inner margin | 12 |
| Rail inner margin | 4 |
| Rail button | 48×48, icon 22, caption 12 pt |
| Chrome control height | 32 |
| Panel control height | 28 |
| Start card | ≥44 high |
| Timing sheet row density | Unchanged |

Fields are `WELL` with a 1 pt `INK3` edge. Small controls carry no strokes, because a stroke per chip costs vertices the performance gate counts.

### 2.7 Icons

Outline icons, 1.75 pt round stroke on a 24 box, one colour, drawn through the existing mask route (`icons.rs:28`, `mask_texture` `icons.rs:112`). Add a 96 px cut to the size loop at `tools/iconforge/src/main.rs:44`. New masks: undo, redo, search, export, layers, ghosts, sheet, hand, flip, gif, video, image, hide. Rail icons 22, bar and band icons 20. Tint: `INK2` at rest, `INK` on hover, `BLUE_DEEP` on wash, white on `BLUE`, `INK` through `disabled_alpha` when disabled. Every icon-only control has a hover card and a spoken name.

### 2.8 States

| State | Fill | Content |
|---|---|---|
| Ghost at rest | none | `INK2` |
| Ghost hover | `WELL` | `INK` |
| Ghost pressed | `BLUE_WASH` | `BLUE_DEEP` |
| Filled at rest | `WELL` | `INK` |
| Filled hover | `HOVER` | `INK` |
| Filled pressed | `PRESSED` | `INK` |
| Current (step, tab, row) | `BLUE_WASH` | `BLUE_DEEP` |
| Armed tool | `BLUE` | white |
| Primary (at most one on screen) | `BLUE`, hover `BLUE_DEEP` | white |
| Focus | 2 pt `BLUE` ring, 2 pt offset | unchanged |
| Disabled | none | `INK` through `disabled_alpha` 0.65. The disabled scope does the dimming, so a control painted by hand uses `INK` there, never a dimmer ink. The reason comes from `can_perform` and shows in the Say line on hover or click |
| Refused | `RED_WASH` | `RED` words plus mark, 4 s dwell |

**At most two solid blue things on screen:** the armed tool and one primary. The Export pill is that primary until it is current, when it takes the wash like any step. Disabled controls stay disabled and visible: they never disappear.

### 2.9 egui style and the theme pin

`plate::visuals()` (`plate.rs:934-993`) starts from `Visuals::light()` and sets:

| Field | Value |
|---|---|
| `panel_fill`, `window_fill` | `ISLAND` |
| `extreme_bg_color`, `faint_bg_color`, `code_bg_color` | `WELL` |
| `hyperlink_color` | `BLUE` |
| `warn_fg_color`, `error_fg_color` | `RED` |
| `selection.bg_fill` | `BLUE_WASH` |
| `selection.stroke` | 1.0 `BLUE_DEEP`. This is also selected widget text colour |
| `window_stroke` | NONE |
| `disabled_alpha` | 0.65, see 2.3 |
| `widgets.noninteractive` | bg `ISLAND`, fg `INK`, bg_stroke `RULE` |
| `widgets.inactive` | weak_bg `WELL`, fg `INK`. A disabled `Button` paints from this slot, because its `Sense` stays interactive (`style.rs:1267-1276`), so it must hold at `disabled_alpha`. The ghost's `INK2` at rest is painted by `plate`, not here |
| `widgets.hovered` | `HOVER`, `INK` |
| `widgets.active` | `PRESSED`, `INK` |
| `widgets.open` | `BLUE_WASH`, `BLUE_DEEP` |
| Corner radii | controls 8, windows 12, menus 10 |
| `window_shadow` | [0, 6], blur 24, alpha 41 |
| `popup_shadow` | [0, 4], blur 16, alpha 46 |

`dress` (`main.rs:181-187`) calls:

```rust
ctx.set_theme(egui::Theme::Light);
ctx.set_style_of(egui::Theme::Light, style.clone());
ctx.set_style_of(egui::Theme::Dark, style);
plate::install_fonts(ctx);
// zoom_with_keyboard stays false
```

Also: `selectable_labels` off, `tooltip_delay` 0.35. The grain is gone: `plate::surface` and `surface_alpha` become a flat fill and `PLATE_GRAIN_PNG` is no longer used. All derived colours (`plate.rs:114-185`) are computed from the new tokens, and 2.4 lists the ones that change kind. `kittest` pins Dark, which is why the style goes in both slots: the gate sets `input_mut().system_theme = Some(Theme::Light)` and asserts `panel_fill` equals `ISLAND`.

### 2.10 Motion

None added. Floats are `fade_in(false)`. Nothing slides, bounces or pulses. A float appears at once and never moves art.

---

## 3 Vocabulary

**Rules.** "Drawing" is art, "frame" is time: `Add drawing`, `Frame 3 of 12`. UI spelling is "Color", and "colour" stays in saved strings and comments. Every old word stays a Search alias in `Action::aliases` (`config.rs:1022`). Trade terms appear only in the hover card's second line, as `Studio word: satsuei`, and as aliases. `vocab.rs` is shared by shipped code and tests, and this table is the pin.

| Today on screen | New display word | Stored id, unchanged |
|---|---|---|
| room | step | `stage_key` |
| network room | shared session | |
| workspace | arrangement | `workspaces.json` |
| layout | Plan | `layout`, `Stage::Layout` |
| genga (drawing) | Draw | `drawing`, `Stage::Drawing` |
| shiage (ink & paint) | Color | `finishing`, `Stage::InkPaint` |
| satsuei (composite) | Effects | `composite`, `Stage::Composite` |
| henshū (edit) | Timing | `edit`, `Stage::Edit` |
| manga (pages) | Manga | `manga`, `Stage::Manga` |
| Brush Forge | Brush maker | `Pane::Forge` |
| 3D layout | 3D layout | `custom` |
| cut | shot | `{cut}` token, `Cut` |
| cel | drawing | |
| column | track | `Column` |
| X-Sheet | Timing sheet | `Pane::XSheet` |
| onion | Ghost frames | `onion` |
| ATARI, GENGA, SHUSEI pencils | Sketch pencil, Clean pencil, Correction pencil | Preset names |
| lasso fill | Draw to fill | `LassoFillTool` |
| alpha lock, lock | Keep inside strokes | `ToggleAlphaLock` |
| comp, composite view | Preview final | `composite_view` |
| HARDNESS | Edge softness | |
| aliased | Pixel edges | |
| STABILIZER | Steady stroke | |
| COLUMNS | Tracks | |
| SHAKU | Length | |
| MEMO | Notes | |
| library | Drawings | |
| Cel Layers | Layers | `Pane::Layers` |
| Presets | Brushes | `Pane::Presets` |
| stack | Layer set | Stack names |
| ARMED | Retired. `Drawing with` in the hover card | |
| Render Queue | Export queue | `Pane::RenderQueue` |
| Node Graph | Effects graph | `Pane::NodeGraph` |
| Build satsuei graph | Full finishing setup | `BuildSatsuei` |
| Build composite from sheet | Rebuild from layers | `BuildComposite` |
| Viewer | Result | `Pane::Viewer` |
| Layout canvas | Preview | |
| Curves | Motion curves | `Pane::Curves` |
| EDIT MODEL | Edit colors | |
| role | Color name | |
| line guard | Protect line art | `line_guard` |
| LIFT KEY | Remove from timeline | |
| Shinkō board | Shots | `Pane::Board` |
| Henshū timeline | Episode timeline | `Pane::Episode` |
| bind | Animate | |
| T.U., T.B. | Zoom in, Zoom out | |
| hold +, hold − | Longer, Shorter | `HoldPlus`, `HoldMinus` |
| Solo merged | One layer | Stack name |
| Layout (LO) | Rough over reference | Stack name |
| Genga stack | Clean line over rough | Stack name |
| Shiage (full) | Full paint | Stack name |
| H, L | Eye and lock icons, spoken `Hide track`, `Lock track` | |
| Next key, Previous key | Next drawing, Previous drawing | `NextKey`, `PrevKey` |
| scrub | Sound while dragging | `ToggleScrub` |
| in, out | Loop start, Loop end | `LoopIn`, `LoopOut` |
| loop | Loop | `ToggleLoop` |
| 24.000 | fps, as `24 fps` | `fps` |

**Kept:** select, fill, eraser, fit. Key pose, Breakdown and Inbetween stay, inside `Mark as ▸` only.

**FX display map** (stored names at `crates/anim-core/src/fx.rs:746-761` stay): Diffusion shows as Soft focus, Grade as Color look, Para as Color wash, Lut as Color table, Matte as Cut-out. Others stay. Help sentences lose their romaji in AU12.

---

## 4 Layout

### 4.1 Regions

Default window stays `with_inner_size([1720, 960])`. The two design sizes are below, for Draw with the strip open at 100% scale and a 16:9 document at Fit. Fit is unchanged: 94% of the pane (`canvas.rs:3847-3851`).

| Region | 1600×900 | 1280×800 |
|---|---|---|
| Top row | 1600×44, y 0–44 | 1280×44, y 0–44 |
| Canvas pane | 1600×764, y 44–808 | 1280×664, y 44–708 |
| Band | 1600×92, y 808–900 | 1280×92, y 708–800 |
| Band transport row | 36 tall | 36 tall |
| Strip, above the transport row | 56 tall (peg bar 14, sheets 38, gap 4) | same |
| Rail | 56 wide at x 12, top y 188 | 56 wide at x 12, top y 138 |
| Paper at Fit | 1277×718, left edge 162 | 1110×624, left edge 85 |
| Rail right edge | 68 | 68 |
| Paper share of window | 63.7% | 67.6% |
| Pen can land on canvas | 83.0% | 80.4% |

Other sizes, same arithmetic:

| Window | Pane | Paper | Paper share | Left edge | Pen can land | Paper gate | Pen gate |
|---|---|---|---:|---:|---:|---:|---:|
| 1600×900 | 1600×764 | 1277×718 | 63.7% | 162 | 83.0% | ≥63.0 | ≥82.5 |
| 1280×800 | 1280×664 | 1110×624 | 67.6% | 85 | 80.4% | ≥66.5 | ≥80.0 |
| 1280×720 | 1280×584 | 976×549 | 58.1% | 152 | 78.2% | ≥57.5 | ≥77.5 |
| 1024×640 | 1024×504 | 842×474 | 60.9% | 91 | 74.7% | ≥60.0 | ≥74.0 |

The floors are never lowered. If a measured value falls below one, find out why.

### 4.2 Top row

`egui::Panel::top("top_bar").exact_size(44.0)`, one row, `DESK` fill, no rule. Three groups:

| Group | Contents | Position |
|---|---|---|
| Menus | File, Edit, View, Animation, Help, More as bare words on the desk | Left, x 8 |
| Steps | `ISLAND` stadium holding six pills, then the off-spine pill when off the spine | `max(menus_end + 16, (W - steps_width) / 2)` |
| Actions | Dirty dot, then `ISLAND` stadium holding Undo, Redo, Search | Right, 8 from the edge |

Width at 100%: 8 + menus about 340 + 16 + steps about 408 + 16 + actions about 221 + 8 = 1017, so the row fits from 1040 up. The off-spine pill needs its own room, below.

- **Step pills** are 32 tall. Current is wash. Export is the primary until current. The pill runs the step's `Action` through `perform`. Hover card: first line purpose, second line `Studio word: <word>`.
- **Off-spine pill.** Read-only, wash state, after Export, showing the workspace name (Manga, 3D layout, Brush maker or a saved arrangement), only while off the spine, spoken `Current place: <name>`. Width is the name's, at least 72 and at most `min(140, room)`, where `room` is what the row has left, and a longer name elides with the full name in its hover card. Draw is the way back.
- **Search** is a button styled as an input, 141 wide, opening the existing palette. Ctrl+K and F3 stay.
- **Dirty dot.** The existing dot from `main.rs:7748-7776`, a 12×16 `Sense::hover` zone spoken `unsaved changes`, with no word. It is hover-only, so it is not a control.
- **Retired:** the filename widget, the facts readout (`main.rs:7713-7793`) and the ARMED line (`main.rs:8065-8106`). The name shows in the window title through `window_title` (`main.rs:4430`). The facts move to the frame counter's hover card: `Shot 1, 48 frames, 24 fps, 1920×1080`. The active layer shows in the Layers float and the Brush hover card (`Drawing with`).
- **Narrow rule.** Under 1040 pt, or 1100 while the off-spine pill shows (1120 and 1180 until AU11, while nine menus still show), the menus collapse into one `Menu` button listing the same menus, and Search becomes an icon. That leaves `room` at least 72 at every gated size.

### 4.3 Rail

An `Area` at `Order::Middle`, `ISLAND` fill, radius 12, inner margin 4, 56 wide at x 12. Its box is vertically centred on the pane's *reference height* and anchored at the box's top. The reference height is the height the pane will have with the strip open: `pane_h − 56` while the strip is closed by its latch, otherwise `pane_h`. So no rail control moves when the island grows or the strip opens, and the tier below does not flip either.

| Order | Item | Size | Behaviour |
|---|---|---|---|
| 1 | Brush | 48×48 | `set_tool`. Clicking it while armed opens Brushes |
| 2 | Eraser | 48×48 | Toggle. Same rule, opens Tool options |
| 3 | Select | 48×48 | Same |
| 4 | Fill | 48×48 | Same |
| 5 | Hand | 48×48 | Held state like Space, no new `CanvasTool`, cleared by any tool |
| 6 | Size | 48×84 | Drag only, with a true-size bubble, new `set_brush_px`, refused mid-stroke |
| 7 | Color | 48×48 | Swatch of `brush_color`, spoken `Brush color`, toggles the Color float |
| 8 | Layers | 48×48 | Toggles the Layers float, or focuses the docked tab if one is on screen |
| 9 | Flip | 48×48 | From the second drawing. Momentary, `Sense::drag`. In Manga this slot is `Panel`, spoken `Panel tool`, shown from the first frame, arming the panel tool through `set_tool` |

Island height 428 until the second drawing, then 476. A small mark on an armed tool says that clicking it opens options. The rail calls `set_tool` directly, because the matching `Action`s toggle. The `full_deck` rule (`canvas.rs:3381`: `self.raster || !raster_available`) gates the rail, so it exists in the harness.

**Tiers.** One rule, by reference height, in every room that shows a rail. Every total is items plus 8 of margin.

| Tier | Reference height | Items | Total height |
|---|---|---|---:|
| Full | ≥492 | The table above, with captions | 476 |
| Compact | 372–491 | Same nine, 36 tall, no captions, Size 36×60 | 356 |
| Pocket | 232–371 | Six 36-tall items: Brush, Eraser, Select, Fill, Color, and `…` (spoken `More tools`) holding Hand, Size, Layers and Flip in a popover | 224 |
| Hidden | Under 232, or a pane under 120 wide | None. The verbs stay in `Edit ▸ Tools ▸`, the View row, the keys and Search | 0 |

No gated room reaches Hidden. The tightest are Timing at an effective 832×520 (pane about 239) and Color at the same size (pane about 350), both Pocket. `aurail_rail_fits_every_room` proves this at the 4 sizes × 3 scales of 4.7, for every room with a canvas: the rail stays inside the pane, the tier is the table's, controls are at least 24, and each rail verb is reachable in at most 2 taps.

The rail is also hidden in Plan's Preview, in 3D and in Brush maker.

### 4.4 Band

`egui::Panel::bottom("status_bar").exact_size(...)`, `DESK` fill. Height 92 with the strip, 36 without. Top to bottom: the strip, then the transport row. The transport row is always the last 36 pt of the window, so no transport control moves when the strip opens.

| Slot | Plain transport (sheet off screen) | Full transport (sheet on screen) |
|---|---|---|
| Left | Previous, Play, Next, counter `Frame 7 of 24` | First, Previous, Play, Next, Last, Frame field, Loop, Loop start, Loop end |
| Centre | Say line | Say line |
| Right | Add drawing, Ghost frames, Speed, Timing sheet | Sound while dragging, fps, Timing sheet |

- Tap the counter to type a frame (`GoToFrame`). Hover shows the facts line.
- On a blank document Previous, Next, Ghost frames and Speed are absent, with their slots reserved. They arrive with the second drawing, and Flip on the rail arrives at the same moment.
- **What moves.** The full-band latch only fills reserved slots, so nothing moves. `strip_open` grows the band from 36 to 92, so the pane loses 56 pt and Fit refits the paper once (`canvas.rs:3847`). That happens only when a document opens or on Add drawing, never during a stroke, because `AddDrawingAfter` is `stroke_guarded`. No rail or transport control moves.
- `dropped` shows only when above 0.
- `Timing sheet` is absent in Plan and Timing, where the sheet is part of the recipe. In Draw it switches the recipe. In Color, Effects, Export and off-spine places it opens the sheet as a pane.
- Under 1040 pt, First, Last, Loop start, Loop end, Sound, fps, Ghost frames and Speed fold into a `…` popover, and the Quick Start dots hide. Play, the counter, Add drawing and Timing sheet never fold. From 1040 to 1160 pt the full transport's Sound while dragging is its icon, its name and card keeping the words. The Say line's floor is 240 pt. A sentence that does not fit one 14 pt line wraps to two 12 pt lines inside the 36-tall row, and its action button never shrinks. The narrowest effective sizes are 832×520 (1024×640 at 150%), 853×533 (1280×800 at 150%), 924×520 (1280×720 at 150%) and 1092×614 (1366×768 at 125%). At 832 the lane is about 328 pt. `ausay_every_hint_fits_at_the_smallest_sizes` holds this.

**Foot items.** REDO·UNDO goes to the top buttons. The room lamp stays in the top row only during a LAN session (`main.rs:7174-7192`). Relaunch stays. Version goes to About. The pressure note (`main.rs:7216-7218`) becomes a hint.

### 4.5 Floats and stacking

| Float | Size | Anchor | Contents |
|---|---|---|---|
| Layers | 240×≤320 | Top right, 12 in | `cel_layers_strip` |
| Start | 320×≤460 | Top right, 12 in, left edge past the pane's centre | Section 8.1 |
| Brushes | 280×≤420 | Beside the rail, x 76 | `brush_rail_ui` (`canvas.rs:3091`), plus the three pencil tiles and the deck's Edge softness, Pixel edges and Steady stroke rows. Essentials: picture tiles, Size, Opacity, Steady stroke. `More`: pressure, tilt, flow, minimum size, Edge softness, Pixel edges, imports, Make a brush |
| Color | 280×300 | Beside the rail, x 76 | `dish_ui`, `SWATCHES` becomes `pub(crate)` |
| Tool options | 240×auto | Beside the rail, x 76 | Options row, `canvas.rs:3594-3739`, the `Fill` chooser and the `Keep inside strokes` row |
| Canvas options | 240×auto | Beside the rail, x 76 | `lightbox_rail_ui` (`canvas.rs:2755`), new `CanvasOptions` Action |

**One overlay, two hosts.** The rail and these floats are drawn by one function, `rail::overlay(ui, &mut OverlayArgs, host: Rect)`, called straight after `canvas.ui` in both places a canvas can live: the `Pane::Canvas` arm (`main.rs:4641`) and the canvas viewport closure (`main.rs:8484`). `host` is that frame's `canvas.dbg_rect`. Exactly one runs on a frame, because the arm `Pane::Canvas if self.float_canvas_open` paints only its label (`main.rs:4620-4629`). `Memory.areas` is per viewport, so the same `Area` ids are safe in both. `aurail_both_canvas_hosts_call_one_overlay` is a rule test over `sources()`.

**What the OS window shows.** The canvas, the rail at the tier its own height gives, and all five floats. Not the top row, the band, the strip, the Say line or the Start float, which stay in the main window. In Plan the docked pane is a preview with no rail, but the OS window is a real canvas and shows it. Floats in the OS window push no `pane_rects` entry, because that window's chords already arrive through `shortcuts::collect_for` as `Ctx::Canvas` (`main.rs:6120-6133`). The native pen stays on the root viewport, as today (`canvas.rs:6981`).

**Rules for all floats.** `Order::Middle`, `constrain_to(host)`, `fade_in(false)`. Height is `min(table height, host height − 24)` and width `min(table width, host width − 88)`. The heading stays outside a `ScrollArea::vertical` holding the body, so a small host scrolls and never clips. The top of a float beside the rail is the rail's top, clamped into the host. One float per side: opening a left float closes the other left float, and opening Layers closes Start. There is no click-outside dismissal. Never open during a stroke: a request waits for pen-up (`stroke_active`, `canvas.rs:1416`). Not saved. Opening one never moves art.

| Layer | Holds |
|---|---|
| Background | Dock, top row, band |
| Middle | Rail, floats, Start float |
| Tooltip | Hover cards, always `interactable(false)` |
| Modal | Palette, save prompts, recovery |

**Hover cards.** `plate::hover_card` uses `Area::new(id).order(Order::Tooltip).interactable(false)`, so the pen passes through and floats below still block. Text comes from the `menus::hover_tip` route: `Name (chord) — help`. `Exact UI descriptions` keeps working. Delay 0.35. This replaces tooltips on the rail, floats, band and strip.

**Where every deck control goes.** Every control of the four retired surfaces has one named home and one pen-only path. This table becomes the frozen const `TODAY_DECK`. It was built by reading `brush_ui`, `brush_rail_ui`, `lightbox_rail_ui` and `dish_ui`, not from `journeys.md`'s 18-control list, whose census cannot see the raster-gated right rail or the hover fold-out (`journeys.md:580`).

| Today (`canvas.rs`) | Home | Pen-only path |
|---|---|---|
| Pencil slots `:3392-3396` | Brushes tiles | Tap Brush, tap Brush, tap the tile |
| Eraser slot `:3397` | Rail | Tap Eraser |
| Select `:3408`, shape buttons `:3604, 3616` | Rail, Tool options | Tap Select, tap Select, tap the shape |
| Fill `:3423`, gap, under, cel or layer `:3635-3649` | Rail, Tool options | Tap Fill, tap Fill, tap the row |
| Lasso fill `:3439`, softness, grow `:3660-3664` | `Fill` chooser (Fill, Draw to fill) in Tool options, its rows below it | Tap Fill, tap Fill, tap `Draw to fill` |
| Alpha lock latch `:3457` | `Keep inside strokes`, one function shown in Brushes and in Tool options, so it serves a brush and an eraser | Tap Eraser, tap Eraser, tap the row |
| Comp latch `:3468` | `View ▸ Preview final` | Tap View, tap the row |
| Layer chip `:3498-3538` | Layers float | Tap Layers, tap a row |
| Vector width `:3547` | Rail Size | Drag Size |
| Color button, swatches `:3562-3582` | Color float | Tap Color, tap a swatch |
| Panel tool and note `:3673-3680` | Manga's rail slot 9, note in Tool options | Tap `Panel tool` |
| Hardness, aliased, stabilizer `:3689, 3702, 3713` | Brushes | Tap Brush, tap Brush, tap the row |
| `brush_rail_ui` `:3091-3350` | Brushes float | Tap Brush, tap Brush, tap the control |
| `lightbox_rail_ui` `:2755-3090` | Canvas options float | `View ▸ Canvas options…`, tap the control |
| `dish_ui` `:2306`, drawn at `:5373` | Color float | Tap Color |

### 4.6 The flush rule

```rust
fn dock_style(base: &egui::Style, dock: &DockState<Pane>) -> egui_dock::Style {
    let mut s = egui_dock::Style::from_egui(base);
    s.dock_area_padding = None;
    s.separator.extra = 26.0; // keep: main.rs comment, tab bar stays grabbable
    s.separator.width = 6.0;
    let flush = dock.iter_all_tabs().count() == 1;
    s.tab_bar.height = if flush { 0.0 } else { 34.0 };
    if flush {
        s.tab.tab_body.inner_margin = egui::Margin::ZERO;
        s.tab.tab_body.stroke = egui::Stroke::NONE;
        s.tab_bar.hline_color = egui::Color32::TRANSPARENT;
    }
    s
}
```

Every field above was checked against `egui_dock-0.20.1/src/style.rs`. The proposals' scratch probe of this dock showed one leaf, tab bar 0, pane body exactly the centre rect, 0 null rects, and a clean reload. AU3 repeats it first on the real editor.

`Lock UI positions` (`config.rs:3908`) keeps feeding `DockArea` only (`main.rs:8431-8440`), with one change. Today `show_close_buttons(!ui_lock)` removes every close button, which on a locked fresh install would strand any panel a person opens. Now `show_close_buttons(true)` is constant, `draggable_tabs(!ui_lock)` stays, and closing is decided tab by tab in `TabViewer::is_closeable`: never for Canvas, and with the lock on, not for a pane of the current recipe. A panel opened by hand keeps its close control. The tab viewer holds the recipe's panes for the frame.

### 4.7 Minimum window and scale

`with_min_inner_size([1024.0, 640.0])`. Effective scale is capped at `min(stored, w/760, h/520)`, where `w` and `h` are the window size in points at 100%. The stored `ui.scale` never changes. All layout gates run at 1600×900, 1280×800, 1280×720 and 1024×640, at 100%, 125% and 150%.

### 4.8 Paper targets

Dual hard gates on a 4 pt grid. A sample is "paper" when `layer_id_at(p)` is the canvas layer, `dbg_rect` holds `p`, `p` is on the paper, and no widget but the canvas's own has a rect holding `p` on that layer (`prev_pass.widgets.get_layer`). The last test is there because `layer_id_at` cannot see widgets on the canvas's own layer. A sample is "pen can land" when all of those hold but the third. "Paper under chrome" counts samples on the paper that a higher layer or a same-layer widget covers, and must be 0 at Fit with no float open. Measured for 16:9, 4:3, 1:1 and 9:16 documents. Never report pane share as paper share. The fit margin does not change.

Accepted: a document wider than about 2.09:1 at 1600×900 runs under the rail by up to 20 pt at Fit.

### 4.9 Other arrangements

Tab bars are 34 tall. "Band" is 36 when the sheet is on screen, 92 with the strip.

| Arrangement | 1600×900 | 1280×800 | Band |
|---|---|---|---:|
| Plan | Left 352, Preview 1248×786 | Left 282, Preview 998×686 | 36 |
| Draw, sheet on | Left 384, canvas 1216×786, paper 1143×643 (51.0%) | Left 307, canvas 973×686, paper 915×514 (46.0%) | 36 |
| Timing | Left 720, right 880, top 508, bottom 312 | Left 576, right 704, top 446, bottom 274 | 36 |
| Color | Canvas 1248×730, paper 1173×660 (53.8%), right 352 | Canvas 998×630, paper 938×528 (48.3%), right 282 | 92 |
| Effects | Left 928, right 672 | Left 742, right 538 | 92 |
| Export | Result 1152×730, Export panel 448 | Result 922×630, Export panel 358 | 92 |

---

## 5 The workflow steps

### 5.1 The six steps

A step is a lens on the same document. Switching never forks data. The display order lives in a new `steps.rs`. `Stage::ALL` is untouched, because tests and stored paths read it.

```rust
pub struct Step {
    pub stage: Stage,
    pub label: &'static str,        // vocab::STEP_*
    pub purpose: &'static str,      // hover card, first line
    pub studio_words: &'static str, // hover card, second line; "" for Export
    pub action: Action,
}
pub const STEPS: [Step; 6] = [ /* Plan, Draw, Timing, Color, Effects, Export */ ];
```

| Step | Purpose | Studio word | `Stage` | Stored key | `Action`, key | Default arrangement | Armed on entry |
|---|---|---|---|---|---|---|---|
| Plan | Set up shots, timing and camera before drawing. | layout | `Layout` | `layout` | `RoomLayout`, F4 | Today's recipe: left 0.22 Timing sheet and Effects graph, Layers below, canvas labelled `Preview` | Paint, stack `Layout (LO)`. Canvas is a preview by owner decision (`main.rs:4632-4634`) |
| Draw | Draw on the paper, one drawing at a time. | genga | `Drawing` | `drawing` | `RoomDrawing`, F5 | Canvas alone. With `sheet_in_draw`: today's recipe minus the dead Brush band | Paint, Ghost frames on, stack `Solo merged` |
| Timing | Arrange when each drawing appears. | henshū | `Edit` | `edit` | `RoomEdit`, F6 | Today's recipe. Sheet edit head on (`main.rs:4572-4577`), full transport | Select |
| Color | Fill your drawings with color. | shiage | `InkPaint` | `finishing` | `RoomFinishing`, F7 | Canvas, then right 0.22 tabs Colors, Check paint, Result. `Result beside` restores today's split | Fill, `fill_ref_cel`, Protect line art forced (`main.rs:4761`), stack `Shiage (full)` |
| Effects | Add glow, blur and camera moves. | satsuei | `Composite` | `composite` | `RoomComposite`, F8 | Today's recipe with Effects graph front. A 40-tall shelf inside the graph pane header | Composite view on |
| Export | Save a file to share. | none | `Export` | none | `RoomExport`, F9 | Result left 0.72, Export panel right 0.28 | Nothing changes |

Timing keeps `RenderQueue` and Effects keeps it, because `render_queue_sits_in_the_composite_and_edit_rooms_and_greys_video_without_ffmpeg` pins that.

### 5.2 Off the spine

| Place | Stage | Stored key | Reached by | Arrangement |
|---|---|---|---|---|
| Manga | `Manga` | `manga` | More ▸ Manga, the `A comic` Start card | Today's recipe minus the Brush band |
| 3D layout | `None` | `custom` | More ▸ 3D layout | `Pane::layout3d_dock`, today's |
| Brush maker | `None` | `custom` | More ▸ Brush maker, new `BrushMakerRoom` replacing the hand row at `main.rs:7869` | `Pane::forge_dock`, today's |

Manga has no chord: no free key is worth spending. The `custom` key arms `Solo merged` as today (`main.rs:92`, `config.rs:2333`).

### 5.3 How entering a step works

`enter_room` (`main.rs:4755`) stays the one door. It changes in two ways:

```rust
fn recipe(&self, st: Stage) -> DockState<Pane> {
    match st {
        Stage::Drawing if self.sheet_in_draw => st.dock_with_sheet(),
        Stage::InkPaint if self.result_in_color => st.dock_with_result(),
        _ => st.dock(),
    }
}
```

`enter_room(Stage::Export)` swaps only the dock and remembers the previous stage. It skips `apply_view` and `line_guard`, and is refused during a stroke by the guard `apply_view` already uses. The pure seam below keeps Export from arming a stack:

```rust
/// The room key the arming block reads. Export arms nothing.
fn arming_key(stage: Option<Stage>, seen: &'static str) -> &'static str {
    match stage { Some(Stage::Export) => seen, s => stage_key(s) }
}
```

`Editor::arm_room(&mut self, layers_cfg, seen: &mut &'static str)` holds the arming block that is `main.rs:5975-5992` today, using `arming_key` in place of `stage_key(editor.stage)`. The App calls it with its own `stack_room_seen` and the Rig with its own, so a Rig under a shipped boot arms what the App arms, and the App's behaviour does not change. Check every other `stage_key` call site. `Stage::Export` joins `PROBE_ROOMS` (9 entries) and the shot chain.

Effects of rooms that stay, by anchor: `main.rs:4757, 4761, 5978-5989, 4572-4577, 4631-4639, 8298, 3196, 4956`.

**Plan.** A stroke attempt shows the Say line `This step shows a preview. Drawing happens in Draw.` with the action `Go to Draw`.

### 5.4 Disclosed sheet and Result

`Timing sheet` in the band is one click, remembered in `ui.sheet_in_draw`, read by Reset this step. `Editor.sheet_in_draw` mirrors it, because `enter_room` and `from_state` see no config. The same pattern holds `Result beside` and `ui.result_in_color`. TV anime HD and Feature UHD presets start sheet-on. Vestigial `Pane::Brush` lines at `workspace.rs:214, 224, 237, 274` go.

**Both directions.** Copying Editor to config is half of it: every new `Editor` starts with the fields false, so without a seed the choice is lost at restart. One helper does the other half:

```rust
/// Every Editor is born through these. `apply_ui` copies the remembered fields in
/// and rebuilds the dock when `recipe` then differs.
pub fn boot(state: AppState, rs: Option<&RenderState>, dir: Option<&Path>, ui: &UiConfig) -> Editor
pub fn boot_form(form: &NewProjectForm, rs: Option<&RenderState>, dir: Option<&Path>, ui: &UiConfig) -> Editor
fn apply_ui(&mut self, ui: &UiConfig)
```

All 13 construction sites use them: `main.rs:2407, 2521, 2718, 4387, 4399, 6463, 6555, 6566, 6718`, `testkit.rs:157, 1172`, `worksafe.rs:1391, 1399`. `open_template` and `welcome_show_me` take `&UiConfig`. `audock_every_editor_is_built_through_boot` is a rule test: over `sources()`, `Editor::from_state(` and `Editor::from_form(` appear only inside `boot` and `boot_form`. The dock rebuilt here has never been drawn and is never saved first, because `Workspace::normalised()` guards every save (1.4).

### 5.5 Beginner export

A beginner cannot finish today: GIF and one-click still do not exist, and MP4 needs a separately installed ffmpeg. This gets built:

| What | Decision |
|---|---|
| GIF | In-house GIF89a writer in `anim-core`, about 150 lines over `weezl` 0.1.12 (already in `Cargo.lock:4691` through `tiff`; add it as a direct dependency). Global 256-colour palette by median cut over a 5-6-5 histogram, no dither, `NETSCAPE2.0` loop forever, frame delays in hundredths from holds with a running remainder (`round(cum_frames*100/fps) - prev`), identical neighbours merged, flattened on white. LZW `BitOrder::Lsb`, sub-blocks of 255. |
| Still | `ExportKind::Still`, one appended preset `Still PNG`, one named file, white background by default. Transparent is a toggle under More options, using the existing `PNG 8-bit`. |
| MP4 | Stays ffmpeg-based and is not bundled. When ffmpeg is missing the row reads `Video needs a free add-on on this PC. GIF works now.` with `Export GIF instead`. A clean-PC MP4 stays unmet and goes on the Lead's list as a bundling decision. Media Foundation is not required. |
| Picture sequence | Today's `PNG` route, named plainly. |
| Comic | `Export pages` from the existing manga route, plus Other formats. |

The Export panel is the same `Pane::RenderQueue` with one layout, not a mode: choices at the top, the pinned job list below, and today's whole queue panel under `More options`. On finish, a Done card: `Exported ball.gif`, `12 drawings, 1.0 s, 184 KB`, `Open file`, `Show in folder`, `Back to Draw`. Open and Show go through `AppRequest`. The file is written first, so the card can only tell the truth. An export started from `File ▸ Export ▸` outside the Export step stays where it is and opens no pane. When it finishes the Say line says `Exported ball.gif.` with `Open file` (8.2).

---

## 6 Command depth

### 6.1 Five levels

| Level | What lives there | Cost |
|---|---|---|
| 1. Rail and band | The verbs of drawing and time | Always visible, one tap |
| 2. Floats | Brushes, Color, Layers, Tool options | One tap on the rail |
| 3. Menus | Everything else with a row, six menus | Two taps |
| 4. Search | Every `Action`, every old word as an alias | Ctrl+K or F3, type, Enter |
| 5. Shortcuts | Every chord, F1 sheet | One press |

Search is visible on the top row, not hidden behind a key. The F1 sheet, `not now: why` on disabled rows, and one registry for menus, palette and sheet all stay.

**Radial, not built (R6).** If the Lead asks for it, its contract is: drag-only or `pointer.latest_pos()` with `primary_down()`, 1.2 s Touch replay guarded, inverses opposite each other, every item mirrored in a menu, never touch `max_click_duration`.

### 6.2 Menus

Five text menus plus More. 59 first-level rows, counted below. The tables after that are the whole contract: every `Action` that has a menu row appears once, with its exact path, in the exact order the menus show it. `hand` marks a row with no `Action`, drawn by hand at that place.

| Menu | Rows | First-level rows |
|---|---:|---|
| File | 11 | New…, Open…, Open recent ▸, Save, Save as…, Shots ▸, Import ▸, Export ▸, Project settings…, Settings…, Quit |
| Edit | 9 | Undo, Redo, Cut, Copy, Paste, Clipboard ▸, Select all, Clear ▸, Tools ▸ |
| View | 12 | Zoom ▸, Fit, Rotate ▸, Mirror, Ghost frames, Preview final, Full quality, Canvas options…, Check ▸, 3D view ▸, Hide tools, Lock UI positions |
| Animation | 10 | Add drawing, Go to ▸, Play ▸, Loop ▸, Insert or delete ▸, Hold ▸, Retime ▸, Mark as ▸, Track ▸, Curves ▸ |
| Help | 8 | Quick Start, Hints, Keyboard shortcuts, Search commands, What's New, Start screen…, Example project, About |
| More | 9 | Manga, 3D layout, Brush maker, Steps ▸, Panels ▸, Effects setup ▸, Manga tools ▸, Saved arrangements ▸, Reset this step |

**File**

| Path | Action |
|---|---|
| `File/New…` | `NewProject` |
| `File/Open…` | `Open` |
| `File/Open recent/<each recent file>` | hand |
| `File/Save` | `Save` |
| `File/Save as…` | `SaveAs` |
| `File/Shots/Previous shot` | `PrevCut` |
| `File/Shots/Next shot` | `NextCut` |
| `File/Shots/Make shots from panels` | `MakeCutsFromPanels` |
| `File/Shots/Add retake note…` | `AddRetakeNote` |
| `File/Shots/Paper size…` | `PaperSize` |
| `File/Shots/Rename shot…`, `Rename scene…`, the shot tree, `Add shot`, `Add scene` | hand |
| `File/Import/Import sound…` | `ImportSound` |
| `File/Import/Add sound track…` | `AddSoundTrack` |
| `File/Import/Import 3D model…` | `Import3DModel` |
| `File/Import/Reload image sequences` | `ReloadSequences` |
| `File/Import/XDTS…` | hand |
| `File/Export/Animated GIF` | `ExportGif` |
| `File/Export/Video (MP4)` | `ExportMp4` |
| `File/Export/Picture sequence (PNG)` | `ExportPngSequence` |
| `File/Export/This frame (PNG)` | `ExportStill` |
| `File/Export/Other formats…` | hand, performs `RoomExport` |
| `File/Export/Manga pages (PNG, TIFF)…` | `ExportPages` |
| `File/Export/EDL (CMX3600)` | `ExportEdl` |
| `File/Export/Color sheet` | `ExportColorSheet` |
| `File/Export/Shot as project (for a freelancer)` | `ExportCutAsProject` |
| `File/Export/Episode` | `RenderEpisode` |
| `File/Project settings…` | `ProjectSettings` |
| `File/Settings…` | `Settings` |
| `File/Quit` | `Quit` |

**Edit**

| Path | Action |
|---|---|
| `Edit/Undo` | `Undo` |
| `Edit/Redo` | `Redo` |
| `Edit/Cut` | `EditCut` |
| `Edit/Copy` | `EditCopy` |
| `Edit/Paste` | `EditPaste` |
| `Edit/Clipboard/Cut cells` | `CutCells` |
| `Edit/Clipboard/Copy cells` | `CopyCells` |
| `Edit/Clipboard/Paste cells` | `PasteCells` |
| `Edit/Clipboard/Paste cells (insert)` | `PasteCellsInsert` |
| `Edit/Clipboard/Copy pixels` | `CopyPixels` |
| `Edit/Clipboard/Paste pixels` | `PastePixels` |
| `Edit/Select all` | `SelectAll` |
| `Edit/Clear/Clear layer` | `ClearCel` |
| `Edit/Clear/Clear whole drawing` | `ClearCelAll` |
| `Edit/Tools/Brush tool` | `BrushTool` |
| `Edit/Tools/Eraser tool` | `ToggleEraser` |
| `Edit/Tools/Select tool` | `SelectTool` |
| `Edit/Tools/Fill tool` | `FillTool` |
| `Edit/Tools/Draw to fill tool` | `LassoFillTool` |
| `Edit/Tools/Keep inside strokes` | `ToggleAlphaLock` |
| `Edit/Tools/Panel tool` | `PanelTool` |
| `Edit/Tools/Smaller brush` | `BrushSmaller` |
| `Edit/Tools/Bigger brush` | `BrushBigger` |
| `Edit/Tools/Next layer` | `CycleCelLayer` |
| `Edit/Tools/Previous layer` | `CycleCelLayerBack` |

**View**

| Path | Action |
|---|---|
| `View/Zoom/Zoom in` | `ZoomIn` |
| `View/Zoom/Zoom out` | `ZoomOut` |
| `View/Zoom/Actual size (100%)` | `Zoom100` |
| `View/Fit` | `ZoomFit` |
| `View/Rotate/Rotate left 15°` | `RotateViewLeft` |
| `View/Rotate/Rotate right 15°` | `RotateViewRight` |
| `View/Rotate/Reset rotation` | `ResetRotation` |
| `View/Mirror` | `MirrorView` |
| `View/Ghost frames` | `ToggleOnion` |
| `View/Preview final` | `ToggleCompositeView` |
| `View/Full quality` | `ToggleFinalLook` |
| `View/Canvas options…` | `CanvasOptions` |
| `View/Check/Miss check` | `MissCheck` |
| `View/Check/Pin this drawing` | `PinToLightTable` |
| `View/Check/Shift & trace` | `ShiftTrace` |
| `View/Check/Reset shift` | `ResetShift` |
| `View/3D view/Front` | `View3dFront` |
| `View/3D view/Back` | `View3dBack` |
| `View/3D view/Right` | `View3dRight` |
| `View/3D view/Left` | `View3dLeft` |
| `View/3D view/Top` | `View3dTop` |
| `View/3D view/Bottom` | `View3dBottom` |
| `View/3D view/Camera` | `View3dCamera` |
| `View/3D view/Ortho or perspective` | `View3dOrtho` |
| `View/3D view/Frame all` | `View3dFrameAll` |
| `View/3D view/Set shot camera from view` | `SetCutCameraFromView` |
| `View/3D view/3D underlay` | `Underlay3d` |
| `View/Hide tools` | `HideTools` |
| `View/Lock UI positions` | `ToggleLockPositions` |

**Animation**

| Path | Action |
|---|---|
| `Animation/Add drawing` | `AddDrawingAfter` |
| `Animation/Go to/Next frame` | `NextFrame` |
| `Animation/Go to/Previous frame` | `PrevFrame` |
| `Animation/Go to/First frame` | `FirstFrame` |
| `Animation/Go to/Last frame` | `LastFrame` |
| `Animation/Go to/Next drawing` | `NextKey` |
| `Animation/Go to/Previous drawing` | `PrevKey` |
| `Animation/Go to/Go to frame…` | `GoToFrame` |
| `Animation/Play/Play or pause` | `PlayPause` |
| `Animation/Play/Sound while dragging` | `ToggleScrub` |
| `Animation/Play/Render preview to memory` | `RamPreview` |
| `Animation/Play/Play episode` | `PlayEpisode` |
| `Animation/Loop/Loop playback` | `ToggleLoop` |
| `Animation/Loop/Loop start` | `LoopIn` |
| `Animation/Loop/Loop end` | `LoopOut` |
| `Animation/Loop/Clear loop range` | `ClearLoop` |
| `Animation/Insert or delete/New drawing here` | `NewDrawing` |
| `Animation/Insert or delete/Duplicate drawing` | `DuplicateCel` |
| `Animation/Insert or delete/Make inbetweens` | `MakeInbetweens` |
| `Animation/Insert or delete/Type into cell` | `TypeIntoCell` |
| `Animation/Insert or delete/Remove from timeline` | `ClearFrameKey` |
| `Animation/Insert or delete/Insert frame` | `InsertFrames` |
| `Animation/Insert or delete/Delete frame` | `DeleteFrames` |
| `Animation/Hold/Longer` | `HoldPlus` |
| `Animation/Hold/Shorter` | `HoldMinus` |
| `Animation/Retime/On ones` | `RetimeOnes` |
| `Animation/Retime/On twos` | `RetimeTwos` |
| `Animation/Retime/On threes` | `RetimeThrees` |
| `Animation/Retime/Reverse timing` | `Reverse` |
| `Animation/Mark as/Key pose` | `KindKey` |
| `Animation/Mark as/Breakdown` | `KindBreakdown` |
| `Animation/Mark as/Inbetween` | `KindInbetween` |
| `Animation/Mark as/Unmarked` | `KindUnset` |
| `Animation/Track/Drawing track` | `ColumnKindCel` |
| `Animation/Track/Background track` | `ColumnKindBg` |
| `Animation/Track/Overlay track` | `ColumnKindBook` |
| `Animation/Track/Reference track` | `ColumnKindRef` |
| `Animation/Track/Hide track` | `ToggleColumnHidden` |
| `Animation/Track/Lock track` | `ToggleColumnLocked` |
| `Animation/Track/Lip flap…` | `LipFlap` |
| `Animation/Track/Remove track` | `RemoveColumn` |
| `Animation/Curves/Hold key` | `InterpHold` |
| `Animation/Curves/Linear` | `InterpLinear` |
| `Animation/Curves/Ease` | `InterpEase` |
| `Animation/Curves/Ease in` | `InterpEaseIn` |
| `Animation/Curves/Ease out` | `InterpEaseOut` |
| `Animation/Curves/Bezier` | `InterpBezier` |
| `Animation/Curves/Auto handles` | `HandlesAuto` |
| `Animation/Curves/Smooth handles` | `HandlesSmooth` |
| `Animation/Curves/Flat handles` | `HandlesFlat` |
| `Animation/Curves/Broken handles` | `HandlesBroken` |
| `Animation/Curves/Frame curves` | `FrameCurves` |

**Help**

| Path | Action |
|---|---|
| `Help/Quick Start` | `QuickStart` |
| `Help/Hints` | `ToggleHints` |
| `Help/Keyboard shortcuts` | `ShortcutSheet` |
| `Help/Search commands` | `CommandPalette` |
| `Help/What's New` | `WhatsNew` |
| `Help/Start screen…` | `WelcomeScreen` |
| `Help/Example project` | `OpenSample` |
| `Help/About` | `About` |

**More**

| Path | Action |
|---|---|
| `More/Manga` | `MangaRoom` |
| `More/3D layout` | `Layout3dRoom` |
| `More/Brush maker` | `BrushMakerRoom` |
| `More/Steps/Plan step` | `RoomLayout` |
| `More/Steps/Draw step` | `RoomDrawing` |
| `More/Steps/Timing step` | `RoomEdit` |
| `More/Steps/Color step` | `RoomFinishing` |
| `More/Steps/Effects step` | `RoomComposite` |
| `More/Steps/Export step` | `RoomExport` |
| `More/Panels/History` | `History` |
| `More/Panels/Motion curves` | `ShowCurves` |
| `More/Panels/Check paint` | `ShowPaintCheck` |
| `More/Panels/Export queue` | `ShowRenderQueue` |
| `More/Panels/Shots` | `ShowCutBoard` |
| `More/Panels/Episode timeline` | `ShowEpisode` |
| `More/Panels/<title> panel`, for each other pane; `Add Result`; `Canvas in an OS window`; `Result in an OS window` | hand |
| `More/Effects setup/Add node…` | `AddNode` |
| `More/Effects setup/Rebuild from layers` | `BuildComposite` |
| `More/Effects setup/Full finishing setup` | `BuildSatsuei` |
| `More/Effects setup/Convert colors to sRGB…` | `ConvertColorsToSrgb` |
| `More/Manga tools/New page` | `NewPage` |
| `More/Manga tools/Clip to panels` | `ClipToPanels` |
| `More/Manga tools/Snap to rulers` | `ToggleRulerSnap` |
| `More/Saved arrangements/Save this arrangement…`, then apply, bind and delete for each saved one | hand |
| `More/Reset this step` | `ResetLayout` |

`Preset1` to `Preset8` stay off the menus. They are `Group::Tool`, they live in the Brushes tiles and on the keys 1 to 8, and `ship_every_action_is_described_placed_and_clash_free` allows `None` for `Group::Tool`. Every other `Action` has a row.

**Row order.** `menu_items` (`menus.rs:178-200`) walks `Action::ALL`, so paths alone would give registry order. It changes to walk a new `menus::ORDER: &[Row]`, where `Row` is `A(Action)` or `Hand(HandRow)`, written in the order of these tables. `aumenu_order_is_the_table` asserts that `ORDER` holds each `Action` with a path exactly once and no other. A path has at most three parts (`menus.rs:178`), so nothing nests under a submenu. `MENUS` (`cmdpal.rs:241`) becomes the six names, and `plain_menus` (`menus.rs:224-228`) follows.

**Merged clipboard rows.** `menu_items` draws one row per `Action`, so a shared path would give two rows and break unique spoken names (1.5 rule 9). So `Edit ▸ Cut`, `Copy` and `Paste` are three new Actions with no chord of their own. Each reads `Editor.last_ctx`, the result of `shortcut_ctx` from the last frame with no popup open, and performs the Action in this table. The row shows the chord of the Action it would perform.

| Row | Over the sheet or strip | Over the canvas |
|---|---|---|
| `EditCut` | `CutCells` | Disabled through `can_perform`, reason `not now: pixels have no Cut. Use Copy, then Clear.` There is no `CutPixels` |
| `EditCopy` | `CopyCells` | `CopyPixels` |
| `EditPaste` | `PasteCells` | `PastePixels` |

The precise rows stay under `Edit ▸ Clipboard ▸` for anyone who wants one by name. The chords do not change: Ctrl+C and Ctrl+V still go to the pane under the pointer.

**Spoken names.** A row speaks its last path segment (`menus.rs:134-173`), and rows are unique in these tables. Controls that share a verb speak differently: the pills say `Plan` and the rows `Plan step`, the rail says `Fill` and the row `Fill tool`, the band says `Add drawing (band)` and the row `Add drawing`, a pane row says `<title> panel`.

`File ▸ Export ▸` lists Animated GIF, Video (MP4), Picture sequence (PNG), This frame (PNG), Other formats (opens the Export step), then the five special exports. The Project menu's colour law line (`menus.rs:232`) moves to Project settings.

**Hand-built rows and their homes.** Each must have an `Action` or a named home. The census test fails otherwise.

| Row today | Home |
|---|---|
| Open Recent (`main.rs:7405-7451`) | File ▸ Open recent ▸, and Start |
| Export rows (`:7480-7549`) | Export step, File ▸ Export ▸, new Actions |
| Rename cut, rename scene (`:7594-7615`) | File ▸ Shots ▸ |
| XDTS (`:7621-7652`) | File ▸ Import ▸, and Other formats |
| Scene and cut tree, add cut, add scene (`:7668-7700`) | File ▸ Shots ▸. PageUp and PageDown unchanged |
| Save arrangement, apply, bind, delete (`:7829-7958`) | More ▸ Saved arrangements ▸ |
| Brush Forge (`:7869`) | More ▸ Brush maker |
| Pane list, add Viewer, OS windows (`:7963-8057`) | More ▸ Panels ▸ |
| LAN host and join | Stay in Settings (`config.rs:3393, 3514`), plus a palette link |
| Settings-only rows | Stay |

The seven context-menu-only sites (`board_panel.rs:375`, `nodegraph_panel.rs:728`, `xsheet_panel.rs:644, 1460, 1890, 2430`, `curves_panel.rs:818`) get menu or palette twins in AU15.

### 6.3 New Actions

Each needs a label, help of 1 to 120 characters ending in a full stop, a menu path, aliases and clean `clashes` (`ship_every_action_is_described_placed_and_clash_free`, `viewnav_new_verbs_are_actions_with_free_chords`). All are additive. Any new verb that changes the document joins `stroke_guarded` (`main.rs:3759`).

| Action | Default chord | Context | Menu path | Part |
|---|---|---|---|---|
| `CanvasOptions` | none | Global | `View/Canvas options…` | AU4 |
| `AddDrawingAfter` | Alt+A | Global | `Animation/Add drawing` | AU5 |
| `GoToFrame` | none | Global | `Animation/Go to/Go to frame…` | AU5 |
| `WhatsNew` | none | Global | `Help/What's New` | AU6 |
| `ToggleLockPositions` | none | Global | `View/Lock UI positions` | AU6 |
| `ToggleHints` | none | Global | `Help/Hints` | AU7 |
| `RoomExport` | F9 | Global | `More/Steps/Export step` | AU9 |
| `ExportGif` | none | Global | `File/Export/Animated GIF` | AU9 |
| `ExportMp4` | none | Global | `File/Export/Video (MP4)` | AU9 |
| `ExportPngSequence` | none | Global | `File/Export/Picture sequence (PNG)` | AU9 |
| `ExportStill` | none | Global | `File/Export/This frame (PNG)` | AU9 |
| `HideTools` | T | Global | `View/Hide tools` | AU10 |
| `BrushMakerRoom` | none | Global | `More/Brush maker` | AU11 |
| `EditCut` | none | Global | `Edit/Cut` | AU11 |
| `EditCopy` | none | Global | `Edit/Copy` | AU11 |
| `EditPaste` | none | Global | `Edit/Paste` | AU11 |

That is 16 new Actions by the end of AU11. Until AU11 a new Action sits in today's matching menu (`Sheet/…`, `View/…`, `File/…`, `Help/…`), so the ship test stays green at every part. The seven context twins of AU15 add seven more. `Action::ALL.len()` goes from 154 to 170 after AU11 and 177 after AU15. Palette gates compute it from `Action::ALL` and never pin a literal.

### 6.4 Key conflict table

Checked against `gaban_chord` (`config.rs:1334-1596`), `default_chord` and `default_also` (`config.rs:1294-1332`), `pane_only` (`:1250-1283`), the Unbound list (`:1588-1594`), and the code's own key reads (`Key::Arrow*` only at `cmdpal.rs:132-133` and `nodegraph_panel.rs:336-337`, `F12` at `main.rs:5890`).

| Chord | Goes to | Today in Gaban | Today in Industry | Conflict | Resolution |
|---|---|---|---|---|---|
| F4 | `RoomLayout` | Free | Free | None. `RoomLayout` is Unbound today | New default |
| F5 | `RoomDrawing` | Free | Free | None. Unbound today | New default |
| F6 | `RoomEdit` | Free | Free | None. Unbound today | New default |
| F7 | `RoomFinishing` | Free | Free | None. Unbound today | New default |
| F8 | `RoomComposite` | Free | Free | None. Unbound today | New default |
| F9 | `RoomExport` | Free | Free | None. New Action | New default |
| T | `HideTools` | Free | Free | Bare key, so it never fires over a text field (`fires_over_field`) | New default |
| Alt+A | `AddDrawingAfter` | Free | Free | None | New default |
| Alt+O | `ColumnKindBook` | Free | Free | None | Moved from Alt+K |
| Alt+W | `ColumnKindBg` | Free | Free | None | Moved from Alt+B |
| Alt+K | `KindKey` alone | Shared with `ColumnKindBook` | Same | Pointer-dependent today | Book leaves, Key stays |
| Alt+B | `KindBreakdown` alone | Shared with `ColumnKindBg` | Same | Pointer-dependent today | Bg leaves, Breakdown stays |
| Left, Up | `PrevFrame` also | Free | Free | egui moves focus on bare arrows when a widget holds focus (`memory/mod.rs:575-578`). Sliders and drags take them when focused. Palette and node popup hold a focused text field. | `default_also`. `fired` skips arrow chords when `ctx.memory(|m| m.focused()).is_some()` |
| Right, Down | `NextFrame` also | Free | Free | Same | Same. Industry keeps Comma and Period as primary |

`ColumnKindBook` and `ColumnKindBg` join `pane_only`, keeping today's over-sheet reach and stopping them from firing elsewhere. `KindKey` and `KindBreakdown` then fire everywhere through `aimed_here` (`shortcuts.rs:93-112`). `ColumnKindCel` (Alt+C) and `ColumnKindRef` (Alt+G) are untouched.

**What "pointer-dependent" counts.** One chord that does different non-clipboard work over the Canvas and over the Sheet. In the Gaban keymap the count goes from 2 (Alt+K, Alt+B) to 0. These stay, and they are the one allowed list in `aukeys_no_chord_depends_on_the_pointer`:

| Chord | Why it may follow the pane |
|---|---|
| Ctrl+C, Ctrl+V | Clipboard: pixels over the canvas, cells over the sheet (`config.rs:1443-1462`) |
| 1, 3, 5, 7, 0, Home, Ctrl+1 | 3D view keys, over the 3D view only (`config.rs:1562-1570`) |
| Delete, Shift+Delete, Industry keymap only | Clear over the canvas, remove over the sheet (`config.rs:1306-1309`). The Industry keymap's seven overrides do not change |

**Rejected chords, do not retry:**

| Chord | Why |
|---|---|
| Ctrl+Shift+X, C, V | egui-winit turns any Ctrl+X, C, V into events (`lib.rs:1001-1003, 1396, 1402, 1408`), so these are dead |
| Ctrl+Alt+anything | AltGr on many layouts. `Ctrl+Alt+0` stays as the one existing use |
| Shift+digit | Logical key differs by layout |
| Bare digits | 1-8 presets, 0 RAM preview |
| Ctrl+1, Ctrl+2 | Taken |
| F10 | Windows menu key |
| F12 | Hard-coded uidump |
| Tab | `AddNode` |
| Space, Shift+Space | Play and hand |
| R | Reserved hold flip |
| Alt+Shift | Windows layout switch |

**Still free after this plan:** bare I, J, U, Y, Z, 9, F2, F11 (bare N stays free in Gaban only), Alt+F, Alt+J, Alt+Q, Alt+R, Alt+S, Alt+Y, Alt+Z.

Kept unchanged: Ctrl+K and F3 (Search), F1 (sheet), R and Space (hold keys), the Industry keymap's seven overrides, 118 of today's 120 chords.

### 6.5 Migration

Defaults change for two old Actions and five unbound ones, so the keymap migrates. Full algorithm in AU10. `keymap_rev: u32`, `#[serde(default)]`. A config without the field reads 0 and migrates once. A fresh install stamps `KEYMAP_REV` and migrates nothing. A user's own binding always wins.

---

## 7 Panels

### 7.1 Essential and disclosed

Display names only. Pane ids do not change. Each panel gets an explicit scroll policy. The `More` disclosure (or `Details`) is per-panel, remembered in `ui.panel_more`, default closed except the Timing sheet. Frozen boot opens all of them through `Editor.more_default`. A control that is disabled stays disabled, with a reason, once shown.

| Panel (`Pane`) | Min size today to new | Essential | Disclosed |
|---|---|---|---|
| Timing sheet (`XSheet`) | 502×360 to 240×250 | Tracks, cells, one-line header | `Details`: spacing, kinds, notes. Chrome 178 to about 90 pt, about 37 rows at 1600×900 |
| Layers | 280×160 to 200×140 | Rows with eye, name, active. Add, Delete, Opacity | `More`: add by role, Layer set, lock, card, clip, reorder, Protect line art, Clear drawing. Held frames read-only with `Go to this drawing` |
| Brushes float (`brush_rail_ui`) | n/a | 3-across 48 pt tile grid, Size, Opacity, Steady stroke | `More`: pressure, Edge softness, Pixel edges, flow, tilt, imports, Make a brush |
| Brushes panel (`Presets`) | Unchanged | Name, `Save current`, saved brushes (`presets_ui`, `main.rs:4681`) | None |
| Brush maker (`Forge`) | 320×480 to 240×220 | Try, Name, Save, core shape | Advanced curves, Replace |
| Colors (`Palette`) | 160×180 to 160×140 | Ten starter swatches (UI constants, `.animproj` untouched, clicking sets `brush_color` with no document change), Add color, Edit colors outside the scroll | Character groups, color names. The popover stays open across picks. Keep `plate::danger` holds |
| Effects graph (`NodeGraph`) | 480×300 to 360×260 | Effects shelf, graph, Set up standard composite | Node library, Rebuild from layers, Full finishing setup |
| Result (`Viewer`) | 200×150 to 160×90 | Picture, fit | Channels, compare |
| History | 180×160 unchanged | All | None |
| Motion curves (`Curves`) | 360×200 unchanged | All | None |
| Check paint (`PaintCheck`) | 260×200 to 220×140 | List, Run | Thresholds |
| Export queue (`RenderQueue`) | 330×420 to 280×220 | Export panel, 5.5 | `More options`: today's whole queue |
| Shots (`Board`) | 170×300 to 170×220 | Cards, status | Filters |
| Episode timeline (`Episode`) | 320×150 to 300×100 | Timeline, Play | EDL |
| 3D view (`View3D`) | 320×260 to 240×200 | View, model, camera | Render settings |
| Manga | 380×scroll to 280×320 | Page list, New page, Split across, Split down, Add panel, Delete panel, Export pages | Screen tone, Perspective rulers |

Palette edits bypass undo today. That stays out of scope.

### 7.2 The simple timeline

The strip is a second view of the same exposures. It reads `Column::runs(0, frame_count)` (`crates/anim-core/src/xsheet.rs:190`), `view.frame`, `runs::rule_tier`, `cut.drawing(id).name` and `play_range()`, and writes through the same commands and `sheet_ops` as the Timing sheet. It has no data of its own. New module `strip.rs`.

**Shape.** One surface: `allocate_exact_size(vec2(w, 56.0), Sense::drag())`, with its own tap logic and its own 0.6 s long-press, because `is_long_touch` kills a `click_and_drag` press at 0.817 s under a pen. Per-cell accesskit nodes through `accesskit_node_builder`. Scrolling uses the `ui_episode` template (`board_panel.rs:714-765`) and follow from `xsheet_panel.rs:1404-1414`.

**Paint.** Steel peg bar 14 tall with white second ticks and `1 s` labels at 12 pt. Below it, `PAPER` sheets 38 tall, with one round `DESK` peg hole of radius 2.5, top corners 0, bottom corners 4, width `frames × scale − 2`. Default scale is `clamp(W / frame_count, 8, 48)` pt per frame at document open, held for the session. Ctrl+wheel 6–48, not saved. A number when the sheet is at least 16 wide, name in the hover card. Current sheet has a 2 pt `BLUE_DEEP` outline. Playhead 2 pt `BLUE_DEEP` with a white-ringed cap. An empty run is a dashed `INK3` outline. A `Used twice` badge on a drawing exposed twice. Track chip from the second track. One active track, `view.active_column`. The strip has no add button: the band's `Add drawing` is the one control for that.

| Gesture | Does |
|---|---|
| Tap | `select_cells` and `goto` the run start |
| Drag on the bar | Scrub |
| Drag a sheet's right edge (last 8 pt) | `sheet_ops::hold_delta` and `SetFrameCount`, committed once on release. The last sheet's end is the cut's end, so that edge is `SetFrameCount` alone unless another track holds the cut longer (AU5 notes) |
| Add drawing (the band's button, Alt+A) | `AddDrawing`, `InsertFrames { frame: run_end + 1, count: run_len }`, `SetCell`, `goto`, one undo. New hold equals the hold of the cell before. On an empty run, uniform. One drawing across the whole cut takes the next frame for one frame instead, and the cut stands (AU5 notes) |
| Speed | Popover: Fast 1 frame per drawing, Medium 2, Slow 3, plus Loop toggle. `retime_range` and `SetFrameCount`, one undo |
| Right click or long-press | `cell_menu` and `kind_menu`, now `pub(crate)` |
| Ctrl+C, X, V over the strip | Same clipboard, `pub(crate)` |
| Enter over the strip | `TypeIntoCell`, as over the sheet. Its handler opens the sheet through the shared `Editor::show_sheet()` (the `sheet_in_draw` switch in Draw, `open_pane` elsewhere), then calls `ask_entry()`. So the sheet opens with typing started, and a rebound chord does the same |
| Delete over the strip | Removes the drawing from the timeline (`ClearFrameKey`), one undo. It never removes a track, which is what `RemoveColumn` does over the sheet. `Editor::aimed(action)` makes this one translation at chord dispatch, in the App and the Rig and not inside `perform`, so `Animation ▸ Track ▸ Remove track` still removes a track and a rebound chord follows |
| Shift+Delete over the strip | Deletes the frame, as over the sheet |

**Routing.** The strip registers `(Pane::XSheet, rect, LayerId::background())` in `pane_rects`, so sheet chords fire over it exactly as over the sheet. `tabs.pane_rects.clear()` moves from `main.rs:8422` to the top of `Editor::ui`.

**Census.** The strip is one large widget, so the census of section 10 would count it as a backdrop. Each sheet's accesskit node counts as a control instead, capped at 12 visible sheets. A blank document adds 1.

**Thumbnails** arrive in AU13, cached by `(DrawingId, content_hash)` through `drawing_composite`, visible sheets only, one job at a time, never while `gesture_safe` is false.

**Left out:** drag reorder.

**Proof.** A property test runs random gestures through the strip and the same commands through the sheet: runs are equal and undo restores byte for byte.

---

## 8 Learning

### 8.1 Start

**Ready paper.** A fresh launch opens directly on a blank Draw step: Animation intent, 1280×720, 12 fps, one track `A`, strip open. The Start float sits over it. An untouched blank never triggers Leave, dirty or autosave.

**Start float.** Owned by `Editor.start: Option<Welcome>`, drawn inside `Editor::ui` as a non-modal `Area` in the main window, so the Rig covers it. Its geometry is the `Start` row of 4.5: width `clamp(pane_w / 2 − 20, 240, 320)`, so its left edge stays past the pane's centre (a pane under 520 wide may cross it, and no gated size has one), height `min(460, pane_h − 24)`, heading and close control outside the scroll. `Help ▸ Start screen…` also sets `editor.start`, with no Modal. The App's shortcut gate at `main.rs:6106` checks `self.welcome`, which stays `None`, so shortcuts stay live. It blocks only its own rect.

Three things close it: the `×` in its header (spoken `Close Start`), Esc, and the first finished stroke, which accepts Animation and keeps the stroke. "Finished stroke" means the stroke signal of 1.5 rule 11, so both stroke routes count. Opening Layers closes it too, because it is a right float (4.5).

| Entry | Lands on |
|---|---|
| `A drawing` (with `Square` and `Tall` chips) | Draw, `INTENTS::PICTURE`, 1920×1080, 2048×2048 or 1080×1920, strip closed |
| `An animation` | Draw, `INTENTS::ANIMATION`, strip open |
| `A comic` | Manga, Manga B5 |
| `Open a project` | File dialog |
| `Open the example` | Sample cut |
| `More sizes` | All seven templates plus `Custom size` (`Welcome.more_open`), inside the scroll body |
| Recent | Up to 2 rows, hidden when empty. The rest stay in `File ▸ Open recent ▸` |
| `Check your pen` | Draw one line, reads `Pressure works` or a plain fix naming Windows Ink |
| `What's new` | Today's tab |
| `Show at startup` | Off means the same paper with no float |

Budget, measured with `More sizes` closed and recent names of two words:

| Block | Height | Controls | Words | Fresh install |
|---|---:|---:|---:|---|
| Margin, heading `Start`, rule, `×` | 72 | 1 | 1 | Same |
| Three cards | 148 | 3 | 6 | Same |
| `Square`, `Tall` | in the cards | 2 | 2 | Same |
| `Open a project`, `Open the example`, `More sizes` | 104 | 3 | 8 | Same |
| Recent heading, two rows | 76 | 2 | 5 | Absent, 76 shorter |
| `Check your pen`, `What's new`, `Show at startup` | 60 | 3 | 8 | Same |
| Total | 460 | 14 | 30 | 384 tall, 12 controls, 25 words |

Against today's 19 and 74.

**One path for every choice.** Card choices go out through `AppRequest::Start(WelcomeAction)`, and the App hands them to a pure seam behind its Leave guard:

```rust
pub enum StartOutcome { Replace(Box<Editor>), Request(AppRequest), Stay }

/// What a Start choice does. No App, no window, no dialog.
pub fn apply_start(a: WelcomeAction, rs: Option<&RenderState>, dir: Option<&Path>, ui: &UiConfig) -> StartOutcome
```

`open_template` (`main.rs:4385`) and `welcome_show_me` (`main.rs:4394`) move behind it. `WelcomeAction` gains `Intent(usize)` for `INTENTS`. `h.answer_start(action)` calls `apply_start` and installs the result, so `austart_every_entry_lands_a_stroke` strokes on the document each entry really makes. `OpenRecent(path)` runs against a temp file. `Open` stays a logged request, because it needs a dialog.

New `templates::INTENTS` const outside `ALL`, so `tidy_sizes_come_from_the_templates` and `SIZES` do not move. Studio presets stay 24 fps. TV anime HD and Feature UHD `room` become Drawing with the sheet on. Animatic lands in Timing with the canvas tab front and the brush armed, because Timing arms Select. Every entry lands a stroke. The New form stays reachable. `Startup::Welcome` keeps its name; `Startup::NewForm` becomes `Startup::ReadyPaper`. Drop to open uses `raw.dropped_files` through the Leave guard.

**Upgraders** get the same paper and float, opened once on What's new with the old-to-new word table.

### 8.2 The Say line

One reserved lane in the band's transport row. Priority: refusal, then status, then hint. A teaching sentence stays until answered, not on a timer. A refusal dwells 4 s, words plus mark on `RED_WASH`. `Editor.say` reads live bindings through `chord_label`, and a hint whose verb has no binding is not shown. Long sentences wrap, per 4.4.

The action button (`Show me`, `Go to Draw`, `Fix it`) exists only while its hint shows.

| When | Line | Action |
|---|---|---|
| New document, no strokes | `Draw on the paper to begin.` | Show me |
| First stroke done | `Next: add a drawing.` | Show me |
| Blank next drawing | `The ghost is your last drawing. Draw the next pose.` | Show me |
| Three drawings, never played | `Press <Play chord> to play.` | Show me |
| Played | `Ready to share? Go to Export.` | Show me |
| Plan, stroke attempt | `This step shows a preview. Drawing happens in Draw.` | Go to Draw |
| Same refusal twice | Reason plus fix | Fix action if one exists |
| Flat pressure after three pen strokes | `Your pen is drawing without pressure.` | Fix it (opens the pen page) |
| An export finished outside the Export step | `Exported ball.gif.` | Open file |
| Color, art, no fill yet | `Pick a color, then tap an area.` | None |
| Effects, first visit | `Pick an effect to add it to the picture.` | None |
| A drawing exposed twice is selected | `This drawing appears twice. Changes show in both.` | None |
| Held frame selected | `This drawing stays on screen for N frames.` | None |
| First Hide tools | `Tap the dot, or press <chord>, to bring the tools back.` (card on the return dot) | None |

**A frame with no drawing of its own.** Four places cannot work here: fill (`canvas.rs:6636-6640`), draw to fill (`:6258-6262`), transform (`:1601-1605`) and refill (`doc.rs:5058-5062`). Today they set one status line, and the fill one sits behind the GPU refusal. They become refusals through one pure seam, so a test reaches the words with no GPU:

```rust
/// None when this frame has its own drawing.
pub fn no_own_cel_reason(&self) -> Option<&'static str> // on AppState
```

| Frame | Sentence |
|---|---|
| Held | `This frame holds an earlier drawing. Go to its first frame, or add a drawing here.` |
| Blank | `This frame has no drawing yet. Draw here first.` |

### 8.3 Quick Start

Five slots, `[bool; 5]`, `EXPORT_STEP = 4`. Ticks are outcomes, not clicks. A stroke outcome is the stroke signal of 1.5 rule 11 and nothing else, so a raster stroke and a vector stroke tick alike.

| Slot | Words | Ticks on | `Show me` does |
|---|---|---|---|
| 0 | Draw something | First finished stroke | Draws an undoable demo circle |
| 1 | Add a drawing | `AddDrawingAfter` | Performs it |
| 2 | Draw the next pose | Finished stroke at frame ≥1 | Draws the next circle |
| 3 | Play it | Playback runs | Plays |
| 4 | Export a file | A finished export file | Enters Export with Animated GIF ringed |

**The demo circle.** Today's Show me never draws a stroke (`main.rs:4939-4965`), so this is new. It uses `commit_raster`, the route a real stroke takes, because vector `AddStroke` is the legacy path. A shipped `doc.rs::ring_tiles(center, radius, width, rgba)` builds the tiles on the CPU. On an empty cell the drawing is made first, the way a stroke makes it (`raster_new_cel`, `canvas.rs:6786`). It is one undo and it fires the stroke signal.

Five dots in the Say line show the place. They count as no controls and no words. Help ▸ Quick Start stays.

### 8.4 Empty states

| Where | Empty state |
|---|---|
| Timing sheet | `(none yet — just draw)`, sentence case |
| Canvas, empty cell | `Draw to add a drawing here`, see below |
| Result | `Nothing to show yet.` plus `Set up standard composite` |
| Colors | Starter swatches plus `Add color` |
| Motion curves | `Animate a value to see its curve here` |
| Export, no art | `Nothing to export yet. Draw something first.` |
| Export queue | `Nothing waiting.` |
| Plan preview | `Nothing here yet.` plus `Go to Draw` |
| Recent | `Nothing yet` |
| Timing, under 2 drawings | `Add a second drawing to set timing.` plus `Go to Draw` |
| Color, no art | `Nothing to color yet.` plus `Go to Draw` |
| Effects, no art | `Nothing to add effects to yet.` plus `Go to Draw` |
| Check paint | `Run a check to find unpainted gaps.` |
| Manga pages | `No pages yet.` plus `New page` |
| Shots | `One shot so far.` plus `Add shot` |

**The canvas line.** It comes from today's hint at `canvas.rs:4874-4889`, whose gate has `!self.raster`, so a real raster canvas never shows it. The `!self.raster` term goes. The line paints when the view is an edit view, nothing is playing, `current_drawing()` is none, nothing is in progress, hints are on, its once-only bit is unset, and the document already holds at least one drawing. On a blank document the Say line says it instead, so the first screen does not carry the sentence twice. `Editor` sets `canvas.empty_cell_hint: bool` each frame. Text is `INK2` on `PAPER`, 15 pt. It sits on an empty cell, so it covers no art and `paper_under_chrome` stays 0. It is the one thing that paints on the paper, and the first finished stroke sets its bit.

### 8.5 How learning fades

Everything is once-only, never timed and never guessed. `ui.hints_seen: u32` is a bitfield. `ui.hints_off: bool` is one switch, in Help ▸ Hints and Settings. Off means no hints, no dots and no action button; the chrome does not change. Turning it back on clears `hints_seen`, so hints start again. There are no skill meters, no adaptive menus, and nothing over art. An upgrading config has `hints_seen` set to all-seen by the migration. The Lead can switch Hints off and on to see them.

---

## 9 Journeys

Today's numbers come from `.work/gaban-ui/understand/journeys.md`, measured on the real editor. The design columns are by construction until the part named under each table scripts them over the `Recorder`. It runs on a Rig that arms rooms as the App does and fails a journey on any refused step (1.5 rule 12). J1 and J2 end in a real file: the dialog counts as one action, `h.answer_export(temp_path)` calls the seam behind it, and the job runs to its end. A fill needs the GPU, so J3 runs in the ignored pass through `with_gpu` and asserts that pixels changed on the color layer. `aucolor_cpu_fill_lands_on_the_color_layer` proves the same landing in the ordinary suite.

### J1 Ball bounce, first-time animator (scripted in AU9)

| # | Today | Actions | Design | Actions |
|---:|---|---:|---|---:|
| 1 | Click `Web 720p` | 1 | Draw on the ready paper (the first stroke accepts the Start float) | 0 |
| 2 | Find `SHAKU`, click, select all, type 12, Enter | 4 | Length grows by itself | 0 |
| 3 | 12 strokes | 12 | 12 strokes | 12 |
| 4 | Next frame ×11 | 11 | Add drawing ×11 | 11 |
| 5 | Play | 1 | Play | 1 |
| 6 | File, Export…, MP4, Save (needs ffmpeg) | 4 | Export, Animated GIF, Save | 3 |

| | Today | Design | Gate |
|---|---:|---:|---:|
| Actions | 33 | 27 | ≤28 |
| Overhead | 10 | 4 | ≤5 |
| Concepts | 8 | 3 | ≤3 |
| Jargon | 9 | 2 | ≤2 |
| Result | MP4 only with ffmpeg | 12-frame GIF, 1.0 s at 12 fps | A real file in a temp folder |

### J2 One still picture (scripted in AU9)

| # | Today | Actions | Design | Actions |
|---:|---|---:|---|---:|
| 1 | Click `Custom size…` | 1 | `A drawing` | 1 |
| 2 | Open `PAPER`, pick a size | 2 | `Square` or `Tall` chip, only if wanted | 0 or 1 |
| 3 | `Start drawing` | 1 | Already on paper | 0 |
| 4 | Paint | creative | Paint | creative |
| 5 | File ▸ Export… | 2 | Export | 1 |
| 6 | Frame `to` field: click, type 1, Enter | 3 | A one-frame document has no range | 0 |
| 7 | `PNG sequence…` | 1 | `PNG image` | 1 |
| 8 | Choose a folder, cannot name the file | 1 | Save, with a name you choose | 1 |

| | Today | Design | Gate |
|---|---:|---:|---:|
| Overhead | 11 (9–13) | 4, 5 with a size chip | ≤4, ≤5 with a size chip |
| Files out | 48 PNGs | 1 named PNG | Exactly 1 |
| Concepts | 7 | 2 | ≤2 |
| Jargon | 6 | 0 or 1 | ≤1 |

### J3 Fill a drawing (scripted in AU13, ignored pass)

| # | Today, first palette fill | Actions | Design | Actions |
|---:|---|---:|---|---:|
| 1 | Click `仕上げ SHIAGE` | 1 | Color | 1 |
| 2 | Read the empty state, scroll the Palette pane | 1 | Tap a starter swatch | 1 |
| 3 | Click `edit model` | 1 | | 0 |
| 4 | Click the name field | 1 | | 0 |
| 5 | Type a character name | 1 | | 0 |
| 6 | Click `+ character` | 1 | | 0 |
| 7 | Click a colour pot | 1 | | 0 |
| 8 | Click a region | 1 | Tap a region | 1 |

Own three colours: Color 1, then Add color and pick ×3 = 6, then a region 1, for 8 actions.

| | Today | Design | Gate |
|---|---:|---:|---:|
| First fill | 8 | 3 | ≤3 |
| Own three colours | 17 | 8 | ≤9 |
| Concepts | 8 | 2 | ≤2 |
| Jargon | 9 | 1 | ≤1 |

### J4 Glow and a camera pan (scripted in AU14)

| # | Today | Actions | Design | Actions |
|---:|---|---:|---|---:|
| 1 | Click `撮影 SATSUEI` | 1 | Effects | 1 |
| 2 | Tab, type glow, Enter | 3 | Glow (live in the Result, wired for you) | 1 |
| 3 | Draw two wires | 2 | | 0 |
| 4 | Adjust threshold, radius, intensity | 3 | Strength slider | 1 |
| 5 | Tab, type camera, Enter | 3 | Camera move | 1 |
| 6 | Wire camera in and out | 2 | Frame the start | 1 |
| 7 | `bind ▾`, `+ new column` | 2 | | 0 |
| 8 | Switch room to reach the sheet | 1 | | 0 |
| 9 | Click the new column header | 1 | Go to the last frame | 1 |
| 10 | Key 1 | 3 | | 0 |
| 11 | Key 2 | 3 | Frame the end | 1 |

| | Today | Design | Gate |
|---|---:|---:|---:|
| Actions | 24 (one-button route 13, 31 nodes) | 7 | ≤8 |
| Wires drawn | 4 | 0 | 0 |
| Room switches after arriving | 1 | 0 | 0 |
| Concepts | 9 | 3 | ≤3 |
| Jargon | 10 | 1 | ≤2 |

### J5 Working animator on the keyboard (scripted in AU10)

| Verb | Today | Design | Gate |
|---|---|---|---|
| F, S, W, Q, =, -, Alt+1/2/3, Alt+I, Insert, O, Shift+I, E | Fire everywhere | Same, and over the strip | Same press count |
| Shift+Delete, Enter | Sheet only | Sheet and strip. Enter over the strip opens the sheet and starts typing | Same press count |
| Delete | Removes a track, sheet only | Same over the sheet. Over the strip it removes the drawing from the timeline, never a track | Same press count |
| Alt+K, Alt+B | Pointer-dependent, 2 | One verb each, everywhere | 0 pointer-dependent chords, as 6.4 counts them |
| Book, Bg column kinds | Alt+K, Alt+B over the sheet | Alt+O, Alt+W over the sheet | Same press count |
| Arrows | 0 of 4 work | 4 of 4 step frames | 4 of 4 |
| Room chords | 0 of 5 | 6 of 6 steps on F4–F9 | 6 of 6 |
| Hold handle | 22×22 | Same, plus the strip edge | Not smaller |

Example edit: W, =, =, Alt+2, Space is 5 presses and 0 clicks today and after.

---

## 10 Gate ledger

Every gate names its measure and the part that first runs it. Once a gate is on, it stays on. `(ignored)` means the gate lives in the ignored pass of 1.5 rule 5, and that part's ignored run includes its prefix. All others run in the ordinary suite.

| Gate | Today | Target | Measured by | Part |
|---|---:|---:|---|---|
| Seeded faults fail their audit, clean fixtures pass | n/a | Every seed fails | `auseam_` | AU1 |
| Stroke signal | n/a | Vector and raster routes bump it, fill and paste do not | `auseam_`, one `(ignored)` | AU1 |
| A refused step in a journey | Scores as done | Fails the journey, naming the step | `auseam_`, `Recorder` | AU1 on |
| Theme: Light and Dark give one style | 148,629 px differ | Style equal | `aulook_`, pixel diff `(ignored)` | AU2 |
| Contrast pairs | n/a | 44, 0 failures | `aulook_contrast_pairs_hold` | AU2 |
| Painted text contrast | n/a | ≥4.5 after blending, disabled and `INK3` ≥3.0, 0 failures in all rooms, menus and modals | `measure::text_contrast_audit` | AU2 |
| Translucent ink on text | 90 sites | 0 | `aulook_` | AU2 |
| Marks on paper | Page outline would be 1.07:1 | Every use reviewed, outline 4.58:1 | `aulook_` | AU2 |
| `render` config survives restart | Lost | Survives | `aulook_` | AU2 |
| Old config loads | Yes | Yes | `ship_older_configs_still_load`, golden | AU2 |
| Chromeless pane | n/a | Tab bar 0, body equals centre rect | `audock_` | AU3 |
| Every save reloads | Fails on an undrawn dock | 0 null rects | `audock_` | AU3 |
| Sheet choice survives a restart | n/a | Survives | `audock_`, real restart | AU3 |
| Clip audit, chrome | n/a | 0 clipped, 0 elided, 4 sizes × 3 scales, `PROBE_ROOMS` | `measure::clip_audit`, `Scope::Chrome` | AU3 on |
| Layout probe | 0 `MOVED` | 0 `MOVED` | `PROBE_ROOMS` | AU3 on |
| Japanese-first labels | 5 tabs | 0 | Census | AU3 |
| Visible Search | 0 | ≥1 | Census | AU3 |
| Paper share | about 48.7% | ≥63.0, 66.5, 57.5, 60.0 | `measure::paper_share` | AU4 |
| Pen can land (`journeys.md` canvas share, target ≥80% at the default size) | 54.5% | ≥82.5, 80.0, 77.5, 74.0 | `measure::paper_share` | AU4 |
| Paper under chrome | n/a | 0 | `measure::paper_share` | AU4 |
| Float moves art | n/a | 0 pt | `aurail_` | AU4 |
| Rail fits | n/a | Inside the pane, table tier, every room with a canvas, 4 sizes × 3 scales | `aurail_rail_fits_every_room` | AU4 |
| Both canvas hosts draw the overlay | Deck lives inside `Canvas::ui` | One function, two call sites, host rect obeyed | `aurail_` | AU4 |
| Deck homes | n/a | Every `TODAY_DECK` row by a pen-only path | `aurail_` | AU4 |
| Median control height | 24 | ≥32 | Census | AU4 |
| Controls under 24 | 30 of 72 | 0 | Census | AU4 |
| Essentials off screen | 3 | 0 | Census | AU4 |
| Pen on rail never paints, 1 pt beside paints | n/a | Holds | `aurail_` | AU4 |
| Float request during a stroke waits | n/a | Holds | `aurail_` | AU4 |
| `seen_pen` trips on stray Touch | Yes | No | `aurail_` | AU4 |
| Chord routing over a float | First rect wins | Top layer wins | `aurail_` | AU4 |
| Icon-only with no card | 11 | 0 | Census | AU5 |
| Pen hold survives 0.817 s | n/a | Holds | `austrip_` | AU5 |
| Band stability | n/a | 0 pt of control movement as the full-band latch opens | `austrip_` | AU5 |
| Strip opens | n/a | One refit, at pen-up or at open. 0 rail or transport movement | `austrip_` | AU5 |
| Two views agree | n/a | Runs equal, undo exact | Property test | AU5 |
| Held and blank refusals | One status | Two sentences, four sites, reachable without a GPU | `austrip_` | AU5 |
| First stroke clicks | 1 | 0 from launch, ≤1 from any entry | `austart_` | AU6 |
| Start entries landing a stroke | 5 of 8 | All | `austart_`, through `apply_start` | AU6 |
| Start float, worst case | 19 controls, 74 words | ≤14, ≤30 | Census | AU6 |
| First screen, `as_first_run()` | 19 controls, 74 words | ≤44, ≤70 | Census | AU6 |
| Paper uncovered with Start at its largest | n/a | ≥58.0% | `measure::paper_share` | AU6 |
| Intent entries | 0 | ≥2 | `austart_` | AU6 |
| Silent rooms | 1 | 0 | `ausay_` | AU7 |
| False copy | 4 | 0 | `ausay_` | AU7 |
| Stale teaching | 2 | 0 | `ausay_` | AU7 |
| Say line fits | n/a | Every hint and refusal, 0 elided, at 832×520 and 853×533 | `ausay_` | AU7 |
| GIF is valid | n/a | Decodes, frames and delays exact | Test decoder | AU8 |
| J1 | 33, 10, 8, 9 | ≤28, ≤5, ≤3, ≤2 | `Recorder`, real GIF | AU9 |
| J2 | 11, 48, 7, 6 | ≤4, 1, ≤2, ≤1 | `Recorder`, real PNG | AU9 |
| Quick Start ends in a file | Stops at step 3 | Ends in a file | `auexport_` | AU9 |
| Show me | 4 of 5 | 5 of 5 | `auexport_` | AU9 |
| MP4 with nothing installed (`journeys.md` target: works) | Greyed | **Not met by this plan.** Plain reason and GIF route instead. The Lead decides on bundling | Reported, not gated | End list |
| J5 | Table above | Table above | `Recorder` | AU10 |
| Arrows | 0 of 4 | 4 of 4 | `aukeys_` | AU10 |
| Step keys | 0 of 5 | 6 of 6 | `aukeys_` | AU10 |
| Pointer-dependent chords (6.4) | 2 | 0, exceptions listed | `aukeys_` | AU10 |
| Keymap goldens | n/a | 10 cases | `aukeys_` | AU10 |
| Chords | 120 | 118 unchanged, 2 moved and listed | `aukeys_` | AU10 |
| Controls, blank Draw, `as_shipped()` | 72 | ≤30 | Census | AU11 (interim tallies AU4, AU5) |
| Controls after a stroke | 98 | ≤36 | Census | AU11 |
| Words, blank Draw, `as_shipped()` | 100 | ≤40 | Census | AU11 |
| Menus | 9 | ≤6 | Census | AU11 |
| First-level rows | 143 | ≤60 | Census | AU11 |
| Menu table | n/a | Every `Action` on its 6.2 path in 6.2's order | `aumenu_` | AU11 |
| Commands in the palette | 154 of 154 | All, computed from `Action::ALL` | `aumenu_` | AU11 |
| Homes census | n/a | Every row has a home | `aumenu_` | AU11 |
| Jargon | 30 | ≤3 | Census, TERMS extended by composite, layout, queue | AU12 |
| Japanese words | 9 | 0 | Census | AU12 |
| Tooltip jargon kinds | 24 | ≤3 | Walk of `help()` and hover text | AU12 |
| Painted text median | 8 px | ≥10 | Census | AU12 |
| Painted text at ≤8 px | 58 of 98 | ≤10% | Census | AU12 |
| Palette reach | n/a | genga, douga, shiage, satsuei, henshū, shaku all found | `auwords_` | AU12 |
| J3 | 8, 17, 8, 9 | ≤3, ≤9, ≤2, ≤1 | `Recorder` `(ignored)`, real pixels | AU13 |
| Fill lands on the color layer | n/a | Pixels change there, the line layer does not | `aucolor_`, CPU route | AU13 |
| Result choice survives a restart | n/a | Survives | `aucolor_`, real restart | AU13 |
| Journey controls behind hover or scroll | 3 | 0 | Census | AU13 |
| J4 | 24, 4, 1, 9, 10 | ≤8, 0, 0, ≤3, ≤2 | `Recorder` | AU14 |
| Clip audit, all panels | n/a | 0 clipped, 0 elided, 4 sizes × 3 scales, `PROBE_ROOMS` | `measure::clip_audit`, `Scope::All` | AU15 on |
| Crowded blank room | 87 | ≤45 | Census | AU15 |
| Sample cut | 132 | ≤80 | Census | AU15 |
| Context-only verbs with a twin | 0 of 7 | 7 of 7 | `audress_` | AU15 |
| Shapes per room | 298–615 | Not above today | Probe | AU15 |
| Vertices per room | 5,154–9,124 | Not above today | Probe | AU15 |
| Tessellation | n/a | ≤407 µs | Probe | AU15 |
| Old token names | Live | 0, render pixel-equal across the removal | `autoken_`, pixel diff `(ignored)` | AU16 |
| Every gate re-run | n/a | All, misses named | Report | AU17 |
| Idle repaint | Today's | Unchanged | Not touched | All |
| Schema | 11 | 11, sample byte-stable | `ship_` | All |
| Suite | 828 | ≥828 plus new, green | `cargo test` | Every part |

**Census definitions** are ported from `.work/gaban-ui/journeys-probe/jprobe.rs:51-106` and `janalyse.py` and stay fixed. Widgets come from `h.ctx.viewport(|v| v.prev_pass.widgets)` with `w.layers()`. A control is focusable, under 5% of the window, with a short side over 6. A backdrop is ≥5%. One addition, for a widget that did not exist when today's numbers were taken: the strip's sheets count as controls through their accesskit nodes, at most 12. The Timing sheet stays a backdrop, so today's numbers stand. A blank document has one sheet, which is in the 29. With two drawings the count is 29 + 1 + 5 = 35 against the ≤36 gate: one more sheet, then Previous, Next, Ghost frames, Speed and Flip. The peg bar's labels run only to the end of the document, so a 1-frame document adds no words. A seam is ≤6. Text comes from walking `h.output().shapes`. Words are whitespace tokens with a letter. Jargon hits count unique strings against the 49 TERMS in `janalyse.py:14-63`, which may only be extended.

---

## 11 What is not changing

**Behaviour.** Canvas engine, strokes, brush maths, pen routes, RealTimeStylus. The internals of the node graph, curves, queue, board, episode, 3D and manga. One registry. `not now: why`. The palette. The F1 sheet. The refusal lane. A stroke creates a drawing. Onion and loop defaults. Length accepts `s+f`. Export starts at once. A blank graph exports. Steps arm tools. One-button builds. Crash recovery. Held danger 350 ms. `one second = N frames`. The Industry keymap. R and Space hold keys. Ctrl+K, F3, F1. Idle repaint is left out of this work. Timing sheet row density.

**Data.** Section 1.4 is the compatibility table. In one line: no stored name changes, every new config field defaults, and `.animproj` stays schema 11.

---

## 12 Not verified

Said plainly, as of this document:

- Nothing here has been built or run. The only things executed were the contrast script (44 pairs, 0 failures), the layout arithmetic, and a script that checked the 6.2 tables against `Action::ALL` and `Action::menu()` at `ebbc5fc`: every Action once, only `Preset1` to `Preset8` left off, 59 first-level rows, no duplicate path, no duplicate spoken name.
- Every design number in section 9 and every target in section 10 is by construction.
- The control and word totals in section 0 and 8.1 are hand counts of this document's own labels. The census may differ by a few. A miss means find out why, never move the ceiling.
- The flush dock probe was the proposals' scratch run. AU3's kill test repeats it on the real editor.
- Where the promise table's "about 48.7%" came from. AU1's grid reads 47.4% in the harness's frame and 49.5% flush. The target and its gate do not depend on it.
- Moving the arming block into `Editor` was read, not run. AU1 pins it with `auseam_arm_room_matches_todays_block`.
- `aufx_glow_changes_the_output_at_once` relies on `deliver::render` running the graph, which was read (`crates/anim-core/src/deliver.rs:130-143`), not run with a Glow.
- `mock-draw-1600.png` shows the transport row above the strip. This document puts it below, for the reason in 4.4. The mock was not redrawn.
- How many of the 14 menu-path pins listed in AU11 sit inside the budget's "Menu case 19" was not counted. AU11 reports the real number.
- No pen was used. Pen behaviour comes from reading code and the earlier sample counts.
- F-keys were not pressed on real hardware.
- GIF size and speed are unmeasured.
- The Done card's `184 KB` is an example, not a measurement.
- The older-build behaviour under Downgrade in 1.4 was read, not run.
- The understanding documents `shell.md`, `canvas.md`, `panels.md`, `pins.md`, `sketchbook.md` and `animux.md` reached this spec through other documents and `critic.md`, not all by direct reading. Canvas proposal sections 1.2–1.7, 5 and 5.1, and Steps proposal lines 1447–1520 and 1569–1726, were not read in full.

---

## 13 Batched list for the Lead

These cannot run headlessly. They go to the Lead once, at the very end. Target rig: XP-Pen Artist 22 Pro.

1. Pen hover and hover cards over the rail and paper.
2. The Windows press-and-hold ring on the rail and Size control. If it shows, the fix is `SetWindowFeedbackSetting` with `FEEDBACK_PEN_PRESSANDHOLD` (needs `Win32_UI_Input_Touch` and `Win32_UI_Controls`, HWND at `main.rs:3936-3940`).
3. Barrel button and eraser end (dropped today at `main.rs:4102, 4049`).
4. Mixed DPI, and a canvas in an OS window on a second display. Check there that the rail and all five floats show and work, that each float stays inside that window, and that the pen paints one point beside them. `aurail_both_canvas_hosts_call_one_overlay` proves the code path; only this check proves the real second window.
5. On a real GPU: the first raster stroke closes the Start float and ticks Quick Start's first dot, and `Draw to add a drawing here` paints on an empty cell of a raster canvas.
6. A real GPU instead of WARP, for everything else.
7. Windows Light mode, by eye.
8. 200% UI scale, and 1080p at 150%.
9. Pressure on first run, and with Windows Ink off.
10. Decision: bundle ffmpeg so MP4 works on a clean PC?
11. The look of the desk while painting.
12. Decision: add the Quick ring (contract in 6.1)?
13. GIF speed and quality on big frames.
14. Final review of step names and order, especially Plan and Effects.

**AU2 notes.** Choices the build made where this document was silent or spoke twice. None needs a device; they are here so the Lead sees them once.

- `lighttable_onion_tints_are_unchanged` cannot pass unedited, as AU2's gate says. 2.4 aliases `AO` to `INK3` and `LEGEND` to `INK2`, and the test asserted those names at their old values. Its two lines now read `PENCIL_AO` and `PENCIL_LEGEND`. All four pinned values are unchanged.
- The Timing sheet's head, from the key row to the chips, stands at `ROW_H` 18, not Panel 28, under "Timing sheet row density: Unchanged" and AU2's "no layout change". At 28 it cost the grid two rows. Whole rows now equal base at 1600×900 (Draw 15, Edit 19, Layout 14, InkPaint 12) and at 1280×720 (9, 12, 8, 7).
- Menus stand at Panel 28 with 2.6's gap, except a menu too long for the window at that height, which stands at `ROW_H` with `SHEET_GAP` (`plate::fit_menu`), because menus do not scroll. Today that is only Sheet: 34 rows need 1100 pt at 28 and take 690 at 18. Without the rule it lost 5, 8 and 11 rows at 1600×900, 1280×800 and 1280×720, where base lost none. AU11's shorter menus may retire the rule.
- Focus: 2.9's `selection.stroke` stays the edge of a focused field, and 2.8's ring stands 2 pt outside that edge. Both show.
- A stock `Button` stands 29: the Body row's 17 plus 2.6's `button_padding` 6 twice, a point over Panel 28. Both numbers are this document's, so neither moved. The canvas options row is `OPTIONS_H` 30 to hold it.
- The layer strip's rename field keeps the strip's 20 pt row.
- Every stock `ui.checkbox`, 11 of them, became `plate::latch`: with no stroke and radius 8, the style rounds a stock box to a pale `WELL` disc.
- The contrast audit reads a word's section colour, so a word painted by hand in a disabled scope is audited undimmed. The disabled look is checked apart, on each fill.
- The legend budget was 8; AU2 moved 9.
- `vocab` still holds the pencil words `ATARI`, `GENGA` and `SHUSEI` in capitals. Nothing reads them, the tiles paint sentence case, and AU12 owns the words.

**AU3 notes.** Choices the build made where this document was silent or spoke twice. None needs a device.

- A template's sheet is that project's. 5.3 says the button is remembered and the presets start sheet-on; AU3's item 4 says the field is copied into config. Copied whole, one TV anime HD project overwrote an explicit Hide and turned the sheet on for every later blank project. So `Editor.sheet_in_draw` carries the template's sheet, and only the button writes `ui.sheet_in_draw`, through `Editor.sheet_request`.
- Reset layout off the spine returns to the arrangement the place opened with (`Editor.place_dock`). The place stays, so the pill, the lit step, the status and the dock agree. A dock off the spine with no place behind it goes to Draw.
- The Timing sheet button is a latch in Draw only. In Color, Effects and off the spine it is a plain action, spoken `Show Timing sheet` and never lit, because a click there cannot hide the pane.
- `add_pane` never joins the Canvas leaf. A click on the paper focuses that leaf, and a tab there would cover the paper.
- The menu words take 2.6's `item_spacing`, 6, not a row of their own: 18 from word to word with the button's padding. `WIDE` and `WIDE_PLACE` did not move. The 32 pt that costs comes out of `room`: 81 at 1180, still over 72, so `3D layout` elides from 1180 to 1188 and shows whole from 1189, with the full name in its hover. AU11's narrower menus give it back.
- Search's field takes `R_INPUT`, as an input does. Its right edge stands 6 inside the key, because a 6 pt corner 2 inside would break the island's round end by 1.3 pt. The key stays 141.
- 2.7's band icons are the five transport keys as well as the Timing sheet button, so a transport key stands 32, not 28. They join the top row at `BAR_ICON` and the panels keep `ICON`.
- The retired ARMED line also named the fill role (normal, shadow, highlight) while fill was armed, and 4.2 did not move that word anywhere. The Palette's ringed pot marks the role today, and AU4's `Drawing with` card owes its name, so AU4's item 7 now says so.

**AU4 notes.** Choices the build made where this document was silent or spoke twice. The first two are the Lead's to settle: this document asks for two things that cannot both hold, and neither is called accepted. None needs a device.

- Paper under the rail at Fit, strip closed. 4.8 says paper under chrome is 0 at Fit with no float open, 4.1 and 4.8 say Fit stays 94% of the pane, and R17 allows one refit, at the strip's opening. At 1280×800 and 1024×640 with the strip closed they collide: 94% of the pane stands a 16:9 paper at left 38.4 and 44.1 under a rail that ends at 68, with 756 and 642 samples under chrome. The build keeps 94% wherever the paper clears the rail, so 1600×900 and 1280×720 do not move and no strip-open Fit moves. Elsewhere it takes the size that just clears the rail, never under the strip-open Fit: 1280×800 goes from 1203×677 to 1128×634, 1024×640 from 936×526 to 872×490, paper left 76 on both, and opening the strip is still the one refit. The two other answers: Fit on the reference height always, which the Reflow note turned down and which takes 1370×771 to 1277×718 at 1600×900, where nothing collides; or keep 94% and accept the overlap at those two sizes. A document wider than about 2.09:1 keeps 4.8's accepted case.
- Art under the rail outside blank Draw. With the Timing sheet on, and in Color, the rail covers the art's left edge at Fit, and 4.8's gate does not reach those arrangements. It follows from 4.9's own arithmetic: art starts at 433 and the rail ends at 465 at 1600 wide, 334 and 388 at 1280. Nothing was changed. The other answer is to fit these arrangements to the area right of the rail.
- The canvas's 14 pt mirror of the sheet's run marks is the sheet's: it shows only while a Timing sheet is on screen, in every room, so blank Draw has none. It stands 12 wide and ends at the rail's left edge.
- Derive stays held and red, as `plate::danger`, because palettes have no undo and it overwrites a chosen shadow. It moved to the name line, away from the swatches.
- A rail key's card is `Name (chord) — help`, 4.5's route, so `Exact UI descriptions` keeps working. The tool keys read the registry's line and start with the Action's label until AU12 renames those labels; Size, Brush color, Layers, Hand, Flip and `…` are written by hand in the plain form.
- The hover card replaced stock tooltips on the rail and in the floats, and on the two layer functions the Layers float shares with the sheet. The sheet's own tooltips stay for AU5, and menu rows stay stock.
- `set_width` is only a floor: the `More` fold widened a float by 16, the fold's indent over a slider set to the body's width, so the fold now sets `slider_width` to its own width.
- No shot shows the second OS window: the headless harness takes the embedded branch, so `aurail_both_canvas_hosts_call_one_overlay` proves the path and item 4 stays the real check.

**AU5 notes.** Choices the build made where this document was silent or spoke twice. None needs a device.

- Add drawing on one drawing across the whole cut. A first stroke on a blank 48-frame cut holds for 48 frames, a hold nobody chose, and 7.2's rule copied it: hold 48, cut 96, `Frame 49 of 96`. Now the new drawing takes the next frame for one frame, the rest of the cut stays and goes empty, the cut stands and it is one undo: `[1, 1, 46]`, `Frame 2 of 48`. From frame 10 it gives `[10, 1, 37]`, and from the cut's last frame `[48, 1]`, cut 49. After that the track has a rhythm and 7.2's rule holds again. `[48, 1]` from frame 1, `[1, 47]`, and shrinking the cut were weighed: the first still jumps 47 frames, the second makes a held drawing nobody asked for, and the third throws frames away.
- The last sheet's edge has no later key for `hold_delta` to move. It refuses to lengthen a tail, and a shortened tail would leave a stale empty key past the cut's end. So that edge is `SetFrameCount` alone, one undo, unless another track holds the cut longer: then it goes through `hold_delta`, the rest of the run goes empty and the cut stands. `SetFrameCount` has no lock of its own, so the strip's verbs refuse a locked track themselves, in the sheet's words.
- 4.4's Sound while dragging and the Say line's 240 floor collide just over the fold: the lane was 132 at 1040 and 184 at 1092. From 1040 to 1160 the key is its speaker icon, its spoken name and card keeping the words. The lane now stands 251 at 1040 and 252 at 1160, its two tightest.
- 4.4's foot words for the full transport had no row in Section 3, so AU12's flip would have missed them. They have rows now and live in `vocab`; `scrub`, `in` and `out` stay aliases. The test migration budget had no line for them either: they moved the pins of 3 older tests (2 in `playback.rs`, 1 in `audock_tests.rs`), reason `label`, which stand inside the ceiling of 157.
- A card never rises after a click until the pointer moves, as a stock tooltip never does. A `Tooltip` layer stands in egui's hit test, so a card rising beside a key just clicked lay over the next key and took its press. A disabled key wears a card too, by `contains_pointer`: egui never calls it hovered. What stays: a pen landing in the same frame on a key under a card that a plain rest raised loses that press, as under a stock tooltip.
- The hover card replaced the stock tooltips on the band, the strip and the Timing sheet's own controls. An elided Say line carries the rest of its words on a card, so `plate::say` turns off the label's own tooltip.
- Delete takes the sheet under the pointer, not the selected one, by 7.2 and the Rejected review notes. The sheet under the pointer lights, so the aim shows, and the cursor goes with the Delete.
- Sheet numbers and `Used twice` stand on one row, clear of the peg hole by 2 pt. The bar's labels take `STEEL_INK`; its second ticks stay white, as 7.2 and the Steel row say.
- The Say line's action slot is 92 pt, `plate::SAY_ACTION`: the longest action word at control size, its pad and the gap. Its rule stands only beside words, and its 6 pt stay either way, so no slot moves.

---

## 14 Part map

Seventeen parts, not about ten. The review found the first, fifth, sixth, seventh and last parts of the eleven-part map were each several deliveries in one, too much for an 80-minute part with review. Scope was not cut, so they split. The biggest visible changes still come first, in the order the Lead sees them: the look in the second part, canvas-first Draw by the fourth, the simple timeline in the fifth, the guided start in the sixth and seventh. The first part shows nothing on screen. It is the measuring floor, and it comes first so the look's contrast gates can fail on purpose before the look lands.

| Part | Title | The Lead sees | Prefix | Min tests | Depends on |
|---|---|---|---:|---|---|
| AU1 | Test seams and audits | Nothing on screen | `auseam_` | 16 | |
| AU2 | New look | The whole app in the new look | `aulook_` | 10 | AU1 |
| AU3 | Flush Draw chassis | One top row, six steps, flush canvas | `audock_` | 14 | AU2 |
| AU4 | Rail and floats | Rail, floats, canvas-first Draw complete | `aurail_` | 18 | AU3 |
| AU5 | Simple timeline | Strip, peg bar, Add drawing, Say line lane | `austrip_` | 16 | AU4 |
| AU6 | Start float and ready paper | Opens on paper, Start float | `austart_` | 14 | AU5 |
| AU7 | Say line and Quick Start | Hints, dots, Show me, empty states | `ausay_` | 12 | AU6 |
| AU8 | GIF and still engine | Nothing new on screen yet | `augif_` | 9 | AU7 |
| AU9 | Export step | Export step, Done card, a real GIF | `auexport_` | 12 | AU8 |
| AU10 | Keys and Hide tools | Arrows, F-keys, Hide tools | `aukeys_` | 14 | AU9 |
| AU11 | Menus | Six menus | `aumenu_` | 8 | AU10 |
| AU12 | Plain words | Every label plain, type at size | `auwords_` | 10 | AU11 |
| AU13 | Color step and strip pictures | Color recipe, swatches, thumbnails | `aucolor_` | 12 | AU12 |
| AU14 | Effects step | Shelf, in-series Glow, Camera move | `aufx_` | 12 | AU12 |
| AU15 | Panels | Advanced panels re-dressed | `audress_` | 8 | AU13, AU14 |
| AU16 | Alias retirement | Nothing, by design | `autoken_` | 3 | AU15 |
| AU17 | Release | What's New, 0.6.0, the Lead's list | `aurel_` | 4 | AU16 |

**Size calibration.** The `gaban-modern` plan's 22 parts ran a median of 80 minutes including review, about 600 to 1,500 new lines each. Each part here has its own gate and leaves a green suite.

**Cut order if time runs out,** first to last: Hide tools, drop to open, strip pictures, `Check your pen`, folded sections on the rarest panels. Nothing on a journey is cut. A cut is named in the report and goes on the Lead's list.

**Test migration budget.** Palette values 2 (AU2). Legends 8 (AU2). Room names 6 (AU3). "room" 4 (AU3, AU11). Menu case 19 (AU11). Trade terms 17 (AU12). Sheet-off 21 to 23 move 0, because of the frozen boot. About 56 are certain, 138 to 142 at the most, against the ceiling of 157 in `pins.md`. 15 trade-word tests stay unedited.

---

## AU1 Test seams and audits

Nothing changes on screen. Everything later parts measure with lands here, and each measure is shown to fail on a seeded fault before anything depends on it.

### Start from

- **First, one local commit of the shot harness and nothing else:** the uncommitted `Cargo.lock`, `app/Cargo.toml`, `app/src/main.rs`, `app/src/testkit.rs` and `app/src/uishot.rs`.
- `testkit.rs`: `Rig` `:45-58`, `Rig::frame` `:61-132`, `EditorHarness` `:136-182`, `with_dir` `:155-180`, `shot` `:191`, `sources()` `:1254-1266`, direct build `:1172`. `uishot.rs`.
- `doc.rs`: `refuse` `:951`, `commit_stroke` `:1046`, `commit_raster` `:1135`, `paste_pixels` `:2209`, call at `:2253`. `canvas.rs`: raster commit `:4410`, `paint_texels` `:9646`, `fill_rig` `:9688`. `main.rs`: arming block `:5975-5992`, `stack_room_seen` `:428`, `Performed` `:3016`.
- `workspace.rs::Stage::dock` `:204-278`.
- Census to port: `.work/gaban-ui/journeys-probe/jprobe.rs:51-106` and `janalyse.py`.

### Build

1. `EditorHarness::with_gpu(state)`, used only by ignored tests. This is the verified shape. Step 2 moves its shared lines into the funnel.

```rust
pub fn with_gpu(state: AppState) -> Self {
    let dir = tempfile::tempdir().expect("temp data dir");
    let rs = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        eframe::egui_wgpu::RendererOptions::PREDICTABLE,
    );
    let (config, config_notes) = Config::load_from(dir.path());
    let editor = Editor::from_state(state, Some(&rs), Some(dir.path()));
    let mut notes = Notes::default();
    for n in config_notes { notes.push(n); }
    let rig = Rig { editor, config, notes, performed: Vec::new(), requests: Vec::new(), pen: Vec::new(), dressed: false };
    let h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(12)
        .with_wait_for_pending_images(false)
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs))
        .build_ui_state(|ui, rig: &mut Rig| rig.frame(ui), rig);
    let mut me = Self { h, dir };
    me.run();
    me
}
```

2. **One funnel.** Every constructor ends in `build(state, dir, boot: Boot, rs)`, with `enum Boot { Frozen, Shipped, FirstRun }`. `new()`, `with_state()`, `with_dir()`, `with_ppp()` and `with_gpu()` are `Frozen`, so the 93 `new()` tests and 46 `with_state` tests all inherit it. `as_shipped()`, `as_first_run()`, `with_state_shipped(state)`, `with_dir_shipped(dir)` and `with_gpu_shipped(state)` are the other two. `frozen()` writes into `rig.config.ui` before the first frame. `h.rebuild_editor(state)` replaces the direct build at `testkit.rs:1172`, so it keeps the boot it was started under.
3. **Stroke signal,** per 1.5 rule 11. `AppState.strokes_done: u64`. `commit_stroke` and `commit_raster` bump it on success, and `commit_raster`'s body moves to a private `land_raster`, which `paste_pixels` calls, so a paste is not a stroke. A stroke whose tiles change nothing is not a stroke either: `land_raster` still answers where it would have landed, so `commit_raster` reads `engine.revision()` around it. A guest's streamed raster stroke never reaches `commit_raster`, so it counts at pen-up on the branch that hands it to the host. `paint_texels` becomes `pub(crate)` under `cfg(test)`.
4. **Arming moves out of the App.** `Editor::arm_room(&mut self, layers_cfg, seen: &mut &'static str)` holds the block from `main.rs:5975-5992`, unchanged in effect. The App keeps its `stack_room_seen` and passes it in, so the App's behaviour does not change. `Rig` gets its own `stack_room_seen` and `arms: bool`, false under `Boot::Frozen`, because today's Rig never arms and tests depend on that, and true under the other two.
5. **`Recorder`** in `measure.rs`: `h.act(..)` counts clicks, keys, strokes and answered dialogs against a named script. A step fails the journey, naming the step, if `refusal_seq` moved or a perform returned `Performed::Ignored`.
6. `uishot` writes every room at 1600×900 and 1280×800 through `with_gpu`, 30 frames per room, waiting for `GraphView::Ready`. With `rs = None` the canvas paints bare paper, so a shot without the GPU proves nothing. A viewer still on `Rendering` or `NoGpu` after the bounded wait stops the walk and names the room, and the run ends by reading its 16 files back. The frozen boot keeps the harness's frame in these pictures; `with_gpu_shipped` is the flush one.
7. `sources()` walks `app/src` recursively. Today the folder is flat; this protects later subfolders.
8. New `#[cfg(test)] mod measure` with:
   - `paper_share(&h) -> Share { paper, pen_land, paper_under_chrome }`, per 4.8. Add `canvas.dbg_layer` beside `dbg_rect` (`canvas.rs:904`, set at `:3836`). Layout's Canvas pane holds the preview, not the canvas's view, so `canvas.dbg_preview` makes its reading `Share::default()`: no paper there, and `view_xform` is the room before's. Its rows in `baseline.json` say `preview: true`, with no zoom.
   - `census(&mut h)`, per the definitions in section 10.
   - `clip_audit(&mut h, Scope) -> Clip { clipped, outside, elided }`. `clipped` is painted text whose `visual_bounding_rect` is cut by its clip by over 1 pt, skipping text clipped by a registered scroll viewport. `elided` counts `TextShape.galley.elided`. `outside` is controls outside the window. `Scope::Chrome` looks only at shapes outside dock pane bodies: top row, band, rail, floats. It goes by the clip, not the shape's centre: a shape cut inside a body (`plate::note_pane_body`, the clip the tab draws under) is the pane's wherever panning left it, and a float over a pane stays chrome whether it takes input or not. `Scope::All` looks at everything. It runs over `PROBE_ROOMS`.
   - `text_contrast_audit(&mut h)`: walk `h.output().shapes` in paint order. For each text section the effective colour is the section colour (`PLACEHOLDER` means `fallback_color`, and `override_text_color` wins), times the colour's own alpha, times `TextShape.opacity_factor`, blended over the backdrop. The backdrop is the prior filled rects under the text's centre, blended down to the first opaque one. Text whose `opacity_factor` is under 1, or whose colour is `INK3`, is held to 3.0, and all other text to 4.5. A `Mesh` or image behind the text counts as unknown and is listed, not passed.
9. `plate::note_viewport(ui, out.inner_rect, axes)`, one line at each of the 19 `ScrollArea` sites (`board_panel` 2, `canvas` 2, `config` 3, one each in `cmdpal`, `forge`, `history_panel`, `main`, `manga_tools`, `menus`, `nodegraph_panel`, `paintcheck`, `palette_panel`, `render_queue`, `welcome`, `xsheet_panel`). egui gives no other way to find a scroll viewport. `axes` are the ones that scroll, `[horizontal, vertical]`: a viewport owns a clip edge only on those, because across them the clip is a parent's, and the rule test reads them off the constructor. The node graph pans by hand, so it reports `plate::note_pan_view(ui, rect, reach)` the same way. The bodies exist only under `cfg(test)`.
10. `vocab.rs`, shared by shipped code and tests, with one `pub const` per row of section 3. **In this part every constant holds today's word.** Pin test included.
11. `Stage::dock_with_sheet()` returning today's Drawing recipe, equal to `dock()` for now. Later parts add to `frozen()`.
12. Migration list. `view3d_numpad_keys_drive_the_view_over_the_pane_and_stay_the_old_keys_elsewhere` (`view3d.rs:859`) and `view3d_alt_drag_navigates_with_a_pen_and_frame_all_shows_the_whole_model` (`view3d.rs:922`) are the two `with_state` tests in `pins.md` Appendix B. `sheetui_waveform_column_reads_the_picked_tracks_level_once_per_change` (`xsheet_panel.rs:2881`) also needs the sheet and is not in the appendix. All three inherit the frozen boot through the funnel and change no line. Tests calling `Stage::X.dock()` directly (`board_panel.rs:883`, `paintcheck.rs:725`, `render_queue.rs:1597`) read the shipped recipe on purpose. A test about the current recipe calls `h.editor().recipe(st)` once AU3 adds it.
13. Baseline: an ignored test records today's census for all rooms in `C:/Users/Lancer/Desktop/LsXavecs AGI/.work/gaban-ui/baseline.json` (path from an env var, the way `UISHOT_DIR` works), and the same numbers go in a const table in `measure.rs`, so the suite reads nothing outside the repo.
14. Assert-count ratchet: count `#[test]` and `assert` uses across `sources()` and fail if either falls below a const floor. Each part raises the floor, never lowers it.
15. **Flush for the person's boots.** egui_kittest wraps a `Ui` harness in `Frame::central_panel(..).outer_margin(8.0)` (`app_kind.rs:55-57`); `App::ui` has no such frame. `Rig::frame` lays out in a child `Ui` on `ctx.content_rect()` under `Shipped` and `FirstRun`, so `paper_share` and the later floors read the window the person has. `Frozen` keeps the frame, so BASELINE and the 72/100/30 pin stand.

### Tests (`auseam_`, at least 16)

`auseam_every_constructor_goes_through_the_funnel`, `auseam_frozen_boot_is_todays_recipe`, `auseam_with_state_is_frozen_too`, `auseam_sources_walk_is_recursive`, `auseam_assert_floor_holds`, `auseam_vocab_table_is_pinned`, `auseam_census_reads_the_baseline`, `auseam_paper_share_counts_a_float_as_chrome` (seed an interactable `Area` over the paper), `auseam_paper_share_counts_a_same_layer_widget`, `auseam_stroke_signal_vector_route`, `auseam_stroke_signal_raster_route` (CPU, through `paint_texels`), `auseam_stroke_signal_skips_fill_and_paste`, `auseam_stroke_signal_on_a_real_device` (ignored, `with_gpu`), `auseam_arm_room_matches_todays_block`, `auseam_app_holds_no_arming_block` (rule), `auseam_rig_arms_unless_frozen`, `auseam_recorder_fails_on_a_refusal`, `auseam_recorder_fails_on_an_ignored_action`, `auseam_contrast_audit_multiplies_opacity_and_alpha`, `auseam_contrast_audit_fails_a_seeded_low_ink`, `auseam_clip_audit_fails_a_cut_label`, `auseam_clip_audit_skips_a_scroll_viewport`, `auseam_clip_audit_counts_elision`, `auseam_every_scroll_area_is_registered` (rule), `auseam_shipped_boot_is_flush_to_the_window`, `auseam_nothing_under_test_opens_a_window` (rule), `auseam_paper_share_reads_the_persons_window`, `auseam_paper_share_reads_no_paper_in_layouts_preview`, `auseam_clip_audit_keeps_a_float_over_a_pane`, `auseam_clip_audit_reads_a_control_by_its_clip`, `auseam_clip_audit_owns_only_the_scrolled_axis`, `auseam_clip_audit_owns_a_pan_view`, `auseam_clip_audit_reads_the_node_graph_as_a_pan_view`, `auseam_stroke_signal_on_a_guests_stream` (ignored, `with_gpu`), `auseam_shots_owed_are_sixteen_at_two_sizes_and_answer_shots_before`, `auseam_shots_check_reads_the_folder`, `auseam_shots_stop_on_a_viewer_with_no_picture`, `auseam_shots_pass_a_room_with_no_viewer`.

### Gates

- Every seeded fault fails its audit, and the clean fixture passes.
- Ratchets on. Baseline matches `journeys.md` for Blank Drawing: 72 controls, 100 words, 30 jargon. A mismatch means the census port is wrong; fix the port.
- The audits run over today's rooms and report only. They start gating in AU2.
- `cargo test` green, ≥828 plus new. Ignored shots written to `.work/gaban-ui/shots-after/`.

### Must not change

Everything on screen. Dock recipes, what a room arms in the App, `.animproj`, exported pixels, pen routes.

### Leave out

The look, renames, new Actions, any shipped behaviour beyond the two moves above (`arm_room`, `land_raster`).

## AU2 New look

The Lead's first look: the whole app in the new look, arrangement unchanged.

### Start from

`plate.rs`: tokens `:35-57`, derived `:114-185`, `visuals` `:934-993`, `legend` `:350`, `font_definitions` `:259`, `surface` `:1036`. `main.rs::dress` `:181-187`. `config.rs::read_sections` `:2824-2850`. `xsheet_panel.rs::identity_swatches` `:2019-2034`. `manga_tools.rs::page_thumb` `:812-838`, `paint_page` `:864-889`. `canvas.rs:3969-4001`.

### Build

1. Tokens and aliases per 2.2 and 2.4. Go through the 71 `AO` and `AKA` uses once: document tints to `PENCIL_AO` and `PENCIL_AKA`, armed or selected to `BLUE`, the rest stay on the alias. Fix `STRUCK`-on-`TALLY` pairs.
2. **Marks on paper.** Grep every `GRAPHITE`, `WELL`, `STRUCK` and `LEGEND` use that is painted on paper or used as a stroke, and apply the review table in 2.4. Each site gets a line in the commit body.
3. **Solid ink.** `legend_dim()` becomes opaque `INK2` for its 90 text sites. Its roughly 61 non-text uses (strokes, frames, grips) move to new `plate::hairline()`, which is `RULE`, each one looked at. `legend_muted()` becomes opaque `INK3`.
4. `CHIP_*` constants per 1.4, and `identity_swatches` reads them.
5. `visuals()` per 2.9 with `disabled_alpha = 0.65`, the theme pin in `dress`, derived colours computed, grain removed, `swatch` gets its `INK3` ring. Every control painted by hand in `plate` uses `INK` inside a disabled scope, per 2.8.
6. Type per 2.5: `font_definitions`, text styles, `drag_value_text_style`, sentence-case `legend`. The 132 hard-coded sizes wait for AU12.
7. Spacing and radii per 2.6 through the style.
8. `render` load fix: one line in `read_sections`.

**Deliberate test moves:** `the_palette_is_exactly_the_ratified_one` and its twin (2) re-pin to section 2. Legend tests (8) move to sentence case through `vocab`. Check the five absolute geometry tests (`nodegraph_panel.rs:1232, 1261`, `curves_panel.rs:888, 909`, `doc.rs:6731`): if padding moves them, re-derive the geometry from the layout, never widen a tolerance.

### Tests (`aulook_`, at least 10)

`aulook_theme_is_pinned_in_both_slots`, `aulook_system_light_gives_the_same_style`, `aulook_tokens_are_the_table`, `aulook_contrast_pairs_hold` (the 44 rows of 2.3), `aulook_painted_text_contrast_holds_everywhere`, `aulook_disabled_button_holds_on_each_fill` (a real disabled `Button` on `ISLAND`, `WELL` and `DESK`: fails at alpha 0.5, passes at 0.65), `aulook_no_translucent_ink_on_text`, `aulook_type_roles_are_the_table`, `aulook_legend_is_sentence_case`, `aulook_render_section_survives_a_restart`, `aulook_v050_config_golden_loads` (built by hand, no `api_key`, no `totp_secret`), `aulook_manga_page_outline_shows_on_paper` (non-text pair, `INK3` on `PAPER`, ≥3.0), `aulook_miss_check_paper_stays_dark`, `aulook_paper_veil_stays_dark`, `aulook_identity_swatches_keep_todays_values`.

### Gates

- 44 contrast pairs, 0 failures. Text audit 0 failures across the 8 `PROBE_ROOMS` on a blank document and on the sample, every menu, the palette, the save prompt, recovery and the F1 sheet.
- Style equal under `system_theme` Light and Dark. In the ignored pass, the pixel diff between them is 0 (today 148,629).
- `lighttable_onion_tints_are_unchanged` passes unedited. The miss check and veil tests at `canvas.rs:9974-9999` pass through the token.
- `cargo test` green, ≥828 plus new. Shots written.

### Must not change

Dock recipes, the words on any label (case only), `.animproj`, stored `LayerProps.chip` values, exported pixels, pen routes, `plate` public function names.

### Leave out

No layout change, no rail, no renames, no menu moves, no new Actions, no 132-size sweep, no alias deletion.

## AU3 Flush Draw chassis

**Recover the Lead's saved rooms (added 2026-10-30, found on the real data folder).** v0.5.0 rejects the Lead's own `workspaces.json` (saved 07-18): an undrawn dock wrote its infinite rects as JSON `null`, `load_from` (`workspace.rs:363`) fails, and the file is kept as `.bad`. Normalising new saves does not bring that file back, so `load_from` must also repair old ones: `null` rect coordinates become finite before the `DockState` parse, and nothing else in the file changes. Use the real file as a fixture (a copy is at `C:/Users/Lancer/Desktop/LsXavecs AGI/.work/gaban-ui/preview/appdata-backup/workspaces.json`; panes and fractions only, no personal data; put it under the app's test fixtures). Add `audock_recovers_null_rect_workspaces`: every saved workspace in the fixture loads, with its name and panes intact, and no note and no `.bad` file. Add the one-line root cause to `LESSONS.md`.

After this part the app has one top row, six plain steps and a flush canvas in Draw. The old tool deck is still there, so nothing is lost while the rail is built.

### Start from

`workspace.rs`: `Stage` `:132-200`, `dock()` `:204-278`, `Workspaces::save_to` `:403`. `main.rs`: `native_options` `:154-163`, `enter_room` `:4755`, `from_form` `:4797`, `from_state` `:4879`, `open_pane` `:5186-5199`, `ResetLayout` `:3195-3199`, `apply_ui_scale` `:5903`, top bar `:7361-8107`, slate tabs `:7803-7824`, DockArea `:8431-8440`, host send `:2085-2097`. `config.rs::Action::label` `:543-547`.

### Build

1. **Kill test first.** On the real editor: a one-tab flush dock draws with tab bar 0 and body equal to the centre rect, saves with 0 null rects, reloads. If it fails, stop and use the bare-host fallback in R1, recorded in the report.
2. `dock_style` per 4.6, used at the `DockArea`.
3. `Stage::dock()` Drawing becomes `DockState::new(vec![Pane::Canvas])`. `dock_with_sheet()` holds today's recipe. Drop the vestigial Brush band from every recipe (`workspace.rs:214, 224, 237, 274`). The runtime removal loop stays.
4. `ui.sheet_in_draw` with `Editor.sheet_in_draw`, plus `Editor::recipe` per 5.3, used by `enter_room`, `boot`, `boot_form` and Reset. The App and the Rig copy the field into config when it changes and save it the App's way. `Editor::boot`, `boot_form` and `apply_ui` per 5.4 at all 13 sites, and the funnel in `testkit.rs` builds through them. `frozen()` sets the field true. TV anime HD and Feature UHD open sheet-on.
5. `Timing sheet` button in today's foot, spoken `Show Timing sheet` and `Hide Timing sheet`. In Draw it switches recipe, unless the dock is neither recipe (hand-arranged), when it uses `open_pane` or closes the tab so a hand arrangement is not thrown away. In Plan and Timing it is absent. Elsewhere it runs `open_pane(Pane::XSheet)`.
6. `open_pane` on a single leaf: `split_right(root, 0.70, [pane])`. Rename the dead override `fn closeable(&mut self, tab: &mut Pane)` (`main.rs:4662`) to `fn is_closeable(&self, tab: &Pane) -> bool`, per 4.6: egui_dock calls only `is_closeable`, so today the Canvas tab shows a close button.
7. Top row per 4.2: `Panel::top("top_bar").exact_size(44.0)`, one row. The nine lower-case menus stay in this part. New `steps.rs` per 5.1 draws the pills. The Export pill runs `ShowRenderQueue` until AU9. Off-spine pill with its width rule. Undo, Redo, Search. Dirty dot. ARMED line, filename and facts readout retired, and the shot menu stays where it is.
8. Step names flip: `vocab::STEP_*` get the real words. `Stage::name()`, `describes()` and `card()` read them. `Action::label` for rooms becomes `Plan step`, `Draw step` and so on, with the old words added to `aliases`. Status reads `step: Draw`. Room-name tests (6) and "room" tests (first 2 of 4) move by `vocab`.
9. `with_min_inner_size([1024.0, 640.0])`, pure `effective_scale(stored, w, h)` per 4.7, used by `apply_ui_scale`.
10. `Workspace::normalised()` at `save_to` and the host send. If `egui_dock` gives no public write to a node's rect, go through `serde_json::Value`: inside any object holding `min` and `max`, a null `x` or `y` becomes 0.0.
11. `app/Cargo.toml:17`: `version = "=0.20.1"`.
12. Narrow rule at 1120 pt, 1180 with the off-spine pill, in this part.

### Tests (`audock_`, at least 14)

`audock_flush_body_is_the_centre_rect`, `audock_flush_dock_saves_and_reloads`, `audock_every_save_path_normalises`, `audock_old_workspaces_still_load`, `audock_draw_is_canvas_alone_when_shipped`, `audock_sheet_button_is_one_click_and_remembered`, `audock_sheet_choice_survives_a_restart` (a shipped harness over a dir clicks `Show Timing sheet`, config is saved, a second harness over the same dir opens Draw with the sheet), `audock_every_editor_is_built_through_boot` (rule), `audock_sheet_button_keeps_a_hand_arrangement`, `audock_reset_reads_the_sheet_switch`, `audock_frozen_boot_keeps_the_sheet`, `audock_top_row_is_one_row_at_every_size`, `audock_steps_show_in_order_and_run_their_actions`, `audock_step_card_names_the_studio_word`, `audock_off_spine_pill_shows_and_clears`, `audock_long_place_name_elides_in_the_pill` (expects exactly one elided text), `audock_undo_redo_search_work_from_the_top_row`, `audock_effective_scale_caps`, `audock_open_pane_splits_a_single_leaf`, `audock_canvas_tab_cannot_close`, `audock_chrome_clip_audit_passes`, `audock_narrow_row_collapses_to_menu`.

### Gates

- Pane body equals the centre rect, tab bar 0, 0 null rects.
- Every save reloads. Old workspaces load.
- Clip audit, `Scope::Chrome`: 0 clipped and 0 elided on the top row and band, over `PROBE_ROOMS` at 1600×900, 1280×800, 1280×720, 1024×640 at 100%, 125%, 150%. Panels keep their old minimum sizes until AU15, so their bodies are not gated here.
- Layout probe 0 `MOVED` lines (`main.rs:6601, 6703`).
- Japanese-first labels 0 on the top row. At least 1 visible Search.
- Suite green, shots written.

### Must not change

`stage_key` strings, `Stage::ALL`, `Pane`, what each step arms, the room recipes other than Draw and the Brush band, the nine menus' contents.

### Leave out

Rail, floats, deck changes, strip, new Actions, `Stage::Export`, menu reshaping.

## AU4 Rail and floats

After this part Draw is canvas-first: paper at its full size, tools on a rail, depth in floats. A canvas in its own OS window keeps its tools.

### Start from

`canvas.rs`: `dish_ui` `:2306`, `lightbox_rail_ui` `:2755`, `brush_rail_ui` `:3091`, `brush_ui` `:3355`, `full_deck` `:3381`, options row `:3594-3739`, `canvas_tool_deck` `:3806`, R flip `:3825-3832`, Space hand `:3857-3861`, right rail `:5148-5233`, left fold-out `:5260-5316`, Paint Dish `:5318-5379`, `handle_pen` gate `:7027, 7073`, `seen_pen` loop `:7054-7060`, `set_tool` `:1451`. `main.rs`: `shortcut_ctx` `:5168`, `pane_rects.push` `:4564`, `Pane::Canvas` arms `:4620-4652`, float canvas viewport `:8470-8513`, hand-tool Actions `:3253-3292`. `menus.rs::hover_tip` `:62`. Reach-ins: `cmdpal.rs:508`, `doc.rs:7245`.

### Build

1. New `rail.rs`: the rail per 4.3, with all four tiers and the reference height. Rail buttons call `set_tool` directly. Clicking an armed tool opens its options. Manga's slot 9 holds `Panel`.
2. New `floats.rs`: the five floats per 4.5, with the size, scroll and top rules. Move the existing bodies, do not rewrite them. Each keeps its widget ids and spoken names. `CanvasOptions` Action, in today's View menu until AU11.
3. **One overlay.** `rail::overlay(ui, &mut OverlayArgs, host)` per 4.5, called after `canvas.ui` at the `Pane::Canvas` arm and in the viewport closure. Floats in the OS window push no `pane_rects`.
4. **Hand:** a held state like Space, cleared by any tool. **Flip:** momentary, `Sense::drag`, shown from the second drawing. **Size:** drag only, 48×84, true-size bubble, new `set_brush_px`, refused mid-stroke.
5. **The Fill chooser and `Keep inside strokes`** per the deck table in 4.5. One `keep_inside_row` function is called from Brushes and from Tool options.
6. **Retire** `canvas_tool_deck`, the left fold-out and the right hover rail as surfaces. Every row of `TODAY_DECK` gets its home, and that const lands with this step.
7. `plate::hover_card` per 4.5. The rail and floats use it now. The band and strip join in AU5. The Brush card's `Drawing with` line carries the fill role's name while fill is armed, the one job of the ARMED line nothing else took (AU3 notes).
8. `shortcut_ctx` keeps the layer: `pane_rects: Vec<(Pane, Rect, LayerId)>`. `let top = ctx.layer_id_at(p)`, then find `r.contains(p) && top == Some(*layer)`, with no `.rev()`, falling back to `dock.find_active_focused()`. Floats push entries: Layers as `Pane::Layers`, others as `Pane::Canvas`.
9. `seen_pen` latches only on a pen Start on the canvas layer, not on any Touch event.
10. Reach-ins go through one helper, so tests and the palette do not know where a control lives. Tests reach a float through `h.open_float(..)`. There is no test-only path in shipped code.
11. Moved tests are listed with reason `reach`.

### Tests (`aurail_`, at least 18)

`aurail_order_and_sizes_are_the_table`, `aurail_nothing_moves_when_the_island_grows`, `aurail_reference_height_ignores_the_strip_latch`, `aurail_armed_tool_opens_its_options`, `aurail_hand_is_held_and_cleared_by_any_tool`, `aurail_size_drag_sets_px_and_refuses_mid_stroke`, `aurail_flip_is_momentary`, `aurail_pen_down_on_rail_never_paints` (`stroke_tap` stays empty), `aurail_pen_one_point_beside_paints`, `aurail_touch_replay_on_size_is_harmless` (1.2 s replay), `aurail_float_during_a_stroke_waits_for_pen_up`, `aurail_float_never_moves_art`, `aurail_only_one_float_per_side`, `aurail_hover_card_passes_the_pen`, `aurail_chord_over_a_float_goes_to_the_float`, `aurail_stray_touch_does_not_set_seen_pen`, `aurail_paper_share_holds_at_every_size_and_aspect`, `aurail_tiers_are_the_table`, `aurail_rail_fits_every_room`, `aurail_every_float_fits_and_scrolls`, `aurail_every_deck_verb_has_a_home` (walks `TODAY_DECK`: each row's taps run with a pen only and the row's state is read back), `aurail_fill_chooser_reaches_draw_to_fill`, `aurail_keep_inside_strokes_serves_brush_and_eraser`, `aurail_manga_rail_holds_the_panel_tool`, `aurail_both_canvas_hosts_call_one_overlay` (rule), `aurail_overlay_takes_the_host_rect` (a host rect away from the pane: rail and floats stay inside it).

### Gates

- Paper ≥63.0, 66.5, 57.5, 60.0 and pen ≥82.5, 80.0, 77.5, 74.0 at the four sizes, band at 92.
- Paper under chrome 0 at Fit for 16:9, 4:3, 1:1, 9:16 with no float open.
- A float moves art by 0 pt.
- Rail fits in every room with a canvas at 4 sizes × 3 scales. Every float fits its host and scrolls. Clip audit `Scope::Chrome` now covers rail and floats.
- Median control height ≥32, 0 under 24, essentials off screen 0, in Draw under `as_shipped()`.
- Every `TODAY_DECK` row has a home and a passing pen-only path.
- Report the interim blank Draw tally of controls and words.
- Suite green, shots written.

### Must not change

Stroke, brush and pen code paths past the layer gate. `CanvasTool` and `SelShape`. Fit. Tool Actions' toggle behaviour. R and Space. The native pen route. The rule that `Exact UI descriptions` keeps working.

### Leave out

Strip, band rework, Radial, drag-to-reorder of the rail, any saved float position.

## AU5 Simple timeline

After this part time lives along the bottom: strip, peg bar, Add drawing, a plain transport and the Say line lane.

### Start from

`main.rs`: foot `:8112-8269`, `play_tick` `:7100-7133`, `foot_right` `:7139-7231`, `tabs.pane_rects.clear()` `:8422`, `TypeIntoCell` `:3185-3188`, `stroke_guarded` `:3759`, chord dispatch `:6100-6134`. `testkit.rs` chord dispatch `:127`. `xsheet_panel.rs`: clipboard `:174`, follow `:1404-1414`, hold handle `:1776-1845`, undo flood `:1850-1864`, `cell_menu` `:1897`, `kind_menu` `:1982`. `board_panel.rs::ui_episode` `:714-765`. `doc.rs`: `select_cells` `:2853`, `goto` `:3630`, `toggle_play` `:3639-3652`, `play_range` `:2693`, `new_drawing_at_frame` `:2418-2421`, `clear_key_at_frame` `:2538`, refill status `:5058-5062`. `sheet_ops.rs` `:134-138, 197-208`. `crates/anim-core/src/xsheet.rs:190`. `canvas.rs`: `:1601-1605, 6258-6262, 6636-6640`.

### Build

1. New `strip.rs` per 7.2. No data of its own, no second code path for edits.
2. Band per 4.4: `exact_size`, 92 with strip, 36 without, strip above the transport row. Plain and full transport follow one predicate: `visible_panes` contains `XSheet`.
3. Strip predicate, R16. `strip_open`, a session field, is set at open if `frame_count > 1` or by Animation intent or by Add drawing, and is never set false by itself. Full-band latch from the second drawing. Its slots are reserved, and `strip_open` is the one accepted refit (R17).
4. `AddDrawingAfter`, chord Alt+A, in today's Sheet menu for now, and in `stroke_guarded`. `GoToFrame`, the counter tap and its hover facts.
5. Longer and Shorter on the sheet edge. Speed popover.
6. Routing per 7.2. `pane_rects.clear()` moves to the top of `Editor::ui`, so the strip's entry survives.
7. **Enter.** `Editor::show_sheet()` is the shared open: the `sheet_in_draw` switch in Draw, `open_pane` elsewhere. The `TypeIntoCell` handler calls it, then `ask_entry()`, so Enter opens the sheet and starts typing in any place and a rebound chord behaves the same.
8. **Delete.** `Editor::aimed(action)` per 7.2 at both chord dispatch sites.
9. Say line lane: refusal and status from `main.rs:8227-8269` merge into it. Refusal dwell 4 s, words plus mark. Hints come in AU7, and the lane already reserves the action button. Wrap rule per 4.4.
10. Foot items per 4.4. Transport cards stop hard-coding chords (`main.rs:8120-8205`) and read `chord_label`.
11. **The hold trap, R19.** Stop with the sheet off screen goes to the run start. With the sheet on screen it stays frame-exact. Tap on the strip lands on the run start. `AppState::no_own_cel_reason()` per 8.2 at its four sites, each as a refusal.
12. Narrow fold per 4.4.

### Tests (`austrip_`, at least 16)

`austrip_sheets_are_the_column_runs`, `austrip_two_views_agree` (property), `austrip_undo_is_byte_exact`, `austrip_tap_lands_on_the_run_start`, `austrip_bar_drag_scrubs`, `austrip_edge_drag_commits_once`, `austrip_add_drawing_is_one_undo_and_copies_the_hold`, `austrip_add_drawing_on_an_empty_run_stays_uniform`, `austrip_speed_is_one_undo`, `austrip_pen_hold_survives_long_touch`, `austrip_long_press_opens_the_cell_menu`, `austrip_sheet_chords_fire_over_the_strip`, `austrip_enter_opens_the_sheet_and_starts_typing`, `austrip_rebound_type_chord_still_works`, `austrip_delete_removes_the_drawing_not_the_track`, `austrip_menu_remove_track_still_removes_a_track`, `austrip_band_follows_the_sheet`, `austrip_transport_row_stays_on_the_bottom_edge`, `austrip_full_band_latch_moves_nothing`, `austrip_strip_opens_with_one_refit_at_pen_up`, `austrip_add_drawing_waits_for_pen_up`, `austrip_stop_settles_only_with_the_sheet_off_screen`, `austrip_no_own_cel_reason_is_the_table` (pure, on `AppState`), `austrip_held_and_blank_refusals_reach_all_four_sites` (rule), `austrip_held_fill_refuses_with_the_held_sentence` (`fill_rig` pattern, past the GPU gate), `austrip_strip_predicate_table`, `austrip_census_counts_strip_cells`, `austrip_every_icon_has_a_card_and_a_name`.

### Gates

- Icon-only with no card 0.
- Two views agree over at least 200 random gestures, seeded.
- Pen hold survives 0.817 s on the strip.
- Band stability: no control moves as the full-band latch opens. Strip opens: one refit, and no rail or transport control moves.
- Clip audit `Scope::Chrome` and layout probe still pass. Report the interim tally.
- Hidden panels do not stop playback: `play_tick` runs before panels at `main.rs:7360`.
- Suite green, shots written.

### Must not change

`Column`, `sheet_ops` semantics, Timing sheet density and behaviour, the `one second = N frames` readout, `Stroke creates a drawing`, loop defaults, playback's viewer gating (`main.rs:8303-8311`), what a stroke on a held frame does.

### Leave out

Thumbnails (AU13), drag reorder, a second active track, hints (AU7), menu moves (AU11).

## AU6 Start float and ready paper

After this part the app opens on ready paper with a small Start float beside it, and every entry lands a working stroke.

### Start from

`welcome.rs`: `WelcomeAction` `:281-294`, `whats_new_due` `:313`, `open_at_startup` `:322`, `Welcome` `:400-418`, `ui` `:458`, tests `:751, 918-921, 1000, 1205-1221`. `main.rs`: `startup_shows_welcome` `:4321`, `enum Startup` `:4327`, `startup_route` `:4340`, `open_template` `:4385`, `welcome_show_me` `:4394`, startup match `:5906-5923`, `File ▸ Welcome screen` `:6079`, shortcut gate `:6100-6108`, `show_welcome` `:6433-6518`. `templates.rs:13-25, 36-110, 114, 154-204`. `newproject.rs:160-164, 289-300, 330-338`.

### Build

1. **Ready paper and float** per 8.1 and the `Start` row of 4.5. `Startup::Welcome` keeps its name and now creates ready paper plus the float, so the assertion at `welcome.rs:1209` holds. `Startup::NewForm` becomes `Startup::ReadyPaper`; the assertion at `:1212` moves once, deliberately, and still pins an exact route.
2. `Editor.start: Option<Welcome>`, drawn in `Editor::ui`. Its Modal and CentralPanel branches retire, so `self.welcome` stays `None`. `open_at_startup` still picks the tab, so an upgrader's float opens on What's new. `WhatsNew` Action (in today's Help menu until AU11) opens it. `welcome_panel()` tests call `welcome::ui` directly and stay.
3. **`apply_start`** per 8.1. `show_welcome`'s action handling moves behind it. `AppRequest::Start(WelcomeAction)` carries the choice, and `h.answer_start(action)` drives the seam in tests.
4. Close control, three dismissals, right-float rule, two recent rows, `More sizes` inside the scroll body.
5. `templates::INTENTS` outside `ALL`:

| Const | Size | fps | `cut_frames` | Tracks | Room |
|---|---|---:|---:|---|---|
| `ANIMATION` | 1280×720 | 12 | 1 | `[A]` | Drawing |
| `PICTURE` | 1920×1080 | 12 | 1 | `[A]` | Drawing |
| `PICTURE_SQUARE` | 2048×2048 | 12 | 1 | `[A]` | Drawing |
| `PICTURE_TALL` | 1080×1920 | 12 | 1 | `[A]` | Drawing |

6. TV anime HD and Feature UHD `room` become Drawing with the sheet on. Animatic lands in Timing with the canvas tab front and the brush armed. Update the `templates.rs` module doc.
7. **Drop to open:** `raw.dropped_files` through the Leave guard.
8. **Check your pen:** draw one line, read pressure, report `Pressure works` or a plain fix naming Windows Ink. Relies on the `seen_pen` fix from AU4.
9. **Lock.** `lock_positions` true at the fresh-install stamp beside `whats_new_seen` (`config.rs:2815-2817`), `UiConfig::default()` unchanged. `ToggleLockPositions` Action, in today's View menu until AU11, and a Settings row, to undo it. `frozen()` keeps it false. `is_closeable` per 4.6, so a locked install can still close what it opened.

### Tests (`austart_`, at least 14)

`austart_fresh_launch_opens_on_ready_paper`, `austart_first_stroke_closes_the_float_and_keeps_the_stroke` (vector route), `austart_raster_stroke_closes_the_float` (CPU route), `austart_stroke_at_the_paper_centre_with_the_float_open`, `austart_largest_float_leaves_the_paper_centre_clear` (`More sizes` open, What's new tab, two recents), `austart_float_blocks_only_its_own_rect`, `austart_esc_closes_the_float`, `austart_close_control_closes_the_float`, `austart_opening_layers_closes_start`, `austart_shortcuts_stay_live_under_the_float`, `austart_show_at_startup_off_keeps_the_paper`, `austart_untouched_blank_never_asks_to_save`, `austart_every_entry_lands_a_stroke` (through `apply_start`, stroke on the result), `austart_open_recent_loads_a_temp_file`, `austart_intents_are_the_table`, `austart_float_budget_at_worst_case`, `austart_first_screen_budget`, `austart_fresh_install_locks_positions`, `austart_locked_keeps_close_on_opened_panels`, `austart_drop_to_open_goes_through_leave`, `austart_check_your_pen_reports`.

### Gates

- First stroke 0 clicks from launch, at most 1 from any Start entry. All entries land a stroke (today 5 of 8).
- Start float ≤14 controls and ≤30 words at its worst case. First screen under `as_first_run()` ≤44 controls and ≤70 words. Paper uncovered with Start at its largest ≥58.0% at 1600×900.
- Intent entries ≥2.
- `aurail_paper_share_holds_at_every_size_and_aspect` still passes under `as_first_run()` once the float closes.
- Suite green, shots written.

### Must not change

`ALL` and `SIZES`, studio presets at 24 fps, crash recovery, the Leave guard, `New…` and its form, the sample cut.

### Leave out

What's New text (AU17), hints, tours, overlays on the paper.

## AU7 Say line and Quick Start

After this part the app teaches itself, one sentence at a time, and stops when asked.

### Start from

`welcome.rs`: STEPS `:50-76`, `EXPORT_STEP` `:79`, `tick` `:82-84`. `main.rs`: `show_me` `:4939-4965`, quick-start tick `:3077`. `doc.rs::commit_raster` `:1135`. `canvas.rs`: empty-cell hint `:4874-4889`, `raster_new_cel` `:6786`. Empty-state sites: `canvas.rs:4874-4889`, `xsheet_panel.rs:607`, `palette_panel.rs:43`, `curves_panel.rs:510`, `main.rs:8388`, `viewer.rs:311`.

### Build

1. **Say line, full.** `Editor.say` per 8.2: priority refusal, status, hint. Hint table as data, not scattered calls. `ui.hints_seen`, `ui.hints_off`. `ToggleHints` Action, in today's Help menu until AU11, and a Settings row. `frozen()` sets `hints_off`.
2. **Quick Start** per 8.3. Five dots in the Say line. Slots tick on outcomes, read only from `strokes_done`, `AddDrawingAfter`, playback and a finished file. Slot 4 still ticks at `queue_export` until AU9 moves it.
3. **Show me** 4 of 5 in this part, with `doc.rs::ring_tiles` per 8.3. Slot 4 waits for AU9.
4. **Empty states** in this batch: Timing sheet, canvas empty cell, Result, Motion curves, Plan preview, Recent, Timing under 2 drawings, Color no art, Effects no art, Check paint. The canvas line drops `!self.raster` per 8.4.
5. **False copy, 4 to 0.** `welcome.rs:542` (the theme tooltip) goes. `newproject.rs:163` ("fixed once the cut is created") goes. `main.rs:8388` and `viewer.rs:311` (Output-node hint) become the Result empty state. `palette_panel.rs:43` becomes a true sentence naming a control on screen; AU13 replaces it.
6. **Stale teaching, 2 to 0.** `new project — draw on the canvas to create frame 1` no longer sticks after the step is done, and a teaching sentence no longer fades before it is answered. The Say line holds both.

### Tests (`ausay_`, at least 12)

`ausay_priority`, `ausay_hints_never_point_backward`, `ausay_a_hint_shows_once`, `ausay_hints_off_changes_no_chrome`, `ausay_hints_on_starts_again`, `ausay_hint_with_no_binding_is_not_shown`, `ausay_quick_start_ticks_on_outcomes`, `ausay_vector_and_raster_strokes_tick_alike`, `ausay_show_me_draws_one_undoable_circle`, `ausay_show_me_is_four_of_five`, `ausay_empty_cell_line_paints_on_a_raster_canvas` (`canvas.raster = true`, CPU), `ausay_empty_cell_line_waits_for_a_first_drawing`, `ausay_no_step_is_silent`, `ausay_false_copy_is_gone`, `ausay_teaching_sentence_stays_until_answered`, `ausay_every_hint_fits_at_the_smallest_sizes`.

### Gates

- Silent rooms 0, false copy 0, stale teaching 0, Quick Start visible.
- `paper_under_chrome` is still 0 with a hint showing, and with the canvas line showing.
- Every hint and refusal at 832×520 and 853×533: `galley.elided` false, action button inside the band.
- Suite green, shots written.

### Must not change

`quick_start: [bool; 5]`, `EXPORT_STEP = 4`, the refusal words of existing refusals.

### Leave out

Skill meters, tours, any hint on a timer.

## AU8 GIF and still engine

After this part the engine can write a real GIF and one named PNG with no add-on. No new button yet.

### Start from

`crates/anim-core/src/deliver.rs`: `write_frame` `:584-585`, `Format` `:903-922`, `preset` about `:990`, `PRESETS` `:1000`, `video_name`, `sequence_pattern`. `crates/anim-core/tests/delivery_tests.rs:606-632`. `render_queue.rs`: `NEEDS_FFMPEG` `:32`, job preset index `:56-57, 78-80`, `render_job` `:338-372`, `render_video` `:377`, `ExportKind` `:791`, `export_job` `:812-838`, `start` `:841`, `poll` `:925`, panel `:1008-1037`, labels `:1300-1305`, job-running tests `:1509, 1641, 1904`. `doc.rs::edl_ext` `:4138-4143`. `main.rs`: `queue_export` `:5205-5225`.

### Build

1. New `crates/anim-core/src/gif.rs` per 5.5, plus `weezl = "0.1"` in `crates/anim-core/Cargo.toml`. Writer shape: `new(path, w, h)`, `push_frame(rgba, delay_cs)`, `finish()`. The palette is built from at most 16 evenly spaced frames, so a long shot does not render twice.
2. `Format::Gif` with `ext` `gif`. `is_video()` is true (meaning one file), so delivery counts do not move. New `needs_ffmpeg()`, false for GIF. `render_queue.rs:1013, 1019, 1037` use `needs_ffmpeg`. `doc.rs:4140 edl_ext` must not pick gif. `write_frame` arm returns `Err("a GIF frame goes to the GIF writer")`.
3. `render_job` gets a branch `render_gif(spec, cancel, tick)` writing `spec.out.join(deliver::video_name(&spec.pattern, &spec.slate.names, "gif"))`.
4. Append to `PRESETS`, at the end: `GIF`, `Still PNG`. New field `Preset.one_file: bool`, true only for `Still PNG`, default false in the `preset` builder. With `one_file`, `render_job` writes the exact name via `video_name(.., "png")`, not `sequence_pattern`. Png8, no alpha, no burn-ins. In `delivery_tests.rs` two pins move once, deliberately, for the one added non-video preset: the name list at `:607-622` gains both names, and the count at `:632` goes from 5 to 6 (GIF has `is_video` true, so only `Still PNG` counts).
5. `ExportKind::{Gif, Still}`, path logic like Mp4. `Still` exports the current frame. `Editor::export_to(kind, path)` is the seam behind the dialog: it calls `queue_export` with the chosen path. `h.run_export_to_end()` drives `render_queue::poll` in the Rig until the job finishes, because only the App polls today (`main.rs:2658`).

### Tests (`augif_`, at least 9)

In `crates/anim-core/tests/gif_tests.rs`, with a small test decoder over `weezl`: `augif_header_and_loop_block`, `augif_delays_follow_holds`, `augif_merges_identical_neighbours`, `augif_flat_colours_decode_exactly`, `augif_palette_stays_under_257`, `augif_sub_blocks_stay_under_256`, `augif_presets_are_append_only`.

In the app: `augif_job_finishes_in_a_temp_folder`, `augif_still_job_writes_one_named_file`, `augif_export_to_runs_without_a_dialog`.

### Gates

- Every GIF decodes with the test decoder: frame count, delays and first and last frame exact.
- `render_queue_sits_in_the_composite_and_edit_rooms_and_greys_video_without_ffmpeg` passes unedited.
- Suite green, shots written. No test opens a dialog or a file.

### Must not change

Existing preset order and indices, `render_video`, image sequence naming, `{cut}`, `export_job`'s existing kinds, a blank graph exporting, export starting at once.

### Leave out

Bundling ffmpeg, Media Foundation, dither, per-frame palettes, a GIF options dialog, transparency in GIF, any new control.

## AU9 Export step

After this part a beginner can finish: a real GIF or PNG comes out with no add-on, and J1 and J2 run end to end.

### Start from

`main.rs`: `AppRequest` `:3028-3056`, `queue_export` `:5205-5225`, `Editor::arm_room`, `PROBE_ROOMS` `:6640`, `show_me` `:4956`. `render_queue.rs` panel `:1008-1037`. `workspace.rs::Stage`. `testkit.rs` request log.

### Build

1. `Stage::Export`, `RoomExport` (F9, a new Action so `fill_missing` delivers it), `arming_key`, `PROBE_ROOMS` to 9, shot chain. Export pill now runs `RoomExport`. Other new Actions per 6.3 and the `File ▸ Export ▸` rows in today's File menu. `AppRequest::ExportTo(kind)`, `AppRequest::OpenFile(path)`, `AppRequest::ShowInFolder(path)`. `h.answer_export(temp_path)` takes a logged `ExportTo` and calls `Editor::export_to`, counting as the one dialog action.
2. New `export_step.rs`, drawn at the top of the `Pane::RenderQueue` arm, per 5.5:

| Document | Choices |
|---|---|
| Animation, `frame_count > 1` | Animated GIF (primary), Video (MP4), Picture sequence (PNG), This frame (PNG) |
| One frame | PNG image (primary), Other formats |
| Comic | Export pages (primary), Other formats |
| No art | `Nothing to export yet. Draw something first.` plus `Go to Draw` |

3. Done card per 5.5. Size and duration come from the finished file and the job, never estimated. The back button names the step the person came from.
4. **Exports started elsewhere.** A `File ▸ Export ▸` row outside the Export step stays where it is and opens no pane. On finish the Say line says `Exported ball.gif.` with `Open file`, per 8.2.
5. Quick Start slot 4 moves from `queue_export` to a job finishing with its file on disk, on either route. Any test pinning the old timing moves to run the job to the end.
6. Show me slot 4 enters Export with Animated GIF ringed. Show me reaches 5 of 5.
7. J1 and J2 run through the `Recorder` under `as_first_run()`, ending in a real file through `h.answer_export`.

### Tests (`auexport_`, at least 12)

`auexport_mp4_without_ffmpeg_says_why_and_offers_gif`, `auexport_step_changes_no_tool_layer_or_view` (under `as_shipped()`, with a non-default tool, layer and view seeded first, so it cannot pass by default), `auexport_step_is_refused_mid_stroke`, `auexport_arming_key_skips_export`, `auexport_choices_follow_the_document`, `auexport_done_card_tells_the_truth`, `auexport_file_menu_export_reports_in_the_say_line`, `auexport_open_and_show_are_requests`, `auexport_no_art_says_so`, `auexport_ball_journey` (decodes the GIF: first and last frame differ), `auexport_still_journey`, `auexport_quick_start_ends_in_a_file`, `auexport_show_me_is_five_of_five`.

### Gates

- J1 ≤28 actions, ≤5 overhead, ≤3 concepts, ≤2 jargon, and a real 12-frame GIF of 1.0 s at 12 fps in a temp folder.
- J2 ≤4 overhead and exactly 1 named PNG, ≤2 concepts, ≤1 jargon.
- No refused or ignored step in either journey.
- Quick Start ends in a file. Show me 5 of 5.
- `Stage::ALL` still lists 5. No code path writes `export` to config.
- Suite green, shots written. No test opens a dialog or a file.

### Must not change

The queue panel's own controls under `More options`, `export_job`'s existing kinds, the routes into the queue from the old menu rows.

### Leave out

Everything in AU8's list. Any change to the queue itself.

## AU10 Keys and Hide tools

After this part arrows work, every step has a key, and Hide tools clears the screen. A working animator's chords are unchanged except the two named moves.

### Start from

`config.rs`: `context` `:1219`, `pane_only` `:1250-1283`, `default_chord` `:1294-1317`, `default_also` `:1320-1332`, `gaban_chord` `:1334-1596`, `clashes` `:1613`, `Config::load` `:2775-2784`, `load_from` `:2793-2822`, `fill_missing` `:2871-2893`, `reset_keybinds` `:2907`, `set_keymap` `:2913`, `chord_for` `:2936`, `conflict` `:2945`. `shortcuts.rs`: `fired` `:67-89`, `aimed_here` `:93-112`, `fires_over_field` `:115-124`.

### Build

1. **Defaults** per 6.3 and 6.4: F4 to F8, T, Alt+O, Alt+W in `gaban_chord`, arrows in `default_also`. F9 arrived with AU9. `ColumnKindBook` and `ColumnKindBg` join `pane_only`.
2. **`fired`** gains a `focused: bool` input, fed by `ctx.memory(|m| m.focused()).is_some()`, and skips arrow chords when it is true. No `Cursor*` dispatch.
3. **Migration** in `Config`, called right after `fill_missing` in `load_from`, and covering the no-data-dir path of `Config::load`:

```rust
pub const KEYMAP_REV: u32 = 1;

/// Runs once on a config older than KEYMAP_REV. Returns what moved.
fn migrate_keymap(&mut self) -> Vec<String> {
    if self.ui.keymap_rev >= KEYMAP_REV { return Vec::new(); }
    let mut moved = Vec::new();
    // Only if the stored chord still equals the old default and the new one is free.
    self.rehome(Action::ColumnKindBook, alt_k(), alt_o(), &mut moved);
    self.rehome(Action::ColumnKindBg, alt_b(), alt_w(), &mut moved);
    // Only if chord and also are both empty and the key is free.
    for (a, k) in [(Action::RoomLayout, F4), (Action::RoomDrawing, F5), (Action::RoomEdit, F6),
                   (Action::RoomFinishing, F7), (Action::RoomComposite, F8)] {
        self.give_if_empty(a, k);
    }
    // Added beside the existing chord, only where the key is free.
    self.add_also_if_free(Action::PrevFrame, &[ArrowLeft, ArrowUp]);
    self.add_also_if_free(Action::NextFrame, &[ArrowRight, ArrowDown]);
    self.ui.hints_seen = u32::MAX;
    self.ui.keymap_rev = KEYMAP_REV;
    moved
}
```

   `UiConfig::default().keymap_rev` stays 0, and a missing field reads 0. The two fresh paths stamp `KEYMAP_REV` explicitly, in the same place and pattern as `whats_new_seen` at `config.rs:2815-2817`, so a fresh install migrates nothing. `moved` goes out through the existing config notes. A rehome that loses (the new chord is taken) leaves the action where it was and adds one note.

4. **Hide tools,** `Editor.tools_hidden`, never persisted: T or a row in today's View menu hides the top row, band, rail and floats in any step. The pane grows and paper refits once. Refused mid-stroke. One 32 pt return dot, an `Area` at (8, 8), with a non-interactable card naming the live binding the first time. Playback continues, because `play_tick` runs before panels. Dock still draws, so nothing is saved undrawn.
5. **J5 script** over the `Recorder`.

### Tests (`aukeys_`, at least 14)

`aukeys_stock_gaban_config_migrates`, `aukeys_stock_industry_keeps_comma_and_period`, `aukeys_users_alt_k_is_left_alone`, `aukeys_users_alt_o_keeps_book_with_one_note`, `aukeys_users_f5_is_kept`, `aukeys_second_load_changes_nothing`, `aukeys_fresh_install_stamps_and_moves_nothing`, `aukeys_reset_and_set_keymap_agree`, `aukeys_f4_to_f8_arrive_on_an_old_config`, `aukeys_render_survives_migration`, `aukeys_arrows_step_frames`, `aukeys_arrows_leave_focused_widgets_alone`, `aukeys_six_steps_have_keys`, `aukeys_no_chord_depends_on_the_pointer` (walks both keymaps, with the 6.4 exceptions as its one allowed list), `aukeys_hide_tools_hides_and_returns`, `aukeys_hide_tools_keeps_playback_running`, `aukeys_hide_tools_refused_mid_stroke`, `aukeys_keyboard_journey`.

Golden configs are hand-built v0.5.0 shapes with no `api_key` and no `totp_secret`.

### Gates

- J5: arrows 4 of 4, steps 6 of 6, pointer-dependent chords 0 per 6.4, example edit 5 presses 0 clicks.
- 10 golden config cases pass.
- `ship_every_action_is_described_placed_and_clash_free` and `viewnav_new_verbs_are_actions_with_free_chords` pass unedited.
- 118 of 120 chords unchanged, checked against a frozen list.
- Suite green, shots written.

### Must not change

`Action` variants, the Industry keymap's seven overrides, one registry, `not now: why`, the palette's behaviour, the F1 sheet, Chord format on disk.

### Leave out

New chords beyond 6.3, a chord editor redesign, any change to the palette ranking, Radial, menu moves.

## AU11 Menus

After this part there are six plain menus, with every command in one known place.

### Start from

`config.rs`: `Action::menu` `:859-1019`, aliases `:1022`. `menus.rs`: `menu_item` `:134-173`, `menu_items` `:178-202`, `plain_menus` `:224-228`, `project_menu` `:232-239`, `help_menu` `:241`. `cmdpal.rs:241, 627, 638`. `main.rs::shortcut_ctx` `:5168-5182`. Hand-built rows per 6.2.

### Build

1. `Action::menu()` returns the path of 6.2 for every Action. `menus::ORDER` per 6.2, and `menu_items` walks it. `MENUS`, `plain_menus`, `project_menu` follow.
2. `EditCut`, `EditCopy`, `EditPaste` with `Editor.last_ctx`, per 6.2. `BrushMakerRoom` replaces the hand row at `main.rs:7869`.
3. Move every hand-built row to its home in 6.2. `More ▸ Steps ▸` holds the six step Actions. Help gets its eight rows, `View ▸ Lock UI positions` and `View ▸ Hide tools` take their places. The canvas window label at `main.rs:4620-4629` becomes `Canvas is open in its own window. More ▸ Panels ▸ brings it back.`
4. Narrow threshold falls to 1040, and 1100 with the off-spine pill.
5. Move the tests that pin a path, each with reason `label`, each keeping its strength: `canvas.rs:9583`, `cmdpal.rs:638-641`, `curves_panel.rs:1095, 1105`, `doc.rs:6355`, `manga_tools.rs:1028, 1337`, `nodegraph_panel.rs:1713`, `paintcheck.rs:699`, `playback.rs:1640-1651, 1661`, `render_queue.rs:1820, 1874`, `board_panel.rs:1102`. Menu-case tests (19) and the last 2 "room" tests move by `vocab`. Report how many of these 14 sites sit inside the 19.

### Tests (`aumenu_`, at least 8)

`aumenu_six_menus_within_budget`, `aumenu_every_action_has_its_table_path`, `aumenu_order_is_the_table`, `aumenu_no_duplicate_rows_or_spoken_names`, `aumenu_every_action_is_in_the_palette` (computed from `Action::ALL`), `aumenu_every_hand_row_has_a_home`, `aumenu_edit_cut_copy_paste_follow_the_last_pane`, `aumenu_edit_cut_says_why_on_the_canvas`, `aumenu_edit_rows_show_the_chord_they_perform`, `aumenu_blank_draw_budget`.

### Gates

- Controls ≤30, after a stroke ≤36, words ≤40, in Draw under `as_shipped()`.
- Menus ≤6, first-level rows ≤60.
- `ship_every_action_is_described_placed_and_clash_free` passes unedited.
- Palette reach equals `Action::ALL.len()`.
- Clip audit `Scope::Chrome` passes with menus open.
- Suite green, shots written.

### Must not change

`Action` variants, chords, what any Action does, one registry, `not now: why`, Ctrl+C and Ctrl+V by pane.

### Leave out

Context twins (AU15), plain words beyond the menu rows (AU12).

## AU12 Plain words

After this part every label is plain, every old word still works in Search, and type is at reading size.

### Start from

`vocab.rs`. `config.rs`: `Action::label` `:489`, `help` `:652`, alias table `:1026-1186`, stacks `:2356-2367`, preset names `:2896-2899`, Settings rows. `crates/anim-core/src/fx.rs:746-790`. `fxui.rs`. `measure.rs` TERMS. Every panel's literal strings. Size sweep: the 132 hard-coded lines and every direct `FontId::monospace`.

### Build

1. Move the 17 trade-term tests to `vocab::X` with values unchanged. Flip the table to section 3 in one commit. The pin test now asserts the final table.
2. Display maps per section 3: stacks, presets, FX, column header words, status words. Stored strings stay.
3. Help sentences (`Action::help`, FX `help`) lose romaji, in plain sentences of at most 120 characters. Their old words go to `aliases` or the hover card's second line.
4. Hover card second line: `Studio word: <word>` for steps and for any label whose table row has a studio term.
5. `T.U.` and `T.B.` (`camera.rs:209` label) show `Zoom in` and `Zoom out`. The stored key `zoom` stays.
6. `TERMS` extended by `composite`, `layout`, `queue`. Never shrunk.
7. The 132 hard-coded sizes and direct `monospace` calls move to the roles in 2.5. A ratchet stops new ones. `genui_glyph_coverage` stays.
8. Remove stale sentences this makes false.

### Tests (`auwords_`, at least 10)

`auwords_vocab_table_is_final`, `auwords_every_old_word_is_an_alias`, `auwords_palette_finds_the_six_studio_words`, `auwords_no_japanese_in_any_step`, `auwords_jargon_budget_blank_draw`, `auwords_tooltip_jargon_budget`, `auwords_painted_text_size`, `auwords_stored_names_are_unchanged`, `auwords_display_maps_are_complete`, `auwords_hover_card_gives_the_studio_word`, `auwords_no_bare_font_sizes`, `auwords_action_help_stays_one_sentence`.

### Gates

- Jargon ≤3 in blank Draw, Japanese words 0 on screen.
- Tooltip jargon kinds ≤3 (today 24).
- Painted text median ≥10 px, ≤10% at ≤8 px. If this misses, `Body` goes to 14.
- Palette finds genga, douga, shiage, satsuei, henshū, shaku.
- The 15 trade-word tests pass unedited.
- Clip audit `Scope::Chrome` passes at all sizes and scales: longer plain words must fit.
- Suite green, shots written.

### Must not change

Every stored id in 1.4. User-typed names. Default track names (`A`, `BG`). `{cut}`. Timing sheet density.

### Leave out

Translation, a glossary window, rewriting help beyond removing jargon, panel layout.

## AU13 Color step and strip pictures

After this part filling a drawing takes three taps, and the strip shows pictures.

### Start from

`workspace.rs` InkPaint recipe `:204-278`, `view` `:281-312`. `palette_panel.rs`: empty state `:43`, ScrollArea `:60-62`, pots `:106-115`, wheels `:116-125`, edit latch `:191-199`, name and `+ character` `:201-215`. `canvas.rs`: `SWATCHES`, `dish_ui` `:2306`, fill path `:1474, 3629, 6600-6706`, `fill_rig` `:9688-9716`. `main.rs:4761`, `cel_layers_strip`. `strip.rs`. `doc.rs::drawing_composite` `:2267-2294`, `display_drawing` `:2312-2320`. `board_panel.rs::refresh_thumbs` `:167`.

### Build

1. `Stage::InkPaint.dock()`: `Canvas`, then `split_right(canvas, 0.78, [Palette, PaintCheck, Viewer(0)])`. `dock_with_result()` holds today's recipe minus the Brush band. `ui.result_in_color` with `Editor.result_in_color`, `recipe` and `apply_ui`, per 5.3 and 5.4. `frozen()` sets it true. A one-click `Result beside` toggle in the Colors panel header, remembered.
2. Ten starter swatches at the top of Colors, as `plate` constants: `#000000`, `#FFFFFF`, `#D93A2B`, `#F2994A`, `#F2C94C`, `#3FA34D`, `#2F7FD8`, `#8A5CC7`, `#F2C9A1`, `#8B5E3C`. A click sets `brush_color` with no document change. `.animproj` stays untouched.
3. `Add color`: a colour pick popover that stays open across picks. The first use creates a default `Colors` character silently through the existing path at `palette_panel.rs:201-215`. Later uses add roles.
4. `Edit colors` sits outside the scroll, so it can never fall below the fold (today it is 12 px below at all widths). Character groups and color names are disclosed. Colour wheels stay in edit mode. `plate::danger` holds stay.
5. `palette_panel.rs:43` becomes `No colors yet.` plus `Add color`.
6. Held frames in Layers show a read-only row with `Go to this drawing`, and the Say line `This drawing stays on screen for N frames.`
7. Hint `Pick a color, then tap an area.`
8. **Strip pictures.** Sheets show a thumbnail through `drawing_composite`, cached by `(DrawingId, content_hash)`, visible sheets only, one job at a time, never while `gesture_safe` is false. A number stays when a sheet is too narrow for a picture.
9. **J3 script** over the `Recorder`, per section 9. The tap on a region is a real fill, so the journey is an ignored `with_gpu` test under `as_shipped()`. The ordinary suite proves the same landing on the CPU route.

### Tests (`aucolor_`, at least 12)

`aucolor_recipe_is_canvas_and_right_tabs`, `aucolor_result_beside_restores_todays_split`, `aucolor_result_beside_is_remembered`, `aucolor_result_choice_survives_a_restart` (same shape as the sheet's restart test), `aucolor_protect_line_art_is_still_forced`, `aucolor_swatch_changes_no_document`, `aucolor_add_color_makes_the_character_silently`, `aucolor_popover_stays_open_across_picks`, `aucolor_edit_colors_is_never_below_the_fold`, `aucolor_held_frame_row_is_read_only`, `aucolor_go_to_this_drawing`, `aucolor_thumbs_key_on_content`, `aucolor_thumbs_wait_for_a_gesture`, `aucolor_cpu_fill_lands_on_the_color_layer` (the `fill_rig` pattern with `canvas.raster = true`, on a state armed by `arm_room`: pixels change on the color layer, the line layer's tiles do not), `aucolor_fill_journey` (ignored, `with_gpu`: asserts pixels changed on the expected layer), `aucolor_no_journey_control_behind_hover_or_scroll`.

### Gates

- J3 in the ignored pass: first fill ≤3, three own colours ≤9, ≤2 concepts, ≤1 jargon, no refused step, pixels changed on the color layer.
- `aucolor_cpu_fill_lands_on_the_color_layer` in the ordinary suite.
- Journey controls behind hover or scroll 0 (today 3).
- `render_queue_` and fill tests pass.
- Thumbnail work adds 0 frames of stall to a stroke: no job while `gesture_safe` is false.
- Paper and pen shares in Draw unchanged.
- Suite green, shots written.

### Must not change

Fill semantics, `line_guard`, palette undo behaviour (bypasses undo today, stays), character data shape, `Palette` as stored in `app.palettes`, sheet row density.

### Leave out

Palette undo, a new colour picker widget, thumbnails in the Timing sheet, drag reorder.

## AU14 Effects step

After this part a glow is one tap and a camera move is two framings. No wires.

### Start from

`doc.rs`: `wire_under_composite` `:4446-4502`, `set_cut_camera_from_view` `:4504-4522`, `set_param_keys` `:3028`, `add_param_column` `:3464`, `set_param_key` `:3483`, node add path `:4526-4537`. `crates/anim-core/src/camera.rs`: specs `:206-217`, feeding camera `:456`. `crates/anim-core/src/command.rs:1159`. `crates/anim-core/src/deliver.rs:106-143`. `nodegraph_panel.rs`: add menu `:228-294`, `Build` `:830-850`. `fxui.rs:433-470`. `viewer.rs`.

### Build

1. **`wire_over_output`** in `doc.rs`, modelled on `wire_under_composite`, with one difference: the new node goes **in series**. The old feed goes to the new node's input 0 and the new node feeds the Output, in one `engine.apply`.

| Graph | Result |
|---|---|
| Output with a feed | Feed → new node → Output |
| Bare Output | New node → Output |
| No graph | Build the auto composite first, then as the first row |
| Hand-built, no Output | Returns false. Shelf disabled with `Add an Output node to use effects here.` |

2. **Shelf.** A 40-tall header inside the graph pane in Effects: Glow, Soft focus, Blur, Color look, Camera move, as filled controls with plain words. The most recently added or selected effect shows its one `Strength` slider at the right end of the shelf, bound to one parameter per kind (for Glow, `intensity`; take the others from the kind's specs in `fx.rs`). Other parameters stay in the inspector.
3. `Add effect` keeps Tab's menu, now with display names. `Full finishing setup` (`BuildSatsuei`, 31 nodes) goes under `More` with a plain warning. `Set up standard composite` shows on an empty graph.
4. **Camera move.** Find the camera feeding the output (`camera.rs:456`) or insert one in series with the first plane pin. In one undo: add param columns for pan x, pan y and zoom, bind them, and key them at the current frame with current values. This is the same batch shape as `set_cut_camera_from_view`, which is Model3D-only and is not reused. A frame box then shows on the Result.
5. **Frame box.** In `viewer.rs`, only while armed: drag to pan, wheel to zoom. Each framing writes keys at the current frame through `set_param_keys`, one undo each. Go to the last frame with End or the strip. The box stays until another tool or a second tap on Camera move.
6. FX display map in shelf, add menu and inspector. Hint `Pick an effect to add it to the picture.`
7. **J4 script** over the `Recorder`.

### Tests (`aufx_`, at least 12)

`aufx_glow_changes_the_output_at_once` (reads `deliver::render`, the CPU route `render_job` uses, before and after), `aufx_insert_on_output_with_a_feed`, `aufx_insert_on_a_bare_output`, `aufx_insert_builds_the_composite_from_no_graph`, `aufx_no_output_disables_with_the_reason`, `aufx_insert_is_one_undo`, `aufx_strength_writes_one_parameter`, `aufx_camera_move_is_one_undo`, `aufx_frame_box_keys_start_and_end`, `aufx_camera_move_reuses_the_feeding_camera`, `aufx_camera_keeps_planes_keys_and_binds`, `aufx_display_map_leaves_stored_names`, `aufx_glow_and_pan_journey`.

### Gates

- J4: ≤8 actions, 0 wires, 0 room switches, ≤3 concepts, ≤2 jargon, no refused step.
- The graph hand-wiring path is unchanged: every `nodegraph_panel` test passes unedited.
- Output nodes after Glow on Web 720p: the auto graph's 6 plus 1, and no surprise nodes.
- `wire_under_composite` and its callers unchanged.
- Suite green, shots written.

### Must not change

Graph data, `FxKind` names, `NodeKind`, Camera parameters and their keys, the graph compositor, curves, `Tab` for `AddNode`, `BuildSatsuei` output, 3D's `set_cut_camera_from_view`.

### Leave out

A new effect, an applied-effects list panel, curve editing in the shelf, easing presets, parallax controls.

## AU15 Panels

After this part every panel speaks the one language and fits its smaller minimum.

### Start from

Each panel in 7.1: `xsheet_panel.rs`, `canvas.rs` layers strip, `forge.rs`, `palette_panel.rs`, `nodegraph_panel.rs`, `viewer.rs`, `curves_panel.rs`, `paint_check` pane, `render_queue.rs`, `board_panel.rs`, `view3d.rs`, manga panel.

### Build

1. Panels per 7.1: minimum sizes, sections, explicit scroll policy, `More` with `ui.panel_more`. `Editor.more_default` stays for the frozen boot.
2. Timing sheet header folds to one line plus `Details`. Chrome falls from 178 to about 90 pt.
3. Tab bars: pill tabs, current in wash, per 2.8.
4. Seven context twins per 6.2, each with an `Action` and a menu or palette path.
5. Remaining empty states: Colors already done, Export queue, Manga pages, Shots.
6. Most-used panels first, rarest last, because folded sections on the rarest panels are in the cut order.
7. Crowded-room budgets: see gates.

### Tests (`audress_`, at least 8)

`audress_panel_minimums_are_the_table`, `audress_every_panel_has_a_scroll_policy`, `audress_more_is_remembered_per_panel`, `audress_context_twins_exist`, `audress_tab_bar_uses_the_look`, `audress_crowded_rooms_hold_the_budget`, `audress_sample_holds_the_budget`, `audress_empty_states_are_complete`, `audress_all_panels_clip_audit_passes`, `audress_shapes_and_vertices_hold`.

### Gates

- Clip audit `Scope::All`: 0 clipped, 0 elided, over `PROBE_ROOMS`, 4 sizes × 3 scales, now that every panel has its new minimum and scroll policy.
- Crowded blank room ≤45, sample cut ≤80. Budgets: Plan about 39, Color about 44, Effects about 40, Timing about 45, Manga about 41.
- Context twins 7 of 7. Palette reach equals `Action::ALL.len()`.
- Shapes per room not above 298–615, vertices not above 5,154–9,124, tessellation ≤407 µs.
- Suite green, shots written.

### Must not change

Panel internals: graph, curves, queue maths, board, episode, 3D and manga logic. Every stored id.

### Leave out

Rebuilding any advanced panel, new features.

## AU16 Alias retirement

After this part the old token names are gone. Nothing on screen changes, and that is the gate.

### Start from

`plate.rs` aliases and every reference across the 23 files.

### Build

1. Every `GRAPHITE`, `LEGEND`, `STRUCK`, `TALLY`, `AO`, `AKA` reference moves to its token, then the aliases go. The review rows of 2.4 already point at final tokens. `PLATE_GRAIN_PNG` stops being compiled in. Leave the asset file on disk.

### Tests (`autoken_`, at least 3)

`autoken_old_names_are_gone` (over `sources()`), `autoken_no_colour_literals_came_back`, `autoken_render_is_unchanged_by_the_removal` (ignored: pixel diff 0 against the part's start, all rooms at 1600×900).

### Gates

- `COLOUR_LITERALS_PINNED = 0` with aliases gone.
- Pixel diff 0 in the ignored pass.
- Text audit and contrast pairs still pass.
- Suite green, shots written.

### Must not change

Any painted colour, any stored value, `CHIP_*`, `PENCIL_AO`, `PENCIL_AKA`.

### Leave out

Any change of colour value or layout.

## AU17 Release

After this part the release is ready for the Lead.

### Start from

`welcome.rs` What's New. `README`. Version sites from `ship_version_is_0_5_0_everywhere` (`testkit.rs:1424`).

### Build

1. What's New: one screen, old word to new word, and moved chords listed by live name through `chord_for`. An upgrader sees it once. Say that the Timing sheet comes back with one click.
2. README: new look, steps, GIF export, keymap notes. `ship_readme_covers_the_release` moves with its reason.
3. Version 0.6.0 in the app, `update::CURRENT_VERSION`, `welcome::VERSION` and the installer. `net::MIN_PEER_VERSION` stays 0.5.0. Rename the test to `ship_version_is_0_6_0_everywhere`.
4. Rewrite the stale code comments listed in 1.3.
5. Update `research/DESIGN-SPEC-editor-ui.md` and `ROOM-CHARTERS.md` with one line at the top pointing to this document.
6. Final tally: run every gate in section 10 once, ordinary and ignored, and write the numbers into the report next to today's. Hand over the batched list in section 13.

### Tests (`aurel_`, at least 4)

`aurel_whats_new_names_moved_chords_live`, `aurel_upgrader_sees_whats_new_once`, `aurel_upgrader_keeps_their_config`, `aurel_no_stale_room_law_in_comments`.

### Gates

- `.animproj` schema 11, sample byte-stable. `ship_older_configs_still_load` passes. Old workspaces still load.
- Every gate in section 10 re-run and reported, with any miss named.
- Suite green. Final shots written at both sizes.

### Must not change

Every stored id. `MIN_PEER_VERSION`.

### Leave out

Bundling ffmpeg, Radial, step memory. Do not push and do not publish.

---

## Rejected review notes

Every blocker and serious issue from the attack was fixed in the spec. Where an issue offered a choice, these are the options not taken, and why. No issue was rejected outright.

| Issue | Option not taken | Why |
|---|---|---|
| Stroke signal | Bumping inside `commit_raster` with no split | `paste_pixels` (`doc.rs:2253`) also calls it, so a paste would close Start and tick Quick Start. `land_raster` keeps the signal honest |
| J3 on a Rig that refuses fills | A CPU fill seam for the whole journey | Both were used: the journey stays an ignored GPU test with real pixels, and the ordinary suite proves the landing with the CPU route that already exists (`fill_rig`). No new seam was needed |
| Disabled contrast | An explicit disabled ink for every control | `disabled_alpha` 0.65 is one number in one place and also dims icons and fills. `INK2` and `INK3` fail at 0.65, so the rule is `INK` inside a disabled scope |
| Reflow | Reserving the strip's height from the start | A one-frame `A drawing` would lose 56 pt of paper for a strip it may never use. One accepted refit, at open or on Add drawing and never during a stroke, costs less. R17 and the gate were narrowed to what is true |
| Strip Enter | Refusing with a route | Opening the sheet and starting to type is what the key means everywhere else, and it lives in the handler, so a rebound chord works |
| Strip Delete | Refusing with a route | The sheet's Delete removes a track, which is too strong for a strip that shows one track. Removing the drawing under the pointer is what a person means there, and it is one undo |
| Tip row | A small store for per-command counts | `hints_seen: u32` cannot hold it, and hover cards and menu rows already show each chord. The row is dropped |
| Spoken names for Add drawing | A separate strip `+` | The mock draws only the band button, so the `+` goes. One control is one less to count and to name |
| Done card | Sending every export to the Export step | A person exporting from File in Draw should stay where they are. The Say line tells the truth with the same words, and the file is the same |
| Fresh-install lock | Unlocking close buttons for all tabs | Recipe panes staying put is the point of the lock. Only panels a person opened by hand keep their close control |
| About ten parts | Keeping eleven parts and trimming scope | The Lead's list of what must not be lost leaves nothing large to trim. Seventeen smaller parts keep every promise, and the first visible change is one part later than it was |
| Mock band order | Transport above the strip, as drawn | The transport row would jump 56 pt when the strip opens. Strip above transport keeps every transport control still |
