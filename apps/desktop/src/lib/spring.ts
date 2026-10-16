// Springs described the way designers tune them: how long the motion takes
// to settle (response, in seconds) and how much it bounces (damping, 1 for
// none). Mapped to the stiffness and damping the motion library uses.

export interface SpringSpec {
  /** Seconds for one undamped swing; smaller is snappier. */
  response: number;
  /** 1 settles without overshoot; lower values bounce. */
  damping: number;
}

export interface SpringTransition {
  type: "spring";
  stiffness: number;
  damping: number;
  mass: 1;
}

export interface Instant {
  duration: 0;
}

/** Stiffness and damping for a unit mass with this response and damping ratio. */
export function spring({ response, damping }: SpringSpec): SpringTransition {
  const r = Math.max(0.01, response);
  const omega = (2 * Math.PI) / r;
  return {
    type: "spring",
    stiffness: round(omega * omega),
    damping: round(2 * Math.max(0, damping) * omega),
    mass: 1,
  };
}

/** The spring, or no animation at all when the user asked for less motion. */
export function motionFor(spec: SpringSpec, reduced: boolean): SpringTransition | Instant {
  return reduced ? { duration: 0 } : spring(spec);
}

/** The island's shared springs. */
export const SPRINGS = {
  /** Size and shape changes of the island (legacy path). */
  morph: { response: 0.42, damping: 0.86 },
  /** Prototype GROW: opening into a card or panel. */
  grow: { response: 0.46, damping: 0.78 },
  /** Prototype SETTLE: size changes while already open. */
  settle: { response: 0.38, damping: 0.97 },
  /** Small things appearing: chips, rows, badges. */
  pop: { response: 0.3, damping: 0.72 },
  /** Panels and docks sliding in. */
  panel: { response: 0.5, damping: 0.92 },
} as const satisfies Record<string, SpringSpec>;

function round(n: number): number {
  return Math.round(n * 100) / 100;
}
