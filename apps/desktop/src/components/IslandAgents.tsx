"use client";

// What hovering the island shows about agents and things that finished:
// a line per agent (the one waiting for you first, answered right there),
// and, after you were away, what finished or is waiting meanwhile.

import { useEffect } from "react";
import { create } from "zustand";
import { answerQuestion, type Session, useAgents } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import type { InboxStatus } from "@/lib/types";
import { limitNote } from "./agents/AgentsTab";
import { Icon } from "./Icon";
import { CardChips, CardHead, CardList, CardNote, type CardRow } from "./IslandCard";
import { RoundButton } from "./IslandGlance";

/** Away this long before the island shows what happened meanwhile. */
const AWAY_MS = 3 * 60_000;
const SEEN_KEY = "sidekick.seenAt";

function readSeen(): number {
  try {
    return Number(localStorage.getItem(SEEN_KEY)) || Date.now();
  } catch {
    return Date.now();
  }
}

/** When the user last looked at what Sidekick had for them. */
export const useSeen = create<{ at: number }>(() => ({ at: typeof window === "undefined" ? 0 : readSeen() }));

export function markSeen() {
  const at = Date.now();
  useSeen.setState({ at });
  try {
    localStorage.setItem(SEEN_KEY, String(at));
  } catch {
    // Not kept: the card may show again after a restart.
  }
}

/** Opens Ask on the Agents tab at one session. */
function openSession(id: string) {
  useAgents.setState({ tab: "agents", current: id });
  void api.askOpen();
}

function ago(ms: number, now: number): string {
  const min = Math.max(1, Math.round((now - ms) / 60_000));
  return min < 60 ? `${min} min` : `${Math.round(min / 60)} h`;
}

function clock(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Keys for the chips under a card: Enter and Alt + a letter or digit. */
function useKeys(keys: Record<string, (() => void) | undefined>) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const k = e.key === "Enter" ? "Enter" : e.altKey ? `Alt ${e.key.toUpperCase()}` : null;
      const run = k ? keys[k] : undefined;
      if (!run) return;
      e.preventDefault();
      run();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
}

/** Agents at work, one row each; the one waiting for you first, answered
 * right here. */
export function AgentsGlance({ sessions }: { sessions: Session[] }) {
  const now = useNow(1000);
  const active = [...sessions]
    .filter((s) => s.status === "working" || s.status === "waiting")
    .sort((a, b) => Number(!!b.question) - Number(!!a.question));
  const waiting = active.find((s) => s.question);
  const step = (s: Session) => {
    const last = [...s.entries].reverse().find((e) => e.kind === "step");
    return last?.kind === "step" ? last.step.label : "Working";
  };
  const wants = (s: Session) =>
    s.question ? `Wants to ${s.question.label.toLowerCase()}${s.question.detail ? ` ${s.question.detail}` : ""}` : "";
  const title = waiting
    ? `${waiting.agent} needs you`
    : active.length > 1
      ? `${active.length} agents working`
      : `${active[0]?.agent ?? "Agent"} is working`;
  const lead = waiting ?? active[0];
  const detail = lead ? `${lead.title} \u00b7 ${lead.project}` : "";
  const rows: CardRow[] = active.map((s) => ({
    key: s.id,
    dot: s.question ? "wait" : undefined,
    title: s.title,
    detail: s.question ? wants(s) : `${s.agent} \u00b7 ${step(s)}`,
    right: s.question ? "Waiting" : clock(now - s.turnAt),
    onClick: () => openSession(s.id),
  }));
  const allow = waiting ? () => answerQuestion(waiting.id, "allow") : undefined;
  const deny = waiting ? () => answerQuestion(waiting.id, "deny") : undefined;
  const open = () => openSession(lead?.id ?? "");
  useKeys({ Enter: allow, "Alt N": deny });
  const limited = sessions.find((x) => x.limit);
  return (
    <div className="flex flex-col">
      <CardHead
        title={title}
        detail={detail}
        right={
          <RoundButton label="Open Agents" onClick={open}>
            <Icon name="terminal" size={15} />
          </RoundButton>
        }
      />
      <CardList rows={rows} />
      {limited?.limit && <CardNote>{limitNote(limited.agent, limited.limit)}</CardNote>}
      {waiting && allow && deny ? (
        <CardChips
          options={[
            {
              label: `Allow ${waiting.question?.detail || waiting.question?.label.toLowerCase() || ""}`,
              keys: "Enter",
              run: allow,
            },
            { label: "Deny", keys: "Alt N", run: deny },
          ]}
        />
      ) : null}
    </div>
  );
}

