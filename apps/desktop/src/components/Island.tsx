"use client";

// The island: a black capsule that morphs like Apple's Dynamic Island. One
// Orb instance lives inside and travels between the compact and expanded
// layouts, so its gaze and blink never reset. The shell size is spring driven
// and interruptible; content cross-fades through a short blur.

import { AnimatePresence, animate, motion, useMotionValue, useReducedMotion } from "motion/react";
import { type CSSProperties, useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import { ISLAND_TOP, PANEL_PAD } from "@/lib/islandSize";
import type { NetNotice } from "@/lib/net";
import { playSound } from "@/lib/sound";
import { connect, notePick, setHovered, uiVolume, useSidekick, watchWaiting } from "@/lib/store";
import { isPaused, type MascotState, type Suggestion } from "@/lib/types";
import { ASK_ORB, AskPanel, VoiceBars } from "./AskPanel";
import { Icon } from "./Icon";
import { Glance, RoundButton } from "./IslandGlance";
import { IslandGuide, useGuide } from "./IslandGuide";
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
  idle: "Nothing needs you right now.",
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
const COMPACT = {
  width: 39,
  busyWidth: 109,
  /** Orb + the offline mark. */
  offlineWidth: 66,
  waitWidth: 248,
  /** Orb + "Listening..." + bars; grows with transcript up to voiceWidth. */
  voiceMinWidth: 156,
  voiceWidth: 320,
  height: 36,
  radius: 18,
  orb: 26,
};
const EXPANDED = { width: 388, minHeight: 78, radius: 30, pad: PANEL_PAD };
/** Ask mode: wider, so commands and answers have room. */
const ASK_WIDTH = 560;
/** The welcome: a short read, so a narrower column than Ask. */
const WELCOME_WIDTH = 440;
/** A setup guide: room for its steps and copy buttons. */
const GUIDE_WIDTH = 452;
const TOP = ISLAND_TOP;

/** Delay before hover expands, so a cursor passing over the top edge does not trigger it. */
const HOVER_IN_MS = 140;
/** Grace period before collapsing after the cursor leaves. */
const HOVER_OUT_MS = 320;

/** Opening settles rather than wobbles: overshoot belongs after a fling,
 * not a hover. */
const morphOpen = { type: "spring", bounce: 0.15, duration: 0.45 } as const;
const morphClose = { type: "spring", bounce: 0.12, duration: 0.42 } as const;
/** Growing or shrinking while already open (an answer streaming in, New):
 * no bounce, so the panel never overshoots and settles back. */
const resize = { type: "spring", bounce: 0, duration: 0.34 } as const;

export function Island() {
  const { mascot, settings, suggestion, hovered: rawHover, visible, ready } = useSidekick();
  const asking = useSidekick((s) => s.ask !== null);
  const view = useSidekick((s) => s.ask?.view);
  const chatting = useSidekick((s) => s.chatId !== null);
  const voiceStatus = useSidekick((s) => s.voiceStatus);
  const online = useSidekick((s) => s.online);
  const netNotice = useSidekick((s) => s.netNotice);
  const waiting = useSidekick((s) => (s.ask ? null : s.waiting));
  const justDone = useSidekick((s) => s.justDone);
  // A task still running after Ask closed: its current step, small.
  const working = useSidekick((s) => {
    const last = s.turns[s.turns.length - 1];
    if (!s.chatId || !last?.streaming) return null;
    return last.tool ? `${last.tool}...` : "Working...";
  });
  const guide = useGuide(waiting);
  // Voice with Ask closed: a compact pill while listening and thinking; the
  // island opens only when the answer starts.
  const hearing = useSidekick((s) => (s.ask ? null : s.hearing));
  const voiceQuestion = useSidekick((s) => (s.ask ? null : s.voiceQuestion));
  const reduced = useReducedMotion() ?? false;
  const now = useNow(15_000);
  const paused = isPaused(settings.pause, now);
  // Closing a panel (Hide, Esc, Done) goes straight to the small orb. The
  // hover card stays off until the cursor leaves and comes back; otherwise
  // shrinking the hit rect clears quiet while useIntent still thinks we are
  // hovering, and "Watching for moments" flashes.
  const [prevAsking, setPrevAsking] = useState(asking);
  const [quiet, setQuiet] = useState(false);
  if (prevAsking !== asking) {
    setPrevAsking(asking);
    if (!asking) setQuiet(true);
  }
  // Cleared only once the cursor has been away as long as hover intent takes
  // to fall; clearing at once let a stale intent reopen a just-hidden welcome.
  useEffect(() => {
    if (!quiet || rawHover) return;
    const id = setTimeout(() => setQuiet(false), HOVER_OUT_MS);
    return () => clearTimeout(id);
  }, [quiet, rawHover]);
  const intent = useIntent(rawHover && !quiet);
  // Until onboarding is done, hover shows the steps being waited on, or else
  // brings the welcome back; the idle card ("watching for moments") would
  // just flash on the way.
  const guideOnHover = !!waiting && !waiting.background;
  const hovered = intent && !quiet && (settings.onboarded || guideOnHover);
  const resumeWelcome =
    intent && !quiet && !settings.onboarded && !asking && !guideOnHover && !justDone && mascot !== "success";
  useEffect(() => {
    if (resumeWelcome) void api.askResumeWelcome();
  }, [resumeWelcome]);
  const preparingVoice =
    !settings.onboarded && !asking && !(voiceStatus?.models.some((m) => m.id === "voice" && m.installed) ?? false);
  // A guide stays open while Sidekick waits on something you finish elsewhere.
  const guiding = !!waiting && !waiting.minimized && (waiting.steps?.length ?? 0) > 0;
  // Voice / in-flight Ask with Ask closed: Listening / Thinking / Working pill.
  // Stay non-bare so the hit rect stays usable (Idle alone would shrink to a
  // pinprick and lock out).
  const voiceBusy = hearing !== null || voiceQuestion !== null || mascot === "listening" || working !== null;
  const voicePill: { text: string; thinking: boolean; working?: boolean } | null = asking
    ? null
    : voiceQuestion !== null
      ? { text: voiceQuestion, thinking: true }
      : hearing !== null || mascot === "listening"
        ? { text: hearing ?? "", thinking: false }
        : working !== null
          ? { text: working, thinking: true, working: true }
          : null;
  // Suggestions stay normal during thinking — do not gate them on !voicePill.
  const expanded =
    asking ||
    preparingVoice ||
    (hovered && !voicePill) ||
    guiding ||
    (OPEN_STATES.has(mascot) && !voicePill) ||
    !!suggestion ||
    (!!netNotice && !voicePill);
  // At rest only the sphere shows. The shell keeps its size (so hover and the
  // orb position do not move) but loses its background.
  const bare =
    !expanded && !chatting && !waiting && !voiceBusy && online && (mascot === "idle" || mascot === "sleeping");
  const busy = chatting || preparingVoice || voiceBusy || mascot === "noticing" || mascot === "working";

  // Hover while Thinking or Working: open Ask so the island is usable, not a dead pill.
  useEffect(() => {
    if (!intent || quiet || asking || !settings.onboarded) return;
    if (voiceQuestion === null && working === null) return;
    void api.askOpen();
  }, [intent, quiet, asking, settings.onboarded, voiceQuestion, working]);

  const [contentHeight, setContentHeight] = useState(0);
  const bump = useMotionValue(1);
  // A mood (thanks, Claude finished) shows for a moment; offline and at
  // rest, the mascot looks a little lost.
  const mood = useSidekick((s) => s.mood);
  const speaking = useSidekick((s) => s.speaking);
  const face =
    mood?.id ??
    (speaking && mascot !== "listening"
      ? "speak"
      : !online && (mascot === "idle" || mascot === "sleeping")
        ? "offline"
        : null);
  const contentEl = useRef<HTMLDivElement | null>(null);

  useEffect(() => connect({ sounds: true }), []);
  // The window stays hidden until this page has drawn, so it never flashes
  // an empty black frame at startup. Two frames: the first one is painted.
  useEffect(() => {
    let raf = requestAnimationFrame(() => {
      raf = requestAnimationFrame(() => void api.islandReady().catch(() => {}));
    });
    return () => cancelAnimationFrame(raf);
  }, []);
  useEffect(() => watchWaiting(), []);

  // Measure expanded content so the capsule grows exactly to fit it. A
  // callback ref, because the content node mounts and unmounts with expansion.
  const observer = useRef<ResizeObserver | null>(null);
  const contentRef = useCallback((el: HTMLDivElement | null) => {
    // Keep the last height while the node is gone (AnimatePresence swaps);
    // zeroing here collapses Settings/Welcome mid-transition. The outgoing
    // panel's null comes after the new panel attached, so it must not
    // disconnect the new panel's observer.
    if (!el) return;
    observer.current?.disconnect();
    contentEl.current = el;
    const measure = () => setContentHeight(el.offsetHeight);
    measure();
    observer.current = new ResizeObserver(measure);
    observer.current.observe(el);
  }, []);

  // Remeasure after paint when Ask content swaps (New, stream, view change),
  // so a stale tall height does not stick after the chat clears.
  const turnsLen = useSidekick((s) => s.turns.length);
  const chatId = useSidekick((s) => s.chatId);
  // biome-ignore lint/correctness/useExhaustiveDependencies: view, turnsLen and chatId are triggers, not inputs
  useEffect(() => {
    if (!asking) return;
    let id2 = 0;
    const id1 = requestAnimationFrame(() => {
      id2 = requestAnimationFrame(() => {
        const el = contentEl.current;
        if (el) setContentHeight(el.offsetHeight);
      });
    });
    return () => {
      cancelAnimationFrame(id1);
      cancelAnimationFrame(id2);
    };
  }, [asking, view, turnsLen, chatId]);

  const showGuide = !!waiting && !waiting.background && !suggestion && (mascot === "idle" || mascot === "sleeping");
  const width = asking
    ? view === "welcome"
      ? WELCOME_WIDTH
      : ASK_WIDTH
    : expanded
      ? showGuide
        ? GUIDE_WIDTH
        : EXPANDED.width
      : voicePill
        ? voiceShellWidth(voicePill.text, voicePill.thinking, voicePill.working)
        : waiting
          ? COMPACT.waitWidth
          : busy
            ? COMPACT.busyWidth
            : !online
              ? COMPACT.offlineWidth
              : COMPACT.width;
  // The island window is already fixed (~560 tall); do not re-cap against
  // innerHeight or Settings/Welcome get clipped by the shell spring.
  const height = expanded ? Math.max(EXPANDED.minHeight, contentHeight + EXPANDED.pad) : COMPACT.height;
  const radius = expanded ? EXPANDED.radius : COMPACT.radius;
  // The bounce is for opening only; once open, size changes are calm.
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    if (!expanded) {
      setSettled(false);
      return;
    }
    const id = setTimeout(() => setSettled(true), 600);
    return () => clearTimeout(id);
  }, [expanded]);
  const transition = reduced ? { duration: 0 } : expanded ? (settled ? resize : morphOpen) : morphClose;

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
  // WebView chrome: Ctrl+J is Downloads — not a Sidekick feature.
  useBlockBrowserKeys();

  if (!ready) return null;

  // The orb scales from its top-left corner, so these are its visual corner.
  const orbScale = asking ? ASK_ORB / ORB : expanded ? 1 : COMPACT.orb / ORB;
  const orbX = expanded ? EXPANDED.pad : (COMPACT.height - COMPACT.orb) / 2 + 1;
  const orbY = expanded ? EXPANDED.pad : (COMPACT.height - COMPACT.orb) / 2;
  // Hidden for a fullscreen app, the island still comes back under the cursor.
  const shown = visible || rawHover;

  return (
    <motion.div
      className="flex h-screen w-screen justify-center select-none"
      style={{ paddingTop: TOP, originY: 0 }}
      initial={false}
      animate={shown ? { opacity: 1, scale: 1, filter: "blur(0px)" } : { opacity: 0, scale: 0.92, filter: "blur(4px)" }}
      transition={reduced ? { duration: 0 } : { duration: shown ? 0.32 : 0.2, ease: [0.23, 1, 0.32, 1] }}
    >
      <motion.div
        className="island-shell relative overflow-hidden text-white"
        data-bare={bare}
        data-color={settings.islandColor}
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
          <Orb
            state={chatting && mascot === "idle" ? "working" : mascot}
            face={chatting && mascot === "idle" ? null : face}
            size={ORB}
            theme={settings.theme}
            alive={settings.alive && visible}
          />
        </motion.div>

        <AnimatePresence initial={false}>
          {!expanded && voicePill && <VoicePill key="voice" {...voicePill} />}
          {!expanded && !bare && !voicePill && (
            <CompactTrailing
              key="compact"
              busy={busy}
              paused={paused}
              offline={!online}
              waiting={waiting?.label ?? null}
            />
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
                style={
                  {
                    left: EXPANDED.pad + ORB + 14,
                    paddingTop: EXPANDED.pad,
                    paddingRight: EXPANDED.pad,
                    // Lists below the orb (You missed) reach back under it.
                    "--orb-indent": `${ORB + 14}px`,
                  } as CSSProperties
                }
                initial={reduced ? { opacity: 0 } : { opacity: 0, filter: "blur(6px)", y: 4 }}
                animate={{ opacity: 1, filter: "blur(0px)", y: 0 }}
                exit={{ opacity: 0, filter: "blur(4px)", transition: { duration: 0.1 } }}
                transition={{ duration: 0.28, delay: 0.06, ease: [0.23, 1, 0.32, 1] }}
              >
                {showGuide && waiting ? (
                  <IslandGuide waiting={waiting} guide={guide} />
                ) : (
                  <ExpandedContent mascot={mascot} paused={paused} suggestion={suggestion} />
                )}
              </motion.div>
            )
          )}
        </AnimatePresence>
      </motion.div>
    </motion.div>
  );
}

