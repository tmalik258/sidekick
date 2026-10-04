"use client";

// What hovering the island shows when nothing is asking for attention: the
// next meeting or how the day is going, and suggestions you missed. Cached
// data shows at once and refreshes behind it.

import { type ReactNode, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { useNow } from "@/lib/hooks";
import { updateSettings, useSidekick } from "@/lib/store";
import { type AppTime, type CalendarToday, formatDuration, type LaterItem } from "@/lib/types";
import { Icon } from "./Icon";

/** A meeting this close (or already on) takes the headline. */
const MEETING_SOON_MIN = 60;
/** Missed items shown before "Show N more". */
const LATER_SHOWN = 3;

export function Glance({ paused }: { paused: boolean }) {
  const now = useNow(30_000);
  const { data: calendar } = useCached<CalendarToday>("calendar-today", api.calendarToday);
  const { data: time } = useCached<AppTime[]>("time-today", api.timeToday);
  const head = headline(now, paused, calendar, time);
  return (
    <div className="flex flex-col">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <p className="truncate font-display text-[15px] leading-5 font-semibold tracking-[-0.015em] text-white">
            {head.title}
          </p>
          <p className="mt-0.5 line-clamp-2 text-[13px] leading-4.5 tracking-[-0.005em] text-[rgb(235_235_245/0.6)] tabular-nums">
            {head.detail}
          </p>
        </div>
        <div className="flex shrink-0 gap-1.5">
          {head.join && (
            <RoundButton label="Join meeting" onClick={() => void api.openReference("page", head.join ?? "")}>
              <Icon name="play" size={14} />
            </RoundButton>
          )}
          <QuickActions paused={paused} />
        </div>
      </div>
      <FullscreenSwitch />
      <LaterList />
    </div>
  );
}

/** Only while a fullscreen app is in front: keep the island out of it. */
function FullscreenSwitch() {
  const fullscreen = useSidekick((s) => s.fullscreen);
  const hide = useSidekick((s) => s.settings.hideInFullscreen);
  if (!fullscreen) return null;
  return (
    <button
      type="button"
      role="switch"
      aria-checked={hide}
      onClick={() => void updateSettings({ hideInFullscreen: !hide })}
      style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)", width: "calc(100% + var(--orb-indent, 0px))" }}
      className="chip mt-3 flex items-center gap-3 rounded-xl bg-white/[0.07] px-3 py-2 text-left hover:bg-white/12"
    >
      <span className="min-w-0 flex-1">
        <span className="block text-[13px] font-medium text-white">Hide while fullscreen</span>
        <span className="block text-[12px] text-[rgb(235_235_245/0.55)]">
          {hide ? "Hidden; hover the top edge to bring it back" : "The island stays on top of this app"}
        </span>
      </span>
      <span
        aria-hidden="true"
        className={`relative h-[22px] w-9 shrink-0 rounded-full transition-colors duration-200 ${hide ? "bg-[#30d158]" : "bg-white/20"}`}
      >
        <span
          className={`absolute top-[2px] left-[2px] size-[18px] rounded-full bg-white shadow transition-transform duration-200 ease-out ${
            hide ? "translate-x-[14px]" : ""
          }`}
        />
      </span>
    </button>
  );
}

interface Headline {
  title: string;
  detail: string;
  join?: string | null;
}

function headline(now: number, paused: boolean, calendar: CalendarToday | null, time: AppTime[] | null): Headline {
  if (paused) return { title: "Paused", detail: "Sensors are off. Resume when you are ready." };
  const meeting = nextMeeting(now, calendar);
  if (meeting) return meeting;
  const hour = new Date(now).getHours();
  const title = hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";
  const total = (time ?? []).reduce((sum, t) => sum + t.secs, 0);
  if (total < 60) return { title, detail: "Nothing needs you right now." };
  const top = topFocus(time ?? []);
  return { title, detail: `${formatDuration(total)} at your PC today${top ? `, mostly ${top}` : ""}.` };
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
  const count = useSidekick((s) => s.later);
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
    <div className="mt-3 flex flex-col gap-1.5" style={{ marginLeft: "calc(var(--orb-indent, 0px) * -1)" }}>
      <div className="flex items-center justify-between text-[12px] text-[rgb(235_235_245/0.6)]">
        <span>{missed ? "You missed" : "Saved for later"}</span>
        <span className="flex items-center gap-3">
          {all && (
            <button type="button" onClick={() => setAll(false)} className="chip hover:text-white">
              Show less
            </button>
          )}
          <button type="button" onClick={() => void api.laterClear()} className="chip hover:text-white">
            Clear
          </button>
        </span>
      </div>
      <div className={`flex flex-col gap-1.5 ${all ? "max-h-[260px] overflow-y-auto overscroll-contain" : ""}`}>
        {shown.map((l, n) => (
          <button
            key={l.id}
            type="button"
            style={{ animationDelay: `${Math.min(n, 6) * 40}ms` }}
            onClick={() => void api.laterOpen(l.id)}
            className="chip rise-in flex w-full shrink-0 items-center gap-3 rounded-xl bg-white/[0.07] px-3 py-1.5 text-left hover:bg-white/12"
          >
            <span className="min-w-0 flex-1 overflow-hidden">
              <span className="block truncate text-[13px] font-medium text-white">{l.title}</span>
              <span className="block truncate text-[12px] text-[rgb(235_235_245/0.55)]">{l.detail}</span>
            </span>
            <span className="shrink-0 text-[11px] text-white/40 tabular-nums">{ago(l.minutesAgo)}</span>
          </button>
        ))}
      </div>
      {!all && items.length > LATER_SHOWN && (
        <button
          type="button"
          onClick={() => setAll(true)}
          className="chip self-start text-[12px] text-white/50 hover:text-white"
        >
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

function QuickActions({ paused }: { paused: boolean }) {
  const hotkey = useSidekick((s) => s.settings.paletteHotkey);
  return (
    <>
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
      <UpdateButton />
    </>
  );
}

/** Only while a newer release is waiting: a small button with a dot. */
function UpdateButton() {
  const update = useSidekick((s) => s.update);
  const [busy, setBusy] = useState(false);
  if (!update) return null;
  const install = () => {
    setBusy(true);
    void api.updateInstall().catch(() => setBusy(false));
  };
  return (
    <span className="relative">
      <RoundButton label={busy ? "Opening the installer..." : `Update to ${update.version}`} onClick={install}>
        <Icon name="update" size={15} />
      </RoundButton>
      <span
        aria-hidden="true"
        className="pointer-events-none absolute top-0 right-0 size-2 rounded-full bg-[#0a84ff] ring-2 ring-black"
      />
    </span>
  );
}

export function RoundButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
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
