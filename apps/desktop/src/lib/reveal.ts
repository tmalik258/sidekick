// Shows a streaming answer word by word at a steady reading pace, whatever
// size the pieces arrive in (Claude Code and Codex send whole sentences;
// local models send a word or two). It never lags far behind: the further
// behind it is, the faster it goes.

import { useReducedMotion } from "motion/react";
import { useEffect, useRef, useState } from "react";

/** Words a second when nearly caught up. */
const BASE_RATE = 40;
/** Seconds to catch up with a backlog: rate grows with the words waiting. */
const CATCH_UP = 0.6;

/** Where the next `count` words end in `text`, from `from`. */
export function advanceWords(text: string, from: number, count: number): number {
  let i = from;
  for (let n = 0; n < count && i < text.length; n++) {
    while (i < text.length && /\s/.test(text[i])) i++;
    while (i < text.length && !/\s/.test(text[i])) i++;
  }
  return i;
}

function wordsIn(text: string): number {
  const m = text.match(/\S+/g);
  return m ? m.length : 0;
}

/**
 * The part of `text` to show now. `live` is true while the answer is still
 * arriving or being revealed; an answer opened from history shows whole.
 */
export function useReveal(text: string, live: boolean): string {
  const reduced = useReducedMotion() ?? false;
  const [shown, setShown] = useState(() => (live ? 0 : text.length));
  const target = useRef(text);
  target.current = text;
  const pos = useRef(shown);
  pos.current = shown;
  const behind = shown < text.length;

  useEffect(() => {
    if (reduced || (!live && !behind)) return;
    let raf = 0;
    let last = performance.now();
    let carry = 0;
    const tick = (now: number) => {
      const full = target.current;
      const at = pos.current;
      if (at < full.length) {
        const waiting = wordsIn(full.slice(at));
        const rate = Math.max(BASE_RATE, waiting / CATCH_UP);
        carry += ((now - last) / 1000) * rate;
        const step = Math.floor(carry);
        if (step > 0) {
          carry -= step;
          const next = advanceWords(full, at, step);
          pos.current = next;
          setShown(next);
        }
      } else {
        carry = 0;
      }
      last = now;
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [reduced, live, behind]);

  if (reduced) return text;
  return text.slice(0, Math.min(shown, text.length));
}
