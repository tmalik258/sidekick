// Agent sessions in the island: Claude Code and Codex working in a project.
// Rust streams each step as `agent://event`; this keeps one timeline per
// session for the Agents tab.

import { create } from "zustand";
import { type OpenChoice, parseChoose } from "./agent-questions";
import { api, EVENTS, listen } from "./bridge";
import { mask } from "./mask";
import { claimsPlanReady, parsePlanDocument } from "./plan";
import type { AgentMode, AgentStarted } from "./types";

export interface Step {
  id: string;
  tool: string;
  label: string;
  detail: string;
  state: "running" | "done" | "failed" | "cancelled";
  /** The end of a command's output, folded under the step. */
  output?: string;
}

export interface PlanItem {
  /** Short step heading. */
  text: string;
  /** Full step write-up under the heading. */
  detail?: string;
  status: "pending" | "in_progress" | "completed";
}

/** Full plan document shown in Plan mode (Cursor-style), not just todos. */
export interface PlanDoc {
  title: string;
  items: PlanItem[];
  assumptions?: string;
  markdown: string;
}

/** One entry in a session's timeline: something said, or a step taken. */
export type Entry =
  | { kind: "you"; text: string }
  | { kind: "text"; text: string }
  | { kind: "step"; step: Step }
  | { kind: "review"; id: string; files: number; error: string | null };

export interface Session extends AgentStarted {
  mode: AgentMode;
  activeMode?: AgentMode;
  compaction?: "running" | "done" | "failed";
  view?: { draft: string; scroll: number; follow: boolean; reviewing: boolean; active: number; reviewTurn?: string };
  title: string;
  status: "working" | "waiting" | "idle" | "ended" | "failed";
  entries: Entry[];
  plan: PlanItem[];
  /** Full plan write-up when parsed from prose or a Codex plan item. */
  planDoc?: PlanDoc | null;
  usage: { used: number; window: number; total?: number; at?: number } | null;
  /** The plan's usage limit, when the agent says it is close or reached. */
  limit?: { status: string; window: string; resetsAt: number | null; used: number | null } | null;
  /** Paused from its card: stopped after the current step, resumes on request. */
  paused?: boolean;
  /** "Not now" on the compact offer, until the context is compacted. */
  compactDismissed?: boolean;
  /** The agent's own multiple-choice question, waiting for an answer. */
  choose?: OpenChoice | null;
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
  /** The Ollama model a Local session runs. */
  localModel?: string;
  /** Picked in the chat box; null is the agent's default. */
  model?: string | null;
  /** Thinking: off, low, medium, high; null is the default. */
  effort?: string | null;
  /** Merged back with Finish. */
  finished?: boolean;
  /** The local model went round in circles: offer a handoff. */
  stuck?: boolean;
}

/** Half the context used: Claude Code works best compacted from here. */
export const COMPACT_AT = 0.5;

/** "Not now" on the compact offer. */
export function dismissCompact(id: string) {
  update(id, (s) => ({ ...s, compactDismissed: true }));
}

export type AskTab = "ask" | "agents" | "repos" | "history";

/** One session big, or the board of live tiles. */
export type AgentsLayout = "one" | "board";

/** A 1440p screen or bigger fits three board tiles across. */
export const wideScreen = () => typeof window !== "undefined" && window.screen.height >= 1440;

interface AgentsState {
  layout: AgentsLayout;
  /** The board tile the chat box talks to. */
  focus: string | null;
  /** Ask's tab: quick questions, agent sessions, or history. */
  tab: AskTab;
  sessions: Session[];
  /** The session shown in the Agents tab; null shows New. */
  current: string | null;
  /** The project New starts in, when something picked it. */
  draftPath?: string | null;
  /** What New's box starts with ("Ask an agent to fix it"). */
  draftPrompt?: string | null;
  /** The repo open in the Git dock; "first" picks one once the list loads. */
  dock?: string | null;
}

