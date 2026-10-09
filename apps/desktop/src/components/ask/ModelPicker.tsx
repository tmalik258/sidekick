"use client";

// Picks which model answers in Ask mode: a chip in the header with a
// Graphite menu under it. Alt M steps through the choices.

import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { COPILOT_APP, setAskModel, setOverlayHit, useSidekick } from "@/lib/store";
import { PROVIDER_LABELS, type ProviderStatus } from "@/lib/types";
import { Icon } from "../Icon";
import { Tip } from "../Tip";
import { KeyHint } from "./parts";

/** What each model costs or where it runs, under its name in the menu. */
const PROVIDER_NOTES: Record<string, string> = {
  claude_code: "Your Claude subscription",
  codex: "Your ChatGPT plan",
  anthropic: "API key, billed per answer",
  local: "On this PC, nothing leaves it",
};

const SHORT: Record<string, string> = {
  claude_code: "Claude Code",
  codex: "Codex",
  anthropic: "Claude API",
  local: "Local",
};

/** The model that answers in Ask mode: Auto (Sidekick picks) or one of
 * those that can answer now. Kept for next time. */
export function ModelPicker({
  choices,
  best,
  picked,
  keys,
}: {
  choices: ProviderStatus[];
  best: ProviderStatus;
  picked: ProviderStatus | null;
  keys: boolean;
}) {
  const [open, setOpen] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const chipRef = useRef<HTMLSpanElement>(null);
  // The menu floats over the window, so the island keeps its size.
  const [at, setAt] = useState<{ left: number; top: number; above: boolean; height: number } | null>(null);
  const items: { id: string | null; title: string; note: string }[] = [
    {
      id: null,
      title: "Auto",
      note: `Picks for each question, now ${PROVIDER_LABELS[best.id] ?? best.id}`,
    },
    ...choices.map((p) => ({
      id: p.id,
      title: PROVIDER_LABELS[p.id] ?? p.id,
      note: PROVIDER_NOTES[p.id] ?? (p.local ? "On this PC" : ""),
    })),
    // Copilot for personal accounts has no API: Sidekick hands the question over.
    { id: COPILOT_APP, title: "Copilot app", note: "Opens Copilot with your question copied" },
  ];
  const copilot = useSidekick((s) => s.askModel) === COPILOT_APP;
  useEffect(() => {
    if (!open) {
      setAt(null);
      return;
    }
    const r = chipRef.current?.getBoundingClientRect();
    if (!r) return;
    const width = 256;
    const height = 30 + items.length * 46;
    const left = Math.min(Math.max(8, r.right - width), window.innerWidth - width - 8);
    // Ask sits at the bottom of a short island window: open above when there
    // is not enough room below (otherwise the menu is clipped).
    const spaceBelow = window.innerHeight - r.bottom;
    const spaceAbove = r.top;
    const above = spaceBelow < height && spaceAbove >= spaceBelow;
    const top = above ? r.top - 6 : r.bottom + 6;
    setAt({ left, top, above, height });
    setOverlayHit({
      x: left,
      y: above ? top - height : top,
      width,
      height,
    });
    requestAnimationFrame(() => listRef.current?.querySelector<HTMLButtonElement>("[aria-checked=true]")?.focus());
    return () => setOverlayHit(null);
  }, [open, items.length]);
  const choose = (id: string | null) => {
    setAskModel(id);
    setOpen(false);
  };
  const onListKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const buttons = [...(listRef.current?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      buttons[(at + (e.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    }
  };
  const shown = picked ?? best;
  return (
    <span ref={chipRef} className="relative shrink-0">
      <Tip label="Model (Alt M)">
        <button
          type="button"
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
          className="ak-model chip"
        >
          <span className="ak-dot" aria-hidden="true" />
          {copilot ? "Copilot" : picked ? (SHORT[shown.id] ?? shown.id) : "Auto"}
          <Icon name="chevron" size={10} />
        </button>
      </Tip>
      <KeyHint show={keys}>Alt M</KeyHint>
      {open &&
        at &&
        createPortal(
          <>
            <button
              type="button"
              aria-label="Close menu"
              tabIndex={-1}
              className="fixed inset-0 z-20 cursor-default"
              onClick={() => setOpen(false)}
            />
            <div
              ref={listRef}
              role="menu"
              aria-label="Model"
              onKeyDown={onListKey}
              style={
                at.above
                  ? { left: at.left, bottom: window.innerHeight - at.top, width: 256 }
                  : { left: at.left, top: at.top, width: 256 }
              }
              className="menu fixed z-30 flex w-64 flex-col gap-0.5 rounded-[14px] p-1 text-[13px]"
            >
              <p className="px-2.5 pt-1 pb-0.5 text-[11px] font-medium text-[rgb(235_235_245/0.45)]">
                Answers come from
              </p>
              {items.map((it) => {
                const on = copilot ? it.id === COPILOT_APP : (picked?.id ?? null) === it.id;
                return (
                  <button
                    key={it.id ?? "auto"}
                    type="button"
                    role="menuitemradio"
                    aria-checked={on}
                    onClick={() => choose(it.id)}
                    className="menu-item"
                  >
                    <span className="font-medium">{it.title}</span>
                    <span className="row-span-2 text-[#0a84ff]" aria-hidden="true">
                      {on && <Icon name="check" size={14} />}
                    </span>
                    {it.note && <small>{it.note}</small>}
                  </button>
                );
              })}
            </div>
          </>,
          document.body,
        )}
    </span>
  );
}
