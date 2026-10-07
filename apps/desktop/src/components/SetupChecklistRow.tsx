"use client";

// One setup checklist row: status, Run / Add for me / Set up, and inline guides.

import { memo, type ReactNode, useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import { friendlyError } from "@/lib/friendly";
import { startWaiting, stopWaiting, useSidekick } from "@/lib/store";
import type { SetupItem } from "@/lib/types";
import { ItemGuide } from "./SetupGuides";

const TAB_LABELS: Record<string, string> = {
  ai: "AI",
  browser: "Browser",
  general: "General",
  search: "Search",
  today: "Today",
  voice: "Voice",
  connections: "Apps",
  privacy: "Privacy",
};

const ONE_CLICK = new Set([
  "claude_hooks",
  "claude_mcp",
  "codex_notify",
  "codex_mcp",
  "browser",
  "code_folders",
  "search_folders",
  "voice",
  "composio",
]);

function oneClickLabel(id: string): string {
  if (id === "claude_hooks" || id === "claude_mcp" || id === "codex_notify" || id === "codex_mcp") return "Add for me";
  if (id === "composio") return "Connect";
  return "Set up";
}

/** Memoized: re-renders only when this row's own data or open state changes. */
export const SetupRow = memo(function SetupRow({
  item,
  onRun,
  onOpenTab,
  inlineGuides,
  guideOpen,
  onToggleGuide,
  onDone,
  checking = false,
  children,
}: {
  item: SetupItem;
  onRun: (id: string) => void;
  onOpenTab?: (tab: string) => void;
  inlineGuides?: boolean;
  guideOpen: boolean;
  onToggleGuide: (id: string) => void;
  onDone: () => void;
  /** Pulse glyph while probes are in flight. */
  checking?: boolean;
  /** Shown under the row, done or not (the Composio row's apps). */
  children?: ReactNode;
}) {
  const [busy, setBusy] = useState(false);
  const waiting = useSidekick((s) => s.waiting);
  // App connects (app:notion) highlight the Composio row — that is the island done ring.
  const justDone = useSidekick(
    (s) => s.justDone === item.id || (item.id === "composio" && (s.justDone?.startsWith("app:") ?? false)),
  );
  // When Settings reopens after a step finished, show that step.
  const rowRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (justDone) rowRef.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [justDone]);
  const [actionError, setActionError] = useState<string | null>(null);
  // The command is there for those who want it, not the first thing you see.
  const [showCommand, setShowCommand] = useState(false);
  const multiline = item.command?.includes("\n") ?? false;
  const oneClick = ONE_CLICK.has(item.id);
  const needsGuide = (Boolean(item.tab) && !item.runnable) || oneClick;
  const showInline = Boolean(inlineGuides && needsGuide);
  const showOpenTab = Boolean(!inlineGuides && needsGuide && onOpenTab && item.tab);
  const showRun = Boolean(item.runnable && item.id !== "claude_hooks");

  const runDirect = () => {
    if (item.id === "browser" || item.id === "code_folders" || item.id === "search_folders" || item.id === "voice") {
      onToggleGuide(item.id);
      return;
    }
    setBusy(true);
    setActionError(null);
    const work =
      item.id === "claude_hooks"
        ? api.claudeAddHooks().then(() => undefined)
        : item.id === "claude_mcp"
          ? api.claudeAddMcp().catch(() => api.setupRun("claude_mcp"))
          : item.id === "codex_notify"
            ? api.codexAddNotify().then(() => undefined)
            : item.id === "codex_mcp"
              ? api.codexAddMcp().then(() => undefined)
              : api.composioSignIn();
    const outside = item.id === "composio";
    void work
      .then(() => {
        onDone();
        // Finishing happens in the browser: the island keeps the steps until it connects.
        if (outside)
          startWaiting(item.id, item.title, {
            resumeTab: item.tab ?? "connections",
            steps: ["Composio opened in your browser.", "Sign in and press Allow."],
            again: runDirect,
          });
      })
      .catch((e) => setActionError(friendlyError(e)))
      .finally(() => setBusy(false));
  };
  const run = () => {
    onRun(item.id);
    // The island keeps the steps until setup status flips.
    startWaiting(item.id, item.title, {
      resumeTab: item.tab ?? "home",
      steps: item.opensApp
        ? [`Sidekick opened the ${item.title} app.`, "This finishes by itself when it is running."]
        : item.opensTerminal
          ? [
              "PowerShell is open. The command is already on the clipboard.",
              "Replace your-key with your API key, paste (Ctrl+V), press Enter.",
              "Come back here — Sidekick notices when the key is set. No restart needed.",
            ]
          : [
              `A PowerShell window is running: ${item.action} ${item.title}.`,
              "Leave it open and answer any prompts there.",
              "This finishes by itself when it is ready.",
            ],
      copies: item.command ? [{ label: "Copy the command", text: item.command }] : undefined,
      again: () => onRun(item.id),
      doneLine: item.opensApp ? `All set. ${item.title} is running.` : undefined,
    });
  };
  const isWaiting = waiting?.id === item.id && !item.done;

  const isDirect =
    item.id === "claude_hooks" ||
    item.id === "claude_mcp" ||
    item.id === "codex_notify" ||
    item.id === "codex_mcp" ||
    item.id === "composio";

  return (
    <div
      ref={rowRef}
      className={`flex flex-col gap-2 rounded-2xl bg-white/[0.06] px-3.5 py-3 ring-1 ring-inset transition-[box-shadow,background-color] duration-[250ms] ease-out hover:bg-white/[0.08] ${
        justDone ? "ring-[#30d158]/70" : "ring-transparent"
      }`}
    >
      <div className="flex items-center gap-3">
        <SetupStatusMark checking={checking} done={item.done} />
        <div className="min-w-0 flex-1">
          <p className="font-medium text-white">
            {item.title}{" "}
            {!(inlineGuides && item.id === "browser") && (
              <span
                className={`font-normal transition-opacity duration-300 ${checking ? "opacity-0" : "opacity-100"} ${
                  item.done ? "text-[#30d158]" : "text-[rgb(235_235_245/0.62)]"
                }`}
              >
                {item.status}
              </span>
            )}
          </p>
          <p className="text-[11.5px] leading-snug text-[rgb(235_235_245/0.62)]">{item.why}</p>
        </div>
        {isWaiting && (
          <div className="flex shrink-0 items-center gap-1.5">
            <span className="text-[12px] text-[rgb(235_235_245/0.6)]">Waiting…</span>
            <SmallButton onClick={() => (isDirect ? runDirect() : run())}>Open again</SmallButton>
            <SmallButton onClick={stopWaiting}>Cancel</SmallButton>
          </div>
        )}
        {!checking && !item.done && !isWaiting && (
          <div className="flex shrink-0 gap-1.5">
            {item.command && !oneClick && !item.opensTerminal && (
              <SmallButton onClick={() => setShowCommand((v) => !v)}>
                {showCommand ? "Hide command" : "Command"}
              </SmallButton>
            )}
            {showRun && (
              <SmallButton primary={!showInline && !isDirect} onClick={run}>
                {item.action}
              </SmallButton>
            )}
            {showInline && (
              <SmallButton
                primary
                disabled={busy}
                onClick={() => {
                  if (isDirect) runDirect();
                  else onToggleGuide(item.id);
                }}
              >
                {busy ? "…" : guideOpen ? "Hide" : oneClickLabel(item.id)}
              </SmallButton>
            )}
            {!showInline && isDirect && (
              <SmallButton primary disabled={busy} onClick={runDirect}>
                {busy ? "…" : oneClickLabel(item.id)}
              </SmallButton>
            )}
            {showOpenTab && !isDirect && (
              <SmallButton primary={!item.command} onClick={() => onOpenTab?.(item.tab as string)}>
                {TAB_LABELS[item.tab as string] ? `Open ${TAB_LABELS[item.tab as string]}` : "Open"}
              </SmallButton>
            )}
            {showOpenTab && isDirect && onOpenTab && item.tab && (
              <SmallButton onClick={() => onOpenTab(item.tab as string)}>
                {TAB_LABELS[item.tab] ? `Open ${TAB_LABELS[item.tab]}` : "Open"}
              </SmallButton>
            )}
          </div>
        )}
      </div>
      {actionError && <p className="text-[11px] text-[#ff453a]">{actionError}</p>}
      {!checking && !item.done && item.command && !oneClick && !item.opensTerminal && showCommand && (
        <div className="flex flex-col gap-1.5">
          <p className="text-[11px] leading-snug text-[rgb(235_235_245/0.62)]">
            Paste in PowerShell or Terminal. Swap in your key, press Enter, then come back here.
          </p>
          <div className="flex items-center gap-1.5">
            <div className="min-w-0 flex-1">
              <Code text={multiline ? item.command.split("\n")[0] : item.command} />
            </div>
            <CopyButton text={item.command} />
          </div>
        </div>
      )}
      {!checking && !item.done && showInline && guideOpen && <ItemGuide id={item.id} onDone={onDone} />}
      {children}
    </div>
  );
});

