"use client";

import { useEffect, useState } from "react";
import { type Session, sendToSession, setSessionMode, setSessionPlan } from "@/lib/agents";
import { Markdown } from "@/lib/markdown";
import { claimsPlanReady, looksLikeQuestionPlan, parsePlanDocument, planFromSession } from "@/lib/plan";

/** Full plan document for Plan mode, title, step write-ups, assumptions, not just todos. */
export function PlanPanel({ session: s, onDraft }: { session: Session; onDraft?: (text: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const plan = s.plan;
  const doc = s.planDoc;
  const lastText = [...s.entries].reverse().find((e) => e.kind === "text");
  const readyClaim = lastText?.kind === "text" && claimsPlanReady(lastText.text) && plan.length === 0;
  const showEmpty = s.mode === "plan" && plan.length === 0 && readyClaim && s.status === "idle";

  useEffect(() => {
    if (s.mode !== "plan") return;
    const { items, doc: next } = planFromSession(s.plan, s.entries);
    if (items.length === 0) return;
    const bad = looksLikeQuestionPlan(s.plan);
    const richer =
      items.length > s.plan.length || bad || (items.some((p) => !!p.detail) && !s.plan.some((p) => !!p.detail));
    const needDoc = !!next && (!s.planDoc || s.planDoc.markdown !== next.markdown);
    if (richer || needDoc) setSessionPlan(s.id, items, next);
  }, [s.id, s.mode, s.plan, s.planDoc, s.entries]);

  useEffect(() => {
    if (!editing) setDraft(doc?.markdown ?? plan.map((p, i) => `${i + 1}. ${p.text}`).join("\n"));
  }, [editing, doc, plan]);

  if (plan.length === 0 && !showEmpty) return null;

  const saveEdits = () => {
    const parsed = parsePlanDocument(draft);
    const items =
      parsed?.items ??
      draft
        .split("\n")
        .map((line) =>
          line
            .replace(/^\s*\d+[.)]\s*/, "")
            .replace(/\*\*/g, "")
            .trim(),
        )
        .filter((t) => t.length > 0)
        .slice(0, 12)
        .map((text) => ({ text, status: "pending" as const }));
    if (items.length === 0) return;
    setSessionPlan(s.id, items, parsed);
    setEditing(false);
    sendToSession(s.id, `Please revise the plan to:\n\n${draft.trim()}`);
  };

  const implement = async () => {
    await setSessionMode(s.id, "edit");
    const body = doc?.markdown || plan.map((p, i) => `${i + 1}. ${p.text}${p.detail ? ` ${p.detail}` : ""}`).join("\n");
    sendToSession(s.id, `Implement the plan.\n\n${body}`);
  };

  const askForPlan = () => {
    sendToSession(
      s.id,
      "Show the full plan as a numbered list of concrete steps with detail under each. Update the collaboration plan with those steps.",
    );
  };

  if (plan.length === 0) {
    return (
      <div className="ak-plan-doc ak-in">
        <div className="ak-plan-doc-h">
          <span>Plan</span>
          <span>Not listed yet</span>
        </div>
        <p className="ak-plan-doc-empty">
          {readyClaim ? "The agent said the plan is ready, but no steps were listed." : "Waiting for plan steps."}
        </p>
        <div className="ak-chips mt-1">
          <button type="button" className="ak-chip primary chip" onClick={askForPlan}>
            Show plan steps
          </button>
        </div>
      </div>
    );
  }

  const done = plan.filter((p) => p.status === "completed").length;
  const at = Math.min(done + 1, plan.length);
  const title = doc?.title || "Plan";

  return (
    <div className="ak-plan-doc ak-in">
      <div className="ak-plan-doc-h">
        <span>Plan</span>
        <span>
          {at} of {plan.length}
        </span>
      </div>
      {editing ? (
        <textarea
          className="ak-plan-edit"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          rows={Math.min(22, Math.max(10, draft.split("\n").length + 2))}
          aria-label="Edit plan"
        />
      ) : (
        <div className="ak-plan-doc-body">
          <p className="ak-plan-doc-title">{title}</p>
          <ol className="ak-plan-doc-steps">
            {plan.map((p, i) => (
              <li key={p.text} className="ak-plan-step" data-s={p.status}>
                <div className="ak-plan-step-h">
                  <span className="ak-cb" aria-hidden="true" />
                  <span className="ak-plan-step-n">{i + 1}.</span>
                  <span className="ak-plan-step-t">{p.text}</span>
                </div>
                {p.detail && (
                  <div className="ak-plan-step-d">
                    <Markdown text={p.detail} />
                  </div>
                )}
              </li>
            ))}
          </ol>
          {doc?.assumptions && (
            <p className="ak-plan-doc-notes">
              <span>Assumptions</span>
              {doc.assumptions}
            </p>
          )}
        </div>
      )}
      {s.mode === "plan" && s.status === "idle" && (
        <div className="ak-chips mt-1.5">
          {editing ? (
            <>
              <button type="button" className="ak-chip primary chip" onClick={saveEdits}>
                Save &amp; send
              </button>
              <button type="button" className="ak-chip chip" onClick={() => setEditing(false)}>
                Cancel
              </button>
            </>
          ) : (
            <>
              <button type="button" className="ak-chip primary chip" onClick={() => void implement()}>
                Implement
              </button>
              <button type="button" className="ak-chip chip" onClick={() => setEditing(true)}>
                Edit
              </button>
              <button type="button" className="ak-chip chip" onClick={() => onDraft?.("Please revise the plan: ")}>
                Ask to revise
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
