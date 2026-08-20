# PSD — the multiplayer rescope (strokes, not pixels)

Room opened 2026-08-20. Supersedes the wire design of PSD-session-room /
PSD-session-v2 (their rulings stand — the mirror, the control window,
author-tagged history — but the TRANSPORT is rescoped).

**PSD gate passed 2026-08-20 — root: bit-exact stroke replay, audited
cheaply, repaired narrowly.** Owner's word: "ratify push till
completed" — all stages build to completion, then ship.

---

## 1. PREMORTEM (candidate story, for the owner to ratify or rewrite)

It is October 2026. The rescope failed and the testing crew drifted
away. The story of how: we streamed strokes and trusted that two
different GPUs would replay them into bit-identical pixels. They did
not — the guest's card rounded one f16 blend differently, the canvases
drifted a hair apart every stroke, and because we had no way to NOTICE
the drift, it compounded silently until the two artists were drawing on
two different pictures. When we finally bolted on a repair, it shipped
whole layers again and was laggier than the tile design it replaced.
The debug we needed lived on the guest's machine, and we never went and
got it.

**Root line: this stands on BIT-EXACT CROSS-MACHINE STROKE REPLAY,
audited cheaply and repaired narrowly; if replay drifts unseen, or the
repair is fatter than the drift, this is rubble.**

## 2. STANZA — the owner's words, verbatim (2026-08-20)

> A I want you to make a debug that is installed and connects through
> the User join and stamps Data to it thats ordered. So then the users
> computer gives you what you need for the debug. I want to rescope the
> actual Design of this Multiplayer drawing Feature. I want you to
> figure out what skills will increase the likeliness of this design
> Being a working state. Here is the full want — I want The application
> to have a multiplayer connection. The Drawing the Host sees is live
> and as low latency as possible. Any Connected User Specifically Has
> the functionallity To draw and edit the Canvas regardless of who drew
> what. If they undo Their Last stroke Gets overwritten. Any connected
> user can create edit or delete frames cels basically everything that
> the Host can do. But the Hosts PC is the save location of all the
> Data. Only Some Features like the Layout and Brush Forged Brushes are
> Held from Application build to the other. That way when you join you
> get the whole Functionality but the persionalization persists accross
> Hosted sessions. Put in the road map a settings Sub Catagory for
> saving Application Layout presets. so you can load the hosts preset
> from there but it defaults to the Installed Applications or
> configured preset of any user on the application and the custom Host
> Layout is loadable. But yes Multi-Artist support feature is the whole
> target. Without any Lag. We need to derive this ourselves. Figure out
> the best way to not eat up bandwidth saving the exact brush strokes
> and Live Drawing User- to host. Low latency Live Colaboration.

- **PURPOSE:** Multi-artist live collaboration without lag — the whole
  target. Full guest parity, per-artist undo, host owns the file,
  personalization stays per-user.
- Root line: stands on bit-exact cross-machine stroke replay, audited
  cheaply and repaired narrowly.
