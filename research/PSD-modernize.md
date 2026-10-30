# PSD — modernize: Gaban Studio 0.5, studio grade from layout to delivery

Room opened 2026-10-28. Plan `gaban-modern`
(`C:/Users/Lancer/Desktop/LsXavecs AGI/hub-tools/stage/plain/gaban-modern.json`), branch `modernize`,
local commits only, never pushed. Owner's order, verbatim:

> You are going to use the locally Gaban studio Application and Build it to be Modernized. It is a
> Traditional Animation studio. Fully Build it for studio grade work. Find what is needed based on past
> research sessions and build it polished and ready for a animator to utilize. Utilize any installed UI
> development skills and toolchains where needed. If any non-local development tools are needed it is ok to
> install them. Think like a Modern 3D artist and what they would want to have in the suite, as well as a
> modern Manga artist and A modern Animation artist. Not only does this drawing application specialize in
> the traditional methods but keeps it Contextualized in a way some one could pick it up and use it right
> off the bat. Gaban studio should include tools in thee Animation workflow not just tailored to the drawing
> process but to the finalized build and FX rendering similar to ufotable workflows and Mappa. Stop the
> build once it is fully built out. and Plan usage accordingly so it will have enough to finish.

**How to read this.** An implementer reads §3 (shared decisions) and its own `## GMn` section. Each section
stands alone: it names its files, the existing code it touches (with line numbers as of 56679ad), its
acceptance criteria and its tests. §1 is the definition of done, §2 is what exists, §4 is the budget, §5 is
what this room does not build.

**Calls taken as recommended** (made on the owner's behalf; the Lead may change any of them):
1. The release is **0.5.0**. The file schema goes from 9 to **11**, skipping 10, because the lost v0.4.0 dev
   build wrote v10 files (D2).
2. **Colour law.** New projects store chips exactly as picked, so screen and export bytes equal the chip.
   Old projects render byte-identical to v0.3 until the artist converts them, and conversion writes a backup
   first (D8).
3. **FX run on the CPU first** (anim-core, rayon). The GPU compositor shows FX subtrees by uploading the CPU
   result, so it never forks the maths (D9).
4. **Rooms.** The spine becomes Layout, Drawing, Ink & Paint, Composite and Edit (Finishing splits in two).
   Forge, Manga and 3D sit off the spine (D12).
5. **Defaults.** Template cuts run 72 frames (3 s at 24 fps). Autosave runs every 5 minutes into the app's
   data folder and keeps the newest 3. The command palette opens with Ctrl+K. There are two keymaps, Gaban
   (the default) and Industry (D13).
6. Lip flap (kuchipaku) uses the anime 3-mouth model (closed, half, open). It is not phoneme lip sync (D14).
7. Manga pages are 350 dpi, and webtoon strips are 800 px wide (D15).
8. Video delivery (H.264, H.265, ProRes, DNxHR) runs through an ffmpeg the user installs; the app never
   bundles one. This machine has ffmpeg 8.1.2 (full build, winget `Gyan.FFmpeg`, rechecked 2026-10-28). It is
   not on PATH, but the existing `find_ffmpeg` (app/src/export.rs 269: exe dir, then PATH, then the winget
   package dir) finds it, and that build has libx264, libx265, prores_ks, dnxhd, aac and pcm_s24le. Image
   sequences (PNG16, TIFF16, EXR) are built in and are the default delivery. Tests never spawn ffmpeg.
9. 3D uses a CPU toon renderer: glTF and OBJ in, with no GPU 3D path and no FBX/Alembic. Models are embedded
   in the project. Image sequences rendered by other apps are referenced on disk, not embedded (D15).
10. Outside tools: **none are installed**. The ffmpeg already on this machine is used twice outside the
    tests: by hand in the sound part, to make three small committed test clips, and by the ship gate's smoke
    render (GM19). If a part finds another tool essential, it says which and why in its result, and the
    shipped app still runs without it.

---

PREMORTEM. It is the end of November 2026. Gaban Studio 0.5 shipped and a small studio used it for one TV
cut. The story of how it failed:
- A quick WGSL glow was written "for the live view". The composite view and the delivered frames disagreed,
  the satsuei artist graded to the screen, and the delivery came back washed out.
- Insert-frames rippled the cel columns but not the camera param columns, so every pan after the insert was
  six frames late. Nobody saw it until the render.
- Autosave wrote into the artist's own file mid-stroke and froze the pen for two seconds on a 300-drawing
  cut.
- A v10 file from the lost dev build would not open, because 0.5 also called itself v10 with different
  tables.
- Typing a cut name "B12" switched to the brush and toggled onion.
- The test suite flashed a console window over the Lead's work on every run.
- A new hire opened the app to a grey canvas and no clue where to start. Welcome existed, but only from a
  menu.
- Then the budget ran out at part fourteen. The ship part never ran, and peers still on 0.3 joined sessions
  and choked on the new commands.

ROOT: **this stands on one document truth rendered by one CPU golden renderer that every view and export
agrees with, and on every edit being an undoable command saved in a format old files still open.** If either
is weak, this is rubble.

NEVER-DO.
1. **No control the renderer ignores.** Every new parameter changes the CPU golden render, the composite view
   and the export alike. If it does not, it is not shown.
2. **No forked render maths.** anim-core's renderer (export.rs and the new fx, camera, gen, r3d, look and
   manga modules) is the truth. The GPU compositor shows such subtrees by calling it (D9) and never by a
   second implementation.
3. **No edit without a Command.** Every document change goes through `Engine::apply` with an exact inverse,
   and one gesture is one undo step. There are exactly two exceptions, both file-level migrations: opening an
   old file, and converting the colour law. Each writes a backup first.
4. **Never break old files.** v1–v9 files, and the lost dev build's v10, open and re-save without losing
   data. New data lives in new side tables. Existing tables never gain columns.
5. **Never write the user's file except on an explicit Save.** Autosave, recovery, thumbnails and crash
   records write to the data folder; the only thing beside the file is its `.bak`. Tests write only inside
   temp dirs and never touch `%APPDATA%/AnimStudio`, `dev_autosave.animproj` or `dev_session.json`.
6. **Never block the pen.** Save, render, FX, decode and scans run off the UI thread, and nothing starts
   mid-stroke.
7. **Tests never open a window**, create a GPU device or an audio device, touch the network, or spawn a child
   process (ffmpeg, powershell, explorer, reg).
8. **Plate tokens only, and no green anywhere** (DESIGN-SPEC-editor-ui.md). The colour-literal ratchet only
   goes down.
9. **No single-key shortcut fires while a text field has focus.**
10. **Never push, publish, tag or run the installer.** Commits stay local on `modernize`.

BLAST RADIUS.
- anim-core changes: `model.rs`, `command.rs`, `lib.rs` (history), `xsheet.rs`, `graph.rs`, `eval.rs`,
  `export.rs` and `store.rs` (v11).
- anim-core gains new modules: `sheet_ops`, `color`, `fx`, `camera`, `text`, `deliver`, `audio`, `look`,
  `board`, `gen`, `r3d` and `manga`.
- App changes: `main.rs` (menus, close guard, `Editor::perform`), `canvas.rs`, `xsheet_panel.rs`,
  `nodegraph_panel.rs`, `graphcomp.rs` (CPU fallback route only), `config.rs`, `workspace.rs`, `plate.rs`,
  `palette*.rs`, `export.rs` and `newproject.rs`.
- The app gains new files: `testkit`, `shortcuts`, `proc`, `worksafe`, `menus`, `cmdpal`, `history_panel`,
  `welcome`, `templates`, `sample`, `fxui`, `paintcheck`, `render_queue`, `playback`, `curves_panel`,
  `board_panel`, `view3d` and `manga_tools` (`storyboard` only if the cut board part reaches its Stretch).
- The repo gains `tools/smoke_render.sh` (ship part) and small sound fixtures in
  `crates/anim-core/tests/fixtures/` (sound part).
- **Not touched:** the brush dab maths beyond hardness and aliasing, the session transport (only its version
  floor, at ship), `kpp`/`kritares`/`brushbank`/`forge`, devloop's files, and `tools/release.sh` (never run).

STAGES (one part each, in this run order; review 2026-10-28 split the X-sheet into its engine and its panel,
so the sheet panel and graph editor part, GM14, runs sixth):
1. testkit
2. never lose work
3. command surface
4. pen aids
5. X-sheet engine
6. sheet panel and graph editor
7. light table
8. ink and paint
9. welcome
10. FX core
11. camera
12. delivery
13. sound
14. satsuei look
15. editorial
16. generators
17. 3D
18. manga
19. sweep: painting on the extended paper (added 2026-10-30, like the next two)
20. sweep: menus, dialogs and Shift+Delete
21. sweep: View3D worker, sequence reload and the 3D underlay
22. ship

## 1. Definition of done

"Fully built out" means two things. Every row below is verified by its test, and the test runs in its part's
gate. Then the ship part's gate passes: the whole test run, the ship test count, the release exe, a smoke
render of that exe through the installed ffmpeg, and the installer build. A filter word in the last column
runs with `cargo test -p animstudio <word>`. A `*_tests` name is an anim-core test file, run with
`cargo test -p anim-core --test <name>`. Every gate runs the whole suite first and then checks a minimum
count of its part's tests with `-- --list` (D0), so a filter that matches nothing fails the gate.

| Stage or persona | Capability | Part | Verified by |
|---|---|---|---|
| Layout (レイアウト) | Paper larger than the frame; camera frame on the paper; field guide and safe frames | GM10 | camera_tests: crop at pan ends, field-guide rects |
| Layout | Layout templates, the LO room and its stack | GM8 | welcome: template sets size, fps, cut length, room |
| Layout | 3D set/props under the drawing as layout reference | GM17 | r3d_tests: projection, toon render |
| Genga (原画) | Keys marked and circled on the sheet | GM6 | lighttable: kind round-trip, sheet mark |
| Genga | Canvas rotate/mirror, stabilizer, straight lines, size keys, eyedropper | GM4 | viewnav: transform round-trip, stroke tap |
| Timing | Insert/delete frames (ripples param columns too), hold ±1, retime 1s/2s/3s, reverse, typed entry, cut length | GM5, GM14 | sheet_edit_tests; sheetui |
| Douga (動画) | Spacing charts, make inbetweens, duplicate cel, pixel clipboard | GM6 | lighttable: inbetweens named and exposed |
| Light table / onion | Depth 0–5, modes drawings/frames/keys, pins, shift & trace | GM6 | lighttable: onion set pure function |
| X-sheet | Column kinds Cel/BG/Book/Ref, hidden/locked, memo/dialogue row, copy/paste, scaffolding undo | GM5, GM14 | sheet_edit_tests; sheetui |
| Cleanup | Aliased lines for fills, stabilizer, paint checks for holes and stray colours | GM4, GM7 | viewnav; color_tests |
| Shiage (仕上げ) | Exact colour law, recolour by chip edit, refill, chip pick | GM7 | color_tests: chip bytes exact |
| Colour designation (色指定) | Palette variants and palette map; colour designation sheet export | GM13, GM15 | look_tests; board_tests |
| Backgrounds | BG/Book columns, image import at native size, JPEG/TIFF, multiplane | GM5, GM10, GM17 | sheet_edit_tests, camera_tests, r3d_tests |
| Satsuei (撮影): glow, diffusion | Blur, Glow (T光), Diffusion, Grade | GM9 | fx_tests |
| Satsuei: light and shadow passes | Role passes, trace-line absorb, para, light wrap, rim, vignette, grain, directional motion blur, LUT, matte, the satsuei template (CA, light rays and colour Curves are Stretch) | GM13 | look_tests; comproom |
| Satsuei: particles | Deterministic particles with presets | GM16 | gen_tests |
| Satsuei: grading | Grade (lift/gamma/gain, exposure, saturation, temperature) and .cube LUT; colour Curves are Stretch | GM9, GM13 | fx_tests, look_tests |
| Satsuei: camera work | Camera node pan/T.U./T.B./rotate/shake; ease in/out and the graph editor (the camera path overlay is Stretch) | GM10, GM14 | camera_tests, curve_tests |
| Satsuei: depth | Multiplane depth, parallax, depth of field; 3D depth pass | GM10, GM17 | camera_tests, r3d_tests |
| Editing (編集) | Cut board, episode timeline, TRT, play episode, episode render, CMX3600 EDL | GM15 | board_tests; cutboard |
| Sound | Tracks, WAV/MP3/OGG/FLAC, waveform, audio-locked playback, scrub, lip flap, mixdown | GM12 | sound_tests |
| Final render and export | Render queue, PNG16/TIFF16/EXR, ProRes/DNxHR/H.264/H.265 when ffmpeg exists, audio muxed, burn-ins, line test, final role policy, batch render from the command line | GM11, GM19 | delivery_tests; render_queue; the ship gate's smoke render |
| 3D artist | glTF/OBJ import, toon render with outlines and passes, View3D with Blender navigation, camera keys, image sequences, 3D underlay on the canvas (turntable is Stretch) | GM17 | r3d_tests; view3d |
| 3D artist | Node workflow: searchable add-node, generated inspectors | GM9 | fx_tests; fxnode |
| 3D artist | Graph editor with Bezier tangents | GM14 | curve_tests; curves |
| Manga artist | B5/A4 350 dpi and webtoon templates, panels with gutters, tones, perspective rulers, page export | GM18 | manga_tests; mangaui |
| Manga artist | Speed lines, focus lines, lettering with tategaki, balloons | GM16 | gen_tests; genui |
| Modern animator | Menus, command palette, undo history, contextual help | GM3 | cmdui |
| Modern animator | Never lose work: unsaved guard, autosave, recovery, backups | GM2 | worksafe |
| Modern animator | Real-time playback with sound, RAM preview | GM12 | sound_tests; playback |
| Modern animator | Animatic from storyboard panels, cuts made from panels, retake notes (the conte storyboard pane is Stretch) | GM15 | board_tests; cutboard |
| Pick up and use | Welcome screen, templates, sample cut, Quick start checklist, UI scale (the first-run role chooser is Stretch) | GM8 | welcome |
| Pick up and use | Role rooms (5 on the spine plus Manga and 3D) | GM8, GM17, GM18 | welcome; view3d; mangaui |
| Pick up and use | Tooltips with shortcuts, F1 sheet, every Action described | GM3, GM19 | cmdui; ship tests |
| Pick up and use | Coherent design: plate tokens, ratchet to zero, layout probe over every room | GM1, GM19 | testkit; ship tests |
| Ship | 0.5.0, README, release build, smoke render and installer built locally; no tag | GM19 | ship_; the gate's release builds, smoke render and tag check |

## 2. What exists today (v0.3.0, HEAD 56679ad, tree clean)

**Engine** — `crates/anim-core`: 5,159 lines, about 112 `#[test]`s.
- `lib.rs`: `Engine::apply(label, Vec<Command>)` with exact inverses. History is `undo_stack:
  VecDeque<Applied>` plus `redo_stack`, and `Applied` carries `label` and `author`.
- `command.rs`: AddDrawing, PaintTiles, Add/Remove/MoveCelLayer, SetCelLayerProps, Add/RemoveImage,
  RemoveDrawing, RenameDrawing, AddStroke, PopStroke, SetCell, SetCutAudio, SetParamKey, Add/RemoveNode,
  Connect, Disconnect, SetNodeKind, SetOutput.
  - Column, param column and cut add/remove are **not** Commands, so they are not undoable.
- `model.rs`: `Project { name, fps: u32, width, height, dpi, scenes, next_id, app_meta }`, then Scene, then
  `Cut { id, name, frame_count, drawings, xsheet, graph, images, audio: Option<AudioClip> }` at line 642.
  - Drawings hold cel layers with an explicit `LayerRole`: Reference, Color, Shadow, Highlight, Rough,
    ShadowLine, HighlightLine, Line, Correction, Custom.
  - `content_hash` is at lines 270/322.
- `xsheet.rs`: `Column { id, name, keys: BTreeMap<u32, Exposure> }` and `Exposure { Drawing(id), Empty }`.
  `ParamColumn` keys are `ParamKey { value, interp: Hold|Linear|Ease }`.
- `graph.rs`: `NodeKind` is DrawingSource{column}, Solid{rgba}, ImageSource{image}, Transform{translate,
  scale, rotate_deg, binds}, Blend{mode} or Output. `BlendMode` is Normal, Multiply, Add, Screen, Subtract,
  Darken, Lighten or Overlay.
- `export.rs`: the CPU golden renderer.
  - Column path: `render_frame`, first column at the bottom.
  - Graph path: `render_graph_frame`, which returns None when no output is wired.
  - Pixels are premultiplied f32 from f16 tiles. `srgb_to_linear` is at line 141.
- `eval.rs`: memoised symbolic evaluation (hashes).
- `fill.rs`: headless flood fill. `edit.rs`: region maths.
- `raster.rs`: tiles, with `TileCoord = (i32, i32)`, so negative coordinates are legal.
- `store.rs`: SQLite, `SCHEMA_VERSION = 9` at line 19.
  - Load refuses any version outside 1..=9 (line 341).
  - Save deletes from known tables only, so unknown tables survive a re-save.
  - Nodes are stored as JSON of `NodeKind`. Param interp is stored as a name ("hold", "linear", "ease").
  - Audio keeps the encoded WAV bytes verbatim.
- `xdts.rs`: XDTS timesheet interop.
- `tests/`: audio, edit, engine, fill, graph_render, image, layer, migration_soak, param, xdts.

**App** — `app/`, package `animstudio` 0.3.0: 29,161 lines in 26 files, about 58 `#[test]`s. Built on egui
0.35, wgpu 29, egui_dock 0.20, octotablet and rodio 0.21.
- `main.rs` (5,288 lines):
  - `App` at line 176, `Editor` at 2961.
  - `App::perform(action)` at 2558 handles every Action.
  - `Editor::from_state(state, None)` at 3307 builds an editor with no GPU (no PaintLayer, no
    GraphCompositor).
  - `probe()`/`probe_at()` at 3868 is a CLI headless layout check at ppp 1.0 and 1.25.
  - `AudioOut` is rodio-based.
- `canvas.rs` (6,180 lines): `CanvasView`, the tools Paint/Select/Fill/LassoFill, onion skin and input.
  Select needs the GPU to lift and preview (line 1087).
- `paint.rs`: the GPU raster brush. `graphcomp.rs`: the GPU graph compositor.
- `doc.rs`: `AppState`, the engine plus position.
- `xsheet_panel.rs`: the sheet, with frames as rows, and the layer strip.
- `nodegraph_panel.rs`: the node graph pane.
- `viewer.rs`, `floatwin.rs`, `floatcanvas.rs`: viewers and OS windows.
- `workspace.rs`:
  - `Pane { Canvas, XSheet, Layers, Brush, Presets, Forge, Palette, NodeGraph, Viewer(u8) }`.
  - `Stage { Layout, Drawing, Finishing, Edit }`, labelled with romaji plus English ("genga (drawing)").
  - workspaces.json.
- `config.rs`:
  - `Config` (about line 995): keybinds, perf, ui, session, pen, layers, presets.
  - `Action` has 40 verbs (line 14).
  - **A whole-file parse failure silently resets everything.**
- `plate.rs`: 8 tokens (GRAPHITE, WELL, LEGEND, STRUCK, AO, AKA, TALLY, PAPER) and the affordance widgets.
  Fonts are IBM Plex, plus the **subset** JP font at line 187. The full `IBMPlexSansJP-Bold.otf` (4 MB) sits
  unused in `app/assets/fonts`.
- `palette.rs`, `palette_panel.rs`: character palettes stored as JSON in `app_meta["palettes"]`, outside
  undo.
- `export.rs`: PNG8 sequence, and MP4 through the system ffmpeg.
- `net.rs`: sessions, with `MIN_PEER_VERSION = "0.3.0"` at line 229.
- `update.rs`: relaunch to update. `devloop.rs`: restart on rebuild, using its own `dev_autosave.animproj` and
  `dev_session.json` (never touch them). `uidump.rs`: the UI dump hook.
- `newproject.rs`: the New Project dialog.
- 86 `Color32` literals sit outside `plate.rs` (measured with GM1 criterion 11's tokens; 52 of them in
  canvas.rs and 19 in nodegraph_panel.rs).

**Tools.**
- `tools/installer` (`animstudio-installer` 0.3.0) embeds `target/release/animstudio.exe`.
- `tools/iconforge`.
- `tools/release.sh` runs the tests, both release builds and the publish steps. **This room never runs it.**

**Research to lean on:**
- `DESIGN-SPEC-editor-ui.md`: the plate law, and "no green".
- `one-piece-2026-pipeline-research.md`: the modern TV pipeline.
- `workflow-trees.md`, `ROOM-CHARTERS.md`, `layers-per-cel-design.md`, `PSD-cel-layers.md` (roles, presets),
  `gpu-graph-compositor-design.md`, `performance-settings.md`.

**Missing today** (what this room builds):
- Undo for scaffolding.
- A dirty mark, close guard, autosave and recovery.
- Menus, a command palette, a history view and help.
- Canvas rotation and pen aids.
- Real X-sheet editing.
- Drawing kinds and inbetween tools.
- Exact colour.
- FX, camera, delivery formats and multi-track sound.
- Editorial, manga and 3D.
- A headless UI test harness.
- Tolerant config loading.

## 3. Shared decisions (every part is bound by these)

**D0. Working rules.**
- **Where:** `C:/Users/Lancer/Desktop/GabanStudio` on branch `modernize`. One implementer per part, then two
  verifiers and a fixer.
- **Commits:** the implementer commits locally as `<id>: <title>` and never pushes. When the part is
  finished, it adds a dated line to `## Build log` at the end of this doc.
- **Gate:** every gate starts with `cd /c/Users/Lancer/Desktop/GabanStudio && export
  PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core`. Every part leaves the app
  building with all tests green.
  - Then the gate checks each of its test minimums with a listing: `n=$(cargo test -q -p animstudio <word> --
    --list | grep -c ': test$') && [ "$n" -ge <min> ]` (or `-p anim-core --test <file>`). The full run comes
    first and is not piped, so the listing decides only the count. A filter that matches nothing, or a build
    that fails, lists 0 tests, `grep -c` exits 1 and the gate fails.
- **Never loosen a test.** Existing assertions stay as written.
- **Size:** a part is roughly 300–1500 lines of Rust plus tests, in one subsystem, with a cap of 45 min at
  most (review 2026-10-28 cut every 60-min cap to 45 and moved work between parts to fit). If the cap is hit,
  the section's **Stretch** items go to `left`; core items never do.
- **App tests** are unit tests inside the `animstudio` bin crate (`#[cfg(test)]` modules under `app/src`).
  Every new app test's name contains its part's filter word. cargo's filter is a substring of the test path,
  so a filter that matches nothing would pass silently. Each section names its minimum, and **the gate
  checks the count** (the Gate bullet above).
- **anim-core tests** go in the integration file the gate names, `crates/anim-core/tests/<name>.rs`.
- **Test limits:** each test finishes in under 2 s in a debug build, and test images are at most 256×256
  unless size is the point. There is no wall clock (clocks are injected), no sleep, no network, no child
  process, no window, no GPU device and no audio device.

**D1. Engine laws.**
- Every edit is `Engine::apply(label, cmds)` with exact inverses. One gesture is one undo step: a drag merges
  into one entry.
- `content_hash` covers pixels and timing only. A property that does not change pixels must not enter it.
- The three render paths must agree:
  - `eval.rs` (symbolic hash);
  - `export.rs` (CPU golden, the truth);
  - `graphcomp.rs` (GPU).
- Any node the GPU path cannot do exactly is rendered by the CPU and uploaded (D9).

**D2. Schema v11 and side tables.**
- The part that adds the first new table (the X-sheet part) sets `SCHEMA_VERSION = 11`.
  - Load accepts 1..=11.
  - Version 10 (the lost v0.4.0 dev build) loads as "v9 plus unknown tables"; its tables, such as
    `cel_layer_groups`, are kept untouched through re-saves.
  - 12 and above refuse with a clear "made by a newer Gaban Studio" error.
- New data lives only in new side tables created `IF NOT EXISTS` on save only. Load never writes the file
  (NEVER-DO 5; today store.rs creates tables only on save and probes by version on load, 604-607): it checks
  `sqlite_master` for each side table and reads a missing one as empty. So later parts add tables inside v11
  with no further bump.
- Save adds each new table to its `DELETE FROM` list. Existing tables never gain columns: re-saves write into
  the old file shape.
- Tables in this room:
  - `cut_meta(cut_id, key, value)`, `column_defs(column_id, json)` and `cut_memos(cut_id, frame, text)`, from
    the X-sheet part;
  - `drawing_defs(drawing_id, json)`, from the light table part;
  - `audio_tracks(cut_id, ord, json, bytes)`, from the sound part;
  - `cut_blobs(cut_id, key, bytes)`, from the satsuei look part;
  - `cut_models(id, cut_id, name, format, bytes)`, from the 3D part.
- Project-level keys go in `meta` (`color_law`, `default_cut_frames`).

**D3. Cut meta.**
- `Cut.meta: BTreeMap<String, String>` holds namespaced keys (`camera.paper`, `prod.status`, `graph.layout`,
  `manga.page`, ...) whose values are JSON. It is edited only through `Command::SetCutMeta { at, key, before:
  Option<String>, after: Option<String> }`.
- A key's owner part defines its JSON. Unknown keys round-trip untouched.

**D4. Scaffolding is undoable, and ripples are total.**
- The X-sheet part turns these into Commands with exact inverses that snapshot what they remove: add,
  remove, rename and move for columns; add and remove for param columns; add, remove and rename for cuts.
- Insert/DeleteFrames ripple **every frame-indexed datum in the cut**: exposures, param keys, memos, and any
  frame-indexed data a later part adds.
- A later part that adds frame-indexed data (retake notes, generated keys, ...) extends the ripple and adds
  a test for it.

**D5. Versions and the wire.**
- Only the ship part sets the version to 0.5.0 (app and installer) and `MIN_PEER_VERSION` to "0.5.0".
  Parts in between keep 0.3.0.
- New Commands and NodeKinds derive serde like the existing ones, so sessions carry them unchanged.

**D6. One verb registry.**
- `config::Action` is the single list of verbs. Every new verb gets:
  - an `Action` variant and its place in `Action::ALL`;
  - `help()` (one plain sentence), `group()`, `menu()` (a path, or None for palette-only) and `aliases()`
    (from the command surface part on);
  - default chords in both keymaps (Gaban and Industry);
  - a `context()` (from the command surface part on): Global, Canvas, Sheet or NodeGraph. The context is the
    pane under the pointer, or else the focused pane. A chord may repeat only across different non-Global
    contexts (Ctrl+C copies cells in the Sheet and pixels on the Canvas).
- `Editor::perform(action)` (from the testkit part) is the one dispatch. Menus, the palette, shortcuts and
  tests all call it.
- The command surface part adds a zero-conflict test that every later part keeps green.
- Single-key chords never fire while a text field has focus (the testkit part).

**D7. UI conventions.**
- **Plate:** tokens from `plate.rs` and its widgets (DETENT/LATCH/DANGER affordances) only. Destructive
  actions use DANGER, refusals go to the Aka lane through the existing `refuse()` pattern, and there is no
  green.
- **Ratchet:** the testkit part pins the colour-literal count outside `plate.rs`. Later parts may only lower
  it, and the ship part drives it to 0.
- **Panes:** new panes are `Pane` variants that live in workspaces. Fixed slots: controls never move under
  the pointer, and the layout probe catches regressions.
- **Accessibility:** icon-only widgets carry accessible labels through `widget_info` (the testkit finds them
  that way).
- **Labels:** the house pattern is the romaji term plus English, e.g. "satsuei (composite)". Kana or kanji
  may appear in the UI only where the embedded font has the glyphs; a test checks this once the generators
  part embeds the full JP font.
- **The pen comes first:** nothing modal opens and nothing heavy runs mid-stroke.
- **Design skill:** the installed `frontend-design` skill may guide hierarchy and spacing. The plate and the
  DESIGN-SPEC override it on tokens.

**D8. Colour law.**
- Today the canvas, paint, fill and Solid paths linearise chip colours (`srgb_to_linear`, `linear_rgba`),
  while egui shows textures gamma-encoded and export writes the stored values. So screen and export agree
  with each other, but both are darker than the chip.
- `anim_core::color::ColorLaw { Legacy, Srgb }` is stored in `meta.color_law`. A missing key means Legacy.
  - `encode(chip) -> stored`: Legacy linearises; Srgb is `c/255`.
  - The stored-to-bytes path is the same for both.
  - So under Srgb, export bytes equal the chip bytes, the display path is unchanged, and compositing happens
    in the stored (gamma) space, as in the AE/Photoshop/CSP defaults.
- **The engine default and `AppState::new_project` stay Legacy**, so every existing test keeps its numbers.
  The New Project dialog and every template create Srgb projects.
- Every place that linearises a chip goes through `ColorLaw::encode` (the ink and paint part finds them all).
- EXR output is scene-linear: Legacy writes the stored values; Srgb linearises them.
- Converting a Legacy project is a file-level migration (NEVER-DO 3):
  1. back up to `<stem>.pre-srgb.animproj`;
  2. remap the tiles;
  3. clear history;
  4. the user saves.

**D9. FX run on the CPU; the GPU falls back.**
- Effects, cameras, generators, 3D, image sequences, role-filtered sources and palette maps are implemented
  once, in anim-core, on premultiplied f32 RGBA buffers, with rayon where it pays.
- `graphcomp` routes them as follows:
  - `needs_cpu(kind)` marks a node, and any subtree containing one is rendered on a worker thread by the
    anim-core renderer from a `Cut` snapshot (cheap Arc clones).
  - The result is uploaded with `queue.write_texture` and cached by (node, frame, eval hash).
  - While a render is pending, the last texture stays up with a TALLY "rendering" dot.
- No WGSL ports of FX in this room.

**D10. Headless UI testing (the testkit part builds it).**
- Dependencies: dev-dependency `egui_kittest = { version = "0.35", default-features = false }`, which uses
  AccessKit queries with no wgpu and no window, plus `tempfile = "3"`.
- `app/src/testkit.rs`, with `#[cfg(test)] mod testkit;` in main.rs, provides:
  - Setup: `EditorHarness::new()` (a 1920×1080, 24 fps project on a 1280×800 screen at ppp 1.0, with a temp
    data dir), `::with_state(AppState)` and `.with_ppp(f32)`.
  - Running: `run()` and `run_frames(n)`.
  - Input: `click(label)` (exactly one match, else a panic with `describe()`), `click_nth(label, n)`,
    `hover`, `key`, `key_mods`, `type_text` and `drag(from, to, steps)`.
  - Queries: `has(label)`, `rect(label)` and `describe()`, which prints the AccessKit tree as text.
  - State: `state()`/`state_mut()`, `editor()`/`editor_mut()`, `perform(Action)` and `data_dir()`.
  - `PanelHarness` drives one panel's `ui` function in a fixed rect.
