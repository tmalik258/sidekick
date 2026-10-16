// Answers end with "OPTION: ..." lines; the island shows them as choices you
// pick with a click or Alt 1-N, and hides the raw lines. "OPTION_MULTI: ..."
// (or "pick any that apply" in the ask) becomes a checkbox list + Confirm.
// Agents that write "1. **Label:** a, b, or c?" get the same UI via extractChoices.

// Allow list markers, numbering, and light markdown wrappers around OPTION/STAT.
const LINE = /^\s*(?:[-*]|\d+[.)])?\s*\*{0,2}`?option:`?\*{0,2}\s*(.+?)\s*$/i;
const MULTI_LINE = /^\s*(?:[-*]|\d+[.)])?\s*\*{0,2}`?option_multi:`?\*{0,2}\s*(.+?)\s*$/i;
// "STAT: Memory | 14.2 of 16 GB | high" lines become number chips under the answer.
const STAT = /^\s*(?:[-*]|\d+[.)])?\s*\*{0,2}`?stat:`?\*{0,2}\s*(.+?)\s*$/i;
const MAX_STATS = 4;
/** Numbered clarifying item: `1. **Folder sizes:** a, b, or c?` */
const NUMBERED = /^\s*\d+[.)]\s+\*\*(.+?):\*\*\s*(.+?)\s*$/;
const MULTI_HINT =
  /\b(pick any|select all that apply|choose all that apply|one or more|any that apply|multi-?select)\b/i;

export interface Stat {
  label: string;
  value: string;
  hot: boolean;
}

export interface Choices {
  body: string;
  options: string[];
  stats: Stat[];
  /** Checkbox list + Confirm instead of one-tap radio. */
  multi: boolean;
}

const MAX = 6;

export function splitOptions(text: string, streaming = false): Choices {
  const lines = text.split("\n");
  const options: string[] = [];
  const body: string[] = [];
  const stats: Stat[] = [];
  let multi = false;
  for (const line of lines) {
    const multiMatch = line.match(MULTI_LINE);
    const m = line.match(LINE);
    const st = line.match(STAT);
    if (st) {
      const [label, value, flag] = st[1].split("|").map((x) => x.trim());
      // A number chip needs a number; "storage | largest files" is a small
      // model echoing tool names.
      if (label && value && /\d/.test(value) && stats.length < MAX_STATS)
        stats.push({ label, value, hot: /^(high|hot|!)$/i.test(flag ?? "") });
    } else if (multiMatch) {
      multi = true;
      if (options.length < MAX) options.push(cleanOpt(multiMatch[1]));
    } else if (m) {
      if (options.length < MAX) options.push(cleanOpt(m[1]));
    } else {
      body.push(line);
    }
  }
  const joined = body.join("\n");
  if (!multi && MULTI_HINT.test(joined)) multi = true;
  // While streaming, hide a last line that is still turning into "OPTION:".
  if (streaming) {
    const last = (body.at(-1) ?? "").trim().toLowerCase();
    const bare = last.replace(/^[-*]\s*/, "");
    if (
      last &&
      ("option:".startsWith(bare) ||
        "option_multi:".startsWith(bare) ||
        "stat:".startsWith(bare) ||
        bare.startsWith("stat:") ||
        bare.startsWith("option_multi:"))
    )
      body.pop();
  }
  return { body: tidy(body.join("\n")).trimEnd(), options, stats, multi };
}

/** Prefer OPTION / OPTION_MULTI lines; otherwise lift numbered choices. */
export function extractChoices(text: string, streaming = false): Choices {
  const explicit = splitOptions(text, streaming);
  if (explicit.options.length > 0 || streaming) return explicit;
  // A numbered list is only a set of choices when the message asks something.
  if (!asksQuestion(text)) return explicit;
  const options: string[] = [];
  const body: string[] = [];
  for (const line of text.split("\n")) {
    const m = line.match(NUMBERED);
    if (!m) {
      body.push(line);
      continue;
    }
    const title = m[1].trim();
    const parts = splitChoiceList(m[2]);
    if (parts.length < 2) {
      body.push(line);
      continue;
    }
    // Drop the numbered line — choices render as the selectable UI only.
    for (const part of parts) {
      if (options.length >= MAX) break;
      const label = `${title}: ${part}`;
      if (!options.includes(label)) options.push(label);
    }
  }
  if (options.length < 2) return explicit;
  const joined = body.join("\n");
  return {
    body: tidy(joined).trimEnd(),
    options,
    stats: explicit.stats,
    multi: MULTI_HINT.test(joined) || MULTI_HINT.test(text),
  };
}

/** The message ends with a question (ignoring trailing list lines). */
function asksQuestion(text: string): boolean {
  const lines = text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
  return lines.slice(-3).some((l) => /\?\s*\**$/.test(l));
}

function cleanOpt(raw: string): string {
  return raw.replace(/^["']|["']$/g, "").trim();
}

/** Join picked labels into one reply the agent can read. */
export function joinMultiPicks(picks: string[]): string {
  const clean = picks.map((p) => p.trim()).filter(Boolean);
  const first = clean[0];
  if (!first) return "";
  if (clean.length === 1) return first;
  const last = clean[clean.length - 1] ?? "";
  if (clean.length === 2) return `${first} and ${last}`;
  return `${clean.slice(0, -1).join(", ")}, and ${last}`;
}

/** Split "a, b, or c" while keeping commas inside (...). */
function splitChoiceList(raw: string): string[] {
  const s = raw.trim().replace(/\?+\s*$/, "");
  const parts: string[] = [];
  let buf = "";
  let depth = 0;
  const push = () => {
    const t = buf.trim();
    if (t) parts.push(t);
    buf = "";
  };
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (c === "(") depth++;
    if (c === ")") depth = Math.max(0, depth - 1);
    if (depth === 0 && c === ",") {
      push();
      continue;
    }
    if (depth === 0 && /\s/.test(c)) {
      const or = s.slice(i + 1).match(/^or\s+/i);
      if (or) {
        push();
        i += or[0].length;
        continue;
      }
    }
    buf += c;
  }
  push();
  return parts.filter((p) => p.length > 0 && p.length < 140);
}

// Small models open with "Hi! I'm Duskie, your assistant." and emojis.
const EMOJI = /(?:\p{Extended_Pictographic}|[\u{1F3FB}-\u{1F3FF}]|\u200d|\uFE0F)/gu;
// Only at the start of the answer so mid-text strip never eats the space
// before the next sentence (which produced "answered.The").
const INTRO = /^(?:(?:hi|hello|hey)[!,.]?\s*)?i(?:'|\u2019)?m (?:duskie|sidekick)[^.!?\n]*[.!?]+\s*/gi;
const GLUED = /([.!?])([A-Z])/g;

export function tidy(text: string): string {
  return text.replace(EMOJI, "").replace(INTRO, "").replace(GLUED, "$1 $2").replace(/ {2,}/g, " ");
}
