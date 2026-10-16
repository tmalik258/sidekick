import { create } from "zustand";
import type { Activity, ActivitySnapshot } from "./types";

export const INACTIVITY_MS = 120_000;
/** Completed / failed stay on the island briefly, then drop (same idea as done pills). */
export const ACTIVITY_HOLD_MS = 5000;
/** Missing-partial "progress unconfirmed": short notice, then clear like a done pill. */
export const UNCONFIRMED_HOLD_MS = 2600;
export const terminal = (a: Activity) => a.phase === "completed" || a.phase === "failed";
export const ownsWorking = (a: Activity) => ["action", "open", "chat", "agent"].includes(a.source) && !terminal(a);
export const latestActivity = (activities: Activity[], source: string) =>
  activities
    .filter((a) => a.source === source)
    .sort(
      (a, b) => Number(terminal(a)) - Number(terminal(b)) || b.startedAt - a.startedAt || b.id.localeCompare(a.id),
    )[0];
export const useActivities = create<{ revision: number; activities: Activity[]; local: Activity[] }>(() => ({
  revision: -1,
  activities: [],
  local: [],
}));
export function acceptActivities(snapshot: ActivitySnapshot) {
  if (snapshot.revision <= useActivities.getState().revision) return;
  useActivities.setState({ revision: snapshot.revision, activities: snapshot.activities });
}
export function beginActivity(label: string): string {
  const id = crypto.randomUUID();
  const now = Date.now();
  useActivities.setState((s) => ({
    local: [
      ...s.local.slice(-63),
      {
        id,
        label,
        source: "open",
        parentId: null,
        phase: "running",
        startedAt: now,
        lastProgressAt: now,
        transferred: null,
        total: null,
        outcome: null,
        dismissed: false,
        revision: 0,
        destination: null,
      },
    ],
  }));
  return id;
}
export function finishActivity(id: string, ok: boolean, outcome: string) {
  useActivities.setState((s) => ({
    local: s.local.map((a) =>
      a.id === id
        ? { ...a, phase: ok ? "completed" : "failed", outcome, dismissed: false, lastProgressAt: Date.now() }
        : a,
    ),
  }));
}
export function dismissLocal(id: string) {
  useActivities.setState((s) => ({ local: s.local.map((a) => (a.id === id ? { ...a, dismissed: true } : a)) }));
}
const size = (bytes: number) => `${(bytes / 1_000_000).toFixed(bytes < 10_000_000 ? 1 : 0)} MB`;

/** Check again / Dismiss: always for downloads & setup; chat/agent only when stuck or done. */
export function needsActions(a: Activity, now: number): boolean {
  if (a.source !== "chat" && a.source !== "agent") return true;
  if (a.phase === "failed" || a.phase === "unconfirmed" || a.phase === "awaitingUser") return true;
  if (a.phase === "completed") return true;
  return now - a.lastProgressAt >= INACTIVITY_MS;
}

/** Seconds to show beside a running label; null when the label carries progress instead. */
export function activityElapsedSecs(a: Activity, now: number): number | null {
  if (a.phase === "failed" || a.phase === "completed" || a.phase === "awaitingUser") return null;
  const idle = Math.max(0, now - a.lastProgressAt);
  if (a.phase === "unconfirmed" || idle >= INACTIVITY_MS) return null;
  if (a.transferred !== null) return null;
  return Math.max(0, Math.floor((now - a.startedAt) / 1000));
}

/** `elapsed: false` when a separate timer sits beside the label. */
export function activityLabel(a: Activity, now: number, opts?: { elapsed?: boolean }): string {
  if (a.phase === "failed") return `${a.label} · ${a.outcome || "Failed"}`;
  if (a.phase === "completed") return a.outcome || `${a.label} · Completed`;
  if (a.phase === "awaitingUser")
    return `${a.source === "browser" ? "Download paused" : "Waiting for you"} · ${a.label}`;
  const phase =
    a.phase === "downloading"
      ? "Downloading"
      : a.phase === "verifying"
        ? "Verifying"
        : a.phase === "installing"
          ? "Installing"
          : "Setting up";
  const label = a.label
    .replace(/^Setting up/, phase)
    .replace(/^Updating Duskie$/, `${phase === "Setting up" ? "Updating" : phase} Duskie`);
  const idle = Math.max(0, now - a.lastProgressAt);
  if (a.phase === "unconfirmed" && idle < INACTIVITY_MS) return `${label} · progress unconfirmed`;
  if (idle >= INACTIVITY_MS || a.phase === "unconfirmed") {
    return `${label} · ${a.phase === "downloading" ? "no new data" : "no confirmed progress"} for ${Math.max(0, Math.floor(idle / 60_000))}m`;
  }
  if (a.transferred !== null) {
    const progress =
      a.total && a.total > 0
        ? `${Math.min(100, Math.floor((a.transferred / a.total) * 100))}% · ${size(a.transferred)} of ${size(a.total)}`
        : `${size(a.transferred)} received`;
    return `${label} · ${progress}`;
  }
  if (opts?.elapsed === false) return `${label}…`;
  return `${label}… · ${Math.max(0, Math.floor((now - a.startedAt) / 1000))}s`;
}
/** A stalled or failed activity: the user has to look at it, so it is never hidden by a timer. */
export function needsAttention(a: Activity, now: number): boolean {
  if (a.phase === "failed" || a.phase === "unconfirmed") return true;
  return !terminal(a) && now - a.lastProgressAt >= INACTIVITY_MS;
}

/**
 * What the island shows. Done activities clear after a short hold. With
 * `keepProblems`, failed, unconfirmed and stalled ones stay until dismissed.
 */
export function visibleActivities(activities: Activity[], now: number, keepProblems = false) {
  // Matched by id: a later copy (newer revision) replaces an earlier one.
  const unique = new Map<string, Activity>();
  for (const a of activities) {
    const seen = unique.get(a.id);
    if (!seen || a.revision >= seen.revision) unique.set(a.id, a);
  }
  return [...unique.values()]
    .filter((a) => {
      if (a.dismissed) return false;
      if (keepProblems && needsAttention(a, now)) return true;
      const idle = now - a.lastProgressAt;
      // Done / failed: brief hold, then gone (compact pill settles).
      if (terminal(a) && idle >= ACTIVITY_HOLD_MS) return false;
      // Unconfirmed never completes on its own — same short hold as done pills.
      if (a.phase === "unconfirmed" && idle >= UNCONFIRMED_HOLD_MS) return false;
      // Stalled downloads (no bytes for 2m): drop the compact pill; don't stick.
      if (idle >= INACTIVITY_MS && (a.source === "browser" || a.source === "file")) return false;
      return true;
    })
    .sort(
      (a, b) => priority(a) - priority(b) || Number(terminal(a)) - Number(terminal(b)) || b.startedAt - a.startedAt,
    );
}
function priority(a: Activity) {
  return a.source === "update" ? 0 : ["browser", "file"].includes(a.source) ? 2 : 1;
}
