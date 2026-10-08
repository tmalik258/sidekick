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
  usageLine,
  useAgents,
  wideScreen,
} from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import type { Agents } from "@/lib/types";
import { AGENT_MARKS, NameField, STEP_TAG } from "./AgentsTab";
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

/** The last few steps, tagged like the transcript; the running one shimmers. */
function lastSteps(s: Session): { tag: string; text: string; live: boolean }[] {
  const out: { tag: string; text: string; live: boolean }[] = [];
  for (let i = s.entries.length - 1; i >= 0 && out.length < 3; i--) {
    const e = s.entries[i];
    if (e.kind === "step") {
      const live = e.step.state === "running";
      out.unshift({
        tag: live ? "···" : (STEP_TAG[e.step.tool] ?? e.step.tool),
        text: e.step.detail ? `${e.step.label} ${e.step.detail}` : e.step.label,
        live,
      });
    }
  }
  return out;
}

/** The newest thing the agent said, for a finished tile. */
function lastSay(s: Session): string | null {
  for (let i = s.entries.length - 1; i >= 0; i--) {
    const e = s.entries[i];
    if (e.kind === "text") return e.text.trim();
    if (e.kind === "you") return null;
  }
  return null;
}

export const markFor = (agent: string): [string, string] => AGENT_MARKS[MARK_BY_NAME[agent] ?? ""] ?? ["L", "#8e8e93"];

function Tile({ s, n, focused }: { s: Session; n: number; focused: boolean }) {
  const [st, label] = state(s);
  const mark = markFor(s.agent);
  const planDone = s.plan.filter((p) => p.status === "completed").length;
  const ctx = s.usage ? Math.round(Math.min(1, s.usage.used / s.usage.window) * 100) : null;
  const [note, setNote] = useState<string | null>(null);
  const say = st === "done" || st === "off" ? lastSay(s) : null;
  const progress = s.plan.length ? Math.round((planDone / s.plan.length) * 100) : (ctx ?? 0);
  const foot =
    st === "ask"
      ? "Waiting for you"
      : st === "done" || st === "off"
        ? s.localModel
          ? `${label} · ${s.localModel}`
          : label
        : [s.plan.length ? `${planDone} of ${s.plan.length}` : null, ctx !== null ? `${ctx}% context` : null]
            .filter(Boolean)
            .join(" · ") || "Starting";
  const open = () => useAgents.setState({ current: s.id, layout: "one" });
  return (
    // biome-ignore lint/a11y/useSemanticElements: a tile holds buttons, so it cannot be one
    <div
      role="button"
      tabIndex={0}
      className="ak-tile"
      data-s={st}
      data-focus={focused}
      onClick={() => useAgents.setState({ focus: s.id })}
      onDoubleClick={(e) => !(e.target as HTMLElement).closest("button, input") && open()}
      onKeyDown={(e) => e.key === "Enter" && e.target === e.currentTarget && open()}
    >
      <div className="ak-th">
        <span className="ak-mark" style={{ background: mark[1] }} aria-hidden="true">
          {mark[0]}
        </span>
        <span className="min-w-0">
          <NameField id={s.id} title={s.title} />
          <span className="ak-tm">
            {s.project}
            {s.agent === "Local"
              ? " · on this PC"
              : s.worktree
                ? ` · ⎇ ${s.worktree}`
                : s.branch
                  ? ` · ⎇ ${s.branch}`
                  : ""}
          </span>
        </span>
        <span className="ak-pill" data-s={st}>
          <i className="ak-pdot" aria-hidden="true" />
          {label}
        </span>
      </div>
      {s.worktree && !s.finished && (
        <p className="ak-twt">
          <span>
            <b>Its own worktree.</b> It never edits the same files as the other session on {s.project}; Finish merges it
            back.
          </span>
        </p>
      )}
      {s.question ? (
        <>
          <p className="ak-tsay">
            {s.question.label}
            {s.question.detail && (
              <>
                {" "}
                <code>{s.question.detail}</code>
              </>
            )}
          </p>
          <div className="ak-tacts">
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
        </>
      ) : say ? (
        <>
          <p className="ak-tsay ak-tclamp">{say}</p>
          <div className="ak-tacts">
            <button type="button" className="ak-chip chip" onClick={open}>
              Open
            </button>
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
        </>
      ) : (
        <ul className="ak-tlog">
          {lastSteps(s).map((t, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: short fixed list
            <li key={i}>
              <span className="k">{t.tag}</span>
              <span className={t.live ? "live" : undefined}>{t.text}</span>
            </li>
          ))}
        </ul>
      )}
      {note && <p className="ak-tnote">{note}</p>}
      <div className="ak-tf">
        {st === "run" || st === "ask" ? (
          <span className="ak-tbar">
            <i className="ak-tfill" style={{ width: `${Math.max(4, progress)}%` }} />
          </span>
        ) : null}
        <span className={st === "run" || st === "ask" ? undefined : "flex-1"}>{foot}</span>
        {n <= 9 && <span className="ak-tkey">Ctrl {n}</span>}
      </div>
    </div>
  );
}

