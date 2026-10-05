"use client";

// While Sidekick waits on something you finish elsewhere (signing in, loading
// the extension, an install), the island keeps the steps on screen with copy
// buttons, so nothing disappears when Settings closes. It never blocks other
// apps: outside its own shape the island is click-through. Every button has
// a global Alt key (Alt 0 cancels), since the island has no focus here,
// until the guide is sent to the background.

import { useEffect, useRef, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { backgroundWaiting, cancelWaiting, minimizeWaiting, type Waiting } from "@/lib/store";

interface GuideButton {
  label: string;
  run: () => void;
  kind: "primary" | "copy" | "plain";
  title?: string;
}

export interface Guide {
  buttons: GuideButton[];
  copied: string | null;
}

/**
 * The guide's buttons and their global Alt keys. Lives in the island, not the
 * card, so the keys work while the guide is minimized to a pill too.
 */
export function useGuide(waiting: Waiting | null): Guide {
  const [copied, setCopied] = useState<string | null>(null);
  const copy = (label: string, text: string) =>
    void navigator.clipboard
      .writeText(text)
      .then(() => {
        setCopied(label);
        setTimeout(() => setCopied(null), 1500);
      })
      .catch(() => undefined);

  // Sent to the background: the keys go back to other apps.
  const buttons: GuideButton[] =
    !waiting || waiting.background
      ? []
      : [
          ...(waiting.copies ?? []).map((c) => ({
            label: c.label,
            title: c.text,
            kind: "copy" as const,
            run: () => copy(c.label, c.text),
          })),
          ...(waiting.again ? [{ label: "Open again", kind: "primary" as const, run: waiting.again }] : []),
          {
            label: waiting.minimized ? "Keep open" : "Minimize",
            kind: "plain",
            run: () => minimizeWaiting(!waiting.minimized),
          },
          { label: "Run in background", kind: "plain", run: backgroundWaiting },
        ];
  const keyed = buttons.slice(0, 9);
  const latest = useRef(keyed);
  latest.current = keyed;

  useEffect(() => {
    if (keyed.length === 0) return;
    void api.guideKeys(keyed.length).catch(() => undefined);
    const off = listen(EVENTS.guideKey, (n) => {
      if (n === 0) cancelWaiting();
      else latest.current[n - 1]?.run();
    });
    return () => {
      void off.then((f) => f());
      void api.guideKeys(0).catch(() => undefined);
    };
  }, [keyed.length]);
  return { buttons, copied };
}

export function IslandGuide({ waiting, guide }: { waiting: Waiting; guide: Guide }) {
  const { buttons, copied } = guide;

  const copies = buttons.filter((b) => b.kind === "copy");
  const actions = buttons.filter((b) => b.kind !== "copy");
  const key = (b: GuideButton) => {
    const i = buttons.indexOf(b);
    return i < 9 ? `Alt ${i + 1}` : null;
  };

  return (
    <div className="flex flex-col">
      <div className="flex items-center gap-2.5">
        <span className="relative flex size-2.5 shrink-0" aria-hidden="true">
          <span className="absolute inline-flex size-full animate-ping rounded-full bg-[#0a84ff] opacity-60" />
          <span className="relative inline-flex size-2.5 rounded-full bg-[#0a84ff]" />
        </span>
        <p className="min-w-0 flex-1 truncate font-display text-[15px] leading-5 font-semibold tracking-[-0.015em] text-white">
          Waiting for {waiting.label}
        </p>
      </div>
      {waiting.steps && waiting.steps.length > 0 && (
        <ol className="mt-2 list-decimal space-y-1 pl-4 text-[13px] leading-snug text-[rgb(235_235_245/0.75)]">
          {waiting.steps.map((s) => (
            <li key={s}>{s}</li>
          ))}
        </ol>
      )}
      {copies.length > 0 && (
        <div className="mt-2.5 flex flex-wrap gap-1.5">
          {copies.map((b) => (
            <GuideChip key={b.label} button={b} hint={key(b)} done={copied === b.label} />
          ))}
        </div>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-1.5">
        {actions.map((b) => (
          <GuideChip key={b.label} button={b} hint={key(b)} />
        ))}
        <button
          type="button"
          onClick={cancelWaiting}
          className="chip ml-auto rounded-full px-2.5 py-1.5 text-[12.5px] text-[rgb(235_235_245/0.6)] hover:text-white"
        >
          Cancel <kbd className="ml-1 font-sans text-[11px] text-white/35">Alt 0</kbd>
        </button>
      </div>
    </div>
  );
}

function GuideChip({ button, hint, done }: { button: GuideButton; hint: string | null; done?: boolean }) {
  const style =
    button.kind === "primary"
      ? "bg-white text-black hover:bg-white/90"
      : button.kind === "copy"
        ? "max-w-full truncate bg-white/[0.08] text-white/90 ring-1 ring-white/10 ring-inset hover:bg-white/[0.14]"
        : "bg-white/12 text-white/90 hover:bg-white/20";
  return (
    <button
      type="button"
      title={button.title}
      onClick={button.run}
      className={`chip rounded-full px-3 py-1.5 text-left text-[12.5px] font-medium ${style}`}
    >
      {done ? "Copied" : button.label}
      {hint && (
        <kbd
          className={`ml-1.5 font-sans text-[11px] ${button.kind === "primary" ? "text-black/40" : "text-white/35"}`}
        >
          {hint}
        </kbd>
      )}
    </button>
  );
}
