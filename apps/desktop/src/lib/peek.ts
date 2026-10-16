// Peek: a small pill or compact card opens into the full card on hover or
// while Alt is held, folds back a moment after, and stays open once clicked.
// Keyboard users get the same through focus. Pure so it can be tested.

export const PEEK_OPEN_MS = 220;
export const PEEK_CLOSE_MS = 450;
/** Idle notification pill stays "fresh" this long, then becomes a count. */
export const PILL_FRESH_MS = 8000;

export interface PeekInput {
  hover: boolean;
  alt: boolean;
  focus: boolean;
  /** Clicked open: stays until dismissed. */
  pinned: boolean;
}

/** Whether the card wants to be open right now, before delays. */
export function wantsOpen(i: PeekInput): boolean {
  return i.pinned || i.hover || i.alt || i.focus;
}

/**
 * How long to wait before the open state changes to `want`, or null when it
 * already matches. Opening waits for intent, closing waits a beat so a
 * stray pointer does not flicker the card.
 */
export function peekDelay(open: boolean, want: boolean, reduced = false): number | null {
  if (open === want) return null;
  if (reduced) return 0;
  return want ? PEEK_OPEN_MS : PEEK_CLOSE_MS;
}

/** At most this many bubbles show; the rest fold into "+N". */
export const MAX_BUBBLES = 3;

export interface Bubbles<T> {
  shown: T[];
  more: number;
}

export function bubblesOf<T>(items: T[], max = MAX_BUBBLES): Bubbles<T> {
  return { shown: items.slice(0, max), more: Math.max(0, items.length - max) };
}

export type PillKind = "none" | "pill" | "count";

/**
 * An idle island shows a new notification as a pill, then as an unread count
 * once the pill has had its moment. A busy island uses bubbles instead.
 */
export function pillKind(opts: { unread: number; fresh: boolean; busy: boolean }): PillKind {
  if (opts.busy || opts.unread <= 0) return "none";
  return opts.fresh ? "pill" : "count";
}
