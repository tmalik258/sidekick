import { describe, expect, it } from "vitest";
import { type IslandContext, resolveIslandState } from "./resolve-island-state";

const base: IslandContext = {
  asking: false,
  askTab: null,
  settingsView: false,
  welcome: false,
  guiding: false,
  expanded: false,
  bare: true,
  fullVoice: false,
  voicePill: false,
  suggestion: false,
  waitingForYou: false,
  merging: false,
  updateCard: false,
  offline: false,
  netCard: false,
  focus: false,
  paused: false,
  notifyFresh: false,
  unread: 0,
  activitiesOpen: false,
  mascot: "idle",
  quietGlance: false,
};

describe("resolveIslandState", () => {
  it("is idle when bare", () => {
    expect(resolveIslandState(base)).toBe("idle");
  });

  it("picks Ask, Agents and Settings over glance", () => {
    expect(resolveIslandState({ ...base, asking: true, askTab: "ask", bare: false })).toBe("ask");
    expect(resolveIslandState({ ...base, asking: true, askTab: "agents", bare: false })).toBe("agents");
    expect(resolveIslandState({ ...base, asking: true, settingsView: true, bare: false })).toBe("settings");
  });

  it("picks suggestion and working before idle", () => {
    expect(resolveIslandState({ ...base, suggestion: true, bare: false })).toBe("suggestion");
    expect(resolveIslandState({ ...base, mascot: "working", voicePill: true, bare: false })).toBe("working");
  });

  it("shows a fresh notify pill, then unread", () => {
    expect(resolveIslandState({ ...base, notifyFresh: true, unread: 1, bare: true })).toBe("new-message");
    expect(resolveIslandState({ ...base, unread: 2, bare: true })).toBe("unread");
  });

  it("opens glance and quiet glance when expanded", () => {
    expect(resolveIslandState({ ...base, expanded: true, bare: false })).toBe("glance");
    expect(resolveIslandState({ ...base, expanded: true, bare: false, quietGlance: true })).toBe("quiet-glance");
  });
});
