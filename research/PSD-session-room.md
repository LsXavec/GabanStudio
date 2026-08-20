# THE SESSION ROOM — live-hosted file + presence (its own room)

PSD gate passed 2026-08-18 — invoked BY the owner with the order. His
words (PURPOSE, verbatim): "Make a new integration that hosts the File
online and is live editable hostable through a generated API key using
a 2FA application like authy. So basically you gotta have the api key
and the 2FA number from the hosts 2FA as well to be able to join the
room. Once your in the File room. It will allow you to see the other
players cursor and live drawing. The Username should be configurable in
the Settings along with setting the API key for connection. Then for
the connect have a button in there that opens a window to put in the
2FA authy authentication code."

PREMORTEM. It is two months on. The session feature damaged the studio.
The story: two artists drew for an hour on documents that had silently
diverged — the guest's strokes landed on frames the host had retimed,
and a naive merge destroyed both versions. Separately, the 2FA was
decorative: codes were replayable, so a captured handshake re-joined
the room a day later and scribbled over a cut. And a mid-stroke
disconnect left half a stroke committed to the document.

ROOT: this stands on ONE authoritative document — the host's — with
every remote effect entering through the same guarded paths as a local
hand, and on auth that actually gates (key + fresh TOTP, never
replayable); if either is weak, this is rubble.

NEVER-DO.
1. Never two truths. Guests render the HOST's document; any doubt means
   a full snapshot resync from the host — never a merge.
2. Never a remote effect outside existing guarded paths. v1 remote
   drawing is PRESENCE (ghost strokes, vanish at pen-up); guest ink
   entering the document is v2 and must ride the engine's own command
   path with the same refusals (hidden layer, line guard, gesture).
3. Never replayable auth: a verified TOTP code is burned for its
   window; the API key never appears in logs, status lines, or wire
   plaintext (challenge-response only). Disconnect mid-anything
   discards, never commits.
4. Scope guard: v1 = host/join + auth + presence (cursors, names, live
   wet strokes both ways) + host-document live view (join snapshot +
   change-driven refresh). Guest ink, retiming, layers, undo stay
   host-only; a guest's refused verb answers through the refusal lane.

BLAST RADIUS: new `app/src/net.rs`; a Session page + 2FA connect window
in the settings; canvas presence overlay + small presence getters; App
plumbing in main.rs; config gains a serde-defaulted SessionConfig.
`crates/anim-core` UNTOUCHED. Deps added: sha1, hmac, rand (base32
hand-rolled with tests). Direct-IP / LAN first; port-forwarding is the
host's business and is documented; no cloud relay inside this room.

STAGES: v1 as above. v2 (guest ink via the command path), named, needs
its own pass under this room's laws.

V2 PURPOSE recorded 2026-08-18, owner's words verbatim, mid-build:
"The other user will have the full build of animstudio on their device.
The connection should allow them to select and do basically everything
the Host can. There edits are just stored independantly but the layers
are persistant and you can be on the same layer. Individual Undo redo
archetype. So you can draw together. Draw on different workflow Panels
ETC. And Canvas will be persistant accross their views."
Ruling: full-peer editing with per-artist undo over shared persistent
layers requires AUTHOR-TAGGED HISTORY in the engine (anim-core) — a
command-stream replica with per-author undo policy. That is outside
this room's blast radius and every prior room's: it needs its OWN gate
with an engine-format premortem before any code. v1 (this build) is its
foundation: rooms, burning auth, presence both ways, host-doc mirror.

## Build log

- 2026-08-18 — V1 DELIVERED (handover StartTime 16:04:03; 20 tests,
  0 warnings). `net.rs`: hand-rolled base32 + RFC-6238 TOTP (verified
  against the RFC's own vectors), challenge-response auth (HMAC-SHA1
  over nonce‖code — the key NEVER crosses the wire), verified codes
  BURNED per window, length-prefixed framing (JSON msgs + raw doc
  snapshots), threaded host with per-client relay, guest client with an
  event channel. Settings → Session: username, room key, Generate
  (key + TOTP secret for Authy manual entry), host controls with the
  LIVE CODE + window countdown readout and the artist list; connect is
  its own window where the 2FA code is entered and nowhere else.
  Canvas: peer cursors (Ao ring + name + pen dot), live wet ghosts for
  same-frame peers, and off-frame peers named at the paper's top edge.
  Host mirrors the document change-driven via `snapshot_bytes()` — the
  SAME `engine.save` path, never a second serializer; guests apply it
  wholesale (never merged), refusing while a gesture is live, and the
  mirror carries NO file path so a guest can never save over the host.
  `AppState.doc_gen` added as the app-side generation stamp (engine
  history is private and stays so).
  WINDOWS NOTE for future work: a TcpStream accepted from a
  non-blocking listener INHERITS non-blocking on Windows — the
  handshake read returned WouldBlock and dropped the connection until
  `set_nonblocking(false)` was called on the accepted stream. Cost one
  red test; the loopback test now guards it.

