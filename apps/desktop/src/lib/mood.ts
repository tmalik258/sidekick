// Moods: a short-lived expression on top of the mascot's state, set by what
// just happened (Claude finished, you said thanks, a password was saved).
// The state machine says what Sidekick is doing; a mood says how it feels
// about it, for a few seconds.

import type { Expression } from "@/components/orb/expressions";

export interface Mood {
  id: Expression;
  until: number;
}

/** How long a mood from a suggestion lasts. */
export const SUGGESTION_MOOD_MS = 4000;

/** Moods for suggestions, by skill id. Skills not listed keep the state's look. */
const BY_SKILL: Record<string, Expression> = {
  "dev.claude-finished": "delight",
  "dev.codex-finished": "delight",
  "dev.claude-needs-you": "curious",
  "dev.claude-permission": "curious",
  "calendar.meeting-soon": "surprised",
  "system.battery-low": "worried",
  "system.disk-low": "worried",
  "system.memory-high": "worried",
  "system.health": "worried",
  "system.day-summary": "proud",
  "system.week-summary": "proud",
  "system.focus": "focus",
  "system.late-night": "sleepy",
  "system.morning-brief": "hello",
  "system.back": "hello",
};

export function moodForSkill(skillId: string): Expression | null {
  return BY_SKILL[skillId] ?? null;
}

/** "thanks", "thank you", "thx", "ty", "tysm" said to Sidekick. */
export function isThanks(text: string): boolean {
  return /\b(thanks|thank you|thank u|thx|tysm|ty)\b/i.test(text) && text.length < 80;
}

const HELLO_KEY = "sidekick-hello-day";

/** True once per calendar day: the first time Sidekick is seen that day. */
export function firstToday(now = new Date()): boolean {
  const day = now.toDateString();
  try {
    if (localStorage.getItem(HELLO_KEY) === day) return false;
    localStorage.setItem(HELLO_KEY, day);
  } catch {
    return false;
  }
  return true;
}