- **There is no GPU in tests.** `Editor::from_state(state, None)` has no PaintLayer and no GraphCompositor.
  - Brush pixels are tested through the stroke tap (the pen aids part records paper-space samples) and
    through pure CPU mirrors of the shader formulas.
  - Fill, lasso, recolour, FX, camera and similar features are tested through their CPU functions,
    Commands and anim-core renders.
- Dialog requests (open, save as) are observed as `AppRequest` values and never opened.

**D11. Paths.**
- Every path function takes a directory (`*_in(dir)`). The real `%APPDATA%/AnimStudio` is resolved only in
  `main()`.
- The data dir holds `autosave/`, `crash.log`, `running.json`, config.json and workspaces.json.
- Writes are atomic: write `<file>.tmp`, then rename it over the file.

**D12. Rooms.**
- The spine:
  - `layout` (LO);
  - `genga (drawing)`;
  - `shiage (ink & paint)`;
  - `satsuei (composite)`;
  - `henshū (edit)`.
- `Stage::Finishing` is renamed `InkPaint`. `Stage` is not serde (workspace.rs 86); a room persists only as
  its `stage_key` string (main.rs 53) in `config.room_stack` (config.rs 732-741, defaults at 837). So
  `stage_key(InkPaint)` stays `"finishing"`, and every saved room-stack binding carries over.
- The welcome part (GM8) adds `Composite` with the key `"composite"`. Its pane layout and its verb live in
  `Stage::dock` and `Stage::view` (workspace.rs 130 and 179). It also gets a layer-stack binding
  (`"composite"` → `"Shiage (full)"`) in `default_room_stacks` and a row in the Layers page's room list
  (config.rs 2130-2136). Note that `room_stack` binds a room to a cel layer-stack preset, not to a pane
  layout.
- Off-spine: `Forge` (exists), `manga (pages)` from the manga part and `3D layout` from the 3D part.

**D13. Defaults.**
- Cut length is the project meta `default_cut_frames`; when it is unset, new cuts use 48 (so old tests stay
  stable). Templates set 72.
- fps defaults to 24 (u32; no 23.976 project rate).
- Autosave: 5 min, newest 3 kept. UI scale: 100%. Keymap: Gaban.

**D14. Lip flap** uses three mouths (closed, half, open) driven by the dialogue track's RMS level, with
hysteresis and a minimum hold of 2 frames. No phonemes.

**D15. Manga and media.**
- Pages are 350 dpi, and their size comes from the trim in mm plus a 3 mm bleed. Webtoon is 800 px wide,
  sliced into 1280 px segments.
- Image sequences from other apps are referenced on disk, relative to the project file when possible.
- 3D models and LUTs are embedded, in `cut_models` and `cut_blobs`.

**D16. Dependencies.**
- Pure-Rust crates only (no C build steps). Pin the major.minor that resolves with the lock.
- Checked against Cargo.lock at 56679ad: only image 0.25.10, tempfile 3.27, windows-sys 0.59, hound 3.5,
  tiff 0.11 and png 0.17 are already locked. egui_kittest, rayon, ab_glyph, symphonia, gltf, tobj and the
  `exr` crate behind image's `exr` feature are **network fetches** (the owner allows installs). Fetch them at
  the start of the part that adds them, and record the resolved versions in the part's result. egui 0.35
  draws text with skrifa 0.42, harfrust 0.7 and vello_cpu 0.0.9 (all locked), so the text module may use
  skrifa outlines and harfrust's `vert` feature instead of ab_glyph if that avoids a fetch.
- If egui_kittest 0.35 cannot be fetched or will not build without wgpu, the testkit falls back to driving
  `egui::Context` passes directly, as `probe_at` does (main.rs 3877). The D10 API stays the same.

| Crate | Where | For | Part |
|---|---|---|---|
| egui_kittest 0.35 (no default features), tempfile 3 | app dev (tempfile also anim-core dev) | headless UI, temp dirs | testkit |
| windows-sys (Win32_System_Console) | app, cfg(windows) | AttachConsole for CLI flags under the GUI subsystem | testkit |
| rayon 1 | anim-core | parallel FX | FX core |
| ab_glyph 0.2 (or the locked skrifa + harfrust) | anim-core | text rasterising | delivery, generators |
| image 0.25 (png, jpeg, tiff; exr is a new feature fetch) | anim-core | TIFF16/EXR writing; JPEG/TIFF import | delivery; 3D |
| symphonia 0.5 (wav, pcm, mp3, ogg, vorbis, flac) | anim-core | audio decoding | sound |
| none (fixtures) | `crates/anim-core/tests/fixtures/` | MP3, OGG and FLAC test clips of at most 20 KB each, made once by hand with the installed ffmpeg 8.1.2 and committed; tests never spawn it | sound |
| gltf 1, tobj 4 | anim-core | model import | 3D |
| pdf-writer (current) | anim-core | optional PDF pages | manga (stretch) |

- **Fonts:** `anim_core::text` embeds the fonts with `include_bytes!("../../../app/assets/fonts/<file>")`,
  and the app uses those same statics, so the bytes exist once.

## 4. Order, budget and fallbacks

The parts run one at a time, in the order of this table (the sheet panel and graph editor part, GM14, runs
sixth). The tokens below are estimates; most are cached reads. Review 2026-10-28 set every cap to 45 min and
moved work between parts so each fits.

| Part | Title | Deps | Cap (min) | Implementer | Verifiers (2×) | Fixer | Total |
|---|---|---|---|---|---|---|---|
| GM1 | Headless UI testkit and input hardening | — | 45 | 2.5M | 2×0.7M | 0.6M | 4.5M |
| GM2 | Never lose work | GM1 | 45 | 3.0M | 2×0.8M | 0.8M | 5.4M |
| GM3 | Menus, command palette, history and help | GM1 GM2 | 45 | 3.0M | 2×0.8M | 0.8M | 5.4M |
| GM4 | Canvas view and pen aids | GM3 | 45 | 3.0M | 2×0.8M | 0.8M | 5.4M |
| GM5 | X-sheet engine: schema v11, undoable sheet edits | GM3 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM14 | Sheet panel and graph editor | GM5 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM6 | Light table and inbetweening | GM4 GM5 GM14 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM7 | Ink and paint | GM4 GM5 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM8 | Welcome, templates, rooms, sample | GM3 GM5 GM6 GM7 | 45 | 3.0M | 2×0.8M | 0.8M | 5.4M |
| GM9 | Compositing FX core | GM5 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM10 | Camera, paper and multiplane | GM5 GM9 GM14 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM11 | Render queue and delivery | GM7 GM8 GM10 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM12 | Sound tracks and playback | GM11 GM14 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM13 | Satsuei look and the satsuei template | GM7 GM8 GM10 GM11 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM15 | Cut board, episode timeline and EDL | GM6 GM11 GM12 GM13 | 45 | 3.0M | 2×0.8M | 0.8M | 5.4M |
| GM16 | Generators | GM9 GM11 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM17 | 3D layout and toon render | GM10 GM11 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM18 | Manga pages, tones, rulers | GM8 GM13 GM16 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM20 | Painting on the extended paper (sweep) | GM4 GM6 GM7 GM10 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM21 | Menus, dialogs and Shift+Delete (sweep) | GM2 GM3 GM5 GM7 GM8 GM11 GM13 GM14 GM18 | 45 | 3.0M | 2×0.8M | 0.8M | 5.4M |
| GM22 | View3D worker, sequence reload, 3D underlay (sweep) | GM4 GM6 GM9 GM11 GM12 GM17 GM20 GM21 | 45 | 3.5M | 2×0.9M | 0.9M | 6.2M |
| GM19 | Ship | all | 45 | 2.0M | 2×0.6M | 0.4M | 3.6M |

**Totals:** implementers about 61.5M, verifiers about 32.2M, fixers about 15.8M, **about 110M tokens overall
(109.5M, ±20%)**. Implementer time is at most 19 × 45 min = 14.25 h. Wall time with verifiers and fixers is
about 18–22 h. The three sweep parts (added 2026-10-30, before the ship part) add about 17.8M and 3 × 45 min.

**Checkpoint.** After the sound part (the 13th in run order), the plan has spent about 75.7M. If the actual
spend is above 91M (120%), apply the fallback below before starting the satsuei look part.

**If usage runs short:**
1. Mark the persona parts `removed`, in reverse order: the manga part first, then the 3D part, then the
   generators part (manga depends on it, so it never goes before manga).
2. Always run the ship part. The release must be coherent even without the persona parts.
3. A part that hits its cap hands its Stretch items on in `left`. It never drops core acceptance criteria.

**Moved to Stretch by the review** (built only if a part has time left, otherwise listed in `left`): the
storyboard pane and conte pages (cut board part), colour Curves, chromatic aberration and light rays (satsuei
look part), the camera path overlay (camera part), the turntable and 2× supersampling (3D part), and the
first-run role chooser (welcome part).

## 5. Out of scope for this room

- Scanned-paper cleanup: binarise, despeckle, colour-pencil line separation.
- WGSL ports of FX, and a GPU 3D renderer.
- Vector cleanup layers and vector strokes.
- AI assist of any kind: auto-inbetween, auto-colour.
- Importing or merging cut packages back from freelancers (export of one cut is in).
- Symmetry and ellipse rulers (perspective rulers are in).
- Internet session relay, code signing, and the plumbing rename.
- Per-cel-layer blend modes. Role passes in the graph cover multiply shadows instead.
- 23.976/29.97 project rates and drop-frame timecode.
- FBX/Alembic, rigging and simulation.
- Microphone recording.
- Production-tracking servers.

---

## GM1 Headless UI testkit and input hardening

*Depends on: nothing. Cap 45 min. Filter word: `testkit`. Binding decisions: D0, D6, D7, D10, D11.*

**Goal.** Give every later part a way to test UI with no window and no GPU. Fix the input and config
fragility that would otherwise bite every later part.

**User-facing behaviour.**
- Typing in any text field never triggers single-key shortcuts: a cut name "B12" stays text. Modifier chords
  (Ctrl+S, Ctrl+K later) still fire. The field keeps its own Ctrl+Z/Y/A/C/V/X.
- A config file with one bad section loads every other section. A bad keybind is dropped on its own. The
  original file is copied to `config.json.bad`, and one note appears in the status lane. workspaces.json
  behaves the same.
- Config and workspace writes are atomic, so a crash mid-write cannot truncate them.
- Release builds open no console window. Helper processes (ffmpeg, powershell, reg) run with
  `CREATE_NO_WINDOW`, so no console flashes. CLI flags such as `--probe` still print to the terminal: they
  call `AttachConsole(ATTACH_PARENT_PROCESS)` when present.
- Icon-only buttons carry accessible names.

**Files and types to extend.**
- `app/Cargo.toml`:
  - `[dev-dependencies] egui_kittest = { version = "0.35", default-features = false }` and `tempfile = "3"`;
  - `windows-sys` (feature `Win32_System_Console`) under `[target.'cfg(windows)'.dependencies]`.
- New `app/src/testkit.rs` with the D10 API.
  - Wrap kittest's AccessKit queries, or drive `egui::Context` passes directly as `probe_at` does (main.rs
    3877): `Context::default()`, `begin_pass`, `editor.ui(&mut root, &pen, &layers_cfg, &mut presets, &mut
    presets_dirty, &[], lock, None)`, `end_pass`.
  - It needs `ctx.enable_accesskit()` for label queries.
- `app/src/main.rs`:
  - Add `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` at the top.
  - Move the editor-level arms of `App::perform` (line 2558) into `impl Editor { pub fn perform(&mut self, a:
    Action) -> Performed }`, with `enum Performed { Done, Ignored, App(AppRequest) }` and `enum AppRequest {
    NewProject, Open, Save, SaveAs, Settings, Quit, .. }`. `App::perform` calls it and handles the
    AppRequest.
  - Keep the probe scenario code in one function, shared by the CLI and the test.
- New `app/src/shortcuts.rs`:
  - `pub fn collect(ctx: &egui::Context, binds: &[Binding]) -> Vec<Action>`, moved out of main.rs.
  - The text-focus rule: when an egui text widget has keyboard focus, drop bindings without Ctrl or Alt, and
    leave Ctrl+Z/Y/A/C/V/X to the field.
- `app/src/config.rs`:
  - `Config::load_from(dir) -> (Config, Vec<String>)` parses into `serde_json::Value`, then section by
    section with defaults; keybinds are taken element by element.
  - `Config::save_to(dir)` is atomic (tmp then rename).
  - The existing `load()`/`save()` wrap these over the real dir.
- `app/src/workspace.rs`: the same tolerant load and atomic save for workspaces.json.
- New `app/src/proc.rs`: `pub fn command(program) -> std::process::Command`, which sets `creation_flags(0x0800_0000)`
  on Windows. The spawn sites at 56679ad are export.rs 208 (ffmpeg) and 305 (the `-version` probe), and
  main.rs 525 and 554 (the update hand-over relaunch of the new exe; update.rs itself spawns nothing). They
  all use it. devloop.rs keeps its own two spawns (220, 310) untouched (BLAST RADIUS), because hiding a dev
  relaunch's console would hide its log.
- `app/src/icons.rs` / `plate.rs`: painted icon buttons call `response.widget_info(||
  WidgetInfo::labeled(WidgetType::Button, enabled, label))`.

**Acceptance criteria.**
1. `EditorHarness::new()` runs 10 frames headless. It has no window, no wgpu device (render state None) and no
   audio, and `describe()` lists the room switcher and the tool deck.
2. Clicking the fill tool by its label sets `canvas.tool == CanvasTool::Fill`. A missing label panics, and the
   message contains `describe()` (`should_panic` test).
3. With a text field focused, pressing the BrushTool/FillTool chord leaves the tool unchanged and puts the
   character into the field. With no focus, the same chord switches the tool.
4. Ctrl+S with a field focused returns `Performed::App(AppRequest::Save)`.
5. Every `Action::ALL` entry can be performed on an `Editor` without panicking. Undo, Redo, tool switches and
   the onion toggle change state exactly as `App::perform` did.
6. A config whose `ui` section has a wrong type, and whose keybinds include one unknown action, keeps every
   other section and keybind. It writes `config.json.bad` with the original bytes and returns exactly one
   note.
7. Saving goes through `<file>.tmp` and a rename. A leftover `.tmp` is ignored on load. A valid config
   round-trips byte-stable.
8. workspaces.json: a single bad workspace is dropped and the rest load.
9. The layout probe runs as a test for every `Stage` at ppp 1.0, 1.25 and 1.5. After the settle step it
   reports 0 MOVED.
10. A source scan finds no `Command::new(` in `app/src` outside `proc.rs` and `devloop.rs` (the one exempt
    file), and finds the `windows_subsystem` attribute in main.rs.
11. A colour-literal ratchet test counts `Color32::from_rgb(`, `from_rgba_unmultiplied(`,
    `from_rgba_premultiplied(`, `from_gray(`, `from_black_alpha(`, `from_white_alpha(` and named colour
    constants other than `TRANSPARENT` in `app/src/*.rs` outside `plate.rs`.
    - It pins the measured count: 86 at 56679ad with exactly these tokens (canvas.rs 52, nodegraph_panel.rs
      19, uidump.rs 4, xsheet_panel.rs, viewer.rs, main.rs and floatwin.rs 2 each, palette_panel.rs, forge.rs
      and config.rs 1 each). Re-measure at the part's start and pin that number.
    - It fails if the count rises, and prints the per-file counts.
12. The harness's config, workspace and autosave paths are under its temp dir. No test computes the real data
    dir.

**Tests to add.** At least 12 tests named `testkit_*`, covering the twelve criteria above.

**Out of scope.** Menus and the palette (the command surface part), autosave (the never-lose-work part), the
stroke tap (the pen aids part) and any restyle.

**Stretch.** `PanelHarness` over the node graph pane; the probe at ppp 2.0.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p animstudio testkit -- --list | grep -c ': test$') && [ "$n" -ge 12 ]`

## GM2 Never lose work: unsaved guard, autosave, recovery

*Depends on: GM1. Cap 45 min. Filter word: `worksafe`. Binding decisions: D0, D1, D2, D10, D11, D13; NEVER-DO 5 and 6.*

**Goal.** No work is lost to a close, a crash, a mis-click or a slow disk.

**User-facing behaviour.**
- **Dirty mark.** The window title and the slate show `●` while the document has unsaved changes. The mark
  clears on save, and also when undo returns to the saved state.
- **Close guard.** Close, New, Open, Open Recent and New-from-template on a dirty document open an in-app
  modal: Save / Don't Save / Cancel. The window close goes through `ViewportCommand::CancelClose` until the
  modal resolves. Save on an untitled document goes to Save As.
- **Background save.** Save runs on a background thread from a snapshot (a `Project` clone, cheap Arcs), so
  the pen never waits. The write is atomic:
  1. copy the existing file to `<file>.tmp`;
  2. save into the tmp;
  3. rename it over the file.
  - This keeps unknown tables (D2).
  - Before replacing the file, the previous version is kept as `<name>.animproj.bak` (one generation).
- **Autosave.** Every N minutes, default 5. A new Settings ▸ General page offers off, 1, 2, 5, 10 and 15
  (`SettingsCategory` at config.rs 907 has no General page today; this part adds it).
  - It runs only when the document is dirty, no stroke is in progress, and the pointer has been up for at
    least 2 s.
  - It writes `<data>/autosave/<stem>-<hash8>.animproj` and keeps the newest 3 per project. It never writes
    the user's file, and never touches the devloop's files.
- **Crash recovery.**
  - At start-up the app writes `<data>/running.json` (pid, open path, autosave path, start time) and removes
    it on a clean exit.
  - If the file is still there at the next start, a Recovery dialog lists the autosaves that are newer than
    the document: Open recovered / Discard / Later.
  - A panic hook writes `<data>/crash.log` (time, message, location) and leaves `running.json` in place, so
    recovery triggers.
- **Recent files.** The 12 most recent, deduplicated. Missing files are greyed out with a Remove option. The
  list is stored in the config's session section and shown in the existing Open control's dropdown (the
  command surface part adds the menu, the welcome part the welcome screen).

**Files and types to extend.**
- `crates/anim-core/src/lib.rs`: `Engine::revision() -> u64`. Each `Applied` gets a unique serial, and the
  revision is the serial on top of the undo stack (0 when it is empty).
- `crates/anim-core/src/store.rs`: `save_atomic(project, path)` (copy, then save into the tmp, then rename,
  plus `.bak`).
- `app/src/doc.rs`: `AppState::{saved_rev, meta_dirty, is_dirty()}`. Edits to project settings and palettes
  outside undo set `meta_dirty`.
- `app/src/main.rs`:
  - `window_title(&AppState) -> String`;
  - close handling through `ctx.input(|i| i.viewport().close_requested())`;
  - the modal;
  - New/Open/Recent through the guard;
  - the recovery dialog at start.
- New `app/src/worksafe.rs`:
  - `AutosaveClock`, a pure scheduler over an injected clock;
  - `autosave_path_in(dir, doc)` and `prune_autosaves(dir, keep)`;
  - `RunningRecord` write, read and remove;
  - `recovery_candidates(dir)`;
  - `crash_report(info) -> String` and `install_panic_hook(dir)`.
- `app/src/config.rs`: `SessionConfig.recent: Vec<PathBuf>`, `autosave_minutes: u32` (default 5) and
  `push_recent()`.

**Acceptance criteria.**
1. A new project is clean. One gesture makes it dirty, undo makes it clean, redo makes it dirty again and save
   makes it clean. Editing a palette sets dirty.
2. `window_title` contains `●` exactly when the document is dirty.
3. A close request while dirty emits `CancelClose` and opens the modal (the harness sees "Save changes?").
   - Don't Save closes, and Cancel stays.
   - Save on an untitled document emits `AppRequest::SaveAs` and opens no dialog.
4. A background save writes a file whose load equals the snapshot (`content_hash` and structure). Edits made
   after the snapshot are not in the file and stay dirty.
5. A file holding an extra table `zz_unknown` keeps that table after a save.
6. After a second save, `.bak` holds the first version.
7. `AutosaveClock` fires only when dirty, idle for at least 2 s and with the interval elapsed. It never fires
   during a stroke and never when autosave is off. `prune_autosaves` keeps the newest 3.
8. A stale `running.json` plus an autosave newer than the document returns that autosave from
   `recovery_candidates`. With no `running.json`, it returns nothing.
9. `crash_report` contains the panic message and location, and the writer puts `crash.log` in the given dir.
   Do not test the global hook itself.
10. Recent files: opening pushes to the front, deduplicates and caps at 12. The list survives a config round
    trip. A missing path is flagged.

**Tests to add.** At least 10 tests named `worksafe_*` (app), plus revision tests in anim-core's
`tests/engine_tests.rs`, written as new test functions.

**Out of scope.** Version history beyond one `.bak`, cloud sync and per-cut autosave.

**Stretch.** File ▸ Revert to Saved.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p animstudio worksafe -- --list | grep -c ': test$') && [ "$n" -ge 10 ]`

## GM3 Menus, command palette, history and help

*Depends on: GM1, GM2. Cap 45 min. Filter word: `cmdui`. Binding decisions: D0, D6, D7, D10.*

**Goal.** Every verb can be found: a menu bar, a searchable command palette, a visible undo history and help
in context, all driven by the one `Action` registry.

**User-facing behaviour.**
- **Menu bar.** It sits in the top chrome without moving the canvas (the layout probe stays at 0 MOVED). It
  has eight menus:
  - File: New, Open, Open Recent ▸, Save, Save As, Export ▸, Settings, Quit.
  - Edit: Undo, Redo, History, Select All.
  - View: zoom, onion, composite view; later parts add more.
  - Sheet, Cut, Room, Window (open or focus each pane; reset layout) and Help (Quick start, Keyboard
    shortcuts, About).
- Each menu item shows its chord, and its tooltip gives one line of help. An item that cannot run is disabled
  and says why: `Editor::can_perform(action) -> Result<(), &'static str>`.
- **Command palette.** Ctrl+K, and F3 if it is free.
  - Fuzzy search over each Action's name, aliases and help.
  - Each row shows the chord and the menu path, with recently used actions first.
  - Enter runs the action and Esc closes. While the palette is open, keys go to its field.
