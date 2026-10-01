"use client";

// First run (P5 onboarding): three short steps inside the island. What
// Sidekick does and what stays private, which AI to use, and a few extras.
// Finishing (or skipping) marks onboarding done; it never shows again.

import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { updateSettings, useSidekick } from "@/lib/store";
import { PROVIDER_LABELS, type ProviderStatus } from "@/lib/types";
import { ASK_ORB } from "./AskPanel";

const STEPS = ["Welcome", "Your AI", "Extras"] as const;

const HOW_TO: Record<string, string> = {
  claude_code: "npm install -g @anthropic-ai/claude-code, then run claude once to sign in",
  anthropic: "Set ANTHROPIC_API_KEY in your environment, then restart Sidekick",
  local: "winget install Ollama.Ollama, then ollama pull qwen3:4b",
};

const PROVIDER_HINT: Record<string, string> = {
  claude_code: "Your Claude subscription",
  anthropic: "Pay as you go API key",
  local: "Free, runs on this PC",
};

export function IslandWelcome() {
  const [step, setStep] = useState(0);
  const finish = () => {
    void updateSettings({ onboarded: true });
    void api.askClose();
  };
  return (
    <div className="flex flex-col">
      <div className="mb-3 flex h-[30px] items-center gap-2" style={{ paddingLeft: ASK_ORB + 10 }}>
        <h1 className="flex-1 font-display text-[17px] font-semibold tracking-[-0.015em]">{STEPS[step]}</h1>
        <div className="flex gap-1" role="img" aria-label={`Step ${step + 1} of ${STEPS.length}`}>
          {STEPS.map((s, i) => (
            <span
              key={s}
              className={`h-1.5 rounded-full transition-all duration-300 ${i === step ? "w-4 bg-white" : "w-1.5 bg-white/25"}`}
            />
          ))}
        </div>
      </div>

      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div
          key={step}
          initial={{ opacity: 0, x: 12, filter: "blur(4px)" }}
          animate={{ opacity: 1, x: 0, filter: "blur(0px)" }}
          exit={{ opacity: 0, x: -12, filter: "blur(4px)", transition: { duration: 0.12 } }}
          transition={{ duration: 0.26, ease: [0.23, 1, 0.32, 1] }}
          className="min-h-[220px]"
        >
          {step === 0 && <Intro />}
          {step === 1 && <Providers />}
          {step === 2 && <Extras />}
        </motion.div>
      </AnimatePresence>

      <div className="mt-4 flex items-center justify-between pb-1">
        <button
          type="button"
          onClick={finish}
          className="chip rounded-full px-2.5 py-1 text-[12.5px] text-[rgb(235_235_245/0.6)] hover:text-white"
        >
          Skip
        </button>
        <div className="flex gap-1.5">
          {step > 0 && (
            <button
              type="button"
              onClick={() => setStep(step - 1)}
              className="chip rounded-full bg-white/[0.12] px-3.5 py-1.5 text-[13px] font-medium text-white/90 hover:bg-white/[0.2]"
            >
              Back
            </button>
          )}
          <button
            type="button"
            onClick={() => (step < STEPS.length - 1 ? setStep(step + 1) : finish())}
            className="chip rounded-full bg-white px-3.5 py-1.5 text-[13px] font-medium text-black hover:bg-white/90"
          >
            {step < STEPS.length - 1 ? "Next" : "Start"}
          </button>
        </div>
      </div>
    </div>
  );
}

function Intro() {
  const hotkey = useSidekick((s) => s.settings.paletteHotkey);
  return (
    <div className="flex flex-col gap-3 text-[13.5px] leading-relaxed text-white/90">
      <p>
        I live up here and notice moments I can help with: a finished download, a dev server starting, a meeting about
        to begin. I offer one or two buttons, and you decide.
      </p>
      <ul className="flex flex-col gap-2">
        <Point title="Press anytime">
          <Kbd>{hotkey}</Kbd> opens Ask mode for commands, search and chat.
        </Point>
        <Point title="Private by default">Everything I see stays on this PC. Pause me from the tray.</Point>
        <Point title="Nothing risky on its own">
          Deleting, installing or stopping things always asks first, and files I create can be undone.
        </Point>
      </ul>
    </div>
  );
}

