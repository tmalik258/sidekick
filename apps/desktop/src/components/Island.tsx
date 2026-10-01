"use client";

// The island: a black capsule that morphs like Apple's Dynamic Island. One
// Orb instance lives inside and travels between the compact and expanded
// layouts, so its gaze and blink never reset. The shell size is spring driven
// and interruptible; content cross-fades through a short blur.

import { AnimatePresence, animate, motion, useMotionValue, useReducedMotion } from "motion/react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import { connect, setHovered, useSidekick } from "@/lib/store";
import { isPaused, type MascotState, type Suggestion } from "@/lib/types";
import { Icon } from "./Icon";
import { Orb } from "./Orb";

const TITLE: Record<MascotState, string> = {
  idle: "Sidekick",
  sleeping: "Resting",
  noticing: "Something changed",
  suggesting: "Suggestion",
  listening: "Listening",
  working: "Working on it",
  success: "Done",
  error: "That did not work",
};

const DETAIL: Record<MascotState, string> = {
  idle: "Watching for moments to help.",
  sleeping: "Sensors are paused.",
  noticing: "Taking a look.",
  suggesting: "Here is an idea.",
  listening: "Go ahead.",
  working: "This takes a moment.",
  success: "All set.",
  error: "Details are in the log.",
};

/** States that hold the island open without hover. */
const OPEN_STATES: ReadonlySet<MascotState> = new Set(["suggesting", "listening", "working", "success", "error"]);

const ORB = 44;
const COMPACT = { width: 124, height: 36, radius: 18, orb: 26 };
const EXPANDED = { width: 388, minHeight: 78, radius: 30, pad: 16 };
const TOP = 6;

/** Delay before hover expands, so a cursor passing over the top edge does not trigger it. */
const HOVER_IN_MS = 140;
/** Grace period before collapsing after the cursor leaves. */
const HOVER_OUT_MS = 320;

const morphOpen = { type: "spring", bounce: 0.3, duration: 0.55 } as const;
const morphClose = { type: "spring", bounce: 0.12, duration: 0.42 } as const;

