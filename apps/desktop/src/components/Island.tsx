"use client";

import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useLayoutEffect, useRef } from "react";
import { api } from "@/lib/bridge";
import { useNow, usePrefersReducedMotion } from "@/lib/hooks";
import { connect, setHovered, useSidekick } from "@/lib/store";
import { isPaused, type MascotState } from "@/lib/types";
import { Mascot } from "./Mascot";

const STATUS: Record<MascotState, string> = {
  idle: "Sidekick",
  sleeping: "Resting",
  noticing: "Hm?",
  suggesting: "Suggestion",
  listening: "Listening",
  working: "Working",
  success: "Done",
  error: "Something went wrong",
};

const DETAIL: Record<MascotState, string> = {
  idle: "Watching for things to help with.",
  sleeping: "Taking a break.",
  noticing: "Something just happened.",
  suggesting: "Here is an idea.",
  listening: "Go ahead.",
  working: "On it.",
  success: "All done.",
  error: "That did not work. Details are in the log.",
};

/** States that keep the island open even without hover. */
const EXPANDED_STATES: ReadonlySet<MascotState> = new Set(["suggesting", "listening", "working", "success", "error"]);

const spring = { type: "spring", stiffness: 420, damping: 34, mass: 0.8 } as const;

export function Island() {
  const { mascot, settings, suggestion, hovered, ready } = useSidekick();
  const reducedMotion = usePrefersReducedMotion();
  const now = useNow(15_000);
  const shellRef = useRef<HTMLDivElement>(null);
  const paused = isPaused(settings.pause, now);
  const expanded = hovered || EXPANDED_STATES.has(mascot);

  useEffect(() => connect({ sounds: true }), []);

  // Report the interactive area so Rust can keep the rest click-through.
  // offset* ignores transforms, so the rect is the final layout box, not a
  // frame of the animation.
  const reportHitRect = useCallback(() => {
    const el = shellRef.current;
    if (!el) return;
    void api.islandSetHitRect({ x: el.offsetLeft, y: el.offsetTop, width: el.offsetWidth, height: el.offsetHeight });
  }, []);

  useLayoutEffect(() => {
    const el = shellRef.current;
    if (!el) return;
    reportHitRect();
    const observer = new ResizeObserver(reportHitRect);
    observer.observe(el);
    return () => observer.disconnect();
  }, [reportHitRect]);

  // Collapse an unattended suggestion after the configured time (FR-UI-02).
  useEffect(() => {
    if (!suggestion || hovered) return;
    const id = setTimeout(
      () => void api.suggestionDismiss(suggestion.id, "timeout"),
      settings.collapseAfterSecs * 1000,
    );
    return () => clearTimeout(id);
  }, [suggestion, hovered, settings.collapseAfterSecs]);

  // Number keys pick an option, Esc dismisses (when the island has focus).
  useEffect(() => {
    if (!suggestion) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void api.suggestionDismiss(suggestion.id, "user");
      const n = Number(e.key);
      if (n >= 1 && n <= suggestion.options.length) void api.suggestionChoose(suggestion.id, n - 1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [suggestion]);

  if (!ready) return null;

  return (
    <div className="flex h-screen w-screen justify-center pt-1.5 select-none">
      <motion.div
        ref={shellRef}
        layout
        transition={reducedMotion ? { duration: 0 } : spring}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        className="h-fit overflow-hidden bg-[rgb(12_12_16/0.94)] text-white shadow-[0_8px_30px_rgb(0_0_0/0.35)] ring-1 ring-white/10"
        style={{ borderRadius: expanded ? 22 : 999, width: expanded ? 400 : 196 }}
      >
        {expanded ? (
          <Expanded mascot={mascot} paused={paused} reducedMotion={reducedMotion} />
        ) : (
          <motion.div layout="position" className="flex h-9 items-center gap-2 pr-4 pl-1.5">
            <Mascot state={mascot} size={30} reducedMotion={reducedMotion} />
            <span className="truncate text-[13px] font-medium text-white/85">{STATUS[mascot]}</span>
            {paused && <PausedDot />}
          </motion.div>
        )}
      </motion.div>
    </div>
  );
}

function Expanded({ mascot, paused, reducedMotion }: { mascot: MascotState; paused: boolean; reducedMotion: boolean }) {
  const suggestion = useSidekick((s) => s.suggestion);

  return (
    <motion.div layout="position" className="flex gap-3 p-3">
      <Mascot state={mascot} size={56} reducedMotion={reducedMotion} />
      <div className="min-w-0 flex-1 py-0.5">
        <div className="flex items-center gap-2">
          <p className="truncate text-[14px] font-semibold">{suggestion?.title ?? STATUS[mascot]}</p>
          {paused && <PausedDot label />}
        </div>
        <p className="mt-0.5 line-clamp-2 text-[12px] text-white/60">
          {suggestion?.detail ?? (paused ? "Sensors are paused." : DETAIL[mascot])}
        </p>

        <AnimatePresence initial={false}>
          {suggestion && (
            <motion.div
              key={suggestion.id}
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0 }}
              className="mt-2.5 flex flex-wrap gap-1.5"
            >
              {suggestion.options.map((option, i) => (
                <button
                  key={option}
                  type="button"
                  onClick={() => void api.suggestionChoose(suggestion.id, i)}
                  className="flex items-center gap-1.5 rounded-full bg-white/10 py-1 pr-3 pl-1 text-[12px] font-medium transition-colors hover:bg-white/20 focus-visible:outline-2 focus-visible:outline-sky-400"
                >
                  <kbd className="grid size-5 place-items-center rounded-full bg-white/15 text-[10px] text-white/80">
                    {i + 1}
                  </kbd>
                  {option}
                </button>
              ))}
              <button
                type="button"
                onClick={() => void api.suggestionDismiss(suggestion.id, "user")}
                className="rounded-full px-2.5 py-1 text-[12px] text-white/50 transition-colors hover:text-white/80"
              >
                Not now
              </button>
            </motion.div>
          )}
        </AnimatePresence>

        {!suggestion && (
          <div className="mt-2.5 flex gap-1.5">
            <button
              type="button"
              onClick={() => void api.openSettings()}
              className="rounded-full bg-white/10 px-3 py-1 text-[12px] font-medium transition-colors hover:bg-white/20"
            >
              Settings
            </button>
            {paused ? (
              <button
                type="button"
                onClick={() => void api.sensorsResume()}
                className="rounded-full bg-white/10 px-3 py-1 text-[12px] font-medium transition-colors hover:bg-white/20"
              >
                Resume
              </button>
            ) : (
              <button
                type="button"
                onClick={() => void api.sensorsPause(15)}
                className="rounded-full bg-white/10 px-3 py-1 text-[12px] font-medium transition-colors hover:bg-white/20"
              >
                Pause 15 min
              </button>
            )}
          </div>
        )}
      </div>
    </motion.div>
  );
}

function PausedDot({ label = false }: { label?: boolean }) {
  return (
    <span className="flex shrink-0 items-center gap-1 text-[11px] text-amber-300/90" title="Sensors paused">
      <span className="size-1.5 rounded-full bg-amber-300" />
      {label && "Paused"}
    </span>
  );
}
