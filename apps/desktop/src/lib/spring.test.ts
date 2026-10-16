import { describe, expect, it } from "vitest";
import { motionFor, SPRINGS, spring } from "./spring";

describe("spring", () => {
  it("maps response and damping to stiffness and damping", () => {
    const s = spring({ response: 1, damping: 1 });
    const omega = 2 * Math.PI;
    expect(s.stiffness).toBeCloseTo(omega * omega, 1);
    expect(s.damping).toBeCloseTo(2 * omega, 1);
    expect(s.mass).toBe(1);
  });

  it("is snappier for a shorter response", () => {
    expect(spring({ response: 0.3, damping: 1 }).stiffness).toBeGreaterThan(
      spring({ response: 0.6, damping: 1 }).stiffness,
    );
  });

  it("never divides by zero or goes negative", () => {
    const s = spring({ response: 0, damping: -1 });
    expect(Number.isFinite(s.stiffness)).toBe(true);
    expect(s.damping).toBe(0);
  });

  it("turns into no animation for reduced motion", () => {
    expect(motionFor(SPRINGS.morph, true)).toEqual({ duration: 0 });
    expect(motionFor(SPRINGS.morph, false)).toMatchObject({ type: "spring" });
  });
});
