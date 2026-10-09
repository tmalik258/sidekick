"use client";

// Picks which model answers in Ask mode: a chip in the header with a
// Graphite menu under it. Alt M steps through the choices.

import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { setAskModel } from "@/lib/store";
import { PROVIDER_LABELS, type ProviderStatus } from "@/lib/types";
import { Icon } from "../Icon";
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
  ];
  useEffect(() => {
    if (open) listRef.current?.querySelector<HTMLButtonElement>("[aria-checked=true]")?.focus();
  }, [open]);
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
    <span className="relative shrink-0">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        title="Model (Alt M)"
        onClick={() => setOpen((o) => !o)}
        className="chip flex h-7 items-center gap-1.5 rounded-full bg-white/[0.1] pr-2 pl-2.5 text-[12px] font-medium text-white/85 hover:bg-white/[0.16]"
      >
        <span className="size-1.5 rounded-full bg-[#30d158]" aria-hidden="true" />
        {picked ? (SHORT[shown.id] ?? shown.id) : "Auto"}
        <Icon name="chevron" size={12} />
      </button>
      <KeyHint show={keys}>Alt M</KeyHint>
      {open && (
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
            className="menu absolute top-full right-0 z-30 mt-1.5 flex w-64 flex-col gap-0.5 rounded-[14px] p-1 text-[13px]"
          >
            <p className="px-2.5 pt-1 pb-0.5 text-[11px] font-medium text-[rgb(235_235_245/0.4)]">Answers come from</p>
            {items.map((it) => {
              const on = (picked?.id ?? null) === it.id;
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
        </>
      )}
    </span>
  );
}
