"use client";

// Setup checklist: what is installed, connected and configured, with the
// exact command for each missing piece. Used by the welcome steps and the
// Setup tab in Settings. Every check runs again on "Check again", and for a
// couple of minutes after a Run so a finished install turns green.
// In welcome, items expand their one-click how-to here so onboarding never
// dumps the user into Settings mid-flow.
// Rows paint at once: the last known status (cached), or the catalog's
// placeholders on the very first run. Refreshing keeps every row on screen.

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useSidekick } from "@/lib/store";
import type { SetupGroup, SetupItem } from "@/lib/types";
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
  const { data: status, refreshing: checking, refresh, set } = useCached("setup-status", api.setupStatus);
  const watch = useRef<ReturnType<typeof setInterval> | null>(null);

  const check = useCallback(() => refresh().catch(() => undefined), [refresh]);

  /** Checks again every few seconds for a while, after starting an install. */
  const watchForChanges = useCallback(() => {
    if (watch.current) clearInterval(watch.current);
    let left = WATCH_TIMES;
    watch.current = setInterval(() => {
      left -= 1;
      if (left <= 0 && watch.current) clearInterval(watch.current);
      void api
        .setupStatus()
        .then(set)
        .catch(() => undefined);
    }, WATCH_EVERY_MS);
  }, [set]);

  useEffect(
    () => () => {
      if (watch.current) clearInterval(watch.current);
    },
    [],
  );

  return { status, checking, check, watchForChanges };
}

/**
 * Rows in one fixed order: the catalog's, with live data in place of each
 * placeholder once it is known, then anything new from Rust at the end. So
 * rows never move or pop in when the data arrives.
 */
function mergeRows(groups: SetupGroup[], live: SetupItem[] | undefined): SetupItem[] {
  const catalog = SETUP_CATALOG.filter((c) => groups.includes(c.group));
  if (!live) return catalog.map(skeletonItem);
  const byId = new Map(live.filter((i) => groups.includes(i.group)).map((i) => [i.id, i]));
  const rows: SetupItem[] = [];
  for (const c of catalog) {
    const item = byId.get(c.id);
    if (item) rows.push(item);
    byId.delete(c.id);
  }
  return [...rows, ...byId.values()];
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
  // Coming back from a step finished elsewhere: make sure its row shows.
  const justDone = useSidekick((s) => s.justDone);
  const showAll = showOptional || Boolean(justDone);
  const [openGuide, setOpenGuide] = useState<string | null>(null);
  const [showDone, setShowDone] = useState(false);

  // Stable callbacks, so a row only re-renders when its own data changes.
  const run = useCallback(
    (id: string) => {
      setError(null);
      api
        .setupRun(id)
        .then(watchForChanges)
        .catch((e) => setError(String(e)));
    },
    [watchForChanges],
  );
  const toggleGuide = useCallback((id: string) => setOpenGuide((open) => (open === id ? null : id)), []);
  const done = useCallback(() => {
    void check();
    watchForChanges();
  }, [check, watchForChanges]);

  const items = mergeRows(groups, status?.items);
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
        <div className="flex flex-col gap-1.5 rounded-2xl bg-[#0a84ff]/15 px-3.5 py-2.5 ring-1 ring-inset ring-[#0a84ff]/40">
          <div className="flex items-center gap-2">
            <p className="flex-1 font-medium text-white">Install everything recommended</p>
            <SmallButton
              onClick={() => {
                void navigator.clipboard.writeText(status.installAll ?? "").catch(() => undefined);
              }}
            >
              Copy command
            </SmallButton>
            <SmallButton primary onClick={() => run("all")}>
              Install all
            </SmallButton>
          </div>
        </div>
      )}

      {groups.map((g) => {
        const shown = items.filter((i) => i.group === g && (i.recommended || showAll));
        if (shown.length === 0) return null;
        // Finished steps fold into one line, so the list is what is left to do.
        const ready = shown.filter((i) => i.done && i.id !== justDone);
        const rows = showDone ? shown : shown.filter((i) => !ready.includes(i));
        return (
          <section key={g} className="flex flex-col gap-1.5">
            {groups.length > 1 && (
              <h3 className="px-1 pt-1 text-[12px] font-semibold text-[rgb(235_235_245/0.55)]">{GROUP_TITLES[g]}</h3>
            )}
            {rows.map((item) => (
              <SetupRow
                key={item.id}
                item={item}
                checking={status === null}
                onRun={run}
                guideOpen={openGuide === item.id}
                onToggleGuide={toggleGuide}
                onOpenTab={onOpenTab}
                inlineGuides={inlineGuides}
                onDone={done}
              />
            ))}
            {!showDone && ready.length > 0 && (
              <button
                type="button"
                onClick={() => setShowDone(true)}
                className="chip flex items-center gap-2 rounded-2xl px-3.5 py-2 text-left text-[12px] text-[rgb(235_235_245/0.6)] hover:bg-white/[0.04] hover:text-white"
              >
                <span className="grid size-[18px] shrink-0 place-items-center rounded-full bg-[#30d158] text-[10px] font-bold text-black">
                  ✓
                </span>
                <span className="min-w-0 flex-1 truncate">
                  {ready.map((i) => i.title).join(", ")} {ready.length === 1 ? "is" : "are"} ready
                </span>
              </button>
            )}
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

/**
 * The setup steps for one thing, inside its own card (Claude Code's install,
 * hooks and tools in its AI card). Only what is left to do; nothing once done.
 */
export function SetupItems({ ids }: { ids: string[] }) {
  const { status, watchForChanges, check } = useSetupStatus();
  const [error, setError] = useState<string | null>(null);
  const [openGuide, setOpenGuide] = useState<string | null>(null);
  const justDone = useSidekick((s) => s.justDone);
  const run = useCallback(
    (id: string) => {
      setError(null);
      api
        .setupRun(id)
        .then(watchForChanges)
        .catch((e) => setError(String(e)));
    },
    [watchForChanges],
  );
  const toggleGuide = useCallback((id: string) => setOpenGuide((open) => (open === id ? null : id)), []);
  const done = useCallback(() => {
    void check();
    watchForChanges();
  }, [check, watchForChanges]);
  if (!status) return null;
  const rows = ids
    .map((id) => status.items.find((i) => i.id === id))
    .filter((i): i is SetupItem => Boolean(i && (!i.done || i.id === justDone)));
  if (rows.length === 0) return null;
  return (
    <div className="flex flex-col gap-1.5 text-[13px]">
      {rows.map((item) => (
        <SetupRow
          key={item.id}
          item={item}
          onRun={run}
          guideOpen={openGuide === item.id}
          onToggleGuide={toggleGuide}
          inlineGuides
          onDone={done}
        />
      ))}
      {error && (
        <p role="alert" className="text-[12px] text-red-300">
          {error}
        </p>
      )}
    </div>
  );
}

/** Recommended steps not done yet, by the Settings tab that has them. */
export function usePendingByTab(): Record<string, SetupItem[]> {
  const { status } = useSetupStatus();
  const out: Record<string, SetupItem[]> = {};
  for (const i of status?.items ?? []) {
    const tab = i.tab ?? (i.group === "ai" ? "ai" : null);
    if (i.done || !i.recommended || i.group === "tools" || !tab || tab === "home") continue;
    out[tab] = [...(out[tab] ?? []), i];
  }
  return out;
}
