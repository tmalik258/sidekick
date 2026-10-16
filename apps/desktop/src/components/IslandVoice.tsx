"use client";

import { motion } from "motion/react";
import { useState } from "react";
import { acceptActivities, activityElapsedSecs, activityLabel, dismissLocal } from "@/lib/activities";
import { useAgents } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useNow } from "@/lib/hooks";
import { REDESIGN_COMPACT } from "@/lib/redesign-layout";
import { useDuskie } from "@/lib/store";
import type { Activity as ActivityItem } from "@/lib/types";
import { VoiceBars } from "./AskPanel";
import { Icon } from "./Icon";
import { Activity } from "./IslandParts";
import { COMPACT } from "./island-layout";
import { Tip } from "./Tip";

export function tail(text: string): string {
  return text.length > 38 ? `...${text.slice(-38).replace(/^\S*\s/, "")}` : text;
}

export function voiceLabel(text: string, thinking: boolean, working?: boolean): string {
  if (working) return text;
  if (thinking) return `Thinking: ${text}`;
  return text.trim() ? tail(text) : "Listening...";
}

/** Full listening style: a waveform, a timer and Stop on the first line,
 * then your words a size larger (grey until final) and what happens next. */
export function FullVoice({ text, thinking }: { text: string; thinking: boolean }) {
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

/** Pixel width of the compact status label (avoids a stretch gap from char guesses). */
let statusMeasureCanvas: HTMLCanvasElement | null = null;

export function statusTextWidth(label: string): number {
  if (typeof document === "undefined") return Math.ceil([...label].length * 7.2);
  if (!statusMeasureCanvas) statusMeasureCanvas = document.createElement("canvas");
  const ctx = statusMeasureCanvas.getContext("2d");
  if (!ctx) return Math.ceil([...label].length * 7.2);
  ctx.font = '500 12.5px ui-sans-serif, system-ui, "Segoe UI", sans-serif';
  return Math.ceil(ctx.measureText(label).width);
}

/** Compact listening shell: tight when empty, grows with speech up to voiceWidth. */
export function voiceShellWidth(
  text: string,
  thinking: boolean,
  opts?: { status?: boolean; cancel?: boolean; indicator?: boolean; redesign?: boolean },
): number {
  const status = !!opts?.status;
  const label = voiceLabel(text, thinking, status);
  // Status: orb + gap + label + optional indicator/cancel + right pad.
  // Charge only for chrome that is actually on screen (Focus has no spinner).
  if (status) {
    return Math.min(COMPACT.statusMaxWidth, Math.max(120, statusChrome(opts) + statusTextWidth(label)));
  }
  const textPx = Math.ceil([...label].length * 7.2);
  // Orb column, gaps, green bars, stop button, right pad.
  const chrome = COMPACT.height + 4 + 10 + 20 + 28 + 14;
  return Math.min(COMPACT.voiceWidth, Math.max(COMPACT.voiceMinWidth, chrome + textPx));
}

/** Orb column + trailing controls for a compact status pill (gap-1.5 + pr-2). */
export function statusChrome(opts?: { cancel?: boolean; indicator?: boolean; redesign?: boolean }): number {
  // Redesign: the mascot column is the prototype's 48px (REDESIGN_COMPACT.padLeft).
  let chrome = (opts?.redesign ? REDESIGN_COMPACT.padLeft : COMPACT.height + 4) + 8;
  if (opts?.indicator) chrome += 6 + 12;
  if (opts?.cancel) chrome += 6 + 24;
  return chrome;
}

export function statusNeedsTruncate(label: string, opts?: { cancel?: boolean; indicator?: boolean }): boolean {
  return statusChrome(opts) + statusTextWidth(label) > COMPACT.statusMaxWidth;
}

/** Voice in the compact island: green bars and the words as they come
 * while listening, then "Thinking" with the question until the answer. */
/** The compact agents pill opens Agents on the board. */
export function openBoard() {
  useAgents.setState({ tab: "agents", layout: "board" });
  void api.askOpen();
}

export const DOT: Record<string, string> = { working: "#64d2ff", waiting: "#ff9f0a" };

// One arc per running agent around a slow-turning ring: blue works, orange waits.
export function AgentRing({ states }: { states: string[] }) {
  const r = 6;
  const c = 2 * Math.PI * r;
  const gap = 2.2;
  const seg = c / states.length - gap;
  return (
    <svg
      viewBox="0 0 16 16"
      className="size-4 animate-[spin_3s_linear_infinite] motion-reduce:animate-none"
      role="img"
      aria-label={`${states.length} agents`}
    >
      {states.map((d, i) => (
        <circle
          // biome-ignore lint/suspicious/noArrayIndexKey: one arc per agent, in order
          key={i}
          cx="8"
          cy="8"
          r={r}
          fill="none"
          stroke={DOT[d] ?? "rgb(255 255 255 / 0.3)"}
          strokeWidth="2"
          strokeLinecap="round"
          strokeDasharray={`${seg} ${c - seg}`}
          strokeDashoffset={-i * (seg + gap)}
        />
      ))}
    </svg>
  );
}

export function VoicePill({
  text,
  thinking,
  working,
  done,
  focus,
  asks,
  waiting,
  failed,
  dots,
  activity,
  onOpen,
  onCancel,
}: {
  text: string;
  thinking: boolean;
  working?: boolean;
  done?: boolean;
  focus?: boolean;
  asks?: boolean;
  waiting?: boolean;
  failed?: boolean;
  dots?: string[];
  activity?: ActivityItem;
  onOpen?: () => void;
  onCancel?: () => void;
}) {
  const now = useNow(1000);
  const redesign = useDuskie((s) => s.settings.redesign.enabled);
  const elapsed = activity ? activityElapsedSecs(activity, now) : null;
  // Compact pill always offers icon actions when an activity is on screen.
  const showActions = !!activity;
  // Split · Ns out of the label so copy, timer, and activity can be spaced apart.
  const baseText = activity && elapsed !== null ? activityLabel(activity, now, { elapsed: false }) : text;
  const label = voiceLabel(baseText, thinking, working || done || focus);
  const fullLabel = elapsed !== null ? `${label} · ${elapsed}s` : label;
  // Listening: label flexes so bars + Stop stay on the right. Status: one
  // tight cluster (label + indicator) so the shell never shows a stretch gap.
  const listening = !(working || thinking || done || focus || failed || waiting || asks);
  const clip =
    listening ||
    statusNeedsTruncate(fullLabel, {
      cancel: !!onCancel || !!focus,
      indicator: !!(thinking || working || done || waiting || failed || asks || (dots && dots.length > 0)),
    });
  const warn = redesign ? "bg-(--warn)" : "bg-[#ff9f0a]";
  const inkStrong = redesign ? "text-(--ink)" : "text-white/90";
  const inkSoft = redesign ? "text-(--ink-2)" : "text-white/62";
  const iconBtn = redesign
    ? "chip grid size-6 place-items-center rounded-[8px] text-(--ink-2) hover:bg-(--fill-2) hover:text-(--ink)"
    : "chip grid size-6 place-items-center rounded-full text-white/55 hover:bg-white/10 hover:text-white";
  const stopBtn = redesign
    ? "chip grid size-6 shrink-0 place-items-center rounded-[8px] bg-(--fill-2) hover:bg-(--fill-3)"
    : "chip grid size-6 shrink-0 place-items-center rounded-full bg-white/[0.09] hover:bg-white/[0.15]";
  const stopMark = redesign ? "size-2 rounded-[2px] bg-(--ink)" : "size-2 rounded-[2px] bg-white";
  const indicator = failed ? (
    <span role="img" aria-label="Failed" className="size-2 shrink-0 rounded-full bg-red-400" />
  ) : waiting ? (
    <span role="img" aria-label="Waiting for confirmed progress" className={`size-2 shrink-0 rounded-full ${warn}`} />
  ) : done ? (
    <span className={`shrink-0 ${redesign ? "text-(--ok,#30d158)" : "text-[#30d158]"}`}>
      <Icon name="check" size={14} />
    </span>
  ) : asks ? (
    <span className="relative flex size-2 shrink-0" role="img" aria-label="Waiting for you">
      <span className={`absolute inset-0 animate-ping rounded-full opacity-60 motion-reduce:animate-none ${warn}`} />
      <span className={`relative size-2 rounded-full ${warn}`} />
    </span>
  ) : dots && dots.length > 1 ? (
    <AgentRing states={dots} />
  ) : thinking || working ? (
    <Activity />
  ) : null;
  const labelClass = `${redesign ? "text-[13px] tracking-[-0.005em]" : "text-[12.5px]"} font-medium ${
    listening ? "min-w-0 flex-1 truncate" : clip ? "min-w-0 truncate" : "shrink-0"
  } ${text.trim() || thinking || working ? inkStrong : inkSoft}`;
  const labelInner = <span className="min-w-0 truncate">{label}</span>;
  const labelEl = onOpen ? (
    <button type="button" onClick={onOpen} className={`text-left ${labelClass}`}>
      {labelInner}
    </button>
  ) : (
    <span className={labelClass}>{labelInner}</span>
  );
  const labeled = clip ? <Tip label={fullLabel}>{labelEl}</Tip> : labelEl;
  const actions = showActions ? (
    <span className="flex shrink-0 items-center gap-0.5">
      <Tip label="Check again">
        <button
          type="button"
          aria-label="Check again"
          onClick={(e) => {
            e.stopPropagation();
            void api
              .activityCheck(activity.id)
              .then(acceptActivities)
              .catch(() => {});
          }}
          className={iconBtn}
        >
          <Icon name="refresh" size={12} />
        </button>
      </Tip>
      <Tip label="Dismiss">
        <button
          type="button"
          aria-label="Dismiss"
          onClick={(e) => {
            e.stopPropagation();
            if (activity.source === "open") dismissLocal(activity.id);
            void api.activityDismiss(activity.id).catch(() => {});
          }}
          className={iconBtn}
        >
          <Icon name="close" size={12} />
        </button>
      </Tip>
    </span>
  ) : null;
  const showCancel = !!focus || (!!onCancel && !!(working || asks));
  const cancelLabel = focus ? "End focus" : "Cancel";
  const runCancel = () => {
    if (focus) void api.focusStop();
    else onCancel?.();
  };
  const pillH = redesign ? "h-[38px]" : "h-9";
  // Prototype .cmp: the label starts after the mascot (48px) on the compact pill.
  const pillPad = redesign && !listening ? "pl-12" : "";
  return (
    <motion.div
      className={`absolute top-0 right-0 flex ${pillH} items-center ${listening ? "gap-2.5 pr-3.5" : redesign ? `${pillPad} pr-2` : "pr-2"}`}
      style={{ left: redesign ? 0 : COMPACT.height + 4 }}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: { delay: 0.1, duration: 0.2 } }}
      exit={{ opacity: 0, transition: { duration: 0.08 } }}
      aria-live="polite"
    >
      {listening ? (
        <>
          {labeled}
          <VoiceBars />
          <Tip label="Stop (Esc)">
            <button
              type="button"
              aria-label="Stop listening"
              onClick={(e) => {
                e.stopPropagation();
                void api.voiceStop();
              }}
              className={stopBtn}
            >
              <span className={stopMark} />
            </button>
          </Tip>
        </>
      ) : (
        <div className={`flex min-w-0 max-w-full items-center ${redesign ? "gap-2.5 pr-3" : "gap-1.5"}`}>
          {labeled}
          {elapsed !== null && (
            <span className={`shrink-0 text-[12.5px] font-medium tabular-nums ${inkStrong}`}>· {elapsed}s</span>
          )}
          {indicator && <span className="flex shrink-0 items-center">{indicator}</span>}
          {actions}
          {showCancel && (
            <Tip label={cancelLabel}>
              <button
                type="button"
                aria-label={cancelLabel}
                onClick={(e) => {
                  e.stopPropagation();
                  runCancel();
                }}
                className={stopBtn}
              >
                <span className={stopMark} />
              </button>
            </Tip>
          )}
        </div>
      )}
    </motion.div>
  );
}
