import { describe, expect, it } from "vitest";
import { CAPTION_CHARS, captionOf } from "./live";

describe("captionOf", () => {
  it("shows the sentence being spoken", () => {
    expect(captionOf("You have two meetings. The first is at ten")).toBe("The first is at ten");
  });
  it("keeps a finished sentence until the next starts", () => {
    expect(captionOf("Hotspot is on. ")).toBe("Hotspot is on.");
  });
  it("trims long sentences from the front at a word", () => {
    const long = `${"word ".repeat(40)}end`;
    const c = captionOf(long);
    expect(c.startsWith("…")).toBe(true);
    expect(c.length).toBeLessThanOrEqual(CAPTION_CHARS + 1);
    expect(c.endsWith("end")).toBe(true);
  });
  it("leaves out option lines and markdown", () => {
    expect(captionOf("Found **it** in [Downloads](C:/x).\nOPTION: Open it")).toBe("Found it in Downloads.");
  });
  it("is empty for nothing", () => {
    expect(captionOf("  ")).toBe("");
  });
});
