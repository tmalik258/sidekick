// Agent sessions in the island: Claude Code and Codex working in a project.
// Rust streams each step as `agent://event`; this keeps one timeline per
// session for the Agents tab.

import { create } from "zustand";
import { api, EVENTS, listen } from "./bridge";
import { mask } from "./mask";
import type { AgentMode, AgentStarted } from "./types";

export interface Step {
  id: string;
  tool: string;
  label: string;
  detail: string;
  state: "running" | "done" | "failed";
  /** The end of a command's output, folded under the step. */
  output?: string;
}

export interface PlanItem {
  text: string;
  status: "pending" | "in_progress" | "completed";
}

/** One entry in a session's timeline: something said, or a step taken. */
export type Entry = { kind: "you"; text: string } | { kind: "text"; text: string } | { kind: "step"; step: Step };

export interface Session extends AgentStarted {
  mode: AgentMode;
  title: string;
  status: "working" | "waiting" | "idle" | "ended" | "failed";
  entries: Entry[];
  plan: PlanItem[];
  usage: { used: number; window: number } | null;
  /** The plan's usage limit, when the agent says it is close or reached. */
  limit?: { status: string; window: string; resetsAt: number | null; used: number | null } | null;
  /** "Not now" on the compact offer, until the context is compacted. */
  compactDismissed?: boolean;
  /** A permission question waiting for the user. */
  question: { id: string; label: string; detail: string } | null;
  error: string | null;
  startedAt: number;
  /** When the current turn began, and how long the last one took. */
  turnAt: number;
  tookMs: number | null;
  /** Files changed when it ended, from Rust. */
  changes: number;
  /** Said before the next message after a rewind, so the agent knows. */
  note?: string | null;
  /** Cut off by a restart: nothing runs until Resume. */
  restored?: boolean;
}

/** Half the context used: Claude Code works best compacted from here. */
export const COMPACT_AT = 0.5;

/** "Not now" on the compact offer. */
export function dismissCompact(id: string) {
  update(id, (s) => ({ ...s, compactDismissed: true }));
}

export type AskTab = "ask" | "agents" | "history";

interface AgentsState {
  /** Ask's tab: quick questions, agent sessions, or history. */
  tab: AskTab;
  sessions: Session[];
  /** The session shown in the Agents tab; null shows New. */
  current: string | null;
  /** The project New starts in, when something picked it. */
  draftPath?: string | null;
  /** What New's box starts with ("Ask an agent to fix it"). */
  draftPrompt?: string | null;
}

/** Sessions are kept in this window's storage, so the timeline comes back
 * after a restart; Rust keeps what Review, Undo and Resume need. */
const STORE_KEY = "sidekick.agents";

function loadSessions(): Session[] {
  try {
    const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(STORE_KEY);
    const list = raw ? (JSON.parse(raw) as Session[]) : [];
    // Whatever was running stopped with Sidekick; each can be resumed.
    return list.map((s) => ({
      ...s,
      status: s.status === "working" || s.status === "waiting" ? "ended" : s.status,
      question: null,
      restored: true,
    }));
  } catch {
    return [];
  }
}

export const useAgents = create<AgentsState>(() => ({ tab: "ask", sessions: loadSessions(), current: null }));

let saveTimer: ReturnType<typeof setTimeout> | null = null;
useAgents.subscribe((st) => {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    try {
      localStorage.setItem(
        STORE_KEY,
        JSON.stringify(st.sessions.slice(0, 20), (_k, v: unknown) => (typeof v === "string" ? mask(v) : v)),
      );
    } catch {
      // Storage full or blocked: the timeline just does not survive a restart.
    }
  }, 400);
});

export const setTab = (tab: AskTab) => useAgents.setState({ tab });

/** Opens New in Agents for one project ("Start an agent here"). */
export const startHere = (path: string, prompt?: string | null) =>
  useAgents.setState({ tab: "agents", current: null, draftPath: path, draftPrompt: prompt ?? null });

function update(id: string, fn: (s: Session) => Session) {
  useAgents.setState((st) => ({ sessions: st.sessions.map((s) => (s.id === id ? fn(s) : s)) }));
}

export async function startSession(agent: string, path: string, prompt: string, mode: AgentMode): Promise<string> {
  const started = await api.agentStart(agent, path, prompt, mode);
  addSession(started, prompt, mode, prompt);
  return started.id;
}

function addSession(started: AgentStarted, title: string, mode: AgentMode, first: string) {
  const session: Session = {
    ...started,
    mode,
    title: title.length > 60 ? `${title.slice(0, 57)}...` : title,
    status: "working",
    entries: [{ kind: "you", text: first }],
    plan: [],
    usage: null,
    question: null,
    error: null,
    startedAt: Date.now(),
    turnAt: Date.now(),
    tookMs: null,
    changes: 0,
  };
  useAgents.setState((st) => ({ sessions: [session, ...st.sessions], current: started.id, tab: "agents" }));
}

/** Continues an Ask conversation in Claude Code or Codex, here in the island. */
export async function handOff(messages: { role: "user" | "assistant"; content: string }[], reason: string | null) {
  const started = await api.agentHandoff(messages, reason);
  const first = messages.find((m) => m.role === "user")?.content ?? "Continue the conversation";
  addSession(started, `Continue: ${first}`, "ask", first);
  return started.id;
}

