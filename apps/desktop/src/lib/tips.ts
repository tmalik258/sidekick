// Short tips shown now and then on the quiet hover card.

export const TIPS = [
  'Say "focus for 25 minutes" to hold notifications.',
  "Hold Alt in Ask to see every shortcut.",
  "Type / in Ask for all commands.",
  'Say "turn on hotspot" or "mute": it happens at once.',
  "Copy an error and press Ctrl+Space: Sidekick explains it.",
  'Ask "what\'s taking space" to see what fills your drive.',
  "Click the info button under an answer to see which model answered.",
  "Settings > Memory shows what Sidekick has learned, and lets you forget it.",
  "Rename Sidekick in Settings > Appearance.",
  'Say "what did I do today" for a summary by project.',
];

/** A tip about one time in four, never the same one twice in a row. */
export function pickTip(): string | null {
  try {
    if (Math.random() > 0.25) return null;
    const last = Number(localStorage.getItem("sk-last-tip") ?? "-1");
    let i = Math.floor(Math.random() * TIPS.length);
    if (i === last) i = (i + 1) % TIPS.length;
    localStorage.setItem("sk-last-tip", String(i));
    return TIPS[i];
  } catch {
    return null;
  }
}
