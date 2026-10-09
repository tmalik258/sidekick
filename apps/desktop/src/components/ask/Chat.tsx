"use client";

// The conversation: turns, live steps, the streamed answer, retry and handoff.

import { useEffect, useState } from "react";
import { handOff } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import { Markdown } from "@/lib/markdown";
import { splitOptions } from "@/lib/options";
import { useReveal } from "@/lib/reveal";
import { retryLast, useSidekick } from "@/lib/store";
import { type Agents, type AiSettings, PROVIDER_LABELS, type Turn } from "@/lib/types";
import { AnswerOptions, Proposals, pendingCount } from "./Proposals";

/** The YAML block of an answer, if it has one. */
export function yamlBlock(text: string): string | null {
  const m = text.match(/```ya?ml\s*\n([\s\S]*?)```/);
  return m ? m[1].trim() : null;
}

export function AddSkill({ yaml }: { yaml: string }) {
  const [state, setState] = useState<{ ok: boolean; message: string } | null>(null);
  const install = async () => {
    try {
      const name = await api.skillInstall(yaml);
      setState({ ok: true, message: `Added "${name}". Manage it in Settings > Skills.` });
    } catch (err) {
      setState({ ok: false, message: String(err) });
    }
  };
  if (state)
    return <p className={`mt-1.5 text-[12.5px] ${state.ok ? "text-[#30d158]" : "text-[#ffb4ae]"}`}>{state.message}</p>;
  return (
    <button type="button" onClick={() => void install()} className="ak-chip primary chip justify-self-start">
      Add skill
    </button>
  );
}

export function Chat({ turns, askedInBar }: { turns: Turn[]; askedInBar: boolean }) {
  const skillMode = useSidekick((s) => s.chatSkill);
  const ai = useSidekick((s) => s.settings.ai);
  const last = turns.at(-1);
  const options = last?.role === "assistant" && !last.streaming ? splitOptions(last.content).options : [];
  // The newest question reads in the input line; only earlier ones show here.
  const lastAsk = turns.findLastIndex((t) => t.role === "user");
  return (
    <div className="ak-body py-0.5">
      {turns.map((t, i) => {
        if (t.role === "user") {
          if (askedInBar && i === lastAsk) return null;
          return (
            // biome-ignore lint/suspicious/noArrayIndexKey: turns only ever append
            <div key={i} className="ak-um ak-in">
              {t.content}
              {t.screen && (
                <span className="mt-0.5 block text-[11px] text-[rgb(235_235_245/0.5)]">with screenshot</span>
              )}
            </div>
          );
        }
        const isLast = i === turns.length - 1;
        const steps = t.steps ?? [];
        return (
          // biome-ignore lint/suspicious/noArrayIndexKey: turns only ever append
          <div key={i} className="group grid gap-2">
            {steps.length > 0 && !t.streaming && <Steps steps={steps} running={false} tookMs={t.tookMs} />}
            {t.streaming && steps.length > 1 && <Steps steps={steps} running tookMs={t.tookMs} />}
            {t.content ? (
              <div className="ak-ans">
                <Answer text={splitOptions(t.content, t.streaming).body} live={isLast && !!t.streaming} />
              </div>
            ) : t.streaming ? (
              <LiveStep step={t.tool ?? null} since={t.startedAt} />
            ) : null}
            {t.streaming && t.content && t.tool && (
              <p className="ak-status">
                <span className="shimmer-text text-[rgb(235_235_245/0.6)]">{t.tool}...</span>
              </p>
            )}
            {t.error && <p className="ak-err">{t.error}</p>}
            {!t.streaming && t.provider && (
              <p className="ak-tag mono">
                <b>
                  {TAG_NAMES[t.provider] ?? PROVIDER_LABELS[t.provider] ?? t.provider}
                  {modelOf(ai, t.provider) && ` · ${modelOf(ai, t.provider)}`}
                </b>
                {t.firstMs !== undefined && <span>first word {seconds(t.firstMs)}</span>}
                <span className="ml-auto opacity-0 transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100">
                  {isLast && i > 0 && !skillMode && <SaveRecipe prompt={turns[i - 1]?.content ?? ""} />}
                </span>
              </p>
            )}
            {t.error && !t.streaming && isLast && <Retry />}
            {skillMode && !t.streaming && yamlBlock(t.content) && <AddSkill yaml={yamlBlock(t.content) ?? ""} />}
            {t.proposals && t.proposals.length > 0 && <Proposals items={t.proposals} keys={isLast} />}
            {isLast && options.length > 0 && <AnswerOptions options={options} start={pendingCount(t.proposals)} />}
            {/* Offered when the local model gives up; Ctrl Enter works when an agent is installed. */}
            {!t.streaming && isLast && (t.handoff || t.error) && <Handoff turns={turns} reason={t.handoff ?? null} />}
          </div>
        );
      })}
    </div>
  );
}

/** Who answered, short, as the tag under an answer reads. */
const TAG_NAMES: Record<string, string> = {
  local: "Local",
  claude_code: "Claude Code",
  codex: "Codex",
  anthropic: "Claude API",
};

/** The model name under an answer, short: "qwen3:4b", "sonnet". */
function modelOf(ai: AiSettings, provider: string): string {
  const m =
    provider === "local"
      ? ai.local.model
      : provider === "claude_code"
        ? ai.claudeCode.model
        : provider === "codex"
          ? ai.codex.model
          : provider === "anthropic"
            ? ai.anthropic.model
            : "";
  return m.replace(/^claude-/, "").replace(/-\d{8}$/, "");
}

