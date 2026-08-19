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
- 2026-08-18 — OWNER AMENDMENT, verbatim: "Import the Krita bundles that
  are installable in application for now Ill test From the Public krita
  brushes later". Shipped `installed_krita_paths()` (Program Files
  Krita (x64) bundles + paintoppresets, plus %APPDATA%/krita — where
  community bundles land too) and a rail button "import installed
  Krita's brushes" (shown only when a Krita exists), drained beside the
  file dialog, never mid-stroke. SMOKE-PROVEN on this machine: 268
  presets imported through the real path, 268 thumbnails cached, every
  size/opacity/flow inside our honest ranges (permanent test, skips
  where Krita is absent). 21 tests, 0 warnings.
- 2026-08-19 — OWNER AMENDMENT, verbatim: "I would like to add now A
  pluggins section in settings that has a sub catagory for Brush banks
  or custom brush imports that supports some of the more modern brush
  data files for importation." Settings gains PLUGINS → brush banks:
  every imported preset carries its BANK (source file); the page lists
  banks with counts and a held REMOVE BANK. New importers, honest each:
  Photoshop .abr (sampled brushes v1/v2 + 8BIMsamp v6+, RLE masks →
  stamp presets); Procreate .brush/.brushset (Shape.png → tip,
  Grain.png → grain; the NSKeyedArchiver params are NOT parsed — sizes
  default, said in the hover); bare .gbr/.gih/.png as single stamps.
- 2026-08-19 — OWNER QUEUE ADDITION, verbatim: "After everything is
  completed in queue. Create me a .exe Installer for my Github. Im
  gonna be giving some testers access. Before doing that though Make
  sure That we install a functionality similar to claude code that Has
  Relaunch to update when the published Build gets written to the
  github. That way its up to date besides the Dev build that we are
  testing. After we test Dev build features we push it to the
  application but this will be a good draft Time to atleast get it out
  there." QUEUED as THE SHIPPING ROOM (own gate when reached): GitHub
  Releases channel + relaunch-to-update (the devloop's proven
  shadow-swap is the updater's core) + installer. Needs from the owner
  at build time: the GitHub repo. Dev channel stays devloop.
- 2026-08-19 — PLUGINS PAGE + BRUSH BANKS DELIVERED (32 tests, 0
  warnings): Settings → plugins lists every bank (source file) with
  count + held REMOVE BANK; dependency audit says "every brush is
  self-contained" or names the missing (Aka) — imports persist in
  config + AnimStudio caches with no live dependency on Krita.
  brushbank.rs: Photoshop .abr (v1/v2 sampled + 8BIMsamp v6/7/9/10,
  PackBits — precedence bug in the RLE caught BY THE TEST before
  shipping), Procreate .brush/.brushset (Shape/Grain PNGs, luma→alpha;
  params honestly defaulted), bare .gbr/.gih/.png stamps. Rail dialog
  takes all formats. Krita scan tags bank "krita".
  QUEUED NEXT: THE SHIPPING ROOM (installer + GitHub
  relaunch-to-update), then his brush-creation project.
