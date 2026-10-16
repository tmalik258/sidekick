// Layout and motion numbers from the Duskie Island Redesign prototype.
// Used only when redesign.enabled is on (ADR-01: values, not a JS port).

/** Prototype island orb base (`--os`). */
export const REDESIGN_OS = 48;

/** Idle / min pill: bare 36×36, radius 18. */
export const REDESIGN_IDLE = { width: 36, height: 36, radius: 18 } as const;

/** Compact content row height (prototype polish: 38 / r19). */
export const REDESIGN_COMPACT = {
  height: 38,
  radius: 19,
  padLeft: 48,
} as const;

/** Glance card (~336) and suggestion (~372). */
export const REDESIGN_GLANCE = { width: 336, radius: 26 } as const;
export const REDESIGN_CARD = { width: 372, radius: 26 } as const;
export const REDESIGN_NOTIFY = { width: 340, radius: 26 } as const;

/** Ask / Agents / Settings in-island sizes from the prototype. */
export const REDESIGN_ASK = { width: 452, radius: 26 } as const;
export const REDESIGN_AGENTS = { width: 560, radius: 26 } as const;
export const REDESIGN_SETTINGS = { width: 660, height: 460, nav: 176, radius: 26 } as const;

/** Hover intent: wait longer than legacy so a pass-over does not open glance. */
export const REDESIGN_HOVER_IN_MS = 380;
export const REDESIGN_HOVER_OUT_MS = 260;

/** Idle notification pill stays fresh this long, then becomes an unread count. */
export const REDESIGN_PILL_FRESH_MS = 8000;

/**
 * Take A (perched): Duskie is larger than the capsule, head above the top
 * edge. Final prototype values after the scale pass.
 */
export const TAKE_A = { x: 3, y: -4, scale: 0.875 } as const;
/** Take A on a tall card. */
export const TAKE_A_CARD = { x: 10, y: 10, scale: 1 } as const;
/** Take A in Ask / Settings (smaller, inset). */
export const TAKE_A_ASK = { x: 13, y: 12, scale: 0.625 } as const;

/** Prototype shell grow / settle springs (for motionFor / SPRINGS). */
export const GROW = { response: 0.46, damping: 0.78 } as const;
export const SETTLE = { response: 0.38, damping: 0.97 } as const;

/** Creature pixel size for a Take scale against the --os base. */
export function takeSize(scale: number, os = REDESIGN_OS): number {
  return Math.round(os * scale);
}
