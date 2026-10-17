"use client";

// While Duskie waits on something you finish elsewhere (signing in, loading
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

/** One step at a time with a progress bar, the things to copy as mono rows,
 * then Next, Minimize and Skip. */
export function IslandGuide({ waiting, guide }: { waiting: Waiting; guide: Guide }) {
  const steps = waiting.steps?.length ? waiting.steps : [`Finish ${waiting.label}`];
  const [k, setK] = useState(0);
  const last = k >= steps.length - 1;
  const copies = guide.buttons.filter((b) => b.kind === "copy");
  return (
    <div className="rd-glance flex flex-col gap-2.5">
      <div className="rd-card-head">
        <p className="rd-eyebrow">
          {waiting.label}, step {k + 1} of {steps.length}
        </p>
        <p className="rd-title">{steps[k]}</p>
        <p className="rd-detail">
          {copies.length ? "Copy the command, run it, then continue." : "Finish this, then continue."}
        </p>
      </div>
      <div className="flex flex-col gap-1" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
        <div
          className="rd-steps"
          role="progressbar"
          aria-valuemin={1}
          aria-valuemax={steps.length}
          aria-valuenow={k + 1}
        >
          {steps.map((s, i) => (
            <i key={s} data-s={i < k ? "done" : i === k ? "now" : undefined} />
          ))}
        </div>
        {copies.map((b, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: labels can repeat and the list never reorders
          <div key={`${i}-${b.label}`} className="rd-list-row mg-row">
            <span className="t mono">{b.title ?? b.label}</span>
            <button type="button" className="chip-s" onClick={b.run}>
              {guide.copied === b.label ? "Copied" : "Copy"}
            </button>
          </div>
        ))}
        <div className="mt-1.5 flex items-center gap-1.5">
          <button type="button" className="rd-btn primary" onClick={() => (last ? minimizeWaiting(true) : setK(k + 1))}>
            {last ? "Done" : "Next"}
          </button>
          {waiting.again && (
            <button type="button" className="rd-btn ghost" onClick={waiting.again}>
              Open again
            </button>
          )}
          <button type="button" className="rd-btn ghost" onClick={() => minimizeWaiting(true)}>
            Minimize
          </button>
          <button type="button" className="rd-btn ghost ml-auto" onClick={cancelWaiting}>
            Skip
          </button>
        </div>
      </div>
    </div>
  );
}
