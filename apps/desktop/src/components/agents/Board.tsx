"use client";

// Several sessions at once: one tile each, two across (three on wide
// screens), one chat box under them for the tile in focus.

import { useEffect, useRef, useState } from "react";
import {
  answerQuestion,
  finishSession,
  resumeSession,
  type Session,
  sendToSession,
  setLayout,
  startSession,
  tuneSession,
  useAgents,
  wideScreen,
} from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import type { Agents } from "@/lib/types";
import { KeyHint } from "../ask/parts";
import { AGENT_MARKS, NameField } from "./AgentsTab";
import { ChatControls } from "./Controls";

const MARK_BY_NAME: Record<string, string> = {
  "Claude Code": "claude_code",
  Codex: "codex",
  "GitHub Copilot": "copilot",
  Cursor: "cursor",
  Local: "local",
};

function state(s: Session): [string, string] {
  if (s.question) return ["ask", "Needs you"];
  if (s.status === "working" || s.status === "waiting") return ["run", "Working"];
  if (s.status === "ended" || s.status === "failed") return ["off", s.status === "failed" ? "Failed" : "Paused"];
  return ["done", "Done"];
}

function lastSteps(s: Session): string[] {
  const out: string[] = [];
  for (let i = s.entries.length - 1; i >= 0 && out.length < 3; i--) {
    const e = s.entries[i];
    if (e.kind === "step") out.unshift(e.step.state === "running" ? `${e.step.label}...` : e.step.label);
    else if (e.kind === "text") out.unshift(e.text.split("\n")[0].slice(0, 90));
  }
  return out;
}

function Tile({ s, n, focused, keys }: { s: Session; n: number; focused: boolean; keys: boolean }) {
  const [st, label] = state(s);
  const mark = AGENT_MARKS[MARK_BY_NAME[s.agent] ?? ""] ?? ["L", "#8e8e93"];
  const planDone = s.plan.filter((p) => p.status === "completed").length;
  const ctx = s.usage ? Math.round(Math.min(1, s.usage.used / s.usage.window) * 100) : null;
  const [note, setNote] = useState<string | null>(null);
  return (
    // biome-ignore lint/a11y/useSemanticElements: a tile holds buttons, so it cannot be one
    <div
      role="button"
      tabIndex={0}
      className="ak-tile"
      data-s={st}
      data-focus={focused}
      onClick={() => useAgents.setState({ focus: s.id })}
      onKeyDown={(e) =>
        e.key === "Enter" && e.target === e.currentTarget && useAgents.setState({ current: s.id, layout: "one" })
      }
    >
      <div className="ak-th">
        <span className="ak-mark" style={{ background: mark[1] }}>
          {mark[0]}
        </span>
        <NameField id={s.id} title={s.title} />
        <span className="ak-pill" data-s={st}>
          {label}
        </span>
        {n <= 9 && <KeyHint show={keys}>{`Ctrl ${n}`}</KeyHint>}
      </div>
      <p className="ak-tsub mono truncate">
        {s.project}
        {s.worktree ? ` · ${s.worktree}` : s.branch ? ` · ${s.branch}` : ""}
      </p>
      <ul className="ak-tsteps">
        {lastSteps(s).map((t, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: short fixed list
          <li key={i} className="truncate">
            {t}
          </li>
        ))}
      </ul>
      {s.question ? (
        <div className="ak-tq">
          <p className="truncate">{s.question.label}</p>
          <div className="flex gap-1.5">
            <button type="button" className="ak-chip primary chip" onClick={() => answerQuestion(s.id, "allow")}>
              Allow
            </button>
            <button type="button" className="ak-chip chip" onClick={() => answerQuestion(s.id, "always")}>
              Allow this session
            </button>
            <button type="button" className="ak-chip chip" onClick={() => answerQuestion(s.id, "deny")}>
              No
            </button>
          </div>
        </div>
      ) : (
        <div className="ak-tfoot">
          {s.plan.length > 0 && (
            <span>
              {planDone}/{s.plan.length} steps
            </span>
          )}
          {ctx !== null && <span>{ctx}% context</span>}
          <span className="flex-1" />
          {st === "off" && (
            <button
              type="button"
              className="ak-chip chip"
              onClick={() => void resumeSession(s.id).catch((e) => setNote(String(e)))}
            >
              Resume
            </button>
          )}
          {s.worktree && st === "done" && !s.finished && (
            <button
              type="button"
              className="ak-chip primary chip"
              onClick={() =>
                void finishSession(s.id)
                  .then(setNote)
                  .catch((e: unknown) => setNote(String(e)))
              }
            >
              Finish
            </button>
          )}
        </div>
      )}
      {s.worktree && !s.finished && (
        <p className="ak-tnote">Works in its own copy on {s.worktree}. Finish merges it back.</p>
      )}
      {note && <p className="ak-tnote">{note}</p>}
    </div>
  );
}