- **History pane** (`Pane::History`).
  - Each undo step is a row with its label and author. The current point is highlighted and the saved point
    is marked (the never-lose-work part's `saved_rev`).
  - Clicking a row undoes or redoes to that point.
- **F1** opens a cheat sheet of every bound action, grouped and searchable.
- Every deck and tool button has a tooltip: name, chord and one line of help.
- Ctrl+Shift+Z redoes (Ctrl+Y stays).
- **Keymap presets** on the existing Settings ▸ Shortcuts page (`SettingsCategory::Shortcuts`, the default
  page; there is no Keys page): Gaban (today's defaults) and Industry. Industry uses the letters common to
  CSP, Toon Boom and Krita for the same verbs, e.g. B brush, E eraser, G fill. Space is the same in both: a
  tap plays or pauses and a drag pans (the pen aids part).
  - User overrides sit on top of the preset.
  - Conflicts are shown inline.

**Files and types to extend.**
- `app/src/config.rs`: on `Action` (line 14, `ALL` at line 59), add:
  - `help()`, `group() -> Group` (File, Edit, View, Sheet, Cut, Room, Window, Help, Tool, Playback);
  - `menu() -> Option<&'static str>` (e.g. "Edit/Undo") and `aliases()`;
  - `context() -> Ctx { Global, Canvas, Sheet, NodeGraph }` (D6); shortcuts dispatch by the pane under the
    pointer;
  - `Keymap { Gaban, Industry }` with `default_binds(keymap)`, plus `config.keymap`.
- New `app/src/menus.rs`: builds the bar from `Action::ALL` grouped by `menu()` and calls `Editor::perform`.
- New `app/src/cmdpal.rs`: `score(query, action) -> Option<i32>` (pure, tested), the palette UI and the
  recent list.
- New `app/src/history_panel.rs`, plus `Pane::History` in `app/src/workspace.rs`.
- `crates/anim-core/src/lib.rs`:
  - `Engine::history() -> Vec<HistoryEntry { label, author, applied: bool }>` (the undo stack, then the redo
    stack);
  - `Engine::position()` and `Engine::undo_to(position)`, which applies single steps.
- `app/src/main.rs`: wire the bar, the palette, F1, tooltips and Ctrl+Shift+Z.

**Acceptance criteria.**
1. Every Action has help of 1–120 characters, a group, and either a menu path or an explicit palette-only
   flag.
2. Neither keymap has a conflict: no two actions share a chord in the same context.
3. All 8 menus are present in the harness, and Edit ▸ Undo undoes (the history position drops by 1).
4. Redo with an empty redo stack is disabled, and its tooltip gives the reason.
5. Ctrl+K opens the palette. "und" ranks Undo first, Enter runs it and Esc closes. While the palette is open,
   pressing B types "b" and does not switch the tool.
6. Alias search: "onion" finds the onion toggle; a later part's verbs inherit the same mechanism.
7. After 5 gestures, clicking row 2 leaves 2 steps applied, and `content_hash` equals the hash recorded after
   step 2. Clicking row 5 restores all 5.
8. F1 lists every Action that has a chord.
9. Ctrl+Shift+Z performs Redo.
10. Industry then Gaban restores the exact Gaban bindings.
11. Hovering each deck tool button shows a tooltip that contains its chord.

**Tests to add.** At least 11 tests named `cmdui_*`, plus anim-core tests for `history`/`undo_to` in
`tests/engine_tests.rs`.

**Out of scope.** Macros, menu customisation and localisation.

**Stretch.** The palette also searches pane names and settings.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p animstudio cmdui -- --list | grep -c ': test$') && [ "$n" -ge 11 ]`

## GM4 Canvas view and pen aids

*Depends on: GM3. Cap 45 min. Filter word: `viewnav`. Binding decisions: D0, D6, D7, D10.*

**Goal.** Drawing feels like a modern drawing app: rotate or mirror the view, navigate by keys, size the
brush by keys, stabilise lines, draw straight lines and pick colours from the canvas.

**User-facing behaviour.**
- **View.** All of these are Actions in both keymaps:
  - rotate with Shift+Space+drag (hold Ctrl while rotating to snap to 15°), plus keyed rotate ±15° and reset
    rotation on chords free in both keymaps. R is not used: holding R is the existing flip (`flip_held`,
    canvas.rs 2740);
  - mirror horizontally (display only);
  - zoom in/out on a fixed ladder, 100%, and fit;
  - pan with Space+drag or the middle mouse. Space is today's PlayPause chord, so PlayPause moves from
    Space's press to its release: a tap (press and release with no drag) plays or pauses, and a drag pans
    without touching playback.
- Strokes, fills, the lasso and selections land at the right paper position under any rotation, mirror or
  zoom.
- Tablet tilt is rotated by the view angle, so tilt dynamics stay consistent.
- `[` and `]` step the brush size along a ladder. Hardness and an **aliased** toggle sit in the OPTIONS row;
  aliased gives hard 0/255 lines for fill-friendly clean-up.
- **Stabilizer** 0–10 in the OPTIONS row (pulled-string smoothing). 0 is raw. The stroke tail always catches
  up to pen-up.
- Shift+click after a stroke draws a straight segment from the last point.
- **Eyedropper:** Alt+click with the brush or fill picks the composited colour under the cursor into the
  current colour.

**Files and types to extend.**
- `app/src/canvas.rs`:
  - `ViewXform { zoom, pan, rotation_deg, mirror }` with `screen_to_paper` / `paper_to_screen`.
  - Paper images are drawn as a textured 4-vertex `egui::Mesh` through one helper, `paint_paper_image(...)`,
    in place of the axis-aligned `painter.image` calls. Those calls stack below/active/wet/above/remote
    textures at about lines 3230–3422, so all of them must move.
  - Every overlay (selection, lasso preview, onion) maps through `paper_to_screen`.
- **Stroke tap** in `canvas.rs`: `CanvasView::stroke_tap: Vec<StrokeSample { x, y, pressure, tilt }>`.
  - These are the paper-space samples the input pipeline produces after mapping and stabilising. They are
    cleared at stroke start and recorded even with no `PaintLayer` (no GPU), so tests can check the input
    path.
- `app/src/paint.rs`:
  - Hardness and aliased become uniforms.
  - A CPU mirror `dab_coverage(dist, radius, hardness, aliased) -> f32` documents and tests the shader
    formula.
  - The WGSL implements the same formula and validates with naga (the existing dev-dep).
- New `Stabilizer` in `app/src/canvas.rs`, or in its own module `app/src/stabilizer.rs` (pure).
- `app/src/config.rs`: the new Actions and their help; `pen.stabilizer: u8`; hardness and aliased in the
  brush preset (both serde default).

**Acceptance criteria.**
1. `screen_to_paper(paper_to_screen(p))` equals p within 1e-3 for rotations 0, 15, 90 and 137°, mirror on and
   off, and zoom 0.25, 1 and 3.7.
2. The same paper-space path dragged through the harness, once with the view rotated 90° and once unrotated,
   gives equal `stroke_tap` samples within 0.5 px.
3. Mirror changes only the display: the tap samples are identical with mirror on and off.
4. The zoom keys walk the pinned ladder. 100% maps 1 paper px to 1 screen point. Fit centres the paper.
5. With Space held, a drag pans, records no tap samples and leaves playback unchanged. Releasing Space
   restores the tool. A Space tap with no drag performs PlayPause exactly once. Holding R still sets
   `flip_held`.
6. `[`/`]` walk the size ladder. Hardness and aliased persist in the preset. With aliased on,
   `dab_coverage` is always 0 or 1. The WGSL validates.
7. Stabilizer 0 is the identity. At level 5, the mean absolute second difference of a seeded noisy path falls
   by at least 50%. The last output sample equals the pen-up point.
8. Shift+click after a stroke taps samples that all lie within 0.5 px of the straight segment.
9. The eyedropper's pick function returns the exact RGBA of a known pixel in a CPU-rendered frame. Alt+click
   sets the brush colour to it.
10. Every new verb is an Action with help, and there are no chord conflicts (the command surface part's test
    stays green).

**Tests to add.** At least 10 tests named `viewnav_*`.

**Out of scope.** Perspective rulers (the manga part), symmetry, vector lines, and brush engine changes beyond
hardness and aliased.

**Stretch.** Rotation with two-finger touch; a per-viewer flip.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p animstudio viewnav -- --list | grep -c ': test$') && [ "$n" -ge 10 ]`

## GM5 X-sheet engine: schema v11 and undoable sheet edits

*Depends on: GM3. Cap 45 min. Gate test file: `sheet_edit_tests`. Binding decisions: D0, D1, D2, D3, D4, D6.*

**Goal.** The X-sheet becomes a production tool: insert and delete frames, holds, retiming, typed entry, cut
length, copy and paste, column kinds and a memo/dialogue row, with every edit undoable. Schema v11.

**Split (review 2026-10-28).** This part builds the engine half: every verb below as Commands with exact
inverses, pure builders in anim-core, schema v11, `render_order()` in every render path, and Actions that
work on the sheet cursor (the current cell, or the current column from the cursor down) from the menus and
the palette. The sheet panel part (GM14, which runs next) builds the panel half: range selection, the
Ctrl+C/X/V keys, the typed-entry field, kind badges with hidden and locked toggles, the memo row, the cut
length field and `show_rows`.

**User-facing behaviour** (the panel bullets land in GM14).
- Select cell ranges by click, Shift+click or drag, across columns.
- Ctrl+C/X/V copy, cut and paste cells.
  - Within a cut, pasted cells reference the same drawings.
  - Across cuts, the drawings are copied with new ids.
  - Paste overwrites by default; Shift+paste inserts.
- **Typed entry.** A small entry field on the selected cell (a text field, so shortcuts are blocked).
  - Digits plus Enter expose the drawing with that name, creating a blank drawing if none exists.
  - `x` makes the cell Empty (カラ).
  - Enter moves down and Tab moves to the next column.
- **Hold.** +1 or −1 on the exposure under the cursor, rippling the column.
- **Retime** the selection to 1s, 2s or 3s: each distinct drawing in the range holds 1, 2 or 3 frames. The
  start of the range stays put, and what follows ripples.
- **Reverse** the selection: drawing order flips, and each exposure keeps its length.
- **Insert or delete frames** at the cursor, across every column (D4). The cut length changes.
- **Cut length field.** Shows frames plus "s+k"; changing it is undoable. Exposures beyond the end are kept,
  hidden, not deleted.
- **Column kinds** Cel (default), BG, Book (overlay held above cels) and Ref (a guide, never rendered), each
  shown as a badge.
  - A column can be **hidden** (excluded from every composite) and **locked** (paint and sheet edits refuse
    with Aka).
  - The column path renders in bands: BG, then Cel, then Book, each band in sheet order.
- **Memo/dialogue row.** Text per frame for セリフ and action notes, shown beside the sheet.
- Add, remove, rename and move for columns, param columns and cuts are now undoable.
- The sheet virtualises rows (`show_rows`), so a 600-frame cut scrolls smoothly.

**Files and types to extend.**
- `crates/anim-core/src/xsheet.rs`:
  - `Column` methods `insert_frames`, `delete_frames`, `hold_delta`, `retime`, `reverse`, plus the same on
    `ParamColumn`;
  - `ColumnKind { Cel, Bg, Book, Ref }` and `ColumnDef { kind, hidden, locked }` (default Cel, false, false).
- `crates/anim-core/src/model.rs`:
  - `Cut.meta: BTreeMap<String,String>` (D3), `Cut.memos: BTreeMap<u32,String>` and `Cut.column_defs`.
  - `Cut::render_order() -> Vec<ColumnId>`: BG, then Cel, then Book; hidden and Ref columns skipped. It is
    used by the column path in export.rs, by eval.rs, and by the canvas below/above stacks and graphcomp. A
    DrawingSource on a hidden or Ref column yields transparent in every path.
- `crates/anim-core/src/command.rs`: new Commands, each with an exact inverse.
  - Columns: Add/RemoveColumn (with a snapshot of keys and def), RenameColumn, MoveColumn.
  - Param columns: Add/RemoveParamColumn.
  - Cuts: AddCut/RemoveCut (snapshot of the whole cut), RenameCut.
  - Frames: SetFrameCount, InsertFrames, and DeleteFrames (with a snapshot of the removed keys, param keys
    and memos).
  - Cells: SetCells (a batch of (column, frame, before, after)).
  - SetCutMeta, SetColumnDef, SetMemo.
- `crates/anim-core/src/store.rs`: `SCHEMA_VERSION = 11`, the side tables `cut_meta`, `column_defs` and
  `cut_memos`, and the version handling in D2.
- New `crates/anim-core/src/sheet_ops.rs` (pure builders that return `Vec<Command>`): `expose_by_name`,
  `copy_block`, `paste_block(block, target, PasteMode::{Overwrite, Insert})`, `retime_range` and
  `reverse_range`. The panel part calls these; it adds no edit logic of its own.
- `app/src/doc.rs`: scaffolding goes through Commands (today `add_param_column` 1818, `remove_active_column`
  1860 and `new_cut` 2004 call the engine's non-undoable `add_column`/`add_param_column`/`remove_column`/
  `add_cut`, lib.rs 112-184).
- `app/src/xsheet_panel.rs`: only its existing add/remove buttons change, to go through the Commands. The
  panel UI is GM14.
- `app/src/canvas.rs`: the below/above stacks use `render_order()`.
- `app/src/config.rs`: the Actions with help and aliases ("twos", "ones", "threes", "hold", "insert frames").

**Acceptance criteria.**
1. Inverse fuzz: 200 seeded random sheet Commands, then undoing all of them, restores `content_hash` and full
   structural equality.
2. Inserting 3 frames at f=10 in a 48-frame cut gives frame count 51. Every column's exposures and every param
   key from f=10 on move by +3, and so do the memos. Deleting the 3 frames restores the original exactly.
3. Retiming a range to 2s makes each distinct drawing hold 2 frames, keeps the range start and ripples what
   follows.
4. Hold +1 extends the exposure by one frame and ripples. Hold −1 on a 1-frame exposure removes it and
   ripples.
5. Reverse flips the drawing order and keeps each exposure's length.
6. `expose_by_name(.., "3")` exposes the drawing named "3", creating it when absent, in one undo step.
   `"x"` makes the cell Empty.
7. `paste_block` of a 3×5 block reproduces its exposures at the target. Pasting across cuts copies the
   drawings (new ids, same tile hashes) in one undo step. Insert mode ripples what follows.
8. Ref and hidden columns are absent from the CPU column render, the graph render and the eval hash. BG and
   Book band order holds in the column render. Legacy cuts, where every column is Cel, render byte-identical
   to before.
9. Remove column then undo restores the column's id, keys and def. Add cut then undo works.
10. Store:
    - A v11 round trip keeps all new data.
    - A v9 fixture loads with empty meta.
    - A v10 file loads and re-saves with its extra table kept. The test builds it from a v9 file plus a
      `cel_layer_groups` table and `schema_version` 10.
    - A v12 file is refused with the "newer Gaban Studio" message.
11. Memo text round-trips and is undoable.
12. A locked column refuses SetCells and paint, with the refusal reason.

**Tests to add.** At least 16 tests in `crates/anim-core/tests/sheet_edit_tests.rs`. The `sheetui_*` app
tests belong to GM14.

**Out of scope.** Camera columns (the camera part); XDTS export of the new kinds (XDTS keeps working, and the
new kinds export as plain cell columns); drag-to-move exposure blocks.

**Stretch.** Drag a selected block to move it; colouring rows by drawing kind (once the light table part
lands).

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test sheet_edit_tests -- --list | grep -c ': test$') && [ "$n" -ge 16 ]`

## GM6 Light table and inbetweening

*Depends on: GM4, GM5, GM14. Cap 45 min. Filter word: `lighttable`. Binding decisions: D0, D1, D2, D6, D7, D10.*

**Goal.** The genga-to-douga workflow: mark keys, breakdowns and inbetweens and see them on the sheet; onion
skin with depth and modes; a light table with pins and shift & trace; spacing charts that generate inbetween
drawings; duplicate cel; and a pixel clipboard.

**User-facing behaviour.**
- **Drawing kind.** Key (原画), Breakdown or Inbetween (動画), set from the cell's context menu or the canvas
  header.
  - On the sheet, a key's number is circled and a breakdown's is underlined.
  - Kind never changes pixels.
- **Onion.** Before and after depths of 0–5 each, with three modes:
  - drawings (the previous and next distinct drawings in the column);
  - frames (fixed frame offsets);
  - keys (the previous and next Key drawings).
  - Further options: an "other columns" toggle, and opacity with falloff. The tints stay today's: Ao
    (160,195,232) behind and Legend (205,206,199) ahead (canvas.rs 3352-3385). Aka is never an onion tint,
    because D7 keeps Aka for refusals.
  - `onion_set` extends today's `onion_neighbors` (doc.rs 1240, which returns `[Option<DrawingId>; 2]`) and
    feeds both of today's ghost paths: the GPU slots (canvas.rs 3170-3190) and the tinted `onion_ghosts`
    path (canvas.rs 3305). At depth 1 in drawings mode it returns exactly what `onion_neighbors` returns now.
    The R flip (canvas.rs 2737-2744 and 3357) keeps borrowing slot 0 and is unchanged.
  - **GPU budget.** The PaintLayer has exactly two onion slots (`onion: [Option<OnionSlot>; 2]`, paint.rs
    163). Each side's ghosts are composited with their weights into that side's existing slot, so depth 5
    adds no textures. The light table's pins get one added slot, which ends today's reuse of slot 1 by the
    ghost pin. That makes three paper-size textures at most, never one per ghost.
- **Light table.** Pin any drawings (column, drawing) so they show at a fixed opacity on every frame. The
  pins generalise today's single `state.view.ghost_pin: Option<DrawingId>` (doc.rs 87, drawn at canvas.rs
  3287), which becomes the first pin, so the existing ghost-pin control keeps working.
  - Shift & trace: with the light table's "shift & trace" latch on, a drag moves the selected pinned ghost and
    Shift+drag rotates it, for view only; with the latch off the pen draws again. A Reset clears the offsets.
    View state stays in memory per cut; it is not document data.
- **Spacing chart** (タイミング指示). On a key, a chart toward the next key: marks at positions in 0..1 such as
  halves, thirds or favouring one key. It is drawn as the classic chart glyph on the canvas and the sheet.
  - **Make inbetweens** turns a chart's n marks between keys A and B into n new blank Inbetween drawings in
    one undo step.
  - Names: the free integers strictly between the key numbers when there are enough; otherwise the from-key
    name plus a letter (`1a`, `1b`, ...).
  - Placement: the span between the keys' exposure starts is split into n+1 equal steps, rounded, each at
    least 1 frame. Each inbetween holds until the next.
- **Duplicate cel.** Copies the drawing and its layer stack under the next free number, in one undo step.
- **Pixel clipboard.** Ctrl+C/Ctrl+V on a selection copies the active layer's pixels. Paste lands in place
  (the same paper position) in another drawing or frame, in one undo step. The clipboard follows context
  (D6): over the canvas with a selection, Ctrl+C/V act on pixels; in the sheet they act on cells (GM14).
  Moving the pasted pixels as a floating selection is Stretch.

**Files and types to extend.**
- `crates/anim-core/src/model.rs`:
  - `DrawingKind { Unset, Key, Breakdown, Inbetween }`;
  - `DrawingDef { kind, chart: Option<SpacingChart { to: DrawingId, marks: Vec<f32> }> }`, stored in the side
    table `drawing_defs(drawing_id, json)` (D2) and excluded from `content_hash`.
- `crates/anim-core/src/command.rs`: `SetDrawingDef { at, drawing, before, after }`.
- `crates/anim-core/src/xsheet.rs`: `prev_key`/`next_key` helpers.
- `crates/anim-core/src/edit.rs`: `copy_region(drawing, layer, polygon) -> ClipImage` and
  `paste_region(...) -> Vec<Command>`, both through the existing `commit_region_edit` path.
- `app/src/canvas.rs`:
  - `OnionCfg { before: u8, after: u8, mode, other_columns, opacity, falloff }`;
  - `onion_set(cut, column, frame, &OnionCfg) -> Vec<(DrawingId, i32 offset, f32 weight)>` (pure);
  - `LightTable { pins, offsets }`;
  - drawing the chart glyph.
- `app/src/xsheet_panel.rs`: kind marks and the kind menu.
- `app/src/doc.rs`: `make_inbetweens(chart) -> Vec<Command>`, `duplicate_cel()` and the clipboard.
- `app/src/config.rs`: the onion config and the new Actions.

**Acceptance criteria.**
1. DrawingDef round-trips through save and load, and `content_hash` is unchanged by kind and chart (pinned).
2. `cell_mark(kind)` returns Circle, Underline or None. The harness sees a key cell's accessible label contain
   "key".
3. `onion_set` is right for each mode at depths 0, 1 and 5, both directions, and with other columns on and
   off. Depth 0 gives an empty set. At depth 1 in drawings mode it equals today's `onion_neighbors` for
   every frame of a fixture cut. The tint constants are unchanged.
4. Light-table pins appear on every frame. Pins and shift offsets never change `content_hash`.
5. A chart with 3 marks between keys "1" and "5" makes drawings "2", "3" and "4" of kind Inbetween. They are
   exposed at the computed frames and one undo removes all of it. Between keys "1" and "2" the names are
   "1a", "1b" and "1c".
6. Duplicate cel gives a new drawing with identical tile hashes and the next free number, in one undo step.
7. Copying a selection from drawing A and pasting it into drawing B makes B's tiles in the region equal A's.
   This works across frames and is one undo step.
8. The onion and light-table settings persist in the config. Shift offsets do not.

**Tests to add.** At least 12 tests named `lighttable_*` (pure functions, doc operations and the harness),
plus store round-trip tests for `drawing_defs` in anim-core's `tests/layer_tests.rs`, written as new
functions.

**Out of scope.** Auto-inbetweening (AI or vector morph); camera-aware onion.

**Stretch.** Rolling over more than one drawing (holding R already flips, canvas.rs 2740); a floating move of
pasted pixels; configurable onion tints.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p animstudio lighttable -- --list | grep -c ': test$') && [ "$n" -ge 12 ]`

## GM7 Ink and paint: exact colour, recolour, paint checks

*Depends on: GM4, GM5. Cap 45 min. Gate test file: `color_tests`; app filter word `inkpaint`. Binding decisions: D0, D1, D8, D9; NEVER-DO 3.*

**Goal.** Shiage gets exact colour: what you pick is what exports. Studios get recolour by chip edit, and
automated paint checks.

**User-facing behaviour.**
- **Colour law** (D8). Projects made by the New Project dialog store chips exactly, and their export bytes
  equal the chip bytes. Old projects render byte-identical to v0.3.
- **Project ▸ Convert colours to sRGB…** For a saved Legacy project, a DANGER confirm explains the steps:
  1. a backup goes to `<stem>.pre-srgb.animproj`;
  2. every tile value is remapped so flat chips export as the chip;
  3. history is cleared;
  4. the document becomes dirty.
- **Recolour by chip edit.** When a palette chip's colour changes, the panel offers "Recolour paint" with
  scope this cut / this scene / project.
  - Every pixel exactly equal to the old chip (stored value, opaque) in the Color, Shadow, Highlight and
    Custom roles becomes the new chip.
  - Line roles are never touched.
  - It is one undo step.
- **Refill.** Shift+click with Fill replaces the clicked 4-connected region of one exact colour, bounded by
  the line, with the current colour. Ctrl+Shift+click replaces that colour throughout the drawing's layer.
