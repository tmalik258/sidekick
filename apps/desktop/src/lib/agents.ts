// Agent sessions in the island: Claude Code and Codex working in a project.
// Rust streams each step as `agent://event`; this keeps one timeline per
// session for the Agents tab.

import { create } from "zustand";
import { api, EVENTS, listen } from "./bridge";
import type { AgentMode, AgentStarted } from "./types";

export interface Step {
  id: string;
  tool: string;
  label: string;
  detail: string;
  state: "running" | "done" | "failed";
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
  /** A permission question waiting for the user. */
  question: { id: string; label: string; detail: string } | null;
  error: string | null;
  startedAt: number;
  /** Files changed when it ended, from Rust. */
  changes: number;
}

export type AskTab = "ask" | "agents" | "history";

interface AgentsState {
  /** Ask's tab: quick questions, agent sessions, or history. */
  tab: AskTab;
  sessions: Session[];
  /** The session shown in the Agents tab; null shows New. */
  current: string | null;
}

export const useAgents = create<AgentsState>(() => ({ tab: "ask", sessions: [], current: null }));

export const setTab = (tab: AskTab) => useAgents.setState({ tab });

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
  update(id, (s) => ({ ...s, entries: [...s.entries, { kind: "you", text }], status: "working" }));
  void api.agentSend(id, text).catch((e: unknown) => update(id, (s) => ({ ...s, error: String(e) })));
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
            ? [...s.entries.slice(0, -1), { kind: "text", text: last.text + text }]
            : [...s.entries, { kind: "text", text }];
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
      return update(id, (s) => ({ ...s, usage: { used: Number(e.used), window: Number(e.window) } }));
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
      }));
    case "ended":
      return update(id, (s) => ({
        ...s,
        status: e.error ? "failed" : "ended",
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