/** Sessions are kept in this window's storage, so the timeline comes back
 * after a restart; Rust keeps what Review, Undo and Resume need. */
const STORE_KEY = "sidekick.agents";

function loadSessions(): Session[] {
  try {
    const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(STORE_KEY);
    const saved = raw ? JSON.parse(raw) : [];
    const list: Session[] = Array.isArray(saved) ? saved : (saved.sessions ?? []);
    // Whatever was running stopped with Duskie; each can be resumed.
    return list.map((s) => ({
      ...s,
      status: s.status === "working" || s.status === "waiting" ? "ended" : s.status,
      question: null,
      choose: null,
      restored: true,
    }));
  } catch {
    return [];
  }
}

const LAYOUT_KEY = "sidekick.agentsLayout";

function loadView(): Partial<AgentsState> {
  try {
    const saved = JSON.parse(localStorage.getItem(STORE_KEY) ?? "{}");
    const ids = new Set(loadSessions().map((s) => s.id));
    return {
      current: ids.has(saved.current) ? saved.current : null,
      focus: ids.has(saved.focus) ? saved.focus : null,
      tab: ["ask", "agents", "repos", "history"].includes(saved.tab) ? saved.tab : "ask",
      layout: saved.layout === "board" ? "board" : saved.layout === "one" ? "one" : loadLayout(),
    };
  } catch {
    return {};
  }
}

export function sessionView(id: string, patch: Partial<NonNullable<Session["view"]>>) {
  update(id, (s) => ({
    ...s,
    view: { draft: "", scroll: 0, follow: true, reviewing: false, active: 0, ...s.view, ...patch },
  }));
}

export async function setSessionMode(id: string, mode: AgentMode) {
  try {
    await api.agentSetMode(id, mode);
    update(id, (s) => ({ ...s, mode }));
  } catch (e) {
    update(id, (s) => ({ ...s, error: String(e) }));
  }
}

/** Replace the session checklist and optional full plan document. */
export function setSessionPlan(id: string, plan: PlanItem[], planDoc?: PlanDoc | null) {
  update(id, (s) => ({
    ...s,
    plan,
    planDoc: planDoc === undefined ? s.planDoc : planDoc,
  }));
}

function loadLayout(): AgentsLayout {
  try {
    return localStorage.getItem(LAYOUT_KEY) === "board" ? "board" : "one";
  } catch {
    return "one";
  }
}

export const useAgents = create<AgentsState>(() => ({
  tab: "ask",
  sessions: loadSessions(),
  current: null,
  layout: typeof window === "undefined" ? "one" : loadLayout(),
  focus: null,
  ...loadView(),
}));

export function setLayout(layout: AgentsLayout) {
  useAgents.setState({ layout });
  try {
    localStorage.setItem(LAYOUT_KEY, layout);
  } catch {
    // Not kept: the layout resets after a restart.
  }
}

/** Gives a session your own name; it shows in the row and the board. */
export function renameSession(id: string, title: string) {
  const t = title.trim();
  if (t) update(id, (s) => ({ ...s, title: t }));
}

/** Model and thinking for one session, from its chat box. */
export function tuneSession(id: string, model: string | null, effort: string | null) {
  update(id, (s) => {
    const agent = s.agent.toLowerCase().replace(/\s+/g, "_");
    writeNewPrefs({ agent, model, effort });
    return { ...s, model, effort };
  });
  void api.agentTune(id, model, effort).catch((e) => update(id, (s) => ({ ...s, error: String(e) })));
}

/** Merges a session's own worktree back into its repo. */
export async function finishSession(id: string): Promise<string> {
  const msg = await api.agentFinish(id);
  update(id, (s) => ({ ...s, finished: true }));
  return msg;
}