- **Chip pick.** Alt+click (the pen aids part's eyedropper): when the colour exactly matches a chip of the
  active palette, that chip becomes active and highlights.
- **Paint check pane** (`Pane::PaintCheck`). Scans a cut or frame range on a worker for two kinds of finding:
  - **holes**: transparent regions enclosed by line art, at least `min_px` (default 4) in area;
  - **stray colours**: paint-role pixels whose colour is not a chip of the column's palette.
  - Each finding gives frame, drawing, layer, bbox and pixel count. A click jumps to it (sets the frame and
    zooms to the bbox). The pane shows a summary count.

**Files and types to extend.**
- New `crates/anim-core/src/color.rs`:
  - `ColorLaw`, `encode` and `decode`;
  - `convert_legacy_to_srgb(project)`;
  - `recolour(project, scope, from, to) -> Vec<Command>`;
  - `refill`/`replace_all` in or beside `fill.rs`;
  - `paint_check(cut, range, chips, min_px) -> Vec<Finding>`.
- `crates/anim-core/src/model.rs` / `store.rs`: `Project.color_law`, stored in `meta.color_law` (missing
  means Legacy).
- **Every place that linearises a chip** goes through `ColorLaw::encode`. At 56679ad `grep -rn
  "srgb_to_linear\|linear_rgba"` finds exactly these (re-run it at the part's start):
  - `canvas.rs` `linear_rgba` (5761), called at 4386 (the brush dab colour), 4795 (lasso fill) and 4980
    (flood fill);
  - `graphcomp.rs` Solid upload (493-497);
  - `export.rs` `render_node` Solid (308-310). `srgb_to_linear` itself is defined at export.rs 141.
  - `fill.rs`, `paint.rs` and image import do not linearise: paint.rs receives the colour already encoded
    at canvas.rs 4386. `graph_render_tests.rs` calls `srgb_to_linear` in its expectations, which stay
    Legacy.
- `app/src/palette_panel.rs`: the Recolour offer.
- `app/src/canvas.rs`: Shift and Ctrl+Shift fill, and the Alt chip pick.
- New `app/src/paintcheck.rs` (the pane), `Pane::PaintCheck` in `workspace.rs`, and the Convert command in
  `main.rs` (backup into the document's folder).
- `app/src/newproject.rs`: new projects use Srgb.

**Acceptance criteria.**
1. Under Srgb, `decode(encode(v)) == v` for all 256 byte values at alpha 255 (through f16 storage).
2. Under Srgb, a filled region with chip c exports as exactly c (CPU render). The display path uses the same
   stored values: one shared encode, asserted by a test that the canvas and graphcomp call sites use it.
3. Under Legacy, every existing golden test passes untouched, and a Legacy project's export hash is unchanged.
4. Convert: a Legacy project's flat chip pixels export within 1 of the chip after conversion. The backup file
   exists and loads, and history is empty afterwards.
5. Recolour of c1 to c2 at each scope changes exactly the matching paint-role pixels. Line pixels and
   non-matching pixels are byte-identical, and it is one undo step.
6. Refill changes only the clicked connected region. Replace-all changes every exact match in the layer and
   nothing else.
7. Chip pick selects the matching chip, and a non-chip colour selects no chip.
8. A synthetic drawing with 2 enclosed holes and 1 stray colour gives exactly 3 findings with correct bboxes.
   A clean drawing gives 0.
9. The paint check runs on a worker, and its UI never blocks: the harness runs frames while it is pending.

**Tests to add.** At least 12 tests in `crates/anim-core/tests/color_tests.rs`, plus at least 5 app tests named
`inkpaint_*`: the convert flow with a backup in a temp dir, chip pick, the paint-check list and jump, and the
recolour history entry. The gate checks both counts.

**Out of scope.** Colour-pencil line separation and scan clean-up; ICC/OCIO colour management; AI colour.

**Stretch.** A "show unpainted" overlay on the canvas; palette import and export as JSON.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test color_tests -- --list | grep -c ': test$') && [ "$n" -ge 12 ] && n=$(cargo test -q -p animstudio inkpaint -- --list | grep -c ': test$') && [ "$n" -ge 5 ]`

## GM8 Welcome, templates, rooms and the sample cut

*Depends on: GM3, GM5, GM6, GM7. Cap 45 min. Filter word: `welcome`. Binding decisions: D0, D7, D8, D10, D12, D13.*

**Goal.** Someone who opens Gaban Studio for the first time knows what to do. This part adds a welcome screen
with templates, recent files and a sample cut, a first-steps checklist, the five spine rooms (Finishing
splits into Ink & Paint and Composite, D12) with room cards that explain each stage, Project Settings, and
UI scale. (Review 2026-10-28: node positions moved to the FX core part, the room split moved here from the
satsuei part, the checklist moved up from Stretch, and the role chooser moved down to Stretch.)

**User-facing behaviour.**
- **Welcome screen** in the central area. It shows when no document was opened by argument, by devloop
  restore or by recovery. It offers:
  - New from template: cards with name, size, fps and length;
  - Open…;
  - Recent files: 12 cards with thumbnails, missing files greyed with Remove;
  - Open the sample cut;
  - a "Show at startup" latch.
- **Templates** are data. Each is created with the Srgb law, `default_cut_frames`, the first room, the
  starting columns, the room cel-stack presets and an auto-comp graph.

  | Template | Size | fps | Cut | First room | Columns |
  |---|---|---|---|---|---|
  | TV anime HD | 1920×1080 | 24 | 72 | layout | BG, A, B, C |
  | Web 720p | 1280×720 | 24 | 72 | genga | BG, A, B |
  | Feature UHD | 3840×2160 | 24 | 72 | layout | BG, A, B, C, D |
  | Animatic / storyboard | 1920×1080 | 24 | 120 | henshū | BOARD (BG band: the boards render and export), A |

  The manga part adds Manga B5, Manga A4 and Webtoon.
- **Auto-comp: "Build composite from sheet"** creates a DrawingSource for each rendered column in
  `render_order()`, chained with Normal Blends into the Output. One undo step.
- **Rooms (D12).** `Stage::Finishing` becomes `InkPaint` ("shiage (ink & paint)", key still `"finishing"`),
  and `Stage::Composite` ("satsuei (composite)", key `"composite"`) is added after it.
  - Its dock (`Stage::dock`, workspace.rs 130) puts the node graph large, with the viewer and the palette
    beside it, and the Curves pane (GM14's graph editor) as a tab beside the node graph: no room docked it
    in GM14 (Window ▸ Curves opens it anywhere), and camera moves are timed here. Its verb (`Stage::view`,
    line 179) turns composite view on.
  - `name` and `describes` split the old "shiage + satsuei" text between the two rooms.
  - `default_room_stacks` (config.rs 837, which maps a room key to a layer-stack preset) binds `"composite"`
    to `"Shiage (full)"`, and the Layers page's room list (config.rs 2130-2136) gains a row.
- **First steps.** A Quick start card on the welcome screen, also opened by Help ▸ Quick start, lists five
  steps: draw a drawing, mark it Key, make an inbetween, play, export. Each ticks the first time its Action
  runs, kept in `config.ui.quick_start`, and each row has a "Show me" button that performs the step's
  Action or switches to its room.
- **Project Settings** window: name, resolution (with aspect), fps, default cut length, dpi, and the colour law
  (read-only, with the Convert button). It is undoable through `Command::SetProjectProps { before, after }`. A
  resolution change keeps all pixels and rebuilds the canvas layers.
- **Room cards.** The room switcher shows each room's term, a one-line purpose and the tools it arms.
- **Sample cut**, built procedurally with no binary assets:
  - a BG gradient with ground;
  - a 24-frame ball bounce on twos, with the keys marked and inbetweens;
  - a palette with line and paint roles.
  - It opens untitled; Save As keeps it. The ship part adds camera and FX.
- **UI scale** in Settings ▸ General: 80–200%, applied through egui's zoom factor and persisted.
- **Thumbnails.** Every save renders the current cut's frame 0 on the save worker (CPU), 256 px wide, into
  `app_meta["thumb"]` as base64 PNG of at most 64 KB. `store::read_meta(path)` reads only the meta table for
  the welcome cards.

**Files and types to extend.**
- New `app/src/welcome.rs`, `app/src/templates.rs` (a `Template` struct and `ALL`) and `app/src/sample.rs`
  (`build_sample() -> AppState`, CPU rasterising into tiles).
- `app/src/newproject.rs` reuses the template data.
- `app/src/workspace.rs`: `Stage::InkPaint` and `Stage::Composite` (Stage at line 86; `ALL`, `name`,
  `describes`, `dock` and `view` at 103-205), and room cards.
- `app/src/main.rs`: `stage_key` (line 53) for the new room.
- `app/src/doc.rs`: `build_auto_comp(cut) -> Vec<Command>`.
- `crates/anim-core/src/command.rs` (`SetProjectProps`) and `store.rs` (`read_meta`).
- `app/src/config.rs`: `ui.scale`, `ui.show_welcome`, `ui.quick_start`, the `"composite"` layer-stack
  default (line 837) and the Layers page row (2130-2136).
- `app/src/main.rs`: start-up routing.

**Acceptance criteria.**
1. A fresh harness with no document shows the Welcome screen: labels for New, Open, Sample cut and every
   template name.
2. Each template card makes a project with the template's size, fps, cut length, Srgb law, first room and
   columns, and with a wired graph Output.
3. Auto-comp parity: for the sample cut at frames 0, 7 and 23, the graph render equals the column render
   byte-for-byte (CPU).
4. Recent: entries come from the config; a missing file is flagged; Remove drops it.
5. A thumbnail is written on save, decodes to 256 px wide, is at most 64 KB, and `read_meta` returns it
   without loading drawings.
6. The sample has at least 12 drawings with keys marked Key, and renders a non-empty frame 0. (No wall-clock
   assertion: D0 bans clocks in tests. The D0 2-second test limit still applies.)
7. `SetProjectProps` has an exact inverse, and the harness Settings window edits fps in one undo step.
8. Rooms: a config whose `room_stack` binds `"finishing"` to a layer stack X loads with InkPaint using X,
   and Composite gets `"Shiage (full)"`. `Stage::ALL` (workspace.rs 103, four stages today) lists the five
   spine rooms in order (Layout, Drawing, InkPaint, Composite, Edit). Entering Composite shows the node graph
   and the viewer, and the layout probe passes in that room.
9. A UI scale of 150% sets the zoom factor, and the layout probe passes at that scale.
10. Quick start: performing NewDrawing ticks step 1 and nothing else; the ticks survive a config round trip;
    "Show me" on step 4 performs PlayPause.

**Tests to add.** At least 10 tests named `welcome_*`.

**Out of scope.** Online templates and tutorials/video.

**Stretch.** A first-run role chooser (animator, painter, compositor, layout, director) that sets the first
room and workspace, shown once and saved in the config.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p animstudio welcome -- --list | grep -c ': test$') && [ "$n" -ge 10 ]`

## GM9 Compositing FX core: blur, glow, diffusion, grade

*Depends on: GM5. Cap 45 min. Gate test file: `fx_tests`; app filter word `fxnode`. Binding decisions: D0, D1, D3, D7, D9, D16; NEVER-DO 1 and 2.*

**Goal.** The node graph gets real satsuei effects, rendered by one CPU implementation that the composite
view, the viewer and export all use: blur, glow (T光), diffusion (透過光 and the soft diffusion filter) and
grade. They are built on a parameter-spec system, so later parts add FX by declaring specs.

**User-facing behaviour.**
- An **Effect** node with an FX kind:
  - Blur: radius X and Y.
  - Glow: threshold, radius, intensity, tint, and an optional matte input.
  - Diffusion: radius, mix, and mode screen or soft light.
  - Grade: lift, gamma, gain, saturation, exposure, temperature and tint.
- Every parameter can be bound to an X-sheet param column so it animates, as Transform binds do.
- **Searchable add-node menu** (Tab or right-click on empty graph space). Typing filters it, and categories
  are Source, Transform, Blend, FX and Output (later parts add Generators and 3D). The node is placed at the
  pointer, in one undo step.
- A **generated inspector** comes from each kind's specs:
  - Float: a drag value with min and max.
  - Color: a plate colour button.
  - Bool: a latch.
  - Choice: a combo.
  - Each parameter has a bind picker. A drag is one undo step.
- In the composite view, FX subtrees render on a worker, and a TALLY dot shows while a render is pending. The
  pen is never blocked.
- **Node positions persist** (moved here from the welcome part). Today `node_positions` is session-only
  (doc.rs 150). They are saved in `cut_meta["graph.layout"]` through `SetCutMeta`, with one merged entry per
  drag.

**Files and types to extend.**
- New `crates/anim-core/src/fx.rs`:
  - `Image { w, h, px: Vec<f32> /* premultiplied RGBA */ }`;
  - `FxKind`;
  - `ParamSpec { key, label, help, kind: ParamKind }` with `ParamKind { Float{min,max,default,step},
    Color{default}, Bool{default}, Choice{options,default} }` (later parts add Curve, Text, Blob and Polys);
  - `FxParams` (serde map; missing keys take the spec default) and `FxBinds` (key to ParamId);
  - `FxKind::{specs, inputs, apply}`, with rayon (new dep).
- `crates/anim-core/src/graph.rs`: `NodeKind::Effect { fx, params, binds }`, plus `input_count`, `name` and
  `references(param)`.
- `crates/anim-core/src/export.rs` `render_node` (line 265): the Effect arm.
- `eval.rs`: the hash includes kind, params and resolved bound values.
- `store.rs`: nothing new, since nodes are JSON of NodeKind.
- `app/src/graphcomp.rs`:
  - `needs_cpu(&NodeKind)` and `subtree_needs_cpu`;
  - a worker that renders from a `Cut` snapshot;
  - an upload cache keyed by (node, frame, eval hash).
- `app/src/nodegraph_panel.rs`: the add-node menu, and node positions read from and written to
  `graph.layout`.
- New `app/src/fxui.rs`: the inspector from specs.

**Formulas (pin them in code comments and tests).**
- Blur radius r means a Gaussian with σ = r/2, done as three separable box passes (boxes-for-Gauss widths).
  Work is O(pixels) per pass, independent of r. r = 0 is the identity. Outside the image is transparent.
- Glow:
  - `bright = max(luma(unpremult) − threshold, 0)/(1 − threshold)`, which then multiplies the colour and the
    tint;
  - `glow = blur(bright, radius) × intensity`;
  - `out = input + glow` (Add, clamped only at the finishers).
- Diffusion: `d = blur(input, radius)`; `out = mix(input, screen(input, d), mix)`. Soft light uses the
  existing Overlay-style curve.
- Grade works on unpremultiplied colour:
  - `c = pow(max(0, gain·(c + lift·(1−c))), 1/gamma) · 2^exposure`;
  - saturation mixes with Rec.709 luma;
  - temperature and tint are per-channel gains;
  - then it premultiplies again.

**Acceptance criteria.**
1. Blur of a single white pixel is symmetric and energy-preserving (sum within 1e-3), and r = 0 is the
   identity.
2. Glow: an image with nothing above the threshold is unchanged. A bright dot gains a halo with monotonic
   falloff. Intensity 0 is the identity.
3. Diffusion mix 0 is the identity. Mix 1 matches the pinned values on a ramp image.
4. Neutral Grade parameters are the identity on an RGBA8 round trip. Saturation 0 gives grey. Exposure +1
   matches the formula.
5. A parameter bound to a param column with keys 0 and 10 gives different, formula-exact results on frames 0
   and 10.
6. The eval hash changes when a parameter or bound value changes and is stable otherwise.
7. An Effect node round-trips through the store, and old files load unchanged.
8. `subtree_needs_cpu` is true above an Effect and false for a plain graph. The cache key changes with the
   eval hash. No GPU device is needed for these tests.
9. In the harness, typing "glo" in the add-node menu and pressing Enter adds a Glow node in one undo step.
10. In the inspector, one drag on a Float parameter is one history entry.
11. Node positions survive save and load, and a node drag is one history entry.

**Tests to add.** At least 12 tests in `crates/anim-core/tests/fx_tests.rs`, plus at least 4 app tests named
`fxnode_*`. The gate checks both counts.

**Out of scope.** WGSL versions; motion blur by sub-frames; optical flow.

**Stretch.** A glow driven only by the matte input. (Directional motion blur is now core in the satsuei
part.)

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test fx_tests -- --list | grep -c ': test$') && [ "$n" -ge 12 ] && n=$(cargo test -q -p animstudio fxnode -- --list | grep -c ': test$') && [ "$n" -ge 4 ]`

## GM10 Camera, paper and multiplane

*Depends on: GM5, GM9, GM14. Cap 45 min. Gate test file: `camera_tests`; app filter word `camview`. Binding decisions: D0, D1, D3, D9; NEVER-DO 1, 2 and 4.*

**Goal.** Layout and satsuei camera work:
- a cut's paper can be larger than the frame, for BG pans and sliding cels (引き);
- a Camera node with pan, T.U./T.B. zoom, rotate and shake, keyed on the sheet;
- multiplane depth with parallax and depth of field;
- field guide and safe frames.

**User-facing behaviour.**
- **Paper.** Cut ▸ Paper size sets `cut_meta["camera.paper"] = {"x":0,"y":0,"w":3000,"h":1080}`. This is a
  rect in **frame coordinates**; the default is the frame itself.
  - The canvas shows the paper with the frame outline inside it, and BG columns can be painted anywhere on
    the paper (tile coordinates are i32, so they may be negative).
  - The GPU layers are created at paper size and rebuilt on a cut switch when the size differs, capped by
    `max_texture_dimension_2d`. The cap is shown.
  - Fill and lasso clip to the paper.
- **Camera node.** Its inputs are planes (one pin per plane, each with a depth z). Its output is the
  frame-size image as the camera sees it.
  - Parameters: `pan_x`, `pan_y` (paper px), `zoom` (1 is the frame, above 1 is T.U., below 1 is T.B.),
    `rotate_deg`, `shake_amp`, `shake_freq`, `shake_seed`, `focus_z`, `aperture` and `parallax_k`.
  - Every parameter can be bound to a param column. The inspector comes from specs (the FX core part).
- **Multiplane.** Each plane z gets a parallax factor `f = 1/(1 + k·z)`: farther planes (z > 0) move less, and
  near planes (z < 0) move more. Depth of field blurs each plane with radius `aperture·|z − focus_z|` px,
  using the FX core blur.
- **Ease.** EaseIn and EaseOut come from the sheet panel part (GM14), which runs before this one.
- **Today's camera keeps working.** A "camera" today is a Transform node bound to `cam.x`, `cam.y` and
  `cam.scale` param columns (nodegraph_panel.rs 438-440). Those graphs render byte-identically. The Camera
  node is new and does not migrate them.
- **Field guide.** The canvas already has field, safe and peg toggles (`show_field`/`show_safe`/`show_peg`,
  canvas.rs 524-526, toggled at 2045-2053, drawn at 2817-2836). Extend those, do not add new ones: their
  rects come from `camera::field_guide` and `safe_frames` (a 12-field guide where field n is n/12 of the
  frame, centred, with labels and a centre cross; action safe 93%, title safe 90%), they are drawn on the
  camera frame inside the paper, and the viewer gets the same safe-frame overlay.
- In the Layout room, the canvas draws the camera frame rect for the current frame on the paper.

**Files and types to extend.**
- New `crates/anim-core/src/camera.rs` (pure):
  - `paper(cut) -> IRect`;
  - `CameraState::at(cut, node, frame)`;
  - `frame_rect_on_paper(state)`;
  - `sample_map(u, v) -> paper point`, where p = C + R(θ)·((u − W/2, v − H/2)/zoom) + shake(t) and C = (W/2
    + pan_x·f, H/2 + pan_y·f);
  - `shake(seed, t, amp, freq)`, made from deterministic sums of hashed-phase sines;
  - `dof_radius`;
  - `field_guide(w, h) -> Vec<Rect>` and `safe_frames`.
- `crates/anim-core/src/graph.rs`: `NodeKind::Camera { planes: Vec<PlaneSpec { depth }>, params, binds }`.
- `crates/anim-core/src/export.rs`: render sources into an arbitrary rect, `render_node_rect(cut, node, frame,
  rect)`.
  - `splat_flat` (line 28) takes a rect offset.
  - With no camera and the default paper, the output is exactly today's.
  - The camera's resampling reuses `transform_image`'s bilinear sampler (line 199). It is not a second
    sampler.
- `app/src/canvas.rs` (paper, frame outline, the existing guide toggles) and `app/src/paint.rs` (layer size
  per cut paper).
- `app/src/graphcomp.rs`: Camera is `needs_cpu`.
- `app/src/viewer.rs`: safe-frame overlay.

**Acceptance criteria.**
1. With the default paper and no camera, every existing `graph_render_tests` and `image_tests` case passes
   untouched.
2. A 3000×1080 paper with a painted BG and a pan keyed from 0 to 1080 px over 48 frames: frame 0 equals the
   paper crop at x = 0, and frame 47 equals the crop at x = 1080, pixel-exact.
3. Zoom 2 shows the centre half scaled 2× and matches the bilinear `transform_image` reference within 1/255.
4. Rotate 90° maps a marker pixel to the expected position.
5. Shake with the same seed is identical; amplitude 0 gives no offset; offsets never exceed the amplitude.
6. Two planes at z 0 and 2 with k 0.5 under a 100 px pan move 100 and 50 px.
7. A plane at `focus_z` gets no blur. A plane off focus matches the blur formula. Aperture 0 gives no blur.
8. A graph whose Transform node is bound to `cam.x`/`cam.y`/`cam.scale` param columns renders
   byte-identically to the same graph rendered before this part (a pinned hash).
9. `field_guide` and `safe_frames` rects are pinned for 1920×1080, and the canvas's existing `show_field` and
   `show_safe` toggles draw exactly those rects.
10. `needs_cpu(Camera)` is true.
11. A tile painted at negative coordinates on an extended paper renders through the camera at the expected
    place.

**Tests to add.** At least 12 tests in `crates/anim-core/tests/camera_tests.rs`, plus at least 3 app tests
named `camview_*`: the paper outline rect, the camera rect on the paper, and the guide toggles. The gate
checks both counts.

**Out of scope.** The 3D camera (the 3D part); camera motion blur.

**Stretch.** The camera path overlay (moved here from the graph editor part): a View toggle, on by default in
the Layout and Composite rooms, drawing a polyline through the camera centre per frame with key dots.
Dragging a dot keys `pan_x` and `pan_y` at that frame through GM14's `SetParamKeys`, and an unbound camera
gets "cam pan x" and "cam pan y" columns created and bound in the same undo step. Its test: the polyline
equals `camera.rs` centres per frame, and a drag at frame k changes only the pan keys at k. Then camera
motion blur from sub-frame samples.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test camera_tests -- --list | grep -c ': test$') && [ "$n" -ge 12 ] && n=$(cargo test -q -p animstudio camview -- --list | grep -c ': test$') && [ "$n" -ge 3 ]`

## GM11 Render queue and delivery

*Depends on: GM7, GM8, GM10. Cap 45 min. Gate test file: `delivery_tests`; app filter word `render_queue`. Binding decisions: D0, D8, D9, D11, D16; NEVER-DO 6 and 7.*

**Goal.** Finish the film: a render queue with delivery presets; image sequences (PNG16, TIFF16, EXR); video
through ffmpeg when it is present, with today's audio mux kept; burn-ins; line-test renders; an explicit
final-render role policy; and a batch render from the command line. All of it runs on a worker. (Review
2026-10-28: episode export, the EDL and the colour designation sheet moved to the cut board part.)

**User-facing behaviour.**
- **Render Queue pane** (`Pane::RenderQueue`).
  - Add jobs: the current cut, selected cuts or a frame range. (The cut board part adds the episode job.)
  - The pane is tabbed into the Composite and Edit rooms' docks (`Stage::dock`, workspace.rs 130).
  - Each job has a preset, an output folder and a pattern (`{project}_{scene}_{cut}_{frame:04}`).
  - Start and Stop; progress per job; finished jobs offer "show folder".
  - Each job renders from a snapshot taken at its start, so later edits do not leak in.
- **Presets** (data):
  - Master PNG 16-bit; TIFF 16-bit; EXR half (linear, D8); PNG 8-bit;
  - H.264 review (MP4 with burn-ins); H.265; ProRes 422 HQ; ProRes 4444 (alpha); DNxHR HQ;
  - Line test.
  - Each preset sets alpha, burn-ins, purpose and scale (100% or 50%).
- **ffmpeg.** Extend the existing lookup; do not write a second one. Today `find_ffmpeg` (app/src/export.rs
  269) tries next to the exe, then PATH, then the winget `Gyan.FFmpeg` package dir, and that last step is
  the one that finds this machine's ffmpeg. It gains a configured path tried first: an "ffmpeg location"
  field on the General settings page (GM2; there is no Export page). When none is found, video presets show
  "needs ffmpeg (ffmpeg.org); image sequences work without it" and are disabled. Frames are piped as
  rawvideo RGBA to ffmpeg's stdin, spawned through `proc::command` (no console window).
- **Audio stays in the video.** Today `spawn_mp4` (export.rs 101) already muxes the cut's WAV as
  `-c:a aac -b:a 192k` (161-201). Every video preset keeps that when the cut has audio: AAC for H.264/H.265,
  `pcm_s24le` for ProRes and DNxHR. The sound part swaps the single clip for the mixdown.
- **Batch render.** `animstudio --render <file.animproj | --sample> --preset <name> --out <dir>` runs one job
  synchronously and never opens a window: `main()` takes this branch before `eframe::run_native`. Under the
  GUI subsystem it prints through GM1's `AttachConsole`. It prints one line per frame and exits 0 on
  success, 1 on a failed job and 2 on bad arguments. `--sample` renders the welcome part's sample cut built
  in memory. The ship gate's smoke render uses it.
- **Burn-ins.** Corner text options: project, cut, frame (and s+k), timecode, date and artist, in Plex Mono.
- **Render purpose.**
  - View is today's look.
  - **Final** excludes the roles Reference, Rough and Correction, and Ref columns.
  - **LineTest** renders only the line roles (Line, ShadowLine, HighlightLine), black on white.
  - The composite view gets a "Final look" toggle, rendered by the CPU on a worker (D9).

**Files and types to extend.**
- New `crates/anim-core/src/text.rs`: Plex Sans and Mono through `include_bytes!` relative to the app assets
  (D16), `layout_line` and `raster_line(text, px, color) -> Image`. Horizontal only; the generators part
  extends it.
- New `crates/anim-core/src/deliver.rs`:
  - `Preset` and `Purpose { View, Final, LineTest }`;
  - `RenderOpts { purpose, scale }`;
  - `render(cut, frame, &RenderOpts) -> Image`, using the graph when it is wired and the column path
    otherwise;
  - `expand_pattern`;
  - `write_png16`, `write_tiff16` and `write_exr`, through `image` 0.25 (tiff is locked; the exr feature is a
    fetch, D16);
  - `ffmpeg_args(codec, fps, w, h, alpha, audio: Option<&Path>, out) -> Vec<String>`, carrying today's audio
    arguments;
  - `burn_in`;
  - `CliJob` and `parse_cli(args) -> Result<CliJob, String>`.
- `app/src/export.rs`: `find_ffmpeg` becomes `find_ffmpeg_in(configured, exe_dir, path_var, local_appdata)`,
  which keeps its order and only checks files. Its PATH step scans `path_var`'s directories for
  `ffmpeg.exe` instead of spawning `ffmpeg -version` through `works` (line 304), so the tests spawn nothing.
  `find_ffmpeg()` calls it with the real values.
- `app/src/main.rs`: the `--render` branch before `run_native`, calling a plain `run_cli_job(job, data_dir)`.
- `crates/anim-core/src/model.rs`: `Drawing::flatten_for(purpose)`. Today's `flatten`, `render_frame` and
  `render_graph_frame` keep their exact results.
- New `app/src/render_queue.rs`:
  - a pure `Queue` state machine;
  - a worker thread with a channel for progress and an `AtomicBool` for cancel.
- `app/src/export.rs`: the existing PNG and MP4 menu items become queue jobs.
- `Pane::RenderQueue` in `app/src/workspace.rs` (`Pane::ALL` and `title`), tabbed into the Composite and
  Edit docks.
- `app/src/config.rs`: ffmpeg path and default output folder.

**Acceptance criteria.**
1. PNG16: a written and re-read frame equals the f32 buffer quantised to 16 bits. TIFF16 and EXR read back
   with matching size and values (EXR within the half-float tolerance). Written in a temp dir.
2. `ffmpeg_args` is pinned for each video preset. For example:
   - ProRes 4444: `-c:v prores_ks -profile:v 4 -pix_fmt yuva444p10le`;
   - H.264: `-c:v libx264 -crf 18 -pix_fmt yuv420p`;
   - DNxHR HQ: `-c:v dnxhd -profile:v dnxhr_hq -pix_fmt yuv422p`.
   - With audio, H.264 carries `-c:a aac -b:a 192k` exactly as `spawn_mp4` does today, and ProRes carries
     `-c:a pcm_s24le`. Without audio there is no audio argument.
   - No process is spawned in tests.
3. `expand_pattern` is pinned (`show_c012_0007.png`), and characters that are illegal in paths are sanitised.
4. Burn-in pixels hash to a pinned value. An empty burn-in is the identity.
5. Final excludes Correction, Rough and Reference layers and Ref columns: a cut with a Correction layer
   renders under Final the same as without it, and under View the layer shows. LineTest output contains only
   line-role pixels.
6. The Queue state machine processes jobs in order. Cancel stops after the current frame. Progress counts
   match. One threaded test renders a 3-frame 64×36 job.
7. `parse_cli` maps `--render x.animproj --preset "Master PNG 16-bit" --out d` to the expected job, and
   rejects an unknown preset or a missing `--out` (exit code 2). `run_cli_job` on a 3-frame 64×36 project
   writes 3 PNG16 files into a temp dir and returns the frame count.
8. `find_ffmpeg_in` with temp dirs as fakes: the configured path wins; then the exe dir; then PATH; then the
   winget package dir under the fake `LOCALAPPDATA`; with none of them it returns None, and the harness
   shows the video presets disabled with the hint.
9. Entering the Composite room in the harness shows a Render Queue tab.

**Tests to add.** At least 10 tests in `crates/anim-core/tests/delivery_tests.rs`, plus at least 4 app tests
named `render_queue_*`. The gate checks both counts.

**Out of scope.** Episode export, EDL and the colour designation sheet (the cut board part); drop-frame
timecode; MXF/XDCAM; ACES/OCIO; network rendering.

**Stretch.** 50% proxy renders.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test delivery_tests -- --list | grep -c ': test$') && [ "$n" -ge 10 ] && n=$(cargo test -q -p animstudio render_queue -- --list | grep -c ': test$') && [ "$n" -ge 4 ]`

## GM12 Sound tracks and real-time playback

*Depends on: GM11, GM14. Cap 45 min. Gate test file: `sound_tests`; app filter word `playback`. Binding decisions: D0, D1, D2, D4, D14, D16; NEVER-DO 6 and 7.*

**Goal.** Sound for timing and lip sync:
- several tracks per cut (dialogue, SE, music);
- common formats decoded in pure Rust;
- waveforms on the sheet;
- audio-locked real-time playback with frame dropping, plus scrubbing;
- a RAM preview cache;
- 3-mouth lip flap (口パク);
- a mixdown for delivery.

**User-facing behaviour.**
- **Tracks.** The existing `Cut.audio` stays as the first (dialogue) track, so the old API and tests are
  unchanged. More tracks live in `Cut.tracks`. Each track has a name, kind (Dialogue, Effects, Music, Other),
  offset in frames, gain in dB, mute and solo.
- **Import WAV, MP3, OGG and FLAC** (symphonia). The encoded bytes are kept verbatim, as `AudioClip` does
  today, and decoded on load.
- **Waveform.** A per-frame RMS bar beside the sheet for the selected track; the track header has
  mute/solo/gain.
- **Playback** is audio-clocked: the playhead follows the samples played.
  - When a frame is not ready in time, it is skipped to stay in sync, and the viewer shows "dropped N".
  - Loop range and play-from-cursor work.
- **Scrub.** With the toggle on, moving the playhead while stopped plays a one-frame snippet.
- **RAM preview.** A worker fills a frame cache of composited CPU renders, capped in MB in Settings ▸
  Performance (default 2048). An Ao bar on the timeline marks cached frames. Playback uses the cache.
  Document changes invalidate the affected frames.
- **Lip flap.**
  1. Choose the dialogue track, a mouth column and its three drawings (closed, half, open) and a range.
  2. Generate writes the exposures from the RMS envelope, with two thresholds, hysteresis and a minimum hold
     of 2 frames, as one undo step.
  3. Threshold sliders are available.
- **Mixdown.** Delivery writes the mix as WAV (hound) beside image sequences, and video jobs mux it. The
  ffmpeg audio arguments stay exactly as the delivery part pinned them (`-c:a aac -b:a 192k` for MP4, as
  `spawn_mp4` has always done, and `-c:a pcm_s24le` for MOV). Only the input changes, from the cut's one clip
  to the mixdown WAV.

**Files and types to extend.**
- New `crates/anim-core/src/audio.rs`:
  - `AudioTrack` and `TrackKind`;
  - `decode(bytes, ext_hint) -> (rate, channels, Vec<f32>)` through symphonia;
  - `resample_linear`;
  - `mix(cut, fps, frames, out_rate) -> Vec<f32>` (stereo);
  - `envelope(..)` (RMS per frame);
  - `lip_flap(env, (lo, hi), min_hold) -> Vec<Mouth>`.
- `crates/anim-core/src/model.rs`: `Cut.tracks`.
- `crates/anim-core/src/store.rs`: the side table `audio_tracks(cut_id, ord, json, bytes)`. `cut_audio` is
  unchanged.
- `crates/anim-core/src/command.rs`: AddTrack, RemoveTrack and SetTrackProps, all with exact inverses. Track
  offsets join the frame ripple (D4).
- `crates/anim-core/src/deliver.rs`: mixdown and the audio arguments.
- `app/src/main.rs` `AudioOut` (line 288): plays the mix.
- New `app/src/playback.rs`:
  - `PlayClock` (pure, injected sample position);
  - `FrameCache` (LRU by bytes, keyed by (cut, frame, eval hash));
  - `scrub_requests`.
- `app/src/xsheet_panel.rs`: the waveform and track headers.

**Acceptance criteria.**
1. A WAV made in the test with hound decodes to the generated samples within 1e-4.
   - Fixtures, each at most 20 KB, in `crates/anim-core/tests/fixtures/`: `tone.wav` (0.5 s, 440 Hz, 16-bit
     mono), and `tone.flac`, `tone.mp3` and `tone.ogg` encoded from it once, by hand, with the installed
     ffmpeg 8.1.2 (call 8). The exact command lines go in a comment at the top of `sound_tests.rs`. Tests
     only read the files and never spawn ffmpeg.
   - The FLAC decodes to the WAV's samples within 1e-4 (lossless).
   - The MP3 and OGG decode to 0.5 s within 30 ms, and their zero-crossing rate gives 440 Hz within 2%.
   - The format router recognises each file by its magic bytes, not by its extension.
2. Mix of two tracks with offsets and gains equals the pinned sample sum. Mute and solo follow the rules.
   Resampling 44.1k to 48k keeps the duration within 1 sample.
3. A v9 file with `cut_audio` loads as the first track. Extra tracks round-trip through `audio_tracks`.
4. `PlayClock`: at audio position t the frame is floor(t·fps). When rendering lags, frames are skipped and
   counted, and the clock never runs ahead of the audio.
5. `FrameCache` evicts the oldest entries beyond the MB cap, and a changed eval hash invalidates the frame.
6. `lip_flap` on a synthetic envelope (silence, loud, mid) gives the pinned Closed/Open/Half sequence, with
   the minimum hold respected. Generate is one undo step.
7. `envelope` of a sine is pinned per frame.
8. A one-frame move while stopped requests exactly one snippet of one frame's length.
9. Track Commands have exact inverses, and InsertFrames shifts track offsets.
10. No test opens an audio device: `AudioOut` is never built in tests.

**Tests to add.** At least 12 tests in `crates/anim-core/tests/sound_tests.rs`, plus at least 4 app tests named
`playback_*`. The gate checks both counts.

**Out of scope.** Microphone recording, audio FX and EQ, phoneme lip sync, surround.

**Stretch.** Waveform zoom; cue markers taken from the memo row.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test sound_tests -- --list | grep -c ': test$') && [ "$n" -ge 12 ] && n=$(cargo test -q -p animstudio playback -- --list | grep -c ': test$') && [ "$n" -ge 4 ]`

## GM13 Satsuei look: passes, light FX and the satsuei template

*Depends on: GM7, GM8, GM10, GM11. Cap 45 min. Gate test file: `look_tests`; app filter word `comproom`. Binding decisions: D0, D1, D2, D8, D9, D12; NEVER-DO 1 and 2. Review 2026-10-28: the room split moved to the welcome part, colour Curves moved to Stretch, and directional motion blur moved here from the FX core part.*

**Goal.** Modern anime satsuei of the ufotable and MAPPA kind:
- light and shadow passes driven by layer roles;
- trace-line absorb;
- para, light wrap, rim, vignette, grain, directional motion blur, LUTs and mattes (chromatic aberration and
  light rays as Stretch);
- palette variants for time of day;
- a one-click satsuei graph, built from the Composite (撮影) room that the welcome part adds.

**User-facing behaviour.**
- **DrawingSource role filter.** Output only the chosen roles: the Shadow pass, the Highlight pass, paint
  only, or lines only.
  - **Trace-line absorb** (色トレス): pixels of the ShadowLine and HighlightLine roles take the paint colour
    under them; where there is no paint, they take the nearest paint pixel within r px (default 3, chamfer
    search); otherwise they are kept.
  - Filtered sources are `needs_cpu`.
- **New FX kinds**, all through the FX core part's specs and Effect node:
  - Para: a linear or radial gradient, two colours, blend multiply/screen/overlay, an optional matte.
  - Light wrap: BG light bleeding into the character edges.
  - Rim light: an edge from alpha, with direction, width and colour.
  - Vignette.
  - Grain: deterministic per frame and seed, with size and amount.
  - Directional motion blur: a box kernel of length L px along angle θ, sampled bilinearly in L + 1 taps,
    for speed and camera shake. L = 0 is the identity. L and θ bind to param columns like any parameter.
  - Chromatic aberration (Stretch): a radial RGB shift.
  - LUT: a .cube 3D LUT with trilinear lookup. The file is stored in `cut_blobs`, so the cut renders on its
    own.
  - Matte: from luma, alpha or role, with invert and choke/spread.
  - Light rays (Stretch): a radial blur from a point.
- **Palette variants.** A character palette gets named variants (day, evening, night ...) aligned to its
  chips. A **PaletteMap** FX maps exact chip colours to a variant's colours, and the app refreshes the map
  when the variant changes. Under the Srgb law the match is exact.
- **"Build satsuei graph"** builds the graph below as one undo step; every node stays editable:
  - BG through a Camera;
  - per character column, a paint pass, a Shadow pass (Matte, then a Grade that darkens it by multiply), a
    Highlight pass (Glow) and lines;
  - then Diffusion, Para, Light wrap, Grade, Grain and Vignette, into the Output.
- **In the Composite room.** The room itself, its stage key and its default stack come from the welcome part
  (D12), and the render queue pane is tabbed into it by the delivery part. This part puts the "Build satsuei
  graph" button in the node graph pane's toolbar. The button is disabled, with a tooltip saying why, when the
  cut has no drawing column.

**Files and types to extend.**
- `crates/anim-core/src/fx.rs`: the new kinds, plus ParamKind::Blob (a key into `cut_blobs`) and
  ::ColorMap.
- New `crates/anim-core/src/look.rs`:
  - `filter_roles`, `absorb_trace_lines`;
  - `parse_cube(text) -> Lut3D` and `apply_lut`;
  - `directional_blur`;
  - `palette_map`;
  - `satsuei_template(cut) -> Vec<Command>`.
- `crates/anim-core/src/graph.rs`: `DrawingSource { column, #[serde(default)] roles: Option<Vec<LayerRole>>,
  #[serde(default)] absorb_trace: bool }`. Old JSON loads.
- `crates/anim-core/src/model.rs` / `store.rs`: `Cut.blobs` in the side table `cut_blobs(cut_id, key, bytes)`.
- `app/src/palette.rs` / `palette_panel.rs`: variants.
- `app/src/nodegraph_panel.rs`: the template button and the FX category.
- `app/src/graphcomp.rs`: `needs_cpu` grows.

**Acceptance criteria.**
1. A role filter of {Shadow} equals the flatten of only the shadow layers, pixel-exact.
2. Absorb: a synthetic blue ShadowLine between two paint regions leaves no blue pixel after absorb, and each
   line pixel takes an adjacent paint colour. Line-role pixels are untouched.
3. Each new FX at neutral parameters is the identity (bit-exact), matches a pinned output for one non-neutral
   setting, and is deterministic. Grain: same seed and frame gives the same pixels; another frame differs.
4. `parse_cube` reads a size-2 identity LUT and a pinned inverting LUT. The identity applies within 1/255. A
   malformed file is an error, not a panic.
5. Directional blur: L = 0 is the identity. A single white pixel blurred at θ = 0 with L = 8 becomes a
   horizontal run of 9 pixels, each 1/9 within 1/255, with nothing above or below it; θ = 90 gives the same
   run vertically.
6. PaletteMap maps exact chip pixels and leaves other pixels unchanged.
7. The satsuei template builds the pinned node chain, renders with no error on the CPU, and the whole subtree
   is `needs_cpu`.
8. In the harness, on the sample cut (welcome part), the Composite room's "Build satsuei graph" button adds
   the pinned chain as one history entry, and undo restores the graph exactly. On a cut with no drawing
   column the button is disabled.
9. Node parameter edits are single undo steps with exact inverses (SetNodeKind).

**Tests to add.** At least 14 tests in `crates/anim-core/tests/look_tests.rs`, plus at least 3 app tests named
`comproom_*`. The gate checks both counts.

**Out of scope.** 3D lighting (the 3D part), optical flow, and real-time full-resolution heavy FX (the RAM
preview from the sound part covers playback).

**Stretch.** Light rays and chromatic aberration, then colour Curves: master and per-channel monotone cubic
splines with ParamKind::Curve and `monotone_curve`. Until then the FX core part's Grade and a .cube LUT cover
grading. If the cap is hit, these go to `left`; the rest is core.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test look_tests -- --list | grep -c ': test$') && [ "$n" -ge 14 ] && n=$(cargo test -q -p animstudio comproom -- --list | grep -c ': test$') && [ "$n" -ge 3 ]`

## GM14 Sheet panel and graph editor

*Depends on: GM5. Runs sixth, right after GM5; the id is kept from the first numbering. Cap 45 min. Gate test file: `curve_tests`; app filter words `sheetui` and `curves`. Binding decisions: D0, D1, D2, D6, D7.*

**Goal.** The sheet's two views. The X-sheet panel gets direct manipulation of the engine verbs that GM5
built, and animators and 3D artists get a real graph editor: Bezier param keys with tangents, ease in and
out, and a Curves pane to edit them. (Review 2026-10-28: the panel half of the old X-sheet part moved here,
and the camera path overlay moved to the camera part's Stretch.)

**User-facing behaviour: the sheet panel** (`app/src/xsheet_panel.rs`, calling GM5's Commands and
`sheet_ops` builders only):
- Select cell ranges by click, Shift+click or drag, across columns. Retime and reverse act on the range.
- Ctrl+C/X/V copy, cut and paste cells through `copy_block`/`paste_block`, in the Sheet context (D6).
  Shift+paste inserts.
- **Typed entry.** A small entry field on the selected cell (a text field, so single-key shortcuts are
  blocked). Digits plus Enter call `expose_by_name`, and `x` makes the cell Empty. Enter moves down and Tab
  moves to the next column.
- Kind badges (Cel, BG, Book, Ref) with hidden and locked toggles in each column header.
- The memo/dialogue row beside the sheet (セリフ and action notes), editing through `SetMemo`.
- The cut length field (frames plus "s+k"), through `SetFrameCount`.
- Rows virtualise through `show_rows`, so a 600-frame cut scrolls smoothly.

**User-facing behaviour: curves.**
- Param interpolation gains EaseIn (t²) and EaseOut (1 − (1−t)²), stored as "ease_in" and "ease_out". Ease
  is unchanged. (Moved here from the camera part.)
- A **Bezier** interpolation with in and out handles (dt in frames, dv) and handle modes auto, smooth, flat
  and broken. Hold, Linear, Ease, EaseIn and EaseOut stay.
- **Curves pane** (`Pane::Curves`):
  - a list of param columns, with the selected columns' curves over frames, colour-coded by plate tokens;
  - zoom and pan; F frames the selection;
  - click or box to select keys; drag to move them (time snaps to frames);
  - drag handles to edit tangents;
  - change interpolation from the context menu;
  - a current-frame line;
  - each drag is one undo step.

**Files and types to extend.**
- `app/src/xsheet_panel.rs`: selection, the clipboard keys, the entry field, kind badges and toggles, the
  memo row, the cut length field and `show_rows`.
- `crates/anim-core/src/xsheet.rs`:
  - `ParamInterp::{EaseIn, EaseOut}` next to the existing Ease (line 91, smoothstep at 158), and the two
    interp names in `store.rs` (beside "hold"/"linear"/"ease" at 244-246 and 628-630);
  - `ParamInterp::Bezier`;
  - `ParamKey` gets `#[serde(default)] handles: Option<Handles { in_dt, in_dv, out_dt, out_dv, mode }>`;
  - segment evaluation: a cubic Bezier in (t, v), solved by Newton with a bisection fallback, with handle dt
    clamped so t stays monotonic. Auto handles have Catmull-Rom slopes and length one third of the segment.
