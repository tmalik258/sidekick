"use client";

// The Agents tab: start Claude Code or Codex in a project, watch it work
// step by step, answer its questions here, steer it, and keep or undo each
// change when it is done.

import { AnimatePresence, motion } from "motion/react";
import { type KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
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
  return (
    <div className="mt-2 flex flex-col gap-2">
      {sessions.length > 0 && (
        <div className="flex items-center gap-1.5 overflow-x-auto pb-0.5">
          {sessions.map((s) => (
            <button
              key={s.id}
              type="button"
              aria-pressed={s.id === current}
              onClick={() => useAgents.setState({ current: s.id })}
              className={`chip flex max-w-44 shrink-0 items-center gap-1.5 rounded-full px-2.5 py-1 text-[12px] ${
                s.id === current ? "bg-white/[0.16] text-white" : "bg-white/[0.06] text-white/65 hover:bg-white/[0.1]"
              }`}
            >
              <StatusDot status={s.status} />
              <span className="truncate">{s.project || s.title}</span>
            </button>
          ))}
          <button
            type="button"
            aria-pressed={current === null}
            onClick={() => useAgents.setState({ current: null })}
            className="chip flex shrink-0 items-center gap-1 rounded-full bg-white/[0.06] px-2.5 py-1 text-[12px] text-white/65 hover:bg-white/[0.1]"
          >
            <Icon name="plus" size={12} /> New
          </button>
        </div>
      )}
      {session ? <SessionView key={session.id} session={session} keys={keys} maxHeight={maxHeight} /> : <NewSession />}
    </div>
  );
}

function StatusDot({ status }: { status: Session["status"] }) {
  const color =
    status === "working"
      ? "bg-[#0a84ff] animate-pulse"
      : status === "waiting"
        ? "bg-[#ff9f0a]"
        : status === "failed"
          ? "bg-[#ff453a]"
          : "bg-[#30d158]";
  return <span className={`size-1.5 shrink-0 rounded-full ${color}`} role="img" aria-label={status} />;
}

function NewSession() {
  const { data: agents } = useCached<Agents>("agents", api.agentsStatus);
  const { data: projects } = useCached<{ name: string; path: string }[]>("projects", api.projectsList);
  const [agent, setAgent] = useState<string>("");
  const [path, setPath] = useState("");
  const [mode, setMode] = useState<AgentMode>("edit");
  const [prompt, setPrompt] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const choices = [
    ...(agents?.claudeCode ? [{ id: "claude_code", name: "Claude Code" }] : []),
    ...(agents?.codex ? [{ id: "codex", name: "Codex" }] : []),
  ];
  const pickedAgent = agent || choices[0]?.id || "";
  const pickedPath = path || projects?.[0]?.path || "";
  useEffect(() => {
    requestAnimationFrame(() => inputRef.current?.focus());
  }, []);
  if (agents && choices.length === 0) {
    return (
      <p className="py-2 text-[13px] text-[rgb(235_235_245/0.6)]">
        Install Claude Code or Codex to run agents here. Settings &gt; AI shows how.
      </p>
    );
  }
  const go = () => {
    if (!prompt.trim() || !pickedPath || busy) return;
    setBusy(true);
    setError(null);
    startSession(pickedAgent, pickedPath, prompt.trim(), mode)
      .then(() => setPrompt(""))
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setBusy(false));
  };
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      go();
    }
  };
  return (
    <div className="flex flex-col gap-2 rounded-2xl bg-white/[0.05] p-2.5">
      <textarea
        ref={inputRef}
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        onKeyDown={onKey}
        rows={2}
        placeholder="What should it do?"
        className="w-full resize-none bg-transparent text-[14px] text-white outline-none placeholder:text-[rgb(235_235_245/0.4)]"
      />
      <div className="flex flex-wrap items-center gap-1.5 text-[12px]">
        <select
          aria-label="Project"
          value={pickedPath}
          onChange={(e) => setPath(e.target.value)}
          className="chip max-w-40 truncate rounded-full bg-white/[0.1] px-2.5 py-1 text-white/85 outline-none"
        >
          {(projects ?? []).map((p) => (
            <option key={p.path} value={p.path}>
              {p.name}
            </option>
          ))}
        </select>
        {choices.length > 1 && (
          <select
            aria-label="Agent"
            value={pickedAgent}
            onChange={(e) => setAgent(e.target.value)}
            className="chip rounded-full bg-white/[0.1] px-2.5 py-1 text-white/85 outline-none"
          >
            {choices.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        )}
        <ModeSwitch mode={mode} onChange={setMode} />
        <button
          type="button"
          disabled={!prompt.trim() || !pickedPath || busy}
          onClick={go}
          className="chip ml-auto rounded-full bg-white px-3 py-1 text-[12.5px] font-medium text-black disabled:opacity-40"
        >
          {busy ? "Starting..." : "Start"}
        </button>
      </div>
      {error && <p className="text-[12px] text-[#ffb4ae]">{error}</p>}
    </div>
  );
}

