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

OWNER RULING 2026-08-19, verbatim: "The User besides the host is just a
live control window like they dont get the Save functionalities of the
host. Their strokes are written to the host pc. So now weird saving or
anything Its just a 1:1 control schema window for a User if they are
joined in a session. So we shouldnt be hitting those lag spikes like
that." This IS the room's original v2 vision (the command-stream
replica) now ordered built: the host STREAMS every applied command
batch to guests, who apply it to their mirror engine WITHOUT history
(one truth; the mirror never undoes on its own). Whole-document
snapshots remain ONLY for the join. Guests lose Save/Open — refused
with why (the host owns the file; their strokes already live there).
anim-core gains a runtime-only applied-commands tap (no format change —
the same discipline as the author tags).

THE MIRROR BUILT 2026-08-19 (v0.1.9, same evening as the ruling):
engine gains a runtime-only applied-commands log (mirror_log +
drain_applied) and apply_mirror (no history — a mirror never undoes on
its own); Command + constituents gained serde derives (RUNTIME wire
only; the SQLite disk format untouched). The host streams every applied
batch (its own strokes, guests' strokes, fills, retimes — everything)
as FRAME_CMDS, serialized on the writer thread, deserialized on the
reader. Whole-document snapshots now fire ONLY for: fresh joins, guest
ResyncRequest (a batch that would not apply), and after any undo/redo
on the host (history_dirty — the PINNED contract: a mirror replaying a
stream diverges after a host undo; proven by test alongside the
stream-equality proof and a serde roundtrip). Guests lost Save/Open
per the ruling, refused with why. ALSO this evening (v0.1.5–v0.1.8):
auto-update staged in background + installs on close; writer threads
(no socket write on the UI thread — THE freeze); async snapshot save/
load; wet-tail cap; versions visible in title/Foot/room-lamp hover.
