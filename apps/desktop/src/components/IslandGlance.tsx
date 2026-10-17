"use client";

// One hover glance: day head + actions, then optional sections (needs you,
// away, activity, later, tip). Cached data shows at once and refreshes behind.

import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { type ReactNode, useEffect, useState } from "react";
import { useActivities, visibleActivities } from "@/lib/activities";
import { setTab, useAgents } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import { shortcutBinding } from "@/lib/shortcuts";
import { installUpdate, updateSettings, useAssistantName, useDuskie } from "@/lib/store";
import { pickTip } from "@/lib/tips";
import { type AppTime, type CalendarToday, formatDuration, type LaterItem } from "@/lib/types";
import { updateBusy, updateLabel } from "@/lib/updates";
import { ActivityProgress } from "./ActivityProgress";
import { Ic, Icon } from "./Icon";
import { AwaySection, NeedsYouSection } from "./IslandAgents";
import { CardChips, CardHead, CardNote } from "./IslandCard";
import { Tip } from "./Tip";

/** Round quick-action button (porcelain chrome). */
const qbClass = "rd-qb chip relative grid size-8 place-items-center overflow-hidden";

/** A meeting this close (or already on) takes the headline. */
const MEETING_SOON_MIN = 60;
/** Missed items shown before "Show N more". */
const LATER_SHOWN = 3;

function openAgentsBoard() {
  useAgents.setState({ tab: "agents", layout: "board" });
  void api.askOpen();
}

function openHistory() {
  setTab("history");
  void api.askOpen();
}

/** Unified island hover: stacked sections, empty ones omit themselves. */
export function Glance({ paused }: { paused: boolean }) {
  const sessions = useAgents((s) => s.sessions);
  const agentsLive = sessions.some((s) => s.status === "working" || s.status === "waiting");
  const { data: crash, refresh: refreshCrash } = useCached<string | null>("crash", api.crashPending);
  const activityState = useActivities();
  const activityNow = useNow(1000);
  // Agent/chat rows belong in the agents list, not the flat activity line.
  const otherActivities = visibleActivities(
    [...activityState.activities, ...activityState.local],
    activityNow,
    true,
  ).filter((a) => !(agentsLive && (a.source === "agent" || a.source === "chat")));
  const now = useNow(30_000);
  const { data: calendar } = useCached<CalendarToday>("calendar-today", api.calendarToday);
  const { data: time } = useCached<AppTime[]>("time-today", api.timeToday);
  const head = headline(now, paused, calendar, time);
  const [focusOpen, setFocusOpen] = useState(false);
  const eyebrow = new Date(now).toLocaleDateString([], { weekday: "long", month: "long", day: "numeric" });
  return (
    <div className="rd-glance flex flex-col gap-2.5">
      {crash && <CrashCard note={crash} onDone={() => void refreshCrash()} />}
      <div className="rd-card-head">
        <p className="rd-eyebrow">{eyebrow}</p>
        <p className="rd-title">{head.title}</p>
        <p className="rd-detail">{head.detail}</p>
      </div>
      {focusOpen && <FocusPicker onDone={() => setFocusOpen(false)} />}
      <NeedsYouSection sessions={sessions} />
      <AwaySection />
      {otherActivities.length > 0 && (
        <div className="mt-3" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
          <ActivityProgress sources={["update", "browser", "file", "action", "open"]} />
        </div>
      )}
      <FullscreenSwitch />
      <LaterList />
      {head.quiet && <QuietTip />}
      <div className="rd-gfoot">
        {head.join ? (
          <button
            type="button"
            className="rd-gfoot-btn"
            onClick={() => void api.openReference("page", head.join ?? "")}
          >
            Join
          </button>
        ) : (
          <span />
        )}
        <QuickActions paused={paused} focusOpen={focusOpen} onFocusOpenChange={setFocusOpen} />
      </div>
    </div>
  );
}

/** About one hover in four, when nothing needs you: one short tip, never
 * the same one twice in a row. */
function QuietTip() {
  const on = useDuskie((s) => s.settings.tips);
  const later = useDuskie((s) => s.later);
  const [tip] = useState(pickTip);
  if (!on || later > 0 || !tip) return null;
  return (
    <p
      className="rd-list-row mt-1 flex items-baseline gap-2"
      style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}
    >
      <span className="rd-eyebrow shrink-0">Tip</span>
      {tip}
    </p>
  );
}

