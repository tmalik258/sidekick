"use client";

// Stale-while-revalidate for data the island shows: the last answer paints
// at once (kept in memory and in localStorage), a fresh one is fetched in the
// background, and state only changes when the data really changed. So a
// refresh never blanks a panel or re-renders rows whose data is the same.

import { useCallback, useEffect, useRef, useState } from "react";

const memory = new Map<string, string>();
const PREFIX = "sidekick:cache:";

function readJson(key: string): string | null {
  const hit = memory.get(key);
  if (hit !== undefined) return hit;
  try {
    const stored = localStorage.getItem(PREFIX + key);
    if (stored !== null) memory.set(key, stored);
    return stored;
  } catch {
    return null;
  }
}

function writeJson(key: string, json: string) {
  memory.set(key, json);
  try {
    localStorage.setItem(PREFIX + key, json);
  } catch {
    // Storage can be full or blocked; memory still has it.
  }
}

/** The last value stored under `key`, if any. */
export function cached<T>(key: string): T | null {
  const json = readJson(key);
  if (json === null) return null;
  try {
    return JSON.parse(json) as T;
  } catch {
    return null;
  }
}

export interface Cached<T> {
  /** Last known data; null only before the very first answer. */
  data: T | null;
  /** A fetch is in flight (data stays on screen meanwhile). */
  refreshing: boolean;
  refresh: () => Promise<void>;
  /** Puts a known value in place, e.g. after an action returned it. */
  set: (next: T) => void;
}

/**
 * Loads `key` with `load`, showing the cached value meanwhile. `every`
 * refreshes on an interval while mounted.
 */
export function useCached<T>(key: string, load: () => Promise<T>, every?: number): Cached<T> {
  const [data, setData] = useState<T | null>(() => cached<T>(key));
  const [refreshing, setRefreshing] = useState(false);
  const shown = useRef<string | null>(readJson(key));
  const loadRef = useRef(load);
  loadRef.current = load;

  const set = useCallback(
    (next: T) => {
      const json = JSON.stringify(next);
      writeJson(key, json);
      // Same data as on screen: keep the old object so nothing re-renders.
      if (json === shown.current) return;
      shown.current = json;
      setData(next);
    },
    [key],
  );

  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      set(await loadRef.current());
    } finally {
      setRefreshing(false);
    }
  }, [set]);

  useEffect(() => {
    // Another panel may have refreshed this key since we mounted.
    const json = readJson(key);
    if (json !== null && json !== shown.current) {
      shown.current = json;
      setData(JSON.parse(json) as T);
    }
    void refresh().catch(() => undefined);
    if (!every) return;
    const id = setInterval(() => void refresh().catch(() => undefined), every);
    return () => clearInterval(id);
  }, [key, every, refresh]);

  return { data, refreshing, refresh, set };
}
