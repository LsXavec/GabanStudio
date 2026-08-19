# Dev loop + UI hook — the room

PSD gate passed 2026-08-15 — roots ratified by the owner, both bind as written.

---

## STANZA

**PURPOSE.** Cut the restart tax out of the audit cycle, and give Claude a way
to see the interface it is changing — without either instrument becoming a
thing that lies.

**ROOT (hot-reload).** This stands on reloaded code and live state agreeing
about memory layout; if that's weak, this is rubble.

**ROOT (interface hook).** This stands on the hook observing the same build he
actually runs; if that's weak, this is rubble.

**NEVER-DO.**
1. Never hot-swap code into a live process. The reload path is
   save → relaunch → restore, so no live state is ever reinterpreted through
   a changed struct layout. No dylib, no `hot-lib-reloader`, no exceptions.
2. Never restart over unsaved work, and never restart mid-gesture. A restart
   that cannot prove the document is recoverable does not happen.
3. Never give the hook its own render path, its own feature flag, or its own
   window. It observes the build he already runs, or it does not exist.
4. Never let "I looked at the dump" substitute for a test. The hook reports
   what is on screen; it does not certify that anything is correct.

**BLAST RADIUS.** `app/src/devloop.rs` (new), `app/src/uidump.rs` (new), and
the wiring lines in `app/src/main.rs` that call them. No engine crate
(`crates/anim-core`) changes. No document format changes. No changes to any
existing render path.

**STAGES.** Valid in the editor stage, on the owner's own machine, during
development. The dev loop is inert unless `ANIMSTUDIO_DEVLOOP=1`. The hook is
compiled into the normal build and is inert until asked.

---

## Design (governed by the stanza above)

### 1. Dev loop — state-preserving restart

Not hot-reload. The running app watches its OWN executable's mtime. When
`cargo build` replaces it:

1. Refuse if unsafe: a pen stroke is down, a modal is open, or an export is
   running. Re-check next tick.
2. Snapshot the session: project path, or — if the document is unsaved or
   dirty — write an autosave beside the session file first.
3. Write `dev_session.json` with `pending: true`.
4. Spawn the new binary, exit this one.
5. The new process sees `pending: true`, restores, clears the flag.

Because state crosses the boundary as a FILE in the document's own format,
never as memory, NEVER-DO 1 is structurally satisfied — there is no layout to
disagree about.

### 2. UI hook — on-demand dump

A key (F12) or `ANIMSTUDIO_UIDUMP=1` writes, into `ui_dump/`:

- `frame_<n>.png` — the actual rendered frame, captured from the live
  viewport via egui's screenshot command. Same render path he sees.
- `state_<n>.txt` — what the app believes it is showing: project, resolution,
  fps, current frame, layer/cel counts, open panes and their rects, active
  workspace stage, tool + brush, and the config values that affect rendering.

The dump is a READ. It never mutates document state.

---

## How to run it

Dev loop (one terminal, left running):

```
set ANIMSTUDIO_DEVLOOP=1 && cargo run -p animstudio
```

Then in another terminal, whenever code changes: `cargo build -p animstudio`.
The open app notices, saves, and comes back on the same frame. Without the
env var the binary behaves exactly as before.

UI dump: press **F12** in the app. Files land in `<exe dir>/ui_dump/` as
`frame_NNN.png` + `state_NNN.txt`.

## What the build taught us

Two things were found by building rather than by planning, and both are now
load-bearing:

1. **Windows will not overwrite a running `.exe`.** `cargo build` fails with
   "Access is denied (os error 5)" while the app is open — which would have
   made the entire dev loop useless. The app now stages a shadow copy of
   itself (`animstudio-devloop.exe`), runs that, and leaves cargo's output
   path unlocked. The shadow watches the ORIGINAL path and relaunches the
   original (never its own stale copy, which would loop on old code forever).
2. **egui's `Color32` is premultiplied; PNG is not.** Writing the screenshot
   bytes straight through looked correct for an opaque window and would have
   silently darkened anything translucent. Caught by
   `png_roundtrips_pixels_in_the_right_order`, which is why that test uses a
   translucent pixel rather than the three opaque ones that pass either way.

## Audit — 2026-08-15, 38 agents, 30 confirmed findings

The stanza was carried verbatim into every reviewing agent as governing law.
It found a defect that would have destroyed work, in the exact code written
to prevent that.

**CRITICAL — the restart retargeted Save to scratch storage.** `from_autosave`
was set to `file_path.is_none()`, so the corrective branch fired only for a
document that had NEVER been saved, and was skipped for one with a real file.
A project opened from `C:\art\shot01.animproj` came back with `file_path`
pointing at `%APPDATA%\AnimStudio\dev_autosave.animproj`; the next Ctrl+S
wrote there silently, reported "saved", and left the real file frozen at its
pre-restart state. NEVER-DO 2, broken by the code that quotes it.
Fixed by carrying the true target in its own field (`Session::origin`) and
restoring it unconditionally, guarded now by
`the_real_save_target_survives_a_handover`.

Also fixed:

- **Restore cleared `pending` before the load was attempted**, so any failure
  threw away the only pointer to the work, silently. Now cleared only after
  the document is genuinely back; a failed load leaves the flag set and says
  where the work is.
- **Session write failures were swallowed** and the process exited anyway,
  stranding a good autosave nothing would reopen. Now returns `Result` and
  aborts the handover.
- **The handover ran BEFORE the frame's input was drained**, so the
  `stroke_active()` guard could not see a stroke that began that frame — a
  mid-gesture restart. Poll moved to the end of `App::ui`.
- **Every message from both instruments was written to a field nothing read.**
  A failed autosave and a successful one looked identical. Both now route to
  the status line.
- **Restore was not gated on `ANIMSTUDIO_DEVLOOP`**, so an ordinary launch
  could resurrect a stale dev session. Now gated, and sessions expire.
- **`poll()` latched true forever**, so a failed relaunch re-serialised the
  whole project every 400ms. Added `give_up()` for failures (refusals still
  retry, as intended).
- **The shadow's baseline mtime was sampled after GPU init**, a window long
  enough for a rebuild to land unseen — the shadow could run older code than
  the file it watched. Now sampled at the top of `main`.
- **Staging failure left the loop enabled but watching a locked binary.** Now
  returns `Staging::Failed`, the loop turns itself off and says why.
- **Dump sequence reset to 0 every restart**, so the "after" dump destroyed
  the "before" — the exact comparison the hook exists for. Seeded from disk.
- **The record lied in three places**: `(modified)` was really "has no path",
  `tool` ignored the armed tool and reported "draw" during select, and the
  pane list included tabs hidden behind stacks. All now read the state that
  drew the frame.

## Verification status (honest)

- Build clean, no warnings. Full workspace suite: 14 suites, 0 failures.
- `uidump`'s PNG encoder and text record are unit-tested (a GPU is not
  required for either).
- BOTH exercised live on the rig 2026-08-17, owner at the controls:
  - F12 through the live viewport: `frame_002.png` + `state_002.txt` written,
    sequence continued from disk, record reported the real armed tool and
    "no file yet" honestly.
  - Rebuild handover, hard case (unsaved scribble): cargo built with no
    exe-lock error (shadow held the lock instead), old shadow (PID 18736)
    noticed, autosaved to `dev_autosave.animproj`, spawned the new build,
    exited; new process (PID 1012) restored on the same frame and cleared
    `pending` only after the restore. `origin: null` — Save still prompts,
    as it must for a never-saved document.
