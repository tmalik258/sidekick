"use client";

// Setup checklist: what is installed, connected and configured, with the
// exact command for each missing piece. Used by the welcome steps and the
// Setup tab in Settings. Every check runs again on "Check again", and for a
// couple of minutes after a Run so a finished install turns green.

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import type { SetupGroup, SetupItem, SetupStatus } from "@/lib/types";

const GROUP_TITLES: Record<SetupGroup, string> = {
  ai: "AI",
  connect: "Connections",
  tools: "Tools",
};

const TAB_LABELS: Record<string, string> = {
  ai: "AI",
  browser: "Browser",
  general: "General",
  search: "Search",
  today: "Today",
  voice: "Voice",
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
}: {
  groups: SetupGroup[];
  onOpenTab: (tab: string) => void;
  /** Hide optional items behind "More" (the welcome steps). */
  compact?: boolean;
}) {
  const { status, checking, check, watchForChanges } = useSetupStatus();
  const [error, setError] = useState<string | null>(null);
  const [showOptional, setShowOptional] = useState(!compact);

  const run = (id: string) => {
    setError(null);
    api
      .setupRun(id)
      .then(watchForChanges)
      .catch((e) => setError(String(e)));
  };

  if (!status) return <p className="py-2 text-[12.5px] text-[rgb(235_235_245/0.55)]">Checking this PC...</p>;

  const items = status.items.filter((i) => groups.includes(i.group));
  const recommended = items.filter((i) => i.recommended);
  const doneCount = recommended.filter((i) => i.done).length;
  const optional = items.filter((i) => !i.recommended);

  return (
    <div className="flex flex-col gap-2 text-[13px]">
      <div className="flex items-center gap-2">
        <p className="flex-1 text-[12px] text-[rgb(235_235_245/0.6)]">
          {recommended.length > 0 && doneCount === recommended.length
            ? "Everything recommended is set up."
            : `${doneCount} of ${recommended.length} recommended steps done.`}
        </p>
        <SmallButton onClick={() => void check()} disabled={checking}>
          {checking ? "Checking..." : "Check again"}
        </SmallButton>
      </div>

      {error && (
        <p role="alert" className="rounded-lg bg-red-500/15 px-3 py-2 text-[12px] text-red-200">
          {error}
        </p>
      )}

      {groups.includes("tools") && status.installAll && (
        <div className="flex flex-col gap-1.5 rounded-2xl bg-[#0a84ff]/15 px-3.5 py-2.5 ring-1 ring-[#0a84ff]/40">
          <div className="flex items-center gap-2">
            <p className="flex-1 font-medium text-white">Install all recommended tools</p>
            <CopyButton text={status.installAll} />
            <SmallButton primary onClick={() => run("all")}>
              Run
            </SmallButton>
          </div>
          <Code text={status.installAll} />
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
              <Row key={item.id} item={item} onRun={run} onOpenTab={onOpenTab} />
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

function Row({
  item,
  onRun,
  onOpenTab,
}: {
  item: SetupItem;
  onRun: (id: string) => void;
  onOpenTab: (tab: string) => void;
}) {
  const multiline = item.command?.includes("\n") ?? false;
  return (
    <div className="flex flex-col gap-1.5 rounded-2xl bg-white/[0.06] px-3.5 py-2.5">
      <div className="flex items-center gap-3">
        <span
          role="img"
          aria-label={item.done ? "Done" : "Not done"}
          className={`grid size-[18px] shrink-0 place-items-center rounded-full text-[10px] font-bold ${
            item.done ? "bg-[#30d158] text-black" : "ring-1 ring-white/30 ring-inset"
          }`}
        >
          {item.done ? "✓" : ""}
        </span>
        <div className="min-w-0 flex-1">
          <p className="font-medium text-white">
            {item.title}{" "}
            <span className={`font-normal ${item.done ? "text-[#30d158]" : "text-[rgb(235_235_245/0.5)]"}`}>
              {item.status}
            </span>
          </p>
          <p className="text-[11.5px] leading-snug text-[rgb(235_235_245/0.55)]">{item.why}</p>
        </div>
        {!item.done && (
          <div className="flex shrink-0 gap-1.5">
            {item.command && <CopyButton text={item.command} />}
            {item.runnable && (
              <SmallButton primary onClick={() => onRun(item.id)}>
                Run
              </SmallButton>
            )}
            {item.tab && !item.runnable && (
              <SmallButton primary={!item.command} onClick={() => onOpenTab(item.tab as string)}>
                {TAB_LABELS[item.tab] ? `Open ${TAB_LABELS[item.tab]}` : "Open"}
              </SmallButton>
            )}
          </div>
        )}
      </div>
      {!item.done && item.command && !multiline && <Code text={item.command} />}
    </div>
  );
}

function Code({ text }: { text: string }) {
  return (
    <code
      title={text}
      className="block truncate rounded-lg bg-black/30 px-2.5 py-1.5 font-mono text-[11px] text-white/80 select-all"
    >
      {text}
    </code>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be refused; the command is selectable anyway.
    }
  };
  return <SmallButton onClick={() => void copy()}>{copied ? "Copied" : "Copy"}</SmallButton>;
}

function SmallButton({
  children,
  onClick,
  disabled,
  primary,
}: {
  children: string;
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`chip rounded-full px-2.5 py-1 text-[12px] font-medium disabled:opacity-50 ${
        primary ? "bg-white text-black hover:bg-white/90" : "bg-white/[0.12] text-white/90 hover:bg-white/[0.2]"
      }`}
    >
      {children}
    </button>
  );
}
