import { describe, expect, it } from "vitest";
import { MASCOT_STATES } from "./types";
import { channels, hexToRgb, PALETTES, WASH_EVENTS, washFor } from "./wash";

describe("wash palettes", () => {
  it("has a palette for every event", () => {
    for (const e of WASH_EVENTS) expect(PALETTES[e]).toBeDefined();
  });

  it("parses hex colors into 0 to 1 channels", () => {
    expect(hexToRgb("#000000")).toEqual([0, 0, 0]);
    expect(hexToRgb("#ffffff")).toEqual([1, 1, 1]);
    expect(channels(PALETTES.idle)).toHaveLength(9);
  });

  it("maps every mascot state to a known wash", () => {
    for (const s of MASCOT_STATES) expect(WASH_EVENTS).toContain(washFor(s));
    expect(washFor("sleeping")).toBe("idle");
    expect(washFor("focus")).toBe("idle");
    expect(washFor("error")).toBe("error");
  });

  it("only transient events settle back", () => {
    expect(PALETTES.idle.settleMs).toBe(0);
    expect(PALETTES.success.settleMs).toBeGreaterThan(0);
  });
});
