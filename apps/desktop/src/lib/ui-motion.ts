// Shared Motion presets for island UI. Ease matches --ease-out-strong in globals.css.

/** Strong ease-out used for UI entrances. */
export const EASE_OUT_STRONG = [0.23, 1, 0.32, 1] as const;

/** Event wash: fade/scale in, hold briefly via timed unmount, exit same path. */
export const WASH_MS = 1400;

export const washTransition = {
  duration: 0.35,
  ease: EASE_OUT_STRONG,
} as const;

export const washReduced = {
  duration: 0.2,
  ease: EASE_OUT_STRONG,
} as const;

/** Stagger for Ask list enter, under 250ms total for a short list. */
export const staggerParent = {
  hidden: {},
  show: {
    transition: { staggerChildren: 0.04, delayChildren: 0.02 },
  },
} as const;

export const staggerChild = {
  hidden: { opacity: 0, transform: "translateY(8px)" },
  show: {
    opacity: 1,
    transform: "translateY(0px)",
    transition: { duration: 0.2, ease: EASE_OUT_STRONG },
  },
} as const;

export const staggerChildReduced = {
  hidden: { opacity: 0 },
  show: {
    opacity: 1,
    transition: { duration: 0.15, ease: EASE_OUT_STRONG },
  },
} as const;
