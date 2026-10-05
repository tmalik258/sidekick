"use client";

// First run: short steps inside the island. What Sidekick
// does and what stays private, then a live checklist of the AI, connections
// and tools to set up, each with the exact command to run, and a few extras.
// Finishing on Start marks onboarding done; the same checklist stays in
// Settings > Setup. The current step is saved in settings so a restart
// resumes where the user left off.

import { AnimatePresence, motion } from "motion/react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { prefetch, revalidate, SETUP_STATUS_CACHE_KEY } from "@/lib/cache";
import { useScrollEdge } from "@/lib/hooks";
import { PANEL_MAX_HEIGHT } from "@/lib/islandSize";
import { updateSettings, useSidekick } from "@/lib/store";
import { ASK_ORB } from "./AskPanel";
import { SetupChecklist } from "./SetupChecklist";
import { SpokenLine, wasHeard } from "./SpokenLine";
import { Extras } from "./welcome/Extras";
import { FoundCard } from "./welcome/FoundCard";
import { WelcomeIntro } from "./welcome/WelcomeIntro";

const STEPS = ["Welcome", "Your AI", "Connect", "Tools", "Extras"] as const;
/** Welcome is info-only — advance on its own once the line has finished. */
const WELCOME_AUTO_SEC = 5;
/** No activity away from the panel: park so it stops blocking the screen. */
const IDLE_HIDE_MS = 8_000;

function clampStep(n: number): number {
  if (!Number.isFinite(n) || n < 0) return 0;
  return Math.min(Math.trunc(n), STEPS.length - 1);
}

