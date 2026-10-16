"use client";

// Dev only: every island state from the registry with its size, face and
// wash. "Announce" speaks the state through a polite live region, the same
// way the island does for a screen reader.

import { useState } from "react";
import { Creature } from "@/components/Creature";
import { GrainWash } from "@/components/GrainWash";
import { ISLAND_STATES, type IslandStateDef } from "@/lib/island-states";

export default function StatesPage() {
  const [said, setSaid] = useState("");
  const [sel, setSel] = useState<IslandStateDef>(ISLAND_STATES[0]);

  return (
    <main className="min-h-screen bg-[var(--c1)] p-6 text-[var(--c2)]">
      <h1 className="mb-1 text-lg font-semibold">Island states</h1>
      <p className="mb-6 text-sm text-[var(--ink-2)]">
        {ISLAND_STATES.length} states. Announce sends the spoken line to the live region.
      </p>
      <div aria-live="polite" role="status" className="mb-4 h-5 text-sm text-[var(--sage-ink)]">
        {said}
      </div>
      <div className="relative mb-6 h-40 overflow-hidden rounded-[var(--r-lg)] border border-[var(--line-2)]">
        <GrainWash base={sel.wash} />
        {sel.mascot && (
          <div className="absolute inset-0 flex items-center justify-center">
            <Creature state={sel.mascot} size={80} magnetic={false} />
          </div>
        )}
      </div>
      <ul className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-4">
        {ISLAND_STATES.map((s: IslandStateDef) => (
          <li
            key={s.id}
            className="overflow-hidden rounded-[var(--r-lg)] border border-[var(--line-2)] bg-[var(--fill)]"
          >
            <div className="p-3">
              <div className="text-sm font-medium">{s.label}</div>
              <div className="mb-2 text-xs text-[var(--ink-3)]">
                {s.size} · {s.mascot ?? "any face"} · {s.wash}
              </div>
              <button
                type="button"
                onClick={() => setSel(s)}
                aria-pressed={sel.id === s.id}
                className="mr-2 rounded-[var(--r-sm)] border border-[var(--line-2)] px-2.5 py-1 text-xs aria-pressed:bg-[var(--fill-3)]"
              >
                Show
              </button>
              <button
                type="button"
                disabled={!s.announce}
                onClick={() => setSaid(s.announce ?? "")}
                className="rounded-[var(--r-sm)] border border-[var(--line-2)] px-2.5 py-1 text-xs disabled:opacity-40"
              >
                Announce
              </button>
            </div>
          </li>
        ))}
      </ul>
    </main>
  );
}
