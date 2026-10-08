"use client";

// The island: a black capsule that morphs like Apple's Dynamic Island. One
// Orb instance lives inside and travels between the compact and expanded
// layouts, so its gaze and blink never reset. The shell size is spring driven
// and interruptible; content cross-fades through a short blur.

import { AnimatePresence, animate, motion, useMotionValue, useReducedMotion } from "motion/react";
import { type CSSProperties, useCallback, useEffect, useRef, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { listenToAgents, useAgents, wideScreen } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useNow, useSystemLook } from "@/lib/hooks";
import { ISLAND_TOP, PANEL_PAD } from "@/lib/islandSize";
import type { NetNotice } from "@/lib/net";
import { playSound } from "@/lib/sound";
import { connect, notePick, setHovered, uiVolume, useSidekick, watchWaiting } from "@/lib/store";
import { isPaused, type MascotState, type Suggestion } from "@/lib/types";
import { Announcer } from "./Announcer";
import { ASK_MASCOT, ASK_ORB, ASK_PAD, AskPanel, VoiceBars } from "./AskPanel";
import { Icon } from "./Icon";
import { Glance, RoundButton } from "./IslandGlance";
import { IslandGuide, useGuide } from "./IslandGuide";
import { IslandSettings } from "./IslandSettings";
import { IslandWelcome } from "./IslandWelcome";
import { MergeCard } from "./MergeCard";
import { Orb } from "./Orb";
import { PreparingVoice } from "./PreparingVoice";
import { Tip } from "./Tip";

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
/** Ask mode, as in the design: 420 wide, 580 for the Agents tab. */
const ASK_WIDTH = 420;
const AGENTS_WIDTH = 580;
/** The board: two tiles across, three on a 1440p screen and up. */
const BOARD_WIDTH = 680;
const BOARD_WIDE_WIDTH = 940;
/** Voice, Full listening style. */
const FULL_VOICE_WIDTH = 400;
const ASK_RADIUS = 26;
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
/** Ctrl+Space is a keyboard action used many times a day: Ask opens in one
 * quick, calm move, with no bounce and no blur. */
const askOpen = { duration: 0.2, ease: [0.23, 1, 0.32, 1] } as const;

