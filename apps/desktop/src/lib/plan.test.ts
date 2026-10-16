import { describe, expect, it } from "vitest";
import { claimsPlanReady, parsePlanDocument } from "./plan";

describe("parsePlanDocument", () => {
  it("does not treat a plain bullet list as a plan", () => {
    expect(parsePlanDocument("Notes:\n- tidy the cache\n- rename the flag")).toBeNull();
  });

  it("reads numbered steps as a plan", () => {
    const doc = parsePlanDocument("Plan: Fix login\n1. Add the test.\n2. Fix the handler.\nThe plan is ready.");
    expect(doc?.items.map((i) => i.text)).toEqual(["Add the test.", "Fix the handler."]);
    expect(claimsPlanReady("The plan is ready.")).toBe(true);
  });
});
