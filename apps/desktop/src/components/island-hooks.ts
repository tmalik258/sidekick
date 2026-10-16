"use client";

import { useEffect, useState } from "react";
import type { Suggestion } from "@/lib/types";
import { choose, dismiss } from "./IslandParts";
import { HOVER_IN_MS, HOVER_OUT_MS } from "./island-layout";

export function useIntent(raw: boolean, inMs = HOVER_IN_MS): boolean {
  const [value, setValue] = useState(false);
  useEffect(() => {
    const id = setTimeout(() => setValue(raw), raw ? inMs : HOVER_OUT_MS);
    return () => clearTimeout(id);
  }, [raw, inMs]);
  return value;
}

/** Number keys pick an option, Esc dismisses (when the island has focus). */
export function useSuggestionKeys(suggestion: Suggestion | null) {
  useEffect(() => {
    if (!suggestion) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey) return; // Alt+N is a global shortcut handled in Rust
      if (e.key === "Escape") dismiss(suggestion);
      const n = Number(e.key);
      if (n >= 1 && n <= suggestion.options.length) choose(suggestion, n - 1, e.shiftKey);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [suggestion]);
}

/** Stops Chromium/WebView2 browser chrome shortcuts that have no place here. */
export function useBlockBrowserKeys() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      if (e.key.toLowerCase() !== "j") return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);
}

/** "24:10" left in Focus mode, ticking each second, or null. */
export function useFocusLeft(until: number | null): string | null {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (until === null) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [until]);
  if (until === null || until <= now) return null;
  const secs = Math.ceil((until - now) / 1000);
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const two = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${two(m)}:${two(s)}` : `${m}:${two(s)}`;
}
