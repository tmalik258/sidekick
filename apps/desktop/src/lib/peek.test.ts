import { describe, expect, it } from "vitest";
import { bubblesOf, PEEK_CLOSE_MS, PEEK_OPEN_MS, peekDelay, pillKind, wantsOpen } from "./peek";

const off = { hover: false, alt: false, focus: false, pinned: false };

describe("peek", () => {
  it("opens on hover, Alt or focus, and stays once pinned", () => {
    expect(wantsOpen(off)).toBe(false);
    expect(wantsOpen({ ...off, hover: true })).toBe(true);
    expect(wantsOpen({ ...off, alt: true })).toBe(true);
    expect(wantsOpen({ ...off, focus: true })).toBe(true);
    expect(wantsOpen({ ...off, pinned: true })).toBe(true);
  });

  it("opens after about 220 ms and folds after about 450 ms", () => {
    expect(peekDelay(false, true)).toBe(PEEK_OPEN_MS);
    expect(peekDelay(true, false)).toBe(PEEK_CLOSE_MS);
    expect(peekDelay(true, true)).toBeNull();
    expect(peekDelay(false, false)).toBeNull();
  });

  it("does not wait when motion is reduced", () => {
    expect(peekDelay(false, true, true)).toBe(0);
  });
});

describe("bubbles and pill", () => {
  it("shows at most three bubbles and counts the rest", () => {
    expect(bubblesOf([1, 2])).toEqual({ shown: [1, 2], more: 0 });
    expect(bubblesOf([1, 2, 3, 4, 5])).toEqual({ shown: [1, 2, 3], more: 2 });
  });

  it("an idle island shows a pill, then the unread count; a busy one shows neither", () => {
    expect(pillKind({ unread: 1, fresh: true, busy: false })).toBe("pill");
    expect(pillKind({ unread: 3, fresh: false, busy: false })).toBe("count");
    expect(pillKind({ unread: 0, fresh: true, busy: false })).toBe("none");
    expect(pillKind({ unread: 2, fresh: true, busy: true })).toBe("none");
  });
});
