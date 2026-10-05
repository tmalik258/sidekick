"use client";

// Stale-while-revalidate for data the island shows: the last answer paints
// at once (kept in memory and in localStorage). Remounts reuse that cache
// without fetching again — Hide then open welcome must not look like
// "Check again". An explicit refresh() (Check again, after a Run) fetches
// in the background and only marks busy when nothing was on screen yet,
// or when the caller asks for it.

import { useCallback, useEffect, useRef, useState } from "react";

const memory = new Map<string, string>();
const PREFIX = "sidekick:cache:";

/** Bump when setup-status shape or copy changes so stale localStorage is ignored. */
export const SETUP_STATUS_CACHE_KEY = "setup-status:v2";

/** Loads fresh data even when a cache entry exists (e.g. entering Connect). */
export function revalidate<T>(key: string, load: () => Promise<T>): void {
  void load()
    .then((next) => writeJson(key, JSON.stringify(next)))
    .catch(() => undefined);
}

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

/** Load into the cache if empty — e.g. while welcome speech plays on the prior step. */
export function prefetch<T>(key: string, load: () => Promise<T>): void {
  if (readJson(key) !== null) return;
  void load()
    .then((next) => writeJson(key, JSON.stringify(next)))
    .catch(() => undefined);
}

/** Write a known value so the next mount (or a quiet remount) paints it. */
export function putCached<T>(key: string, value: T): void {
  writeJson(key, JSON.stringify(value));
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
  /** A fetch is in flight and the UI should show checking (no data yet, or asked). */
  refreshing: boolean;
  /** Fetches again. Pass `{ busy: true }` for Check again so the list pulses. */
  refresh: (opts?: { busy?: boolean }) => Promise<void>;
  /** Puts a known value in place, e.g. after an action returned it. */
  set: (next: T) => void;
}

/**
 * Loads `key` with `load`, showing the cached value meanwhile. Remount with
 * a cache hit does not refetch. `every` refreshes on an interval while mounted.
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

  const refresh = useCallback(
    async (opts?: { busy?: boolean }) => {
      const showBusy = opts?.busy === true || shown.current === null;
      if (showBusy) setRefreshing(true);
      try {
        set(await loadRef.current());
      } finally {
        if (showBusy) setRefreshing(false);
      }
    },
    [set],
  );

  useEffect(() => {
    // Another panel may have refreshed this key since we mounted.
    const json = readJson(key);
    if (json !== null && json !== shown.current) {
      shown.current = json;
      setData(JSON.parse(json) as T);
    }
    // First time only: a remount with cache must not look like Check again.
    if (shown.current === null) {
      void refresh().catch(() => undefined);
    }
    if (!every) return;
    const id = setInterval(() => void refresh().catch(() => undefined), every);
    return () => clearInterval(id);
  }, [key, every, refresh]);

  return { data, refreshing, refresh, set };
}
