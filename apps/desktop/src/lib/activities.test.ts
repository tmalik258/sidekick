import { describe, expect, it } from "vitest";
import { ACTIVITY_HOLD_MS, INACTIVITY_MS, visibleActivities } from "./activities";
import type { Activity } from "./types";

const base: Activity = {
  id: "a",
  label: "Downloading model",
  source: "browser",
  parentId: null,
  phase: "downloading",
  startedAt: 0,
  lastProgressAt: 0,
  transferred: null,
  total: null,
  outcome: null,
  dismissed: false,
  revision: 0,
  destination: null,
};

describe("visibleActivities", () => {
  it("drops failed and stalled ones after their hold by default", () => {
    const failed = { ...base, id: "f", phase: "failed" as const };
    const now = ACTIVITY_HOLD_MS + 1;
    expect(visibleActivities([failed], now)).toHaveLength(0);
    expect(visibleActivities([base], INACTIVITY_MS + 1)).toHaveLength(0);
  });

  it("keeps failed and stalled ones until dismissed when asked", () => {
    const failed = { ...base, id: "f", phase: "failed" as const };
    expect(visibleActivities([failed], 10 * ACTIVITY_HOLD_MS, true)).toHaveLength(1);
    expect(visibleActivities([base], INACTIVITY_MS + 1, true)).toHaveLength(1);
    expect(visibleActivities([{ ...failed, dismissed: true }], 1e9, true)).toHaveLength(0);
  });

  it("still clears finished ones", () => {
    const done = { ...base, id: "d", phase: "completed" as const };
    expect(visibleActivities([done], ACTIVITY_HOLD_MS + 1, true)).toHaveLength(0);
  });

  it("matches by id and keeps the newest revision", () => {
    const old = { ...base, label: "old", revision: 1 };
    const fresh = { ...base, label: "fresh", revision: 2 };
    const out = visibleActivities([fresh, old], 0);
    expect(out).toHaveLength(1);
    expect(out[0].label).toBe("fresh");
  });
});
