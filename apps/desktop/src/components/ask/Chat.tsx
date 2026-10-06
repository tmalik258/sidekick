"use client";

// The conversation: turns, live steps, the streamed answer, retry and handoff.

import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { handOff } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import { Markdown } from "@/lib/markdown";
import { splitOptions } from "@/lib/options";
import { useReveal } from "@/lib/reveal";
import { retryLast, useSidekick } from "@/lib/store";
import { type Agents, PROVIDER_LABELS, type Turn } from "@/lib/types";
import { AnswerOptions, Proposals, pendingCount } from "./Proposals";
import { ease } from "./parts";

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
    <button
      type="button"
      onClick={() => void install()}
      className="chip mt-1.5 h-8 rounded-full bg-white px-3.5 text-[13px] font-medium text-black hover:bg-white/90"
    >
      Add skill
    </button>
  );
}

export function Chat({ turns }: { turns: Turn[] }) {
  const skillMode = useSidekick((s) => s.chatSkill);
  const last = turns.at(-1);
  const options = last?.role === "assistant" && !last.streaming ? splitOptions(last.content).options : [];
  return (
    <div className="space-y-2.5 py-1">
      {turns.map((t, i) => (
        <motion.div
          // biome-ignore lint/suspicious/noArrayIndexKey: turns only ever append
          key={i}
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.2, ease }}
          className={t.role === "user" ? "flex justify-end" : ""}
        >
          {t.role === "user" ? (
            <div className="max-w-[85%] rounded-[18px] rounded-br-md bg-white/[0.14] px-3 py-1.5 text-[13.5px] whitespace-pre-wrap">
              {t.content}
              {t.screen && (
                <span className="mt-0.5 block text-[11px] text-[rgb(235_235_245/0.5)]">with screenshot</span>
              )}
            </div>
          ) : (
            <div className="group text-[13.5px] leading-relaxed text-white/90">
              {t.content ? (
                <Answer
                  text={splitOptions(t.content, t.streaming).body}
                  live={i === turns.length - 1 && !!t.streaming}
                />
              ) : t.streaming ? (
                <LiveStep step={t.tool ?? null} since={t.startedAt} />
              ) : null}
              {(t.steps?.length ?? 0) > 1 ? (
                <Steps steps={t.steps ?? []} running={!!t.streaming} tookMs={t.tookMs} />
              ) : (
                t.streaming &&
                t.content &&
                t.tool && <p className="mt-0.5 text-[11.5px] text-[rgb(235_235_245/0.5)]">{t.tool}...</p>
              )}
              {t.error && (
                <p className="mt-1 rounded-xl bg-[#ff453a]/15 px-3 py-2 text-[12.5px] text-[#ffb4ae]">{t.error}</p>
              )}
              {t.error && !t.streaming && i === turns.length - 1 && <Retry />}
              {skillMode && !t.streaming && yamlBlock(t.content) && <AddSkill yaml={yamlBlock(t.content) ?? ""} />}
              {!t.streaming && t.provider && (
                <p className="mt-1 flex items-center gap-2 text-[11px] text-[rgb(235_235_245/0.38)]">
                  {/* Who answered and how fast. */}
                  <span>
                    {PROVIDER_LABELS[t.provider] ?? t.provider}
                    {t.tookMs !== undefined && ` · ${seconds(t.tookMs)}`}
                  </span>
                  <span className="opacity-0 transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100">
                    {i === turns.length - 1 && i > 0 && !skillMode && (
                      <SaveRecipe prompt={turns[i - 1]?.content ?? ""} />
                    )}
                  </span>
                </p>
              )}
              {t.proposals && t.proposals.length > 0 && <Proposals items={t.proposals} keys={i === turns.length - 1} />}
              {i === turns.length - 1 && options.length > 0 && (
                <AnswerOptions options={options} start={pendingCount(t.proposals)} />
              )}
              {/* Offered when the local model gives up; Ctrl Enter works when an agent is installed. */}
              {!t.streaming && i === turns.length - 1 && (t.handoff || t.error) && (
                <Handoff turns={turns} reason={t.handoff ?? null} />
              )}
            </div>
          )}
        </motion.div>
      ))}
    </div>
  );
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
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="chip mt-1 text-[11.5px] text-[rgb(235_235_245/0.45)] hover:text-white"
      >
        {steps.length} steps{tookMs !== undefined && ` · ${seconds(tookMs)}`} ›
      </button>
    );
  }
  // Steps only grow, in order, so their position is a stable id.
  const keyed = steps.map((s, n) => ({ s, id: `${n}:${s}` }));
  return (
    <ol className="mt-1 flex flex-col gap-0.5 text-[11.5px]">
      {keyed.map(({ s, id }, i) => {
        const live = running && i === steps.length - 1;
        return (
          <li
            key={id}
            className={`flex items-center gap-1.5 ${live ? "text-white/80" : "text-[rgb(235_235_245/0.45)]"}`}
          >
            <span
              className={`grid size-3 shrink-0 place-items-center rounded-full text-[8px] ${
                live ? "animate-pulse bg-[#0a84ff]/60" : "bg-[#30d158]/70 text-black transition-colors duration-150"
              }`}
            >
              {live ? "" : "✓"}
            </span>
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
    <button
      type="button"
      onClick={() => retryLast()}
      className="chip mt-1.5 flex h-8 items-center gap-2 self-start rounded-full bg-white/[0.12] px-3.5 text-[13px] font-medium text-white/90 hover:bg-white/[0.2]"
    >
      Try again
      <kbd className="font-sans text-[11px] text-white/40">Alt R</kbd>
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
        className={`chip h-8 self-start rounded-full px-3.5 text-[13px] font-medium disabled:opacity-50 ${
          reason ? "bg-white text-black hover:bg-white/90" : "bg-white/[0.12] text-white/90 hover:bg-white/[0.2]"
        }`}
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
    <p className="flex items-center gap-2 py-1.5 text-[13px]" role="status" aria-live="polite">
      <span className="shimmer-text text-[rgb(235_235_245/0.6)]">{step ? `${step}...` : "Thinking..."}</span>
      {elapsed !== null && elapsed > 400 && (
        <span className="text-[11px] text-[rgb(235_235_245/0.35)] tabular-nums" aria-hidden="true">
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
