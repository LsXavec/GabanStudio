# GABAN STUDIO — the rename

PSD gate 2026-08-19. Owner's word, verbatim: "rename it to Gaban
studio" (from the naming slate; gaban = 画版, the drawing-plate — the
Plate & Pencil identity in one coined word).

PREMORTEM. A month on, the rename stranded the testers. The story: the
new release shipped only gaban-studio.exe assets, and every installed
v0.1.2 updater — which looks for an asset named animstudio*.exe —
reported "release carries no animstudio exe" forever. The repo rename
broke the baked-in update_repo for anyone whose HTTP client didn't
follow the redirect. And the installer renamed its install directory,
so existing installs became orphans with two copies fighting over one
Start-Menu entry.

ROOT: this stands on the split between DISPLAY identity (what people
read: Gaban Studio) and PLUMBING identity (what machines match:
animstudio.exe, %APPDATA%/AnimStudio, the updater's asset contract) —
display renames NOW, plumbing stays stable until a dedicated migration
gate. If that split blurs, testers strand.

NEVER-DO.
1. The updater's asset contract (animstudio*.exe) and the exe/dir names
   DO NOT change in this pass. Every future release keeps shipping the
   asset the installed updaters match.
2. The repo rename rides GitHub's permanent redirect — VERIFIED live
   against the old URL before anything ships; the baked default
   update_repo moves to the new name for new builds.
3. Everything human-facing renames: window title, installer display +
   shortcut, README, release titles, repo name/description.

BLAST RADIUS: main.rs title strings, installer display strings,
tools/release.sh title, README, repo rename + default update_repo,
version bump + v0.1.3 release. No format, no paths, no exe names.

## Build log

- 2026-08-19 — SHIPPED as v0.1.3. Display renamed everywhere (title,
  eframe app id, installer text + DisplayName/Publisher, Start-Menu
  shortcut now "Gaban Studio.lnk" with the uninstaller sweeping the old
  name too, release titles, README, repo → LsXavec/GabanStudio with
  description). Plumbing UNCHANGED per NEVER-DO 1 (animstudio.exe,
  %APPDATA%/AnimStudio, updater asset contract). NEVER-DO 2 verified
  live: the old API URL 301s and serves the release; asset download via
  the old URL byte-counted; ureq default max_redirects=10 confirmed in
  the vendored source. Baked default update_repo → LsXavec/GabanStudio
  for new builds.
