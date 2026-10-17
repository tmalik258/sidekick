"use client";

// The island: a black capsule that morphs like Apple's Dynamic Island. One
// Orb instance lives inside and travels between the compact and expanded
// layouts, so its gaze and blink never reset. The shell size is spring driven
// and interruptible; content cross-fades through a short blur.

import { AnimatePresence, animate, motion, useMotionValue, useReducedMotion } from "motion/react";
import { type CSSProperties, useCallback, useEffect, useRef, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import {
  activityLabel,
  INACTIVITY_MS,
  latestActivity,
  ownsWorking,
  terminal,
  useActivities,
  visibleActivities,
} from "@/lib/activities";
import { interruptSession, listenToAgents, useAgents, wideScreen } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow, useSystemLook } from "@/lib/hooks";
import { PILL_FRESH_MS } from "@/lib/peek";
import { resolveIslandState } from "@/lib/resolve-island-state";
import { connect, setHovered, stopLive, useDuskie, watchWaiting } from "@/lib/store";
import { type CalendarToday, isPaused, type MascotState, type Settings, type Suggestion } from "@/lib/types";
import { updateBusy, updateLabel } from "@/lib/updates";
import { PALETTES, type WashEvent } from "@/lib/wash";
import { ActivitiesCard } from "./ActivitiesCard";
import { AgentCard } from "./AgentCard";
import { Announcer } from "./Announcer";
import { ASK_PAD, AskPanel } from "./AskPanel";
import { Creature } from "./Creature";
import { type BubbleItem, IslandBubbles } from "./IslandBubbles";
import { IslandGuide, useGuide } from "./IslandGuide";
import { CompactTrailing, ExpandedContent } from "./IslandParts";
import { IslandPill, type PillModel } from "./IslandPill";
import { IslandSettings } from "./IslandSettings";
import { FullVoice, openBoard, VoicePill, voiceShellWidth } from "./IslandVoice";
import { IslandWelcome } from "./IslandWelcome";
import { useBlockBrowserKeys, useFocusLeft, useIntent, useSuggestionKeys } from "./island-hooks";
import { COMPACT, EXPANDED, OPEN_STATES, SHELL_HOVER_IN_MS, SHELL_HOVER_OUT_MS, TOP } from "./island-layout";
import { MeetingCard } from "./MeetingCard";
import { NetCard } from "./NetCard";
import { PreparingVoice } from "./PreparingVoice";
import { UpdateCard } from "./UpdateCard";
import { shellGeom } from "./use-island-shell";

