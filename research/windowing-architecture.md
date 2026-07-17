# Windowing / Workspace Architecture — "Lens-Dock"

**Date:** 2026-07-17
**Method:** multi-agent derivation — 4 research areas (39 findings on egui docking/viewports, pro-app workspace models, panel linking/grouping, our code), 3 competing architectures generated, 9 independent judges, synthesis.
**Task:** "derive the project root, split the workflow into windows; each UI element positionable, linkable as groups."

## Recommended architecture: LENS-DOCK

An **egui_dock docking shell** where every UI element is a first-class, serializable **Panel** carrying a **Lens** over ONE shared Document; the node graph is a real panel bound to the actual `anim-core` `Cut.graph`; linking is **colored per-axis channels** resolved once per frame; second-monitor / pen-display tear-out uses **deferred viewports**.

It's a **docking-first** shell (buy `egui_dock` for spatial layout) with the node-graph vision honored (graph is a panel, not a bolt-on) and only two things hand-built because no crate provides them: the AppState→Document+ViewContext split, and OS-window tear-out.

### Why this base (from the judge panel)
- On the weighted axes — egui-feasibility and solo-effort — a docking-first shell (feasibility ~8/10) crushes the pure node-graph-of-panels candidate ("Patchbay", ~3.7/10), which pays a multi-month custom-UI tax to hand-roll what `egui_dock` gives free and bets everything on a per-subtree interactive-zoom capability egui hasn't demonstrated.
- Between the two docking candidates, the graph-as-real-panel one wins because the architecture law is literally "the document IS a node graph" — it makes the NodeGraph an undoable panel bound to the real `Cut.graph`, and persists layout via an opaque blob in our existing transactional SQLite, honoring the headless-engine law.
- The one grafted correction: **deferred (not immediate) viewports** for OS windows — the pen display and main monitor run at different refresh rates, and deferred repaints each independently.

## The models

**Panel model.** Every UI element becomes an addressable, serializable `Panel { id: PanelId, kind: PanelKind, lens: Lens, view: PanelView }`. Layout tree = `egui_dock`'s `DockState<PanelId>` (stores ids only → cheap to serialize). `PanelKind` (Canvas | Xsheet | Library | NodeGraph | Inspector | Timeline…) is runtime-switchable per pane via right-click (Blender's editor-type dropdown as a property, not a fixed slot). One `TabViewer` dispatches through a `HashMap<PanelId, Panel>` into `trait PanelContent { fn title; fn ui(&mut self, ui, cx: &mut PanelCx); }`. Because egui renders tabs sequentially in immediate mode, a single `&mut Document` threaded through the TabViewer is safe — no `Rc<RefCell>` for the document.

- `CanvasView` (already a self-contained struct owning zoom/pan/brush + the pen state machine) drops in as `PanelView::Canvas` almost verbatim; multiple canvas instances each get independent zoom/pen state for free.
- `xsheet_panel::ui` (free fn) becomes `struct XsheetView` — the smallest new-code item.
- Top bar + status bar stay as fixed chrome outside the dock; the `DockArea` occupies the CentralPanel.

**Workspace model.** A **fixed ordered stage spine** (DaVinci Resolve pages) with Blender-style per-stage customization. The eight stages — Storyboard → Layout → Genga → Sakkan → Douga → Paint → Composite → Edit — are the switchable workspaces. Users can't invent stages but freely rearrange/split/tab within one and Save-as. A workspace = layout + tool/mode + selection (switching to Genga restores the pencil, Sakkan the red pencil, Paint the fill tool…). Each stage ships an opinionated default layout. **The re-lensing invariant is absolute:** switching stage only swaps the layout preset and re-points each panel's Lens — it never copies/forks/converts document data.

**Linking & grouping — two orthogonal keys:**
- **(A) Spatial group** = position in `egui_dock`'s split tree; a split subtree or tab group moves/resizes/undocks as one unit (free from egui_dock).
- **(B) Logical link group** = a colored `ChannelId` on each panel's Lens deciding which panels share cut/frame/column/selection/view-range. Per-axis toggles: `Lens { cut: Follow(ch)|Pin, frame: Follow(ch)|Pin, … }` + optional pairwise gang-with-offset. Resolved once per frame on the app shell (collect intents → single-writer per channel → panels read) to kill oscillation/lag. Default: everything on channel "Main" = today's single cursor, so linking is behavior-preserving.
- **"Collapse into a group that moves as one"** is scoped to the NodeGraph panel's interior over the real `Cut.graph` (stable NodeId + exact-inverse commands already exist) via later `AddGroup/Ungroup` commands — not a window-manager feature.

**Multi-window — two tiers.** In-app floating (egui_dock tab tear-out) is free. True OS windows on the second monitor / XP-Pen use **deferred** egui viewports (`show_viewport_deferred` + `ViewportBuilder::with_position`). The pen-latency objection doesn't bite: `commit_stroke` accumulates the wet stroke locally and only issues a command at pen-up, so the in-progress line paints at the pen window's full rate; only the one-time dry commit crosses the shared boundary. Own the detach/re-dock state machine yourself; the main thread keeps `Engine` ownership (single-writer undo), detached windows render from an `Arc<RwLock>` snapshot and push `Vec<Command>` intents back through `Engine.apply`.