function ago(t: number): string {
  const mins = Math.round((Date.now() - t) / 60_000);
  if (mins < 60) return `${Math.max(1, mins)} min ago`;
  const h = Math.round(mins / 60);
  return h < 24 ? `${h} h ago` : `${Math.round(h / 24)} d ago`;
}

function EmptySlot({ recent, n }: { recent: Session[]; n: number }) {
  const { data: agents } = useCached<Agents>("agents", api.agentsStatus);
  const { data: projects } = useCached<{ name: string; path: string }[]>("projects", api.projectsList);
  const [picking, setPicking] = useState(false);
  const [agent, setAgent] = useState<string | null>(null);
  const [prompt, setPrompt] = useState("");
  const path = projects?.[0]?.path ?? "";
  const kinds: [string, string, boolean][] = [
    ["claude_code", "Claude Code", !!agents?.claudeCode],
    ["codex", "Codex", !!agents?.codex],
    ["local", "Local", true],
  ];
  if (!picking && !agent) {
    return (
      <button type="button" className="ak-tile ak-empty" onClick={() => setPicking(true)}>
        <span className="ak-plus" aria-hidden="true">
          +
        </span>
        Start or bring back a session
        {n <= 9 && <span className="ak-tkey">Ctrl {n}</span>}
      </button>
    );
  }
  return (
    <div className="ak-tile ak-pickslot">
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
                setPicking(false);
                setPrompt("");
              });
          }}
        />
      ) : (
        <>
          <p className="ak-tlab">New</p>
          <div className="ak-tpick">
            {kinds
              .filter((k) => k[2])
              .map(([id, name]) => (
                <button key={id} type="button" onClick={() => setAgent(id)}>
                  <span className="ak-mark sm" style={{ background: AGENT_MARKS[id]?.[1] }} aria-hidden="true">
                    {AGENT_MARKS[id]?.[0]}
                  </span>
                  {name}
                </button>
              ))}
          </div>
          {recent.length > 0 && (
            <>
              <p className="ak-tlab">Bring back</p>
              <div className="ak-trec">
                {recent.slice(0, 3).map((r) => {
                  const m = markFor(r.agent);
                  return (
                    <button key={r.id} type="button" onClick={() => void resumeSession(r.id)}>
                      <span className="ak-mark sm" style={{ background: m[1] }} aria-hidden="true">
                        {m[0]}
                      </span>
                      <span className="truncate">{r.title}</span>
                      <small>
                        {r.project} · {ago(r.startedAt)}
                      </small>
                    </button>
                  );
                })}
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}

export function Board({ sessions }: { sessions: Session[]; keys?: boolean }) {
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
          <EmptySlot recent={[]} n={1} />
        </div>
        <button type="button" className="chip self-start" onClick={() => setLayout("one")}>
          Back to one session
        </button>
      </div>
    );
  }

  const working = live.filter((s) => state(s)[0] === "run").length;
  const needs = live.filter((s) => s.question).length;
  const limits = Array.from(new Set(live.map((s) => s.agent)))
    .map((a) => [a, usageLine(a)] as const)
    .filter((x): x is readonly [string, string] => !!x[1] && x[1] !== "ready");
  return (
    <div className="ak-board">
      <div className="ak-btop">
        <span className="ak-bsum">
          <b>
            {live.length} session{live.length === 1 ? "" : "s"}
          </b>
          {working > 0 && (
            <span>
              <i className="ak-bdot" style={{ background: "#64d2ff" }} />
              {working} working
            </span>
          )}
          {needs > 0 && (
            <span>
              <i className="ak-bdot" style={{ background: "#ff9f0a" }} />
              {needs} needs you
            </span>
          )}
        </span>
        {limits.map(([a, line]) => {
          const m = markFor(a);
          return (
            <span key={a} className="ak-blim">
              <span className="ak-mark sm" style={{ background: m[1] }} aria-hidden="true">
                {m[0]}
              </span>
              {line.split(",")[0]}
            </span>
          );
        })}
      </div>
      <div className="relative">
        <div ref={grid} className="ak-grid" data-wide={wideScreen()} data-more={below > 0} onScroll={measure}>
          {live.map((s, i) => (
            <Tile key={s.id} s={s} n={i + 1} focused={s.id === target?.id} />
          ))}
          <EmptySlot recent={paused} n={live.length + 1} />
        </div>
        {below > 0 && <p className="ak-below">{below} more below</p>}
      </div>
      {target && (
        <div className="ak-composer ak-two">
          <div className="ak-to">
            To
            <span className="ak-mark sm" style={{ background: markFor(target.agent)[1] }} aria-hidden="true">
              {markFor(target.agent)[0]}
            </span>
            <b className="truncate">{target.title}</b>
            {live.length > 1 && <span>· Ctrl 1 to {Math.min(9, live.length)} to switch</span>}
          </div>
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
              placeholder="Message this session"
            />
          </div>
          <ChatControls
            agent={target.agent}
            model={target.model}
            effort={target.effort}
            onModel={(m) => tuneSession(target.id, m, target.effort ?? null)}
            onEffort={(e) => tuneSession(target.id, target.model ?? null, e)}
            usage={target.usage}
            localModel={target.localModel}
            limit={target.limit}
            keys="Enter send"
          />
        </div>
      )}
    </div>
  );
}
