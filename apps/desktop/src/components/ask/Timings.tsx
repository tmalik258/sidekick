"use client";

// The timings overlay: how fast Ask was, last and typical, against the
// targets. Ctrl+Alt+Shift+T shows or hides it; the choice is remembered on
// this PC. The numbers also go to timings.log in Sidekick's data folder.

import { useEffect, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { median, TIMING_LABELS, TIMING_TARGETS } from "@/lib/timings";
import type { Timing } from "@/lib/types";

const KEY = "sidekick.timings";
/** Values kept per timing for the typical (median) figure. */
const KEEP = 20;

function shown(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export function Timings() {
  const [visible, setVisible] = useState(shown);
  const [values, setValues] = useState<Record<Timing["name"], number[]>>({
    open_to_ready: [],
    enter_to_first_word: [],
    speech_to_first_sound: [],
  });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.altKey && e.shiftKey && e.code === "KeyT") {
        e.preventDefault();
        setVisible((v) => {
          try {
            localStorage.setItem(KEY, v ? "0" : "1");
          } catch {}
          return !v;
        });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (!visible) return;
    const add = (t: Timing) => setValues((v) => ({ ...v, [t.name]: [...(v[t.name] ?? []), t.ms].slice(-KEEP) }));
    let off: (() => void) | undefined;
    let gone = false;
    void api
      .timingsRecent()
      .then((list) => {
        if (!gone) for (const t of list) add(t);
      })
      .catch(() => {});
    void listen(EVENTS.timing, add).then((f) => {
      if (gone) f();
      else off = f;
    });
    return () => {
      gone = true;
      off?.();
    };
  }, [visible]);

  if (!visible) return null;
  return (
    <div
      role="status"
      aria-label="Timings"
      className="mt-2 grid gap-1 rounded-xl bg-white/[0.06] px-3 py-2 font-mono text-[11px] text-[rgb(235_235_245/0.6)]"
    >
      {(Object.keys(TIMING_LABELS) as Timing["name"][]).map((name) => {
        const list = values[name];
        const last = list.at(-1);
        const typical = median(list);
        const target = TIMING_TARGETS[name];
        const ok = typical !== null && typical <= target;
        return (
          <div key={name} className="flex items-center justify-between gap-3">
            <span className="truncate font-sans">{TIMING_LABELS[name]}</span>
            <span className="tabular-nums">
              <span className="text-white/85">{last === undefined ? "-" : `${last} ms`}</span>
              <span className={ok ? " text-[#30d158]" : typical === null ? "" : " text-[#ff9f0a]"}>
                {typical === null ? "" : ` · typical ${typical}`}
              </span>
              <span>{` · goal ${target}`}</span>
            </span>
          </div>
        );
      })}
    </div>
  );
}
