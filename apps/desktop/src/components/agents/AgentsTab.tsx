"use client";

// The Agents tab: start Claude Code or Codex in a project, watch it work
// step by step, answer its questions here, steer it, and keep or undo each
// change when it is done.

import { type CSSProperties, Fragment, useEffect, useMemo, useRef, useState } from "react";
import {
  answerQuestion,
  closeSession,
  type Entry,
  type Session,
  sendToSession,
  startSession,
  useAgents,
} from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import { Markdown } from "@/lib/markdown";
import type { AgentMode, Agents } from "@/lib/types";
import { KeyHint } from "../ask/parts";
import { Icon } from "../Icon";
import { Review } from "./Review";

const MODES: { id: AgentMode; label: string; note: string }[] = [
  { id: "plan", label: "Plan", note: "Reads and plans, changes nothing" },
  { id: "ask", label: "Ask", note: "Asks before every change" },
  { id: "edit", label: "Edit", note: "Edits files, asks before commands" },
  { id: "full", label: "Full", note: "Does everything without asking" },
];

export function AgentsTab({ keys, maxHeight }: { keys: boolean; maxHeight: number }) {
  const sessions = useAgents((s) => s.sessions);
  const current = useAgents((s) => s.current);
  const session = sessions.find((s) => s.id === current) ?? null;
  return session ? (
    <SessionView key={session.id} session={session} sessions={sessions} keys={keys} maxHeight={maxHeight} />
  ) : (
    <NewSession sessions={sessions} />
  );
}

/** Every session as a chip, then + New. */
function SessionChips({ sessions, current }: { sessions: Session[]; current: string | null }) {
  if (sessions.length === 0) return null;
  return (
    <div className="ak-sess">
      {sessions.map((s) => (
        <button
          key={s.id}
          type="button"
          aria-pressed={s.id === current}
          onClick={() => useAgents.setState({ current: s.id })}
          className="ak-sp chip max-w-56"
        >
          <span className="ak-sd" data-s={s.status} role="img" aria-label={s.status} />
          <span className="truncate">{s.title}</span>
          <em>
            {s.project}
            {s.agent === "Codex" ? " · Codex" : ""}
          </em>
        </button>
      ))}
      <button
        type="button"
        aria-pressed={false}
        onClick={() => useAgents.setState({ current: null })}
        className="ak-sp chip text-[rgb(235_235_245/0.36)]"
      >
        + New
      </button>
    </div>
  );
}

function NewSession({ sessions }: { sessions: Session[] }) {
  const { data: agents } = useCached<Agents>("agents", api.agentsStatus);
  const { data: projects } = useCached<{ name: string; path: string }[]>("projects", api.projectsList);
  const [agent, setAgent] = useState<string>("");
  const [path, setPath] = useState("");
  const [mode, setMode] = useState<AgentMode>("edit");
  const [prompt, setPrompt] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const choices = [
    ...(agents?.claudeCode ? [{ id: "claude_code", name: "Claude Code" }] : []),
    ...(agents?.codex ? [{ id: "codex", name: "Codex" }] : []),
  ];
  const pickedAgent = agent || choices[0]?.id || "";
  const pickedPath = path || projects?.[0]?.path || "";
  useEffect(() => {
    requestAnimationFrame(() => inputRef.current?.focus());
  }, []);
  const go = () => {
    if (!prompt.trim() || !pickedPath || busy) return;
    setBusy(true);
    setError(null);
    startSession(pickedAgent, pickedPath, prompt.trim(), mode)
      .then(() => setPrompt(""))
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setBusy(false));
  };
  return (
    <>
      <div className="ak-head">
        <span className="ak-title">New session</span>
      </div>
      <SessionChips sessions={sessions} current={null} />
      {agents && choices.length === 0 ? (
        <p className="ak-group py-1 text-[13px]">
          Install Claude Code or Codex to run agents here. Settings &gt; AI shows how.
        </p>
      ) : (
        <>
          <div className="ak-meta">
            {choices.length > 1 ? (
              <select
                aria-label="Agent"
                value={pickedAgent}
                onChange={(e) => setAgent(e.target.value)}
                className="ak-mi b chip"
              >
                {choices.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.name}
                  </option>
                ))}
              </select>
            ) : (
              <span className="ak-mi b">{choices[0]?.name ?? "Claude Code"}</span>
            )}
            <select
              aria-label="Project"
              value={pickedPath}
              onChange={(e) => setPath(e.target.value)}
              className="ak-mi chip max-w-44 truncate"
            >
              {(projects ?? []).map((p) => (
                <option key={p.path} value={p.path}>
                  {p.name}
                </option>
              ))}
            </select>
            <ModeSwitch mode={mode} onChange={setMode} />
          </div>
          <div className="ak-composer">
            <input
              ref={inputRef}
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  go();
                }
              }}
              placeholder="What should it do?"
            />
            <span className="ckeys mono">{busy ? "Starting..." : "Enter start"}</span>
          </div>
          {error && <p className="ak-err">{error}</p>}
        </>
      )}
    </>
  );
}