export function Island() {
  const { mascot, settings, suggestion, hovered: rawHover, visible, ready } = useSidekick();
  const asking = useSidekick((s) => s.ask !== null);
  const askTab = useAgents((s) => s.tab);
  const agentsLayout = useAgents((s) => s.layout);
  useEffect(listenToAgents, []);
  const view = useSidekick((s) => s.ask?.view);
  // The Ask panel itself (not Settings or the welcome shown in its place).
  const askPanel = asking && view !== "settings" && view !== "welcome";
  const chatting = useSidekick((s) => s.chatId !== null);
  const voiceStatus = useSidekick((s) => s.voiceStatus);
  const online = useSidekick((s) => s.online);
  const netNotice = useSidekick((s) => s.netNotice);
  const merging = useSidekick((s) => s.merge !== null);
  const waiting = useSidekick((s) => (s.ask ? null : s.waiting));
  const justDone = useSidekick((s) => s.justDone);
  // A task still running after Ask closed: its current step, small.
  const chatWorking = useSidekick((s) => {
    const last = s.turns[s.turns.length - 1];
    if (!s.chatId || !last?.streaming) return null;
    return last.tool ? `${last.tool}...` : "Working...";
  });
  // Agents keep working with Ask closed: a small pill says so, and says
  // when one needs an answer. Hovering it lists them.
  const agentWorking = useAgents((s) => {
    const waiting = s.sessions.find((x) => x.status === "waiting");
    if (waiting) return `${waiting.agent} needs you`;
    const live = s.sessions.filter((x) => x.status === "working" || x.status === "waiting");
    const busy = live.filter((x) => x.status === "working");
    if (live.length > 1)
      return busy.length === live.length
        ? `${live.length} agents working`
        : `${live.length} agents · ${busy.length} working`;
    return busy[0] ? `${busy[0].agent}: ${busy[0].project}` : null;
  });
  // One dot per running agent in the compact pill.
  const agentDots = useAgents(
    useShallow((s) => s.sessions.filter((x) => x.status === "working" || x.status === "waiting").map((x) => x.status)),
  );
  // A question opens the island by itself, unless Do Not Disturb, a
  // fullscreen app or a pause says to stay small; it shrinks back once
  // answered.
  const questionId = useAgents((s) => s.sessions.find((x) => x.question)?.question?.id ?? null);
  const fullscreen = useSidekick((s) => s.fullscreen);
  const [dndOn, setDndOn] = useState(false);
  useEffect(() => {
    if (!questionId) return;
    let live = true;
    void api
      .dndGet()
      .then((v) => live && setDndOn(v === true))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [questionId]);
  const working = useSidekick((s) => (s.ask ? null : (chatWorking ?? agentWorking)));
  // An agent waiting on an answer: the mascot looks up curious and the pill
  // shows an amber dot instead of the busy bars.
  const agentAsks = useAgents((s) => s.sessions.some((x) => x.status === "waiting"));
  const asksYou = working !== null && chatWorking === null && agentAsks;
  const guide = useGuide(waiting);
  // Voice with Ask closed: a compact pill while listening and thinking; the
  // island opens only when the answer starts.
  const hearing = useSidekick((s) => (s.ask ? null : s.hearing));
  const voiceQuestion = useSidekick((s) => (s.ask ? null : s.voiceQuestion));
  // Something finished that needs nothing more: a short done pill.
  const donePill = useSidekick((s) => (s.ask ? null : s.donePill));
  // Focus mode: a compact pill with the focus face and the time left.
  const focusUntil = useSidekick((s) => (s.ask ? null : s.focusUntil));
  const focusLeft = useFocusLeft(focusUntil);
  const reduced = useReducedMotion() ?? false;
  const look = useSystemLook();
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
  const voiceBusy =
    hearing !== null ||
    voiceQuestion !== null ||
    donePill !== null ||
    mascot === "listening" ||
    working !== null ||
    focusLeft !== null;
  const voicePill: {
    text: string;
    thinking: boolean;
    working?: boolean;
    done?: boolean;
    focus?: boolean;
    asks?: boolean;
    dots?: string[];
  } | null = asking
    ? null
    : donePill !== null
      ? { text: donePill, thinking: false, done: true }
      : voiceQuestion !== null
        ? { text: voiceQuestion, thinking: true }
        : hearing !== null || mascot === "listening"
          ? { text: hearing ?? "", thinking: false }
          : working !== null
            ? { text: working, thinking: !asksYou, working: true, asks: asksYou, dots: agentDots }
            : focusLeft !== null
              ? { text: `Focus · ${focusLeft}`, thinking: false, focus: true }
              : null;
  // Only agents at work: hover lists them (and answers in place) instead of
  // opening Ask.
  const agentsOnly = voiceQuestion === null && hearing === null && chatWorking === null && agentWorking !== null;
  // Full listening style: the island opens while you talk, with a waveform,
  // a timer and your words larger, instead of the slim pill.
  const fullVoice =
    voicePill !== null &&
    !voicePill.working &&
    !voicePill.focus &&
    !voicePill.done &&
    (settings.voice.listeningStyle ?? "compact") === "full";
  // Suggestions stay normal during thinking — do not gate them on !voicePill.
  const askOpens =
    questionId !== null && !dndOn && !fullscreen && !paused && settings.onboarded && agentsOnly && !quiet;
  const expanded =
    asking ||
    askOpens ||
    fullVoice ||
    preparingVoice ||
    (hovered && (!voicePill || agentsOnly)) ||
    guiding ||
    (OPEN_STATES.has(mascot) && !voicePill) ||
    !!suggestion ||
    (!!netNotice && !voicePill) ||
    merging;
  // At rest only the sphere shows. The shell keeps its size (so hover and the
  // orb position do not move) but loses its background.
  const bare =
    !expanded && !chatting && !waiting && !voiceBusy && online && (mascot === "idle" || mascot === "sleeping");
  const busy = chatting || preparingVoice || voiceBusy || mascot === "noticing" || mascot === "working";

  // Hover while Thinking or Working: open Ask so the island is usable, not a dead pill.
  useEffect(() => {
    if (!intent || quiet || asking || !settings.onboarded) return;
    if (voiceQuestion === null && working === null) return;
    if (agentsOnly) return;
    void api.askOpen();
  }, [intent, quiet, asking, settings.onboarded, voiceQuestion, working, agentsOnly]);

  const [contentHeight, setContentHeight] = useState(0);
  const bump = useMotionValue(1);
  // A mood (thanks, Claude finished) shows for a moment; offline and at
  // rest, the mascot looks a little lost.
  const mood = useSidekick((s) => s.mood);
  const speaking = useSidekick((s) => s.speaking);
  const face =
    mood?.id ??
    (asksYou && !voiceQuestion && hearing === null ? "curious" : null) ??
    (focusLeft !== null && !voiceQuestion && hearing === null && mascot !== "listening" ? "focus" : null) ??
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
      : askPanel && askTab === "agents"
        ? agentsLayout === "board"
          ? wideScreen()
            ? BOARD_WIDE_WIDTH
            : BOARD_WIDTH
          : AGENTS_WIDTH
        : ASK_WIDTH
    : expanded
      ? fullVoice
        ? FULL_VOICE_WIDTH
        : showGuide
          ? GUIDE_WIDTH
          : EXPANDED.width
      : voicePill
        ? voiceShellWidth(voicePill.text, voicePill.thinking, voicePill.working || voicePill.done)
        : waiting
          ? COMPACT.waitWidth
          : busy
            ? COMPACT.busyWidth
            : !online
              ? COMPACT.offlineWidth
              : COMPACT.width;
  // The island window is already fixed (~560 tall); do not re-cap against
  // innerHeight or Settings/Welcome get clipped by the shell spring.
  const height = expanded
    ? Math.max(EXPANDED.minHeight, contentHeight + (askPanel ? ASK_PAD.bottom : EXPANDED.pad))
    : COMPACT.height;
  const radius = askPanel ? ASK_RADIUS : expanded ? EXPANDED.radius : COMPACT.radius;
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
  const transition = reduced
    ? { duration: 0 }
    : expanded
      ? settled
        ? resize
        : asking
          ? askOpen
          : morphOpen
      : morphClose;

  const overlayHit = useSidekick((s) => s.overlayHit);
  // Report the target shape as the interactive area; outside it the window
  // stays click-through. A floating menu expands the rect so it stays usable.
  useEffect(() => {
    const base = { x: (window.innerWidth - width) / 2, y: TOP, width, height };
    const rect = overlayHit
      ? {
          x: Math.min(base.x, overlayHit.x),
          y: Math.min(base.y, overlayHit.y),
          width: Math.max(base.x + base.width, overlayHit.x + overlayHit.width) - Math.min(base.x, overlayHit.x),
          height: Math.max(base.y + base.height, overlayHit.y + overlayHit.height) - Math.min(base.y, overlayHit.y),
        }
      : base;
    void api.islandSetHitRect(rect);
  }, [width, height, overlayHit]);

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
  const orbScale = askPanel ? ASK_MASCOT.size / ORB : asking ? ASK_ORB / ORB : expanded ? 1 : COMPACT.orb / ORB;
  const orbX = askPanel ? ASK_MASCOT.x : expanded ? EXPANDED.pad : (COMPACT.height - COMPACT.orb) / 2 + 1;
  const orbY = askPanel ? ASK_MASCOT.y : expanded ? EXPANDED.pad : (COMPACT.height - COMPACT.orb) / 2;
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
        data-solid={look.solid || undefined}
        data-simple={look.simple || undefined}
        initial={false}
        animate={{ width, height, borderRadius: radius }}
        transition={transition}
        style={{ scaleX: bump, originY: 0 }}
        // Focusing the input while the shell is still small would scroll its
        // content up; the shell never scrolls.
        onScroll={(e) => {
          e.currentTarget.scrollTop = 0;
        }}
        onPointerEnter={() => setHovered(true)}
        onPointerLeave={() => setHovered(false)}
      >
        <Announcer />
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
            alive={settings.alive && visible && !look.simple}
            simple={look.simple}
            active={shown && !bare}
          />
        </motion.div>

        <AnimatePresence initial={false}>
          {!expanded && voicePill && (
            <VoicePill key="voice" {...voicePill} onOpen={agentsOnly ? () => openBoard() : undefined} />
          )}
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
              style={
                askPanel
                  ? { left: ASK_PAD.x, paddingTop: ASK_PAD.top, paddingRight: ASK_PAD.x }
                  : { left: EXPANDED.pad, paddingTop: EXPANDED.pad, paddingRight: EXPANDED.pad }
              }
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0, transition: { duration: 0.08 } }}
              transition={{ duration: reduced ? 0 : 0.14, ease: "easeOut" }}
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
                {fullVoice && voicePill ? (
                  <FullVoice text={voicePill.text} thinking={voicePill.thinking} />
                ) : showGuide && waiting ? (
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

/** Full listening style: a waveform, a timer and Stop on the first line,
 * then your words a size larger (grey until final) and what happens next. */
function FullVoice({ text, thinking }: { text: string; thinking: boolean }) {
  const [start] = useState(() => Date.now());
  const now = useNow(1000);
  const secs = Math.max(0, Math.floor((now - start) / 1000));
  const words = text.trim();
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex h-9 items-center gap-2.5">
        {thinking ? (
          <span className="flex-1">
            <Activity />
          </span>
        ) : (
          <span className="voice-wave min-w-0 flex-1" aria-hidden="true">
            {Array.from({ length: 30 }, (_, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: fixed bars
              <i key={i} style={{ animationDelay: `${-((i * 0.37) % 0.9).toFixed(2)}s` }} />
            ))}
          </span>
        )}
        <span className="min-w-[2.6em] text-right font-mono text-[11.5px] text-white/45 tabular-nums">
          {Math.floor(secs / 60)}:{String(secs % 60).padStart(2, "0")}
        </span>
        <Tip label="Stop (Esc)">
          <button
            type="button"
            aria-label="Stop listening"
            onClick={() => void api.voiceStop()}
            className="chip grid size-7 shrink-0 place-items-center rounded-full bg-white/[0.09] hover:bg-white/[0.15]"
          >
            <span className="size-[9px] rounded-[2.5px] bg-white" />
          </button>
        </Tip>
      </div>
      <p
        className={`min-h-[1.45em] px-1 text-[16.5px] leading-[1.45] tracking-[-0.005em] ${
          words ? "text-white" : "text-white/45"
        }`}
      >
        {words || "Listening..."}
      </p>
      <p className="flex items-center gap-[7px] px-1 pb-0.5 text-[11.5px] text-white/45">
        {thinking ? (
          <span className="shimmer-text">Thinking</span>
        ) : (
          <>
            <span className="voice-live size-1.5 rounded-full bg-white" aria-hidden="true" />
            Listening · pause to send ·
            <kbd className="rounded-[4px] bg-white/[0.09] px-[5px] py-px font-mono text-[10.5px] text-white/62">
              Esc
            </kbd>
            cancel
          </>
        )}
      </p>
    </div>
  );
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
/** The compact agents pill opens Agents on the board. */
function openBoard() {
  useAgents.setState({ tab: "agents", layout: "board" });
  void api.askOpen();
}

const DOT: Record<string, string> = { working: "#64d2ff", waiting: "#ff9f0a" };

function VoicePill({
  text,
  thinking,
  working,
  done,
  focus,
  asks,
  dots,
  onOpen,
}: {
  text: string;
  thinking: boolean;
  working?: boolean;
  done?: boolean;
  focus?: boolean;
  asks?: boolean;
  dots?: string[];
  onOpen?: () => void;
}) {
  const label = voiceLabel(text, thinking, working || done || focus);
  return (
    <motion.div
      className="absolute top-0 right-0 flex h-9 items-center gap-2.5 pr-3.5"
      style={{ left: COMPACT.height + 4 }}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: { delay: 0.1, duration: 0.2 } }}
      exit={{ opacity: 0, transition: { duration: 0.08 } }}
      aria-live="polite"
    >
      {onOpen ? (
        <button
          type="button"
          onClick={onOpen}
          className="min-w-0 flex-1 truncate text-left text-[12.5px] font-medium text-white/90"
        >
          {label}
        </button>
      ) : (
        <span
          className={`min-w-0 flex-1 truncate text-[12.5px] font-medium ${
            text.trim() || thinking ? "text-white/90" : "text-white/62"
          }`}
        >
          {label}
        </span>
      )}
      {done ? (
        <span className="text-[#30d158]">
          <Icon name="check" size={14} />
        </span>
      ) : focus ? (
        <button
          type="button"
          aria-label="End focus"
          onClick={() => void api.focusStop()}
          className="rounded-full px-2 py-0.5 text-[11px] text-white/62 hover:bg-white/10 hover:text-white"
        >
          End
        </button>
      ) : asks ? (
        <span className="relative flex size-2" role="img" aria-label="Waiting for you">
          <span className="absolute inset-0 animate-ping rounded-full bg-[#ff9f0a]/60 motion-reduce:animate-none" />
          <span className="relative size-2 rounded-full bg-[#ff9f0a]" />
        </span>
      ) : dots && dots.length > 1 ? (
        <span className="flex items-center gap-1" role="img" aria-label={`${dots.length} agents`}>
          {dots.map((d, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: one dot per agent, in order
            <i key={i} className="size-1.5 rounded-full" style={{ background: DOT[d] ?? "rgb(255 255 255 / 0.3)" }} />
          ))}
        </span>
      ) : thinking ? (
        <Activity />
      ) : (
        <VoiceBars />
      )}
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
        <Tip label="Offline">
          <span role="img" aria-label="Offline" className="text-[#ff9f0a]">
            <Icon name="wifiOff" size={14} />
          </span>
        </Tip>
      ) : paused ? (
        <Tip label="Paused">
          <span className="size-1.5 rounded-full bg-[#ffd60a]" style={{ boxShadow: "0 0 8px #ffd60a" }} />
        </Tip>
      ) : (
        later > 0 && (
          <Tip label={`${later} waiting for you`}>
            <span className="grid h-4 min-w-4 place-items-center rounded-full bg-[#0a84ff] px-1 text-[10px] leading-none font-semibold text-white">
              {later}
            </span>
          </Tip>
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
  const merge = useSidekick((s) => s.merge);
  const reporting = (mascot === "success" || mascot === "error" || mascot === "working") && !suggestion;
  if (merge && !suggestion) return <MergeCard path={merge.path} />;
  if (netNotice && !suggestion) return <NetNoticeCard notice={netNotice} />;
  // A suggestion that expired or was taken leaves nothing to show: fall back to the glance.
  if (!suggestion && (mascot === "idle" || mascot === "sleeping" || mascot === "suggesting")) {
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
              suggestion?.skillId.startsWith("notify.") || (!suggestion && detail.includes("\n"))
                ? "line-clamp-6 whitespace-pre-line"
                : "line-clamp-2"
            } text-[13px] leading-4.5 tracking-[-0.005em] text-[rgb(235_235_245/0.6)]`}
          >
            {detail}
          </p>
          {suggestion?.why && (
            <p className="mt-1 truncate text-[11.5px] leading-4 text-[rgb(235_235_245/0.42)]">{suggestion.why}</p>
          )}
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
          onClick={(e) => choose(suggestion, i, e.shiftKey)}
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
              i === 0 ? "text-black/40" : "text-white/45"
            }`}
          >
            Alt {i + 1}
          </kbd>
        </motion.button>
      ))}
      {alwaysAt >= 0 && (
        <Tip label={`From now on, "${suggestion.options[alwaysAt]}" without asking. Undo in Settings > Skills.`}>
          <motion.button
            type="button"
            onClick={() => always(suggestion, alwaysAt)}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.2, delay: 0.12 + suggestion.options.length * 0.04 }}
            className="chip rounded-full px-2.5 py-1.5 text-[13px] text-[rgb(235_235_245/0.6)] hover:text-white"
          >
            Always {suggestion.options[alwaysAt].toLowerCase()}
          </motion.button>
        </Tip>
      )}
      <motion.button
        type="button"
        onClick={() => dismiss(suggestion)}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.2, delay: 0.12 + suggestion.options.length * 0.04 }}
        className="ak-ignore chip rounded-full px-3 py-1.5 text-[13px] text-[rgb(235_235_245/0.7)] hover:text-white"
      >
        {/* Empties as the island's own close timer runs; full again on hover. */}
        <span
          className="ak-ignore-fill"
          aria-hidden="true"
          style={{ animationDuration: `${useSidekick.getState().settings.collapseAfterSecs}s` }}
        />
        <span className="relative">Ignore</span>
        <kbd className="relative ml-1.5 font-sans text-[11px] text-white/45">
          <i className="alt-pre">Alt </i>0
        </kbd>
      </motion.button>
    </div>
  );
}

/** Shift opens a link in a private window. */
function choose(suggestion: Suggestion, index: number, priv = false) {
  playSound("select", uiVolume(), useSidekick.getState().settings.soundKit);
  useSidekick.setState({ running: suggestion.options[index] ?? null });
  notePick(suggestion.skillId, suggestion.options[index] ?? "");
  void api.suggestionChoose(suggestion.id, index, priv);
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
      if (n >= 1 && n <= suggestion.options.length) choose(suggestion, n - 1, e.shiftKey);
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

/** "24:10" left in Focus mode, ticking each second, or null. */
function useFocusLeft(until: number | null): string | null {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (until === null) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [until]);
  if (until === null || until <= now) return null;
  const secs = Math.ceil((until - now) / 1000);
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const two = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${two(m)}:${two(s)}` : `${m}:${two(s)}`;
}
