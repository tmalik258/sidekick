// The faces Duskie can make, and which one each state of the state machine
// wears. The drawing lives in creature.ts.

import type { MascotState } from "@/lib/types";

export const EXPRESSIONS = [
  "hm",
  "idle",
  "suggest",
  "curious",
  "listen",
  "think",
  "speak",
  "happy",
  "delight",
  "hello",
  "celebrate",
  "love",
  "wink",
  "proud",
  "surprised",
  "shy",
  "focus",
  "worried",
  "sad",
  "oops",
  "tired",
  "sleepy",
  "sleep",
  "offline",
] as const;

export type Expression = (typeof EXPRESSIONS)[number];

/** The face for each state of the state machine. */
export const FOR_STATE: Record<MascotState, Expression> = {
  idle: "idle",
  sleeping: "sleep",
  noticing: "curious",
  // Prototype FACE: suggestion / notice → curious (big eyes), not suggest arms-out.
  suggesting: "curious",
  listening: "listen",
  working: "think",
  success: "happy",
  error: "oops",
  focus: "focus",
};