/** Newest words matter most while talking; keep the pill from growing forever. */
function tail(text: string): string {
  return text.length > 38 ? `...${text.slice(-38).replace(/^\S*\s/, "")}` : text;
}

function voiceLabel(text: string, thinking: boolean, working?: boolean): string {
  if (working) return text;
  if (thinking) return `Thinking: ${text}`;
  return text.trim() ? tail(text) : "Listening...";
}

/** Compact listening shell: tight when empty, grows with speech up to voiceWidth. */
function voiceShellWidth(text: string, thinking: boolean, working?: boolean): number {
  const label = voiceLabel(text, thinking, working);
  // Orb column, gaps, green bars, right pad.
  const chrome = COMPACT.height + 4 + 10 + 20 + 14;
  const textPx = Math.ceil([...label].length * 7.4);
  return Math.min(COMPACT.voiceWidth, Math.max(COMPACT.voiceMinWidth, chrome + textPx));
}

/** Voice in the compact island: green bars and the words as they come
 * while listening, then "Thinking" with the question until the answer. */
function VoicePill({ text, thinking, working }: { text: string; thinking: boolean; working?: boolean }) {
  const label = voiceLabel(text, thinking, working);
  return (
    <motion.div
      className="absolute top-0 right-0 flex h-9 items-center gap-2.5 pr-3.5"
      style={{ left: COMPACT.height + 4 }}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: { delay: 0.1, duration: 0.2 } }}
      exit={{ opacity: 0, transition: { duration: 0.08 } }}
      aria-live="polite"
    >
      <span
        className={`min-w-0 flex-1 truncate text-[12.5px] font-medium ${
          text.trim() || thinking ? "text-white/90" : "text-white/50"
        }`}
      >
        {label}
      </span>
      {thinking ? <Activity /> : <VoiceBars />}
    </motion.div>
  );
}