export function IslandWelcome() {
  const saved = useSidekick((s) => s.settings.welcomeStep);
  const [step, setStep] = useState(() => clampStep(saved));
  const [autoIn, setAutoIn] = useState<number | null>(null);
  /** Bumped on any real user input so the idle hide clock restarts. */
  const [idleGen, setIdleGen] = useState(0);
  /** Welcome line finished (or was already heard); idle hide waits for this. */
  const [lineReady, setLineReady] = useState(() => wasHeard(0));
  /** Pointer over the panel — reading counts as activity, not idle. */
  const [over, setOver] = useState(false);
  const edges = useScrollEdge();
  const root = useRef<HTMLDivElement>(null);

  const poke = () => setIdleGen((n) => n + 1);

  const go = (next: number) => {
    const n = clampStep(next);
    setStep(n);
    void updateSettings({ welcomeStep: n });
    // Sidekick talks each step through once; a step already heard is shown
    // whole and quiet. Either way the line being said is cut off.
    if (wasHeard(n)) void api.voiceStop();
    else void api.voiceWelcomeStep(n);
  };
  const finish = () => {
    void api.voiceStop().then(() => void api.voiceSay("That's everything. You're all set."));
    // Onboarded must stick before close; otherwise Rust keeps welcome locked.
    void updateSettings({ onboarded: true, welcomeStep: step }).then(() => api.askClose());
  };
  const last = step === STEPS.length - 1;

  useEffect(() => {
    setStep(clampStep(saved));
  }, [saved]);

  // Warm setup caches while the user hears this step (and the next step's UI
  // is not on screen yet).
  useEffect(() => {
    if (step >= 2) revalidate(SETUP_STATUS_CACHE_KEY, api.setupStatus);
    else prefetch(SETUP_STATUS_CACHE_KEY, api.setupStatus);
    if (step < 2) prefetch("setup-detect", api.setupDetect);
  }, [step]);

  // Leaving Welcome clears the countdown; Spoken starts it again when the
  // line finishes so we never cut the welcome speech short.
  useEffect(() => {
    if (step !== 0) setAutoIn(null);
    setLineReady(wasHeard(step));
  }, [step]);

  // Pointer activity resets idle hide without making the shell a control.
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    const enter = () => setOver(true);
    const leave = () => setOver(false);
    const activity = () => setIdleGen((n) => n + 1);
    el.addEventListener("pointerenter", enter);
    el.addEventListener("pointerleave", leave);
    el.addEventListener("pointerdown", activity);
    el.addEventListener("wheel", activity, { passive: true });
    return () => {
      el.removeEventListener("pointerenter", enter);
      el.removeEventListener("pointerleave", leave);
      el.removeEventListener("pointerdown", activity);
      el.removeEventListener("wheel", activity);
    };
  }, []);

  // go closes over the latest setters; only the countdown clock should restart.
  // biome-ignore lint/correctness/useExhaustiveDependencies: step and autoIn only
  useEffect(() => {
    if (step !== 0 || autoIn === null || autoIn <= 0) return;
    const id = setTimeout(() => {
      if (autoIn <= 1) go(1);
      else setAutoIn(autoIn - 1);
    }, 1000);
    return () => clearTimeout(id);
  }, [step, autoIn]);

  // Talking, auto-advancing, or the user is still on the panel (reading /
  // scrolling). Waiting already parks welcome via startWaiting.
  const busy = !lineReady || autoIn !== null || over;
  // idleGen and step restart the clock after activity or a step change.
  // biome-ignore lint/correctness/useExhaustiveDependencies: busy plus restart triggers
  useEffect(() => {
    if (busy) return;
    const id = setTimeout(() => void api.askDeferWelcome(), IDLE_HIDE_MS);
    return () => clearTimeout(id);
  }, [busy, idleGen, step]);

  return (
    <div ref={root} className="flex flex-col" style={{ maxHeight: PANEL_MAX_HEIGHT }}>
      <div className="mb-3 flex h-7.5 shrink-0 items-center gap-2" style={{ paddingLeft: ASK_ORB + 10 }}>
        <h1 className="flex-1 font-display text-[17px] font-semibold tracking-[-0.015em]">{STEPS[step]}</h1>
        <ol className="flex gap-1" aria-label={`Step ${step + 1} of ${STEPS.length}`}>
          {STEPS.map((s, i) => (
            <li
              key={s}
              aria-label={s}
              aria-current={i === step ? "step" : undefined}
              className={`h-1.5 rounded-full transition-[width,background-color] duration-200 ease-out ${
                i === step ? "w-4 bg-white" : i < step ? "w-1.5 bg-white/55" : "w-1.5 bg-white/20"
              }`}
            />
          ))}
        </ol>
      </div>

      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div
          key={step}
          ref={edges}
          initial={{ opacity: 0, x: 12, filter: "blur(4px)" }}
          animate={{ opacity: 1, x: 0, filter: "blur(0px)" }}
          exit={{ opacity: 0, x: -12, filter: "blur(4px)", transition: { duration: 0.12 } }}
          transition={{ duration: 0.26, ease: [0.23, 1, 0.32, 1] }}
          className="settings-scroll -mr-3 min-h-0 flex-auto overflow-y-auto pr-3 pl-0.5"
          onScroll={poke}
        >
          <Spoken
            step={step}
            onReady={() => {
              setLineReady(true);
              if (step === 0) setAutoIn(WELCOME_AUTO_SEC);
            }}
          >
            {step === 0 && <WelcomeIntro />}
            {step === 1 && (
              <>
                <FoundCard onDone={() => undefined} />
                <SetupChecklist groups={["ai"]} inlineGuides />
              </>
            )}
            {step === 2 && <SetupChecklist groups={["connect"]} inlineGuides />}
            {step === 3 && <SetupChecklist groups={["tools"]} inlineGuides />}
            {step === 4 && <Extras />}
          </Spoken>
        </motion.div>
      </AnimatePresence>

      {lineReady && (
        <div className="mt-4 flex shrink-0 items-center justify-between pb-1">
          <FooterLink onClick={() => void api.askDeferWelcome()}>Hide</FooterLink>
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
              onClick={() => (last ? finish() : go(step + 1))}
              className="chip rounded-full bg-white px-3.5 py-1.5 text-[13px] font-medium text-black hover:bg-white/90"
            >
              {last ? "Start" : autoIn !== null ? `Next · ${autoIn}` : "Next"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function FooterLink({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="chip rounded-full px-2.5 py-1 text-[12.5px] text-[rgb(235_235_245/0.6)] hover:bg-white/[0.06] hover:text-white"
    >
      {children}
    </button>
  );
}

/** Sidekick talks the step through (each word shown as it is heard), then
 * the step's content appears. Step content mounts hidden during speech so
 * setup detect/status can load before the reveal. */
function Spoken({ step, children, onReady }: { step: number; children: ReactNode; onReady?: () => void }) {
  const [revealed, setRevealed] = useState(() => wasHeard(step));
  const onReadyRef = useRef(onReady);
  onReadyRef.current = onReady;

  useEffect(() => {
    if (wasHeard(step)) {
      setRevealed(true);
      onReadyRef.current?.();
    } else {
      setRevealed(false);
    }
  }, [step]);

  const show = () => {
    setRevealed(true);
    onReadyRef.current?.();
  };

  return (
    <div className="flex flex-col gap-3">
      <SpokenLine step={step} onDone={show} />
      <motion.div
        initial={false}
        animate={revealed ? { opacity: 1, y: 0, filter: "blur(0px)" } : { opacity: 0, y: 8, filter: "blur(4px)" }}
        transition={{ duration: 0.35, ease: [0.23, 1, 0.32, 1] }}
        className={`flex flex-col gap-2.5 text-[13px] ${revealed ? "" : "hidden"}`}
        aria-hidden={!revealed}
      >
        {children}
      </motion.div>
    </div>
  );
}
