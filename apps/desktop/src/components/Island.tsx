"use client";

// The island: a black capsule that morphs like Apple's Dynamic Island. One
// Orb instance lives inside and travels between the compact and expanded
// layouts, so its gaze and blink never reset. The shell size is spring driven
// and interruptible; content cross-fades through a short blur.

import { AnimatePresence, animate, motion, useMotionValue, useReducedMotion } from "motion/react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import { playSound } from "@/lib/sound";
import { connect, setHovered, uiVolume, useSidekick, watchWaiting } from "@/lib/store";
import { isPaused, type LaterItem, type MascotState, type Suggestion } from "@/lib/types";
import { ASK_ORB, AskPanel } from "./AskPanel";
import { Icon } from "./Icon";
import { IslandSettings } from "./IslandSettings";
import { IslandWelcome } from "./IslandWelcome";
import { Orb } from "./Orb";
import { PreparingVoice } from "./PreparingVoice";

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
const COMPACT = { width: 39, busyWidth: 109, waitWidth: 248, height: 36, radius: 18, orb: 26 };
const EXPANDED = { width: 388, minHeight: 78, radius: 30, pad: 16 };
/** Ask mode: wider, so commands and answers have room. */
const ASK_WIDTH = 560;
const TOP = 6;

/** Delay before hover expands, so a cursor passing over the top edge does not trigger it. */
const HOVER_IN_MS = 140;
/** Grace period before collapsing after the cursor leaves. */
const HOVER_OUT_MS = 320;

const morphOpen = { type: "spring", bounce: 0.3, duration: 0.55 } as const;
const morphClose = { type: "spring", bounce: 0.12, duration: 0.42 } as const;

