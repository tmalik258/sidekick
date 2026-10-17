"use client";

import { motion } from "motion/react";
import { latestActivity, useActivities } from "@/lib/activities";
import { api } from "@/lib/bridge";
import type { NetNotice } from "@/lib/net";
import { playSound } from "@/lib/sound";
import { installUpdate, notePick, uiVolume, useDuskie } from "@/lib/store";
import type { MascotState, Suggestion } from "@/lib/types";
import { Ic } from "./Icon";
import { Glance, RoundButton } from "./IslandGlance";
import { COMPACT, DETAIL, TITLE } from "./island-layout";
import { MergeCard } from "./MergeCard";
import { Tip } from "./Tip";
import { UpdateProgress } from "./UpdateProgress";

export function CompactTrailing({
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
  const later = useDuskie((s) => s.later);
  if (waiting) {
    return (
      <motion.div
        className="absolute top-0 right-0 flex h-[38px] items-center gap-2.5 pr-3.5"
        style={{ left: COMPACT.height + 4 }}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1, transition: { delay: 0.12, duration: 0.2 } }}
        exit={{ opacity: 0, transition: { duration: 0.08 } }}
      >
        <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium text-(--c2)/85">Waiting for {waiting}</span>
        <Activity />
      </motion.div>
    );
  }
  return (
    <motion.div
      className="absolute top-0 right-0 flex h-[38px] items-center pr-3.5"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: { delay: 0.12, duration: 0.2 } }}
      exit={{ opacity: 0, transition: { duration: 0.08 } }}
    >
      {busy ? (
        <Activity />
      ) : offline ? (
        <Tip label="Offline">
          <span role="img" aria-label="Offline" className="text-(--warn)">
            <Ic name="wifi" size={14} />
          </span>
        </Tip>
      ) : paused ? (
        <Tip label="Paused">
          <span className="size-1.5 rounded-full bg-(--warn)" style={{ boxShadow: "0 0 8px var(--warn)" }} />
        </Tip>
      ) : (
        later > 0 && (
          <Tip label={`${later} waiting for you`}>
            <span className="grid h-4 min-w-4 place-items-center rounded-full bg-(--c3) px-1 text-[10px] leading-none font-semibold text-(--c1)">
              {later}
            </span>
          </Tip>
        )
      )}
    </motion.div>
  );
}

/** Three bars that breathe while the mascot is busy. */
export function Activity() {
  return (
    <span className="flex h-3 items-center gap-0.75" role="img" aria-label="Busy">
      {[0, 1, 2].map((i) => (
        <motion.span
          key={i}
          className="w-0.75 rounded-full bg-(--listen)"
          animate={{ height: [4, 12, 4] }}
          transition={{ duration: 0.9, repeat: Number.POSITIVE_INFINITY, delay: i * 0.15, ease: "easeInOut" }}
        />
      ))}
    </span>
  );
}

