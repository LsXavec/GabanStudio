# THE SWEETENING PASS — the room

PSD gate passed 2026-08-18 — RATIFIED by the owner: "ratified, batch one".
Root: polish is strictly corrective, and muscle memory is sacred.

Owner's order 2026-08-18: "Lets continue on application sweetening and
making it work good" / "follow premortem stanza and walk through the
edits". Claude drafts the failure story; the owner authors or ratifies
the line that binds.

## PREMORTEM (candidate)

It is a month out. The sweetening pass damaged the studio. The story of
how: nothing dramatic broke — it eroded. A hundred small "while I'm in
here" edits went in behind one audit, each defensible alone; together
they moved controls the hand had already memorised, renamed verbs
mid-habit, and quietly changed two guarded behaviours that looked
cosmetic from the outside. A defect list written by machines became a
work order no human had ratified, and polish became redesign. When the
owner sat down to draw, the instrument he had learned over two days no
longer answered the same way, and he could not name what had changed
because the log said only "polish".

ROOT (candidate): this stands on polish being STRICTLY corrective —
every edit traceable to a stated law and a named defect, never to a
machine's taste — and on muscle memory being sacred: nothing an artist
has already learned moves, renames, or changes meaning without the
owner's explicit word. If either is weak, this is rubble.

## STANZA (candidate)

PURPOSE. Close the gap between what the ratified laws say and what the
running app actually does — and nothing else.

ROOT. Polish is strictly corrective, and muscle memory is sacred; if
either is weak, this is rubble.

NEVER-DO.
1. Never an edit without a citation: each change names the law it
   restores and the defect it closes. "It would look better" is not a
   defect; an unratified improvement is a proposal, not a fix.
2. Never move, rename, or re-key anything the owner already uses —
   pencil slots, the four fold-outs, the room tabs, the transport, the
   keys — without his explicit word for that specific item.
3. Never touch document, stroke path, session auth, or the guarded
   paths. Refusals stay refusals; DANGER holds stay held. A "cosmetic"
   edit that changes behaviour is not cosmetic.

BLAST RADIUS. app-crate UI only, in the files the audit names. No
anim-core, no net.rs protocol, no config schema changes beyond
serde-defaulted additions.

STAGES. One delivery per batch, each logged with its citations, each
looked at by the owner in the running app before the next.

## DISPATCH (already run, read-only)

The four-surface audit (workflow wf_fa95c784-40e) carried the editor
room's laws verbatim and was READ-ONLY by construction. Its findings
are verified against the real files by a final agent before any edit,
and ranked by what the owner notices in daily drawing use.

## Build log

- 2026-08-18 — BATCH ONE delivered (7 fixes, each cited to a law + the
  audit item that found it; 20 tests, 0 warnings):
  [1] config.rs — "Reset to defaults" on the Shortcuts page called
      `*self = Self::default()`: it wiped every brush preset, layer
      colour, perf setting, both UI toggles AND the session room key +
      TOTP secret, then saved to disk. One unguarded click. Now
      `reset_keybinds()` (shortcuts only) behind a held DANGER labelled
      RESET SHORTCUTS. LAW: affordances — destruction is guarded and
      names what it destroys.
  [2] doc.rs — a failed SAVE (and failed OPEN) wrote grey chatter that
      faded in 4s, identical in weight to "saved". Now refuse(): Aka
      lane + canvas edge flash, and the save wording says the work is
      NOT on disk. LAW: the Warning Law.
  [3] main.rs — the Slate's filename + vitals were variable-width
      labels BEFORE the room tabs, so the tabs slid sideways the moment
      the dirty mark appeared — every session, on the first stroke.
      Both are now fixed slots with elision; the dirty mark is a
      painted Tally dot in reserved space. LAW: stability.
  [4] plate.rs + xsheet_panel.rs — `selection.stroke = TALLY` is what
      egui paints a selected widget's TEXT with, so six panels rendered
      amber labels: column tabs, cut menu, library, interp letters,
      node pickers, settings sidebar. Selection is now a Tally GROUND
      under a Struck label; the column tabs became real DETENTS.
      LAW: the colour law — Tally is a lamp, never text.
  [5] plate.rs + xsheet_panel.rs — the cel-layer opacity slider had no
      visible track (egui paints the rail in widgets.inactive.bg_fill =
      GRAPHITE, bg_stroke NONE under our Visuals). Added `plate::rail`,
      the plate's fourth and final control kind: Well track, Legend
      edge, Tally fill, Struck handle; readout fixed to Mono 11 Struck
      (the old .monospace().small() resolved to 9.5 PROPORTIONAL).
      LAW: affordances + type.
  [6] main.rs — `guard_gesture()` (every cut/room/save transition) said
      the same sentence as the keyboard guard but through chatter. Now
      refuses. LAW: the colour law.
  [7] canvas.rs — the PEN path refused composite view in chatter while
      the mouse path already refused properly. Now both refuse.
      LAW: the Warning Law.
  NOT TOUCHED, by the room's law: nothing moved, renamed or re-keyed.
- 2026-08-18 — INCIDENT: batch two's first attempt applied a regex across
  every file in app/src to collapse space runs. Python's \s includes
  newlines, so it deleted line breaks in all 23 files and pulled code up
  into `//` comments. Two automated repair attempts made it worse (the
  second folded ~2,000 legitimate code lines into comments). Recovery:
  a corpus of known-good lines rebuilt from the session transcript
  (Writes, Edit before/after text, file reads) plus git HEAD, cut off
  BEFORE the damage so broken text could not poison it; each damaged
  line decomposed back into its corpus lines; rustfmt for indentation;
  ~15 orphaned comment fragments fixed by hand. Verified: 0 errors,
  0 warnings (an unused-variable warning would betray any statement
  still trapped in a comment), 20 app tests + all anim-core suites
  green, owner confirmed the running app. Committed as 6005424 — the
  two days of work had been sitting uncommitted, which turned a mistake
  into a crisis.
  STANDING RULES ADDED (owner's room, NEVER-DO 1 restated with teeth):
  never run a pattern edit across multiple files; edit named sites only.
  Commit before any sweep. When a repair is not converging, stop and say
  so rather than iterating.
- 2026-08-18 — BATCH TWO redone, named sites only (0 errors, 0 warnings,
  20 tests):
  [10] TEN string literals that spanned lines without a `\` continuation
       — each baked a newline + the next line's indent into the text the
       artist reads. Found with a stateful detector (string-state carried
       across lines, raw strings and char literals excluded), then each
       of the ten reassembled explicitly. Zero remain.
  [8]  the New Project screen claimed "everything here can be changed
       later except the paper size" — false in the opposite direction:
       nothing on that form can be changed later. Now: "Both are fixed
       once the cut is created."
  [16] a failed Open on the startup screen did nothing at all — the same
       dialog reappeared silently. NewProjectForm gained `error`, the
       reason is carried back from the Open arm, and it prints in Aka
       under the buttons. Cancelling clears it.
  [31] the name field's hint could never show (Default pre-filled
       "Untitled"). Now empty — with a fallback at Create so an untyped
       name still makes a named cut, not a blank one.
  [43] the on-twos hint used integer division: 25fps read "12 on twos"
       (12.5 is right) and 1fps read "1 frames". Now floating-point,
       singular at 1, and the clause is suppressed below 12fps.
  [30] the dpi caption implied the value did something; it is stored and
       read by nothing today, and now says so.
  [32] the swap-dimensions button wore Icon::Fit, the fit-view mark.
