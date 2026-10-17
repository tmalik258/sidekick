"use client";

// What hovering the island shows about agents and things that finished:
// a line per agent (the one waiting for you first, answered right there),
// and, after you were away, what finished or is waiting meanwhile.

import { useEffect } from "react";
import { create } from "zustand";
import { answerQuestion, interruptSession, type Session, useAgents } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import type { InboxStatus } from "@/lib/types";
import { limitNote } from "./agents/AgentsTab";
import { CardChips, CardHead, CardList, CardNote, type CardRow } from "./IslandCard";

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

/** When the user last looked at what Duskie had for them. */
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

function liveSessions(sessions: Session[]) {
  return sessions.filter((s) => s.status === "working" || s.status === "waiting");
}

function agentRow(s: Session, now: number): CardRow {
  const last = [...s.entries].reverse().find((e) => e.kind === "step");
  const step = last?.kind === "step" ? last.step.label : "Working";
  return {
    key: s.id,
    dot: s.question ? ("wait" as const) : undefined,
    title: s.title,
    detail: s.question ? `Wants to ${s.question.label.toLowerCase()}` : `${s.agent} \u00b7 ${step}`,
    right: s.question ? "Waiting" : clock(now - s.turnAt),
    onClick: () => openSession(s.id),
  };
}

/** Agents on the glance: permission first, otherwise the working list. */
export function NeedsYouSection({ sessions }: { sessions: Session[] }) {
  const now = useNow(1000);
  const live = liveSessions(sessions);
  const waiting = sessions.find((s) => s.question);
  const question = waiting?.question;
  const allow = question ? () => answerQuestion(waiting.id, "allow") : undefined;
  const always = question ? () => answerQuestion(waiting.id, "always") : undefined;
  const deny = question ? () => answerQuestion(waiting.id, "deny") : undefined;
  const lead = waiting ?? live[0];
  const open = lead ? () => openSession(lead.id) : undefined;
  const cancelAll = () => {
    for (const s of live) interruptSession(s.id);
  };
  useKeys({
    Enter: allow ?? open,
    "Alt Y": always,
    "Alt N": deny,
    "Alt O": open,
    "Alt X": live.length > 0 && !question ? cancelAll : undefined,
  });
  if (waiting && question) {
    const others = live.filter((s) => s !== waiting);
    const othersAsk = others.some((s) => s.question);
    const limited = sessions.find((x) => x.limit);
    const rows: CardRow[] = [
      {
        key: waiting.id,
        dot: "wait",
        title: waiting.title,
        detail: `Wants to ${question.label.toLowerCase()}${question.detail ? ` ${question.detail}` : ""}`,
        right: "Waiting",
        onClick: () => openSession(waiting.id),
      },
      ...others.map((s) => agentRow(s, now)),
    ];
    return (
      <div className="mt-3 flex flex-col">
        <CardHead title={`${waiting.agent} needs you`} detail={`${waiting.title} \u00b7 ${waiting.project}`} />
        <CardList rows={rows} />
        {limited?.limit && <CardNote>{limitNote(limited.agent, limited.limit)}</CardNote>}
        {others.length > 0 && (
          <p
            className="px-3 pt-1 text-[12px] text-[rgb(235_235_245/0.45)]"
            style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}
          >
            {others.length} more {othersAsk ? "running" : "working · nothing else needs you"}
          </p>
        )}
        <CardChips
          options={[
            {
              label: `Allow ${question.detail || question.label.toLowerCase()}`,
              keys: "Enter",
              run: () => answerQuestion(waiting.id, "allow"),
            },
            { label: "Allow this session", keys: "Alt Y", run: () => answerQuestion(waiting.id, "always") },
            { label: "Deny", keys: "Alt N", run: () => answerQuestion(waiting.id, "deny") },
            { label: "Open in Agents", keys: "Alt O", run: () => openSession(waiting.id) },
          ]}
        />
      </div>
    );
  }
  const first = live[0];
  if (!first) return null;
  const limited = live.find((x) => x.limit);
  const title = live.length === 1 ? `${first.agent} is working` : `${live.length} agents working`;
  const detail =
    live.length === 1 ? `${first.title} \u00b7 ${first.project}` : live.map((s) => s.agent).join(" \u00b7 ");
  return (
    <div className="mt-3 flex flex-col">
      <CardHead title={title} detail={detail} />
      <CardList label="Running" rows={live.map((s) => agentRow(s, now))} />
      {limited?.limit && <CardNote>{limitNote(limited.agent, limited.limit)}</CardNote>}
      <CardChips
        options={[
          {
            label: live.length === 1 ? "Open in Agents" : "Open Agents",
            keys: "Enter",
            run: () => openSession(first.id),
          },
          { label: live.length === 1 ? "Cancel" : "Cancel all", keys: "Alt X", run: cancelAll },
        ]}
      />
    </div>
  );
}

/** @deprecated Prefer NeedsYouSection inside the unified glance. */
export function AgentsGlance({ sessions }: { sessions: Session[] }) {
  return <NeedsYouSection sessions={sessions} />;
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
      if (s.reviewable && s.changes > 0) reviewId ??= s.id;
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

/** "While you were away", glance section; null when nothing to report. */
export function AwaySection() {
  const { rows, reviewId, waitingId, agent } = useAway();
  const review = reviewId ? () => openSession(reviewId) : undefined;
  const answer = waitingId ? () => openSession(waitingId) : undefined;
  useKeys(
    rows.length > 0 ? { "Alt 1": review ?? answer, "Alt 2": review ? answer : undefined, "Alt X": markSeen } : {},
  );
  if (rows.length === 0) return null;
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
    <div className="mt-3 flex flex-col">
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

/** @deprecated Prefer AwaySection inside the unified glance. */
export function AwayCard() {
  return <AwaySection />;
}