- `crates/anim-core/src/store.rs`: the interp text `bezier:<in_dt>,<in_dv>,<out_dt>,<out_dv>,<mode>`. There
  is no column change (D2).
- `crates/anim-core/src/command.rs`: `SetParamKeys`, a batch for multi-key drags. SetParamKey exists.
- New `app/src/curves_panel.rs`: a pure `CurveView` mapping (frame and value to screen) and the pane.
- `Pane::Curves`.
- `app/src/config.rs`: Actions.

**Acceptance criteria.**
1. Bezier endpoints are exact. Handles equivalent to linear match Linear within 1e-4 at every integer frame. A
   sample curve gives pinned values. Time solving is monotonic for any clamped handles.
2. The existing interps and their tests are unchanged, and the Bezier text round-trips through the store.
3. `CurveView` round-trips between screen and value within 1e-4. In the harness, a key drag moves the key by
   the expected frames and value in one undo step. Box select works, and F frames everything.
4. EaseIn and EaseOut values at t 0, 0.25, 0.5 and 1 are pinned. The existing Ease values are unchanged. The
   new names round-trip.
5. Dragging onto a frame that already has a key moves that key; it never duplicates it.
6. Sheet panel, through the harness: a Shift+click range selects the expected cells; typing "3" then Enter
   in the entry field exposes drawing "3" in one undo step, and typing "B12" there changes no tool; Ctrl+C
   then Ctrl+V on another column reproduces the block; setting the cut length field to 60 gives 60 frames
   in one undo step; the hidden toggle drops the column from the CPU column render; a 600-frame cut renders
   its rows without panic and lays out only the visible rows.

**Tests to add.** At least 10 tests in `crates/anim-core/tests/curve_tests.rs`, at least 5 app tests named
`curves_*` and at least 5 named `sheetui_*`. The gate checks all three counts.

**Out of scope.** Motion paths for drawings, expressions and physics; the camera path overlay (the camera
part's Stretch).

**Stretch.** Copy and paste keys; an ease preset library.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test curve_tests -- --list | grep -c ': test$') && [ "$n" -ge 10 ] && n=$(cargo test -q -p animstudio curves -- --list | grep -c ': test$') && [ "$n" -ge 5 ] && n=$(cargo test -q -p animstudio sheetui -- --list | grep -c ': test$') && [ "$n" -ge 5 ]`

## GM15 Cut board, episode timeline and EDL

*Depends on: GM6, GM11, GM12, GM13. Cap 45 min. Gate test file: `board_tests`; app filter word `cutboard`. Binding decisions: D0, D1, D3, D4, D7, D8. Review 2026-10-28: episode export, the EDL and the colour designation sheet moved here from the delivery part. The storyboard pane and conte pages moved to Stretch; the animatic stays core through the welcome part's Animatic template.*

**Goal.** Editorial (編集) inside the app:
- a cut board with production status;
- an episode timeline with total running time (TRT) and reordering;
- playing the episode, and rendering it as one job;
- a CMX3600 EDL for the online editor;
- an animatic from storyboard panels, and cuts made from its panels;
- the colour designation sheet (色指定表) for the paint team;
- retake notes;
- exporting a cut as its own project for a freelancer.

**User-facing behaviour.**
- **Board pane** (`Pane::Board`).
  - A grid of cut cards: thumbnail (CPU render of the first key or frame 0), cut number, length in s+k,
    status, assignee and note count.
  - Status values: LO, Genga, Sakkan, Douga, Paint, Composite, Done, Retake.
  - Filters by status and assignee. A click opens the cut. Status and assignee are editable and undoable,
    through `cut_meta` `prod.status` and `prod.assignee`.
- **Episode timeline** (`Pane::Episode`).
  - Cuts in order as blocks proportional to their length. Drag to reorder with `MoveCut`, which is undoable.
  - TRT is shown as `MM:SS+FF` against a target (`app_meta["episode.target"]`), with over and under marked.
- **Play episode.** Plays the scene's cuts back to back, each cut's frames and sound (the RAM cache from the
  sound part).
- **Episode render.** The render queue gains an Episode job: every cut of the scene in order, through
  `episode_frame_map`, as one image sequence or one video, with the cuts' mixdowns laid end to end.
- **EDL.** "Export EDL" writes a CMX3600 EDL for the episode, non-drop, record start 01:00:00:00. There is
  one video event per cut, plus an audio event when the cut has sound. Each event's source is the cut's
  render (`expand_pattern` without the frame field), from source in 00:00:00:00, and carries a
  `* FROM CLIP NAME:` line.
- **Colour designation sheet.** "Export colour sheet" writes one PNG per character palette. Each chip has
  its swatch, name, role and hex, and the satsuei part's variants (day, evening, night ...) appear as extra
  columns. Text goes through `anim_core::text`. Swatches use the chip value exactly as stored (Srgb law, D8).
- **Animatic from panels.** A cut from the Animatic / storyboard template (welcome part) has a BOARD column.
  Each exposure there is a panel, and the exposure length is the panel's duration.
  - A panel's cut number is set from the cell's context menu and stored in the drawing's `DrawingDef` JSON
    (light table part). The default is the panel's order.
  - The cut plays as the animatic with its tracks (sound part).
  - **"Make cuts from panels"** creates, for each distinct cut number in order, a cut whose length is the sum
    of its panels, with the panel pictures exposed on a Ref column. One undo step.
- **Retake notes.** `prod.notes` holds a JSON list of {frame, text, author, resolved}. Notes show as markers
  on the sheet rows, the board shows their count, and adding one can set status Retake. Notes join the frame
  ripple (D4).
- **Export cut as project.** Writes a new `.animproj` holding that cut, its palettes and project settings. The
  source is untouched.

**Files and types to extend.**
- New `crates/anim-core/src/board.rs` (pure):
  - `CutStatus`, `ProdInfo` (from and to cut_meta);
  - `trt_frames(scene)` and `format_trt(frames, fps)`;
  - `episode_frame_map(scene) -> Vec<(cut_index, frame)>`;
  - `edl_cmx3600(scene, fps, pattern) -> String`;
  - `color_sheet_layout(palette) -> Vec<Rect>` and `render_color_sheet(palette) -> Image`;
  - `cuts_from_panels(cut, column) -> Vec<Command>`.
- `crates/anim-core/src/command.rs`: `MoveCut { scene, from, to }`.
- `crates/anim-core/src/store.rs`: `export_cut(project, scene, cut, path)`.
- `crates/anim-core/src/deliver.rs`: the Episode job's frame source and the mixdowns laid end to end.
- New `app/src/board_panel.rs` (Board and Episode panes).
- `app/src/render_queue.rs`: the Episode job kind.
- `app/src/playback.rs`: episode play.
- `app/src/xsheet_panel.rs`: note markers and the panel cut-number menu.
- `Pane::Board` and `Pane::Episode` in `workspace.rs`, tabbed into the Edit room's dock (`Stage::dock`).

**Acceptance criteria.**
1. `MoveCut` has an exact inverse, and the order persists through save and load.
2. `format_trt` is pinned (e.g. 30,972 frames at 24 fps is "21:30+12"), and so is the target difference.
3. Status and assignee round-trip through cut_meta. A status change is one undo step. The status filter
   returns exactly the matching cuts.
4. Panels of 36, 24 and 48 frames, numbered cut 1, 1 and 2, give 2 cuts of 60 and 48 frames, with the
   pictures exposed on a Ref column. One undo step, and undo restores the scene exactly.
5. `edl_cmx3600` for three cuts of 24, 36 and 48 frames at 24 fps equals a pinned text: events 001-003 with
   record in/out 01:00:00:00-01:00:01:00, 01:00:01:00-01:00:02:12 and 01:00:02:12-01:00:04:12, each naming
   its clip.
6. An Episode job over those three cuts, rendered into a temp dir, writes 108 frames that equal the per-cut
   renders frame for frame (hashes). Its mixdown at 48 kHz has 216,000 samples per channel.
7. `render_color_sheet` for a 6-chip palette with one variant has the pinned size and pinned chip rects. Each
   swatch's centre pixel equals the stored chip value exactly, and every label rect holds text pixels.
8. Note markers fall on the right rows, and InsertFrames moves the notes.
9. `export_cut` writes a file that loads with one scene and one cut, whose drawing, sheet and graph hashes
   equal the source's.
10. `episode_frame_map` is right across cut boundaries.
11. In the harness, the Board shows a card per cut, clicking a card opens that cut, and the Edit room shows
    Board and Episode tabs.

**Tests to add.** At least 10 tests in `crates/anim-core/tests/board_tests.rs`, plus at least 6 app tests
named `cutboard_*`. The gate checks both counts.

**Out of scope.** Production-tracking servers (ShotGrid, ftrack), merging freelancer cuts back, multi-scene
episodes beyond scene order, and AAF/XML timelines.

**Stretch.** The storyboard pane (`Pane::Storyboard`, the vertical conte layout with picture, action,
dialogue and time per panel, where editing a duration ripples through SetCells), then conte page export (A4
portrait, 5 panels per page, 150 dpi PNG, then PDF), then slate frames in the episode render.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test board_tests -- --list | grep -c ': test$') && [ "$n" -ge 10 ] && n=$(cargo test -q -p animstudio cutboard -- --list | grep -c ': test$') && [ "$n" -ge 6 ]`

## GM16 Generators: particles, speed lines, lettering

*Depends on: GM9, GM11. Cap 45 min. Gate test file: `gen_tests`; app filter word `genui`. Binding decisions: D0, D7, D9, D16; NEVER-DO 1 and 2.*

**Goal.** Procedural content for both anime FX and manga:
- deterministic particle systems;
- speed lines (流線) and focus lines (集中線);
- lettering, including vertical Japanese (縦書き);
- speech balloons.

**User-facing behaviour.**
- A **Generator** node with an inspector from specs (the FX core system), listed under "Generators" in the
  add-node menu.
- **Particles.**
  - Emitter: point, line, rect, or a region from a matte input.
  - Motion and look: rate, lifetime with variance, velocity with spread, gravity, wind, turbulence, and size,
    colour and opacity over life.
  - Shapes: dot, streak (along velocity), soft disc, petal, snowflake.
  - Blending: Normal or Add.
  - **Analytic and deterministic.** The state at frame f is computed from the seed and the birth index, with
    no simulation history, so any frame renders on its own and scrubbing is exact.
  - Presets: Sparks, Snow, Rain, Dust, Sakura petals, Embers.
- **Speed lines.** N lines along an angle, with seeded perpendicular offsets and length and thickness ranges,
  tapered at both ends. An optional clear band; a jitter per frame, or held on twos.
- **Focus lines.** Radial lines from beyond the frame toward a centre, stopping at a clear ellipse (rx, ry)
  plus seeded jitter. Tapered toward the centre; density and thickness are set.
- **Text.**
  - Direction: horizontal or **vertical**. Vertical text runs in columns right to left, characters top to
    bottom.
    - Brackets and dashes use their vertical forms or are rotated: 「」『』（）, ー〜…―.
    - 、 and 。 sit in the top-right of the cell.
  - **Tate-chu-yoko:** runs of 1–2 ASCII digits are set upright inside one cell.
  - Size, leading, tracking, colour, outline width and colour, box position and wrap width.
  - Fonts: the bundled Plex Sans, and the **full** IBM Plex Sans JP Bold (`app/assets/fonts`, 4 MB), now
    embedded.
- **Balloons.** Ellipse, rounded rect, cloud or burst (shout), with a tail to a point, fill and stroke. Put a
  Text node over one to letter it.
- **UI font.** With the full JP font embedded, the UI's JP fallback switches to it and drops the subset. A
  test asserts that every non-ASCII label string in `app/src` has glyphs in the embedded font (D7).

**Files and types to extend.**
- New `crates/anim-core/src/gen.rs`:
  - `GenKind { Particles, SpeedLines, FocusLines, Text, Balloon }` with their specs (ParamKind::Text added);
  - `particles_at(params, frame) -> Vec<Particle>` (pure), rasterised as soft discs and streaks;
  - `speed_lines`, `focus_lines`, `balloon` and `text`.
- `crates/anim-core/src/text.rs` (from the delivery part): vertical layout, tate-chu-yoko, outlines (dilated
  coverage) and the JP font static. It keeps the rasteriser the delivery part chose (D16): ab_glyph, or the
  locked skrifa + harfrust, whose `vert` feature supplies the vertical glyph forms.
- `crates/anim-core/src/graph.rs` (`NodeKind::Generator { gen, params, binds }`), `export.rs`, `eval.rs`.
- `app/src/graphcomp.rs`: Generator is `needs_cpu`.
- `app/src/plate.rs`: the JP fallback reads `anim_core::text`'s bytes.
- `app/src/nodegraph_panel.rs` / `fxui.rs`: the category and the Text field.

**Acceptance criteria.**
1. Particles: same seed and frame give identical pixels. Frame f rendered alone equals frame f rendered after
   f−1. The alive count is analytic (births within (f−L, f]) and matches. Gravity moves the centroid down
   over life.
2. Every preset renders non-empty inside the frame, with valid parameters.
3. Speed lines: every line is within 0.5° of the angle, and the count matches the density. A seed change
   changes the layout. With on-twos, frames 2k and 2k+1 are identical.
4. Focus lines: no ink inside the clear ellipse; every line points at the centre within 1°; the count is
   pinned.
5. Horizontal text: the pixel hash of "Gaban" at 48 px is pinned, and the bbox width grows with the string.
6. Vertical text:
   - the glyph centres of あいう share x, with y increasing;
   - a wrap moves to the next column to the left;
   - 、 sits in the top-right quadrant of its cell;
   - ー is taller than it is wide;
   - "12" takes one cell.
7. Balloon: the tail tip pixel is at the given point; the fill is inside and the stroke on the outline.
8. Generator nodes round-trip through the store, their parameters can be bound, and they are `needs_cpu`.
9. The UI glyph-coverage test passes. It is an app test (`genui_glyph_coverage`) that scans `app/src` for
   non-ASCII string literals.
10. In the harness, typing "speed" in the add-node menu and pressing Enter adds a SpeedLines Generator node in
    one history entry, and its inspector shows the spec's parameters.

**Tests to add.** At least 14 tests in `crates/anim-core/tests/gen_tests.rs`, plus at least 2 app tests named
`genui_*`. The gate checks both counts.

**Out of scope.** Particle collisions, fluid and smoke simulation, text on a path, and ruby (furigana).

**Stretch.** Ruby; user font files stored in `cut_blobs`; particle sprites taken from a column's drawing. If the
cap is hit, the cloud and burst balloon shapes go to `left` first, then the Dust and Embers presets.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test gen_tests -- --list | grep -c ': test$') && [ "$n" -ge 14 ] && n=$(cargo test -q -p animstudio genui -- --list | grep -c ': test$') && [ "$n" -ge 2 ]`

## GM17 3D layout and toon render

*Depends on: GM10, GM11. Cap 45 min. Gate test file: `r3d_tests`; app filter word `view3d`. Binding decisions: D0, D1, D2, D9, D12, D15, D16.*

**Goal.** The 3D artist's bridge:
- import glTF and OBJ models (sets, props, vehicles);
- place them with a 3D camera in a View3D pane that navigates like Blender;
- render them toon-shaded with outlines and passes, in the CPU golden path, as a node keyed on the sheet;
- reference image sequences rendered by other apps;
- import JPEG and TIFF.

**User-facing behaviour.**
- **File ▸ Import ▸ 3D model** (.glb, .gltf, .obj) into the cut. The bytes are embedded in `cut_models`.
  Meshes are triangulated, and the material base colours are read.
- **Model3D node.**
  - Object transform: translate, rotate and scale.
  - Camera: position, target, roll, focal length in mm on a 36 mm film back, near and far.
  - Light direction.
  - Toon: shade steps (2 or 3), shadow colour multiplier, highlight threshold.
  - Outline: width in px, colour, and crease angle.
  - Pass: Beauty, Lines, Shadow matte, Depth (normalised) or ID matte.
  - Every parameter can be bound to param columns, so camera moves are keyed on the sheet.
- **CPU toon rasteriser**:
  - edge-function rasterising with the top-left rule;
  - near-plane clipping and an f32 z-buffer;
  - cel-shaded Lambert quantised to steps;
  - outlines from depth, normal or ID discontinuities, dilated to width;
  - deterministic.
- **View3D pane** (`Pane::View3D`).
  - The CPU render runs at half resolution while navigating and full resolution when idle.
  - Blender navigation: MMB orbit, Shift+MMB pan, wheel or Ctrl+MMB zoom; numpad 1, 3 and 7 for front,
    right and top, with Ctrl for the opposite view; numpad 0 for the camera view; numpad 5 for ortho.
  - **"Set cut camera from view"** keys the camera parameters at the current frame, in one undo step.
- **The 3D render as a layout underlay.** A canvas toggle shows a chosen Model3D node's render for the current
  frame under the drawing, view only like a light-table pin and rendered on the CPU worker, so layout artists
  draw over 3D.
- **ImageSequence node.**
  - A pattern relative to the project file (`renders/shot_####.png`), with first frame, offset and
    hold-last.
  - PNG, TIFF, EXR and JPEG are supported. A missing frame renders transparent and is reported as a warning.
  - The sequence is referenced, not embedded (D15).
- **ImageSource import** accepts JPEG and TIFF; they are decoded and stored as PNG, like today's imports.

**Files and types to extend.**
- New `crates/anim-core/src/r3d.rs`:
  - `Mesh`, and `load_gltf(bytes)` / `load_obj(bytes)` through gltf and tobj;
  - `Camera3`, with `look_at`, `perspective` and `focal_to_fov`;
  - `rasterize(scene, camera, w, h) -> Buffers { color, depth, normal, id }`;
  - `toon_shade` and `outline`;
  - `Orbit` (navigation maths, pure).
- `crates/anim-core/src/graph.rs`: `NodeKind::Model3D { model, params, binds }` and `NodeKind::ImageSequence {
  pattern, first, offset, hold_last }`.
- `crates/anim-core/src/store.rs`: `cut_models`.
- `crates/anim-core/src/deliver.rs`: `RenderOpts.base_dir` and warnings for sequences.
- `export.rs` and `eval.rs`.
- New `app/src/view3d.rs`.
- `Pane::View3D`, and the off-spine "3D layout" room in `workspace.rs`.
- `app/src/main.rs`: the import menu.
- `app/src/doc.rs`: image import through `image` 0.25.
- `app/src/graphcomp.rs`: both nodes are `needs_cpu`.

**Acceptance criteria.**
1. An OBJ cube written in the test loads as 12 triangles with a bbox of ±1. A minimal triangle .glb built in
   the test loads.
2. A point at the target projects to the frame centre, and 50 mm on 36 mm gives a 39.6° horizontal FOV
   (pinned).
3. A front-facing quad covers the expected pixel count within one edge's worth. The nearer triangle wins the
   z test.
4. A sphere render has exactly `steps` distinct shade levels, and the shadow multiplier is applied.
5. Cube silhouette outline pixels are the outline colour and within width bounds. Width 0 means no outline.
6. The Depth pass is monotonic with distance, and the ID matte is distinct per object.
7. Bound camera parameters change the frames, and renders are deterministic.
8. `cut_models` round-trips byte-identical.
9. `Orbit` maths round-trips. The numpad views set exact orientations. "Set cut camera from view" writes keys
   in one undo step.
10. `shot_####.png` at frame 7 resolves to `shot_0007.png`. A missing file renders transparent with one
    warning. Relative paths resolve against `base_dir`.
11. JPEG and TIFF imports decode to the right size.

**Tests to add.** At least 14 tests in `crates/anim-core/tests/r3d_tests.rs`, plus at least 4 app tests named
`view3d_*`. The gate checks both counts.

**Out of scope.** A GPU 3D renderer, FBX and Alembic, rigging and skinning, PBR, and cast shadows between
objects.

**Stretch.** Turntable (bind the object's Y rotation to a param column keyed linearly from 0 to 360 over the
cut); optional 2× supersampling; glTF base-colour textures; glTF node animation mapped to frames; shadow
mapping. If the cap is hit, the canvas underlay goes to `left` first, then EXR in ImageSequence.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test r3d_tests -- --list | grep -c ': test$') && [ "$n" -ge 14 ] && n=$(cargo test -q -p animstudio view3d -- --list | grep -c ': test$') && [ "$n" -ge 4 ]`

## GM18 Manga pages, tones and perspective rulers

*Depends on: GM8, GM13, GM16. Cap 45 min. Gate test file: `manga_tests`; app filter word `mangaui`. Binding decisions: D0, D3, D7, D9, D12, D15.*

**Goal.** A manga artist can make finished pages:
- B5/A4 page templates at 350 dpi with bleed and trim;
- panel (コマ) layout with gutters and clipping;
- screentones as a non-destructive Tone FX;
- perspective rulers (1, 2 and 3 point) with stroke snapping;
- page export as PNG or TIFF in colour, grey or 1-bit, plus an optional PDF.

**User-facing behaviour.**
- **Templates.** Each is added to the welcome part's `templates.rs`.
  - Manga B5: 182×257 mm trim plus 3 mm bleed, which at 350 dpi is **2591×3624 px**.
  - Manga A4: 210×297 mm plus bleed, **2976×4175 px**. Every size rounds mm·dpi/25.4 to the nearest pixel,
    as the gutters do.
  - Webtoon: 800 px wide, sliced into 1280 px segments.
  - Guides show the bleed (裁ち切り), the trim (仕上がり線) and the inner frame (基本枠).
- **Pages.** A book is a scene, and a page is a cut with `frame_count` 1. The Manga room's page list shows one row
  per page (thumbnail and number), and New page adds a cut in one undo step (AddCut, from the X-sheet part). Page
  geometry lives in `cut_meta["manga.page"]`.
- **Manga room**, "manga (pages)", off the spine. It holds the canvas with page guides, the page list, the
  panel tool, tones, rulers and layers.
- **Panel tool.**
  - Drag a rect, or split a panel with a line. The gutters default to 2 mm horizontal and 5 mm vertical and
    are editable; the border width defaults to 0.5 mm.
  - Panels live in a **Panels** generator's polygon parameter (ParamKind::Polys). Each edit is one undo step
    through SetNodeKind.
  - **Clip to panels:** the template builds, per art column, DrawingSource, then a Matte (the panels'
    interior, from the satsuei look part), over white paper; the Panels borders go on top.
- **Tone FX.** A grey tone layer becomes an amplitude-modulated halftone.
  - Options: line frequency (lpi, default 60), angle (default 45°) and dot shape (round, square or line).
  - The output is ink on transparent, 1-bit when needed, and the result is non-destructive.
- **Perspective rulers.**
  - Place 1, 2 or 3 vanishing points and a horizon on the canvas; they are kept in
    `cut_meta["manga.rulers"]`.
  - With snapping on, a stroke chooses its direction after about 6 px of movement. The candidates are the
    lines through each vanishing point plus horizontal and vertical (the third vanishing point replaces
    vertical in 3-point). The nearest-angle candidate wins, and later points are projected onto it.
  - The snap acts on the stroke-tap pipeline before the stabilizer (the pen aids part).
- **Page export.** PNG or TIFF at full dpi in Colour, Grey or Mono.
  - Mono is 1-bit: by threshold, or as-is when it is already halftone.
  - Trim marks are optional.
  - DPI metadata is written: PNG pHYs 13780 px/m, TIFF resolution 350 per inch.
  - Webtoon is sliced into segments.
  - Optional PDF of all pages at physical size (pdf-writer).

**Files and types to extend.**
- New `crates/anim-core/src/manga.rs` (pure):
  - `page_px(trim_mm, bleed_mm, dpi)`, `trim_rect` and `inner_frame`;
  - `split_panel(poly, line, gutter_px)`;
  - `halftone(density, x, y, lpi, angle, shape, dpi) -> bool`, plus a coverage helper;
  - `snap_direction(start, current, rulers) -> Option<Line>` and `project`;
  - `slice_webtoon`.
- `crates/anim-core/src/fx.rs` (`FxKind::Tone`) and `gen.rs` (`GenKind::Panels`, ParamKind::Polys).
- `crates/anim-core/src/deliver.rs`: `write_page(img, path, format, mode, dpi, marks)` and the optional PDF.
- `app/src/templates.rs` (the three templates) and new `app/src/manga_tools.rs` (panel and ruler tools).
- `app/src/canvas.rs`: page guides and the snap hook.
- `app/src/workspace.rs`: the Manga room.

**Acceptance criteria.**
1. B5 at 350 dpi with a 3 mm bleed gives 2591×3624. A4 gives 2976×4175 (303 mm is 4175.2 px; rounding, not
   ceil). The trim and inner-frame rects are pinned.
2. Splitting a full panel with a 5 mm gutter leaves a gap of round(5·350/25.4) = 69 px. Undo restores the
   panel.
3. Art outside the panel polygons renders transparent, and inside it is unchanged.
4. Tone at 60 lpi and 45°:
   - flat 10, 30, 50, 70 and 90% grey give ink coverage within ±3%;
   - 0% gives no ink and 100% is solid;
   - Mono output is only 0 or 255;
   - the dot-grid centres rotate with the angle (pinned).
5. Snap: a 1-point case snaps to the line through the vanishing point or to horizontal or vertical, whichever
   is nearest. Pinned 2-point and 3-point cases. Snap off is the identity.
6. Exported PNG pHYs is 13780 px/m and TIFF resolution is 350. Mono contains only 0 and 255. Grey uses Rec.709
   luma. The webtoon slice count is pinned. If the PDF is built: N pages with MediaBox equal to the trim plus
   bleed in points.
7. In the harness, the Manga room shows the panel, tone and ruler tools, and the B5 template makes a
   2591×3624 project.

**Tests to add.** At least 14 tests in `crates/anim-core/tests/manga_tests.rs`, plus at least 4 app tests named
`mangaui_*`. The gate checks both counts.

**Out of scope.** Symmetry and ellipse rulers, automatic text flow into balloons, 3D-derived perspective, and
600 dpi B4 analogue manuscript sizes (VRAM).

**Stretch.** PDF export; slanted (polygon) panels drawn freehand. If the cap is hit, the Webtoon template and
its slicing go to `left` first, then the line dot shape.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p anim-core --test manga_tests -- --list | grep -c ': test$') && [ "$n" -ge 14 ] && n=$(cargo test -q -p animstudio mangaui -- --list | grep -c ': test$') && [ "$n" -ge 4 ]`

## GM19 Ship: polish gates, version and release build

*Depends on: GM1–GM18 and the sweep parts GM20–GM22 (or every part not `removed`). Cap 45 min. Filter word: `ship_`. Binding decisions: D0, D5, D6, D7; NEVER-DO 8 and 10.*

**Goal.** Lock in quality and produce 0.5.0 locally:
- the colour ratchet at zero;
- pane smoke tests and the probe over every room;
- every Action described, with no conflicts;
- a refreshed sample, a README, an About and a What's New;
- the release build, a headless smoke render of the real exe through the installed ffmpeg, and the
  installer.

Nothing is published.

**User-facing behaviour.**
- On the first launch of 0.5.0, the welcome screen shows a **What's New** tab grouped by persona: animator,
  painter, compositor, 3D, manga and editor. Help ▸ About shows the version.
- The sample cut is refreshed to show off the release: a camera pan over a wide BG, glow and diffusion,
  particles, keys and inbetweens, and a short dialogue track generated in code (hound), so the smoke render
  has sound to mux. It uses only parts that were not removed.