let saveTimer: ReturnType<typeof setTimeout> | null = null;
/** Cap persisted sessions so streaming does not stringify unbounded timelines. */
const SAVE_CAP = 20;
function saveView(st = useAgents.getState()) {
  try {
    localStorage.setItem(
      STORE_KEY,
      JSON.stringify(
        {
          version: 2,
          sessions: st.sessions.slice(0, SAVE_CAP),
          current: st.current,
          focus: st.focus,
          tab: st.tab,
          layout: st.layout,
        },
        (_k, v: unknown) => (typeof v === "string" ? mask(v) : v),
      ),
    );
  } catch {
    // Storage full or blocked: the timeline just does not survive a restart.
  }
}
useAgents.subscribe(() => {
  if (saveTimer) clearTimeout(saveTimer);
  // Idle longer than a stream token so typing/draft flushes do not fight agent events.
  saveTimer = setTimeout(() => saveView(), 800);
});
if (typeof window !== "undefined") {
  window.addEventListener("pagehide", () => saveView());
  window.addEventListener("beforeunload", () => saveView());
}
/** Flush session view (draft/scroll) + localStorage immediately, island hide / unmount. */
export function flushAgentsView() {
  if (saveTimer) clearTimeout(saveTimer);
  saveView();
}

/** Last usual agent per project path (+ `__last` for first paint before path is known). */
const USUAL_KEY = "sidekick.agentUsualByPath";
/** New-session agent/model/effort/mode, so reopening Agents does not flash defaults. */
const NEW_PREFS_KEY = "sidekick.agentNewPrefs";

export type AgentNewPrefs = {
  agent?: string | null;
  model?: string | null;
  effort?: string | null;
  mode?: AgentMode;
  byAgent?: Record<string, { model?: string | null; effort?: string | null }>;
};

function readJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

/** Cached usual agent for a project; `__last` when path is not ready yet. */
export function readUsualAgent(path?: string | null): string | null {
  const map = readJson<Record<string, string | null>>(USUAL_KEY, {});
  if (path && map[path]) return map[path];
  return map.__last ?? null;
}

export function writeUsualAgent(path: string, id: string | null) {
  try {
    const map = readJson<Record<string, string | null>>(USUAL_KEY, {});
    map[path] = id;
    map.__last = id;
    localStorage.setItem(USUAL_KEY, JSON.stringify(map));
  } catch {
    // Storage blocked: next open may flash until agentUsual returns again.
  }
}

export function readNewPrefs(): AgentNewPrefs {
  return readJson<AgentNewPrefs>(NEW_PREFS_KEY, {});
}

export function writeNewPrefs(patch: AgentNewPrefs) {
  try {
    const prev = readNewPrefs();
    const agent = patch.agent !== undefined ? patch.agent : prev.agent;
    const byAgent = { ...prev.byAgent, ...patch.byAgent };
    if (agent && (patch.model !== undefined || patch.effort !== undefined || patch.byAgent?.[agent])) {
      byAgent[agent] = {
        ...prev.byAgent?.[agent],
        ...patch.byAgent?.[agent],
        ...(patch.model !== undefined ? { model: patch.model } : {}),
        ...(patch.effort !== undefined ? { effort: patch.effort } : {}),
      };
    }
    const next: AgentNewPrefs = { ...prev, ...patch, byAgent };
    localStorage.setItem(NEW_PREFS_KEY, JSON.stringify(next));
  } catch {
    // Not kept across restarts.
  }
}

/** Opens a tab. Agents defaults to New unless a session is still working or waiting. */
export function setTab(tab: AskTab) {
  if (tab !== "agents") {
    useAgents.setState({ tab });
    return;
  }
  const st = useAgents.getState();
  const cur = st.sessions.find((s) => s.id === st.current);
  useAgents.setState({ tab, current: cur ? st.current : null });
}

/** Opens New in Agents for one project ("Start an agent here"). */
export const startHere = (path: string, prompt?: string | null) =>
  useAgents.setState({ tab: "agents", current: null, draftPath: path, draftPrompt: prompt ?? null });

function update(id: string, fn: (s: Session) => Session) {
  useAgents.setState((st) => ({ sessions: st.sessions.map((s) => (s.id === id ? fn(s) : s)) }));
}