function ModeSwitch({ mode, onChange }: { mode: AgentMode; onChange: (m: AgentMode) => void }) {
  return (
    <fieldset aria-label="Mode" className="flex rounded-full bg-white/[0.08] p-0.5">
      {MODES.map((m) => (
        <button
          key={m.id}
          type="button"
          title={m.note}
          aria-pressed={mode === m.id}
          onClick={() => onChange(m.id)}
          className={`chip rounded-full px-2 py-0.5 text-[11.5px] font-medium ${
            mode === m.id ? "bg-white text-black" : "text-white/65 hover:text-white"
          }`}
        >
          {m.label}
        </button>
      ))}
    </fieldset>
  );
}

/** A ring showing how much of the context window is used. */
function ContextRing({ used, window }: { used: number; window: number }) {
  const p = Math.min(1, used / window);
  const r = 7;
  const c = 2 * Math.PI * r;
  return (
    <span
      className="flex items-center gap-1 text-[11px] text-[rgb(235_235_245/0.45)]"
      title={`${Math.round(p * 100)}% of the context used`}
    >
      <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
        <circle cx="9" cy="9" r={r} fill="none" stroke="rgb(255 255 255 / 0.12)" strokeWidth="2" />
        <circle
          cx="9"
          cy="9"
          r={r}
          fill="none"
          stroke={p > 0.85 ? "#ff9f0a" : "#f5f5f7"}
          strokeWidth="2"
          strokeDasharray={`${c * p} ${c}`}
          transform="rotate(-90 9 9)"
          strokeLinecap="round"
        />
      </svg>
      {Math.round(p * 100)}%
    </span>
  );
}

function SessionView({ session: s, keys, maxHeight }: { session: Session; keys: boolean; maxHeight: number }) {
  const [text, setText] = useState("");
  const [reviewing, setReviewing] = useState(false);
  const scroll = useRef<HTMLDivElement>(null);
  const now = useNow(1000);
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
  }, [s.entries.length]);

  // Alt T: carry on in a terminal.
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.altKey && !e.ctrlKey && e.key.toLowerCase() === "t") {
        e.preventDefault();
        void api.agentTerminal(s.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [s.id]);

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

  if (reviewing) {
    return <Review session={s} keys={keys} maxHeight={maxHeight} onDone={() => setReviewing(false)} />;
  }
  const send = () => {
    const t = text.trim();
    if (!t) return;
    sendToSession(s.id, t);
    setText("");
  };
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2 text-[12px] text-[rgb(235_235_245/0.55)]">
        <span className="truncate font-medium text-white/85">{s.project}</span>
        {s.branch && <span className="truncate">· {s.branch}</span>}
        <span>· {s.agent}</span>
        <span>· {MODES.find((m) => m.id === s.mode)?.label}</span>
        <span className="ml-auto flex shrink-0 items-center gap-2">
          {s.usage && <ContextRing used={s.usage.used} window={s.usage.window} />}
          {working && <span className="tabular-nums">{Math.round((now - s.startedAt) / 1000)} s</span>}
        </span>
      </div>

      {s.plan.length > 0 && (
        <ol className="flex flex-col gap-0.5 rounded-xl bg-white/[0.04] px-2.5 py-1.5 text-[12px]">
          {s.plan.map((p) => (
            <li
              key={p.text}
              className={`flex items-center gap-1.5 ${
                p.status === "completed"
                  ? "text-[rgb(235_235_245/0.45)] line-through"
                  : p.status === "in_progress"
                    ? "text-white"
                    : "text-white/65"
              }`}
            >
              <span aria-hidden="true">{p.status === "completed" ? "✓" : p.status === "in_progress" ? "›" : "·"}</span>
              {p.text}
            </li>
          ))}
        </ol>
      )}

      <div ref={scroll} className="ask-scroll flex flex-col gap-1.5 overflow-y-auto pr-1" style={{ maxHeight }}>
        {s.entries.map((e, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: entries only ever append
          <EntryRow key={i} entry={e} />
        ))}
      </div>

      {working && (
        <p className="shimmer-text text-[12.5px] text-[rgb(235_235_245/0.6)]" role="status" aria-live="polite">
          {s.question ? "Waiting for you" : current ? `${current}...` : "Working..."}
        </p>
      )}

      <AnimatePresence>
        {s.question && (
          <motion.div
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            className="flex flex-col gap-2 rounded-2xl bg-[#ff9f0a]/[0.12] p-2.5 ring-1 ring-[#ff9f0a]/30 ring-inset"
            role="alertdialog"
            aria-label={`${s.agent} asks`}
          >
            <p className="text-[13px] text-white">
              {s.agent} wants to: <span className="font-medium">{s.question.label}</span>
            </p>
            {s.question.detail && (
              <code className="block truncate rounded-lg bg-black/30 px-2 py-1 font-mono text-[11.5px] text-white/80">
                {s.question.detail}
              </code>
            )}
            <div className="flex gap-1.5">
              <AnswerButton solid label="Allow" hint="Alt A" keys onClick={() => answerQuestion(s.id, "allow")} />
              <AnswerButton label="Always" hint="Alt Y" keys onClick={() => answerQuestion(s.id, "always")} />
              <AnswerButton label="No" hint="Alt N" keys onClick={() => answerQuestion(s.id, "deny")} />
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {s.error && <p className="rounded-xl bg-[#ff453a]/15 px-3 py-2 text-[12.5px] text-[#ffb4ae]">{s.error}</p>}

      {done && s.reviewable && (
        <button
          type="button"
          onClick={() => setReviewing(true)}
          className="chip self-start rounded-full bg-white px-3.5 py-1.5 text-[13px] font-medium text-black"
        >
          Review changes
        </button>
      )}

      <div className="flex items-center gap-1.5">
        {s.status !== "ended" && (
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                send();
              }
            }}
            placeholder={working ? "Steer it" : "Ask for more"}
            className="min-w-0 flex-1 rounded-full bg-white/[0.08] px-3 py-1.5 text-[13px] text-white outline-none placeholder:text-[rgb(235_235_245/0.4)]"
          />
        )}
        {working && (
          <button
            type="button"
            onClick={() => void api.agentStop(s.id)}
            className="chip rounded-full bg-white/[0.12] px-3 py-1.5 text-[12.5px] text-white/90"
          >
            Stop
          </button>
        )}
        <span className="relative">
          <button
            type="button"
            title="Carry on in a terminal"
            onClick={() => void api.agentTerminal(s.id)}
            className="chip rounded-full bg-white/[0.08] px-3 py-1.5 text-[12.5px] text-white/80"
          >
            Open in terminal
          </button>
          <KeyHint show={keys}>Alt T</KeyHint>
        </span>
        {!working && (
          <button
            type="button"
            aria-label="Close session"
            title="Close session"
            onClick={() => closeSession(s.id)}
            className="chip grid size-7 place-items-center rounded-full bg-white/[0.08] text-white/70"
          >
            <Icon name="close" size={12} />
          </button>
        )}
      </div>
    </div>
  );
}