- **Smoke render** (`tools/smoke_render.sh`, run by the gate after the release build). It runs
  `target/release/animstudio.exe --render --sample` (the delivery part's batch render, which opens no window)
  with the presets "Master PNG 16-bit", "ProRes 422 HQ" and "H.264 review", each into its own folder under a
  `mktemp -d` directory.
  - It checks that the PNG folder holds exactly the sample's frame count of files.
  - With ffprobe, it checks that the MOV's video codec is `prores` and the MP4's is `h264`, that both have
    the sample's frame count (`-count_frames`), and that each has one audio stream (`pcm_s24le` and `aac`).
  - It finds ffmpeg and ffprobe the way `find_ffmpeg` does (PATH, then the winget `Gyan.FFmpeg` package
    dir). If they are missing, it fails with a clear message and does not skip.
  - It deletes the temp dir on exit (`trap`). It never starts the GUI, an installer or a browser.
- README.md covers features by persona, quick start, keyboard basics, building, and where the research lives.

**Files and types to extend.**
- Every file with `Color32` literals outside `plate.rs`: move them to plate tokens or derived plate helpers.
- `app/src/sample.rs` and `app/src/welcome.rs`.
- `app/Cargo.toml` and `tools/installer/Cargo.toml`: version 0.5.0.
- `app/src/net.rs`: `MIN_PEER_VERSION = "0.5.0"` (line 229).
- `README.md`.
- Ship tests in `app/src/testkit.rs` (named `ship_*`).
- New `tools/smoke_render.sh`.

**Acceptance criteria.**
1. The colour-literal ratchet is pinned at 0.
2. Every Action has help, a group and a menu path or palette-only flag, and neither keymap has a conflict.
3. Every `Pane` variant renders 30 headless frames at ppp 1.0, 1.25 and 1.5 with no panic, and the probe
   reports 0 MOVED for every room.
4. `env!("CARGO_PKG_VERSION")` is 0.5.0 in the app, and the installer package is 0.5.0. `MIN_PEER_VERSION` is
   "0.5.0". The About text shows 0.5.0.
5. The refreshed sample renders all its frames through the CPU renderer (at reduced size in debug) with no
   error.
6. The full gate passes, in this order: both test packages, the `ship_*` count, the animstudio release
   build, the smoke render, the no-tag check, and then the installer release build, which embeds the new exe
   (LESSONS.md: the exe must be built before the installer).
7. No `v0.5*` tag exists (`git tag --list 'v0.5*'` prints nothing), and the gate checks that.
   `tools/release.sh` is not run, and nothing is pushed, published or installed (NEVER-DO 8 and 10; these
   are rules, not tests).
8. The smoke render exits 0: PNG count, both codecs, both frame counts and both audio streams as above.

**Tests to add.** At least 5 tests named `ship_*`. The gate checks the count.

**Out of scope.** GitHub releases, code signing, installing on this machine, and the update channel.

**Stretch.** None. If the cap is hit, the ratchet work is what goes to `left`, with the count reached.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p animstudio ship_ -- --list | grep -c ': test$') && [ "$n" -ge 5 ] && cargo build --release -p animstudio && bash tools/smoke_render.sh && [ -z "$(git tag --list 'v0.5*')" ] && cargo build --release -p animstudio-installer`

## GM20 Painting on the extended paper

*Depends on: GM4, GM6, GM7, GM10. Cap 45 min. Filter word: `extpaper_`. Binding decisions: D0, D1, D3, D7, D10; NEVER-DO 3, 4, 6 and 7.*

**Goal.** The camera part's extended paper (Cut ▸ Paper size…) becomes paper you can draw on: a BG artist paints
a pan BG past the frame's edges, and the camera pans over what was painted. Today the canvas still paints on a
frame-sized sheet:
- the GPU layers are frame-sized, and `fill_texture` drops every tile outside the frame;
- fill, refill and replace-all, the lasso fill, select-all, the eyedropper and the presence pointer stop at the
  frame.

This part takes the camera part's two core `left` items: paint layers at paper size with the GPU cap shown, and
painting, fill and lasso over the paper.

**User-facing behaviour.**
- **Layers at paper size.** With a paper larger than the frame, the canvas's GPU layers cover the paper: every
  target `make_target` builds (active, wet, remote, below, above and scratch). The onion, light-table and
  other-column slots at paper size are Stretch; until then they keep the frame's size and show its part.
  - A layer has an origin. Texel (0, 0) is the paper's top-left corner in frame coordinates, so a tile at
    negative coordinates uploads, reads back and displays at its place. A tile across the paper's edge is
    uploaded and read back in part, as the frame's right and bottom edges are today.
  - The size is capped by the device's `max_texture_dimension_2d` (`wgpu::Device::limits()`). Past the cap the
    layers cover the largest window of the paper that holds the frame, and painting stops at its edge. The
    Paper size window says so ("this GPU paints at most 8192 px a side: painting covers 8192×1080 of the
    10000×1080 paper") and shows the layers' GPU memory (8 bytes a texel, every target and slot counted).
  - The layers are rebuilt on a cut switch, or when "set paper" changes the size, never mid-stroke: a change
    that arrives during a stroke waits until the stroke lands (NEVER-DO 6).
  - The composite viewer and the GPU graph keep their frame-sized pool. Their DrawingSource and ImageSource
    slots read the frame window of the paper-sized layer (`composite_layers_into`), so their pixels are as
    before. The viewer asks for the same `LayerRect` as the canvas, from one helper, so the two never rebuild
    the layers back and forth.
- **Painting beyond the frame.** The brush, the eraser, fill, refill and replace-all (Shift+click and
  Ctrl+Shift+click), the lasso fill and select-all work over the whole paper. Each commit is one `PaintTiles`
  undo step, as on the frame (NEVER-DO 3). A fill click outside the paper is refused on the Aka lane: "outside
  the paper: Cut ▸ Paper size… extends it" (D7).
- The eyedropper and the session presence pointer work over the whole paper.
- **Old work is untouched.** With the default paper (paper = frame) the layer sizes, the tiles, the fills and
  every pixel are exactly as before (NEVER-DO 4). In canvas.rs, `paper_w`/`paper_h` mean the frame today; the
  paper rect is `camera::paper(cut, w, h)`.

**Files and types to extend.**
- `app/src/paint.rs`:
  - a pure `LayerRect { origin: (i32, i32), size: (u32, u32), capped: bool }`, made by
    `layer_rect(paper: IRect, frame: (u32, u32), max_dim: u32)`, and `tile_span(tile, &LayerRect)`: the part of
    a tile that falls on the layer, and where. Upload and readback both use it, so the two directions cannot
    disagree;
  - `PaintLayer::new` (402) and `ensure_size` (998) take a `LayerRect`; `create_layer_texture` (906) and
    `make_target` (928) build at its size;
  - `fill_texture` (1397) maps tiles through `tile_span` instead of dropping negative ones, and so does
    `read_tiles` (1597); `set_onion` (1464) and `sync_other_column` (1523) follow with the Stretch;
  - `build_projection` (1140), the dab `Uniforms` (144; the origin can ride in its `_pad`) and the shaders
    offset by the origin;
  - `composite_layers_into` (1148) also fills the GPU graph's frame-sized DrawingSource and ImageSource slots
    (graphcomp.rs 606 and 617); there it reads the layer's frame window, texels from −origin at the frame's
    size.
- `crates/anim-core/src/fill.rs`: `flood_fill_mask_in(reference, seed, area: IRect, opts, clip)` fills inside
  `area`, in frame coordinates (negative allowed). `flood_fill_mask` (139) becomes that call over the frame and
  stays bit-identical. `FillMask` (57) gains its origin, and `get` reads frame coordinates.
- `crates/anim-core/src/color.rs`: `refill` (523) and `replace_all` (567), through `seed_texel` (498), get the
  same area forms (`refill_in`, `replace_all_in`). The frame forms become those calls over the frame and stay
  bit-identical, so `color_tests` pass unchanged.
- `crates/anim-core/src/camera.rs`: `paper(cut, w, h)` (177), `MAX_PAPER` and `MAX_PAPER_AREA`, read only.
- `app/src/canvas.rs`:
  - the `ensure_size` calls (4011, 4019) pass the cut's `LayerRect`, and `paint_paper_image` (233) draws the
    layers at the paper;
  - `select_all` (1364) selects the paper, the fill call (6371) uses `flood_fill_mask_in` over the paper, and
    `commit_lasso_fill` (6076) clips its grid to the paper instead of the frame;
  - `pick_sample` (7406) and the presence filter (3774) accept points on the paper.
- `app/src/main.rs`: `PaperForm` (4220) and `paper_size_window` (4984) show the cap and the memory. The editor
  keeps the device's limit, set from the wgpu device when the canvas is built (the testkit sets its own). The
  viewer's `p.ensure_size(w, h)` (8138) takes the cut's `LayerRect` from the canvas's helper; `g.ensure_size`
  stays the frame. `PaintLayer::new`'s two callers (4714, 4793) may start from the frame's rect; the first
  `ensure_size` moves the layers to the paper.
- `app/src/doc.rs`: `commit_raster` (1092) is unchanged; its tiles may now be negative. `refill_at` (4935)
  passes the paper to the area forms.

**Acceptance criteria.**
1. `layer_rect` pins:
   - paper = frame 1920×1080 gives origin (0, 0) and size 1920×1080, not capped;
   - a 3000×1080 paper at x −540 gives origin (−540, 0) and size 3000×1080;
   - a 10000×1080 paper with `max_dim` 8192 gives an 8192-wide rect that holds the whole frame, `capped`.
2. `tile_span` with origin (−540, −64): a tile inside maps whole, a tile across the left edge maps only its
   right part, and a tile off the layer maps to nothing. A CPU mirror of upload then readback through
   `tile_span` gives back the same tile map, negative tiles included. The frame window a frame-sized target
   reads (`composite_layers_into`, through a pure helper) starts at layer texel (540, 64) for that origin and
   has the frame's size.
3. `flood_fill_mask_in` over a 3000×1080 paper at x −540: a seed at (−300, 500) inside an inked ring fills the
   ring's inside beyond the frame; a ring cut by the paper's edge fills only up to the edge; a seed at
   (−600, 500) is refused as `OffPaper`. Over the frame rect it equals `flood_fill_mask` bit for bit.
4. In the harness, on that paper: a fill click left of the frame is one undo step with tiles at negative x, and
   undo removes them; Shift+click refill and Ctrl+Shift+click replace-all there work the same way; a click
   outside the paper is refused with the Aka text and changes nothing.
5. On that paper, select-all selects the rect from (−540, 0) to (2460, 1080), and a lasso fill drawn across
   the paper's left edge fills only up to it (no texel left of x −540). The eyedropper at (−100, 500) picks the
   colour painted there, and a pointer moved there (a pointer-move event in the harness) sets `presence_pos` to
   that point. With paper = frame, all four are as before.
6. Paper = frame: the layer rect, a pinned fill mask and a pinned committed tile set equal their values from
   before this part.
7. The canvas keeps the `LayerRect` it asks the GPU layer for, with or without a GPU (the harness has none),
   from one helper that every `ensure_size` caller uses (canvas.rs twice and the viewer in main.rs; a source
   scan). It follows the cut on a cut switch and after "set paper"; a paper set while the stroke tap shows a
   stroke in progress changes it only after the stroke lands.
8. With the editor's limit set to 8192, the Paper size window shows the cap text for a 10000 px paper; for a
   3000×1080 paper it shows the memory, computed by a pure helper (pinned).

**Tests to add.** At least 8 tests named `extpaper_*`. The gate checks the count. A real GPU's textures cannot
be reached headless (NEVER-DO 7): the tests pin the geometry, the CPU fill and a CPU mirror of the tile mapping,
and the build log lists painting beyond the frame on a real GPU as not verified.

**Out of scope.** Tiled GPU layers to get past `max_texture_dimension_2d`; papers past the camera part's
`MAX_PAPER` and `MAX_PAPER_AREA`; the camera path overlay and camera motion blur.

**Stretch.** The onion and light-table slots at paper size (`set_onion`), then the other columns' slots
(`sync_other_column`); until then they keep the frame's size. If the cap is hit, the stretch goes to `left`.
The core (criteria 1–8) never does (D0): every `make_target` target, the graph's frame window, the cap and
memory text, and every tool above over the paper.

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p animstudio extpaper_ -- --list | grep -c ': test$') && [ "$n" -ge 8 ]`

## GM21 Menus, dialogs and Shift+Delete

*Depends on: GM2, GM3, GM5, GM7, GM8, GM11, GM13, GM14, GM18. Cap 45 min. Filter word: `tidy_`. Binding decisions: D0, D4, D6, D7, D8, D10, D11, D15; NEVER-DO 3, 5 and 9.*

**Goal.** Close the small gaps the done parts left in the shell, most important first:
- on Windows, Shift+Delete never reaches Delete frames (sheet) or Industry's Clear cel all (canvas);
- renaming a cut is the last scaffolding edit that cannot be undone;
- the New Project dialog keeps its own size table and ignores the templates' dpi and page;
- the sample cut is a Welcome button only, not a verb;
- Settings ▸ General does not say whether ffmpeg was found;
- palettes cannot move between projects (Stretch);
- there is no File ▸ Revert to saved (Stretch).

**User-facing behaviour.**
- **Shift+Delete on Windows.** egui-winit turns Shift+Delete into `Event::Cut` and sends no key event (Ctrl+X
  becomes the same event). `Chord::matches` (config.rs 1660) counts a Cut event with Shift held and Ctrl not
  held as a press of Delete with Shift. So Delete frames fires over the sheet, and Industry's Clear cel all over
  the canvas, once a press. Ctrl+X still cuts cells and fires neither. While a text field has focus nothing
  fires (the chord has no Ctrl, NEVER-DO 9), and the field cuts its text as before. A real Shift+Delete key
  event (another platform, the testkit) still fires once.
- **Rename cut is undoable.** `AppState::rename_current_cut` (doc.rs 3378) applies `Command::RenameCut`
  (command.rs 282, which has its exact inverse) instead of the engine's direct `rename_cut`. A rename is one
  undo step (D4); renaming to the same name adds none.
- **New Project from the templates.** templates.rs owns the sizes. `templates::SIZES` lists each template's size
  under its name, then Square 1080 (1080×1080) and Vertical 1080×1920, which have no template. newproject.rs
  keeps no table of its own (its `PRESETS` goes).
  - Picking a template in "start from" sets the size, the rate and the dpi from it. A page template gives its
    page's size and 350 dpi (D15).
  - With a page template, the size and dpi fields are greyed with "set by the page template". `new_state()`
    (newproject.rs 68) keeps the page's size and dpi; today it writes the form's 300 dpi and lets a typed size
    leave the page meta behind.
  - For every template, the dialog's project equals the Welcome card's (`templates::build`) apart from its
    name, and it is Srgb (D8).
- **Open the sample cut** is a verb: new Action `OpenSample` ("Open the sample cut", group File, menu
  "File/Open the sample cut", aliases sample, demo, example, tutorial; no chord in either keymap). Over unsaved
  work it asks "Save changes?" first, through a new `Leave::Sample` (main.rs 4350), like Open. Then it opens
  `sample::build_sample()` as the Welcome button does, through `go` (5406), so the canvas resyncs. A guest in a
  room is refused, as for Open.
- **ffmpeg on Settings ▸ General.** Under "ffmpeg location" the page shows "found: <path>", or
  `render_queue::NEEDS_FFMPEG` with "look again". It reads and fills the render queue's own cache
  (`RenderQueue.ffmpeg`), so the page and the pane always agree. The search reads files only and runs once per
  location, until "look again" or a changed location; never each frame.
- **Palette import and export** (Stretch, first). New Actions `ExportPalette` ("File/Export/Palette
  (JSON)…") and `ImportPalette` ("File/Import palette…"), aliases palette, colour model, iro shitei. The file
  dialogs are the shell's, through new `AppRequest::ExportPalette` and `AppRequest::ImportPalette` (like
  `ImportSound`).
  - The file is `{"format": "gaban-palette", "version": 1, "characters": [...]}`, the characters exactly as
    palette.rs serialises them, variants included. Export writes it atomically (D11) and is refused with "no
    character palettes yet" when there are none.
  - Import only adds. Each character joins the project's palettes, and a name clash gets " (2)" (then " (3)").
    The document becomes dirty (palettes live outside undo, as palette.rs says), and the status lane says
    "imported N characters".
  - A file that is not a palette file (bad JSON, another format tag, a newer version) is refused with the
    reason, and nothing changes.
- **Revert to saved** (Stretch, second). New Action `RevertToSaved` ("File/Revert to saved…", aliases revert
  to saved, reload from disk, discard changes; no chord).
  - It is greyed with "never saved: nothing to revert to" without a file, and "no unsaved changes" when clean.
  - Otherwise a modal asks "Revert to the saved file? The edits since your last save are dropped." with a held
    DANGER "Revert" (`plate::danger_hint`, like "Don't Save"; a click does nothing) and Cancel.
  - Revert lands any gesture in flight, then reopens the file through `AppState::open_path`. The document is
    clean, the title has no ●, and undo starts empty. Nothing is written (NEVER-DO 5).

**Files and types to extend.**
- `app/src/config.rs`:
  - `Chord::matches` (1660);
  - Action `OpenSample` (and with the stretch `ImportPalette`, `ExportPalette` and `RevertToSaved`) in
    `Action::ALL`, each with help, group, menu, aliases, context and no chord in either keymap (D6);
  - `general_page` (3711) takes the queue's ffmpeg cache.
- `app/src/main.rs`: `Editor::perform` arms and `can_perform` (5145) refusals; `Leave::Sample` (4350) and `go`
  (5406); the File menu entries. With the stretch: `AppRequest::ImportPalette` and `ExportPalette` (3015),
  answered by the shell's file dialogs, and the revert modal beside the "Save changes?" one (5648).
- `app/src/doc.rs`: `rename_current_cut` (3378); with the stretch, `import_palettes(path)` and
  `export_palettes(path)` through `palettes_mut()` (4838).
- Stretch: `app/src/palette.rs`: `PaletteFile { format, version, characters }`, `to_json`, `from_json` (the
  reason on a refusal) and `Palettes::merge(characters) -> usize`.
- `app/src/templates.rs` (`SIZES`, `Template`, `ALL`, `build`) and `app/src/newproject.rs` (`NewProjectForm`,
  `new_state` 68, `form_ui` 103).
- `app/src/render_queue.rs` (`RenderQueue.ffmpeg` 558, `NEEDS_FFMPEG` 32, its "look again" 1023) and
  `app/src/export.rs` (`find_ffmpeg` 31).
- Stretch: `app/src/fxui.rs` (a `ParamKind::Polys` arm), `shortcuts_page` in config.rs (3052), and the source
  inspector in `app/src/nodegraph_panel.rs` (around 1075).

**Acceptance criteria.**
1. Over the sheet, an input frame holding `Event::Cut` with Shift held, no Ctrl and no key event fires exactly
   DeleteFrames (Gaban keymap). Over the canvas with the Industry keymap the same frame fires exactly
   ClearCelAll. Ctrl+X's Cut event fires neither and still cuts the selected cells. With a text field focused
   nothing fires. A Shift+Delete key event alone still fires DeleteFrames once.
2. `rename_current_cut("CUT02")` adds one undo step. Undo restores the old name and the `content_hash`, and
   redo the new name. A rename to the current name adds no step.
3. `templates::SIZES` holds every template's size plus Square 1080 and Vertical 1080×1920, and newproject.rs
   has no size table of its own (a source scan). For every template in `templates::ALL`, the form's project
   equals `templates::build`'s in width, height, fps, dpi, columns and `cut_meta["manga.page"]`, and is Srgb.
   Manga B5 gives 2591×3624 at 350 dpi. With a page template picked, the harness finds the size and dpi fields
   disabled.
4. `OpenSample` has help, group File, its menu path and its aliases. On a clean document it opens the sample
   (`sample::SIZE`, `sample::FRAMES`). On a dirty one it holds the "Save changes?" modal, and a held
   "Don't Save" then opens the sample. A guest is refused.
5. Settings ▸ General in the PanelHarness: with the cache holding a found path it shows "found: <path>"; with
   none found it shows `NEEDS_FFMPEG` and "look again", whose click empties the cache. Thirty frames of the
   page run the search at most once (the queue counts its searches).
6. (Stretch) Palette: exporting, then importing into an empty project, gives the same characters, roles and
   variants (the JSON is equal). Importing a file with "Hana" into a project that has "Hana" adds "Hana (2)", leaves
   "Hana" as it was, marks the document dirty and reports the count. Bad JSON, a wrong format tag and a newer
   version are each refused with their reason, and the palettes and the dirty flag stay as they were. The
   Actions raise their `AppRequest`s; `ExportPalette` with no characters is refused with "no character
   palettes yet".
7. (Stretch) Revert: `can_perform(RevertToSaved)` is refused with "never saved: nothing to revert to" for an
   untitled project and "no unsaved changes" when clean. After a save to a temp folder and one edit, a held "Revert"
   reopens the file: the edit is gone, the state is clean, the title has no ●, and undo is empty. A click on
   "Revert", and Cancel, each change nothing.
8. Every new Action is in `Action::ALL` with help, a group and a menu path, and the zero-conflict test stays
   green in both keymaps.

**Tests to add.** At least 10 tests named `tidy_*`, from the core criteria (1–5 and 8) alone; the stretch adds
its own. The gate checks the count.

**Out of scope.** Adding and renaming scenes through Commands, and a Delete cut verb (no part promised them);
merging an imported palette into an existing character by role; a recent-palettes list.

**Stretch**, in this order (the first two were stretch in their own parts, ink and paint and never lose work):
- Palette import and export, as above (criterion 6).
- Revert to saved, as above (criterion 7).
- A `ParamKind::Polys` arm in fxui.rs: the Panels node's "panels" parameter shows "N panels: edit them with the
  panel (koma) tool (K) or the Manga pane" instead of a bare legend.
- A harness test of Settings ▸ Shortcuts: the Gaban and Industry preset buttons switch the keymap, and a
  planted clash shows its inline conflict label.
- A harness test of the source inspector's pass picker and its "iro-tore (absorb trace lines)" checkbox: each
  change is one undo step.