export async function startSession(
  agent: string,
  path: string,
  prompt: string,
  mode: AgentMode,
  model: string | null = null,
  effort: string | null = null,
): Promise<string> {
  const started = await api.agentStart(agent, path, prompt, mode, model, effort);
  addSession(started, prompt, mode, prompt);
  update(started.id, (s) => ({ ...s, model, effort }));
  return started.id;
}

function addSession(started: AgentStarted, title: string, mode: AgentMode, first: string) {
  const session: Session = {
    ...started,
    mode,
    title: title.length > 60 ? `${title.slice(0, 57)}...` : title,
    status: "working",
    activeMode: mode,
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

/** Hands a stuck session to the agent that gets handoffs, with what was
 * said so far. */
export function handOffSession(s: Session) {
  const messages = s.entries.flatMap((e): { role: "user" | "assistant"; content: string }[] =>
    e.kind === "you"
      ? [{ role: "user", content: e.text }]
      : e.kind === "text"
        ? [{ role: "assistant", content: e.text }]
        : [],
  );
  update(s.id, (x) => ({ ...x, stuck: false }));
  return handOff(messages, `${s.agent} got stuck in ${s.project}`);
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
    // Sending anything answers or replaces the agent's open question.
    choose: null,
    status: "working",
    paused: false,
    stuck: false,
    activeMode: s.status === "working" || s.status === "waiting" ? s.activeMode : s.mode,
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
 * Duskie restarted. */
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
    planDoc: null,
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
    focus: st.focus === id ? null : st.focus,
  }));
}

/** How many sessions are working or waiting for an answer. */
export function activeCount(sessions: Session[]): number {
  return sessions.filter((s) => s.status === "working" || s.status === "waiting").length;
}

function entriesCancelRunning(entries: Entry[]): Entry[] {
  return entries.map((e) =>
    e.kind === "step" && e.step.state === "running" ? { kind: "step", step: { ...e.step, state: "cancelled" } } : e,
  );
}

