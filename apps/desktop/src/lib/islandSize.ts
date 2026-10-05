// The island's fixed geometry, shared by the shell and the panels inside it.

/** Gap between the top of the screen and the island. */
export const ISLAND_TOP = 6;
/** Padding inside the expanded island, on every side. */
export const PANEL_PAD = 16;
/**
 * The tallest a panel (welcome, Settings) may be: the island window's height
 * less the gap above and the shell's padding. A panel capped at this always
 * fits, so the window never clips the shell.
 */
export const PANEL_MAX_HEIGHT = `calc(100vh - ${ISLAND_TOP + PANEL_PAD * 2}px)`;
