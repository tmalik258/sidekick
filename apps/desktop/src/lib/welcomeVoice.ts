// What a spoken reply means on the welcome: move between steps, hide or skip.
// Anything else gets a short spoken "finish setup first".

export type WelcomeCommand = "next" | "back" | "skip" | "finish" | "hide";

const RULES: [WelcomeCommand, RegExp][] = [
  ["back", /\b(back|previous|go back|last step)\b/],
  ["next", /\bskip (this|it|that|step)\b/],
  ["skip", /\bskip\b/],
  ["hide", /\b(hide|later|not now|minimi[sz]e|go away)\b/],
  ["finish", /\b(start|finish|finished|done|let'?s go|get started|all set)\b/],
  ["next", /\b(next|continue|go on|move on|carry on|okay|ok|yes|yeah|sure|ready|got it)\b/],
];

export function welcomeCommand(text: string): WelcomeCommand | null {
  const t = text.toLowerCase().replace(/[^a-z' ]+/g, " ");
  return RULES.find(([, re]) => re.test(t))?.[0] ?? null;
}

let handler: ((text: string) => void) | null = null;

/** The welcome takes what is heard while it is open; returns the release. */
export function onWelcomeHeard(fn: (text: string) => void): () => void {
  handler = fn;
  return () => {
    if (handler === fn) handler = null;
  };
}

export function welcomeHeard(text: string): boolean {
  if (!handler) return false;
  handler(text);
  return true;
}