function CompactTrailing({
  paused,
  busy,
  offline,
  waiting,
}: {
  paused: boolean;
  busy: boolean;
  offline: boolean;
  waiting: string | null;
}) {
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
      ) : offline ? (
        <span role="img" aria-label="Offline" title="Offline" className="text-[#ff9f0a]">
          <Icon name="wifiOff" size={14} />
        </span>
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
  const running = useSidekick((s) => s.running);
  const netNotice = useSidekick((s) => s.netNotice);
  const reporting = (mascot === "success" || mascot === "error" || mascot === "working") && !suggestion;
  if (netNotice && !suggestion) return <NetNoticeCard notice={netNotice} />;
  if (!suggestion && (mascot === "idle" || mascot === "sleeping")) {
    return <Glance paused={paused} />;
  }
  // Working names what it is doing; done and error say what happened.
  const detail =
    suggestion?.detail ??
    (mascot === "working" && running
      ? `${running}...`
      : reporting && result
        ? result.message
        : paused && mascot !== "sleeping"
          ? "Sensors are paused."
          : DETAIL[mascot]);
  return (
    <div className="flex flex-col">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate font-display text-[15px] leading-5 font-semibold tracking-[-0.015em] text-white">
            {suggestion?.title ?? TITLE[mascot]}
          </p>
          <p
            className={`mt-0.5 ${
              suggestion?.skillId.startsWith("notify.") ? "line-clamp-4 whitespace-pre-line" : "line-clamp-2"
            } text-[13px] leading-4.5 tracking-[-0.005em] text-[rgb(235_235_245/0.6)]`}
          >
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
        ) : null}
      </div>

      {suggestion ? <Options key={suggestion.id} suggestion={suggestion} /> : null}
    </div>
  );
}

