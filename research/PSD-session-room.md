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