function EmptySlot({ recent }: { recent: Session[] }) {
  const { data: agents } = useCached<Agents>("agents", api.agentsStatus);
  const { data: projects } = useCached<{ name: string; path: string }[]>("projects", api.projectsList);
  const [agent, setAgent] = useState<string | null>(null);
  const [prompt, setPrompt] = useState("");
  const path = projects?.[0]?.path ?? "";
  const kinds: [string, string, boolean][] = [
    ["claude_code", "Claude Code", !!agents?.claudeCode],
    ["codex", "Codex", !!agents?.codex],
    ["local", "Local", true],
  ];
  return (
    <div className="ak-tile ak-empty">
      {agent ? (
        <input
          // biome-ignore lint/a11y/noAutofocus: picked a moment ago
          autoFocus
          value={prompt}
          placeholder="What should it do?"
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") setAgent(null);
            if (e.key === "Enter" && prompt.trim() && path)
              void startSession(agent, path, prompt.trim(), "edit").then(() => {
                useAgents.setState({ layout: "board" });
                setAgent(null);
                setPrompt("");
              });
          }}
        />
      ) : (
        <>
          <p className="ak-tsub">New</p>
          <div className="flex flex-wrap gap-1.5">
            {kinds
              .filter((k) => k[2])
              .map(([id, name]) => (
                <button key={id} type="button" className="ak-chip chip" onClick={() => setAgent(id)}>
                  {name}
                </button>
              ))}
          </div>
          {recent.length > 0 && (
            <>
              <p className="ak-tsub">Bring back</p>
              <div className="flex flex-wrap gap-1.5">
                {recent.slice(0, 3).map((s) => (
                  <button
                    key={s.id}
                    type="button"
                    className="ak-chip chip max-w-48 truncate"
                    onClick={() => void resumeSession(s.id)}
                  >
                    {s.title}
                  </button>
                ))}
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}

export function Board({ sessions, keys }: { sessions: Session[]; keys: boolean }) {
  const focus = useAgents((s) => s.focus);
  const live = sessions.filter((s) => !s.restored || s.status !== "ended");
  const paused = sessions.filter((s) => s.restored && s.status === "ended");
  const target = live.find((s) => s.id === focus) ?? live.find((s) => s.question) ?? live[0] ?? null;
  const [text, setText] = useState("");
  const grid = useRef<HTMLDivElement>(null);
  const [below, setBelow] = useState(0);

  // Ctrl 1-9 puts a tile in focus.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.altKey || e.metaKey) return;
      const n = Number(e.key);
      if (!n || n > live.length) return;
      e.preventDefault();
      useAgents.setState({ focus: live[n - 1].id });
      grid.current?.children[n - 1]?.scrollIntoView({ block: "nearest" });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [live]);

  const measure = () => {
    const el = grid.current;
    if (!el) return;
    const hidden = Array.from(el.children).filter(
      (c) => (c as HTMLElement).offsetTop >= el.scrollTop + el.clientHeight - 8,
    ).length;
    setBelow(hidden);
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: count again when tiles change
  useEffect(measure, [live.length]);

  const send = () => {
    const t = text.trim();
    if (!t || !target) return;
    sendToSession(target.id, t);
    setText("");
  };

  if (live.length === 0 && paused.length === 0) {
    return (
      <div className="ak-board">
        <div className="ak-grid">
          <EmptySlot recent={[]} />
        </div>
        <button type="button" className="chip self-start" onClick={() => setLayout("one")}>
          Back to one session
        </button>
      </div>
    );
  }

  return (
    <div className="ak-board">
      <div className="relative">
        <div ref={grid} className="ak-grid" data-wide={wideScreen()} data-more={below > 0} onScroll={measure}>
          {live.map((s, i) => (
            <Tile key={s.id} s={s} n={i + 1} focused={s.id === target?.id} keys={keys} />
          ))}
          <EmptySlot recent={paused} />
        </div>
        {below > 0 && <p className="ak-below">{below} more below</p>}
      </div>
      {target && (
        <div className="ak-composer ak-two">
          <div className="ak-row">
            <input
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  send();
                }
              }}
              placeholder={`Tell ${target.title.slice(0, 32)}...`}
            />
          </div>
          <ChatControls
            agent={target.agent}
            model={target.model}
            effort={target.effort}
            onModel={(m) => tuneSession(target.id, m, target.effort ?? null)}
            onEffort={(e) => tuneSession(target.id, target.model ?? null, e)}
            usage={target.usage}
            limit={target.limit}
            keys="Enter send"
          />
        </div>
      )}
    </div>
  );
}