export function sendToSession(id: string, text: string) {
  const note = useAgents.getState().sessions.find((s) => s.id === id)?.note;
  update(id, (s) => ({
    ...s,
    entries: [...s.entries, { kind: "you", text }],
    status: "working",
    turnAt: Date.now(),
    tookMs: null,
    note: null,
    error: null,
  }));
  void api
    .agentSend(id, note ? `${note}\n\n${text}` : text)
    .catch((e: unknown) => update(id, (s) => ({ ...s, status: "failed", error: String(e) })));
}

/** Starts the agent again in its own earlier session, after it ended or
 * Sidekick restarted. */
export async function resumeSession(id: string) {
  await api.agentResume(id);
  update(id, (s) => ({ ...s, status: "idle", restored: false, error: null }));
}

/** Puts the code back to before the user's message at `entry` (an index in
 * entries) and drops what came after; the next message tells the agent. */
export async function rewindSession(id: string, entry: number): Promise<number> {
  const s = useAgents.getState().sessions.find((x) => x.id === id);
  if (!s) return 0;
  const index = s.entries.slice(0, entry).filter((e) => e.kind === "you").length;
  const said = s.entries[entry]?.kind === "you" ? (s.entries[entry] as { text: string }).text : "";
  const n = await api.agentRewind(id, index);
  update(id, (x) => ({
    ...x,
    entries: x.entries.slice(0, entry),
    plan: [],
    note: `(I rewound this conversation to before my message "${said.slice(0, 80)}". The file changes made after it were undone. Ignore everything after that point and work from here.)`,
  }));
  return n;
}

/** Which user message an entry is, counting from 0, for Rewind. */
export function messageIndex(s: Session, entry: number): number {
  return s.entries.slice(0, entry).filter((e) => e.kind === "you").length;
}

export function answerQuestion(id: string, answer: "allow" | "always" | "deny") {
  const s = useAgents.getState().sessions.find((x) => x.id === id);
  if (!s?.question) return;
  void api.agentAnswer(s.question.id, answer);
  update(id, (x) => ({ ...x, question: null, status: "working" }));
}

export function closeSession(id: string) {
  void api.agentClose(id);
  useAgents.setState((st) => ({
    sessions: st.sessions.filter((s) => s.id !== id),
    current: st.current === id ? null : st.current,
  }));
}

/** How many sessions are working or waiting for an answer. */
export function activeCount(sessions: Session[]): number {
  return sessions.filter((s) => s.status === "working" || s.status === "waiting").length;
}

function onEvent(e: { session: string; kind: string } & Record<string, unknown>) {
  const id = e.session;
  switch (e.kind) {
    case "working":
      return update(id, (s) => ({ ...s, status: s.question ? "waiting" : "working" }));
    case "text":
      return update(id, (s) => {
        const last = s.entries.at(-1);
        const text = String(e.text ?? "");
        const entries: Entry[] =
          last?.kind === "text"
            ? [...s.entries.slice(0, -1), { kind: "text", text: mask(last.text + text) }]
            : [...s.entries, { kind: "text", text: mask(text) }];
        return { ...s, entries };
      });
    case "step":
      return update(id, (s) => {
        const at = s.entries.findIndex((x) => x.kind === "step" && x.step.id === e.id);
        if (at >= 0) {
          const entries = [...s.entries];
          const old = entries[at] as { kind: "step"; step: Step };
          entries[at] = {
            kind: "step",
            step: {
              ...old.step,
              state: e.state as Step["state"],
              label: (e.label as string) || old.step.label,
              detail: (e.detail as string) || old.step.detail,
              output: (e.output as string) || old.step.output,
            },
          };
          return { ...s, entries };
        }
        const step: Step = {
          id: String(e.id ?? crypto.randomUUID()),
          tool: String(e.tool ?? ""),
          label: String(e.label ?? ""),
          detail: String(e.detail ?? ""),
          state: (e.state as Step["state"]) ?? "running",
        };
        return { ...s, entries: [...s.entries, { kind: "step", step }] };
      });
    case "plan":
      return update(id, (s) => ({ ...s, plan: (e.items as PlanItem[]) ?? [] }));
    case "usage":
      return update(id, (s) => {
        const usage = { used: Number(e.used), window: Number(e.window) };
        // After a compact the offer may come back the next time it fills up.
        const compactDismissed = s.compactDismissed && usage.used / usage.window >= COMPACT_AT;
        return { ...s, usage, compactDismissed };
      });
    case "limit":
      return update(id, (s) => ({
        ...s,
        limit:
          e.status === "allowed"
            ? null
            : {
                status: String(e.status),
                window: String(e.window ?? ""),
                resetsAt: typeof e.resetsAt === "number" ? e.resetsAt : null,
                used: typeof e.used === "number" ? e.used : null,
              },
      }));
    case "ask":
      return update(id, (s) => ({
        ...s,
        status: "waiting",
        question: { id: String(e.question), label: String(e.label ?? ""), detail: String(e.detail ?? "") },
      }));
    case "answered":
      return update(id, (s) => (s.question?.id === e.question ? { ...s, question: null } : s));
    case "turn":
      return update(id, (s) => ({
        ...s,
        status: e.error ? "failed" : "idle",
        error: e.error ? String(e.error) : null,
        tookMs: Date.now() - s.turnAt,
      }));
    case "ended":
      return update(id, (s) => ({
        ...s,
        status: e.error ? "failed" : "ended",
        tookMs: s.tookMs ?? Date.now() - s.turnAt,
        error: e.error ? String(e.error) : s.error,
        changes: Number(e.changes ?? 0),
        question: null,
      }));
  }
}

let started = false;
/** Listens for agent events once, for the island window. */
export function listenToAgents() {
  if (started) return;
  started = true;
  void listen(EVENTS.agentEvent, onEvent);
}
