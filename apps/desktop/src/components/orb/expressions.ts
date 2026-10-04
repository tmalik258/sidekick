// The mascot's faces. Eyes are the mascot and always show; the mouth and
// effects are reactions that come and go; blush is mood and stays for the
// whole expression. Each expression rests in a still pose reached on
// springs, with at most one small nudge on entry (never a loop).

import type { MascotState } from "@/lib/types";

/** Eye shapes, drawn in a 20 x 30 box. */
export const EYES = {
  pill: '<rect x="5" y="3" width="10" height="24" rx="5"/>',
  wide: '<rect x="3.5" y="1" width="13" height="28" rx="6.5"/><circle class="orb-glint" cx="12.4" cy="7.2" r="2.2"/>',
  big: '<rect x="4" y="2" width="12" height="26" rx="6"/>',
  small: '<rect x="6" y="9" width="8" height="15" rx="4"/>',
  half: '<rect x="5" y="14" width="10" height="11" rx="5"/>',
  focus: '<rect x="4" y="11" width="12" height="9" rx="4.5"/>',
  closed: '<path class="orb-line" d="M4 16 Q10 19 16 16" stroke-width="3"/>',
  happy: '<path class="orb-line" d="M4 18.5 Q10 8.5 16 18.5" stroke-width="3.4"/>',
  dot: '<circle cx="10" cy="16" r="3.8"/>',
  squintL: '<path class="orb-line" d="M5.5 10 L14.5 15 L5.5 20" stroke-width="3.2"/>',
  squintR: '<path class="orb-line" d="M14.5 10 L5.5 15 L14.5 20" stroke-width="3.2"/>',
  worriedL: '<path d="M5 12 L15 8.5 L15 23 a5 5 0 0 1 -10 0 Z"/>',
  worriedR: '<path d="M15 12 L5 8.5 L5 23 a5 5 0 0 0 10 0 Z"/>',
} as const;
export type EyeShape = keyof typeof EYES;

/** Mouth shapes, drawn in a 40 x 20 box. */
export const MOUTHS = {
  smile: '<path class="orb-line" d="M12 7 Q20 14 28 7" stroke-width="3.2"/>',
  grin: '<path d="M10 5 Q20 7 30 5 Q28 16 20 16 Q12 16 10 5Z"/>',
  o: '<ellipse cx="20" cy="10" rx="4" ry="5"/>',
  small: '<ellipse cx="20" cy="10" rx="3" ry="2.4"/>',
  flat: '<path class="orb-line" d="M15 9 L25 9" stroke-width="3"/>',
  frown: '<path class="orb-line" d="M13 12 Q20 6 27 12" stroke-width="3"/>',
  cat: '<path class="orb-line" d="M12 7 Q16 12 20 7 Q24 12 28 7" stroke-width="2.8"/>',
} as const;
export type MouthShape = keyof typeof MOUTHS;

/** One physical nudge when an expression starts. */
export type Accent = "hop" | "hop2" | "jump" | "nod" | "lift" | "sink" | "shiver" | "hello" | "wink";
/** The only loops: slow and tiny. */
export type Life = "breathe" | "breatheSlow" | "pulse" | "sway" | "still";
export type Effect = "glints" | "heart" | "zzz" | "zzzSoft" | "dots" | "offline";

export interface Look {
  /** One shape for both eyes, or [left, right]. */
  eyes: EyeShape | [EyeShape, EyeShape];
  mouth?: MouthShape;
  blush?: boolean;
  /** Halo colors; the theme's own when omitted. */
  halo?: [string, string, string];
  haloOpacity?: number;
  /** Faint tint mixed into the body highlight. */
  tint?: string;
  /** Halo rotation speed relative to idle; 0 stops it. */
  spin?: number;
  dim?: boolean;
  /** Where the eyes look (-1..1); the cursor otherwise. */
  gaze?: [number, number];
  /** Resting pose: y and scale in units of the orb's size, tilt in degrees. */
  pose?: { y?: number; tilt?: number; scale?: number };
  accent?: Accent;
  life?: Life;
  effect?: Effect;
}

const AI: [string, string, string] = ["#ff7a45", "#e14bff", "#4d8bff"];

