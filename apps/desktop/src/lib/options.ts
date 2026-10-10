// Answers end with "OPTION: ..." lines; the island shows them as chips you
// pick with a click or Alt 1-3, and hides the raw lines.

const LINE = /^\s*[-*]?\s*option:\s*(.+?)\s*$/i;
// "STAT: Memory | 14.2 of 16 GB | high" lines become number chips under the answer.
const STAT = /^\s*[-*]?\s*stat:\s*(.+?)\s*$/i;
const MAX_STATS = 4;

export interface Stat {
  label: string;
  value: string;
  hot: boolean;
}
const MAX = 3;

export function splitOptions(text: string, streaming = false): { body: string; options: string[]; stats: Stat[] } {
  const lines = text.split("\n");
  const options: string[] = [];
  const body: string[] = [];
  const stats: Stat[] = [];
  for (const line of lines) {
    const m = line.match(LINE);
    const st = line.match(STAT);
    if (st) {
      const [label, value, flag] = st[1].split("|").map((x) => x.trim());
      // A number chip needs a number; "storage | largest files" is a small
      // model echoing tool names.
      if (label && value && /\d/.test(value) && stats.length < MAX_STATS)
        stats.push({ label, value, hot: /^(high|hot|!)$/i.test(flag ?? "") });
    } else if (m) {
      if (options.length < MAX) options.push(m[1].replace(/^["']|["']$/g, ""));
    } else {
      body.push(line);
    }
  }
  // While streaming, hide a last line that is still turning into "OPTION:".
  if (streaming) {
    const last = (body.at(-1) ?? "").trim().toLowerCase();
    const bare = last.replace(/^[-*]\s*/, "");
    if (last && ("option:".startsWith(bare) || "stat:".startsWith(bare) || bare.startsWith("stat:"))) body.pop();
  }
  return { body: tidy(body.join("\n")).trimEnd(), options, stats };
}

// Small models open with "Hi! I'm Sidekick, your assistant." and emojis.
const EMOJI = /(?:\p{Extended_Pictographic}|[\u{1F3FB}-\u{1F3FF}]|\u200d|\uFE0F)/gu;
const INTRO = /(?:^|\s)(?:(?:hi|hello|hey)[!,.]?\s*)?i(?:'|\u2019)?m sidekick[^.!?\n]*[.!?]/gi;

export function tidy(text: string): string {
  return text.replace(EMOJI, "").replace(/ {2,}/g, " ").replace(INTRO, "");
}