OWNER AMENDMENT 2026-08-19, verbatim: "lets remove the authy for now
its confusing." The KEY is the gate: the challenge-response HMAC stays
(key never on the wire, nonce kills replays) but the TOTP code no
longer validates — dormant, not deleted (verify_totp, the enrollment
UI, and the 2FA connect window remain in the tree behind allow/false
gates). Wire shape UNCHANGED: the mac covers whatever code string the
guest sends, so a v0.1.3 guest typing any code still joins a new host.
New guests join with address + key only; the Connect button joins
directly. Loopback test amended: wrong KEY refuses; codeless joins.

SESSION PERF repair 2026-08-19 (owner: both machines "pausing and
unfreezing" rhythmically in-session). Three compounding causes fixed:
(1) HOST: every pen-up's doc_gen bump triggered a FULL document save on
the UI thread next frame — snapshots now require changed + pen-up +
2.5s settle (fresh joins still force one). (2) GUEST: every snapshot
rebuilt the whole Editor incl. GPU paint targets — the "buffering";
same-paper-size snapshots now swap state IN PLACE keeping GPU targets,
playhead, zoom. (3) presence throttled to 20Hz (was per-frame wet
clones + JSON both directions). Known remaining cost, named: the
host's snapshot save itself still runs on the UI thread once per
settle window — an async save is the future fix if it still shows.

SESSION PERF 2 repair 2026-08-19 (owner mid-test: "lagging diagnose";
measured >1 core with a live guest at 192.168.0.192): the 2.5s sync
metronome itself was the lag — the host's full-document SQLite save and
the guest's full parse both ran on the UI thread. Both moved to worker
threads: the host clones the Arc-tiled project (cheap) and a worker
runs store::save; the guest parses off-thread and the UI only swaps the
finished AppState (drain runs every frame; mid-stroke drops it and the
cadence brings the next). snapshot_bytes superseded and removed.
Shipped as v0.1.6.

SESSION PERF 3 repair 2026-08-19 (owner: "still freezing ... on my pc
and the users"): the true root — EVERY outbound socket write ran
blocking on the UI thread. The host froze pushing the multi-MB snapshot
into TCP (broadcast_except wrote inline); the guest froze pushing
stroke tiles (whose JSON encodes each u16 texel as ~5 ASCII bytes —
megabytes per stroke — serialized inline too). Symmetric freezes,
exactly as reported. Fix: one WRITER THREAD per connection; the UI
enqueues (Out::Raw / Out::Json); EditTiles' expensive serialization
happens on the writer; a stalled peer stalls only its own queue.
Shipped as v0.1.7. NAMED NEXT if wire size ever hurts: EditTiles as a
binary frame kind instead of JSON numbers (~5x smaller).

OWNER AMENDMENT 2026-08-19, verbatim: "Make the application show a
network session on the same network that way we dont need the key and
can just easily join for the testing phase. push to the live ap."
LAN DISCOVERY: a hosting app broadcasts a UDP beacon (name, port,
version, open flag) every 2s on 41101; every app listens and the
Session page lists rooms on this network with one-click JOIN. Open
rooms (the default while the testing phase lasts) skip the key check
entirely — a LAN-trust trade the owner chose, recorded here; the
"require key" latch restores the gate per room, and remote/keyed
joining is unchanged. Wire compatible: open hosts simply skip the mac
verification, so older guests join with whatever key they hold.

THE 90MB LINE 2026-08-19 (first catch by the wire log, one line:
"OUT kind=1 bytes=89960448"): the join snapshot was 90MB of
uncompressed f16 raster — a minute over wifi. Everything the owner
reported cascaded from it: "very delayed" (the transfer), "file
drawings not showing" (mirror still in flight), "lift your pen it
removes" (the guest drew against the BLANK mirror; their lift replaced
the host's inked tiles with blank-merged ones), "still lags" (the pipe
stayed saturated). Fixes: FRAME_DOC deflates on the snapshot worker and
inflates on the reader (Compression::fast; raw+deflated sizes logged);
and guests REFUSE strokes until the first snapshot/commands arrive
("the host's file is still arriving") — a blank mirror can never again
eat the host's ink. v0.1.15; both ends must match (deflated wire).