/** What finished or started waiting since the user last looked. */
export function useAway(): {
  rows: CardRow[];
  reviewId: string | null;
  waitingId: string | null;
  agent: string | null;
} {
  const seen = useSeen((s) => s.at);
  const sessions = useAgents((s) => s.sessions);
  const { data: inbox } = useCached<InboxStatus>("inbox-status", api.notificationsStatus);
  const now = useNow(60_000);
  const rows: CardRow[] = [];
  let reviewId: string | null = null;
  let waitingId: string | null = null;
  let agent: string | null = null;
  if (now - seen < AWAY_MS) return { rows, reviewId, waitingId, agent };
  for (const s of sessions) {
    const endedAt = s.tookMs !== null ? s.turnAt + s.tookMs : null;
    if (s.question) {
      waitingId ??= s.id;
      agent ??= s.agent;
      rows.push({
        key: s.id,
        dot: "wait",
        title: `${s.agent} is waiting`,
        detail: `${s.title} · wants to ${s.question.label.toLowerCase()}`,
        right: ago(s.turnAt, now),
        onClick: () => openSession(s.id),
      });
    } else if (endedAt && endedAt > seen && (s.status === "idle" || s.status === "ended")) {
      if (s.reviewable) reviewId ??= s.id;
      rows.push({
        key: s.id,
        dot: "done",
        title: `${s.agent} finished`,
        detail: `${s.title}${s.changes ? ` · ${s.changes} ${s.changes === 1 ? "change" : "changes"} to review` : ""}`,
        right: ago(endedAt, now),
        onClick: () => openSession(s.id),
      });
    }
  }
  for (const n of inbox?.items ?? []) {
    const at = Date.parse(n.ts);
    if (rows.length >= 5 || !(at > seen) || (n.level !== "now" && n.level !== "soon")) continue;
    rows.push({ key: `n${n.id}`, title: n.title || n.app, detail: n.app, right: ago(at, now) });
  }
  return { rows, reviewId, waitingId, agent };
}

/** "While you were away": what finished or waits, with the next step for each. */
export function AwayCard() {
  const { rows, reviewId, waitingId, agent } = useAway();
  const review = reviewId ? () => openSession(reviewId) : undefined;
  const answer = waitingId ? () => openSession(waitingId) : undefined;
  useKeys({ "Alt 1": review ?? answer, "Alt 2": review ? answer : undefined, "Alt X": markSeen });
  const done = rows.filter((r) => r.dot === "done").length;
  const waits = rows.filter((r) => r.dot === "wait").length;
  const parts = [
    done && `${done} finished`,
    waits && `${waits} waiting for you`,
    rows.length - done - waits && `${rows.length - done - waits} new`,
  ].filter(Boolean);
  const options = [
    review && { label: "Review the change", keys: "Alt 1", run: review },
    answer && { label: `Answer ${agent}`, keys: review ? "Alt 2" : "Alt 1", run: answer },
  ].filter((o): o is { label: string; keys: string; run: () => void } => !!o);
  return (
    <div className="flex flex-col">
      <CardHead title="While you were away" detail={parts.join(" \u00b7 ")} />
      <CardList
        label="Since you last looked"
        action={
          <button type="button" onClick={markSeen} className="chip hover:text-white">
            Clear all{" "}
            <kbd className="ml-1 font-sans text-[11px] text-white/45">
              <i className="alt-pre">Alt </i>X
            </kbd>
          </button>
        }
        rows={rows}
      />
      {options.length > 0 && <CardChips options={options} />}
    </div>
  );
}