/** Only while a fullscreen app is in front: keep the island out of it. */
function FullscreenSwitch() {
  const fullscreen = useDuskie((s) => s.fullscreen);
  const hide = useDuskie((s) => s.settings.hideInFullscreen);
  if (!fullscreen) return null;
  return (
    <button
      type="button"
      role="switch"
      aria-checked={hide}
      onClick={() => void updateSettings({ hideInFullscreen: !hide })}
      style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)", width: "calc(100% + var(--orb-indent, 0px))" }}
      className="rd-list-row chip mt-1 flex items-center gap-3 text-left"
    >
      <span className="min-w-0 flex-1">
        <span className="block text-[13px] font-medium text-(--ink)">Hide while fullscreen</span>
        <span className="block text-[12px] text-(--ink-2)">
          {hide ? "Hidden; hover the top edge to bring it back" : "The island stays on top of this app"}
        </span>
      </span>
      <span
        aria-hidden="true"
        className="flex shrink-0 gap-0.5 rounded-[10px] bg-(--fill) p-[3px] text-[12.5px] leading-none shadow-[inset_0_0_0_1px_var(--line)]"
      >
        <span
          className={`rounded-[8px] px-2.5 py-[5px] ${hide ? "bg-(--c2) font-semibold text-(--c1)" : "text-(--ink-2)"}`}
        >
          On
        </span>
        <span
          className={`rounded-[8px] px-2.5 py-[5px] ${hide ? "text-(--ink-2)" : "bg-(--c2) font-semibold text-(--c1)"}`}
        >
          Off
        </span>
      </span>
    </button>
  );
}

interface Headline {
  title: string;
  detail: string;
  /** Nothing pressing: a tip may show. */
  quiet?: boolean;
  join?: string | null;
}

function headline(now: number, paused: boolean, calendar: CalendarToday | null, time: AppTime[] | null): Headline {
  if (paused) return { title: "Paused", detail: "Sensors are off. Resume when you are ready." };
  const meeting = nextMeeting(now, calendar);
  if (meeting) return meeting;
  const hour = new Date(now).getHours();
  const title = hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";
  const total = (time ?? []).reduce((sum, t) => sum + t.secs, 0);
  if (total < 60) return { title, detail: "Nothing needs you right now.", quiet: true };
  const top = topFocus(time ?? []);
  return { title, detail: `${formatDuration(total)} at your PC today${top ? `, mostly ${top}` : ""}.`, quiet: true };
}

/** Today's meetings come as local "HH:MM". */
function at(now: number, hhmm: string): number {
  const [h, m] = hhmm.split(":").map(Number);
  const d = new Date(now);
  d.setHours(h ?? 0, m ?? 0, 0, 0);
  return d.getTime();
}

function nextMeeting(now: number, calendar: CalendarToday | null): Headline | null {
  for (const m of calendar?.meetings ?? []) {
    const start = at(now, m.start);
    const end = at(now, m.end);
    if (end <= now) continue;
    if (start <= now) return { title: `${m.title} is on now`, detail: `Until ${m.end}.`, join: m.joinUrl };
    const mins = Math.round((start - now) / 60_000);
    if (mins > MEETING_SOON_MIN) return null;
    return {
      title: mins < 1 ? `${m.title} is starting` : `${m.title} in ${mins} min`,
      detail: `${m.start} to ${m.end}.`,
      join: m.joinUrl,
    };
  }
  return null;
}

/** Where most of today went: a project when known, else the app. */
function topFocus(time: AppTime[]): string | null {
  const by = new Map<string, number>();
  for (const t of time) {
    const key = t.project ? `on ${t.project}` : `in ${t.app}`;
    by.set(key, (by.get(key) ?? 0) + t.secs);
  }
  let best: [string, number] | null = null;
  for (const e of by) if (!best || e[1] > best[1]) best = e;
  return best?.[0] ?? null;
}

