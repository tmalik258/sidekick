"use client";

// What a screen reader hears: state changes (listening, working, done, an
// agent waiting) and answers a sentence or two at a time as they stream,
// instead of every token or nothing at all.

import { useEffect, useRef, useState } from "react";
import { useAgents } from "@/lib/agents";
import { useSidekick } from "@/lib/store";
import type { MascotState } from "@/lib/types";

const STATE_WORDS: Partial<Record<MascotState, string>> = {
  listening: "Listening",
  working: "Working",
  success: "Done",
  error: "Something went wrong",
};

/** The next part of a streaming answer worth reading out: up to the last
 * sentence end past `said`, or everything left once the answer is done.
 * Returns the chunk and how much of `text` has now been said. */
export function nextChunk(text: string, said: number, done: boolean): [string, number] {
  if (text.length <= said) return ["", said];
  if (done) return [text.slice(said).trim(), text.length];
  const rest = text.slice(said);
  let end = -1;
  for (const m of rest.matchAll(/[.!?:;](\s|$)|\n\n/g)) end = (m.index ?? 0) + m[0].length;
  // Short bursts wait for more, so a reader is not interrupted every word.
  if (end < 0 || end < 40) return ["", said];
  return [rest.slice(0, end).trim(), said + end];
}

/** Markdown marks read as words otherwise ("asterisk asterisk"). */
const plain = (t: string) => t.replace(/[*_`#>|]+/g, "").replace(/\[([^\]]+)\]\([^)]+\)/g, "$1");

export function Announcer() {
  const [line, setLine] = useState("");
  const said = useRef({ id: "", n: 0 });
  const mascot = useSidekick((s) => s.mascot);
  const last = useSidekick((s) => s.turns[s.turns.length - 1]);
  const turns = useSidekick((s) => s.turns.length);
  const waiting = useAgents((s) => s.sessions.find((x) => x.question));

  useEffect(() => {
    const word = STATE_WORDS[mascot];
    if (word) setLine(word);
  }, [mascot]);

  useEffect(() => {
    if (last?.role !== "assistant") return;
    const id = `${turns}`;
    if (said.current.id !== id) said.current = { id, n: 0 };
    if (last.error) {
      setLine(last.error);
      return;
    }
    const [chunk, n] = nextChunk(last.content, said.current.n, !last.streaming);
    said.current.n = n;
    if (chunk) setLine(plain(chunk));
  }, [last, turns]);

  useEffect(() => {
    if (waiting?.question) setLine(`${waiting.agent} needs you: ${waiting.question.label}`);
  }, [waiting?.agent, waiting?.question]);

  return (
    <div className="sr-only" role="status" aria-live="polite" aria-atomic="true">
      {line}
    </div>
  );
}
