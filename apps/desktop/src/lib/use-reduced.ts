"use client";

// The one reduced motion check for island UI. Wraps Motion's hook so every
// caller reads the same setting and treats "unknown" as "allow motion".

import { useReducedMotion } from "motion/react";

/** Collapses Motion's tri-state answer (null while unknown) to a boolean. */
export function resolveReduced(value: boolean | null | undefined): boolean {
  return value ?? false;
}

export function useReduced(): boolean {
  return resolveReduced(useReducedMotion());
}
