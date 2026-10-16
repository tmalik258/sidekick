import { describe, expect, it } from "vitest";
import { filterRows, groupsOf, lean, reasonFor, sortRows, splitKey } from "./learned";

const rows = [
  { key: "app:WhatsApp", shift: -14, updated: 30 },
  { key: "mail:ali@x", shift: 12, updated: 10 },
  { key: "github:ci", shift: 0, updated: 20 },
];

describe("learned list", () => {
  it("splits a key into group and name", () => {
    expect(splitKey("app:WhatsApp")).toEqual({ group: "App", name: "WhatsApp", prefix: "app:" });
    expect(splitKey("odd").group).toBe("Other");
  });

  it("says which way a shift leans", () => {
    expect(lean(12)).toBe("more");
    expect(lean(-14)).toBe("less");
    expect(lean(2)).toBe("neutral");
    expect(reasonFor(-20)).toMatch(/less/);
  });

  it("searches by name and group", () => {
    expect(filterRows(rows, "whats")).toHaveLength(1);
    expect(filterRows(rows, "mail")).toHaveLength(1);
    expect(filterRows(rows, "")).toHaveLength(3);
  });

  it("sorts by recent, name and level", () => {
    expect(sortRows(rows, "recent").map((r) => r.key)[0]).toBe("app:WhatsApp");
    expect(sortRows(rows, "name").map((r) => r.key)[0]).toBe("mail:ali@x");
    expect(sortRows(rows, "level").map((r) => r.key)[0]).toBe("app:WhatsApp");
  });

  it("lists the groups with counts", () => {
    expect(groupsOf(rows)).toHaveLength(3);
    expect(groupsOf(rows).find((g) => g.prefix === "mail:")?.count).toBe(1);
  });
});
