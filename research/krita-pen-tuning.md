# Krita-Derived Pen Input Tuning

**Date:** 2026-07-17
**Method:** multi-agent derivation (6 research agents on Krita source/docs + egui/winit issues, 78 findings, 12/14 claims adversarially verified against primary sources on invent.kde.org / github.com/KDE/krita).
**Problem:** a widening "blob" of ink at the end of every pen stroke on lift-off (XP-Pen Artist 22 Pro 2nd Gen, Windows Ink → egui Touch events).

## Root cause (ranked)

It is a **terminal-pressure bug, not a geometry-stacking bug.**

1. **PRIMARY** — the last committed point kept a high, non-zero pressure at lift, and a round anti-aliased end-cap at that width renders as a filled disc. Two mechanisms: (a) `force: None` packets before lift reused `last_pressure`, pinning the tail at the last hard-press value; (b) the End branch added no taper, on the false premise that "the taper already lives in the Move samples." **It doesn't** — egui/winit on Windows Ink never delivers the decreasing-pressure Move samples Krita relies on. winit maps pen pressure 0 → `None` (0 falls outside its `1..=1024` normalization arm), and the Move stream simply stops before pressure decays.
2. **SECONDARY** — EMA lag: `smoothed += (raw - smoothed) * 0.5` stays above the true decaying pressure, so a `.min()` endpoint clamp was a no-op and never reached ~0.
3. **TERTIARY** — the renderer floored every segment at `0.3 px` and drew independent round-capped line segments, so any residual endpoint width became a literal minimum-radius dot.
4. **NON-CAUSE** — the distance filter (`MIN_SAMPLE_DIST`) is correct and was a red herring.

## What Krita actually does (verified)

- **Default smoothing is "Basic," not Weighted** (`KisConfig::lineSmoothingType()` returns 1 = `SIMPLE_SMOOTHING`). Weighted smoothing is a distance-weighted Gaussian (σ = `effectiveSmoothnessDistance`/3, default distance 50 px), self-sizing (~3σ), pressure smoothing OFF by default.
- **Krita discards the tablet-release event**: `KisToolFreehand::endPrimaryAction` begins with `Q_UNUSED(event)`. It never paints from the lift event's pressure/position.
- Krita's clean tip on lift comes from (a) the **driver reporting decreasing pressure** through the final Move samples, and (b) the **Stabilizer's "Finish line"** flushing its delay buffer to the true cursor. **Our stack has neither** — so we must synthesize the taper.
- Krita's default **pressure curve is identity** (`"0,0;1,1;"`), so a pressure-0 tip yields a zero-size dab — no floor.
- **Scalable Distance is ON by default** → the smoothing/spacing dead-zone is a constant *physical* span (distance ÷ zoom). Our screen-space `MIN_SAMPLE_DIST` matches this.

## The fix implemented (app/src/canvas.rs)

7-step input pipeline:

1. **Source isolation** — read pressure only from `Event::Touch`; a 1-frame `mouse_lockout` after `Touch::End` suppresses egui's synthesized primary-drag so no phantom flat-pressure mouse point is appended.
2. **Start** — distrust Start force (usually None on Windows Ink); seed `START_SEED = 0.3`, marked `seed_pending`, overwritten by the first real Move. A pure tap keeps the seed → modest dot, not a blob.
3. **Move pressure** — `Some(>0)` real; `Some(0)` explicit taper; `None` reuses `last_pressure`.
4. **Spike rejection** — reject an upward jump > `SPIKE_DELTA = 0.35` when the pen moved < `MIN_SAMPLE_DIST` (stationary release/contact spike); don't poison `last_pressure`.
5. **Smoothing + distance gate** — median-of-3 on raw, then EMA (`PRESSURE_SMOOTH = 0.5`); bank sub-threshold samples onto the retained endpoint instead of stacking segments.
6. **End taper (THE FIX)** — ramp the last `END_TAPER_POINTS = 5` points' pressure to 0 via smoothstep, overwriting the pinned/lagged tail. No fabricated geometry beyond where the pen actually was.
7. **Render** — drop the `0.3` floor; `WIDTH_FLOOR = 0.1` interior only, so the zero-pressure tip vanishes cleanly.

## Parameters (portable constants)

| Const | Value | Why |
|---|---|---|
| `START_SEED` | 0.3 | provisional Start pressure; overwritten by first real Move |
| `MIN_SAMPLE_DIST` | 1.5 screen px | constant physical dead-zone (Krita Scalable Distance) |
| `PRESSURE_SMOOTH` | 0.5 | weak EMA; not trusted to reach 0 (End taper owns the tail) |
| `SPIKE_DELTA` | 0.35 | reject stationary upward pressure spikes |
| `END_TAPER_POINTS` | 5 | trailing points ramped to 0 at lift |
| `WIDTH_FLOOR` | 0.1 px | interior anti-invisibility floor; NOT applied to defeat the tip taper |

## Pitfalls (do not regress)

- Don't force terminal pressure to 0 in one step — ramp it (smoothstep) or the tip looks cut.
- Don't trust End/Start force on Windows — winit gives None at pressure 0, not a high spike; `unwrap_or(1.0)` would paint a full-width cap.
- Don't rely on the EMA reaching 0 by itself; the End ramp must *overwrite* the tail.
- Don't reinterpret every `None` Move packet as decay — some drivers emit None mid-stroke; that would thin the line. The End taper is driver-independent.
- Don't read `Response.dragged()`/`interact_pointer_pos()` for pen width — that consumes egui's duplicate pressure-less mouse stream (double points at flat 0.5).
- Don't keep the global `.max(0.3)` width floor — it re-creates the tip dot.
- **Wintab/legacy-mouse fallback:** if the driver routes the pen as legacy mouse (not WM_POINTER), egui gets no Touch/force and every stroke degrades to flat-0.5 with no taper. Future: detect "stroke saw zero valid `Some(f>0)` samples" and surface that pressure is unavailable rather than silently shipping full-width lines.
