"use client";

// Picks which model answers in Ask mode.

import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { setAskModel } from "@/lib/store";
import { PROVIDER_LABELS, type ProviderStatus } from "@/lib/types";

/** The model that answers in Ask mode: Auto (Sidekick picks) or one of
 * those that can answer now. Kept for next time. */
export function ModelPicker({
  choices,
  best,
  picked,
}: {
  choices: ProviderStatus[];
  best: ProviderStatus;
  picked: ProviderStatus | null;
}) {
  const [open, setOpen] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const label = (p: ProviderStatus) => `${PROVIDER_LABELS[p.id] ?? p.id}${p.local ? ", on this PC" : ""}`;
  const items: { id: string | null; text: string }[] = [
    { id: null, text: `Auto (${PROVIDER_LABELS[choices[0]?.id] ?? "best"})` },
    ...choices.map((p) => ({ id: p.id, text: label(p) })),
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
  return (
    <span className="relative min-w-0">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        title="Change the model (Alt M)"
        onClick={() => setOpen((o) => !o)}
        className="flex max-w-full items-center gap-1 truncate rounded-md px-1 py-0.5 hover:bg-white/[0.08] hover:text-white/80"
      >
        <span className="truncate">{picked ? label(picked) : `Auto: ${label(best)}`}</span>
        <span aria-hidden="true">▾</span>
      </button>
      {open && (
        <div
          ref={listRef}
          role="menu"
          aria-label="Model"
          onKeyDown={onListKey}
          className="absolute bottom-full left-0 z-20 mb-1.5 flex min-w-52 flex-col gap-0.5 rounded-xl bg-[#1c1c1e] p-1 text-[12.5px] shadow-xl ring-1 ring-white/10"
        >
          {items.map((it) => {
            const on = (picked?.id ?? null) === it.id;
            return (
              <button
                key={it.id ?? "auto"}
                type="button"
                role="menuitemradio"
                aria-checked={on}
                onClick={() => choose(it.id)}
                className={`flex w-full items-center justify-between gap-3 rounded-lg px-2.5 py-1.5 text-left ${
                  on ? "bg-white/[0.12] text-white" : "text-white/75 hover:bg-white/[0.08]"
                }`}
              >
                {it.text}
                {on && <span aria-hidden="true">✓</span>}
              </button>
            );
          })}
        </div>
      )}
    </span>
  );
}
