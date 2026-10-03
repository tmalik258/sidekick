"use client";

// What Ask mode offers before you type: context chips and starters.

import { sendChat, setAsk, useSidekick } from "@/lib/store";
import type { AskContext, CalendarToday } from "@/lib/types";
import type { IconName } from "../Icon";
import { Chip } from "./parts";

export interface Command {
  id: string;
  label: string;
  hint?: string;
  icon: IconName;
  run: () => void;
  /** Keep Ask mode open after running. */
  stay?: boolean;
}

export function ContextChips() {
  const ask = useSidekick((s) => s.ask);
  if (!ask) return null;
  const { context } = ask;
  return (
    <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
      {context.app && (
        <Chip on={ask.attachWindow} onClick={() => setAsk({ attachWindow: !ask.attachWindow })} title={context.title}>
          {context.app}
        </Chip>
      )}
      {context.clipboardSecret ? (
        <span className="rounded-full px-2.5 py-1 text-[11.5px] text-[rgb(235_235_245/0.35)]">
          Clipboard hidden (looks like a secret)
        </span>
      ) : (
        context.clipboardKind && (
          <Chip
            on={ask.attachClip}
            onClick={() => setAsk({ attachClip: !ask.attachClip })}
            title={context.clipboardPreview}
          >
            Clipboard: {context.clipboardKind.replace("_", " ")}
          </Chip>
        )
      )}
      <Chip
        on={ask.localOnly}
        onClick={() => setAsk({ localOnly: !ask.localOnly })}
        title="Only use a model on this PC"
      >
        This PC only
      </Chip>
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
