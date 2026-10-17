"use client";

// Stale-while-revalidate for data the island shows: the last answer paints
// at once (kept in memory and in localStorage). Remounts reuse that cache
// without fetching again, Hide then open welcome must not look like
// "Check again". An explicit refresh() (Check again, after a Run) fetches
// in the background and only marks busy when nothing was on screen yet,
// or when the caller asks for it.

import { useCallback, useEffect, useRef, useState } from "react";

const memory = new Map<string, string>();
const PREFIX = "sidekick:cache:";
const subscribers = new Map<string, Set<(json: string) => void>>();
const pending = new Map<string, Promise<unknown>>();

function loadOnce<T>(key: string, load: () => Promise<T>): Promise<T> {
  const active = pending.get(key);
  if (active) return active as Promise<T>;
  const request = Promise.resolve().then(load);
  pending.set(key, request);
  const clear = () => {
    pending.delete(key);
  };
  void request.then(clear, clear);
  return request;
}

/** Bump when setup-status shape or copy changes so stale localStorage is ignored. */
export const SETUP_STATUS_CACHE_KEY = "setup-status:v2";

/** Loads fresh data even when a cache entry exists (e.g. entering Connect). */
export function revalidate<T>(key: string, load: () => Promise<T>): void {
  // Settings may have changed while an older probe was still running.
  const active = pending.get(key);
  const next = active ? active.catch(() => undefined).then(() => loadOnce(key, load)) : loadOnce(key, load);
  void next.then((next) => writeJson(key, JSON.stringify(next))).catch(() => undefined);
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
  for (const notify of subscribers.get(key) ?? []) notify(json);
}

/** Load into the cache if empty, e.g. while welcome speech plays on the prior step. */
export function prefetch<T>(key: string, load: () => Promise<T>): void {
  if (readJson(key) !== null) return;
  void loadOnce(key, load)
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
 * a cache hit does not refetch unless `revalidateOnMount` is set. `every`
 * refreshes on an interval while mounted. Writes update all mounted consumers.
 */
export function useCached<T>(
  key: string,
  load: () => Promise<T>,
  every?: number,
  revalidateOnMount = false,
): Cached<T> {
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
        set(await loadOnce(key, loadRef.current));
      } finally {
        if (showBusy) setRefreshing(false);
      }
    },
    [key, set],
  );

  useEffect(() => {
    const notify = (json: string) => {
      if (json === shown.current) return;
      shown.current = json;
      setData(JSON.parse(json) as T);
    };
    const listeners = subscribers.get(key) ?? new Set<(json: string) => void>();
    listeners.add(notify);
    subscribers.set(key, listeners);
    // Another panel may have refreshed this key since we mounted.
    const json = readJson(key);
    if (json !== null && json !== shown.current) {
      shown.current = json;
      setData(JSON.parse(json) as T);
    }
    // Revalidation keeps cached rows visible while the fresh answer loads.
    if (shown.current === null || revalidateOnMount) {
      void refresh().catch(() => undefined);
    }
    const id = every ? setInterval(() => void refresh().catch(() => undefined), every) : undefined;
    return () => {
      if (id !== undefined) clearInterval(id);
      listeners.delete(notify);
      if (listeners.size === 0) subscribers.delete(key);
    };
  }, [key, every, refresh, revalidateOnMount]);

  return { data, refreshing, refresh, set };
}