function Code({ text }: { text: string }) {
  return (
    <code
      title={text}
      className="block truncate rounded-lg bg-black/30 px-2.5 py-1.5 font-mono text-[11px] text-white/80 select-all"
    >
      {text}
    </code>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be refused; the command is selectable anyway.
    }
  };
  return <SmallButton onClick={() => void copy()}>{copied ? "Copied" : "Copy"}</SmallButton>;
}

export function SmallButton({
  children,
  onClick,
  disabled,
  primary,
}: {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  primary?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`chip inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[12px] font-medium disabled:opacity-50 ${
        primary ? "bg-white text-black hover:bg-white/90" : "bg-white/12 text-white/90 hover:bg-white/20"
      }`}
    >
      {children}
    </button>
  );
}

/** Tiny ring for Check again / other short waits. */
export function SetupSpinner({ className = "" }: { className?: string }) {
  return (
    <span
      aria-hidden="true"
      className={`size-3 shrink-0 animate-spin rounded-full border border-current border-r-transparent ${className}`}
    />
  );
}

/** One status circle for rows and the folded "ready" bar — checking pulses. */
export function SetupStatusMark({ checking, done }: { checking: boolean; done: boolean }) {
  return (
    <span
      role="img"
      aria-label={checking ? "Checking" : done ? "Done" : "Not done"}
      className={`grid size-[18px] shrink-0 place-items-center rounded-full text-[10px] font-bold transition-colors duration-300 ${
        checking ? "animate-pulse bg-white/15" : done ? "bg-[#30d158] text-black" : "ring-1 ring-white/30 ring-inset"
      }`}
    >
      {!checking && done ? "✓" : ""}
    </span>
  );
}