function AnswerButton({
  label,
  hint,
  solid,
  keys,
  onClick,
}: {
  label: string;
  hint: string;
  solid?: boolean;
  keys: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`chip flex items-center gap-1.5 rounded-full px-3 py-1 text-[12.5px] font-medium ${
        solid ? "bg-white text-black" : "bg-white/[0.12] text-white"
      }`}
    >
      {label}
      {keys && <kbd className={`font-sans text-[11px] ${solid ? "text-black/45" : "text-white/40"}`}>{hint}</kbd>}
    </button>
  );
}

const STEP_ICON: Record<string, string> = {
  Read: "↗",
  Edit: "✎",
  MultiEdit: "✎",
  Write: "✎",
  Bash: "›_",
  Grep: "⌕",
  Glob: "⌕",
  WebSearch: "⌕",
  WebFetch: "↗",
};

function EntryRow({ entry: e }: { entry: Entry }) {
  if (e.kind === "you") {
    return (
      <div className="self-end rounded-[16px] rounded-br-md bg-white/[0.14] px-3 py-1.5 text-[13px] whitespace-pre-wrap">
        {e.text}
      </div>
    );
  }
  if (e.kind === "text") {
    return (
      <div className="text-[13px] leading-relaxed text-white/90">
        <Markdown text={e.text} />
      </div>
    );
  }
  const st = e.step;
  return (
    <div className="flex min-w-0 items-center gap-2 text-[12px]">
      <span
        className={`grid size-5 shrink-0 place-items-center rounded-md font-mono text-[10px] ${
          st.state === "failed"
            ? "bg-[#ff453a]/20 text-[#ffb4ae]"
            : st.state === "running"
              ? "bg-[#0a84ff]/25 text-white"
              : "bg-white/[0.08] text-white/60"
        }`}
        aria-hidden="true"
      >
        {STEP_ICON[st.tool] ?? "•"}
      </span>
      <span className={`truncate ${st.state === "running" ? "text-white" : "text-white/65"}`}>{st.label}</span>
      {st.detail && (
        <code className="min-w-0 truncate font-mono text-[11px] text-[rgb(235_235_245/0.4)]">{st.detail}</code>
      )}
    </div>
  );
}
