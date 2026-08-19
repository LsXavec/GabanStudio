# SESSION V2 — author-tagged history, guest edits that SAVE

PSD gate passed 2026-08-18. Owner's purpose, verbatim (2026-08-18):
"The other user will have the full build of animstudio on their device.
The connection should allow them to select and do basically everything
the Host can. There edits are just stored independantly but the layers
are persistant and you can be on the same layer. Individual Undo redo
archetype. So you can draw together." — and, ordering the gate:
"make sure we actually make some way that the layers work even if its
not a shared archetype. Viewing live that the other person is drawing
and being on the same layer persistant accross the host is the most
important as then Edits get saved."

PREMORTEM. It is a month on. V2 damaged the studio. The story: guest
strokes were applied by a second writer, so two histories raced — an
undo on one machine resurrected pixels the other had painted over, and
a tile patch computed against a stale mirror landed on the WRONG
drawing after the host retimed a column. Worse, per-artist undo let a
guest undo a command buried under three of the host's: the engine
un-applied it out of order and the layer came back with holes. The
file saved from the host no longer matched what either artist saw.

ROOT: this stands on ONE WRITER (the host's engine) and on edits that
name their target by IDENTITY (drawing id + layer NAME) and are refused
when that identity is gone; and on undo that is only ever applied to a
command still safe to remove. If any is weak, this is rubble.

NEVER-DO.
1. Never a second writer. Guest edits are REQUESTS; the host's engine
   applies them through its own guarded paths, and the result returns
   to everyone as the host's snapshot. A guest never commits locally.
2. Never target by position. An edit names drawing id + LAYER NAME
   (defect-16 semantics). If the host can't resolve it, the edit is
   REFUSED back to its author through the Aka lane — never guessed at,
   never applied to a neighbour.
3. Never out-of-order undo. An author's undo is honoured only while
   their command is still safely removable: nothing later touches the
   same drawing+layer. Otherwise it REFUSES with why. No selective
   surgery on a shared stack.
4. Never widen the format silently: the author tag lives in the runtime
   history only. Saved files stay byte-identical in schema — a tag is
   session metadata, not document data.

BLAST RADIUS: `crates/anim-core` — an author field on runtime history
entries + `undo_last_by(author)` with the safety test + tests (NO
serde/format change). `app/src/net.rs` (EditRequest/EditRefused msgs),
`app/src/canvas.rs` (guest pen-up sends instead of commits),
`app/src/main.rs` (host applies + re-broadcasts). No UI redesign.

STAGES: v2a = guest raster edits land on the host, by-name layers,
refusals home to their author. v2b = per-author undo/redo. Both here;
vector-stroke edits, selection transforms and retiming by guests stay
host-only until their own pass.

## Build log

- 2026-08-18 — V2a+V2b DELIVERED (20 app tests + engine suites green,
  0 warnings). ENGINE (anim-core, the room's authorized reach): runtime
  author tag on history steps (`Applied.author`, NEVER serialized),
  `set_author`, `undo_last_by`/`redo_last_by` with the SAFETY LAW — an
  author's step is undone only while nothing later touches the same
  (drawing, layer); otherwise it refuses. `undo_depth_by` for lamps.
  Three tests pin the laws: each author undoes only their own; undo
  refuses under a later same-layer edit and succeeds once it clears;
  and a tagged history saves a document identical to an untagged one
  (the file-schema claim, tested by round-trip since the container is
  SQLite and byte-equality is not a meaningful instrument).
  APP: guests never commit — pen-up ships the finished tile patch
  (drawing id + LAYER NAME) to the host; the host applies it through
  its own engine under the author's tag, re-hashing every tile on
  arrival (TileData::from_vec) and length-checking it, so a malformed
  patch cannot poison content addressing; the before-state comes from
  the HOST's document so undo stays exact on its own truth. Refusals
  travel home and surface in that artist's Aka lane. Guest Ctrl+Z/Y
  become requests to the host. Guests cannot yet create cels (refused
  with why) — named, not silently broken.
- 2026-08-18 — SESSION PAGE fix found while the owner watched: Generate
  produced a secret that was invisible until hosting, though enrollment
  must happen BEFORE hosting. The enrollment block (secret + LIVE code
  + countdown + "Authy should show this same number") is now shared,
  shown whenever a secret exists, with an Aka line for an invalid
  secret. Owner confirmed: "code shows and counts down."
  NOTE for future iteration: the dev loop refuses handover while a
  dialog is open — iterating on the Settings page itself means
  close → blink → reopen. Working as designed (never restart over a
  modal); worth its own amendment only if it becomes a real tax.