function Providers() {
  const ai = useSidekick((s) => s.settings.ai);
  const [status, setStatus] = useState<ProviderStatus[] | null>(null);
  useEffect(() => {
    void api.aiStatus().then(setStatus);
  }, []);
  const toggle = (id: string, enabled: boolean) => {
    const key = id === "claude_code" ? "claudeCode" : (id as "anthropic" | "local");
    void updateSettings({ ai: { ...ai, [key]: { ...ai[key], enabled } } });
  };
  const enabled = (id: string) =>
    id === "claude_code" ? ai.claudeCode.enabled : id === "anthropic" ? ai.anthropic.enabled : ai.local.enabled;
  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      <p className="text-[rgb(235_235_245/0.7)]">
        Chat, summaries and drafts use one of these. I try them in this order and work without any of them.
      </p>
      {(status ?? [])
        .filter((p) => p.id !== "semif")
        .map((p) => (
          <div key={p.id} className="flex items-center gap-3 rounded-2xl bg-white/[0.06] px-3.5 py-2.5">
            <span
              className={`size-2 shrink-0 rounded-full ${p.available ? "bg-[#30d158]" : "bg-white/25"}`}
              style={p.available ? { boxShadow: "0 0 8px #30d158" } : undefined}
            />
            <div className="min-w-0 flex-1">
              <p className="font-medium text-white">
                {PROVIDER_LABELS[p.id] ?? p.id}{" "}
                <span className="font-normal text-[rgb(235_235_245/0.5)]">{PROVIDER_HINT[p.id]}</span>
              </p>
              <p className="truncate text-[11.5px] text-[rgb(235_235_245/0.55)]" title={HOW_TO[p.id]}>
                {p.available ? "Ready" : HOW_TO[p.id]}
              </p>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={enabled(p.id)}
              aria-label={PROVIDER_LABELS[p.id] ?? p.id}
              onClick={() => toggle(p.id, !enabled(p.id))}
              className={`relative h-[22px] w-[38px] shrink-0 rounded-full transition-colors ${enabled(p.id) ? "bg-[#30d158]" : "bg-white/20"}`}
            >
              <span
                className="absolute top-[2px] left-[2px] size-[18px] rounded-full bg-white transition-transform duration-200"
                style={{ transform: enabled(p.id) ? "translateX(16px)" : "none" }}
              />
            </button>
          </div>
        ))}
      {status === null && <p className="text-[rgb(235_235_245/0.5)]">Checking what is installed...</p>}
    </div>
  );
}

function Extras() {
  const settings = useSidekick((s) => s.settings);
  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      <Choice
        title="Talk to me"
        hint='Say "Hey Sidekick". Downloads about 180 MB of speech models once; all on this PC.'
        on={settings.voice.enabled}
        onChange={(enabled) => {
          void updateSettings({ voice: { ...settings.voice, enabled } });
          if (enabled) void api.voiceDownload();
        }}
      />
      <Choice
        title="Start with Windows"
        hint="So I am there when you sit down."
        on={settings.launchAtLogin}
        onChange={(launchAtLogin) => void updateSettings({ launchAtLogin })}
      />
      <p className="px-1 pt-1 text-[12px] leading-relaxed text-[rgb(235_235_245/0.55)]">
        Later, in Settings: pair the browser extension (Browser), add your calendar link for meeting reminders (Today),
        and choose folders to search (Search).
      </p>
    </div>
  );
}

function Choice({
  title,
  hint,
  on,
  onChange,
}: {
  title: string;
  hint: string;
  on: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={() => onChange(!on)}
      className={`chip flex items-center gap-3 rounded-2xl px-3.5 py-2.5 text-left ring-1 ${
        on ? "bg-white/[0.1] ring-[#0a84ff]" : "bg-white/[0.06] ring-transparent hover:bg-white/[0.09]"
      }`}
    >
      <div className="min-w-0 flex-1">
        <p className="font-medium text-white">{title}</p>
        <p className="text-[11.5px] text-[rgb(235_235_245/0.55)]">{hint}</p>
      </div>
      <span
        className={`grid size-5 shrink-0 place-items-center rounded-full text-[11px] font-bold ${
          on ? "bg-[#0a84ff] text-white" : "bg-white/15 text-transparent"
        }`}
      >
        ✓
      </span>
    </button>
  );
}

function Point({ title, children }: { title: string; children: ReactNode }) {
  return (
    <li className="rounded-2xl bg-white/[0.06] px-3.5 py-2.5">
      <p className="font-medium text-white">{title}</p>
      <p className="text-[12.5px] text-[rgb(235_235_245/0.6)]">{children}</p>
    </li>
  );
}

function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="rounded-md bg-white/15 px-1.5 py-0.5 font-sans text-[11.5px] text-white">{children}</kbd>;
}