**Persistence — two separated stores.** Document = existing SQLite `.animproj` (unchanged, owned by anim-core). Layout = new app-owned serde/RON with three tiers: compiled-in defaults (always-available fallback) + per-user overrides in `%APPDATA%/AnimStudio` + optional per-project layout embedded via an opaque blob in the engine's `meta` table (engine never interprets the bytes → no UI types leak into anim-core). Every saved layout is schema-versioned; a deserialize failure falls back to the stage default rather than crashing.

**Integration (the mostly-invisible refactor):** split today's fused `AppState` (Engine + cursor) into `Document { engine, file_path }` and a linkable `ViewContext { scene, cut, active_column, frame, selected_drawing, onion, playing }`. Every mutation still routes through `Engine.apply(label, Vec<Command>)`, so undo coverage and the post-undo `sanitize()` dangling-id guard are preserved. **anim-core is not touched at all** — every window/dock/lens/link/tool type lives in the app crate only. Do the split behind a single shared "Main" ViewContext first, so behavior is byte-identical and the diff is auditable.

## Phased implementation

| Phase | Deliverable | Effort |
|---|---|---|
| **0 — Dependency + shell swap** | Add egui_dock (serde); pin egui/eframe/egui_dock to one minor. Replace fixed left+central panels with one `DockArea` hosting the SAME canvas + xsheet as tabs/splits, still driven by today's AppState. Ships: canvas+xsheet draggable/tabbable/undockable; layout round-trips to RON. | S |
| **1 — Panel trait + registry** | `PanelId/Panel/PanelKind/PanelContent/PanelCx`. CanvasView → `PanelView::Canvas`; xsheet fn → `XsheetView`. Context-menu change-kind. Ships: two canvas tabs with independent zoom/pan/pen. | M |
| **2 — Document + ViewContext split + link channels** | Extract cursor into ViewContext; add LinkContexts + per-panel Lens (per-axis Follow/Pin) + two-phase per-frame resolve + link-color chip UI. Default "Main" = today exactly. Ships: two canvases show different cuts/frames; a ganged pair scrubs together. (Highest-churn — done behind "Main" so it's non-regressing.) | L |
| **3 — Workspaces (stage spine)** | Workspace struct + built-in default layouts for all 8 stages + top-bar switcher + per-stage tool restore + Save-as + schema-versioned persistence. Ships: Genga↔Composite re-lenses the same document with zero data change. | M |
| **4 — Node graph as a panel** | Promote the spike's node editor (pan/zoom/wire-drag/bezier) into `PanelKind::NodeGraph` bound to the REAL Cut.graph via AddNode/Connect/… commands (inherits undo + eval-invalidation). Discard the spike's Vec-index model. Ships: wiring DrawingSource→…→Output updates a Viewer, undoable. | M |
| **5 — Multi-window OS tear-out** | DetachedWindow + detach/re-dock over deferred viewports; each OS window hosts its own mini DockState. Document behind `Arc<RwLock>` + Vec<Command> intent channel. VALIDATE on the real XP-Pen 60Hz + high-refresh rig (Win11 mixed-DPI bugs). Ships: canvas floats onto the pen display as a real OS window with unified undo. | L |
| **6 — Node-subnetwork groups (gated, engine-touching)** | anim-core `NodeKind::Group` + AddGroup/Ungroup with exact inverses. Deferred behind demonstrated need. | L |

## Key risks
- **Version lockstep:** egui/eframe/egui_dock must share the same egui minor; Phase 0 exists to surface this first.
- **Phase 2 is the highest-churn refactor** (touches every cursor read) — a mistake can let edits escape undo. Do it behind "Main".
- **Multi-monitor mixed-DPI is genuinely buggy in egui on Win11** (blurry text, wrong placement, issue #4918) on exactly the XP-Pen + main-monitor rig — Phase 5 must be validated on real hardware.
- egui_dock is binary-tree splits only; arbitrary grids would mean migrating to egui_tiles (losing tab close-buttons + in-app floating) — a conscious bet.

## Open questions for the user
1. **Per-project vs per-user layouts** — project remembers its own (SQLite blob), purely per-user (Nuke), or both with precedence?
2. **Are the 8 stages your real pipeline, and is a fixed non-extensible spine OK** (Resolve model) vs. user-addable custom stages (Blender)? Hard to reverse.
3. **Which panels genuinely need REAL OS windows** on the 2nd monitor/pen display vs. in-app floating? If only the canvas, Phase 5 shrinks to one special case.
4. **Visible-cable linking vs. colored-channel + pin?** Channels cover ~90% cheaply; visible wires are more faithful to the node-graph vision but add a second interaction surface. Recommendation: channels first.
5. **Confirm the exact target rig** (monitor resolutions/refresh/DPI + XP-Pen model) so Phase 5 mixed-DPI validation runs on real hardware from day one.
