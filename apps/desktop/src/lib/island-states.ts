// Every state the island can show, in one place: how big it is, which face
// and wash go with it, and what a screen reader hears. The island, the dev
// page and the announcer all read this, so a new state is added once.

import type { MascotState } from "./types";
import type { WashEvent } from "./wash";

export type IslandSize = "pill" | "compact" | "card" | "panel";

export interface IslandStateDef {
  id: string;
  label: string;
  size: IslandSize;
  /** The mascot state that goes with it, or null when it shows over any. */
  mascot: MascotState | null;
  wash: WashEvent;
  /** Spoken when the state appears; null stays silent. */
  announce: string | null;
}

export const ISLAND_STATES = [
  { id: "idle", label: "Idle", size: "pill", mascot: "idle", wash: "idle", announce: null },
  { id: "quiet-glance", label: "Quiet glance", size: "card", mascot: "idle", wash: "idle", announce: null },
  { id: "glance", label: "Glance", size: "card", mascot: "idle", wash: "idle", announce: null },
  { id: "noticing", label: "Noticing", size: "compact", mascot: "noticing", wash: "noticing", announce: null },
  {
    id: "listening",
    label: "Listening",
    size: "compact",
    mascot: "listening",
    wash: "listening",
    announce: "Listening",
  },
  { id: "working", label: "Working", size: "compact", mascot: "working", wash: "working", announce: "Working" },
  { id: "done", label: "Done", size: "compact", mascot: "success", wash: "success", announce: "Done" },
  { id: "error", label: "Error", size: "compact", mascot: "error", wash: "error", announce: "Something went wrong" },
  {
    id: "suggestion",
    label: "Suggestion",
    size: "card",
    mascot: "suggesting",
    wash: "suggesting",
    announce: "Suggestion",
  },
  {
    id: "suggest-peek",
    label: "Suggestion, compact",
    size: "compact",
    mascot: "suggesting",
    wash: "noticing",
    announce: "Suggestion",
  },
  {
    id: "waiting",
    label: "Waiting for you",
    size: "card",
    mascot: "suggesting",
    wash: "warning",
    announce: "Waiting for you",
  },
  { id: "new-message", label: "New message", size: "compact", mascot: null, wash: "agent", announce: "New message" },
  { id: "unread", label: "Unread", size: "pill", mascot: null, wash: "agent", announce: null },
  { id: "activities", label: "Activities", size: "card", mascot: null, wash: "working", announce: null },
  { id: "focus", label: "Focus", size: "compact", mascot: "focus", wash: "idle", announce: "Focus on" },
  { id: "paused", label: "Paused", size: "pill", mascot: "sleeping", wash: "idle", announce: "Paused" },
  { id: "offline", label: "Offline", size: "compact", mascot: null, wash: "warning", announce: "You are offline" },
  { id: "agent-card", label: "Agent", size: "card", mascot: "working", wash: "agent", announce: null },
  { id: "notify-card", label: "Notification", size: "card", mascot: "noticing", wash: "noticing", announce: null },
  { id: "net-card", label: "No connection", size: "card", mascot: null, wash: "warning", announce: "No connection" },
  { id: "update", label: "Update", size: "card", mascot: null, wash: "agent", announce: "Update ready" },
  {
    id: "merge-conflict",
    label: "Merge conflict",
    size: "card",
    mascot: "suggesting",
    wash: "warning",
    announce: "Merge conflict",
  },
  { id: "setup-guide", label: "Setup guide", size: "card", mascot: "noticing", wash: "noticing", announce: null },
  { id: "welcome", label: "Welcome", size: "card", mascot: "success", wash: "success", announce: null },
  {
    id: "ask",
    label: "Ask",
    size: "panel",
    mascot: "noticing",
    wash: "suggesting",
    announce: null,
  },
  {
    id: "agents",
    label: "Agents",
    size: "panel",
    mascot: "working",
    wash: "agent",
    announce: null,
  },
  {
    id: "settings",
    label: "Settings",
    size: "panel",
    mascot: "idle",
    wash: "idle",
    announce: null,
  },
] as const satisfies readonly IslandStateDef[];

export type IslandStateId = (typeof ISLAND_STATES)[number]["id"];

export function stateDef(id: IslandStateId): IslandStateDef {
  return ISLAND_STATES.find((s) => s.id === id) as IslandStateDef;
}

/** The registry entries that go with a mascot state, in registry order. */
export function statesFor(mascot: MascotState): IslandStateDef[] {
  return ISLAND_STATES.filter((s) => s.mascot === mascot);
}