function ModeSwitch({ mode, onChange }: { mode: AgentMode; onChange?: (m: AgentMode) => void }) {
  return (
    <fieldset aria-label="Mode" className="ak-seg border-0">
      {MODES.map((m) => (
        <button
          key={m.id}
          type="button"
          title={m.note}
          aria-pressed={mode === m.id}
          disabled={!onChange}
          onClick={() => onChange?.(m.id)}
          className="chip"
        >
          {m.label}
        </button>
      ))}
    </fieldset>
  );
}

/** How much of the context window is used, as a small ring. */
function ContextRing({ used, window }: { used: number; window: number }) {
  const p = Math.round(Math.min(1, used / window) * 100);
  return (
    <span className="ak-ring mono" title={`${p}% of the context used`}>
      <i style={{ "--p": p } as CSSProperties} aria-hidden="true" />
      {p}%
    </span>
  );
}

/** "1:12", or "8 s" under a minute. */
function clock(ms: number): string {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s} s` : `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function SessionView({
  session: s,
  sessions,
  keys,
  maxHeight,
}: {
  session: Session;
  sessions: Session[];
  keys: boolean;
  maxHeight: number;
}) {
  const [text, setText] = useState("");
  const [reviewing, setReviewing] = useState(false);
  const scroll = useRef<HTMLDivElement>(null);
  const now = useNow(250);
  const working = s.status === "working" || s.status === "waiting";
  const done = s.status === "idle" || s.status === "ended" || s.status === "failed";
  const current = useMemo(() => {
    const step = [...s.entries].reverse().find((e) => e.kind === "step" && e.step.state === "running");
    return step && step.kind === "step" ? step.step.label : null;
  }, [s.entries]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: follow new entries
  useEffect(() => {
    const el = scroll.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [s.entries.length, s.plan.length, s.question, s.status]);

  // Alt 1 reviews, Alt 2 (or Alt T) carries on in a terminal.
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
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [s.id, done, s.reviewable, s.changes]);

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
    setText("");
  };
  const planDone = s.plan.filter((p) => p.status === "completed").length;
  const planAt = Math.min(planDone + 1, s.plan.length);
  // The plan reads after the first thing said, where the agent made it.
  const firstSaid = s.entries.findIndex((e) => e.kind === "text");
  const planAfter = firstSaid >= 0 ? firstSaid : 0;
  const usedPct = s.usage ? Math.round(Math.min(1, s.usage.used / s.usage.window) * 100) : null;

  return (
    <>
      <div className="ak-head">
        <span className="ak-title">{s.title}</span>
        {s.usage && <ContextRing used={s.usage.used} window={s.usage.window} />}
        {working ? (
          <button type="button" onClick={() => void api.agentStop(s.id)} className="ak-stop chip">
            Stop <kbd>Esc</kbd>
          </button>
        ) : (
          <button
            type="button"
            aria-label="Close session"
            title="Close session"
            onClick={() => closeSession(s.id)}
            className="ak-ibtn chip"
          >
            <Icon name="close" size={13} />
          </button>
        )}
      </div>
      <SessionChips sessions={sessions} current={s.id} />
      <div className="ak-meta">
        <span className="ak-mi b">{s.agent}</span>
        <span className="ak-mi">{s.project}</span>
        {s.branch && <span className="ak-mi">⎇ {s.branch}</span>}
        <ModeSwitch mode={s.mode} />
      </div>

      {reviewing ? (
        <Review session={s} maxHeight={maxHeight} onDone={() => setReviewing(false)} />
      ) : (
        <>
          <div ref={scroll} className="ak-tl ak-scroll" style={{ maxHeight: maxHeight - 40 }}>
            {s.entries.map((e, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: entries only ever append
              <Fragment key={i}>
                <EntryRow entry={e} />
                {i === planAfter && s.plan.length > 0 && (
                  <div className="ak-todo ak-in">
                    <div className="h">
                      <span>Plan</span>
                      <span>
                        {planAt} of {s.plan.length}
                      </span>
                    </div>
                    {s.plan.map((p) => (
                      <div key={p.text} className="ak-td" data-s={p.status}>
                        <span className="ak-cb" aria-hidden="true" />
                        {p.text}
                      </div>
                    ))}
                  </div>
                )}
              </Fragment>
            ))}

            {working && !s.question && (
              <p className="ak-status" role="status" aria-live="polite">
                <span className="shimmer-text text-[rgb(235_235_245/0.6)]">
                  {current ? `${current}...` : "Working..."}
                </span>
                <span className="ak-timer mono">{clock(now - s.turnAt)}</span>
              </p>
            )}

            {s.question && (
              <div className="ak-ask ak-in" role="alertdialog" aria-label={`${s.agent} asks`}>
                <p>
                  {s.agent} wants to: <span className="font-medium text-white">{s.question.label}</span>
                </p>
                {s.question.detail && <code className="mono">{s.question.detail}</code>}
                <div className="ak-chips">
                  <button type="button" onClick={() => answerQuestion(s.id, "allow")} className="ak-chip primary chip">
                    Allow <kbd>Alt A</kbd>
                  </button>
                  <button type="button" onClick={() => answerQuestion(s.id, "always")} className="ak-chip chip">
                    Always <kbd>Alt Y</kbd>
                  </button>
                  <button type="button" onClick={() => answerQuestion(s.id, "deny")} className="ak-chip chip">
                    No <kbd>Alt N</kbd>
                  </button>
                </div>
              </div>
            )}

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
                {s.reviewable && (
                  <button type="button" onClick={() => setReviewing(true)} className="ak-chip primary chip">
                    {s.changes > 0 ? `Review ${s.changes} ${s.changes === 1 ? "change" : "changes"}` : "Review changes"}
                    <kbd>Alt 1</kbd>
                  </button>
                )}
                <button type="button" onClick={() => void api.agentTerminal(s.id)} className="ak-chip chip">
                  Open in terminal <kbd>Alt 2</kbd>
                </button>
              </div>
            )}
          </div>

          {s.status !== "ended" && (
            <div className="ak-composer">
              <input
                value={text}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    send();
                  }
                }}
                placeholder={working ? "Steer it, it reads this next" : "Ask for more"}
              />
              <span className="ckeys mono">{working ? "Esc interrupt" : "Enter send"}</span>
              <KeyHint show={keys}>Alt T</KeyHint>
            </div>
          )}
        </>
      )}
    </>
  );
}

/** The short tag in front of each step, as in a terminal log. */
const STEP_TAG: Record<string, string> = {
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

function EntryRow({ entry: e }: { entry: Entry }) {
  if (e.kind === "you") return <div className="ak-um ak-in">{e.text}</div>;
  if (e.kind === "text") {
    return (
      <div className="ak-ans ak-in px-0">
        <Markdown text={e.text} />
      </div>
    );
  }
  const st = e.step;
  return (
    <div className="ak-tg ak-in" data-s={st.state}>
      <span className="ak-k mono">{STEP_TAG[st.tool] ?? st.tool.slice(0, 5)}</span>
      <span className={`shrink-0 ${st.state === "running" ? "text-white" : ""}`}>{st.label}</span>
      {st.detail && <span className="d mono">{st.detail}</span>}
    </div>
  );
}
