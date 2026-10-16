// Palettes for the grain wash behind the island. Each event retints the wash
// and sends a ring out; the base states hold their tint until the next one.

import type { MascotState } from "./types";

export const WASH_EVENTS = [
  "idle",
  "noticing",
  "suggesting",
  "listening",
  "working",
  "agent",
  "success",
  "warning",
  "error",
] as const;

export type WashEvent = (typeof WASH_EVENTS)[number];

export interface WashPalette {
  a: string;
  b: string;
  c: string;
  /** How lively the wash is, 0 to 1. */
  energy: number;
  /** Ring strength sent out when the palette is entered, 0 for none. */
  ring: number;
  /** Events fall back to the base state after this long; 0 holds. */
  settleMs: number;
}

export const PALETTES: Record<WashEvent, WashPalette> = {
  idle: { a: "#7a8f7e", b: "#3a4a3e", c: "#f2f0eb", energy: 0.22, ring: 0, settleMs: 0 },
  noticing: { a: "#95a89a", b: "#6f7d70", c: "#f2f0eb", energy: 0.42, ring: 0.3, settleMs: 0 },
  suggesting: { a: "#95a89a", b: "#b7ad98", c: "#f2f0eb", energy: 0.55, ring: 0.4, settleMs: 0 },
  listening: { a: "#a8c0cc", b: "#6f8f9a", c: "#e6eef0", energy: 0.72, ring: 0.5, settleMs: 0 },
  working: { a: "#7a8f7e", b: "#b7ad98", c: "#f2f0eb", energy: 0.6, ring: 0.3, settleMs: 0 },
  agent: { a: "#a8c0cc", b: "#7a8f7e", c: "#f2f0eb", energy: 0.55, ring: 0.45, settleMs: 1800 },
  success: { a: "#9cc79a", b: "#7a8f7e", c: "#f2f0eb", energy: 1, ring: 1, settleMs: 1800 },
  warning: { a: "#d9a55b", b: "#8f7a5a", c: "#f2f0eb", energy: 0.75, ring: 0.65, settleMs: 1800 },
  error: { a: "#de8676", b: "#8a5a50", c: "#f2f0eb", energy: 0.9, ring: 0.85, settleMs: 2400 },
};

/** The base wash for each mascot state. */
export function washFor(state: MascotState): WashEvent {
  switch (state) {
    case "noticing":
    case "suggesting":
    case "listening":
    case "working":
      return state;
    case "success":
      return "success";
    case "error":
      return "error";
    default:
      return "idle";
  }
}

/** "#rrggbb" as three channels from 0 to 1. */
export function hexToRgb(hex: string): [number, number, number] {
  const n = (i: number) => Number.parseInt(hex.slice(i, i + 2), 16) / 255;
  return [n(1), n(3), n(5)];
}

/** The nine color channels (a, b, c) a palette feeds the shader. */
export function channels(p: WashPalette): number[] {
  return [...hexToRgb(p.a), ...hexToRgb(p.b), ...hexToRgb(p.c)];
}
