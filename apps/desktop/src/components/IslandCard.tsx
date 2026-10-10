"use client";

// The hover day card's layout, for the other island cards: title and detail
// beside the mascot (round buttons on the right), then a labelled list and
// option chips across the full width below, so nothing sits in an empty
// column under the mascot.

import type { ReactNode } from "react";
import { ChipKeys } from "./ask/parts";

/** Pulls a block left under the mascot so it spans the card. */
const FULL = { marginLeft: "calc(var(--orb-indent, 0px) * -1)", width: "calc(100% + var(--orb-indent, 0px))" };

export function CardHead({ title, detail, right }: { title: ReactNode; detail?: ReactNode; right?: ReactNode }) {
  return (
    <div className="flex items-start gap-3">
      <div className="min-w-0 flex-1">
        <p className="truncate font-display text-[15px] leading-5 font-semibold tracking-[-0.015em] text-white">
          {title}
        </p>
        {detail && (
          <p className="mt-0.5 line-clamp-2 text-[13px] leading-4.5 tracking-[-0.005em] text-[rgb(235_235_245/0.6)] tabular-nums">
            {detail}
          </p>
        )}
      </div>
      {right && <div className="flex shrink-0 gap-1.5">{right}</div>}
    </div>
  );
}

export interface CardRow {
  key: string;
  title: string;
  detail?: string;
  right?: string;
  /** Orange: waits for you; green: finished. */
  dot?: "wait" | "done";
  onClick?: () => void;
}

const DOT = { wait: "bg-[#ff9f0a]", done: "bg-[#30d158]" } as const;

/** A labelled list, like "You missed" on the day card. */
export function CardList({ label, action, rows }: { label?: string; action?: ReactNode; rows: CardRow[] }) {
  if (rows.length === 0) return null;
  return (
    <div className="mt-3 flex flex-col gap-1.5" style={FULL}>
      {(label || action) && (
        <div className="flex items-center justify-between text-[12px] text-[rgb(235_235_245/0.6)]">
          <span>{label}</span>
          {action}
        </div>
      )}
      {rows.map((r, n) => (
        <button
          key={r.key}
          type="button"
          style={{ animationDelay: `${Math.min(n, 6) * 40}ms` }}
          onClick={r.onClick}
          className="chip rise-in flex w-full shrink-0 items-center gap-3 rounded-xl bg-white/[0.07] px-3 py-1.5 text-left hover:bg-white/12"
        >
          {r.dot && <span className={`size-1.5 shrink-0 rounded-full ${DOT[r.dot]}`} aria-hidden="true" />}
          <span className="min-w-0 flex-1 overflow-hidden">
            <span className="block truncate text-[13px] font-medium text-white">{r.title}</span>
            {r.detail && <span className="block truncate text-[12px] text-[rgb(235_235_245/0.62)]">{r.detail}</span>}
          </span>
          {r.right && <span className="shrink-0 text-[11px] text-white/45 tabular-nums">{r.right}</span>}
        </button>
      ))}
    </div>
  );
}

/** A block of text across the card, in the list's style (an error, a note). */
export function CardNote({ children }: { children: ReactNode }) {
  return (
    <div
      className="mt-3 rounded-xl bg-white/[0.07] px-3 py-2 font-mono text-[12px] leading-4.5 break-words text-[rgb(235_235_245/0.75)]"
      style={FULL}
    >
      {children}
    </div>
  );
}

export interface CardOption {
  label: string;
  keys?: string;
  run: () => void;
}

/** Option chips across the card: the first white, the rest glass, and a
 * quiet text button last (Not now, Clear all). */
export function CardChips({ options, quiet }: { options: CardOption[]; quiet?: CardOption }) {
  return (
    <div className="mt-3 flex flex-wrap items-center gap-1.5" style={FULL}>
      {options.map((o, i) => (
        <button
          // biome-ignore lint/suspicious/noArrayIndexKey: labels can repeat and the list never reorders
          key={`${i}-${o.label}`}
          type="button"
          onClick={o.run}
          className={`chip flex max-w-full min-h-8 items-center gap-2 rounded-full px-3 py-1.5 text-left text-[13px] font-medium tracking-[-0.01em] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#0a84ff] ${
            i === 0 ? "bg-white text-black hover:bg-white/90" : "bg-white/12 text-white hover:bg-white/20"
          }`}
        >
          <span className="min-w-0 truncate leading-snug">{o.label}</span>
          {o.keys && (
            <kbd
              className={`shrink-0 self-center font-sans text-[11px] leading-none ${i === 0 ? "text-black/40" : "text-white/45"}`}
            >
              <ChipKeys keys={o.keys} />
            </kbd>
          )}
        </button>
      ))}
      {quiet && (
        <button
          type="button"
          onClick={quiet.run}
          className="chip rounded-full px-2.5 py-1.5 text-[13px] text-[rgb(235_235_245/0.6)] hover:text-white"
        >
          {quiet.label}
          {quiet.keys && (
            <kbd className="ml-1.5 font-sans text-[11px] text-white/45">
              <ChipKeys keys={quiet.keys} />
            </kbd>
          )}
        </button>
      )}
    </div>
  );
}