function onEvent(e: { session: string; kind: string } & Record<string, unknown>) {
  const id = e.session;
  switch (e.kind) {
    case "working":
      return update(id, (s) => ({
        ...s,
        status: s.question ? "waiting" : "working",
        activeMode: (e.mode as AgentMode) ?? s.activeMode ?? s.mode,
      }));
    case "control_error":
      return update(id, (s) => ({ ...s, error: String(e.error) }));
    case "compaction":
      return update(id, (s) => ({ ...s, compaction: e.state as Session["compaction"] }));
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
    case "cancel_steps":
      return update(id, (s) => ({ ...s, entries: entriesCancelRunning(s.entries) }));
    case "step":
      return update(id, (s) => {
        const at = s.entries.findIndex((x) => x.kind === "step" && x.step.id === e.id);
        if (at >= 0) {
          const entries = [...s.entries];
          const old = entries[at] as { kind: "step"; step: Step };
          // Prefer the longer text so a completed web search query replaces "Search the web".
          const prefer = (next: unknown, prev: string) => {
            const n = String(next ?? "").trim();
            if (!n) return prev;
            return n.length >= prev.trim().length ? String(next ?? "") : prev;
          };
          entries[at] = {
            kind: "step",
            step: {
              ...old.step,
              state: e.state as Step["state"],
              label: prefer(e.label, old.step.label),
              detail: prefer(e.detail, old.step.detail),
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
    case "stuck":
      return update(id, (s) => ({ ...s, stuck: true }));
    case "model":
      return update(id, (s) => ({ ...s, localModel: String(e.name ?? "") }));
    case "plan":
      return update(id, (s) => ({ ...s, plan: (e.items as PlanItem[]) ?? [] }));
    case "plan_doc": {
      const doc = parsePlanDocument(String(e.text ?? ""));
      if (!doc)
        return update(id, (s) => ({ ...s, planDoc: { title: "Plan", items: s.plan, markdown: String(e.text ?? "") } }));
      return update(id, (s) => ({
        ...s,
        plan: doc.items.length > 0 ? doc.items : s.plan,
        planDoc: doc,
      }));
    }
    case "usage":
      return update(id, (s) => {
        const usage = {
          used: Number(e.used),
          window: Number(e.window),
          total: typeof e.total === "number" ? e.total : undefined,
          at: Date.now(),
        };
        // After a compact the offer may come back the next time it fills up.
        const compactDismissed = s.compactDismissed && usage.used / usage.window >= COMPACT_AT;
        return { ...s, usage, compactDismissed };
      });
    case "limit":
      if (typeof e.used === "number") {
        const agent = useAgents.getState().sessions.find((x) => x.id === id)?.agent;
        if (agent)
          noteUsage(agent, {
            used: e.used,
            window: String(e.window ?? ""),
            resetsAt: typeof e.resetsAt === "number" ? e.resetsAt : null,
            at: Date.now(),
          });
      }
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
    case "choose": {
      const choose = parseChoose(e);
      return choose ? update(id, (s) => ({ ...s, choose })) : undefined;
    }
    case "answered":
      return update(id, (s) => (s.question?.id === e.question ? { ...s, question: null } : s));
    case "turn_diff": {
      const diff = String(e.diff ?? "");
      if (!diff.trim()) return;
      return update(id, (s) => ({ ...s, changes: Math.max(s.changes, 1) }));
    }
    case "turn":
      return update(id, (s) => {
        const entries = appendReview(s.entries, e.review);
        // Plan mode often answers in prose without turn/plan/updated, lift the doc.
        let plan = s.plan;
        let planDoc = s.planDoc;
        // Only the newest answer of this turn, and only when it says the plan
        // is ready. Older entries are never scanned, so an old plan is not
        // lifted again on a later turn.
        if ((s.mode === "plan" || s.activeMode === "plan") && (plan.length === 0 || !planDoc)) {
          const last = entries.at(-1);
          if (last?.kind === "text" && claimsPlanReady(last.text)) {
            const doc = parsePlanDocument(last.text);
            if (doc && doc.items.length > 0) {
              plan = doc.items;
              planDoc = doc;
            }
          }
        }
        return {
          ...s,
          status: e.error ? "failed" : "idle",
          error: e.error ? String(e.error) : null,
          tookMs: Date.now() - s.turnAt,
          changes: Number(e.changes ?? s.changes),
          entries,
          plan,
          planDoc,
        };
      });
    case "ended":
      return update(id, (s) => ({
        ...s,
        status: e.error ? "failed" : "ended",
        tookMs: s.tookMs ?? Date.now() - s.turnAt,
        error: e.error ? String(e.error) : s.error,
        changes: Number(e.changes ?? 0),
        question: null,
        entries: appendReview(s.entries, e.review),
      }));
  }
}

/** Timeline Review rows only when there are real files (or a capture error). */
function appendReview(entries: Entry[], review: unknown): Entry[] {
  const r = review as { id: string; files: number; error: string | null } | null | undefined;
  if (!r?.id) return entries;
  if (r.files === 0 && !r.error) return entries;
  if (entries.some((entry) => entry.kind === "review" && entry.id === r.id)) return entries;
  return [...entries, { kind: "review", id: r.id, files: r.files, error: r.error }];
}

let started = false;
/** Listens for agent events once, for the island window. */
export function listenToAgents() {
  if (started) return;
  started = true;
  void listen(EVENTS.agentEvent, onEvent);
}

/** One plan-limit window an agent reported, kept so the picker and usage pop can show it. */
export interface SeenUsage {
  used: number;
  window: string;
  resetsAt: number | null;
  at: number;
}

const USAGE_KEY = "sidekick.agentUsage";

/** Per agent, each rate-limit window (5-hour, weekly, …). */
type SeenByAgent = Record<string, Record<string, SeenUsage>>;

function isSeenUsage(v: unknown): v is SeenUsage {
  return !!v && typeof v === "object" && "used" in v && "window" in v && "at" in v;
}

/** Older stores kept one SeenUsage per agent; lift those into a window map. */
function seenAll(): SeenByAgent {
  try {
    const raw = JSON.parse(localStorage.getItem(USAGE_KEY) ?? "{}") as Record<string, unknown>;
    const out: SeenByAgent = {};
    for (const [agent, v] of Object.entries(raw)) {
      if (isSeenUsage(v)) {
        out[agent] = { [v.window || "plan"]: v };
      } else if (v && typeof v === "object") {
        const windows: Record<string, SeenUsage> = {};
        for (const [w, u] of Object.entries(v as Record<string, unknown>)) {
          if (isSeenUsage(u)) windows[w] = u;
        }
        if (Object.keys(windows).length) out[agent] = windows;
      }
    }
    return out;
  } catch {
    return {};
  }
}

function noteUsage(agent: string, u: SeenUsage) {
  const key = u.window || "plan";
  try {
    const all = seenAll();
    localStorage.setItem(USAGE_KEY, JSON.stringify({ ...all, [agent]: { ...all[agent], [key]: u } }));
  } catch {
    // Not kept: the picker just shows nothing for this agent.
  }
}

/** Every plan window Duskie has seen for an agent (5-hour, weekly, …). */
export function seenWindows(agent: string): SeenUsage[] {
  const map = seenAll()[agent];
  if (!map) return [];
  return Object.values(map);
}

/** The busiest live window, for a one-line chip; null when never seen. */
export function seenUsage(agent: string): SeenUsage | null {
  const now = Date.now();
  const live = seenWindows(agent).filter((u) => !(u.resetsAt !== null && u.resetsAt * 1000 <= now));
  if (live.length === 0) return null;
  return live.reduce((a, b) => (pct(b) > pct(a) ? b : a));
}

function pct(u: SeenUsage): number {
  return Math.round(u.used <= 1 ? u.used * 100 : u.used);
}

/** "85% of 5-hour, 20 min ago"; "ready" once every window has reset; null when never seen. */
export function usageLine(agent: string, now = Date.now()): string | null {
  const all = seenWindows(agent);
  if (all.length === 0) return null;
  const live = all.filter((u) => !(u.resetsAt !== null && u.resetsAt * 1000 <= now));
  if (live.length === 0) return "ready";
  const u = live.reduce((a, b) => (pct(b) > pct(a) ? b : a));
  const span = u.window === "five_hour" ? "5-hour" : u.window.startsWith("seven_day") ? "week" : "plan";
  const mins = Math.round((now - u.at) / 60_000);
  const ago = mins < 1 ? "just now" : mins < 60 ? `${mins} min ago` : `${Math.round(mins / 60)} h ago`;
  return `${pct(u)}% of ${span}, ${ago}`;
}

/** Pause: stop the turn but keep the session ready to pick up where it was. */
export function pauseSession(id: string) {
  interruptSession(id);
  update(id, (s) => ({ ...s, paused: true }));
}

/** Resume a paused session by asking it to carry on. */
export function resumePaused(id: string) {
  update(id, (s) => ({ ...s, paused: false }));
  sendToSession(id, "Continue where you left off.");
}

/** Esc / Stop: leave the session idle right away; Rust keeps it resumable on the next send. */
export function interruptSession(id: string) {
  update(id, (s) => ({
    ...s,
    status: "idle",
    question: null,
    tookMs: s.tookMs ?? Date.now() - s.turnAt,
    error: null,
    entries: entriesCancelRunning(s.entries),
  }));
  void api.agentStop(id);
}

/** Ctrl G: the Repos tab with the Git dock open on the repo that has changes. */
export function openGit() {
  useAgents.setState({ dock: useAgents.getState().dock ?? "first" });
  setTab("repos");
}