If the cap is hit, what is not built of the stretch goes to `left`; the core (criteria 1–5 and 8) never does
(D0).

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p animstudio tidy_ -- --list | grep -c ': test$') && [ "$n" -ge 10 ]`

## GM22 View3D worker, sequence reload and the 3D underlay

*Depends on: GM4, GM6, GM9, GM11, GM12, GM17, GM20, GM21. Cap 45 min. Filter word: `worker3d_`. Binding decisions: D0, D1, D7, D9, D10, D15; NEVER-DO 2, 6 and 7.*

**Goal.** The 3D layout part's three open items, most important first:
- the View3D pane renders on the UI thread, so a heavy model stalls orbiting, scrubbing and the pen;
- an image sequence re-rendered on disk shows only after an edit;
- the canvas cannot show the 3D layout under the drawing (the first item that part sent to `left`).

**User-facing behaviour.**
- **View3D on a worker.** The pane hands each render to a CPU worker, following the Final look in viewer.rs
  (`FinalLook`, 46-160): one keyed job at a time, and the newest key wins (D9).
  - While a job runs, the pane keeps the last picture with the TALLY dot ("rendering"). A job that lands after
    the key moved on still replaces the picture (it is newer than the one shown), as the Final look does, and
    the next job is for the newest key: the keys in between are skipped, so an orbit's picture follows the drag
    at the render's pace instead of freezing until the drag stops.
  - A render that fails or panics is said once on the Aka lane, and the same key is not tried again every
    frame.
  - The key stays `render_key` (view, frame, size, node, model hash, law, frame width). Orbiting, scrubbing and
    resizing never wait for a render (NEVER-DO 6).
  - The worker gets owned inputs (the model asset, the node's recipe at the frame, the view camera, the size),
    never a borrow of the cut, and runs `render_view`'s maths unchanged (NEVER-DO 2).
- **Sequence reload.** An ImageSequence's eval value folds the shown file's length and modified time whenever
  its path resolves: an absolute pattern always (an untitled project's imports are absolute), a relative one
  when the project folder is known (`AppState::project_dir()`, handed to the evaluator). That is a stat only,
  never a read, at most once per node and frame between invalidations. Two things invalidate the sequence nodes:
  - the window regaining focus (you come back from the 3D app that re-rendered them);
  - a new Action `ReloadSequences` ("File/Reload image sequences", aliases refresh, reload renders, reload
    sequence; no chord).

  Neither is a document edit: no undo step, and the document stays clean. A file that did not change keeps its
  value, so the composite, the FX cache and the RAM preview's `FrameCache` all hit; a changed file misses only
  for the frames that show it. A relative pattern without a project folder keeps today's value.
- **3D underlay.** A View ▸ 3D underlay latch: new Action `Underlay3d`, context Canvas, no chord, off by
  default, kept for the session.
  - It draws the cut's model node (`view3d::model_node`) under the drawing, on the frame rect, at the light
    table's opacity (`LightTableCfg.opacity`, 0.45). The picture is the node's own CPU render at the current
    frame (`anim_core::export::render_node_px_env`, the graph's path), so it shows what the composite shows.
  - It is greyed with `view3d::NO_MODEL` ("no Model3D node — File ▸ Import 3D model…") when the cut has none.
  - It renders on the same worker, keyed by (node, eval value, size, colour law), at most `MAX_RENDER_W` wide.
    The node's eval value already moves with what changes per frame (a keyed camera), so a still model
    scrubbed to another frame reuses the last picture. It never starts a job mid-stroke, and while playing it
    holds the last picture that finished.
  - It is view only: no undo step, not dirty, never saved or exported. The canvas records it as drawn, beside
    `Furniture` (canvas.rs 460): node, frame, rect and opacity.

**Files and types to extend.**
- `app/src/view3d.rs`: `View3dState` (45) holds the job and its key; `ui` (254) asks the worker instead of
  calling `render_view` (211) inline; `render_key` (239), `model_node` (112), `MAX_RENDER_W` (30), `NO_MODEL`
  (33).
- `app/src/viewer.rs`: `FinalLook` (46-160) and `GraphView` (24), the pattern to follow.
- `crates/anim-core/src/eval.rs`: the ImageSequence arm (257-267) folds the stamp; `Evaluator` (22) gains the
  project folder (`set_base`); `invalidate_nodes` (278) serves the reload.
- `crates/anim-core/src/r3d.rs`: `sequence_stamp(path) -> Option<(u64, u128)>` (length, mtime in ns) beside
  `sequence_file` (1480).
- `app/src/doc.rs`: `project_dir()` (951) feeds the evaluator whenever the file path changes, and
  `reload_sequences()` invalidates every ImageSequence node and what is downstream of it.
- `app/src/canvas.rs`: the underlay under the below stack, `LightTableCfg` (7538), `Furniture` (460).
- `app/src/playback.rs`: `FrameCache` follows the eval value as it does today (a changed hash drops the frame).
- `app/src/config.rs`: Actions `ReloadSequences` and `Underlay3d` (D6). `app/src/main.rs`: their `perform` and
  `can_perform` arms, the View and File menu entries, and the focus hook (`egui::Event::WindowFocused(true)`).
  The 3D layout room's help (config.rs 781) and its Room-menu hover (main.rs 7696) stop saying the underlay is
  not in this build.

**Acceptance criteria.**
1. A key change in the View3D pane starts one job; the pane keeps the last texture and shows the TALLY dot
   until the job lands. `ui` never calls `render_view` itself (a source scan).
2. The job logic, driven without a thread (one pure state machine that the pane and the underlay share): with
   key A's job pending, wanting B and then C starts nothing; A's result then replaces the picture with the
   TALLY dot still on, and the next job is C's (B is never rendered); C's result clears the dot.
3. A job that fails (a model that does not parse) is reported once on the Aka lane, and the same key is not
   resubmitted on the next frames. A worker that panics (its channel disconnects, fed to the job logic in a
   pure test) is handled the same way.
4. With a project folder, or with an absolute pattern and none, the ImageSequence value changes when the shown
   file's length or mtime changes, and stays equal when neither does. A relative pattern without a project
   folder keeps today's string (pinned).
5. `ReloadSequences`, and an `Event::WindowFocused(true)`, invalidate exactly the sequence nodes and what is
   downstream of them. After one file in a temp folder is rewritten, the next eval differs only for the frames
   that show it. Neither adds an undo step or makes the document dirty.
6. `Underlay3d` is refused with `NO_MODEL` on a cut with no Model3D node. On a cut with one (the 3D part's
   test cube, `CUBE_OBJ` in view3d.rs) it turns on, and the canvas records an underlay on the frame rect at
   opacity 0.45 for the current frame. Toggling adds no undo step and leaves the document clean, and an export
   renders the same pixels (a pinned FNV) with it on or off.
7. The underlay key is (node, eval value, size, colour law): a still model scrubbed to another frame reuses
   the last picture and starts no job, while a keyed camera's next frame starts one; a stroke in progress (the
   stroke tap) starts no underlay job; while playing, the canvas holds the last finished picture.
8. Both new Actions have help, a group and a menu path, and neither keymap has a conflict. Neither the 3D
   layout room's help (`Action::Layout3dRoom`) nor the Room menu's hover says the underlay is not in this
   build (the hover by a source scan of main.rs).

**Tests to add.** At least 7 tests named `worker3d_*`. The gate checks the count.

**Out of scope.** A GPU 3D renderer; the underlay in the viewer or under the onion frames; a file watcher
thread for sequences; the 3D part's stretch (turntable, 2× supersampling, glTF textures and animation, shadow
mapping).

**Stretch.** An underlay node picker when the cut has several Model3D nodes (the default stays
`view3d::model_node(cut, selected)`), and an underlay opacity slider in the lightbox fold-out. If the cap is
hit, the stretch goes to `left`; the core never does (D0).

**Gate.** `cd /c/Users/Lancer/Desktop/GabanStudio && export PATH=/c/Users/Lancer/.cargo/bin:$PATH && cargo test -p animstudio -p anim-core && n=$(cargo test -q -p animstudio worker3d_ -- --list | grep -c ': test$') && [ "$n" -ge 7 ]`

---

## Build log

Each part adds one dated line when it finishes: what was delivered, test counts, what went to `left`, and
what was not verified.

- 2026-10-28 — Room planned: 18 build parts and the ship part, in plan `gaban-modern`. No code changed yet.
- 2026-10-29 — Plan reviewed (begun 2026-10-28), still 18 build parts plus ship, all pending:
  - every cap is now 45 min;
  - the X-sheet part is split into its engine (GM5) and its panel, which joins the graph editor (GM14, runs
    sixth);
  - episode export, the EDL and the colour sheet moved from delivery to the cut board part;
  - the room split moved to the welcome part;
  - every gate now checks its test counts with `-- --list`;
  - the ship gate adds a smoke render and a no-tag check.
  No code changed.
- 2026-10-29 — The testkit part: `testkit.rs` (EditorHarness, PanelHarness over egui_kittest 0.35), 19
  `testkit_*` tests; `shortcuts.rs` with the text-focus rule; `Editor::perform` -> `Performed`/`AppRequest`;
  tolerant config and workspace loads with `.bad` copies and one note; atomic `.tmp` + rename saves;
  `proc::command` with CREATE_NO_WINDOW; the release build is GUI-subsystem; accessible names on the
  painted plate controls and room tabs; the layout probe covers all four rooms at 1.0, 1.25 and 1.5 (it
  now installs the app's fonts — before this it could not draw the room tabs). Colour literals pinned at
  86. Left: the probe at 2.0 and a PanelHarness over the node graph pane (stretch). Not verified: a
  release build's console behaviour and the relaunch with CREATE_NO_WINDOW (both need a real window).
- 2026-10-29 — The never-lose-work part: `Engine::revision()` (unique serials; undo/redo return to the
  saved value; trimmed history keeps a base; out-of-order session undo gets fresh serials) with 3
  revision tests; `store::save_atomic` (copy to `.tmp`, save, keep `.bak`, rename — unknown tables
  survive); `AppState::{saved_rev, meta_dirty, is_dirty()}` with background saves from a snapshot;
  scaffolding and palette edits mark dirty; `window_title` with ●; the slate dot reads `is_dirty`; window
  close, New, Open and Open Recent go through a "Save changes?" modal (Save / held Don't Save / Cancel;
  untitled Save asks the shell for Save As); new `worksafe.rs` (autosave clock, autosave paths and prune
  to 3, running.json, recovery candidates and the recovery modal, crash log and panic hook);
  Settings ▸ General autosave interval; 12 recent files in the session config. 15 `worksafe_*` tests;
  colour literals unchanged at 86. Left: File ▸ Revert to Saved (stretch). Not verified: the real window
  close and title, the rfd dialogs, the panic hook in a real crash, the recovery modal at a real start-up,
  and two app instances sharing one running.json.
- 2026-10-29 — The never-lose-work review fixes: a document carried across a rebuild or update relaunch
  comes back dirty, and the relaunch takes this copy's running record with it (put back if it fails); one
  running record per copy (`running-<pid>-<started>.json`) with a liveness check, so a second window is
  never offered as a crash; shortcuts and the update check stand down while "Save changes?" or Recovery
  is up (Enter saves, Esc cancels); Ctrl+S during a save queues one more instead of refusing, and a close
  waits for a landing save; Recovery lists every autosave (local time and age, newest picked, untitled
  wording) and Open recovered goes through the unsaved guard; DERIVE and SET on the swatch board mark
  dirty; a close outranks a pending New/Open; the held DANGERs carry hints; the slate dot is named
  "unsaved changes"; Open Recent shows the parent folder, checks the files once as it opens, and can clear
  the list; a failed autosave says so once per streak on the Aka lane, a landed one leaves "autosaved
  HH:MM"; Settings ▸ General names the autosave folder and opens it; every untitled document has its own
  autosave group; the per-frame clones are gone. 31 `worksafe_*` tests; colour literals unchanged at 86.
  Not verified: two real copies side by side, the relaunch hand-off, the opened folder window, the failed
  autosave line (needs a real worker), and the App's own shortcut gate (the testkit mirrors it).
- 2026-10-29 — Menus, command palette, history and help: the registry gives every action a help line,
  a group, a menu path (the deck's tools are palette-only) and aliases; the slate's row carries file, edit,
  view, sheet, room (was ws), window (was panes), cut and help, every item showing its chord, a one-line
  tooltip, and greyed with the reason when `Editor::can_perform` says no; clicks go through
  `Editor::invoke` (the one dispatch). Ctrl+K / F3 opens the palette (name > alias > help scoring, recent
  first, Enter runs, Esc closes, the field keeps the keys); F1 lists every bound action, grouped and
  searchable; Help has Quick start and About. History pane (`Pane::History`, Edit ▸ History): label and
  author per step, current armed, saved point marked, a click jumps by single steps
  (`Engine::history/position/undo_to`). Ctrl+Shift+Z redoes beside Ctrl+Y (`Binding.also`). Keymaps
  Gaban and Industry on Settings ▸ Shortcuts (overrides kept on top; conflicts said inline); shortcuts
  resolve a shared chord by the pane under the pointer, else the focused one. Deck tooltips read the live
  chords. 14 `cmdui_*` tests + 2 engine tests; colour literals unchanged at 86. Left: the palette does
  not search pane names or settings (stretch); cut sits after window on the slate (its old place).
  Not verified: the real window's menus and F3, the Settings ▸ Shortcuts preset detents (config tested,
  not the page), and the App's own palette gate (the testkit mirrors it).
- 2026-10-29 — GM4 canvas view and pen aids: `ViewXform` (zoom, pan, turn, display-only mirror) maps
  screen↔paper for every tool; paper images are one textured quad each (`paint_paper_image`). Space
  hand: drag pans, Shift+drag turns (Ctrl snaps 15°), PlayPause moved to Space's release (a tap). Nine
  new Actions in both keymaps: zoom in/out on a pinned ladder (Ctrl+= / Ctrl+-), 100% (Ctrl+1), rotate
  ±15° (Ctrl+[ / Ctrl+]), reset (Ctrl+\), mirror (H), brush size `[` / `]` on a size ladder; egui's
  own keyboard UI zoom is off so Ctrl+= / - / 0 reach them. Hardness and aliased in the OPTIONS row and
  the preset; aliased rides the dab's hardness (`ALIASED_HARDNESS`, the wire's Dab is unchanged) with a
  CPU mirror `dab_coverage`. Stabilizer 0–10 (pulled string + light smoothing, `stabilizer.rs`, tail
  reeled to pen-up), Shift+click rules a line from the last stroke, Alt+click picks the composited
  colour. Stroke tap (`stroke_tap`) for the harness. 17 `viewnav_*` tests; colour literals unchanged at
  86. Left: two-finger touch rotation, per-viewer flip (stretch). Not verified: the real window, the
  mesh and aliased shader on a GPU, tablet tilt on hardware, and the Legacy colour law on a repainted
  pick (GM7).
- 2026-10-29 — The X-sheet engine part (GM5), engine half: schema v11 with the side tables `cut_meta`,
  `column_defs` and `cut_memos` (created on save, probed on load; a v10 file loads as v9 plus tables it
  does not know, which survive re-saves; v12 and later are refused as "made by a newer Gaban Studio");
  `Cut.meta`, `Cut.memos`, `Cut.column_defs`; `ColumnKind { Cel, Bg, Book, Ref }` and
  `ColumnDef { kind, hidden, locked }`; `Cut::render_order()` in the CPU column render, `composite_pixel`,
  the CPU graph render, eval and the GPU graph compositor (a hidden or Ref DrawingSource is transparent).
  16 new Commands with exact inverses (Add/Remove/Rename/MoveColumn, Add/RemoveParamColumn,
  Add/Remove/RenameCut, SetFrameCount, Insert/DeleteFrames, SetCells, SetCutMeta, SetColumnDef, SetMemo);
  a locked column refuses cell edits, paint and frame ripples with "column X is locked". New
  `sheet_ops.rs`: `hold_delta`, `retime_range`, `reverse_range`, `expose_by_name`, `copy_block`,
  `paste_block` (Overwrite / Insert). Raster tiles now serialize as (coord, tile) pairs so a cut snapshot
  passes the serde_json Command wire. 25 `sheet_edit_tests`; the layer test's current-schema check reads
  11. Left: the app half — doc.rs scaffolding through the Commands (with the cursor fixed up after an
  undo), the canvas below/above stacks on `render_split`, the config.rs sheet Actions with their aliases
  and perform arms, and the app-side lock refusal before a stroke starts. Not verified: the canvas and
  the app with kinds and locks (no app path sets them yet), and the GPU compositor's skip on a GPU.
- 2026-10-29 — GM5 review fixes, the app half: doc.rs adds and removes columns, param columns and cuts
  through AddColumn / RemoveColumn / AddParamColumn / AddCut, one undo step each; `keep_view_live` puts
  the view back on a live cut and column after an undo, a redo or a peer's edit. 14 sheet Actions on the
  sheet cursor (Sheet menu; palette words ones, twos, threes, hold, insert frames, delete frames, reverse,
  haikei; the same chords in both keymaps: Insert, Shift+Delete over the sheet only, = and -, Alt+1/2/3,
  Alt+V reverse, Alt+C/B/K/G kinds, Alt+H hide, Alt+L lock), each refusing on the Aka lane with the
  reason. The canvas stacks come from `render_split`: hidden and Ref columns never draw, a BG draws under
  the active cel, a later cel or a Book over it. A locked column refuses the stroke at pen-down ("column A
  is locked: unlock it in the sheet to draw"). Engine: DeleteFrames re-keys the first frame after the
  range (the fuzz checks it); the lock covers the cel-layer edits, RemoveColumn and MoveColumn; no cut
  has 0 frames (add_cut, load, AddCut); refusals count frames from 1; Hold +1 on the last drawing says
  why; a multi-column retime pads every column to one end and kara keeps its length; a cross-cut paste
  renames a copy whose name is taken; badges Cel / BG / Book / Ref with tooltips; `Engine::remove_column`
  drops the def. 32 `sheet_edit_tests`, 9 `sheetedit_*` app tests, and the GPU compositor's source
  selection tested without a device. Left: a column kind from a later build still reads as Cel and is
  re-saved as Cel (needs a raw pass-through of the def row; first needed by the camera part). Not
  verified: the real window (menus, chords over the real sheet pane) and the GPU compositor on a GPU.
- 2026-10-29 — The sheet panel and graph editor part (GM14). Engine: `ParamInterp` gains EaseIn (t²),
  EaseOut (1−(1−t)²) and Bezier, stored in the existing interp text ("ease_in", "ease_out", "bezier" or
  "bezier:in_dt,in_dv,out_dt,out_dv,mode"; no new column, old words read as before). `Handles` with
  auto / smooth / flat / broken modes (auto is the Catmull-Rom slope at 1/3 of each segment, so two auto
  keys are exactly Linear); handles are refused on a non-Bezier key. `BezierSeg` clamps a handle into its
  segment and solves time monotonically (Newton inside a bisection bracket); `ParamColumn::sample` (f64),
  `handles_at`, `segment`. `Command::SetParamKeys` (one batch, validated whole, exact inverse) and
  `sheet_ops::move_param_keys` (whole-frame shifts; a moved key replaces the key it lands on, never a
  duplicate). App, the sheet panel: a range by click, Shift+click or drag feeds retime and reverse; Copy /
  Cut / Paste cells (Ctrl+C / X / V, Ctrl+Shift+V inserts, over the sheet only) through `copy_block` /
  `paste_block`, also answering the system clipboard's Copy / Cut / Paste events that egui-winit sends in
  place of those keys; a typed entry on the cursor's cell (a name and Enter exposes it by name, x empties,
  Enter moves down, Tab moves right; a text field, so B or 1 typed there fires nothing); a kind badge and
  H / L toggles per column header; a memo column and memo field (SetMemo); a length field in frames or
  seconds+frames (SetFrameCount); rows through `show_rows`. The Curves pane (panes menu): a column list,
  curves in plate tokens, click / Shift+click / box select, key drags snapped to whole frames and handle
  drags as one SetParamKeys each, interpolation and handle mode from the context menu, F frames the
  selection or everything (taken before the sheet's F), wheel zoom, Alt- or middle-drag pan, the
  current-frame line. 12 `curve_tests`, 7 `curves_*` and 7 `sheetui_*` app tests. Left: copying and
  pasting keys between columns, an ease preset library, and a room that docks the Curves pane by default.
  Not verified: the real window (the system clipboard round trip, Tab in the entry, wheel zoom), and on
  Windows Shift+Delete arrives as a Cut event, so GM5's Shift+Delete chord does not reach the sheet there
  (found here, not changed).
- 2026-10-29 — The light table and inbetweening part (GM6). Engine: `DrawingKind` (unmarked / genga key /
  breakdown / douga inbetween) and `SpacingChart` in a `DrawingDef` kept per cut beside the drawings (never
  in `content_hash`), `SetDrawingDef` with an exact inverse (RemoveDrawing drops and restores the def), the
  v11 side table `drawing_defs` (no version bump; a file without it loads empty), `prev_key` / `next_key`,
  and the pixel clipboard `copy_region` / `paste_region` (src-over, one PaintTiles). App: the onion's depth
  0–5 each side, drawings / frames / keys mode, other columns, opacity and falloff in the config's `onion`
  section (depth 1 drawings equals the old pair on every frame); onion slots grow to three (behind, ahead,
  light-table pins), tints unchanged; the light table replaces the single ghost pin with pins shown on every
  frame at the `light_table` opacity, shift & trace (a drag moves the selected pin, Shift+drag turns it, the
  pen rests; reset), view state only; kinds circled / underlined on the sheet with a chart glyph and a
  spoken cell name; spacing-chart presets; make inbetweens (one undo step: blank inbetweens named between
  the keys or lettered, keys between cleared); duplicate cel; Copy / Paste pixels (Ctrl+C / Ctrl+V on the
  canvas). 11 verbs. 4 `layer_tests` and 18 `lighttable_*` app tests; the colour-literal pin drops 86 → 83.
  Left: a sheet-cell context menu for the kind, the chart glyph on the canvas itself, Copy / Paste through
  the system clipboard events. Not verified: the real window, the GPU light-table slot and its shifted tiles
  on screen.
- 2026-10-29 — The ink and paint part (GM7). Engine: `color.rs` with `ColorLaw` (Legacy / Srgb) kept in
  `meta.color_law` (a file without it loads Legacy; an unknown value is refused as corrupt), the CPU Solid
  node and the GPU graph compositor's Solid both encoding by the law (`render_graph_frame_law`), the
  Legacy-to-sRGB convert (exact Legacy chip channels snap to the Srgb chip, other texels go through the
  float OETF, images untouched), `recolour` over a cut, scene or project (Color / Shadow / Highlight /
  Custom roles, locked layers and columns skipped, one PaintTiles per layer), `refill` (bounded by the
  cel's line layers, grown by the fill grow) and `replace_all`, and `paint_check` (holes and stray colours,
  each drawing read once at its first frame in range). App: New Project makes Srgb projects (the engine and
  `AppState::new_project` stay Legacy); the brush and fills ink through `CanvasView::ink`, the eyedropper
  decodes by the law and arms the matching chip; Shift+click with Fill refills, Ctrl+Shift+click replaces
  every match; a chip edit on the swatch board offers Recolour this cut / this scene / project / Keep
  paint; File ▸ Convert colours to sRGB… (held DANGER; the backup `<stem>.pre-srgb.animproj` beside the
  file, history cleared, the document dirty, the user's file untouched); Window ▸ Paint check (whole cut or
  frames, hole size, chips of every character or one; a worker thread; a finding jumps to its frame, column
  and box). 17 `color_tests`, 6 `inkpaint_*` app tests. Left: the "show unpainted" overlay and palette JSON
  import / export (stretch). Not verified: the real window, the GPU Solid on screen, and the Shift /
  Ctrl+Shift fill click through the harness (the engine side is tested).
- 2026-10-29 — GM7 review fixes: Convert colours to sRGB… moves to a new Project menu that says the
  colour law ("colour law: legacy (linear, v0.3)", also under the item's tooltip); an unknown colour law
  is refused as a newer file (`EngineError::NewerColorLaw`), not as corrupt. A fill click lands on release:
  the canvas senses clicks too, so a still press started no drag and filled nothing. Shift+click refill
  and Ctrl+Shift+click replace keep inside the selection (a click outside it says so) and are named in
  the fill status line, the deck tooltip, the help line, the palette words (refill, nuri-naoshi, replace
  colour), F1's new canvas-gesture rows and quick start line 7. The paint check is the nuri check (paint
  check): palette words nuri, kensa, shiage kensa; Ctrl+Shift+K in both keymaps; tabbed behind the palette
  in the finishing room. Its hole wall is the visible art without Reference / Rough / Correction layers;
  part-covered paint is one soft edge per layer, not strays; a fresh frame range starts at the playhead;
  the list belongs to the cut it checked (another cut's list is hidden and its jump refused), turns stale
  when the paint moves, builds only the rows in view, and a jump arms the finding's layer. The swatch
  board rings one pot (the active chip, kept while the brush holds its colour; the pot lens arms it too);
  the recolour buttons say what they touch and name another character that shares the old colour; a
  dropped offer is said. The Legacy pins are the pre-GM7 build's hashes (built from 27cbf36). 20
  `color_tests`, 13 `inkpaint_*` app tests (the Shift / Ctrl+Shift click now through the harness). Left:
  a project-scope recolour and the colour-law convert still run on the UI thread (a worker needs
  stale-diff protection). Not verified: the real window.
- 2026-10-29 — GM8 Welcome, templates, rooms and the sample cut. Start-up shows the Welcome screen
  (welcome.rs) unless its "Show at startup" latch is off: four template cards (templates.rs: Srgb law,
  `default_cut_frames`, columns with BG / Ref kinds, the first room, an auto-comp graph), Custom size…,
  Open…, 12 recent cards read through `store::read_meta` (missing files greyed with Remove), the sample
  cut (sample.rs: 640×360, a BG plate and a 12-drawing ball bounce on twos, keys marked, a Ball palette)
  and the Quick start checklist. Rooms (D12): `Stage::InkPaint` (key still "finishing") and
  `Stage::Composite` ("composite", Shiage (full); graph + curves, viewer with the canvas behind it,
  palette; composite view on); a room missing from a saved `room_stack` gets its built-in binding; the
  slate tab's hover is the room card. `Command::SetProjectProps` (exact inverse, all nodes invalidated)
  behind Project ▸ Project settings…; Project ▸ Build composite from sheet (one undo step, byte parity
  with the column render). `Project.default_cut_frames` (meta, unset = 48). The save worker writes a
  256-px base64 PNG thumbnail (≤ 64 KB) into `app.thumb`; `load` leaves it out of the document. UI scale
  80–200% in Settings ▸ General (`ui.scale`, egui's zoom factor). Quick start ticks persist in
  `ui.quick_start`; Help ▸ Quick start gains the checklist with Show me. 13 `welcome_*` tests and a
  composite-room probe. Left: newproject.rs still keeps its own resolution presets; the role chooser
  (stretch). Not verified: the real window.
- 2026-10-29 — GM9 Compositing FX core. Engine: `fx.rs` (premultiplied f32 `Image`; `ParamSpec` with
  Float, Color, Bool and Choice kinds; `FxParams` whose missing keys take the spec default, `FxBinds`;
  `FxKind` Blur, Glow, Diffusion, Grade with `specs`, `inputs`, `apply`; rows in parallel with rayon
  1.12). Formulas pinned in code and tests: blur σ = r/2 as three box passes (boxes-for-Gauss),
  transparent outside; glow `bright = max(luma − t, 0)/(1 − t)` × colour × tint (the tint through the
  colour law), `out = in + blur(bright)·intensity`, an optional matte input gates it; diffusion
  `mix(in, screen | soft light(in, blur(in)), mix)`; grade on unpremultiplied colour, saturation about
  Rec.709 luma, temperature and tint as ±25 % channel gains. `NodeKind::Effect { fx, params, binds }`
  (serde defaults: old files load unchanged), `references(param)`; the eval hash spells every resolved
  value; the export's `render_node` Effect arm and `render_node_px`; the `graph.layout` cut meta (its
  SetCutMeta invalidates nothing). App: graphcomp `needs_cpu`, `subtree_needs_cpu`, `cpu_roots`, a
  worker thread rendering from a `Cut` snapshot, an upload cache keyed (node, frame, eval hash) whose
  bookkeeping (`FxCache`) is apart from the device, the last result plus a TALLY dot
  (`GraphView::Rendering`) while a render is pending; the node graph's searchable add-node menu (Tab or
  a right-click on empty space; Source, Transform, Blend, FX, Output; by name, category or alias; placed
  at the pointer in one undo step; Build composite from sheet in its foot); node positions saved in
  `graph.layout` (add, remove and import carry it in the same step; a drag is one step); `fxui.rs`, the
  inspector from specs (drag value with its range, colour button, latch, combo, a bind picker per
  bindable parameter; one gesture, one step). 17 `fx_tests`, 6 `fxnode_*` app tests. Left: the stretch
  (a glow driven only by its matte); colour parameters cannot be bound. Not verified: the GPU path on a
  real GPU (upload, blit, the dot) and the real window.
- 2026-10-29 — GM9 review fixes. Node places moved from the one `graph.layout` map to a key per node,
  `graph.layout.<node id>` = `[x, y]` (`graph::layout_key`, `place_node`, `layout_from_meta`): an add,
  move or removal touches its own node's key only, so a guest's step is never refused over another
  artist's places and `undo_last_by` reverts nobody else's moves; a node never placed by hand takes the
  same column slot after a load. `Action::AddNode` ("Add node…", group Composite, Project ▸ Add node…,
  Tab in both keymaps, context Node Graph) opens the add menu; the graph's hints name its chord. Input
  pins are named in the node body (Legend dim) with a tooltip of what each takes
  (`FxKind::input_names`, `input_help`). Every parameter is bindable: a bound colour mixes its chip toward
  a second chip (`<key>.to`, shown only while bound) by the column's 0–1, per byte. Diffusion's "soft
  light" is now "overlay", what it always ran (old files read through `ParamKind::Choice::renamed`; "soft
  light" stays a search alias), with values pinned where the soft copy differs. Blur gains "repeat edges".
  Studio terms beside the English in the add menu and as aliases: bokashi, tōkakō / T-kō, difyūjon; Grade
  labels "temperature (warm–cool)", "tint (magenta–green)". A new column is "Glow radius", then "Glow
  radius 2"…; hover text on the kind label, combos and bind pickers. graphcomp: a job's `Cut` holds only
  what its subtree shows at that frame (`job_cut`); a render that panics answers no pixels, the lamp goes
  out and that key is not retried (`FxCache::fail`); the views say "rendering FX…" beside the TALLY dot.
  21 `fx_tests`, 11 `fxnode_*` app tests. Left: the stretch; a per-FX "Add effect ▸" submenu; a removed
  unplaced node lets the unplaced nodes after it close ranks at the next load. Not verified: the real
  window and the GPU path on a real GPU.
- 2026-10-29 — GM10 Camera, paper and multiplane. Engine: `camera.rs` (paper in `cut_meta["camera.paper"]`,
  the frame stored as no key; `CAMERA_SPECS` through the GM9 spec system, every one bindable, resolved by
  the shared `fx::resolve_specs`; the sample map, parallax `1/(1+k·z)` floored at 0.05, seeded bounded
  shake, DOF radius, `field_guide`, `safe_frames`, `frame_rect_on_paper`, `output_camera`).
  `NodeKind::Camera { planes, params, binds }` (one pin per plane, `PlaneSpec { depth }`); export renders
  each plane at the paper rect, blurs it by DOF with the FX core's blur, samples it bilinear through the
  map and lays the planes far→near (stable pin order); `render_node_rect` renders any rect of the frame
  (negative tiles too); the default path is byte-identical (pinned FNV of the cam.x/cam.y/cam.scale
  Transform graph taken before the change). The eval recipe names the camera, its paper and its planes.
  App: the Camera node in the add menu (Transform, aliases satsuei/multiplane/T.U./T.B.…), pins named per
  plane, an inspector from the specs (the FX inspector's spec core, shared) with depth per plane and
  + plane / − plane; graphcomp `needs_cpu(Camera)` and a job's cut keeps the paper key; the canvas's field
  and safe toggles draw the camera module's rects (field numbers, centre cross) through the view, the
  extended paper's outline and the output camera's frame (AKA), recorded as drawn; the viewer gets a safe
  latch and overlay; cut ▾ paper x/y/w/h with "set paper" (one undo step) and "frame". 14 `camera_tests`,
  5 `camview_*` app tests. Shake frequency is in cycles per frame (the camera knows no fps). Left: paint
  layers at paper size (GPU cap shown), fill/lasso clipped to the paper, painting beyond the frame, the
  stretch (camera path overlay, motion blur). Not verified: the real window and a real GPU.
- 2026-10-29 — GM10 review fixes. The paper is bounded by area as well as side: `MAX_PAPER_AREA` 32 Mpx
  (a camera renders each plane as an f32 buffer of the whole paper, so 16384² would need 4 GiB a buffer
  and abort the render); the frame is always a valid paper (an 8K frame past the budget still clears the
  key). `Action::PaperSize` ("Paper size…", group Cut, Cut ▸ Paper size…, aliases paper/hiki/BG pan/pan
  paper/kami/BG sheet, no chord) opens a Paper size window in place of the cut ▾ fields: "paper (BG
  sheet)" with corner and size fields, presets 1.5× wide / 2× wide / 2× tall centred on the frame and
  "paper = frame", the limits and the draft's Mpx shown, "set paper" one undo step; the draft belongs to
  the cut it was opened on and re-reads on a cut switch. The field guide, its numbers, the safe frames
  and the centre cross ride the output camera's z 0 map (`Furniture::view`/`quad`), so they follow a
  panned, zoomed or turned camera; with no camera they sit on the frame as before. The camera frame is
  Ao at 2 px, not Aka. 15 `camera_tests`, 7 `camview_*`. Still Left, a core item that needs its own part:
  paint layers at paper size (origin offset through upload, readback, brush uniforms and display; the
  device's `max_texture_dimension_2d` cap shown), painting beyond the frame, fill/lasso clipped to the
  paper; also the camera frame shown only in the Layout/Composite rooms (the canvas knows no room).
- 2026-10-29 — GM11 Render queue and delivery. Engine: `deliver.rs` — `Purpose { View, Final, LineTest }`
  (`keeps(role)`: Final drops Reference, Rough and Correction; LineTest keeps the line roles),
  `RenderOpts { purpose, scale }`, `render(cut, law, frame, w, h, &opts)` (View is today's render on either
  path; Final renders the graph on a role-filtered clone of the cut, or the column path through
  `Drawing::flatten_for`, Ref columns skipped as before; LineTest is the column path's line roles, black on
  white; scale under 0.75 is a 2×2 box half), `write_png16`/`write_tiff16` through image 0.25, `write_exr`
  half through the exr crate (image writes EXR only as f32), scene-linear by the colour law, `write_png8`,
  `ffmpeg_args` per codec (H.264 exactly v0.3's, H.265, ProRes 422 HQ and 4444, DNxHR HQ; the audio as v0.3
  muxed it, `-ss`/`-t` kept, AAC 192k or `pcm_s24le`), ten `PRESETS`, `expand_pattern`/`video_name`
  (sanitised), `burn_in` (Plex Mono through ab_glyph on a 60% black band in the four corners; s+k and
  HH:MM:SS:FF), `deliver_frame`, `parse_cli`. `text.rs`: the Plex Sans and Mono statics (the app's fonts now
  come from them), `layout_line`, `raster_line`. `flatten`, `render_frame` and `render_graph_frame` keep
  their results. App: `render_queue.rs` — a pure `Queue` (in order, one at a time; a done or failed job
  hands on, a stopped one stops the run), a `JobSpec` snapshot taken at the start, `render_job` (cancel read
  before each frame; video piped as rawvideo RGBA into ffmpeg through `proc::command`), a worker with a
  channel and an AtomicBool; the Render Queue pane (preset detents, the video ones disabled with "needs
  ffmpeg (ffmpeg.org); image sequences work without it", burn-in latches, folder, pattern, + this cut,
  + every cut in the scene, + frames, Start / Stop / clear finished, progress bars, "show folder"), tabbed
  into the Composite and Edit docks and listed in the panes menu, polled every frame by the shell.
  `find_ffmpeg_in(configured, exe_dir, path_var, local_appdata)` checks files only (PATH's folders scanned,
  nothing spawned); `find_ffmpeg(configured)`; Settings › General "ffmpeg location" and "render output
  folder" (`config.render`); `animstudio --render <file | --sample> --preset <name> --out <dir>` before the
  dev loop and `run_native` (`cli_exit`, `run_cli_job`; 0/1/2). Fetched: image 0.25.10, exr 1.74.2,
  ab_glyph 0.2.32 and their dependencies. 15 `delivery_tests`, 7 `render_queue_*` app tests. Deviations:
  `render` takes the colour law and frame size (a cut knows neither); `ffmpeg_args` takes
  `AudioIn { path, offset, secs }` to keep v0.3's seek and trim; `raster_line` takes the face. Left: the
  File ▸ Export PNG and MP4 items still run their own exporters (not yet queue jobs); the composite view's
  "Final look" toggle; adding hand-picked cuts (the pane adds this cut, every cut of the scene, or a frame
  range). Not verified: a real ffmpeg encode (tests spawn nothing), the real window.
- 2026-10-29 — GM11 review fixes; the three Left items are done. File ▸ Export… is the render queue's:
  "open the render queue (all presets)" first, then the menu's frame range (the whole cut until a field
  moves), "PNG sequence…" (PNG 8-bit) and "MP4 video…" (H.264 review, greyed with "needs ffmpeg" when none
  is found); each queues a job (`Editor::queue_export`, `render_queue::export_job`), brings the queue
  forward and starts it. The v0.3 exporters and their window are gone (`export.rs` keeps `suggest_dir` and
  the ffmpeg search); Show me ▸ Export opens the Edit room's queue; the dev loop's rebuild waits for a
  rendering queue. `Action::ShowRenderQueue` ("Render Queue (delivery)", Window, Window ▸ Render Queue
  (delivery), aliases render/export/deliver/output/line test/prores/satsuei out, Ctrl+Shift+E in both
  keymaps, Global) opens it from any room. "Final look": a latch on every viewer beside "safe" and
  `Action::ToggleFinalLook` (View ▸ Final look (delivery), Shift+C) show the composite as the master
  delivers it (Purpose::Final), rendered by the CPU on a worker from a snapshot (`viewer::FinalLook`, keyed
  by cut, frame, engine revision and size), the last frame kept up with the TALLY dot meanwhile. A
  whole-cut job keeps `frames: None` and reads the cut's length when it starts (`Job { frames, span }`,
  `resolve`). New "Line test PNG" preset (PNG 8-bit, black on white, no ffmpeg). A Final job's snapshot is
  filtered once (`retain_purpose`), not per frame. `{frame:0N}` pads at most 10 (`MAX_FRAME_PAD`); an image
  pattern without a frame token gets `_{frame:04}` (`sequence_pattern`). The pane: "look again" searches
  for ffmpeg again; "half size (proxy)" per job; "pick cuts" latches and "+ selected cuts" (in the
  project's order); "+ frames" defaults to the whole cut; the output field's hint names the configured
  default or `render` beside the project; the plate's meter in place of the progress bar; hovers on the
  controls. CLI: `--scene` / `--cut` (`pick_cut`; names ignore case, an unknown name lists what is there),
  the cut it renders said when there are several, the usage said once. 18 `delivery_tests`, 14
  `render_queue_*`. Not verified: a real ffmpeg encode, the real window and its file dialogs.
- 2026-10-29 — GM12 Sound tracks and real-time playback (core). Engine: `audio.rs` — `TrackKind`,
  `TrackProps` (name, kind, offset, gain dB, mute, solo), `AudioTrack`; `decode` through symphonia 0.5.5
  (fetched: symphonia, -core, -bundle-flac, -bundle-mp3, -codec-pcm, -codec-vorbis, -format-ogg,
  -format-riff, -metadata, -utils-xiph 0.5.5, encoding_rs 0.8.42; gapless on), routed by magic bytes
  (`sniff`); `resample_linear`, `mix` (stereo at 48 kHz, mute wins over solo, clamped), `write_wav`
  (hound, 24-bit), `envelope` (per-frame RMS), `lip_flap` (hysteresis 0.8, minimum hold) and
  `lip_flap_commands` (one SetCells). `Cut.audio` stays the first (dialogue) track, its settings in
  `Cut.audio_props`; more tracks in `Cut.tracks`; `AudioClip::from_bytes` (a WAV keeps hound's exact
  path). Commands `AddTrack` / `RemoveTrack` / `SetTrackProps` with exact inverses (track 0 is the first
  track); InsertFrames / DeleteFrames ripple track offsets (a delete that swallows an offset restores it
  exactly on undo). Store: side table `audio_tracks(cut_id, ord, json, bytes)` (D2: made on save, probed on
  load; ord 0 holds the first track's settings when they differ from the default); `cut_audio` unchanged.
  `deliver::mixdown` / `write_mixdown` over a frame range. App: `playback.rs` — `PlayClock` (frame =
  start + floor(played · fps / rate), passed-over frames counted, never back), `place` / `play_range`
  (loop range, play from the cursor), `scrub_requests`, `mix_key`, `FrameCache` (LRU by bytes, MB cap,
  a changed eval hash drops the frame). The Editor plays the mix of every track (cached by `mix_key`),
  audio-clocked from the playing source's sample position; transport: "in" / "out" / × loop range,
  "scrub" latch, "dropped N". `AudioOut::open` refuses in a test build. File menu: Import sound… /
  Add sound track… (WAV, MP3, OGG, FLAC) / Remove track N. Render queue: video jobs mux the mixdown WAV
  (ffmpeg audio args unchanged, only the input), image sequences write the range's mixdown WAV beside the
  frames. Fixtures `tests/fixtures/tone.{wav,flac,mp3,ogg}` (0.5 s of 440 Hz at 16 kHz, made once with
  ffmpeg 8.1.2 — 16 kHz keeps each under 20 KB). 15 `sound_tests`, 7 `playback_*`. Listed as left here,
  against D0 (they are core), and built by the review fixes below: the sheet's waveform strip and track
  headers, the lip-flap window, the RAM preview's worker, Ao bar and Settings ▸ Performance MB cap.
  Stretch, not built: waveform zoom, cue markers. Not verified: sound through a real device (latency: the
  clock follows the samples handed to the device), the real window and its file dialogs, a real ffmpeg
  mux of the mixdown.
- 2026-10-29 — GM12 review fixes. Sheet (`xsheet_panel.rs`): a track header row over the sheet — the
  track picker ("1 dialogue — serifu (dialogue)": number, name, `TrackKind::label`), the kind combo, M / S
  latches (painted, spoken "mute track" / "solo track"), gain in dB (−60..+24) and offset in frames as
  DragValues that show their value while dragged and land as one `SetTrackProps` on release (one undo step
  per drag or click); REMOVE TRACK (DANGER) in the picker's context menu, the File menu's "Remove track N"
  gone. The waveform column draws the picked track's per-frame RMS (`playback::track_envelope`), read once
  per change under `envelope_key` (cut, track, its offset and clip, the cut's length and fps; per track,
  not the mix key, so another track's edit reads nothing again), in plate tokens. Lip flap:
  `Action::LipFlap` ("kuchipaku (lip flap)…", Sheet ▸ Lip flap…, group and context Sheet, aliases lip sync
  / kuchipaku / mouth, no chord) opens a non-modal window — track (the first dialogue track to start),
  mouth column, the closed / half / open drawings (the active column's first three to start), an in–out
  range (the loop range, else the whole cut), the two threshold sliders,
  the envelope with the mouths it would write and their counts; "generate" is one `Engine::apply`
  (`AppState::generate_lip_flap`). Workers (NEVER-DO 6, D9): Import sound… / Add sound track… read and
  decode on a worker (`begin_sound_import` / `poll_sound_import`: "decoding NAME…", a second import refused
  while one decodes, a file that does not decode refused saying why) and land as one step; the playback
  mix is made on a worker (`playback::MixJob`, keyed by `mix_key`; the sound starts when it lands, a dead
  worker plays silent with a refusal); `audio::mix` resamples only the span of each clip inside the mix;
  `decode_shared` reads a clip's own buffer without a copy; an RF64 WAV is read as the RIFF it is
  (`rf64_as_riff`). RAM preview: `Action::RamPreview` (Sheet ▸ Play ▸ Render RAM preview, chord 0) — a CPU
  worker draws the play range into the `FrameCache` (now a `BTreeMap` by use order, eviction O(log n)),
  each frame held under its own key (eval hash, what each column shows, the drawing's content hash, the
  look), so an edit drops only the frames whose key moved and another cut, size, colour law or final look
  starts afresh; the cap is `perf.ram_preview_mb` (default 2048, Settings ▸ Performance slider 256–32768);
  a full cap or a frame larger than it is refused; Ao bars on the sheet mark held frames; the viewer shows
  the held frame while playing. Verbs (D6), each with help, group, menu, aliases and context, chords the
  same in both keymaps: ImportSound (File, Ctrl+I), AddSoundTrack (File, Ctrl+Shift+I), LoopIn (Shift+I),
  LoopOut (Shift+O), ClearLoop (Alt+X), ToggleScrub (Shift+S) in Playback and Sheet ▸ Play; the transport
  and the menus call `perform`. Transport (D7): fixed slots — the range readout says "—" when unset, × is
  greyed then, "dropped N" keeps its width; the tooltips name the range and the chord ("the loop range
  starts at this frame (Shift+I)"; loop off "stops at the end of the loop range (or the cut)"). Minors:
  the loop range is per cut, rippled by Insert / Delete frame (view state: undo and the range verbs leave
  it), a start past it plays on to the cut's end; stop never sounds a scrub
  snippet; scrub defaults on, lives in `perf.scrub_audio`, and the first sound import says so in the status
  line; a new track is named after its file and its kind guessed from the stem (`TrackKind::named`:
  bgm / music / m01 → Music, se / sfx / fx → Effects, serifu / dialogue / voice / vo → Dialogue, else
  Effects); the render queue's "mixdown WAV" latch (default on) per job — the WAV is named by
  `deliver::video_name`, which already trimmed the separator the frame token leaves (the test pins its
  name and length); `has_sound` / `any_solo` / `mix_key` walk `tracks_iter`, no Vec per frame. 18
  `sound_tests`, 17 `playback_*`, `sheetui_waveform_*`, `render_queue_mixdown_*`; the colour-literal pin
  83 → 82. Not verified: the real window, its file dialogs and a real device.
- 2026-10-29 — GM13 Satsuei look: passes, light FX and the satsuei template (core). Engine: `look.rs` —
  role passes (`DrawingSource { column, roles, absorb_trace }`, both serde-defaulted so an old node reads
  and a plain one writes byte-for-byte as before; `NodeKind::source` / `pass`), the trace-line absorb
  (iro-tore: ShadowLine / HighlightLine pixels take the paint under them, else the nearest within 3 px by
  chamfer 3-4, coverage kept), and the nine look kinds `FxKind::LOOK` (Para, Light wrap, Rim light,
  Vignette, Grain, Motion blur, LUT, Matte, Palette map; maths once in `look.rs`, `fx.rs` dispatches;
  each the identity at its neutral setting); `FxCtx { frame, blobs }` — Grain's recipe spells the frame,
  a LUT's the file's hash; `.cube` reader (`parse_cube`, errors say which line); the cut's files
  (`Cut.blobs`, `SetCutBlob` with an exact inverse, SQLite `cut_blobs`); `satsuei_template` — BG →
  Camera; per cel column paint pass (absorb) × Shadow → Matte (white) → Grade 0.75 (Multiply), Highlight
  → Glow (Screen), Line pass (Normal); Book over; then Diffusion → Para → Light wrap (BG camera, the
  characters) → Grade → Grain → Vignette → Output, one batch, laid out. App: the graph pane's "Build
  satsuei graph" button (greyed with the reason when the cut has no drawing column), one undo step;
  filtered sources route to the CPU worker with the FX (D9), the job cut carries the cut's files; the
  source inspector's pass picker and "absorb trace lines"; the add menu's "Look" category (the FX
  category stays the four core kinds the GM9 search test pins); a LUT parameter's "load .cube…"
  (refused with the reason when it does not read; file + node in one step); palette variants per
  character (Palette panel ▸ variants) feed the Palette map's picker and refresh its nodes in one step.
  19 `look_tests`, 3 `comproom_*`. Not verified: the real window, the file dialog, GPU display of the
  look at size and its speed at 1920×1080. Left: chromatic aberration, light rays, colour Curves; a
  registered verb (menu / palette entry) for Build satsuei graph.
- 2026-10-29 — GM15 Cut board, episode timeline and EDL (core and panes). Engine: `board.rs` — `CutStatus`
  (8, stable keys) and `ProdInfo` in cut meta `prod.status` / `prod.assignee` / `prod.notes` (one
  SetCutMeta per changed field; a key this build cannot read is left alone), `filter_cuts`, `trt_frames`,
  `format_trt` (30,972 at 24 = "21:30+12"), target difference and `parse_trt` (`app.episode.target`),
  `episode_frame_map` / `cut_starts` / `locate`, the pinned CMX3600 `edl_cmx3600` (non-drop, record
  01:00:00:00, V per cut plus AA when it sounds, `* FROM CLIP NAME:` from the queue's pattern), the
  colour designation sheet (`color_sheet_layout` pinned at 604×332 for 6 chips + 1 variant;
  `render_color_sheet`, swatches the stored chip exactly, text via `text`), `panels` / `cuts_from_panels`
  (panel cut number in `DrawingDef.cut`, written only when set; new cuts after the animatic with the
  pictures copied on a Ref column, one batch), retake notes in the frame ripple (D4; kept as JSON values
  so unknown fields ride along, the inverse restores the bytes); `Command::MoveCut` (exact inverse, the
  order saves through `ord`); `store::export_cut`; `deliver::Episode` (frame source over the cuts, the
  mixdowns laid end to end slot by slot). App: `Pane::Board` (cards: frame-0 picture, number, s+k, status
  combo, assignee, open notes; filters; open; make cuts from panels, colour sheets, export cut as project)
  and `Pane::Episode` (blocks by length, drag = MoveCut, TRT against the target over/under, Play episode
  runs on into the next cut, Export EDL, render episode), both tabbed in the Edit room; the render
  queue's Episode job (`+ episode`, one sequence or video, the episode's mixdown). 12 `board_tests`, 8
  `cutboard_*`. Not verified: the real window, a video Episode job through ffmpeg. Left: note markers on
  the sheet rows and the panel cut-number cell menu, registered verbs (menu / palette) for the exports,
  the storyboard pane, conte pages, slate frames in the episode render.
- 2026-10-29 — GM15 review fixes. The sheet: an Aka tick per noted row at the memo cell's edge (dim once
  resolved; hovered, each note with its author and state), riding Insert frames; every row's menu lists
  the frame's notes as latches (resolved) and "add retake note…"; a BOARD or Bg / Ref cell's menu sets
  the panel's cut number (−, +, typed or dragged, "clear (use panel order)"; the cell shows C1, C2 …),
  the cells' menu staying open until a pick or a click off it. The retake note window (frame, what to
  fix, "set Retake" latched; refused with no text or unreadable notes, kept open). Nine registered verbs
  (D6: help, aliases, a chord in both keymaps, menus): Shinkō board, Henshū timeline, Play episode, the
  three exports (File ▸ Export), Make cuts from panels, Render episode (the queue comes forward), Add
  retake note…; every pane button asks the editor for its verb. Card pictures keyed on their own cut's
  frame-0 eval (`playback::frame_hash`), drawn on a worker, never started mid-stroke, gone with their
  cut; no scene cloned per frame; the status on the card's edge, open notes in Aka; make cuts on the
  animatic's card, refused off an animatic, named `CUTnn` past every name the scene has; export as
  project never over a file (`_2` …); Play episode passes a cut with no columns and stops; the D7
  labels and pane titles; the timeline scrolls and zooms, names clipped, a playhead. Notes read
  leniently (a note missing a field still shows; unreadable notes are refused, never overwritten).
  A card stacks top-down at one size (the board's wrapped, centred row flow had pushed a card's combo
  and notes menu below the visible board, where no click lands). The command palette reads ā ī ū ē ō as
  the plain vowel ("shinko" finds the Shinkō board). 14 `board_tests`, 18 `cutboard_*`.
- 2026-10-29 — GM16 Generators: particles, speed lines, lettering. Engine: `generator.rs` (not `gen.rs`:
  `gen` is reserved in Rust 2024, so the module is `anim_core::generator` and the node is
  `NodeKind::Generator { generator, params, binds }`) — `GenKind` Particles / Speed lines / Focus lines /
  Text / Balloon with specs, help, romaji terms and aliases; analytic particles (births `i/rate − preroll`,
  each choice a hash of seed, index and stream, closed-form motion with gravity, wind and a seeded
  wobble; point, line, rect and matte emitters; dot, streak, soft disc, petal and snowflake; normal or
  add) and the six presets (Sparks, Snow, Rain, Dust, Sakura petals, Embers); tapered speed lines with a
  clear band, focus lines stopping at a clear ellipse, held / every frame / on twos; SDF balloons
  (ellipse, rounded rect, cloud, burst, tail to a point, inner stroke). `ParamKind::Text` / `FxValue::Text`
  (`{"text": …}`, never bindable). `text.rs`: horizontal and tategaki layout (columns right to left,
  dashes and brackets turned, 、。 in the top-right corner, small kana nudged, two-digit tate-chu-yoko,
  line-start kinsoku hanging, wrap), outlines by dilation, the full Plex Sans JP Bold as an engine static.
  The CPU renderer's Generator arm draws over a camera plane's paper; the eval recipe spells the motion
  frame so still generators cache-hit. App: the Generators category (and a Particles entry per preset)
  in the add menu, the inspector from the specs with a text field, `needs_cpu`. The UI's JP fallback is
  the full face (the subset file stays on disk, unused); the glyph-coverage test found ▾ ▸ ⌂ 🖌 drawn by
  no face of the stack, so egui's Hack closes the proportional tail (▾ ▸) and the host-preset mark is ◆,
  the workspace brush menu "brush: …". Vertical Latin stays upright one per cell (no rotated runs;
  ab_glyph has no `vert` feature, so vertical forms are turned or shifted horizontal glyphs). 19
  `gen_tests`, 3 `genui_*`.
- 2026-10-29 — GM16 review fixes. A balloon's box comes from its geometry (every cloud bump's disc,
  every burst tip, the tail): a cloud at HD proportions had its top cut flat. Lettering draws only the
  target's window and the outline's reach (glyphs outside it skipped), the outline an exact Euclidean
  distance transform in two separable passes, so 101 ■ at 2000 px with a 100 px outline into 256×144
  is cheap; wrapping keeps a running line length. Add node finds the generators by the artists' words
  (fukidashi, ryūsen, shūchūsen, tategaki, serifu …) and its help names them. The Text kind's term is
  shashoku (set type; kakimoji is the hand-drawn kind). A new balloon centres where a new Text block
  does (y 0.5). The text field commits on a click away or Ctrl+Enter (its hover says so); an edit
  whose field let go while its pane was hidden commits when it shows again. A Text node is titled by
  its first words on the graph. Speed and focus lines also move on threes. The glyph test reads the
  engine's UI strings; the test minimums and the headless rule are read from the source. Left for a
  later part: on-canvas handles for a balloon's tail, the focus-line centre and the clear ellipse
  (typed as frame fractions today); presets stay in px tuned at 1920×1080 (every px parameter's help
  says px). 22 `gen_tests`, 6 `genui_*`.
- 2026-10-29 — GM17 3D layout and toon render: new `r3d.rs` — glTF (.glb, or .gltf with embedded
  buffers; node transforms, base colour factor) and OBJ (triangulated; its .mtl embedded as `#mtl `
  comment lines at import) read from the bytes in the new `cut_models` side table (`ModelAsset`,
  `Command::SetCutModel`, no version bump); `Camera3` on a 36 mm back (50 mm → 39.6°), a deterministic
  rasteriser (top-left rule, near clipping, perspective-correct 1/z depth), stepped half-Lambert toon
  bands with the shadow chip, silhouette/fold/crease outlines dilated to width, and the beauty, lines,
  shadow matte, depth and ID matte passes; `NodeKind::Model3D` (27 specs, the numbers bindable) and
  `NodeKind::ImageSequence` (`shot_####.png`, first file at an offset frame, hold last) in export,
  eval and the GPU view's CPU subtrees; `export::RenderEnv` carries the project folder and de-duplicated
  warnings (`deliver::render_env`, a missing frame is transparent with one warning). JPEG and TIFF
  images import as 8-bit PNG. App: View3D pane (Blender orbit/pan/zoom on the middle button and wheel,
  view buttons for numpad 1/3/7 with their opposites, 0 and 5; half-size render while navigating; "Set
  cut camera from view" keys the node's camera at the frame, columns made and bound, one undo step),
  the "3D layout room", File ▸ Import 3D model…, the add menu's 3D category and the Model3D and
  sequence inspectors. 19 `r3d_tests`, 5 `view3d_*`; colour literals unchanged. Left: the canvas 3D
  underlay; numpad keys themselves (egui reads numpad digits as the row's, which are chords: 0 RAM
  preview, Ctrl+0/1 zoom); the project folder reaches `deliver::render_env` but not yet the GPU view,
  viewers, playback or the render queue (relative sequence paths read from the working folder there);
  stretch items (turntable, 2× supersampling, glTF textures and animation, shadow mapping). Not
  verified: the real window, the rfd pickers, the toon look by eye, models beyond the test cube,
  triangle and sphere.
- 2026-10-30 — GM18 Manga pages, tones and perspective rulers: new `manga.rs` — `Page` (B5 2591×3624
  and A4 2976×4175 at 350 dpi with 3 mm bleed, trim and inner frame in px; a webtoon 800 px wide,
  6400 down, exported in 1280 px segments) in the cut meta `manga.page`, `Rulers` (1–3 vanishing
  points and the horizon) in `manga.rulers`; panels are the new `ParamKind::Polys` of a new
  `GenKind::Panels` (white insides, the 0.5 mm border inside the outline, clear gutters);
  `split_panel` cuts on whole px with the 5 mm row / 2 mm side gutter (69 / 28 px); `page_graph` is
  white Solid paper, each column through a Matte of the Panels (a TONE column through a Tone first),
  the Panels multiplied on top — one step, and a rebuild keeps the panels and the tone's values.
  `FxKind::Tone`: an AM halftone (lpi, angle, round/square/line dots, the page's dpi as a parameter,
  1-bit mono). `Snap` picks a stroke's line once it has moved 6 screen px (the lines to each VP, the
  horizontal and the vertical, a third VP standing in for the vertical; the nearest angle wins) and
  projects every later point; the canvas feeds it before the stabilizer on the pen's and the mouse's
  paths. Export (`deliver::write_page`, `export_pages`): PNG with pHYs 13780 px/m, TIFF at 350 an
  inch, colour, grey (Rec.709 luma) or 1-bit mono, trim marks, webtoon segments `<cut>_NN`. App: the
  Manga B5, Manga A4 and Webtoon templates (first page P01); the off-spine "manga (pages)" room with
  the Manga pane (pages, New page, the page thumbnail and panel list, Split across / Split down, Add /
  Delete panel, gutter mm, Clip to panels, tone presets 42.5/60/85 lpi, 0°/45°, dot shapes, rulers
  none/1/2/3-point and the snap latch); verbs New page, Clip to panels and the Manga room; the add
  menu's Manga category. 20 `manga_tests`, 6 `mangaui_*`; colour literals unchanged. Left: PDF export
  and slanted freehand panels (stretch); panel gestures on the canvas (drag a rect, draw a split
  line); dragging the VPs and drawing the ruler lines and page guides over the canvas; the app's
  page-export command and worker (core `export_pages` is done); proxy scaling of the Tone's and the
  Panels' px parameters. Not verified: the real window, the tone by eye, a print at a printer.
