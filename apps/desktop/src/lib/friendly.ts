// Raw errors (Rust, HTTP, shell) turned into a sentence that says what to do
// next. Unknown errors pass through, minus noise like "Error:" prefixes.

const RULES: [RegExp, string][] = [
  [/timed? ?out|timeout|deadline/i, "That took too long. Check your internet connection and try again."],
  [
    /network|dns|connection (refused|reset)|could not connect|failed to connect|error sending request/i,
    "Could not reach the service. Check your internet connection and try again.",
  ],
  [/\b401\b|unauthori[sz]ed|invalid api key|expired token|not signed in/i, "The sign-in has expired. Connect again."],
  [
    /\b403\b|forbidden|permission denied|access is denied/i,
    "Windows or the service refused access. Try again, or run it yourself.",
  ],
  [/\b404\b|not found/i, "It could not be found. It may have moved or been removed."],
  [/\b429\b|rate limit|too many requests/i, "Too many tries at once. Wait a minute and try again."],
  [/\b5\d\d\b|service unavailable|bad gateway/i, "The service is having trouble. Try again in a minute."],
  [
    /is not installed|not recognized as an internal or external command|cannot find the file/i,
    "That tool is not installed yet. Install it from Settings > Home.",
  ],
];

let offline = false;
/** Kept in sync with the connection, so failures while offline say so. */
export function setOffline(value: boolean) {
  offline = value;
}

const NETWORK =
  /timed? ?out|timeout|deadline|network|dns|connection (refused|reset)|could not connect|failed to connect|error sending request/i;

export function friendlyError(raw: unknown): string {
  const text = String(raw)
    .replace(/^(Error|error|Failed):\s*/g, "")
    .trim();
  if (offline && NETWORK.test(text)) return "You're offline. Try again once the internet is back.";
  const hit = RULES.find(([re]) => re.test(text));
  return hit ? hit[1] : text;
}
