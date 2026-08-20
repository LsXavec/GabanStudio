# THE SHIPPING ROOM — installer, testers, relaunch-to-update

PSD gate 2026-08-19. Owner's order, verbatim (recorded 2026-08-19 in
PSD-brush-library, now gated): "After everything is completed in queue.
Create me a .exe Installer for my Github. Im gonna be giving some
testers access. Before doing that though Make sure That we install a
functionality similar to claude code that Has Relaunch to update when
the published Build gets written to the github. That way its up to date
besides the Dev build that we are testing. After we test Dev build
features we push it to the application but this will be a good draft
Time to atleast get it out there." And: "go ahead with the shipping
room."

PREMORTEM. A month on, this damaged the studio and a tester's trust.
The story: the updater downloaded a half-written asset and swapped it in
— the tester's app never opened again, and no old binary remained to go
back to. A second tester lost a morning's drawing because "relaunch to
update" restarted without carrying the session, and the update check
itself — run on the UI thread — froze the pen for two seconds every
launch on slow wifi. Worst: the updater trusted whatever URL the
release JSON named, and a compromised release could have handed testers
any executable silently.

ROOT: this stands on the swap being REVERSIBLE (the old exe survives
beside the new until the new one has actually run) and on the update
path reusing the PROVEN session-carry (the devloop's autosave+restore).
If either is weak, this is rubble.

NEVER-DO.
1. Never swap without a way back: the running exe is RENAMED aside, the
   new one takes its place, and the old is deleted only by a LATER
   successful launch. A failed download or short file never replaces
   anything (length checked against the release asset's declared size).
2. Never lose the session: relaunch-to-update saves exactly as the
   devloop handover does (autosave + session file) and the new process
   restores it. No update while a stroke is live or a dialog is open —
   the same guards, reused.
3. Never block the pen: version checks and downloads run on their own
   thread; the UI reads a channel. Failures are quiet chatter, never a
   refusal — being offline is a configuration, not a contradiction.
4. Never fetch from anywhere but the CONFIGURED repo's GitHub API and
   its release assets over https; the asset must be named for this app
   (animstudio-*.exe). The dev channel (devloop) stays untouched and
   wins when armed — a dev build never self-updates.
5. The installer is OUR OWN Rust binary (tools/installer) embedding the
   release exe — no third-party installer framework to vet. It writes
   to %LOCALAPPDATA%\AnimStudio, makes Start-Menu + uninstall entries,
   and never touches HKLM or anything system-wide.

BLAST RADIUS: new app/src/update.rs, Cargo dep ureq, Config
update_repo (serde default), Settings Plugins page updates section,
Foot UPDATE READY lamp + relaunch, devloop.rs gains the RESUME arming
for the restore path only (cited under this room), tools/installer new
workspace member. anim-core untouched.

STAGES: S1 update.rs (check/download/swap, tests on version compare +
rename dance) → S2 UI (lamp + settings) → S3 installer → S4 release
build + handoff (repo name + first release need the owner).

## Build log

- 2026-08-19 — S1–S3 DELIVERED (36 tests, 0 warnings; release binaries
  built: animstudio.exe 23.3MB + AnimStudio-Setup.exe 23.6MB).
  update.rs: GitHub latest-release check on its own thread (configured
  repo only, https only, asset must be animstudio*.exe and not *setup*);
  version compare garbage-proof (tested); download length-checked
  against the asset's declared size; the swap RENAMES the running exe
  aside and rolls back if seating fails (rename dance tested); a LATER
  successful launch sweeps the old binary. Foot lamp "relaunch to
  update (tag)" + Tally dot; click guarded like a devloop handover (no
  stroke, no dialogs) and carries the session through the devloop's own
  autosave+session machinery (devloop.rs gained resume_armed() — the
  RESTORE path arms on ANIMSTUDIO_RESUME; the watcher never does).
  Settings → Plugins → updates: repo field + check now + version note.
  Dev channel wins: an ANIMSTUDIO_DEVLOOP build never self-updates.
  tools/installer: our own Rust setup exe (NEVER-DO 5) — embeds the
  release exe, seats %LOCALAPPDATA%\AnimStudio, Start-Menu shortcut via
  the shell's COM object, HKCU-only uninstall entry, uninstall.exe that
  keeps the owner's data and says so. tools/release.sh = the whole
  path: tests → release builds → gh release create with both assets.
  Release builds surfaced one latent bug: egui Style.debug is
  debug-only — now cfg-gated in plate.rs.
  S4 WAITING ON THE OWNER: repo name + visibility for the first
  release (gh authed as LsXavec).
- 2026-08-19 — S4 SHIPPED. Owner's word: "LsXavec/AnimStudio public,
  releases only." The repo existed (private, his July source push —
  fully contained in local git as 3fb85c1's history, nothing lost):
  replaced with a README-only main, master deleted, flipped public.
  v0.1.0 published with animstudio.exe + AnimStudio-Setup.exe;
  anonymous download verified BYTE-IDENTICAL to the built binary.
  update_repo now DEFAULTS to LsXavec/AnimStudio — testers update with
  zero setup; the dev build (devloop-armed) still never self-updates.
- 2026-08-19 — AV FALSE-POSITIVE AMENDMENT (owner: "its flagging the
  other machine as a virus"). Causes named: unsigned fresh binary with
  ZERO version metadata, a setup exe that embeds+writes another exe
  (dropper pattern), and the cmd/ping self-delete idiom (textbook
  malware tell). Shipped: winresource version metadata on both exes
  (verified in the built binary), self-delete REMOVED (uninstall.exe
  stays behind, says why), and GabanStudio-portable.zip added to every
  release (bare exe, no installer machinery — least suspicious path).
  v0.1.4 assets re-cut. THE REAL FIX is code signing (an EV/OV cert or
  Azure Trusted Signing) — named as a future gate with a real cost.
- 2026-08-19 — OWNER AMENDMENT, verbatim: "again make the application
  updatable on the other users pc so I dont have to reinstall from git
  every time on close it should update." Two causes, two fixes:
  (1) DISCIPLINE: same-tag asset re-cuts are INVISIBLE to installed
  apps (0.1.4 == 0.1.4 → "up to date"). Every shipped fix now bumps the
  patch version — no more --clobber re-cuts. (2) AUTO-UPDATE: when the
  checker finds a newer release the download STAGES in the background
  (length-checked as before, swap NOT performed); the Foot lamp becomes
  an instant relaunch; and on normal app close a fully-staged build is
  swapped silently so the NEXT launch is the new version. Never blocks
  exit on an unfinished download; never touches a devloop-armed build;
  the rename dance stays reversible.