export function Island() {
  const { mascot, settings, suggestion, hovered: rawHover, visible, ready } = useDuskie();
  const asking = useDuskie((s) => s.ask !== null);
  const askTab = useAgents((s) => s.tab);
  const agentsLayout = useAgents((s) => s.layout);
  const agentCurrent = useAgents((s) => s.current);
  useEffect(listenToAgents, []);
  const view = useDuskie((s) => s.ask?.view);
  // The Ask panel itself (not Settings or the welcome shown in its place).
  const askPanel = asking && view !== "welcome";
  const chatting = useDuskie((s) => s.chatId !== null);
  const voiceStatus = useDuskie((s) => s.voiceStatus);
  const online = useDuskie((s) => s.online);
  const laterCount = useDuskie((s) => s.later);
  // A newly held suggestion shows as a short notice, then the island rests.
  const storeReady = useDuskie((s) => s.ready);
  const [laterSeen, setLaterSeen] = useState<number | null>(null);
  const [laterFresh, setLaterFresh] = useState(false);
  if (storeReady && laterSeen === null) setLaterSeen(laterCount);
  else if (laterSeen !== null && laterCount !== laterSeen) {
    setLaterSeen(laterCount);
    setLaterFresh(laterCount > laterSeen);
  }
  useEffect(() => {
    if (!laterFresh) return;
    const t = setTimeout(() => setLaterFresh(false), 8000);
    return () => clearTimeout(t);
  }, [laterFresh]);
  const later = laterFresh ? laterCount : 0;
  // Design: losing the connection only changes the pill; the card opens on a click.
  const [netOpen, setNetOpen] = useState(false);
  if (netOpen && online) setNetOpen(false);
  const netShown = netOpen;
  const merging = useDuskie((s) => s.merge !== null);
  const updateProgress = useDuskie((s) => s.updateProgress);
  const updateActivity = useActivities((s) => latestActivity(s.activities, "update"));
  const updateSuppressed = updateActivity?.dismissed;
  const updateNow = useNow(1000);
  const updateWorking =
    updateProgress && !updateSuppressed && updateBusy(updateProgress)
      ? updateActivity
        ? activityLabel(updateActivity, updateNow)
        : updateLabel(updateProgress)
      : null;
  const waiting = useDuskie((s) => (s.ask ? null : s.waiting));
  const justDone = useDuskie((s) => s.justDone);
  // A task still running after Ask closed: its current step, small.
  const chatWorking = useDuskie((s) => {
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
  // One ring arc per running agent in the compact pill.
  const agentDots = useAgents(
    useShallow((s) => s.sessions.filter((x) => x.status === "working" || x.status === "waiting").map((x) => x.status)),
  );
  // A question opens the island by itself, unless Do Not Disturb, a
  // fullscreen app or a pause says to stay small; it shrinks back once
  // answered.
  const questionId = useAgents((s) => s.sessions.find((x) => x.question)?.question?.id ?? null);
  const fullscreen = useDuskie((s) => s.fullscreen);
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
  // Skill / action in flight with Ask closed: same slim pill as chat/agents.
  // Combine outside a store selector so closed-over chat/agent labels stay fresh.
  const activityState = useActivities();
  const activityNow = useNow(1000);
  const keepProblems = true;
  const liveActivities = visibleActivities(
    [...activityState.activities, ...activityState.local],
    activityNow,
    keepProblems,
  );
  const foreground = liveActivities.filter((a) => !["browser", "file"].includes(a.source));
  const backgroundAll = liveActivities.filter((a) => ["browser", "file"].includes(a.source));
  const background = backgroundAll.filter((a) => !terminal(a));
  const pausedDownloads = background.filter((a) => a.phase === "awaitingUser").length;
  const waitingDownloads = background.filter(
    (a) => a.phase === "unconfirmed" || activityNow - a.lastProgressAt >= INACTIVITY_MS,
  ).length;
  const skillWorking = foreground[0] ? activityLabel(foreground[0], activityNow) : null;
  const downloadWorking =
    background.length > 1
      ? pausedDownloads > 0
        ? `${background.length} downloads · ${pausedDownloads} paused`
        : waitingDownloads > 0
          ? `${background.length} downloads · ${waitingDownloads} awaiting progress`
          : `${background.length} downloads in progress`
      : backgroundAll[0]
        ? activityLabel(background[0] ?? backgroundAll[0], activityNow)
        : null;
  useEffect(() => {
    if (
      mascot !== "working" ||
      chatWorking ||
      agentWorking ||
      activityState.revision < 0 ||
      [...activityState.activities, ...activityState.local].some(ownsWorking)
    )
      return;
    const timer = setTimeout(() => {
      const current = useActivities.getState();
      if (
        [...current.activities, ...current.local].some(ownsWorking) ||
        useDuskie.getState().chatId ||
        useAgents.getState().sessions.some((s) => s.status === "working" || s.status === "waiting")
      )
        return;
      useDuskie.setState((s) => (s.mascot === "working" ? { mascot: "idle", running: null } : {}));
      void api.activityReconcile().catch(() => {});
    }, 1000);
    return () => clearTimeout(timer);
  }, [mascot, chatWorking, agentWorking, activityState]);
  const agentAsks = useAgents((s) => s.sessions.some((x) => x.status === "waiting"));
  const askingTitle = useAgents((s) => s.sessions.find((x) => x.status === "waiting")?.title ?? null);
  const { data: calendarToday } = useCached<CalendarToday>("calendar-today", api.calendarToday);
  const meetingSoon = soonMeeting(updateNow, calendarToday);
  const cardAgent = useAgents(
    (s) =>
      (s.sessions.find((x) => x.status === "waiting") ?? s.sessions.find((x) => x.status === "working"))?.id ?? null,
  );
  const splitAgent = useAgents(
    useShallow((s) => {
      const x = s.sessions.find((y) => y.status === "working");
      if (!x) return null;
      const mark = x.agent
        .split(/\s+/)
        .map((w) => w[0] ?? "")
        .join("")
        .slice(0, 2)
        .toUpperCase();
      return {
        id: x.id,
        agent: x.agent,
        mark,
        done: x.plan.filter((p) => p.status === "completed").length,
        total: x.plan.length,
      };
    }),
  );
  const askingCount = useAgents((s) => s.sessions.filter((x) => x.status === "waiting").length);
  const working = asking
    ? null
    : ((agentAsks ? agentWorking : null) ??
      updateWorking ??
      skillWorking ??
      chatWorking ??
      agentWorking ??
      downloadWorking);
  // Prefer an exact label match. Multi-download and update-without-activity
  // labels are synthetic, so fall back to the owning row when find misses.
  const displayedActivity = working
    ? (liveActivities.find((a) => activityLabel(a, activityNow) === working) ??
      (working === downloadWorking ? (background[0] ?? backgroundAll[0]) : undefined) ??
      (working === updateWorking ? (updateActivity ?? undefined) : undefined))
    : undefined;
  const activityWaiting =
    (working === downloadWorking && background.length > 1 && (pausedDownloads > 0 || waitingDownloads > 0)) ||
    (displayedActivity &&
      !terminal(displayedActivity) &&
      (displayedActivity.phase === "awaitingUser" ||
        displayedActivity.phase === "unconfirmed" ||
        activityNow - displayedActivity.lastProgressAt >= INACTIVITY_MS));
  // An agent waiting on an answer: the mascot looks up curious and the pill
  // shows an amber dot instead of the busy bars.
  const asksYou = agentAsks && working === agentWorking;
  const guide = useGuide(waiting);
  // Voice with Ask closed: a compact pill while listening and thinking; the
  // island opens only when the answer starts.
  const hearing = useDuskie((s) => (s.ask ? null : s.hearing));
  const live = useDuskie((s) => (s.ask ? null : s.live));
  const voiceQuestion = useDuskie((s) => (s.ask ? null : s.voiceQuestion));
  // Something finished that needs nothing more: a short done pill.
  // Also covers mascot success without Undo / Open (e.g. "Switched to Claude"
  // after Settings closed, showDone was skipped while Ask was open).
  const donePill = useDuskie((s) => {
    if (s.ask || updateBusy(s.updateProgress)) return null;
    if (s.donePill) return s.donePill;
    if (s.mascot !== "success") return null;
    const r = s.lastResult;
    if (r?.undoId != null || r?.path) return null;
    return r?.message ?? "Done";
  });
  // Success that still needs Undo or Open folder: keep the expanded card.
  const successCard = useDuskie((s) => {
    if (s.ask || s.mascot !== "success") return false;
    const r = s.lastResult;
    return !!(r && (r.undoId != null || r.path));
  });
  // Focus mode: a compact pill with the focus face and the time left.
  const focusUntil = useDuskie((s) => (s.ask ? null : s.focusUntil));
  const focusLeft = useFocusLeft(focusUntil);
  const reduced = useReducedMotion() ?? false;
  const look = useSystemLook();
  const now = useNow(15_000);
  const paused = isPaused(settings.pause, now);
  // Closing a panel (Hide, Esc, Done) goes straight to the small orb. The
  // hover card stays off until the cursor leaves and comes back; otherwise
  // shrinking the hit rect clears quiet while useIntent still thinks we are
  // hovering, and "Watching for moments" flashes.
  const [contentHeight, setContentHeight] = useState(0);
  const [prevAsking, setPrevAsking] = useState(asking);
  const [quiet, setQuiet] = useState(false);
  if (prevAsking !== asking) {
    setPrevAsking(asking);
    if (!asking) setQuiet(true);
    // Drop stale panel height on Ask open so the shell grows from compact
    // instead of opening tall and shrinking to the new measure.
    if (asking) setContentHeight(0);
  }
  // Cleared only once the cursor has been away as long as hover intent takes
  // to fall; clearing at once let a stale intent reopen a just-hidden welcome.
  useEffect(() => {
    if (!quiet || rawHover) return;
    const id = setTimeout(() => setQuiet(false), SHELL_HOVER_OUT_MS);
    return () => clearTimeout(id);
  }, [quiet, rawHover]);
  const intent = useIntent(rawHover && !quiet, SHELL_HOVER_IN_MS);

  // Fresh notification pill → unread count after PILL_FRESH_MS.
  const [notifyAt, setNotifyAt] = useState<number | null>(null);
  const suggestionId = suggestion?.id ?? null;
  useEffect(() => {
    if (!suggestionId) {
      setNotifyAt(null);
      return;
    }
    setNotifyAt(Date.now());
    const id = setTimeout(() => setNotifyAt(null), PILL_FRESH_MS);
    return () => clearTimeout(id);
  }, [suggestionId]);
  // A suggestion shows as a compact line first (the peek replaces the
  // fresh-message pill); hover 220ms or holding Alt opens the card, leaving
  // folds it back after 450ms, and a click inside keeps it open.
  const notifyFresh = (meetingSoon !== null && !paused) || (false && notifyAt !== null);
  const [peek, setPeek] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [altHeld, setAltHeld] = useState(false);
  const [peekFor, setPeekFor] = useState(suggestionId);
  if (peekFor !== suggestionId) {
    setPeekFor(suggestionId);
    setPeek(false);
    setPinned(false);
  }
  useEffect(() => {
    if (!suggestionId) return;
    const id = setTimeout(() => setPeek(rawHover), rawHover ? 220 : 450);
    return () => clearTimeout(id);
  }, [suggestionId, rawHover]);
  useEffect(() => {
    if (!suggestionId) return;
    const down = (e: KeyboardEvent) => {
      if (e.key === "Alt") setAltHeld(true);
    };
    const up = (e: KeyboardEvent) => {
      if (e.key === "Alt") setAltHeld(false);
    };
    const blur = () => setAltHeld(false);
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", blur);
    };
  }, [suggestionId]);
  // Keyboard users get the peek through focus: tabbing into the island opens it.
  const [keyFocus, setKeyFocus] = useState(false);
  useEffect(() => {
    const into = (e: FocusEvent) =>
      setKeyFocus(
        e.target instanceof HTMLElement &&
          e.target.matches(":focus-visible") &&
          !e.target.matches("input, textarea, [contenteditable]"),
      );
    const out = () => setKeyFocus(false);
    window.addEventListener("focusin", into);
    window.addEventListener("focusout", out);
    return () => {
      window.removeEventListener("focusin", into);
      window.removeEventListener("focusout", out);
    };
  }, []);
  const suggestOpen = peek || pinned || altHeld || keyFocus;
  // Until onboarding is done, hover shows the steps being waited on, or else
  // brings the welcome back; the idle card ("watching for moments") would
  // just flash on the way.
  const guideOnHover = !!waiting && !waiting.background;
  // Pending actions take priority over resuming onboarding on hover.
  const needsAttention =
    !!suggestion ||
    questionId !== null ||
    merging ||
    updateProgress !== null ||
    mascot === "error" ||
    skillWorking !== null;
  const hovered = intent && !quiet && (settings.onboarded || guideOnHover || needsAttention);
  const resumeWelcome =
    intent &&
    !quiet &&
    !settings.onboarded &&
    !asking &&
    !guideOnHover &&
    !needsAttention &&
    !justDone &&
    mascot !== "success";
  useEffect(() => {
    if (resumeWelcome) void api.askResumeWelcome();
  }, [resumeWelcome]);
  const preparingVoice =
    !settings.onboarded &&
    !asking &&
    !needsAttention &&
    !waiting &&
    !(voiceStatus?.models.some((m) => m.id === "voice" && m.installed) ?? false);
  // A guide stays open while Duskie waits on something you finish elsewhere.
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
    waiting?: boolean;
    failed?: boolean;
    dots?: string[];
  } | null = asking
    ? null
    : donePill !== null
      ? { text: donePill, thinking: false, done: true }
      : voiceQuestion !== null
        ? { text: voiceQuestion, thinking: true }
        : hearing !== null || mascot === "listening"
          ? { text: hearing ?? "", thinking: false }
          : live !== null
            ? { text: live.caption || (live.tool ?? ""), thinking: !live.caption }
            : working !== null
              ? {
                  text: working,
                  thinking: !asksYou && !activityWaiting && (!displayedActivity || !terminal(displayedActivity)),
                  working: true,
                  asks: asksYou,
                  waiting: !!activityWaiting,
                  done: displayedActivity?.phase === "completed",
                  failed: displayedActivity?.phase === "failed",
                  dots: agentDots,
                }
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
  // Suggestions stay normal during thinking, do not gate them on !voicePill.
  const askOpens =
    questionId !== null && !dndOn && !fullscreen && !paused && settings.onboarded && agentsOnly && !quiet;
  const expanded =
    asking ||
    askOpens ||
    fullVoice ||
    preparingVoice ||
    (hovered && (!voicePill || agentsOnly || updateWorking !== null || liveActivities.length > 0)) ||
    guiding ||
    (OPEN_STATES.has(mascot) && !voicePill && mascot === "suggesting") ||
    successCard ||
    (!!suggestion && suggestOpen) ||
    (netShown && !voicePill) ||
    updateProgress?.phase === "failed" ||
    merging;
  // At rest only the sphere shows. The shell keeps its size (so hover and the
  // orb position do not move) but loses its background.
  const bare =
    !expanded && !chatting && !waiting && !voiceBusy && online && (mascot === "idle" || mascot === "sleeping");
  const busy = chatting || preparingVoice || voiceBusy || mascot === "noticing" || mascot === "working";

  // Hover while Thinking or Working: open Ask so the island is usable, not a dead pill.
  useEffect(() => {
    if (!intent || quiet || asking || !settings.onboarded || updateWorking !== null || suggestion) return;
    if (voiceQuestion === null && liveActivities.length > 0) return;
    if (voiceQuestion === null && working === null) return;
    if (agentsOnly) return;
    void api.askOpen();
  }, [
    intent,
    quiet,
    asking,
    settings.onboarded,
    voiceQuestion,
    working,
    agentsOnly,
    updateWorking,
    suggestion,
    liveActivities.length,
  ]);

  const bump = useMotionValue(1);
  // A mood (thanks, Claude finished) shows for a moment; offline and at
  // rest, the mascot looks a little lost.
  const mood = useDuskie((s) => s.mood);
  const speaking = useDuskie((s) => s.speaking);
  const face =
    mood?.id ??
    (asksYou && !voiceQuestion && hearing === null ? "curious" : null) ??
    (focusLeft !== null && !voiceQuestion && hearing === null && mascot !== "listening" ? "focus" : null) ??
    (paused && !voiceQuestion && hearing === null ? "sleepy" : null) ??
    (speaking && mascot !== "listening"
      ? "speak"
      : !online && (mascot === "idle" || mascot === "sleeping")
        ? "oops"
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
  const turnsLen = useDuskie((s) => s.turns.length);
  const chatId = useDuskie((s) => s.chatId);
  // biome-ignore lint/correctness/useExhaustiveDependencies: triggers only, remeasure when Agents body changes size
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
  }, [asking, view, turnsLen, chatId, askTab, agentCurrent, agentsLayout]);

  const showGuide =
    !!waiting &&
    !waiting.background &&
    !suggestion &&
    questionId === null &&
    !updateProgress &&
    (mascot === "idle" || mascot === "sleeping");
  const pillWidth = voicePill
    ? voiceShellWidth(voicePill.text, voicePill.thinking, {
        status: !!(
          voicePill.working ||
          voicePill.done ||
          voicePill.waiting ||
          voicePill.failed ||
          voicePill.focus ||
          voicePill.asks
        ),
        cancel: agentsOnly || !!voicePill.focus,
        indicator: !!(
          voicePill.working ||
          voicePill.thinking ||
          voicePill.done ||
          voicePill.waiting ||
          voicePill.failed ||
          voicePill.asks ||
          (voicePill.dots && voicePill.dots.length > 0)
        ),
      })
    : waiting
      ? COMPACT.waitWidth
      : busy
        ? COMPACT.busyWidth
        : !online
          ? COMPACT.offlineWidth
          : COMPACT.width;
  // The bounce is for opening only; once open, size changes are calm.
  const [settled, setSettled] = useState(false);
  useEffect(() => {
    if (!expanded && !asking) {
      setSettled(false);
      return;
    }
    const id = setTimeout(() => setSettled(true), 600);
    return () => clearTimeout(id);
  }, [expanded, asking]);

  const lastError = useDuskie((s) => (s.lastResult && !s.lastResult.ok ? s.lastResult.message : null));
  const peeking: Suggestion | null = suggestion && !suggestOpen ? suggestion : null;
  const pillModel: PillModel | null =
    !expanded && !asking
      ? peeking
        ? {
            kind: "suggest",
            // Compact (lower priority): the source or title only, no chip.
            ...(peeking.compact
              ? { label: sourceApp(peeking) ?? peeking.title }
              : sourceApp(peeking)
                ? { label: sourceApp(peeking) as string, dim: peeking.title }
                : { label: peeking.title }),
            ...(peeking.compact
              ? {}
              : { chip: peeking.options[0], onChip: () => void api.suggestionChoose(peeking.id, 0) }),
          }
        : hearing !== null || mascot === "listening"
          ? { kind: "listening", label: hearing?.trim() ? hearing : "", dim: hearing?.trim() ? undefined : "Listening" }
          : live !== null
            ? {
                kind: "caption",
                label: live.caption || (live.tool ?? "Thinking"),
                chip: live.caption && live.tool ? live.tool : undefined,
                onChip: stopLive,
              }
            : mascot === "error"
              ? { kind: "error", label: shortError(lastError) }
              : donePill !== null
                ? { kind: "done", label: donePill }
                : asksYou && askingTitle && voiceQuestion === null
                  ? {
                      kind: "waiting",
                      label: askingTitle,
                      dim: "waiting for you",
                      count: askingCount,
                      onOpen: () => void api.askOpen(),
                    }
                  : working !== null || voiceQuestion !== null
                    ? {
                        kind: "working",
                        label: displayedActivity
                          ? activityLabel(displayedActivity, activityNow, { elapsed: false }).replace(/(\.\.\.|…)$/, "")
                          : (voiceQuestion ?? working ?? "").replace(/(\.\.\.|…)$/, ""),
                        startedAt: displayedActivity?.startedAt,
                      }
                    : focusLeft !== null
                      ? {
                          kind: "focus",
                          label: "Focus",
                          dim: focusUntil ? `until ${clockTime(focusUntil)}` : undefined,
                          left: focusLeft,
                        }
                      : meetingSoon && !paused
                        ? {
                            kind: "notify",
                            label: meetingSoon.title,
                            dim: meetingSoon.mins < 1 ? "starting now" : `starts in ${meetingSoon.mins} minutes`,
                            chip: meetingSoon.join ? "Join the call" : undefined,
                            onChip: () => meetingSoon.join && void api.openReference("page", meetingSoon.join),
                          }
                        : later > 0 && !paused
                          ? {
                              kind: "waiting",
                              label: later === 1 ? "1 suggestion" : `${later} suggestions`,
                              dim: "held while you were busy",
                              count: later,
                            }
                          : paused
                            ? { kind: "paused", label: "Paused", dim: pauseDim(settings.pause) }
                            : !online
                              ? {
                                  kind: "offline",
                                  label: "Offline",
                                  dim: "catching up when back",
                                  onOpen: () => setNetOpen(true),
                                }
                              : mascot === "noticing"
                                ? { kind: "noticing", label: "Taking a look" }
                                : null
      : null;
  const [pillMeasured, setPillMeasured] = useState(0);

  const registryId = resolveIslandState({
    asking,
    askTab: view === "settings" ? "settings" : asking ? askTab : null,
    settingsView: view === "settings",
    welcome: view === "welcome",
    guiding,
    expanded,
    bare,
    fullVoice,
    voicePill: voicePill !== null,
    suggestion: !!suggestion && suggestOpen,
    suggestPeek: !!suggestion && !suggestOpen,
    waitingForYou: !!waiting && !waiting.minimized && !guiding,
    merging,
    updateCard: updateProgress?.phase === "failed" || (hovered && updateWorking !== null),
    offline: !online,
    netCard: netShown && !voicePill,
    notifyCard: hovered && meetingSoon !== null && !paused,
    agentCard: hovered && agentsOnly && cardAgent !== null,
    focus: focusLeft !== null,
    paused,
    notifyFresh: !!notifyFresh && !busy && !expanded,
    unread: later,
    activitiesOpen: expanded && liveActivities.length > 0 && !suggestion,
    mascot,
    quietGlance:
      expanded &&
      !suggestion &&
      !merging &&
      questionId === null &&
      liveActivities.length === 0 &&
      (mascot === "idle" || mascot === "sleeping"),
  });
  const layout = shellGeom({
    id: registryId,
    contentHeight,
    pillWidth: pillModel && pillMeasured ? pillMeasured : pillWidth,
    askTab,
    agentsWide: agentCurrent !== null,
    boardWide: agentsLayout === "board" && wideScreen(),
    settingsHeight: view === "settings" ? Math.max(460, contentHeight + 18) : undefined,
    reduced,
    settled: settled || (!expanded && !asking),
  });
  const width = layout.geom.width;
  const height = layout.geom.height;
  const radius = layout.geom.radius;
  const transition = layout.transition;

  const overlayHit = useDuskie((s) => s.overlayHit);
  // Report the target shape as the interactive area; outside it the window
  // stays click-through. A floating menu expands the rect so it stays usable.
  // Debounced: during spring animations width/height change every frame, so
  // we wait one frame before sending across the IPC boundary.
  const hitRectTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => {
    clearTimeout(hitRectTimer.current);
    hitRectTimer.current = setTimeout(() => {
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
    }, 16);
    return () => clearTimeout(hitRectTimer.current);
  }, [width, height, overlayHit]);

  // A small squish when something gets the mascot's attention while compact.
  useEffect(() => {
    if (mascot !== "noticing" || reduced) return;
    void animate(bump, [1, 1.06, 1], { duration: 0.42, ease: [0.23, 1, 0.32, 1] });
  }, [mascot, reduced, bump]);

  // Number keys must not pick a suggestion while the user is typing.
  useSuggestionKeys(asking ? null : suggestion);
  // WebView chrome: Ctrl+J is Downloads, not a Duskie feature.
  useBlockBrowserKeys();

  if (!ready) return null;

  const mascotX = layout.geom.mascotX;
  const mascotY = layout.geom.mascotY;
  const mascotSize = layout.geom.mascotSize;
  // Size stays 48; Take A / Ask scale via CSS (prototype orbAt). perch is on geom.
  const mascotScale = layout.geom.mascotScale;
  // Hidden for a fullscreen app, the island still comes back under the cursor.
  const shown = visible || rawHover;
  const shellColor = settings.islandColor;
  const washEvent =
    mascot === "success" || donePill || mascot === "error"
      ? mascot === "error"
        ? ("error" as const)
        : ("success" as const)
      : null;
  const agentBubble: BubbleItem[] =
    splitAgent && working !== agentWorking
      ? [
          {
            id: `agent-${splitAgent.id}`,
            label: splitAgent.total > 1 ? `${splitAgent.done + 1} of ${splitAgent.total}` : splitAgent.agent,
            ring: { mark: splitAgent.mark, done: splitAgent.done, total: splitAgent.total },
          },
        ]
      : [];
  const bubbleItems = [
    ...agentBubble,
    ...(busy && suggestion
      ? [{ id: suggestion.id, label: suggestion.title || "Suggestion" }]
      : busy
        ? liveActivities.slice(1, 5).map((a) => ({ id: a.id, label: a.label || a.source }))
        : []),
  ];

  return (
    <motion.div
      className="flex h-screen w-screen justify-center select-none"
      style={{ paddingTop: TOP, originY: 0 }}
      initial={false}
      animate={shown ? { opacity: 1, scale: 1, filter: "blur(0px)" } : { opacity: 0, scale: 0.92, filter: "blur(4px)" }}
      transition={reduced ? { duration: 0 } : { duration: shown ? 0.32 : 0.2, ease: [0.23, 1, 0.32, 1] }}
    >
      <div className="relative">
        <motion.div
          className="island-shell relative overflow-hidden text-white"
          data-bare={layout.geom.bare}
          data-perch={layout.geom.perch || undefined}
          data-color={shellColor}
          data-state={registryId}
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
          onPointerDownCapture={() => {
            if (suggestion && peek) setPinned(true);
          }}
          onPointerEnter={() => setHovered(true)}
          onPointerLeave={() => setHovered(false)}
        >
          <div
            className="isl-tint"
            aria-hidden
            style={{ "--tint": tintRgb(washEvent ?? layout.geom.wash) } as CSSProperties}
          />
          <Announcer registryId={registryId} />

          <AnimatePresence initial={false}>
            {pillModel && <IslandPill key={`pill-${pillModel.kind}`} model={pillModel} onWidth={setPillMeasured} />}
            {!pillModel && !expanded && voicePill && (
              <VoicePill
                key="voice"
                {...voicePill}
                activity={displayedActivity}
                onOpen={agentsOnly ? () => openBoard() : undefined}
                onCancel={
                  agentsOnly
                    ? () => {
                        for (const s of useAgents.getState().sessions) {
                          if (s.status === "working" || s.status === "waiting") interruptSession(s.id);
                        }
                      }
                    : undefined
                }
              />
            )}
            {!pillModel && !expanded && !bare && !voicePill && (
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
            {/* Exclusive branches only (as on main): keeping Ask mounted while closed
              stacked with Expanded and stuck the island after rapid open/close. */}
            {asking ? (
              <motion.div
                key="ask"
                ref={contentRef}
                className="absolute top-0 right-0"
                style={
                  askPanel
                    ? ({
                        // Design: only the tab row sits beside Duskie; the rest runs full width.
                        left: 12,
                        paddingTop: ASK_PAD.top,
                        paddingRight: ASK_PAD.x,
                        "--ak-indent": `${layout.geom.contentPadLeft - 12}px`,
                      } as CSSProperties)
                    : {
                        left: layout.geom.contentPadLeft,
                        paddingTop: EXPANDED.pad,
                        paddingRight: EXPANDED.pad,
                      }
                }
                initial={
                  reduced
                    ? { opacity: 0 }
                    : askPanel && askTab === "agents"
                      ? { opacity: 0, filter: "blur(8px)", y: 10, scale: 0.985 }
                      : { opacity: 0, y: 4 }
                }
                animate={{ opacity: 1, filter: "blur(0px)", y: 0, scale: 1 }}
                exit={{
                  opacity: 0,
                  filter: reduced ? undefined : "blur(4px)",
                  y: reduced ? 0 : -3,
                  transition: { duration: 0.1 },
                }}
                transition={
                  reduced
                    ? { duration: 0 }
                    : askPanel && askTab === "agents"
                      ? { duration: 0.34, delay: 0.05, ease: [0.23, 1, 0.32, 1] }
                      : { duration: 0.18, ease: [0.23, 1, 0.32, 1] }
                }
              >
                {view === "settings" ? <IslandSettings /> : view === "welcome" ? <IslandWelcome /> : <AskPanel />}
              </motion.div>
            ) : preparingVoice ? (
              <motion.div
                key="preparing"
                ref={contentRef}
                className="absolute top-0 right-0"
                style={{
                  left: layout.geom.contentPadLeft,
                  paddingTop: EXPANDED.pad,
                  paddingRight: EXPANDED.pad,
                }}
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
                      left: layout.geom.contentPadLeft,
                      paddingTop: EXPANDED.pad,
                      paddingRight: EXPANDED.pad,
                      // Lists below the orb (You missed) reach back under it.
                      "--orb-indent": `${layout.geom.mascotSize + 14}px`,
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
                  ) : registryId === "activities" ? (
                    <ActivitiesCard />
                  ) : registryId === "update" && updateWorking !== null ? (
                    <UpdateCard activityId={updateActivity?.id ?? null} startedAt={updateActivity?.startedAt ?? null} />
                  ) : registryId === "agent-card" && cardAgent ? (
                    <AgentCard id={cardAgent} />
                  ) : registryId === "notify-card" && meetingSoon ? (
                    <MeetingCard meeting={meetingSoon} />
                  ) : registryId === "net-card" ? (
                    <NetCard onClose={() => setNetOpen(false)} />
                  ) : (
                    <ExpandedContent mascot={mascot} paused={paused} suggestion={suggestion} />
                  )}
                </motion.div>
              )
            )}
          </AnimatePresence>
        </motion.div>
        {bubbleItems.length > 0 && <IslandBubbles items={bubbleItems} />}
        {/* Outside the shell, so its clipping never cuts Duskie's arms or hops. */}
        <motion.div
          className="pointer-events-none absolute top-0 left-0 z-2"
          initial={false}
          animate={{ x: mascotX, y: mascotY, scale: mascotScale }}
          transition={transition}
          style={{ originX: 0, originY: 0 }}
        >
          <span className="dk-orb" data-face={haloFace(mascot)}>
            <span className="dk-halo" />
            <Creature
              state={chatting && mascot === "idle" ? "working" : mascot}
              face={chatting && mascot === "idle" ? null : face}
              size={mascotSize}
              theme={settings.theme}
              alive={settings.alive && visible && !look.simple}
              simple={look.simple}
              active={shown}
              bust={false}
            />
          </span>
        </motion.div>
      </div>
    </motion.div>
  );
}

