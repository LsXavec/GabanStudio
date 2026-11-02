# Gaban Studio

*Gaban* (画版) — the drawing-plate. A 2D animation studio that goes from the
first rough to the delivered file, in plain steps on one bar: a GPU brush
engine that imports Krita, Photoshop and Procreate brushes, a Timing sheet
that works like the paper timesheets, effects that look the same on screen
and in the delivery, and a shared drawing session over your network.

Version 0.6.0.

## What's new in the look

The window is a light sheet of paper with the tools around it, and the
canvas comes first. Pick what you are making and the bar shows only its
steps; each step lays the window out for that job:

| You are making | The steps |
|---|---|
| A drawing | Draw, Color, Effects, Export |
| An animation | Plan, Timing, Color, Effects, Export |
| A comic | Manga, Color, Export |

An animation opens on Plan with its Timing sheet. A drawing opens on Draw,
with no sheet. Nine lessons make something real, rookie to experienced
(Help ▸ Lessons), and What's New opens once after an update with every old
word beside its new one.

## Export

The Export step makes the file in one click: a PNG image for a still, a GIF
for a moving shot (with `Export GIF instead` when a video is on offer).
GIF, stills and image sequences work with nothing else installed; video
needs an ffmpeg you install yourself ([ffmpeg.org](https://ffmpeg.org)).
Everything else, every format and preset, stays on Export's queue.

## Install

Download **AnimStudio-Setup.exe** from the
[latest release](../../releases/latest) and run it. Per-user install, no
admin needed. The app checks this repo's latest release and offers
"relaunch to update" when a newer build is published.

(The binaries keep their original `animstudio` file names so installed
apps keep updating seamlessly across the rename.)

## What it does, by who reaches for it

**Animator (Plan, Draw, Timing).**
- The Timing sheet as the master: insert and delete frames (every track and
  camera key ripples), holds, retimes on 1s/2s/3s, reverse, typed entry,
  memos and the dialogue row — every edit one undo step.
- Keys, inbetweens and layout drawings marked on the sheet; spacing charts
  and Make inbetweens; the light table with ghost frames 0–5, pins, shift
  and trace.
- Canvas rotate and mirror, a 0–10 stabilizer, Shift-click straight lines,
  brush size keys and the Alt-click eyedropper.
- Never lose work: an unsaved guard, autosave every 5 minutes into the data
  folder (newest 3 kept), recovery after a crash, and one `.bak` beside the
  file.

**Painter (Color).**
- Exact colour: new projects keep chips exactly as picked, so the screen
  and the export bytes equal the chip. Older projects open unchanged and
  convert on request, with a backup written first.
- Recolour by editing a chip, refill, chip pick, palette variants and
  palette maps, and the paint check for holes and stray colours across a
  shot.

**Compositor (Effects).**
- Effects run once on the CPU, so the composite view and the delivered frames
  agree: blur, glow (T光), diffusion, grade (lift/gamma/gain, exposure,
  saturation, temperature) and .cube LUTs.
- Role passes, trace-line absorb, para, light wrap, rim, vignette, grain,
  directional motion blur, mattes and the full finishing setup.
- The camera node: pan, T.U./T.B., rotate and shake over paper larger than
  the frame, multiplane depth and depth of field, eased in the graph editor.
- Particles, speed lines and focus lines as generator nodes.
- The Export queue: PNG16, TIFF16 and EXR built in; ProRes, DNxHR, H.264
  and H.265 through an ffmpeg you install, with the sound muxed, burn-ins
  and motion checks.

**3D artist.** glTF and OBJ import with a toon render (outlines and passes),
View3D with Blender navigation, the shot camera keyed from the view, image
sequences from other apps referenced on disk, and the canvas 3D underlay.

**Manga artist.** B5 and A4 pages at 350 dpi and 800 px webtoon strips,
panels with gutters, tones, perspective rulers, lettering with tategaki and
balloons, and page export.

**Editor (Timing).** Sound tracks (WAV, MP3, OGG, FLAC) with waveforms,
audio-locked playback, scrub, lip flap and mixdown; Shots, a card per shot
with its retake notes; the episode timeline with TRT; CMX3600 EDL export;
and batch rendering from the command line with no window:

    animstudio --render <file.animproj | --sample> --preset "<preset>" --out <dir>

Presets include "Master PNG 16-bit", "ProRes 422 HQ" and "H.264 review".

## Quick start

1. Open the app. It opens on ready paper: draw straight away, nothing to set
   up first. The Start card in the middle offers **A drawing**, **An
   animation** or **A comic**, your recent files and More sizes. After an
   update it opens once on **What's New**.
2. New here? Press **Follow a lesson** on Start, or Help ▸ Lessons: nine
   lessons, rookie to experienced, each ending in a real file. The example
   project (a glowing ball bounce on twos, a camera pan across a wide
   background, sakura petals and a dialogue line) opens from the same page.
3. The dots on the line at the foot of the window count your Quick Start
   steps (also Help ▸ Quick Start), and each step has a **Show me**. They
   depend on what you are making:
   - A drawing: draw something, color it, add an effect, save it, export a
     file.
   - An animation: draw something, add a drawing, draw the next pose, play
     it, export a file.
   - A comic: draw on the page, cut it into panels, add a page, save it,
     export pages.
4. Move along the bar at the top, or press F4 to F9. 3D layout and the Brush
   maker sit under the More menu, and Ctrl+K finds everything.

## Keyboard basics (Gaban keymap)

| Keys | Does |
|---|---|
| Ctrl+K (or F3) | the command palette: every verb, searchable |
| F1 | the shortcut sheet |
| Space | play / stop |
| F / S (or Right, Down / Left, Up) | next / previous frame |
| W / Q | next / previous key |
| F4 to F9 | go to a step: Plan, Draw, Timing, Color, Effects, Export |
| T | hide the tools; press again, or tap the dot, to bring them back |
| E | new drawing in the current cell |
| X | eraser |
| Ctrl+Z, Ctrl+Y (or Ctrl+Shift+Z) | undo, redo |
| Ctrl+0, Ctrl+= / Ctrl+- | fit the view, zoom in / out |
| Space-drag, Shift+Space-drag | pan, rotate the view |
| Alt+A | add a drawing, wherever you are |

Keymap notes for an upgrader: over the sheet, the Overlay track moved from
Alt+K to Alt+O and the Background track from Alt+B to Alt+W, so Alt+K and
Alt+B each mean one thing now. A key you rebound yourself stays yours, and
What's New lists both by the name they carry on your keyboard.

Settings ▸ Shortcuts switches to the Industry keymap (and rebinds any verb);
neither keymap has a conflict. Hovering a control names its chord.

## Building

Rust (stable) on Windows. From the repo root:

    cargo test -p animstudio -p anim-core      # the whole suite, headless
    cargo build --release -p animstudio         # target/release/animstudio.exe
    bash tools/smoke_render.sh                  # renders the sample through the release exe and ffprobe-checks it
    cargo build --release -p animstudio-installer   # AnimStudio-Setup.exe (embeds the exe: build it first)

The tests never open a window, a GPU device or an audio device. The smoke
render needs ffmpeg and ffprobe (PATH, or the winget `Gyan.FFmpeg`
package); the app itself runs without them and offers image sequences.

## Where things live

- `crates/anim-core` — the document, the commands and their inverses, the
  SQLite file format (schema v11; v1–v10 files still open), and the CPU
  golden renderer every view and export agrees with.
- `app/` — the egui/wgpu shell (`animstudio`).
- `tools/installer`, `tools/iconforge`, `tools/smoke_render.sh`.
- `research/` — the design record: `PSD-approachable-ui.md` (this release's
  plan) and `PSD-approachable-ui-feedback.md`, `PSD-modernize.md` (the last
  release's plan and build log), `one-piece-2026-pipeline-research.md`,
  `workflow-trees.md`, and, kept as history, `DESIGN-SPEC-editor-ui.md`,
  `ROOM-CHARTERS.md` and the earlier PSDs.
