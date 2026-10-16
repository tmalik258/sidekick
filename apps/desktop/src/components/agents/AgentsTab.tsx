"use client";

// The Agents tab: start Claude Code or Codex in a project, watch it work
// step by step, answer its questions here, steer it, and keep or undo each
// change when it is done.

import {
  type CSSProperties,
  Fragment,
  type KeyboardEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { AnswerOptions } from "@/components/ask/Proposals";
import { activityLabel, useActivities } from "@/lib/activities";
import { type AgentCommand, commandCatalog } from "@/lib/agent-commands";
import { answerText, type OpenChoice } from "@/lib/agent-questions";
import {
  answerQuestion,
  COMPACT_AT,
  closeSession,
  dismissCompact,
  type Entry,
  flushAgentsView,
  handOffSession,
  interruptSession,
  messageIndex,
  readNewPrefs,
  readUsualAgent,
  renameSession,
  resumeSession,
  rewindSession,
  type Session,
  type Step,
  sendToSession,
  sessionView,
  setSessionMode,
  startHere,
  startSession,
  tuneSession,
  usageLine,
  useAgents,
  writeNewPrefs,
  writeUsualAgent,
} from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import { Markdown } from "@/lib/markdown";
import { extractChoices } from "@/lib/options";
import { parsePlanDocument } from "@/lib/plan";
import { setAsk, setOverlayHit, useDuskie } from "@/lib/store";
import type { AgentInfo, AgentMode, Agents, CursorChat, EditorList } from "@/lib/types";
import { scrollIfActive } from "../ask/parts";
import { Icon } from "../Icon";
import { Select } from "../settings/ui";
import { Tip } from "../Tip";
import { Board } from "./Board";
import { ChatControls } from "./Controls";
import { PlanPanel } from "./PlanPanel";
import { ProjectPicker } from "./ProjectPicker";
import { Review } from "./Review";

const MODES: { id: AgentMode; label: string; note: string }[] = [
  { id: "plan", label: "Plan", note: "Reads and plans, changes nothing" },
  { id: "ask", label: "Ask", note: "Asks before every change" },
  { id: "edit", label: "Edit", note: "Edits files, asks before commands" },
  { id: "full", label: "Full", note: "Does everything without asking" },
];

/** Next Plan → Ask → Edit → Full (wraps). Bound to Ctrl+. in the composer. */
function nextAgentMode(mode: AgentMode): AgentMode {
  const i = MODES.findIndex((m) => m.id === mode);
  return MODES[(i + 1) % MODES.length].id;
}

/** Letter and colour for each agent in the picker. */
export const AGENT_MARKS: Record<string, [string, string]> = {
  claude_code: ["C", "#d97757"],
  codex: ["X", "#10a37f"],
  copilot: ["G", "#8957e5"],
  cursor: ["R", "#9aa0a6"],
  local: ["L", "#5e9cff"],
};

/** Picture marks when the letter chip is not enough. */
export const AGENT_IMAGES: Record<string, string> = {
  claude_code: "/agents/claude-code.png",
  codex: "/agents/codex.svg",
  cursor: "/agents/cursor.png",
  local: "/agents/ollama.png",
};

const MARK_BY_NAME: Record<string, string> = {
  "Claude Code": "claude_code",
  Codex: "codex",
  "GitHub Copilot": "copilot",
  Cursor: "cursor",
  Local: "local",
};

/** Normalize a display name or id to the AGENT_MARKS key. */
export function agentMarkId(agent: string): string {
  return MARK_BY_NAME[agent] ?? agent.toLowerCase().replace(/\s+/g, "_");
}

/** Letter and colour from a session's agent display name. */
export const markFor = (agent: string): [string, string] => AGENT_MARKS[agentMarkId(agent)] ?? ["L", "#8e8e93"];

/** Agent chip: Clawd / Codex art when we have it, else letter + colour. */
export function AgentMark({ agent, sm, className = "" }: { agent: string; sm?: boolean; className?: string }) {
  const id = agentMarkId(agent);
  const image = AGENT_IMAGES[id];
  const [letter, color] = markFor(agent);
  const cls = `ak-mark${sm ? " sm" : ""}${image ? " ak-mark-pic" : ""} ${className}`.trim();
  if (image) {
    return (
      <span className={cls} aria-hidden="true">
        {/* public/ asset; Next Image is unnecessary for a tiny mark */}
        <img src={image} alt="" draggable={false} />
      </span>
    );
  }
  return (
    <span className={cls} style={{ background: color }} aria-hidden="true">
      {letter}
    </span>
  );
}

/** Chats started in Cursor, refreshed while Agents is visible. */
function useCursorChats(active: boolean): CursorChat[] {
  const [chats, setChats] = useState<CursorChat[]>([]);
  useEffect(() => {
    if (!active) return;
    let live = true;
    let busy = false;
    const load = () => {
      if (busy) return;
      busy = true;
      void api
        .cursorChats()
        .then((c) => {
          if (live) setChats(c);
        })
        .catch(() => {})
        .finally(() => {
          busy = false;
        });
    };
    load();
    const t = setInterval(load, 30_000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [active]);
  return chats;
}

const CURSOR = "cursor:";

export function AgentsTab({
  maxHeight,
  active = true,
}: {
  maxHeight: number;
  /** False while Ask is closed but Agents is kept warm — skip background work. */
  active?: boolean;
}) {
  const sessions = useAgents((s) => s.sessions);
  const current = useAgents((s) => s.current);
  const layout = useAgents((s) => s.layout);
  const cursor = useCursorChats(active);
  const session = sessions.find((s) => s.id === current) ?? null;
  if (layout === "board") return <Board sessions={sessions} maxHeight={maxHeight} active={active} />;
  const watched = current?.startsWith(CURSOR) ? cursor.find((c) => CURSOR + c.id === current) : undefined;
  if (watched) return <CursorView chat={watched} sessions={sessions} cursor={cursor} />;
  return session ? (
    <SessionView key={session.id} session={session} sessions={sessions} maxHeight={maxHeight} active={active} />
  ) : (
    <NewSession sessions={sessions} cursor={cursor} />
  );
}

/** A chat started in Cursor: status and last reply; replies happen in Cursor. */
function CursorView({ chat, sessions, cursor }: { chat: CursorChat; sessions: Session[]; cursor: CursorChat[] }) {
  const [error, setError] = useState<string | null>(null);
  return (
    <>
      <div className="ak-head">
        <span className="ak-title truncate">{chat.title}</span>
        <span className="ak-mi">Watching</span>
      </div>
      <SessionChips sessions={sessions} current={CURSOR + chat.id} cursor={cursor} />
      <div className="ak-meta">
        <span className="ak-mi b">Cursor</span>
        {chat.project && <span className="ak-mi">{chat.project}</span>}
        <span className="ak-mi">{chat.status === "working" ? "Working" : "Idle"}</span>
      </div>
      {chat.lastReply ? (
        <div className="ak-in text-[13px]">
          <Markdown text={chat.lastReply} />
        </div>
      ) : (
        <p className="ak-note">No reply yet.</p>
      )}
      <div className="ak-composer justify-center">
        <button
          type="button"
          disabled={!chat.path}
          onClick={() => chat.path && void api.cursorOpen(chat.path).catch((e: unknown) => setError(String(e)))}
          className="ak-chip chip"
        >
          Open in Cursor
        </button>
      </div>
      {error && <p className="ak-err">{error}</p>}
    </>
  );
}

/** Session chips with + New first when viewing one (to leave it), then Cursor chats. */
function SessionChips({
  sessions,
  current,
  cursor = [],
}: {
  sessions: Session[];
  current: string | null;
  cursor?: CursorChat[];
}) {
  if (sessions.length === 0 && cursor.length === 0 && current === null) return null;
  return (
    <div className="ak-sess">
      {current !== null && (
        <button
          type="button"
          onClick={() => useAgents.setState({ current: null })}
          className="ak-sp chip text-[rgb(235_235_245/0.45)]"
        >
          + New
        </button>
      )}
      {sessions.map((s) => {
        const color = markFor(s.agent)[1];
        return (
          <button
            key={s.id}
            type="button"
            aria-pressed={s.id === current}
            onClick={() => useAgents.setState({ current: s.id })}
            className="ak-sp chip max-w-56"
            style={{ "--sp": color } as CSSProperties}
          >
            <span className="ak-sd" data-s={s.status} role="img" aria-label={s.status} />
            <span className="truncate">{s.title}</span>
            <em>
              {s.agent}
              {s.project ? ` · ${s.project}` : ""}
            </em>
          </button>
        );
      })}
      {cursor.map((c) => {
        const color = markFor("Cursor")[1];
        return (
          <button
            key={c.id}
            type="button"
            aria-pressed={CURSOR + c.id === current}
            onClick={() => useAgents.setState({ current: CURSOR + c.id })}
            className="ak-sp chip max-w-56"
            style={{ "--sp": color } as CSSProperties}
          >
            <span className="ak-sd" data-s={c.status} role="img" aria-label={c.status} />
            <span className="truncate">{c.title}</span>
            <em>{c.project ? `${c.project} · Cursor` : "Cursor"}</em>
          </button>
        );
      })}
    </div>
  );
}

function NewSession({ sessions, cursor }: { sessions: Session[]; cursor: CursorChat[] }) {
  const redesign = useDuskie((st) => st.settings.redesign.enabled);
  const { data: agents, refreshing, refresh } = useCached<Agents>("agents", api.agentsStatus);
  const { data: projects } = useCached<{ name: string; path: string }[]>("projects", api.projectsList, 30_000);
  const prefs = useRef(readNewPrefs()).current;
  // Last session's agent fills the gap before usual-cache / prefs exist.
  const lastAgentId = sessions[0] ? agentMarkId(sessions[0].agent) : null;
  const lastModel = sessions[0]?.model ?? null;
  const lastEffort = sessions[0]?.effort ?? null;
  const [agent, setAgent] = useState<string>(prefs.agent ?? "");
  const draftPath = useAgents((s) => s.draftPath);
  const [path, setPath] = useState(draftPath ?? "");
  const [mode, setMode] = useState<AgentMode>(prefs.mode ?? "edit");
  const draftPrompt = useAgents((s) => s.draftPrompt);
  const [prompt, setPrompt] = useState(draftPrompt ?? "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const seedAgent = prefs.agent || readUsualAgent() || lastAgentId || "";
  const seedTune = (seedAgent && prefs.byAgent?.[seedAgent]) || prefs;
  const [model, setModel] = useState<string | null>(
    seedTune.model ?? (seedAgent === lastAgentId ? lastModel : null) ?? null,
  );
  const [effort, setEffort] = useState<string | null>(
    seedTune.effort ?? (seedAgent === lastAgentId ? lastEffort : null) ?? null,
  );
  const inputRef = useRef<HTMLInputElement>(null);
  // Ready agents first; the rest stay listed with what they need.
  const all = agents?.list ?? [];
  const choices = [...all.filter(ready), ...all.filter((a) => !ready(a) && a.installed)];
  const missing = all.filter((a) => !a.installed);
  const pickedPath = path || projects?.[0]?.path || "";
  // Seed from cache / last session so the first paint matches usual (no Claude→Codex flash).
  const [usual, setUsual] = useState<string | null>(() => readUsualAgent(pickedPath || undefined) || lastAgentId);
  // biome-ignore lint/correctness/useExhaustiveDependencies: path drives refresh; usual seed is sync
  useEffect(() => {
    if (!pickedPath) return;
    let live = true;
    const cached = readUsualAgent(pickedPath);
    if (cached && cached !== usual) setUsual(cached);
    void api
      .agentUsual(pickedPath)
      .then((id) => {
        writeUsualAgent(pickedPath, id);
        if (live) setUsual(id);
      })
      .catch(() => live && setUsual(null));
    return () => {
      live = false;
    };
  }, [pickedPath]);
  const usualChoice = choices.find((c) => c.id === usual && ready(c))?.id;
  const pickedAgent = agent || usualChoice || choices[0]?.id || "";
  const picked = all.find((a) => a.id === pickedAgent);
  const pickAgent = (id: string) => {
    setAgent(id);
    const tune = readNewPrefs().byAgent?.[id];
    if (tune) {
      if (tune.model !== undefined) setModel(tune.model ?? null);
      if (tune.effort !== undefined) setEffort(tune.effort ?? null);
    }
    writeNewPrefs({ agent: id, model: tune?.model ?? model, effort: tune?.effort ?? effort, mode });
  };
  const pickModel = (m: string | null) => {
    setModel(m);
    writeNewPrefs({ agent: pickedAgent || agent || null, model: m, effort, mode });
  };
  const pickEffort = (e: string | null) => {
    setEffort(e);
    writeNewPrefs({ agent: pickedAgent || agent || null, model, effort: e, mode });
  };
  const pickMode = (m: AgentMode) => {
    setMode(m);
    writeNewPrefs({ agent: pickedAgent || agent || null, model, effort, mode: m });
  };
  useEffect(() => {
    requestAnimationFrame(() => inputRef.current?.focus({ preventScroll: true }));
  }, []);
  const go = () => {
    if (!prompt.trim() || !pickedPath || busy || (picked && !ready(picked))) return;
    setBusy(true);
    setError(null);
    // Confirm readiness right before start; avoid refresh on every open.
    void refresh().catch(() => undefined);
    writeNewPrefs({ agent: pickedAgent, model, effort, mode });
    writeUsualAgent(pickedPath, pickedAgent);
    startSession(pickedAgent, pickedPath, prompt.trim(), mode, model, effort)
      .then(() => setPrompt(""))
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setBusy(false));
  };
  return (
    <>
      {redesign && sessions.length > 0 ? (
        <SessionList sessions={sessions} />
      ) : (
        <>
          <div className="ak-head">
            <span className="ak-title">New session</span>
          </div>
          <SessionChips sessions={sessions} current={null} cursor={cursor} />
        </>
      )}
      {!agents || (refreshing && choices.length === 0) ? (
        <NewSessionLoading />
      ) : choices.length === 0 ? (
        <NoAgentsReady sessions={sessions.length} cursor={cursor.length} missing={missing} />
      ) : (
        <>
          <div className="ak-meta">
            {choices.length + missing.length > 1 ? (
              <span className="ak-mi b">
                <Select
                  variant="plain"
                  overlay
                  label="Agent"
                  value={pickedAgent}
                  onChange={pickAgent}
                  options={[...choices, ...missing].map((c) => ({
                    value: c.id,
                    label: c.name,
                    sub: pickerNote(c),
                    icon: AGENT_MARKS[c.id]?.[0] ?? "C",
                    color: AGENT_MARKS[c.id]?.[1] ?? "#d97757",
                    image: AGENT_IMAGES[c.id],
                  }))}
                />
              </span>
            ) : (
              <span className="ak-mi b">
                {choices[0]?.name ?? "Claude Code"}
                {choices[0] && usageLine(choices[0].name) && (
                  <em className="ml-1.5 font-normal not-italic opacity-70">{usageLine(choices[0].name)}</em>
                )}
              </span>
            )}
            <span className="ak-mi max-w-44">
              <ProjectPicker
                projects={projects ?? []}
                path={pickedPath}
                onChange={(next) => {
                  setPath(next);
                  useAgents.setState({ draftPath: next });
                }}
              />
            </span>
            <div className="ak-modes">
              <ModeSwitch mode={mode} onChange={pickMode} />
              <span className="keys mono">Ctrl+.</span>
            </div>
          </div>
          {picked && !ready(picked) && <FixLine agent={picked} />}
          {picked?.local && ready(picked) && (
            <p className="ak-local">
              <strong>On this PC.</strong> Nothing leaves it. Slower; best for small, clear changes. Every edit waits in
              Review.
            </p>
          )}
          <div className="ak-composer ak-two">
            <div className="ak-row">
              <input
                ref={inputRef}
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                onKeyDown={(e) => {
                  if (e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && e.key === ".") {
                    e.preventDefault();
                    pickMode(nextAgentMode(mode));
                    return;
                  }
                  if (e.key === "Enter") {
                    e.preventDefault();
                    go();
                  }
                }}
                placeholder="What should it do?"
              />
            </div>
            <ChatControls
              agent={pickedAgent}
              model={model}
              effort={effort}
              onModel={pickModel}
              onEffort={pickEffort}
              keys={busy ? "Starting..." : "Enter start"}
            />
          </div>
          {error && <p className="ak-err">{error}</p>}
        </>
      )}
    </>
  );
}

/** No CLI is ready: past sessions can still be opened; setup goes through Settings. */
function NoAgentsReady({ sessions, cursor, missing }: { sessions: number; cursor: number; missing: AgentInfo[] }) {
  const hasPast = sessions > 0 || cursor > 0;
  return (
    <div className="ak-group flex flex-col gap-2 py-1 text-[13px]">
      {hasPast ? (
        <p>
          No coding agent is ready on this PC right now. Open a session above to review it, or set one up in Settings.
        </p>
      ) : (
        <p>Add Claude Code, Codex, Copilot, or Cursor to start a session here.</p>
      )}
      {missing.slice(0, 3).map((a) => (
        <FixLine key={a.id} agent={a} />
      ))}
      <button
        type="button"
        className="ak-chip chip self-start"
        onClick={() => setAsk({ view: "settings", settingsTab: "ai" })}
      >
        Open Settings → AI
      </button>
    </div>
  );
}

/** Skeleton of the new-session form while agents are detected (same idea as ReposLoading). */
function NewSessionLoading() {
  return (
    <div className="ak-nload" role="status" aria-busy="true" aria-label="Finding coding agents">
      <div className="ak-meta ak-rskel" aria-hidden="true">
        <i className="ak-npill" style={{ width: 92 }} />
        <i className="ak-npill" style={{ width: 108 }} />
        <i className="ak-npill" style={{ width: 74 }} />
      </div>
      <div className="ak-composer ak-two ak-rskel" aria-hidden="true">
        <div className="ak-row">
          <i className="ak-nbar" />
        </div>
        <div className="ak-nctrls">
          <i className="ak-npill" style={{ width: 58 }} />
          <i className="ak-npill" style={{ width: 48 }} />
          <i className="ak-npill" style={{ width: 44 }} />
        </div>
      </div>
      <p className="ak-nload-msg">
        <span className="shimmer-text">Finding coding agents</span>
      </p>
    </div>
  );
}

/** Ready to start: installed, not known signed out, not out of usage. */
function ready(a: AgentInfo): boolean {
  return a.installed && a.signedIn !== false && !a.limited;
}

/** Where it runs and whether it is ready, in a few words. */
/** Where it runs, then the last plan usage seen when it is ready. */
function pickerNote(a: AgentInfo): string {
  const usage = ready(a) && !a.local ? usageLine(a.name) : null;
  return usage ? `Cloud · ${usage}` : readyNote(a);
}

function readyNote(a: AgentInfo): string {
  if (!a.installed) {
    // Cursor app on PATH is not the agent CLI Duskie runs.
    if (a.id === "cursor") return "Needs agent CLI";
    return "Not installed";
  }
  if (a.local) return "On this PC. Slower, best for small, clear changes.";
  if (a.signedIn === false) return "Cloud · sign in needed";
  if (a.limited) return "Cloud · out of usage for now";
  return a.signedIn ? "Cloud · ready" : "Cloud · installed";
}

/** The one step that makes an agent ready, with Copy when it is a command. */
function FixLine({ agent }: { agent: AgentInfo }) {
  const [copied, setCopied] = useState(false);
  const step = agent.limited && !agent.fix ? null : agent.fix;
  const command = step && !/\s(from|then)\s/.test(step) ? step : null;
  return (
    <p className="ak-note ak-in flex items-center gap-2">
      <span className="min-w-0 flex-1">
        {agent.name}: {readyNote(agent).replace(/^Cloud · /, "")}.
        {step && (
          <>
            {" "}
            {command ? "Run " : ""}
            <code className="mono">{step}</code>
          </>
        )}
      </span>
      {command && (
        <button
          type="button"
          className="ak-chip chip"
          onClick={() => {
            void navigator.clipboard?.writeText(command).then(() => setCopied(true));
          }}
        >
          {copied ? "Copied" : "Copy"}
        </button>
      )}
    </p>
  );
}

function ModeSwitch({ mode, onChange, agent }: { mode: AgentMode; onChange?: (m: AgentMode) => void; agent?: string }) {
  return (
    <fieldset aria-label="Mode" className="ak-seg border-0">
      {MODES.map((m) => (
        <Tip
          key={m.id}
          label={
            agent === "Codex"
              ? {
                  plan: "Codex Plan collaboration mode in a read-only sandbox",
                  ask: "Workspace access; untrusted commands require approval",
                  edit: "Workspace files and commands allowed; asks for escalation",
                  full: "Full access without sandbox or approval prompts",
                }[m.id]
              : agent === "Local"
                ? m.id === "plan"
                  ? "Read-only tools"
                  : "Every edit requires approval; commands are unavailable"
                : m.note
          }
        >
          <button
            type="button"
            aria-pressed={mode === m.id}
            disabled={!onChange}
            onClick={() => onChange?.(m.id)}
            className="chip"
          >
            {m.label}
          </button>
        </Tip>
      ))}
    </fieldset>
  );
}

/** How much of the context window is used, as a small ring. */
export function ContextRing({ used, window, onCompact }: { used: number; window: number; onCompact?: () => void }) {
  const p = Math.round(Math.min(1, used / window) * 100);
  if (!onCompact) {
    return (
      <Tip label={`${p}% of the context used`}>
        <span className="ak-ring mono">
          <i style={{ "--p": p } as CSSProperties} aria-hidden="true" />
          {p}%
        </span>
      </Tip>
    );
  }
  // One click runs /compact: the conversation is summed up to free room.
  return (
    <Tip label={`${p}% of the context used. Click to compact it.`}>
      <button
        type="button"
        onClick={onCompact}
        aria-label={`${p}% of the context used. Compact`}
        className="ak-ring mono chip rounded-full px-1 hover:text-white"
      >
        <i style={{ "--p": p } as CSSProperties} aria-hidden="true" />
        {p}%
      </button>
    </Tip>
  );
}

/** How much memory the session's CLI uses, checked every 5 seconds. */
function SessionMemory({ id, live }: { id: string; live: boolean }) {
  const [bytes, setBytes] = useState<number | null>(null);
  useEffect(() => {
    if (!live) {
      setBytes(null);
      return;
    }
    let on = true;
    const check = () =>
      void api
        .agentMemory(id)
        .then((b) => on && setBytes(b))
        .catch(() => undefined);
    check();
    const t = setInterval(check, 5000);
    return () => {
      on = false;
      clearInterval(t);
    };
  }, [id, live]);
  if (bytes === null) return null;
  const mb = bytes / (1024 * 1024);
  return (
    <Tip label="Memory this session uses">
      <span className="ak-mi">{mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${Math.round(mb)} MB`}</span>
    </Tip>
  );
}

/** "1:12", or "8 s" under a minute. */
function clock(ms: number): string {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s} s` : `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** follow:false + scroll:0 is a bad flush (open before layout); treat as stick-to-bottom. */
function stickBottom(st: { scroll: number; follow: boolean }): boolean {
  return st.follow !== false || st.scroll <= 0;
}

function SessionView({
  session: s,
  sessions,
  maxHeight,
  active = true,
}: {
  session: Session;
  sessions: Session[];
  maxHeight: number;
  active?: boolean;
}) {
  const [text, setLocalText] = useState(s.view?.draft ?? "");
  const textRef = useRef(text);
  const savedScroll = s.view?.scroll ?? 0;
  const savedFollow = s.view?.follow ?? true;
  const scrollState = useRef({
    scroll: savedScroll,
    // Coerce corrupted prefs so reopen does not land at the top of a 200kpx timeline.
    follow: savedFollow === false && savedScroll <= 0 ? true : savedFollow,
  });
  // biome-ignore lint/correctness/useExhaustiveDependencies: restore draft/scroll only when the session switches
  useEffect(() => {
    const draft = s.view?.draft ?? "";
    textRef.current = draft;
    setLocalText(draft);
    const scroll = s.view?.scroll ?? 0;
    const follow = s.view?.follow ?? true;
    scrollState.current = { scroll, follow: follow === false && scroll <= 0 ? true : follow };
  }, [s.id]);
  const setText = (draft: string) => {
    textRef.current = draft;
    setLocalText(draft);
  };
  const flushView = () => {
    const st = scrollState.current;
    const follow = stickBottom(st);
    sessionView(s.id, { draft: textRef.current, scroll: follow ? st.scroll : st.scroll, follow });
    flushAgentsView();
  };
  const reviewing = s.view?.reviewing ?? false;
  const setReviewing = (next: boolean) =>
    sessionView(s.id, { draft: textRef.current, ...scrollState.current, reviewing: next });
  const [rewindAt, setRewindAt] = useState<number | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** Bumped after rewind so the composer focuses and selects the restored draft. */
  const [editFocus, setEditFocus] = useState(0);
  const { data: editors } = useCached<EditorList>("editors", api.editorsList);
  const editor = editors?.editors.find((e) => e.id === editors.current)?.name ?? null;
  const scroll = useRef<HTMLDivElement>(null);
  const activity = useActivities((state) => state.activities.find((a) => a.parentId === s.id && !a.dismissed));
  const working = s.status === "working" || s.status === "waiting";
  const now = useNow(active && working ? 250 : 60_000);
  const done = s.status === "idle" || s.status === "ended" || s.status === "failed";
  const current = useMemo(() => {
    const step = [...s.entries].reverse().find((e) => e.kind === "step" && e.step.state === "running");
    return step && step.kind === "step" ? step.step.label : null;
  }, [s.entries]);
  // Restore scroll before paint; ResizeObserver keeps follow pinned while Markdown grows.
  // Skip while warm/hidden — hidden nodes report scrollHeight 0 and would wipe the saved offset.
  // biome-ignore lint/correctness/useExhaustiveDependencies: follow new entry count, not every streamed token array identity
  useLayoutEffect(() => {
    if (!active) return;
    const el = scroll.current;
    if (!el) return;
    const apply = () => {
      el.scrollTop = stickBottom(scrollState.current) ? el.scrollHeight : scrollState.current.scroll;
    };
    apply();
    const ro = new ResizeObserver(() => {
      if (stickBottom(scrollState.current)) el.scrollTop = el.scrollHeight;
    });
    ro.observe(el);
    const id = requestAnimationFrame(apply);
    return () => {
      ro.disconnect();
      cancelAnimationFrame(id);
    };
  }, [s.id, reviewing, s.entries.length, s.plan.length, s.question, s.status, active]);
  // The cleanup intentionally captures the current session identity.
  // biome-ignore lint/correctness/useExhaustiveDependencies: saves on session switch/unmount
  useEffect(() => {
    if (!active) return;
    const onHide = () => flushView();
    document.addEventListener("visibilitychange", onHide);
    window.addEventListener("pagehide", onHide);
    return () => {
      document.removeEventListener("visibilitychange", onHide);
      window.removeEventListener("pagehide", onHide);
      flushView();
    };
  }, [s.id, active]);

  // Alt 1 reviews, Alt 2 (or Alt T) carries on in a terminal.
  // biome-ignore lint/correctness/useExhaustiveDependencies: setReviewing is recreated each render; s.id covers the session
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (!e.altKey || e.ctrlKey || e.metaKey) return;
      const k = e.key.toLowerCase();
      if (k === "t" || (k === "2" && done)) {
        e.preventDefault();
        void api.agentTerminal(s.id);
      } else if (k === "1" && done && s.reviewable && s.changes !== 0) {
        e.preventDefault();
        setReviewing(true);
      } else if (k === "3" && done && editor) {
        e.preventDefault();
        void api.agentOpenEditor(s.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [s.id, done, s.reviewable, s.changes, editor]);

  // Half the context used: offer /compact (Alt K), Claude Code only.
  const offerCompact =
    s.agent === "Claude Code" &&
    done &&
    !s.compactDismissed &&
    !!s.usage &&
    s.usage.used / s.usage.window >= COMPACT_AT;
  useEffect(() => {
    if (!offerCompact) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (!e.altKey || e.ctrlKey || e.metaKey || e.key.toLowerCase() !== "k") return;
      e.preventDefault();
      sendToSession(s.id, "/compact");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [s.id, offerCompact]);

  // Alt A, Y, N answer a question while one waits.
  useEffect(() => {
    if (!s.question) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || !e.altKey) return;
      const k = e.key.toLowerCase();
      const answer = k === "a" ? "allow" : k === "y" ? "always" : k === "n" ? "deny" : null;
      if (!answer) return;
      e.preventDefault();
      answerQuestion(s.id, answer);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [s.id, s.question]);

  const send = () => {
    const t = text.trim();
    if (!t) return;
    sendToSession(s.id, t);
    textRef.current = "";
    setLocalText("");
    sessionView(s.id, { draft: "", ...scrollState.current });
  };
  const usedPct = s.usage ? Math.round(Math.min(1, s.usage.used / s.usage.window) * 100) : null;
  // Long sessions: paint the recent tail first (open lag was parsing every Markdown block).
  const TAIL = 28;
  const [showEarlier, setShowEarlier] = useState(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset the tail fold when the session switches
  useEffect(() => setShowEarlier(false), [s.id]);
  const entryStart = showEarlier || s.entries.length <= TAIL ? 0 : s.entries.length - TAIL;
  const visibleEntries = s.entries.slice(entryStart);
  let lastAgentText = -1;
  for (let k = s.entries.length - 1; k >= 0; k--) {
    if (s.entries[k].kind === "text") {
      lastAgentText = k;
      break;
    }
  }

  return (
    <>
      <div className="ak-head">
        <NameField id={s.id} title={s.title} />
        {working ? (
          <button type="button" onClick={() => interruptSession(s.id)} className="ak-stop chip">
            Stop <kbd>Esc</kbd>
          </button>
        ) : (
          <Tip label="Close session">
            <button
              type="button"
              aria-label="Close session"
              onClick={() => closeSession(s.id)}
              className="ak-ibtn chip"
            >
              <Icon name="close" size={13} />
            </button>
          </Tip>
        )}
      </div>
      <SessionChips sessions={sessions} current={s.id} />
      <div className="ak-meta">
        <span className="ak-mi b">{s.agent}</span>
        <span className="ak-mi">{s.project}</span>
        {s.branch && <span className="ak-mi">⎇ {s.branch}</span>}
        <SessionMemory id={s.id} live={active && s.status !== "ended" && s.status !== "failed" && !s.restored} />
        <div className="ak-modes">
          <ModeSwitch agent={s.agent} mode={s.mode} onChange={(mode) => void setSessionMode(s.id, mode)} />
          <span className="keys mono">Ctrl+.</span>
        </div>
        {working && s.activeMode !== s.mode && <span className="ak-note">Applies to next message</span>}
      </div>

      {reviewing ? (
        <Review session={s} maxHeight={maxHeight} onDone={() => setReviewing(false)} />
      ) : (
        <>
          <div
            ref={scroll}
            onScroll={(e) => {
              const el = e.currentTarget;
              scrollState.current = {
                scroll: el.scrollTop,
                follow: el.scrollHeight - el.scrollTop - el.clientHeight < 48,
              };
            }}
            className="ak-tl ak-scroll"
            style={{ maxHeight: maxHeight - 40 }}
          >
            {entryStart > 0 && (
              <button type="button" className="ak-chip chip self-start" onClick={() => setShowEarlier(true)}>
                Show earlier ({entryStart} more)
              </button>
            )}
            {visibleEntries.map((e, j) => {
              const i = entryStart + j;
              // Last agent prose (idle or still "working" after a clarifying ask).
              const lastText = e.kind === "text" && !s.question && i === lastAgentText;
              // Full plan lives in PlanPanel — don't repeat the same prose above it.
              const planCovers =
                lastText && !!s.planDoc && e.kind === "text" && (parsePlanDocument(e.text)?.items.length ?? 0) >= 2;
              return (
                <Fragment key={i}>
                  {!planCovers && (
                    <EntryRow
                      entry={e}
                      choices={lastText}
                      onPick={(t) => sendToSession(s.id, t)}
                      onReview={() => setReviewing(true)}
                      onRewind={e.kind === "you" && done && s.reviewable ? () => setRewindAt(i) : undefined}
                    />
                  )}
                  {rewindAt === i && (
                    <RewindConfirm
                      session={s}
                      entry={i}
                      onDone={(msg, draft) => {
                        setRewindAt(null);
                        setNotice(msg);
                        if (draft) {
                          setText(draft);
                          setEditFocus((n) => n + 1);
                        }
                      }}
                    />
                  )}
                  {i === lastAgentText && (
                    <PlanPanel
                      session={s}
                      onDraft={(t) => {
                        setText(t);
                        setEditFocus((n) => n + 1);
                      }}
                    />
                  )}
                </Fragment>
              );
            })}
            {/* Plan with no agent text yet still needs a home. */}
            {lastAgentText < 0 && (
              <PlanPanel
                session={s}
                onDraft={(t) => {
                  setText(t);
                  setEditFocus((n) => n + 1);
                }}
              />
            )}

            {working && !s.question && (
              <p className="ak-status" role="status" aria-live="polite">
                <span className="shimmer-text text-[rgb(235_235_245/0.6)]">
                  {activity
                    ? activityLabel(activity, now, { elapsed: false })
                    : current
                      ? `${current}...`
                      : "Waiting for the next agent step"}
                </span>
                <span className="ak-timer mono">{clock(now - s.turnAt)}</span>
              </p>
            )}

            {s.question && (
              <div className="ak-ask ak-in" role="alertdialog" aria-label={`${s.agent} asks`}>
                <p>
                  {s.agent} wants to: <span className="font-medium text-white">{s.question.label}</span>
                </p>
                {s.question.detail && (
                  <code className="mono ak-scroll">{s.question.detail.replace(/\\\\/g, "\\")}</code>
                )}
                <div className="ak-chips">
                  <button type="button" onClick={() => answerQuestion(s.id, "allow")} className="ak-chip primary chip">
                    Allow{" "}
                    <kbd>
                      <i className="alt-pre">Alt </i>A
                    </kbd>
                  </button>
                  <button type="button" onClick={() => answerQuestion(s.id, "always")} className="ak-chip chip">
                    Allow this session{" "}
                    <kbd>
                      <i className="alt-pre">Alt </i>Y
                    </kbd>
                  </button>
                  <button type="button" onClick={() => answerQuestion(s.id, "deny")} className="ak-chip chip">
                    Deny{" "}
                    <kbd>
                      <i className="alt-pre">Alt </i>N
                    </kbd>
                  </button>
                </div>
              </div>
            )}

            {s.choose && <ChoiceCard open={s.choose} agent={s.agent} onAnswer={(text) => sendToSession(s.id, text)} />}

            {s.error && <p className="ak-err">{s.error}</p>}

            {done && !s.error && (
              <p className="ak-done ak-in">
                <span className="ok">✓</span>
                {[
                  s.tookMs !== null ? `Done in ${clock(s.tookMs)}` : "Done",
                  s.changes > 0 ? `${s.changes} ${s.changes === 1 ? "file" : "files"} changed` : null,
                  usedPct !== null ? `${usedPct}% of context used` : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </p>
            )}
            {done && (
              <div className="ak-chips">
                {s.reviewable && s.changes > 0 && (
                  <button type="button" onClick={() => setReviewing(true)} className="ak-chip primary chip">
                    Review {s.changes} {s.changes === 1 ? "change" : "changes"}
                    <kbd>
                      <i className="alt-pre">Alt </i>1
                    </kbd>
                  </button>
                )}
                <button type="button" onClick={() => void api.agentTerminal(s.id)} className="ak-chip chip">
                  Open in terminal{" "}
                  <kbd>
                    <i className="alt-pre">Alt </i>2
                  </kbd>
                </button>
                {editor && (
                  <button type="button" onClick={() => void api.agentOpenEditor(s.id)} className="ak-chip chip">
                    Open in {editor}{" "}
                    <kbd>
                      <i className="alt-pre">Alt </i>3
                    </kbd>
                  </button>
                )}
              </div>
            )}
            {offerCompact && s.usage && (
              <div className="ak-ask ak-in" role="status">
                <p>
                  Context is {Math.round((s.usage.used / s.usage.window) * 100)}% full. Compacting now keeps {s.agent}{" "}
                  quick and on track.
                </p>
                <div className="ak-chips">
                  <button
                    type="button"
                    onClick={() => sendToSession(s.id, "/compact")}
                    className="ak-chip primary chip"
                  >
                    Compact{" "}
                    <kbd>
                      <i className="alt-pre">Alt </i>K
                    </kbd>
                  </button>
                  <button type="button" onClick={() => dismissCompact(s.id)} className="ak-chip chip">
                    Not now
                  </button>
                </div>
              </div>
            )}
            {s.limit && <p className="ak-note ak-in">{limitNote(s.agent, s.limit)}</p>}
            {s.stuck && (
              <p className="ak-note ak-in flex items-center gap-2">
                <span className="min-w-0 flex-1">{s.agent} is going round in circles on this one.</span>
                <button type="button" onClick={() => void handOffSession(s)} className="ak-chip chip">
                  Hand off
                </button>
              </p>
            )}
            {notice && <p className="ak-note ak-in">{notice}</p>}
            {s.compaction && (
              <p role="status" className="ak-note">
                {s.compaction === "running"
                  ? "Compacting context…"
                  : s.compaction === "done"
                    ? "Context compacted"
                    : "Compaction failed; see the session error."}
              </p>
            )}
          </div>

          {s.restored || s.status === "ended" || (s.status === "failed" && !working) ? (
            <div>
              <ChatControls
                agent={s.agent}
                model={s.model}
                effort={s.effort}
                usage={s.usage}
                limit={s.limit}
                onModel={(m) => tuneSession(s.id, m, s.effort ?? null)}
                onEffort={(e) => tuneSession(s.id, s.model ?? null, e)}
              />
              <div className="ak-chips">
                <button
                  type="button"
                  onClick={() => void resumeSession(s.id).catch((e: unknown) => setNotice(String(e)))}
                  className="ak-chip primary chip"
                >
                  Resume
                </button>
                <span className="ak-note">
                  {s.restored ? "Duskie restarted since this ran." : "This session ended."} Resume carries on where it
                  stopped.
                </span>
              </div>
            </div>
          ) : (
            <Composer
              session={s}
              working={working}
              text={text}
              setText={setText}
              editFocus={editFocus}
              onSend={send}
              onBlur={flushView}
              onReview={() => setReviewing(true)}
              onClear={() => {
                void api
                  .agentProject(s.id)
                  .then((path) => startHere(path))
                  .catch((e) => setNotice(String(e)));
              }}
              onRewind={() => {
                const last = s.entries
                  .map((e, i) => (e.kind === "you" ? i : -1))
                  .filter((i) => i > 0)
                  .at(-1);
                if (last !== undefined) setRewindAt(last);
                else setNotice("There is no earlier message to go back to.");
              }}
            />
          )}
        </>
      )}
    </>
  );
}

/** The short tag in front of each step, as in a terminal log. */
export const STEP_TAG: Record<string, string> = {
  Read: "Read",
  Edit: "Edit",
  MultiEdit: "Edit",
  Write: "Write",
  NotebookEdit: "Edit",
  Bash: "Run",
  Grep: "Find",
  Glob: "Find",
  WebSearch: "Web",
  WebFetch: "Web",
  Task: "Agent",
  TodoWrite: "Plan",
  command: "Run",
  fileChange: "Edit",
  webSearch: "Web",
};

function EntryRow({
  entry: e,
  onRewind,
  onReview,
  choices,
  onPick,
}: {
  entry: Entry;
  onRewind?: () => void;
  onReview?: () => void;
  /** Last agent reply while idle: show selectable clarifying choices. */
  choices?: boolean;
  onPick?: (text: string) => void;
}) {
  if (e.kind === "review") {
    return (
      <div className="ak-chips">
        <span className="ak-note">{e.error ?? `${e.files} files changed`}</span>
        <button type="button" className="ak-chip chip" onClick={() => onReview?.()}>
          Review
        </button>
      </div>
    );
  }
  if (e.kind === "you") {
    if (!onRewind) return <div className="ak-um ak-in">{e.text}</div>;
    return (
      <div className="ak-um-wrap ak-in">
        <button type="button" onClick={onRewind} className="ak-rwb chip">
          ↺ Rewind to here
        </button>
        <div className="ak-um">{e.text}</div>
      </div>
    );
  }
  if (e.kind === "text") {
    // Always hide OPTION/STAT lines from prose; the selectable UI owns them.
    const parsed = extractChoices(e.text, !choices);
    return (
      <div className="grid gap-2">
        {parsed.body.trim() ? (
          <div className="ak-ans ak-in px-0">
            <Markdown text={parsed.body} />
          </div>
        ) : null}
        {choices && parsed.options.length > 0 && onPick && (
          <AnswerOptions options={parsed.options} multi={parsed.multi} onPick={onPick} />
        )}
      </div>
    );
  }
  const st = e.step;
  return (
    <>
      <div className="ak-tg ak-in" data-s={st.state}>
        <span className="ak-k mono">{STEP_TAG[st.tool] ?? st.tool.slice(0, 5)}</span>
        <span
          className={`shrink-0 ${st.state === "running" ? "text-white" : st.state === "cancelled" ? "text-[var(--i3)]" : ""}`}
        >
          {st.label}
        </span>
        {st.detail && <span className="d mono">{st.detail}</span>}
      </div>
      {st.output && STEP_TAG[st.tool] === "Run" && <Output step={st} />}
    </>
  );
}

/** A command's output: collapsed to a line count until opened. */
function Output({ step }: { step: Step }) {
  const [open, setOpen] = useState(false);
  const lines = (step.output ?? "").split("\n");
  const count = lines.length;
  return (
    <div className="ak-term ak-in">
      <button type="button" onClick={() => setOpen((o) => !o)} className="chip flex items-center gap-1.5">
        <Icon name="chevron" size={12} className={open ? "" : "-rotate-90"} />
        {open ? "Show less" : `${count} ${count === 1 ? "line" : "lines"}`}
      </button>
      {open && <pre className="mono">{step.output ?? ""}</pre>}
    </div>
  );
}

/** "Rewind to before this message?" with how many files go back. */
function RewindConfirm({
  session: s,
  entry,
  onDone,
}: {
  session: Session;
  entry: number;
  /** `draft` is the removed user message, put back in the composer to edit. */
  onDone: (message: string | null, draft?: string) => void;
}) {
  const [files, setFiles] = useState<number | null>(null);
  useEffect(() => {
    void api
      .agentRewindPreview(s.id, messageIndex(s, entry))
      .then(setFiles)
      .catch(() => setFiles(0));
  }, [s, entry]);
  const said = s.entries[entry]?.kind === "you" ? (s.entries[entry] as { text: string }).text : "";
  const go = () =>
    void rewindSession(s.id, entry)
      .then((n) => {
        const filesNote = n === 0 ? "No files to put back" : `${n} ${n === 1 ? "file" : "files"} put back`;
        onDone(`↺ Rewound · ${filesNote} · edit and send`, said);
      })
      .catch((e: unknown) => onDone(String(e)));
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        go();
      } else if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onDone(null);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });
  return (
    <div
      className="ak-ask ak-in"
      style={{
        background: "rgb(255 255 255 / 0.05)",
        color: "var(--ink)",
        boxShadow: "inset 0 0 0 0.5px rgb(255 255 255 / 0.12)",
      }}
    >
      <p>
        Rewind to before “{said.length > 60 ? `${said.slice(0, 57)}...` : said}”?{" "}
        {files === null
          ? ""
          : files === 0
            ? "No files changed since then."
            : `This puts back ${files} ${files === 1 ? "file" : "files"} the agent changed since.`}{" "}
        Your message opens in the box to edit.
      </p>
      <div className="ak-chips">
        <button type="button" onClick={go} className="ak-chip primary chip">
          Rewind <kbd>Enter</kbd>
        </button>
        <button type="button" onClick={() => onDone(null)} className="ak-chip chip">
          Cancel <kbd>Esc</kbd>
        </button>
      </div>
    </div>
  );
}

type Slash = AgentCommand;

/** Where you type to the agent: / lists commands, @ finds files in the project. */
export function Composer({
  session: s,
  working,
  text,
  setText,
  editFocus = 0,
  onSend,
  onClear,
  onReview,
  onRewind,
  onBlur,
}: {
  session: Session;
  working: boolean;
  text: string;
  setText: (t: string) => void;
  /** After rewind: focus the box and select the restored draft (Cursor-style edit). */
  editFocus?: number;
  onSend: () => void;
  onClear: () => void;
  onReview: () => void;
  onRewind: () => void;
  onBlur?: () => void;
}) {
  const [commands, setCommands] = useState<Slash[]>(commandCatalog(s.agent, []));
  const [panel, setPanel] = useState<{ name: string; at: number }>();
  const [notice, setNotice] = useState<string | null>(null);
  const [terminal, setTerminal] = useState<string | null>(null);
  const [files, setFiles] = useState<string[]>([]);
  const [sel, setSel] = useState(0);
  const [float, setFloat] = useState<{ left: number; bottom: number; maxWidth: number } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: focus again when the session changes
  useEffect(() => {
    requestAnimationFrame(() => inputRef.current?.focus({ preventScroll: true }));
  }, [s.id]);
  useEffect(() => {
    if (!editFocus) return;
    requestAnimationFrame(() => {
      const el = inputRef.current;
      if (!el) return;
      el.focus({ preventScroll: true });
      el.select();
    });
  }, [editFocus]);
  useEffect(() => {
    void api
      .agentCommands(s.id)
      .then((cmds) => setCommands(commandCatalog(s.agent, cmds)))
      .catch(() => setCommands(commandCatalog(s.agent, [])));
  }, [s.id, s.agent]);
  const slash = text.startsWith("/") && !text.includes(" ");
  const mention = /(^|\s)@([^\s@]*)$/.exec(text);
  const query = mention?.[2] ?? null;
  useEffect(() => {
    if (query === null) {
      setFiles([]);
      return;
    }
    let live = true;
    const id = setTimeout(() => {
      void api
        .agentFiles(s.id, query)
        .then((f) => live && setFiles(f))
        .catch(() => undefined);
    }, 80);
    return () => {
      live = false;
      clearTimeout(id);
    };
  }, [query, s.id]);
  const shownCommands = slash ? commands.filter((c) => c.name.startsWith(text.toLowerCase())) : [];
  const count = slash ? shownCommands.length : query !== null ? files.length : 0;
  const at = Math.min(sel, Math.max(count - 1, 0));
  const placePop = () => {
    const el = wrapRef.current;
    if (!el || count === 0) {
      setFloat(null);
      setOverlayHit(null);
      return;
    }
    const r = el.getBoundingClientRect();
    // Cap at the composer / viewport; actual width comes from content.
    const maxWidth = Math.max(200, Math.min(r.width, window.innerWidth - 16));
    const left = Math.min(Math.max(8, r.left), window.innerWidth - maxWidth - 8);
    const bottom = window.innerHeight - r.top + 6;
    const want = Math.min(200, 28 + count * 36);
    setFloat((prev) =>
      prev && prev.left === left && prev.bottom === bottom && prev.maxWidth === maxWidth
        ? prev
        : { left, bottom, maxWidth },
    );
    // Estimate until the pop mounts and the next effect measures it.
    setOverlayHit({
      x: left,
      y: Math.max(8, r.top - 6 - want),
      width: maxWidth,
      height: want + 8,
    });
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-place when the typed text or the list changes
  useLayoutEffect(() => {
    placePop();
    if (count === 0) return;
    // Composer sits outside the timeline scroller; only the viewport size
    // moves it. Listening to scroll re-placed the pop on every wheel tick
    // inside the menu and fought the list.
    window.addEventListener("resize", placePop);
    return () => {
      window.removeEventListener("resize", placePop);
      setOverlayHit(null);
    };
  }, [count, slash, query, text]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: measure again when the list length changes
  useLayoutEffect(() => {
    if (!float || !popRef.current) return;
    const r = popRef.current.getBoundingClientRect();
    const hit = { x: r.left, y: r.top, width: r.width, height: r.height };
    setOverlayHit(hit);
  }, [float, count]);
  const runCommand = (c: Slash, args = "") => {
    setText("");
    setNotice(null);
    if (c.execution === "terminal") {
      setTerminal(`${c.name}${args ? ` ${args}` : ""}`);
      return;
    }
    if (c.execution === "provider") {
      sendToSession(s.id, `${c.name}${args ? ` ${args}` : ""}`);
      return;
    }
    const mode = c.name.slice(1) as AgentMode;
    if (["plan", "ask", "edit", "full"].includes(mode)) {
      void setSessionMode(s.id, mode);
      return;
    }
    const fail = (e: unknown) => setNotice(String(e));
    switch (c.name) {
      case "/clear":
        onClear();
        break;
      case "/new":
        useAgents.setState({ current: null });
        break;
      case "/rewind":
        onRewind();
        break;
      case "/diff":
        onReview();
        break;
      case "/permissions":
        if (["plan", "ask", "edit", "full"].includes(args)) void setSessionMode(s.id, args as AgentMode);
        else
          setNotice(
            `Selected: ${s.mode}. Use the mode buttons above or Ctrl+. to cycle. Codex Edit allows workspace commands and asks for escalation; Full disables the sandbox.`,
          );
        break;
      case "/model":
        if (args) tuneSession(s.id, args === "default" ? null : args, s.effort ?? null);
        else setPanel({ name: "model", at: Date.now() });
        break;
      case "/effort":
        if (args && ["off", "low", "medium", "high", "max", "xhigh", "default"].includes(args))
          tuneSession(s.id, s.model ?? null, args === "default" ? null : args);
        else if (args) setNotice("Unsupported effort. Choose off, low, medium, high, max, or default.");
        else setPanel({ name: "effort", at: Date.now() });
        break;
      case "/usage":
        setPanel({ name: "usage", at: Date.now() });
        break;
      case "/status":
        setNotice(
          `${s.agent} · ${s.status} · selected mode ${s.mode} · model ${s.model ?? "default"} · effort ${s.effort ?? "default"} · session ${s.id}`,
        );
        break;
      case "/rename":
        if (args) renameSession(s.id, args);
        else setNotice("Use /rename followed by a title, or edit the session title above.");
        break;
      case "/copy": {
        const e = [...s.entries].reverse().find((e) => e.kind === "text");
        if (e?.kind === "text")
          void navigator.clipboard
            .writeText(e.text)
            .then(() => setNotice("Response copied"))
            .catch(fail);
        break;
      }
      case "/resume":
        void resumeSession(s.id).catch(fail);
        break;
      case "/compact":
        if (working) {
          setNotice("Wait for the current turn to finish before compacting.");
          break;
        }
        if (s.agent === "Codex") void api.agentControl(s.id, "thread/compact/start").catch(fail);
        else if (s.agent === "Claude Code") sendToSession(s.id, "/compact");
        else setNotice("Compaction is not supported by this provider.");
        break;
      default:
        setNotice(`Unsupported control: ${c.name}`);
    }
  };
  const submit = () => {
    const match = /^\/(\S+)(?:\s+(.*))?$/.exec(text.trim());
    if (!match) {
      onSend();
      return;
    }
    const command = commands.find((c) => c.name === `/${match[1].toLowerCase()}`);
    if (command) runCommand(command, match[2] ?? "");
    else setNotice(`Unsupported command /${match[1]}. Use / to see supported controls.`);
  };
  const pickFile = (f: string) => {
    if (!mention) return;
    setText(`${text.slice(0, text.length - mention[2].length - 1)}@${f} `);
    inputRef.current?.focus({ preventScroll: true });
  };
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && e.key === ".") {
      e.preventDefault();
      e.stopPropagation();
      void setSessionMode(s.id, nextAgentMode(s.mode));
      return;
    }
    if (count && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      setSel((at + (e.key === "ArrowDown" ? 1 : -1) + count) % count);
    } else if (count && (e.key === "Enter" || e.key === "Tab")) {
      e.preventDefault();
      if (slash) {
        const c = shownCommands[at];
        if (c) runCommand(c);
      } else if (files[at]) pickFile(files[at]);
      setSel(0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      submit();
    } else if (e.key === "Escape" && count) {
      e.preventDefault();
      setText(slash ? "" : text.replace(/(^|\s)@[^\s@]*$/, "$1"));
      setSel(0);
    }
  };
  let lastGroup = "";
  const pop =
    count > 0 && float
      ? createPortal(
          <div
            ref={popRef}
            className="ak-pop ak-pop-float ak-in"
            role="listbox"
            aria-label={slash ? "Commands" : "Files"}
            style={{
              position: "fixed",
              left: float.left,
              bottom: float.bottom,
              width: "max-content",
              maxWidth: float.maxWidth,
              zIndex: 100,
            }}
          >
            {slash ? (
              shownCommands.map((c, i) => {
                const head = c.group !== lastGroup;
                lastGroup = c.group;
                return (
                  <Fragment key={c.name}>
                    {head && <p className="g">{c.group}</p>}
                    <button
                      type="button"
                      role="option"
                      aria-selected={i === at}
                      data-sel={i === at}
                      ref={scrollIfActive(i === at)}
                      onMouseMove={() => setSel(i)}
                      onClick={() => runCommand(c)}
                      className="it"
                    >
                      <span className="c mono">{c.name}</span>
                      <span className="d">{c.description}</span>
                    </button>
                  </Fragment>
                );
              })
            ) : (
              <>
                <p className="g">Files in {s.project}</p>
                {files.map((f, i) => (
                  <button
                    key={f}
                    type="button"
                    role="option"
                    aria-selected={i === at}
                    data-sel={i === at}
                    ref={scrollIfActive(i === at)}
                    onMouseMove={() => setSel(i)}
                    onClick={() => pickFile(f)}
                    className="it"
                  >
                    <span className="c mono">{f.split("/").pop()}</span>
                    <span className="d">{f.split("/").slice(0, -1).join("/")}</span>
                  </button>
                ))}
              </>
            )}
          </div>,
          document.body,
        )
      : null;
  return (
    <>
      {pop}
      {notice && (
        <p role="status" className="ak-note">
          {notice}
        </p>
      )}
      {terminal && (
        <div className="ak-chips">
          <span className="ak-note">
            Terminal only: <code className="mono">{terminal}</code>
          </span>
          <button
            type="button"
            className="ak-chip chip"
            onClick={() => void api.agentTerminal(s.id).catch((e) => setNotice(String(e)))}
          >
            Open in terminal
          </button>
          <button
            type="button"
            className="ak-chip chip"
            onClick={() => void navigator.clipboard.writeText(terminal).catch((e) => setNotice(String(e)))}
          >
            Copy command
          </button>
        </div>
      )}
      <div ref={wrapRef} className="ak-composer ak-two relative">
        <div className="ak-row">
          <input
            ref={inputRef}
            value={text}
            onChange={(e) => {
              setText(e.target.value);
              setSel(0);
            }}
            onBlur={() => onBlur?.()}
            onKeyDown={onKey}
            placeholder={working ? "Steer it, it reads this next" : "Ask for more, / for commands, @ for files"}
          />
        </div>
        <ChatControls
          agent={s.agent}
          model={s.model}
          effort={s.effort}
          onModel={(m) => tuneSession(s.id, m, s.effort ?? null)}
          onEffort={(e) => tuneSession(s.id, s.model ?? null, e)}
          usage={s.usage}
          panel={panel}
          onRefresh={
            s.agent === "Codex"
              ? () => void api.agentControl(s.id, "account/rateLimits/read").catch((e) => setNotice(String(e)))
              : undefined
          }
          localModel={s.localModel}
          limit={s.limit}
          onCompact={
            (s.agent === "Claude Code" || s.agent === "Codex") && !working
              ? () => runCommand({ name: "/compact", description: "", group: "Duskie", execution: "control" })
              : undefined
          }
          keys={working ? "Esc interrupt" : "Enter send"}
        />
      </div>
    </>
  );
}

/** The session's name: click to rename, Enter keeps it, Esc puts it back. */
export function NameField({ id, title }: { id: string; title: string }) {
  const [v, setV] = useState(title);
  useEffect(() => setV(title), [title]);
  return (
    <input
      aria-label="Session name"
      className="ak-title ak-name"
      value={v}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => (v.trim() ? renameSession(id, v) : setV(title))}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        else if (e.key === "Escape") {
          e.stopPropagation();
          setV(title);
          requestAnimationFrame(() => (document.activeElement as HTMLElement | null)?.blur());
        }
      }}
    />
  );
}

/** "You have used 85% of Claude Code's 5-hour limit. It resets at 3:40 PM." */
export function limitNote(agent: string, l: NonNullable<Session["limit"]>): string {
  const span =
    l.window === "five_hour" ? "5-hour limit" : l.window.startsWith("seven_day") ? "weekly limit" : "usage limit";
  const resets = l.resetsAt
    ? ` It resets at ${new Date(l.resetsAt * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}.`
    : "";
  if (l.status === "rejected")
    return `${agent} has used up its ${span}.${resets} Ask mode uses another model meanwhile.`;
  const pct = l.used !== null ? `${Math.round(l.used <= 1 ? l.used * 100 : l.used)}% of ` : "most of ";
  return `You have used ${pct}${agent}'s ${span}.${resets}`;
}

/** The agent's own multiple-choice question: tap an option to answer. With
 * several questions, each is answered in turn; "Several" questions allow
 * more than one pick. */
function ChoiceCard({ open, agent, onAnswer }: { open: OpenChoice; agent: string; onAnswer: (text: string) => void }) {
  const [picks, setPicks] = useState<string[][]>([]);
  const at = picks.length;
  const [multi, setMulti] = useState<string[]>([]);
  const q = open.questions[at];
  if (!q) return null;
  const next = (chosen: string[]) => {
    const all = [...picks, chosen];
    setMulti([]);
    if (all.length >= open.questions.length) onAnswer(answerText(open, all));
    else setPicks(all);
  };
  return (
    <section className="ak-ask ak-in" aria-label={`${agent} asks`}>
      <p>
        {agent} asks: <span className="font-medium text-white">{q.question}</span>
      </p>
      <div className="ak-chips">
        {q.options.map((o) => {
          const on = multi.includes(o.label);
          return (
            <button
              key={o.label}
              type="button"
              title={o.description}
              aria-pressed={q.multiSelect ? on : undefined}
              onClick={() =>
                q.multiSelect
                  ? setMulti(on ? multi.filter((m) => m !== o.label) : [...multi, o.label])
                  : next([o.label])
              }
              className={`ak-chip chip ${on ? "primary" : ""}`}
            >
              {o.label}
            </button>
          );
        })}
        {q.multiSelect && (
          <button type="button" onClick={() => next(multi)} className="ak-chip primary chip">
            Done
          </button>
        )}
      </div>
    </section>
  );
}

type Group = "Needs you" | "Running" | "Ready to review" | "Done" | "Stopped";
const GROUPS: Group[] = ["Needs you", "Running", "Ready to review", "Done", "Stopped"];

function groupOf(s: Session): Group {
  if (s.status === "waiting" || s.question || s.choose) return "Needs you";
  if (s.status === "working") return "Running";
  if (s.status === "failed") return "Stopped";
  if (!s.finished && s.changes > 0) return "Ready to review";
  return "Done";
}

const MODE_WORD: Record<string, string> = { plan: "Plan", ask: "Ask", edit: "Edit", full: "Full" };

function ago(ms: number): string {
  const m = Math.max(0, Math.round(ms / 60_000));
  if (m < 60) return `${m} min`;
  const h = Math.round(m / 60);
  return h < 24 ? `${h} h` : `${Math.round(h / 24)} d`;
}

/** Agents home: every session grouped by what it needs, newest first. */
function SessionList({ sessions }: { sessions: Session[] }) {
  const now = useNow(30_000);
  const running = sessions.filter((s) => s.status === "working").length;
  const need = sessions.filter((s) => groupOf(s) === "Needs you").length;
  return (
    <div className="ag-home">
      <p className="ag-count">
        {running} of {sessions.length} running
        {need > 0 && <span className="ag-need"> · {need} need you</span>}
      </p>
      {GROUPS.map((g) => {
        const list = sessions.filter((s) => groupOf(s) === g).sort((a, b) => b.turnAt - a.turnAt);
        if (list.length === 0) return null;
        return (
          <div key={g} className="ag-group">
            <p className="ak-group">
              {g} <span className="ag-n">{list.length}</span>
            </p>
            {list.map((s) => (
              <button
                key={s.id}
                type="button"
                className="ag-row chip"
                onClick={() => useAgents.setState({ current: s.id })}
              >
                <AgentMark agent={s.agent} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate">{s.title}</span>
                  <span className="ag-sub block truncate">
                    {s.project} · {MODE_WORD[s.mode] ?? s.mode} · {ago(now - s.turnAt)}
                  </span>
                </span>
                <span className="ag-st" data-g={g}>
                  {g === "Needs you"
                    ? "Needs you"
                    : g === "Running"
                      ? "Working"
                      : g === "Ready to review"
                        ? "Ready to review"
                        : g}
                </span>
              </button>
            ))}
          </div>
        );
      })}
    </div>
  );
}