/** Suggestions you missed (shown and timed out) or that waited quietly. */
function LaterList() {
  const count = useDuskie((s) => s.later);
  const { data, refresh } = useCached<LaterItem[]>("later-list", api.laterList);
  const [all, setAll] = useState(false);
  // The count changes when one is added, opened or cleared.
  useEffect(() => {
    if (count > 0) void refresh().catch(() => undefined);
  }, [count, refresh]);
  const items = count > 0 ? (data ?? []) : [];
  if (items.length === 0) return null;
  const missed = items.some((l) => l.missed);
  const shown = all ? items : items.slice(0, LATER_SHOWN);
  return (
    <div
      className="mt-3 flex flex-col gap-1.5"
      style={{
        marginLeft: "calc(var(--orb-indent, 0px) * -1)",
        width: "calc(100% + var(--orb-indent, 0px))",
      }}
    >
      <div className="flex items-center justify-between text-[12px] text-(--ink-2)">
        <span>{missed ? "You missed" : "Saved for later"}</span>
        <span className="flex items-center gap-3">
          {all && (
            <button type="button" onClick={() => setAll(false)} className="chip rd-gfoot-btn">
              Show less
            </button>
          )}
          <button type="button" onClick={() => void api.laterClear()} className="chip rd-gfoot-btn">
            Clear
          </button>
        </span>
      </div>
      <div
        className={`flex flex-col gap-1.5 ${all ? "island-scroll max-h-[260px] overflow-y-auto overscroll-contain" : ""}`}
      >
        {shown.map((l, n) => (
          // biome-ignore lint/a11y/useSemanticElements: the row holds its own dismiss button, so it cannot be a button
          <div
            key={l.id}
            role="button"
            tabIndex={0}
            style={{ animationDelay: `${Math.min(n, 6) * 40}ms` }}
            className="rise-in rd-list-row chip"
            onClick={() => void api.laterOpen(l.id)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void api.laterOpen(l.id);
            }}
          >
            <span className="dot" style={{ background: "#95a89a" }} />
            <span className="t">
              {l.title}
              {l.detail && <span className="text-(--ink-3)"> {l.detail}</span>}
            </span>
            <span className="r">{ago(l.minutesAgo)}</span>
            <button
              type="button"
              className="rx"
              aria-label={`Clear ${l.title}`}
              onClick={(e) => {
                e.stopPropagation();
                void api.laterDismiss(l.id);
              }}
            >
              <Ic name="x" size={12} />
            </button>
          </div>
        ))}
      </div>
      {!all && items.length > LATER_SHOWN && (
        <button type="button" onClick={() => setAll(true)} className="chip rd-gfoot-btn self-start text-[12px]">
          Show {items.length - LATER_SHOWN} more
        </button>
      )}
    </div>
  );
}

function ago(mins: number): string {
  if (mins < 1) return "now";
  if (mins < 60) return `${mins} min`;
  return `${Math.floor(mins / 60)} h`;
}

function QuickActions({
  paused,
  focusOpen,
  onFocusOpenChange,
}: {
  paused: boolean;
  focusOpen: boolean;
  onFocusOpenChange: (open: boolean) => void;
}) {
  const update = useDuskie((s) => s.update);
  const shift = useShiftHeld();
  // Single row keeps the glance short, two rows stacked taller than the copy.
  return (
    <div className="flex gap-1.5">
      <AskShiftAgentsButton shift={shift} />
      <FocusShiftPauseButton paused={paused} open={focusOpen} onOpenChange={onFocusOpenChange} shift={shift} />
      {update ? <UpdateButton /> : null}
      <SettingsShiftHistoryButton shift={shift} />
    </div>
  );
}

/** Shift for glance morphs. The shell stays unfocused, so keydown never
 * arrives, Rust polls while hovered (and Shift+Alt for suggestions); pointer
 * events back it up when the cursor is over the webview. */
function useShiftHeld(): boolean {
  const fromRust = useDuskie((s) => s.shiftHeld);
  const [fromPointer, setFromPointer] = useState(false);
  useEffect(() => {
    const sync = (e: PointerEvent | KeyboardEvent) => setFromPointer(e.shiftKey);
    const clear = () => setFromPointer(false);
    window.addEventListener("pointermove", sync);
    window.addEventListener("pointerdown", sync);
    window.addEventListener("pointerover", sync);
    window.addEventListener("keydown", sync);
    window.addEventListener("keyup", sync);
    window.addEventListener("blur", clear);
    return () => {
      window.removeEventListener("pointermove", sync);
      window.removeEventListener("pointerdown", sync);
      window.removeEventListener("pointerover", sync);
      window.removeEventListener("keydown", sync);
      window.removeEventListener("keyup", sync);
      window.removeEventListener("blur", clear);
    };
  }, []);
  return fromRust || fromPointer;
}

