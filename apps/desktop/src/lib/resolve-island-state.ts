// Turns the live app's flags into one registry id so the shell, wash and
// announcer all agree (ADR-02).

import type { IslandStateId } from "./island-states";
import type { MascotState } from "./types";

export interface IslandContext {
  asking: boolean;
  /** Ask tab: ask | agents | repos | history, agents uses agents wash. */
  askTab: "ask" | "agents" | "repos" | "history" | "settings" | null;
  settingsView: boolean;
  welcome: boolean;
  guiding: boolean;
  expanded: boolean;
  bare: boolean;
  fullVoice: boolean;
  /** Compact voice / working / focus pill. */
  voicePill: boolean;
  suggestion: boolean;
  /** A suggestion shown small first; hover or Alt opens it. */
  suggestPeek?: boolean;
  waitingForYou: boolean;
  merging: boolean;
  updateCard: boolean;
  offline: boolean;
  netCard: boolean;
  /** Hovering a notification pill opens its card. */
  notifyCard?: boolean;
  /** Hovering the pill while an agent works opens that agent's card. */
  agentCard?: boolean;
  focus: boolean;
  paused: boolean;
  /** Fresh idle notification (pill), else unread count when unread > 0. */
  notifyFresh: boolean;
  unread: number;
  activitiesOpen: boolean;
  mascot: MascotState;
  quietGlance: boolean;
}

/**
 * Priority order mirrors the prototype: Ask and panels win, then attention
 * cards, then compact pills, then idle / bare.
 */
export function resolveIslandState(c: IslandContext): IslandStateId {
  if (c.welcome) return "welcome";
  if (c.asking && (c.settingsView || c.askTab === "settings")) return "settings";
  if (c.asking && c.askTab === "agents") return "agents";
  if (c.asking) return "ask";
  if (c.guiding) return "setup-guide";
  if (c.merging) return "merge-conflict";
  if (c.updateCard) return "update";
  if (c.waitingForYou) return "waiting";
  if (c.suggestion) return "suggestion";
  if (c.suggestPeek) return "suggest-peek";
  if (c.notifyCard) return "notify-card";
  if (c.agentCard) return "agent-card";
  if (c.netCard) return "net-card";
  if (c.activitiesOpen) return "activities";
  if (c.mascot === "error") return "error";
  if (c.mascot === "success") return "done";
  if (c.mascot === "listening" || c.fullVoice) return "listening";
  if (c.focus) return "focus";
  if (c.mascot === "working" || (c.voicePill && !c.focus)) return "working";
  if (c.mascot === "noticing") return "noticing";
  if (c.paused) return "paused";
  if (c.offline && !c.bare) return "offline";
  if (c.notifyFresh) return "new-message";
  if (c.unread > 0 && !c.expanded) return "unread";
  if (c.expanded && c.quietGlance) return "quiet-glance";
  if (c.expanded) return "glance";
  return "idle";
}