- **NEVER-DO:**
  1. **Never ship pixels in the steady state.** Pixels cross the wire
     only at join and in a targeted repair. If a design change would
     put tile bytes back on the per-stroke path, stop and reopen this
     room.
  2. **Never apply out of host order.** The host is the sequencer;
     every peer (guests AND the host's own engine) applies edits in the
     host's numbered order. Local prediction may run ahead of it, never
     instead of it.
  3. **Never trust replay unaudited.** Every replayed stroke is
     hash-checked against the host's tiles; a mismatch repairs exactly
     the mismatched tiles, quietly, and is stamped to the debug log.
  4. **Never build a stage the ordered debug channel cannot see.**
     Instrument first (Stage 0 ships before any transport change), and
     every later stage stamps its own vitals.
- **BLAST RADIUS:** `app/src/net.rs`, the session pump in
  `app/src/main.rs`, the stroke begin/point/end seams in
  `app/src/canvas.rs`, `crates/anim-core` (history + a replay entry
  point), `app/src/config.rs` (roadmap UI only). The disk format
  (SQLite via store.rs) is untouched. The single-player path is
  untouched — everything rides session state.
- **STAGES:** 0 debug channel → 1 stroke wire + replay + audit →
  2 live remote wet ink → 3 full-parity commands + per-artist undo →
  4 personalization (layout presets). Valid on LAN and direct-IP TCP
  sessions; internet relay is a later room.

## 3. THE DERIVATION — why strokes, not pixels

**What we ship today (the flaw).** A guest stroke is read back from the
GPU and shipped as tiles: 64×64×4 channels of u16 = 32 KB raw per tile,
~4× that as JSON. A stroke crossing 20 tiles ≈ 2–3 MB *per pen lift*,
sent only AFTER the lift, then echoed back. A join snapshot measured
89,960,448 bytes before deflate. Pixels are the heaviest possible
representation of a stroke, and they arrive latest.

**What a stroke actually is.** The brush engine was built deterministic
by law: no wall-clock, no RNG — jitter/scatter/fuzz come from
`hash01(dab_index, salt)`, curves from `curve_eval`, and the forge
preview shares the exact stroke-path math. Therefore a stroke is fully
determined by its INPUT: brush definition + color + size + the point
stream (x, y, pressure). Ship that, and every machine can grow the
same pixels itself.

**The arithmetic.** One packed point = 8 bytes (x, y as quarter-pixel
i16s; pressure as u16; 2 spare). A tablet at 240 Hz = ~2 KB/s while a
hand is actually drawing — batched per UI frame, so remote ink runs one
frame plus RTT (~1–2 ms on LAN) behind the pen. Against 2–3 MB per
lift: roughly a thousandfold cut, and it's LIVE during the stroke
instead of after it. Ten artists drawing at once ≈ 20 KB/s.

**Ordering (the sequencer).** All edits — strokes and Commands alike —
flow to the host. The host assigns one global sequence number, applies
to the authoritative engine, and rebroadcasts to every peer including
an echo to the origin. Every peer applies in that order; canvases
cannot diverge by ordering. Guests draw with zero perceived latency
because their own stroke renders locally as prediction; the echo
confirms it (and in the rare conflict, host order wins and the repair
path reconciles).

**Audit and repair (the humility clause).** Determinism is proven on
one machine; across GPUs, f16 blending can differ by rounding. So we
verify instead of hoping: after committing a replayed stroke, each peer
hashes the touched tiles (the hash already lives on `TileData`); the
host broadcasts its own hashes for the same stroke; any peer whose
hashes differ requests exactly those tiles. Steady state: zero pixel
traffic. Worst case (a truly divergent GPU): a few tiles per stroke —
still hundreds of times lighter than today — and every mismatch is
stamped to the debug log so drift is a measured fact, not a mystery.

**Per-artist undo without snapshots.** History entries are already
author-tagged and `undo_last_by(author)` exists. Undo becomes an
ordered event like any other: guest sends UndoRequest, host applies
`undo_last_by`, broadcasts `Undone{author}` in sequence, every peer
replays the same call on its own identical history. "If they undo,
their last stroke gets overwritten" — and nobody else's. The
snapshot-on-undo contract from the mirror era dies with the tile wire.

## 4. THE DESIGN

**Stage 0 — the ordered debug channel (built first, on the CURRENT
system).** Every `slog` line gains a per-machine monotonic sequence
number. A guest buffers its lines and ships them over the live session
every ~2 s (`Msg::DebugLog{seq, lines}`); the host appends them to
`session_log_remote.txt` tagged `[name #seq +t]`, arrival-ordered (TCP
keeps per-peer order; seq numbers expose any gap). Both sides also
stamp a STATS line every 5 s: fps, frame-gap max, queue depths, bytes
in/out. One file on the host machine tells the whole system's story in
order — the guest's computer gives us what we need. This lands before
any transport change, so it observes the v0.1.15 lag question too.

**Stage 1 — the stroke wire + replay + audit.** New binary frame
`FRAME_STROKE`: `StrokeBegin{author, stroke_id, seq, target
(cut/frame/cel/layer), brush def inline, color, size, mode}`,
`StrokePoints{stroke_id, packed points}` (one per UI frame while
drawing), `StrokeEnd{stroke_id, point_count}`. The brush def travels
INLINE — a forge brush the host never installed still replays exactly,
which is what keeps forge brushes per-user (stanza law) without
breaking replay. On StrokeEnd every peer runs the stroke through the
one shared replay entry point (same code path local drawing uses) and
commits it as an author-tagged history entry. Then the audit: touched
tile hashes compared to the host's, mismatches repaired tile-by-tile.
The old EditTiles path and the guest readback-diff machinery retire.

**Stage 2 — live remote wet ink.** While StrokePoints stream in, every
peer feeds them to the real brush engine as a wet overlay — you watch
the actual brush lay ink live, not a ghost polyline. The wet layer
drops when the committed replay lands (same swap local strokes already
do). This is "The Drawing the Host sees is live."

**Stage 3 — full guest parity + per-artist undo.** Guests send any
`Command` (create/edit/delete frames, cels, layers, retimes —
everything) to the host; host validates, sequences, applies,
rebroadcasts. Guest UI unlocks everything except Save/SaveAs/Open —
the host's PC stays the one save location (stanza). Undo as derived
above. Conflicting commands (two people delete the same frame) resolve
by host order; the loser's UI just sees the world move on.

**Stage 4 — personalization + layout presets.** Layout and forge
brushes never sync (they live in per-machine config; brush defs already
travel inline per stroke). New Settings subcategory **Layout ▸
Presets**: save/load named application-layout presets; defaults to the
user's own configured layout; while in a session the host's layout is
offered as one loadable preset. (Roadmap entry — built in this stage.)

**Testing spine.** The determinism suite grows a two-engine test: feed
one recorded point stream to two engines through the wire
encode/decode; assert tile-hash equality (the audit path doubles as the
assert). Loopback session tests extend to stroke frames and ordered
undo. The erase-race and divergence pins from the mirror era stay
green until their machinery is retired with Stage 1, then their
replacements pin the new contracts.

## 5. SKILLS THAT RAISE THE LIKELIHOOD (the owner's explicit ask)

Techniques (the craft):
1. **Deterministic input replication** — the fighting-game netcode
   family (GGPO lineage) and exactly how Drawpile runs multi-artist
   canvases in production. Our brush engine was accidentally built for
   it: determinism is already law and already tested.
2. **Single-sequencer total ordering** — one authority numbers every
   event; convergence becomes arithmetic, not luck.
3. **Audit-and-repair (checksum netcode)** — verify cheap hashes, repair
   narrow, log every repair. Turns the scariest unknown (GPU drift)
   into a bounded, measured cost.
4. **Local prediction with authoritative echo** — zero perceived
   latency for your own pen without forking the truth.
5. **Instrumentation-first debugging** — the wire log found the 90 MB
   line in one read after days of deduction. Stage 0 extends that eye
   to the guest's machine before anything else changes.

Process (the bound skills): premortem-stanza-dispatch gates the build
(this room); superpowers:brainstorming produced this design pass;
superpowers:writing-plans turns it into the staged implementation plan
after ratification; superpowers:test-driven-development drives the
determinism/ordering pins; superpowers:systematic-debugging + the
Stage-0 channel own every live-test failure; feature-dev:code-reviewer
audits each stage against the NEVER-DO list before it ships.

## 6. ROADMAP

- **Settings ▸ Layout ▸ Presets** (Stage 4): named application-layout
  presets; per-user default; host's layout loadable during a session.
- Internet relay / NAT traversal: a later room (this one is LAN +
  direct IP).
- Binary point packing is Stage 1; binary framing for remaining JSON
  messages: later, only if Stage 0 stats say it matters.

---

## 7. BUILD AMENDMENTS (2026-08-20, derived during the build)

1. **The replay unit is the DAB LIST, not the point list.** The dab
   builder proved pure over (points + latched params), and `Dab` is a
   48-byte Pod — a bit-exact wire cast. Shipping dabs collapses ALL
   brush-def replication (curves, jitter, scatter, spacing, density,
   smudge color — smudge is FOLDED IN, immune to ordering) down to:
   tip image + grain image (content-addressed, announced once per
   session) + mode + opacity. Bit-exactness got strictly stronger;
   points+def replay is retired as premortem risk.
2. **The one boundary exception:** a stroke that CREATES its cel rides
   the command lane whole (tiles included, once) — its fresh ids are
   the reason (see 4). Steady-state strokes never ship pixels.
3. **Stage 2 approximations (visual only, commit exact):** the live
   overlay pre-scales alpha per dab and previews tip masks as the
   procedural falloff; the sequenced commit replays exactly. Presence
   wet ghosts retired — the dab stream IS the live view.
4. **Id partition:** every guest allocates entity ids in its own 2^48
   `next_id` range (armed at snapshot swap from peer_id). Parallel
   creations cannot collide; the engine is untouched.
5. **One law, every hand:** while in a room the engine's BASE author is
   the local artist's name — every history entry carries its hand on
   every machine (pinned by
   `per_artist_undo_replays_identically_on_replicas`). Undo is a
   sequenced `Undone{author}` event replayed via `undo_last_by`
   everywhere — the HOST's undo included, routed through the same lane.
   Snapshot-on-undo is retired; snapshots serve joins + resyncs only.
   In-session undo reaches back to the session's start (solo entries
   are untagged — deliberately out of a room's reach).
6. **Guest predictions:** a guest's UI edits apply locally at once and
   travel whole to be sequenced; the origin skips its echo. A refused
   or conflicting prediction is healed by resync (LAN window ~ms; the
   seq lane detects divergence). Save/SaveAs/Open stay host-only.
7. **Known deferred:** host layout re-broadcast is join-only; internet
   relay, binary JSON frames, and stroke-level GC of very long sessions
   remain later rooms.

## 8. THE FIRST LIVE TEST + THE AUDIT (2026-08-20, same night)

Two-machine test (owner hosting, tester joined): strokes replayed ok
(6,443-dab strokes, hashes clean), but the owner reported LAG on long
fast strokes; the log showed resync-snapshot storms with no logged
cause, peers=0 while the socket lived, and the 47-agent adversarial
audit returned 38 confirmed findings (4 refuted). The premortem's
"repair fatter than the drift" came true in a shape nobody drew: the
STROKE wire was clean — the pixels leaked around it through the
COMMAND lane (fills, lassos, transforms, clears ship full before+after
texels as JSON — ~4MB per 20-tile fill per receiver).

**v0.2.1 (shipped that night):**
- Command lane: PaintTiles BEFORES stripped from the wire (receivers
  rebuild from their own identical documents — the inverse stays
  exact) + whole batches deflate. Fills/clears drop ~20–40×.
- The dab walk is INCREMENTAL — O(n) per stroke, was O(n²); pinned
  bit-identical by incremental_dabs_match_full_rebuild.
- guest_ready lifecycle: reset at every session boundary, set on the
  editor that SURVIVES the swap (different-size joins froze forever);
  stale-channel cleanup at lane reset; mid-stroke snapshots WAIT
  instead of being dropped; parse failures re-request.
- Replays + overlay run under the composite lens too (they starved).
- Replay/audit/repair resolve layers PROJECT-WIDE, never through the
  viewer's cut (a peer browsing another cut killed strokes).
- Audit covers coords only THIS machine changed (drift by
  construction); id high-water survives swaps (no re-minted ids).
- Aborted strokes + departing peers send sequenced no-op Ends —
  gathers and the remote overlay no longer leak/wedge.
- Refusals go to the refused artist only (a broadcast refusal made
  every guest resync); duplicate join names refused at the door.
- Host seq order matches host doc order (replay_done numbers BEFORE
  the frame's arrivals); NEVER-DO 4 repairs: every resync logs its
  cause, joins/leaves/undones logged, STATS carries socket-truth
  peers + queue depths.

**v0.2.2 (shipped, same night — the second live test's verdicts):**
The retest measured the truth the design had to face: the two GPUs
NEVER blend f16 identically — every guest stroke audits at 100% drift
(38/38 tiles), so audit-and-repair is the STEADY STATE, not the edge
case; and the stale-snapshot livelock was the big lag (three 26MB
snapshots at ~25–30s each on the tester's link, echoes 75s late,
repairs overwriting un-echoed ink = "it takes out what they drew").
- **Snapshots are requester-only** (joiners + resync requesters get
  the doc; the room never hears it), and the host HOLDS every
  seq-assigning path while one builds (replay numbering, command
  drains, guest cmds/undo requests re-queued) — a snapshot can no
  longer arrive stale; the livelock is structurally dead. Join-time
  resource/layout refreshes go to the joiner alone.
- **Repairs are binary + deflated** (SUB_REPAIR: header + one deflated
  texel blob; ~10–20× smaller than the JSON texels) — per-stroke
  convergence now costs ~tens of KB, wire-invisible on LAN.

**v0.2.3 (shipped — the owner's X-sheet ruling, verbatim: "the X-sheet
can be a problem source right now. If either user draws it must
populate for both. Otherwise the shared files will go corrupted
state"):** the unified sequenced queue landed — strokes, command
batches and undos apply in ONE host order through the canvas executor
on every guest (timeline edits can no longer overtake strokes; befores
rebuild at application time), and the mid-stroke race is closed: a cel
created under a live pen always broadcasts its AddDrawing/SetCell
(X-sheet entry) whole, with the streamed stroke aborted for peers.
Either artist drawing populates the X-sheet on both machines, always.

**The v0.2.4 queue (remaining, severity order):**
1. Redo re-audit (a repaired guest's redo can reintroduce drifted
   after-images unaudited).
3. Robust peer identity (echo-skip/undo keyed by display name; give
   peers wire ids).
4. Fill/lasso/transform as OPERATIONS (seed + params + audit) — the
   true dab-class fix; strip+deflate is the bridge.
5. Replay perf: batch same-layer replays under one readback, cache
   armed replay resources, bound the readback to the stroke's rect.
6. DESIGN FORK to weigh if repair traffic still bites: since cross-GPU
   drift is universal, the host could ship its committed texels
   (deflated) WITH the End for direct guest commit — trading the
   replay's bandwidth win for zero-audit convergence on mismatched
   GPUs, keeping dab replay for the live view only. The owner decides.
7. Overlay draws on the stroke's TARGET cel only; guest ungated
   pre-snapshot predictions; snapshot temp-file races; import assets
   deflated/deduped.

---
*PSD gate passed 2026-08-20 — root: bit-exact stroke replay, audited
cheaply, repaired narrowly. Stages 0–4 built same day; v0.2.0 shipped;
first live test + 47-agent audit same night; v0.2.1 lag/correctness
batch shipped on its heels.*