/** Newest words matter most while talking; keep the pill from growing forever. */

/** Wash palette colour as "r g b" for the tint gradient. */
function tintRgb(e: WashEvent): string {
  const h = PALETTES[e].a;
  return [1, 3, 5].map((i) => Number.parseInt(h.slice(i, i + 2), 16)).join(" ");
}

/** Halo colour and strength follow the mascot's mood. */
function haloFace(m: MascotState): string {
  if (m === "listening") return "listen";
  if (m === "success") return "happy";
  if (m === "error") return "sad";
  if (m === "working") return "work";
  if (m === "sleeping") return "sleep";
  return "idle";
}

/** First clause of an error, short enough for the compact line. */
function shortError(msg: string | null): string {
  const first = (msg ?? "").split(/[.\n]/)[0]?.trim() ?? "";
  if (!first) return "That did not work";
  return first.length > 42 ? `${first.slice(0, 41)}…` : first;
}

function clockTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

function pauseDim(p: Settings["pause"]): string | undefined {
  return p.kind === "until" ? `until ${clockTime(Date.parse(p.until))}` : undefined;
}

const APPS = [
  "Outlook",
  "Gmail",
  "Slack",
  "Teams",
  "Calendar",
  "Chrome",
  "Edge",
  "WhatsApp",
  "Discord",
  "GitHub",
  "Notion",
];

/** The app a suggestion came from, when its text names one. */
function sourceApp(s: { skillId: string; detail: string }): string | null {
  const hay = `${s.skillId} ${s.detail}`.toLowerCase();
  return APPS.find((a) => hay.includes(a.toLowerCase())) ?? null;
}

/** A meeting starting within ten minutes (or just started), for the notify pill. */
function soonMeeting(now: number, cal: CalendarToday | null | undefined) {
  for (const m of cal?.meetings ?? []) {
    const [h, mm] = m.start.split(":").map(Number);
    const d = new Date(now);
    d.setHours(h ?? 0, mm ?? 0, 0, 0);
    const mins = Math.round((d.getTime() - now) / 60_000);
    if (mins >= -2 && mins <= 10)
      return { title: m.title, mins: Math.max(0, mins), join: m.joinUrl, notes: m.notes ?? null };
  }
  return null;
}
