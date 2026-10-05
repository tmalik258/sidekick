"use client";

// Browser extension one-click helper: one row per installed browser, each
// paired on its own. Set up stages the folder, copies the path and opens the
// browser's extensions page; the island keeps the steps while you finish.

import { useEffect, useRef, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { installExtension } from "@/lib/store";
import type { BrowserStatus } from "@/lib/types";

/** The welcome's browser step (and anywhere the checklist expands it inline). */
export function BrowserInstallPanel({ onDone }: { onDone?: () => void }) {
  const { data: browsers, refresh } = useCached<BrowserStatus[]>("browsers", api.browsersStatus);
  // Already-paired ids when the panel first got data — only onDone for a new pair.
  const known = useRef<Set<string> | null>(null);

  useEffect(() => {
    const off = listen(EVENTS.browsersChanged, () => void refresh().catch(() => undefined));
    return () => {
      void off.then((f) => f());
    };
  }, [refresh]);

  useEffect(() => {
    if (!browsers) return;
    if (known.current === null) {
      known.current = new Set(browsers.filter((b) => b.connected).map((b) => b.id));
      return;
    }
    const fresh = browsers.some((b) => b.connected && !known.current?.has(b.id));
    for (const b of browsers) {
      if (b.connected) known.current.add(b.id);
    }
    // Once when a browser newly pairs — not on a timer (that pulsed Checking…).
    if (fresh) onDone?.();
  }, [browsers, onDone]);

  return (
    <div className="flex flex-col gap-2 text-[12px] leading-relaxed text-(--muted)">
      <p>Opens the extensions page and copies the path. You Load unpacked once.</p>
      {browsers === null ? (
        <div className="h-9" aria-busy="true" />
      ) : browsers.length === 0 ? (
        <p>No supported browser found.</p>
      ) : (
        <ul className="flex flex-col gap-1">
          {browsers.map((b) => (
            <BrowserRow key={b.id} browser={b} />
          ))}
        </ul>
      )}
    </div>
  );
}

function BrowserRow({ browser }: { browser: BrowserStatus }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const install = () => {
    setBusy(true);
    setError(null);
    void installExtension(browser.id, browser.name)
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  };
  return (
    <li className="flex flex-col gap-1 rounded-xl px-2.5 py-1.5 transition-colors duration-150 hover:bg-white/[0.04]">
      <div className="flex items-center gap-2.5">
        <span
          aria-hidden="true"
          className={`size-1.5 shrink-0 rounded-full ${browser.connected ? "bg-[#30d158]" : "bg-white/25"}`}
        />
        <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium text-white">{browser.name}</span>
        {browser.connected ? (
          <span className="text-[12px] text-[#30d158]">Paired</span>
        ) : (
          <button
            type="button"
            disabled={busy}
            onClick={install}
            className="chip rounded-full bg-white px-2.5 py-1 text-[12px] font-medium text-black hover:bg-white/90 disabled:opacity-50"
          >
            {busy ? "Opening..." : "Set up"}
          </button>
        )}
      </div>
      {error && <p className="text-[11px] text-[#ff453a]">{error}</p>}
    </li>
  );
}
