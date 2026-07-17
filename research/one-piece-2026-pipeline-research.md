# How a One Piece Episode Gets Made (2025–2026) — Verified Pipeline Research

**Purpose:** Ground-truth reference for deriving the feature list of a modern 2D animation studio app.
**Method:** Deep-research run (2026-07-15): 5 search angles, 18 sources fetched, 85 claims extracted, 25 adversarially verified — 20 confirmed, 5 refuted and excluded. Confidence labels below reflect that verification.

---

## The Big Picture

A 2025–26 One Piece episode at Toei Animation moves through a **fully digitalized, heavily distributed pipeline**. Cut data travels over optical cable between Tokyo's Oizumi Studio and Toei Animation Philippines (TAP) in Manila, and over dedicated lines to **21 domestic subcontractor studios**. No physical materials move — everything is digital files flowing between stages. TAP handles roughly **70% of Toei's total workload** (in-betweening, digital ink & paint, backgrounds, special effects, 3DCG, and some key animation). *(High confidence — Toei's own corporate pages, though the 70% is self-reported as of 2021.)*

The stage divisions have been stable since the cel era; what changed is that every stage is now a software tool with digital handoffs:

```
Script → Storyboard (ekonte) → Layout → Key animation (genga) → Supervisor check (sakkan)
→ In-betweening (douga) → Ink & Paint (shiage) → [+ Backgrounds] [+ 3DCG]
→ Photography/Compositing (satsuei) → Editing → Sound → Broadcast
```

The single most important structural fact for an app design: **the exposure sheet (timesheet) is the master artifact**. It governs timing across genga, douga, paint, and photography, and it travels with the cut. Drawings and timing are separate objects.

---

## Stage-by-Stage Findings

### 1. Drawing/timing stack: the RETAS lineage → CLIP STUDIO PAINT era

**Verified (high):** Toei's documented digital backbone was CELSYS's **RETAS suite**, whose modules map one-to-one onto the pipeline stages:

| Module | Stage | What the artist does in it |
|---|---|---|
| **Stylos HD** | genga/douga drawing | Tablet drawing, vector/raster, multi-layer editing, onion skinning |
| **TraceMan HD** | scan/cleanup | 48-bit scanning, vector-tracing paper pencil lines into resolution-independent line art |
| **PaintMan HD** | ink & paint | Digital cel coloring on Stylos/TraceMan output |
| **CoreRETAS HD** | shooting/compositing | Scene setup, exposure-sheet-driven rendering, export to video |

RETAS was the dominant 2D suite in the Japanese industry with Toei specifically named among its users. CELSYS has designated **CLIP STUDIO PAINT** as its successor (RETAS sales ended ~2015; exact current support status was contested in verification — treat "fully discontinued" with caution).

**Verified (high) — the modern signal:** Toei developed its own **"Toei Animation Digital Exposure Sheet"** — timesheet-editing software that follows the paper timesheet format, built and demonstration-tested jointly with CELSYS, **links timing data into CLIP STUDIO PAINT EX's timeline**, is freely distributed for commercial use, and uses the **XDTS file format** (which OpenToonz also supports). Toei participates in ACTF (the industry's digital-production forum), where the current-era toolset is CSP 3.0–5.0 + Tabmate, the Digital Exposure Sheet, OpenToonz, and TVPaint.

**Takeaway:** Toei's current drawing/timing direction is CLIP STUDIO PAINT EX + XDTS digital exposure sheets. XDTS is an open, documented interchange format — a real interop opportunity for a new app.

### 2. Key animation workflow (best-documented proxy: Dragon Ball Super era)

**Verified (medium — documented 2015–2019, workflow shape persists but software is migrating):**
- Hand-drawn key animation is scanned and **vector-traced** so pencil lines become resolution-independent data.
- **Shadow markup is isolated onto its own layer automatically** during trace (animators draw shadow boundaries in colored pencil; the software separates them).
- The animation supervisor (sakkan) corrects frames **digitally on a correction layer** — a non-destructive overlay on top of the original artist's drawing.
- The software **builds the cut's timeline by inserting blank in-between frames according to the timing sheet** — timing exists before the in-betweens do.
- Some contracted studios already draw genga fully digitally and skip scanning entirely.
- Effects animation on One Piece (e.g. ep. 1074 lightning) is at least partly digital brush-based rather than pencil. *(Blog-tier evidence, consistent with the above.)*

### 3. In-betweening (douga) and the outsourcing machine

