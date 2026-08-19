# THE BRUSH LIBRARY — Krita imports living in the rail

PSD gate 2026-08-18. Owner's order, verbatim: "Lets work on getting
those Krita brushes imported into our application. All the addon
brushes and the capability to load brushes from community brushes
aswell. That way We dont have to make all the slow process. Lets
encorporate it into the Right canvas opening window. Figure out a good
UI layout. and Commit."

PREMORTEM. A month on: the library damaged trust. Two ways. A community
bundle imported 400 presets whose STROKES looked nothing like Krita's —
our dab is not Krita's engine — and the artist blamed their hand before
they blamed the import; and the rail, now carrying hundreds of
thumbnails, wrote image bytes into config.json until every save chugged.

ROOT: this stands on the import being an HONEST MAPPING into our dab
(name, size, flow, opacity — said plainly), and on thumbnails being
CACHE, never config. If either is weak, this is rubble.

NEVER-DO.
1. Never pretend Krita fidelity. An imported preset paints with OUR
   engine at the preset's size and strength; the scope line in kpp.rs
   stays the law. No silent wrongness — unparseable file = counted and
   said, never guessed.
2. Thumbnails live on disk (%APPDATA%/AnimStudio/brush_thumbs), loaded
   lazily as textures, painted-dab fallback when absent. NEVER in
   config.json. Presets stay the small serde structs they are.
3. The rail BROWSES, ARMS and IMPORTS; editing stays in Settings →
   Brushes. No engine/stroke-path changes. Import never fires
   mid-stroke.

BLAST RADIUS: kpp.rs (thumb extraction+cache), canvas.rs (rail library
section + request flag + texture cache), main.rs (drain request → file
dialog → import_files → presets_dirty). Commit when green (his word).

## Build log

- 2026-08-18 — BUILT + DELIVERED (0 errors, 0 warnings, 20 tests).
  kpp.rs: thumb_dir/thumb_key/save_thumb — each imported .kpp's own PNG
  becomes a 64px cache file (nearest-neighbour, RGBA8 expand; exotic
  formats skip to the dab fallback). Bundle path fills the cache per
  entry; dead thumbless parse_bundle deleted. canvas.rs: rail LIBRARY
  section — count value-line, search (appears past 6 presets), 3-wide
  48px thumbnail wells (Krita's own icons, tinted none; painted dab
  when uncached), Tally ring on the armed preset, hover = name + size,
  click = apply_preset, "import brushes…" raising request_brush_import.
  main.rs Editor::ui drains the flag with &mut presets, never
  mid-stroke; failures refuse, outcomes chatter with counts.
