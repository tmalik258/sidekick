// The seam between the island and a dock (Review or Git) beside it. It can be
// dragged, nudged with the arrow keys, and reset with a double click, and it
// never squeezes either side below its minimum.

export const ISLAND_MIN = 372;
export const DOCK_MIN = 600;
/** Below this window width the dock stacks under the island instead. */
export const STACK_BELOW = 1100;
export const NUDGE = 16;
export const NUDGE_BIG = 64;

/** The island's width for a dragged position, kept inside both minimums. */
export function clampIsland(want: number, total: number): number {
  const max = Math.max(ISLAND_MIN, total - DOCK_MIN);
  return Math.round(Math.min(max, Math.max(ISLAND_MIN, want)));
}

/** The width the island starts at, and returns to on a double click. */
export function defaultIsland(total: number): number {
  return clampIsland(Math.round(total * 0.4), total);
}

export function stacked(windowWidth: number): boolean {
  return windowWidth < STACK_BELOW;
}

/** Arrow keys nudge; Shift makes a bigger step; Home and End jump to a minimum. */
export function nudge(key: string, shift: boolean, island: number, total: number): number | null {
  const step = shift ? NUDGE_BIG : NUDGE;
  if (key === "ArrowLeft") return clampIsland(island - step, total);
  if (key === "ArrowRight") return clampIsland(island + step, total);
  if (key === "Home") return clampIsland(0, total);
  if (key === "End") return clampIsland(total, total);
  return null;
}

/** What a saved width becomes when the window is a different size now. */
export function restore(saved: number | null, total: number): number {
  return saved === null ? defaultIsland(total) : clampIsland(saved, total);
}