/** Keeps the question as a recipe, to run again by name or on a trigger. */
export function SaveRecipe({ prompt }: { prompt: string }) {
  const [saved, setSaved] = useState<string | null>(null);
  if (!prompt.trim()) return null;
  if (saved) return <span>{saved}</span>;
  return (
    <button
      type="button"
      onClick={() =>
        void api
          .recipeSave({ id: "", name: "", prompt, trigger: { when: "manual" }, auto: false, enabled: true })
          .then(setSaved)
          .catch((e) => setSaved(String(e)))
      }
      className="chip hover:text-white"
    >
      Save as recipe
    </button>
  );
}

/** The steps of a multi-step task: done ones ticked, the current one live.
 * Folds to one line once the answer is in. */
export function Steps({ steps, running, tookMs }: { steps: string[]; running: boolean; tookMs?: number }) {
  const [open, setOpen] = useState(false);
  if (!running && !open) {
    return (
      <button type="button" onClick={() => setOpen(true)} className="ak-steps chip text-left hover:text-white">
        <span className="ok">✓</span>
        {steps.length} {steps.length === 1 ? "step" : "steps"}
        {tookMs !== undefined && ` · ${seconds(tookMs)}`}
      </button>
    );
  }
  // Steps only grow, in order, so their position is a stable id.
  const keyed = steps.map((s, n) => ({ s, id: `${n}:${s}` }));
  return (
    <ol className="grid gap-0.5 px-1 text-[12px]">
      {keyed.map(({ s, id }, i) => {
        const live = running && i === steps.length - 1;
        return (
          <li
            key={id}
            className={`flex items-center gap-1.5 ${live ? "text-white/80" : "text-[rgb(235_235_245/0.36)]"}`}
          >
            <span className={live ? "ak-dot animate-pulse bg-white" : "text-[#30d158]"}>{live ? "" : "✓"}</span>
            {s}
          </li>
        );
      })}
    </ol>
  );
}

/** The coding agent that gets handoffs, or null when neither is installed. */
export function useAgentName(): string | null {
  const { data } = useCached<Agents>("agents", api.agentsStatus);
  return data?.handoff ?? null;
}

/** "Try again" (Alt R) under a failed answer. */
export function Retry() {
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.altKey && !e.ctrlKey && e.key.toLowerCase() === "r") {
        e.preventDefault();
        retryLast();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <button type="button" onClick={() => retryLast()} className="ak-chip chip justify-self-start">
      Try again
      <kbd>Alt R</kbd>
    </button>
  );
}

export function Handoff({ turns, reason }: { turns: Turn[]; reason: string | null }) {
  const [state, setState] = useState<string>("idle");
  const agent = useAgentName();
  if (!agent) return null;
  const go = () => {
    setState("opening");
    const messages = turns.filter((t) => !t.error && t.content.trim()).map(({ role, content }) => ({ role, content }));
    handOff(messages, reason)
      .then(() => setState("opened"))
      .catch((e) => setState(String(e)));
  };
  if (state === "opened") {
    return <p className="mt-1.5 text-[12px] text-[rgb(235_235_245/0.55)]">{agent} carries on in the Agents tab.</p>;
  }
  return (
    <div className="mt-1.5 flex flex-col gap-1">
      {reason && <p className="text-[12px] text-[rgb(235_235_245/0.6)]">Too much for the local model: {reason}.</p>}
      <button
        type="button"
        disabled={state === "opening"}
        onClick={go}
        className={`ak-chip chip self-start disabled:opacity-50 ${reason ? "primary" : ""}`}
      >
        {state === "opening" ? "Opening..." : `Continue in ${agent}`}
      </button>
      {state !== "idle" && state !== "opening" && <p className="text-[12px] text-[#ffb4ae]">{state}</p>}
    </div>
  );
}

/** An answer as it arrives: word by word at a reading pace. */
export function Answer({ text, live }: { text: string; live: boolean }) {
  const shown = useReveal(text, live);
  // The first words settle in from a slight blur; after that, words stream.
  return (
    <div className="settle">
      <Markdown text={shown} />
    </div>
  );
}

/** "1.2 s", or "850 ms" under a second. */
export function seconds(ms: number): string {
  return ms < 1000 ? `${Math.round(ms / 10) * 10} ms` : `${(ms / 1000).toFixed(1)} s`;
}

/** Before the first words: one shimmering line saying what Sidekick is
 * doing, with a timer. */
export function LiveStep({ step, since }: { step: string | null; since?: number }) {
  const now = useNow(100);
  const elapsed = since ? Math.max(0, now - since) : null;
  return (
    <p className="ak-status" role="status" aria-live="polite">
      <span className="shimmer-text text-[rgb(235_235_245/0.6)]">{step ? `${step}...` : "Thinking..."}</span>
      {elapsed !== null && (
        <span className="ak-timer mono" aria-hidden="true">
          {(elapsed / 1000).toFixed(1)} s
        </span>
      )}
    </p>
  );
}

export function Thinking() {
  return (
    <span className="inline-flex gap-1 py-2" role="status" aria-label="Thinking">
      <span className="thinking-dot" />
      <span className="thinking-dot" />
      <span className="thinking-dot" />
    </span>
  );
}
