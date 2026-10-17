"use client";

// The update while it downloads, opened from the working pill: version,
// a progress bar, size and time left, and a way to hide it.

import { api } from "@/lib/bridge";
import { useDuskie } from "@/lib/store";
import { updateDetail, updateLabel, updatePercent } from "@/lib/updates";
import { Ic } from "./Icon";

export function UpdateCard({ activityId, startedAt }: { activityId: string | null; startedAt: number | null }) {
  const progress = useDuskie((s) => s.updateProgress);
  const info = useDuskie((s) => s.update);
  if (!progress) return null;
  const pct = updatePercent(progress);
  let left: string | null = null;
  if (progress.total && progress.done > 0 && startedAt) {
    const rate = progress.done / Math.max(1, (Date.now() - startedAt) / 1000);
    const s = Math.round((progress.total - progress.done) / rate);
    left = s >= 60 ? `${Math.floor(s / 60)} min left` : `${s} s left`;
  }
  return (
    <div className="rd-glance flex flex-col gap-2.5">
      <div className="rd-card-head">
        <p className="rd-eyebrow flex items-center gap-1.5">
          <span className="rd-dot" style={{ background: "var(--ok)" }} />
          Duskie {info?.version ?? ""}
        </p>
        <p className="rd-title">
          {updateLabel(progress)
            .replace(/\.\.\.$/, "")
            .replace(/ · \d+%$/, "")}
        </p>
        <p className="rd-detail">Duskie restarts on its own once it is ready.</p>
      </div>
      <div className="flex flex-col gap-1.5" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
        <span className="act-bar" aria-hidden>
          <i style={{ width: `${pct ?? 0}%` }} />
        </span>
        <div className="flex justify-between text-[12px] text-[var(--ink-3)] tnum">
          <span>{updateDetail(progress)}</span>
          {left && <span>{left}</span>}
        </div>
        <div className="mt-1 flex items-center gap-1.5">
          <button
            type="button"
            className="rd-btn primary"
            disabled={progress.phase !== "opening" && progress.phase !== "failed"}
            onClick={() => void api.updateInstall().catch(() => {})}
          >
            {progress.phase === "failed" ? "Try again" : "Restart to update"}
          </button>
          {info?.url && (
            <button type="button" className="rd-btn ghost" onClick={() => void api.aiOpenLink(info.url)}>
              What is new
            </button>
          )}
          <button
            type="button"
            className="rd-btn ghost ml-auto chip-s ic-only"
            aria-label="Hide"
            disabled={!activityId}
            onClick={() => activityId && void api.activityDismiss(activityId).catch(() => {})}
          >
            <Ic name="x" size={12} />
          </button>
        </div>
      </div>
    </div>
  );
}
