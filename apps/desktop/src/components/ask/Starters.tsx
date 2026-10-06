"use client";

// What Ask mode offers before you type: context chips and starters.

import { sendChat, setAsk, useSidekick } from "@/lib/store";
import type { AskContext, CalendarToday } from "@/lib/types";
import type { IconName } from "../Icon";
import { KeyHint } from "./parts";

export interface Command {
  id: string;
  label: string;
  hint?: string;
  icon: IconName;
  run: () => void;
  /** Keep Ask mode open after running. */
  stay?: boolean;
}

/** What goes with the question, as one quiet line: the app you were in
 * and what you copied. Click one to add it or leave it out. This PC only
 * sits at the end. */
export function ContextLine({ keys }: { keys: boolean }) {
  const ask = useSidekick((s) => s.ask);
  if (!ask) return null;
  const { context } = ask;
  const items: { id: string; label: string; on: boolean; title?: string | null; toggle: () => void }[] = [];
  if (context.app) {
    items.push({
      id: "app",
      label: context.app,
      on: ask.attachWindow,
      title: context.title,
      toggle: () => setAsk({ attachWindow: !ask.attachWindow }),
    });
  }
  if (!context.clipboardSecret && context.clipboardKind) {
    const preview = (context.clipboardPreview ?? "").replace(/\s+/g, " ").trim();
    items.push({
      id: "clip",
      label: `Copied: ${preview.length > 28 ? `${preview.slice(0, 26)}...` : preview || context.clipboardKind}`,
      on: ask.attachClip,
      title: context.clipboardPreview,
      toggle: () => setAsk({ attachClip: !ask.attachClip }),
    });
  }
  return (
    <div className="mt-1.5 flex items-center gap-2 text-[12px]" style={{ paddingLeft: 40 }}>
      <span className="flex min-w-0 flex-1 items-center gap-1 truncate">
        {items.map((it, n) => (
          <span key={it.id} className="flex min-w-0 items-center gap-1">
            {n > 0 && <span className="text-[rgb(235_235_245/0.25)]">·</span>}
            <button
              type="button"
              aria-pressed={it.on}
              title={`${it.on ? "Goes with your question" : "Add to your question"}${it.title ? `: ${it.title}` : ""}`}
              onClick={it.toggle}
              className={`chip group/ctx flex min-w-0 items-center gap-1 truncate rounded-md px-1 ${
                it.on
                  ? "text-[rgb(235_235_245/0.75)]"
                  : "text-[rgb(235_235_245/0.35)] hover:text-[rgb(235_235_245/0.6)]"
              }`}
            >
              {!it.on && <span aria-hidden="true">+</span>}
              <span className="truncate">{it.label}</span>
              {it.on && (
                <span aria-hidden="true" className="opacity-0 transition-opacity group-hover/ctx:opacity-60">
                  ×
                </span>
              )}
            </button>
          </span>
        ))}
        {context.clipboardSecret && (
          <span className="truncate text-[rgb(235_235_245/0.3)]">Clipboard hidden (looks like a secret)</span>
        )}
      </span>
      <span className="relative shrink-0">
        <button
          type="button"
          aria-pressed={ask.localOnly}
          title="Only use a model on this PC (Alt P)"
          onClick={() => setAsk({ localOnly: !ask.localOnly })}
          className={`chip rounded-full px-2 py-0.5 text-[11.5px] font-medium ${
            ask.localOnly ? "bg-white text-black" : "bg-white/[0.08] text-[rgb(235_235_245/0.55)] hover:bg-white/[0.14]"
          }`}
        >
          This PC only
        </button>
        <KeyHint show={keys} side="right">
          Alt P
        </KeyHint>
      </span>
    </div>
  );
}

/** A meeting starting within the hour, from today's "HH:MM" list. */
export function soonestMeeting(calendar: CalendarToday | null): { title: string; start: string } | null {
  const now = new Date();
  for (const m of calendar?.meetings ?? []) {
    const [h, min] = m.start.split(":").map(Number);
    const at = new Date(now);
    at.setHours(h ?? 0, min ?? 0, 0, 0);
    const mins = (at.getTime() - now.getTime()) / 60_000;
    if (mins >= -5 && mins <= 60) return m;
  }
  return null;
}

/**
 * What Ask offers before you type, from what you are doing: the error you
 * copied, the page you are on, the meeting coming up. Enter runs the first.
 */
export function contextStarters({
  context,
  page,
  meeting,
  focusInput,
}: {
  context: AskContext | null;
  page: string | null;
  meeting: { title: string; start: string } | null;
  focusInput: (prefix: string) => void;
}): Command[] {
  const out: Command[] = [];
  const clip = context?.clipboardSecret ? null : context?.clipboardKind;
  if (clip === "stack_trace") {
    out.push({
      id: "starter:error",
      label: "Explain the error I copied",
      hint: "Two ways to fix it",
      icon: "ask",
      run: () => sendChat("Explain the error I copied and give me the two most likely fixes.", { clipboard: true }),
      stay: true,
    });
  }
  if (page) {
    out.push({
      id: "starter:page",
      label: "Summarize this page",
      hint: "In three lines",
      icon: "ask",
      run: () => sendChat("Summarize this page in three short lines."),
      stay: true,
    });
  }
  if (meeting) {
    out.push({
      id: "starter:meeting",
      label: `Prepare for ${meeting.title}`,
      hint: `At ${meeting.start}`,
      icon: "ask",
      run: () =>
        sendChat(`Help me prepare for "${meeting.title}" at ${meeting.start}: related notes, files and open items.`),
      stay: true,
    });
  }
  out.push(
    {
      id: "starter:find",
      label: "Find a file...",
      hint: "Searches your PC",
      icon: "folder",
      run: () => focusInput("Find "),
      stay: true,
    },
    {
      id: "starter:today",
      label: "What did I work on today?",
      icon: "ask",
      run: () => sendChat("What did I work on today? Two lines."),
      stay: true,
    },
  );
  return out.slice(0, 3);
}
