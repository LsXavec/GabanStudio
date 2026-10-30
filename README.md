# Gaban Studio

*Gaban* (画版) — the drawing-plate. A node-graph 2D animation studio
built on the traditional anime pipeline — layout, genga, douga, shiage,
satsuei, henshū — from the first rough to the delivered file: a GPU brush
engine that imports Krita, Photoshop and Procreate brushes, an exposure
sheet that works like the paper timesheets, compositing FX that look the
same on screen and in the delivery, and a collaborative session room.

Version 0.5.0.

## Install

Download **AnimStudio-Setup.exe** from the
[latest release](../../releases/latest) and run it. Per-user install, no
admin needed. The app checks this repo's latest release and offers
"relaunch to update" when a newer build is published.

(The binaries keep their original `animstudio` file names so installed
apps keep updating seamlessly across the rename.)

## What it does, by who reaches for it

**Animator (genga, douga, timing).**
- The X-sheet as the master: insert and delete frames (every column and
  camera key ripples), holds, retimes on 1s/2s/3s, reverse, typed entry,
  memos and the dialogue row — every edit one undo step.
- Keys, inbetweens and layout drawings marked on the sheet; spacing charts
  and Make inbetweens; the light table with onion depth 0–5, pins, shift
  and trace.
- Canvas rotate and mirror, a 0–10 stabilizer, Shift-click straight lines,
  brush size keys and the Alt-click eyedropper.
- Never lose work: an unsaved guard, autosave every 5 minutes into the data
  folder (newest 3 kept), recovery after a crash, and one `.bak` beside the
  file.

**Painter (shiage).**
- Exact colour: new projects keep chips exactly as picked, so the screen
  and the export bytes equal the chip. Older projects open unchanged and
  convert on request, with a backup written first.
- Recolour by editing a chip, refill, chip pick, palette variants and
  palette maps, and the nuri check (paint check) for holes and stray
  colours across a cut.

**Compositor (satsuei).**
- FX run once on the CPU, so the composite view and the delivered frames
  agree: blur, glow (T光), diffusion, grade (lift/gamma/gain, exposure,
  saturation, temperature) and .cube LUTs.
- Role passes, trace-line absorb, para, light wrap, rim, vignette, grain,
  directional motion blur, mattes and the satsuei template.
- The camera node: pan, T.U./T.B., rotate and shake over paper larger than
  the frame, multiplane depth and depth of field, eased in the graph editor.
- Particles, speed lines and focus lines as generator nodes.
- The render queue: PNG16, TIFF16 and EXR built in; ProRes, DNxHR, H.264
  and H.265 through an ffmpeg you install, with the sound muxed, burn-ins
  and line tests.

**3D artist.** glTF and OBJ import with a toon render (outlines and passes),
View3D with Blender navigation, the cut camera keyed from the view, image
sequences from other apps referenced on disk, and the canvas 3D underlay.

**Manga artist.** B5 and A4 pages at 350 dpi and 800 px webtoon strips,
panels with gutters, tones, perspective rulers, lettering with tategaki and
balloons, and page export.

**Editor (henshū).** Sound tracks (WAV, MP3, OGG, FLAC) with waveforms,
audio-locked playback, scrub, lip flap and mixdown; the shinkō board (cut
board) with retake notes; the episode timeline with TRT; CMX3600 EDL export;
and batch rendering from the command line with no window:

    animstudio --render <file.animproj | --sample> --preset "<preset>" --out <dir>

Presets include "Master PNG 16-bit", "ProRes 422 HQ" and "H.264 review".

## Quick start

1. Open the app. The Welcome screen offers templates, recent files and the
   sample cut (a ball bounce on twos over a BG plate, with a serifu line).
   After an update it opens on **What's New**.
2. Pick a template (or Custom size…), or open the sample.
3. Follow the Quick start checklist on the Welcome screen (also Help ▸ Quick
   start): make a drawing, mark it Key, time it on the sheet, paint it,
   render it. Each step has a "Show me".
4. Switch rooms from the slate: layout, genga (drawing), shiage (ink &
   paint), satsuei (composite), henshū (edit); manga, 3D layout and the
   brush forge sit off the spine.

## Keyboard basics (Gaban keymap)

| Keys | Does |
|---|---|
| Ctrl+K (or F3) | the command palette: every verb, searchable |
| F1 | the shortcut sheet |
| Space | play / stop |
| F / S | next / previous frame |
| W / Q | next / previous key |
| E | new drawing in the current cell |
| X | eraser |
| Ctrl+Z, Ctrl+Y (or Ctrl+Shift+Z) | undo, redo |
| Ctrl+0, Ctrl+= / Ctrl+- | fit the view, zoom in / out |
| Space-drag, Shift+Space-drag | pan, rotate the view |

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
- `research/` — the design record: `PSD-modernize.md` (this release's plan
  and build log), `DESIGN-SPEC-editor-ui.md` (the plate: tokens, no green),
  `one-piece-2026-pipeline-research.md`, `workflow-trees.md`,
  `ROOM-CHARTERS.md` and the earlier PSDs.
