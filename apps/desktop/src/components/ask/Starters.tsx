"use client";

// What Ask mode offers before you type: context chips and starters.

import { sendChat, setAsk, useSidekick } from "@/lib/store";
import type { AskContext, CalendarToday } from "@/lib/types";
import { Icon, type IconName } from "../Icon";
import { Tip } from "../Tip";
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

/** What goes with the question, as one quiet line under it: the app you
 * were in and what you copied. Click one to leave it out (or back in).
 * This PC only sits at the end. */
export function ContextLine({ keys }: { keys: boolean }) {
  // Clicking a chip leaves focus in the input; otherwise the next Enter
  // would press the chip again and switch it back.
  const keepFocus = (e: { preventDefault: () => void }) => e.preventDefault();
  const ask = useSidekick((s) => s.ask);
  const offline = useSidekick((s) => !s.online);
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
    // What it is ("stack trace", "link"); the text itself shows on hover.
    items.push({
      id: "clip",
      label: `Copied: ${context.clipboardKind.replace(/_/g, " ")}`,
      on: ask.attachClip,
      title: context.clipboardPreview,
      toggle: () => setAsk({ attachClip: !ask.attachClip }),
    });
  }
  return (
    <div className="ak-ctx">
      {offline && <span className="ak-offl">Offline</span>}
      {items.map((it) => (
        <span key={it.id} className={`flex min-w-0 items-center ${it.id === "clip" ? "shrink" : "shrink-0"}`}>
          <Tip label={`${it.on ? "Click to exclude" : "Click to include"}${it.title ? ` · ${it.title}` : ""}`}>
            <button
              type="button"
              aria-pressed={it.on}
              onMouseDown={keepFocus}
              onClick={it.toggle}
              className={`ak-ctx-i chip ${it.id === "clip" ? "cpy" : ""}`}
            >
              <Icon name={it.on ? "check" : "plus"} size={10} />
              <span className="truncate">{it.label}</span>
            </button>
          </Tip>
        </span>
      ))}
      {context.clipboardSecret && <span className="ak-ctx-i cpy">Clipboard hidden (looks like a secret)</span>}
      <span className="relative ml-auto shrink-0">
        <Tip label="This PC only (Alt P)">
          <button
            type="button"
            aria-pressed={ask.localOnly}
            onMouseDown={keepFocus}
            onClick={() => setAsk({ localOnly: !ask.localOnly })}
            className="ak-pc chip"
          >
            <Icon name={ask.localOnly ? "check" : "lock"} size={10} />
            {ask.localOnly && "This PC only"}
          </button>
        </Tip>
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
      label: "Find a file",
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
