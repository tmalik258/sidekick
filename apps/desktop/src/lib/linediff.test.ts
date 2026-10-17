import { describe, expect, it } from "vitest";
import { lineMarks } from "./linediff";

describe("lineMarks", () => {
  it("marks nothing when unchanged", () => {
    expect(lineMarks("a\nb", "a\nb")).toEqual({ added: [], changed: [], removed: [] });
  });
  it("marks a changed line and an added one", () => {
    expect(lineMarks("a\nb\nc", "a\nB\nx\nc")).toEqual({ added: [3], changed: [2], removed: [] });
  });
  it("marks a removal", () => {
    expect(lineMarks("a\nb\nc", "a\nc")).toEqual({ added: [], changed: [], removed: [1] });
  });
  it("treats a new file as all added", () => {
    expect(lineMarks("", "x\ny").added).toEqual([1, 2]);
  });
});
