"use client";

import { useCallback, useEffect, useRef, useState } from "react";

/** Re-renders every `ms` so time-based labels (pause countdown) stay fresh. */
export function useNow(ms: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(id);
  }, [ms]);
  return now;
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
