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

interface Row {
  key: string;
  dot: "wait" | "done" | "work";
  title: string;
  sub: string;
  right: string;
  open?: () => void;
}

function Rows({ rows }: { rows: Row[] }) {
  return (
    <div className="ak-bglist">
      {rows.map((r, i) => (
        <button
          key={r.key}
          type="button"
          onClick={r.open}
          className={`ak-bgr chip ${i === 0 && r.dot === "wait" ? "first" : ""}`}
        >
          <span className="ak-sd" data-s={r.dot === "wait" ? "waiting" : r.dot === "done" ? "idle" : "working"} />
          <span className="min-w-0 text-left">
            <b className="block truncate text-[13.5px] font-semibold">{r.title}</b>
            <em className="block truncate text-[11.5px] text-[rgb(235_235_245/0.36)] not-italic">{r.sub}</em>
          </span>
          <span className="whitespace-nowrap text-right text-[12px] text-[rgb(235_235_245/0.6)]">{r.right}</span>
        </button>
      ))}
    </div>
  );
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

/** Agents at work, one line each; the one waiting for you first. */
export function AgentsGlance({ sessions }: { sessions: Session[] }) {
  const now = useNow(1000);
  const active = [...sessions]
    .filter((s) => s.status === "working" || s.status === "waiting")
    .sort((a, b) => Number(b.status === "waiting") - Number(a.status === "waiting"));
  const waiting = active.find((s) => s.question);
  const step = (s: Session) => {
    const last = [...s.entries].reverse().find((e) => e.kind === "step");
    return last?.kind === "step" ? last.step.label : "Working";
  };
  const rows: Row[] = active.map((s) => ({
    key: s.id,
    dot: s.question ? "wait" : "work",
    title: s.title,
    sub: s.question
      ? `Wants to ${s.question.label.toLowerCase()}${s.question.detail ? ` ${s.question.detail}` : ""}`
      : `${s.project} · ${step(s)}`,
    right: s.question ? "Waiting" : clock(now - s.turnAt),
    open: () => openSession(s.id),
  }));
  const allow = waiting ? () => answerQuestion(waiting.id, "allow") : undefined;
  useKeys({ Enter: allow });
  return (
    <div className="ak grid gap-2">
      <p className="text-[15px] font-semibold">Agents</p>
      <Rows rows={rows} />
      <div className="ak-chips" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
        {waiting?.question && (
          <button type="button" onClick={allow} className="ak-chip primary chip max-w-60">
            <span className="truncate">Allow {waiting.question.detail || waiting.question.label.toLowerCase()}</span>
            <kbd>Enter</kbd>
          </button>
        )}
        <button type="button" onClick={() => openSession(active[0]?.id ?? "")} className="ak-chip chip">
          Open Agents
        </button>
      </div>
    </div>
  );
}

/** What finished or started waiting since the user last looked. */
export function useAway(): { rows: Row[]; reviewId: string | null; waitingId: string | null; agent: string | null } {
  const seen = useSeen((s) => s.at);
  const sessions = useAgents((s) => s.sessions);
  const { data: inbox } = useCached<InboxStatus>("inbox-status", api.notificationsStatus);
  const now = useNow(60_000);
  const rows: Row[] = [];
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
        sub: `${s.title} · wants to ${s.question.label.toLowerCase()}`,
        right: ago(s.turnAt, now),
        open: () => openSession(s.id),
      });
    } else if (endedAt && endedAt > seen && (s.status === "idle" || s.status === "ended")) {
      if (s.reviewable) reviewId ??= s.id;
      rows.push({
        key: s.id,
        dot: "done",
        title: `${s.agent} finished`,
        sub: `${s.title}${s.changes ? ` · ${s.changes} ${s.changes === 1 ? "change" : "changes"} to review` : ""}`,
        right: ago(endedAt, now),
        open: () => openSession(s.id),
      });
    }
  }
  for (const n of inbox?.items ?? []) {
    const at = Date.parse(n.ts);
    if (rows.length >= 5 || !(at > seen) || (n.level !== "now" && n.level !== "soon")) continue;
    rows.push({ key: `n${n.id}`, dot: "work", title: n.title || n.app, sub: n.app, right: ago(at, now) });
  }
  return { rows, reviewId, waitingId, agent };
}

/** "While you were away": what finished or waits, with the next step for each. */
export function AwayCard() {
  const { rows, reviewId, waitingId, agent } = useAway();
  const review = reviewId ? () => openSession(reviewId) : undefined;
  const answer = waitingId ? () => openSession(waitingId) : undefined;
  useKeys({ "Alt 1": review ?? answer, "Alt 2": review ? answer : undefined, "Alt X": markSeen });
  return (
    <div className="ak grid gap-2">
      <p className="flex items-center gap-2 text-[15px] font-semibold">
        While you were away
        <span className="ak-model ml-auto">{rows.length} new</span>
      </p>
      <Rows rows={rows} />
      <div className="ak-chips" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
        {review && (
          <button type="button" onClick={review} className="ak-chip primary chip">
            Review the change <kbd>Alt 1</kbd>
          </button>
        )}
        {answer && (
          <button type="button" onClick={answer} className={`ak-chip chip ${review ? "" : "primary"}`}>
            Answer {agent} <kbd>{review ? "Alt 2" : "Alt 1"}</kbd>
          </button>
        )}
        <button type="button" onClick={markSeen} className="ak-chip chip">
          Clear all <kbd>Alt X</kbd>
        </button>
      </div>
    </div>
  );
}
