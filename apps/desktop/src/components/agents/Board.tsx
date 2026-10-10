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
import { useSidekick } from "@/lib/store";
import type { Agents } from "@/lib/types";
import { Select } from "../settings/ui";
import { AgentMark, agentMarkId, NameField, projectPlace, STEP_TAG } from "./AgentsTab";
import { ChatControls } from "./Controls";

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

function Tile({ s, n, focused }: { s: Session; n: number; focused: boolean }) {
  const [st, label] = state(s);
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
      data-agent={agentMarkId(s.agent)}
      data-focus={focused}
      onClick={() => useAgents.setState({ focus: s.id })}
      onDoubleClick={(e) => !(e.target as HTMLElement).closest("button, input") && open()}
      onKeyDown={(e) => e.key === "Enter" && e.target === e.currentTarget && open()}
    >
      <div className="ak-th">
        <AgentMark agent={s.agent} />
        <span className="min-w-0">
          <NameField id={s.id} title={s.title} />
          <span className="ak-tm">
            {s.agent}
            {s.project ? ` · ${s.project}` : ""}
            {s.worktree ? ` · ⎇ ${s.worktree}` : s.branch ? ` · ⎇ ${s.branch}` : ""}
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
          <p className="ak-tsay ak-tclamp">{plain(say)}</p>
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

const PICK_NOTE: Record<string, string> = {
  claude_code: "Cloud",
  codex: "Cloud",
  local: "This PC",
};

function EmptySlot({
  recent,
  openSignal = 0,
  onDismiss,
}: {
  recent: Session[];
  /** Parent bumps this (Ctrl N) to open the new-session picker. */
  openSignal?: number;
  /** After the picker closes — put focus back on the board composer. */
  onDismiss?: () => void;
}) {
  const { data: agents } = useCached<Agents>("agents", api.agentsStatus);
  const { data: projects } = useCached<{ name: string; path: string }[]>("projects", api.projectsList);
  const draftPath = useAgents((s) => s.draftPath);
  const [picking, setPicking] = useState(false);
  const [agent, setAgent] = useState<string | null>(null);
  const [prompt, setPrompt] = useState("");
  const [path, setPath] = useState(draftPath ?? "");
  const [highlight, setHighlight] = useState(0);
  const pickRoot = useRef<HTMLDivElement>(null);
  const list = projects ?? [];
  const pickedPath = (path && list.some((p) => p.path === path) ? path : null) || list[0]?.path || "";
  const project = list.find((p) => p.path === pickedPath);
  const choosePath = (next: string) => {
    setPath(next);
    useAgents.setState({ draftPath: next });
  };
  const closePick = () => {
    setAgent(null);
    setPicking(false);
    setPrompt("");
    setHighlight(0);
    onDismiss?.();
  };
  // Keep the picker on a real project when the list loads or draft goes stale.
  useEffect(() => {
    if (!list.length) return;
    if (pickedPath && pickedPath !== path) setPath(pickedPath);
  }, [list, pickedPath, path]);
  // Ctrl N (from Board) opens this picker.
  useEffect(() => {
    if (openSignal === 0) return;
    setAgent(null);
    setPrompt("");
    setHighlight(0);
    setPicking(true);
    // Drop composer focus so 1–3 / arrows land on the picker, not the chat box.
    requestAnimationFrame(() => {
      (document.activeElement as HTMLElement | null)?.blur?.();
      pickRoot.current?.focus({ preventScroll: true });
    });
  }, [openSignal]);
  // Esc closes the picker (or steps back), not the island — AskPanel also
  // listens for Esc on window; capture + preventDefault wins that race.
  useEffect(() => {
    if (!picking) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.repeat) return;
      // Project menu is open: let Select take Esc first.
      if (useSidekick.getState().overlayHit) return;
      e.preventDefault();
      e.stopPropagation();
      if (agent) {
        setAgent(null);
        setPrompt("");
      } else closePick();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [picking, agent]);
  const kinds: [string, string, boolean][] = [
    ["claude_code", "Claude Code", !!agents?.claudeCode],
    ["codex", "Codex", !!agents?.codex],
    ["local", "Local", true],
  ];
  const ready = kinds.filter((k) => k[2]);
  const picked = ready.find(([id]) => id === agent);
  // While choosing an agent: 1–9 / arrows pick, [ ] cycle project.
  useEffect(() => {
    if (!picking || agent) return;
    const moveProject = (dir: 1 | -1) => {
      if (list.length === 0) return;
      const i = Math.max(
        0,
        list.findIndex((p) => p.path === pickedPath),
      );
      const next = list[(i + dir + list.length) % list.length];
      setPath(next.path);
      useAgents.setState({ draftPath: next.path });
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;
      if (e.key === "[" || e.key === "ArrowUp") {
        e.preventDefault();
        moveProject(-1);
        return;
      }
      if (e.key === "]" || e.key === "ArrowDown") {
        e.preventDefault();
        moveProject(1);
        return;
      }
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        setHighlight((h) => (h + ready.length - 1) % Math.max(ready.length, 1));
        return;
      }
      if (e.key === "ArrowRight") {
        e.preventDefault();
        setHighlight((h) => (h + 1) % Math.max(ready.length, 1));
        return;
      }
      if (e.key === "Enter") {
        const id = ready[highlight]?.[0];
        if (!id || !pickedPath) return;
        e.preventDefault();
        setAgent(id);
        return;
      }
      const num = Number(e.key);
      if (num >= 1 && num <= ready.length) {
        e.preventDefault();
        if (pickedPath) setAgent(ready[num - 1][0]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [picking, agent, ready, pickedPath, highlight, list]);
  const projectPicker =
    list.length > 0 ? (
      <span className="ak-pickproj">
        <span className="ak-pickproj-l">In</span>
        <Select
          variant="plain"
          overlay
          searchable
          menuWidth={300}
          label="Project"
          value={pickedPath}
          onChange={choosePath}
          group="Project"
          options={list.map((p) => ({
            value: p.path,
            label: p.name,
            sub: projectPlace(p.path),
            title: p.path,
          }))}
        />
      </span>
    ) : (
      <span className="ak-tm">Add a project in Settings</span>
    );

  if (!picking && !agent) {
    return (
      <button type="button" className="ak-tile ak-empty" onClick={() => setPicking(true)}>
        <span className="ak-plus" aria-hidden="true">
          +
        </span>
        <span className="ak-empty-t">Start or bring back</span>
        {recent.length > 0 ? (
          <span className="ak-empty-s">{recent.length} paused · {ready.length} ready</span>
        ) : (
          <span className="ak-empty-s">{ready.map(([, name]) => name).join(" · ")}</span>
        )}
        <span className="ak-tkey">Ctrl N</span>
      </button>
    );
  }

  return (
    <div ref={pickRoot} className="ak-tile ak-pickslot" tabIndex={-1}>
      {agent && picked ? (
        <>
          <div className="ak-pickhead">
            <AgentMark agent={agent} />
            <span className="min-w-0">
              <b>{picked[1]}</b>
              {projectPicker}
            </span>
            <button type="button" className="ak-pickback" onClick={() => setAgent(null)}>
              Back
            </button>
          </div>
          <input
            // biome-ignore lint/a11y/noAutofocus: picked a moment ago
            autoFocus
            className="ak-pickq"
            value={prompt}
            placeholder="What should it do?"
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && prompt.trim() && pickedPath)
                void startSession(agent, pickedPath, prompt.trim(), "edit").then(() => {
                  useAgents.setState({ layout: "board", draftPath: pickedPath });
                  closePick();
                });
            }}
          />
          <p className="ak-pickhint">
            {pickedPath
              ? `Enter to start in ${project?.name ?? "project"} · Esc back`
              : "Add a project in Settings first"}
          </p>
        </>
      ) : (
        <>
          <div className="ak-pickhead">
            <span className="min-w-0">
              <b>New session</b>
              {projectPicker}
            </span>
            <button type="button" className="ak-pickback" onClick={closePick}>
              Close
            </button>
          </div>
          <div className="ak-tpick" data-n={ready.length}>
            {ready.map(([id, name], i) => (
              <button
                key={id}
                type="button"
                data-agent={id}
                data-focus={i === highlight}
                onMouseEnter={() => setHighlight(i)}
                onClick={() => setAgent(id)}
                disabled={!pickedPath}
              >
                <span className="ak-tpick-mark">
                  <AgentMark agent={id} />
                </span>
                <span className="ak-tpick-copy">
                  <strong>{name === "Claude Code" ? "Claude" : name}</strong>
                  <em>{PICK_NOTE[id] ?? ""}</em>
                </span>
                <span className="ak-tpick-key">{i + 1}</span>
              </button>
            ))}
          </div>
          <p className="ak-pickhint">
            {pickedPath
              ? "1–3 agent · [ ] project · Enter · Esc"
              : "Add a project in Settings first"}
          </p>
          {recent.length > 0 && (
            <div className="ak-trec">
              <p className="ak-tlab">Bring back</p>
              {recent.slice(0, 2).map((r) => (
                <button key={r.id} type="button" onClick={() => void resumeSession(r.id)}>
                  <AgentMark agent={r.agent} sm />
                  <span className="truncate">{r.title}</span>
                  <small>
                    {r.agent} · {ago(r.startedAt)}
                  </small>
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}

/** A reply as plain text for a tile: no **, `, # or [link](url) marks. */
function plain(md: string): string {
  return md
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/(\*\*|__|`)/g, "")
    .replace(/^\s*(#+|[-*]|\d+\.)\s+/gm, "")
    .replace(/\s+/g, " ")
    .trim();
}

export function Board({ sessions, maxHeight }: { sessions: Session[]; keys?: boolean; maxHeight: number }) {
  const focus = useAgents((s) => s.focus);
  const live = sessions.filter((s) => !s.restored || s.status !== "ended");
  const paused = sessions.filter((s) => s.restored && s.status === "ended");
  const target = live.find((s) => s.id === focus) ?? live.find((s) => s.question) ?? live[0] ?? null;
  const [text, setText] = useState("");
  const grid = useRef<HTMLDivElement>(null);
  const [below, setBelow] = useState(0);
  const [openNew, setOpenNew] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const targetId = target?.id;
  const focusComposer = () => {
    requestAnimationFrame(() => inputRef.current?.focus({ preventScroll: true }));
  };
  // Ready to type on open, and again when Ctrl 1-9 picks another tile.
  // biome-ignore lint/correctness/useExhaustiveDependencies: focus again when the target changes
  useEffect(() => {
    focusComposer();
  }, [targetId]);

  // Ctrl N: new session. Ctrl 1–9: focus a live session tile.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.altKey || e.metaKey || e.shiftKey) return;
      if (e.key.toLowerCase() === "n") {
        e.preventDefault();
        setOpenNew((t) => t + 1);
        return;
      }
      const n = Number(e.key);
      if (!n || n > 9 || n > live.length) return;
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
        <div className="ak-grid" style={{ maxHeight }}>
          <EmptySlot recent={[]} openSignal={openNew} />
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
        {limits.map(([a, line]) => (
          <span key={a} className="ak-blim">
            <AgentMark agent={a} sm />
            {line.split(",")[0]}
          </span>
        ))}
      </div>
      <div className="relative">
        <div
          ref={grid}
          className="ak-grid"
          data-wide={wideScreen()}
          data-more={below > 0}
          style={{ maxHeight }}
          onScroll={measure}
        >
          {live.map((s, i) => (
            <Tile key={s.id} s={s} n={i + 1} focused={s.id === target?.id} />
          ))}
          <EmptySlot recent={paused} openSignal={openNew} onDismiss={focusComposer} />
        </div>
        {below > 0 && <p className="ak-below">{below} more below</p>}
      </div>
      {target && (
        <div className="ak-composer ak-two">
          <div className="ak-to">
            To
            <AgentMark agent={target.agent} sm />
            <b className="truncate">
              {target.agent}
              {target.title ? ` · ${target.title}` : ""}
            </b>
          </div>
          <div className="ak-row">
            <input
              ref={inputRef}
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