**Verified (high):** TAP in Manila is a wholly-owned, fully integrated subsidiary — connected by optical cable, doing in-between animation on 938+ One Piece episodes, digital coloring on 947+, background art on 944+, key animation on 210+ (per credit databases, with 2024+ credits proving currency). TAP was the first Philippine studio to fully digitize in-betweening (2000) and backgrounds (2004) *(self-reported)*.

**Verified (medium) — the "gross" outsourcing model:** When a full episode is subcontracted, the outside studio works from the core team's storyboard and supplies its own key animators, supervisors, in-betweeners, painters, episode director, and production desk — **but background art and compositing stay with the main studio's regular crew** to keep the look consistent. This defines which stages travel together as a unit and which stay centralized.

**Refuted — do not use:** the "60% outsourced" figure (0-3 refuted; the verified figure is ~70% and it's not classified as outsourcing since TAP is in-house).

### 4. Ink & Paint / coloring

**Verified (high) — premise correction:** **There is no evidence Toei uses OpenToonz.** The official OpenToonz user list (Ghibli, Ponoc, TRIGGER, Studio Chizu, MADHOUSE/DLE, Kamikaze Douga, etc.) never mentions Toei. OpenToonz derives from *Toonz Ghibli Edition* and its 2016 open-sourcing was **Dwango-led** — the common claim that "Toei funded OpenToonz" is false.

Toei's coloring lineage is RETAS **PaintMan**, with the CSP-era migration path above. As a *functional model* of a Toonz-family paint stage (not Toei practice): on Ghibli's *The Boy and the Heron* (2023), drawings were scanned with GTS and color-designed + painted in OpenToonz.

Functional essence of the paint stage either way: named **color models per character** (palettes with normal/shadow/highlight variants), gap-closing fill on traced line art, and separation of trace lines vs. shadow lines vs. color regions.

### 5. Backgrounds

**Evidence gap.** No software claim survived verification (Photoshop is the industry default assumption, but treat as unverified). What is verified: backgrounds are a distinct department, TAP paints them at scale digitally, and they stay centralized even on gross-outsourced episodes.

### 6. 3DCG

**Verified (high):** Toei primarily uses **Autodesk Maya** (named in Toei's own current job postings), with **Houdini and Nuke** also in recruitment materials. CG needs (ships, crowds, effects) are **identified at the approved-layout stage** and routed to digital artists alongside 2D digital-effects work (fire/water motion, textures). Even Toei's Unreal Engine experiments keep Maya as the asset backbone.

### 7. Photography / compositing (satsuei)

**Evidence gap on Toei specifically.** Industry-standard satsuei is **Adobe After Effects** — the photography department composites colored cels + backgrounds + 3DCG per cut and applies filters, lighting, diffusion, and camera work (only blog-tier sourcing survived for this; Nuke appears in Toei's 3DCG job postings). Functionally the stage consumes: painted cel layers, BG art, CG renders, and the exposure sheet's camera instructions.

### 8. Editing, sound, finishing

**Evidence gap.** No verified claims — standard broadcast post workflow assumed, nothing Toei-specific confirmed.

### 9. Production management

**Partially verified:** The *structure* is confirmed — cut data flows digitally between Oizumi, TAP, and 21 domestic studios; the exposure sheet is the cut-level management artifact; gross episodes have their own production desks. The *software* (asset tracking / cut status systems, e.g. ShotGrid or in-house) is an **evidence gap**.

---

## Modernization (dated, verified)

1. **AI plans (high confidence):** Toei's May 2025 financial report (p.20) formally targeted **four stages for future AI**: storyboarding (AI-generated simple layouts), coloring (AI color spec + correction), **in-betweening (AI line correction + in-between generation — aimed at the most labor-intensive outsourced stage)**, and backgrounds (AI from photos), tied to its Preferred Networks investment. After public backlash Toei clarified it is **not currently using AI** in production; the slide was quietly dropped from the next report. Planned Preferred Computing Infrastructure use from early 2026.
2. **Schedule revolution (high confidence):** Announced Oct 28, 2025 — One Piece abandons 26 years of just-in-time weekly production. From **January 2026**: two-cour annual format, **~26 episodes/year**, with a Jan–Mar hiatus building a production buffer. Confirmed executed: Elbaph arc Cour 1 launched April 5, 2026. Verified rationale: matching the manga's pacing (a claimed "capacity bottleneck" rationale was refuted 0-3).

---

## Refuted claims — never cite these

- ~~Toei adopted RETAS in 1993 / was first to digitalize then~~ (0-3; conflicting with documented Feb 1997 first digital TV production)
- ~~Toei outsources 60% to the Philippines~~ (0-3)
- ~~Quality/capacity bottleneck drove the 2026 schedule change~~ (0-3)
- ~~GTS preserves halftone info in its format~~ (0-3)
- ~~Toei funded OpenToonz's open-sourcing~~ (premise error; Dwango-led)

## Open questions (unresolvable from public sources)

1. What Toei *actually* runs in 2025–26 for drawing and ink & paint on One Piece — CSP EX at production scale, legacy PaintMan, or undisclosed in-house — and what file formats beyond XDTS move between stages.
2. The satsuei software and the production-management/cut-tracking system running the weekly flow.
3. Concrete staffing/workflow changes behind the post-ep-1000 Egghead-era visual upgrade.
4. Whether any announced AI tooling has actually deployed since May 2025.

---

## What This Means for Your App — Derived Feature Candidates

Each verified pipeline fact maps to a candidate capability. This is the raw material for your ability list:

| # | Pipeline truth | Candidate feature |
|---|---|---|
| 1 | Exposure sheet is the master timing artifact, separate from drawings | **X-sheet as a first-class object**: drawings are a library; frames reference drawings; retiming never duplicates art |
| 2 | XDTS is an open interchange format (Toei/CELSYS/OpenToonz all speak it) | **XDTS import/export** → instant interop with the real anime industry |
| 3 | Timeline is built from the timesheet with blank douga slots | **Timing-first workflow**: block in timing before in-betweens exist; empty frame slots as visible work queue |
| 4 | Sakkan corrections live on non-destructive overlay layers | **Correction-layer system**: reviewer draws on top, original preserved, diff-toggle view |
| 5 | Shadow markup auto-separates from main lines at trace time | **Multi-plane line art**: trace lines / shadow lines / color-boundary lines as distinct channels per drawing |
| 6 | Paper scan → vector trace is still a real on-ramp | **Scan + vector-trace import** (resolution-independent cleanup of pencil lines) |
| 7 | Ink & paint uses named per-character color models | **Color models as first-class assets**: character palettes (normal/shadow/highlight sets) that propagate edits across every frame using them |
| 8 | Work is organized by *cuts*, not one long timeline | **Cut-based project structure**: scenes contain cuts; each cut owns its drawings, x-sheet, and status |
| 9 | Cuts flow between studios/people with defined stage handoffs | **Cut status tracking / pipeline board**: layout → genga → check → douga → paint → composite, with assignments |
| 10 | Gross outsourcing keeps BG + composite centralized | **Role-scoped project sharing**: export a cut bundle containing only the stages a collaborator owns |
| 11 | CG is identified at layout and composited later | **Reference-layer imports** (image sequences / video / 3D renders) that sit in the layer stack for animating against |
| 12 | Satsuei composites cels + BG + CG with camera work and filters | **Compositing stage built in**: camera (pan/zoom/rotate/multiplane), blend modes, diffusion/glow/blur, per-cut render |
| 13 | Toei's own AI targets: in-between generation, line correction, auto-color, BG-from-photo | **AI-assist roadmap** aligned with what the top studio itself wants — inbetweening assist first (biggest labor sink) |
| 14 | Everything is digital handoffs over a network | **Clean file format + export discipline**: cut bundles, image sequences, and open formats over monolithic blobs |
| 15 | The 2026 schedule change = industry moving from just-in-time to buffered production | **Planning views** (per-cut progress vs. deadline) matter even for solo/small-team users |

## Sources (18 fetched; key ones)

- Toei corporate — production network, TAP, optical-cable pipeline: corp.toei-anim.co.jp
- Toei — One Piece 2026 schedule announcement: toei-animation.com/one-piece-2026-production-schedule
- CELSYS — ACTF / Digital Exposure Sheet / RETAS history: celsys.com (primary)
- Kanzenshuu — stage-by-stage Toei TV workflow (DBS era): kanzenshuu.com/animation-production/process
- OpenToonz official use-case page (primary; disproves Toei link): opentoonz.github.io
- Sakuga Blog — outsourcing structure, satsuei glossary: blog.sakugabooru.com
- Toei May 2025 financial report p.20 (AI slide, since retired) + AniTrendz/ANN/Animation Magazine coverage
- Wikipedia — RETAS, Toonz, PH animation outsourcing (secondary corroboration)
