import { describe, expect, it } from "vitest";
import { baseName, countChanges, groupByFolder } from "./changes";
import type { GitFile } from "./types";

const f = (path: string, staged: boolean, unstaged: boolean): GitFile => ({ path, kind: "M", staged, unstaged });

describe("changes list", () => {
  it("groups files under folder headings in order", () => {
    const g = groupByFolder([f("src/b.ts", false, true), f("README.md", false, true), f("src/a.ts", true, false)]);
    expect(g.map((x) => x.folder)).toEqual(["(root)", "src"]);
    expect(g[1]?.files.map((x) => baseName(x.path))).toEqual(["a.ts", "b.ts"]);
  });

  it("counts staged files, and flags a fully staged tree", () => {
    const list = [f("a", true, false), f("b", true, true), f("c", false, true)];
    expect(countChanges(list)).toEqual({ staged: 2, unstaged: 2, total: 3, all: false });
    expect(countChanges([f("a", true, false)]).all).toBe(true);
    expect(countChanges([]).all).toBe(false);
  });
});
