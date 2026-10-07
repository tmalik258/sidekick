"use client";

// Ask's states besides a normal answer: a failure that says what happened
// and offers the fix, and the first run before any model is set up.

import { useEffect, useRef, useState } from "react";
import { handOff } from "@/lib/agents";
import { api } from "@/lib/bridge";
import { useCached } from "@/lib/cache";
import { askWhenOnline, retryLast, setAsk, setAskModel, useSidekick } from "@/lib/store";
import type { SetupItem, SetupStatus, Turn } from "@/lib/types";
import { useAgentName } from "./Chat";

interface Failure {
  title: string;
  detail: string;
  /** The fix, when there is one Sidekick can do. */
  fix?: { label: string; run: () => Promise<void> };
}

/** Waits until a model can answer (after starting Ollama), up to `ms`. */
async function untilReady(ms: number): Promise<boolean> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const status = await api.aiStatus().catch(() => []);
    if (status.some((p) => p.available && p.id !== "semif")) return true;
    await new Promise((r) => setTimeout(r, 1000));
  }
  return false;
}

/** What went wrong, in words, and the fix. */
export function explain(error: string, provider: string | null | undefined): Failure {
  const e = error.toLowerCase();
  if (
    /11434|ollama|local model is not running|no local model is running/.test(e) ||
    (provider === "local" && /connect|refused|error sending request/.test(e))
  ) {
    return {
      title: "The local model is not running",
      detail: "Ollama is installed but stopped. Starting it takes a few seconds.",
      fix: {
        label: "Start Ollama",
        run: async () => {
          await api.setupRun("ollama");
          if (await untilReady(20_000)) retryLast();
        },
      },
    };
  }
  if (/too old|unknown option|unexpected argument|unrecognized (option|argument|subcommand)|method not found/.test(e)) {
    const codex = provider === "codex" || /codex/.test(e);
    return {
      title: `${codex ? "Codex" : "Claude Code"} needs an update`,
      detail: `This version is too old for Sidekick. Run ${codex ? "npm install -g @openai/codex@latest" : "claude update"} in a terminal, then retry.`,
    };
  }
  if (/no model found|pull a model|ollama pull/.test(e)) {
    return {
      title: "No local model downloaded yet",
      detail: "Ollama is running but has no chat model yet. Sidekick downloads one that suits this PC.",
      fix: { label: "Download it", run: () => api.setupRun("ollama_chat") },
    };
  }
  if (/not logged in|log in|login|sign in|unauthori|401|invalid api key|authentication/.test(e)) {
    return {
      title: "Sign-in needed",
      detail: "The model you picked needs you to sign in again.",
      fix: {
        label: "Open AI settings",
        run: async () => setAsk({ view: "settings", settingsTab: "ai" }),
      },
    };
  }
  if (/did not answer in|took over \d+ minutes/.test(e)) {
    return {
      title: "That model got stuck",
      detail: "Sidekick stopped it and tried the others. Try again, or pick a different model.",
    };
  }
  if (/claude code usage limit/.test(e)) {
    return {
      title: "Claude Code's usage is used up",
      detail: "Sidekick tried your other models too. Ask the model on this PC, or try again once it resets.",
      fix: {
        label: "Ask the local model",
        run: async () => {
          setAskModel("local");
          retryLast();
        },
      },
    };
  }
  if (/rate limit|429|usage limit|quota|overloaded|529/.test(e)) {
    return {
      title: "That model is busy or out of quota",
      detail: "Ask the model on this PC instead, or try again in a little while.",
      fix: {
        label: "Ask the local model",
        run: async () => {
          setAskModel("local");
          retryLast();
        },
      },
    };
  }
  if (/no ai provider|no ai set up/.test(e)) {
    return { title: "No AI is set up yet", detail: "Pick how Sidekick answers below." };
  }
  if (/timed out|dns|network|offline|internet/.test(e)) {
    return {
      title: "The internet is not reachable",
      detail: "The answer needs the web. Sidekick can ask again when you are back online.",
      fix: { label: "Ask when I'm back online", run: async () => askWhenOnline() },
    };
  }
  return { title: "That did not work", detail: error };
}

/** A failed answer: what happened, the fix (Enter), the agent (Ctrl Enter)
 * and Retry (Alt R). */
