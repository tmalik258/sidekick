"use client";

// The Agents tab: start Claude Code or Codex in a project, watch it work
// step by step, answer its questions here, steer it, and keep or undo each
// change when it is done.

import { type CSSProperties, Fragment, type KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import {
  answerQuestion,
  COMPACT_AT,
  closeSession,
  dismissCompact,
  type Entry,
  messageIndex,
  resumeSession,
  rewindSession,
  type Session,
  type Step,
  sendToSession,
  startSession,
  useAgents,
} from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import { Markdown } from "@/lib/markdown";
import type { AgentMode, Agents, EditorList } from "@/lib/types";
import { KeyHint } from "../ask/parts";
import { Icon } from "../Icon";
import { Select } from "../settings/ui";
import { Tip } from "../Tip";
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
        className="ak-sp chip text-[rgb(235_235_245/0.45)]"
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
  // Where the agent and project menus open, so the island grows to show them.
  const [menuSlot, setMenuSlot] = useState<HTMLDivElement | null>(null);
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
    requestAnimationFrame(() => inputRef.current?.focus({ preventScroll: true }));
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
              <span className="ak-mi b">
                <Select
                  variant="plain"
                  portal={menuSlot}
                  label="Agent"
                  value={pickedAgent}
                  onChange={setAgent}
                  options={choices.map((c) => ({
                    value: c.id,
                    label: c.name,
                    sub: c.id === "codex" ? "Your ChatGPT plan" : "Your Claude plan",
                    icon: c.id === "codex" ? "X" : "C",
                    color: c.id === "codex" ? "#10a37f" : "#d97757",
                  }))}
                />
              </span>
            ) : (
              <span className="ak-mi b">{choices[0]?.name ?? "Claude Code"}</span>
            )}
            <span className="ak-mi max-w-44">
              <Select
                variant="plain"
                portal={menuSlot}
                label="Project"
                value={pickedPath}
                onChange={setPath}
                group="Project"
                options={(projects ?? []).map((p) => ({
                  value: p.path,
                  label: p.name,
                  sub: p.path.replace(/^[A-Za-z]:[\\/]Users[\\/][^\\/]+/, "~").replace(/^\/home\/[^/]+/, "~"),
                  icon: p.name.slice(0, 1).toUpperCase(),
                }))}
              />
            </span>
            <ModeSwitch mode={mode} onChange={setMode} />
          </div>
          <div ref={setMenuSlot} />
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
        <Tip key={m.id} label={m.note}>
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
function ContextRing({ used, window, onCompact }: { used: number; window: number; onCompact?: () => void }) {
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
  const [rewindAt, setRewindAt] = useState<number | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const { data: editors } = useCached<EditorList>("editors", api.editorsList);
  const editor = editors?.editors.find((e) => e.id === editors.current)?.name ?? null;
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
        {s.usage && (
          <ContextRing
            used={s.usage.used}
            window={s.usage.window}
            onCompact={
              s.agent === "Claude Code" && s.status !== "working" && s.status !== "ended"
                ? () => sendToSession(s.id, "/compact")
                : undefined
            }
          />
        )}
        {working ? (
          <button type="button" onClick={() => void api.agentStop(s.id)} className="ak-stop chip">
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
        <SessionMemory id={s.id} live={s.status !== "ended" && s.status !== "failed" && !s.restored} />
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
                <EntryRow
                  entry={e}
                  onRewind={e.kind === "you" && done && s.reviewable ? () => setRewindAt(i) : undefined}
                />
                {rewindAt === i && (
                  <RewindConfirm
                    session={s}
                    entry={i}
                    onDone={(msg) => {
                      setRewindAt(null);
                      setNotice(msg);
                    }}
                  />
                )}
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
            {notice && <p className="ak-note ak-in">{notice}</p>}
          </div>

          {s.restored || s.status === "ended" || (s.status === "failed" && !working) ? (
            <div className="ak-chips">
              <button
                type="button"
                onClick={() => void resumeSession(s.id).catch((e: unknown) => setNotice(String(e)))}
                className="ak-chip primary chip"
              >
                Resume
              </button>
              <span className="ak-note">
                {s.restored ? "Sidekick restarted since this ran." : "This session ended."} Resume carries on where it
                stopped.
              </span>
            </div>
          ) : (
            <Composer
              session={s}
              working={working}
              text={text}
              setText={setText}
              onSend={send}
              onClear={() => {
                closeSession(s.id);
              }}
              onRewind={() => {
                const last = s.entries
                  .map((e, i) => (e.kind === "you" ? i : -1))
                  .filter((i) => i > 0)
                  .at(-1);
                if (last !== undefined) setRewindAt(last);
                else setNotice("There is no earlier message to go back to.");
              }}
              keys={keys}
            />
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

function EntryRow({ entry: e, onRewind }: { entry: Entry; onRewind?: () => void }) {
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
    return (
      <div className="ak-ans ak-in px-0">
        <Markdown text={e.text} />
      </div>
    );
  }
  const st = e.step;
  return (
    <>
      <div className="ak-tg ak-in" data-s={st.state}>
        <span className="ak-k mono">{STEP_TAG[st.tool] ?? st.tool.slice(0, 5)}</span>
        <span className={`shrink-0 ${st.state === "running" ? "text-white" : ""}`}>{st.label}</span>
        {st.detail && <span className="d mono">{st.detail}</span>}
      </div>
      {st.output && STEP_TAG[st.tool] === "Run" && <Output step={st} />}
    </>
  );
}

/** A command's output, folded to its last lines; click for the rest. */
function Output({ step }: { step: Step }) {
  const [open, setOpen] = useState(step.state === "failed");
  const lines = (step.output ?? "").split("\n");
  // Folded: the last three lines that say something.
  const shown = open ? lines : lines.filter((l) => l.trim()).slice(-3);
  return (
    <div className="ak-term ak-in">
      <pre className="mono">{shown.join("\n")}</pre>
      {lines.length > 3 && (
        <button type="button" onClick={() => setOpen((o) => !o)} className="chip">
          {open ? "Show less" : `Show all ${lines.length} lines`}
        </button>
      )}
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
  onDone: (message: string | null) => void;
}) {
  const [files, setFiles] = useState<number | null>(null);
  useEffect(() => {
    void api
      .agentRewindPreview(s.id, messageIndex(s, entry))
      .then(setFiles)
      .catch(() => setFiles(0));
  }, [s, entry]);
  const go = () =>
    void rewindSession(s.id, entry)
      .then((n) =>
        onDone(`↺ Rewound · ${n} ${n === 1 ? "file" : "files"} put back · write a new message to go another way`),
      )
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
  const said = s.entries[entry]?.kind === "you" ? (s.entries[entry] as { text: string }).text : "";
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
            : `This puts back ${files} ${files === 1 ? "file" : "files"} the agent changed since.`}
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

type Slash = { name: string; description: string; group: string };

/** Where you type to the agent: / lists commands, @ finds files in the project. */
function Composer({
  session: s,
  working,
  text,
  setText,
  onSend,
  onClear,
  onRewind,
  keys,
}: {
  session: Session;
  working: boolean;
  text: string;
  setText: (t: string) => void;
  onSend: () => void;
  onClear: () => void;
  onRewind: () => void;
  keys: boolean;
}) {
  const [commands, setCommands] = useState<Slash[]>([]);
  const [files, setFiles] = useState<string[]>([]);
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    void api
      .agentCommands(s.id)
      .then(setCommands)
      .catch(() => setCommands([]));
  }, [s.id]);
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
  const runCommand = (c: Slash) => {
    setText("");
    if (c.name === "/clear") onClear();
    else if (c.name === "/rewind") onRewind();
    else sendToSession(s.id, c.name);
  };
  const pickFile = (f: string) => {
    if (!mention) return;
    setText(`${text.slice(0, text.length - mention[2].length - 1)}@${f} `);
    inputRef.current?.focus({ preventScroll: true });
  };
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
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
      onSend();
    }
  };
  let lastGroup = "";
  return (
    <>
      {count > 0 && (
        <div className="ak-pop ak-in" role="listbox" aria-label={slash ? "Commands" : "Files"}>
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
        </div>
      )}
      <div className="ak-composer relative">
        <input
          ref={inputRef}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            setSel(0);
          }}
          onKeyDown={onKey}
          placeholder={working ? "Steer it, it reads this next" : "Ask for more, / for commands, @ for files"}
        />
        <span className="ckeys mono">{working ? "Esc interrupt" : "Enter send"}</span>
        <KeyHint show={keys}>Alt T</KeyHint>
      </div>
    </>
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