function keyHint(keys: string | undefined): string {
  const k = keys ? shortcutBinding(keys) : "";
  return k ? ` (${k})` : "";
}

/** Settings by default; hold Shift to morph into History (one slot, animated icon). */
function SettingsShiftHistoryButton({ shift }: { shift: boolean }) {
  const reduced = useReducedMotion() ?? false;
  const history = shift;
  const label = history ? "History" : "Settings · Shift for History";
  return (
    <Tip label={label}>
      <button
        type="button"
        aria-label={label}
        onClick={(e) => {
          if (e.shiftKey || history) openHistory();
          else void api.openSettings();
        }}
        className={qbClass}
      >
        <AnimatePresence mode="popLayout" initial={false}>
          <motion.span
            key={history ? "history" : "settings"}
            className="grid place-items-center"
            initial={reduced ? false : { opacity: 0, scale: 0.7, rotate: history ? -20 : 20 }}
            animate={{ opacity: 1, scale: 1, rotate: 0 }}
            exit={reduced ? undefined : { opacity: 0, scale: 0.7, rotate: history ? 20 : -20 }}
            transition={{ duration: 0.16, ease: [0.23, 1, 0.32, 1] }}
          >
            <Ic name={history ? "clock" : "gear"} size={15} />
          </motion.span>
        </AnimatePresence>
      </button>
    </Tip>
  );
}

/** Ask by default; hold Shift to morph into Agents (one slot, animated icon). */
function AskShiftAgentsButton({ shift }: { shift: boolean }) {
  const askKeys = shortcutBinding(useDuskie((s) => s.settings.paletteHotkey));
  const agentsKeys = shortcutBinding(useDuskie((s) => s.settings.shortcuts.agents ?? ""));
  const name = useAssistantName();
  const reduced = useReducedMotion() ?? false;
  const agents = shift;
  const label = agents
    ? `Agents${keyHint(agentsKeys)}`
    : `Ask ${name}${keyHint(askKeys)} · Shift for Agents${keyHint(agentsKeys)}`;
  return (
    <Tip label={label}>
      <button
        type="button"
        aria-label={label}
        onClick={(e) => {
          if (e.shiftKey || agents) openAgentsBoard();
          else void api.askOpen();
        }}
        className={qbClass}
      >
        <AnimatePresence mode="popLayout" initial={false}>
          <motion.span
            key={agents ? "agents" : "ask"}
            className="grid place-items-center"
            initial={reduced ? false : { opacity: 0, scale: 0.7, rotate: agents ? -20 : 20 }}
            animate={{ opacity: 1, scale: 1, rotate: 0 }}
            exit={reduced ? undefined : { opacity: 0, scale: 0.7, rotate: agents ? 20 : -20 }}
            transition={{ duration: 0.16, ease: [0.23, 1, 0.32, 1] }}
          >
            <Ic name={agents ? "term" : "ask"} size={15} />
          </motion.span>
        </AnimatePresence>
      </button>
    </Tip>
  );
}

const FOCUS_PRESETS = [15, 25, 45, 60, 90, 120] as const;
const FOCUS_MIN = 5;
const FOCUS_MAX = 180;
const FOCUS_LAST_KEY = "sidekick.focusMinutes";

function readLastFocus(): number {
  try {
    const n = Number(localStorage.getItem(FOCUS_LAST_KEY));
    if (Number.isFinite(n) && n >= FOCUS_MIN && n <= FOCUS_MAX) return Math.round(n);
  } catch {
    // Default below.
  }
  return 25;
}

function writeLastFocus(minutes: number) {
  try {
    localStorage.setItem(FOCUS_LAST_KEY, String(minutes));
  } catch {
    // Preference is optional.
  }
}

/** Focus by default; hold Shift to morph into Pause / Resume (one slot). When
 * sensors are already paused, Resume is primary and Shift reveals Focus. */
