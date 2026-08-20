---
name: devloop-deliver
description: SEEDLING (advisory) — the AnimStudio delivery checklist; ship a roomed UI change through the shadow-copy dev loop with verification and a build-log row
---

# devloop-deliver — SEEDLING (advisory)

**Status: PROBATION.** Born 2026-08-17 by /skill-forge under the operator's
standing stanza ("Build the Tools in this workflow that can recognize a
skill gap … Automatically implements a new one …"). Advisory only: its
output is a checklist, never a gate. A human's word graduates it.

## Evidence (law 1)

The same sequence hand-run across 3+ distinct days, from this workflow's
own records (research/PSD-devloop-uihook.md and PSD-editor-repaint.md
build logs):

- 2026-08-15 — devloop/uidump build cycles ("Build clean, no warnings.
  Full workspace suite: 14 suites, 0 failures").
- 2026-08-16 — F12 dump verification cycles (ui_dump/state_000/001
  timestamps).
- 2026-08-17 — six+ delivery cycles logged verbatim ("delivered to the
  running app via dev-loop handover (PID 27600)", "(PID 16360)",
  "(PID 17872 — NB Windows reused the PID …)", "(PID 24988)",
  "handover 19:36:17").

The PID-reuse incident (17872) is the seedling's reason to exist: an
unverified handover check gave a false reading, caught only by process
StartTime. The checklist encodes that lesson.

## The checklist

1. `cargo check -p animstudio` FIRST — a broken build must never reach
   `cargo build` (check produces no exe, so the running app never blinks
   on an error).
2. `cargo test -p animstudio` — suites stay green before delivery.
3. `cargo build -p animstudio` — this IS the deploy: the running shadow
   notices the new exe and hands over.
4. Verify the handover by **process StartTime** (PowerShell
   `(Get-Process animstudio-devloop).StartTime`), never by PID alone —
   Windows reuses PIDs (proven 2026-08-17, PID 17872).
5. Verify `dev_session.json` shows `"pending": false` — the restore
   completed and cleared its flag.
6. Append a build-log row to the governing room doc
   (research/PSD-editor-repaint.md) — what shipped, the verification,
   what remains.
7. The owner looks (F12 or eyes) before the next phase starts — the
   room's phase-boundary law; this checklist never substitutes for it.

## Blast radius / stages

Reads process state and the session file; runs cargo; writes ONLY the
room doc's build log. Valid on the owner's machine, editor stage, while
research/PSD-editor-repaint.md (or a successor room) governs. Advisory:
it reminds, it does not gate.
