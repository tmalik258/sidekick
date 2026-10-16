import { ISLAND_TOP, PANEL_PAD } from "@/lib/islandSize";
import { SPRINGS, spring } from "@/lib/spring";
import type { MascotState } from "@/lib/types";

export const TITLE: Record<MascotState, string> = {
  idle: "Duskie",
  sleeping: "Resting",
  noticing: "Something changed",
  suggesting: "Suggestion",
  listening: "Listening",
  working: "Checking activity status",
  success: "Done",
  error: "That did not work",
  focus: "Focusing",
};

export const DETAIL: Record<MascotState, string> = {
  idle: "Nothing needs you right now.",
  sleeping: "Sensors are paused.",
  noticing: "Taking a look.",
  suggesting: "Here is an idea.",
  listening: "Go ahead.",
  working: "Waiting for an activity report.",
  success: "All set.",
  error: "Details are in the log.",
  focus: "Held for later.",
};

/** States that hold the island open without hover. Working and plain Done stay a compact pill. */
export const OPEN_STATES: ReadonlySet<MascotState> = new Set(["suggesting", "listening", "error"]);

export const ORB = 44;
export const COMPACT = {
  width: 39,
  busyWidth: 109,
  /** Orb + the offline mark. */
  offlineWidth: 66,
  waitWidth: 248,
  /** Orb + "Listening..." + bars; grows with transcript up to voiceWidth. */
  voiceMinWidth: 156,
  voiceWidth: 320,
  /** Working / download status: grow with the label, truncate only past this. */
  statusMaxWidth: 520,
  height: 36,
  radius: 18,
  orb: 26,
};
export const EXPANDED = { width: 388, minHeight: 78, radius: 30, pad: PANEL_PAD };
/** Ask mode, as in the design: 420 wide, 580 for the Agents tab. */
export const ASK_WIDTH = 420;
export const AGENTS_WIDTH = 580;
/** The board: two tiles across, three on a 1440p screen and up. */
export const BOARD_WIDTH = 680;
export const BOARD_WIDE_WIDTH = 940;
/** Voice, Full listening style. */
export const FULL_VOICE_WIDTH = 400;
export const ASK_RADIUS = 26;
/** The welcome: a short read, so a narrower column than Ask. */
export const WELCOME_WIDTH = 440;
/** A setup guide: room for its steps and copy buttons. */
export const GUIDE_WIDTH = 452;
export const TOP = ISLAND_TOP;

/** Delay before hover expands, so a cursor passing over the top edge does not trigger it. */
export const HOVER_IN_MS = 140;
/** Longer in-delay when the compact pill shows dismiss / recheck (easy to brush). */
export const HOVER_IN_ACTIONS_MS = 450;
/** Grace period before collapsing after the cursor leaves. */
export const HOVER_OUT_MS = 320;

/** Opening settles rather than wobbles: overshoot belongs after a fling,
 * not a hover. Stiffness form — Motion rejects some bounce/duration springs. */
export const morphOpen = spring(SPRINGS.morph);
export const morphClose = spring({ response: 0.42, damping: 0.9 });
/** Growing or shrinking while already open (an answer streaming in, New):
 * no bounce, so the panel never overshoots and settles back. */
export const resize = spring(SPRINGS.settle);
/** Ctrl+Space is a keyboard action used many times a day: Ask opens in one
 * quick, calm move, with no bounce and no blur. */
export const askOpen = { duration: 0.2, ease: [0.23, 1, 0.32, 1] } as const;
/** Agents is a richer surface: a short Dynamic Island spring, still snappy. */
export const agentsOpen = spring({ response: 0.42, damping: 0.86 });
