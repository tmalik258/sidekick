"use client";

// Setup checklist: what is installed, connected and configured, with the
// exact command for each missing piece. Used by the welcome steps and the
// Setup tab in Settings. Every check runs again on "Check again", and for a
// couple of minutes after a Run so a finished install turns green.
// In welcome, items expand their one-click how-to here so onboarding never
// dumps the user into Settings mid-flow.
// Rows paint immediately as skeletons (pulse glyphs); never a blank text line.

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import type { SetupGroup, SetupStatus } from "@/lib/types";
import { SETUP_CATALOG, skeletonItem } from "./SetupCatalog";
import { SetupRow, SmallButton } from "./SetupChecklistRow";

const GROUP_TITLES: Record<SetupGroup, string> = {
  ai: "AI",
  connect: "Connections",
  tools: "Tools",
};

const WATCH_EVERY_MS = 6000;
const WATCH_TIMES = 20;

export function useSetupStatus() {
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const watch = useRef<ReturnType<typeof setInterval> | null>(null);

  const check = useCallback(async () => {
    setChecking(true);
    try {
      setStatus(await api.setupStatus());
    } finally {
      setChecking(false);
    }
  }, []);

  /** Checks again every few seconds for a while, after starting an install. */
  const watchForChanges = useCallback(() => {
    if (watch.current) clearInterval(watch.current);
    let left = WATCH_TIMES;
    watch.current = setInterval(() => {
      left -= 1;
      if (left <= 0 && watch.current) clearInterval(watch.current);
      void api.setupStatus().then(setStatus);
    }, WATCH_EVERY_MS);
  }, []);

  useEffect(() => {
    void check();
    return () => {
      if (watch.current) clearInterval(watch.current);
    };
  }, [check]);

  return { status, checking, check, watchForChanges };
}

export function SetupChecklist({
  groups,
  onOpenTab,
  compact,
  /** Keep how-to inside the checklist (welcome) instead of jumping to a Settings tab. */
  inlineGuides,
}: {
  groups: SetupGroup[];
  onOpenTab?: (tab: string) => void;
  /** Hide optional items behind "More" (the welcome steps). */
  compact?: boolean;
  inlineGuides?: boolean;
}) {
  const { status, checking, check, watchForChanges } = useSetupStatus();
  const [error, setError] = useState<string | null>(null);
  const [showOptional, setShowOptional] = useState(!compact);
  const [openGuide, setOpenGuide] = useState<string | null>(null);

  const run = (id: string) => {
    setError(null);
    api
      .setupRun(id)
      .then(watchForChanges)
      .catch((e) => setError(String(e)));
  };

  const skeletons = SETUP_CATALOG.filter((c) => groups.includes(c.group)).map(skeletonItem);
  const live = status?.items.filter((i) => groups.includes(i.group)) ?? [];
  // Keep previous rows while refreshing; otherwise show catalog skeletons.
  const items = live.length > 0 ? live : skeletons;
  const pending = status === null || checking;
  const recommended = items.filter((i) => i.recommended);
  const doneCount = recommended.filter((i) => i.done).length;
  const optional = items.filter((i) => !i.recommended);

  return (
    <div className="flex flex-col gap-2 text-[13px]">
      <div className="flex items-center gap-2">
        <p className="flex-1 text-[12px] text-[rgb(235_235_245/0.6)]">
          {status === null
            ? "\u00a0"
            : recommended.length > 0 && doneCount === recommended.length
              ? "Everything recommended is set up."
              : `${doneCount} of ${recommended.length} recommended steps done.`}
        </p>
        <SmallButton
          onClick={() => {
            void check();
            watchForChanges();
          }}
          disabled={checking}
        >
          Check again
        </SmallButton>
      </div>

      {error && (
        <p role="alert" className="rounded-lg bg-red-500/15 px-3 py-2 text-[12px] text-red-200">
          {error}
        </p>
      )}

      {groups.includes("tools") && status?.installAll && (
        <div className="flex flex-col gap-1.5 rounded-2xl bg-[#0a84ff]/15 px-3.5 py-2.5 ring-1 ring-[#0a84ff]/40">
          <div className="flex items-center gap-2">
            <p className="flex-1 font-medium text-white">Install all recommended tools</p>
            <SmallButton
              onClick={() => {
                void navigator.clipboard.writeText(status.installAll!).catch(() => undefined);
              }}
            >
              Copy
            </SmallButton>
            <SmallButton primary onClick={() => run("all")}>
              Run
            </SmallButton>
          </div>
          <code
            title={status.installAll}
            className="block truncate rounded-lg bg-black/30 px-2.5 py-1.5 font-mono text-[11px] text-white/80 select-all"
          >
            {status.installAll}
          </code>
        </div>
      )}

      {groups.map((g) => {
        const rows = items.filter((i) => i.group === g && (i.recommended || showOptional));
        if (rows.length === 0) return null;
        return (
          <section key={g} className="flex flex-col gap-1.5">
            {groups.length > 1 && (
              <h3 className="px-1 pt-1 text-[11px] font-semibold tracking-wide text-[rgb(235_235_245/0.45)] uppercase">
                {GROUP_TITLES[g]}
              </h3>
            )}
            {rows.map((item) => (
              <SetupRow
                key={item.id}
                item={item}
                checking={pending}
                onRun={run}
                guideOpen={openGuide === item.id}
                onToggleGuide={() => setOpenGuide(openGuide === item.id ? null : item.id)}
                onOpenTab={onOpenTab}
                inlineGuides={inlineGuides}
                onDone={() => {
                  void check();
                  watchForChanges();
                }}
              />
            ))}
          </section>
        );
      })}

      {compact && optional.length > 0 && (
        <button
          type="button"
          onClick={() => setShowOptional(!showOptional)}
          className="chip self-start rounded-full px-2.5 py-1 text-[12px] text-[rgb(235_235_245/0.6)] hover:text-white"
        >
          {showOptional ? "Fewer" : `${optional.length} optional`}
        </button>
      )}
    </div>
  );
}