export function FailureCard({ turn, turns }: { turn: Turn; turns: Turn[] }) {
  const f = explain(turn.error ?? "", turn.provider);
  const agent = useAgentName();
  const [busy, setBusy] = useState<string | null>(null);
  const fix = f.fix;
  const runFix = useRef<() => void>(() => undefined);
  runFix.current = () => {
    if (!fix || busy) return;
    setBusy(`${fix.label}...`);
    void fix
      .run()
      .catch((e: unknown) => setBusy(String(e)))
      .finally(() => setBusy((b) => (b?.endsWith("...") ? null : b)));
  };
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      const empty = !(e.target instanceof HTMLInputElement) || e.target.value.trim() === "";
      if (e.key === "Enter" && !e.ctrlKey && !e.altKey && empty) {
        e.preventDefault();
        runFix.current();
      } else if (e.altKey && !e.ctrlKey && e.key.toLowerCase() === "r") {
        e.preventDefault();
        retryLast();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);
  const toAgent = () => {
    const messages = turns.filter((t) => !t.error && t.content.trim()).map(({ role, content }) => ({ role, content }));
    void handOff(messages, f.title);
  };
  return (
    <div className="ak-body">
      <div className="ak-err ak-in" role="alert">
        <span className="font-semibold text-[13.5px]">{f.title}</span>
        <span className="text-[12.5px] text-[rgb(235_235_245/0.6)]">{busy ?? f.detail}</span>
      </div>
      <div className="ak-chips">
        {fix && (
          <button type="button" disabled={!!busy} onClick={() => runFix.current()} className="ak-chip primary chip">
            {fix.label} <kbd>Enter</kbd>
          </button>
        )}
        {agent && turn.provider !== "claude_code" && turn.provider !== "codex" && (
          <button type="button" onClick={toAgent} className="ak-chip chip">
            Ask {agent} instead <kbd>Ctrl Enter</kbd>
          </button>
        )}
        <button type="button" onClick={() => retryLast()} className="ak-chip chip">
          Retry{" "}
          <kbd>
            <i className="alt-pre">Alt </i>R
          </kbd>
        </button>
      </div>
    </div>
  );
}

/** First run without a model: apps, files and commands already work; pick
 * how questions get answered. */
export function FirstRun() {
  const { data: setup, refresh } = useCached<SetupStatus>("setup-status", api.setupStatus);
  const [running, setRunning] = useState<string | null>(null);
  // While something installs or downloads, check again now and then.
  useEffect(() => {
    if (!running) return;
    const id = setInterval(() => void refresh().catch(() => undefined), 3000);
    return () => clearInterval(id);
  }, [running, refresh]);
  const item = (id: string) => setup?.items.find((i) => i.id === id);
  const ollama = item("ollama");
  const chat = item("ollama_chat");
  const pcReady = !!ollama?.done && !!chat?.done;
  const run = (i: SetupItem | undefined) => {
    if (!i) return;
    setRunning(i.id);
    void api.setupRun(i.id).catch(() => setRunning(null));
  };
  // This PC needs Ollama first, then its model.
  const pcStep = !ollama?.done ? ollama : chat;
  const options: {
    id: string;
    title: string;
    note: string;
    action: string;
    recommended?: boolean;
    go: () => void;
    done?: boolean;
  }[] = [
    {
      id: "pc",
      title: "This PC",
      note: "Free and private. Installs Ollama and a model picked for this PC.",
      action: pcReady ? "Ready" : running === pcStep?.id ? "Setting up..." : (pcStep?.action ?? "Set up"),
      recommended: true,
      go: () => run(pcStep),
      done: pcReady,
    },
    {
      id: "claude_code",
      title: "Claude Code",
      note: `Uses your Claude plan. ${item("claude_code")?.done ? "Found on this PC." : "Not installed."}`,
      action: item("claude_code")?.done ? "Use it" : "Install",
      go: () => (item("claude_code")?.done ? setAskModel("claude_code") : run(item("claude_code"))),
    },
    {
      id: "codex",
      title: "Codex",
      note: `Uses your ChatGPT plan. ${item("codex")?.done ? "Found on this PC." : "Not installed."}`,
      action: item("codex")?.done ? "Use it" : "Install",
      go: () => (item("codex")?.done ? setAskModel("codex") : run(item("codex"))),
    },
    {
      id: "anthropic",
      title: "Anthropic API",
      note: "Pay per answer with your own key.",
      action: "Add key",
      go: () => setAsk({ view: "settings", settingsTab: "ai" }),
    },
  ];
  return (
    <div className="ak-body ak-in">
      <div className="grid gap-1 px-1">
        <p className="text-[14.5px] font-semibold">Choose how Sidekick answers</p>
        <p className="text-[13px] text-[rgb(235_235_245/0.6)]">
          Apps, files and commands work already. Pick one to answer questions too; you can add more later.
        </p>
      </div>
      <div className="grid gap-1.5">
        {options.map((o) => (
          <div key={o.id} className={`ak-opt ${o.recommended && !o.done ? "rec" : ""}`}>
            <span className="oi">{o.title.slice(0, 1)}</span>
            <span className="min-w-0">
              <span className="ot">{o.title}</span>
              <span className="od">{o.note}</span>
            </span>
            <button type="button" disabled={o.done || running !== null} onClick={o.go} className="ob chip">
              {o.action}
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

/** "Offline" at the start of the context line while the internet is gone. */
export function useOffline(): boolean {
  return !useSidekick((s) => s.online);
}
