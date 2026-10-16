import { describe, expect, it } from "vitest";
import { ISLAND_STATES, statesFor } from "./island-states";
import { MASCOT_STATES } from "./types";
import { PALETTES } from "./wash";

describe("island state registry", () => {
  it("has unique ids", () => {
    const ids = ISLAND_STATES.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("covers every state the design lists", () => {
    expect(ISLAND_STATES).toHaveLength(25);
  });

  it("points only at real washes", () => {
    for (const s of ISLAND_STATES) expect(PALETTES[s.wash]).toBeDefined();
  });

  it("has a state for every mascot state", () => {
    for (const m of MASCOT_STATES) expect(statesFor(m).length).toBeGreaterThan(0);
  });

  it("uses no dashes or emoji in spoken text", () => {
    for (const s of ISLAND_STATES) {
      if (s.announce) expect(s.announce).toMatch(/^[A-Za-z ]+$/);
    }
  });
});