- 2026-10-30 — GM18 review fixes: the canvas draws a page's guides whenever the cut is a page (the
  "page guides" latch, on): the bleed edge, trim and inner frame, the panels, the horizon and a fan
  of rays from each VP, named "VP1"… and "horizon". The panel (koma) tool (K): a drag on bare paper
  draws a panel, one across a panel splits it at the gutter (held level or upright within 4°), one
  from a VP or the horizon moves it (a point on the horizon carries it) — one undo step each, the
  drag drawn as it would land. The Manga pane lists one row a page (thumbnail, number, name), places
  each VP and the horizon by number, sets the panels' border (mm) and the tone's lpi (20–120) and
  angle (0–180), and exports: verb Export pages (Ctrl+E; File ▸ Export ▸ Manga pages…) asks the
  shell for a folder and writes on a worker (PNG/TIFF; colour, grey or mono and its threshold; trim
  marks). Snap to rulers is a verb (View ▸ Snap to rulers, Ctrl+2 over the canvas) that the pane's
  latch runs. New page (Ctrl+Shift+N) is named one past the book's highest P number and is refused
  in a project that is no book; export refuses two pages that would write one file; Clip to panels
  keeps free generators and shows DANGER (the Aka edge and its note, button and menu) over a graph
  shaped by hand. 26 `manga_tests`, 15 `mangaui_*`; colour literals unchanged. Left: PDF export and
  slanted freehand panels (stretch); proxy scaling of the Tone's and the Panels' px parameters. Not
  verified: the real window and its folder picker, the guides and the tone by eye, a print.
- 2026-10-30 — Sweep of the done parts' `left` lists before ship (73 items, each checked against the code at
  64df613; Lead default: finish the leftovers before ship). 31 were already done at HEAD. 15 go to three parts
  that run before GM19, which now depends on them: GM20 Painting on the extended paper (2 items), GM21 Menus,
  dialogs and the palette file (10, three of them stretch) and GM22 View3D worker, sequence reload and the 3D
  underlay (3). 19 are skipped: stretch not needed daily, or covered by what exists; proxy scaling of the
  Tone's and the Panels' px parameters is moot (proxies render full size, then shrink). 8 need the real
  window, a GPU or the Lead's eyes and stay not verified.
- 2026-10-30 — Sweep review. Every planned item is still open at 64df613 (at least three checked a part), and
  the planner's "done at HEAD" spot checks held. D0 lets only Stretch go to `left`, so GM20's onion,
  light-table and other-column slots at paper size became its Stretch, and GM21's palette import and export
  and Revert to saved (stretch in their own parts) became its first Stretch items. GM21 is renamed Menus,
  dialogs and Shift+Delete (10 items, five of them stretch). GM20 now covers what a paper-sized layer breaks:
  the composite viewer's `ensure_size` (one layer-rect helper, or the two rebuild each other's layers), the GPU
  graph's frame window in `composite_layers_into`, refill, replace-all and the lasso fill (frame-bounded
  today) and the presence pointer; its layer-rect criterion is a harness check plus a source scan. GM22: the
  underlay is keyed by the node's eval value (a frame in the key contradicted "a still model reuses its
  picture"); a late View3D job still replaces the picture, as the Final look does (dropping it froze the
  picture for a whole orbit); the job logic is pinned without a thread; absolute sequence patterns (an untitled
  project's imports) get the stamp; the 3D layout room's help stops saying the underlay is missing. The
  headers' filter words match the gates (`extpaper_`, `tidy_`, `worker3d_`), and the run table and the stage
  list carry the sweep parts. The skipped list stands: none of the 19 is met in a first session. Not
  verified: the 45-minute fit of each part is an estimate.
- 2026-10-30 — GM20 Painting on the extended paper: `paint.rs` gains `LayerRect` (origin, size, capped, and
  the frame it serves), `paint_area` (the paper when it holds the frame, else the frame), `layer_rect` (each
  side over the device limit cut to a window centred on the frame, kept in the paper), `tile_span` (upload and
  readback both go through it; negative tiles legal), `layer_tiles`, `merge_outside` (art outside a capped or
  shrunk layer survives the pen-up and replay readbacks), `layer_memory` / `memory_text` / `cap_text` and
  `composite_uniform`. The dab shader takes the layer origin in the old pad slot; the composite pass reads a
  frame-sized window texel for texel (`textureLoad`) when the layer is not the frame, so the graph pool, the
  viewer and the onion and column slots stay frame-sized. The canvas keeps its layer rect with or without a
  GPU (`sync_layer_rect`, held mid-stroke) and every `ensure_size` caller takes it; a rebuild bumps a
  generation that resyncs the canvas. anim-core: `flood_fill_mask_in`, `refill_in`, `replace_all_in` and
  `composite_pixel_at` over any paper area (the frame forms delegate, bit for bit); fill / refill / replace
  clicked off the paper are refused on the Aka lane ("outside the paper: Cut ▸ Paper size… extends it");
  select-all, the lasso fill, the eyedropper and the presence pointer cover the paper. The Paper size window
  shows the cap line and the GPU memory. 12 `extpaper_` tests. Left: the onion, light table and other
  columns are still drawn frame-sized (their Stretch to paper size). Not verified: painting beyond the frame
  on a real GPU (dabs, readback, the composite window and the layer-sized display quads have no headless
  check beyond the CPU mirror and naga validation).
- 2026-10-30 — GM21 Menus, dialogs and Shift+Delete: `Chord::matches` counts a Cut event with Shift held and
  Ctrl not held as a Delete press (egui-winit's Windows Shift+Delete), so Delete frames fires over the sheet
  and Industry's Clear cel all over the canvas, once; Ctrl+X still only cuts cells, and nothing fires over a
  focused field. `rename_current_cut` applies `Command::RenameCut` (one undo step; the same name adds none).
  `templates::SIZES` (every template's size, then Square 1080 and Vertical 1080×1920) and `Template::dpi`;
  newproject.rs loses `PRESETS`, `pick_template` sets size, rate and dpi, a page template greys the paper,
  size and dpi ("set by the page template") and `new_state` keeps the page's paper and dpi; the editor's
  layers take the project's size. Action `OpenSample` (File ▸ Open the sample cut, no chord) through
  `Leave::Sample` (asked first over unsaved work, refused to a guest). Settings ▸ General shows "found:
  <path>" or the queue's NEEDS_FFMPEG with "look again", from `RenderQueue.ffmpeg` (searches counted). 14
  `tidy_` tests. Left: the stretch list (palette JSON, Revert to saved, the Polys inspector arm, the
  Shortcuts page and pass-picker harness tests). Not verified: a real Windows Shift+Delete keypress in the
  running app (egui-winit's Cut is reproduced as an event in the harness), and the field's own text cut.
- 2026-10-30 — GM21 review fixes: Shift+Delete's Cut over the sheet leaves a selected cell uncut (the
  clipboard stays empty), and over a focused field nothing fires while the field cuts its own selected
  text (the room name field, now tested). Renaming a cut to its own name says so on the status lane.
  Settings ▸ General: with no editor open (the Welcome screen) the shell's own render queue answers;
  mid-typing only an answer for the text as typed shows; the cached answer is read without allocating
  (`RenderQueue::ffmpeg_cached`, `search_ffmpeg`, which the pane's `ffmpeg_path` shares). New Project:
  each paper row reads "<name> — <w>×<h>" and a size shows once; a page's size picked as paper brings
  its print dpi; blank after a template goes back to the form's defaults. 16 `tidy_` tests. Left: the
  whole stretch list as above (the part reached its cap first). Not verified: a real Windows
  Shift+Delete keypress, and Settings opened from the Welcome screen, in the running app.
- 2026-10-30 — GM22 View3D worker, sequence reload and the 3D underlay: `view3d.rs` gains `JobLogic`
  (one keyed job, the newest key waits, a failed key said once and never resubmitted, a lost worker
  the same) and `Worker3d` (a thread per job, the one texture kept until the next picture lands); the
  pane renders through `view_job` on it with the TALLY dot while busy. The evaluator holds the project's
  folder (`set_base`): an ImageSequence whose path resolves folds the shown file's length and mtime
  (`r3d::sequence_stamp`, a stat); a relative pattern with no folder keeps its old value. File ▸ Reload
  image sequences and the window regaining focus drop the sequence nodes and their downstream only
  (`AppState::reload_sequences`; the RAM preview rechecks, the final look rereads). View ▸ 3D underlay
  (`Underlay`, Canvas context, no chord) draws the cut's Model3D node as export renders it (at most
  960 wide) at 0.45 on the frame under the columns, keyed by node, eval value, size and law; no job
  mid-stroke or while playing. 8 `worker3d_` tests and one core test. Left: the stretch (the node
  picker, the opacity slider). Not verified: the underlay and the dot in the running app.
  Review fixes: the underlay fades at the light table's opacity (`LightTableCfg.opacity`, 0.45 by
  default); an ImageSequence's stats are kept per node with its values (`r3d::SeqStats`: each file
  number stat'ed once per reload, a held frame never searches back again); a reload keeps a running
  RAM pass unless a frame it draws changed (then it starts over); a return mid-stroke reloads when the
  pen lifts; a lost worker repaints; the canvas shows the TALLY dot while the underlay renders; the
  menu item says why a ticked underlay shows nothing (no Model3D, composite view, a BG column over
  it); aliases "3d lo", "3d layout", "layout under", "genzu". 12 `worker3d_` tests and two core tests.
- 2026-10-30 — GM19 Ship: polish gates, version and release build: the colour-literal ratchet is
  pinned at 0 (82 → 0) through plate helpers — `alpha(token, a)` for a token at a transparency,
  `chip(rgba)` for the animator's own colours (chips, inks, swatches, the picker), `UNTINTED`/`tint`
  for textures, `halo`/`shade` for marks that must read on any paper, `legend_muted` for the node
  graph's idle pins (its other literals were tokens already). 0.5.0 in the app and the installer,
  `MIN_PEER_VERSION` 0.5.0; About says it. The sample's first track carries a serifu line made in code
  (`sample::dialogue_wav`, hound: a 150 Hz buzz under three syllables, as long as the cut). The Welcome
  screen gains a What's New tab grouped by persona, opened first while `ui.whats_new_seen` is not this
  version. The probe walks every room with a canvas (`PROBE_ROOMS`: the spine, manga, the forge).
  `tools/smoke_render.sh` renders the sample through the release exe: 24 PNG16 files; ProRes (prores,
  pcm_s24le) and H.264 (h264, aac), 24 frames each. README rewritten by persona. 26 `ship_` tests
  (17 pane smokes at three scales; the retired Brush pane is removed on sight). Gate: 823 passed, 0
  failed, 1 ignored; smoke render passed; no v0.5 tag; installer built. Left: the sample's FX showcase
  (camera pan over a wide BG, glow, diffusion, particles). Not verified: the What's New first-launch
  route and the About window in the running app.
  Review fixes: the sample is the release's showcase. Its BG plate runs 320 px past the frame's left
  edge (the cut's paper, 960×360) and a Camera node pans across it (pan x −320 → 0, eased, over the 24
  frames); the ball goes through Glow and Diffusion, held to the frame off the camera; Sakura petals
  drift over the comp. The tests premised on the plain auto-comp start from `sample::build_auto_comp`.
  The probe walks the 3D layout room too (`ProbeRoom::Layout3d` holds its View3D still: `probe_run_in`
  takes the pane). Start-up opens the Welcome screen through `welcome::open_at_startup` (What's New
  while due, recording the version; Start after). A fresh install (no config.json) starts seen and
  opens on Start; a config an earlier version wrote reads "" and is due. Tooltips on the Start and
  What's New detents. The picker's ends are `plate::PICK_WHITE`/`PICK_BLACK`, and a chip made of
  numbers outside the tests is counted (0). Decided: with "Show at startup" off the app opens the New
  Project form and What's New waits one click away on File ▸ Welcome screen…; a Help ▸ What's New verb
  is left. 31 `ship_` tests (README coverage, a 0.3.0 config loading whole, the start-up route, the 3D
  layout probe, numeric chips). Gate: 828 passed, 0 failed, 1 ignored; smoke render passed; no v0.5
  tag; installer built. Not verified: the What's New first launch and the About window in the running
  app; the showcase was looked at in two rendered frames (1 and 24) only.