function FocusShiftPauseButton({
  paused,
  open,
  onOpenChange,
  shift,
}: {
  paused: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  shift: boolean;
}) {
  const focusing = useDuskie((s) => s.focusUntil !== null && s.focusUntil > Date.now());
  const focusKeys = useDuskie((s) => s.settings.shortcuts.focus);
  const pauseKeys = useDuskie((s) => s.settings.shortcuts.pause);
  const reduced = useReducedMotion() ?? false;
  // Paused → Resume is primary; otherwise Focus is primary and Shift → Pause.
  const pauseSide = paused ? !shift : shift;
  const focusHint = keyHint(focusKeys);
  const pauseHint = keyHint(pauseKeys);
  const label = pauseSide
    ? paused
      ? `Resume${pauseHint} · Shift for Focus${focusHint}`
      : `Pause 15 minutes${pauseHint}`
    : focusing
      ? `End focus${focusHint}`
      : open
        ? "Close"
        : paused
          ? `Focus${focusHint}`
          : `Focus${focusHint} · Shift for Pause${pauseHint}`;
  const active = !pauseSide && (open || focusing);
  const icon = pauseSide ? (paused ? "play" : "pause") : "focus";
  return (
    <Tip label={label}>
      <button
        type="button"
        aria-label={label}
        aria-expanded={pauseSide ? undefined : open}
        onClick={(e) => {
          const wantPause = e.shiftKey ? !paused : pauseSide;
          if (wantPause) {
            onOpenChange(false);
            void (paused ? api.sensorsResume() : api.sensorsPause(15));
            return;
          }
          if (focusing) {
            onOpenChange(false);
            void api.focusStop();
            return;
          }
          onOpenChange(!open);
        }}
        className={`${qbClass}${active ? " rd-qb-on" : ""}`}
      >
        <AnimatePresence mode="popLayout" initial={false}>
          <motion.span
            key={icon}
            className="grid place-items-center"
            initial={reduced ? false : { opacity: 0, scale: 0.7, rotate: pauseSide ? -20 : 20 }}
            animate={{ opacity: 1, scale: 1, rotate: 0 }}
            exit={reduced ? undefined : { opacity: 0, scale: 0.7, rotate: pauseSide ? 20 : -20 }}
            transition={{ duration: 0.16, ease: [0.23, 1, 0.32, 1] }}
          >
            <Icon name={icon} size={pauseSide ? 14 : 15} />
          </motion.span>
        </AnimatePresence>
      </button>
    </Tip>
  );
}

