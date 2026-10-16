// What Settings shows for "What Duskie has learned": a plain name for each
// learned sender or source, how it leans, and search and sort over the list.

import type { LessonRow } from "./types";

export type LearnedSort = "recent" | "name" | "level";

const GROUPS: Record<string, string> = {
  app: "App",
  mail: "Mail",
  github: "GitHub",
  chat: "Chat",
};

/** "app:WhatsApp" becomes the group "App" and the name "WhatsApp". */
export function splitKey(key: string): { group: string; name: string; prefix: string } {
  const i = key.indexOf(":");
  if (i < 0) return { group: "Other", name: key, prefix: "" };
  const prefix = key.slice(0, i);
  return { group: GROUPS[prefix] ?? "Other", name: key.slice(i + 1), prefix: `${prefix}:` };
}

/** One word for how a learned shift leans. */
export function lean(shift: number): "more" | "less" | "neutral" {
  if (shift >= 6) return "more";
  if (shift <= -6) return "less";
  return "neutral";
}

/** Why it leans, in a sentence. */
export function reasonFor(shift: number): string {
  switch (lean(shift)) {
    case "more":
      return "You usually act on these";
    case "less":
      return "You asked for less of these";
    default:
      return "Not much learned yet";
  }
}

export function filterRows(rows: LessonRow[], query: string): LessonRow[] {
  const q = query.trim().toLowerCase();
  if (!q) return rows;
  return rows.filter((r) => {
    const { group, name } = splitKey(r.key);
    return `${group} ${name}`.toLowerCase().includes(q);
  });
}

export function sortRows(rows: LessonRow[], by: LearnedSort): LessonRow[] {
  const out = [...rows];
  if (by === "name") out.sort((a, b) => splitKey(a.key).name.localeCompare(splitKey(b.key).name));
  else if (by === "level") out.sort((a, b) => a.shift - b.shift || a.key.localeCompare(b.key));
  else out.sort((a, b) => b.updated - a.updated || a.key.localeCompare(b.key));
  return out;
}

/** The groups present in the list, with how many rows each has. */
export function groupsOf(rows: LessonRow[]): { prefix: string; group: string; count: number }[] {
  const seen = new Map<string, { prefix: string; group: string; count: number }>();
  for (const r of rows) {
    const { group, prefix } = splitKey(r.key);
    const g = seen.get(prefix) ?? { prefix, group, count: 0 };
    g.count += 1;
    seen.set(prefix, g);
  }
  return [...seen.values()].sort((a, b) => a.group.localeCompare(b.group));
}
