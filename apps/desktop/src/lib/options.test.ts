import { describe, expect, it } from "vitest";
import { extractChoices } from "./options";

describe("extractChoices", () => {
  it("does not turn a numbered status list into choices", () => {
    const text =
      "Done. Here is what changed:\n1. **Cache:** added, tuned, or removed\n2. **Retry:** now on, off, or auto\nAll good.";
    expect(extractChoices(text).options).toEqual([]);
  });

  it("does not read 'one or more' as a multi pick without a question", () => {
    const text = "This fixes one or more bugs.\n- first\n- second";
    const c = extractChoices(text);
    expect(c.options).toEqual([]);
    expect(c.body).toContain("first");
  });

  it("still lifts choices when the message asks a question", () => {
    const text = "Which way?\n1. **Folder:** src, lib, or app?\n2. **Mode:** fast or safe?";
    expect(extractChoices(text).options.length).toBeGreaterThanOrEqual(2);
  });

  it("keeps explicit OPTION lines", () => {
    expect(extractChoices("Pick one\nOPTION: A\nOPTION: B").options).toEqual(["A", "B"]);
  });
});
