"use client";

// Dev only: every foundation piece on one page (tokens, springs, mascot
// states, the event wash). Open /dev/foundation/ in the browser to check them.

import { useRef, useState } from "react";
import { Creature } from "@/components/Creature";
import { EXPRESSIONS } from "@/components/creature/expressions";
import { GrainWash, type GrainWashHandle } from "@/components/GrainWash";
import { IslandWash } from "@/components/IslandWash";
import { SPRINGS, spring } from "@/lib/spring";
import { MASCOT_STATES, type MascotState } from "@/lib/types";
import { WASH_EVENTS, washFor } from "@/lib/wash";

const TOKENS = ["--c1", "--c2", "--c3", "--c4", "--sage-ink", "--ok", "--warn", "--err", "--listen"] as const;

export default function FoundationPage() {
  const [mascot, setMascot] = useState<MascotState>("idle");
  const [donePill, setDonePill] = useState<string | null>(null);
  const wash = useRef<GrainWashHandle>(null);

  return (
    <main className="min-h-screen bg-[var(--c1)] p-6 text-[var(--c2)]">
      <h1 className="mb-6 text-lg font-semibold">Foundation</h1>

      <section className="mb-8">
        <h2 className="mb-3 text-sm text-[var(--ink-2)]">Tokens</h2>
        <div className="flex flex-wrap gap-3">
          {TOKENS.map((t) => (
            <div key={t} className="w-28">
              <div
                className="mb-1 h-12 rounded-[var(--r-md)] border border-[var(--line-2)]"
                style={{ background: `var(${t})` }}
              />
              <code className="text-xs text-[var(--ink-3)]">{t}</code>
            </div>
          ))}
        </div>
      </section>

      <section className="mb-8">
        <h2 className="mb-3 text-sm text-[var(--ink-2)]">Springs</h2>
        <table className="text-sm">
          <tbody>
            {Object.entries(SPRINGS).map(([name, s]) => {
              const t = spring(s);
              return (
                <tr key={name}>
                  <td className="pr-6 py-1">{name}</td>
                  <td className="pr-6 py-1 text-[var(--ink-3)]">
                    response {s.response}s, damping {s.damping}
                  </td>
                  <td className="py-1 text-[var(--ink-3)]">
                    stiffness {t.stiffness}, damping {t.damping}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>

      <section className="mb-8">
        <h2 className="mb-3 text-sm text-[var(--ink-2)]">Mascot states</h2>
        <div className="flex flex-wrap gap-2">
          {MASCOT_STATES.map((s) => (
            <button
              key={s}
              type="button"
              onClick={() => setMascot(s)}
              aria-pressed={mascot === s}
              className="rounded-[var(--r-sm)] border border-[var(--line-2)] px-3 py-1.5 text-sm aria-pressed:bg-[var(--fill-3)]"
            >
              {s}
            </button>
          ))}
        </div>
      </section>

      <section className="mb-8">
        <h2 className="mb-3 text-sm text-[var(--ink-2)]">Every expression</h2>
        <div className="flex flex-wrap gap-4">
          {EXPRESSIONS.map((x) => (
            <div key={x} className="flex w-24 flex-col items-center gap-1">
              <Creature state="idle" face={x} size={72} magnetic={false} />
              <code className="text-xs text-[var(--ink-3)]">{x}</code>
            </div>
          ))}
        </div>
      </section>

      <section className="relative mb-8 h-40 overflow-hidden rounded-[var(--r-lg)] border border-[var(--line-2)] bg-[var(--fill)]">
        <IslandWash simple={false} mascot={mascot} donePill={donePill} />
        <div className="relative z-10 flex h-full items-center justify-center text-sm text-[var(--ink-2)]">
          Wash shows on success or when a pill is set
        </div>
      </section>

      <section className="mb-8">
        <h2 className="mb-3 text-sm text-[var(--ink-2)]">Grain wash (base follows the mascot state)</h2>
        <div className="mb-3 h-48 overflow-hidden rounded-[var(--r-lg)] border border-[var(--line-2)]">
          <GrainWash base={washFor(mascot)} handle={wash} />
        </div>
        <div className="flex flex-wrap gap-2">
          {WASH_EVENTS.map((e) => (
            <button
              key={e}
              type="button"
              onClick={() => wash.current?.fire(e)}
              className="rounded-[var(--r-sm)] border border-[var(--line-2)] px-3 py-1.5 text-sm"
            >
              {e}
            </button>
          ))}
        </div>
      </section>

      <section>
        <h2 className="mb-3 text-sm text-[var(--ink-2)]">Event wash</h2>
        <button
          type="button"
          onClick={() => setDonePill(`Done ${Date.now()}`)}
          className="rounded-[var(--r-sm)] border border-[var(--line-2)] px-3 py-1.5 text-sm"
        >
          Fire a done event
        </button>
      </section>
    </main>
  );
}
