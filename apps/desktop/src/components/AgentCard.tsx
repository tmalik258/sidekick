"use client";

// One agent at work, opened from its pill: which agent and project, what it
// is on, how far along the plan, its latest steps, and a way to open, pause
// or stop it.

import { interruptSession, pauseSession, resumePaused, type Session, type Step, useAgents } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { Ic } from "./Icon";

export function AgentCard({ id }: { id: string }) {
  const s = useAgents((st) => st.sessions.find((x) => x.id === id));
  if (!s) return null;
  const plan = s.plan;
  const at = plan.findIndex((p) => p.status === "in_progress");
  const step = at >= 0 ? at : plan.filter((p) => p.status === "completed").length;
  const steps = s.entries
    .filter((e): e is { kind: "step"; step: Step } => e.kind === "step")
    .map((e) => e.step)
    .slice(-2);
  const open = () => {
    useAgents.setState({ tab: "agents", current: s.id });
    void api.askOpen();
  };
  return (
    <div className="rd-glance flex flex-col gap-2.5">
      <div className="rd-card-head">
        <p className="rd-eyebrow flex items-center gap-1.5">
          <span className="rd-dot" style={{ background: s.status === "waiting" ? "var(--warn)" : "#a8c0cc" }} />
          {s.agent} in {s.project}
        </p>
        <p className="rd-title">{s.title}</p>
        <p className="rd-detail">{detail(s, step)}</p>
      </div>
      <div className="flex flex-col gap-1" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
        {plan.length > 1 && (
          <div className="rd-steps" aria-hidden>
            {plan.map((p, i) => (
              <i key={p.text} data-s={p.status === "completed" ? "done" : i === step ? "now" : undefined} />
            ))}
          </div>
        )}
        {steps.map((st) => (
          <div key={st.id} className="rd-list-row mg-row">
            <span style={{ color: st.state === "failed" ? "var(--err)" : "var(--ink-3)", display: "inline-flex" }}>
              <Ic name={st.state === "failed" ? "x" : st.state === "running" ? "dot" : "check"} size={12} />
            </span>
            <span className="t mono">{st.label}</span>
            {st.detail && <span className="r">{st.detail}</span>}
          </div>
        ))}
        <div className="mt-1.5 flex items-center gap-1.5">
          <button type="button" className="rd-btn primary" onClick={open}>
            Open session
          </button>
          {(s.status === "working" || s.paused) && (
            <button
              type="button"
              className="rd-btn ghost chip-s ic-only"
              aria-label={s.paused ? "Resume" : "Pause"}
              title={s.paused ? "Resume" : "Pause"}
              onClick={() => (s.paused ? resumePaused(s.id) : pauseSession(s.id))}
            >
              <Ic name={s.paused ? "play" : "pause"} size={12} />
            </button>
          )}
          {s.status === "working" && (
            <button
              type="button"
              className="rd-btn ghost ml-auto chip-s ic-only"
              aria-label="Stop"
              style={{ color: "var(--err)" }}
              onClick={() => interruptSession(s.id)}
            >
              <Ic name="stop" size={12} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function detail(s: Session, step: number): string {
  const where = s.worktree ?? s.branch;
  if (s.status === "waiting") return s.question?.label ?? "Waiting for your answer.";
  if (s.plan.length > 1)
    return `Step ${Math.min(step + 1, s.plan.length)} of ${s.plan.length}${where ? `, on ${where}` : ""}`;
  return where ? `On ${where}` : "Working";
}
