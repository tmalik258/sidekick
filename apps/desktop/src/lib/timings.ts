// How fast Ask feels, measured in the island: open to ready and Enter to the
// first word. Each timing goes to the Rust core, which logs it on this PC and
// sends it back to the overlay (with the voice timing it measures itself).

import { api } from "./bridge";

const sent = new Map<string, number>();

export const timings = {
  /** Ask opened: `sentAt` is when Rust was asked to open it (ms since 1970). */
  opened(sentAt: number | undefined) {
    if (!sentAt) return;
    // Two frames: the panel has painted and the input can take keys.
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        const ms = Date.now() - sentAt;
        if (ms >= 0) void api.timingRecord("open_to_ready", ms).catch(() => {});
      }),
    );
  },
  /** A question was sent. */
  sent(id: string) {
    sent.set(id, performance.now());
  },
  /** The first words of chat `id` arrived (later calls do nothing). */
  firstWord(id: string) {
    const at = sent.get(id);
    if (at === undefined) return;
    sent.delete(id);
    void api.timingRecord("enter_to_first_word", Math.round(performance.now() - at)).catch(() => {});
  },
  /** Chat `id` ended (maybe with no words). */
  done(id: string) {
    sent.delete(id);
  },
};

export const TIMING_LABELS = {
  open_to_ready: "Open to ready",
  enter_to_first_word: "Enter to first word",
  speech_to_first_sound: "End of speech to first sound",
} as const;

export const TIMING_TARGETS = {
  open_to_ready: 100,
  enter_to_first_word: 1500,
  speech_to_first_sound: 700,
} as const;

export function median(values: number[]): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : Math.round((sorted[mid - 1] + sorted[mid]) / 2);
}
