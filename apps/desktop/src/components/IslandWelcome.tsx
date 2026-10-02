"use client";

// First run (P5 onboarding): short steps inside the island. What Sidekick
// does and what stays private, then a live checklist of the AI, connections
// and tools to set up, each with the exact command to run, and a few extras.
// Finishing (or skipping) marks onboarding done; the same checklist stays in
// Settings > Setup. The current step is saved in settings so a restart
// resumes where the user left off.

import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { updateSettings, useSidekick } from "@/lib/store";
import type { SetupFound } from "@/lib/types";
import { onWelcomeHeard, welcomeCommand } from "@/lib/welcomeVoice";
import { ASK_ORB, Hearing } from "./AskPanel";
import { Icon } from "./Icon";
import { SetupChecklist } from "./SetupChecklist";
import { SpokenLine, wasHeard } from "./SpokenLine";

const STEPS = ["Welcome", "Your AI", "Connect", "Tools", "Extras"] as const;

function clampStep(n: number): number {
  if (!Number.isFinite(n) || n < 0) return 0;
  return Math.min(Math.trunc(n), STEPS.length - 1);
}

export function IslandWelcome() {
  const saved = useSidekick((s) => s.settings.welcomeStep);
  const [step, setStep] = useState(() => clampStep(saved));

  useEffect(() => {
    setStep(clampStep(saved));
  }, [saved]);

  const go = (next: number) => {
    const n = clampStep(next);
    setStep(n);
    void updateSettings({ welcomeStep: n });
    // Sidekick talks each step through once; a step already heard is shown
    // whole and quiet. Either way the line being said is cut off.
    if (wasHeard(n)) void api.voiceStop();
    else void api.voiceWelcomeStep(n);
  };
  const finish = (completed: boolean) => {
    void api.voiceStop().then(() => {
      if (completed) void api.voiceSay("That's everything. Welcome aboard.");
    });
    // Onboarded must stick before close; otherwise Rust keeps welcome locked.
    void updateSettings({ onboarded: true, welcomeStep: step }).then(() => api.askClose());
  };

  // "Hey Sidekick, next": spoken replies move through the steps.
  const hearing = useSidekick((s) => s.hearing);
  const canTalk = useSidekick((s) => s.settings.voice.enabled && (s.voiceStatus?.listening ?? false));
  const last = step === STEPS.length - 1;
  useEffect(() =>
    onWelcomeHeard((text) => {
      if (!text.trim()) return;
      const command = welcomeCommand(text);
      if (command === "back") go(step - 1);
      else if (command === "skip") finish(false);
      else if (command === "hide") void api.askDeferWelcome();
      else if (command === "next" || command === "finish") last ? finish(true) : go(step + 1);
      else void api.voiceSay("Let's finish setting up first. Then ask me anything.");
    }),
  );
  const talk = () => {
    useSidekick.setState({ hearing: "" });
    api.voiceListen().catch(() => useSidekick.setState({ hearing: null }));
  };
  return (
    <div className="flex flex-col">
      <div className="mb-3 flex h-7.5 items-center gap-2" style={{ paddingLeft: ASK_ORB + 10 }}>
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
          className="settings-scroll -mr-3 max-h-95 overflow-y-auto pr-3 pl-0.5"
        >
          <Spoken step={step}>
            {step === 0 && (
              <>
                <Intro />
                <FoundCard onDone={() => go(1)} />
              </>
            )}
            {step === 1 && <SetupChecklist groups={["ai"]} compact inlineGuides />}
            {step === 2 && <SetupChecklist groups={["connect"]} compact inlineGuides />}
            {step === 3 && <SetupChecklist groups={["tools"]} compact inlineGuides />}
            {step === 4 && <Extras />}
          </Spoken>
        </motion.div>
      </AnimatePresence>

      {hearing !== null && (
        <div className="mt-3 flex items-center gap-2 rounded-xl bg-white/[0.06] px-3 py-2">
          <Hearing text={hearing} />
          <button
            type="button"
            onClick={() => {
              useSidekick.setState({ hearing: null });
              void api.voiceStop();
            }}
            className="chip shrink-0 rounded-full px-2.5 py-1 text-[12.5px] text-[rgb(235_235_245/0.6)] hover:text-white"
          >
            Stop
          </button>
        </div>
      )}

      <div className="mt-4 flex items-center justify-between pb-1">
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => finish(false)}
            className="chip rounded-full px-2.5 py-1 text-[12.5px] text-[rgb(235_235_245/0.6)] hover:text-white"
          >
            Skip
          </button>
          <button
            type="button"
            onClick={() => void api.askDeferWelcome()}
            className="chip rounded-full px-2.5 py-1 text-[12.5px] text-[rgb(235_235_245/0.6)] hover:text-white"
          >
            Hide
          </button>
          {canTalk && hearing === null && (
            <button
              type="button"
              aria-label="Talk (or say Hey Sidekick, next)"
              title="Talk (or say Hey Sidekick, next)"
              onClick={talk}
              className="chip grid size-7 place-items-center rounded-full bg-white/[0.12] text-white/85 hover:bg-white/[0.2]"
            >
              <Icon name="mic" size={14} />
            </button>
          )}
        </div>
        <div className="flex gap-1.5">
          {step > 0 && (
            <button
              type="button"
              onClick={() => go(step - 1)}
              className="chip rounded-full bg-white/12 px-3.5 py-1.5 text-[13px] font-medium text-white/90 hover:bg-white/20"
            >
              Back
            </button>
          )}
          <button
            type="button"
            onClick={() => (last ? finish(true) : go(step + 1))}
            className="chip rounded-full bg-white px-3.5 py-1.5 text-[13px] font-medium text-black hover:bg-white/90"
          >
            {last ? "Start" : "Next"}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Sidekick talks the step through (each word shown as it is heard), then
 * the step's content appears. */
function Spoken({ step, children }: { step: number; children: ReactNode }) {
  const [spoken, setSpoken] = useState(false);
  return (
    <div className="flex flex-col gap-3">
      <SpokenLine step={step} onDone={() => setSpoken(true)} />
      <AnimatePresence initial={false}>
        {spoken && (
          <motion.div
            initial={{ opacity: 0, y: 8, filter: "blur(4px)" }}
            animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
            transition={{ duration: 0.35, ease: [0.23, 1, 0.32, 1] }}
            className="flex flex-col gap-2.5 text-[13px]"
          >
            {children}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function Intro() {
  const hotkey = useSidekick((s) => s.settings.paletteHotkey);
  const voiceStatus = useSidekick((s) => s.voiceStatus);
  const downloading = voiceStatus?.downloading ?? false;
  const missing = (voiceStatus?.missingBytes ?? 0) > 0;
  return (
    <div className="flex flex-col gap-3 text-[13.5px] leading-relaxed text-white/90">
      {(downloading || missing) && (
        <p className="rounded-2xl bg-white/[0.06] px-3.5 py-2.5 text-[12.5px] text-[rgb(235_235_245/0.7)]">
          {downloading
            ? "Getting my voice ready (speech models download once, about 205 MB)…"
            : "Speech models will download so I can talk with you."}
        </p>
      )}
      <ul className="flex flex-col gap-2">
        <Point title="Summon">
          <Kbd>{hotkey}</Kbd> or say "Hey Sidekick".
        </Point>
        <Point title="Local">What I see stays on this PC.</Point>
        <Point title="Consent">I act only when you say. Anything I make can be undone.</Point>
      </ul>
    </div>
  );
}

interface Pick {
  id: string;
  label: string;
  /** What it is for, shown under the label. */
  hint?: string;
  on: boolean;
}

function picksFrom(f: SetupFound, off: Set<string>): Pick[] {
  const list: Omit<Pick, "on">[] = [];
  const repos = f.codeFolders.reduce((n, c) => n + c.repos, 0);
  if (f.codeFolders.length) {
    list.push({ id: "code", label: `Your code: ${f.codeFolders.map((c) => c.label).join(", ")} (${repos} repos)` });
  }
  if (f.searchFolders.length) {
    list.push({ id: "search", label: `Search ${f.searchFolders.map((c) => c.label).join(", ")}` });
  }
  if (f.chatModels.length) list.push({ id: "model", label: `Local AI: ${f.chatModels[0]}` });
  if (f.claudeInstalled && !f.claudeHooks) {
    list.push({ id: "hooks", label: "Claude Code: hear when it finishes or asks" });
  }
  if (f.claudeInstalled && !f.claudeMcp) {
    list.push({ id: "mcp", label: "Claude Code: let it use Sidekick's tools" });
  }
  for (const item of f.installable.filter((i) => i.recommended && !i.done && i.runnable)) {
    list.push({ id: `install:${item.id}`, label: `Install ${item.title}`, hint: item.why });
  }
  return list.map((p) => ({ ...p, on: !off.has(p.id) }));
}

/** Everything found on this PC, ticked, with one button to set it all up. */
function FoundCard({ onDone }: { onDone: () => void }) {
  const { data: found } = useCached<SetupFound>("setup-detect", api.setupDetect);
  // Unticked rows; everything else found is ticked. Kept apart from the data
  // so a refresh in the background never resets your choices.
  const [off, setOff] = useState<Set<string>>(() => new Set());
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const picks = useMemo(() => (found ? picksFrom(found, off) : []), [found, off]);

  if (!found || picks.length === 0) return null;

  const on = (id: string) => picks.some((p) => p.id === id && p.on);
  const apply = () => {
    setBusy(true);
    setError(null);
    void api
      .setupApply({
        codeFolders: on("code") ? found.codeFolders.map((f) => f.path) : [],
        searchFolders: on("search") ? found.searchFolders.map((f) => f.path) : [],
        chatModel: on("model") ? (found.chatModels[0] ?? null) : null,
        claudeHooks: on("hooks"),
        claudeMcp: on("mcp"),
        install: picks.filter((p) => p.on && p.id.startsWith("install:")).map((p) => p.id.slice(8)),
        voice: false,
        launchAtLogin: useSidekick.getState().settings.launchAtLogin,
      })
      .then((d) => {
        setDone(d);
        setTimeout(onDone, 1200);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  };

  return (
    <div className="mt-3 flex flex-col gap-2 rounded-2xl bg-white/[0.06] p-3.5 text-[13px]">
      <p className="font-medium text-white">Here is what I found</p>
      <ul className="flex flex-col gap-1.5">
        {picks.map((p) => (
          <li key={p.id}>
            <label className="flex cursor-pointer items-start gap-2.5 text-[12.5px] text-[rgb(235_235_245/0.8)]">
              <input
                type="checkbox"
                checked={p.on}
                onChange={(e) =>
                  setOff((prev) => {
                    const next = new Set(prev);
                    if (e.target.checked) next.delete(p.id);
                    else next.add(p.id);
                    return next;
                  })
                }
                className="check mt-0.5 shrink-0"
              />
              <span className="min-w-0">
                {p.label}
                {p.hint && <span className="block text-[11.5px] text-[rgb(235_235_245/0.5)]">{p.hint}</span>}
              </span>
            </label>
          </li>
        ))}
      </ul>
      {done ? (
        <p className="text-[12px] text-[#30d158]">{done.length ? done.join(". ") : "All set"}.</p>
      ) : (
        <button
          type="button"
          disabled={busy || !picks.some((p) => p.on)}
          onClick={apply}
          className="chip self-start rounded-full bg-white px-3.5 py-1.5 text-[13px] font-medium text-black hover:bg-white/90 disabled:opacity-50"
        >
          {busy ? "Setting up..." : "Set it all up"}
        </button>
      )}
      {error && <p className="text-[12px] text-[#ff453a]">{error}</p>}
      <p className="text-[11.5px] text-[rgb(235_235_245/0.5)]">
        Claude Code&apos;s settings are backed up before anything is added.
      </p>
    </div>
  );
}

function Extras() {
  const settings = useSidekick((s) => s.settings);
  return (
    <div className="flex flex-col gap-2.5 text-[13px]">
      <Choice
        title="Talk to me"
        hint='Say "Hey Sidekick". Downloads about 205 MB of speech models once; all on this PC.'
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
        Everything here, and the setup checklist, stays in Settings. Come back to Setup anytime to see what is left.
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
      className={`chip flex items-center gap-3 rounded-2xl px-3.5 py-2.5 text-left ring-1 ring-inset ${
        on ? "bg-white/10 ring-[#0a84ff]" : "bg-white/6 ring-transparent hover:bg-white/9"
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
    <li className="rounded-2xl bg-white/6 px-3.5 py-2.5">
      <p className="font-medium text-white">{title}</p>
      <p className="text-[12.5px] text-[rgb(235_235_245/0.6)]">{children}</p>
    </li>
  );
}

function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="rounded-md bg-white/15 px-1.5 py-0.5 font-sans text-[11.5px] text-white">{children}</kbd>;
}
