# LASSO FILL — loop a region, fill it, feather its edge

PSD gate 2026-08-19. Owner's order, verbatim: "Make the tool that is
like the lasso fill tool. and have it have parameters to edit like the
softness of the outside edge etc"

PREMORTEM. A month on, the tool damaged the sheet. The story: each
lasso committed a dozen little PaintTiles, so one undo peeled the fill
apart in flakes. The feather was computed with a per-pixel × per-edge
loop that froze the pen for a second on any generous loop. It painted
onto hidden and guarded layers because it had its own guard code that
missed two of the five refusals. And "softness" measured OUTWARD, so
every feathered anime flat bled over its line art.

ROOT: this stands on one loop = ONE commit through the SAME guarded
door every region edit uses (commit_region_edit → one undo step), and
on the mask math being bounded (scanline rasterize + chamfer distance,
O(area), clipped to the paper). If either is weak, this is rubble.

NEVER-DO.
1. One commit, one undo. commit_region_edit only; never a stream of
   patches, never a private engine path.
2. The fill tool's guard set applies VERBATIM: composite view refuses,
   hidden layer says so, held/blank frame says so, unresolved layer
   refuses. No new guard code paths — the same checks in the same
   order.
3. Softness feathers INWARD from the loop's edge (signed distance;
   grow shifts the edge ±px). A feathered flat never bleeds outward
   past where the artist drew the loop.
4. Bounded math: scanline polygon rasterization + two-pass chamfer —
   no per-pixel-per-edge loops; the working grid is the loop's bbox
   clipped to the paper.

BLAST RADIUS: canvas.rs only (CanvasTool::LassoFill, deck button,
OPTIONS row fields, input handler, preview loop, mask+diff builder).
anim-core untouched (commit_region_edit already exists). Escape
cancels a half-drawn loop; switching tools clears it.

## Build log

- 2026-08-19 — DELIVERED (41 tests, 0 warnings). CanvasTool::LassoFill:
  deck button (Icon::Lasso) after fill; OPTIONS row gains SOFTNESS
  (0–64px) and GROW (−16..+16px) fields; Ao dashed closed-loop preview;
  Esc cancels; tool switches clear a half loop. Input mirrors the fill
  tool's guards in the fill tool's order (composite refuse, hidden
  layer, held/blank frame, unresolved layer — NEVER-DO 2), minus the
  GPU requirement: this path is pure CPU. Commit: scanline even-odd
  rasterization over the loop's paper-clipped bbox, two-pass 3-4
  chamfer signed distance, alpha ramped INWARD over softness with grow
  shifting the edge (NEVER-DO 3, pinned by test: feather never bleeds
  outward, centre stays solid), src-over in premultiplied f16 onto the
  active layer's tiles, ONE commit_region_edit = one undo step
  (NEVER-DO 1). One placement stumble during the build (the OPTIONS
  row and the input dispatch share a match pattern; a script aborted
  mid-edit and the retry worked on stale text) — caught by the
  compiler, both arms verified by line number before shipping.