/** The connection dropped or came back. */
function NetNoticeCard({ notice }: { notice: NetNotice }) {
  return (
    <div className="flex items-start gap-3">
      <div className="min-w-0 flex-1">
        <p className="truncate font-display text-[15px] leading-5 font-semibold tracking-[-0.015em] text-white">
          {notice.title}
        </p>
        <p className="mt-0.5 line-clamp-2 text-[13px] leading-4.5 tracking-[-0.005em] text-[rgb(235_235_245/0.6)]">
          {notice.detail}
        </p>
      </div>
      <span
        className={`grid size-8 shrink-0 place-items-center rounded-full ${
          notice.online ? "bg-[#30d158]/15 text-[#30d158]" : "bg-[#ff9f0a]/15 text-[#ff9f0a]"
        }`}
        aria-hidden="true"
      >
        <Icon name={notice.online ? "wifi" : "wifiOff"} size={16} />
      </span>
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
          Always {suggestion.options[alwaysAt].toLowerCase()}
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
  useSidekick.setState({ running: suggestion.options[index] ?? null });
  notePick(suggestion.skillId, suggestion.options[index] ?? "");
  void api.suggestionChoose(suggestion.id, index);
}

function always(suggestion: Suggestion, index: number) {
  playSound("select", uiVolume(), useSidekick.getState().settings.soundKit);
  useSidekick.setState({ running: suggestion.options[index] ?? null });
  void api.suggestionAlways(suggestion.id, index);
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

/** Stops Chromium/WebView2 browser chrome shortcuts that have no place here. */
function useBlockBrowserKeys() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      if (e.key.toLowerCase() !== "j") return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);
}
