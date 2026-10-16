// The Git dock's Changes list: files grouped under their folder, and what
// the staging counts say.

import type { GitFile } from "./types";

export interface Group {
  folder: string;
  files: GitFile[];
}

/** Files grouped by folder, folders and files in name order. The repo root
 * is shown as "(root)". */
export function groupByFolder(files: GitFile[]): Group[] {
  const map = new Map<string, GitFile[]>();
  for (const f of files) {
    const at = f.path.lastIndexOf("/");
    const folder = at < 0 ? "(root)" : f.path.slice(0, at);
    map.set(folder, [...(map.get(folder) ?? []), f]);
  }
  return [...map.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([folder, list]) => ({ folder, files: [...list].sort((a, b) => a.path.localeCompare(b.path)) }));
}

export interface Counts {
  staged: number;
  unstaged: number;
  total: number;
  /** Everything is staged: the button reads Unstage all. */
  all: boolean;
}

export function countChanges(files: GitFile[]): Counts {
  const staged = files.filter((f) => f.staged).length;
  const unstaged = files.filter((f) => f.unstaged).length;
  return {
    staged,
    unstaged,
    total: files.length,
    all: files.length > 0 && files.every((f) => f.staged && !f.unstaged),
  };
}

/** The base name shown in the list; the folder is in the heading. */
export function baseName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}