export function Island() {
  const { mascot, settings, suggestion, hovered: rawHover, ready } = useSidekick();
  const reduced = useReducedMotion() ?? false;
  const now = useNow(15_000);
  const paused = isPaused(settings.pause, now);
  const hovered = useIntent(rawHover);
  const expanded = hovered || OPEN_STATES.has(mascot) || !!suggestion;

  const [contentHeight, setContentHeight] = useState(0);
  const bump = useMotionValue(1);

  useEffect(() => connect({ sounds: true }), []);

  // Measure expanded content so the capsule grows exactly to fit it. A
  // callback ref, because the content node mounts and unmounts with expansion.
  const observer = useRef<ResizeObserver | null>(null);
  const contentRef = useCallback((el: HTMLDivElement | null) => {
    observer.current?.disconnect();
    if (!el) return;
    const measure = () => setContentHeight(el.offsetHeight);
    measure();
    observer.current = new ResizeObserver(measure);
    observer.current.observe(el);
  }, []);

  const width = expanded ? EXPANDED.width : COMPACT.width;
  // The measured content box already includes the top padding.
  const height = expanded ? Math.max(EXPANDED.minHeight, contentHeight + EXPANDED.pad) : COMPACT.height;
  const radius = expanded ? EXPANDED.radius : COMPACT.radius;
  const transition = reduced ? { duration: 0 } : expanded ? morphOpen : morphClose;

  // Report the target shape as the interactive area; outside it the window
  // stays click-through. Sent once per change, not per animation frame.
  useEffect(() => {
    void api.islandSetHitRect({ x: (window.innerWidth - width) / 2, y: TOP, width, height });
  }, [width, height]);

  // A small squish when something gets the mascot's attention while compact.
  useEffect(() => {
    if (mascot !== "noticing" || reduced) return;
    void animate(bump, [1, 1.06, 1], { duration: 0.42, ease: [0.23, 1, 0.32, 1] });
  }, [mascot, reduced, bump]);

  useSuggestionKeys(suggestion);
  useSuggestionTimeout(suggestion, rawHover, settings.collapseAfterSecs);

  if (!ready) return null;

  // The orb scales from its top-left corner, so these are its visual corner.
  const orbScale = expanded ? 1 : COMPACT.orb / ORB;
  const orbX = expanded ? EXPANDED.pad : (COMPACT.height - COMPACT.orb) / 2 + 1;
  const orbY = expanded ? EXPANDED.pad : (COMPACT.height - COMPACT.orb) / 2;

  return (
    <div className="flex h-screen w-screen justify-center select-none" style={{ paddingTop: TOP }}>
      <motion.div
        className="island-shell relative overflow-hidden text-white"
        initial={false}
        animate={{ width, height, borderRadius: radius }}
        transition={transition}
        style={{ scaleX: bump, originY: 0 }}
        onPointerEnter={() => setHovered(true)}
        onPointerLeave={() => setHovered(false)}
      >
        <motion.div
          className="absolute top-0 left-0"
          initial={false}
          animate={{ x: orbX, y: orbY, scale: orbScale }}
          transition={transition}
          style={{ originX: 0, originY: 0 }}
        >
          <Orb state={mascot} size={ORB} />
        </motion.div>

        <AnimatePresence initial={false}>
          {!expanded && <CompactTrailing key="compact" mascot={mascot} paused={paused} />}
        </AnimatePresence>

        <AnimatePresence initial={false} mode="popLayout">
          {expanded && (
            <motion.div
              key="expanded"
              ref={contentRef}
              className="absolute top-0 right-0"
              style={{ left: EXPANDED.pad + ORB + 14, paddingTop: EXPANDED.pad, paddingRight: EXPANDED.pad }}
              initial={reduced ? { opacity: 0 } : { opacity: 0, filter: "blur(6px)", y: 4 }}
              animate={{ opacity: 1, filter: "blur(0px)", y: 0 }}
              exit={{ opacity: 0, filter: "blur(4px)", transition: { duration: 0.1 } }}
              transition={{ duration: 0.28, delay: 0.06, ease: [0.23, 1, 0.32, 1] }}
            >
              <ExpandedContent mascot={mascot} paused={paused} suggestion={suggestion} />
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    </div>
  );
}

function CompactTrailing({ mascot, paused }: { mascot: MascotState; paused: boolean }) {
  const busy = mascot === "noticing" || mascot === "working" || mascot === "listening";
  return (
    <motion.div
      className="absolute top-0 right-0 flex h-9 items-center pr-3.5"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: { delay: 0.12, duration: 0.2 } }}
      exit={{ opacity: 0, transition: { duration: 0.08 } }}
    >
      {busy ? (
        <Activity />
      ) : (
        <span
          className={`size-1.5 rounded-full ${paused ? "bg-[#ffd60a]" : "bg-[#30d158]"}`}
          style={{ boxShadow: `0 0 8px ${paused ? "#ffd60a" : "#30d158"}` }}
          title={paused ? "Paused" : "Watching"}
        />
      )}
    </motion.div>
  );
}

/** Three bars that breathe while the mascot is busy. */
function Activity() {
  return (
    <span className="flex h-3 items-center gap-[3px]" role="img" aria-label="Busy">
      {[0, 1, 2].map((i) => (
        <motion.span
          key={i}
          className="w-[3px] rounded-full bg-white/85"
          animate={{ height: [4, 12, 4] }}
          transition={{ duration: 0.9, repeat: Number.POSITIVE_INFINITY, delay: i * 0.15, ease: "easeInOut" }}
        />
      ))}
    </span>
  );
}

