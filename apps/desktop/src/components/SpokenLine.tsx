"use client";

// What Sidekick says on a welcome step, shown word by word as it is heard. Rust
// reports when each sentence starts sounding and how long it lasts (Unix ms,
// so a late-mounting panel still lines up); words inside a sentence are
// spread by length, with a beat after punctuation. Only words heard so far
// are in the layout, so the island grows with the line. With no audio (no
// speakers, or voice failed) the words keep a natural speaking pace on their
// own. Each step speaks once per run; coming back shows it whole.

import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { api, EVENTS, listen } from "@/lib/bridge";
import type { WelcomeSpeech } from "@/lib/types";

/** Pace when nothing is heard: about 3 words a second, like speech. */
const WORD_MS = 330;
/** With no word from the voice at all, pace the words after this long. */
const AUDIO_WAIT_MS = 6000;
/** Audio is on its way (the model may be loading): wait this long at most. */
const PENDING_WAIT_MS = 20_000;

/** Steps already spoken this run; going back to one shows it whole. */
const heard = new Set<number>();

export function wasHeard(step: number): boolean {
  return heard.has(step);
}

function words(text: string): string[] {
  return text.split(/\s+/).filter(Boolean);
}

/** Relative time a word takes: its letters, plus a beat after punctuation. */
function weight(word: string): number {
  const end = word.at(-1) ?? "";
  return word.length + 1 + (/[.!?]/.test(end) ? 4 : /[,;:]/.test(end) ? 2 : 0);
}

/**
 * When each word of `script` appears (Unix ms), from the sentences heard so
 * far. Words not heard yet get Infinity.
 */
export function revealTimes(script: string, speech: WelcomeSpeech | null, pacedFrom: number | null): number[] {
  const all = words(script);
  const times = all.map(() => Number.POSITIVE_INFINITY);
  if (speech && speech.pieces.length > 0) {
    let i = 0;
    for (const piece of speech.pieces) {
      const own = words(piece.text);
      const total = own.reduce((n, w) => n + weight(w), 0) || 1;
      let at = piece.startsAt;
      for (const w of own) {
        if (i >= times.length) break;
        times[i] = at;
        at += (piece.ms * weight(w)) / total;
        i += 1;
      }
    }
    return times;
  }
  if (pacedFrom !== null) {
    let at = pacedFrom;
    for (let i = 0; i < all.length; i += 1) {
      times[i] = at;
      at += (WORD_MS * weight(all[i])) / 6;
    }
  }
  return times;
}

/** When the line is over: the voice's own end, or the last paced word. */
function endTime(times: number[], speech: WelcomeSpeech | null, paced: boolean): number | null {
  if (speech?.endsAt && speech.pieces.length > 0) return speech.endsAt;
  if (paced && times.length > 0) return (times.at(-1) ?? 0) + 600;
  return null;
}

export function SpokenLine({ step, onDone }: { step: number; onDone: () => void }) {
  const [latest, setSpeech] = useState<WelcomeSpeech | null>(null);
  const [pacedFrom, setPacedFrom] = useState<number | null>(null);
  const already = heard.has(step);
  const [shown, setShown] = useState(already ? Number.POSITIVE_INFINITY : 0);
  const done = useRef(already);
  // Only this step's timeline counts; another step's speech may still be
  // the latest one reported.
  const speech = latest?.step === step ? latest : null;
  const onDoneRef = useRef(onDone);
  onDoneRef.current = onDone;

  // The timeline so far, then each update as sentences are queued.
  useEffect(() => {
    let live = true;
    void api
      .voiceWelcome()
      .then((s) => live && setSpeech(s))
      .catch(() => undefined);
    const off = listen(EVENTS.voiceWelcome, (s) => live && setSpeech(s));
    return () => {
      live = false;
      void off.then((f) => f());
    };
  }, []);

  // No audio coming: pace the words ourselves.
  useEffect(() => {
    if (done.current || pacedFrom !== null) return;
    if (speech?.silent) {
      setPacedFrom(Date.now());
      return;
    }
    if (speech && speech.pieces.length > 0) return;
    const wait = speech?.pending ? PENDING_WAIT_MS : AUDIO_WAIT_MS;
    const id = setTimeout(() => setPacedFrom(Date.now()), wait);
    return () => clearTimeout(id);
  }, [speech, pacedFrom]);

  const script = latest?.lines[step] ?? "";
  const list = useMemo(() => words(script), [script]);
  const usePaced = pacedFrom !== null && !(speech && speech.pieces.length > 0);
  const times = useMemo(
    () => revealTimes(script, speech, usePaced ? pacedFrom : null),
    [script, speech, usePaced, pacedFrom],
  );
  const ends = endTime(times, speech, usePaced);

  // One clock for the whole line; state changes only when another word
  // appears, so the paragraph re-renders once per word, not per frame.
  useEffect(() => {
    if (done.current || list.length === 0) return;
    let frame = 0;
    const tick = () => {
      const now = Date.now();
      const count = times.filter((t) => t <= now).length;
      // A word once shown stays shown, even if the audio's own timing
      // (arriving after a paced start) would reveal it a little later.
      setShown((n) => Math.max(n, count));
      if (ends !== null && now >= ends) {
        done.current = true;
        heard.add(step);
        onDoneRef.current();
        return;
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [times, ends, list.length, step]);

  // Already heard earlier in this run: tell the step at once.
  useEffect(() => {
    if (done.current) onDoneRef.current();
  }, []);

  // Only the words heard so far are in the layout, so the island grows
  // with the line instead of opening at its full height.
  const visible = list.slice(0, Math.min(shown, list.length));
  return (
    <p className="font-display text-[17px] leading-snug tracking-[-0.01em] text-white">
      {/* Screen readers get the whole line at once. */}
      <span className="sr-only">{script}</span>
      {visible.map((w, i) => (
        // Words are fixed for the line; the index is their identity.
        // biome-ignore lint/suspicious/noArrayIndexKey: stable list
        <Fragment key={i}>
          <span aria-hidden="true" className="spoken-word inline-block">
            {w}
          </span>{" "}
        </Fragment>
      ))}
    </p>
  );
}
