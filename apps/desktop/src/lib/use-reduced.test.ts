import { describe, expect, it } from "vitest";
import { resolveReduced } from "./use-reduced";

describe("resolveReduced", () => {
  it("treats an unknown answer as allowing motion", () => {
    expect(resolveReduced(null)).toBe(false);
    expect(resolveReduced(undefined)).toBe(false);
  });

  it("passes the user's preference through", () => {
    expect(resolveReduced(true)).toBe(true);
    expect(resolveReduced(false)).toBe(false);
  });
});