export const EXPRESSIONS = {
  idle: { eyes: "pill", life: "breathe" },
  suggest: {
    eyes: "pill",
    halo: ["#0a84ff", "#64d2ff", "#5e5ce6"],
    haloOpacity: 0.7,
    tint: "#0a84ff",
    spin: 1.4,
    accent: "lift",
    life: "breathe",
  },
  happy: {
    eyes: "happy",
    mouth: "smile",
    blush: true,
    halo: ["#30d158", "#a6f4b5", "#64d2ff"],
    haloOpacity: 0.65,
    tint: "#30d158",
    accent: "hop",
    life: "breathe",
  },
  delight: {
    eyes: "happy",
    mouth: "grin",
    blush: true,
    halo: ["#ffd60a", "#ff9f0a", "#ff375f"],
    haloOpacity: 0.75,
    spin: 2.2,
    accent: "hop2",
    effect: "glints",
    life: "breathe",
  },
  love: {
    eyes: "happy",
    mouth: "cat",
    blush: true,
    halo: ["#ff375f", "#ff8ab3", "#ffc8dd"],
    haloOpacity: 0.7,
    pose: { tilt: -5 },
    accent: "nod",
    effect: "heart",
    life: "breathe",
  },
  wink: { eyes: "pill", mouth: "smile", pose: { tilt: -6 }, accent: "wink", life: "breathe" },
  curious: {
    eyes: ["big", "pill"],
    halo: ["#ff9f0a", "#ffd60a", "#ff9f0a"],
    haloOpacity: 0.65,
    tint: "#ff9f0a",
    spin: 1.8,
    pose: { tilt: 8 },
    accent: "lift",
    life: "breathe",
  },
  think: { eyes: "pill", gaze: [-0.45, -0.65], halo: AI, haloOpacity: 0.8, spin: 4, effect: "dots", life: "sway" },
  listen: { eyes: "wide", halo: AI, haloOpacity: 0.9, spin: 3, pose: { scale: 1.03 }, life: "pulse" },
  surprised: {
    eyes: "wide",
    mouth: "o",
    halo: ["#ff9f0a", "#ffd60a", "#ff453a"],
    haloOpacity: 0.8,
    spin: 2.5,
    pose: { y: -0.015, scale: 1.03 },
    accent: "jump",
    life: "breathe",
  },
  shy: {
    eyes: "dot",
    gaze: [0.35, 0.7],
    mouth: "small",
    blush: true,
    halo: ["#ffc8dd", "#ff8ab3", "#ffc8dd"],
    haloOpacity: 0.55,
    pose: { tilt: 7, scale: 0.95, y: 0.015 },
    accent: "sink",
    life: "breathe",
  },
  proud: {
    eyes: "happy",
    mouth: "cat",
    gaze: [0, -0.4],
    halo: ["#ffd60a", "#ffffff", "#ffd60a"],
    haloOpacity: 0.55,
    pose: { y: -0.02, tilt: -3 },
    accent: "lift",
    effect: "glints",
    life: "breathe",
  },
  focus: { eyes: "focus", halo: ["#0a84ff", "#5e5ce6", "#0a84ff"], haloOpacity: 0.45, spin: 0.5, life: "still" },
  worried: {
    eyes: ["worriedL", "worriedR"],
    mouth: "flat",
    halo: ["#ff9f0a", "#ff453a", "#ff9f0a"],
    haloOpacity: 0.55,
    pose: { scale: 0.97 },
    accent: "shiver",
    life: "breathe",
  },
  sad: {
    eyes: ["worriedL", "worriedR"],
    gaze: [0, 0.6],
    mouth: "frown",
    halo: ["#5e5ce6", "#64d2ff", "#5e5ce6"],
    haloOpacity: 0.4,
    spin: 0.4,
    pose: { y: 0.03, scale: 0.95 },
    accent: "sink",
    life: "breathe",
  },
  oops: {
    eyes: ["squintL", "squintR"],
    mouth: "flat",
    halo: ["#ff453a", "#ff9f0a", "#ff375f"],
    haloOpacity: 0.6,
    tint: "#ff453a",
    pose: { scale: 0.97 },
    accent: "shiver",
    life: "breathe",
  },
  sleepy: {
    eyes: "half",
    mouth: "small",
    haloOpacity: 0.2,
    spin: 0.4,
    pose: { y: 0.015, tilt: 4 },
    effect: "zzzSoft",
    life: "breatheSlow",
  },
  sleep: {
    eyes: "closed",
    haloOpacity: 0,
    spin: 0,
    dim: true,
    pose: { y: 0.02, scale: 0.97 },
    effect: "zzz",
    life: "breatheSlow",
  },
  offline: {
    eyes: "small",
    gaze: [0, 0.25],
    mouth: "flat",
    halo: ["#8e8e93", "#636366", "#8e8e93"],
    haloOpacity: 0.28,
    spin: 0.3,
    dim: true,
    pose: { y: 0.025, scale: 0.96 },
    accent: "sink",
    effect: "offline",
    life: "breathe",
  },
  hello: { eyes: "happy", mouth: "smile", blush: true, pose: { tilt: -4 }, accent: "hello", life: "breathe" },
  celebrate: {
    eyes: "happy",
    mouth: "grin",
    blush: true,
    halo: ["#30d158", "#ffd60a", "#ff375f"],
    haloOpacity: 0.85,
    spin: 3,
    accent: "hop2",
    effect: "glints",
    life: "breathe",
  },
} satisfies Record<string, Look>;

export type Expression = keyof typeof EXPRESSIONS;

/** The face for each state of the state machine. */
export const FOR_STATE: Record<MascotState, Expression> = {
  idle: "idle",
  sleeping: "sleep",
  noticing: "curious",
  suggesting: "suggest",
  listening: "listen",
  working: "think",
  success: "happy",
  error: "oops",
};
