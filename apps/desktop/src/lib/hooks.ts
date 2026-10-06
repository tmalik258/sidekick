"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./bridge";

/** True while Alt is held on its own: key badges show, like Windows ribbons. */
export function useAltHeld(): boolean {
  const [held, setHeld] = useState(false);
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.key === "Alt") setHeld(true);
      else if (!e.altKey) setHeld(false);
    };
    const up = (e: KeyboardEvent) => {
      if (e.key === "Alt" || !e.altKey) setHeld(false);
    };
    const off = () => setHeld(false);
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    window.addEventListener("blur", off);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", off);
    };
  }, []);
  return held;
}

/** Re-renders every `ms` so time-based labels (pause countdown) stay fresh. */
export function useNow(ms: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(id);
  }, [ms]);
  return now;
}

/**
 * Windows Do Not Disturb state. Optional `autoEnable` turns it on once when
 * the OS reports off (welcome Extras); Settings leaves that false.
 */
export function useDnd(opts?: { onError?: (e: string) => void; autoEnable?: boolean }): {
  on: boolean;
  busy: boolean;
  set: (want: boolean) => void;
} {
  const [on, setOn] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const autoEnable = opts?.autoEnable ?? false;
  const onErrorRef = useRef(opts?.onError);
  onErrorRef.current = opts?.onError;
  const autoDone = useRef(false);
  const busyRef = useRef(false);

  useEffect(() => {
    let cancelled = false;
    void api.dndGet().then((now) => {
      if (cancelled) return;
      if (autoEnable && !autoDone.current && now !== true) {
        autoDone.current = true;
        setBusy(true);
        setOn(true);
        void api
          .dndSet(true)
          .catch((e) => onErrorRef.current?.(String(e)))
          .then(() => api.dndGet())
          .then((after) => {
            if (!cancelled) setOn(after ?? true);
          })
          .finally(() => {
            if (!cancelled) setBusy(false);
          });
        return;
      }
      setOn(now ?? false);
    });
    return () => {
      cancelled = true;
    };
  }, [autoEnable]);

  const set = useCallback((want: boolean) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setOn(want);
    void api
      .dndSet(want)
      .catch((e) => onErrorRef.current?.(String(e)))
      .then(() => api.dndGet())
      .then((now) => setOn(now ?? want))
      .finally(() => {
        busyRef.current = false;
        setBusy(false);
      });
  }, []);

  return { on: on ?? false, busy, set };
}

/** Within this of an edge counts as at it (sub-pixel scroll positions). */
const EDGE_SLACK = 2;
/** Only fade when more than this is cut off — matches --fade-bottom in CSS. */
const FADE_BAND = 20;

/**
 * A callback ref for a scroller: sets `data-fade-top` while there is content
 * above, and `data-fade-bottom` while there is content below, so its edges
 * fade only when something is actually cut off. Follows scrolling, resizes
 * and content growing in place.
 */
export function useScrollEdge(): (el: HTMLElement | null) => void {
  const cleanup = useRef<(() => void) | null>(null);
  return useCallback((el: HTMLElement | null) => {
    cleanup.current?.();
    cleanup.current = null;
    if (!el) return;
    let raf = 0;
    const measure = () => {
      const top = el.scrollTop > EDGE_SLACK;
      // Need more than the fade band cut off, or a settled short panel keeps a
      // black wash over the last lines (welcome Extras after the footer mounts).
      const below = el.scrollHeight - el.scrollTop - el.clientHeight;
      const bottom = below > FADE_BAND;
      el.toggleAttribute("data-fade-top", top);
      el.toggleAttribute("data-fade-bottom", bottom);
    };
    const update = () => {
      cancelAnimationFrame(raf);
      // Two frames: footer / spoken reveal often land after the first paint.
      raf = requestAnimationFrame(() => {
        raf = requestAnimationFrame(measure);
      });
    };
    const sizes = new ResizeObserver(update);
    const watchChildren = () => {
      sizes.disconnect();
      sizes.observe(el);
      for (const child of el.children) sizes.observe(child);
    };
    const children = new MutationObserver(() => {
      watchChildren();
      update();
    });
    watchChildren();
    // class/style: Spoken toggles `hidden` without childList changes.
    children.observe(el, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["class", "style", "hidden"],
    });
    el.addEventListener("scroll", update, { passive: true });
    update();
    cleanup.current = () => {
      cancelAnimationFrame(raf);
      sizes.disconnect();
      children.disconnect();
      el.removeEventListener("scroll", update);
    };
  }, []);
}