/** Choose how long, then start, presets plus a quiet custom stepper. */
function FocusPicker({ onDone }: { onDone: () => void }) {
  const [minutes, setMinutes] = useState(readLastFocus);
  const start = (n: number) => {
    const next = Math.min(FOCUS_MAX, Math.max(FOCUS_MIN, Math.round(n)));
    writeLastFocus(next);
    void api.focusStart(next);
    onDone();
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onDone();
      }
      if (e.key === "Enter") {
        e.preventDefault();
        start(minutes);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  const nudge = (delta: number) => setMinutes((m) => Math.min(FOCUS_MAX, Math.max(FOCUS_MIN, m + delta)));
  return (
    <div
      className="rise-in mt-3 rounded-[12px] bg-(--fill-2) p-3 shadow-[inset_0_0_0_1px_var(--line)]"
      style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="font-display text-[14px] leading-5 font-semibold tracking-[-0.015em] text-(--ink)">Focus</p>
          <p className="mt-0.5 text-[12px] leading-4 text-(--ink-2)">Suggestions and alerts wait until you are done.</p>
        </div>
        <div className="flex shrink-0 items-center gap-1 rounded-[10px] bg-(--fill) px-1 py-0.5 shadow-[inset_0_0_0_1px_var(--line)]">
          <button
            type="button"
            aria-label="Five minutes less"
            onClick={() => nudge(-5)}
            className="chip grid size-7 place-items-center rounded-[8px] text-(--ink-2) hover:bg-(--fill-2) hover:text-(--ink)"
          >
            −
          </button>
          <span className="min-w-[4.5rem] text-center font-display text-[15px] font-semibold tracking-[-0.02em] text-(--ink) tabular-nums">
            {minutes}
            <span className="ml-1 text-[11px] font-medium text-(--ink-2)">min</span>
          </span>
          <button
            type="button"
            aria-label="Five minutes more"
            onClick={() => nudge(5)}
            className="chip grid size-7 place-items-center rounded-[8px] text-(--ink-2) hover:bg-(--fill-2) hover:text-(--ink)"
          >
            +
          </button>
        </div>
      </div>
      <div className="mt-3 flex flex-wrap gap-1.5">
        {FOCUS_PRESETS.map((n) => {
          const on = minutes === n;
          return (
            <button
              key={n}
              type="button"
              onClick={() => setMinutes(n)}
              className={`chip min-h-8 rounded-[10px] px-3 text-[12.5px] font-medium tracking-[-0.01em] tabular-nums transition-[background,color,transform] duration-150 ${
                on ? "bg-(--c2) text-(--c1)" : "bg-(--fill) text-(--ink-2) hover:bg-(--fill-2) hover:text-(--ink)"
              }`}
            >
              {n}m
            </button>
          );
        })}
      </div>
      <div className="mt-3 flex items-center gap-2">
        <button
          type="button"
          onClick={() => start(minutes)}
          className="chip flex min-h-9 flex-1 items-center justify-center gap-2 rounded-[10px] bg-(--c2) px-4 text-[13px] font-semibold tracking-[-0.01em] text-(--c1) hover:opacity-92 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-(--c4)"
        >
          <Ic name="focus" size={14} />
          Start {minutes} minutes
          <kbd className="ml-0.5 font-sans text-[11px] font-medium text-(--c1)/40">Enter</kbd>
        </button>
        <button
          type="button"
          onClick={onDone}
          className="chip shrink-0 rounded-[10px] px-3 py-2 text-[12.5px] text-(--ink-2) hover:bg-(--fill) hover:text-(--ink)"
        >
          Not now
        </button>
      </div>
    </div>
  );
}

/** Only while a newer release is waiting: a small button with a dot. */
function UpdateButton() {
  const update = useDuskie((s) => s.update);
  const progress = useDuskie((s) => s.updateProgress);
  const busy = updateBusy(progress);
  if (!update) return null;
  const install = () => {
    void installUpdate().catch(() => undefined);
  };
  return (
    <span className="relative">
      <RoundButton
        label={progress ? updateLabel(progress) : `Update to ${update.version}`}
        onClick={install}
        disabled={busy}
      >
        {busy ? (
          <span
            aria-hidden="true"
            className="size-4 animate-spin rounded-full border-2 border-white/25 border-t-white motion-reduce:animate-none"
          />
        ) : (
          <Ic name="update" size={15} />
        )}
      </RoundButton>
      <span
        aria-hidden="true"
        className="pointer-events-none absolute top-0 right-0 size-2 rounded-full bg-(--c3,#0a84ff) ring-2 ring-(--c1,#000)"
      />
    </span>
  );
}

export function RoundButton({
  label,
  onClick,
  children,
  disabled = false,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
  disabled?: boolean;
}) {
  return (
    <Tip label={label}>
      <button
        type="button"
        aria-label={label}
        disabled={disabled}
        onClick={onClick}
        className={`${qbClass} disabled:cursor-wait disabled:opacity-60`}
      >
        {children}
      </button>
    </Tip>
  );
}

const ISSUES = "https://github.com/tmalik258/sidekick/issues/new";

/** After a crash, for people who opted in: the report, and sending it is
 * their choice (a GitHub issue they can read and edit first). */
function CrashCard({ note, onDone }: { note: string; onDone: () => void }) {
  const dismiss = () => void api.crashDismiss().then(onDone);
  const report = () =>
    void api.appInfo().then((info) => {
      const body = `Duskie ${info.version} closed unexpectedly.\n\n\`\`\`\n${note}\n\`\`\`\n\nWhat I was doing:\n`;
      const url = `${ISSUES}?title=${encodeURIComponent("Crash report")}&body=${encodeURIComponent(body)}`;
      void api.aiOpenLink(url).then(dismiss);
    });
  return (
    <div className="flex flex-col">
      <CardHead
        title="Duskie closed unexpectedly"
        detail="The report has the version and the error, nothing else. You read it before it is sent."
      />
      <CardNote>
        <span className="line-clamp-3">{note}</span>
      </CardNote>
      <CardChips options={[{ label: "Report it", run: report }]} quiet={{ label: "Ignore", run: dismiss }} />
    </div>
  );
}