function ExpandedContent({
  mascot,
  paused,
  suggestion,
}: {
  mascot: MascotState;
  paused: boolean;
  suggestion: Suggestion | null;
}) {
  return (
    <div className="flex flex-col">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate font-display text-[15px] leading-5 font-semibold tracking-[-0.015em] text-white">
            {suggestion?.title ?? TITLE[mascot]}
          </p>
          <p className="mt-0.5 line-clamp-2 text-[13px] leading-[18px] tracking-[-0.005em] text-[rgb(235_235_245/0.6)]">
            {suggestion?.detail ?? (paused && mascot !== "sleeping" ? "Sensors are paused." : DETAIL[mascot])}
          </p>
        </div>
        {!suggestion && <QuickActions paused={paused} />}
      </div>

      {suggestion && <Options suggestion={suggestion} />}
    </div>
  );
}

function QuickActions({ paused }: { paused: boolean }) {
  return (
    <div className="flex shrink-0 gap-1.5">
      <RoundButton
        label={paused ? "Resume" : "Pause 15 minutes"}
        onClick={() => void (paused ? api.sensorsResume() : api.sensorsPause(15))}
      >
        <Icon name={paused ? "play" : "pause"} size={14} />
      </RoundButton>
      <RoundButton label="Settings" onClick={() => void api.openSettings()}>
        <Icon name="settings" size={15} />
      </RoundButton>
    </div>
  );
}

function RoundButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className="chip grid size-8 place-items-center rounded-full bg-white/[0.12] text-white/90 hover:bg-white/[0.2] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#0a84ff]"
    >
      {children}
    </button>
  );
}

function Options({ suggestion }: { suggestion: Suggestion }) {
  return (
    <div className="mt-3 flex flex-wrap items-center gap-1.5">
      {suggestion.options.map((option, i) => (
        <motion.button
          key={option}
          type="button"
          onClick={() => void api.suggestionChoose(suggestion.id, i)}
          initial={{ opacity: 0, y: 6, filter: "blur(4px)" }}
          animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
          transition={{ duration: 0.26, delay: 0.12 + i * 0.04, ease: [0.23, 1, 0.32, 1] }}
          className={`chip flex h-8 items-center gap-2 rounded-full pr-3.5 pl-3 text-[13px] font-medium tracking-[-0.01em] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#0a84ff] ${
            i === 0 ? "bg-white text-black hover:bg-white/90" : "bg-white/[0.12] text-white hover:bg-white/[0.2]"
          }`}
        >
          {option}
          <kbd className={`font-sans text-[11px] ${i === 0 ? "text-black/40" : "text-white/35"}`}>{i + 1}</kbd>
        </motion.button>
      ))}
      <motion.button
        type="button"
        onClick={() => void api.suggestionDismiss(suggestion.id, "user")}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.2, delay: 0.12 + suggestion.options.length * 0.04 }}
        className="chip ml-0.5 h-8 rounded-full px-2.5 text-[13px] text-[rgb(235_235_245/0.6)] hover:text-white"
      >
        Not now
      </motion.button>
    </div>
  );
}

/** Hover with intent: a short delay in, a grace period out. */
function useIntent(raw: boolean): boolean {
  const [value, setValue] = useState(false);
  useEffect(() => {
    const id = setTimeout(() => setValue(raw), raw ? HOVER_IN_MS : HOVER_OUT_MS);
    return () => clearTimeout(id);
  }, [raw]);
  return value;
}

/** Number keys pick an option, Esc dismisses (when the island has focus). */
function useSuggestionKeys(suggestion: Suggestion | null) {
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
}

/** An unattended suggestion dismisses itself after the configured time (FR-UI-02). */
function useSuggestionTimeout(suggestion: Suggestion | null, hovered: boolean, seconds: number) {
  useEffect(() => {
    if (!suggestion || hovered) return;
    const id = setTimeout(() => void api.suggestionDismiss(suggestion.id, "timeout"), seconds * 1000);
    return () => clearTimeout(id);
  }, [suggestion, hovered, seconds]);
}
