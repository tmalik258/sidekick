// Live captions: the newest sentence of a spoken answer, short enough for the
// compact island. The full answer stays in Ask history.

/** Longest caption shown at once. */
export const CAPTION_CHARS = 90;

/** The sentence being spoken now, trimmed from the front when long. */
export function captionOf(text: string): string {
  const flat = text
    .split("\n")
    .filter((l) => !/^\s*OPTION:/i.test(l))
    .join(" ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[*_`#|>]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  if (!flat) return "";
  const parts = flat.split(/(?<=[.!?])\s+/);
  let last = parts.at(-1) ?? "";
  // A sentence just ended with nothing after it yet: keep showing it.
  if (!last && parts.length > 1) last = parts.at(-2) ?? "";
  if (last.length <= CAPTION_CHARS) return last;
  const cut = last.slice(-CAPTION_CHARS);
  const space = cut.indexOf(" ");
  return `…${space > 0 ? cut.slice(space + 1) : cut}`;
}
