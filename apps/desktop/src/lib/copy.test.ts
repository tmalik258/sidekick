import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

// Writing rules: no em dashes in UI text, code comments or docs.
const DASH = String.fromCharCode(0x2014);

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    if (statSync(p).isDirectory()) return files(p);
    return /\.(tsx?|css|md|rs)$/.test(n) ? [p] : [];
  });
}

describe("copy", () => {
  it("has no em dashes", () => {
    const roots = ["src", "src-tauri/src", "../../docs", "../../crates"];
    const bad = roots
      .flatMap(files)
      .flatMap((f) =>
        readFileSync(f, "utf8")
          .split("\n")
          .map((l, i) => ({ f, i: i + 1, l }))
          .filter(({ l }) => l.includes(DASH) && !new RegExp(`[[^]]*${DASH}`).test(l)),
      )
      .map(({ f, i }) => `${f}:${i}`);
    expect(bad).toEqual([]);
  });
});
