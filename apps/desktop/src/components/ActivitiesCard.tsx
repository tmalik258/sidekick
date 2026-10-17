"use client";

// Everything running in the background, as one card: what it is, how far
// along, how fast, and a way to stop it.

import { activityLabel, dismissLocal, useActivities, visibleActivities } from "@/lib/activities";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import type { Activity } from "@/lib/types";
import { Ic } from "./Icon";

const MB = 1_000_000;

function sizeLine(a: Activity, now: number): string | null {
  if (a.total == null || a.transferred == null) return null;
  const secs = Math.max(1, (now - a.startedAt) / 1000);
  const rate = a.transferred / secs;
  const left = rate > 0 ? Math.max(0, (a.total - a.transferred) / rate) : null;
  const parts = [
    `${Math.round(a.transferred / MB)} MB of ${Math.round(a.total / MB)} MB`,
    `${(rate / MB).toFixed(1)} MB/s`,
  ];
  if (left !== null)
    parts.push(left >= 60 ? `${Math.floor(left / 60)}m ${Math.round(left % 60)}s left` : `${Math.round(left)}s left`);
  return parts.join("  ·  ");
}

export function ActivitiesCard() {
  const state = useActivities();
  const now = useNow(1000);
  const list = visibleActivities([...state.activities, ...state.local], now, true);
  const running = list.filter((a) => a.phase !== "completed" && a.phase !== "failed");
  return (
    <div className="rd-glance flex flex-col gap-2.5">
      <div className="rd-card-head">
        <p className="rd-eyebrow">Activities</p>
        <p className="rd-title">{running.length === 1 ? "1 running" : `${running.length} running`}</p>
        <p className="rd-detail">Nothing else in the background</p>
      </div>
      <div className="flex flex-col gap-1.5" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
        {list.map((a) => {
          const pct = a.total && a.transferred != null ? Math.min(1, a.transferred / a.total) : null;
          const sub = sizeLine(a, now);
          return (
            <div key={a.id} className="act-row">
              <span className="act-ic" aria-hidden>
                <Ic name={a.source === "browser" || a.source === "file" ? "folder" : "update"} size={13} />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px]">{activityLabel(a, now, { elapsed: false })}</span>
                {sub && <span className="act-sub block truncate">{sub}</span>}
                {pct !== null && (
                  <span className="act-bar" aria-hidden>
                    <i style={{ width: `${pct * 100}%` }} />
                  </span>
                )}
              </span>
              <button
                type="button"
                className="chip-s ic-only"
                aria-label={`Stop ${a.label}`}
                onClick={() => {
                  if (a.source === "open") dismissLocal(a.id);
                  void api.activityDismiss(a.id).catch(() => {});
                }}
              >
                <Ic name="x" size={12} />
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}