export function ExpandedContent({
  mascot,
  paused,
  suggestion,
}: {
  mascot: MascotState;
  paused: boolean;
  suggestion: Suggestion | null;
}) {
  const activityState = useActivities();
  const result = useDuskie((s) => s.lastResult);
  const running = useDuskie((s) => s.running);
  const netNotice = useDuskie((s) => s.netNotice);
  const merge = useDuskie((s) => s.merge);
  const progress = useDuskie((s) => s.updateProgress);
  const reporting = (mascot === "success" || mascot === "error" || mascot === "working") && !suggestion;
  if (merge && !suggestion) return <MergeCard path={merge.path} />;
  if (netNotice && !suggestion) return <NetNoticeCard notice={netNotice} />;
  if (progress && !suggestion && !latestActivity(activityState.activities, "update")?.dismissed) {
    return (
      <div className="flex flex-col gap-3">
        <UpdateProgress progress={progress} />
        {progress.phase === "failed" && (
          <div className="flex gap-3 text-[13px]">
            <button
              type="button"
              className="chip text-white"
              onClick={() => void installUpdate().catch(() => undefined)}
            >
              Retry update
            </button>
            <button
              type="button"
              className="chip text-white/60"
              onClick={() => useDuskie.setState({ updateProgress: null })}
            >
              Dismiss
            </button>
          </div>
        )}
        <Glance paused={paused} />
      </div>
    );
  }
  // Idle / suggesting / agent working / needs-you: one unified glance (activities are a section).
  if (!suggestion && (mascot === "idle" || mascot === "sleeping" || mascot === "suggesting" || mascot === "working")) {
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
                <Ic name="folder" size={15} />
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
export function NetNoticeCard({ notice }: { notice: NetNotice }) {
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
        <Ic name="wifi" size={16} />
      </span>
    </div>
  );
}

export function UndoButton({ id }: { id: number }) {
  const undo = async () => {
    const result = useDuskie.getState().lastResult;
    try {
      const message = await api.actionUndo(id);
      useDuskie.setState({ lastResult: result && { ...result, message, path: null, undoId: null } });
    } catch (err) {
      useDuskie.setState({ lastResult: result && { ...result, ok: false, message: String(err), undoId: null } });
    }
  };
  return (
    <RoundButton label="Undo" onClick={() => void undo()}>
      <Ic name="undo" size={15} />
    </RoundButton>
  );
}

export function Options({ suggestion }: { suggestion: Suggestion }) {
  const alwaysAt = suggestion.always?.findIndex(Boolean) ?? -1;
  // Shift while hovered, or Shift+Alt anywhere, Rust polls; no hover required.
  const priv = useDuskie((s) => s.shiftHeld);
  return (
    <div className="mt-3 flex flex-col gap-1.5">
      {suggestion.options.map((option, i) => {
        const openUrl = suggestion.actions[i] === "open_url";
        const label = priv && openUrl ? privateOpenLabel(option) : option;
        return (
          <motion.button
            // biome-ignore lint/suspicious/noArrayIndexKey: labels can repeat and the list never reorders
            key={`${i}-${option}`}
            type="button"
            onClick={(e) => choose(suggestion, i, e.shiftKey || (priv && openUrl))}
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.22, delay: 0.08 + i * 0.03, ease: [0.23, 1, 0.32, 1] }}
            className={`rd-opt chip justify-between gap-2 ${i === 0 ? "rd-opt-primary" : ""}`}
          >
            <span className="min-w-0 flex-1 leading-snug text-balance">{label}</span>
            <kbd className={`shrink-0 font-sans text-[11px] leading-none ${i === 0 ? "opacity-40" : "opacity-45"}`}>
              Alt {i + 1}
            </kbd>
          </motion.button>
        );
      })}
      <div className="mt-1 flex flex-wrap items-center gap-2">
        {alwaysAt >= 0 && (
          <Tip label={`From now on, "${suggestion.options[alwaysAt]}" without asking. Undo in Settings > Skills.`}>
            <button type="button" onClick={() => always(suggestion, alwaysAt)} className="rd-gfoot-btn chip">
              Always {suggestion.options[alwaysAt].toLowerCase()}
            </button>
          </Tip>
        )}
        <button type="button" onClick={() => showLess(suggestion)} className="rd-gfoot-btn chip">
          Show less
        </button>
        <button type="button" onClick={() => dismiss(suggestion)} className="rd-gfoot-btn chip">
          Dismiss
          <kbd className="ml-1.5 font-sans text-[11px] opacity-45">
            <i className="alt-pre">Alt </i>0
          </kbd>
        </button>
      </div>
    </div>
  );
}

export const OPEN_ACTIONS = new Set([
  "open_path",
  "open_folder",
  "open_in_editor",
  "open_url",
  "open_app",
  "launch_app",
  "open_system_page",
  "routine_open",
  "routine_open_all",
]);

/** Shift+Alt morph: browser chips become their private/incognito names. */
export function privateOpenLabel(option: string): string {
  if (/\b(private|incognito|inprivate)\b/i.test(option)) return option;
  switch (option.trim().toLowerCase()) {
    case "edge":
      return "Edge InPrivate";
    case "firefox":
      return "Firefox Private";
    case "zen":
      return "Zen Private";
    case "brave":
      return "Brave Incognito";
    case "chrome":
      return "Chrome Incognito";
    case "default browser":
      return "Private window";
    default:
      return `${option} private`;
  }
}

export function runningLabel(suggestion: Suggestion, index: number): string {
  const action = suggestion.actions[index];
  if (action && OPEN_ACTIONS.has(action)) return "Opening";
  return suggestion.options[index] ?? "Working";
}

/** Shift+click or Shift+Alt+N opens a link in a private window. */
export function choose(suggestion: Suggestion, index: number, priv = false) {
  playSound("select", uiVolume(), useDuskie.getState().settings.soundKit);
  useDuskie.setState({ running: runningLabel(suggestion, index) });
  notePick(suggestion.skillId, suggestion.options[index] ?? "");
  void api.suggestionChoose(suggestion.id, index, priv);
}

export function always(suggestion: Suggestion, index: number) {
  playSound("select", uiVolume(), useDuskie.getState().settings.soundKit);
  useDuskie.setState({ running: runningLabel(suggestion, index) });
  void api.suggestionAlways(suggestion.id, index);
}

export function dismiss(suggestion: Suggestion) {
  playSound("toggle_off", uiVolume(), useDuskie.getState().settings.soundKit);
  void api.suggestionDismiss(suggestion.id, "user");
}

/** Quieter next time; counts toward scoring (Dismiss does not). */
export function showLess(suggestion: Suggestion) {
  playSound("toggle_off", uiVolume(), useDuskie.getState().settings.soundKit);
  void api.attentionShowLess(suggestion.skillId);
  void api.suggestionDismiss(suggestion.id, "user");
}

/** Hover with intent: a short delay in (or longer when the pill has actions), a grace period out. */