export function Island() {
  const { mascot, settings, suggestion, hovered: rawHover, visible, ready } = useSidekick();
  const asking = useSidekick((s) => s.ask !== null);
  const view = useSidekick((s) => s.ask?.view);
  const chatting = useSidekick((s) => s.chatId !== null);
  const voiceStatus = useSidekick((s) => s.voiceStatus);
  const waiting = useSidekick((s) => (s.ask ? null : s.waiting));
  const reduced = useReducedMotion() ?? false;
  const now = useNow(15_000);
  const paused = isPaused(settings.pause, now);
  // Closing a panel (Hide, Esc, a click elsewhere) goes straight to the
  // small orb: the hovered card stays off until the cursor has left the
  // island once. Adjusted during render, so no in-between frame is drawn.
  const [prevAsking, setPrevAsking] = useState(asking);
  const [quiet, setQuiet] = useState(false);
  if (prevAsking !== asking) {
    setPrevAsking(asking);
    if (!asking && rawHover) setQuiet(true);
  }
  if (quiet && !rawHover) setQuiet(false);
  const intent = useIntent(rawHover);
  // Until onboarding is done, hovering only brings the welcome back; the
  // idle card ("watching for moments") would just flash on the way.
  const hovered = intent && !quiet && settings.onboarded;
  const preparingVoice =
    !settings.onboarded && !asking && !(voiceStatus?.models.some((m) => m.id === "voice" && m.installed) ?? false);
  const expanded = asking || preparingVoice || hovered || OPEN_STATES.has(mascot) || !!suggestion;
  // At rest only the sphere shows. The shell keeps its size (so hover and the
  // orb position do not move) but loses its background.
  const bare = !expanded && !chatting && !waiting && (mascot === "idle" || mascot === "sleeping");
  const busy = chatting || preparingVoice || mascot === "noticing" || mascot === "working" || mascot === "listening";

  const [contentHeight, setContentHeight] = useState(0);
  const bump = useMotionValue(1);

  useEffect(() => connect({ sounds: true }), []);
  useEffect(() => watchWaiting(), []);

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

  const width = asking
    ? ASK_WIDTH
    : expanded
      ? EXPANDED.width
      : waiting
        ? COMPACT.waitWidth
        : busy
          ? COMPACT.busyWidth
          : COMPACT.width;
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

  // Number keys must not pick a suggestion while the user is typing.
  useSuggestionKeys(asking ? null : suggestion);

  if (!ready) return null;

  // The orb scales from its top-left corner, so these are its visual corner.
  const orbScale = asking ? ASK_ORB / ORB : expanded ? 1 : COMPACT.orb / ORB;
  const orbX = expanded ? EXPANDED.pad : (COMPACT.height - COMPACT.orb) / 2 + 1;
  const orbY = expanded ? EXPANDED.pad : (COMPACT.height - COMPACT.orb) / 2;

  return (
    <motion.div
      className="flex h-screen w-screen justify-center select-none"
      style={{ paddingTop: TOP, originY: 0 }}
      initial={false}
      animate={
        visible ? { opacity: 1, scale: 1, filter: "blur(0px)" } : { opacity: 0, scale: 0.92, filter: "blur(4px)" }
      }
      transition={reduced ? { duration: 0 } : { duration: visible ? 0.32 : 0.2, ease: [0.23, 1, 0.32, 1] }}
    >
      <motion.div
        className="island-shell relative overflow-hidden text-white"
        data-bare={bare}
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
          <Orb state={chatting && mascot === "idle" ? "working" : mascot} size={ORB} theme={settings.theme} />
        </motion.div>

        <AnimatePresence initial={false}>
          {!expanded && !bare && (
            <CompactTrailing key="compact" busy={busy} paused={paused} waiting={waiting?.label ?? null} />
          )}
        </AnimatePresence>

        <AnimatePresence initial={false} mode="popLayout">
          {asking ? (
            <motion.div
              key="ask"
              ref={contentRef}
              className="absolute top-0 right-0"
              style={{ left: EXPANDED.pad, paddingTop: EXPANDED.pad, paddingRight: EXPANDED.pad }}
              initial={reduced ? { opacity: 0 } : { opacity: 0, filter: "blur(6px)", y: 4 }}
              animate={{ opacity: 1, filter: "blur(0px)", y: 0 }}
              exit={{ opacity: 0, filter: "blur(4px)", transition: { duration: 0.1 } }}
              transition={{ duration: 0.28, delay: 0.06, ease: [0.23, 1, 0.32, 1] }}
            >
              {view === "settings" ? <IslandSettings /> : view === "welcome" ? <IslandWelcome /> : <AskPanel />}
            </motion.div>
          ) : preparingVoice ? (
            <motion.div
              key="preparing"
              ref={contentRef}
              className="absolute top-0 right-0"
              style={{ left: EXPANDED.pad + ORB + 14, paddingTop: EXPANDED.pad, paddingRight: EXPANDED.pad }}
              initial={reduced ? { opacity: 0 } : { opacity: 0, filter: "blur(6px)", y: 4 }}
              animate={{ opacity: 1, filter: "blur(0px)", y: 0 }}
              exit={{ opacity: 0, filter: "blur(4px)", transition: { duration: 0.1 } }}
              transition={{ duration: 0.28, delay: 0.06, ease: [0.23, 1, 0.32, 1] }}
            >
              <PreparingVoice voiceStatus={voiceStatus} />
            </motion.div>
          ) : (
            expanded && (
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
            )
          )}
        </AnimatePresence>
      </motion.div>
    </motion.div>
  );
}

function CompactTrailing({ paused, busy, waiting }: { paused: boolean; busy: boolean; waiting: string | null }) {
  const later = useSidekick((s) => s.later);
  if (waiting) {
    return (
      <motion.div
        className="absolute top-0 right-0 flex h-9 items-center gap-2.5 pr-3.5"
        style={{ left: COMPACT.height + 4 }}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1, transition: { delay: 0.12, duration: 0.2 } }}
        exit={{ opacity: 0, transition: { duration: 0.08 } }}
      >
        <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium text-white/85">Waiting for {waiting}</span>
        <Activity />
      </motion.div>
    );
  }
  return (
    <motion.div
      className="absolute top-0 right-0 flex h-9 items-center pr-3.5"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: { delay: 0.12, duration: 0.2 } }}
      exit={{ opacity: 0, transition: { duration: 0.08 } }}
    >
      {busy ? (
        <Activity />
      ) : paused ? (
        <span className="size-1.5 rounded-full bg-[#ffd60a]" style={{ boxShadow: "0 0 8px #ffd60a" }} title="Paused" />
      ) : (
        later > 0 && (
          <span
            className="grid h-4 min-w-4 place-items-center rounded-full bg-[#0a84ff] px-1 text-[10px] leading-none font-semibold text-white"
            title={`${later} waiting for you`}
          >
            {later}
          </span>
        )
      )}
    </motion.div>
  );
}

