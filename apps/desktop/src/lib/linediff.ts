// Which lines of `now` differ from `base`, for the editor's change markers.
// Trims the common start and end, then lines in the middle are changed
// (if base had lines there) or added; a gap with nothing left marks a
// deletion at that line.

export interface LineMarks {
  added: number[];
  changed: number[];
  /** Line after which lines were removed (0 means at the top). */
  removed: number[];
}

export function lineMarks(base: string, now: string): LineMarks {
  const a = base === "" ? [] : base.split("\n");
  const b = now.split("\n");
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }
  const marks: LineMarks = { added: [], changed: [], removed: [] };
  const oldCount = endA - start;
  for (let i = start; i < endB; i++) {
    (i - start < oldCount ? marks.changed : marks.added).push(i + 1);
  }
  if (endB === start && oldCount > 0) marks.removed.push(start);
  return marks;
}
