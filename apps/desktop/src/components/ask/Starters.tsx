"use client";

// What Ask mode offers before you type: context chips and starters.

import { sendChat, setAsk, useDuskie } from "@/lib/store";
import type { AskContext, CalendarToday } from "@/lib/types";
import { Ic, type IcName, Icon } from "../Icon";
import { Tip } from "../Tip";
import { pendingCount, useAltDigits } from "./Proposals";
import { KeyHint } from "./parts";

export interface Command {
  id: string;
  label: string;
  hint?: string;
  icon: IcName;
  run: () => void;
  /** Keep Ask mode open after running. */
  stay?: boolean;
}

/** What goes with the question, as one quiet line under it: the app you
 * were in and what you copied. Click one to leave it out (or back in).
 * This PC only sits at the end. Hold Alt for 1 / 2 on the chips. */
export function ContextLine({ keys }: { keys: boolean }) {
  // Clicking a chip leaves focus in the input; otherwise the next Enter
  // would press the chip again and switch it back.
  const keepFocus = (e: { preventDefault: () => void }) => e.preventDefault();
  const ask = useDuskie((s) => s.ask);
  const offline = useDuskie((s) => !s.online);
  // Answer actions already own Alt 1…n; leave chips free when those are waiting.
  const proposalDigits = useDuskie((s) => pendingCount(s.turns.at(-1)?.proposals));
  const { context } = ask ?? { context: null };
  const items: {
    id: string;
    label: string;
    on: boolean;
    title?: string | null;
    tip: string;
    toggle: () => void;
  }[] = [];
  if (ask && context?.app) {
    items.push({
      id: "app",
      label: context.app,
      on: ask.attachWindow,
      title: context.title,
      tip: ask.attachWindow ? `Exclude what's visible in ${context.app}` : `Include what's visible in ${context.app}`,
      toggle: () => setAsk({ attachWindow: !ask.attachWindow }),
    });
  }
  if (ask && context && !context.clipboardSecret && context.clipboardKind) {
    // What it is ("stack trace", "link"); the text itself shows on hover.
    items.push({
      id: "clip",
      label: `Copied: ${context.clipboardKind.replace(/_/g, " ")}`,
      on: ask.attachClip,
      title: context.clipboardPreview,
      tip: ask.attachClip ? "Exclude clipboard" : "Include clipboard",
      toggle: () => setAsk({ attachClip: !ask.attachClip }),
    });
  }
  useAltDigits(proposalDigits > 0 ? 0 : Math.min(items.length, 9), 0, (n) => {
    items[n]?.toggle();
  });
  if (!ask || !context) return null;
  return (
    <div className="ak-ctx">
      {offline && <span className="ak-offl">Offline</span>}
      {items.map((it, i) => (
        <span key={it.id} className={`relative flex min-w-0 items-center ${it.id === "clip" ? "shrink" : "shrink-0"}`}>
          <Tip label={`${it.tip}${it.title ? ` · ${it.title}` : ""} · Alt ${i + 1}`}>
            <button
              type="button"
              aria-pressed={it.on}
              onMouseDown={keepFocus}
              onClick={it.toggle}
              className={`ak-ctx-i chip ${it.id === "clip" ? "cpy" : ""}`}
            >
              <Ic name={it.id === "clip" ? "copy" : "file"} size={12} />
              <span className="truncate">{it.label}</span>
            </button>
          </Tip>
          {proposalDigits === 0 && i < 9 && (
            <KeyHint show={keys} side="left">
              {`Alt ${i + 1}`}
            </KeyHint>
          )}
        </span>
      ))}
      {context.clipboardSecret && <span className="ak-ctx-i cpy">Clipboard hidden (looks like a secret)</span>}
      <span className="relative ml-auto shrink-0">
        <Tip label="This PC only (Alt P)">
          <button
            type="button"
            aria-pressed={ask.localOnly}
            aria-label="This PC only"
            onMouseDown={keepFocus}
            onClick={() => setAsk({ localOnly: !ask.localOnly })}
            className="ak-pc chip"
          >
            <Ic name={ask.localOnly ? "check" : "lock"} size={10} />
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

const MAIL = /outlook|gmail|thunderbird|mail/i;
const MEETING = /teams|zoom|meet|onenote|notion|obsidian|notes/i;
const CODE = /code|cursor|visual studio|idea|pycharm|webstorm|terminal/i;

/** Everyday work help from the app you are in and the time of day:
 * inbox triage and replies in your tone, notes to tasks, standup and
 * weekly status, handing work to a coding agent. */
export function roleStarters(
  context: AskContext | null,
  clip: string | null | undefined,
  focusInput: (prefix: string) => void,
  now = new Date(),
  coder = true,
): Command[] {
  const out: Command[] = [];
  const where = `${context?.app ?? ""} ${context?.title ?? ""}`;
  if (MAIL.test(where)) {
    out.push(
      {
        id: "starter:reply",
        label: "Draft a reply in my tone",
        hint: "From this email",
        icon: "send",
        run: () => sendChat("Draft a reply to this email in my usual tone. Short, friendly, no filler."),
        stay: true,
      },
      {
        id: "starter:triage",
        label: "Triage my inbox",
        hint: "What needs me today",
        icon: "box",
        run: () => sendChat("Triage my inbox: what needs a reply today, what can wait, what to archive."),
        stay: true,
      },
    );
  }
  if (clip && MEETING.test(where)) {
    out.push({
      id: "starter:tasks",
      label: "Turn my notes into tasks",
      hint: "Owner and due date each",
      icon: "check",
      run: () =>
        sendChat("Turn the notes I copied into a task list with an owner and due date each.", { clipboard: true }),
      stay: true,
    });
  }
  if (coder && CODE.test(where)) {
    out.push({
      id: "starter:delegate",
      label: "Hand this to a coding agent",
      hint: "Say what to do",
      icon: "pr",
      run: () => focusInput("Ask the coding agent to "),
      stay: true,
    });
  }
  const hour = now.getHours();
  if (now.getDay() === 5 && hour >= 13) {
    out.push({
      id: "starter:weekly",
      label: "Draft my weekly status",
      hint: "Done, next, blocked",
      icon: "send",
      run: () => sendChat("Draft my weekly status from what I worked on this week: done, next, blocked."),
      stay: true,
    });
  } else if (hour < 11 && now.getDay() >= 1 && now.getDay() <= 5) {
    out.push({
      id: "starter:standup",
      label: "Draft my standup",
      hint: "Yesterday, today, blockers",
      icon: "send",
      run: () => sendChat("Draft my standup from what I worked on yesterday: yesterday, today, blockers. Three lines."),
      stay: true,
    });
  }
  return out;
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
  coder = true,
}: {
  context: AskContext | null;
  page: string | null;
  meeting: { title: string; start: string } | null;
  focusInput: (prefix: string) => void;
  /** "Do you work with code?" No: starters about mail, notes and the day. */
  coder?: boolean;
}): Command[] {
  const out: Command[] = [];
  const clip = context?.clipboardSecret ? null : context?.clipboardKind;
  if (!coder && clip) {
    out.push({
      id: "starter:clip",
      label: "Summarize what I copied",
      hint: "Three lines",
      icon: "copy",
      run: () => sendChat("Summarize what I copied in three short lines.", { clipboard: true }),
      stay: true,
    });
  }
  if (coder && clip === "stack_trace") {
    out.push({
      id: "starter:error",
      label: "Explain the error I copied",
      hint: "Two ways to fix it",
      icon: "warn",
      run: () => sendChat("Explain the error I copied and give me the two most likely fixes.", { clipboard: true }),
      stay: true,
    });
  }
  if (page) {
    out.push({
      id: "starter:page",
      label: "Summarize this page",
      hint: "In three lines",
      icon: "open",
      run: () => sendChat("Summarize this page in three short lines."),
      stay: true,
    });
  }
  if (context?.app && !page) {
    const app = context.app;
    out.push({
      id: "starter:howto",
      label: `How do I use ${app}?`,
      hint: "Three steps, shows where",
      icon: "spark",
      run: () =>
        sendChat(`How do I use ${app} for what I am doing? Three short steps, and point at the first control.`),
      stay: true,
    });
  }
  if (meeting) {
    out.push({
      id: "starter:meeting",
      label: `Prepare for ${meeting.title}`,
      hint: `At ${meeting.start}`,
      icon: "clock",
      run: () =>
        sendChat(`Help me prepare for "${meeting.title}" at ${meeting.start}: related notes, files and open items.`),
      stay: true,
    });
  }
  out.push(...roleStarters(context, clip, focusInput, new Date(), coder));
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
      icon: "clock",
      run: () => sendChat("What did I work on today? Two lines."),
      stay: true,
    },
  );
  return out.slice(0, 3);
}