/** Three bars that breathe while the mascot is busy. */
function Activity() {
  return (
    <span className="flex h-3 items-center gap-0.75" role="img" aria-label="Busy">
      {[0, 1, 2].map((i) => (
        <motion.span
          key={i}
          className="w-0.75 rounded-full bg-white/85"
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
  const result = useSidekick((s) => s.lastResult);
  const reporting = (mascot === "success" || mascot === "error" || mascot === "working") && !suggestion;
  const detail =
    suggestion?.detail ??
    (reporting && result ? result.message : paused && mascot !== "sleeping" ? "Sensors are paused." : DETAIL[mascot]);
  return (
    <div className="flex flex-col">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate font-display text-[15px] leading-5 font-semibold tracking-[-0.015em] text-white">
            {suggestion?.title ?? TITLE[mascot]}
          </p>
          <p className="mt-0.5 line-clamp-2 text-[13px] leading-4.5 tracking-[-0.005em] text-[rgb(235_235_245/0.6)]">
            {detail}
          </p>
        </div>
        {!suggestion && reporting && (result?.path || result?.undoId) ? (
          <div className="flex shrink-0 gap-1.5">
            {result.undoId != null && <UndoButton id={result.undoId} />}
            {result.path && (
              <RoundButton label="Show in folder" onClick={() => void api.revealPath(result.path ?? "")}>
                <Icon name="folder" size={15} />
              </RoundButton>
            )}
          </div>
        ) : (
          !suggestion && <QuickActions paused={paused} />
        )}
      </div>

      {suggestion && <Options suggestion={suggestion} />}
      {!suggestion && !reporting && <LaterList />}
    </div>
  );
}

function UndoButton({ id }: { id: number }) {
  const undo = async () => {
    const result = useSidekick.getState().lastResult;
    try {
      const message = await api.actionUndo(id);
      useSidekick.setState({ lastResult: result && { ...result, message, path: null, undoId: null } });
    } catch (err) {
      useSidekick.setState({ lastResult: result && { ...result, ok: false, message: String(err), undoId: null } });
    }
  };
  return (
    <RoundButton label="Undo" onClick={() => void undo()}>
      <Icon name="undo" size={15} />
    </RoundButton>
  );
}

function QuickActions({ paused }: { paused: boolean }) {
  const hotkey = useSidekick((s) => s.settings.paletteHotkey);
  return (
    <div className="flex shrink-0 gap-1.5">
      <RoundButton label={`Ask Sidekick (${hotkey})`} onClick={() => void api.askOpen()}>
        <Icon name="ask" size={15} />
      </RoundButton>
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
      className="chip grid size-8 place-items-center rounded-full bg-white/12 text-white/90 hover:bg-white/20 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#0a84ff]"
    >
      {children}
    </button>
  );
}

function Options({ suggestion }: { suggestion: Suggestion }) {
  const alwaysAt = suggestion.always?.findIndex(Boolean) ?? -1;
  return (
    <div className="mt-3 flex flex-wrap items-center gap-1.5">
      {suggestion.options.map((option, i) => (
        <motion.button
          key={option}
          type="button"
          onClick={() => choose(suggestion, i)}
          initial={{ opacity: 0, y: 6, filter: "blur(4px)" }}
          animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
          transition={{ duration: 0.26, delay: 0.12 + i * 0.04, ease: [0.23, 1, 0.32, 1] }}
          className={`chip flex max-w-full min-h-8 items-center gap-2 rounded-full px-3 py-1.5 text-left text-[13px] font-medium tracking-[-0.01em] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#0a84ff] ${
            i === 0 ? "bg-white text-black hover:bg-white/90" : "bg-white/12 text-white hover:bg-white/20"
          }`}
        >
          <span className="max-w-60 leading-snug text-balance">{option}</span>
          <kbd
            className={`shrink-0 self-center font-sans text-[11px] leading-none ${
              i === 0 ? "text-black/40" : "text-white/35"
            }`}
          >
            Alt {i + 1}
          </kbd>
        </motion.button>
      ))}
      {alwaysAt >= 0 && (
        <motion.button
          type="button"
          onClick={() => always(suggestion, alwaysAt)}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.2, delay: 0.12 + suggestion.options.length * 0.04 }}
          title={`From now on, "${suggestion.options[alwaysAt]}" without asking. Undo in Settings > Skills.`}
          className="chip rounded-full px-2.5 py-1.5 text-[13px] text-[rgb(235_235_245/0.6)] hover:text-white"
        >
          Always do this
        </motion.button>
      )}
      <motion.button
        type="button"
        onClick={() => dismiss(suggestion)}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.2, delay: 0.12 + suggestion.options.length * 0.04 }}
        className="chip rounded-full px-2.5 py-1.5 text-[13px] text-[rgb(235_235_245/0.6)] hover:text-white"
      >
        Not now
        <kbd className="ml-1.5 font-sans text-[11px] text-white/35">Alt 0</kbd>
      </motion.button>
    </div>
  );
}

