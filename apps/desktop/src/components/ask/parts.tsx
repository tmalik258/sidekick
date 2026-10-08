"use client";

// Small pieces shared across Ask mode: chips, rows, key hints.

import type { ReactNode } from "react";
import { Tip } from "../Tip";

export const ease = [0.23, 1, 0.32, 1] as const;

export function Chip({
  on,
  onClick,
  title,
  children,
}: {
  on: boolean;
  onClick: () => void;
  title?: string | null;
  children: ReactNode;
}) {
  const button = (
    <button
      type="button"
      aria-pressed={on}
      onClick={onClick}
      className={`chip max-w-[200px] truncate rounded-full px-2.5 py-1 text-[11.5px] font-medium ${
        on ? "bg-white text-black" : "bg-white/[0.1] text-[rgb(235_235_245/0.7)] hover:bg-white/[0.16]"
      }`}
    >
      {children}
    </button>
  );
  return title ? <Tip label={title}>{button}</Tip> : button;
}

export function Pill({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="chip h-7 shrink-0 rounded-full bg-white/[0.12] px-3 text-[12px] font-medium text-white/85 hover:bg-white/[0.2]"
    >
      {children}
    </button>
  );
}

export function Row({
  active,
  onHover,
  onClick,
  children,
}: {
  active: boolean;
  onHover: () => void;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <li>
      <button
        type="button"
        onMouseMove={onHover}
        onClick={onClick}
        className={`flex w-full items-center gap-2.5 rounded-[14px] px-1.5 py-1.5 text-left text-[13.5px] tracking-[-0.01em] transition-colors duration-100 ${
          active ? "bg-white/[0.14] text-white ring-1 ring-inset ring-white/25" : "text-white/80"
        }`}
      >
        {children}
      </button>
    </li>
  );
}

export const SOURCE_LABELS: Record<string, string> = {
  file: "File",
  download: "Download",
  screenshot: "Screenshot",
  clipboard: "Copied",
  page: "Web page",
  claude: "Claude Code",
  action: "Action",
  chat: "Ask",
};

/** Matches come back between [ and ]; show them bold. */
export function Snippet({ text }: { text: string }) {
  const parts = text.split(/(\[[^\]]*\])/g);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith("[") && p.endsWith("]") ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: parts have no identity beyond order
          <strong key={i} className="font-semibold text-white">
            {p.slice(1, -1)}
          </strong>
        ) : (
          p
        ),
      )}
    </>
  );
}

/** Rows already scrolled for the current highlight spell. Ref callbacks are
 *  new every render; without this, each parent update re-pins the row and
 *  fights the wheel. */
const scrolledActive = new WeakMap<HTMLElement, true>();

/** Keeps the highlighted row of a list in view as the arrows move it. */
export function scrollIfActive(active: boolean) {
  return (el: HTMLElement | null) => {
    if (!el) return;
    if (!active) {
      scrolledActive.delete(el);
      return;
    }
    if (scrolledActive.has(el)) return;
    scrolledActive.set(el, true);
    el.scrollIntoView({ block: "nearest" });
  };
}

/** "5 min ago", "yesterday", or a date. */
export function ago(iso: string): string {
  const mins = Math.round((Date.now() - Date.parse(iso)) / 60_000);
  if (!Number.isFinite(mins)) return "";
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins} min ago`;
  if (mins < 24 * 60) return `${Math.round(mins / 60)} h ago`;
  if (mins < 48 * 60) return "yesterday";
  return new Date(iso).toLocaleDateString();
}

/** While Alt is held only the letter is needed, like Windows KeyTips: "Alt M" is "M". */
export function keyLetter(keys: string): string {
  return keys.replace(/^Alt[ +]/, "");
}

/** Keys inside a chip: "Alt 1" drops "Alt " while Alt is held. */
export function ChipKeys({ keys }: { keys: string }) {
  const letter = keyLetter(keys);
  return letter === keys ? (
    keys
  ) : (
    <>
      <i className="alt-pre">Alt </i>
      {letter}
    </>
  );
}

/** A key badge that shows on its control while Alt is held. */
export function KeyHint({
  show,
  children,
  side = "left",
}: {
  show: boolean;
  children: ReactNode;
  side?: "left" | "right";
}) {
  return (
    <span
      aria-hidden="true"
      className={`key-hint pointer-events-none absolute -top-1.5 z-10 rounded-[5px] whitespace-nowrap bg-white px-1 font-sans text-[9.5px] leading-[15px] font-semibold text-black shadow-[0_2px_6px_rgb(0_0_0/0.45)] ${
        side === "left" ? "-left-1" : "-right-1"
      }`}
      data-show={show}
    >
      {typeof children === "string" ? keyLetter(children) : children}
    </span>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded-[5px] bg-white/[0.1] px-1.5 py-px font-sans text-[10px] text-[rgb(235_235_245/0.6)]">
      {children}
    </kbd>
  );
}
