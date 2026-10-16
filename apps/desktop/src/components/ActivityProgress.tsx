"use client";
import {
  acceptActivities,
  activityElapsedSecs,
  activityLabel,
  dismissLocal,
  useActivities,
  visibleActivities,
} from "@/lib/activities";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import { useDuskie } from "@/lib/store";
import { Icon } from "./Icon";
import { Tip } from "./Tip";

export function ActivityProgress({ sources }: { sources?: string[] } = {}) {
  const state = useActivities();
  const now = useNow(1000);
  const keepProblems = useDuskie((s) => s.settings.redesign.enabled);
  const activities = visibleActivities([...state.activities, ...state.local], now, keepProblems).filter(
    (a) => !sources || sources.includes(a.source),
  );
  if (activities.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      {activities.map((a) => {
        const elapsed = activityElapsedSecs(a, now);
        const label = activityLabel(a, now, { elapsed: false });
        return (
          <div key={a.id} className="flex min-w-0 items-center gap-2">
            <p className="min-w-0 flex-1 truncate text-[13px] leading-4.5 text-[var(--ink,#f5f5f7)]" role="status">
              {label}
              {elapsed !== null && (
                <span className="text-[var(--i3,rgb(235_235_245/0.55))] tabular-nums"> · {elapsed}s</span>
              )}
            </p>
            <span className="flex shrink-0 items-center gap-0.5">
              <Tip label="Check again">
                <button
                  type="button"
                  aria-label="Check again"
                  className="chip grid size-7 place-items-center rounded-full text-white/55 hover:bg-white/10 hover:text-white"
                  onClick={() =>
                    void api
                      .activityCheck(a.id)
                      .then(acceptActivities)
                      .catch(() => {})
                  }
                >
                  <Icon name="refresh" size={12} />
                </button>
              </Tip>
              <Tip label="Dismiss">
                <button
                  type="button"
                  aria-label="Dismiss"
                  className="chip grid size-7 place-items-center rounded-full text-white/55 hover:bg-white/10 hover:text-white"
                  onClick={() => {
                    if (a.source === "open") dismissLocal(a.id);
                    void api.activityDismiss(a.id).catch(() => {});
                  }}
                >
                  <Icon name="close" size={12} />
                </button>
              </Tip>
            </span>
          </div>
        );
      })}
    </div>
  );
}