function choose(suggestion: Suggestion, index: number) {
  playSound("select", uiVolume(), useSidekick.getState().settings.soundKit);
  void api.suggestionChoose(suggestion.id, index);
}

function always(suggestion: Suggestion, index: number) {
  playSound("select", uiVolume(), useSidekick.getState().settings.soundKit);
  void api.suggestionAlways(suggestion.id, index);
}

/** Suggestions held while you were busy, opened one at a time. */
function LaterList() {
  const count = useSidekick((s) => s.later);
  const [items, setItems] = useState<LaterItem[]>([]);
  useEffect(() => {
    if (count > 0) void api.laterList().then(setItems);
    else setItems([]);
  }, [count]);
  if (items.length === 0) return null;
  return (
    <div className="mt-3 flex flex-col gap-1.5">
      <div className="flex items-center justify-between text-[12px] text-[rgb(235_235_245/0.6)]">
        <span>Saved for later</span>
        <button type="button" onClick={() => void api.laterClear()} className="chip hover:text-white">
          Clear
        </button>
      </div>
      {items.slice(0, 4).map((l) => (
        <button
          key={l.id}
          type="button"
          onClick={() => void api.laterOpen(l.id)}
          className="chip flex items-center justify-between gap-3 rounded-xl bg-white/[0.07] px-3 py-1.5 text-left hover:bg-white/[0.12]"
        >
          <span className="min-w-0">
            <span className="block truncate text-[13px] font-medium text-white">{l.title}</span>
            <span className="block truncate text-[12px] text-[rgb(235_235_245/0.55)]">{l.detail}</span>
          </span>
          <span className="shrink-0 text-[11px] text-white/40">{l.minutesAgo < 1 ? "now" : `${l.minutesAgo} min`}</span>
        </button>
      ))}
    </div>
  );
}

function dismiss(suggestion: Suggestion) {
  playSound("toggle_off", uiVolume(), useSidekick.getState().settings.soundKit);
  void api.suggestionDismiss(suggestion.id, "user");
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
      if (e.altKey) return; // Alt+N is a global shortcut handled in Rust
      if (e.key === "Escape") dismiss(suggestion);
      const n = Number(e.key);
      if (n >= 1 && n <= suggestion.options.length) choose(suggestion, n - 1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [suggestion]);
}
